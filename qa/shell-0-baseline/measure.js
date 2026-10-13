'use strict';
// SHELL-0 baseline measurement: startup, page switching and memory of the
// CURRENT multi-page application, before any shell migration
// (docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md §11-§15).
//
//   node qa/shell-0-baseline/measure.js [--launches=12] [--cycles=25] [--out=path]
//
// Method (all through the sanctioned harness; no product code is touched):
//   * Every Electron process is spawned by VisualQaRunner (concurrency 1, launch
//     cap, cooldown, PID tracking, post-run leak check) in the existing
//     WRL_FORGE_CAPTURE_SERVER mode, under the visual-QA lock and the workspace
//     guard. Jobs use the capture server's existing `world`, `keyboard.goto`
//     (the same loadFile + currentPage update gotoPage performs) and
//     `keyboard.executeJS` kinds. Nothing is injected into a document.
//   * STARTUP: N sequential launches, one process each (cold start is inherently
//     one launch per sample). Launch time is taken at spawn in this process;
//     the renderer reports its navigation timing against performance.timeOrigin
//     (an epoch clock on the same machine).
//   * PAGE SWITCH + SOAK: ONE process runs C round trips World -> Editor ->
//     World and C round trips Mall -> Editor -> Mall. Each leg is timed from the
//     moment the goto job is sent to the moment the destination page is usable
//     (editor: its own __wrlEditor.ready(); Mall/World: load complete + its
//     state check), polled at 5 ms inside the destination page.
//   * MEMORY: after a settle, the renderer forces two GCs (V8 --expose-gc, a
//     measurement-only switch) and reports performance.memory (precise-memory
//     switch); this process then reads VmRSS and Pss for every process in the
//     Electron tree from /proc (Linux). Renderer PIDs are recorded to detect
//     process replacement across navigations.
//   * Scratch fixtures live under the OS temp dir; Electron's userData is a
//     fresh temp profile, so the owner's preferences/recovery files are never
//     read or written.
//
// Output: qa/shell-0-baseline/RESULTS.json (machine-readable) + a summary.

const fs = require('fs');
const os = require('os');
const path = require('path');
const zlib = require('zlib');
const { spawn } = require('child_process');
const { VisualQaRunner } = require('../visual-qa/runner');
const { acquire } = require('../visual-qa/lock');
const { makeCaptureTransport } = require('../visual-qa/transport');
const { guardWindowsWorkspace } = require('../visual-qa/workspace-guard');
const { parseArgs } = require('../visual-qa/cli');

const repoRoot = path.join(__dirname, '..', '..');
const now = () => performance.timeOrigin + performance.now(); // hi-res epoch ms

// --- statistics --------------------------------------------------------------
function quantile(sorted, q) {
  if (!sorted.length) return null;
  const pos = (sorted.length - 1) * q;
  const lo = Math.floor(pos); const hi = Math.ceil(pos);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (pos - lo);
}
function stats(values) {
  const v = values.filter((x) => typeof x === 'number' && Number.isFinite(x)).sort((a, b) => a - b);
  if (!v.length) return null;
  const r = (x) => Math.round(x * 10) / 10;
  return { n: v.length, median: r(quantile(v, 0.5)), p10: r(quantile(v, 0.1)), p90: r(quantile(v, 0.9)),
    min: r(v[0]), max: r(v[v.length - 1]), iqr: r(quantile(v, 0.75) - quantile(v, 0.25)) };
}
// Least-squares slope per cycle and r^2 -- growth vs noise.
function trend(values) {
  const pts = values.map((y, x) => [x, y]).filter(([, y]) => Number.isFinite(y));
  const n = pts.length;
  if (n < 3) return null;
  const mx = pts.reduce((s, [x]) => s + x, 0) / n;
  const my = pts.reduce((s, [, y]) => s + y, 0) / n;
  let sxy = 0; let sxx = 0; let syy = 0;
  for (const [x, y] of pts) { sxy += (x - mx) * (y - my); sxx += (x - mx) ** 2; syy += (y - my) ** 2; }
  const slope = sxx ? sxy / sxx : 0;
  const r2 = sxx && syy ? (sxy * sxy) / (sxx * syy) : 0;
  const head = stats(pts.slice(0, 5).map(([, y]) => y));
  const tail = stats(pts.slice(-5).map(([, y]) => y));
  return { slopePerCycle: Math.round(slope * 10) / 10, r2: Math.round(r2 * 1000) / 1000,
    first5Median: head && head.median, last5Median: tail && tail.median };
}

