'use strict';
// SHELL-0 contextual-panel contribution boundary
// (docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md §9; APP-ARCH-0 §19).
//
// The `app.contextualPanels` service: selection-specific editors (Transform,
// Extrusion, IndexedFaceSet, Material ...) contributed as
//
//   { id, title, appliesTo(selectionInfo, analysis), mount(host, ctx),
//     update?(ctx), dispose() }
//
// and mounted into ONE Context host by reconcile(), which the shell calls on
// every sceneSelection change and every analysis (watch() wires that):
//
//   * The question asked is exactly "does this contribution apply to the
//     current PROVEN selection?". resolveContext() supplies
//     { selection, analysis } from the document session's EXISTING authorities
//     (sceneSelection + the analysis products) -- this module never parses,
//     never builds a scene tree and never keeps its own selection.
//   * Fail closed (WD.md §7): no context, no selection, or a selection whose
//     identity is not proven (`selection.proven !== true`) applies NOTHING, and
//     every mounted editor is disposed. A throwing appliesTo does not apply.
//   * Newly applicable -> createHost(id) + mount(host, ctx). Still applicable ->
//     update(ctx) if provided. No longer applicable -> dispose() + releaseHost.
//     A mount that throws leaves the editor unmounted (its host is released;
//     dispose() is NOT called -- mount must not leave partial resources behind
//     when it throws). An update that throws is disposed (fail closed). A
//     throwing dispose still clears the active state and releases the host.
//     One record's failure never stops the others from reconciling.
//   * Re-activation is a NEW mount: after a dispose, the next applicable
//     selection gets a fresh createHost(id) and a fresh mount(host, ctx).
//     Nothing from the previous activation is handed back.
//   * Editors write only through planners -> applyVerifiedEdits (APP-ARCH-0
//     §19); nothing here can edit source.
//
// None of the example editors is implemented in SHELL-0.

(function () {
  const CONTEXTUAL_ID_PATTERN = /^[a-z][a-zA-Z0-9]*$/;

  function contextualError(code, message) {
    const e = new Error(`${code}: ${message}`);
    e.code = code;
    return e;
  }

  function createContextualPanelHost({ resolveContext, createHost, releaseHost } = {}) {
    if (typeof resolveContext !== 'function') throw contextualError('ECONTEXTUAL_INVALID', 'resolveContext() is required');
    if (typeof createHost !== 'function') throw contextualError('ECONTEXTUAL_INVALID', 'createHost(id) is required');
    const release = typeof releaseHost === 'function' ? releaseHost : () => {};
    const records = new Map(); // id -> record (registration order)
    const mountedHosts = new Map(); // id -> host

    function validate(r) {
      const bad = (why) => { throw contextualError('ECONTEXTUAL_INVALID', why); };
      if (!r || typeof r !== 'object') bad('a contextual panel must be an object');
      if (typeof r.id !== 'string' || !CONTEXTUAL_ID_PATTERN.test(r.id)) bad(`invalid contextual panel id ${JSON.stringify(r.id)}`);
      if (typeof r.title !== 'string' || r.title.trim() === '') bad(`${r.id}: title is required`);
      for (const fn of ['appliesTo', 'mount', 'dispose']) if (typeof r[fn] !== 'function') bad(`${r.id}: ${fn} must be a function`);
      if (r.update !== undefined && typeof r.update !== 'function') bad(`${r.id}: update must be a function`);
    }

    function unmountOne(id) {
      const host = mountedHosts.get(id);
      if (host === undefined) return false;
      mountedHosts.delete(id);
      try { records.get(id).dispose(); } finally { release(id, host); }
      return true;
    }

    function register(record) {
      validate(record);
      if (records.has(record.id)) throw contextualError('ECONTEXTUAL_DUPLICATE', `contextual panel ${record.id} is already registered`);
      records.set(record.id, record);
      return function unregister() {
        if (records.get(record.id) !== record) return;
        try { unmountOne(record.id); } finally { records.delete(record.id); }
      };
    }

    function applies(record, ctx) {
      if (!ctx || !ctx.selection || ctx.selection.proven !== true) return false;
      try { return !!record.appliesTo(ctx.selection, ctx.analysis); } catch (err) {
        console.error(`[contextual-panels] ${record.id} appliesTo failed:`, err);
        return false;
      }
    }

    // One record's failure never stops the others from being reconciled.
    function reconcile() {
      const ctx = resolveContext();
      const out = { mounted: [], updated: [], disposed: [], failed: [] };
      for (const [id, record] of records) {
        const want = applies(record, ctx);
        const have = mountedHosts.has(id);
        if (!want && have) {
          try {
            unmountOne(id);
            out.disposed.push(id);
          } catch (err) {
            // unmountOne already dropped the active state and released the host
            console.error(`[contextual-panels] ${id} dispose failed:`, err);
            out.failed.push(id);
          }
        } else if (want && !have) {
          const host = createHost(id);
          try {
            record.mount(host, ctx);
            mountedHosts.set(id, host);
            out.mounted.push(id);
          } catch (err) {
            release(id, host);
            console.error(`[contextual-panels] ${id} mount failed:`, err);
            out.failed.push(id);
          }
        } else if (want && have && record.update) {
          try {
            record.update(ctx);
            out.updated.push(id);
          } catch (err) {
            // fail closed: an editor that could not take the new context is
            // torn down rather than left showing stale state
            console.error(`[contextual-panels] ${id} update failed:`, err);
            try { unmountOne(id); } catch (err2) { console.error(`[contextual-panels] ${id} dispose failed:`, err2); }
            out.failed.push(id);
          }
        }
      }
      return out;
    }

    function mounted() { return [...mountedHosts.keys()]; }

    // Wire reconcile() to change sources. Each source is a subscribe(fn)
    // function returning its unsubscribe (sceneSelection.subscribe, an analysis
    // notifier). Returns ONE cleanup for all of them.
    function watch(...subscribeFns) {
      const offs = subscribeFns.map((sub) => sub(() => { reconcile(); }));
      let done = false;
      return function unwatch() {
        if (done) return;
        done = true;
        for (let i = offs.length - 1; i >= 0; i--) offs[i]();
      };
    }

    // Every mounted editor is disposed even if some throw; the errors are
    // re-thrown afterwards (one error, or one AggregateError), never swallowed.
    function dispose() {
      const errors = [];
      for (const id of [...mountedHosts.keys()].reverse()) {
        try { unmountOne(id); } catch (err) { errors.push(err); }
      }
      records.clear();
      if (errors.length === 1) throw errors[0];
      if (errors.length > 1) throw new AggregateError(errors, 'ECONTEXTUAL_DISPOSE_FAILED: several contextual panels threw');
    }

    return Object.freeze({ register, reconcile, mounted, watch, dispose });
  }

  const WRL_SHELL_CONTEXTUAL_API = Object.freeze({ CONTEXTUAL_ID_PATTERN, createContextualPanelHost });

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = WRL_SHELL_CONTEXTUAL_API;
  } else {
    window.WrlShellContextualPanels = WRL_SHELL_CONTEXTUAL_API;
  }
})();
