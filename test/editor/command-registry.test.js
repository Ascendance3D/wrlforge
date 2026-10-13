'use strict';
// UI-0 Command Registry unit tests (docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md §6, §9).
const test = require('node:test');
const assert = require('node:assert/strict');
const { createCommandRegistry } = require('../../src/editor/command-registry');

const rec = (id, extra = {}) => ({ id, label: id, area: id.split('.')[0], run() { return 1; }, ...extra });
const code = (fn, c) => assert.throws(fn, (e) => e.code === c, `expected ${c}`);

test('register returns an unregister fn', () => {
  const r = createCommandRegistry();
  const un = r.register(rec('file.save'));
  assert.equal(typeof un, 'function');
  assert.ok(r.has('file.save'));
  un();
  assert.ok(!r.has('file.save'));
  assert.equal(r.get('file.save'), null);
});

test('descriptor is frozen, exact shape, no run exposed', () => {
  const r = createCommandRegistry();
  r.register(rec('file.save', { keys: ['Mod+S'], checked: () => true }));
  const d = r.get('file.save');
  assert.ok(Object.isFrozen(d));
  assert.deepEqual(Object.keys(d).sort(), ['area', 'id', 'isToggle', 'keyOwner', 'keys', 'label']);
  assert.equal(d.run, undefined);
  assert.deepEqual(d.keys, ['Mod+s']);
  assert.equal(d.keyOwner, 'app');
  assert.equal(d.isToggle, true);
  assert.ok(Object.isFrozen(d.keys));
  assert.equal(r.get('nope.x'), null);
});

test('ECOMMAND_DUPLICATE', () => {
  const r = createCommandRegistry();
  r.register(rec('file.save'));
  code(() => r.register(rec('file.save')), 'ECOMMAND_DUPLICATE');
});

test('ECOMMAND_INVALID variants', () => {
  const r = createCommandRegistry();
  const bad = [
    rec('file.save', { id: 'Bad' }),
    rec('file.save', { id: 'file' }),
    rec('file.save', { id: 'file.Save' }),
    rec('file.save', { area: 'edit' }),
    { id: 'zzz.save', label: 'x', area: 'zzz', run() {} },
    rec('file.save', { label: undefined }),
    rec('file.save', { label: '  ' }),
    rec('file.save', { run: 'x' }),
    rec('file.save', { enabled: true }),
    rec('file.save', { checked: 'y' }),
    rec('file.save', { keyOwner: 'plugin' }),
    rec('file.save', { keys: 'Mod+S' }),
    rec('file.save', { keys: [1] }),
    rec('file.save', { keys: ['Ctrl+S'] }),
    null,
  ];
  for (const b of bad) code(() => r.register(b), 'ECOMMAND_INVALID');
  assert.equal(r.list().length, 0);
});

test('ECOMMAND_UNKNOWN from isEnabled/isChecked/execute', () => {
  const r = createCommandRegistry();
  code(() => r.isEnabled('file.x'), 'ECOMMAND_UNKNOWN');
  code(() => r.isChecked('file.x'), 'ECOMMAND_UNKNOWN');
  code(() => r.execute('file.x'), 'ECOMMAND_UNKNOWN');
});

test('list is in registration order', () => {
  const r = createCommandRegistry();
  for (const id of ['view.b', 'file.a', 'edit.c']) r.register(rec(id));
  assert.deepEqual(r.list().map((d) => d.id), ['view.b', 'file.a', 'edit.c']);
});

test('execute runs once, returns value, ctx is {source}', () => {
  const r = createCommandRegistry();
  const seen = [];
  r.register(rec('file.save', { run(ctx) { seen.push(ctx); return 42; } }));
  assert.deepEqual(r.execute('file.save'), { ok: true, value: 42 });
  assert.equal(seen.length, 1);
  assert.deepEqual(seen[0], { source: 'api' });
  r.execute('file.save', { source: 'toolbar' });
  assert.deepEqual(seen[1], { source: 'toolbar' });
  assert.equal(seen.length, 2);
});

