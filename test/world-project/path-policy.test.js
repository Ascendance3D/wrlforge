'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('path');
const { classifyReference, CATEGORY, toPosix, RESOLVABLE, REMOTE } = require('../../src/world-project/path-policy');

const ROOT = path.resolve('/proj');
const DIR = path.resolve('/proj/vrml');

test('a clean relative path within the project is local + resolvable', () => {
  const c = classifyReference('img/wall.jpg', DIR, ROOT);
  assert.equal(c.category, CATEGORY.LOCAL);
  assert.equal(c.resolvable, true);
  assert.equal(c.remote, false);
  assert.equal(c.resolved, path.resolve('/proj/vrml/img/wall.jpg'));
  assert.equal(c.projectRelative, 'vrml/img/wall.jpg');
});

test('a nested-directory relative path stays local', () => {
  const c = classifyReference('sub/deep/leaf.png', DIR, ROOT);
  assert.equal(c.category, CATEGORY.LOCAL);
  assert.equal(c.projectRelative, 'vrml/sub/deep/leaf.png');
});

test('.. that stays inside the root is still local', () => {
  const c = classifyReference('../shared/x.png', DIR, ROOT);
  assert.equal(c.category, CATEGORY.LOCAL);
  assert.equal(c.projectRelative, 'shared/x.png');
});

test('.. that escapes the root is a traversal (unsafe, not resolvable)', () => {
  const c = classifyReference('../../etc/passwd', DIR, ROOT);
  assert.equal(c.category, CATEGORY.TRAVERSAL);
  assert.equal(c.resolvable, false);
});

test('an absolute POSIX path is absolute-local (unsafe)', () => {
  const c = classifyReference('/etc/hosts', DIR, ROOT);
  assert.equal(c.category, CATEGORY.ABSOLUTE);
  assert.equal(c.resolvable, false);
});

test('a Windows drive path is absolute-local', () => {
  const c = classifyReference('C:\\textures\\x.png', DIR, ROOT);
  assert.equal(c.category, CATEGORY.ABSOLUTE);
});

test('http/https is remote-http', () => {
  assert.equal(classifyReference('http://x/y.png', DIR, ROOT).category, CATEGORY.REMOTE_HTTP);
  assert.equal(classifyReference('https://x/y.png', DIR, ROOT).category, CATEGORY.REMOTE_HTTP);
});

test('protocol-relative is remote-protocol', () => {
  const c = classifyReference('//cdn/y.png', DIR, ROOT);
  assert.equal(c.category, CATEGORY.REMOTE_PROTOCOL);
  assert.equal(c.remote, true);
});

test('other network schemes are remote-other', () => {
  assert.equal(classifyReference('ftp://h/z.wav', DIR, ROOT).category, CATEGORY.REMOTE_OTHER);
  assert.equal(classifyReference('ws://h/s', DIR, ROOT).category, CATEGORY.REMOTE_OTHER);
});

test('inline VRML/JS script pseudo-schemes are inline-script, not assets', () => {
  assert.equal(classifyReference('vrmlscript: function f(){}', DIR, ROOT).category, CATEGORY.INLINE_SCRIPT);
  assert.equal(classifyReference('javascript: g()', DIR, ROOT).category, CATEGORY.INLINE_SCRIPT);
});

test('empty / whitespace is malformed', () => {
  assert.equal(classifyReference('   ', DIR, ROOT).category, CATEGORY.MALFORMED);
});

test('file:// is treated as absolute-local', () => {
  const c = classifyReference('file:///proj/vrml/img/a.png', DIR, ROOT);
  assert.equal(c.category, CATEGORY.ABSOLUTE);
});

test('no project root leaves projectRelative as null', () => {
  const c = classifyReference('x.png', DIR, null);
  assert.equal(c.category, CATEGORY.LOCAL);
  assert.equal(c.resolvable, true);
  assert.equal(c.projectRelative, null);
  assert.equal(c.resolved, path.resolve('/proj/vrml/x.png'));
});

test('absolute references resolve projectRelative when inside root', () => {
  const c1 = classifyReference('file:///proj/vrml/img/a.png', DIR, ROOT);
  assert.equal(c1.category, CATEGORY.ABSOLUTE);
  assert.equal(c1.projectRelative, 'vrml/img/a.png');

  const c2 = classifyReference('file:///proj/vrml/img/a.png', DIR, null);
  assert.equal(c2.category, CATEGORY.ABSOLUTE);
  assert.equal(c2.projectRelative, null);

  const c3 = classifyReference('/proj/a/b.png', DIR, ROOT);
  assert.equal(c3.category, CATEGORY.ABSOLUTE);
  assert.equal(c3.projectRelative, 'a/b.png');

  const c4 = classifyReference('/outside/x.png', DIR, ROOT);
  assert.equal(c4.category, CATEGORY.ABSOLUTE);
  assert.equal(c4.projectRelative, null);

  const c5 = classifyReference('file:a.png', DIR, ROOT);
  assert.equal(c5.category, CATEGORY.ABSOLUTE);
});

test('normalization and edge cases', () => {
  const c1 = classifyReference('sub\\\\x.png', DIR, ROOT);
  assert.equal(c1.category, CATEGORY.LOCAL);
  assert.equal(c1.projectRelative, 'vrml/sub/x.png');

  const c2 = classifyReference('  x.png  ', DIR, ROOT);
  assert.equal(c2.category, CATEGORY.LOCAL);
  assert.equal(c2.projectRelative, 'vrml/x.png');

  const c3 = classifyReference('..', DIR, ROOT);
  assert.equal(c3.category, CATEGORY.LOCAL);
  assert.equal(c3.projectRelative, '');

  const c4 = classifyReference(null, DIR, ROOT);
  assert.equal(c4.category, CATEGORY.MALFORMED);
});

test('schemes and exports', () => {
  const c1 = classifyReference('mailto:a@b', DIR, ROOT);
  assert.equal(c1.category, CATEGORY.REMOTE_OTHER);
  assert.equal(c1.note, 'mailto: remote URL');

  const c2 = classifyReference('HTTP://H/x', DIR, ROOT);
  assert.equal(c2.category, CATEGORY.REMOTE_HTTP);
  assert.equal(c2.note, 'http URL');

  assert.equal(toPosix('a\\b'), 'a/b');
  assert.equal(RESOLVABLE.has('local'), true);

  assert.deepEqual(Array.from(REMOTE).sort(), [CATEGORY.REMOTE_HTTP, CATEGORY.REMOTE_PROTOCOL, CATEGORY.REMOTE_OTHER].sort());
});
