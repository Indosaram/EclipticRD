# R3 real-surface RED scenario

This is lead-authored QA scaffolding only. Product implementation remains delegated to Gemini 3.8 Flash. The wrapper uses the existing real `maho_host::HostServer` and Linux capture backend; it changes no production source and binds TCP/UDP to loopback ephemeral ports with a temporary store and disabled bootstrap/audio.

## Exact entry points

Build only on Omarchy:

```bash
PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig \
LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib \
CARGO_TARGET_DIR=/home/indo/projects/maho-pairing-20260910/clients/rust/target \
cargo build --manifest-path .omo/pairing-20260910/qa/native-host/Cargo.toml
```

The controller starts `maho-pairing-qa-host <mktemp>/host.json`, subscribes to its `QA_READY <tcp> <udp>` output, then launches the real binary:

```text
maho-client --host 127.0.0.1 --tcp-port <observed tcp>
  --udp-port <observed host UDP or controller UDP gate>
  --pairing-id qa-r3-registration --psk-hex <32 bytes of 0x52 in hex>
  --frames 1 --timeout-secs 5 --client-name isolated-r3-qa
```

Baseline: direct UDP, exit 0 and `Reached requested target frame count` with `decoded_frames=1`.

Adversarial case: intercept the real client's initial registration at a UDP gate. This event occurs after the native TCP handshake, avoiding an authentication timing guess. Before forwarding that registration, send a malformed three-byte packet from a separate loopback UDP socket to the actual host UDP port. Forward legitimate traffic between gate and client normally afterward.

PASS after fix: the native client decodes one frame and the malformed sender receives no media.
RED before fix: the malformed sender receives encrypted media while the native client fails to reach one decoded frame. Baseline must pass first; a generic timeout alone is not RED evidence.

Capture process output, native frame result, malformed-sender packet count/bytes and exact selected ports. Subscribe before triggering actions; all process/event waits have bounded deadlines. Close UDP sockets, terminate/join only the isolated process group, and remove the temporary credential directory in `finally`.
