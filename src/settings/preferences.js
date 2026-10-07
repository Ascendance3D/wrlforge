'use strict';
// Phase: Preferences & Settings -- the pure preferences model.
//
// One shared authority for every renderer-side user preference the app honors.
// The user's existing settings already persist in localStorage under keys
// first written by renderer/editor.js (`wrlforge.editor.theme`,
// `wrlforge.editor.zoom`, `wrlforge.editor.previewLayout`). This module wraps
// those keys with a uniform read/write/normalize interface, adds one small
// auxiliary field (`lastNonContrastTheme`) the High Contrast toggle needs to
// revert to the user's previous non-contrast theme, and exposes a single
// `update()` / `setHighContrastEnabled()` pure state transition.
//
// Storage layout (one source of truth per preference; no shadow values):
//
//   wrlforge.editor.theme               one of PREF_THEMES
//   wrlforge.editor.zoom                integer ZOOM_MIN..ZOOM_MAX
//   wrlforge.editor.previewLayout       one of PREF_PREVIEW_LAYOUTS
//   wrlforge.editor.lastNonContrastTheme one of PREF_THEMES (never 'contrast')
//   wrlforge.editor.workspaceMode       one of PREF_WORKSPACE_MODES (WD2-C)
//
// Pure: no DOM, no fs, no Electron. The renderer wraps this with a
// state-store + change events. Tolerates missing/garbage storage. Reads
// each key independently so a corrupted value for one key cannot prevent
// the others from being honored.

const KEY_THEME = 'wrlforge.editor.theme';
const KEY_ZOOM = 'wrlforge.editor.zoom';
const KEY_PREVIEW_LAYOUT = 'wrlforge.editor.previewLayout';
const KEY_LAST_NON_CONTRAST_THEME = 'wrlforge.editor.lastNonContrastTheme';
// WD2-C: the remembered editor workspace -- 'code' (the source-primary layout
// every earlier release shipped, hence the default) or 'model' (visual-primary,
// Source collapsible). A new/empty document opens in Model regardless.
const KEY_WORKSPACE_MODE = 'wrlforge.editor.workspaceMode';

const DEFAULTS = Object.freeze({
  theme: 'dark',
  zoom: 0,
  previewLayout: 'split',
  lastNonContrastTheme: 'dark',
  workspaceMode: 'code',
});

// Namespaced to avoid a top-level collision with src/editor/ui-state.js
// (THEMES, ZOOM_MIN, ZOOM_MAX) and src/editor/browser/editor-view.js
// (also THEMES) in the shared classic-script scope of renderer/editor.html.
const PREF_THEMES = Object.freeze(['dark', 'light', 'terminal', 'tokyo', 'contrast']);
const PREF_PREVIEW_LAYOUTS = Object.freeze(['split', 'preview-max', 'editor-only']);
const PREF_WORKSPACE_MODES = Object.freeze(['code', 'model']);
const PREF_ZOOM_MIN = -3;
const PREF_ZOOM_MAX = 8;

const THEME_LABELS = Object.freeze({
  dark: 'Dark',
  light: 'Light',
  terminal: 'Terminal',
  tokyo: 'Tokyo Night',
  contrast: 'High Contrast',
});

const PREVIEW_LAYOUT_LABELS = Object.freeze({
  split: 'Split',
  'preview-max': 'Preview maximized',
  'editor-only': 'Editor only',
});

function isValidTheme(t) {
  return typeof t === 'string' && PREF_THEMES.indexOf(t) !== -1;
}
function isValidPreviewLayout(l) {
  return typeof l === 'string' && PREF_PREVIEW_LAYOUTS.indexOf(l) !== -1;
}
function isValidWorkspaceMode(m) {
  return typeof m === 'string' && PREF_WORKSPACE_MODES.indexOf(m) !== -1;
}
function clampZoom(level) {
  const n = Math.round(Number(level));
  if (!Number.isFinite(n)) return DEFAULTS.zoom;
  return Math.max(PREF_ZOOM_MIN, Math.min(PREF_ZOOM_MAX, n));
}

// Validate / fill defaults. Each field falls back independently so a single
// bad value never blocks the others.
function normalize(input) {
  const src = input || {};
  const theme = isValidTheme(src.theme) ? src.theme : DEFAULTS.theme;
  const zoom = clampZoom(src.zoom);
  const previewLayout = isValidPreviewLayout(src.previewLayout)
    ? src.previewLayout
    : DEFAULTS.previewLayout;
  // lastNonContrastTheme is internal: it must be a non-contrast theme.
  const lastNonContrastTheme = isValidTheme(src.lastNonContrastTheme)
    && src.lastNonContrastTheme !== 'contrast'
    ? src.lastNonContrastTheme
    : DEFAULTS.lastNonContrastTheme;
  const workspaceMode = isValidWorkspaceMode(src.workspaceMode) ? src.workspaceMode : DEFAULTS.workspaceMode;
  return { theme, zoom, previewLayout, lastNonContrastTheme, workspaceMode };
}

// Read the current preferences from a storage object (e.g. localStorage).
// `storage` is a duck-typed `{ getItem(key) }` to keep this module
// dependency-free. Tolerates throwing storage (private mode, quota, etc.).
function read(storage) {
  const safeGet = (k) => {
    if (!storage) return null;
    try { return storage.getItem(k); } catch (e) { return null; }
  };
  return normalize({
    theme: safeGet(KEY_THEME),
    zoom: safeGet(KEY_ZOOM),
    previewLayout: safeGet(KEY_PREVIEW_LAYOUT),
    lastNonContrastTheme: safeGet(KEY_LAST_NON_CONTRAST_THEME),
    workspaceMode: safeGet(KEY_WORKSPACE_MODE),
  });
}

