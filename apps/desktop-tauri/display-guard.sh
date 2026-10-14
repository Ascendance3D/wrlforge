#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# The one shared check that must pass before ANY screen read (a capture, a
# recording, a window query) or ANY synthetic mouse or keyboard input in a
# test. It proves that the current DISPLAY is a private Xvfb of this user,
# reached with that Xvfb's own cookie, from a test profile; otherwise it
# refuses. There is no bypass and no owner-display mode.
#
# Usage: ./display-guard.sh --check            (exit 0 and "display-guard: OK :N")
#        ./display-guard.sh --exec -- CMD ...  (check, then exec CMD; for manual
#                                               capture or input drivers)
#        ./display-guard.sh --self-test
#        ./display-guard.sh --cleanup-test         (interrupts --self-test)
#
# Refused (exit 3, "display-guard: REFUSED: ...") when:
#   * DISPLAY is unset, :0, or not a local ":N" display;
#   * no Xvfb process of this user serves ":N" with "-auth FILE", or the
#     display socket is missing or not owned by this user;
#   * XAUTHORITY is unset, is not that same FILE, has no cookie for ":N", or
#     the X server does not accept the cookie;
#   * the owner's GUI session is inherited: WAYLAND_DISPLAY or
#     DBUS_SESSION_BUS_ADDRESS is set, or HOME, an XDG directory or
#     XAUTHORITY is in the real home or the owner's /run/user directory.
# The live cookie check (xdpyinfo) only queries the server; it reads no
# pixels and sends no input.
set -uo pipefail

real_home() { getent passwd "$(id -u)" | cut -d: -f6; }

# Messages never show the real home path.
refuse() { local m="$*"; echo "display-guard: REFUSED: ${m//$(real_home)/\~}" >&2; exit 3; }

# The arguments of an Xvfb process of this user that serves ":$1", one per
# line; empty when there is none. pgrep only narrows the candidates (a
# process it misses is refused); each one is still checked below.
xvfb_args() {
  local want=":$1" p e
  for p in $(pgrep -u "$(id -u)" -x Xvfb | sed 's|^|/proc/|'); do
    [[ -O "$p" ]] || continue
    e="$(readlink "$p/exe" 2>/dev/null)" || continue
    [[ "${e##*/}" == Xvfb ]] || continue
    if tr '\0' '\n' <"$p/cmdline" 2>/dev/null | sed 1d | awk -v d="$want" '$0 == d { f = 1 } END { exit !f }'; then
      tr '\0' '\n' <"$p/cmdline" 2>/dev/null | sed 1d
      return 0
    fi
  done
}

