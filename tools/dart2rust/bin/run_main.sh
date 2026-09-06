#!/bin/sh
# The runtime ruler: build the translated program's entry (`dart_main`, see
# workspace.py) out of the *stubbed* workspace the chain left behind, run it,
# and log how far it gets. A stub or a refusal on the path panics with its
# reason; that panic is the next thing to translate.
#
#   bin/run_main.sh <log>
#
# Under the same memory guard as the chain (see run_chain.sh), and never
# beside it: the chain edits the workspace this builds from.
set -u
log=$1
: "${DART2RUST_JOBS:=4}"
: "${DART2RUST_MIN_FREE_GB:=16}"
here=$(cd "$(dirname "$0")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
cd "$here/.crate-ws" || exit 2
(
  cargo build -p dart_main -j "$DART2RUST_JOBS" 2>&1 | grep -E '^error' -A12 | head -80
  echo "BUILD-DONE"
  # The whole run to its own file, then the status: through `head` the
  # program died of SIGPIPE past 60 lines and the status was `head`'s.
  # A budget inside the program (it reports and dumps at the end of it; a
  # periodic timer keeps a Flutter app alive for ever), `timeout` behind it.
  # The assets a `flutter build` of the program left, for the runtime's
  # `flutter/assets` (see runtime/src/lib.rs); unset means no assets.
  DART2RUST_ASSETS="${DART2RUST_ASSETS:-$HOME/gallery_upstream/build/flutter_assets}" \
  DART2RUST_RUN_SECONDS="${DART2RUST_RUN_SECONDS:-60}" RUST_BACKTRACE=1 timeout 120 ./target/debug/dart_main > "$log.run" 2>&1
  echo "RUN-DONE exit=$?"
  head -c 400000 "$log.run"
) > "$log" 2>&1 &
chain=$!
floor_kb=$((DART2RUST_MIN_FREE_GB * 1024 * 1024))
while kill -0 "$chain" 2>/dev/null; do
  avail=$(awk '/MemAvailable/ {print $2}' /proc/meminfo)
  if [ "$avail" -lt "$floor_kb" ]; then
    echo "OOM-GUARD: MemAvailable ${avail}kB under ${floor_kb}kB; killing the build" >> "$log"
    pkill -x rustc
    pkill -x cargo
    sleep 2
    pkill -9 -x rustc
    exit 3
  fi
  sleep 2
done
wait "$chain"
