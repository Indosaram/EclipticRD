# MahoRD iOS Client Design Specification

Status: implementation contract for the physical iPhone client.
Authority: adaptation of `clients/rust/tauri-shell/DESIGN.md` to mobile handheld touch ergonomics and iOS safe-area layout.
Platform: iOS 16+ via Tauri v2 (`clients/rust/ios-shell`).
Mode: Operate.

---

## 1. Product Truth & Mobile Ergonomics

MahoRD on iOS is an ultra-low latency remote desktop operator's tool for iPhone. The interface carries the established desktop charcoal and coral visual identity while respecting physical touchscreen constraints:

- **Thumb-Zone Architecture**: Primary connection actions and floating in-session controls sit within the lower half and accessible margins of the screen, avoiding reach fatigue.
- **Safe-Area Inset Enforcement**: The UI strictly honors `env(safe-area-inset-*)` across Dynamic Island, camera notches, rounded display corners, and the bottom home indicator.
- **Strict Input Scope Isolation**: Touches landing on the floating overlay, accessory keyboard bar, status sheets, or dialogs belong exclusively to local mobile UI and are never forwarded to the remote host. Touching local UI or losing app focus flushes pending motion and releases held remote touches.
- **No Fictional State**: A successful network handshake is separate from first decoded/presented video. The UI explicitly distinguishes `idle`, `connecting`, `waiting-video`, `streaming`, and `error`. Rendered FPS measures actual client WebGL presentations, never captured or host-reported metrics.

---

## 2. Design Tokens

Tokens match the desktop system defined in `clients/rust/tauri-shell/DESIGN.md`, augmented for iOS safe areas and touch sizing:

| Token | Value | Purpose |
| --- | --- | --- |
| `--bg` | `#141517` | Main workspace background / letterboxing |
| `--sidebar-bg` | `#1b1d20` | Navigation / secondary background |
| `--card-bg` | `#222529` | Forms, modals, floating overlays |
| `--card-hover` | `#2a2e33` | Pressed neutral state |
| `--text` | `#f4f5f6` | Primary high-contrast text |
| `--text-muted` | `#b2b7bf` | Secondary metadata and labels |
| `--border` | `#3c4148` | Card and panel borders |
| `--border-control`| `#747c88` | Active input and button outlines |
| `--accent` | `#f47660` | Primary coral action fill |
| `--accent-hover` | `#ff8a75` | Primary action highlight |
| `--accent-active`| `#dc6652` | Primary action pressed |
| `--on-accent` | `#17191c` | Dark contrast text on coral |
| `--focus` | `#ffad9c` | Accessibility focus ring |
| `--success` | `#83d4ab` | Confirmed active stream indicator |
| `--warning` | `#e9bf72` | Pending / waiting explanation |
| `--danger` | `#ffaaa2` | Errors and disconnect actions |
| `--disabled-bg` | `#30343a` | Disabled surface |
| `--disabled-text`| `#939ba6`| Disabled label |
| `--touch-min` | `44px` | Apple HIG minimum touch target size |
| `--radius-control`| `8px` | Buttons, fields |
| `--radius-panel` | `14px` | Cards, modals, floating pills |
| `--radius-pill` | `9999px`| Floating control badges |
| `--shadow-overlay`| `0 8px 24px rgba(0, 0, 0, 0.55)` | Floating bars and sheets |

### Typography
- UI font: `-apple-system, BlinkMacSystemFont, "SF Pro Text", "SF Pro Display", sans-serif`.
- Numeric / data font: `ui-monospace, SFMono-Regular, Menlo, monospace` for sequence, resolution, addresses, and FPS.
- Sizes: Title 22/28px 700; Section 16/22px 600; Body 14/20px 400; Metadata 12/16px 500.

---

## 3. Screen Layout & Orientation Contracts

