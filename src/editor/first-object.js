'use strict';
// First Object actions + beginner properties (Phase WD2-C).
//
// PURE and browser-safe: requires only src/vrml modules. No fs, no Electron, no
// CodeMirror, no DOM. Bundled into the editor view and published on
// `WRLForgeSceneBridge.firstObject`, and required directly by the Node tests --
// one implementation, no test-only path. The sibling of inspector-edit.js:
//
//   * prepare{Add,Duplicate,Delete,PropertySet} -- from the selected scene item
//     to the exact AST node, then through src/vrml/structure-edit.js /
//     simple-object.js to a VERIFIED WD1.2 edit set. The DOM never computes an
//     offset and never builds VRML text.
//   * selectInserted -- after the analysis of an Add / Duplicate, the item of
//     the node that verified insertion created (structure-edit
//     resolveInsertedNode), or null. Never "the newest Box", never a name.
//   * objectForItem / displayLabels -- the beginner view of the selection and
//     the display-only "Box"/"Sphere" names for the scene tree.
//   * refusalText -- one plain sentence per refusal reason.

const sceneTree = require('../vrml/scene-tree');
const structureEdit = require('../vrml/structure-edit');
const simpleObject = require('../vrml/simple-object');
const fieldEdit = require('../vrml/field-edit');
const tx = require('../vrml/document-transaction');

const R = structureEdit.STRUCTURE_REASON;
const FR = fieldEdit.FIELD_EDIT_REASON;
const refused = (reason) => Object.freeze({ status: fieldEdit.PLAN_STATUS.REFUSED, reason });

// The selected item's exact AST node of the same build, or a refusal.
function nodeFor({ session, tree, itemId }) {
  if (!tx.isParseSession(session) || !tree) return { refusal: refused(FR.STALE_SESSION) };
  const item = sceneTree.itemById(tree, itemId);
  if (!item) return { refusal: refused(FR.STALE_SESSION) };
  if (item.kind !== sceneTree.KIND.NODE) return { refusal: refused(FR.NOT_A_NODE) };
  const node = sceneTree.astNodeForItem(tree, itemId);
  return node ? { node, item } : { refusal: refused(FR.NOT_A_NODE) };
}

function prepareAdd({ session, currentText, primitive }) {
  if (!tx.isParseSession(session)) return refused(FR.STALE_SESSION);
  return structureEdit.planInsertObject({ session, currentText, primitive });
}

function prepareDuplicate({ session, tree, currentText, itemId }) {
  const n = nodeFor({ session, tree, itemId });
  return n.refusal || structureEdit.planDuplicateNode({ session, currentText, node: n.node });
}

function prepareDelete({ session, tree, currentText, itemId }) {
  const n = nodeFor({ session, tree, itemId });
  return n.refusal || structureEdit.planDeleteNode({ session, currentText, node: n.node });
}

function preparePropertySet({ session, tree, currentText, itemId, key, components }) {
  const n = nodeFor({ session, tree, itemId });
  return n.refusal || simpleObject.planPropertySet({ session, currentText, node: n.node, key, components });
}

/**
 * The beginner view of the selected item: `{itemId, primitive, properties}` for
 * a recognised simple object, else null.
 */
function objectForItem({ session, tree, currentText, itemId }) {
  const n = nodeFor({ session, tree, itemId });
  if (n.refusal) return null;
  const d = simpleObject.describeObject(session, n.node, { currentText });
  return d.reason ? null : Object.freeze({ itemId, primitive: d.primitive, properties: d.properties });
}

/** itemId -> 'Box' | 'Sphere' for every recognised simple object. Display only. */
function displayLabels(tree) {
  const out = new Map();
  if (!tree || !Array.isArray(tree.items)) return out;
  for (const item of tree.items) {
    if (item.kind !== sceneTree.KIND.NODE || item.nodeType !== 'Transform') continue;
    const label = simpleObject.displayLabel(sceneTree.astNodeForItem(tree, item.id));
    if (label) out.set(item.id, label);
  }
  return out;
}

/**
 * After the analysis of a dispatched Add / Duplicate: the scene item of the
 * node its verified insertion created, or null (the caller then clears the
 * selection visibly). `pendingApply` must be exactly that transaction.
 */
