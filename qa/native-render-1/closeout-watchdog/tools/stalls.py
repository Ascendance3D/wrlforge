"""Classify every UI-thread stall >= THRESH ms in the gated/control phase, all runs."""
import json, os, sys, bisect
root, thresh = sys.argv[1], float(sys.argv[2])
agg = {}
for name in sorted(os.listdir(root)):
    d = os.path.join(root, name)
    if not os.path.isdir(d): continue
    raw = json.load(open(os.path.join(d, "report.raw.json")))
    ph = "control-idle" if name.startswith("control") else "gpu-load"
    S = raw["samples"]; ts = [s[0] for s in S]
    groups = {}
    for p in raw["pings"]:
        if p[3] != ph or p[1] - p[0] < thresh: continue
        key = round(p[1])  # released together
        g = groups.setdefault(key, [p[0], p[1]]); g[0] = min(g[0], p[0])
    c = agg.setdefault(name.split("-")[0], {"stalls": 0, "busy": 0, "starved": 0, "asleep": 0, "asleep_sys": {}, "span_ms": []})
    for a, b in groups.values():
        i, j = bisect.bisect_left(ts, a), bisect.bisect_right(ts, b)
        rows = [s[1] for s in S[max(i-1,0):j+1] if s[1]]
        if len(rows) < 2: continue
        run = (rows[-1][1] - rows[0][1]) / 1e6; rq = (rows[-1][2] - rows[0][2]) / 1e6; span = b - a
        c["stalls"] += 1; c["span_ms"].append(round(span, 1))
        if run >= 0.6 * span: c["busy"] += 1
        elif rq >= 0.6 * span: c["starved"] += 1
        else:
            c["asleep"] += 1
            for r in rows: c["asleep_sys"][r[3]] = c["asleep_sys"].get(r[3], 0) + 1
for k, c in agg.items():
    sp = sorted(c.pop("span_ms")); c["span_median"] = sp[len(sp)//2] if sp else None; c["span_max"] = sp[-1] if sp else None
    print(k, json.dumps(c))
