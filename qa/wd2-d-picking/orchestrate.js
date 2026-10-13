'use strict';
// WD2-D (#30) official Electron evidence: viewport picking on the REAL editor
// page, REAL X_ITE 15.1.10 and REAL Chromium pointer input.
//
//   xvfb-run -a node qa/wd2-d-picking/orchestrate.js      (Linux)
//   WD2D_ONLY='<regex on phase id>' narrows a debugging run.
//
// TRANSPORT (QA-only, no product change). The capture server in main.js (as on
// origin/main) opens each fixture through the real editorController in ONE
// reused Electron process per launch, driven by VisualQaRunner. This harness
// adds only a launch flag -- `--remote-debugging-port=0` -- and drives the page
// over the Chrome DevTools Protocol (./cdp.js): `Input.dispatchMouseEvent` for
// real pointer input, `Runtime.evaluate` for the page's existing read-only
// __wrlEditor hooks. Sequencing: each job's `before` (run in this harness's
// writeJob, before that job is written) acts on the document the previous job
// opened. Nothing in main.js, preload.js, IPC, CSP or the renderer exists for
// this harness.
//
// BUILD PRECONDITION. renderer/vendor/wrl-editor.bundle.js is gitignored and is
// NOT produced by `npm ci`. Without it window.WRLForgeSceneBridge is undefined
// and editor.js init throws "Cannot read properties of undefined (reading
// 'sceneTree')" (initSceneViews) -- the QA #1 failure; with a STALE bundle the
// bridge lacks viewportPick and every click is a silent no-op. So this harness
// builds the bundle itself and proves on the live page that the bridge carries
// viewportPick before any case runs.
//
// ORACLE. The accepted WD2-C0 oracle (spikes/wd2-c0-xite-picking/fixtures.js,
// read-only) supplies P1-P22: text composed by concatenation, every
// occurrence's exact [start,end) recorded as written, a world-space aim point
// per click and an independent projection. QA-authored fixtures below follow
// the same rule. A PROVEN pick whose spans differ from the oracle is WRONG; the
// run fails unless WRONG = 0.
//
// Electron runs with --user-data-dir under the OS temp dir (never
// ~/.config/wrl-forge); sources are scratch files under the OS temp dir.

const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { spawn, spawnSync } = require('child_process');
const { VisualQaRunner } = require('../visual-qa/runner');
const { acquire } = require('../visual-qa/lock');
const CDP = require('./cdp');
const C0 = require('../../spikes/wd2-c0-xite-picking/fixtures');

const repoRoot = path.join(__dirname, '..', '..');
const OUT = path.join(__dirname, 'screenshots');
const RESULTS = path.join(__dirname, 'RESULTS.json');
const SIZE = '1500x950';
const ONLY = process.env.WD2D_ONLY ? new RegExp(process.env.WD2D_ONLY) : null;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sha256 = (data) => crypto.createHash('sha256').update(data).digest('hex');
const same = (a, b) => !!a && !!b && a.start === b.start && a.end === b.end;

// ---- results ----------------------------------------------------------------------
const checks = [];
const picks = [];
const PRE = {};
let wrong = 0;
const tally = {};
function check(name, ok, detail) {
  checks.push({ name, ok: !!ok, detail: detail === undefined ? null : detail });
  process.stdout.write(`  ${ok ? 'PASS' : 'FAIL'}  ${name}\n`);
  return !!ok;
}

// ---- QA-authored oracle fixtures (same rule as C0: spans by construction) ---------
function composer({ eol = '\n', prefix = '', comment = null } = {}) {
  let text = `${prefix}#VRML V2.0 utf8${eol}`;
  if (comment) text += `# ${comment}${eol}`;
  const spans = {};
  const api = {
    put(s) { text += s.split('\n').join(eol); return api; },
    mark(name, fn) { const start = text.length; fn(); spans[name] = Object.freeze({ start, end: text.length }); return api; },
    done() { return { text, spans }; },
  };
  api.put('Viewpoint { position 0 0 20 description "front" }\n');
  return api;
}
const proven = (clicked, logical) => ({ status: 'PROVEN', clicked, logical: logical || clicked });
const refusal = (c, status) => ({ ...c, expect: { status } });

// C0 P5 geometry (twins at x = -2 / +2), authorable in every mandatory source
// form. `edited`: a span-shifting edit INSIDE the left object.
function twins(opts = {}) {
  const c = composer(opts);
  for (const [name, x] of [['left', -2], ['right', 2]]) {
    c.mark(`${name}.transform`, () => {
      c.put(`Transform { translation ${x} 0 0 children [ `);
      c.mark(`${name}.shape`, () => c.put(name === 'left' && opts.edited ? 'Shape { geometry Box { size 2.5 2.5 2.5 } }' : 'Shape { geometry Box { } }'));
      c.put(' ] }');
    });
    c.put('\n');
  }
  return {
    id: 'twins', ...c.done(),
    clicks: [
      { id: 'left', world: [-2, 0, 1], expect: proven('left.shape', 'left.transform') },
      { id: 'right', world: [2, 0, 1], expect: proven('right.shape', 'right.transform') },
      { id: 'right-2', world: [2.4, -0.4, 1], expect: proven('right.shape', 'right.transform') },
      { id: 'left-2', world: [-1.6, 0.4, 1], expect: proven('left.shape', 'left.transform') },
    ],
  };
}

// Every pointing-device sensor type, each beside its own Box; one plain Box.
function sensorsFixture() {
  const c = composer();
  const types = [['PS', 'PlaneSensor', -4.5], ['CS', 'CylinderSensor', -1.5], ['SS', 'SphereSensor', 1.5], ['TS', 'TouchSensor', 4.5]];
  for (const [def, type, x] of types) {
    c.mark(`${def}.transform`, () => {
      c.put(`Transform { translation ${x} 2 0 children [ DEF ${def} ${type} { } `);
      c.mark(`${def}.shape`, () => c.put('Shape { geometry Box { size 2 2 2 } }'));
      c.put(' ] }');
    });
    c.put('\n');
  }
  c.mark('plain.transform', () => {
    c.put('Transform { translation 0 -3 0 children [ ');
    c.mark('plain.shape', () => c.put('Shape { geometry Box { size 2 2 2 } }'));
    c.put(' ] }');
  });
  c.put('\n');
  return {
    id: 'sensors', ...c.done(),
    clicks: [
      ...types.map(([def, type, x]) => ({ id: type, def, world: [x, 2, 1], expect: { status: 'REFUSED_SENSOR_CONFLICT', clicked: `${def}.shape` } })),
      { id: 'plain', world: [0, -3, 1], expect: proven('plain.shape', 'plain.transform') },
    ],
  };
}

// Geometry the Mall bbox traversal does not measure (src/preview/
// bbox-traversal.js has no ElevationGrid case): "bounds unavailable", so the
// Cybertown Fit render falls back to the document text itself.
function noBoundsFixture() {
  const c = composer();
  c.mark('eg.transform', () => {
    c.put('Transform { translation -2 2 0 rotation 1 0 0 1.5708 children [ ');
    c.mark('eg.shape', () => c.put('Shape { appearance Appearance { material Material { } } geometry ElevationGrid { xDimension 2 zDimension 2 xSpacing 4 zSpacing 4 height [ 0 0 0 0 ] solid FALSE } }'));
    c.put(' ] }');
  });
  c.put('\n');
  return { id: 'no-bounds', ...c.done(), clicks: [{ id: 'grid', world: [0, 0, 0], expect: proven('eg.shape', 'eg.shape') }] };
}

// ---- scratch files ------------------------------------------------------------------
const scratch = [];
function stageDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-wd2dqa-'));
  scratch.push(dir);
  return dir;
}
function stage(name, text, extra = {}) {
  const dir = stageDir();
  const p = path.join(dir, name);
  fs.writeFileSync(p, text, 'utf8');
  for (const [n, t] of Object.entries(extra)) fs.writeFileSync(path.join(dir, n), t, 'utf8');
  return p;
}

