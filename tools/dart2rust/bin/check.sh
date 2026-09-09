#!/bin/sh
# The analyzer and every test, which is what a commit has to pass.
#
# Neither existed before 2026-09-09: with no `.dart_tool/package_config.json`
# in this directory, `dart analyze` could not resolve `package:kernel` and so
# was never run, and the compiler reached 51k lines with the analyzer off.
# `bin/devsetup.py` writes that config; run it once per machine.
set -u
here=$(cd "$(dirname "$0")/.." && pwd)
cd "$here"

if [ ! -f .dart_tool/package_config.json ]; then
    echo "no .dart_tool/package_config.json -- run: python3 bin/devsetup.py" >&2
    exit 1
fi

. "$here/bin/experiments.sh"

dart=$(command -v dart || true)
[ -n "$dart" ] || { echo "no dart on PATH" >&2; exit 1; }

echo "== dart format (via bin/fmt.py: dart_style cannot read `augment`) =="
python3 bin/fmt.py --check || status_fmt=1

echo
echo "== state a refusal would leave behind =="
python3 bin/statecheck.py || status_state=1

echo
echo "== dart analyze =="
# An error fails the run. It could not before: `lib/frontend.dart`, the
# analyzer front end the Kernel one is checked against, had stopped compiling
# against analyzer 14's rebuilt AST (47 errors), so the gate was written as
# `|| true` -- which swallowed every *other* file's errors with it. The front
# end was migrated and the count is zero, so the gate is real again.
#
# Warnings and infos do not fail on their own -- 86 of them stand (guards the
# Kernel API's tightened nullability made dead, casts the analyser can prove
# redundant, deprecations), and making them fatal in one step would fail every
# commit until they are gone. They are a queue rather than a gate. But a queue
# with no ruler is what let this file's `|| true` sit here, so the *count* is
# the gate: it may fall, never rise, and when it falls this number comes down
# with it in the same commit.
analyze_ceiling=86
analyze_out=$(dart analyze --no-fatal-warnings lib bin test 2>&1) || status_analyze=1
printf '%s\n' "$analyze_out"
issues=$(printf '%s\n' "$analyze_out" |
    sed -n 's/^\([0-9][0-9]*\) issues* found\.$/\1/p')
[ -n "$issues" ] || issues=0
if [ "$issues" -gt "$analyze_ceiling" ]; then
    echo "$issues issues, and $analyze_ceiling is the ceiling: fix them, do" \
         "not raise it" >&2
    status_analyze=1
elif [ "$issues" -lt "$analyze_ceiling" ]; then
    echo "$issues issues, under the ceiling of $analyze_ceiling -- lower" \
         "analyze_ceiling in bin/check.sh to $issues" >&2
    status_analyze=1
fi

echo
echo "== tests =="
status=$(( ${status_fmt:-0} | ${status_state:-0} | ${status_analyze:-0} ))
for t in test/*_test.dart; do
    dart run $DART2RUST_EXPERIMENTS "$t" || status=1
done
exit $status
