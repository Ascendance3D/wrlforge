'use strict';
// VRML97 parser facade (Phase 7A). One import surface for consumers:
//
//   const { parse } = require('./src/vrml');
//   const result = parse(text, { profile: 'world' });
//   // result: { tree, tokens, comments, diagnostics, defs, routes, uses,
//   //           assetRefs, truncated, depthCapped, limits, profile }
//
// Pure: text in, tree + diagnostics + index out. Gzip is handled OUTSIDE the
// parser (the main process decompresses to plain UTF-8 first; see
// src/preview/wrl-source.js). `profile` is carried through for later Mall vs.
// World rule layering but does NOT change syntax parsing in 7A -- the tree is
// profile-neutral by design.

const { tokenize, TT, KEYWORDS } = require('./tokenizer');
const { parse: parseSyntax, DEFAULT_LIMITS } = require('./parser');
const { analyze } = require('./analyze');
const assetRefs = require('./asset-refs');
const ast = require('./ast');
const diagnostics = require('./diagnostics');
// Read-only offset -> token/node lookup over a parse result. Opt-in and lazy:
// `parse()` does NOT build one, so nothing existing pays for it.
const { createSourceMap } = require('./source-map');
// Span-patch algebra (WD1.2): pure text-in/text-out edits anchored to the exact
// spans the parser and source map report. No callers in production yet.
const edit = require('./edit');
// Generated VRML97/X3D node + field schema (WD1.3). Data only -- committed, so
// no generator or ISO mirror is needed at runtime. See scripts/build-node-schema.js.
const nodeSchema = require('./node-schema');
// Parse sessions + verified document transactions, and the two-tier stable node
// identity built on them (WD1.4). Pure and browser-safe; no callers in
// production yet -- this is the foundation a future scene tree / inspector
// selection model resolves through, not a UI integration.
//
// NARROWED ON PURPOSE. Both modules export more than appears here: internal
// predicates, the receipt's canonical edit view, and the session guards exist for
// node-identity.js's own CommonJS composition and for focused tests. They are not
// consumer-facing, and a facade is the wrong place to publish "you could also
// reach in here". What follows is the whole intended public surface -- consumer
// operations plus the constant tables a caller needs to branch on a `status` or a
// `reason`. Adding to it is a decision, not a convenience.
const documentTransaction = require('./document-transaction');
const nodeIdentity = require('./node-identity');
// The WD1.5 scope graph and the WD1.6-B consumer query over it. `scope-graph.js`
// stays UNPUBLISHED as a module -- its ~60 exports are the internal semantic
// surface, and publishing them would make consumers depend on the substrate
// instead of the consumer layer. `symbols.js` stays unpublished entirely.
const scopeGraph = require('./scope-graph');
const interfaceQuery = require('./interface-query');
// WD1.6-C, the containment query over the same graph. Like `interface-query.js`
// it is a CONSUMER layer, not a new authority: every fact it judges came from
// P1/P2A, WD1.6-B or the WD1.6-A schema.
const containment = require('./containment');
// WD1.6-D, the structured semantic findings model over the same graph. Like the
// two above it is a CONSUMER layer: every fact it reports was decided by
// P1/P2A/P2B/P2C, WD1.6-B or WD1.6-C, and it re-expresses them in one record
// shape without deciding anything itself.
const semanticFindings = require('./semantic-findings');
// WD1.7-C's PURE half: ISO 4.9.3 prototype target selection over one parsed
// target document, plus the reachable prototype-dependency enumeration a
// traversal needs. It lives HERE, beside the other semantic authorities, because
// 4.9.3 is a statement about a document and not about a filesystem -- it takes a
// parse result and a written fragment, and it has never heard of a URL, a base
// document, an archive root or a retrieval. The Node-side orchestration that
// walks a candidate list and recurses across documents is `src/proto-resolution/`
// and is deliberately NOT reachable from this facade.
const protoTarget = require('./proto-target');
// WD1.7-D's PURE half: ISO 4.9.2 interface agreement between a local EXTERNPROTO
// declaration and the PROTO declaration WD1.7-C proved to implement it. It lives
// HERE for the same reason `proto-target.js` does -- 4.9.2 is a statement about
// two DOCUMENTS, not about a filesystem: it takes two scope graphs and two
// declarations, and it has never heard of a URL, a base document, an archive or
// a retrieval. The Node-side orchestration that consumes WD1.7-C evidence and
// composes this with the 4.8.3 class authority is `src/proto-enrichment/` and is
// deliberately NOT reachable from this facade.
const protoAgreement = require('./proto-agreement');

