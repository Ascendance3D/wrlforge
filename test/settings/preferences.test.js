'use strict';
// Phase: Preferences & Settings -- the pure model tests.
// No DOM, no fs, no Electron. Each test exercises one transition / read /
// write rule from src/settings/preferences.js.

const test = require('node:test');
const assert = require('node:assert/strict');
const {
  DEFAULTS, THEMES, PREVIEW_LAYOUTS, ZOOM_MIN, ZOOM_MAX,
  THEME_LABELS, PREVIEW_LAYOUT_LABELS,
  isValidTheme, isValidPreviewLayout, clampZoom,
  normalize, read, write, update,
  setHighContrastEnabled, highContrastEnabled, zoomModel,
  KEY_THEME, KEY_ZOOM, KEY_PREVIEW_LAYOUT, KEY_LAST_NON_CONTRAST_THEME,
} = require('../../src/settings/preferences');

// The shared namespaced names (PREF_THEMES, PREF_ZOOM_MIN, PREF_ZOOM_MAX,
// PREF_PREVIEW_LAYOUTS) are not re-exported on purpose: outside the
// renderer they're just the public `THEMES`, `PREVIEW_LAYOUTS`, `ZOOM_MIN`,
// `ZOOM_MAX` aliases. The renderer uses them under the local PREF_* names
// so the shared classic-script scope of editor.html does not collide
// with src/editor/ui-state.js.

// ---- defaults & invariants --------------------------------------------------

test('DEFAULTS: theme is dark, zoom 0, previewLayout split, lastNonContrastTheme dark', () => {
  assert.equal(DEFAULTS.theme, 'dark');
  assert.equal(DEFAULTS.zoom, 0);
  assert.equal(DEFAULTS.previewLayout, 'split');
  assert.equal(DEFAULTS.lastNonContrastTheme, 'dark');
});

test('THEMES has exactly five values, including contrast', () => {
  assert.deepEqual([...THEMES].sort(), ['contrast', 'dark', 'light', 'terminal', 'tokyo']);
});

test('PREVIEW_LAYOUTS has exactly three values, including editor-only', () => {
  assert.deepEqual([...PREVIEW_LAYOUTS].sort(), ['editor-only', 'preview-max', 'split']);
});

test('THEME_LABELS has every theme id keyed to a non-empty label', () => {
  for (const t of THEMES) {
    assert.equal(typeof THEME_LABELS[t], 'string');
    assert.ok(THEME_LABELS[t].length > 0, 'theme ' + t + ' has empty label');
  }
});

test('PREVIEW_LAYOUT_LABELS has every layout id keyed to a non-empty label', () => {
  for (const l of PREVIEW_LAYOUTS) {
    assert.equal(typeof PREVIEW_LAYOUT_LABELS[l], 'string');
    assert.ok(PREVIEW_LAYOUT_LABELS[l].length > 0, 'layout ' + l + ' has empty label');
  }
});

// ---- validation primitives --------------------------------------------------

test('isValidTheme accepts every THEMES entry and rejects others', () => {
  for (const t of THEMES) assert.equal(isValidTheme(t), true);
  assert.equal(isValidTheme('hot-pink'), false);
  assert.equal(isValidTheme(null), false);
  assert.equal(isValidTheme(undefined), false);
  assert.equal(isValidTheme(42), false);
  assert.equal(isValidTheme(''), false);
});

test('isValidPreviewLayout accepts every PREVIEW_LAYOUTS entry and rejects others', () => {
  for (const l of PREVIEW_LAYOUTS) assert.equal(isValidPreviewLayout(l), true);
  assert.equal(isValidPreviewLayout('half'), false);
  assert.equal(isValidPreviewLayout(null), false);
  assert.equal(isValidPreviewLayout(''), false);
});

test('clampZoom rounds and clamps', () => {
  assert.equal(clampZoom(0), 0);
  assert.equal(clampZoom(-3), -3);
  assert.equal(clampZoom(8), 8);
  assert.equal(clampZoom(-100), ZOOM_MIN);
  assert.equal(clampZoom(100), ZOOM_MAX);
  assert.equal(clampZoom(1.6), 2);
  assert.equal(clampZoom('not a number'), DEFAULTS.zoom);
  assert.equal(clampZoom(null), DEFAULTS.zoom);
  assert.equal(clampZoom(undefined), DEFAULTS.zoom);
});