// ---- preflight (the QA #1 root cause) --------------------------------------------------
function preflight() {
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
  const b = spawnSync(npm, ['run', 'build:editor'], { cwd: repoRoot, encoding: 'utf8' });
  const bundle = path.join(repoRoot, 'renderer', 'vendor', 'wrl-editor.bundle.js');
  const built = b.status === 0 && fs.existsSync(bundle);
  const text = built ? fs.readFileSync(bundle, 'utf8') : '';
  const json = (rel) => JSON.parse(fs.readFileSync(path.join(repoRoot, rel), 'utf8'));
  const lock = json('package-lock.json');
  return {
    bundleBuilt: built, bundleSha256: built ? sha256(text) : null, bundleHasViewportPick: text.includes('viewportPick'),
    xite: {
      packageJson: json('package.json').dependencies.x_ite,
      lockRoot: lock.packages[''].dependencies.x_ite,
      lockResolved: lock.packages['node_modules/x_ite'].version,
      installed: json('node_modules/x_ite/package.json').version,
    },
    electron: require('electron/package.json').version,
    xiteMin: fs.readFileSync(path.join(repoRoot, 'node_modules', 'x_ite', 'dist', 'x_ite.min.js'), 'utf8'),
  };
}

// ---- one Electron launch: jobs open documents, CDP acts on them -------------------------
async function runLaunch(label, phases, { args = [] } = {}) {
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-wd2dqa-profile-'));
  scratch.push(userData);
  let session = null;
  let chain = Promise.resolve();
  const harnessErrors = [];
  const runAct = async (ph) => {
    if (!ph || !ph.act || !session) return;
    const exBefore = session.exceptions.length;
    const ceBefore = session.consoleErrors.length;
    const t0 = Date.now();
    process.stdout.write(`-- ${label}/${ph.id}\n`);
    try {
      await dismissRecovery(session);
      await ph.act(makeCtx(session, ph));
    } catch (e) {
      check(`${label}/${ph.id}: act completed without a harness error`, false, String((e && e.stack) || e));
    }
    ph.result = {
      ms: Date.now() - t0,
      exceptions: session.exceptions.slice(exBefore),
      consoleErrors: session.consoleErrors.slice(ceBefore).filter((m) => m.level === 'error'),
    };
  };
  const jobs = phases.map((ph, i) => ({
    id: `${label}-${ph.id}`,
    size: ph.size || SIZE,
    editor: ph.editor,
    before: async () => {
      if (i === 0) {
        session = await CDP.attach(userData);
        await session.send('Page.enable');
      } else {
        await runAct(phases[i - 1]);
      }
      if (ph.beforeOpen) await ph.beforeOpen(session);
    },
  }));
  jobs.push({ id: `${label}-final`, keyboard: { kind: 'inspectFocus', delayMs: 10 }, before: () => runAct(phases[phases.length - 1]) });
  const runner = new VisualQaRunner({
    spawn: () => spawn(require('electron'), ['.', '--no-sandbox', `--user-data-dir=${userData}`, '--remote-debugging-port=0',
      '--use-angle=swiftshader', '--enable-unsafe-swiftshader', ...args], {
      cwd: repoRoot,
      env: { ...process.env, WRL_FORGE_CAPTURE_SERVER: '1', WRL_FORGE_NO_EDITOR: '1', WRL_FORGE_SETTLE_MS: '1500' },
      stdio: ['pipe', 'pipe', 'inherit'],
    }),
    maxLaunches: 1,
    retriesPerLaunch: 0, // a retry would replay acts against another state
    readyTimeoutMs: 60000,
    captureTimeoutMs: 900000, // includes this harness's act on the previous job
    log: (rec) => { if (rec.event !== 'capture:done' && rec.event !== 'capture:start') process.stdout.write(`${JSON.stringify({ event: rec.event, id: rec.id || null, error: rec.error || undefined })}\n`); },
    writeJob: (child, job) => {
      chain = chain.then(async () => {
        try { if (job.before) await job.before(); } catch (e) { harnessErrors.push(String((e && e.stack) || e)); }
        child.stdin.write(`${JSON.stringify(job)}\n`);
      });
    },
  });
  let results = null;
  let runError = null;
  try {
    results = await runner.run(jobs);
  } catch (e) {
    runError = String((e && e.stack) || e);
  }
  await chain.catch(() => {});
  if (session) session.close();
  await sleep(500);
  const ps = spawnSync('pgrep', ['-f', `user-data-dir=${userData}`], { encoding: 'utf8' });
  const leftover = ps.status === 0 ? ps.stdout.trim().split('\n').filter(Boolean) : [];
  return {
    label, runError, harnessErrors, survivors: runner.survivors(), leftover, launches: runner.launchesUsed,
    completedJobs: results ? results.length : 0, totalJobs: jobs.length,
    phases: phases.map((p) => ({ id: p.id, ...(p.result || {}) })),
    exceptions: session ? session.exceptions : [], consoleErrors: session ? session.consoleErrors : [],
  };
}

async function dismissRecovery(session) {
  const shown = await session.evaluate(`(() => { const d = document.getElementById('wrlforgeRecoveryRoot');
    return !!(d && (d.offsetWidth > 0 || d.offsetHeight > 0)); })()`);
  if (!shown) return;
  await session.hook('clickFirst', '#wrlforgeRecoveryRoot .actions button:first-child'); // Start fresh
  await sleep(2500);
}

