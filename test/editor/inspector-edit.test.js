'use strict';
// WD2-B -- Inspector Apply -> verified WD1.2 edits -> CodeMirror transaction ->
// analysis -> scene tree -> WD1.4 re-anchor -> Inspector refresh.
//
// Exercises the production modules end to end: src/editor/language.js (the
// analyzer the editor bundle calls), the WD1.5 scope graph + scene tree +
// findings + P4-A presentation exactly as renderer/editor.js wires them,
// src/editor/inspector-edit.js (planning + selection survival + change-chain
// conversion), and REAL CodeMirror state/history (@codemirror/state +
// @codemirror/commands) with the same transaction annotations the editor view
// dispatches. No second editing pipeline: the buffer IS a CodeMirror document.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const { EditorState, Transaction } = require('@codemirror/state');
const { history, undo, redo, isolateHistory, undoDepth } = require('@codemirror/commands');

const language = require('../../src/editor/language');
const inspectorEdit = require('../../src/editor/inspector-edit');
const sceneTree = require('../../src/vrml/scene-tree');
const scopeGraph = require('../../src/vrml/scope-graph');
const semanticFindings = require('../../src/vrml/semantic-findings');
const presentation = require('../../src/vrml/presentation');
const tx = require('../../src/vrml/document-transaction');
const fe = require('../../src/vrml/field-edit');

const H = '#VRML V2.0 utf8\n';
const O = inspectorEdit.OUTCOME;

// The renderer's onAnalysis pipeline (renderer/editor.js), over one analysis.
function analyse(text) {
  const a = language.analyze(text, { profile: 'generic' });
  const graph = scopeGraph.buildScopeGraph(a.parseResult);
  const useResolver = (useNode) => {
    const r = scopeGraph.resolve(graph, useNode);
    return scopeGraph.isResolved(r) && r.symbol && r.symbol.node
      ? { status: 'resolved', targetAstNode: r.symbol.node } : { status: 'unresolved' };
  };
  const tree = sceneTree.buildSceneTree(a.parseResult, { useResolver });
  const findings = presentation.presentDocumentFindings(semanticFindings.findingsForDocument(graph));
  return { text, tree, findings, session: tx.createParseSession(text, a.parseResult), diagnostics: a.diagnostics };
}

