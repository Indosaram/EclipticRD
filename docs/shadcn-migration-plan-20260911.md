# tauri-shell UI: shadcn/ui migration plan

Status: proposal / implementation contract. Nothing in this plan has been implemented.
Date: 2026-09-11
Scope: `clients/rust/tauri-shell/ui` (desktop launcher + session surface) only.

---

## 1. Where we actually are today (measured, not assumed)

The desktop UI is **not** shadcn/ui, and it is not React, Tailwind, or Radix. It is a
hand-written static web app served directly by Tauri.

| Fact | Evidence |
|------|----------|
| No `package.json` anywhere in the repo | `clients/rust/tauri-shell/package.json` and repo-root `package.json` both absent |
| No build step | `tauri.conf.json` -> `"frontendDist": "../ui"` points at raw source files; no `beforeDevCommand` / `beforeBuildCommand` / `devUrl` |
| No React / Tailwind / Radix / cva | grep for `tailwind`, `radix`, `shadcn`, `cva(`, `react` over `ui/` returns 0 matches |
| Markup is semantic BEM-ish CSS classes | `class="this-computer-card"`, `class="status-badge badge-online"` — not utility classes |
| Hand-rolled token system | `ui/styles.css:4` `:root` defines `--bg`, `--card-bg`, `--border`, `--accent: #f47660`, `--radius-control: 6px`, `--space-1..10` |

Size and shape of what would have to move:

| File | Lines | Nature |
|------|-------|--------|
| `ui/index.html` | 1,524 | Markup (lines 1–170) + **one inline `<script>` of ~1,350 lines** starting at line 171 |
| `ui/styles.css` | 643 | Tokens, component CSS, 1 container query, 3 media queries, `prefers-reduced-motion`, `forced-colors` |
| `ui/library.js` | 124 | UMD module (`module.exports` + `window.LibraryModel`), DOM-free state |
| `ui/connection-state.js` | 206 | UMD module (`window.ConnectionLifecycle`), connection state machine |
| `ui/session-overlay.js` | 209 | UMD module (`window.SessionOverlay`), overlay/held-input state |
| `ui/*.test.mjs` (4 files) | 1,083 | 2 run under `bun:test`, 2 under `node:test` |

DOM coupling inside the inline script: **80 unique `id` attributes**, 16 `getElementById`
lookups, 41 `addEventListener` registrations, 3 `setInterval` loops (stats 500 ms, audio
status 500 ms, library refresh 3000 ms) and 2 `requestAnimationFrame` loops (video poll,
pointer-motion flush).

### 1.1 The IPC surface that must survive byte-for-byte

Literal `invoke(...)` call sites in `ui/`:

`connect`, `disconnect`, `stats`, `poll_frame_raw`, `send_input` (7 sites), `set_bitrate`
(2 sites), `list_pairings`, `forget_pairing`, `list_hosts`, `get_host_status`.

**Dynamically dispatched commands — a grep-driven port will silently drop these:**

- `index.html:860` — `const action = isRunning ? 'stop_host' : 'start_host';` then `invoke(action)`.
- `index.html:633–640` — `audioCommand(command, args)` wraps `invoke(command, args)`; callers pass
  `audio_status`, `list_audio_devices`, `set_audio_volume`, `set_audio_muted`, `set_audio_device`.

Any port must enumerate commands from `src-tauri/src/lib.rs` (22 `#[tauri::command]` functions),
not from UI call sites.

### 1.2 The hot path that must not regress

`pollVideoFrame()` runs on `requestAnimationFrame`, calls `invoke('poll_frame_raw')`, reads the
returned `ArrayBuffer` with a `DataView` (NV12 Y/UV planes plus a 9-byte cursor tail), uploads
via `texImage2D` into `R8`/`RG8` textures (with `LUMINANCE`/`LUMINANCE_ALPHA` fallback for WebGL1),
and updates `#remote-cursor` by direct transform write. Context acquisition is
`getContext('webgl2')` with a `webgl` fallback.

This loop touches the DOM up to 60 times per second with a multi-megabyte buffer. It is the
single hardest constraint on this migration.

---

## 2. Goal, and the honest question of whether to do it

**Goal.** Replace the bespoke launcher UI with shadcn/ui components on React + Vite + Tailwind v4,
so that future UI work is composition of reviewed components rather than edits to a 1,350-line
inline script.

