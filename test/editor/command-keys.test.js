'use strict';
// UI-0 shortcut grammar + parity with ui-state.resolveShortcut + CodeMirror
// conflict table (docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md §9).
const test = require('node:test');
const assert = require('node:assert/strict');
const CR = require('../../src/editor/command-registry');
const UI = require('../../src/editor/ui-state');

const { normalizeShortcut, shortcutFromEvent, isModShortcut, createCommandRegistry } = CR;
const invalid = (s) => assert.throws(() => normalizeShortcut(s), (e) => e.code === 'ECOMMAND_INVALID', s);

test('normalizeShortcut grammar', () => {
  assert.equal(normalizeShortcut('Mod+S'), 'Mod+s');
  assert.equal(normalizeShortcut('Mod+Shift+S'), 'Mod+Shift+s');
  assert.equal(normalizeShortcut('Mod++'), 'Mod++');
  assert.equal(normalizeShortcut('Mod+Shift+0'), 'Mod+0');
  assert.equal(normalizeShortcut('Mod+Shift+Enter'), 'Mod+Shift+Enter');
  assert.equal(normalizeShortcut('Mod+add'), 'Mod+Add');
  assert.equal(normalizeShortcut('Mod+Shift+Add'), 'Mod+Add');
  assert.equal(normalizeShortcut('Delete'), 'Delete');
  assert.equal(normalizeShortcut('+'), '+');
  assert.equal(normalizeShortcut('Mod+='), 'Mod+=');
  assert.equal(normalizeShortcut('Mod+_'), 'Mod+_');
});

test('normalizeShortcut rejects invalid shortcuts', () => {
  for (const s of ['Ctrl+S', 'Alt+S', 'Mod+Mod+S', 'Mod+Shift+Shift+S', '', 'Mod+', 'Mod+Tab', 'Mod+ArrowUp', 'Mod+Foo', 'Mod+Ctrl+S']) invalid(s);
  assert.throws(() => normalizeShortcut(null), (e) => e.code === 'ECOMMAND_INVALID');
  assert.throws(() => normalizeShortcut(5), (e) => e.code === 'ECOMMAND_INVALID');
});

test('shortcutFromEvent', () => {
  assert.equal(shortcutFromEvent({ key: 's', ctrlKey: true }), 'Mod+s');
  assert.equal(shortcutFromEvent({ key: 's', metaKey: true }), 'Mod+s');
  assert.equal(shortcutFromEvent({ key: 'S', ctrlKey: true, shiftKey: true }), 'Mod+Shift+s');
  assert.equal(shortcutFromEvent({ key: 's', ctrlKey: true, altKey: true }), 'Mod+s'); // Alt ignored
  assert.equal(shortcutFromEvent({ key: 'Enter', ctrlKey: true, shiftKey: true }), 'Mod+Shift+Enter');
  assert.equal(shortcutFromEvent({ key: '0', ctrlKey: true, shiftKey: true }), 'Mod+0');
  assert.equal(shortcutFromEvent({ key: '+', ctrlKey: true, shiftKey: true }), 'Mod++');
  assert.equal(shortcutFromEvent({ key: 'Delete' }), 'Delete');
  assert.equal(shortcutFromEvent({ key: 's' }), 's');
  assert.equal(shortcutFromEvent({ ctrlKey: true }), null);
  assert.equal(shortcutFromEvent({ key: '', ctrlKey: true }), null);
  assert.equal(shortcutFromEvent({ key: 'Tab', ctrlKey: true }), null);
  assert.equal(shortcutFromEvent(null), null);
});

test('isModShortcut', () => {
  assert.equal(isModShortcut('Mod+s'), true);
  assert.equal(isModShortcut('Delete'), false);
  assert.equal(isModShortcut(null), false);
});

// --- exhaustive parity with ui-state.resolveShortcut ---------------------------

function editorRegistry() {
  const r = createCommandRegistry();
  const add = (id, keys, keyOwner) => r.register({ id, label: id, area: id.split('.')[0], keys, keyOwner, run() {} });
  add('file.save', ['Mod+S']);
  add('file.saveAs', ['Mod+Shift+S']);
  add('edit.gotoLine', ['Mod+G']);
  add('file.close', ['Mod+W']);
  add('view.zoomIn', ['Mod+=', 'Mod++', 'Mod+Add']);
  add('view.zoomOut', ['Mod+-', 'Mod+_', 'Mod+Subtract']);
  add('view.zoomReset', ['Mod+0']);
  add('preview.update', ['Mod+Enter']);
  add('preview.toggleMaximize', ['Mod+Shift+Enter']);
  add('edit.undo', ['Mod+Z'], 'editor');
  add('edit.redo', ['Mod+Y', 'Mod+Shift+Z'], 'editor');
  add('edit.find', ['Mod+F'], 'editor');
  return r;
}

const NAME_TO_ID = {
  save: 'file.save', saveAs: 'file.saveAs', gotoLine: 'edit.gotoLine', close: 'file.close',
  zoomIn: 'view.zoomIn', zoomOut: 'view.zoomOut', zoomReset: 'view.zoomReset',
  previewUpdate: 'preview.update', previewMaximize: 'preview.toggleMaximize',
};

