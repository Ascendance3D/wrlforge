'use strict';
// UI-0 command bindings (docs/ui/UI0_COMMAND_WORKSPACE_ARCHITECTURE.md §9.1, §10.1).
//
// The DOM side of the Command Registry, for the editor page:
//
//   * bindControls -- every `[data-command]` button gets the ONE click listener
//     (registry.execute, source 'toolbar') and has its `disabled` /
//     `aria-pressed` painted from the registry on every invalidate(). A
//     `<select data-command-select>` whose options carry `data-command` is a
//     radio group: it shows the option whose command is checked and runs the
//     chosen option's command on change. Labels, titles and visibility are left
//     exactly as the markup / layout code set them.
//   * installKeyboard -- ONE keydown listener replacing the hand-written
//     if/else chain. Pure migration (#35): bubble phase, no defaultPrevented
//     yield, so today's CodeMirror double-fire on Mod+G / Mod+Enter is
//     preserved for #121 to resolve deliberately. A claimed app shortcut is
//     consumed (preventDefault) even when its command is disabled. Bindings
//     owned by the editor (keyOwner 'editor') are never dispatched here.

(function () {
  const CR = (typeof module !== 'undefined' && module.exports)
    ? require('../src/editor/command-registry')
    : window.WrlCommandRegistry;

  function bindError(message) {
    const e = new Error(`ECOMMAND_BIND: ${message}`);
    e.code = 'ECOMMAND_BIND';
    return e;
  }

  function bindControls(reg, root) {
    const paints = [];
    const controls = Array.from(root.querySelectorAll('[data-command]')).filter((n) => n.tagName !== 'OPTION');
    for (const node of controls) {
      const id = node.getAttribute('data-command');
      if (!reg.has(id)) throw bindError(`no command "${id}" for #${node.id || node.tagName}`);
      node.addEventListener('click', () => { reg.execute(id, { source: 'toolbar' }); });
      const toggle = reg.get(id).isToggle;
      paints.push(() => {
        node.disabled = !reg.isEnabled(id);
        if (toggle) node.setAttribute('aria-pressed', String(reg.isChecked(id)));
      });
    }
    for (const sel of Array.from(root.querySelectorAll('[data-command-select]'))) {
      const options = Array.from(sel.querySelectorAll('option'));
      const byValue = new Map();
      for (const opt of options) {
        const id = opt.getAttribute('data-command');
        if (!id || !reg.has(id)) throw bindError(`option "${opt.value}" of #${sel.id} has no registered command`);
        byValue.set(opt.value, id);
      }
      sel.addEventListener('change', () => {
        const id = byValue.get(sel.value);
        if (id) reg.execute(id, { source: 'toolbar' });
      });
      paints.push(() => {
        for (const [value, id] of byValue) {
          if (reg.isChecked(id)) { if (sel.value !== value) sel.value = value; break; }
        }
      });
    }
    const paint = () => { for (const p of paints) p(); };
    const unsubscribe = reg.subscribe(paint);
    paint();
    return unsubscribe;
  }

  function isTextEntry(target) {
    if (!target || typeof target !== 'object') return false;
    const tag = String(target.tagName || '').toUpperCase();
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
    if (target.isContentEditable) return true;
    return !!(target.closest && target.closest('.cm-editor'));
  }

  function installKeyboard(reg, { target } = {}) {
    const onKeyDown = (e) => {
      const key = CR.shortcutFromEvent(e);
      if (!key) return;
      const binding = reg.keyBindings().find((b) => b.key === key && reg.get(b.id).keyOwner === 'app');
      if (!binding) return;
      // Bare keys belong to text entry (and, later, the world); none exist today.
      if (!CR.isModShortcut(key) && isTextEntry(e.target)) return;
      e.preventDefault();
      reg.execute(binding.id, { source: 'keyboard' });
    };
    target.addEventListener('keydown', onKeyDown);
    return () => target.removeEventListener('keydown', onKeyDown);
  }

  const WRL_COMMAND_BINDINGS_API = Object.freeze({ bindControls, installKeyboard });
  if (typeof module !== 'undefined' && module.exports) module.exports = WRL_COMMAND_BINDINGS_API;
  else window.WrlCommandBindings = WRL_COMMAND_BINDINGS_API;
})();
