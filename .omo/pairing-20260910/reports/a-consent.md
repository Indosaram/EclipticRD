# Phase A Implementation & Reconciliation Report: R2 Bootstrap Consent/ID Binding & R11 Host Random Default PIN

- Date: 2026-09-10
- Active Task ID: `st_01a08938`
- Prior Task ID: `st_01a0892a`
- Parent Session: `01a0890a-69f1-7e5e-80cb-959dc3ddb61c`
- Root Session: `01a0890a-69f1-7e5e-80cb-959dc3ddb61c`
- Target Systems: `clients/rust/maho-host/src/session.rs`, `clients/rust/maho-host/src/main.rs`
- Reference Contracts: `.omo/pairing-20260910/contracts.md` (Sections 1.2, 1.9, 4.1, 4.2, 4.3, 10.1, Amendments 3 & 4)
- Base Commit: `71f8b05f5a0e9d53ce0249beb46ca864b7a836f8`
- Execution Environment: Omarchy Linux (`indo@100.91.254.71`) at `/home/indo/projects/maho-pairing-20260910` with `PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig` and `LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:$LD_LIBRARY_PATH`

---

## 1. Executive Summary & Provenance Reconciliation

This report documents the verification, gap completion, and audit reconciliation for findings **R2** (Cryptographic Binding of Bootstrap Consent) and **R11** (Secure Random Default PIN Policy).

### 1.1 Provenance and Task Reconciliation
- **Baseline State from `st_01a0892a`:** The implementation was initiated under `st_01a0892a`, establishing core session gating structures in `session.rs` and the injection seam in `main.rs`, along with pre-existing RED logs in `.omo/pairing-20260910/evidence/st_01a0892a-red-regression.log`.
- **Audit Findings in `st_01a08938`:**
  1. In `main.rs`, the production invocation `main()` was invoking `select_pin(..., || "12345678".to_string())` with `auto_approve || !io::stdin().is_terminal()`. While unit tests tested `select_pin` via mock closures, runtime production execution was still hardcoded to `"12345678"`. This was corrected to `select_pin(cli.bootstrap_pin, cli.pin.as_deref(), random_pin)?` with `auto_approve = cli.auto_approve`.
  2. In `session.rs`, `test_bootstrap_without_consent_handshake_rejected_with_no_capture_or_input` verified unconsented handshake rejection and zero media starts, but did not assert on pre-handshake control/input packet rejection. The test was enhanced to transmit unauthenticated `ControlMessage::Ping` and `InputEvent::MouseMove` packets over loopback TLS prior to the handshake, verifying that the host refuses both packets without emitting `HandshakeAck`, `Control/Pong`, or `InputAck`.
  3. Pre-existing RED evidence was verified against disk artifacts (`st_01a0892a-red-regression.log`), and independent scoped mutation proofs were executed under `st_01a08938` confirming failure of all tests under defect states.

---

## 2. Red Regression Verification & Mutation Proofs

### 2.1 Provenance RED Artifacts (`st_01a0892a`)
Stored in `.omo/pairing-20260910/evidence/st_01a0892a-red-regression.log`:
- R11 unpatched binary panicked at `maho-host/src/main.rs:241:9` (`assertion left == right failed: default invocation must invoke generator, left: 0, right: 1`).
- R2 unpatched session panicked on all three scenarios:
  - `test_bootstrap_authenticated_session_rejects_duplicate_handshake`: panicked at `maho-host/src/session.rs:4589:9: server must reject duplicate handshake with AlreadyAuthenticated, got: Ok(())`.
  - `test_bootstrap_without_consent_handshake_rejected_with_no_capture_or_input`: panicked at `maho-host/src/session.rs:4332:9: server must reject unconsented bootstrap handshake with PreAuth, got: Ok(())`.
  - `test_bootstrap_consent_b_cannot_use_a`: panicked at `maho-host/src/session.rs:4412:9: server must reject handshake with mismatched pairing ID with IdentityMismatch, got: Ok(())`.

### 2.2 Independent Mutation Proofs (`st_01a08938`)
Executed on remote Omarchy builder (`indo@100.91.254.71`):

