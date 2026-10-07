# WRL Forge Roadmap

Status key: ✅ done · 🚧 current lane · ⏳ planned · ⛔ deferred (not approved)

This roadmap is scoped in phases. Each phase lists prerequisites, risks, and completion criteria. Do not begin a phase without prior approval — this document describes the plan, it is not itself authorization to start implementation.

## Phase 0 — Existing Foundation ✅

The working `vrmlpad` tool, now the Mall Item lane of WRL Forge.

- Gzip-transparent file handling (`isGzip`, decompress/recompress)
- External VSCodium editing via `.edit.wrl` sibling files
- Cybertown Mall item validation (`validator.js`)
- Backup-before-overwrite repack workflow
- Status-panel UI with live re-validation polling
- Window position/size persistence

**Completion criteria:** met — this phase is the shipped, working state as of the rename.

## Phase 1 — Product Rename and Profile Foundation ✅

- Rename `vrmlpad` → **WRL Forge** across package metadata, branding, docs, and launchers
- Introduce the explicit three-profile model (Mall Item / World Project / Generic VRML97) in documentation
- Sketch a validation-profile architecture (separate validator modules per profile, no shared Cybertown-specific logic)
- Git baseline established, automated test foundation added (`npm test` / `npm run check`)
- **No major feature implementation** — this phase is naming, documentation, and infrastructure only

**Prerequisites:** none beyond the Phase 0 foundation.

**Risks:**
- Electron's `userData` path is derived from `package.json` `name`; renaming it silently loses saved window state without a migration fallback. *(Mitigated: `main.js` falls back to the old `~/.config/vrmlpad` path — see AGENTS.md "Rename note".)*
- Over-renaming internal symbols (IPC channels, bridge object names) purely for cosmetic consistency adds risk without user-facing benefit. *(Mitigated: `mall:*` channels and `window.vrmlpad` were deliberately retained.)*

**Completion criteria:** met.
- All user-facing branding (window title, panel UI, desktop launcher, package description) reads "WRL Forge"
- Existing Mall Item functionality verified unregressed (open/edit/validate/repack/backup/window-state)
- `AGENTS.md`, `CLAUDE.md`, `README.md`, and this roadmap reflect the expanded mission
- A git repository with a clean baseline commit exists, and a `node:test` suite covers the extracted pure modules plus `validator.js`

## Phase 2A — Fit Engine and X_ITE Technical Spike 🚧 (this lane)

- A pure, independently-tested fit-math module encoding the Cybertown Mall Item fit rules (ground `Y=-1.75`, center `X=0`, max `Z<=+1`, max dims `10x10x10`, default requested scale `125%`), decoupled from how a bounding box is obtained
- An isolated technical spike (`spikes/xite-mall-fit/`) proving out X_ITE as the bounds source: load a VRML97 item, derive a transform-aware world-space bounding box by walking the parsed scene graph (no regex/string scraping of geometry), and render a non-exported guide overlay (ground plane, center axis, Z-limit plane, 10m cage)
- Explicitly **not** integrated into the production app — the spike has its own isolated Electron process, its own fixtures, no shared IPC surface with `main.js`
- Explicitly display-only — no apply/bake/mutation path exists in the spike

**Prerequisites:** Phase 1 complete.

**Risks:**
- Silent geometry mutation would violate the non-destructive convention — mitigated by the spike having no write-capable code path at all, not just an unused one.
- X_ITE might not expose a trustworthy, transform-aware bounding box — confirmed during this phase: it does not expose one publicly, so bounds are computed via manual `SFMatrix4` world-transform accumulation over the parsed scene graph instead (see `spikes/xite-mall-fit/NOTES.md`).

**Completion criteria:** met.
- Fit-math module has `node:test` coverage of the required edge cases (compliant, off-center, above/below ground, exceeds Z limit, exceeds 10m, zero-size axis, negative coordinates, nested-transform, rotated, custom rules)
- Spike demonstrates transform-aware bbox extraction (verified against a nested-transform fixture and a rotated fixture by hand-calculation, plus a real Cybertown Mall item) and honestly documents where confidence is lower (Extrusion, DEF/USE, texture resolution — see NOTES.md)
- Findings are recorded in `spikes/xite-mall-fit/NOTES.md` to inform Phase 2B scoping

## Phase 2B0 — Extrusion / Gzip / Texture Remediation ✅ (this lane)

A narrow remediation lane required by the independent Phase 2A QA's
**CONDITIONAL GO** before any Phase 2B production UI work. Kept inside the
isolated spike — no production app code changed.

- **Extrusion bounds corrected**: the QA blocker (scale/orientation ignored →
  dangerous width/depth underestimate) is fixed with an exact VRML97
  cross-section sweep (`spikes/xite-mall-fit/extrusion-bounds.js`), verified
  EXACT against X_ITE's own generated-mesh bounds on 9 fixtures plus
  hand-derived transformed cases, with a conservative (never-smaller) fallback
  for ambiguous spines.
- **Gzip → X_ITE**: X_ITE now receives decompressed text only, via a read-only
  main-process channel reusing the production `isGzip` helper.
- **Relative textures** resolve against the source `.wrl`'s directory
  (`browser.baseURL`); missing/case-mismatch textures warn clearly without
  breaking bounds.
- Security posture preserved (`contextIsolation:true`, `nodeIntegration:false`,
  read-only IPC confined to `fixtures/`, no write path).

**Completion criteria:** met. Evidence:
`qa/phase-2b0-extrusion-loading/RESULTS.md`; tests in
`spikes/xite-mall-fit/*.test.js`. This closes Phase 2B's open items (Extrusion
accuracy, local texture resolution, gzip-to-X_ITE), so **Phase 2B is now
unblocked** pending its own approved lane.

## Phase 2B — Mall Item Fit Production UI ✅

Delivered as **Phase 2B1** (the production integration lane; Phase 2B0 above was
the preceding remediation). The proven spike modules were promoted to
`src/preview/` (single source of truth — the spike now references them, no
duplicate implementations), `x_ite` (MIT, v15.1.10) was added to the root
dependencies, and an embedded X_ITE preview was wired into the Mall Item
workspace. This is the first point X_ITE enters `main.js`/`renderer/`.

- Embedded X_ITE preview *inside the production app*, loaded via a read-only
  main-process channel (`preview:load`, role-based, gzip decompressed in main;
  X_ITE only ever receives plain text).
- **Original** vs. **Cybertown Fit** modes. Fit mode applies a **preview-only**
  parent `Transform` (scale+offset from the authoritative bounds) plus the
  non-exported guide overlay — never written to any file.
- Cybertown guide overlays (ground plane, center axis, Z-limit plane, 10m cage,
  optional item box) with per-guide toggles, reusing the Phase 2A guide layer.
- Live bounds/scale/offset/rule report from the shared `fit-math` module, driven
  by the transform-aware X_ITE bounds (`bbox-traversal`); honest confidence
  (exact | conservative | unavailable).
- The validator's advisory untransformed placement line is **suppressed** when
  authoritative bounds are present, so placement verdicts never conflict.
- Layered texture/URL security: read-only path-free IPC, `safeResolve`
  confinement, `session.webRequest` remote-request cancellation, strict CSP,
  `contextIsolation`/`nodeIntegration` unchanged. Remote URLs are blocked and
  tested; path traversal is blocked and tested.
- A permanent in-repository DEF/USE fixture (`test/fixtures/preview/def-use.wrl`)
  plus an Electron preview test that verifies both occurrences are counted.

**Prerequisites:** met — Phase 2A reviewed; Phase 2B0 resolved Extrusion
accuracy, local texture resolution, and gzip-to-X_ITE loading.

**Completion criteria:** met. Evidence:
`qa/phase-2b1-production-fit/RESULTS.md`, `docs/PREVIEW_ARCHITECTURE.md`, and the
78-test suite (incl. Electron smoke + preview tests). No geometry is mutated;
the fit is preview-only. Apply/Bake Transform remains **not** implemented
(deferred; requires a separate approved lane).

**Explicitly not implemented in this lane:** Apply Transform, Bake Transform,
coordinate rewriting, wrapper insertion, automatic fitted-file saving.

## Phase 3 — World Project Recon 🔄

**Phase 3A (recon + asset graph) landed** — see `docs/WORLD_PROJECT_RECON.md` and the
read-only `qa/world-recon/` analyzer (`npm run recon:world`). Evidence gathered over
71 archived places + the CTR bundled world.

- [x] Collect known-good world `.wrl`/project samples for reference (campuscolony archive of 71 places; CTR `hitek_col`)
- [x] Determine actual texture-count limits — **the ~20-texture web-form figure is NOT a server constraint** (18/60 places exceed 20; max 70 unique in hi-tek). Package **size** limit remains unresolved (flagged).
- [x] Draft a world-specific validation rules profile, kept separate from `validator.js`'s Mall Item rules (documented, **not enforced**)
- [ ] Document the current CTR (Cybertown Revival) world-submission workflow as it actually exists today — *open (needs operator/process input)*
- [ ] Inspect Scott99's `worlduploader`/`itemuploader` tools (https://www.3dgrove.com) as historical workflow references only — no code or asset copying without established licensing/permission — *open*

