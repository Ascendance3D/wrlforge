# SHELL-0 — Shell Contracts and Measurements

| | |
|---|---|
| Lane | SHELL-0 ([#128](https://github.com/Ascendance3D/wrlforge/issues/128)), foundation implementation only |
| Architecture | APP-ARCH-0, [`DESKTOP_APPLICATION_SHELL_REVIEW.md`](DESKTOP_APPLICATION_SHELL_REVIEW.md) (PR #127), §24.1 |
| Baseline | `origin/main` `957102acf4dfae4e789810231061391b83f295ba` |
| Product behavior changed | none. No page, `main.js` or `preload.js` loads the new modules (asserted by `test/shell/boundaries.test.js`). |
| Dependencies added | none |
| Repository rules changed | none. `CLAUDE.md`, `AGENTS.md`, `WD.md` unchanged; the bundling rule text in §17.4 is for a later lane to adopt. |
| Unblocks | #121 (§18), #122 (§19) |

This document records what SHELL-0 built and measured. It does not repeat
APP-ARCH-0; section references of the form "APP-ARCH-0 §n" point there.

---

## 1. Contract map

Every contract is a pure, framework-free first-party module under
`src/shell/`, with no callers yet. Each uses the repository's dual export
(CommonJS for main and `node:test`, `window.WrlShell*` for a classic page) and
is wrapped in an IIFE so a later classic-script load cannot collide on
top-level names.

| Contract | Module | Builds on | Status |
|---|---|---|---|
| Disposable lifecycle | `src/shell/disposable.js` | — | new |
| Application contribution | `src/shell/contribution.js` | `services.js`, `disposable.js` | new |
| Application-service boundary | `src/shell/services.js` | — | new (closed catalog) |
| Profile ids (data only) | `src/shell/profiles.js` | `CLAUDE.md` profiles | new |
| Command contribution | `src/shell/command-service.js` | **the UI-0 Command Registry**, unchanged | wrapper, no second registry |
| Panel contribution | `src/shell/panel-service.js` | **the UI-0 Panel Registry**, unchanged | wrapper, no second registry |
| Tool contribution | `src/shell/tool-registry.js` | `app.commands` | new |
| Contextual-panel contribution | `src/shell/contextual-panels.js` | document session authorities | new |
| Menu integration (renderer half) | `src/shell/menu-boundary.js` | UI-0 §10.3, the Command Registry | new, no IPC |
| Document session | `src/shell/document-session.js` | CodeMirror handle, `sceneSelection` | new |

**Existing production modules changed:** none. `src/editor/command-registry.js`,
`src/editor/panel-registry.js`, `src/editor/workspace-presets.js`,
`src/editor/scene-selection.js` and `renderer/command-bindings.js` are
byte-identical to the baseline. The only non-new file touched is
`scripts/run-tests.js`, which gains `test/shell` in its directory list.

### 1.1 Ownership map

```text
main EditorController / EditorSession ── path, format, gzip, authorization, sessionId
                                         (unchanged; never enters the renderer contracts)

renderer application (Shell-2 builds it; SHELL-0 defines its parts)
├─ createApplication({ services })               contribution.js
│   ├─ app.commands  ─ command-service ─▶ UI-0 Command Registry (the one registry)
│   ├─ app.panels    ─ panel-service   ─▶ UI-0 Panel Registry   (the one registry)
│   ├─ app.tools            tool-registry       (records over commands)
│   ├─ app.contextualPanels contextual-panels   (mount/dispose by proven selection)
│   ├─ app.documents        document-session    (one DocumentSession at a time)
│   ├─ app.workspaces       boundary only       (#121 wraps setWorkspaceMode)
│   ├─ app.dialogs          boundary only       (#121: isModalOpen)
│   └─ app.preferences      WrlPreferences      (unchanged)
├─ per contribution: a scoped `app` + its own disposable store
└─ DocumentSession
    ├─ CodeMirror handle   source text + undo   (read through, never copied)
    ├─ sceneSelection      selection            (the same object)
    ├─ analysis products   derived, replaced wholesale
    └─ preview reference   derived projection (X_ITE)
```

## 2. Application contribution contract

```js
// a feature module
module.exports = {
  id: 'workspace.commands',
  contribute(app) {
    app.commands.register({ id: 'workspace.reset', area: 'workspace', label: 'Reset Workspace', run });
    app.disposables.add(listen(window, 'resize', onResize));
  },
};

// the shell (Shell-2)
const application = createApplication({ services: { commands, panels, tools, documents, ... } });
const handle = application.contribute(require('./workspace-commands'));
handle.dispose();          // or application.dispose() at teardown
```

| Rule | Behavior | Test |
|---|---|---|
| Scoped `app` | Each contribution receives a frozen view of the services. Calls to methods the catalog marks `tracked` (`register`, `subscribe`) are owned by the contribution's store. | `contribution.test.js` |
| No discarded handles | A contribution that ignores the return value of `register`/`subscribe` still has it cleaned up. | "unretained handles are still owned" |
| Retained handles | The returned handle still works; calling it early also releases it from the store, so nothing is cleaned twice. | "retained handle" |
| Atomic | `contribute()` is synchronous. If it throws, or returns a promise, everything it registered is disposed before the error propagates (`ECONTRIBUTION_FAILED`, `ECONTRIBUTION_ASYNC`). | "rolled back atomically" |
| Unique | Contribution ids follow `area.name`; a live duplicate is `ECONTRIBUTION_DUPLICATE`. The same id may contribute again after disposal. | "invalid and duplicate" |
| Ordering | `application.dispose()` disposes contributions in reverse order, then refuses new ones (`EAPP_DISPOSED`). | "reverse order" |

There is no dependency-injection container and no state-management package.
The shell constructs each service once and passes instances in.

## 3. Disposable lifecycle contract

`src/shell/disposable.js`: `toDisposable`, `asDisposable`,
`createDisposableStore`, `listen`.

| Property | Rule |
|---|---|
| Shape | A disposable is `{ dispose() }`. A bare function (today's `unregister`/`unsubscribe`) is accepted at the boundary and wrapped. Anything else is `EDISPOSABLE_INVALID`: an attachment with no cleanup handle is a contract violation. |
| Ownership | The store's owner (a contribution, a mounted panel, a document session) disposes it exactly when the thing it represents ends. |
| Repeat disposal | Idempotent. A second `dispose()` is a no-op. |
| Order | Reverse registration order, so a later attachment that may depend on an earlier one goes first. Nested stores dispose children-first. |
| Failures | Every child is disposed even if some throw; the errors are re-thrown afterwards — the error itself when one child threw, or one `AggregateError` (`EDISPOSABLE_FAILED`) whose `errors` holds **every** failure in disposal order when several did. No later failure is dropped. The store is disposed and empty afterwards either way. |
| Late attachment | `add()` after disposal disposes the newcomer immediately, so it cannot outlive its owner. |
| Early release | `release(d)` disposes one child and forgets it. |
| DOM | `listen(target, type, fn)` returns the `removeEventListener` as a disposable. |

Leak tests: 25 `listen()` attachments and 25 contributions that each attach
selection, preference, registry and DOM subscriptions all return their
listener counts to the baseline after disposal.

Existing renderer modules are **not** migrated (the discarded handles in
APP-ARCH-0 §6 remain until Shell-2 adopts this contract).

## 4. Document-session boundary

`src/shell/document-session.js` defines the renderer `DocumentSession` of
APP-ARCH-0 §15. It is a controller over existing authorities, not a model:

| Concern | Authority | What the session does |
|---|---|---|
| Source text | CodeMirror handle | `text()` reads `handle.getText()` on every call. Nothing is cached. |
| Undo | CodeMirror `history()` | `history()` reads `handle.historyDepth()`. There is no undo, redo or stack in the session. |
| Source edits | `handle.applyVerifiedEdits` | the one edit path, delegated unchanged |
| Selection | `sceneSelection` | `session.selection` **is** the controller object |
| Path, gzip, authorization | main `EditorSession` | not present. Only `sessionId` (from main's `describe()`) is held, for `isCurrent(id)`. |
| Profile | `context` at open | fixed at construction (`'mall' \| 'world' \| 'generic'`) |
| Analysis | derived | `setAnalysis(products)` replaces wholesale; dropped on dispose |
| Preview | X_ITE projection | an optional reference, disposed with the session |
| Workspace, `sourceOpen`, layout | shell workspace service | refused |

**Why a second canonical document cannot appear.** It is enforced in code,
not by convention:
- the constructor accepts exactly `sessionId`, `profile`, `editor`,
  `selection` and `preview`. `text`, `source`, `buffer`, `baseline`, `undo`,
  `history`, `sceneGraph`, `ast`, `path`, `sourcePath`, `workspaceMode`,
  `sourceOpen` and `layout` are refused by name (`EDOCSESSION_FORBIDDEN`), and
  any other key is refused too;
- the session object is frozen and has no `setText`, `setDoc`, `undo`,
  `redo`, `serialize`, `save` or `path`;
- a test types into the handle and proves the session's enumerable state and
  its JSON never contain the text, while `text()` returns it.

`dispose()` disposes session-owned attachments, the preview reference and
the CodeMirror view (the session owns the view for the document's lifetime,
APP-ARCH-0 §15). After disposal `text()` throws `EDOCSESSION_DISPOSED`.

`createDocumentSlot()` is `app.documents`: one current session, and opening
a new one disposes the previous one (today's one-document rule).

**Deferred to Shell-2:** who owns the saved baseline and therefore the dirty
flag. Today `S.baseline` in `editor.js` and main's `EditorSession` hold it;
the session contract deliberately refuses it rather than add a third holder.

> SHELL-0 does not settle the final dirty-baseline owner.
> Shell-2 must resolve that ownership without creating a second canonical source or undo model.

The `baseline` refusal is a SHELL-0 guard against a third, unreviewed copy —
not a ruling that dirty state can never involve the session. Shell-2 may change
the option list when it settles ownership, subject to that rule.

## 5. Application-service boundary

`src/shell/services.js` is a **closed catalog**. `createApplication` rejects an
unknown name (`EAPP_SERVICE_UNKNOWN`), a deferred one
(`EAPP_SERVICE_DEFERRED`), a missing required one (`EAPP_SERVICE_MISSING`) and
an instance without its declared surface (`EAPP_SERVICE_INVALID`).

| Service | Status | Owner | Surface |
|---|---|---|---|
| `commands` (required) | existing | UI-0 Command Registry via `command-service.js` | register, has, get, list, isEnabled, isChecked, execute, keyBindings, subscribe, invalidate, profilesOf |
| `panels` (required) | existing | UI-0 Panel Registry via `panel-service.js` | register, has, get, list, isVisible, focus, show, hide, mount, unmount, isMounted, notifyResize |
| `tools` | SHELL-0 | `tool-registry.js` | register, has, get, list, state, activate, subscribe |
| `contextualPanels` | SHELL-0 | `contextual-panels.js` | register, reconcile, mounted, watch, dispose |
| `documents` | SHELL-0 | `document-session.js` | current, open, close, subscribe |
| `workspaces` | boundary | `setWorkspaceMode` + `workspace-presets.js`, wrapped by #121 | mode, preset, setMode, subscribe |
| `dialogs` | boundary | dialog stack, #121 (D3) | isModalOpen |
| `preferences` | existing | `WrlPreferences` | get, set, subscribe |

**Named by APP-ARCH-0 §16 but not admitted in SHELL-0** (passing one is an
error, so `app` cannot quietly become a god-object):

| Name | Why not now |
|---|---|
| `keyboard` | `installKeyboard` is already the one dispatcher; #121 adds the `dialogs.isModalOpen()` gate inside it |
| `preview` | `PreviewSession` and profile adapters are Shell-2; the session holds only a reference |
| `files` | a thin façade over `window.vrmlpad.editor.*` is Shell-2; main stays the path authority |
| `validation` | profile-contributed; a shared service would invite Mall rules into shared code |
| `status`, `notifications` | one status bar and one `aria-live` service are Shell-3 |
| `menu` | main-process (#122); the renderer half is `menu-boundary.js` over `app.commands` |

## 6. Command contribution boundary

`createCommandService(registry, { getProfile })` is `app.commands`. It stores,
executes and paints through the **one** UI-0 registry and adds only:

- **`profiles`** (optional array of profile ids). An unlisted command is
  profile-neutral. A listed one is folded into the record's own `enabled`, so
  `isEnabled`, `execute`, the toolbar painter and the future menu all see the
  same answer. With no document (`getProfile() === null`) a profile-limited
  command is disabled. The record's own `enabled` is consulted only inside
  its profiles (`profileAllowed && originalEnabled()`).
- **`profiles` never weakens registry validation.** `enabled` must still be
  `undefined` or a function. Any other value (`false`, `true`, a string,
  `null` …) is rejected with the registry's own `ECOMMAND_INVALID`, the same
  message a profile-neutral record gets; it is never reinterpreted as
  "always enabled", and nothing — command or `profilesOf` entry — is left
  registered. Duplicates remain the registry's `ECOMMAND_DUPLICATE` and never
  replace the first record's profiles.
- **`profilesOf(id)`** for menu/toolbar presentation.

Profile **containment** is data; no profile **rule** is in shared code
(`profiles.js` holds the three ids only; `boundaries.test.js` asserts no
`src/shell` module requires `validator.js`, `src/mall`, `src/world-project`,
`src/vrml`, `fs` or Electron). Command ids, the record shape, key ownership,
shortcut normalization and the enabled/checked pull model are unchanged. A
command registered through a contribution is removed when the contribution is
disposed. No current command was moved; #121 commands are not migrated here.

## 7. Panel contribution boundary

`createPanelService(registry)` is `app.panels`. Records with `element()` pass
straight through, so the existing editor panels need no change. A record with
`mount` is **mountable**:

```js
{ id, title, mount(host), dispose(), resize?(), focus?(), canToggle?, show?(), hide?() }
```

| Concern | Rule |
|---|---|
| Identity | stable panel ids, validated by the UI-0 registry (reserved `console` still refused) |
| Mount | one active mount at a time: the same host again is a no-op; another host while mounted is `EPANEL_MOUNTED`; a throwing `mount` leaves it unmounted, with no `dispose()` (mount must not leave partial resources when it throws), and may be retried |
| Element | a mountable panel's `element()` is its host after mount, `null` before |
| Visibility | still derived from the element on every read (UI-0 §11.2); an unmounted panel is not visible |
| Hidden but docked | stays mounted (CodeMirror and X_ITE are not re-created on a tab switch) |
| Dispose | `panels.unmount(id)` calls `dispose()` exactly once per mount; a repeated unmount is a no-op. State is cleared before `dispose()` runs, so a throwing dispose (whose error propagates) still leaves the panel unmounted. Unregistering a mounted panel unmounts it first; a repeated unregister is a no-op |
| Remount | disposing one mounted instance never spends the registration: after an unmount, `mount(id, host)` with the same or a different host is a fresh `mount()` call with fresh resources |
| Focus | `panels.focus(id)` stays the only focus path (UI-0 D6); it never opens a panel and reports `hidden` for an unmounted one |
| Resize | `panels.notifyResize(id)` forwards a dock engine's resize to the panel |
| Workspace visibility | unchanged: CSS + workspace classes decide what renders; presets stay semantics |

No docking engine is selected or integrated (UI-C0 #38). No panel moved.

Tested lifecycle (`test/shell/command-panel-services.test.js`): register →
mount(host A) → hide while docked (stays mounted, focus refuses) → show →
focus → host change refused → unmount (listeners back to zero, repeat safe) →
mount(host B) → unmount → mount(B) again → unregister while mounted (disposed,
repeat safe). Each mount attaches its own listeners and the test asserts none
survive into the next. Also: a throwing dispose leaves the panel remountable;
a failed mount can be retried.

## 8. Tool contribution boundary

`createToolRegistry({ commands, getProfile, getSelectionInfo })` is
`app.tools`:

```js
app.tools.register({ id: 'tool.move', commandId: 'model.move', group: 'transform',
  label?, icon?, profiles?: ['world'], appliesTo?: (sel) => sel.type === 'Transform' });
```

- Groups are fixed to APP-ARCH-0 §18: `select`, `transform`, `create`,
  `appearance`, `hierarchy`, `behavior`, `animation`. A test registers all 16
  example tools (Select … Keyframe) as records only.
- A tool has **no behavior and no enabled state of its own.** `activate` is
  `commands.execute(commandId)`; `state(id)` is
  `{ available, enabled, checked }`, where `enabled`/`checked` are the
  command's.
- **Available** = command registered ∧ profile listed (if any) ∧
  `appliesTo(selection)` on a **proven** selection. Without a proven
  selection a tool that declares `appliesTo` is unavailable (fail closed,
  `WD.md` §7). A throwing `appliesTo` is unavailable.
- The command must exist when the tool registers (`ETOOL_INVALID`).
- `list(group)` returns registration order, which is toolbar order;
  `subscribe` fires when the set changes. Painting follows
  `commands.subscribe`, exactly like `[data-command]` controls.

No tool is implemented. Tool-specific behavior stays in its command, through a
pure planner → `applyVerifiedEdits` (APP-ARCH-0 §18).

## 9. Contextual-panel boundary

`createContextualPanelHost({ resolveContext, createHost, releaseHost })` is
`app.contextualPanels`:

```js
{ id, title, appliesTo(selectionInfo, analysis), mount(host, ctx), update?(ctx), dispose() }
```

- `resolveContext()` returns `{ selection, analysis }` from the document
  session's **existing** authorities (`sceneSelection` + the analysis
  products). The host never parses, never builds a scene tree and keeps no
  selection of its own.
- `reconcile()` answers, per contribution, *"does this apply to the current
  proven selection?"* and then mounts the newly applicable ones, calls
  `update` on those still applicable, and disposes those that no longer apply.
- **Fail closed:** no context, no selection, or `selection.proven !== true`
  applies nothing and disposes every mounted editor. A throwing `appliesTo`
  does not apply (a mounted editor is disposed); a throwing `mount` is
  reported in `failed`, its host is released and no active state remains; a
  throwing `update` disposes that editor rather than leave it showing stale
  state; a throwing `dispose` still clears the active state and releases the
  host.
- **Host callbacks are isolated like records.** A throwing `createHost(id)`
  leaves that record inactive (nothing to clean: no host was handed out) and
  reconciliation continues. A throwing `releaseHost` — after an unmount, a
  failed mount, a failed update, a failed dispose, `unregister` or host
  `dispose()` — never stops the remaining cleanup. A throwing
  `resolveContext()` is treated as **no context**: every mounted editor is
  disposed, nothing is mounted, `update` is never called, and the result
  carries `contextFailed: true`. A context exception is never turned into a
  guessed or stale context.
- **No failure is discarded.** `reconcile()` returns `{ mounted, updated,
  disposed, failed, errors, contextFailed }`; `errors` lists every failure as
  `{ id, phase, error }` (`id` null for `resolveContext`; phases
  `resolveContext`, `createHost`, `mount`, `releaseHost`, `update`,
  `unmount`) in registration order, then the order they happened. When an
  editor's `dispose` **and** its `releaseHost` both throw, both are attempted
  and both are kept in one `AggregateError` (`code`
  `ECONTEXTUAL_CLEANUP_FAILED`, dispose error first) — the
  `EDISPOSABLE_FAILED` shape, not a second error system.
- One record's (or one callback's) failure never stops the others from
  reconciling, and a later reconcile after the fault is removed is a normal,
  fresh one.
- **Re-activation is a fresh mount.** After a dispose, the next applicable
  proven selection gets a new `createHost(id)` and a new `mount(host, ctx)`;
  nothing from the previous activation is handed back.
- `dispose()` disposes every mounted editor even when several throw, and
  re-throws (one error, or one `AggregateError`).
- `watch(sceneSelection.subscribe, analysisNotifier)` wires reconcile to the
  change sources and returns one cleanup.

Tested lifecycle (`test/shell/tools-contextual.test.js`): no selection →
proven match → mount → update → non-match (disposed, subscriptions back to
zero) → matching type but unproven (nothing) → proven match again (a second
mount on a new host) → dispose.

`selectionInfo` is supplied by the document session (Shell-2). Its contract is
`{ id, type, kind, proven }`, where `proven` means the identity is
source-proven by the WD1.4 rules. The current Object panel
(`model-workspace.js`) is the expected first adopter; no editor is implemented
here.

## 10. Menu integration boundary

`src/shell/menu-boundary.js` is the renderer half of UI-0 §10.3 / APP-ARCH-0
§20. **No menu, IPC channel or preload API is added.**

| Step | Contract |
|---|---|
| Menu-addressable | `createMenuCommandSet(ids)`: a frozen allow-list of valid, unique command ids. Being registered is not enough; an id is data. |
| Dispatch | `dispatchMenuCommand(registry, menuSet, id)` → `{ ok, reason? }`. Refuses `not-allowed`, `unregistered`, `no-registry` (a page without a registry before Shell-4: dropped and logged) and `disabled` (through the registry, including out-of-profile commands). Runs with `source: 'menu'`, the same path as toolbar and keyboard. Never throws for a bad id. |
| State | `menuStateSnapshot` / `subscribeMenuState(registry, menuSet, send)`: pulled from `isEnabled`/`isChecked`, sent once and then only on change, returned as a disposable. With no registry: one all-disabled snapshot, nothing owned. |
| Per-page | the subscription is installed per page load until Shell-4 (APP-ARCH-0 §20 amendment 2); main still disables editor-only items when `currentPage !== 'editor'`. |

## 11. Baseline methodology

**Scope.** Every startup, page-switch and memory figure in §12–§14 is a
**repeatable SHELL-0 QA baseline**: measured through the established
visual-QA/capture environment on one Linux/X11 machine (Ryzen 9 5900X, Node
v24.21.0, Electron 41.7.1), in capture-server QA mode, with `--no-sandbox`
(production launches are sandboxed), a warm OS page cache, and small fixtures.
They are **not** universal end-user performance claims. A future comparison
must use equivalent methodology (this harness, unchanged switches, comparable
machine and fixtures) or state explicitly how and why it differs.

`RESULTS.json` is **evidence, not a golden file.** No test or CI job compares
it byte-for-byte or numerically; a re-run on another day or machine produces
different numbers and that is expected. It carries the method, machine,
versions, fixtures, every raw sample and the runner log. It holds no absolute
paths, temp directories or usernames; raw OS process ids were replaced by
file-wide ordinals (`p1`, `p2`, …) that keep the process-tree links and the
renderer-persistence evidence (`method.pidPolicy`).

`qa/shell-0-baseline/measure.js`, results in
`qa/shell-0-baseline/RESULTS.json` (summary, every raw sample, and the runner
log).

| | |
|---|---|
| Harness | `VisualQaRunner` + the existing `WRL_FORGE_CAPTURE_SERVER` mode, under the visual-QA lock and `guardWindowsWorkspace`. Jobs use the capture server's existing `world`, `keyboard.goto` (the same `loadFile` + `currentPage` update `gotoPage` performs) and `keyboard.executeJS` kinds. **No product code or capture-server code changed.** |
| Profile | a fresh temp `--user-data-dir`; scratch fixtures under the OS temp dir (`test/fixtures/valid-gzip.wrl`, `test/fixtures/world/valid70`) |
| Switches | `--no-sandbox` (as every visual-QA orchestrator), plus measurement-only `--enable-precise-memory-info` and `--js-flags=--expose-gc` |
| Clock | `performance.timeOrigin + performance.now()` in both the harness and the renderer (epoch ms, same machine) |
| Startup | 12 sequential launches, one process each, 1.5 s cooldown; statistics exclude the first launch, which is reported separately |
| Page switch | one process; 25 World → Editor → World and 25 Mall → Editor → Mall round trips. A leg is timed from sending the `goto` job to the moment the destination page is usable, polled at 5 ms inside the page: editor `__wrlEditor.ready()`; World load complete and its scan state (`current`) present; Mall load complete. |
| Memory | after each leg: 1 s settle (3 s at startup), two forced GCs, `performance.memory`; then `VmRSS` and `Pss` for every process in the Electron tree from `/proc`. Zygote-forked children keep the zygote's argv, so roles come from the tree: children of the `--no-zygote-sandbox` zygote are the GPU process; children of the plain zygote are renderers; the largest is the page renderer, the other Chromium's spare. |
| Machine | Linux 7.0.0-34-generic x64, X11; AMD Ryzen 9 5900X (24 threads), 63 GB; Node v24.21.0; Electron 41.7.1 |

Reproduce: `node qa/shell-0-baseline/measure.js [--launches=12] [--cycles=25]`
(about 8 minutes on this machine; needs a display).

## 12. Startup baseline

Starting page: **`index.html` (Mall)** on every launch.

| Launch → … (11 launches after the first) | median | p10–p90 | IQR |
|---|---|---|---|
| navigation start (process boot) | 250.9 ms | 244.1–255.1 | 9.3 |
| **Mall page usable** (DOMContentLoaded end; all `defer` scripts have run) | **1,091.3 ms** | 1,074.0–1,110.2 | 31.8 |
| load event end | 1,092.4 ms | 1,075.2–1,111.4 | 31.9 |
| capture-server READY line (`did-finish-load` in main) | 1,095.5 ms | 1,078.1–1,114.5 | 31.9 |
| navigation start → usable, in the renderer | 839.7 ms | 822.7–855.9 | 21.6 |

First launch of the run: 1,127 ms to usable.

| After 3 s stabilization (Mall page, no item open) | median | p10–p90 |
|---|---|---|
| renderer JS heap (after GC) | 6.8 MB | 6.8–6.8 |
| main RSS | 167.0 MB | 166.4–167.7 |
| page renderer RSS / PSS | 130.6 / 80.7 MB | 130.1–131.3 / 80.3–81.1 |
| whole Electron tree PSS | 284.4 MB | 283.8–284.7 |

Most of the start is document load, not process boot: about 840 ms of the
1.09 s is the Mall page parsing and running its scripts, dominated by the
1.3 MB X_ITE classic script.

## 13. Page-switch baseline

Every leg is a **full document load**: 100 of 100 legs started a new
navigation (a new `timeOrigin`), so a round trip is **two renderer reloads**.
The page renderer **process** is reused: one page-renderer PID served all 50
round trips; there is no process swap.

| Leg (25 each) | median | p10–p90 | max | navigation → load |
|---|---|---|---|---|
| World → Editor | 296.4 ms | 245.8–312.0 | 535.6 (first) | 183.5 ms |
| Editor → World | 111.5 ms | 108.5–117.0 | 157.6 | 103.6 ms |
| Mall → Editor | 206.6 ms | 197.4–214.9 | 362.5 (first) | 181.4 ms |
| Editor → Mall | 101.7 ms | 96.8–106.5 | 114.9 | 98.6 ms |

| Round trip (sum of legs + the page's open IPC) | median | p10–p90 |
|---|---|---|
| **World → Editor → World** | **408.5 ms** | 360.3–433.1 |
| **Mall → Editor → Mall** | **319.9 ms** | 311.6–332.4 |

Request → navigation start is about 1 ms; the rest is document load and
initialization. The page's own IPC before the switch is
`editor.openWorldPrimary` 0.6 ms and `editor.openMall` 13.5 ms (medians).

**State after the switch:**

| | Result |
|---|---|
| Editor has its document | 50/50 |
| World page rehydrated from `world:describe` | 25/25 |
| **Mall page restored its item** | **0/25 — the known Mall rehydration gap is confirmed.** Main still holds `currentSession`, but the page comes back blank; the user had to re-open the item before 24 of 25 round trips (the first used the initial open). That manual re-open is excluded from the timings above. |

`mall:describe` was not added (Shell-4).

## 14. Memory baseline

**Stabilized pages** (1 s settle, two GCs):

| Page | JS heap | page renderer RSS / PSS | main RSS | tree PSS |
|---|---|---|---|---|
| Mall, fresh launch, no item (§12) | 6.8 MB | 130.6 / 80.7 MB | 167.0 MB | 284.4 MB |
| World, fresh process, `valid70` open | 14.5 MB | 182.2 / 126.5 MB | 183.0 MB | 366.2 MB |
| Editor, World document, first visit | 29.4 MB | 223.7 / 167.3 MB | 186.1 MB | 414.6 MB |
| Mall, item open (median of the last 5 soak samples) | 12.0 MB | 192.4 MB RSS | 193.5 MB | 403.6 MB |
| Editor, Mall document (last 5 soak samples) | 14.6 MB | 196.0 MB RSS | 194.0 MB | 407.9 MB |

**Soak** (25 round trips per profile, one process; first-5 vs last-5 sample
medians and the least-squares slope per round trip):

| Series | JS heap after GC | page renderer RSS | tree PSS | main RSS |
|---|---|---|---|---|
| World page | 38.2 → 38.2 MB (slope 0, r² 0.17) | 283.7 → 288.9 (+0.4, r² 0.11) | 476.1 → 498.4 (+1.2, r² 0.42) | 189.2 → 195.0 (+0.2, r² 0.08) |
| Editor, World document | 40.7 → 40.7 MB (0, r² 0.01) | 290.7 → 291.4 (+0.8, r² 0.14) | 485.2 → 502.0 (+1.5, r² 0.38) | 188.9 → 194.3 (+0.2, r² 0.10) |
| Mall page | 32.1 → 12.0 MB (−0.8, r² 0.57) | 271.5 → 192.4 (−4.2, r² 0.84) | 481.6 → 403.6 (−4.2, r² 0.86) | 197.3 → 193.5 (−0.1) |
| Editor, Mall document | 34.7 → 14.6 MB (−0.9, r² 0.62) | 275.5 → 196.0 (−4.3, r² 0.85) | 487.2 → 407.9 (−4.3, r² 0.87) | 197.4 → 194.0 (0) |

**Classification** (no leak is claimed from a rising sample):

- **Retained growth: none demonstrated.** The JS heap after GC is flat
  across the 25 World round trips and falls across the Mall ones.
- **Reload / detached-context effect.** After the World phase the heap sits
  about 24 MB above a fresh World page (14.5 → 38 MB) even after forced GCs,
  and falls back only after about seven more navigations (the Mall-page series
  reads 32 MB seven times, then 12–17 MB). Earlier documents' contexts outlive
  a forced GC for several navigations in the reused renderer process. This is
  a cost of reloading pages, not of any one page.
- **Allocator retention.** Tree PSS rises during the World phase and
  plateaus (about 498 MB for the last seven samples), then releases about
  80 MB during the Mall phase: a high-water mark, not linear growth.
- **Main process.** RSS is a sawtooth (183 → 197, a drop to 182, up to 200,
  ending at 194 MB): garbage-collected, with a net drift of about +11 MB over
  50 round trips. This is **not** classified as a leak: it **needs a longer
  soak before classification**. At this run length it cannot be separated
  from allocator/GC behavior (§21).

No retained renderer growth was proven in the measured soak.

## 15. Reproducing

| Measurement | Command | Output |
|---|---|---|
| Startup, page switch, memory | `node qa/shell-0-baseline/measure.js` | `qa/shell-0-baseline/RESULTS.json` |
| Module spike build | `node spikes/shell-0-module-loading/build.js` | `spikes/shell-0-module-loading/out/BUILD.json` (gitignored) |
| Module spike run | `node spikes/shell-0-module-loading/run.js` | `spikes/shell-0-module-loading/out/RESULTS.json` (gitignored) |

Both Electron runs take the visual-QA lock, so they never overlap each other
or any other visual QA.

## 16. Native ES-module `file://` spike

`spikes/shell-0-module-loading/` (see its `NOTES.md` for every probe). One
**sandboxed** Electron 41.7.1 process with WRL Forge's renderer settings and
the editor page's CSP.

| Question | Answer |
|---|---|
| Does `type="module"` load from `file://`? | Yes. Relative static imports, `.mjs`, dynamic `import()` and top-level `await` all work. |
| Module origin | `"file://"`; `import.meta.url` is the real file URL |
| CSP | `script-src 'self' file:` and a strict `script-src 'self'` both allow it with no violation. Inline modules are blocked, as inline classic scripts are. |
| Electron 41 / sandbox | works with the default sandbox, `contextIsolation`, no `nodeIntegration` |
| Packaging | works from inside an `asar` |
| Error reporting | a top-level throw reports file, line and column. **A missing import is silent in the console**: only an element-level `error` event fires. A bare specifier fails with a clear `TypeError`. |
| Debugging | stack traces show the original file and line; no source map needed |
| Shared `src/` modules | importable for their side effect (the `window.*` global), **not by name**: CommonJS modules have no ES exports (`SyntaxError`) |
| Testability | `node:test` can import ES modules, but the shared modules must stay CommonJS for main |

Native ESM is reliable in Electron 41 under the product's constraints.

## 17. esbuild comparison and bundling recommendation

### 17.1 Comparison

| Criterion | Native ESM | esbuild entry bundles |
|---|---|---|
| CSP | no change needed | no change needed (local files, no `eval`) |
| `file://` loading | works (§16) | works (a classic `defer` script, as today) |
| Shared CommonJS `src/` | **cannot import by name** — `src/` would have to become dual-format ES modules or keep the `window.*` globals | consumes CommonJS directly; `src/` unchanged |
| Source maps | not needed | local linked `.map` files; no CSP change; applied by DevTools, `Error.stack` shows bundle lines |
| Debugging | original files in stacks | original files in DevTools via the map |
| Failure diagnosis | missing import silent in the console | missing import is a **build error** |
| Testability | pure modules unchanged; ESM files need `import()` in tests | pure modules unchanged; add "each entry builds, no Node built-in reachable" (APP-ARCH-0 §23) |
| Determinism | no build output | byte-identical across three builds **from the same build root** (emitted path comments depend on the working directory) |
| Packaging | many files in `asar` | one file per entry in `asar`; already built in release CI |
| Load time, 42-file / ~330 KB graph | 33.5 ms (p10–p90 30.8–34.3) | **11.6 ms** (10.9–13.5); classic tags today 32.4 ms |
| Build overhead | none | 15 ms for the graph; `npm run build:editor` already runs before start and in CI |
| Emitted entry files | every module (27+ on the editor page) | one per page entry: three now, one after Shell-4, plus the existing CodeMirror bundle |
| Developer complexity | no build step; module graph explicit | one more esbuild entry in the existing script; module graph explicit |

**What these load figures are.** They come from a **synthetic** 42-module,
~330 KB graph sized like the editor page — not from the actual current WRL
Forge renderer scripts — and X_ITE is **outside** that comparison entirely.
The ~2.9× ratio (11.6 vs 33.5 ms) supports the *architecture selection*; it is
**not** a predicted or guaranteed speedup for real WRL Forge page loads, which
are dominated by work this spike did not model.

Context for the load figures: the page switches in §13 take 100–300 ms, and
about 840 ms of start-up is the Mall page's scripts, dominated by X_ITE. X_ITE
stays a classic vendor script under either option. Bundling saves about 21 ms
per editor-sized load; the larger win is removing reloads (Shell-4).

### 17.2 Recommendation

**`ESBUILD_ENTRY_BUNDLES`**

1. **The shared code is CommonJS, and must stay so.** Main and `node:test`
   `require` `src/` directly. Native ESM cannot import those modules by name,
   so it would force a dual-format conversion of `src/` or keep the `window.*`
   globals the shell is meant to retire. esbuild consumes them as they are.
2. **Failures surface at build time.** A missing import in native ESM is
   silent in the console.
3. **It loaded measurably faster** in the synthetic graph: one bundle vs 42
   files (11.6 vs 33.5 ms). Supporting evidence for the choice, not a product
   speedup claim (§17.1).
4. **No new tool.** esbuild is an existing devDependency, its output is
   deterministic for a fixed build root (`absWorkingDir`), and the build
   already runs before start and in release CI.

**Determinism scope.** The bundle is byte-identical across builds for a
**fixed build root / `absWorkingDir`**. It is not byte-identical regardless of
invocation directory: esbuild's emitted path comments are relative to the
working directory, so launching the spike's `build.js` from a different
directory changes the bytes (found in independent QA of PR #129). A build
script that wants reproducible output passes an explicit `absWorkingDir` (the
repository root) rather than inheriting `process.cwd()`.

Native ESM remains a technically valid fallback; nothing in the contracts
depends on the choice (the contract modules are classic-script safe today and
bundle unchanged).

### 17.3 Not activated

SHELL-0 changes **no** repository rule. The binding rule is still
`CLAUDE.md`: *"Renderer UI is plain HTML/CSS/JS (plus the editor's esbuild
bundle); no framework or bundler without separate approval."* No new
renderer entry point uses esbuild in this lane.

### 17.4 Rule text for the later approved lane

Proposed replacement for that `CLAUDE.md` bullet (APP-ARCH-0 §12 text with
the SHELL-0 evidence folded in):

> Renderer UI is plain HTML/CSS/JS without a UI framework. **esbuild** (already
> an approved devDependency) is the one bundler. It may bundle first-party
> renderer entry points into local IIFE files under `renderer/`, with local
> linked source maps, built by an npm script and not committed. No other
> bundler, no CDN, no runtime dependency added by bundling, no Node built-in
> reachable from a renderer bundle, and no transform that requires loosening
> the CSP. Bundle output must be deterministic for a fixed build root: the
> build script passes an explicit esbuild `absWorkingDir` (the repository
> root) rather than inheriting the invocation directory. Shared `src/` modules stay
> CommonJS so main and `node:test` keep requiring them directly. Vendor
> runtimes (X_ITE) stay classic script tags.

## 18. What #121 receives

#121 builds against these contracts and does not wait for any later stage.

| #121 need | SHELL-0 contract |
|---|---|
| New commands outside `editor.js` (Play, `workspace.reset`, `panel.*.focus`) | feature modules with `contribute(app)`: e.g. a workspace-commands and a panel-commands contribution, registering through `app.commands` (§2, §6) |
| Every new subscription retained | the contribution's scoped `app` owns `register`/`subscribe` handles; `app.disposables` takes DOM listeners via `listen()` (§3) |
| D3 dialog key blocking | the `dialogs` boundary: `{ isModalOpen() }`. #121 implements it and gates `installKeyboard` on it, replacing per-dialog `classList` checks (§5). |
| Workspace focus / reset / Play | the `workspaces` boundary: `{ mode, preset, setMode, subscribe }`, wrapping the existing `setWorkspaceMode` + `workspace-presets.js` authority. Play's hidden-panel set is preset **data** (APP-ARCH-0 §25). |
| `panel.*.focus` | `app.panels.focus(id)`, the only focus path (§7) |
| Command gating | the existing `enabled` pull model; `profiles` data if a command is profile-limited (§6) |

**Adoption step #121 owns:** it is the first caller. `editor.js` creates
`createApplication({ services })` over its existing registry instances
(`createCommandService`, `createPanelService`) and contributes #121's modules.
The modules are classic-script safe, so they load as `<script defer>` tags
under the current rule (update `test/editor/script-load-order.test.js`); no
bundling-policy change is needed.

## 19. What #122 receives

| #122 need | SHELL-0 contract |
|---|---|
| How a command becomes menu-addressable | listed in `createMenuCommandSet(ids)`; registered alone is not enough |
| Executing a menu id | `dispatchMenuCommand(registry, menuSet, id)` → `registry.execute(id, { source: 'menu' })`; refusals `not-allowed`, `unregistered`, `no-registry`, `disabled` |
| Pages without a registry | `registry === null` → `no-registry`, dropped and logged; a state snapshot reports every item unregistered and disabled |
| Enabled / checked state | `subscribeMenuState(registry, menuSet, send)`: the payload for the `ui:commandState` channel UI-0 §10.3(6) describes, deduplicated, returned as a disposable |
| Renderer subscription disposal | dispose per page load until Shell-4 |
| Placement | `src/main/app-menu.js` (APP-ARCH-0 §20) |

#122 still owns: the template, the preload `onCommand` API, both IPC
channels, the security review, whether the allow-list is one shared module
for main and renderer, and main's `currentPage !== 'editor'` disabling.

## 20. What later stages receive

| Stage | Receives |
|---|---|
| Shell-1a/1b | the startup and main-RSS baselines (§12, §14) to compare against after `main.js` is decomposed and hardened; the harness re-runs unchanged |
| Shell-2 | every contract in §1–§10 to adopt: `DocumentSession` replacing `S`, `editor.js` registrations moved into contributions, the discarded handles of APP-ARCH-0 §6 moved into disposable stores; the bundling rule text (§17.4) for its first entry bundle; the baseline-ownership question (§4) |
| Shell-3 | the deferred `status` / `notifications` services (§5) |
| Shell-4 | the numbers to beat: 320–410 ms per profile round trip and two reloads per trip (§13); the Mall rehydration gap (0/25); the detached-context memory effect (§14), which a reload-free shell should remove; the soak harness to re-run |
| UI-C0 / UI-1 | the mountable-panel lifecycle (§7) a dock engine calls |
| WD2-E and model tools | the tool and contextual-panel records (§8, §9) |

## 21. Known limits

- One machine and platform (Linux, X11). Windows and macOS were not measured.
- Runs use the capture-server mode, `--no-sandbox` and two measurement-only
  switches. A production launch is sandboxed and has no capture listeners.
- Not a cold-cache start: the OS page cache was warm (dropping it needs root).
- Fixtures are small (a 201-byte Mall item; the 300 KB `valid70` world).
  Larger items and worlds cost more preview time.
- Switch timing starts at the `goto` job. Button dispatch, the editor's Back
  steps (`setText`, recovery flush) and the user's Mall re-open are outside
  it; the open IPC is reported separately.
- Paint timing entries were not reported in capture mode, so "usable" is
  DOMContentLoaded end, not first paint.
- `performance.memory` covers the whole renderer isolate, including detached
  contexts of earlier documents.
- 25 round trips per profile cannot settle the main-process drift (§14).
- The spike's load figures use a synthetic graph sized like the editor page,
  not the editor's real scripts.

## 21a. Validation and visual-QA findings

Recorded as found; neither is fixed in SHELL-0, and neither is caused by it
(no product code changed).

**A. Canonical runner defect.** `npm run test:visual` does not start correctly
under the current Node 24 invocation (`node --test test/visual/`, a
directory argument). The working invocation needed explicit
test files and serialized execution:

```bash
npm run build:editor
WRL_FORGE_ALLOW_VISUAL=1 node --test --test-concurrency=1 test/visual/*.test.js
```

That invocation is a workaround for this lane, **not** a fix of the
repository's canonical script.

**B. Pre-existing Extrusion failure.** The Extrusion visual bounds case
(`qa-extrusion-scale.wrl`) fails with `bbox == null`. The same failure
reproduces on `main` at `957102ac`.

Result at the SHELL-0 candidate (2026-10-07): the canonical script exits 1
with `Cannot find module '…/test/visual'` (1 test, 1 fail); the serialized
invocation runs 20 tests, **18 pass, 2 fail** — the Extrusion case
(`TypeError: Cannot read properties of null (reading 'min')`) and its parent
group ("production preview across all fixtures in ONE reused Electron
process"), which fails only because of that subtest.

Non-blocking for SHELL-0 because product code is unchanged, the failure was
independently reproduced on the baseline, and the remaining visual smoke
cases pass under the serialized invocation. **The canonical visual suite is
not reported as a pass.**

No existing GitHub issue tracks either (searched 2026-10-07). Suggested
trackers, for owner authorization after SHELL-0:

- `[QA] Repair canonical visual test invocation on Node 24`
- `[QA] Investigate pre-existing Extrusion visual bounds failure`

## 21b. UI design references (not a contract)

Owner-supplied shell concepts live read-only at
`~/Projects/cybertown/wrlforge/assets/WRL_Forge_Shell_Concepts_1280x800`
(the owner checkout, outside Git tracking).
Future UI agents may use them as design reference material. They are **not**
copied into this branch, not implemented, and not encoded in any contract.

The contracts are **layout-neutral**: nothing in `src/shell/` assumes a
quad-view, a left toolbar, a right Inspector, a bottom-docked Source panel, or
any one concept as final. Panels are ids with a mount lifecycle; tools are
grouped records; placement belongs to the later docking/UI lanes.

## 21c. Independent-QA notes deferred to later work

Independent QA of PR #129 required two fixes (RF1 host-callback isolation,
§9; RF2 profiled `enabled` validation, §6), both made. It also recorded
non-blocking notes that are **deliberately not addressed in SHELL-0** and are
left for the lane that first exercises each contract:

- contribution-service assumptions about the cleanup handles a service
  returns;
- error reporting when a contribution's rollback cleanup itself throws;
- the breadth of the scoped service API handed to a contribution;
- nested `reconcile()` reentrancy (a reconcile triggered from inside a
  `mount`/`update`/`dispose` callback);
- wording differences between surface tables in this document;
- the visual-QA findings in §21a (unchanged).

## 22. Deferred (not started)

Persistent shell, `mall:describe`, shared chrome, `main.js` decomposition,
Electron navigation hardening (Shell-1b), renderer path-authority fixes
(SEC-SHELL-0), dirty-document close guard, the application menu (#122),
#121 workspace behavior, UI-C0 docking spike, UI-1 docking, WD2-E transforms,
geometry tools, visual redesign, any repository-rule change, and migration of
any existing caller onto the new contracts.

## 23. Files

| File | Kind |
|---|---|
| `src/shell/{disposable,contribution,services,profiles,command-service,panel-service,tool-registry,contextual-panels,menu-boundary,document-session}.js` | contract modules (no callers) |
| `test/shell/*.test.js`, `test/shell/fakes.js` | focused contract tests |
| `scripts/run-tests.js` | adds `test/shell` to the test directories |
| `qa/shell-0-baseline/measure.js`, `RESULTS.json` | baseline harness and results |
| `spikes/shell-0-module-loading/` | ESM vs esbuild spike (`out/` gitignored) |
| this document | lane record |
