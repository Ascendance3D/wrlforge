"""Per-run table for a batch dir: gate p99/max, stalls classified by UI-thread state."""
import json, os, subprocess, sys
root = sys.argv[1]
here = os.path.dirname(os.path.abspath(__file__))
rows = []
for name in sorted(os.listdir(root)):
    d = os.path.join(root, name)
    if not os.path.isdir(d) or not os.path.exists(os.path.join(d, "report.raw.json")):
        if os.path.isdir(d): rows.append({"run": name, "error": "no raw trace"})
        continue
    a = json.loads(subprocess.run([sys.executable, "-I", os.path.join(here, "analyze.py"), d, "--detail"], capture_output=True, text=True).stdout)
    ph = a.get("phase", {})
    gated = [s for s in a["stalls_ge_25ms"] if s["phase"] == ph.get("name")]
    cls = {}
    for s in gated:
        u = s["ui"] or {}
        tot = max(1, u.get("n", 1)); run = u.get("run_ms") or 0; rq = u.get("rq_wait_ms") or 0
        span = s["max_ms"]
        k = "busy" if run >= 0.6 * span else ("starved" if rq >= 0.6 * span else "asleep")
        cls[k] = cls.get(k, 0) + 1
    rows.append({"run": name, "pass": a["pass"], "phase": ph.get("name"), "n": ph.get("n"), "p99": ph.get("p99"), "max": ph.get("max"),
                 "stalls_ge_25ms_in_phase": len(gated), "class": cls, "gpu_mean": a["gpu_util_pct"]["mean"], "gpu_max": a["gpu_util_pct"]["max"],
                 "load_start": a["loadavg"][0].split()[0] if a["loadavg"][0] else None, "pings": a["pings"], "failures": a["failures"]})
for r in rows: print(json.dumps(r))