// P4-A -- the presentation policy over everything above.
//
// HERE, and browser-safe, because it decides nothing about a document: it reads
// verdicts the authorities above already froze and answers only "how should a
// consumer show this". It is the one place severity, attention rank, ordering,
// default visibility, filter tags and save policy are decided, so WD2 can
// render a findings list without re-deriving any of them -- and without a
// second, divergent opinion growing inside the editor, the inspector and the
// scene tree.
const presentation = require('./presentation');
// P4-B -- the user-facing diagnostic text over P4-A.
//
// P4-A decides severity, ordering and visibility. P4-B decides what words a
// consumer renders. It reads ONLY the structured facts a presentation already
// exposes, never the semantic substrate, and the message it returns is text
// only -- no severity, group, saveBlocking, ranking or compatibility decision
// is reachable from here, so a presentation cannot drift into a second policy.
const messages = require('./messages');
// WD2-A -- the read-only scene-tree projection a scene-tree view and an
// inspector both consume. Pure, browser-safe, identity-stable across one
// session (the id is derived from the source range, so a re-render of the
// same parse produces the same ids; after a reparse the caller decides how
// to map selections forward). NO presentation, NO message text, NO semantic
// verdict -- the scene tree is a structural projection over the AST only.
const sceneTree = require('./scene-tree');
// WD2-B -- typed field-value editing over the same lossless core: an explicitly
// authored built-in field + a typed value -> a VERIFIED WD1.2 edit set. Pure and
// browser-safe; it serializes nothing and dispatches nothing.
const fieldEdit = require('./field-edit');
// WD2-C -- First Object: new-node templates, exact-span structural edits
// (insert / duplicate / delete / absent-field insert) and the beginner
// simple-object facade. Pure; they serialize nothing and dispatch nothing.
const nodeTemplates = require('./node-templates');
const structureEdit = require('./structure-edit');
const simpleObject = require('./simple-object');

const publicDocumentTransaction = Object.freeze({
  // Prove that an edit set is exactly what turned one exact text into another.
  verifyTransaction: documentTransaction.verifyTransaction,
  // A consumer holding a receipt across an async boundary can ask whether it is
  // still a receipt at all before offering to re-anchor through it.
  isVerifiedReceipt: documentTransaction.isVerifiedReceipt,
  TX_ERROR: documentTransaction.TX_ERROR,
  TX_STATUS: documentTransaction.TX_STATUS,
  TX_REASON: documentTransaction.TX_REASON,
});

const publicNodeIdentity = Object.freeze({
  // Sessions are created here rather than on the transaction facade because every
  // identity call needs one and nothing else does.
  createParseSession: documentTransaction.createParseSession,
  createCurrentSelection: nodeIdentity.createCurrentSelection,
  resolveCurrentSelection: nodeIdentity.resolveCurrentSelection,
  createTransactionAnchor: nodeIdentity.createTransactionAnchor,
  resolveTransactionAnchor: nodeIdentity.resolveTransactionAnchor,
  createPersistentAnchor: nodeIdentity.createPersistentAnchor,
  resolvePersistentAnchor: nodeIdentity.resolvePersistentAnchor,
  isResolved: nodeIdentity.isResolved,
  isAmbiguous: nodeIdentity.isAmbiguous,
  isRefused: nodeIdentity.isRefused,
  IDENTITY_ERROR: nodeIdentity.IDENTITY_ERROR,
  IDENTITY_STATUS: nodeIdentity.IDENTITY_STATUS,
  ANCHOR_STATUS: nodeIdentity.ANCHOR_STATUS,
  IDENTITY_REASON: nodeIdentity.IDENTITY_REASON,
  ANCHOR_KIND: nodeIdentity.ANCHOR_KIND,
});

/**
 * WD1.6-B -- what interface can be proven for one node occurrence?
 *
 * `buildScopeGraph` is published HERE and only here. A facade-public query that
 * takes a graph must offer a facade-supported way to build one; without it a
 * consumer would be forced to bypass the facade and require `scope-graph.js`
 * directly, which is exactly what this narrowing exists to prevent. The graph
 * itself stays OPAQUE -- it is a frozen handle with no reachable state, and none
 * of the queries over it are published.
 *
 * The constants are the tables a consumer must branch on to read a projection.
 * There is deliberately no `isUnsupported` predicate: `symbols.js` never defined
 * one, and inventing it here would put a semantic predicate in a facade. Compare
 * against `STATUS.UNSUPPORTED`.
 */