function appLookup(reg, ev) {
  const key = shortcutFromEvent(ev);
  if (!key) return null;
  const b = reg.keyBindings().find((x) => x.key === key && reg.get(x.id).keyOwner === 'app');
  return b ? b.id : null;
}

test('parity with UI.resolveShortcut across the input space', () => {
  const reg = editorRegistry();
  const keys = [];
  for (let c = 97; c <= 122; c++) keys.push(String.fromCharCode(c), String.fromCharCode(c - 32));
  for (let d = 0; d <= 9; d++) keys.push(String(d));
  keys.push(...'!"#$%&\'()*+,-./:;<=>?@[\\]^_`{|}~');
  keys.push(' ', 'Enter', 'ENTER', 'enter', 'Escape', 'Delete', 'Tab', 'ArrowUp', 'ArrowDown', 'Backspace', 'Home', 'End', 'PageUp', 'PageDown', 'Insert', 'add', 'Add', 'subtract', 'Subtract');
  for (let i = 1; i <= 12; i++) keys.push('F' + i);
  keys.push('Shift', 'Control', '', 'Unidentified');
  let checked = 0;
  for (const key of keys) {
    for (const mod of [true, false]) {
      for (const shift of [true, false]) {
        const expected = NAME_TO_ID[UI.resolveShortcut({ key, ctrlOrMeta: mod, shift })] || null;
        const viaCtrl = appLookup(reg, { key, ctrlKey: mod, metaKey: false, shiftKey: shift });
        const viaMeta = appLookup(reg, { key, ctrlKey: false, metaKey: mod, shiftKey: shift });
        const label = JSON.stringify({ key, mod, shift });
        assert.equal(viaCtrl, expected, 'ctrl ' + label);
        assert.equal(viaMeta, expected, 'meta ' + label);
        checked++;
      }
    }
  }
  assert.ok(checked > 500, `checked ${checked}`);
});

test('editor-owned bindings are never chosen by the app-only lookup', () => {
  const reg = editorRegistry();
  const cases = [
    { key: 'z', ctrlKey: true }, { key: 'y', ctrlKey: true },
    { key: 'z', ctrlKey: true, shiftKey: true }, { key: 'f', ctrlKey: true },
  ];
  for (const ev of cases) {
    assert.equal(appLookup(reg, ev), null, JSON.stringify(ev));
    assert.ok(reg.keyBindings().some((b) => b.key === shortcutFromEvent(ev)), 'binding exists for editor key');
  }
  for (const b of reg.keyBindings()) {
    if (reg.get(b.id).keyOwner === 'app') {
      for (const k of ['Mod+z', 'Mod+y', 'Mod+Shift+z', 'Mod+f']) assert.notEqual(b.key, k);
    }
  }
});

// --- CodeMirror conflict table -------------------------------------------------

// #35 preserves these collisions (pure migration of today's behaviour: the app
// dispatcher and CodeMirror both react). UI-0-I2 (#121) resolves Mod+G and
// Mod+Enter per owner decision D2. Pairs are [appCommandId, codemirrorKey].
const KNOWN_CODEMIRROR_COLLISIONS = Object.freeze([
  ['edit.gotoLine', 'Mod-Alt-g'], // CodeMirror gotoLine; Alt is not significant to the app dispatcher
  ['edit.gotoLine', 'Mod-g'], // CodeMirror findNext
  ['preview.update', 'Mod-Enter'], // CodeMirror insertBlankLine
]);

const MODIFIERS = { Mod: 'ctrlKey', Ctrl: 'ctrlKey', Control: 'ctrlKey', Alt: 'altKey', Shift: 'shiftKey', Meta: 'metaKey', Cmd: 'metaKey' };

function cmKeyToEvent(spec) {
  const ev = { key: '', ctrlKey: false, altKey: false, shiftKey: false, metaKey: false };
  let rest = spec;
  for (;;) {
    const m = /^([A-Za-z]+)-(.+)$/.exec(rest);
    if (m && Object.prototype.hasOwnProperty.call(MODIFIERS, m[1])) { ev[MODIFIERS[m[1]]] = true; rest = m[2]; } else break;
  }
  ev.key = rest;
  return ev;
}

async function loadKeymaps() {
  const load = async (name) => { try { return require(name); } catch { return import(name); } };
  const cmds = await load('@codemirror/commands');
  const search = await load('@codemirror/search');
  return [...cmds.defaultKeymap, ...cmds.historyKeymap, ...search.searchKeymap, cmds.indentWithTab];
}

test('CodeMirror default keymaps collide with app shortcuts only as documented', async () => {
  const reg = editorRegistry();
  const bindings = await loadKeymaps();
  assert.ok(bindings.length > 50);
  const found = new Set();
  for (const b of bindings) {
    for (const spec of [b.key, b.linux, b.win]) {
      if (!spec) continue;
      const id = appLookup(reg, cmKeyToEvent(spec));
      if (id) found.add(JSON.stringify([id, spec]));
    }
  }
  const measured = [...found].map((s) => JSON.parse(s)).sort((a, b) => (a.join() < b.join() ? -1 : 1));
  const expected = KNOWN_CODEMIRROR_COLLISIONS.map((p) => [...p]).sort((a, b) => (a.join() < b.join() ? -1 : 1));
  if (JSON.stringify(measured) !== JSON.stringify(expected)) console.log('MEASURED', JSON.stringify(measured));
  assert.deepEqual(measured, expected);
});
