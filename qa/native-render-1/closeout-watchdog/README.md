# NATIVE-RENDER-1 closeout — D14 watchdog at KWin scale 1.5

Date: 2026-10-10. Build: debug (`target/debug/wrl-forge`); only a debug
build existed when the original failed run was made. Display: private `kwin_wayland --virtual`, 1600×1000,
`--scale 1.5`, NVIDIA RTX 3060 Ti, Mailbox, peer PID proven by the harness.

## Method

- Gate unchanged: gated phases need p99 < 50 ms and max < 100 ms. A ping
  every 5 ms through `run_on_main_thread`; latency = callback start − send.
  This is the main-thread ping response delay. The watchdog does not wait
  for a reply, so several pings can wait at the same time, and tao runs one
  queued event per GTK main-loop pass.
- Added (recorded only): `report.raw.json` with every ping
  (`sent, ran, phase_at_send, phase_at_run`), phase changes, and the UI
  thread and render threads' `/proc/self/task/<tid>` state, `schedstat`
  (run time, run-queue wait), syscall and wchan every 1 ms.
- Harness adds `gpu.csv` (nvidia-smi, 100 ms) and `environment.json`
  (load average, KWin per-thread CPU every 5 ms on CLOCK_MONOTONIC).
- Stall classes (`tools/stalls.py`): UI run time ≥ 60 % of the stall =
  busy; run-queue wait ≥ 60 % = starved; otherwise asleep. These classes do
  not measure queue delay, and "busy" does not prove continuous CPU
  execution.
- Native and X_ITE-only control (`--smoke-native-control`) runs alternate.

## Results

| set | native | control |
|---|---|---|
| `batch` (10 pairs) | 10/10 pass; gpu-load p99 14.16–15.75 ms, max 14.82–27.46 ms | 10/10; control-idle p99 14.11–23.13 ms, max 15.82–31.83 ms |
| `final` (2 pairs, final binary) | 2/2 pass; p99 14.19, 14.36 ms; max 15.08, 14.94 ms | p99 13.87, 14.39 ms; max 14.37, 16.76 ms |
| `stress24` (5 pairs, 24 busy processes, diagnostic) | 0/5 pass; gpu-load p99 282–1005 ms | control-idle p99 458–1271 ms (not gated; above the limits) |

Per-run rows: `*-table.jsonl`. Reports: `runs/`. Raw traces for all 34 runs:
`raw-traces.tar.xz`. Stall classes: `stalls-*.txt`.

- Batch, stalls ≥ 10 ms in the measured phase: native 1,435 (1,434 busy,
  1 asleep while the thread state read "running"), control 1,576 (all busy).
  The main thread can be busy while a ping waits in the queue; these classes
  do not show whether a delay was a blocked UI callback or a queued ping.
- GPU use: mean 13.6–14.9 % (native), 11.4–13.1 % (control); max 24 %.
- Load average at start: 0.99–2.85. The original run's report does not
  record a load average; a value of 2.37 was noted shortly after that run.

## Conclusion

The original failure did not return in 12 native runs. Its saved report has
summaries only, so its cause is not proven. Nearest rank on its summary
(n = 920, p99 77.03, max 85.02) gives at least 10 samples of 77.03 ms or more.
That does not show how many delay episodes occurred: one episode can delay
several queued pings (the self-review measured 6 and 12 such pings in one
episode). The number and cause of the original delay episodes remain
unknown. CPU saturation produces delays of that size and larger in both
modes; that is consistent with the failure, but not proved for it.

Corrected 2026-10-10 after the closeout self-review
(`../closeout-self-review/`): two earlier claims were wrong and are removed —
"no delay was a delayed message" and "the original run had at least 5
separate stalls". All numbers above are unchanged.
