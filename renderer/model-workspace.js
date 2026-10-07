'use strict';
// Model workspace (Phase WD2-C "First Object").
//
// A plain DOM binding, like scene-inspector.js. It renders:
//
//   * the Model bar -- the Model/Code workspace switch, the always-visible Add
//     Box / Add Sphere buttons, Duplicate, Delete, a "Selected: ..." line, the
//     Show/Hide Source toggle and a textual status line;
//   * the "Object" panel -- the beginner properties of the selected simple
//     object (Position, Rotation, Size|Radius, Color) with the VRML field name
//     and type as secondary text.
//
// It decides NOTHING about the document. Every value it shows comes from
// `deps.objectFor(itemId)` (the pure simple-object facade over the current
// parse); every action goes through `deps.add / duplicate / remove /
// applyProperty`, which plan and verify an exact source edit and dispatch it as
// one CodeMirror transaction. It never computes an offset, never builds VRML
// text, keeps no value of its own and owns no selection (it reads the shared
// selection controller). Numbers are committed on Enter / Apply, a colour on
// the picker's `change` (not on every `input` while dragging).

(function () {
  // Colour conversion for the native picker. 8-bit channels are written with at
  // most three decimals -- 1/255 > 0.001, so every #rrggbb round-trips -- and
  // spelled by Number->String, so 1 is "1", 0 is "0", 128/255 is "0.502".
  function hexToColorComponents(hex) {
    const m = /^#([0-9a-fA-F]{2})([0-9a-fA-F]{2})([0-9a-fA-F]{2})$/.exec(String(hex || ''));
    if (!m) return null;
    return [m[1], m[2], m[3]].map((h) => String(Number((parseInt(h, 16) / 255).toFixed(3))));
  }
  function colorComponentsToHex(values) {
    if (!Array.isArray(values) || values.length !== 3) return '#cccccc';
    return '#' + values.map((v) => {
      const n = Number(v);
      const c = Number.isFinite(n) ? Math.round(Math.min(1, Math.max(0, n)) * 255) : 204;
      return c.toString(16).padStart(2, '0');
    }).join('');
  }

  function clear(node) { while (node.firstChild) node.removeChild(node.firstChild); }
  function make(tag, cls, text) {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  }

  function createModelWorkspace(deps) {
    const els = deps.els;
    const selection = deps.selection;
    let renderedKey = null;
    let pendingFocus = null; // { key, component, message }

    function setStatus(text, isError) {
      if (!els.status) return;
      els.status.textContent = text || '';
      els.status.classList.toggle('err', !!isError);
    }

    // --- the Model bar --------------------------------------------------------
    function paintBar() {
      const mode = deps.getMode();
      els.modelBtn.setAttribute('aria-pressed', String(mode === 'model'));
      els.codeBtn.setAttribute('aria-pressed', String(mode === 'code'));
      const open = deps.isSourceOpen();
      els.sourceBtn.hidden = mode !== 'model';
      els.sourceBtn.setAttribute('aria-pressed', String(open));
      els.sourceBtn.textContent = open ? 'Hide Source' : 'Show Source';
      const docOpen = deps.isOpen();
      els.addBox.disabled = !docOpen;
      els.addSphere.disabled = !docOpen;
      const id = selection.getSelection();
      const info = id ? deps.describeSelection(id) : null;
      const node = !!(info && info.isNode);
      els.duplicate.disabled = !node;
      els.remove.disabled = !node;
      els.selected.textContent = info ? `Selected: ${info.label}` : 'Nothing selected';
    }

    function wire() {
      els.modelBtn.addEventListener('click', () => { deps.setMode('model'); refresh(); });
      els.codeBtn.addEventListener('click', () => { deps.setMode('code'); refresh(); });
      els.sourceBtn.addEventListener('click', () => { deps.setSourceOpen(!deps.isSourceOpen()); refresh(); });
      const act = (fn) => () => {
        const res = fn();
        if (res && res.message) setStatus(res.message, !res.ok);
      };
      els.addBox.addEventListener('click', act(() => deps.add('Box')));
      els.addSphere.addEventListener('click', act(() => deps.add('Sphere')));
      els.duplicate.addEventListener('click', act(() => deps.duplicate(selection.getSelection())));
      els.remove.addEventListener('click', act(() => deps.remove(selection.getSelection())));
      selection.subscribe(() => refresh());
    }

    // --- the Object panel -----------------------------------------------------
    function inputsOf(row) { return Array.from(row.querySelectorAll('input.prop-num')); }

    function commit(itemId, prop, row) {
      const msg = row.querySelector('.prop-msg');
      let components;
      let focusIndex = 0;
      if (prop.kind === 'color') {
        components = hexToColorComponents(row.querySelector('input.prop-color').value);
      } else {
        const inputs = inputsOf(row);
        inputs.forEach((i) => i.removeAttribute('aria-invalid'));
        components = inputs.map((i) => i.value);
        const a = document.activeElement;
        focusIndex = Math.max(0, inputs.indexOf(a));
      }
      const res = deps.applyProperty(itemId, prop.key, components);
      if (res.status === 'ready') {
        setStatus('', false); // the panel's own "Applied." is the feedback now
        // The analysis that follows re-renders the panel; restore focus + say so.
        pendingFocus = { key: prop.key, component: focusIndex, message: 'Applied.' };
        renderedKey = null;
        refresh();
        return;
      }
      if (res.status === 'unchanged') { msg.textContent = 'No change.'; return; }
      msg.textContent = `Invalid: ${deps.refusalText(res)}`;
      if (prop.kind !== 'color' && Number.isInteger(res.componentIndex)) {
        const bad = inputsOf(row)[res.componentIndex];
        if (bad) { bad.setAttribute('aria-invalid', 'true'); bad.focus(); }
      }
    }

    function restore(prop, row) {
      inputsOf(row).forEach((i, k) => { i.value = prop.values[k]; i.removeAttribute('aria-invalid'); });
      row.querySelector('.prop-msg').textContent = '';
    }

    function propRow(itemId, prop) {
      const base = `prop-${prop.key}`;
      const row = make('div', 'prop-row');
      row.dataset.prop = prop.key;
      row.setAttribute('role', 'group');
      row.setAttribute('aria-labelledby', `${base}-label`);
      const head = make('div', 'prop-head');
      const label = make('span', 'prop-label', prop.label);
      label.id = `${base}-label`;
      head.appendChild(label);
      const tech = make('span', 'prop-tech', `${prop.field} · ${prop.type}`);
      tech.id = `${base}-tech`;
      head.appendChild(tech);
      head.appendChild(make('span', 'prop-state', prop.present ? '' : '(default)'));
      row.appendChild(head);
      const body = make('div', 'prop-inputs');
      const msg = make('div', 'prop-msg');
      msg.id = `${base}-msg`;
      msg.setAttribute('role', 'status');
      msg.setAttribute('aria-live', 'polite');
      if (!prop.editable) {
        body.appendChild(make('span', 'prop-value', prop.values.join(' ') || '—'));
        msg.textContent = `Read-only: ${deps.refusalText({ reason: prop.reason })}`;
      } else if (prop.kind === 'color') {
        const pick = document.createElement('input');
        pick.type = 'color';
        pick.className = 'prop-color';
        pick.id = `${base}-picker`;
        pick.value = colorComponentsToHex(prop.values);
        pick.setAttribute('aria-labelledby', `${base}-label`);
        pick.setAttribute('aria-describedby', `${base}-tech ${base}-msg`);
        pick.addEventListener('change', () => commit(itemId, prop, row));
        body.appendChild(pick);
        body.appendChild(make('span', 'prop-value', prop.values.join(' ')));
      } else {
        prop.values.forEach((v, k) => {
          const id = `${base}-${k}`;
          const lab = make('label', 'prop-comp', prop.componentLabels[k]);
          lab.htmlFor = id;
          lab.id = `${id}-l`;
          const input = document.createElement('input');
          input.type = 'text';
          input.className = 'prop-num';
          input.id = id;
          input.value = v;
          input.dataset.component = String(k);
          input.setAttribute('inputmode', 'decimal');
          input.setAttribute('spellcheck', 'false');
          input.setAttribute('aria-labelledby', `${base}-label ${id}-l`);
          input.setAttribute('aria-describedby', `${base}-tech ${base}-msg`);
          // Focusing a value selects it, so clicking "0" and typing "3" gives
          // 3, not 03. The mouseup that follows a click-to-focus would
          // otherwise collapse the selection to a caret.
          let justFocused = false;
          input.addEventListener('focus', () => { if (typeof input.select === 'function') input.select(); justFocused = true; });
          input.addEventListener('mouseup', (e) => { if (justFocused) e.preventDefault(); justFocused = false; });
          input.addEventListener('blur', () => { justFocused = false; });
          input.addEventListener('keydown', (e) => {
            justFocused = false;
            if (e.key === 'Enter') { e.preventDefault(); commit(itemId, prop, row); }
            else if (e.key === 'Escape') { e.preventDefault(); restore(prop, row); }
          });
          const cell = make('span', 'prop-field');
          cell.appendChild(lab);
          cell.appendChild(input);
          body.appendChild(cell);
        });
        const apply = make('button', 'prop-apply', 'Apply');
        apply.type = 'button';
        apply.setAttribute('aria-label', `Apply ${prop.label}`);
        apply.addEventListener('click', () => commit(itemId, prop, row));
        body.appendChild(apply);
      }
      row.appendChild(body);
      if (prop.key === 'rotation' && prop.editable) {
        row.appendChild(make('div', 'prop-hint', 'Turns the object around the axis. A quarter turn is 1.5708.'));
      }
      row.appendChild(msg);
      return row;
    }

    function paintPanel() {
      const id = selection.getSelection();
      const key = `${id}|${deps.analysisToken()}`;
      if (key === renderedKey) return;
      renderedKey = key;
      const root = els.props;
      clear(root);
      const obj = id ? deps.objectFor(id) : null;
      if (!obj) {
        const info = id ? deps.describeSelection(id) : null;
        root.appendChild(make('div', 'empty-note', info
          ? `${info.label} is not a simple object. Its fields are in the Inspector below.`
          : 'Add a Box or Sphere, or select one in the Scene tree.'));
        return;
      }
      const title = make('div', 'prop-title', obj.primitive);
      title.id = 'objectTitle';
      root.appendChild(title);
      for (const prop of obj.properties) root.appendChild(propRow(id, prop));
      if (pendingFocus) {
        const row = root.querySelector(`.prop-row[data-prop="${pendingFocus.key}"]`);
        if (row) {
          row.querySelector('.prop-msg').textContent = pendingFocus.message;
          const target = row.querySelector('input.prop-color') || inputsOf(row)[pendingFocus.component];
          if (target) target.focus();
        }
        pendingFocus = null;
      }
    }

    function refresh() {
      paintBar();
      paintPanel();
    }

    wire();
    return { refresh, setStatus };
  }

  const api = { createModelWorkspace, hexToColorComponents, colorComponentsToHex };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else window.WRLForgeModelWorkspace = api;
})();
