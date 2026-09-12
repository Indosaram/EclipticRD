# Phase A Task Implementation Report: R1 Role-Separated Default Credential Stores & Safe Legacy Client Migration

- Task ID: `st_01a0892d`
- Worker: `hephaestus`
- Base Commit: `71f8b05f5a0e9d53ce0249beb46ca864b7a836f8`
- Date: 2026-09-10
- Remote Test Target: `indo@100.91.254.71` (`/home/indo/projects/maho-pairing-20260910`)

---

## 1. Executive Summary

Implemented finding R1 from `contracts.md` and `docs/remote-connection-pairing-review-plan-20260910.md` strictly within the store and persistence seam without touching transport, authorization gating, or model names:
1. **Directional Separation of Default Credential Stores**:
   - Host default store: `host-authorizations.json` (temporary file: `host-authorizations.json.tmp`).
   - Client default store: `client-pairings.json` (temporary file: `client-pairings.json.tmp`).
   - Both reside under the standard platform `MahoRD` application data directory (`dirs::data_dir()` / `$XDG_DATA_HOME` / `HOME`), but never share filenames or temporary files.
2. **Safe Legacy Client Migration**:
   - If `client-pairings.json` does not exist and legacy `pairing-keys.json` is present in the application data directory, `maho-app::PairingStore::load_all()` automatically reads and validates the entries, writing them into `client-pairings.json`.
   - The legacy `pairing-keys.json` file is treated as strict read-only backup; it is never mutated or deleted by MahoRD.
   - Deletion from the migrated client store removes entries from `client-pairings.json` only, preserving `pairing-keys.json` untouched.
3. **Strict Host Isolation (No Auto-Import)**:
   - `maho-host::PairingStore` resolves strictly to `host-authorizations.json`. It never imports, reads, or trusts `pairing-keys.json`.
   - Inbound peers must obtain explicit operator authorization on first connection under the new system.
4. **Deterministic Deadlock-Free Subprocess Execution & Trust Isolation Assertions**:
   - In `maho-host/tests/pairing_isolation.rs`, `run_isolated_subprocess` uses direct `Command::status()` without mutex/wait-thread machinery, eliminating any deadlock potential between wait locks and termination signals.
   - Observable trust-mixing assertions run ahead of filename assertions: `host_store.load_all()` is asserted empty after client saves an outbound key.
   - Socket teardown is deterministic: the client writes a clean `ControlMessage::Disconnect` packet and drops the TLS stream *before* awaiting `server_done_rx.recv_timeout(Duration::from_secs(3))`.
   - Server receive completion assertions (`server_res` and `server2_res`) are explicitly asserted for expected status (`Err` on rejected TLS, `Ok` on clean disconnect) rather than ignored.
   - Zero unbounded joins; channel timeouts bounded to 3s with test completing in 0.27s.
   - In-module unit test `host_default_store_path_and_current_psks_isolation` in `maho-host/src/session.rs` verifies that `current_psks()` ignores legacy `pairing-keys.json` in isolated environments and only populates explicitly approved inbound records.

---

## 2. File Ownership and Changes

All changes adhered strictly to the assigned scope:

| File Path | Scope | Description |
|---|---|---|
| `clients/rust/maho-app/src/pairing.rs` | Assigned Production Source & Tests | Updated `default_path()` to `client-pairings.json`; added `legacy_default_path()`; implemented safe legacy migration in `load_all()`; added unit tests for paths, migration, preservation, and deletion. Record struct fields (`id, name, key, added_at_unix_ms`) were preserved unchanged. |
| `clients/rust/maho-host/src/session.rs` | Assigned Production Source (Store ONLY) | Added `PairingStore::default_path()` pointing to `host-authorizations.json`; updated `PairingStore::host_default()` to use `default_path()`. Strictly non-importing of legacy store. Added in-module unit test `host_default_store_path_and_current_psks_isolation`. Transport/auth-gating logic untouched. |
| `clients/rust/maho-host/Cargo.toml` | Assigned Dev-Dependency | Added `maho-app = { path = "../maho-app" }` under `[dev-dependencies]` for integration testing. |
| `clients/rust/maho-host/tests/pairing_isolation.rs` | Assigned New Integration Test | Real default-path isolated subprocess test with deadlock-free `Command::status()`, explicit clean socket teardown, non-ignored server receive assertions, trust-mixing isolation assertions preceding filename checks, and legacy migration invariants. |

