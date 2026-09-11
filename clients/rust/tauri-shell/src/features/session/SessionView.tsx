import { useState, useEffect, useRef, useCallback } from "react";
import { SessionCanvas } from "./SessionCanvas";
import {
  pollFrameRaw,
  sendInput,
  agentReleaseAll,
  type InputPayload,
} from "@/lib/ipc";
import {
  createHeldInputTracker,
  createOverlayState,
  shouldForwardKeyboardEvent,
  type MouseButton,
} from "@/lib/overlay";
import type { ConnectionInstance, ConnectionSnapshot } from "@/lib/connection";

export interface SessionViewProps {
  connection?: ConnectionInstance;
  snapshot?: ConnectionSnapshot;
  state?: ConnectionSnapshot;
  pollFrame?: () => Promise<ArrayBuffer | null>;
  onDisconnect?: () => void;
  onHome?: () => void;
  onError?: (message: string) => void;
}

function getDomElement(id: string): HTMLElement | null {
  if (typeof document === "undefined" || typeof document.getElementById !== "function") {
    return null;
  }
  return document.getElementById(id);
}

function mapMouseButton(button: number): MouseButton | null {
  if (button === 0) return "left";
  if (button === 1) return "middle";
  if (button === 2) return "right";
  return null;
}

function mouseButtonEventType(button: MouseButton, isDown: boolean): string {
  if (button === "left") return isDown ? "LeftMouseDown" : "LeftMouseUp";
  if (button === "middle") return isDown ? "MiddleMouseDown" : "MiddleMouseUp";
  if (button === "right") return isDown ? "RightMouseDown" : "RightMouseUp";
  return "";
}