**Prerequisites:** access to real-world sample files and/or documentation of the current submission process; Phase 1 complete.

**Risks:**
- Acting on assumed limits (e.g., encoding "20 textures" as a hard rule) would produce false validation failures for legitimate worlds.
- Reverse-engineering Scott99's tools beyond "workflow reference" risks license/IP issues — treat as read-only research, not a source to copy from.

**Completion criteria:**
- A documented (not yet enforced) world validation rule set, with each rule traceable to an actual confirmed constraint rather than an assumption
- Open questions about real server limits explicitly flagged as unresolved, not silently guessed

## Phase 4 — World Asset Resolver 🔄

**Phase 4A (production resolver + read-only workspace) landed** — see
`docs/WORLD_PROJECT_ARCHITECTURE.md`. The Phase 3A recon logic was promoted into
`src/world-project/` (single source of truth; `qa/world-recon/*` re-export it),
wired behind confined read-only `world:*` IPC, and rendered in a dedicated
`renderer/world.html` workspace (summary, filterable asset table, dependency
view). The embedded world preview followed in **Phase 4B** (below); still no
packaging/upload.

- [x] Open a project folder (not just a single file), with primary-file
  detection (ambiguity surfaced, never guessed) — plus direct primary-file open
- [x] Parse local URL references across the primary world and nested
  `Inline` assets (gzip + plain, bounded + cycle-safe)
- [x] Discover **EXTERNPROTO** dependencies (added in **WD1.7-B2** — Phase 4A/5A
  shipped without them; see the note below)
- [x] Discover textures and nested local assets, however many — **no** arbitrary
  20-texture limit (fixtures + tests cover 24 and 70 unique textures)
- [x] Missing-file, filename-case-mismatch, absolute/traversal (unsafe), remote,
  duplicate, and dependency-cycle diagnostics
- [x] Read-only workspace UI (summary, filters, dependency view) sharing the one
  window; Mall Item lane unchanged; profile kept separate from `validator.js`
- [x] Non-mutation verified (fixtures byte-identical before/after scanning) and
  one controlled `VisualQaRunner` visual run of the workspace states

**Prerequisites:** Phase 3's world rules profile (draft) — met.

**Risks (addressed):**
- Recursive/inline asset graphs could be large or cyclic — bounded traversal
  (`maxWrlNodes`/`maxDepth`) + visited-set cycle safety; cycles are reported.
- Case-mismatch detection is done in code (not leaning on the local fs), so it
  catches a hazard that a case-insensitive dev machine would mask.

**Completion criteria:** met — the resolver enumerates all referenced local
assets in a real multi-texture project (`test/fixtures/world/mini`, 25 textures)
and flags deliberately-broken references (`test/fixtures/world/broken`: missing,
case mismatch, unsafe, remote).

**EXTERNPROTO note (corrected in WD1.7-B2).** As shipped, Phase 4A/5A discovery
was anchored on url-**named** fields, and an EXTERNPROTO URL list has no field
name — so external prototype libraries were never in the asset graph, and a
bundle could omit all of them and still report `ready`
(`F3-WORLD-PROJECT-SCANNER-EXTERNPROTO-OMISSION`, recorded in
`docs/white-dune-2026/WD1_7_A_EXTERNAL_PROTO_EVIDENCE.md` §19). **WD1.7-B2**
closed it by discovering declarations from the AST and retrieving candidates
through the WD1.7-B substrate; see
`docs/white-dune-2026/WD1_7_B2_WORLD_PROJECT_INTEGRATION.md`. What that lane
deliberately does **not** prove — that a retrieved artifact contains the named
PROTO (ISO 4.9.3), and what the artifact's own dependencies are — is **WD1.7-C**,
now built (`docs/white-dune-2026/WD1_7_C_TARGET_SELECTION.md`): ISO 4.9.3
fragment / first-PROTO-excluding-EXTERNPROTO selection, the ISO 4.5.2 ordered
candidate walk that stops on `RESOLVED` rather than on `RETRIEVED`, ISO 4.5.3
base propagation through prototype instantiation, and a dependency graph with
cycle detection on `(decodedContentHash, selectedProtoName)`.

The graph **says when it is not exhaustive**. WD1.5-P2A does not index node
occurrences inside PROTO interface *default* values, and C creates no second
resolver to cover them — so when the traversal reaches such a region it reports
the region and returns `complete: false` rather than a silently partial graph.
The same applies to a type binding P2A withheld without proving the name
undeclared. Every reason is enumerated in `INCOMPLETENESS_REASON` with its own
evidence.

**WD1.7-D** then consumes C's proof to add the two external semantic facts WD1.6
could not establish locally
(`docs/white-dune-2026/WD1_7_D_INTERFACE_CLASS_ENRICHMENT.md`): the **ISO 4.9.2**
directional interface check (`local ⊆ target`, so a target superset is
conforming; a missing member and a declared-type difference are the clause's two
errors, while an access-category difference is ISO-*silent* and is reported as
its own observation, never as non-conformance), and an **externally proven ISO
4.8.3 implementation class** — including the case where the target's own first
body node is an EXTERNPROTO, which is followed through C's already-proven edges
rather than through a second resolver. `containment.js`'s 4.8.3 derivation was
**extracted, not duplicated**, so the local class and the externally proven one
cannot drift.

D **enriches; it never mutates.** No WD1.6 query gained an evidence, context or
resolver parameter, `childLegality` is unchanged and independently observable,
and the reserved `compatibility` slot stays `null` for every WD1.7-D record —
permanently, on current evidence, and by the owner-ratified decision below.

**WD1.7-E** was chartered to classify D's output and, on the evidence, cannot:
**WD1.7-E0** opened blaxxun interactive's own shipped authoring documentation
(`docs/white-dune-2026/WD1_7_E_COMPATIBILITY_POLICY_DECISION.md`, ratified
2026-08-29) and found that the one documented rule speaking directly to external
prototype interfaces covers the **URN** reference form — which WRL Forge
deliberately never retrieves, so C never selects one and D never reports on one.
Nothing in the documentation addresses a `.wrl` file target, and corpus
prevalence is not evidence. `ACCESS_DIFFERS`, `MEMBER_MISSING` and
`TYPE_MISMATCH` therefore stay `null`, which is the **correct terminal answer**,
not a deferral.

**WD1.7-E1** is the rescoped lane that resulted
(`docs/white-dune-2026/WD1_7_E1_BLAXXUN_CONTACT_PROFILE.md`): one earned public
profile, **`blaxxun-contact`** (blaxxun Contact 3D 4.x–5.x), a closed five-entry
behaviour registry with per-claim evidence tiers, and the `EXTRA_STANDARD` /
`TOLERATED_VIOLATION` split that a remediation decision turns on. Compatibility
is a **sibling projection**: `src/vrml/compatibility.js` supplies only the opaque
slot value, `semantic-findings.js` re-emits every strict field verbatim through
its one constructor, and no strict fact can change. **One** of the five documented
behaviours has a structured observation that exactly represents it (`exposedField`
in a `Script` interface); the other four stay **registry-only**, with their
evidence recorded and no finding invented to display them. Every other candidate
profile name — `Blaxxun/GLView`, `glview`, bare `blaxxun`, `cybertown-compat`,
`legacy-vrml` — is **retired**; `blaxxun-3d` is deferred. There is no runtime
detection, no presentation policy and no boolean. Like every predecessor, E1 is
**consumer-free** and publishes nothing on `src/vrml/index.js`.

**P4-A** turns all of that into the one thing a UI actually needs
(`docs/white-dune-2026/P4_A_PRESENTATION_POLICY.md`): a single deterministic
**presentation projection** over the semantic evidence, so WD2 never invents its
own severity, ordering, visibility or save policy. `src/vrml/presentation.js` is
pure, browser-safe and published on the facade as `vrml.presentation`.

Five decisions are load-bearing, each with a live mutation control:
**compatibility never downgrades a strict severity** — an ISO-prohibited
construct that blaxxun Contact tolerates is still an error with a `tolerated`,
`portable: false` annotation beside it; **severity and confidence are
orthogonal** — where the ISO axis asserts a normative claim, confidence is not
consulted at all, so a `recovered` violation and its proven twin present
identically; **recovered findings stay visible** — 86.96% of the corpus carries
that confidence, and it earns its own filter tag and a lower attention rank, not
suppression; **no semantic finding ever blocks an ordinary Save**
(`saveBlocking` is a frozen `false` everywhere, with no parameter that changes
it); and **"could not determine" is never presented as "known invalid"** —
`unsupported`, `ambiguous` and a `withheld` agreement rollup are warnings, not
proven errors.

The presentation is a **sibling projection**: the finding is carried by identity
and frozen, and P4 can neither construct nor re-emit one. A complete matrix guard
enumerates every exported semantic vocabulary — finding codes, ISO results,
statuses, agreement codes/bases/statuses and compatibility classifications — and
fails when a new value arrives without an adjudication, because every table is
total, tight and read through one throwing accessor with no `default:` anywhere.
Message text is deferred to **P4-B**, and export/package gating is deliberately
left with the profile authorities that already own it
(`src/world-project/package-plan.js`, `validator.js`).

