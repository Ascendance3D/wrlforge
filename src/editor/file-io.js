'use strict';
// Native editor file I/O (Phase 7B) -- the ONLY fs-touching part of the editor
// lane. Conservative, backup-first, verify-before-commit saving plus
// external-change detection. Every fs/zlib/crypto/clock touch goes through an
// injectable `deps` bag so the interrupted-write, corrupt-write, and
// conflict-detection paths are exercised by unit tests without real disk faults.
//
// Reuses the production primitives rather than re-deriving them:
//   * isGzip, getGzipCap (magic bytes, cap) <- src/files/vrml-file.js
//   * readWrlSource (gzip-transparent load) <- src/preview/wrl-source.js
//   * backupPath (timestamped, collision-free naming) <- src/files/backups.js
//
// Format is round-tripped exactly: a gzip source saves back as gzip, a plain
// source as plain. A gzip file is NEVER silently rewritten as plain.
//
// GZIP PRESERVATION (Lane B, B1). A gzip `.wrl` on disk may have been packed by
// a stronger encoder than Node's zlib -- the real Cybertown corpus holds
// Zopfli-packed items. Re-encoding one with `gzipSync(level 9)` when its
// decompressed text has NOT changed destroys a better artifact for no reason
// (measured on a shipping item: 72,820 B -> 87,366 B). `safeSave` therefore
// supports an OPT-IN true no-op: when the destination already IS the exact gzip
// artifact the requested text needs, nothing is written at all.
//
// Three properties make that safe, and each is load-bearing:
//   * OPT-IN. `preserveExistingGzip` defaults to false, so Save As -- which also
//     calls safeSave -- keeps its current write behaviour untouched.
//   * CONFLICT FIRST. The external-change guard runs BEFORE the preservation
//     test, so a file that changed underneath us still raises EEXTERNAL even if
//     its new contents happen to decode to the same text.
//   * FAIL CLOSED. Identity must be PROVEN by an exact decompressed-text
//     comparison. Anything unprovable -- missing, plain, corrupt, unreadable or
//     simply different -- falls through to the normal encode-and-write path.
//
// PRE-WRITE CANDIDATE GUARDS (Lane B, B2). Two further OPT-IN options let a
// caller prove things about the encoded bytes BEFORE the destination is touched
// at all -- no temp, no backup, no rename:
//   * `verifyCandidate` -- the encoded buffer must decode back to exactly the
//     requested text, or ESAVE/EVERIFY is raised with nothing written. This does
//     NOT replace the existing post-temp read-back; the first check proves the
//     ENCODER, the second proves the BYTES THAT REACHED THE DISK.
//   * `maxBytes` -- an exact ceiling on the encoded candidate's length. Over it,
//     ESIZE is raised before any mutation and the destination keeps its bytes.
//
// Both default to off, so every existing caller is byte-for-byte unchanged.
// `maxBytes` implies `verifyCandidate`: refusing a candidate on its size is only
// meaningful once that candidate is known to be the right document.
//
// This module stays PROFILE-NEUTRAL. `maxBytes` is a number the caller supplies;
// file-io does not know what a Mall upload limit is and never imports
// validator.js. The Mall's 81,920 B ceiling is passed in by src/mall/repack.js.

const nodeFs = require('fs');
const nodeZlib = require('zlib');
const crypto = require('crypto');
const { isGzip, getGzipCap } = require('../files/vrml-file');
const { readWrlSource } = require('../preview/wrl-source');
const { backupPath } = require('../files/backups');
const { FORMAT, formatFromGzip } = require('./wrl-document');

// Default dependency bag: the real platform. Tests pass a partial override.
function resolveDeps(deps) {
  return {
    fs: (deps && deps.fs) || nodeFs,
    zlib: (deps && deps.zlib) || nodeZlib,
    hash: (deps && deps.hash) || defaultHash,
    now: (deps && deps.now) || (() => new Date()),
    readSource: (deps && deps.readSource) || readWrlSource,
    getGzipCap: (deps && deps.getGzipCap) || getGzipCap,
  };
}

