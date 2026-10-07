'use strict';
// WD2-C runtime QA for the Model workspace DOM binding (renderer/model-workspace.js)
// under a small DOM stub, with REAL beginner-property descriptors from the
// current parse (src/editor/first-object.js objectForItem). The real-Electron
// proof is qa/wd2-c-first-object/orchestrate.js.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const language = require('../../src/editor/language');
const firstObject = require('../../src/editor/first-object');
const sceneTree = require('../../src/vrml/scene-tree');
const sceneSelectionMod = require('../../src/editor/scene-selection');
const tx = require('../../src/vrml/document-transaction');
const templates = require('../../src/vrml/node-templates');

const H = '#VRML V2.0 utf8\n';

// --- a tiny DOM: enough selectors for tag / .class / [data-x="v"] -----------
function makeDom() {
  const doc = { activeElement: null };
  function matches(el, sel) {
    const m = /^([a-z]+)?((?:\.[\w-]+)*)(?:\[data-([\w-]+)="([^"]*)"\])?$/.exec(sel);
    if (!m) throw new Error(`stub selector unsupported: ${sel}`);
    if (m[1] && el.tagName !== m[1]) return false;
    for (const c of (m[2] || '').split('.').filter(Boolean)) if (!el.classList.contains(c)) return false;
    if (m[3] && el.dataset[m[3]] !== m[4]) return false;
    return true;
  }
  function all(root, sel, out) {
    for (const c of root.children) { if (matches(c, sel)) out.push(c); all(c, sel, out); }
    return out;
  }
  function makeEl(tagName) {
    const classes = new Set();
    const el = {
      tagName, children: [], attrs: {}, dataset: {}, listeners: {}, hidden: false, disabled: false, value: '', type: '',
      _text: null,
      get className() { return [...classes].join(' '); },
      set className(v) { classes.clear(); String(v).split(/\s+/).filter(Boolean).forEach((c) => classes.add(c)); },
      classList: { add: (c) => classes.add(c), remove: (c) => classes.delete(c), contains: (c) => classes.has(c),
        toggle: (c, on) => { if (on === undefined ? !classes.has(c) : on) classes.add(c); else classes.delete(c); } },
      get id() { return this.attrs.id || ''; },
      set id(v) { this.attrs.id = String(v); },
      set htmlFor(v) { this.attrs.for = String(v); },
      getAttribute(k) { return k in this.attrs ? this.attrs[k] : null; },
      setAttribute(k, v) { this.attrs[k] = String(v); },
      removeAttribute(k) { delete this.attrs[k]; },
      appendChild(c) { this.children.push(c); c.parentNode = this; return c; },
      removeChild(c) { this.children.splice(this.children.indexOf(c), 1); return c; },
      get firstChild() { return this.children[0] || null; },
      get textContent() { return this._text != null ? this._text : this.children.map((c) => c.textContent).join(''); },
      set textContent(v) { this.children = []; this._text = String(v); },
      addEventListener(n, fn) { (this.listeners[n] = this.listeners[n] || []).push(fn); },
      fire(n, extra) { const e = { type: n, preventDefault() { e.defaultPrevented = true; }, ...(extra || {}) }; (this.listeners[n] || []).forEach((f) => f(e)); return e; },
      click() { if (!this.disabled) this.fire('click'); },
      focus() { doc.activeElement = this; this.fire('focus'); },
      select() { this.selected = true; },
      querySelector(sel) { return all(this, sel, [])[0] || null; },
      querySelectorAll(sel) { return all(this, sel, []); },
    };
    return el;
  }
  doc.createElement = makeEl;
  return doc;
}

function loadView(doc) {
  const ctx = { document: doc, window: {}, console };
  vm.createContext(ctx);
  vm.runInContext(fs.readFileSync(path.join(__dirname, '..', '..', 'renderer', 'model-workspace.js'), 'utf8'), ctx);
  return ctx.window.WRLForgeModelWorkspace;
}

function analyse(text) {
  const a = language.analyze(text, { profile: 'generic' });
  return { text, tree: sceneTree.buildSceneTree(a.parseResult), session: tx.createParseSession(text, a.parseResult) };
}

