'use strict';
// Phase 7C2 + 7C3 -- the in-editor live-preview orchestrator. Thin DOM glue on
// top of pure, unit-tested pieces:
//   * window.WrlPreviewState     -- the last-valid-scene state machine (7C1)
//   * window.WrlPreviewScheduler -- the 700 ms debounce / coalescing model (7C1)
//   * window.WrlEditorUI         -- pure layout + status view-models (ui-state.js)
//   * window.wrlPreview          -- the REUSED Mall X_ITE render + fit engine
//                                   (preview.js), driven with an injected source
//   * window.wrlWorldPreview     -- the REUSED World X_ITE render engine
//                                   (world-preview.js), same injected-source
//                                   pattern, plus opt-in viewpoint preservation
//   * window.vrmlpad.editor      -- the confined main-process preview bridge
//
// The profile comes from the OPEN document's context ('mall' | 'world'); the
// matching engine renders, and the profile-specific controls show. The renderer
// NEVER supplies a filesystem path: it sends only { sessionId, text,
// bufferVersion }. Main authorizes the session against its own authority (the
// held Mall source, or the World scan graph), byte-substitutes the unsaved
// buffer through the overlay, and returns the payload. One render runs at a
// time (serial in-flight), so completions can never land out of order; the
// overlay's generation check is the belt-and-suspenders authority.

