'use strict';
const { test } = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const store = require('../../src/editor/recovery-store');

function tmpUserData() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-recstore-'));
}

test('makeRecord + save/load round-trip a complete recovery record', () => {
  const ud = tmpUserData();
  const record = store.makeRecord({
    sourcePath: '/tmp/x.wrl',
    context: 'mall',
    profile: 'mall-item',
    root: null,
    format: 'plain',
    baseline: '#VRML V2.0 utf8\n',
    buffer: '#VRML V2.0 utf8\nEdit #1\nEdit #2',
    dirty: true,
    activeWorkspace: 'editor',
    updatedAt: 1700000000000,
  });
  assert.strictEqual(store.saveRecovery(ud, record), true);
  const loaded = store.loadRecovery(ud);
  assert.strictEqual(loaded.ok, true);
  assert.deepStrictEqual(loaded.record, record);
});

test('loadRecovery returns { ok:false, reason } when absent', () => {
  const ud = tmpUserData();
  const r = store.loadRecovery(ud);
  assert.strictEqual(r.ok, false);
  assert.strictEqual(r.reason, 'absent');
});

test('loadRecovery tolerates garbage / bad JSON', () => {
  const ud = tmpUserData();
  fs.writeFileSync(store.recoveryStorePath(ud), 'not json{');
  const r = store.loadRecovery(ud);
  assert.strictEqual(r.ok, false);
  assert.strictEqual(r.reason, 'bad-json');
});

test('loadRecovery rejects a record missing the schemaVersion', () => {
  const ud = tmpUserData();
  fs.writeFileSync(store.recoveryStorePath(ud), JSON.stringify({
    sourcePath: '/x', context: 'mall', profile: 'mall-item',
    format: 'plain', baseline: '', buffer: '', dirty: true, activeWorkspace: 'editor',
  }));
  const r = store.loadRecovery(ud);
  assert.strictEqual(r.ok, false);
  assert.strictEqual(r.reason, 'schema-mismatch');
});

test('loadRecovery rejects a record with a future schemaVersion', () => {
  const ud = tmpUserData();
  fs.writeFileSync(store.recoveryStorePath(ud), JSON.stringify({
    schemaVersion: 999, sourcePath: '/x', context: 'mall', profile: 'mall-item',
    format: 'plain', baseline: '', buffer: '', dirty: true, activeWorkspace: 'editor',
  }));
  const r = store.loadRecovery(ud);
  assert.strictEqual(r.ok, false);
  assert.strictEqual(r.reason, 'schema-mismatch');
});

test('loadRecovery accepts a record with sourcePath = null (unsaved buffer)', () => {
  const ud = tmpUserData();
  const record = store.makeRecord({
    sourcePath: null, context: 'generic', profile: 'generic',
    root: null, format: 'plain', baseline: '', buffer: 'draft that was never saved',
    dirty: true, activeWorkspace: 'editor', updatedAt: 1700000000000,
  });
  store.saveRecovery(ud, record);
  const r = store.loadRecovery(ud);
  assert.strictEqual(r.ok, true);
  assert.strictEqual(r.record.sourcePath, null);
  assert.strictEqual(r.record.buffer, 'draft that was never saved');
});

test('loadRecovery rejects a record whose buffer is not a string', () => {
  const ud = tmpUserData();
  fs.writeFileSync(store.recoveryStorePath(ud), JSON.stringify({
    schemaVersion: 1, sourcePath: '/x', context: 'mall', profile: 'mall-item',
    format: 'plain', baseline: '', buffer: 42, dirty: true, activeWorkspace: 'editor',
  }));
  const r = store.loadRecovery(ud);
  assert.strictEqual(r.ok, false);
  assert.strictEqual(r.reason, 'bad-buffer');
});

test('loadRecovery rejects a record whose activeWorkspace is unknown', () => {
  const ud = tmpUserData();
  fs.writeFileSync(store.recoveryStorePath(ud), JSON.stringify({
    schemaVersion: 1, sourcePath: '/x', context: 'mall', profile: 'mall-item',
    format: 'plain', baseline: '', buffer: '', dirty: true, activeWorkspace: 'inventory',
  }));
  const r = store.loadRecovery(ud);
  assert.strictEqual(r.ok, false);
  assert.strictEqual(r.reason, 'bad-workspace');
});

test('clearRecovery forgets the record', () => {
  const ud = tmpUserData();
  store.saveRecovery(ud, store.makeRecord({
    sourcePath: '/x', context: 'mall', profile: 'mall-item',
    root: null, format: 'plain', baseline: '', buffer: 'd', dirty: true,
  }));
  assert.strictEqual(store.clearRecovery(ud), true);
  assert.strictEqual(store.loadRecovery(ud).ok, false);
  // Idempotent: clearing an absent record is still a success.
  assert.strictEqual(store.clearRecovery(ud), true);
});

test('gzip format values survive a round-trip', () => {
  const ud = tmpUserData();
  const record = store.makeRecord({
    sourcePath: '/x.wrz', context: 'mall', profile: 'mall-item', root: null,
    format: 'gzip', baseline: '#VRML V2.0 utf8\n', buffer: '#VRML V2.0 utf8\nEdit',
    dirty: true, activeWorkspace: 'editor', updatedAt: 1700000000000,
  });
  store.saveRecovery(ud, record);
  const r = store.loadRecovery(ud);
  assert.strictEqual(r.ok, true);
  assert.strictEqual(r.record.format, 'gzip');
});

