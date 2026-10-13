'use strict';
// SHELL-0 profile ids (docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md §6).
//
// The three document profiles of CLAUDE.md, as DATA only. These ids are the
// values of the editor's existing `context` ('mall' | 'world' | 'generic').
// No profile rule (Mall size cap, WorldInfo, texture limits, World packaging)
// lives here or in any shared shell module: a profile's rules stay in its own
// modules, and shared code only ever asks "is this contribution listed for the
// current profile?".

(function () {
  const PROFILE_IDS = Object.freeze(['mall', 'world', 'generic']);

  function normalizeProfileList(list, label) {
    const where = label ? `${label}: ` : '';
    const fail = (why) => {
      const e = new Error(`EPROFILE_INVALID: ${where}${why}`);
      e.code = 'EPROFILE_INVALID';
      throw e;
    };
    if (!Array.isArray(list) || list.length === 0) fail('profiles must be a non-empty array');
    for (const p of list) if (!PROFILE_IDS.includes(p)) fail(`unknown profile ${JSON.stringify(p)}`);
    if (new Set(list).size !== list.length) fail('profiles must not repeat');
    return Object.freeze([...list]);
  }

  const WRL_SHELL_PROFILES_API = Object.freeze({ PROFILE_IDS, normalizeProfileList });

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = WRL_SHELL_PROFILES_API;
  } else {
    window.WrlShellProfiles = WRL_SHELL_PROFILES_API;
  }
})();
