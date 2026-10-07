'use strict';
// WD2-D -- viewport pick resolver: plain-data X_ITE hit snapshot -> exact
// authored occurrence -> existing scene-tree item, or an explicit refusal.
//
// PURE and browser-safe: requires only src/vrml modules. No X_ITE (private or
// public), no DOM, no Electron. Bundled into the editor view and published on
// `WRLForgeSceneBridge.viewportPick`, and required directly by the Node tests.
// The snapshot it consumes is produced by src/preview/xite-pick-adapter.js,
// the only module that touches X_ITE.
//
// WHERE THE PROOF COMES FROM (WD2-C0 spike §12, ported):
//   1. Provenance: X_ITE's own parser reported that runtime object R came from
//      the node statement at [start,end) of the EXACT generation text.
//   2. Exact join: [start,end) equals ONE WRL Forge AST Node range, both ends.
//      There is no nearest / containing / overlapping lookup.
//   3. Uniqueness: R and every document ancestor have exactly ONE source
//      occurrence and ONE live runtime parent, so R is rendered once and "the
//      hit Shape" is "the clicked occurrence". A USE anywhere refuses.
//   4. Agreement: the runtime chain equals the AST containment chain, link by
//      link (each link proven by its own span) -- a consistency check, not a
//      matcher.
//
// NO FALLBACK. Every failed step is a refusal. Nothing here retries by name,
// type, index, offset proximity, similarity or matrix.

const { NODE, walk } = require('../vrml/ast');
const sceneTree = require('../vrml/scene-tree');
const simpleObject = require('../vrml/simple-object');

const STATUS = Object.freeze({
  PROVEN: 'PROVEN',
  NO_HIT: 'NO_HIT',
  REFUSED_AMBIGUOUS: 'REFUSED_AMBIGUOUS',
  REFUSED_EXTERNAL: 'REFUSED_EXTERNAL',
  REFUSED_SENSOR_CONFLICT: 'REFUSED_SENSOR_CONFLICT',
  REFUSED_STALE: 'REFUSED_STALE',
  UNSUPPORTED: 'UNSUPPORTED',
  COMPATIBILITY_DISABLED: 'COMPATIBILITY_DISABLED',
});

const REASON = Object.freeze({
  SHAPE: 'shape',
  PROMOTED: 'shape-promoted-to-proven-simple-object',
  NO_GEOMETRY: 'no-geometry-under-pointer',
  VIEWER_ACTIVE_OR_NO_HIT: 'viewer-active-or-no-hit',
  SEVERAL_OCCURRENCES: 'runtime-node-referenced-by-several-source-occurrences',
  SEVERAL_PARENTS: 'runtime-node-has-several-live-parents',
  OTHER_DOCUMENT: 'hit-belongs-to-another-document',
  ROOT_IS_OTHER_DOCUMENT: 'preview-root-is-another-document',
  SENSOR: 'pointing-device-sensor-under-pointer',
  OTHER_GENERATION: 'hit-from-another-preview-generation',
  SOURCE_CHANGED: 'source-changed-since-preview',
  LAST_VALID_SCENE: 'preview-shows-last-valid-scene',
  SCENE_REPLACED: 'preview-scene-replaced',
  PROTO_INSTANCE: 'proto-instance',
  SYNTAX_ERRORS: 'document-has-syntax-errors',
  NO_PROVENANCE: 'runtime-node-has-no-parse-provenance',
  DETACHED: 'runtime-node-detached',
  PARENT_OUTSIDE_DOCUMENT: 'runtime-parent-outside-document',
  CHAIN_UNBOUNDED: 'runtime-chain-unbounded',
  JOIN_TYPE: 'exact-span-join-type-disagrees',
  CHAIN_DISAGREES: 'runtime-chain-disagrees-with-source-containment',
  NOT_IN_TREE: 'logical-node-not-in-scene-tree',
  GENERATION_UNPROVABLE: 'generation-unprovable',
  NOT_THE_DOCUMENT: 'preview-is-not-the-document',
});

// `exact-span-join-found-<n>` is built per result.
const joinFound = (n) => `exact-span-join-found-${n}`;

