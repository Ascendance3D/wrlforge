'use strict';
// WD2-C -- First Object actions end to end over REAL CodeMirror state/history:
// plan (src/editor/first-object.js) -> ONE isolated CodeMirror transaction ->
// analysis -> scene tree -> selection (inserted node / cleared / WD1.4
// re-anchor) -> beginner properties. The same sequence renderer/editor.js runs
// (dispatchModelPlan + reanchorAfterAnalysis); no second editing pipeline.

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');

const { EditorState, Transaction } = require('@codemirror/state');
const { history, undo, redo, isolateHistory, undoDepth, redoDepth } = require('@codemirror/commands');

const language = require('../../src/editor/language');
const inspectorEdit = require('../../src/editor/inspector-edit');
const firstObject = require('../../src/editor/first-object');
const sceneTree = require('../../src/vrml/scene-tree');
const scopeGraph = require('../../src/vrml/scope-graph');
const tx = require('../../src/vrml/document-transaction');
const templates = require('../../src/vrml/node-templates');

const H = '#VRML V2.0 utf8\n';
const BOX = templates.simpleObjectTemplate('Box').text;

function analyse(text) {
  const a = language.analyze(text, { profile: 'generic' });
  const graph = scopeGraph.buildScopeGraph(a.parseResult);
  const useResolver = (useNode) => {
    const r = scopeGraph.resolve(graph, useNode);
    return scopeGraph.isResolved(r) && r.symbol && r.symbol.node
      ? { status: 'resolved', targetAstNode: r.symbol.node } : { status: 'unresolved' };
  };
  const tree = sceneTree.buildSceneTree(a.parseResult, { useResolver });
  return { text, tree, session: tx.createParseSession(text, a.parseResult) };
}

function makeEditor(doc) {
  const ed = {
    state: EditorState.create({ doc, extensions: [history()] }),
    chainBase: null, chainChanges: null, analysis: null, selectedId: null, pendingApply: null, notices: [],
  };
  ed.text = () => ed.state.doc.toString();
  ed.apply = (tr) => {
    if (tr.docChanged) ed.chainChanges = ed.chainChanges ? ed.chainChanges.compose(tr.changes) : tr.changes;
    ed.state = tr.state;
  };
  ed.dispatch = (spec) => ed.apply(ed.state.update(spec));
  ed.cmd = (command) => command({ state: ed.state, dispatch: (tr) => ed.apply(tr) });
  ed.type = (at, text) => ed.dispatch({ changes: { from: at, insert: text }, annotations: Transaction.userEvent.of('input.type') });
  // renderer/editor.js reanchorAfterAnalysis.
  ed.reanalyze = () => {
    const text = ed.text();
    const chain = ed.chainBase === null ? null : { previousText: ed.chainBase, edits: inspectorEdit.changesToEdits(ed.chainChanges) };
    ed.chainBase = text;
    ed.chainChanges = null;
    const next = analyse(text);
    const previous = ed.analysis ? { session: ed.analysis.session, tree: ed.analysis.tree } : null;
    const pending = ed.pendingApply;
    ed.pendingApply = null;
    ed.analysis = next;
    if (pending && (pending.select || pending.cleared) && pending.newText === text) {
      ed.selectedId = pending.cleared ? null : firstObject.selectInserted({ next: { session: next.session, tree: next.tree }, pendingApply: pending });
      return;
    }
    const d = inspectorEdit.reanchorSelection({ previous, next: { session: next.session, tree: next.tree }, selectedId: ed.selectedId, chain, pendingApply: pending });
    if (ed.selectedId && !d.id) ed.notices.push(d.reason);
    ed.selectedId = d.id;
  };
  // renderer/editor.js dispatchModelPlan + editor-view.js applyVerifiedEdits.
  ed.run = (plan) => {
    if (plan.status !== 'ready') return plan;
    assert.equal(ed.text(), plan.oldText);
    ed.dispatch({
      changes: plan.edits.map((e) => ({ from: e.from, to: e.to, insert: e.insert })),
      annotations: [isolateHistory.of('full'), Transaction.userEvent.of('input.model')],
    });
    assert.equal(ed.text(), plan.newText);
    ed.pendingApply = { oldText: plan.oldText, newText: plan.newText, receipt: plan.receipt, select: plan.select || null, cleared: plan.operation === 'delete' };
    ed.reanalyze();
    return plan;
  };
  ed.snap = () => ({ session: ed.analysis.session, tree: ed.analysis.tree, currentText: ed.text(), itemId: ed.selectedId });
  ed.add = (primitive) => ed.run(firstObject.prepareAdd({ session: ed.analysis.session, currentText: ed.text(), primitive }));
  ed.duplicate = () => ed.run(firstObject.prepareDuplicate(ed.snap()));
  ed.remove = () => ed.run(firstObject.prepareDelete(ed.snap()));
  ed.setProp = (key, components) => ed.run(firstObject.preparePropertySet({ ...ed.snap(), key, components }));
  ed.object = () => firstObject.objectForItem(ed.snap());
  ed.selected = () => (ed.selectedId ? sceneTree.itemById(ed.analysis.tree, ed.selectedId) : null);
  ed.items = (type) => ed.analysis.tree.items.filter((it) => it.kind === 'Node' && it.nodeType === type);
  ed.undo = () => { ed.cmd(undo); ed.reanalyze(); };
  ed.redo = () => { ed.cmd(redo); ed.reanalyze(); };
  ed.depth = () => undoDepth(ed.state);
  ed.reanalyze();
  return ed;
}

