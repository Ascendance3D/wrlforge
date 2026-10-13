// SPDX-License-Identifier: GPL-3.0-or-later
// TEMPORARY JavaScript compatibility surface for the X_ITE preview.
//
// X_ITE is a JavaScript renderer; until a native renderer is approved it is
// reached ONLY through the functions below, called from the Rust/Wasm UI.
// The adapter receives TEXT (the canonical buffer the Rust backend returned)
// and never a path. It has no Tauri/IPC access of its own beyond what the
// page has, and the CSP blocks every remote origin, so remote URLs inside a
// world are never fetched.
//
// TEXTURE-LOCAL-1: relative URLs (ImageTexture) resolve against the base URL
// Rust issued for this generation (`wrlres://localhost/<token>/`); the
// Rust scheme handler serves only image files inside the document folder
// while that token is current. The text is passed through unchanged, so
// provenance spans are exactly the source's.
//
// VISUAL-2 picking: every PRIVATE X_ITE picking access lives in
// xite-pick-adapter.js (the WD2-D adapter, copied verbatim at build time from
// src/preview/xite-pick-adapter.js). VISUAL-3A manipulation: every X_ITE
// camera / temporary-translation access lives in xite-gizmo-adapter.js. This
// file uses only their public surfaces and returns PLAIN DATA; Rust decides
// what a pick or a drag means.
//
// VISUAL-3A1: the Move tool binds the runtime node the pick adapter's
// provenance generation recorded for the authored Transform's EXACT span
// (gizmoLocate -> Rust proof -> gizmoBind); runtime objects never cross to
// Rust. The camera carry keeps the user's view across a scene replacement:
// the bound viewpoint's identity is captured when its generation retires
// (its provenance span, or "the layer default"), Rust maps the span to the
// new revision through the exact logged changes, and the user offsets read
// at X_ITE's SHUTDOWN of the old world are written onto the PROVEN same
// viewpoint in a microtask right after the new world binds -- before X_ITE
// draws a frame of it.
//
// B2-CAMERA: Fit / standard views / Reset / the viewpoint list call the
// gizmo adapter's view controls; a viewpoint crosses to Rust only as its
// provenance span in the generation on screen, and comes back the same way
// (nodeAt), exactly like a Move binding. A document's FIRST load may ask for
// one automatic Fit (meta.autoFit), done only when the scene binds the
// layer's default viewpoint -- an authored Viewpoint is never overridden.
// The camera is view state: no source change, no dirty flag, no undo step.
(function () {
  'use strict';

  // TEXTURE-LOCAL-1: X_ITE workaround, found on 15.1.10 and kept unchanged for
  // 16.5.0 (XITE-16-UPGRADE-2), whose interests still hold their target only
  // through a WeakRef. FileLoader.setScene waits for
  // the new scene's loadCount through an interest whose ONLY reference to
  // the FileLoader is a WeakRef. When a texture is still loading after the
  // parse, nothing else holds the FileLoader; if the collector runs first
  // the interest is dropped and createX3DFromString never settles (measured:
  // the preview stuck on "updating", for wrlres AND data: textures alike).
  // Only objects with X_ITE's `set_loadCount__` method are held strongly,
  // and only for the life of the WeakRef, which X_ITE drops when the
  // interest is removed (set_loadCount__ runs) or its scene is discarded.
  // Every other WeakRef is untouched.
  (function () {
    var Native = window.WeakRef;
    if (typeof Native !== 'function') return;
    function HeldWeakRef(target) {
      var ref = Reflect.construct(Native, [target], new.target || HeldWeakRef);
      if (target && typeof target.set_loadCount__ === 'function') heldTargets.set(ref, target);
      return ref;
    }
    var heldTargets = new WeakMap();
    HeldWeakRef.prototype = Object.create(Native.prototype, { constructor: { value: HeldWeakRef } });
    HeldWeakRef.prototype.deref = function () {
      return heldTargets.has(this) ? heldTargets.get(this) : Native.prototype.deref.call(this);
    };
    Object.setPrototypeOf(HeldWeakRef, Native);
    window.WeakRef = HeldWeakRef;
  })();

  // B3 (LV2): an instance span hint from (iFrom, iTo), or null (absent / -1).
  function instHint(iFrom, iTo) {
    var f = Number(iFrom), t = Number(iTo);
    return Number.isInteger(f) && Number.isInteger(t) && f >= 0 && t > f ? { start: f, end: t } : null;
  }

  function canvas() { return document.getElementById('viewport'); }

  // XITE-16-UPGRADE-2: the smoke-only console trace (consoleTraceStart).
  var consoleTrace = null;

  // ---- picking state ---------------------------------------------------------
  // `adapter` belongs to exactly one X_ITE browser; a replaced viewport gets a
  // new one. `active` is the generation on screen: {session, revision, hash,
  // seq} as the UI named it at load time, or null (retired).
  var pick = {
    adapter: null, gizmo: null, browser: null, active: null, scene: null, generation: null, loadSeq: 0,
    // VISUAL-3A1 camera carry: the identity captured at retire, the result
    // of the newest load's carry, and a per-frame view-matrix trace (tests).
    capture: null, camera: null, trace: null,
    // B2-CAMERA: the newest load's automatic-Fit result.
    autoFit: null,
  };

  function adapterFor(browser) {
    if (pick.browser !== browser) {
      if (pick.adapter) pick.adapter.dispose();
      if (pick.gizmo) pick.gizmo.unbind();
      pick.adapter = null;
      pick.gizmo = null;
      pick.active = null;
      pick.scene = null;
      pick.generation = null;
      pick.capture = null;
      pick.browser = browser || null;
      if (browser && window.WrlXiteGizmoAdapter) {
        try {
          pick.gizmo = window.WrlXiteGizmoAdapter.createXiteGizmoAdapter({ X3D: window.X3D, browser: browser });
        } catch (e) {
          pick.gizmo = null;
        }
      }
      if (browser && window.WrlXitePickAdapter) {
        try {
          pick.adapter = window.WrlXitePickAdapter.createXitePickAdapter({ X3D: window.X3D, browser: browser });
        } catch (e) {
          pick.adapter = null;
        }
      }
    }
    return pick.adapter;
  }

  // A retired generation can no longer be manipulated: any temporary
  // translation is put back and the binding dropped.
  // The bound viewpoint's identity in the generation that is retiring: the
  // layer's default viewpoint, or an authored one with exactly ONE
  // provenance occurrence. A retire with no generation on screen keeps the
  // previous capture (the scene on screen did not change).
  function captureIdentity() {
    if (!pick.active || !pick.generation || !pick.gizmo || !pick.adapter) return;
    var cap = {
      session: pick.active.session, revision: pick.active.revision, seq: pick.active.seq,
      kind: 'unprovable', reason: 'viewpoint-unknown', node: null, from: null, to: null,
    };
    var b = pick.gizmo.boundViewpoint();
    if (b) {
      cap.node = b.node;
      if (b.isDefault) {
        cap.kind = 'default';
        cap.reason = null;
      } else {
        var sp = pick.adapter.spansOf(pick.generation, b.node);
        if (sp && sp.length === 1) {
          cap.kind = 'authored';
          cap.reason = null;
          cap.from = sp[0].start;
          cap.to = sp[0].end;
        } else {
          cap.reason = sp && sp.length ? 'viewpoint-has-several-occurrences' : 'viewpoint-without-provenance';
        }
      }
    }
    pick.capture = cap;
  }

  function retire(reason) {
    captureIdentity();
    pick.active = null;
    pick.scene = null;
    pick.generation = null;
    if (pick.gizmo) pick.gizmo.unbind();
    if (pick.adapter) pick.adapter.retire(reason || 'preview-scene-replaced');
  }

  window.addEventListener('pagehide', function () {
    pick.active = null;
    pick.scene = null;
    pick.generation = null;
    pick.capture = null;
    if (pick.gizmo) pick.gizmo.unbind();
    pick.gizmo = null;
    if (pick.adapter) pick.adapter.dispose();
    pick.adapter = null;
  });

  // ---- VISUAL-3A1 camera carry ----------------------------------------------
  function maxDelta(a, b) {
    if (!a || !b) return null;
    var d = 0;
    for (var i = 0; i < 16; i++) d = Math.max(d, Math.abs(a[i] - b[i]));
    return d;
  }

  // Arm the carry for ONE load: {done()} always. `meta.camera` is what the UI
  // proved: {kind: 'default'} or {kind: 'authored', from, to} (the captured
  // viewpoint's span mapped by Rust to THIS revision's preview text).
  function cameraPlan(browser, meta, mine, scene, generation) {
    var X3D = window.X3D;
    var cap = pick.capture;
    var result = function (status, reason, extra) {
      if (mine !== pick.loadSeq) return; // a newer load reports its own
      var r = { seq: meta ? meta.seq : null, session: meta ? meta.session : null, status: status, reason: reason || null };
      if (extra) for (var k in extra) r[k] = extra[k];
      pick.camera = r;
    };
    var none = { done: function () {} };
    var want = meta && meta.camera;
    if (!meta || !want) { result('not-requested', cap ? 'not-proven-for-this-revision' : 'no-capture'); return none; }
    if (!cap || cap.session !== meta.session || cap.kind !== want.kind) { result('not-requested', 'capture-does-not-match'); return none; }
    if (!pick.gizmo || !X3D || !X3D.X3DConstants || !Number.isInteger(X3D.X3DConstants.SHUTDOWN_EVENT)
      || typeof browser.addBrowserCallback !== 'function' || typeof browser.removeBrowserCallback !== 'function') {
      result('not-restored', 'camera-carry-unavailable');
      return none;
    }
    if (want.kind === 'authored' && !(Number.isInteger(want.from) && Number.isInteger(want.to) && generation)) {
      result('not-restored', 'viewpoint-not-proven-in-new-scene');
      return none;
    }
    var EV = X3D.X3DConstants.SHUTDOWN_EVENT;
    var key = {};
    var fired = false;
    var off = function () { try { browser.removeBrowserCallback(key, EV); } catch (e) { /* gone */ } };
    browser.addBrowserCallback(key, EV, function () {
      off();
      // Every load whose world really replaces the scene carries the camera
      // onto ITS proven viewpoint -- an older load superseded after it
      // replaced the world included -- and the capture then follows that
      // viewpoint, so the next replacement continues from it. Only the
      // newest load reports and consumes the capture.
      fired = true;
      var t0 = performance.now();
      var b = pick.gizmo.boundViewpoint();
      if (!b || b.node !== cap.node) { result('not-restored', 'viewpoint-changed-before-reload'); return; }
      var state = pick.gizmo.readCamera(b.node);
      var before = pick.gizmo.viewMatrix();
      if (!state || !before) { result('not-restored', 'camera-unreadable'); return; }
      // X_ITE binds the new world's viewpoint synchronously after this
      // callback, in the same task; a microtask runs before its next frame.
      Promise.resolve().then(function () {
        if (browser.currentScene !== scene) { result('not-restored', 'superseded'); return; }
        var nb = pick.gizmo.boundViewpoint();
        if (!nb) { result('not-restored', 'viewpoint-unknown'); return; }
        if (want.kind === 'default') {
          if (!nb.isDefault) { result('not-restored', 'new-scene-binds-another-viewpoint'); return; }
        } else {
          var target = pick.adapter ? pick.adapter.nodeAt(generation, want.from, want.to) : null;
          if (!target) { result('not-restored', 'viewpoint-not-proven-in-new-scene'); return; }
          if (nb.node !== target) { result('not-restored', 'new-scene-binds-another-viewpoint'); return; }
        }
        if (!pick.gizmo.writeCamera(nb.node, state)) { result('not-restored', 'camera-write-failed'); return; }
        cap.node = nb.node;
        if (mine === pick.loadSeq && pick.capture === cap) pick.capture = null; // consumed
        var after = pick.gizmo.viewMatrix();
        result('restored', null, { kind: want.kind, ms: performance.now() - t0, delta: maxDelta(before, after) });
      });
    });
    return {
      done: function () {
        off();
        if (!fired) result('not-restored', 'world-not-replaced');
      },
    };
  }

  // B2-CAMERA: the first load of a document frames the scene once, only on
  // the layer's default viewpoint (no authored Viewpoint is bound).
  async function autoFit(browser, meta, mine, scene) {
    var r = { seq: meta.seq, session: meta.session, status: 'skipped', reason: null };
    try {
      if (typeof browser.nextFrame === 'function') await browser.nextFrame();
    } catch (e) { /* a frame is only a settle point */ }
    if (mine !== pick.loadSeq || browser.currentScene !== scene) { r.reason = 'superseded'; pick.autoFit = r; return; }
    var b = pick.gizmo ? pick.gizmo.boundViewpoint() : null;
    if (!b) r.reason = 'camera-unavailable';
    else if (!b.isDefault) r.reason = 'authored-viewpoint';
    else {
      var f = pick.gizmo.viewAll(0);
      r.status = f.ok ? 'fitted' : 'failed';
      r.reason = f.ok ? null : f.reason;
    }
    pick.autoFit = r;
  }

  function cameraGizmo() {
    var c = canvas();
    if (c && c.browser) adapterFor(c.browser);
    return pick.gizmo;
  }

  window.wrlforgePreview = {
    probe: function () {
      var t = document.createElement('canvas');
      var gl2 = t.getContext('webgl2');
      var gl = gl2 || t.getContext('webgl');
      var c = canvas();
      var xite = typeof window.X3D === 'function' || typeof window.X3D === 'object';
      var info = gl2 ? 'webgl2' : (gl ? 'webgl1' : 'no-webgl');
      if (gl) {
        var dbg = gl.getExtension('WEBGL_debug_renderer_info');
        if (dbg) info += ' · ' + gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL);
      }
      return info + ' · X_ITE ' + (xite ? (window.X3D.getBrowser ? 'loaded' : 'present') : 'missing') +
        ' · canvas ' + (c && c.browser ? 'ready' : 'not ready');
    },
    // Read-only render check (VISUAL-1 tests): after the next frame, the
    // fraction of drawn pixels that differ from the corner (background)
    // pixel. -1 when the canvas or its WebGL context is unavailable.
    coverage: async function () {
      var c = canvas();
      if (!c || !c.browser) return -1;
      var b = c.browser;
      await b.nextFrame();
      var gl = b.getContext();
      if (!gl) return -1;
      var w = gl.drawingBufferWidth, h = gl.drawingBufferHeight;
      if (!w || !h) return -1;
      var px = new Uint8Array(w * h * 4);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px);
      var r = px[0], g = px[1], bl = px[2], n = 0;
      for (var i = 0; i < px.length; i += 4) {
        if (Math.abs(px[i] - r) + Math.abs(px[i + 1] - g) + Math.abs(px[i + 2] - bl) > 24) n++;
      }
      return n / (w * h);
    },
    // Read-only (VISUAL-2 tests): [r, g, b] of the drawn frame at a client
    // point, after the next frame; null when unavailable.
    pixel: async function (clientX, clientY) {
      var c = canvas();
      if (!c || !c.browser) return null;
      var b = c.browser;
      await b.nextFrame();
      var gl = b.getContext();
      var r = c.getBoundingClientRect();
      if (!gl || !(r.width > 0) || !(r.height > 0)) return null;
      var x = Math.floor((clientX - r.left) / r.width * gl.drawingBufferWidth);
      var y = Math.floor((1 - (clientY - r.top) / r.height) * gl.drawingBufferHeight);
      var px = new Uint8Array(4);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.readPixels(x, y, 1, 1, gl.RGBA, gl.UNSIGNED_BYTE, px);
      return [px[0], px[1], px[2]];
    },
    // Tests: the newest load's phase ('parse' / 'replace' / 'done'), JSON.
    loadState: function () {
      return JSON.stringify({ phase: pick.phase || null, now: Math.round(performance.now()) });
    },
    // Read-only (TEXTURE-LOCAL-1 tests): the drawn frame as PNG bytes
    // (Uint8Array) after the next frame, or null.
    capture: async function () {
      var c = canvas();
      if (!c || !c.browser) return null;
      var b = c.browser;
      await b.nextFrame();
      var gl = b.getContext();
      if (!gl) return null;
      var w = gl.drawingBufferWidth, h = gl.drawingBufferHeight;
      var px = new Uint8Array(w * h * 4);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      gl.readPixels(0, 0, w, h, gl.RGBA, gl.UNSIGNED_BYTE, px);
      var out = document.createElement('canvas');
      out.width = w;
      out.height = h;
      var ctx = out.getContext('2d');
      var img = ctx.createImageData(w, h);
      for (var y = 0; y < h; y++) {
        var src = (h - 1 - y) * w * 4;
        img.data.set(px.subarray(src, src + w * 4), y * w * 4);
        for (var x = 0; x < w; x++) img.data[y * w * 4 + x * 4 + 3] = 255;
      }
      ctx.putImageData(img, 0, 0);
      var blob = await new Promise(function (res) { out.toBlob(res, 'image/png'); });
      return blob ? new Uint8Array(await blob.arrayBuffer()) : null;
    },
    // `meta` names the generation: {session, revision, hash, seq} from the
    // UI, or null for a scene that is not a document (the empty world after
    // Close). The previous generation is retired before anything else, so a
    // pick during the load is stale, never a hit on the old scene.
    load: async function (text, meta, base) {
      var c = canvas();
      if (!c || !c.browser) return 'unavailable: X_ITE canvas not initialized';
      var browser = c.browser;
      // Only a Rust-issued wrlres base is accepted; anything else (or none)
      // gets the never-valid token, so relative URLs are refused by Rust.
      var b = String(base || '');
      browser.baseURL = /^wrlres:\/\/localhost\/[A-Za-z0-9]+\/$/.test(b) ? b : 'wrlres://localhost/none/';
      var a = adapterFor(browser);
      var mine = ++pick.loadSeq;
      var phase = function (ph) { pick.phase = { seq: mine, phase: ph, t: Math.round(performance.now()) }; };
      phase('parse');
      retire('preview-scene-replaced');
      try {
        var scene, generation = null;
        if (a && meta) {
          var r = await a.parseWithProvenance(String(text == null ? '' : text),
            { sessionId: meta.session, generationId: meta.seq });
          if (!r.scene) return 'superseded';
          scene = r.scene;
          generation = r.generation;
        } else {
          scene = await browser.createX3DFromString(String(text == null ? '' : text));
        }
        var carry = cameraPlan(browser, meta, mine, scene, generation);
        phase('replace');
        try {
          await browser.replaceWorld(scene);
        } finally {
          carry.done();
        }
        // Only the newest load may become the picking generation.
        if (mine === pick.loadSeq && a && generation && a.activate(generation, scene)) {
          pick.active = { session: meta.session, revision: meta.revision, hash: meta.hash, seq: meta.seq };
          pick.scene = scene;
          pick.generation = generation;
        }
        if (meta && meta.autoFit && mine === pick.loadSeq) await autoFit(browser, meta, mine, scene);
        phase('done');
        return 'loaded: ' + scene.rootNodes.length + ' root node(s)';
      } catch (e) {
        // Keep the last valid scene on screen (the JS preview-state rule);
        // it is no longer the document, so it can never be picked. Only the
        // NEWEST load may abort/retire: an older load superseded mid
        // replaceWorld ("Replacing world aborted") must not discard the
        // newer load's pending generation (VISUAL-3A1 fix).
        if (mine === pick.loadSeq) {
          if (a) a.abort();
          retire('preview-shows-last-valid-scene');
        }
        return 'error (last valid scene kept): ' + String((e && e.message) || e).slice(0, 300);
      }
    },
    // Retire the generation on screen (document changed / closed / replaced).
    retire: function (reason) { retire(reason); },
    // The adapter's plain-data snapshot of one click, as JSON, plus the
    // generation the hit belongs to and the active one at this instant.
    pick: function (clientX, clientY) {
      var t0 = performance.now();
      var c = canvas();
      var a = c && c.browser ? adapterFor(c.browser) : null;
      var s = a ? a.pick(Number(clientX), Number(clientY)) : { outcome: 'disabled', reason: 'adapter-unavailable' };
      var g = s.generation || null;
      return JSON.stringify({
        outcome: s.outcome,
        reason: s.reason || null,
        shape: s.shape || null,
        ctxKind: s.ctxKind || null,
        sensors: s.sensors ? s.sensors.slice() : [],
        graph: s.graph || {},
        generation: g ? { session: g.sessionId, seq: g.overlayGeneration } : null,
        active: pick.active,
        ms: performance.now() - t0,
      });
    },
    // ---- VISUAL-3A translation gizmo (plain data in and out) ----------------
    // The camera as JSON, or '' when it cannot be read.
    gizmoCamera: function () {
      var c = canvas();
      if (c && c.browser) adapterFor(c.browser);
      var cam = pick.gizmo ? pick.gizmo.camera() : null;
      return cam ? JSON.stringify(cam) : '';
    },
    // VISUAL-3A1: the plain-data runtime chain of the ONE runtime node the
    // generation `seq` recorded for the preview span [from, to), for Rust to
    // prove. JSON (a pick-style snapshot, outcome `found` or a refusal).
    // B3: `body` -- the span is a PROTO body statement; the runtime node is
    // the PICKED instance's copy of it (snapshotBodySpan). LV2: `iFrom` /
    // `iTo` (>= 0) -- the copy of the instance with exactly that source span
    // instead (a continuation Rust carried through a commit); never the pick.
    gizmoLocate: function (seq, from, to, body, iFrom, iTo) {
      var c = canvas();
      var a = c && c.browser ? adapterFor(c.browser) : null;
      var s;
      if (!a) s = { outcome: 'disabled', reason: 'adapter-unavailable' };
      else if (!pick.active || pick.active.seq !== seq || !pick.generation) s = { outcome: 'stale', reason: 'preview-scene-replaced' };
      else if (body === true) s = a.snapshotBodySpan(pick.generation, Number(from), Number(to), instHint(iFrom, iTo));
      else s = a.snapshotSpan(pick.generation, Number(from), Number(to));
      return JSON.stringify({
        outcome: s.outcome, reason: s.reason || null, shape: s.shape || null, ctxKind: s.ctxKind || null,
        sensors: [], graph: s.graph || {},
      });
    },
    // Bind the runtime node of [from, to) in generation `seq` -- the one Rust
    // just proved. `index` / `name` / (x, y, z) are assertions only. The
    // binding re-proves (same generation, same span, same object) before
    // every temporary translation. JSON {ok, reason}.
    // B3: `body` -- bind the PICKED instance's copy (bodyNodeAt); the root
    // index and DEF name assertions do not apply (index -1, name '').
    gizmoBind: function (seq, from, to, index, name, x, y, z, body, iFrom, iTo) {
      var c = canvas();
      if (c && c.browser) adapterFor(c.browser);
      if (!pick.gizmo || !pick.adapter) return JSON.stringify({ ok: false, reason: 'adapter-unavailable' });
      if (!pick.active || pick.active.seq !== seq || !pick.scene || !pick.generation) {
        pick.gizmo.unbind();
        return JSON.stringify({ ok: false, reason: 'preview-scene-replaced' });
      }
      var adapter = pick.adapter, gen = pick.generation, f = Number(from), t = Number(to);
      var isBody = body === true, hint = instHint(iFrom, iTo);
      var node = isBody ? adapter.bodyNodeAt(gen, f, t, null, hint) : adapter.nodeAt(gen, f, t);
      var verify = function () {
        return !!pick.active && pick.active.seq === seq && pick.generation === gen && pick.adapter === adapter
          && !!node && (isBody ? adapter.bodyNodeAt(gen, f, t, node, hint) : adapter.nodeAt(gen, f, t)) === node;
      };
      return JSON.stringify(pick.gizmo.bind(pick.scene, node, Number(index), String(name || ''), [Number(x), Number(y), Number(z)], verify));
    },
    // ---- B2 selection highlight (read-only, plain data) ---------------------
    // The rendered bounds of the runtime node generation `seq` recorded for
    // [from, to) -- the span Rust just PROVED -- plus each proven ancestor
    // (`spans`: flat [from, to, ...], the direct parent first, as Rust
    // returned them). Every node comes from nodeAt (exactly one runtime node
    // per span), never a search. JSON {ok, box, ancestors} or {ok, reason}.
    // B3: `body` -- a PROTO body node: the PICKED instance's copy
    // (bodyNodeAt), its ancestors followed up THAT copy's runtime chain and
    // checked span by span (chainOf).
    // LV2: `iFrom` / `iTo` -- the instance hint, as for gizmoLocate.
    // LV2: `geometry` 1 adds the node's shape list (matrices, types, vertex
    // counts), 2 the vertices too -- for the precise outline -- as `shapes`,
    // or `shapesReason` when they cannot be read (the box stays usable).
    highlightBounds: function (seq, from, to, spans, body, iFrom, iTo, geometry) {
      var c = canvas();
      if (c && c.browser) adapterFor(c.browser);
      var out = function (o) { return JSON.stringify(o); };
      if (!pick.gizmo || !pick.adapter) return out({ ok: false, reason: 'adapter-unavailable' });
      if (!pick.active || pick.active.seq !== seq || !pick.generation) return out({ ok: false, reason: 'preview-scene-replaced' });
      var gen = pick.generation, a = pick.adapter;
      var isBody = body === true, hint = instHint(iFrom, iTo);
      var node = isBody ? a.bodyNodeAt(gen, Number(from), Number(to), null, hint) : a.nodeAt(gen, Number(from), Number(to));
      if (!node) return out({ ok: false, reason: !isBody ? 'runtime-node-for-span-not-unique' : hint ? 'proto-instance-not-proven' : 'proto-instance-not-picked' });
      var b = pick.gizmo.bounds(node);
      if (!b.ok) return out(b);
      var list = Array.prototype.slice.call(spans || []).map(Number);
      if (list.length % 2) return out({ ok: false, reason: 'runtime-node-not-proven' });
      var chain = isBody ? a.chainOf(gen, node, list) : null;
      if (isBody && !chain) return out({ ok: false, reason: 'runtime-ancestor-not-proven' });
      var ancestors = [];
      for (var i = 0; i < list.length; i += 2) {
        var an = isBody ? chain[i / 2] : a.nodeAt(gen, Number(list[i]), Number(list[i + 1]));
        var info = an ? pick.gizmo.ancestor(an) : null;
        if (!info) return out({ ok: false, reason: 'runtime-ancestor-not-proven' });
        ancestors.push(info);
      }
      var result = { ok: true, box: b.box, ancestors: ancestors };
      if (geometry === 1 || geometry === 2) {
        var sh = pick.gizmo.shapes(node, geometry === 2);
        if (sh.ok) result.shapes = sh.shapes;
        else result.shapesReason = sh.reason;
      }
      return out(result);
    },
    // B3 (LV2): the document instances rendering a copy of the PROTO body
    // statement [from, to) in generation `seq`, as JSON
    // {instances: [[from, to], ...], unproven} (plain data; the secondary
    // outline proves each one like the picked copy).
    bodyInstances: function (seq, from, to) {
      var c = canvas();
      if (c && c.browser) adapterFor(c.browser);
      if (!pick.adapter || !pick.active || pick.active.seq !== seq || !pick.generation) return JSON.stringify({ instances: [], unproven: 0 });
      var r = pick.adapter.bodyInstances(pick.generation, Number(from), Number(to));
      return JSON.stringify({ instances: r.instances.map(function (i) { return [i.start, i.end]; }), unproven: r.unproven });
    },
    gizmoSet: function (x, y, z) { return !!pick.gizmo && pick.gizmo.set(x, y, z); },
    // VISUAL-4B: name is translation / rotation / scale; values a number array.
    gizmoSetField: function (name, values) { return !!pick.gizmo && pick.gizmo.setField(String(name), values); },
    gizmoFrame: function () {
      var f = pick.gizmo ? pick.gizmo.frame() : null;
      return f ? JSON.stringify(f) : null;
    },
    gizmoRestore: function () { if (pick.gizmo) pick.gizmo.restore(); },
    gizmoRelease: function () { if (pick.gizmo) pick.gizmo.release(); },
    gizmoUnbind: function () { if (pick.gizmo) pick.gizmo.unbind(); },
    // Tests: the bound node's rendered translation as JSON, or ''.
    gizmoRendered: function () {
      var r = pick.gizmo ? pick.gizmo.rendered() : null;
      return r ? JSON.stringify(r) : '';
    },
    // ---- VISUAL-3A1 camera carry (plain data) -------------------------------
    // The identity captured when the last generation retired, without the
    // runtime node: JSON {session, revision, seq, kind, reason, from, to}.
    cameraCapture: function () {
      var k = pick.capture;
      if (!k) return '';
      return JSON.stringify({ session: k.session, revision: k.revision, seq: k.seq, kind: k.kind, reason: k.reason, from: k.from, to: k.to });
    },
    // The newest load's carry result, JSON, or ''.
    cameraResult: function () { return pick.camera ? JSON.stringify(pick.camera) : ''; },
    // ---- B2-CAMERA view controls (plain data) --------------------------------
    // Fit (X_ITE View All). `time`: transition seconds. JSON {ok, reason}.
    cameraFit: function (time) {
      var g = cameraGizmo();
      return JSON.stringify(g ? g.viewAll(Number(time)) : { ok: false, reason: 'adapter-unavailable' });
    },
    // The FrameInput for protocol::camera, JSON (or {reason}).
    cameraFrame: function () {
      var g = cameraGizmo();
      return JSON.stringify(g ? g.frameInput() : { reason: 'adapter-unavailable' });
    },
    // Write Rust's ViewOffsets (JSON) onto the bound viewpoint. JSON {ok, reason}.
    cameraApply: function (json) {
      var g = cameraGizmo();
      var o = null;
      try { o = JSON.parse(String(json)); } catch (e) { o = null; }
      if (!g) return JSON.stringify({ ok: false, reason: 'adapter-unavailable' });
      if (!o) return JSON.stringify({ ok: false, reason: 'bad-view' });
      return JSON.stringify(g.applyView(o));
    },
    // Reset the bound viewpoint's user offsets. JSON {ok, reason, isDefault}.
    cameraReset: function () {
      var g = cameraGizmo();
      return JSON.stringify(g ? g.resetView() : { ok: false, reason: 'adapter-unavailable' });
    },
    // The authored viewpoints of the generation on screen, each by its ONE
    // provenance span: JSON {seq, items: [{from, to, description, name,
    // bound}]}; a viewpoint without exactly one span is not listed.
    cameraViewpoints: function () {
      var g = cameraGizmo();
      var out = { seq: pick.active ? pick.active.seq : null, items: [] };
      if (!g || !pick.adapter || !pick.active || !pick.generation) return JSON.stringify(out);
      var list = g.viewpoints() || [];
      for (var i = 0; i < list.length; i++) {
        var sp = pick.adapter.spansOf(pick.generation, list[i].node);
        if (!sp || sp.length !== 1) continue;
        out.items.push({ from: sp[0].start, to: sp[0].end, description: list[i].description, name: list[i].name, bound: list[i].bound });
      }
      return JSON.stringify(out);
    },
    // Bind the viewpoint at [from, to) of generation `seq`. JSON {ok, reason}.
    cameraBind: function (seq, from, to) {
      var g = cameraGizmo();
      if (!g || !pick.adapter) return JSON.stringify({ ok: false, reason: 'adapter-unavailable' });
      if (!pick.active || pick.active.seq !== seq || !pick.generation) return JSON.stringify({ ok: false, reason: 'preview-scene-replaced' });
      var node = pick.adapter.nodeAt(pick.generation, Number(from), Number(to));
      if (!node) return JSON.stringify({ ok: false, reason: 'viewpoint-not-proven' });
      return JSON.stringify(g.bindViewpoint(node));
    },
    // The newest load's automatic-Fit result, JSON, or ''.
    cameraAutoFit: function () { return pick.autoFit ? JSON.stringify(pick.autoFit) : ''; },
    // Tests: the bound viewpoint's offsets and view matrix, JSON, or ''.
    cameraState: function () {
      var st = pick.gizmo ? pick.gizmo.cameraState() : null;
      if (!st) return '';
      st.view = pick.gizmo.viewMatrix();
      return JSON.stringify(st);
    },
    // Tests: record the bound viewpoint's view matrix on EVERY animation
    // frame from now on, against the matrix at the start.
    cameraTraceStart: function () {
      var ref = pick.gizmo ? pick.gizmo.viewMatrix() : null;
      var tr = { ref: ref, frames: 0, missing: 0, maxDelta: 0, worstFrame: -1, on: true };
      if (pick.trace) pick.trace.on = false;
      pick.trace = tr;
      var step = function () {
        if (!tr.on) return;
        var v = pick.gizmo ? pick.gizmo.viewMatrix() : null;
        var d = maxDelta(tr.ref, v);
        if (d === null) tr.missing += 1;
        else if (d > tr.maxDelta) { tr.maxDelta = d; tr.worstFrame = tr.frames; }
        tr.frames += 1;
        window.requestAnimationFrame(step);
      };
      window.requestAnimationFrame(step);
      return !!ref;
    },
    cameraTraceStop: function () {
      var tr = pick.trace;
      pick.trace = null;
      if (!tr) return '';
      tr.on = false;
      return JSON.stringify({ frames: tr.frames, missing: tr.missing, maxDelta: tr.maxDelta, worstFrame: tr.worstFrame, hadRef: !!tr.ref });
    },
    // XITE-16-UPGRADE-2 tests ONLY (--smoke-texture): between start and
    // stop, copy console log/info/warn/error lines (truncated) and count
    // DOMParser.parseFromString calls, then restore both exactly. Nothing is
    // ever started outside a smoke run, kept after stop, or sent anywhere.
    consoleTraceStart: function () {
      if (consoleTrace) return false;
      var tr = { lines: [], xml: 0, restore: [] };
      ['log', 'info', 'warn', 'error'].forEach(function (k) {
        var orig = console[k];
        console[k] = function () {
          var text = Array.prototype.map.call(arguments, function (a) {
            return a instanceof Error ? a.name + ': ' + a.message : String(a);
          }).join(' ');
          tr.lines.push(k + ': ' + text.slice(0, 600));
          return orig.apply(console, arguments);
        };
        tr.restore.push(function () { console[k] = orig; });
      });
      var P = window.DOMParser && window.DOMParser.prototype;
      if (P && typeof P.parseFromString === 'function') {
        var parse = P.parseFromString;
        P.parseFromString = function () { tr.xml += 1; return parse.apply(this, arguments); };
        tr.restore.push(function () { P.parseFromString = parse; });
      }
      consoleTrace = tr;
      return true;
    },
    consoleTraceStop: function () {
      var tr = consoleTrace;
      consoleTrace = null;
      if (!tr) return '';
      tr.restore.reverse().forEach(function (f) { f(); });
      return JSON.stringify({ lines: tr.lines, xml: tr.xml });
    },
    // {ok, reason}: whether picking is available at all (compatibility).
    pickStatus: function () {
      var c = canvas();
      var a = c && c.browser ? adapterFor(c.browser) : null;
      if (!a) return JSON.stringify({ ok: false, reason: 'adapter-unavailable' });
      return JSON.stringify(a.compatibility());
    },
  };
})();
