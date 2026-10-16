# Linux visual smoke test coverage inventory

Run every command from `apps/desktop-tauri/` after building the debug binary (`target/debug/wrl-forge`). This inventory lists tests that exist. It records no run results.

| Mode | Command | Evidence File | Dependency | Status |
|---|---|---|---|---|
| launch | `./smoke.sh --headless file.wrl...` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| New World | `./smoke.sh --headless --create` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| picking | `./smoke.sh --headless --pick` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh`, `Node.js`, `xdotool` | test exists but not run in this audit |
| keys | `./smoke.sh --headless --keys` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh`, `Node.js`, `xdotool` | test exists but not run in this audit |
| Move | `./smoke.sh --headless --move` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh`, `xdotool` | test exists but not run in this audit |
| Rotate/Scale | `./smoke.sh --headless --rotscale` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh`, `xdotool` | test exists but not run in this audit |
| Extrusion | `./smoke.sh --headless --extrusion` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| Theme (smoke.sh) | `./smoke.sh --headless --theme tokyo-night-light file.wrl...` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| Texture (direct) | `python3 smoke-texture-plan.py <temp-dir>` && `env WRLFORGE_SMOKE_XVFB=1 xvfb-run -a -s "-screen 0 1600x1000x24" ./isolated-run.sh --root <temp-profile> -- ./target/debug/wrl-forge --smoke-texture <temp-dir> --smoke-report <report.json> --config-dir <temp-config>` | `<report.json>` | `smoke-texture-plan.py`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| docking | `./smoke.sh --headless --dock` | `main.json`, `restore.json`, `damaged.json` (in temp run dir) | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh`, `xdotool` | test exists but not run in this audit |
| recovery | `./smoke.sh --headless --recovery` | `ready.json`, `restore.json`, `later.json` (in temp run dir) | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| About/menu | `./smoke.sh --headless --menu` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh`, `xdotool` | test exists but not run in this audit |