1. **R11 Default PIN Seam Mutation:**
   - Mutated `select_pin` default case to return `"12345678"`.
   - Command:
     ```bash
     ssh indo@100.91.254.71 "cd /home/indo/projects/maho-pairing-20260910 && PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:$LD_LIBRARY_PATH cargo test --manifest-path clients/rust/Cargo.toml -p maho-host pin"
     ```
   - Exit code: `101`
   - Output:
     ```text
     ---- tests::test_pin_default_selects_injected_generator_branch stdout ----
     thread 'tests::test_pin_default_selects_injected_generator_branch' panicked at maho-host/src/main.rs:243:9:
     assertion `left == right` failed: default invocation must invoke generator
       left: 0
      right: 1
     ```

2. **R2 Consent Binding & Identity Mismatch Mutation:**
   - Mutated `session.rs` to bypass `granted_pairing_id` checks on bootstrap connections.
   - Command:
     ```bash
     ssh indo@100.91.254.71 "cd /home/indo/projects/maho-pairing-20260910 && PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:$LD_LIBRARY_PATH cargo test --manifest-path clients/rust/Cargo.toml -p maho-host bootstrap"
     ```
   - Exit code: `101`
   - Output:
     ```text
     ---- session::tests::test_bootstrap_consent_b_cannot_use_a stdout ----
     thread 'session::tests::test_bootstrap_consent_b_cannot_use_a' panicked at maho-host/src/session.rs:4508:9:
     server must reject handshake with mismatched pairing ID with IdentityMismatch, got: Ok(())

     ---- session::tests::test_bootstrap_without_consent_handshake_rejected_with_no_capture_or_input stdout ----
     thread 'session::tests::test_bootstrap_without_consent_handshake_rejected_with_no_capture_or_input' panicked at maho-host/src/session.rs:4423:9:
     server must reject unconsented bootstrap handshake with PreAuth, got: Ok(())
     ```

3. **R2 Duplicate Handshake Mutation:**
   - Mutated `session.rs` to allow `PacketType::Handshake` in `SessionState::Authenticated` without returning `AlreadyAuthenticated`.
   - Command:
     ```bash
     ssh indo@100.91.254.71 "cd /home/indo/projects/maho-pairing-20260910 && PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:$LD_LIBRARY_PATH cargo test --manifest-path clients/rust/Cargo.toml -p maho-host test_bootstrap_authenticated_session_rejects_duplicate_handshake"
     ```
   - Exit code: `101`
   - Output:
     ```text
     ---- session::tests::test_bootstrap_authenticated_session_rejects_duplicate_handshake stdout ----
     thread 'session::tests::test_bootstrap_authenticated_session_rejects_duplicate_handshake' panicked at maho-host/src/session.rs:4690:9:
     server must reject duplicate handshake with AlreadyAuthenticated, got: Ok(())
     ```

---

## 3. Implementation Details

### 3.1 `clients/rust/maho-host/src/main.rs`
- **PIN Selection Function:**
  ```rust
  pub fn select_pin<F>(
      bootstrap_pin: Option<String>,
      pin_opt: Option<&str>,
      mut generator: F,
  ) -> Result<String>
  where
      F: FnMut() -> String,
  {
      match (bootstrap_pin, pin_opt) {
          (Some(pin), None) => validate_pin(pin),
          (None, Some("generate")) | (None, None) => Ok(generator()),
          (None, Some(other)) => bail!("--pin accepts only 'generate', got '{other}'"),
          (Some(_), Some(_)) => unreachable!("clap enforces conflicts"),
      }
  }
  ```
- **Main Production Wiring:**
  ```rust
  let pin = select_pin(cli.bootstrap_pin, cli.pin.as_deref(), random_pin)?;
  ```
- **CLI Options:**
  - `cli.auto_approve` retained directly without background non-terminal auto-approval bypass.
  - `--bootstrap-pin <8-digits>` accepts explicit PIN for automated testing/headless environments.
  - `--pin generate` generates a fresh random PIN via `generator`.