function setup(text) {
  const doc = makeDom();
  const MW = loadView(doc);
  const ids = ['modelBtn', 'codeBtn', 'sourceBtn', 'addBox', 'addSphere', 'duplicate', 'remove', 'selected', 'status', 'props'];
  const els = Object.fromEntries(ids.map((k) => [k, doc.createElement(k === 'props' || k === 'status' || k === 'selected' ? 'div' : 'button')]));
  const selection = sceneSelectionMod.createSelectionController();
  const state = { mode: 'model', source: false, analysis: analyse(text), version: 1, calls: [], next: { status: 'ready' } };
  const view = MW.createModelWorkspace({
    els, selection,
    isOpen: () => true,
    getMode: () => state.mode,
    setMode: (m) => { state.mode = m; },
    isSourceOpen: () => state.source,
    setSourceOpen: (o) => { state.source = o; },
    describeSelection: (id) => { const it = sceneTree.itemById(state.analysis.tree, id); return it ? { label: it.nodeType || it.kind, isNode: it.kind === 'Node' } : null; },
    objectFor: (id) => firstObject.objectForItem({ session: state.analysis.session, tree: state.analysis.tree, currentText: state.analysis.text, itemId: id }),
    analysisToken: () => state.version,
    add: (p) => { state.calls.push(['add', p]); return { ok: true, message: `${p} created at origin.` }; },
    duplicate: (id) => { state.calls.push(['duplicate', id]); return { ok: false, message: 'Cannot duplicate this object because it contains a DEF name that would conflict.' }; },
    remove: (id) => { state.calls.push(['remove', id]); return { ok: true, message: 'Box deleted.' }; },
    applyProperty: (id, key, comps) => { state.calls.push(['apply', key, comps]); return state.next; },
    refusalText: (plan) => firstObject.refusalText(plan),
  });
  const selectTransform = () => selection.setSelection(state.analysis.tree.items.find((it) => it.nodeType === 'Transform').id);
  return { doc, els, view, state, selection, selectTransform, MW };
}

// Values produced inside the vm realm have that realm's Array prototype.
const plain = (v) => JSON.parse(JSON.stringify(v));
const row = (els, key) => els.props.querySelector(`.prop-row[data-prop="${key}"]`);

test('colour conversion: every 8-bit channel round-trips; bounded stable spelling', () => {
  const MW = loadView(makeDom());
  for (let v = 0; v < 256; v += 1) {
    const hex = `#${v.toString(16).padStart(2, '0')}0000`;
    const comps = MW.hexToColorComponents(hex);
    assert.ok(/^(0|1|0\.\d{1,3})$/.test(comps[0]), comps[0]);
    assert.equal(MW.colorComponentsToHex(comps), hex);
  }
  assert.deepEqual(plain(MW.hexToColorComponents('#ff8000')), ['1', '0.502', '0']);
  assert.equal(MW.hexToColorComponents('red'), null);
  assert.equal(MW.colorComponentsToHex(['0.8', '0.8', '0.8']), '#cccccc');
});

test('the Model bar: mode pressed states, Source toggle only in Model, Duplicate/Delete need a node selection', () => {
  const { els, view, state, selectTransform } = setup(`${H}${templates.simpleObjectTemplate('Box').text}\n`);
  view.refresh();
  assert.equal(els.modelBtn.getAttribute('aria-pressed'), 'true');
  assert.equal(els.codeBtn.getAttribute('aria-pressed'), 'false');
  assert.equal(els.sourceBtn.hidden, false);
  assert.equal(els.sourceBtn.textContent, 'Show Source');
  assert.equal(els.duplicate.disabled, true);
  assert.equal(els.selected.textContent, 'Nothing selected');
  selectTransform();
  assert.equal(els.duplicate.disabled, false);
  assert.equal(els.remove.disabled, false);
  assert.equal(els.selected.textContent, 'Selected: Transform');
  els.sourceBtn.click();
  assert.equal(state.source, true);
  assert.equal(els.sourceBtn.getAttribute('aria-pressed'), 'true');
  assert.equal(els.sourceBtn.textContent, 'Hide Source');
  els.codeBtn.click();
  assert.equal(state.mode, 'code');
  assert.equal(els.sourceBtn.hidden, true);
});

test('Add / Duplicate / Delete report their outcome as TEXT (errors flagged, not colour-only)', () => {
  const { els, view, state, selectTransform } = setup(`${H}${templates.simpleObjectTemplate('Box').text}\n`);
  view.refresh();
  els.addBox.click();
  assert.equal(els.status.textContent, 'Box created at origin.');
  assert.equal(els.status.classList.contains('err'), false);
  selectTransform();
  els.duplicate.click();
  assert.match(els.status.textContent, /^Cannot duplicate/);
  assert.equal(els.status.classList.contains('err'), true);
  assert.deepEqual(state.calls.map((c) => c[0]), ['add', 'duplicate']);
});

test('the Object panel: beginner labels, technical secondary text, labelled inputs, defaults shown', () => {
  const { els, view, selectTransform } = setup(`${H}${templates.simpleObjectTemplate('Box').text}\n`);
  view.refresh();
  assert.match(els.props.textContent, /Add a Box or Sphere/);
  selectTransform();
  const rows = els.props.querySelectorAll('.prop-row');
  assert.deepEqual(rows.map((r) => r.querySelector('.prop-label').textContent), ['Position', 'Rotation', 'Size', 'Color']);
  assert.deepEqual(rows.map((r) => r.querySelector('.prop-tech').textContent),
    ['translation · SFVec3f', 'rotation · SFRotation', 'size · SFVec3f', 'diffuseColor · SFColor']);
  const pos = row(els, 'position');
  assert.equal(pos.getAttribute('role'), 'group');
  assert.equal(pos.getAttribute('aria-labelledby'), 'prop-position-label');
  const inputs = pos.querySelectorAll('input.prop-num');
  assert.deepEqual(inputs.map((i) => i.value), ['0', '0', '0']);
  assert.equal(inputs[0].getAttribute('aria-labelledby'), 'prop-position-label prop-position-0-l');
  assert.equal(inputs[0].getAttribute('aria-describedby'), 'prop-position-tech prop-position-msg');
  assert.equal(pos.querySelector('.prop-state').textContent, '(default)');
  assert.equal(row(els, 'color').querySelector('input.prop-color').value, '#cccccc');
  assert.equal(row(els, 'size').querySelectorAll('label.prop-comp').map((l) => l.textContent).join(), 'Width (X),Height (Y),Depth (Z)');
});

