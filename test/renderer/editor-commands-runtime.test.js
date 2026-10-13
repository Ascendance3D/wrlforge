'use strict';
// UI-0 (#35) behavioural runtime test for the editor page's Command Registry
// wiring (docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md §7, §8, §10.1, §22, §26).
//
// The REAL scripts -- ui-state, scene-selection, model-workspace,
// workspace-presets, command-registry, panel-registry, command-bindings and
// editor.js -- are loaded, in editor.html order, into ONE vm context whose
// global IS `window` (classic-script semantics). Only the page's environment is
// stubbed: a DOM generated from the real renderer/editor.html markup (one stub
// per `id="..."`, with its tag and attributes, the layout <select> with its three
// <option data-command> children), the main-process bridge, the CodeMirror
// handle, the preview orchestrator, the scene bridge and the preferences model.
// Everything is driven through stub clicks and synthetic window keydown events.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const ROOT = path.join(__dirname, '..', '..');
const UI = require('../../src/editor/ui-state');

const read = (rel) => fs.readFileSync(path.join(ROOT, rel), 'utf8');
const HTML = read('renderer/editor.html');
const SCRIPTS = [
  'src/editor/ui-state.js',
  'src/editor/scene-selection.js',
  'renderer/model-workspace.js',
  'src/editor/workspace-presets.js',
  'src/editor/command-registry.js',
  'src/editor/panel-registry.js',
  'renderer/command-bindings.js',
  'renderer/editor.js',
].map((rel) => ({ rel, code: read(rel) }));

const TEXT = '#VRML V2.0 utf8\nShape { geometry Box {} }\n';
const plain = (v) => JSON.parse(JSON.stringify(v));

// The exact id set of docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md §7.2 minus the
// two NEW commands (workspace.play, workspace.reset), which are #121's.
const EXPECTED_COMMAND_IDS = [
  'file.back', 'file.save', 'file.saveAs', 'file.reload', 'file.openExternal', 'file.close',
  'edit.undo', 'edit.redo', 'edit.find', 'edit.replace', 'edit.gotoLine',
  'view.zoomIn', 'view.zoomOut', 'view.zoomReset', 'view.highContrast', 'view.preferences',
  'workspace.code', 'workspace.model', 'workspace.toggleSource',
  'preview.update', 'preview.showSaved', 'preview.toggleMaximize',
  'preview.layout.split', 'preview.layout.previewMax', 'preview.layout.editorOnly',
  'preview.findNewFiles',
  'model.addBox', 'model.addSphere', 'model.duplicate', 'model.delete',
];

const EXPECTED_PANEL_IDS = ['source', 'preview', 'object', 'outline', 'sceneTree', 'inspector', 'diagnostics', 'advisories'];

// --- stub DOM generated from the real editor.html ------------------------------

function parseAttrs(text) {
  const attrs = {};
  const re = /([\w:-]+)(?:\s*=\s*"([^"]*)")?/g;
  let m;
  while ((m = re.exec(text))) attrs[m[1]] = m[2] === undefined ? '' : m[2];
  return attrs;
}

function makeElement(tagName, attrs, doc) {
  const classes = new Set(String(attrs.class || '').split(/\s+/).filter(Boolean));
  const listeners = {};
  const a = { ...attrs };
  delete a.class;
  const el = {
    tagName: String(tagName).toUpperCase(),
    attrs: a,
    children: [],
    parentNode: null,
    dataset: {},
    style: { setProperty() {} },
    listeners,
    disabled: Object.prototype.hasOwnProperty.call(attrs, 'disabled'),
    hidden: Object.prototype.hasOwnProperty.call(attrs, 'hidden'),
    value: attrs.value !== undefined ? attrs.value : '',
    checked: false,
    type: attrs.type || '',
    tabIndex: -1,
    innerHTML: '',
    _text: '',
    get id() { return a.id || ''; },
    set id(v) { a.id = String(v); },
    get className() { return [...classes].join(' '); },
    set className(v) { classes.clear(); String(v).split(/\s+/).filter(Boolean).forEach((c) => classes.add(c)); },
    classList: {
      add: (c) => classes.add(c),
      remove: (c) => classes.delete(c),
      contains: (c) => classes.has(c),
      toggle: (c, force) => {
        const on = force === undefined ? !classes.has(c) : !!force;
        if (on) classes.add(c); else classes.delete(c);
        return on;
      },
    },
    getAttribute(k) { return Object.prototype.hasOwnProperty.call(a, k) ? a[k] : null; },
    setAttribute(k, v) { a[k] = String(v); },
    removeAttribute(k) { delete a[k]; },
    appendChild(c) { c.parentNode = el; el.children.push(c); return c; },
    removeChild(c) { const i = el.children.indexOf(c); if (i >= 0) el.children.splice(i, 1); return c; },
    get firstChild() { return el.children[0] || null; },
    get lastChild() { return el.children[el.children.length - 1] || null; },
    get textContent() { return el._text; },
    set textContent(v) { el.children = []; el._text = String(v); },
    addEventListener(n, fn) { (listeners[n] = listeners[n] || []).push(fn); },
    removeEventListener(n, fn) { listeners[n] = (listeners[n] || []).filter((f) => f !== fn); },
    fire(n, extra) {
      const e = { type: n, target: el, defaultPrevented: false, preventDefault() { e.defaultPrevented = true; }, ...(extra || {}) };
      (listeners[n] || []).slice().forEach((f) => f(e));
      return e;
    },
    click() { if (!el.disabled) el.fire('click'); },
    focus() { doc.activeElement = el; },
    select() {},
    closest() { return null; },
    contains() { return false; },
    getClientRects() { return []; },
    getBoundingClientRect() { return { left: 0, top: 0, width: 0, height: 0 }; },
    scrollIntoView() {},
    querySelector(sel) { return el.querySelectorAll(sel)[0] || null; },
    querySelectorAll(sel) {
      if (sel === 'option') return el.children.filter((c) => c.tagName === 'OPTION');
      return [];
    },
  };
  for (const [k, v] of Object.entries(a)) if (k.startsWith('data-')) el.dataset[k.slice(5)] = v;
  return el;
}

