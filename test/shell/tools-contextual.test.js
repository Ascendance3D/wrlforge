'use strict';
// SHELL-0 tool and contextual-panel contribution boundaries
// (src/shell/tool-registry.js, src/shell/contextual-panels.js).
const test = require('node:test');
const assert = require('node:assert/strict');
const { createToolRegistry, TOOL_GROUPS } = require('../../src/shell/tool-registry');
const { createContextualPanelHost } = require('../../src/shell/contextual-panels');
const { createApplication } = require('../../src/shell/contribution');
const { realServices, createSelectionController } = require('./fakes');

const code = (fn, c) => assert.throws(fn, (e) => e.code === c, `expected ${c}`);
const cmd = (id, extra = {}) => ({ id, area: id.split('.')[0], label: id, run: () => id, ...extra });

function toolWorld({ profile = 'world', sel = null } = {}) {
  const s = realServices({ profile });
  const ctx = { sel };
  const tools = createToolRegistry({ commands: s.commands, getProfile: () => s.ctx.profile, getSelectionInfo: () => ctx.sel });
  return { ...s, ctx, tools };
}

test('tools: the §18 groups are fixed and every example tool fits one', () => {
  assert.deepEqual([...TOOL_GROUPS], ['select', 'transform', 'create', 'appearance', 'hierarchy', 'behavior', 'animation']);
  const { commands, tools } = toolWorld();
  const examples = {
    select: ['Select'], transform: ['Move', 'Rotate', 'Scale'],
    create: ['Box', 'Sphere', 'Cone', 'Cylinder', 'Extrusion', 'IndexedFaceSet'],
    appearance: ['Material', 'Texture'], hierarchy: ['Group', 'Ungroup'], behavior: ['Route'], animation: ['Keyframe'],
  };
  for (const [group, names] of Object.entries(examples)) {
    for (const n of names) {
      const id = n[0].toLowerCase() + n.slice(1);
      commands.register(cmd(`model.${id}`));
      tools.register({ id: `tool.${id}`, commandId: `model.${id}`, group });
    }
  }
  assert.deepEqual(tools.list('transform').map((t) => t.id), ['tool.move', 'tool.rotate', 'tool.scale']);
  assert.equal(tools.list().length, 16);
  assert.equal(tools.get('tool.box').label, 'model.box'); // falls back to the command label
});

test('tools: record validation fails closed (command must exist)', () => {
  const { commands, tools } = toolWorld();
  commands.register(cmd('model.move'));
  code(() => tools.register({ id: 'tool.move', commandId: 'model.nothing', group: 'transform' }), 'ETOOL_INVALID');
  code(() => tools.register({ id: 'move', commandId: 'model.move', group: 'transform' }), 'ETOOL_INVALID');
  code(() => tools.register({ id: 'tool.move', commandId: 'model.move', group: 'gizmos' }), 'ETOOL_INVALID');
  code(() => tools.register({ id: 'tool.move', commandId: 'model.move', group: 'transform', profiles: ['x'] }), 'EPROFILE_INVALID');
  tools.register({ id: 'tool.move', commandId: 'model.move', group: 'transform' });
  code(() => tools.register({ id: 'tool.move', commandId: 'model.move', group: 'transform' }), 'ETOOL_DUPLICATE');
});

