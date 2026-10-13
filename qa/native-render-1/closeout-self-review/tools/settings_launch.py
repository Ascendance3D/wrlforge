#!/usr/bin/env python3
"""QA: real launch path for viewport.renderer. NO --smoke-native, NO --native-viewport.
Private kwin_wayland --virtual (peer-PID proven), throwaway HOME/config, a fixture
document opened by path. Observes render threads ('wrlforge-render*') in the app
process for WAIT seconds. usage: settings_launch.py BINARY OUTDIR CASE_JSON_OR_- [WAIT]"""
import json, os, secrets, shutil, socket, struct, subprocess, sys, tempfile, time
binary, out, case = sys.argv[1:4]; wait = float(sys.argv[4]) if len(sys.argv) > 4 else 10
os.makedirs(out, exist_ok=False)
work = tempfile.mkdtemp(prefix="nr1-qa-")
run = os.path.join(work, "run"); os.makedirs(run, mode=0o700)
home = os.path.join(work, "home"); os.makedirs(home)
cfg = os.path.join(out, "config"); os.makedirs(cfg)
if case != "-":
    open(os.path.join(cfg, "settings.json"), "w").write(case)
doc = os.path.join(out, "doc.wrl")
open(doc, "w").write("#VRML V2.0 utf8\nShape { geometry Box {} }\n")
name = "nr1qa-" + secrets.token_hex(4)
cenv = {"PATH": "/usr/bin:/bin", "HOME": home, "XDG_RUNTIME_DIR": run, "LANG": "C.UTF-8"}
logs = open(os.path.join(out, "kwin.log"), "w")
server = subprocess.Popen(["kwin_wayland", "--virtual", "--socket", name, "--width", "1600", "--height", "1000",
                           "--no-lockscreen", "--no-global-shortcuts", "--no-kactivities", "--scale", "1.5"], env=cenv, stdout=logs, stderr=logs)
path = os.path.join(run, name); deadline = time.time() + 15
while not os.path.exists(path) and time.time() < deadline and server.poll() is None:
    time.sleep(0.05)
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM); s.connect(path)
pid = struct.unpack("3i", s.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, struct.calcsize("3i")))[0]; s.close()
if pid != server.pid:
    server.kill(); sys.exit("REFUSED: peer pid mismatch")
env = dict(cenv, WAYLAND_DISPLAY=name, GDK_BACKEND="wayland", NO_AT_BRIDGE="1", WEBKIT_DISABLE_COMPOSITING_MODE="1")
p = subprocess.Popen(["dbus-run-session", "--", binary, "--config-dir", cfg, doc], env=env,
                     stdout=open(os.path.join(out, "app.out"), "w"), stderr=subprocess.STDOUT, start_new_session=True)
def app_pid():
    for d in os.listdir("/proc"):
        if d.isdigit():
            try:
                if os.readlink(f"/proc/{d}/exe") == os.path.realpath(binary) and open(f"/proc/{d}/environ", "rb").read().find(name.encode()) >= 0:
                    return int(d)
            except OSError:
                pass
apid, render, seen, t0 = None, 0, [], time.time()
while time.time() - t0 < wait:
    apid = apid or app_pid()
    if apid:
        try:
            names = [open(f"/proc/{apid}/task/{t}/comm").read().strip() for t in os.listdir(f"/proc/{apid}/task")]
            n = sum(1 for c in names if c.startswith("wrlforge-render"))
            render = max(render, n)
            if n and not seen: seen.append(round(time.time() - t0, 2))
        except OSError:
            pass
    time.sleep(0.1)
alive = apid is not None and os.path.exists(f"/proc/{apid}")
os.killpg(p.pid, 15)
try: p.wait(10)
except subprocess.TimeoutExpired: os.killpg(p.pid, 9); p.wait(5)
server.terminate()
try: server.wait(15)
except subprocess.TimeoutExpired: server.kill(); server.wait(5)
shutil.rmtree(work, ignore_errors=True)
r = {"case": case, "app_pid_found": apid is not None, "app_alive_at_end": alive, "max_render_threads": render,
     "first_render_thread_s": seen[0] if seen else None, "native": render > 0, "proof_peer_pid": pid == server.pid}
json.dump(r, open(os.path.join(out, "result.json"), "w"), indent=1); print(json.dumps(r))
