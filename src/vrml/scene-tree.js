'use strict';
// Scene-tree read model (Phase WD2-A).
//
// A pure, UI-neutral projection of a parse result onto the document's scene
// structure. The model is what a scene-tree view and an inspector consume --
// neither owns a private copy of the document, and neither parses source text.
//
// Built on the existing AST (`vrml.parse` output). It walks the tree exactly
// once, identifies the items a UI cares about, assigns each a stable session-
// scoped identifier derived from its source range, and walks no source text.
// It never mutates the AST, never calls `vrml.parse` itself, and never
// re-tokenizes anything.
//
// Items are intentional, not exhaustive:
//
//   INCLUDED  top-level Nodes (with or without DEF)
//             nested Nodes reached through SFNode / MFNode field values
//             USE references (always -- a USE is inspectable on its own)
//             PROTO declarations (their body is descended into)
//             EXTERNPROTO declarations (interface list shown via inspector)
//             ROUTE statements (at any nesting level)
//
//   EXCLUDED  scalar / array / string / boolean / null values
//             IS bindings (they live on a field declaration, not as their own
//                          scene item; the inspector shows them via the field)
//             InterfaceDecl entries (their PROTO/EXTERNPROTO parent carries
//                                   the inspectable shape)
//             ROUTE / PROTO / EXTERNPROTO inside an MFNode array -- the
//               parser-accepted Cybertown compatibility pattern, but treating
//               it as a scene item would fabricate hierarchy the author did
//               not write
//
// USE -> DEF resolution uses the parse result's own flat-scope `defsByName`
// (from `src/vrml/analyze.js`). It is documented as non-authoritative, and the
// scene tree carries that label honestly -- the inspector renders the
// flat-scope answer without upgrading it.
//
// IDs are session-stable and derived purely from `kind` and the source range:
// the same parse fed in twice produces the same item ids. They are NOT
// persisted (WD1.4 §7 forbids durable ids in the document); they are NOT
// authoritative node identity (the inspector navigates via `range`, which the
// AST already carries).

const { NODE, walk } = require('./ast');

// ITEM KIND -- the only discriminator a consumer needs to branch on. Picked
// over the AST `type` so a single value space covers what the scene cares
// about (one entry per inspectable thing, with PROTO instances flagged on
// their Node rather than split into a sixth kind).
const KIND = Object.freeze({
  DOCUMENT: 'Document',
  NODE: 'Node',
  USE: 'Use',
  PROTO: 'Proto',
  EXTERNPROTO: 'ExternProto',
  ROUTE: 'Route',
});

// USE-resolution status. The values match the names the inspector and the
// message catalog already use, so consumers do not need a translator.
const USE_TARGET = Object.freeze({
  RESOLVED: 'resolved',
  UNRESOLVED: 'unresolved',
});

