'use strict';
// UI-0 Panel Registry (docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md §11).
//
// DESCRIBES the editor page's existing panels: identity, title, how to focus
// them, whether they are currently visible, and which can be shown or hidden.
// It is not a layout engine: no geometry, order, tabs, sizes or docking. It owns
// no document data. Visibility is derived from the panel element on every read
// (the CSS + workspace/layout classes stay the single authority), never stored.

const PANEL_ID_PATTERN = /^[a-z][a-zA-Z0-9]*$/;
// Reserved for a future panel that does not exist yet; never registered (§11.4).
const RESERVED_PANEL_IDS = Object.freeze(['console']);

const FOCUSABLE = 'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"]), [contenteditable="true"]';

function panelError(code, message) {
  const e = new Error(`${code}: ${message}`);
  e.code = code;
  return e;
}

function validatePanel(r) {
  const bad = (why) => { throw panelError('EPANEL_INVALID', why); };
  if (!r || typeof r !== 'object') bad('a panel record must be an object');
  if (typeof r.id !== 'string' || !PANEL_ID_PATTERN.test(r.id)) bad(`invalid panel id ${JSON.stringify(r.id)}`);
  if (RESERVED_PANEL_IDS.includes(r.id)) bad(`panel id "${r.id}" is reserved`);
  if (typeof r.title !== 'string' || r.title.trim() === '') bad(`${r.id}: title is required`);
  if (typeof r.element !== 'function') bad(`${r.id}: element must be a function returning the panel root`);
  if (r.focus !== undefined && typeof r.focus !== 'function') bad(`${r.id}: focus must be a function`);
  if (r.canToggle !== undefined && typeof r.canToggle !== 'boolean') bad(`${r.id}: canToggle must be a boolean`);
  if (r.canToggle && (typeof r.show !== 'function' || typeof r.hide !== 'function')) bad(`${r.id}: a toggleable panel needs show() and hide()`);
}

function createPanelRegistry() {
  const panels = new Map(); // id -> { record, descriptor }
  let disposed = false;

  function entry(id) {
    const e = panels.get(id);
    if (!e) throw panelError('EPANEL_UNKNOWN', `no panel ${JSON.stringify(id)}`);
    return e;
  }

  function register(record) {
    if (disposed) throw panelError('EPANEL_DISPOSED', 'the registry has been disposed');
    validatePanel(record);
    if (panels.has(record.id)) throw panelError('EPANEL_DUPLICATE', `panel ${record.id} is already registered`);
    const descriptor = Object.freeze({ id: record.id, title: record.title, canToggle: !!record.canToggle });
    const e = { record, descriptor };
    panels.set(record.id, e);
    return function unregister() { if (panels.get(record.id) === e) panels.delete(record.id); };
  }

  function has(id) { return panels.has(id); }
  function get(id) { const e = panels.get(id); return e ? e.descriptor : null; }
  function list() { return [...panels.values()].map((e) => e.descriptor); }

  // Rendered <=> the element exists and produces layout boxes.
  function isVisible(id) {
    const node = entry(id).record.element();
    return !!(node && typeof node.getClientRects === 'function' && node.getClientRects().length > 0);
  }

  // Focus never opens a hidden panel: opening is a workspace/layout decision.
  function focus(id) {
    const { record } = entry(id);
    if (!isVisible(id)) return { ok: false, reason: 'hidden' };
    if (record.focus) { record.focus(); return { ok: true }; }
    const node = record.element();
    const target = node.querySelector ? node.querySelector(FOCUSABLE) : null;
    if (!target) return { ok: false, reason: 'no-focusable' };
    target.focus();
    return { ok: true };
  }

  function toggleResult(r) { return r && typeof r === 'object' && typeof r.ok === 'boolean' ? r : { ok: true }; }
  function show(id) {
    const { record } = entry(id);
    if (!record.canToggle) return { ok: false, reason: 'not-toggleable' };
    return toggleResult(record.show());
  }
  function hide(id) {
    const { record } = entry(id);
    if (!record.canToggle) return { ok: false, reason: 'not-toggleable' };
    return toggleResult(record.hide());
  }

  function dispose() { disposed = true; panels.clear(); }

  return Object.freeze({ register, has, get, list, isVisible, focus, show, hide, dispose });
}

const WRL_PANEL_REGISTRY_API = Object.freeze({ PANEL_ID_PATTERN, RESERVED_PANEL_IDS, createPanelRegistry });

if (typeof module !== 'undefined' && module.exports) {
  module.exports = WRL_PANEL_REGISTRY_API;
} else {
  window.WrlPanelRegistry = WRL_PANEL_REGISTRY_API;
}
