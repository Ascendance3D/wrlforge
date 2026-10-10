#!/usr/bin/env bash
# PKG-LINUX-1: install + test + uninstall in a clean Ubuntu 24.04 container.
# Mounted at /tmp/t (the smoke modes only accept /tmp paths).
set -uo pipefail
T=/tmp/t; O="$T/out-final/deb"; mkdir -p "$O"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null
echo "== toolchain on the test system"
for t in cargo rustc rustup node npm; do command -v "$t" || echo "$t: absent"; done
echo "== install the .deb"
apt-get install -y -qq "$T/pkg/wrl-forge_0.1.0_amd64.deb" > "$O/apt-install.log" 2>&1; echo "apt install exit=$?"
dpkg -s wrl-forge | sed -n '/^Package/p;/^Status/p;/^Version/p;/^Depends/p;/^Recommends/p'
dpkg -L wrl-forge
echo "== ldd: unresolved libraries"; ldd /usr/bin/wrl-forge | sed -n '/not found/p'; echo "(end)"
echo "== desktop entry"; cat "/usr/share/applications/WRL Forge.desktop"
apt-get install -y -qq xvfb xauth xdotool > /dev/null 2>&1; echo "test tools (xvfb, xdotool) exit=$?"
echo "== tests: installed /usr/bin/wrl-forge"
"$T/run.sh" /usr/bin/wrl-forge "$O/runs"; echo "deb tests exit=$?"
echo "== AppImage on the same clean system (extract-and-run, no FUSE)"
"$T/run.sh" "$T/appimage-wrap.sh" "$T/out-final/appimage-container/runs"; echo "appimage tests exit=$?"
echo "== uninstall"
apt-get remove -y -qq wrl-forge > "$O/apt-remove.log" 2>&1; echo "apt remove exit=$?"
dpkg -s wrl-forge 2>&1 | head -n 1
ls /usr/bin/wrl-forge "/usr/lib/WRL Forge" "/usr/share/applications/WRL Forge.desktop" 2>&1
chown -R "${HOST_UID:-1000}:${HOST_UID:-1000}" "$T/out-final"
