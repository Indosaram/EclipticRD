# EclipticRD desktop design contract

Status: implementation contract, not a claim that the redesign has shipped.
Scope: the Tauri shell served from `ui/`; preserve Rust pairing, streaming,
input payloads, fullscreen fallback, and existing user work. Mode: Operate.

## 1. Direction and reference authority

A quiet computer library leads directly to a remote desktop, with a persistent
local session launcher that never makes the user guess which machine receives
input. This is a desktop operator's tool, often used beside a bright remote
screen; charcoal chrome reduces competition with that screen. The signature is
the continuity of computer identity from library card to compact session bar.
No marketing hero, decorative performance dashboard, glowing gradients, or
decorative card motion. EclipticRD retains its own name and simple E mark.

Real references reviewed on 2026-09-05, via text rendering at `r.jina.ai` because
direct requests to Parsec were challenged/403:

- [Parsec: Use the Web App (browser)](https://support.parsec.app/hc/en-us/articles/32381650129300-Use-the-Web-App-browser).
  The article embeds its actual `web_computers_page.png` app reference at
  <https://support.parsec.app/hc/article_attachments/32381655249812> and describes
  accessing a computer from the client. Benchmark: computer-first navigation
  and obvious connection action. The article also distinguishes web and native
  performance; do not transfer Parsec performance claims to EclipticRD.
- [Parsec: Immersive Mode Setting](https://support.parsec.app/hc/en-us/articles/32361385571860-Immersive-Mode-Setting).
  Benchmark: explicit local versus remote input ownership and a way back to
  local control. EclipticRD must not claim Parsec's immersive mode or hotkeys.

The reference documents were read; their linked screenshot pixels were not
inspected in this foundation pass. Exact geometry/colors below are authored
EclipticRD decisions, not sampled Parsec values or a literal clone. Search,
favorites, restrained red-orange, and session-overlay coherence are explicit
project requirements, not claims established by those reference texts.

## 2. Product truth and information hierarchy

Primary task: find a known computer and connect. Alternate entry: direct host
address plus optional pairing PIN. One active session only. Home ends the
session; it is not background streaming or multi-session navigation.

Reading order: app identity -> Computers title -> search/filter -> computer
identity/status -> Connect. Direct connection is a clearly labeled, always
discoverable secondary entry, not another destination. Session order: host ->
actual session state -> rendered FPS / Decode -> local actions -> expanded
details. No accounts, friends, hosting switches, cloud catalog, settings,
network ping, bitrate, resolution selector, quality preset, or reconnect toggle
without an existing executable data/action source.

| Surface/data | Actual source and honest presentation |
| --- | --- |
| Computer library | `list_hosts`: `id`, `name`, `ip`, `os`, `online`, `paired`, nullable `last_seen`. Show returned records only. |
| Availability | `online` includes Tailscale presence and always-true fallback testbeds. Use “Available” / “Offline” as discovery hints, with nearby library helper “Availability is a discovery hint, not an ERD readiness check. Default testbeds may appear available without a probe.” Never “Ready”, “Reachable”, or a service-health guarantee. |
| Pairing | `paired` is store matching, not active authentication. Use “Saved pairing” or “PIN may be needed”. Do not expose keys or pairing IDs. |
| Search | Local case-insensitive substring match on name, IP, OS. No network request and no search of secret PINs. |
| Available filter (C1) | Required alongside Favorites. Locally include only records with `online === true`; compose with search and Favorites. Preserve the nearby discovery-only helper: this is not an ERD readiness filter. All clears both filters. |
| Favorites | Local preference, not cloud state. Store a versioned list of normalized IP keys in localStorage (`eclipticrd.favorites.v1`); IP is used because current `id` can change when paired. Never store PINs. Explain persistence errors; keep session-only choice if storage is unavailable. |
| Direct connection | Existing `connect` with host, fixed TCP 19730 / UDP 19731, optional PIN. No unsupported port editing or URL-format promise. |
| Session state | `connect` result + `stats.connected` / `stats.state`; first valid rendered frame is separate from successful connection. |
| Video | Preserve the working shader, raw NV12 `poll_frame_raw` pipeline, texture upload and aspect-fit canvas unchanged during this redesign. No fake preview thumbnails. |
| Rendered FPS | Label “Rendered FPS”: existing browser rendered-frame count over elapsed time, not host capture FPS. Details/help says “Frames rendered by this client per second”. Static/background windows can report 0 while decoded counters advance; zero alone must not imply disconnect or failed decoding. Do not substitute a synthetic positive number. Unavailable samples show an unavailable marker, not invented zero. |
| Decode | `stats.latency_p50_ms` / `latency_p99_ms`, currently local receive-to-decode elapsed samples. Labels MUST say “Decode p50” / “Decode p99”, with ms. Not network, round-trip, or end-to-end latency. |
| Counters | `frames_received`, `frames_decoded`, `audio_packets_received`; packets are not an audio-enabled indicator. |
| Footer | Quiet muted product name only. Remove “Local: probing...” and the Agent API badge/address; neither establishes live status. Do not replace them with another probe, health dot, or unsupported version claim. |

## 3. Tokens

Use CSS custom properties, not scattered replacements or a new framework.

| Token | Value | Purpose |
| --- | --- | --- |
| `--bg` | `#141517` | Main workspace |
| `--sidebar-bg` | `#1b1d20` | Navigation |
| `--card-bg` | `#222529` | Cards, forms, opaque overlay |
| `--card-hover` | `#2a2e33` | Hovered neutral controls |
| `--text` | `#f4f5f6` | Primary text |
| `--text-muted` | `#b2b7bf` | Metadata and help, not disabled opacity |
| `--border` | `#3c4148` | Decorative separation |
| `--border-control` | `#747c88` | Input boundary / essential affordance |
| `--accent` | `#f47660` | Primary action fill, selected mark |
| `--accent-hover` | `#ff8a75` | Primary hover |
| `--accent-active` | `#dc6652` | Pressed primary |
| `--on-accent` | `#17191c` | Dark text on coral, not white |
| `--focus` | `#ffad9c` | 2px focus ring, 3px offset |
| `--success` | `#83d4ab` | Confirmed active session only |
| `--warning` | `#e9bf72` | Unavailable/pending explanations |
| `--danger` | `#ffaaa2` | Error and disconnect text |
| `--disabled-bg` / `--disabled-text` | `#30343a` / `#939ba6` | Disabled, still legible |
| `--space-*` | `4, 8, 12, 16, 20, 24, 32, 40px` | Shared spacing scale |
| `--radius-control` / `--radius-panel` | `6px` / `10px` | Inputs/buttons; cards/dialogs |
| `--shadow-overlay` | `0 12px 32px #00000059` | Overlays only; cards flat |
| `--motion-fast` / `--motion-panel` | `120ms` / `180ms` | Color/focus; disclosure |

Type: native `-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif` for
headings and interface; `ui-monospace, SFMono-Regular, Consolas, monospace` for
addresses/numeric values only. No network font dependency. Title 28/34px 650;
section 16/24px 600; computer name 17/24px 600; body/control 14/20px 400/600;
metadata 12/18px 400. Use tabular numerals for counters. Sentence case, no
all-caps micro-label hierarchy. Color carries state only with text/icon support.

## 4. Layout contracts

All dimensions are CSS client-area pixels, excluding OS window decoration.

### Desktop: 1280x800

- 200px navigation rail, remaining 1080px main area. Main padding 32px gives
  1016px usable width. Rail brand 64px high; Computers and Favorites 40px rows.
- Header: title, one-line helper, Refresh aligned right; 80px region. Below:
  40px search row and All/Available/Favorites filter state, then 24px separation.
- Computer grid: three equal columns, 16px gaps, 328px cards, minimum 172px
  card height. The supplied native baseline has five records, so this gives
  two rows with the final position empty. No fake records or stretched
  promotional filler. Rows grow for wrapped metadata.
- Direct connection: below the library section, full-width distinct neutral
  panel with heading/helper, labeled host flex field, 144px PIN, 112px Connect.
  For the baseline five-record inventory, the panel's connection controls
  should remain above the desktop fold; additional records scroll naturally.
- Main scrolls vertically as records increase; rail stays fixed. Do not fix
  the direct panel over cards or hide overflow to mask sizing defects.

```text
| EclipticRD     | Computers                           Refresh |
| Computers     | Find a computer or connect by address.        |
| Favorites     | [ Search computers            ] All/Avail/Fav |
|               | Listed computers (actual result count)       |
|               | [ computer / status / star / Connect ] x 3   |
|               |                                              |
|               | Direct connection                            |
|               | Host address       Pairing PIN (optional)    |
|               | [ host           ] [ PIN       ] [ Connect ] |
| EclipticRD    | inline notice/error region                    |
```

### Compact: 900x650

- At widths below 1040px use a 72px icon rail with accessible tooltips and
  names; 24px main padding yields 780px content. Two 382px cards, 16px gap.
- Header 64px; search stays 40px and full-width. Filter controls may occupy a
  second row without hiding search or Refresh. Preserve the same reading order.
- Direct panel host occupies first full row; PIN + Connect share next row.
  Labels remain above inputs. Main scrolls naturally if notices expand.
- Below 720px effective width (including zoom), use a single card column,
  wrap toolbar/form rows, reduce main padding to 16px; no horizontal scrolling.
  This is overflow/accessibility resilience, not a new mobile product goal.

### Session at both sizes

Video owns the entire client area and retains aspect ratio with black
letterboxing. No layout resizing when the launcher opens. Persistent centered
bar: top 16px, height at least 48px, maximum width `calc(100% - 32px)`;
host name can truncate, actions cannot. Target width 560-640px desktop, at
most 600px compact. Identity/FPS/Decode are quiet; Disconnect is explicit.
Expandable details: 320px wide, 8px below bar, viewport-bounded and vertically
scrollable when needed. Keep Home, disclosure, and Disconnect in the bar;
Fullscreen and metrics in the details. Do not auto-hide the only exit control.

Connecting state uses the same viewport plus a centered opaque 400px-max
status panel with host, state, and Cancel; details launcher is not actionable
until connected. A successful connect with no frame says “Waiting for video”,
not “Streaming”. Errors return to an actionable local surface, not a dead canvas.

## 5. Reusable component anatomy and states

Reuse lightweight HTML/CSS primitives; no dependency or component framework
is implied. Each actual action has default, hover, focus-visible, pressed,
disabled, pending, and failure feedback where asynchronous work applies.

| Primitive | Anatomy and contract |
| --- | --- |
| Button | Native button, optional 16px stroke icon + label; minimum 40px height, 12px horizontal padding. Primary coral for Connect, neutral Refresh/Cancel/Fullscreen, danger text for Disconnect. Pending keeps width and changes label; never silently repeats requests. |
| Icon button | 40x40px target, 18px consistent 1.75px stroke icon, accessible name and hover/focus tooltip. Favorite uses `aria-pressed`; no nested buttons. |
| Navigation/filter | Real button/link, icon + label + optional actual count; `aria-current` for destination or `aria-pressed` for filter, not mixed semantics. Favorites is a filtered Computers view, not a cloud page. |
| Field | Visible label, input, help/error associated with `aria-describedby`; 40px minimum height. Default/hover/focus/invalid/disabled. Host is text with spellcheck off; PIN is password, optional, paste allowed. No permanent sample IP value. |
| Computer card | Article/list item: OS/neutral computer icon, name, separate favorite button, address, availability and pairing text, Connect footer. Unknown OS gets generic computer icon, never assumed Apple. Whole card is not clickable. Long names wrap to two lines; address remains selectable. |
| Status/notice | Icon + meaningful short text + optional recovery button. Loading/notice uses polite live region; action failure uses alert once. Errors stay visible until dismissed or resolved, never disappear on a timer. |
| Session launcher | Local input scope, compact bar, disclosure button controlling a hidden region, `dl` metrics and fullscreen toggle. Expanded/collapsed are state, not separate styles. |
| Connection panel | Labelled status/dialog region, host, state, Cancel. Local focus contained while modal; restore triggering control on completion/cancel/error. No fabricated progress percentage. |

## 6. Complete screen/state inventory

| Surface | Required states and transitions |
| --- | --- |
| Library | Initial loading: static card-shaped placeholders, `aria-busy`, one “Loading computers” announcement. Loaded: actual count/cards. Refreshing: keep prior cards and mark busy; one in-flight refresh. Failure: keep prior results labeled not refreshed, show Retry. Empty: “No computers listed” + direct address action. |
| Search | Empty query, matching query, no results, clear query. No results offers Clear search, not a loading spinner. Search, Available and Favorites filters compose; visible count reflects all active predicates. Preserve query across refresh/session return. |
| Available filter | Off/on, matching records, no available matches, refresh changing availability. Empty results offer Clear filters; selection uses `aria-pressed`. No network request or service probe is implied. |
| Favorites | Unselected/selected star, all/favorites filter, empty favorites with explanation, missing previously favorited host not invented as a live record, persistence unavailable notice. Toggle without initiating connection. |
| Computer | Available, offline, saved pairing, pairing unknown/new, unknown OS, long/malformed display text. Availability is only a discovery hint, explained beside the library. Offline connect is disabled with reason; direct form remains available for explicit address attempts. Busy disables competing Connect actions. |
| Direct/PIN | Pristine, editing, blank address validation, invalid PIN, submitting, pairing rejection, other command rejection, cancelled, connected. Trim address; do not impose unsupported DNS/IP restrictions. Nonblank PIN must be exactly 8 ASCII digits (`^[0-9]{8}$`), proven by `erd-net/src/tls_psk.rs:323-325`; reject other lengths and non-ASCII digits inline. Blank is allowed for saved reconnect, not new pairing. For unpaired card, populate direct form and focus PIN before intentional submit. Keep PIN as a string to preserve leading zeros; backend remains authoritative. |
| Connection | Connecting -> command success/waiting for video -> first valid rendered frame/streaming. Cancel during pending must invalidate late completion and use existing disconnect path. Rejected connect shows exact safe error detail plus Edit address/Retry; no blocking browser alert. No automatic reconnect or blind timed retry. |
| Session | Collapsed/expanded launcher; windowed/fullscreen; unavailable fullscreen error; video unavailable/renderer failure; stats pending/available/unavailable; remote session ended; explicit disconnect pending/success/failure. Never retain old FPS or Decode as current data after failure or new connection. |
| Session return | Home and Disconnect both release held inputs and end the session, clear viewport, exit fullscreen, restore library focus and query/favorites. If teardown fails, disclose it and offer retry rather than claiming success. |
| No Tauri bridge | Clearly state “Desktop connection controls are unavailable in this browser.” Disable native actions. Do not show fake successful commands, hosts, session health, or stats; fixtures belong only to explicitly identified test surfaces. |

## 7. DOM hooks and API boundary

Preserve existing IDs used by rendering, automation, and input routing:

- Shell: `sidebar`, `main-view`, `host-grid`, `direct-ip`, `direct-pin`.
- Video: `viewport-container`, `viewport`, `screen-canvas` (never rename the
  latter two: `SessionOverlay` uses them to decide remote input ownership).
- Session: `session-overlay` with `data-ui-scope`, `launcher-panel`,
  `session-connecting-modal`, `modal-connecting-text`, `session-host-name`.
- Actions: `btn-home`, `btn-expand`, `btn-disconnect`, `btn-fullscreen`,
  `btn-fullscreen-label`, `btn-pointer-lock`, `btn-pointer-lock-label`;
  preserve disclosure `aria-controls`, fullscreen `aria-pressed`, and
  pointer lock `aria-pressed`.
- Metrics: `session-stat-fps`, `session-stat-lat`, `session-stat-p50`,
  `session-stat-p99`, `session-stat-state`, `session-stat-frames`,
  `session-stat-decoded`, `session-stat-audio`. Legacy `lat` ID stays while
  visible label changes to Decode; IDs are not permission to mislabel data.

New stable hooks: `host-search`, `btn-refresh`, `filter-all`, `filter-available`,
`filter-favorites`, `library-status`, `direct-form`, `btn-direct-connect`,
`direct-error`, `session-error`. Cards use `data-host-id` and buttons
`data-action="connect"` / `data-action="favorite"`. Bind events without
interpolating remote names/addresses into inline JavaScript or HTML. Render
untrusted strings as text. Display values and DOM hook values are separate.

Keep the callable behavior of `refreshHosts`, `connectDirect`,
`connectToHost(ip, name, pin)`, `cancelConnecting`, `doDisconnect`,
`endSessionToHome`, `toggleLauncherPanel`, `toggleSessionFullscreen`.
Calls remain `list_hosts`, `connect({host,tcpPort:19730,udpPort:19731,pin})`,
`disconnect`, `stats`, `poll_frame_raw`, and `send_input({event})`.
Pointer input contract:
- Buttons: `button 0` -> `LeftMouseDown`/`LeftMouseUp`; `button 1` ->
  `MiddleMouseDown`/`MiddleMouseUp`; `button 2` -> `RightMouseDown`/`RightMouseUp`.
  Unsupported extra buttons (`button >= 3`) are rejected and never map to left click.
- Relative pointer motion: pointer lock utilizes `RelativeMove` carrying
  accumulated motion deltas in `scroll_dx` (horizontal) and `scroll_dy` (vertical).
- Pointer lock availability: `btn-pointer-lock` is visible only when real webview
  `Element.prototype.requestPointerLock` and `document.exitPointerLock` APIs exist;
  unsupported environments must never simulate a locked state.
- Teardown & release: window blur, tab hidden state, and disconnect exit pointer
  lock and emit release events for all currently held mouse buttons and keys.
No new Rust API is required by this contract. Preserve raw buffer framing,
render scheduling, coordinate mapping, held-key releases, and native
fullscreen fallback; presentation work must not rewrite the transport.

## 8. Accessibility and input safety

- Landmarks: navigation, main, one h1, h2 for library/direct connection.
  Real labels and native controls; list semantics for library. No global
  `user-select:none` on addresses, errors, or input values.
- Text contrast >=4.5:1, large text >=3:1; essential boundaries/focus >=3:1.
  Verify actual computed pairs, especially disabled and coral states. Decorative
  borders need not meet essential-control contrast. No color-only status.
- Focus ring visible throughout; tab order follows visual order. Hover is
  supplemental, not the only access to names, explanations, or actions.
- Modal makes background inert and keeps Cancel reachable. On Escape, collapse
  local details and focus disclosure; do not steal Escape from remote video.
- Overlay pointer activation/focus entry, window blur, hidden document, and disconnect
  release held remote inputs. Form, dialog, nav, search, and launcher keys
  must never be sent to the remote. Maintain an accessible local focus path;
  do not advertise an unimplemented capture-release hotkey.
- Reduced motion removes spinner rotation/disclosure animation; static text
  still communicates pending work. No card translation, scaling, shimmer,
  entrance choreography, or blur needed to read the interface.
- Metrics are not live regions at every poll. Announce only meaningful state
  transitions. Remote canvas gets a useful label, but do not claim its pixel
  contents are screen-reader accessible without a remote accessibility bridge.

## 9. Objective acceptance criteria

1. At 1280x800 and 900x650, capture library (actual records), search match/no
   results, Available/empty Available, favorites/empty favorites, direct validation/PIN/error, loading and
   failed refresh. No clipped labels, overlapping controls, horizontal scroll,
   duplicate refresh destinations, or fake records. Baseline five records plus
   direct entry fit the desktop first viewport.
2. All controls follow 40px target, 6px control corner, shared type and spacing
   rules; all coral primary labels use dark on-accent text. Card hover changes
   border/background only. No decorative glow/gradient or unsupported panels.
3. Native lead captures connecting, waiting/streaming, launcher collapsed and
   expanded, fullscreen, stats unavailable, disconnect/Home return and failure
   recovery. Video aspect fit and pointer alignment remain correct at both sizes.
4. Test search/filter composition and persistence with deterministic fixtures;
   mark fixtures explicitly. Async tests subscribe before action and await exact
   state/event with bounded timeout, never fixed sleeps or timing luck.
5. Test keyboard-only local controls, input isolation/held-input release, focus
   restoration, 200% text zoom/reflow, reduced motion and measured contrast.
6. A final PASS must name every exercised surface and identify live backend,
   deterministic fixture, or unverified evidence for each. Browser preview is
   not proof of native connect, rendering, fullscreen, audio or remote input.
