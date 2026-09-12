# Host authorization and PIN lead verification

## Source review

Read the actual host packet gating, connection-local `granted_pairing_id`, handshake identity checks, and input/media acceptance paths. Bootstrap connections cannot handshake before consent and can use only their granted ID. Paired TLS still requires an application handshake before input/media. Repeated authenticated handshakes are rejected. Default PIN uses the existing random generator through a deterministic selection seam; explicit PIN behavior is preserved.

Read the actual loopback tests for no-consent rejection, mismatched granted ID, normal consent/paired success, and duplicate handshake. Read the injected-generator PIN tests. The tests assert server errors, acknowledgment types, and media-start counts rather than only function-call mocks or random inequality.

## Independent verification

- `mon_VMHEKXTB5EDK08M8`: remote host bootstrap/PIN targets, exit 0.
- `mon_H2A2RASDES0BFQ1T`: remote shell `pairing_tests`, 5 passed, exit 0.
- `mon_SF43AYS1VQTTA8HH`: broad related tests, exit 0.
- `mon_VP63H6483P3P08ZM`: explicitly synchronized final local sources, then reran the exact broad command below; exit 0.

```bash
ssh -o BatchMode=yes indo@100.91.254.71 \
  'export PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig
   export LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:$LD_LIBRARY_PATH
   cd /home/indo/projects/maho-pairing-20260910 &&
   cargo test --manifest-path clients/rust/Cargo.toml \
     -p maho-host -p maho-app -p tauri-shell'
```

Final related run: 340 tests and 3 doc tests passed; 0 failed. Isolated subprocess sub-results are not counted twice. One existing ignored test remains: `discovery_tests::observe_installed_tailscale`, which explicitly requires `--ignored --nocapture`. No skips or weakened assertions were added. The `paired_tls` name filter in an earlier command selected zero tests and is not counted; paired-TLS behavior is covered by the actual named bootstrap success/duplicate scenarios.

The commands compile and exercise current sources on Omarchy. Shared LSP is unavailable (`LSP daemon unreachable`); no clean LSP result is claimed.

## Cleanup and boundary

Loopback test listeners, worker threads and isolated data directories are owned by tests and closed/joined/dropped at completion. The broad runner process exited 0. No production host was restarted and no permanent deployment occurred.

This verifies the host increment, not the full R1-R11 plan. Flash quota prevented the independent phase-A worker gate and all later phases. Shell IPC changes remain in the working tree because their integration hunk overlaps pre-existing discovery work; they are not included in the host-only commit.