// Build an item WITHOUT freezing it. The constructor populates a mutable
// childIds array (the parent pushes into it later), so freezing has to wait
// until every item has been linked -- finalizeItem() does that. Consumers
// receive the frozen result of buildSceneTree, not the raw drafts.
function makeItem(partial) {
  return {
    id: partial.id,
    kind: partial.kind,
    parentId: partial.parentId == null ? null : partial.parentId,
    childIds: [],
    depth: partial.depth,
    range: partial.range ? { start: { ...partial.range.start }, end: { ...partial.range.end } } : null,
    // Per-kind metadata. Consumers branch on `kind` first; the metadata
    // fields are intentionally narrow and never overlap (a Node has nodeType
    // but not protoName, etc.). The matrix test pins the per-kind key set.
    nodeType: partial.kind === KIND.NODE ? (partial.nodeType || null) : undefined,
    def: partial.kind === KIND.NODE ? (partial.def || null) : undefined,
    defRange: partial.kind === KIND.NODE && partial.defRange
      ? { start: { ...partial.defRange.start }, end: { ...partial.defRange.end } }
      : undefined,
    protoInstance: partial.kind === KIND.NODE ? !!partial.protoInstance : undefined,
    protoInstanceName: partial.kind === KIND.NODE && partial.protoInstance ? (partial.protoInstanceName || null) : undefined,
    fieldsCount: partial.kind === KIND.NODE ? (partial.fieldsCount | 0) : undefined,
    fieldNames: partial.kind === KIND.NODE ? (partial.fieldNames || []).slice() : undefined,
    useName: partial.kind === KIND.USE ? (partial.useName || null) : undefined,
    useNameRange: partial.kind === KIND.USE && partial.useNameRange
      ? { start: { ...partial.useNameRange.start }, end: { ...partial.useNameRange.end } }
      : undefined,
    useFieldOwner: partial.kind === KIND.USE ? (partial.useFieldOwner || null) : undefined,
    useFieldName: partial.kind === KIND.USE ? (partial.useFieldName || null) : undefined,
    useTargetStatus: partial.kind === KIND.USE ? (partial.useTargetStatus || USE_TARGET.UNRESOLVED) : undefined,
    useTargetItemId: partial.kind === KIND.USE ? (partial.useTargetItemId || null) : undefined,
    protoName: partial.kind === KIND.PROTO ? (partial.protoName || null) : undefined,
    protoNameRange: partial.kind === KIND.PROTO && partial.protoNameRange
      ? { start: { ...partial.protoNameRange.start }, end: { ...partial.protoNameRange.end } }
      : undefined,
    protoHasBody: partial.kind === KIND.PROTO ? !!partial.protoHasBody : undefined,
    protoInterfaceCount: partial.kind === KIND.PROTO ? (partial.protoInterfaceCount | 0) : undefined,
    externprotoName: partial.kind === KIND.EXTERNPROTO ? (partial.externprotoName || null) : undefined,
    externprotoNameRange: partial.kind === KIND.EXTERNPROTO && partial.externprotoNameRange
      ? { start: { ...partial.externprotoNameRange.start }, end: { ...partial.externprotoNameRange.end } }
      : undefined,
    externprotoInterfaceCount: partial.kind === KIND.EXTERNPROTO ? (partial.externprotoInterfaceCount | 0) : undefined,
    routeFromNode: partial.kind === KIND.ROUTE ? (partial.routeFromNode || null) : undefined,
    routeFromEvent: partial.kind === KIND.ROUTE ? (partial.routeFromEvent || null) : undefined,
    routeToNode: partial.kind === KIND.ROUTE ? (partial.routeToNode || null) : undefined,
    routeToEvent: partial.kind === KIND.ROUTE ? (partial.routeToEvent || null) : undefined,
    routeResolvedFrom: partial.kind === KIND.ROUTE ? !!partial.routeResolvedFrom : undefined,
    routeResolvedTo: partial.kind === KIND.ROUTE ? !!partial.routeResolvedTo : undefined,
    documentHasHeader: partial.kind === KIND.DOCUMENT ? !!partial.documentHasHeader : undefined,
    documentStatementCount: partial.kind === KIND.DOCUMENT ? (partial.documentStatementCount | 0) : undefined,
  };
}

// Freeze every draft item and its childIds array. Called exactly once at
// the end of buildSceneTree, after the parent links have all been pushed.
function finalizeItem(item) {
  item.fieldNames = Object.freeze(item.fieldNames);
  item.childIds = Object.freeze(item.childIds.slice());
  return Object.freeze(item);
}

// Stable id derived from a kind + source range. Offsets are bytes into the
// source text; for one parse they are unique per item because two items at
// the same starting offset must be the same item.
function idFor(kind, range) {
  if (!range || !range.start) return `${kind.toLowerCase()}-0-0`;
  const s = range.start.offset | 0;
  const e = (range.end && range.end.offset != null) ? (range.end.offset | 0) : s;
  return `${kind.toLowerCase()}-${s}-${e}`;
}

// Range copy -- share offsets, leave the source immutable.
function rangeCopy(r) {
  if (!r) return null;
  return {
    start: { offset: r.start.offset, line: r.start.line | 0, column: r.start.column | 0 },
    end: { offset: r.end.offset, line: r.end.line | 0, column: r.end.column | 0 },
  };
}

