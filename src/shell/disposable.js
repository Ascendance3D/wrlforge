'use strict';
// SHELL-0 disposable lifecycle contract
// (docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md §3).
//
// ONE cleanup model for everything a renderer module attaches: event listeners,
// registry entries, subscriptions, mounted UI, and later preview/tool state.
// Today the page reload is the only teardown (APP-ARCH-0 §6); a persistent
// shell turns every discarded unsubscribe into a leak, so every attachment must
// return something this module can own.
//
// Rules:
//   * A disposable is `{ dispose() }`. A bare function is accepted at the
//     boundary and wrapped (today's registries return `unregister()` functions).
//   * dispose() is IDEMPOTENT: the second call is a no-op, never a double free.
//   * A store disposes its children in REVERSE registration order, so a thing
//     registered after (and possibly depending on) another is torn down first.
//   * Every child is disposed even if an earlier one throws; the errors are
//     re-thrown together afterwards (one AggregateError), never swallowed.
//   * add() after the store is disposed disposes the newcomer IMMEDIATELY and
//     reports it -- a late registration can never outlive its owner.
//
// Pure: no DOM, no Electron, no timers. Wrapped in an IIFE so a future
// classic-script load cannot collide on top-level names (script-load-order).

(function () {
  function disposableError(code, message) {
    const e = new Error(`${code}: ${message}`);
    e.code = code;
    return e;
  }

  function isDisposable(value) {
    return !!value && typeof value === 'object' && typeof value.dispose === 'function';
  }

  // fn -> an idempotent disposable that calls fn at most once.
  function toDisposable(fn) {
    if (typeof fn !== 'function') throw disposableError('EDISPOSABLE_INVALID', 'toDisposable needs a function');
    let done = false;
    return Object.freeze({
      dispose() {
        if (done) return;
        done = true;
        fn();
      },
    });
  }

  // function | disposable -> disposable. Anything else is a contract violation:
  // an attachment that returns nothing has no way to be cleaned up.
  function asDisposable(value) {
    if (typeof value === 'function') return toDisposable(value);
    if (isDisposable(value)) return value;
    throw disposableError('EDISPOSABLE_INVALID', 'expected a dispose function or a { dispose() } object');
  }

  function disposeAll(list) {
    const errors = [];
    for (let i = list.length - 1; i >= 0; i--) {
      try { list[i].dispose(); } catch (err) { errors.push(err); }
    }
    if (errors.length === 1) throw errors[0];
    if (errors.length > 1) throw new AggregateError(errors, 'EDISPOSABLE_FAILED: several disposables threw');
  }

  // An owned collection of disposables. The owner calls dispose() exactly when
  // the thing it represents (a contribution, a mounted panel, a document) ends.
  function createDisposableStore() {
    let items = [];
    let disposed = false;

    function add(value) {
      const d = asDisposable(value);
      if (disposed) {
        d.dispose();
        return d;
      }
      items.push(d);
      return d;
    }

    // Release ONE child early (e.g. a panel closed while its contribution lives
    // on). Disposes it and forgets it; unknown children are ignored.
    function release(d) {
      const i = items.indexOf(d);
      if (i === -1) return false;
      items.splice(i, 1);
      d.dispose();
      return true;
    }

    function dispose() {
      if (disposed) return;
      disposed = true;
      const list = items;
      items = [];
      disposeAll(list);
    }

    return Object.freeze({
      add,
      release,
      dispose,
      get size() { return items.length; },
      get isDisposed() { return disposed; },
    });
  }

  // target.addEventListener(type, fn, opts) with its removal as a disposable.
  function listen(target, type, fn, options) {
    if (!target || typeof target.addEventListener !== 'function') {
      throw disposableError('EDISPOSABLE_INVALID', 'listen needs an EventTarget-like object');
    }
    target.addEventListener(type, fn, options);
    return toDisposable(() => target.removeEventListener(type, fn, options));
  }

  const WRL_SHELL_DISPOSABLE_API = Object.freeze({
    isDisposable, toDisposable, asDisposable, createDisposableStore, listen,
  });

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = WRL_SHELL_DISPOSABLE_API;
  } else {
    window.WrlShellDisposable = WRL_SHELL_DISPOSABLE_API;
  }
})();