test('disabled command does not run; enabled evaluated live', () => {
  const r = createCommandRegistry();
  let on = false; let runs = 0;
  r.register(rec('file.save', { enabled: () => on, run() { runs++; } }));
  assert.deepEqual(r.execute('file.save'), { ok: false, reason: 'disabled' });
  assert.equal(runs, 0);
  assert.equal(r.isEnabled('file.save'), false);
  on = true;
  assert.equal(r.isEnabled('file.save'), true);
  assert.equal(r.execute('file.save').ok, true);
  assert.equal(runs, 1);
  on = false;
  assert.equal(r.execute('file.save').ok, false);
  assert.equal(runs, 1);
});

test('isEnabled defaults true; isChecked null for non-toggles and live for toggles', () => {
  const r = createCommandRegistry();
  let c = false;
  r.register(rec('file.a'));
  r.register(rec('view.t', { checked: () => c }));
  assert.equal(r.isEnabled('file.a'), true);
  assert.equal(r.isChecked('file.a'), null);
  assert.equal(r.isChecked('view.t'), false);
  c = true;
  assert.equal(r.isChecked('view.t'), true);
});

test('async run: resolved value and rejection propagate', async () => {
  const r = createCommandRegistry();
  r.register(rec('file.ok', { run: async () => 'v' }));
  r.register(rec('file.bad', { run: async () => { throw new Error('boom'); } }));
  const p = r.execute('file.ok');
  assert.ok(typeof p.then === 'function');
  assert.deepEqual(await p, { ok: true, value: 'v' });
  await assert.rejects(() => r.execute('file.bad'), /boom/);
});

test('registry survives a throwing handler', () => {
  const r = createCommandRegistry();
  r.register(rec('file.bad', { run() { throw new Error('sync boom'); } }));
  r.register(rec('file.good', { run() { return 'g'; } }));
  assert.throws(() => r.execute('file.bad'), /sync boom/);
  assert.equal(r.list().length, 2);
  assert.deepEqual(r.execute('file.good'), { ok: true, value: 'g' });
  let n = 0;
  r.subscribe(() => n++);
  r.invalidate();
  assert.equal(n, 1);
});

test('subscribe / invalidate / unsubscribe', () => {
  const r = createCommandRegistry();
  let n = 0;
  const un = r.subscribe(() => n++);
  r.invalidate(); r.invalidate();
  assert.equal(n, 2);
  un();
  r.invalidate();
  assert.equal(n, 2);
  code(() => r.subscribe(null), 'ECOMMAND_INVALID');
});

test('a throwing listener is isolated and logged', () => {
  const r = createCommandRegistry();
  const logged = [];
  const orig = console.error;
  console.error = (...a) => logged.push(a);
  try {
    let after = 0;
    r.subscribe(() => { throw new Error('listener boom'); });
    r.subscribe(() => { after++; });
    r.invalidate();
    assert.equal(after, 1);
    assert.equal(logged.length, 1);
  } finally {
    console.error = orig;
  }
});

test('dispose clears listeners and blocks register', () => {
  const r = createCommandRegistry();
  let n = 0;
  r.register(rec('file.a'));
  r.subscribe(() => n++);
  r.dispose();
  r.invalidate();
  assert.equal(n, 0);
  assert.equal(r.list().length, 0);
  code(() => r.register(rec('file.b')), 'ECOMMAND_DISPOSED');
});

test('keyBindings returns normalized entries', () => {
  const r = createCommandRegistry();
  r.register(rec('file.save', { keys: ['Mod+S'] }));
  r.register(rec('view.zoomIn', { keys: ['Mod+=', 'Mod++'] }));
  assert.deepEqual(r.keyBindings(), [
    { key: 'Mod+s', id: 'file.save' },
    { key: 'Mod+=', id: 'view.zoomIn' },
    { key: 'Mod++', id: 'view.zoomIn' },
  ]);
});

test('ECOMMAND_KEY_CONFLICT between app commands; editor-owned overlap allowed', () => {
  const r = createCommandRegistry();
  r.register(rec('file.save', { keys: ['Mod+S'] }));
  code(() => r.register(rec('file.other', { keys: ['Mod+s'] })), 'ECOMMAND_KEY_CONFLICT');
  assert.ok(!r.has('file.other'));
  // editor-owned key equal to an app key is allowed
  r.register(rec('edit.find', { keys: ['Mod+S'], keyOwner: 'editor' }));
  assert.ok(r.has('edit.find'));
});