// Every tag with an id in the real markup -> one stub (select gets its <option>s).
function buildDocument() {
  const doc = {
    readyState: 'interactive',
    activeElement: null,
    byId: new Map(),
    all: [],
    listeners: {},
    documentElement: { style: { setProperty() {} } },
  };
  const tagRe = /<([a-zA-Z][a-zA-Z0-9]*)\b([^>]*)>/g;
  let m;
  while ((m = tagRe.exec(HTML))) {
    const tag = m[1].toLowerCase();
    if (tag === 'script' || tag === 'link' || tag === 'meta' || tag === 'style') continue;
    const attrs = parseAttrs(m[2]);
    if (!attrs.id) continue;
    const el = makeElement(tag, attrs, doc);
    if (tag === 'select') {
      const end = HTML.indexOf('</select>', m.index);
      const inner = HTML.slice(m.index + m[0].length, end);
      const optRe = /<option\b([^>]*)>/g;
      let o;
      while ((o = optRe.exec(inner))) {
        const opt = makeElement('option', parseAttrs(o[1]), doc);
        el.appendChild(opt);
        doc.all.push(opt);
      }
      if (el.children.length) el.value = el.children[0].value;
    }
    doc.byId.set(attrs.id, el);
    doc.all.push(el);
  }
  doc.getElementById = (id) => doc.byId.get(id) || null;
  doc.createElement = (tag) => makeElement(tag, {}, doc);
  doc.querySelector = (sel) => (/^section\./.test(sel) ? makeElement('section', {}, doc) : null);
  doc.querySelectorAll = (sel) => {
    if (sel === '[data-command]') return doc.all.filter((e) => e.getAttribute('data-command') !== null);
    if (sel === '[data-command-select]') return doc.all.filter((e) => e.getAttribute('data-command-select') !== null);
    return [];
  };
  doc.addEventListener = (n, fn) => { (doc.listeners[n] = doc.listeners[n] || []).push(fn); };
  doc.removeEventListener = (n, fn) => { doc.listeners[n] = (doc.listeners[n] || []).filter((f) => f !== fn); };
  doc.dispatchEvent = (ev) => { (doc.listeners[ev.type] || []).slice().forEach((f) => f(ev)); return true; };
  return doc;
}

// --- the page harness ---------------------------------------------------------

async function flush(n = 6) { for (let i = 0; i < n; i += 1) await new Promise((r) => setImmediate(r)); }

