'use strict';
// Cross-platform editor discovery (Phase 6A). Exercises the Windows AND Linux
// resolution paths on ANY host by injecting `platform`, `env`, and `existsSync`
// (no real filesystem, no spawning). This is the explicit, code-based Windows
// verification the lane requires — Windows behavior is tested from Linux.

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('path');
const { resolveEditor, buildLaunch, isCmdShim, macCandidates, EDITOR_ENV } = require('../../src/editor/editor-locator');

// A fake fs: `existsSync` true only for the given set of paths.
const fakeExists = (present) => {
  const set = new Set(present);
  return (p) => set.has(p);
};

// Windows-ish env (POSIX-style bases so path.isAbsolute/path.join stay consistent
// on the Linux test host; PATH uses ';' as Windows does).
const winEnv = (over = {}) => ({
  LOCALAPPDATA: '/win/AppData/Local',
  ProgramFiles: '/win/Program Files',
  'ProgramFiles(x86)': '/win/Program Files (x86)',
  PATH: '/win/System32;/win/tools',
  PATHEXT: '.COM;.EXE;.BAT;.CMD',
  ...over,
});
const vscodiumExe = () => path.join('/win/AppData/Local', 'Programs', 'VSCodium', 'VSCodium.exe');
const vscodiumCmd = () => path.join('/win/AppData/Local', 'Programs', 'VSCodium', 'bin', 'codium.cmd');
const vscodeExe = () => path.join('/win/AppData/Local', 'Programs', 'Microsoft VS Code', 'Code.exe');

// ---- Linux / macOS ---------------------------------------------------------

test('linux: finds codium on PATH', () => {
  const codium = path.join('/usr/bin', 'codium');
  const r = resolveEditor({ platform: 'linux', env: { PATH: '/usr/bin:/bin' }, existsSync: fakeExists([codium]) });
  assert.equal(r.found, true);
  assert.equal(r.command, codium);
  assert.equal(r.shell, false);
});

test('linux: falls back to code when codium absent', () => {
  const code = path.join('/usr/bin', 'code');
  const r = resolveEditor({ platform: 'linux', env: { PATH: '/usr/bin' }, existsSync: fakeExists([code]) });
  assert.equal(r.command, code);
});

test('linux: path-default codium preserves historical behavior when PATH probe misses', () => {
  const r = resolveEditor({ platform: 'linux', env: { PATH: '/nothing' }, existsSync: fakeExists([]) });
  assert.equal(r.found, true);
  assert.equal(r.command, 'codium');
  assert.equal(r.source, 'path-default');
});

test('macOS: finds VSCodium in the user Applications folder without relying on PATH', () => {
  const env = { HOME: '/Users/tester', PATH: '/usr/bin:/bin' };
  const candidate = macCandidates(env)[0];
  const r = resolveEditor({ platform: 'darwin', env, existsSync: fakeExists([candidate]) });
  assert.equal(r.found, true);
  assert.equal(r.command, candidate);
  assert.equal(r.shell, false);
  assert.equal(r.source, 'install-location');
});

test('macOS: finds system-wide VS Code when VSCodium is absent', () => {
  const env = { HOME: '/Users/tester', PATH: '/usr/bin:/bin' };
  const candidate = macCandidates(env).at(-1);
  const r = resolveEditor({ platform: 'darwin', env, existsSync: fakeExists([candidate]) });
  assert.equal(r.found, true);
  assert.equal(r.command, candidate);
  assert.equal(r.source, 'install-location');
});

test('macOS: PATH command takes precedence over an application bundle', () => {
  const env = { HOME: '/Users/tester', PATH: '/opt/homebrew/bin:/usr/bin' };
  const codium = path.join('/opt/homebrew/bin', 'codium');
  const bundle = macCandidates(env)[0];
  const r = resolveEditor({ platform: 'darwin', env, existsSync: fakeExists([codium, bundle]) });
  assert.equal(r.command, codium);
  assert.equal(r.source, 'path');
});

test('macOS: reports not-found clearly instead of returning an unresolvable command', () => {
  const r = resolveEditor({
    platform: 'darwin',
    env: { HOME: '/Users/tester', PATH: '/usr/bin:/bin' },
    existsSync: fakeExists([]),
  });
  assert.equal(r.found, false);
  assert.match(r.hint, /VSCodium\/VS Code/);
  assert.match(r.hint, new RegExp(EDITOR_ENV));
  assert.ok(r.tried.some((candidate) => /Applications/.test(candidate)));
});

// ---- Windows ---------------------------------------------------------------

test('windows: finds VSCodium.exe in LOCALAPPDATA (no shell)', () => {
  const r = resolveEditor({ platform: 'win32', env: winEnv(), existsSync: fakeExists([vscodiumExe()]) });
  assert.equal(r.found, true);
  assert.equal(r.command, vscodiumExe());
  assert.equal(r.shell, false, '.exe launches without a shell (spaces-safe args array)');
  assert.equal(r.source, 'install-location');
});