**Non-goals.** No protocol, pairing, capture, encode, decode, or input-payload change. No visual
redesign: the migration is token-preserving (see §5). No change to `ios-shell`.

**Be clear about the motivation.** The current UI already looks like shadcn because its token
system converged on the same conventions (charcoal surfaces, 6 px radius, muted borders, badge +
card patterns). If the objective is purely *visual*, this migration buys almost nothing — Option A
below gets there in a day at zero runtime risk. The defensible reason to migrate is
**maintainability**: 1,350 lines of application logic living inside an HTML file, with 80 id-based
DOM bindings and tests that regex-extract the script out of the markup, is the actual problem.

### Options

| Option | What it is | Cost | Risk | Verdict |
|--------|-----------|------|------|---------|
| **A. Token parity only** | Keep vanilla; realign `:root` to shadcn's variable names/semantics, adopt shadcn component *recipes* as plain CSS | ~1 day | None — no toolchain, tests untouched | Take this if the goal is "look like shadcn" |
| **B. Hybrid** | React + shadcn for the launcher, vanilla for the session surface | ~4 days | High — two state systems, two event models, duplicated connection state | **Rejected.** Worst of both |
| **C. Full migration, quarantined hot path** | React 19 + Vite + Tailwind v4 + shadcn for all chrome; session canvas stays imperative and uncontrolled | ~8–12 working days | Medium, bounded by §6 rules | **Recommended** if maintainability is the goal |

The rest of this document specifies **Option C**. Option A is a strict subset of Phase P1, so
starting C and stopping after P1 is a legitimate, shippable outcome.

---

## 3. Target architecture

```
clients/rust/tauri-shell/
├── package.json              # new — bun-managed
├── vite.config.ts            # new — react + @tailwindcss/vite, alias @ -> ./src
├── tsconfig.json             # new — baseUrl ".", paths { "@/*": ["./src/*"] }
├── components.json           # new — shadcn CLI config
├── index.html                # new — Vite entry, <div id="root">, no inline logic
├── src/
│   ├── main.tsx
│   ├── styles/globals.css    # @import "tailwindcss"; :root tokens; @theme inline
│   ├── lib/
│   │   ├── ipc.ts            # single typed wrapper over invoke(); the ONLY invoke caller
│   │   ├── library.ts        # port of ui/library.js (DOM-free)
│   │   ├── connection.ts     # port of ui/connection-state.js (DOM-free)
│   │   └── overlay.ts        # port of ui/session-overlay.js (DOM-free)
│   ├── components/ui/        # shadcn-generated, do not hand-edit
│   ├── features/
│   │   ├── library/          # Computers dashboard, host cards, saved credentials, direct connect
│   │   ├── host/             # This Computer card, start/stop sharing, copy info
│   │   └── session/
│   │       ├── SessionCanvas.tsx   # uncontrolled: mounts once, owns the canvas
│   │       ├── renderer.ts         # verbatim WebGL/NV12 code, framework-free
│   │       ├── input.ts            # pointer/keyboard capture -> send_input
│   │       └── SessionOverlay.tsx  # overlay bar; stats via ref writes, not state
│   └── app/App.tsx
├── ui/                       # DELETED at the end of P6, not before
└── src-tauri/                # unchanged
```

Hard rule: **`src/lib/*.ts` stays DOM-free and React-free.** Those three modules are the only
pieces with real test coverage today; keeping them framework-agnostic is what lets their tests
survive the migration (§7).

---

## 4. Phases

Each phase is independently revertible and ends with named evidence. Do not start a phase before
the previous phase's evidence exists.

### P0 — Toolchain, with the old UI still running

1. `bun init` in `clients/rust/tauri-shell`; add `react`, `react-dom`, `@vitejs/plugin-react`,
   `vite`, `typescript`, `tailwindcss`, `@tailwindcss/vite`, `@tauri-apps/api`.
2. `vite.config.ts` per shadcn's Vite template: `plugins: [react(), tailwindcss()]`,
   `resolve.alias { "@": path.resolve(__dirname, "./src") }`, `server.port = 1420`,
   `server.strictPort = true`, `build.outDir = "dist"`.
3. `tsconfig.json`: `compilerOptions.baseUrl = "."`, `paths: { "@/*": ["./src/*"] }`.
4. `npx shadcn@latest init` (answer: Vite, TypeScript, `src/styles/globals.css`, base color slate —
   overwritten in P1).
