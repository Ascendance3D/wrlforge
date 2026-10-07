# CLAUDE.md

## Working with Ryan

- **Direct assignment is authorization.** If Ryan's current prompt assigns this
  agent implementation, QA, review, runtime/visual testing or architecture
  review, this agent does that work itself with its own tools; it does not ask
  Ryan to choose another model or provider. A "fresh independent QA agent" is
  **the recipient of the prompt** unless Ryan explicitly names another provider.
- **External or paid delegation is opt-in.** Use another model, process, external
  reviewer or paid service only when Ryan explicitly asks for or approves that
  provider in the current conversation. Never spend paid quota without it.
- **Staged execution.** One lane at a time; STOP with a structured report at each
  gate; Ryan gives GO/NO-GO; never continue silently into the next lane.
- **Copy/paste prompts** use one outer four-backtick fence and say whether the
  recipient starts fresh, continues, runs `/clear`, or runs `/exit`.

## Product contract

- WRL Forge is a **standards-first VRML97/X3D model and code editor**.
- The **exact source text is the document**. Code, Scene Tree, Inspector and
  viewport are projections over that one document; visual edits patch it.
- Cybertown is a **profile** over the editor, not its identity.
- **No direct upload**: no Cybertown upload/auth/networking code unless Ryan
  reverses that decision. Do not present it as a missing feature.
- `GPL-3.0-or-later`, DCO sign-off (`CONTRIBUTING.md`). Open-source reuse
  follows `WD.md` §1 and `OPEN_SOURCE_PROVENANCE.md`.

## Architecture invariants

- Read `WD.md` before touching `src/vrml/` or anything that edits a document.
  No second editable document model, canonical scene graph or CST; no AST→text
  whole-file serializer. Every edit is a source-span patch.
- One selection authority (`sceneSelection`); CodeMirror is the undo authority.
- Unsupported or ambiguous identity **fails closed**. Never confidently select
  or edit a different source node than the one proven.
- `src/vrml/` is the **sole** grammar/parser authority: no second parser,
  grammar or competing semantic model. It does not replace `validator.js`,
  World scanning, the preview resolver or packaging without an approved lane.
- **X_ITE is the renderer**, loaded locally (no CDN). Never build a custom
  VRML/X3D renderer. New X_ITE integration points need their own approved lane.
- Runtime dependencies stay **X_ITE-only** unless Ryan approves another. Prefer
  Node built-ins; `zlib` only for gzip and archives. VSCodium is optional.
- File identity is by content (gzip magic bytes), not extension; gzip `.wrl` is
  decompressed for editing and recompressed on save.
- Keep the `mall:*` IPC and `window.vrmlpad` bridge names; no cosmetic renames.

## Profiles

- **Mall Item**, **World Project** and **Generic VRML97** each own their
  validator, preview and page, sharing only generic infrastructure. Rules never
  leak: Mall limits (80 KiB gzip cap, `WorldInfo`, forbidden nodes, texture
  rules) are not World or Generic rules.
- `validator.js` is Mall-only, pure and filesystem-free; keep it in step with
  `../new-items/CLAUDE.md`. A "20 local textures" cap is an unverified web-form
  assumption, never a World rule.
- Mall behavior (gzip, `.edit.wrl`, validation, repack) must not regress. The
  mall `.wrl` is the source of truth; Fit preview is display-only.

## UI and renderer

- Renderer UI is plain HTML/CSS/JS (plus the editor's esbuild bundle); no
  framework or bundler without separate approval.
- UI state is not document state: never in source, analysis or undo history.
- `docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md` is authoritative: toolbars,
  menus and keyboard invoke shared registry commands.
- Keep `test/editor/script-load-order.test.js` in sync with editor-page
  scripts; shared browser modules use module-unique `const` names.

## Security and file safety

- Keep `contextIsolation: true`, `nodeIntegration: false` and a strict CSP with
  no remote origin. Add capability through new IPC handlers; never relax these,
  not even to make QA or preview easier.
- No direct renderer filesystem access. Main owns every path; the renderer never
  sends a write path, and unsaved text never expands authorization.
- Verify before any destructive write. Real overwrites use the shared atomic
  safe-write path with a backup; **no write means no backup**.
- World Project packaging never mutates project sources.

## Tooling and testing

- LSP first where useful; `ast-grep` for structure (by name, never bare `sg`);
  `rg` for text; `fd`/`rg --files` for filenames.
- No recursive shell `grep`, no `grep` in pipelines, no `rg -r*` unless replacing.
- Graphify is available for orientation; `graphify update .` is code-only.
  Do not reinstall or reconfigure it without permission.
- `npm run check` runs the gated suite; `npm start` launches the app.
- Linux is the primary development and validation platform. Keep core logic
  cross-platform-conscious (`path.join`, no hardcoded separators or home paths).
- Electron visual work uses the repository's `VisualQaRunner` harness; do not
  fork ad-hoc capture code or bypass `qa/visual-qa/workspace-guard.js`. This is
  a test-harness rule, not a provider rule.
- `test/fixtures/**` stays byte-exact via `.gitattributes`; no CRLF normalization.

## Authoritative references

- Status: GitHub issues and the Project — never this file.
- Core: `WD.md` · `docs/VRML_PARSER.md` · `docs/NATIVE_EDITOR_ARCHITECTURE.md`
- UI: `docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md`
- Profiles: `docs/MALL_SIZE_CONTRACT.md` · `docs/WORLD_PROJECT_ARCHITECTURE.md`
  · `docs/PREVIEW_ARCHITECTURE.md`
- Platform: `docs/PLATFORM_NOTES.md` · `docs/BUILD.md` · `docs/WINDOWS_QA_RUNBOOK.md`
- Project: `README.md` · `docs/WRL_FORGE_ROADMAP.md` · `CONTRIBUTING.md`
  · `OPEN_SOURCE_PROVENANCE.md` · `SECURITY.md` · `SUPPORT.md`
