'use strict';
// Pure tests for the preview URL policy: the network-layer block predicate and
// the preflight remote-URL scan. These enforce "no arbitrary HTTP/HTTPS resource
// loading" independently of Electron -- the same predicate is wired into
// session.webRequest in main.js, and an Electron test (electron-preview.test.js)
// confirms it actually cancels remote requests at runtime.

const test = require('node:test');
const assert = require('node:assert/strict');
const { isBlockedPreviewUrl, scanRemoteUrls, schemeOf, ALLOWED_SCHEMES } = require('../../src/preview/url-policy');

test('local schemes are allowed (not blocked)', () => {
  for (const u of [
    'file:///home/u/item/stone.png',
    'data:image/png;base64,AAAA',
    'blob:abc-123',
    'stone.png',              // relative -> resolved against the file:// base
    './tex/wood.png',
    'tex/wood.png',
  ]) {
    assert.equal(isBlockedPreviewUrl(u), false, `${u} should be allowed`);
  }
});

test('isBlockedPreviewUrl is false for every allowed scheme', () => {
  assert.equal(isBlockedPreviewUrl('about:blank'), false);
  assert.equal(isBlockedPreviewUrl('wrlworld://w/a.png'), false);
  assert.equal(isBlockedPreviewUrl('chrome-extension://x/y'), false);
  assert.equal(isBlockedPreviewUrl('devtools://d'), false);

  for (const scheme of ALLOWED_SCHEMES) {
    assert.equal(isBlockedPreviewUrl(`${scheme}://test`), false);
  }
});

test('isBlockedPreviewUrl is true for javascript, mailto, padded HTTPS, and Windows drive paths', () => {
  assert.equal(isBlockedPreviewUrl('javascript:alert(1)'), true);
  assert.equal(isBlockedPreviewUrl('mailto:a@b'), true);
  assert.equal(isBlockedPreviewUrl('  HTTPS://x  '), true);
  assert.equal(isBlockedPreviewUrl('C:/x.png'), true);
});

test('explicit HTTP URLs are blocked', () => {
  assert.equal(isBlockedPreviewUrl('http://example.com/texture.png'), true);
  assert.equal(isBlockedPreviewUrl('HTTP://EXAMPLE.COM/x.PNG'), true);
});

test('explicit HTTPS URLs are blocked', () => {
  assert.equal(isBlockedPreviewUrl('https://example.com/texture.png'), true);
  assert.equal(isBlockedPreviewUrl('https://cdn.evil/x.js'), true);
});

test('protocol-relative and other network schemes are blocked', () => {
  assert.equal(isBlockedPreviewUrl('//example.com/texture.png'), true);
  assert.equal(isBlockedPreviewUrl('ftp://host/file'), true);
  assert.equal(isBlockedPreviewUrl('ws://host/socket'), true);
  assert.equal(isBlockedPreviewUrl('wss://host/socket'), true);
});

test('empty / nullish input is not treated as a blocked request', () => {
  assert.equal(isBlockedPreviewUrl(''), false);
  assert.equal(isBlockedPreviewUrl(null), false);
  assert.equal(isBlockedPreviewUrl(undefined), false);
});

test('schemeOf parses the leading scheme and ignores protocol-relative', () => {
  assert.equal(schemeOf('https://x/y'), 'https');
  assert.equal(schemeOf('file:///a'), 'file');
  assert.equal(schemeOf('//host/x'), null);
  assert.equal(schemeOf('relative/path.png'), null);
});

test('scanRemoteUrls finds ws/wss, trims/deduplicates, ignores file/mailto, and handles null', () => {
  const text = `#VRML V2.0 utf8
    Appearance { texture ImageTexture { url "ws://host/a" } }
    Appearance { texture ImageTexture { url [ "  https://h/e " ] } }
    Appearance { texture ImageTexture { url "https://h/e" } }
    Appearance { texture ImageTexture { url "WSS://host/b" } }
    Appearance { texture ImageTexture { url "file:///local" } }
    Appearance { texture ImageTexture { url "mailto:a@b" } }`;

  const remote = scanRemoteUrls(text);
  assert.deepEqual(remote.sort(), [
    'https://h/e',
    'ws://host/a',
    'WSS://host/b',
  ].sort());

  assert.deepEqual(scanRemoteUrls(null), []);
});

test('scanRemoteUrls finds http/https/protocol-relative references in url fields', () => {
  const text = `#VRML V2.0 utf8
    Appearance { texture ImageTexture { url "http://example.com/a.png" } }
    Appearance { texture ImageTexture { url [ "https://cdn.test/b.jpg" ] } }
    Appearance { texture ImageTexture { url "//proto.rel/c.gif" } }
    Appearance { texture ImageTexture { url "local.png" } }`;
  const remote = scanRemoteUrls(text);
  assert.deepEqual(remote.sort(), [
    '//proto.rel/c.gif',
    'http://example.com/a.png',
    'https://cdn.test/b.jpg',
  ]);
});

test('scanRemoteUrls known limitation: only the first string in an array is scanned', () => {
  // Known limitation: the advisory scan only reads the FIRST string in the array.
  // The network gate isBlockedPreviewUrl still enforces every actual request.
  const text = `#VRML V2.0 utf8
    Appearance { texture ImageTexture { url [ "a.png" "https://h/x" ] } }`;

  assert.deepEqual(scanRemoteUrls(text), []);
});

test('scanRemoteUrls returns empty for a purely local item', () => {
  const text = `#VRML V2.0 utf8
    Appearance { texture ImageTexture { url "stone.png" } }
    Appearance { texture ImageTexture { url [ "./tex/wood.png" ] } }`;
  assert.deepEqual(scanRemoteUrls(text), []);
});