// ---- per-act helpers ----------------------------------------------------------------------
function makeCtx(session, ph) {
  const hook = session.hook;
  const ev = session.evaluate;
  const canvas = JSON.stringify(ph.canvas || 'preview');
  const ctx = { session, hook, ev, ph };

  ctx.settle = async ({ model = true, timeout = 30000 } = {}) => {
    const t0 = Date.now();
    let ps = null;
    while (Date.now() - t0 < timeout) {
      ps = await hook('previewState');
      const armedOk = !model || (ps && ps.picking && ps.picking.armed && ps.picking.renderedArmed);
      if (ps && ps.state === 'current' && armedOk) { await sleep(450); return ps; }
      await sleep(150);
    }
    return ps;
  };
  ctx.mode = async () => (await hook('modelState')).mode;
  ctx.toModel = async () => { if ((await ctx.mode()) !== 'model') await hook('click', 'modeModelBtn'); return ctx.settle(); };
  ctx.toCode = async () => { if ((await ctx.mode()) !== 'code') await hook('click', 'modeCodeBtn'); await sleep(300); };
  ctx.sourceOpen = async (open) => {
    const st = await hook('modelState');
    if (st.sourceOpen !== open) await hook('click', 'sourceToggleBtn');
    await sleep(700);
  };
  ctx.rect = () => ev(`(() => { const c = document.getElementById(${canvas});
    if (!c) return null; const r = c.getBoundingClientRect(); return { left: r.left, top: r.top, width: r.width, height: r.height }; })()`);
  // Public X_ITE SAI only (getNamedNode / field reads / set_bind): QA observation.
  ctx.scene = (expr) => ev(`(() => { const scene = document.getElementById(${canvas}).browser.currentScene; return (${expr}); })()`);
  ctx.field = (def, name) => ctx.scene(`(() => { const v = scene.getNamedNode(${JSON.stringify(def)})[${JSON.stringify(name)}];
    if (v && typeof v === 'object' && typeof v.length === 'number') return Array.from(v);
    if (v && typeof v === 'object' && 'x' in v && 'y' in v && 'z' in v) return [v.x, v.y, v.z]; // SFVec3f
    return v && typeof v === 'object' && typeof v.valueOf === 'function' ? v.valueOf() : v; })()`);
  ctx.bind = async (def) => { await ctx.scene(`(scene.getNamedNode(${JSON.stringify(def)}).set_bind = true, true)`); await sleep(2500); };
  ctx.listeners = async () => {
    const { result } = await session.send('Runtime.evaluate', { expression: 'document.querySelector(".preview-col")' });
    if (!result || !result.objectId) return null;
    const { listeners } = await session.send('DOMDebugger.getEventListeners', { objectId: result.objectId });
    const count = (type) => listeners.filter((l) => l.type === type && l.useCapture).length;
    return { pointerdown: count('pointerdown'), pointerup: count('pointerup'), pointercancel: count('pointercancel') };
  };
  ctx.pristineHook = async (label) => {
    const h = await ev(`(() => { const f = X3D.VRMLParser.prototype.nodeStatement; return { source: String(f), length: f.length }; })()`);
    return check(`${label}: VRMLParser.prototype.nodeStatement is X_ITE's own method (arity 0, source found in x_ite.min.js)`,
      h.length === 0 && PRE.xiteMin.includes(h.source), { length: h.length, head: h.source.slice(0, 48) });
  };
  // QA-only, observable: a NEW adapter on this page uses the page's one
  // module-private coordinator; it can prove its probe and complete a
  // provenance parse promptly only if no owner or waiter is stuck.
  ctx.coordinatorIdle = () => ev(`(async () => {
    const a = window.WrlXitePickAdapter.createXitePickAdapter({ X3D: window.X3D, browser: document.getElementById(${canvas}).browser });
    const t0 = performance.now();
    let compat = a.compatibility();
    while (!compat.ok && compat.reason === 'compatibility-unproven' && performance.now() - t0 < 8000) {
      await new Promise((r) => setTimeout(r, 20)); compat = a.compatibility();
    }
    const r = await Promise.race([a.parseWithProvenance('#VRML V2.0 utf8\\nShape { geometry Box { } }\\n', { generationId: 'qa-idle' }),
      new Promise((res) => setTimeout(() => res('timeout'), 8000))]);
    a.dispose();
    return { compat, acquired: r !== 'timeout', generation: !!(r && r.generation), ms: Math.round(performance.now() - t0) };
  })()`);
  ctx.shot = async (name, expectFn, what) => {
    await session.screenshot(path.join(OUT, name));
    const state = await hook('modelState');
    check(`screenshot ${name} shows ${what}`, expectFn(state), { mode: state.mode, sourceOpen: state.sourceOpen, editorVisible: state.editorVisible, status: state.status, selected: state.selected });
  };
  ctx.canvasPixels = async () => {
    const r = await ctx.rect();
    const shot = await session.send('Page.captureScreenshot', { format: 'png', clip: { x: r.left, y: r.top, width: r.width, height: r.height, scale: 1 } });
    return sha256(shot.data);
  };
  ctx.client = async (c, fx) => {
    const r = await ctx.rect();
    const camera = c.camera ? fx.cameras[c.camera] : C0.DEFAULT_CAMERA;
    const p = C0.project(c.world, camera, r.width, r.height);
    return { x: r.left + p.x, y: r.top + p.y };
  };

  // One real click at a world point; returns what the page resolved.
  ctx.click = async (fx, c, opts = {}) => {
    const { x, y } = await ctx.client(c, fx);
    const before = await hook('lastViewportPick');
    const selBefore = await hook('sceneSelection');
    const caretBefore = await hook('caretHead');
    await session.mouse('move', x, y);
    await session.mouse('down', x, y);
    const during = opts.during ? await opts.during() : null;
    const j = opts.jitter || 0;
    if (j) await session.mouse('move', x + j, y + j);
    await session.mouse('up', x + j, y + j);
    let pick = null;
    for (let i = 0; i < 40; i++) {
      const now = await hook('lastViewportPick');
      if (now && (!before || now.seq > before.seq)) { pick = now; break; }
      if (opts.expectNone && i >= 12) break;
      await sleep(50);
    }
    return { x: Math.round(x), y: Math.round(y), pick, sel: await hook('sceneSelection'), selBefore, caret: await hook('caretHead'), caretBefore, during };
  };

  // Click + grade against the oracle. WRONG iff PROVEN names another occurrence.
  ctx.expect = async (fx, c, tag, opts = {}) => {
    const res = await ctx.click(fx, c, opts);
    const exp = c.expect;
    const spans = opts.spans || fx.spans;
    const p = res.pick;
    const got = p ? p.status : 'NO_PICK';
    let isWrong = false;
    let ok;
    if (got === 'PROVEN') {
      isWrong = exp.status !== 'PROVEN' || !same(p.source.logical, spans[exp.logical]) || !same(p.source.shape, spans[exp.clicked]);
      ok = !isWrong && !!res.sel && res.sel.id === p.sceneTreeItemId && res.caret === res.caretBefore;
    } else {
      const unchanged = (res.sel ? res.sel.id : null) === (res.selBefore ? res.selBefore.id : null);
      ok = got === exp.status && unchanged && res.caret === res.caretBefore && (!opts.reason || (!!p && p.reason === opts.reason));
    }
    if (isWrong) wrong += 1;
    tally[got] = (tally[got] || 0) + 1;
    const row = {
      case: tag, fixture: fx.id, click: c.id, expect: exp.status, expectReason: opts.reason || null, got, reason: p ? p.reason : null,
      seq: p ? p.seq : null, wrong: isWrong, ok, at: { x: res.x, y: res.y }, source: p && p.source ? p.source : null,
      selection: res.sel ? res.sel.id : null, selectionBefore: res.selBefore ? res.selBefore.id : null, caretMoved: res.caret !== res.caretBefore,
    };
    picks.push(row);
    check(`${tag}: ${fx.id}/${c.id} -> ${exp.status}${opts.reason ? ` ${opts.reason}` : ''} (got ${got}${p && p.reason ? ` ${p.reason}` : ''})`, ok && !isWrong, row);
    return { ...res, row };
  };
  return ctx;
}

// ---- the phases -----------------------------------------------------------------------------
const mallJob = (file, extra) => ({ context: 'mall', mallPath: file, previewLayout: 'split', fitMode: 'original', ...extra });

function c0Phases() {
  return C0.build().map((fx) => ({
    id: fx.id,
    editor: mallJob(stage(`${fx.id}.wrl`, fx.text, fx.children || {})),
    act: async (ctx) => {
      const ps = await ctx.toModel();
      check(`${fx.id}: preview current and picking armed + compatible in Model`,
        ps && ps.state === 'current' && ps.picking.armed && ps.picking.compatibility && ps.picking.compatibility.ok, ps && { state: ps.state, picking: ps.picking, chip: ps.chip });
      let bound = null;
      for (const c of fx.clicks) {
        if (c.camera && bound !== c.camera) { await ctx.bind(c.camera); bound = c.camera; }
        if (fx.id === 'P17-touchsensor' && c.id === 'sensed') {
          // The authored TouchSensor still activates under the SAME real press.
          const r = await ctx.expect(fx, c, 'V6', { during: () => ctx.field('TS', 'isActive') });
          check('P17: authored TouchSensor isActive during the press (WD2-D did not steal the event)', r.during === true, r.during);
          check('P17: authored TouchSensor touchTime set on release', Number(await ctx.field('TS', 'touchTime')) > 0);
        } else if (fx.id === 'P18-anchor') {
          const before = await ctx.field('Far', 'isBound');
          await ctx.expect(fx, c, 'V6');
          await sleep(1500);
          const after = await ctx.field('Far', 'isBound');
          check('P18: the Anchor still navigated (#Far bound after the same click)', before === false && after === true, { before, after });
        } else {
          await ctx.expect(fx, c, 'V6');
        }
      }
      if (fx.id === 'P5-anonymous-twins') {
        await ctx.shot('01-proven-selection.png', (s) => s.mode === 'model' && !!s.selected, 'a PROVEN viewport selection in Model');
      }
    },
  }));
}