// A minimal editor: one CodeMirror EditorState with history, the composed
// change chain since the last analysis (as editor-view.js keeps it), and the
// analysis/selection state renderer/editor.js keeps.
function makeEditor(doc) {
  const ed = {
    state: EditorState.create({ doc, extensions: [history()] }),
    chainBase: null,
    chainChanges: null,
    analysis: null,
    selectedId: null,
    pendingApply: null,
    lastOutcome: null,
  };
  ed.text = () => ed.state.doc.toString();
  ed.dispatch = (spec) => {
    const tr = ed.state.update(spec);
    ed.apply(tr);
  };
  ed.apply = (tr) => {
    if (tr.docChanged) ed.chainChanges = ed.chainChanges ? ed.chainChanges.compose(tr.changes) : tr.changes;
    ed.state = tr.state;
  };
  ed.cmd = (command) => command({ state: ed.state, dispatch: (tr) => ed.apply(tr) });
  ed.reanalyze = () => {
    const text = ed.text();
    const chain = ed.chainBase === null ? null : { previousText: ed.chainBase, edits: inspectorEdit.changesToEdits(ed.chainChanges) };
    ed.chainBase = text;
    ed.chainChanges = null;
    const next = analyse(text);
    const previous = ed.analysis ? { session: ed.analysis.session, tree: ed.analysis.tree } : null;
    const decision = inspectorEdit.reanchorSelection({
      previous, next: { session: next.session, tree: next.tree }, selectedId: ed.selectedId, chain, pendingApply: ed.pendingApply,
    });
    ed.pendingApply = null;
    ed.analysis = next;
    ed.lastOutcome = decision;
    ed.selectedId = decision.id;
    return decision;
  };
  // A reload/open: fresh state, broken chain (editor-view.js setDoc).
  ed.setDoc = (text) => {
    ed.state = EditorState.create({ doc: text, extensions: [history()] });
    ed.chainBase = null;
    ed.chainChanges = null;
  };
  // renderer/editor.js applyInspectorField + editor-view.js applyVerifiedEdits.
  ed.applyField = (fieldName, components) => {
    const fields = ed.fields();
    const field = fields.fields.find((f) => f.name === fieldName);
    const plan = inspectorEdit.prepareInspectorApply({
      session: ed.analysis.session, tree: ed.analysis.tree, currentText: ed.text(),
      itemId: ed.selectedId, fieldIndex: field.index, fieldName, components,
    });
    if (plan.status !== 'ready') return plan;
    assert.equal(ed.text(), plan.oldText);
    ed.dispatch({
      changes: plan.edits.map((e) => ({ from: e.from, to: e.to, insert: e.insert })),
      annotations: [isolateHistory.of('full'), Transaction.userEvent.of('input.inspector')],
    });
    assert.equal(ed.text(), plan.newText, 'CodeMirror holds exactly the verified new text');
    ed.pendingApply = { oldText: plan.oldText, newText: plan.newText, receipt: plan.receipt };
    ed.reanalyze();
    return plan;
  };
  ed.fields = () => inspectorEdit.fieldsForItem({
    session: ed.analysis.session, tree: ed.analysis.tree, currentText: ed.text(), itemId: ed.selectedId,
  });
  ed.selected = () => (ed.selectedId ? sceneTree.itemById(ed.analysis.tree, ed.selectedId) : null);
  ed.reanalyze();
  return ed;
}

const nodeItems = (ed, nodeType) => ed.analysis.tree.items.filter((it) => it.kind === 'Node' && it.nodeType === nodeType);
const values = (ed, fieldName) => ed.fields().fields.find((f) => f.name === fieldName).components.map((c) => c.text);

const PRIMARY = H
  + 'Transform {\n'
  + '  translation 0 0 0\n'
  + '  children [\n'
  + '    Shape {\n'
  + '      geometry Box { size 2 2 2 }\n'
  + '    }\n'
  + '  ]\n'
  + '}\n';

// ---------------------------------------------------------------------------
// The primary acceptance flow (pure half; the Electron QA runs it for real)
// ---------------------------------------------------------------------------

test('primary: select Transform, Apply X=3, source/selection/Inspector update; Undo/Redo exact', () => {
  const ed = makeEditor(PRIMARY);
  const transform = nodeItems(ed, 'Transform')[0];
  ed.selectedId = transform.id;
  const before = ed.fields().fields.find((f) => f.name === 'translation');
  assert.equal(before.type, 'SFVec3f');
  assert.equal(before.editable, true);
  assert.deepEqual(values(ed, 'translation'), ['0', '0', '0']);

  const plan = ed.applyField('translation', ['3', '0', '0']);
  assert.equal(plan.status, 'ready');
  assert.equal(ed.text(), PRIMARY.replace('translation 0 0 0', 'translation 3 0 0'));
  assert.notEqual(ed.lastOutcome.outcome, O.LOST);
  assert.equal(ed.lastOutcome.reason, 'verified-transaction-span-match', 'proven by WD1.4 Tier 1');
  assert.equal(ed.selected().nodeType, 'Transform');
  assert.deepEqual(values(ed, 'translation'), ['3', '0', '0']);
  assert.equal(undoDepth(ed.state), 1, 'one Apply = one history event');

  ed.cmd(undo);
  assert.equal(ed.text(), PRIMARY, 'one Undo restores the exact original text');
  ed.reanalyze();
  assert.equal(ed.lastOutcome.reason, 'verified-transaction-span-match');
  assert.equal(ed.selected().nodeType, 'Transform');
  assert.deepEqual(values(ed, 'translation'), ['0', '0', '0']);

  ed.cmd(redo);
  assert.equal(ed.text(), PRIMARY.replace('translation 0 0 0', 'translation 3 0 0'), 'one Redo restores the exact edited text');
  ed.reanalyze();
  assert.equal(ed.selected().nodeType, 'Transform');
  assert.deepEqual(values(ed, 'translation'), ['3', '0', '0']);
});