### Portrait (e.g. 390x844 pt)
- **Connect Surface**: Centered card with 16px horizontal margins. Title and brand icon at top. Host input, optional 8-digit numeric PIN input, Connect button, and informative error banners. Padded for bottom safe-area insets (`env(safe-area-inset-bottom)`).
- **In-Session Viewport**: The remote desktop canvas fills available screen bounds while strictly preserving the remote aspect ratio (letterboxed or pillarboxed).
- **Floating Overlay Pill**: Collapsed by default into a translucent 44px pill placed at the top-right safe margin (`env(safe-area-inset-top) + 8px`, `env(safe-area-inset-right) + 12px`). Tapping expands the floating action bar.
- **Accessory Keyboard Bar**: Docked immediately above the bottom home indicator (`calc(env(safe-area-inset-bottom) + 6px)`). Contains Esc, Tab, arrow keys, sticky modifiers (Shift, Ctrl, Opt, Cmd), and soft keyboard toggle. Can be tucked away with a single swipe or toggle button.

### Landscape (e.g. 844x390 pt)
- Maximizes video presentation area. Letterboxing shifts horizontally.
- Safe-area insets adjust: Dynamic Island / notch clearance on left or right (`env(safe-area-inset-left)` / `env(safe-area-inset-right)`).
- Floating controls remain pinned to safe corners without obscuring the active display center.

---

## 4. Interaction & Touch Architecture

### Input Scope Segregation
```
┌────────────────────────────────────────────────────────┐
│ Window / Document Root                                 │
│  ┌──────────────────────────────────────────────────┐  │
│  │ Local UI Scope (Touch ignored by remote host)    │  │
│  │  - Floating Overlay Bar                          │  │
│  │  - Accessory Key Bar                             │  │
│  │  - Connection Modal / Details Sheet              │  │
│  └──────────────────────────────────────────────────┘  │
│  ┌──────────────────────────────────────────────────┐  │
│  │ Remote Desktop Surface (Canvas)                  │  │
│  │  - Top-left normalized aspect-fit coords (0..1)  │  │
│  │  - Serialized touch events: began, moved, ended  │  │
│  │  - Cancellation on blur/hidden/pointercancel     │  │
│  └──────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────┘
```

### Touch Normalization Contract
Touches landing on the canvas are mapped to the aspect-fit video rectangle:
1. Determine canvas bounding box $W_{rect}, H_{rect}$.
2. Compute aspect ratio $A_v = W_{video} / H_{video}$ and $A_c = W_{rect} / H_{rect}$.
3. Calculate fit geometry:
   - If $A_c > A_v$: $H_{fit} = H_{rect}$, $W_{fit} = H_{rect} \times A_v$, $X_{off} = (W_{rect} - W_{fit}) / 2$, $Y_{off} = 0$.
   - Else: $W_{fit} = W_{rect}$, $H_{fit} = W_{rect} / A_v$, $X_{off} = 0$, $Y_{off} = (H_{rect} - H_{fit}) / 2$.
4. Calculate normalized top-left coordinates:
   $$x = \text{clamp}\left(\frac{clientX - X_{off}}{W_{fit}}, 0.0, 1.0\right)$$
   $$y = \text{clamp}\left(\frac{clientY - Y_{off}}{H_{fit}}, 0.0, 1.0\right)$$
5. Send via `touch({ event: { id, x, y, phase } })` where phase is `"began" | "moved" | "ended" | "cancelled"`.
6. Note: Rust touch handler owns inverted-Y wire normalization; UI emits normalized top-left coordinates.

### Touch Modes
- **Direct Touch**: Touches correspond directly to absolute pointer positions and primary clicks.
- **Trackpad Mode**: Relative motion with simulated mouse buttons. Switched via `set_touch_mode({ mode: "direct" | "trackpad" })`.

### Interruption & Teardown Safety
- `pointercancel` and local UI context switches automatically dispatch `"cancelled"` for all active touches.
- `window.blur` while the document remains visible releases held touch inputs, but explicitly preserves frame polling and the active stream so system permission alerts do not freeze video.
- Background transitions (`document.visibilitychange (hidden)` or `window.pagehide`) during connecting or streaming release all inputs, halt frame polling, and invoke native `disconnect()`, invalidating the session generation so no resources are consumed in the background.
- Returning to the foreground does not resurrect the disconnected session or accept stale frame deliveries from prior generations.

---

## 5. Video Pipeline & NV12 Shaders

