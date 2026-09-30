# CLAUDE.md

## Mission

**WRL Forge** — *Model. Edit. Preview. Validate. Package.*

WRL Forge is a **standards-first VRML97/X3D model and code editor**. Its spine is
a lossless document core: the exact source text is the document, and every view
onto it — code, scene tree, inspector, viewport — is a derived projection that
edits back through source-text patches.

That core is the product. Cybertown is a **profile over it**, not its identity.

@WD.md — the lossless document core (canonical model, node schema, node identity,
scope semantics, and the **open-source reuse policy**). Read it before touching
`src/vrml/` or designing anything that edits rather than reads.

WRL Forge is **`GPL-3.0-or-later`** free software. Reusing GPL-compatible
open-source implementation material (White Dune included) is permitted with
preserved notices and recorded provenance — see `OPEN_SOURCE_PROVENANCE.md`.
Contributions are accepted under the same licence, with DCO sign-off and no
copyright assignment — see `CONTRIBUTING.md`.

### The two halves

1. **Code editing** — a native WRL editor with real VRML97 syntax highlighting,
   parser diagnostics, outline, and live 3D preview of the *unsaved* buffer
   (Phase 7A/7B/7C, built). No external editor required.
2. **Model editing** — visual authoring over the same document: scene tree,
   typed field inspector, viewport manipulation, PROTO/ROUTE tooling. The
   document core (WD1.1–WD1.5) is built and gated; the authoring UI (**WD2**) is
   **not started** and needs its own approved lane.

Both halves edit **one** document. Neither owns a private copy of it.

### Profiles, not products

The standards core applies to every VRML97 document. Three profile layers sit on
top, and **their rules never leak downward or sideways**:

1. **Mall Item** — the original, complete lane. Cybertown Mall upload rules
   (80KB gzip cap, required `WorldInfo`, forbidden nodes, texture rules,
   placement bounds).
2. **World Project** — a different shape of problem: many local textures, project
   folders, nested `Inline`, **no** 80KB cap and **no** texture limit. Must not
   inherit mall-item restrictions.
3. **Generic VRML97** — inspection/preview with **no** Cybertown validation unless
   a Cybertown profile is explicitly enabled.

Do not copy Mall Item validation rules into World Project or Generic code paths as
though they universally apply. Each profile gets its own validator, sharing only
genuinely generic infrastructure (gzip handling, file I/O, parsing primitives).

The project was previously named `vrmlpad` and scoped narrowly to Cybertown Mall
items. That narrow tool is the working foundation this expansion builds on — it is
kept and grown into one profile of a larger editor, never rewritten away. See
`docs/WRL_FORGE_ROADMAP.md` for the phased plan.

### The two things a generic editor cannot do for Cybertown

1. **Gzip transparency** — the mall's actual upload files are gzip-compressed
   `.wrl` (real VRML text inside, binary bytes on disk). WRL Forge decompresses
   for editing and recompresses on save.
2. **Cybertown Mall validation** — the upload rules from `../new-items/CLAUDE.md`.
   **Mall Item profile only.**

### Platform

Linux is the first supported platform and must be thoroughly tested — every lane's
validation happens on Linux first. **Windows** has a validated **private, unsigned
test build** (Phase 6A): editor discovery is cross-platform
(`src/editor/editor-locator.js`), filename-case detection works on
case-insensitive filesystems (the directory listing, not `existsSync`, is
authoritative), and the app is packaged with `electron-builder` (MIT; portable
`.exe` + NSIS installer, `npm run build:win`, needs `wine` when cross-building
from Linux — **or builds natively on Windows** via the cross-platform
`scripts/build-win.js` wrapper, Phase 7C5). The full Phase 7C feature set is
**accepted on native Windows 11** (beta `1.3.0-beta.1`); Windows visual QA uses a
file-based capture transport (`qa/visual-qa/transport.js`) since a GUI-subsystem
`electron.exe` cannot read stdin. See `docs/WINDOWS_QA_RUNBOOK.md` and
`qa/phase-7c5-cross-platform/`. Keep reusable core logic cross-platform-conscious
(`path.join`, no hardcoded separators or `/home/<user>` paths). Do not delay Linux
work to speculatively build Windows support first. See `docs/PLATFORM_NOTES.md`
and `docs/BUILD.md`.

