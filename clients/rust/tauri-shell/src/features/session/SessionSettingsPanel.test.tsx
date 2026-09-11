import { describe, it, expect, mock, beforeEach } from "bun:test";

// Track IPC calls
let setBitrateCalls: number[] = [];
let setAudioVolumeCalls: number[] = [];
let setAudioMutedCalls: boolean[] = [];
let setAudioDeviceCalls: (string | null)[] = [];
let listAudioDevicesCalls = 0;
let audioStatusCalls = 0;

let currentAudioStatus = {
  active: true,
  volume: 1.0,
  muted: false,
  device_id: null as string | null,
  devices: [
    { id: "dev-speaker", name: "Speakers", supported: true },
    { id: "dev-headphones", name: "Headphones", supported: true },
  ],
  consumed_samples: 500,
  error: null as string | null,
};

mock.module("@/lib/ipc", () => {
  return {
    setBitrate: async (bitrateMbps: number) => {
      setBitrateCalls.push(bitrateMbps);
    },
    audioStatus: async () => {
      audioStatusCalls++;
      return { ...currentAudioStatus };
    },
    listAudioDevices: async () => {
      listAudioDevicesCalls++;
      return { ...currentAudioStatus };
    },
    setAudioVolume: async (volume: number) => {
      setAudioVolumeCalls.push(volume);
      currentAudioStatus = { ...currentAudioStatus, volume };
      return { ...currentAudioStatus };
    },
    setAudioMuted: async (muted: boolean) => {
      setAudioMutedCalls.push(muted);
      currentAudioStatus = { ...currentAudioStatus, muted };
      return { ...currentAudioStatus };
    },
    setAudioDevice: async (deviceId: string | null) => {
      setAudioDeviceCalls.push(deviceId);
      currentAudioStatus = { ...currentAudioStatus, device_id: deviceId };
      return { ...currentAudioStatus };
    },
    isNativeAvailable: () => true,
    invokeCommand: async () => {},
  };
});

// Ensure localStorage mock
const storageMap = new Map<string, string>();
const fakeLocalStorage = {
  getItem: (key: string) => storageMap.get(key) ?? null,
  setItem: (key: string, val: string) => storageMap.set(key, String(val)),
  removeItem: (key: string) => storageMap.delete(key),
  clear: () => storageMap.clear(),
};
Object.defineProperty(globalThis, "localStorage", {
  value: fakeLocalStorage,
  writable: true,
  configurable: true,
});
if (typeof window === "undefined") {
  (globalThis as any).window = globalThis;
}
Object.defineProperty(window, "localStorage", {
  value: fakeLocalStorage,
  writable: true,
  configurable: true,
});