test('tools: state is derived from the command + profile + PROVEN selection', () => {
  const { commands, tools, ctx, ctx: c, commandRegistry } = toolWorld({ profile: 'world' });
  let enabled = true;
  let ran = 0;
  commands.register(cmd('model.move', { enabled: () => enabled, run: () => { ran++; } }));
  tools.register({ id: 'tool.move', commandId: 'model.move', group: 'transform', profiles: ['world', 'generic'],
    appliesTo: (sel) => sel.type === 'Transform' });
  // no selection -> unavailable (fail closed)
  assert.deepEqual({ ...tools.state('tool.move') }, { available: false, enabled: false, checked: null });
  assert.deepEqual(tools.activate('tool.move'), { ok: false, reason: 'unavailable' });
  // unproven selection -> unavailable even when appliesTo would say yes
  c.sel = { type: 'Transform', proven: false };
  assert.equal(tools.state('tool.move').available, false);
  c.sel = { type: 'Transform', proven: true };
  assert.deepEqual({ ...tools.state('tool.move') }, { available: true, enabled: true, checked: null });
  tools.activate('tool.move');
  assert.equal(ran, 1);
  // enabled is the command's, not the tool's
  enabled = false;
  assert.equal(tools.state('tool.move').enabled, false);
  assert.deepEqual(tools.activate('tool.move'), { ok: false, reason: 'disabled' });
  // out of profile -> unavailable
  enabled = true;
  ctx.sel = { type: 'Transform', proven: true };
  const s2 = toolWorld({ profile: 'mall', sel: { type: 'Transform', proven: true } });
  s2.commands.register(cmd('model.move'));
  s2.tools.register({ id: 'tool.move', commandId: 'model.move', group: 'transform', profiles: ['world'] });
  assert.equal(s2.tools.state('tool.move').available, false);
  // command unregistered later -> unavailable, never a dangling run
  commandRegistry.dispose();
  assert.equal(tools.state('tool.move').available, false);
});

test('tools: a throwing appliesTo is unavailable, not a crash', () => {
  const { commands, tools } = toolWorld({ sel: { proven: true } });
  commands.register(cmd('model.box'));
  tools.register({ id: 'tool.box', commandId: 'model.box', group: 'create', appliesTo: () => { throw new Error('bad'); } });
  const orig = console.error; console.error = () => {};
  try { assert.equal(tools.state('tool.box').available, false); } finally { console.error = orig; }
});

test('tools: set-change subscription and contribution cleanup', () => {
  const s = realServices({ profile: 'world' });
  const tools = createToolRegistry({ commands: s.commands, getProfile: () => 'world', getSelectionInfo: () => null });
  const a = createApplication({ services: { commands: s.commands, panels: s.panels, tools } });
  let changes = 0;
  const base = tools.listenerCount();
  const h = a.contribute({
    id: 'transform.tools',
    contribute(v) {
      v.tools.subscribe(() => { changes++; });
      v.commands.register(cmd('model.move'));
      v.tools.register({ id: 'tool.move', commandId: 'model.move', group: 'transform' });
    },
  });
  assert.equal(changes, 1);
  assert.equal(tools.listenerCount(), base + 1);
  h.dispose();
  assert.equal(tools.has('tool.move'), false);
  assert.equal(s.commandRegistry.has('model.move'), false);
  assert.equal(tools.listenerCount(), base);
});

function contextualWorld() {
  const selection = createSelectionController();
  const items = new Map([
    ['t1', { id: 't1', type: 'Transform', proven: true }],
    ['e1', { id: 'e1', type: 'Extrusion', proven: true }],
    ['u1', { id: 'u1', type: 'Transform', proven: false }],
  ]);
  const analysis = { version: 1 };
  const hosts = [];
  const released = [];
  const host = createContextualPanelHost({
    // the document session's EXISTING authorities: selection id + analysis lookup
    resolveContext: () => {
      const id = selection.getSelection();
      return { selection: id ? items.get(id) || null : null, analysis };
    },
    createHost: (id) => { const h = { id }; hosts.push(h); return h; },
    releaseHost: (id) => released.push(id),
  });
  return { selection, host, hosts, released, analysis };
}

function editor(id, type, log) {
  return {
    id, title: id,
    appliesTo: (sel) => sel.type === type,
    mount: (h, ctx) => log.push(`mount:${id}:${ctx.selection.id}`),
    update: (ctx) => log.push(`update:${id}:${ctx.selection.id}`),
    dispose: () => log.push(`dispose:${id}`),
  };
}

