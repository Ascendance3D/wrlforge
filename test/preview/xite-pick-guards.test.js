'use strict';
// WD2-D upgrade guards (WD2_D_XITE_PICKING_CONTRACT.md §Upgrade guards).
// These fail LOUDLY if the X_ITE pin or the private picking surface drifts,
// or if private X_ITE access leaks out of the one adapter module.
//
//   G1  package.json pins x_ite exactly 15.1.10; lockfile resolves 15.1.10
//   G2  installed node_modules/x_ite is 15.1.10
//   G3  x_ite.min.js still contains each private name; x_ite.d.ts still does
//       NOT declare the private ones (if X_ITE publishes them, revisit)
//   G8  source scan: private names only in src/preview/xite-pick-adapter.js;
//       the pure resolver touches no X_ITE API; no module-level capture state
//       in the adapter; no fallback matcher in either
//   + the fixtures are the production WD2-C First Object bytes, the oracle is
//     independent, and the C0 research artifact is not a production input.
//   + contract surface: the adapter module exports ONLY the factory, an
//     adapter has EXACTLY the seven contract methods, the one coordinator is
//     module-private (no factory, no injection, no state export), and WD2-D
//     needs nothing from main.js / preload.js.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const ROOT = path.join(__dirname, '..', '..');
const read = (rel) => fs.readFileSync(path.join(ROOT, rel), 'utf8');
// Source without comments, so prose never trips (or satisfies) a scan.
const code = (rel) => read(rel).replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:'"`\\])\/\/.*$/gm, '$1');

const ADAPTER = 'src/preview/xite-pick-adapter.js';
const RESOLVER = 'src/editor/viewport-pick.js';
const PIN = '15.1.10';

test('G1: x_ite is pinned exactly (no range) in package.json and the lockfile', () => {
  const pkg = JSON.parse(read('package.json'));
  assert.equal(pkg.dependencies.x_ite, PIN);
  const lock = JSON.parse(read('package-lock.json'));
  assert.equal(lock.packages[''].dependencies.x_ite, PIN);
  assert.equal(lock.packages['node_modules/x_ite'].version, PIN);
  assert.match(code(ADAPTER), new RegExp(`const XITE_VERSION = '${PIN.replace(/\./g, '\\.')}';`), 'the adapter checks the same version at runtime');
});

test('G2: the installed X_ITE is exactly 15.1.10', () => {
  const installed = JSON.parse(read('node_modules/x_ite/package.json'));
  assert.equal(installed.version, PIN);
});

test('G3: every private surface still exists in the shipped bundle; none is public', () => {
  const min = read('node_modules/x_ite/dist/x_ite.min.js');
  for (const name of ['VRMLParser', 'nodeStatement', 'comments', 'lastIndex', 'getScene', 'getExecutionContext',
    'isInsideProtoDeclaration', 'touch', 'getHit', 'shapeNode', 'sensors', 'getParents', 'getWorld', 'getLayer0',
    'groupNodes', 'getViewport', 'X3DBaseNode', 'X3DExecutionContext', 'X3DScene', 'getTypeName']) {
    assert.ok(min.includes(name), `x_ite.min.js no longer contains ${name}`);
  }
  const dts = read('node_modules/x_ite/dist/x_ite.d.ts');
  for (const name of ['nodeStatement', 'getHit', 'getParents', 'getLayer0']) {
    assert.ok(!new RegExp(`\\b${name}\\b`).test(dts), `x_ite.d.ts now declares ${name}: revisit the private adapter`);
  }
});

// Production JS outside node_modules / spikes / tests / vendor bundles.
function productionFiles() {
  const out = ['main.js', 'preload.js', 'validator.js'].filter((f) => fs.existsSync(path.join(ROOT, f)));
  const walk = (dir) => {
    for (const e of fs.readdirSync(path.join(ROOT, dir), { withFileTypes: true })) {
      const rel = path.posix.join(dir, e.name);
      if (e.isDirectory()) { if (e.name !== 'vendor') walk(rel); } else if (e.name.endsWith('.js')) out.push(rel);
    }
  };
  walk('src');
  walk('renderer');
  return out;
}

