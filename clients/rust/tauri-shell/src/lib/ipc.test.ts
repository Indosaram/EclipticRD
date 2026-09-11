import { describe, expect, it, beforeEach, afterEach, mock } from "bun:test";
import {
  ERD_COMMANDS,
  isNativeAvailable,
  invokeCommand,
  listHosts,
  connect,
  getCursorPosition,
  setBitrate,
  pollFrameRaw,
  disconnect,
  audioStatus,
  listAudioDevices,
  setAudioVolume,
  setAudioMuted,
  setAudioDevice,
  listPairings,
  forgetPairing,
  stats,
  sendInput,
  agentExecuteAction,
  agentGetScreenInfo,
  agentCaptureScreen,
  agentReleaseAll,
  getHostStatus,
  startHost,
  stopHost,
} from "./ipc";
import type {
  HostItem,
  PairingSummary,
  SessionStats,
  HostStatus,
  CursorState,
  ScreenInfo,
  DesktopAudioStatus,
  InputPayload,
  AgentAction,
} from "./ipc";

const ALL_22_COMMANDS = [
  "list_hosts",
  "connect",
  "get_cursor_position",
  "set_bitrate",
  "poll_frame_raw",
  "disconnect",
  "audio_status",
  "list_audio_devices",
  "set_audio_volume",
  "set_audio_muted",
  "set_audio_device",
  "list_pairings",
  "forget_pairing",
  "stats",
  "send_input",
  "agent_execute_action",
  "agent_get_screen_info",
  "agent_capture_screen",
  "agent_release_all",
  "get_host_status",
  "start_host",
  "stop_host",
] as const;

const DYNAMIC_7_COMMANDS = [
  "start_host",
  "stop_host",
  "audio_status",
  "list_audio_devices",
  "set_audio_volume",
  "set_audio_muted",
  "set_audio_device",
] as const;

describe("ERD_COMMANDS registry", () => {
  it("has exact length of 22 commands", () => {
    expect(ERD_COMMANDS.length).toBe(22);
  });

  it("contains all 22 authoritative backend commands", () => {
    for (const cmd of ALL_22_COMMANDS) {
      expect(ERD_COMMANDS).toContain(cmd);
    }
  });

  it("contains all 7 dynamically invoked commands", () => {
    for (const cmd of DYNAMIC_7_COMMANDS) {
      expect(ERD_COMMANDS).toContain(cmd);
    }
  });
});

describe("isNativeAvailable", () => {
  const originalTauri = (globalThis as any).window?.__TAURI__;

  afterEach(() => {
    if (typeof (globalThis as any).window !== "undefined") {
      if (originalTauri !== undefined) {
        (globalThis as any).window.__TAURI__ = originalTauri;
      } else {
        delete (globalThis as any).window.__TAURI__;
      }
    }
  });

  it("returns false when window.__TAURI__ is not present", () => {
    if (typeof (globalThis as any).window === "undefined") {
      (globalThis as any).window = {};
    }
    delete (globalThis as any).window.__TAURI__;
    expect(isNativeAvailable()).toBe(false);
  });

  it("returns true when window.__TAURI__ exists", () => {
    if (typeof (globalThis as any).window === "undefined") {
      (globalThis as any).window = {};
    }
    (globalThis as any).window.__TAURI__ = {};
    expect(isNativeAvailable()).toBe(true);
  });
});

