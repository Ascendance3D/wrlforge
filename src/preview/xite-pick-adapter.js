'use strict';
// WD2-D -- the ONE narrow X_ITE picking adapter (WD2_D_XITE_PICKING_CONTRACT.md).
//
// Every private X_ITE surface WD2-D depends on is touched here and nowhere
// else (a source-scan test enforces it). The adapter does exactly two things:
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
  // The exact X_ITE build every private-surface assumption below was proven on.
  const XITE_VERSION = '15.1.10';

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
    if (browser.version !== XITE_VERSION) return 'xite-version-mismatch'; // V1 / G2
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
        let kept = 0;
        for (const rec of token.capture.records) {
          if (rec.scene !== scene || rec.ctx !== scene || rec.inProto) continue;
          let list = occ.get(rec.node);
          if (!list) { list = []; occ.set(rec.node, list); }
          list.push(Object.freeze({ start: rec.start, end: rec.end }));
          kept += 1;
        }
        const spans = isProbe ? token.capture.records
          .filter((rec) => rec.scene === scene && rec.ctx === scene && !rec.inProto)
          .map((rec) => ({ type: typeOf(rec.node), start: rec.start, end: rec.end })) : null;
        token.capture.records.length = 0;
        const generation = Object.freeze({
          sessionId: meta.sessionId == null ? null : meta.sessionId,
          overlayGeneration: meta.generationId == null ? null : meta.generationId,
          text,
        });
        gens.set(generation, { scene, occ, kept, spans });
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
      disposed = true;
      liveBrowser = null;
    }

    // ---- pick ------------------------------------------------------------------
    const snap = (outcome, reason, generation, extra) => Object.freeze({ outcome, reason, generation: generation || null, ...(extra || {}) });

    function typeOf(node) {
      try { return fn(node, 'getTypeName') ? node.getTypeName() : null; } catch (e) { return null; }
    }

    function pick(clientX, clientY) {
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
      const shape = hit.shapeNode;
      if (!fn(shape, 'getParents') || !fn(shape, 'getExecutionContext')) {
        structuralDisable('runtime-parents-unavailable', false);
        return snap('unsupported', 'runtime-parents-unavailable', generation);
      }
      if (!fn(shape, 'getTypeName')) return snap('unsupported', 'runtime-node-type-unavailable', generation);

      const world = liveBrowser.getWorld();
      const layer0 = world && fn(world, 'getLayer0') ? world.getLayer0() : null;
      if (!layer0 || !layer0.groupNode || !layer0.groupNodes) {
        structuralDisable('world-infrastructure-missing', false);
        return snap('disabled', 'world-infrastructure-missing', null);
      }
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
        const ctxs = [];
        const seen = new Set();
        const stack = [...node.getParents()];
        while (stack.length) {
          const p = stack.pop();
          if (!p || seen.has(p)) continue;
          seen.add(p);
          if (p instanceof X3D.X3DExecutionContext) { ctxs.push(p === g.scene ? 'SCENE' : 'OTHER_CONTEXT'); continue; }
          if (p instanceof X3D.X3DBaseNode) { nodes.add(p); continue; }
          if (fn(p, 'getParents')) for (const q of p.getParents()) stack.push(q);
        }
        return { nodes: [...nodes], ctxs };
      };
      const graph = {};
      const queue = [shape];
      try {
        while (queue.length && Object.keys(graph).length < 256) {
          const n = queue.shift();
          const l = label(n);
          if (graph[l]) continue;
          const isInfra = infra.has(n);
          const { nodes, ctxs } = isInfra ? { nodes: [], ctxs: [] } : runtimeParents(n);
          graph[l] = Object.freeze({
            type: typeOf(n),
            ctxKind: isInfra ? 'world-infrastructure' : ctxKind(n),
            occurrences: Object.freeze((g.occ.get(n) || []).slice()),
            parents: Object.freeze([...nodes.map(label), ...ctxs]),
          });
          for (const p of nodes) queue.push(p);
        }
      } catch (e) {
        return snap('unsupported', 'runtime-parents-unavailable', generation);
      }
      return snap('hit', null, generation, {
        shape: label(shape),
        ctxKind: ctxKind(shape),
        sensors: Object.freeze([...hit.sensors.keys()].map(typeOf)),
        graph: Object.freeze(graph),
      });
    }

    // The WHOLE public surface (contract §Private X_ITE adapter).
    return Object.freeze({ compatibility, parseWithProvenance, activate, retire, pick, abort, dispose });
  }

  const pickAdapterApi = Object.freeze({ createXitePickAdapter });

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = pickAdapterApi;
  } else if (typeof window !== 'undefined') {
    window.WrlXitePickAdapter = pickAdapterApi;
  }
})();