test('windows: prefers VSCodium.exe over the codium.cmd shim', () => {
  const r = resolveEditor({ platform: 'win32', env: winEnv(), existsSync: fakeExists([vscodiumExe(), vscodiumCmd()]) });
  assert.equal(r.command, vscodiumExe());
});

test('windows: uses the codium.cmd shim (shell) when no .exe exists', () => {
  const r = resolveEditor({ platform: 'win32', env: winEnv(), existsSync: fakeExists([vscodiumCmd()]) });
  assert.equal(r.command, vscodiumCmd());
  assert.equal(r.shell, true, '.cmd must go through the shell');
});

test('windows: falls back to VS Code Code.exe', () => {
  const r = resolveEditor({ platform: 'win32', env: winEnv(), existsSync: fakeExists([vscodeExe()]) });
  assert.equal(r.command, vscodeExe());
});

test('windows: PATH shim as a last resort', () => {
  const shim = path.join('/win/tools', 'codium.cmd');
  const r = resolveEditor({ platform: 'win32', env: winEnv(), existsSync: fakeExists([shim]) });
  assert.equal(r.command, shim);
  assert.equal(r.shell, true);
  assert.equal(r.source, 'path');
});

test('windows: not-found returns a clear hint and what was tried', () => {
  const r = resolveEditor({ platform: 'win32', env: winEnv(), existsSync: fakeExists([]) });
  assert.equal(r.found, false);
  assert.match(r.hint, new RegExp(EDITOR_ENV));
  assert.match(r.hint, /VSCodium/);
  assert.ok(r.tried.length >= 4, 'reports the candidate locations it checked');
  assert.ok(r.tried.includes(vscodiumExe()));
});

// ---- override --------------------------------------------------------------

test('override: absolute .exe path is used (no shell)', () => {
  const custom = path.join('/custom', 'MyEditor.exe');
  const r = resolveEditor({ platform: 'win32', env: winEnv(), override: custom, existsSync: fakeExists([custom]) });
  assert.equal(r.command, custom);
  assert.equal(r.source, 'override');
  assert.equal(r.shell, false);
});

test('override: absolute .cmd path uses the shell on windows', () => {
  const custom = path.join('/custom', 'ed.cmd');
  const r = resolveEditor({ platform: 'win32', env: winEnv(), override: custom, existsSync: fakeExists([custom]) });
  assert.equal(r.shell, true);
});

test('override via WRL_FORGE_EDITOR env, resolved on PATH', () => {
  const bin = path.join('/opt/ed', 'myed');
  const r = resolveEditor({ platform: 'linux', env: { PATH: '/opt/ed', [EDITOR_ENV]: 'myed' }, existsSync: fakeExists([bin]) });
  assert.equal(r.command, bin);
  assert.equal(r.source, 'override');
});

test('override: a bare command is trusted on linux even if PATH probe misses', () => {
  const r = resolveEditor({ platform: 'linux', env: { PATH: '/x', [EDITOR_ENV]: 'my-editor' }, existsSync: fakeExists([]) });
  assert.equal(r.found, true);
  assert.equal(r.command, 'my-editor');
});

test('override: an unresolvable override on windows falls through to not-found', () => {
  const r = resolveEditor({ platform: 'win32', env: winEnv({ [EDITOR_ENV]: 'ghost.exe' }), existsSync: fakeExists([]) });
  assert.equal(r.found, false);
  assert.ok(r.tried.some((t) => /ghost\.exe/.test(t)));
});

// ---- buildLaunch: spaces / non-ASCII safety --------------------------------

test('buildLaunch: .exe passes the file as a plain argv entry (spaces safe, no shell)', () => {
  const file = '/win/My Worlds/naïve scene.edit.wrl';
  const spec = buildLaunch({ command: vscodiumExe(), shell: false }, file);
  assert.equal(spec.options.shell, false);
  assert.deepEqual(spec.args, [file], 'file is one argv entry — Node quotes it for the child');
  assert.equal(spec.command, vscodiumExe());
});

test('buildLaunch: .cmd shim passes the file as a plain argv entry (spaces safe, no shell)', () => {
  const cmd = vscodiumCmd();
  const file = '/win/My Worlds/naïve scene.edit.wrl';
  const spec = buildLaunch({ command: cmd, shell: true }, file);
  assert.equal(spec.options.shell, false);
  assert.deepEqual(spec.args, [file]);
  assert.equal(spec.command, cmd);
});

test('isCmdShim recognizes .cmd/.bat, not .exe', () => {
  assert.equal(isCmdShim('a/b/codium.cmd'), true);
  assert.equal(isCmdShim('a/b/run.BAT'), true);
  assert.equal(isCmdShim('a/b/VSCodium.exe'), false);
});
