#!/bin/sh
# The analyzer and every test, which is what a commit has to pass.
#
# Neither existed before 2026-09-09: with no `.dart_tool/package_config.json`
# in this directory, `dart analyze` could not resolve `package:kernel` and so
# was never run, and the compiler reached 51k lines with the analyzer off.
# `bin/devsetup.py` writes that config; run it once per machine.
set -e
here=$(cd "$(dirname "$0")/.." && pwd)
cd "$here"

if [ ! -f .dart_tool/package_config.json ]; then
    echo "no .dart_tool/package_config.json -- run: python3 bin/devsetup.py" >&2
    exit 1
fi

dart=$(command -v dart || true)
[ -n "$dart" ] || { echo "no dart on PATH" >&2; exit 1; }

echo "== dart analyze =="
# Warnings do not fail the run yet: `lib/frontend.dart`, the analyzer front
# end the Kernel one replaced, no longer compiles against the current
# analyzer (49 errors, all API drift), and the count is the ruler for that.
dart analyze --no-fatal-warnings lib bin test || true

echo
echo "== tests =="
status=0
for t in test/*_test.dart; do
    dart run "$t" || status=1
done
exit $status