C and D are a **general resolver and a general enrichment layer, not World
Project features**, and nothing in the World Project lane consumes either yet. B2's rule is unchanged — a bundle still
packages *every locally retrievable fallback artifact*, because a viewer may
reach for one C did not select. Recursive discovery of assets referenced only
inside a **selected** external library remains deferred
(`DEFERRED_TO_POST_C_WORLD_PROJECT_CONSUMER`): it changes what *blocks* a bundle,

**P4-B** is the message text P4-A deferred
(`docs/white-dune-2026/P4_B_DIAGNOSTIC_MESSAGE_CATALOG.md`). It is a
**text projection** over the P4-A presentation, not a second policy layer:
`src/vrml/messages.js` is pure, browser-safe, returns plain structured
strings and never carries severity, group, saveBlocking or ordering.
Three catalog tables (`SEMANTIC_TEMPLATES`, `AGREEMENT_FINDING_TEMPLATES`,
`ROLLUP_TEMPLATES`) cover every `FINDING_CODE`, every `AGREEMENT_FINDING`,
every `AGREEMENT_STATUS`, and every currently producible
`(code, reason)` combination. A complete matrix guard enumerates every
cell of the `code × iso × confidence` and `agreement code × basis` cross
products and fails when a new value arrives without an adjudication.
Strict wording is direct (no "may be", "perhaps", "possibly"); uncertain
wording uses "WRLForge could not determine"; recovered confidence keeps
the proven twin's title and summary and may add a single detail sentence;
compatibility (TOLERATED_VIOLATION) preserves both facts — "not VRML97-
conforming" and "blaxxun-contact is documented to accept this behavior" —
and never hides the strict violation; ACCESS_DIFFERS names ISO 4.9.2
silence without claiming a violation; agreement rollups use rollup
wording that does not look like a per-member error. WD2 renders the text;
P4-B never decides.
which is a packaging-policy decision and needs its own approved lane.

### Phase 4B — World Preview ✅

The embedded X_ITE **world** preview landed — it renders a complete world,
honouring the gzip/nested-Inline asset graph the Phase 4A resolver produces. See
`docs/WORLD_PROJECT_ARCHITECTURE.md` and `docs/PREVIEW_ARCHITECTURE.md`.

- [x] Read-only preview loaded from decompressed text (`world:previewLoad`),
  taking **no** renderer-supplied path — main owns every project path.
- [x] Controlled local dependency resolution: X_ITE resolves nested Inline /
  textures through a privileged, standard, LOCAL-only `wrlworld://` scheme whose
  handler serves **only** asset-graph-authorized files (readable WRL nodes +
  present exact-case assets), gzip-decompressed, confined to the project root.
  Each nested WRL resolves relative URLs from its **own** directory.
- [x] Plain/gzip primary **and** plain/gzip nested Inline; >20 and ≥70 textures
  with no truncation; repeated deps; bounded dependency cycles; per-file bases;
  filenames with spaces.
- [x] Viewpoint discovery + selection (including viewpoints authored inside
  nested Inlines, via `EnableInlineViewpoints`), Reset View, navigation modes,
  loading / warning / stale / failure states, loaded-vs-missing counts, and an
  explicit **Refresh Preview**. Temporary parse error keeps the last valid scene
  (flagged stale).
- [x] Missing / case-mismatch / remote / unsafe references surfaced but never
  loaded; inline scripts never executed (CSP blocks eval). No project mutation
  (fixtures byte-identical before/after; parse-fail/recover writes only a scratch
  project under the OS temp dir). No Mall Item rules applied in World mode.
- [x] One serialized `VisualQaRunner` run of all 10 states
  (`qa/phase-4b-world-preview/`): one launch, graceful exit, no leak.

**Explicitly not implemented in this lane** (unchanged from Phase 4A): asset
repair, file copy/rename/delete, packaging, direct upload, Apply/Bake transforms,
Windows packaging. World preview is **analysis + display only**; it never marks a
project upload-ready.

### Phase 5A — World Project Packaging Audit + World Project Bundle ✅

A narrow packaging slice for the World Project lane (the world half of Phase 6),
kept read-only except for one explicit user action. See
`docs/WORLD_PROJECT_ARCHITECTURE.md` (“Packaging”) and
`docs/WORLD_PACKAGE_QUESTIONS.md`.

- [x] **Package Audit** (read-only): a deterministic package plan from the
  production asset graph — packaged file set (primary + nested local WRL + present
  approved assets) with project-relative path, asset type, byte size, content hash
  (sha256), referencing WRL files, and dependency depth; totals (files / bytes /
  WRL count / unique textures); findings (missing / case / unsafe / remote /
  cycles / repeated); and **unused** files under the root (reported, never
  auto-included). Repeated references packaged once; plain + gzip WRL; >20 and ≥70
  textures.
- [x] **Build World Project Bundle** (explicit action, requires a destination): a
  **deterministic ZIP** (Node `zlib` only — **no** third-party archive
  dependency; fixed 1980 timestamps) containing `project/<relpath>` (byte-for-byte,
  structure preserved), `MANIFEST.json`, `REPORT.md`, and a
  `READ-ME-FIRST.txt`, all labelled **“WRL Forge World Project Bundle”** — a review +
  manual hand-off package (uploaded by hand through the Cybertown website; not a
  server-certified upload format).
- [x] **Blocking rules**: missing / case-mismatch / absolute / traversal / remote
  / unreadable references block packaging; cycles are reported but do **not** block
  (local + bounded). Build refuses a blocked project, an in-project destination,
  and overwriting an existing file; it re-hashes every file against the manifest.
- [x] **Non-mutation** verified (source byte-identical across audit + real build;
  bundle contents/hashes match the manifest; deterministic output). One serialized
  `VisualQaRunner` run of all packaging states (`qa/phase-5a-world-packaging/`).
- [x] Open questions for a **true** upload-ready packager (archive/layout format,
  size cap, asset-count limits, allowed types, naming/case, primary-world naming,
  auth + submission workflow) documented in `docs/WORLD_PACKAGE_QUESTIONS.md`.

**Explicitly not implemented:** direct upload, authentication, automatic repairs,
path rewriting, renaming, asset deletion, source mutation, Apply/Bake transforms,
internal editing, Windows packaging. Packaging is **analysis + a review bundle
only**; it never uploads and claims no current-server compatibility.

## Phase 5 — Embedded X_ITE Preview ✅

Subsumed by **Phase 2B** (Mall embedded X_ITE preview, including 2A spike + 2B0
remediation) and **Phase 4B** (World embedded X_ITE preview). No remaining
unimplemented work in 7D scope.

- X_ITE is the only embedded rendering engine — no custom VRML/X3D renderer exists
  (`package.json` `dependencies` = `x_ite` only; per-product rule).
- Preview surfaces use `contextIsolation: true` / `nodeIntegration: false`; every
  renderer carries a strict CSP with no remote origin; remote URLs are blocked at
  the network layer (`session.webRequest`); read-only IPC is confined to
  `preview:load` / `world:previewLoad`; the `wrlworld:` scheme is asset-graph
  allow-listed and project-root-confined.
- VSCodium is preserved as an explicit optional action (`src/editor/mall-edit-flow.js`,
  passive launch removed in Phase 7B1); the live launch closeout is **Phase 6B1**
  (13/13 on real Win11).
- Both contexts render real Cybertown content on Linux + Win11:
  `qa/phase-7c-mall-preview/` 18/18 and `qa/phase-7c-world-preview/` 22/22
  on both platforms.

Phase 7D0's audit (`docs/PHASE_7D_BASELINE.md` §3) classifies this as
`ALREADY_SATISFIED_NEEDS_ROADMAP_RECONCILIATION`.

## Phase 6 — Packaging ⏳

The **World Project** half landed early as **Phase 5A** above (deterministic
package audit + review bundle, `docs/WORLD_PROJECT_ARCHITECTURE.md` “Packaging”).
Remaining for this phase: the Mall Item equivalent, and — gated on
`docs/WORLD_PACKAGE_QUESTIONS.md` being answered — a *true* upload-ready world
package format (the Phase 5A review bundle is explicitly **not** confirmed for
direct upload).

- Deterministic Mall Item package output
- Deterministic World Project package output *(Phase 5A: review-bundle form done; server-format-confirmed form pending the open questions)*
- Human-readable validation report (shared format across profiles where it makes sense)
- **No direct upload** — packaging produces a file/folder ready for a human to submit, it does not submit anything itself

**Prerequisites:** Phases 1–5 substantially complete, since packaging depends on validation and asset resolution being trustworthy.

**Risks:**
- "Deterministic" needs to mean actually reproducible (same input → byte-identical or at least content-identical output), which constrains things like timestamp embedding in package metadata.

**Completion criteria:**
- Re-running packaging on unchanged input produces an equivalent package
- Validation report clearly states pass/fail per rule with enough detail to act on
- No network calls are made by the packaging step