// ---- normalize: per-field independence -------------------------------------

test('normalize: missing object -> DEFAULTS', () => {
  assert.deepEqual(normalize(undefined), DEFAULTS);
  assert.deepEqual(normalize(null), DEFAULTS);
  assert.deepEqual(normalize({}), DEFAULTS);
});

test('normalize: valid input passes through unchanged', () => {
  const v = { theme: 'tokyo', zoom: 4, previewLayout: 'preview-max', lastNonContrastTheme: 'dark', workspaceMode: 'model' };
  assert.deepEqual(normalize(v), v);
});

test('normalize: each bad field falls back independently', () => {
  const v = { theme: 'hot-pink', zoom: 'NaN', previewLayout: 'half', lastNonContrastTheme: '??? ' };
  const out = normalize(v);
  assert.equal(out.theme, DEFAULTS.theme);
  assert.equal(out.zoom, DEFAULTS.zoom);
  assert.equal(out.previewLayout, DEFAULTS.previewLayout);
  assert.equal(out.lastNonContrastTheme, DEFAULTS.lastNonContrastTheme);
});

test('normalize: lastNonContrastTheme rejects contrast (the field must be non-contrast)', () => {
  assert.equal(normalize({ lastNonContrastTheme: 'contrast' }).lastNonContrastTheme, DEFAULTS.lastNonContrastTheme);
  assert.equal(normalize({ lastNonContrastTheme: 'dark' }).lastNonContrastTheme, 'dark');
  assert.equal(normalize({ lastNonContrastTheme: 'tokyo' }).lastNonContrastTheme, 'tokyo');
});

test('normalize: bad zoom falls back to default (not the raw value)', () => {
  assert.equal(normalize({ zoom: 99 }).zoom, ZOOM_MAX);
  assert.equal(normalize({ zoom: -99 }).zoom, ZOOM_MIN);
  assert.equal(normalize({ zoom: 'abc' }).zoom, DEFAULTS.zoom);
});

// ---- read: storage indirection ---------------------------------------------

function makeStorage(initial) {
  const map = new Map(Object.entries(initial || {}));
  return {
    getItem(k) { return map.has(k) ? map.get(k) : null; },
    setItem(k, v) { map.set(k, String(v)); },
  };
}

test('read: empty / missing storage -> DEFAULTS', () => {
  assert.deepEqual(read(null), DEFAULTS);
  assert.deepEqual(read(undefined), DEFAULTS);
  assert.deepEqual(read(makeStorage()), DEFAULTS);
});

test('read: parses the four known keys into the right fields', () => {
  const s = makeStorage({
    [KEY_THEME]: 'tokyo',
    [KEY_ZOOM]: '5',
    [KEY_PREVIEW_LAYOUT]: 'preview-max',
    [KEY_LAST_NON_CONTRAST_THEME]: 'light',
  });
  // WD2-C added workspaceMode; absent from storage -> its default.
  assert.deepEqual(read(s), { theme: 'tokyo', zoom: 5, previewLayout: 'preview-max', lastNonContrastTheme: 'light', workspaceMode: 'code' });
});

test('read: tolerates throwing storage', () => {
  const bad = { getItem() { throw new Error('blocked'); } };
  assert.deepEqual(read(bad), DEFAULTS);
});

test('read: a bad value in one key does not block the others', () => {
  const s = makeStorage({
    [KEY_THEME]: 'hot-pink',
    [KEY_ZOOM]: '4',
    [KEY_PREVIEW_LAYOUT]: 'split',
    [KEY_LAST_NON_CONTRAST_THEME]: 'dark',
  });
  const out = read(s);
  assert.equal(out.theme, DEFAULTS.theme);
  assert.equal(out.zoom, 4);
  assert.equal(out.previewLayout, 'split');
  assert.equal(out.lastNonContrastTheme, 'dark');
});

// ---- write ------------------------------------------------------------------

test('write: persists all four keys as strings', () => {
  const s = makeStorage();
  write(s, { theme: 'terminal', zoom: 3, previewLayout: 'split', lastNonContrastTheme: 'light' });
  assert.equal(s.getItem(KEY_THEME), 'terminal');
  assert.equal(s.getItem(KEY_ZOOM), '3');
  assert.equal(s.getItem(KEY_PREVIEW_LAYOUT), 'split');
  assert.equal(s.getItem(KEY_LAST_NON_CONTRAST_THEME), 'light');
});

