'use strict';
// SHELL-0 command and panel contribution boundaries
// (src/shell/command-service.js, src/shell/panel-service.js) over the REAL
// UI-0 registries -- proving they extend, not replace, them.
const test = require('node:test');
const assert = require('node:assert/strict');
const { createApplication } = require('../../src/shell/contribution');
const { realServices, fakeElement } = require('./fakes');

const code = (fn, c) => assert.throws(fn, (e) => e.code === c, `expected ${c}`);
const cmd = (id, extra = {}) => ({ id, area: id.split('.')[0], label: id, run: () => id, ...extra });

test('commands: profile-neutral records pass to the registry unchanged', () => {
  const { commands, commandRegistry } = realServices();
  const un = commands.register(cmd('file.save', { keys: ['Mod+S'] }));
  assert.ok(commandRegistry.has('file.save'));
  assert.deepEqual(commandRegistry.get('file.save').keys, ['Mod+s']);
  assert.equal(commands.profilesOf('file.save'), null);
  assert.equal(commands.isEnabled('file.save'), true);
  un();
  assert.equal(commandRegistry.has('file.save'), false);
});

test('commands: `profiles` is data folded into the registry\'s own enabled/execute', () => {
  const { ctx, commands, commandRegistry } = realServices({ profile: null });
  let ran = 0;
  let own = true;
  commands.register(cmd('file.repack', { profiles: ['mall'], enabled: () => own, run: () => { ran++; } }));
  assert.deepEqual(commands.profilesOf('file.repack'), ['mall']);
  // stored in the ONE registry; the descriptor carries no profile field
  assert.deepEqual(Object.keys(commandRegistry.get('file.repack')).sort(), ['area', 'id', 'isToggle', 'keyOwner', 'keys', 'label']);
  // no document -> disabled; execute refuses through the registry
  assert.equal(commandRegistry.isEnabled('file.repack'), false);
  assert.deepEqual(commandRegistry.execute('file.repack'), { ok: false, reason: 'disabled' });
  ctx.profile = 'world';
  assert.equal(commandRegistry.isEnabled('file.repack'), false);
  ctx.profile = 'mall';
  assert.equal(commandRegistry.isEnabled('file.repack'), true);
  commandRegistry.execute('file.repack');
  assert.equal(ran, 1);
  own = false; // the record's own enabled still applies inside the profile
  assert.equal(commandRegistry.isEnabled('file.repack'), false);
});

test('commands: invalid profiles refused; ids and duplicates still the registry\'s rules', () => {
  const { commands } = realServices();
  for (const profiles of [[], ['cybertown'], ['mall', 'mall'], 'mall']) {
    code(() => commands.register(cmd('model.box', { profiles })), 'EPROFILE_INVALID');
  }
  code(() => commands.register(cmd('Not.valid', { profiles: ['world'] })), 'ECOMMAND_INVALID');
  commands.register(cmd('model.box', { profiles: ['world', 'generic'] }));
  code(() => commands.register(cmd('model.box', { profiles: ['world'] })), 'ECOMMAND_DUPLICATE');
  assert.deepEqual(commands.profilesOf('model.box'), ['world', 'generic']);
});

test('commands: a contribution\'s profiled command is removed with the contribution', () => {
  const { commands, panels, commandRegistry } = realServices({ profile: 'world' });
  const a = createApplication({ services: { commands, panels } });
  const h = a.contribute({ id: 'world.packaging', contribute(v) { v.commands.register(cmd('file.package', { profiles: ['world'] })); } });
  assert.equal(commandRegistry.isEnabled('file.package'), true);
  h.dispose();
  assert.equal(commandRegistry.has('file.package'), false);
  assert.equal(commands.profilesOf('file.package'), null);
});

test('panels: element() records pass through to the UI-0 registry unchanged', () => {
  const { panels, panelRegistry } = realServices();
  const el = fakeElement();
  panels.register({ id: 'outline', title: 'Outline', element: () => el });
  assert.ok(panelRegistry.isVisible('outline'));
  assert.deepEqual(panels.focus('outline'), { ok: true });
  code(() => panels.mount('outline', fakeElement()), 'EPANEL_NOT_MOUNTABLE');
});