// Wrap a Map so a consumer that tries to mutate it gets a TypeError.
// `get`, `has`, iteration and `size` all keep working; `set`, `delete` and
// `clear` throw. The whole point of the read model is to be immutable, but
// Object.freeze on the outer object does not reach into Map slots -- this
// Proxy does, without bringing in a new dependency.
//
// `size` is a getter on `Map.prototype` whose body reads the receiver's
// internal `[[MapData]]` slot. A naive `Reflect.get(target, 'size', receiver)`
// therefore returns undefined (the receiver is the Proxy, not the Map). The
// trap returns `target.size` directly for that one key -- the Map's own
// accessor, called with the Map as `this`, is the only correct call.
function readOnlyMap(map, label) {
  if (!map) return map;
  const name = label || 'SCENE_TREE_READ_ONLY';
  return new Proxy(map, {
    get(target, prop, receiver) {
      if (prop === 'set' || prop === 'delete' || prop === 'clear') {
        return () => { throw new TypeError(`${name}: scene-tree maps are read-only`); };
      }
      if (prop === 'size') return target.size;
      const value = Reflect.get(target, prop, receiver);
      return typeof value === 'function' ? value.bind(target) : value;
    },
  });
}

// WD2-B: the private link between a built tree's items and the AST objects
// they project. Keyed by the frozen tree object, so it is released with the
// tree and can never answer for a different parse. It is NOT a lookup by
// range: an item id is mapped to the exact AST object it was emitted from
// during the single build walk, and back. The editor needs both directions
// to hand the WD1.4 identity layer a real node from the selected item and to
// find the item that projects a node identity re-anchored after an edit.
// Nothing here is exposed as a mutable Map.
const AST_LINKS = new WeakMap();

function link(ctx, id, astNode) {
  ctx.astByItemId.set(id, astNode);
  ctx.itemIdByAst.set(astNode, id);
}

// Map every PROTO/EXTERNPROTO declaration name (in document order) so a Node
// whose `nodeType` matches a PROTO name can be marked as a PROTO instance.
function collectProtoNames(tree) {
  const out = new Map(); // name -> first occurrence index (document order)
  walk(tree, (n) => {
    if (n.type === NODE.PROTO && n.name) {
      if (!out.has(n.name)) out.set(n.name, { kind: KIND.PROTO });
    } else if (n.type === NODE.EXTERNPROTO && n.name) {
      if (!out.has(n.name)) out.set(n.name, { kind: KIND.EXTERNPROTO });
    }
  });
  return out;
}

// Walk one AST node (a Node, an Array, or a scalar) and append child scene
// items to `into` when they carry a node / use / proto / route. Returns the
// parent item id so a USE knows which field / Node owns it.
function collectValueChildren(parentItemId, parentFieldName, value, depth, ctx) {
  if (!value || typeof value !== 'object') return;
  if (value.type === NODE.NODE) {
    emitNode(value, parentItemId, parentFieldName, depth, ctx);
    return;
  }
  if (value.type === NODE.USE) {
    emitUse(value, parentItemId, parentFieldName, depth, ctx);
    return;
  }
  if (value.type === NODE.ARRAY) {
    for (const item of value.items || []) {
      collectValueChildren(parentItemId, parentFieldName, item, depth, ctx);
    }
  }
}

