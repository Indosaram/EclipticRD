# FFmpeg Windows Dynamic Link Libraries Guide

This document describes the runtime FFmpeg dynamic library (`.dll`) dependencies for EclipticRD on Windows (`x86_64-pc-windows-msvc` / `x86_64-pc-windows-gnu`).

## 1. Required FFmpeg DLLs

When dynamic linking against FFmpeg (via `ffmpeg-next` / `ffmpeg-sys-next` in `erd-decode` or `erd-host`), the following shared library DLLs must be placed in the same directory as `erd-host.exe` / `erd-client.exe` (or in a system directory indexed by `%PATH%`):

| Library Module | Typical DLL Names (FFmpeg 7.x / 6.x) | Purpose in EclipticRD |
|---|---|---|
| **libavcodec** | `avcodec-61.dll`, `avcodec-60.dll` | Video and audio decoding (H.264/HEVC) and hardware acceleration interfaces (D3D11VA). |
| **libavformat** | `avformat-61.dll`, `avformat-60.dll` | Container multiplexing and demuxing protocols. |
| **libavutil** | `avutil-59.dll`, `avutil-58.dll` | Core cryptographic utilities, pixel formats, memory buffers, and SIMD optimizations. |
| **libswscale** | `swscale-8.dll`, `swscale-7.dll` | Color space conversion (YUV420P to BGRA/RGBA) and frame scaling. |
| **libswresample** | `swresample-5.dll`, `swresample-4.dll` | Audio sample rate and channel layout resampling (e.g., stereo 48 kHz). |

### Optional / Transitive Dependencies

Depending on your build environment and build tools (vcpkg, Gyan.dev FFmpeg release builds, or custom MinGW builds), the following additional dependency DLLs may be needed:
- `libx264-*.dll` / `libx265-*.dll` (if software encoding libraries are dynamically referenced)
- `zlib1.dll` / `zlib.dll`
- `libmfx-*.dll` / `mfx.dll` (if Intel QuickSync acceleration is enabled)

---

## 2. Recommended Directory Layout on Windows

When distributing pre-built binaries, lay out your folder as follows:

```
EclipticRD/
├── erd-host.exe
├── erd-client.exe
├── avcodec-61.dll
├── avformat-61.dll
├── avutil-59.dll
├── swscale-8.dll
├── swresample-5.dll
├── LICENSE
├── README.md
└── README-FFMPEG.md
```

---

## 3. License and Compliance Notes

FFmpeg is licensed under the **GNU Lesser General Public License (LGPL) version 2.1 or later**, or the **GNU General Public License (GPL) version 2 or later** if built with GPL-only components (such as `libx264`, `libx265`, `libdav1d`, or `--enable-gpl`).

When distributing EclipticRD bundles alongside FFmpeg binaries:

1. **Dynamic Linking Compliance (LGPL v2.1+)**:
   - Keep the FFmpeg libraries in separate DLL files (`avcodec-*.dll`, etc.) as shipped. Do not statically combine LGPL FFmpeg code into proprietary binaries unless source code and relinking mechanisms are provided.
2. **License Notices**:
   - Provide a copy of the LGPL / GPL license terms with your application distribution.
   - Clearly state in your documentation that EclipticRD uses FFmpeg under LGPL/GPL.
3. **Source Code Availability**:
   - If distributing modified FFmpeg binaries, provide access to the FFmpeg source code and build configurations matching your distributed DLLs.
4. **Official Source Code**:
   - FFmpeg source code and build instructions are available at: [https://ffmpeg.org/download.html](https://ffmpeg.org/download.html)
