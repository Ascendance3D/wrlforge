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
(function () {
  'use strict';
  function canvas() { return document.getElementById('viewport'); }

  // ---- picking state ---------------------------------------------------------
  // `adapter` belongs to exactly one X_ITE browser; a replaced viewport gets a
  // new one. `active` is the generation on screen: {session, revision, hash,
  // seq} as the UI named it at load time, or null (retired).
  var pick = { adapter: null, gizmo: null, browser: null, active: null, scene: null, loadSeq: 0 };

  function adapterFor(browser) {
    if (pick.browser !== browser) {
      if (pick.adapter) pick.adapter.dispose();
      if (pick.gizmo) pick.gizmo.unbind();
      pick.adapter = null;
      pick.gizmo = null;
      pick.active = null;
      pick.scene = null;
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
  function retire(reason) {
    pick.active = null;
    pick.scene = null;
    if (pick.gizmo) pick.gizmo.unbind();
    if (pick.adapter) pick.adapter.retire(reason || 'preview-scene-replaced');
  }

  window.addEventListener('pagehide', function () {
    pick.active = null;
    pick.scene = null;
    if (pick.gizmo) pick.gizmo.unbind();
    pick.gizmo = null;
    if (pick.adapter) pick.adapter.dispose();
    pick.adapter = null;
  });

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
        await browser.replaceWorld(scene);
        // Only the newest load may become the picking generation.
        if (mine === pick.loadSeq && a && generation && a.activate(generation, scene)) {
          pick.active = { session: meta.session, revision: meta.revision, hash: meta.hash, seq: meta.seq };
          pick.scene = scene;
        }
        return 'loaded: ' + scene.rootNodes.length + ' root node(s)';
      } catch (e) {
        // Keep the last valid scene on screen (the JS preview-state rule);
        // it is no longer the document, so it can never be picked.
        if (a) a.abort();
        if (mine === pick.loadSeq) retire('preview-shows-last-valid-scene');
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
    // Bind root node `index` (DEF `name`, or '') of the generation on
    // screen, which must be `seq` and show the source translation (x, y, z).
    // JSON {ok, reason}.
    gizmoBind: function (seq, index, name, x, y, z) {
      var c = canvas();
      if (c && c.browser) adapterFor(c.browser);
      if (!pick.gizmo) return JSON.stringify({ ok: false, reason: 'adapter-unavailable' });
      if (!pick.active || pick.active.seq !== seq || !pick.scene) {
        pick.gizmo.unbind();
        return JSON.stringify({ ok: false, reason: 'preview-scene-replaced' });
      }
      return JSON.stringify(pick.gizmo.bind(pick.scene, Number(index), String(name || ''), [Number(x), Number(y), Number(z)]));
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
    // {ok, reason}: whether picking is available at all (compatibility).
    pickStatus: function () {
      var c = canvas();
      var a = c && c.browser ? adapterFor(c.browser) : null;
      if (!a) return JSON.stringify({ ok: false, reason: 'adapter-unavailable' });
      return JSON.stringify(a.compatibility());
    },
  };
})();