// --- /proc process tree (Linux) -----------------------------------------------
function readProc(pid, file) { try { return fs.readFileSync(`/proc/${pid}/${file}`, 'utf8'); } catch { return null; } }
function kb(text, key) { const m = text && text.match(new RegExp(`^${key}:\\s+(\\d+) kB`, 'm')); return m ? Number(m[1]) : null; }
function processTree(rootPid) {
  if (process.platform !== 'linux') return null;
  const parent = new Map();
  for (const name of fs.readdirSync('/proc')) {
    if (!/^\d+$/.test(name)) continue;
    const stat = readProc(name, 'stat');
    if (!stat) continue;
    const after = stat.slice(stat.lastIndexOf(')') + 2).split(' ');
    parent.set(Number(name), Number(after[1]));
  }
  const tree = [rootPid];
  for (let i = 0; i < tree.length; i++) for (const [pid, pp] of parent) if (pp === tree[i]) tree.push(pid);
  // Zygote-forked children keep the zygote's argv, so their role is read from
  // the tree: children of the --no-zygote-sandbox zygote are the GPU process,
  // children of the plain zygote are renderers (the page, plus Chromium's
  // pre-warmed spare renderer).
  const argv = new Map(tree.map((pid) => [pid, (readProc(pid, 'cmdline') || '').split('\0')]));
  const typeOf = (pid) => {
    if (pid === rootPid) return 'main';
    const a = argv.get(pid) || [];
    const t = a.find((x) => x.startsWith('--type='));
    return t ? t.slice(7) : 'other';
  };
  return tree.map((pid) => {
    let type = typeOf(pid);
    const pp = parent.get(pid);
    if (type === 'zygote' && typeOf(pp) === 'zygote') {
      type = (argv.get(pp) || []).includes('--no-zygote-sandbox') ? 'gpu-process' : 'renderer';
    }
    return {
      pid, ppid: pp, type,
      rssKB: kb(readProc(pid, 'status'), 'VmRSS'),
      pssKB: kb(readProc(pid, 'smaps_rollup'), 'Pss'),
    };
  });
}
function summarizeTree(tree) {
  if (!tree) return null;
  const sum = (list, k) => list.reduce((s, p) => s + (p[k] || 0), 0);
  const renderers = tree.filter((p) => p.type === 'renderer').sort((a, b) => b.rssKB - a.rssKB);
  const active = renderers[0] || null; // the page renderer; the rest is the spare
  const main = tree.find((p) => p.type === 'main');
  return {
    mainRssKB: main ? main.rssKB : null,
    rendererRssKB: active ? active.rssKB : null,
    rendererPssKB: active ? active.pssKB : null,
    rendererPid: active ? active.pid : null,
    rendererCount: renderers.length,
    rendererPids: renderers.map((p) => p.pid),
    totalRssKB: sum(tree, 'rssKB'),
    totalPssKB: sum(tree, 'pssKB'),
    processes: tree.map((p) => ({ pid: p.pid, ppid: p.ppid, type: p.type, rssKB: p.rssKB, pssKB: p.pssKB })),
  };
}

// --- renderer probes (run through the capture server's executeJS) ---------------
const NAV = `(function(){var n=performance.getEntriesByType('navigation')[0]||{};var p={};performance.getEntriesByType('paint').forEach(function(e){p[e.name]=e.startTime;});
  return {timeOrigin:performance.timeOrigin,domInteractive:n.domInteractive,dclEnd:n.domContentLoadedEventEnd,loadEnd:n.loadEventEnd,firstPaint:p['first-paint'],fcp:p['first-contentful-paint'],path:location.pathname.split('/').pop()};})()`;