# Refuse a path in the real home or in the owner's runtime directory.
not_owner_path() {
  local name="$1" p="$2" h
  h="$(real_home)"
  p="$(realpath -m -- "$p")"
  [[ "$p" != "$h" ]] || refuse "$name is the real home"
  case "$p/" in "$h"/*) refuse "$name is inside the real home";; esac
  case "$p/" in /run/user/*) refuse "$name is in the owner's runtime directory (/run/user)";; esac
}

check() {
  # 1. The display name.
  [[ -n "${DISPLAY:-}" ]] || refuse "DISPLAY is not set"
  [[ "$DISPLAY" =~ ^:([0-9]+)(\.[0-9]+)?$ ]] || refuse "DISPLAY '$DISPLAY' is not a local ':N' display"
  local n="${BASH_REMATCH[1]}"
  [[ "$n" != 0 ]] || refuse "DISPLAY :0 is the owner's display"

  # 2. No inherited owner GUI session; a test profile.
  [[ -z "${WAYLAND_DISPLAY:-}" ]] || refuse "WAYLAND_DISPLAY is set (the owner's compositor)"
  [[ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]] || refuse "the owner's D-Bus session is inherited"
  [[ -n "${HOME:-}" ]] || refuse "HOME is not set"
  not_owner_path HOME "$HOME"
  local v
  for v in XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME XDG_RUNTIME_DIR; do
    [[ -z "${!v:-}" ]] || not_owner_path "$v" "${!v}"
  done

  # 3. A private Xvfb of this user serves the display, with access control.
  local args; args="$(xvfb_args "$n")"
  [[ -n "$args" ]] || refuse "DISPLAY :$n is not served by a private Xvfb of this user"
  local sock="/tmp/.X11-unix/X$n"
  [[ -S "$sock" && -O "$sock" ]] || refuse "the socket of :$n is missing or not owned by this user"
  local auth; auth="$(awk 'p { print; exit } $0 == "-auth" { p = 1 }' <<<"$args")"
  [[ -n "$auth" ]] || refuse "the Xvfb of :$n runs without -auth (no access control)"

  # 4. XAUTHORITY is that Xvfb's own cookie file, and the server accepts it.
  [[ -n "${XAUTHORITY:-}" ]] || refuse "XAUTHORITY is not set"
  not_owner_path XAUTHORITY "$XAUTHORITY"
  [[ -f "$XAUTHORITY" && -O "$XAUTHORITY" ]] || refuse "XAUTHORITY is missing or not owned by this user"
  [[ "$(realpath -- "$XAUTHORITY")" == "$(realpath -m -- "$auth")" ]] \
    || refuse "XAUTHORITY is not the cookie file of the Xvfb of :$n"
  timeout 5 xauth -f "$XAUTHORITY" list ":$n" 2>/dev/null | awk '$2 == "MIT-MAGIC-COOKIE-1" && $3 ~ /^[0-9a-f]{32}$/ { f = 1 } END { exit !f }' \
    || refuse "XAUTHORITY has no valid cookie for :$n"
  timeout 5 xdpyinfo -display ":$n" >/dev/null 2>&1 || refuse "the X server of :$n does not accept the cookie"

  echo "display-guard: OK :$n"
}

# Each refusal is tested with a harmless command behind --exec; the command,
# and stand-ins named like capture and input tools, must never run.
self_test() {
  local me
  # Global (not local): the EXIT trap runs after this function has returned.
  st_dir="$(mktemp -d "${TMPDIR:-/tmp}/wrlforge-display-guard-test.XXXXXX")" || exit 1
  st_xpid=""
  # Normal end, failure or interruption (INT, TERM, HUP): stop only OUR Xvfb
  # (the one whose -auth is our own cookie file) and remove only our dir.
  st_cleanup() {
    if [[ -n "$st_xpid" ]] && [[ -O "/proc/$st_xpid" ]] \
       && tr '\0' '\n' <"/proc/$st_xpid/cmdline" 2>/dev/null | grep -qxF -- "$st_dir/xauth"; then
      kill "$st_xpid" 2>/dev/null; wait "$st_xpid" 2>/dev/null
    fi
    st_xpid=""
    rm -rf -- "$st_dir"
  }
  trap st_cleanup EXIT
  trap 'exit 130' INT; trap 'exit 143' TERM; trap 'exit 129' HUP
  local t="$st_dir"
  me="$(realpath -- "$0")"
  local fails=0 n=0
  # Stand-ins: if the guard or --exec ever started a real tool by name,
  # it would start one of these instead, which only leaves a mark.
  mkdir -p "$t/bin" "$t/p/home" "$t/p/config" "$t/p/data" "$t/p/cache" "$t/p/state" "$t/p/run"
  local tool
  for tool in import xdotool ffmpeg xwd scrot; do
    printf '#!/bin/sh\ntouch "%s/ran-%s"\n' "$t" "$tool" >"$t/bin/$tool"
    chmod +x "$t/bin/$tool"
  done
  # A clean test profile; each case changes one thing.
  local clean=(env -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS -u XDG_SESSION_TYPE
    PATH="$t/bin:$PATH" HOME="$t/p/home" XDG_CONFIG_HOME="$t/p/config" XDG_DATA_HOME="$t/p/data"
    XDG_CACHE_HOME="$t/p/cache" XDG_STATE_HOME="$t/p/state" XDG_RUNTIME_DIR="$t/p/run")
  ran() { compgen -G "$t/ran*" >/dev/null; }
  expect_refused() {
    local name="$1"; shift
    n=$((n + 1)); rm -f "$t"/ran*
    if "$@" >"$t/out" 2>&1; then echo "FAIL $name: accepted"; fails=$((fails + 1))
    elif ran; then echo "FAIL $name: a command ran: $(cd "$t" && ls ran*)"; fails=$((fails + 1))
    elif ! grep -q 'display-guard: REFUSED' "$t/out"; then echo "FAIL $name: $(head -1 "$t/out")"; fails=$((fails + 1))
    else echo "ok   $name: $(sed -n 's/^display-guard: REFUSED: //p' "$t/out" | head -1)"; fi
  }
  local cap=("$me" --exec -- xdotool key a) shot=("$me" --exec -- import -window root "$t/shot.png")
  local xr=(timeout 30 xvfb-run -a -s "-screen 0 640x480x24")

  # A private Xvfb of our own with a known cookie file. The X server reloads
  # that file when it changes, so a WRONG cookie can only come from another
  # file; invalid data is written into the server's own file last.
  local dn=""; local i
  for i in $(seq 140 199); do
    [[ -e "/tmp/.X11-unix/X$i" || -e "/tmp/.X$i-lock" ]] || { dn="$i"; break; }
  done
  local xpid=""
  if [[ -n "$dn" ]]; then
    xauth -f "$t/xauth" add ":$dn" MIT-MAGIC-COOKIE-1 "$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')" 2>/dev/null
    Xvfb ":$dn" -screen 0 640x480x24 -nolisten tcp -noreset -auth "$t/xauth" >/dev/null 2>&1 &
    xpid=$!; st_xpid=$xpid
    for _ in $(seq 1 100); do [[ -S "/tmp/.X11-unix/X$dn" ]] && break; sleep 0.05; done
  fi

  expect_refused "unset DISPLAY" env -u DISPLAY "${clean[@]}" "${cap[@]}"
  expect_refused "DISPLAY :0" "${clean[@]}" DISPLAY=:0 "${shot[@]}"
  expect_refused "DISPLAY :0.0" "${clean[@]}" DISPLAY=:0.0 "${cap[@]}"
  expect_refused "remote display" "${clean[@]}" DISPLAY=localhost:10 "${cap[@]}"
  expect_refused "nonexistent display" "${clean[@]}" DISPLAY=:7777 XAUTHORITY="$t/none" "${shot[@]}"
  expect_refused "XAUTHORITY unset" "${xr[@]}" env -u XAUTHORITY "${clean[@]}" "${cap[@]}"
  printf 'not an Xauthority file\n' >"$t/garbage"
  expect_refused "invalid Xauthority data" "${xr[@]}" "${clean[@]}" XAUTHORITY="$t/garbage" "${shot[@]}"
  expect_refused "missing Xauthority file" "${xr[@]}" "${clean[@]}" XAUTHORITY="$t/missing" "${cap[@]}"
  if [[ -n "$xpid" ]] && kill -0 "$xpid" 2>/dev/null; then
    xauth -f "$t/other" add ":$dn" MIT-MAGIC-COOKIE-1 "$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')" 2>/dev/null
    expect_refused "wrong cookie (other file)" "${clean[@]}" DISPLAY=":$dn" XAUTHORITY="$t/other" "${shot[@]}"
    printf 'not an Xauthority file\n' >"$t/xauth"
    expect_refused "invalid data in the Xvfb's cookie file" "${clean[@]}" DISPLAY=":$dn" XAUTHORITY="$t/xauth" "${shot[@]}"
  else
    n=$((n + 2)); echo "FAIL own Xvfb cookie tests: could not start the test Xvfb"; fails=$((fails + 2))
  fi
  expect_refused "inherited Wayland" "${xr[@]}" "${clean[@]}" WAYLAND_DISPLAY=wayland-0 "${cap[@]}"
  expect_refused "inherited D-Bus" "${xr[@]}" "${clean[@]}" DBUS_SESSION_BUS_ADDRESS=unix:path=/nonexistent "${cap[@]}"
  expect_refused "real HOME" "${xr[@]}" "${clean[@]}" HOME="$(real_home)" "${cap[@]}"
  expect_refused "owner runtime dir" "${xr[@]}" "${clean[@]}" XDG_RUNTIME_DIR="/run/user/$(id -u)" "${cap[@]}"
  expect_refused "XDG_CONFIG_HOME in real home" "${xr[@]}" "${clean[@]}" XDG_CONFIG_HOME="$(real_home)/.config" "${cap[@]}"

  # Approved: a private Xvfb from xvfb-run with its own cookie and a clean
  # test profile. --exec runs the (stand-in) tool only after the OK line.
  n=$((n + 1)); rm -f "$t"/ran*
  if "${xr[@]}" "${clean[@]}" "${cap[@]}" >"$t/out" 2>&1 && [[ -e "$t/ran-xdotool" ]] \
     && grep -q '^display-guard: OK :[0-9]' "$t/out"; then
    echo "ok   approved private Xvfb: $(head -1 "$t/out")"
  else
    echo "FAIL approved private Xvfb: $(head -2 "$t/out" | tr '\n' ' ')"; fails=$((fails + 1))
  fi

  st_cleanup
  echo "DISPLAY GUARD SELF-TEST pass=$((n - fails)) fail=$fails (want $n)"
  [[ $fails -eq 0 ]]
}

# The Xvfb processes of this user whose -auth file is under directory $1.
xvfb_under() {
  local p
  for p in $(pgrep -u "$(id -u)" -x Xvfb); do
    tr '\0' '\n' <"/proc/$p/cmdline" 2>/dev/null | awk -v d="$1/" 'p { if (index($0, d) == 1) f = 1; p = 0 } $0 == "-auth" { p = 1 } END { exit !f }' && echo "$p"
  done
}

# Interrupt the self-test (TERM, INT, HUP) while its own Xvfb runs: its temp
# directory and every Xvfb it started must be gone, and an Xvfb and a file
# that it does not own (a decoy of this test) must remain.
cleanup_test() {
  local me
  # Global (not local): the EXIT trap runs after this function has returned.
  ct_dir="$(mktemp -d /tmp/wrlforge-display-guard-cleanup.XXXXXX)" || exit 1
  ct_dpid=""
  ct_end() { [[ -n "$ct_dpid" ]] && { kill "$ct_dpid" 2>/dev/null; wait "$ct_dpid" 2>/dev/null; }; ct_dpid=""; rm -rf -- "$ct_dir"; }
  trap ct_end EXIT
  trap 'exit 130' INT; trap 'exit 143' TERM; trap 'exit 129' HUP
  local d="$ct_dir"
  me="$(realpath -- "$0")"
  local fails=0 n=0 dn="" i dpid="" sig
  for i in $(seq 260 299); do
    [[ -e "/tmp/.X11-unix/X$i" || -e "/tmp/.X$i-lock" ]] || { dn="$i"; break; }
  done
  # The decoy: not started by the self-test, so it must survive it.
  mkdir "$d/decoy"; echo keep >"$d/decoy/file"
  xauth -f "$d/decoy/xauth" add ":$dn" MIT-MAGIC-COOKIE-1 "$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')" 2>/dev/null
  Xvfb ":$dn" -screen 0 640x480x24 -nolisten tcp -noreset -auth "$d/decoy/xauth" >/dev/null 2>&1 &
  dpid=$!; ct_dpid=$dpid
  for sig in TERM INT HUP; do
    n=$((n + 1)); mkdir "$d/$sig"
    # A background job starts with INT ignored; give it the default back.
    TMPDIR="$d/$sig" env --default-signal=INT "$me" --self-test >"$d/$sig.out" 2>&1 &
    local cpid=$! xp=""
    # Its own Xvfb (-auth .../wrlforge-display-guard-test.*/xauth), not one
    # of xvfb-run.
    for _ in $(seq 1 200); do
      for i in $(xvfb_under "$d/$sig"); do
        tr '\0' '\n' <"/proc/$i/cmdline" 2>/dev/null | grep -q '/wrlforge-display-guard-test\.[^/]*/xauth$' && { xp=$i; break; }
      done
      [[ -n "$xp" ]] && break; sleep 0.05
    done
    if [[ -z "$xp" ]]; then echo "FAIL $sig: the self-test's Xvfb did not start"; fails=$((fails + 1)); kill "$cpid" 2>/dev/null; wait "$cpid" 2>/dev/null; continue; fi
    kill "-$sig" "$cpid"; wait "$cpid"; local rc=$?
    local left; left="$(xvfb_under "$d/$sig" | tr '\n' ' ')"
    if compgen -G "$d/$sig/wrlforge-display-guard-test.*" >/dev/null; then echo "FAIL $sig: its temp directory remains"; fails=$((fails + 1))
    elif [[ -n "$left" ]] || kill -0 "$xp" 2>/dev/null; then echo "FAIL $sig: its Xvfb remains ($xp $left)"; fails=$((fails + 1))
    elif ! kill -0 "$dpid" 2>/dev/null || [[ "$(cat "$d/decoy/file" 2>/dev/null)" != keep ]]; then echo "FAIL $sig: the decoy Xvfb or file is gone"; fails=$((fails + 1))
    elif grep -q 'DISPLAY GUARD SELF-TEST' "$d/$sig.out"; then echo "FAIL $sig: the self-test was not interrupted"; fails=$((fails + 1))
    else echo "ok   $sig: exit=$rc, its Xvfb $xp and temp directory removed; decoy Xvfb $dpid and file kept"; fi
  done
  ct_end
  echo "DISPLAY GUARD CLEANUP TEST pass=$((n - fails)) fail=$fails (want $n)"
  [[ $fails -eq 0 ]]
}

case "${1:-}" in
  --check) check;;
  --exec)
    shift; [[ "${1:-}" == -- ]] && shift
    [[ $# -gt 0 ]] || refuse "no command"
    check
    exec "$@";;
  --self-test) self_test;;
  --cleanup-test) cleanup_test;;
  *) echo "usage: $0 --check | --exec -- CMD... | --self-test | --cleanup-test" >&2; exit 2;;
esac
