#!/bin/sh
# The two things a fixture's answer can depend on, hashed separately.
#
#     eval "$(bin/fx/fingerprint.sh)"   # sets fp_code and fp_prelude
#
# `bin/fx.sh` used to translate every fixture before it looked at its stamp,
# so the cache saved the cargo step (~3.5s) and paid the translation (~10.6s)
# every time -- a sweep over 104 unchanged fixtures re-derived, from the
# dill up, an answer it already had. Measured 2026-09-11: 8 minutes for a
# corpus that had not moved.
#
# Split in two because the two halves have different inputs, and the split
# is provable rather than hopeful:
#
#   * `lib/prelude.dart` declares exactly one top-level name, `const
#     rustPrelude`, and `bin/dart2rust_package.dart` uses it in exactly one
#     place -- written verbatim to `dart_prelude.rs`. So a prelude edit can
#     change that one file and no other. Nothing needs re-translating.
#   * everything else the translator is made of decides the fixture's own
#     module, so a change there does need the dill and the front end again.
#
# The Dart SDK is in the code half: it builds the dill and runs the other
# end of every fixture.
#
# The code half hashes every `.dart`, `.py` and `.tmpl` under `lib/` and
# `bin/`, which is wider than the pipeline: editing `bin/panic_ruler.py`
# costs one full sweep for nothing. That is the direction to be wrong in.
# The narrow list would be `lib/**`, `bin/dart2rust_package.dart`,
# `bin/{dill,paths,fixtures}.py` and `bin/fx/*` -- and the day someone adds
# an import to `bin/fx/build.py` without adding it here, the ruler starts
# answering from a cache that is stale, which is worse than slow.
here=$(cd "$(dirname "$0")/../.." && pwd)
cd "$here" || exit 1
# Named rather than found on PATH: a shell without the Flutter SDK on it
# would hash "dart: command not found", which is a stable string and would
# silently pin the fingerprint to the wrong thing.
dart=$(command -v dart || echo "$HOME/flutter_sdk/bin/dart")
[ -x "$dart" ] || { echo "fingerprint: no dart at $dart" >&2; exit 2; }
# ..and a third: what the *Dart* side alone depends on. A fixture's dill is
# a whole-program `gen_kernel --aot --tfa` over the Flutter platform (5.85s
# of the 10.6s a fixture costs, measured 2026-09-11), and its inputs are the
# fixture's source and the SDK -- no part of `lib/` reaches it. Neither does
# the Dart half of the comparison, which is the VM running that same source.
# So a backend change, which must re-derive every fixture's Rust, need not
# re-derive either of those.
echo "fp_sdk=$( { "$dart" --version 2>&1
    python3 -c "import os, sys; sys.path.insert(0, 'bin'); import dill; \
p = os.path.join(dill.paths()['platform'], 'platform_strong.dill'); \
st = os.stat(p); print(p, st.st_size, int(st.st_mtime))" 2>&1
    md5sum "$HOME/gallery_upstream/.dart_tool/package_config.json" 2>&1
  } | md5sum | cut -d' ' -f1)"
echo "fp_prelude=$(md5sum lib/prelude.dart | cut -d' ' -f1)"
echo "fp_code=$( { find lib bin -type f \( -name '*.dart' -o -name '*.py' \
        -o -name '*.tmpl' \) ! -path 'lib/prelude.dart' -print0 \
        | sort -z | xargs -0 md5sum
    "$dart" --version 2>&1; } | md5sum | cut -d' ' -f1)"
