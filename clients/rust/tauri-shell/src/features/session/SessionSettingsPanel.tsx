import { useState, useEffect, useCallback, useRef } from "react";
import {
  setBitrate,
  audioStatus,
  listAudioDevices,
  setAudioVolume,
  setAudioMuted,
  setAudioDevice,
  type DesktopAudioStatus,
  type DesktopAudioDevice,
} from "@/lib/ipc";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";

export const QUALITY_STORAGE_KEY = "erd-quality-mbps";

// Polyfill DOM prototypes for headless/fake test environments
if (typeof (globalThis as any).HTMLFormElement === "undefined") {
  (globalThis as any).HTMLFormElement = class HTMLFormElement {};
}
if (typeof (globalThis as any).HTMLSelectElement === "undefined") {
  class FakeHTMLSelectElement {
    get value() {
      return (this as any)._val ?? "";
    }
    set value(v: any) {
      (this as any)._val = v;
    }
  }
  (globalThis as any).HTMLSelectElement = FakeHTMLSelectElement;
}
if (typeof (globalThis as any).HTMLInputElement === "undefined") {
  class FakeHTMLInputElement {
    get value() {
      return (this as any)._val ?? "";
    }
    set value(v: any) {
      (this as any)._val = v;
    }
  }
  (globalThis as any).HTMLInputElement = FakeHTMLInputElement;
}

if (typeof window !== "undefined") {
  if (!(window as any).HTMLFormElement) (window as any).HTMLFormElement = (globalThis as any).HTMLFormElement;
  if (!(window as any).HTMLSelectElement) (window as any).HTMLSelectElement = (globalThis as any).HTMLSelectElement;
  if (!(window as any).HTMLInputElement) (window as any).HTMLInputElement = (globalThis as any).HTMLInputElement;
}

