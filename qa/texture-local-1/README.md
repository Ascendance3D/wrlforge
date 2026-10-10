# TEXTURE-LOCAL-1 — QA evidence

Local `ImageTexture` images load in the packaged Tauri app (X_ITE preview).

Fixture generator: `apps/desktop-tauri/smoke-texture-plan.py` (16 fixtures,
solid known colors). In-app run: `--smoke-texture <dir>` samples the RENDERED
viewport pixels, checks the Rust texture-warning count, checks the source is
unchanged, and stores a PNG capture of each frame. Rust then checks every
fixture file is byte-identical and that no file outside a document folder
was served. Runner: `tools/run.sh BIN OUTDIR`; container: `tools/deb-in-container.sh`.

| run | result |
|---|---|
| debug binary, 6 parallel Xvfb runs | 6/6 pass, 27/27 steps each (`results/debug-6x/`) |
| AppImage, build host | 27/27 (`results/appimage-host/`, captures) |
| `.deb` installed, clean Ubuntu 24.04 container (no Rust/Node/repo) | open plain, open gzip, create, pick, texture: all pass (`results/deb-container/`) |
| AppImage, same container (extract-and-run, no FUSE) | open plain, open gzip, create, pick, texture: all pass (`results/appimage-container/`) |
| `cargo test -p wrl-forge-desktop` | 55 passed |

Cases: JPG same folder; gzip `.wrl` + JPG; PNG; GIF; child folder; spaces;
fallback (first wins); missing first → second; DEF/USE; missing; invalid
bytes; traversal (`../`, `%2e%2e`, `..%2f`, `sub/../../`); symlink file and
folder escape; http/https/file:/absolute; document switch with a request held
in flight (reply refused, `stale`); save → close → reopen → reload.

X_ITE 15.1.10 defect found and worked around (`preview-adapter.js`): a texture
finishing after the parse could leave `createX3DFromString` pending forever
(FileLoader held only by a WeakRef). Reproduced with `data:` textures too;
before the fix ~1 stall per 15 loads under parallel load; after, 0 in 12 runs.

Packages: `SHA256SUMS.txt`. Packages are NOT published (owner instruction).