### Phase 6A — Windows Compatibility Recon + First Private Build ✅

Prepared and validated the existing app for Windows **without** changing Linux
behavior or adding product features. See `docs/PLATFORM_NOTES.md`,
`docs/BUILD.md`, and `qa/phase-6a-windows/`.

- [x] Audited Linux-only assumptions; the only in-code one was `spawn('codium')`.
  Made editor discovery cross-platform (`src/editor/editor-locator.js`): Linux
  `codium`/`code`, Windows install-location search + PATH shims, a configurable
  override (`WRL_FORGE_EDITOR` / `settings.json` `editorCommand`), and a clear
  "editor not found" message. Spaces/non-ASCII-safe launch args.
- [x] Hardened case-mismatch detection for **case-insensitive** filesystems: the
  directory listing (not `existsSync`) is now authoritative on every platform, so
  Windows/macOS catch an authored `Stone.PNG`→`stone.png` hazard instead of
  masking it. Explicit code-based test + verified on real NTFS.
- [x] Verified paths (drive letters/backslashes), gzip open, project scanning
  (nested + gzip, >20 and 70 textures), World Project Bundle ZIP + integrity, window-state
  / userData migration, and spaces/non-ASCII paths on **real Windows 11** via a
  packaged-runtime self-test (31/31).
- [x] Produced a **private, unsigned** Windows test build with **electron-builder**
  (MIT): portable `.exe` + NSIS installer, neutral placeholder icon, labelled
  **Private Test Build — Unsigned**. `electron` moved to `devDependencies`.
- [x] Verified the packaged app **launches** on Windows 11 (Mall + World lanes,
  correct branding, clean exit) and kept Linux fully green (234 unit tests + Mall/
  World-preview/packaging visual regressions).

**Explicitly not implemented** (unchanged exclusions): direct upload, auth,
upload-ready CTR packaging, auto-update, code signing, public releases, Microsoft
Store, asset repair, Apply/Bake, internal editor, and any new world/Mall features.
A broader Windows **beta** (signing, SmartScreen, live VSCodium launch, dialog-
driven flows, arm64) is a separate future lane.

### Phase 6B — Windows Beta Hardening ✅

Turned the Phase 6A private test build into a **beta candidate** (`1.1.0-beta.1`,
labelled **Private Beta — Unsigned**) by validating the real GUI workflows on
Windows 11. **No new product features** — hardening + validation only. See
`qa/phase-6b-windows/RESULTS.md`, `docs/SIGNING_READINESS.md`, and
`docs/BETA_RELEASE_NOTES.md`.

- [x] Fresh versioned x64 beta artifacts (portable + NSIS + `SHA256SUMS`), rebuilt
  with `CSC_IDENTITY_AUTO_DISCOVERY=false` and **verified genuinely unsigned** (PE
  cert table empty). Kept private + git-ignored; **no public GitHub Release**.
- [x] **Committed** packaged-runtime self-test (`win-selftest.js`, unlike 6A) —
  **37/37 on real Windows 11** (Electron 41.7.1 / Node 24.15): gzip, nested + gzip
  scan, 25 & 71-texture worlds, viewpoints, missing/remote/unsafe/case diagnostics,
  Package Audit, World Project Bundle + ZIP hash integrity + in-project/overwrite refusals,
  spaces + non-ASCII paths, **all editor-override cases incl. the invalid-override
  fall-through**, `.edit.wrl` generation, window-state/userData paths. Also green on
  Linux (CI-verifiable before the VM).
- [x] **Live GUI focused pass** on the beta build: portable + NSIS-installed launch,
  native file/folder dialogs, **X_ITE Mall Original/Fit preview renders on Windows**
  (the Phase 6A gap), gzip open, **X_ITE World preview + viewpoints render**,
  Package Audit + **World Project Bundle written outside the project (6/6 manifest hashes
  match)**, window-state persistence across portable↔installed (shared
  `%APPDATA%\wrl-forge`), clean exit, Start-menu launch, and **uninstall** (app +
  shortcuts removed, user projects untouched). Non-mutation verified (fixtures
  byte-identical).
- [x] **Signing-readiness audit** (`docs/SIGNING_READINESS.md`): cert format
  (OV/EV → token/KMS, not bare `.pfx`), env/config points (`CSC_*` / `win.sign` /
  `signtoolOptions`), RFC-3161 timestamping, CI implications (needs a Windows/cloud
  signing stage; signed artifacts aren't byte-reproducible → re-checksum after
  signing), secret handling. **No certificate purchased/generated/used.**
- [x] Linux stays fully green: 234 unit tests, syntax gate, and the Mall/World-
  preview/World-packaging visual regressions (20 pass) run **serially through the
  sanctioned harness**, no leaks (launch-storm guardrail preserved).

**Verdict (Phase 6B):** **CONDITIONAL GO** for a limited private Windows beta,
gated on one live check — a live VSCodium "Open in Editor" run (VSCodium was not
installed in the VM at 6B; the clear not-found path was verified instead) — plus
the standing scope facts that artifacts stay **Unsigned** (no SmartScreen-
elimination claim) and the World Project Bundle is **not** confirmed for direct Cybertown
upload.

### Phase 6B1 — VSCodium live-launch closeout ✅

Closed the one remaining Phase 6B condition. VSCodium **1.126.04524 (x64)** was
installed in the WinBoat Windows 11 VM via its official user-setup installer and
the **production editor path was driven end-to-end, 13/13**
(`qa/phase-6b1-vscodium/RESULTS.md` + `win-editor-verify.js` under the packaged
Electron-as-node): automatic install-location **discovery**; plain **and** gzip
`.wrl` → `.edit.wrl`; a genuine `buildLaunch`+`spawn` of VSCodium on an `.edit.wrl`
whose path has a **space and a non-ASCII character**; `editorCommand` **and**
`WRL_FORGE_EDITOR` overrides honored; **invalid override falls back** to discovery;
**single** editor instance (no launch loop); **clean exit** (no survivors); and
**source fixtures unmutated**. A live VSCodium window open on the `.edit.wrl` is
captured in `qa/phase-6b1-vscodium/screenshots/`. No product code changed — this is
verification + docs only. Linux stayed green (`npm test` / `npm run check`).

**Verdict (Phase 6B1):** **GO** for limited private Windows beta distribution. The
last live condition is closed; the only remaining items are the deliberate,
documented scope constraints of a private beta — artifacts remain **Unsigned**
(SmartScreen warning expected, **not** eliminated), **x64 only** (no Windows
ARM64), and the World Project Bundle is **not** confirmed for direct Cybertown (CTR)
upload. Public release, signing, direct upload, and auto-update remain out of scope.

**Explicitly not implemented** (unchanged exclusions): direct upload, auth,
upload-ready CTR packaging, auto-update, code signing (only readiness documented),
public releases, Microsoft Store, Apply/Bake, **Windows ARM64**, any new Mall/World
features beyond the Phase 7 editor/parser foundation below.

## Product Direction (locked, 2026-07-12) 🔒

Set after the Phase 6B1 closeout, before broader beta distribution:

1. **No direct uploads to Cybertown.** WRL Forge will not add authentication,
   networking, or submission code. Users upload through the Cybertown Mall or the
   existing Cybertown **website** workflow, by hand.
2. **No prototype / test-build / "unavailable feature" copy** in the user-facing
   application. Do not advertise absent features or ship disabled buttons for
   features that don't exist. Truthful runtime states (missing file, parse error,
   blocked remote URL, case mismatch, unsaved changes, editor-not-found,
   conservative bounds, unsupported syntax) stay. Automated tests, visual QA,
   safety guardrails, and honest errors are **kept**, never removed.
3. **VSCodium is an OPTIONAL external editor**, not a requirement. The integration
   (Linux + Windows, verified in Phase 6B/6B1) is preserved; the "editor not found"
   message appears only when the user requests the external-editor action. WRL
   Forge must eventually function without any external editor — see Phase 7.
4. The World review bundle is now labelled **"WRL Forge World Project Bundle"** — a
   review + manual hand-off package, not a server-certified upload format.
5. A **native WRL editor and a real VRML97 parser** are now a **beta requirement**
   (Phase 7), so the app stands on its own without VSCodium.

## Phase 7 — Native Editor and VRML97 Parser Foundation ⏳

Delivers the last hard beta requirement: a native editing experience backed by a
real tokenizer/structural parser, with the external editor demoted to optional.
X_ITE remains the ONLY renderer — **do not build a renderer**. See
`docs/NATIVE_EDITOR_ARCHITECTURE.md` for the full design; the parser produces a
reusable syntax tree feeding diagnostics, scene outline, asset-reference discovery,
`DEF`/`USE` validation, navigation, future formatting, safe targeted edits, and the
Mall/World validation profiles.