function defaultHash(buf) {
  return crypto.createHash('sha1').update(buf).digest('hex');
}

// --- state stamp for external-change detection -------------------------------
// A cheap size+mtime hint plus an authoritative content hash. Hash is the
// tiebreaker (mtime granularity/preservation varies across filesystems and
// tools), so a file that was rewritten with identical bytes is NOT a conflict,
// and a same-size edit still is.
function statFile(filePath, deps) {
  const d = resolveDeps(deps);
  const st = d.fs.statSync(filePath);
  const raw = d.fs.readFileSync(filePath);
  return { mtimeMs: st.mtimeMs, size: st.size, hash: d.hash(raw) };
}

// Compare a previously captured stamp to the file on disk NOW. Returns
// { changed, reason, current }. A vanished file counts as changed.
function detectExternalChange(prevStat, filePath, deps) {
  const d = resolveDeps(deps);
  if (!d.fs.existsSync(filePath)) {
    return { changed: true, reason: 'deleted', current: null };
  }
  const current = statFile(filePath, deps);
  if (!prevStat) return { changed: false, reason: 'no-baseline', current };
  if (current.hash !== prevStat.hash) {
    const reason = current.size !== prevStat.size ? 'size' : 'content';
    return { changed: true, reason, current };
  }
  return { changed: false, reason: 'unchanged', current };
}

// --- load --------------------------------------------------------------------
// Open a WRL into a plain-text buffer, transparently decompressing gzip. Returns
// { sourcePath, text, format, stat } ready for wrl-document.createDocument.
function loadDocument(filePath, deps) {
  const d = resolveDeps(deps);
  const { text, wasGzipped } = d.readSource(filePath);
  return {
    sourcePath: filePath,
    text,
    format: formatFromGzip(wasGzipped),
    stat: statFile(filePath, deps),
  };
}

// --- encode / decode ---------------------------------------------------------
function encodeForSave(text, format, deps) {
  const d = resolveDeps(deps);
  const utf8 = Buffer.from(text, 'utf8');
  if (format === FORMAT.GZIP) return d.zlib.gzipSync(utf8, { level: 9 });
  return utf8;
}

// Decode bytes back to text per format, using magic bytes as the ground truth so
// verification catches a format mismatch (e.g. a save that failed to compress).
function decodeBytes(bytes, format, deps) {
  const d = resolveDeps(deps);
  if (format === FORMAT.GZIP) {
    if (!isGzip(bytes)) throw new Error('expected gzip bytes but magic bytes are absent');
    try {
      const cap = d.getGzipCap();
      return d.zlib.gunzipSync(bytes, { maxOutputLength: cap }).toString('utf8');
    } catch (err) {
      if (err && (err.code === 'ERR_BUFFER_TOO_LARGE' || err.code === 'ERR_STRING_TOO_LONG')) {
        throw new Error('decoding candidate exceeded the allowed decompression limit');
      }
      throw err;
    }
  }
  try {
    return bytes.toString('utf8');
  } catch (err) {
    if (err && err.code === 'ERR_STRING_TOO_LONG') {
      throw new Error('decoding candidate exceeds the maximum text size');
    }
    throw err;
  }
}

// --- gzip preservation (Lane B, B1) ------------------------------------------
// Is the file at `filePath` ALREADY the exact gzip artifact that saving `text`
// in `format` would need? Pure predicate: it only reads, and it returns a
// boolean -- never a "probably".
//
// The bar is exact decompressed-text identity. Deliberately NOT sufficient, and
// each rejected for a concrete reason:
//   * file size          -- two valid gzip artifacts can share a byte length AND
//                           a decompressed text while their compressed bytes
//                           differ (twin-small.wrl.gz is 451 bytes; so is a
//                           copy of it with one gzip header byte changed).
//   * mtime              -- says nothing about contents.
//   * compressed length  -- same as size.
//   * gzip header        -- encoder metadata, not payload.
//   * a hash of the decompressed text alone -- a hash comparison is only as
//                           good as what it is compared against; we have both
//                           strings in hand, so compare them directly.
//
// Every failure mode answers `false`, including a read or inflate that throws:
// preservation is an optimization of writes, never an error bypass.
function wouldPreserve({ filePath, text, format }, deps) {
  const d = resolveDeps(deps);
  if (format !== FORMAT.GZIP) return false;
  if (typeof text !== 'string') return false;
  try {
    if (!d.fs.existsSync(filePath)) return false;
    const raw = d.fs.readFileSync(filePath);
    if (!isGzip(raw)) return false;
    const cap = d.getGzipCap();
    return d.zlib.gunzipSync(raw, { maxOutputLength: cap }).toString('utf8') === text;
  } catch {
    // Unreadable, truncated, corrupt or over limit: identity is unprovable, so it is false.
    return false;
  }
}

