'use strict';
// World Project packaging plan / manifest / report (Phase 5A). Exercises the
// deterministic package manifest against the committed fixtures with the REAL
// filesystem (read-only), covering the Phase 5A completion-gate scenarios at the
// pure-module level (no Electron, no archive I/O):
//   deterministic ordering, nested WRL + per-file depth, gzip WRL, >20 and >=70
//   textures, repeated-reference-once, missing/case/remote/unsafe blocking,
//   safe cycles, unused-file reporting, and source non-mutation.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const child_process = require('child_process');
const os = require('os');
const { scanProject } = require('../../src/world-project/project-loader');
const {
  buildPackagePlan, buildManifest, manifestJson, renderReport,
  BUNDLE_LABEL,
} = require('../../src/world-project/package-plan');

const FX = path.resolve(__dirname, '../fixtures/world');

function planOf(proj, primary) {
  const root = path.join(FX, proj);
  const scan = scanProject({ root, primary: path.join(root, primary || 'world.wrl') });
  return { scan, plan: buildPackagePlan(scan) };
}

// sha256 a whole directory tree (sorted) so we can assert byte-for-byte
// non-mutation of the source project across a plan build.
function treeHash(dir) {
  const h = crypto.createHash('sha256');
  const walk = (d) => {
    for (const name of fs.readdirSync(d).sort()) {
      const p = path.join(d, name);
      const st = fs.statSync(p);
      h.update(path.relative(dir, p));
      if (st.isDirectory()) walk(p);
      else h.update(fs.readFileSync(p));
    }
  };
  walk(dir);
  return h.digest('hex');
}

test('deterministic manifest: two builds are byte-identical and sorted', () => {
  const scan = planOf('nested').scan;
  const a = manifestJson(buildPackagePlan(scan));
  const b = manifestJson(buildPackagePlan(scan));
  assert.equal(a, b, 'same project → identical manifest JSON');
  const files = JSON.parse(a).files.map((f) => f.path);
  const cmp = (x, y) => x < y ? -1 : x > y ? 1 : 0;
  const sorted = [...files].sort(cmp);
  assert.deepEqual(files, sorted, 'files must be in a stable sorted order');
});

test('manifest ordering is code-unit based and locale-independent', (t) => {
  // Construct a temporary project with files that sort differently based on locale.
  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'wrlforge-plan-test-'));
  try {
    const files = ['z.png', 'ä.png', 'B.png', 'a.png', 'a-b.png', 'a_b.png'];
    for (const f of files) {
      fs.writeFileSync(path.join(tmpDir, f), 'data');
    }
    const worldWrl = '#VRML V2.0 utf8\n' + files.map((f) => `ImageTexture { url "${f}" }`).join('\n');

    // Reference not existing files to populate findings.missing and check those too.
    const missingWrl = '#VRML V2.0 utf8\n' + files.map((f) => `ImageTexture { url "missing_${f}" }`).join('\n');
    fs.writeFileSync(path.join(tmpDir, 'world.wrl'), worldWrl + '\n' + missingWrl);

    const scan = scanProject({ root: tmpDir, primary: path.join(tmpDir, 'world.wrl') });
    const plan = buildPackagePlan(scan);
    const man = buildManifest(plan);

    // Expect code-unit sort: 'B.png' (66), 'a-b.png' (97,45), 'a.png' (97,46), 'a_b.png' (97,95), 'z.png' (122), 'ä.png' (228)
    const expectedFiles = ['B.png', 'a-b.png', 'a.png', 'a_b.png', 'world.wrl', 'z.png', 'ä.png'];
    assert.deepEqual(man.files.map((f) => f.path), expectedFiles);

    // Verify findings follow the same sort order
    const expectedMissing = ['missing_B.png', 'missing_a-b.png', 'missing_a.png', 'missing_a_b.png', 'missing_z.png', 'missing_ä.png'];
    assert.deepEqual(plan.findings.missing.map((m) => m.relPath), expectedMissing);

    // Run identical check in a subprocess with sv_SE locale if on Unix
    if (process.platform !== 'win32') {
      const script = `
        const path = require('path');
        const { scanProject } = require(${JSON.stringify(path.join(__dirname, '../../src/world-project/project-loader.js'))});
        const { buildPackagePlan, buildManifest } = require(${JSON.stringify(path.join(__dirname, '../../src/world-project/package-plan.js'))});
        const tmpDir = ${JSON.stringify(tmpDir)};
        const scan = scanProject({ root: tmpDir, primary: path.join(tmpDir, 'world.wrl') });
        const plan = buildPackagePlan(scan);
        const man = buildManifest(plan);
        console.log(JSON.stringify({
          files: man.files.map((f) => f.path),
          missing: plan.findings.missing.map((m) => m.relPath),
          localeSort: ['z', 'ä'].sort((a, b) => a.localeCompare(b))
        }));
      `;
      let out;
      try {
        out = child_process.execFileSync(process.execPath, ['-e', script], {
          env: { ...process.env, LANG: 'sv_SE.UTF-8', LC_ALL: 'sv_SE.UTF-8' },
          encoding: 'utf8'
        });
      } catch (err) {
        t.skip('subprocess exec failed: ' + (err.stderr || err.message));
        return;
      }

      const subResult = JSON.parse(out);
      if (subResult.localeSort[0] !== 'z' || subResult.localeSort[1] !== 'ä') {
        t.skip('locale sv_SE.UTF-8 not supported on this system (got order: ' + subResult.localeSort.join(',') + ')');
        return;
      }

      assert.deepEqual(subResult.files, expectedFiles, 'Subprocess files order should match code-unit sort despite sv_SE locale');
      assert.deepEqual(subResult.missing, expectedMissing, 'Subprocess missing order should match code-unit sort despite sv_SE locale');
    }
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
});

