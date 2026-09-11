import { describe, expect, it } from "bun:test";
import { parseFrame } from "./renderer";

function createFrameBuffer(
  width: number,
  height: number,
  tail?: { x: number; y: number; type: number } | number[]
): ArrayBuffer {
  const yLen = width * height;
  const uvWidth = Math.floor(width / 2);
  const uvHeight = Math.ceil(height / 2);
  const uvLen = uvWidth * uvHeight * 2;
  const tailLen = tail ? (Array.isArray(tail) ? tail.length : 9) : 0;
  const totalLen = 16 + yLen + uvLen + tailLen;

  const buf = new ArrayBuffer(totalLen);
  const view = new DataView(buf);
  view.setUint32(0, width, true);
  view.setUint32(4, height, true);

  const yData = new Uint8Array(buf, 16, yLen);
  for (let i = 0; i < yLen; i++) {
    yData[i] = (i & 0xff);
  }

  const uvData = new Uint8Array(buf, 16 + yLen, uvLen);
  for (let i = 0; i < uvLen; i++) {
    uvData[i] = ((i * 2) & 0xff);
  }

  if (tail) {
    const tailOffset = 16 + yLen + uvLen;
    if (Array.isArray(tail)) {
      const u8 = new Uint8Array(buf);
      for (let i = 0; i < tail.length; i++) {
        u8[tailOffset + i] = tail[i];
      }
    } else {
      view.setFloat32(tailOffset, tail.x, true);
      view.setFloat32(tailOffset + 4, tail.y, true);
      view.setUint8(tailOffset + 8, tail.type);
    }
  }

  return buf;
}

describe("parseFrame", () => {
  it("a well-formed buffer yields the right width, height, plane lengths and a null cursor", () => {
    const width = 64;
    const height = 48;
    const yLen = 64 * 48;
    const uvWidth = Math.floor(64 / 2);
    const uvHeight = Math.ceil(48 / 2);
    const uvLen = uvWidth * uvHeight * 2;
    const buf = createFrameBuffer(width, height);

    const parsed = parseFrame(buf);
    expect(parsed).not.toBeNull();
    expect(parsed?.width).toBe(width);
    expect(parsed?.height).toBe(height);
    expect(parsed?.y.byteLength).toBe(yLen);
    expect(parsed?.uv.byteLength).toBe(uvLen);
    expect(parsed?.cursor).toBeNull();
  });

  it("the same buffer plus a 9-byte tail yields the cursor x, y and visible flag", () => {
    const width = 64;
    const height = 48;
    const buf = createFrameBuffer(width, height, { x: 142.25, y: 284.5, type: 1 });

    const parsed = parseFrame(buf);
    expect(parsed).not.toBeNull();
    expect(parsed?.width).toBe(width);
    expect(parsed?.height).toBe(height);
    expect(parsed?.cursor).not.toBeNull();
    expect(parsed?.cursor?.x).toBeCloseTo(142.25, 2);
    expect(parsed?.cursor?.y).toBeCloseTo(284.5, 2);
    expect(parsed?.cursor?.visible).toBe(true);
  });

  it("cursor type 0 yields visible false", () => {
    const width = 32;
    const height = 32;
    const buf = createFrameBuffer(width, height, { x: 10.0, y: 20.0, type: 0 });

    const parsed = parseFrame(buf);
    expect(parsed).not.toBeNull();
    expect(parsed?.cursor).not.toBeNull();
    expect(parsed?.cursor?.visible).toBe(false);
    expect(parsed?.cursor?.x).toBeCloseTo(10.0, 2);
    expect(parsed?.cursor?.y).toBeCloseTo(20.0, 2);
  });

  it("cursor type > 0 (e.g. type 2) yields visible true", () => {
    const width = 16;
    const height = 16;
    const buf = createFrameBuffer(width, height, { x: 5.5, y: 7.5, type: 2 });

    const parsed = parseFrame(buf);
    expect(parsed).not.toBeNull();
    expect(parsed?.cursor).not.toBeNull();
    expect(parsed?.cursor?.visible).toBe(true);
  });

  it("buffers shorter than the header yield null instead of throwing", () => {
    expect(parseFrame(new ArrayBuffer(0))).toBeNull();
    expect(parseFrame(new ArrayBuffer(4))).toBeNull();
    expect(parseFrame(new ArrayBuffer(15))).toBeNull();
  });

  it("buffers shorter than header+planes both yield null instead of throwing", () => {
    const width = 64;
    const height = 48;
    const fullBuf = createFrameBuffer(width, height);
    // fullBuf length is 16 + 3072 + 1536 = 4624 bytes
    const headerOnly = fullBuf.slice(0, 16);
    const partialY = fullBuf.slice(0, 16 + 100);
    const yOnly = fullBuf.slice(0, 16 + 3072);
    const partialUv = fullBuf.slice(0, fullBuf.byteLength - 1);

    expect(parseFrame(headerOnly)).toBeNull();
    expect(parseFrame(partialY)).toBeNull();
    expect(parseFrame(yOnly)).toBeNull();
    expect(parseFrame(partialUv)).toBeNull();
  });

  it("buffers with partial tail (< 9 bytes) yield null cursor without throwing", () => {
    const width = 16;
    const height = 16;
    // 8 extra bytes instead of 9
    const buf = createFrameBuffer(width, height, [1, 2, 3, 4, 5, 6, 7, 8]);
    const parsed = parseFrame(buf);
    expect(parsed).not.toBeNull();
    expect(parsed?.cursor).toBeNull();
  });

  it("calculates UV dimensions matching NV12 half-sampling math", () => {
    const width = 65;
    const height = 47;
    const yLen = 65 * 47;
    const uvWidth = Math.floor(65 / 2); // 32
    const uvHeight = Math.ceil(47 / 2); // 24
    const uvLen = uvWidth * uvHeight * 2; // 1536
    const buf = createFrameBuffer(width, height);

    const parsed = parseFrame(buf);
    expect(parsed).not.toBeNull();
    expect(parsed?.width).toBe(65);
    expect(parsed?.height).toBe(47);
    expect(parsed?.y.byteLength).toBe(yLen);
    expect(parsed?.uv.byteLength).toBe(uvLen);
  });
});