### Open-source components

Prefer open-source components; the current stack is:

- **Electron** — application shell.
- **X_ITE** (MIT) — the approved VRML/X3D rendering and preview engine. See
  `spikes/xite-mall-fit/` for the Phase 2A spike and its `NOTES.md`.
- **CodeMirror 6** (`@codemirror/*`, `@lezer/highlight`) + **esbuild** — MIT
  **devDependencies** for the native editor; the bundle is
  `renderer/vendor/wrl-editor.bundle.js` (gitignored, `npm run build:editor`).
  **No CDN, no second grammar** — `src/vrml` is the sole authority via
  `src/editor/language.js`.
- **VSCodium** — **optional** external editor (not bundled). An integration, not
  a dependency.
- **Node.js built-ins** — `zlib`, `fs`, `path`, `child_process`, `node:test` —
  preferred over adding a dependency where they suffice (no external test
  framework; no third-party archive library — the World bundle ZIP is built on
  `zlib` alone).
- **@resvg/resvg-js** (MPL-2.0, **devDependency**) — SVG→PNG only for
  `scripts/build-icons.js`. Never `require`d at runtime. See `docs/ICONS.md`.

Runtime dependencies stay **`x_ite`-only**. Before adding any dependency
(editor component, 3D lib, UI framework, archive library), re-read this section.

**Do not build a custom VRML/X3D renderer under any circumstances** — X_ITE is the
approved engine.

## Search and Code Navigation

* Use LSP first for definitions, references, implementations, symbols, types, and diagnostics when available.
* Use `ast-grep` for structural code queries when installed; use text search only for literal or regex matching.
* For text search, prefer the built-in Grep tool, `rg`, or `git grep`; for filenames, prefer `fd`/`fdfind` or `rg --files`.
* Do not use recursive shell `grep`, broad `find | xargs grep`, or custom Python/Perl crawlers unless the preferred tools cannot perform the task.
* Ripgrep recurses by default: never use `rg -r`, `rg -rn`, `rg -rl`, or similar bundled forms unless replacement output is explicitly intended.
* Scope searches to the smallest relevant directory, file type, or glob, and limit output before widening the search.

### Tool availability on this machine (verify, don't assume)

- `rg` — installed, the default.
- **`ast-grep` — installed, version `0.45.0`** (as of 2026-08-06), at
  `~/.local/bin/ast-grep`. That is the executable to invoke; the bullet above is
  conditional (*"when installed"*) and the condition is now met.
  **Always call `ast-grep` by name, never bare `sg`** — two different `sg`
  binaries exist on this machine and which one a shell resolves depends on
  `PATH` order: `~/.local/bin/sg` is ast-grep's own deprecated alias, while
  `/usr/bin/sg` is the unrelated shadow-utils setgid command. Never assume which
  one a shell will pick without checking.
- `mgrep` — installed but **not usable**: the CLI has a token at `~/.mgrep/token.json`
  and no provisioned store (`404 Stores with identifiers 'mgrep' not found`), and
  `-s` does not create one. Don't route work through it until that is fixed.
- **graphify** — the semantic knowledge graph. Orient with it before ad-hoc file
  reads. `graphify update .` is **code-only and needs no LLM**; do not reach for
  `extract --backend gemini` when `update` suffices.
- Never put `grep` in a pipeline. `grep -c` prints `0` *and* exits non-zero, so
  `grep -c … || echo 0` yields `"0\n0"` and silently corrupts the caller.

## Working with Ryan

These are workflow rules, not code rules. They bind every agent in this repo.

- **Never pick a QA or review tool yourself.** A task prompt naming one is *not*
  authorization — ask first. The approved routing is: **MiniMax M3** for major
  code/architecture QA · **MiniMax M2.7** for large text parsing · **AGY
  (Antigravity)** for browser/visual/runtime QA · **Copilot for automatic PR
  review only**. No manual Codex, Codex CLI, Copilot CLI, or Gemini CLI QA.
- **Never spend paid tool quota without Ryan's explicit approval** in that
  conversation.
