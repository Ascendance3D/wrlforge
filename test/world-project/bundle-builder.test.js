'use strict';
// World Project review-bundle builder (Phase 5A). Uses the REAL filesystem for
// reads (committed fixtures) and writes ONLY into a fresh OS-temp output dir.
// Covers: bundle layout, contents+hashes match the manifest, deterministic
// output, existing-output collision handling, in-project write refusal, blocked
// refusal, and source non-mutation across a real build.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const os = require('os');
const path = require('path');
const crypto = require('crypto');
const { scanProject } = require('../../src/world-project/project-loader');
const { buildPackagePlan } = require('../../src/world-project/package-plan');
const { writeReviewBundle, assembleBundleBuffer } = require('../../src/world-project/bundle-builder');
const { readZip } = require('../../src/world-project/zip-writer');

const FX = path.resolve(__dirname, '../fixtures/world');

function scanOf(proj, primary) {
  const root = path.join(FX, proj);
  return scanProject({ root, primary: path.join(root, primary || 'world.wrl') });
}
function tmpDir() { return fs.mkdtempSync(path.join(os.tmpdir(), 'wf-bundle-')); }
function sha(buf) { return crypto.createHash('sha256').update(buf).digest('hex'); }

function treeHash(dir) {
  const h = crypto.createHash('sha256');
  const walk = (d) => {
    for (const name of fs.readdirSync(d).sort()) {
      const p = path.join(d, name);
      const st = fs.statSync(p);
      h.update(path.relative(dir, p));
      if (st.isDirectory()) walk(p); else h.update(fs.readFileSync(p));
    }
  };
  walk(dir);
  return h.digest('hex');
}