test('panels: mount(host) / dispose() lifecycle, derived visibility, focus through the registry', () => {
  const { panels, panelRegistry } = realServices();
  const calls = [];
  panels.register({
    id: 'context', title: 'Context',
    mount(host) { calls.push(['mount', host]); },
    dispose() { calls.push(['dispose']); },
    resize() { calls.push(['resize']); },
  });
  // unmounted: registered, not visible, focus never opens it
  assert.ok(panelRegistry.has('context'));
  assert.equal(panels.isVisible('context'), false);
  assert.deepEqual(panels.focus('context'), { ok: false, reason: 'hidden' });
  panels.notifyResize('context'); // ignored while unmounted

  const host = fakeElement();
  assert.deepEqual(panels.mount('context', host), { ok: true });
  assert.deepEqual(panels.mount('context', host), { ok: true, already: true });
  code(() => panels.mount('context', fakeElement()), 'EPANEL_MOUNTED');
  assert.ok(panels.isMounted('context'));
  assert.equal(panels.isVisible('context'), true);
  assert.deepEqual(panels.focus('context'), { ok: true });
  assert.equal(host.state.focused, 1);

  // hidden-but-docked stays mounted: visibility changes, no remount
  host.state.visible = false;
  assert.equal(panels.isVisible('context'), false);
  assert.ok(panels.isMounted('context'));
  panels.notifyResize('context');

  assert.deepEqual(panels.unmount('context'), { ok: true });
  assert.deepEqual(panels.unmount('context'), { ok: true, already: true });
  assert.deepEqual(calls.map((c) => c[0]), ['mount', 'resize', 'dispose']);
  assert.equal(calls[0][1], host);
});

test('panels: unregistering a mounted panel disposes it; a throwing mount stays unmounted', () => {
  const { panels, panelRegistry } = realServices();
  let disposed = 0;
  const un = panels.register({ id: 'tools', title: 'Tools', mount() {}, dispose() { disposed++; } });
  panels.mount('tools', fakeElement());
  un(); un();
  assert.equal(disposed, 1);
  assert.equal(panelRegistry.has('tools'), false);

  panels.register({ id: 'flaky', title: 'Flaky', mount() { throw new Error('boom'); }, dispose() { disposed++; } });
  assert.throws(() => panels.mount('flaky', fakeElement()), /boom/);
  assert.equal(panels.isMounted('flaky'), false);
  assert.equal(disposed, 1);
});

test('panels: mountable record validation', () => {
  const { panels } = realServices();
  code(() => panels.register({ id: 'a', title: 'A', mount() {} }), 'EPANEL_INVALID');
  code(() => panels.register({ id: 'b', title: 'B', mount() {}, dispose() {}, element: () => null }), 'EPANEL_INVALID');
  code(() => panels.register({ id: 'c', title: 'C', mount: 1, dispose() {} }), 'EPANEL_INVALID');
  code(() => panels.register({ id: 'console', title: 'Console', mount() {}, dispose() {} }), 'EPANEL_INVALID'); // reserved id: registry rule
  code(() => panels.mount('nope', fakeElement()), 'EPANEL_UNKNOWN');
});

test('panels: contribution disposal unmounts and unregisters', () => {
  const { commands, panels, panelRegistry } = realServices();
  const a = createApplication({ services: { commands, panels } });
  let disposed = 0;
  const h = a.contribute({ id: 'context.panel', contribute(v) { v.panels.register({ id: 'context', title: 'Context', mount() {}, dispose() { disposed++; } }); } });
  panels.mount('context', fakeElement());
  h.dispose();
  assert.equal(disposed, 1);
  assert.equal(panelRegistry.has('context'), false);
});

// A host stand-in that counts its live listeners (leak detection).
function countingHost() {
  const el = fakeElement();
  const listeners = new Set();
  el.addEventListener = (type, fn) => { listeners.add(fn); };
  el.removeEventListener = (type, fn) => { listeners.delete(fn); };
  el.listenerCount = () => listeners.size;
  return el;
}