(function () {
  const PS = window.WrlPreviewState;
  const SCHED = window.WrlPreviewScheduler;
  const UI = window.WrlEditorUI;
  const bridge = window.vrmlpad.editor;

  const el = (id) => document.getElementById(id);
  const encoder = typeof TextEncoder !== 'undefined' ? new TextEncoder() : null;
  function byteLen(text) {
    return encoder ? encoder.encode(text).length : unescape(encodeURIComponent(text)).length;
  }

  // Phase: Preferences & Settings -- the preview LAYOUT is now a shared
  // preferences value (window.WrlPreferences). The split fraction remains a
  // local cosmetic detail of the divider (not a global user preference), so
  // it keeps its own localStorage key. Layout edits from the Preferences
  // dialog reach this orchestrator through the shared model -- the
  // orchestrator's own applyLayout() is the single live applier.
  const LAYOUT_KEY = 'wrlforge.editor.previewLayout';
  const SPLIT_KEY = 'wrlforge.editor.previewSplit';
  function savedLayout() {
    if (window.WrlPreferences) {
      return UI.resolvePreviewLayout(window.WrlPreferences.get('previewLayout'));
    }
    try { return UI.resolvePreviewLayout(window.localStorage.getItem(LAYOUT_KEY)); }
    catch (e) { return UI.PREVIEW_LAYOUT_DEFAULT; }
  }
  function savedSplit() {
    try { return UI.clampSplit(window.localStorage.getItem(SPLIT_KEY)); }
    catch (e) { return UI.SPLIT_DEFAULT; }
  }
  function persistLayout(l) {
    if (window.WrlPreferences) window.WrlPreferences.set('previewLayout', l);
    else try { window.localStorage.setItem(LAYOUT_KEY, l); } catch (e) { /* best-effort */ }
  }
  function persistSplit(f) { try { window.localStorage.setItem(SPLIT_KEY, String(f)); } catch (e) { /* best-effort */ } }

  // --- orchestrator state ----------------------------------------------------
  const St = {
    active: false,
    sessionId: null,
    context: 'mall',        // the open document's profile: 'mall' | 'world'
    getText: () => '',
    getVersion: () => 0,
    sm: PS.createPreviewState(),
    scheduler: SCHED.createPreviewScheduler({ debounceMs: 700 }),
    timer: null,
    inFlight: false,
    displaySaved: false,
    sizeTier: 'auto',       // last known size band, for the status chip
    newRefs: 0,             // buffer references not yet in the World graph
    lastRenderMs: null,     // last scene-replacement duration (QA/perf evidence)
    layout: 'split',
    split: 0.5,
    // WD2-D viewport picking: armed only in the Model workspace (editor.js).
    pickArmed: false,
    pickHandlers: null,     // { onPick(snapshot, currentCheck), onCompatibility(c) }
    pickDown: null,         // the pending primary pointerdown { snapshot, x, y, pointerId, check }
    pickListening: false,
    renderedArmed: false,
    editedIsPrimary: true,  // World: false when the root string is NOT the edited file
    rescanning: false,      // "Find new files" in flight (the command's enabled state)
  };

  // UI-0: the layout / rescan state behind the preview commands changed. The
  // editor page's command registry re-reads enabled/checked on this event
  // (editor.js loads first, so a direct subscription is not possible here).
  function notifyStateChange() {
    if (typeof document.dispatchEvent === 'function' && typeof CustomEvent === 'function') {
      document.dispatchEvent(new CustomEvent('wrl-editor-preview-state'));
    }
  }

  function nowMs() { return Date.now(); }

  // The render engine for the open document's profile. Both are the REUSED
  // page-scope controllers (never forked); only one is ever driven per document.
  function engine() {
    return St.context === 'world' ? window.wrlWorldPreview : window.wrlPreview;
  }

  // --- status chip -----------------------------------------------------------
  function paintChip() {
    const chip = el('previewChip');
    if (!chip) return;
    const model = UI.previewStatusModel({
      state: St.sm.state,
      failureCategory: St.sm.failureCategory,
      saved: St.displaySaved,
      sizeTier: St.sizeTier,
      newRefs: St.newRefs,
    });
    chip.textContent = model.label;
    chip.className = 'preview-chip tone-' + model.tone;
    chip.setAttribute('data-state', model.key);
  }

  // --- layout ----------------------------------------------------------------
  function applyLayout() {
    const m = UI.previewLayoutModel(St.layout, St.split);
    St.layout = m.layout; St.split = m.split;
    const main = el('editorMain');
    if (main) {
      main.classList.toggle('layout-split', m.layout === 'split');
      main.classList.toggle('layout-preview-max', m.layout === 'preview-max');
      main.classList.toggle('layout-editor-only', m.layout === 'editor-only');
      main.style.setProperty('--wrl-split', String(m.split));
    }
    const divider = el('previewDivider');
    if (divider) {
      divider.setAttribute('aria-valuenow', String(m.splitPercent));
      divider.style.display = m.layout === 'split' ? '' : 'none';
    }
    // The pressed state and the layout select are painted from the command
    // registry (preview.toggleMaximize / preview.layout.*); the label stays here.
    const maxBtn = el('previewMaxBtn');
    if (maxBtn) maxBtn.textContent = m.maximized ? 'Restore' : 'Maximize';
    persistLayout(m.layout); persistSplit(m.split);
    notifyStateChange();
    // Entering a layout that shows the preview for the first time: render it.
    if (m.previewVisible && St.active && St.sm.displayedGeneration === 0 && !St.inFlight) {
      requestUpdate('manual');
    }
  }

  function setLayout(mode) { St.layout = UI.resolvePreviewLayout(mode); applyLayout(); }
  function toggleMaximize() { St.layout = UI.togglePreviewMaximize(St.layout); applyLayout(); }
  function stepSplit(delta) { St.split = UI.splitStep(St.split, delta); applyLayout(); }

  // --- divider (mouse + keyboard) -------------------------------------------
  function wireDivider() {
    const divider = el('previewDivider');
    const main = el('editorMain');
    if (!divider || !main) return;
    let dragging = false;
    const onMove = (e) => {
      if (!dragging) return;
      const rect = main.getBoundingClientRect();
      if (rect.width <= 0) return;
      const frac = (e.clientX - rect.left) / rect.width;
      St.split = UI.clampSplit(frac);
      main.style.setProperty('--wrl-split', String(St.split));
      const pct = Math.round(St.split * 100);
      divider.setAttribute('aria-valuenow', String(pct));
    };
    const onUp = () => {
      if (!dragging) return;
      dragging = false;
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      persistSplit(St.split);
    };
    divider.addEventListener('mousedown', (e) => {
      if (St.layout !== 'split') return;
      dragging = true; e.preventDefault();
      document.addEventListener('mousemove', onMove);
      document.addEventListener('mouseup', onUp);
    });
    divider.addEventListener('keydown', (e) => {
      if (e.key === 'ArrowLeft') { stepSplit(-0.05); e.preventDefault(); }
      else if (e.key === 'ArrowRight') { stepSplit(+0.05); e.preventDefault(); }
      else if (e.key === 'Home') { St.split = UI.SPLIT_MIN; applyLayout(); e.preventDefault(); }
      else if (e.key === 'End') { St.split = UI.SPLIT_MAX; applyLayout(); e.preventDefault(); }
    });
    // Register the div listeners for deterministic teardown.
    St._dividerCleanup = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
    };
  }

  // --- the auto/manual update pipeline --------------------------------------
  function armTimer(dueAt) {
    if (St.timer) { clearTimeout(St.timer); St.timer = null; }
    St.timer = setTimeout(pump, Math.max(0, dueAt - nowMs()));
  }

  function pump() {
    St.timer = null;
    if (!St.active) return;
    const p = St.scheduler.poll(St.sessionId, nowMs());
    if (!p.fire) { if (p.dueAt != null) armTimer(p.dueAt); return; }
    fire(p.kind);
  }

  // The editor buffer changed: bump to Outdated, then either schedule a debounced
  // auto-refresh (<=1 MiB) or fall to manual-only (>1 MiB) with plain wording.
  function onEdit() {
    if (!St.active) return;
    St.displaySaved = false;
    const version = St.getVersion();
    const bytes = byteLen(St.getText());
    St.sm = PS.edit(St.sm, version);
    const r = St.scheduler.requestAuto(St.sessionId, { bufferVersion: version, byteLength: bytes, at: nowMs() });
    if (!r.scheduled) {
      // Over the auto threshold: manual Update only.
      St.sizeTier = 'manual';
      paintChip();
      return;
    }
    St.sizeTier = 'auto';
    armTimer(r.dueAt);
    paintChip();
  }

  // Manual Update (button / Ctrl+Enter): bypass the debounce entirely.
  function manualUpdate() {
    if (!St.active) return;
    St.displaySaved = false;
    const version = St.getVersion();
    St.scheduler.requestManual(St.sessionId, { bufferVersion: version, at: nowMs() });
    if (St.timer) { clearTimeout(St.timer); St.timer = null; }
    pump();
  }

  // Explicitly ask for one render now (used when a hidden preview is first shown).
  function requestUpdate(kind) {
    if (kind === 'manual') manualUpdate(); else onEdit();
  }

  async function fire() {
    if (!St.active || St.inFlight) return;
    St.inFlight = true;
    const version = St.getVersion();
    const text = St.getText();
    let res;
    try {
      res = await bridge.previewLoad(St.sessionId, text, version);
    } catch (e) {
      St.inFlight = false;
      St.sm = PS.fail(St.sm, St.sm.requestedGeneration, 'parser');
      paintChip();
      return afterFire();
    }
    if (!res || !res.ok) {
      St.inFlight = false;
      handleRefusal(res);
      return afterFire();
    }
    const gen = res.generation;
    St.sizeTier = res.sizeTier || 'auto';
    St.newRefs = res.buffer && Array.isArray(res.buffer.newRefs) ? res.buffer.newRefs.length : 0;
    paintWorldIdentity(res);
    // WD2-D: the displayed scene is about to change -- release any pending
    // provenance capture and retire the active pick map BEFORE parsing.
    retirePicking('preview-shows-last-valid-scene');
    St.editedIsPrimary = !(St.context === 'world' && res.editedIsPrimary === false);
    const provenance = St.pickArmed && St.editedIsPrimary ? { sessionId: St.sessionId, generationId: gen } : null;
    St.sm = PS.beginUpdate(St.sm, gen, res.bufferVersion);
    paintChip(); // "Updating…"

    let result;
    const t0 = nowMs();
    try {
      // A NESTED World edit is pre-validated through X_ITE (never the parser)
      // BEFORE the world is replaced: X_ITE treats a failed Inline as an async
      // warning, so skipping this would swap in a full scene with the edited
      // piece silently missing instead of keeping the last good version.
      if (St.context === 'world' && res.editedIsPrimary === false && typeof res.editedText === 'string') {
        const v = await window.wrlWorldPreview.validateText(res.editedText);
        if (!v.ok) throw new Error(v.error || 'nested text rejected');
      }
      // World refreshes preserve the user's viewpoint/navigation where possible.
      result = await engine().load({ source: async () => res, preserveView: St.context === 'world', provenance });
    } catch (e) {
      result = { ok: false, parseError: String((e && e.message) || e) };
    }
    St.lastRenderMs = nowMs() - t0;
    // Confirm the generation with main (older/replayed generations are refused
    // there); our serial pipeline means this is always the in-flight one.
    try { await bridge.previewAccept(St.sessionId, gen); } catch (e) { /* best-effort */ }

    if (result && result.ok) {
      St.sm = PS.succeed(St.sm, gen, res.bufferVersion, 'buffer');
      St.displaySaved = false;
      St.renderedArmed = St.pickArmed; // provenance was requested where provable
    } else if (result && result.parseError) {
      // X_ITE could not parse the newest text -- preview.js kept the last valid
      // scene on screen. Surface "showing last good version" (or "can't display").
      St.sm = PS.fail(St.sm, gen, 'scene-load');
    } else {
      St.sm = PS.fail(St.sm, gen, 'scene-load');
    }
    paintChip();
    St.inFlight = false;
    reportPickCompatibility();
    afterFire();
  }

  // After a render settles, honour any newer coalesced edit.
  function afterFire() {
    if (!St.active) return;
    const pend = St.scheduler.pendingFor(St.sessionId);
    if (!pend) return;
    const now = nowMs();
    if (now >= pend.dueAt) pump(); else armTimer(pend.dueAt);
  }

  function handleRefusal(res) {
    const reason = res && res.reason;
    if (reason === 'too-large') {
      St.sizeTier = 'refused';
      // Leave any existing scene up; the chip explains the save-then-open path.
      paintChip();
      return;
    }
    // Authorization / session problems: keep last valid, show a soft failure.
    St.sm = PS.fail(St.sm, St.sm.requestedGeneration, 'scene-load');
    paintChip();
  }

  // "Show saved version": render the on-disk source, not the buffer. For a World
  // document this renders the FULL world entirely from disk (main skips the
  // overlay for this render); the unsaved buffer and dirty state are untouched,
  // and a later Update returns to the unsaved version.
  async function showSaved() {
    if (!St.active) return;
    let res;
    try { res = await bridge.previewSaved(St.sessionId); }
    catch (e) { return; }
    if (!res || !res.ok) return;
    paintWorldIdentity(res);
    retirePicking('preview-scene-replaced'); // the saved file is not the buffer
    St.renderedArmed = false;
    try {
      await engine().load({ source: async () => res });
      St.displaySaved = true;
      paintChip();
    } catch (e) { /* the engine kept the last valid scene */ }
  }

  // Identify what the World pane is showing: always the FULL project, and which
  // document inside it is being edited. Mall documents leave the line untouched.
  function paintWorldIdentity(res) {
    if (St.context !== 'world' || !res) return;
    const line = el('epEditedLine');
    if (!line) return;
    if (res.primaryRel) {
      line.textContent = res.editedIsPrimary || !res.editedRel
        ? `Full World Project preview — primary: ${res.primaryRel}`
        : `Full World Project preview — primary: ${res.primaryRel} · editing: ${res.editedRel}`;
    }
  }

  // Explicit "Find new files" (World only): main reruns its own project scan --
  // no path crosses IPC -- then a fresh Update renders against the new graph.
  async function findNewFiles() {
    if (!St.active || St.context !== 'world') return;
    St.rescanning = true; notifyStateChange();
    let res = null;
    try { res = await bridge.previewRescan(St.sessionId); } catch (e) { res = null; }
    St.rescanning = false; notifyStateChange();
    if (res && res.ok) manualUpdate();
  }

  // --- WD2-D viewport picking -------------------------------------------------
  // Pointer glue only. The engine's pick adapter (the one private X_ITE module)
  // turns a click into a plain-data snapshot; editor.js resolves it through the
  // pure resolver and the ONE selection authority. Never preventDefault /
  // stopPropagation: X_ITE's own navigation, sensors and Anchors are untouched.
  const PICK_SLOP_PX = 4;

  function pickTargets() {
    const out = [];
    for (const eng of [window.wrlPreview, window.wrlWorldPreview]) {
      const t = eng && typeof eng.pickTarget === 'function' ? eng.pickTarget() : null;
      if (t) out.push(t);
    }
    return out;
  }

  // Abort any pending provenance capture and retire the displayed pick map of
  // every engine (both X_ITE browsers on the page). Adapters are disposed only
  // with their browser (engines) or at page teardown (pagehide below).
  function retirePicking(reason) {
    for (const t of pickTargets()) t.retire(reason);
  }

  function activeTarget() {
    const t = engine() && typeof engine().pickTarget === 'function' ? engine().pickTarget() : null;
    return t && t.element ? t : null;
  }

  function onPickDown(e) {
    St.pickDown = null;
    if (!St.pickArmed || !St.active || !St.pickHandlers || e.button !== 0 || e.isPrimary === false) return;
    const canvasEl = document.getElementById(St.context === 'world' ? 'wpCanvas' : 'preview');
    const path = typeof e.composedPath === 'function' ? e.composedPath() : [e.target];
    if (!canvasEl || !path.includes(canvasEl)) return;
    const t = activeTarget();
    let snapshot;
    let check = () => 'hit-from-another-preview-generation';
    if (St.displaySaved) snapshot = { outcome: 'stale', reason: 'preview-scene-replaced' };
    else if (St.context === 'mall' && window.wrlPreview && window.wrlPreview.currentMode() === 'fit') {
      snapshot = { outcome: 'unsupported', reason: 'preview-is-not-the-document' };
    } else if (!St.editedIsPrimary) snapshot = { outcome: 'external', reason: 'preview-root-is-another-document' };
    else if (!t || t.element !== canvasEl) snapshot = { outcome: 'stale', reason: 'preview-shows-last-valid-scene' };
    else {
      snapshot = t.adapter.pick(e.clientX, e.clientY);
      check = t.currentCheck;
    }
    St.pickDown = { snapshot, check, x: e.clientX, y: e.clientY, pointerId: e.pointerId };
  }

  function onPickUp(e) {
    const down = St.pickDown;
    St.pickDown = null;
    if (!down || e.pointerId !== down.pointerId || !St.pickArmed || !St.pickHandlers) return;
    // A drag is navigation, never a pick.
    if (Math.abs(e.clientX - down.x) > PICK_SLOP_PX || Math.abs(e.clientY - down.y) > PICK_SLOP_PX) return;
    St.pickHandlers.onPick(down.snapshot, down.check);
  }

  function onPickCancel() { St.pickDown = null; }

  function listenForPicks(on) {
    const col = document.querySelector('.preview-col');
    if (!col || on === St.pickListening) return;
    const m = on ? 'addEventListener' : 'removeEventListener';
    col[m]('pointerdown', onPickDown, true);
    col[m]('pointerup', onPickUp, true);
    col[m]('pointercancel', onPickCancel, true);
    St.pickListening = on;
  }

  // Model workspace on/off. Off (Code) is inert: no listener, no touch(), no
  // provenance requested, and any capture/map is released.
  function armPicking(on, handlers) {
    const next = !!on;
    if (handlers) St.pickHandlers = handlers;
    if (!next) {
      St.pickDown = null;
      if (St.pickArmed) retirePicking('preview-scene-replaced');
      St.pickArmed = false;
      St.renderedArmed = false; // re-arming renders once with provenance
      listenForPicks(false);
      return;
    }
    St.pickArmed = true;
    listenForPicks(true);
    reportPickCompatibility();
    // The displayed scene was rendered while picking was off (Code workspace,
    // Show saved): render the buffer once so picking can prove identities.
    if (St.active && !St.inFlight && St.sm.displayedGeneration > 0 && !St.renderedArmed && !St.displaySaved) {
      manualUpdate();
    }
  }

  function reportPickCompatibility() {
    if (!St.pickArmed || !St.pickHandlers || typeof St.pickHandlers.onCompatibility !== 'function') return;
    const t = activeTarget();
    St.pickHandlers.onCompatibility(t ? t.adapter.compatibility() : null);
  }

  // --- lifecycle -------------------------------------------------------------
  // Called by editor.js once the editor has an open Mall or World document.
  // Idempotent per session; a different session first tears down the previous
  // one (its overlay, timer, and scene can never leak into this document).
  function start({ sessionId, getText, getVersion, context } = {}) {
    if (St.active && St.sessionId === sessionId) return;
    if (St.active) stop();
    St.active = true;
    St.sessionId = sessionId;
    St.context = context === 'world' ? 'world' : 'mall';
    St.getText = typeof getText === 'function' ? getText : (() => '');
    St.getVersion = typeof getVersion === 'function' ? getVersion : (() => 0);
    St.sm = PS.createPreviewState();
    St.scheduler.cancel(sessionId);
    St.inFlight = false;
    St.displaySaved = false;
    St.sizeTier = 'auto';
    St.newRefs = 0;
    St.lastRenderMs = null;
    St.editedIsPrimary = true;
    St.renderedArmed = false;
    applyProfileBody();
    paintChip();
    // Render the initial buffer immediately (unless the preview pane is hidden).
    const m = UI.previewLayoutModel(St.layout, St.split);
    if (m.previewVisible) requestUpdate('manual');
  }

  // Show only the open profile's preview body (Mall: fit modes/guides/report;
  // World: viewpoints/navigation/Find new files). The inactive body is hidden
  // and its X_ITE canvas never receives a scene.
  function applyProfileBody() {
    const col = document.querySelector('.preview-col');
    if (!col) return;
    col.classList.toggle('context-world', St.context === 'world');
    col.classList.toggle('context-mall', St.context !== 'world');
  }

  // Tear down: stop timers, forget the scene, tell main to drop the overlay.
  function stop() {
    if (St.timer) { clearTimeout(St.timer); St.timer = null; }
    retirePicking('preview-scene-replaced');
    St.pickDown = null;
    St.renderedArmed = false;
    if (St._dividerCleanup) St._dividerCleanup();
    const sid = St.sessionId;
    if (sid != null) {
      St.scheduler.cancel(sid);
      try { bridge.previewClose(sid); } catch (e) { /* best-effort */ }
    }
    St.active = false;
    St.inFlight = false;
    St.sm = PS.close(St.sm);
    St.displaySaved = false;
  }

  // --- wiring ----------------------------------------------------------------
  function wire() {
    St.layout = savedLayout();
    St.split = savedSplit();
    wireDivider();
    applyLayout();
    paintChip();
    // Update / Show saved / Maximize / Find new files and the layout select are
    // bound to their commands by editor.js (UI-0 command bindings).

    // Tell main to drop the overlay if the renderer is torn down (reload / close /
    // navigate). This is the renderer-reload cleanup path.
    window.addEventListener('beforeunload', () => {
      if (St.sessionId != null) { try { bridge.previewClose(St.sessionId); } catch (e) { /* ignore */ } }
    });
    // WD2-D page teardown: release every pick adapter (hook, owner, maps).
    window.addEventListener('pagehide', () => {
      listenForPicks(false);
      for (const t of pickTargets()) t.adapter.dispose();
    });
  }

  // Public surface for editor.js + the serialized QA harness. No capability beyond
  // what the page already does through its own controls.
  window.wrlEditorPreview = {
    start, stop, onEdit, manualUpdate, showSaved, findNewFiles,
    setLayout, toggleMaximize, stepSplit, armPicking,
    // UI-0 public read accessors (the command registry must not read _state()).
    getLayout: () => St.layout,
    isRescanning: () => St.rescanning,
    displayedGeneration: () => St.sm.displayedGeneration,
    // QA / introspection (no buffer text exposed).
    _state: () => ({
      state: St.sm.state, failureCategory: St.sm.failureCategory,
      displayedGeneration: St.sm.displayedGeneration, requestedGeneration: St.sm.requestedGeneration,
      haveLastValid: St.sm.haveLastValid, saved: St.displaySaved, sizeTier: St.sizeTier,
      context: St.context, newRefs: St.newRefs, lastRenderMs: St.lastRenderMs,
      layout: St.layout, split: St.split, chip: (el('previewChip') || {}).textContent,
      world: (St.context === 'world' && window.wrlWorldPreview) ? window.wrlWorldPreview._debug() : null,
      picking: {
        armed: St.pickArmed, listening: St.pickListening, renderedArmed: St.renderedArmed,
        compatibility: activeTarget() ? activeTarget().adapter.compatibility() : null,
      },
    }),
    _leak: () => bridge.previewLeak(),
  };

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', wire);
  } else {
    wire();
  }
})();
