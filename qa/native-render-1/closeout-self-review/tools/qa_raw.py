"""QA: (1) summary == recomputation from report.raw.json, every phase; (2) outstanding pings;
(3) the 5 longest samples in the measured phase with UI/render/KWin deltas."""
import json, os, re, sys, bisect, math
def pct(v):
    s = sorted(v); i = min(len(s) - 1, max(0, math.ceil(len(s) * 0.99) - 1)); return s[i], s[-1], len(s)
def one(d, top=5):
    rep = json.load(open(f"{d}/report.json")); raw = json.load(open(f"{d}/report.raw.json")); env = json.load(open(f"{d}/environment.json"))
    m = re.search(r"tv_sec: (\d+), tv_nsec: (\d+)", raw["base_monotonic"]); base = int(m.group(1)) * 1e3 + int(m.group(2)) / 1e6
    P = raw["pings"]; byph = {}
    for p in P: byph.setdefault(p[3], []).append(p[1] - p[0])
    mism = []
    for c in rep["checks"]:
        if not c["name"].startswith("watchdog:"): continue
        ph = c["name"].split(":", 1)[1].split(" (")[0]
        n, p99, mx = re.search(r"n=(\d+) p99=([\d.]+)ms max=([\d.]+)ms", c["detail"]).groups()
        rp, rm, rn = pct(byph.get(ph, [0]))
        if int(n) != rn or abs(float(p99) - rp) > 0.006 or abs(float(mx) - rm) > 0.006: mism.append((ph, (n, p99, mx), (rn, round(rp, 3), round(rm, 3))))
    extra = set(byph) - {c["name"].split(":", 1)[1].split(" (")[0] for c in rep["checks"] if c["name"].startswith("watchdog:")}
    ev = sorted([(p[0], 1) for p in P] + [(p[1], -1) for p in P]); cur = mx_out = 0
    for _, k in ev: cur += k; mx_out = max(mx_out, cur)
    fifo = all(P2[1] >= P1[1] for P1, P2 in zip(sorted(P), sorted(P)[1:]))
    S = raw["samples"]; ts = [s[0] for s in S]; ran = sorted(p[1] for p in P)
    ph = "control-idle" if rep.get("control_run") else "gpu-load"
    comp = env["compositor"]; ct = [r[0] for r in comp]
    rows = []
    for p in sorted([p for p in P if p[3] == ph], key=lambda p: p[0] - p[1])[:top]:
        a, b = p[:2]
        i, j = bisect.bisect_left(ts, a), bisect.bisect_right(ts, b); w = S[i:j]
        ui = [s[1] for s in w if s[1]]
        r = {"lat": round(b - a, 2), "t": round(a), "phase_send": p[2], "phase_run": p[3], "samples": len(w)}
        if len(ui) >= 2:
            span = w[-1][0] - w[0][0]
            r["ui"] = {"span": round(span, 1), "cpu": round((ui[-1][1] - ui[0][1]) / 1e6, 1), "rqwait": round((ui[-1][2] - ui[0][2]) / 1e6, 1),
                       "states": dict(sorted({x[0]: sum(1 for y in ui if y[0] == x[0]) for x in ui}.items())),
                       "sys": dict(sorted({x[3]: sum(1 for y in ui if y[3] == x[3]) for x in ui}.items())),
                       "wchan": dict(sorted({x[4]: sum(1 for y in ui if y[4] == x[4]) for x in ui}.items()))}
            rt = {}
            for s in (w[0], w[-1]):
                for tid, x in s[2]: rt.setdefault(tid, []).append(x)
            r["render"] = {str(t): {"cpu": round((v[-1][1] - v[0][1]) / 1e6, 1), "wait": round((v[-1][2] - v[0][2]) / 1e6, 1)} for t, v in rt.items() if len(v) == 2}
        # other pings that ran while this one waited (queue still draining => not a full stall)
        r["other_pings_ran_during_wait"] = bisect.bisect_left(ran, b - 0.05) - bisect.bisect_right(ran, a)
        ci, cj = bisect.bisect_left(ct, base + a), bisect.bisect_right(ct, base + b) - 1
        if cj > ci:
            A, B = comp[ci], comp[cj]; dA = {x[0]: x[1] for x in A[1:]}; tot = 0; best = (0.0, 0)
            for x in B[1:]:
                dd = (x[1] - dA.get(x[0], x[1])) / 1e6; tot += dd; best = max(best, (dd, x[0]))
            r["kwin"] = {"span": round(B[0] - A[0], 1), "cpu": round(tot, 1), "top_thread_cpu": round(best[0], 1)}
        rows.append(r)
    return {"run": d.split("/")[-2] + "/" + d.split("/")[-1], "pass": rep["pass"], "summary_mismatch": mism, "raw_phases_without_check": sorted(extra),
            "pings_sent": raw["pings_sent"], "pings_ran": len(P), "max_outstanding": mx_out, "fifo": fifo, "top": rows}
for d in sys.argv[1:]:
    print(json.dumps(one(d.rstrip("/"))))
