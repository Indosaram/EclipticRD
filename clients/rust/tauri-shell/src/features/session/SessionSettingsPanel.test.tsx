import { describe, it, expect, mock, beforeEach } from "bun:test";

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

function createStyleObject(): any {
  const s: any = {
    setProperty(k: string, v: string) {
      s[k] = String(v);
    },
    removeProperty(k: string): string {
      const prev = s[k] ?? "";
      delete s[k];
      return prev;
    },
    getPropertyValue(k: string) {
      return s[k] ?? "";
    },
  };
  return s;
}

// Set up minimal DOM environment before importing react-dom/client if document is undefined
if (typeof globalThis.document === "undefined") {
  function createFakeElement(tag: string, ownerDoc: any): any {
    const listeners: Record<string, Function[]> = {};
    const el: any = {
      tagName: (tag || "div").toUpperCase(),
      nodeType: 1,
      childNodes: [] as any[],
      children: [] as any[],
      style: createStyleObject(),
      attributes: {} as Record<string, string>,
      disabled: false,
      value: "",
      options: [],
      querySelectorAll(selector: string) {
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
      },
      querySelector(selector: string) {
        return (this as any).querySelectorAll(selector)[0] ?? null;
      },
      closest(sel: string) {
        let curr: any = this;
        while (curr) {
          if (sel.startsWith("#") && (curr.id === sel.slice(1) || curr.attributes?.id === sel.slice(1))) return curr;
          if (curr.tagName && curr.tagName.toLowerCase() === sel.toLowerCase()) return curr;
          curr = curr.parentNode;
        }
        return null;
      },
      _onClick: undefined as any,
      get onClick() {
        for (const key of Object.keys(this)) {
          if (key.startsWith("__reactProps") && this[key]?.onClick) {
            return this[key].onClick;
          }
        }
        return this._onClick;
      },
      set onClick(fn: any) {
        this._onClick = fn;
      },
      click() {
        for (const key of Object.keys(this)) {
          if (key.startsWith("__reactProps") && typeof this[key]?.onClick === "function") {
            this[key].onClick({
              currentTarget: this,
              target: this,
              preventDefault() {},
              stopPropagation() {},
            });
            return;
          }
        }
        if (typeof this._onClick === "function") {
          this._onClick({
            currentTarget: this,
            target: this,
            preventDefault() {},
            stopPropagation() {},
          });
          return;
        }
        this.dispatchEvent({ type: "click", target: this, currentTarget: this });
      },
      setAttribute(k: string, v: string) {
        this.attributes[k] = String(v);
        if (k === "disabled") {
          this.disabled = true;
        }
      },
      getAttribute(k: string) {
        if (k === "disabled" && this.disabled) return "";
        return this.attributes[k] ?? null;
      },
      removeAttribute(k: string) {
        delete this.attributes[k];
        if (k === "disabled") {
          this.disabled = false;
        }
      },
      addEventListener(type: string, fn: Function) {
        (listeners[type] ||= []).push(fn);
      },
      removeEventListener(type: string, fn: Function) {
        if (listeners[type]) {
          listeners[type] = listeners[type].filter((l) => l !== fn);
        }
      },
      dispatchEvent(e: any) {
        try {
          if (!e.target) Object.defineProperty(e, "target", { value: this, configurable: true });
          if (!e.currentTarget) Object.defineProperty(e, "currentTarget", { value: this, configurable: true });
        } catch (_) {}
        const list = [...(listeners[e.type] || [])];
        for (const fn of list) fn(e);
        return true;
      },
      appendChild(c: any) {
        this.childNodes.push(c);
        if (c.nodeType === 1) this.children.push(c);
        c.parentNode = this;
        return c;
      },
      removeChild(c: any) {
        const i = this.childNodes.indexOf(c);
        if (i !== -1) this.childNodes.splice(i, 1);
        const j = this.children.indexOf(c);
        if (j !== -1) this.children.splice(j, 1);
        c.parentNode = null;
        return c;
      },
      insertBefore(n: any, ref: any) {
        const i = this.childNodes.indexOf(ref);
        if (i !== -1) this.childNodes.splice(i, 0, n);
        else this.childNodes.push(n);
        if (n.nodeType === 1) {
          const j = this.children.indexOf(ref);
          if (j !== -1) this.children.splice(j, 0, n);
          else this.children.push(n);
        }
        n.parentNode = this;
        return n;
      },
      ownerDocument: ownerDoc,
      parentNode: null,
      getBoundingClientRect() {
        return { left: 0, top: 0, width: 320, height: 400, right: 320, bottom: 400 };
      },
      getContext() {
        return {};
      },
      get textContent(): string {
        return this.childNodes.map((c: any) => c.textContent ?? "").join("");
      },
      set textContent(v: string) {
        this.childNodes = [ownerDoc.createTextNode(v)];
        this.children = [];
      },
      classList: {
        add() {},
        remove() {},
        contains() {
          return false;
        },
        toggle() {
          return false;
        },
      },
      focus() {},
      blur() {},
    };
    return el;
  }

  const doc: any = {
    nodeType: 9,
    ownerDocument: null,
    createElement(tag: string) {
      return createFakeElement(tag, doc);
    },
    createElementNS(_ns: string, tag: string) {
      return createFakeElement(tag, doc);
    },
    createTextNode(text: string) {
      return {
        nodeType: 3,
        textContent: String(text),
        nodeValue: String(text),
        ownerDocument: doc,
        parentNode: null,
      };
    },
    createComment(data: string) {
      return { nodeType: 8, data, ownerDocument: doc, parentNode: null };
    },
    createDocumentFragment() {
      const f = createFakeElement("#document-fragment", doc);
      f.nodeType = 11;
      return f;
    },
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent() {
      return true;
    },
    defaultView: null,
    activeElement: null,
  };

  doc.defaultView = globalThis;
  doc.documentElement = createFakeElement("html", doc);
  doc.head = createFakeElement("head", doc);
  doc.body = createFakeElement("body", doc);
  doc.documentElement.appendChild(doc.head);
  doc.documentElement.appendChild(doc.body);

  globalThis.document = doc;
  globalThis.window = globalThis as any;
  (globalThis as any).HTMLCanvasElement = class HTMLCanvasElement {};
  (globalThis as any).HTMLDivElement = class HTMLDivElement {};
  (globalThis as any).HTMLButtonElement = class HTMLButtonElement {};
  (globalThis as any).HTMLIFrameElement = class HTMLIFrameElement {};
  (globalThis as any).HTMLSelectElement = class HTMLSelectElement {};
  (globalThis as any).Element = class Element {};
  (globalThis as any).Node = class Node {};
} else {
  // If document was defined by an earlier test file, enhance createElement with style methods and options
  const origCreate = globalThis.document.createElement.bind(globalThis.document);
  globalThis.document.createElement = function (tag: string, options?: any) {
    const el = origCreate(tag, options);
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
      if (!el.style || typeof el.style.setProperty !== "function") {
        el.style = createStyleObject();
      }
      if (tag && tag.toLowerCase() === "select" && !(el as any).options) {
        (el as any).options = [];
      }
    }
    return el;
  };
}

