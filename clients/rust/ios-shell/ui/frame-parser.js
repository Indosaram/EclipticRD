(function (root, factory) {
  const api = factory();
  root.FrameParser = api;
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  }
})(typeof window !== 'undefined' ? window : globalThis, function () {
  'use strict';

  function getArrayBufferAndOffset(input) {
    if (!input) return null;
    if (input instanceof ArrayBuffer) {
      return { buffer: input, byteOffset: 0, byteLength: input.byteLength };
    }
    if (ArrayBuffer.isView(input)) {
      return {
        buffer: input.buffer,
        byteOffset: input.byteOffset,
        byteLength: input.byteLength
      };
    }
    return null;
  }

  function parseNv12Frame(input) {
    const info = getArrayBufferAndOffset(input);
    if (!info) {
      return { ok: false, error: 'Invalid frame payload type' };
    }

    if (info.byteLength < 16) {
      return { ok: false, error: 'Frame buffer shorter than 16-byte header' };
    }

    const view = new DataView(info.buffer, info.byteOffset, info.byteLength);
    const width = view.getUint32(0, true);
    const height = view.getUint32(4, true);

    let sequence = 0n;
    if (typeof view.getBigUint64 === 'function') {
      sequence = view.getBigUint64(8, true);
    } else {
      const low = BigInt(view.getUint32(8, true));
      const high = BigInt(view.getUint32(12, true));
      sequence = (high << 32n) | low;
    }

    if (width === 0 || height === 0) {
      return { ok: false, error: 'Invalid zero dimensions in frame header' };
    }

    const yLength = width * height;
    const uvHeight = Math.ceil(height / 2);
    const uvLength = width * uvHeight;
    const requiredBytes = 16 + yLength + uvLength;

    if (info.byteLength < requiredBytes) {
      return {
        ok: false,
        error: 'Buffer truncated for dimensions ' + width + 'x' + height
      };
    }

    const yOffset = info.byteOffset + 16;
    const uvOffset = yOffset + yLength;
    const yData = new Uint8Array(info.buffer, yOffset, yLength);
    const uvData = new Uint8Array(info.buffer, uvOffset, uvLength);

    return {
      ok: true,
      width,
      height,
      sequence,
      yOffset: 16,
      yLength,
      uvOffset: 16 + yLength,
      uvLength,
      yData,
      uvData
    };
  }

  return {
    parseNv12Frame
  };
});