// Emit one Node as a scene item, then descend into its SFNode / MFNode fields
// to emit nested nodes and USEs. Returns the item id so a caller can wire
// parent links.
function emitNode(node, parentId, fieldName, depth, ctx) {
  const id = idFor(KIND.NODE, node.range);
  const fieldNames = [];
  for (const f of node.fields || []) {
    if (f && f.name) fieldNames.push(f.name);
  }
  const isProto = node.nodeType != null && ctx.protoNames.has(node.nodeType);
  const item = makeItem({
    id,
    kind: KIND.NODE,
    parentId,
    childIds: [],
    depth,
    range: rangeCopy(node.range),
    nodeType: node.nodeType || null,
    def: node.def || null,
    defRange: node.def ? rangeCopy(node.defRange) : null,
    protoInstance: isProto,
    protoInstanceName: isProto ? node.nodeType : null,
    fieldsCount: (node.fields || []).length,
    fieldNames,
  });
  ctx.items.push(item);
  ctx.byId.set(id, item);
  link(ctx, id, node);
  if (parentId != null) ctx.byId.get(parentId).childIds.push(id);
  for (const f of node.fields || []) {
    if (!f || !f.name) continue;
    // IS bindings have no value to descend into.
    if (f.isBinding) continue;
    collectValueChildren(id, f.name, f.value, depth + 1, ctx);
  }
  return id;
}

function emitUse(use, parentId, fieldName, depth, ctx) {
  const id = idFor(KIND.USE, use.range);
  // The renderer is the single USE-resolution authority: it owns the
  // scope-graph handle, knows the 4.6.2 binding rules, and decides whether a
  // USE is resolved. The read model itself NEVER answers from the flat
  // defsByName alone -- that lookup is cross-scope-blind and would resolve an
  // outer `USE` to a DEF declared inside a PROTO body.
  //
  // Verdict shape: { status: 'resolved'|'unresolved', targetAstNode?: Node }
  // When 'resolved', targetAstNode is the bound DEF's AST node from the same
  // parse the tree was built from; the id is derived here so the read model
  // stays the only place that knows its id format.
  let useTargetStatus = USE_TARGET.UNRESOLVED;
  let useTargetItemId = null;
  if (ctx.useResolver) {
    const verdict = ctx.useResolver(use);
    if (verdict && verdict.status === USE_TARGET.RESOLVED && verdict.targetAstNode && verdict.targetAstNode.range) {
      useTargetStatus = USE_TARGET.RESOLVED;
      useTargetItemId = idFor(KIND.NODE, verdict.targetAstNode.range);
    } else if (verdict && verdict.status === USE_TARGET.UNRESOLVED) {
      useTargetStatus = USE_TARGET.UNRESOLVED;
      useTargetItemId = null;
    }
  }
  const item = makeItem({
    id,
    kind: KIND.USE,
    parentId,
    childIds: [],
    depth,
    range: rangeCopy(use.range),
    useName: use.name || null,
    useNameRange: use.name ? rangeCopy(use.nameRange) : null,
    useFieldOwner: parentId,
    useFieldName: fieldName || null,
    useTargetStatus,
    useTargetItemId,
  });
  ctx.items.push(item);
  ctx.byId.set(id, item);
  link(ctx, id, use);
  if (parentId != null) ctx.byId.get(parentId).childIds.push(id);
  return id;
}

function emitProto(proto, parentId, depth, ctx) {
  const id = idFor(KIND.PROTO, proto.range);
  const item = makeItem({
    id,
    kind: KIND.PROTO,
    parentId,
    childIds: [],
    depth,
    range: rangeCopy(proto.range),
    protoName: proto.name || null,
    protoNameRange: proto.name ? rangeCopy(proto.nameRange) : null,
    protoHasBody: Array.isArray(proto.body),
    protoInterfaceCount: (proto.interfaces || []).length,
  });
  ctx.items.push(item);
  ctx.byId.set(id, item);
  link(ctx, id, proto);
  if (parentId != null) ctx.byId.get(parentId).childIds.push(id);
  // PROTO body is descended: top-level statements there are their own
  // inspectable scene items.
  for (const stmt of proto.body || []) {
    if (!stmt) continue;
    if (stmt.type === NODE.NODE) {
      emitNode(stmt, id, null, depth + 1, ctx);
    } else if (stmt.type === NODE.ROUTE) {
      emitRoute(stmt, id, depth + 1, ctx);
    }
    // A PROTO body may not contain PROTO/EXTERNPROTO per ISO 4.8, and any
    // such attempt is rejected by the parser; nothing to do here.
  }
  return id;
}

