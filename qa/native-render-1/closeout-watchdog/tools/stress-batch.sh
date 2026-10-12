#!/bin/bash
# Diagnostic only: the same interleaved pairs while N busy-loop processes saturate the CPU.
N=${N:-24}; pids=()
cleanup() { kill "${pids[@]}" 2>/dev/null; wait "${pids[@]}" 2>/dev/null; }
trap cleanup EXIT INT TERM
for i in $(seq "$N"); do timeout 600 python3 -I -c "while True: pass" & pids+=($!); done
sleep 2
/tmp/nr1-closeout/tools/batch.sh "$@" ; rc=$?
echo "stress=$N rc=$rc"