async function boot({ text = TEXT, context = 'mall', layout = 'split' } = {}) {
  const doc = buildDocument();
  const errors = [];
  const calls = {
    save: [], saveAs: [], reload: [], close: [], goto: [], openInExternal: [], setText: [], recovery: [],
    undo: 0, redo: 0, openSearch: 0, gotoLine: [], setFontSize: [], setTheme: [],
    ep: { manualUpdate: 0, showSaved: 0, toggleMaximize: 0, setLayout: [], findNewFiles: 0, armPicking: [], start: 0, stop: 0, onEdit: 0 },
    prefsSet: [], prefsShow: 0, highContrast: [],
    prepare: [],
  };
  const timers = [];
  const win = {
    document: doc,
    console: {
      log() {}, info() {}, warn() {}, debug() {},
      error: (...args) => errors.push(args.map(String).join(' ')),
    },
    setTimeout: (fn) => { timers.push(fn); return timers.length; },
    clearTimeout() {},
    localStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    sessionStorage: { getItem: () => null, setItem() {}, removeItem() {} },
    listeners: {},
    addEventListener(n, fn) { (win.listeners[n] = win.listeners[n] || []).push(fn); },
    removeEventListener(n, fn) { win.listeners[n] = (win.listeners[n] || []).filter((f) => f !== fn); },
    dispatchEvent(ev) { (win.listeners[ev.type] || []).slice().forEach((f) => f(ev)); return true; },
  };
  win.window = win;

  // -- main-process bridge
  const bridge = {
    saveImpl: async () => ({ ok: true, format: 'plain' }),
    async describe() {
      return { open: true, sessionId: 1, context, sourcePath: '/tmp/x.wrl', format: 'plain', gzip: false, text, baseline: text, profile: 'mall-item' };
    },
    save: (...args) => { calls.save.push(args); return bridge.saveImpl(...args); },
    saveAs: async (...args) => { calls.saveAs.push(args); return { ok: true, sourcePath: '/tmp/y.wrl', format: 'plain' }; },
    reload: async (...args) => { calls.reload.push(args); return { text, format: 'plain' }; },
    close: async (...args) => { calls.close.push(args); return { ok: true }; },
    openInExternal: async (...args) => { calls.openInExternal.push(args); return { editorStatus: { launched: true } }; },
    setText: async (...args) => { calls.setText.push(args); },
    recoveryRecordDirty: async (p) => { calls.recovery.push(p); },
    restore: async () => ({ restored: false }),
  };
  win.vrmlpad = { editor: bridge, goto: async (p) => { calls.goto.push(p); } };

  // -- CodeMirror handle
  const H = { current: text, opts: null };
  let handleTheme = 'dark';
  const handle = {
    getText: () => H.current,
    setDoc: (t) => { H.current = t; },
    undo: () => { calls.undo += 1; },
    redo: () => { calls.redo += 1; },
    openSearch: () => { calls.openSearch += 1; },
    focus() {},
    gotoLine: (n) => { calls.gotoLine.push(n); },
    setFontSize: (n) => { calls.setFontSize.push(n); },
    setTheme: (t) => { handleTheme = t; calls.setTheme.push(t); },
    getTheme: () => handleTheme,
    historyDepth: () => 3,
    isReadOnly: () => false,
    applyVerifiedEdits: () => ({ ok: true }),
    reanalyzeNow() {},
    revealRange() {},
    view: { state: { selection: { main: { head: 0 } } } },
  };
  win.WrlEditor = { create: (_el, opts) => { H.opts = opts; return handle; } };

  // -- scene bridge + views (enough for model.* to report a message)
  const tree = { items: [{ id: 'n1', kind: 'Node', nodeType: 'Transform' }, { id: 'n2', kind: 'Node', nodeType: 'Shape' }] };
  const refusal = () => { calls.prepare.push(1); return { status: 'refused', reason: 'x' }; };
  win.WRLForgeSceneBridge = {
    sceneTree: {
      buildSceneTree: () => tree,
      itemById: (t, id) => (t && t.items.find((i) => i.id === id)) || null,
      itemContainingOffset: () => null,
    },
    identity: { createParseSession: (t, parse) => ({ text: t, parse }) },
    firstObject: {
      displayLabels: () => new Map(),
      objectForItem: () => null,
      prepareAdd: refusal,
      prepareDuplicate: refusal,
      prepareDelete: refusal,
      preparePropertySet: refusal,
      refusalText: () => 'refused text',
    },
  };
  win.WRLForgeSceneTree = { createSceneTreeView: () => ({ setSceneTree() {} }), labelFor: (i) => i.kind };
  win.WRLForgeInspector = { createInspector: () => ({ setFindings() {}, setSceneTree() {}, setNotice() {} }) };

  // -- shared preferences model (real contract: subscribe fires once, synchronously)
  const prefs = { theme: 'dark', zoom: 0, workspaceMode: 'code' };
  const subs = [];
  const notify = () => subs.slice().forEach((f) => f({ ...prefs }));
  win.WrlPreferences = {
    get: (k) => prefs[k],
    set: (k, v) => {
      if (prefs[k] === v) return;
      prefs[k] = v;
      calls.prefsSet.push([k, v]);
      notify();
    },
    subscribe: (fn) => { subs.push(fn); fn({ ...prefs }); return () => { subs.splice(subs.indexOf(fn), 1); }; },
    setHighContrast: (on) => { calls.highContrast.push(on); win.WrlPreferences.set('theme', on ? 'contrast' : 'dark'); },
    show: () => { calls.prefsShow += 1; },
    createButton: () => ({ click() {} }),
  };

  // -- live-preview orchestrator
  const ep = {
    layout, rescanning: false, generation: 1,
    start() { calls.ep.start += 1; },
    stop() { calls.ep.stop += 1; },
    onEdit() { calls.ep.onEdit += 1; },
    manualUpdate() { calls.ep.manualUpdate += 1; },
    showSaved() { calls.ep.showSaved += 1; },
    toggleMaximize() {
      calls.ep.toggleMaximize += 1;
      ep.layout = ep.layout === 'preview-max' ? 'split' : 'preview-max';
      doc.dispatchEvent({ type: 'wrl-editor-preview-state' });
    },
    setLayout(l) { calls.ep.setLayout.push(l); ep.layout = l; doc.dispatchEvent({ type: 'wrl-editor-preview-state' }); },
    findNewFiles() { calls.ep.findNewFiles += 1; },
    armPicking(on, handlers) { calls.ep.armPicking.push([on, handlers]); },
    getLayout: () => ep.layout,
    isRescanning: () => ep.rescanning,
    displayedGeneration: () => ep.generation,
    _state: () => ({ picking: null }),
  };
  win.wrlEditorPreview = ep;

  const ctx = vm.createContext(win);
  for (const s of SCRIPTS) vm.runInContext(s.code, ctx, { filename: s.rel });
  for (let i = 0; i < 20 && !win.__wrlEditor.ready(); i += 1) await flush(1);
  assert.equal(win.__wrlEditor.ready(), true, 'editor page finished init');
  await flush();

  const H2 = {
    doc, win, calls, errors, bridge, handle, ep, prefs, tree, timers,
    ed: win.__wrlEditor,
    byId: (id) => doc.getElementById(id),
    text: () => H.current,
    // Simulate a user edit: CodeMirror updates its doc, then calls onChange.
    edit(next) { H.current = next; H.opts.onChange(); },
    analyse() {
      H.opts.onAnalysis({ version: 1, diagnostics: [], advisories: [], outline: [], parseResult: {}, text: H.current });
    },
    key(init) {
      const ev = {
        type: 'keydown', key: '', ctrlKey: false, metaKey: false, shiftKey: false, altKey: false,
        target: { tagName: 'BODY' }, defaultPrevented: false, preventDefault() { ev.defaultPrevented = true; }, ...init,
      };
      (win.listeners.keydown || []).slice().forEach((f) => f(ev));
      return ev;
    },
    commandList: () => plain(win.__wrlEditor.commands.list()),
    stateEvent: () => doc.dispatchEvent({ type: 'wrl-editor-preview-state' }),
    resetCalls() {
      calls.ep.armPicking.length = 0;
      calls.prefsSet.length = 0;
    },
  };
  return H2;
}

const dirtyText = TEXT + '# edit\n';

// Click-or-key equivalence helper: run `act` on a fresh page, return observable.
async function bothPaths({ prepare, click, key, observe }) {
  const viaClick = await boot();
  if (prepare) await prepare(viaClick);
  const beforeClick = observe(viaClick);
  await click(viaClick);
  await flush();
  const clickDelta = observe(viaClick, beforeClick);

  const viaKey = await boot();
  if (prepare) await prepare(viaKey);
  const beforeKey = observe(viaKey);
  const ev = key(viaKey);
  await flush();
  const keyDelta = observe(viaKey, beforeKey);
  return { clickDelta, keyDelta, ev };
}

