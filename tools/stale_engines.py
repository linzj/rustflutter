"""Find engine libraries older than the FFI sources they were built from.

Written after an incident. `rf_paint_set_color_filter` and
`rf_paint_clear_color_filter` were added to `rustflutter_ffi.cc` and verified
by the usual gate, which builds `out/host_debug_unopt` and runs the Rust
tests there. Those tests link the **stub** engine, not the real one, so
nothing in the gate ever linked the C++ side.

The result was five static libraries -- one per output directory -- sitting
at builds that predated the two new functions, and the way it surfaced was an
undefined symbol in a *downstream* project three weeks later:

    lld-link: error: undefined symbol: rf_paint_set_color_filter

The gate now builds `rustflutter_engine` in every output directory, so this
should not recur. This exists because "should not recur" is a claim about my
memory, and a check is not.

  python tools/stale_engines.py          # the report; exit 1 if any is stale
"""
import os
import sys

import paths

SRC = os.path.join(paths.REPO, 'src')
OUT = paths.OUT

# The C++ that becomes `rustflutter_engine`. Not the whole engine: these are
# the files this project writes, and the ones a change here can outrun.
FFI = [
    os.path.join(SRC, 'flutter', 'rust', 'ffi'),
    os.path.join(SRC, 'flutter', 'rust', 'host'),
    os.path.join(SRC, 'flutter', 'runtime', 'runtime_controller.cc'),
    os.path.join(SRC, 'flutter', 'runtime', 'rust_app_api.h'),
]

LIBS = ['rustflutter_engine.lib', 'librustflutter_engine.a']


def newest_source():
    """When the FFI last changed, and which file it was."""
    newest, which = 0.0, None
    for entry in FFI:
        if os.path.isfile(entry):
            paths = [entry]
        elif os.path.isdir(entry):
            paths = [
                os.path.join(root, name)
                for root, _, files in os.walk(entry)
                for name in files
                if name.endswith(('.cc', '.h', '.cpp'))
            ]
        else:
            continue
        for path in paths:
            stamp = os.path.getmtime(path)
            if stamp > newest:
                newest, which = stamp, path
    return newest, which


def engines():
    """Every built engine library, by output directory."""
    found = []
    if not os.path.isdir(OUT):
        return found
    for name in sorted(os.listdir(OUT)):
        directory = os.path.join(OUT, name)
        if not os.path.isdir(directory):
            continue
        for lib in LIBS:
            path = os.path.join(directory, 'obj', 'flutter', 'rust', lib)
            if os.path.exists(path):
                found.append((name, path, os.path.getmtime(path)))
    return found


def main():
    newest, which = newest_source()
    if which is None:
        print('no FFI sources found')
        return 0
    print('newest FFI source: %s' % os.path.relpath(which, SRC).replace(os.sep, '/'))

    built = engines()
    if not built:
        print('no engine libraries built yet')
        return 0

    stale = []
    for name, path, stamp in built:
        state = 'stale' if stamp < newest else 'ok'
        if state == 'stale':
            stale.append(name)
        print('  %-24s %s' % (name, state))

    if stale:
        print()
        print('%d engine %s older than the FFI they were built from.'
              % (len(stale), 'library is' if len(stale) == 1 else 'libraries are'))
        print('A downstream project linking one of these gets an undefined')
        print('symbol for anything added since. Rebuild with:')
        for name in stale:
            print('  ninja -C out/%s rustflutter_engine' % name)
        return 1
    print()
    print('every engine library is at or ahead of the FFI sources')
    return 0


if __name__ == '__main__':
    sys.exit(main())
