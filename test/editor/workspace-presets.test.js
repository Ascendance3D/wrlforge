'use strict';
// UI-0 workspace presets (docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md §12.2, §12.3).
const test = require('node:test');
const assert = require('node:assert/strict');
const WP = require('../../src/editor/workspace-presets');
const UI = require('../../src/editor/ui-state');
// preferences.js exports its workspace-mode list as `WORKSPACE_MODES` (PREF_WORKSPACE_MODES internally).
const prefs = require('../../src/settings/preferences');

test('table and each preset are frozen', () => {
  assert.ok(Object.isFrozen(WP.WORKSPACE_PRESETS));
  for (const p of Object.values(WP.WORKSPACE_PRESETS)) assert.ok(Object.isFrozen(p));
  assert.ok(Object.isFrozen(WP.ACTIVE_WORKSPACE_MODES));
  assert.ok(Object.isFrozen(WP.PERSISTABLE_WORKSPACE_MODES));
});

test('exact preset fields (§12.3)', () => {
  assert.deepEqual(WP.WORKSPACE_PRESETS.code, { label: 'Code', composition: 'source-primary', viewportPicking: false, visualAuthoring: true, sourceEditing: true, persistable: true });
  assert.deepEqual(WP.WORKSPACE_PRESETS.model, { label: 'Model', composition: 'visual-primary', viewportPicking: true, visualAuthoring: true, sourceEditing: true, persistable: true });
  assert.deepEqual(WP.WORKSPACE_PRESETS.play, { label: 'Play', composition: 'visual-primary', viewportPicking: false, visualAuthoring: false, sourceEditing: false, persistable: false });
  assert.deepEqual(Object.keys(WP.WORKSPACE_PRESETS), ['code', 'model', 'play']);
});

test('active modes exclude play; resolveWorkspaceMode coerces', () => {
  assert.deepEqual([...WP.ACTIVE_WORKSPACE_MODES], ['code', 'model']);
  assert.equal(WP.resolveWorkspaceMode('model'), 'model');
  assert.equal(WP.resolveWorkspaceMode('code'), 'code');
  for (const m of ['play', 'bogus', undefined, null, 5]) assert.equal(WP.resolveWorkspaceMode(m), 'code');
});

test('persistable modes are single-sourced with ui-state and preferences', () => {
  assert.deepEqual([...WP.PERSISTABLE_WORKSPACE_MODES], ['code', 'model']);
  assert.deepEqual([...WP.PERSISTABLE_WORKSPACE_MODES], [...UI.WORKSPACE_MODES]);
  assert.deepEqual([...WP.PERSISTABLE_WORKSPACE_MODES], [...prefs.WORKSPACE_MODES]);
});

test('workspacePreset viewportPicking', () => {
  assert.equal(WP.workspacePreset('model').viewportPicking, true);
  assert.equal(WP.workspacePreset('code').viewportPicking, false);
  assert.equal(WP.workspacePreset('play').label, 'Code'); // play is not activatable in #35
  assert.equal(WP.workspacePreset(undefined).label, 'Code');
});