// ---------------------------------------------------------------------------
// 1. Every data-command resolves to a registered command; exact id inventory
// ---------------------------------------------------------------------------

test('1. every [data-command] and option data-command in editor.html resolves to a registered command', async () => {
  const h = await boot();
  const registered = new Set(h.commandList().map((c) => c.id));
  const inMarkup = [...HTML.matchAll(/\sdata-command="([^"]+)"/g)].map((m) => m[1]);
  assert.ok(inMarkup.length >= 25, `markup binds ${inMarkup.length} controls`);
  for (const id of inMarkup) assert.ok(registered.has(id), `${id} is registered`);
  // The three layout options really are inside the select.
  const sel = h.byId('previewLayoutSelect');
  assert.deepEqual(sel.querySelectorAll('option').map((o) => o.getAttribute('data-command')),
    ['preview.layout.split', 'preview.layout.previewMax', 'preview.layout.editorOnly']);
  assert.deepEqual(h.errors, [], 'init logged no console.error');
});

test('1b. the registered inventory is exactly §7.2 minus workspace.play / workspace.reset', async () => {
  const h = await boot();
  const ids = h.commandList().map((c) => c.id);
  assert.equal(ids.length, EXPECTED_COMMAND_IDS.length);
  assert.equal(ids.length, 30, 'measured registered command count');
  assert.deepEqual([...ids].sort(), [...EXPECTED_COMMAND_IDS].sort());
  assert.ok(!ids.includes('workspace.play'), 'no Play in #35');
  assert.ok(!ids.includes('workspace.reset'), 'no Reset Workspace in #35');
  assert.ok(!ids.some((id) => id.startsWith('workspace.play')));
  assert.ok(!ids.some((id) => id.startsWith('selection.') || id.startsWith('panel.')), 'reserved areas stay empty');
});

// ---------------------------------------------------------------------------
// 2. Toolbar and keyboard reach the same handler
// ---------------------------------------------------------------------------

test('2. Save: the button and Ctrl+S have the same effect', async () => {
  const r = await bothPaths({
    prepare: async (h) => { h.edit(dirtyText); },
    click: (h) => h.byId('saveBtn').click(),
    key: (h) => h.key({ key: 's', ctrlKey: true }),
    observe: (h) => ({ saves: h.calls.save.length, args: plain(h.calls.save[0] || null), baseline: h.ed.status().dirty }),
  });
  assert.equal(r.clickDelta.saves, 1);
  assert.deepEqual(r.keyDelta, r.clickDelta);
  assert.deepEqual(r.clickDelta.args, [1, dirtyText, false]);
  assert.equal(r.clickDelta.baseline, false, 'saved -> clean');
  assert.equal(r.ev.defaultPrevented, true);
});

test('2. Save As: the button and Ctrl+Shift+S have the same effect', async () => {
  const r = await bothPaths({
    click: (h) => h.byId('saveAsBtn').click(),
    key: (h) => h.key({ key: 'S', ctrlKey: true, shiftKey: true }),
    observe: (h) => ({ saveAs: h.calls.saveAs.length, args: plain(h.calls.saveAs[0] || null) }),
  });
  assert.equal(r.clickDelta.saveAs, 1);
  assert.deepEqual(r.keyDelta, r.clickDelta);
});

test('2. Go to line: the button, Ctrl+G and the Meta variant open the same modal and Go calls handle.gotoLine', async () => {
  for (const how of ['click', 'ctrl', 'meta']) {
    const h = await boot();
    assert.equal(h.ed.modalVisible(), false);
    if (how === 'click') h.byId('gotoBtn').click();
    else h.key({ key: 'g', ctrlKey: how === 'ctrl', metaKey: how === 'meta' });
    await flush();
    assert.equal(h.ed.modalVisible(), true, `${how}: modal backdrop shows`);
    assert.equal(h.byId('modalTitle').textContent, 'Go to line');
    h.byId('modalInput').value = '7';
    h.byId('modalActions').children[0].click(); // Go
    await flush();
    assert.equal(h.ed.modalVisible(), false);
    assert.deepEqual(h.calls.gotoLine, [7], `${how}: exactly one gotoLine(7)`);
  }
});

test('2. Close: the button and Ctrl+W both close the session once', async () => {
  const r = await bothPaths({
    click: (h) => h.byId('closeBtn').click(),
    key: (h) => h.key({ key: 'w', ctrlKey: true }),
    observe: (h) => ({ close: h.calls.close.length, goto: plain(h.calls.goto), stop: h.calls.ep.stop }),
  });
  assert.equal(r.clickDelta.close, 1);
  assert.deepEqual(r.clickDelta.goto, ['mall']);
  assert.deepEqual(r.keyDelta, r.clickDelta);
});

test('2. Zoom in / out / reset: buttons and keys land on the same zoom level', async () => {
  const cases = [
    { btn: 'zoomInBtn', keys: [{ key: '=', ctrlKey: true }, { key: '+', ctrlKey: true, shiftKey: true }, { key: 'Add', ctrlKey: true }, { key: '=', metaKey: true }], from: 0, to: 1 },
    { btn: 'zoomOutBtn', keys: [{ key: '-', ctrlKey: true }, { key: '_', ctrlKey: true, shiftKey: true }, { key: 'Subtract', ctrlKey: true }], from: 0, to: -1 },
    { btn: 'zoomResetBtn', keys: [{ key: '0', ctrlKey: true }], from: 3, to: 0 },
  ];
  for (const c of cases) {
    const viaClick = await boot();
    viaClick.ed.setZoom(c.from);
    viaClick.byId(c.btn).click();
    assert.equal(viaClick.ed.zoom(), c.to, `${c.btn} click`);
    for (const k of c.keys) {
      const viaKey = await boot();
      viaKey.ed.setZoom(c.from);
      const ev = viaKey.key(k);
      assert.equal(viaKey.ed.zoom(), c.to, `${c.btn} key ${JSON.stringify(k)}`);
      assert.equal(ev.defaultPrevented, true);
      assert.equal(viaKey.win.WrlPreferences.get('zoom'), c.to, 'zoom persisted through the shared model');
    }
  }
});

