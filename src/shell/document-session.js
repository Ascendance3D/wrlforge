'use strict';
// SHELL-0 document-session boundary
// (docs/architecture/SHELL_0_CONTRACTS_AND_MEASUREMENTS.md §4;
//  APP-ARCH-0 §15; WD.md §2, §7).
//
// The renderer DocumentSession is a CONTROLLER over existing authorities. It is
// not a document model and owns no new data:
//
//   CodeMirror handle   = source text + undo authority   (session.text(), .history())
//   sceneSelection      = selection authority            (session.selection, same object)
//   main EditorSession  = path, authorization, sessionId (session.sessionId only)
//   X_ITE preview       = derived projection             (session.preview, a reference)
//   analysis products   = derived, replaced wholesale    (session.analysis())
//
// Guards that make "no second canonical document" structural, not a promise:
//   * Construction takes ONLY the keys below. Text, buffers, undo stacks, scene
//     graphs, paths and workspace/layout state are refused by name and any
//     unknown key is refused too -- a second source copy cannot be smuggled in.
//   * text() and history() READ THROUGH to the CodeMirror handle on every call;
//     nothing is cached. There is no setText, no undo/redo and no serializer:
//     source changes only through applyVerifiedEdits (the handle's existing
//     verified, single-transaction path) or CodeMirror itself.
//   * profile and sessionId are fixed at construction (read-only after open).
//   * Paths never enter the renderer session: main stays the path authority.
//   * Workspace mode, sourceOpen and layout stay in the shell's workspace
//     service (APP-ARCH-0 §15); they are refused here.
//
// createDocumentSlot() is the `app.documents` service: one document at a time
// (today's product rule). Opening a session disposes the previous one.

