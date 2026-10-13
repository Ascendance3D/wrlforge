#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# STACK-REVIEW-1 QA HOLD fix: the ONE repeatable Linux gate. It runs every
# required test group on a clean checkout and checks the COMPLETE test
# inventory below, so a missing, filtered, skipped or extra test is a
# failure -- never a silent pass. A smaller count is a failure here, not a
# question.
#   ./gate-linux.sh WORKTREE BASE_BIN OUT [TOOLS]
# WORKTREE  a clean checkout of the commit under test (git status empty)
# BASE_BIN  target/debug/wrl-forge built from the comparison (starting) SHA
# OUT       a new output directory (summary.txt, logs, per-fixture TSVs)
# TOOLS     an apps/desktop-tauri dir to take build tools from: the pinned
#           wasm-bindgen (target/tools) and the verified X_ITE cache
#           (target/xite; build-ui.sh verifies it again), and the staged
#           license texts (target/pkg-licenses). Default: none, so they
#           must already be in WORKTREE.
# The pick oracle and the JavaScript tests need WORKTREE/node_modules/x_ite
# (an ignored directory; a link to the root checkout's copy is enough).
# Needs: Rust + wasm32, xvfb-run, xdotool, python3, node (test-only: the
# JavaScript adapter tests and the pick oracle plan), rg. Exit 0 = every
# group passed and matched the inventory.
set -uo pipefail
wt="$(cd "${1:?worktree}" && pwd)"; base_bin="${2:?base binary}"; out="${3:?output dir}"; tools="${4:-}"
app="$wt/apps/desktop-tauri"
[[ ! -e "$out" ]] || { echo "$out exists; give a new directory" >&2; exit 2; }
[[ -x "$base_bin" ]] || { echo "no base binary $base_bin" >&2; exit 2; }
mkdir -p "$out"
t0=$(date +%s); bad=0
log() { echo "[$(( $(date +%s) - t0 ))s] $*" | tee -a "$out/summary.txt"; }
# res NAME OK DETAIL: one gate line; OK=1 passes
res() { if [[ "$2" == 1 ]]; then log "PASS $1 :: $3"; else log "FAIL $1 :: $3"; bad=$((bad + 1)); fi; }

# ---- the complete inventory at this commit --------------------------------
# cargo: "<binary>|passed|ignored" per test target, doc-tests included. Each
# must run, with exactly these counts, 0 failed and 0 filtered out.
inv_app='unittests:wrl_forge_desktop|88|1
unittests:wrl_forge|0|0
tests:app_shell|5|0
tests:flux_regression|6|0
tests:scope_regression|3|0
tests:typed_regression|3|0
unittests:wrlforge_desktop_protocol|96|0
unittests:wrlforge_render|0|0
unittests:wrlforge_scene|25|0
unittests:wrlforge_ui|18|0
doc:wrl_forge_desktop|0|0
doc:wrlforge_desktop_protocol|0|0
doc:wrlforge_render|0|0
doc:wrlforge_scene|0|0
doc:wrlforge_ui|0|0'
inv_crates='unittests:wrlforge_document|15|0
unittests:wrlforge_text|25|0
unittests:wrlforge_vrml|189|0
unittests:wrlforge_wasm|3|0
doc:wrlforge_document|0|0
doc:wrlforge_text|0|0
doc:wrlforge_vrml|0|0
doc:wrlforge_wasm|0|0'
# GUI: "<suite>:<report>|steps"; every step ok and the report pass=true.
inv_gui='menu:report|89
dock:main|65
dock:restore|7
dock:damaged|7
pick:report|130
create:report|51
extrusion:report|58
move:report|156
rotscale:report|72
recovery:restore|34
recovery:later|15
theme:report|82
texture:report|46
native:report|46'
guards_want=151; node_want=56; route_want=2; fx_want=58

# ---- 0. the checkout -------------------------------------------------------
head="$(git -C "$wt" rev-parse HEAD)"; dirty="$(git -C "$wt" status --porcelain | wc -l)"
res checkout "$([[ $dirty -eq 0 ]] && echo 1)" "HEAD=$head dirty=$dirty"
if [[ -n "$tools" ]]; then
  mkdir -p "$app/target/tools/bin" "$app/target/xite"
  cp "$tools/target/tools/bin/wasm-bindgen" "$app/target/tools/bin/"
  ver="$(sed -n 's/^version=//p' "$app/xite/PIN" | head -1)"
  [[ -d "$app/target/xite/$ver" ]] || cp -a "$tools/target/xite/$ver" "$app/target/xite/"
  # tauri.conf.json bundles target/pkg-licenses (made by package-linux.sh);
  # the backend build script refuses to run without it.
  [[ -d "$app/target/pkg-licenses" ]] || cp -a "$tools/target/pkg-licenses" "$app/target/"