test('2. Preview Update and Maximize: buttons and Ctrl+Enter / Ctrl+Shift+Enter agree', async () => {
  const upd = await bothPaths({
    click: (h) => h.byId('previewUpdateBtn').click(),
    key: (h) => h.key({ key: 'Enter', ctrlKey: true }),
    observe: (h) => ({ n: h.calls.ep.manualUpdate }),
  });
  assert.equal(upd.clickDelta.n, 1);
  assert.deepEqual(upd.keyDelta, upd.clickDelta);

  const max = await bothPaths({
    click: (h) => h.byId('previewMaxBtn').click(),
    key: (h) => h.key({ key: 'Enter', ctrlKey: true, shiftKey: true }),
    observe: (h) => ({ n: h.calls.ep.toggleMaximize, layout: h.ep.layout, pressed: h.byId('previewMaxBtn').getAttribute('aria-pressed') }),
  });
  assert.equal(max.clickDelta.n, 1);
  assert.equal(max.clickDelta.layout, 'preview-max');
  assert.equal(max.clickDelta.pressed, 'true');
  assert.deepEqual(max.keyDelta, max.clickDelta);
});

// ---------------------------------------------------------------------------
// 3. No duplicate execution
// ---------------------------------------------------------------------------

test('3. one click is exactly one handler call; one keydown is exactly one handler call', async () => {
  const h = await boot();
  h.edit(dirtyText);
  h.byId('saveBtn').click();
  await flush();
  assert.equal(h.calls.save.length, 1, 'one Save click -> one bridge.save');

  h.edit(dirtyText + '# again\n');
  h.key({ key: 's', ctrlKey: true });
  await flush();
  assert.equal(h.calls.save.length, 2, 'one Ctrl+S -> exactly one more bridge.save');

  h.byId('previewUpdateBtn').click();
  assert.equal(h.calls.ep.manualUpdate, 1);
  h.key({ key: 'Enter', ctrlKey: true });
  assert.equal(h.calls.ep.manualUpdate, 2);

  h.byId('previewMaxBtn').click();
  h.key({ key: 'Enter', ctrlKey: true, shiftKey: true });
  assert.equal(h.calls.ep.toggleMaximize, 2);

  h.byId('previewSavedBtn').click();
  assert.equal(h.calls.ep.showSaved, 1);
  h.byId('previewFindNewBtn').click();
  assert.equal(h.calls.ep.findNewFiles, 1);

  // Each bound control carries exactly one click listener.
  for (const node of h.doc.all.filter((e) => e.tagName === 'BUTTON' && e.getAttribute('data-command'))) {
    assert.equal((node.listeners.click || []).length, 1, `#${node.id} has one click listener`);
  }
  // And one window keydown listener serves the whole page.
  assert.equal((h.win.listeners.keydown || []).length, 1);
});

// ---------------------------------------------------------------------------
// 4. Disabled state prevents execution through every path
// ---------------------------------------------------------------------------

test('4. Save on a clean document: disabled button, no-op click, Ctrl+S swallowed but not executed', async () => {
  const h = await boot();
  const btn = h.byId('saveBtn');
  assert.equal(btn.disabled, true);
  btn.click(); // a disabled <button> fires nothing
  const ev = h.key({ key: 's', ctrlKey: true });
  assert.equal(ev.defaultPrevented, true, 'claimed shortcut is consumed');
  assert.deepEqual(plain(h.ed.commands.execute('file.save')), { ok: false, reason: 'disabled' });
  await flush();
  assert.equal(h.calls.save.length, 0);
});

test('4. Duplicate / Delete are disabled with no selection and every path refuses', async () => {
  const h = await boot();
  h.analyse();
  for (const [btn, id] of [['duplicateBtn', 'model.duplicate'], ['deleteBtn', 'model.delete']]) {
    assert.equal(h.byId(btn).disabled, true, `${btn} disabled`);
    h.byId(btn).click();
    assert.deepEqual(plain(h.ed.commands.execute(id)), { ok: false, reason: 'disabled' });
  }
  assert.equal(h.calls.prepare.length, 0, 'no plan was prepared');
  // Selecting a Node enables them.
  h.ed.sceneSelectFirst('Transform', 0);
  assert.equal(h.byId('duplicateBtn').disabled, false);
  assert.equal(h.byId('deleteBtn').disabled, false);
});

test('4. workspace.toggleSource is disabled in Code (every path) and works in Model', async () => {
  const h = await boot();
  const btn = h.byId('sourceToggleBtn');
  assert.equal(btn.disabled, true);
  btn.click();
  assert.deepEqual(plain(h.ed.commands.execute('workspace.toggleSource')), { ok: false, reason: 'disabled' });
  assert.equal(h.byId('editorMain').classList.contains('source-open'), false);

  h.byId('modeModelBtn').click();
  assert.equal(btn.disabled, false);
  btn.click();
  assert.equal(h.byId('editorMain').classList.contains('source-open'), true);
  assert.equal(btn.getAttribute('aria-pressed'), 'true');
  btn.click();
  assert.equal(h.byId('editorMain').classList.contains('source-open'), false);
  assert.equal(btn.getAttribute('aria-pressed'), 'false');
});

// ---------------------------------------------------------------------------
// 5. CodeMirror-owned keys are never dispatched by the page
// ---------------------------------------------------------------------------

