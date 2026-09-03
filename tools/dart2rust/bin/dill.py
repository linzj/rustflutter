# -*- coding: utf-8 -*-
"""Build an app.dill, and set up `package:kernel` to read it.

Both halves need the **same Dart revision**, and that is the whole reason this
script exists rather than a line in a README. The Flutter SDK at
`bin/cache/dart-sdk` and the Dart checkout inside the engine are different
revisions, and the Kernel binary format is versioned: reading a dill built by
one with the `pkg/kernel` of the other fails with

    Unexpected Kernel Format Version 140 (expected 139)

The engine checkout happens to carry a complete matched set -- its own built
`dart-sdk`, a `dartaotruntime`, the `frontend_server` snapshot, the patched
platform, and `pkg/kernel` -- all at the revision in `out/<mode>/dart-sdk/revision`.
So everything here is taken from that one tree and nothing from the Flutter SDK.

    python tools/dart2rust/bin/dill.py --check
    python tools/dart2rust/bin/dill.py --config <out.json>
    python tools/dart2rust/bin/dill.py --build package:gallery/main.dart \
        --packages <app>/.dart_tool/package_config.json -o app.dill
"""
import argparse
import io
import json
import os
import subprocess
import sys

# The engine checkout. Found once, here, so a move is one edit.
#
# `pkg/kernel` is under engine/src/**flutter**/third_party/dart, not
# engine/src/third_party -- a search that stopped one level short of this path
# is what produced the earlier, wrong conclusion that Kernel was unobtainable.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from paths import ENGINE, exe  # noqa: E402

DART_PKG = os.path.join(ENGINE, 'flutter', 'third_party', 'dart', 'pkg')


def out_dir():
    """The engine build whose dart-sdk matches `pkg/kernel`."""
    for mode in ('host_release', 'host_profile_unopt', 'host_debug_unopt'):
        candidate = os.path.join(ENGINE, 'out', mode)
        if os.path.isdir(os.path.join(candidate, 'dart-sdk', 'bin')):
            return candidate
    raise SystemExit('no engine out/ with a built dart-sdk under %s' % ENGINE)


def paths():
    out = out_dir()
    sdk = os.path.join(out, 'dart-sdk')
    return {
        'out': out,
        'dart': os.path.join(sdk, 'bin', exe('dart')),
        'aot': os.path.join(sdk, 'bin', exe('dartaotruntime')),
        'frontend_server': os.path.join(
            out, 'gen', 'frontend_server_aot.dart.snapshot'),
        'platform': os.path.join(out, 'flutter_patched_sdk'),
        'kernel': os.path.join(DART_PKG, 'kernel'),
        'fe_shared': os.path.join(DART_PKG, '_fe_analyzer_shared'),
        'revision': os.path.join(sdk, 'revision'),
    }


def check():
    found = paths()
    ok = True
    for name, path in found.items():
        exists = os.path.exists(path)
        ok = ok and exists
        print('%-16s %s  %s' % (name, 'OK  ' if exists else 'MISSING', path))
    if os.path.exists(found['revision']):
        print('revision:', io.open(found['revision']).read().strip())
    return 0 if ok else 1


def write_config(target, extra_root=None):
    """A package_config that resolves `package:kernel` from the checkout.

    Written rather than committed because the paths are absolute and belong to
    this machine; the one place they come from is `ENGINE` above.
    """
    found = paths()
    packages = [
        {'name': 'kernel', 'rootUri': 'file:///' + found['kernel'].replace('\\', '/'),
         'packageUri': 'lib/', 'languageVersion': '3.13'},
        {'name': '_fe_analyzer_shared',
         'rootUri': 'file:///' + found['fe_shared'].replace('\\', '/'),
         'packageUri': 'lib/', 'languageVersion': '3.13'},
    ]
    if extra_root:
        packages.append({
            'name': os.path.basename(extra_root.rstrip('/\\')),
            'rootUri': 'file:///' + os.path.abspath(extra_root).replace('\\', '/'),
            'packageUri': '', 'languageVersion': '3.13'})
    os.makedirs(os.path.dirname(os.path.abspath(target)) or '.', exist_ok=True)
    io.open(target, 'w', encoding='utf-8', newline='\n').write(
        json.dumps({'configVersion': 2, 'packages': packages}, indent=2))
    print('wrote', target)
    return 0


def build(entry, app_packages, output):
    found = paths()
    command = [
        found['aot'], found['frontend_server'],
        '--sdk-root', found['platform'] + os.sep,
        '--target=flutter',
        '--packages', os.path.abspath(app_packages),
        '--output-dill', os.path.abspath(output),
        '--no-print-incremental-dependencies',
        entry,
    ]
    print(' '.join(command))
    result = subprocess.run(command, capture_output=True, text=True,
                            errors='replace')
    text = (result.stdout or '') + (result.stderr or '')
    # The frontend server reports compile errors on stdout and still exits 0,
    # so the exit code alone is not the answer.
    problems = [l for l in text.splitlines()
                if 'Error:' in l or l.startswith('org-dartlang')]
    if problems:
        print('%d problem line(s):' % len(problems))
        for line in problems[:10]:
            print('  ', line.strip()[:160])
    if not os.path.exists(output):
        print('no dill written')
        return 1
    print('%s: %.1f MB' % (output, os.path.getsize(output) / 1e6))
    return 1 if problems else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--config', metavar='FILE')
    parser.add_argument('--config-root', metavar='DIR',
                        help='an extra package rooted here, for a probe script')
    parser.add_argument('--build', metavar='ENTRY')
    parser.add_argument('--packages', metavar='FILE')
    parser.add_argument('-o', '--output', metavar='FILE')
    args = parser.parse_args()

    if args.check:
        return check()
    if args.config:
        return write_config(args.config, args.config_root)
    if args.build:
        if not args.packages or not args.output:
            raise SystemExit('--build needs --packages and -o')
        return build(args.build, args.packages, args.output)
    parser.print_help()
    return 2


if __name__ == '__main__':
    sys.exit(main())
