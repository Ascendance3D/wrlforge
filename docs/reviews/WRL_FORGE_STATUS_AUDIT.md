# WRL Forge Status Audit

## 1. Completed Work

*   **Crash Recovery (RUST-STAB-1)**
    *   *Source:* `docs/architecture/TAURI_RUST_MIGRATION_1.md`, `apps/desktop-tauri/README.md`
    *   *Evidence:* Mentions of `RUST-STAB-1` crash recovery, tests in `src-tauri/src/recovery.rs`, and smoke test `--recovery`.
    *   *Status:* Complete.
    *   *Recommended Action:* None needed.

*   **Viewport Picking (VISUAL-2 / WD2-D)**
    *   *Source:* `docs/WRL_FORGE_ROADMAP.md`, `apps/desktop-tauri/README.md`
    *   *Evidence:* `WD2-D` is marked as merged (PR #117). `--pick` smoke test in Tauri app.
    *   *Status:* Complete.
    *   *Recommended Action:* Update `PRODUCT_VISION.md` to indicate this is complete instead of "next planned".

*   **3D Transform Gizmos & Authoring (VISUAL-3A, VISUAL-4B / WD2-E)**
    *   *Source:* `docs/architecture/TAURI_RUST_MIGRATION_1.md`, `apps/desktop-tauri/README.md`
    *   *Evidence:* `VISUAL-4B` Rotate and Scale tools documented, `--move` and `--rotscale` smoke tests. Code in `ui/src/smoke/rotscale.rs`.
    *   *Status:* Complete.
    *   *Recommended Action:* Update `PRODUCT_VISION.md` and move `WD2-E` to "Completed history" in `WRL_FORGE_ROADMAP.md`.

*   **Extrusion Authoring (VISUAL-4A)**
    *   *Source:* `docs/architecture/TAURI_RUST_MIGRATION_1.md`
    *   *Evidence:* Extrusion editor documented, `--extrusion` smoke test, and `ui/src/smoke/extrusion.rs`.
    *   *Status:* Complete.
    *   *Recommended Action:* Add to completed features list in `PRODUCT_VISION.md`.

*   **Docking & Configurable Workspace (UI-DOCK-1 / UI-C0 / UI-1)**
    *   *Source:* `docs/WRL_FORGE_ROADMAP.md`, `apps/desktop-tauri/README.md`, GitHub Issue titles.
    *   *Evidence:* Issue "UI-DOCK-1: magnetic docking and resizable workspace (1.4.5 Beta)" merged. Smoke test `--dock`.
    *   *Status:* Complete.
    *   *Recommended Action:* Move `UI-C0` / `UI-1` to "Completed history" in `WRL_FORGE_ROADMAP.md`.

*   **Desktop Menus & Commands (UI-MENU-1)**
    *   *Source:* `docs/architecture/TAURI_RUST_MIGRATION_1.md`, GitHub Issues
    *   *Evidence:* "UI-MENU-1: desktop menus and command integration (1.4.5 Beta)" merged.
    *   *Status:* Complete.
    *   *Recommended Action:* None needed.

*   **Scope Graph & Typed References (RUST-STAB-2A, RUST-STAB-2B)**
    *   *Source:* `docs/architecture/TAURI_RUST_MIGRATION_1.md`
    *   *Evidence:* Documented as built, tests in `crates/wrlforge-vrml`.
    *   *Status:* Complete.
    *   *Recommended Action:* Ensure roadmap reflects parser capability advancement.

## 2. Active Work

*   **Tauri Rust Migration (TAURI-RUST-MIGRATION-1)**
    *   *Source:* `apps/desktop-tauri/README.md`, `docs/architecture/TAURI_RUST_MIGRATION_1.md`
    *   *Evidence:* Porting of features from Electron to Rust/Tauri.
    *   *Status:* In progress.
    *   *Recommended Action:* Continue migration according to milestones.

*   **Hierarchy Authoring (WD2-F)**
    *   *Source:* `docs/WRL_FORGE_ROADMAP.md`
    *   *Evidence:* Listed under "Next".
    *   *Status:* Active.
    *   *Recommended Action:* None.

## 3. Blocked Work

*   **macOS Tauri Build Support**
    *   *Source:* `apps/desktop-tauri/README.md`
    *   *Evidence:* Mentions "macOS 15+ is a target but has not been built yet."
    *   *Status:* Blocked (likely needs setup/hardware/CI).
    *   *Recommended Action:* Set up macOS CI environment for Tauri build.

## 4. Future Work

*   **Protected-preview Script diagnostics (SEC-SCRIPT-1)**
    *   *Source:* `docs/WRL_FORGE_ROADMAP.md`
    *   *Evidence:* Listed under "Next".
    *   *Status:* Future.
    *   *Recommended Action:* None.

*   **Appearance (WD2-G), Behavior (WD2-H), Animation (WD2-I), PROTO (WD2-J), Advanced Source Tools (WD2-K)**
    *   *Source:* `docs/WRL_FORGE_ROADMAP.md`
    *   *Evidence:* Listed under "Later".
    *   *Status:* Future.
    *   *Recommended Action:* None.

## 5. Outdated Roadmap Entries & Documentation

*   **`docs/PRODUCT_VISION.md` - Visual Features Status**
    *   *Source:* `docs/PRODUCT_VISION.md`
    *   *Evidence:* States "Viewport picking and 3D transform gizmos are the next planned visual features."
    *   *Status:* Outdated. These features are complete and tested in the Rust/Tauri application.
    *   *Recommended Action:* Update `PRODUCT_VISION.md` to reflect that viewport picking, translation, rotation, and scaling are already completed.

*   **`docs/WRL_FORGE_ROADMAP.md` - Workspace and Transforms**
    *   *Source:* `docs/WRL_FORGE_ROADMAP.md`
    *   *Evidence:* Lists `UI-C0` / `UI-1` (Docking) and `WD2-E` (Transform authoring) under "Next".
    *   *Status:* Outdated. Both have been implemented (UI-DOCK-1, VISUAL-4B).
    *   *Recommended Action:* Move these entries to the "Completed history" section.

## 6. Conflicting Windows and Mobile Tasks

*   **Windows Target Conflict**
    *   *Source:* `docs/PRODUCT_VISION.md`, `apps/desktop-tauri/README.md` vs `apps/desktop-tauri/src-tauri/tauri.conf.json`, `apps/desktop-tauri/ui/src/about.rs`
    *   *Evidence:* The vision and Tauri README explicitly state "Windows and mobile are not supported targets" and "Windows and mobile are out of scope". However, `apps/desktop-tauri/src-tauri/tauri.conf.json` contains `"windows": ["main"]`, and `apps/desktop-tauri/ui/src/about.rs` says "1.5.0 is reserved for the complete Rust release on Linux, Windows and macOS." Legacy Electron `package.json` also has `build:win`.
    *   *Status:* Conflicting.
    *   *Recommended Action:* Remove Windows specific code/configs from the Tauri app and update the About text.

## 7. Recommended Tasks for Jules (Grouped by Dependency & Priority)

### High Priority
*   **Task 1: Documentation Alignment (Independent)**
    *   *Description:* Update `docs/PRODUCT_VISION.md` to mark viewport picking, transform gizmos, and extrusion as completed. Move `UI-C0` / `UI-1` (Docking) and `WD2-E` (Transform authoring) to "Completed history" in `docs/WRL_FORGE_ROADMAP.md`.
*   **Task 2: Windows Target Cleanup (Independent)**
    *   *Description:* Remove `"windows": ["main"]` capability from `apps/desktop-tauri/src-tauri/tauri.conf.json`. Correct the text in `apps/desktop-tauri/ui/src/about.rs` to reflect the approved Linux and macOS 15+ targets only, removing mention of Windows.

### Medium Priority
*   **Task 3: Legacy Electron Scripts Deprecation (Dependent on Tauri Migration)**
    *   *Description:* If Electron is fully replaced, remove `build:win` and `qa:windows` from `package.json` to prevent confusion about supported platforms.