test('5. Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z / Ctrl+F reach no handler and are not prevented; the buttons run once', async () => {
  const h = await boot();
  for (const k of [
    { key: 'z', ctrlKey: true }, { key: 'y', ctrlKey: true }, { key: 'Z', ctrlKey: true, shiftKey: true },
    { key: 'f', ctrlKey: true }, { key: 'z', metaKey: true },
  ]) {
    const ev = h.key(k);
    assert.equal(ev.defaultPrevented, false, `${JSON.stringify(k)} is left to CodeMirror`);
  }
  assert.equal(h.calls.undo, 0);
  assert.equal(h.calls.redo, 0);
  assert.equal(h.calls.openSearch, 0);

  h.byId('undoBtn').click();
  assert.equal(h.calls.undo, 1);
  h.byId('redoBtn').click();
  assert.equal(h.calls.redo, 1);
  h.byId('findBtn').click();
  assert.equal(h.calls.openSearch, 1);
  h.byId('replaceBtn').click();
  assert.equal(h.calls.openSearch, 2, 'Replace shares Find\'s handler (as before the migration)');
  assert.equal(h.calls.undo, 1);
  assert.equal(h.calls.redo, 1);

  const owners = Object.fromEntries(h.commandList().filter((c) => c.keyOwner === 'editor').map((c) => [c.id, c.keys]));
  assert.deepEqual(Object.keys(owners).sort(), ['edit.find', 'edit.redo', 'edit.undo']);
});

// ---------------------------------------------------------------------------
// 6. Toolbar enabled state equals UI.toolbarModel
// ---------------------------------------------------------------------------

const TOOLBAR_BUTTONS = {
  save: 'saveBtn', saveAs: 'saveAsBtn', reload: 'reloadBtn', undo: 'undoBtn', redo: 'redoBtn',
  find: 'findBtn', replace: 'replaceBtn', gotoLine: 'gotoBtn', external: 'externalBtn', close: 'closeBtn',
};

function assertToolbarMatches(h, model, label) {
  assert.deepEqual(Object.keys(model).sort(), Object.keys(TOOLBAR_BUTTONS).sort(), 'every toolbarModel key has a button');
  for (const [key, id] of Object.entries(TOOLBAR_BUTTONS)) {
    assert.equal(h.byId(id).disabled, !model[key].enabled, `${label}: #${id} vs toolbarModel.${key}`);
  }
}

test('6. toolbar enabled state equals toolbarModel across clean / dirty / saving', async () => {
  const h = await boot();
  assertToolbarMatches(h, UI.toolbarModel({ open: true, dirty: false, saving: false }), 'clean');

  h.edit(dirtyText);
  assertToolbarMatches(h, UI.toolbarModel({ open: true, dirty: true, saving: false }), 'dirty');
  assert.equal(h.byId('saveBtn').disabled, false);

  let release;
  h.bridge.saveImpl = () => new Promise((resolve) => { release = () => resolve({ ok: true, format: 'plain' }); });
  h.byId('saveBtn').click();
  await flush();
  assertToolbarMatches(h, UI.toolbarModel({ open: true, dirty: true, saving: true }), 'saving');
  for (const id of Object.values(TOOLBAR_BUTTONS)) assert.equal(h.byId(id).disabled, true, `saving: #${id}`);
  // A second Ctrl+S while saving must not start a second save.
  h.key({ key: 's', ctrlKey: true });
  assert.equal(h.calls.save.length, 1);

  release();
  await flush();
  assertToolbarMatches(h, UI.toolbarModel({ open: true, dirty: false, saving: false }), 'saved');
});

// ---------------------------------------------------------------------------
// 7. Checked state
// ---------------------------------------------------------------------------

test('7. workspace.code / workspace.model aria-pressed follow the mode', async () => {
  const h = await boot();
  assert.equal(h.byId('modeCodeBtn').getAttribute('aria-pressed'), 'true');
  assert.equal(h.byId('modeModelBtn').getAttribute('aria-pressed'), 'false');
  h.byId('modeModelBtn').click();
  assert.equal(h.byId('modeCodeBtn').getAttribute('aria-pressed'), 'false');
  assert.equal(h.byId('modeModelBtn').getAttribute('aria-pressed'), 'true');
  assert.equal(h.ed.commands.isChecked('workspace.model'), true);
  assert.equal(h.ed.commands.isChecked('workspace.code'), false);
  h.byId('modeCodeBtn').click();
  assert.equal(h.byId('modeCodeBtn').getAttribute('aria-pressed'), 'true');
  assert.equal(h.byId('modeModelBtn').getAttribute('aria-pressed'), 'false');
});

test('7. Maximize aria-pressed and the layout select follow the preview layout after the state event', async () => {
  const h = await boot();
  const max = h.byId('previewMaxBtn');
  const sel = h.byId('previewLayoutSelect');
  assert.equal(max.getAttribute('aria-pressed'), 'false');
  assert.equal(sel.value, 'split');

  h.ep.layout = 'preview-max';
  h.stateEvent();
  assert.equal(max.getAttribute('aria-pressed'), 'true');
  assert.equal(sel.value, 'preview-max');

  h.ep.layout = 'editor-only';
  h.stateEvent();
  assert.equal(max.getAttribute('aria-pressed'), 'false');
  assert.equal(sel.value, 'editor-only');

  h.ep.layout = 'split';
  h.stateEvent();
  assert.equal(sel.value, 'split');

  // The select is a radio group over commands: a change runs the chosen option's command once.
  sel.value = 'preview-max';
  sel.fire('change');
  assert.deepEqual(h.calls.ep.setLayout, ['preview-max']);
  assert.equal(max.getAttribute('aria-pressed'), 'true');

  // "Find new files" follows the rescan state.
  assert.equal(h.byId('previewFindNewBtn').disabled, false);
  h.ep.rescanning = true;
  h.stateEvent();
  assert.equal(h.byId('previewFindNewBtn').disabled, true);
  h.byId('previewFindNewBtn').click();
  assert.equal(h.calls.ep.findNewFiles, 0);
});