const publicInterfaceQuery = Object.freeze({
  buildScopeGraph: scopeGraph.buildScopeGraph,
  effectiveInterfaceOf: interfaceQuery.effectiveInterfaceOf,
  BINDING_FORM: scopeGraph.BINDING_FORM,
  ACCESS: scopeGraph.ACCESS,
  ENDPOINT_ORIGIN: scopeGraph.ENDPOINT_ORIGIN,
  STATUS: scopeGraph.STATUS,
  REASON: scopeGraph.REASON,
  SCOPE_ERROR: scopeGraph.SCOPE_ERROR,
  isResolved: scopeGraph.isResolved,
  isUnresolved: scopeGraph.isUnresolved,
  isAmbiguous: scopeGraph.isAmbiguous,
  isInvalid: scopeGraph.isInvalid,
  isRecovered: scopeGraph.isRecovered,
});

/**
 * WD1.6-C -- may a candidate legally occupy a node-valued field?
 *
 * Narrow ON PURPOSE. The class-derivation helpers, the exclusion-completeness
 * whitelist and the class-complement table are the module's reasoning, not its
 * contract; publishing them would invite a consumer to re-implement the
 * judgement instead of reading the verdict. `CANDIDATE_KIND` IS published
 * because a verdict carries it and a consumer must be able to branch on it.
 *
 * The scope graph is built through `interfaceQuery.buildScopeGraph`, so the whole
 * consumer path is `parse -> interfaceQuery.buildScopeGraph ->
 * containment.childLegality` with no internal import anywhere in it.
 *
 * NO POLICY. `UNSUPPORTED` does not mean "permit" and `UNRESOLVED` does not mean
 * "warn" -- what a UI does with an uncertain verdict is WD2's decision, and
 * encoding it here would make the semantic answer unrecoverable.
 */
const publicContainment = Object.freeze({
  childLegality: containment.childLegality,
  // ISO 4.8.3 entered at a prototype DECLARATION rather than at an occurrence --
  // the SAME derivation `childLegality` runs, published because WD1.7-D needs to
  // ask it of an externally proven target and a second implementation of "the
  // first body node determines the class" is exactly what must not exist. An
  // `ExternProto` is answered strictly (`UNSUPPORTED`), with no way to reach
  // external evidence from here.
  protoImplementationClass: containment.protoImplementationClass,
  CONTAINMENT_STATUS: containment.CONTAINMENT_STATUS,
  CONTAINMENT_REASON: containment.CONTAINMENT_REASON,
  CANDIDATE_KIND: containment.CANDIDATE_KIND,
});

/**
 * WD1.6-D -- what is semantically wrong with this document, and how sure are we?
 *
 * Narrow ON PURPOSE, and narrower than the two above: ONE query plus the two
 * tables a consumer must branch on. The ISO classification table, the producers
 * and the placement traversal are the module's reasoning, not its contract.
 *
 * `STATUS` and `REASON` are deliberately NOT re-exported here -- a finding's
 * `confidence` and `reason` ARE the scope graph's own values, and publishing a
 * second copy of those tables under this name would invite a consumer to
 * believe they are a different vocabulary. Read them from `interfaceQuery`.
 *
 * NO PRESENTATION. There is no severity, no message, no visibility and no
 * adapter that would produce one -- deciding those is P4's job, and a finding
 * exists so P4 can decide them from facts rather than from a pre-judged label.
 */
const publicSemanticFindings = Object.freeze({
  findingsForDocument: semanticFindings.findingsForDocument,
  FINDING_CODE: semanticFindings.FINDING_CODE,
  ISO_RESULT: semanticFindings.ISO_RESULT,
});

/**
 * WD1.7-C (pure) -- which PROTO does this target document supply, and what does
 * a realized implementation depend on?
 *
 * Narrow, in the same spirit as the three above: two queries, one declaration
 * reader, and the constant tables a consumer must branch on. There is no
 * candidate walker and no resolver here -- those need retrieval, retrieval needs
 * `fs`, and this facade must stay loadable in the renderer.
 *
 * `selectPrototypeTarget` returns AST handles that are PARSE-LIFETIME ONLY:
 * derived, disposable projections of the caller's own parse, never a persistent
 * identity and never written anywhere (WD.md §2).
 */
const publicProtoTarget = Object.freeze({
  selectPrototypeTarget: protoTarget.selectPrototypeTarget,
  externProtoCandidates: protoTarget.externProtoCandidates,
  prototypeDependencies: protoTarget.prototypeDependencies,
  SELECTION_RULE: protoTarget.SELECTION_RULE,
  SELECTION_STATUS: protoTarget.SELECTION_STATUS,
  SELECTION_REASON: protoTarget.SELECTION_REASON,
  DEPENDENCY_KIND: protoTarget.DEPENDENCY_KIND,
  COVERAGE_GAP: protoTarget.COVERAGE_GAP,
});

