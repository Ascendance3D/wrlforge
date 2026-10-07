'use strict';
// UI-0 Command Registry (docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md §6, §9).
//
// Pure: no DOM, no Electron, no state of its own beyond the registered records.
// A command record holds BEHAVIOUR references (run / enabled / checked) that
// close over the page's existing authorities; enabled and checked are computed
// when read, never stored. The registry owns no source, AST, selection, undo
// history, preview scene or workspace state.
//
// Shortcut grammar (§9.2, as adopted by #35): `[Mod+][Shift+]<key>`. `Mod`
// matches Ctrl OR Meta on every platform (today's `ctrlOrMeta`). Alt is not
// significant: today's resolveShortcut ignores it, so a pure migration must
// too. Shift is significant only for letters and named keys; for digits and
// punctuation the layout needs Shift to produce the key, so it is dropped.

const COMMAND_AREAS = Object.freeze(['file', 'edit', 'view', 'workspace', 'preview', 'selection', 'model', 'panel']);
const COMMAND_ID_PATTERN = /^[a-z][a-zA-Z0-9]*(\.[a-z][a-zA-Z0-9]*){1,2}$/;
const KEY_OWNERS = Object.freeze(['app', 'editor']);

// Named keys, keyed by their lowercased KeyboardEvent.key spelling.
const NAMED_KEYS = Object.freeze({
  enter: 'Enter', escape: 'Escape', delete: 'Delete', add: 'Add', subtract: 'Subtract',
  f1: 'F1', f2: 'F2', f3: 'F3', f4: 'F4', f5: 'F5', f6: 'F6',
  f7: 'F7', f8: 'F8', f9: 'F9', f10: 'F10', f11: 'F11', f12: 'F12',
});
// Add / Subtract behave like the punctuation they stand for: Shift-insensitive.
const SHIFT_INSENSITIVE_NAMED = new Set(['Add', 'Subtract']);

function commandError(code, message) {
  const e = new Error(`${code}: ${message}`);
  e.code = code;
  return e;
}

// key token -> canonical key, or null when the token is not a key.
function canonicalKey(token) {
  if (typeof token !== 'string' || token === '') return null;
  const lower = token.toLowerCase();
  if (Object.prototype.hasOwnProperty.call(NAMED_KEYS, lower)) return NAMED_KEYS[lower];
  if ([...token].length === 1) return lower;
  return null;
}

function shiftMatters(key) {
  if (/^[a-z]$/.test(key)) return true;
  return Object.values(NAMED_KEYS).includes(key) && !SHIFT_INSENSITIVE_NAMED.has(key);
}

function compose(mod, shift, key) {
  return (mod ? 'Mod+' : '') + (shift && shiftMatters(key) ? 'Shift+' : '') + key;
}

// 'Mod+Shift+S' -> 'Mod+Shift+s'; 'Mod++' -> 'Mod++'. Throws ECOMMAND_INVALID.
function normalizeShortcut(text) {
  if (typeof text !== 'string' || text === '') throw commandError('ECOMMAND_INVALID', 'a shortcut must be a non-empty string');
  let keyToken;
  let mods;
  if (text === '+' || text.endsWith('++')) {
    keyToken = '+';
    mods = text.length > 1 ? text.slice(0, -2).split('+') : [];
  } else {
    const parts = text.split('+');
    keyToken = parts.pop();
    mods = parts;
  }
  let mod = false;
  let shift = false;
  for (const m of mods) {
    if (m === 'Mod' && !mod) mod = true;
    else if (m === 'Shift' && !shift) shift = true;
    else throw commandError('ECOMMAND_INVALID', `unsupported shortcut modifier "${m}" in "${text}"`);
  }
  const key = canonicalKey(keyToken);
  if (!key) throw commandError('ECOMMAND_INVALID', `unsupported shortcut key in "${text}"`);
  return compose(mod, shift, key);
}

// A keydown-shaped object -> its normalized shortcut, or null.
function shortcutFromEvent(e) {
  if (!e) return null;
  const key = canonicalKey(e.key);
  if (!key) return null;
  return compose(!!(e.ctrlKey || e.metaKey), !!e.shiftKey, key);
}

function isModShortcut(shortcut) { return typeof shortcut === 'string' && shortcut.startsWith('Mod+'); }