- **Staged execution.** Multi-phase plans run one lane at a time, with a
  structured STOP + report and a GO/NO-GO between phases. Never run a whole
  multi-lane plan uninterrupted.
- **Copy/paste prompts** use a single outer four-backtick fence, and state
  explicitly whether the recipient should start a fresh session, continue the
  current one, `/clear`, or `/exit`.

## Parser and native editor

The **VRML97 parser** is built (Phase 7A, corpus-hardened in 7A1): a
dependency-free, token-driven tokenizer + structural parser under `src/vrml/`
(`tokenizer`/`parser`/`ast`/`diagnostics`/`analyze`/`asset-refs`/`index`; see
`docs/VRML_PARSER.md`). It is **pure** — text in, tree + diagnostics + semantic
index + asset-refs out, no fs, no Electron — and profile-neutral. Reuse it
(`require('./src/vrml')`); do not fork it.

7A1 fixed three real-corpus rejections (hyphen/plus identifiers, multiline
strings, header encoding case) and leniently accepts Cybertown/Blaxxun
`ROUTE`/`PROTO`-in-MFNode-array — a **98.1%** corpus diagnostic reduction.

**Its semantic scope is flat and NOT authoritative** (PROTO DEF leakage,
cross-PROTO false duplicate-`DEF`, USE-before-DEF, context-insensitive `IS`).
`@WD.md` §8 records exactly how far that goes and the accepted design for fixing
it. Until that lane lands, do **not** present `VRML040`–`VRML044` as
authoritative.

The parser **is** wired into the native editor for highlighting, syntax
diagnostics, advisories and outline. It has **not** replaced `validator.js`, the
World Project scanner, the preview resolver, packaging, or URL extraction — those
run on their own code paths and must not be routed through it without a new
approved lane.

The **native editor** (Phase 7B, `docs/NATIVE_EDITOR_ARCHITECTURE.md`) is a
CodeMirror 6 workspace (`renderer/editor.html`/`editor.js`, `src/editor/*`,
`browser/editor-view.js`) reachable from both lanes via `editor:*` IPC and the
`window.vrmlpad.editor` bridge. **Main owns every path**: the renderer sends
text + intent + an opaque `sessionId`, never a write path; Save As uses a main
dialog; a World reference is authorized against the scan graph + realpath.
Verify-before-commit save, timestamped backup, external-change
Reload/Save-As/Cancel, gzip transparency, five themes (incl. **High Contrast**).

**Vision accommodations (Feature A)** are built and **renderer-only** (no
main/preload/IPC/CSP change): a persisted zoom level (`Ctrl` `+`/`-`/`0`) scales
the code area via a **font compartment** in `editor-view.js` (decoupled from the
theme compartment) *and* the chrome via a `--wrl-ui-scale` rem layer. Zoom math is
pure in `ui-state.js` (`resolveZoom`/`zoomStep`/`zoomModel`). Reuse it; do not
CSS-transform the X_ITE canvas with the chrome scale.

Keep `test/editor/script-load-order.test.js` in sync when adding editor-page
scripts — shared browser modules must use **module-unique const names** or they
collide in the shared script scope.

## Preview

Embedded X_ITE previews ship for **Mall Item** (Phase 2B1) and **World Project**
(Phase 4B). `x_ite` is a root dependency loaded **locally, never a CDN**.

The two are **separate profiles**: `renderer/preview.js` does Mall
Original/Cybertown Fit with placement bounds; `renderer/world-preview.js` renders
a whole world (nested `Inline`, gzip, ≥70 textures) with **no** Mall
fit/guide/placement/cap rules. The **Generic VRML97** embedded preview (Phase 5)
still needs its own approved lane — shipping the World preview does not license
opportunistic X_ITE integration elsewhere.

**Live preview of the unsaved buffer is built for both profiles** — Mall (7C2) and
World (7C3) — split-view, **no temp file ever written**. Its foundation is three
pure `src/preview/` modules: `buffer-overlay.js` (session-scoped,
**byte-substitution-only**, **never authorizes a path**, requires an authorization
proof), `preview-state.js` (last-valid-scene state machine), and
`preview-scheduler.js` (clock-injected 700 ms debounce).

