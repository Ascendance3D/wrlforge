'use strict';
// SHELL-0 module-loading spike -- build step. Evidence only; never shipped.
//
//   node spikes/shell-0-module-loading/build.js
//
// Writes everything under out/ (gitignored, regenerable):
//   out/bundle-basic.js(.map)    esbuild IIFE of app/esm/bundle-entry.js
//   out/many/{esm,classic}/...   one synthetic first-party graph, 41 modules,
//   out/many/bundle.js(.map)     in three loading strategies: native ESM,
//   out/many/*.html              classic defer tags (today) and one bundle
//   out/spike.asar               app/ + out/bundle-basic.* packed like a release
//   out/BUILD.json               build time, sizes, file counts, determinism
//
// Uses only the existing devDependencies (esbuild, and @electron/asar, which
// electron-builder already installs). Deterministic: no clock in any output.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const esbuild = require('esbuild');

const HERE = __dirname;
const OUT = path.join(HERE, 'out');
const MANY = path.join(OUT, 'many');
const CSP = "default-src 'none'; script-src 'self' file: 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; connect-src 'self' file: data: blob:; object-src 'none'; base-uri 'none';";
const FEATURES = 8;
const LEAVES = 4;

const sha = (buf) => crypto.createHash('sha256').update(buf).digest('hex');

// ~8 KB of ordinary first-party-looking code per module. Every function is
// referenced at load (as a real feature module's registrations would be), so
// the bundle cannot tree-shake the comparison away.
function body(name) {
  const fns = [];
  for (let k = 0; k < 24; k++) {
    fns.push(`function ${name}_f${k}(x, y) {
  const a = [x, y, ${k}].map((v) => (typeof v === 'number' ? v * ${k + 1} : String(v).length));
  let s = 0;
  for (const v of a) s += v % ${k + 7};
  if (s > ${k * 3}) return { id: '${name}.${k}', value: s, label: 'item-' + s.toString(16) };
  return { id: '${name}.${k}', value: -s, label: null };
}`);
  }
  return fns.join('\n');
}

function moduleNames() {
  const out = ['util'];
  for (let f = 0; f < FEATURES; f++) {
    out.push(`feature${f}`);
    for (let l = 0; l < LEAVES; l++) out.push(`leaf${f}_${l}`);
  }
  return out; // 1 + 8 + 32 = 41 modules (+ entry)
}

function writeGraph() {
  const esmDir = path.join(MANY, 'esm');
  const classicDir = path.join(MANY, 'classic');
  fs.mkdirSync(esmDir, { recursive: true });
  fs.mkdirSync(classicDir, { recursive: true });
  const names = moduleNames();
  const classicOrder = [];
  for (const name of names) {
    const imports = [];
    if (name.startsWith('feature')) {
      const f = name.slice(7);
      for (let l = 0; l < LEAVES; l++) imports.push(`import { leaf${f}_${l}_f0 } from './leaf${f}_${l}.js';`);
      imports.push("import { util_f0 } from './util.js';");
    }
    const uses = name.startsWith('feature')
      ? `[${[...Array(LEAVES).keys()].map((l) => `leaf${name.slice(7)}_${l}_f0(1, 2)`).join(', ')}, util_f0(3, 4)]` : '[]';
    const code = body(name);
    fs.writeFileSync(path.join(esmDir, `${name}.js`),
      `${imports.join('\n')}\n${code}\nexport { ${[...Array(24).keys()].map((k) => `${name}_f${k}`).join(', ')} };\n(globalThis.__reg || (globalThis.__reg = [])).push(['${name}', ${uses}.length, [${[...Array(24).keys()].map((k) => `${name}_f${k}`).join(', ')}].length]);\n`);
    classicOrder.push(name);
  }
  // classic order: leaves and util before features (what tag order must encode today)
  classicOrder.sort((a, b) => (a.startsWith('feature') ? 1 : 0) - (b.startsWith('feature') ? 1 : 0));
  for (const name of classicOrder) {
    const code = body(name);
    fs.writeFileSync(path.join(classicDir, `${name}.js`),
      `'use strict';\n(function () {\n${code}\nwindow.__mods = window.__mods || {};\nwindow.__mods.${name} = { ${[...Array(24).keys()].map((k) => `${name}_f${k}`).join(', ')} };\n(globalThis.__reg || (globalThis.__reg = [])).push(['${name}', 0]);\n})();\n`);
  }
  const entry = `${[...Array(FEATURES).keys()].map((f) => `import { feature${f}_f0 } from './feature${f}.js';`).join('\n')}
const r = [${[...Array(FEATURES).keys()].map((f) => `feature${f}_f0(5, 6)`).join(', ')}];
window.__spikeFinish({ modules: globalThis.__reg.length, results: r.length });
`;
  fs.writeFileSync(path.join(esmDir, 'entry.js'), entry);
  fs.writeFileSync(path.join(classicDir, 'zz-finish.js'), "'use strict';\nwindow.__spikeFinish({ modules: globalThis.__reg.length, results: 8 });\n");
  fs.copyFileSync(path.join(HERE, 'app', 'probe.js'), path.join(MANY, 'probe.js'));
  const page = (title, tags) => `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="${CSP}">
<title>${title}</title>
<script src="probe.js"></script>
${tags}
</head>
<body></body>
</html>
`;
  fs.writeFileSync(path.join(MANY, 'esm.html'), page('many: native ESM', '<script type="module" src="esm/entry.js"></script>'));
  fs.writeFileSync(path.join(MANY, 'classic.html'), page('many: classic defer tags',
    [...classicOrder, 'zz-finish'].map((n) => `<script defer src="classic/${n}.js"></script>`).join('\n')));
  fs.writeFileSync(path.join(MANY, 'bundle.html'), page('many: esbuild entry bundle', '<script defer src="bundle.js"></script>'));
  return { modules: names.length + 1, classicTags: classicOrder.length + 1 };
}

