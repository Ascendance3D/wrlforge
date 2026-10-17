'use strict';
// World Project packaging PLAN + manifest + report (Phase 5A).
//
// Turns a read-only asset-graph scan into a DETERMINISTIC description of what a
// portable "review bundle" would contain -- without writing anything. This is the
// analysis half of the packaging lane; `bundle-builder.js` is the only module
// that actually creates a file, and only into a caller-chosen output path.
//
// What it computes:
//   * the packaged file set = primary WRL + nested local WRL (that exist on disk)
//     + present, exact-case local assets, each with project-relative path, asset
//     type, byte size, content hash (sha256), referencing WRL files, and
//     dependency depth,
//   * totals (files, bytes, WRL count, unique textures),
//   * findings surfaced by the graph (missing / case / unsafe / remote / cycles /
//     repeated references),
//   * files found under the project root that are NOT referenced (unused),
//   * the BLOCKING findings and an overall status (ready / blocked / needs-review).
//
// Packaging SAFETY: a bundle is BLOCKED when any required referenced asset is
// missing, case-mismatched, absolute, escapes the project root, or is remote --
// i.e. anything that could not be reproduced portably. A dependency CYCLE is
// reported but does NOT block (all its files are local and the walk is bounded).
//
// WHAT `ready` MEANS, stated because WD1.7-B2 extends it: every REQUIRED file the
// scan accounted for is present under the project root at its exact written case
// and was read and hashed, so the bundle can be reproduced portably. It has never
// meant "this world is semantically valid VRML97" and it does not mean that now.
//
// EXTERNPROTO (WD1.7-B2). External prototype libraries used to be invisible to
// the graph, so a bundle could omit every one of them and still report `ready`.
// They now arrive as `graph.externProtos` -- one record per DECLARATION, holding
// its ordered candidate list. A retrieved candidate artifact is packaged (deduped
// against everything else by absolute path); a declaration whose candidates were
// provably absent, refused as non-portable, or indeterminate BLOCKS; and a
// declaration this lane honestly cannot judge -- a `urn:`, an ISO 4.5.3
// instantiation-relative base, a recovery-damaged declaration -- is surfaced for
// REVIEW rather than presented as a failure. What is still NOT proven, and is
// WD1.7-C's: that a retrieved artifact contains the named PROTO, and what that
// artifact's own dependencies are.
//
// It never repairs a URL, renames/flattens, copies an external asset, or rewrites
// WRL source. Pure/injectable (fs reads + hashing injected) so it is unit-tested
// against the committed fixtures without Electron.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const { kindOf } = require('./asset-graph');
const { EXTERNPROTO_GROUP_STATUS } = require('./externproto-deps');

const PACKAGE_SCHEMA_VERSION = 1;
const GENERATOR = 'WRL Forge — World Project packaging (Phase 5A)';
// The label on every bundle output and manifest. The bundle is a review + manual
// hand-off package: WRL Forge does not upload it, and no server-certified format is
// claimed (the exact CTR requirements are not yet documented).
const BUNDLE_LABEL = 'WRL Forge World Project Bundle';
const BUNDLE_DISCLAIMER =
  'This bundle is produced by WRL Forge to help you review a World Project and ' +
  'prepare its files for manual upload through the Cybertown website. It is a ' +
  'review and hand-off package, not a server-certified upload format: WRL Forge ' +
  'does not upload it anywhere, and the exact Cybertown / CTR submission ' +
  'requirements (format, size limits, naming) are not yet documented — see ' +
  'docs/WORLD_PACKAGE_QUESTIONS.md. Review the contents, then submit through the ' +
  'normal Cybertown upload workflow.';

// Where the project's own files live inside the bundle archive (kept under a
// prefix so a project file literally named MANIFEST.json can never collide with
// the bundle's own metadata files at the archive root).
const BUNDLE_FILES_PREFIX = 'project/';
const MANIFEST_NAME = 'MANIFEST.json';
const REPORT_NAME = 'REPORT.md';
const README_NAME = 'READ-ME-FIRST.txt';