function sensorPhase() {
  const fx = sensorsFixture();
  return {
    id: 'sensors-all',
    editor: mallJob(stage('sensors.wrl', fx.text)),
    act: async (ctx) => {
      await ctx.toModel();
      for (const c of fx.clicks) {
        if (!c.def) { await ctx.expect(fx, c, 'sensor'); continue; }
        const r = await ctx.expect(fx, c, 'sensor', { during: () => ctx.field(c.def, 'isActive') });
        check(`${c.id}: authored sensor isActive during the same real press`, r.during === true, r.during);
        if (c.def === 'TS') {
          check('TouchSensor: touchTime set on release', Number(await ctx.field('TS', 'touchTime')) > 0);
          await ctx.shot('04-sensor-refusal.png', (s) => s.mode === 'model' && /interactive/.test(String(s.status)), 'the sensor refusal line');
        }
      }
      // A real DRAG on the PlaneSensor: the authored sensor tracks it; WD2-D picks nothing.
      const { x, y } = await ctx.client(fx.clicks[0], fx);
      const before = await ctx.hook('lastViewportPick');
      await ctx.session.mouse('move', x, y);
      await ctx.session.mouse('down', x, y);
      for (let i = 1; i <= 8; i++) { await ctx.session.mouse('move', x + i * 6, y); await sleep(30); }
      await ctx.session.mouse('up', x + 48, y);
      await sleep(400);
      const tr = await ctx.field('PS', 'translation_changed');
      const after = await ctx.hook('lastViewportPick');
      check('PlaneSensor drag: authored translation_changed moved', Array.isArray(tr) && Math.abs(tr[0]) > 0.1, tr);
      check('PlaneSensor drag: no WD2-D pick (a drag is never a pick)', after.seq === before.seq, { before: before.seq, after: after.seq });
    },
  };
}

function formsPhases() {
  const forms = {
    LF: {},
    CRLF: { eol: '\r\n' },
    BOM: { prefix: '﻿' },
    Unicode: { comment: 'Ünïcödé ✓ 日本語 😀 — surrogate pairs shift UTF-16 offsets' },
  };
  return Object.entries(forms).map(([name, opts]) => {
    const fx = twins(opts);
    return {
      id: `form-${name}`,
      editor: mallJob(stage(`twins-${name}.wrl`, fx.text)),
      act: async (ctx) => {
        await ctx.toModel();
        // WD2-D compares exact strings only. Grade against the string the
        // editor buffer actually holds (CodeMirror's own, pre-existing handling).
        let spans = null;
        let bufferForm = null;
        if (await ctx.hook('bufferEquals', fx.text)) { spans = fx.spans; bufferForm = `${name} exactly as written`; }
        const lf = twins({ comment: opts.comment || null });
        if (!spans && await ctx.hook('bufferEquals', lf.text)) {
          spans = lf.spans;
          bufferForm = name === 'CRLF' ? 'LF (CodeMirror line-break normalization)' : 'without the BOM';
        }
        check(`V7 ${name}: the editor buffer is a known exact string (${bufferForm})`, !!spans, bufferForm);
        PRE[`form${name}`] = bufferForm;
        const ms = await ctx.hook('modelState');
        if (ms.sourceOpen) await ctx.sourceOpen(false);
        const ps = await ctx.hook('previewState');
        for (const c of fx.clicks.slice(0, 2)) {
          const res = await ctx.click(fx, c);
          const p = res.pick;
          const got = p ? p.status : 'NO_PICK';
          const isWrong = got === 'PROVEN' && (!spans || !same(p.source.logical, spans[c.expect.logical]) || !same(p.source.shape, spans[c.expect.clicked]));
          if (isWrong) wrong += 1;
          tally[got] = (tally[got] || 0) + 1;
          // PROVEN with exact spans, or fail-closed (no generation -> a refusal).
          const ok = !isWrong && (got === 'PROVEN' || (name === 'BOM' && /^(REFUSED_STALE|UNSUPPORTED)$/.test(got)));
          picks.push({ case: 'V7', fixture: `twins-${name}`, click: c.id, expect: 'PROVEN|fail-closed', got, reason: p && p.reason, wrong: isWrong, ok, source: p && p.source, bufferForm });
          check(`V7 ${name}: ${c.id} -> ${got}${p && p.reason ? ` ${p.reason}` : ''} (exact spans or fail-closed)`, ok, { got, reason: p && p.reason, preview: ps.state, chip: ps.chip, bufferForm });
        }
      },
    };
  });
}

function stalePhase() {
  const fx = twins();
  const edited = twins({ edited: true });
  const broken = `${fx.text}Transform {\n`;
  return {
    id: 'stale',
    editor: mallJob(stage('stale.wrl', fx.text)),
    act: async (ctx) => {
      const [left, right] = fx.clicks;
      await ctx.toModel();
      await ctx.expect(fx, left, 'V4 fresh');
      // an edit before the debounced refresh: generation text !== buffer
      await ctx.hook('setText', edited.text);
      await ctx.expect(fx, refusal(right, 'REFUSED_STALE'), 'V4 after-edit, before refresh', { reason: 'source-changed-since-preview' });
      await ctx.settle();
      await ctx.expect(edited, edited.clicks[1], 'V4 after-edit, refreshed (shifted spans)');
      await ctx.expect(edited, edited.clicks[0], 'V4 after-edit, refreshed (grown object)');
      // a reload while the pointer is down: the older press resolves against a retired generation
      await ctx.expect(edited, refusal(edited.clicks[1], 'REFUSED_STALE'), 'V4 reload during the press',
        { reason: 'hit-from-another-preview-generation', during: async () => { await ctx.hook('previewUpdate'); await ctx.settle(); return true; } });
      // a syntax error: X_ITE keeps the last valid scene on screen
      await ctx.hook('setText', broken);
      await sleep(2800);
      const ps = await ctx.hook('previewState');
      check('V4 last-valid: the preview reports it shows the last valid scene', ps.haveLastValid && ps.state !== 'current', { state: ps.state, chip: ps.chip });
      await ctx.sourceOpen(false); // a damaged document auto-opens Source (WD2-C)
      await ctx.expect(edited, refusal(edited.clicks[0], 'REFUSED_STALE'), 'V4 last-valid', { reason: 'preview-shows-last-valid-scene' });
      await ctx.shot('02-stale-last-valid.png', (s) => s.mode === 'model' && /out of date/.test(String(s.status)), 'the stale refusal over the last valid scene');
      await ctx.hook('setText', fx.text);
      await ctx.settle();
      await ctx.expect(fx, left, 'V4 fixed');
      // Show saved: the disk file, not the buffer
      await ctx.hook('setText', edited.text);
      await ctx.settle();
      await ctx.hook('previewSaved');
      await sleep(2500);
      await ctx.expect(fx, refusal(right, 'REFUSED_STALE'), 'V4 show saved', { reason: 'preview-scene-replaced' });
      await ctx.hook('setText', fx.text);
      await ctx.settle();
      // Cybertown Fit is not the document
      await ctx.hook('fitMode', 'fit');
      await sleep(2500);
      await ctx.expect(fx, refusal(left, 'UNSUPPORTED'), 'V4 Cybertown Fit', { reason: 'preview-is-not-the-document' });
      await ctx.hook('fitMode', 'original');
      await ctx.settle();
      await ctx.expect(fx, right, 'V4 Original again');
    },
  };
}

function fitNoBoundsPhase() {
  const fx = noBoundsFixture();
  return {
    id: 'fit-no-bounds',
    editor: mallJob(stage('no-bounds.wrl', fx.text)),
    act: async (ctx) => {
      await ctx.toModel();
      const pill = await ctx.ev("(document.getElementById('confidencePill') || {}).textContent || null");
      check('Fit no-bounds: the Mall report says bounds are unavailable', /unavailable/.test(String(pill)), pill);
      const [c] = fx.clicks;
      await ctx.expect(fx, c, 'Fit no-bounds, Original');
      await ctx.hook('fitMode', 'fit');
      await sleep(2500);
      const r = await ctx.expect(fx, refusal(c, 'UNSUPPORTED'), 'Fit no-bounds, Fit (renders the document; refused)', { reason: 'preview-is-not-the-document' });
      PRE.fitNoBounds = { pill, fit: r.row };
      await ctx.hook('fitMode', 'original');
      await ctx.settle();
      await ctx.expect(fx, c, 'Fit no-bounds, Original again');
    },
  };
}

