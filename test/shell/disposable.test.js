'use strict';
// SHELL-0 disposable lifecycle contract (src/shell/disposable.js).
const test = require('node:test');
const assert = require('node:assert/strict');
const { toDisposable, asDisposable, isDisposable, createDisposableStore, listen } = require('../../src/shell/disposable');

const code = (fn, c) => assert.throws(fn, (e) => e.code === c, `expected ${c}`);

// An EventTarget that can report how many listeners it holds.
function countingTarget() {
  const map = new Map();
  return {
    addEventListener(type, fn) { if (!map.has(type)) map.set(type, new Set()); map.get(type).add(fn); },
    removeEventListener(type, fn) { if (map.has(type)) map.get(type).delete(fn); },
    dispatch(type) { for (const fn of [...(map.get(type) || [])]) fn({ type }); },
    count() { let n = 0; for (const s of map.values()) n += s.size; return n; },
  };
}

test('toDisposable is one-shot: the cleanup runs exactly once', () => {
  let n = 0;
  const d = toDisposable(() => { n++; });
  d.dispose(); d.dispose(); d.dispose();
  assert.equal(n, 1);
  assert.ok(isDisposable(d));
  code(() => toDisposable('nope'), 'EDISPOSABLE_INVALID');
});

test('asDisposable accepts a function or a disposable, refuses anything else', () => {
  let n = 0;
  asDisposable(() => { n++; }).dispose();
  const obj = { dispose() { n++; } };
  assert.equal(asDisposable(obj), obj);
  assert.equal(n, 1);
  for (const bad of [undefined, null, 1, 'x', {}, { dispose: 1 }]) code(() => asDisposable(bad), 'EDISPOSABLE_INVALID');
});

test('a store disposes in reverse registration order, once', () => {
  const order = [];
  const s = createDisposableStore();
  s.add(() => order.push('a'));
  s.add({ dispose: () => order.push('b') });
  s.add(() => order.push('c'));
  assert.equal(s.size, 3);
  s.dispose();
  s.dispose();
  assert.deepEqual(order, ['c', 'b', 'a']);
  assert.equal(s.size, 0);
  assert.equal(s.isDisposed, true);
});

test('every child is disposed even when some throw; errors are re-thrown, not swallowed', () => {
  const order = [];
  const s = createDisposableStore();
  s.add(() => order.push('a'));
  s.add(() => { throw new Error('b failed'); });
  s.add(() => order.push('c'));
  assert.throws(() => s.dispose(), /b failed/);
  assert.deepEqual(order, ['c', 'a']);

  const t = createDisposableStore();
  t.add(() => { throw new Error('x'); });
  t.add(() => { throw new Error('y'); });
  assert.throws(() => t.dispose(), (e) => e instanceof AggregateError && e.errors.length === 2);
});

test('add() after dispose disposes the newcomer immediately (no late leak)', () => {
  const s = createDisposableStore();
  s.dispose();
  let n = 0;
  s.add(() => { n++; });
  assert.equal(n, 1);
  assert.equal(s.size, 0);
});

test('release() disposes one child early and forgets it', () => {
  const order = [];
  const s = createDisposableStore();
  s.add(() => order.push('a'));
  const b = s.add(() => order.push('b'));
  assert.equal(s.release(b), true);
  assert.equal(s.release(b), false);
  s.dispose();
  assert.deepEqual(order, ['b', 'a']);
});

test('nested stores: disposing the parent tears the child down first (reverse order)', () => {
  const order = [];
  const parent = createDisposableStore();
  parent.add(() => order.push('parent-first'));
  const child = createDisposableStore();
  child.add(() => order.push('child-1'));
  child.add(() => order.push('child-2'));
  parent.add(child);
  parent.dispose();
  assert.deepEqual(order, ['child-2', 'child-1', 'parent-first']);
  assert.equal(child.isDisposed, true);
});

test('leak: listeners attached through listen() return to baseline after dispose', () => {
  const target = countingTarget();
  const baseline = target.count();
  const s = createDisposableStore();
  let hits = 0;
  for (let i = 0; i < 25; i++) s.add(listen(target, i % 2 ? 'keydown' : 'resize', () => { hits++; }));
  assert.equal(target.count(), baseline + 25);
  target.dispatch('resize');
  assert.equal(hits, 13);
  s.dispose();
  assert.equal(target.count(), baseline);
  target.dispatch('resize');
  assert.equal(hits, 13);
  code(() => listen(null, 'x', () => {}), 'EDISPOSABLE_INVALID');
});

test('several failing children: every child still cleaned, every error kept in disposal order', () => {
  const cleaned = [];
  const s = createDisposableStore();
  s.add(() => { cleaned.push('a'); throw new Error('a failed'); });
  s.add(() => { cleaned.push('b'); });
  s.add(() => { cleaned.push('c'); throw new Error('c failed'); });
  s.add(() => { cleaned.push('d'); throw new Error('d failed'); });
  let caught = null;
  try { s.dispose(); } catch (e) { caught = e; }
  assert.deepEqual(cleaned, ['d', 'c', 'b', 'a']);
  assert.ok(caught instanceof AggregateError);
  assert.match(caught.message, /^EDISPOSABLE_FAILED/);
  assert.deepEqual(caught.errors.map((e) => e.message), ['d failed', 'c failed', 'a failed']);
  // deterministic afterwards: disposed, empty, and a second dispose is a no-op
  assert.equal(s.isDisposed, true);
  assert.equal(s.size, 0);
  s.dispose();
  assert.deepEqual(cleaned, ['d', 'c', 'b', 'a']);
});
