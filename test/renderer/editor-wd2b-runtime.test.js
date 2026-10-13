'use strict';
// WD2-B runtime QA for the Inspector DOM binding (renderer/scene-inspector.js)
// under a DOM stub, with REAL field descriptors and a REAL Apply path:
// deps.applyField runs src/editor/inspector-edit.js prepareInspectorApply, the
// verified new text is analysed, the selection is re-anchored through WD1.4,
// and the Inspector re-renders -- the same sequence renderer/editor.js runs.
// The real-Electron proof is qa/wd2-b-typed-inspector/orchestrate.js.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const vrml = require('../../src/vrml');
const presentationMod = require('../../src/vrml/presentation');
const messagesMod = require('../../src/vrml/messages');
const sceneTreeMod = require('../../src/vrml/scene-tree');
const sceneSelectionMod = require('../../src/editor/scene-selection');
const inspectorEdit = require('../../src/editor/inspector-edit');
const tx = require('../../src/vrml/document-transaction');

const H = '#VRML V2.0 utf8\n';

function makeBrowserContext() {
  const doc = { activeElement: null };
  function makeEl(tag) {
    const classes = new Set();
    const el = {
      tag,
      children: [],
      attrs: {},
      style: {},
      dataset: {},
      _listeners: {},
      tabIndex: -1,
      value: '',
      checked: false,
      get className() { return [...classes].join(' '); },
      set className(v) { classes.clear(); for (const c of String(v).split(/\s+/).filter(Boolean)) classes.add(c); },
      get classList() {
        return { add: (c) => classes.add(c), remove: (c) => classes.delete(c), contains: (c) => classes.has(c), toggle() {} };
      },
      get id() { return this.attrs.id || ''; },
      set id(v) { this.attrs.id = String(v); },
      getAttribute(k) { return this.attrs[k] == null ? null : this.attrs[k]; },
      setAttribute(k, v) { this.attrs[k] = String(v); },
      removeAttribute(k) { delete this.attrs[k]; },
      appendChild(child) { this.children.push(child); child.parentNode = this; return child; },
      removeChild(child) {
        const i = this.children.indexOf(child);
        if (i >= 0) this.children.splice(i, 1);
        return child;
      },
      addEventListener(name, fn) { (this._listeners[name] = this._listeners[name] || []).push(fn); },
      get firstChild() { return this.children[0] || null; },
      get textContent() {
        if (this._textContent != null) return this._textContent;
        return this.children.map((c) => c.textContent || '').join('');
      },
      set textContent(v) { this._textContent = String(v); this.children = []; },
      focus() { doc.activeElement = this; },
    };
    return el;
  }
  const sandbox = {
    console: { log() {}, warn() {}, error() {} },
    setTimeout: () => 0,
    clearTimeout() {},
    document: Object.assign(doc, {
      createElement: (tag) => makeEl(tag),
      documentElement: { style: { setProperty() {} } },
    }),
  };
  sandbox.window = sandbox;
  vm.createContext(sandbox);
  const src = fs.readFileSync(path.join(__dirname, '..', '..', 'renderer', 'scene-inspector.js'), 'utf8');
  vm.runInContext(src, sandbox, { filename: 'renderer/scene-inspector.js' });
  return sandbox;
}

function all(el, pred, out = []) {
  if (pred(el)) out.push(el);
  for (const c of el.children || []) all(c, pred, out);
  return out;
}
const hasClass = (cls) => (e) => (e.className || '').split(' ').includes(cls);
const byId = (root, id) => all(root, (e) => e.attrs && e.attrs.id === id)[0] || null;
function fire(el, type, extra = {}) {
  // Bubble from the target to its ancestors, like a real keydown/click.
  const e = { type, target: el, defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, ...extra };
  for (let n = el; n; n = n.parentNode) for (const fn of (n._listeners[type] || [])) fn(e);
  return e;
}

