'use strict';
// Pure native-editor UI view-models (Phase 7B). Everything the editor workspace
// renders -- toolbar enable/disable, the status bar, the outline rows, the
// conflict-dialog decision, keyboard-shortcut dispatch, stale-analysis rejection
// -- is computed here from plain data, so it unit-tests in Node with no DOM.
// renderer/editor.js is a thin binding that maps these models onto elements.

const DIAG_CAP = 200; // cap visible diagnostics/advisories; the true total is kept

// Built-in editor themes (must mirror the palette ids in browser/editor-view.js).
// Two light-on-dark, one dark-on-light, one terminal green, and a pure-black
// High Contrast theme for low-vision use.
const THEMES = [
  { id: 'dark', label: 'Dark' },
  { id: 'light', label: 'Light' },
  { id: 'terminal', label: 'Terminal' },
  { id: 'tokyo', label: 'Tokyo Night' },
  { id: 'contrast', label: 'High Contrast' },
];
const DEFAULT_THEME = 'dark';
function isValidTheme(id) { return THEMES.some((t) => t.id === id); }
// A saved/requested theme falls back to the default when absent or unknown.
function resolveTheme(id) { return isValidTheme(id) ? id : DEFAULT_THEME; }

// --- zoom model (single source of truth for code font + chrome scale) --------
// One integer "zoom level" scales BOTH the CodeMirror code area (codeFontPx, fed
// to the editor handle's setFontSize) and the app chrome (chromeScale, applied
// as the --wrl-ui-scale CSS variable). Keeping both in one pure model means they
// can never drift. Bounds mirror MIN/MAX_FONT_PX in browser/editor-view.js.
const ZOOM_MIN = -3;
const ZOOM_MAX = 8;
const ZOOM_DEFAULT = 0;
const ZOOM_BASE_FONT_PX = 13; // matches editor-view DEFAULT_FONT_PX
const ZOOM_MIN_FONT_PX = 9;   // matches editor-view MIN_FONT_PX
const ZOOM_MAX_FONT_PX = 28;  // matches editor-view MAX_FONT_PX

function clampZoom(level) {
  const n = Math.round(Number(level));
  if (!Number.isFinite(n)) return ZOOM_DEFAULT;
  return Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, n));
}
// A persisted/typed value -> a valid level (defaults on absent/garbage).
function resolveZoom(raw) {
  const n = Math.round(Number(raw));
  return Number.isFinite(n) ? clampZoom(n) : ZOOM_DEFAULT;
}
function zoomStep(level, delta) { return clampZoom(clampZoom(level) + Math.round(Number(delta) || 0)); }
// The derived view-model for a level: 10% per step, clamped font, percent label.
function zoomModel(level) {
  const lv = clampZoom(level);
  const factor = 1 + lv * 0.1; // lv -3..+8 -> 0.7..1.8
  const codeFontPx = Math.max(ZOOM_MIN_FONT_PX, Math.min(ZOOM_MAX_FONT_PX, Math.round(ZOOM_BASE_FONT_PX * factor)));
  return { level: lv, codeFontPx, chromeScale: factor, label: Math.round(factor * 100) + '%' };
}

// --- live-preview layout + status models (Phase 7C2) -------------------------
// Pure view-models for the in-editor live preview: the split-view layout, the
// draggable divider fraction, and the release-quality status chip. DOM-free so
// they unit-test in Node; renderer/editor-preview.js maps them onto elements.
// Internal state-machine names (from src/preview/preview-state.js) NEVER reach
// the user -- previewStatusModel is the single mapping to plain wording.

const PREVIEW_LAYOUTS = Object.freeze(['split', 'preview-max', 'editor-only']);
const PREVIEW_LAYOUT_DEFAULT = 'split';
const SPLIT_MIN = 0.2;   // keep at least 20% for whichever pane is shrinking
const SPLIT_MAX = 0.8;   // ...and no more than 80%, so both stay usable
const SPLIT_DEFAULT = 0.5; // locked 50/50 default