// Ensure mock localStorage
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
if (globalThis.window) {
  Object.defineProperty(globalThis.window, "localStorage", {
    value: fakeLocalStorage,
    writable: true,
    configurable: true,
  });
}

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

import React from "react";
import { createRoot } from "react-dom/client";
import {
  SessionSettingsPanel,
  QUALITY_STORAGE_KEY,
  savedBitrateMbps,
} from "./SessionSettingsPanel";

function findElements(node: any, predicate: (el: any) => boolean): any[] {
  const results: any[] = [];
  function walk(current: any) {
    if (!current) return;
    if (current.nodeType === 1) {
      try {
        if (predicate(current)) results.push(current);
      } catch (_) {}
    }
    for (const child of current.childNodes || current.children || []) {
      walk(child);
    }
  }
  walk(node);
  return results;
}

function findElement(node: any, predicate: (el: any) => boolean): any | null {
  const matches = findElements(node, predicate);
  return matches[0] ?? null;
}

function triggerClick(el: any) {
  for (const key of Object.keys(el)) {
    if (key.startsWith("__reactProps") && typeof el[key]?.onClick === "function") {
      el[key].onClick({
        currentTarget: el,
        target: el,
        preventDefault() {},
        stopPropagation() {},
      });
      return;
    }
  }
  if (typeof el.click === "function") {
    el.click();
  }
}

function triggerSliderCommit(el: any, val: number) {
  for (const key of Object.keys(el)) {
    if (key.startsWith("__reactProps")) {
      const p = el[key];
      if (typeof p?.onValueCommit === "function") {
        p.onValueCommit([val]);
        return;
      }
      if (typeof p?.onChange === "function") {
        p.onChange({ target: { value: val }, currentTarget: el });
        return;
      }
    }
  }
  if (typeof el.dispatchEvent === "function") {
    const ev = new Event("change") as any;
    ev.target = { value: val };
    el.dispatchEvent(ev);
  }
}