(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import {
  SessionSettingsPanel,
  QUALITY_STORAGE_KEY,
  savedBitrateMbps,
  applyBitrateCeiling,
  applyVolumeLevel,
  toggleAudioMuted,
  applyAudioDeviceSelection,
  refreshAudioDeviceList,
} from "./SessionSettingsPanel";

describe("SessionSettingsPanel", () => {
  beforeEach(() => {
    storageMap.clear();
    setBitrateCalls = [];
    setAudioVolumeCalls = [];
    setAudioMutedCalls = [];
    setAudioDeviceCalls = [];
    listAudioDevicesCalls = 0;
    audioStatusCalls = 0;
    currentAudioStatus = {
      active: true,
      volume: 1.0,
      muted: false,
      device_id: null,
      devices: [
        { id: "dev-speaker", name: "Speakers", supported: true },
        { id: "dev-headphones", name: "Headphones", supported: true },
      ],
      consumed_samples: 500,
      error: null,
    };
  });

  it("changing the volume slider calls the volume wrapper with a 0..1 value, not 0..100", async () => {
    // 1. Driving volume change with value 65 on 0..100 scale
    await applyVolumeLevel(65, true);

    expect(setAudioVolumeCalls.length).toBe(1);
    const calledVolume = setAudioVolumeCalls[0];

    // Assert that the called value is in 0..1, NOT 0..100
    expect(calledVolume).toBe(0.65);
    expect(calledVolume).toBeLessThanOrEqual(1.0);
    expect(calledVolume).toBeGreaterThanOrEqual(0.0);
    expect(calledVolume).not.toBe(65);

    // 2. Another volume change e.g. 20 -> 0.20
    await applyVolumeLevel(20, true);

    expect(setAudioVolumeCalls.length).toBe(2);
    expect(setAudioVolumeCalls[1]).toBe(0.2);
    expect(setAudioVolumeCalls[1]).toBeLessThanOrEqual(1.0);
    expect(setAudioVolumeCalls[1]).toBeGreaterThanOrEqual(0.0);
    expect(setAudioVolumeCalls[1]).not.toBe(20);

    // 3. Min boundary: 0 -> 0.0
    await applyVolumeLevel(0, true);
    expect(setAudioVolumeCalls[2]).toBe(0.0);

    // 4. Max boundary: 100 -> 1.0
    await applyVolumeLevel(100, true);
    expect(setAudioVolumeCalls[3]).toBe(1.0);
  });

  it("the mute button toggles against the CURRENT muted state", async () => {
    // 1. Initial state: currently unmuted (muted = false) -> toggles to true
    currentAudioStatus.muted = false;
    await toggleAudioMuted(currentAudioStatus.muted, true);

    expect(setAudioMutedCalls.length).toBe(1);
    expect(setAudioMutedCalls[0]).toBe(true);

    // 2. Second toggle: currently muted (muted = true) -> toggles to false
    currentAudioStatus.muted = true;
    await toggleAudioMuted(currentAudioStatus.muted, true);

    expect(setAudioMutedCalls.length).toBe(2);
    expect(setAudioMutedCalls[1]).toBe(false);

    // 3. Third toggle: currently unmuted (muted = false) -> toggles to true
    currentAudioStatus.muted = false;
    await toggleAudioMuted(currentAudioStatus.muted, true);

    expect(setAudioMutedCalls.length).toBe(3);
    expect(setAudioMutedCalls[2]).toBe(true);
  });

  it("choosing a bitrate calls the bitrate wrapper with the Mbps number and persists to localStorage", async () => {
    // Initial saved bitrate should default to 50 when storage is empty
    expect(savedBitrateMbps()).toBe(50);

    // User chooses 25 Mbps
    await applyBitrateCeiling(25, true);

    // Asserts: calls bitrate wrapper with the Mbps number (25)
    expect(setBitrateCalls.length).toBe(1);
    expect(setBitrateCalls[0]).toBe(25);
    expect(typeof setBitrateCalls[0]).toBe("number");

    // Asserts: persists to localStorage under the key "erd-quality-mbps"
    expect(globalThis.localStorage.getItem("erd-quality-mbps")).toBe("25");
    expect(globalThis.localStorage.getItem(QUALITY_STORAGE_KEY)).toBe("25");
    expect(savedBitrateMbps()).toBe(25);

    // User chooses 100 Mbps
    await applyBitrateCeiling(100, true);

    expect(setBitrateCalls.length).toBe(2);
    expect(setBitrateCalls[1]).toBe(100);
    expect(typeof setBitrateCalls[1]).toBe("number");
    expect(globalThis.localStorage.getItem("erd-quality-mbps")).toBe("100");
    expect(globalThis.localStorage.getItem(QUALITY_STORAGE_KEY)).toBe("100");
    expect(savedBitrateMbps()).toBe(100);

    // User chooses 8 Mbps
    await applyBitrateCeiling(8, true);
    expect(setBitrateCalls[2]).toBe(8);
    expect(globalThis.localStorage.getItem("erd-quality-mbps")).toBe("8");
    expect(savedBitrateMbps()).toBe(8);
  });

  it("refresh button calls listAudioDevices and apply button calls setAudioDevice", async () => {
    // Refresh audio device list
    await refreshAudioDeviceList(true);
    expect(listAudioDevicesCalls).toBe(1);

    // Apply specific audio device
    let released = false;
    const releaseInputs = async () => {
      released = true;
    };
    await applyAudioDeviceSelection("dev-headphones", true, releaseInputs);

    expect(released).toBe(true);
    expect(setAudioDeviceCalls.length).toBe(1);
    expect(setAudioDeviceCalls[0]).toBe("dev-headphones");

    // Apply system default (passes null to IPC)
    await applyAudioDeviceSelection("default", true);
    expect(setAudioDeviceCalls.length).toBe(2);
    expect(setAudioDeviceCalls[1]).toBeNull();
  });

  it("renders SessionSettingsPanel without throwing", async () => {
    if (typeof document !== "undefined" && typeof document.createElement === "function") {
      const container = document.createElement("div");
      const root = createRoot(container);

      await act(async () => {
        root.render(React.createElement(SessionSettingsPanel, { isConnected: true }));
      });

      expect(audioStatusCalls).toBeGreaterThanOrEqual(1);

      act(() => {
        root.unmount();
      });
    }
  });
});