function selectInserted({ next, pendingApply }) {
  if (!next || !tx.isParseSession(next.session) || !next.tree || !pendingApply || !pendingApply.select) return null;
  if (pendingApply.newText !== next.session.text) return null;
  const r = structureEdit.resolveInsertedNode({
    session: next.session,
    receipt: pendingApply.receipt,
    span: pendingApply.select.span,
    nodeType: pendingApply.select.nodeType,
  });
  if (r.status !== structureEdit.INSERTED_STATUS.RESOLVED) return null;
  const item = sceneTree.itemForAstNode(next.tree, r.node);
  return item ? item.id : null;
}

const UNPROVABLE = 'Cannot make this change because its exact source location cannot be proved.';
const REFUSAL_TEXT = Object.freeze({
  [R.CONTAINS_DEF]: 'Cannot duplicate this object because it contains a DEF name that would conflict.',
  [R.CONTAINS_USE]: 'Cannot duplicate this object because it contains a USE of a shared node.',
  [R.CONTAINS_ROUTE_OR_PROTO]: 'Cannot duplicate this object because it contains a ROUTE or PROTO.',
  [R.REFERENCED]: 'Cannot delete this object because other nodes reference it.',
  [R.IN_PROTO]: 'Cannot change this node because it is inside a PROTO definition.',
  [R.NOT_IN_NODE_LIST]: 'Cannot change this node because it is the only value of a single-node field.',
  [R.PARENT_NOT_PROVABLE]: 'Cannot change this node because its parent field cannot be proved to be a node list.',
  [R.SYNTAX_ERRORS]: 'Cannot change the scene while the document has syntax errors. Fix them in Source first.',
  [R.PARSE_INCOMPLETE]: 'Cannot change the scene because the document exceeded a parse limit.',
  [R.NO_HEADER]: 'Cannot add an object because this document has no VRML97 header (#VRML V2.0 utf8).',
  [R.HEADER_NOT_VRML97]: 'Cannot add an object because this document is not VRML97 (#VRML V2.0 utf8).',
  [R.STALE_SESSION]: 'The document changed; try again once it has been analysed.',
  [R.NOT_A_NODE]: 'Select a node first.',
  [R.NODE_INCOMPLETE]: 'Cannot change this node because it is incomplete.',
  [R.UNKNOWN_NODE_TYPE]: 'Cannot change this node because it is not a standard VRML97 node.',
  [R.PROTO_INSTANCE]: 'Cannot change this node because its type is a PROTO.',
  [R.NODE_NOT_IN_SESSION]: 'The document changed; try again once it has been analysed.',
  [R.UNSUPPORTED_PRIMITIVE]: 'That object type cannot be added yet.',
  [R.FIELD_ALREADY_AUTHORED]: UNPROVABLE,
  [R.BODY_NOT_LOCATED]: UNPROVABLE,
  [R.TOKENS_CHANGED]: UNPROVABLE,
  [R.ROUND_TRIP_FAILED]: UNPROVABLE,
  [R.TRANSACTION_REJECTED]: UNPROVABLE,
  [FR.FIELD_DUPLICATED]: 'Cannot edit this value because the field is written more than once.',
  [FR.IS_BOUND]: 'Cannot edit this value because it is connected to a PROTO interface (IS).',
  [simpleObject.OBJECT_REASON.NO_MATERIAL]: 'This object has no Material, so its color cannot be changed here.',
  [simpleObject.OBJECT_REASON.NOT_A_SIMPLE_OBJECT]: 'Select a Box or Sphere object to edit its properties.',
});

/** One plain sentence for a refusal, with any names it carries. */
function refusalText(plan) {
  const reason = plan && plan.reason;
  if (plan && typeof plan.message === 'string' && plan.message) return plan.message;
  const base = REFUSAL_TEXT[reason] || `Cannot make this change (${reason || 'unknown reason'}).`;
  const names = plan && Array.isArray(plan.names) && plan.names.length ? ` (${plan.names.join(', ')})` : '';
  return names ? base.replace(/\.$/, `${names}.`) : base;
}

module.exports = {
  REFUSAL_TEXT,
  prepareAdd,
  prepareDuplicate,
  prepareDelete,
  preparePropertySet,
  objectForItem,
  displayLabels,
  selectInserted,
  refusalText,
};