5. `tauri.conf.json` diff. Note the live config is `clients/rust/tauri-shell/tauri.conf.json`, the sibling of
   `Cargo.toml`, so the path is `./dist` and **not** `../dist`. (`src-tauri/tauri.conf.json` is a stale
   duplicate that no cargo manifest references; `cargo metadata` resolves `tauri-shell` to the outer manifest.)
   ```jsonc
   "build": {
     "frontendDist": "./dist",
     "devUrl": "http://localhost:1420",
     "beforeDevCommand": "bun run dev",
     "beforeBuildCommand": "bun run build"
   }
   ```
6. Copy the current `ui/index.html` to the Vite root unchanged so the app still boots end-to-end
   through the new pipeline before a single component is rewritten.

**Evidence:** `bun run build` exits 0; `cargo run -p tauri-shell` opens a window that still
connects to a host and streams video through the Vite-built bundle.

**Decision point:** `withGlobalTauri: true` currently lets the UI reach `window.__TAURI__`.
Keep it true through P0–P5 so the legacy inline script and new code can coexist; flip to `false`
in P6 once every call goes through `src/lib/ipc.ts` importing `@tauri-apps/api/core`.

### P1 — Token bridge (this is where the design is preserved)

Translate `ui/styles.css:4–44` into shadcn's variable contract in `src/styles/globals.css`, using
`:root` + `@theme inline` (Tailwind v4 form). Full mapping in §5. Keep `color-scheme: dark` and do
not introduce a light theme — the DESIGN.md contract says charcoal chrome exists so it does not
compete with the remote screen.

Carry over verbatim: the `@container (max-width: 41rem)` overlay reflow, `@media (width < 1040px |
720px | 400px)`, `prefers-reduced-motion`, `forced-colors`. Tailwind v4 supports `@container`
natively; the overlay's `container-type: inline-size` becomes `@container/overlay` utilities.

**Evidence:** a scratch page rendering `Button`, `Card`, `Badge`, `Input` that visually matches the
current screenshot palette; side-by-side before/after screenshot in `docs/evidence/`.

### P2 — Port the three state modules to TypeScript

`library.js`, `connection-state.js`, `session-overlay.js` -> `src/lib/*.ts`. These are already
UMD-wrapped and DOM-free; the port is types plus `export`, not a rewrite. Their tests move with
them, converted to `bun:test` imports (§7).

**Evidence:** `bun test src/lib` green, with the same assertion count as the current
`library.test.mjs` (187 lines) + `connection-state.test.mjs` (359 lines) + the DOM-free half of
`session-overlay.test.mjs`.

### P3 — `src/lib/ipc.ts`: one typed IPC boundary

Enumerate all 22 `#[tauri::command]` functions from `src-tauri/src/lib.rs` into a typed map
(name -> args -> return). Include the dynamically dispatched ones from §1.1. Every later phase
imports from here; `invoke` appears nowhere else in `src/`.

**Evidence:** `grep -rn "invoke(" src/ --include=*.tsx` returns 0 hits; a unit test asserts the
command-name union matches a literal list, so adding a Rust command without wiring the UI fails loudly.

### P4 — Launcher surface -> shadcn components

Port markup + handlers per §6 mapping. Order: sidebar -> Computers header -> This Computer card ->
search/filter toolbar -> saved credentials -> host grid -> direct connect -> settings/audio panel.
One surface per commit.

**Evidence:** per surface, a screenshot at 1280×800 and at 200 % text zoom, plus the existing
keyboard/ARIA behaviours (`aria-current` on nav, `aria-pressed` on filters and favorites,
`aria-busy` on refresh) asserted in component tests.

### P5 — Session surface, uncontrolled

`renderer.ts` receives the WebGL/NV12 code **verbatim** — same context acquisition, same fallbacks,
same `DataView` offsets, same cursor tail parsing. `SessionCanvas.tsx` is a `useRef` host that
mounts the renderer once in `useEffect` with `[]` deps and never re-renders while a session is
live. Overlay stats (FPS, decode ms, latency) are written through refs into DOM text nodes at the
existing 500 ms cadence — they never become React state.

**Evidence:** a 10-minute live session against the Windows and Linux testbeds with frame-rate,
decode-time and dropped-frame numbers within 5 % of the pre-migration baseline captured in P0;
input latency spot-checked with the agent API (`/api/v1/input/action`), not the GUI.

### P6 — Delete `ui/`, flip `withGlobalTauri` to `false`

