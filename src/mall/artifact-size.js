'use strict';
// Mall upload-artifact measurement (Lane A: artifact size truth).
//
// This is the file-truth half of the Mall size contract. `validator.js` stays
// filesystem-free and only evaluates facts; this module is the ONLY place that
// weighs a real `.wrl` on disk and proves whether it still represents the text
// the user is looking at. See docs/MALL_SIZE_CONTRACT.md.
//
// Why proving identity matters: the shipping artifact and the editor buffer
// drift apart the moment somebody types. A stale 72,820-byte file says nothing
// about the size of an edit that has never been packed, so this module reports
// the mismatch instead of letting the old byte count stand in for the new one.
//
// Only a GZIP file counts as a Mall upload artifact. A plain `.wrl` on disk is
// the editable source; when it matches the buffer its length is already
// reported as `textBytes`, so reporting it again as an "upload artifact" would
// invent an upload that has not been packed.

const fsDefault = require('fs');
const zlibDefault = require('zlib');
const { isGzip, getGzipCap } = require('../files/vrml-file');

// Nothing on disk that could be weighed. Every field is null rather than 0 or
// false: "no artifact" is not "an artifact of zero bytes".
const NO_ARTIFACT = Object.freeze({
  artifactBytes: null,
  artifactIsGzip: null,
  artifactMatchesText: null,
});

// A plain file is present but is not an upload artifact. `artifactIsGzip` is
// false (we looked, and it is not gzip); the byte count stays null because no
// upload artifact exists to weigh.
const PLAIN_ARTIFACT = Object.freeze({
  artifactBytes: null,
  artifactIsGzip: false,
  artifactMatchesText: null,
});

// Measure the artifact at `filePath` and prove whether it decompresses to
// `text` byte-for-byte.
//
// Returns a `sizeContext` suitable for `validate(text, sizeContext)`:
//   { artifactBytes, artifactIsGzip, artifactMatchesText }
//
// Never throws: an unreadable or corrupt file degrades to an honest "unknown"
// rather than taking down a validation run.
//
// deps (injectable for tests): { fs, zlib, isGzip }
function measureArtifact(filePath, text, deps = {}) {
  const fs = deps.fs || fsDefault;
  const zlib = deps.zlib || zlibDefault;
  const gzipCheck = deps.isGzip || isGzip;

  if (!filePath) return { ...NO_ARTIFACT };

  let raw;
  try {
    if (!fs.existsSync(filePath)) return { ...NO_ARTIFACT };
    raw = fs.readFileSync(filePath);
  } catch {
    return { ...NO_ARTIFACT };
  }

  if (!gzipCheck(raw)) return { ...PLAIN_ARTIFACT };

  // The artifact IS a real upload artifact -- weigh the bytes that exist, not a
  // recompression of what they contain.
  let inflated = null;
  try {
    const getCap = deps.getGzipCap || getGzipCap;
    const cap = getCap();
    inflated = zlib.gunzipSync(raw, { maxOutputLength: cap });
  } catch {
    // Gzip magic bytes but a broken stream or over limit: the size is real, the identity is
    // unprovable. Report both facts honestly.
    return { artifactBytes: raw.length, artifactIsGzip: true, artifactMatchesText: null };
  }

  return {
    artifactBytes: raw.length,
    artifactIsGzip: true,
    artifactMatchesText: Buffer.compare(inflated, Buffer.from(text, 'utf8')) === 0,
  };
}

// Assemble a Mall payload without letting validator output overwrite measured
// file facts (or the reverse).
//
// This exists because the defect it guards against already shipped: `main.js`
// spread `...validate(text)` over a payload that had just set the real artifact
// byte count, and the validator's text-derived number silently won. Building
// the payload in one audited place -- with an explicit collision check -- means
// a future field rename cannot quietly reintroduce that clobber.
function mallPayload(base, validation) {
  const collisions = Object.keys(base).filter((k) => Object.hasOwn(validation, k));
  if (collisions.length) {
    throw new Error(
      `mallPayload: validator result would overwrite measured file facts: ${collisions.join(', ')}`
    );
  }
  return { ...base, ...validation };
}

module.exports = { measureArtifact, mallPayload, NO_ARTIFACT, PLAIN_ARTIFACT };
