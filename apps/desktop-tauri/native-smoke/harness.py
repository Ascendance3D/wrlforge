#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""NATIVE-RENDER-1: run `wrl-forge --smoke-native` on a PRIVATE display.

usage: harness.py xvfb|weston|kwin OUTDIR BINARY [EXTRA_APP_ARGS...]
env:   GDK_SCALE (Xvfb), NR1_OUTPUT_SCALE (weston/KWin output scale),
       VK_ICD_FILENAMES (adapter choice) are passed through.

Safety rules (owner conditions; Step 0 design):
- start our OWN display server: Xvfb (-displayfd), weston (headless) or KWin
  (--virtual), on a fresh socket inside a private 0700 runtime directory;
- prove with SO_PEERCRED on a fresh connection that the socket belongs to the
  PID we started; refuse otherwise;
- build the app environment from an ALLOW-LIST: never WAYLAND_SOCKET, never
  an inherited DISPLAY/WAYLAND_DISPLAY/DBUS address; a throwaway HOME;
- run the app under `dbus-run-session` (no autolaunched session daemons);
- send NO input of any kind: the smoke drives the viewport host itself.
The report is OUTDIR/report.json; the proof is added to it.
"""
import json, os, secrets, shutil, socket, struct, subprocess, sys, tempfile, time

kind, out, binary, *extra = sys.argv[1:]
out = os.path.abspath(out)
os.makedirs(out, exist_ok=False)
work = tempfile.mkdtemp(prefix="nr1-smoke-")
run = os.path.join(work, "run"); os.makedirs(run, mode=0o700)
home = os.path.join(work, "home"); os.makedirs(home)
logs = open(os.path.join(out, f"{kind}.log"), "w")

def peer_pid(path):
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(path)
    pid, _, _ = struct.unpack("3i", s.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")))
    s.close()
    return pid

env = {"PATH": "/usr/bin:/bin", "HOME": home, "XDG_RUNTIME_DIR": run, "LANG": "C.UTF-8",
       "NO_AT_BRIDGE": "1", "WEBKIT_DISABLE_COMPOSITING_MODE": "1"}
for k in ("VK_ICD_FILENAMES", "GDK_SCALE"):
    if k in os.environ:
        env[k] = os.environ[k]

if kind == "xvfb":
    r, w = os.pipe()
    server = subprocess.Popen(["Xvfb", "-displayfd", str(w), "-screen", "0", "1600x1000x24", "-nolisten", "tcp", "-noreset"],
                              pass_fds=(w,), stdout=subprocess.DEVNULL, stderr=logs)
    os.close(w)
    num, deadline = b"", time.time() + 10
    while not num.endswith(b"\n") and time.time() < deadline:
        num += os.read(r, 16)
    num = num.strip().decode()
    def refuse(msg):
        server.kill(); sys.exit("REFUSED: " + msg)
    if not num.isdigit():
        refuse("Xvfb did not report a display number")
    if os.environ.get("DISPLAY", "").split(".")[0] == ":" + num:
        refuse("private display equals the user's display")
    path = f"/tmp/.X11-unix/X{num}"
    env.update({"DISPLAY": ":" + num, "GDK_BACKEND": "x11"})
elif kind in ("weston", "kwin"):
    name = "nr1-" + secrets.token_hex(4)
    cenv = {"PATH": "/usr/bin:/bin", "HOME": home, "XDG_RUNTIME_DIR": run, "LANG": "C.UTF-8"}
    if "VK_ICD_FILENAMES" in os.environ:
        cenv["VK_ICD_FILENAMES"] = os.environ["VK_ICD_FILENAMES"]
    scale = os.environ.get("NR1_OUTPUT_SCALE")
    if kind == "weston":
        cmd = ["weston", "--backend=headless", f"--socket={name}", "--width=1600", "--height=1000", "--idle-time=0"]
        if scale: cmd.append(f"--scale={scale}")
    else:
        cmd = ["kwin_wayland", "--virtual", "--socket", name, "--width", "1600", "--height", "1000",
               "--no-lockscreen", "--no-global-shortcuts", "--no-kactivities"]
        if scale: cmd += ["--scale", scale]
    server = subprocess.Popen(cmd, env=cenv, stdout=logs, stderr=logs)
    path = os.path.join(run, name)
    deadline = time.time() + 15
    while not os.path.exists(path) and time.time() < deadline and server.poll() is None:
        time.sleep(0.05)
    def refuse(msg):
        server.terminate(); server.wait(5); sys.exit("REFUSED: " + msg)
    if not os.path.exists(path):
        refuse(f"{kind} did not create {path}")
    env.update({"WAYLAND_DISPLAY": name, "GDK_BACKEND": "wayland"})
else:
    sys.exit("display must be xvfb, weston or kwin")

pid = peer_pid(path)
if pid != server.pid:
    refuse(f"{path} peer pid {pid} != started pid {server.pid}")
assert "WAYLAND_SOCKET" not in env and "DBUS_SESSION_BUS_ADDRESS" not in env
assert not (kind != "xvfb" and "DISPLAY" in env)
proof = {"output_scale": os.environ.get("NR1_OUTPUT_SCALE") or os.environ.get("GDK_SCALE") or "1", "display_server": kind, "server_pid": server.pid, "socket": path, "peer_pid": pid, "env_keys": sorted(env)}

report = os.path.join(out, "report.json")
args = ["dbus-run-session", "--", binary, "--smoke-native", os.path.join(out, "run"),
        "--smoke-report", report, "--config-dir", os.path.join(out, "config"), *extra]
# Closeout environment evidence (recorded only, never gated): system load,
# GPU use every 100 ms, and the display server's per-thread CPU every 5 ms.
import threading
def first_line(p):
    try:
        return open(p).readline().strip()
    except OSError:
        return ""
envlog = {"loadavg_start": first_line("/proc/loadavg"), "cpu_start": first_line("/proc/stat"), "compositor": []}
gpu = None
if shutil.which("nvidia-smi"):
    gpu = subprocess.Popen(["nvidia-smi", "--query-gpu=timestamp,utilization.gpu,clocks.gr,power.draw", "--format=csv,noheader,nounits", "-lms", "100"],
                           stdout=open(os.path.join(out, "gpu.csv"), "w"), stderr=subprocess.DEVNULL)
sampling = True
def sample_compositor():
    while sampling:
        row = [round(time.monotonic() * 1e3, 3)]  # CLOCK_MONOTONIC ms
        try:
            for t in sorted(os.listdir(f"/proc/{server.pid}/task")):
                run_ns, wait_ns = open(f"/proc/{server.pid}/task/{t}/schedstat").read().split()[:2]
                row.append([int(t), int(run_ns), int(wait_ns)])
        except (OSError, ValueError):
            pass
        envlog["compositor"].append(row)
        time.sleep(0.005)
sampler = threading.Thread(target=sample_compositor, daemon=True); sampler.start()

t0 = time.time()
try:
    p = subprocess.run(args, env=env, capture_output=True, text=True, timeout=240)
    code, se = p.returncode, p.stderr
except subprocess.TimeoutExpired:
    code, se = "timeout", ""
finally:
    sampling = False
    sampler.join(1)
    if gpu:
        gpu.terminate(); gpu.wait(5)
    envlog["loadavg_end"] = first_line("/proc/loadavg")
    json.dump(envlog, open(os.path.join(out, "environment.json"), "w"))
    server.terminate()
    try:
        server.wait(5)
    except subprocess.TimeoutExpired:
        server.kill()
try:
    rep = json.load(open(report))
except Exception as e:
    rep = {"pass": False, "error": f"no report: {e}"}
rep["exit_code"] = code
rep["wall_s"] = round(time.time() - t0, 2)
rep["display_proof"] = proof
rep["stderr_tail"] = se.strip().splitlines()[-40:]
json.dump(rep, open(report, "w"), indent=1)
# The throwaway HOME / runtime dir are this run's own scratch.
shutil.rmtree(work, ignore_errors=True)
print(json.dumps({"pass": rep.get("pass"), "exit": code, "checks": rep.get("checks_total"),
                  "failures": [f["name"] + ": " + f.get("detail", "") for f in rep.get("failures", [])],
                  "proof": proof}, indent=1))
sys.exit(0 if rep.get("pass") is True and code == 0 else 1)
