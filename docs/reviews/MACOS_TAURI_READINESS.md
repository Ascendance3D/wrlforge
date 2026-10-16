# macOS 15+ Tauri Application Build Readiness Review

## Executive Summary

WRL Forge's transition to a pure Rust / Tauri 2 desktop architecture (`apps/desktop-tauri`) has established complete macOS codebase readiness at the source, dependency, and configuration levels.

The application compiles native Cocoa / Quartz Core bindings via `objc2`, renders native 3D viewports through `wgpu` with Metal support (`CAMetalLayer`), and implements native macOS menu bar structures (`UI-MENU-1`). Cross-compilation and type-checking pass on GitHub Actions `macos-latest` runners in continuous integration.

However, full release readiness on macOS 15+ (Sequoia) requires runtime validation on real Apple Silicon hardware running macOS 15+, as well as dedicated Tauri packaging scripts and signing pipeline integration.

---

## Technical Inspection Breakdown

### 1. Source Code & OS Bindings
- **Native Viewport (`src/viewport/mac.rs`)**: Implements `CAMetalLayer` sublayer creation and embedding inside Tauri's `NSWindow` / `NSView` hierarchy. Uses `CATransaction` flushing and tracks window occlusion via `NSWindow.occlusionState` to pause rendering when obscured.
- **Native Menu Bar (`src/app_menu.rs`, `src/lib.rs`)**: Implements standard macOS Application Menu (`WRL Forge` -> About, Services, Hide, Show All, Quit) and standard File/Edit menus. Integrates with Tauri event loop for `menu-command` IPC events.
- **Wasm & Webview UI (`apps/desktop-tauri/ui`)**: Leptos UI compiles to WebAssembly and runs inside WebKit (`WKWebView` on macOS). Local texture loading uses custom `wrlres://` scheme handlers (`src/resources.rs`).

### 2. Dependencies (`Cargo.toml`)
- **Metal Graphics Backend**: `wgpu` (v30.0.1) explicitly enables the `metal` feature flag (`apps/desktop-tauri/src-tauri/Cargo.toml`).
- **macOS System Framework Bindings**: `[target.'cfg(target_os = "macos")'.dependencies]` references:
  - `objc2` (=0.6.5)
  - `objc2-foundation` (=0.3.2)
  - `objc2-app-kit` (=0.3.2)
  - `objc2-quartz-core` (=0.3.2)
  - `objc2-core-foundation` (=0.3.2)

### 3. Packaging Configuration (`tauri.conf.json`)
- **Bundle Targets**: Configured with `"targets": ["deb", "appimage", "app", "dmg"]`.
- **Product Metadata**: Identifier (`org.ascendance3d.wrlforge.tauri`), product name (`WRL Forge`), icon set (`icons/32x32.png`, `128x128.png`, `256x256.png`, `icon.png`).
- **Missing macOS Specifics in Tauri Config**: `bundle.macOS` settings (such as explicit `entitlements` plist paths or `minimumSystemVersion: "15.0"`) are not yet declared in `tauri.conf.json` (though `assets/entitlements.mac.plist` exists in the repository for the Electron lane).

### 4. CI & Workflows (`.github/workflows/`)
- **`ci.yml`**: Includes `macos-latest` matrix job that executes `npm ci`, icon generation, editor bundling, and static checks (`npm run check`).
- **`release.yml`**: Currently handles macOS packaging via Electron (`npm run dist:mac` / `scripts/notarize-dmg.js`). **Tauri macOS release packaging is not yet hooked into `release.yml`**.

---

## What Works vs. What Requires Real macOS 15+ Testing

### What Works (Code-Complete & Static Verified)
1. **Rust / Cocoa Source Code**: All macOS-specific conditional code (`cfg(target_os = "macos")`) compiles without warnings or errors.
2. **`wgpu` Metal Pipeline**: `wgpu` Metal feature flag and `CAMetalLayer` layer management code compile and link against `objc2-quartz-core`.
3. **Application Menu Bar**: Objective-C menu tree setup (`app_menu.rs`) and menu command routing.
4. **CI Integration**: `ci.yml` validates non-GUI compilation and syntax on GitHub `macos-latest` runners.
5. **Icon Assets**: PNG icons suitable for `.app` and `.dmg` generation are generated deterministically.

### What Requires Real macOS 15+ Testing & Validation
1. **Runtime `CAMetalLayer` WebKit Sublayer Composition**:
   - Verifying `CAMetalLayer` rendering underneath/alongside `WKWebView` on macOS 15+.
   - Checking high-DPI Retina scaling (`backingScaleFactor`) and window resize handling.
   - Verifying `NSWindow.occlusionState` behavior when windows are minimized or hidden across spaces.
2. **Runtime Menu & Shortcut Dispatch**:
   - Testing native menu command execution and keyboard shortcuts (`Cmd+Q`, `Cmd+C`, `Cmd+V`, `Cmd+Z`, `Cmd+S`).
3. **Packaging & Signing Execution**:
   - Running `cargo tauri build --bundles app,dmg` on macOS 15+ to produce `.app` and `.dmg`.
   - Developer ID Application signing, Hardened Runtime entitlement verification, and Apple `notarytool` stapling on the resulting DMG/App.
4. **Local Resource Protocol (`wrlres://`)**:
   - Validating custom URI protocol handling for local textures under WebKit security policies in macOS 15.

---

## macOS 15+ Readiness Checklist

- [x] **Rust macOS Bindings**: `objc2` framework crates included in `Cargo.toml`.
- [x] **Metal Renderer Configuration**: `wgpu` configured with `metal` feature flag.
- [x] **Native Viewport Implementation**: `CAMetalLayer` integration in `src/viewport/mac.rs`.
- [x] **Native Application Menu**: macOS menu structure created in `src/app_menu.rs`.
- [x] **Tauri Targets**: `app` and `dmg` listed in `tauri.conf.json`.
- [x] **CI Compilation Check**: Matrix job running on `macos-latest` in `ci.yml`.
- [ ] **Tauri macOS Entitlements Config**: Add `bundle.macOS` settings to `tauri.conf.json`.
- [ ] **macOS Packaging Script**: Create `apps/desktop-tauri/packaging/package-mac.sh`.
- [ ] **Runtime Validation on macOS 15+ Hardware**: Test native viewport rendering, WebKit composition, and input/menu shortcuts.
- [ ] **Release Workflow Integration**: Update `.github/workflows/release.yml` for Tauri macOS builds.

---

## File References

- `apps/desktop-tauri/src-tauri/Cargo.toml` — macOS dependencies (`objc2`, `wgpu` Metal feature)
- `apps/desktop-tauri/src-tauri/src/viewport/mac.rs` — `CAMetalLayer` and `NSWindow` integration
- `apps/desktop-tauri/src-tauri/src/app_menu.rs` — Native macOS menu bar implementation
- `apps/desktop-tauri/src-tauri/tauri.conf.json` — Tauri 2 bundle targets (`app`, `dmg`)
- `apps/desktop-tauri/README.md` — Tauri desktop shell documentation
- `.github/workflows/ci.yml` — Continuous integration matrix with `macos-latest`
- `.github/workflows/release.yml` — Existing release workflow (Electron macOS signing pipeline reference)
- `assets/entitlements.mac.plist` — Hardened Runtime entitlements reference plist
