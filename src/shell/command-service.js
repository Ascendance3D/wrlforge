'use strict';
// SHELL-0 command contribution boundary
// (docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md §6;
//  APP-ARCH-0 §17; UI-0 §6, §9).
//
// The `app.commands` service. It is NOT a second registry: every record is
// stored in, executed by, and painted from the one UI-0 Command Registry
// (src/editor/command-registry.js). This module only adds what APP-ARCH-0 §17
// specified for contributions:
//
//   * `profiles` (optional array of profile ids) on a command record. An
//     unlisted command is profile-neutral. A listed one is enabled only while
//     the current document's profile is in the list -- folded into the
//     record's own `enabled`, so registry.isEnabled / execute / the toolbar
//     painter / the future menu all see the same answer. Profile containment is
//     DATA here; no profile's RULES live in this module. Adding `profiles`
//     never weakens the registry's validation: a record's `enabled` must still
//     be undefined or a function, and anything else is rejected exactly as the
//     registry rejects it for a profile-neutral command -- never reinterpreted
//     as "always enabled".
//   * `profilesOf(id)` so a menu or toolbar can ask which profiles a command
//     belongs to without re-reading the record.
//
// Command ids, the record shape, key ownership and the enabled/checked pull
// model are the registry's and are unchanged. register() still returns the
// registry's unregister function; under the contribution host it is owned by
// the contribution (contribution.js).

(function () {
  const isNode = typeof module !== 'undefined' && module.exports;
  const PR = isNode ? require('./profiles') : window.WrlShellProfiles;

  function commandServiceError(code, message) {
    const e = new Error(`${code}: ${message}`);
    e.code = code;
    return e;
  }

  // registry: a createCommandRegistry() instance.
  // getProfile: () => current document profile id, or null with no document.
  function createCommandService(registry, { getProfile } = {}) {
    if (!registry || typeof registry.register !== 'function') {
      throw commandServiceError('ECOMMAND_SERVICE_INVALID', 'a command registry is required');
    }
    if (typeof getProfile !== 'function') {
      throw commandServiceError('ECOMMAND_SERVICE_INVALID', 'getProfile() is required');
    }
    const profilesById = new Map();

    function register(record) {
      if (!record || typeof record !== 'object' || record.profiles === undefined) return registry.register(record);
      const profiles = PR.normalizeProfileList(record.profiles, `command ${record.id}`);
      const own = record.enabled;
      const inProfile = () => profiles.includes(getProfile());
      const { profiles: _omit, ...rest } = record;
      void _omit;
      if (own !== undefined && typeof own !== 'function') {
        // Not ours to reinterpret: the registry's own validation decides, on
        // the record exactly as written (it rejects with ECOMMAND_INVALID).
        // Should it ever accept the value, refuse rather than register a
        // command with no profile gate.
        registry.register(rest)();
        throw commandServiceError('ECOMMAND_SERVICE_INVALID', `${record.id}: enabled must be undefined or a function`);
      }
      const unregister = registry.register({
        ...rest,
        enabled: own === undefined ? inProfile : () => inProfile() && !!own(),
      });
      profilesById.set(record.id, profiles);
      return function unregisterProfiled() {
        unregister();
        if (!registry.has(record.id)) profilesById.delete(record.id);
      };
    }

    // null = profile-neutral (or unknown id).
    function profilesOf(id) { return profilesById.get(id) || null; }

    return Object.freeze({
      register,
      profilesOf,
      has: registry.has,
      get: registry.get,
      list: registry.list,
      isEnabled: registry.isEnabled,
      isChecked: registry.isChecked,
      execute: registry.execute,
      keyBindings: registry.keyBindings,
      subscribe: registry.subscribe,
      invalidate: registry.invalidate,
    });
  }

  const WRL_SHELL_COMMAND_SERVICE_API = Object.freeze({ createCommandService });

  if (isNode) {
    module.exports = WRL_SHELL_COMMAND_SERVICE_API;
  } else {
    window.WrlShellCommandService = WRL_SHELL_COMMAND_SERVICE_API;
  }
})();
