// SPDX-License-Identifier: GPL-3.0-or-later
// VISUAL-3A -- the ONE narrow X_ITE manipulation adapter (Tauri app only).
//
// Every X_ITE surface the translation gizmo needs is touched here and nowhere
// else, and nothing leaves this file except PLAIN DATA (numbers, strings,
// booleans). Two things only:
//
//   1. CAMERA. The active layer's viewpoint view and projection matrices, the
//      layer viewport rectangle and the canvas rectangle, exactly as X_ITE
//      last drew them (PRIVATE X_ITE 15.1.10 surfaces: getActiveLayer,
//      getViewpoint, getViewMatrix, getProjectionMatrix, getViewport,
//      getRectangle). The gizmo math is Rust (protocol::gizmo).
//   2. TEMPORARY TRANSLATION. During a drag, the bound Transform's rendered
//      translation is overridden through the PUBLIC SAI (rootNodes,
//      getNamedNode, getNodeTypeName, the translation field). The source
//      document is not touched; the override belongs to one scene and is
//      restored on cancel.
//
// Binding: Rust proved the node is a top-level Transform and named its index
// among the top-level node statements (X_ITE's VRML parser appends each one
// to rootNodes in source order); a DEF name, if any, is defined exactly once,
// never USEd and never a ROUTE destination. The UI binds only for the
// preview generation that rendered that exact revision. Here the root node
// must ALSO be a Transform in the CURRENT scene, be the node getNamedNode
// returns for the DEF name (when there is one), and render exactly the
// source translation (single precision) -- or nothing binds.
(function () {
  'use strict';
  var XITE_VERSION = '15.1.10';

  function fn(o, name) {
    try { return !!o && typeof o[name] === 'function'; } catch (e) { return false; }
  }

  function m16(m) {
    if (!m) return null;
    var out = new Array(16);
    for (var i = 0; i < 16; i++) {
      var v = Number(m[i]);
      if (!Number.isFinite(v)) return null;
      out[i] = v;
    }
    return out;
  }

  function createXiteGizmoAdapter(opts) {
    var X3D = opts && opts.X3D;
    var browser = opts && opts.browser;
    // {scene, node, field, original:[3], overridden}
    var bound = null;

    function compatibility() {
      if (!X3D || !browser || typeof X3D.SFVec3f !== 'function') return { ok: false, reason: 'adapter-unavailable' };
      // Every private assumption below was proven on exactly this build.
      if (browser.version !== XITE_VERSION) return { ok: false, reason: 'xite-version-mismatch' };
      if (!fn(browser, 'getActiveLayer') || !fn(browser, 'getViewport')) return { ok: false, reason: 'camera-unavailable' };
      return { ok: true };
    }

    // The camera as plain data, or null when it cannot be read.
    function camera() {
      if (!compatibility().ok) return null;
      try {
        var layer = browser.getActiveLayer();
        if (!layer || !fn(layer, 'getViewpoint')) return null;
        var vp = layer.getViewpoint();
        if (!vp || !fn(vp, 'getViewMatrix') || !fn(vp, 'getProjectionMatrix')) return null;
        var view = m16(vp.getViewMatrix());
        var proj = m16(vp.getProjectionMatrix(layer));
        var rectv = layer.getViewport().getRectangle();
        var size = browser.getViewport();
        var el = browser.element;
        var r = el && el.getBoundingClientRect ? el.getBoundingClientRect() : null;
        if (!view || !proj || !rectv || !size || !r) return null;
        var out = {
          view: view,
          proj: proj,
          viewport: [Number(rectv[0]), Number(rectv[1]), Number(rectv[2]), Number(rectv[3])],
          size: [Number(size[2]), Number(size[3])],
          rect: [r.left, r.top, r.width, r.height],
        };
        var all = out.viewport.concat(out.size, out.rect);
        for (var i = 0; i < all.length; i++) if (!Number.isFinite(all[i])) return null;
        return out;
      } catch (e) {
        return null;
      }
    }

    function same(a, b) {
      // The scene holds single-precision floats.
      return Math.abs(a - b) <= 1e-6 + Math.abs(b) * 1e-6;
    }

    function sameNode(a, b) {
      try {
        if (a === b) return true;
        var va = a && typeof a.getValue === 'function' ? a.getValue() : a;
        var vb = b && typeof b.getValue === 'function' ? b.getValue() : b;
        return !!va && va === vb;
      } catch (e) {
        return false;
      }
    }

    // Bind root node `index` of `scene` (the generation's scene); `name` is
    // its DEF name or ''.
    function bind(scene, index, name, expected) {
      unbind();
      if (!compatibility().ok) return { ok: false, reason: 'adapter-unavailable' };
      if (!scene || browser.currentScene !== scene) return { ok: false, reason: 'preview-scene-replaced' };
      var node = null;
      try {
        var roots = scene.rootNodes;
        if (roots && Number.isInteger(index) && index >= 0 && index < roots.length) node = roots[index];
      } catch (e) { node = null; }
      if (!node) return { ok: false, reason: 'node-not-in-preview' };
      if (name) {
        var named = null;
        try { named = scene.getNamedNode(String(name)); } catch (e) { named = null; }
        if (!named || !sameNode(node, named)) return { ok: false, reason: 'preview-node-differs-from-def' };
      }
      var type = null;
      try { type = node.getNodeTypeName(); } catch (e) { type = null; }
      if (type !== 'Transform') return { ok: false, reason: 'preview-node-not-a-transform' };
      var t = node.translation;
      if (!t || !same(Number(t.x), expected[0]) || !same(Number(t.y), expected[1]) || !same(Number(t.z), expected[2])) {
        return { ok: false, reason: 'preview-position-differs-from-source' };
      }
      bound = { scene: scene, node: node, original: [Number(t.x), Number(t.y), Number(t.z)], overridden: false };
      return { ok: true };
    }

    function live() {
      return !!bound && browser.currentScene === bound.scene;
    }

    // Temporary rendered translation (never the document).
    function set(x, y, z) {
      if (!live()) return false;
      var v = [Number(x), Number(y), Number(z)];
      if (!v.every(Number.isFinite)) return false;
      bound.node.translation = new X3D.SFVec3f(v[0], v[1], v[2]);
      bound.overridden = true;
      return true;
    }

    // Put back the rendered translation the scene had at bind time.
    function restore() {
      var b = bound;
      if (b && b.overridden && browser.currentScene === b.scene) {
        try { b.node.translation = new X3D.SFVec3f(b.original[0], b.original[1], b.original[2]); } catch (e) { /* scene gone */ }
      }
      if (b) b.overridden = false;
    }

    // Forget the binding WITHOUT restoring: a committed move stays drawn until
    // the new source revision's scene replaces it.
    function release() {
      bound = null;
    }

    function unbind() {
      restore();
      bound = null;
    }

    // The rendered translation of the bound node (tests), or null.
    function rendered() {
      if (!live()) return null;
      var t = bound.node.translation;
      return [Number(t.x), Number(t.y), Number(t.z)];
    }

    return Object.freeze({ compatibility: compatibility, camera: camera, bind: bind, set: set, restore: restore, release: release, unbind: unbind, rendered: rendered });
  }

  window.WrlXiteGizmoAdapter = Object.freeze({ createXiteGizmoAdapter: createXiteGizmoAdapter });
})();
