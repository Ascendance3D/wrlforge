'use strict';
// WD2-B (typed Inspector field editing) real-Electron runtime QA.
//
// Drives the REAL editor page -- CodeMirror bundle, scene tree, Inspector, the
// Phase 7C2 unsaved-buffer X_ITE preview and the existing safe-save path --
// through ONE reused capture-server process via VisualQaRunner (no per-capture
// launches, zero-survivor teardown). Every interaction goes through the page's
// own window.__wrlEditor hooks: select a scene item, type into an Inspector
// field control, press Enter / Escape, click Apply / Cancel / Undo / Redo /
// Save. Nothing is injected.
//
//   node qa/wd2-b-typed-inspector/orchestrate.js
//
// The headline gate (WD2-B section 20): Transform translation 0 0 0 -> X = 3
// from the Inspector; exact source; same node still selected; Inspector reads
// 3/0/0; the X_ITE preview's authoritative world-space bounds move by +3 on X
// from the UNSAVED buffer; dirty; Undo restores the exact source and bounds;
// Redo restores the edit; Save persists it to disk. Plus: invalid input, Escape,
// multi-component Apply = one Undo, twin siblings, a visibly lost selection,
// diagnostics, High Contrast + zoom, and zero console errors/warnings.
//
// Sources are SCRATCH files under the OS temp dir (the capture server refuses
// anything else). PNGs + RESULTS.json land beside this file.

const fs = require('fs');
const os = require('os');
const path = require('path');
const zlib = require('zlib');
const { spawn } = require('child_process');
const { VisualQaRunner } = require('../visual-qa/runner');
const { acquire } = require('../visual-qa/lock');
const { makeCaptureTransport } = require('../visual-qa/transport');

const repoRoot = path.join(__dirname, '..', '..');
const OUT = path.join(__dirname, 'screenshots');
const SIZE = '1400x900';
const SETTLE = 2600; // > the 700 ms preview debounce + an X_ITE render

const H = '#VRML V2.0 utf8\n';
const PRIMARY = H
  + 'Transform {\n'
  + '  translation 0 0 0\n'
  + '  children [\n'
  + '    Shape {\n'
  + '      geometry Box { size 2 2 2 }\n'
  + '    }\n'
  + '  ]\n'
  + '}\n';
const EDITED = PRIMARY.replace('translation 0 0 0', 'translation 3 0 0');
const MULTI = PRIMARY.replace('translation 0 0 0', 'translation 3 1 -2');

const TWINS = H
  + 'Group { children [\n'
  + '  Transform { translation 0 0 0 children [ Shape { geometry Box { size 1 1 1 } } ] }\n'
  + '  Transform { translation 0 0 0 children [ Shape { geometry Box { size 1 1 1 } } ] }\n'
  + '] }\n';
