#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Run WRL Forge (the debug binary or the packaged AppImage) with EVERY
# per-user store in a disposable profile, and prove it.
#
# Usage: ./isolated-run.sh --root /tmp/DIR [--owner-display] [--no-data-check] -- APP [ARGS...]
#        ./isolated-run.sh --self-test
#
# Before the app starts, it is refused unless:
#   * ROOT is a fresh or own directory under /tmp (never inside the real home);
#   * HOME, XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME, XDG_STATE_HOME and
#     XDG_RUNTIME_DIR are set to directories inside ROOT (this script sets them);
#   * the app talks to a private X server only: DISPLAY is served by a running
#     Xvfb (unless --owner-display, for a person running the smoke by hand),
#     WAYLAND_DISPLAY is unset and GDK_BACKEND=x11, and the owner's D-Bus
#     session is not inherited.
# Tauri resolves its config dir (settings.json), data dir (WebKitGTK
# localstorage, caches, recovery) and cache dir from these XDG variables, so
# they all land in ROOT.
# After the start, the child's real environment (/proc/PID/environ) is checked.
# After the exit, a METADATA snapshot (path, size, mtime; never contents) of
# the real profile's app directories must be unchanged, and ROOT must hold the
# app's data directory (the proof that storage resolved inside it).
set -uo pipefail
ID=org.ascendance3d.wrlforge.tauri
OLD_ID=wrl-forge

real_home() { getent passwd "$(id -u)" | cut -d: -f6; }

# Messages never show the real home path.
refuse() { local m="$*"; echo "isolated-run: REFUSED: ${m//$(real_home)/\~}" >&2; exit 3; }

# The real profile's app directories (they may not exist).
real_dirs() {
  local h; h="$(real_home)"
  printf '%s\n' "$h/.config/$ID" "$h/.local/share/$ID" "$h/.cache/$ID" "$h/.local/state/$ID" \
    "$h/.config/$OLD_ID" "$h/.local/share/$OLD_ID" "$h/.cache/$OLD_ID"
}

# Path, size and mtime of every entry; contents are never read.
snapshot() {
  local d
  while read -r d; do
    if [[ -e "$d" ]]; then find "$d" -printf '%p %s %T@\n' 2>/dev/null | LC_ALL=C sort
    else echo "$d absent"; fi
  done < <(real_dirs)
}

# Validate the environment the app WILL get (the current one, after setup).
check_env() {
  local root="$1" owner_display="$2" h v p
  h="$(real_home)"
  case "$root/" in /tmp/*) ;; *) refuse "root '$root' is not under /tmp";; esac
  case "$root/" in "$h"/*) refuse "root is inside the real home";; esac
  for v in HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME XDG_RUNTIME_DIR; do
    p="${!v:-}"
    [[ -n "$p" ]] || refuse "$v is not set"
    p="$(realpath -m -- "$p")"
    case "$p/" in "$root"/*) ;; *) refuse "$v is outside the disposable root";; esac
    [[ -d "$p" ]] || refuse "$v does not exist"
  done
  [[ -z "${WAYLAND_DISPLAY:-}" ]] || refuse "WAYLAND_DISPLAY is set (the owner's compositor)"
  [[ "${GDK_BACKEND:-}" == x11 ]] || refuse "GDK_BACKEND is not x11"
  [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]] || refuse "the owner's D-Bus session is inherited"
  [[ -n "${DISPLAY:-}" ]] || refuse "DISPLAY is not set"
  if [[ "$owner_display" != 1 ]]; then
    local n="${DISPLAY#*:}"; n="${n%%.*}"
    [[ "$n" =~ ^[0-9]+$ ]] || refuse "DISPLAY '$DISPLAY' is not a local X display"
    local found=0 c
    for c in /proc/[0-9]*/cmdline; do
      tr '\0' ' ' <"$c" 2>/dev/null | awk -v d=":$n" '$1 ~ /(^|\/)Xvfb$/ { for (i = 2; i <= NF; i++) if ($i == d) f = 1 } END { exit !f }' && { found=1; break; }
    done
    [[ $found -eq 1 ]] || refuse "DISPLAY :$n is not a private Xvfb"
  fi
}

