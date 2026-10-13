'use strict';
// WD2-B -- the scene tree's private item <-> AST link (astNodeForItem /
// itemForAstNode). Object identity within one build only; the WD2-A read-model
// guarantees (frozen items, read-only maps, item key set) are unchanged.

const test = require('node:test');
const assert = require('node:assert/strict');

const vrml = require('../../src/vrml');
const st = require('../../src/vrml/scene-tree');
const { NODE, walk } = require('../../src/vrml/ast');

const SRC = '#VRML V2.0 utf8\n'
  + 'PROTO P [ ] { Transform { } }\n'
  + 'EXTERNPROTO E [ ] "e.wrl"\n'
  + 'DEF S Shape { geometry Box { } }\n'
  + 'Group { children [ Transform { } Transform { } USE S ] }\n'
  + 'ROUTE S.a TO S.b\n';

test('every item links to the exact AST object it was emitted from, and back', () => {
  const parsed = vrml.parse(SRC);
  const tree = st.buildSceneTree(parsed);
  for (const item of tree.items) {
    const astNode = st.astNodeForItem(tree, item.id);
    assert.ok(astNode, `item ${item.id} has an AST object`);
    assert.equal(st.itemForAstNode(tree, astNode), item);
    const expectedType = { Document: NODE.DOCUMENT, Node: NODE.NODE, Use: NODE.USE, Proto: NODE.PROTO, ExternProto: NODE.EXTERNPROTO, Route: NODE.ROUTE }[item.kind];
    assert.equal(astNode.type, expectedType);
    if (item.kind === 'Node') assert.equal(astNode.nodeType, item.nodeType);
  }
  // Two byte-identical Transforms are two distinct AST objects and items.
  const transforms = [];
  walk(parsed.tree, (n) => { if (n.type === NODE.NODE && n.nodeType === 'Transform') transforms.push(n); });
  const ids = transforms.map((n) => st.itemForAstNode(tree, n).id);
  assert.equal(new Set(ids).size, transforms.length);
});

test('a node from another parse -- or another tree of the same parse -- never matches', () => {
  const a = vrml.parse(SRC);
  const b = vrml.parse(SRC);
  const ta = st.buildSceneTree(a);
  const tb = st.buildSceneTree(b);
  const node = st.astNodeForItem(ta, ta.items.find((it) => it.kind === 'Node').id);
  assert.equal(st.itemForAstNode(tb, node), null);
  assert.equal(st.astNodeForItem({ items: [] }, 'node-0-1'), null, 'a look-alike object has no links');
  assert.equal(st.astNodeForItem(ta, 'node-999999-1000000'), null);
  assert.equal(st.itemForAstNode(ta, null), null);
});

test('F5/C3 preserved: the tree stays frozen with read-only maps and the WD2-A item key set', () => {
  const tree = st.buildSceneTree(vrml.parse(SRC));
  assert.ok(Object.isFrozen(tree));
  assert.throws(() => tree.byId.set('x', 1), TypeError);
  assert.equal(tree.byId.size, tree.items.length);
  for (const item of tree.items) {
    assert.ok(Object.isFrozen(item));
    assert.equal(Object.keys(item).some((k) => /ast/i.test(k)), false, 'no AST reference leaks onto an item');
  }
  assert.deepEqual(Object.keys(tree).sort(), ['byId', 'defsByName', 'items', 'root', 'totals']);
});
