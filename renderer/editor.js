'use strict';
// Native editor workspace binding (Phase 7B). Thin DOM glue: all decisions come
// from window.WrlEditorUI (pure, unit-tested), the editing surface from
// window.WrlEditor (CodeMirror bundle), and every filesystem/path action from
// window.vrmlpad.editor (the confined main-process controller). The renderer is
// the source of truth for buffer text (no IPC per keystroke); it pushes the
// buffer to main only on save/navigate, and computes dirty locally vs. baseline.

const UI = window.WrlEditorUI;
const bridge = window.vrmlpad.editor;
const SceneSelection = window.WRLForgeSceneSelection;
const SceneTreeView = window.WRLForgeSceneTree;
const InspectorView = window.WRLForgeInspector;
const ModelWorkspace = window.WRLForgeModelWorkspace;
const sceneBridge = window.WRLForgeSceneBridge; // from the bundled editor view

const el = (id) => document.getElementById(id);
const els = {
  back: el('backBtn'), save: el('saveBtn'), saveAs: el('saveAsBtn'), reload: el('reloadBtn'),
  undo: el('undoBtn'), redo: el('redoBtn'), find: el('findBtn'), replace: el('replaceBtn'),
  goto: el('gotoBtn'), external: el('externalBtn'), close: el('closeBtn'),
  editor: el('editor'), msg: el('editorMsg'),
  outlineList: el('outlineList'), diagList: el('diagList'), advList: el('advList'),
  diagCount: el('diagCount'), advCount: el('advCount'),
  sceneTree: el('sceneTree'), sceneInspector: el('sceneInspector'),
  stFile: el('stFile'), stFormat: el('stFormat'), stDirty: el('stDirty'), stSave: el('stSave'),
  stCursor: el('stCursor'), stDiag: el('stDiag'), stAdv: el('stAdv'),
  themeSelect: el('themeSelect'),
  zoomOut: el('zoomOutBtn'), zoomIn: el('zoomInBtn'), zoomReset: el('zoomResetBtn'),
  zoomLabel: el('zoomLabel'),
  // WD2-C: Model workspace.
  main: el('editorMain'),
  modelBtn: el('modeModelBtn'), codeBtn: el('modeCodeBtn'), sourceBtn: el('sourceToggleBtn'),
  addBox: el('addBoxBtn'), addSphere: el('addSphereBtn'),
  duplicate: el('duplicateBtn'), remove: el('deleteBtn'),
  modelSelected: el('modelSelected'), modelStatus: el('modelStatus'), objectProps: el('objectProps'),
};

// Phase: Preferences & Settings -- the theme, zoom, and preview layout
// controls below all read and write through the shared preferences model
// (window.WrlPreferences). The same module is used by the Mall, World, and
// Editor toolbars, so the value the user picks here is the value the
// Preferences & Settings dialog shows (and vice versa). There is no
// shadow value. The dialog's "Open" / theme / zoom handlers subscribe
// to the same model, so an edit from any surface updates the others live.

// Apply a zoom level: scale the app chrome via the --wrl-ui-scale CSS variable,
// resize the code font via the editor handle, update the label, and persist.
function applyZoom(level) {
  const z = UI.zoomModel(level);
  S.zoom = z.level;
  document.documentElement.style.setProperty('--wrl-ui-scale', String(z.chromeScale));
  if (S.handle) S.handle.setFontSize(z.codeFontPx);
  if (els.zoomLabel) {
    els.zoomLabel.textContent = z.label;
    els.zoomLabel.setAttribute('aria-label', 'Editor size ' + z.label);
  }
  // Persist + notify via the shared preferences model. set() is a no-op
  // when the level is already current, so the loop never re-fires.
  if (window.WrlPreferences) window.WrlPreferences.set('zoom', z.level);
}

function savedTheme() {
  if (window.WrlPreferences) {
    const t = window.WrlPreferences.get('theme');
    return UI.resolveTheme(t);
  }
  return UI.DEFAULT_THEME;
}
function savedZoom() {
  if (window.WrlPreferences) {
    const z = window.WrlPreferences.get('zoom');
    return UI.resolveZoom(z);
  }
  return UI.ZOOM_DEFAULT;
}

// --- renderer-local editor state --------------------------------------------
const S = {
  handle: null,
  sessionId: null,
  context: 'generic',
  sourcePath: '',
  format: 'plain',
  gzip: false,
  baseline: '',           // opened / last-saved text; dirty = current !== baseline
  cursor: { line: 1, column: 1 },
  diagnostics: [],
  advisories: [],
  outline: [],
  appliedAnalysisVersion: 0,
  saving: false,
  saveState: null,        // overrides derived clean/dirty during the save lifecycle
  zoom: 0,                // current zoom level (persisted); scales code + chrome
  bufferVersion: 0,       // monotonic per-edit counter promoted onto preview requests
  // WD2-A -- scene-tree read model + structured findings. Built once per
  // analysis and consumed by both the scene-tree view and the inspector.
  // findings holds the ORDERED (by P4-A) presentation results; the inspector
  // filters them by the selected item's range.
  sceneTree: null,
  findings: [],
  // WD2-B -- the parse session (WD1.4) binding the analysed text to the parse
  // the scene tree was built from, and the verified receipt of an Inspector
  // Apply dispatched since that analysis. Both are ephemeral and never leave
  // the renderer; selection survival re-anchors only through them.
  analysisSession: null,
  pendingApply: null,
  // WD2-C -- the workspace for THIS document ('model' | 'code'; the remembered
  // preference is only written when the user switches), whether Source is
  // expanded in Model, display-only friendly labels for the current tree, and
  // whether the damaged-document notice has been shown for the current error
  // state (so Source opens once, not on every analysis).
  workspaceMode: 'code',
  sourceOpen: false,
  displayLabels: new Map(),
  damaged: false,
};

// WD2-A -- one selection authority shared by the scene-tree view and the
// inspector. Constructed at module load so both views see the same state.
const sceneSelection = SceneSelection.createSelectionController();

let sceneTreeView = null;
let inspectorView = null;
let modelWorkspace = null;

function currentText() { return S.handle ? S.handle.getText() : S.baseline; }
function isDirty() { return UI.isDirty(currentText(), S.baseline); }

// Phase Beta 2 -- record the dirty buffer to the recovery store on every
// change. We THROTTLE on the renderer side (1.5s trailing) so the IPC is
// never per-keystroke; main still debounces (5s) before it actually writes
// to disk. The data carried matches the recovery-store schema (see
// src/editor/recovery-store.js).
const RECOVERY_THROTTLE_MS = 1500;
const RECOVERY_WORKSPACE_KEY = 'wrlforge.recovery.lastWorkspace';
let _recoveryPending = null;
let _recoveryTimer = null;
function scheduleRecoverySnapshot() {
  if (!S.handle || !S.sessionId) return;
  const payload = {
    sourcePath: S.sourcePath || null,
    context: S.context || 'generic',
    profile: S.context === 'mall' ? 'mall-item' : S.context === 'world' ? 'world' : 'generic',
    root: null,
    format: S.gzip ? 'gzip' : 'plain',
    baseline: S.baseline || '',
    buffer: currentText(),
    dirty: true,
    activeWorkspace: 'editor',
  };
  _recoveryPending = payload;
  if (_recoveryTimer) return;
  _recoveryTimer = setTimeout(async () => {
    _recoveryTimer = null;
    const p = _recoveryPending;
    _recoveryPending = null;
    if (!p) return;
    try {
      await bridge.recoveryRecordDirty(p);
    } catch (e) { /* best-effort; the next edit reschedules */ }
  }, RECOVERY_THROTTLE_MS);
}
// Force-flush any pending recovery snapshot (used on Close and Back so the
// last keystrokes survive a navigate-away even if the timer hasn't fired).
async function flushRecoverySnapshot() {
  if (_recoveryTimer) {
    clearTimeout(_recoveryTimer);
    _recoveryTimer = null;
  }
  if (!_recoveryPending) return;
  const p = _recoveryPending;
  _recoveryPending = null;
  try { await bridge.recoveryRecordDirty(p); } catch (e) { /* best-effort */ }
}

