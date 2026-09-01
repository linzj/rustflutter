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


# Host sources only one platform compiles. Judging every engine against all of
# them makes a ruler that **cannot be cleared**: editing the Windows host marks
# the Android engines stale, and no rebuild fixes it because no Android target
# ever compiled that file. A permanently red instrument is worse than none --
# it trains the reader to ignore it.
PLATFORM_ONLY = {
    # `_vk` is here with the host's own sources because only the host build
    # compiles it: `ninja -C out/android_release_arm64 rustflutter_engine`
    # answers "no work to do" while this ruler calls that library stale
    # against `rustflutter_vk.cc` -- which is the permanently-red case above,
    # arriving by a new route. Move it the day an Android config builds
    # Vulkan.
    'win': ('_win.cc', '_win.h'),
    'mac': ('_mac.mm', '_mac.cc', '_mac.h', '_ios.mm', '_ios.cc', '_ios.h'),
    'android': ('_android.cc', '_android.h'),
    'linux': ('_linux.cc', '_linux.h'),
}

# Sources every **host** build compiles and no device build does. The Vulkan
# backend is the case that brought this table its second entry: only the host
# configuration builds `rustflutter_vk.cc`, and
# `ninja -C out/android_release_arm64 rustflutter_engine` answering "no work
# to do" while this ruler called that library stale against it is the
# permanently-red case again, arriving by a new route. Move it the day an
# Android configuration builds Vulkan.
HOST_ONLY = ('_vk.cc', '_vk.h')


def platform_of(out_dir):
    """Which platform an output directory builds for, by its name.

    A host directory builds for **this** machine, which is why the last line
    asks the interpreter rather than assuming Windows: on a Linux checkout the
    host sources ending in `_linux` are the ones that *are* compiled, and
    naming the wrong host there would exclude exactly the files that matter.
    """
    name = out_dir.lower()
    if 'android' in name:
        return 'android'
    if 'mac' in name or 'ios' in name:
        return 'mac'
    return 'linux' if sys.platform.startswith('linux') else 'win'


def compiled_for(path, platform):
    """Whether `path` is a source that `platform` actually builds."""
    name = os.path.basename(path)
    if name.endswith(HOST_ONLY) and platform in ('android', 'mac'):
        return False
    for other, suffixes in PLATFORM_ONLY.items():
        if other == platform:
            continue
        if name.endswith(suffixes):
            return False
    # Anything under host/android/ is the Android host's alone.
    normalised = path.replace(os.sep, '/')
    if '/host/android/' in normalised and platform != 'android':
        return False
    return True


def newest_source(platform=None):
    """When the FFI last changed, and which file it was.

    With a `platform`, only the sources that platform compiles are looked at.
    """
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
            if platform is not None and not compiled_for(path, platform):
                continue
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
    overall, which = newest_source()
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
        # Each engine is judged against **its own** platform's sources.
        newest, source = newest_source(platform_of(name))
        state = 'stale' if source is not None and stamp < newest else 'ok'
        if state == 'stale':
            stale.append(name)
            print('  %-24s %s (%s)'
                  % (name, state,
                     os.path.relpath(source, SRC).replace(os.sep, '/')))
        else:
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