const BUNDLE_OPTS = { bundle: true, format: 'iife', target: 'chrome122', legalComments: 'none', sourcemap: 'linked', logLevel: 'silent' };

async function bundle(entryPoint, outfile) {
  const t0 = performance.now();
  await esbuild.build({ ...BUNDLE_OPTS, entryPoints: [entryPoint], outfile });
  const ms = performance.now() - t0;
  // determinism: two in-memory rebuilds must equal the written bytes
  const again = await Promise.all([1, 2].map(() => esbuild.build({ ...BUNDLE_OPTS, entryPoints: [entryPoint], outfile, write: false })));
  const written = fs.readFileSync(outfile);
  const deterministic = again.every((r) => {
    const js = r.outputFiles.find((f) => f.path === path.resolve(outfile));
    return js && sha(Buffer.from(js.contents)) === sha(written);
  });
  return { buildMs: Math.round(ms * 10) / 10, bytes: written.length, mapBytes: fs.statSync(outfile + '.map').size, sha256: sha(written), deterministic };
}

function dirBytes(dir) {
  let n = 0; let files = 0;
  for (const f of fs.readdirSync(dir)) { n += fs.statSync(path.join(dir, f)).size; files++; }
  return { bytes: n, files };
}

async function main() {
  fs.rmSync(OUT, { recursive: true, force: true });
  fs.mkdirSync(OUT, { recursive: true });
  const graph = writeGraph();
  const basic = await bundle(path.join(HERE, 'app', 'esm', 'bundle-entry.js'), path.join(OUT, 'bundle-basic.js'));
  const many = await bundle(path.join(MANY, 'esm', 'entry.js'), path.join(MANY, 'bundle.js'));

  // A release-like asar: the page tree plus the built bundle, read through
  // Electron's asar-aware file:// loader.
  const stage = path.join(OUT, 'asar-stage');
  fs.cpSync(path.join(HERE, 'app'), path.join(stage, 'app'), { recursive: true });
  fs.mkdirSync(path.join(stage, 'out'), { recursive: true });
  for (const f of ['bundle-basic.js', 'bundle-basic.js.map']) fs.copyFileSync(path.join(OUT, f), path.join(stage, 'out', f));
  await require('@electron/asar').createPackage(stage, path.join(OUT, 'spike.asar'));
  fs.rmSync(stage, { recursive: true, force: true });

  const report = {
    esbuild: require('esbuild/package.json').version,
    options: BUNDLE_OPTS,
    graph,
    bundleBasic: basic,
    bundleMany: many,
    esmMany: dirBytes(path.join(MANY, 'esm')),
    classicMany: dirBytes(path.join(MANY, 'classic')),
    asar: { bytes: fs.statSync(path.join(OUT, 'spike.asar')).size },
  };
  fs.writeFileSync(path.join(OUT, 'BUILD.json'), JSON.stringify(report, null, 2) + '\n');
  process.stdout.write(JSON.stringify(report, null, 2) + '\n');
}

main().catch((err) => { console.error(err && err.stack || err); process.exit(1); });
