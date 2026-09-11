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
cd "$here" || exit 1

# What this fixture's answer depends on, hashed in two parts (see
# `bin/fx/fingerprint.sh` for why the split is sound). `bin/allfx.sh`
# computes them once and passes them down; a lone `bin/fx.sh` computes its
# own.
if [ -n "${DART2RUST_FX_FP_CODE:-}" ] && [ -n "${DART2RUST_FX_FP_PRELUDE:-}" ] &&
        [ -n "${DART2RUST_FX_FP_SDK:-}" ]; then
    fp_code=$DART2RUST_FX_FP_CODE
    fp_prelude=$DART2RUST_FX_FP_PRELUDE
    fp_sdk=$DART2RUST_FX_FP_SDK
else
    eval "$(bin/fx/fingerprint.sh)" || exit 2
    [ -n "${fp_code:-}" ] && [ -n "${fp_prelude:-}" ] && [ -n "${fp_sdk:-}" ] ||
        { echo "no fingerprint (is dart on PATH?)" >&2; exit 2; }
fi
fixture_md5=$(md5sum "$src/$name.dart" | cut -d' ' -f1)
key_code=$(printf '%s %s\n' "$fp_code" "$fixture_md5" | md5sum | cut -d' ' -f1)
# The dill and the Dart half of the comparison share one key, because they
# share their inputs: this fixture's source and the SDK. A backend change
# moves `key_code` and not this one.
key_dart=$(printf '%s %s\n' "$fp_sdk" "$fixture_md5" | md5sum | cut -d' ' -f1)

# The stamp is read *before* anything runs. It used to be read after the
# translation, so the cache saved the cargo step and paid the dill, the
# front end and the package emitter every time: 10.6 of the 14 seconds a
# fixture cost, spent re-deriving an answer already on disk. A sweep over
# an unmoved corpus took 8 minutes for nothing (measured 2026-09-11).
#
# The rule's name is part of it, so a stamp written under an older rule
# does not match and the fixture is re-run once against the new one.
# `panicgate1` stamps predate this split and re-run once.
#
# `DART2RUST_FX_FORCE=1` does the whole thing anyway.
stamp=$work/$name.agreed
want="panicgate2 $key_code $fp_prelude"
force=${DART2RUST_FX_FORCE:-0}
if [ "$force" != 1 ] && [ -s "$stamp" ] && [ "$want" = "$(cat "$stamp")" ] &&
        [ -s "$work/$name.rust.out" ] && [ -s "$work/$name.dart.out" ]; then
    echo "--- rust: $(cat "$work/$name.rust.out")"
    echo "--- dart: $(cat "$work/$name.dart.out")"
    echo AGREE
    exit 0
fi
had_code=$(awk '{print $2}' "$stamp" 2>/dev/null)
rm -f "$stamp"

# Only the prelude moved: refresh that one file and leave the rest alone.
# `lib/prelude.dart` declares one name and the package emitter writes it
# verbatim, so the fixture's own module, the entry wrapper and the whole
# Dart side are provably unchanged -- and the Dart side is not re-run.
if [ "$force" != 1 ] && [ "$had_code" = "$key_code" ] &&
        [ -f "$work/pk_$name/dart_prelude.rs" ] && [ -s "$work/$name.dart.out" ]; then
    pre=$work/prelude.$fp_prelude.rs
    if [ ! -s "$pre" ]; then
        # Written elsewhere and moved into place: a sweep runs fixtures side
        # by side and two of them would otherwise write the same file at once.
        dart run bin/fx/emit_prelude.dart "$pre.$$" || { echo "PRELUDE FAILED"; exit 1; }
        mv -f "$pre.$$" "$pre"
    fi
    cp "$pre" "$work/pk_$name/dart_prelude.rs"
    reused_dart=1
else
    # `bin/fx/build.py` writes beside the fixture it reads, so the source is
    # copied into the build directory rather than built in place.
    cp "$src/$name.dart" "$work/$name.dart"
    log=$work/$name.build.log
    if ! FX_MAIN="print(fx.use());" FX_AOT=${FX_AOT:-1} FX_DILL_KEY="$key_dart" \
            python3 bin/fx/build.py "$work" "$name" > "$log" 2>&1; then
        echo "BUILD FAILED"; tail -25 "$log"; exit 1
    fi
    grep -q '^ok True' "$log" ||
        { echo "TRANSLATE FAILED"; tail -25 "$log"; exit 1; }
    reused_dart=0
fi

( cd "$work/pk_$name" && cargo run -q -j "${DART2RUST_JOBS:-2}" --bin run ) \
    2> "$work/$name.cargo.log" | tail -1 > "$work/$name.rust.out"
rc=${PIPESTATUS[0]}

# The other end is the Dart VM running the fixture's own source: the
# translator is not in its inputs, so a backend change does not move it
# either. Keyed like the dill, and skipped on the same terms.
if [ "$reused_dart" != 1 ] && [ -s "$work/$name.dart.out" ] &&
        [ "$key_dart" = "$(cat "$work/$name.dart.key" 2>/dev/null)" ]; then
    reused_dart=1
fi
if [ "$reused_dart" != 1 ]; then
    dart run --packages="$HOME/gallery_upstream/.dart_tool/package_config.json" \
        "$work/entry_$name.dart" 2> "$work/$name.dart.err" \
        | tail -1 > "$work/$name.dart.out"
    printf '%s\n' "$key_dart" > "$work/$name.dart.key"
fi

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
    printf '%s\n' "$want" > "$stamp"
    echo AGREE
else
    echo DISAGREE; exit 1
fi