test('a length-changing Apply moves the item span; identity re-anchors it to the new id', () => {
  const ed = makeEditor(PRIMARY);
  const before = nodeItems(ed, 'Transform')[0];
  ed.selectedId = before.id;
  ed.applyField('translation', ['-12.5', '0', '0']);
  assert.equal(ed.lastOutcome.outcome, O.REANCHORED);
  assert.notEqual(ed.selectedId, before.id, 'the id is a span-derived label, not identity');
  assert.equal(ed.selected().nodeType, 'Transform');
  assert.deepEqual(values(ed, 'translation'), ['-12.5', '0', '0']);
  ed.cmd(undo);
  ed.reanalyze();
  assert.equal(ed.lastOutcome.outcome, O.REANCHORED);
  assert.equal(ed.selectedId, before.id);
  assert.equal(ed.text(), PRIMARY);
});

test('a multi-component Apply is ONE history event; one Undo reverses all of it', () => {
  const ed = makeEditor(H + 'Material { diffuseColor 0.8 0.8 0.8 }\n');
  ed.selectedId = nodeItems(ed, 'Material')[0].id;
  const plan = ed.applyField('diffuseColor', ['1', '0', '0.25']);
  assert.equal(plan.edits.length, 3);
  assert.equal(ed.text(), H + 'Material { diffuseColor 1 0 0.25 }\n');
  assert.equal(undoDepth(ed.state), 1);
  ed.cmd(undo);
  assert.equal(ed.text(), H + 'Material { diffuseColor 0.8 0.8 0.8 }\n');
  assert.equal(undoDepth(ed.state), 0);
});

test('an Apply right after typing does not merge into the typing history event', () => {
  const ed = makeEditor(PRIMARY);
  ed.selectedId = nodeItems(ed, 'Transform')[0].id;
  // Adjacent typing inside the Transform, annotated like CodeMirror's keymap.
  const at = ed.text().indexOf('translation 0 0 0') + 'translation 0 0 0'.length;
  ed.dispatch({ changes: { from: at, insert: ' ' }, annotations: Transaction.userEvent.of('input.type') });
  ed.reanalyze();
  assert.equal(ed.selected().nodeType, 'Transform', 'typing inside the node keeps it (verified chain)');
  ed.applyField('translation', ['3', '0', '0']);
  assert.equal(undoDepth(ed.state), 2);
  ed.cmd(undo);
  assert.ok(ed.text().includes('translation 0 0 0 \n'), 'Undo reverses only the Apply, not the typing');
});

// ---------------------------------------------------------------------------
// Selection survival: zero wrong anchors
// ---------------------------------------------------------------------------

const TWINS = H + 'Group { children [\n  Transform { translation 0 0 0 }\n  Transform { translation 0 0 0 }\n] }\n';

test('twins: editing the SECOND of two identical siblings keeps the SECOND selected', () => {
  const ed = makeEditor(TWINS);
  const [first, second] = nodeItems(ed, 'Transform');
  ed.selectedId = second.id;
  ed.applyField('translation', ['5', '0', '0']);
  const [newFirst, newSecond] = nodeItems(ed, 'Transform');
  assert.equal(ed.selectedId, newSecond.id, 'identity never jumps to the twin');
  assert.notEqual(ed.selectedId, newFirst.id);
  assert.equal(newFirst.id, first.id, 'the untouched twin keeps its span');
  assert.deepEqual(values(ed, 'translation'), ['5', '0', '0']);
});

