# Issues and Security/Quality Audit Report

**Repository:** [Ascendance3D/wrlforge](https://github.com/Ascendance3D/wrlforge)
**Audited commit:** `ee68e9c3e8f9948af800ba39fd83d7d0d303d800` (the PR base when the audit ran)
**Status:** Audit & Analysis Report (Draft)

---

## Executive Summary

An audit of the repository was performed to analyze **80 open GitHub issues** (issues #1 to #159 at the audited commit; issues #160 and later are Jules task issues added afterward, and 90 issues were open when this PR was reviewed) and **37 open Dependabot alerts** (see section 2).

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

## 2. Analysis of Security and Quality Alerts

**Source:** the GitHub Dependabot alerts API for this repository, queried during the review of this PR (`state=open`). The query returned **37 open alerts**: 23 high, 13 medium, 1 low. Code-scanning returned "no analysis found". Secret-scanning alerts were not readable with the available token scope, so the secret-scanning count is **not verified**. The alerts were not re-checked against `npm audit` output.

### Dependabot alerts by package

| Package | Alerts | Highest severity | Scope | Patched version (per advisory) |
|---|---|---|---|---|
| `electron` | 6 | high | development | 41.10.3 to 41.10.6 |
| `fast-uri` | 7 | high | development | 3.1.4 to 3.1.7 |
| `@xmldom/xmldom` | 8 | high | development | 0.8.14 to 0.8.15 |
| `extract-zip` | 2 | high | development | none published |
| `http-cache-semantics` | 1 | high | development | none published |
| `js-yaml` | 2 | high | development | 4.3.1 and 4.3.2 |
| `brace-expansion` | 3 | medium | development | 1.1.21, 2.1.7, 5.0.12 |
| `undici` | 4 | medium | development | 6.28.0 and 6.28.1 |
| `esbuild` | 1 | medium | development | 0.25.0 |
| `sprintf-js` | 1 | medium | development | none published |
| `glib` (Rust) | 2 | medium | **runtime** | 0.20.0 |

All npm alerts have `development` scope: they come from the legacy Electron toolchain. The two `glib` alerts are the only `runtime` alerts and affect the Rust/Tauri build. Their fix needs a review of the Tauri and GTK dependency tree, which is outside this audit. The advisory IDs are in the Dependabot alert list.

The `esbuild` alert needs `>= 0.25.0`, so the current `^0.24.2` range is affected. No `tar` alert is open. Fixes for the packages with no published patch need an owner decision.

### Rust toolchain

`crates/Cargo.toml` sets `rust-version = "1.95"`. The compiler version of the original audit session was not recorded. A later local check (2026-10-16) ran `rustc 1.95.0 (59807616e 2026-04-14)`, which meets the required version. No toolchain gap was found.

---

## 3. Recommended Phased Implementation Roadmap

1. **Phase 1: Security & Dependency Remediation**
   * Owner decision: whether to fix development-scope npm alerts in the legacy Electron toolchain, and how to handle the runtime `glib` alerts in the Rust/Tauri build.
   * Each dependency change needs its own approved lane.
2. **Phase 2: Foundation Workspace & Security Sandbox**
   * Finalize UI Architecture & Command UX (#33, #38, #43, #121, #122).
   * Implement Protected-preview Script diagnostics and isolated mode (#75, #80).
3. **Phase 3: Visual Authoring Features (WD2-E through WD2-K)**
   * Complete WD2-E transform authoring (snapping #56, multi-selection #57).
   * Sequence WD2-F through WD2-K authoring lanes.
4. **Phase 4: Release Completion Gates**
   * Complete cross-platform smoke QA (#102), accessibility (#107), and product completion gate (#96).
