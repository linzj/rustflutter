#!/bin/bash
# One fixture cycle: translate <name>.dart, build it, run both ends, diff.
#
# This is the *second* ruler. `bin/run_chain.sh` says how much of the gallery
# translates and compiles; this says whether the Rust and the Dart compute the
# same answer. STATUS.md's method -- "one general rule per group, and a
# fixture that agrees with Dart before the rule lands" -- is this script.
#
#     bin/fx.sh identlist
#     --- rust: red/12|black/12
#     --- dart: red/12|black/12
#     AGREE
#
# A fixture is a Dart *library* (no `main`) in $DART2RUST_FX with a top-level
# synchronous `use()` returning a String. Everything the run needs is built
# beside it; nothing lands in the repository.
#
# It goes through the AOT pipeline (`FX_AOT=1`), so TFA runs and the input is
# the same shape the gallery's dill is -- a fixture built the non-AOT way has
# fields TFA would have removed and agrees where the gallery does not.
#
# `cargo` runs here, so this must not run beside `bin/run_chain.sh`: two
# `cargo check`s side by side took the whole VM down once (see run_chain.sh).
#
# Recorded here rather than in a scratch directory because STATUS.md has cited
# `fx.sh` since ws749 while the file itself lived under /tmp, where it would
# not have survived a reboot.
set -u
here=$(cd "$(dirname "$0")/.." && pwd)
work=${DART2RUST_FX:-$HOME/dart2rust_build/fx}
name=${1:-}
[ -n "$name" ] || { echo "usage: bin/fx.sh <fixture name>   (fixtures in $work)" >&2; exit 2; }
mkdir -p "$work"
cd "$here" || exit 1

log=$work/$name.build.log
if ! FX_MAIN="print(fx.use());" FX_AOT=${FX_AOT:-1} \
        python3 bin/fx/build.py "$work" "$name" > "$log" 2>&1; then
    echo "BUILD FAILED"; tail -25 "$log"; exit 1
fi
grep -q '^ok True' "$log" || { echo "TRANSLATE FAILED"; tail -25 "$log"; exit 1; }

( cd "$work/pk_$name" && cargo run -q -j "${DART2RUST_JOBS:-2}" --bin run ) \
    2> "$work/$name.cargo.log" | tail -1 > "$work/$name.rust.out"
rc=${PIPESTATUS[0]}

dart run --packages="$HOME/gallery_upstream/.dart_tool/package_config.json" \
    "$work/entry_$name.dart" 2> "$work/$name.dart.err" \
    | tail -1 > "$work/$name.dart.out"

echo "--- rust: $(cat "$work/$name.rust.out")"
echo "--- dart: $(cat "$work/$name.dart.out")"
[ "$rc" -eq 0 ] || { echo "CARGO FAILED"; tail -30 "$work/$name.cargo.log"; exit 1; }
if diff -q "$work/$name.rust.out" "$work/$name.dart.out" > /dev/null; then
    echo AGREE
else
    echo DISAGREE; exit 1
fi