function emitExternProto(ext, parentId, depth, ctx) {
  const id = idFor(KIND.EXTERNPROTO, ext.range);
  const item = makeItem({
    id,
    kind: KIND.EXTERNPROTO,
    parentId,
    childIds: [],
    depth,
    range: rangeCopy(ext.range),
    externprotoName: ext.name || null,
    externprotoNameRange: ext.name ? rangeCopy(ext.nameRange) : null,
    externprotoInterfaceCount: (ext.interfaces || []).length,
  });
  ctx.items.push(item);
  ctx.byId.set(id, item);
  link(ctx, id, ext);
  if (parentId != null) ctx.byId.get(parentId).childIds.push(id);
  return id;
}

function emitRoute(route, parentId, depth, ctx) {
  const id = idFor(KIND.ROUTE, route.range);
  const fromNode = route.from ? route.from.node : null;
  const toNode = route.to ? route.to.node : null;
  const fromEvent = route.from ? route.from.event : null;
  const toEvent = route.to ? route.to.event : null;
  const resolvedFrom = fromNode != null && ctx.defsByName.has(fromNode);
  const resolvedTo = toNode != null && ctx.defsByName.has(toNode);
  const item = makeItem({
    id,
    kind: KIND.ROUTE,
    parentId,
    childIds: [],
    depth,
    range: rangeCopy(route.range),
    routeFromNode: fromNode,
    routeFromEvent: fromEvent,
    routeToNode: toNode,
    routeToEvent: toEvent,
    routeResolvedFrom: resolvedFrom,
    routeResolvedTo: resolvedTo,
  });
  ctx.items.push(item);
  ctx.byId.set(id, item);
  link(ctx, id, route);
  if (parentId != null) ctx.byId.get(parentId).childIds.push(id);
  return id;
}

// buildSceneTree(parseResult, opts?) -> { root, items, byId, totals, defsByName }
//   parseResult : the object returned by `vrml.parse(text, opts)`
//   opts.useResolver : optional (useAstNode) => { status, targetItemId? }.
//     When present, USE items get their `useTargetStatus`/`useTargetItemId`
//     from the resolver (typically one that consults a WD1.5 scope graph).
//     When absent, every USE is UNRESOLVED -- the flat-scope answer is NEVER
//     trusted on its own (it would accept cross-PROTO false positives).
//   root        : the Document item (the top of the scene)
//   items       : every item in document order (root first, depth-first)
//   byId        : id -> item (a Map WRAPPED for read-only access)
//   totals      : { count, maxDepth, byKind }
//   defsByName  : the input's defsByName (also wrapped read-only)
//
// The returned structure is FROZEN and its lookup Maps are read-only proxies:
// a consumer that tries to mutate them gets a TypeError. A consumer who needs
// to mutate derives its own state; the read model is a projection.
function buildSceneTree(parseResult, opts) {
  if (!parseResult || typeof parseResult !== 'object' || !parseResult.tree) {
    throw new Error('SCENE_TREE_INVALID_INPUT: expected a parse result with a tree');
  }
  const tree = parseResult.tree;
  const defsByName = parseResult.defsByName instanceof Map
    ? parseResult.defsByName
    : new Map();

  const ctx = {
    items: [],
    byId: new Map(),
    defsByName,
    protoNames: collectProtoNames(tree),
    useResolver: opts && typeof opts.useResolver === 'function' ? opts.useResolver : null,
    astByItemId: new Map(),
    itemIdByAst: new Map(),
  };

  // Root item -- always present, even for an empty document.
  const rootId = idFor(KIND.DOCUMENT, tree && tree.range);
  const rootItem = makeItem({
    id: rootId,
    kind: KIND.DOCUMENT,
    parentId: null,
    childIds: [],
    depth: 0,
    range: rangeCopy(tree && tree.range),
    documentHasHeader: !!(tree && tree.header),
    documentStatementCount: tree && tree.statements ? tree.statements.length : 0,
  });
  ctx.items.push(rootItem);
  ctx.byId.set(rootId, rootItem);
  if (tree) link(ctx, rootId, tree);

  // Top-level statements.
  if (tree && Array.isArray(tree.statements)) {
    for (const stmt of tree.statements) {
      if (!stmt) continue;
      if (stmt.type === NODE.NODE) emitNode(stmt, rootId, null, 1, ctx);
      else if (stmt.type === NODE.PROTO) emitProto(stmt, rootId, 1, ctx);
      else if (stmt.type === NODE.EXTERNPROTO) emitExternProto(stmt, rootId, 1, ctx);
      else if (stmt.type === NODE.ROUTE) emitRoute(stmt, rootId, 1, ctx);
    }
  }

  // Freeze every draft item and its childIds. Must happen AFTER all parent
  // links are pushed, since pushing to a frozen childIds throws.
  for (const it of ctx.items) finalizeItem(it);
  const items = Object.freeze(ctx.items.slice());
  const byKind = {};
  let maxDepth = 0;
  for (const it of items) {
    byKind[it.kind] = (byKind[it.kind] | 0) + 1;
    if (it.depth > maxDepth) maxDepth = it.depth;
  }

  // Document-order invariant: every child index in every parent's childIds
  // appears at the same document order in `items` as the child walked the
  // tree. This is asserted by `Q1` (test/vrml/scene-tree.test.js).
  const totals = Object.freeze({
    count: items.length,
    maxDepth,
    byKind: Object.freeze(byKind),
  });

  const result = Object.freeze({
    root: ctx.byId.get(rootId),
    items,
    byId: readOnlyMap(ctx.byId, 'SCENE_TREE_BYID_READ_ONLY'),
    totals,
    defsByName: readOnlyMap(defsByName, 'SCENE_TREE_DEFS_READ_ONLY'),
  });
  AST_LINKS.set(result, { astByItemId: ctx.astByItemId, itemIdByAst: ctx.itemIdByAst });
  return result;
}