/**
 * WD1.7-D (pure) -- does a local EXTERNPROTO interface satisfy ISO 4.9.2 against
 * the implementation WD1.7-C proved?
 *
 * Narrow, in the same spirit as the others: one comparison, one `NOT_ATTEMPTED`
 * constructor for a caller whose C outcome proved no target, and the constant
 * tables a consumer must branch on. There is no resolver here and no retrieval --
 * both sides arrive as a scope graph this facade's own `buildScopeGraph` built.
 *
 * NO PRESENTATION and NO COMPATIBILITY PROFILE. A finding carries what was
 * observed and whose rule it is (`AGREEMENT_BASIS`); severity is P4's and profile
 * classification is WD1.7-E's, and neither is reachable from here.
 */
const publicProtoAgreement = Object.freeze({
  compareInterfaceAgreement: protoAgreement.compareInterfaceAgreement,
  notAttempted: protoAgreement.notAttempted,
  AGREEMENT_STATUS: protoAgreement.AGREEMENT_STATUS,
  MEMBER_STATUS: protoAgreement.MEMBER_STATUS,
  AGREEMENT_FINDING: protoAgreement.AGREEMENT_FINDING,
  AGREEMENT_BASIS: protoAgreement.AGREEMENT_BASIS,
  AGREEMENT_REASON: protoAgreement.AGREEMENT_REASON,
});

/**
 * P4-A -- how should a consumer SHOW a semantic finding?
 *
 * The counterpart to every "NO PRESENTATION" note above. Those modules refuse
 * severity, wording, visibility and ordering so that this one can decide them
 * once, from facts, in a place a reviewer can read end to end.
 *
 * Published in full, unlike the reasoning tables of the modules it reads: the
 * whole point of the lane is that a consumer branches on these values, so the
 * vocabularies ARE the contract. What is NOT published is the policy tables --
 * `CLAIM_BY_ISO` and its siblings are how the decision is made, not what it is.
 *
 * NO MESSAGE TEXT and NO COMPATIBILITY REGISTRY. Prose is P4-B, and which
 * profile earns an attachment stays where WD1.7-E1 put it; this facade exposes
 * how an attachment is PRESENTED, never how one is granted.
 */
const publicPresentation = Object.freeze({
  presentSemanticFinding: presentation.presentSemanticFinding,
  presentAgreementFinding: presentation.presentAgreementFinding,
  presentAgreementStatus: presentation.presentAgreementStatus,
  presentDocumentFindings: presentation.presentDocumentFindings,
  orderPresentations: presentation.orderPresentations,
  SEVERITY: presentation.SEVERITY,
  FINDING_ORIGIN: presentation.FINDING_ORIGIN,
  CLAIM: presentation.CLAIM,
  CONFIDENCE_CLASS: presentation.CONFIDENCE_CLASS,
  FINDING_GROUP: presentation.FINDING_GROUP,
  FILTER_TAG: presentation.FILTER_TAG,
  PRESENTATION_ERROR: presentation.PRESENTATION_ERROR,
});

/**
 * P4-B -- the words a presentation result renders.
 *
 * One entry point per P4-A presentation entry point, plus one dispatcher that
 * reads `presentation.origin`. Each returns a frozen `{ id, title, summary,
 * detail }` -- nothing else -- so WD2 never has to decide how a finding reads.
 *
 * `MESSAGE_ID` and `MESSAGE_ERROR` are published because they are stable
 * consumer-facing identifiers a UI keys off (the ID for filtering, the error
 * code for fall-through). The catalog tables themselves are reasoning, not
 * contract, and stay on the module -- the same split P4-A made.
 */
const publicMessages = Object.freeze({
  messageForSemanticFinding: messages.messageForSemanticFinding,
  messageForAgreementFinding: messages.messageForAgreementFinding,
  messageForAgreementStatus: messages.messageForAgreementStatus,
  messageForPresentation: messages.messageForPresentation,
  MESSAGE_ID: messages.MESSAGE_ID,
  MESSAGE_ERROR: messages.MESSAGE_ERROR,
});

