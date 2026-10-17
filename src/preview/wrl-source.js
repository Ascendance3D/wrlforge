'use strict';
// Main-process WRL source loader for the spike. Reads a .wrl from disk in the
// trusted Node/main process, detects gzip via the PRODUCTION magic-byte helper
// (reused, not re-implemented), decompresses when needed, and returns plain
// UTF-8 text. X_ITE only ever receives decompressed text -- it is never asked
// to fetch or parse gzip bytes itself (Phase 2B0 Action Item 3).
//
// Read-only: this module opens files for reading and never writes. It has no
// path-resolution policy of its own -- callers pass an already-validated
// absolute path (see texture-base.safeResolve in main.js).

const fs = require('fs');
const zlib = require('zlib');
// Reuse the trusted gzip detection from the production file layer rather than
// duplicating the magic-byte check here.
const { isGzip, getGzipCap } = require('../files/vrml-file');

// Returns { text, wasGzipped, rawBytes }. Throws a clear, prefixed error if a
// file that looks gzipped fails to inflate (truncated/corrupt archive).
function readWrlSource(filePath, deps = {}) {
  const fsTarget = deps.fs || fs;
  const raw = fsTarget.readFileSync(filePath);
  const wasGzipped = isGzip(raw);
  let text;
  if (wasGzipped) {
    try {
      const getCap = deps.getGzipCap || getGzipCap;
      const cap = getCap();
      text = zlib.gunzipSync(raw, { maxOutputLength: cap }).toString('utf8');
    } catch (err) {
      if (err && (err.code === 'ERR_BUFFER_TOO_LARGE' || err.code === 'ERR_STRING_TOO_LONG')) {
        throw new Error(`WRL source at ${filePath} exceeds the allowed decompression limit.`);
      }
      throw new Error(`WRL source at ${filePath} has gzip magic bytes but failed to decompress: ${err.message}`);
    }
  } else {
    try {
      text = raw.toString('utf8');
    } catch (err) {
      if (err && err.code === 'ERR_STRING_TOO_LONG') {
        throw new Error(`WRL source at ${filePath} exceeds the allowed decompression limit.`);
      }
      throw err;
    }
  }
  return { text, wasGzipped, rawBytes: raw.length };
}

module.exports = { readWrlSource };
