'use strict';
// UI-0 command bindings runtime (renderer/command-bindings.js) against a stub DOM
// (docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md §9.1, §10.1).
const test = require('node:test');
const assert = require('node:assert/strict');
const CR = require('../../src/editor/command-registry');
const CB = require('../../renderer/command-bindings');

function el(tagName, attrs = {}, extra = {}) {
  const listeners = {};
  const node = {
    tagName, id: attrs.id || '', disabled: false, value: '', children: [],
    _attrs: { ...attrs },
    getAttribute(n) { return Object.prototype.hasOwnProperty.call(this._attrs, n) ? this._attrs[n] : null; },
    setAttribute(n, v) { this._attrs[n] = String(v); },
    addEventListener(t, f) { (listeners[t] = listeners[t] || []).push(f); },
    removeEventListener(t, f) { listeners[t] = (listeners[t] || []).filter((x) => x !== f); },
    fire(t, ev = {}) { for (const f of [...(listeners[t] || [])]) f(ev); },
    listenerCount(t) { return (listeners[t] || []).length; },
    click() { if (!this.disabled) this.fire('click'); },
    querySelectorAll(sel) { return sel === 'option' ? this.children.filter((c) => c.tagName === 'OPTION') : []; },
    ...extra,
  };
  return node;
}
const button = (id, cmd) => el('BUTTON', { id, 'data-command': cmd });
const option = (value, cmd) => { const o = el('OPTION', { 'data-command': cmd }); o.value = value; return o; };
function root(nodes) {
  return {
    querySelectorAll(sel) {
      if (sel === '[data-command]') return nodes.flatMap((n) => [n, ...n.children]).filter((n) => n.getAttribute('data-command') !== null);
      if (sel === '[data-command-select]') return nodes.filter((n) => n.getAttribute('data-command-select') !== null);
      return [];
    },
  };
}
const rec = (id, extra = {}) => ({ id, label: id, area: id.split('.')[0], run() {}, ...extra });
const keydown = (target, o) => {
  const ev = { defaultPrevented: false, target: el('DIV'), preventDefault() { this.defaultPrevented = true; }, ...o };
  target.fire('keydown', ev);
  return ev;
};

test('click executes with source toolbar', () => {
  const reg = CR.createCommandRegistry();
  const seen = [];
  reg.register(rec('file.save', { run: (ctx) => seen.push(ctx.source) }));
  const b = button('b', 'file.save');
  CB.bindControls(reg, root([b]));
  b.click();
  assert.deepEqual(seen, ['toolbar']);
});

test('paints disabled + aria-pressed initially and on invalidate', () => {
  const reg = CR.createCommandRegistry();
  let on = false; let checked = false;
  reg.register(rec('file.save', { enabled: () => on }));
  reg.register(rec('view.toggle', { checked: () => checked }));
  const b = button('b', 'file.save');
  const t = button('t', 'view.toggle');
  CB.bindControls(reg, root([b, t]));
  assert.equal(b.disabled, true);
  assert.equal(t.getAttribute('aria-pressed'), 'false');
  assert.equal(b.getAttribute('aria-pressed'), null);
  on = true; checked = true;
  assert.equal(b.disabled, true); // not repainted until invalidate
  reg.invalidate();
  assert.equal(b.disabled, false);
  assert.equal(t.getAttribute('aria-pressed'), 'true');
});

test('disabled command refuses through click, forced dispatch and keyboard', () => {
  const reg = CR.createCommandRegistry();
  let runs = 0;
  reg.register(rec('file.save', { keys: ['Mod+S'], enabled: () => false, run() { runs++; } }));
  const b = button('b', 'file.save');
  CB.bindControls(reg, root([b]));
  b.click();
  assert.equal(runs, 0);
  b.fire('click'); // forced dispatch bypassing the disabled guard
  assert.equal(runs, 0);
  const win = el('WINDOW');
  CB.installKeyboard(reg, { target: win });
  const ev = keydown(win, { key: 's', ctrlKey: true });
  assert.equal(ev.defaultPrevented, true);
  assert.equal(runs, 0);
});

test('unknown data-command throws ECOMMAND_BIND', () => {
  const reg = CR.createCommandRegistry();
  assert.throws(() => CB.bindControls(reg, root([button('b', 'file.nope')])), (e) => e.code === 'ECOMMAND_BIND');
  const sel = el('SELECT', { id: 's', 'data-command-select': '' });
  sel.children = [option('x', 'view.nope')];
  assert.throws(() => CB.bindControls(reg, root([sel])), (e) => e.code === 'ECOMMAND_BIND');
});

test('OPTION elements carrying data-command are not bound as buttons', () => {
  const reg = CR.createCommandRegistry();
  reg.register(rec('workspace.code', { checked: () => true }));
  const o = option('code', 'workspace.code');
  const r = { querySelectorAll: (s) => (s === '[data-command]' ? [o] : []) };
  CB.bindControls(reg, r);
  assert.equal(o.listenerCount('click'), 0);
  assert.equal(o.getAttribute('aria-pressed'), null);
  assert.equal(o.disabled, false);
});