### Frame Reception & Parsing
1. Binary buffer received from `poll_frame()`.
2. Header validation: 16 bytes:
   - `width`: u32 LE (bytes 0..3)
   - `height`: u32 LE (bytes 4..7)
   - `sequence`: u64 LE (bytes 8..15)
3. Plane offsets:
   - Y plane: offset 16, length $W \times H$.
   - UV plane: offset $16 + W \times H$, length $W \times \lceil H / 2 \rceil$.
4. Expected length: $16 + W \times H + W \times \lceil H / 2 \rceil$.

### WebGL2 NV12 Shader (with WebGL1 fallback)
- Textures:
  - Y Plane: Texture unit 0, `gl.R8` / `gl.RED` (`gl.LUMINANCE` in WebGL1).
  - UV Plane: Texture unit 1, `gl.RG8` / `gl.RG` (`gl.LUMINANCE_ALPHA` in WebGL1).
- Fragment Shader: Converts NV12 BT.601 limited-range YUV to sRGB clamped to $[0, 1]$.
- Periodic presentation report: `presented({ sequence })` called after the first real GL draw and periodically every 60 frames to record verifiable client presentation progress.

---

## 6. Pairing & Credential Security

- **PIN Security**: Pairing PIN is strictly ephemeral in memory and never stored in `localStorage` or persisted to unencrypted files.
- **Host Persistence**: The last connected host address may be stored in `localStorage` (`mahord.ios.last_host`) for user convenience.
- **Authoritative Validation**: Host must be non-empty; PIN if provided must be exactly 8 ASCII digits (`^[0-9]{8}$`).
- **QA Provisioning**: `startup()` returns `{ host, auto_connect }`. If `auto_connect` is true and a host is present, the app automatically calls `connect({ host, pin: null })`, exercising identical paths without leaking keys.

---

## 7. Accessibility & Target Sizing

- Every interactive button and field satisfies the 44px minimum touch target size.
- Visual focus and active feedback provided via `--focus` and pressed color shifts.
- Accessible ARIA roles: `role="dialog"`, `aria-label`, `aria-pressed` for toggles, `role="status"` for counters and live regions.
- Text contrast: All labels maintain at least 4.5:1 contrast against dark background.

---

## 8. LAN Discovery & Local Network Ergonomics

### Bonjour Service Contract
- Discovers hosts advertising `_maho-rd._tcp.local.` via system DNS-SD (`DNSServiceBrowse` / native Rust backend).
- Discovered attributes: `id` (service fullname), `name` (host display name), `ip` (resolved IPv4 or scoped IPv6), `os` (`macos`, `windows`, `linux`), `tcp_port` (signaling/control port), `udp_port` (media datagram port).

### Visual Hierarchy & Mobile Card Architecture
- **Discovered Hosts Section**: Rendered above the manual address form on the Connect surface (`#view-connect`). If hosts are present, cards are displayed in a touch-friendly vertical stack with 8px gaps and 44px minimum target heights.
- **Host Card Anatomy**:
  - Primary row: Host display name in 16px 600 weight (`--text`), with OS badge (`--text-muted`, `--card-bg`, 4px padding, 6px radius).
  - Secondary row: Resolved IP address and ports (`ui-monospace`, 12px, `--text-muted`).
  - Tapping a card populates the host field, records discovered TCP/UDP ports, and focuses the PIN field if unpaired.
- **Distinct Discovery States**:
  - `loading`: Subtle scanning indicator with "Scanning local network...". Shown during initial DNS-SD resolution or when iOS local network permission alert is presented.
  - `idle` (with hosts): List of discovered cards with refresh trigger button.
  - `idle` (empty): "No nearby MahoRD hosts found on this network. Connect by IP address below."
  - `error`: Distinct warning banner (`--warning` text, `--card-bg`) indicating permission denied or mDNS failure. Manual IP connection is never blocked.

### Security Invariant: Unauthenticated Discovery Hints
- Service advertisement names are unverified broadcast metadata.
- Selecting a host card NEVER claims `paired: true` based solely on advertised hostname matching a previous pairing name.
- Pairing keys and session authentication remain strictly authoritative via Keychain lookup by resolved address and explicit PIN entry for unpaired hosts.
