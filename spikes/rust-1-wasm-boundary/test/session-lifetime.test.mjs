// SPDX-License-Identifier: GPL-3.0-or-later
// RUST-1A Task C: session lifetime, isolation and recovery through the facade.
//
// "Memory" here is WebAssembly linear memory (`memory.buffer.byteLength`). It
// never shrinks, so a test reads it as a HIGH-WATER MARK: growth on a repeat
// of an identical workload is evidence of retention; a flat repeat means the
// freed memory was reused.
//
//   node spikes/rust-1-wasm-boundary/build.mjs
//   node --test spikes/rust-1-wasm-boundary/test/*.test.mjs
//   RUST1A_HEAVY=1 node --max-old-space-size=16000 --test spikes/rust-1-wasm-boundary/test/session-lifetime.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadGlue, loadEngine } from '../loaders/node.mjs';
import { createTextEngine } from '../../../crates/wrlforge-wasm/js/wrlforge-text.mjs';

const PKG = join(dirname(fileURLToPath(import.meta.url)), '..', 'out', 'pkg');
const glue = await loadGlue(PKG, 'lifetime');
const engine = createTextEngine(glue);
// initSync on an initialized module returns the existing exports (memory included).
const wasmMemory = () => glue.initSync({ module: new Uint8Array(0) }).memory.buffer.byteLength;

const code = (fn) => {
  try { fn(); } catch (e) { return e.code; }
  return 'NO-ERROR';
};

test('1. repeated session creation and disposal reuses memory (no growth on repeat)', (t) => {
  const doc = 'DEF a Transform { children [ Shape {} ] }\n'.repeat(500); // ~21 KB
  const cycle = (n) => { for (let i = 0; i < n; i += 1) { const s = engine.openSession(doc); s.current().text(); s.dispose(); } };
  cycle(2000);
  const warm = wasmMemory();
  cycle(5000);
  assert.equal(wasmMemory(), warm, 'linear memory grew on an identical repeat');
  // A refused creation allocates nothing that outlives the call.
  for (let i = 0; i < 5000; i += 1) assert.equal(code(() => engine.openSession(`x\uD800${doc}`)), 'EENCODING');
  assert.equal(wasmMemory(), warm);
  t.diagnostic(`high-water ${warm} bytes after 7000 open/dispose cycles of a ${doc.length}-unit document`);
});

test('2. many sequential revisions: correct; metadata growth measured', (t) => {
  const s = engine.openSession('r0');
  let snap = s.current();
  const first = snap;
  const before = wasmMemory();
  const N = 200000;
  for (let i = 1; i <= N; i += 1) snap = s.update(snap, `r${i % 7}`);
  assert.equal(snap.text(), `r${N % 7}`);
  assert.equal(code(() => first.text()), 'ESESSIONSTALE');
  const grew = wasmMemory() - before;
  t.diagnostic(`${N} revisions on one session: linear memory +${grew} bytes (${(grew / N).toFixed(2)} B/revision; `
    + 'SessionCore.issued keeps one u64 per revision until dispose)');
  // Bounded by the session: after dispose the same workload on a new session reuses it.
  s.dispose();
  const s2 = engine.openSession('r0');
  let p = s2.current();
  const mark = wasmMemory();
  for (let i = 1; i <= N; i += 1) p = s2.update(p, `r${i % 7}`);
  assert.equal(wasmMemory(), mark, 'a disposed session released its revision record');
  s2.dispose();
});

test('3. every stale snapshot refuses, for every operation, after many updates', () => {
  const s = engine.openSession('a\r\nb');
  const old = [s.current()];
  for (let i = 0; i < 50; i += 1) old.push(s.update(old[old.length - 1], `v${i}\r\n`));
  const live = old.pop();
  for (const o of old) {
    for (const fn of [() => o.text(), () => o.length, () => o.toUtf8(0), () => o.fromUtf8(0), () => o.validateSpan(0, 0),
      () => o.lineCol(0), () => o.offsetAt(0, 0), () => o.lineInfo(), () => s.update(o, 'x'), () => s.propose(o, []),
      () => s.verifyTransaction(o, [], '')]) {
      assert.equal(code(fn), 'ESESSIONSTALE');
    }
    assert.equal(typeof o.revision, 'number', 'a stale snapshot still names its revision');
  }
  assert.equal(live.text(), 'v49\r\n');
  s.dispose();
});

