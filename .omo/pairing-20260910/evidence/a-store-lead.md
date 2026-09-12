# R1 lead verification

## Observed production delta

- Client default file renamed to `client-pairings.json` on macOS/Linux/Windows.
- Host default file renamed to `host-authorizations.json`.
- Client load migrates legacy records only when its new file is absent; legacy file is preserved.
- Host never auto-imports the legacy file.
- Client migration uses the existing save permissions/serialization path.

## Independent command

```bash
ssh -o BatchMode=yes indo@100.91.254.71 \
  'export PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig
   export LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:$LD_LIBRARY_PATH
   cd /home/indo/projects/maho-pairing-20260910 &&
   cargo test --manifest-path clients/rust/Cargo.toml -p maho-host --test pairing_isolation &&
   cargo test --manifest-path clients/rust/Cargo.toml -p maho-app pairing'
```

Monitor: `mon_KGECH0XAYV6XW867`, process `bash_9`, exit 0.

Observed: 2 host integration cases passed; 6 app pairing unit cases passed; 2 matching session integration cases passed. Several unrelated targets were filtered to zero tests and are not counted as coverage.

The host integration subprocess runs under isolated XDG data path. It checks outbound credentials are absent from the inbound store, exercises a real paired TLS attempt against the host, permits a separately approved inbound credential, and verifies deletion does not cross roles. Legacy migration preservation is separately checked. TempDir and joined worker lifetimes perform test cleanup.

## Review notes

Lead identified an open successful TLS stream kept alive before server-thread join and an ignored timeout followed by unbounded join; exact correction sent to producer. Final source inspection must confirm those corrections before closing R1.

Lead diagnostics attempted on `maho-app/src/pairing.rs`: unavailable because the shared LSP daemon is unreachable. Remote compilation in the command above passed.

R1 acceptance is not yet closed solely by green output: producer terminal status, final teardown source, and change provenance must be resolved before commit.

## Acceptance closure

The final harness now uses isolated `Command.status()` instead of a waiter holding the child mutex. Explicit protocol disconnect and checked completion signals precede host joins. The producer is terminal/completed in the phase A snapshot. The extra host consent and shell changes belong to the separate planned lanes, not R1; they are excluded from the R1 staged patch.

Lead reran `cargo test --manifest-path clients/rust/Cargo.toml -p maho-host --test pairing_isolation` on Omarchy: monitor `mon_W7GW0YWKM24Z3N86`, exit 0, 2/2 passed (inner subprocess results are not double-counted). Combined with the earlier app and session test results and original RED receipts in `reports/a-store.md`, R1 passes its acceptance.
