'use strict';
// WD2-C0 CANDIDATE MAPPING: X_ITE hit -> exact source-backed scene item.
//
// SPIKE ONLY. Pure: no fs, no Electron, no DOM. It consumes
//   * the exact text one preview generation was built from,
//   * that generation's id,
//   * a hit record captured by browser/harness.js in that generation:
//       hit.shape            runtime label of hit.shapeNode
//       hit.sensors          pointing-device sensors under the pointer
//       hit.ctxKind          execution-context class of the hit Shape
//       hit.graph[label]     { type, ctxKind, occurrences[{start,end,kind}],
//                              parents[label|'SCENE'|'OTHER_CONTEXT'] }
// and returns exactly one of:
//   PROVEN | REFUSED_AMBIGUOUS | REFUSED_EXTERNAL | REFUSED_SENSOR_CONFLICT |
//   UNSUPPORTED | REFUSED_STALE | NO_HIT
//
// WHERE THE PROOF COMES FROM
//   1. Provenance: X_ITE's own VRML parser reported, while parsing this exact
//      string, that runtime object R was produced by the nodeStatement at
//      [start,end). Not inferred from order, position or content.
//   2. Exact join: [start,end) must equal ONE WRL Forge AST Node range, both
//      ends, byte for byte. There is no nearest/containing/overlapping lookup.
//   3. Instance uniqueness: R and every document ancestor of R have exactly
//      ONE source occurrence (no DEF+USE) and exactly ONE live runtime document
//      parent. Then R is rendered exactly once, so "the hit Shape" and "the
//      clicked occurrence" are the same thing. Any USE on the chain breaks
//      this and the pick is refused: X_ITE's hit carries no instance path.
//   4. Agreement: the runtime parent chain must equal the AST containment
//      chain, element by element (each verified by its own exact span). A
//      Script/ROUTE that re-parented a node at runtime fails this and refuses.
//
// NO FALLBACK. When any step fails the answer is a refusal. There is no retry
// by DEF name, node type, geometry type, sibling index, nearest offset, text
// similarity or matrix comparison anywhere in this file (spike.test.js scans
// for it).

const vrml = require('../../src/vrml');
const { NODE } = vrml.ast;

const STATUS = Object.freeze({
  PROVEN: 'PROVEN',
  REFUSED_AMBIGUOUS: 'REFUSED_AMBIGUOUS',
  REFUSED_EXTERNAL: 'REFUSED_EXTERNAL',
  REFUSED_SENSOR_CONFLICT: 'REFUSED_SENSOR_CONFLICT',
  UNSUPPORTED: 'UNSUPPORTED',
  REFUSED_STALE: 'REFUSED_STALE',
  NO_HIT: 'NO_HIT',
});

const span = (n) => ({ start: n.range.start.offset, end: n.range.end.offset });
const key = (s, e) => `${s}:${e}`;

// One source context per preview generation. Built once per generation; the
// caller discards it together with the generation.
function createSourceContext(text, generation) {
  const parse = vrml.parse(text);
  const syntaxErrors = parse.syntaxDiagnostics.filter((d) => d.severity === 'error').length;
  const byExactSpan = new Map(); // "start:end" -> [AST Node]
  const enclosing = new Map(); // AST Node -> nearest enclosing Node | Document | Proto
  const typedParent = new Map();
  vrml.ast.walk(parse.tree, (n, parent) => {
    typedParent.set(n, parent);
    if (n.type === NODE.NODE && n.range) {
      const k = key(n.range.start.offset, n.range.end.offset);
      if (!byExactSpan.has(k)) byExactSpan.set(k, []);
      byExactSpan.get(k).push(n);
    }
  });
  for (const n of typedParent.keys()) {
    if (n.type !== NODE.NODE) continue;
    let p = typedParent.get(n);
    while (p && p.type !== NODE.NODE && p.type !== NODE.DOCUMENT && p.type !== NODE.PROTO) p = typedParent.get(p);
    enclosing.set(n, p || null);
  }
  const sceneTree = vrml.sceneTree.buildSceneTree(parse);
  return Object.freeze({ text, generation, parse, syntaxErrors, byExactSpan, enclosing, sceneTree });
}

const refuse = (status, reason, extra) => Object.freeze({ status, reason, ...(extra || {}) });