function resolvePreviewLayout(raw) {
  return PREVIEW_LAYOUTS.includes(raw) ? raw : PREVIEW_LAYOUT_DEFAULT;
}
// Clamp a split fraction (editor's share of the work area) to a usable range.
function clampSplit(raw) {
  const n = Number(raw);
  if (!Number.isFinite(n)) return SPLIT_DEFAULT;
  return Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, n));
}
// Step the split by a delta (keyboard divider control), staying clamped.
function splitStep(fraction, delta) {
  return clampSplit(clampSplit(fraction) + (Number(delta) || 0));
}
// The derived layout view-model: which panes show, the clamped fraction, and a
// whole-percent label for the divider's aria-valuenow.
function previewLayoutModel(layout, split) {
  const l = resolvePreviewLayout(layout);
  const f = clampSplit(split);
  return {
    layout: l,
    split: f,
    splitPercent: Math.round(f * 100),
    editorVisible: l !== 'preview-max',
    previewVisible: l !== 'editor-only',
    sidebarVisible: l === 'split', // maximized/editor-only reclaim the sidebar space
    maximized: l === 'preview-max',
  };
}
// Maximize toggle (Ctrl/Cmd+Shift+Enter): split <-> preview-max. From
// editor-only, maximizing goes straight to preview-max.
function togglePreviewMaximize(current) {
  return resolvePreviewLayout(current) === 'preview-max' ? 'split' : 'preview-max';
}

// Map an INTERNAL preview state (src/preview/preview-state.js) + size tier to the
// release-quality chip the user sees. No engineering jargon ever leaves here.
// `saved` overrides everything (the "Show saved version" display); an oversized
// buffer is refused with plain wording; the manual band explains why auto stops.
// `newRefs` (Phase 7C3, World) is the count of buffer references not yet in the
// project graph -- it takes over the settled states so the user is pointed at
// the explicit "Find new files" action, but never masks an in-flight update or
// a hard failure.
function previewStatusModel({ state, failureCategory, saved, sizeTier, newRefs } = {}) {
  if (saved) return { key: 'saved', label: 'Showing saved version', tone: 'info' };
  if (sizeTier === 'refused') {
    return {
      key: 'too-large',
      label: 'This file is too large to display from unsaved changes. Save it, then use the saved version.',
      tone: 'warn',
    };
  }
  const pendingNew = (Number(newRefs) || 0) > 0;
  if (pendingNew && (state === 'current' || state === 'outdated' || state === 'showing-last-valid')) {
    return { key: 'new-file', label: 'New file reference found — choose Find new files', tone: 'warn' };
  }
  switch (state) {
    case 'current': return { key: 'live', label: 'Live', tone: 'ok' };
    case 'updating': return { key: 'updating', label: 'Updating…', tone: 'info' };
    case 'outdated':
      return sizeTier === 'manual'
        ? { key: 'manual', label: 'Large file — use Update to refresh', tone: 'warn' }
        : { key: 'outdated', label: 'Outdated', tone: 'warn' };
    case 'showing-last-valid':
      return failureCategory === 'missing-asset'
        ? { key: 'missing', label: 'Some parts missing', tone: 'warn' }
        : { key: 'last-valid', label: 'Showing last good version', tone: 'warn' };
    case 'failed':
      return failureCategory === 'missing-asset'
        ? { key: 'missing', label: 'Some parts missing', tone: 'warn' }
        : { key: 'failed', label: 'Can’t display latest', tone: 'err' };
    case 'closed': return { key: 'idle', label: 'Preview', tone: 'muted' };
    case 'idle':
    default: return { key: 'idle', label: 'Preview', tone: 'muted' };
  }
}

const FORMAT_LABEL = { plain: 'Plain', gzip: 'gzip' };
function formatLabel(format) { return FORMAT_LABEL[format] || 'Plain'; }

// The renderer is the source of truth for buffer text (avoids an IPC round-trip
// per keystroke); dirty is a pure comparison against the opened/last-saved text.
function isDirty(current, baseline) { return current !== baseline; }