function validateRecord(r) {
  const bad = (why) => { throw commandError('ECOMMAND_INVALID', why); };
  if (!r || typeof r !== 'object') bad('a command record must be an object');
  if (typeof r.id !== 'string' || !COMMAND_ID_PATTERN.test(r.id)) bad(`invalid command id ${JSON.stringify(r.id)}`);
  if (!COMMAND_AREAS.includes(r.area)) bad(`${r.id}: unknown area ${JSON.stringify(r.area)}`);
  if (r.id.split('.')[0] !== r.area) bad(`${r.id}: area "${r.area}" must equal the id's first segment`);
  if (typeof r.label !== 'string' || r.label.trim() === '') bad(`${r.id}: label is required`);
  if (typeof r.run !== 'function') bad(`${r.id}: run must be a function`);
  if (r.enabled !== undefined && typeof r.enabled !== 'function') bad(`${r.id}: enabled must be a function`);
  if (r.checked !== undefined && typeof r.checked !== 'function') bad(`${r.id}: checked must be a function`);
  if (r.keyOwner !== undefined && !KEY_OWNERS.includes(r.keyOwner)) bad(`${r.id}: keyOwner must be 'app' or 'editor'`);
  if (r.keys !== undefined && (!Array.isArray(r.keys) || r.keys.some((k) => typeof k !== 'string'))) bad(`${r.id}: keys must be an array of strings`);
}

function createCommandRegistry() {
  const commands = new Map(); // id -> { record, descriptor }
  const listeners = new Set();
  let disposed = false;

  function entry(id) {
    const e = commands.get(id);
    if (!e) throw commandError('ECOMMAND_UNKNOWN', `no command ${JSON.stringify(id)}`);
    return e;
  }

  function register(record) {
    if (disposed) throw commandError('ECOMMAND_DISPOSED', 'the registry has been disposed');
    validateRecord(record);
    if (commands.has(record.id)) throw commandError('ECOMMAND_DUPLICATE', `command ${record.id} is already registered`);
    const keyOwner = record.keyOwner || 'app';
    const keys = Object.freeze([...new Set((record.keys || []).map(normalizeShortcut))]);
    if (keyOwner === 'app') {
      for (const { descriptor } of commands.values()) {
        if (descriptor.keyOwner !== 'app') continue;
        const clash = descriptor.keys.find((k) => keys.includes(k));
        if (clash) throw commandError('ECOMMAND_KEY_CONFLICT', `${record.id} and ${descriptor.id} both claim ${clash}`);
      }
    }
    const descriptor = Object.freeze({
      id: record.id, label: record.label, area: record.area, keys, keyOwner,
      isToggle: typeof record.checked === 'function',
    });
    const e = { record, descriptor };
    commands.set(record.id, e);
    return function unregister() { if (commands.get(record.id) === e) commands.delete(record.id); };
  }

  function has(id) { return commands.has(id); }
  function get(id) { const e = commands.get(id); return e ? e.descriptor : null; }
  function list() { return [...commands.values()].map((e) => e.descriptor); }

  function isEnabled(id) {
    const { record } = entry(id);
    return record.enabled ? !!record.enabled() : true;
  }

  function isChecked(id) {
    const { record } = entry(id);
    return record.checked ? !!record.checked() : null;
  }

  // Enabled is evaluated immediately before run. A disabled command refuses
  // silently; a handler's throw or rejection reaches the caller unchanged and
  // leaves the registry untouched (nothing is mutated around run).
  function execute(id, ctx) {
    const { record } = entry(id);
    if (record.enabled && !record.enabled()) return { ok: false, reason: 'disabled' };
    const value = record.run(Object.freeze({ source: (ctx && ctx.source) || 'api' }));
    if (value && typeof value.then === 'function') return value.then((v) => ({ ok: true, value: v }));
    return { ok: true, value };
  }

  function keyBindings() {
    const out = [];
    for (const { descriptor } of commands.values()) for (const key of descriptor.keys) out.push({ key, id: descriptor.id });
    return out;
  }

  function subscribe(fn) {
    if (typeof fn !== 'function') throw commandError('ECOMMAND_INVALID', 'subscribe needs a function');
    if (disposed) return () => {};
    listeners.add(fn);
    return () => { listeners.delete(fn); };
  }

  // Ask every surface to re-read enabled/checked. One faulty listener never
  // stops the others repainting; its error is logged, not swallowed.
  function invalidate() {
    for (const fn of [...listeners]) {
      try { fn(); } catch (err) { console.error('[command-registry] listener failed:', err); }
    }
  }

  function dispose() {
    disposed = true;
    commands.clear();
    listeners.clear();
  }

  return Object.freeze({ register, has, get, list, isEnabled, isChecked, execute, keyBindings, subscribe, invalidate, dispose });
}

const WRL_COMMAND_REGISTRY_API = Object.freeze({
  COMMAND_AREAS, COMMAND_ID_PATTERN, KEY_OWNERS,
  createCommandRegistry, normalizeShortcut, shortcutFromEvent, isModShortcut,
});

if (typeof module !== 'undefined' && module.exports) {
  module.exports = WRL_COMMAND_REGISTRY_API;
} else {
  window.WrlCommandRegistry = WRL_COMMAND_REGISTRY_API;
}
