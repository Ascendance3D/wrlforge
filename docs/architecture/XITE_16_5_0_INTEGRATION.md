# XITE-16-UPGRADE-2 — X_ITE 16.5.0 integration (Rust/Tauri app)

Status: **implemented, Draft PR, not merged.** Base: `edd659e` (PR #139,
`feature/rust-stab-2b-typed-references`). Evidence from the compatibility
spike (XITE-16-UPGRADE-1, spike commit `305e629`) is preserved outside the
repository under `.qa/wrlforge/xite-16-upgrade-1/` in the workspace. This
lane does not use the spike commit as a base.

The Rust/Tauri app now ships X_ITE **16.5.0**. The Electron v1.4.0 app
still ships X_ITE **15.1.10**. Both apps share one picking adapter.

## 1. Upstream identity

| item | value |
|---|---|
| repository | `https://github.com/create3000/x_ite` |
| tag | `16.5.0` (lightweight) |
| commit | `1956793db500353c8f9fdb078b67429310978323` (`git ls-remote`, rechecked 2026-10-11) |
| includes | PR #232 (merge `eee6e93d…`), PR #234 (merge `51452007…`) |
| npm tarball | `https://registry.npmjs.org/x_ite/-/x_ite-16.5.0.tgz`, `gitHead` = the tag commit |
| tarball SHA-256 | `48dc4fed65ae1b7bb4d4183178291c14bb2cc0ddb58d5a37d5fa6c6a62889142` |
| tarball integrity | `sha512-bpxQecS31b6zVc2SvZlThJNRK4tXBlmr+D5aoO9cGgE71e5nKu8z9sPba+GkFx2GcADncN9oMwpXbbPWeuEtBA==` (equals the npm registry value) |
| `x_ite.min.js` | `7f5996e6544229cdb5f863e4756c798150a09d9aecca98ab4c7af2d8aa9778b0` |
| `x_ite.css` | `e0a8d1fc55bf6f3309470d2eeaef5a948213f27ae029f88901bd970eefa1d3ea` |
| `LICENSE.md` | `4b4a56dafaa360d8541ed5425e9e891cb678e6cd5eaf71bfe7ef25514db01695` |
| all shipped files | 129 files, one SHA-256 each: `apps/desktop-tauri/xite/x_ite-16.5.0.sha256` |

The 129 shipped files in the npm tarball are byte-identical to the same
files in the `dist/` committed at the tag. The XITE-16-UPGRADE-1 manifest
(`xite-16.5.0-dist.sha256`, 136 files) also lists 7 files that WRL Forge
never ships: `X3DUOM.xml`, `example.html`, `x_ite.d.ts`, `x_ite.js`,
`x_ite.min.mjs`, `x_ite.mjs`, and `x_ite.zip`. X_ITE was not rebuilt from
source.