test('twins: editing the FIRST keeps the FIRST, though the second shifted', () => {
  const ed = makeEditor(TWINS);
  const [first] = nodeItems(ed, 'Transform');
  ed.selectedId = first.id;
  ed.applyField('translation', ['10', '0', '0']);
  const [newFirst, newSecond] = nodeItems(ed, 'Transform');
  assert.equal(ed.selectedId, newFirst.id);
  assert.notEqual(ed.selectedId, newSecond.id);
});

test('zero wrong anchors: a sweep of Apply actions over every editable field keeps the exact node', () => {
  const src = H
    + 'DEF A Transform { translation 0 0 0 rotation 0 0 1 0 children [\n'
    + '  Transform { translation 0 0 0 children [ Shape { appearance Appearance { material Material { diffuseColor 0.8 0.8 0.8 } } geometry Sphere { radius 1 } } ] }\n'
    + '  Transform { translation 0 0 0 children [ Shape { appearance Appearance { material Material { diffuseColor 0.8 0.8 0.8 } } geometry Sphere { radius 1 } } ] }\n'
    + '] }\n'
    + 'PROTO P [ field SFFloat r 1 ] { Transform { translation 1 1 1 children [ Shape { geometry Sphere { radius IS r } } ] } }\n'
    + 'WorldInfo { title "t" }\n';
  let wrong = 0;
  let proven = 0;
  const base = makeEditor(src);
  const targets = base.analysis.tree.items.filter((it) => it.kind === 'Node').map((it, i) => ({ i, nodeType: it.nodeType }));
  for (const t of targets) {
    const ed = makeEditor(src);
    const items = ed.analysis.tree.items.filter((it) => it.kind === 'Node');
    ed.selectedId = items[t.i].id;
    const editable = ed.fields().fields.filter((f) => f.editable);
    for (const f of editable) {
      const comps = f.components.map((c) => {
        if (f.kind === 'bool') return !c.value;
        if (f.kind === 'string') return `${c.value}x`;
        return String(Math.min(c.value + 0.5, f.bounds && f.bounds.max !== null ? f.bounds.max : Infinity));
      });
      const res = ed.applyField(f.name, comps);
      if (res.status !== 'ready') continue;
      const nowItems = ed.analysis.tree.items.filter((it) => it.kind === 'Node');
      if (ed.selectedId === null) continue; // a safe loss is allowed
      proven += 1;
      if (ed.selectedId !== nowItems[t.i].id) wrong += 1;
    }
  }
  assert.ok(proven >= 8, `the sweep proved ${proven} re-anchors`);
  assert.equal(wrong, 0, 'ZERO wrong re-anchors');
});

test('a node inside a PROTO body re-anchors through the transaction', () => {
  const ed = makeEditor(H + 'PROTO P [ ] { Transform { translation 1 2 3 } }\nP { }\n');
  ed.selectedId = nodeItems(ed, 'Transform')[0].id;
  ed.applyField('translation', ['1', '2', '30']);
  assert.equal(ed.selected().nodeType, 'Transform');
  assert.deepEqual(values(ed, 'translation'), ['1', '2', '30']);
});

test('typing that crosses the selected node boundary LOSES the selection (fail closed)', () => {
  const ed = makeEditor(TWINS);
  ed.selectedId = nodeItems(ed, 'Transform')[0].id;
  // Replace the whole first Transform with an identical copy -- offset
  // arithmetic would "find" it; identity must refuse.
  const from = ed.text().indexOf('Transform');
  const to = ed.text().indexOf('}', from) + 1;
  ed.dispatch({ changes: { from, to, insert: 'Transform { translation 0 0 0 }' } });
  assert.equal(ed.reanalyze().outcome, O.LOST);
  assert.equal(ed.selectedId, null);
});

test('a reload (setDoc) breaks the chain: the selection is lost even for identical text', () => {
  const ed = makeEditor(PRIMARY);
  ed.selectedId = nodeItems(ed, 'Transform')[0].id;
  ed.setDoc(PRIMARY);
  const d = ed.reanalyze();
  assert.equal(d.outcome, O.LOST);
  assert.equal(d.reason, inspectorEdit.LOSS.NO_VERIFIED_TRANSACTION);
});

