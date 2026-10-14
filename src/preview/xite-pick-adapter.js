'use strict';
// WD2-D -- the ONE narrow X_ITE picking adapter (WD2_D_XITE_PICKING_CONTRACT.md).
//
// Every private X_ITE surface WD2-D depends on is touched here and nowhere
// else (a source-scan test enforces it). The adapter does exactly three things:
//
//   1. PROVENANCE. While -- and only while -- X_ITE parses one exact preview
//      string, X3D.VRMLParser.prototype.nodeStatement is wrapped so every node
//      statement it consumes is recorded as { runtime node, [start,end) } in
//      the parser's own offsets into that very string. The wrapper is
//      installed by the owner of one shared hook coordinator, restored in a
//      `finally` (or synchronously by abort()), and never restored over a
//      wrapper it does not own. Nothing is added to or rewritten in the text.
//   2. HIT SNAPSHOT. A click becomes browser.touch() + browser.getHit(), and
//      the hit Shape's runtime parent chain is copied into PLAIN DATA (labels,
//      types, occurrences). No runtime object leaves pick().
//   3. SPAN BINDING (VISUAL-3A1 amendment). The reverse lookup for the Tauri
//      Move tool: the ONE runtime node a generation recorded for an exact
//      span (snapshotSpan: plain data for Rust to prove; nodeAt: the node,
//      for the trusted JS layer only; spansOf: a node's occurrences).
//
// Mapping a snapshot to a source occurrence is NOT done here: that is the
// pure resolver src/editor/viewport-pick.js, which never touches X_ITE.
//
// Shared state is limited to the hook coordinator's arbitration state (owner
// token, FIFO waiters, page-lifetime `disabled`). Captures, records, runtime
// maps and generations are per parse / per adapter -- never module state.
//
// Dual use, like src/editor/scene-selection.js: CommonJS for node:test (driven
// with fakes), a classic <script defer> on editor.html publishing
// window.WrlXitePickAdapter. Everything lives inside one function scope so no
// top-level name can collide in the page's shared script scope.