test('Enter commits the typed components; Escape restores; a refusal marks the component and says why', () => {
  const { els, view, state, doc, selectTransform } = setup(`${H}${templates.simpleObjectTemplate('Box').text}\n`);
  view.refresh();
  selectTransform();
  let inputs = row(els, 'position').querySelectorAll('input.prop-num');
  inputs[0].value = '3';
  inputs[0].focus();
  const e = inputs[0].fire('keydown', { key: 'Enter' });
  assert.equal(e.defaultPrevented, true);
  assert.deepEqual(plain(state.calls.at(-1)), ['apply', 'position', ['3', '0', '0']]);
  // After a ready apply the panel re-renders and says "Applied." with focus back.
  inputs = row(els, 'position').querySelectorAll('input.prop-num');
  assert.equal(row(els, 'position').querySelector('.prop-msg').textContent, 'Applied.');
  assert.equal(doc.activeElement, inputs[0]);
  // Refusal on component 1.
  state.next = { status: 'refused', reason: 'input-not-a-number', componentIndex: 1, message: '"abc" is not a number.' };
  inputs[1].value = 'abc';
  inputs[1].focus();
  inputs[1].fire('keydown', { key: 'Enter' });
  assert.equal(inputs[1].getAttribute('aria-invalid'), 'true');
  assert.equal(row(els, 'position').querySelector('.prop-msg').textContent, 'Invalid: "abc" is not a number.');
  assert.equal(inputs[1].value, 'abc', 'typed text kept');
  inputs[1].fire('keydown', { key: 'Escape' });
  assert.equal(inputs[1].value, '0');
  assert.equal(inputs[1].getAttribute('aria-invalid'), null);
});

test('the colour control commits on change only (never on every input while dragging)', () => {
  const { els, view, state, selectTransform } = setup(`${H}${templates.simpleObjectTemplate('Box').text}\n`);
  view.refresh();
  selectTransform();
  const pick = row(els, 'color').querySelector('input.prop-color');
  pick.value = '#ff0000';
  pick.fire('input');
  assert.equal(state.calls.length, 0);
  pick.fire('change');
  assert.deepEqual(plain(state.calls.at(-1)), ['apply', 'color', ['1', '0', '0']]);
});

test('read-only properties say why in text; non-objects point to the Inspector', () => {
  const { els, view, selectTransform } = setup(`${H}Transform { children [ Shape { geometry Box { } } ] }\n`);
  view.refresh();
  selectTransform();
  const c = row(els, 'color');
  assert.equal(c.querySelectorAll('input.prop-num').length + (c.querySelector('input.prop-color') ? 1 : 0), 0);
  assert.equal(c.querySelector('.prop-msg').textContent, 'Read-only: This object has no Material, so its color cannot be changed here.');
  const other = setup(`${H}Group { }\n`);
  other.view.refresh();
  other.selection.setSelection(other.state.analysis.tree.items.find((it) => it.nodeType === 'Group').id);
  assert.match(other.els.props.textContent, /Group is not a simple object\. Its fields are in the Inspector below\./);
});

test('the panel does not re-render (and wipe typed text) unless the selection or the analysis changed', () => {
  const { els, view, state, selectTransform } = setup(`${H}${templates.simpleObjectTemplate('Sphere').text}\n`);
  view.refresh();
  selectTransform();
  const input = row(els, 'radius').querySelector('input.prop-num');
  input.value = '7';
  view.refresh();
  assert.equal(row(els, 'radius').querySelector('input.prop-num'), input);
  assert.equal(input.value, '7');
  state.version += 1;
  view.refresh();
  assert.notEqual(row(els, 'radius').querySelector('input.prop-num'), input);
});

test('focusing a numeric value selects it (typing replaces "0" instead of making "03")', () => {
  const { els, view, selectTransform } = setup(`${H}${templates.simpleObjectTemplate('Box').text}\n`);
  view.refresh();
  selectTransform();
  const x = row(els, 'position').querySelectorAll('input.prop-num')[0];
  x.focus();
  assert.equal(x.selected, true);
  const up = x.fire('mouseup');
  assert.equal(up.defaultPrevented, true, 'the click\'s mouseup does not collapse the selection');
  assert.equal(x.fire('mouseup').defaultPrevented, undefined, 'later clicks place the caret normally');
});
