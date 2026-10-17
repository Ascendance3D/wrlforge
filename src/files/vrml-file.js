'use strict';
const path = require('path');

const GZIP_MAGIC = Buffer.from([0x1f, 0x8b]);

function isGzip(buf) {
  return buf.length >= 2 && buf[0] === GZIP_MAGIC[0] && buf[1] === GZIP_MAGIC[1];
}

// Plain-text working copy lives next to the mall .wrl as "<name>.edit.wrl",
// so VSCodium's existing X3D/VRML extensions (syntax highlighting + live
// preview) can open it directly -- they can't render/edit inside a gzip file.
function editPathFor(mallPath) {
  const dir = path.dirname(mallPath);
  const base = path.basename(mallPath, path.extname(mallPath));
  return path.join(dir, `${base}.edit.wrl`);
}

// Shared limits for decompression to prevent gzip bombs.
// 64 MiB is comfortably above any legitimate text .wrl we open (largest known is ~1.3MB),
// and the 250x ratio ensures even tiny hostile compressed inputs cannot expand unbounded.
// 250x was chosen because valid limit-twin-small.wrl.gz expands from 589 bytes to 120,030 bytes (~204x).
const MAX_DECODED_BYTES = 64 * 1024 * 1024;
const MAX_EXPANSION_RATIO = 250;

// Returns the bounded maximum output length for gunzipSync.
function getGzipCap(rawLength) {
  const ratioCap = Math.floor(rawLength * MAX_EXPANSION_RATIO);
  return Math.min(MAX_DECODED_BYTES, ratioCap);
}

module.exports = { GZIP_MAGIC, isGzip, editPathFor, getGzipCap, MAX_DECODED_BYTES, MAX_EXPANSION_RATIO };
