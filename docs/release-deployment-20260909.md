# Desktop and host release deployment — 2026-09-09

## Outcome and scope

Linux host, Windows host, and macOS desktop releases were built on Omarchy and
deployed. iPhone/iOS builds, installation, and device interaction were excluded.
No project commit or push was created.

The release used a frozen snapshot of the existing working tree, including its
uncommitted integration changes, rather than HEAD alone. All 282 recorded source
hashes matched both the local original and the remote build source. The directory
label `20260909-782e7af` is a release identifier, not a claim that the binaries
contain only that commit.

Evidence is under `.omo/release-deploy-20260909/`. That directory contains private
rollback/runtime artifacts and must not be committed wholesale.

## Installed targets

| Target | Installation and launcher |
| --- | --- |
| Linux | `/home/indo/.local/share/EclipticRD/releases/20260909-782e7af/erd-host`; enabled systemd **user** unit `erd-host.service` |
| Windows | `C:\erd\clients\rust\target\release\erd-host.exe`; existing `erd-host-run` scheduled task, VBS and batch launcher retained |
| macOS | `/Applications/EclipticRD.app`; ARM64 executable `Contents/MacOS/tauri-shell`; FFmpeg 7 libraries bundled in `Contents/Frameworks` |

Linux retains the existing user, home, Wayland display, DBus address, pairing
store, and `HDMI-A-2` output. Its previous launcher was a manual process; this
deployment changes it to an enabled user service. Windows retains its existing
interactive task principal and command arguments.

The Mac application retains bundle identifier `com.eclipticrd.shell` and version
`0.1.0`. It is ad-hoc signed, as was the previous installation, not notarized.
Its packaging declares macOS 12.0 minimum; older macOS versions were not tested.
Only packaging/signing and runtime execution occurred on the Mac; Rust, OpenSSL,
FFmpeg, and cross-toolchain compilation occurred on Omarchy.

### SHA-256

| Artifact | SHA-256 |
| --- | --- |
| Linux deployed host | `3b1b58140bbca43c52a253cf655c314e4f0ad68941138ccfde57e4c45b0f0de4` |
| Linux previous host | `7f8b5a152b73c82d6fd61fd7f30312c1b5eb690e19ce0f4d6f095cfa24bc4850` |
| Windows deployed host | `b2dca16aebc05258a43ddc1c0f797bc4fde82cff49e112457f9390a975a5aeec` |
| Windows previous host | `c46f1e78408e36e56333253451c39805c55e21a2cc2dd45087a386070cf48d85` |
| Mac installed executable, after signing | `52948a7ae822769994b890f64698bb66dd32d95bffb4bb617409651e47bbfb47` |
| Mac previous executable | `a37dcebe02cd64e2b2614337b82eaf1823ffdac2258687c7dec441a249404089` |
| Mac compiler output, before local signing | `77136af3eef0c2249e551b8d7786b8351de161047a1b7127e068c93f351b74d2` |

The Mac hashes differ before and after signing because signing changes the Mach-O
file. Installed and staged signed hashes matched.

## Verification

- Rust workspace tests, excluding `erd-ios`, passed on Omarchy
  (`host-builds-clean.log`, `RELEASE_TESTS_PASS`). One existing test was ignored.
- Linux, Windows GNU, and macOS ARM64 release builds completed.
- Desktop non-page tests passed: 28 Bun tests and 32 Node tests (`ui-tests.log`).
  Eleven additional WebView page tests passed (`ui-page-tests.log`). These page
  tests use deterministic native-IPC fixtures, not a live Tauri connection.
- Deployed Linux and Windows hosts each authenticated and decoded 30 frames using
  the Linux release client (`linux-stream.log`, `windows-stream.log`).
- The Mac release client reconnected with stored pairings, decoded **170 Linux
  frames** and **78 Windows frames**, and exited successfully after API disconnect
  (`macos-linux-reconnect.log`, `macos-windows-tailscale.log`).
- Both Mac client sessions returned real **3840 x 1600** PNG screenshots.
  Mouse-move API requests returned HTTP 200 with one event sent; disconnect
  returned HTTP 200 and released one tracked input. Windows's response and PNG
  digest are in `windows-api-evidence.json`; Linux's API response was observed in
  the session tool output.
- Windows TCP 19730 and UDP 19731 were owned by the deployed process; its running
  path and hash matched (`windows-final-check.log`).
- Linux `/proc/2505717/maps` showed FFmpeg loaded from the deployed release's
  `lib` directory, not the temporary builder (`linux-runtime-maps.txt`).
- The installed Mac app launched and remained observable at its installed path.
  Its loaded FFmpeg libraries came from the app bundle. New and rollback bundles
  passed `codesign --verify --deep --strict`.

**Verification limits:** Native Mac GUI visual inspection and GUI-driven streaming
were not completed; the `orca` CLI was unavailable (`command not found`).
Streaming was exercised through the actual release CLI/API, not by pretending a
browser fixture was the native app. Screenshot pixels could not be visually
assessed by the available model. Audio playback was not separately verified.

### Runtime observations, not a latency certification