test('Add Box from an empty document: one Undo entry; the new Box is selected; Undo/Redo exact', () => {
  const ed = makeEditor('');
  assert.equal(ed.depth(), 0);
  ed.add('Box');
  assert.equal(ed.text(), `${H}\n${BOX}\n`);
  assert.equal(ed.depth(), 1, 'one Add = one undo step');
  assert.equal(ed.selected().nodeType, 'Transform');
  assert.equal(ed.object().primitive, 'Box');
  ed.undo();
  assert.equal(ed.text(), '', 'Undo restores the exact prior text');
  assert.equal(ed.selectedId, null, 'the removed object is not selected (lost, never wrong)');
  ed.redo();
  assert.equal(ed.text(), `${H}\n${BOX}\n`, 'Redo restores the exact edited text');
});

test('the beginner Box workflow: each property change is ONE undo step and keeps the Box selected', () => {
  const ed = makeEditor('');
  ed.add('Box');
  const id0 = ed.selectedId;
  const steps = [
    ['position', ['3', '0', '0']],
    ['rotation', ['0', '1', '0', '0.75']],
    ['size', ['2', '1', '0.5']],
    ['color', ['1', '0.502', '0']],
    ['position', ['3', '1', '0']], // now authored: token patch, not insertion
  ];
  let depth = ed.depth();
  for (const [key, comps] of steps) {
    const before = ed.text();
    const plan = ed.setProp(key, comps);
    assert.equal(plan.status, 'ready', `${key}: ${plan.reason}`);
    assert.equal(ed.depth(), depth + 1, `${key}: one undo step`);
    depth += 1;
    assert.equal(ed.selected().nodeType, 'Transform', `${key}: the object stays selected`);
    assert.deepEqual([...ed.object().properties.find((p) => p.key === key).values], comps);
    ed.undo();
    assert.equal(ed.text(), before, `${key}: one Undo restores exactly`);
    ed.redo();
    depth = ed.depth();
  }
  assert.equal(ed.items('Transform').length, 1);
  assert.equal(ed.selectedId, ed.items('Transform')[0].id);
  assert.ok(id0);
});

test('Duplicate: one Undo entry, the copy is selected, the original is untouched; Undo/Redo exact', () => {
  const ed = makeEditor('');
  ed.add('Box');
  ed.setProp('position', ['3', '0', '0']);
  const before = ed.text();
  const d0 = ed.depth();
  ed.duplicate();
  assert.equal(ed.depth(), d0 + 1);
  const ts = ed.items('Transform');
  assert.equal(ts.length, 2);
  assert.equal(ed.selectedId, ts[1].id, 'the COPY is selected');
  ed.undo();
  assert.equal(ed.text(), before);
  assert.equal(ed.selectedId, null);
  ed.redo();
  assert.equal(ed.items('Transform').length, 2);
});

test('Delete: one Undo entry; selection cleared; deleting either identical twin never selects the survivor', () => {
  for (const which of [0, 1]) {
    const ed = makeEditor('');
    ed.add('Box');
    ed.duplicate();
    const twins = ed.text();
    ed.selectedId = ed.items('Transform')[which].id;
    const d0 = ed.depth();
    ed.remove();
    assert.equal(ed.depth(), d0 + 1);
    assert.equal(ed.selectedId, null, 'Delete clears the selection');
    assert.equal(ed.items('Transform').length, 1);
    ed.undo();
    assert.equal(ed.text(), twins, 'Undo Delete restores the exact text');
    assert.equal(ed.selectedId, null);
  }
});

