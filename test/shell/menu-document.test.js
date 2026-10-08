'use strict';
// SHELL-0 menu integration boundary (src/shell/menu-boundary.js) and
// document-session boundary (src/shell/document-session.js).
const test = require('node:test');
const assert = require('node:assert/strict');
const M = require('../../src/shell/menu-boundary');
const { createDocumentSession, createDocumentSlot, FORBIDDEN_KEYS } = require('../../src/shell/document-session');
const { realServices, fakeEditor, createSelectionController } = require('./fakes');

const code = (fn, c) => assert.throws(fn, (e) => e.code === c, `expected ${c}`);
const cmd = (id, extra = {}) => ({ id, area: id.split('.')[0], label: id, run: () => id, ...extra });

test('menu set: ids only, valid, unique', () => {
  const set = M.createMenuCommandSet(['file.save', 'edit.undo']);
  assert.ok(set.allows('file.save'));
  assert.equal(set.allows('file.open'), false);
  assert.equal(set.allows({ toString: () => 'file.save' }), false);
  for (const bad of [[], ['file.save', 'file.save'], ['rm -rf'], [42], 'file.save']) code(() => M.createMenuCommandSet(bad), 'EMENU_INVALID');
});

test('menu dispatch: same registry path as toolbar/keyboard, with source "menu"', () => {
  const { commands, commandRegistry } = realServices();
  const sources = [];
  commands.register({ ...cmd('file.save'), run: (ctx) => { sources.push(ctx.source); return 'saved'; } });
  const set = M.createMenuCommandSet(['file.save', 'edit.undo']);
  assert.deepEqual(M.dispatchMenuCommand(commandRegistry, set, 'file.save'), { ok: true, value: 'saved' });
  assert.deepEqual(sources, ['menu']);
});

test('menu dispatch refuses: not allow-listed, unregistered, page without a registry, disabled', () => {
  const { ctx, commands, commandRegistry } = realServices({ profile: 'world' });
  let ran = 0;
  commands.register(cmd('file.repack', { profiles: ['mall'], run: () => { ran++; } }));
  commands.register(cmd('view.secret', { run: () => { ran++; } }));
  const set = M.createMenuCommandSet(['file.repack', 'edit.undo']);
  const log = [];
  const opts = { log: (r) => log.push(r) };
  assert.deepEqual(M.dispatchMenuCommand(commandRegistry, set, 'view.secret', opts), { ok: false, reason: 'not-allowed' });
  assert.deepEqual(M.dispatchMenuCommand(commandRegistry, set, 'edit.undo', opts), { ok: false, reason: 'unregistered' });
  assert.deepEqual(M.dispatchMenuCommand(null, set, 'edit.undo', opts), { ok: false, reason: 'no-registry' });
  assert.deepEqual(M.dispatchMenuCommand(commandRegistry, set, 'file.repack', opts), { ok: false, reason: 'disabled' });
  assert.equal(ran, 0);
  assert.deepEqual(log.map((r) => r.reason), ['not-allowed', 'unregistered', 'no-registry']);
  ctx.profile = 'mall';
  assert.equal(M.dispatchMenuCommand(commandRegistry, set, 'file.repack').ok, true);
  assert.equal(ran, 1);
});

test('menu state: pulled from the registry, pushed only on change, disposable', () => {
  const { ctx, commands, commandRegistry } = realServices({ profile: 'mall' });
  let dark = false;
  commands.register(cmd('view.dark', { checked: () => dark }));
  commands.register(cmd('file.repack', { profiles: ['mall'] }));
  const set = M.createMenuCommandSet(['view.dark', 'file.repack', 'edit.undo']);
  const sent = [];
  const sub = M.subscribeMenuState(commandRegistry, set, (s) => sent.push(s));
  assert.deepEqual(sent[0], [
    { id: 'view.dark', registered: true, enabled: true, checked: false },
    { id: 'file.repack', registered: true, enabled: true, checked: null },
    { id: 'edit.undo', registered: false, enabled: false, checked: null },
  ]);
  commandRegistry.invalidate(); // nothing changed -> nothing sent
  assert.equal(sent.length, 1);
  dark = true; ctx.profile = 'world';
  commandRegistry.invalidate();
  assert.equal(sent.length, 2);
  assert.equal(sent[1][0].checked, true);
  assert.equal(sent[1][1].enabled, false);
  sub.dispose(); sub.dispose();
  dark = false;
  commandRegistry.invalidate();
  assert.equal(sent.length, 2);
});

test('menu state on a page without a registry: one all-disabled snapshot, nothing owned', () => {
  const set = M.createMenuCommandSet(['file.save']);
  const sent = [];
  const sub = M.subscribeMenuState(null, set, (s) => sent.push(s));
  assert.deepEqual(sent, [[{ id: 'file.save', registered: false, enabled: false, checked: null }]]);
  sub.dispose();
});