test('write: null / undefined prefs is a no-op (never crashes the app)', () => {
  const s = makeStorage();
  write(s, null);
  write(s, undefined);
  assert.equal(s.getItem(KEY_THEME), null);
});

test('write: tolerates throwing storage (best-effort)', () => {
  const bad = { setItem() { throw new Error('quota'); } };
  // Should NOT throw.
  write(bad, { theme: 'light', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'light' });
});

// ---- update: per-key state transitions --------------------------------------

test('update: theme transition to contrast remembers the prior non-contrast theme', () => {
  const out = update({ theme: 'tokyo', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'dark' }, 'theme', 'contrast');
  assert.equal(out.theme, 'contrast');
  assert.equal(out.lastNonContrastTheme, 'tokyo');
});

test('update: no-op theme write (theme == cur.theme) does not overwrite the memory', () => {
  const before = { theme: 'dark', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'light' };
  const after = update(before, 'theme', 'dark');
  assert.equal(after.theme, 'dark');
  assert.equal(after.lastNonContrastTheme, 'light');
});

test('update: switch contrast -> contrast keeps lastNonContrastTheme intact', () => {
  const before = { theme: 'contrast', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'tokyo' };
  const after = update(before, 'theme', 'contrast');
  assert.equal(after.lastNonContrastTheme, 'tokyo');
});

test('update: switch contrast -> non-contrast does not touch lastNonContrastTheme', () => {
  const before = { theme: 'contrast', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'tokyo' };
  const after = update(before, 'theme', 'light');
  assert.equal(after.theme, 'light');
  assert.equal(after.lastNonContrastTheme, 'tokyo');
});

test('update: invalid theme value falls back to default', () => {
  const out = update(DEFAULTS, 'theme', 'hot-pink');
  assert.equal(out.theme, DEFAULTS.theme);
});

test('update: zoom transition clamps and rounds', () => {
  assert.equal(update(DEFAULTS, 'zoom', 1.6).zoom, 2);
  assert.equal(update(DEFAULTS, 'zoom', 100).zoom, ZOOM_MAX);
  assert.equal(update(DEFAULTS, 'zoom', -100).zoom, ZOOM_MIN);
  assert.equal(update(DEFAULTS, 'zoom', 'abc').zoom, DEFAULTS.zoom);
});

test('update: previewLayout rejects unknown values', () => {
  assert.equal(update(DEFAULTS, 'previewLayout', 'half').previewLayout, DEFAULTS.previewLayout);
  assert.equal(update(DEFAULTS, 'previewLayout', 'split').previewLayout, 'split');
});

test('update: lastNonContrastTheme rejects contrast', () => {
  const out = update(DEFAULTS, 'lastNonContrastTheme', 'contrast');
  assert.equal(out.lastNonContrastTheme, DEFAULTS.lastNonContrastTheme);
  const out2 = update(DEFAULTS, 'lastNonContrastTheme', 'terminal');
  assert.equal(out2.lastNonContrastTheme, 'terminal');
});

test('update: returns a NEW object (never mutates the input)', () => {
  const before = { theme: 'dark', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'dark' };
  const after = update(before, 'zoom', 3);
  assert.notEqual(after, before);
  assert.deepEqual(before, { theme: 'dark', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'dark' });
  assert.equal(after.zoom, 3);
});

// ---- highContrastEnabled + setHighContrastEnabled --------------------------

test('highContrastEnabled: derived purely from theme === contrast', () => {
  assert.equal(highContrastEnabled({ theme: 'contrast', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'dark' }), true);
  assert.equal(highContrastEnabled({ theme: 'dark', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'dark' }), false);
  assert.equal(highContrastEnabled(null), false);
});

test('setHighContrastEnabled(off -> on): switches to contrast and remembers prior theme', () => {
  const before = { theme: 'tokyo', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'dark' };
  const after = setHighContrastEnabled(before, true);
  assert.equal(after.theme, 'contrast');
  assert.equal(after.lastNonContrastTheme, 'tokyo');
});