describe("invokeCommand", () => {
  afterEach(() => {
    if (typeof (globalThis as any).window !== "undefined") {
      delete (globalThis as any).window.__TAURI__;
    }
  });

  it("throws clear error when Tauri is not available", async () => {
    if (typeof (globalThis as any).window !== "undefined") {
      delete (globalThis as any).window.__TAURI__;
    }
    expect(invokeCommand("list_hosts")).rejects.toThrow(
      /Tauri IPC is not available/
    );
  });

  it("dispatches to window.__TAURI__.core.invoke", async () => {
    const mockInvoke = mock(async (cmd: string, args?: Record<string, unknown>) => {
      return { echoed: cmd, args };
    });
    (globalThis as any).window = {
      __TAURI__: {
        core: {
          invoke: mockInvoke,
        },
      },
    };

    const res = await invokeCommand<{ echoed: string; args?: any }>("list_hosts", {
      test: 123,
    });
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(mockInvoke).toHaveBeenCalledWith("list_hosts", { test: 123 });
    expect(res).toEqual({ echoed: "list_hosts", args: { test: 123 } });
  });

  it("falls back to window.__TAURI__.tauri", async () => {
    const mockInvoke = mock(async (cmd: string, args?: Record<string, unknown>) => {
      return { fallback: cmd, args };
    });
    (globalThis as any).window = {
      __TAURI__: {
        tauri: mockInvoke,
      },
    };

    const res = await invokeCommand<{ fallback: string; args?: any }>("stats");
    expect(mockInvoke).toHaveBeenCalledTimes(1);
    expect(mockInvoke).toHaveBeenCalledWith("stats", undefined);
    expect(res).toEqual({ fallback: "stats", args: undefined });
  });
});

describe("named thin wrappers", () => {
  let invoked: { cmd: string; args?: Record<string, unknown> }[] = [];

  beforeEach(() => {
    invoked = [];
    (globalThis as any).window = {
      __TAURI__: {
        core: {
          invoke: async (cmd: string, args?: Record<string, unknown>) => {
            invoked.push({ cmd, args });
            return "ok";
          },
        },
      },
    };
  });

  afterEach(() => {
    delete (globalThis as any).window.__TAURI__;
  });

  it("calls each thin wrapper and dispatches expected command and args", async () => {
    await listHosts();
    expect(invoked[invoked.length - 1]).toEqual({ cmd: "list_hosts", args: undefined });

    await connect({ host: "10.0.0.1", tcpPort: 19730 });
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "connect",
      args: { host: "10.0.0.1", tcpPort: 19730 },
    });

    await getCursorPosition();
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "get_cursor_position",
      args: undefined,
    });

    await setBitrate(50);
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "set_bitrate",
      args: { bitrateMbps: 50 },
    });

    await pollFrameRaw();
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "poll_frame_raw",
      args: undefined,
    });

    await disconnect();
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "disconnect",
      args: undefined,
    });

    await audioStatus();
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "audio_status",
      args: undefined,
    });

    await listAudioDevices();
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "list_audio_devices",
      args: undefined,
    });

    await setAudioVolume(0.8);
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "set_audio_volume",
      args: { volume: 0.8 },
    });

    await setAudioMuted(true);
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "set_audio_muted",
      args: { muted: true },
    });

    await setAudioDevice("output-0");
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "set_audio_device",
      args: { deviceId: "output-0" },
    });

    await listPairings();
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "list_pairings",
      args: undefined,
    });

    await forgetPairing("pair-123");
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "forget_pairing",
      args: { id: "pair-123" },
    });

    await stats();
    expect(invoked[invoked.length - 1]).toEqual({ cmd: "stats", args: undefined });

    const inputEvt: InputPayload = { event_type: "MouseMove", x: 0.5, y: 0.5 };
    await sendInput(inputEvt);
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "send_input",
      args: { event: inputEvt as any },
    });

    const agentAction: AgentAction = { action: "mouse_move", x: 0.5, y: 0.5 };
    await agentExecuteAction(agentAction);
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "agent_execute_action",
      args: { action: agentAction as any },
    });

    await agentGetScreenInfo();
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "agent_get_screen_info",
      args: undefined,
    });

    await agentCaptureScreen("jpeg");
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "agent_capture_screen",
      args: { format: "jpeg" },
    });

    await agentReleaseAll();
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "agent_release_all",
      args: undefined,
    });

    await getHostStatus();
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "get_host_status",
      args: undefined,
    });

    await startHost();
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "start_host",
      args: undefined,
    });

    await stopHost();
    expect(invoked[invoked.length - 1]).toEqual({
      cmd: "stop_host",
      args: undefined,
    });
  });
});
