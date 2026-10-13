'use strict';
// Inspector field editing + selection survival (Phase WD2-B).
//
// PURE and browser-safe: requires only src/vrml modules. No fs, no Electron, no
// CodeMirror, no DOM. Bundled into the editor view (src/editor/browser/
// editor-view.js) and published to the renderer on `WRLForgeSceneBridge`, and
// required directly by the Node tests -- one implementation, no test-only path.
//
// It owns two decisions the renderer must not make by itself:
//
//   1. prepareInspectorApply -- from the selected scene item to the exact AST
//      node, then through src/vrml/field-edit.js to a verified WD1.2 edit set.
//      The DOM never computes an offset.
//
//   2. reanchorSelection -- after every analysis, which item (if any) is the
//      SAME node the user had selected. The answer comes only from the WD1.4
//      identity layer: a Tier 1 transaction anchor resolved through a VERIFIED
//      receipt for the exact (previous analysed text -> new analysed text)
//      transaction. Never by item id, label, DEF name, row number, raw offset or
//      nearest match. A selection that cannot be proven is LOST, visibly; the
//      hard gate is zero wrong re-anchors.
//
// Where the receipt comes from:
//   * an Inspector Apply already holds the receipt it was gated on; when the
//     analysis is exactly that transaction, that receipt is used as-is;
//   * any other change (typing, Undo, Redo, a QA setText) arrives as the edit
//     chain the editor view composed from CodeMirror's own change sets since
//     the previous analysis, and is verified here before it is trusted;
//   * a reload / reopen / setDoc arrives with NO chain, so nothing can be proven
//     and a selection is lost -- Tier 1 must never survive a broken chain.

const sceneTree = require('../vrml/scene-tree');
const fieldEdit = require('../vrml/field-edit');
const identity = require('../vrml/node-identity');
const tx = require('../vrml/document-transaction');

const OUTCOME = Object.freeze({
  NONE: 'none',             // nothing was selected
  KEPT: 'kept',             // proven, and the item id did not change
  REANCHORED: 'reanchored', // proven, and the item now has a different id
  LOST: 'lost',             // could not be proven -> the selection must clear
});

const LOSS = Object.freeze({
  NO_PREVIOUS_ANALYSIS: 'no-previous-analysis',
  NOT_IN_PREVIOUS_TREE: 'selection-not-in-previous-tree',
  NO_VERIFIED_TRANSACTION: 'no-verified-transaction',
  NO_AST_NODE: 'selection-has-no-ast-node',
  NO_ITEM_FOR_NODE: 'no-scene-item-for-reanchored-node',
  NOT_PROVABLE_KIND: 'selection-kind-not-provable-across-a-change',
});

const result = (outcome, id, reason) => Object.freeze({ outcome, id: id == null ? null : id, reason: reason || null });

// The verified receipt for previous.text -> next.text, or null.
function receiptFor(previousSession, nextSession, chain, pendingApply) {
  if (pendingApply && pendingApply.oldText === previousSession.text
      && pendingApply.newText === nextSession.text && tx.isVerifiedReceipt(pendingApply.receipt)) {
    return pendingApply.receipt;
  }
  if (!chain || chain.previousText !== previousSession.text || !Array.isArray(chain.edits)) return null;
  const receipt = tx.verifyTransaction({ oldText: previousSession.text, edits: chain.edits, newText: nextSession.text });
  return receipt.status === tx.TX_STATUS.VERIFIED ? receipt : null;
}

/**
 * Decide which item, if any, is the selection after a new analysis.
 *
 * @param {object} input
 * @param {{session:object, tree:object}|null} input.previous The analysis the
 *   selection was made against (parse session + scene tree).
 * @param {{session:object, tree:object}} input.next The new analysis.
 * @param {string|null} input.selectedId The selected item id in `previous.tree`.
 * @param {{previousText:string, edits:Array}|null} input.chain The editor's
 *   composed change set since the previous analysis, as WD1.2 edits.
 * @param {{oldText:string, newText:string, receipt:object}|null} input.pendingApply
 *   The receipt of an Inspector Apply dispatched since the previous analysis.
 * @returns {{outcome:string, id:string|null, reason:string|null}} Frozen.
 */