// --- safe save ---------------------------------------------------------------
// Conservative save that never leaves the destination in a half-written state:
//   2. refuse if the destination changed under us (unless allowOverwrite)
//  2a. OPT-IN: if the destination already IS the needed gzip artifact, no-op
//   1. encode text (gzip when the target format is gzip)
//  1a. OPT-IN: verify the in-memory candidate decodes back to exactly `text`
//  1b. OPT-IN: refuse (ESIZE) a candidate longer than `maxBytes`
//   3. write a temp sibling, flushing to disk (fsync) and closing it
//   4. VERIFY the temp reopens and decodes back to exactly `text`
//   5. back up the prior file to a timestamped, collision-free name
//   6. atomically rename the verified temp over the destination
//   7. report success only after the verified swap
//
// The original is only ever replaced by an already-verified temp, so ANY failure
// (encode, write, fsync, verify) leaves the source file untouched and removes the
// temp -- the caller keeps the buffer dirty and shows the error. Returns
// { ok, preserved, bytesWritten, backup, stat, format }.
//
// `preserveExistingGzip` (default false) opts into step 2a. Only the native
// Save path sets it; Save As deliberately does not (see the header note).
//
// `verifyCandidate` (default false) opts into step 1a and `maxBytes` (default
// null = no ceiling) into step 1b; both are Lane B B2 and used by the Mall
// repack path. Steps 1a/1b run AFTER preservation on purpose: a save that needs
// no write has no candidate to judge, so an existing well-packed artifact can
// never be refused for the size a hypothetical re-encode would have had.
function safeSave({
  filePath, text, format, expectedStat, allowOverwrite, preserveExistingGzip,
  verifyCandidate = false, maxBytes = null,
}, deps) {
  const d = resolveDeps(deps);
  const fmt = format || FORMAT.PLAIN;

  // (2) conflict guard -- before touching anything on disk, and BEFORE the
  // preservation test. An externally-changed file must raise EEXTERNAL even
  // when its new contents happen to decode to the text we were about to save:
  // the user asked to be told, and "it already matches" is not an answer to
  // "somebody else edited this".
  if (!allowOverwrite && expectedStat && d.fs.existsSync(filePath)) {
    const change = detectExternalChange(expectedStat, filePath, deps);
    if (change.changed) {
      throw taggedError('EEXTERNAL',
        `The file changed on disk since it was opened (${change.reason}); refusing to overwrite.`,
        { current: change.current });
    }
  }

  // (2a) true no-op. Nothing is encoded, no temp is created, no backup is
  // taken (there is no overwrite to protect against -- owner policy) and the
  // destination's bytes and mtime are left exactly as they are. The returned
  // stat describes that untouched file, so the caller's conflict baseline
  // stays correct.
  if (preserveExistingGzip && wouldPreserve({ filePath, text, format: fmt }, deps)) {
    return { ok: true, preserved: true, bytesWritten: 0, backup: null, format: fmt, stat: statFile(filePath, deps) };
  }

  // (1) encode.
  const bytes = encodeForSave(text, fmt, deps);

  // A size ceiling is only meaningful once the candidate is known to be the
  // right document, so asking for one asks for the verification too.
  const wantVerify = verifyCandidate || maxBytes != null;

  // (1a) prove the ENCODER before anything on disk moves. The post-temp
  // read-back below still runs and proves a different thing -- the bytes that
  // actually reached the filesystem. Neither check substitutes for the other.
  if (wantVerify) {
    let decoded;
    try {
      decoded = decodeBytes(bytes, fmt, deps);
    } catch (err) {
      throw taggedError('EVERIFY',
        `Encoded candidate did not verify: ${err.message}`, { candidateBytes: bytes.length });
    }
    if (decoded !== text) {
      throw taggedError('EVERIFY',
        'Encoded candidate did not verify: it does not decode back to the buffer.',
        { candidateBytes: bytes.length });
    }
  }

  // (1b) size ceiling. Strictly greater-than: a candidate of exactly `maxBytes`
  // is inside the limit and must be allowed through.
  if (maxBytes != null && bytes.length > maxBytes) {
    throw taggedError('ESIZE',
      `Encoded candidate is ${bytes.length} B, over the ${maxBytes} B limit by ${bytes.length - maxBytes} B; nothing was written.`,
      { candidateBytes: bytes.length, maxBytes, overBytes: bytes.length - maxBytes });
  }

  const tmpPath = tempSiblingPath(filePath, d);
  try {
    // (3) write temp sibling with an explicit flush+close. Inside the try so a
    // failure mid-write (which may still have created the temp via openSync) is
    // cleaned up too -- the temp is never left presented as a completed save.
    writeAndFlush(tmpPath, bytes, d);

    // (4) verify the temp round-trips before we let it become the file.
    const back = d.fs.readFileSync(tmpPath);
    const decoded = decodeBytes(back, fmt, deps);
    if (decoded !== text) {
      throw taggedError('EVERIFY', 'Written file did not verify: decoded contents differ from the buffer.');
    }

    // (5) back up the prior file (timestamped -> never clobbers an unrelated
    // backup; that is the documented rotation rule).
    let backup = null;
    if (d.fs.existsSync(filePath)) {
      backup = backupPath(filePath, d.now);
      d.fs.copyFileSync(filePath, backup);
    }

    // (6) atomic replace.
    d.fs.renameSync(tmpPath, filePath);

    // (7) success -- stamp the freshly written file for future conflict checks.
    return { ok: true, preserved: false, bytesWritten: bytes.length, backup, format: fmt, stat: statFile(filePath, deps) };
  } catch (err) {
    // Never leave the temp presented as a completed save; original stays intact.
    safeUnlink(tmpPath, d);
    throw err;
  }
}

