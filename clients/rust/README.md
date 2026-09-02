# EclipticRD Rust Client

This directory contains the Rust client workspace scaffold.

## Layout

- `erd-proto`: wire protocol types, framing, and cryptographic primitives
- `erd-net`: network transport built on `erd-proto`
- `erd-decode`: media decoding built on `erd-proto`; FFmpeg support is optional
- `erd-render`: video/audio presentation built on `erd-proto`
- `erd-app`: application orchestration over the protocol, network, decode, and render crates

The dependency direction is `erd-proto <- erd-net <- erd-app`. The decode and render crates depend only on the protocol layer among workspace crates.

## Build and test

Install the Rust toolchain with [rustup](https://rustup.rs/), then run:

```sh
cd clients/rust
cargo build --workspace
cargo test --workspace
```

The default build does not require FFmpeg. To compile the optional FFmpeg integration, install FFmpeg development libraries for your platform and run:

```sh
cargo build -p erd-decode --features ffmpeg
```
