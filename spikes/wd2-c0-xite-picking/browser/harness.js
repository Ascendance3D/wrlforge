'use strict';
// WD2-C0 browser harness. SPIKE ONLY -- never loaded by the product.
//
// What it does, and the X_ITE surfaces it touches (all recorded in the report):
//
//  1. PARSE-TIME PROVENANCE HOOK (private API: X3D.VRMLParser.prototype
//     .nodeStatement, x_ite 15.1.10 dist/x_ite.js:33942). While -- and only
//     while -- this harness parses the exact document string, every
//     nodeStatement the classic-VRML parser consumes is recorded as
//     {runtime node object, start, end, kind}. `start` is the parser's own
//     lastIndex after leading whitespace/comments, `end` its lastIndex after
//     the statement. Both are offsets into the very string X_ITE parsed, which
//     is the very string WRL Forge parsed: provenance, not inference. Nothing
//     is added to, removed from or rewritten in the source.
//  2. PER-GENERATION MAP. Each load is a new generation with its own WeakMap
//     runtime-node -> occurrences. Nothing survives a load.
//  3. HIT (private API: browser.touch(x, y) dist/x_ite.js:71639 and
//     browser.getHit() :71537 -- neither is in x_ite.d.ts).
//  4. RUNTIME PARENT GRAPH (private API: X3DChildObject.getParents()
//     :15719). Used to *verify* the runtime graph still has the source
//     graph's shape at pick time (a Script/ROUTE could re-parent a node).
//
// Harness labels (rt1, rt2, ...) are harness-side only; they never enter the
// VRML text.

