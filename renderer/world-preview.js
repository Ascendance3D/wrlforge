'use strict';
// Embedded X_ITE World Project preview controller (renderer, Phase 4B).
//
// PROFILE SEPARATION: this is the World Project preview, deliberately SEPARATE
// from the Mall Item preview (renderer/preview.js). It applies NO Mall placement
// rules -- no Cybertown Fit, no fit-math, no guides, no bounds compliance, no
// 80KB cap, no texture cap. A world is analysed and displayed as-authored.
//
// Read-only + local-only + asset-graph-authorized:
//   - The decompressed primary text and a wrlworld:// base URL come from the
//     main process (window.vrmlpad.world.loadPreview -- no path supplied here).
//   - X_ITE resolves every nested Inline / texture against wrlworld:// URLs,
//     which route back to the main-process handler; that handler serves ONLY
//     files the production asset graph authorized, confined to the project root,
//     gzip-decompressed. Missing / case-mismatched / remote / unsafe references
//     are NOT served (they surface as runtime load warnings).
//   - Each nested WRL resolves its relative URLs from its OWN directory (the
//     scheme is hierarchical), so per-file base paths are correct.
//
// This preview is analysis + display only. It never marks a project upload-ready,
// never repairs/copies/renames assets, never packages or uploads, never edits.

