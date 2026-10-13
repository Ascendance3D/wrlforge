'use strict';
// UI-0 Panel Registry unit tests (docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md §11).
const test = require('node:test');
const assert = require('node:assert/strict');
const { createPanelRegistry } = require('../../src/editor/panel-registry');

const code = (fn, c) => assert.throws(fn, (e) => e.code === c, `expected ${c}`);

function fakePanel(opts = {}) {
  const state = { visible: opts.visible !== false, focused: 0 };
  const focusable = opts.noFocusable ? null : { focus() { state.focused++; } };
  const el = {
    getClientRects() { return state.visible ? [1] : []; },
    querySelector() { return focusable; },
  };
  return { state, el };
}
const base = (id, el, extra = {}) => ({ id, title: id.toUpperCase(), element: () => el, ...extra });

test('registration, frozen descriptor, list order, get null', () => {
  const r = createPanelRegistry();
  const un = r.register(base('preview', fakePanel().el));
  r.register(base('outline', fakePanel().el, { canToggle: true, show() {}, hide() {} }));
  const d = r.get('preview');
  assert.ok(Object.isFrozen(d));
  assert.deepEqual(d, { id: 'preview', title: 'PREVIEW', canToggle: false });
  assert.equal(r.get('outline').canToggle, true);
  assert.deepEqual(r.list().map((x) => x.id), ['preview', 'outline']);
  assert.equal(r.get('nope'), null);
  assert.ok(r.has('preview'));
  un();
  assert.ok(!r.has('preview'));
});

test('EPANEL_DUPLICATE', () => {
  const r = createPanelRegistry();
  r.register(base('preview', fakePanel().el));
  code(() => r.register(base('preview', fakePanel().el)), 'EPANEL_DUPLICATE');
});

test('EPANEL_INVALID variants', () => {
  const r = createPanelRegistry();
  const el = fakePanel().el;
  const bad = [
    base('Bad', el), base('1x', el), base('has space', el),
    base('console', el),
    { id: 'a', element: () => el },
    base('a', el, { title: '  ' }),
    { id: 'a', title: 'A', element: el },
    base('a', el, { focus: 'x' }),
    base('a', el, { canToggle: 'yes' }),
    base('a', el, { canToggle: true }),
    base('a', el, { canToggle: true, show() {} }),
    null,
  ];
  for (const b of bad) code(() => r.register(b), 'EPANEL_INVALID');
  assert.equal(r.list().length, 0);
});

test('EPANEL_UNKNOWN', () => {
  const r = createPanelRegistry();
  for (const m of ['isVisible', 'focus', 'show', 'hide']) code(() => r[m]('x'), 'EPANEL_UNKNOWN');
});

test('isVisible is derived live; false when element() is null', () => {
  const r = createPanelRegistry();
  const p = fakePanel();
  r.register(base('preview', p.el));
  assert.equal(r.isVisible('preview'), true);
  p.state.visible = false;
  assert.equal(r.isVisible('preview'), false);
  p.state.visible = true;
  assert.equal(r.isVisible('preview'), true);
  r.register({ id: 'gone', title: 'G', element: () => null });
  assert.equal(r.isVisible('gone'), false);
});

test('focus on a hidden panel refuses without show or focus', () => {
  const r = createPanelRegistry();
  const p = fakePanel({ visible: false });
  let shown = 0; let customFocus = 0;
  r.register(base('preview', p.el, { canToggle: true, show() { shown++; }, hide() {}, focus() { customFocus++; } }));
  assert.deepEqual(r.focus('preview'), { ok: false, reason: 'hidden' });
  assert.equal(shown, 0);
  assert.equal(customFocus, 0);
  assert.equal(p.state.focused, 0);
});

test('focus uses record.focus, else first focusable, else no-focusable', () => {
  const r = createPanelRegistry();
  const a = fakePanel();
  let custom = 0;
  r.register(base('a', a.el, { focus() { custom++; } }));
  assert.deepEqual(r.focus('a'), { ok: true });
  assert.equal(custom, 1);
  assert.equal(a.state.focused, 0);

  const b = fakePanel();
  r.register(base('b', b.el));
  assert.deepEqual(r.focus('b'), { ok: true });
  assert.equal(b.state.focused, 1);

  const c = fakePanel({ noFocusable: true });
  r.register(base('c', c.el));
  assert.deepEqual(r.focus('c'), { ok: false, reason: 'no-focusable' });
});

test('show/hide: not-toggleable, call-through, record-supplied failure', () => {
  const r = createPanelRegistry();
  r.register(base('fixed', fakePanel().el));
  assert.deepEqual(r.show('fixed'), { ok: false, reason: 'not-toggleable' });
  assert.deepEqual(r.hide('fixed'), { ok: false, reason: 'not-toggleable' });
  const calls = [];
  r.register(base('tog', fakePanel().el, { canToggle: true, show() { calls.push('show'); }, hide() { calls.push('hide'); } }));
  assert.deepEqual(r.show('tog'), { ok: true });
  assert.deepEqual(r.hide('tog'), { ok: true });
  assert.deepEqual(calls, ['show', 'hide']);
  r.register(base('veto', fakePanel().el, { canToggle: true, show: () => ({ ok: false, reason: 'busy' }), hide: () => ({ ok: true }) }));
  assert.deepEqual(r.show('veto'), { ok: false, reason: 'busy' });
});

test('dispose clears and blocks register', () => {
  const r = createPanelRegistry();
  r.register(base('a', fakePanel().el));
  r.dispose();
  assert.equal(r.list().length, 0);
  code(() => r.register(base('b', fakePanel().el)), 'EPANEL_DISPOSED');
});

test('registry stores nothing about layout or document', () => {
  const r = createPanelRegistry();
  assert.deepEqual(Object.keys(r), ['register', 'has', 'get', 'list', 'isVisible', 'focus', 'show', 'hide', 'dispose']);
  r.register(base('a', fakePanel().el, { width: 300, order: 2 }));
  assert.deepEqual(Object.keys(r.get('a')).sort(), ['canToggle', 'id', 'title']);
});