// The live-preview orchestrator (renderer/editor-preview.js), present only when
// the preview lane is loaded. Guarded so the editor works without it.
const EP = () => window.wrlEditorPreview;

// --- rendering ---------------------------------------------------------------
function render() {
  const describe = {
    open: !!S.handle, sourcePath: S.sourcePath, format: S.format, gzip: S.gzip,
    dirty: isDirty(), context: S.context,
  };
  const status = UI.statusModel({
    describe, cursor: S.cursor, diagnostics: S.diagnostics, advisories: S.advisories, saveState: S.saveState,
  });
  const tb = UI.toolbarModel({ open: status.open, dirty: status.dirty, saving: S.saving });

  els.save.disabled = !tb.save.enabled;
  els.saveAs.disabled = !tb.saveAs.enabled;
  els.reload.disabled = !tb.reload.enabled;
  els.undo.disabled = !tb.undo.enabled;
  els.redo.disabled = !tb.redo.enabled;
  els.find.disabled = !tb.find.enabled;
  els.replace.disabled = !tb.replace.enabled;
  els.goto.disabled = !tb.gotoLine.enabled;
  els.external.disabled = !tb.external.enabled;
  els.close.disabled = !tb.close.enabled;

  els.stFile.textContent = status.fileName || '—';
  els.stFile.title = status.sourcePath || '';
  els.stFormat.textContent = status.open ? status.format : '';
  els.stDirty.innerHTML = status.dirty ? '<span class="dirty-dot">●</span> Modified' : '';
  els.stSave.textContent = status.saveLabel;
  els.stSave.className = 'seg save-' + status.saveState;
  els.stCursor.textContent = status.cursor;
  els.stDiag.textContent = String(status.diagnosticCount);
  els.stAdv.textContent = String(status.advisoryCount);

  const back = UI.originNav(S.context);
  els.back.textContent = '← ' + back.label;

  renderOutline();
  renderDiagnostics();
  renderSceneTree();
  if (modelWorkspace) modelWorkspace.refresh();
}

// WD2-A: scene-tree + inspector re-render. Both consume S.sceneTree and
// S.findings. The inspector filters by the currently selected item's range.
function renderSceneTree() {
  if (!sceneTreeView || !inspectorView) return;
  sceneTreeView.setSceneTree(S.sceneTree);
  // Inspector needs the findings list; pass it as a closure-captured
  // dependency so it picks up the latest findings on every render.
  inspectorView.setFindings(S.findings);
  inspectorView.setSceneTree(S.sceneTree);
}

function clearChildren(node) { while (node.firstChild) node.removeChild(node.firstChild); }

function renderOutline() {
  clearChildren(els.outlineList);
  const rows = UI.flattenOutline(S.outline);
  if (!rows.length) {
    els.outlineList.appendChild(emptyNote(S.handle ? 'No nodes.' : 'No document.'));
    return;
  }
  for (const r of rows) {
    const div = document.createElement('div');
    div.className = 'row-item outline';
    div.style.paddingLeft = (12 + r.depth * 14) + 'px';
    const kind = document.createElement('span');
    kind.className = 'kind';
    kind.textContent = r.kind === 'node' ? '' : r.kind;
    const label = document.createElement('span');
    label.textContent = r.label;
    div.appendChild(kind);
    div.appendChild(label);
    div.tabIndex = 0;
    const go = () => navigateTo(r.from, r.to);
    div.addEventListener('click', go);
    div.addEventListener('keydown', (e) => { if (e.key === 'Enter') go(); });
    els.outlineList.appendChild(div);
  }
}

function renderDiagnostics() {
  const diag = UI.capDiagnostics(S.diagnostics, UI.DIAG_CAP);
  const adv = UI.capDiagnostics(S.advisories, UI.DIAG_CAP);
  els.diagCount.textContent = String(diag.total);
  els.advCount.textContent = String(adv.total);
  fillDiagList(els.diagList, diag, 'No syntax diagnostics.', false);
  fillDiagList(els.advList, adv, 'No advisories.', true);
}

function fillDiagList(container, capped, emptyText, advisory) {
  clearChildren(container);
  if (!capped.total) { container.appendChild(emptyNote(emptyText)); return; }
  for (const d of capped.shown) {
    const div = document.createElement('div');
    div.className = 'row-item ' + (advisory ? 'advisory' : 'sev-' + (d.severity || 'error'));
    const loc = document.createElement('span');
    loc.className = 'loc';
    loc.textContent = `${d.line || 1}:${d.column || 1}`;
    const msg = document.createElement('span');
    msg.textContent = d.message + (d.code ? ` (${d.code})` : '');
    div.appendChild(loc);
    div.appendChild(msg);
    div.tabIndex = 0;
    const go = () => navigateTo(d.from, d.to);
    div.addEventListener('click', go);
    div.addEventListener('keydown', (e) => { if (e.key === 'Enter') go(); });
    container.appendChild(div);
  }
  if (capped.capped) {
    const note = emptyNote(`Showing ${capped.shown.length} of ${capped.total}; ${capped.hidden} more not listed.`);
    container.appendChild(note);
  }
}

function emptyNote(text) {
  const n = document.createElement('div');
  n.className = 'empty-note';
  n.textContent = text;
  return n;
}

function navigateTo(from, to) {
  if (!S.handle || from == null) return;
  S.handle.revealRange(from, to == null ? from : to);
}

function showMsg(text, isErr) {
  els.msg.textContent = text;
  els.msg.className = 'editor-msg' + (isErr ? ' err' : '');
  els.msg.style.display = text ? 'block' : 'none';
}

// --- in-DOM modal (no native alert/confirm/prompt) --------------------------
function showModal({ title, message, buttons, input }) {
  return new Promise((resolve) => {
    const backdrop = el('modalBackdrop');
    el('modalTitle').textContent = title;
    el('modalMsg').textContent = message || '';
    const inputEl = el('modalInput');
    if (input) {
      inputEl.type = input.type || 'text';
      inputEl.value = input.value != null ? String(input.value) : '';
      inputEl.style.display = 'block';
    } else {
      inputEl.style.display = 'none';
    }
    const actions = el('modalActions');
    clearChildren(actions);
    const finish = (value) => { backdrop.classList.remove('show'); resolve(input ? { value, input: inputEl.value } : { value }); };
    for (const b of buttons) {
      const btn = document.createElement('button');
      btn.textContent = b.label;
      if (!b.primary) btn.className = 'secondary';
      btn.addEventListener('click', () => finish(b.value));
      actions.appendChild(btn);
    }
    backdrop.classList.add('show');
    (input ? inputEl : actions.lastChild).focus();
  });
}

async function confirmUnsaved(actionLabel) {
  if (!isDirty()) return true;
  const r = await showModal({
    title: 'Discard unsaved changes?',
    message: `This document has unsaved changes. ${actionLabel} anyway?`,
    buttons: [
      { label: actionLabel, value: 'ok', primary: true },
      { label: 'Keep editing', value: 'cancel' },
    ],
  });
  return r.value === 'ok';
}