Pre-existing baseline dirty files (`inject_macos.rs`, `main.rs`, `discovery_tests.rs`, `lib.rs`, `index.html`, etc.) were left completely intact.

---

## 3. Regression Failure (RED) Receipts Before Fix

Before modifying `maho-app/src/pairing.rs` and `maho-host/src/session.rs`, `pairing_isolation.rs` and `Cargo.toml` were executed on remote target `indo@100.91.254.71`.

### Exact Remote Command:
```bash
ssh -o BatchMode=yes indo@100.91.254.71 \
  "export PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig && \
   export LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:\$LD_LIBRARY_PATH && \
   cd /home/indo/projects/maho-pairing-20260910 && \
   cargo test --manifest-path clients/rust/Cargo.toml -p maho-host --test pairing_isolation"
```

### Exit Code:
`101`

### Failure Output:
```text
running 2 tests
test test_outbound_client_pairing_rejected_as_host_authorization ... FAILED
test test_legacy_pairing_keys_not_auto_imported_by_host_and_migrated_by_client ... FAILED

failures:

---- test_outbound_client_pairing_rejected_as_host_authorization stdout ----
thread 'test_outbound_client_pairing_rejected_as_host_authorization' panicked at maho-host/tests/pairing_isolation.rs:
Host authorization store must be empty and must not contain client outbound keys (trust mixing detected: [PairingRecord { id: "client-outbound-id-1", ... }])

---- test_legacy_pairing_keys_not_auto_imported_by_host_and_migrated_by_client stdout ----
thread 'test_legacy_pairing_keys_not_auto_imported_by_host_and_migrated_by_client' panicked at maho-host/tests/pairing_isolation.rs:
Host must NEVER auto-import records from legacy pairing-keys.json (trust mixing: [PairingRecord { id: "legacy-client-1", ... }])

test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
```

**Diagnostic Analysis**:
1. Client and host defaulted to the same file (`pairing-keys.json`), causing client outbound keys to immediately mix into host authorizations.
2. Host store loaded from `pairing-keys.json`, auto-importing legacy records into host authorization.

---

## 4. Implementation Details

### 4.1 Client Store Separation & Migration (`maho-app/src/pairing.rs`)
```rust
pub fn default_path() -> Result<PathBuf, PairingStoreError> {
    // macOS: ~/Library/Application Support/MahoRD/client-pairings.json
    // Windows: %APPDATA%\MahoRD\client-pairings.json
    // Linux/Unix: $XDG_DATA_HOME/MahoRD/client-pairings.json or ~/.local/share/MahoRD/client-pairings.json
    ...
    Ok(base.join("MahoRD").join("client-pairings.json"))
}

pub fn legacy_default_path() -> Result<PathBuf, PairingStoreError> {
    let default = Self::default_path()?;
    Ok(default.with_file_name("pairing-keys.json"))
}

pub fn load_all(&self) -> Result<Vec<PairingRecord>, PairingStoreError> {
    match &self.backend {
        StoreBackend::File(path) => {
            if !path.exists() {
                let legacy_path = path.with_file_name("pairing-keys.json");
                if legacy_path.exists() && legacy_path != *path {
                    let legacy_records = Self::read_file_records(&legacy_path)?;
                    if !legacy_records.is_empty() {
                        self.write_records(path, &legacy_records)?;
                        return Ok(legacy_records);
                    }
                }
            }
            Self::read_file_records(path)
        }
        ...
    }
}
```

