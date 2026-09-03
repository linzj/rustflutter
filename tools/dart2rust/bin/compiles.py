# -*- coding: utf-8 -*-
"""Translate upstream libraries and try to *compile* them.

    python tools/dart2rust/bin/compiles.py <app.dill> [--libraries 40]

The census counts members **emitted**. That is not the same as members that
compile, and round 32 showed the gap the hard way: `xs.length` was emitted as
`.length()` against a bare `List` for months, counted as translated, and could
never have built. This ruler asks the other question -- run rustc over the
output and see.

Each library is compiled on its own, with no stubs and nothing else in scope,
so almost none of them will build. That *is* the measurement: what the output
reaches for and does not have is the shopping list for a minimal `dart:core`
and `dart:ui`, which is the wall everything else has been queuing behind.

Reported: how many libraries compile, and what the ones that do not are
missing, ranked. Names, not counts of errors -- ten uses of one missing type
is one thing to build.
"""
import argparse
import io
import os
import re
import subprocess
import sys
import tempfile
from collections import Counter
from concurrent import futures

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
REPO = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))

sys.path.insert(0, HERE)
import dill as dill_tool  # noqa: E402

MISSING = re.compile(
    r"cannot find (?:type|value|function|struct|trait|attribute macro) "
    r"`([^`]+)`|"
    r"failed to resolve: use of (?:unresolved module or unlinked crate|"
    r"undeclared type) `([^`]+)`|"
    r"cannot find (?:type|value) `([^`]+)` in this scope")


def run(command, cwd=REPO, timeout=None):
    return subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                          errors='replace', timeout=timeout)


def libraries(dill, prefix):
    paths = dill_tool.paths()
    r = run([paths['dart'], 'run', '--packages=' + CONFIG,
             HERE + '/dart2rust_kernel.dart', dill, prefix, '--list',
             '--all'])
    # `--list` prints a class count and the uri; take the uri.
    out = []
    for line in r.stdout.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[1].startswith('package:'):
            out.append(parts[1])
    return out


def examine(dill, uri, work):
    stem = uri.rsplit('/', 1)[-1].replace('.dart', '')
    out = os.path.join(work, stem + '.rs')
    paths = dill_tool.paths()
    r = run([paths['dart'], 'run', '--packages=' + CONFIG,
             HERE + '/dart2rust_kernel.dart', dill, uri, '-o', out])
    if r.returncode != 0 or not os.path.exists(out):
        return uri, None, ['<did not translate>']
    r = run(['rustc', '--edition', '2021', '--crate-type', 'lib',
             '--emit=metadata', '-o', out + '.meta', '-A', 'warnings', out])
    if r.returncode == 0:
        return uri, True, []
    missing = []
    for match in MISSING.finditer(r.stderr):
        missing.append(next(g for g in match.groups() if g))
    return uri, False, missing


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('dill')
    parser.add_argument('--prefix', default='package:flutter/')
    parser.add_argument('--libraries', type=int, default=40)
    args = parser.parse_args()

    global CONFIG
    scratch = os.path.join(TOOL, '.agree')
    os.makedirs(scratch, exist_ok=True)
    CONFIG = os.path.join(scratch, 'kernel_package_config.json')
    if not os.path.exists(CONFIG):
        dill_tool.write_config(CONFIG, TOOL)

    uris = libraries(args.dill, args.prefix)[:args.libraries]
    if not uris:
        raise SystemExit('no libraries matched %s' % args.prefix)

    work = tempfile.mkdtemp(prefix='d2r_compiles_')
    compiled = 0
    missing = Counter()
    failed = 0
    with futures.ThreadPoolExecutor(max_workers=min(len(uris), 16)) as pool:
        for uri, ok, names in pool.map(
                lambda u: examine(args.dill, u, work), uris):
            if ok:
                compiled += 1
            else:
                failed += 1
                # Counted once per library, not once per use: ten uses of one
                # missing type is one thing to build.
                for name in set(names):
                    missing[name] += 1

    print('%s, %d libraries' % (args.prefix, len(uris)))
    print('  compile on their own: %d' % compiled)
    print('  do not:               %d' % failed)
    print('')
    print('  what they reach for and do not have (libraries needing it):')
    for name, count in missing.most_common(30):
        print('    %5d  %s' % (count, name))
    return 0


if __name__ == '__main__':
    sys.exit(main())