test('7. view.highContrast checked follows the WrlPreferences theme (and the command toggles it)', async () => {
  const h = await boot();
  assert.equal(h.ed.commands.isChecked('view.highContrast'), false);
  h.win.WrlPreferences.set('theme', 'contrast'); // e.g. the shared Preferences dialog
  assert.equal(h.ed.commands.isChecked('view.highContrast'), true);
  assert.equal(h.byId('themeSelect').value, 'contrast');
  assert.deepEqual(h.calls.setTheme, ['contrast']);
  h.ed.commands.execute('view.highContrast');
  assert.deepEqual(h.calls.highContrast, [false]);
  assert.equal(h.ed.commands.isChecked('view.highContrast'), false);
  h.ed.commands.execute('view.highContrast');
  assert.equal(h.ed.commands.isChecked('view.highContrast'), true);
});

test('7. Preferences button opens the shared dialog exactly once', async () => {
  const h = await boot();
  h.byId('prefsBtn').click();
  assert.equal(h.calls.prefsShow, 1);
});

// ---------------------------------------------------------------------------
// 8. Workspace regression
// ---------------------------------------------------------------------------

test('8. Code -> Model -> Code keeps source, history and selection; picking is armed only in Model', async () => {
  const h = await boot();
  h.analyse();
  h.ed.sceneSelectFirst('Transform', 0);
  const text0 = h.text();
  const depth0 = h.ed.historyDepth();
  const sel0 = plain(h.ed.sceneSelection());
  assert.equal(sel0.id, 'n1');
  h.resetCalls();
  const main = h.byId('editorMain');
  assert.equal(main.classList.contains('workspace-model'), false);

  h.byId('modeModelBtn').click();
  assert.equal(h.ed.pickState().mode, 'model');
  assert.equal(main.classList.contains('workspace-model'), true);
  assert.equal(h.text(), text0, 'source byte-identical in Model');
  assert.equal(h.ed.historyDepth(), depth0);
  assert.deepEqual(plain(h.ed.sceneSelection()), sel0);
  assert.equal(h.calls.ep.armPicking.at(-1)[0], true, 'viewport picking armed in Model');
  assert.equal(typeof h.calls.ep.armPicking.at(-1)[1].onPick, 'function');
  assert.deepEqual(h.calls.prefsSet, [['workspaceMode', 'model']]);

  h.byId('modeCodeBtn').click();
  assert.equal(h.ed.pickState().mode, 'code');
  assert.equal(main.classList.contains('workspace-model'), false);
  assert.equal(h.text(), text0, 'source byte-identical back in Code');
  assert.equal(h.ed.historyDepth(), depth0);
  assert.deepEqual(plain(h.ed.sceneSelection()), sel0);
  assert.equal(h.calls.ep.armPicking.at(-1)[0], false, 'viewport picking inert in Code');
  assert.ok(h.calls.ep.armPicking.every(([on]) => typeof on === 'boolean'));
  assert.deepEqual(h.calls.prefsSet, [['workspaceMode', 'model'], ['workspaceMode', 'code']]);
  assert.deepEqual(h.errors, []);
});

test('8. workspace.model through the API works; there is no Play and no preference ever becomes "play"', async () => {
  const h = await boot();
  const r = h.ed.commands.execute('workspace.model');
  assert.equal(r.ok, true);
  assert.equal(h.byId('editorMain').classList.contains('workspace-model'), true);
  assert.equal(h.byId('modeModelBtn').getAttribute('aria-pressed'), 'true');
  assert.equal(h.prefs.workspaceMode, 'model');
  assert.throws(() => h.ed.commands.execute('workspace.play'), /ECOMMAND_UNKNOWN/);
  assert.throws(() => h.ed.commands.execute('workspace.reset'), /ECOMMAND_UNKNOWN/);
  assert.ok(!h.commandList().some((c) => c.id.startsWith('workspace.play')));
  // The only workspace values the page ever writes are the persistable ones.
  h.ed.commands.execute('workspace.code');
  for (const [k, v] of h.calls.prefsSet.filter(([k2]) => k2 === 'workspaceMode')) {
    assert.ok(v === 'code' || v === 'model', `${k}=${v}`);
  }
  assert.notEqual(h.prefs.workspaceMode, 'play');
});

test('8. a stray "play" preference value is never applied as a workspace', async () => {
  const h = await boot();
  h.win.WrlPreferences.set('workspaceMode', 'play');
  h.byId('modeCodeBtn').click(); // the page only ever switches via its two active modes
  assert.equal(h.ed.pickState().mode, 'code');
  assert.equal(h.byId('editorMain').classList.contains('workspace-model'), false);
});

// ---------------------------------------------------------------------------
// 9. Model-bar commands report text, independent of the invocation source
// ---------------------------------------------------------------------------

test('9. Box: a click and execute("model.addBox") write the identical refusal text with the error class', async () => {
  const h = await boot();
  h.analyse();
  const status = h.byId('modelStatus');
  assert.equal(h.byId('addBoxBtn').disabled, false);

  h.byId('addBoxBtn').click();
  const viaClick = { text: status.textContent, err: status.classList.contains('err') };
  assert.equal(viaClick.text, 'refused text');
  assert.equal(viaClick.err, true);

  status.textContent = '';
  status.classList.remove('err');
  const r = h.ed.commands.execute('model.addBox');
  assert.equal(r.ok, true);
  assert.equal(status.textContent, viaClick.text, 'same message via the API');
  assert.equal(status.classList.contains('err'), true);
  assert.equal(h.calls.prepare.length, 2);

  // Sphere and the selection-bound commands report the same way.
  h.byId('addSphereBtn').click();
  assert.equal(status.textContent, 'refused text');
  h.ed.sceneSelectFirst('Transform', 0);
  status.textContent = '';
  h.byId('duplicateBtn').click();
  assert.equal(status.textContent, 'refused text');
  status.textContent = '';
  h.ed.commands.execute('model.delete');
  assert.equal(status.textContent, 'refused text');
});

