# LAN discovery implementation and physical iPhone deployment

## Result

Implemented standard DNS-SD/mDNS discovery, independent of Tailscale, across
the host, desktop client and iOS shell. Linux and Windows development hosts were
restarted with the new implementation. The updated `com.eclipticrd.ios` app was
installed and launched on the physical iPhone 16 Plus.

**Physical iPhone host-list display, card-to-host authentication, video, audio
and remote input have not been verified.** Web Inspector is disabled on the
phone, and the image inspection tools were unavailable. A real phone screenshot
was captured but its content was not independently read.

No simulator/emulator, git commit, App Store release or Android deployment was
performed. This task updates the existing iOS app, not a new Android application.

## Design

- Service: `_erd._tcp.local.`, SRV port matching the actual TCP listener.
- TXT metadata: protocol version 3, name, OS and UDP port. No PIN or pairing key.
- Apple browser: system DNSService APIs, persistent resolution/address callbacks,
  removal handling, scoped addresses, permission errors and cancellable workers.
- Linux/Windows: `mdns-sd`, with interface filtering and daemon shutdown.
- Mobile: nearby-host cards, refresh, empty/error states, current-address selection,
  explicit PIN entry for unauthenticated LAN records, custom-port forwarding and
  foreground/background cancellation. Direct address connection is preserved.
- Desktop: merges independent LAN and optional Tailscale observations. Tailscale
  failure does not hide LAN results. Discovery is not proof of authentication.
- No hardcoded host cards, subnet scanning or new custom discovery UDP port.

Bonjour was selected instead of the earlier raw UDP broadcast proposal because
Apple requires the restricted multicast entitlement for raw broadcast/multicast.
Browsing declared Bonjour service types via system APIs avoids that requirement;
the app still requires the user's local-network permission.

Sources:
- https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy
- https://docs.rs/mdns-sd/latest/mdns_sd/

## Verification

All non-iOS compilation/tests ran on Omarchy, in the isolated source workspace
`/home/indo/projects/erd-lan-20260909`. Only physical `aarch64-apple-ios` compilation
and Xcode signing/packaging ran on Mac.

| Check | Observed result |
| --- | --- |
| Initial network RED | Missing `erd_net::discovery`, compiler exit 101 |
| Initial mobile RED | 0 pass, 14 fail |
| Final mobile UI tests | 53 pass, 0 fail across 7 files |
| Desktop UI tests | 56 pass, 0 fail across 4 files |
| iOS Rust unit tests on Linux | 15 pass |
| Network Rust unit tests | 46 pass |
| Discovery metadata integration tests | 18 pass |
| Host Rust unit tests | 70 pass |
| Host integration tests | 6 pass |
| Desktop Rust unit tests | 50 pass, 1 existing live-surface test ignored |
| Linux build | `erd-host`, `erd-net`, `tauri-shell`, `erd-ios` successful |
| Windows cross-build on Omarchy | `x86_64-pc-windows-gnu` release successful |
| Physical iOS compile | Successful |
| iOS signed build and IPA export | Successful after Swift library-path correction |
| Signed app verification | `codesign --verify --deep --strict` successful |
| Change whitespace check | `git diff --check` successful for affected tracked paths |

LSP diagnostic requests for network, host, desktop and iOS failed because the
local LSP daemon was unreachable. Compiler/test results are the available
substitute, not a claim of successful LSP validation.

Two pre-existing Linux injection warnings remain: unused `ABSOLUTE_AXIS_MAX`
and `scale_to_uinput`. This discovery change did not modify those definitions.
New discovery compiler errors and warnings were fixed, not suppressed.

The first desktop test run could not load `libavutil.so.59`; setting
`LD_LIBRARY_PATH=/home/indo/erd-ffmpeg7/lib` resolved the environment failure.
An initial iOS link failed on Swift compatibility symbols because the old
`TOOLCHAIN_DIR` resolved to a Metal toolchain. The ARM64 project now uses the
selected Xcode's `XcodeDefault.xctoolchain` Swift library directory.

## Real deployment and LAN evidence

### Windows

- Installed binary: `C:\erd\clients\rust\target\release\erd-host.exe`.
- Previous binary backup: `C:\erd\erd-host-before-lan.exe`.
- Existing scheduled task `erd-host-run` and its authentication configuration
  were preserved. New observed process: PID 20376.
- Added `EclipticRD-LAN-mDNS`, inbound UDP 5353, restricted to LocalSubnet and
  this executable.
- Host log confirms LAN advertisement and listeners on TCP 19730 / UDP 19731.
- Mac `dns-sd -B _erd._tcp local.` observed `DESKTOP-1LAPJMP`.
- `dns-sd -L DESKTOP-1LAPJMP _erd._tcp local.` resolved
  `DESKTOP-1LAPJMP.local.:19730`, interface 14, with
  `protocol=3 udp_port=19731 os=windows name=DESKTOP-1LAPJMP`.
- TCP connection to actual LAN address `192.168.0.60:19730` succeeded.

This proves a real host advertises and is reachable over LAN; it does not prove
the iPhone displayed or connected to that host.

### Linux

- Preserved the running host's arguments and graphical-session environment.
- Backup/new binary and restricted logs are under
  `/home/indo/projects/erd-lan-20260909/deployment`.
- New observed process: PID 2241989.
- Actual `erd-discover --timeout-secs 5` output included:
  `indo._erd._tcp.local.`, IP `1.231.34.236`, OS `linux`,
  TCP 19730, UDP 19731.
- Linux is on a different physical subnet from the phone/Windows LAN. Its
  visibility on its own LAN is not evidence it appears on the phone's LAN.

### iPhone

- Physical device: iPhone 16 Plus, CoreDevice
  `F1C581E0-A54E-5E85-8013-4F02DF80F98B`.
- Package: `clients/rust/ios-shell/gen/apple/build/arm64/EclipticRD.ipa`.
- Installation succeeded, bundle ID `com.eclipticrd.ios`.
- Installed container:
  `/private/var/containers/Bundle/Application/D0ADF61A-BB1C-4E8D-902E-D6644B959A33/EclipticRD.app/`.
- Launch with `--terminate-existing` succeeded.
- Packaged Info.plist contains `NSLocalNetworkUsageDescription` and
  `NSBonjourServices` entries `_erd._tcp`, `_erd._udp`.
- Actual screenshot captured:
  `.omo/lan-discovery-20260909/iphone-lan.png`.
- WebInspector returned `Web inspector is not enabled` after deployment.

## UI surface checks and remaining gate

Existing static UI was exercised in a WebKit WebView at 390x844 and 844x390.
The Tauri discovery boundary was explicitly mocked for this UI-only check.
Observed: host card selection focuses PIN, a same-ID address update selects the
new address, removed hosts disappear, empty/permission-error states preserve the
direct form, no horizontal overflow, visible buttons at least 44px high, and the
landscape form is vertically scrollable. Screenshots `ui-portrait.png`,
`ui-empty.png`, `ui-denied.png`, `ui-landscape.png` are in the evidence directory.
Screenshot aesthetic review is unverified: the model could not receive images
and the separate vision tool failed. Browser DOM checks are not native LAN QA.

Remaining physical-device gate requires local-network permission and enabling
Settings > Apps > Safari > Advanced > Web Inspector on the iPhone. Then verify
the real Windows card, authenticate through it, and observe video/audio/input.
Do not mark this gate complete based on the build, test or desktop LAN evidence.
