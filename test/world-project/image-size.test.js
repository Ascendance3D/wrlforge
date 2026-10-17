'use strict';
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const { imageSize } = require('../../src/world-project/image-size');

test('parses a real committed PNG header', () => {
  const buf = fs.readFileSync(path.resolve(__dirname, '../fixtures/world/gz/img/floor.png'));
  assert.deepEqual(imageSize(buf.subarray(0, 64)), { width: 16, height: 16 });
});

test('parses a GIF89a logical screen size', () => {
  const gif = Buffer.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x0a, 0x00, 0x14, 0x00]);
  assert.deepEqual(imageSize(gif), { width: 10, height: 20 });
});

test('parses a BMP BITMAPINFOHEADER size', () => {
  const bmp = Buffer.alloc(30);
  bmp.write('BM', 0, 'ascii');
  bmp.writeInt32LE(64, 18);
  bmp.writeInt32LE(-48, 22); // top-down bitmaps carry a negative height
  assert.deepEqual(imageSize(bmp), { width: 64, height: 48 });
});

test('parses a JPEG SOF0 frame size', () => {
  // SOI, then a SOF0 (ffc0) segment: len=17, precision=8, height=24, width=32.
  const jpeg = Buffer.concat([
    Buffer.from([0xff, 0xd8]),
    Buffer.from([0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x18, 0x00, 0x20, 0x03]),
    Buffer.alloc(8),
  ]);
  assert.deepEqual(imageSize(jpeg), { width: 32, height: 24 });
});

test('returns null for non-image / truncated data', () => {
  assert.equal(imageSize(Buffer.from('not an image')), null);
  assert.equal(imageSize(Buffer.alloc(4)), null);
  assert.equal(imageSize(null), null);
});

test('does not decode gzip (a gzipped png header is not a png)', () => {
  const gz = zlib.gzipSync(Buffer.from([0x89, 0x50, 0x4e, 0x47]));
  assert.equal(imageSize(gz.subarray(0, 32)), null);
});

test('PNG returns null if IHDR is missing, valid size for IHDR, and null if truncated under 24 bytes', () => {
  const noIhdr = Buffer.alloc(24);
  noIhdr.writeUInt32BE(0x89504e47, 0);
  noIhdr.write('IXXX', 12, 'ascii');
  assert.equal(imageSize(noIhdr), null);

  const validIhdr = Buffer.alloc(24);
  validIhdr.writeUInt32BE(0x89504e47, 0);
  validIhdr.write('IHDR', 12, 'ascii');
  validIhdr.writeUInt32BE(300, 16);
  validIhdr.writeUInt32BE(200, 20);
  assert.deepEqual(imageSize(validIhdr), { width: 300, height: 200 });

  assert.equal(imageSize(validIhdr.subarray(0, 23)), null);
});

test('JPEG segment walking skips APP0 and DHT to find SOF2 size', () => {
  const app0 = Buffer.concat([Buffer.from([0xff, 0xe0, 0x00, 0x10]), Buffer.alloc(14)]);
  const dht = Buffer.concat([Buffer.from([0xff, 0xc4, 0x00, 0x04]), Buffer.alloc(2)]);
  const sof2 = Buffer.concat([Buffer.from([0xff, 0xc2, 0x00, 0x11, 0x08, 0x01, 0x2c, 0x01, 0x90, 0x03]), Buffer.alloc(8)]);

  const jpeg = Buffer.concat([Buffer.from([0xff, 0xd8]), app0, dht, sof2]);

  assert.deepEqual(imageSize(jpeg), { height: 300, width: 400 });
});

test('JPEG returns null for invalid segment lengths and truncated scans', () => {
  const invalidLen = Buffer.concat([Buffer.from([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x01]), Buffer.alloc(20)]);
  assert.equal(imageSize(invalidLen), null);

  const dhtZeros = Buffer.concat([Buffer.from([0xff, 0xd8, 0xff, 0xc4, 0x00, 0x04, 0x00, 0x00]), Buffer.alloc(20)]);
  assert.equal(imageSize(dhtZeros), null);
});

test('imageSize handles fill bytes, GIF87a, invalid GIFs, short BMPs, and buffers under 8 bytes', () => {
  const fillBytesJpeg = Buffer.concat([Buffer.from([0xff, 0xd8, 0xff, 0xff, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x00, 0x18, 0x00, 0x20, 0x03]), Buffer.alloc(8)]);
  assert.deepEqual(imageSize(fillBytesJpeg), { height: 24, width: 32 });

  const gif87a = Buffer.concat([Buffer.from('GIF87a'), Buffer.from([0x05, 0x00, 0x06, 0x00])]);
  assert.deepEqual(imageSize(gif87a), { width: 5, height: 6 });

  const gif90a = Buffer.concat([Buffer.from('GIF90a'), Buffer.from([0x05, 0x00, 0x06, 0x00])]);
  assert.equal(imageSize(gif90a), null);

  const shortBmp = Buffer.alloc(25);
  shortBmp.write('BM', 0, 'ascii');
  assert.equal(imageSize(shortBmp), null);

  assert.equal(imageSize(Buffer.alloc(7)), null);
});