// --- actions -----------------------------------------------------------------
async function doSave() {
  if (!S.handle || S.saving || !isDirty()) return;
  S.saving = true; S.saveState = UI.SAVE_STATE.SAVING; render();
  const text = currentText();
  let res;
  try {
    res = await bridge.save(S.sessionId, text, false);
  } catch (e) {
    S.saving = false; S.saveState = UI.SAVE_STATE.ERROR; showMsg('Save failed: ' + e.message, true); render(); return;
  }
  S.saving = false;
  if (res && res.ok) {
    S.baseline = text; S.format = res.format; S.saveState = UI.SAVE_STATE.SAVED; showMsg('', false);
    render();
    return;
  }
  if (res && res.code === 'EEXTERNAL') {
    S.saveState = UI.SAVE_STATE.CONFLICT; render();
    await resolveConflict();
    return;
  }
  S.saveState = UI.SAVE_STATE.ERROR; showMsg('Save failed: ' + (res && res.error || 'unknown error'), true); render();
}

async function resolveConflict() {
  const r = await showModal({
    title: 'File changed on disk',
    message: 'This file was modified outside WRL Forge since you opened it. Reload the on-disk version (discarding your edits), save your version to a new file, or cancel and keep editing.',
    buttons: [
      { label: 'Reload from disk', value: 'reload' },
      { label: 'Save As…', value: 'saveAs', primary: true },
      { label: 'Cancel', value: 'cancel' },
    ],
  });
  const decision = UI.conflictDecision(r.value);
  if (decision.action === UI.CONFLICT_ACTION.RELOAD) await doReload(true);
  else if (decision.action === UI.CONFLICT_ACTION.SAVE_AS) await doSaveAs();
  // Cancel: leave the buffer dirty; the conflict status remains.
}

async function doSaveAs() {
  if (!S.handle || S.saving) return;
  const text = currentText();
  S.saving = true; S.saveState = UI.SAVE_STATE.SAVING; render();
  let res;
  try {
    res = await bridge.saveAs(S.sessionId, text, null); // keep current format
  } catch (e) {
    S.saving = false; S.saveState = UI.SAVE_STATE.ERROR; showMsg('Save As failed: ' + e.message, true); render(); return;
  }
  S.saving = false;
  if (!res || res.canceled) { S.saveState = isDirty() ? UI.SAVE_STATE.DIRTY : UI.SAVE_STATE.CLEAN; render(); return; }
  if (res.ok) {
    S.baseline = text; S.sourcePath = res.sourcePath; S.format = res.format; S.gzip = res.format === 'gzip';
    S.saveState = UI.SAVE_STATE.SAVED; showMsg('', false); render();
  }
}

async function doReload(force) {
  if (!S.handle || S.saving) return;
  if (!force && !(await confirmUnsaved('Reload'))) return;
  let res;
  try {
    res = await bridge.reload(S.sessionId);
  } catch (e) {
    showMsg('Reload failed: ' + e.message, true); return;
  }
  S.handle.setDoc(res.text);
  S.baseline = res.text; S.format = res.format; S.gzip = res.format === 'gzip';
  S.saveState = UI.SAVE_STATE.CLEAN; showMsg('', false); render();
  // The buffer now matches the reloaded disk source: refresh the live preview.
  S.bufferVersion += 1;
  if (EP()) EP().onEdit();
}

async function doGotoLine() {
  if (!S.handle) return;
  const r = await showModal({
    title: 'Go to line',
    message: 'Enter a line number:',
    input: { type: 'number', value: S.cursor.line },
    buttons: [{ label: 'Go', value: 'ok', primary: true }, { label: 'Cancel', value: 'cancel' }],
  });
  if (r.value !== 'ok') return;
  const n = parseInt(r.input, 10);
  if (Number.isFinite(n) && n >= 1) S.handle.gotoLine(n);
}

async function doExternal() {
  try {
    const res = await bridge.openInExternal(S.sessionId);
    const st = res && res.editorStatus;
    if (st && st.launched === false && st.reason === 'not-found') {
      showMsg('⚠ ' + (st.hint || 'External editor not found.') +
        ' Set WRL_FORGE_EDITOR or editorCommand in settings.json to your editor path.', false);
    } else {
      showMsg('', false);
    }
  } catch (e) { showMsg('Could not launch the external editor: ' + e.message, true); }
}

async function doClose() {
  if (!(await confirmUnsaved('Close'))) return;
  const back = UI.originNav(S.context);
  if (EP()) EP().stop();  // tear down preview timers + drop the overlay in main
  // Phase Beta 2 -- flush the last keystroke before close so the recovery
  // snapshot (if any) reflects reality, then let main recordClear via close().
  await flushRecoverySnapshot();
  try { await bridge.close(S.sessionId); } catch (e) { /* nothing open */ }
  await window.vrmlpad.goto(back.page);
}

async function doBack() {
  // Preserve the (possibly unsaved) buffer in the main-process session so it
  // survives the page switch, then navigate. The session stays open, but the
  // preview overlay is dropped (the editor page is leaving); it re-registers on
  // return. This is the navigate-away cleanup path.
  const back = UI.originNav(S.context);
  if (S.handle && S.sessionId != null) {
    try { await bridge.setText(S.sessionId, currentText()); } catch (e) { /* session may be gone */ }
  }
  if (EP()) EP().stop();
  // Phase Beta 2 -- flush a final recovery snapshot before navigating away so
  // the user does not lose the last keystrokes if a crash happens mid-nav.
  await flushRecoverySnapshot();
  // Phase: Accessibility + Performance -- record the originating workspace's
  // primary action so the destination page can restore focus to it. The
  // sessionStorage key is session-scoped (cleared when the window closes),
  // not persistent cross-session state. The destination reads and clears it
  // exactly once on load.
  const focusId = back.page === 'world' ? 'nativeEditorBtn' : 'repackBtn';
  try { window.sessionStorage.setItem('wrlforge.nav.returnFocusId', focusId); } catch (e) { /* best-effort */ }
  await window.vrmlpad.goto(back.page);
}

// --- wiring ------------------------------------------------------------------
function wireButtons() {
  els.save.addEventListener('click', doSave);
  els.saveAs.addEventListener('click', doSaveAs);
  els.reload.addEventListener('click', () => doReload(false));
  els.undo.addEventListener('click', () => S.handle && S.handle.undo());
  els.redo.addEventListener('click', () => S.handle && S.handle.redo());
  els.find.addEventListener('click', () => S.handle && S.handle.openSearch());
  els.replace.addEventListener('click', () => S.handle && S.handle.openSearch());
  els.goto.addEventListener('click', doGotoLine);
  els.external.addEventListener('click', doExternal);
  els.close.addEventListener('click', doClose);
  els.back.addEventListener('click', doBack);
  if (els.zoomIn) els.zoomIn.addEventListener('click', () => applyZoom(UI.zoomStep(S.zoom, +1)));
  if (els.zoomOut) els.zoomOut.addEventListener('click', () => applyZoom(UI.zoomStep(S.zoom, -1)));
  if (els.zoomReset) els.zoomReset.addEventListener('click', () => applyZoom(UI.ZOOM_DEFAULT));

  // Phase: Preferences & Settings -- the single Preferences button on the
  // editor toolbar opens the shared dialog. The same dialog is reachable
  // from the Mall + World toolbars.
  const prefsBtn = document.getElementById('prefsBtn');
  if (prefsBtn && window.WrlPreferences && typeof window.WrlPreferences.createButton === 'function') {
    prefsBtn.addEventListener('click', () => {
      if (typeof window.WrlPreferences.show === 'function') window.WrlPreferences.show(prefsBtn);
      else window.WrlPreferences.createButton({ id: 'prefsBtn' }).click();
    });
  }

  // App-level accelerators (CodeMirror owns undo/redo/find/replace via its keymap).
  window.addEventListener('keydown', (e) => {
    const cmd = UI.resolveShortcut({ key: e.key, ctrlOrMeta: e.ctrlKey || e.metaKey, shift: e.shiftKey });
    if (!cmd) return;
    e.preventDefault();
    if (cmd === 'save') doSave();
    else if (cmd === 'saveAs') doSaveAs();
    else if (cmd === 'gotoLine') doGotoLine();
    else if (cmd === 'close') doClose();
    else if (cmd === 'zoomIn') applyZoom(UI.zoomStep(S.zoom, +1));
    else if (cmd === 'zoomOut') applyZoom(UI.zoomStep(S.zoom, -1));
    else if (cmd === 'zoomReset') applyZoom(UI.ZOOM_DEFAULT);
    else if (cmd === 'previewUpdate') { if (EP()) EP().manualUpdate(); }
    else if (cmd === 'previewMaximize') { if (EP()) EP().toggleMaximize(); }
  });
}