export function SessionView({
  connection,
  snapshot: propSnapshot,
  state: propState,
  pollFrame: propPollFrame,
  onDisconnect,
  onHome,
  onError,
}: SessionViewProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);

  // Derive initial snapshot from prop or connection instance
  const [snapshot, setSnapshot] = useState<ConnectionSnapshot>(() => {
    return (
      propSnapshot ??
      propState ??
      connection?.snapshot() ?? {
        phase: "streaming",
        host: null,
        fieldErrors: {},
        error: null,
        cleanupError: null,
        stats: null,
        statsStatus: "idle",
        statsError: null,
        generation: 0,
        busy: false,
      }
    );
  });

  // Sync snapshot from connection instance or prop changes
  useEffect(() => {
    if (!connection) return;
    setSnapshot(connection.snapshot());
    return connection.subscribe(setSnapshot);
  }, [connection]);

  useEffect(() => {
    const next = propSnapshot ?? propState;
    if (next) {
      setSnapshot(next);
    }
  }, [propSnapshot, propState]);

  // Overlay state ported model from @/lib/overlay
  const overlayStateRef = useRef(createOverlayState());
  const [expanded, setExpanded] = useState(() => overlayStateRef.current.getExpanded());
  const [fullscreenActive, setFullscreenActive] = useState(() =>
    overlayStateRef.current.getFullscreenActive()
  );

  useEffect(() => {
    return overlayStateRef.current.subscribe((snap) => {
      setExpanded(snap.expanded);
      setFullscreenActive(snap.fullscreenActive);
    });
  }, []);

  // Stats refresh cadence (500ms like legacy)
  useEffect(() => {
    if (!connection) return;
    const timer = setInterval(() => {
      connection.refreshStats().catch(() => {});
    }, 500);
    return () => clearInterval(timer);
  }, [connection]);

  // Track FPS calculation locally based on polled frames
  const frameCountRef = useRef(0);
  const lastFpsCalcTimeRef = useRef(
    typeof performance !== "undefined" ? performance.now() : Date.now()
  );
  const [fps, setFps] = useState<number | null>(null);

  const handlePollFrame = useCallback(async () => {
    if (propPollFrame) {
      return propPollFrame();
    }
    const buf = await pollFrameRaw();
    if (buf && buf.byteLength >= 16) {
      if (connection) {
        connection.markFrameRendered(connection.token());
      }
      frameCountRef.current++;
      const now = typeof performance !== "undefined" ? performance.now() : Date.now();
      const elapsed = now - lastFpsCalcTimeRef.current;
      if (elapsed >= 1000) {
        setFps(Math.round((frameCountRef.current * 1000) / elapsed));
        frameCountRef.current = 0;
        lastFpsCalcTimeRef.current = now;
      }
    }
    return buf;
  }, [connection, propPollFrame]);

  // Input forwarding & held input tracking
  const heldInputsRef = useRef(createHeldInputTracker());
  const lastPointerRef = useRef({ x: 0, y: 0, viewWidth: 1280, viewHeight: 800 });
  const pendingPointerRef = useRef<InputPayload | null>(null);
  const pointerRafRef = useRef<number | null>(null);

  const isConnected = snapshot.phase === "streaming";
  const isConnectedRef = useRef(isConnected);
  useEffect(() => {
    isConnectedRef.current = isConnected;
  }, [isConnected]);

  // Pointer motion throttling (~16ms rAF queue)
  const flushPointerMotion = useCallback(() => {
    if (pointerRafRef.current !== null) {
      const cancel =
        typeof cancelAnimationFrame === "function"
          ? cancelAnimationFrame
          : clearTimeout;
      cancel(pointerRafRef.current);
      pointerRafRef.current = null;
    }
    const event = pendingPointerRef.current;
    pendingPointerRef.current = null;
    if (isConnectedRef.current && event) {
      sendInput(event).catch((err) => {
        console.error("send_input pointer error:", err);
      });
    }
  }, []);

  const sendPointerEvent = useCallback(
    (eventType: string, e: MouseEvent) => {
      if (!isConnectedRef.current) return;
      const isMotion =
        eventType === "MouseMove" ||
        eventType === "LeftMouseDragged" ||
        eventType === "RightMouseDragged";
      if (!isMotion) flushPointerMotion();

      const canvas = getDomElement("video-canvas") as HTMLCanvasElement | null;
      const rect = canvas?.getBoundingClientRect();
      if (!rect || rect.width <= 0 || rect.height <= 0) return;

      const canvasWidth = canvas?.width || 3840;
      const canvasHeight = canvas?.height || 1600;
      const videoAspect =
        canvasWidth > 0 && canvasHeight > 0 ? canvasWidth / canvasHeight : 16 / 9;
      const canvasAspect = rect.width / rect.height;
      let displayW = rect.width;
      let displayH = rect.height;
      let offsetX = 0;
      let offsetY = 0;

      if (canvasAspect > videoAspect) {
        displayH = rect.height;
        displayW = displayH * videoAspect;
        offsetX = (rect.width - displayW) / 2;
      } else {
        displayW = rect.width;
        displayH = displayW / videoAspect;
        offsetY = (rect.height - displayH) / 2;
      }

      const rawX = e.clientX - rect.left - offsetX;
      const rawY = e.clientY - rect.top - offsetY;
      const clampedX = Math.max(0, Math.min(rawX, displayW));
      const clampedY = Math.max(0, Math.min(rawY, displayH));

      lastPointerRef.current.x = clampedX;
      lastPointerRef.current.y = clampedY;
      lastPointerRef.current.viewWidth = displayW;
      lastPointerRef.current.viewHeight = displayH;

      let mod = 0;
      if (e.shiftKey) mod |= 1;
      if (e.ctrlKey) mod |= 2;
      if (e.altKey) mod |= 4;
      if (e.metaKey) mod |= 8;

      const event: InputPayload = {
        event_type: eventType,
        x: clampedX,
        y: clampedY,
        view_width: displayW,
        view_height: displayH,
        modifiers: mod,
        scroll_dx: 0.0,
        scroll_dy: 0.0,
      };

      if (isMotion) {
        pendingPointerRef.current = event;
        if (pointerRafRef.current === null) {
          const req =
            typeof requestAnimationFrame === "function"
              ? requestAnimationFrame
              : (cb: FrameRequestCallback) =>
                  setTimeout(() => cb(Date.now()), 16) as unknown as number;
          pointerRafRef.current = req(flushPointerMotion);
        }
        return;
      }

      sendInput(event).catch((err) => {
        console.error("send_input pointer error:", err);
      });
    },
    [flushPointerMotion]
  );

  const sendRelativePointerEvent = useCallback(
    (e: MouseEvent) => {
      if (!isConnectedRef.current) return;
      const dx = typeof e.movementX === "number" ? e.movementX : 0;
      const dy = typeof e.movementY === "number" ? e.movementY : 0;
      if (dx === 0 && dy === 0) return;

      let mod = 0;
      if (e.shiftKey) mod |= 1;
      if (e.ctrlKey) mod |= 2;
      if (e.altKey) mod |= 4;
      if (e.metaKey) mod |= 8;

      const event: InputPayload = {
        event_type: "RelativeMove",
        x: lastPointerRef.current.x,
        y: lastPointerRef.current.y,
        view_width: lastPointerRef.current.viewWidth || 1280,
        view_height: lastPointerRef.current.viewHeight || 800,
        modifiers: mod,
        scroll_dx: dx,
        scroll_dy: dy,
      };

      if (
        pendingPointerRef.current &&
        pendingPointerRef.current.event_type === "RelativeMove"
      ) {
        pendingPointerRef.current.scroll_dx =
          (pendingPointerRef.current.scroll_dx || 0) + dx;
        pendingPointerRef.current.scroll_dy =
          (pendingPointerRef.current.scroll_dy || 0) + dy;
        pendingPointerRef.current.modifiers = mod;
      } else {
        flushPointerMotion();
        pendingPointerRef.current = event;
        if (pointerRafRef.current === null) {
          const req =
            typeof requestAnimationFrame === "function"
              ? requestAnimationFrame
              : (cb: FrameRequestCallback) =>
                  setTimeout(() => cb(Date.now()), 16) as unknown as number;
          pointerRafRef.current = req(flushPointerMotion);
        }
      }
    },
    [flushPointerMotion]
  );

  // Release held input on attention switch or teardown
  const releaseHeldInputs = useCallback(async () => {
    flushPointerMotion();
    const canvas = getDomElement("video-canvas") as HTMLCanvasElement | null;
    const viewWidth = lastPointerRef.current.viewWidth || canvas?.clientWidth || 1280;
    const viewHeight = lastPointerRef.current.viewHeight || canvas?.clientHeight || 800;
    const events = heldInputsRef.current.releaseEvents(
      lastPointerRef.current.x,
      lastPointerRef.current.y,
      viewWidth,
      viewHeight
    );
    if (!events.length) return;
    for (const event of events) {
      try {
        await sendInput(event as InputPayload);
      } catch (err) {
        console.error("send_input release error:", err);
      }
    }
  }, [flushPointerMotion]);

  const releaseLocalInputs = useCallback(() => {
    if (!isConnectedRef.current) return;
    releaseHeldInputs().catch((err) => {
      console.error("releaseLocalInputs error:", err);
    });
  }, [releaseHeldInputs]);

  // Wire event listeners on viewport, window, and overlay
  useEffect(() => {
    const viewport = getDomElement("viewport");

    const handleMouseDown = (e: MouseEvent) => {
      if (!isConnectedRef.current) return;
      viewport?.focus();
      const button = mapMouseButton(e.button);
      if (!button) return;
      if (e.button === 1) e.preventDefault();
      heldInputsRef.current.mouseDown(button);
      sendPointerEvent(mouseButtonEventType(button, true), e);
    };

    const handleMouseMove = (e: MouseEvent) => {
      if (typeof document !== "undefined" && document.pointerLockElement) {
        sendRelativePointerEvent(e);
      } else if (heldInputsRef.current.isButtonDown("left")) {
        sendPointerEvent("LeftMouseDragged", e);
      } else if (heldInputsRef.current.isButtonDown("right")) {
        sendPointerEvent("RightMouseDragged", e);
      } else {
        sendPointerEvent("MouseMove", e);
      }
    };

    const handleContextMenu = (e: MouseEvent) => {
      e.preventDefault();
    };

    const handleWheel = (e: WheelEvent) => {
      if (!isConnectedRef.current) return;
      e.preventDefault();
      const canvas = getDomElement("video-canvas") as HTMLCanvasElement | null;
      const rect = canvas?.getBoundingClientRect();
      const rawX = rect ? Math.max(0, Math.min(e.clientX - rect.left, rect.width)) : 0;
      const rawY = rect ? Math.max(0, Math.min(e.clientY - rect.top, rect.height)) : 0;

      sendInput({
        event_type: "ScrollWheel",
        x: rawX,
        y: rawY,
        view_width: rect?.width || 1280,
        view_height: rect?.height || 800,
        modifiers: 0,
        scroll_dx: e.deltaX,
        scroll_dy: e.deltaY,
      }).catch((err) => {
        console.error("send_input wheel error:", err);
      });
    };

    const handleWindowMouseUp = (e: MouseEvent) => {
      const button = mapMouseButton(e.button);
      if (!button) return;
      if (heldInputsRef.current.isButtonDown(button)) {
        heldInputsRef.current.mouseUp(button);
        sendPointerEvent(mouseButtonEventType(button, false), e);
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as { tagName?: unknown; id?: unknown } | null;
      if (!shouldForwardKeyboardEvent({ target })) {
        if (e.key === "Escape" && overlayStateRef.current.getExpanded()) {
          overlayStateRef.current.setExpanded(false);
          const btnExpand = getDomElement("btn-expand");
          btnExpand?.focus();
        }
        return;
      }
      if (!isConnectedRef.current) return;
      if (e.repeat) return;

      let mod = 0;
      if (e.shiftKey) mod |= 1;
      if (e.ctrlKey) mod |= 2;
      if (e.altKey) mod |= 4;
      if (e.metaKey) mod |= 8;

      const canvas = getDomElement("video-canvas") as HTMLCanvasElement | null;
      heldInputsRef.current.keyDown(e.keyCode, mod);
      sendInput({
        event_type: "KeyDown",
        key_code: e.keyCode,
        modifiers: mod,
        view_width: canvas?.clientWidth || 1280,
        view_height: canvas?.clientHeight || 800,
      }).catch((err) => {
        console.error("send_input keydown error:", err);
      });
    };

    const handleKeyUp = (e: KeyboardEvent) => {
      const target = e.target as { tagName?: unknown; id?: unknown } | null;
      if (!shouldForwardKeyboardEvent({ target })) return;
      if (!isConnectedRef.current) return;

      let mod = 0;
      if (e.shiftKey) mod |= 1;
      if (e.ctrlKey) mod |= 2;
      if (e.altKey) mod |= 4;
      if (e.metaKey) mod |= 8;

      const canvas = getDomElement("video-canvas") as HTMLCanvasElement | null;
      heldInputsRef.current.keyUp(e.keyCode);
      sendInput({
        event_type: "KeyUp",
        key_code: e.keyCode,
        modifiers: mod,
        view_width: canvas?.clientWidth || 1280,
        view_height: canvas?.clientHeight || 800,
      }).catch((err) => {
        console.error("send_input keyup error:", err);
      });
    };

    const handleBlur = () => {
      if (typeof document !== "undefined" && document.pointerLockElement) {
        try {
          document.exitPointerLock();
        } catch (_) {}
      }
      releaseLocalInputs();
    };

    const handleVisibilityChange = () => {
      if (typeof document !== "undefined" && document.hidden) {
        if (document.pointerLockElement) {
          try {
            document.exitPointerLock();
          } catch (_) {}
        }
        releaseLocalInputs();
      }
    };

    const handleFullscreenChange = () => {
      if (typeof document !== "undefined") {
        overlayStateRef.current.setFullscreenActive(Boolean(document.fullscreenElement));
      }
    };

    const overlay = getDomElement("session-overlay");

    const handleOverlayPointerDown = (e: PointerEvent) => {
      const target = e.target as HTMLElement | null;
      if (
        target &&
        (target.closest("#btn-home") || target.closest("#btn-disconnect"))
      ) {
        return;
      }
      releaseLocalInputs();
    };

    const handleOverlayFocusIn = () => {
      releaseLocalInputs();
    };

    if (viewport) {
      viewport.addEventListener("mousedown", handleMouseDown);
      viewport.addEventListener("mousemove", handleMouseMove);
      viewport.addEventListener("contextmenu", handleContextMenu);
      viewport.addEventListener("wheel", handleWheel, { passive: false });
    }

    if (overlay) {
      overlay.addEventListener("pointerdown", handleOverlayPointerDown);
      overlay.addEventListener("focusin", handleOverlayFocusIn);
    }

    if (typeof window !== "undefined") {
      window.addEventListener("mouseup", handleWindowMouseUp);
      window.addEventListener("keydown", handleKeyDown);
      window.addEventListener("keyup", handleKeyUp);
      window.addEventListener("blur", handleBlur);
    }

    if (typeof document !== "undefined") {
      document.addEventListener("visibilitychange", handleVisibilityChange);
      document.addEventListener("fullscreenchange", handleFullscreenChange);
    }

    return () => {
      if (viewport) {
        viewport.removeEventListener("mousedown", handleMouseDown);
        viewport.removeEventListener("mousemove", handleMouseMove);
        viewport.removeEventListener("contextmenu", handleContextMenu);
        viewport.removeEventListener("wheel", handleWheel);
      }
      if (overlay) {
        overlay.removeEventListener("pointerdown", handleOverlayPointerDown);
        overlay.removeEventListener("focusin", handleOverlayFocusIn);
      }
      if (typeof window !== "undefined") {
        window.removeEventListener("mouseup", handleWindowMouseUp);
        window.removeEventListener("keydown", handleKeyDown);
        window.removeEventListener("keyup", handleKeyUp);
        window.removeEventListener("blur", handleBlur);
      }
      if (typeof document !== "undefined") {
        document.removeEventListener("visibilitychange", handleVisibilityChange);
        document.removeEventListener("fullscreenchange", handleFullscreenChange);
      }

      // Teardown: flush pending motion, release held inputs, and invoke host release-all
      flushPointerMotion();
      releaseHeldInputs().catch(() => {});
      agentReleaseAll().catch(() => {});
    };
  }, [
    sendPointerEvent,
    sendRelativePointerEvent,
    flushPointerMotion,
    releaseHeldInputs,
    releaseLocalInputs,
  ]);

  const handleHome = () => {
    if (onHome) {
      onHome();
    } else if (connection) {
      connection.disconnect();
    }
  };

  const handleDisconnect = () => {
    if (onDisconnect) {
      onDisconnect();
    } else if (connection) {
      connection.disconnect();
    }
  };

  const handleToggleExpand = () => {
    overlayStateRef.current.toggleExpanded();
  };

  const handleToggleFullscreen = async () => {
    try {
      if (typeof document !== "undefined" && document.fullscreenElement) {
        await document.exitFullscreen();
      } else if (containerRef.current) {
        await containerRef.current.requestFullscreen();
      }
    } catch (err) {
      console.error("Fullscreen toggle failed:", err);
    }
  };

  const hostName = snapshot.host?.name || snapshot.host?.ip || "Remote host";
  const ms = (val?: number | null) => (val == null ? "—" : `${val.toFixed(1)} ms`);

  const fpsText =
    fps != null && snapshot.phase === "streaming" ? `Rendered FPS ${fps}` : "Rendered FPS —";
  const latText =
    snapshot.stats?.latency_p50_ms != null
      ? `Decode p50 ${ms(snapshot.stats.latency_p50_ms)}`
      : "Decode p50 —";

  const showConnectingModal =
    snapshot.phase === "connecting" || snapshot.phase === "waiting-video";
  const isActive =
    snapshot.phase === "waiting-video" || snapshot.phase === "streaming";

  return (
    <div
      ref={containerRef}
      id="viewport-container"
      className="relative w-full h-full overflow-hidden bg-black select-none"
      data-phase={snapshot.phase}
    >
      {/* Floating session overlay reproducing legacy copy, ids, and controls */}
      <div
        id="session-overlay"
        data-ui-scope
        role="group"
        aria-label="Session controls"
        className="absolute top-4 left-1/2 -translate-x-1/2 z-[1100] flex flex-col items-center gap-2 pointer-events-none w-[min(40rem,calc(100%-32px))] max-w-[calc(100%-32px)]"
      >
        <div
          id="launcher-bar"
          className="launcher-bar flex items-center gap-2 w-full min-h-[48px] p-1 box-border rounded-[var(--radius-panel)] bg-[var(--card)] border border-[var(--border)] shadow-[var(--shadow-overlay)] pointer-events-auto"
        >
          <div className="overlay-badge flex items-center gap-2 pl-2 font-semibold min-w-[6rem] flex-1 text-sm text-[var(--foreground)]">
            <div className="status-dot w-2 h-2 rounded-full bg-[var(--success)] shrink-0" />
            <span id="session-host-name" className="truncate">
              {hostName}
            </span>
          </div>

          <div className="overlay-stats flex items-center gap-3 text-xs text-[var(--muted-foreground)] tabular-nums shrink-0">
            <span id="session-stat-fps">{fpsText}</span>
            <span id="session-stat-lat">{latText}</span>
          </div>

          <button
            type="button"
            id="btn-home"
            className="overlay-btn icon-only inline-flex items-center justify-center w-8 h-8 rounded-md hover:bg-[var(--secondary)] text-[var(--foreground)] transition-colors cursor-pointer shrink-0"
            title="Home — end session and return to main screen"
            aria-label="Home — end session and return to main screen"
            onClick={handleHome}
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
            >
              <path d="M3 9l9-7 9 7v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
              <polyline points="9 22 9 12 15 12 15 22" />
            </svg>
          </button>

          <button
            type="button"
            id="btn-expand"
            className="overlay-btn icon-only inline-flex items-center justify-center w-8 h-8 rounded-md hover:bg-[var(--secondary)] text-[var(--foreground)] transition-colors cursor-pointer shrink-0"
            aria-expanded={expanded}
            aria-controls="launcher-panel"
            title="More session controls"
            aria-label="More session controls"
            onClick={handleToggleExpand}
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              className={`transition-transform duration-200 ${expanded ? "rotate-180" : ""}`}
            >
              <polyline points="6 9 12 15 18 9" />
            </svg>
          </button>

          <button
            type="button"
            id="btn-disconnect"
            className="overlay-btn btn-disconnect inline-flex items-center gap-1.5 px-3 h-8 rounded-md text-xs font-medium text-[var(--destructive)] hover:bg-[var(--secondary)] transition-colors cursor-pointer shrink-0"
            title="Disconnect"
            onClick={handleDisconnect}
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
            >
              <path d="M18.36 6.64a9 9 0 1 1-12.73 0" />
              <line x1="12" y1="2" x2="12" y2="12" />
            </svg>
            Disconnect
          </button>
        </div>

        {expanded && (
          <div
            id="launcher-panel"
            role="region"
            aria-label="Session details"
            className="launcher-panel w-[320px] max-w-full max-h-[calc(100dvh-96px)] overflow-y-auto p-4 flex flex-col gap-3 rounded-[var(--radius-panel)] bg-[var(--card)] border border-[var(--border)] shadow-[var(--shadow-overlay)] pointer-events-auto text-sm text-[var(--foreground)]"
          >
            <div className="text-xs font-semibold uppercase tracking-wider text-[var(--muted-foreground)]">
              Session details
            </div>
            <p className="text-xs text-[var(--muted-foreground)]">
              Frames rendered by this client per second. Decode measures local
              receive-to-decode time, not network latency.
            </p>
            {snapshot.statsError && (
              <p
                id="session-stats-notice"
                role="status"
                className="text-xs text-[var(--destructive)]"
              >
                {snapshot.statsError}
              </p>
            )}
            <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-xs">
              <dt className="text-[var(--muted-foreground)]">State</dt>
              <dd id="session-stat-state" className="text-right font-mono">
                {snapshot.phase}
              </dd>
              <dt className="text-[var(--muted-foreground)]">Decode p50</dt>
              <dd id="session-stat-p50" className="text-right font-mono">
                {ms(snapshot.stats?.latency_p50_ms)}
              </dd>
              <dt className="text-[var(--muted-foreground)]">Decode p99</dt>
              <dd id="session-stat-p99" className="text-right font-mono">
                {ms(snapshot.stats?.latency_p99_ms)}
              </dd>
              <dt className="text-[var(--muted-foreground)]">Frames received</dt>
              <dd id="session-stat-frames" className="text-right font-mono">
                {snapshot.stats?.frames_received != null
                  ? String(snapshot.stats.frames_received)
                  : "—"}
              </dd>
              <dt className="text-[var(--muted-foreground)]">Frames decoded</dt>
              <dd id="session-stat-decoded" className="text-right font-mono">
                {snapshot.stats?.frames_decoded != null
                  ? String(snapshot.stats.frames_decoded)
                  : "—"}
              </dd>
              <dt className="text-[var(--muted-foreground)]">Audio packets</dt>
              <dd id="session-stat-audio" className="text-right font-mono">
                {snapshot.stats?.audio_packets_received != null
                  ? String(snapshot.stats.audio_packets_received)
                  : "—"}
              </dd>
            </dl>
            <div className="pt-2 border-t border-[var(--border)] flex gap-2">
              <button
                type="button"
                id="btn-fullscreen"
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-md text-xs font-medium border border-[var(--border)] hover:bg-[var(--secondary)] transition-colors cursor-pointer"
                aria-pressed={fullscreenActive}
                title="Toggle fullscreen"
                onClick={handleToggleFullscreen}
              >
                <svg
                  width="12"
                  height="12"
                  viewBox="0 0 24 24"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="2"
                >
                  <path d="M8 3H5a2 2 0 0 0-2 2v3m18 0V5a2 2 0 0 0-2-2h-3m0 18h3a2 2 0 0 0 2-2v-3M3 16v3a2 2 0 0 0 2 2h3" />
                </svg>
                <span id="btn-fullscreen-label">
                  {fullscreenActive ? "Exit Fullscreen" : "Fullscreen"}
                </span>
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Connecting modal displayed while connecting / waiting for video */}
      {showConnectingModal && (
        <div
          id="session-connecting-modal"
          data-ui-scope
          role="dialog"
          aria-modal="true"
          aria-labelledby="modal-connecting-text"
          className="absolute inset-0 z-[1200] flex items-center justify-center bg-black/60 backdrop-blur-sm pointer-events-auto"
        >
          <div className="connection-panel flex flex-col items-center gap-3 p-6 rounded-[var(--radius-panel)] bg-[var(--card)] border border-[var(--border)] shadow-[var(--shadow-overlay)] min-w-[280px]">
            <div
              className="spinner w-6 h-6 border-2 border-[var(--muted)] border-t-[var(--primary)] rounded-full animate-spin"
              aria-hidden="true"
            />
            <div
              id="modal-connecting-text"
              role="status"
              className="text-sm font-medium text-[var(--foreground)]"
            >
              {snapshot.phase === "waiting-video"
                ? "Waiting for video"
                : "Connecting"}
            </div>
            <p id="modal-host-name" className="text-xs text-[var(--muted-foreground)]">
              {snapshot.host?.name || snapshot.host?.ip || ""}
            </p>
            <button
              id="btn-cancel-connect"
              type="button"
              className="mt-2 px-4 py-1.5 rounded-md text-xs font-medium border border-[var(--border)] hover:bg-[var(--secondary)] text-[var(--foreground)] transition-colors cursor-pointer"
              onClick={handleDisconnect}
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Session canvas video renderer */}
      <SessionCanvas
        active={isActive}
        pollFrame={handlePollFrame}
        onError={onError}
      />
    </div>
  );
}