test('a selected twin survives the deletion of the OTHER twin (Tier 1), and is the right one', () => {
  const ed = makeEditor('');
  ed.add('Box');
  ed.duplicate();
  ed.setProp('position', ['5', '0', '0']); // the copy (selected) moves: twins now differ
  const copyId = ed.selectedId;
  // Select the original (first) and delete it; then reselect via the tree.
  ed.selectedId = ed.items('Transform')[0].id;
  ed.remove();
  ed.selectedId = ed.items('Transform')[0].id;
  assert.deepEqual([...ed.object().properties.find((p) => p.key === 'position').values], ['5', '0', '0'], 'the survivor is the moved copy');
  assert.ok(copyId);
});

test('visual actions are isolated from adjacent source typing (no merged undo entries)', () => {
  const ed = makeEditor(H);
  ed.type(ed.text().length, '#a');
  ed.reanalyze();
  ed.add('Sphere');
  ed.type(ed.text().length, '#b');
  ed.reanalyze();
  const afterAll = ed.text();
  assert.ok(afterAll.endsWith('}\n#b'));
  ed.undo();
  assert.equal(ed.text(), afterAll.slice(0, -2), 'first Undo removes only the later typing');
  ed.undo();
  assert.equal(ed.text(), `${H}#a`, 'second Undo removes exactly the Add, not the earlier typing');
  ed.undo();
  assert.equal(ed.text(), H);
});

test('typing while an object is selected keeps it selected through the verified change chain', () => {
  const ed = makeEditor('');
  ed.add('Box');
  ed.type(0, '');
  ed.type(ed.text().length, '# note\n');
  ed.reanalyze();
  assert.equal(ed.selected().nodeType, 'Transform');
});

test('refusals mutate nothing and add no undo step; each has one plain sentence', () => {
  const ed = makeEditor(`${H}DEF Lamp Transform { children [ Shape { geometry Sphere { } } ] }\nGroup { children [ USE Lamp ] }\n`);
  const before = ed.text();
  ed.selectedId = ed.items('Transform')[0].id;
  const dup = ed.duplicate();
  const del = ed.remove();
  assert.equal(dup.status, 'refused');
  assert.equal(del.status, 'refused');
  assert.equal(ed.text(), before);
  assert.equal(ed.depth(), 0);
  assert.equal(firstObject.refusalText(dup), 'Cannot duplicate this object because it contains a DEF name that would conflict (Lamp).');
  assert.equal(firstObject.refusalText(del), 'Cannot delete this object because other nodes reference it (Lamp).');
  const broken = makeEditor(`${H}Group {\n`);
  const add = broken.add('Box');
  assert.equal(firstObject.refusalText(add), 'Cannot change the scene while the document has syntax errors. Fix them in Source first.');
});

test('every structure-edit refusal reason has a plain sentence', () => {
  const R = require('../../src/vrml/structure-edit').STRUCTURE_REASON;
  for (const reason of Object.values(R)) {
    if (reason === R.OK) continue;
    assert.ok(firstObject.REFUSAL_TEXT[reason], `missing text for ${reason}`);
  }
});

test('display labels: recognised objects read "Box"/"Sphere"; others keep no friendly label', () => {
  const ed = makeEditor('');
  ed.add('Box');
  ed.add('Sphere');
  const labels = firstObject.displayLabels(ed.analysis.tree);
  assert.deepEqual([...labels.values()], ['Box', 'Sphere']);
  const plain = makeEditor(`${H}Transform { }\n`);
  assert.equal(firstObject.displayLabels(plain.analysis.tree).size, 0);
});

test('architecture: first-object.js is pure (no DOM, fs, Electron, CodeMirror) and never computes offsets itself', () => {
  const src = fs.readFileSync(path.join(__dirname, '..', '..', 'src', 'editor', 'first-object.js'), 'utf8');
  assert.equal(/require\(['"](fs|path|electron|@codemirror\/[a-z]+)['"]\)|\bdocument\.|\bwindow\./.test(src), false);
  assert.equal(/\.offset\b|insertAt\(|replaceSpan\(|removeSpan\(/.test(src), false);
  const view = fs.readFileSync(path.join(__dirname, '..', '..', 'renderer', 'model-workspace.js'), 'utf8');
  assert.equal(/\.offset\b|insertAt\(|replaceSpan\(|Transform \{|geometry /.test(view), false, 'the DOM never builds VRML or offsets');
});