const READY = {
  editor: `!!(window.__wrlEditor && window.__wrlEditor.ready())`,
  mall: `document.readyState === 'complete'`,
  world: `document.readyState === 'complete' && typeof current !== 'undefined' && current !== null`,
};
// Poll (5 ms) until the page reports usable; return the epoch moment it was seen.
function readyProbe(page, timeoutMs = 8000) {
  const extra = page === 'mall' ? `mallStateRestored: typeof state !== 'undefined' && state !== null,`
    : page === 'world' ? `worldRehydrated: typeof current !== 'undefined' && current !== null,`
      : `editorHasDocument: !!(window.__wrlEditor && window.__wrlEditor.ready()),`;
  return `(async function(){var t0=performance.now();var ok=false;while(performance.now()-t0<${timeoutMs}){if(${READY[page]}){ok=true;break;}await new Promise(function(r){setTimeout(r,5);});}
    return {ready:ok,readyEpoch:performance.timeOrigin+performance.now(),${extra}nav:${NAV}};})()`;
}
const MEM_PROBE = (settleMs) => `(async function(){await new Promise(function(r){setTimeout(r,${settleMs});});var gc=typeof window.gc==='function';
  var before=performance.memory?performance.memory.usedJSHeapSize:null;if(gc){window.gc();window.gc();}await new Promise(function(r){setTimeout(r,250);});
  var m=performance.memory||{};return {gcAvailable:gc,heapUsedBeforeGC:before,heapUsed:m.usedJSHeapSize,heapTotal:m.totalJSHeapSize,path:location.pathname.split('/').pop()};})()`;

const js = (id, code) => ({ id, keyboard: { kind: 'executeJS', code } });
const goto = (id, page) => ({ id, keyboard: { kind: 'goto', page, delayMs: 1 } });

// --- fixtures -----------------------------------------------------------------
function stageFixtures() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-shell0-'));
  const mallPath = path.join(dir, 'item.wrl');
  fs.copyFileSync(path.join(repoRoot, 'test', 'fixtures', 'valid-gzip.wrl'), mallPath);
  const worldRoot = path.join(dir, 'world');
  fs.cpSync(path.join(repoRoot, 'test', 'fixtures', 'world', 'valid70'), worldRoot, { recursive: true });
  // sanity: the Mall fixture is a real gzip item
  zlib.gunzipSync(fs.readFileSync(mallPath));
  return { dir, mallPath, worldRoot, worldPrimary: path.join(worldRoot, 'world.wrl') };
}

function makeSpawner(userData, transportEnv, onSpawn) {
  return () => {
    const t = now();
    const child = spawn(require('electron'), [
      '.', '--no-sandbox', `--user-data-dir=${userData}`,
      '--enable-precise-memory-info', '--js-flags=--expose-gc',
    ], {
      cwd: repoRoot,
      env: { ...process.env, WRL_FORGE_CAPTURE_SERVER: '1', WRL_FORGE_NO_EDITOR: '1', WRL_FORGE_SETTLE_MS: '200', ...transportEnv },
      stdio: ['pipe', 'pipe', 'ignore'],
    });
    onSpawn(child, t);
    return child;
  };
}

