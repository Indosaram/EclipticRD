# Optimized FFmpeg 7 on Omarchy

Run builds only on the Omarchy Linux x86_64 builder. The scripts create a private
prefix; they do not replace `/home/indo/maho-ffmpeg7`, install system packages or
restart a deployed service.

```sh
bash scripts/build-ffmpeg-omarchy.sh \
  /home/indo/maho-ffmpeg7-optimized \
  /home/indo/build-ffmpeg7-optimized
bash scripts/with-ffmpeg7.sh /home/indo/maho-ffmpeg7-optimized \
  cargo build --manifest-path clients/rust/Cargo.toml --release -p maho-host
bash scripts/with-ffmpeg7.sh /home/indo/maho-ffmpeg7-optimized \
  clients/rust/target/release/maho-host --help
```

Both build paths must be new, separate absolute paths. Failed partial builds remain
for diagnosis; choose fresh paths after correcting the cause. `MAHO_BUILD_JOBS`
controls parallel compilation (default 6).

The script pins FFmpeg 7.0.2, NVIDIA codec headers n12.1.14.0 and NASM 3.02 with
SHA-256 checks. NASM is extracted inside the private build tree, not installed.
Checksums were computed from the explicit HTTPS upstream/archive URLs in the script.
Installed development dependencies are pinned to the verified Omarchy versions:
x264 0.165.3222, libva 1.24.0 and libdrm 2.4.134. A different version fails the build
until its pin and verification evidence are deliberately updated.

Prerequisites are GCC, Make, pkg-config, curl, tar, bsdtar and those development
libraries. The prefix supports VAAPI, NVENC, x264 fallback and FFmpeg's H.264/HEVC
decoders. NVENC compilation does not imply an NVIDIA device exists at runtime.
GPL/nonfree configure flags match the enabled encoders; this is a private build,
not a redistributable packaging recipe.

`build-receipt.txt` records source hashes, compiler/assembler versions, dependency
versions, configure flags and loaded libraries. Source/options/dependency versions
are reproducible; bit-identical output across compiler or system-library changes
is not claimed.

Always use the wrapper for both compilation and execution. It verifies FFmpeg
major/minor versions, pkg-config origin, x86 assembly and prefix runtime resolution
before invoking the command. It does not silently substitute the system FFmpeg.
A Cargo artifact built against another prefix should use a separate
`CARGO_TARGET_DIR` when switching dependency builds.

The performance reason for `--enable-x86asm` is documented in
`.omo/scaler-causality-20260909/report.md`: switching only libswscale changed the
tested scaler prototype from roughly 54 ms to 18.4 ms. That does not itself make the
prototype faster than the existing scalar conversion; retain the production path
unless separate native performance and pixel checks justify a replacement.
