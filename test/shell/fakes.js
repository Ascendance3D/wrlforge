'use strict';
// Shared fakes for the SHELL-0 contract tests. Not a test file (no .test.js).
const { createCommandRegistry } = require('../../src/editor/command-registry');
const { createPanelRegistry } = require('../../src/editor/panel-registry');
const { createSelectionController } = require('../../src/editor/scene-selection');
const { createCommandService } = require('../../src/shell/command-service');
const { createPanelService } = require('../../src/shell/panel-service');

// A panel root that is "rendered" while visible.
function fakeElement({ visible = true } = {}) {
  const state = { visible, focused: 0 };
  const focusable = { focus() { state.focused++; } };
  return {
    state,
    getClientRects() { return state.visible ? [1] : []; },
    querySelector() { return focusable; },
  };
}

// The real UI-0 registries, wrapped by the real SHELL-0 services.
function realServices({ profile = null } = {}) {
  const ctx = { profile };
  const commandRegistry = createCommandRegistry();
  const panelRegistry = createPanelRegistry();
  const commands = createCommandService(commandRegistry, { getProfile: () => ctx.profile });
  const panels = createPanelService(panelRegistry);
  return { ctx, commandRegistry, panelRegistry, commands, panels };
}

// A CodeMirror-handle stand-in with the handle's real method names.
function fakeEditor(initial = '#VRML V2.0 utf8\n') {
  let text = initial;
  let undo = 0;
  let destroyed = 0;
  return {
    getText: () => text,
    historyDepth: () => ({ undo, redo: 0 }),
    applyVerifiedEdits({ oldText, edits, newText }) {
      if (oldText !== text) return { ok: false, reason: 'stale' };
      let out = text;
      for (const e of [...edits].sort((a, b) => b.from - a.from)) out = out.slice(0, e.from) + e.insert + out.slice(e.to);
      if (out !== newText) return { ok: false, reason: 'mismatch' };
      text = out; undo++;
      return { ok: true };
    },
    destroy() { destroyed++; },
    // test-only: a user typing in CodeMirror
    type(s) { text += s; undo++; },
    get destroyed() { return destroyed; },
  };
}

module.exports = { fakeElement, realServices, fakeEditor, createSelectionController };
