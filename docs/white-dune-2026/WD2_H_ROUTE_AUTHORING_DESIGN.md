# WD2-H-R Behavior and ROUTE authoring — design

**Status:** Draft — proposal for owner review; not approved

This document proposes the architecture for behavior and ROUTE authoring in WRL Forge, satisfying issue #71. This is a design-only document; no production code changes are included.

## 1. Goal
Visualize and edit ROUTEs, sensors, TimeSensor and Interpolators, allowing users to see which nodes are wired together and create/edit interaction wiring without hand-typing ROUTEs.

## 2. In Scope
- ROUTE visualization
- ROUTE editing
- Sensors
- TimeSensor
- Interpolators
- Interaction wiring

## 3. Out of Scope
- Script execution (SEC-SCRIPT-2)
- Keyframe timeline (WD2-I)

## 4. Architecture Constraints

### 4.1. Source-First Principle
The exact source text is the document (WD.md §2); every edit lands as WD1.2 span patches.
There is no second editable document model, canonical scene graph, or AST→text regeneration.

### 4.2. No Sidecar State
No hidden source instrumentation, synthetic identifiers, or sidecar semantic state is permitted. All relationships are derived directly from the exact parsed source.

### 4.3. Fail-Closed Identity
Unsupported or ambiguous identity fails closed (WD.md §7 hard gate). We must never offer a "confident wrong node".

### 4.4. One Undo Transaction
One user-visible action (e.g., creating a ROUTE, rewiring an endpoint, or deleting a connection) equals exactly one coherent undo transaction in CodeMirror, restoring the document to its precise byte-exact prior state.

### 4.5. Proven versus Half-Proven Edges
ROUTE legality comes strictly from the `WD1.5-P2C` endpoint resolution subsystem.
Half-proven edges (where one or both endpoints cannot be firmly resolved or type-checked) are NOT offered as proven. The ROUTE graph shows only fully proven edges as "proven". Half-proven edges must be visibly distinct or suppressed in the authoring UI, reflecting their unresolved status.

## 5. Type-Checked Creation and Wiring

### 5.1. Endpoint Resolution Requirement
When creating a new ROUTE between `sourceNode.sourceEvent` and `destNode.destEvent`, the operation MUST consult `WD1.5-P2C` endpoint resolution to ensure:
- The source node is resolved and possesses the specified `eventOut` (or `exposedField`).
- The destination node is resolved and possesses the specified `eventIn` (or `exposedField`).

### 5.2. Type Compatibility (ISO 4.10)
Created ROUTEs must type-check per ISO 4.10 requirements. The data type of the source event must exactly match the data type of the destination event.

### 5.3. Refusal Reasons
If a user attempts to create an illegal connection, the system must refuse the operation and provide a clear reason, sourced from the existing diagnostic catalogs (e.g., `ROUTE_CONNECTION_REJECTED` due to type mismatch, or missing endpoint).

## 6. Minimal Source-Span Patches
When a ROUTE is created or deleted, the document is modified using the existing `WD1.2` patch algebra.
- **Creation:** Appends the `ROUTE sourceNode.sourceEvent TO destNode.destEvent` statement at an appropriate valid location (e.g., end of the enclosing scope or document root), preserving surrounding whitespace, comments, and CRLF conventions.
- **Deletion:** Identifies the exact `WD1.2` source span of the `ROUTE` statement and issues a patch to remove exactly those bytes.

## 7. Dependencies
- No hard technical block recorded; planning order is expressed by Horizon.
- **Blocks:** WD2-I (Animation and timeline authoring).
- **Relies on:** WD1.5-P2C (ROUTE endpoints, event resolution, and type checking constraints) and WD1.2 (source-span patching).

## 8. Evidence and Test Plan

### 8.1. Corpus Proof
The design must be validated against a focused corpus that tests the parser, scope resolution, and endpoint validation logic against real-world `ROUTE`, sensor, and interpolator patterns. The existing `WD1.5-P2C` corpus harness will be used to verify that no regressions occur.

### 8.2. Visual QA Cases (Electron)
The Electron visual QA suite must be updated to cover:
- Display of proven ROUTEs.
- Refusal of type-mismatched or unresolvable ROUTE creation attempts.
- One-transaction undo/redo behavior verifying the exact byte-for-byte restoration of the document.

## 9. Unresolved Decisions
- **Placement of New ROUTEs:** When inserting a new `ROUTE` statement into the source text via a span patch, what is the canonical placement rule (e.g., end of document, immediately after the source node definition, or clustered with existing ROUTEs)? The fallback is end-of-scope appending.

## 10. Limits
This design explicitly limits the system to visual authoring and type-checking of the ROUTE topology. Execution of behaviors (Scripts) and timeline animation authoring remain strictly out of scope.
