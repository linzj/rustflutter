#!/bin/sh
# The third ruler: every fixture, in one sweep.
#
#     bin/allfx.sh
#     binaryfloatrecv    AGREE
#     ...
#     ffistruct          error: could not compile `pk_ffistruct` ...
#
# One line per fixture in `fx/`: what `bin/fx.sh` said last. `AGREE` means the
# translated Rust and the Dart VM printed the same thing. Two fixtures are
# *deliberately* red and are expected to stay that way -- `ffistruct` (a
# `dart:ffi` struct the translator refuses on purpose) and `forinmut` (the
# acceptance test for the withdrawn `mut` rule, see 撤回与作废 in STATUS.md).
# Anything else red is a regression.
#
# `cargo` runs here, so this must not run beside `bin/run_chain.sh` (see the
# note there about two `cargo check`s taking the VM down).
#
# Written down 2026-09-10. It had been typed by hand every round, so when the
# fixture directory was lost there was no record of even how it was swept.
set -u
here=$(cd "$(dirname "$0")/.." && pwd)
src=${DART2RUST_FX_SRC:-$here/fx}
cd "$here" || exit 1
for path in "$src"/*.dart; do
    name=$(basename "$path" .dart)
    out=$(bin/fx.sh "$name" 2>&1)
    if echo "$out" | grep -q '^AGREE$'; then
        printf '%-18s AGREE\n' "$name"
    else
        printf '%-18s %s\n' "$name" \
            "$(echo "$out" | grep -E '^(error|DISAGREE|BUILD FAILED|TRANSLATE FAILED|CARGO FAILED)' | tail -1)"
    fi
done