const cmp = (a, b) => a < b ? -1 : a > b ? 1 : 0;

const TEXTURE_KIND = 'texture';

const relOf = (root, abs) =>
  path.relative(path.resolve(root), path.resolve(abs)).split(path.sep).join('/');

function sha256(buf) {
  return crypto.createHash('sha256').update(buf).digest('hex');
}

// Bounded recursive listing of every FILE under root (absolute paths). Used for
// unused-file detection. Injectable; default is real fs.
function defaultListAllFiles(root, cap = 20000) {
  const out = [];
  const stack = [path.resolve(root)];
  while (stack.length && out.length < cap) {
    const dir = stack.pop();
    let entries;
    try { entries = fs.readdirSync(dir, { withFileTypes: true }); } catch { continue; }
    for (const e of entries) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) stack.push(p);
      else if (e.isFile()) out.push(p);
    }
  }
  return out;
}

// deps (injectable): readFile(abs)->Buffer, hash(buf)->hex, listAllFiles(root)->abs[]
function buildPackagePlan(scan, deps = {}) {
  const readFile = deps.readFile || ((p) => fs.readFileSync(p));
  const hash = deps.hash || sha256;
  const listAllFiles = deps.listAllFiles || ((r) => defaultListAllFiles(r));

  const graph = (scan && scan.graph) || {};
  const projectRoot = path.resolve(scan.root);
  const primary = path.resolve(scan.primary);
  const usable = scan.status === 'ok';

  // referrer map: resolved abs target -> sorted distinct project-relative referrers.
  const refMap = new Map();
  for (const r of graph.references || []) {
    if (!r.resolved) continue;
    const key = path.resolve(r.resolved);
    if (!refMap.has(key)) refMap.set(key, new Set());
    if (r.referrerRelative) refMap.get(key).add(r.referrerRelative);
  }
  // EXTERNPROTO declarations contribute referrer provenance for the artifacts
  // they retrieved, exactly as a url-field reference does for its target.
  const externProtoGroups = graph.externProtos || [];
  for (const g of externProtoGroups) {
    for (const c of g.candidates || []) {
      if (!c.artifactPath) continue;
      const key = path.resolve(projectRoot, c.artifactPath);
      if (!refMap.has(key)) refMap.set(key, new Set());
      if (g.referrerRelative) refMap.get(key).add(g.referrerRelative);
    }
  }
  const referrersOf = (abs) => [...(refMap.get(path.resolve(abs)) || new Set())].sort();

  // wrl abs -> depth (from the bounded walk).
  const wrlDepth = new Map();
  for (const n of graph.wrlNodes || []) wrlDepth.set(path.resolve(n.path), n.depth);

  // Candidate packaged files: existing WRL nodes + present assets. We read each
  // one's real bytes so the recorded size + hash always match what is packaged.
  const candidates = [];
  for (const n of graph.wrlNodes || []) {
    if (n.bytes == null) continue; // referenced-but-missing WRL is tracked in `missing`
    candidates.push({ abs: path.resolve(n.path), kind: 'wrl', depth: n.depth || 0 });
  }
  for (const a of graph.assets || []) {
    if (!a.present) continue;
    const abs = path.resolve(a.path);
    const referrerDepths = (a.referencedBy || [])
      .map((p) => wrlDepth.get(path.resolve(p)))
      .filter((d) => d != null);
    const depth = referrerDepths.length ? Math.min(...referrerDepths) + 1 : 1;
    candidates.push({ abs, kind: a.kind, depth, refCount: a.refCount });
  }

  // Every candidate artifact whose bytes were retrieved is packaged. Conservative
  // ON PURPOSE: an EXTERNPROTO URL list is an ORDERED FALLBACK (ISO 4.5.2), and
  // which entry a browser settles on depends on target interpretability -- a
  // WD1.7-C question. Packaging only the first retrieved candidate would silently
  // break the author's fallback chain on evidence this lane does not have.
  for (const g of externProtoGroups) {
    for (const c of g.candidates || []) {
      if (!c.artifactPath) continue;
      candidates.push({
        abs: path.resolve(projectRoot, c.artifactPath),
        kind: kindOf(c.artifactPath),
        depth: (g.depth || 0) + 1,
      });
    }
  }

  // De-dup by absolute path (a repeated reference is packaged ONCE), read + hash.
  const byAbs = new Map();
  for (const c of candidates) {
    if (byAbs.has(c.abs)) {
      // Keep the shallowest depth if two candidates map to the same file.
      const prev = byAbs.get(c.abs);
      if (c.depth < prev.depth) prev.depth = c.depth;
      continue;
    }
    byAbs.set(c.abs, c);
  }

  const files = [];
  const readErrors = [];
  for (const c of byAbs.values()) {
    let buf;
    try { buf = readFile(c.abs); } catch (err) {
      readErrors.push({ relPath: relOf(projectRoot, c.abs), error: String((err && err.message) || err) });
      continue;
    }
    const referencedBy = referrersOf(c.abs);
    files.push({
      relPath: relOf(projectRoot, c.abs),
      kind: c.kind,
      bytes: buf.length,
      sha256: hash(buf),
      depth: c.depth,
      referencedBy,
      refCount: c.refCount != null ? c.refCount : Math.max(referencedBy.length, c.kind === 'wrl' && c.depth === 0 ? 0 : 1),
    });
  }
  files.sort((a, b) => cmp(a.relPath, b.relPath));

  const packagedRel = new Set(files.map((f) => f.relPath));

  // Unused = present files under the root not in the packaged set.
  const unusedFiles = usable
    ? listAllFiles(projectRoot)
        .map((abs) => relOf(projectRoot, abs))
        .filter((rel) => rel && !rel.startsWith('..') && !packagedRel.has(rel))
        .sort()
        .map((rel) => ({ relPath: rel }))
    : [];

  // Findings (project-relative where a path is available).
  const missing = (graph.missing || []).map((m) => ({
    relPath: relOf(projectRoot, m.path), kind: m.kind, refCount: m.refCount,
  })).sort((a, b) => cmp(a.relPath, b.relPath));
  const caseMismatches = (graph.caseMismatches || []).map((c) => ({
    referenced: relOf(projectRoot, c.referenced), actual: path.basename(c.actual),
  })).sort((a, b) => cmp(a.referenced, b.referenced));
  const unsafe = [...new Set((graph.unsafe || []).map((u) => u.url))].sort()
    .map((url) => ({ url, category: (graph.unsafe.find((u) => u.url === url) || {}).category || null }));
  const remote = [...new Set((graph.remoteRefs || []).map((r) => r.url))].sort()
    .map((url) => ({ url }));
  const cycles = (graph.cycles || []).map((c) => ({
    from: relOf(projectRoot, c.from), to: relOf(projectRoot, c.to),
  })).sort((a, b) => cmp(a.from + a.to, b.from + b.to));
  const repeated = files.filter((f) => (f.refCount || 0) > 1)
    .map((f) => ({ relPath: f.relPath, refCount: f.refCount }));

  // One finding per EXTERNPROTO DECLARATION, keeping the written candidate order,
  // the written URL, the fragment and each candidate's own retrieval outcome. The
  // four uncertainty kinds NOT_FOUND / NOT_RETRIEVED_BY_POLICY /
  // UNSUPPORTED_REFERENCE / context-required stay distinguishable here -- they are
  // different problems with different fixes and must not collapse to "missing".
  const externProtos = externProtoGroups.map((g) => ({
    referrer: relOf(projectRoot, g.referrer),
    name: g.name,
    status: g.status,
    base: g.base,
    enclosingProto: g.enclosingProto || null,
    candidates: (g.candidates || []).map((c) => ({
      index: c.index,
      writtenUrl: c.writtenUrl,
      form: c.form || null,
      // Preserved as provenance. NOT validated against a PROTO name and NOT part
      // of any filename -- ISO 4.9.3 target selection is WD1.7-C's.
      fragment: c.fragment == null ? null : c.fragment,
      retrieval: c.status || null,
      reason: c.reason || null,
      artifact: c.artifactPath || null,
    })),
  })).sort((a, b) => cmp(a.referrer, b.referrer)
    || cmp(String(a.name), String(b.name))
    || (a.candidates.length - b.candidates.length));

  const externProtoBlocking = externProtos.filter((g) => (
    g.status === EXTERNPROTO_GROUP_STATUS.MISSING
    || g.status === EXTERNPROTO_GROUP_STATUS.NOT_PORTABLE
    || g.status === EXTERNPROTO_GROUP_STATUS.INDETERMINATE
  ));
  const externProtoReview = externProtos.filter((g) => (
    g.status === EXTERNPROTO_GROUP_STATUS.UNSUPPORTED
    || g.status === EXTERNPROTO_GROUP_STATUS.CONTEXT_REQUIRED
    || g.status === EXTERNPROTO_GROUP_STATUS.UNPROVABLE
    || g.status === EXTERNPROTO_GROUP_STATUS.NO_CANDIDATES
  ));
  // A document whose EXTERNPROTO declarations could not be enumerated at all.
  // Unknown is not the same as none, so it is surfaced rather than assumed empty.
  const externProtoErrors = (graph.externProtoErrors || []).map((e) => ({
    referrer: relOf(projectRoot, e.referrer), error: e.error,
  })).sort((a, b) => cmp(a.referrer, b.referrer));

  // Blocking rules: anything that could not be reproduced portably.
  const blocking = [];
  if (missing.length) blocking.push({ code: 'missing-assets', count: missing.length,
    message: `${missing.length} referenced asset(s) are missing on disk` });
  if (caseMismatches.length) blocking.push({ code: 'case-mismatch', count: caseMismatches.length,
    message: `${caseMismatches.length} reference(s) differ only by filename case (breaks on a case-sensitive server)` });
  if (unsafe.length) blocking.push({ code: 'unsafe-path', count: unsafe.length,
    message: `${unsafe.length} reference(s) use an absolute path or escape the project root` });
  if (remote.length) blocking.push({ code: 'remote-reference', count: remote.length,
    message: `${remote.length} remote URL reference(s) cannot be bundled` });
  if (readErrors.length) blocking.push({ code: 'unreadable', count: readErrors.length,
    message: `${readErrors.length} referenced file(s) could not be read` });
  if (!usable) blocking.push({ code: 'primary-unreadable', count: 1,
    message: 'the primary world file could not be read/parsed' });
  // Each EXTERNPROTO failure keeps its own name. Collapsing them into
  // `missing-assets` would tell a user to go find a file when the real problem is
  // a remote library, an unreadable archive, or a base this lane cannot know.
  const epMissing = externProtoBlocking.filter((g) => g.status === EXTERNPROTO_GROUP_STATUS.MISSING);
  const epNotPortable = externProtoBlocking.filter((g) => g.status === EXTERNPROTO_GROUP_STATUS.NOT_PORTABLE);
  const epIndeterminate = externProtoBlocking.filter((g) => g.status === EXTERNPROTO_GROUP_STATUS.INDETERMINATE);
  if (epMissing.length) blocking.push({ code: 'externproto-missing', count: epMissing.length,
    message: `${epMissing.length} EXTERNPROTO declaration(s) name an external prototype library that is missing on disk` });
  if (epNotPortable.length) blocking.push({ code: 'externproto-not-portable', count: epNotPortable.length,
    message: `${epNotPortable.length} EXTERNPROTO declaration(s) can only be satisfied from outside the project (remote or out-of-root) and cannot be bundled` });
  if (epIndeterminate.length) blocking.push({ code: 'externproto-indeterminate', count: epIndeterminate.length,
    message: `${epIndeterminate.length} EXTERNPROTO declaration(s) could not be read, decoded, or resolved to a single artifact` });

  const needsReview = !blocking.length &&
    (cycles.length > 0 || unusedFiles.length > 0 || !!graph.truncated || !!graph.depthCapped
      || externProtoReview.length > 0 || externProtoErrors.length > 0);
  const status = blocking.length ? 'blocked' : (needsReview ? 'needs-review' : 'ready');

  const totals = {
    totalFiles: files.length,
    totalBytes: files.reduce((n, f) => n + f.bytes, 0),
    wrlCount: files.filter((f) => f.kind === 'wrl').length,
    uniqueTextureCount: files.filter((f) => f.kind === TEXTURE_KIND).length,
  };

  return {
    ok: usable,
    schemaVersion: PACKAGE_SCHEMA_VERSION,
    label: BUNDLE_LABEL,
    projectName: path.basename(projectRoot),
    primaryRel: relOf(projectRoot, primary),
    status,
    totals,
    files,
    findings: { missing, caseMismatches, unsafe, remote, cycles, repeated, externProtos },
    externProtoReview,
    externProtoErrors,
    unusedFiles,
    readErrors,
    blocking,
    // Cycles are safe to bundle: every file in a cycle is local and the walk was
    // bounded/cycle-safe, so the packaged set is finite and complete.
    cyclesSafe: true,
    truncated: !!graph.truncated,
    depthCapped: !!graph.depthCapped,
  };
}

