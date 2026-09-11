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