// --- phase 1: startup ------------------------------------------------------------
async function measureStartup(launches, userData, transport, log) {
  const samples = [];
  for (let i = 0; i < launches; i++) {
    let spawnAt = null; let pid = null; let readyAt = null; let mem = null;
    const runner = new VisualQaRunner({
      spawn: makeSpawner(userData, transport.env, (child, t) => { spawnAt = t; pid = child.pid; }),
      maxLaunches: 1, retriesPerLaunch: 0, cooldownMs: 1500, readyTimeoutMs: 30000, captureTimeoutMs: 60000,
      now,
      log: (rec) => {
        log.push({ phase: 'startup', sample: i, ...rec, payload: undefined });
        if (rec.event === 'ready') readyAt = rec.ts;
        if (rec.event === 'capture:done' && rec.id === 'mem') mem = summarizeTree(processTree(pid));
      },
      ...transport.runnerOpts,
    });
    const [probe, heap] = await runner.run([js('probe', readyProbe('mall')), js('mem', MEM_PROBE(3000))]);
    const n = probe.executeJS.nav;
    samples.push({
      sample: i,
      startPage: n.path,
      spawnToNavStartMs: n.timeOrigin - spawnAt,
      spawnToDclMs: n.timeOrigin + n.dclEnd - spawnAt,
      spawnToLoadMs: n.timeOrigin + n.loadEnd - spawnAt,
      spawnToReadyLineMs: readyAt - spawnAt,
      spawnToFcpMs: n.fcp != null ? n.timeOrigin + n.fcp - spawnAt : null,
      navToDclMs: n.dclEnd, navToLoadMs: n.loadEnd,
      heapUsed: heap.executeJS.heapUsed, gcAvailable: heap.executeJS.gcAvailable,
      memory: mem,
    });
    process.stdout.write(`startup ${i + 1}/${launches}: launch->usable(DCL) ${Math.round(samples[i].spawnToDclMs)} ms\n`);
  }
  return samples;
}

// --- phase 2: page switching + soak ----------------------------------------------
function switchJobs(cycles, fx) {
  const jobs = [];
  // World: open the scratch project through the capture server's world job.
  jobs.push({ id: 'world-open', world: { root: fx.worldRoot, primary: fx.worldPrimary } });
  jobs.push(js('mem-world-0', MEM_PROBE(1500)));
  for (let i = 1; i <= cycles; i++) {
    // the World page's own button handler, minus the click: openWorldPrimary, then goto
    jobs.push(js(`w-ipc-${i}`, `(async function(){var a=performance.now();await window.vrmlpad.editor.openWorldPrimary();return {ipcMs:performance.now()-a};})()`));
    jobs.push(goto(`w-goto-editor-${i}`, 'editor'));
    jobs.push(js(`w-ready-editor-${i}`, readyProbe('editor')));
    jobs.push(js(`mem-w-editor-${i}`, MEM_PROBE(1000)));
    jobs.push(goto(`w-goto-world-${i}`, 'world'));
    jobs.push(js(`w-ready-world-${i}`, readyProbe('world')));
    jobs.push(js(`mem-w-world-${i}`, MEM_PROBE(1000)));
  }
  // Mall: open the scratch item exactly as the capture server's fixture job does.
  const openMall = `(async function(){var a=performance.now();var d=await window.vrmlpad.openMallPath(${JSON.stringify(fx.mallPath)});window.__wrlForgeApplyOpen(d);return {reopenMs:performance.now()-a};})()`;
  jobs.push(goto('mall-home', 'mall'));
  jobs.push(js('mall-open', openMall));
  jobs.push(js('mem-mall-0', MEM_PROBE(1500)));
  for (let i = 1; i <= cycles; i++) {
    // The Mall page loses its item on return (no mall:describe). A user must
    // re-open it; that manual rehydration is done here, OUTSIDE the switch timing.
    jobs.push(js(`m-ipc-${i}`, `(async function(){var reopened=false;if(typeof state==='undefined'||state===null){reopened=true;await (${openMall});}
      var a=performance.now();await window.vrmlpad.editor.openMall();return {ipcMs:performance.now()-a,reopened:reopened};})()`));
    jobs.push(goto(`m-goto-editor-${i}`, 'editor'));
    jobs.push(js(`m-ready-editor-${i}`, readyProbe('editor')));
    jobs.push(js(`mem-m-editor-${i}`, MEM_PROBE(1000)));
    jobs.push(goto(`m-goto-mall-${i}`, 'mall'));
    jobs.push(js(`m-ready-mall-${i}`, readyProbe('mall')));
    jobs.push(js(`mem-m-mall-${i}`, MEM_PROBE(1000)));
  }
  return jobs;
}

