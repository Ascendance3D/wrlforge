'use strict';
// SHELL-0 application contribution contract
// (docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md §2;
//  APP-ARCH-0 §16-§17).
//
// A feature module is a contribution:
//
//   module.exports = {
//     id: 'workspace.commands',
//     contribute(app) {
//       app.commands.register({ id: 'workspace.reset', ... });
//       app.disposables.add(listen(window, 'resize', onResize));
//     },
//   };
//
// The shell calls `application.contribute(contribution)` once per module.
// Nothing is registered in a central file.
//
// Ownership, which is the point of the contract:
//   * `app` is a per-contribution SCOPED view of the services. Every call to a
//     service method the catalog marks `tracked` (register, subscribe) has its
//     cleanup handle taken into the contribution's own disposable store, so a
//     contribution cannot discard an unsubscribe even by accident.
//   * The handle returned to the contribution still works: calling it early
//     releases the registration from the store too (no double cleanup).
//   * `app.disposables` takes anything else the contribution attaches (DOM
//     listeners via disposable.listen, timers, mounted UI).
//   * contribute() may also return a function or disposable; it is owned too.
//   * contribute() is synchronous and atomic: if it throws (or returns a
//     promise) everything it registered so far is disposed before the error
//     propagates. A half-registered feature never stays live.
//   * Disposing the application disposes contributions in reverse order.
//
// Pure: no DOM, no Electron. No dependency-injection container -- the shell
// builds each service itself and hands the instances to createApplication().

(function () {
  const isNode = typeof module !== 'undefined' && module.exports;
  const D = isNode ? require('./disposable') : window.WrlShellDisposable;
  const SV = isNode ? require('./services') : window.WrlShellServices;

  const CONTRIBUTION_ID_PATTERN = /^[a-z][a-zA-Z0-9]*(\.[a-z][a-zA-Z0-9]*)*$/;

  function contributionError(code, message, cause) {
    const e = new Error(`${code}: ${message}`);
    e.code = code;
    if (cause !== undefined) e.cause = cause;
    return e;
  }

  // One service, viewed through one contribution's store.
  function scopeService(name, svc, store) {
    const tracked = new Set(SV.SERVICE_CATALOG[name].tracked);
    const view = {};
    for (const key of Object.keys(svc)) {
      const value = svc[key];
      if (typeof value !== 'function') {
        Object.defineProperty(view, key, { enumerable: true, get: () => svc[key] });
        continue;
      }
      if (!tracked.has(key)) {
        view[key] = value.bind(svc);
        continue;
      }
      view[key] = function trackedCall(...args) {
        const result = value.apply(svc, args);
        const owned = store.add(D.asDisposable(result));
        const release = () => { store.release(owned); };
        return typeof result === 'function' ? release : Object.freeze({ dispose: release });
      };
    }
    return Object.freeze(view);
  }

  function validateContribution(c) {
    if (!c || typeof c !== 'object') throw contributionError('ECONTRIBUTION_INVALID', 'a contribution must be an object');
    if (typeof c.id !== 'string' || !CONTRIBUTION_ID_PATTERN.test(c.id)) {
      throw contributionError('ECONTRIBUTION_INVALID', `invalid contribution id ${JSON.stringify(c.id)}`);
    }
    if (typeof c.contribute !== 'function') throw contributionError('ECONTRIBUTION_INVALID', `${c.id}: contribute(app) is required`);
  }

  function createApplication({ services } = {}) {
    const instances = SV.validateServices(services);
    const live = new Map(); // id -> { store }
    let disposed = false;

    function scopedApp(id, store) {
      const app = { contributionId: id, disposables: store };
      for (const [name, svc] of Object.entries(instances)) app[name] = scopeService(name, svc, store);
      return Object.freeze(app);
    }

    function contribute(contribution) {
      if (disposed) throw contributionError('EAPP_DISPOSED', 'the application has been disposed');
      validateContribution(contribution);
      const { id } = contribution;
      if (live.has(id)) throw contributionError('ECONTRIBUTION_DUPLICATE', `contribution ${id} is already active`);
      const store = D.createDisposableStore();
      let result;
      try {
        result = contribution.contribute(scopedApp(id, store));
        if (result && typeof result.then === 'function') {
          throw contributionError('ECONTRIBUTION_ASYNC', `${id}: contribute(app) must be synchronous`);
        }
        if (result !== undefined) store.add(D.asDisposable(result));
      } catch (err) {
        try { store.dispose(); } catch (cleanupErr) { /* report the original failure */ void cleanupErr; }
        if (err && err.code === 'ECONTRIBUTION_ASYNC') throw err;
        throw contributionError('ECONTRIBUTION_FAILED', `${id}: contribute(app) threw; its registrations were rolled back`, err);
      }
      const entry = { store };
      live.set(id, entry);
      return Object.freeze({
        id,
        dispose() {
          if (live.get(id) !== entry) return;
          live.delete(id);
          store.dispose();
        },
      });
    }

    function has(id) { return live.has(id); }
    function list() { return [...live.keys()]; }
    // Owned registrations of one contribution (diagnostics / leak tests).
    function ownedCount(id) { const e = live.get(id); return e ? e.store.size : 0; }

    function dispose() {
      if (disposed) return;
      disposed = true;
      const entries = [...live.values()];
      live.clear();
      const errors = [];
      for (let i = entries.length - 1; i >= 0; i--) {
        try { entries[i].store.dispose(); } catch (err) { errors.push(err); }
      }
      if (errors.length === 1) throw errors[0];
      if (errors.length > 1) throw new AggregateError(errors, 'EAPP_DISPOSE_FAILED: several contributions failed to dispose');
    }

    return Object.freeze({ contribute, has, list, ownedCount, dispose, services: instances });
  }

  const WRL_SHELL_CONTRIBUTION_API = Object.freeze({ CONTRIBUTION_ID_PATTERN, createApplication });

  if (isNode) {
    module.exports = WRL_SHELL_CONTRIBUTION_API;
  } else {
    window.WrlShellContribution = WRL_SHELL_CONTRIBUTION_API;
  }
})();