function reanchorSelection(input) {
  const { previous, next, selectedId, chain, pendingApply } = input || {};
  if (selectedId == null) return result(OUTCOME.NONE, null, null);
  if (!previous || !tx.isParseSession(previous.session) || !previous.tree
      || !next || !tx.isParseSession(next.session) || !next.tree) {
    return result(OUTCOME.LOST, null, LOSS.NO_PREVIOUS_ANALYSIS);
  }
  const item = sceneTree.itemById(previous.tree, selectedId);
  if (!item) return result(OUTCOME.LOST, null, LOSS.NOT_IN_PREVIOUS_TREE);
  const receipt = receiptFor(previous.session, next.session, chain, pendingApply);
  if (!receipt) return result(OUTCOME.LOST, null, LOSS.NO_VERIFIED_TRANSACTION);

  // The document root is unique by construction: there is exactly one.
  if (item.kind === sceneTree.KIND.DOCUMENT) {
    const id = next.tree.root ? next.tree.root.id : null;
    return id === selectedId ? result(OUTCOME.KEPT, id) : result(OUTCOME.REANCHORED, id);
  }

  if (item.kind === sceneTree.KIND.NODE) {
    const astNode = sceneTree.astNodeForItem(previous.tree, selectedId);
    if (!astNode) return result(OUTCOME.LOST, null, LOSS.NO_AST_NODE);
    const created = identity.createTransactionAnchor(previous.session, astNode);
    if (created.status !== identity.ANCHOR_STATUS.CREATED) return result(OUTCOME.LOST, null, created.reason);
    const resolved = identity.resolveTransactionAnchor(created.anchor, next.session, receipt);
    // `ambiguous` is a loss too: identity refused to choose, so neither do we.
    if (!identity.isResolved(resolved)) return result(OUTCOME.LOST, null, resolved.reason);
    const nextItem = sceneTree.itemForAstNode(next.tree, resolved.node);
    if (!nextItem) return result(OUTCOME.LOST, null, LOSS.NO_ITEM_FOR_NODE);
    return nextItem.id === selectedId
      ? result(OUTCOME.KEPT, nextItem.id, resolved.reason)
      : result(OUTCOME.REANCHORED, nextItem.id, resolved.reason);
  }

  // USE / PROTO / EXTERNPROTO / ROUTE: WD1.4 identity covers node INSTANCES
  // only. Byte-identical text parses deterministically to the same items, so an
  // unchanged document keeps them; any real change loses them.
  if (previous.session.text === next.session.text) {
    const same = sceneTree.itemById(next.tree, selectedId);
    if (same && same.kind === item.kind) return result(OUTCOME.KEPT, selectedId);
  }
  return result(OUTCOME.LOST, null, LOSS.NOT_PROVABLE_KIND);
}

/**
 * A CodeMirror ChangeSet (or anything with the same `iterChanges` shape) as a
 * WD1.2 edit list: `{from, to, insert}` against the change set's START document.
 * Duck-typed so this module never imports CodeMirror. Null -> []. The result is
 * evidence for `verifyTransaction`, never trusted unverified.
 */
function changesToEdits(changes) {
  const edits = [];
  if (!changes) return edits;
  changes.iterChanges((fromA, toA, _fromB, _toB, inserted) => {
    edits.push({ from: fromA, to: toA, insert: inserted.toString() });
  });
  return edits;
}

/**
 * The Inspector's field list for one selected item, or null for an item kind
 * that has no field values (Document / USE / PROTO / EXTERNPROTO / ROUTE).
 */
function fieldsForItem({ session, tree, currentText, itemId }) {
  const item = sceneTree.itemById(tree, itemId);
  if (!item || item.kind !== sceneTree.KIND.NODE || !tx.isParseSession(session)) return null;
  const node = sceneTree.astNodeForItem(tree, itemId);
  if (!node) return null;
  return fieldEdit.inspectNodeFields(session, node, { currentText });
}

/**
 * Plan one Inspector Apply against the selected item. Returns the
 * src/vrml/field-edit.js plan verbatim (ready / unchanged / refused).
 */
function prepareInspectorApply({ session, tree, currentText, itemId, fieldIndex, fieldName, components }) {
  const R = fieldEdit.FIELD_EDIT_REASON;
  const refuse = (reason) => Object.freeze({ status: fieldEdit.PLAN_STATUS.REFUSED, reason });
  if (!tx.isParseSession(session) || !tree) return refuse(R.STALE_SESSION);
  const item = sceneTree.itemById(tree, itemId);
  if (!item) return refuse(R.STALE_SESSION);
  if (item.kind !== sceneTree.KIND.NODE) return refuse(R.NOT_A_NODE);
  const node = sceneTree.astNodeForItem(tree, itemId);
  if (!node) return refuse(R.NOT_A_NODE);
  return fieldEdit.planFieldEdit({ session, currentText, node, fieldIndex, fieldName, components });
}

module.exports = {
  OUTCOME,
  LOSS,
  reanchorSelection,
  changesToEdits,
  fieldsForItem,
  prepareInspectorApply,
};