Only after P5 evidence exists. Remove `ui/`, move `capabilities/session-overlay.json` unchanged
(the fullscreen permission set does not change), delete the legacy test files superseded in §7.

**Evidence:** `cargo build --release -p tauri-shell` exit 0, bundle opens, `rg -n "__TAURI__" src/`
returns 0 hits.

### P7 — Release verification

Follow the existing deployment discipline: build, sign, back up to
`~/Library/Application Support/MahoRD-releases/<date>-<sha>/rollback/`, deploy to
`/Applications/MahoRD.app`, verify connect + stream + input against both testbeds.

---

## 5. Token bridge (P1)

Existing value -> shadcn variable. Values are the current hex from `ui/styles.css`; convert to
`oklch()` at implementation time if the team prefers shadcn's default notation, but **do not change
the rendered color**.

| shadcn var | Current source | Value |
|---|---|---|
| `--background` | `--bg` | `#141517` |
| `--foreground` | `--text` | `#f4f5f6` |
| `--card` | `--card-bg` | `#222529` |
| `--card-foreground` | `--text` | `#f4f5f6` |
| `--popover` | `--card-bg` | `#222529` |
| `--muted` | `--disabled-bg` | `#30343a` |
| `--muted-foreground` | `--text-muted` | `#b2b7bf` |
| `--primary` | `--accent` | `#f47660` |
| `--primary-foreground` | `--on-accent` | `#17191c` |
| `--secondary` | `--card-hover` | `#2a2e33` |
| `--border` | `--border` | `#3c4148` |
| `--input` | `--border-control` | `#747c88` |
| `--ring` | `--focus` | `#ffad9c` |
| `--destructive` | `--danger` | `#ffaaa2` |
| `--sidebar` | `--sidebar-bg` | `#1b1d20` |
| `--radius` | `--radius-control` | `0.375rem` (6 px) |

Extensions shadcn does not ship, added via `@theme inline` exactly as its docs prescribe for custom
tokens: `--color-success` (`#83d4ab`), `--color-warning` (`#e9bf72`), `--color-accent-hover`
(`#ff8a75`), `--color-accent-active` (`#dc6652`), `--radius-panel` (10 px), `--shadow-overlay`.

Spacing: `--space-1..10` is already the 4 px scale, i.e. Tailwind's default `1..10` steps. Drop the
custom variables and use `p-2`, `gap-3`, etc. Fonts (`--font-ui`, `--font-data`) map to
`--font-sans` / `--font-mono`.

---

## 6. Component mapping and the hot-path quarantine

### 6.1 DOM -> shadcn

| Current | shadcn component |
|---|---|
| `#sidebar` + `.sidebar-nav` | `sidebar` (with `SidebarProvider`), or plain `nav` + `Button variant="ghost"` if the collapsible behaviour is unwanted |
| `.this-computer-card` | `Card` + `CardHeader`/`CardContent` + `Badge variant="outline"` |
| `.status-badge .badge-online` / `.badge-paused` | `Badge` with `success` / `warning` variants added via cva |
| `#host-search`, `#direct-ip`, `#direct-pin` | `Input`, wrapped in `Form` + `zod` if validation moves off `validateConnection` (it should not — keep validation in `lib/connection.ts`) |
| `.filter-group` (All / Available / Favorites) | `ToggleGroup type="single"` |
| `.host-card` grid | `Card` + `Button` (`Connect`), `Toggle` (favorite star) |
| `.saved-pairing-card` | `Card` + `Badge` + `AlertDialog` on Forget (destructive, currently unconfirmed — a genuine UX upgrade) |
| `#session-connecting-modal` | `Dialog` with `modal` + explicit `onOpenChange` cancel |
| `#session-error` | `Alert variant="destructive"` |
| `#audio-device` native `<select>` | `Select` |
| `#audio-volume` | `Slider` |
| Ad-hoc status text | `Sonner` toasts for transient results only; keep persistent state inline |

### 6.2 Non-negotiable rules for the session surface

1. `SessionCanvas` renders `<canvas>` once. Its `useEffect` has `[]` deps. A re-render of the
   canvas element is a bug — assert it in a test by counting renderer inits.
2. Frame buffers from `poll_frame_raw` never enter React state, context, or a store. They go
   `invoke -> DataView -> texImage2D` inside `renderer.ts`.
3. Per-frame cursor updates write `style.transform` directly on the overlay node through a ref.
4. Stats update at the existing 500 ms cadence through ref writes; if they ever become state,
   memoize so no ancestor of the canvas re-renders.