test('panels: owner lifecycle register -> mount -> hide/show docked -> unmount -> remount -> unmount -> unregister', () => {
  const { createDisposableStore, listen } = require('../../src/shell/disposable');
  const { panels, panelRegistry } = realServices();
  const log = [];
  let live = null; // the CURRENT mount's resources; never reused across mounts
  let generation = 0;
  let current = null;
  const un = panels.register({
    id: 'context', title: 'Context', canToggle: true,
    show() { current.state.visible = true; log.push('show'); },
    hide() { current.state.visible = false; log.push('hide'); },
    focus() { log.push('focus'); },
    mount(host) {
      assert.equal(live, null, 'previous mount resources must be gone');
      generation++;
      current = host;
      live = createDisposableStore();
      live.add(listen(host, 'keydown', () => {}));
      live.add(listen(host, 'focusin', () => {}));
      log.push(`mount:${generation}`);
    },
    dispose() { live.dispose(); live = null; log.push(`dispose:${generation}`); },
    resize() { log.push('resize'); },
  });
  const a = countingHost();
  panels.mount('context', a);
  assert.equal(a.listenerCount(), 2);
  assert.equal(panels.isVisible('context'), true);
  assert.deepEqual(panels.focus('context'), { ok: true });

  // hidden while docked: stays mounted, no remount, focus refuses
  assert.deepEqual(panels.hide('context'), { ok: true });
  assert.ok(panels.isMounted('context'));
  assert.equal(panels.isVisible('context'), false);
  assert.deepEqual(panels.focus('context'), { ok: false, reason: 'hidden' });
  panels.notifyResize('context');
  assert.deepEqual(panels.show('context'), { ok: true });
  assert.deepEqual(panels.focus('context'), { ok: true });
  assert.deepEqual(panels.mount('context', a), { ok: true, already: true });

  // a host change while mounted is still refused
  code(() => panels.mount('context', countingHost()), 'EPANEL_MOUNTED');

  // unmount releases every listener; repeated unmount is safe
  panels.unmount('context');
  panels.unmount('context');
  assert.equal(a.listenerCount(), 0);
  assert.equal(panels.isMounted('context'), false);
  assert.equal(panels.isVisible('context'), false);
  assert.ok(panelRegistry.has('context'), 'a disposed mount does not spend the registration');

  // remount into a DIFFERENT host is defined: a fresh mount, fresh resources
  const b = countingHost();
  assert.deepEqual(panels.mount('context', b), { ok: true });
  assert.equal(b.listenerCount(), 2);
  assert.equal(a.listenerCount(), 0, 'nothing re-attached to the old host');
  panels.unmount('context');
  assert.equal(b.listenerCount(), 0);

  // and the same host again after an unmount is also a fresh mount
  panels.mount('context', b);
  assert.equal(b.listenerCount(), 2);

  // unregister while mounted disposes; repeated unregister is safe
  un(); un();
  assert.equal(b.listenerCount(), 0);
  assert.equal(panelRegistry.has('context'), false);
  code(() => panels.mount('context', b), 'EPANEL_UNKNOWN');
  assert.deepEqual(log, [
    'mount:1', 'focus', 'hide', 'resize', 'show', 'focus', 'dispose:1',
    'mount:2', 'dispose:2', 'mount:3', 'dispose:3',
  ]);
});

test('panels: a throwing dispose still leaves the panel unmounted and remountable', () => {
  const { panels } = realServices();
  let fail = true;
  let mounts = 0;
  panels.register({ id: 'context', title: 'Context', mount() { mounts++; }, dispose() { if (fail) throw new Error('dispose boom'); } });
  panels.mount('context', fakeElement());
  assert.throws(() => panels.unmount('context'), /dispose boom/);
  assert.equal(panels.isMounted('context'), false);
  fail = false;
  panels.mount('context', fakeElement());
  assert.equal(mounts, 2);
  assert.deepEqual(panels.unmount('context'), { ok: true });
});

test('panels: a failed mount can be retried', () => {
  const { panels } = realServices();
  let fail = true;
  let disposed = 0;
  panels.register({ id: 'context', title: 'Context', mount() { if (fail) throw new Error('mount boom'); }, dispose() { disposed++; } });
  const h = fakeElement();
  assert.throws(() => panels.mount('context', h), /mount boom/);
  assert.equal(panels.isMounted('context'), false);
  assert.equal(disposed, 0, 'no dispose for a mount that never completed');
  fail = false;
  assert.deepEqual(panels.mount('context', h), { ok: true });
  panels.unmount('context');
  assert.equal(disposed, 1);
});
