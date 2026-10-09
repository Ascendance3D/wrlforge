// SPDX-License-Identifier: GPL-3.0-or-later
// TEMPORARY JavaScript compatibility surface for the X_ITE preview.
//
// X_ITE is a JavaScript renderer; until a native renderer is approved it is
// reached ONLY through these two functions, called from the Rust/Wasm UI.
// The adapter receives TEXT (the canonical buffer the Rust backend returned)
// and never a path. It has no Tauri/IPC access of its own beyond what the
// page has, and the CSP blocks every remote origin, so remote URLs inside a
// world are never fetched. Relative textures resolve against the app origin
// and are simply not found (texture serving is not migrated yet).
(function () {
  'use strict';
  function canvas() { return document.getElementById('viewport'); }
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
    load: async function (text) {
      var c = canvas();
      if (!c || !c.browser) return 'unavailable: X_ITE canvas not initialised';
      var browser = c.browser;
      try {
        var scene = await browser.createX3DFromString(String(text == null ? '' : text));
        await browser.replaceWorld(scene);
        return 'loaded: ' + scene.rootNodes.length + ' root node(s)';
      } catch (e) {
        // Keep the last valid scene on screen (the JS preview-state rule).
        return 'error (last valid scene kept): ' + String((e && e.message) || e).slice(0, 300);
      }
    }
  };
})();
