# Issues and Security/Quality Audit Report

**Repository:** [Ascendance3D/wrlforge](https://github.com/Ascendance3D/wrlforge)
**PR Head SHA:** `ee68e9c3e8f9948af800ba39fd83d7d0d303d800`
**Status:** Audit & Analysis Report (Draft)

---

## Executive Summary

An audit of the repository was performed to analyze the **80 open GitHub issues** and **37 security/quality alerts** (including 18 npm vulnerability packages, test module import dependencies, and Rust compiler toolchain requirements).

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

### Npm Vulnerabilities (18 Packages Identified via `npm audit`)
1. **`esbuild` (Moderate):** Dev dependency in `esbuild <=0.24.2` (GHSA-67mh-4wv8-2f99). Recommended target: Upgrade to `esbuild >= 0.28.2`.
2. **`js-yaml` (High):** Quadratic CPU consumption & empty merge sources DoS (CVE-2026-59870 / GHSA-5p4m-2wfm-xmqj). Recommended target: Upgrade to `js-yaml >= 4.3.1`.
3. **`electron` (High):** Protocol response session cache / sandbox iframe popup bypass. Recommended target: Upgrade to latest supported 41.x patch or 42.x.
4. **`electron-builder` & `app-builder-lib` (Moderate):** Transitive dependency on `@electron/get` / `global-agent` / `roarr` / `sprintf-js`. Recommended target: Update `electron-builder` to latest 26.x version.
5. **Transitive High Severity Packages:**
   * `brace-expansion` (DoS via unbounded expansion length)
   * `extract-zip` (Symlink path traversal)
   * `tar` (Uncontrolled recursion in long-path tar member selection)
   * `undici` (CRLF injection and response splitting)
   * `fast-uri` (Host confusion & SSRF)
   * `http-cache-semantics` (Cross-user cached response disclosure)
   * `@xmldom/xmldom` (Entity reference and element name injection)

### Rust Toolchain Parity
* `crates/Cargo.toml` specifies `rust-version = "1.95"`.
* Current environment has `rustc 1.94.0`. Building workspace crates requires updating the Rust toolchain to `rustc 1.95+`.

---

## 3. Recommended Phased Implementation Roadmap

1. **Phase 1: Security & Dependency Remediation**
   * Upgrade `esbuild` and `js-yaml` in `package.json`.
   * Run `npm audit fix` for transitive vulnerability resolutions.
   * Update Rust toolchain to `1.95`.
2. **Phase 2: Foundation Workspace & Security Sandbox**
   * Finalize UI Architecture & Command UX (#33, #38, #43, #121, #122).
   * Implement Protected-preview Script diagnostics and isolated mode (#75, #80).
3. **Phase 3: Visual Authoring Features (WD2-E through WD2-K)**
   * Complete WD2-E transform authoring (snapping #56, multi-selection #57).
   * Sequence WD2-F through WD2-K authoring lanes.
4. **Phase 4: Release Completion Gates**
   * Complete cross-platform smoke QA (#102), accessibility (#107), and product completion gate (#96).