async function measureSwitching(cycles, userData, fx, transport, log) {
  let pid = null;
  const sentAt = new Map(); const trees = new Map();
  const runner = new VisualQaRunner({
    spawn: makeSpawner(userData, transport.env, (child) => { pid = child.pid; }),
    maxLaunches: 1, retriesPerLaunch: 0, readyTimeoutMs: 30000, captureTimeoutMs: 60000,
    now,
    log: (rec) => {
      if (rec.event === 'capture:start') sentAt.set(rec.id, rec.ts);
      if (rec.event === 'capture:done' && rec.id.startsWith('mem-')) trees.set(rec.id, summarizeTree(processTree(pid)));
      if (rec.event !== 'capture:start' && rec.event !== 'capture:done') log.push({ phase: 'switch', ...rec });
    },
    ...transport.runnerOpts,
  });
  const jobs = switchJobs(cycles, fx);
  process.stdout.write(`switching: ${jobs.length} jobs in one process...\n`);
  const results = await runner.run(jobs);
  const byId = new Map(results.map((r) => [r.id, r]));
  const legs = [];
  for (const [prefix, origin] of [['w', 'world'], ['m', 'mall']]) {
    for (let i = 1; i <= cycles; i++) {
      for (const [to, from] of [['editor', origin], [origin, 'editor']]) {
        const ready = byId.get(`${prefix}-ready-${to}-${i}`).executeJS;
        const t0 = sentAt.get(`${prefix}-goto-${to}-${i}`);
        legs.push({
          roundTrip: `${origin}->editor->${origin}`, cycle: i, from, to,
          ready: ready.ready,
          switchMs: ready.readyEpoch - t0,
          requestToNavStartMs: ready.nav.timeOrigin - t0,
          navToLoadMs: ready.nav.loadEnd,
          mallStateRestored: ready.mallStateRestored,
          worldRehydrated: ready.worldRehydrated,
          editorHasDocument: ready.editorHasDocument,
          ipcMs: to === 'editor' ? byId.get(`${prefix}-ipc-${i}`).executeJS.ipcMs : null,
          mallReopenedByUser: prefix === 'm' && to === 'editor' ? byId.get(`m-ipc-${i}`).executeJS.reopened : null,
        });
      }
    }
  }
  const mem = [];
  for (const r of results) {
    if (!r.id.startsWith('mem-')) continue;
    mem.push({ id: r.id, ...r.executeJS, tree: trees.get(r.id) || null });
  }
  return { jobs: jobs.length, legs, mem };
}