(function () {
  // The exact X_ITE builds every private-surface assumption below was proven
  // on, one per host application (XITE-16-UPGRADE-2): 15.1.10 is the Electron
  // app's pin (root package.json), 16.5.0 the Rust/Tauri app's (apps/
  // desktop-tauri/xite/PIN, proven in XITE-16-UPGRADE-1). Exact strings only:
  // no range, no prefix, no newer-is-fine. Each host's build refuses any
  // other X_ITE, and the G4 probe still runs on whichever build is loaded.
  const XITE_VERSIONS = Object.freeze(['15.1.10', '16.5.0']);

  // ---- shared parser-hook coordinator ---------------------------------------
  // Both X_ITE browsers on the page share ONE VRMLParser.prototype, so ONE
  // module-private coordinator serializes hook ownership across all adapters:
  // FIFO, one owner at a time, page-lifetime `disabled` for structural failure.
  // It is created exactly once per module instance (= per page/realm) and is
  // neither exported nor injectable: every adapter uses this one.
  const hookCoordinator = (function () {
    let owner = null;
    const waiters = []; // FIFO of { token, resolve }
    let disabled = null;

    function wake() {
      while (owner === null && waiters.length) {
        const w = waiters.shift();
        if (disabled) { w.resolve('disabled'); continue; }
        owner = w.token;
        w.resolve('owned');
      }
      if (disabled) while (waiters.length) waiters.shift().resolve('disabled');
    }

    function acquire(token) {
      if (disabled) return Promise.resolve('disabled');
      return new Promise((resolve) => {
        waiters.push({ token, resolve });
        wake();
      });
    }

    // Idempotent; acts only for the current owner.
    function release(token) {
      if (owner !== token) return false;
      owner = null;
      wake();
      return true;
    }

    // Removes a still-waiting token; its acquire() resolves 'cancelled'.
    function cancel(token) {
      for (let i = 0; i < waiters.length; i++) {
        if (waiters[i].token !== token) continue;
        const [w] = waiters.splice(i, 1);
        w.resolve('cancelled');
        return true;
      }
      return false;
    }

    function disable(reason) {
      if (!disabled) disabled = String(reason || 'disabled');
      wake();
    }

    return Object.freeze({ acquire, release, cancel, disable, disabledReason: () => disabled });
  })();

  // ---- compatibility (contract P-rows) ---------------------------------------
  const fn = (o, k) => !!o && typeof o[k] === 'function';

  // Parser-side surfaces are SHARED by every browser on the page: a failure
  // there disables the coordinator (page lifetime, every adapter).
  function checkParserSurfaces(X3D) {
    if (!X3D || typeof X3D.VRMLParser !== 'function') return 'parser-class-missing'; // P1
    const proto = X3D.VRMLParser.prototype;
    if (!proto || !Object.prototype.hasOwnProperty.call(proto, 'nodeStatement')
      || typeof proto.nodeStatement !== 'function' || proto.nodeStatement.length !== 0) return 'parser-hook-missing'; // P2
    if (!fn(proto, 'comments')) return 'parser-comments-missing'; // P3
    if (!fn(proto, 'getScene') || !fn(proto, 'getExecutionContext') || !fn(proto, 'isInsideProtoDeclaration')) {
      return 'parser-context-missing'; // P5
    }
    if (typeof X3D.X3DBaseNode !== 'function' || typeof X3D.X3DExecutionContext !== 'function'
      || typeof X3D.X3DScene !== 'function') return 'class-missing'; // P12
    return null;
  }

  // Browser-side surfaces belong to one adapter.
  function checkBrowserSurfaces(browser) {
    if (!browser) return 'browser-missing';
    if (typeof browser.version !== 'string' || !XITE_VERSIONS.includes(browser.version)) return 'xite-version-mismatch'; // V1 / G2
    if (!fn(browser, 'createX3DFromString') || !fn(browser, 'replaceWorld')) return 'browser-public-api-missing';
    if (!fn(browser, 'touch') || browser.touch.length !== 2) return 'touch-missing'; // P6
    if (!fn(browser, 'getHit')) return 'hit-shape-changed'; // P7
    if (!fn(browser, 'getWorld')) return 'world-infrastructure-missing'; // P10
    if (!fn(browser, 'getViewport')) return 'viewport-missing'; // P11
    if (!browser.element || typeof browser.element.getBoundingClientRect !== 'function') return 'viewport-missing';
    return null;
  }

  // G4 probe: a fixed string whose node spans are known by construction. Run
  // through the SAME provenance path production uses; any mismatch disables.
  // Leading whitespace, a comment and non-ASCII text precede the first node.
  // Spans are known by concatenation, never searched for.
  const PROBE_HEAD = '#VRML V2.0 utf8\n# probe é ✓\n  ';
  const PROBE_BOX = 'Box { }';
  const PROBE_SHAPE_HEAD = 'Shape { geometry ';
  const PROBE_SHAPE = `${PROBE_SHAPE_HEAD}${PROBE_BOX} }`;
  const PROBE_TRANSFORM_HEAD = 'DEF WD2DProbe Transform { children [ ';
  const PROBE_TRANSFORM = `${PROBE_TRANSFORM_HEAD}${PROBE_SHAPE} ] }`;
  const PROBE_TEXT = `${PROBE_HEAD}${PROBE_TRANSFORM}\n`;
  function probeExpectations() {
    const t = PROBE_HEAD.length;
    const s = t + PROBE_TRANSFORM_HEAD.length;
    const b = s + PROBE_SHAPE_HEAD.length;
    return [
      { type: 'Transform', start: t, end: t + PROBE_TRANSFORM.length },
      { type: 'Shape', start: s, end: s + PROBE_SHAPE.length },
      { type: 'Box', start: b, end: b + PROBE_BOX.length },
    ];
  }

  // ---- the adapter -------------------------------------------------------------
  function createXitePickAdapter({ X3D, browser } = {}) {
    const coordinator = hookCoordinator;
    let liveBrowser = browser || null;
    let compat = { state: 'unknown', reason: null };
    let live = null;              // this adapter's one live (waiting/owning) token
    let pending = null;           // a minted generation not yet activated
    let active = null;            // the displayed generation
    let retiredReason = 'preview-shows-last-valid-scene';
    const gens = new WeakMap();   // generation token -> { scene, occ }
    let disposed = false;

    function structuralDisable(reason, shared) {
      if (compat.state !== 'disabled') compat = { state: 'disabled', reason };
      if (shared) coordinator.disable(reason);
      retireActive();
    }

    function compatibility() {
      const shared = coordinator.disabledReason();
      if (shared) return { ok: false, reason: shared };
      if (disposed) return { ok: false, reason: 'adapter-disposed' };
      if (compat.state === 'disabled') return { ok: false, reason: compat.reason };
      if (compat.state !== 'compatible') return { ok: false, reason: 'compatibility-unproven' };
      return { ok: true };
    }

    // Synchronous structural checks at creation.
    {
      const parserFail = checkParserSurfaces(X3D);
      const browserFail = parserFail ? null : checkBrowserSurfaces(liveBrowser);
      if (parserFail) structuralDisable(parserFail, true);
      else if (browserFail) structuralDisable(browserFail, false);
    }

    function retireActive() {
      if (active) gens.delete(active);
      active = null;
    }

    // ---- parse with provenance -------------------------------------------------
    function makeWrapper(original, capture) {
      const BaseNode = X3D.X3DBaseNode;
      return function wd2dNodeStatement() {
        if (capture.closed || this.input !== capture.text) return original.apply(this, arguments);
        this.comments(); // idempotent: the original calls it first, too
        const start = this.lastIndex;
        const node = original.apply(this, arguments); // its exceptions propagate unchanged
        try {
          if (!capture.closed && node && typeof node === 'object' && node instanceof BaseNode) {
            capture.records.push({
              node, start, end: this.lastIndex,
              scene: this.getScene(), ctx: this.getExecutionContext(),
              inProto: !!this.isInsideProtoDeclaration(),
            });
          }
        } catch (e) {
          capture.poisoned = true; // recording never breaks the parse
        }
        return node;
      };
    }

    // B1 -- PROTO instance provenance. X_ITE builds every PROTO instance body
    // by copying the declaration body's nodes (X3DNode.prototype.copy). While
    // -- and only while -- one provenance parse owns the parser hook, `copy`
    // is wrapped so every copy records the ONE node it was copied from. A
    // runtime node of an instance body is then joined to the exact span of
    // its declaration statement. Optional: when the hook cannot be installed,
    // is displaced, or one copy result is claimed by two originals, PROTO
    // bodies simply have no provenance (every such pick refuses).
    function installCopyHook(token) {
      try {
        const NodeProto = X3D.X3DNode && X3D.X3DNode.prototype;
        if (!NodeProto || !Object.prototype.hasOwnProperty.call(NodeProto, 'copy')
          || typeof NodeProto.copy !== 'function') return;
        const BaseNode = X3D.X3DBaseNode;
        const capture = token.capture;
        const original = NodeProto.copy;
        capture.copies = new WeakMap();
        capture.copyConflict = new WeakSet();
        // B3: the same joins, in order, so a declaration node's copies (one
        // per PROTO instance) can be listed. Per parse, never module state.
        capture.copyPairs = [];
        const wrapper = function wrlforgeCopy() {
          const copy = original.apply(this, arguments); // exceptions propagate unchanged
          try {
            if (!capture.closed && copy && copy !== this && copy instanceof BaseNode) {
              const prev = capture.copies.get(copy);
              if (prev && prev !== this) capture.copyConflict.add(copy);
              else { capture.copies.set(copy, this); capture.copyPairs.push([copy, this]); }
            }
          } catch (e) {
            capture.copies = null; // recording never breaks the copy
          }
          return copy;
        };
        // The FIRST statement of a PROTO body is parsed by
        // rootNodeStatement, not nodeStatement: record it the same way, so
        // the body's root node has its exact span too.
        const VP = X3D.VRMLParser.prototype;
        let root = null;
        if (Object.prototype.hasOwnProperty.call(VP, 'rootNodeStatement')
          && typeof VP.rootNodeStatement === 'function' && VP.rootNodeStatement.length === 0) {
          root = { original: VP.rootNodeStatement, wrapper: makeWrapper(VP.rootNodeStatement, capture) };
          VP.rootNodeStatement = root.wrapper;
        }
        NodeProto.copy = wrapper;
        token.copyHook = { NodeProto, original, wrapper, root };
      } catch (e) {
        token.capture.copies = null;
      }
    }

    function removeCopyHook(token) {
      const h = token.copyHook;
      if (!h) return;
      token.copyHook = null;
      if (h.NodeProto.copy === h.wrapper) h.NodeProto.copy = h.original;
      else token.capture.copies = null; // displaced: prove nothing
      if (h.root) {
        const VP = X3D.VRMLParser.prototype;
        if (VP.rootNodeStatement === h.root.wrapper) VP.rootNodeStatement = h.root.original;
        else token.capture.copies = null;
      }
    }

    // The `finally` of one provenance parse. Writes to the prototype only to
    // remove ITS OWN wrapper; a displaced wrapper is left in place.
    function settle(token) {
      if (token.state !== 'owning') return; // aborted / lost: already handled
      const proto = X3D.VRMLParser.prototype;
      if (proto.nodeStatement === token.wrapper) {
        proto.nodeStatement = token.original;
        token.state = 'completed';
      } else {
        token.state = 'lost';
        structuralDisable('parser-hook-displaced', true);
      }
      removeCopyHook(token);
      token.capture.closed = true;
      coordinator.release(token);
    }

    // Synchronous abort of one token (contract §Abort, steps 1-5).
    function abortToken(token) {
      if (!token) return;
      if (token.state === 'owning') {
        const proto = X3D.VRMLParser.prototype;
        if (proto.nodeStatement === token.wrapper) {
          proto.nodeStatement = token.original;
          token.state = 'aborted';
        } else {
          token.state = 'lost';
          structuralDisable('parser-hook-displaced', true);
        }
        removeCopyHook(token);
        token.capture.closed = true;
      } else if (token.state === 'waiting') {
        token.state = 'aborted';
      }
      if (!coordinator.release(token)) coordinator.cancel(token);
      if (live === token) live = null;
    }

    async function plainParse(text, reason) {
      const scene = await liveBrowser.createX3DFromString(text);
      return { scene, generation: null, reason };
    }

    // `isProbe`: the G4 probe's token is never this adapter's `live` request,
    // so an editor abort()/supersession cannot cancel the compatibility proof.
    async function provenanceParse(text, meta, isProbe) {
      if (!isProbe && live) abortToken(live); // same-adapter supersession
      const token = { state: 'waiting', wrapper: null, original: null, capture: null };
      if (!isProbe) live = token;
      const r = await coordinator.acquire(token);
      if (r === 'cancelled' || token.state === 'aborted') {
        coordinator.release(token); // owned-then-aborted before this continuation ran
        if (live === token) live = null;
        return { scene: null, generation: null, reason: 'cancelled' };
      }
      if (r !== 'owned') {
        if (live === token) live = null;
        return plainParse(text, compatibility().reason || 'parser-hook-unavailable');
      }

      const proto = X3D.VRMLParser.prototype;
      try {
        const original = proto.nodeStatement;
        if (typeof original !== 'function') throw new Error('parser-hook-missing');
        const capture = { text, records: [], poisoned: false, closed: false };
        token.original = original;
        token.capture = capture;
        token.wrapper = makeWrapper(original, capture);
        proto.nodeStatement = token.wrapper;
        token.state = 'owning';
        installCopyHook(token);
      } catch (e) {
        if (token.state === 'owning') abortToken(token);
        else { token.state = 'lost'; coordinator.release(token); }
        if (live === token) live = null;
        structuralDisable('parser-hook-missing', true);
        return plainParse(text, 'parser-hook-missing');
      }

      let scene;
      try {
        scene = await liveBrowser.createX3DFromString(text);
      } finally {
        settle(token);
        if (live === token) live = null;
      }
      if (token.state !== 'completed' || token.capture.poisoned) {
        return { scene, generation: null, reason: 'generation-unprovable' };
      }
      try {
        const occ = new WeakMap();
        // VISUAL-3A1: exact span -> the runtime node(s) recorded for it.
        const bySpan = new Map();
        let kept = 0;
        for (const rec of token.capture.records) {
          if (rec.scene !== scene || rec.ctx !== scene || rec.inProto) continue;
          let list = occ.get(rec.node);
          if (!list) { list = []; occ.set(rec.node, list); }
          list.push(Object.freeze({ start: rec.start, end: rec.end }));
          const key = `${rec.start}:${rec.end}`;
          let nodes = bySpan.get(key);
          if (!nodes) { nodes = new Set(); bySpan.set(key, nodes); }
          nodes.add(rec.node);
          kept += 1;
        }
        const spans = isProbe ? token.capture.records
          .filter((rec) => rec.scene === scene && rec.ctx === scene && !rec.inProto)
          .map((rec) => ({ type: typeOf(rec.node), start: rec.start, end: rec.end })) : null;
        // B1: declaration-body statements of a PROTO declared directly in
        // this scene, keyed by the declaration node. Nested PROTOs: none.
        const protoOcc = new WeakMap();
        // B3: exact span -> declaration-body node(s) recorded for it.
        const protoBySpan = new Map();
        if (token.capture.copies) {
          for (const rec of token.capture.records) {
            if (rec.scene !== scene || !rec.inProto || !rec.ctx) continue;
            let outer = null;
            try { outer = fn(rec.ctx, 'getOuterNode') ? rec.ctx.getOuterNode() : null; } catch (e) { outer = null; }
            if (!outer || !fn(outer, 'getBody') || outer.getBody() !== rec.ctx) continue;
            if (!fn(outer, 'getExecutionContext') || outer.getExecutionContext() !== scene) continue;
            if (!fn(outer, 'getProtoDeclaration') || outer.getProtoDeclaration() !== outer) continue;
            let list = protoOcc.get(rec.node);
            if (!list) { list = []; protoOcc.set(rec.node, list); }
            list.push(Object.freeze({ start: rec.start, end: rec.end }));
            const key = `${rec.start}:${rec.end}`;
            let decl = protoBySpan.get(key);
            if (!decl) { decl = new Set(); protoBySpan.set(key, decl); }
            decl.add(rec.node);
          }
        }
        // B3: declaration node -> its copies (the instance-body nodes).
        const copiesOf = new Map();
        if (token.capture.copies && token.capture.copyPairs) {
          for (const [copy, from] of token.capture.copyPairs) {
            if (token.capture.copyConflict.has(copy)) continue;
            // X_ITE may return the SAME copy twice within one instance
            // (named nodes are copied once, then reused): one candidate.
            let list = copiesOf.get(from);
            if (!list) { list = []; copiesOf.set(from, list); }
            if (!list.includes(copy)) list.push(copy);
          }
        }
        if (token.capture.copyPairs) token.capture.copyPairs.length = 0;
        const copyOf = token.capture.copies;
        const copyConflict = token.capture.copyConflict;
        token.capture.records.length = 0;
        const generation = Object.freeze({
          sessionId: meta.sessionId == null ? null : meta.sessionId,
          overlayGeneration: meta.generationId == null ? null : meta.generationId,
          text,
        });
        gens.set(generation, { scene, occ, bySpan, kept, spans, protoOcc, copyOf, copyConflict, protoBySpan, copiesOf, lastHit: null });
        if (!isProbe) pending = generation;
        return { scene, generation };
      } catch (e) {
        return { scene, generation: null, reason: 'generation-unprovable' };
      }
    }

    // G4: verify the parser's offsets on a fixed string before trusting them.
    async function runProbe() {
      try {
        const res = await provenanceParse(PROBE_TEXT, { sessionId: null, generationId: 'probe' }, true);
        const g = res.generation && gens.get(res.generation);
        if (res.generation) gens.delete(res.generation);
        if (compat.state === 'disabled') return;
        // settle() already restored the original (or disabled on displacement).
        const restored = !!g && !coordinator.disabledReason();
        const want = probeExpectations();
        const spans = g ? g.spans : null;
        // Statements are recorded on completion (inner before outer); compare
        // as a set of exact {type,start,end} triples.
        const ok = restored && !!spans && spans.length === want.length && want.every((w) => spans.filter((s) => (
          s.type === w.type && s.start === w.start && s.end === w.end)).length === 1);
        if (!ok) { structuralDisable('parser-offsets-unproven', true); return; }
        compat = { state: 'compatible', reason: null };
      } catch (e) {
        if (compat.state !== 'disabled') structuralDisable('parser-offsets-unproven', true);
      }
    }

    const probed = compat.state === 'unknown' ? runProbe() : Promise.resolve();

    // Replaces browser.createX3DFromString(text) for the editor live preview.
    async function parseWithProvenance(text, meta = {}) {
      await probed;
      if (disposed) return { scene: null, generation: null, reason: 'cancelled' };
      const c = compatibility();
      if (!c.ok) return plainParse(text, c.reason);
      return provenanceParse(String(text), meta || {});
    }

    function activate(generation, scene) {
      if (!generation || generation !== pending) return false;
      const g = gens.get(generation);
      if (!g || g.scene !== scene) return false;
      retireActive();
      pending = null;
      active = generation;
      return true;
    }

    function retire(reason) {
      retireActive();
      retiredReason = reason || 'preview-shows-last-valid-scene';
    }

    function abort() {
      if (live) abortToken(live);
      if (pending) { gens.delete(pending); pending = null; }
    }

    function dispose() {
      abort();
      retire('preview-scene-replaced');
      removeGate();
      disposed = true;
      liveBrowser = null;
    }

    // ---- scene input gate (LINUX-EDIT-INPUT-SAFETY) --------------------------
    // X_ITE's PointingDevice calls browser.buttonPressEvent / motionNotifyEvent
    // from its own mousedown / mousemove listeners on the canvas, BEFORE the
    // editor learns the click is a pick: a TouchSensor fired its touchTime (and
    // every ROUTE behind it) while Select was active. While the editor owns the
    // pointer -- every state except Interact -- both entry points answer "no
    // sensor" without activating or hovering one: TouchSensors, drag sensors
    // and Anchors stay quiet, and the press falls through to the viewer, so a
    // drag that starts on a sensor orbits. Release is never gated: nothing is
    // activated while closed, and a press made in Interact finishes normally.
    // touch() / getHit() are not gated, so pick refusals still see the authored
    // sensors. The wrappers are own properties of this one browser; dispose()
    // removes them. Installed (closed) by the first sceneInput() call, so a
    // host that never asks (the Electron editor) keeps X_ITE's own behavior.
    // PR #149 review: every sceneInput() call VERIFIES that both entry points
    // are still this adapter's wrappers. A missing surface, a foreign wrapper,
    // a replaced wrapper or an install error reports unavailable and closes
    // the gate (the host must then block the pointer); the next call tries to
    // install again, so the gate recovers only through a passing check.
    const GATED = ['buttonPressEvent', 'motionNotifyEvent'];
    const gate = { open: false, wrappers: null, reason: 'pointer-sensor-gate-not-installed' };
    function gateVerified() {
      const b = liveBrowser;
      return !!(b && gate.wrappers && GATED.every((k) => b[k] === gate.wrappers[k]));
    }

    function installGate() {
      const b = liveBrowser;
      try {
        if (!b || !fn(b, 'buttonPressEvent') || !fn(b, 'motionNotifyEvent') || !fn(b, 'buttonReleaseEvent')) {
          gate.reason = 'pointer-sensor-entry-missing';
          return;
        }
        const mine = (k) => !!gate.wrappers && b[k] === gate.wrappers[k];
        if (GATED.some((k) => hasOwn(b, k) && !mine(k))) {
          gate.reason = 'pointer-sensor-entry-already-wrapped';
          return;
        }
        const wrappers = gate.wrappers || {};
        gate.wrappers = wrappers;
        for (const k of GATED) {
          if (hasOwn(b, k) && mine(k)) continue;
          const original = b[k];
          const wrapper = function () { return gate.open ? original.apply(this, arguments) : false; };
          wrappers[k] = wrapper;
          b[k] = wrapper;
        }
        if (!gateVerified()) gate.reason = 'pointer-sensor-gate-not-verified';
      } catch (e) {
        gate.reason = 'pointer-sensor-gate-install-error';
      }
    }

    function removeGate() {
      const b = liveBrowser;
      gate.open = false;
      gate.reason = 'adapter-disposed';
      if (!b || !gate.wrappers) return;
      for (const k of GATED) if (b[k] === gate.wrappers[k]) delete b[k];
      gate.wrappers = null;
    }

    // sceneInput(open?): open (true) only for the explicit Interact mode;
    // anything else closes. Returns 'interact' | 'edit' | 'unavailable:<why>'.
    function sceneInput(open) {
      if (disposed) return 'unavailable:adapter-disposed';
      if (!gateVerified()) {
        gate.open = false;
        installGate();
        if (!gateVerified()) return `unavailable:${gate.reason}`;
      }
      if (open !== undefined) gate.open = open === true;
      return gate.open ? 'interact' : 'edit';
    }

    // ---- pick ------------------------------------------------------------------
    const snap = (outcome, reason, generation, extra) => Object.freeze({ outcome, reason, generation: generation || null, ...(extra || {}) });

    function typeOf(node) {
      try { return fn(node, 'getTypeName') ? node.getTypeName() : null; } catch (e) { return null; }
    }

    // The runtime parent chain of `root` as PLAIN DATA (labels local to one
    // call): types, execution-context kinds, provenance occurrences and
    // parents, up to the scene. Shared by pick() and snapshotSpan().
    // {graph, label, ctxKind} | {disabled: reason} | {graph: null}.
    function snapshotGraph(g, root) {
      const world = liveBrowser.getWorld();
      const layer0 = world && fn(world, 'getLayer0') ? world.getLayer0() : null;
      if (!layer0 || !layer0.groupNode || !layer0.groupNodes) return { disabled: 'world-infrastructure-missing' };
      const infra = new Set([layer0, layer0.groupNode, layer0.groupNodes]);

      // Plain-data snapshot. Labels are local to this call.
      const labels = new Map();
      const label = (o) => { if (!labels.has(o)) labels.set(o, `n${labels.size + 1}`); return labels.get(o); };
      const ctxKind = (node) => {
        let ctx = null;
        try { ctx = fn(node, 'getExecutionContext') ? node.getExecutionContext() : null; } catch (e) { ctx = null; }
        if (!ctx) return 'none';
        if (ctx === g.scene) return 'document';
        if (ctx instanceof X3D.X3DScene) return 'external-scene';
        return 'proto-body';
      };
      const runtimeParents = (node) => {
        const nodes = new Set();
        const bodies = new Set();
        const ctxs = [];
        const seen = new Set();
        const stack = [...node.getParents()];
        while (stack.length) {
          const p = stack.pop();
          if (!p || seen.has(p)) continue;
          seen.add(p);
          if (p instanceof X3D.X3DExecutionContext) {
            if (p === g.scene) { ctxs.push('SCENE'); continue; }
            // B1: the body of a PROTO instance names its instance node.
            let outer = null;
            try { outer = fn(p, 'getOuterNode') ? p.getOuterNode() : null; } catch (e) { outer = null; }
            if (outer instanceof X3D.X3DBaseNode && fn(outer, 'getBody') && outer.getBody() === p
              && !(fn(outer, 'getProtoDeclaration') && outer.getProtoDeclaration() === outer)) {
              bodies.add(outer);
              continue;
            }
            ctxs.push('OTHER_CONTEXT');
            continue;
          }
          if (p instanceof X3D.X3DBaseNode) { nodes.add(p); continue; }
          if (fn(p, 'getParents')) for (const q of p.getParents()) stack.push(q);
        }
        return { nodes: [...nodes], bodies: [...bodies], ctxs };
      };
      // A PROTO instance body node's occurrences are those of the ONE
      // declaration node it was copied from (B1); none when unproven.
      const occurrencesOf = (n) => {
        const own = g.occ.get(n);
        if (own) return own;
        if (!g.copyOf || !g.protoOcc || g.copyConflict.has(n)) return [];
        const from = g.copyOf.get(n);
        return (from && g.protoOcc.get(from)) || [];
      };
      const graph = {};
      const queue = [root];
      try {
        while (queue.length && Object.keys(graph).length < 256) {
          const n = queue.shift();
          const l = label(n);
          if (graph[l]) continue;
          const isInfra = infra.has(n);
          const { nodes, bodies, ctxs } = isInfra ? { nodes: [], bodies: [], ctxs: [] } : runtimeParents(n);
          graph[l] = Object.freeze({
            type: typeOf(n),
            ctxKind: isInfra ? 'world-infrastructure' : ctxKind(n),
            occurrences: Object.freeze(occurrencesOf(n).slice()),
            parents: Object.freeze([...nodes.map(label), ...bodies.map((b) => `BODY:${label(b)}`), ...ctxs]),
          });
          for (const p of nodes) queue.push(p);
          for (const b of bodies) queue.push(b);
        }
      } catch (e) {
        return { graph: null };
      }
      return { graph: Object.freeze(graph), label, ctxKind };
    }

    // `layer` (P0-A): which Shape under the pointer, 0 = the nearest (the
    // plain pick), 1 = the one behind it, ... modulo the number found. The
    // snapshot reports `layer` (the one returned) and `layers` (how many).
    function pick(clientX, clientY, layer) {
      const c = compatibility();
      if (!c.ok) return snap('disabled', c.reason, null);
      if (!active) return snap('stale', retiredReason, null);
      const generation = active;
      const g = gens.get(generation);
      if (!g || liveBrowser.currentScene !== g.scene) return snap('stale', 'preview-scene-replaced', generation);

      // Client (CSS px) -> X_ITE touch space (viewport px, origin bottom-left),
      // spike §11. Never assume CSS px equal framebuffer px.
      // getViewport() is X_ITE's SFVec4f [x, y, w, h] field: indexable, not an Array.
      const rect = liveBrowser.element.getBoundingClientRect();
      const vp = liveBrowser.getViewport();
      const vw = vp ? Number(vp[2]) : NaN;
      const vh = vp ? Number(vp[3]) : NaN;
      if (!(vw > 0) || !(vh > 0) || !Number.isFinite(vw) || !Number.isFinite(vh)) {
        structuralDisable('viewport-missing', false);
        return snap('disabled', 'viewport-missing', null);
      }
      // A zero-size canvas (hidden pane) is no hit, not an incompatibility.
      if (!(rect.width > 0) || !(rect.height > 0)) return snap('no-hit', 'no-geometry-under-pointer', generation);
      const x = (clientX - rect.left) / rect.width * vw;
      const y = (1 - (clientY - rect.top) / rect.height) * vh;

      // touch() === false leaves the PREVIOUS hit in place: never read getHit().
      if (liveBrowser.touch(x, y) !== true) return snap('no-hit', 'viewer-active-or-no-hit', generation);

      const hit = liveBrowser.getHit();
      if (!hit || typeof hit !== 'object' || !('shapeNode' in hit) || !(hit.sensors instanceof Map)) {
        structuralDisable('hit-shape-changed', false);
        return snap('disabled', 'hit-shape-changed', null);
      }
      if (!(hit.id > 0) || !hit.shapeNode) return snap('no-hit', 'no-geometry-under-pointer', generation);
      const want = Number.isInteger(layer) && layer > 0 ? layer : 0;
      const layers = hitLayers(x, y, hit);
      const chosen = layers[want % layers.length];
      const shape = chosen.shape;
      if (!fn(shape, 'getParents') || !fn(shape, 'getExecutionContext')) {
        structuralDisable('runtime-parents-unavailable', false);
        return snap('unsupported', 'runtime-parents-unavailable', generation);
      }
      if (!fn(shape, 'getTypeName')) return snap('unsupported', 'runtime-node-type-unavailable', generation);

      const r = snapshotGraph(g, shape);
      if (r.disabled) {
        structuralDisable(r.disabled, false);
        return snap('disabled', r.disabled, null);
      }
      if (!r.graph) return snap('unsupported', 'runtime-parents-unavailable', generation);
      const { graph, label, ctxKind } = r;
      // B3: the hit Shape, kept for the TRUSTED layer only (bodyNodeAt):
      // which PROTO instance the user clicked. Never returned.
      g.lastHit = shape;
      return snap('hit', null, generation, {
        shape: label(shape),
        ctxKind: ctxKind(shape),
        sensors: Object.freeze(chosen.sensors.map(typeOf)),
        graph,
        layer: want % layers.length,
        layers: layers.length,
      });
    }

    // ---- P0-A hit layers (depth peeling) ---------------------------------------
    // X_ITE 16.5.0's touch() is a GPU id pass: getWorld().traverse(POINTER)
    // makes every pointing Shape call renderObject.addPointingShape(this) from
    // Shape.prototype.traverse, the shapes are drawn into a 1x1 id/depth
    // buffer and getHit() reports the ONE nearest id -- a transparent coat
    // over opaque paint always wins. There is no hit list to read. To reach
    // the shapes BEHIND it, the hit Shape is taken out of the pointer pass by
    // shadowing its `traverse` with an OWN no-op, touch() is repeated, and the
    // next-nearest Shape answers; and so on. All of it runs synchronously
    // (touch() renders and reads its pixel without yielding), so no frame is
    // drawn and no event is processed with a Shape missing, and every shadow
    // is deleted in `finally` -- after which the frontmost hit is re-touched
    // so X_ITE's hit state is exactly what a plain touch() leaves.
    const MAX_LAYERS = 8;
    const hasOwn = (o, k) => Object.prototype.hasOwnProperty.call(o, k);
    function quietTraverse() {}
    function hideFromPointer(shape) {
      if (!shape || typeof shape !== 'object' || hasOwn(shape, 'traverse') || !fn(shape, 'traverse')) return false;
      try {
        Object.defineProperty(shape, 'traverse', { value: quietTraverse, configurable: true, writable: true });
      } catch (e) {
        return false;
      }
      return shape.traverse === quietTraverse;
    }
    function unhide(shape) {
      if (hasOwn(shape, 'traverse') && shape.traverse === quietTraverse) delete shape.traverse;
    }
    // The Shapes under (x, y), front to back, each with the sensors its own
    // touch() reported. `hit` is the record getHit() returned for the plain
    // touch: X_ITE keeps ONE hit record per browser (getHit() returns
    // this[_hit]; touch(x, y, hit = this[_hit]) rewrites it), so it is read
    // again after every strict-true touch() rather than calling getHit()
    // twice. Were it ever a copy, the front Shape would "answer again" and
    // the list would stop at one layer -- the plain pick, never a guess.
    function hitLayers(x, y, hit) {
      const layers = [{ shape: hit.shapeNode, sensors: [...hit.sensors.keys()] }];
      const hidden = [];
      try {
        while (layers.length < MAX_LAYERS) {
          const front = layers[layers.length - 1].shape;
          if (!hideFromPointer(front)) break;
          hidden.push(front);
          if (liveBrowser.touch(x, y) !== true) break;
          if (!(hit.id > 0) || !hit.shapeNode || !(hit.sensors instanceof Map)) break;
          // A Shape already listed answering again means hiding did not
          // work: stop rather than list it twice.
          const next = hit.shapeNode;
          if (layers.some((l) => l.shape === next)) break;
          layers.push({ shape: next, sensors: [...hit.sensors.keys()] });
        }
      } finally {
        for (const s of hidden) unhide(s);
        if (hidden.length) liveBrowser.touch(x, y);
      }
      return layers;
    }

    // ---- VISUAL-3A1 runtime binding (span -> runtime node) ----------------------
    // The reverse of pick(): the Move tool names an AUTHORED node by its exact
    // provenance span and asks which runtime node X_ITE's parser created for
    // it. Same generation rules as pick(): the generation must be one this
    // adapter minted (pending or active) and its scene must be the one on
    // screen. Nothing is searched: the span must match a recorded statement
    // exactly, both ends, and be carried by exactly ONE runtime node.

    // The generation's record, or null when it may not be used now.
    function usable(generation) {
      if (!compatibility().ok || !generation) return null;
      const g = gens.get(generation);
      if (!g || !g.bySpan || liveBrowser.currentScene !== g.scene) return null;
      return g;
    }

    function spanNodes(g, start, end) {
      if (!Number.isInteger(start) || !Number.isInteger(end) || start < 0 || end <= start) return [];
      const set = g.bySpan.get(`${start}:${end}`);
      return set ? [...set] : [];
    }

    // PLAIN DATA for Rust to prove (wrlforge_vrml::pick::resolve_node): the
    // located node's runtime chain, exactly as pick() snapshots a hit Shape.
    function snapshotSpan(generation, start, end) {
      const c = compatibility();
      if (!c.ok) return snap('disabled', c.reason, null);
      const g = usable(generation);
      if (!g) return snap('stale', 'preview-scene-replaced', generation);
      const nodes = spanNodes(g, start, end);
      if (nodes.length !== 1) return snap('unsupported', 'runtime-node-for-span-not-unique', generation, { count: nodes.length });
      const node = nodes[0];
      if (!fn(node, 'getParents') || !fn(node, 'getExecutionContext')) return snap('unsupported', 'runtime-parents-unavailable', generation);
      const r = snapshotGraph(g, node);
      if (r.disabled) {
        structuralDisable(r.disabled, false);
        return snap('disabled', r.disabled, null);
      }
      if (!r.graph) return snap('unsupported', 'runtime-parents-unavailable', generation);
      return snap('found', null, generation, {
        shape: r.label(node), ctxKind: r.ctxKind(node), sensors: Object.freeze([]), graph: r.graph,
      });
    }

    // The runtime node for the span -- for the TRUSTED JavaScript layer only
    // (the gizmo adapter), never across the Rust/Wasm boundary. null unless
    // exactly one node carries the span in a usable generation.
    function nodeAt(generation, start, end) {
      const g = usable(generation);
      if (!g) return null;
      const nodes = spanNodes(g, start, end);
      return nodes.length === 1 ? nodes[0] : null;
    }

    // The provenance occurrences of a runtime node (plain data), or null.
    function spansOf(generation, node) {
      const g = usable(generation);
      if (!g || !node || typeof node !== 'object') return null;
      return Object.freeze((g.occ.get(node) || []).map((o) => Object.freeze({ start: o.start, end: o.end })));
    }

    // ---- B3 PROTO body binding (span -> the PICKED instance's node) -----------
    // A PROTO body statement has no runtime node of its own in the scene: X_ITE
    // copies the declaration body once per instance. The span names exactly
    // ONE declaration node (recorded while parsing); its copies are the
    // candidates. One copy (one instance): that one. Several: the one whose
    // instance the newest pick of THIS generation hit (the copy is a runtime
    // ancestor of the hit Shape), when exactly one is. Anything else: null,
    // never a guess.

    // True when `node` is `shape` or one of its runtime ancestors, climbing
    // node parents and PROTO instance bodies (to their instance node).
    function encloses(node, shape) {
      const seen = new Set();
      const stack = [shape];
      while (stack.length && seen.size < 4096) {
        const n = stack.pop();
        if (!n || seen.has(n)) continue;
        seen.add(n);
        if (n === node) return true;
        if (n instanceof X3D.X3DExecutionContext) {
          let outer = null;
          try { outer = fn(n, 'getOuterNode') ? n.getOuterNode() : null; } catch (e) { outer = null; }
          if (outer) stack.push(outer);
          continue;
        }
        if (fn(n, 'getParents')) for (const q of n.getParents()) stack.push(q);
      }
      return false;
    }

    // B3 (LV2): the DOCUMENT instance a body copy belongs to, as
    // { node, start, end } (its ONE provenance occurrence), or null. Climbs
    // the copy's runtime parents to its body execution context; the context
    // must be the body of exactly ONE instance node that is itself a
    // document node with exactly one recorded occurrence. Never a search by
    // value, name or position.
    function instanceOf(g, copy) {
      const outers = new Set();
      const seen = new Set();
      const stack = [copy];
      while (stack.length && seen.size < 4096) {
        const n = stack.pop();
        if (!n || seen.has(n)) continue;
        seen.add(n);
        if (n instanceof X3D.X3DExecutionContext) {
          if (n === g.scene) return null; // not inside an instance body
          let outer = null;
          try { outer = fn(n, 'getOuterNode') ? n.getOuterNode() : null; } catch (e) { outer = null; }
          if (outer instanceof X3D.X3DBaseNode && fn(outer, 'getBody') && outer.getBody() === n
            && !(fn(outer, 'getProtoDeclaration') && outer.getProtoDeclaration() === outer)) outers.add(outer);
          else return null;
          continue;
        }
        if (fn(n, 'getParents')) for (const q of n.getParents()) stack.push(q);
      }
      if (outers.size !== 1) return null;
      const inst = [...outers][0];
      const occ = g.occ.get(inst) || [];
      if (occ.length !== 1) return null;
      return { node: inst, start: occ[0].start, end: occ[0].end };
    }

    // The span hint `inst` ({ start, end } of a document instance), or null.
    function hintOf(inst) {
      if (!inst || typeof inst !== 'object') return null;
      const start = Number(inst.start), end = Number(inst.end);
      return Number.isInteger(start) && Number.isInteger(end) && start >= 0 && end > start ? { start, end } : null;
    }

    // { node } or { node: null, reason }. `keep`: a node bound earlier; it
    // stays the answer while it is still one of the candidates (a later pick
    // elsewhere never re-targets a binding). LV2 `inst`: the exact source
    // span of the instance whose copy is meant (a continuation after a
    // commit, carried by Rust through the edit): the ONE copy whose
    // instance has exactly that occurrence, else nothing -- it replaces the
    // pick, never falls back to it.
    function bodyNode(g, start, end, keep, inst) {
      if (!g.protoBySpan || !g.copiesOf) return { node: null, reason: 'proto-provenance-unavailable' };
      if (!Number.isInteger(start) || !Number.isInteger(end) || start < 0 || end <= start) return { node: null, reason: 'runtime-node-for-span-not-unique' };
      const decl = g.protoBySpan.get(`${start}:${end}`);
      if (!decl || decl.size !== 1) return { node: null, reason: 'runtime-node-for-span-not-unique' };
      const copies = g.copiesOf.get([...decl][0]) || [];
      if (copies.length === 0) return { node: null, reason: 'proto-instance-not-rendered' };
      const hint = hintOf(inst);
      if (inst != null && !hint) return { node: null, reason: 'proto-instance-not-proven' };
      if (hint) {
        const at = copies.filter((c) => {
          const i = instanceOf(g, c);
          return !!i && i.start === hint.start && i.end === hint.end;
        });
        if (at.length !== 1) return { node: null, reason: 'proto-instance-not-proven' };
        if (keep) return at[0] === keep ? { node: keep } : { node: null, reason: 'runtime-node-not-proven' };
        return { node: at[0] };
      }
      if (keep) return copies.includes(keep) ? { node: keep } : { node: null, reason: 'runtime-node-not-proven' };
      if (copies.length === 1) return { node: copies[0] };
      const hit = g.lastHit;
      const under = hit ? copies.filter((c) => encloses(c, hit)) : [];
      if (under.length === 1) return { node: under[0] };
      return { node: null, reason: 'proto-instance-not-picked' };
    }

    // PLAIN DATA for Rust to prove: the chosen instance node's runtime chain
    // (through BODY: to its instance, up to the scene), like snapshotSpan.
    function snapshotBodySpan(generation, start, end, inst) {
      const c = compatibility();
      if (!c.ok) return snap('disabled', c.reason, null);
      const g = usable(generation);
      if (!g) return snap('stale', 'preview-scene-replaced', generation);
      const { node, reason } = bodyNode(g, start, end, null, inst == null ? null : inst);
      if (!node) return snap('unsupported', reason, generation);
      if (!fn(node, 'getParents') || !fn(node, 'getExecutionContext')) return snap('unsupported', 'runtime-parents-unavailable', generation);
      const r = snapshotGraph(g, node);
      if (r.disabled) {
        structuralDisable(r.disabled, false);
        return snap('disabled', r.disabled, null);
      }
      if (!r.graph) return snap('unsupported', 'runtime-parents-unavailable', generation);
      return snap('found', null, generation, {
        shape: r.label(node), ctxKind: r.ctxKind(node), sensors: Object.freeze([]), graph: r.graph,
      });
    }

    // The chosen instance's runtime node -- TRUSTED JavaScript layer only.
    // `keep` (optional): re-prove an earlier answer (same generation, same
    // span, still a copy of that one declaration node).
    function bodyNodeAt(generation, start, end, keep, inst) {
      const g = usable(generation);
      if (!g) return null;
      return bodyNode(g, start, end, keep && typeof keep === 'object' ? keep : null, inst == null ? null : inst).node || null;
    }

    // B3 (LV2): the instances that render a copy of the body statement at
    // [start, end): PLAIN DATA, one frozen { start, end } per copy whose
    // document instance is proven (instanceOf), in copy order, plus how
    // many copies could not be attributed. For the secondary outline of the
    // OTHER instances an edit of the shared definition changes; each one is
    // then bound and proven like the picked one (by its span hint).
    function bodyInstances(generation, start, end) {
      const g = usable(generation);
      if (!g || !g.protoBySpan || !g.copiesOf) return Object.freeze({ instances: Object.freeze([]), unproven: 0 });
      const decl = g.protoBySpan.get(`${start}:${end}`);
      if (!decl || decl.size !== 1) return Object.freeze({ instances: Object.freeze([]), unproven: 0 });
      const out = [];
      let unproven = 0;
      for (const c of g.copiesOf.get([...decl][0]) || []) {
        const i = instanceOf(g, c);
        if (i && !out.some((o) => o.start === i.start && o.end === i.end)) out.push(Object.freeze({ start: i.start, end: i.end }));
        else if (!i) unproven += 1;
      }
      return Object.freeze({ instances: Object.freeze(out), unproven });
    }

    // The runtime ancestors of a bound body copy, checked against the spans
    // Rust proved (flat [from, to, ...], direct parent first) -- for the
    // TRUSTED layer only (the highlight's frames). Each step must have
    // exactly ONE parent node (through fields, and from an instance body to
    // its instance node) whose ONE provenance occurrence is exactly the next
    // span. Never a search; any disagreement is null.
    function chainOf(generation, node, spans) {
      const g = usable(generation);
      if (!g || !node || typeof node !== 'object' || !Array.isArray(spans) || spans.length % 2) return null;
      const occurrences = (n) => {
        const own = g.occ.get(n);
        if (own) return own;
        if (!g.copyOf || !g.protoOcc || g.copyConflict.has(n)) return [];
        const from = g.copyOf.get(n);
        return (from && g.protoOcc.get(from)) || [];
      };
      const parentsOf = (n) => {
        const out = new Set();
        const seen = new Set();
        const stack = fn(n, 'getParents') ? [...n.getParents()] : [];
        while (stack.length && seen.size < 4096) {
          const p = stack.pop();
          if (!p || seen.has(p)) continue;
          seen.add(p);
          if (p instanceof X3D.X3DExecutionContext) {
            if (p === g.scene) continue;
            let outer = null;
            try { outer = fn(p, 'getOuterNode') ? p.getOuterNode() : null; } catch (e) { outer = null; }
            if (outer instanceof X3D.X3DBaseNode && fn(outer, 'getBody') && outer.getBody() === p
              && !(fn(outer, 'getProtoDeclaration') && outer.getProtoDeclaration() === outer)) out.add(outer);
            else out.add(null); // another context: unprovable
            continue;
          }
          if (p instanceof X3D.X3DBaseNode) { out.add(p); continue; }
          if (fn(p, 'getParents')) for (const q of p.getParents()) stack.push(q);
        }
        return [...out];
      };
      const result = [];
      let cur = node;
      const infra = new Set();
      try {
        const world = liveBrowser.getWorld();
        const layer0 = world && fn(world, 'getLayer0') ? world.getLayer0() : null;
        if (layer0) [layer0, layer0.groupNode, layer0.groupNodes].forEach((x) => infra.add(x));
      } catch (e) {
        return null;
      }
      try {
        for (let i = 0; i < spans.length; i += 2) {
          // World infrastructure (layer groups) is never an authored parent.
          const ps = parentsOf(cur).filter((p) => !infra.has(p));
          if (ps.length !== 1 || !ps[0]) return null;
          const occ = occurrences(ps[0]);
          if (occ.length !== 1 || occ[0].start !== Number(spans[i]) || occ[0].end !== Number(spans[i + 1])) return null;
          result.push(ps[0]);
          cur = ps[0];
        }
      } catch (e) {
        return null;
      }
      return result;
    }

    // The WHOLE public surface (contract §Private X_ITE adapter, plus the
    // VISUAL-3A1 binding amendment: snapshotSpan, nodeAt, spansOf, and the
    // B3 PROTO body binding: snapshotBodySpan, bodyNodeAt, chainOf, and the
    // LV2 instance listing bodyInstances, and the scene input gate sceneInput).
    return Object.freeze({ compatibility, parseWithProvenance, activate, retire, pick, abort, dispose, snapshotSpan, nodeAt, spansOf, snapshotBodySpan, bodyNodeAt, chainOf, bodyInstances, sceneInput });
  }

  const pickAdapterApi = Object.freeze({ createXitePickAdapter });

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = pickAdapterApi;
  } else if (typeof window !== 'undefined') {
    window.WrlXitePickAdapter = pickAdapterApi;
  }
})();