5. `send_input` dispatch stays synchronous from the DOM event handler. No `startTransition`,
   no debounce beyond the existing ~16 ms pointer-motion rAF flush.
6. React StrictMode double-invokes effects in dev — either disable StrictMode for this subtree or
   make renderer init idempotent. Decide in P5, write it down in the code.

---

## 7. Test migration (the part that will hurt)

| File | Lines | Runner | Fate |
|---|---|---|---|
| `library.test.mjs` | 187 | `bun:test` + `vm.runInNewContext` | **Keep.** Drop the `vm` shim, import `@/lib/library`. Assertions unchanged. |
| `connection-state.test.mjs` | 359 | `bun:test` | **Keep.** Import `@/lib/connection`. Assertions unchanged. |
| `session-overlay.test.mjs` | 294 | `node:test` | **Split.** State assertions -> `bun:test` against `@/lib/overlay`. The markup assertions (`assert.match(indexHtml, /id="btn-pointer-lock"/)`, "index.html ships ... markup") are dead on arrival — id-based markup contracts must become component render tests with `@testing-library/react` + `happy-dom`. |
| `performance.test.mjs` | 243 | `node:test` | **Rewrite.** It does `html.match(/<script>([\s\S]*?)<\/script>/)[1]` and evaluates the inline script in a `vm`. After P6 there is no inline script. Re-target the extracted behaviours at `renderer.ts` / `input.ts` as direct module tests. |

Consequence to accept up front: roughly **540 lines of existing test code (the two `node:test`
files) must be rewritten, not ported.** That is the single largest hidden cost in this migration and
the main reason Option A exists.

New runner policy after migration: everything under `bun test`. The current split
(`bun test` for UMD modules, `node --test` for the HTML-coupled ones) exists only because of the
CommonJS/ESM interop issue noted in the project conventions; TypeScript ESM modules remove it.

---

## 8. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| React re-render enters the 60 fps path | Frame drops, input lag — the product's core value | §6.2 rules; P5 evidence requires within-5 % parity against a P0 baseline |
| Dynamic `invoke` commands dropped during port | Audio controls and host start/stop silently dead | P3 enumerates from `lib.rs`, with a test asserting the command union |
| 540 lines of markup-coupled tests rewritten | Temporary coverage gap around the overlay | Do P5 before deleting `session-overlay.test.mjs`; land replacements in the same commit |
| CSP is `null` today; a bundler + dev server changes asset loading | Blank window in release bundle | Verify the release bundle (not just `bun run dev`) at the end of every phase |
| Bundle size / cold start on a latency-first product | Slower window paint | Measure first-paint in P0 and P6; React + shadcn should stay under ~200 KB gzipped, `dynamic import` the settings panel if not |
| `ios-shell/ui` stays vanilla (app.js 28 KB, its own renderer) | Two divergent UI codebases | Explicitly accepted. Do not half-migrate iOS. Revisit only after desktop P7 |
| Rust build location rule | Wasted cycles | Frontend work (`bun`) runs locally on the Mac with no Rust compile; Rust builds follow the standing Omarchy-only rule and the existing macOS desktop-deployment exception |

---

## 9. Abort criteria

Stop and revert to the last green phase if any holds:

- P5 shows > 5 % regression in FPS, decode time, or dropped frames that is not fixed within one day.
- Input latency becomes perceptibly worse in live use on either testbed.
- P4 cannot reproduce the 200 % text-zoom / `@container` overlay reflow that DESIGN.md requires.

Each phase is a separate commit; `git revert` of a phase must leave a working app. That is the
reason `ui/` is deleted in P6 rather than P0.

---

## 10. Effort

| Phase | Estimate |
|---|---|
| P0 toolchain | 0.5 day |
| P1 tokens | 1 day |
| P2 module port | 1 day |
| P3 IPC boundary | 0.5 day |
| P4 launcher components | 3–4 days |
| P5 session surface | 2–3 days |
| P6 cleanup | 0.5 day |
| P7 release verification | 1 day |
| **Total** | **~9.5–11.5 working days** |

Stopping after P1 (visual parity only, no framework) is ~1.5 days and carries no runtime risk.

---

## 11. Out of scope

`ios-shell`, `maho-host`, `maho-app`, protocol crates, the agent HTTP/MCP API, and the Rust-side IPC
handlers. No dependency, model, or deployment configuration changes. No commit or deployment is
authorized by this document.
