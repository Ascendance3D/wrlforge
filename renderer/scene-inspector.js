'use strict';
// Scene inspector (Phase WD2-A; typed field editing added in WD2-B).
//
// Shows the structured facts P4-A and P4-B leave behind for the selected
// scene item, plus a few facts facts the scene tree itself owns. NEVER maps
// a semantic code to severity (P4-A's job), NEVER maps a semantic code to
// prose (P4-B's job), NEVER parses source text. The view is a pure DOM
// binding over the read model and the message catalog.
//
// WD2-B adds a "Fields" section for a selected Node: every explicitly authored
// field with its VRML97 type and current value; the editable ones get generic
// per-type controls (checkbox / numeric component inputs / text). Nothing
// mutates on a keystroke -- Enter or Apply commits, Escape or Cancel restores the
// document's value. The view NEVER computes a source offset: it hands
// `deps.applyField(item, field, components)` the typed values and renders the
// structured answer. Invalid input never reaches the document; the message is
// associated with the input, focus stays, and the typed text is kept.

(function () {
  const KIND = {
    DOCUMENT: 'Document',
    NODE: 'Node',
    USE: 'Use',
    PROTO: 'Proto',
    EXTERNPROTO: 'ExternProto',
    ROUTE: 'Route',
  };
  const USE_TARGET = { RESOLVED: 'resolved', UNRESOLVED: 'unresolved' };

  // Short local labels for the field-edit reason ids (src/vrml/field-edit.js).
  // Presentation text only -- the decision was made by the pure model.
  const REASON_TEXT = {
    'not-a-node-instance': 'not a node instance',
    'node-not-in-parse-session': 'selection is from an older analysis',
    'parse-session-is-stale': 'the document changed; waiting for analysis',
    'document-parse-incomplete': 'the document exceeded a parse limit',
    'document-has-syntax-errors': 'the document has syntax errors',
    'node-incomplete': 'this node is incomplete',
    'node-type-not-standard-vrml97': 'not a standard VRML97 node',
    'node-type-is-a-proto-name': 'this node type is a PROTO/EXTERNPROTO',
    'field-not-explicitly-authored': 'field not authored on this node',
    'field-authored-more-than-once': 'field authored more than once',
    'field-not-in-schema': 'not a VRML97 field of this node',
    'field-is-x3d-only': 'X3D-only field',
    'field-is-not-a-stored-field': 'event, not a stored field',
    'field-is-is-bound': 'IS-bound to a PROTO interface',
    'field-type-not-editable-in-wd2b': 'type not editable yet',
    'field-value-missing': 'value missing',
    'field-value-shape-does-not-match-type': 'value does not match its type',
    'field-value-token-invalid': 'value is not a valid literal',
    'transaction-rejected': 'the edit could not be verified',
    'round-trip-verification-failed': 'the edit did not round-trip through the parser',
    'editor-read-only': 'the editor is read-only',
    'buffer-changed': 'the document changed; try again',
    stale: 'the selection is out of date',
  };
  function reasonText(reason) { return REASON_TEXT[reason] || reason || 'unknown'; }

  let fieldDomSeq = 0;

  function emptyNote(text) {
    const n = document.createElement('div');
    n.className = 'empty-note';
    n.textContent = text;
    return n;
  }

  function clearChildren(node) { while (node.firstChild) node.removeChild(node.firstChild); }

  function makeNotice(text) {
    const n = document.createElement('div');
    n.className = 'inspector-notice';
    n.setAttribute('role', 'status');
    n.textContent = 'Note: ' + text;
    return n;
  }

  function kv(table, label, value, opts) {
    if (value == null || value === '') return;
    const tr = document.createElement('tr');
    const th = document.createElement('th');
    th.textContent = label;
    const td = document.createElement('td');
    if (opts && opts.mono) td.className = 'mono';
    td.textContent = value;
    tr.appendChild(th); tr.appendChild(td);
    table.appendChild(tr);
  }

  // Render the inspector for `item` (or the appropriate empty state). The
  // `deps` argument is the read-model + message facade; this file never
  // imports them, so tests can stub it.
  function renderInspector(rootEl, item, deps) {
    clearChildren(rootEl);
    if (deps && deps.notice) rootEl.appendChild(makeNotice(deps.notice));
    if (!deps || !deps.presentation || !deps.messages) {
      rootEl.appendChild(emptyNote('Inspector unavailable.'));
      return;
    }
    if (!item) {
      rootEl.appendChild(emptyNote('No selection. Choose an item in the scene tree.'));
      return;
    }

    const header = document.createElement('div');
    header.className = 'inspector-header kind-' + item.kind.toLowerCase();
    const title = document.createElement('div');
    title.className = 'inspector-title';
    title.textContent = titleFor(item);
    const kind = document.createElement('span');
    kind.className = 'inspector-kind';
    kind.textContent = item.kind;
    header.appendChild(title);
    header.appendChild(kind);
    rootEl.appendChild(header);

    const facts = document.createElement('table');
    facts.className = 'inspector-facts kv';
    renderFacts(facts, item);
    rootEl.appendChild(facts);

    // WD2-B: typed field values for a Node.
    if (item.kind === KIND.NODE && typeof deps.fieldsFor === 'function') {
      renderFields(rootEl, item, deps.fieldsFor(item), deps);
    }

    // Diagnostics linked to the selected item.
    rootEl.appendChild(makeHeading('Diagnostics'));
    const findings = (deps.findingsFor || (() => []))(item);
    if (!findings.length) {
      rootEl.appendChild(emptyNote('No diagnostics for this item.'));
    } else {
      rootEl.appendChild(renderFindings(findings, deps));
    }
  }

  // ---- WD2-B: Fields ------------------------------------------------------

  function renderFields(rootEl, item, info, deps) {
    rootEl.appendChild(makeHeading('Fields'));
    if (!info || !Array.isArray(info.fields) || !info.fields.length) {
      rootEl.appendChild(emptyNote('No explicitly authored fields.'));
      return;
    }
    if (info.status !== 'editable') {
      rootEl.appendChild(emptyNote('Read-only: ' + reasonText(info.reason) + '.'));
    }
    const list = document.createElement('div');
    list.className = 'inspector-fields';
    for (const field of info.fields) list.appendChild(renderFieldRow(item, field, deps));
    rootEl.appendChild(list);
  }

  function makeEl(tag, className, text) {
    const n = document.createElement(tag);
    if (className) n.className = className;
    if (text != null) n.textContent = text;
    return n;
  }

  function renderFieldRow(item, field, deps) {
    fieldDomSeq += 1;
    const base = 'wd2b-f' + fieldDomSeq;
    const row = makeEl('div', 'field-row ' + (field.editable ? 'field-editable' : 'field-readonly'));
    row.setAttribute('role', 'group');
    row.dataset.fieldIndex = String(field.index);
    row.dataset.fieldName = field.name || '';
    row.setAttribute('aria-labelledby', base + '-name ' + base + '-type');

    const head = makeEl('div', 'field-head');
    const nameEl = makeEl('span', 'field-name mono', field.name || '?');
    nameEl.id = base + '-name';
    const typeEl = makeEl('span', 'field-type mono', field.type || 'unknown type');
    typeEl.id = base + '-type';
    const stateEl = makeEl('span', 'field-state', field.editable ? 'editable' : 'read-only');
    head.appendChild(nameEl); head.appendChild(typeEl); head.appendChild(stateEl);
    row.appendChild(head);

    if (!field.editable) {
      row.appendChild(makeEl('div', 'field-value mono', field.valueExcerpt || ''));
      row.appendChild(makeEl('div', 'field-reason', 'Read-only: ' + reasonText(field.reason) + '.'));
      return row;
    }

    const msg = makeEl('div', 'field-msg');
    msg.id = base + '-msg';
    msg.setAttribute('role', 'status');
    msg.setAttribute('aria-live', 'polite');

    const controls = makeEl('div', 'field-controls');
    const inputs = [];
    const boolLabels = [];
    field.components.forEach((c, i) => {
      const id = base + '-c' + i;
      const wrap = makeEl('span', 'field-comp');
      const input = document.createElement('input');
      input.id = id;
      if (field.kind === 'bool') {
        input.type = 'checkbox';
        input.checked = c.value === true;
        const lab = makeEl('label', 'field-comp-label', c.value ? 'TRUE' : 'FALSE');
        lab.id = id + '-label';
        lab.setAttribute('for', id);
        input.addEventListener('change', () => { lab.textContent = input.checked ? 'TRUE' : 'FALSE'; });
        boolLabels[i] = lab;
        wrap.appendChild(input); wrap.appendChild(lab);
      } else {
        input.type = 'text';
        input.value = c.text;
        input.spellcheck = false;
        input.autocomplete = 'off';
        if (field.kind === 'number') {
          input.setAttribute('inputmode', field.type === 'SFInt32' ? 'numeric' : 'decimal');
          input.className = 'field-num';
        } else {
          input.className = 'field-text';
        }
        const lab = makeEl('label', 'field-comp-label', c.label);
        lab.id = id + '-label';
        lab.setAttribute('for', id);
        wrap.appendChild(lab); wrap.appendChild(input);
      }
      // Accessible name: field name + component + VRML97 type (all visible text).
      input.setAttribute('aria-labelledby', base + '-name ' + id + '-label ' + base + '-type');
      input.setAttribute('aria-describedby', msg.id);
      input.dataset.component = String(i);
      inputs.push(input);
      controls.appendChild(wrap);
    });

    const apply = makeEl('button', 'field-apply', 'Apply');
    apply.type = 'button';
    apply.setAttribute('aria-describedby', msg.id);
    apply.setAttribute('aria-label', 'Apply ' + (field.name || 'field'));
    const cancel = makeEl('button', 'field-cancel', 'Cancel');
    cancel.type = 'button';
    cancel.setAttribute('aria-label', 'Cancel changes to ' + (field.name || 'field'));
    controls.appendChild(apply);
    controls.appendChild(cancel);
    row.appendChild(controls);
    row.appendChild(msg);

    function clearInvalid() {
      for (const inp of inputs) inp.removeAttribute('aria-invalid');
      row.classList.remove('field-invalid');
    }
    function restore() {
      field.components.forEach((c, i) => {
        if (field.kind === 'bool') {
          inputs[i].checked = c.value === true;
          if (boolLabels[i]) boolLabels[i].textContent = c.value ? 'TRUE' : 'FALSE';
        } else {
          inputs[i].value = c.text;
        }
      });
      clearInvalid();
      msg.textContent = '';
    }
    function commit(focusIndex) {
      const values = inputs.map((inp) => (field.kind === 'bool' ? inp.checked === true : inp.value));
      clearInvalid();
      const res = deps.applyField
        ? state.apply(item, field, values, focusIndex)
        : { status: 'refused', reason: 'stale' };
      if (res && res.status === 'ready') return; // the Inspector re-rendered
      if (res && res.status === 'unchanged') { msg.textContent = 'No change.'; return; }
      const bad = res && Number.isInteger(res.componentIndex) ? inputs[res.componentIndex] : null;
      if (bad) {
        bad.setAttribute('aria-invalid', 'true');
        row.classList.add('field-invalid');
        msg.textContent = 'Invalid: ' + (res.message || reasonText(res.reason));
        bad.focus();
      } else {
        msg.textContent = 'Not applied: ' + ((res && res.message) || reasonText(res && res.reason)) + '.';
        const back = inputs[Number.isInteger(focusIndex) ? focusIndex : 0];
        if (back) back.focus();
      }
    }
    const indexOfTarget = (t) => { const i = inputs.indexOf(t); return i >= 0 ? i : 0; };
    row.addEventListener('keydown', (e) => {
      if (e.key === 'Enter' && e.target !== cancel) {
        e.preventDefault();
        commit(indexOfTarget(e.target === apply ? inputs[0] : e.target));
      } else if (e.key === 'Escape') {
        e.preventDefault();
        restore();
        msg.textContent = 'Restored the document value.';
        const back = inputs.indexOf(e.target) >= 0 ? e.target : inputs[0];
        if (back) back.focus();
      }
    });
    apply.addEventListener('click', () => commit(0));
    cancel.addEventListener('click', () => {
      restore();
      msg.textContent = 'Restored the document value.';
      if (inputs[0]) inputs[0].focus();
    });

    // Post-Apply focus + confirmation: the row that was just committed regains
    // focus in its matching component after the re-render.
    const pf = state.pendingFocus;
    if (pf && pf.fieldIndex === field.index && pf.fieldName === field.name) {
      msg.textContent = 'Applied.';
      state.focusTarget = inputs[pf.componentIndex] || inputs[0] || null;
    }
    return row;
  }

  // Shared between renderFieldRow and createInspector (one Inspector per page).
  const state = {
    pendingFocus: null,
    focusTarget: null,
    apply: () => ({ status: 'refused', reason: 'stale' }),
  };

  function titleFor(item) {
    switch (item.kind) {
      case KIND.NODE:
        return item.def ? `${item.nodeType} (DEF ${item.def})` : item.nodeType;
      case KIND.USE:
        return `USE ${item.useName || '?'}`;
      case KIND.PROTO:
        return `PROTO ${item.protoName || '?'}`;
      case KIND.EXTERNPROTO:
        return `EXTERNPROTO ${item.externprotoName || '?'}`;
      case KIND.ROUTE:
        return `ROUTE ${item.routeFromNode || '?'} → ${item.routeToNode || '?'}`;
      case KIND.DOCUMENT:
        return 'Document';
      default:
        return item.kind;
    }
  }

  function renderFacts(table, item) {
    switch (item.kind) {
      case KIND.NODE: {
        kv(table, 'Node type', item.nodeType, { mono: true });
        if (item.def) kv(table, 'DEF', item.def, { mono: true });
        if (item.protoInstance) kv(table, 'PROTO instance of', item.protoInstanceName, { mono: true });
        kv(table, 'Fields', String(item.fieldsCount));
        if (item.fieldNames && item.fieldNames.length) {
          kv(table, 'Field names', item.fieldNames.join(', '), { mono: true });
        }
        if (item.range) kv(table, 'Source range', rangeStr(item.range), { mono: true });
        break;
      }
      case KIND.USE: {
        kv(table, 'Name', item.useName, { mono: true });
        kv(table, 'Resolution', item.useTargetStatus);
        kv(table, 'Field', item.useFieldName || '(top-level)', { mono: true });
        if (item.range) kv(table, 'Source range', rangeStr(item.range), { mono: true });
        break;
      }
      case KIND.PROTO: {
        kv(table, 'Name', item.protoName, { mono: true });
        kv(table, 'Has body', item.protoHasBody ? 'yes' : 'no');
        kv(table, 'Interface members', String(item.protoInterfaceCount));
        if (item.range) kv(table, 'Source range', rangeStr(item.range), { mono: true });
        break;
      }
      case KIND.EXTERNPROTO: {
        kv(table, 'Name', item.externprotoName, { mono: true });
        kv(table, 'Interface members', String(item.externprotoInterfaceCount));
        if (item.range) kv(table, 'Source range', rangeStr(item.range), { mono: true });
        break;
      }
      case KIND.ROUTE: {
        kv(table, 'From', `${item.routeFromNode}.${item.routeFromEvent}`, { mono: true });
        kv(table, 'From resolved', item.routeResolvedFrom ? 'yes' : 'no');
        kv(table, 'To', `${item.routeToNode}.${item.routeToEvent}`, { mono: true });
        kv(table, 'To resolved', item.routeResolvedTo ? 'yes' : 'no');
        if (item.range) kv(table, 'Source range', rangeStr(item.range), { mono: true });
        break;
      }
      case KIND.DOCUMENT: {
        kv(table, 'Header', item.documentHasHeader ? 'yes' : 'no');
        kv(table, 'Statements', String(item.documentStatementCount));
        break;
      }
      default:
        break;
    }
  }

  function rangeStr(range) {
    if (!range) return '';
    return `${range.start.offset}–${range.end.offset} (L${range.start.line})`;
  }

  function makeHeading(text) {
    const h = document.createElement('h3');
    h.className = 'inspector-heading';
    h.textContent = text;
    return h;
  }

  // Render already-presented records directly. `findings` is the array of
  // `{finding, presentation}` records the editor binding hands in -- P4-A
  // already ordered and severity-classified them, and the inspector must
  // NOT call `presentDocumentFindings` a second time. Re-presenting would
  // throw EPRESENTATIONSHAPE (the records carry `presentation`, not the
  // raw-finding shape) and would also re-derive an order P4-A already
  // produced. The view paints severity chip colour and stops.
  function renderFindings(findings, deps) {
    const list = document.createElement('div');
    list.className = 'inspector-findings';
    // Phase: Accessibility + Performance -- the findings list is display-only
    // (rows have no activation, no navigation, no editing). Per the lane's
    // keyboard acceptance rule, "if findings are display-only, use semantic
    // list markup without fake interactive controls." role="list" pairs with
    // each row's role="listitem" (set below) -- the screen reader virtual
    // cursor reaches every row without making rows fake-focusable.
    list.setAttribute('role', 'list');
    list.setAttribute('aria-label', 'Diagnostic findings for selected item');
    for (const result of findings) {
      // result is `{finding, presentation}`. Defensive: a future caller
      // passing a raw finding would still work because P4-B accepts either
      // shape (the dispatcher reads `presentation.origin`); we just never
      // present twice here.
      const presentation = result && result.presentation;
      if (!presentation) continue;
      const text = deps.messages.messageForPresentation(result);
      const row = document.createElement('div');
      row.className = 'inspector-row sev-' + (presentation.severity || 'error');
      row.setAttribute('role', 'listitem');

      // Severity chip is the ONE place a styling choice lives; P4-A already
      // chose the severity value. The view only paints it.
      const chip = document.createElement('span');
      chip.className = 'inspector-chip';
      chip.textContent = severityLabel(presentation.severity);
      row.appendChild(chip);

      const body = document.createElement('div');
      body.className = 'inspector-body';

      const t = document.createElement('div');
      t.className = 'inspector-row-title';
      t.textContent = text.title;
      body.appendChild(t);

      const s = document.createElement('div');
      s.className = 'inspector-row-summary';
      s.textContent = text.summary;
      body.appendChild(s);

      if (text.detail) {
        const d = document.createElement('div');
        d.className = 'inspector-row-detail';
        d.textContent = text.detail;
        body.appendChild(d);
      }

      row.appendChild(body);
      list.appendChild(row);
    }
    return list;
  }

  // Human-readable severity label. Mapped from the SEVERITY value P4-A
  // emits -- this is presentation text, NOT severity selection.
  function severityLabel(sev) {
    if (sev === 'error') return 'Error';
    if (sev === 'warning') return 'Warning';
    if (sev === 'info') return 'Info';
    if (sev === 'hint') return 'Hint';
    return 'Issue';
  }

  function createInspector(rootEl, selection, deps) {
    let currentTree = null;
    let currentFindings = [];
    let notice = null;
    // WD2-B: one Apply = plan + verified dispatch + synchronous re-analysis, so
    // every re-render it causes has happened by the time applyField returns.
    state.apply = (item, field, values, focusIndex) => {
      state.pendingFocus = { fieldIndex: field.index, fieldName: field.name, componentIndex: focusIndex | 0 };
      state.focusTarget = null;
      let res;
      try {
        res = deps.applyField(item, field, values);
      } finally {
        const target = state.focusTarget;
        state.pendingFocus = null;
        state.focusTarget = null;
        if (res && res.status === 'ready' && target) target.focus();
      }
      return res;
    };
    function render() {
      const id = selection.getSelection();
      // Look up the selected item via the scene-tree facade so we do not
      // depend on the (now read-only-proxied) `tree.byId` Map directly.
      const item = id && currentTree && deps.itemById
        ? deps.itemById(currentTree, id)
        : null;
      renderInspector(rootEl, item, {
        presentation: deps.presentation,
        messages: deps.messages,
        fieldsFor: deps.fieldsFor,
        applyField: deps.applyField,
        notice,
        findingsFor: (item) => {
          if (!item || !currentTree || !deps.itemContainingOffset) return [];
          // Diagnostic ownership: each finding belongs to the SINGLE most-
          // specific scene item containing its range start. A finding inside
          // a nested Shape appears on the Shape -- not on the Shape's
          // enclosing Group, and not on the Document. This is the rule F3
          // exists to enforce: P4-A ordering is preserved, but the owner
          // filter is a one-item match, not an ancestor-containment match.
          return currentFindings.filter((p) => {
            const finding = p.finding;
            if (!finding || !finding.range) return false;
            const off = finding.range.start && finding.range.start.offset;
            if (off == null) return false;
            const owner = deps.itemContainingOffset(currentTree, off);
            return owner && owner.id === item.id;
          });
        },
      });
    }

    const renderWithNotice = () => render();

    selection.subscribe((id) => {
      if (id != null) notice = null;
      renderWithNotice();
    });
    return {
      setSceneTree(tree) {
        // WD2-B: an unchanged tree is not re-rendered -- a status refresh must
        // never wipe what the user is typing into a field control.
        if ((tree || null) === currentTree) return;
        currentTree = tree || null;
        renderWithNotice();
      },
      // WD2-B: a visible, non-colour explanation when a selection is lost.
      setNotice(text) {
        notice = text ? String(text) : null;
        renderWithNotice();
      },
      // Findings list comes from the editor binding; it is the
      // already-ordered presentation array. The inspector filters by
      // most-specific ownership.
      setFindings(findings) {
        const next = Array.isArray(findings) ? findings : [];
        if (next === currentFindings) return;
        currentFindings = next;
        renderWithNotice();
      },
    };
  }

  const api = {
    KIND,
    USE_TARGET,
    createInspector,
    renderInspector,
  };

  if (typeof module !== 'undefined' && module.exports) {
    module.exports = api;
  } else {
    window.WRLForgeInspector = api;
  }
})();