test('saveRecovery rejects an invalid record shape (does not write)', () => {
  const ud = tmpUserData();
  const bad = store.makeRecord({
    sourcePath: '/x', context: 'mall', profile: 'mall-item',
    root: null, format: 'FLAGRANT_ERROR', baseline: '', buffer: 'd', dirty: true,
  });
  // makeRecord coerces 'FLAGRANT_ERROR' to 'plain', so build a truly bad record.
  const truly = { schemaVersion: 1, sourcePath: '/x', context: 'mall', profile: 'mall-item', root: null, format: 'FLAGRANT', baseline: '', buffer: 'd', dirty: true, activeWorkspace: 'editor' };
  assert.strictEqual(store.saveRecovery(ud, truly), false);
  // The first one WAS valid (coerced) and DID write, so clear it first.
  fs.rmSync(store.recoveryStorePath(ud), { force: true });
  assert.strictEqual(store.saveRecovery(ud, truly), false);
  assert.strictEqual(store.loadRecovery(ud).ok, false);
});

test('validateRecord rejects null and non-objects (case 1)', () => {
  assert.deepStrictEqual(store.validateRecord(null), { ok: false, reason: 'bad-shape' });
  assert.deepStrictEqual(store.validateRecord('s'), { ok: false, reason: 'bad-shape' });
});

test('validateRecord rejects specific field overrides from a valid record (case 2)', () => {
  const validRecord = {
    schemaVersion: 2,
    sourcePath: '/path',
    context: 'mall',
    profile: 'mall-item',
    root: '/root',
    format: 'plain',
    baseline: 'base',
    buffer: 'buff',
    dirty: true,
    activeWorkspace: 'editor',
    updatedAt: 1234,
    sourceStat: { mtimeMs: 1, size: 1, hash: 'h' }
  };

  assert.deepStrictEqual(store.validateRecord({ ...validRecord, context: 1 }), { ok: false, reason: 'bad-context' });
  assert.deepStrictEqual(store.validateRecord({ ...validRecord, profile: null }), { ok: false, reason: 'bad-profile' });
  assert.deepStrictEqual(store.validateRecord({ ...validRecord, sourcePath: 5 }), { ok: false, reason: 'bad-sourcePath' });
  assert.deepStrictEqual(store.validateRecord({ ...validRecord, root: 5 }), { ok: false, reason: 'bad-root' });
  assert.deepStrictEqual(store.validateRecord({ ...validRecord, format: 'zip' }), { ok: false, reason: 'bad-format' });
  assert.deepStrictEqual(store.validateRecord({ ...validRecord, baseline: 1 }), { ok: false, reason: 'bad-baseline' });
  assert.deepStrictEqual(store.validateRecord({ ...validRecord, dirty: 'y' }), { ok: false, reason: 'bad-dirty' });
});

test('validateRecord handles sourceStat and schemaVersion variants (case 3)', () => {
  const validRecord = {
    schemaVersion: 2,
    sourcePath: '/path',
    context: 'mall',
    profile: 'mall-item',
    root: '/root',
    format: 'plain',
    baseline: 'base',
    buffer: 'buff',
    dirty: true,
    activeWorkspace: 'editor',
    updatedAt: 1234
  };

  assert.deepStrictEqual(store.validateRecord({ ...validRecord, sourceStat: 'x' }), { ok: false, reason: 'bad-sourceStat' });
  assert.deepStrictEqual(store.validateRecord({ ...validRecord, sourceStat: { mtimeMs: 1, size: '1', hash: 'h' } }), { ok: false, reason: 'bad-sourceStat' });
  assert.deepStrictEqual(store.validateRecord({ ...validRecord, sourceStat: { mtimeMs: 1, size: 1, hash: 'h' } }), { ok: true });

  // older schema version 1 is ok
  assert.deepStrictEqual(store.validateRecord({ ...validRecord, schemaVersion: 1 }), { ok: true });
});

test('makeRecord sanitization and loadRecovery reporting of damaged record (case 4)', () => {
  // makeRecord sanitizing
  const record = store.makeRecord({
    sourceStat: { mtimeMs: '12', size: 3, hash: 'ab' }
  });
  assert.deepStrictEqual(record.sourceStat, { mtimeMs: 12, size: 3, hash: 'ab' });

  const recordNoHash = store.makeRecord({
    sourceStat: { mtimeMs: 12, size: 3, hash: '' }
  });
  assert.strictEqual(recordNoHash.sourceStat, null);

  // Bogus activeWorkspace, format, buffer, and sourcePath values
  const bogusRecord = store.makeRecord({
    activeWorkspace: 'invalid-workspace',
    format: 'bogus-format',
    buffer: null,
    sourcePath: null
  });
  assert.strictEqual(bogusRecord.activeWorkspace, 'editor');
  assert.strictEqual(bogusRecord.format, 'plain');
  assert.strictEqual(bogusRecord.buffer, '');
  assert.strictEqual(bogusRecord.sourcePath, null);

  // A damaged record written to a temp dir makes loadRecovery report that reason
  const ud = tmpUserData();
  fs.writeFileSync(store.recoveryStorePath(ud), JSON.stringify({
    ...bogusRecord,
    schemaVersion: 2,
    context: 123 // invalid type to trigger a reason
  }));
  const r = store.loadRecovery(ud);
  assert.strictEqual(r.ok, false);
  assert.strictEqual(r.reason, 'bad-context');
});
