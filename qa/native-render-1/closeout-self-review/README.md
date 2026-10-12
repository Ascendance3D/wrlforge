# NATIVE-RENDER-1 closeout self-review evidence

**This is NOT independent QA.** The same session that wrote the closeout code
ran this review (2026-10-10) and the follow-up correction task. An independent
reviewer must still verify it.

## Contents

| path | what |
|---|---|
| `repeat/` | 5 matched pairs, private `kwin_wayland --virtual`, scale 1.5, debug build: `report.json`, `gpu.csv`, `kwin.log`, harness `*.out` |
| `repeat-raw.tar.xz` | the 10 runs' `report.raw.json` and `environment.json`, byte-identical to the originals |
| `repeat-table.jsonl` | per-run p99 / max, computed with `../closeout-watchdog/tools/table.py` |
| `raw-check.jsonl` | for 44 runs (closeout `batch`/`final`/`stress24` + `repeat`): summary vs raw recomputation, pings sent/ran, maximum outstanding pings, FIFO order, the 5 longest samples |
| `top3.txt` | the 3 longest samples per run: UI CPU time and run-queue wait change, state, syscall, wchan, render-thread CPU/wait, KWin CPU, phase, other pings run during the wait |
| `settings/` | real launch path (no `--smoke-native`, no `--native-viewport`): one folder per case with `config/settings.json`, `result.json`, `app.out`, `kwin.log` |
| `fmt-check-at-review.txt` | `cargo fmt --check` output at review time |
| `start-state.txt` | worktree content hashes at review start |
| `tools/` | `qa_raw.py`, `settings_launch.py` |
| `correction-verify/` | correction task checks: 1 native KWin 1.5 run, 2 settings launches, `cargo fmt --check` before and after the correction |
| `correction-verify-raw.tar.xz` | that run's `report.raw.json` and `environment.json`, byte-identical |

Settings cases: `case-1` and `case-9` `native-experimental`; `case-2` no
settings file; `case-3` `{"viewport":{}}`; `case-4` `native`; `case-5`
`bogus`; `case-6` `Native-Experimental`; `case-7` leading space; `case-8`
trailing space. `case-1-script-timeout` is a first attempt of case 1 whose
test script failed (KWin shutdown timed out) before writing a result; it is
kept.

A render-thread count of 0 proves that native rendering did not start. It
does not prove that X_ITE drew a scene.

## Integrity

- `SOURCE-HASHES.sha256`: SHA-256 of every source file as found in the
  temporary folders, before any change. All listed source files were present;
  none were missing.
- `SANITIZED.txt`: the 26 text files in which the home-folder path was
  replaced by `~` (logs, `report.json` stderr tails, fmt output). No measured
  value changed: all 28 JSON files compare equal after that replacement, and
  the raw archives hold no home path.
- `MANIFEST.sha256`: SHA-256 of every file in this folder as preserved.

## Findings (self-review, not independent)

- Each watchdog ping is posted without waiting for a reply; tao runs one
  queued event per GTK main-loop pass. Several pings can wait at the same
  time (maximum 6–11 in normal native runs).
- One delay episode delayed 6 (native, `repeat/native-4`) and 12 (control,
  `repeat/control-4`) queued pings to within 8 ms of its maximum. A sample
  count from a summary therefore does not give the number of episodes.
- Pair 4 of `repeat` showed higher delay in both modes (native p99
  41.72 ms, control p99 41.03 ms). Native passed 5/5.