(function () {
  const W = (window.vrmlpad && window.vrmlpad.world) || null;

  let browser = null;
  let ready = null;
  let meta = null;                 // last payload from loadPreview
  let haveValidScene = false;      // a real scene has rendered at least once
  let viewpointNodes = [];         // live X_ITE viewpoint nodes for the selector
  let userNav = null;              // the user's explicit navigation-mode choice
  const runtimeWarnings = [];
  // WD2-D: the private X_ITE pick adapter for THIS browser (editor page only),
  // and the generation this engine activated together with its scene.
  let pickAdapter = null;
  let pickShown = null;

  const el = (id) => document.getElementById(id);

  // ---- runtime load-failure capture (missing/relative/remote deps) -----------
  const origError = console.error;
  console.error = function (...a) {
    const msg = a.join(' ');
    if (/(texture|inline|\.wrl|\.png|\.jpg|\.jpeg|\.gif|Failed to (load|fetch)|Couldn't load|could not|404)/i.test(msg)) {
      runtimeWarnings.push(msg.slice(0, 240));
    }
    origError.apply(console, a);
  };

  function setStatus(text, kind) {
    const s = el('wpStatus');
    if (!s) return;
    s.textContent = text;
    s.style.color = kind === 'error' ? '#ff9b9b' : kind === 'warn' ? '#ffce6a' : '#9ab';
  }
  function setStale(on) {
    const b = el('wpStale');
    if (b) b.style.display = on ? 'inline-block' : 'none';
  }

  // ---- X_ITE readiness -------------------------------------------------------
  // Same X_ITE/WebGL startup contract as the Mall lane: X3D() resolving proves
  // only that <x3d-canvas>.browser exists, not that its WebGL context is alive.
  // The World preview reaches createX3DFromString() through the SAME X_ITE
  // surface, so the SAME bounded readiness gate applies here -- no World Project
  // behaviour, policy, or Mall rule is involved.
  // See src/preview/browser-readiness.js.
  const Readiness = window.WrlBrowserReadiness;

  async function acquireBrowser() {
    const res = await Readiness.acquireReadyBrowser({
      awaitX3D: () => X3D(),
      getBrowser: () => { const c = el('wpCanvas'); return c ? c.browser : null; },
      recreateBrowser: () => {
        const c = Readiness.replaceCanvasElement(document, 'wpCanvas');
        return c ? c.browser : null;
      },
      sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
      now: () => Date.now(),
    });
    browser = res.browser;
    discardPickAdapter();
    if (window.WrlXitePickAdapter) {
      pickAdapter = window.WrlXitePickAdapter.createXitePickAdapter({ X3D: window.X3D, browser });
    }
    // Discover viewpoints authored inside nested Inline scenes too.
    try { browser.setBrowserOption('EnableInlineViewpoints', true); } catch { /* older X_ITE */ }
    return browser;
  }

  function discardPickAdapter() {
    pickShown = null;
    if (pickAdapter) pickAdapter.dispose();
    pickAdapter = null;
  }

  function retirePick(reason) {
    pickShown = null;
    if (pickAdapter) { pickAdapter.abort(); pickAdapter.retire(reason); }
  }

  // null while `generation` is the displayed one, else the REFUSED_STALE reason.
  function pickCurrentCheck(generation) {
    if (!pickShown || !generation || pickShown.generation !== generation) return 'hit-from-another-preview-generation';
    if (!browser || browser.currentScene !== pickShown.scene) return 'preview-scene-replaced';
    return null;
  }

  async function ensureBrowser() {
    if (browser && Readiness.isBrowserUsable(browser)) return browser;
    if (browser) { browser = null; ready = null; discardPickAdapter(); }
    if (!ready) {
      ready = acquireBrowser().catch((err) => {
        // Never cache a rejected promise or a half-initialised browser: Refresh
        // must be able to retry without restarting the app.
        ready = null;
        browser = null;
        throw err;
      });
    }
    return ready;
  }

  // The default source: the World workspace's read-only disk loader. The editor
  // live-preview lane (Phase 7C3) passes its own `source` returning the
  // authorized UNSAVED-buffer payload (same shape), plus `preserveView: true` so
  // a refresh keeps the user's viewpoint/navigation where possible. The render/
  // viewpoint/warnings path below is reused verbatim, never forked.
  const defaultSource = () => W.loadPreview();

  // ---- public: (re)load the current world into the preview -------------------
  // Returns the debug snapshot augmented with { ok } / { ok:false, parseError |
  // error | initError } so an orchestrator (the editor lane) can drive its own
  // state machine while the existing QA hooks keep their fields. `initError` means
  // the preview could not start -- no world text was parsed, so nothing about the
  // project is being called invalid.
  async function load(opts = {}) {
    const source = (opts && opts.source) || (W ? defaultSource : null);
    if (!source) { setStatus('World preview unavailable (no bridge).', 'error'); return { ...debugState(), ok: false, error: 'no-bridge' }; }
    try {
      await ensureBrowser();
    } catch (err) {
      // Initialization failure -- no world text was parsed, so nothing about the
      // project is being called invalid. Reported as initError, not parseError.
      const msg = String((err && err.message) || err);
      setStatus(msg + ' Use Refresh to try again.', 'error');
      setStale(haveValidScene);
      renderMeta();
      return { ...debugState(), ok: false, initError: msg };
    }
    setStatus('Loading world preview…');
    let payload;
    try {
      payload = await source();
    } catch (err) {
      setStatus('Cannot load preview: ' + (err && err.message || err), 'error');
      return { ...debugState(), ok: false, error: String((err && err.message) || err) };
    }
    meta = payload;
    runtimeWarnings.length = 0;

    if (payload.status === 'primary-unreadable' || payload.text == null) {
      // Primary can't be read (e.g. corrupt gzip). Keep any last valid scene.
      setStatus('Primary world could not be read: ' + (payload.error || 'unknown') +
        (haveValidScene ? ' — keeping last valid preview.' : ''), 'error');
      setStale(haveValidScene);
      renderMeta();
      return { ...debugState(), ok: false, parseError: payload.error || 'primary-unreadable' };
    }

    // Capture the bound viewpoint's identity BEFORE the scene is replaced, so a
    // successful refresh can restore it (DEF -> description -> index -> first).
    const prevView = opts && opts.preserveView ? captureViewState() : null;

    // WD2-D: the editor asks for provenance only in the Model workspace, and
    // only when the World root string IS the edited document.
    const provenance = opts && opts.provenance && pickAdapter ? opts.provenance : null;
    pickShown = null;
    try {
      browser.baseURL = payload.baseURL;
      let scene;
      let generation = null;
      if (provenance) {
        const r = await pickAdapter.parseWithProvenance(payload.text, provenance);
        if (!r.scene) return { ...debugState(), ok: false, cancelled: true };
        scene = r.scene;
        generation = r.generation;
      } else {
        scene = await browser.createX3DFromString(payload.text);
      }
      await browser.replaceWorld(scene);
      pickShown = generation && pickAdapter.activate(generation, scene) ? { generation, scene } : null;
      haveValidScene = true;
      setStale(false);
    } catch (err) {
      // A dead WebGL context makes X_ITE throw from INSIDE its VRML parser, which
      // rewrites it as "Unexpected end of file". Re-check readiness before
      // blaming the world.
      if (!Readiness.isBrowserUsable(browser)) {
        browser = null; ready = null;   // next load()/Refresh re-acquires
        discardPickAdapter();
        const msg = '3D preview lost its graphics context before this world could be parsed. The world was NOT read as invalid — use Refresh to try again.';
        setStatus(msg, 'error');
        setStale(haveValidScene);
        renderMeta();
        return { ...debugState(), ok: false, initError: msg, detail: String((err && err.message) || err) };
      }
      // Temporary parse error (e.g. a half-written external save): keep the last
      // valid scene AND its viewpoint, flag stale, allow a manual Refresh. Do NOT
      // clear the canvas.
      setStatus('Parse error — ' + (haveValidScene ? 'keeping last valid preview.' : 'no scene yet.') +
        ' Fix the world and Refresh. (' + (err && err.message || err) + ')', 'error');
      setStale(haveValidScene);
      renderMeta();
      return { ...debugState(), ok: false, parseError: String((err && err.message) || err) };
    }

    // Let nested Inline scenes + textures attempt to load before enumerating.
    await new Promise((r) => setTimeout(r, 500));
    discoverViewpoints();
    if (prevView && window.WrlViewpointPreserve) {
      const target = window.WrlViewpointPreserve.resolveViewpointRestore(
        prevView,
        viewpointNodes.map((vp, i) => describeViewpoint(vp, i)),
      );
      if (target.action === 'bind') selectViewpoint(target.index, { syncSelect: true });
    }
    // Preserve the user's explicit navigation-mode choice across refreshes (the
    // new scene's own NavigationInfo would otherwise reset it on every update).
    if (opts && opts.preserveView && userNav) setNavigation(userNav);
    setStatus(payload.wasGzipped
      ? 'World preview loaded (primary from gzip). Analysis + display only — not an upload check.'
      : 'World preview loaded. Analysis + display only — not an upload check.');
    renderMeta();
    return { ...debugState(), ok: true };
  }

  // ---- viewpoints ------------------------------------------------------------
  function activeLayer() {
    try { return browser.getActiveLayer(); } catch { return null; }
  }
  function viewpointLabel(vp, i) {
    let desc = '';
    try { desc = (vp.getDescriptions() || []).join(' » '); } catch { /* ignore */ }
    if (desc) return desc;
    let type = 'Viewpoint';
    try { type = vp.getTypeName(); } catch { /* ignore */ }
    return `${type} ${i + 1}`;
  }
  function discoverViewpoints() {
    const sel = el('wpViewpoint');
    viewpointNodes = [];
    const layer = activeLayer();
    try { viewpointNodes = (layer && layer.getUserViewpoints()) || []; } catch { viewpointNodes = []; }
    if (!sel) return;
    sel.innerHTML = '';
    if (!viewpointNodes.length) {
      const o = document.createElement('option');
      o.textContent = '(default view)';
      sel.appendChild(o);
      sel.disabled = true;
      return;
    }
    sel.disabled = false;
    viewpointNodes.forEach((vp, i) => {
      const o = document.createElement('option');
      o.value = String(i);
      o.textContent = viewpointLabel(vp, i);
      let bound = false;
      try { bound = vp._isBound && vp._isBound.getValue(); } catch { /* ignore */ }
      if (bound) o.selected = true;
      sel.appendChild(o);
    });
  }
  // The raw identity of one live viewpoint node, for the pure preservation
  // resolver (src/preview/viewpoint-preserve.js): DEF name + raw description
  // (null when unset -- the synthetic "Viewpoint N" label never participates).
  function describeViewpoint(vp, i) {
    // DEF name where this X_ITE build exposes it (internal nodes answer
    // getName(); SFNode wrappers answer getNodeName()); null otherwise -- the
    // resolver then falls through to the description/index tiers.
    let name = null;
    try { name = (vp.getName && vp.getName()) || (vp.getNodeName && vp.getNodeName()) || null; } catch { /* ignore */ }
    let description = null;
    try { description = (vp.getDescriptions() || []).join(' » ') || null; } catch { /* ignore */ }
    return { name, description, index: i };
  }

  // The currently-BOUND viewpoint's identity (or null when the default view is
  // active). Captured before a preserving refresh; also exposed for QA.
  function captureViewState() {
    let index = -1;
    viewpointNodes.forEach((vp, i) => {
      try { if (vp._isBound && vp._isBound.getValue()) index = i; } catch { /* ignore */ }
    });
    if (index < 0) return null;
    return describeViewpoint(viewpointNodes[index], index);
  }

  function selectViewpoint(i, opts) {
    const layer = activeLayer();
    const vp = viewpointNodes[i];
    if (!layer || !vp) return;
    try { browser.bindViewpoint(layer, vp); } catch (err) { setStatus('Could not bind viewpoint: ' + (err && err.message || err), 'warn'); }
    if (opts && opts.syncSelect) {
      const sel = el('wpViewpoint');
      if (sel && !sel.disabled) sel.value = String(i);
    }
  }
  function resetView() {
    try {
      const vp = browser.getActiveViewpoint();
      if (vp && typeof vp.resetUserOffsets === 'function') vp.resetUserOffsets();
    } catch { /* ignore */ }
  }

  // ---- pre-validation (X_ITE as the authority, without touching the scene) ---
  // Parse `text` through X_ITE WITHOUT replacing the displayed world. Used by
  // the editor lane before a nested-override refresh: X_ITE loads a failed
  // Inline as an async warning, so an unparseable nested buffer would otherwise
  // replace the full scene with that piece missing instead of keeping the last
  // good version. Parser diagnostics are NOT consulted -- X_ITE decides.
  async function validateText(text) {
    try {
      await ensureBrowser();
    } catch (err) {
      return { ok: false, initError: String((err && err.message) || err) };
    }
    try {
      await browser.createX3DFromString(String(text == null ? '' : text));
      return { ok: true };
    } catch (err) {
      // Same separation as load(): a dead graphics context is never reported as
      // the edited text being invalid.
      if (!Readiness.isBrowserUsable(browser)) {
        browser = null; ready = null;
        discardPickAdapter();
        return { ok: false, initError: '3D preview lost its graphics context; the text was not validated.', detail: String((err && err.message) || err) };
      }
      return { ok: false, error: String((err && err.message) || err) };
    }
  }

  // ---- navigation mode (optional; feature-detected) --------------------------
  function setNavigation(mode) {
    try {
      const nav = browser.getActiveNavigationInfo();
      if (nav) nav.type = [mode, 'ANY'];
    } catch { /* X_ITE without a settable NavigationInfo -> ignore */ }
  }

  // ---- meta / warnings panel -------------------------------------------------
  function renderMeta() {
    if (!meta) return;
    const c = meta.counts || {};
    const loadedCount = (c.presentAssets || 0);
    const missingCount = (c.missing || 0) + (c.caseMismatches || 0);
    const lm = el('wpLoadedMissing');
    if (lm) lm.textContent = `${loadedCount} local asset(s) loaded · ${missingCount} unavailable (missing/case)`;
    const src = el('wpPrimary');
    if (src) src.textContent = `${meta.primaryRel || '—'} ${meta.wasGzipped ? '(gzip)' : '(plain)'}`;

    const box = el('wpWarnings');
    if (!box) return;
    const items = [];
    // Buffer-scoped findings from the editor live preview (Phase 7C3): a payload
    // carries `.buffer` only on the editor lane -- the workspace disk preview
    // never sees this branch. New references are surfaced, never auto-loaded.
    if (meta.buffer && meta.buffer.newRefs && meta.buffer.newRefs.length) {
      items.push(`<div class="warn">New file reference found — choose Find new files: ${meta.buffer.newRefs.map(esc).join(', ')}</div>`);
    }
    if (meta.stale) items.push(`<div class="warn err">Showing the last valid scene (a rescan/refresh did not fully succeed).</div>`);
    if (meta.remoteUrls && meta.remoteUrls.length) {
      items.push(`<div class="warn err">Remote URL(s) blocked (never fetched): ${meta.remoteUrls.map(esc).join(', ')}</div>`);
    }
    if (meta.unsafeRefs && meta.unsafeRefs.length) {
      items.push(`<div class="warn err">Unsafe path(s) refused (absolute / escapes project root): ${meta.unsafeRefs.map(esc).join(', ')}</div>`);
    }
    if (meta.missingAssets && meta.missingAssets.length) {
      items.push(`<div class="warn">Missing local asset(s): ${meta.missingAssets.slice(0, 12).map(esc).join(', ')}${meta.missingAssets.length > 12 ? ` +${meta.missingAssets.length - 12}` : ''}</div>`);
    }
    if (meta.caseMismatches && meta.caseMismatches.length) {
      items.push(`<div class="warn">Case mismatch (breaks on case-sensitive servers): ${meta.caseMismatches.slice(0, 8).map((c) => esc(c.referenced) + ' → ' + esc(c.actual)).join(', ')}</div>`);
    }
    for (const w of [...new Set(runtimeWarnings)].slice(0, 12)) items.push(`<div class="warn">Runtime: ${esc(w)}</div>`);
    box.innerHTML = items.join('');
  }

  function esc(s) {
    return String(s).replace(/[&<>"]/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[ch]));
  }

  function debugState() {
    return {
      status: meta ? meta.status : 'none',
      haveValidScene,
      wasGzipped: meta ? !!meta.wasGzipped : false,
      baseURL: meta ? meta.baseURL : null,
      viewpoints: viewpointNodes.map((vp, i) => viewpointLabel(vp, i)),
      boundViewpoint: captureViewState(),
      navChoice: userNav,
      counts: meta ? meta.counts : null,
      remoteUrls: meta ? meta.remoteUrls : [],
      missingAssets: meta ? meta.missingAssets : [],
      buffer: meta && meta.buffer ? meta.buffer : null,
      editedRel: meta && meta.editedRel != null ? meta.editedRel : null,
      editedIsPrimary: meta && meta.editedIsPrimary != null ? !!meta.editedIsPrimary : null,
      runtimeWarnings: [...new Set(runtimeWarnings)],
    };
  }

  // ---- wiring ----------------------------------------------------------------
  function wire() {
    const rf = el('wpRefresh'); if (rf) rf.addEventListener('click', () => load());
    const rv = el('wpReset'); if (rv) rv.addEventListener('click', resetView);
    const vp = el('wpViewpoint'); if (vp) vp.addEventListener('change', (e) => selectViewpoint(Number(e.target.value)));
    const nav = el('wpNav');
    if (nav) {
      nav.addEventListener('change', (e) => {
        userNav = e.target.value; // remembered so a preserving refresh can restore it
        setNavigation(e.target.value);
      });
    }
  }

  // Public API (used by world.js, the editor live-preview lane, and the
  // read-only visual-QA capture harness).
  window.wrlWorldPreview = {
    load, wire, resetView, discoverViewpoints, validateText, _debug: debugState,
    // WD2-D: this browser's pick adapter + its <x3d-canvas>, or null.
    pickTarget: () => (pickAdapter && browser
      ? { adapter: pickAdapter, element: el('wpCanvas'), retire: retirePick, currentCheck: pickCurrentCheck } : null),
  };

  // QA hook: load (optionally select a viewpoint / reset) and return debug JSON.
  // Read-only; adds no capability beyond what world:previewLoad already returns.
  window.__wrlForgeWorldPreview = async function (opts) {
    const dbg = await load();
    if (opts && Number.isInteger(opts.viewpoint)) { selectViewpoint(opts.viewpoint); }
    if (opts && opts.reset) { resetView(); }
    return dbg;
  };

  document.addEventListener('DOMContentLoaded', wire);
})();