function resolvePick(ctx, hit) {
  const base = { generation: ctx.generation };
  if (!hit || hit.generation !== ctx.generation) {
    return refuse(STATUS.REFUSED_STALE, 'hit-from-another-preview-generation', base);
  }
  if (!hit.shape) return refuse(STATUS.NO_HIT, 'no-geometry-under-pointer', base);
  if (hit.sensors && hit.sensors.length) {
    return refuse(STATUS.REFUSED_SENSOR_CONFLICT, 'pointing-device-sensor-under-pointer',
      { ...base, sensors: hit.sensors.map((s) => s.type) });
  }
  if (hit.ctxKind === 'external-scene') return refuse(STATUS.REFUSED_EXTERNAL, 'hit-belongs-to-another-document', base);
  if (hit.ctxKind !== 'document') return refuse(STATUS.UNSUPPORTED, `hit-execution-context-${hit.ctxKind}`, base);
  if (ctx.syntaxErrors) return refuse(STATUS.UNSUPPORTED, 'document-has-syntax-errors', base);

  // ---- runtime chain, hit Shape upward, each link uniquely provable --------
  const chain = [];
  let label = hit.shape;
  for (let guard = 0; guard < 256; guard++) {
    const g = hit.graph && hit.graph[label];
    if (!g) return refuse(STATUS.UNSUPPORTED, 'runtime-node-missing-from-hit-graph', base);
    if (g.occurrences.length === 0) return refuse(STATUS.UNSUPPORTED, 'runtime-node-has-no-parse-provenance', base);
    if (g.occurrences.length > 1) {
      return refuse(STATUS.REFUSED_AMBIGUOUS, 'runtime-node-referenced-by-several-source-occurrences',
        { ...base, at: g.type, occurrences: g.occurrences.length });
    }
    const docParents = [];
    let atScene = false;
    for (const p of g.parents) {
      if (p === 'SCENE') { atScene = true; continue; }
      const pg = hit.graph[p];
      if (!pg || p === 'OTHER_CONTEXT') return refuse(STATUS.UNSUPPORTED, 'runtime-parent-outside-document', base);
      // X_ITE's layer0 Layer/Group holding the root nodes, identified in the
      // harness by object identity from world.getLayer0() -- never by "has no
      // provenance". Any other unprovenanced parent stays a parent and so
      // makes the chain ambiguous.
      if (pg.ctxKind === 'world-infrastructure') continue;
      if (pg.ctxKind !== 'document') return refuse(STATUS.UNSUPPORTED, `runtime-parent-in-${pg.ctxKind}`, base);
      docParents.push(p);
    }
    chain.push({ label, occurrence: g.occurrences[0], type: g.type });
    const parentCount = docParents.length + (atScene ? 1 : 0);
    if (parentCount > 1) {
      return refuse(STATUS.REFUSED_AMBIGUOUS, 'runtime-node-has-several-live-parents', { ...base, at: g.type, parents: parentCount });
    }
    if (parentCount === 0) return refuse(STATUS.UNSUPPORTED, 'runtime-node-detached', base);
    if (atScene) break;
    label = docParents[0];
  }
  if (!chain.length || chain.length >= 256) return refuse(STATUS.UNSUPPORTED, 'runtime-chain-unbounded', base);

  // ---- exact join + containment agreement ---------------------------------
  const ast = [];
  for (const link of chain) {
    const hits = ctx.byExactSpan.get(key(link.occurrence.start, link.occurrence.end)) || [];
    if (hits.length !== 1) return refuse(STATUS.UNSUPPORTED, `exact-span-join-found-${hits.length}`, base);
    if (hits[0].nodeType !== link.type) return refuse(STATUS.UNSUPPORTED, 'exact-span-join-type-disagrees', base);
    ast.push(hits[0]);
  }
  for (let i = 0; i < ast.length; i++) {
    const enc = ctx.enclosing.get(ast[i]);
    const expected = i + 1 < ast.length ? ast[i + 1] : ctx.parse.tree;
    if (enc !== expected) return refuse(STATUS.UNSUPPORTED, 'runtime-chain-disagrees-with-source-containment', base);
  }

  // ---- logical object: WD2-C simple-object promotion, proven from source --
  const shapeAst = ast[0];
  const parentAst = ast[1] || null;
  let logical = shapeAst;
  let role = 'shape';
  if (parentAst) {
    const obj = vrml.simpleObject.recognize(parentAst);
    if (obj && obj.shape === shapeAst) { logical = parentAst; role = 'simple-object'; }
  }
  const item = vrml.sceneTree.itemForAstNode(ctx.sceneTree, logical);
  if (!item) return refuse(STATUS.UNSUPPORTED, 'logical-node-not-in-scene-tree', base);
  return Object.freeze({
    status: STATUS.PROVEN,
    reason: role === 'simple-object' ? 'shape-promoted-to-proven-simple-object' : 'shape',
    generation: ctx.generation,
    runtimeShape: hit.shape,
    source: Object.freeze({
      shape: { ...span(shapeAst), nodeType: shapeAst.nodeType },
      logical: { ...span(logical), nodeType: logical.nodeType, role },
      chainDepth: ast.length,
    }),
    sceneTreeItemId: item.id,
  });
}

module.exports = { STATUS, createSourceContext, resolvePick };
