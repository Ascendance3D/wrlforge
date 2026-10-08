'use strict';
// SHELL-0 application-service boundary
// (docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md §5;
//  APP-ARCH-0 docs/architecture/DESKTOP_APPLICATION_SHELL_REVIEW.md §16).
//
// The CLOSED list of services a contribution may receive as `app.<name>`.
// It is a catalog, not a container: the shell (Shell-2+) constructs each
// service itself, once, and passes the instances in. There is no lookup by
// string at run time, no lazy construction and no dependency graph.
//
// A service is admitted only when current or approved future behavior needs it.
// Everything else APP-ARCH-0 §16 names is recorded in DEFERRED_SERVICES with the
// reason and the lane that owns it, and is REJECTED if passed -- so the `app`
// object cannot quietly grow into a god-object.
//
// Each entry declares:
//   owner    -- where the authority lives (an existing module, or a contract)
//   status   -- 'existing' (wraps today's module, API unchanged) |
//               'shell-0'  (contract module built in SHELL-0, no callers yet) |
//               'boundary' (shape only; implemented by the named lane)
//   methods  -- the surface a provided instance must have
//   tracked  -- methods whose return value is a cleanup handle; the
//               contribution host takes ownership of it (contribution.js)

(function () {
  const SERVICE_CATALOG = Object.freeze({
    commands: Object.freeze({
      owner: 'src/editor/command-registry.js via src/shell/command-service.js',
      status: 'existing',
      required: true,
      methods: Object.freeze(['register', 'has', 'get', 'list', 'isEnabled', 'isChecked', 'execute', 'keyBindings', 'subscribe', 'invalidate']),
      tracked: Object.freeze(['register', 'subscribe']),
    }),
    panels: Object.freeze({
      owner: 'src/editor/panel-registry.js via src/shell/panel-service.js',
      status: 'existing',
      required: true,
      methods: Object.freeze(['register', 'has', 'get', 'list', 'isVisible', 'focus', 'show', 'hide']),
      tracked: Object.freeze(['register']),
    }),
    tools: Object.freeze({
      owner: 'src/shell/tool-registry.js',
      status: 'shell-0',
      required: false,
      methods: Object.freeze(['register', 'has', 'get', 'list', 'state', 'activate', 'subscribe']),
      tracked: Object.freeze(['register', 'subscribe']),
    }),
    contextualPanels: Object.freeze({
      owner: 'src/shell/contextual-panels.js',
      status: 'shell-0',
      required: false,
      methods: Object.freeze(['register', 'reconcile', 'mounted']),
      tracked: Object.freeze(['register']),
    }),
    documents: Object.freeze({
      owner: 'src/shell/document-session.js (createDocumentSlot)',
      status: 'shell-0',
      required: false,
      methods: Object.freeze(['current', 'subscribe']),
      tracked: Object.freeze(['subscribe']),
    }),
    workspaces: Object.freeze({
      // Today: editor.js S.workspaceMode + setWorkspaceMode, reading
      // src/editor/workspace-presets.js. #121 wraps that one authority; it does
      // not create a second one.
      owner: 'editor.js setWorkspaceMode + src/editor/workspace-presets.js (wrapped by #121)',
      status: 'boundary',
      required: false,
      methods: Object.freeze(['mode', 'preset', 'setMode', 'subscribe']),
      tracked: Object.freeze(['subscribe']),
    }),
    dialogs: Object.freeze({
      // #121 D3: one question replaces per-dialog classList checks.
      owner: 'dialog stack (built by #121; modal stack completed in Shell-2)',
      status: 'boundary',
      required: false,
      methods: Object.freeze(['isModalOpen']),
      tracked: Object.freeze([]),
    }),
    preferences: Object.freeze({
      owner: 'renderer/preferences.js WrlPreferences (localStorage)',
      status: 'existing',
      required: false,
      methods: Object.freeze(['get', 'set', 'subscribe']),
      tracked: Object.freeze(['subscribe']),
    }),
  });

  // Named by APP-ARCH-0 §16 but NOT admitted in SHELL-0. Passing one is an error.
  const DEFERRED_SERVICES = Object.freeze({
    keyboard: 'stays command-bindings.installKeyboard (one dispatcher already); #121 adds the dialogs.isModalOpen() gate inside it',
    preview: 'PreviewSession + profile adapters are Shell-2; today editor-preview.js owns it and the document session only references it',
    files: 'thin façade over window.vrmlpad.editor.* is Shell-2; main stays the only path authority',
    validation: 'profile-contributed (language.js, validator.js); a shared validation service would invite Mall rules into shared code',
    status: 'one status bar / aria-live service is Shell-3 (shared chrome)',
    notifications: 'one toast / announce service is Shell-3 (shared chrome)',
    menu: 'main-process (src/main/app-menu.js, #122); the renderer side is src/shell/menu-boundary.js over app.commands, not a service',
  });

  function serviceError(code, message) {
    const e = new Error(`${code}: ${message}`);
    e.code = code;
    return e;
  }

  // { name: instance } -> frozen copy, after checking every name is admitted,
  // every required service is present and every instance has its surface.
  function validateServices(services) {
    if (!services || typeof services !== 'object') throw serviceError('EAPP_SERVICE_INVALID', 'services must be an object');
    const out = {};
    for (const name of Object.keys(services)) {
      if (Object.prototype.hasOwnProperty.call(DEFERRED_SERVICES, name)) {
        throw serviceError('EAPP_SERVICE_DEFERRED', `"${name}" is not a SHELL-0 service: ${DEFERRED_SERVICES[name]}`);
      }
      const spec = SERVICE_CATALOG[name];
      if (!spec) throw serviceError('EAPP_SERVICE_UNKNOWN', `"${name}" is not in the service catalog`);
      const svc = services[name];
      if (!svc || typeof svc !== 'object') throw serviceError('EAPP_SERVICE_INVALID', `${name} must be an object`);
      const missing = spec.methods.filter((m) => typeof svc[m] !== 'function');
      if (missing.length) throw serviceError('EAPP_SERVICE_INVALID', `${name} is missing ${missing.join(', ')}`);
      out[name] = svc;
    }
    for (const [name, spec] of Object.entries(SERVICE_CATALOG)) {
      if (spec.required && !out[name]) throw serviceError('EAPP_SERVICE_MISSING', `required service "${name}" was not provided`);
    }
    return Object.freeze(out);
  }

  const WRL_SHELL_SERVICES_API = Object.freeze({ SERVICE_CATALOG, DEFERRED_SERVICES, validateServices });

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = WRL_SHELL_SERVICES_API;
  } else {
    window.WrlShellServices = WRL_SHELL_SERVICES_API;
  }
})();