test('4. a snapshot of one session is foreign to every other session', () => {
  const docs = ['alpha', 'beta', 'gamma'].map((t) => engine.openSession(t));
  for (const a of docs) {
    for (const b of docs) {
      if (a === b) continue;
      const bs = b.current();
      assert.equal(code(() => a.update(bs, 'x')), 'ESESSIONFOREIGN');
      assert.equal(code(() => a.propose(bs, [])), 'ESESSIONFOREIGN');
      assert.equal(code(() => a.verifyTransaction(bs, [], 'beta')), 'ESESSIONFOREIGN');
    }
  }
  assert.deepEqual(docs.map((d) => d.current().text()), ['alpha', 'beta', 'gamma']);
  docs.forEach((d) => d.dispose());
});

test('5. sessions and snapshots of a separately initialized engine are foreign (raw serials collide)', async () => {
  const other = await loadEngine(PKG, 'lifetime-other');
  const glueB = await loadGlue(PKG, 'lifetime-raw');
  // Fresh instances restart their counters: raw serial/revision numbers collide.
  const r1 = new glueB.RawSession('x');
  const glueC = await loadGlue(PKG, 'lifetime-raw-2');
  const r2 = new glueC.RawSession('y');
  assert.deepEqual([r1.serial(), r1.revision()], [r2.serial(), r2.revision()], 'numbers are per instance');
  r1.free();
  r2.free();
  // The facade does not rely on the numbers: identity is per engine.
  const mine = engine.openSession('mine');
  const theirs = other.openSession('theirs');
  const proto = Object.getPrototypeOf(mine);
  const theirProto = Object.getPrototypeOf(theirs);
  assert.notEqual(proto, theirProto);
  assert.equal(code(() => mine.update(theirs.current(), 'x')), 'ESESSIONFOREIGN');
  assert.equal(code(() => proto.current.call(theirs)), 'ESESSIONFOREIGN');
  assert.equal(code(() => theirProto.current.call(mine)), 'ESESSIONFOREIGN');
  const snapProto = Object.getPrototypeOf(mine.current());
  assert.equal(code(() => Object.getOwnPropertyDescriptor(snapProto, 'length').get.call(theirs.current())), 'ESESSIONFOREIGN');
  assert.equal(theirs.current().text(), 'theirs');
  mine.dispose();
  theirs.dispose();
});

test('6. forged public objects never reach a document', () => {
  const s = engine.openSession('private');
  const snap = s.current();
  const proto = Object.getPrototypeOf(s);
  const snapProto = Object.getPrototypeOf(snap);
  const forgeries = [
    {}, Object.create(proto), Object.create(snapProto), new Proxy(s, {}), new Proxy(snap, {}),
    structuredClone(snap), JSON.parse(JSON.stringify(snap)), { revision: snap.revision }, Object.assign(Object.create(snapProto), {}),
  ];
  for (const f of forgeries) {
    assert.equal(code(() => snapProto.text.call(f)), 'ESESSIONFOREIGN');
    assert.equal(code(() => proto.current.call(f)), 'ESESSIONFOREIGN');
    assert.equal(code(() => s.update(f, 'x')), 'ESESSIONFOREIGN');
  }
  // Frozen: no prototype swap, no added state.
  assert.throws(() => Object.setPrototypeOf(s, {}));
  assert.throws(() => { 'use strict'; s.raw = 1; });
  assert.throws(() => { snapProto.text = () => 'hijack'; });
  assert.equal(snap.text(), 'private');
  s.dispose();
});

test('7. every call after disposal refuses; disposal is idempotent and isolated', () => {
  const s = engine.openSession('gone');
  const keep = engine.openSession('kept');
  const snap = s.current();
  s.dispose();
  s.dispose();
  assert.equal(s.disposed, true);
  for (const fn of [() => s.current(), () => snap.text(), () => snap.length, () => snap.lineInfo(), () => s.update(snap, 'x'),
    () => s.propose(snap, []), () => s.verifyTransaction(snap, [], 'gone')]) {
    assert.equal(code(fn), 'ESESSIONDISPOSED');
  }
  assert.equal(keep.current().text(), 'kept');
  keep.dispose();
});