interface Nv12Pixel {
  y: number;
  u: number;
  v: number;
}

interface RgbPixel {
  r: number;
  g: number;
  b: number;
}

function clamp(val: number, min: number, max: number): number {
  return Math.min(Math.max(val, min), max);
}

function hostBgraToNv12(r: number, g: number, b: number): Nv12Pixel {
  const y = (77 * r + 150 * g + 29 * b) >> 8;
  const u = 128 + ((-43 * r - 85 * g + 128 * b) >> 8);
  const v = 128 + ((128 * r - 107 * g - 21 * b) >> 8);
  return { y, u, v };
}

function shaderNv12ToRgb(y: number, u: number, v: number): RgbPixel {
  // Texture sampling normalizes unsigned bytes [0, 255] to [0.0, 1.0]
  const texY = y / 255.0;
  const texU = u / 255.0;
  const texV = v / 255.0;

  // New full-range BT.601 shader math (no 16/255 offset, no 255/219 scaling, no 255/224 scaling)
  const shaderY = texY;
  const shaderU = texU - 0.5;
  const shaderV = texV - 0.5;

  const rNorm = clamp(shaderY + 1.402 * shaderV, 0.0, 1.0);
  const gNorm = clamp(shaderY - 0.344136 * shaderU - 0.714136 * shaderV, 0.0, 1.0);
  const bNorm = clamp(shaderY + 1.772 * shaderU, 0.0, 1.0);

  return {
    r: rNorm * 255.0,
    g: gNorm * 255.0,
    b: bNorm * 255.0,
  };
}

describe("shader artifact range validation", () => {
  it("fails if limited-range BT.601 shader expansions or offsets are reintroduced in renderer.ts", async () => {
    const rendererSrc = await Bun.file(new URL("./renderer.ts", import.meta.url)).text();

    expect(rendererSrc.includes("255.0 / 219.0")).toBe(false);
    expect(rendererSrc.includes("255.0/219.0")).toBe(false);
    expect(rendererSrc.includes("255.0 / 224.0")).toBe(false);
    expect(rendererSrc.includes("255.0/224.0")).toBe(false);
    expect(rendererSrc.includes("16.0 / 255.0")).toBe(false);
    expect(rendererSrc.includes("16.0/255.0")).toBe(false);

    expect(rendererSrc.includes("texture(u_yPlane")).toBe(true);
    expect(rendererSrc.includes("texture2D(u_yPlane")).toBe(true);

    const occurrences = rendererSrc.split("- vec2(0.5, 0.5)").length - 1;
    expect(occurrences).toBeGreaterThanOrEqual(2);
  });
});