function populateThemes() {
  clearChildren(els.themeSelect);
  for (const t of UI.THEMES) {
    const opt = document.createElement('option');
    opt.value = t.id;
    opt.textContent = t.label;
    els.themeSelect.appendChild(opt);
  }
  els.themeSelect.value = savedTheme();
  els.themeSelect.addEventListener('change', () => {
    const id = UI.resolveTheme(els.themeSelect.value);
    // Write to the shared preferences model; subscribers (the dialog, future
    // surfaces) see the change immediately and the value is persisted.
    if (window.WrlPreferences) window.WrlPreferences.set('theme', id);
    if (S.handle) S.handle.setTheme(id);
  });
}

function mountEditor(text, profile) {
  S.handle = window.WrlEditor.create(els.editor, {
    doc: text,
    profile,
    theme: savedTheme(),
    fontSize: UI.zoomModel(savedZoom()).codeFontPx,
    onChange: () => {
      if (S.saveState !== UI.SAVE_STATE.CONFLICT) S.saveState = null;
      S.bufferVersion += 1;               // monotonic; promoted onto preview requests
      if (EP()) EP().onEdit();            // schedule a debounced live-preview refresh
      scheduleRecoverySnapshot();         // Phase Beta 2 -- throttle a recovery write
      render();
    },
    onCursor: (c) => { S.cursor = c; els.stCursor.textContent = UI.cursorLabel(c); },
    onAnalysis: (a) => {
      if (!UI.isFreshAnalysis(a.version, S.appliedAnalysisVersion)) return;
      S.appliedAnalysisVersion = a.version;
      // WD2-B -- the analysis the current selection was made against.
      const previousAnalysis = S.analysisSession && S.sceneTree
        ? { session: S.analysisSession, tree: S.sceneTree }
        : null;
      S.diagnostics = a.diagnostics; S.advisories = a.advisories; S.outline = a.outline;
      // WD2-A -- build the scene tree + structured findings from the SAME
      // parse the diagnostics come from (a.parseResult). The renderer never
      // parses source text on its own.
      if (a.parseResult && sceneBridge && sceneBridge.sceneTree) {
        // 1. Build the scope graph FIRST. findingsForDocument requires one --
        //    a raw parseResult is not a graph and throws ESCOPEGRAPH, which
        //    we now surface visibly instead of silently turning into [].
        let graph = null;
        if (sceneBridge.scopeGraph && sceneBridge.scopeGraph.buildScopeGraph) {
          try {
            graph = sceneBridge.scopeGraph.buildScopeGraph(a.parseResult);
          } catch (e) {
            // Programming error: parseResult is rejected by buildScopeGraph.
            // Surface loudly so tests/devtools see it; fall through with
            // graph=null so the scene tree and inspector still render.
            console.error('[WD2-A] buildScopeGraph failed:', e && (e.message || e));
          }
        }
        // 2. Build a USE resolver that consults the graph. Without it, every
        //    USE item falls back to UNRESOLVED -- we never report a USE as
        //    resolved from a flat defsByName alone (F4).
        const useResolver = graph && sceneBridge.scopeGraph.resolve
          ? (useNode) => {
              try {
                const resolution = sceneBridge.scopeGraph.resolve(graph, useNode);
                if (resolution && sceneBridge.scopeGraph.isResolved(resolution)
                    && resolution.symbol && resolution.symbol.node) {
                  return { status: 'resolved', targetAstNode: resolution.symbol.node };
                }
                return { status: 'unresolved' };
              } catch (e) {
                // A bad USE lookup is contained -- other USEs still resolve --
                // but we surface it so the bug is visible.
                console.error('[WD2-A] resolveUse failed:', e && (e.message || e));
                return { status: 'unresolved' };
              }
            }
          : null;
        // 3. Build the scene tree.
        try {
          S.sceneTree = sceneBridge.sceneTree.buildSceneTree(a.parseResult, { useResolver });
        } catch (e) {
          console.error('[WD2-A] buildSceneTree failed:', e && (e.message || e));
          S.sceneTree = null;
        }
        // 4. Findings: the graph is the input -- NEVER the parseResult.
        let rawFindings = [];
        if (graph && sceneBridge.semanticFindings && sceneBridge.semanticFindings.findingsForDocument) {
          try {
            rawFindings = sceneBridge.semanticFindings.findingsForDocument(graph);
          } catch (e) {
            // ESCOPEGRAPH here would be a programming error -- we built the
            // graph from the parseResult ourselves, so it must be valid.
            // Surface it visibly rather than swallowing to [].
            console.error('[WD2-A] findingsForDocument failed:', e && (e.message || e));
          }
        }
        // P4-A ordering -- never done by the renderer; it stores the array.
        try {
          S.findings = sceneBridge.presentation
            ? sceneBridge.presentation.presentDocumentFindings(rawFindings)
            : [];
        } catch (e) {
          console.error('[WD2-A] presentDocumentFindings failed:', e && (e.message || e));
          S.findings = [];
        }
        // WD2-B -- selection survival. The selected item is re-anchored ONLY
        // through WD1.4 identity and a verified transaction (the Inspector's
        // own receipt, or the editor's verified change chain); an unproven
        // selection is cleared visibly. Never by id, label, DEF or offset.
        reanchorAfterAnalysis(previousAnalysis, a);
        S.displayLabels = sceneBridge.firstObject ? sceneBridge.firstObject.displayLabels(S.sceneTree) : new Map();
        noteDamage();
      } else {
        S.sceneTree = null;
        S.findings = [];
        S.analysisSession = null;
        S.pendingApply = null;
        S.displayLabels = new Map();
        if (sceneSelection.getSelection()) {
          sceneSelection.clearSelection();
          if (inspectorView) inspectorView.setNotice('Selection cleared: the document could not be analysed.');
        }
      }
      render();
    },
  });
}

