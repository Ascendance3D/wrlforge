// SPDX-License-Identifier: GPL-3.0-or-later
// VISUAL-3A / 3A1 / B2 -- the ONE narrow X_ITE manipulation adapter (Tauri app).
//
// Every X_ITE surface the translation gizmo, the camera carry and the selection
// highlight need is touched here and nowhere else, and nothing leaves this file
// toward Rust except PLAIN DATA (numbers, strings, booleans). Four things only:
//
//   1. CAMERA. The active layer's viewpoint view and projection matrices, the
//      layer viewport rectangle and the canvas rectangle, exactly as X_ITE
//      last drew them (PRIVATE X_ITE 16.5.0 surfaces, as in 15.1.10: getActiveLayer,
//      getViewpoint, getViewMatrix, getProjectionMatrix, getViewport,
//      getRectangle). The gizmo math is Rust (protocol::gizmo).
//   2. TEMPORARY FRAME. During a drag, the bound Transform's rendered
//      translation, rotation or scale (VISUAL-4B) is overridden through the
//      PUBLIC SAI (an SFNode over the proven runtime node; that field). The
//      source document is not touched; the override belongs to one scene and
//      is restored on cancel.
//   3. CAMERA CARRY (VISUAL-3A1). The bound viewpoint's user offsets -- what
//      orbit, pan and zoom change (PRIVATE: positionOffset,
//      orientationOffset, scaleOffset, scaleOrientationOffset,
//      centerOfRotationOffset, fieldOfViewScale, the layer's
//      defaultViewpoint, X3DViewpointNode.update, getWorld().getActiveLayer)
//      -- read before a scene is
//      replaced and written onto the PROVEN same viewpoint after. Never an
//      authored field, never the source.
//   4. VIEW CONTROLS (B2-CAMERA). Fit uses X_ITE's PUBLIC browser.viewAll
//      (layer, transitionTime). The standard views read the scene bounds in
//      the bound viewpoint's coordinate system (PRIVATE: X3D.Box3,
//      X3DLayerNode.getBBox, getModelMatrix, getFieldOfView,
//      getViewport().getRectangle) and write the user offsets Rust computed
//      (protocol::camera), then re-check the result through
//      getUserPosition / getUserOrientation; a mismatch puts the old
//      offsets back. Both measure the GEOMETRY: an authored bboxCenter /
//      bboxSize is only a culling hint (ISO/IEC 14772-1 4.6.4) and can be
//      wrong -- a real mall item's exporter flipped its Z, so framing the
//      hint missed the car -- so while the bounds are read, synchronously,
//      each hinted node's isDefaultBBoxSize is shadowed (PRIVATE: _children,
//      getInnerNode, isDefaultBBoxSize) and put back right after. Reset uses
//      resetUserOffsets. The viewpoint list reads
//      the layer's traversed viewpoints (getViewpoints) and binds one
//      through the PUBLIC browser.bindViewpoint. All of it is editor view
//      state: never an authored field, never the source.
//
//   5. SELECTION HIGHLIGHT (B2). The rendered bounding box of a PROVEN
//      runtime node and its proven ancestors' frames, read-only (PRIVATE:
//      X3D.Box3, getInnerNode, getBBox; PUBLIC SAI for Transform fields).
//      LV2: the shapes X_ITE draws for that same node, for the precise
//      outline (PRIVATE: X3D.Matrix4, X3DNode.getShapes, Shape.getGeometry,
//      X3DGeometryNode.getGeometryType / getVertices), read-only.
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
  // Tauri-only: exactly the build in apps/desktop-tauri/xite/PIN. build-ui.sh
  // refuses to embed a dist this constant does not name.
  var XITE_VERSION = '16.5.0';

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
    // {scene, node, verify, original:{translation, rotation, scale} (SF
    //  copies), overridden:{name: true}}
    var bound = null;
    var FIELDS = ['translation', 'rotation', 'scale'];

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

    // Bind the PROVEN runtime node `node` (from the pick adapter's nodeAt,
    // or bodyNodeAt for a PROTO body node: index -1, name '') of `scene`.
    // `index` / `name` / `expected` are assertions only (root position, DEF
    // name or '', source translation). `verify()` must stay
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
      // B3: index -1 is a PROTO body node -- the picked instance's copy,
      // which is no root node; Rust proved its chain through the instance.
      if (index !== -1) {
        var atIndex = null;
        try {
          var roots = scene.rootNodes;
          if (roots && Number.isInteger(index) && index >= 0 && index < roots.length) atIndex = roots[index];
        } catch (e) { atIndex = null; }
        if (!atIndex || !sameNode(atIndex, sf)) return { ok: false, reason: 'preview-root-order-disagrees' };
      }
      if (name) {
        var named = null;
        try { named = scene.getNamedNode(String(name)); } catch (e) { named = null; }
        if (!named || !sameNode(sf, named)) return { ok: false, reason: 'preview-node-differs-from-def' };
      }
      var t = sf.translation;
      if (!t || !same(Number(t.x), expected[0]) || !same(Number(t.y), expected[1]) || !same(Number(t.z), expected[2])) {
        return { ok: false, reason: 'preview-position-differs-from-source' };
      }
      var original = {};
      try {
        for (var i = 0; i < FIELDS.length; i++) {
          var f = sf[FIELDS[i]];
          if (!f || typeof f.copy !== 'function') return { ok: false, reason: 'runtime-node-not-proven' };
          original[FIELDS[i]] = f.copy();
        }
      } catch (e) {
        return { ok: false, reason: 'runtime-node-not-proven' };
      }
      bound = { scene: scene, node: sf, verify: verify, original: original, overridden: {} };
      return { ok: true };
    }

    function live() {
      if (!bound || browser.currentScene !== bound.scene) return false;
      try { return bound.verify() === true; } catch (e) { return false; }
    }

    // Temporary rendered translation (never the document).
    function set(x, y, z) {
      return setField('translation', [x, y, z]);
    }

    // VISUAL-4B: a temporary rendered translation / rotation (x y z angle)
    // / scale of the bound node (never the document).
    function setField(name, values) {
      if (!live() || FIELDS.indexOf(name) < 0 || !values) return false;
      var v = Array.prototype.map.call(values, Number);
      if (!v.every(Number.isFinite)) return false;
      if (name === 'rotation') {
        if (v.length !== 4 || typeof X3D.SFRotation !== 'function') return false;
        bound.node.rotation = new X3D.SFRotation(v[0], v[1], v[2], v[3]);
      } else {
        if (v.length !== 3) return false;
        bound.node[name] = new X3D.SFVec3f(v[0], v[1], v[2]);
      }
      bound.overridden[name] = true;
      return true;
    }

    // Put back every rendered field the scene had at bind time.
    function restore() {
      var b = bound;
      if (b && browser.currentScene === b.scene) {
        for (var i = 0; i < FIELDS.length; i++) {
          if (!b.overridden[FIELDS[i]]) continue;
          try { b.node[FIELDS[i]] = b.original[FIELDS[i]].copy(); } catch (e) { /* scene gone */ }
        }
      }
      if (b) b.overridden = {};
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

    function finiteOrUndefined(v) {
      return typeof v === 'number' && Number.isFinite(v) ? v : undefined;
    }

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
        // B2-CAMERA: the clip planes X_ITE's View All chose for the framed
        // distance (plain numbers or undefined), so a Fit survives a reload
        // exactly.
        out.nearDistance = finiteOrUndefined(vp.nearDistance);
        out.farDistance = finiteOrUndefined(vp.farDistance);
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
        if ('nearDistance' in state) vp.nearDistance = state.nearDistance;
        if ('farDistance' in state) vp.farDistance = state.farDistance;
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

    // VISUAL-4B: the bound node's rendered frame as plain numbers, or null.
    // Rust compares it with the source (a secondary assertion only).
    function frame() {
      if (!live()) return null;
      try {
        var t = bound.node.translation, r = bound.node.rotation, s = bound.node.scale;
        var out = {
          translation: [t.x, t.y, t.z].map(Number),
          rotation: [r.x, r.y, r.z, r.angle].map(Number),
          scale: [s.x, s.y, s.z].map(Number),
        };
        return finiteAll(out.translation.concat(out.rotation, out.scale)) ? out : null;
      } catch (e) {
        return null;
      }
    }

    // ---- B2-CAMERA view controls ---------------------------------------------
    // The current world's active layer and its bound viewpoint, or null.
    function boundLayer() {
      if (!compatibility().ok || !fn(browser, 'getWorld')) return null;
      try {
        var world = browser.getWorld();
        var layer = world && fn(world, 'getActiveLayer') ? world.getActiveLayer() : null;
        if (!layer || !fn(layer, 'getViewpoint') || !('defaultViewpoint' in layer)) return null;
        var vp = layer.getViewpoint();
        return vp ? { layer: layer, node: vp, isDefault: vp === layer.defaultViewpoint } : null;
      } catch (e) {
        return null;
      }
    }

    // The grouping nodes of the current scene that carry an authored bbox
    // hint, reached through children, Switch choices, LOD levels and
    // PROTO instances' first body node (Inline content is not entered).
    function hintedNodes() {
      var out = [], seen = new Set(), stack = [];
      var push = function (list) {
        var arr = null;
        try { arr = list && fn(list, 'getValue') ? list.getValue() : list; } catch (e) { arr = null; }
        if (!arr || typeof arr.length !== 'number') return;
        for (var i = 0; i < arr.length; i++) {
          var el = arr[i];
          try { stack.push(el && fn(el, 'getValue') ? el.getValue() : el); } catch (e) { /* skip */ }
        }
      };
      try { push(browser.currentScene.rootNodes); } catch (e) { return out; }
      while (stack.length && seen.size < 1000000) {
        var n = stack.pop();
        if (!n || typeof n !== 'object' || seen.has(n)) continue;
        seen.add(n);
        try {
          var inner = fn(n, 'getInnerNode') ? n.getInnerNode() : n;
          if (inner && inner !== n) { stack.push(inner); continue; }
        } catch (e) { continue; }
        try { if (fn(n, 'isDefaultBBoxSize') && !n.isDefaultBBoxSize()) out.push(n); } catch (e) { /* no hint */ }
        push(n._children);
        push(n._choice);
        push(n._level);
      }
      return out;
    }

    // Run f() with every bbox hint ignored (geometry bounds), then restore.
    function withGeometryBounds(f) {
      var hinted = hintedNodes();
      var always = function () { return true; };
      for (var i = 0; i < hinted.length; i++) hinted[i].isDefaultBBoxSize = always;
      try {
        return f();
      } finally {
        for (var j = 0; j < hinted.length; j++) delete hinted[j].isDefaultBBoxSize;
      }
    }

    function isViewpoint(vp) {
      try { return !!vp && fn(vp, 'getTypeName') && vp.getTypeName() === 'Viewpoint'; } catch (e) { return false; }
    }

    // Fit: X_ITE's own View All on the bound viewpoint (PUBLIC). `time` is
    // the transition in seconds (0 = immediate). {ok, reason}.
    function viewAll(time) {
      var b = boundLayer();
      if (!b) return { ok: false, reason: 'camera-unavailable' };
      if (!fn(browser, 'viewAll')) return { ok: false, reason: 'view-all-unavailable' };
      var t = Number(time);
      try {
        withGeometryBounds(function () { browser.viewAll(b.layer, Number.isFinite(t) && t >= 0 ? t : 0); });
        return { ok: true, isDefault: b.isDefault };
      } catch (e) {
        return { ok: false, reason: 'view-all-failed' };
      }
    }

    function v3(f) { return [Number(f.x), Number(f.y), Number(f.z)]; }

    // The plain numbers protocol::camera::FrameInput needs, or {reason}.
    function frameInput() {
      var b = boundLayer();
      if (!b || typeof X3D.Box3 !== 'function') return { reason: 'camera-unavailable' };
      var vp = b.node, layer = b.layer;
      if (!isViewpoint(vp)) return { reason: 'viewpoint-type-unsupported' };
      if (!fn(layer, 'getBBox') || !fn(layer, 'getViewport') || !fn(vp, 'getModelMatrix') || !fn(vp, 'getFieldOfView')) {
        return { reason: 'camera-unavailable' };
      }
      try {
        var box = withGeometryBounds(function () { return layer.getBBox(new X3D.Box3()); });
        var local = box.copy().multRight(vp.getModelMatrix().copy().inverse());
        var size = local.size, center = local.center;
        var rect = layer.getViewport().getRectangle();
        var out = {
          center: [Number(center.x), Number(center.y), Number(center.z)],
          size: [Number(size.x), Number(size.y), Number(size.z)],
          position: v3(vp._position),
          orientation: [Number(vp._orientation.x), Number(vp._orientation.y), Number(vp._orientation.z), Number(vp._orientation.angle)],
          centerOfRotation: v3(vp._centerOfRotation),
          fieldOfView: Number(vp.getFieldOfView()),
          viewport: [Number(rect[2]), Number(rect[3])],
        };
        var all = out.center.concat(out.size, out.position, out.orientation, out.centerOfRotation, out.viewport, [out.fieldOfView]);
        if (!finiteAll(all)) return { reason: 'camera-unreadable' };
        // LV2-C Frame Selection: viewpoint -> world and the current user
        // orientation offset (plain numbers; omitted when unreadable).
        var model = m16(vp.getModelMatrix());
        var oo = vp._orientationOffset;
        var ooNums = oo ? [oo.x, oo.y, oo.z, oo.angle].map(Number) : null;
        if (model) out.model = model;
        if (ooNums && finiteAll(ooNums)) out.orientationOffset = ooNums;
        return out;
      } catch (e) {
        return { reason: 'camera-unreadable' };
      }
    }

    function near(a, b) {
      return Math.abs(a - b) <= 1e-4 * Math.max(1, Math.abs(a), Math.abs(b));
    }

    // Write the user offsets Rust computed for a standard view onto the
    // bound viewpoint, then re-check the user pose they produce. {ok, reason}.
    function applyView(o) {
      var b = boundLayer();
      if (!b || !isViewpoint(b.node)) return { ok: false, reason: 'camera-unavailable' };
      var vp = b.node;
      if (!fn(vp, 'update') || !fn(vp, 'getUserPosition') || !fn(vp, 'getUserOrientation') || typeof X3D.SFRotation !== 'function') {
        return { ok: false, reason: 'camera-unavailable' };
      }
      var before = readCamera(vp);
      if (!before) return { ok: false, reason: 'camera-unreadable' };
      try {
        var p = o.positionOffset, r = o.orientationOffset, c = o.centerOfRotationOffset;
        var nums = p.concat(r, c, o.userPosition, o.userOrientation, [o.nearDistance]).map(Number);
        if (nums.length !== 18 || !finiteAll(nums)) return { ok: false, reason: 'bad-view' };
        vp._positionOffset = new X3D.SFVec3f(p[0], p[1], p[2]);
        vp._orientationOffset = new X3D.SFRotation(r[0], r[1], r[2], r[3]);
        vp._scaleOffset = new X3D.SFVec3f(1, 1, 1);
        vp._scaleOrientationOffset = new X3D.SFRotation(0, 0, 1, 0);
        vp._centerOfRotationOffset = new X3D.SFVec3f(c[0], c[1], c[2]);
        vp._fieldOfViewScale = 1;
        vp.nearDistance = Number(o.nearDistance);
        vp.farDistance = fn(vp, 'getMaxFarValue') ? vp.nearDistance * vp.getMaxFarValue() / 0.125 : undefined;
        vp.update();
        // Re-check: the renderer's own user pose must be the requested one.
        var up = vp.getUserPosition();
        var dir = vp.getUserOrientation().multVecRot(vp.getUserPosition().set(0, 0, -1));
        var top = vp.getUserOrientation().multVecRot(vp.getUserPosition().set(0, 1, 0));
        var wantDir = rotate(o.userOrientation, [0, 0, -1]), wantUp = rotate(o.userOrientation, [0, 1, 0]);
        var ok = near(up.x, o.userPosition[0]) && near(up.y, o.userPosition[1]) && near(up.z, o.userPosition[2])
          && near(dir.x, wantDir[0]) && near(dir.y, wantDir[1]) && near(dir.z, wantDir[2])
          && near(top.x, wantUp[0]) && near(top.y, wantUp[1]) && near(top.z, wantUp[2]);
        if (!ok) {
          writeCamera(vp, before);
          return { ok: false, reason: 'view-not-verified' };
        }
        return { ok: true };
      } catch (e) {
        writeCamera(vp, before);
        return { ok: false, reason: 'view-write-failed' };
      }
    }

    // Plain-number rotation of v by a VRML axis-angle (the re-check only).
    function rotate(r, v) {
      var n = Math.hypot(r[0], r[1], r[2]);
      if (!(n > 1e-12)) return v.slice();
      var k = [r[0] / n, r[1] / n, r[2] / n], cs = Math.cos(r[3]), sn = Math.sin(r[3]);
      var d = k[0] * v[0] + k[1] * v[1] + k[2] * v[2];
      var x = [k[1] * v[2] - k[2] * v[1], k[2] * v[0] - k[0] * v[2], k[0] * v[1] - k[1] * v[0]];
      return [0, 1, 2].map(function (i) { return v[i] * cs + x[i] * sn + k[i] * d * (1 - cs); });
    }

    // Reset: the bound viewpoint as authored (zero user offsets).
    function resetView() {
      var b = boundLayer();
      if (!b || !fn(b.node, 'resetUserOffsets')) return { ok: false, reason: 'camera-unavailable' };
      try {
        b.node.resetUserOffsets();
        return { ok: true, isDefault: b.isDefault };
      } catch (e) {
        return { ok: false, reason: 'reset-failed' };
      }
    }

    // The authored Viewpoint nodes the layer traversed (runtime objects, JS
    // only), in scene order, once each: [{node, description, name, bound}].
    function viewpoints() {
      var b = boundLayer();
      if (!b || !fn(b.layer, 'getViewpoints')) return null;
      try {
        var list = b.layer.getViewpoints();
        var all = list && fn(list, 'get') ? list.get() : null;
        if (!all) return null;
        var seen = new Set(), out = [];
        for (var i = 0; i < all.length; i++) {
          var vp = all[i];
          if (!vp || seen.has(vp) || vp === b.layer.defaultViewpoint || !isViewpoint(vp)) continue;
          seen.add(vp);
          var d = '';
          try { d = String(vp._description.getValue()); } catch (e) { d = ''; }
          var name = '';
          try { name = fn(vp, 'getName') ? String(vp.getName() || '') : ''; } catch (e) { name = ''; }
          out.push({ node: vp, description: d, name: name, bound: vp === b.node });
        }
        return out;
      } catch (e) {
        return null;
      }
    }

    // ---- B2 selection highlight (read-only, plain data out) ------------------
    // The rendered bounding box of a PROVEN runtime node (the pick adapter's
    // nodeAt for a span Rust proved): X_ITE's Box3 frame -- a matrix mapping
    // the cube [-1, 1]^3 onto the box -- in the node's PARENT coordinates
    // (PRIVATE X_ITE 16.5.0 surfaces: X3D.Box3, X3DNode.getInnerNode,
    // X3DNode.getBBox). A PROTO instance is measured through getInnerNode,
    // X_ITE's own "the node this instance renders as". Nothing is written.
    function bounds(node) {
      if (!compatibility().ok || typeof X3D.Box3 !== 'function') return { ok: false, reason: 'bounds-unavailable' };
      if (!node || typeof node !== 'object') return { ok: false, reason: 'runtime-node-not-proven' };
      try {
        var inner = fn(node, 'getInnerNode') ? node.getInnerNode() : node;
        if (!inner || !fn(inner, 'getBBox')) return { ok: false, reason: 'node-has-no-rendered-bounds' };
        // Geometry nodes return their own box and ignore the argument.
        var b = inner.getBBox(new X3D.Box3(), false);
        var m = b ? m16(b.matrix) : null;
        return m ? { ok: true, box: m } : { ok: false, reason: 'node-has-no-rendered-bounds' };
      } catch (e) {
        return { ok: false, reason: 'node-has-no-rendered-bounds' };
      }
    }

    // LV2 precise outline (read-only, plain data out): the shapes X_ITE
    // itself draws for a PROVEN runtime node -- X3DNode.getShapes(list,
    // matrix), the traversal X_ITE's StaticGroup uses: visible children
    // only, each with its shape -> node-parent matrix -- and each shape's
    // geometry type (0 points, 1 lines, 2/3 triangles) and rendered vertex
    // count. `withVertices`: also the vertices (x y z per vertex, the
    // shape's own coordinates; X_ITE stores x y z w). A geometry node
    // itself is its own single shape. More than `maxVertices` in total,
    // or any unreadable shape, refuses: a partial outline would not be the
    // node's outline. Nothing is written.
    function shapes(node, withVertices, maxVertices) {
      if (!compatibility().ok || typeof X3D.Matrix4 !== 'function') return { ok: false, reason: 'geometry-unavailable' };
      if (!node || typeof node !== 'object') return { ok: false, reason: 'runtime-node-not-proven' };
      var max = Number(maxVertices) > 0 ? Number(maxVertices) : 300000;
      try {
        var inner = fn(node, 'getInnerNode') ? node.getInnerNode() : node;
        var list;
        if (inner && fn(inner, 'getShapes')) {
          list = inner.getShapes([], new X3D.Matrix4());
        } else if (inner && fn(inner, 'getVertices') && fn(inner, 'getGeometryType')) {
          list = [{ modelMatrix: null, geometry: inner }];
        } else {
          return { ok: false, reason: 'node-has-no-rendered-geometry' };
        }
        if (!Array.isArray(list)) return { ok: false, reason: 'geometry-unavailable' };
        var out = [], total = 0;
        for (var i = 0; i < list.length; i++) {
          var e = list[i] || {};
          var g = e.geometry || (e.shapeNode && fn(e.shapeNode, 'getGeometry') ? e.shapeNode.getGeometry() : null);
          if (!g || !fn(g, 'getVertices') || !fn(g, 'getGeometryType')) return { ok: false, reason: 'geometry-unavailable' };
          var k = Number(g.getGeometryType());
          var verts = g.getVertices();
          if (!verts || typeof verts.length !== 'number' || verts.length % 4 || !(k >= 0 && k <= 3)) return { ok: false, reason: 'geometry-unavailable' };
          var n = verts.length / 4;
          total += n;
          if (total > max) return { ok: false, reason: 'geometry-too-detailed' };
          var m = e.modelMatrix ? m16(e.modelMatrix) : [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
          if (!m) return { ok: false, reason: 'geometry-unavailable' };
          if (!withVertices) {
            out.push({ m: m, k: k, n: n });
            continue;
          }
          var v = new Array(n * 3);
          for (var j = 0; j < n; j++) {
            v[3 * j] = Number(verts[4 * j]);
            v[3 * j + 1] = Number(verts[4 * j + 1]);
            v[3 * j + 2] = Number(verts[4 * j + 2]);
          }
          if (!finiteAll(v)) return { ok: false, reason: 'geometry-unavailable' };
          out.push({ m: m, k: k, n: n, v: v });
        }
        return { ok: true, shapes: out };
      } catch (e) {
        return { ok: false, reason: 'geometry-unavailable' };
      }
    }

    // A proven ancestor's runtime type and, for a Transform, its rendered
    // fields (PUBLIC SAI only), or null when unreadable.
    function ancestor(node) {
      if (!compatibility().ok || typeof X3D.SFNode !== 'function' || !node || typeof node !== 'object') return null;
      try {
        var sf = new X3D.SFNode(node);
        if (sf.getValue() !== node) return null;
        var type = sf.getNodeTypeName();
        if (typeof type !== 'string') return null;
        if (type !== 'Transform') return { type: type };
        var t = sf.translation, r = sf.rotation, s = sf.scale, so = sf.scaleOrientation, c = sf.center;
        var f = {
          translation: [t.x, t.y, t.z].map(Number),
          rotation: [r.x, r.y, r.z, r.angle].map(Number),
          scale: [s.x, s.y, s.z].map(Number),
          scaleOrientation: [so.x, so.y, so.z, so.angle].map(Number),
          center: [c.x, c.y, c.z].map(Number),
        };
        var all = f.translation.concat(f.rotation, f.scale, f.scaleOrientation, f.center);
        return finiteAll(all) ? { type: type, frame: f } : null;
      } catch (e) {
        return null;
      }
    }

    // Bind one of viewpoints()' nodes through the PUBLIC browser service.
    function bindViewpoint(node) {
      var b = boundLayer();
      if (!b || !fn(browser, 'bindViewpoint')) return { ok: false, reason: 'camera-unavailable' };
      var list = viewpoints() || [];
      var known = false;
      for (var i = 0; i < list.length; i++) if (list[i].node === node) known = true;
      if (!known) return { ok: false, reason: 'viewpoint-not-in-scene' };
      try {
        browser.bindViewpoint(b.layer, node);
        return { ok: true };
      } catch (e) {
        return { ok: false, reason: 'bind-failed' };
      }
    }

    return Object.freeze({
      viewAll: viewAll, frameInput: frameInput, applyView: applyView, resetView: resetView, viewpoints: viewpoints, bindViewpoint: bindViewpoint,
      compatibility: compatibility, camera: camera, bind: bind, set: set, setField: setField, restore: restore, release: release, unbind: unbind, rendered: rendered, frame: frame,
      boundViewpoint: boundViewpoint, readCamera: readCamera, writeCamera: writeCamera, viewMatrix: viewMatrix, cameraState: cameraState,
      bounds: bounds, ancestor: ancestor, shapes: shapes,
    });
  }

  window.WrlXiteGizmoAdapter = Object.freeze({ createXiteGizmoAdapter: createXiteGizmoAdapter });
})();
