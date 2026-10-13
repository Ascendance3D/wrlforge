'use strict';
// SHELL-0 application contribution contract (src/shell/contribution.js) and
// service boundary (src/shell/services.js), against the REAL UI-0 registries.
const test = require('node:test');
const assert = require('node:assert/strict');
const { createApplication } = require('../../src/shell/contribution');
const { SERVICE_CATALOG, DEFERRED_SERVICES } = require('../../src/shell/services');
const { createDocumentSlot } = require('../../src/shell/document-session');
const { createToolRegistry } = require('../../src/shell/tool-registry');
const { listen } = require('../../src/shell/disposable');
const { realServices, fakeElement, createSelectionController } = require('./fakes');

const code = (fn, c) => assert.throws(fn, (e) => e.code === c, `expected ${c}`);
const cmd = (id, extra = {}) => ({ id, area: id.split('.')[0], label: id, run: () => id, ...extra });

function app(extra = {}) {
  const s = realServices();
  const a = createApplication({ services: { commands: s.commands, panels: s.panels, ...extra } });
  return { ...s, a };
}

test('a contribution registers through app.* and its registrations are live', () => {
  const { a, commandRegistry, panelRegistry } = app();
  const h = a.contribute({
    id: 'workspace.commands',
    contribute(appView) {
      assert.equal(appView.contributionId, 'workspace.commands');
      appView.commands.register(cmd('workspace.reset'));
      appView.panels.register({ id: 'objectPanel', title: 'Object', element: () => fakeElement() });
    },
  });
  assert.equal(h.id, 'workspace.commands');
  assert.ok(commandRegistry.has('workspace.reset'));
  assert.ok(panelRegistry.has('objectPanel'));
  assert.deepEqual(a.list(), ['workspace.commands']);
  assert.equal(a.ownedCount('workspace.commands'), 2);
});

test('disposing a contribution removes everything it registered, once', () => {
  const { a, commandRegistry, panelRegistry } = app();
  const h = a.contribute({
    id: 'panel.commands',
    contribute(appView) {
      appView.commands.register(cmd('panel.focusOutline'));
      appView.commands.register(cmd('panel.focusPreview'));
      appView.panels.register({ id: 'outline', title: 'Outline', element: () => fakeElement() });
    },
  });
  h.dispose();
  h.dispose();
  assert.equal(commandRegistry.has('panel.focusOutline'), false);
  assert.equal(commandRegistry.has('panel.focusPreview'), false);
  assert.equal(panelRegistry.has('outline'), false);
  assert.equal(a.has('panel.commands'), false);
  // the same id may contribute again after disposal
  a.contribute({ id: 'panel.commands', contribute(v) { v.commands.register(cmd('panel.focusOutline')); } });
  assert.ok(commandRegistry.has('panel.focusOutline'));
});

test('retained handle: calling it early releases the registration from the store too', () => {
  const { a, commandRegistry } = app();
  let early;
  a.contribute({
    id: 'feature.x',
    contribute(v) {
      early = v.commands.register(cmd('view.toggleThing'));
      v.commands.register(cmd('view.other'));
    },
  });
  assert.equal(typeof early, 'function');
  early();
  assert.equal(commandRegistry.has('view.toggleThing'), false);
  assert.equal(a.ownedCount('feature.x'), 1);
  // a later re-register by someone else is NOT removed by disposing feature.x
  commandRegistry.register(cmd('view.toggleThing'));
  a.dispose();
  assert.ok(commandRegistry.has('view.toggleThing'));
  assert.equal(commandRegistry.has('view.other'), false);
});

test('unretained handles are still owned: a contribution cannot leak a subscription', () => {
  const selection = createSelectionController();
  const prefsListeners = new Set();
  const preferences = {
    get: () => ({}), set: () => {},
    subscribe(fn) { prefsListeners.add(fn); return () => prefsListeners.delete(fn); },
  };
  const { a, commandRegistry } = app({ preferences });
  const target = { n: 0, addEventListener() { this.n++; }, removeEventListener() { this.n--; } };
  const baseline = { sel: selection.listenerCount(), prefs: prefsListeners.size, dom: target.n };
  const handles = [];
  for (let i = 0; i < 25; i++) {
    handles.push(a.contribute({
      id: `feature.f${i}`,
      contribute(v) {
        v.commands.subscribe(() => {});        // handle discarded on purpose
        v.preferences.subscribe(() => {});     // handle discarded on purpose
        v.disposables.add(selection.subscribe(() => {}));
        v.disposables.add(listen(target, 'resize', () => {}));
        v.commands.register(cmd(`view.f${i}`));
      },
    }));
  }
  assert.equal(selection.listenerCount(), baseline.sel + 25);
  assert.equal(prefsListeners.size, baseline.prefs + 25);
  assert.equal(target.n, baseline.dom + 25);
  for (const h of handles) h.dispose();
  assert.equal(selection.listenerCount(), baseline.sel);
  assert.equal(prefsListeners.size, baseline.prefs);
  assert.equal(target.n, baseline.dom);
  assert.equal(commandRegistry.list().length, 0);
});

