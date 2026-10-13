'use strict';
// WD2-C ("First Object") real-Electron acceptance QA.
//
// Drives the REAL editor page -- CodeMirror bundle, Model workspace, scene tree,
// Object panel, Inspector, the Phase 7C2 unsaved-buffer X_ITE preview and the
// existing safe-save path -- through ONE reused capture-server process via
// VisualQaRunner. Every interaction goes through the page's own
// window.__wrlEditor hooks, which click the page's own buttons and type into the
// page's own inputs. Nothing is injected into the document.
//
//   node qa/wd2-c-first-object/orchestrate.js
//
// SOURCE-DIFF ORACLE: every expected buffer below is a LITERAL string written
// out by hand from the WD2-C generation convention -- not computed by the code
// under test -- and each step is checked with bufferEquals (exact, whole
// buffer). The per-step "only its owned region changed" proof is then
// recomputed here from consecutive literals (prefix/suffix comparison).
//
// Electron runs with --user-data-dir under the OS temp dir, so the owner's real
// preferences / recovery files are never read or written. Sources are scratch
// files under the OS temp dir (the capture server refuses anything else).

const fs = require('fs');
const os = require('os');
const path = require('path');
const zlib = require('zlib');
const { spawn } = require('child_process');
const { VisualQaRunner } = require('../visual-qa/runner');
const { acquire } = require('../visual-qa/lock');
const { makeCaptureTransport } = require('../visual-qa/transport');
const vrml = require('../../src/vrml');

const repoRoot = path.join(__dirname, '..', '..');
const OUT = path.join(__dirname, 'screenshots');
const SIZE = '1500x950';
const SETTLE = 2600; // > the 700 ms preview debounce + an X_ITE render

// --- the literal oracle -------------------------------------------------------
const H = '#VRML V2.0 utf8\n';
const obj = (prim, { rot, pos, size, radius, color } = {}) => [
  'Transform {',
  ...(rot ? [`  rotation ${rot}`] : []),
  ...(pos ? [`  translation ${pos}`] : []),
  '  children [',
  '    Shape {',
  '      appearance Appearance {',
  '        material Material {',
  ...(color ? [`          diffuseColor ${color}`] : []),
  '        }',
  '      }',
  `      geometry ${prim} {`,
  ...(size ? [`        size ${size}`] : []),
  ...(radius ? [`        radius ${radius}`] : []),
  '      }',
  '    }',
  '  ]',
  '}',
].join('\n');

const BOX0 = obj('Box');
const BOX_POS = obj('Box', { pos: '3 0 0' });
const BOX_SIZE = obj('Box', { pos: '3 0 0', size: '2 1 0.5' });
const BOX_ROT = obj('Box', { rot: '0 0 1 0.5', pos: '3 0 0', size: '2 1 0.5' });
const BOX_COLOR = obj('Box', { rot: '0 0 1 0.5', pos: '3 0 0', size: '2 1 0.5', color: '1 0 0' });
const SPH0 = obj('Sphere');
const SPH_R = obj('Sphere', { radius: '2' });

const T = {
  empty: '',
  add: `${H}\n${BOX0}\n`,
  position: `${H}\n${BOX_POS}\n`,
  size: `${H}\n${BOX_SIZE}\n`,
  rotation: `${H}\n${BOX_ROT}\n`,
  color: `${H}\n${BOX_COLOR}\n`,
  duplicate: `${H}\n${BOX_COLOR}\n${BOX_COLOR}\n`,
};
T.afterDelete = T.color;
T.addSphere = `${T.duplicate}\n${SPH0}\n`;
T.radius = `${T.duplicate}\n${SPH_R}\n`;

const SHARED = H
  + 'DEF Lamp Transform { children [ Shape { geometry Sphere { } } ] }\n'
  + 'Group { children [ USE Lamp ] }\n';
const BROKEN = `${H}Transform { children [ Shape {\n`;
const CONTRAST = `${H}\n${BOX_COLOR}\n`;

function stage(name, text) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-wd2cqa-'));
  const p = path.join(dir, name);
  fs.writeFileSync(p, text, 'utf8');
  return p;
}

