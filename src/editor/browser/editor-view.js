// Native editor view (Phase 7B) -- the CodeMirror 6 assembly, bundled to
// renderer/vendor/wrl-editor.bundle.js by `npm run build:editor` (esbuild). This
// is the ONLY file that touches CodeMirror; all VRML language logic comes from
// ../language.js (which is the Phase 7A parser, the single grammar authority).
//
// Exposes window.WrlEditor.create(parent, opts) -> a handle the renderer drives.
// Highlighting and SYNTAX diagnostics recompute on a bounded debounce off the
// real tokenizer/parser; stale results are dropped by a monotonic doc version so
// a late parse can never apply diagnostics from an older buffer.

import { EditorState, StateField, StateEffect, Compartment, Transaction } from '@codemirror/state';
import {
  EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter,
  drawSelection, dropCursor, rectangularSelection, crosshairCursor, Decoration,
} from '@codemirror/view';
import { history, historyKeymap, defaultKeymap, indentWithTab, undo, redo, isolateHistory, undoDepth, redoDepth } from '@codemirror/commands';
import { search, searchKeymap, highlightSelectionMatches, openSearchPanel } from '@codemirror/search';
import { bracketMatching } from '@codemirror/language';
import { lintGutter, setDiagnostics } from '@codemirror/lint';
import * as language from '../language.js';

const analyze = language.analyze;

// --- tokenizer-driven highlight layer ---------------------------------------
const setHighlights = StateEffect.define();

const highlightField = StateField.define({
  create() { return Decoration.none; },
  update(deco, tr) {
    deco = deco.map(tr.changes); // keep spans roughly aligned between parses
    for (const e of tr.effects) {
      if (e.is(setHighlights)) deco = buildDecorations(tr.state, e.value);
    }
    return deco;
  },
  provide: (f) => EditorView.decorations.from(f),
});

function buildDecorations(state, highlights) {
  const docLen = state.doc.length;
  const spans = highlights
    .filter((h) => h.from < h.to && h.from >= 0 && h.to <= docLen)
    .sort((a, b) => a.from - b.from || a.to - b.to);
  const ranges = spans.map((h) => Decoration.mark({ class: `cm-vrml-${h.cls}` }).range(h.from, h.to));
  return Decoration.set(ranges, true);
}