test('a forged/mismatched chain is rejected by verifyTransaction -> lost', () => {
  const ed = makeEditor(PRIMARY);
  const transform = nodeItems(ed, 'Transform')[0];
  const next = analyse(PRIMARY.replace('translation 0 0 0', 'translation 3 0 0'));
  const at = PRIMARY.indexOf('translation 0') + 'translation '.length;
  const forged = { previousText: PRIMARY, edits: [{ from: at, to: at + 1, insert: '4' }] };
  const d = inspectorEdit.reanchorSelection({
    previous: { session: ed.analysis.session, tree: ed.analysis.tree }, next: { session: next.session, tree: next.tree },
    selectedId: transform.id, chain: forged, pendingApply: null,
  });
  assert.equal(d.outcome, O.LOST);
  // A pending Apply receipt for a DIFFERENT transaction is not used either.
  const otherPlan = fe.planFieldEdit({
    session: ed.analysis.session, currentText: PRIMARY, node: sceneTree.astNodeForItem(ed.analysis.tree, transform.id),
    fieldIndex: 0, fieldName: 'translation', components: ['9', '0', '0'],
  });
  const d2 = inspectorEdit.reanchorSelection({
    previous: { session: ed.analysis.session, tree: ed.analysis.tree }, next: { session: next.session, tree: next.tree },
    selectedId: transform.id, chain: null, pendingApply: { oldText: otherPlan.oldText, newText: otherPlan.newText, receipt: otherPlan.receipt },
  });
  assert.equal(d2.outcome, O.LOST);
});

test('Document root re-selects the new root; USE/ROUTE/PROTO survive only an unchanged text', () => {
  const src = H + 'DEF S Shape { }\nTransform { children [ USE S ] }\nROUTE S.a TO S.b\nPROTO P [ ] { Group { } }\n';
  const ed = makeEditor(src);
  ed.selectedId = ed.analysis.tree.root.id;
  ed.dispatch({ changes: { from: ed.text().length, insert: '# more\n' } });
  assert.equal(ed.reanalyze().outcome, O.REANCHORED);
  assert.equal(ed.selected().kind, 'Document');
  for (const kind of ['Use', 'Route', 'Proto']) {
    const e2 = makeEditor(src);
    e2.selectedId = e2.analysis.tree.items.find((it) => it.kind === kind).id;
    assert.equal(e2.reanalyze().outcome, O.KEPT, `${kind} kept when nothing changed`);
    e2.dispatch({ changes: { from: e2.text().length, insert: '# x\n' } });
    assert.equal(e2.reanalyze().outcome, O.LOST, `${kind} lost after a change`);
  }
});

// ---------------------------------------------------------------------------
// Refusals at the editor boundary
// ---------------------------------------------------------------------------

test('Apply refuses for a non-Node selection and for a stale analysis; nothing mutates', () => {
  const ed = makeEditor(H + 'DEF S Shape { }\nTransform { translation 0 0 0 children [ USE S ] }\n');
  const before = ed.text();
  const use = ed.analysis.tree.items.find((it) => it.kind === 'Use');
  const r1 = inspectorEdit.prepareInspectorApply({ session: ed.analysis.session, tree: ed.analysis.tree, currentText: before, itemId: use.id, fieldIndex: 0, fieldName: 'x', components: ['1'] });
  assert.equal(r1.reason, fe.FIELD_EDIT_REASON.NOT_A_NODE);
  const r2 = inspectorEdit.prepareInspectorApply({ session: ed.analysis.session, tree: ed.analysis.tree, currentText: before, itemId: ed.analysis.tree.root.id, fieldIndex: 0, fieldName: 'x', components: ['1'] });
  assert.equal(r2.reason, fe.FIELD_EDIT_REASON.NOT_A_NODE);
  const tf = nodeItems(ed, 'Transform')[0];
  const r3 = inspectorEdit.prepareInspectorApply({ session: ed.analysis.session, tree: ed.analysis.tree, currentText: before + ' ', itemId: tf.id, fieldIndex: 0, fieldName: 'translation', components: ['1', '0', '0'] });
  assert.equal(r3.reason, fe.FIELD_EDIT_REASON.STALE_SESSION);
  const r4 = inspectorEdit.prepareInspectorApply({ session: ed.analysis.session, tree: ed.analysis.tree, currentText: before, itemId: 'node-1-2', fieldIndex: 0, fieldName: 'translation', components: ['1', '0', '0'] });
  assert.equal(r4.reason, fe.FIELD_EDIT_REASON.STALE_SESSION);
  assert.equal(ed.text(), before);
  assert.equal(undoDepth(ed.state), 0);
});