function dragPhase() {
  const fx = twins();
  return {
    id: 'click-drag',
    editor: mallJob(stage('drag.wrl', fx.text)),
    act: async (ctx) => {
      const [left, right] = fx.clicks;
      await ctx.toModel();
      await ctx.expect(fx, right, 'V5 click');
      await ctx.expect(fx, left, 'V5 click with 2px jitter (within slop)', { jitter: 2 });
      await ctx.hook('sceneSelectFirst', 'Transform', 1); // select RIGHT through the tree
      const sel0 = await ctx.hook('sceneSelection');
      const px0 = await ctx.canvasPixels();
      const before = await ctx.hook('lastViewportPick');
      const { x, y } = await ctx.client(left, fx);
      await ctx.session.mouse('move', x, y);
      await ctx.session.mouse('down', x, y);
      for (let i = 1; i <= 10; i++) { await ctx.session.mouse('move', x + i * 8, y + i * 3); await sleep(25); }
      await ctx.session.mouse('up', x + 80, y + 30);
      await sleep(900);
      const after = await ctx.hook('lastViewportPick');
      const sel1 = await ctx.hook('sceneSelection');
      const px1 = await ctx.canvasPixels();
      check('V5 drag: no pick resolved (sequence unchanged)', after.seq === before.seq, { before: before.seq, after: after.seq });
      check('V5 drag: selection unchanged', sel0 && sel1 && sel0.id === sel1.id, { sel0, sel1 });
      check('V5 drag: X_ITE navigation still happened (canvas pixels changed)', px0 !== px1);
    },
  };
}

function codeInertPhase() {
  const fx = twins();
  return {
    id: 'model-code-caret',
    editor: mallJob(stage('code-inert.wrl', fx.text)),
    act: async (ctx) => {
      const [left, right] = fx.clicks;
      await ctx.toModel();
      const lm = await ctx.listeners();
      check('V15 Model: exactly one capture listener per pointer type on the preview', lm && lm.pointerdown === 1 && lm.pointerup === 1 && lm.pointercancel === 1, lm);
      const caret0 = await ctx.hook('caretHead');
      await ctx.expect(fx, left, 'V15 Model pick');
      check('V15/O1: a PROVEN viewport selection does not move the caret', (await ctx.hook('caretHead')) === caret0);
      await ctx.toCode();
      const lc = await ctx.listeners();
      const st = await ctx.hook('pickState');
      check('V11 Code: no pointer listener attached', lc && lc.pointerdown === 0 && lc.pointerup === 0 && lc.pointercancel === 0, lc);
      check('V11 Code: picking not armed', st.picking && st.picking.armed === false && st.mode === 'code', st);
      const res = await ctx.click(fx, right, { expectNone: true });
      check('V11 Code: a click on the preview resolves nothing and changes no selection',
        !res.pick && (res.sel ? res.sel.id : null) === (res.selBefore ? res.selBefore.id : null), { pick: res.pick, sel: res.sel, selBefore: res.selBefore });
      await ctx.shot('03-code-workspace-inert.png', (s) => s.mode === 'code' && s.editorVisible === true, 'the Code workspace (source editor visible)');
      await ctx.toModel();
      const lm2 = await ctx.listeners();
      check('V11 back in Model: listeners re-attached exactly once', lm2 && lm2.pointerdown === 1 && lm2.pointerup === 1 && lm2.pointercancel === 1, lm2);
      await ctx.expect(fx, right, 'V15 Model again');
    },
  };
}

function touchFalsePhase() {
  const fx = twins();
  return {
    id: 'touch-false',
    editor: mallJob(stage('touch-false.wrl', fx.text)),
    act: async (ctx) => {
      const [left, right] = fx.clicks;
      await ctx.toModel();
      await ctx.expect(fx, left, 'V13 prior valid pick'); // X_ITE's shared hit now holds LEFT
      const px0 = await ctx.canvasPixels();
      // Leave X_ITE's Examine viewer ACTIVE with real input: press the left
      // button on LEFT, release a DIFFERENT button (X_ITE's mouseup ignores it).
      const pl = await ctx.client(left, fx);
      await ctx.session.mouse('move', pl.x, pl.y);
      await ctx.session.mouse('down', pl.x, pl.y);
      await ctx.session.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: pl.x, y: pl.y, button: 'middle', buttons: 0, clickCount: 1 });
      await sleep(250);
      const viewerActive = await ctx.ev("document.getElementById('preview').browser.getViewer().isActive()");
      check('V13 setup: the X_ITE viewer is active (QA private read)', viewerActive === true, viewerActive);
      await ctx.hook('sceneSelectFirst', 'Transform', 1); // select RIGHT through the tree
      const res = await ctx.expect(fx, refusal(right, 'NO_HIT'), 'V13 touch() false', { reason: 'viewer-active-or-no-hit' });
      check('V13: the retained LEFT hit was not selected', res.sel && res.selBefore && res.sel.id === res.selBefore.id, { sel: res.sel, before: res.selBefore });
      await ctx.session.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: pl.x, y: pl.y, button: 'left', buttons: 0, clickCount: 1 });
      await sleep(300);
      check('V13 teardown: the viewer is released', (await ctx.ev("document.getElementById('preview').browser.getViewer().isActive()")) === false);
      // The pointer moved while X_ITE's viewer was active, so X_ITE really
      // rotated the camera: the oracle projection no longer holds. Prove it,
      // then reload (a Mall reload rebinds the authored Viewpoint).
      check('V13 teardown: the active viewer moved the camera (projection invalid until reset)', (await ctx.canvasPixels()) !== px0);
      await ctx.hook('previewUpdate');
      await ctx.settle();
      check('V13 teardown: the reload restored the authored camera', (await ctx.canvasPixels()) === px0);
      await ctx.expect(fx, right, 'V13 normal picking resumes');
    },
  };
}

function abortPhase() {
  const grid = { ...C0.gridFixture(1000, 125), id: 'grid1000' };
  // The edit inserts a comment right after the header, BEFORE every span, so
  // the edited oracle is every span shifted by exactly its length.
  const INSERT = '# edited\n';
  const editedGrid = grid.text.replace('#VRML V2.0 utf8\n', `#VRML V2.0 utf8\n${INSERT}`);
  const edited = { ...grid, id: 'grid1000-edited', text: editedGrid,
    spans: Object.fromEntries(Object.entries(grid.spans).map(([k, s]) => [k, { start: s.start + INSERT.length, end: s.end + INSERT.length }])) };
  return {
    id: 'abort-mid-parse',
    editor: mallJob(stage('grid1000.wrl', grid.text)),
    act: async (ctx) => {
      await ctx.toModel();
      await ctx.pristineHook('V14 before');
      const r = await ctx.ev(`(async () => {
        const P = X3D.VRMLParser.prototype; const orig = P.nodeStatement;
        window.__wrlEditor.setText(${JSON.stringify(editedGrid)});
        window.__wrlEditor.previewUpdate(); // this size tier renders on request, not on the debounce
        const t0 = performance.now(); let seen = false;
        while (performance.now() - t0 < 10000) { if (P.nodeStatement !== orig) { seen = true; break; } await new Promise((res) => setTimeout(res, 0)); }
        if (!seen) return { seen };
        document.getElementById('modeCodeBtn').click(); // Model -> Code: abort() + retire(), synchronously
        return { seen, restoredSynchronously: P.nodeStatement === orig, waitedMs: Math.round(performance.now() - t0) };
      })()`);
      check('V14: a provenance wrapper was observed during the real asynchronous X_ITE parse', r.seen === true, r);
      check('V14: abort() restored X_ITE\'s method synchronously', r.restoredSynchronously === true, r);
      const ps = await ctx.settle({ model: false });
      await ctx.pristineHook('V14 after the late completion');
      check('V14: the late completion is harmless (preview current, not reported as a failure)', ps.state === 'current', { state: ps.state, chip: ps.chip, failure: ps.failureCategory });
      PRE.abortLateCompletion = { state: ps.state, chip: ps.chip };
      await ctx.toModel();
      check('V14: the buffer is the edited text', await ctx.hook('bufferEquals', editedGrid));
      for (const c of edited.clicks.slice(0, 4)) await ctx.expect(edited, c, 'V14 the next parse proceeds normally');
    },
  };
}