// --- themes ------------------------------------------------------------------
// Four built-in themes, each a self-contained palette. All colors are chosen for
// strong text-on-background contrast (light foregrounds on dark backgrounds and
// vice-versa; every token color sits well clear of its background). One builder
// turns a palette into a full CodeMirror theme so the four never drift in
// structure -- only their colors differ.
const PALETTES = {
  light: {
    dark: false,
    bg: '#fbfbfb', fg: '#282a36', caret: '#1f6feb', selection: '#cfe3ff',
    gutterBg: '#f2f2f2', gutterFg: '#9aa0a6', activeLine: '#eef2f7', activeGutterFg: '#282a36',
    panelBg: '#eef1f5', panelFg: '#282a36', border: '#d5d9de', matchBg: '#ffe08a',
    header: '#5c6370', comment: '#5c6370', keyword: '#a626a4', is: '#a626a4', nullc: '#0b7285',
    bool: '#0b7285', nodeType: '#8a5a00', defName: '#c1121f', useName: '#c1121f',
    fieldName: '#1f5fd6', number: '#8a5a00', string: '#1f7a1f', invalid: '#c1121f', bracket: '#282a36',
  },
  dark: {
    dark: true,
    bg: '#1e2127', fg: '#e6e6e6', caret: '#7db3ff', selection: '#31405c',
    gutterBg: '#1a1d23', gutterFg: '#7a828e', activeLine: '#262b33', activeGutterFg: '#e6e6e6',
    panelBg: '#22262e', panelFg: '#e6e6e6', border: '#3a3f4a', matchBg: '#4a5568',
    header: '#8a94a6', comment: '#8a94a6', keyword: '#d38fff', is: '#d38fff', nullc: '#56c5ff',
    bool: '#56c5ff', nodeType: '#e5c07b', defName: '#ff7b72', useName: '#ff9d95',
    fieldName: '#79b8ff', number: '#f0b072', string: '#8fdf8f', invalid: '#ff7b72', bracket: '#c8ccd4',
  },
  terminal: {
    dark: true,
    bg: '#000000', fg: '#33ff66', caret: '#33ff66', selection: '#0f5f2f',
    gutterBg: '#000000', gutterFg: '#1f9f4f', activeLine: '#04140a', activeGutterFg: '#7dffa8',
    panelBg: '#02160b', panelFg: '#7dffa8', border: '#0f5f2f', matchBg: '#0f7f3f',
    header: '#3aa564', comment: '#3aa564', keyword: '#7dff9f', is: '#7dff9f', nullc: '#66ffd0',
    bool: '#66ffd0', nodeType: '#d7ff5a', defName: '#ffd24a', useName: '#ffe08a',
    fieldName: '#5affc8', number: '#ffd24a', string: '#a8ff7d', invalid: '#ff6b6b', bracket: '#33ff66',
  },
  tokyo: { // Tokyo Night
    dark: true,
    bg: '#1a1b26', fg: '#c0caf5', caret: '#7aa2f7', selection: '#2c3457',
    gutterBg: '#16161e', gutterFg: '#565f89', activeLine: '#1f2233', activeGutterFg: '#c0caf5',
    panelBg: '#1f2335', panelFg: '#c0caf5', border: '#2a2e42', matchBg: '#3d59a1',
    header: '#565f89', comment: '#565f89', keyword: '#bb9af7', is: '#bb9af7', nullc: '#7dcfff',
    bool: '#7dcfff', nodeType: '#7aa2f7', defName: '#f7768e', useName: '#ff9db4',
    fieldName: '#7dcfff', number: '#ff9e64', string: '#9ece6a', invalid: '#f7768e', bracket: '#a9b1d6',
  },
  contrast: { // High Contrast -- pure black, bright saturated tokens, for low vision
    dark: true,
    bg: '#000000', fg: '#ffffff', caret: '#ffff00', selection: '#0060c0',
    gutterBg: '#000000', gutterFg: '#c8c8c8', activeLine: '#101830', activeGutterFg: '#ffffff',
    panelBg: '#000000', panelFg: '#ffffff', border: '#ffffff', matchBg: '#0060c0',
    header: '#00ffff', comment: '#00e0e0', keyword: '#ff80ff', is: '#ff80ff', nullc: '#00ff00',
    bool: '#00ff00', nodeType: '#ffff00', defName: '#ff6060', useName: '#ffb0b0',
    fieldName: '#66d9ff', number: '#ffd000', string: '#66ff66', invalid: '#ff4040', bracket: '#ffffff',
  },
};

const THEMES = Object.keys(PALETTES);
const DEFAULT_THEME = 'dark';