*Correction (STACK-REVIEW-1, PR #140 review r4242714080):* the first version
of this record listed only six of the seven files; `x_ite.zip` was missing.
The list above comes from a name-by-name comparison of the two manifests:
136 − 129 = 7 excluded files, 0 shipped files absent from the 136-file
manifest, and 0 SHA-256 differences for the 129 shared names.

## 2. How the Tauri build obtains X_ITE

```
apps/desktop-tauri/
  xite/PIN                  version, tarball URL + SHA-256, integrity, tag, commit
  xite/x_ite-16.5.0.sha256  the 129 shipped files and their SHA-256
  fetch-xite.sh             download (build time) / --verify DIR (no network)
  build-ui.sh               verifies, then embeds exactly the manifest files
```

1. `build-ui.sh` reads the version from `xite/PIN`. With no `X_ITE_DIST` it
   uses `target/xite/16.5.0/dist`, and runs `./fetch-xite.sh` once if that
   is missing.
2. `fetch-xite.sh` downloads the pinned tarball over HTTPS only (`curl
   --proto =https`). It refuses a tarball whose SHA-256 differs from the
   pin. It extracts only `package/package.json` and `package/dist` into a
   new temporary directory. Then it copies **only the manifest files** into
   `target/xite/16.5.0/` (gitignored) and verifies the copy.
3. `fetch-xite.sh --verify` runs on **every** dist, also one given by
   `X_ITE_DIST`. It refuses (exit 4) a package version that is not exactly
   `16.5.0`, a symlinked dist directory, or a symlinked `package.json`. It
   refuses (exit 5) a missing file, a changed byte, or a symlink in **any**
   component of a manifest path (STACK-REVIEW-1; the first version checked
   only the last component). It refuses (exit 2) a manifest line that is not
   `<64 hex> <relative path>` or whose path has an empty, `.` or `..`
   component. The fetch copies with `cp -P`, so a link stays a link and fails.
4. `build-ui.sh` runs `check-xite-adapters.sh`, which refuses (exit 4) when
   the one real `const XITE_VERSIONS = Object.freeze([...]);` declaration of
   the pick adapter does not list the pinned version as an exact element, or
   the one real `var XITE_VERSION = '...';` line of the gizmo adapter is not
   the pinned version. Each adapter is first masked by `xite-js-mask.awk`, a
   small POSIX-awk JavaScript lexer, so a declaration in a comment (line or
   block), a string or a template literal does not count. The declaration
   must be the only write to its name and a direct statement of the
   adapter's one top-level IIFE, so one in a nested or dead block does not
   count either. The lexer refuses what it cannot lex with certainty
   (STACK-REVIEW-1 and its follow-up; the first version searched the whole
   file, the second matched lines and accepted a block comment).
5. `build-ui.sh` embeds through `fetch-xite.sh --install`: it copies only the
   manifest files into a new `dist/vendor/x_ite/` and then verifies the
   **copy** (`--verify-copy`): every file is a regular file with its pinned
   SHA-256, no path component is a symlink, and nothing unlisted is there.
   A source that changes after step 3 is caught on the bytes that ship; on
   refusal `dist/` is removed. An extra file in a fuller `X_ITE_DIST` is
   never embedded. `test-xite-guards.sh` runs 77 offline cases for 3–5.

No file is replaced after `build-ui.sh` runs. `packaging/package-linux.sh`
runs `build-ui.sh` unchanged. The network is used only at build time, once
per clean `target/`. The app never downloads X_ITE: `dist/` is embedded in
the binary, and the CSP has no remote origin. The root `node_modules`
(Electron's 15.1.10) is no longer read by the Tauri build.

The scripts are Bash with `curl`, `tar`, and `sha256sum` or `shasum -a 256`.
They have no Linux-only path. Windows (Git Bash) and macOS were **not**
tested (section 10).

## 3. Adapter version policy

| file | used by | policy |
|---|---|---|
| `src/preview/xite-pick-adapter.js` | Electron **and** Tauri (copied verbatim) | `XITE_VERSIONS = Object.freeze(['15.1.10', '16.5.0'])`; the browser version must be a string that is an exact member |
| `apps/desktop-tauri/ui/static/xite-gizmo-adapter.js` | Tauri only | `XITE_VERSION = '16.5.0'`, exact |

- There is one source of picking logic. The adapter is not forked per app.
- The list has exactly two entries, each proven on its host. 15.1.10 is
  proven by WD2-D and Electron QA. 16.5.0 is proven by XITE-16-UPGRADE-1 and
  this lane. A third version needs its own lane.
- No range, prefix, or "newer is fine" rule is used. The tests reject
  `16.5.1`, `16.4.0`, `16.5`, `16.5.0-dev`, a padded string, a joined
  list, an array, a number, and a missing value (`xite-version-mismatch`,
  the fail-closed `COMPATIBILITY_DISABLED` path).
- The G4 probe, every P-row surface check, the shared hook coordinator,
  and the stale, ambiguous, sensor-conflict, and PROTO refusals are
  unchanged. The source-selection proof rules are unchanged.
- Each host pins its own build. A mismatch fails the build, so a host cannot
  load the other host's version by accident.

Guard tests (`test/preview/xite-pick-guards.test.js`):

| guard | checks |
|---|---|
| G1 (Electron) | root `package.json` + lockfile pin exactly `15.1.10` (unchanged) |
| G1 | the adapter list is exactly `['15.1.10', '16.5.0']`, declared once, used once, with an exact-membership gate and no range logic |
| G1 (Tauri) | `xite/PIN` is exactly 16.5.0 at commit `1956793`. The manifest lists the three main files with the recorded hashes, has no duplicate entry, no traversal, and no absolute path, and includes all three font license files |
| G1 (Tauri) | the gizmo adapter pin, and that `build-ui.sh` never names `node_modules`, verifies every dist, and holds no second hard-coded version |
| G2 (Electron) | installed `node_modules/x_ite` is 15.1.10 (unchanged) |
| G3 | private names in Electron's `x_ite.min.js` (unchanged) |
| G3 (Tauri) | the same private names in the fetched 16.5.0 bundle. This check skips until `fetch-xite.sh` has run, and it ran in every verification here |

## 4. How Electron compatibility is preserved

- The root `package.json` and `package-lock.json` are **unchanged**:
  `x_ite` stays exactly `15.1.10`. Electron G1 and G2 still pass.
- The Electron renderer still loads `node_modules/x_ite/dist/x_ite.min.js`
  (15.1.10). The shared adapter still accepts 15.1.10.
- `test/preview/_fake-xite.js` still defaults to `15.1.10`, which is
  Electron's real runtime. New adapter tests run the gate and the G4 probe
  for both versions.
- `npm run check`: 2,637 tests, 2,633 pass, 0 fail, 4 skipped. The 4 skips
  are the existing maintainer-only ISO-mirror schema tests.
- The Electron GUI (VisualQaRunner) was **not** re-run. The only Electron
  code change is the adapter's version list, which keeps 15.1.10 exactly.
- `src/vrml/node-schema.js` and the Rust schema stay generated from the
  15.1.10 `x_ite.d.ts`. This lane does not regenerate them.
- No wider Electron migration is needed.

## 5. Changed renderer behavior and the ROUTE notice

X_ITE 16.5.0 drops a ROUTE whose node it cannot find, with a console
warning, and renders the rest of the scene. X_ITE 15.1.10 rejected the whole
scene, and the preview kept the last valid scene. Two fixtures show the
change: `flux-regression/malformed/m6_route_before_def.wrl` and
`test/fixtures/vrml/invalid-route.wrl`.

The owner accepts partial rendering when it is clearly reported. WRL Forge
now reports it from **Rust diagnostics only**:

- `doc_preview_source` returns `route_warnings`. Each entry is a ROUTE whose
  whole-ROUTE verdict (`scope::RouteCheck`, RUST-STAB-2B) is `Unresolved` or
  `Invalid` in the exact preview text. That covers an unresolved node
  (SCOPE001/002/003), an unknown event (SCOPE101), a direction violation
  (SCOPE102/103), and a type mismatch (SCOPE104).
- Answers that are `Ambiguous`, `Recovered`, or `Unsupported` prove nothing
  and never warn. This includes a duplicate DEF, a damaged parse, and an
  EXTERNPROTO the URL would decide.
- The viewport shows "⚠ The preview may be incomplete because one or more
  ROUTEs are invalid." with the line, and the full list on hover. The notice
  changes only when a load succeeds. A refused load keeps the last valid
  scene, so it keeps that scene's notice. Close clears it.
- Nothing is inferred from what X_ITE drew. The authored ROUTE is never
  changed. The source diagnostics are still shown. The notice is advisory
  and never blocks Save.
- Partial rendering does not authorize edits. A dropped ROUTE is not a node
  statement, so no provenance record and no pick target can come from it.
  The new `badroute` pick variants put an invalid ROUTE before every node.
  All 12 clicks there are PROVEN on the exact oracle span, with WRONG 0.

Tests: `service::tests::route_warnings_report_only_proven_invalid_routes`
covers the two fixtures, SCOPE101–104, valid ROUTEs, ambiguous, damaged,
and EXTERNPROTO cases, plus a BOM + CRLF file through the service, with no
source change and an exact Save As. The file smoke step "ROUTE notice
matches the Rust-proven invalid ROUTEs" checks the DOM against Rust for the
revision on screen.

## 6. Upstream fixes verified at runtime (WebView console)

The app does not forward WebView console output, and this lane does not add
that. The texture smoke installs a **smoke-only** trace
(`wrlforgePreview.consoleTraceStart/Stop`). It copies console lines and counts
`DOMParser.parseFromString` calls for one load only, then restores both.
Nothing starts outside a smoke run, nothing is kept after stop, and nothing
is sent anywhere.

**PR #232 — resource fallback diagnostics** (texture fixtures):

| fixture | URLs | console | requested by X_ITE, in order |
|---|---|---|---|
| t07 first wins | `cyan.png`, `red.jpg` | none | `cyan.png` |
| t08 missing first | `missing.jpg`, `orange.png` | 1 × `warn: … Couldn't load URL '…missing.jpg', trying URL 2 of 2.` | `missing.jpg`, `orange.png` |
| t10 one missing | `nothere.jpg` | 1 × `error: … Couldn't load URL '…nothere.jpg'.` | `nothere.jpg` |
| t13 symlink escape | `link.jpg`, `linkdir/red-outside.jpg` | 1 × "trying URL 2 of 2" + 1 × `error: … Couldn't load any of the 2 URLs, tried in this order: 1. … Forbidden 2. … Forbidden` | `link.jpg`, `linkdir/red-outside.jpg` |

The fallback order equals the authored order. The rendered colors and the
Rust resource logs are unchanged from XITE-16-UPGRADE-1.

**PR #234 — no XML parsing of non-XML scenes:**

| input | DOMParser calls | load |
|---|---|---|
| every VRML97 texture fixture (14 loads) | 0 | as expected |
| VRML97 string | 0 | loaded |
| BOM + VRML97 string | 0 | refused by the VRML parser (the app strips the BOM before preview, so app BOM files load: `x__bom-crlf-route`, `x__bom-proto`) |
| X3D XML string | 1 | loaded |
| BOM + X3D XML with `<?xml …?>` | 1 | refused by `DOMParser` (a BOM before the declaration is not well-formed in a parsed string) |
| BOM + X3D XML without a declaration | 1 | loaded |

## 7. Regression results (Linux x64, exact commit)

*Correction (STACK-REVIEW-1, PR #140 review r4242714093):* the first version
of this sentence said that all runs used `18c2fcf`, while the PR named
`c23646b`. The evidence (`.qa/wrlforge/xite-16-upgrade-2/`, internal) holds
two full sets, one per commit. `c23646b` only adds this document, and the
debug and release binaries are byte-identical at both commits.

| run | `18c2fcf` (implementation) | `c23646b` (final) |
|---|---|---|
| `cargo test` desktop, crates, workspace | yes (`logs/verify-*`) | yes (`logs/final-*`) |
| `npm run check` | yes (`logs/verify-npm-check.log`) | yes (`logs/final-npm-check.log`) |
| GUI: create, extrusion, move, rotate/scale, texture, pick, recovery | yes (`results/dev-18c2fcf/`) | yes (`results/final-c23646b/`) |
| file and Inspector smoke, 58 fixtures | yes | yes; identical per file |
| Linux packages | yes (`logs/verify-package-A.log`; SHA-256 below) | yes (`logs/final-package.log`; `.deb` `a355a4bc…`, AppImage `a232e8f3…`) |
| packaged-binary runs (`strace`) | yes (`pkgrun/dev-18c2fcf/`) | yes (`pkgrun/final-c23646b/`) |

Per-file and per-suite verdicts are identical at both commits (pick and
recovery differ only in temporary paths). The unprefixed logs
`cargo-test-*.log`, `npm-check.log` and `pkgrun.txt` do not record a commit;
they are not used as evidence for either SHA. The table below gives the
results, which are the same for both commits.

Each set ran in a clean, detached verification worktree.
`build-ui.sh` fetched X_ITE from scratch there. The tests used temporary
copies of the 58 XITE-16-UPGRADE-1 fixtures and Xvfb (software rendering).
Each GUI run had its own `xvfb-run -a` display and `/tmp` directory.

| suite | result | reference (lane 1, 16.5.0 pinned) |
|---|---|---|
| `cargo test -p wrl-forge-desktop` | 87 pass, 0 fail | 86 (+1 new ROUTE test) |
| `cargo test -p wrlforge-vrml -p wrlforge-document` | 165 pass, 0 fail | 165 |
| protocol, render, scene crates | 57 pass, 0 fail | — |
| `npm run check` (Electron + guards) | 2,633 pass, 0 fail, 4 skipped | — |
| file smoke, 58 fixtures | 47 pass; the 11 fails are the known invalid inputs | 45 pass (both gzip twins timed out) |
| Inspector smoke, 58 fixtures | 18 pass, identical per file | 18 |
| pick matrix | **WRONG 0**; 37 fixtures, 152 clicks; PROVEN 128, REFUSED_AMBIGUOUS 16, NO_HIT 3, REFUSED_SENSOR_CONFLICT 2, UNSUPPORTED 1, SKIP 2 | WRONG 0; 34 fixtures, 140 clicks; PROVEN 116, the rest identical |
| `WRONG_SOURCE_SELECTIONS = 0` | 199 picks, 165 proven, 28 refused, 6 no-hit, **0 wrong**, 0 oracle mismatches | 187 picks, 0 wrong |
| move | 156/156 steps | 156/156 |
| rotate / scale | 71/71 steps, 0 differing from lane 1 | identical |
| extrusion | 58/58 steps, 0 differing | identical |
| create (Box, Sphere, Cylinder, Cone, Inspector, undo, redo, save) | 51/51 steps (2 differ: backup timestamps only) | identical |
| texture + resources | 46/46 steps (27 earlier steps + 19 new #232/#234 steps) | 27/27 |
| recovery (SIGKILL, Restore, Discard, Later) | crash phase OK (4 records); restore 34/34; later 15/15 | identical |

What the suites cover: startup and shutdown (every run); plain and gzip
VRML97; BOM, CRLF, CR, and Unicode (pick source-form variants, `x__bom-*`,
`vrml__crlf`); PROTO, ROUTE, and malformed source; the live unsaved preview
and the last valid scene (file smoke); camera preservation (move,
rotate/scale); Inspector edits; undo and redo; local textures, relative
paths, and URL fallback (texture).

Fixture eligibility is separate from failure. Of the 40 Inspector-mode
failures, 33 are fixtures with no editable SF field on a real node. Three
cannot run (the two gzip twins time out at 120 s; `preview__gz-corrupt` is
an invalid input that exits 101). Three stop at a missing DOM reply
(`vrml__crlf`, `vrml__plain-twin`, `vrml__valid-gzip`), and `vrml__numbers`
is an invalid input. All 40 are identical per file to the 15.1.10 control
in lane 1, so none is an X_ITE regression. Valid fixtures give positive
Inspector coverage: the 18 passing fixtures (14 Inspector steps each), plus
create 9/9, move 13/13, and extrusion 5/5 Inspector steps.

The two gzip twins passed the file smoke in this run. They timed out at
120 s in every lane-1 build and still time out in Inspector mode. The
cause was not investigated, so this result is not claimed as a fix.

Caps: 150 s for each file run (smoke.sh's own cap is 120 s), 180 s for
move, rotate/scale, and texture, and 240 s for pick. Pick needed 176 s,
which is too close to 180 s. Pick, move, and the other suites ran
staggered by 3 s; the files ran 8 in parallel, and the Inspector runs 6.

## 8. Numeric differences

The numeric values are those of the lane-1 16.5.0 pinned build, to the
last digit. Compared with 15.1.10 they keep the small, repeatable shifts
that lane 1 recorded:

| measurement | 15.1.10 | 16.5.0 (lane 1 and now) |
|---|---|---|
| pick: viewport coverage, four objects | 18.13% | 18.10% |
| extrusion: Create Extrusion coverage | 5.23% | 5.29% |
| rotate/scale: rotate-drag angle | 0.7799 rad | 0.7783374 rad |

Move differs from lane 1 in 25 numeric steps (camera trace and drag
values), and the ok flag is the same on both sides. Move is noisy run to
run: lane 1 measured 29 differing steps between two 15.1.10 control runs.
**No tolerance was changed.** Every invariant passes with the existing
tolerances.

## 9. Resource security

Unchanged in this lane: `tauri.conf.json` (CSP), `src-tauri/src/resources.rs`
(the `wrlres` resolver), and `src-tauri/capabilities`. Texture results:

- t12, traversal (`../`, `..%2f`, `%2e%2e/`, `sub/../../`): untextured, and
  Rust reports "path leaves the document folder".
- t13, symlink escape (file and directory link): HTTP 403 for both, the
  untextured surface, and "link resolves outside the document folder".
- t14, remote and absolute (`http:`, `https:`, `file:`, absolute path):
  never loaded, untextured.
- t15, a reply for a closed document: refused, 403 "preview replaced or
  document closed".
- t10 (missing) and t11 (invalid bytes): the untextured surface, with a
  Rust warning.

The X_ITE fetch adds no runtime network access. It is a build-time
download, HTTPS only, and checked against the pinned SHA-256.

## 10. Platform status

| platform | status |
|---|---|
| Linux x64 (Ubuntu 24.04, Xvfb software rendering) | **tested**: everything above, plus the packages below |
| Windows x64 | **not tested**. No native Windows system was used. Wine was not used as evidence. `fetch-xite.sh` and `build-ui.sh` need Bash (Git Bash) with `curl`, `tar`, and `sha256sum`. |
| macOS 15+ (Apple Silicon) | **not tested**. No macOS system was available. The scripts fall back to `shasum -a 256`. |

A Linux pass does not prove Windows or macOS behavior.

**Linux packages** (`packaging/package-linux.sh`, unchanged process, commit
`18c2fcf`; not published; the `c23646b` packages are `.deb` `a355a4bc…` and
AppImage `a232e8f3…`, same sizes and the same payload):

| artifact | SHA-256 |
|---|---|
| `WRL Forge_0.1.0_amd64.deb` (11,983,394 B) | `c3ecd52b6b518f1c6473383f0742db559cf15a14d0eafdf7b2320dd1f16d8ca6` |
| `WRL Forge_0.1.0_amd64.AppImage` (92,908,024 B) | `ca3a25b5fb2682f7ba13e69b17886610662b71a58b5a95a3bfc17cb77922f993` |

- The private-path check in `package-linux.sh` passed.
- Both packages carry `X_ITE-LICENSE.md` (SHA-256 `4b4a56…`, equal to
  upstream 16.5.0), `THIRD_PARTY_NOTICES.md`, `README-licenses.txt` (names
  X_ITE 16.5.0), `LICENSE`, `COPYRIGHT.md`, `LICENSE-tokyo-night.txt`, and
  `RUST-CRATES-LICENSES.txt`. The three font license files are inside the
  embedded `dist/`.
- The extracted `.deb` binary and the AppImage (through `AppRun`) ran with
  an empty environment. `PATH` held only `xdotool`, `timeout`, `bash`,
  `readlink`, `dirname`, `realpath`, `dbus-send`, and `gsettings`.
  `strace` recorded **0** executions of `node`, `npm`, `cargo`, or `rustc`.
- File smoke on `vrml__invalid-route.wrl`: 54/54 steps in each package,
  including the ROUTE notice (2 ROUTEs).
- Move smoke: 156/156 steps in each package. The gizmo adapter accepts only
  exactly 16.5.0, so this is runtime proof of the embedded version.

## 11. Remaining limits

1. Windows x64 and macOS 15+ are not tested (section 10).
2. The Electron GUI was not re-run against the changed adapter. The adapter
   still accepts 15.1.10 exactly; the Node suite passes.
3. Linux was tested under Xvfb with software rendering only. Coverage
   values can differ on a real GPU.
4. The gzip twin fixtures still time out in Inspector mode. They passed the
   file smoke once; the cause is not known.
5. 33 Inspector fixtures have no eligible SF field. This is fixture
   eligibility, unchanged since lane 1.
6. The ROUTE notice reports proven source defects only. A ROUTE that X_ITE
   drops for another reason (for example inside an EXTERNPROTO whose
   interface Rust cannot see) gives no notice by design.
7. The PR #232/#234 console proof uses a smoke-only trace. Production still
   does not forward WebView console output.
8. A BOM before an X3D XML declaration, given to X_ITE as a raw string, does
   not load (`DOMParser`). WRL Forge never passes such a string; it is
   recorded, not fixed.
9. The node schema stays generated from the 15.1.10 `x_ite.d.ts`.
10. The packages are unsigned and not published.
