// SPDX-License-Identifier: GPL-3.0-or-later
// VISUAL-3A / 3A1 -- the ONE narrow X_ITE manipulation adapter (Tauri app).
//
// Every X_ITE surface the translation gizmo and the camera carry need is
// touched here and nowhere else, and nothing leaves this file toward Rust
// except PLAIN DATA (numbers, strings, booleans). Three things only:
//
//   1. CAMERA. The active layer's viewpoint view and projection matrices, the
//      layer viewport rectangle and the canvas rectangle, exactly as X_ITE
//      last drew them (PRIVATE X_ITE 15.1.10 surfaces: getActiveLayer,
//      getViewpoint, getViewMatrix, getProjectionMatrix, getViewport,
//      getRectangle). The gizmo math is Rust (protocol::gizmo).
//   2. TEMPORARY TRANSLATION. During a drag, the bound Transform's rendered
//      translation is overridden through the PUBLIC SAI (an SFNode over the
//      proven runtime node; its translation field). The source document is
//      not touched; the override belongs to one scene and is restored on
//      cancel.
//   3. CAMERA CARRY (VISUAL-3A1). The bound viewpoint's user offsets -- what
//      orbit, pan and zoom change (PRIVATE: positionOffset,
//      orientationOffset, scaleOffset, scaleOrientationOffset,
//      centerOfRotationOffset, fieldOfViewScale, the layer's
//      defaultViewpoint, X3DViewpointNode.update, getWorld().getActiveLayer)
//      -- read before a scene is
//      replaced and written onto the PROVEN same viewpoint after. Never an
//      authored field, never the source.
//
// Binding (VISUAL-3A1): the runtime node is NOT found here. The pick
// adapter's provenance generation names the ONE runtime node X_ITE's parser
// created for the authored Transform's exact span, and Rust proved its
// runtime chain (wrlforge_vrml::pick::resolve_node). bind() only receives
// that node. The root index, the DEF name and the rendered translation are
// SECONDARY consistency assertions: any disagreement refuses; agreement
// establishes nothing. A `verify` callback re-proves the node (same
// generation, same span, same object) before every temporary translation.
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

    // Bind the PROVEN runtime node `node` (from the pick adapter's nodeAt)
    // of `scene`. `index` / `name` / `expected` are assertions only (root
    // position, DEF name or '', source translation). `verify()` must stay
    // true for the binding to be used.
    function bind(scene, node, index, name, expected, verify) {
      unbind();
      if (!compatibility().ok || typeof X3D.SFNode !== 'function') return { ok: false, reason: 'adapter-unavailable' };
      if (!scene || browser.currentScene !== scene) return { ok: false, reason: 'preview-scene-replaced' };
      if (!node || typeof node !== 'object') return { ok: false, reason: 'runtime-node-not-proven' };
      if (typeof verify !== 'function' || verify() !== true) return { ok: false, reason: 'runtime-node-not-proven' };
      var sf = null;
      try { sf = new X3D.SFNode(node); } catch (e) { sf = null; }
      var wraps = false;
      try { wraps = !!sf && sf.getValue() === node; } catch (e) { wraps = false; }
      if (!wraps) return { ok: false, reason: 'runtime-node-not-proven' };
      var type = null;
      try { type = sf.getNodeTypeName(); } catch (e) { type = null; }
      if (type !== 'Transform') return { ok: false, reason: 'preview-node-not-a-transform' };
      // Assertion: the proven node sits at the root position Rust computed.
      var atIndex = null;
      try {
        var roots = scene.rootNodes;
        if (roots && Number.isInteger(index) && index >= 0 && index < roots.length) atIndex = roots[index];
      } catch (e) { atIndex = null; }
      if (!atIndex || !sameNode(atIndex, sf)) return { ok: false, reason: 'preview-root-order-disagrees' };
      if (name) {
        var named = null;
        try { named = scene.getNamedNode(String(name)); } catch (e) { named = null; }
        if (!named || !sameNode(sf, named)) return { ok: false, reason: 'preview-node-differs-from-def' };
      }
      var t = sf.translation;
      if (!t || !same(Number(t.x), expected[0]) || !same(Number(t.y), expected[1]) || !same(Number(t.z), expected[2])) {
        return { ok: false, reason: 'preview-position-differs-from-source' };
      }
      bound = { scene: scene, node: sf, verify: verify, original: [Number(t.x), Number(t.y), Number(t.z)], overridden: false };
      return { ok: true };
    }

    function live() {
      if (!bound || browser.currentScene !== bound.scene) return false;
      try { return bound.verify() === true; } catch (e) { return false; }
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

    // ---- VISUAL-3A1 camera carry ---------------------------------------------
    var OFFSETS = ['positionOffset', 'orientationOffset', 'scaleOffset', 'scaleOrientationOffset', 'centerOfRotationOffset'];

    function finiteAll(v) {
      for (var i = 0; i < v.length; i++) if (!Number.isFinite(v[i])) return false;
      return true;
    }

    // The CURRENT world's active layer's bound viewpoint (runtime object, JS
    // layer only) and whether it is the layer's built-in default viewpoint;
    // null if unknown. The world's own layer, not browser.getActiveLayer():
    // the browser adopts a new world's layer only at its `initialized`
    // event, after the restore must already have happened.
    function boundViewpoint() {
      if (!compatibility().ok || !fn(browser, 'getWorld')) return null;
      try {
        var world = browser.getWorld();
        var layer = world && fn(world, 'getActiveLayer') ? world.getActiveLayer() : null;
        if (!layer || !fn(layer, 'getViewpoint') || !('defaultViewpoint' in layer)) return null;
        var vp = layer.getViewpoint();
        if (!vp) return null;
        return { node: vp, isDefault: vp === layer.defaultViewpoint };
      } catch (e) {
        return null;
      }
    }

    // The viewpoint's user offsets as private SF copies, or null.
    function readCamera(vp) {
      try {
        var out = {};
        for (var i = 0; i < OFFSETS.length; i++) {
          var f = vp['_' + OFFSETS[i]];
          if (!f || typeof f.copy !== 'function') return null;
          var c = f.copy();
          var nums = 'angle' in c ? [c.x, c.y, c.z, c.angle] : [c.x, c.y, c.z];
          if (!finiteAll(nums.map(Number))) return null;
          out[OFFSETS[i]] = c;
        }
        var fov = vp._fieldOfViewScale;
        var s = fov && typeof fov.getValue === 'function' ? Number(fov.getValue()) : NaN;
        if (!(s > 0) || !Number.isFinite(s)) return null;
        out.fieldOfViewScale = s;
        return out;
      } catch (e) {
        return null;
      }
    }

    // Write offsets read by readCamera onto `vp` and recompute its matrices.
    function writeCamera(vp, state) {
      if (!vp || !state || !fn(vp, 'update')) return false;
      try {
        for (var i = 0; i < OFFSETS.length; i++) vp['_' + OFFSETS[i]] = state[OFFSETS[i]];
        vp._fieldOfViewScale = state.fieldOfViewScale;
        vp.update();
        return true;
      } catch (e) {
        return false;
      }
    }

    // The bound viewpoint's view matrix as plain data, or null.
    function viewMatrix() {
      var b = boundViewpoint();
      if (!b || !fn(b.node, 'getViewMatrix')) return null;
      try { return m16(b.node.getViewMatrix()); } catch (e) { return null; }
    }

    // Plain-data camera state of the bound viewpoint (tests), or null.
    function cameraState() {
      var b = boundViewpoint();
      var c = b ? readCamera(b.node) : null;
      if (!c) return null;
      var o = {};
      for (var i = 0; i < OFFSETS.length; i++) {
        var v = c[OFFSETS[i]];
        o[OFFSETS[i]] = 'angle' in v ? [v.x, v.y, v.z, v.angle].map(Number) : [v.x, v.y, v.z].map(Number);
      }
      o.fieldOfViewScale = c.fieldOfViewScale;
      o.isDefault = b.isDefault;
      return o;
    }

    // The rendered translation of the bound node (tests), or null.
    function rendered() {
      if (!live()) return null;
      var t = bound.node.translation;
      return [Number(t.x), Number(t.y), Number(t.z)];
    }

    return Object.freeze({
      compatibility: compatibility, camera: camera, bind: bind, set: set, restore: restore, release: release, unbind: unbind, rendered: rendered,
      boundViewpoint: boundViewpoint, readCamera: readCamera, writeCamera: writeCamera, viewMatrix: viewMatrix, cameraState: cameraState,
    });
  }

  window.WrlXiteGizmoAdapter = Object.freeze({ createXiteGizmoAdapter: createXiteGizmoAdapter });
})();
