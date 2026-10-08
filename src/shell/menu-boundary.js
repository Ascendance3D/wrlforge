'use strict';
// SHELL-0 menu integration boundary -- the RENDERER half that #122 consumes
// (docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md §10;
//  APP-ARCH-0 §20; UI-0 §10.3).
//
// SHELL-0 builds no menu, no IPC channel and no preload API. It fixes the
// contract on the renderer side so #122 only has to connect a transport:
//
//   main (#122, src/main/app-menu.js)          renderer (this module)
//   fixed template of command ids  ──id──▶     dispatchMenuCommand(registry, allow, id)
//   enabled/checked per item       ◀─state──   subscribeMenuState(registry, allow, send)
//
//   * A command becomes menu-addressable ONLY by being listed in the menu's
//     allow-list (createMenuCommandSet). Being registered is not enough, and
//     an id is data, never code.
//   * Dispatch refuses, with a reason and no throw, anything not a listed id,
//     anything not registered on THIS page, and -- before the persistent shell
//     exists -- any page that has no registry at all (Mall / World pages):
//     the id is dropped and logged ('no-registry'). Disabled commands refuse
//     through the registry itself ('disabled'), including profile-limited
//     commands outside their profile (command-service.js).
//   * Menu state is PULLED from the registry (isEnabled / isChecked), the same
//     answer the toolbar paints. subscribeMenuState pushes a snapshot once and
//     then only when it CHANGES, and returns the unsubscribe as a disposable --
//     re-installed per page load until Shell-4 (APP-ARCH-0 §20 amendment 2).
//   * Main remains responsible for disabling editor-only items when
//     currentPage !== 'editor' (§20 amendment 2); a snapshot from a page
//     without a registry reports every item unregistered/disabled.

(function () {
  const isNode = typeof module !== 'undefined' && module.exports;
  const CR = isNode ? require('../editor/command-registry') : window.WrlCommandRegistry;

  const MENU_SOURCE = 'menu';

  function menuError(code, message) {
    const e = new Error(`${code}: ${message}`);
    e.code = code;
    return e;
  }

  // The fixed, allow-listed set of command ids the application menu may send.
  function createMenuCommandSet(ids) {
    if (!Array.isArray(ids) || ids.length === 0) throw menuError('EMENU_INVALID', 'the menu command set must be a non-empty array');
    for (const id of ids) {
      if (typeof id !== 'string' || !CR.COMMAND_ID_PATTERN.test(id)) throw menuError('EMENU_INVALID', `invalid command id ${JSON.stringify(id)}`);
    }
    if (new Set(ids).size !== ids.length) throw menuError('EMENU_INVALID', 'the menu command set must not repeat an id');
    const set = new Set(ids);
    return Object.freeze({ ids: Object.freeze([...ids]), allows: (id) => typeof id === 'string' && set.has(id) });
  }

  // id arriving from the menu transport -> { ok, reason?, value? }. Never throws
  // for a bad id; the handler's own errors propagate exactly as for a toolbar.
  function dispatchMenuCommand(registry, menuSet, id, { log } = {}) {
    const note = typeof log === 'function' ? log : () => {};
    if (!menuSet || !menuSet.allows(id)) { note({ dropped: id, reason: 'not-allowed' }); return { ok: false, reason: 'not-allowed' }; }
    if (!registry) { note({ dropped: id, reason: 'no-registry' }); return { ok: false, reason: 'no-registry' }; }
    if (!registry.has(id)) { note({ dropped: id, reason: 'unregistered' }); return { ok: false, reason: 'unregistered' }; }
    return registry.execute(id, { source: MENU_SOURCE });
  }

  function menuStateSnapshot(registry, menuSet) {
    return menuSet.ids.map((id) => {
      if (!registry || !registry.has(id)) return { id, registered: false, enabled: false, checked: null };
      return { id, registered: true, enabled: registry.isEnabled(id), checked: registry.isChecked(id) };
    });
  }

  function sameSnapshot(a, b) {
    if (!a || a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      const x = a[i]; const y = b[i];
      if (x.id !== y.id || x.registered !== y.registered || x.enabled !== y.enabled || x.checked !== y.checked) return false;
    }
    return true;
  }

  // send(snapshot) now, then on every registry invalidate() that changes it.
  // Returns a disposable; with no registry it sends once and owns nothing.
  function subscribeMenuState(registry, menuSet, send) {
    if (typeof send !== 'function') throw menuError('EMENU_INVALID', 'subscribeMenuState needs a send function');
    let last = null;
    const push = () => {
      const snap = menuStateSnapshot(registry, menuSet);
      if (sameSnapshot(last, snap)) return;
      last = snap;
      send(snap);
    };
    push();
    if (!registry) return Object.freeze({ dispose() {} });
    let off = registry.subscribe(push);
    return Object.freeze({
      dispose() {
        if (!off) return;
        const f = off;
        off = null;
        f();
      },
    });
  }

  const WRL_SHELL_MENU_API = Object.freeze({
    MENU_SOURCE, createMenuCommandSet, dispatchMenuCommand, menuStateSnapshot, subscribeMenuState,
  });

  if (isNode) {
    module.exports = WRL_SHELL_MENU_API;
  } else {
    window.WrlShellMenuBoundary = WRL_SHELL_MENU_API;
  }
})();