fi

# ---- 1. builds -------------------------------------------------------------
(cd "$app" && ./build-ui.sh) >"$out/build-ui.txt" 2>&1; rc=$?
res build-ui-release-wasm "$([[ $rc -eq 0 ]] && echo 1)" "exit=$rc $(tail -1 "$out/build-ui.txt")"
sha() { sha256sum "$1" | cut -d' ' -f1; }
pick="$wt/src/preview/xite-pick-adapter.js"; giz="$app/ui/static/xite-gizmo-adapter.js"
ver="$(sed -n 's/^version=//p' "$app/xite/PIN" | head -1)"
(cd "$app" && ./check-xite-adapters.sh --staged dist "$pick" "$(sha "$pick")" "$giz" "$(sha "$giz")" "$ver" && ./fetch-xite.sh --verify-copy dist/vendor/x_ite) >"$out/staged.txt" 2>&1; rc=$?
res staged-adapters-and-embedded-xite "$([[ $rc -eq 0 ]] && echo 1)" "exit=$rc X_ITE $ver"
(cd "$app" && cargo build -p wrl-forge-desktop -j 16) >"$out/build-desktop.txt" 2>&1; rc=$?
bin="$app/target/debug/wrl-forge"
res build-desktop "$([[ $rc -eq 0 ]] && echo 1)" "exit=$rc $("$bin" --version 2>&1 | head -1)"

