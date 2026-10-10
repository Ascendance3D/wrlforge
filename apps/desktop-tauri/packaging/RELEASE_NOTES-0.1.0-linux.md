# WRL Forge 0.1.0 (Tauri/Rust) — Linux test packages

Status: **internal test build. Not a public release.**

Packages: `wrl-forge_0.1.0_amd64.deb` and `WRL Forge_0.1.0_amd64.AppImage`
(x86_64). Built on Ubuntu 24.04. Windows, macOS and mobile packages are not
part of this build.

## What is in it

The Tauri 2 / Rust desktop application from TAURI-RUST-MIGRATION-1 (#131) and
NATIVE-RENDER-1 (#132): source editor, Scene Tree, field Inspector, Create
(New World → basic objects), viewport picking, the translation gizmo and the
X_ITE 15.1.10 preview. The UI and X_ITE are embedded in the binary; the
installed application needs no repository files, Node.js or Rust toolchain.

## Renderer

- **X_ITE is the default viewport.**
- The native wgpu viewport is **experimental, hidden and off**. It is used only
  with the `--native-viewport` flag or with
  `"viewport": {"renderer": "native-experimental"}` in `settings.json`. Any
  other value selects X_ITE. It does selection only (GPU and CPU picks must
  agree); there is no native viewport editing.

## Runtime requirements

The `.deb` declares `libwebkit2gtk-4.1-0` and `libgtk-3-0` (apt installs their
dependencies) and recommends `libvulkan1` and `mesa-vulkan-drivers` (only the
experimental native viewport uses Vulkan). The AppImage carries WebKitGTK and
GTK and needs a FUSE-capable system (`libfuse2`) or
`--appimage-extract-and-run`.

## Known limits

- **KWin fractional scale 1.5 watchdog (NATIVE-RENDER-1, D14):** one run of the
  native viewport at KWin output scale 1.5 failed the UI watchdog gate (p99
  77 ms; gate p99 < 50 ms, max < 100 ms). Later runs passed, and the X_ITE-only
  control shows similar delay. The cause is unknown and is not fixed. This
  affects only the experimental native viewport.
- Tested headless (Xvfb, software rendering). A real desktop session (GNOME,
  Plasma, Wayland) was not tested with these packages.
- Package file names come from the product name and contain a space
  (`WRL Forge_0.1.0_amd64.*`).
- The packages are not signed.
- 38 of 426 third-party Rust crates publish no license file; the license list
  gives their SPDX license and authors.
