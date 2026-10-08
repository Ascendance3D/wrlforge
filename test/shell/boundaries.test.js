'use strict';
// SHELL-0 structural boundaries, asserted by source scan:
//   * the contract modules are foundation-only -- no production page, main or
//     preload loads them yet (later lanes adopt them);
//   * no shared shell module reaches a profile's rules, the filesystem,
//     Electron, the parser, or anything that could become a second source;
//   * every module uses the dual export and an IIFE (classic-script safe).
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');

const ROOT = path.join(__dirname, '..', '..');
const SHELL = path.join(ROOT, 'src', 'shell');
const modules = fs.readdirSync(SHELL).filter((f) => f.endsWith('.js')).sort();
const read = (p) => fs.readFileSync(p, 'utf8');
const code = (src) => src.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '');
// code() minus string/template literal bodies (prose in error messages is not a dependency).
const bare = (src) => code(src).replace(/'(?:[^'\\\n]|\\.)*'|`(?:[^`\\]|\\.)*`/g, "''");

test('the shell contract module set', () => {
  assert.deepEqual(modules, [
    'command-service.js', 'contextual-panels.js', 'contribution.js', 'disposable.js',
    'document-session.js', 'menu-boundary.js', 'panel-service.js', 'profiles.js',
    'services.js', 'tool-registry.js',
  ]);
});

// Every production source file: main, preload, renderer/** and src/** outside
// src/shell (the vendor bundle directory is generated, not authored).
function productionFiles() {
  const out = ['main.js', 'preload.js'];
  const walk = (rel) => {
    for (const d of fs.readdirSync(path.join(ROOT, rel), { withFileTypes: true })) {
      const r = path.join(rel, d.name);
      if (d.isDirectory()) {
        if (r === path.join('src', 'shell') || r === path.join('renderer', 'vendor')) continue;
        walk(r);
      } else if (/\.(html|js)$/.test(d.name)) out.push(r);
    }
  };
  walk('renderer');
  walk('src');
  return out;
}

test('foundation only: no production entry point loads src/shell yet', () => {
  const files = productionFiles();
  assert.ok(files.length > 20, 'scan found the production tree');
  for (const f of files) {
    const src = read(path.join(ROOT, f));
    assert.ok(!src.includes('src/shell/'), `${f} must not load src/shell in SHELL-0`);
    assert.ok(!/require\(\s*['"][./]*shell\//.test(src), `${f} must not require the shell layer in SHELL-0`);
    assert.ok(!src.includes('WrlShell'), `${f} must not use a WrlShell* global in SHELL-0`);
  }
});

test('no shared shell module imports profile rules, fs, Electron, the parser or a DOM global', () => {
  const ALLOWED = new Set(['./disposable', './services', './profiles', '../editor/command-registry']);
  for (const f of modules) {
    const src = code(read(path.join(SHELL, f)));
    for (const m of src.matchAll(/require\(\s*['"]([^'"]+)['"]\s*\)/g)) {
      assert.ok(ALLOWED.has(m[1]), `${f} requires ${m[1]}`);
    }
    const body = bare(read(path.join(SHELL, f)));
    for (const banned of ['validator', 'ipcRenderer', 'vrmlpad', 'document.', 'localStorage', 'process.', 'X3D']) {
      assert.ok(!body.includes(banned), `${f} uses ${banned}`);
    }
  }
});

test('dual export + IIFE in every module (safe as a classic <script> later)', () => {
  for (const f of modules) {
    const src = read(path.join(SHELL, f));
    assert.match(src, /\n\(function \(\) \{\n/, `${f} is not wrapped in an IIFE`);
    assert.match(src, /module\.exports = WRL_SHELL_[A-Z_]+_API;/, `${f} has no CommonJS export`);
    assert.match(src, /window\.WrlShell[A-Za-z]+ = WRL_SHELL_[A-Z_]+_API;/, `${f} has no browser export`);
  }
});
