#!/usr/bin/env bash
# PKG-LINUX-1 packaged-app tests. Usage: run.sh BIN OUTDIR
# Every run uses disposable copies and a temporary --config-dir.
set -uo pipefail
bin="$1"; out="$2"; here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p "$out"; fail=0
xv=(xvfb-run -a -s "-screen 0 1600x1000x24")
run() { # name timeout args...
  local n="$1" t="$2"; shift 2
  local d="$out/$n"; mkdir -p "$d/config"
  timeout "$t" "${xv[@]}" env WRLFORGE_SMOKE_XVFB=1 "$bin" "$@" --smoke-report "$d/report.json" --config-dir "$d/config" >"$d/stdout.txt" 2>"$d/stderr.txt"
  local code=$? pass
  pass=$(sed -n 's/.*"pass": *\(true\|false\).*/\1/p' "$d/report.json" 2>/dev/null | head -1)
  echo "$n: exit=$code pass=${pass:-none}"
  [[ $code -eq 0 && "$pass" == "true" ]] || fail=1
}
for f in pkg-test-basic.wrl pkg-test-basic-gzip.wrl; do
  mkdir -p "$out/open-$f.d"; cp "$here/inputs/$f" "$out/open-$f.d/"
  run "open-$f" 120 --smoke "$out/open-$f.d/$f" --smoke-inspector
done
mkdir -p "$out/create.d"; run create 170 --smoke-create "$out/create.d"
rm -rf "$out/pick.d"; cp -r "$here/plan" "$out/pick.d"; run pick 170 --smoke-pick "$out/pick.d"
exit $fail