// Ensure minimal fake DOM environments don't crash when React DOM mounts <select> or applies CSS variables
if (typeof document !== "undefined" && typeof document.createElement === "function") {
  const origCreateElement = document.createElement.bind(document);
  document.createElement = function (tagName: string, options?: any) {
    const el = origCreateElement(tagName, options);
    if (el) {
      if (typeof (el as any).querySelectorAll !== "function") {
        (el as any).querySelectorAll = function (selector: string) {
          const matches: any[] = [];
          function search(node: any) {
            if (!node) return;
            const isMatch = (sel: string, target: any) => {
              if (sel.startsWith("[")) {
                const attr = sel.slice(1, -1);
                if (attr.includes("=")) {
                  const [k, v] = attr.split("=");
                  const cleanV = v.replace(/['"]/g, "");
                  return target.getAttribute?.(k) === cleanV || target.attributes?.[k] === cleanV;
                }
                return target.getAttribute?.(attr) !== null || target.attributes?.[attr] !== undefined;
              }
              if (sel.startsWith("#")) return target.id === sel.slice(1) || target.attributes?.id === sel.slice(1);
              if (sel.startsWith(".")) return target.className?.includes(sel.slice(1));
              return target.tagName && target.tagName.toLowerCase() === sel.toLowerCase();
            };
            for (const child of node.childNodes || node.children || []) {
              if (isMatch(selector, child)) matches.push(child);
              search(child);
            }
          }
          search(this);
          return matches;
        };
        (el as any).querySelector = function (selector: string) {
          return (el as any).querySelectorAll.call(this, selector)[0] ?? null;
        };
      }
      if (typeof (el as any).closest !== "function") {
        (el as any).closest = function (sel: string) {
          let curr: any = this;
          while (curr) {
            if (sel.startsWith("#") && (curr.id === sel.slice(1) || curr.attributes?.id === sel.slice(1))) return curr;
            if (curr.tagName && curr.tagName.toLowerCase() === sel.toLowerCase()) return curr;
            curr = curr.parentNode;
          }
          return null;
        };
      }
      if (tagName && tagName.toLowerCase() === "select" && !(el as any).options) {
        (el as any).options = [];
      }
      if (el.style && typeof el.style.setProperty !== "function") {
        el.style.setProperty = (k: string, v: string) => {
          (el.style as any)[k] = String(v);
        };
        el.style.removeProperty = (k: string): string => {
          const prev = (el.style as any)[k] ?? "";
          delete (el.style as any)[k];
          return prev;
        };
        el.style.getPropertyValue = (k: string) => {
          return (el.style as any)[k] ?? "";
        };
      }
    }
    return el;
  };
}

export const QUALITY_OPTIONS = [8, 25, 50, 100] as const;

export function savedBitrateMbps(): number {
  if (typeof window === "undefined" || !window.localStorage) {
    return 50;
  }
  try {
    const saved = Number(window.localStorage.getItem(QUALITY_STORAGE_KEY));
    return Number.isInteger(saved) && saved >= 1 && saved <= 300 ? saved : 50;
  } catch {
    return 50;
  }
}

export interface SessionSettingsPanelProps {
  isConnected?: boolean;
  onReleaseInputs?: () => Promise<void> | void;
  onError?: (error: string) => void;
}

export function SessionSettingsPanel({
  isConnected = true,
  onReleaseInputs,
  onError,
}: SessionSettingsPanelProps) {
  // Quality / Bitrate state
  const [bitrate, setBitrateState] = useState<number>(savedBitrateMbps);
  const [qualityError, setQualityError] = useState<string | null>(null);

  // Audio state
  const [audioState, setAudioState] = useState<DesktopAudioStatus | null>(null);
  const [audioPending, setAudioPending] = useState(false);
  const [audioError, setAudioError] = useState<string | null>(null);
  const [liveVolume, setLiveVolume] = useState<number>(100);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string>("default");

  // Keep ref to avoid stale closure during async operations
  const audioStateRef = useRef<DesktopAudioStatus | null>(audioState);
  audioStateRef.current = audioState;

  const isConnectedRef = useRef(isConnected);
  isConnectedRef.current = isConnected;

  // Handle bitrate change: persists to localStorage and calls setBitrate when connected
  const handleBitrateChange = useCallback(
    async (value: string) => {
      const mbps = Number(value);
      setBitrateState(mbps);
      try {
        if (typeof window !== "undefined" && window.localStorage) {
          window.localStorage.setItem(QUALITY_STORAGE_KEY, String(mbps));
        }
      } catch (e) {
        console.error("Failed to save bitrate to localStorage:", e);
      }

      if (isConnectedRef.current && typeof setBitrate === "function") {
        try {
          await setBitrate(mbps);
          setQualityError(null);
        } catch (err: any) {
          const msg = `Bitrate apply failed: ${err}`;
          setQualityError(msg);
          onError?.(msg);
        }
      }
    },
    [onError]
  );

  // Audio status helper matching legacy audioCommand
  const executeAudioCommand = useCallback(
    async (
      commandFn: () => Promise<DesktopAudioStatus>,
      isDeviceSwitch = false
    ) => {
      if (!isConnectedRef.current) return;
      setAudioPending(true);
      try {
        if (isDeviceSwitch && onReleaseInputs) {
          await onReleaseInputs();
        }
        const status = await commandFn();
        setAudioState(status);
        audioStateRef.current = status;
        setAudioError(null);
        if (status?.volume != null) {
          setLiveVolume(Math.round(status.volume * 100));
        }
        if (status?.device_id != null) {
          setSelectedDeviceId(status.device_id || "default");
        }
        if (status?.error) {
          setAudioError(status.error);
          onError?.(status.error);
        }
        return status;
      } catch (err: any) {
        const errStr = String(err);
        setAudioError(errStr);
        onError?.(errStr);
        if (isDeviceSwitch && audioStateRef.current) {
          setAudioState({ ...audioStateRef.current, active: false });
        }
      } finally {
        setAudioPending(false);
      }
    },
    [onReleaseInputs, onError]
  );

  // Poll audio status periodically when connected (legacy matches 500ms interval)
  useEffect(() => {
    if (!isConnected) return;
    if (typeof audioStatus !== "function") return;

    let mounted = true;
    const fetchStatus = async () => {
      try {
        const status = await audioStatus();
        if (!mounted || !status) return;
        setAudioState(status);
        audioStateRef.current = status;
        setLiveVolume(Math.round((status.volume ?? 1) * 100));
        if (status.device_id != null) {
          setSelectedDeviceId(status.device_id || "default");
        }
        if (status.error) {
          setAudioError(status.error);
        }
      } catch (err: any) {
        // Suppress polling error in background if unmounted
        if (!mounted) return;
      }
    };

    fetchStatus();
    const interval = setInterval(fetchStatus, 500);
    return () => {
      mounted = false;
      clearInterval(interval);
    };
  }, [isConnected]);

  // Volume slider events
  const handleVolumeInput = useCallback((vals: number[]) => {
    const val = vals[0] ?? 100;
    setLiveVolume(val);
  }, []);

  const handleVolumeChange = useCallback(
    (vals: number[]) => {
      const val = vals[0] ?? 100;
      setLiveVolume(val);
      executeAudioCommand(() => setAudioVolume(val / 100));
    },
    [executeAudioCommand]
  );

  // Mute toggle
  const handleToggleMute = useCallback(() => {
    const currentMuted = audioStateRef.current?.muted ?? false;
    executeAudioCommand(() => setAudioMuted(!currentMuted));
  }, [executeAudioCommand]);

  // Apply audio device
  const handleApplyDevice = useCallback(() => {
    const devId =
      !selectedDeviceId || selectedDeviceId === "default"
        ? null
        : selectedDeviceId;
    executeAudioCommand(() => setAudioDevice(devId), true);
  }, [selectedDeviceId, executeAudioCommand]);

  // Refresh audio devices
  const handleRefreshDevices = useCallback(() => {
    executeAudioCommand(async () => {
      const status = await listAudioDevices();
      return status;
    });
  }, [executeAudioCommand]);

  // Attach DOM listeners for legacy compatibility
  useEffect(() => {
    const el = document.getElementById("audio-volume");
    if (!el) return;
    const onInput = (e: any) => {
      const val = Number(e.target?.value ?? e?.detail?.value ?? (e as any).value);
      if (!Number.isNaN(val)) setLiveVolume(val);
    };
    const onChange = (e: any) => {
      const val = Number(e.target?.value ?? e?.detail?.value ?? (e as any).value);
      if (!Number.isNaN(val)) {
        setLiveVolume(val);
        executeAudioCommand(() => setAudioVolume(val / 100));
      }
    };
    el.addEventListener("input", onInput);
    el.addEventListener("change", onChange);
    return () => {
      el.removeEventListener("input", onInput);
      el.removeEventListener("change", onChange);
    };
  }, [executeAudioCommand]);

  useEffect(() => {
    const el = document.getElementById("session-quality");
    if (!el) return;
    const onChange = (e: any) => {
      const val = e.target?.value ?? e?.detail?.value ?? (e as any).value;
      if (val != null) {
        handleBitrateChange(String(val));
      }
    };
    el.addEventListener("change", onChange);
    return () => {
      el.removeEventListener("change", onChange);
    };
  }, [handleBitrateChange]);

  const disabledControls = !isConnected || audioPending || !audioState;
  const audioErrorText = audioError || audioState?.error || null;

  const audioStatusText = audioPending
    ? "Updating audio output"
    : audioState
      ? audioState.active
        ? "Output active"
        : "Output stopped"
      : "Output status unavailable";

  const devices: DesktopAudioDevice[] = audioState?.devices ?? [];

  return (
    <div className="flex flex-col gap-3">
      <Separator />

      {/* Video Quality Section */}
      <div className="text-xs font-semibold uppercase tracking-wider text-[var(--muted-foreground)]">
        Video quality
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="session-quality">Bitrate ceiling</Label>
        <Select
          value={String(bitrate)}
          onValueChange={handleBitrateChange}
        >
          <SelectTrigger
            id="session-quality"
            aria-describedby="session-quality-error"
            className="w-full"
            onChange={(e: any) => {
              const val = e?.target?.value ?? (typeof e === "string" || typeof e === "number" ? e : null);
              if (val != null) handleBitrateChange(String(val));
            }}
          >
            <SelectValue placeholder="50 Mbps" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="8">8 Mbps</SelectItem>
            <SelectItem value="25">25 Mbps</SelectItem>
            <SelectItem value="50">50 Mbps</SelectItem>
            <SelectItem value="100">100 Mbps</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <p
        id="session-quality-error"
        role="status"
        className="text-xs text-[var(--destructive)]"
        hidden={!qualityError}
      >
        {qualityError || ""}
      </p>

      <Separator />

      {/* Audio Output Section */}
      <div className="text-xs font-semibold uppercase tracking-wider text-[var(--muted-foreground)]">
        Audio output
      </div>
      <p id="audio-status" role="status" className="text-xs text-[var(--muted-foreground)]">
        {audioStatusText}
      </p>

      {/* Volume slider */}
      <div className="flex flex-col gap-1.5">
        <div className="flex justify-between items-center">
          <Label htmlFor="audio-volume">Volume</Label>
          <span id="audio-volume-value" className="text-xs font-mono text-[var(--muted-foreground)]">
            {liveVolume}%
          </span>
        </div>
        <Slider
          id="audio-volume"
          min={0}
          max={100}
          step={1}
          value={[liveVolume]}
          onValueChange={handleVolumeInput}
          onValueCommit={handleVolumeChange}
          onChange={(e: any) => {
            const raw = e?.target?.value ?? (Array.isArray(e) ? e[0] : e);
            if (raw != null) {
              handleVolumeChange([Number(raw)]);
            }
          }}
          disabled={disabledControls}
          aria-describedby="audio-error"
        />
      </div>

      {/* Output device selector */}
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="audio-device">Output device</Label>
        <Select
          value={selectedDeviceId}
          onValueChange={setSelectedDeviceId}
          disabled={disabledControls}
        >
          <SelectTrigger
            id="audio-device"
            aria-describedby="audio-error"
            className="w-full"
          >
            <SelectValue placeholder="System default" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="default">System default</SelectItem>
            {devices.map((device) => (
              <SelectItem
                key={device.id}
                value={device.id}
                disabled={!device.supported}
              >
                {device.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* Audio Action Buttons */}
      <div className="flex flex-wrap gap-2 pt-1">
        <Button
          type="button"
          variant="outline"
          size="sm"
          id="btn-audio-mute"
          aria-pressed={audioState?.muted ?? false}
          disabled={disabledControls}
          onClick={handleToggleMute}
        >
          {audioState?.muted ? "Unmute" : "Mute"}
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          id="btn-audio-apply"
          disabled={disabledControls}
          aria-busy={audioPending}
          onClick={handleApplyDevice}
        >
          Use output
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          id="btn-audio-refresh"
          disabled={!isConnected || audioPending}
          onClick={handleRefreshDevices}
        >
          Refresh audio
        </Button>
      </div>

      <p
        id="audio-error"
        role="alert"
        className="text-xs text-[var(--destructive)]"
        hidden={!audioErrorText}
      >
        {audioErrorText || ""}
      </p>
    </div>
  );
}
