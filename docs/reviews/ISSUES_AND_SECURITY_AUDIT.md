# Issues and Security/Quality Audit Report

**Repository:** [Ascendance3D/wrlforge](https://github.com/Ascendance3D/wrlforge)
**Base commit:** `ee68e9c3e8f9948af800ba39fd83d7d0d303d800` (`main` when this audit branch was cut)
**Status:** Audit & Analysis Report (Draft)

---

## Executive Summary

An audit of the repository was performed to analyze the **80 open GitHub issues** (excluding the ten Jules queue issues #160-#169 opened later) and **37 open Dependabot alerts** (GitHub Dependabot API, queried 2026-10-16; code-scanning has no analysis on this repository), plus the Rust toolchain requirement.

No production code or package configurations were altered as part of this analysis task.

---

## 1. Analysis of the 80 Open Issues

The 80 open issues belong to four primary functional domains:

### Domain A: UI & Workspace Architecture (17 Issues)
* **Core Issues:**
  * [#33: [UI-0] Command and workspace architecture](https://github.com/Ascendance3D/wrlforge/issues/33)
  * [#38: [UI-C0] Dockable workspace engine spike](https://github.com/Ascendance3D/wrlforge/issues/38)
  * [#43: [UI-1] Configurable docked workspace](https://github.com/Ascendance3D/wrlforge/issues/43)
  * [#121: [UI-0-I2] Workspace behavior and command UX](https://github.com/Ascendance3D/wrlforge/issues/121)
  * [#122: [UI-0-M] Application menu command bridge](https://github.com/Ascendance3D/wrlforge/issues/122)
* **Sub-tasks & QA:** #36, #37, #39, #40, #41, #42, #44, #45, #46, #47, #48, #49.

### Domain B: White Dune 2026 Authoring Lanes (42 Issues)
* **Core Lanes:**
  * [#50: [WD2-E] Transform authoring and snapping](https://github.com/Ascendance3D/wrlforge/issues/50) (Note: Basic Translate, Rotate, and Scale gizmos/tools are implemented in the Rust/Tauri app, but WD2-E acceptance items such as snapping #56 and multi-selection #57 remain incomplete).
  * [#60: [WD2-F] Source-safe hierarchy authoring](https://github.com/Ascendance3D/wrlforge/issues/60)
  * [#65: [WD2-G] Appearance authoring](https://github.com/Ascendance3D/wrlforge/issues/65)
  * [#70: [WD2-H] Behavior and ROUTE authoring](https://github.com/Ascendance3D/wrlforge/issues/70)
  * [#81: [WD2-I] Animation and timeline authoring](https://github.com/Ascendance3D/wrlforge/issues/81)
  * [#86: [WD2-J] PROTO and EXTERNPROTO visual workflow](https://github.com/Ascendance3D/wrlforge/issues/86)
  * [#91: [WD2-K] Advanced VRML Pad and source tools](https://github.com/Ascendance3D/wrlforge/issues/91)
  * [#118: [WD2-D-INLINE] Viewport selection and opening of Inline content](https://github.com/Ascendance3D/wrlforge/issues/118)
* **Sub-tasks & QA:** #51–#59, #61–#64, #66–#69, #71–#74, #82–#85, #87–#90, #92–#95, #119.

### Domain C: Script Security & Diagnostics (6 Issues)
* **Core Security:**
  * [#75: [SEC-SCRIPT-1] Protected-preview Script diagnostics](https://github.com/Ascendance3D/wrlforge/issues/75)
  * [#80: [SEC-SCRIPT-2] Explicit isolated Script execution mode](https://github.com/Ascendance3D/wrlforge/issues/80)
* **Sub-tasks & QA:** #76, #77, #78, #79.

### Domain D: Release Completion Gates (15 Issues)
* **Completion Gates:**
  * [#96: [COMPLETE] WRL Forge product completion gate](https://github.com/Ascendance3D/wrlforge/issues/96)
  * Sub-gates: #97 (New File), #98 (Workspace Recovery), #99 (Keybindings), #100 (Layout Reset), #101 (Release Packaging), #102 (Cross-Platform Smoke QA), #103 (Installer Docs), #104 (Walkthrough), #105 (VRML Compatibility), #106 (Performance), #107 (Accessibility), #108 (Security Review), #109 (Documentation Cleanup), #110 (Roadmap Closure).

---

## 2. Analysis of Security Alerts

Source: GitHub Dependabot open alerts (37). Counts are alerts, not packages; one package can carry several. Severities are as GitHub reports them. `npm audit` was not run for this document, so no claim here is derived from it.

| Package | Open alerts | Severity (GitHub) | First patched version (GitHub) |
| --- | --- | --- | --- |
| `electron` | 6 | 5 high, 1 medium | 41.10.3 to 41.10.6 (latest needed: 41.10.6) |
| `fast-uri` | 7 | high | 3.1.4 to 3.1.7 |
| `@xmldom/xmldom` | 8 | 6 high, 2 medium | 0.8.14 / 0.8.15 |
| `undici` | 4 | 3 medium, 1 low | 6.28.0 / 6.28.1 |
| `js-yaml` | 2 | high | 4.3.1 / 4.3.2 |
| `extract-zip` | 2 | high | none published |
| `http-cache-semantics` | 1 | high | none published |
| `brace-expansion` | 3 | medium | 1.1.21 / 2.1.7 / 5.0.12 |
| `esbuild` | 1 | medium | 0.25.0 |
| `sprintf-js` | 1 | medium | none published |
| `glib` (Rust crate) | 2 | medium | 0.20.0 |

Notes:
* `package.json` on `main` pins `esbuild ^0.24.2` and `js-yaml ^4.3.0`, both inside the affected ranges. `electron ^41.7.1` resolves per the lockfile; confirm it reaches 41.10.6.
* Alert paths for transitive packages (`electron-builder` and its dependencies) were not traced here.
* Two `extract-zip`, one `http-cache-semantics` and one `sprintf-js` alert have no patched version; remediation needs an upstream bump or replacement, which is unverified.
* An earlier draft listed `tar` as a high-severity finding. It has no open Dependabot alert and is removed.

### Rust Toolchain Parity
* `crates/Cargo.toml` specifies `rust-version = "1.95"`.
* The audit environment reported `rustc 1.94.0` (unverified here; the reviewer environment has 1.95.0). Building workspace crates requires updating the Rust toolchain to `rustc 1.95+`.

---

## 3. Recommended Phased Implementation Roadmap

1. **Phase 1: Security & Dependency Remediation**
   * Upgrade `esbuild` (>= 0.25.0) and `js-yaml` (>= 4.3.2) in `package.json`.
   * Re-check the Dependabot alert list after the lockfile update; do not assume `npm audit fix` clears alerts without a published patch.
   * Update Rust toolchain to `1.95`.
2. **Phase 2: Foundation Workspace & Security Sandbox**
   * Finalize UI Architecture & Command UX (#33, #38, #43, #121, #122).
   * Implement Protected-preview Script diagnostics and isolated mode (#75, #80).
3. **Phase 3: Visual Authoring Features (WD2-E through WD2-K)**
   * Complete WD2-E transform authoring (snapping #56, multi-selection #57).
   * Sequence WD2-F through WD2-K authoring lanes.
4. **Phase 4: Release Completion Gates**
   * Complete cross-platform smoke QA (#102), accessibility (#107), and product completion gate (#96).