# Check the started child's environment and wait for it.
check_child() {
  local pid="$1" root="$2" v want got
  local envf="/proc/$pid/environ"
  for _ in 1 2 3 4 5 6 7 8 9 10; do [[ -r "$envf" ]] && break; sleep 0.05; done
  [[ -r "$envf" ]] || return 0 # Already gone: the exit check still runs.
  for v in HOME XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME XDG_RUNTIME_DIR GDK_BACKEND; do
    want="${!v}"
    got="$(tr '\0' '\n' <"$envf" 2>/dev/null | sed -n "s/^$v=//p" | head -1)"
    if [[ -n "$got" && "$got" != "$want" ]]; then
      kill -KILL "$pid" 2>/dev/null
      refuse "the child's $v differs from the isolated value"
    fi
  done
  if tr '\0' '\n' <"$envf" 2>/dev/null | awk -F= '$1 == "WAYLAND_DISPLAY" || $1 == "DBUS_SESSION_BUS_ADDRESS" { f = 1 } END { exit !f }'; then
    kill -KILL "$pid" 2>/dev/null
    refuse "the child inherited the owner's Wayland or D-Bus session"
  fi
}

run() {
  local root="" owner_display=0 data_check=1
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --root) root="$2"; shift 2;;
      --owner-display) owner_display=1; shift;;
      --no-data-check) data_check=0; shift;;
      --) shift; break;;
      *) refuse "unknown option $1";;
    esac
  done
  [[ -n "$root" ]] || refuse "no --root"
  [[ $# -gt 0 ]] || refuse "no app command"
  case "$root/" in /tmp/*) ;; *) refuse "root '$root' is not under /tmp";; esac
  mkdir -p -- "$root" || refuse "cannot create the root"
  root="$(realpath -- "$root")"
  [[ -O "$root" ]] || refuse "the root is not owned by this user"
  local d
  for d in home config data cache state run; do mkdir -p -- "$root/$d"; done
  chmod 700 "$root/run"
  export HOME="$root/home" XDG_CONFIG_HOME="$root/config" XDG_DATA_HOME="$root/data" \
    XDG_CACHE_HOME="$root/cache" XDG_STATE_HOME="$root/state" XDG_RUNTIME_DIR="$root/run" \
    GDK_BACKEND=x11
  unset WAYLAND_DISPLAY DBUS_SESSION_BUS_ADDRESS
  check_env "$root" "$owner_display"
  local before; before="$(snapshot)"
  "$@" &
  local pid=$!
  # Forward a stop request to the app (smoke.sh kills its crash run).
  trap 'kill -TERM "$pid" 2>/dev/null' TERM INT
  check_child "$pid" "$root"
  wait "$pid"; local code=$?
  while kill -0 "$pid" 2>/dev/null; do wait "$pid"; code=$?; done
  local after; after="$(snapshot)"
  if [[ "$before" != "$after" ]]; then
    echo "isolated-run: REAL PROFILE CHANGED during the run:" >&2
    diff <(echo "$before") <(echo "$after") | sed -n 's/^[<>] //p' | awk '{print "  " $1}' | sed "s|$(real_home)|~|" | LC_ALL=C sort -u >&2
    exit 4
  fi
  if [[ $data_check -eq 1 && ! -d "$XDG_DATA_HOME/$ID" ]]; then
    echo "isolated-run: the app's data dir did not resolve inside the root" >&2
    exit 5
  fi
  exit "$code"
}

# Each unsafe configuration is refused BEFORE the command runs; a safe one
# runs and its child sees the isolated environment.
self_test() {
  local t; t="$(mktemp -d /tmp/wrlforge-isolation-test.XXXXXX)"
  local fails=0 n=0 me="$0"
  expect_refused() {
    local name="$1"; shift
    n=$((n + 1))
    rm -f "$t/ran"
    if "$@" >"$t/out" 2>&1; then echo "FAIL $name: accepted"; fails=$((fails + 1));
    elif [[ -e "$t/ran" ]]; then echo "FAIL $name: the app command ran"; fails=$((fails + 1));
    elif ! grep -q 'REFUSED' "$t/out"; then echo "FAIL $name: $(head -1 "$t/out")"; fails=$((fails + 1));
    else echo "ok   $name: $(sed -n 's/^isolated-run: REFUSED: //p' "$t/out" | head -1)"; fi
  }
  local app=(touch "$t/ran")
  expect_refused "root outside /tmp" "$me" --root "$(real_home)/wrlforge-isolation-never" -- "${app[@]}"
  expect_refused "no root" "$me" -- "${app[@]}"
  expect_refused "owner display :0" env DISPLAY=:0 "$me" --root "$t/a" --no-data-check -- "${app[@]}"
  expect_refused "display that is no Xvfb" env DISPLAY=:7777 "$me" --root "$t/b" --no-data-check -- "${app[@]}"
  expect_refused "no display" env -u DISPLAY "$me" --root "$t/c" --no-data-check -- "${app[@]}"
  # check_env on its own: a real HOME / a missing XDG dir / Wayland left set.
  expect_refused "HOME is the real home" env HOME="$(real_home)" XDG_CONFIG_HOME="$t/d/config" XDG_DATA_HOME="$t/d/data" \
    XDG_CACHE_HOME="$t/d/cache" XDG_STATE_HOME="$t/d/state" XDG_RUNTIME_DIR="$t/d/run" GDK_BACKEND=x11 \
    bash -c "mkdir -p $t/d/{config,data,cache,state,run}; source '$me' --source-only; check_env '$t/d' 1; touch '$t/ran'"
  expect_refused "XDG_DATA_HOME unset" env -u XDG_DATA_HOME HOME="$t/e/home" XDG_CONFIG_HOME="$t/e/config" \
    XDG_CACHE_HOME="$t/e/cache" XDG_STATE_HOME="$t/e/state" XDG_RUNTIME_DIR="$t/e/run" GDK_BACKEND=x11 \
    bash -c "mkdir -p $t/e/{home,config,cache,state,run}; source '$me' --source-only; check_env '$t/e' 1; touch '$t/ran'"
  expect_refused "Wayland left set" env WAYLAND_DISPLAY=wayland-0 HOME="$t/f/home" XDG_CONFIG_HOME="$t/f/config" XDG_DATA_HOME="$t/f/data" \
    XDG_CACHE_HOME="$t/f/cache" XDG_STATE_HOME="$t/f/state" XDG_RUNTIME_DIR="$t/f/run" GDK_BACKEND=x11 \
    bash -c "mkdir -p $t/f/{home,config,data,cache,state,run}; source '$me' --source-only; check_env '$t/f' 1; touch '$t/ran'"
  # Safe: under a private Xvfb, the child sees only the isolated values.
  n=$((n + 1))
  if xvfb-run -a -s "-screen 0 640x480x24" env WAYLAND_DISPLAY=wayland-0 DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent \
      "$me" --root "$t/ok" --no-data-check -- sh -c 'env > "$1"' sh "$t/child-env" >"$t/out" 2>&1 \
     && grep -qx "HOME=$t/ok/home" "$t/child-env" && grep -qx "XDG_DATA_HOME=$t/ok/data" "$t/child-env" \
     && grep -qx "XDG_CONFIG_HOME=$t/ok/config" "$t/child-env" && grep -qx "GDK_BACKEND=x11" "$t/child-env" \
     && ! grep -q '^WAYLAND_DISPLAY=' "$t/child-env" && ! grep -q '^DBUS_SESSION_BUS_ADDRESS=' "$t/child-env"; then
    echo "ok   safe profile: accepted; the child sees only the disposable profile"
  else
    echo "FAIL safe profile: $(head -2 "$t/out")"; fails=$((fails + 1))
  fi
  rm -rf -- "$t"
  echo "ISOLATION SELF-TEST pass=$((n - fails)) fail=$fails (want $n)"
  [[ $fails -eq 0 ]]
}

[[ "${1:-}" == --source-only ]] && return 0 2>/dev/null
if [[ "${1:-}" == --self-test ]]; then self_test; exit $?; fi
run "$@"