**Status:** Phase 7A (parser) and Phase 7B (native editor) have **shipped** and are
in production (see the ✅ sub-sections below). Within Phase 7C, planning (**7C0**) is
complete, the **7C4** Windows-native QA harness is **built**, **7C4.1** (Windows
Workspace Isolation Guard) is **built**, **Feature A (Vision Accommodations)** is
**built**, **7C1** (the pure buffer-overlay foundation) is **built**, **7C2** (the
Mall unsaved-buffer live preview) is **built**, and **7C3** (the World unsaved-buffer
live preview — primary + nested overrides, viewpoint preservation, Find new files) is
**built** and shipped in the native editor; and **7C5** (cross-platform acceptance +
private beta refresh) is **complete** — the full 7C feature set is accepted on both
Linux and native Windows 11 (local NTFS), with the refreshed private unsigned Windows
x64 beta `1.3.0-beta.1`. Phase 7D (beta polish) remains a **plan**. Anything still
plan-only ships no code without separate approval.

### Phase 7A — Parser Foundation ✅
**Shipped (parser-only lane).** A dependency-free, token-driven VRML97 tokenizer +
structural parser under `src/vrml/` (`tokenizer`, `parser`, `ast`, `diagnostics`,
`analyze`, `asset-refs`, `index`), producing a profile-neutral partial syntax tree
with exact source spans, stable diagnostic codes, bounded error recovery, and
explicit depth/node safety limits. Includes a semantic index (`DEF`/`USE`/`ROUTE`)
and a read-only AST asset-reference extractor validated for **parity** against the
production World Project scanner. Fixture corpus + `node:test` coverage under
`test/fixtures/vrml/` and `test/vrml/`; parser files wired into the `check` gate.
**No editing UI**, and **no change** to `validator.js` / World Project scanning /
Mall Fit / X_ITE preview / packaging / VSCodium / UI / save. See
`docs/VRML_PARSER.md` for grammar coverage, AST shape, diagnostic model, recovery,
safety limits, parity status, known limitations, and the Phase 7B integration
boundary. Design in `docs/NATIVE_EDITOR_ARCHITECTURE.md` (§ "Phase 7A scope").

**Phase 7A1 — Corpus Compatibility Corrections ✅.** Independent real-corpus QA
(Gemini, 2,124 Cybertown files) gave 7A a CONDITIONAL GO. Fixed three rejected
valid VRML97 forms — internal `-`/`+` in identifiers, multiline (LF/CRLF/CR)
strings incl. inline Script source, and case-sensitive header encoding — plus
lenient acceptance of the pervasive Cybertown/Blaxxun `ROUTE`/`PROTO`-inside-MFNode-
array pattern. Read-only corpus re-audit: diagnostics **−98.1%** (926,063 →
17,201), clean parses 961 → 1,745 of 2,124; remaining bulk is the documented
flat-scope duplicate-`DEF` limitation (NOT fixed — no PROTO-scope rewrite in this
lane). Parser-only; no production system changed. See `docs/VRML_PARSER.md`.

### Phase 7B — Native Editor ✅
Shipped a first-class native WRL editor so WRL Forge edits and safely saves plain
**and** gzip `.wrl` without any external editor. Built on **CodeMirror 6** (MIT,
local `@codemirror/*` + `@lezer/highlight`, bundled by esbuild → `renderer/vendor/`,
**no CDN**, all **devDependencies**; runtime deps stay `x_ite`-only). The existing
Phase 7A tokenizer/parser is the **sole** language authority — highlighting,
diagnostics, and the outline all derive from it; there is no second grammar.

Delivered: a new Editor workspace (`renderer/editor.html`/`editor.js`) reachable
from both lanes (Mall "Open in Native Editor" edits the real `.wrl` gzip-
transparently — no `.edit.wrl`; World "Open Primary WRL in Native Editor" plus a
per-dependency "Edit" that main authorizes against the scan graph). Line numbers,
undo/redo, search & replace, bracket matching, active-line highlight, VRML97
syntax highlighting, **authoritative** syntax diagnostics (click-to-navigate,
capped with a retained total) kept **separate** from a clearly-labelled
**non-authoritative** advisories panel (flat-scope VRML040–044; never blocks
saving), an AST outline (click-to-navigate), dirty tracking, cursor Ln/Col, a
conservative **safe save** (encode → conflict-guard → temp+fsync → verify-decode
→ timestamped backup → atomic rename → verify), external-change detection with a
**Reload / Save As / Cancel** dialog, Save As, Reload, Go-to-line, session
restore (confined to the previously-authorized context), optional "Open in
External Editor", and **four themes** (Dark/Light/Terminal/Tokyo Night, contrast-
checked, persisted). Security preserved: `contextIsolation:true`,
`nodeIntegration:false`, the narrow `window.vrmlpad.editor` bridge, and
**main-process path ownership** (the renderer sends text + intent + an opaque
sessionId, never a write path; Save As targets only a main-owned dialog path).

Verification: **382** non-visual tests; serialized **Linux visual QA 15/15**
(one Electron process via `VisualQaRunner`); a pure-Node **perf gate** (analyze()
< the 250 ms debounce across small/world/327 KB/1.3 MB/script-heavy/many-errors);
private unsigned **Windows x64** build with the editor packed in `app.asar` and 6
editor cases added to the Windows selftest (Linux-green; NTFS run is the WinBoat
step). See `docs/NATIVE_EDITOR_ARCHITECTURE.md` and
`qa/phase-7b-native-editor/RESULTS.md`. **Excludes** (Phase 7C): unsaved-buffer
X_ITE preview, live per-keystroke rendering, AST rewriting, formatting, scope-
aware PROTO analysis.