const SAVE_STATE = Object.freeze({
  CLEAN: 'clean', DIRTY: 'dirty', SAVING: 'saving', SAVED: 'saved', ERROR: 'error', CONFLICT: 'conflict',
});
function saveStateLabel(s) {
  switch (s) {
    case SAVE_STATE.SAVING: return 'Saving…';
    case SAVE_STATE.SAVED: return 'Saved';
    case SAVE_STATE.ERROR: return 'Save failed';
    case SAVE_STATE.CONFLICT: return 'File changed on disk';
    case SAVE_STATE.DIRTY: return 'Unsaved changes';
    default: return 'No changes';
  }
}

function basename(p) {
  if (!p) return '';
  const parts = String(p).split(/[\\/]/);
  return parts[parts.length - 1] || String(p);
}

function cursorLabel(cursor) {
  const line = cursor && cursor.line ? cursor.line : 1;
  const col = cursor && cursor.column ? cursor.column : 1;
  return `Ln ${line}, Col ${col}`;
}

// Cap a diagnostics/advisories list for display while retaining the total.
function capDiagnostics(list, cap = DIAG_CAP) {
  const all = Array.isArray(list) ? list : [];
  const shown = all.slice(0, cap);
  return { shown, total: all.length, hidden: Math.max(0, all.length - shown.length), capped: all.length > shown.length };
}

// The complete status-bar model. `saveState` (optional) overrides the derived
// clean/dirty state during a save lifecycle.
function statusModel({ describe, cursor, diagnostics, advisories, saveState } = {}) {
  const d = describe || { open: false };
  const effective = saveState || (d.dirty ? SAVE_STATE.DIRTY : SAVE_STATE.CLEAN);
  return {
    open: !!d.open,
    fileName: d.open ? basename(d.sourcePath) : '',
    sourcePath: d.open ? d.sourcePath : '',
    format: d.open ? formatLabel(d.format) : '',
    gzip: !!(d.open && d.gzip),
    dirty: !!(d.open && d.dirty),
    cursor: cursorLabel(cursor),
    diagnosticCount: (diagnostics || []).length,
    advisoryCount: (advisories || []).length,
    saveState: effective,
    saveLabel: saveStateLabel(effective),
    context: d.context || 'generic',
  };
}

// Toolbar enable/disable. Save is offered only when there is something to save;
// everything else needs an open, non-saving document.
function toolbarModel({ open = false, dirty = false, saving = false } = {}) {
  const active = open && !saving;
  return {
    save: { enabled: active && dirty },
    saveAs: { enabled: active },
    reload: { enabled: active },
    undo: { enabled: active },
    redo: { enabled: active },
    find: { enabled: active },
    replace: { enabled: active },
    gotoLine: { enabled: active },
    close: { enabled: open && !saving },
    external: { enabled: active },
  };
}

// Flatten the nested AST outline into rows carrying depth + a navigation target
// (the source range to reveal on click).
function flattenOutline(outline, depth = 0, into = []) {
  for (const e of outline || []) {
    into.push({ label: e.label, kind: e.kind, depth, from: e.from, to: e.to, line: e.line });
    if (e.children && e.children.length) flattenOutline(e.children, depth + 1, into);
  }
  return into;
}

// Where a Back button returns to, from the document's originating context.
function originNav(context) {
  if (context === 'world') return { page: 'world', label: 'Back to World Project' };
  if (context === 'mall') return { page: 'mall', label: 'Back to Mall Item' };
  return { page: 'mall', label: 'Back' };
}

// Conflict-dialog choice (external change detected) -> the action to take.
const CONFLICT_ACTION = Object.freeze({ RELOAD: 'reload', SAVE_AS: 'saveAs', CANCEL: 'cancel' });
function conflictDecision(choice) {
  switch (choice) {
    case 'reload': return { action: CONFLICT_ACTION.RELOAD, discardsBuffer: true };
    case 'saveAs': return { action: CONFLICT_ACTION.SAVE_AS, discardsBuffer: false };
    default: return { action: CONFLICT_ACTION.CANCEL, discardsBuffer: false };
  }
}