function makeTheme(p) {
  // Font size is intentionally NOT set here -- it lives in its own compartment
  // (fontThemeOf, below) so zoom and theme stay orthogonal: changing one never
  // clobbers the other.
  return EditorView.theme({
    '&': { height: '100%', backgroundColor: p.bg, color: p.fg },
    '.cm-content': { caretColor: p.caret },
    '.cm-scroller': { fontFamily: 'ui-monospace, SFMono-Regular, Menlo, Consolas, monospace' },
    '.cm-cursor, .cm-dropCursor': { borderLeftColor: p.caret },
    '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection': { backgroundColor: p.selection },
    '.cm-gutters': { backgroundColor: p.gutterBg, color: p.gutterFg, border: 'none' },
    '.cm-activeLine': { backgroundColor: p.activeLine },
    '.cm-activeLineGutter': { backgroundColor: p.activeLine, color: p.activeGutterFg },
    '.cm-selectionMatch': { backgroundColor: p.matchBg },
    '.cm-panels': { backgroundColor: p.panelBg, color: p.panelFg },
    '.cm-panels input, .cm-panels button': { backgroundColor: p.bg, color: p.fg, border: `1px solid ${p.border}` },
    '.cm-searchMatch': { backgroundColor: p.matchBg },
    '.cm-searchMatch.cm-searchMatch-selected': { outline: `1px solid ${p.caret}` },
    '.cm-vrml-header': { color: p.header, fontStyle: 'italic' },
    '.cm-vrml-comment': { color: p.comment, fontStyle: 'italic' },
    '.cm-vrml-keyword': { color: p.keyword },
    '.cm-vrml-def': { color: p.keyword, fontWeight: '600' },
    '.cm-vrml-use': { color: p.keyword, fontWeight: '600' },
    '.cm-vrml-route': { color: p.keyword, fontWeight: '600' },
    '.cm-vrml-proto': { color: p.keyword, fontWeight: '600' },
    '.cm-vrml-is': { color: p.is },
    '.cm-vrml-null': { color: p.nullc },
    '.cm-vrml-bool': { color: p.bool },
    '.cm-vrml-nodeType': { color: p.nodeType, fontWeight: '600' },
    '.cm-vrml-defName': { color: p.defName, fontWeight: '600' },
    '.cm-vrml-useName': { color: p.useName },
    '.cm-vrml-fieldName': { color: p.fieldName },
    '.cm-vrml-number': { color: p.number },
    '.cm-vrml-string': { color: p.string },
    '.cm-vrml-invalid': { color: p.invalid, textDecoration: `underline wavy ${p.invalid}` },
    '.cm-vrml-bracket': { color: p.bracket },
  }, { dark: p.dark });
}

// Prebuild the themes once (they are static).
const THEME_EXT = {};
for (const name of THEMES) THEME_EXT[name] = makeTheme(PALETTES[name]);
function themeExtOf(name) { return THEME_EXT[name] || THEME_EXT[DEFAULT_THEME]; }

// --- font size (its own compartment, decoupled from theme) -------------------
// The code-area font size is a separate, reconfigurable extension so zoom and
// theme never disturb each other. Bounds are shared with the renderer's zoom
// model (ui-state.js) via the window.WrlEditor export so both clamp identically.
const DEFAULT_FONT_PX = 13;
const MIN_FONT_PX = 9;
const MAX_FONT_PX = 28;
function clampFontPx(px) {
  const n = Number(px);
  if (!Number.isFinite(n)) return DEFAULT_FONT_PX;
  return Math.max(MIN_FONT_PX, Math.min(MAX_FONT_PX, Math.round(n)));
}
function fontThemeOf(px) {
  return EditorView.theme({ '&': { fontSize: clampFontPx(px) + 'px' } });
}