test('a returned function/disposable is owned too', () => {
  const { a } = app();
  let n = 0;
  const h1 = a.contribute({ id: 'a.one', contribute: () => () => { n++; } });
  const h2 = a.contribute({ id: 'a.two', contribute: () => ({ dispose() { n++; } }) });
  h1.dispose(); h2.dispose();
  assert.equal(n, 2);
  code(() => a.contribute({ id: 'a.three', contribute: () => 42 }), 'ECONTRIBUTION_FAILED');
});

test('a throwing contribute() is rolled back atomically', () => {
  const { a, commandRegistry } = app();
  const err = (() => {
    try {
      a.contribute({
        id: 'broken.feature',
        contribute(v) {
          v.commands.register(cmd('model.one'));
          v.commands.register(cmd('model.one')); // ECOMMAND_DUPLICATE
        },
      });
    } catch (e) { return e; }
    return null;
  })();
  assert.equal(err.code, 'ECONTRIBUTION_FAILED');
  assert.equal(err.cause.code, 'ECOMMAND_DUPLICATE');
  assert.equal(commandRegistry.has('model.one'), false);
  assert.equal(a.has('broken.feature'), false);
});

test('an async contribute() is refused and rolled back', () => {
  const { a, commandRegistry } = app();
  code(() => a.contribute({
    id: 'async.feature',
    contribute(v) { v.commands.register(cmd('model.two')); return Promise.resolve(); },
  }), 'ECONTRIBUTION_ASYNC');
  assert.equal(commandRegistry.has('model.two'), false);
});

test('invalid and duplicate contributions', () => {
  const { a } = app();
  for (const bad of [null, {}, { id: 'Bad', contribute() {} }, { id: 'a..b', contribute() {} }, { id: 'ok' }]) {
    code(() => a.contribute(bad), 'ECONTRIBUTION_INVALID');
  }
  a.contribute({ id: 'dup', contribute() {} });
  code(() => a.contribute({ id: 'dup', contribute() {} }), 'ECONTRIBUTION_DUPLICATE');
});

test('application dispose: reverse order, then refuses new contributions', () => {
  const { a } = app();
  const order = [];
  a.contribute({ id: 'first', contribute: () => () => order.push('first') });
  a.contribute({ id: 'second', contribute: () => () => order.push('second') });
  a.dispose();
  a.dispose();
  assert.deepEqual(order, ['second', 'first']);
  code(() => a.contribute({ id: 'late', contribute() {} }), 'EAPP_DISPOSED');
});

test('service boundary: closed catalog, deferred services refused, required present, surface checked', () => {
  const s = realServices();
  code(() => createApplication({ services: { commands: s.commands } }), 'EAPP_SERVICE_MISSING');
  code(() => createApplication({ services: { commands: s.commands, panels: s.panels, everything: {} } }), 'EAPP_SERVICE_UNKNOWN');
  for (const name of Object.keys(DEFERRED_SERVICES)) {
    code(() => createApplication({ services: { commands: s.commands, panels: s.panels, [name]: {} } }), 'EAPP_SERVICE_DEFERRED');
  }
  code(() => createApplication({ services: { commands: s.commands, panels: s.panels, dialogs: {} } }), 'EAPP_SERVICE_INVALID');
  // catalog and deferred lists are disjoint, and every tracked method is part of the surface
  for (const name of Object.keys(SERVICE_CATALOG)) {
    assert.ok(!(name in DEFERRED_SERVICES), name);
    for (const t of SERVICE_CATALOG[name].tracked) assert.ok(SERVICE_CATALOG[name].methods.includes(t), `${name}.${t}`);
  }
  // the real SHELL-0 services satisfy their catalog entries
  const documents = createDocumentSlot();
  const tools = createToolRegistry({ commands: s.commands, getProfile: () => null, getSelectionInfo: () => null });
  const dialogs = { isModalOpen: () => false };
  const a = createApplication({ services: { commands: s.commands, panels: s.panels, documents, tools, dialogs } });
  assert.deepEqual(Object.keys(a.services).sort(), ['commands', 'dialogs', 'documents', 'panels', 'tools']);
});

test('scoped app is frozen and exposes only catalog services plus its own store', () => {
  const { a } = app();
  let seen;
  a.contribute({ id: 'peek', contribute(v) { seen = v; } });
  assert.ok(Object.isFrozen(seen));
  assert.ok(Object.isFrozen(seen.commands));
  assert.deepEqual(Object.keys(seen).sort(), ['commands', 'contributionId', 'disposables', 'panels']);
});