function analyze(startup, switching) {
  const warm = startup.slice(1);
  const pick = (list, k) => list.map((s) => s[k]);
  const startupSummary = {
    firstLaunch: startup[0] ? { spawnToDclMs: startup[0].spawnToDclMs, spawnToLoadMs: startup[0].spawnToLoadMs } : null,
    subsequent: {
      spawnToNavStartMs: stats(pick(warm, 'spawnToNavStartMs')),
      spawnToDclMs: stats(pick(warm, 'spawnToDclMs')),
      spawnToFcpMs: stats(pick(warm, 'spawnToFcpMs')),
      spawnToLoadMs: stats(pick(warm, 'spawnToLoadMs')),
      spawnToReadyLineMs: stats(pick(warm, 'spawnToReadyLineMs')),
      navToDclMs: stats(pick(warm, 'navToDclMs')),
      rendererHeapUsedMB: stats(warm.map((s) => s.heapUsed / 1048576)),
      mainRssMB: stats(warm.map((s) => s.memory && s.memory.mainRssKB / 1024)),
      rendererRssMB: stats(warm.map((s) => s.memory && s.memory.rendererRssKB / 1024)),
      rendererPssMB: stats(warm.map((s) => s.memory && s.memory.rendererPssKB / 1024)),
      totalPssMB: stats(warm.map((s) => s.memory && s.memory.totalPssKB / 1024)),
    },
    startPages: [...new Set(startup.map((s) => s.startPage))],
  };
  const legKey = (l) => `${l.from}->${l.to} (${l.roundTrip})`;
  const legSummary = {};
  for (const l of switching.legs) {
    const k = legKey(l);
    (legSummary[k] = legSummary[k] || []).push(l);
  }
  const switchSummary = {};
  for (const [k, list] of Object.entries(legSummary)) {
    switchSummary[k] = {
      switchMs: stats(list.map((l) => l.switchMs)),
      requestToNavStartMs: stats(list.map((l) => l.requestToNavStartMs)),
      navToLoadMs: stats(list.map((l) => l.navToLoadMs)),
      allReady: list.every((l) => l.ready),
      mallStateRestored: list[0].mallStateRestored === undefined ? undefined : list.filter((l) => l.mallStateRestored).length + '/' + list.length,
      worldRehydrated: list[0].worldRehydrated === undefined ? undefined : list.filter((l) => l.worldRehydrated).length + '/' + list.length,
      editorHasDocument: list[0].editorHasDocument === undefined ? undefined : list.filter((l) => l.editorHasDocument).length + '/' + list.length,
      mallReopenedByUser: list[0].mallReopenedByUser == null ? undefined : list.filter((l) => l.mallReopenedByUser).length + '/' + list.length,
    };
  }
  const series = (re) => switching.mem.filter((m) => re.test(m.id));
  const memSeries = (re) => {
    const s = series(re);
    const pagePids = s.map((m) => (m.tree ? m.tree.rendererPid : null));
    return {
      samples: s.length,
      heapUsedMB: trend(s.map((m) => m.heapUsed / 1048576)),
      heapBeforeGcMB: trend(s.map((m) => m.heapUsedBeforeGC / 1048576)),
      mainRssMB: trend(s.map((m) => m.tree && m.tree.mainRssKB / 1024)),
      rendererRssMB: trend(s.map((m) => m.tree && m.tree.rendererRssKB / 1024)),
      totalPssMB: trend(s.map((m) => m.tree && m.tree.totalPssKB / 1024)),
      distinctPageRendererPids: new Set(pagePids).size,
    };
  };
  const stable = (id) => {
    const m = switching.mem.find((x) => x.id === id);
    return m && { heapUsedMB: m.heapUsed / 1048576, mainRssMB: m.tree && m.tree.mainRssKB / 1024,
      rendererRssMB: m.tree && m.tree.rendererRssKB / 1024, rendererPssMB: m.tree && m.tree.rendererPssKB / 1024,
      totalPssMB: m.tree && m.tree.totalPssKB / 1024 };
  };
  return {
    startup: startupSummary,
    switching: switchSummary,
    memory: {
      stabilized: {
        world: stable('mem-world-0'), mall: stable('mem-mall-0'),
        editorFromWorld: stable('mem-w-editor-1'), editorFromMall: stable('mem-m-editor-1'),
      },
      soak: {
        worldPage: memSeries(/^mem-w-world-/), editorFromWorld: memSeries(/^mem-w-editor-/),
        mallPage: memSeries(/^mem-m-mall-/), editorFromMall: memSeries(/^mem-m-editor-/),
      },
    },
  };
}

// Raw OS process ids are transient and machine-specific. They are replaced,
// file-wide, by first-seen ordinals ("p1", "p2", ...): the same process keeps
// the same ordinal in every sample, so the tree's parent links and the
// "one page renderer across navigations" evidence survive without the ids.
const PID_KEYS = new Set(['pid', 'ppid', 'rendererPid']);
function anonymizePids(value) {
  const ordinals = new Map();
  const label = (pid) => {
    if (typeof pid !== 'number') return pid;
    if (!ordinals.has(pid)) ordinals.set(pid, `p${ordinals.size + 1}`);
    return ordinals.get(pid);
  };
  const walk = (v) => {
    if (Array.isArray(v)) return v.map(walk);
    if (!v || typeof v !== 'object') return v;
    const out = {};
    for (const [k, x] of Object.entries(v)) {
      if (PID_KEYS.has(k)) out[k] = label(x);
      else if (k === 'rendererPids' && Array.isArray(x)) out[k] = x.map(label);
      else out[k] = walk(x);
    }
    return out;
  };
  return walk(value);
}

