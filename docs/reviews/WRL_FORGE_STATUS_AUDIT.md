# WRL Forge Status Audit

## 1. Completed Work

*   **Crash Recovery (RUST-STAB-1)**
    *   *Source:* `docs/architecture/TAURI_RUST_MIGRATION_1.md`, `apps/desktop-tauri/README.md`
    *   *Evidence:* Mentions of `RUST-STAB-1` crash recovery, tests in `src-tauri/src/recovery.rs`, and smoke test `--recovery`.
    *   *Status:* Documented as complete in repository docs; tests and smoke runs were not re-run in this audit.
    *   *Recommended Action:* None needed.

*   **Extrusion Authoring (VISUAL-4A)**
    *   *Source:* `docs/architecture/TAURI_RUST_MIGRATION_1.md`
    *   *Evidence:* Extrusion editor documented, `--extrusion` smoke test, and `ui/src/smoke/extrusion.rs`.
    *   *Status:* Documented as complete in repository docs; tests and smoke runs were not re-run in this audit.
    *   *Recommended Action:* Add to completed features list in `PRODUCT_VISION.md`.

*   **Desktop Menus & Commands (UI-MENU-1)**
    *   *Source:* `docs/architecture/TAURI_RUST_MIGRATION_1.md`
    *   *Evidence:* Documented as built, smoke tests via `--menu`.
    *   *Status:* Documented as complete in repository docs; tests and smoke runs were not re-run in this audit.
    *   *Recommended Action:* Document in roadmap history.

*   **Scope Graph & Typed References (RUST-STAB-2A, RUST-STAB-2B)**
    *   *Source:* `docs/architecture/TAURI_RUST_MIGRATION_1.md`
    *   *Evidence:* Documented as built, tests in `crates/wrlforge-vrml`.
    *   *Status:* Documented as complete in repository docs; tests and smoke runs were not re-run in this audit.
    *   *Recommended Action:* Ensure roadmap reflects parser capability advancement.

