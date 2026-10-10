# PKG-LINUX-1 — Linux packages: build and test evidence

Lane: `feature/pkg-linux-1`, based on PR #132 head
`cbc7312da31af14f192ace2a30f8c2e5929f46ab` (`feature/native-render-1`).
Date: 2026-10-10. Self-tested by the packaging agent; the owner waived an
independent QA round for this lane.

## Packages

| file (as staged for test) | bundler output name | bytes | SHA-256 |
|---|---|---|---|
| `wrl-forge_0.1.0_amd64.deb` | `WRL Forge_0.1.0_amd64.deb` | 11,481,848 | `0c53c05b5f1dd9c707d2dccb1ceeb74c79e0df6a9b946efd174086de5028c843` |
| `WRL-Forge_0.1.0_amd64.AppImage` | `WRL Forge_0.1.0_amd64.AppImage` | 92,441,080 | `e94d380e8a7beb9335aac6891167066c24c062c2e562dffdb7c8404af706076e` |

The files were renamed only to remove the space for testing; the bytes are
identical (`SHA256SUMS.txt`). Identity is unchanged: product `WRL Forge`,
version `0.1.0`, identifier `org.ascendance3d.wrlforge.tauri`, Debian package
`wrl-forge`, binary `/usr/bin/wrl-forge`. The packages are not published.

## Build environment

| item | value |
|---|---|
| OS | Ubuntu 24.04.5 LTS, x86_64, kernel 7.0.0 |
| Rust | 1.95.0 (stable), targets `x86_64-unknown-linux-gnu`, `wasm32-unknown-unknown` |
| tauri / tauri-cli | 2.12.2 / 2.12.1 (wry 0.57.0, tao 0.37.1) |
| tauri-bundler AppImage tool | linuxdeploy `07333c6` (downloaded by tauri-cli) |
| Leptos / wgpu | 0.8.22 / 30.0.1 |
| wasm-bindgen-cli | 0.2.129 |
| X_ITE | 15.1.10 (copied from the pinned `node_modules`; Node is not run) |
| WebKitGTK / GTK (build host) | 2.52.6 / 3.24.41 |

## Build commands

```sh
cargo install tauri-cli --version '^2' --locked
cd apps/desktop-tauri
./packaging/package-linux.sh
```

`package-linux.sh` runs `build-ui.sh` (release Wasm UI + X_ITE into `dist/`),
stages the license texts, then `cargo tauri build --bundles deb,appimage`.
It sets `--remap-path-prefix` (no builder paths in the binaries; the script
fails if one is found) and thin LTO with one codegen unit per core (a cold
build took 3 min 50 s on 24 cores; the workspace profile's fat LTO with one
codegen unit ran the final link on one core).

## What the packages contain

- `.deb`: `/usr/bin/wrl-forge` (UI, X_ITE and adapters are embedded in the
  binary), `/usr/share/applications/WRL Forge.desktop`, hicolor icons
  32/128/256, `/usr/lib/WRL Forge/licenses/` (GPL-3.0 `LICENSE`,
  `COPYRIGHT.md`, `THIRD_PARTY_NOTICES.md`, `X_ITE-LICENSE.md`,
  `LICENSE-tokyo-night.txt`, `RUST-CRATES-LICENSES.txt` with 426 crates,
  `README-licenses.txt`) and `/usr/share/doc/wrl-forge/copyright`.
- AppImage: the same files plus the bundled GTK/WebKitGTK/GStreamer libraries
  and their 128 Debian `copyright` files under `usr/share/doc/`.
- Private path scan (`/home/<user>`, `Projects/cybertown`, `.worktrees`) over
  every file of the extracted `.deb` and AppImage: **0 files**.

## Runtime dependencies

`.deb` control: `Depends: libwebkit2gtk-4.1-0, libgtk-3-0`;
`Recommends: libvulkan1, mesa-vulkan-drivers`. Direct `NEEDED` libraries of
the binary: libwebkit2gtk-4.1, libjavascriptcoregtk-4.1, libsoup-3.0, libgtk-3,
libgdk-3, libgdk_pixbuf-2.0, libcairo, libgio/libgobject/libglib-2.0,
libdbus-1, libwayland-client, libgcc_s, libc, libm. Loaded at run time only by
the experimental native viewport: libvulkan.so.1 and a Vulkan driver, libX11 /
libxcb. On the clean container, apt resolved every dependency from Ubuntu
24.04 and `ldd /usr/bin/wrl-forge` reported no missing library.