function overlapPhase() {
  const fx = twins();
  return {
    id: 'two-browsers-overlap',
    editor: mallJob(stage('overlap.wrl', fx.text)),
    act: async (ctx) => {
      await ctx.toModel();
      const r = await ctx.ev(`(async () => {
        const b1 = document.getElementById('preview').browser; const b2 = document.getElementById('wpCanvas').browser;
        if (!b1 || !b2) return { error: 'two browsers unavailable', b1: !!b1, b2: !!b2 };
        const P = X3D.VRMLParser.prototype; const orig = P.nodeStatement;
        const A = window.WrlXitePickAdapter;
        const a1 = A.createXitePickAdapter({ X3D, browser: b1 }); const a2 = A.createXitePickAdapter({ X3D, browser: b2 });
        const settle = async (a) => { let c = a.compatibility(); for (let i = 0; i < 400 && !c.ok && c.reason === 'compatibility-unproven'; i++) { await new Promise((r) => setTimeout(r, 20)); c = a.compatibility(); } return c; };
        const c1 = await settle(a1); const c2 = await settle(a2);
        const log = [];
        const spy = (b, tag) => { const had = Object.prototype.hasOwnProperty.call(b, 'createX3DFromString'); const f = b.createX3DFromString;
          b.createX3DFromString = async function (t) { log.push(tag + ':start'); try { return await f.call(this, t); } finally { log.push(tag + ':end'); } };
          return () => { if (had) b.createX3DFromString = f; else delete b.createX3DFromString; }; };
        const u1 = spy(b1, 'A'); const u2 = spy(b2, 'B');
        const wrappers = new Set(); let sampling = true;
        (async () => { while (sampling) { const f = P.nodeStatement; if (f !== orig) wrappers.add(f); await new Promise((r) => setTimeout(r, 0)); } })();
        const text = '#VRML V2.0 utf8\\n' + Array.from({ length: 300 }, (_, i) => 'Transform { translation ' + i + ' 0 0 children [ Shape { geometry Box { } } ] }').join('\\n') + '\\n';
        const [r1, r2] = await Promise.all([a1.parseWithProvenance(text, { generationId: 'qa-A' }), a2.parseWithProvenance(text, { generationId: 'qa-B' })]);
        sampling = false; u1(); u2();
        const out = { c1, c2, log, distinctWrappersObserved: wrappers.size, gen1: !!r1.generation, gen2: !!r2.generation, restored: P.nodeStatement === orig };
        a1.dispose(); a2.dispose();
        return out;
      })()`);
      check('V3: two real X_ITE browsers on one parser prototype; both adapters compatible', r.c1 && r.c1.ok && r.c2 && r.c2.ok, r);
      check('V3: overlapping requests serialize (A starts and ends before B starts)', Array.isArray(r.log) && r.log.join(',') === 'A:start,A:end,B:start,B:end', r.log);
      check('V3: each request minted its own generation; the hook is restored', r.gen1 && r.gen2 && r.restored, r);
      await ctx.pristineHook('V3 after');
      await ctx.expect(fx, fx.clicks[0], 'V3 the page\'s own adapter still picks');
    },
  };
}

async function endStateChecks(ctx, fx, label) {
  await ctx.pristineHook(label);
  const idle = await ctx.coordinatorIdle();
  check(`${label}: coordinator has no owner and no waiter (a new adapter acquires and completes promptly)`, idle.compat.ok && idle.acquired && idle.generation, idle);
  await ctx.pristineHook(`${label} (after the idle probe)`);
  const l = await ctx.listeners();
  check(`${label}: exactly one capture listener per pointer type (no duplicates)`, l && l.pointerdown === 1 && l.pointerup === 1 && l.pointercancel === 1, l);
  const ps = await ctx.hook('previewState');
  const res = await ctx.expect(fx, fx.clicks[1], `${label}: pick`);
  check(`${label}: the only active generation is the displayed one`, !!res.pick && res.pick.generation === ps.displayedGeneration,
    { pickGeneration: res.pick && res.pick.generation, displayed: ps.displayedGeneration });
}

function retentionPhase() {
  const fx = twins();
  const variant = twins({ edited: true });
  return {
    id: 'retention',
    editor: mallJob(stage('retention.wrl', fx.text)),
    act: async (ctx) => {
      await ctx.toModel();
      const out = {};
      // 100 rapid Model <-> Code round trips (200 transitions), chip sampled each step.
      out.rapid = await ctx.ev(`(async () => {
        const chips = {}; const chip = () => (document.getElementById('previewChip') || {}).textContent || '';
        const note = () => { const c = chip(); chips[c] = (chips[c] || 0) + 1; };
        for (let i = 0; i < 100; i++) {
          document.getElementById('modeCodeBtn').click(); note();
          document.getElementById('modeModelBtn').click(); note();
          await new Promise((r) => setTimeout(r, 0));
        }
        return chips;
      })()`);
      const rapidSettled = await ctx.settle();
      out.rapidSettled = rapidSettled && { state: rapidSettled.state, chip: rapidSettled.chip, failure: rapidSettled.failureCategory };
      // normal cadence: 20 settled round trips, chip read after each.
      out.normal = {};
      for (let i = 0; i < 20; i++) {
        await ctx.toCode();
        const ps = await ctx.toModel();
        out.normal[ps.chip] = (out.normal[ps.chip] || 0) + 1;
      }
      // preview reloads (edits), then Fit <-> Original
      for (let i = 0; i < 10; i++) { await ctx.hook('setText', i % 2 ? fx.text : variant.text); await ctx.settle(); }
      await ctx.hook('setText', fx.text);
      await ctx.settle();
      for (let i = 0; i < 10; i++) { await ctx.hook('fitMode', i % 2 ? 'original' : 'fit'); await sleep(900); }
      await ctx.hook('fitMode', 'original');
      await ctx.settle();
      // abort: an edit, its debounce fires, Model is left immediately -- x5
      out.abortChips = {};
      for (let i = 0; i < 5; i++) {
        await ctx.hook('setText', i % 2 ? fx.text : variant.text);
        await sleep(760);
        await ctx.toCode();
        const ps = await ctx.settle({ model: false });
        out.abortChips[ps.chip] = (out.abortChips[ps.chip] || 0) + 1;
        await ctx.toModel();
      }
      await ctx.hook('setText', fx.text);
      await ctx.settle();
      PRE.retention = out;
      check('retention: 200 rapid transitions settle to a current preview with picking armed', out.rapidSettled && out.rapidSettled.state === 'current', out.rapidSettled);
      check('retention: normal-cadence transitions never show "last good version"', !Object.keys(out.normal).some((k) => /last good/i.test(k)), out.normal);
      check('retention: aborted renders never show "last good version"', !Object.keys(out.abortChips).some((k) => /last good/i.test(k)), out.abortChips);
      await endStateChecks(ctx, fx, 'retention end');
    },
  };
}

function disposalPhase() {
  const fx = twins({ comment: 'second document (session switch)' });
  return {
    id: 'session-disposal',
    editor: mallJob(stage('second.wrl', fx.text)),
    act: async (ctx) => {
      await ctx.toModel();
      const leak = await ctx.hook('previewLeak');
      check('disposal: main holds only the new session\'s overlay after the switch', leak && leak.size === 1 && leak.activeGenerations <= 1, leak);
      await endStateChecks(ctx, fx, 'after a session switch');
    },
  };
}

function worldPhases() {
  const p16 = C0.build().find((f) => f.id === 'P16-inline');
  const dir = stageDir();
  const primary = path.join(dir, 'primary.wrl');
  fs.writeFileSync(primary, p16.text, 'utf8');
  for (const [n, t] of Object.entries(p16.children)) fs.writeFileSync(path.join(dir, n), t, 'utf8');
  const child = path.join(dir, 'P16-child.wrl');
  return [
    {
      id: 'world-inline', canvas: 'wpCanvas',
      editor: { context: 'world', root: dir, primary },
      act: async (ctx) => {
        await ctx.toModel();
        for (const c of p16.clicks) await ctx.expect(p16, c, 'World Inline');
        await ctx.shot('06-world-inline-refusal.png', (s) => s.mode === 'model' && /Inline/.test(String(s.status)), 'the Inline refusal line');
      },
    },
    {
      id: 'world-nested-edit', canvas: 'wpCanvas',
      editor: { context: 'world', root: dir, primary, ref: child },
      act: async (ctx) => {
        await ctx.toModel();
        const ps = await ctx.hook('previewState');
        check('nested World edit: a World session editing the Inline child', ps && ps.context === 'world', ps && { context: ps.context, state: ps.state, chip: ps.chip });
        for (const c of p16.clicks) {
          await ctx.expect(p16, refusal(c, 'REFUSED_EXTERNAL'), 'nested World', { reason: 'preview-root-is-another-document' });
        }
      },
    },
  ];
}

