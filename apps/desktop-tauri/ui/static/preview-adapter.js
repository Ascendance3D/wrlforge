// SPDX-License-Identifier: GPL-3.0-or-later
// TEMPORARY JavaScript compatibility surface for the X_ITE preview.
//
// X_ITE is a JavaScript renderer; until a native renderer is approved it is
// reached ONLY through the functions below, called from the Rust/Wasm UI.
// The adapter receives TEXT (the canonical buffer the Rust backend returned)
// and never a path. It has no Tauri/IPC access of its own beyond what the
// page has, and the CSP blocks every remote origin, so remote URLs inside a
// world are never fetched. Relative textures resolve against the app origin
// and are simply not found (texture serving is not migrated yet).
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
(function () {
  'use strict';
  function canvas() { return document.getElementById('viewport'); }

  // ---- picking state ---------------------------------------------------------
  // `adapter` belongs to exactly one X_ITE browser; a replaced viewport gets a
  // new one. `active` is the generation on screen: {session, revision, hash,
  // seq} as the UI named it at load time, or null (retired).
  var pick = {
    adapter: null, gizmo: null, browser: null, active: null, scene: null, generation: null, loadSeq: 0,
    // VISUAL-3A1 camera carry: the identity captured at retire, the result
    // of the newest load's carry, and a per-frame view-matrix trace (tests).
    capture: null, camera: null, trace: null,
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
    // `meta` names the generation: {session, revision, hash, seq} from the
    // UI, or null for a scene that is not a document (the empty world after
    // Close). The previous generation is retired before anything else, so a
    // pick during the load is stale, never a hit on the old scene.
    load: async function (text, meta) {
      var c = canvas();
      if (!c || !c.browser) return 'unavailable: X_ITE canvas not initialized';
      var browser = c.browser;
      var a = adapterFor(browser);
      var mine = ++pick.loadSeq;
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
    gizmoLocate: function (seq, from, to) {
      var c = canvas();
      var a = c && c.browser ? adapterFor(c.browser) : null;
      var s;
      if (!a) s = { outcome: 'disabled', reason: 'adapter-unavailable' };
      else if (!pick.active || pick.active.seq !== seq || !pick.generation) s = { outcome: 'stale', reason: 'preview-scene-replaced' };
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
    gizmoBind: function (seq, from, to, index, name, x, y, z) {
      var c = canvas();
      if (c && c.browser) adapterFor(c.browser);
      if (!pick.gizmo || !pick.adapter) return JSON.stringify({ ok: false, reason: 'adapter-unavailable' });
      if (!pick.active || pick.active.seq !== seq || !pick.scene || !pick.generation) {
        pick.gizmo.unbind();
        return JSON.stringify({ ok: false, reason: 'preview-scene-replaced' });
      }
      var adapter = pick.adapter, gen = pick.generation, f = Number(from), t = Number(to);
      var node = adapter.nodeAt(gen, f, t);
      var verify = function () {
        return !!pick.active && pick.active.seq === seq && pick.generation === gen && pick.adapter === adapter
          && !!node && adapter.nodeAt(gen, f, t) === node;
      };
      return JSON.stringify(pick.gizmo.bind(pick.scene, node, Number(index), String(name || ''), [Number(x), Number(y), Number(z)], verify));
    },
    gizmoSet: function (x, y, z) { return !!pick.gizmo && pick.gizmo.set(x, y, z); },
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
    // {ok, reason}: whether picking is available at all (compatibility).
    pickStatus: function () {
      var c = canvas();
      var a = c && c.browser ? adapterFor(c.browser) : null;
      if (!a) return JSON.stringify({ ok: false, reason: 'adapter-unavailable' });
      return JSON.stringify(a.compatibility());
    },
  };
})();