// Reload the on-disk contents into a fresh buffer, discarding buffer edits. Same
// shape as loadDocument; the caller resets baseline+buffer to this (see
// wrl-document.markSynced).
function reloadDocument(filePath, deps) {
  return loadDocument(filePath, deps);
}

// --- helpers -----------------------------------------------------------------
function tempSiblingPath(filePath, d) {
  // A hidden sibling in the same directory so the final rename is atomic (same
  // filesystem). The timestamp keeps concurrent saves of the same file distinct.
  const stamp = d.now().toISOString().replace(/[:.]/g, '-');
  return `${filePath}.wrlforge-tmp-${stamp}`;
}

function writeAndFlush(tmpPath, bytes, d) {
  const fd = d.fs.openSync(tmpPath, 'w');
  try {
    d.fs.writeSync(fd, bytes, 0, bytes.length, 0);
    d.fs.fsyncSync(fd);
  } finally {
    d.fs.closeSync(fd);
  }
}

function safeUnlink(p, d) {
  try {
    if (d.fs.existsSync(p)) d.fs.unlinkSync(p);
  } catch {
    // Best-effort cleanup; a leftover temp is inert and never shadows the source.
  }
}

function taggedError(code, message, extra) {
  const err = new Error(message);
  err.code = code;
  if (extra) Object.assign(err, extra);
  return err;
}

module.exports = {
  loadDocument,
  reloadDocument,
  safeSave,
  wouldPreserve,
  encodeForSave,
  decodeBytes,
  statFile,
  detectExternalChange,
};