test('bundle contains project files under project/, plus MANIFEST/REPORT/README', () => {
  const dir = tmpDir();
  try {
    const dest = path.join(dir, 'nested-review-bundle.zip');
    const res = writeReviewBundle(scanOf('nested'), dest);
    assert.ok(res.ok && fs.existsSync(dest));
    const names = readZip(fs.readFileSync(dest)).map((e) => e.name).sort();
    assert.deepEqual(names, [
      'MANIFEST.json',
      'READ-ME-FIRST.txt',
      'REPORT.md',
      'project/img/floor.png',
      'project/parts/deep/more.wrl',
      'project/parts/deep/tex/lamp.png',
      'project/parts/panel.wrl',
      'project/parts/tex/wall art.png',
      'project/world.wrl',
    ]);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('bundle contents and hashes match the manifest exactly', () => {
  const dir = tmpDir();
  try {
    const dest = path.join(dir, 'b.zip');
    writeReviewBundle(scanOf('valid70'), dest);
    const entries = readZip(fs.readFileSync(dest));
    const manifest = JSON.parse(entries.find((e) => e.name === 'MANIFEST.json').data.toString('utf8'));
    assert.equal(manifest.files.length, 72);
    for (const mf of manifest.files) {
      const entry = entries.find((e) => e.name === 'project/' + mf.path);
      assert.ok(entry, `bundle contains project/${mf.path}`);
      assert.equal(entry.data.length, mf.bytes, `size matches manifest for ${mf.path}`);
      assert.equal(sha(entry.data), mf.sha256, `hash matches manifest for ${mf.path}`);
    }
    // And every packaged entry corresponds to a manifest file (no strays).
    const projEntries = entries.filter((e) => e.name.startsWith('project/'));
    assert.equal(projEntries.length, manifest.files.length);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('the archived project file bytes equal the source bytes (no rewrite)', () => {
  const dir = tmpDir();
  try {
    const dest = path.join(dir, 'b.zip');
    writeReviewBundle(scanOf('nested'), dest);
    const entries = readZip(fs.readFileSync(dest));
    for (const e of entries.filter((x) => x.name.startsWith('project/'))) {
      const rel = e.name.slice('project/'.length);
      const src = fs.readFileSync(path.join(FX, 'nested', rel));
      assert.deepEqual(e.data, src, `${rel} is byte-for-byte the source`);
    }
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('assembled bundle buffer is deterministic', () => {
  const scan = scanOf('nested');
  const plan = buildPackagePlan(scan);
  const a = assembleBundleBuffer(scan, plan).buffer;
  const b = assembleBundleBuffer(scan, plan).buffer;
  assert.ok(a.equals(b));
});

test('refuses to overwrite an existing output (no silent clobber)', () => {
  const dir = tmpDir();
  try {
    const dest = path.join(dir, 'b.zip');
    writeReviewBundle(scanOf('nested'), dest);
    const before = fs.readFileSync(dest);
    assert.throws(() => writeReviewBundle(scanOf('nested'), dest), (e) => e.code === 'EEXISTS');
    assert.deepEqual(fs.readFileSync(dest), before, 'existing bundle untouched');
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('refuses to write the bundle inside the source project', () => {
  assert.throws(
    () => writeReviewBundle(scanOf('nested'), path.join(FX, 'nested', 'bundle.zip')),
    (e) => e.code === 'EINPROJECT');
  // No stray file created in the fixture.
  assert.ok(!fs.existsSync(path.join(FX, 'nested', 'bundle.zip')));
});

test('refuses to build a blocked project and writes nothing', () => {
  const dir = tmpDir();
  try {
    const dest = path.join(dir, 'broken.zip');
    assert.throws(() => writeReviewBundle(scanOf('broken'), dest), (e) => e.code === 'EBLOCKED');
    assert.ok(!fs.existsSync(dest), 'no bundle written for a blocked project');
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('building a real bundle does not mutate the source project', () => {
  const dir = tmpDir();
  try {
    const src = path.join(FX, 'nested');
    const before = treeHash(src);
    writeReviewBundle(scanOf('nested'), path.join(dir, 'b.zip'));
    assert.equal(treeHash(src), before, 'source tree byte-identical after build');
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('cycle project (safe) builds successfully', () => {
  const dir = tmpDir();
  try {
    const dest = path.join(dir, 'cycle.zip');
    const res = writeReviewBundle(scanOf('cycle', 'a.wrl'), dest);
    assert.ok(res.ok);
    assert.equal(res.status, 'needs-review');
    assert.ok(fs.existsSync(dest));
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});

test('World Project bundle entry names are portable on every platform', () => {
  const root = tmpDir();
  let destDir;
  try {
    fs.mkdirSync(path.join(root, 'vrml', 'tex', 'sub'), { recursive: true });
    fs.mkdirSync(path.join(root, 'audio'), { recursive: true });
    fs.mkdirSync(path.join(root, 'tex'), { recursive: true });

    // Primary world.wrl
    fs.writeFileSync(path.join(root, 'world.wrl'), 'Inline { url "vrml/a.wrl" }');
    // Nested a.wrl that references other nested files
    fs.writeFileSync(
      path.join(root, 'vrml', 'a.wrl'),
      'ImageTexture { url "tex/sub/b.png" } AudioClip { url "../audio/c.wav" } ImageTexture { url "../tex/ü-ß.png" }'
    );
    // Create referenced empty files
    fs.writeFileSync(path.join(root, 'vrml', 'tex', 'sub', 'b.png'), 'png');
    fs.writeFileSync(path.join(root, 'audio', 'c.wav'), 'wav');
    fs.writeFileSync(path.join(root, 'tex', 'ü-ß.png'), 'utf8-png');

    destDir = tmpDir();
    const dest = path.join(destDir, 'bundle.zip');
    const scan = scanProject({ root, primary: path.join(root, 'world.wrl') });
    const plan = buildPackagePlan(scan);

    // Ensure all test files were found and added to the plan
    assert.equal(plan.files.length, 5, 'plan should contain world.wrl, a.wrl, b.png, c.wav, and ü-ß.png');
    const relPaths = plan.files.map((f) => f.relPath).sort();
    assert.deepEqual(relPaths, [
      'audio/c.wav',
      'tex/ü-ß.png',
      'vrml/a.wrl',
      'vrml/tex/sub/b.png',
      'world.wrl'
    ]);

    writeReviewBundle(scan, dest, { plan });
    assert.ok(fs.existsSync(dest));

    const buf = fs.readFileSync(dest);
    const entries = readZip(buf);

    // 1. Entry names contain no \, do not start with /, no .. segment, no drive letter
    for (const entry of entries) {
      assert.ok(!entry.name.includes('\\'), `entry name contains \\: ${entry.name}`);
      assert.ok(!entry.name.startsWith('/'), `entry name starts with /: ${entry.name}`);
      assert.ok(!entry.name.includes('..'), `entry name contains ..: ${entry.name}`);
      assert.ok(!/^[A-Za-z]:/.test(entry.name), `entry name contains drive letter: ${entry.name}`);
    }

    // 2. Project file entry is exactly BUNDLE_FILES_PREFIX + <posix relative path>,
    // with same relative paths as plan.files[].relPath
    const projectEntries = entries.filter(e => e.name.startsWith('project/'));
    assert.equal(projectEntries.length, plan.files.length);

    for (const file of plan.files) {
      const entryName = `project/${file.relPath}`;
      const entry = entries.find(e => e.name === entryName);
      assert.ok(entry, `missing entry for ${entryName}`);
    }

    // Manifest paths match archive entry names one for one
    const manifestEntry = entries.find(e => e.name === 'MANIFEST.json');
    assert.ok(manifestEntry);
    const manifest = JSON.parse(manifestEntry.data.toString('utf8'));
    assert.equal(manifest.files.length, plan.files.length);
    for (const mf of manifest.files) {
      const entryName = `project/${mf.path}`;
      const entry = entries.find(e => e.name === entryName);
      assert.ok(entry, `manifest file ${mf.path} not found in archive as ${entryName}`);
    }

    // 3. File name with non-ASCII UTF-8 round-trips byte-exact
    const utf8Name = 'project/tex/ü-ß.png';
    const utf8Entry = entries.find(e => e.name === utf8Name);
    assert.ok(utf8Entry, 'UTF-8 entry missing');
    assert.deepEqual(utf8Entry.data, Buffer.from('utf8-png'), 'UTF-8 entry data mismatch');

    // Central directory header should have general-purpose bit 11 (FLAG_UTF8)
    let eocd = -1;
    for (let i = buf.length - 22; i >= 0; i--) {
      if (buf.readUInt32LE(i) === 0x06054b50) { eocd = i; break; }
    }
    const count = buf.readUInt16LE(eocd + 10);
    let p = buf.readUInt32LE(eocd + 16);
    let foundUtf8Central = false;
    for (let n = 0; n < count; n++) {
      const nameLen = buf.readUInt16LE(p + 28);
      const extraLen = buf.readUInt16LE(p + 30);
      const commentLen = buf.readUInt16LE(p + 32);
      const name = buf.toString('utf8', p + 46, p + 46 + nameLen);
      if (name === utf8Name) {
        foundUtf8Central = true;
        const flags = buf.readUInt16LE(p + 8);
        assert.ok((flags & 0x0800) === 0x0800, 'UTF-8 flag (bit 11) not set in central directory header');
      }
      p += 46 + nameLen + extraLen + commentLen;
    }
    assert.ok(foundUtf8Central, 'UTF-8 entry not found in central directory');

  } finally {
    fs.rmSync(root, { recursive: true, force: true });
    if (destDir) fs.rmSync(destDir, { recursive: true, force: true });
  }
});