const TWINS_EDITED = TWINS.replace(/(Transform \{ translation 0 0 0[^\n]*\n)(  Transform \{ translation )0 0 0/, '$1$2-4.5 0 0');
const REPLACED = H + 'Transform { translation 0 0 0 children [ Shape { geometry Sphere { } } ] }\n';
const BROKEN = H + 'Transform { translation 0 0 0 children [ Shape {\n';

function stage(name, text) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-wd2bqa-'));
  const p = path.join(dir, name);
  fs.writeFileSync(p, text, 'utf8');
  return p;
}

function realSpawn(extraEnv = {}) {
  return spawn(require('electron'), ['.', '--no-sandbox'], {
    cwd: repoRoot,
    env: { ...process.env, WRL_FORGE_CAPTURE_SERVER: '1', WRL_FORGE_NO_EDITOR: '1', WRL_FORGE_SETTLE_MS: '1500', ...extraEnv },
    stdio: ['pipe', 'pipe', 'inherit'],
  });
}

const png = (name) => path.join(OUT, name + '.png');
const call = (name, args, label, wait) => ({ call: name, args: args || [], label: label || null, wait: wait || 0 });

function primarySteps() {
  return [
    call('sceneSelectFirst', ['Transform', 0], 'select', 300),
    call('sceneSelection', [], 'sel-before'),
    call('inspectorFields', [], 'fields-before'),
    call('previewBBox', [], 'bbox-before'),
    call('status', [], 'status-before'),
    { capture: png('01-before-apply') },
    call('inspectorSet', ['translation', 0, '3'], 'type-x'),
    call('inspectorKey', ['translation', 0, 'Enter'], 'enter', SETTLE),
    call('bufferEquals', [EDITED], 'buf-edited'),
    call('sceneSelection', [], 'sel-after'),
    call('inspectorFields', [], 'fields-after'),
    call('activeInfo', [], 'focus-after'),
    call('status', [], 'status-after'),
    call('previewState', [], 'preview-after'),
    call('previewBBox', [], 'bbox-after'),
    { capture: png('02-after-apply-x3') },
    call('click', ['undoBtn'], 'undo', SETTLE),
    call('bufferEquals', [PRIMARY], 'buf-undo'),
    call('sceneSelection', [], 'sel-undo'),
    call('inspectorFields', [], 'fields-undo'),
    call('previewBBox', [], 'bbox-undo'),
    call('status', [], 'status-undo'),
    { capture: png('03-after-undo') },
    call('click', ['redoBtn'], 'redo', SETTLE),
    call('bufferEquals', [EDITED], 'buf-redo'),
    call('sceneSelection', [], 'sel-redo'),
    call('inspectorFields', [], 'fields-redo'),
    call('previewBBox', [], 'bbox-redo'),
    { capture: png('04-after-redo') },
    // Invalid input: refused before the document; focus + text kept; Escape restores.
    call('inspectorSet', ['translation', 1, 'abc'], 'type-invalid'),
    call('inspectorKey', ['translation', 1, 'Enter'], 'enter-invalid', 300),
    call('bufferEquals', [EDITED], 'buf-invalid'),
    call('inspectorFields', [], 'fields-invalid'),
    call('activeInfo', [], 'focus-invalid'),
    { capture: png('05-invalid-input') },
    call('inspectorKey', ['translation', 1, 'Escape'], 'escape', 200),
    call('inspectorFields', [], 'fields-escape'),
    call('activeInfo', [], 'focus-escape'),
    // Multi-component Apply via the Apply button = ONE undo step.
    call('inspectorSet', ['translation', 1, '1'], 'type-y'),
    call('inspectorSet', ['translation', 2, '-2'], 'type-z'),
    call('inspectorClick', ['translation', 'apply'], 'apply-multi', SETTLE),
    call('bufferEquals', [MULTI], 'buf-multi'),
    call('sceneSelection', [], 'sel-multi'),
    call('click', ['undoBtn'], 'undo-multi', SETTLE),
    call('bufferEquals', [EDITED], 'buf-multi-undo'),
    call('previewBBox', [], 'bbox-multi-undo'),
    // Save the X = 3 edit through the existing safe-save path.
    call('click', ['saveBtn'], 'save', 1800),
    call('status', [], 'status-saved'),
    call('sceneSelection', [], 'sel-saved'),
    { capture: png('06-saved') },
  ];
}

function twinSteps() {
  return [
    call('sceneSelectFirst', ['Transform', 1], 'select-second', 300),
    call('sceneSelection', [], 'sel-before'),
    call('inspectorSet', ['translation', 0, '-4.5'], 'type'),
    call('inspectorKey', ['translation', 0, 'Enter'], 'enter', SETTLE),
    call('bufferEquals', [TWINS_EDITED], 'buf-edited'),
    call('sceneSelection', [], 'sel-after'),
    call('inspectorFields', [], 'fields-after'),
    { capture: png('07-twin-second-edited') },
    // Replace the whole document: the selection cannot be proven -> lost, visibly.
    call('setText', [REPLACED], 'replace', 1200),
    call('sceneSelection', [], 'sel-lost'),
    call('inspectorFields', [], 'fields-lost'),
    { capture: png('08-selection-lost-notice') },
    // Diagnostics keep working; a damaged document makes fields read-only.
    call('setText', [BROKEN], 'break', 1200),
    call('status', [], 'status-broken'),
    call('sceneSelectFirst', ['Transform', 0], 'select-broken', 300),
    call('inspectorFields', [], 'fields-broken'),
    { capture: png('09-damaged-read-only') },
  ];
}

function contrastSteps() {
  return [
    call('sceneSelectFirst', ['Transform', 0], 'select', 400),
    call('inspectorFields', [], 'fields'),
    call('inspectorKey', ['translation', 0, 'Tab'], 'focus-x', 200),
    call('activeInfo', [], 'focus'),
    { capture: png('10-contrast-zoom-inspector') },
  ];
}

function findStep(job, label) {
  const s = job && job.steps ? job.steps.find((x) => x.label === label) : null;
  return s ? s.result : undefined;
}

function near(a, b) { return Math.abs(a - b) < 1e-6; }

async function main() {
  if (process.platform !== 'win32' && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
    console.error('wd2-b-typed-inspector: no DISPLAY/WAYLAND_DISPLAY -- refusing to launch Electron headless-blind.');
    process.exit(2);
  }
  const transport = makeCaptureTransport();
  fs.mkdirSync(OUT, { recursive: true });
  const primary = stage('primary.wrl', PRIMARY);
  const twins = stage('twins.wrl', TWINS);
  const contrast = stage('contrast.wrl', PRIMARY);
  const scratch = [primary, twins, contrast].map((p) => path.dirname(p));
  const mall = (p, extra) => ({ context: 'mall', mallPath: p, previewLayout: 'split', fitMode: 'original', captureConsole: true, ...extra });

  const jobs = [
    { id: 'primary', editor: mall(primary, { steps: primarySteps() }), size: SIZE },
    { id: 'twins', editor: mall(twins, { steps: twinSteps() }), size: SIZE },
    { id: 'contrast-zoom', editor: mall(contrast, { theme: 'contrast', zoom: 4, steps: contrastSteps() }), size: SIZE },
  ];

  const log = [];
  const runner = new VisualQaRunner({
    spawn: () => realSpawn(transport.env),
    maxLaunches: 2,
    retriesPerLaunch: 1,
    captureTimeoutMs: 120000,
    log: (rec) => { log.push(rec); process.stdout.write(JSON.stringify(rec) + '\n'); },
    ...transport.runnerOpts,
  });

  const release = acquire();
  let results = [];
  let runError = null;
  try {
    results = await runner.run(jobs);
  } catch (err) {
    runError = String((err && err.message) || err);
  } finally {
    release();
    transport.cleanup();
  }

  // The saved bytes, read back from the scratch file the editor saved.
  let savedText = null;
  try {
    const raw = fs.readFileSync(primary);
    savedText = (raw[0] === 0x1f && raw[1] === 0x8b ? zlib.gunzipSync(raw) : raw).toString('utf8');
  } catch (err) { savedText = null; }

  const P = results.find((r) => r.id === 'primary');
  const T = results.find((r) => r.id === 'twins');
  const C = results.find((r) => r.id === 'contrast-zoom');
  const g = (job, label) => findStep(job, label);
  const tr = (job, label) => {
    const f = g(job, label);
    return f && f.fields ? f.fields.find((x) => x.name === 'translation') : null;
  };
  const bx = (job, label) => g(job, label) || { min: [NaN], max: [NaN] };
  const checks = [];
  const check = (name, ok, detail) => checks.push({ name, ok: !!ok, detail: detail === undefined ? null : detail });

  // --- primary acceptance ---
  const selBefore = g(P, 'sel-before');
  check('1 fixture opened in the native editor', P && P.editor && P.editor.file === 'primary.wrl', P && P.editor);
  check('2 Transform selected in the scene tree (row in sync)', selBefore && selBefore.nodeType === 'Transform' && selBefore.rowId === selBefore.id, selBefore);
  const fb = tr(P, 'fields-before');
  check('3 Inspector shows translation as editable SFVec3f', fb && fb.type === 'SFVec3f' && fb.state === 'editable' && fb.values.join() === '0,0,0', fb);
  check('3a accessible names carry field, component and type', fb && fb.names[0] === 'translation X SFVec3f', fb && fb.names);
  check('5/6 Apply (Enter) -> CodeMirror source is exactly "translation 3 0 0", all other bytes unchanged', g(P, 'buf-edited') === true);
  const sa = g(P, 'sel-after');
  check('7 the same Transform remains selected (tree row in sync)', sa && sa.nodeType === 'Transform' && sa.ordinal === 0 && sa.rowId === sa.id, sa);
  const fa = tr(P, 'fields-after');
  check('8 Inspector reads 3 / 0 / 0', fa && fa.values.join() === '3,0,0', fa);
  check('8a "Applied." announced; focus back in translation X', fa && fa.message === 'Applied.' && (g(P, 'focus-after') || {}).field === 'translation' && (g(P, 'focus-after') || {}).component === 0, g(P, 'focus-after'));
  const b0 = bx(P, 'bbox-before');
  const b1 = bx(P, 'bbox-after');
  check('9 X_ITE unsaved preview moved the Box +3 on X (authoritative world bounds)',
    near(b0.min[0], -1) && near(b0.max[0], 1) && near(b1.min[0], 2) && near(b1.max[0], 4) && near(b1.min[1], b0.min[1]), { before: b0, after: b1 });
  check('10 editor is dirty', (g(P, 'status-after') || {}).dirty === true && (g(P, 'status-before') || {}).dirty === false);
  check('11 Undo restores the exact original source', g(P, 'buf-undo') === true);
  check('11a Undo keeps the Transform selected; Inspector 0/0/0', (g(P, 'sel-undo') || {}).nodeType === 'Transform' && (tr(P, 'fields-undo') || {}).values.join() === '0,0,0');
  const bu = bx(P, 'bbox-undo');
  check('12 preview returns to the original position after Undo', near(bu.min[0], -1) && near(bu.max[0], 1), bu);
  check('12a Undo of the only edit clears dirty', (g(P, 'status-undo') || {}).dirty === false);
  check('13 Redo restores the edited source', g(P, 'buf-redo') === true && (tr(P, 'fields-redo') || {}).values.join() === '3,0,0');
  const br = bx(P, 'bbox-redo');
  check('13a preview moves again after Redo', near(br.min[0], 2) && near(br.max[0], 4), br);
  const fi = tr(P, 'fields-invalid');
  check('invalid input refused: source unchanged', g(P, 'buf-invalid') === true);
  check('invalid input: aria-invalid on Y only, message text, typed text kept, focus on Y',
    fi && fi.invalid.join() === 'false,true,false' && fi.message.startsWith('Invalid: ') && fi.values[1] === 'abc'
      && (g(P, 'focus-invalid') || {}).component === 1, { fi, focus: g(P, 'focus-invalid') });
  const fe = tr(P, 'fields-escape');
  check('Escape restores the document value and keeps focus', fe && fe.values.join() === '3,0,0' && fe.invalid.every((x) => !x)
    && (g(P, 'focus-escape') || {}).component === 1, fe);
  check('multi-component Apply (Apply button) patches Y and Z together', g(P, 'buf-multi') === true && (g(P, 'sel-multi') || {}).nodeType === 'Transform');
  check('one Undo reverses the whole multi-component Apply', g(P, 'buf-multi-undo') === true);
  check('14 Save persists "translation 3 0 0" through the existing safe-save path', savedText === EDITED && (g(P, 'status-saved') || {}).dirty === false,
    { savedMatches: savedText === EDITED, status: g(P, 'status-saved') });
  check('diagnostics functional on the valid document (0 syntax diagnostics)', (g(P, 'status-after') || {}).diag === '0');

  // --- twins / lost selection / damaged document ---
  const ts = g(T, 'sel-after');
  check('twin: editing the SECOND identical sibling keeps the SECOND selected', (g(T, 'sel-before') || {}).ordinal === 1 && ts && ts.ordinal === 1 && ts.rowId === ts.id && g(T, 'buf-edited') === true, ts);
  check('twin: Inspector reads -4.5 / 0 / 0', (tr(T, 'fields-after') || {}).values.join() === '-4.5,0,0');
  const fl = g(T, 'fields-lost');
  check('a whole-document replacement LOSES the selection visibly (no wrong anchor)', g(T, 'sel-lost') === null && fl && typeof fl.notice === 'string' && fl.notice.includes('Selection cleared'), fl);
  check('diagnostics functional: the damaged document reports syntax diagnostics', Number((g(T, 'status-broken') || {}).diag) > 0, g(T, 'status-broken'));
  const fbk = g(T, 'fields-broken');
  check('a damaged document makes every field read-only', fbk && fbk.fields.length > 0 && fbk.fields.every((f) => f.state === 'read-only'), fbk);

  // --- accessibility visual ---
  const fc = tr(C, 'fields');
  check('High Contrast + zoom: Inspector controls render', fc && fc.state === 'editable' && fc.values.length === 3);

  // --- console + process hygiene ---
  const consoleEntries = results.flatMap((r) => (r.console || []).map((c) => ({ job: r.id, ...c })));
  const bad = consoleEntries.filter((c) => c.level === 'error' || c.level === 'warning');
  check('no unexpected console error or warning', bad.length === 0, bad);
  const survivors = runner.survivors();
  check('zero surviving Electron processes', survivors.length === 0, survivors);
  check('all jobs completed', !runError && results.length === jobs.length, runError);

  const passed = checks.filter((c) => c.ok).length;
  const report = {
    launches: runner.launchesUsed,
    pids: log.filter((l) => l.event === 'launch').map((l) => l.pid),
    runError,
    survivors,
    assertions: { total: checks.length, passed, failed: checks.length - passed },
    checks,
    console: consoleEntries,
    jobs: results.map((r) => ({ id: r.id, editor: r.editor, preview: r.preview, steps: r.steps })),
  };
  fs.writeFileSync(path.join(__dirname, 'RESULTS.json'), JSON.stringify(report, null, 2) + '\n');
  for (const d of scratch) { try { fs.rmSync(d, { recursive: true, force: true }); } catch { /* ignore */ } }

  console.log('\n=== WD2-B typed Inspector editing -- Electron runtime QA ===');
  for (const c of checks) console.log(`  ${c.ok ? 'PASS' : 'FAIL'}  ${c.name}${c.ok ? '' : '  ' + JSON.stringify(c.detail)}`);
  console.log(`\n${passed}/${checks.length} assertions · launches ${report.launches} · console entries ${consoleEntries.length} (${bad.length} error/warning)`);
  const ok = passed === checks.length;
  console.log(ok ? 'RESULT: PASS' : 'RESULT: FAIL');
  process.exit(ok ? 0 : 1);
}

main().catch((err) => { console.error(err); process.exit(1); });
