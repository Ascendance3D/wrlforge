#!/usr/bin/env bash
# TEXTURE-LOCAL-1: install + test + uninstall in a clean Ubuntu 24.04
# container (no Rust, no Node, no repository). Mounted at /tmp/t.
set -uo pipefail
T=/tmp/t; O="$T/out/deb"; mkdir -p "$O"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq >/dev/null
echo "== toolchain on the test system"
for t in cargo rustc rustup node npm; do command -v "$t" || echo "$t: absent"; done
echo "== install the .deb"
apt-get install -y -qq "$T/pkg/wrl-forge_0.1.0_amd64.deb" > "$O/apt-install.log" 2>&1; echo "apt install exit=$?"
dpkg -s wrl-forge | sed -n '/^Package/p;/^Status/p;/^Version/p'
apt-get install -y -qq xvfb xauth xdotool > /dev/null 2>&1; echo "test tools (xvfb, xdotool) exit=$?"
echo "== tests: installed /usr/bin/wrl-forge"
"$T/run.sh" /usr/bin/wrl-forge "$O/runs"; echo "deb tests exit=$?"
echo "== AppImage on the same clean system (extract-and-run, no FUSE)"
"$T/run.sh" "$T/appimage-wrap.sh" "$T/out/appimage/runs"; echo "appimage tests exit=$?"
echo "== uninstall"
apt-get remove -y -qq wrl-forge > "$O/apt-remove.log" 2>&1; echo "apt remove exit=$?"
ls /usr/bin/wrl-forge 2>&1
chown -R "${HOST_UID:-1000}:${HOST_UID:-1000}" "$T/out"
