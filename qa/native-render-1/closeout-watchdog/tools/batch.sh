#!/bin/bash
# usage: batch.sh OUTROOT FIRST LAST  -- interleaved native/control pairs on KWin scale 1.5
H=/home/ryan/Projects/cybertown/.worktrees/wrlforge/native-render-1/apps/desktop-tauri/native-smoke/harness.py
B=/home/ryan/Projects/cybertown/.worktrees/wrlforge/native-render-1/apps/desktop-tauri/target/debug/wrl-forge
for i in $(seq -w "$2" "$3"); do
  NR1_OUTPUT_SCALE=1.5 python3 "$H" kwin "$1/native-$i" "$B" > "$1/native-$i.out" 2>&1
  n=$?
  NR1_OUTPUT_SCALE=1.5 python3 "$H" kwin "$1/control-$i" "$B" --smoke-native-control > "$1/control-$i.out" 2>&1
  c=$?
  echo "pair $i native=$n control=$c"
  if [ -z "$NOSTOP" ] && { [ "$n" != 0 ] || [ "$c" != 0 ]; }; then echo "STOP at pair $i"; exit 1; fi
done
