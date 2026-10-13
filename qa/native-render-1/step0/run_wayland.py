#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""NATIVE-RENDER-1 Step 0: run a host self-test on a PRIVATE Wayland compositor.

Safety rules (design report section 4):
- start our own compositor (weston headless or KWin virtual) on a random
  `nr1-` socket inside a private XDG_RUNTIME_DIR (mode 0700);
- prove with SO_PEERCRED that the socket belongs to the PID we started;
- build the child environment from an allow-list: no WAYLAND_SOCKET, no
  DISPLAY, no inherited DBUS address; throwaway HOME;
- refuse to run if any proof fails. No input is sent (self-test only).

usage: run_wayland.py OUT.json weston|kwin BINARY [ARGS...]
"""
import json, os, secrets, socket, struct, subprocess, sys, tempfile, time

out_path, comp, binary, *args = sys.argv[1:]
work = tempfile.mkdtemp(prefix="nr1s0wl-")
run = os.path.join(work, "run"); os.makedirs(run, mode=0o700)
home = os.path.join(work, "home"); os.makedirs(home)
name = "nr1-" + secrets.token_hex(4)
cenv = {"PATH": "/usr/bin:/bin", "HOME": home, "XDG_RUNTIME_DIR": run, "LANG": "C.UTF-8"}
if comp == "weston":
    cmd = ["weston", "--backend=headless", f"--socket={name}", "--width=1600", "--height=1000", "--idle-time=0"]
elif comp == "kwin":
    cmd = ["kwin_wayland", "--virtual", "--socket", name, "--width", "1600", "--height", "1000", "--no-lockscreen", "--no-global-shortcuts", "--no-kactivities"]
else:
    sys.exit("compositor must be weston or kwin")
for k in ("VK_ICD_FILENAMES",):
    if k in os.environ: cenv[k] = os.environ[k]
log = open(os.path.join(work, comp + ".log"), "w")
c = subprocess.Popen(cmd, env=cenv, stdout=log, stderr=log)
path = os.path.join(run, name)
deadline = time.time() + 15
while not os.path.exists(path) and time.time() < deadline and c.poll() is None:
    time.sleep(0.05)
def refuse(msg):
    c.terminate(); c.wait(5); sys.exit("REFUSED: " + msg)
if not os.path.exists(path):
    refuse(f"{comp} did not create {path}")
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(path)
pid, _, _ = struct.unpack("3i", s.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")))
s.close()
if pid != c.pid:
    refuse(f"{path} peer pid {pid} != started {comp} pid {c.pid}")
env = {"PATH": "/usr/bin:/bin", "HOME": home, "XDG_RUNTIME_DIR": run, "WAYLAND_DISPLAY": name, "GDK_BACKEND": "wayland",
       "LANG": "C.UTF-8", "NO_AT_BRIDGE": "1", "WEBKIT_DISABLE_COMPOSITING_MODE": "1"}
for k in ("STEP0_FORCE_FIFO_ONLY", "NR0_PRESENT", "VK_ICD_FILENAMES", "GDK_SCALE"):
    if k in os.environ: env[k] = os.environ[k]
assert "WAYLAND_SOCKET" not in env and "DISPLAY" not in env
proof = {"compositor": comp, "pid": c.pid, "socket": path, "peer_pid": pid, "env_keys": sorted(env)}
try:
    p = subprocess.run(["dbus-run-session", "--", binary, *args], env=env, capture_output=True, text=True, timeout=300)
    code, so, se = p.returncode, p.stdout, p.stderr
except subprocess.TimeoutExpired as e:
    code, so, se = "timeout", e.stdout.decode(errors="replace") if isinstance(e.stdout, bytes) else (e.stdout or ""), ""
finally:
    c.terminate()
    try: c.wait(5)
    except subprocess.TimeoutExpired: c.kill()
i = so.find("{\n")
rep = json.loads(so[i:]) if i >= 0 else {"pass": False, "raw_stdout": so[-4000:]}
rep["exit_code"] = code
rep["display_proof"] = proof
rep["stderr_tail"] = se.strip().splitlines()[-30:]
json.dump(rep, open(out_path, "w"), indent=1)
print(json.dumps({"pass": rep.get("pass"), "exit": code, "checks": rep.get("checks_total"), "failures": [f["name"] for f in rep.get("failures", [])], "proof": proof}, indent=1))