AppImage: carries its libraries; needs `libfuse2` to mount, or
`--appimage-extract-and-run` (used in the container test).

## Tests

All runs use temporary copies under `/tmp` and a temporary `--config-dir`.
No owner VRML file was opened or changed. Inputs: `inputs/pkg-test-basic.wrl`
(written for this lane) and its gzip copy, the New World created by the app,
and the 34 generated picking fixtures of `smoke-pick-plan.cjs`. Runner:
`tools/run.sh BIN OUTDIR` (as run; it reads `inputs/` and `plan/` beside
itself). Display: Xvfb 1600×1000, software rendering.

| run | `.deb` installed, clean Ubuntu 24.04 container | AppImage, same container (no FUSE) | AppImage, build host |
|---|---|---|---|
| open plain `.wrl` + Inspector (61 checks) | pass | pass | pass |
| open gzip `.wrl` + Inspector (61 checks) | pass | pass | pass |
| create (51 checks) | pass | pass | pass |
| pick, 34 fixtures, 140 clicks (127 checks) | pass | pass | pass |

The container had no `cargo`, `rustc`, `rustup`, `node` or `npm`, and no
repository files: only the package and the test inputs were mounted.

Required operations, and the checks that prove them (`results/*/`):

| # | operation | proof |
|---|---|---|
| 1 | start | every run; create: "startup: no document" |
| 2 | open a `.wrl` | open: "startup document opened through Rust" (plain and gzip) |
| 3 | source editor | open: "editor shows exactly the Rust view projection" |
| 4 | X_ITE scene | open: "WebView WebGL probe … X_ITE loaded", "X_ITE preview loaded the unsaved buffer"; create: "viewport renders the new geometry" |
| 5 | create a basic object | create: Box, Sphere, Cone, Cylinder |
| 6 | select from Scene Tree / viewport | open + create: Scene Tree selection; pick: 140 viewport clicks |
| 7 | edit a field | open: Inspector `diffuseColor` Apply; create: translation and diffuseColor through the controls |
| 8 | save | open: "Save button wrote through Rust" + one backup; create: Save As, then Ctrl+S with backup |
| 9 | close and reopen | create: "Close: no document…", "Reopen: same source, Scene Tree and rendered object" |
| 10 | saved source is correct | open: "saved file decodes to exactly the expected source" (plain and gzip); create: "saved world is exactly New World + Box (edited) + Sphere + Cone + Cylinder", "parses as VRML97 with no diagnostics" |

Native renderer (packaged binary from the `.deb`, build host):

- `native-smoke/harness.py xvfb` with `--smoke-native`: **46/46 checks pass**,
  including the GPU/CPU pick agreement checks (`results/native-selftest/`).
- Real launch path, KWin `--virtual` at scale 1.5, no smoke or native flag
  (`tools/settings_launch.py` from NATIVE-RENDER-1): `native-experimental` →
  native render threads start; no settings file → X_ITE only; `"native"` →
  X_ITE only (`results/settings-launch/`). X_ITE stays the default.

Install / uninstall (container, `results/deb-container/container-output.txt`):
`apt-get install ./wrl-forge_0.1.0_amd64.deb` exit 0; `apt-get remove
wrl-forge` exit 0, and the binary, `/usr/lib/WRL Forge` and the desktop entry
are gone. The `.deb` was not installed on the build host.

Workspace checks on this branch: `cargo test --offline --workspace` 106
passed, 0 failed. `cargo fmt --all --check`: 231 differences, the same
pre-existing NATIVE-RENDER-1 set; this lane changes no Rust source.

## Not tested / known limits

- A real desktop session (GNOME, Plasma, Wayland compositor with a GPU) was
  not run with these packages; all GUI runs used Xvfb, except the settings
  launch on KWin `--virtual`.
- Other distributions than Ubuntu 24.04 were not tried.
- KWin scale-1.5 watchdog finding (NATIVE-RENDER-1 D14) stays open; it applies
  to the experimental native viewport only.
- 38 of 426 Rust crates publish no license file; `RUST-CRATES-LICENSES.txt`
  lists their SPDX license and authors.
- The packages are unsigned. Bundler file names contain a space.
- The `.desktop` entry has no `MimeType` (no `.wrl` file association).
