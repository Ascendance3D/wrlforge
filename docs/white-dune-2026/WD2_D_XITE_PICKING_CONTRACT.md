# WD2-D X_ITE Picking Compatibility Contract

Lane: [#28 WD2-D](https://github.com/Ascendance3D/wrlforge/issues/28) ·
research sub-lane [#29 WD2-D-R](https://github.com/Ascendance3D/wrlforge/issues/29) ·
implementation [#30 WD2-D-I](https://github.com/Ascendance3D/wrlforge/issues/30) ·
independent QA [#31 WD2-D-Q](https://github.com/Ascendance3D/wrlforge/issues/31).

## Current status

**IMPLEMENTED AND MERGED.**

- Implementation #30: complete.
- Independent QA #31: complete, `WRLFORGE_WD2_D_INDEPENDENT_QA_RETEST_PASS`.
- [PR #117](https://github.com/Ascendance3D/wrlforge/pull/117) merged at
  `f39c62612d79d217c795562787d6d92764508e48`.
- X_ITE production pin: exactly `15.1.10`.

Final result, QA evidence, refusal model and known limitations:
[`WD2_D_XITE_PICKING_CLOSEOUT.md`](WD2_D_XITE_PICKING_CLOSEOUT.md).

## Design basis (pre-implementation)

The rest of this document is the architecture contract as approved before #30
began. It remains the specification #30 implemented. Statements about what
"this lane" changed, what #30 "must" do, and integration points marked "not
implemented" describe the research lane (#29) at the time of writing.

Approved by the owner for WD2-D-I. The research lane itself was contract
design only and changed no production file, dependency, test or workflow.

Owner decisions applied (2026-10-07): viewport selection does not move the
source caret; picking is armed only in the Model workspace; parser-hook
ownership is serialized across X_ITE browsers by one shared coordinator;
`abort()` releases owned hook state synchronously; `touch() === false` is
`NO_HIT` and never reads the previous hit; LF, CRLF, BOM and Unicode are
mandatory #30 verification cases.

Basis: `origin/main` `516e5641b7aadf0991cb6f86449264365b3a68a5`. X_ITE inspected:
`node_modules/x_ite` **15.1.10** (lockfile `sha512-1SIfSoIx…`). Line numbers
below (`:NNNNN`) refer to the unminified `node_modules/x_ite/dist/x_ite.js`;
production loads `dist/x_ite.min.js`, where every method name listed here
survives minification (checked by text search).

## Scope

In scope: the contract #30 must implement for *click in the X_ITE preview →
exact authored occurrence → existing selection authority, or an explicit
refusal*.

Out of scope: transform manipulation (WD2-E), multi-selection, Model/Play
architecture (UI-0), PROTO visual workflow (WD2-J), opening Inline documents,
any X_ITE upgrade, sensor-dispatch suppression (§12).

## Architecture invariants

- The exact source text is the document (`WD.md` §2). No second document, no
  canonical scene graph, no AST→text serializer, no whole-document regeneration.
- X_ITE receives the **unmodified** preview string. No injected DEF names,
  comments, metadata or wrappers.
- One selection authority: `sceneSelection` (`renderer/editor.js:121`, built by
  `src/editor/scene-selection.js` `createSelectionController`). WD2-D adds **no**
  viewport selection store.
- Fail closed (`WD.md` §7). Losing or refusing a selection is acceptable;
  selecting a different node is not. Required invariant:
  **`WRONG_SOURCE_SELECTIONS = 0`**.
- No fallback, ever, by DEF name, node type, geometry type, sibling index,
  matrix / `modelViewMatrix`, transform, geometry fingerprint, nearest or
  containing source offset, structural similarity, rendered position, or
  heuristic scoring.
- X_ITE is the only renderer. Electron security is unchanged: `contextIsolation`
  on, `nodeIntegration` off, no renderer filesystem access, CSP and network
  policy unchanged. Everything in this contract runs in the existing renderer
  page; it needs no IPC, preload or main-process change.

## Accepted WD2-C0 basis

- `WD2_C0_XITE_PICKING_SPIKE.md`, `WD2_C0_XITE_PICKING_CLOSEOUT.md`,
  `spikes/wd2-c0-xite-picking/**`.
- Spike `WRLFORGE_WD2_C0_XITE_PICKING_SPIKE_PASS_WITH_LIMITATIONS`; QA
  `WRLFORGE_WD2_C0_INDEPENDENT_QA_PASS_WITH_CONDITIONS`; owner disposition
  *PRIVATE X_ITE PICKING BASIS APPROVED FOR WD2-D WITH REQUIRED COMPATIBILITY
  GUARDS*.
- Reused as-is: the parse-time `nodeStatement` provenance hook, the
  generation-bound `WeakMap`, the refusal ladder and chain proof (spike §12,
  `mapping.js` `resolvePick`), the promotion rule (spike §13), the
  coordinate rule (spike §11).
- **Not** reused: the harness's permanent prototype patch and its global
  `state.capture` slot (closeout findings 1 and 2). This contract replaces both.

## Private X_ITE adapter

**One module owns every private X_ITE access:**
`src/preview/xite-pick-adapter.js` (new in #30; browser-loaded, dual-use like
`src/editor/scene-selection.js` so Node tests can drive it with fakes; added to
`test/editor/script-load-order.test.js` with module-unique const names).

Module boundary:

- **Only** `src/preview/xite-pick-adapter.js` may touch private X_ITE API. No
  other production file may reference `VRMLParser`, `nodeStatement`, `touch`,
  `getHit`, `getParents`, `getLayer0`, `groupNode(s)`, `getViewport`, or
  internal node `getExecutionContext`. A source-scan test enforces this (G8).
- The **shared parser-hook coordinator** (§Shared parser-hook coordinator) lives
  inside this module, module-private. It owns hook arbitration only.
- Capture and provenance records are **per parse**, local to one
  `parseWithProvenance` call, never module state.
- The pure mapper `src/editor/viewport-pick.js` contains **no** X_ITE access,
  private or public. It consumes plain-data snapshots only.

Public surface (the whole of it):

```js
const adapter = createXitePickAdapter({ X3D, browser });   // one per X_ITE browser

adapter.compatibility()
  // -> { ok: true } | { ok: false, reason }
  //    false is sticky for the page lifetime ONLY for a structural private-API
  //    failure (P-row assertion, displaced wrapper). Per-click refusals never
  //    change it.

adapter.parseWithProvenance(text, { generationId })
  // replaces browser.createX3DFromString(text) for the editor live preview.
  // Acquires the shared coordinator first (serialized, §coordinator).
  // -> Promise<{ scene, generation }>  generation is an opaque frozen token,
  //    or { scene, generation: null, reason } when provenance is unprovable.
  //    Rejects exactly as createX3DFromString would (parse errors propagate).

adapter.activate(generation, scene)
  // after replaceWorld(scene) succeeded: this generation is the displayed one.
  // Ignored unless generation's capture COMPLETED and was not aborted or
  // superseded. Any previously active generation is disposed first.

adapter.retire()
  // the displayed scene is about to change or the session ended: the active
  // generation is disposed; every later pick is REFUSED_STALE.

adapter.pick(clientX, clientY)
  // -> PickHit snapshot: plain data, no runtime object (§Runtime hit mapping).
  //    touch() === false -> NO_HIT without reading getHit() (§touch false).

adapter.abort()
  // synchronously release this adapter's pending capture (§Abort contract).

adapter.dispose()
  // abort() + retire(); drops the browser reference.
```

Deliberately absent: any general X_ITE abstraction, any write to the scene,
any runtime object in a return value that outlives the call.

The **source join** (exact-span → AST → scene item, promotion) is pure and
X_ITE-free: `src/editor/viewport-pick.js`, bundled into the editor view and
published on `WRLForgeSceneBridge.viewportPick` like `firstObject`. It ports
`spikes/wd2-c0-xite-picking/mapping.js` `resolvePick` but consumes the
**existing analysis** parse, not a parse of its own (§Performance).

## Private API inventory

Every surface #30 depends on. "C0" = directly exercised by the accepted spike.

| # | object / member | WD2-D use | assumption | runtime assertion | on failure | C0 |
|---|---|---|---|---|---|---|
| P1 | `X3D.VRMLParser` (Namespace re-export, :36031) | locate the classic-VRML parser prototype | reachable on the global `X3D` namespace; absent from `x_ite.d.ts` | `typeof X3D.VRMLParser === 'function'` | `COMPATIBILITY_DISABLED` `parser-class-missing` | yes |
| P2 | `VRMLParser.prototype.nodeStatement` (:33942) | wrap for one parse | own prototype method, zero formal params, returns the runtime node of one DEF/USE/anonymous statement; first thing it does is `this.comments()` | `Object.hasOwn(proto,'nodeStatement')`, `typeof === 'function'`, `.length === 0` | disabled `parser-hook-missing` | yes |
| P3 | `VRMLParser.prototype.comments` (:33531) | skip leading whitespace/comments before reading `start` | idempotent | `typeof === 'function'` | disabled `parser-comments-missing` | yes |
| P4 | parser instance `input`, `lastIndex` (set by `setInput`, :33425–33427) | gate on exact text; `start`/`end` offsets | `input` is the exact string passed to `createX3DFromString`; `lastIndex` is a UTF-16 code-unit index into it | probe parse (G4) yields the expected `[start,end)` | disabled `parser-offsets-unproven` | yes |
| P5 | parser `getScene()` (:32972), `getExecutionContext()` (:32976), `isInsideProtoDeclaration()` (:32992) | attribute each record to the parsed scene's own top context; drop PROTO bodies | records from other scenes/contexts are identifiable by object identity | `typeof === 'function'` for each | disabled `parser-context-missing` | yes |
| P6 | `browser.touch(x, y)` (:71639) | run X_ITE's POINTER pick at one viewport pixel | returns `true` on a hit; returns `false` **without resetting the hit** while the viewer is active or on an XR pose mismatch (:71641–71645) | `typeof === 'function'`, `.length === 2` | disabled `touch-missing` | yes |
| P7 | `browser.getHit()` (:71537) → `{ id, shapeNode, sensors: Map, layerNode, … }` (:71430) | read the hit **only after `touch()` returned `true`** | one shared mutable object; `shapeNode` is an internal Shape node, not an SFNode proxy | `typeof getHit === 'function'`; result has `'shapeNode' in hit` and `hit.sensors instanceof Map` | disabled `hit-shape-changed` | yes |
| P8 | runtime node `getParents()` (`X3DChildObject`, :15719) | climb Shape → document root | returns an `IterableWeakSet` of live parents (fields, nodes, execution contexts) | `typeof === 'function'` on the hit Shape; result iterable | pick `UNSUPPORTED` `runtime-parents-unavailable`, and compatibility disabled | yes |
| P9 | runtime node `getExecutionContext()` (`X3DBaseNode`, :25222) | Inline / PROTO-body / document classification | the generation's scene object is the context of document nodes | `typeof === 'function'` | as P8 | yes |
| P10 | `browser.getWorld()` (:79569) → `getLayer0()` (:57998) → `layer.groupNode`, `layer.groupNodes` (:45256–45257) | exclude X_ITE's root holders **by object identity only** | these three objects hold the scene's root nodes | all three non-null after `activate()` | disabled `world-infrastructure-missing` | yes |
| P11 | `browser.getViewport()` (:72425) → `[x, y, w, h]` | client → touch coordinates (spike §11) | `w`,`h` are the touch-space extent | array-like of length 4, finite | disabled `viewport-missing` | yes |
| P12 | `X3D.X3DBaseNode` (:25771), `X3D.X3DExecutionContext` (:60182), `X3D.X3DScene` (:61366) | `instanceof` classification of parents and contexts | internal classes on the namespace; `X3DBaseNode` absent from `x_ite.d.ts`, the other two declared there but applied to internal objects | each `typeof === 'function'` | disabled `class-missing` | yes |
| P13 | internal node `getTypeName()` | the exact-span join's type check | equals the AST `nodeType` for VRML97 built-ins | `typeof === 'function'` on the hit Shape | pick `UNSUPPORTED` | yes |

Public (`x_ite.d.ts`) surfaces also relied on: `createX3DFromString`,
`replaceWorld`, `currentScene`, `baseURL`, `version`.

**Not a production dependency:** `browser.getPointerFromEvent` (:71511). C0
used it only to cross-check the coordinate rule; it stays a QA cross-check.

Two facts found in this lane that C0 did not state and #30 must honour:

1. **The VRML statements are parsed asynchronously.** `x3dScene()` (:33561)
   parses the header synchronously, then parses every statement inside
   `browser.loadComponents(scene).then(…)` (:33594). The hook window therefore
   spans the whole awaited `createX3DFromString`, not one synchronous call.
2. **A `false` from `touch()` leaves the previous hit in place.** The early
   returns at :71641–71645 skip the reset at :71694–71700, so
   `getHit().shapeNode` can still name the *previous* pick's Shape. Reading
   `getHit()` after a `false` would be a wrong-selection path.

## Shared parser-hook coordinator

`editor.html` hosts two X_ITE browsers (`#preview` Mall, `#wpCanvas` World) in
one realm. Both share **one** `X3D.VRMLParser.prototype.nodeStatement`, so one
adapter per browser is not enough on its own. The adapter module therefore
contains exactly one module-private **hook coordinator** shared by every adapter
on the page.

Preferred production design: a **serialized FIFO ownership queue** with
ownership tokens. Deterministic serialization, not racing and not refusing
normal editor work.

```text
coordinator (module-private, one per page/realm)
  owner:    OwnershipToken | null
  waiters:  FIFO of { token, resolve }
  disabled: reason | null              // page-lifetime, structural failures only

acquire(token)  -> Promise<'owned' | 'cancelled' | 'disabled'>
                   resolves 'owned' only when owner === null; FIFO order
release(token)  -> idempotent; acts only if owner === token, then wakes the
                   next waiter that is still live
cancel(token)   -> removes a still-waiting token; it resolves 'cancelled'
disable(reason) -> every adapter's compatibility() becomes false
```

- At most one token owns the parser hook at a time, across all browsers.
- A request from the **same adapter** that is still waiting or owning when a
  newer request arrives is superseded: the older one is aborted (§Abort
  contract) before the newer one queues. Each adapter therefore has at most
  one live request, and the queue is at most one entry per browser.
- A request from **another adapter** waits its turn. While waiting, nothing is
  rendered for it. The editor drives one engine per document
  (`editor-preview.js` `engine()`), so cross-browser contention is
  exceptional, not normal.
- A waiter is never left hanging on a parse that does not settle. Every owner
  has an abort call site (§Abort call sites), and a waiter whose own
  generation is retired before it acquires is `cancel`led.

### Allowed shared state

Only the coordinator's arbitration state: `owner` token, the waiter queue, and
`disabled`. Tokens carry an identity and a state
(`waiting | owning | completed | aborted | lost`). Nothing else.

### Forbidden shared state

No captured source spans, no runtime-node maps, no generation records, no
current-scene provenance, no mutable parse-result buffers at module or `window`
level. **Condition 5 is unchanged**: the coordinator is not a capture slot.

## Parse-hook lifecycle

`parseWithProvenance(text)`:

```text
1. compatibility() must be ok          else plain createX3DFromString, generation null
2. token = new OwnershipToken(); supersede this adapter's previous live token
3. r = await coordinator.acquire(token)
   r !== 'owned'                       -> plain createX3DFromString, generation null
                                          ('cancelled' callers do not render at all)
4. capture = { text, records: [], poisoned: false, closed: false }   // local to this call
   original = proto.nodeStatement
   wrapper  = makeWrapper(original, capture)                          // closes over capture
   token.wrapper = wrapper; token.original = original; token.capture = capture
   proto.nodeStatement = wrapper; token.state = 'owning'
5. try   { scene = await browser.createX3DFromString(text) }
   finally { settle(token) }
6. token.state !== 'completed'         -> return { scene, generation: null }
                                          (never build or publish a map)
7. build the generation map from capture.records (§Exact-source provenance)
```

`settle(token)` (the `finally`):

```text
if token.state === 'owning':
    if proto.nodeStatement === token.wrapper:
        proto.nodeStatement = token.original; token.state = 'completed'
    else:
        lost ownership (§Displaced wrapper)
    token.capture.closed = true
    coordinator.release(token)
else:                                  // aborted / lost already handled
    do nothing: no prototype write, no publication, no second release
```

The wrapper:

```text
if (capture.closed || this.input !== capture.text) return original.call(this)
this.comments(); start = this.lastIndex
node = original.call(this)          // its exceptions propagate unchanged
try   { if node is an internal node: capture.records.push({node, start, end: this.lastIndex,
                                       scene: this.getScene(), ctx: this.getExecutionContext(),
                                       inProto: this.isInsideProtoDeclaration()}) }
catch { capture.poisoned = true }   // recording never breaks the parse
return node
```

Restoration is in `finally`, so it runs when the parse succeeds, when X_ITE
rejects, and when anything after the parse throws (map building is step 7,
after the `finally`). Setup failure before step 4 installs nothing and releases
the token. **No permanent prototype patch can remain**: after every exit path
`proto.nodeStatement === original`, unless another owner displaced it, in which
case WD2-D does not write to the prototype at all (G5, G6).

## Abort contract

X_ITE parses statements after an asynchronous `loadComponents` (:33594), so a
provenance parse can stay pending. `abort()` therefore acts synchronously; it
is not a flag that the late `finally` must notice.

`abort()` on this adapter's live token, in this order, synchronously:

1. If `token.state === 'owning'` and `proto.nodeStatement === token.wrapper`,
   restore `token.original` now. If it is `owning` but the wrapper is no
   longer installed, handle as §Displaced wrapper instead.
2. Invalidate the capture: `token.state = 'aborted'`, `capture.closed = true`.
   The wrapper, if anyone still calls it, passes straight through.
3. Retire the not-yet-published generation: it can never be activated.
4. Block late publication: steps 6–7 of `parseWithProvenance` and `activate()`
   both require `token.state === 'completed'`; an aborted token never gets
   there.
5. Release the coordinator (`release` if owning, `cancel` if waiting).
6. Picking for that attempted generation stays fail-closed: no map exists, so
   every click is `REFUSED_STALE` or `UNSUPPORTED` `generation-unprovable`.

A late completion after `abort()` is harmless by construction: `settle()` sees
`aborted` and writes nothing, so it cannot restore over somebody else's
wrapper, publish a map, reactivate an old generation, overwrite a newer
generation, or release a coordinator it no longer owns. The X_ITE scene it
resolves with is simply not activated; whether it is still displayed is the
render path's existing decision, and picking refuses for it either way.

## Abort call sites

Every one of these aborts the adapter's live token (and retires the active
generation where the displayed scene changes) **before** continuing. A new
provenance parse cannot start until the previous owner has released, because
`acquire` serializes.

| trigger | call site (current code) |
|---|---|
| preview reload / newer generation supersedes | `renderer/editor-preview.js` `fire()`, before `PS.beginUpdate` (line 251); also implicit in `parseWithProvenance` step 2 |
| source-driven replacement | `editor-preview.js` `showSaved()`; Mall `renderer/preview.js` `switchMode()` / `refreshGuides()` → `renderForMode()` (the Fit render is never a provenance parse) |
| `stop()` / session switch | `editor-preview.js` `stop()`, and `start()` for a different session (which calls `stop()`) |
| browser disposal | `preview.js` / `world-preview.js` `ensureBrowser()` discarding an unusable browser, and `src/preview/browser-readiness.js` canvas replacement: `adapter.dispose()` |
| workspace teardown | `renderer/editor.js` `setWorkspaceMode` leaving Model: `abort()` + `retire()` + listener removal |
| page teardown | a `pagehide` handler: `dispose()` on every adapter |

## Displaced wrapper

If, while this token is `owning`, `proto.nodeStatement !== token.wrapper` (at
`settle()` or `abort()`), ownership was lost. The adapter must:

- **detect** it by exact identity against its own wrapper;
- **not write** to `proto.nodeStatement` at all, neither restore nor re-install;
- **invalidate** the capture (`token.state = 'lost'`, `capture.closed = true`)
  so the generation is never built or published;
- **disable** picking: `coordinator.disable('parser-hook-displaced')`, which
  turns `compatibility()` false for every adapter on the page, because the
  shared prototype is in an unknown state;
- **release** its coordinator ownership, so waiters wake, see `disabled`, and
  fall back to plain parses.

Rendering continues with plain `createX3DFromString`. Testable with a fake
prototype (G6) and in Electron (V12).

## Capture isolation (no global slot)

- The capture object is created inside one `parseWithProvenance` call and is
  reachable only through that call's stack, its token, and its wrapper's
  closure. There is no module-level or `window`-level capture variable.
- The coordinator holds ownership tokens, never records (§Allowed shared
  state).
- Any other parse that runs while the wrapper is installed (an Inline, a
  Script `createVrmlFromString`, World `validateText`, a plain parse from a
  waiter that fell back) passes straight through: its `input` differs, or, for
  an identical string, its records carry a different `scene` and are dropped
  at step 7. Generation A's capture is closed and its token released before
  generation B can install, so A cannot write into B.

## touch() === false invariant

A production invariant, not an optimization:

> When `browser.touch(x, y) === false`, the adapter **MUST NOT** read or reuse
> `browser.getHit()`. The result is `NO_HIT` `viewer-active-or-no-hit`.

`touch()` returns `false` early while the X_ITE viewer is active or on an XR
pose mismatch (:71641–71645), **before** the reset that clears `id`,
`shapeNode` and `layerNode` (:71694–71700). The shared hit can therefore still
hold the previous pick's Shape. Additionally, even after `true`, the adapter
requires `hit.id > 0 && hit.shapeNode`, reads the hit synchronously in the same
handler, and copies only plain data out of it. A retained hit can never become
a source selection. Guarded by G13 and V13.

## Preview generation model

A generation is a frozen adapter-minted token:

```text
{ sessionId, overlayGeneration, text, scene, occ: WeakMap<runtimeNode, Occurrence[]> }
```

- `overlayGeneration` is main's per-session counter already returned by
  `editor:previewLoad` (`res.generation`, `src/preview/buffer-overlay.js`
  `beginGeneration`). It is diagnostic; **identity is the token object**.
- `text` is the exact string handed to `createX3DFromString`.
- `scene` is the scene object `createX3DFromString` resolved with.

The adapter holds at most one *active* generation. Rules:

| event | result |
|---|---|
| a preview attempt begins (`fire()` → `PS.beginUpdate`) | `abort()` + `retire()`: any pending capture released, active generation disposed **before** parsing. Old maps are unusable from this moment. |
| parse succeeds and `replaceWorld(scene)` succeeds | `activate(generation, scene)`, only if its token is `completed` |
| a pending parse is aborted or superseded | its generation is never activated; a late completion publishes nothing (§Abort contract) |
| parse fails; X_ITE keeps the last valid scene on screen | no generation is active → every click `REFUSED_STALE` `preview-shows-last-valid-scene` |
| source text changes after activation | generation stays active, but its `text` no longer equals the editor text → `REFUSED_STALE` `source-changed-since-preview` |
| a hit arrives for an older generation (stale closure, delayed handler) | `REFUSED_STALE` `hit-from-another-preview-generation` |
| a newer generation starts while an older event is in flight | the older event resolves against a retired token → `REFUSED_STALE` |
| the displayed scene is no longer the generation's scene (Mall *Cybertown Fit* re-render, Anchor navigation, "Show saved version") | `browser.currentScene !== generation.scene` → `REFUSED_STALE` `preview-scene-replaced` |
| editor session switch, `stop()`, page unload, browser recreated by `browser-readiness.js` | `dispose()` / `retire()`; a recreated browser gets a new adapter |

## Exact-source provenance

- Offsets are the parser's own `lastIndex` values into its own `input`, which
  `GoldenGate.createInput('STRING')` → `decodeText` (:100657) passes through
  unchanged for a string. They are UTF-16 code-unit offsets, the unit of the
  WRL Forge AST `range.*.offset`.
- Provenance is usable only when **all three strings are identical by `===`**:
  `generation.text`, the analysis text (`S.analysisSession.text`, from the same
  `onAnalysis` that built `S.sceneTree`), and `currentText()`. Otherwise
  `REFUSED_STALE`. No normalization is applied anywhere; there is no second
  normalized document.
- **WD2-D itself never normalizes source text** before provenance mapping: no
  line-ending, BOM, Unicode or whitespace rewriting anywhere in the adapter,
  the resolver or the integration.
- **Mandatory #30 verification cases** (acceptance, not open questions):
  **LF, CRLF, UTF-8 BOM and Unicode text** must each be proven to give exact
  source offsets end to end (editor buffer → preview text → X_ITE `lastIndex`
  → AST range → scene item), or to fail closed with no generation. Unit
  coverage G12, Electron coverage V7. Known facts going in:
  - *Unicode*: C0 checked a non-ASCII comment before the first node (spike §4).
  - *CRLF*: not exercised by C0. CodeMirror 6 with no `lineSeparator` facet
    (none is configured in `src/editor/browser/editor-view.js`) normalizes
    line breaks inside its own document, and the preview is fed that buffer,
    so generation and analysis text are the same string. #30 proves it.
  - *BOM*: not exercised by C0. `VRMLParser.isValid` requires the string to
    start with `#VRML`/`#X3D`/a statement, so a leading U+FEFF would make
    X_ITE reject and leave no generation (fail closed). #30 proves the actual
    outcome.
- Comments/whitespace: `start` is read after `this.comments()`, so it equals the
  AST Node range start (C0: every node, every fixture).
- Kept records: `scene === generation.scene && ctx === generation.scene &&
  !inProto`. Everything else is dropped.

## Runtime hit mapping

```text
pointerdown (primary button, capture phase on the <x3d-canvas>)
  -> armed? (Model workspace only)   no  -> ignore (no result, no UI)
  -> compatibility     no  -> COMPATIBILITY_DISABLED
  -> active generation none -> REFUSED_STALE
  -> client -> touch coords (spike §11, via getViewport)
  -> touch(x, y) === false  -> NO_HIT 'viewer-active-or-no-hit'   (never read getHit)
  -> hit = getHit(); hit.id > 0 && hit.shapeNode   else NO_HIT
  -> snapshot: walk shapeNode upward via getParents into plain data
       { generation token, sensors: count/types, ctxKind per node,
         occurrences per node, parents per node }   (labels, never objects)
  -> resolver (pure) on pointerup
```

The authoritative runtime object is **`hit.shapeNode`**, read synchronously
inside the same handler that called `touch()`. The snapshot holds no runtime
object, so nothing can outlive the generation.

Commit happens on the matching `pointerup` only when the pointer moved less
than a small slop and the same generation is still active; otherwise the
gesture was navigation and nothing is selected. (C0 proved capture-phase
`mousedown` picking; drag discrimination is new in #30 and must be proven, V5.)

The resolver applies the C0 ladder, in order:

1. generation retired / text mismatch / scene replaced → `REFUSED_STALE`
2. no Shape → `NO_HIT`
3. `sensors.size > 0` → `REFUSED_SENSOR_CONFLICT`
4. Shape context is another `X3DScene` → `REFUSED_EXTERNAL`
5. Shape context is not the generation scene (PROTO body) → `UNSUPPORTED` `proto-instance`
6. analysis has syntax errors → `UNSUPPORTED` `document-has-syntax-errors`
7. climb: every node needs exactly 1 occurrence (`>1` → `REFUSED_AMBIGUOUS`,
   `0` → `UNSUPPORTED`) and exactly 1 live document parent, where only the
   P10 objects are excluded, by identity (`>1` → `REFUSED_AMBIGUOUS`,
   `0` → `UNSUPPORTED` `runtime-node-detached`)
8. each occurrence `[start,end)` equals exactly one AST `Node` range, same type
   (else `UNSUPPORTED`)
9. runtime chain equals AST containment chain link by link (else `UNSUPPORTED`)
10. promotion (§Shape/Transform)
11. `sceneTree.itemForAstNode(S.sceneTree, logical)` (object identity) → item,
    else `UNSUPPORTED`

## Shape / Transform selection policy

Exactly spike §13. Promote the proven Shape to its parent Transform only when:

- the Shape's single proven parent is a `Transform`;
- `simpleObject.recognize(parentAst)` (`src/vrml/simple-object.js:95`, WD2-C's
  recognizer) returns non-null; and
- the recognized `.shape` is **the same AST object** as the proven Shape.

Otherwise select the proven Shape itself (P11b, P13, P14, P15, Anchor). Never
climb further, never "nearest Transform", never by outline-row structure.

## Selection integration

- **The only selection authority is `sceneSelection`** (`renderer/editor.js:121`).
- WD2-D's only write, on `PROVEN` only:

  ```text
  proven viewport hit → existing scene item → sceneSelection.setSelection(item.id)
  ```

- Scene Tree (`renderer/scene-tree.js` `selection.subscribe`), Inspector
  (`renderer/scene-inspector.js:526`) and the Model panel
  (`renderer/model-workspace.js:92`) update through their existing
  subscriptions. WD2-D adds no listener of its own to them.
- Forbidden: a viewport selection store, an X_ITE selection store, a duplicate
  selected-node field, or separate Inspector synchronization.
- On any refusal the current selection is **left unchanged** (a refused click
  never clears a valid tree selection) and the reason is shown (§Compatibility
  disable behavior).
- **Source caret (owner decision O1, resolved):** WD2-D does **not** move the
  CodeMirror caret. It adds no `setSelectionRange`, caret synchronization,
  source scrolling or cursor-follow. Viewport selection has exactly the
  source/tree/Inspector semantics a Scene Tree selection has today, which also
  does not move the caret (findings use the separate `navigateTo`,
  `renderer/editor.js:302`). Caret-follow, if wanted later, is separate UX
  scope.
- Selection survival after an edit is unchanged: WD2-B
  `reanchorAfterAnalysis` owns it.

## Model vs Play

There is no Play mode. The workspace is `'model' | 'code'`
(`renderer/editor.js:113`, `setWorkspaceMode`); Model/Play is UI-0
([#33](https://github.com/Ascendance3D/wrlforge/issues/33)).

Final interaction rule (owner decision O2, resolved):

```text
Model workspace:
    viewport picking can be armed if compatibility is proven
Code workspace:
    viewport picking is inert (no listener, no touch(), no provenance parse
    requested; the preview behaves exactly as today)
Future Play workspace/mode:
    viewport editor picking is OFF unless a later authorized architecture lane
    explicitly changes this rule
Authored pointing-device sensor / Anchor conflict:
    authored interaction wins; editor selection refuses (REFUSED_SENSOR_CONFLICT)
```

WD2-D never calls `preventDefault`/`stopPropagation` and never alters X_ITE's
own pointer handling. A Model/Play toggle, sensor-dispatch suppression (spike
§15 Policy D) and any tool-handle overlay belong to UI-0 / WD2-E. WD2-D does
not implement UI-0.

## Refusal states

```js
{ status, reason, generation /* overlay counter, diagnostic */,
  source: { shape:{start,end,nodeType}, logical:{start,end,nodeType,role} } | null,
  sceneTreeItemId: string | null }
```

| status | when | reasons (machine codes) | UX text (contextual) |
|---|---|---|---|
| `PROVEN` | exact occurrence proven | `shape`, `shape-promoted-to-proven-simple-object` | — (selection changes) |
| `NO_HIT` | nothing pickable under pointer; **`touch() === false`, mapped without reading `getHit()`** | `no-geometry-under-pointer`, `viewer-active-or-no-hit` | none |
| `REFUSED_AMBIGUOUS` | shared runtime identity | `runtime-node-referenced-by-several-source-occurrences`, `runtime-node-has-several-live-parents` | "This object is drawn by a shared (DEF/USE) node; select it in the Scene Tree." |
| `REFUSED_EXTERNAL` | Inline content, or a World preview whose root is not the edited document | `hit-belongs-to-another-document`, `preview-root-is-another-document` | "This object belongs to an Inline file and cannot be selected from this document." |
| `REFUSED_SENSOR_CONFLICT` | pointing-device sensor or Anchor encloses the hit | `pointing-device-sensor-under-pointer` | "This object is interactive (sensor or link); select it in the Scene Tree." |
| `REFUSED_STALE` | preview is not the current exact text | `hit-from-another-preview-generation`, `source-changed-since-preview`, `preview-shows-last-valid-scene`, `preview-scene-replaced` | "The preview is out of date; select after it updates." |
| `UNSUPPORTED` | proof incomplete or out of WD2-D identity | `proto-instance`, `document-has-syntax-errors`, `runtime-node-has-no-parse-provenance`, `runtime-node-detached`, `exact-span-join-found-<n>`, `exact-span-join-type-disagrees`, `runtime-chain-disagrees-with-source-containment`, `logical-node-not-in-scene-tree`, `generation-unprovable`, `preview-is-not-the-document` | "This object cannot be selected from the preview; select it in the Scene Tree." |
| `COMPATIBILITY_DISABLED` | a P1–P13 assertion failed | the P-row reason, `parser-hook-displaced` | persistent line (below) |

Every non-`PROVEN` status selects nothing and **leaves the existing selection
unchanged**. No status collapses to `null`.

Where #30 must emit `preview-is-not-the-document`: the Mall *Cybertown Fit*
render (`renderer/preview.js` `fitPreviewVrml`, text + guides) and any render
that did not go through `parseWithProvenance`. Where
`preview-root-is-another-document`: a World session editing a nested file
(`editedIsPrimary === false`; the root string is the saved primary,
`src/preview/world-preview-bridge.js` §nested edit).

## Sensor / interaction contract

Policy C (spike §15), because no Play mode exists and suppression would need
further private API:

- `hit.sensors.size > 0` → `REFUSED_SENSOR_CONFLICT`. This covers TouchSensor,
  PlaneSensor, CylinderSensor, SphereSensor (all pointing-device sensors
  registered on the hit) and Anchor (X_ITE registers an internal TouchSensor,
  measured in C0 P18).
- WD2-D's `touch()` is inert by itself (C0 §10). X_ITE's own pointer handling
  on the same click still fires sensors and follows Anchors, exactly as the
  preview does today. An Anchor that replaces the world retires the generation
  (`preview-scene-replaced`).
- WD2-D uses the browser's shared hit object, not a private one passed as
  `touch`'s third parameter: `touch` writes `sensor.hit = hit` (:71674), and
  X_ITE's `buttonReleaseEvent` matches active sensors by that object (:71606),
  so a private hit object could break an in-progress sensor release.
- Never pick on `pointermove`; only on a primary-button `pointerdown`.

## Stale preview contract

When the current source is invalid and X_ITE still shows the last valid scene,
there is no active generation (it was retired when the failed attempt began)
and the generation text could not equal the current text anyway. Every click
is `REFUSED_STALE` `preview-shows-last-valid-scene`. A stale runtime hit is
never mapped into the new text; old offsets are never re-applied.

## Compatibility disable behavior

- State per adapter: `unknown` → `compatible` | `disabled(reason)`. Checked at
  adapter creation (P1–P7, P11, P12, plus the G4 probe parse) and re-checked
  per pick (P7–P10, P13 on the actual objects).
- **Page-lifetime disable is reserved for structural private-API failures**: a
  failed P-row assertion, a failed G4 probe, or a displaced wrapper
  (`coordinator.disable`, which applies to every adapter because the parser
  prototype is shared).
- **Ordinary per-click refusals never disable picking**: `NO_HIT`,
  `REFUSED_*`, and per-pick `UNSUPPORTED` leave compatibility unchanged, and
  the next click is evaluated normally. An aborted or unprovable generation
  only makes clicks on *that* generation refuse.
- Disabled affects **only** viewport picking. Source selection and editing,
  Scene Tree selection, Inspector, Model panel, live preview rendering and
  navigation stay usable; `parseWithProvenance` degrades to plain
  `createX3DFromString`.
- UX: one **persistent** line in the existing `#modelStatus` element
  (`renderer/editor.html:455`, `role="status"`) while in Model:
  *"Preview picking unavailable (X_ITE compatibility check failed:
  `<reason>`). Select objects in the Scene Tree."* Refusals are **contextual**:
  the same line shows the refusal text after a click and clears on the next
  selection change. No new panel, dialog, cursor or layout.
- The reason is also logged once to the console with the P-row id.

## Disposal

Nothing may outlive its lifecycle:

| retained thing | released by |
|---|---|
| parser wrapper | `settle()` in the `finally` of its own parse, or `abort()`; never by a different token |
| coordinator ownership / queue entry | `release` at settle, `release`/`cancel` at `abort()`; never held past either |
| capture records | dropped at end of `parseWithProvenance`, or at `abort()` |
| `WeakMap` occurrence map, generation token, `scene` reference | `retire()` at the next attempt, `dispose()` on `stop()`/session switch/unload |
| pointer listeners (`pointerdown`/`pointerup` capture on the canvas) | removed on `stop()`, on Model→Code, and before a `browser-readiness.js` canvas replacement; re-attached to the new canvas |
| X_ITE object references | never stored outside the active generation; snapshots hold labels only |
| adapter | one per X_ITE browser; discarded with a recreated browser |

This fits the existing teardown in `renderer/editor-preview.js` `stop()`
(timers, scheduler, `previewClose`) and `renderer/preview.js` /
`renderer/world-preview.js` `ensureBrowser()` (discard unusable browser).

## Proposed integration points (not implemented)

| what | where |
|---|---|
| adapter creation | `renderer/preview.js` `acquireBrowser()` and `renderer/world-preview.js` equivalent: one adapter per acquired browser; all adapters share the module's one coordinator |
| start capture | the editor-lane render only, and only while Model is armed: `preview.js` `load()` line 189 and `world-preview.js` `load()` line 151 call `adapter.parseWithProvenance(text, …)` instead of `createX3DFromString` when the orchestrator asks (an opt passed through `engine().load(...)`, `editor-preview.js:266`) |
| abort / retire old map | every row of §Abort call sites; first of all `editor-preview.js` `fire()` immediately before `PS.beginUpdate` (line 251) |
| publish generation | after `replaceWorld` succeeds (`preview.js:190`, `world-preview.js:152`): `adapter.activate(...)`; never for `fitPreviewVrml()` renders or `validateText` |
| disable on incompatibility | `adapter.compatibility()` read by the Model status painter |
| attach / remove click handling | `editor.js` `setWorkspaceMode` / `applyWorkspace` arm on entering Model, disarm on leaving it; `editor-preview.js` `start()`/`stop()` |
| selection | `editor.js`: resolver result → `sceneSelection.setSelection(item.id)`, nothing else |

## X_ITE version pin

Current: `package.json` `"x_ite": "^15.1.10"`; lockfile and installed copy are
15.1.10. **#30 must change it to exactly `"x_ite": "15.1.10"`** (and the
lockfile's root `packages[""].dependencies` entry to match) before any private
API is used. This lane changed neither file.

## Upgrade guards

Node tests (collected by `npm run check`, `test/preview/`):

| id | guard |
|---|---|
| G1 | `package.json` declares `x_ite` exactly `15.1.10` (no range); lockfile resolves 15.1.10 |
| G2 | installed `node_modules/x_ite/package.json` version is `15.1.10` |
| G3 | `x_ite.min.js` contains each private method name in P1–P11; `x_ite.d.ts` still does **not** declare `nodeStatement`, `getHit`, `getParents`, `getLayer0` (if X_ITE ever publishes them, the adapter should be revisited, not silently kept) |
| G5 | fake prototype: after success, after a parse rejection, after a wrapper-recording throw, after a map-building throw, `proto.nodeStatement === original` and the coordinator owner is `null` |
| G6 | **displaced wrapper**: another owner replaces the wrapper mid-parse; the adapter writes nothing to the prototype (the replacement survives), the capture is `lost`, no generation is published, `coordinator.disable` fires, ownership is released and a waiter wakes to `disabled` |
| G7 | **shared prototype**: two adapters (two fake browsers, one fake prototype) request provenance parses concurrently; the second waits until the first releases; at no instant are two wrappers installed; neither map contains the other's records |
| G8 | source scan: no file outside `src/preview/xite-pick-adapter.js` references the P-surface names; `src/editor/viewport-pick.js` references no X_ITE API; no module-level capture/record/map state in the adapter; no fallback (DEF name, type, index, nearest, fingerprint, matrix) in either — the spike's `spike.test.js` scan, ported |
| G9 | resolver unit tests: every refusal status and reason; a refusal leaves `sceneSelection` unchanged |
| G10 | missing/changed P-surface on a fake `X3D` → `COMPATIBILITY_DISABLED`; the selection controller still accepts Scene Tree selection; a per-click refusal does **not** change `compatibility()` |
| G12 | **source forms**: LF, CRLF, BOM and Unicode fixture strings; offsets recorded from a fake parser that reports `lastIndex` into the exact input equal the WRL Forge AST ranges of the same string; nothing in the path rewrites the text |
| G13 | **touch false**: a fake browser whose `getHit()` still holds a previous valid Shape returns `false` from `touch()`; result is `NO_HIT` `viewer-active-or-no-hit`, `getHit` was **not called** (spy), the old Shape is not selected |
| G14 | **abort**: a pending provenance parse (fake `createX3DFromString` that resolves later) is aborted; synchronously after `abort()` the original method is restored and the coordinator is released; the next queued parse then installs and completes normally; the late resolution of the aborted parse publishes no map, does not activate, does not touch the newer owner's wrapper, and does not release the newer owner |

Electron runtime guards (real X_ITE, existing `VisualQaRunner` path):

| id | guard |
|---|---|
| G4 | probe parse of a fixed string with known spans returns exactly those `[start,end)`, via the same adapter code path production uses; any mismatch disables |
| G11 | `touch` + `getHit` on a known fixture returns a Shape whose `getParents()` reaches `getLayer0().groupNode(s)` |

## Condition-to-test matrix

| # | condition | production guard | unit / integration test | Electron / visual QA | expected failure result |
|---|---|---|---|---|---|
| 1 | Pin X_ITE `15.1.10` | exact `package.json` + lockfile | G1, G2 | V1 (runtime `browser.version`) | test fails; CI red |
| 2 | One narrow adapter | all private access, incl. the coordinator, in `xite-pick-adapter.js`; pure mapper X_ITE-free | G8 source scan | — (static) | test fails |
| 3 | Hook only for the active parse | wrapper installed only by the coordinator owner, gated on `capture.text`/`capture.closed`; `abort()` removes it synchronously | G5, G7, G14 | V2, V3, V14 | generation `null` → `UNSUPPORTED` `generation-unprovable` |
| 4 | Restore in `finally` | `settle()` in `finally`; `abort()`; ownership-checked restore only | G5, G6, G14 | V2, V12, V14 | displaced → `COMPATIBILITY_DISABLED` `parser-hook-displaced`, no destructive restore |
| 5 | No global capture slot | coordinator holds only owner token + queue; capture is call-local | G7, G8 (no module-level capture state) | V3 (two browsers, overlapping requests) | overlap impossible; a loser waits, never shares a capture |
| 6 | Generation + exact text | token identity; `generation.text === analysis text === currentText()`; `currentScene === generation.scene`; no normalization | G9 stale cases, G12 | V4, V7 | `REFUSED_STALE` |
| 7 | Discard maps every reload | `abort()` + `retire()` at every §Abort call site; aborted tokens never publish | G9, G14 | V4, V9, V14 | `REFUSED_STALE` |
| 8 | Disable when unprovable | structural failures disable for page lifetime; per-click refusals never do | G10, G6, G4 | V8, V12 | `COMPATIBILITY_DISABLED` |
| 9 | No fallback | none exists; `touch()===false` never reads the old hit; resolver returns refusals only | G8, G9 off-by-one → `UNSUPPORTED`, G13 | V6, V13 | `UNSUPPORTED` / `REFUSED_AMBIGUOUS` / `NO_HIT` |
| 10 | First-class refusals | refusal enum + reasons + UX text; selection unchanged on refusal | G9, G13 | V6 (P9–P12, P16–P19), V13 | named status, selection unchanged |
| 11 | Loud upgrade tests | G1–G4, G11 | G1–G3 in `npm run check` | G4, G11 in QA | test fails / disabled with P-row reason |
| 12 | Tree/Inspector work when disabled | disable gates picking only; Code workspace inert | G10 | V8, V11 | picking disabled; tree/Inspector/source/rendering unaffected |

**12/12 COVERED**, including the coordinator (rows 3, 5), abort (3, 4, 7),
displaced wrapper (4, 8), `touch() === false` (9, 10) and source forms (6).

## Electron / visual QA matrix (#31)

| id | case | expected |
|---|---|---|
| V1 | runtime version | `browser.version === '15.1.10'` |
| V2 | hook lifecycle: success, parse error | `VRMLParser.prototype.nodeStatement` is the original after each |
| V3 | **two X_ITE browsers sharing one parser prototype** (Mall `#preview` + World `#wpCanvas`), **overlapping provenance requests** | requests serialize; never two wrappers; no cross-generation records |
| V4 | stale: after reload, after edit (span shift via WD1.2 edit), last-valid after a syntax error, Fit mode, Show saved | `REFUSED_STALE` (or `preview-is-not-the-document` for Fit) |
| V5 | click vs drag-navigate | click selects; drag selects nothing |
| V6 | the C0 fixture matrix below, through real pointer events | every row as expected; WRONG = 0 |
| V7 | **LF, CRLF, UTF-8 BOM, Unicode** documents | PROVEN with exact spans, or fail-closed with no generation; never wrong |
| V8 | forced incompatibility (one P-surface removed in the harness) | persistent status line; source, tree + Inspector selection, Apply and rendering still work |
| V9 | leak: close session, switch document | no wrapper, no listener, no coordinator owner, adapter generation null |
| V10 | coordinate layouts from spike §11 incl. UI zoom | PROVEN to the same object as C0 |
| V11 | **Code workspace inert** | no listener attached, no `touch()`, clicking the preview changes no selection |
| V12 | **displaced wrapper ownership** (harness installs its own wrapper mid-parse) | replacement left in place; picking disabled with `parser-hook-displaced`; rendering continues |
| V13 | **`touch()` false with an old hit retained** (click while the viewer is active, after a prior valid pick) | `NO_HIT`; previous object not selected |
| V14 | **abort during asynchronous parse** and **late completion after abort** (stop / supersede mid-parse) | original hook restored at abort; late completion publishes nothing; next parse proceeds normally |
| V15 | **Model workspace active** | picking armed when compatible; PROVEN clicks select via `sceneSelection`; caret does not move |

Target: **0 confident wrong selections.**

## C0 fixture matrix (production expectations)

| fixture | expected |
|---|---|
| P1 root Shape / background | PROVEN Shape / NO_HIT |
| P2 Transform, no Appearance | PROVEN, promoted to Transform |
| P3 WD2-C Box | PROVEN, promoted to its Transform |
| P4 WD2-C Box + Sphere | each PROVEN to its own Transform |
| P5 anonymous twins | PROVEN, never swapped |
| P6 12 siblings × 3 orders | 36/36 PROVEN |
| P7 identical nested structures | PROVEN to the inner Transform |
| P8 DEF without USE | PROVEN |
| P9, P10 DEF + USE(s), incl. the DEF original | REFUSED_AMBIGUOUS |
| P11 shared geometry | A PROVEN (promoted); B PROVEN as Shape |
| P12 shared Shape | REFUSED_AMBIGUOUS |
| P13 Transform with two Shapes / P14 Group | each Shape PROVEN, no promotion |
| P15 Switch / hidden child area | shown child PROVEN / NO_HIT |
| P16 Inline | local PROVEN; Inline child REFUSED_EXTERNAL |
| P17 TouchSensor / P18 Anchor | REFUSED_SENSOR_CONFLICT; plain sibling PROVEN |
| P19 PROTO instance | UNSUPPORTED `proto-instance` |
| P20 occlusion / P21 transparent / P22 rotated camera | front-most PROVEN, as C0 |
| stale after reload / after edit | REFUSED_STALE |
| no-hit controls | NO_HIT |
| compatibility failure | COMPATIBILITY_DISABLED; tree selection works |

Target: **0 confident wrong selections.**

## USE, Inline and PROTO

- **USE:** every instance returns the same runtime Shape; the hit carries no
  instance path, only `modelViewMatrix`, and matrix matching is forbidden. Any
  node on the chain with more than one occurrence or more than one live parent
  → `REFUSED_AMBIGUOUS`, **including the DEF original**. A DEF with no USE is
  provable.
- **Inline:** content lives in the Inline's own `X3DScene` →
  `REFUSED_EXTERNAL`. Its children are never mapped into the containing text.
- **PROTO / EXTERNPROTO:** instance bodies are a separate execution context,
  copied per instance, and their records are dropped at parse time →
  `UNSUPPORTED` `proto-instance`. No mapping into expanded instances. Visual
  PROTO work is WD2-J.

## Performance

No second parse: X_ITE's existing preview parse carries the hook, and the
source side reuses the editor's existing analysis (`a.parseResult`,
`S.sceneTree`, `S.analysisSession`). The resolver's exact-span index and
containment map are built lazily on the first pick per analysis and cached
by object identity. C0 cost (SwiftShader, pessimistic): hook overhead within
noise at 100 objects, +15–30% parse time at 1,000; `touch()` median 10 ms at
1,000; resolve < 1 ms. Retained: one `WeakMap` entry per authored node of the
active generation.

## Security

No change: no IPC channel, no preload capability, no main-process code, no CSP
edit, no network or filesystem access. The adapter wraps a method of X_ITE's
own parser for the duration of one parse in the existing renderer page.

## Deferred scope

Model/Play toggle and sensor-dispatch suppression (UI-0), opening Inline
documents, PROTO-instance selection (WD2-J), USE-occurrence selection (needs
an X_ITE instance path that does not exist), multi-selection, transform
gizmos (WD2-E), Windows/macOS/real-GPU picking measurement (part of #31),
any X_ITE upgrade.
