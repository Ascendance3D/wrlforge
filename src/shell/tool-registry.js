'use strict';
// SHELL-0 modeling-tool contribution boundary
// (docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md §8; APP-ARCH-0 §18).
//
// The `app.tools` service: tool RECORDS that toolbars render from
// `tools.list(group)`. A tool is presentation + applicability over ONE command;
// it has no behavior of its own:
//
//   app.tools.register({
//     id: 'tool.move', commandId: 'model.move', group: 'transform',
//     label: 'Move', icon: 'move', profiles?: ['world'],
//     appliesTo?: (selectionInfo) => selectionInfo.type === 'Transform',
//   });
//
//   * Running a tool is commands.execute(commandId). Enabled and checked are
//     the command's (one pull model, UI-0 §6) -- no second enabled state.
//   * A tool is AVAILABLE (shown/usable) only when its command is registered,
//     the current profile is listed (if `profiles` is given), and appliesTo
//     accepts the current proven selection. appliesTo never sees an unproven
//     selection: with no proven selection a tool that declares appliesTo is
//     unavailable (fail closed, WD.md §7). A throwing appliesTo is unavailable.
//   * The command must already be registered when the tool is (fail closed:
//     a tool can never point at nothing).
//   * Tool-specific behavior never lives here: the source effect stays in the
//     command, through a pure planner -> applyVerifiedEdits (APP-ARCH-0 §18).
//
// None of the §18 tools (Select ... Keyframe) is implemented in SHELL-0.

(function () {
  const isNode = typeof module !== 'undefined' && module.exports;
  const PR = isNode ? require('./profiles') : window.WrlShellProfiles;

  const TOOL_GROUPS = Object.freeze(['select', 'transform', 'create', 'appearance', 'hierarchy', 'behavior', 'animation']);
  const TOOL_ID_PATTERN = /^tool\.[a-z][a-zA-Z0-9]*$/;

  function toolError(code, message) {
    const e = new Error(`${code}: ${message}`);
    e.code = code;
    return e;
  }

  // commands: the app.commands service. getProfile: () => profile id | null.
  // getSelectionInfo: () => { proven: true, ... } | null -- from the document
  // session's existing selection + analysis authorities, never recomputed here.
  function createToolRegistry({ commands, getProfile, getSelectionInfo } = {}) {
    if (!commands || typeof commands.execute !== 'function') throw toolError('ETOOL_INVALID', 'the commands service is required');
    if (typeof getProfile !== 'function') throw toolError('ETOOL_INVALID', 'getProfile() is required');
    if (typeof getSelectionInfo !== 'function') throw toolError('ETOOL_INVALID', 'getSelectionInfo() is required');

    const tools = new Map(); // id -> { record, descriptor, profiles }
    const listeners = new Set();

    function changed() {
      for (const fn of [...listeners]) {
        try { fn(); } catch (err) { console.error('[tool-registry] listener failed:', err); }
      }
    }

    function validate(r) {
      const bad = (why) => { throw toolError('ETOOL_INVALID', why); };
      if (!r || typeof r !== 'object') bad('a tool record must be an object');
      if (typeof r.id !== 'string' || !TOOL_ID_PATTERN.test(r.id)) bad(`invalid tool id ${JSON.stringify(r.id)}`);
      if (typeof r.commandId !== 'string' || !commands.has(r.commandId)) bad(`${r.id}: command ${JSON.stringify(r.commandId)} is not registered`);
      if (!TOOL_GROUPS.includes(r.group)) bad(`${r.id}: unknown group ${JSON.stringify(r.group)}`);
      if (r.label !== undefined && (typeof r.label !== 'string' || r.label.trim() === '')) bad(`${r.id}: label must be a non-empty string`);
      if (r.icon !== undefined && typeof r.icon !== 'string') bad(`${r.id}: icon must be a string`);
      if (r.appliesTo !== undefined && typeof r.appliesTo !== 'function') bad(`${r.id}: appliesTo must be a function`);
    }

    function register(record) {
      validate(record);
      if (tools.has(record.id)) throw toolError('ETOOL_DUPLICATE', `tool ${record.id} is already registered`);
      const profiles = record.profiles === undefined ? null : PR.normalizeProfileList(record.profiles, `tool ${record.id}`);
      const command = commands.get(record.commandId);
      const descriptor = Object.freeze({
        id: record.id, commandId: record.commandId, group: record.group,
        label: record.label || command.label, icon: record.icon || null, profiles,
      });
      const e = { record, descriptor, profiles };
      tools.set(record.id, e);
      changed();
      return function unregister() {
        if (tools.get(record.id) !== e) return;
        tools.delete(record.id);
        changed();
      };
    }

    function entry(id) {
      const e = tools.get(id);
      if (!e) throw toolError('ETOOL_UNKNOWN', `no tool ${JSON.stringify(id)}`);
      return e;
    }

    function has(id) { return tools.has(id); }
    function get(id) { const e = tools.get(id); return e ? e.descriptor : null; }
    // Registration order within a group is the toolbar order.
    function list(group) {
      return [...tools.values()].filter((e) => group === undefined || e.descriptor.group === group).map((e) => e.descriptor);
    }

    function available(e) {
      if (!commands.has(e.record.commandId)) return false;
      if (e.profiles && !e.profiles.includes(getProfile())) return false;
      if (!e.record.appliesTo) return true;
      const sel = getSelectionInfo();
      if (!sel || sel.proven !== true) return false;
      try { return !!e.record.appliesTo(sel); } catch (err) {
        console.error(`[tool-registry] ${e.record.id} appliesTo failed:`, err);
        return false;
      }
    }

    // Read on demand, never stored: { available, enabled, checked }.
    function state(id) {
      const e = entry(id);
      const ok = available(e);
      return Object.freeze({
        available: ok,
        enabled: ok && commands.isEnabled(e.record.commandId),
        checked: ok ? commands.isChecked(e.record.commandId) : null,
      });
    }

    function activate(id, ctx) {
      const e = entry(id);
      if (!available(e)) return { ok: false, reason: 'unavailable' };
      return commands.execute(e.record.commandId, { source: (ctx && ctx.source) || 'toolbar' });
    }

    // Fires when the SET of tools changes (re-render a toolbar). State repaint
    // follows commands.subscribe, exactly like [data-command] controls.
    function subscribe(fn) {
      if (typeof fn !== 'function') throw toolError('ETOOL_INVALID', 'subscribe needs a function');
      listeners.add(fn);
      return () => { listeners.delete(fn); };
    }

    function listenerCount() { return listeners.size; }

    return Object.freeze({ register, has, get, list, state, activate, subscribe, listenerCount });
  }

  const WRL_SHELL_TOOL_REGISTRY_API = Object.freeze({ TOOL_GROUPS, TOOL_ID_PATTERN, createToolRegistry });

  if (isNode) {
    module.exports = WRL_SHELL_TOOL_REGISTRY_API;
  } else {
    window.WrlShellToolRegistry = WRL_SHELL_TOOL_REGISTRY_API;
  }
})();