| Mac receiver session | Observation |
| --- | --- |
| Linux over Tailscale | Receiver-reported loss ratio `0.2947530864`; decode p95 `10,795 us`; host encode p95 `102,232 us` |
| Windows over Tailscale | Receiver-reported loss ratio `0`; decode p95 `5,450 us`; host ready-to-encode p95 `946,181 us`; host encode p95 `28,310 us` |

These are short smoke-test samples, not end-to-end presentation latency.
Connectivity passed, but the loss and host-side waiting measurements do not
justify claiming healthy low-latency performance.

Direct Windows LAN address `192.168.0.60` returned `No route to host` from the Mac.
The final Windows session used the reachable Tailscale address `100.126.171.58`.
This run therefore does **not** certify same-LAN connectivity or discovery.

## Resolved build/deployment interruptions

- AppleDouble `._*` transfer metadata was removed from the private source copy
  after Tauri attempted to parse it as a capability JSON file.
- Linux GNU `strip` could not process Mach-O libraries; the osxcross cctools
  variant also rejected indirect symbols. LLVM `strip` succeeded while preserving
  imported symbols.
- OpenSSL initially selected native Linux `ranlib`; setting the target-specific
  macOS `RANLIB_aarch64_apple_darwin` completed the build.
- Windows's first replacement encountered a still-locked executable after
  termination. Automatic recovery restarted the previous binary. The corrected
  script waits on the process's actual exit before copying; replacement then
  passed. Both rollback copies have the same original hash.
- A late fresh PIN attempt was rejected after the host's existing five-minute
  bootstrap window expired. Reconnection with stored pairings succeeded; the
  authentication policy was not weakened.
- LSP diagnostics were unavailable because the LSP daemon was unreachable.
  Shell syntax, PowerShell parser, plist, build, test, and runtime checks supplied
  the verification described above. Omarchy also lacked `lsof`; Linux runtime
  mappings were checked through `/proc` instead.

## Rollback

Backups are retained:

- Linux: `/home/indo/.local/share/EclipticRD/releases/20260909-782e7af/rollback/`
  contains the original host and pairing file. The original executable also
  remains at `/home/indo/projects/erd-lan-20260909/deployment/erd-host`.
- Windows: `C:\erd\releases\20260909-782e7af\rollback\` contains the original host,
  pairing file, task XML, VBS, and batch launcher. `rollback-after-lock` is a second
  matching backup made before the successful retry.
- Mac: `~/Library/Application Support/EclipticRD-releases/20260909-782e7af/rollback/`
  contains `EclipticRD.app`, `application-support`, and `webkit`.

The Linux and Windows backup binaries passed `--help` on their respective hosts;
the Mac backup passed signature validation and matched its recorded hash.
Other than Windows's observed automatic recovery, rollback was **not** executed.

To revert Linux, stop and disable the new user unit, then launch the original
executable under the saved environment and original arguments. Required values
are `LD_LIBRARY_PATH=/home/indo/erd-ffmpeg7/lib`,
`XDG_RUNTIME_DIR=/run/user/1000`, `WAYLAND_DISPLAY=wayland-1`, and
`DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus`; working directory is
`/home/indo`, output is `HDMI-A-2`. The saved environment and launch details are in
`linux-runtime-environment.json`, `linux-before.log`, and `journal.md`.

To revert Windows, stop the process at the exact deployed executable path and
wait for its actual exit. Copy `rollback\erd-host.exe` back to that path and run
`Start-ScheduledTask -TaskName erd-host-run`. Existing launcher files were not
changed; their backups and task XML are available if separately needed.

To revert Mac, quit the current app and confirm its process has exited. Move the
new bundle aside, restore the backed-up `EclipticRD.app` to `/Applications`,
validate its signature, and launch it.

Do not routinely restore pairing or WebKit backups when reverting binaries:
doing so could discard legitimate user changes made after deployment.

## Retained artifacts and cleanup

The private builder at `/home/indo/projects/erd-release-20260909-nSnZkP/` was
reduced from **13,462,468 KiB to 159,788 KiB**, reclaiming about **12.7 GiB**.
Only this run's source build directory, FFmpeg build trees, osxcross build and
toolchain, duplicate SDK, and strip probe were removed (`cleanup.log`).

Retained there: platform binaries under `artifacts/`, FFmpeg runtime prefixes,
the FFmpeg 7.0.2 source archive, original source snapshot and hash manifest, and
`source-reproducible.tar.gz` containing the actual build source without targets.
The latter's SHA-256 is
`a83c4ab23ea34674e72d6141c50b62a610c27b5dcaec4c98db8669d8e7f27930`.
The Linux release directory also retains the release `erd-client`.

The signed Mac bundle and CLI, Windows executable, deployment scripts, test logs,
and runtime statistics remain under the local evidence directory. Existing
project changes, shared Cargo caches, previous deployments, and rollback data
were not removed.

The two pairings created exclusively for these smoke tests were revoked through
the deployed hosts after verification. Their local and remote key copies and
the unnecessary full Linux environment dump were deleted. Original user pairings
were retained; the local evidence directory is restricted to its owner.