The bridges are pure/injectable main-process authorizers:
`mall-preview-bridge.js` builds its proof from the held Mall source;
`world-preview-bridge.js` authorizes the held document against the **current scan
graph** (root match, membership, exact-case, realpath re-check). Unsaved text
**never expands authorization by itself** — `editor:previewRescan` ("Find new
files") is the explicit, normal rescan. Both reuse the existing renderers
verbatim through an injected source loader.

Shared modules live in `src/preview/` — reuse them, don't duplicate, and don't
route `validator.js` / World scanning / packaging through them. See
`docs/PREVIEW_ARCHITECTURE.md`.

A narrowly-scoped spike at `spikes/xite-mall-fit/` (Phase 2A) is a deliberate
exception to "don't begin that work without approval" — commissioned to de-risk
Phase 5/2B in complete isolation. It does **not** constitute Phase 5's approval.

## Locked product decisions

- **Direct upload will not be built** (2026-07-12). No upload, auth, or
  networking code. Users upload by hand through the Cybertown website. **Do not
  present its absence as a missing or roadmap feature.**
- **No custom renderer.** X_ITE only.
- **No third-party archive dependency.** Node `zlib` only.
- **VSCodium is optional**, never required.

### Unverified assumptions — do not encode as fact

- The old Cybertown item-upload web form limited items to 20 local textures. This
  may be a limitation of that **web form**, not a server-side limit. World
  Projects must not inherit an arbitrary 20-texture cap — real worlds reach ~70.

## Profile detail

### Mall Item (complete)

Open plain or gzip `.wrl` · create a plain `.edit.wrl` working copy · edit
natively (or optionally launch VSCodium) · validate Mall rules · show placement,
offsets, scaling and bounds (advisory) · backup and repack.

### World Project (Phase 4A/4B/5A)

A read-only asset resolver and workspace (`src/world-project/`,
`renderer/world.html`, confined `world:*` IPC):

- Open a primary world `.wrl`/`.wrz` **or** a project folder (ambiguity is
  surfaced, never guessed)
- Discover textures and local VRML assets across nested `Inline` (gzip and plain,
  bounded + cycle-safe), with **no arbitrary texture limit**
- Detect missing files, filename-case mismatches, absolute/traversal paths,
  remote references (surfaced, never fetched), duplicates and cycles
- Report via `src/world-project/profile.js`, **structurally separate** from
  `validator.js`, with confidence-tagged findings
- **Preview** the whole world through a confined `wrlworld://` scheme served
  **only** from the asset-graph allow-list, gzip-decompressed, confined to the
  project root. Inline scripts never run. Analysis + display only.
- **Audit + package** into a portable **WRL Forge World Project Bundle** (Phase
  5A): a deterministic plan (`package-plan.js`), a deterministic ZIP
  (`zip-writer.js`, `zlib` only), written by `bundle-builder.js` to a destination
  **outside** the project. Missing/case/absolute/traversal/remote/unreadable
  **block**; cycles don't. Refuses blocked/in-project/overwrite and re-hashes
  every file against the manifest.

It is a **review + manual hand-off bundle** — no direct upload, no
server-certified-format claim (open questions in
`docs/WORLD_PACKAGE_QUESTIONS.md`). Everything except that one explicit write is
read-only. Automatic path repair, copy, rename, delete and Apply/Bake are **not**
implemented; each is its own future approved lane.

See `docs/WORLD_PROJECT_ARCHITECTURE.md`.

### Generic VRML97 (planned)

Open plain or gzip VRML97 · inspect syntax and structure · resolve local assets ·
preview and report scene information · apply **no** Cybertown validation unless
the user opts into a Cybertown profile.

## Architecture

- `main.js` — Electron main process. Owns all filesystem/IPC logic:
  - `isGzip()` sniffs the gzip magic bytes (`1f 8b`) — file identity is by content, not extension. A `.wrl` on disk may be gzip or plain text; both are valid mall artifacts at different pipeline stages.
  - `editPathFor()` derives the plain working-copy path: `<name>.wrl` → `<name>.edit.wrl`, written next to the mall file so it inherits VSCodium's workspace trust for `~/Projects/cybertown` (untrusted folders run extensions in Restricted Mode, which silently disables 3D preview — always keep edit files inside a trusted tree).
  - `openMallFile()` — decompress-if-needed, write `.edit.wrl`. Opening a Mall item **never** launches the external editor; that happens only through the explicit "Open in External Editor" action.
  - `mall:check` — re-validate the current `.edit.wrl` on demand (polled every 3s by the renderer while a file is open).
  - `mall:repack` — a **thin** handler that hands the write to `src/mall/repack.js`.
    It does **not** write the mall file itself and no longer runs its own
    backup-before-write. The shared safe-write discipline in
    `src/editor/file-io.js` runs: an **unchanged gzip artifact is preserved**
    (no write, no backup, no new mtime), a changed one is encoded to a
    **verified in-memory candidate**, refused before any mutation if it is over
    the exact **81,920 B** (80 KiB) upload limit (`ESIZE`), and otherwise written to a
    temp sibling, fsynced, read back, verified, backed up
    (`<name>.wrl.bak-<timestamp>`) and atomically renamed into place. Lane A
    then measures the **real** file on disk for the verdict — the pre-write
    candidate count is never reported as the upload size. See
    `docs/MALL_SIZE_CONTRACT.md`.
  - `loadWindowState()` / `saveWindowState()` — window position/size persistence, with a fallback that reads the pre-rename `vrmlpad` userData directory (see "Rename note").
  - The `mall:*` IPC channel names and the `window.vrmlpad` bridge object name are retained from the pre-rename codebase. They are internal symbols, not user-facing branding — do not rename them for cosmetic consistency. The World Project lane deliberately reuses the `window.vrmlpad` bridge (adding a `world` sub-object) rather than introducing a new bridge name.
- `preload.js` — contextBridge. Keep `contextIsolation: true` / `nodeIntegration: false`; add new capabilities as new IPC handlers, never by relaxing this. This applies to the embedded X_ITE preview too.
- `src/vrml/` — the parser and the lossless document core. See `@WD.md`.
- `src/preview/` — shared preview/fit modules, single source of truth reused by the production app and the isolated spike: `fit-math.js`, `extrusion-bounds.js`, `bbox-traversal.js` (browser-only), `guides.js`, `texture-base.js`, `wrl-source.js` (main-process), `url-policy.js`, plus the unsaved-buffer foundation and the two preview bridges. Pure/browser modules keep no Electron/fs dependency so they are `node:test`-able.
- `src/editor/` — `editor-locator.js` (cross-platform external-editor discovery, pure/injectable), `wrl-document`, `file-io`, `language`, `session`, `session-store`, `path-authorizer`, `editor-controller`, `ui-state`. `src/settings/app-settings.js` is a read-only `settings.json` under userData.
- `src/world-project/` — the World Project profile: pure/injectable modules (`url-fields`, `path-policy`, `image-size`, `asset-graph`, `profile`, `project-stats`, `session`), the main-process `project-loader`, `preview-source`, and the packaging trio (`package-plan`, `zip-writer`, `bundle-builder`). `qa/world-recon/*` re-export these — don't duplicate them.
- `renderer/` — plain HTML/CSS/JS, **no framework, no bundler** (except the editor's esbuild bundle). `index.html` + `renderer.js` are the Mall lane; `world.html` + `world.js` the World lane; `editor.html` + `editor.js` the native editor. Each carries a strict CSP with **no remote origin**; the editor's CSP is the World superset (adds LOCAL `wrlworld:` — still **no new scheme**). Navigation between pages is main-controlled (`app:goto`, whitelisted). Each new profile gets its own clearly-labeled page rather than overloading another.
- `validator.js` — pure `validate(text) -> { results, ok, gzipBytes, rawBytes }`. No filesystem access. Mirrors the Mall Item rules in `../new-items/CLAUDE.md`. If the mall rules change, update both together — they must not drift. **Mall Item-specific**; a World Project validator is a separate module, not an extension of this one.

### Rename note (vrmlpad → WRL Forge)

- `package.json` `name` changed from `vrmlpad` to `wrl-forge`, so
  `app.getPath('userData')` moved from `~/.config/vrmlpad` to `~/.config/wrl-forge`.
- `main.js` migrates window-state on first read (read-only; it doesn't delete or
  move the old file).
- The `mall:*` channels and `window.vrmlpad` bridge name were deliberately **not**
  renamed — see Architecture.

## Conventions

- **Backups before any overwrite** of a real mall `.wrl` — never repack without
  the backup step (now owned by `safeSave`, taken after verification and
  immediately before the atomic rename). A repack that writes **nothing** —
  preservation, or a refusal — correctly takes **no** backup: the owner policy is
  *no write means no backup*. This non-destructive/backup-first convention is
  mandatory for **every** profile. World Project packaging honours it by never
  mutating the source at all.
- `.edit.wrl` working copies are disposable/regenerable; the mall `.wrl` (or its
  most recent backup) is the source of truth.
- No bundler or framework dependency for the renderer. If it starts needing state
  management or routing, that is a sign scope has crept beyond what's warranted
  and should be reconsidered, not built out.
- Keep `validator.js` pure and dependency-free (Node's `zlib` only). Future
  profile validators follow the same pure-function, no-filesystem shape.
- Existing Mall Item functionality (gzip handling, `.edit.wrl` workflow,
  backup/repack, validation) must not regress as new profiles are added. Treat it
  as a contract, not a first draft.
- Electron visual work routes through `VisualQaRunner` only — never a
  per-capture Electron launch. On **Windows**, `qa/visual-qa/workspace-guard.js`
  refuses UNC / network-drive / host-share workspaces and is a **no-op on Linux**.
  Don't route around it.
- Test fixtures stay byte-exact across platforms via the root `.gitattributes`
  (`-text` on `test/fixtures/**`). Do **not** add CRLF-normalizing rules — they
  corrupt the gzip/CRLF twins.

## Workflow (Mall Item lane)

1. `npm start` (== `electron .`) launches the panel.
2. "Open mall .wrl…" → picks a file, writes `.edit.wrl`, loads the item into the
   embedded preview.
3. Edit in the native editor (or click "Open in External Editor" for VSCodium, if
   installed). The preview shows **Original** and **Cybertown Fit** with
   transform-aware bounds and placement guides.
4. Validation re-runs automatically every few seconds. The Fit preview is
   **display-only** — it never rewrites your file (Apply/Bake is not implemented).
5. "Repack & Save to mall .wrl" writes the gzip mall file — the actual
   `.edit.wrl` text, never a preview-fitted transform. It does **not** always
   write: if the existing gzip artifact already decompresses to exactly this
   text it is **preserved untouched** (the button says *Already saved ✓*), which
   is what keeps a Zopfli-packed item from being re-encoded into an over-limit
   file. A changed document that would exceed 81,920 B is **refused before
   anything is written** (*Not saved*), leaving the existing artifact
   byte-identical. A real write backs up first, then swaps atomically.

## Known gotchas (found during build/verification)

- GTK's file-open dialog does not reliably accept `Ctrl+L` + typed path + `Return`
  in this environment — typed text can land in the fuzzy-search box instead of the
  location bar. Navigating via the folder tree/breadcrumbs is reliable.
- A file opened from an untrusted folder (e.g. `/tmp`) puts VSCodium in Restricted
  Mode, which disables the X_ITE preview extension **with no obvious error** — the
  `X3D: Preview 3D Model` command simply won't appear. This is why `.edit.wrl`
  siblings are written next to the mall file inside the already-trusted
  `~/Projects/cybertown` tree rather than to a temp directory.
- On **Windows**, never run dev/QA/build from a host SMB share, a UNC path, or a
  mapped network drive — running `npm ci`/builds/fixture-writing QA there wiped
  `node_modules`. Now enforced by `qa/visual-qa/workspace-guard.js` (Phase 7C4.1),
  wired into `qa:windows`, `qa:visual`, the packed self-test, and
  `build:win`/`build:win:portable`. Clone to a local NTFS path such as
  `C:\Projects\wrlforge`.
- A GUI-subsystem `electron.exe` on Windows has a **dead `process.stdin`** — the
  capture server uses a file-based job transport there
  (`qa/visual-qa/transport.js`, `WRL_FORGE_CAPTURE_JOBS_FILE`). The POSIX stdin
  path is unchanged; reuse `VisualQaRunner`'s hooks, don't fork.