describe("BT.601 full-range NV12 round-trip", () => {
  // Tolerance of 3/255 (~0.01176 normalized, or 3.0 in 0..255 space)
  // Max observed integer-quantization error across test colors is ~1.07/255.
  const TOLERANCE_255 = 3;
  const TOLERANCE_NORM = 3 / 255;

  const testCases: { name: string; r: number; g: number; b: number }[] = [
    { name: "black", r: 0, g: 0, b: 0 },
    { name: "white", r: 255, g: 255, b: 255 },
    { name: "mid grey", r: 128, g: 128, b: 128 },
    { name: "pure red", r: 255, g: 0, b: 0 },
    { name: "pure green", r: 0, g: 255, b: 0 },
    { name: "pure blue", r: 0, g: 0, b: 255 },
  ];

  for (const { name, r, g, b } of testCases) {
    it(`round-trips ${name} (r=${r}, g=${g}, b=${b}) within 3/255 tolerance`, () => {
      const { y, u, v } = hostBgraToNv12(r, g, b);
      const out = shaderNv12ToRgb(y, u, v);

      const errR = Math.abs(out.r - r);
      const errG = Math.abs(out.g - g);
      const errB = Math.abs(out.b - b);

      expect(errR).toBeLessThanOrEqual(TOLERANCE_255);
      expect(errG).toBeLessThanOrEqual(TOLERANCE_255);
      expect(errB).toBeLessThanOrEqual(TOLERANCE_255);

      expect(errR / 255).toBeLessThanOrEqual(TOLERANCE_NORM);
      expect(errG / 255).toBeLessThanOrEqual(TOLERANCE_NORM);
      expect(errB / 255).toBeLessThanOrEqual(TOLERANCE_NORM);
    });
  }

  it("discriminates against limited-range decoding with a tight <= 1/255 bound on mid grey", () => {
    // Under the old limited-range math:
    //   y = (128.0 / 255.0 - 16.0 / 255.0) * (255.0 / 219.0) = 112 / 219 = 0.5114155
    //   which produced 130.41, i.e. an error of 2.41/255 from 128.
    // Under full-range math:
    //   y = 128.0 / 255.0, u = 128.0 / 255.0 - 0.5, v = 128.0 / 255.0 - 0.5
    //   which round-trips 128 exactly (with chroma offset 0.5/255 yielding max error ~0.886/255).
    // An error bound of <= 1/255 strictly passes for full-range math and fails for limited-range math,
    // so this assertion is what actually fails if the limited-range bug returns.
    const TIGHT_TOLERANCE_255 = 1;
    const TIGHT_TOLERANCE_NORM = 1 / 255;

    const { y, u, v } = hostBgraToNv12(128, 128, 128);
    const out = shaderNv12ToRgb(y, u, v);

    const errR = Math.abs(out.r - 128);
    const errG = Math.abs(out.g - 128);
    const errB = Math.abs(out.b - 128);

    expect(errR).toBeLessThanOrEqual(TIGHT_TOLERANCE_255);
    expect(errG).toBeLessThanOrEqual(TIGHT_TOLERANCE_255);
    expect(errB).toBeLessThanOrEqual(TIGHT_TOLERANCE_255);

    expect(errR / 255).toBeLessThanOrEqual(TIGHT_TOLERANCE_NORM);
    expect(errG / 255).toBeLessThanOrEqual(TIGHT_TOLERANCE_NORM);
    expect(errB / 255).toBeLessThanOrEqual(TIGHT_TOLERANCE_NORM);
  });

  it("explicitly asserts that WHITE comes back near 255 without being clipped by limited-range math", () => {
    const { y, u, v } = hostBgraToNv12(255, 255, 255);
    // Luma for full-range white spans up to 255 (not 235)
    expect(y).toBe(255);
    const out = shaderNv12ToRgb(y, u, v);
    // Under limited-range math, raw Y was ~278.3 (blown highlights).
    // Under full-range math, RGB values are all within TOLERANCE_255 of 255.
    expect(out.r).toBeGreaterThanOrEqual(255 - TOLERANCE_255);
    expect(out.g).toBeGreaterThanOrEqual(255 - TOLERANCE_255);
    expect(out.b).toBeGreaterThanOrEqual(255 - TOLERANCE_255);
    expect(Math.abs(out.r - 255)).toBeLessThanOrEqual(TOLERANCE_255);
    expect(Math.abs(out.g - 255)).toBeLessThanOrEqual(TOLERANCE_255);
    expect(Math.abs(out.b - 255)).toBeLessThanOrEqual(TOLERANCE_255);
  });

  it("explicitly asserts that BLACK comes back near 0 without being crushed by limited-range math", () => {
    const { y, u, v } = hostBgraToNv12(0, 0, 0);
    // Luma for full-range black starts at 0 (not 16)
    expect(y).toBe(0);
    const out = shaderNv12ToRgb(y, u, v);
    // Under limited-range math, raw Y was -18.6 (crushed blacks below 16).
    // Under full-range math, RGB values are all within TOLERANCE_255 of 0.
    expect(out.r).toBeLessThanOrEqual(TOLERANCE_255);
    expect(out.g).toBeLessThanOrEqual(TOLERANCE_255);
    expect(out.b).toBeLessThanOrEqual(TOLERANCE_255);
    expect(Math.abs(out.r - 0)).toBeLessThanOrEqual(TOLERANCE_255);
    expect(Math.abs(out.g - 0)).toBeLessThanOrEqual(TOLERANCE_255);
    expect(Math.abs(out.b - 0)).toBeLessThanOrEqual(TOLERANCE_255);
  });
});