// --- editor handle -----------------------------------------------------------
function create(parent, opts = {}) {
  const options = opts || {};
  const profile = options.profile || 'generic';
  const debounceMs = Number.isFinite(options.debounceMs) ? options.debounceMs : 250;
  const editable = new Compartment();
  const themeCompartment = new Compartment();
  const fontCompartment = new Compartment();
  let currentTheme = THEMES.includes(options.theme) ? options.theme : DEFAULT_THEME;
  let currentFontPx = clampFontPx(options.fontSize != null ? options.fontSize : DEFAULT_FONT_PX);

  let version = 0; // monotonic; bumps on every doc change and every re-analyze
  let pending = null; // debounce timer
  // WD2-B: the change chain since the last DELIVERED analysis. `chainBase` is
  // the exact text that analysis parsed; `chainChanges` is CodeMirror's own
  // composed ChangeSet from that text to the current buffer. Both reset on
  // setDoc (a reload is not a transaction), so a broken chain is reported as
  // null and the renderer can never re-anchor a selection through it.
  let chainBase = null;
  let chainChanges = null;

  const listener = EditorView.updateListener.of((update) => {
    if (update.docChanged && typeof options.onChange === 'function') {
      options.onChange(update.state.doc.toString());
    }
    if ((update.docChanged || update.selectionSet) && typeof options.onCursor === 'function') {
      const head = update.state.selection.main.head;
      const line = update.state.doc.lineAt(head);
      options.onCursor({ line: line.number, column: head - line.from + 1 });
    }
    if (update.docChanged) {
      chainChanges = chainChanges ? chainChanges.compose(update.changes) : update.changes;
      scheduleAnalyze();
    }
  });

  // A builder (not a hoisted const) so setDoc can rebuild a fresh EditorState
  // (which resets undo history) while carrying the CURRENT theme AND font size
  // forward -- each compartment's initial value must reflect a live
  // setTheme/setFontSize, not the value at first construction.
  const buildExtensions = () => [
    lineNumbers(),
    highlightActiveLineGutter(),
    highlightActiveLine(),
    history(),
    drawSelection(),
    dropCursor(),
    rectangularSelection(),
    crosshairCursor(),
    bracketMatching(),
    search({ top: true }),
    highlightSelectionMatches(),
    lintGutter(),
    highlightField,
    themeCompartment.of(themeExtOf(currentTheme)),
    fontCompartment.of(fontThemeOf(currentFontPx)),
    editable.of(EditorView.editable.of(options.readOnly !== true)),
    keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
    listener,
  ];

  const view = new EditorView({ state: EditorState.create({ doc: options.doc || '', extensions: buildExtensions() }), parent });

  function scheduleAnalyze() {
    if (pending) clearTimeout(pending);
    const forVersion = ++version;
    pending = setTimeout(() => runAnalyze(forVersion), debounceMs);
  }

  function runAnalyze(forVersion) {
    // Drop a stale parse: if the buffer moved on while we were scheduled, this
    // result is for an older version and must not apply.
    if (forVersion !== version) return;
    const text = view.state.doc.toString();
    let result;
    try {
      result = analyze(text, { profile });
    } catch {
      // A parser fault must never wipe the last good editor state; leave the
      // current highlights/diagnostics in place.
      return;
    }
    if (forVersion !== version) return; // moved on during analyze
    // WD2-B: the WD1.2 edit set (offsets against `chainBase`) CodeMirror applied
    // since the previous delivered analysis. The renderer VERIFIES it before
    // trusting it; this is evidence, not authority.
    const transaction = chainBase === null ? null : { previousText: chainBase, edits: inspectorEdit.changesToEdits(chainChanges) };
    chainBase = text;
    chainChanges = null;
    view.dispatch({ effects: setHighlights.of(result.highlights) });
    view.dispatch(setDiagnostics(view.state, result.diagnostics.map((d) => ({
      from: d.from, to: d.to, severity: d.severity, message: d.message, source: d.code,
    }))));
    if (typeof options.onAnalysis === 'function') {
      options.onAnalysis({
        version: forVersion, // monotonic; lets the renderer drop a stale callback too
        diagnostics: result.diagnostics,
        advisories: result.advisories,
        outline: result.outline,
        meta: result.meta,
        // WD2-A: the parse result is exposed here so the editor binding can
        // build a scene tree + structured findings from the SAME parse the
        // diagnostics came from. The renderer is the only consumer; the
        // editor surface itself does not consume this field.
        parseResult: result.parseResult,
        // WD2-B: the exact text parsed (binds a parse session to this parse)
        // and the change chain from the previously delivered analysis.
        text,
        transaction,
      });
    }
  }

  runAnalyze(++version); // initial synchronous pass so the first paint is highlighted

  return {
    view,
    getText: () => view.state.doc.toString(),
    setDoc(text) {
      // Fresh state -> resets undo history (a reload/open is not undoable back
      // into the previous file), carrying the current theme, then re-analyze.
      view.setState(EditorState.create({ doc: text, extensions: buildExtensions() }));
      chainBase = null;
      chainChanges = null;
      runAnalyze(++version);
    },
    setTheme(name) {
      if (!THEMES.includes(name)) return currentTheme;
      currentTheme = name;
      view.dispatch({ effects: themeCompartment.reconfigure(themeExtOf(name)) });
      return currentTheme;
    },
    getTheme() { return currentTheme; },
    setFontSize(px) {
      currentFontPx = clampFontPx(px);
      view.dispatch({ effects: fontCompartment.reconfigure(fontThemeOf(currentFontPx)) });
      return currentFontPx;
    },
    getFontSize() { return currentFontPx; },
    focus: () => view.focus(),
    setReadOnly(ro) { view.dispatch({ effects: editable.reconfigure(EditorView.editable.of(!ro)) }); },
    revealRange(from, to) {
      const len = view.state.doc.length;
      const a = Math.max(0, Math.min(from, len));
      const b = Math.max(a, Math.min(to == null ? from : to, len));
      view.dispatch({ selection: { anchor: a, head: b }, scrollIntoView: true });
      view.focus();
    },
    gotoLine(line) {
      const n = Math.max(1, Math.min(line, view.state.doc.lines));
      const l = view.state.doc.line(n);
      view.dispatch({ selection: { anchor: l.from }, scrollIntoView: true });
      view.focus();
    },
    reanalyzeNow() { runAnalyze(++version); },
    isReadOnly() { return !view.state.facet(EditorView.editable); },
    // WD2-B: dispatch one VERIFIED Inspector edit set as ONE CodeMirror
    // transaction on the same document the code editor edits. Refuses unless
    // the buffer is still exactly the text the edits were verified against; the
    // whole set is one isolated history event, so one Undo reverses one Apply.
    // WD2-C: structural First Object actions use the same dispatch, tagged
    // 'input.model' instead of 'input.inspector' (a closed list -- no caller
    // can smuggle another event type in).
    applyVerifiedEdits({ oldText, edits, newText, userEvent } = {}) {
      if (!view.state.facet(EditorView.editable)) return { ok: false, reason: 'editor-read-only' };
      if (typeof oldText !== 'string' || typeof newText !== 'string' || !Array.isArray(edits) || edits.length === 0) {
        return { ok: false, reason: 'malformed-edit-set' };
      }
      if (view.state.doc.toString() !== oldText) return { ok: false, reason: 'buffer-changed' };
      view.dispatch({
        changes: edits.map((e) => ({ from: e.from, to: e.to, insert: e.insert })),
        annotations: [isolateHistory.of('full'), Transaction.userEvent.of(userEvent === 'input.model' ? 'input.model' : 'input.inspector')],
      });
      if (view.state.doc.toString() !== newText) {
        // Unreachable for a verified set; if it ever happens, take it back.
        undo(view);
        return { ok: false, reason: 'post-dispatch-mismatch' };
      }
      return { ok: true };
    },
    // Toolbar-driven equivalents of the built-in keymap commands, so the app's
    // buttons and CodeMirror's own shortcuts route through the same commands.
    // WD2-C QA/evidence: CodeMirror's own history depths (one visual action
    // must add exactly one undo step).
    historyDepth() { return { undo: undoDepth(view.state), redo: redoDepth(view.state) }; },
    // Focus follows into the code area only when it is visible (WD2-C: the
    // Model workspace may collapse Source; a keyboard user keeps their place).
    undo() { undo(view); if (view.dom.offsetParent) view.focus(); },
    redo() { redo(view); if (view.dom.offsetParent) view.focus(); },
    openSearch() { openSearchPanel(view); },
    destroy() { if (pending) clearTimeout(pending); view.destroy(); },
  };
}