// Contextual one-line UX text per status (contract §Refusal states).
const STATUS_TEXT = Object.freeze({
  REFUSED_AMBIGUOUS: 'This object is drawn by a shared (DEF/USE) node; select it in the Scene Tree.',
  REFUSED_EXTERNAL: 'This object belongs to an Inline file and cannot be selected from this document.',
  REFUSED_SENSOR_CONFLICT: 'This object is interactive (sensor or link); select it in the Scene Tree.',
  REFUSED_STALE: 'The preview is out of date; select after it updates.',
  UNSUPPORTED: 'This object cannot be selected from the preview; select it in the Scene Tree.',
});

function refusalText(result) {
  if (!result) return '';
  if (result.status === STATUS.COMPATIBILITY_DISABLED) return compatibilityText(result.reason);
  if (result.status === STATUS.REFUSED_EXTERNAL && result.reason === REASON.ROOT_IS_OTHER_DOCUMENT) {
    return 'The preview shows the full World, not this file; select it in the Scene Tree.';
  }
  return STATUS_TEXT[result.status] || '';
}

function compatibilityText(reason) {
  return `Preview picking unavailable (X_ITE compatibility check failed: ${reason || 'unknown'}). Select objects in the Scene Tree.`;
}

// The contract P-row (private API inventory) behind each structural reason,
// for the one console line logged when picking disables.
const COMPATIBILITY_ROW = Object.freeze({
  'parser-class-missing': 'P1', 'parser-hook-missing': 'P2', 'parser-comments-missing': 'P3',
  'parser-offsets-unproven': 'P4/G4', 'parser-context-missing': 'P5', 'touch-missing': 'P6',
  'hit-shape-changed': 'P7', 'runtime-parents-unavailable': 'P8/P9', 'world-infrastructure-missing': 'P10',
  'viewport-missing': 'P11', 'class-missing': 'P12', 'xite-version-mismatch': 'G2',
  'parser-hook-displaced': 'displaced-wrapper',
});

// Anything else is a missing public X_ITE surface (createX3DFromString, ...).
function compatibilityRow(reason) {
  return COMPATIBILITY_ROW[reason] || 'public-api';
}

// ---- source side: built lazily per analysis parse, cached by identity ----------
const sourceIndexCache = new WeakMap(); // parse result -> index

function sourceIndex(parse) {
  let idx = sourceIndexCache.get(parse);
  if (idx) return idx;
  const byExactSpan = new Map(); // "start:end" -> [AST Node]
  const typedParent = new Map();
  walk(parse.tree, (n, parent) => {
    typedParent.set(n, parent);
    if (n.type === NODE.NODE && n.range) {
      const k = `${n.range.start.offset}:${n.range.end.offset}`;
      if (!byExactSpan.has(k)) byExactSpan.set(k, []);
      byExactSpan.get(k).push(n);
    }
  });
  const enclosing = new Map(); // AST Node -> nearest enclosing Node | Document | Proto
  for (const n of typedParent.keys()) {
    if (n.type !== NODE.NODE) continue;
    let p = typedParent.get(n);
    while (p && p.type !== NODE.NODE && p.type !== NODE.DOCUMENT && p.type !== NODE.PROTO) p = typedParent.get(p);
    enclosing.set(n, p || null);
  }
  const diags = Array.isArray(parse.syntaxDiagnostics) ? parse.syntaxDiagnostics : [];
  idx = Object.freeze({ byExactSpan, enclosing, syntaxErrors: diags.filter((d) => d && d.severity === 'error').length });
  sourceIndexCache.set(parse, idx);
  return idx;
}

const spanOf = (n) => ({ start: n.range.start.offset, end: n.range.end.offset });

function result(status, reason, generation, extra) {
  return Object.freeze({
    status,
    reason,
    generation: generation ? generation.overlayGeneration : null,
    source: null,
    sceneTreeItemId: null,
    ...(extra || {}),
  });
}

