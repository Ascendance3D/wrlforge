# STACK-REVIEW-1 — Copilot findings on PRs #139–#143

Lane: STACK-REVIEW-1. Branch `review/copilot-stack-hardening-1`.
Base: `07729396228ced257361985896d533f619b25428` (`feature/ui-menu-1`, the
head of Draft PR #143). The parent branches of PRs #139–#143 are not
changed. Nothing is merged, tagged, released or published.

## 1. Inventory

The inventory was read from GitHub on 2026-10-12, before any code change.
It matches the expected count. No new comment was found.

| PR | branch | Copilot findings |
|---|---|---|
| #139 | `feature/rust-stab-2b-typed-references` | 0 |
| #140 | `feature/xite-16-5-0-integration` | 5 |
| #141 | `feature/app-shell-0` | 5 |
| #142 | `feature/ui-dock-1-magnetic-workspace` | 3 |
| #143 | `feature/ui-menu-1` | 3 |
| | **total** | **16** |

An outdated review thread was not taken as proof of a fix. Each finding was
checked against the code at the base SHA.

## 2. Status of all 16 findings

Status words: **fixed here** (a commit on this branch), **fixed in PR #143**
(an older commit in the stack, re-verified here), **documentation** (a
document correction on this branch).

| # | PR | comment | file | status | commit |
|---|---|---|---|---|---|
| 1 | #140 | [r4242714025](https://github.com/Ascendance3D/wrlforge/pull/140#discussion_r4242714025) | `fetch-xite.sh` | still present at base → **fixed here** | `309a310` |
| 2 | #140 | [r4242714046](https://github.com/Ascendance3D/wrlforge/pull/140#discussion_r4242714046) | `build-ui.sh` | still present at base → **fixed here** | `309a310` |
| 3 | #140 | [r4242714062](https://github.com/Ascendance3D/wrlforge/pull/140#discussion_r4242714062) | `service.rs` | still present at base → **fixed here** | `604718b` |
| 4 | #140 | [r4242714080](https://github.com/Ascendance3D/wrlforge/pull/140#discussion_r4242714080) | `XITE_16_5_0_INTEGRATION.md` | documentation → **fixed here** | `4e94e3d` |
| 5 | #140 | [r4242714093](https://github.com/Ascendance3D/wrlforge/pull/140#discussion_r4242714093) | `XITE_16_5_0_INTEGRATION.md` | documentation → **fixed here** | `4e94e3d` |
| 6 | #141 | [r4242710837](https://github.com/Ascendance3D/wrlforge/pull/141#discussion_r4242710837) | `about.rs` | still present at base → **fixed here** | `0caa7b5` |
| 7 | #141 | [r4242710854](https://github.com/Ascendance3D/wrlforge/pull/141#discussion_r4242710854) | `APP_SHELL_0.md` | documentation → **fixed here** | `4e94e3d` |
| 8 | #141 | [r4242710866](https://github.com/Ascendance3D/wrlforge/pull/141#discussion_r4242710866) | `APP_SHELL_0.md` | documentation → **fixed here** | `4e94e3d` |
| 9 | #141 | [r4242710880](https://github.com/Ascendance3D/wrlforge/pull/141#discussion_r4242710880) | `APP_SHELL_0.md` | code fixed in PR #143 (`c488c80`); documentation **fixed here** | `4e94e3d` |
| 10 | #141 | [r4242710895](https://github.com/Ascendance3D/wrlforge/pull/141#discussion_r4242710895) | `APP_SHELL_0.md` | code fixed in PR #143 (`c488c80`); documentation **fixed here** | `4e94e3d` |
| 11 | #142 | [r4242637970](https://github.com/Ascendance3D/wrlforge/pull/142#discussion_r4242637970) | `dock.rs` | still present at base → **fixed here** | `7ac0bcb` |
| 12 | #142 | [r4242637993](https://github.com/Ascendance3D/wrlforge/pull/142#discussion_r4242637993) | `dock.rs` | **fixed in PR #143** | `6e790cb` |
| 13 | #142 | [r4242638014](https://github.com/Ascendance3D/wrlforge/pull/142#discussion_r4242638014) | `dock.rs` | **fixed in PR #143** | `6e790cb` |
| 14 | #143 | [r4243044078](https://github.com/Ascendance3D/wrlforge/pull/143#discussion_r4243044078) | `clipboard.rs` | **fixed in PR #143** | `d19744f` |
| 15 | #143 | [r4243044099](https://github.com/Ascendance3D/wrlforge/pull/143#discussion_r4243044099) | `app_menu.rs` | **fixed in PR #143** | `d19744f` |
| 16 | #143 | [r4243044118](https://github.com/Ascendance3D/wrlforge/pull/143#discussion_r4243044118) | `menu.rs` | **fixed in PR #143** | `0772939` |

Fixed on this branch: 1, 2, 3, 6, 11 (code) and 4, 5, 7, 8, 9, 10
(documentation). Fixed by older commits in PR #143: 9 and 10 (code), 12,
13, 14, 15, 16. No finding was classified "not reproduced".

## 3. Findings fixed on this branch

### 3.1 #1 — X_ITE symlink verification (`309a310`)

- **Problem.** `fetch-xite.sh --verify` checked `-L` only on the last path
  component. A symlinked `assets/` directory passed, so the verified tree
  could come from outside the selected dist.
- **Change.** `verify` refuses a symlinked dist directory, a symlinked
  `package.json`, and a symlink in **any** component of a manifest path.
  Every manifest line must be `<64 hex> <relative path>` with no empty, `.`
  or `..` component, so no entry can name a file outside the dist. The
  fetch copies with `cp -P`, so a link in the tarball stays a link and
  fails. The pinned SHA-256 checks are unchanged.
- **Evidence.** `apps/desktop-tauri/test-xite-guards.sh` (offline, no
  download). 18 symlink and manifest cases: valid tree; trailing slash;
  final-file link to a correct-hash file; intermediate and deep
  intermediate directory links; file and directory links outside the tree;
  symlinked dist; symlinked `package.json`; hash and version mismatch; six
  bad manifest paths. All pass. The base script accepted 5 of the forbidden
  cases. The real pinned 16.5.0 tree still verifies.
- **Limit (closed by §3.6).** `build-ui.sh` copied the files after the
  check and did not check the copy. The follow-up verifies the copy.

### 3.2 #2 — X_ITE version guard (`309a310`)

- **Problem.** `build-ui.sh` searched the whole adapter for `'16.5.0'`. A
  comment or another string passed after the version left `XITE_VERSIONS`.
- **Change.** New `check-xite-adapters.sh` (Bash and `sed`, no Node.js).
  It needs exactly one line that is only
  `const XITE_VERSIONS = Object.freeze([...]);`, whose elements are all
  exact quoted versions and include the pin. The gizmo adapter needs
  exactly one line that is only `var XITE_VERSION = '<pin>';`.
- **Evidence.** 10 adapter cases in `test-xite-guards.sh`: valid; version
  removed; version only in comments; version in another string; trailing
  comment; duplicate declaration; prefix version `16.5.0.1`; non-literal
  element; gizmo version only in a comment; gizmo declaration commented out.
  All pass. The base check accepted 7 of the forbidden cases. The Node
  adapter suites pass (47 pass, 1 skipped), including G10: 15.1.10 and
  16.5.0 pass the runtime gate. The pick smoke accepts the embedded 16.5.0.
- **Defect found later (fixed in §3.6).** The line match accepted a valid
  declaration line inside a `/* ... */` block comment, a template literal or
  a dead block.
- **Limit.** The check reads the line shapes that the adapters use now. A
  reformatted declaration (for example over several lines) is refused, not
  accepted; the check then needs an update.

### 3.3 #3 — ROUTE diagnostic lookup (`604718b`)

- **Problem.** `invalid_routes()` scanned every scope diagnostic from the
  start for each proven-invalid ROUTE: O(N²) per debounced preview.
- **Change.** `ScopeGraph::diagnostics()` sorts by `(start, end, code)`
  (checked in `crates/wrlforge-vrml/src/scope.rs`; a `debug_assert` guards
  it). `first_finding_inside()` binary-searches to the first finding that
  starts at or after the ROUTE and stops at the first finding that starts
  after the ROUTE end. It returns the finding that a linear `find` returns.
  No verdict, text, line or source byte changes.
- **Evidence.** The old lookup is kept in the tests as a reference. Generated
  documents with valid ROUTEs, seven proven-invalid kinds (missing node,
  before-DEF, across a PROTO boundary, missing event, wrong direction twice,
  type mismatch), an ambiguous DEF and an EXTERNPROTO-decided endpoint give
  identical warnings for 0, 1, 3 and 40 blocks (7 warnings per block). Every repository `.wrl`
  fixture gives identical warnings. The existing ROUTE test passes.
- **Performance** (release, `route_warning_lookup_timing`, ignored test):

  | ROUTEs | invalid | old (total) | new (total) | parse + scope + diagnostics |
  |---|---|---|---|---|
  | 2,750 | 1,750 | 12.7 ms | 9.8 ms | 10.9 ms |
  | 11,000 | 7,000 | 69.8 ms | 49.7 ms | 34.0 ms |
  | 44,000 | 28,000 | 612.2 ms | 216.2 ms | 187.0 ms |

  At 44,000 ROUTEs the lookup part falls from about 425 ms to about 29 ms.
  The totals include the shared parse, so they are single runs, not a
  benchmark.

### 3.4 #6 — About dialog focus (`0caa7b5`)

- **Problem.** Focus moved to Close only after `app_info` answered, and the
  Loading → Ready change rebuilt the whole dialog. With a slow IPC the focus
  stayed outside `role="dialog"`, so Escape did not work.
- **Change.** The dialog frame and Close button live for the whole open;
  only the body follows Loading / Ready / Failed. Close takes the focus when
  it mounts (`NodeRef` + `Effect`). Each open has a number; an answer for an
  older open is dropped, so a late answer never reopens a closed dialog and
  never replaces a later open's state. Closing gives the focus back to the
  control that had it before the open. The version display is unchanged.
- **Evidence.** Menu smoke, 6 new steps with a test-only delayed `app_info`
  (`about::show_with`, 1.5 s): focus on Close before the answer; Escape
  while loading closes it, focus returns to Source, no reopen; Close while
  loading, same; a late failing answer for an earlier open does not replace
  a later open; after the answer the version shows and focus stays on Close;
  Source focus and selection are restored and the document is unchanged.
  The base logic fails the focus and Escape steps (66/69, run stopped).
- **Limit.** The delay is a test hook around the same code path, not a slow
  backend.

### 3.5 #11 — Magnetic docking guide (`7ac0bcb`)

- **Problem.** The edge guide was `zone_rect`: half of the target group's
  current content. The accepted drop splits the whole tab node, adds a tab
  strip and a divider, clamps to minimum sizes, collapses splits and frees
  the moved panel's space.
- **Change.** `layout::drop_preview()` builds the drop on a copy of the
  layout, validates it with `Layout::dock`, solves it with the workspace's
  own `solve()` and returns the moved panel's content box. A valid
  non-strip drop shows that box. `zone_rect` stays only as the fallback for
  a refused target. The live layout does not change before the release.
- **Evidence.** Protocol tests: `drop_preview` equals the applied layout's
  box for every preset × panel × anchor × zone in three workspace sizes
  (refused drops return the same error), plus four edges, minimum width,
  merge, split and a group left with one tab. Dock smoke, 10 real drags:

  | case | guide vs. final panel box |
  |---|---|
  | Inspector → Viewport left / right / top / bottom (its split collapses) | 0.00 px each |
  | Diagnostics → Viewport bottom (neighbor stays) | 0.00 px |
  | Diagnostics → Source center (merge) | 0.00 px |
  | Scene Tree → Source right (split) | 0.00 px |
  | Source out of Modeling's two-tab group → Viewport right | 0.00 px |
  | Inspector → Modeling's two-tab group, bottom | 0.00 px |
  | Scene Tree → Viewport left at 1000 px (minimum width binds) | 0.00 px |

  `SNAP_GUIDE_MAX_DIFF_PX = 0.00`, tolerance 1 CSS px. The old guide was off
  by 3.33–355.38 px in the same cases. An invalid target shows the
  fallback (0.01 px from `zone_rect`) and docks nothing. The document,
  Source selection, preview, X_ITE canvas and Source textarea objects are
  unchanged; a later real pick selects the right node.
- **Limit.** A tab-strip drop still shows the insertion caret, not the
  content box; it marks the tab index.

### 3.6 Follow-up — block-comment declaration and copied-file check

This is an owner-requested follow-up to #1 and #2, not a new Copilot
finding. It is one commit after `845e30a` on this branch.

- **Defect.** `check-xite-adapters.sh` matched lines. It accepted this input
  (the real declaration does not list `16.5.0`):

  ```javascript
  /*
    const XITE_VERSIONS = Object.freeze(['15.1.10', '16.5.0']);
  */
  const XITE_VERSIONS = Object.freeze([
    '15.1.10'
  ]);
  ```

  Reproduced on `845e30a` before any change: exit 0. The same check also
  accepted the declaration inside a template literal and inside
  `if (false) { ... }` (exit 0 each).
- **Change.** New `apps/desktop-tauri/xite-js-mask.awk`: a POSIX-awk
  JavaScript lexer (about 70 lines, no Node.js, no new dependency). It turns
  each comment into a space, each string into `'_'` (an exact version string
  stays), and each template literal with all `${...}` expressions into
  `` `_` ``. It reports the brace depth of each line and the depth-0 code.
  It refuses (exit 3) an unterminated comment, string or template, a string
  line continuation, unbalanced braces, and a `/` that could start a regular
  expression. `check-xite-adapters.sh` then needs, on the masked code:
  one top-level IIFE and nothing else; exactly one write to the name (`=`,
  a compound assignment, `++`/`--`, a second `let`/`var`/`const`); and that
  write as the one-line declaration at IIFE depth 1. The version rules are
  unchanged: exact quoted elements, the pin in the list, the gizmo equal to
  the pin. Comments around the declaration are now allowed.
- **Copied files.** `build-ui.sh` verified the source and then copied it
  with no further check. It now calls `fetch-xite.sh --install SRC DEST`,
  which copies the manifest files into a new `dist/vendor/x_ite/` and runs
  `--verify-copy` on the copy: each manifest file is a regular file with its
  pinned SHA-256, no path component is a symlink, and the copy holds no
  unlisted entry. On refusal `build-ui.sh` removes `dist/` and exits 4. The
  release file list (129 files) and the pinned distribution are unchanged.
- **Evidence.** `test-xite-guards.sh`: 77 cases at this commit (91 after
  §3.7), all pass. They are the 18
  symlink/manifest and 10 adapter cases from §3.1/§3.2 (unchanged), 33 new
  declaration cases (whole synthetic adapters: current one-line form, normal
  surrounding comments, block comment then wrong real declaration, block
  comment only, inline block comment, string, template, nested template,
  `}` in a template string, version in an unrelated variable, duplicates,
  missing, multi-line, double-quoted, malformed, dead block, inner function,
  outside the IIFE, a second top-level statement, regex literal,
  unterminated string/comment/template, line continuation, unbalanced brace,
  division, and six gizmo cases including reassignment and `+=`), and 16
  copy cases (valid install; source changed, linked or removed after the
  check; copy with a missing file, changed bytes, file link, directory link,
  extra file, extra link; existing or linked destination; and two static
  checks that `build-ui.sh` embeds only through `--install`). With the
  `845e30a` scripts, 18 of the 33 new declaration cases give the wrong
  result, and the copy check does not exist. The lexer gives byte-identical
  output for both real adapters under gawk, mawk and BusyBox awk. The real
  pinned 16.5.0 tree installs and verifies (129 files).
- **Limit.** The lexer does not support regular-expression literals,
  string line continuations or destructuring writes. The first two refuse;
  the adapters use none of them. The check does not prove that code before
  the declaration cannot `return` or `throw`.

### 3.7 PR #144 review (Copilot, on `845e30a`)

Copilot review
[5486909537](https://github.com/Ascendance3D/wrlforge/pull/144#pullrequestreview-5486909537)
has two findings. Both are confirmed and fixed in one commit after §3.6.
They are not counted in the 16 findings of PRs #139–#143.

- **#144-A (high, [r4244998855](https://github.com/Ascendance3D/wrlforge/pull/144#discussion_r4244998855)):
  the fetch path bypasses the intermediate-link guard.** `fetch-xite.sh`
  extracted the tarball, copied each manifest file with `cp -P`, and
  verified only the copy. `cp -P` keeps only a final link; it follows a link
  in a parent directory. Reproduced on the `845e30a` script: with a linked
  `package/dist/assets` whose files have the pinned hashes, `--verify` of the
  extracted tree refuses (exit 5), but the copy has no link and its check —
  the only check of the old fetch — passes (exit 0).
  **Change.** The fetch now runs `verify` (version, `package.json`, a link in
  any component, hashes) on the **extracted** tree before it copies
  anything, then copies through `install_copy` and verifies the result.
  `install_copy` also refuses a linked source before it copies; the check of
  the copy stays authoritative. `--from-tarball TGZ` runs the same fetch path
  on a local tarball (the SHA-256 pin still applies), so the path has
  offline tests. **Evidence.** 14 new cases in `test-xite-guards.sh` (91 in
  total, all pass): valid tarball, result verifies, no link in the result;
  intermediate, deep, final-file, dist and `package.json` links refuse and
  write nothing; hash mismatch; unpinned tarball; an unlisted tarball file
  is not copied; linked install source. The real pinned 16.5.0 tarball,
  downloaded through the new path into a temporary kit, verifies and is
  byte-identical (129 files) to the existing pinned copy.
- **#144-B (low, [r4244998880](https://github.com/Ascendance3D/wrlforge/pull/144#discussion_r4244998880)):
  the About step count.** `about_dialog` adds six `r.step` checks, not five.
  §3.4 now says 6. No code change.

### 3.8 Independent QA HOLD — the final staged adapters (on `e3a1496`)

The independent QA of `e3a1496` gave **HOLD**. Cause: `build-ui.sh` checked
the two **source** adapters, but it did not check their **final copies** in
`dist/` (`xite-pick-adapter.js`, `xite-gizmo-adapter.js`). The copies were
made before the check, and nothing read them again.

**Reproduction (before the change).** A temporary build kit ran the
`e3a1496` `build-ui.sh` with a fake `cargo` and `wasm-bindgen`, a synthetic
X_ITE, and a test-only `cp` shim on `PATH` that changes one staged adapter.
The original adapter files were not changed. Every tampered stage built
with exit 0 and kept `dist/`:

| tampered stage | `e3a1496` build | `dist/` |
|---|---|---|
| staged pick adapter: one byte added | exit 0 | present, differs from source |
| staged pick adapter: removed | exit 0 | present, adapter missing |
| staged gizmo adapter: a symlink to a copy | exit 0 | present, symlink |
| staged gizmo adapter: version `0.0.1` | exit 0 | present, differs from source |
| pick source changed before staging (valid declaration) | exit 0 | present |

**Change.**

- `build-ui.sh` takes the SHA-256 of each adapter source **before** the
  source check. It runs the source check (unchanged), then copies, then
  installs X_ITE (unchanged), then runs the new staged check. The source
  check now runs before any adapter copy.
- `check-xite-adapters.sh --staged DIST PICK_SRC PICK_SHA GIZMO_SRC
  GIZMO_SHA VERSION` refuses unless each staged adapter is a regular file
  (not a symlink, not missing, not a directory), has the SHA-256 taken before
  the source check, and is byte-identical (`cmp`) to its repository source.
  Then it runs the same declaration check on the **staged** files. A
  symlinked `DIST` also refuses.
- Any failure in `build-ui.sh` — any step — removes `dist/` (an `EXIT`
  trap), and the script exits non-zero. `package-linux.sh` runs under
  `set -e`, so a refused build stops packaging.
- Not changed: the X_ITE pin (16.5.0), the declaration rules, the archive
  and symlink checks. No Node.js in the build.

**Negative tests.** `test-xite-guards.sh` keeps all 91 earlier cases and
adds 60 (151 in total, all pass). Part (a) runs `--staged` on a staged
directory; part (b) runs the real `build-ui.sh` in a temporary layout with
the `cp` shim and checks that each refusal comes from the staged check and
leaves **no** `dist/`:

| # | case | `--staged` | `build-ui.sh` |
|---|---|---|---|
| 1 | both staged adapters valid | ok | ok, stage = sources |
| 2 | pick adapter missing | refuse | refuse, no `dist/` |
| 3 | gizmo adapter missing | refuse | refuse, no `dist/` |
| 4 | pick adapter changed | refuse | refuse, no `dist/` |
| 5 | gizmo adapter changed | refuse | refuse, no `dist/` |
| 6 | version removed from staged pick adapter | refuse | refuse, no `dist/` |
| 7 | wrong staged gizmo version | refuse | refuse, no `dist/` |
| 8 | version only in a comment | refuse | refuse, no `dist/` |
| 9 | changed bytes, valid declaration | refuse (SHA-256) | refuse, no `dist/` |
| 10 | staged adapter is a symlink (pick; gizmo) | refuse | refuse, no `dist/` |
| 11 | source changed after its check (pick; gizmo) | refuse (SHA-256) | refuse, no `dist/` |
| 12 | stage matches both sources exactly | ok | ok |

Also: the staged declaration check runs even when the bytes match (a source
without the version refuses); a directory in place of an adapter, a
symlinked `DIST` and a malformed SHA-256 argument refuse; and the order
(SHA-256, source check, copies, staged check, success) is checked in
`build-ui.sh`. The `cp` shim exists only in the test; production code has
no hook and no delay.

### 3.9 Test inventory and count reconciliation

The main agent reported 208 application tests, 193 shared-crate tests and
86 menu steps. The independent QA reported 108, 153 and 72.

**Main agent commands** (`e3a1496` gate): `cargo test --workspace` in
`apps/desktop-tauri` and in `crates`, summed over every `test result:` line;
the menu count is the `steps` array of the `--menu` smoke report.

| application target | tests |
|---|---|
| `wrl-forge-desktop` lib | 83 (+1 ignored: `route_warning_lookup_timing`, run separately) |
| `wrl-forge-desktop` integration: `app_shell` 5, `flux_regression` 6, `scope_regression` 3, `typed_regression` 3 | 17 |
| `wrl-forge-desktop` bin (`main.rs`), `wrlforge-render`, 5 doc-test targets | 0 |
| `wrlforge-desktop-protocol` | 73 |
| `wrlforge-scene` | 25 |
| `wrlforge-ui` | 10 |
| **total** | **208** |

| shared crate | tests |
|---|---|
| `wrlforge-vrml` | 150 |
| `wrlforge-text` | 25 |
| `wrlforge-document` | 15 |
| `wrlforge-wasm` | 3 |
| 4 doc-test targets | 0 |
| **total** | **193** |

**Independent QA.** Its exact commands are not recorded in any artifact
available to this lane. Facts that are available:

- The QA worktree (`stack-review-1-independent-qa`, `e3a1496`) built the
  same 15 application and 4 shared-crate test binaries. `--list` on each
  gives the same counts as above (84 listed = 83 + 1 ignored). So no test is
  missing from the code that QA built.
- 108 = `wrlforge-desktop-protocol` 73 + `wrlforge-scene` 25 +
  `wrlforge-ui` 10: every application package **except**
  `wrl-forge-desktop` (100 tests in 5 targets).
  153 = `wrlforge-vrml` 150 + `wrlforge-wasm` 3: every crate **except**
  `wrlforge-text` 25 and `wrlforge-document` 15.
  These are the targets that cargo prints **last**, so a partial read of the
  test output (or a package filter) gives these numbers. This is an
  inference, not a recorded command.
- The menu smoke report written at 02:42 (after the main gate, in the QA
  time window) has 86 steps, all ok. No menu report or plan on this branch
  has 72 steps (an older local report, from 01:43, has 69). The
  source of 72 is **not explained** by the available artifacts.

The smaller counts are not treated as failures, and they are not treated as
passes. To end the question, `apps/desktop-tauri/gate-linux.sh` is now the
**one repeatable Linux gate**. It runs every required group and fails if any
count differs from this inventory:

| group | command (in the gate) | required |
|---|---|---|
| release Wasm + staged adapters | `build-ui.sh`; `check-xite-adapters.sh --staged`; `fetch-xite.sh --verify-copy` | exit 0 |
| desktop backend | `cargo build -p wrl-forge-desktop` | exit 0 |
| application tests | `cargo test --workspace --no-fail-fast` (`apps/desktop-tauri`) | the 15 targets above, exact counts, 0 failed, 0 filtered |
| shared crates | `cargo test --workspace --no-fail-fast` (`crates`) | the 8 targets above, exact counts, 0 failed, 0 filtered |
| X_ITE guards | `test-xite-guards.sh` | 151 pass, 0 fail |
| JavaScript adapter tests | `node --test` (3 pick-adapter files; test-only) | 48 pass, 0 fail, 0 skipped |
| ROUTE | `cargo test route_warning`; release `route_warning_lookup_timing --ignored` | 2 ok; timing runs |
| GUI | `smoke.sh --headless` menu / dock / pick / create / extrusion / move / rotscale / recovery / theme; texture; native self-test | menu 86, dock 64 + 7 + 7, pick 130, create 51, extrusion 58, move 156, rotscale 71, recovery 34 + 15, theme 82, texture 46, native 46 steps; all ok |
| source safety | every `WRONG_SOURCE_SELECTIONS` step in every GUI report | ok, 0 wrong |
| fixtures | File and Inspector smoke on the 58 repository fixtures, new and starting-SHA binary | same names; 0 verdict and 0 failing-step differences; baseline failures and timeouts listed separately |

## 4. Documentation corrections (`4e94e3d`)

- **#4.** The seventh excluded file is `x_ite.zip`. The full list comes from
  a name-by-name comparison of the 136-file XITE-16-UPGRADE-1 manifest with
  the 129-file shipped manifest: 7 excluded, 0 extra, 0 hash differences.
- **#5.** The record now has a per-commit table. The internal evidence
  (`.qa/wrlforge/xite-16-upgrade-2/`) holds full sets for `18c2fcf` and
  `c23646b`: cargo tests, `npm run check`, every GUI suite, 58-fixture file
  and Inspector smoke, packages and packaged-binary runs. Verdicts are
  identical. `c23646b` only adds the document; the binaries are
  byte-identical. Logs that do not record a commit are not used.
- **#7.** New §4.2.1 gives the as-built load order of `Layout::from_wire`:
  refuse the written shape; drop unknown IDs and choose the active tab;
  remove empty groups; collapse one-child splits and merge same-axis
  splits; renormalize sizes; validate; else use the preset with a notice.
- **#8.** The uniqueness rule is for **visible** panels.
- **#9.** Quit is the registered `app.quit` command (Ctrl+Q / ⌘Q, CloseGate);
  the macOS native item runs through `command::from_native_menu` and
  `command::run`.
- **#10.** Theme is three commands, one per theme
  (`view.theme.tokyoNight`, `view.theme.tokyoNightStorm`,
  `view.theme.tokyoNightLight`), not five themes behind one command.

Historical test reports in both documents are not changed.

## 5. Findings fixed by older commits in PR #143 (re-verified)

| # | fix | evidence on this branch (menu / dock smoke, real X input) |
|---|---|---|
| 12 | `6e790cb`: after a drop, the moved tab takes the focus | "after a drop the moved tab (Inspector) has the keyboard focus"; each of the 10 snap-guide drags also checks it |
| 13 | `6e790cb`: Delete on a focused tab focuses the group's new active tab | "Delete on a focused tab moves the focus to the tab now active in its group" |
| 14 | `d19744f`: Cut / Paste capture editor, session, selection and text before the clipboard await, and refuse a stale state | 7 clipboard-race steps: moved selection, typed edit, replaced document, failed clipboard, rapid Cut/Cut/Paste, Paste during a pending edit — no text, revision or undo change |
| 15 | `d19744f`: one UI listener validates `menu-command` with the registry and runs `command::run` | native About and Quit run once; once again after a layout change; keys outside the application menu run nothing |
| 16 | `0772939`: menu handlers stop only the keys they use | F10 / Alt toggles, Alt+V, shortcuts from a bar button and an open menu, AltGr and IME keys, in both layouts |

No working fix was rewritten.

## 6. Source safety

From the menu and dock smoke runs on this branch (an open VRML document,
real X input):

- docking with a Source selection and a scroll position: selection, scroll,
  syntax layer, revision, dirty flag, undo / redo unchanged;
- a layout change or window resize during a Move drag: the drag is
  canceled, no source edit, no undo step;
- menu activation, About and Quit with unsaved edits: the document stays
  Modified and unchanged; Quit Cancel keeps the recovery records;
- delayed Cut and Paste: stale results edit nothing and add no undo step;
- picking after docking and after the menu run: the exact node.

`WRONG_SOURCE_SELECTIONS = 0` in both runs. No lost edit, no source change.

## 7. Platform status

Linux x64 is tested (Xvfb, software rendering). Windows x64 and macOS 15+
(Apple Silicon) are **not** tested in this lane: no native system was used.
Linux does not prove the native macOS menu (About / Quit in the application
menu) or Windows behavior; those checks stay open.

## 8. Regression gate

The full Linux gate runs on the final SHA of this branch in a new clean
worktree. Its results are in the Draft PR description, not in this file, so
that the gated SHA stays the final SHA.

## 9. Suggested GitHub replies (not posted)

Threads stay unresolved until the owner decides.

- **#1 (r4242714025):** Fixed in `309a310` on `review/copilot-stack-hardening-1`. `--verify` now refuses a link in any component of a manifest path, a linked dist or `package.json`, and any manifest path with an empty, `.` or `..` component. `test-xite-guards.sh` covers final-file, intermediate, deep, outside-tree and correct-hash links; the old check accepted five of them.
- **#2 (r4242714046):** Fixed in `309a310` and hardened in the follow-up commit (§3.6). `check-xite-adapters.sh` lexes each adapter first, so only the one real `XITE_VERSIONS` declaration (and the gizmo `XITE_VERSION` line) at the IIFE top level counts; a version in a line or block comment, a string, a template literal or a nested block is refused. 43 offline adapter cases.
- **#3 (r4242714062):** Fixed in `604718b`. The diagnostics are sorted by `(start, end, code)`, so the lookup binary-searches and stops at the ROUTE end. Warnings are identical to the old lookup on generated documents and all fixtures; 44,000 ROUTEs: 612 ms → 216 ms total.
- **#4 (r4242714080):** Fixed in `4e94e3d`: the seventh file is `x_ite.zip`, confirmed by a name-by-name manifest comparison.
- **#5 (r4242714093):** Fixed in `4e94e3d`: the record now lists which runs used `18c2fcf` and which used `c23646b`. Both have full evidence with identical verdicts; `c23646b` only adds the document.
- **#6 (r4242710837):** Fixed in `0caa7b5`: Close takes the focus when the dialog mounts, before IPC; late answers are dropped; focus returns on close. Covered with a delayed `app_info` in the menu smoke.
- **#7 (r4242710854):** Fixed in `4e94e3d`: APP_SHELL_0 §4.2.1 documents the as-built normalization and the preset fallback.
- **#8 (r4242710866):** Fixed in `4e94e3d`: the rule is now "each visible panel appears exactly once".
- **#9 (r4242710880):** Quit was registered as `app.quit` in `c488c80` (PR #143); `4e94e3d` updates the contract.
- **#10 (r4242710895):** `c488c80` (PR #143) uses one command per theme (three themes); `4e94e3d` updates the contract.
- **#11 (r4242637970):** Fixed in `7ac0bcb`: the guide is the moved panel's box in the solved candidate layout. 10 real drags measured 0.00 px against the final box (old guide: up to 355 px).
- **#12 (r4242637993):** Fixed in `6e790cb` (PR #143); re-verified on the stack head.
- **#13 (r4242638014):** Fixed in `6e790cb` (PR #143); re-verified on the stack head.
- **#14 (r4243044078):** Fixed in `d19744f` (PR #143); seven stale-state race steps pass on the stack head.
- **#15 (r4243044099):** Fixed in `d19744f` (PR #143); one validated listener, re-verified.
- **#16 (r4243044118):** Fixed in `0772939` (PR #143); the F10 / Alt / shortcut steps pass on the stack head in both layouts.
- **#144-A (r4244998855):** Confirmed and fixed (§3.7): the fetch now verifies the extracted tree — including every parent-directory link — before it copies, and still verifies the copy. `--from-tarball` gives the fetch path offline tests: intermediate, deep, final-file, dist and `package.json` links in a tarball all refuse and write nothing.
- **#144-B (r4244998880):** Confirmed: six steps. The record now says 6.

## 10. Remaining limits

1. Windows and macOS are not tested (§7).
2. The X_ITE version guard accepts only the current one-line declaration
   shape, and its lexer refuses regular-expression literals (§3.2, §3.6).
   The check-then-copy window is closed: the copy is verified (§3.6).
3. Tab-strip drops show an insertion caret, not a content box (§3.5).
4. The About delay is a test hook (§3.4).
5. The temporary `fx58` fixture set is gone; fixture comparisons use the
   repository fixtures only.
6. QA HOLD fix (§3.8): the staged-adapter check trusts the local build
   filesystem between its own steps (check, hash, compare); it is a build
   guard, not a defense against a concurrent local attacker.
7. The independent QA's exact test commands are not recorded; its 72 menu
   steps are not explained (§3.9). `gate-linux.sh` fixes the inventory for
   any later run.
