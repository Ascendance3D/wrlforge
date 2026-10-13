#!/usr/bin/env node
'use strict';
// WD2-C0 X_ITE picking spike -- deterministic runner.
//
//   node spikes/wd2-c0-xite-picking/probe.js            # run Electron + grade
//   node spikes/wd2-c0-xite-picking/probe.js --grade-only
//
// Launches the spike Electron driver (isolated --user-data-dir under the OS
// temp dir, removed afterwards; never ~/.config/wrl-forge) under xvfb-run with
// SwiftShader WebGL, twice: the main matrix at devicePixelRatio 1 and a
// coordinate-only pass at --force-device-scale-factor=2. Then grades every
// click against the independent oracle (fixtures.js) and writes
// out/PICKING_MATRIX.json. Output holds fixture-relative ids only.
const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawnSync } = require('child_process');
const fixtures = require('./fixtures');
const { grade, CATEGORIES } = require('./grade');
const mapping = require('./mapping');
const vrml = require('../../src/vrml');

const SPIKE = __dirname;
const REPO = path.join(SPIKE, '..', '..');
const OUT = path.join(SPIKE, 'out');
const ELECTRON = path.join(REPO, 'node_modules', '.bin', process.platform === 'win32' ? 'electron.cmd' : 'electron');

// ---- source-edit job: an ordinary exact-source span edit (WD1.2 algebra) ---
function buildEditJob() {
  const fx = fixtures.build().find((f) => f.id === 'P5-anonymous-twins');
  const parse = vrml.parse(fx.text);
  const left = parse.tree.statements.find((s) => s.type === 'Node' && s.nodeType === 'Transform');
  const tr = left.fields.find((f) => f.name === 'translation');
  const after = vrml.edit.applyEdits(fx.text, [vrml.edit.replaceSpan({ from: tr.value.range.start.offset, to: tr.value.range.end.offset }, '-4.25 0 0')]);
  const delta = after.length - fx.text.length;
  // Oracle truth after the edit, by arithmetic on the authored spans: the
  // edit lies inside left.transform, before left.shape and everything right.
  const s = fx.spans;
  const shifted = (sp) => ({ start: sp.start + delta, end: sp.end + delta });
  const truthAfter = {
    'left.transform': { start: s['left.transform'].start, end: s['left.transform'].end + delta },
    'left.shape': shifted(s['left.shape']),
    'right.transform': shifted(s['right.transform']),
    'right.shape': shifted(s['right.shape']),
  };
  return {
    before: fx.text, after, delta, truthBefore: s, truthAfter,
    clicks: [
      { id: 'left', before: [-2, 0, 1], after: [-4.25, 0, 1], logical: 'left.transform', shape: 'left.shape' },
      { id: 'right', before: [2, 0, 1], after: [2, 0, 1], logical: 'right.transform', shape: 'right.shape' },
    ],
  };
}

function runElectron(variant, extraArgs, env) {
  const ud = fs.mkdtempSync(path.join(os.tmpdir(), 'wd2c0-userdata-'));
  const args = ['-a', '-s', '-screen 0 1280x1024x24', ELECTRON, path.join(SPIKE, 'electron', 'main.js'),
    `--user-data-dir=${ud}`, '--use-angle=swiftshader', '--enable-unsafe-swiftshader', ...extraArgs];
  const r = spawnSync('xvfb-run', args, { env: { ...process.env, ...env, WD2C0_VARIANT: variant }, encoding: 'utf8', timeout: 600000 });
  fs.rmSync(ud, { recursive: true, force: true });
  const log = `${r.stdout || ''}${r.stderr || ''}`.split('\n').filter((l) => /\[wd2c0\]|\[page\].*(Error|Parser)/.test(l));
  if (r.status !== 0) throw new Error(`electron variant ${variant} exited ${r.status}\n${log.join('\n')}`);
  return log;
}

const timed = (fn) => { const t = process.hrtime.bigint(); const v = fn(); return [v, Number(process.hrtime.bigint() - t) / 1e6]; };
const summarizeHit = (h) => h && ({
  runtimeShape: h.shape, shapeType: h.shapeType, geometry: h.geometry, geometryType: h.geometryType,
  ctxKind: h.ctxKind, ctxWorld: h.ctxWorld, layer: h.layer, sensors: h.sensors,
  shapeOccurrences: h.shape && h.graph ? h.graph[h.shape].occurrences : [],
  shapeRuntimeParents: h.shape && h.graph ? h.graph[h.shape].parents : [],
  modelViewTranslation: h.modelViewMatrix ? h.modelViewMatrix.slice(12, 15) : null,
  point: h.point, normal: h.normal, touch: h.touch, client: h.client, touchMs: h.touchMs,
});