// One Inspector bound exactly as renderer/editor.js binds it, over a live
// "document" whose Apply goes through the production planner.
function mount(text) {
  const win = makeBrowserContext();
  const state = { text };
  const analyse = (t) => {
    const parseResult = vrml.parse(t);
    return { text: t, tree: sceneTreeMod.buildSceneTree(parseResult), session: tx.createParseSession(t, parseResult) };
  };
  state.analysis = analyse(text);
  const selection = sceneSelectionMod.createSelectionController();
  const root = win.document.createElement('div');
  const applyCalls = [];
  const inspector = win.WRLForgeInspector.createInspector(root, selection, {
    presentation: presentationMod,
    messages: messagesMod,
    itemById: sceneTreeMod.itemById,
    itemContainingOffset: sceneTreeMod.itemContainingOffset,
    findingsForDocument: () => [],
    fieldsFor: (item) => inspectorEdit.fieldsForItem({
      session: state.analysis.session, tree: state.analysis.tree, currentText: state.text, itemId: item.id,
    }),
    applyField: (item, field, components) => {
      applyCalls.push({ field: field.name, components });
      const plan = inspectorEdit.prepareInspectorApply({
        session: state.analysis.session, tree: state.analysis.tree, currentText: state.text,
        itemId: item.id, fieldIndex: field.index, fieldName: field.name, components,
      });
      if (plan.status !== 'ready') return plan;
      // "Dispatch", then the synchronous re-analysis + re-anchor editor.js does.
      state.text = plan.newText;
      const previous = state.analysis;
      const next = analyse(plan.newText);
      const decision = inspectorEdit.reanchorSelection({
        previous, next, selectedId: selection.getSelection(), chain: null,
        pendingApply: { oldText: plan.oldText, newText: plan.newText, receipt: plan.receipt },
      });
      state.analysis = next;
      inspector.setSceneTree(next.tree);
      if (decision.id && decision.id !== selection.getSelection()) selection.setSelection(decision.id);
      return plan;
    },
  });
  inspector.setSceneTree(state.analysis.tree);
  const select = (nodeType) => {
    const item = state.analysis.tree.items.find((it) => it.kind === 'Node' && it.nodeType === nodeType);
    selection.setSelection(item.id);
    return item;
  };
  const row = (name) => all(root, hasClass('field-row')).find((r) => r.dataset.fieldName === name);
  const inputs = (name) => all(row(name), (e) => e.tag === 'input');
  return { win, root, state, selection, inspector, select, row, inputs, applyCalls };
}

const PRIMARY = H + 'Transform {\n  translation 0 0 0\n  children [ Shape { geometry Box { size 2 2 2 } } ]\n}\n';

test('runtime: a selected Transform shows a Fields section with typed, labelled controls', () => {
  const m = mount(PRIMARY);
  m.select('Transform');
  const headings = all(m.root, hasClass('inspector-heading')).map((h) => h.textContent);
  assert.deepEqual(headings, ['Fields', 'Diagnostics']);
  const tr = m.row('translation');
  assert.ok(tr, 'translation row rendered');
  assert.equal(tr.getAttribute('role'), 'group');
  assert.ok(all(tr, hasClass('field-type'))[0].textContent === 'SFVec3f');
  assert.ok(all(tr, hasClass('field-state'))[0].textContent === 'editable');
  const ins = m.inputs('translation');
  assert.equal(ins.length, 3);
  assert.deepEqual(ins.map((i) => i.value), ['0', '0', '0']);
  const labels = all(tr, hasClass('field-comp-label')).map((l) => l.textContent);
  assert.deepEqual(labels, ['X', 'Y', 'Z']);
  for (const input of ins) {
    // Every id the accessible name / description references exists.
    for (const ref of input.getAttribute('aria-labelledby').split(' ')) assert.ok(byId(tr, ref), `labelledby ${ref}`);
    assert.ok(byId(tr, input.getAttribute('aria-describedby')), 'describedby -> message element');
    const lab = all(tr, (e) => e.tag === 'label' && e.getAttribute('for') === input.id);
    assert.equal(lab.length, 1, 'a <label for> is associated with the input');
  }
  // children (MFNode) is shown, readable, and read-only with a reason.
  const ch = m.row('children');
  assert.ok(all(ch, hasClass('field-reason'))[0].textContent.startsWith('Read-only:'));
  assert.equal(all(ch, (e) => e.tag === 'input').length, 0);
});

test('runtime: Enter commits; the source changes; selection and focus survive; "Applied." is announced', () => {
  const m = mount(PRIMARY);
  m.select('Transform');
  const x = m.inputs('translation')[0];
  x.value = '3';
  const ev = fire(x, 'keydown', { key: 'Enter' });
  assert.equal(ev.defaultPrevented, true);
  assert.equal(JSON.stringify(m.applyCalls), JSON.stringify([{ field: 'translation', components: ['3', '0', '0'] }]));
  assert.equal(m.state.text, PRIMARY.replace('translation 0 0 0', 'translation 3 0 0'));
  const selected = sceneTreeMod.itemById(m.state.analysis.tree, m.selection.getSelection());
  assert.equal(selected.nodeType, 'Transform');
  const fresh = m.inputs('translation');
  assert.deepEqual(fresh.map((i) => i.value), ['3', '0', '0']);
  assert.equal(m.win.document.activeElement, fresh[0], 'focus returns to the edited component');
  assert.equal(all(m.row('translation'), hasClass('field-msg'))[0].textContent, 'Applied.');
});