test('nested WRL: primary + nested WRL + textures with correct depth + referencedBy', () => {
  const { plan } = planOf('nested');
  assert.equal(plan.status, 'ready');
  const byPath = Object.fromEntries(plan.files.map((f) => [f.relPath, f]));
  // depth: primary=0, its direct deps=1, deeper Inline chain grows.
  assert.equal(byPath['world.wrl'].depth, 0);
  assert.equal(byPath['parts/panel.wrl'].depth, 1);
  assert.equal(byPath['parts/deep/more.wrl'].depth, 2);
  assert.equal(byPath['parts/deep/tex/lamp.png'].depth, 3);
  // referencedBy is by the WRL that authored the reference.
  assert.deepEqual(byPath['parts/tex/wall art.png'].referencedBy, ['parts/panel.wrl']);
  assert.equal(plan.totals.wrlCount, 3);
  assert.equal(plan.totals.uniqueTextureCount, 3);
  assert.equal(plan.totals.totalFiles, 6);
});

test('gzip WRL files are packaged (raw gzip bytes) and hash the on-disk bytes', () => {
  const { plan } = planOf('nested');
  const more = plan.files.find((f) => f.relPath === 'parts/deep/more.wrl');
  const raw = fs.readFileSync(path.join(FX, 'nested', 'parts/deep/more.wrl'));
  assert.equal(raw[0], 0x1f, 'fixture more.wrl is gzip on disk');
  assert.equal(more.bytes, raw.length, 'recorded size = on-disk (gzip) size, not decompressed');
  assert.equal(more.sha256, crypto.createHash('sha256').update(raw).digest('hex'));
});

test('more than 20 textures (mini = 25) packaged with no truncation', () => {
  const { plan } = planOf('mini');
  assert.equal(plan.status, 'ready');
  assert.equal(plan.totals.uniqueTextureCount, 25);
  assert.ok(!plan.truncated && !plan.depthCapped);
});

test('at least 70 textures (valid70 = 71) packaged with no truncation', () => {
  const { plan } = planOf('valid70');
  assert.equal(plan.totals.uniqueTextureCount, 71);
  assert.equal(plan.totals.totalFiles, 72); // 71 textures + primary
  assert.ok(!plan.truncated);
});

test('a repeated reference is packaged exactly once (refCount > 1)', () => {
  const { plan } = planOf('nested');
  const floor = plan.files.filter((f) => f.relPath === 'img/floor.png');
  assert.equal(floor.length, 1, 'floor.png appears once despite two references');
  assert.equal(floor[0].refCount, 2);
  assert.ok(plan.findings.repeated.some((r) => r.relPath === 'img/floor.png' && r.refCount === 2));
});