/**
 * WD2-A -- a read-only scene-tree read model for the editor's scene-tree view
 * and inspector. Built over the AST, never over source text. Identifiers are
 * session-stable (derived from a kind and the source range), so a re-render
 * of the same parse produces the same ids; after a reparse the caller decides
 * how to map selections forward (the read model is not authoritative identity).
 *
 * Two helpers are published:
 *   - `buildSceneTree(parseResult)` produces `{ root, items, byId, totals }`.
 *   - `itemContainingOffset(treeResult, offset)` finds the most-specific item
 *     a finding or click at `offset` should attach to.
 *   - `itemById(treeResult, id)` looks one up by id.
 *
 * NOT published: the constructor helpers, `idFor`, `rangeCopy` -- they are
 * reasoning, not contract, and the same facade split P4-A made.
 */
const publicSceneTree = Object.freeze({
  buildSceneTree: sceneTree.buildSceneTree,
  itemContainingOffset: sceneTree.itemContainingOffset,
  itemById: sceneTree.itemById,
  astNodeForItem: sceneTree.astNodeForItem,
  itemForAstNode: sceneTree.itemForAstNode,
  KIND: sceneTree.KIND,
  USE_TARGET: sceneTree.USE_TARGET,
});

const publicFieldEdit = Object.freeze({
  inspectNodeFields: fieldEdit.inspectNodeFields,
  planFieldEdit: fieldEdit.planFieldEdit,
  FIELD_EDIT_STATUS: fieldEdit.FIELD_EDIT_STATUS,
  FIELD_EDIT_REASON: fieldEdit.FIELD_EDIT_REASON,
  PLAN_STATUS: fieldEdit.PLAN_STATUS,
  EDITABLE_TYPES: fieldEdit.EDITABLE_TYPES,
});

const publicStructureEdit = Object.freeze({
  planInsertObject: structureEdit.planInsertObject,
  planDuplicateNode: structureEdit.planDuplicateNode,
  planDeleteNode: structureEdit.planDeleteNode,
  planFieldInsert: structureEdit.planFieldInsert,
  resolveInsertedNode: structureEdit.resolveInsertedNode,
  STRUCTURE_REASON: structureEdit.STRUCTURE_REASON,
  INSERTED_STATUS: structureEdit.INSERTED_STATUS,
  PLAN_STATUS: structureEdit.PLAN_STATUS,
});

const publicNodeTemplates = Object.freeze({
  simpleObjectTemplate: nodeTemplates.simpleObjectTemplate,
  PRIMITIVES: nodeTemplates.PRIMITIVES,
  VRML97_HEADER: nodeTemplates.VRML97_HEADER,
});

const publicSimpleObject = Object.freeze({
  recognize: simpleObject.recognize,
  displayLabel: simpleObject.displayLabel,
  describeObject: simpleObject.describeObject,
  planPropertySet: simpleObject.planPropertySet,
  OBJECT_REASON: simpleObject.OBJECT_REASON,
});

// parse(text, opts) -> full result. opts: { profile, maxDepth, maxNodes }.
function parse(text, opts = {}) {
  const syntax = parseSyntax(text, opts);
  const semantic = analyze(syntax.tree);
  const refs = assetRefs.extractAssetRefs(syntax.tree);
  return {
    profile: opts.profile || 'generic',
    tree: syntax.tree,
    tokens: syntax.tokens,
    comments: syntax.comments,
    diagnostics: syntax.diagnostics.concat(semantic.diagnostics),
    syntaxDiagnostics: syntax.diagnostics,
    semanticDiagnostics: semantic.diagnostics,
    defs: semantic.defs,
    defsByName: semantic.defsByName,
    duplicateDefs: semantic.duplicateDefs,
    uses: semantic.uses,
    routes: semantic.routes,
    assetRefs: refs,
    truncated: syntax.truncated,
    depthCapped: syntax.depthCapped,
    limits: syntax.limits,
  };
}

module.exports = {
  parse,
  tokenize,
  analyze,
  createSourceMap,
  edit,
  nodeSchema,
  documentTransaction: publicDocumentTransaction,
  nodeIdentity: publicNodeIdentity,
  interfaceQuery: publicInterfaceQuery,
  containment: publicContainment,
  semanticFindings: publicSemanticFindings,
  protoTarget: publicProtoTarget,
  protoAgreement: publicProtoAgreement,
  presentation: publicPresentation,
  messages: publicMessages,
  sceneTree: publicSceneTree,
  fieldEdit: publicFieldEdit,
  structureEdit: publicStructureEdit,
  nodeTemplates: publicNodeTemplates,
  simpleObject: publicSimpleObject,
  ast,
  diagnostics,
  assetRefs,
  TT,
  KEYWORDS,
  DEFAULT_LIMITS,
};