window.WrlEditor = { create, THEMES, DEFAULT_THEME, DEFAULT_FONT_PX, MIN_FONT_PX, MAX_FONT_PX };

// WD2-A: scene-tree + structured-diagnostic bridge. The renderer is the only
// consumer of this surface; it never touches the parser or the semantic
// authorities directly. The bridge is a thin module load over already-bundled
// vrml/ modules -- esbuild pulled them in when editor-view.js imported
// language.js, so this is a re-export, not a second load. Renderer-side: every
// entry below is read-only and pure -- never accepts a write path or a path
// argument, never crosses back into main.
import * as vrmlSceneTree from '../../vrml/scene-tree.js';
import * as vrmlPresentation from '../../vrml/presentation.js';
import * as vrmlMessages from '../../vrml/messages.js';
import * as vrmlSemanticFindings from '../../vrml/semantic-findings.js';
import * as vrmlScopeGraph from '../../vrml/scope-graph.js';
import * as vrmlDocumentTransaction from '../../vrml/document-transaction.js';
import * as vrmlFieldEdit from '../../vrml/field-edit.js';
import * as inspectorEdit from '../inspector-edit.js';
import * as firstObject from '../first-object.js';
import * as viewportPick from '../viewport-pick.js';

window.WRLForgeSceneBridge = Object.freeze({
  sceneTree: Object.freeze({
    buildSceneTree: vrmlSceneTree.buildSceneTree,
    itemContainingOffset: vrmlSceneTree.itemContainingOffset,
    itemById: vrmlSceneTree.itemById,
    // WD2-B: item <-> exact AST object of the same build (object identity).
    astNodeForItem: vrmlSceneTree.astNodeForItem,
    itemForAstNode: vrmlSceneTree.itemForAstNode,
    KIND: vrmlSceneTree.KIND,
    USE_TARGET: vrmlSceneTree.USE_TARGET,
  }),
  // vrml/{presentation,messages,semantic-findings}.js export CommonJS
  // module.exports objects; under esbuild's interop an `import * as` gives
  // the namespace a `.default` slot, but the public surface IS the
  // module.exports -- so the renderer reads straight off the namespace.
  presentation: vrmlPresentation,
  messages: vrmlMessages,
  semanticFindings: Object.freeze({
    findingsForDocument: vrmlSemanticFindings.findingsForDocument,
    FINDING_CODE: vrmlSemanticFindings.FINDING_CODE,
    ISO_RESULT: vrmlSemanticFindings.ISO_RESULT,
  }),
  // The scope-graph surface the renderer needs to (a) build the graph that
  // `findingsForDocument` expects and (b) resolve USE names through 4.6.2
  // instead of the flat defsByName. The renderer is the single authority on
  // USE resolution; `resolve(graph, useNode)` is its answer, the scene tree
  // consumes it.
  scopeGraph: Object.freeze({
    buildScopeGraph: vrmlScopeGraph.buildScopeGraph,
    resolve: vrmlScopeGraph.resolve,
    STATUS: vrmlScopeGraph.STATUS,
    isResolved: vrmlScopeGraph.isResolved,
  }),
  // WD2-B: binds each analysed text to its parse (WD1.4 session) so a
  // selection can be re-anchored through a verified transaction.
  identity: Object.freeze({
    createParseSession: vrmlDocumentTransaction.createParseSession,
  }),
  // WD2-B: the pure field-edit planning + selection-survival decisions. The
  // renderer never computes an offset or picks a re-anchored node itself.
  inspectorEdit: Object.freeze({
    fieldsForItem: inspectorEdit.fieldsForItem,
    prepareInspectorApply: inspectorEdit.prepareInspectorApply,
    reanchorSelection: inspectorEdit.reanchorSelection,
    OUTCOME: inspectorEdit.OUTCOME,
  }),
  fieldEdit: Object.freeze({
    FIELD_EDIT_REASON: vrmlFieldEdit.FIELD_EDIT_REASON,
    PLAN_STATUS: vrmlFieldEdit.PLAN_STATUS,
  }),
  // WD2-C: First Object -- Add / Duplicate / Delete / beginner properties as
  // VERIFIED edit sets, identity of an inserted node, display-only labels.
  firstObject: Object.freeze({
    prepareAdd: firstObject.prepareAdd,
    prepareDuplicate: firstObject.prepareDuplicate,
    prepareDelete: firstObject.prepareDelete,
    preparePropertySet: firstObject.preparePropertySet,
    objectForItem: firstObject.objectForItem,
    displayLabels: firstObject.displayLabels,
    selectInserted: firstObject.selectInserted,
    refusalText: firstObject.refusalText,
  }),
  // WD2-D: viewport picking -- the PURE resolver from a plain-data X_ITE hit
  // snapshot to an existing scene item, or an explicit refusal. It touches no
  // X_ITE API; src/preview/xite-pick-adapter.js produces the snapshot.
  viewportPick: Object.freeze({
    resolvePick: viewportPick.resolvePick,
    refusalText: viewportPick.refusalText,
    compatibilityText: viewportPick.compatibilityText,
    compatibilityRow: viewportPick.compatibilityRow,
    STATUS: viewportPick.STATUS,
  }),
});