function main() {
  fs.mkdirSync(OUT, { recursive: true });
  const editJob = buildEditJob();
  const editPath = path.join(OUT, 'edit-job.json');
  fs.writeFileSync(editPath, JSON.stringify(editJob, null, 1));
  if (!process.argv.includes('--grade-only')) {
    runElectron('main', [], { WD2C0_EDIT: editPath });
    runElectron('dpr2', ['--force-device-scale-factor=2'], {});
  }
  const raw = JSON.parse(fs.readFileSync(path.join(OUT, 'raw-main.json'), 'utf8'));
  const rawDpr = JSON.parse(fs.readFileSync(path.join(OUT, 'raw-dpr2.json'), 'utf8'));
  const F = new Map(fixtures.build().map((f) => [f.id, f]));
  const totals = Object.fromEntries(CATEGORIES.map((c) => [c, 0]));
  const controls = { NO_HIT_expected: 0, NO_HIT_correct: 0 };
  const rows = [];
  const mapMs = [];
  const misses = [];

  const gradeRun = (fx, gen, clicks, group) => {
    const [ctx, buildMs] = timed(() => mapping.createSourceContext(fx.text, gen));
    for (const { click, hit } of clicks) {
      const truth = fx.clicks.find((c) => c.id === click);
      const [res, ms] = timed(() => mapping.resolvePick(ctx, hit));
      mapMs.push(ms);
      const g = grade(truth.expect, fx.spans, res);
      if (truth.expect.status === 'NO_HIT') {
        controls.NO_HIT_expected++;
        if (res.status === 'NO_HIT') controls.NO_HIT_correct++;
        else if (g.wrong) totals.WRONG++;
      } else {
        totals[g.category]++;
        if (res.status === 'NO_HIT') misses.push(`${fx.id}/${click}`);
      }
      rows.push({
        group, fixture: fx.id, click, camera: truth.camera || 'front',
        truth: g.truth,
        runtime: summarizeHit(hit),
        mapping: { status: res.status, reason: res.reason, resolved: res.source || null, sceneTreeItemId: res.sceneTreeItemId || null },
        category: truth.expect.status === 'NO_HIT' ? `CONTROL_${res.status}` : g.category,
        wrong: g.wrong, wrongWhy: g.wrongWhy, matchesExpectation: g.matchesExpectation,
      });
    }
    return buildMs;
  };

  for (const rec of raw.fixtures) gradeRun(F.get(rec.id), rec.load.generation, rec.clicks, 'matrix');
  const perf = raw.perf.map((p) => {
    const fx = fixtures.gridFixture(p.n, p.n === 1 ? 1 : p.n === 100 ? 5 : 25);
    const buildMs = gradeRun(fx, p.loadHookOn.generation, p.clicks, 'perf');
    const touch = p.clicks.map((c) => c.hit.touchMs).sort((a, b) => a - b);
    return {
      fixture: fx.id, objects: p.n, clicks: p.clicks.length,
      loadHookOffMs: p.loadHookOff.ms, loadHookOnMs: p.loadHookOn.ms, provenanceRecords: p.loadHookOn.kept,
      touchMs: { median: touch[Math.floor(touch.length / 2)], max: touch[touch.length - 1] },
      sourceContextBuildMs: Math.round(buildMs * 1000) / 1000,
    };
  });
  mapMs.sort((a, b) => a - b);

  // ---- coordinate passes: pointer event vs. direct touch must agree, and
  // both must hit the oracle's object.
  const p6 = F.get('P6-many-siblings');
  const coordinate = [...raw.coordinate, ...rawDpr.coordinate].map((L) => {
    const ctxTouchGen = L.rows[0] && L.rows[0].viaTouch.generation;
    const ctx = mapping.createSourceContext(p6.text, ctxTouchGen);
    return {
      layout: L.name, pageZoom: L.zoom, canvas: L.info,
      rows: L.rows.map((r) => {
        const truth = p6.clicks.find((c) => c.id === r.click);
        const a = grade(truth.expect, p6.spans, mapping.resolvePick(ctx, r.viaTouch));
        const b = r.viaPointer ? grade(truth.expect, p6.spans, mapping.resolvePick(ctx, r.viaPointer)) : null;
        if (a.wrong || (b && b.wrong)) totals.WRONG++;
        return {
          click: r.click,
          touchCategory: a.category, pointerCategory: b ? b.category : 'NO_POINTER_EVENT',
          sameRuntimeShape: !!r.viaPointer && r.viaPointer.shape === r.viaTouch.shape,
          ourConversion: r.viaPointer && r.viaPointer.ourConversion,
          xiteGetPointerFromEvent: r.viaPointer && r.viaPointer.xiteGetPointerFromEvent,
          conversionsAgree: !!r.viaPointer && !!r.viaPointer.xiteGetPointerFromEvent
            && Math.abs(r.viaPointer.ourConversion.x - r.viaPointer.xiteGetPointerFromEvent.x) < 1e-6
            && Math.abs(r.viaPointer.ourConversion.y - r.viaPointer.xiteGetPointerFromEvent.y) < 1e-6,
        };
      }),
    };
  });

  // ---- reload: identities do not persist; stale hits are refused ----------
  const p5 = F.get('P5-anonymous-twins');
  const rl = raw.reload;
  const ctx2 = mapping.createSourceContext(p5.text, rl.g2.generation);
  const reload = {
    generations: [rl.g1.generation, rl.g2.generation],
    gen1Shapes: rl.picks1.map((p) => p.shape), gen2Shapes: rl.picks2.map((p) => p.shape),
    anyRuntimeIdentityPersisted: rl.picks1.some((p, i) => p.shape === rl.picks2[i].shape),
    gen1ObjectsInGen2Map: rl.retained.map((r) => r.inCurrentMap),
    staleGen1HitsAgainstGen2: rl.picks1.map((h) => mapping.resolvePick(ctx2, h).status),
    gen2HitsAgainstGen2: rl.picks2.map((h, i) => grade(p5.clicks[i].expect, p5.spans, mapping.resolvePick(ctx2, h)).category),
  };

  // ---- source edit -----------------------------------------------------------
  const ed = raw.edit;
  const ctxB = mapping.createSourceContext(editJob.before, ed.g1.generation);
  const ctxA = mapping.createSourceContext(editJob.after, ed.g2.generation);
  const ex = (spans, c) => ({ status: 'PROVEN', clicked: c.shape, logical: c.logical });
  const edit = {
    delta: editJob.delta,
    beforeEdit: ed.picks1.map((h, i) => grade(ex(editJob.truthBefore, editJob.clicks[i]), editJob.truthBefore, mapping.resolvePick(ctxB, h)).category),
    afterEdit: ed.picks2.map((h, i) => grade(ex(editJob.truthAfter, editJob.clicks[i]), editJob.truthAfter, mapping.resolvePick(ctxA, h)).category),
    staleBeforeHitsAgainstAfter: ed.picks1.map((h) => mapping.resolvePick(ctxA, h).status),
    // What an UNGUARDED stale map would do: old offsets joined into new text.
    unguardedStaleJoin: ed.picks1.map((h) => {
      const occ = h.graph[h.shape].occurrences[0];
      const n = ctxA.byExactSpan.get(`${occ.start}:${occ.end}`);
      return { oldSpan: occ, joinsInNewText: n ? n.length : 0, newTextAtOldSpan: editJob.after.slice(occ.start, occ.end) };
    }),
  };
  for (const c of [...edit.beforeEdit, ...edit.afterEdit]) if (c === 'WRONG') totals.WRONG++;

  const matrix = {
    spike: 'WD2-C0 X_ITE viewport picking -> exact-source identity',
    xite: { version: raw.env.init.xiteVersion, electron: raw.env.electron, chrome: raw.env.chrome },
    privateApi: {
      touch: raw.env.init.publicBrowserHasTouch, getHit: raw.env.init.publicBrowserHasGetHit, VRMLParser: raw.env.init.hasVRMLParser,
    },
    totals,
    controls,
    aimedClicksThatMissedGeometry: misses,
    mappingLookupMs: { median: mapMs[Math.floor(mapMs.length / 2)], p95: mapMs[Math.floor(mapMs.length * 0.95)], max: mapMs[mapMs.length - 1] },
    sensors: raw.sensors.map((s) => ({ fixture: s.fixture, eventsAfterTouchOnly: s.eventsAfterTouchOnly.map((e) => e.field), eventsAfterRealClick: s.eventsAfterRealClick.map((e) => e.field) })),
    anchor: raw.anchor && { boundBefore: raw.anchor.before, boundAfterTouchOnly: raw.anchor.afterTouchOnly, boundAfterRealClick: raw.anchor.afterRealClick },
    reload, edit, coordinate, perf,
    rows,
  };
  fs.writeFileSync(path.join(OUT, 'PICKING_MATRIX.json'), JSON.stringify(matrix, null, 1));
  for (const f of fixtures.build()) fs.writeFileSync(path.join(OUT, `${f.id}.wrl`), f.text);
  console.log(JSON.stringify({ totals, controls, misses, reload: { persisted: reload.anyRuntimeIdentityPersisted, stale: reload.staleGen1HitsAgainstGen2 }, edit: { before: edit.beforeEdit, after: edit.afterEdit, stale: edit.staleBeforeHitsAgainstAfter } }, null, 1));
  if (totals.WRONG !== 0) { console.error('WRONG_SOURCE_SELECTIONS != 0'); process.exitCode = 1; }
}

main();