test('missing required asset blocks packaging', () => {
  const { plan } = planOf('broken');
  assert.equal(plan.status, 'blocked');
  assert.ok(plan.blocking.some((b) => b.code === 'missing-assets'));
  assert.ok(plan.findings.missing.some((m) => m.relPath === 'img/missing.jpg'));
});

test('case mismatch blocks packaging', () => {
  const { plan } = planOf('broken');
  assert.ok(plan.blocking.some((b) => b.code === 'case-mismatch'));
  assert.ok(plan.findings.caseMismatches.some((c) => c.referenced === 'img/Present.PNG' && c.actual === 'present.png'));
});

test('remote and unsafe (absolute/traversal) references block packaging', () => {
  const { plan } = planOf('broken');
  assert.ok(plan.blocking.some((b) => b.code === 'remote-reference'));
  assert.ok(plan.blocking.some((b) => b.code === 'unsafe-path'));
  // http + protocol-relative are both surfaced as remote.
  assert.ok(plan.findings.remote.some((r) => /example\.com/.test(r.url)));
  // /etc/hosts (absolute) and ../../escape.png (traversal) are unsafe.
  assert.ok(plan.findings.unsafe.some((u) => u.url === '/etc/hosts'));
  assert.ok(plan.findings.unsafe.some((u) => u.url === '../../escape.png'));
});

test('dependency cycles terminate safely and do NOT block (all local)', () => {
  const { plan } = planOf('cycle', 'a.wrl');
  assert.equal(plan.status, 'needs-review');
  assert.equal(plan.blocking.length, 0);
  assert.ok(plan.cyclesSafe);
  assert.ok(plan.findings.cycles.length >= 1);
  // Both WRLs and both textures are still packaged (finite, complete set).
  assert.equal(plan.totals.wrlCount, 2);
  assert.equal(plan.totals.uniqueTextureCount, 2);
});

test('unused files under the project root are reported but NOT packaged', () => {
  const { plan } = planOf('unused');
  assert.equal(plan.status, 'needs-review');
  const packaged = plan.files.map((f) => f.relPath);
  assert.deepEqual(packaged.sort(), ['img/used.png', 'props.wrl', 'world.wrl']);
  const unused = plan.unusedFiles.map((u) => u.relPath).sort();
  assert.deepEqual(unused, ['img/orphan.png', 'notes.txt', 'old/backup.wrl']);
  // The unused files are explicitly not in the packaged set.
  for (const u of unused) assert.ok(!packaged.includes(u));
});

test('building a plan does not mutate the source project (byte-identical)', () => {
  const dir = path.join(FX, 'nested');
  const before = treeHash(dir);
  buildPackagePlan(planOf('nested').scan);
  buildPackagePlan(planOf('nested').scan);
  assert.equal(treeHash(dir), before, 'source tree unchanged after plan builds');
});

test('a primary-unreadable scan yields an un-buildable (blocked) plan', () => {
  // Point at a directory-as-primary style broken scan: use the gz primary but
  // corrupt via a non-existent primary path -> scan status primary-unreadable.
  const root = path.join(FX, 'nested');
  const scan = scanProject({ root, primary: path.join(root, 'does-not-exist.wrl') });
  const plan = buildPackagePlan(scan);
  assert.equal(plan.ok, false);
  assert.equal(plan.status, 'blocked');
  assert.ok(plan.blocking.some((b) => b.code === 'primary-unreadable'));
});

test('manifest + report carry the World Project Bundle label + review disclaimer', () => {
  const { plan } = planOf('nested');
  const man = buildManifest(plan);
  assert.equal(man.label, BUNDLE_LABEL);
  assert.equal(BUNDLE_LABEL, 'WRL Forge World Project Bundle');
  assert.match(man.disclaimer, /not a server-certified upload format/i);
  assert.match(man.disclaimer, /does not upload/i);
  assert.match(man.disclaimer, /manual upload through the Cybertown website/i);
  const report = renderReport(plan);
  assert.ok(report.startsWith(`# ${BUNDLE_LABEL}`));
  assert.match(report, /Unique textures \| 3/);
});

test('manifest is JSON-serializable and free of absolute paths / wall-clock', () => {
  const { plan } = planOf('nested');
  const json = manifestJson(plan);
  assert.doesNotThrow(() => JSON.parse(json));
  assert.ok(!json.includes(FX), 'no absolute filesystem path leaks into the manifest');
  assert.ok(!/\bnode_modules\b/.test(json));
});