(function () {
  const state = {
    browser: null,
    canvas: null,
    gen: 0,
    current: null,
    labels: new WeakMap(),
    nextLabel: 1,
    capture: null,
    hookCalls: 0,
    pointerArmed: false,
    pointerResults: [],
    retained: [],
  };

  const label = (o) => {
    if (!o) return null;
    let l = state.labels.get(o);
    if (!l) { l = `rt${state.nextLabel++}`; state.labels.set(o, l); }
    return l;
  };
  const round = (v, p = 4) => Math.round(v * 10 ** p) / 10 ** p;
  const arr = (v, n) => Array.from({ length: n }, (_, i) => round(v[i]));
  const typeName = (n) => (n && typeof n.getTypeName === 'function' ? n.getTypeName() : null);
  const basename = (u) => (u ? String(u).split('/').pop() : null);

  // ---- 1. provenance hook --------------------------------------------------
  function installHook() {
    const VP = X3D.VRMLParser && X3D.VRMLParser.prototype;
    if (!VP || typeof VP.nodeStatement !== 'function' || typeof VP.comments !== 'function') {
      throw new Error('PRIVATE_API_MISSING: X3D.VRMLParser.prototype.nodeStatement/comments');
    }
    const original = VP.nodeStatement;
    VP.nodeStatement = function wd2c0NodeStatement() {
      const cap = state.capture;
      if (!cap || this.input !== cap.text) return original.call(this);
      state.hookCalls++;
      this.comments(); // idempotent: the original calls it first, too.
      const start = this.lastIndex;
      const head = this.input.slice(start, start + 4);
      const kind = /^USE[\s,#]/.test(head) ? 'use' : /^DEF[\s,#]/.test(head) ? 'def' : 'node';
      const node = original.call(this);
      if (node && typeof node === 'object' && typeof node.getTypeName === 'function') {
        cap.records.push({
          node, start, end: this.lastIndex, kind,
          inProto: typeof this.isInsideProtoDeclaration === 'function' ? !!this.isInsideProtoDeclaration() : null,
          scene: this.getScene(),
          ctx: this.getExecutionContext(),
        });
      } else if (node && typeof node === 'object') {
        cap.placeholders++;
      }
      return node;
    };
  }

  // ---- 2. load one generation ---------------------------------------------
  async function frames(n = 3) {
    for (let i = 0; i < n; i++) await new Promise((r) => requestAnimationFrame(() => r()));
  }

  async function load(text, opts = {}) {
    const b = state.browser;
    const hook = opts.hook !== false;
    b.baseURL = opts.baseURL;
    const t0 = performance.now();
    state.capture = hook ? { text, records: [], placeholders: 0 } : null;
    let scene;
    try {
      scene = await b.createX3DFromString(text);
    } finally {
      var cap = state.capture;
      state.capture = null;
    }
    const tParse = performance.now();
    const occ = new WeakMap();
    let kept = 0, dropped = 0;
    const occurrences = [];
    if (cap) {
      for (const r of cap.records) {
        // Only statements of THIS scene's own top execution context; PROTO
        // bodies (separate context, copied per instance) are excluded.
        if (r.scene !== scene || r.ctx !== scene || r.inProto) { dropped++; continue; }
        kept++;
        let list = occ.get(r.node);
        if (!list) { list = []; occ.set(r.node, list); }
        list.push({ start: r.start, end: r.end, kind: r.kind });
        occurrences.push({ rt: label(r.node), type: typeName(r.node), start: r.start, end: r.end, kind: r.kind });
      }
    }
    const tMap = performance.now();
    await b.replaceWorld(scene);
    await frames(opts.frames || 4);
    const tReady = performance.now();
    const gen = { id: ++state.gen, scene, text, occ, hook };
    state.current = gen;
    return {
      generation: gen.id,
      hook,
      // The hook records only when parser.input === this exact string.
      gatedOnExactInput: !!cap,
      hookRecords: cap ? cap.records.length : 0,
      kept, dropped,
      placeholders: cap ? cap.placeholders : 0,
      occurrences: opts.withOccurrences === false ? undefined : occurrences,
      sceneIsPublicScene: scene === b.currentScene,
      ms: { parse: round(tParse - t0, 3), map: round(tMap - tParse, 3), replaceAndFrames: round(tReady - tMap, 3) },
    };
  }

  // ---- 3/4. hit dump --------------------------------------------------------
  // X_ITE's own world infrastructure that holds the scene's root nodes:
  // layer0 (private API browser.getWorld() :79569, X3DWorld.getLayer0()
  // :57998) and its groupNode / groupNodes (X3DLayerNode, :45251). They live
  // in the DOCUMENT's execution context, so only object identity -- never
  // "has no provenance" -- may classify them as infrastructure.
  function worldInfrastructure() {
    const world = typeof state.browser.getWorld === 'function' ? state.browser.getWorld() : null;
    const layer0 = world && typeof world.getLayer0 === 'function' ? world.getLayer0() : null;
    return new Set([layer0, layer0 && layer0.groupNode, layer0 && layer0.groupNodes].filter(Boolean));
  }

  // Execution-context class of a runtime node. 'document' is the scene this
  // generation parsed from the document string; an X3DScene that is not it is
  // another document (an Inline, or X_ITE's private scene); anything else is a
  // PROTO instance body.
  function ctxKind(ctx, gen) {
    if (!ctx) return 'none';
    if (ctx === gen.scene) return 'document';
    if (ctx instanceof X3D.X3DScene) return 'external-scene';
    return 'proto-body';
  }

  // Runtime parents of a node: climb through non-node parents (fields, SFNode
  // wrappers) until reaching node objects or an execution context.
  function runtimeParents(node, gen) {
    const nodes = new Set();
    const ctxs = [];
    const seen = new Set();
    const stack = [...(node.getParents ? node.getParents() : [])];
    while (stack.length) {
      const p = stack.pop();
      if (!p || seen.has(p)) continue;
      seen.add(p);
      if (p instanceof X3D.X3DExecutionContext) { ctxs.push(p === gen.scene ? 'SCENE' : 'OTHER_CONTEXT'); continue; }
      if (p instanceof X3D.X3DBaseNode) { nodes.add(p); continue; }
      if (typeof p.getParents === 'function') for (const q of p.getParents()) stack.push(q);
    }
    return { nodes: [...nodes], ctxs };
  }

  function upwardGraph(start, gen, limit = 64) {
    const infra = worldInfrastructure();
    const graph = {};
    const queue = [start];
    while (queue.length && Object.keys(graph).length < limit) {
      const n = queue.shift();
      const l = label(n);
      if (graph[l]) continue;
      const { nodes, ctxs } = runtimeParents(n, gen);
      graph[l] = {
        type: typeName(n),
        ctxKind: infra.has(n) ? 'world-infrastructure' : ctxKind(n.getExecutionContext ? n.getExecutionContext() : null, gen),
        name: typeof n.getName === 'function' ? (n.getName() || null) : null,
        occurrences: (gen.occ.get(n) || []).slice(),
        parents: [...nodes.map(label), ...ctxs],
      };
      for (const p of nodes) queue.push(p);
    }
    return graph;
  }

  function dumpHit(hit, gen, extra) {
    const shape = hit.shapeNode || null;
    const geometry = shape && typeof shape.getGeometry === 'function' ? shape.getGeometry() : null;
    const out = {
      generation: gen.id,
      id: hit.id,
      shape: label(shape),
      shapeType: typeName(shape),
      geometry: label(geometry),
      geometryType: typeName(geometry),
      ctxKind: shape ? ctxKind(shape.getExecutionContext(), gen) : null,
      ctxWorld: shape ? basename(shape.getExecutionContext().worldURL) : null,
      layer: typeName(hit.layerNode),
      sensors: [...hit.sensors.keys()].map((n) => ({ rt: label(n), type: typeName(n), name: n.getName ? (n.getName() || null) : null })),
      point: arr(hit.point, 3),
      normal: arr(hit.normal, 3),
      modelViewMatrix: arr(hit.modelViewMatrix, 16),
      pointer: arr(hit.pointer, 2),
      graph: shape ? upwardGraph(shape, gen) : null,
      ...extra,
    };
    return out;
  }

  function canvasInfo() {
    const el = state.canvas;
    const r = el.getBoundingClientRect();
    const gl = el.shadowRoot ? el.shadowRoot.querySelector('canvas') : null;
    const vp = typeof state.browser.getViewport === 'function' ? Array.from(state.browser.getViewport()) : null;
    return {
      rect: { left: r.left, top: r.top, width: r.width, height: r.height },
      drawingBuffer: gl ? { width: gl.width, height: gl.height } : null,
      viewport: vp,
      devicePixelRatio: window.devicePixelRatio,
    };
  }

  // The coordinate rule under test: client (CSS px) -> X_ITE touch space
  // (drawing-buffer px, origin bottom-left).
  function clientToTouch(clientX, clientY) {
    const r = state.canvas.getBoundingClientRect();
    const vp = state.browser.getViewport();
    return {
      x: (clientX - r.left) / r.width * vp[2],
      y: (1 - (clientY - r.top) / r.height) * vp[3],
    };
  }

  function pickClient(clientX, clientY, extra = {}) {
    const gen = state.current;
    const t = clientToTouch(clientX, clientY);
    const t0 = performance.now();
    const ok = state.browser.touch(t.x, t.y);
    const touchMs = performance.now() - t0;
    const hit = state.browser.getHit();
    if (extra.retain && hit.shapeNode) state.retained.push({ gen: gen.id, node: hit.shapeNode });
    return dumpHit(hit, gen, { touchReturned: ok, touch: { x: round(t.x, 3), y: round(t.y, 3) }, touchMs: round(touchMs, 4), client: { x: round(clientX, 3), y: round(clientY, 3) } });
  }

  // Real pointer path: a capture-phase mousedown listener on the canvas element
  // (runs before X_ITE's own surface handlers).
  function onMouseDown(e) {
    if (!state.pointerArmed) return;
    state.pointerArmed = false;
    const ours = clientToTouch(e.clientX, e.clientY);
    const theirs = typeof state.browser.getPointerFromEvent === 'function'
      ? state.browser.getPointerFromEvent(e) : null;
    const res = pickClient(e.clientX, e.clientY, { via: 'pointer-event' });
    res.xiteGetPointerFromEvent = theirs ? { x: round(theirs.x, 3), y: round(theirs.y, 3) } : null;
    res.ourConversion = { x: round(ours.x, 3), y: round(ours.y, 3) };
    state.pointerResults.push(res);
  }

  // ---- reload identity probe -----------------------------------------------
  function retainedReport() {
    const gen = state.current;
    return state.retained.map((r) => ({
      label: label(r.node),
      fromGeneration: r.gen,
      currentGeneration: gen.id,
      inCurrentMap: gen.occ.has(r.node),
    }));
  }

  // ---- sensor event probe -----------------------------------------------------
  function watchSensor(name) {
    const node = state.current.scene.getNamedNode(name);
    const events = [];
    for (const f of ['touchTime', 'isActive', 'isOver']) {
      node.addFieldCallback(`wd2c0-${f}`, f, (v) => events.push({ field: f, value: typeof v === 'object' ? String(v) : v }));
    }
    state.sensorEvents = events;
    return true;
  }

  async function bindViewpoint(name, settleMs = 2500) {
    const vp = state.current.scene.getNamedNode(name);
    vp.set_bind = true;
    await new Promise((r) => setTimeout(r, settleMs));
    await frames(3);
    return true;
  }

  function activeViewpointDescription() {
    const vp = state.browser.getActiveViewpoint ? state.browser.getActiveViewpoint() : null;
    return vp ? (vp._description ? vp._description.getValue() : typeName(vp)) : null;
  }

  function setLayout({ left = 0, top = 0, width = 800, height = 800, rootFontPx = null } = {}) {
    const f = document.getElementById('frame');
    f.style.left = `${left}px`;
    f.style.top = `${top}px`;
    state.canvas.style.width = `${width}px`;
    state.canvas.style.height = `${height}px`;
    if (rootFontPx) document.documentElement.style.fontSize = `${rootFontPx}px`;
    return frames(4).then(canvasInfo);
  }

  async function init() {
    state.canvas = document.getElementById('canvas');
    await X3D();
    state.browser = state.canvas.browser;
    await state.browser.loadComponents?.(state.browser.getProfile?.('Full'));
    installHook();
    state.canvas.addEventListener('mousedown', onMouseDown, true);
    return {
      xiteVersion: X3D.getBrowser ? X3D.getBrowser().version : null,
      publicBrowserHasTouch: typeof state.browser.touch === 'function',
      publicBrowserHasGetHit: typeof state.browser.getHit === 'function',
      hasVRMLParser: !!X3D.VRMLParser,
      canvas: canvasInfo(),
    };
  }

  window.wd2c0 = {
    init, load, pickClient, canvasInfo, setLayout, retainedReport, watchSensor,
    sensorEvents: () => (state.sensorEvents || []).slice(),
    bindViewpoint, activeViewpointDescription,
    armPointer: () => { state.pointerArmed = true; state.pointerResults.length = 0; return true; },
    takePointer: () => state.pointerResults.splice(0),
    hookCalls: () => state.hookCalls,
    clearRetained: () => { state.retained.length = 0; },
  };
})();
