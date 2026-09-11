#!/bin/bash
# bash, not sh: the job control below (`wait -n`) is bash's.
# The third ruler: every fixture, in one sweep.
#
#     bin/allfx.sh
#     binaryfloatrecv    AGREE
#     ...
#     ffistruct          error: could not compile `pk_ffistruct` ...
#
# One line per fixture in `fx/`: what `bin/fx.sh` said last. `AGREE` means the
# translated Rust and the Dart VM printed the same thing *and nothing
# panicked* -- see the panic rule at the top of `bin/fx.sh`, which is the
# criterion this sweep reports. `PANICKED` is its own verdict rather than one
# of the `CARGO FAILED` lines, because a panic the program survived leaves
# the exit status at 0 and used to read AGREE.
#
# The first sweep after that rule landed re-runs every fixture: the stamp
# carries the rule's name, so every `.agreed` written before 2026-09-11 is
# stale by construction.
#
# Two fixtures are
# *deliberately* red and are expected to stay that way -- `ffistruct` and
# `forinmut` (the acceptance test for the withdrawn `mut` rule, see 撤回与作废
# in STATUS.md). Anything else red is a regression.
#
# `ffistruct` changed how it is red at ws1015 and the line here changed with
# it. It used to be "a `dart:ffi` struct the translator refuses on purpose",
# which stopped being true when the read half landed: it now builds and
# panics at its first *write*, where `_storeInt64` refuses. The write needs
# the aliasing decision (a `Uint8List` is a value, so a store lands in a
# copy). `ffistructread` is the green half.
#
# `cargo` runs here, so this must not run beside `bin/run_chain.sh` (see the
# note there about two `cargo check`s taking the VM down).
#
# Written down 2026-09-10. It had been typed by hand every round, so when the
# fixture directory was lost there was no record of even how it was swept.
set -u
here=$(cd "$(dirname "$0")/.." && pwd)
src=${DART2RUST_FX_SRC:-$here/fx}
#
# Run a few at a time. `DART2RUST_FX_JOBS` is how many at once (default 3),
# and each one compiles at `-j 1`, so the rustc count is the same handful
# either way. Not more: the fixtures are small crates but they each build
# the prelude, and the memory floor this repository keeps (see run_chain.sh)
# is there because a compile that runs past the machine takes the whole VM
# with it.
#
# What a sweep costs, measured 2026-09-11 (104 fixtures, 3 jobs):
#
#   nothing changed          8m00  ->  seconds   (the stamp is read first now)
#   only `lib/prelude.dart`  8m00  ->  ~2m       (no dill, no front end, no
#                                                 Dart side; one file copied)
#   the backend changed      8m00  ->  8m00      (everything must be re-derived)
#
# The middle row is the one that was worth the work: the prelude is what
# moves in most rounds, and it cannot change any generated file but
# `dart_prelude.rs`.
cd "$here" || exit 1
jobs=${DART2RUST_FX_JOBS:-3}

# The two fingerprints, computed once for the whole sweep rather than 104
# times, and the prelude written once rather than raced for. `fx.sh` reads
# its stamp against these *before* it translates anything; see
# `bin/fx/fingerprint.sh`.
eval "$(bin/fx/fingerprint.sh)" || exit 2
[ -n "${fp_code:-}" ] && [ -n "${fp_prelude:-}" ] ||
    { echo "no fingerprint (is dart on PATH?)" >&2; exit 2; }
export DART2RUST_FX_FP_CODE=$fp_code DART2RUST_FX_FP_PRELUDE=$fp_prelude
work=${DART2RUST_FX:-$here/.build/fx}
mkdir -p "$work"
if [ ! -s "$work/prelude.$fp_prelude.rs" ]; then
    dart run bin/fx/emit_prelude.dart "$work/prelude.$fp_prelude.rs.tmp" || exit 2
    mv -f "$work/prelude.$fp_prelude.rs.tmp" "$work/prelude.$fp_prelude.rs"
fi
# ..and the older ones go: one is 400 KB and they are worth nothing once the
# sweep has moved on.
find "$work" -maxdepth 1 -name 'prelude.*.rs' \
    ! -name "prelude.$fp_prelude.rs" -delete

results=$(mktemp -d)
trap 'rm -rf "$results"' EXIT
i=0
for path in "$src"/*.dart; do
    name=$(basename "$path" .dart)
    i=$((i + 1))
    (
        out=$(DART2RUST_JOBS=1 bin/fx.sh "$name" 2>&1)
        if echo "$out" | grep -q '^AGREE$'; then
            printf '%-18s AGREE\n' "$name"
        else
            printf '%-18s %s\n' "$name" \
                "$(echo "$out" | grep -E '^(error|DISAGREE|PANICKED|BUILD FAILED|TRANSLATE FAILED|CARGO FAILED)' | tail -1)"
        fi
    ) > "$results/$(printf '%04d' "$i").$name" &
    while [ "$(jobs -rp | wc -l)" -ge "$jobs" ]; do wait -n; done
done
wait
cat "$results"/*
