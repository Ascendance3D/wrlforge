# SEC-SCRIPT-1: Protected-preview Script diagnostics Design

**Issue:** [#76](https://github.com/Ascendance3D/wrlforge/issues/76)
**Parent:** [#75](https://github.com/Ascendance3D/wrlforge/issues/75)
**Status:** Draft — proposal for owner review; not approved

## Background

WRL Forge provides a source-first VRML97 authoring environment with a protected embedded X_ITE preview. The current system architecture, by design, maintains a strict security boundary for the preview (`contextIsolation: true`, `nodeIntegration: false`, and a Content-Security-Policy (CSP) that explicitly omits `unsafe-eval`). As documented in `docs/WORLD_PROJECT_ARCHITECTURE.md` and `docs/PREVIEW_ARCHITECTURE.md`, inline scripts (`vrmlscript:`, `javascript:`) do not execute.

When a valid VRML97 `Script` node is authored, it parses correctly according to the ISO grammar rules (supported by `src/vrml/parser.js` and `src/vrml/semantic-findings.js`, e.g. `GRAMMAR_SCRIPT_INTERFACE`). However, because `unsafe-eval` is disabled, the script cannot run in the X_ITE renderer preview. This currently can result in misleading or missing rendering behaviors, or generic browser/renderer errors about `eval` being blocked. A clear diagnostic mechanism is necessary to inform the user about this expected limitation without compromising the secure sandbox.

## Design

This design introduces an accurate, user-visible notice for valid VRML97 `Script` nodes, explaining that they cannot execute in the protected preview.

1. **Diagnostic Rule:**
   - Add a new non-fatal advisory/diagnostic specific to `Script` nodes containing URL code logic (e.g., `vrmlscript:` or `javascript:` references) or just the usage of `Script` nodes in general.
   - **Message:** "Script nodes are parsed and saved correctly, but their code cannot execute in the WRL Forge protected preview for security reasons."

2. **Suppression of Misleading Follow-on Errors:**
   - A `Script` node failing to execute due to the CSP may trigger downstream X_ITE rendering errors (e.g., `EvalError` or failures in ROUTEs connected to the script).
   - These follow-on runtime errors originating from X_ITE when attempting to evaluate the script should be intercepted and suppressed from the user-facing diagnostics panel if the node has already been flagged by the `Script` preview limitation notice. This prevents the user from thinking there is a syntax error in their code when the issue is purely an execution boundary.
   - **Mechanism:** When the preview renderer emits warnings/errors, filter out those specific to the disabled eval mechanism if a `Script` node is in the document.

## Security & Invariants

This design explicitly preserves the existing security posture:
- **No Script Execution:** We do not design or enable Script execution.
- **Strict CSP:** The `script-src` remains unchanged (e.g. `'self' file: 'wasm-unsafe-eval'`). No `unsafe-eval` is added.
- **Process Boundaries:** `contextIsolation: true`, `nodeIntegration: false`, and the network remote-request boundaries (`url-policy.js`, `preview-source.js`) remain strictly unchanged.

## Threat / Failure Cases

- **Threat: CSP Bypass.** A modification accidentally relaxes the CSP to allow `unsafe-eval`.
  - *Mitigation:* Explicit test verifying the CSP header in `editor.html` and `world.html` lacks `unsafe-eval`.
- **Failure Case: Diagnostic Spam.** Every keystroke inside a Script node produces a new execution error that clutters the UI.
  - *Mitigation:* Ensure the new diagnostic is deduplicated per Script node and suppression of runtime eval errors is effective.
- **Failure Case: Valid Scripts Refused.** The parser starts failing/refusing to save valid VRML97 Script nodes.
  - *Mitigation:* Existing WD1.2 span patch and parser grammar rules remain untouched.

## Test Plan

- **Focused Tests:**
  - `test/vrml/script-diagnostic.test.js` (proposed): Verify that the semantic findings logic emits the correct notice when a `Script` node is parsed.
  - Verify that the suppression logic accurately filters out `EvalError` or CSP-blocked logs originating from the preview.
- **Electron Visual QA:**
  - Create a test VRML file with a valid `Script` node. Open it in the Electron app and verify the advisory is visible.
  - Confirm the preview does not crash and no unrelated "Eval blocked" error bubbles up.
- **Independent Security Review Criteria:**
  - Confirm the patch contains zero changes to Electron `webPreferences`, `main.js`, `editor.html` CSP, or `world.html` CSP.
  - Confirm no new remote origins are added.
  - Confirm no `eval` logic is introduced.

## Dependencies and Unresolved Decisions

- **Dependencies:** Relies on the existing P4 diagnostics and presentation pipeline (`src/vrml/presentation.js`, `src/vrml/messages.js`).
- **Unresolved Decisions:**
  - Whether to show the diagnostic on the `Script` node definition itself, or once per document. (Proposal: show on the `Script` node).

## Limits and Evidence

- This is a design/investigation PR only.
- No production code, dependency, configuration, CI, issue-state, or unrelated file changes are included.
- **Evidence:** `src/vrml/parser.js` currently parses `Script` correctly. `docs/WORLD_PROJECT_ARCHITECTURE.md` proves CSP limits.

## Review Status

Owner review or independent QA is **not complete**. Draft — proposal for owner review; not approved.