// Look up the smallest scene item whose range contains the given offset.
// `inclusive` puts an exact-start match inside the item; an offset past the
// last range returns null. Used by the inspector to attach diagnostics to
// the most-specific scene item they live inside.
function itemContainingOffset(sceneTreeResult, offset) {
  if (!sceneTreeResult || typeof offset !== 'number') return null;
  let best = null;
  let bestSize = Infinity;
  for (const item of sceneTreeResult.items) {
    if (!item.range) continue;
    const start = item.range.start.offset;
    const end = item.range.end.offset;
    if (offset < start || offset > end) continue;
    const size = end - start;
    if (size < bestSize) { bestSize = size; best = item; }
  }
  return best;
}

// Look up the scene item whose id matches. Returns null when the id is
// unknown or the scene tree is empty. The scene tree is a parse-time
// projection; after a reparse an item id may refer to nothing, and the
// caller is responsible for deciding what to do (the inspector renders an
// empty state, the tree view clears the highlight).
function itemById(sceneTreeResult, id) {
  if (!sceneTreeResult || id == null) return null;
  const item = sceneTreeResult.byId.get(id);
  return item || null;
}

// WD2-B: the exact AST object an item was emitted from, or null. Only a tree
// returned by buildSceneTree answers; the object belongs to that tree's parse.
function astNodeForItem(sceneTreeResult, id) {
  const links = sceneTreeResult && typeof sceneTreeResult === 'object' ? AST_LINKS.get(sceneTreeResult) : null;
  if (!links || id == null) return null;
  return links.astByItemId.get(id) || null;
}

// WD2-B: the item that projects exactly this AST object, or null. Object
// identity only -- an AST object from another parse never matches.
function itemForAstNode(sceneTreeResult, astNode) {
  const links = sceneTreeResult && typeof sceneTreeResult === 'object' ? AST_LINKS.get(sceneTreeResult) : null;
  if (!links || !astNode || typeof astNode !== 'object') return null;
  const id = links.itemIdByAst.get(astNode);
  return id == null ? null : itemById(sceneTreeResult, id);
}

module.exports = {
  KIND,
  USE_TARGET,
  buildSceneTree,
  itemContainingOffset,
  itemById,
  astNodeForItem,
  itemForAstNode,
};