// Does closing (or reloading) now require an "unsaved changes" confirmation?
function needsUnsavedConfirm({ dirty } = {}) { return !!dirty; }

// App-level keyboard shortcut resolver. CodeMirror owns Undo/Redo/Find/Replace
// through its own keymap; these are the app-level accelerators editor.js
// intercepts. Returns a command name or null.
function resolveShortcut({ key, ctrlOrMeta, shift } = {}) {
  if (!ctrlOrMeta) return null;
  const k = String(key || '').toLowerCase();
  if (k === 's') return shift ? 'saveAs' : 'save';
  if (k === 'g' && !shift) return 'gotoLine';
  if (k === 'w' && !shift) return 'close';
  // Zoom accelerators: Ctrl/Cmd with +/=/-/0 (both the shifted and unshifted
  // punctuation forms are accepted so any keyboard layout reaches them).
  if (k === '=' || k === '+' || k === 'add') return 'zoomIn';
  if (k === '-' || k === '_' || k === 'subtract') return 'zoomOut';
  if (k === '0') return 'zoomReset';
  // Live-preview accelerators (Phase 7C2): Ctrl/Cmd+Enter updates the preview,
  // Ctrl/Cmd+Shift+Enter toggles preview-maximize.
  if (k === 'enter') return shift ? 'previewMaximize' : 'previewUpdate';
  return null;
}

// Stale-analysis rejection: apply an analysis result only if it is at least as
// new as the last one applied (a late/out-of-order callback for an older buffer
// version is dropped). Belt-and-suspenders over the editor view's own guard.
function isFreshAnalysis(resultVersion, appliedVersion) {
  return typeof resultVersion === 'number' && resultVersion >= (appliedVersion || 0);
}

// --- WD2-C workspace mode -----------------------------------------------------
// 'model' = visual-primary (large preview, Add/Properties visible, Source
// collapsible); 'code' = the source-primary layout. A document whose every
// line is blank or a comment (empty, whitespace-only, header-only) holds no
// scene yet, so it opens in Model whatever was remembered; any other document
// opens in the remembered mode. Text-level only: no parsing here.
const UI_WORKSPACE_MODES = Object.freeze(['code', 'model']);
function isEmptyScene(text) {
  if (typeof text !== 'string') return false;
  return text.split(/\r\n|\n|\r/).every((line) => /^[ \t,]*(#.*)?$/.test(line));
}
function initialWorkspaceMode({ text, remembered } = {}) {
  if (isEmptyScene(text)) return 'model';
  return UI_WORKSPACE_MODES.includes(remembered) ? remembered : 'code';
}

const API = {
  DIAG_CAP, SAVE_STATE, CONFLICT_ACTION, THEMES, DEFAULT_THEME,
  ZOOM_MIN, ZOOM_MAX, ZOOM_DEFAULT,
  formatLabel, isDirty, saveStateLabel, basename, cursorLabel,
  capDiagnostics, statusModel, toolbarModel, flattenOutline,
  originNav, conflictDecision, needsUnsavedConfirm, resolveShortcut, isFreshAnalysis,
  isValidTheme, resolveTheme, clampZoom, resolveZoom, zoomStep, zoomModel,
  // Phase 7C2 live-preview view-models.
  PREVIEW_LAYOUTS, PREVIEW_LAYOUT_DEFAULT, SPLIT_MIN, SPLIT_MAX, SPLIT_DEFAULT,
  resolvePreviewLayout, clampSplit, splitStep, previewLayoutModel,
  togglePreviewMaximize, previewStatusModel,
  // WD2-C workspace mode.
  WORKSPACE_MODES: UI_WORKSPACE_MODES, isEmptyScene, initialWorkspaceMode,
};

// Dual use: CommonJS for Node unit tests, a window global for the renderer (this
// file is loaded by a plain <script> in editor.html -- no bundler on that page).
if (typeof module !== 'undefined' && module.exports) {
  module.exports = API;
} else {
  window.WrlEditorUI = API;
}