const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-wd2cqa-profile-'));
function realSpawn(extraEnv = {}) {
  return spawn(require('electron'), ['.', '--no-sandbox', `--user-data-dir=${userData}`], {
    cwd: repoRoot,
    env: { ...process.env, WRL_FORGE_CAPTURE_SERVER: '1', WRL_FORGE_NO_EDITOR: '1', WRL_FORGE_SETTLE_MS: '1500', ...extraEnv },
    stdio: ['pipe', 'pipe', 'inherit'],
  });
}

const png = (name) => path.join(OUT, name + '.png');
const call = (name, args, label, wait) => ({ call: name, args: args || [], label: label || null, wait: wait || 0 });
const look = (tag) => [
  call('bufferEquals', [null], `buf-${tag}`), // placeholder replaced below
  call('modelState', [], `model-${tag}`),
  call('sceneSelection', [], `sel-${tag}`),
  call('historyDepth', [], `hist-${tag}`),
  call('previewScene', [], `scene-${tag}`),
  call('previewBBox', [], `bbox-${tag}`),
];
const snap = (tag, text) => {
  const steps = look(tag);
  steps[0] = call('bufferEquals', [text], `buf-${tag}`);
  return steps;
};
const t0 = Date.now();
const timed = (label) => call('modelState', [], label); // timestamps come from step order in RESULTS

function firstObjectSteps() {
  return [
    ...snap('start', T.empty),
    call('focusInfo', [], 'focus-start'),
    { capture: png('01-empty-model-mode') },
    // 1. Add Box (one click)
    call('click', ['addBoxBtn'], 'add-box', SETTLE),
    ...snap('add', T.add),
    call('focusInfo', [], 'focus-add'),
    { capture: png('02-box-created-selected') },
    // 2. Position X = 3 (type, Enter)
    call('propSet', ['position', 0, '3'], 'type-pos'),
    call('propKey', ['position', 0, 'Enter'], 'enter-pos', SETTLE),
    ...snap('position', T.position),
    call('focusInfo', [], 'focus-position'),
    call('inspectorFields', [], 'inspector-position'),
    { capture: png('03-position-x3') },
    // 3. Size 2 1 0.5 (type three, Apply via Enter on the last)
    call('propSet', ['size', 0, '2'], 'type-size-x'),
    call('propSet', ['size', 1, '1'], 'type-size-y'),
    call('propSet', ['size', 2, '0.5'], 'type-size-z'),
    call('propKey', ['size', 2, 'Enter'], 'enter-size', SETTLE),
    ...snap('size', T.size),
    { capture: png('04-size') },
    // 4. Rotation angle 0.5 rad
    call('propSet', ['rotation', 3, '0.5'], 'type-rot'),
    call('propKey', ['rotation', 3, 'Enter'], 'enter-rot', SETTLE),
    ...snap('rotation', T.rotation),
    // invalid input: refused, nothing changes
    call('propSet', ['position', 1, 'abc'], 'type-invalid'),
    call('propKey', ['position', 1, 'Enter'], 'enter-invalid', 400),
    call('bufferEquals', [T.rotation], 'buf-invalid'),
    call('modelState', [], 'model-invalid'),
    call('historyDepth', [], 'hist-invalid'),
    call('propKey', ['position', 1, 'Escape'], 'escape-invalid', 200),
    // 5. Color red through the colour control
    call('propColor', ['#ff0000'], 'pick-red', SETTLE),
    ...snap('color', T.color),
    { capture: png('05-color-red') },
    // 6. Duplicate
    call('click', ['duplicateBtn'], 'duplicate', SETTLE),
    ...snap('duplicate', T.duplicate),
    { capture: png('06-duplicated') },
    // 7. Undo / Redo the duplicate
    call('click', ['undoBtn'], 'undo-dup', SETTLE),
    ...snap('undo-dup', T.color),
    call('inspectorFields', [], 'inspector-undo-dup'),
    call('click', ['redoBtn'], 'redo-dup', SETTLE),
    ...snap('redo-dup', T.duplicate),
    // 8. Delete the duplicate (select it in the tree first), then Undo Delete
    call('sceneSelectFirst', ['Transform', 1], 'select-copy', 300),
    call('click', ['deleteBtn'], 'delete', SETTLE),
    ...snap('delete', T.afterDelete),
    { capture: png('07-deleted') },
    call('click', ['undoBtn'], 'undo-delete', SETTLE),
    ...snap('undo-delete', T.duplicate),
    // 9. Add Sphere, Radius 2
    call('click', ['addSphereBtn'], 'add-sphere', SETTLE),
    ...snap('add-sphere', T.addSphere),
    call('propSet', ['radius', 0, '2'], 'type-radius'),
    call('propKey', ['radius', 0, 'Enter'], 'enter-radius', SETTLE),
    ...snap('radius', T.radius),
    call('inspectorFields', [], 'inspector-radius'),
    { capture: png('08-sphere-radius') },
    // 10. Open Source: the exact buffer is right there
    call('click', ['sourceToggleBtn'], 'show-source', 800),
    call('modelState', [], 'model-source'),
    { capture: png('09-source-open') },
    // 11. Save through the existing safe-save path
    call('click', ['saveBtn'], 'save', 1800),
    call('status', [], 'status-saved'),
    timed('end'),
  ];
}