test('contextual panels: applies to the PROVEN selection; mount, update, dispose', () => {
  const { selection, host, released } = contextualWorld();
  const log = [];
  host.register(editor('transformEditor', 'Transform', log));
  host.register(editor('extrusionEditor', 'Extrusion', log));
  const unwatch = host.watch(selection.subscribe);
  selection.setSelection('t1');
  assert.deepEqual(host.mounted(), ['transformEditor']);
  selection.setSelection('e1');
  assert.deepEqual(host.mounted(), ['extrusionEditor']);
  host.reconcile(); // e.g. a new analysis with the same selection
  // unproven identity -> nothing applies (fail closed)
  selection.setSelection('u1');
  assert.deepEqual(host.mounted(), []);
  selection.clearSelection();
  assert.deepEqual(log, [
    'mount:transformEditor:t1',
    'dispose:transformEditor', 'mount:extrusionEditor:e1',
    'update:extrusionEditor:e1',
    'dispose:extrusionEditor',
  ]);
  assert.deepEqual(released, ['transformEditor', 'extrusionEditor']);
  const before = selection.listenerCount();
  unwatch(); unwatch();
  assert.equal(selection.listenerCount(), before - 1);
});

test('contextual panels: a failing mount or appliesTo is contained', () => {
  const { selection, host, released } = contextualWorld();
  const orig = console.error; console.error = () => {};
  try {
    host.register({ id: 'flaky', title: 'Flaky', appliesTo: () => true, mount() { throw new Error('x'); }, dispose() { throw new Error('never'); } });
    host.register({ id: 'confused', title: 'Confused', appliesTo: () => { throw new Error('y'); }, mount() {}, dispose() {} });
    selection.setSelection('t1');
    const r = host.reconcile();
    assert.deepEqual(r.failed, ['flaky']);
    assert.deepEqual(host.mounted(), []);
    assert.deepEqual(released, ['flaky']);
  } finally { console.error = orig; }
});

test('contextual panels: validation, unregister and dispose', () => {
  const { selection, host } = contextualWorld();
  code(() => host.register({ id: 'x', title: 'X', appliesTo() {}, mount() {} }), 'ECONTEXTUAL_INVALID');
  code(() => host.register({ id: 'Bad id', title: 'X', appliesTo() {}, mount() {}, dispose() {} }), 'ECONTEXTUAL_INVALID');
  const log = [];
  const un = host.register(editor('transformEditor', 'Transform', log));
  code(() => host.register(editor('transformEditor', 'Transform', log)), 'ECONTEXTUAL_DUPLICATE');
  selection.setSelection('t1');
  host.reconcile();
  un();
  assert.deepEqual(host.mounted(), []);
  host.register(editor('again', 'Transform', log));
  host.reconcile();
  host.dispose();
  assert.deepEqual(log, ['mount:transformEditor:t1', 'dispose:transformEditor', 'mount:again:t1', 'dispose:again']);
  code(() => createContextualPanelHost({ createHost() {} }), 'ECONTEXTUAL_INVALID');
});

test('contextual panels: owner lifecycle none -> match -> update -> non-match -> match again (fresh mount)', () => {
  const { createDisposableStore } = require('../../src/shell/disposable');
  const { selection, host, hosts, released } = contextualWorld();
  const log = [];
  let subs = 0; // live subscriptions held by the CURRENT activation
  const seenHosts = [];
  let live = null;
  host.register({
    id: 'transformEditor', title: 'Transform',
    appliesTo: (sel) => sel.type === 'Transform',
    mount(h, ctx) {
      assert.equal(live, null, 'no resources carried over from a prior activation');
      seenHosts.push(h);
      live = createDisposableStore();
      subs++; live.add(() => { subs--; });
      log.push(`mount:${ctx.selection.id}`);
    },
    update(ctx) { log.push(`update:${ctx.selection.id}`); },
    dispose() { live.dispose(); live = null; log.push('dispose'); },
  });
  const unwatch = host.watch(selection.subscribe);
  assert.deepEqual(host.mounted(), []); // no applicable selection
  selection.setSelection('t1');
  assert.deepEqual(host.mounted(), ['transformEditor']);
  assert.equal(subs, 1);
  host.reconcile(); // selection/analysis update while applicable
  selection.setSelection('e1'); // non-matching
  assert.deepEqual(host.mounted(), []);
  assert.equal(subs, 0);
  selection.setSelection('u1'); // matching type but UNPROVEN -> nothing
  assert.deepEqual(host.mounted(), []);
  selection.setSelection('t1'); // matching again -> a second, fresh activation
  assert.deepEqual(host.mounted(), ['transformEditor']);
  assert.equal(subs, 1);
  assert.equal(seenHosts.length, 2);
  assert.notEqual(seenHosts[0], seenHosts[1], 'a fresh host per activation');
  assert.deepEqual(hosts.length, 2);
  host.dispose();
  assert.equal(subs, 0);
  assert.deepEqual(log, ['mount:t1', 'update:t1', 'dispose', 'mount:t1', 'dispose']);
  assert.deepEqual(released, ['transformEditor', 'transformEditor']);
  unwatch();
});