// Write the full preferences to storage. Errors are swallowed (best-effort).
function write(storage, prefs) {
  const safeSet = (k, v) => {
    if (!storage) return;
    try { storage.setItem(k, String(v)); } catch (e) { /* best-effort */ }
  };
  if (!prefs) return;
  safeSet(KEY_THEME, prefs.theme);
  safeSet(KEY_ZOOM, prefs.zoom);
  safeSet(KEY_PREVIEW_LAYOUT, prefs.previewLayout);
  safeSet(KEY_LAST_NON_CONTRAST_THEME, prefs.lastNonContrastTheme);
  if (prefs.workspaceMode !== undefined) safeSet(KEY_WORKSPACE_MODE, prefs.workspaceMode);
}

// Single-key state transition. Returns the new full state, OR the input
// `prev` unchanged when the transition is a no-op (so callers can detect
// "nothing changed" with a reference equality check, and downstream
// subscribers don't churn on a re-write of the same value). Invalid
// values fall through to defaults rather than throw; the caller's UI is
// the source of valid values, and a poisoned preference must never crash.
function update(prev, key, value) {
  // Field-level guards first, against the input as-given (so a no-op
  // write of the same valid value returns the original object).
  if (key === 'theme') {
    if (isValidTheme(value) && prev && prev.theme === value) return prev;
  } else if (key === 'zoom') {
    if (Number.isFinite(Number(value)) && prev && prev.zoom === clampZoom(value)) return prev;
  } else if (key === 'previewLayout') {
    if (isValidPreviewLayout(value) && prev && prev.previewLayout === value) return prev;
  } else if (key === 'lastNonContrastTheme') {
    if (isValidTheme(value) && value !== 'contrast' && prev && prev.lastNonContrastTheme === value) return prev;
  } else if (key === 'workspaceMode') {
    if (isValidWorkspaceMode(value) && prev && prev.workspaceMode === value) return prev;
  }
  const cur = normalize(prev);
  if (key === 'theme') {
    const t = isValidTheme(value) ? value : DEFAULTS.theme;
    if (t === cur.theme) return cur;
    const next = { ...cur, theme: t };
    if (t === 'contrast' && cur.theme !== 'contrast') {
      next.lastNonContrastTheme = cur.theme;
    }
    return next;
  }
  if (key === 'zoom') {
    const z = clampZoom(value);
    if (z === cur.zoom) return cur;
    return { ...cur, zoom: z };
  }
  if (key === 'previewLayout') {
    const l = isValidPreviewLayout(value) ? value : DEFAULTS.previewLayout;
    if (l === cur.previewLayout) return cur;
    return { ...cur, previewLayout: l };
  }
  if (key === 'lastNonContrastTheme') {
    if (!isValidTheme(value) || value === 'contrast') return cur;
    if (value === cur.lastNonContrastTheme) return cur;
    return { ...cur, lastNonContrastTheme: value };
  }
  if (key === 'workspaceMode') {
    if (!isValidWorkspaceMode(value) || value === cur.workspaceMode) return cur;
    return { ...cur, workspaceMode: value };
  }
  return cur;
}

// High Contrast toggle transition: ON = force theme='contrast' (remembering
// the prior non-contrast); OFF = revert to lastNonContrastTheme.
function setHighContrastEnabled(prev, enabled) {
  const cur = normalize(prev);
  if (enabled) {
    if (cur.theme === 'contrast') return cur;
    return update(cur, 'theme', 'contrast');
  }
  const revert = cur.lastNonContrastTheme !== 'contrast'
    ? cur.lastNonContrastTheme
    : DEFAULTS.lastNonContrastTheme;
  return update(cur, 'theme', revert);
}

function highContrastEnabled(prefs) {
  return !!(prefs && prefs.theme === 'contrast');
}

// Zoom level (integer) -> derived display model. Duplicates the editor's
// own ui-state.js model here for the test suite and the dialog's label;
// ui-state.js remains the renderer's source of truth, but the dialog
// computes its own label so the pure module is self-contained.
const PREF_ZOOM_BASE_FONT_PX = 13;
function zoomModel(level) {
  const lv = clampZoom(level);
  const factor = 1 + lv * 0.1;
  const codeFontPx = Math.round(PREF_ZOOM_BASE_FONT_PX * factor);
  return { level: lv, chromeScale: factor, codeFontPx, label: Math.round(factor * 100) + '%' };
}

const PREF_API = {
  KEY_THEME, KEY_ZOOM, KEY_PREVIEW_LAYOUT, KEY_LAST_NON_CONTRAST_THEME, KEY_WORKSPACE_MODE,
  DEFAULTS,
  THEMES: PREF_THEMES, PREVIEW_LAYOUTS: PREF_PREVIEW_LAYOUTS, WORKSPACE_MODES: PREF_WORKSPACE_MODES,
  ZOOM_MIN: PREF_ZOOM_MIN, ZOOM_MAX: PREF_ZOOM_MAX,
  THEME_LABELS, PREVIEW_LAYOUT_LABELS,
  isValidTheme, isValidPreviewLayout, isValidWorkspaceMode, clampZoom,
  normalize, read, write, update,
  setHighContrastEnabled, highContrastEnabled, zoomModel,
};

if (typeof module !== 'undefined' && module.exports) {
  module.exports = PREF_API;
} else if (typeof window !== 'undefined') {
  window.WrlPreferencesCore = PREF_API;
}
