# EclipticRD Rust workspace

This is the active EclipticRD workspace, containing the host, desktop and iOS
applications, protocol and transport crates, media pipeline, and automation API.
It is no longer a client-only scaffold.

Start with the [project README](../../README.md) for platform status, native
dependencies, connection instructions, and verification limits.

## Build and test

Run from this directory after installing the native dependencies:

```sh
cargo build --locked --release -p erd-host -p erd-app
cargo build --locked --release -p tauri-shell --features tauri/custom-protocol
cargo test --locked --workspace --exclude erd-ios
```

Default desktop and headless-client builds **do require FFmpeg**. The verified
native dependency is FFmpeg 7.0.2; `ffmpeg-next` 8.1.0 is the Rust wrapper version,
not a requirement to install FFmpeg 8.

The iOS application has separate native tooling, signing and physical-device
requirements. Excluding `erd-ios` from the desktop test command does not establish
iOS correctness.

## Workspace members

`erd-proto`, `erd-net`, `erd-decode`, `erd-render`, `erd-app`, `erd-host`,
`erd-mobile`, `tauri-shell`, and `ios-shell`.

The nested `tauri-shell/src-tauri/Cargo.toml` is not the primary workspace entry;
build the `tauri-shell` package from this workspace.

## Licensing

Original project source: [MIT](../../LICENSE).
Native dependencies and Rust packages retain their own terms:
[third-party notices](../../THIRD_PARTY_NOTICES.md) and
[dependency inventory](../../docs/dependency-licenses.md).

The pinned Omarchy FFmpeg script is a private, nonfree build and is not a public
binary distribution recipe.
