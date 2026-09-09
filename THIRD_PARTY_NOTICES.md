# Third-party software and distribution requirements

EclipticRD's original source is covered by the [MIT License](LICENSE). This does
not relicense its dependencies, operating-system frameworks, drivers, or SDKs.

## Rust dependency inventory

[The dependency inventory](docs/dependency-licenses.md) records the license
expressions declared by the packages resolved from the committed lockfiles.
Preserve the actual copyright, license and NOTICE files of dependencies included
in a distributed binary; the inventory is not a replacement for those files.

Most packages offer MIT, Apache-2.0, BSD, ISC or similar permissive terms. Important
distinctions in the current main-workspace graph include:

- `cssparser`, `cssparser-macros`, `dtoa-short`, `option-ext` and `selectors`:
  **MPL-2.0**. Its file-level source and notice obligations apply when distributing
  covered software; it does not automatically relicense unrelated EclipticRD files.
- `ring`: **Apache-2.0 AND ISC**; both parts of the expression matter.
- `dpi`: **Apache-2.0 AND MIT**.
- `unicode-ident`: **(MIT OR Apache-2.0) AND Unicode-3.0**.
- ICU/data crates include **Unicode-3.0** terms.
- `webpki-roots` declares **CDLA-Permissive-2.0** for its certificate-root data.
- `ffmpeg-next` and `ffmpeg-sys-next` declare **WTFPL** for their Rust wrappers.
  Those declarations do **not** describe the native FFmpeg libraries.

An `OR` expression offers alternatives; an `AND` expression requires the
applicable terms together. Legacy slash-separated declarations are preserved
as supplied by upstream metadata rather than silently rewritten.

## FFmpeg and codecs

FFmpeg's baseline license is LGPL-2.1-or-later, with optional GPL components and
external libraries that change the resulting build's terms.

The current `scripts/build-ffmpeg-omarchy.sh` enables:

```text
--enable-gpl --enable-libx264 --enable-nonfree
```

The verified FFmpeg 7.0.2 binary reports:

```text
This version of ffmpeg has nonfree parts compiled in.
Therefore it is not legally redistributable.
```

**Do not redistribute that FFmpeg build or present the existing private Linux
bundle as an approved public release.** Making EclipticRD's source public does
not change this restriction. Removing a filename or adding this notice does not
make the existing binaries redistributable.

A public binary release needs a separately reviewed build configuration:

1. Select a redistributable FFmpeg/codec configuration and verify its actual
   `-L` and `-buildconf` output.
2. For an LGPL distribution, follow FFmpeg's published requirements, including
   suitable linking/replacement arrangements, notices, and corresponding source.
3. If GPL components such as libx264 are enabled, meet the GPL obligations for
   the distributed combination. The first-party MIT grant remains available,
   but it is not the only license relevant to that binary.
4. Ship corresponding source/build information and required notices for the
   exact native libraries supplied with the application.

The prior macOS FFmpeg configuration differed from the Linux build. That does
not make the macOS bundle's complete redistribution obligations fulfilled;
each delivered artifact needs its own dependency and notice review.

Upstream references:

- [FFmpeg legal information and distribution checklist](https://ffmpeg.org/legal.html)
- [FFmpeg 7.0.2 license, including nonfree configuration](https://github.com/FFmpeg/FFmpeg/blob/n7.0.2/LICENSE.md)
- [x264 project](https://www.videolan.org/developers/x264.html)

## OpenSSL, GPU and platform components

- The Rust `openssl-src` build wrapper and the native OpenSSL sources have
  separate license materials. The resolved build uses OpenSSL 3.6.3, whose
  Apache-2.0 license/NOTICE obligations must be included where applicable.
- The `nvenc` Rust crate declares MIT. NVIDIA drivers, SDK materials and runtime
  components are not relicensed by that crate or this repository. Inspect the
  exact NVIDIA/header material included in any package.
- Apple SDKs/frameworks, Windows SDK/runtime components, Linux system libraries,
  GTK and WebKitGTK have their own terms. Do not copy an SDK or driver into a
  release merely because EclipticRD's source is MIT.
- Source licensing does not itself grant codec patent or trademark rights.

## Publication scope

This repository publication covers source and documentation. No existing private
application bundle, FFmpeg binary, signing identity or SDK is being published as
part of the repository visibility change. GitHub release assets and CI artifacts
must be reviewed separately before being promoted as binary releases.