/**
 * resolvePick({ snapshot, currentCheck, analysis, currentText }) -> result
 *
 *   snapshot     the adapter's plain-data pick snapshot (or an orchestrator
 *                pre-refusal of the same shape: { outcome, reason })
 *   currentCheck null when the snapshot's generation is still the displayed
 *                one, else the REFUSED_STALE reason (the engine's pickTarget().currentCheck)
 *   analysis     { text, parse, sceneTree } -- the editor's CURRENT analysis
 *   currentText  the editor buffer now
 *
 * Exactly one status, never null. Only PROVEN carries a sceneTreeItemId.
 */
function resolvePick({ snapshot, currentCheck = null, analysis = null, currentText } = {}) {
  if (!snapshot || typeof snapshot !== 'object') return result(STATUS.UNSUPPORTED, REASON.GENERATION_UNPROVABLE, null);
  const gen = snapshot.generation || null;
  switch (snapshot.outcome) {
    case 'disabled': return result(STATUS.COMPATIBILITY_DISABLED, snapshot.reason || 'compatibility-unproven', null);
    case 'stale': return result(STATUS.REFUSED_STALE, snapshot.reason || REASON.LAST_VALID_SCENE, gen);
    case 'external': return result(STATUS.REFUSED_EXTERNAL, snapshot.reason || REASON.OTHER_DOCUMENT, gen);
    default: break;
  }
  if (!gen) return result(STATUS.UNSUPPORTED, snapshot.reason || REASON.GENERATION_UNPROVABLE, null);

  // 1. Stale: the generation must still be displayed and its exact text must
  //    equal both the analysed text and the buffer. `===`, no normalization.
  if (currentCheck) return result(STATUS.REFUSED_STALE, currentCheck, gen);
  if (!analysis || typeof analysis.text !== 'string' || !analysis.parse || !analysis.sceneTree
    || gen.text !== analysis.text || gen.text !== currentText) {
    return result(STATUS.REFUSED_STALE, REASON.SOURCE_CHANGED, gen);
  }

  // 2. No Shape under the pointer (touch() false is mapped here, unread).
  if (snapshot.outcome === 'no-hit') return result(STATUS.NO_HIT, snapshot.reason || REASON.NO_GEOMETRY, gen);
  if (snapshot.outcome !== 'hit') return result(STATUS.UNSUPPORTED, snapshot.reason || REASON.GENERATION_UNPROVABLE, gen);
  if (!snapshot.shape || !snapshot.graph) return result(STATUS.NO_HIT, REASON.NO_GEOMETRY, gen);

  // 3. Authored interaction wins.
  if (snapshot.sensors && snapshot.sensors.length) {
    return result(STATUS.REFUSED_SENSOR_CONFLICT, REASON.SENSOR, gen, { sensors: snapshot.sensors.slice() });
  }
  // 4/5. Execution context of the hit Shape.
  if (snapshot.ctxKind === 'external-scene') return result(STATUS.REFUSED_EXTERNAL, REASON.OTHER_DOCUMENT, gen);
  if (snapshot.ctxKind === 'proto-body') return result(STATUS.UNSUPPORTED, REASON.PROTO_INSTANCE, gen);
  if (snapshot.ctxKind !== 'document') return result(STATUS.UNSUPPORTED, REASON.DETACHED, gen);

  // 6. A damaged document proves nothing.
  const idx = sourceIndex(analysis.parse);
  if (idx.syntaxErrors) return result(STATUS.UNSUPPORTED, REASON.SYNTAX_ERRORS, gen);

  // 7. Climb: every link uniquely provable.
  const graph = snapshot.graph;
  const chain = [];
  let label = snapshot.shape;
  let reachedScene = false;
  for (let guard = 0; guard < 256 && !reachedScene; guard++) {
    const g = Object.prototype.hasOwnProperty.call(graph, label) ? graph[label] : null;
    if (!g) return result(STATUS.UNSUPPORTED, REASON.CHAIN_UNBOUNDED, gen);
    if (g.ctxKind === 'proto-body') return result(STATUS.UNSUPPORTED, REASON.PROTO_INSTANCE, gen);
    if (g.ctxKind === 'external-scene') return result(STATUS.REFUSED_EXTERNAL, REASON.OTHER_DOCUMENT, gen);
    if (g.ctxKind !== 'document') return result(STATUS.UNSUPPORTED, REASON.PARENT_OUTSIDE_DOCUMENT, gen);
    if (!g.occurrences || g.occurrences.length === 0) return result(STATUS.UNSUPPORTED, REASON.NO_PROVENANCE, gen);
    if (g.occurrences.length > 1) return result(STATUS.REFUSED_AMBIGUOUS, REASON.SEVERAL_OCCURRENCES, gen);
    const docParents = [];
    let atScene = 0;
    for (const p of g.parents || []) {
      if (p === 'SCENE') { atScene += 1; continue; }
      if (p === 'OTHER_CONTEXT') return result(STATUS.UNSUPPORTED, REASON.PARENT_OUTSIDE_DOCUMENT, gen);
      const pg = Object.prototype.hasOwnProperty.call(graph, p) ? graph[p] : null;
      if (!pg) return result(STATUS.UNSUPPORTED, REASON.CHAIN_UNBOUNDED, gen);
      // X_ITE's layer0 holders, identified by OBJECT IDENTITY in the adapter.
      if (pg.ctxKind === 'world-infrastructure') continue;
      if (pg.ctxKind === 'proto-body') return result(STATUS.UNSUPPORTED, REASON.PROTO_INSTANCE, gen);
      if (pg.ctxKind !== 'document') return result(STATUS.UNSUPPORTED, REASON.PARENT_OUTSIDE_DOCUMENT, gen);
      docParents.push(p);
    }
    const parentCount = docParents.length + atScene;
    if (parentCount > 1) return result(STATUS.REFUSED_AMBIGUOUS, REASON.SEVERAL_PARENTS, gen);
    if (parentCount === 0) return result(STATUS.UNSUPPORTED, REASON.DETACHED, gen);
    chain.push({ occurrence: g.occurrences[0], type: g.type });
    if (atScene) reachedScene = true;
    else label = docParents[0];
  }
  if (!reachedScene) return result(STATUS.UNSUPPORTED, REASON.CHAIN_UNBOUNDED, gen);

  // 8. Exact join, same type.
  const ast = [];
  for (const link of chain) {
    const found = idx.byExactSpan.get(`${link.occurrence.start}:${link.occurrence.end}`) || [];
    if (found.length !== 1) return result(STATUS.UNSUPPORTED, joinFound(found.length), gen);
    if (found[0].nodeType !== link.type) return result(STATUS.UNSUPPORTED, REASON.JOIN_TYPE, gen);
    ast.push(found[0]);
  }
  // 9. Runtime chain == AST containment chain.
  for (let i = 0; i < ast.length; i++) {
    const expected = i + 1 < ast.length ? ast[i + 1] : analysis.parse.tree;
    if (idx.enclosing.get(ast[i]) !== expected) return result(STATUS.UNSUPPORTED, REASON.CHAIN_DISAGREES, gen);
  }

  // 10. Promotion: only to WD2-C's recognized simple object of THIS Shape.
  const shapeAst = ast[0];
  const parentAst = ast[1] || null;
  let logical = shapeAst;
  let role = 'shape';
  if (parentAst) {
    const obj = simpleObject.recognize(parentAst);
    if (obj && obj.shape === shapeAst) { logical = parentAst; role = 'simple-object'; }
  }
  // 11. The existing scene item, by AST object identity.
  const item = sceneTree.itemForAstNode(analysis.sceneTree, logical);
  if (!item) return result(STATUS.UNSUPPORTED, REASON.NOT_IN_TREE, gen);
  return Object.freeze({
    status: STATUS.PROVEN,
    reason: role === 'simple-object' ? REASON.PROMOTED : REASON.SHAPE,
    generation: gen.overlayGeneration,
    source: Object.freeze({
      shape: Object.freeze({ ...spanOf(shapeAst), nodeType: shapeAst.nodeType }),
      logical: Object.freeze({ ...spanOf(logical), nodeType: logical.nodeType, role }),
    }),
    sceneTreeItemId: item.id,
  });
}

module.exports = { STATUS, REASON, resolvePick, refusalText, compatibilityText, compatibilityRow };