test('contextual panels: appliesTo failure after mount tears down; mount failure retries cleanly', () => {
  const { selection, host } = contextualWorld();
  const orig = console.error; console.error = () => {};
  try {
    let appliesThrows = false;
    let mountThrows = true;
    const log = [];
    host.register({
      id: 'transformEditor', title: 'Transform',
      appliesTo: (sel) => { if (appliesThrows) throw new Error('applies'); return sel.type === 'Transform'; },
      mount() { if (mountThrows) throw new Error('mount'); log.push('mount'); },
      dispose() { log.push('dispose'); },
    });
    selection.setSelection('t1');
    assert.deepEqual(host.reconcile().failed, ['transformEditor']);
    assert.deepEqual(host.mounted(), []); // no stale active state after a failed mount
    mountThrows = false;
    assert.deepEqual(host.reconcile().mounted, ['transformEditor']); // retry is a clean mount
    appliesThrows = true; // a throwing appliesTo means "does not apply"
    assert.deepEqual(host.reconcile().disposed, ['transformEditor']);
    assert.deepEqual(host.mounted(), []);
    assert.deepEqual(log, ['mount', 'dispose']);
  } finally { console.error = orig; }
});

test('contextual panels: a throwing update or dispose never stops the others reconciling', () => {
  const { selection, host, released } = contextualWorld();
  const orig = console.error; console.error = () => {};
  try {
    const log = [];
    host.register({ id: 'badUpdate', title: 'A', appliesTo: (s) => s.type === 'Transform',
      mount() { log.push('mount:A'); }, update() { throw new Error('update'); }, dispose() { log.push('dispose:A'); } });
    host.register({ id: 'badDispose', title: 'B', appliesTo: (s) => s.type === 'Transform',
      mount() { log.push('mount:B'); }, dispose() { throw new Error('dispose'); } });
    host.register(editor('good', 'Transform', log));
    selection.setSelection('t1');
    host.reconcile();
    assert.deepEqual(host.mounted(), ['badUpdate', 'badDispose', 'good']);
    const r = host.reconcile(); // update: A throws -> disposed (fail closed); good still updated
    assert.deepEqual(r.failed, ['badUpdate']);
    assert.deepEqual(r.updated, ['good']);
    assert.deepEqual(host.mounted(), ['badDispose', 'good']);
    selection.setSelection('e1'); // B's dispose throws; good is still disposed
    const r2 = host.reconcile();
    assert.deepEqual(r2.failed, ['badDispose']);
    assert.deepEqual(r2.disposed, ['good']);
    assert.deepEqual(host.mounted(), [], 'a throwing dispose leaves no stale active state');
    assert.deepEqual(released, ['badUpdate', 'badDispose', 'good']);
    assert.deepEqual(log, ['mount:A', 'mount:B', 'mount:good:t1', 'dispose:A', 'update:good:t1', 'dispose:good']);
  } finally { console.error = orig; }
});

test('contextual panels: dispose() disposes every editor even when several throw', () => {
  const { selection, host, released } = contextualWorld();
  const log = [];
  const bad = (id) => ({ id, title: id, appliesTo: () => true, mount() {}, dispose() { log.push(id); throw new Error(id); } });
  host.register(bad('one'));
  host.register(editor('good', 'Transform', log));
  host.register(bad('two'));
  selection.setSelection('t1');
  host.reconcile();
  assert.throws(() => host.dispose(), (e) => e instanceof AggregateError && e.errors.map((x) => x.message).join() === 'two,one');
  assert.deepEqual(log, ['mount:good:t1', 'two', 'dispose:good', 'one']);
  assert.deepEqual(host.mounted(), []);
  assert.deepEqual(released, ['two', 'good', 'one']);
});