test('select radio group paints and executes', () => {
  const reg = CR.createCommandRegistry();
  let mode = 'code'; const ran = [];
  for (const m of ['code', 'model']) {
    reg.register(rec(`workspace.${m}`, { checked: () => mode === m, run: (ctx) => { ran.push([m, ctx.source]); mode = m; } }));
  }
  const sel = el('SELECT', { id: 's', 'data-command-select': '' });
  sel.children = [option('code', 'workspace.code'), option('model', 'workspace.model')];
  sel.value = 'model';
  CB.bindControls(reg, root([sel]));
  assert.equal(sel.value, 'code'); // painted from checked option
  sel.value = 'model';
  sel.fire('change');
  assert.deepEqual(ran, [['model', 'toolbar']]);
  reg.invalidate();
  assert.equal(sel.value, 'model');
});

test('bindControls returns an unsubscribe that stops repainting', () => {
  const reg = CR.createCommandRegistry();
  let on = true;
  reg.register(rec('file.save', { enabled: () => on }));
  const b = button('b', 'file.save');
  const un = CB.bindControls(reg, root([b]));
  un(); on = false; reg.invalidate();
  assert.equal(b.disabled, false);
});

test('keyboard: Ctrl+S and Meta+S execute with source keyboard and preventDefault', () => {
  const reg = CR.createCommandRegistry();
  const seen = [];
  reg.register(rec('file.save', { keys: ['Mod+S'], run: (ctx) => seen.push(ctx.source) }));
  const win = el('WINDOW');
  CB.installKeyboard(reg, { target: win });
  const e1 = keydown(win, { key: 's', ctrlKey: true });
  const e2 = keydown(win, { key: 's', metaKey: true });
  assert.deepEqual(seen, ['keyboard', 'keyboard']);
  assert.ok(e1.defaultPrevented && e2.defaultPrevented);
});

test('keyboard: editor-owned and unbound keys are untouched', () => {
  const reg = CR.createCommandRegistry();
  let runs = 0;
  reg.register(rec('edit.undo', { keys: ['Mod+Z'], keyOwner: 'editor', run() { runs++; } }));
  const win = el('WINDOW');
  CB.installKeyboard(reg, { target: win });
  const e1 = keydown(win, { key: 'z', ctrlKey: true });
  const e2 = keydown(win, { key: 'q', ctrlKey: true });
  const e3 = keydown(win, { key: 'x' });
  const e4 = keydown(win, { key: 'Tab', ctrlKey: true });
  assert.equal(runs, 0);
  for (const e of [e1, e2, e3, e4]) assert.equal(e.defaultPrevented, false);
});

test('keyboard: Mod shortcuts fire from inputs and .cm-editor, even if already defaultPrevented', () => {
  const reg = CR.createCommandRegistry();
  let runs = 0;
  reg.register(rec('file.save', { keys: ['Mod+S'], run() { runs++; } }));
  const win = el('WINDOW');
  CB.installKeyboard(reg, { target: win });
  keydown(win, { key: 's', ctrlKey: true, target: el('INPUT') });
  keydown(win, { key: 's', ctrlKey: true, target: el('DIV', {}, { closest: (s) => (s === '.cm-editor' ? {} : null) }) });
  assert.equal(runs, 2);
  keydown(win, { key: 's', ctrlKey: true, defaultPrevented: true });
  assert.equal(runs, 3); // today's double-fire preserved
});

test('keyboard: bare-key bindings are suppressed in text entry', () => {
  const reg = CR.createCommandRegistry();
  let runs = 0;
  reg.register(rec('model.testDelete', { keys: ['Delete'], run() { runs++; } }));
  const win = el('WINDOW');
  CB.installKeyboard(reg, { target: win });
  const suppressed = [
    el('INPUT'), el('TEXTAREA'), el('DIV', {}, { isContentEditable: true }),
    el('DIV', {}, { closest: (s) => (s === '.cm-editor' ? {} : null) }),
  ];
  for (const t of suppressed) {
    const ev = keydown(win, { key: 'Delete', target: t });
    assert.equal(ev.defaultPrevented, false);
  }
  assert.equal(runs, 0);
  const ev = keydown(win, { key: 'Delete', target: el('DIV', {}, { closest: () => null }) });
  assert.equal(runs, 1);
  assert.equal(ev.defaultPrevented, true);
});

test('keyboard: uninstall removes the listener', () => {
  const reg = CR.createCommandRegistry();
  let runs = 0;
  reg.register(rec('file.save', { keys: ['Mod+S'], run() { runs++; } }));
  const win = el('WINDOW');
  const un = CB.installKeyboard(reg, { target: win });
  assert.equal(win.listenerCount('keydown'), 1);
  un();
  assert.equal(win.listenerCount('keydown'), 0);
  keydown(win, { key: 's', ctrlKey: true });
  assert.equal(runs, 0);
});