### 4.2 Host Authorization Store Isolation (`maho-host/src/session.rs`)
```rust
impl PairingStore {
    pub fn default_path() -> Result<PathBuf, SessionError> {
        let directory = dirs::data_dir()
            .ok_or_else(|| SessionError::Store("Application Support is unavailable".into()))?
            .join("MahoRD");
        Ok(directory.join("host-authorizations.json"))
    }

    pub fn host_default() -> Result<Self, SessionError> {
        Ok(Self::new(Self::default_path()?))
    }
```

---

## 5. Verification (GREEN) Receipts

### 5.1 Verification Command 1: `pairing_isolation`
```bash
ssh -o BatchMode=yes indo@100.91.254.71 \
  "export PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig && \
   export LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:\$LD_LIBRARY_PATH && \
   cd /home/indo/projects/maho-pairing-20260910 && \
   cargo test --manifest-path clients/rust/Cargo.toml -p maho-host --test pairing_isolation"
```

#### Output:
```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 2.02s
     Running tests/pairing_isolation.rs (clients/rust/target/debug/deps/pairing_isolation-9d808868c5fab39c)

running 2 tests
test test_legacy_pairing_keys_not_auto_imported_by_host_and_migrated_by_client ... ok
test test_outbound_client_pairing_rejected_as_host_authorization ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.27s
```
**Exit Code**: `0`

### 5.2 Verification Command 2: `maho-app pairing`
```bash
ssh -o BatchMode=yes indo@100.91.254.71 \
  "export PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig && \
   export LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:\$LD_LIBRARY_PATH && \
   cd /home/indo/projects/maho-pairing-20260910 && \
   cargo test --manifest-path clients/rust/Cargo.toml -p maho-app pairing"
```

#### Output:
```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.29s
     Running unittests src/lib.rs (clients/rust/target/debug/deps/maho_app-fa73e0a6b217a4ff)

running 6 tests
test pairing::tests::test_client_default_path_filename ... ok
test pairing::tests::ephemeral_pairing_store_roundtrip_and_delete ... ok
test pairing::tests::test_temporary_file_naming_isolated ... ok
test pairing::tests::test_deletion_isolated_from_legacy ... ok
test pairing::tests::test_legacy_client_migration_skips_when_client_pairings_already_exists ... ok
test pairing::tests::test_legacy_client_migration_preserves_legacy_and_writes_client_pairings ... ok

test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 119 filtered out; finished in 0.00s

     Running tests/session_mock.rs (clients/rust/target/debug/deps/session_mock-54582c90b09275f5)

running 2 tests
test connect_with_pairing_direct_round_trip ... ok
test mock_server_pairing_handshake_and_input_round_trip ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 5 filtered out; finished in 0.22s
```
**Exit Code**: `0`

### 5.3 Verification Command 3: `maho-host --lib`
```bash
ssh -o BatchMode=yes indo@100.91.254.71 \
  "export PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig && \
   export LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:\$LD_LIBRARY_PATH && \
   cd /home/indo/projects/maho-pairing-20260910 && \
   cargo test --manifest-path clients/rust/Cargo.toml -p maho-host --lib"
```

#### Output:
```text
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.24s
     Running unittests src/lib.rs (clients/rust/target/debug/deps/maho_host-3d5d23414c23b8a6)

running 90 tests
...
test session::tests::host_default_store_path_and_current_psks_isolation ... ok
...
test result: ok. 90 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.94s
```
**Exit Code**: `0`

---

## 6. Audit Receipts

1. **Clean git status**: No commits made, only assigned files modified and report created.
2. **Pre-existing changes**: Cleanly preserved.
3. **Deadlock-free execution**: `Command::status()` executes directly without wait-lock contention, channel timeouts are bounded, server receive completions are explicitly asserted, and TLS streams are cleanly disconnected.
4. **Dev dependency**: `maho-app = { path = "../maho-app" }` added to `clients/rust/maho-host/Cargo.toml` under `[dev-dependencies]`.
