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

# rustc's front end is one thread, and this workspace is a chain of eleven
# crates in which two hold 79% of the lines: a full `cargo check --workspace
# -j 6` ran at 98% CPU on a 32-core machine and took 133s. `-Zthreads`
# parallelises the two passes that are 79% of that -- `type_check_crate` and
# `MIR_borrow_checking` -- and took it to 72.6s at eight threads, 0 errors,
# the same 6.5 GB peak. Past eight it flattens: the critical path is two
# crates, and the front end saturates near 2.5 cores. Measured 2026-09-08.
#
# It is a `-Z` flag on a stable toolchain, so it needs the bootstrap escape
# hatch, and the parallel front end is still experimental --
# `DART2RUST_THREADS=1` turns it off. run_main.sh sets exactly the same two
# variables on purpose: a different RUSTFLAGS is a different fingerprint, and
# the chain and the run would each rebuild the other's work.
: "${DART2RUST_THREADS:=8}"
export RUSTC_BOOTSTRAP=1
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Zthreads=$DART2RUST_THREADS"
here=$(cd "$(dirname "$0")/.." && pwd)
dart=$HOME/flutter_sdk/engine/src/out/host_profile/dart-sdk/bin/dart
export PATH="$HOME/.cargo/bin:$PATH"

cd "$here" || exit 2
(
  # The workspace is re-partitioned every run, so crate identities
  # change and cargo stops recognising -- and therefore stops
  # reclaiming -- what it built last time: 721 runs had left 248 GB
  # in target/, under 16 GB of it live. This reclaims it while
  # nothing is compiling.
  python3 bin/prune_target.py \
  && "$dart" run --packages=.agree/kernel_package_config.json bin/dart2rust_package.dart \
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