test('changesToEdits mirrors a composed CodeMirror ChangeSet and verifies', () => {
  const ed = makeEditor('abcdef');
  ed.dispatch({ changes: [{ from: 1, to: 2, insert: 'XY' }, { from: 4, insert: '!' }] });
  ed.dispatch({ changes: { from: 0, to: 1, insert: '' } });
  const edits = inspectorEdit.changesToEdits(ed.chainChanges);
  const receipt = tx.verifyTransaction({ oldText: 'abcdef', edits, newText: ed.text() });
  assert.equal(receipt.status, 'verified');
  assert.deepEqual(inspectorEdit.changesToEdits(null), []);
});

// ---------------------------------------------------------------------------
// Architecture
// ---------------------------------------------------------------------------

const ROOT = path.join(__dirname, '..', '..');
const read = (rel) => fs.readFileSync(path.join(ROOT, rel), 'utf8');

test('architecture: inspector-edit is pure and never restores selection by id/offset/label', () => {
  const src = read('src/editor/inspector-edit.js');
  const requires = [...src.matchAll(/require\(\s*['"]([^'"]+)['"]\s*\)/g)].map((m) => m[1]);
  for (const r of requires) assert.ok(r.startsWith('../vrml/'), `only src/vrml modules: ${r}`);
  const code = src.replace(/\/\/.*$/gm, '').replace(/\/\*[\s\S]*?\*\//g, '');
  for (const banned of [/@codemirror/, /electron/, /\bdocument\./, /\bwindow\./, /itemContainingOffset/, /\.def\b/, /\.label\b/]) {
    assert.ok(!banned.test(code), `inspector-edit.js must not use ${banned}`);
  }
  assert.ok(/resolveTransactionAnchor/.test(code), 'selection survives only through WD1.4 Tier 1');
});

test('architecture: the renderer gates Apply on the pure plan and dispatches through the editor handle', () => {
  const editor = read('renderer/editor.js');
  assert.ok(/prepareInspectorApply\(/.test(editor));
  assert.ok(/applyVerifiedEdits\(/.test(editor));
  assert.ok(/reanchorSelection\(/.test(editor));
  // The WD2-A raw-id retention is gone: the selection is never kept merely
  // because an item with the same id exists after a reparse.
  assert.ok(!/!sceneBridge\.sceneTree\.itemById\(S\.sceneTree, sceneSelection\.getSelection\(\)\)/.test(editor));
  const inspector = read('renderer/scene-inspector.js');
  for (const banned of [/\.from\b/, /\.to\b/, /range\.start\.offset\s*[+-]/, /replaceSpan|applyEdits|createEdit/]) {
    assert.ok(!banned.test(inspector.replace(/\/\/.*$/gm, '')), `the Inspector DOM never computes offsets: ${banned}`);
  }
  const view = read('src/editor/browser/editor-view.js');
  assert.ok(/isolateHistory\.of\('full'\)/.test(view), 'one Apply is one isolated history event');
  assert.ok(/view\.state\.doc\.toString\(\) !== oldText/.test(view), 'dispatch refuses a changed buffer');
});