// WD2-B -- decide the selection for a fresh analysis (see onAnalysis).
function reanchorAfterAnalysis(previousAnalysis, a) {
  const pendingApply = S.pendingApply;
  S.pendingApply = null;
  let nextSession = null;
  if (typeof a.text === 'string' && sceneBridge.identity) {
    try {
      nextSession = sceneBridge.identity.createParseSession(a.text, a.parseResult);
    } catch (e) {
      console.error('[WD2-B] createParseSession failed:', e && (e.message || e));
    }
  }
  S.analysisSession = nextSession;
  // WD2-C -- a dispatched Add / Duplicate selects exactly the node its verified
  // insertion created; a Delete clears the selection (the node is gone). Both
  // only when this analysis is exactly that transaction.
  if (pendingApply && (pendingApply.select || pendingApply.cleared) && pendingApply.newText === a.text) {
    if (pendingApply.cleared) { sceneSelection.clearSelection(); return; }
    let id = null;
    if (nextSession && S.sceneTree && sceneBridge.firstObject) {
      try {
        id = sceneBridge.firstObject.selectInserted({ next: { session: nextSession, tree: S.sceneTree }, pendingApply });
      } catch (e) {
        console.error('[WD2-C] selectInserted failed:', e && (e.message || e));
      }
    }
    if (id) { sceneSelection.setSelection(id); return; }
    sceneSelection.clearSelection();
    if (inspectorView) inspectorView.setNotice('Selection cleared: the new object could not be proven to be the inserted one.');
    return;
  }
  const selectedId = sceneSelection.getSelection();
  if (selectedId == null) return;
  let decision = null;
  if (nextSession && S.sceneTree && sceneBridge.inspectorEdit) {
    try {
      decision = sceneBridge.inspectorEdit.reanchorSelection({
        previous: previousAnalysis,
        next: { session: nextSession, tree: S.sceneTree },
        selectedId,
        chain: a.transaction || null,
        pendingApply,
      });
    } catch (e) {
      console.error('[WD2-B] reanchorSelection failed:', e && (e.message || e));
    }
  }
  if (decision && decision.id) {
    if (decision.id !== selectedId) sceneSelection.setSelection(decision.id);
    return;
  }
  sceneSelection.clearSelection();
  if (inspectorView) {
    inspectorView.setNotice('Selection cleared: the previously selected item could not be proven to be the same item after this change.');
  }
}

// WD2-B -- the Inspector's field list for the selected item, from the SAME
// analysis the scene tree came from. Null for non-Node items.
function inspectorFieldsFor(item) {
  if (!item || !S.analysisSession || !S.sceneTree || !sceneBridge.inspectorEdit) return null;
  try {
    return sceneBridge.inspectorEdit.fieldsForItem({
      session: S.analysisSession, tree: S.sceneTree, currentText: currentText(), itemId: item.id,
    });
  } catch (e) {
    console.error('[WD2-B] fieldsForItem failed:', e && (e.message || e));
    return null;
  }
}

// WD2-B -- one Inspector Apply: plan + verify (pure), then ONE CodeMirror
// transaction on the shared buffer, then an immediate analysis so the scene
// tree, the re-anchored selection and the Inspector refresh together. Dirty
// tracking, recovery, live preview and diagnostics all follow from the normal
// onChange/onAnalysis path -- nothing here duplicates them.
function applyInspectorField(item, field, components) {
  const refused = (reason) => ({ status: 'refused', reason });
  if (!S.handle || !item || !field) return refused('stale');
  if (S.handle.isReadOnly && S.handle.isReadOnly()) return refused('editor-read-only');
  const plan = sceneBridge.inspectorEdit.prepareInspectorApply({
    session: S.analysisSession,
    tree: S.sceneTree,
    currentText: currentText(),
    itemId: item.id,
    fieldIndex: field.index,
    fieldName: field.name,
    components,
  });
  if (plan.status !== 'ready') return plan;
  const sent = S.handle.applyVerifiedEdits({ oldText: plan.oldText, edits: plan.edits, newText: plan.newText });
  if (!sent || !sent.ok) return refused((sent && sent.reason) || 'dispatch-failed');
  S.pendingApply = { oldText: plan.oldText, newText: plan.newText, receipt: plan.receipt };
  S.handle.reanalyzeNow();
  return plan;
}

// --- WD2-C: Model workspace ----------------------------------------------------

// A blocking syntax error (anything but the header line) pauses every visual
// edit; in Model the Source pane opens ONCE per damaged state so the user can
// see why, and the status line says so in words.
function noteDamage() {
  const damaged = (S.diagnostics || []).some((d) => d && d.severity === 'error' && d.code !== 'VRML001' && d.code !== 'VRML002');
  if (damaged && !S.damaged && S.workspaceMode === 'model') {
    S.sourceOpen = true;
    applyWorkspace();
    if (modelWorkspace) modelWorkspace.setStatus('The document has syntax errors, so visual editing is paused. Fix them in Source.', true);
  } else if (!damaged && S.damaged && modelWorkspace) {
    modelWorkspace.setStatus('', false);
  }
  S.damaged = damaged;
}

function applyWorkspace() {
  if (!els.main) return;
  const model = S.workspaceMode === 'model';
  els.main.classList.toggle('workspace-model', model);
  els.main.classList.toggle('source-open', model && S.sourceOpen);
  // The preview must render in Model even when the remembered preview layout
  // hid it ("Editor only"): ask once for the current buffer.
  const st = EP() ? EP()._state() : null;
  if (model && st && st.layout === 'editor-only' && st.displayedGeneration === 0 && S.handle) EP().manualUpdate();
}

function setWorkspaceMode(mode, persist) {
  S.workspaceMode = mode === 'model' ? 'model' : 'code';
  if (persist && window.WrlPreferences) window.WrlPreferences.set('workspaceMode', S.workspaceMode);
  applyWorkspace();
}

function rememberedWorkspaceMode() {
  return window.WrlPreferences ? window.WrlPreferences.get('workspaceMode') : 'code';
}

// Dispatch one VERIFIED structural plan as ONE CodeMirror transaction, then
// analyse immediately so the tree, the selection (inserted node / cleared) and
// the panels refresh together. Never mutates on a refusal.
function dispatchModelPlan(plan, okMessage) {
  const FO = sceneBridge.firstObject;
  if (!plan || plan.status !== 'ready') {
    return { ok: false, plan, message: plan && plan.status === 'unchanged' ? 'No change.' : FO.refusalText(plan) };
  }
  if (S.handle.isReadOnly && S.handle.isReadOnly()) return { ok: false, plan, message: 'The editor is read-only.' };
  const sent = S.handle.applyVerifiedEdits({ oldText: plan.oldText, edits: plan.edits, newText: plan.newText, userEvent: 'input.model' });
  if (!sent || !sent.ok) return { ok: false, plan, message: 'The document changed before the edit could be applied; try again.' };
  S.pendingApply = {
    oldText: plan.oldText, newText: plan.newText, receipt: plan.receipt,
    select: plan.select || null, cleared: plan.operation === 'delete',
  };
  S.handle.reanalyzeNow();
  return { ok: true, plan, message: okMessage };
}

function modelSnapshot(itemId) {
  return { session: S.analysisSession, tree: S.sceneTree, currentText: currentText(), itemId };
}

function friendlyLabel(item) {
  if (!item) return null;
  const friendly = S.displayLabels.get(item.id);
  return friendly || (SceneTreeView && SceneTreeView.labelFor ? SceneTreeView.labelFor(item) : item.kind);
}

function addObject(primitive) {
  if (!S.handle || !S.analysisSession) return { ok: false, message: 'Open a document first.' };
  const plan = sceneBridge.firstObject.prepareAdd({ session: S.analysisSession, currentText: currentText(), primitive });
  return dispatchModelPlan(plan, `${primitive} created at origin.`);
}

function duplicateSelected(itemId) {
  if (!S.handle || itemId == null) return { ok: false, message: 'Select an object first.' };
  const label = friendlyLabel(sceneBridge.sceneTree.itemById(S.sceneTree, itemId));
  const plan = sceneBridge.firstObject.prepareDuplicate(modelSnapshot(itemId));
  return dispatchModelPlan(plan, `${label} duplicated. The copy is selected.`);
}