// Deterministic manifest object (no wall-clock, no absolute paths) -- two builds
// of the same project produce byte-identical JSON. `JSON.stringify(buildManifest,
// null, 2)` is the machine-readable manifest that goes into the bundle.
function buildManifest(plan) {
  return {
    schemaVersion: plan.schemaVersion,
    generator: GENERATOR,
    label: plan.label,
    disclaimer: BUNDLE_DISCLAIMER,
    project: plan.projectName,
    primary: plan.primaryRel,
    status: plan.status,
    totals: plan.totals,
    files: plan.files.map((f) => ({
      path: f.relPath,
      kind: f.kind,
      bytes: f.bytes,
      sha256: f.sha256,
      depth: f.depth,
      refCount: f.refCount,
      referencedBy: f.referencedBy,
    })),
    findings: plan.findings,
    unusedFiles: plan.unusedFiles.map((u) => u.relPath),
    blocking: plan.blocking,
  };
}

function manifestJson(plan) {
  return JSON.stringify(buildManifest(plan), null, 2) + '\n';
}

// Human-readable Markdown report. Deterministic (no timestamps).
function renderReport(plan) {
  const L = [];
  const push = (s) => L.push(s);
  push(`# ${plan.label}`);
  push('');
  push(BUNDLE_DISCLAIMER);
  push('');
  push(`- **Project:** ${plan.projectName}`);
  push(`- **Primary world:** \`${plan.primaryRel}\``);
  push(`- **Status:** ${plan.status.toUpperCase()}`);
  push('');
  push('## Totals');
  push('');
  push(`| Metric | Value |`);
  push(`|---|---|`);
  push(`| Files packaged | ${plan.totals.totalFiles} |`);
  push(`| Total bytes | ${plan.totals.totalBytes} |`);
  push(`| WRL files | ${plan.totals.wrlCount} |`);
  push(`| Unique textures | ${plan.totals.uniqueTextureCount} |`);
  push('');

  if (plan.blocking.length) {
    push('## Blocking findings (bundle cannot be built until resolved)');
    push('');
    for (const b of plan.blocking) push(`- **${b.code}** — ${b.message}`);
    push('');
  }

  const f = plan.findings;
  const section = (title, rows) => {
    if (!rows.length) return;
    push(`## ${title}`);
    push('');
    for (const r of rows) push(`- ${r}`);
    push('');
  };
  section('Missing assets', f.missing.map((m) => `\`${m.relPath}\` (${m.kind}, ×${m.refCount})`));
  section('Case mismatches', f.caseMismatches.map((c) => `\`${c.referenced}\` on disk as \`${c.actual}\``));
  section('Unsafe references', f.unsafe.map((u) => `\`${u.url}\` (${u.category})`));
  section('Remote references', f.remote.map((r) => `\`${r.url}\``));
  section('Dependency cycles (reported, not blocking)', f.cycles.map((c) => `\`${c.from}\` → \`${c.to}\``));
  // One line per DECLARATION, then its candidates in written order. Order is
  // normative (ISO 4.5.2 "decreasing order of preference"), so the report shows
  // the list rather than a chosen winner -- this lane has no winner to show.
  section('EXTERNPROTO declarations (external prototype libraries)', (f.externProtos || []).map((g) => {
    const where = g.base === 'context-required'
      ? ' — base is the instantiating file (ISO 4.5.3); not resolvable from this document alone'
      : g.base === 'unavailable' ? ' — no project-relative base for the declaring file' : '';
    const cands = g.candidates.map((c) => `\n  - [${c.index}] \`${c.writtenUrl}\`${c.fragment ? ` (fragment \`${c.fragment}\`)` : ''} → ${c.retrieval || 'not evaluated'}${c.reason ? ` (${c.reason})` : ''}${c.artifact ? ` → \`${c.artifact}\`` : ''}`).join('');
    return `\`${g.referrer}\` declares \`${g.name}\` — **${g.status}**${where}${cands}`;
  }));
  section('EXTERNPROTO scan errors (declarations could not be enumerated)',
    (plan.externProtoErrors || []).map((e) => `\`${e.referrer}\` — ${e.error}`));
  section('Repeated references (packaged once)', f.repeated.map((r) => `\`${r.relPath}\` (×${r.refCount})`));
  section('Unused files under the project root (NOT included in the bundle)',
    plan.unusedFiles.map((u) => `\`${u.relPath}\``));

  push('## Packaged files');
  push('');
  push(`| Path | Type | Bytes | Depth | sha256 |`);
  push(`|---|---|---|---|---|`);
  for (const file of plan.files) {
    push(`| \`${file.relPath}\` | ${file.kind} | ${file.bytes} | ${file.depth} | \`${file.sha256.slice(0, 16)}…\` |`);
  }
  push('');
  return L.join('\n');
}

function readmeText(plan) {
  return [
    plan.label,
    '',
    BUNDLE_DISCLAIMER,
    '',
    `Project: ${plan.projectName}`,
    `Primary world: ${plan.primaryRel}`,
    `Status: ${plan.status.toUpperCase()}`,
    '',
    `The project's files are under "${BUNDLE_FILES_PREFIX}" preserving their`,
    'original relative folder structure. MANIFEST.json is the machine-readable',
    'manifest; REPORT.md is the human-readable report. Nothing here has been',
    'repaired, renamed, flattened, or rewritten -- the files are byte-for-byte',
    'copies of the source project.',
    '',
  ].join('\n');
}

module.exports = {
  PACKAGE_SCHEMA_VERSION,
  GENERATOR,
  BUNDLE_LABEL,
  BUNDLE_DISCLAIMER,
  BUNDLE_FILES_PREFIX,
  MANIFEST_NAME,
  REPORT_NAME,
  README_NAME,
  buildPackagePlan,
  buildManifest,
  manifestJson,
  renderReport,
  readmeText,
  sha256,
  defaultListAllFiles,
  relOf,
};