function triggerSelectChange(el: any, value: string) {
  for (const key of Object.keys(el)) {
    if (key.startsWith("__reactProps")) {
      const p = el[key];
      if (typeof p?.onChange === "function") {
        p.onChange({ target: { value }, currentTarget: el });
        return;
      }
      if (typeof p?.onValueChange === "function") {
        p.onValueChange(value);
        return;
      }
    }
  }
  if (typeof el.dispatchEvent === "function") {
    const ev = new Event("change") as any;
    ev.target = { value };
    el.dispatchEvent(ev);
  }
}

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
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    root.render(React.createElement(SessionSettingsPanel, { isConnected: true }));
    // Wait for initial audioStatus fetch and mount
    await new Promise((resolve) => setTimeout(resolve, 50));

    const slider = findElement(container, (el) => el.getAttribute("id") === "audio-volume");
    expect(slider).not.toBeNull();

    // Trigger volume slider change with value 65 (on 0..100 slider scale)
    triggerSliderCommit(slider, 65);
    await new Promise((resolve) => setTimeout(resolve, 30));

    expect(setAudioVolumeCalls.length).toBe(1);
    const calledVolume = setAudioVolumeCalls[0];

    // Assert that the called value is in 0..1, NOT 0..100
    expect(calledVolume).toBe(0.65);
    expect(calledVolume).toBeLessThanOrEqual(1.0);
    expect(calledVolume).toBeGreaterThanOrEqual(0.0);
    expect(calledVolume).not.toBe(65);

    // Another volume change e.g. 20 -> 0.20
    triggerSliderCommit(slider, 20);
    await new Promise((resolve) => setTimeout(resolve, 30));

    expect(setAudioVolumeCalls.length).toBe(2);
    expect(setAudioVolumeCalls[1]).toBe(0.2);
    expect(setAudioVolumeCalls[1]).toBeLessThanOrEqual(1.0);

    root.unmount();
    document.body.removeChild(container);
  });

  it("the mute button toggles against the CURRENT muted state", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    // Initial state: muted is false
    currentAudioStatus.muted = false;

    root.render(React.createElement(SessionSettingsPanel, { isConnected: true }));
    await new Promise((resolve) => setTimeout(resolve, 50));

    const muteBtn = findElement(container, (el) => el.getAttribute("id") === "btn-audio-mute");
    expect(muteBtn).not.toBeNull();

    // 1st click: currently unmuted (muted = false) -> should toggle to true
    triggerClick(muteBtn);
    await new Promise((resolve) => setTimeout(resolve, 40));

    expect(setAudioMutedCalls.length).toBe(1);
    expect(setAudioMutedCalls[0]).toBe(true);

    // 2nd click: currently muted (muted = true) -> should toggle to false
    triggerClick(muteBtn);
    await new Promise((resolve) => setTimeout(resolve, 40));

    expect(setAudioMutedCalls.length).toBe(2);
    expect(setAudioMutedCalls[1]).toBe(false);

    root.unmount();
    document.body.removeChild(container);
  });

  it("choosing a bitrate calls the bitrate wrapper with the Mbps number and persists to localStorage", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    root.render(React.createElement(SessionSettingsPanel, { isConnected: true }));
    await new Promise((resolve) => setTimeout(resolve, 50));

    const qualitySelect = findElement(
      container,
      (el) => el.getAttribute("id") === "session-quality"
    );
    expect(qualitySelect).not.toBeNull();

    // User chooses 25 Mbps
    triggerSelectChange(qualitySelect, "25");
    await new Promise((resolve) => setTimeout(resolve, 30));

    // Asserts: calls bitrate wrapper with the Mbps number (25)
    expect(setBitrateCalls.length).toBe(1);
    expect(setBitrateCalls[0]).toBe(25);
    expect(typeof setBitrateCalls[0]).toBe("number");

    // Asserts: persists to localStorage under QUALITY_STORAGE_KEY ('erd-quality-mbps')
    expect(globalThis.localStorage.getItem(QUALITY_STORAGE_KEY)).toBe("25");
    expect(savedBitrateMbps()).toBe(25);

    // User chooses 100 Mbps
    triggerSelectChange(qualitySelect, "100");
    await new Promise((resolve) => setTimeout(resolve, 30));

    expect(setBitrateCalls.length).toBe(2);
    expect(setBitrateCalls[1]).toBe(100);
    expect(globalThis.localStorage.getItem(QUALITY_STORAGE_KEY)).toBe("100");
    expect(savedBitrateMbps()).toBe(100);

    root.unmount();
    document.body.removeChild(container);
  });

  it("refresh button calls listAudioDevices and apply button calls setAudioDevice", async () => {
    const container = document.createElement("div");
    document.body.appendChild(container);
    const root = createRoot(container);

    root.render(React.createElement(SessionSettingsPanel, { isConnected: true }));
    await new Promise((resolve) => setTimeout(resolve, 50));

    const refreshBtn = findElement(
      container,
      (el) => el.getAttribute("id") === "btn-audio-refresh"
    );
    expect(refreshBtn).not.toBeNull();
    triggerClick(refreshBtn);
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(listAudioDevicesCalls).toBe(1);

    const applyBtn = findElement(
      container,
      (el) => el.getAttribute("id") === "btn-audio-apply"
    );
    expect(applyBtn).not.toBeNull();
    triggerClick(applyBtn);
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(setAudioDeviceCalls.length).toBe(1);

    root.unmount();
    document.body.removeChild(container);
  });
});