function deleteSelected(itemId) {
  if (!S.handle || itemId == null) return { ok: false, message: 'Select an object first.' };
  const label = friendlyLabel(sceneBridge.sceneTree.itemById(S.sceneTree, itemId));
  const plan = sceneBridge.firstObject.prepareDelete(modelSnapshot(itemId));
  return dispatchModelPlan(plan, `${label} deleted.`);
}

function applyObjectProperty(itemId, key, components) {
  if (!S.handle) return { status: 'refused', reason: 'stale' };
  const plan = sceneBridge.firstObject.preparePropertySet({ ...modelSnapshot(itemId), key, components });
  if (plan.status !== 'ready') return plan;
  const res = dispatchModelPlan(plan, null);
  return res.ok ? plan : { status: 'refused', reason: 'dispatch-failed', message: res.message };
}

function initModelWorkspace() {
  if (modelWorkspace || !ModelWorkspace || !els.modelBtn) return;
  modelWorkspace = ModelWorkspace.createModelWorkspace({
    els: {
      modelBtn: els.modelBtn, codeBtn: els.codeBtn, sourceBtn: els.sourceBtn,
      addBox: els.addBox, addSphere: els.addSphere, duplicate: els.duplicate, remove: els.remove,
      selected: els.modelSelected, status: els.modelStatus, props: els.objectProps,
    },
    selection: sceneSelection,
    isOpen: () => !!S.handle,
    getMode: () => S.workspaceMode,
    setMode: (m) => setWorkspaceMode(m, true),
    isSourceOpen: () => S.sourceOpen,
    setSourceOpen: (open) => { S.sourceOpen = !!open; applyWorkspace(); },
    describeSelection: (id) => {
      const item = S.sceneTree ? sceneBridge.sceneTree.itemById(S.sceneTree, id) : null;
      return item ? { label: friendlyLabel(item), isNode: item.kind === 'Node' } : null;
    },
    objectFor: (id) => {
      if (!S.analysisSession || !S.sceneTree) return null;
      try { return sceneBridge.firstObject.objectForItem(modelSnapshot(id)); } catch (e) {
        console.error('[WD2-C] objectForItem failed:', e && (e.message || e));
        return null;
      }
    },
    // The panel re-renders only when the selection or the analysed parse changes.
    analysisToken: () => S.appliedAnalysisVersion,
    add: addObject,
    duplicate: duplicateSelected,
    remove: deleteSelected,
    applyProperty: applyObjectProperty,
    refusalText: (plan) => sceneBridge.firstObject.refusalText(plan),
  });
}

// One-time wiring of the scene-tree view + inspector to the shared selection
// authority. Called from init() after the views exist on the page.
function initSceneViews() {
  if (sceneTreeView || inspectorView) return;
  sceneTreeView = SceneTreeView.createSceneTreeView(els.sceneTree, sceneSelection, {
    itemContainingOffset: sceneBridge.sceneTree.itemContainingOffset,
    // WD2-C: display-only "Box" / "Sphere" for recognised simple objects.
    displayLabelFor: (item) => S.displayLabels.get(item.id) || null,
  });
  inspectorView = InspectorView.createInspector(els.sceneInspector, sceneSelection, {
    presentation: sceneBridge.presentation,
    messages: sceneBridge.messages,
    // C1: the inspector looks up the selected item by id through the
    // scene-tree facade. The renderer is the single wiring authority -- no
    // private scene-tree code path lives in the inspector itself.
    itemById: sceneBridge.sceneTree.itemById,
    // F3 (diagnostic ownership): the inspector attaches each finding to
    // the SMALLEST scene item containing its range.start.offset, never to
    // every ancestor. Without this dep, ownership falls back to the looser
    // "any containing item" rule the QA report flagged.
    itemContainingOffset: sceneBridge.sceneTree.itemContainingOffset,
    // C2: `S.findings` is already P4-A presented records (`{finding,
    // presentation}`), ordered by P4-A. The inspector consumes them
    // directly -- it must NOT call presentDocumentFindings a second time.
    findingsForDocument: () => S.findings,
    // WD2-B: typed field editing. The Inspector asks; the pure model and the
    // verified-transaction gate decide; the DOM never computes an offset.
    fieldsFor: inspectorFieldsFor,
    applyField: applyInspectorField,
  });
}

async function init() {
  wireButtons();
  populateThemes();
  applyZoom(savedZoom()); // set chrome scale + label on cold start (font seeded at mount)

  // Phase: Preferences & Settings -- mirror the editor's visible controls to
  // the shared preferences model so a change in the Preferences & Settings
  // dialog updates this page live. The shared module fires its initial
  // subscriber callback synchronously with the current state, so a fresh
  // navigation refreshes the controls without an extra round-trip.
  if (window.WrlPreferences) {
    window.WrlPreferences.subscribe((prefs) => {
      // Theme: only update the visible <select> if the user isn't the one
      // who just changed it. The change handler above writes to the shared
      // model first, which would otherwise re-fire and clobber focus.
      if (els.themeSelect && els.themeSelect.value !== prefs.theme) {
        els.themeSelect.value = UI.resolveTheme(prefs.theme);
      }
      if (S.handle) {
        const current = S.handle.getTheme ? S.handle.getTheme() : null;
        if (current !== prefs.theme) S.handle.setTheme(prefs.theme);
      }
      // Zoom: the existing applyZoom() does the live work AND writes back
      // to the shared model. A no-op write is guarded by set() itself, so
      // there's no infinite loop here -- the second call is a cheap compare
      // and returns the same state object.
      if (S.zoom !== UI.resolveZoom(prefs.zoom)) {
        applyZoom(prefs.zoom);
      }
    });
  }
  // WD2-A: bind the scene-tree view + inspector to the shared selection
  // authority BEFORE the editor mounts, so the first analysis paints them.
  initSceneViews();
  initModelWorkspace();

  // Phase Beta 2 -- run the shared recovery prompt BEFORE we mount anything.
  // The prompt is the single authority for "Restore / Start Fresh"; on the
  // editor page, Restore means "re-mount with the recovered buffer". The
  // prompt's default behaviour (navigate to /editor) is a no-op here because
  // we are already on the editor page; we supply a custom onRestore so the
  // recovered session is reflected in the editor view immediately.
  // The recovery record stays on disk after adoption -- only Save success,
  // Discard, or Start Fresh clear it.
  if (window.WRLForgeRecoveryPrompt) {
    await window.WRLForgeRecoveryPrompt.maybePrompt({
      onRestore: async (adopted) => {
        if (adopted && adopted.sourceMissingRecovered) {
          // The source file is gone. The current editor model requires a
          // real source path to host a session; without one we cannot
          // mount safely without risking an accidental write. The recovery
          // record stays on disk and the user can re-decide on next launch.
          showMsg(
            'The source file the recovery refers to is no longer on disk. ' +
            'Your unsaved work is preserved in the recovery file and will ' +
            'be offered again on next launch.',
            true,
          );
          render();
          return;
        }
        try {
          const d = await bridge.describe({ includeText: true });
          if (d && d.open) {
            S.sessionId = d.sessionId;
            S.context = d.context || 'generic';
            S.sourcePath = d.sourcePath;
            S.format = d.format;
            S.gzip = !!d.gzip;
            S.baseline = d.baseline != null ? d.baseline : d.text;
            const profile = d.profile || (S.context === 'world' ? 'world' : S.context === 'mall' ? 'mall-item' : 'generic');
            setWorkspaceMode(UI.initialWorkspaceMode({ text: d.text, remembered: rememberedWorkspaceMode() }), false);
            mountEditor(d.text, profile);
            render();
            S.handle.focus();
          }
        } catch (e) { /* fall through to the regular init flow */ }
      },
    });
  }

  let d;
  try { d = await bridge.describe({ includeText: true }); } catch (e) { d = { open: false }; }
  // Cold start / direct navigation: try to restore the most-recent document.
  if (!d.open) {
    try { const r = await bridge.restore(); if (r && r.restored) d = r; } catch (e) { /* none */ }
  }
  if (!d.open) {
    showMsg('No document is open. Open one from the Mall or World workspace.', false);
    render();
    return;
  }
  S.sessionId = d.sessionId;
  S.context = d.context || 'generic';
  S.sourcePath = d.sourcePath;
  S.format = d.format;
  S.gzip = !!d.gzip;
  S.baseline = d.baseline != null ? d.baseline : d.text;
  const profile = d.profile || (S.context === 'world' ? 'world' : S.context === 'mall' ? 'mall-item' : 'generic');
  // WD2-C: a new/empty document opens in Model; anything else in the
  // remembered workspace. Not persisted here -- only a user switch is.
  setWorkspaceMode(UI.initialWorkspaceMode({ text: d.text, remembered: rememberedWorkspaceMode() }), false);
  mountEditor(d.text, profile);
  render();
  if (S.workspaceMode === 'model' && els.addBox) els.addBox.focus(); else S.handle.focus();

  // Live preview covers both profiles: Mall (Phase 7C2) and World (Phase 7C3).
  // The orchestrator sends only text+version to main; the document's context
  // selects the render engine and the profile-specific controls.
  if ((S.context === 'mall' || S.context === 'world') && EP()) {
    EP().start({
      sessionId: S.sessionId,
      getText: currentText,
      getVersion: () => S.bufferVersion,
      context: S.context,
    });
    applyWorkspace(); // WD2-C: Model shows the preview even under "Editor only"
  }
}