test('8. a trap poisons the INSTANCE: every engine over it refuses (RUST-1A fix)', async () => {
  const g = await loadGlue(PKG, 'lifetime-poison');
  const healthy = createTextEngine(g);
  const kept = healthy.openSession('kept');
  // A second view of the SAME instance whose apply_edits traps. Spreading the
  // namespace keeps the instance's RawSession class, as any real view does.
  const trapping = createTextEngine({ ...g, apply_edits: () => { throw new WebAssembly.RuntimeError('unreachable'); } });
  assert.equal(code(() => trapping.applyEdits('a', [])), 'EENGINE');
  assert.equal(trapping.poisoned, true);
  assert.equal(healthy.poisoned, true, 'the other engine on the trapped instance is poisoned too');
  assert.equal(code(() => healthy.applyEdits('a', [])), 'EENGINE');
  assert.equal(code(() => kept.current().text()), 'EENGINE');
  assert.equal(code(() => healthy.openSession('new')), 'EENGINE');
  assert.equal(code(() => createTextEngine(g).checkText('a')), 'EENGINE', 'a new engine on a poisoned instance refuses');
  // A separately initialized instance is unaffected.
  const fresh = await loadEngine(PKG, 'lifetime-after-poison');
  assert.equal(fresh.poisoned, false);
  assert.equal(fresh.applyEdits('ab', [{ from: 1, to: 1, insert: 'x' }]), 'axb');
  // The suite's main engine is on a different instance and stays healthy.
  assert.equal(engine.poisoned, false);
});

test('9. a refused creation and a coded refusal never poison or leak', () => {
  for (const bad of [`a\uDC00`, 7, null, undefined, {}]) {
    assert.notEqual(code(() => engine.openSession(bad)), 'NO-ERROR');
  }
  assert.equal(engine.poisoned, false);
  const s = engine.openSession('ok');
  assert.equal(code(() => s.update(s.current(), 'bad\uD800')), 'EENCODING');
  assert.equal(code(() => s.verifyTransaction(s.current(), [{ from: 9, to: 9, insert: '' }], 'ok')), 'EEDITBOUNDS');
  assert.equal(engine.poisoned, false);
  assert.equal(s.current().text(), 'ok');
  s.dispose();
});

test('10. repeated loading of independent instances: each works; module instances are retained by the ESM loader', async (t) => {
  const engines = [];
  for (let i = 0; i < 20; i += 1) {
    const e = await loadEngine(PKG, `lifetime-repeat-${i}`);
    const s = e.openSession(`doc ${i}`);
    assert.equal(s.current().text(), `doc ${i}`);
    s.dispose();
    engines.push(e);
  }
  // Each pair of engines is mutually foreign.
  const a = engines[0].openSession('a');
  assert.equal(code(() => engines[1].openSession('b').update(a.current(), 'x')), 'ESESSIONFOREIGN');
  t.diagnostic('ES module instances cannot be unloaded; a renderer should create ONE instance and reuse it');
});

test('heavy (RUST1A_HEAVY=1): a REAL wasm trap (allocation failure) poisons every engine on the instance', { skip: !process.env.RUST1A_HEAVY }, async () => {
  const g = await loadGlue(PKG, 'lifetime-oom');
  const e1 = createTextEngine(g);
  const e2 = createTextEngine(g);
  const kept = e2.openSession('kept');
  const big = 'x'.repeat(2 ** 28);
  const edits = Array.from({ length: 20 }, () => ({ from: 0, to: 0, insert: big })); // > 4 GiB of wasm32 memory
  const err = (() => { try { e1.applyEdits('', edits); } catch (e) { return e; } return null; })();
  assert.equal(err && err.code, 'EENGINE');
  assert.ok(err.cause instanceof WebAssembly.RuntimeError);
  assert.equal(e2.poisoned, true);
  assert.equal(code(() => kept.current().text()), 'EENGINE');
});