# ---- 2. cargo tests against the inventory ---------------------------------
# inventory LOG WANT: every "Running <kind> ... (deps/<name>-<hash>)" or
# "Doc-tests <name>" pairs with its "test result:" line.
inventory() {
  awk '/^ *Running unittests /  { k = "unittests" } /^ *Running tests\// { k = "tests" }
       /^ *Running / { n = $NF; sub(/^.*\//, "", n); sub(/-[0-9a-f]+\)$/, "", n); id = k ":" n }
       /^ *Doc-tests / { id = "doc:" $2 }
       /^test result:/ { print id "|" $4 "|" $8 "|" $6 "|" $12; id = "" }' "$1" | sort >"$1.got"
  # got: id|passed|ignored|failed|filtered
  local want; want="$(printf '%s\n' "$2" | sort)"
  diff <(printf '%s\n' "$want") <(cut -d'|' -f1-3 "$1.got") >"$1.diff"
  local f; f="$(awk -F'|' '$4 != 0 || $5 != 0' "$1.got" | wc -l)"
  [[ ! -s "$1.diff" && $f -eq 0 ]]
}
(cd "$app" && cargo test --workspace --no-fail-fast -j 16) >"$out/test-app.txt" 2>&1; rc=$?
inventory "$out/test-app.txt" "$inv_app"; ok=$?
res test-app-workspace "$([[ $rc -eq 0 && $ok -eq 0 ]] && echo 1)" "exit=$rc $(awk -F'|' '{p+=$2; i+=$3; f+=$4; x+=$5} END {print p" pass "i" ignored "f" fail "x" filtered in "NR" targets"}' "$out/test-app.txt.got") inventory-diff=$(wc -l <"$out/test-app.txt.diff")"
(cd "$wt/crates" && cargo test --workspace --no-fail-fast -j 16) >"$out/test-crates.txt" 2>&1; rc=$?
inventory "$out/test-crates.txt" "$inv_crates"; ok=$?
res test-shared-crates "$([[ $rc -eq 0 && $ok -eq 0 ]] && echo 1)" "exit=$rc $(awk -F'|' '{p+=$2; i+=$3; f+=$4; x+=$5} END {print p" pass "i" ignored "f" fail "x" filtered in "NR" targets"}' "$out/test-crates.txt.got") inventory-diff=$(wc -l <"$out/test-crates.txt.diff")"

# ---- 3. X_ITE guards, adapters, ROUTE -------------------------------------
(cd "$app" && timeout 300 ./test-xite-guards.sh) >"$out/xite-guards.txt" 2>&1; rc=$?
s="$(tail -1 "$out/xite-guards.txt")"
res xite-guards "$([[ $rc -eq 0 && "$s" == "SUMMARY pass=$guards_want fail=0" ]] && echo 1)" "exit=$rc $s (want $guards_want)"
(cd "$wt" && timeout 120 node --test test/preview/xite-pick-adapter.test.js test/preview/xite-pick-guards.test.js test/preview/xite-pick-binding.test.js test/preview/xite-gizmo-highlight.test.js) >"$out/node-adapter.txt" 2>&1; rc=$?
np="$(sed -n 's/^ℹ pass //p' "$out/node-adapter.txt")"; nf="$(sed -n 's/^ℹ fail //p' "$out/node-adapter.txt")"; ns="$(sed -n 's/^ℹ skipped //p' "$out/node-adapter.txt")"; nt="$(sed -n 's/^ℹ todo //p' "$out/node-adapter.txt")"
res js-adapter-tests "$([[ $rc -eq 0 && "$np" == "$node_want" && "$nf" == 0 && "$ns" == 0 && "${nt:-0}" == 0 ]] && echo 1)" "exit=$rc pass=$np fail=$nf skipped=$ns todo=${nt:-0} (want $node_want)"
(cd "$app/src-tauri" && cargo test -p wrl-forge-desktop route_warning -j 16) >"$out/route-tests.txt" 2>&1; rc=$?
n="$(rg -c '^test service::tests::route_warning.* ok$' "$out/route-tests.txt")"
res route-tests "$([[ $rc -eq 0 && "$n" == "$route_want" ]] && echo 1)" "exit=$rc $n ok (want $route_want)"
(cd "$app/src-tauri" && cargo test --release -p wrl-forge-desktop route_warning_lookup_timing -j 16 -- --ignored --nocapture) >"$out/route-timing.txt" 2>&1; rc=$?
res route-timing-ignored-test "$([[ $rc -eq 0 ]] && rg -q 'ROUTE-TIMING' "$out/route-timing.txt" && echo 1)" "exit=$rc $(rg 'ROUTE-TIMING' "$out/route-timing.txt" | tail -1)"

# ---- 4. GUI suites, each on its own display -------------------------------
gui() { local name=$1; shift; (cd "$app" && timeout 600 "$@") >"$out/gui-$name.txt" 2>&1; echo $? >"$out/gui-$name.rc"; }
gui menu ./smoke.sh --headless --menu &
gui dock ./smoke.sh --headless --dock &
gui pick ./smoke.sh --headless --pick &
gui create ./smoke.sh --headless --create &
gui extrusion ./smoke.sh --headless --extrusion &
gui move ./smoke.sh --headless --move &
gui rotscale ./smoke.sh --headless --rotscale &
gui recovery ./smoke.sh --headless --recovery &
gui theme ./smoke.sh --headless --theme tokyo-night-light "$wt/test/fixtures/vrml/nested.wrl" &
# texture: a fresh EMPTY plan dir; the report and logs stay outside it
( tex="$out/texture-plan"; mkdir -p "$tex" "$out/texture-config"
  python3 "$app/smoke-texture-plan.py" "$tex" >"$out/texture-plan.txt" 2>&1 &&
  cd "$app" && timeout 400 env WRLFORGE_SMOKE_XVFB=1 xvfb-run -a -s "-screen 0 1600x1000x24" "$bin" --smoke-texture "$tex" --smoke-report "$out/texture-report.json" --config-dir "$out/texture-config" >"$out/gui-texture.txt" 2>&1
  echo $? >"$out/gui-texture.rc"; echo "report=$out/texture-report.json" >>"$out/gui-texture.txt" ) &
( timeout 300 python3 "$app/native-smoke/harness.py" xvfb "$out/native" "$bin" >"$out/gui-native.txt" 2>&1
  echo $? >"$out/gui-native.rc"; echo "report=$out/native/report.json" >>"$out/gui-native.txt" ) &
wait
# A report is <suite>.d/<name>.json; texture's is texture-report.json and
# native's is native/report.json -- both keyed "report" in the inventory.
while IFS='|' read -r key want; do
  suite="${key%%:*}"; rep="${key#*:}"
  path="$(rg -o 'report=\S+' "$out/gui-$suite.txt" | sed 's/^report=//' | awk -v r="$rep" '{ n = $0; sub(/^.*\//, "", n); sub(/\.json$/, "", n); sub(/^texture-/, "", n) } n == r' | tail -1)"
  got="$(python3 -I - "$path" <<'PY' 2>/dev/null
import json, sys
r = json.load(open(sys.argv[1]))
if "checks" in r:   # native harness: checks is a list of {name, pass, detail}
    c = r.get("checks", [])
    print(r.get("pass") is True and r.get("failures") == [] and r.get("checks_total") == len(c),
          len(c), sum(1 for x in c if x.get("pass") is True))
else:
    s = r.get("steps", [])
    print(r.get("pass") is True, len(s), sum(1 for x in s if x.get("ok") is True))
PY
)"
  read -r p n k <<<"${got:-False 0 0}"
  rc="$(cat "$out/gui-$suite.rc" 2>/dev/null || echo none)"
  res "gui-$suite-$rep" "$([[ $rc == 0 && $p == True && $n == "$want" && $k == "$want" ]] && echo 1)" "exit=$rc pass=$p steps=$n ok=$k (want $want)"
done <<<"$inv_gui"
# WRONG_SOURCE_SELECTIONS over every GUI report: each such step ok, 0 wrong
ws="$(rg -o 'report=\S+' "$out"/gui-*.txt -N --no-filename | sed 's/^report=//' | sort -u | python3 -I -c '
import json, re, sys
n = wrong = 0
for p in sys.stdin.read().split():
    try: r = json.load(open(p))
    except Exception: continue
    for s in r.get("steps", []):
        if "WRONG_SOURCE_SELECTIONS" in s.get("name", ""):
            n += 1
            # "N picks, 0 wrong" (menu, dock) or "... wrong 0 ..." (pick)
            m = re.search(r"(\d+) wrong\b|\bwrong (\d+)", s.get("detail", ""))
            if not (s.get("ok") is True and m and (m.group(1) or m.group(2)) == "0"): wrong += 1
print(n, wrong)')"
read -r wn ww <<<"$ws"
res wrong-source-selections "$([[ $wn -gt 0 && $ww -eq 0 ]] && echo 1)" "WRONG_SOURCE_SELECTIONS steps=$wn failing=$ww"

# ---- 5. File and Inspector fixtures, by name, against the base -----------
rg --files "$wt/test/fixtures" -g '*.wrl' -g '*.wrz' | rg -v '/(parts|sub)/' | sort >"$out/fxqa.txt"
res fixture-set "$([[ $(wc -l <"$out/fxqa.txt") -eq $fx_want ]] && echo 1)" "$(wc -l <"$out/fxqa.txt") files (want $fx_want)"
one() { # mode label bin file -> name verdict failing-steps
  local mode=$1 label=$2 b=$3 f=$4
  local rel=${f#"$wt/test/fixtures/"}; local name=${rel//\//__}; name=${name%.*}
  local extra=(); [[ $mode == inspector ]] && extra=(--inspector)
  local o; o=$(cd "$app" && WRLFORGE_BIN="$b" timeout 200 ./smoke.sh --headless "${extra[@]}" "$f" 2>&1); local code=$?
  local line; line=$(echo "$o" | rg ': exit=' | tail -1)
  local ex; ex=$(echo "$line" | sed -n 's/.*exit=\([0-9]*\).*/\1/p')
  local pass; pass=$(echo "$line" | sed -n 's/.*pass=\([a-z]*\).*/\1/p')
  local rep; rep=$(echo "$line" | sed -n 's/.*report=\(.*\)/\1/p')
  local v=FAIL
  if [[ "$ex" == 0 && "$pass" == true ]]; then v=PASS; elif [[ "$ex" == 124 || "$code" == 124 || "$pass" == none || -z "$pass" ]]; then v=TIMEOUT; fi
  local failing=""
  [[ -f "$rep" ]] && failing=$(python3 -I -c "import json,sys;r=json.load(open(sys.argv[1]));print(';'.join(s['name'] for s in r.get('steps',[]) if not s.get('ok')))" "$rep" 2>/dev/null)
  printf '%s\t%s\t%s\n' "$name" "$v" "$failing" >>"$out/$mode-$label.tsv"
}
export -f one; export wt app out
for mode in files inspector; do
  j=8; [[ $mode == inspector ]] && j=6
  for label in new base; do
    b=$bin; [[ $label == base ]] && b=$base_bin
    : >"$out/$mode-$label.tsv"
    xargs -a "$out/fxqa.txt" -P "$j" -I{} bash -c 'one "$0" "$1" "$2" "$3"' "$mode" "$label" "$b" {}
    sort -o "$out/$mode-$label.tsv" "$out/$mode-$label.tsv"
    log "INFO $mode $label: $(cut -f2 "$out/$mode-$label.tsv" | sort | uniq -c | tr '\n' ' ')"
  done
  names="$(diff <(cut -f1 "$out/$mode-new.tsv") <(cut -f1 "$out/$mode-base.tsv") | rg -c '^[<>]' || true)"
  vd="$(join -t$'\t' <(cut -f1,2 "$out/$mode-new.tsv") <(cut -f1,2 "$out/$mode-base.tsv") | awk -F'\t' '$2 != $3' | tee "$out/$mode-verdict-diffs.tsv" | wc -l)"
  sd="$(diff <(cut -f1,3 "$out/$mode-new.tsv") <(cut -f1,3 "$out/$mode-base.tsv") | rg -c '^[<>]' || true)"
  n="$(wc -l <"$out/$mode-new.tsv")"
  res "fixtures-$mode-vs-base" "$([[ $n -eq $fx_want && ${names:-0} -eq 0 && $vd -eq 0 && ${sd:-0} -eq 0 ]] && echo 1)" "$n files, name diffs=${names:-0} verdict diffs=$vd failing-step diffs=${sd:-0}"
done

log "GATE $([[ $bad -eq 0 ]] && echo PASS || echo "FAIL ($bad failing groups)") HEAD=$head"
[[ $bad -eq 0 ]]