const RESULTS_SCOPE = 'Repeatable SHELL-0 QA baseline (capture-server mode, --no-sandbox, warm page cache, small fixtures). '
  + 'Evidence for later comparison under equivalent methodology -- not a golden file, not an end-user performance claim, '
  + 'and never asserted byte-for-byte or numerically by CI.';
const PID_POLICY = 'Process ids are file-wide first-seen ordinals (p1, p2, ...), not OS pids.';

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const launches = Number(args.flags.launches || 12);
  const cycles = Number(args.flags.cycles || 25);
  const outPath = args.flags.out || path.join(__dirname, 'RESULTS.json');
  guardWindowsWorkspace({ cwd: repoRoot, label: 'shell-0-baseline' });
  if (process.platform !== 'win32' && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
    console.error('shell-0-baseline: no DISPLAY/WAYLAND_DISPLAY -- refusing to launch Electron headless-blind.');
    process.exit(2);
  }
  const transport = makeCaptureTransport();
  const fx = stageFixtures();
  const userData = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-shell0-profile-'));
  const log = [];
  const release = acquire();
  let startup; let switching;
  try {
    startup = await measureStartup(launches, userData, transport, log);
    switching = await measureSwitching(cycles, userData, fx, transport, log);
  } finally {
    release();
    transport.cleanup();
    for (const d of [fx.dir, userData]) { try { fs.rmSync(d, { recursive: true, force: true }); } catch { /* ignore */ } }
  }
  const electronVersion = require('electron/package.json').version;
  const results = {
    lane: 'SHELL-0',
    generatedAt: new Date().toISOString(),
    scope: RESULTS_SCOPE,
    method: {
      pidPolicy: PID_POLICY,
      harness: 'VisualQaRunner + WRL_FORGE_CAPTURE_SERVER (existing world / keyboard.goto / keyboard.executeJS jobs)',
      electronSwitches: ['--no-sandbox (as every visual-QA orchestrator)', '--enable-precise-memory-info', '--js-flags=--expose-gc'],
      launches, cycles,
      startupStatsExcludeFirstLaunch: true,
      memorySettleMs: { startup: 3000, soak: 1000 },
      fixtures: { mall: 'test/fixtures/valid-gzip.wrl', world: 'test/fixtures/world/valid70' },
      clock: 'performance.timeOrigin + performance.now() in both processes (epoch ms, same machine)',
      memorySource: process.platform === 'linux' ? '/proc/<pid>/status VmRSS + /proc/<pid>/smaps_rollup Pss over the Electron process tree' : 'unavailable on this platform',
    },
    machine: {
      platform: `${process.platform} ${os.release()}`, arch: process.arch,
      cpu: os.cpus()[0] && os.cpus()[0].model, cores: os.cpus().length,
      totalMemGB: Math.round(os.totalmem() / 1073741824),
      node: process.version, electron: electronVersion,
      display: process.env.WAYLAND_DISPLAY ? 'wayland' : 'x11',
    },
    summary: analyze(startup, switching),
    raw: { startup, legs: switching.legs, mem: switching.mem, switchJobs: switching.jobs },
    runnerLog: log,
  };
  fs.writeFileSync(outPath, JSON.stringify(anonymizePids(results), null, 2) + '\n');
  process.stdout.write(JSON.stringify(results.summary, null, 2) + '\n');
  process.stdout.write(`wrote ${path.relative(repoRoot, outPath)}\n`);
}

if (require.main === module) {
  main().catch((err) => { console.error(err && err.stack || err); process.exit(1); });
}

module.exports = { stats, trend, summarizeTree, anonymizePids, RESULTS_SCOPE, PID_POLICY };
