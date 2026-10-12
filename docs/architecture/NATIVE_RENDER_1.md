# NATIVE-RENDER-1 — the hidden native viewport

Lane: `feature/native-render-1` at `fa4db9e` (owner decision CONDITIONAL GO,
D1–D15, 2026-10-10). Branch renderer policy governs (D13); `main` is not
changed.

**What it is.** A wgpu (Vulkan / Metal) viewport for the Tauri app that draws
a small, standards-first projection of the document and lets the user SELECT
objects in it. **Selection only**: no manipulation, no second edit path.
**Hidden and off by default** (D7): X_ITE stays the default viewport and the
fallback for every failure.

## Turning it on (test only)

Either of:

- launch flag `--native-viewport`, or
- `settings.json` (Tauri's per-app config dir) containing
  `"viewport": {"renderer": "native-experimental"}`. No UI writes this key.
  Any other value, including `"native"`, a missing key or a corrupt file,
  selects X_ITE.

Test flags: `--native-force-fifo-only` (exercise D8), `--smoke-native <dir>`
(self-test, below), `--smoke-native-control` (the same run with NO native
viewport: measures the main-thread ping response delay without it).

## Layout (D6)

A native split. Linux: the WebView and the viewport are the two panes of a
`GtkPaned`. macOS: the WKWebView and a layer-backed `NSView` are siblings in
the window's content view. No native view is placed over a hole in the
WebView. The WebView's own viewport section collapses to its title and
status lines while the native viewport is shown.

Input in the native pane: click selects, drag orbits, wheel zooms, Escape
clears the selection (in the WebView), Home resets the view.

## Crates and modules

| path | role |
|---|---|
| `apps/desktop-tauri/scene` (`wrlforge-scene`) | Pure. One parse → one `RenderScene`: Transform / Shape / Appearance / Material / Box / Sphere / Cylinder / Cone with ISO 14772-1 defaults and the ISO 6.52 transform order `T·C·R·SR·S·−SR·−C`. Deterministic tessellation (`mesh`), f64 camera (`camera`), CPU pick oracle (`oracle`), native hit → `pick::Hit` (`identity`). No I/O, no GPU, `forbid(unsafe_code)`. |
| `apps/desktop-tauri/render` (`wrlforge-render`) | wgpu renderer (`gpu`) and the render thread (`thread`). Sees only a `RenderScene`: never the text, a path, a file or the network. `forbid(unsafe_code)`; no `unwrap`/`expect`/`panic` (clippy-denied). |
| `src-tauri/src/native.rs` | Tauri-free document side: `show` (project the current text with a new generation; a damaged text keeps the last valid projection), `resolve` (frame → `PickOutcome` via the existing `wrlforge_vrml::pick::resolve`), `selected_ids` (Scene Tree item → highlight). |
| `src-tauri/src/viewport/` | UI-thread host: `mod.rs` (state, events, teardown, deferred close), `linux.rs` + `wl.rs` (X11 / Wayland), `mac.rs` (AppKit / Core Animation), `none.rs` (other platforms: never starts), `smoke.rs` (`--smoke-native`). |
| `ui/src/native.rs` | WebView side: routes the preview to `native_show`, applies native picks through the ONE selection authority (`ui::pick_select`), clears on Escape, asks Rust to highlight the selection. |
| `apps/desktop-tauri/native-smoke/harness.py` | Private-display harness (below). |

`wrlforge-document` stays the only source authority (D3). The projection is
derived and disposable; nothing is ever printed back to text.

## Rendering

- Color pass: MSAA 4× when the adapter supports it; opaque objects in scene
  order, then transparent objects back to front with depth writes off.
- Lighting: ISO 4.14.4 with the default headlight only (ISO 6.29: white,
  intensity 1, ambientIntensity 0): `OE + OD·(N·L) + OS·(N·H)^(128·shininess)`,
  clamped. A NULL appearance or material is unlit white (ISO 4.14.4 / 6.43).
- Mirrored world matrices (`det < 0`) use a clockwise-front pipeline, so
  culling keeps the outside faces.
- Not drawn, and listed in `not_shown` with a reason: every other node type,
  PROTO instances, `USE` (no proven Rust DEF/USE scope resolver yet), shapes
  whose appearance/material/geometry is a `USE`, textures, and any value that
  cannot be read exactly (duplicate field, `IS`, wrong shape, out of range,
  zero scale, zero rotation axis with a non-zero angle). The projection never
  guesses an "intended" value.

## Picking (D14)

1. The click's logical position is taken against the **last drawn frame**,
   before any queued camera, size or scene change. A pick in the same batch
   as a size change is refused (`viewport-resizing`); a hidden frame refuses
   (`frame-not-shown`).
2. CPU oracle: rays through the SAME triangles the GPU draws, in object
   space, with a non-normalized ray; back faces culled by the same rule.
   Refusal band: any point within **1 physical pixel** showing a different
   object → `pick-near-object-edge`; the two nearest objects within `1e-5`
   clip depth → `pick-depth-tie`. Internal triangle edges are not
   boundaries (the band compares object ids).
3. GPU: a separate, non-multisampled 1×1 id pass with the frame's own camera
   and that pixel's projection (`View::pick_proj`).
4. `agree`: oracle refusal first; then a GPU failure or ANY disagreement →
   refused (`gpu-cpu-pick-disagree`). Never a vote.
5. Binding: the reply carries the frame (seq, session, revision,
   generation, text hash). Rust refuses it unless that projection is the one
   on screen now for that session and the text still hashes the same; the
   UI refuses it unless the generation is the one it was told is on screen.
6. Proof: the drawn instance path becomes a plain-data `pick::Hit` and goes
   through the existing `pick::resolve` against the canonical parse and the
   Scene Tree (span join, type join, containment chain, simple-object
   promotion). Sensors refuse first; any DEF on the path that is also `USE`d
   refuses as ambiguous.

## Threads, surfaces, teardown (Step 0 rules)

- One render thread per viewport owns instance, device, queue and surface.
  The UI thread never presents, reads back or polls. Requests go through a
  small `Pending` record; replies are posted with `run_on_main_thread`.
- X11: the render thread opens its OWN XCB connection; GDK's `Display*`
  never crosses threads. Present: FIFO allowed.
- Wayland: a desync `wl_subsurface` on GDK's `wl_display` with an empty input
  region. Present must be Mailbox or Immediate; FIFO-only disables native and
  keeps X_ITE (D8). A buffer-scale change re-creates the viewport.
- macOS: a `CAMetalLayer` sublayer whose delegate is a plain NSObject that
  answers `window` with nil (wgpu 30's occlusion walk otherwise calls
  `-[NSView window]` and `-[NSWindow occlusionState]` on the render thread);
  every configure/draw in an explicit `CATransaction`;
  `allowsNextDrawableTimeout` re-enabled; occlusion read on the main thread.
- Device loss: detected by a 250 ms heartbeat poll; reported; never
  restarted (D9). X_ITE takes over.
- Teardown: the native window/layer lives until `Destroyed`; after 500 ms
  the viewport is parked (hidden, still realized). Window close and app exit
  wait for `Destroyed` up to 5 s, then exit the process without destroying a
  surface a render thread may still use.

## Tests

- `cargo test` (desktop workspace): `wrlforge-scene` 25 (including an
  independent analytic reference: slab ray/box, ray/sphere, hand-written
  Rodrigues transform — no call into the code under test), `src-tauri` 47
  (native resolve/stale/selection/BOM, hidden setting), protocol 26.
- `native-smoke/harness.py xvfb|weston|kwin OUTDIR BINARY [args]`: starts its
  own display server, proves the socket's peer PID is that server, builds an
  allow-listed environment (never `WAYLAND_SOCKET`, no inherited display or
  bus), runs the app under `dbus-run-session`, sends NO input, and adds the
  proof to the report. `NR1_OUTPUT_SCALE` sets the compositor scale.
- `--smoke-native` checks (46): ready + platform rule, projection, picks of
  every class through the WebView's selection authority, UI adoption
  (highlight request), Escape, full-frame GPU/CPU sweep, band share, hidden
  frame, forced disagreement, 3 create/destroy cycles, stale text, blocked
  present + teardown (park), device loss, no thread left, watchdog per phase
  (gated phases: p99 < 50 ms, max < 100 ms).

Evidence: `qa/native-render-1/` (`step0/` and `nr1-linux/`; home paths
replaced by `~`).

## Dependencies (D5)

`apps/desktop-tauri/Cargo.lock`: 500 → 557 packages (57 added, none removed;
2 local). Licenses of the 55 registry additions: MIT OR Apache-2.0 family,
MIT, Zlib OR Apache-2.0 OR MIT, BSD-2-Clause OR Apache-2.0 OR MIT,
Apache-2.0 (`codespan-reporting`, `gethostname`, `spirv`), ISC
(`libloading`). All GPL-3.0-or-later compatible. Direct additions are pinned:
`wgpu =30.0.1` (features `std, parking_lot, wgsl, vulkan, metal`; no DX12,
GLES or WebGPU), `glam =0.30.10`, `bytemuck =1.25.2`, `pollster =1.0.1`,
`raw-window-handle =0.6.2`, Linux `x11rb =0.13.2` (`dl-libxcb`),
`wayland-client =0.31.15`, `wayland-backend =0.3.17`, and GTK/GDK and objc2
crates already in the lock. `crates/Cargo.lock` is unchanged (D4). No
upstream source was copied (no provenance entry needed). Scena and OxideAV
are deferred (D12).

## Results (2026-10-10)

`--smoke-native`, final build, each on a private display proven by peer PID
(Linux) or the owner-approved M1 session (macOS):

| system | adapter / present | scale | result |
|---|---|---|---|
| Xvfb (X11) | NVIDIA RTX 3060 Ti, Vulkan, FIFO | 1, 2 | 46/46 (45/45 before the idle-baseline phase was added) |
| Xvfb (X11) | llvmpipe, Vulkan, FIFO | 1 | 45/45 |
| weston 13 headless | llvmpipe (NVIDIA cannot present there), Mailbox | 1, 2 | 46/46, 45/45 |
| KWin 5.27 `--virtual` | NVIDIA, Mailbox | 1, 2 | 46/46, 45/45 |
| KWin 5.27 `--virtual` | llvmpipe, Mailbox | 1 | 45/45 |
| KWin 5.27 `--virtual` | NVIDIA, Mailbox | 1.5 | 4 of 5 runs pass; 1 run failed the watchdog gate (below) |
| weston, `--native-force-fifo-only` | — | 1 | 4/4: native disabled, X_ITE kept (D8) |
| Apple M1, macOS 27.0.1 | Metal, FIFO | 2 | 46/46; under Main Thread Checker (dylib load proven): 46/46, 0 reports |

Open finding (D14): KWin `--virtual` at output scale 1.5, gpu-load phase,
first run p99 77 ms / max 85 ms (gate: p99 < 50, max < 100). Four later runs
at 1.5 passed (p99 15–24 ms). The X_ITE-only control in the same environment
measures a main-thread ping response delay of p99 14.6 ms / max 19.6 ms
(scale 1: 3.3 / 4.4 ms). These figures do not show that fractional scaling
causes the normal ~14 ms delay; that cause is not measured. Cause not proven.
Not reported as fixed.

Closeout follow-up (same day, `qa/native-render-1/closeout-watchdog/`): the
smoke now writes `<report>.raw.json` (every ping, phase changes, and the UI
and render threads' `/proc` scheduler state every 1 ms); the harness adds
GPU use and compositor CPU. The gate is unchanged. KWin 1.5, interleaved:
native 12/12 pass (gpu-load p99 14.2–15.8 ms, max 14.8–27.5 ms), X_ITE-only
control 12/12 (p99 13.9–23.1 ms, max 14.4–31.8 ms). Both renderer modes show
similar delay. With all 24 cores saturated, both modes far exceed the limits
(native p99 282–1005 ms, control 458–1271 ms).

What the watchdog measures: the main-thread ping response delay. The
watchdog thread posts a ping every 5 ms (it does not wait for a reply), so
several pings can wait at the same time; tao runs one queued event per GTK
main-loop pass. The main thread can be busy while a ping waits in that
queue, so a long sample is not by itself a blocked UI callback.

In the first 20 runs, the closeout rule (UI CPU time ≥ 60 % of the window)
marked 3,010 of 3,011 grouped delay windows of 10 ms or more as busy. That
rule ignores queue delay and does not prove continuous CPU execution. The
self-review (`qa/native-render-1/closeout-self-review/`) checked CPU time and
run-queue wait for the three longest samples of each run only; findings for
those samples are not extended to other samples.

The original failure did not return and its cause remains unknown; its saved
report has summaries only. Nearest rank on its 920 samples (p99 77.03 ms, max
85.02 ms) gives at least 10 samples of 77.03 ms or more. That does not show
how many delay episodes occurred: in the self-review, one episode delayed
6 (native) and 12 (control) queued pings to within 8 ms of its maximum. The
number and cause of the original delay episodes remain unknown.

Non-regression: the existing `smoke.sh --headless --create --pick --move`
(X_ITE path; 34 fixtures, 140 clicks) passes with this branch.

## Limits (not claimed)

- Real GNOME Wayland, Plasma 6, fractional scale on a real session, runtime
  scale change and the oldest supported macOS: pre-production stage (D10),
  not run. Headless GNOME was not available here.
- Cinnamon and Intel Mac: open, no support claimed.
- NR1 sends no OS input; clicks are injected at the host (after GTK/AppKit).
  Real pointer input on a live session needs owner approval (D11).
- The macOS no-delegate negative control was run in Step 0 only (the product
  has no switch to remove the delegate).
- Drawn scope is small on purpose: `USE`, textures, PROTO instances and all
  other node types are listed in `not_shown`, not drawn.
