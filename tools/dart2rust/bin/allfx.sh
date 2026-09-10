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
# translated Rust and the Dart VM printed the same thing. Two fixtures are
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
# Run a few at a time: with the compile short-circuit in `fx.sh` most
# fixtures only translate, which is one `dart` process each and the whole
# cost of the sweep -- 9m42 of it, sequential, at ws972. `DART2RUST_FX_JOBS`
# is how many at once (default 3), and each one compiles at `-j 1`, so the
# rustc count is the same handful either way. Not more: the fixtures are
# small crates but they each build the prelude, and the memory floor this
# repository keeps (see run_chain.sh) is there because a compile that runs
# past the machine takes the whole VM with it.
cd "$here" || exit 1
jobs=${DART2RUST_FX_JOBS:-3}
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
                "$(echo "$out" | grep -E '^(error|DISAGREE|BUILD FAILED|TRANSLATE FAILED|CARGO FAILED)' | tail -1)"
        fi
    ) > "$results/$(printf '%04d' "$i").$name" &
    while [ "$(jobs -rp | wc -l)" -ge "$jobs" ]; do wait -n; done
done
wait
cat "$results"/*