- **Unit Tests:**
  - `test_pin_default_selects_injected_generator_branch`: proves default invocation executes generator seam.
  - `test_pin_generate_flag_selects_injected_generator_branch`: proves `--pin generate` executes generator seam.
  - `test_pin_explicit_bootstrap_pin_bypasses_generator`: proves explicit PIN bypasses generator seam.
  - `test_pin_validation_accepts_8_digits_and_rejects_invalid`: bounds format validation (8 decimal digits).
  - `test_random_pin_retained_and_format_valid`: validates `random_pin()` format without probabilistic inequality.

### 3.2 `clients/rust/maho-host/src/session.rs`
- **Typed Error Domain:**
  ```rust
  #[error("session is already authenticated")]
  AlreadyAuthenticated,
  ```
- **State Machine Rules:**
  ```rust
  Self::Authenticated => {
      matches!(packet_type, PacketType::InputEvent | PacketType::Control)
  }
  ```
- **Connection-Local Consent & ID Binding:**
  - Added connection-local grant: `let mut granted_pairing_id: Option<String> = None;`.
  - In packet loop:
    ```rust
    if !state.allows(header.packet_type) {
        debug!(?state, ?header.packet_type, "refusing packet for current state");
        if state == SessionState::Authenticated && header.packet_type == PacketType::Handshake {
            return Err(SessionError::AlreadyAuthenticated);
        }
        continue;
    }
    ```
  - Operator approval in `PacketType::PairingRequest` sets:
    `granted_pairing_id = Some(record.id.clone());`
    `state = SessionState::PairingGranted;`
  - In `PacketType::Handshake`:
    - Checks `state == SessionState::Authenticated` -> `SessionError::AlreadyAuthenticated`.
    - Negotiated bootstrap identity requires `granted_pairing_id` to match `handshake.pairing_id` exactly:
      ```rust
      if let Some(identity_pairing_id) = negotiated_identity.strip_prefix(PAIRING_IDENTITY_PREFIX) {
          if identity_pairing_id != handshake.pairing_id {
              return Err(SessionError::IdentityMismatch);
          }
      } else if negotiated_identity == BOOTSTRAP_IDENTITY {
          let Some(expected_id) = &granted_pairing_id else {
              return Err(SessionError::PreAuth);
          };
          if *expected_id != handshake.pairing_id {
              return Err(SessionError::IdentityMismatch);
          }
      } else {
          return Err(SessionError::IdentityMismatch);
      }
      ```
  - Reordered media pipeline start before `HandshakeAck` dispatch:
    ```rust
    let (media_tx, media_rx) = mpsc::sync_channel(16);
    media_handle = Some(self.media_source.start(media_tx)?);
    media_receiver = Some(media_rx);
    send_tcp_packet(&mut stream, PacketType::HandshakeAck, &ack.encode()?)?;
    ```
- **Loopback TLS Test Suite:**
  - `test_bootstrap_without_consent_handshake_rejected_with_no_capture_or_input`: unconsented client connecting via bootstrap TLS transmits unauthenticated `ControlMessage::Ping` and `InputEvent::MouseMove` (refused by server) followed by `Handshake("PAIR_A")`; server terminates with `SessionError::PreAuth`, media starts == 0, and asserts neither `HandshakeAck` nor `Control/InputAck` are emitted.
  - `test_bootstrap_consent_b_cannot_use_a`: client obtains consent for "client-B", but sends `Handshake("PAIR_A")`; rejected with `SessionError::IdentityMismatch`, media starts == 0.
  - `test_bootstrap_normal_b_and_paired_a_work`: validates normal approved bootstrap B connects and gets `HandshakeAck` with media started; and paired client A connecting via pairing TLS PSK connects and gets `HandshakeAck` with media started.
  - `test_bootstrap_authenticated_session_rejects_duplicate_handshake`: authenticated session sends second Handshake packet; rejected with `SessionError::AlreadyAuthenticated`, media capture not restarted.

---

## 4. Green Verification (Final Results)

All tests executed independently under `st_01a08938` on remote host `indo@100.91.254.71` with `PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig` and `LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:$LD_LIBRARY_PATH`.

