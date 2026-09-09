(function (root, factory) {
  const api = factory();
  root.VideoRenderer = api;
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  }
})(typeof window !== 'undefined' ? window : globalThis, function () {
  'use strict';

  function compileShader(gl, type, source) {
    const shader = gl.createShader(type);
    gl.shaderSource(shader, source);
    gl.compileShader(shader);
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      const err = gl.getShaderInfoLog(shader);
      gl.deleteShader(shader);
      throw new Error('Shader compilation failed: ' + err);
    }
    return shader;
  }

  function createProgram(gl, vsSource, fsSource) {
    const vs = compileShader(gl, gl.VERTEX_SHADER, vsSource);
    const fs = compileShader(gl, gl.FRAGMENT_SHADER, fsSource);
    const program = gl.createProgram();
    gl.attachShader(program, vs);
    gl.attachShader(program, fs);
    gl.linkProgram(program);
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      const err = gl.getProgramInfoLog(program);
      gl.deleteProgram(program);
      throw new Error('Program linking failed: ' + err);
    }
    return program;
  }

  function createRenderer(canvas, options) {
    if (!canvas) {
      throw new Error('Canvas element required for VideoRenderer');
    }

    const onPresented = options && typeof options.onPresented === 'function'
      ? options.onPresented
      : null;

    let isWebGL2 = true;
    let gl = canvas.getContext('webgl2', {
      alpha: false,
      depth: false,
      stencil: false,
      antialias: false,
      desynchronized: true,
      preserveDrawingBuffer: false
    });

    if (!gl) {
      isWebGL2 = false;
      gl = canvas.getContext('webgl', {
        alpha: false,
        depth: false,
        stencil: false,
        antialias: false,
        preserveDrawingBuffer: false
      });
    }

    if (!gl) {
      throw new Error('WebGL is not supported in this browser context');
    }

    const vsSource = isWebGL2
      ? `#version 300 es
        in vec2 a_pos;
        in vec2 a_texCoord;
        out vec2 v_texCoord;
        void main() {
          gl_Position = vec4(a_pos, 0.0, 1.0);
          v_texCoord = a_texCoord;
        }`
      : `attribute vec2 a_pos;
        attribute vec2 a_texCoord;
        varying vec2 v_texCoord;
        void main() {
          gl_Position = vec4(a_pos, 0.0, 1.0);
          v_texCoord = a_texCoord;
        }`;

    const fsSource = isWebGL2
      ? `#version 300 es
        precision highp float;
        in vec2 v_texCoord;
        out vec4 fragColor;
        uniform sampler2D u_yPlane;
        uniform sampler2D u_uvPlane;
        void main() {
          float y = (texture(u_yPlane, v_texCoord).r - (16.0 / 255.0)) * (255.0 / 219.0);
          vec2 uv = texture(u_uvPlane, v_texCoord).rg - vec2(128.0 / 255.0, 128.0 / 255.0);
          float u = uv.r * (255.0 / 224.0);
          float v = uv.g * (255.0 / 224.0);
          float r = y + 1.402 * v;
          float g = y - 0.344136 * u - 0.714136 * v;
          float b = y + 1.772 * u;
          fragColor = vec4(clamp(vec3(r, g, b), 0.0, 1.0), 1.0);
        }`
      : `precision highp float;
        varying vec2 v_texCoord;
        uniform sampler2D u_yPlane;
        uniform sampler2D u_uvPlane;
        void main() {
          float y = (texture2D(u_yPlane, v_texCoord).r - (16.0 / 255.0)) * (255.0 / 219.0);
          vec2 uv = vec2(texture2D(u_uvPlane, v_texCoord).r, texture2D(u_uvPlane, v_texCoord).a) - vec2(128.0 / 255.0, 128.0 / 255.0);
          float u = uv.r * (255.0 / 224.0);
          float v = uv.g * (255.0 / 224.0);
          float r = y + 1.402 * v;
          float g = y - 0.344136 * u - 0.714136 * v;
          float b = y + 1.772 * u;
          gl_FragColor = vec4(clamp(vec3(r, g, b), 0.0, 1.0), 1.0);
        }`;

    const program = createProgram(gl, vsSource, fsSource);
    gl.useProgram(program);

    const posBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, posBuf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([
      -1.0, -1.0,  1.0, -1.0, -1.0,  1.0,
      -1.0,  1.0,  1.0, -1.0,  1.0,  1.0
    ]), gl.STATIC_DRAW);

    const posLoc = gl.getAttribLocation(program, 'a_pos');
    gl.enableVertexAttribArray(posLoc);
    gl.vertexAttribPointer(posLoc, 2, gl.FLOAT, false, 0, 0);

    const texBuf = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, texBuf);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([
      0.0, 1.0,  1.0, 1.0,  0.0, 0.0,
      0.0, 0.0,  1.0, 1.0,  1.0, 0.0
    ]), gl.STATIC_DRAW);

    const texLoc = gl.getAttribLocation(program, 'a_texCoord');
    gl.enableVertexAttribArray(texLoc);
    gl.vertexAttribPointer(texLoc, 2, gl.FLOAT, false, 0, 0);

    function setupTexture(unit) {
      const tex = gl.createTexture();
      gl.activeTexture(unit);
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      return tex;
    }

    const yTexture = setupTexture(gl.TEXTURE0);
    const uvTexture = setupTexture(gl.TEXTURE1);

    gl.uniform1i(gl.getUniformLocation(program, 'u_yPlane'), 0);
    gl.uniform1i(gl.getUniformLocation(program, 'u_uvPlane'), 1);

    let lastWidth = 0;
    let lastHeight = 0;
    let presentedCount = 0;

    function render(frame) {
      if (!gl || gl.isContextLost()) return false;
      const width = frame.width;
      const height = frame.height;
      const yData = frame.yData;
      const uvData = frame.uvData;
      const sequence = frame.sequence;

      const uvWidth = Math.floor(width / 2);
      const uvHeight = Math.ceil(height / 2);

      if (lastWidth !== width || lastHeight !== height) {
        canvas.width = width;
        canvas.height = height;
        gl.viewport(0, 0, width, height);
        lastWidth = width;
        lastHeight = height;

        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, yTexture);
        if (isWebGL2) {
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, width, height, 0, gl.RED, gl.UNSIGNED_BYTE, null);
        } else {
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.LUMINANCE, width, height, 0, gl.LUMINANCE, gl.UNSIGNED_BYTE, null);
        }

        gl.activeTexture(gl.TEXTURE1);
        gl.bindTexture(gl.TEXTURE_2D, uvTexture);
        if (isWebGL2) {
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.RG8, uvWidth, uvHeight, 0, gl.RG, gl.UNSIGNED_BYTE, null);
        } else {
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.LUMINANCE_ALPHA, uvWidth, uvHeight, 0, gl.LUMINANCE_ALPHA, gl.UNSIGNED_BYTE, null);
        }
      }

      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);

      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, yTexture);
      if (isWebGL2) {
        gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, width, height, gl.RED, gl.UNSIGNED_BYTE, yData);
      } else {
        gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, width, height, gl.LUMINANCE, gl.UNSIGNED_BYTE, yData);
      }

      gl.activeTexture(gl.TEXTURE1);
      gl.bindTexture(gl.TEXTURE_2D, uvTexture);
      if (isWebGL2) {
        gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, uvWidth, uvHeight, gl.RG, gl.UNSIGNED_BYTE, uvData);
      } else {
        gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, uvWidth, uvHeight, gl.LUMINANCE_ALPHA, gl.UNSIGNED_BYTE, uvData);
      }

      gl.drawArrays(gl.TRIANGLES, 0, 6);
      presentedCount++;

      if (onPresented) {
        if (presentedCount === 1 || presentedCount % 60 === 0) {
          try {
            onPresented(sequence);
          } catch (_) {}
        }
      }

      return true;
    }

    function clear() {
      if (!gl || gl.isContextLost()) return;
      gl.clearColor(0.0, 0.0, 0.0, 1.0);
      gl.clear(gl.COLOR_BUFFER_BIT);
      presentedCount = 0;
    }

    function getDimensions() {
      return { width: lastWidth, height: lastHeight };
    }

    function getPresentedCount() {
      return presentedCount;
    }

    return {
      render,
      clear,
      getDimensions,
      getPresentedCount,
      isWebGL2: () => isWebGL2
    };
  }

  return {
    createRenderer
  };
});