function reopenSteps() {
  return [
    call('modelState', [], 'model-open'),
    call('sceneSelectFirst', ['Transform', 0], 'select-first', 400),
    call('modelState', [], 'model-selected'),
    call('previewScene', [], 'scene-open'),
    { capture: png('10-reopened-code-mode') },
    call('click', ['modeModelBtn'], 'switch-model', 800),
    call('modelState', [], 'model-switched'),
    { capture: png('11-switched-to-model') },
  ];
}

function refusalSteps() {
  return [
    call('modelState', [], 'model-open'),
    call('sceneSelectFirst', ['Transform', 0], 'select-def', 400),
    call('click', ['duplicateBtn'], 'dup-def', 600),
    call('modelState', [], 'model-dup-def'),
    call('bufferEquals', [SHARED], 'buf-dup-def'),
    call('click', ['deleteBtn'], 'del-ref', 600),
    call('modelState', [], 'model-del-ref'),
    call('bufferEquals', [SHARED], 'buf-del-ref'),
    call('historyDepth', [], 'hist-refusals'),
    { capture: png('12-refusals') },
    call('setText', [BROKEN], 'break', 1500),
    call('modelState', [], 'model-broken'),
    call('click', ['addBoxBtn'], 'add-broken', 600),
    call('modelState', [], 'model-add-broken'),
    call('bufferEquals', [BROKEN], 'buf-add-broken'),
    { capture: png('13-damaged-source-opened') },
  ];
}

function contrastSteps() {
  return [
    call('sceneSelectFirst', ['Transform', 0], 'select', 500),
    call('propKey', ['position', 0, 'Tab'], 'focus', 200),
    call('modelState', [], 'model'),
    { capture: png('14-contrast-zoom-model') },
  ];
}

function findStep(job, label) {
  const s = job && job.steps ? job.steps.find((x) => x.label === label) : null;
  return s ? s.result : undefined;
}

// Only [from, to) of `a` became `insert` in `b`: the longest common prefix and
// suffix, reported as the owned region of one step.
function changedRegion(a, b) {
  let p = 0;
  while (p < a.length && p < b.length && a[p] === b[p]) p += 1;
  let s = 0;
  while (s < a.length - p && s < b.length - p && a[a.length - 1 - s] === b[b.length - 1 - s]) s += 1;
  return { from: p, removed: a.slice(p, a.length - s), inserted: b.slice(p, b.length - s) };
}

