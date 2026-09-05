#!/bin/sh
# The measuring chain (dill -> .crate/src -> workspace -> stubs.py), under a
# memory guard.
#
# Two full `cargo check`s side by side, and then one alone at six jobs, took
# the whole WSL2 VM down (2026-09-05): a single `rustc` on a merged crate can
# run past the machine, and the kernel's OOM killer takes the VM with it.
# There is no sudo here, so no cgroup; this watches `MemAvailable` instead
# and kills the compile itself when it falls under the floor -- the chain
# then fails visibly, with a line in the log saying why, instead of the VM.
#
#   bin/run_chain.sh <report> <log>
#
# Environment: DART2RUST_JOBS (cargo -j, default 4), DART2RUST_MIN_FREE_GB
# (the floor, default 16), DART2RUST_ERASE / DART2RUST_ERASE_OBJECT /
# DART2RUST_OPEN pass through to the driver.
set -u
report=$1
log=$2
: "${DART2RUST_JOBS:=4}"
: "${DART2RUST_MIN_FREE_GB:=16}"
export DART2RUST_JOBS
here=$(cd "$(dirname "$0")/.." && pwd)
dart=$HOME/flutter_sdk/engine/src/out/host_profile/dart-sdk/bin/dart
export PATH="$HOME/.cargo/bin:$PATH"

cd "$here" || exit 2
(
  "$dart" run --packages=.agree/kernel_package_config.json bin/dart2rust_package.dart \
    "$HOME/dart2rust_build/gallery/app_aot_sig.dill" "package:,dart:ui" .crate/src \
  && python3 bin/workspace.py \
  && python3 bin/stubs.py --rounds 80 --report "$report"
) > "$log" 2>&1 &
chain=$!

floor_kb=$((DART2RUST_MIN_FREE_GB * 1024 * 1024))
while kill -0 "$chain" 2>/dev/null; do
  avail=$(awk '/MemAvailable/ {print $2}' /proc/meminfo)
  if [ "$avail" -lt "$floor_kb" ]; then
    last=$(tail -1 "$log" | cut -c1-120)
    crates=$(for pid in $(pgrep -x rustc); do tr '\0' ' ' < /proc/$pid/cmdline 2>/dev/null | sed -n 's/.*--crate-name \([^ ]*\).*/\1/p'; done | sort | uniq -c | sort -rn | tr '\n' ' ')
    echo "OOM-GUARD: MemAvailable ${avail}kB under ${floor_kb}kB; killing the compile (rustc on: $crates) (last: $last)" >> "$log"
    # Exact process names: `pkill -f rustc` also took the shell that was
    # watching the log, whose command line mentioned rustc.
    pkill -x rustc
    pkill -x cargo
    pkill -f 'python3 bin/stubs.py'
    pkill -f 'bin/dart2rust_package.dart'
    sleep 2
    pkill -9 -x rustc
    exit 3
  fi
  sleep 2
done
wait "$chain"