// V10: the same exact objects under every layout. One document, many rows.
function coordinatePhase(id, size, rows) {
  const fx = twins();
  return {
    id, size,
    editor: mallJob(stage(`${id}.wrl`, fx.text)),
    act: async (ctx) => {
      await ctx.toModel();
      PRE.coordinates = PRE.coordinates || [];
      for (const row of rows) {
        await row.apply(ctx);
        await sleep(1200);
        const rect = await ctx.rect();
        const dpr = await ctx.ev('window.devicePixelRatio');
        const label = `V10 ${id} ${row.name} (DPR ${dpr}, canvas ${Math.round(rect.width)}x${Math.round(rect.height)})`;
        const n0 = picks.length;
        for (const c of fx.clicks) await ctx.expect(fx, c, label);
        const rowPicks = picks.slice(n0);
        PRE.coordinates.push({ launch: id, row: row.name, dpr, canvas: { w: Math.round(rect.width), h: Math.round(rect.height) },
          proven: rowPicks.filter((p) => p.got === 'PROVEN' && p.ok).length, of: rowPicks.length, wrong: rowPicks.filter((p) => p.wrong).length });
        if (row.undo) { await row.undo(ctx); await sleep(800); }
      }
      await ctx.hook('setZoom', 0);
    },
  };
}
const ROW = {
  base: { name: 'base', apply: async () => {} },
  zoom: (n) => ({ name: `ui-zoom ${n}`, apply: (ctx) => ctx.hook('setZoom', n), undo: (ctx) => ctx.hook('setZoom', 0) }),
  source: { name: 'Source open', apply: (ctx) => ctx.sourceOpen(true), undo: (ctx) => ctx.sourceOpen(false) },
  sourceZoom: { name: 'Source open + ui-zoom 4', apply: async (ctx) => { await ctx.sourceOpen(true); await ctx.hook('setZoom', 4); },
    undo: async (ctx) => { await ctx.hook('setZoom', 0); await ctx.sourceOpen(false); } },
  splitUp: { name: 'Source open, split +2 steps', apply: async (ctx) => { await ctx.sourceOpen(true); await ctx.hook('previewStepSplit', 1); await ctx.hook('previewStepSplit', 1); },
    undo: async (ctx) => { await ctx.hook('previewStepSplit', -1); await ctx.hook('previewStepSplit', -1); await ctx.sourceOpen(false); } },
  splitDown: { name: 'Source open, split -2 steps', apply: async (ctx) => { await ctx.sourceOpen(true); await ctx.hook('previewStepSplit', -1); await ctx.hook('previewStepSplit', -1); },
    undo: async (ctx) => { await ctx.hook('previewStepSplit', 1); await ctx.hook('previewStepSplit', 1); await ctx.sourceOpen(false); } },
  maximize: { name: 'preview maximized', apply: (ctx) => ctx.hook('previewMaximize'), undo: (ctx) => ctx.hook('previewMaximize') },
};

function displacedPhase() {
  const fx = twins();
  return {
    id: 'displaced-wrapper',
    editor: mallJob(stage('displaced.wrl', fx.text)),
    act: async (ctx) => {
      await ctx.toModel();
      const r = await ctx.ev(`(async () => {
        const b = document.getElementById('preview').browser; const P = X3D.VRMLParser.prototype; const orig = P.nodeStatement;
        const a = window.WrlXitePickAdapter.createXitePickAdapter({ X3D, browser: b });
        for (let i = 0; i < 400 && a.compatibility().reason === 'compatibility-unproven'; i++) await new Promise((r) => setTimeout(r, 20));
        const text = '#VRML V2.0 utf8\\n' + Array.from({ length: 400 }, () => 'Shape { geometry Box { } }').join('\\n') + '\\n';
        const p = a.parseWithProvenance(text, { generationId: 'qa-displace' });
        let installed = false;
        for (let i = 0; i < 4000 && !installed; i++) { if (P.nodeStatement !== orig) installed = true; else await new Promise((r) => setTimeout(r, 0)); }
        if (!installed) return { installed };
        const theirs = P.nodeStatement;
        const foreign = function qaForeignWrapper() { return theirs.apply(this, arguments); };
        P.nodeStatement = foreign;
        const res = await p;
        return { installed, rendered: !!res.scene, generation: !!res.generation, foreignSurvives: P.nodeStatement === foreign,
          qaAdapter: a.compatibility(), pageAdapter: window.wrlPreview.pickTarget().adapter.compatibility() };
      })()`);
      check('V12: wrapper displaced mid-parse; the replacement is left in place', r.installed && r.foreignSurvives, r);
      check('V12: no generation published for the displaced parse; it still rendered', r.rendered && !r.generation, r);
      check('V12: picking disabled page-wide with parser-hook-displaced', r.qaAdapter && r.qaAdapter.reason === 'parser-hook-displaced' && r.pageAdapter.reason === 'parser-hook-displaced', r);
      await ctx.expect(fx, refusal(fx.clicks[0], 'COMPATIBILITY_DISABLED'), 'V12 click', { reason: 'parser-hook-displaced' });
      const st = await ctx.hook('modelState');
      check('V12: the persistent status line names the reason', /Preview picking unavailable .*parser-hook-displaced/.test(String(st.status)), st.status);
      const id = await ctx.hook('sceneSelectFirst', 'Transform', 1);
      check('V12: Scene Tree selection still works', !!id && (await ctx.hook('sceneSelection')).id === id);
      await ctx.hook('setText', twins({ edited: true }).text);
      const ps = await ctx.settle({ model: false });
      check('V12: the live preview still renders edits', ps.state === 'current', { state: ps.state });
      const logged = ctx.session.consoleErrors.filter((m) => /\[WD2-D\] viewport picking disabled: parser-hook-displaced \(displaced-wrapper\)/.test(m.message));
      check('V12: the reason was logged once with its row', logged.length === 1, logged);
    },
  };
}

function forcedIncompatPhase() {
  const fx = twins();
  let scriptId = null;
  return {
    id: 'forced-incompatibility',
    editor: mallJob(stage('incompat.wrl', fx.text)),
    // Registered BEFORE this job's page loads, through X_ITE's own load-time
    // extension hook: P2 (nodeStatement arity 0) is changed, behaviour kept.
    beforeOpen: async (session) => {
      const r = await session.send('Page.addScriptToEvaluateOnNewDocument', { source: `(() => {
        const k = Symbol.for('X_ITE.extensions'); const list = window[k] || (window[k] = []);
        list.push((X3D) => { const P = X3D.VRMLParser && X3D.VRMLParser.prototype; if (!P) return;
          const o = P.nodeStatement; P.nodeStatement = function qaArityOne(unused) { return o.apply(this, arguments); }; });
      })();` });
      scriptId = r.identifier;
    },
    act: async (ctx) => {
      try {
        await ctx.toModel();
        const st = await ctx.hook('modelState');
        check('V8: persistent compatibility line in Model (P2 changed by the harness)', /Preview picking unavailable .*parser-hook-missing/.test(String(st.status)) && st.statusError, st.status);
        await ctx.shot('05-compatibility-disabled.png', (s) => s.mode === 'model' && /Preview picking unavailable/.test(String(s.status)), 'the persistent compatibility line');
        await ctx.expect(fx, refusal(fx.clicks[0], 'COMPATIBILITY_DISABLED'), 'V8 click', { reason: 'parser-hook-missing' });
        const id = await ctx.hook('sceneSelectFirst', 'Transform', 0);
        check('V8: Scene Tree selection works', !!id && (await ctx.hook('sceneSelection')).id === id);
        const fields = await ctx.hook('inspectorFields');
        check('V8: the Inspector shows the selected node', fields && fields.fields.some((f) => f.name === 'translation'), fields && fields.fields.map((f) => f.name));
        await ctx.hook('inspectorSet', 'translation', 1, '1.5');
        await ctx.hook('inspectorClick', 'translation', 'apply');
        await sleep(400);
        check('V8: Inspector Apply edits the source', !(await ctx.hook('bufferEquals', fx.text)));
        const ps = await ctx.settle({ model: false });
        check('V8: rendering still works', ps.state === 'current', { state: ps.state });
        const st2 = await ctx.hook('modelState');
        check('V8: the compatibility line persists across a selection change', /Preview picking unavailable/.test(String(st2.status)), st2.status);
        const logged = ctx.session.consoleErrors.filter((m) => /\[WD2-D\] viewport picking disabled: parser-hook-missing \(P2\)/.test(m.message));
        check('V8: the reason was logged once with its P-row', logged.length === 1, logged);
      } finally {
        await ctx.session.send('Page.removeScriptToEvaluateOnNewDocument', { identifier: scriptId });
      }
    },
  };
}