// ---------------------------------------------------------------------------
// 10. Panels
// ---------------------------------------------------------------------------

test('10. the panel registry lists exactly the eight existing panels (no console)', async () => {
  const h = await boot();
  const ids = plain(h.ed.panels.list()).map((p) => p.id);
  assert.deepEqual(ids, EXPECTED_PANEL_IDS);
  assert.ok(!ids.includes('console'));
  assert.throws(() => h.ed.panels.isVisible('console'), /EPANEL_UNKNOWN/);
});

// ---------------------------------------------------------------------------
// 11. Keyboard coverage (§26)
// ---------------------------------------------------------------------------

// view.highContrast is the one registered command with neither keys nor a bound
// toolbar control. Keep this list to exactly that one entry.
const ALLOWED_WITHOUT_TOOLBAR_CONTROL = Object.freeze({
  'view.highContrast': 'reachable through the shared Preferences dialog checkbox; the dialog is shared with the Mall/World pages and stays widget-local in #35',
});

test('11. every registered command has keys or a bound focusable [data-command] control', async () => {
  assert.deepEqual(Object.keys(ALLOWED_WITHOUT_TOOLBAR_CONTROL), ['view.highContrast'], 'exactly one allowed exception');
  const h = await boot();
  const controls = new Map(); // id -> true when a button or a select option binds it
  for (const node of h.doc.all) {
    const id = node.getAttribute('data-command');
    if (!id) continue;
    const focusable = node.tagName === 'BUTTON' || (node.tagName === 'OPTION' && node.parentNode && node.parentNode.tagName === 'SELECT');
    if (focusable) controls.set(id, true);
  }
  const uncovered = [];
  for (const c of h.commandList()) {
    if (c.keys.length > 0 || controls.has(c.id)) continue;
    uncovered.push(c.id);
  }
  assert.deepEqual(uncovered, Object.keys(ALLOWED_WITHOUT_TOOLBAR_CONTROL), 'only the documented exception lacks a control');
  // The exception is really key-less and control-less (the list is accurate).
  const hc = h.commandList().find((c) => c.id === 'view.highContrast');
  assert.deepEqual(hc.keys, []);
  assert.equal(controls.has('view.highContrast'), false);
  // Every key-owning command with an app owner really dispatches.
  const dispatched = h.commandList().filter((c) => c.keys.length && c.keyOwner === 'app').map((c) => c.id).sort();
  assert.deepEqual(dispatched, [
    'edit.gotoLine', 'file.close', 'file.save', 'file.saveAs', 'preview.toggleMaximize', 'preview.update',
    'view.zoomIn', 'view.zoomOut', 'view.zoomReset',
  ]);
});

// ---------------------------------------------------------------------------
// 12. Source scans (regression guards only)
// ---------------------------------------------------------------------------

test('12. no direct click listener remains on a migrated element id (editor.js, model-workspace.js, editor-preview.js)', () => {
  const migratedIds = [...HTML.matchAll(/<button\b[^>]*\sid="([^"]+)"[^>]*\sdata-command="/g)].map((m) => m[1]);
  assert.ok(migratedIds.length >= 25, `found ${migratedIds.length} migrated button ids`);
  const files = ['renderer/editor.js', 'renderer/model-workspace.js', 'renderer/editor-preview.js'];
  const oldNames = [
    'save', 'saveAs', 'reload', 'undo', 'redo', 'find', 'replace', 'goto', 'external', 'close', 'back',
    'zoomIn', 'zoomOut', 'zoomReset', 'modelBtn', 'codeBtn', 'sourceBtn', 'addBox', 'addSphere', 'duplicate', 'remove',
  ];
  for (const rel of files) {
    const src = read(rel);
    for (const id of migratedIds) {
      assert.ok(!new RegExp(`(?:getElementById|\\bel)\\(\\s*['"]${id}['"]\\s*\\)[^;\\n]*addEventListener\\(\\s*['"]click`).test(src),
        `${rel}: no inline click wiring on #${id}`);
      for (const v of src.matchAll(new RegExp(`(?:const|let|var)\\s+(\\w+)\\s*=\\s*(?:document\\.getElementById|\\bel)\\(\\s*['"]${id}['"]\\s*\\)`, 'g'))) {
        assert.ok(!new RegExp(`\\b${v[1]}\\.addEventListener\\(\\s*['"]click`).test(src),
          `${rel}: variable ${v[1]} (#${id}) has no click listener`);
        assert.ok(!new RegExp(`\\b${v[1]}\\.onclick\\b`).test(src), `${rel}: ${v[1]} has no onclick`);
      }
    }
    for (const n of oldNames) {
      assert.ok(!new RegExp(`\\bels\\.${n}\\.addEventListener\\(\\s*['"]click`).test(src), `${rel}: no els.${n} click listener`);
      assert.ok(!new RegExp(`\\bels\\.${n}\\.onclick\\b`).test(src), `${rel}: no els.${n}.onclick`);
    }
    assert.ok(!/getElementById\(\s*'previewUpdateBtn'\s*\)/.test(src), `${rel}: no previewUpdateBtn click wiring`);
  }
});

test('12. editor.js no longer carries the old resolveShortcut keydown chain', () => {
  const src = read('renderer/editor.js');
  assert.ok(!src.includes('UI.resolveShortcut('), 'no UI.resolveShortcut( call');
  assert.ok(!/(?:window|document)\.addEventListener\(\s*['"]keydown['"]/.test(src), 'no page-level keydown listener in editor.js');
  assert.match(src, /CommandBindings\.installKeyboard\(/);
  assert.match(src, /CommandBindings\.bindControls\(/);
});
