# -*- coding: utf-8 -*-
"""Build the gallery's AOT dill -- the chain's input -- inside the repository.

    python3 tools/dart2rust/bin/gallery_dill.py [--force]

Writes `tools/dart2rust/.build/gallery/app_aot_sig.dill`, which
`bin/run_chain.sh` reads. Already there and non-empty: nothing is done unless
`--force`.

The dill is `gen_kernel --aot --tfa --minimal-kernel` over the app's
`lib/main.dart` (see `bin/dill.py` for why `gen_kernel` and not the frontend
server, and why `--minimal-kernel`). It is a *closed world*: TFA has run, so
what is in it is what a release would compile, which is the whole reason the
chain measures against it rather than against the source.

It lived at `~/dart2rust_build/gallery/app_aot_sig.dill` until 2026-09-10 --
outside the repository, with no script that could put it back. One cleanup of
that directory took the dill, the whole fixture corpus and the render ruler's
reference with it, and nothing here said how to rebuild any of them. This file
is the half of that lesson that can be written down: a product this chain
cannot run without is either in the tree or has a script that regenerates it.
"""
import io
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
sys.path.insert(0, HERE)

import dill as dill_tool  # noqa: E402  (same directory, owns the toolchain)
from paths import APP, APP_PACKAGES  # noqa: E402

OUT = os.path.join(TOOL, '.build', 'gallery', 'app_aot_sig.dill')
ENTRY = os.path.join(APP, 'lib', 'main.dart')


def as_uri(path):
    return 'file://' + os.path.abspath(path).replace(os.sep, '/')


def main():
    force = '--force' in sys.argv[1:]
    if os.path.exists(OUT) and os.path.getsize(OUT) > 0 and not force:
        print('have', OUT, os.path.getsize(OUT), 'bytes (--force to rebuild)')
        return 0
    if not os.path.exists(ENTRY):
        print('no app entry at', ENTRY)
        return 1
    if not os.path.exists(APP_PACKAGES):
        print('no package_config at', APP_PACKAGES,
              '-- run `flutter pub get` in', APP)
        return 1
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    code = dill_tool.build(as_uri(ENTRY), APP_PACKAGES, OUT, aot=True)
    if code != 0 or not os.path.exists(OUT):
        print('gen_kernel failed:', code)
        return 1
    print('wrote', OUT, os.path.getsize(OUT), 'bytes')
    # The dill's identity is a number STATUS.md quotes beside every reading
    # (`dill 0700f1e5`), so print it here rather than leaving it to be looked
    # up: a number without the dill it was measured on is not a reading.
    import hashlib
    digest = hashlib.md5(io.open(OUT, 'rb').read()).hexdigest()
    print('md5', digest)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