### Phase 7B1 — Native Editor Closeout ✅ (in progress)
Closes the issues from the independent Phase 7B review (Gemini, CONDITIONAL GO).
Removed the **passive external-editor launch**: opening a Mall `.wrl` no longer
launches VSCodium and never surfaces an "editor not found" message — the external
editor starts **only** through the explicit "Open in External Editor" action
(`src/editor/mall-edit-flow.js` extracts the passive-open vs explicit-launch logic
so it is unit-tested without Electron). Native editing still edits the real source
and never creates a `.edit.wrl`; the explicit external action ensures/refreshes the
`.edit.wrl` working copy. Corrected stale parser/editor documentation across the
agent guides and `docs/` (parser is wired into the native editor but has **not**
replaced the Mall validator / World scanner / preview resolver / packaging;
buffer-driven preview is Phase 7C, not 7B). Expanded `test/product-posture.test.js`
to scan `renderer/editor.html` + `renderer/editor.js` and guard native-editor
wording (no "planned", no unsaved-buffer-preview claim, no user-facing "Review
Bundle" label). The externally-triggered `world:buildReviewBundle` IPC channel and
its `buildReviewBundle` bridge/handler names are **intentionally retained** for
stability (internal, not user-facing). Real Windows 11 native-editor GUI
verification runs in this lane (WinBoat).

### Phase 7C — Editor + Preview Integration
Preview refresh from the unsaved editor buffer, debounced parsing, last-valid-scene
behavior, Mall Item and World Project contexts, reload/conflict handling, and the
safe save + backup workflow end-to-end. Being delivered one lane at a time, with a
stop-and-report gate between lanes.

**Planning — 7C0 ✅ complete.** `docs/PHASE_7C_PROPOSAL.md` (unsaved-buffer X_ITE
preview: buffer-overlay model, generation/stale model, last-valid state machine,
parser/X_ITE policy, security + threat model, collapsible editor split-view) and
`docs/WINDOWS_NATIVE_QA_PLAN.md` (Windows-native agent QA workflow, packaged-app
automation, evidence format, shared 7C0–7C5 slices).

**7C4 — Windows-native QA harness ✅ built.** `qa/phase-7c-windows/` +
`qa/visual-qa/` cross-platform (`qa:windows`); Tier 1 packed self-test + Tier 2
`VisualQaRunner` + evidence with a fixture-mutation NO-GO gate.

**7C4.1 — Windows Workspace Isolation Guard ✅ built.** `qa/visual-qa/workspace-guard.js`
refuses UNC / mapped-network-drive / host-share workspaces on Windows (the WinBoat
`\\<host-share>\Data` share that broke `node_modules`), wired into `qa:windows`,
`qa:visual`, the Windows self-test, and the Windows build scripts; plus an
evidence-export allowlist (share is export-only, never node_modules/.git/source/
fixtures/backups/binaries). Linux paths are never blocked. See
`docs/WINDOWS_NATIVE_QA_PLAN.md` §"Workspace isolation".

**Feature A — Vision Accommodations ✅ built.** For low-vision users: one coherent
zoom level (Ctrl `+`/`-`/`0`, persisted) that scales **both** the CodeMirror code
area (a font compartment in `src/editor/browser/editor-view.js`) **and** the app
chrome (a `--wrl-ui-scale` rem layer in `renderer/editor.html`); a fifth **High
Contrast** theme; a toolbar zoom group. Pure zoom model in `src/editor/ui-state.js`;
visual QA `qa/phase-7c-vision/` (`qa:vision`). No main/preload/IPC/CSP change.

**7C1 — Buffer-overlay foundation ✅ built.** The pure, main-process-ready model for
previewing an unsaved buffer without writing a temp file — **no UI, no X_ITE, no CSP
or scheme change, nothing wired into a preview page yet.** Three dependency-free
modules under `src/preview/`: `buffer-overlay.js` (a session-scoped registry that
performs **byte substitution only** — it never authorizes a path, expands a graph,
resolves a renderer path, fetches, writes, or mutates a source; registration requires
an authorization **proof** the owning controller already obtained from the Mall
session / World scan graph, the narrow integration boundary), `preview-state.js` (the
pure last-valid-scene state machine — a failed newer render keeps the last good scene;
an older result never overrides a newer one), and `preview-scheduler.js` (a
clock-injected 700 ms debounce / coalescing coordinator, no real timer). Ordering is
by monotonic integers (`bufferVersion` per edit, `generation` per attempt) — never
timestamps. Size bands: auto-refresh ≤ **1 MiB**, manual Update above that, hard
refusal above **8 MiB** (refused, never truncated). 46 pure tests in
`test/preview/buffer-overlay.test.js`; wired into the `check` gate. See
`docs/PHASE_7C_PROPOSAL.md` §4–§10 and `docs/PREVIEW_ARCHITECTURE.md`.

**7C2 — Mall unsaved-buffer live preview ✅ built.** A split-view X_ITE preview of the
in-memory Mall editor buffer with **no temp file**. The renderer sends only
`{sessionId, text, bufferVersion}`; `src/preview/mall-preview-bridge.js` (pure/injectable,
`node:test`-able) resolves the editor session, confirms the held source **equals the
active authorized Mall item**, builds the `mallAuthorization` proof from *that* path, and
byte-substitutes the buffer through the 7C1 overlay (`editor:previewLoad`/`previewSaved`/
`previewAccept`/`previewClose` IPC). `renderer/editor-preview.js` + `editor.html`'s
`.preview-col` + draggable divider are the split-view (layout mode + split fraction
persisted; `Ctrl+Enter` Update, `Ctrl+Shift+Enter` maximize), **reusing `renderer/
preview.js` verbatim** (Original/Cybertown-Fit/guides/fit-report, remote-URL block) via an
injected source loader. Transport is direct string-swap with a `file://` base URL — **no
new scheme, no CSP origin** beyond the Mall X_ITE superset now on `editor.html`. Release
copy via `ui-state.js` `previewStatusModel` (Live / Updating… / Outdated / Showing last
good version / Showing saved version / Some parts missing / large-file / too-large). Auto
≤ 1 MiB (700 ms debounce, coalesced), manual Update 1–8 MiB, refused > 8 MiB. Last-valid
scene survives a temporary syntax error; older generations never replace newer; overlay +
generation counts are **0** after close (QA leak assertion). Nonvisual tests:
`test/preview/mall-preview-bridge.test.js` + `ui-state` preview models. Visual QA
`qa/phase-7c-mall-preview/` (`qa:mall-preview`): 18/18, 1 reused Electron process, 0
survivors, leak-clean; perf/stress `stress.js` (100 edits → 1 render). See
`docs/PREVIEW_ARCHITECTURE.md` §"Phase 7C2".

**7C3 — World unsaved-buffer live preview ✅ built.** The same split view previews an
unsaved **World** document — the primary or any authorized nested WRL — inside the
**full world scene**, no temp file, no new scheme. `src/preview/world-preview-bridge.js`
(pure/injectable, `node:test`-able) authorizes the held document against the **current
scan graph** (root match, graph membership, exact-case, realpath re-check), builds the
`worldAuthorization` proof, and installs the `wrlworld://` serving context; the shared
`editor:preview*` IPC routes by document context, and `editor:previewRescan` is the
explicit **Find new files** normal rescan (unsaved text never expands authorization —
new/missing/case/remote/unsafe buffer references are classified and surfaced only).
The unsaved **primary** is a string-swap with the primary's `wrlworld://` base; an
unsaved **nested** WRL substitutes inside `resolveWorldRequest` via an injectable
`overlayLookup` consulted only after root confinement + the allow-list (absent by
default → the workspace disk preview is byte-identical). `renderer/world-preview.js`
is reused verbatim (injected source) with opt-in viewpoint preservation (pure
`src/preview/viewpoint-preserve.js`: DEF → unique description → index → first →
default), navigation-mode restore, and X_ITE pre-validation of nested buffers (a
broken nested edit keeps the last good FULL scene). Saved fallback renders the whole
world from disk without dropping the unsaved overlay. Nonvisual tests:
`test/preview/world-preview-bridge.test.js`, `test/preview/viewpoint-preserve.test.js`,
`test/editor/script-load-order.test.js` (shared-scope co-load guard), `ui-state`
new-file chip model. Visual QA `qa/phase-7c-world-preview/` (`qa:world-preview`):
**22/22** outcome-gated states (incl. a 72-texture world, a nested gzip Inline, and
project-switch cleanup), 1 reused Electron process, 0 survivors, leak-clean; pure
perf/stress `stress.js` (coalescing, alternating document switches,
failed-then-repaired ordering, hash-verified no-write). Locked decisions carried
forward as implemented: 700 ms debounce, 50/50 default split, manual-only Find new
files, 1 MiB auto / 8 MiB hard bands, `Ctrl+Enter` / `Ctrl+Shift+Enter`, and the
split / preview-max / editor-only layouts. See `docs/PREVIEW_ARCHITECTURE.md`
§"Phase 7C3".

**7C5 — Cross-platform acceptance + private beta refresh ✅ complete.** The full 7C
feature set (vision accommodations, native editor, Mall + World unsaved-buffer
previews, last-valid/saved-version fallback, viewpoint & nav preservation, the
Windows-native QA harness) accepted on **both Linux and native Windows 11 Pro**
(libvirt/QEMU guest, local NTFS `C:\Projects\wrlforge`, driven headlessly over SSH).
567/567 tests + syntax gate on both OSes; Tier-1 packed self-test 55/55; all four
Windows GUI visual suites pass via a new **file-based capture transport** (see below);
`build:win` produces the unsigned portable + NSIS `1.3.0-beta.1`; full NSIS
install/uninstall lifecycle + VSCodium launch verified. Three acceptance-found defects
fixed (`f3107af` CRLF `.gitattributes`, `0a9eca8` Windows file transport, `84fdcea`
cross-platform `build:win`). Evidence: `qa/phase-7c5-cross-platform/`. The file
transport exists because a GUI-subsystem `electron.exe` on Windows has an
immediately-ended `process.stdin`, so the capture server reads jobs from
`WRL_FORGE_CAPTURE_JOBS_FILE` there (`qa/visual-qa/transport.js`); the POSIX stdin
path is unchanged.

**7C5.1 — App-icon integration + QA-record closeout ✅ complete.** Scoped closeout,
not a new product phase. The four owner-approved `assets/wrl-forge-*.svg` sources are
integrated; `npm run build:icons` (`scripts/build-icons.js`, `@resvg/resvg-js`
devDependency + pure-Node ICO assembler) deterministically produces the committed
platform icons under `assets/generated/icons/`. **Cyan opaque** is the single
executable identity (window/exe/installer/shortcut/taskbar); all four variants ship
in the install (`resources/icons/`) so a user can repoint a shortcut via Windows
**Change Icon**, and `WRL_FORGE_ICON` selects a build's identity. The retired
placeholder (`assets/icon.ico`/`_make-icon.js`) is removed. The independent
`qa/phase-7c-windows/gemini-qa-report-7c5.md` was corrected against real evidence
(Linux 567/567 across 55 test files, not "224/224"; evidence-based cleanup wording;
registry-confirmed `DisplayVersion`). See `docs/ICONS.md` and
`qa/phase-7c5-icon-closeout/`.

### Phase 7D — Beta Polish
Keyboard accessibility, performance on large worlds, crash recovery, session
restoration, Windows + Linux verification (through the sanctioned VisualQaRunner —
no multi-process screenshot loops), and beta packaging.

The owner-approved lanes that finish Phase 7D are **Phase Beta 2** (crash
recovery, awaiting independent QA), **Phase: Accessibility + Performance**
(D1 + D2, keyboard + perf), and **Phase: Cross Platform Beta** (D3 + D5 +
D6, Windows + packaging). The 7D0 baseline (`docs/PHASE_7D_BASELINE.md`)
kept the old labels (7D1 / 7D2 / 7D3) which are preserved for evidence
back-compat; new documentation uses the names above.

### Phase Beta 2 — Crash Recovery ✅

**Status:** **Phase Beta 2 CLOSED.** Independent final re-QA returned
`PHASE_BETA_2_FINAL_REQA_PASS`. All nine verification items passed
(B1 source identity / external-change protection; B2 missing-source
recovery viewer; B3 second crash after Restore; B4 Editor recovery
prompt; M1 World recovery prompt; recovery lifecycle; graceful close;
session precedence; security boundary). Final runtime: 58 / 58
Electron runtime assertions, 0 console errors, 0 console warnings.
Final repo tests: **1983 / 1983**, 0 failed, 0 skipped. See
`docs/PHASE_BETA_2_CRASH_RECOVERY.md` for the as-built record.

Crash recovery: preserve unsaved native-editor work across abnormal exits
and surface a Restore / Start Fresh prompt on next launch. Source files on
disk are NEVER mutated by recovery.

The recovery file is cleared only by **successful Save / Discard /
explicit Start Fresh** — that rule is enforced in code and pinned by
tests in `test/editor/phase-beta-2-corrections.test.js` and
`phase-beta-2-integration.test.js`.

- [x] Debounced recovery snapshot of the dirty editor buffer under `userData`
      (`src/editor/recovery-store.js` + `src/editor/recovery-controller.js`).
- [x] Restore / Start Fresh prompt at startup (`renderer/recovery-prompt.js`).
- [x] Restore re-installs the recovered buffer as dirty, source untouched.
- [x] Lifecycle hooks at Save success (clears), Save failure (keeps),
      Close (clears), explicit Discard (clears), Back (force-flushes),
      `render-process-gone` (single-shot reload, burst-guarded).
- [x] **QA pass 1 corrections** — B1/B2/B3 lifecycle, B4/M1 prompt pages.
- [x] **QA pass 2 corrections** — real `sourceStat` conflict anchor, script
      order on every page, missing-source viewer.
- [x] **Independent final re-QA**: `PHASE_BETA_2_FINAL_REQA_PASS`.
- [ ] No `mall-session.json` / `world-session.json` (deferred per
      owner decisions §4.4 / §4.5 — see also the Phase 7D0 record).
- [ ] No merge-editor for the case where the on-disk source changed
      since the snapshot.

### Phase: Accessibility + Performance ✅

D1 (keyboard accessibility: ARIA + `aria-keyshortcuts` on the Mall/World
toolbars; `role=list` + roving-tabindex parity in the inspector; recorded
`Ctrl+L` / `Ctrl+O` do-not-add because of the GTK dialog gotcha) plus D2
(large-project perf re-measure: re-baseline under the post-WD2-A main; a
renderer-side memory sanity check on a large world).

**Status:** **CLOSED.** Independent QA verdict:
`ACCESSIBILITY_PERFORMANCE_QA_PASS_WITH_NOTES`.

- Mall + World toolbars carry `role="toolbar"` and a labelled accessible
  name; the two shortcut-bearing Mall buttons advertise `Ctrl+R` /
  `Ctrl+E` through `aria-keyshortcuts`.
- `Ctrl+R` (Repack) and `Ctrl+E` (Open in Native Editor) call the same
  named action path the buttons use; `shortcutSuppressed` blocks the
  keystroke while a text input / select / content-editable region / a
  modal-backdrop owns focus, while `Alt` is held, or while the matching
  button is disabled.
- Inspector findings list is `role="list"` with an `aria-label`;
  rows keep `role="listitem"` (display-only, no fake interactive
  controls).
- Back-navigation focus returns to the originating workspace's primary
  action through a single sessionStorage flag, consumed exactly once.
- Phase 7B perf gate **PASS** on the current `main` (gate:
  `MEDIAN_GATE` — profile median < 250 ms; all profile medians pass:
  small Mall 0.2 ms · representative World 1.4 ms · ~327 KB
  49.3 ms · ~1.3 MB corpus 216.2 ms · script-heavy 34.4 ms · many
  errors 14.2 ms).
- Real Electron renderer memory: **45.20 MB stable** over 30
  edit-and-reparse cycles (no retention leak).
- 72-texture World current render: **847 ms** (`qa/phase-accessibility-perf/`).
- WD2-A pipeline TOTAL: 4.9 ms median / 9.0 ms max on a representative
  World — no production performance code change required.
- 13 focused behavioural tests added
  (`test/renderer/accessibility-runtime.test.js`); 2003 / 2003 tests
  pass, 0 fail, 0 skipped.

**Product direction (durable):** Accessibility is a major WRL Forge
product priority. New user-interface work must include accessibility
design and acceptance checks from the start.

**Future — Preferences & Settings (PLANNED, not started):** a future
top-menu area for user preferences, with accessibility as a
first-class part.

### Preferences & Settings ✅ CLOSED

**Status:** **CLOSED.** Independent final QA returned
`PREFERENCES_SETTINGS_QA_PASS_WITH_NOTES`. Final tests: **2065 / 2065**
pass, 0 failed, 0 skipped. **0 runtime dependencies added. 0 dev
dependencies added.** `contextIsolation: true` / `nodeIntegration: false`
unchanged. No new IPC channel, no new preload bridge, no new network
origin, no new dependency.

A single, shared **Preferences & Settings** area reachable from the
Mall, World, and Editor toolbars. One shared settings model in
`src/settings/preferences.js` (pure, dependency-free), one dialog
(`renderer/preferences.js`), four sections (Appearance, Accessibility,
Keyboard, Editor), and live application of every change. The existing
editor theme / zoom / preview-layout controls are now views of the
same model — no shadow values. Persistence is in `localStorage`
under the same keys the editor has used since Phase 7B; one tiny new
auxiliary key (`wrlforge.editor.lastNonContrastTheme`) backs the High
Contrast toggle's "off" action. The dialog implements the full
a11y contract: `role="dialog"`, `aria-modal="true"`, labelled title,
Escape to close, Tab focus containment, focus return to the opener.
Migration: `NO_MIGRATION_REQUIRED`. Real-Electron smoke + capture-server
runs on the published `1.3.0-beta.5` (Linux x64) confirm the page
loads, the dialog opens, values reflect the persisted state, and
`window.WrlPreferences.set('theme', 'tokyo')` writes through to
`localStorage` immediately. See `docs/PREFERENCES_SETTINGS.md`
for the as-built record.

**Accepted notes from independent QA:**

- **KNOWN_POLISH_NOTE** — UI size currently scales the **Native Editor**
  UI and code area only. Mall / World / Preferences chrome does not
  scale. Global chrome scaling is a future lane; no product-code
  change in this closeout.
- **High Contrast fallback** — if a user's first run under the new
  model has `theme === 'contrast'` with no recorded
  `lastNonContrastTheme`, the first toggle-off reverts to `dark`
  (the default), not the user's original non-contrast theme. This is
  a single-occurrence small loss and is acceptable; the behavior is
  documented in `docs/PREFERENCES_SETTINGS.md` §7 and §11.

**Explicitly not in this lane** (deferred / locked / out of scope):
shortcut remapping, Reset-Everything, external-editor command editor,
cross-window preference sync, application auto-update, direct upload.
Each is its own possible future lane; none is silently absent.

See `docs/ACCESSIBILITY_PERFORMANCE.md` for the full as-built record
and `qa/phase-accessibility-perf/PERF.json` for the WD2-A pipeline
measurements.

### Phase: Cross Platform Beta 🚧 (implemented, awaiting independent QA)

D5 + D6: re-run `dist:linux` on `main` and record SHA-256; re-run the
Tier-1 / Tier-2 visual suites on both platforms; resolve the per-release
`LICENSE` text question; produce a matching cross-platform `v1.3.0-beta.5`
artifact identity. **No signing**, **no public Microsoft Store**, **no
ARM64** — these are explicit, documented out-of-scope items.

**Status:** **implemented** (2026-08-31). The lane bumped the working tree to
`package.json.version = "1.3.0-beta.5"` on **both** the Linux host and the
sanctioned Windows 11 guest (libvirt/QEMU `win11` at `192.168.122.170`,
local NTFS `C:\Projects\wrlforge`), built the five configured x64 artifacts
(`dist:linux` → AppImage + tar.gz; `build:win` → NSIS `Setup` + MSI +
Portable), generated `release/SHA256SUMS-1.3.0-beta.5.txt` (verified
`sha256sum -c` 9/9 OK across the new and historical lines), ran the full NSIS
install → launch → uninstall lifecycle on Windows 11 (Start-menu shortcut
present after install, install dir + shortcut + registry entry removed after
uninstall, `%APPDATA%\wrl-forge\window-state.json` + `editor-session.json`
SHA-256 unchanged across the cycle), and ran the existing source-level
regression suites — `npm test` 2003/2003, `accessibility-runtime` 13/13,
`phase-beta-2-integration` 5/5, `phase-beta-2-corrections` 21/21. Five
artifacts, all unsigned, all x64:

> **Public-download checksums:** the authoritative SHA-256 manifest for the
> published `v1.3.0-beta.5` release is `SHA256SUMS-1.3.0-beta.5.txt` attached
> to the GitHub Release. The hashes below are the **pre-publication
> independently QA-tested build** and may differ from the later GitHub Actions
> rebuild — verify against `SHA256SUMS-1.3.0-beta.5.txt` for public downloads.

- `WRL-Forge-1.3.0-beta.5-linux-x64.AppImage` — 134,527,292 B,
  `45cb38735c814ffc14cb00b6002f7aa7505ac495d1b17b97a6bf99c554f47bec`
- `WRL-Forge-1.3.0-beta.5-linux-x64.tar.gz` — 127,632,242 B,
  `8cb1c5a2d7fd0524ec075a6d61830d24b328db581d0c41d9dd39cd2c94953df4`
- `WRL-Forge-Setup-1.3.0-beta.5-x64.exe` (NSIS) — 111,019,007 B,
  `35f0839816e294af3934e26afc318e74116c24846439b932c685acdcda186771`
- `WRL-Forge-Portable-1.3.0-beta.5-x64.exe` — 110,799,995 B,
  `c8dca0ec6a6b0a3c4ddd87a9e8f46005bb98c87a938f1c0f06cd5527b5768e30`
- `WRL-Forge-1.3.0-beta.5-x64.msi` (MSI) — 126,521,344 B,
  `a521e530887ca3582d703fbb8c507b8e6e0efa970010ca7b6f0997efef1ecf03`

Source SHA: `2f3591b3c56bee1de5ed38de609f22b11b4f5997`. License: **GPL-3.0-or-later**
(`package.json`, `LICENSE`, and the shipped `licenses/LICENSE` inside both the
Linux AppImage and the Windows installer). Evidence: `docs/CROSS_PLATFORM_BETA.md`,
`docs/RELEASES.md` (new beta.5 entry), `docs/BETA_RELEASE_NOTES.md` (new
beta.5 entry).

### Phase WD2-A — Read-Only Scene Tree and Inspector Foundation

**Closed.** Independent final re-QA passed (`WD2_A_FINAL_REQA_PASS`):
70 / 70 Electron runtime assertions, 0 console errors, 0 console warnings;
1923 / 1923 repository tests, 0 failed, 0 skipped. WD2-A is the first
user-interface consumer of the completed P4 chain — a read-only scene tree
and a read-only inspector that consume the same parse the diagnostics come
from. No editing yet, no auto-fix, no localisation.

- Pure read model `src/vrml/scene-tree.js` (built on the AST; never re-
  parses source text; frozen output; `byId` and `defsByName` Maps wrapped
  in read-only Proxies with full Map-read parity: `size`, `keys`, `values`,
  `entries`, `forEach`, `for...of`, `Symbol.iterator`) with a
  `vrml.sceneTree` facade entry.
- One selection authority `src/editor/scene-selection.js` shared by the
  scene-tree view and the inspector.
- Renderer bindings `renderer/scene-tree.js` + `renderer/scene-inspector.js`
  with ARIA tree semantics, depth-first nested rendering, leaf rows with
  no `aria-expanded`, and roving-tabindex keyboard navigation.
- Diagnostics flow strictly through `vrml.presentation` (P4-A) for ordering
  and severity, and `vrml.messages` (P4-B) for text — the view paints the
  severity chip's color and stops there. Ownership is most-specific: a
  finding is shown only on the smallest scene item containing its range.
  The Inspector consumes the already-presented records directly; it does
  NOT call `presentDocumentFindings` a second time.
- USE resolution authority: the renderer builds the WD1.5 scope graph
  first, hands `buildSceneTree` a `useResolver` that calls
  `scopeGraph.resolve(graph, useNode)` (exposed through the bundled
  `scopeGraph` facade), and `findingsForDocument` receives the graph
  (never the raw parseResult, which throws `ESCOPEGRAPH`). The
  `vrml.interfaceQuery` facade deliberately does NOT publish `resolve`
  -- the bundled re-export is the only consumer-facing entry.
- 67 new focused tests (Q1–Q15, M1–M8b, M9, M10, plus F1–F5 + C1–C5
  correction tests, plus 5 runtime tests under DOM stubs); full
  `npm test` + `npm run check` **1923 / 1923**, 0 failed, 0 skipped
  (1852 baseline + 67 new).

Two rounds of independent QA flagged ten findings in total:
round one: F1 `ESCOPEGRAPH` swallowed to `[]`, F2 nested rows not rendered,
F3 diagnostic ownership attached to all ancestors, F4 cross-PROTO
false-resolved USE, F5 mutable Map in frozen return value;
round two: C1 Inspector did not receive `itemById`, C2 Inspector
double-presented findings (would throw `EPRESENTATIONSHAPE`),
C3 read-only Map Proxy broke `size`, C4 leaf rows carried
`aria-expanded="false"`, C5 `vrml.interfaceQuery.resolve` was an unused
facade entry. All ten are reproduced against the prior implementation,
fixed, and pinned with regression tests that fail under the old behaviour.

See `docs/white-dune-2026/WD2_A_SCENE_TREE_INSPECTOR_FOUNDATION.md` for the
as-built record. WD2 editing lanes (field editing, node creation / deletion,
drag-and-drop, reparenting, rename, PROTO/ROUTE editing, auto-fix) and all
WD2-deferred work remain in the deferred list below.

### Phase WD2-B — Typed Inspector Field Editing (first visual mutation)

**IMPLEMENTED — AWAITING INDEPENDENT QA.** Not closed. The Inspector becomes
write-capable for existing, explicitly authored built-in VRML97 field values
of nine schema types (`SFBool`, `SFInt32`, `SFFloat`, `SFTime`, `SFVec2f`,
`SFVec3f`, `SFColor`, `SFRotation`, `SFString`). The source text stays the
document: an Inspector Apply becomes a verified WD1.2 span patch of the
narrowest token/component span, dispatched as ONE isolated CodeMirror history
event on the same buffer the code editor uses, so dirty tracking, recovery,
the live X_ITE preview, Save / Save As, diagnostics, Undo and Redo all follow
from the existing pipeline. The selected node survives an edit only through
WD1.4 Tier 1 identity (a transaction anchor resolved through a verified
receipt); anything unproven is cleared visibly — zero wrong re-anchors.

- Pure model `src/vrml/field-edit.js` (schema-typed, fail-closed gates,
  numeric/string validation, round-trip verification) and
  `src/editor/inspector-edit.js` (apply planning + selection survival),
  published on `vrml.fieldEdit` and the bundled `WRLForgeSceneBridge`.
- Read-only for everything else: absent/default fields, MF* values,
  SFNode/MFNode, IS-bound and X3D-only fields, PROTO-named and vendor node
  types, USE/ROUTE/PROTO/EXTERNPROTO/Document, and any document with a
  structural syntax error.
- Runtime QA: `npm run qa:wd2b` (real Electron, unsaved-buffer X_ITE bounds
  move +3 on X and back through Undo/Redo, Save persists).

See `docs/white-dune-2026/WD2_B_TYPED_INSPECTOR_EDITING.md`. Node creation /
deletion, MF array editing, ROUTE/PROTO editing, gizmos, colour pickers and
every other structural or viewport authoring capability remain future lanes.

### Phase WD2-C — First Object (beginner visual creation + safe structural editing)

**IMPLEMENTED — AWAITING OWNER REVIEW / INDEPENDENT QA.** Not closed. Built on
the WD2-B candidate. A beginner can create a Box or Sphere, change its
Position, Rotation, Size (Box) / Radius (Sphere) and Color, Duplicate it,
Delete it, Undo/Redo each step and Save valid VRML97 — without opening Source
or knowing VRML terms. Every action is an exact, verified source-text patch
dispatched as one isolated CodeMirror history event; the X_ITE preview follows
the buffer through the existing unsaved-buffer path.

- `src/vrml/node-templates.js` (new anonymous `Transform → Shape →
  Appearance/Material → Box|Sphere`, no DEF, no defaults, no metadata),
  `src/vrml/structure-edit.js` (root insertion, absent-field insertion,
  exact-byte Duplicate, owned-span Delete, inserted-node identity; token-stream
  and round-trip verified), `src/vrml/simple-object.js` (beginner facade over
  the parse + schema), `src/editor/first-object.js`, `renderer/model-workspace.js`.
- Model workspace: new/empty documents open in Model (preview first, Add /
  Duplicate / Delete always visible, Object panel, Source one click away);
  existing documents use the remembered `workspaceMode` preference.
- Conservative refusals with plain sentences: syntax errors, missing / non-VRML97
  header, PROTO bodies, SFNode values, DEF/USE/ROUTE/PROTO inside a duplicate,
  deleting a DEF that a USE/ROUTE elsewhere references.
- Runtime QA: `npm run qa:wd2c` (real Electron, literal source oracle per step,
  one undo per action, live X_ITE checks, Save + reopen).

See `docs/white-dune-2026/WD2_C_FIRST_OBJECT.md`. Picking (WD2-C0/WD2-D),
gizmos (WD2-E), hierarchy editing (WD2-F), presets, textures, ROUTE/animation
and PROTO editing remain future lanes.

## Deferred ⛔

Not scheduled into any phase above; requires explicit future direction before any design work begins:

- **Direct upload / authentication / server submission — will NOT be built** (locked product decision). Upload stays a manual Cybertown-website workflow.
- Upload-ready CTR packaging (gated on `docs/WORLD_PACKAGE_QUESTIONS.md`) — the World Project Bundle is review + manual hand-off only.
- Automatic destructive rewrites of user content (all mutation stays backup-first and, where relevant, preview-before-apply)
- Framework migration (e.g., introducing a UI framework/bundler to the renderer) — the current plain HTML/CSS/JS approach is intentional, see `AGENTS.md` Conventions