(function () {
  const isNode = typeof module !== 'undefined' && module.exports;
  const D = isNode ? require('./disposable') : window.WrlShellDisposable;
  const PR = isNode ? require('./profiles') : window.WrlShellProfiles;

  const ALLOWED_KEYS = Object.freeze(['sessionId', 'profile', 'editor', 'selection', 'preview']);
  // Named so the refusal says WHY, not just "unknown key".
  const FORBIDDEN_KEYS = Object.freeze({
    text: 'source text lives in the CodeMirror handle',
    source: 'source text lives in the CodeMirror handle',
    buffer: 'source text lives in the CodeMirror handle',
    baseline: 'the saved baseline stays where it is today until Shell-2 decides dirty-state ownership',
    undo: 'CodeMirror is the undo authority',
    history: 'CodeMirror is the undo authority',
    sceneGraph: 'there is no canonical scene graph (WD.md §2)',
    ast: 'analysis is derived; pass it through setAnalysis()',
    path: 'main owns every path',
    sourcePath: 'main owns every path',
    workspaceMode: 'workspace state belongs to the shell workspace service',
    sourceOpen: 'workspace state belongs to the shell workspace service',
    layout: 'layout state belongs to the shell workspace service',
  });

  function sessionError(code, message) {
    const e = new Error(`${code}: ${message}`);
    e.code = code;
    return e;
  }

  function createDocumentSession(options) {
    if (!options || typeof options !== 'object') throw sessionError('EDOCSESSION_INVALID', 'options are required');
    for (const key of Object.keys(options)) {
      if (Object.prototype.hasOwnProperty.call(FORBIDDEN_KEYS, key)) {
        throw sessionError('EDOCSESSION_FORBIDDEN', `"${key}" is not session state: ${FORBIDDEN_KEYS[key]}`);
      }
      if (!ALLOWED_KEYS.includes(key)) throw sessionError('EDOCSESSION_INVALID', `unknown option "${key}"`);
    }
    const { sessionId, profile, editor, selection, preview } = options;
    if (!(typeof sessionId === 'number' && Number.isInteger(sessionId)) && !(typeof sessionId === 'string' && sessionId !== '')) {
      throw sessionError('EDOCSESSION_INVALID', 'sessionId (from main) is required');
    }
    if (!PR.PROFILE_IDS.includes(profile)) throw sessionError('EDOCSESSION_INVALID', `unknown profile ${JSON.stringify(profile)}`);
    if (!editor || typeof editor.getText !== 'function') throw sessionError('EDOCSESSION_INVALID', 'editor must be the CodeMirror handle (getText)');
    if (!selection || typeof selection.getSelection !== 'function' || typeof selection.subscribe !== 'function') {
      throw sessionError('EDOCSESSION_INVALID', 'selection must be the sceneSelection controller');
    }

    const owned = D.createDisposableStore();
    let analysis = null;
    let disposed = false;

    function live() { if (disposed) throw sessionError('EDOCSESSION_DISPOSED', `session ${sessionId} has been disposed`); }

    const session = {
      get sessionId() { return sessionId; },
      get profile() { return profile; },
      get selection() { return selection; },
      get preview() { return preview || null; },
      get isDisposed() { return disposed; },
      isCurrent(id) { return !disposed && id === sessionId; },
      // Read-through, never cached.
      text() { live(); return editor.getText(); },
      history() { live(); return typeof editor.historyDepth === 'function' ? editor.historyDepth() : null; },
      // The ONE source-edit path: CodeMirror's verified single transaction.
      applyVerifiedEdits(args) {
        live();
        if (typeof editor.applyVerifiedEdits !== 'function') return { ok: false, reason: 'unsupported' };
        return editor.applyVerifiedEdits(args);
      },
      // Derived products, replaced wholesale per analysis run; never edited.
      analysis() { return analysis; },
      setAnalysis(products) { live(); analysis = products == null ? null : products; },
      // Session-lifetime attachments (preview wiring, selection subscriptions).
      own(value) { return owned.add(value); },
      dispose() {
        if (disposed) return;
        disposed = true;
        analysis = null;
        const errors = [];
        try { owned.dispose(); } catch (err) { errors.push(err); }
        if (preview && typeof preview.dispose === 'function') { try { preview.dispose(); } catch (err) { errors.push(err); } }
        // The session owns the CodeMirror view for the document's lifetime.
        if (typeof editor.destroy === 'function') { try { editor.destroy(); } catch (err) { errors.push(err); } }
        if (errors.length === 1) throw errors[0];
        if (errors.length > 1) throw new AggregateError(errors, 'EDOCSESSION_DISPOSE_FAILED');
      },
    };
    return Object.freeze(session);
  }

  // app.documents: the one current document session (or null).
  function createDocumentSlot() {
    let current = null;
    const listeners = new Set();
    function notify() {
      for (const fn of [...listeners]) {
        try { fn(current); } catch (err) { console.error('[document-slot] listener failed:', err); }
      }
    }
    function open(session) {
      if (!session || typeof session.dispose !== 'function' || typeof session.text !== 'function') {
        throw sessionError('EDOCSESSION_INVALID', 'open() needs a document session');
      }
      const previous = current;
      current = session;
      try { if (previous && previous !== session) previous.dispose(); } finally { notify(); }
      return session;
    }
    function close() {
      if (!current) return false;
      const previous = current;
      current = null;
      try { previous.dispose(); } finally { notify(); }
      return true;
    }
    function subscribe(fn) {
      if (typeof fn !== 'function') throw sessionError('EDOCSESSION_INVALID', 'subscribe needs a function');
      listeners.add(fn);
      return () => { listeners.delete(fn); };
    }
    return Object.freeze({ current: () => current, open, close, subscribe, listenerCount: () => listeners.size });
  }

  const WRL_SHELL_DOCUMENT_SESSION_API = Object.freeze({
    ALLOWED_KEYS, FORBIDDEN_KEYS, createDocumentSession, createDocumentSlot,
  });

  if (isNode) {
    module.exports = WRL_SHELL_DOCUMENT_SESSION_API;
  } else {
    window.WrlShellDocumentSession = WRL_SHELL_DOCUMENT_SESSION_API;
  }
})();