test('setHighContrastEnabled(on -> off): reverts to lastNonContrastTheme', () => {
  const before = { theme: 'contrast', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'tokyo' };
  const after = setHighContrastEnabled(before, false);
  assert.equal(after.theme, 'tokyo');
});

test('setHighContrastEnabled: double-on is idempotent (no state churn)', () => {
  const before = { theme: 'contrast', zoom: 2, previewLayout: 'split', lastNonContrastTheme: 'tokyo', workspaceMode: 'code' };
  const after = setHighContrastEnabled(before, true);
  assert.deepEqual(after, before);
});

test('setHighContrastEnabled: off when lastNonContrastTheme is contrast (legacy) falls back to default', () => {
  // A legacy or corrupted record may have lastNonContrastTheme = 'contrast'.
  // Off must NEVER leave theme === 'contrast' -- it must revert to the
  // default non-contrast theme.
  const before = { theme: 'contrast', zoom: 0, previewLayout: 'split', lastNonContrastTheme: 'contrast' };
  const after = setHighContrastEnabled(before, false);
  assert.equal(after.theme, DEFAULTS.theme);
});

test('setHighContrastEnabled: toggle round-trip preserves zoom and previewLayout', () => {
  const before = { theme: 'tokyo', zoom: 4, previewLayout: 'preview-max', lastNonContrastTheme: 'dark' };
  const a = setHighContrastEnabled(before, true);
  assert.equal(a.theme, 'contrast');
  assert.equal(a.zoom, 4);
  assert.equal(a.previewLayout, 'preview-max');
  const b = setHighContrastEnabled(a, false);
  assert.equal(b.theme, 'tokyo');
  assert.equal(b.zoom, 4);
  assert.equal(b.previewLayout, 'preview-max');
});

// ---- zoomModel --------------------------------------------------------------

test('zoomModel: 0 -> 100% / 13px / factor 1.0', () => {
  assert.deepEqual(zoomModel(0), { level: 0, chromeScale: 1, codeFontPx: 13, label: '100%' });
});

test('zoomModel: -3 -> 70%', () => {
  const m = zoomModel(-3);
  assert.equal(m.level, -3);
  assert.equal(Math.round(m.chromeScale * 100), 70);
  assert.equal(m.label, '70%');
});

test('zoomModel: 8 -> 180%', () => {
  const m = zoomModel(8);
  assert.equal(m.level, 8);
  assert.equal(Math.round(m.chromeScale * 100), 180);
  assert.equal(m.label, '180%');
});

test('zoomModel: 1 -> 110%', () => {
  const m = zoomModel(1);
  assert.equal(m.label, '110%');
});

test('zoomModel: invalid input clamps to default 0', () => {
  assert.equal(zoomModel('abc').level, 0);
  assert.equal(zoomModel(null).level, 0);
  assert.equal(zoomModel(undefined).level, 0);
});

// ---- WD2-C: workspaceMode ----------------------------------------------------

test('WD2-C workspaceMode: default is code; model/code accepted; garbage falls back', () => {
  const P = require('../../src/settings/preferences.js');
  assert.equal(P.DEFAULTS.workspaceMode, 'code');
  assert.deepEqual([...P.WORKSPACE_MODES].sort(), ['code', 'model']);
  assert.equal(normalize({ workspaceMode: 'model' }).workspaceMode, 'model');
  assert.equal(normalize({ workspaceMode: 'visual' }).workspaceMode, 'code');
  const s = makeStorage({ [P.KEY_WORKSPACE_MODE]: 'model' });
  assert.equal(read(s).workspaceMode, 'model');
});

test('WD2-C workspaceMode: update + write round-trip through the same storage keys', () => {
  const P = require('../../src/settings/preferences.js');
  const a = normalize({});
  const b = update(a, 'workspaceMode', 'model');
  assert.equal(b.workspaceMode, 'model');
  assert.equal(update(b, 'workspaceMode', 'model'), b, 'a no-op write returns the same object');
  assert.equal(update(b, 'workspaceMode', 'nonsense').workspaceMode, 'model', 'an invalid value changes nothing');
  const s = makeStorage({});
  write(s, b);
  assert.equal(read(s).workspaceMode, 'model');
  assert.equal(P.KEY_WORKSPACE_MODE, 'wrlforge.editor.workspaceMode');
});