const PRIVATE = [/\bVRMLParser\b/, /\bnodeStatement\b/, /\.touch\s*\(/, /\bgetHit\b/, /\bgetParents\b/,
  /\bgetLayer0\b/, /\bgroupNodes?\b/, /\bgetViewport\b/, /\bisInsideProtoDeclaration\b/, /\.getExecutionContext\s*\(/];

test('G8: private X_ITE picking surfaces are referenced only by the adapter', () => {
  const files = productionFiles();
  assert.ok(files.includes(ADAPTER) && files.includes(RESOLVER) && files.includes('renderer/editor.js'));
  for (const f of files) {
    if (f === ADAPTER) continue;
    const src = code(f);
    for (const re of PRIVATE) assert.doesNotMatch(src, re, `${f} references private X_ITE surface ${re}`);
  }
  // and the adapter really is where they live
  const a = code(ADAPTER);
  for (const re of PRIVATE.slice(0, 6)) assert.match(a, re);
});

test('G8: the pure resolver references no X_ITE API at all, public or private', () => {
  const src = code(RESOLVER);
  for (const re of [/\bX3D\b/, /\bbrowser\b/, /createX3DFromString|replaceWorld|currentScene|getTypeName/, /\bwindow\.[A-Za-z]|\bdocument\.[A-Za-z]/,
    /require\([^)]*preview\//]) {
    assert.doesNotMatch(src, re, `viewport-pick.js references ${re}`);
  }
});

test('G8: no module-level capture / record / map state in the adapter', () => {
  const src = code(ADAPTER);
  // Module scope = the IIFE body outside createXitePickAdapter / the coordinator.
  const body = src.slice(src.indexOf('(function () {'));
  const COORD = /const hookCoordinator = \(function \(\) \{([\s\S]*?)\n {2}\}\)\(\);\n/;
  const moduleScope = body
    .replace(COORD, '')
    .replace(/function createXitePickAdapter\([\s\S]*?\n {2}\}\n\n {2}const pickAdapterApi/, 'const pickAdapterApi');
  for (const re of [/\bnew (Weak)?Map\(/, /\bnew Set\(/, /\b(capture|records|occ|gens|generation|currentScene)\s*=/, /\bwindow\.\w+ = (?!pickAdapterApi;)/]) {
    assert.doesNotMatch(moduleScope, re, `module-level state ${re} in the adapter`);
  }
  // the coordinator holds only arbitration state
  const coord = COORD.exec(body)[1];
  const lets = [...coord.matchAll(/\b(?:let|const) (\w+)/g)].map((m) => m[1]).filter((n) => !['w', 'i'].includes(n));
  for (const n of lets) assert.ok(['owner', 'waiters', 'disabled'].includes(n), `coordinator state ${n}`);
  // the capture is created inside one parse call and gated on the exact text
  assert.match(src, /const capture = \{ text, records: \[\], poisoned: false, closed: false \}/);
  assert.match(src, /capture\.closed \|\| this\.input !== capture\.text/);
  assert.doesNotMatch(src, /createX3DFromString\((?!text\)|String\(text\)\)|PROBE_TEXT)/, 'X_ITE must be given the unmodified text');
});

test('G8: no fallback strategy in the adapter or the resolver', () => {
  for (const f of [ADAPTER, RESOLVER]) {
    const src = code(f);
    for (const bad of [/getName|\.def\b|defsByName/, /nearest|closest|itemContainingOffset/, /indexOf|findIndex|childIds\[/,
      /fingerprint|similar|levenshtein|score/i, /modelViewMatrix|matrix/i, /\.sort\(/]) {
      assert.doesNotMatch(src, bad, `${f}: forbidden fallback pattern ${bad}`);
    }
  }
});

test('G8: the touch() === false rule is structural: getHit() only after a strict true', () => {
  const src = code(ADAPTER);
  const t = src.indexOf('.touch(x, y)');
  const g = src.indexOf('.getHit()');
  assert.ok(t > 0 && g > t, 'getHit() follows touch()');
  assert.match(src.slice(t, g), /\.touch\(x, y\) !== true\) return snap\('no-hit', 'viewer-active-or-no-hit'/);
  assert.equal((src.match(/\.getHit\(\)/g) || []).length, 1, 'exactly one getHit() call site');
});

test('fixtures: the WD2-C Box / Sphere are the production First Object bytes; the oracle is independent', () => {
  const templates = require('../../src/vrml/node-templates');
  const fixtures = require('./_pick-fixtures');
  const p3 = fixtures.BUILDERS['P3-wd2c-box']({});
  const s = p3.spans['box.transform'];
  assert.equal(p3.text.slice(s.start, s.end), templates.simpleObjectTemplate('Box').text);
  const p4 = fixtures.BUILDERS['P4-wd2c-box-sphere']({});
  const t = p4.spans['sphere.transform'];
  assert.equal(p4.text.slice(t.start, t.end).replace('  translation 2 0 0\n', ''), templates.simpleObjectTemplate('Sphere').text);
  const oracle = code('test/preview/_pick-fixtures.js');
  assert.doesNotMatch(oracle, /require\(/, 'the oracle requires nothing (not src/, not the fake, not the resolver)');
});

test('the C0 research spike is not a production input', () => {
  for (const f of productionFiles()) assert.doesNotMatch(code(f), /spikes\//, `${f} must not load the C0 spike`);
});

// ---- contract surface (QA #1 / #2 blocking findings) ---------------------------
const CONTRACT_METHODS = ['abort', 'activate', 'compatibility', 'dispose', 'parseWithProvenance', 'pick', 'retire'];

function freshAdapterModule() {
  const abs = path.join(ROOT, ADAPTER);
  delete require.cache[require.resolve(abs)];
  return require(abs);
}

test('surface: the module exports ONLY createXitePickAdapter (CommonJS and browser global)', () => {
  assert.deepEqual(Object.keys(freshAdapterModule()), ['createXitePickAdapter']);
  // The page build: a classic script in a window realm, no `module`.
  const sandbox = { window: {} };
  vm.runInNewContext(read(ADAPTER), sandbox, { filename: ADAPTER });
  assert.deepEqual(Object.keys(sandbox.window), ['WrlXitePickAdapter'], 'exactly one page global');
  assert.deepEqual(Object.keys(sandbox.window.WrlXitePickAdapter), ['createXitePickAdapter']);
  assert.ok(Object.isFrozen(sandbox.window.WrlXitePickAdapter));
});

test('surface: an adapter has EXACTLY the seven contract methods, frozen', () => {
  const { createFakeX3D, createFakeBrowser } = require('./_fake-xite');
  const A = freshAdapterModule();
  const X3D = createFakeX3D();
  const adapter = A.createXitePickAdapter({ X3D, browser: createFakeBrowser(X3D) });
  assert.deepEqual(Object.keys(adapter).sort(), CONTRACT_METHODS);
  for (const k of CONTRACT_METHODS) assert.equal(typeof adapter[k], 'function', k);
  assert.ok(Object.isFrozen(adapter));
  // no WD2-D escape hatch under any of the names QA found
  for (const k of ['isCurrent', 'ready', 'hookDiagnostics', 'createHookCoordinator', 'coordinatorState', 'coordinator']) {
    assert.equal(k in adapter, false, `adapter.${k}`);
    assert.equal(k in A, false, `module.${k}`);
  }
  adapter.dispose();
});

test('one coordinator: a caller-supplied coordinator is ignored, never used', async () => {
  const { createFakeX3D, createFakeBrowser, settleProbe } = require('./_fake-xite');
  const A = freshAdapterModule();
  const X3D = createFakeX3D();
  const used = [];
  const spy = new Proxy({}, { get: (t, k) => { used.push(k); return () => Promise.resolve('owned'); } });
  const browser = createFakeBrowser(X3D);
  const adapter = A.createXitePickAdapter({ X3D, browser, coordinator: spy, probe: false });
  assert.deepEqual(adapter.compatibility(), { ok: false, reason: 'compatibility-unproven' }, '`probe: false` cannot skip G4');
  assert.deepEqual(await settleProbe(adapter), { ok: true });
  const r = await adapter.parseWithProvenance('#VRML V2.0 utf8\nShape { geometry Box { } }\n', { generationId: 1 });
  assert.ok(r.generation);
  assert.deepEqual(used, [], 'the injected coordinator was never touched');
  adapter.dispose();
});

test('one coordinator: created once, module-private, never exported or injectable (source)', () => {
  const src = code(ADAPTER);
  assert.equal((src.match(/const hookCoordinator = \(function \(\) \{/g) || []).length, 1, 'exactly one coordinator instance');
  assert.doesNotMatch(src, /function createHookCoordinator|createHookCoordinator\s*[:,(]/, 'no coordinator factory');
  assert.match(src, /function createXitePickAdapter\(\{ X3D, browser \} = \{\}\)/, 'the factory takes exactly { X3D, browser }');
  assert.match(src, /const coordinator = hookCoordinator;/, 'every adapter uses the one coordinator');
  assert.match(src, /const pickAdapterApi = Object\.freeze\(\{ createXitePickAdapter \}\);/);
});

test('WD2-D needs no main-process or preload support', () => {
  // Targeted, not a snapshot: none of WD2-D's names or its old QA input path.
  for (const f of ['main.js', 'preload.js']) {
    const src = code(f);
    for (const re of [/xite-pick-adapter|viewport-pick/, /WrlXitePickAdapter|viewportPick|ViewportPick/, /pickState|pickTarget|armPicking/,
      /mouse\(Down\|Up\|Move\)|type: ['"]mouse(Down|Up|Move)['"]/]) {
      assert.doesNotMatch(src, re, `${f}: WD2-D dependency ${re}`);
    }
  }
});

test('every structural disable reason maps to a contract P-row for the console line', () => {
  const VP = require('../../src/editor/viewport-pick');
  const src = read(ADAPTER); // the P-row comments are the anchor here
  const reasons = new Set([
    ...[...src.matchAll(/return '([a-z-]+)'; \/\/ (?:P\d+|V1)/g)].map((m) => m[1]),
    ...[...src.matchAll(/structuralDisable\('([a-z-]+)'/g)].map((m) => m[1]),
  ]);
  for (const r of ['parser-class-missing', 'parser-hook-missing', 'parser-comments-missing', 'parser-context-missing',
    'class-missing', 'touch-missing', 'hit-shape-changed', 'world-infrastructure-missing', 'viewport-missing',
    'parser-offsets-unproven', 'parser-hook-displaced', 'runtime-parents-unavailable', 'xite-version-mismatch']) {
    assert.ok(reasons.has(r), `adapter reason ${r}`);
  }
  for (const r of reasons) assert.match(VP.compatibilityRow(r), /^(P\d+(\/(P|G)\d+)?|G\d|displaced-wrapper)$/, r);
});
