# -*- coding: utf-8 -*-
"""Regenerate every `testdata/src/*.rs` that has a fixture behind it.

    python tools/dart2rust/bin/regen.py           # all of them
    python tools/dart2rust/bin/regen.py loops     # one, by name

Two things this does that doing it by hand kept getting wrong:

* **Which front end.** Almost every file comes from the analyzer one, because
  that is what `dart2rust.dart` runs. `constinstance.rs` comes from the Kernel
  one -- the analyzer never meets an evaluated constant, so testing that work
  means testing Kernel's output. Forgetting which is which silently replaces a
  file with the other side's version.
* **The `use` line.** A generated file names `RangeError`, which lives in
  `lib.rs`, and nothing in the generator knows that. Regenerating by hand
  dropped the import and the crate stopped building. Decided by looking at the
  text rather than by remembering, so a new file gets it too.

Runs the fixtures at the same time: one Dart VM start each, serially, was most
of the wall clock.
"""
import argparse
import io
import os
import subprocess
import sys
from concurrent import futures

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
REPO = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
FIXTURES = os.path.join(TOOL, 'testdata', 'fixtures')
SRC = os.path.join(TOOL, 'testdata', 'src')

FLUTTER_DART = 'E:/source/flutter/bin/cache/dart-sdk/bin/dart.exe'
FLUTTER_PKGS = '--packages=E:/source/flutter/.dart_tool/package_config.json'

sys.path.insert(0, HERE)
import dill as dill_tool  # noqa: E402
import fixtures as fixtures_tool  # noqa: E402

# Files whose Rust must come from the Kernel front end.
FROM_KERNEL = {'constinstance'}

# Notes to put above a generated file. Written here rather than carried over
# from the previous version of the file: the generator writes comments of its
# own -- the refusal notices -- and a rule that kept "the comments at the top"
# copied those forward too, once per regeneration.
NOTES = {
    'constinstance': '''// Generated from fixtures/constinstance.dart by the **Kernel** front end.
//
// Every other file here comes from the analyzer one, because that is what
// dart2rust.dart runs. That round's work is Kernel-only -- the analyzer never
// meets an evaluated constant -- so testing it means testing that side's
// output. fixtures.py still checks the two agree on everything but the
// constants themselves, which the fixture declares with // DIFFERS:.

''',
}

# What a generated file may need from the crate root, and how to tell.
IMPORTS = {'RangeError': 'use crate::RangeError;',
           'Isolate': 'use crate::Isolate;',
           'DartAny': 'use crate::DartAny;',
           'Type': 'use crate::Type;',
           'Map': 'use crate::Map;',
           'Set': 'use crate::Set;'}


def needed_imports(text):
    lines = []
    for name, line in sorted(IMPORTS.items()):
        if name in text and line not in text:
            lines.append(line)
    return lines


def from_analyzer(fixture, out):
    r = subprocess.run(
        [FLUTTER_DART, 'run', FLUTTER_PKGS,
         os.path.join(HERE, 'dart2rust.dart'), fixture, '--all', '-o', out],
        cwd=REPO, capture_output=True, text=True, errors='replace')
    return r.returncode == 0, (r.stdout or '') + (r.stderr or '')


def write_prelude():
    """The fixture crate gets the same prelude the package crate does.

    It was copied by hand before -- `Isolate`, `Completer` and `RangeError`
    each written twice -- which is a second source of truth for exactly the
    thing a fixture exists to hold still.
    """
    source = io.open(os.path.join(TOOL, 'lib', 'prelude.dart'),
                     encoding='utf-8').read()
    opening = "const rustPrelude = r" + "'''"
    closing = "'''" + ";"
    start = source.index(opening) + len(opening)
    end = source.index(closing, start)
    text = source[start:end].lstrip('\n')
    out = os.path.join(SRC, 'dart_prelude.rs')
    if not os.path.exists(out) or io.open(out, encoding='utf-8').read() != text:
        io.open(out, 'w', encoding='utf-8', newline='\n').write(text)
        # The hook checks this crate with rustfmt. `prelude.dart` is written
        # for a reader rather than for rustfmt, so the copy is formatted here
        # instead of the source being bent to match.
        subprocess.run(['rustfmt', '--edition', '2021', out],
                       capture_output=True)


def regenerate(stem, config, work):
    fixture = os.path.join(FIXTURES, stem + '.dart')
    out = os.path.join(SRC, stem + '.rs')
    header = NOTES.get(stem, '')
    if stem in FROM_KERNEL:
        holder = os.path.join(work, stem)
        os.makedirs(holder, exist_ok=True)
        dill_path = fixtures_tool.build_dill(fixture, holder)
        if dill_path is None:
            return stem, 'DILL FAILED'
        ok, log = fixtures_tool.from_kernel(dill_path, fixture, out, config)
    else:
        ok, log = from_analyzer(fixture, out)
    if not ok:
        return stem, log.strip().splitlines()[:1]

    text = io.open(out, encoding='utf-8').read()
    imports = needed_imports(header + text)
    prefix = ('\n'.join(imports) + '\n\n') if imports else ''
    io.open(out, 'w', encoding='utf-8', newline='\n').write(
        prefix + header + text)
    subprocess.run(['rustfmt', '--edition', '2021', out], capture_output=True)
    return stem, 'ok%s' % (' (kernel)' if stem in FROM_KERNEL else '')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('names', nargs='*', help='fixture names; default all')
    args = parser.parse_args()

    scratch = os.path.join(TOOL, '.agree')
    os.makedirs(scratch, exist_ok=True)
    config = os.path.join(scratch, 'kernel_package_config.json')
    if not os.path.exists(config):
        dill_tool.write_config(config, TOOL)

    write_prelude()
    stems = sorted(
        f[:-5] for f in os.listdir(FIXTURES)
        if f.endswith('.dart')
        and os.path.exists(os.path.join(SRC, f[:-5] + '.rs')))
    if args.names:
        stems = [s for s in stems if s in args.names]
    if not stems:
        raise SystemExit('nothing to regenerate')

    failed = []
    workers = min(len(stems), 16)
    with futures.ThreadPoolExecutor(max_workers=workers) as pool:
        for stem, status in pool.map(
                lambda s: regenerate(s, config, scratch), stems):
            print('%-14s %s' % (stem, status))
            if status != 'ok' and status != 'ok (kernel)':
                failed.append(stem)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
