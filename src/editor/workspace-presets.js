'use strict';
// UI-0 workspace presets (docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md §12.3).
//
// A pure frozen table of workspace SEMANTICS, not layouts. editor.js keeps
// S.workspaceMode + setWorkspaceMode as the one authority and reads the preset
// for the current mode instead of testing `=== 'model'`.
//
// `play` is defined here as architecture metadata only. It is not selectable in
// #35: setWorkspaceMode admits only the ACTIVE_WORKSPACE_MODES, and no
// workspace.play command exists. UI-0-I2 (#121) activates it.

const WORKSPACE_PRESETS = Object.freeze({
  code: Object.freeze({
    label: 'Code', composition: 'source-primary', viewportPicking: false,
    visualAuthoring: true, sourceEditing: true, persistable: true,
  }),
  model: Object.freeze({
    label: 'Model', composition: 'visual-primary', viewportPicking: true,
    visualAuthoring: true, sourceEditing: true, persistable: true,
  }),
  play: Object.freeze({
    label: 'Play', composition: 'visual-primary', viewportPicking: false,
    visualAuthoring: false, sourceEditing: false, persistable: false,
  }),
});

// The modes a user can be in today (#35). Play joins in #121.
const ACTIVE_WORKSPACE_MODES = Object.freeze(['code', 'model']);
// The modes that may be remembered as the startup workspace (§17).
const PERSISTABLE_WORKSPACE_MODES = Object.freeze(Object.keys(WORKSPACE_PRESETS).filter((m) => WORKSPACE_PRESETS[m].persistable));

// Today's coercion, unchanged: anything that is not an active mode is 'code'.
function resolveWorkspaceMode(mode) {
  return ACTIVE_WORKSPACE_MODES.includes(mode) ? mode : 'code';
}

function workspacePreset(mode) {
  return WORKSPACE_PRESETS[resolveWorkspaceMode(mode)];
}

const WRL_WORKSPACE_PRESETS_API = Object.freeze({
  WORKSPACE_PRESETS, ACTIVE_WORKSPACE_MODES, PERSISTABLE_WORKSPACE_MODES,
  resolveWorkspaceMode, workspacePreset,
});

if (typeof module !== 'undefined' && module.exports) {
  module.exports = WRL_WORKSPACE_PRESETS_API;
} else {
  window.WrlWorkspacePresets = WRL_WORKSPACE_PRESETS_API;
}
