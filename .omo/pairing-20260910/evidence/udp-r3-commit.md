# R3 coordinator acceptance and isolated commit proof

## Implemented contract

Capability bit 7 is mandatory on both handshake sides. Native default configuration and the actual CLI advertise it. A peer without support fails before Ready/media startup; no plaintext fallback exists.

The client sends a sealed, empty Ping using its current client-to-host cipher and retains that cipher for subsequent sends. The host authenticates and validates registration before selecting an endpoint, freezes that endpoint, and limits receive work per session-loop pass.

The commit excludes the concurrent `serve_with_stop` API, its polling loop/test, shell hosting/UI changes, and unrelated discovery work. It includes three separately reviewed equivalent `maho-app` lint corrections required by the strict gate: collapse the token comparison, remove a unit-value binding, and copy the input-ACK value without cloning.

## Actual evidence

- Original coordinator real-surface RED: `r3-live-red.json`. Direct baseline decoded one frame; a three-byte unauthenticated sender received 517 encrypted packets / 492514 bytes while the legitimate client decoded zero frames.
- Coordinator real-surface GREEN: `r3-live-lead-green.json`, monitor `mon_T49CGNB8XND8KZ84`, exit 0. Both baseline and adversarial legitimate clients decoded a real 3840x1600 HEVC frame. Rogue packet/byte counts were both zero. Both binary hashes were unchanged during the run.
- Exact requested Rust gate and cleanup receipt: `udp-lead-green.log`, monitor `mon_2K7WD8XW2NBDSEAC`, exit 0. The host/app `udp` filter selected 10 actual tests, including prior-session rejection, invalid endpoint selection, both missing-capability directions and subsequent client nonce progression. The net `udp` filter selected 23 tests.
- Earlier protocol filters selected zero tests and are not counted as proof. The exact command below was subsequently read from the actual test source and run by the coordinator; monitor `mon_2ZJA13M4TS45XQ0X` selected one test and passed:

```bash
cargo test --manifest-path clients/rust/Cargo.toml -p maho-proto \
  --test protocol_v3 capabilities_authenticated_udp_registration_round_trip -- --exact
```

## Exact staged-tree gate

The initial staged tree passed tests but failed strict Clippy on three existing app lints; `udp-index-initial.log` preserves that exit 101 rather than hiding it. Flash corrected only those three sites.

The coordinator exported Git index tree `49b557eac865c928b906bee6a739d92fecdb1fb2` directly, without unstaged changes or normalized file copies, into the owned Omarchy `.omo/r3-index-check` directory. Monitor `mon_9CXH0MXP7HQW91VW` then ran:

```bash
cargo test --manifest-path clients/rust/Cargo.toml \
  -p maho-proto -p maho-net -p maho-host -p maho-app
cargo clippy --manifest-path clients/rust/Cargo.toml \
  -p maho-proto -p maho-net -p maho-host -p maho-app --no-deps -- -D warnings
```

All selected test binaries and doc tests passed; strict Clippy passed; combined exit 0. Full per-suite output is in `udp-index-green.log`. No lint was suppressed or failing test removed.

The live controller closed its UDP sockets and owned process groups, and removed both temporary credential stores. A separate SSH check verified all four PIDs and both directories absent. CUDA acceleration remained unavailable in this testbed; real-frame encoding fallback succeeded. This is registration/security evidence, not a hardware-performance claim.

The concurrent shell fixed-PIN/automatic-consent policy remains a separately tracked R11 conflict. This R3 acceptance does not resolve it or claim completion of phases C-E and the final personal review.