test('runtime: invalid input never reaches the document; message + aria-invalid + focus + text kept', () => {
  const m = mount(PRIMARY);
  m.select('Transform');
  const [x, y] = m.inputs('translation');
  y.value = 'abc';
  fire(x, 'keydown', { key: 'Enter' });
  assert.equal(m.state.text, PRIMARY, 'source unchanged');
  assert.equal(y.getAttribute('aria-invalid'), 'true');
  assert.equal(x.getAttribute('aria-invalid'), null);
  const msg = byId(m.row('translation'), y.getAttribute('aria-describedby'));
  assert.ok(msg.textContent.startsWith('Invalid: '), 'text, not colour, carries the state');
  assert.equal(m.win.document.activeElement, y, 'focus moves to the invalid component');
  assert.equal(y.value, 'abc', 'the typed text is preserved');
  // Escape restores the document value and clears the invalid state.
  fire(y, 'keydown', { key: 'Escape' });
  assert.equal(y.value, '0');
  assert.equal(y.getAttribute('aria-invalid'), null);
  assert.equal(msg.textContent, 'Restored the document value.');
  assert.equal(m.win.document.activeElement, y, 'focus stays in the field');
  assert.equal(m.state.text, PRIMARY);
});

test('runtime: Cancel restores; an unchanged Apply says "No change."; Apply button commits', () => {
  const m = mount(H + 'Material { diffuseColor 0.8 0.8 0.8 }\n');
  m.select('Material');
  const ins = m.inputs('diffuseColor');
  ins[0].value = '9';
  const cancel = all(m.row('diffuseColor'), hasClass('field-cancel'))[0];
  fire(cancel, 'click');
  assert.equal(ins[0].value, '0.8');
  const apply = all(m.row('diffuseColor'), hasClass('field-apply'))[0];
  fire(apply, 'click');
  assert.equal(all(m.row('diffuseColor'), hasClass('field-msg'))[0].textContent, 'No change.');
  ins[0].value = '1.5';
  fire(apply, 'click');
  assert.ok(all(m.row('diffuseColor'), hasClass('field-msg'))[0].textContent.includes('≤ 1'), 'schema bound explained');
  ins[0].value = '1';
  ins[2].value = '0';
  fire(apply, 'click');
  assert.equal(m.state.text, H + 'Material { diffuseColor 1 0.8 0 }\n');
});

test('runtime: SFBool checkbox + SFString text controls commit through the same path', () => {
  const m = mount(H + 'DirectionalLight { on TRUE }\nWorldInfo { title "Old" }\n');
  m.select('DirectionalLight');
  const [cb] = m.inputs('on');
  assert.equal(cb.type, 'checkbox');
  assert.equal(cb.checked, true);
  cb.checked = false;
  fire(cb, 'change');
  fire(cb, 'keydown', { key: 'Enter' });
  assert.ok(m.state.text.includes('DirectionalLight { on FALSE }'));
  m.select('WorldInfo');
  const [t] = m.inputs('title');
  t.value = 'He said "hi"';
  fire(t, 'keydown', { key: 'Enter' });
  assert.ok(m.state.text.includes('title "He said \\"hi\\""'));
});

test('runtime: a re-render with the SAME tree does not wipe typed text', () => {
  const m = mount(PRIMARY);
  const findings = [];
  m.inspector.setFindings(findings);
  m.select('Transform');
  const [x] = m.inputs('translation');
  x.value = '42';
  // What renderer/editor.js render() does on every status refresh.
  m.inspector.setFindings(findings);
  m.inspector.setSceneTree(m.state.analysis.tree);
  assert.ok(m.inputs('translation')[0] === x, 'same DOM -> same input');
  assert.equal(x.value, '42');
});

test('runtime: non-Node selections and read-only documents expose no controls', () => {
  const m = mount(H + 'DEF S Shape { }\nTransform { children [ USE S ] }\nGroup { children [ Shape {\n');
  m.selection.setSelection(m.state.analysis.tree.root.id);
  assert.equal(all(m.root, hasClass('inspector-heading')).map((h) => h.textContent).includes('Fields'), false);
  const use = m.state.analysis.tree.items.find((it) => it.kind === 'Use');
  m.selection.setSelection(use.id);
  assert.equal(all(m.root, hasClass('field-row')).length, 0);
  m.select('Transform');
  assert.equal(all(m.root, (e) => e.tag === 'input').length, 0, 'syntax errors -> read-only');
  assert.ok(all(m.root, hasClass('empty-note')).some((n) => n.textContent.includes('syntax errors')));
});

test('runtime: a lost selection is announced in text', () => {
  const m = mount(PRIMARY);
  m.select('Transform');
  m.selection.clearSelection();
  m.inspector.setNotice('Selection cleared: test.');
  const note = all(m.root, hasClass('inspector-notice'))[0];
  assert.equal(note.textContent, 'Note: Selection cleared: test.');
  assert.equal(note.getAttribute('role'), 'status');
  m.select('Transform');
  assert.equal(all(m.root, hasClass('inspector-notice')).length, 0, 'a new selection clears the notice');
});