### 4.1 Bootstrap Test Suite
```bash
ssh indo@100.91.254.71 "cd /home/indo/projects/maho-pairing-20260910 && PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:$LD_LIBRARY_PATH cargo test --manifest-path clients/rust/Cargo.toml -p maho-host bootstrap"
```
**Exit Code:** `0`
**Output:**
```text
     Running unittests src/lib.rs (clients/rust/target/debug/deps/maho_host-3d5d23414c23b8a6)

running 7 tests
test session::tests::lockout_disables_bootstrap_after_five_failures ... ok
test session::tests::bootstrap_kdf_runs_once_per_unchanged_pairing_window ... ok
test session::tests::cached_bootstrap_obeys_lockout_expiry_and_pairing_revocation ... ok
test session::tests::test_bootstrap_authenticated_session_rejects_duplicate_handshake ... ok
test session::tests::test_bootstrap_consent_b_cannot_use_a ... ok
test session::tests::test_bootstrap_without_consent_handshake_rejected_with_no_capture_or_input ... ok
test session::tests::test_bootstrap_normal_b_and_paired_a_work ... ok

test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 83 filtered out; finished in 0.52s

     Running unittests src/main.rs (clients/rust/target/debug/deps/maho_host-63f9d0457be7c696)

running 1 test
test tests::test_pin_explicit_bootstrap_pin_bypasses_generator ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.00s
```

### 4.2 PIN Test Suite
```bash
ssh indo@100.91.254.71 "cd /home/indo/projects/maho-pairing-20260910 && PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:$LD_LIBRARY_PATH cargo test --manifest-path clients/rust/Cargo.toml -p maho-host pin"
```
**Exit Code:** `0`
**Output:**
```text
     Running unittests src/main.rs (clients/rust/target/debug/deps/maho_host-63f9d0457be7c696)

running 5 tests
test tests::test_pin_validation_accepts_8_digits_and_rejects_invalid ... ok
test tests::test_pin_default_selects_injected_generator_branch ... ok
test tests::test_pin_explicit_bootstrap_pin_bypasses_generator ... ok
test tests::test_random_pin_retained_and_format_valid ... ok
test tests::test_pin_generate_flag_selects_injected_generator_branch ... ok

test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

### 4.3 Full `maho-host` Package Suite
```bash
ssh indo@100.91.254.71 "cd /home/indo/projects/maho-pairing-20260910 && PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:$LD_LIBRARY_PATH cargo test --manifest-path clients/rust/Cargo.toml -p maho-host"
```
**Exit Code:** `0`
**Results:**
- Unittests `src/lib.rs`: 90 passed, 0 failed
- Unittests `src/main.rs`: 5 passed, 0 failed
- Integration `tests/linux_audio_selection.rs`: 6 passed, 0 failed
- Integration `tests/pairing_isolation.rs`: 2 passed, 0 failed
- Doc-tests: 3 passed, 0 failed
- Total: **106 passed, 0 failed**.

### 4.4 Clippy Verification
```bash
ssh indo@100.91.254.71 "cd /home/indo/projects/maho-pairing-20260910 && PKG_CONFIG_PATH=/home/indo/maho-ffmpeg7/lib/pkgconfig LD_LIBRARY_PATH=/home/indo/maho-ffmpeg7/lib:$LD_LIBRARY_PATH cargo clippy --manifest-path clients/rust/Cargo.toml -p maho-host --all-targets --no-deps -- -D warnings"
```
**Exit Code:** `0`
**Status:** Clean (0 warnings, 0 errors).

---

## 5. Scope & Boundary Checklist

- [x] Provenance reconciled between prior `st_01a0892a` artifacts and active `st_01a08938` execution.
- [x] Only assigned files edited: `clients/rust/maho-host/src/session.rs`, `clients/rust/maho-host/src/main.rs`.
- [x] No edits to `maho-app`, `tauri-shell`, or `ios-shell`.
- [x] Existing `random_pin()` implementation retained.
- [x] Injected generator seam used for default PIN proof without probabilistic inequality assertions.
- [x] Production `main()` default PIN uses `random_pin`.
- [x] Connection-local bootstrap consent bound to granted pairing ID.
- [x] Unconsented Handshake rejected with no capture or input started; pre-handshake control and input frames refused.
- [x] Consent B cannot use ID A.
- [x] Duplicate handshake rejected on authenticated session.
- [x] Actual loopback TLS and explicit event synchronization used throughout tests.
- [x] Upstream R1 store changes preserved (`host-authorizations.json`).
- [x] Zero git commits created.
