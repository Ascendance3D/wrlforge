# Linux visual smoke test coverage inventory

| Mode | Command | Evidence File | Dependency | Status |
|---|---|---|---|---|
| launch | `./smoke.sh --headless file.wrl...` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| New World | `./smoke.sh --headless --create` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| picking | `./smoke.sh --headless --pick` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh`, `Node.js`, `xdotool` | test exists but not run in this audit |
| Move | `./smoke.sh --headless --move` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh`, `xdotool` | test exists but not run in this audit |
| Rotate/Scale | `./smoke.sh --headless --rotscale` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh`, `xdotool` | test exists but not run in this audit |
| Extrusion | `./smoke.sh --headless --extrusion` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| texture (smoke.sh) | `./smoke.sh --headless --theme tokyo-night-light file.wrl...` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| texture (direct) | `./target/debug/wrl-forge --smoke-texture ...` | `report.json` | `smoke-texture-plan.py`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| docking | `./smoke.sh --headless --dock` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh`, `xdotool` | test exists but not run in this audit |
| recovery | `./smoke.sh --headless --recovery` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
| About/menu | `./smoke.sh --headless --menu` | `report.json` | `smoke.sh`, `xvfb-run`, `isolated-run.sh`, `display-guard.sh` | test exists but not run in this audit |