function session(extra = {}) {
  const editor = fakeEditor();
  const selection = createSelectionController();
  return { editor, selection, s: createDocumentSession({ sessionId: 7, profile: 'world', editor, selection, ...extra }) };
}

test('document session: text and history READ THROUGH to the CodeMirror handle', () => {
  const { editor, s } = session();
  assert.equal(s.text(), editor.getText());
  editor.type('Group { }\n');
  assert.equal(s.text(), '#VRML V2.0 utf8\nGroup { }\n');
  assert.deepEqual(s.history(), { undo: 1, redo: 0 });
});

test('document session: no second canonical source -- nothing holds the text', () => {
  const { editor, s } = session();
  editor.type('DEF Marker Transform { }\n');
  const text = editor.getText();
  // no enumerable state carries the text; JSON of the session never contains it
  assert.ok(!JSON.stringify(s).includes('Marker'));
  for (const key of Object.keys(s)) assert.notEqual(typeof s[key] === 'string' && s[key], text);
  // no write/undo/serialize surface exists at all
  for (const forbidden of ['setText', 'setDoc', 'undo', 'redo', 'serialize', 'save', 'path']) assert.equal(s[forbidden], undefined, forbidden);
  assert.ok(Object.isFrozen(s));
});

test('document session: construction refuses source, undo, scene-graph, path and workspace state', () => {
  const editor = fakeEditor();
  const selection = createSelectionController();
  const base = { sessionId: 1, profile: 'mall', editor, selection };
  for (const key of Object.keys(FORBIDDEN_KEYS)) {
    code(() => createDocumentSession({ ...base, [key]: 'x' }), 'EDOCSESSION_FORBIDDEN');
  }
  code(() => createDocumentSession({ ...base, anything: 1 }), 'EDOCSESSION_INVALID');
  code(() => createDocumentSession({ ...base, profile: 'cybertown' }), 'EDOCSESSION_INVALID');
  code(() => createDocumentSession({ ...base, sessionId: undefined }), 'EDOCSESSION_INVALID');
  code(() => createDocumentSession({ ...base, editor: { text: 'x' } }), 'EDOCSESSION_INVALID');
  code(() => createDocumentSession({ ...base, selection: { id: 'x' } }), 'EDOCSESSION_INVALID');
});

test('document session: profile/sessionId fixed; selection is the SAME authority object', () => {
  const { selection, s } = session();
  assert.equal(s.selection, selection);
  selection.setSelection('n3');
  assert.equal(s.selection.getSelection(), 'n3');
  assert.throws(() => { s.profile = 'mall'; }, TypeError);
  assert.equal(s.profile, 'world');
  assert.equal(s.isCurrent(7), true);
  assert.equal(s.isCurrent(8), false);
});

test('document session: the one edit path is the handle\'s verified edit', () => {
  const { editor, s } = session();
  const oldText = s.text();
  const newText = oldText + 'Shape { }\n';
  assert.deepEqual(s.applyVerifiedEdits({ oldText, edits: [{ from: oldText.length, to: oldText.length, insert: 'Shape { }\n' }], newText }), { ok: true });
  assert.equal(editor.getText(), newText);
  assert.equal(s.applyVerifiedEdits({ oldText, edits: [], newText: oldText }).ok, false); // stale -> refused by the handle
});

test('document session: analysis is derived and dropped; dispose owns attachments, preview and the view', () => {
  let previewDisposed = 0;
  const { editor, selection, s } = session({ preview: { dispose() { previewDisposed++; } } });
  s.setAnalysis({ version: 1 });
  assert.deepEqual(s.analysis(), { version: 1 });
  const before = selection.listenerCount();
  s.own(selection.subscribe(() => {}));
  assert.equal(selection.listenerCount(), before + 1);
  s.dispose(); s.dispose();
  assert.equal(selection.listenerCount(), before);
  assert.equal(previewDisposed, 1);
  assert.equal(editor.destroyed, 1);
  assert.equal(s.analysis(), null);
  assert.equal(s.isCurrent(7), false);
  code(() => s.text(), 'EDOCSESSION_DISPOSED');
});

test('document slot: one document at a time; opening disposes the previous', () => {
  const slot = createDocumentSlot();
  const seen = [];
  const off = slot.subscribe((cur) => seen.push(cur && cur.sessionId));
  const a = session().s;
  const b = createDocumentSession({ sessionId: 8, profile: 'mall', editor: fakeEditor(), selection: createSelectionController() });
  slot.open(a);
  slot.open(b);
  assert.equal(a.isDisposed, true);
  assert.equal(slot.current(), b);
  assert.equal(slot.close(), true);
  assert.equal(slot.close(), false);
  assert.equal(b.isDisposed, true);
  assert.deepEqual(seen, [7, 8, null]);
  off();
  assert.equal(slot.listenerCount(), 0);
  code(() => slot.open({}), 'EDOCSESSION_INVALID');
});