// The first phase of every launch: prove this page is the right build.
function gatePhase() {
  return {
    id: 'page-gate',
    editor: mallJob(stage('gate.wrl', twins().text)),
    act: async (ctx) => {
      const g = await ctx.ev(`({ bridge: !!window.WRLForgeSceneBridge, viewportPick: !!(window.WRLForgeSceneBridge && window.WRLForgeSceneBridge.viewportPick),
        adapter: window.WrlXitePickAdapter ? Object.keys(window.WrlXitePickAdapter) : null,
        version: document.getElementById('preview').browser.version })`);
      check('page: WRLForgeSceneBridge.viewportPick present (the fresh bundle is loaded)', g.bridge && g.viewportPick, g);
      check('page: the adapter global exposes only createXitePickAdapter', JSON.stringify(g.adapter) === '["createXitePickAdapter"]', g.adapter);
      check('V1: runtime browser.version === 15.1.10', g.version === '15.1.10', g.version);
      const ps = await ctx.toModel();
      check('G4: the real-X_ITE probe passed (picking compatible in Model)', ps && ps.picking.compatibility && ps.picking.compatibility.ok === true, ps && ps.picking);
      await ctx.pristineHook('V2 after the probe and the first provenance parse');
    },
  };
}

// ---- main -------------------------------------------------------------------------------------
async function main() {
  if (process.platform === 'win32') {
    console.error('wd2-d-picking: CDP sequencing needs the stdin capture transport (Linux).');
    process.exit(2);
  }
  if (!process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
    console.error('wd2-d-picking: no DISPLAY -- run under xvfb-run.');
    process.exit(2);
  }
  fs.mkdirSync(OUT, { recursive: true });
  for (const f of fs.readdirSync(OUT)) if (f.endsWith('.png')) fs.unlinkSync(path.join(OUT, f)); // never keep stale evidence
  Object.assign(PRE, preflight());
  check('preflight: editor bundle built from this tree', PRE.bundleBuilt, PRE.bundleSha256);
  check('preflight: the bundle carries the viewportPick bridge', PRE.bundleHasViewportPick);
  check('G1/G2: x_ite exactly 15.1.10 (package.json, lockfile root + resolved, installed)', Object.values(PRE.xite).every((v) => v === '15.1.10'), PRE.xite);
  if (!PRE.bundleBuilt || !PRE.bundleHasViewportPick) { finish({ aborted: 'HARNESS_PRECONDITION: editor bundle missing or stale' }); return; }

  const want = (ph) => !ONLY || ONLY.test(ph.id) || ph.id === 'page-gate';
  const release = acquire();
  const t0 = Date.now();
  const launches = [];
  try {
    const dpr1 = [
      gatePhase(),
      ...c0Phases(),
      sensorPhase(),
      ...formsPhases(),
      stalePhase(),
      fitNoBoundsPhase(),
      dragPhase(),
      codeInertPhase(),
      touchFalsePhase(),
      abortPhase(),
      overlapPhase(),
      retentionPhase(),
      disposalPhase(),
      ...worldPhases(),
      coordinatePhase('coords-1500x950', SIZE, [ROW.base, ROW.zoom(-3), ROW.zoom(4), ROW.zoom(8), ROW.source, ROW.sourceZoom, ROW.splitUp, ROW.splitDown, ROW.maximize]),
      coordinatePhase('coords-1100x760', '1100x760', [ROW.base, ROW.source, ROW.zoom(4)]),
      coordinatePhase('coords-1900x1100', '1900x1100', [ROW.base, ROW.zoom(4), ROW.source]),
      displacedPhase(),
      forcedIncompatPhase(),
    ].filter(want);
    launches.push(await runLaunch('dpr1', dpr1));
    const dpr2 = [gatePhase(), coordinatePhase('coords-dpr2-1500x950', SIZE, [ROW.base, ROW.zoom(4), ROW.source, ROW.splitUp, ROW.maximize])].filter(want);
    if (dpr2.length > 1) launches.push(await runLaunch('dpr2', dpr2, { args: ['--force-device-scale-factor=2'] }));
  } finally {
    release();
  }
  finish({ launches, wallClockMs: Date.now() - t0 });
}

function finish(extra) {
  const launches = extra.launches || [];
  const exceptions = launches.flatMap((l) => l.exceptions.map((e) => ({ launch: l.label, ...e })));
  const consoleErrors = launches.flatMap((l) => l.consoleErrors.filter((m) => m.level === 'error').map((m) => ({ launch: l.label, ...m })));
  const consoleWarnings = launches.flatMap((l) => l.consoleErrors.filter((m) => m.level !== 'error').map((m) => ({ launch: l.label, ...m })));
  if (!extra.aborted) {
    check('renderer exceptions = 0 (CDP Runtime.exceptionThrown; full stacks recorded)', exceptions.length === 0, exceptions);
    check('renderer console errors = 0', consoleErrors.length === 0, consoleErrors);
    check('every launch completed every job without a harness error',
      launches.every((l) => !l.runError && l.completedJobs === l.totalJobs && !l.harnessErrors.length),
      launches.map((l) => ({ label: l.label, runError: l.runError, harnessErrors: l.harnessErrors, completed: `${l.completedJobs}/${l.totalJobs}` })));
    check('Electron processes left running = 0', launches.every((l) => l.survivors.length === 0 && l.leftover.length === 0), launches.map((l) => ({ survivors: l.survivors, leftover: l.leftover })));
    check('WRONG_SOURCE_SELECTIONS = 0', wrong === 0, wrong);
  }
  const failed = checks.filter((c) => !c.ok);
  const { xiteMin, ...pre } = PRE;
  void xiteMin;
  const results = {
    lane: 'WD2-D-I #30 repair pass -- implementation-side Electron evidence',
    generatedAt: new Date().toISOString(),
    aborted: extra.aborted || null,
    transport: 'VisualQaRunner capture server (main.js unchanged) + Chrome DevTools Protocol Input.dispatchMouseEvent via --remote-debugging-port=0',
    preflight: pre,
    wallClockMs: extra.wallClockMs || null,
    wrong, statusTally: tally,
    assertions: { total: checks.length, passed: checks.length - failed.length, failed: failed.length },
    failedChecks: failed.map((c) => c.name),
    picks, checks,
    launches: launches.map((l) => ({ label: l.label, runError: l.runError, harnessErrors: l.harnessErrors, survivors: l.survivors, leftover: l.leftover, completed: `${l.completedJobs}/${l.totalJobs}`, phases: l.phases })),
    exceptions, consoleErrors, consoleWarnings,
  };
  fs.writeFileSync(RESULTS, `${JSON.stringify(results, null, 2)}\n`);
  for (const d of scratch) { try { fs.rmSync(d, { recursive: true, force: true }); } catch { /* best effort */ } }
  process.stdout.write(`\n${checks.length - failed.length}/${checks.length} assertions · WRONG=${wrong} · picks ${picks.length} · tally ${JSON.stringify(tally)}\n`);
  process.stdout.write(`RESULT: ${failed.length === 0 && !extra.aborted ? 'PASS' : 'FAIL'}\n`);
  process.exitCode = failed.length === 0 && !extra.aborted ? 0 : 1;
}

main().catch((e) => { console.error(e); process.exit(1); });