*   **Viewport Picking (VISUAL-2 / WD2-D)**
    *   *Source:* `docs/WRL_FORGE_ROADMAP.md`, `apps/desktop-tauri/README.md`, `docs/architecture/TAURI_RUST_MIGRATION_1.md`, `docs/white-dune-2026/WD2_D_XITE_PICKING_CLOSEOUT.md`
    *   *Evidence:* `WD2-D` is marked as merged (PR #117) and closeout record confirms `WRLFORGE_WD2_D_INDEPENDENT_QA_RETEST_PASS`. `--pick` smoke test in Tauri app. Rust proof and xdotool tests exist.
    *   *Status:* Documented complete; closeout record cited, tests not re-run in this audit (within its approved scope; deferred selection edge cases tracked separately e.g. #118, #119).
    *   *Recommended Action:* Update `docs/PRODUCT_VISION.md` to indicate this is complete instead of "next planned".

## 2. Active Work

*   **Tauri Rust Migration (TAURI-RUST-MIGRATION-1)**
    *   *Source:* `apps/desktop-tauri/README.md`, `docs/architecture/TAURI_RUST_MIGRATION_1.md`
    *   *Evidence:* Porting of features from Electron to Rust/Tauri.
    *   *Status:* Partially implemented.
    *   *Recommended Action:* Continue migration according to milestones.

*   **3D Transform Gizmos & Authoring (VISUAL-3A, VISUAL-4B / WD2-E)**
    *   *Source:* `docs/architecture/TAURI_RUST_MIGRATION_1.md`, `apps/desktop-tauri/README.md`, `docs/WRL_FORGE_ROADMAP.md`
    *   *Evidence:* `VISUAL-4B` Rotate and Scale tools documented, `--move` and `--rotscale` smoke tests. Code in `ui/src/smoke/rotscale.rs`. WD2-E is marked as "Next" in roadmap.
    *   *Status:* Partially implemented (feature implementation done, full workstream completion pending acceptance).
    *   *Recommended Action:* Update `docs/PRODUCT_VISION.md`.

*   **Docking & Configurable Workspace (UI-DOCK-1 / UI-C0 / UI-1)**
    *   *Source:* `docs/WRL_FORGE_ROADMAP.md`, `apps/desktop-tauri/README.md`
    *   *Evidence:* Smoke test `--dock` exists. UI-C0 / UI-1 are marked as "Next" in roadmap.
    *   *Status:* Partially implemented (feature implementation done, full workstream completion pending acceptance).
    *   *Recommended Action:* None.

## 3. Blocked Work

*   **macOS Tauri Build Support**
    *   *Source:* `apps/desktop-tauri/README.md`
    *   *Evidence:* Mentions "macOS 15+ is a target but has not been built yet."
    *   *Status:* macOS-specific source and dependencies exist. No native macOS Rust/Tauri build, CI job, or run is verified (cause of the block is unverified). See `docs/reviews/MACOS_TAURI_READINESS.md`.
    *   *Recommended Action:* Owner decision: authorize a macOS native build check when the macOS 15+ phase starts.

## 4. Future Work

*   **Hierarchy Authoring (WD2-F)**
    *   *Source:* `docs/WRL_FORGE_ROADMAP.md`
    *   *Evidence:* Listed under "Next".
    *   *Status:* Open or not authorized.
    *   *Recommended Action:* None.

*   **Protected-preview Script diagnostics (SEC-SCRIPT-1)**
    *   *Source:* `docs/WRL_FORGE_ROADMAP.md`
    *   *Evidence:* Listed under "Next".
    *   *Status:* Open or not authorized.
    *   *Recommended Action:* None.

*   **Appearance (WD2-G), Behavior (WD2-H), Animation (WD2-I), PROTO (WD2-J), Advanced Source Tools (WD2-K)**
    *   *Source:* `docs/WRL_FORGE_ROADMAP.md`
    *   *Evidence:* Listed under "Later".
    *   *Status:* Open or not authorized.
    *   *Recommended Action:* None.

## 5. Outdated Roadmap Entries & Documentation

*   **`docs/PRODUCT_VISION.md` - Visual Features Status**
    *   *Source:* `docs/PRODUCT_VISION.md`
    *   *Evidence:* States "Viewport picking and 3D transform gizmos are the next planned visual features."
    *   *Status:* Stale claim. Repository docs (`apps/desktop-tauri/README.md`, `docs/architecture/TAURI_RUST_MIGRATION_1.md`) describe these features as implemented; this audit did not re-run their tests.
    *   *Recommended Action:* Update `docs/PRODUCT_VISION.md` to state that viewport picking is built (WD2-D closed) and that the Translate, Rotate and Scale tools are implemented. WD2-E (#50) stays open: snapping is partial, multi-selection is missing, and independent QA is pending.

## 6. Target Platforms Definition (Linux, macOS, Windows 11)

*   **Target Platforms Roadmap Synchronization**
    *   *Source:* Owner Decision 2026-10-15.
    *   *Evidence:* The product vision correctly focuses on Linux as the current primary target, followed by macOS 15+. Windows 11 is definitively recognized as a future planned target, not permanently out-of-scope. Mobile remains definitively out-of-scope.
    *   *Status:* Documentation requires alignment to properly articulate the sequence: Linux (Current) -> macOS 15+ (Later) -> Windows 11 (Future). Current references treating Windows as "out of scope" must be revised to "planned future target (currently unauthorized for implementation)". Windows assets should remain.
    *   *Recommended Action:* Update `docs/PRODUCT_VISION.md` and `apps/desktop-tauri/README.md` to explicitly list Windows 11 as a planned future target while clarifying it is not yet authorized for implementation, packaging, or release.

## 7. Recommended Tasks for Jules (Grouped by Dependency & Priority)

### High Priority
*   **Task 1: Documentation Alignment (Independent)**
    *   *Description:* Update `docs/PRODUCT_VISION.md` to mark viewport picking and extrusion as built, and the Translate, Rotate and Scale tools as implemented. State that WD2-E (#50) is not accepted: snapping is partial, multi-selection is missing, and independent QA is pending. Update the same file and `apps/desktop-tauri/README.md` to articulate the platform rollout sequence (Linux -> macOS 15+ -> Windows 11), ensuring Windows 11 is recognized as a future target and not permanently out-of-scope. Mobile should be the only platform stated as out-of-scope.
