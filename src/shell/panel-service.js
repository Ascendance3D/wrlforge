'use strict';
// SHELL-0 panel contribution boundary
// (docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md §7;
//  APP-ARCH-0 §17, §19; UI-0 §11).
//
// The `app.panels` service. It is NOT a second registry: identity, title,
// visibility, focus and show/hide stay in the one UI-0 Panel Registry
// (src/editor/panel-registry.js), which this wraps unchanged.
//
// What it adds is the MOUNT LIFECYCLE a future dock host needs (§19):
//
//   record = { id, title, mount(host), dispose(), resize?(), focus?(),
//              canToggle?, show?(), hide?() }
//
//   * A record with `element()` is today's CSS-composed panel and is passed
//     straight through -- current panels keep working with no change.
//   * A record with `mount` is a MOUNTABLE panel. It has no element until a
//     host calls mount(id, hostElement); its `element()` is then that host.
//     Visibility is still derived from the element on every read (UI-0 §11.2),
//     so an unmounted panel is simply not visible and focus() reports
//     'hidden' -- it never opens anything.
//   * ONE active mount at a time. Hidden-but-docked panels stay mounted
//     (CodeMirror and X_ITE must not be re-created on a tab switch). Mounting
//     the same host again is a no-op; a different host while mounted is an
//     error (EPANEL_MOUNTED).
//   * unmount(id) calls the record's dispose() exactly once per mount and
//     drops the element; a second unmount is a no-op. The state is cleared
//     BEFORE dispose() runs, so a throwing dispose still leaves the panel
//     cleanly unmounted (the error propagates).
//   * After an unmount the panel is NOT spent: a later mount(id, host) -- the
//     same host or a different one -- is a fresh mount() call. Disposing one
//     mounted instance never makes the registration unusable.
//   * A mount() that throws leaves the panel unmounted; dispose() is not
//     called for it (mount must not leave partial resources when it throws).
//   * Unregistering a mounted panel unmounts it first.
//   * notifyResize(id) forwards a dock engine's resize to the panel (for
//     CodeMirror requestMeasure / the X_ITE canvas).
//   * focus stays panels.focus(id) -- the only focus path (UI-0 D6).
//
// No docking engine is chosen or integrated here (UI-C0 #38 owns that).

(function () {
  function panelServiceError(code, message) {
    const e = new Error(`${code}: ${message}`);
    e.code = code;
    return e;
  }

  function createPanelService(registry) {
    if (!registry || typeof registry.register !== 'function') {
      throw panelServiceError('EPANEL_SERVICE_INVALID', 'a panel registry is required');
    }
    const mountable = new Map(); // id -> { record, host }

    function registerMountable(record) {
      if (typeof record.dispose !== 'function') {
        throw panelServiceError('EPANEL_INVALID', `${record.id}: a mountable panel needs dispose()`);
      }
      if (record.element !== undefined) {
        throw panelServiceError('EPANEL_INVALID', `${record.id}: a mountable panel gets its element from mount(host); do not also pass element()`);
      }
      if (record.resize !== undefined && typeof record.resize !== 'function') {
        throw panelServiceError('EPANEL_INVALID', `${record.id}: resize must be a function`);
      }
      const state = { record, host: null };
      const { mount: _m, dispose: _d, resize: _r, ...rest } = record;
      void _m; void _d; void _r;
      const unregister = registry.register({ ...rest, element: () => state.host });
      mountable.set(record.id, state);
      return function unregisterMountable() {
        if (mountable.get(record.id) !== state) return;
        try { unmount(record.id); } finally {
          mountable.delete(record.id);
          unregister();
        }
      };
    }

    function register(record) {
      if (record && typeof record === 'object' && record.mount !== undefined) {
        if (typeof record.mount !== 'function') throw panelServiceError('EPANEL_INVALID', `${record.id}: mount must be a function`);
        return registerMountable(record);
      }
      return registry.register(record);
    }

    function stateOf(id) {
      const s = mountable.get(id);
      if (!s) throw panelServiceError(registry.has(id) ? 'EPANEL_NOT_MOUNTABLE' : 'EPANEL_UNKNOWN', `panel ${JSON.stringify(id)} is not a mountable panel`);
      return s;
    }

    function mount(id, host) {
      const s = stateOf(id);
      if (!host) throw panelServiceError('EPANEL_INVALID', `${id}: mount needs a host element`);
      if (s.host === host) return { ok: true, already: true };
      if (s.host) throw panelServiceError('EPANEL_MOUNTED', `${id} is already mounted in another host`);
      s.record.mount(host);
      s.host = host;
      return { ok: true };
    }

    function unmount(id) {
      const s = stateOf(id);
      if (!s.host) return { ok: true, already: true };
      s.host = null;
      s.record.dispose();
      return { ok: true };
    }

    function isMounted(id) { const s = mountable.get(id); return !!(s && s.host); }

    function notifyResize(id) {
      const s = stateOf(id);
      if (s.host && s.record.resize) s.record.resize();
    }

    return Object.freeze({
      register, mount, unmount, isMounted, notifyResize,
      has: registry.has,
      get: registry.get,
      list: registry.list,
      isVisible: registry.isVisible,
      focus: registry.focus,
      show: registry.show,
      hide: registry.hide,
    });
  }

  const WRL_SHELL_PANEL_SERVICE_API = Object.freeze({ createPanelService });

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = WRL_SHELL_PANEL_SERVICE_API;
  } else {
    window.WrlShellPanelService = WRL_SHELL_PANEL_SERVICE_API;
  }
})();
