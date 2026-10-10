"""NATIVE-RENDER-1 closeout: classify UI-thread watchdog delays from raw traces.
usage: analyze.py RUNDIR [--detail]"""
import json, re, sys, os, statistics
d = sys.argv[1]; detail = "--detail" in sys.argv
rep = json.load(open(os.path.join(d, "report.json")))
raw = json.load(open(os.path.join(d, "report.raw.json")))
env = json.load(open(os.path.join(d, "environment.json")))
m = re.search(r"tv_sec: (\d+), tv_nsec: (\d+)", raw["base_monotonic"])
base_ms = int(m.group(1)) * 1e3 + int(m.group(2)) / 1e6
samples = raw["samples"]  # [t, ui, [[tid, rt], ...]]
st = [s[0] for s in samples]
import bisect
def window(a, b):
    i, j = bisect.bisect_left(st, a), bisect.bisect_right(st, b)
    return samples[max(i - 1, 0):j + 1]
def describe(win, key):
    rows = []
    for s in win:
        if key == "ui":
            if s[1]: rows.append(s[1])
        else:
            for tid, r in s[2]:
                rows.append(r)
    if not rows: return None
    states, sysc, wch = {}, {}, {}
    for r in rows:
        states[r[0]] = states.get(r[0], 0) + 1
        sysc[r[3]] = sysc.get(r[3], 0) + 1
        wch[r[4]] = wch.get(r[4], 0) + 1
    if key == "ui":
        run = (rows[-1][1] - rows[0][1]) / 1e6; wait = (rows[-1][2] - rows[0][2]) / 1e6
    else:
        run = wait = None
    return {"n": len(rows), "states": states, "syscalls": sysc, "wchan": wch, "run_ms": run, "rq_wait_ms": wait}
SYS = {"7": "poll", "271": "ppoll", "202": "futex", "232": "epoll_wait", "0": "read", "1": "write", "16": "ioctl", "running": "running", "35": "nanosleep", "230": "clock_nanosleep", "47": "recvmsg", "46": "sendmsg", "281": "epoll_pwait"}
def named(h): return {SYS.get(k, k): v for k, v in sorted(h.items(), key=lambda x: -x[1])}
def comp_cpu(a, b):
    rows = env["compositor"]; A, B = base_ms + a, base_ms + b
    inw = [r for r in rows if A <= r[0] <= B]
    if len(inw) < 2: return None
    tot = lambda r: sum(x[1] for x in r[1:])
    span = inw[-1][0] - inw[0][0]
    return {"cpu_ms": round((tot(inw[-1]) - tot(inw[0])) / 1e6, 2), "span_ms": round(span, 2)}
out = {"run": os.path.basename(d.rstrip("/")), "pass": rep.get("pass"), "failures": [f["name"] + " " + f["detail"] for f in rep.get("failures", [])],
       "control": rep.get("control_run"), "loadavg": [env.get("loadavg_start"), env.get("loadavg_end")]}
for c in rep["checks"]:
    if c["name"].startswith("watchdog:gpu-load") or c["name"].startswith("watchdog:control-idle"):
        out["gate"] = c["name"] + " " + c["detail"]
pings = raw["pings"]
out["pings"] = {"sent": raw["pings_sent"], "ran": len(pings)}
mis = [p for p in pings if p[2] != p[3]]
out["phase_mismatch"] = len(mis)
ph = "control-idle" if rep.get("control_run") else "gpu-load"
gp = [p for p in pings if p[3] == ph]
lat = sorted(p[1] - p[0] for p in gp)
if lat:
    i = max(0, min(len(lat) - 1, -(-len(lat) * 99 // 100) - 1))
    out["phase"] = {"name": ph, "n": len(lat), "p99": round(lat[i], 2), "max": round(lat[-1], 2), "median": round(statistics.median(lat), 2)}
# Stalls: group pings by run time: pings that ran within 0.5 ms of each other after a gap.
stalls = []
for p in sorted(pings, key=lambda p: p[0]):
    l = p[1] - p[0]
    if l >= 25:
        if stalls and abs(stalls[-1]["ran"] - p[1]) < 1.0:
            stalls[-1]["pings"] += 1; stalls[-1]["max"] = max(stalls[-1]["max"], l); stalls[-1]["from"] = min(stalls[-1]["from"], p[0])
        else:
            stalls.append({"from": p[0], "ran": p[1], "max": l, "pings": 1, "phase": p[3]})
gpu = []
try:
    for line in open(os.path.join(d, "gpu.csv")):
        f = [x.strip() for x in line.split(",")]
        gpu.append(float(f[1]))
except Exception: pass
out["gpu_util_pct"] = {"max": max(gpu) if gpu else None, "mean": round(statistics.mean(gpu), 1) if gpu else None}
res = []
for s in stalls:
    a, b = s["from"], s["ran"]
    r = {"phase": s["phase"], "t_ms": round(a, 1), "max_ms": round(s["max"], 2), "pings_released": s["pings"],
         "ui": describe(window(a, b), "ui"), "render": describe(window(a, b), "rt"), "compositor": comp_cpu(a, b)}
    if r["ui"]: r["ui"]["syscalls"] = named(r["ui"]["syscalls"])
    if r["render"]: r["render"]["syscalls"] = named(r["render"]["syscalls"])
    res.append(r)
out["stalls_ge_25ms"] = res if detail else len(res)
if not detail:
    out["worst"] = max(res, key=lambda r: r["max_ms"]) if res else None
print(json.dumps(out, indent=1))
