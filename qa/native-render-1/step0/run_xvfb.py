#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""NATIVE-RENDER-1 Step 0: run a host self-test on a PRIVATE Xvfb.

Safety rules (design report section 4):
- start our own Xvfb (-displayfd) and prove, with SO_PEERCRED on a fresh
  connection to its socket, that the socket belongs to the PID we started;
- build the child environment from an allow-list (no WAYLAND_SOCKET, no
  WAYLAND_DISPLAY, no inherited DISPLAY or DBUS address), with a throwaway HOME;
- refuse to run if any proof fails. No input is sent (self-test only).

usage: run_xvfb.py OUT.json SCALE BINARY [ARGS...]
"""
import json, os, socket, struct, subprocess, sys, tempfile, time

out_path, scale, binary, *args = sys.argv[1:]
work = tempfile.mkdtemp(prefix="nr1s0-")
r, w = os.pipe()
xvfb = subprocess.Popen(["Xvfb", "-displayfd", str(w), "-screen", "0", "1600x1000x24", "-nolisten", "tcp", "-noreset"],
                        pass_fds=(w,), stdout=subprocess.DEVNULL, stderr=open(os.path.join(work, "xvfb.log"), "w"))
os.close(w)
num = b""
deadline = time.time() + 10
while not num.endswith(b"\n") and time.time() < deadline:
    num += os.read(r, 16)
num = num.strip().decode()
if not num.isdigit():
    xvfb.kill(); sys.exit("REFUSED: Xvfb did not report a display number")
user_display = os.environ.get("DISPLAY", "")
if user_display.split(".")[0] == ":" + num:
    xvfb.kill(); sys.exit("REFUSED: private display equals the user's display")
path = f"/tmp/.X11-unix/X{num}"
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(path)
pid, uid, gid = struct.unpack("3i", s.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")))
s.close()
if pid != xvfb.pid:
    xvfb.kill(); sys.exit(f"REFUSED: {path} peer pid {pid} != started Xvfb pid {xvfb.pid}")
home = os.path.join(work, "home"); os.makedirs(home)
env = {"PATH": "/usr/bin:/bin", "HOME": home, "DISPLAY": ":" + num, "GDK_BACKEND": "x11",
       "XDG_RUNTIME_DIR": work, "LANG": os.environ.get("LANG", "C.UTF-8"), "GDK_SCALE": scale,
       "WEBKIT_DISABLE_COMPOSITING_MODE": "1", "NO_AT_BRIDGE": "1"}
for k in ("STEP0_FORCE_FIFO_ONLY", "NR0_PRESENT", "WGPU_BACKEND", "VK_ICD_FILENAMES"):
    if k in os.environ: env[k] = os.environ[k]
proof = {"display": ":" + num, "xvfb_pid": xvfb.pid, "socket": path, "peer_pid": pid, "user_display": user_display, "env_keys": sorted(env)}
try:
    p = subprocess.run(["dbus-run-session", "--", binary, *args], env=env, capture_output=True, text=True, timeout=300)
    code, so, se = p.returncode, p.stdout, p.stderr
except subprocess.TimeoutExpired as e:
    code, so, se = "timeout", (e.stdout or b"").decode(errors="replace") if isinstance(e.stdout, bytes) else (e.stdout or ""), ""
finally:
    xvfb.terminate(); xvfb.wait(5)
i = so.find("{\n")
rep = json.loads(so[i:]) if i >= 0 else {"pass": False, "raw_stdout": so[-4000:]}
rep["exit_code"] = code
rep["display_proof"] = proof
rep["stderr_tail"] = se.strip().splitlines()[-30:]
json.dump(rep, open(out_path, "w"), indent=1)
print(json.dumps({"pass": rep.get("pass"), "exit": code, "checks": rep.get("checks_total"), "failures": [f["name"] for f in rep.get("failures", [])], "proof": proof}, indent=1))
