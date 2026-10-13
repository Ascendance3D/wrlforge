'use strict';
// Scene-tree view (Phase WD2-A). Read-only tree of scene items, with ARIA
// tree semantics, keyboard navigation, and a single selection authority the
// inspector subscribes to. No editing -- the lane contract.
//
// Architecture: the view is a thin DOM binding over `WRLForgeSceneTree`
// (this file's window global). It owns a `setSceneTree(tree, opts)` entry,
// a `setSelection(id)` entry from the selection controller, and a click
// handler that calls into the SAME selection controller. There is no second
// state -- the DOM reflects the read model and the controller; nothing else.
//
// Dual use: this file is loaded by <script defer> from editor.html (a plain
// browser script), so it also exposes a window global. The module is DOM-
// only and not Node-importable -- that matches the existing renderer scripts
// in the workspace and avoids needing a bundler on this page (per AGENTS.md).

(function () {
  const KIND = {
    DOCUMENT: 'Document',
    NODE: 'Node',
    USE: 'Use',
    PROTO: 'Proto',
    EXTERNPROTO: 'ExternProto',
    ROUTE: 'Route',
  };
  const USE_TARGET = { RESOLVED: 'resolved', UNRESOLVED: 'unresolved' };

  // Label for a single scene item -- the same source of truth the inspector
  // uses to derive its title, so a tree row and an inspector header agree.
  function labelFor(item) {
    switch (item.kind) {
      case KIND.NODE: {
        const def = item.def ? `DEF ${item.def} ` : '';
        const inst = item.protoInstance ? ` (PROTO ${item.nodeType})` : '';
        return `${def}${item.nodeType}${inst}`;
      }
      case KIND.USE: {
        const status = item.useTargetStatus === USE_TARGET.RESOLVED ? '→' : '⚠';
        return `${status} USE ${item.useName || '?'}`;
      }
      case KIND.PROTO:
        return `PROTO ${item.protoName || '?'}`;
      case KIND.EXTERNPROTO:
        return `EXTERNPROTO ${item.externprotoName || '?'}`;
      case KIND.ROUTE:
        return `ROUTE ${item.routeFromNode || '?'}.${item.routeFromEvent || '?'} → ${item.routeToNode || '?'}.${item.routeToEvent || '?'}`;
      case KIND.DOCUMENT:
        return 'Document';
      default:
        return item.kind;
    }
  }

  // Build the tree's DOM once per `setSceneTree`. We render a flat list of
  // <div role="treeitem"> elements with explicit aria-level so screen
  // readers can announce the hierarchy. `tree.items` is depth-first,
  // document-ordered: walking it (skipping the Document root, already pushed)
  // produces the visual row order. Each item carries `depth` from the
  // read-model walk, so the aria-level is just `depth + 1`. CSS hides any
  // indentation shape we don't want.
  //
  // Public as `buildSceneTreeDom` (see the api object at the bottom) -- a
  // Node-side test loads this module under `vm.runInContext` with a minimal
  // `document` stub and calls `buildSceneTreeDom(tree)` to assert that
  // nested items render (F2). The function is the SAME one the view calls.
  function buildSceneTreeDom(tree, opts) {
    const dom = [];
    // The Document row is the root of the tree.
    dom.push(makeRow(tree.root, opts));
    // Every non-root item is a row in document order. `tree.items` is frozen
    // (so we cannot splice into it) and depth-first (parent precedes
    // children), which is exactly the visual order a flat tree row list
    // needs. The mapping from source range to item is owned by the read
    // model; the view only reflects it.
    for (const item of tree.items) {
      if (item === tree.root) continue;
      dom.push(makeRow(item, opts));
    }
    return dom;
  }

  function makeRow(item, opts) {
    const row = document.createElement('div');
    row.className = 'scene-row kind-' + item.kind.toLowerCase();
    row.dataset.id = item.id;
    row.setAttribute('role', 'treeitem');
    row.setAttribute('aria-level', String(item.depth + 1));
    row.setAttribute('aria-selected', 'false');
    row.tabIndex = -1; // only the active row is tabbable

    if (item.childIds && item.childIds.length) {
      // A non-leaf row declares its expand state. WD2-A does not implement
      // expand/collapse, so the structural value is always "true" (the
      // children are always rendered); the attribute is informational, not
      // interactive. The renderer never hides children on toggle.
      row.setAttribute('aria-expanded', 'true');
    }
    // Leaves carry NO aria-expanded: the WAI-ARIA tree pattern only
    // declares the attribute for items that participate in the
    // expand/collapse interaction. A leaf is not expandable; setting the
    // attribute to "false" would be a false claim about UI state.

    const label = document.createElement('span');
    label.className = 'scene-label';
    // WD2-C: a recognised simple object reads as "Box" / "Sphere" first, with
    // the real node label kept visible beside it. DISPLAY ONLY -- derived from
    // the current parse by the renderer, never written to the source and never
    // used for identity.
    const friendly = opts && typeof opts.displayLabelFor === 'function' ? opts.displayLabelFor(item) : null;
    label.textContent = friendly || labelFor(item);
    row.appendChild(label);
    if (friendly) {
      const type = document.createElement('span');
      type.className = 'scene-type';
      type.textContent = labelFor(item);
      row.appendChild(type);
    }

    // Optional: a small inline status indicator for USE / ROUTE
    // resolution, derived from the read model only.
    if (item.kind === KIND.USE && item.useTargetStatus === USE_TARGET.UNRESOLVED) {
      const tag = document.createElement('span');
      tag.className = 'scene-tag scene-tag-warn';
      tag.textContent = 'unresolved';
      row.appendChild(tag);
    } else if (item.kind === KIND.ROUTE && (!item.routeResolvedFrom || !item.routeResolvedTo)) {
      const tag = document.createElement('span');
      tag.className = 'scene-tag scene-tag-warn';
      tag.textContent = 'unresolved';
      row.appendChild(tag);
    }

    // Indentation by depth. Stays in CSS-friendly px so the chrome zoom
    // (--wrl-ui-scale rem layer) does not collapse it.
    row.style.paddingLeft = (8 + item.depth * 14) + 'px';

    // Click selects.
    row.addEventListener('click', () => {
      opts.onSelect(item.id);
    });

    return row;
  }

  function clearChildren(node) { while (node.firstChild) node.removeChild(node.firstChild); }

  function createSceneTreeView(rootEl, selection, deps) {
    let currentTree = null;
    let currentSelection = null;
    let rowById = new Map();

    function render() {
      clearChildren(rootEl);
      rowById = new Map();
      if (!currentTree) {
        rootEl.setAttribute('aria-label', 'Scene tree');
        return;
      }
      rootEl.setAttribute('aria-label', 'Scene tree of document');
      const dom = buildSceneTreeDom(currentTree, {
        onSelect: (id) => selection.setSelection(id),
        displayLabelFor: deps && typeof deps.displayLabelFor === 'function' ? deps.displayLabelFor : null,
      });
      for (const r of dom) {
        rootEl.appendChild(r);
        rowById.set(r.dataset.id, r);
      }
      applySelection(currentSelection);
      // The first visible row is the roving tabIndex target.
      const first = rootEl.querySelector('.scene-row');
      if (first) first.tabIndex = 0;
    }

    function applySelection(id) {
      const prev = rootEl.querySelector('.scene-row[aria-selected="true"]');
      if (prev) {
        prev.setAttribute('aria-selected', 'false');
        prev.tabIndex = -1;
      }
      if (id == null) return;
      const row = rowById.get(id);
      if (!row) return; // selection may refer to an item no longer present
      row.setAttribute('aria-selected', 'true');
      row.tabIndex = 0;
    }

    // Roving-tabindex keyboard navigation across the flat row list (the
    // tree's logical children list). The view flattens children of every
    // node into the same row set so a single up/down traversal is honest
    // about which rows are reachable in document order.
    function visibleRows() {
      return Array.from(rootEl.querySelectorAll('.scene-row'));
    }

    function focusRow(idx) {
      const rows = visibleRows();
      if (!rows.length) return;
      const wrapped = ((idx % rows.length) + rows.length) % rows.length;
      const r = rows[wrapped];
      r.focus();
    }

    function focusRowById(id) {
      const row = rowById.get(id);
      if (row) row.focus();
    }

    function onKeydown(e) {
      const rows = visibleRows();
      if (!rows.length) return;
      const active = document.activeElement;
      const idx = rows.indexOf(active);
      if (idx < 0) return;
      let handled = true;
      switch (e.key) {
        case 'ArrowDown': focusRow(idx + 1); break;
        case 'ArrowUp':   focusRow(idx - 1); break;
        case 'Home':      focusRow(0); break;
        case 'End':       focusRow(rows.length - 1); break;
        case 'Enter':
        case ' ':
          if (active && active.dataset && active.dataset.id) selection.setSelection(active.dataset.id);
          else handled = false;
          break;
        default: handled = false;
      }
      if (handled) e.preventDefault();
    }

    rootEl.addEventListener('keydown', onKeydown);

    selection.subscribe((id) => {
      currentSelection = id;
      applySelection(id);
    });

    return {
      setSceneTree(tree) {
        // A re-render that may move selection. If the previously selected id
        // no longer exists, the selection controller still holds it; a future
        // setSelection(...) from elsewhere will clear it. We do NOT clear it
        // here -- the controller is the single authority.
        currentTree = tree || null;
        render();
      },
      getSelectedId() { return currentSelection; },
      focusById(id) { focusRowById(id); },
      rowForId(id) { return rowById.get(id) || null; },
      // Helper for editor.js navigation: given an offset, find the row that
      // owns it and focus + select it.
      focusOffset(offset) {
        if (!deps || typeof deps.itemContainingOffset !== 'function') return;
        if (!currentTree) return;
        const item = deps.itemContainingOffset(currentTree, offset);
        if (!item) return;
        selection.setSelection(item.id);
        focusRowById(item.id);
      },
    };
  }

  const api = {
    KIND,
    USE_TARGET,
    createSceneTreeView,
    labelFor,
    // Test entry: build the DOM row list without mounting. The renderer's
    // own scene-tree view runs `buildDom` and mounts the result; a Node-side
    // test loads this module under `vm.runInContext` with a minimal `document`
    // stub and calls `buildSceneTreeDom(tree)` directly to assert that nested
    // items render (F2). The function is the SAME one the view calls; no
    // second implementation, no second authority.
    buildSceneTreeDom,
  };

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  } else {
    window.WRLForgeSceneTree = api;
  }
})();