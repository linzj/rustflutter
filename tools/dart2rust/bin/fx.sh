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
# **A panic is never a pass** (rule, 2026-09-11). Not a wrong answer, not a
# right answer reached by aborting -- any panic at all, whatever the exit
# status says. AGREE means the two ends computed the same thing the same way:
# a Dart `throw` is a `Result` on this side, so a Rust panic means the
# translation dropped a path Dart can still catch.
#
# The gate below fires on the exit status *and* on the run's stderr, because
# passing the status alone was not enough:
#
#   * the fixture crates `bin/fx/build.py` writes have no `panic = "abort"`,
#     so they unwind where the gallery's workspace aborts;
#   * `run_callback` in the prelude wraps every event-loop callback in
#     `catch_unwind`, reports the panic as `dart2rust: <what> panicked: ..`
#     and lets the loop go on -- so a fixture could panic inside a microtask,
#     print the right last line, exit 0 and read AGREE.
#
# What this rule asks for is larger than the gate. Measured 2026-09-11, the
# gallery's own translation stands at 29,439 `.unwrap()` (Dart's `x!` lowers
# to one, `expressions.dart:363`; `as` to `dart_cast_to(..).unwrap()`, 3,068
# of them), plus 48 `panic!("uncaught Dart exception: ..")` between the
# prelude and the generated code. `.unwrap_or*` -- 3,384 more -- is not in
# that number and is not a panic; counting `\.unwrap()` as an ERE swallows
# them, which is how this figure was first written down as 32,818.
# Every one of those unwraps is a Dart-visible throw
# -- `TypeError`, `Bad state`, `FormatException`, `ArgumentError` -- that
# Dart code can catch and this program cannot. Closing them is a
# whole-program analysis: each null-assert, cast and prelude throw becomes an
# `Err` on a path that already carries `Result`, and the callers that do not
# carry one have to start. That is the work; this line is the ruler for it.
#
# Legitimately still a panic, because it is a fact about *this translator*
# rather than about the program: a stub, a refusal (`dart2rust: not
# translated`), TFA-dead code (`unreachable!`), and a native the host did not
# answer (`panic!("native ..")`).
#
# A fixture is a Dart *library* (no `main`) in `fx/` with a top-level
# synchronous `use()` returning a String. The **source** is in the repository
# and the build products are not: everything the run needs is built in
# `.build/fx/`, which .gitignore drops.
#
# Both halves lived in `~/dart2rust_build/fx` until 2026-09-10 -- outside the
# repository, where one `rm -rf` took the whole corpus with it. A fixture is
# an acceptance test; it belongs in the tree that keeps the rule it tests.
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
src=${DART2RUST_FX_SRC:-$here/fx}
work=${DART2RUST_FX:-$here/.build/fx}
name=${1:-}
[ -n "$name" ] || { echo "usage: bin/fx.sh <fixture name>   (fixtures in $src)" >&2; exit 2; }
[ -f "$src/$name.dart" ] || { echo "no fixture $name.dart in $src" >&2; exit 2; }
mkdir -p "$work"
# `bin/fx/build.py` writes beside the fixture it reads, so the source is
# copied into the build directory rather than built in place.
cp "$src/$name.dart" "$work/$name.dart"
cd "$here" || exit 1

log=$work/$name.build.log
if ! FX_MAIN="print(fx.use());" FX_AOT=${FX_AOT:-1} \
        python3 bin/fx/build.py "$work" "$name" > "$log" 2>&1; then
    echo "BUILD FAILED"; tail -25 "$log"; exit 1
fi
grep -q '^ok True' "$log" || { echo "TRANSLATE FAILED"; tail -25 "$log"; exit 1; }

# Translating is dart and costs no cargo; building and running is the whole
# cost of a sweep. The crate's Rust -- the fixture's module, the prelude,
# `lib.rs` -- is exactly what cargo would compile, so when it comes out
# byte-identical to the last run that AGREED, the answer is the same answer
# and there is nothing to learn from compiling it again.
#
# `DART2RUST_FX_FORCE=1` compiles anyway.
stamp=$work/$name.agreed
# The rule's name is part of the stamp, so a stamp written under an older
# rule does not match and the fixture is re-run once against the new one.
# Every `.agreed` on disk before 2026-09-11 predates the panic gate.
now="panicgate1 $(cat "$work/pk_$name"/*.rs 2>/dev/null | md5sum | cut -d' ' -f1)"
if [ "${DART2RUST_FX_FORCE:-0}" != 1 ] && [ -s "$stamp" ] &&
        [ "$now" = "$(cat "$stamp")" ]; then
    echo "--- rust: $(cat "$work/$name.rust.out" 2>/dev/null)"
    echo "--- dart: $(cat "$work/$name.dart.out" 2>/dev/null)"
    echo AGREE
    exit 0
fi
rm -f "$stamp"

( cd "$work/pk_$name" && cargo run -q -j "${DART2RUST_JOBS:-2}" --bin run ) \
    2> "$work/$name.cargo.log" | tail -1 > "$work/$name.rust.out"
rc=${PIPESTATUS[0]}

dart run --packages="$HOME/gallery_upstream/.dart_tool/package_config.json" \
    "$work/entry_$name.dart" 2> "$work/$name.dart.err" \
    | tail -1 > "$work/$name.dart.out"

echo "--- rust: $(cat "$work/$name.rust.out")"
echo "--- dart: $(cat "$work/$name.dart.out")"
# A panic is never a pass -- see the rule at the top. Checked before the exit
# status, because a panic `run_callback` swallowed leaves the status at 0.
panics="thread '[^']*' panicked at|^dart2rust: .* panicked:"
if grep -qE "$panics" "$work/$name.cargo.log"; then
    echo PANICKED; grep -m3 -E "$panics" "$work/$name.cargo.log"; exit 1
fi
[ "$rc" -eq 0 ] || { echo "CARGO FAILED"; tail -30 "$work/$name.cargo.log"; exit 1; }
if diff -q "$work/$name.rust.out" "$work/$name.dart.out" > /dev/null; then
    printf '%s\n' "$now" > "$stamp"
    echo AGREE
else
    echo DISAGREE; exit 1
fi