// Exposed only for the serialized visual-QA capture harness (main.js editor
// jobs). Each method wraps an action the page already performs through its own
// buttons/handle -- it adds no capability or privilege beyond the DOM the editor
// page already has, mirroring the __wrlForge* hooks on the Mall/World pages.
window.__wrlEditor = {
  ready: () => !!S.handle || els.msg.style.display === 'block',
  setText: (t) => {
    if (!S.handle) return false;
    S.handle.view.dispatch({ changes: { from: 0, to: S.handle.getText().length, insert: t } });
    return true;
  },
  click: (id) => { const n = el(id); if (n) n.click(); },
  setTheme: (t) => { els.themeSelect.value = t; els.themeSelect.dispatchEvent(new Event('change')); },
  zoom: () => S.zoom,
  setZoom: (n) => { applyZoom(n); return S.zoom; },
  clickFirst: (sel) => { const n = document.querySelector(sel); if (n) n.click(); return !!n; },
  modalVisible: () => el('modalBackdrop').classList.contains('show'),
  // Live-preview QA hooks (Phase 7C2): each wraps an action the page performs
  // through its own preview controls. No buffer text is exposed.
  previewUpdate: () => { if (window.wrlEditorPreview) window.wrlEditorPreview.manualUpdate(); },
  previewSaved: () => { if (window.wrlEditorPreview) window.wrlEditorPreview.showSaved(); },
  // World live-preview hooks (Phase 7C3): each drives the page's own controls.
  previewFindNew: () => { if (window.wrlEditorPreview) window.wrlEditorPreview.findNewFiles(); },
  previewViewpoint: (i) => {
    const sel = el('wpViewpoint');
    if (sel && !sel.disabled) { sel.value = String(i); sel.dispatchEvent(new Event('change')); }
  },
  previewNav: (mode) => {
    const sel = el('wpNav');
    if (sel) { sel.value = mode; sel.dispatchEvent(new Event('change')); }
  },
  previewReset: () => { const n = el('wpReset'); if (n) n.click(); },
  previewLayout: (m) => { if (window.wrlEditorPreview) window.wrlEditorPreview.setLayout(m); },
  previewMaximize: () => { if (window.wrlEditorPreview) window.wrlEditorPreview.toggleMaximize(); },
  previewStepSplit: (d) => { if (window.wrlEditorPreview) window.wrlEditorPreview.stepSplit(d); },
  previewState: () => (window.wrlEditorPreview ? window.wrlEditorPreview._state() : null),
  previewLeak: () => (window.wrlEditorPreview ? window.wrlEditorPreview._leak() : null),
  fitMode: (m) => { const n = el(m === 'fit' ? 'modeFit' : 'modeOriginal'); if (n) { n.checked = true; n.dispatchEvent(new Event('change')); } },
  // WD2-B QA hooks: each drives or reads the page's own scene tree / Inspector
  // DOM exactly as a user would (selection, typing into a field control, Enter /
  // Escape / Apply / Cancel). `bufferEquals` answers a yes/no comparison so no
  // buffer text is exposed.
  sceneSelectFirst: (nodeType, nth) => {
    const items = S.sceneTree ? S.sceneTree.items.filter((it) => it.kind === 'Node' && it.nodeType === nodeType) : [];
    const item = items[nth | 0];
    if (!item) return null;
    sceneSelection.setSelection(item.id);
    return item.id;
  },
  sceneSelection: () => {
    const id = sceneSelection.getSelection();
    const item = id && S.sceneTree ? sceneBridge.sceneTree.itemById(S.sceneTree, id) : null;
    if (!item) return null;
    const same = S.sceneTree.items.filter((it) => it.kind === item.kind && it.nodeType === item.nodeType);
    const row = document.querySelector('#sceneTree .scene-row[aria-selected="true"]');
    return { id, kind: item.kind, nodeType: item.nodeType || null, def: item.def || null,
      ordinal: same.indexOf(item), rowId: row ? row.dataset.id : null, rowCount: document.querySelectorAll('#sceneTree .scene-row').length };
  },
  inspectorFields: () => {
    const notice = document.querySelector('#sceneInspector .inspector-notice');
    return {
      notice: notice ? notice.textContent : null,
      fields: Array.from(document.querySelectorAll('#sceneInspector .field-row')).map((r) => ({
        name: r.dataset.fieldName,
        type: (r.querySelector('.field-type') || {}).textContent || null,
        state: (r.querySelector('.field-state') || {}).textContent || null,
        values: Array.from(r.querySelectorAll('input')).map((i) => (i.type === 'checkbox' ? i.checked : i.value)),
        invalid: Array.from(r.querySelectorAll('input')).map((i) => i.getAttribute('aria-invalid') === 'true'),
        names: Array.from(r.querySelectorAll('input')).map((i) => (i.getAttribute('aria-labelledby') || '').split(' ')
          .map((ref) => { const n = document.getElementById(ref); return n ? n.textContent : '?'; }).join(' ')),
        message: (r.querySelector('.field-msg') || {}).textContent || '',
        reason: (r.querySelector('.field-reason') || {}).textContent || null,
      })),
    };
  },
  inspectorSet: (fieldName, comp, value) => {
    const r = document.querySelector(`#sceneInspector .field-row[data-field-name="${CSS.escape(fieldName)}"]`);
    const input = r ? r.querySelectorAll('input')[comp | 0] : null;
    if (!input) return false;
    input.focus();
    if (input.type === 'checkbox') { input.checked = !!value; input.dispatchEvent(new Event('change', { bubbles: true })); }
    else { input.value = String(value); input.dispatchEvent(new Event('input', { bubbles: true })); }
    return true;
  },
  inspectorKey: (fieldName, comp, key) => {
    const r = document.querySelector(`#sceneInspector .field-row[data-field-name="${CSS.escape(fieldName)}"]`);
    const input = r ? r.querySelectorAll('input')[comp | 0] : null;
    if (!input) return false;
    input.focus();
    input.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
    return true;
  },
  inspectorClick: (fieldName, which) => {
    const r = document.querySelector(`#sceneInspector .field-row[data-field-name="${CSS.escape(fieldName)}"]`);
    const b = r ? r.querySelector(which === 'cancel' ? '.field-cancel' : '.field-apply') : null;
    if (!b) return false;
    b.focus();
    b.click();
    return true;
  },
  activeInfo: () => {
    const a = document.activeElement;
    const r = a && a.closest ? a.closest('.field-row') : null;
    return { tag: a ? a.tagName : null, field: r ? r.dataset.fieldName : null,
      component: a && a.dataset && a.dataset.component != null ? Number(a.dataset.component) : null,
      focusVisibleOutline: a ? getComputedStyle(a).outlineStyle : null };
  },
  bufferEquals: (t) => !!S.handle && S.handle.getText() === t,
  // WD2-C QA hooks: drive / read the Model workspace's own DOM (Add, Duplicate,
  // Delete, the Object panel, the colour picker), CodeMirror's history depth,
  // and a READ-ONLY walk of the live X_ITE scene (types, Transform
  // translations, Box sizes, Sphere radii, Material colours). No buffer text.
  modelState: () => {
    const props = Array.from(document.querySelectorAll('#objectProps .prop-row')).map((r) => ({
      key: r.dataset.prop,
      label: (r.querySelector('.prop-label') || {}).textContent || null,
      tech: (r.querySelector('.prop-tech') || {}).textContent || null,
      state: (r.querySelector('.prop-state') || {}).textContent || '',
      values: Array.from(r.querySelectorAll('input.prop-num')).map((i) => i.value),
      color: (r.querySelector('input.prop-color') || {}).value || null,
      names: Array.from(r.querySelectorAll('input')).map((i) => (i.getAttribute('aria-labelledby') || '').split(' ')
        .map((ref) => { const n = document.getElementById(ref); return n ? n.textContent : '?'; }).join(' ')),
      message: (r.querySelector('.prop-msg') || {}).textContent || '',
    }));
    const title = document.getElementById('objectTitle');
    const empty = document.querySelector('#objectProps .empty-note');
    return {
      mode: S.workspaceMode, sourceOpen: S.sourceOpen,
      mainClass: els.main ? els.main.className : null,
      editorVisible: !!(els.editor && els.editor.offsetParent),
      previewVisible: !!(document.getElementById('preview') && document.getElementById('preview').offsetParent),
      selected: els.modelSelected ? els.modelSelected.textContent : null,
      status: els.modelStatus ? els.modelStatus.textContent : null,
      statusError: !!(els.modelStatus && els.modelStatus.classList.contains('err')),
      buttons: ['addBoxBtn', 'addSphereBtn', 'duplicateBtn', 'deleteBtn', 'modeModelBtn', 'modeCodeBtn', 'sourceToggleBtn']
        .map((id) => { const b = el(id); return { id, disabled: !!(b && b.disabled), hidden: !!(b && b.hidden), pressed: b ? b.getAttribute('aria-pressed') : null, name: b ? (b.getAttribute('aria-label') || b.textContent) : null }; }),
      title: title ? title.textContent : null,
      empty: empty ? empty.textContent : null,
      props,
      treeRows: Array.from(document.querySelectorAll('#sceneTree .scene-row')).map((r) => r.textContent),
      remembered: window.WrlPreferences ? window.WrlPreferences.get('workspaceMode') : null,
    };
  },
  propSet: (key, comp, value) => {
    const input = document.querySelectorAll(`#objectProps .prop-row[data-prop="${CSS.escape(key)}"] input.prop-num`)[comp | 0];
    if (!input) return false;
    input.focus(); input.value = String(value); input.dispatchEvent(new Event('input', { bubbles: true }));
    return true;
  },
  propKey: (key, comp, k) => {
    const input = document.querySelectorAll(`#objectProps .prop-row[data-prop="${CSS.escape(key)}"] input.prop-num`)[comp | 0];
    if (!input) return false;
    input.focus(); input.dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true }));
    return true;
  },
  propColor: (hex) => {
    const input = document.querySelector('#objectProps .prop-row[data-prop="color"] input.prop-color');
    if (!input) return false;
    const before = S.handle ? S.handle.getText() : null;
    input.focus(); input.value = hex;
    input.dispatchEvent(new Event('input', { bubbles: true })); // dragging: must NOT commit
    const committedOnInput = !!S.handle && S.handle.getText() !== before;
    input.dispatchEvent(new Event('change', { bubbles: true })); // the user's pick
    return { committedOnInput };
  },
  historyDepth: () => (S.handle && S.handle.historyDepth ? S.handle.historyDepth() : null),
  focusInfo: () => {
    const a = document.activeElement;
    const r = a && a.closest ? a.closest('.prop-row') : null;
    return { id: a ? a.id : null, tag: a ? a.tagName : null, prop: r ? r.dataset.prop : null };
  },
  previewScene: () => {
    const c = document.getElementById('preview');
    const b = c && c.browser;
    const scene = b && b.currentScene;
    if (!scene) return null;
    const out = { counts: {}, translations: [], boxes: [], spheres: [], colors: [] };
    const seen = new Set();
    const n3 = (v) => [v.x, v.y, v.z].map((x) => Math.round(x * 1e6) / 1e6);
    const visit = (n) => {
      if (!n || seen.has(n)) return;
      seen.add(n);
      const t = n.getNodeTypeName();
      out.counts[t] = (out.counts[t] || 0) + 1;
      if (t === 'Transform') out.translations.push(n3(n.translation));
      if (t === 'Box') out.boxes.push(n3(n.size));
      if (t === 'Sphere') out.spheres.push(Math.round(n.radius * 1e6) / 1e6);
      if (t === 'Material') out.colors.push([n.diffuseColor.r, n.diffuseColor.g, n.diffuseColor.b].map((x) => Math.round(x * 1e3) / 1e3));
      for (const f of ['children', 'appearance', 'material', 'geometry']) {
        let v;
        try { v = n[f]; } catch (e) { v = undefined; }
        if (!v) continue;
        if (typeof v.length === 'number' && typeof v.getNodeTypeName !== 'function') { for (let i = 0; i < v.length; i += 1) visit(v[i]); }
        else visit(v);
      }
    };
    for (let i = 0; i < scene.rootNodes.length; i += 1) visit(scene.rootNodes[i]);
    return out;
  },
  previewBBox: () => {
    const d = window.wrlPreview && window.wrlPreview._debug ? window.wrlPreview._debug() : null;
    return d && d.bbox ? { min: Array.from(d.bbox.min || []), max: Array.from(d.bbox.max || []) } : null;
  },
  status: () => ({
    file: els.stFile.textContent, format: els.stFormat.textContent, dirty: isDirty(),
    save: els.stSave.textContent, diag: els.stDiag.textContent, adv: els.stAdv.textContent,
    outlineRows: document.querySelectorAll('#outlineList .row-item').length,
  }),
};

if (document.readyState === 'loading') {
  document.addEventListener('DOMContentLoaded', init);
} else {
  init();
}