async function main() {
  if (process.platform !== 'win32' && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
    console.error('wd2-c-first-object: no DISPLAY/WAYLAND_DISPLAY -- refusing to launch Electron headless-blind.');
    process.exit(2);
  }
  const transport = makeCaptureTransport();
  fs.mkdirSync(OUT, { recursive: true });
  const primary = stage('first-object.wrl', T.empty);
  const shared = stage('shared.wrl', SHARED);
  const contrast = stage('contrast.wrl', CONTRAST);
  const scratch = [primary, shared, contrast].map((p) => path.dirname(p));
  const mall = (p, extra) => ({ context: 'mall', mallPath: p, previewLayout: 'split', fitMode: 'original', captureConsole: true, ...extra });

  const log = [];
  const runner = new VisualQaRunner({
    spawn: () => realSpawn(transport.env),
    maxLaunches: 2,
    retriesPerLaunch: 1,
    captureTimeoutMs: 240000,
    log: (rec) => { log.push(rec); process.stdout.write(JSON.stringify(rec) + '\n'); },
    ...transport.runnerOpts,
  });

  const release = acquire();
  let results = [];
  let reopen = null;
  let runError = null;
  try {
    // The reopen job opens the SAME scratch file after the first job saved it
    // (jobs run in order in one process), i.e. exactly the saved bytes.
    results = await runner.run([
      { id: 'first-object', editor: mall(primary, { steps: firstObjectSteps() }), size: SIZE },
      { id: 'reopen', editor: mall(primary, { steps: reopenSteps() }), size: SIZE },
      { id: 'refusals', editor: mall(shared, { steps: refusalSteps() }), size: SIZE },
      { id: 'contrast-zoom', editor: mall(contrast, { theme: 'contrast', zoom: 4, steps: contrastSteps() }), size: SIZE },
    ]);
    reopen = results.find((r) => r.id === 'reopen');
  } catch (err) {
    runError = String((err && err.message) || err);
  } finally {
    release();
    transport.cleanup();
  }

  let savedText = null;
  try {
    const raw = fs.readFileSync(primary);
    savedText = (raw[0] === 0x1f && raw[1] === 0x8b ? zlib.gunzipSync(raw) : raw).toString('utf8');
  } catch { savedText = null; }

  const P = results.find((r) => r.id === 'first-object');
  const R = results.find((r) => r.id === 'refusals');
  const C = results.find((r) => r.id === 'contrast-zoom');
  const g = (job, label) => findStep(job, label);
  const prop = (job, label, key) => { const m = g(job, label); return m && m.props ? m.props.find((x) => x.key === key) : null; };
  const depth = (label) => (g(P, `hist-${label}`) || {}).undo;
  const checks = [];
  const check = (name, ok, detail) => checks.push({ name, ok: !!ok, detail: detail === undefined ? null : detail });

  // --- Model mode on an empty document ---
  const m0 = g(P, 'model-start');
  check('1-3 empty document opens in MODEL mode: preview visible, Source collapsed (not removed)',
    m0 && m0.mode === 'model' && m0.previewVisible && !m0.editorVisible && /workspace-model/.test(m0.mainClass), m0);
  check('Add Box / Add Sphere visible and enabled, labelled', m0 && ['addBoxBtn', 'addSphereBtn'].every((id) => {
    const b = m0.buttons.find((x) => x.id === id); return b && !b.disabled && !b.hidden && /^Add (Box|Sphere)$/.test(b.name);
  }), m0 && m0.buttons);
  check('keyboard focus starts on Add Box', (g(P, 'focus-start') || {}).id === 'addBoxBtn', g(P, 'focus-start'));
  check('empty document is not persisted as a remembered mode', m0 && m0.remembered === 'code', m0 && m0.remembered);

  // --- Add Box ---
  check('5 Add Box -> exact source (header + one generated object, nothing else)', g(P, 'buf-add') === true);
  const ma = g(P, 'model-add');
  const sa = g(P, 'sel-add');
  check('5 the new Box is the selection (tree row + Object panel agree)', sa && sa.nodeType === 'Transform' && sa.rowId === sa.id
    && ma && ma.selected === 'Selected: Box' && ma.title === 'Box', { sa, selected: ma && ma.selected, title: ma && ma.title });
  check('status says where it was created', ma && ma.status === 'Box created at origin.', ma && ma.status);
  check('tree shows the display label "Box" with the node type kept visible', ma && ma.treeRows.some((r) => r === 'BoxTransform'), ma && ma.treeRows);
  check('beginner labels for a Box: Position, Rotation, Size, Color (VRML names secondary)',
    ma && ma.props.map((p) => p.label).join() === 'Position,Rotation,Size,Color'
      && ma.props.map((p) => p.tech).join() === 'translation · SFVec3f,rotation · SFRotation,size · SFVec3f,diffuseColor · SFColor', ma && ma.props);
  check('absent fields show schema defaults (0 0 0 / 0 0 1 0 / 2 2 2 / #cccccc)',
    prop(P, 'model-add', 'position').values.join(' ') === '0 0 0' && prop(P, 'model-add', 'rotation').values.join(' ') === '0 0 1 0'
      && prop(P, 'model-add', 'size').values.join(' ') === '2 2 2' && prop(P, 'model-add', 'color').color === '#cccccc');
  const sc1 = g(P, 'scene-add');
  check('6 X_ITE shows the new Box (1 Transform, 1 Box size 2 2 2 at the origin)',
    sc1 && sc1.counts.Box === 1 && sc1.boxes[0].join() === '2,2,2' && sc1.translations[0].join() === '0,0,0', sc1);
  check('Add = exactly ONE undo step', depth('add') === depth('start') + 1, [depth('start'), depth('add')]);

  // --- Position ---
  check('8-9 Position X=3 -> exact source: one inserted "translation 3 0 0" line', g(P, 'buf-position') === true);
  const sp = g(P, 'sel-position');
  check('the same Box stays selected after Position', sp && sp.nodeType === 'Transform' && sp.ordinal === 0, sp);
  check('10 X_ITE Box moved to X=3 (translation and world bounds)', (g(P, 'scene-position') || {}).translations[0].join() === '3,0,0'
    && Math.abs((g(P, 'bbox-position') || { min: [NaN] }).min[0] - 2) < 1e-6 && Math.abs((g(P, 'bbox-position') || { max: [NaN] }).max[0] - 4) < 1e-6,
  { scene: g(P, 'scene-position'), bbox: g(P, 'bbox-position') });
  check('"Applied." and focus back on Position X', prop(P, 'model-position', 'position').message === 'Applied.'
    && (g(P, 'focus-position') || {}).id === 'prop-position-0', { msg: prop(P, 'model-position', 'position').message, focus: g(P, 'focus-position') });
  check('Position = exactly ONE undo step', depth('position') === depth('add') + 1);
  const ip = ((g(P, 'inspector-position') || {}).fields || []).find((f) => f.name === 'translation');
  check('technical Inspector shows the same value on the same node (translation 3 0 0, SFVec3f, editable)',
    ip && ip.values.join(' ') === '3 0 0' && ip.type === 'SFVec3f' && ip.state === 'editable'
      && prop(P, 'model-position', 'position').values.join(' ') === '3 0 0', ip);

  // --- Size / Rotation ---
  check('11 Size 2 1 0.5 -> exact source', g(P, 'buf-size') === true);
  check('12 X_ITE Box dimensions changed', (g(P, 'scene-size') || {}).boxes[0].join() === '2,1,0.5', g(P, 'scene-size'));
  check('Size (3 components) = exactly ONE undo step', depth('size') === depth('position') + 1);
  check('Rotation angle 0.5 -> exact source (absent rotation inserted)', g(P, 'buf-rotation') === true && depth('rotation') === depth('size') + 1);
  const pi = prop(P, 'model-invalid', 'position');
  check('invalid input is refused before the document; message is text', g(P, 'buf-invalid') === true
    && depth('invalid') === depth('rotation') && /^Invalid: /.test(pi.message), pi);

  // --- Color ---
  check('13 Color via the colour control -> exactly "diffuseColor 1 0 0" inserted', g(P, 'buf-color') === true);
  check('the colour control does not commit while dragging (input), only on change', (g(P, 'pick-red') || {}).committedOnInput === false, g(P, 'pick-red'));
  check('14 X_ITE Material is red', ((g(P, 'scene-color') || {}).colors || [])[0] && g(P, 'scene-color').colors[0].join() === '1,0,0', g(P, 'scene-color'));
  check('Color = exactly ONE undo step', depth('color') === depth('rotation') + 1);

  // --- Duplicate / Undo / Redo / Delete ---
  check('15 Duplicate -> exact bytes of the object copied right after it', g(P, 'buf-duplicate') === true);
  const sd = g(P, 'sel-duplicate');
  check('the COPY (second Transform) is selected after Duplicate', sd && sd.nodeType === 'Transform' && sd.ordinal === 1 && sd.rowId === sd.id, sd);
  check('16 two objects exist in X_ITE', (g(P, 'scene-duplicate') || {}).counts.Box === 2, g(P, 'scene-duplicate'));
  check('Duplicate = exactly ONE undo step', depth('duplicate') === depth('color') + 1);
  check('17-18 one Undo -> one object, exact source', g(P, 'buf-undo-dup') === true && (g(P, 'scene-undo-dup') || {}).counts.Box === 1);
  check('Undo of Duplicate loses the (removed) selection visibly, never a wrong one', g(P, 'sel-undo-dup') === null
    && /Selection cleared/.test((g(P, 'inspector-undo-dup') || {}).notice || ''), g(P, 'inspector-undo-dup'));
  check('19 Redo -> exact duplicated source', g(P, 'buf-redo-dup') === true);
  check('20 Delete the duplicate -> exact source; selection cleared; status text',
    g(P, 'buf-delete') === true && g(P, 'sel-delete') === null && (g(P, 'model-delete') || {}).status === 'Box deleted.', g(P, 'model-delete'));
  check('Delete = exactly ONE undo step', depth('delete') === depth('redo-dup') + 1);
  check('21 Undo Delete -> exact source restored', g(P, 'buf-undo-delete') === true && (g(P, 'scene-undo-delete') || {}).counts.Box === 2);

  // --- Sphere ---
  check('22 Add Sphere -> exact source; Sphere selected', g(P, 'buf-add-sphere') === true && (g(P, 'model-add-sphere') || {}).selected === 'Selected: Sphere');
  check('Sphere labels: Position, Rotation, Radius, Color (radius is NOT "Size")',
    (g(P, 'model-add-sphere') || { props: [] }).props.map((p) => p.label).join() === 'Position,Rotation,Radius,Color');
  check('23 Radius 2 -> exact source', g(P, 'buf-radius') === true);
  check('24 X_ITE Sphere radius 2', ((g(P, 'scene-radius') || {}).spheres || []).join() === '2', g(P, 'scene-radius'));
  const ir = g(P, 'inspector-radius');
  check('technical Inspector agrees: the selected Sphere object\'s Transform lists only its children (no invented fields)',
    ir && ir.fields.map((f) => f.name).join() === 'children', ir && ir.fields);

  // --- Source / Save / reopen ---
  const ms = g(P, 'model-source');
  check('25 Show Source reveals the CodeMirror buffer in Model mode', ms && ms.editorVisible && ms.sourceOpen
    && (ms.buttons.find((b) => b.id === 'sourceToggleBtn') || {}).pressed === 'true', ms);
  const parsed = savedText !== null ? vrml.parse(savedText) : null;
  check('26-27 Save writes the exact final source; it is valid VRML97 (0 syntax errors)',
    savedText === T.radius && parsed && parsed.syntaxDiagnostics.filter((d) => d.severity === 'error').length === 0
      && (g(P, 'status-saved') || {}).dirty === false, { saved: savedText === T.radius, status: g(P, 'status-saved') });
  const ro = g(reopen, 'model-open');
  check('28 reopened saved file: existing document opens in the remembered (Code) workspace', ro && ro.mode === 'code' && ro.editorVisible, ro);
  const rs = g(reopen, 'model-selected');
  check('29 reopened scene valid: Box properties read back 3 0 0 / 2 1 0.5 / red', rs && rs.title === 'Box'
    && rs.props.find((p) => p.key === 'position').values.join(' ') === '3 0 0'
    && rs.props.find((p) => p.key === 'size').values.join(' ') === '2 1 0.5'
    && rs.props.find((p) => p.key === 'color').color === '#ff0000', rs);
  check('reopened preview: 3 objects (2 Box + 1 Sphere)', ((g(reopen, 'scene-open') || {}).counts || {}).Transform === 3, g(reopen, 'scene-open'));
  const rsw = g(reopen, 'model-switched');
  check('switching to Model is remembered through the existing preferences', rsw && rsw.mode === 'model' && rsw.remembered === 'model', rsw);

  // --- refusals / damaged ---
  const rr0 = g(R, 'model-open');
  check('remembered Model mode applies to the next existing document', rr0 && rr0.mode === 'model', rr0);
  const rd = g(R, 'model-dup-def');
  check('Duplicate of a DEF object refused with a plain sentence; source unchanged', g(R, 'buf-dup-def') === true && rd && rd.statusError
    && /DEF name that would conflict/.test(rd.status), rd && rd.status);
  const rdl = g(R, 'model-del-ref');
  check('Delete of a referenced object refused, naming the reference; source unchanged', g(R, 'buf-del-ref') === true && rdl && rdl.statusError
    && /other nodes reference it \(Lamp\)/.test(rdl.status), rdl && rdl.status);
  check('refusals add no undo step', (g(R, 'hist-refusals') || {}).undo === 0, g(R, 'hist-refusals'));
  const rb = g(R, 'model-broken');
  check('damaged document: Source opens automatically with a text explanation', rb && rb.sourceOpen && rb.editorVisible && rb.statusError
    && /syntax errors/.test(rb.status), rb);
  const rab = g(R, 'model-add-broken');
  check('damaged document: Add refused, nothing written', g(R, 'buf-add-broken') === true && rab && /syntax errors/.test(rab.status), rab && rab.status);

  // --- accessibility visual ---
  const cm = g(C, 'model');
  check('High Contrast + zoom: Object panel renders with accessible names', cm && cm.props.length === 4
    && cm.props[0].names[0] === 'Position X', cm && cm.props[0]);

  // --- per-step owned-region proof (literal oracle) ---
  const chain = [
    ['add', T.empty, T.add], ['position', T.add, T.position], ['size', T.position, T.size],
    ['rotation', T.size, T.rotation], ['color', T.rotation, T.color], ['duplicate', T.color, T.duplicate],
    ['delete', T.duplicate, T.afterDelete], ['undo-delete', T.afterDelete, T.duplicate],
    ['add-sphere', T.duplicate, T.addSphere], ['radius', T.addSphere, T.radius],
  ];
  const regions = chain.map(([name, a, b]) => ({ name, ...changedRegion(a, b) }));
  const expectInsert = {
    position: '  translation 3 0 0\n', size: '        size 2 1 0.5\n', rotation: '  rotation 0 0 1 0.5\n',
    color: '          diffuseColor 1 0 0\n', radius: '        radius 2\n',
  };
  check('source-diff: each property step inserted exactly one field line and removed nothing',
    Object.entries(expectInsert).every(([k, ins]) => {
      const r = regions.find((x) => x.name === k);
      return r && r.removed === '' && r.inserted.length === ins.length && (`${r.inserted}`.includes(ins.trim()));
    }), regions.filter((r) => expectInsert[r.name]));
  check('source-diff: Duplicate only inserted a byte copy of the object; Delete only removed it',
    regions.find((r) => r.name === 'duplicate').removed === '' && regions.find((r) => r.name === 'duplicate').inserted.replace(/^\n?/, '').includes(BOX_COLOR)
      && regions.find((r) => r.name === 'delete').inserted === '' && regions.find((r) => r.name === 'delete').removed.includes(BOX_COLOR));

  // --- console + process hygiene ---
  const consoleEntries = results.flatMap((r) => (r.console || []).map((c) => ({ job: r.id, ...c })));
  const bad = consoleEntries.filter((c) => c.level === 'error' || c.level === 'warning');
  check('no unexpected console error or warning', bad.length === 0, bad);
  const survivors = runner.survivors();
  check('zero surviving Electron processes', survivors.length === 0, survivors);
  check('all jobs completed', !runError && results.length === 4, runError);

  const passed = checks.filter((c) => c.ok).length;
  const report = {
    launches: runner.launchesUsed,
    runError,
    survivors,
    wallClockMs: Date.now() - t0,
    assertions: { total: checks.length, passed, failed: checks.length - passed },
    checks,
    sourceDiffRegions: regions,
    savedTextMatchesOracle: savedText === T.radius,
    console: consoleEntries,
    jobs: results.map((r) => ({ id: r.id, editor: r.editor, preview: r.preview, steps: r.steps })),
  };
  fs.writeFileSync(path.join(__dirname, 'RESULTS.json'), JSON.stringify(report, null, 2) + '\n');
  for (const d of scratch.concat([userData])) { try { fs.rmSync(d, { recursive: true, force: true }); } catch { /* ignore */ } }

  console.log('\n=== WD2-C First Object -- Electron acceptance QA ===');
  for (const c of checks) console.log(`  ${c.ok ? 'PASS' : 'FAIL'}  ${c.name}${c.ok ? '' : '  ' + JSON.stringify(c.detail)}`);
  console.log(`\n${passed}/${checks.length} assertions · launches ${report.launches} · console entries ${consoleEntries.length} (${bad.length} error/warning)`);
  const ok = passed === checks.length;
  console.log(ok ? 'RESULT: PASS' : 'RESULT: FAIL');
  process.exit(ok ? 0 : 1);
}

main().catch((err) => { console.error(err); process.exit(1); });
