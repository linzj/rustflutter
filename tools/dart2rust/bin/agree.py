# -*- coding: utf-8 -*-
"""Do the two front ends agree?

`frontend.dart` reads analyzer's resolved AST; `frontend_kernel.dart` reads the
toolchain's own `.dill`. They share the IR and the backend, so they should
produce Rust that behaves the same -- and if they do not, one of them has the
language wrong.

This makes that a check rather than something someone did once: it generates
`alignment.rs` from each front end in turn, runs the whole test crate against
each, and diffs the two outputs so the differences are visible instead of
assumed.

    python tools/dart2rust/bin/agree.py --dill <app.dill>

The dill and its package config come from `bin/dill.py`; run

    python tools/dart2rust/bin/dill.py --build package:gallery/main.dart \
        --packages <app>/.dart_tool/package_config.json -o app.dill

once, and pass the result here.
"""
import argparse
import difflib
import io
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
TESTDATA = os.path.join(os.path.dirname(HERE), 'testdata')
TARGET = os.path.join(TESTDATA, 'src', 'alignment.rs')
HEADER = 'use crate::{Offset, Rect, Size, TextDirection};\n\n'

UPSTREAM = 'E:/source/flutter/packages/flutter/lib/src/painting/alignment.dart'
LIBRARY = 'package:flutter/src/painting/alignment.dart'
FLUTTER_DART = 'E:/source/flutter/bin/cache/dart-sdk/bin/dart.exe'
FLUTTER_PKGS = '--packages=E:/source/flutter/.dart_tool/package_config.json'

sys.path.insert(0, HERE)
import dill as dill_tool  # noqa: E402  -- for the matched toolchain paths


def run(command, cwd=REPO):
    return subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                          errors='replace')


def from_analyzer(out):
    r = run([FLUTTER_DART, 'run', FLUTTER_PKGS,
             'tools/dart2rust/bin/dart2rust.dart', UPSTREAM, '--all', '-o', out])
    return r.returncode == 0, (r.stderr or '')


def from_kernel(out, dill_path, config):
    paths = dill_tool.paths()
    r = run([paths['dart'], 'run', '--packages=' + config,
             'tools/dart2rust/bin/dart2rust_kernel.dart', dill_path, LIBRARY,
             '-o', out])
    return r.returncode == 0, (r.stderr or '')


def install(source):
    text = io.open(source, encoding='utf-8').read()
    io.open(TARGET, 'w', encoding='utf-8', newline='\n').write(HEADER + text)
    subprocess.run(['rustfmt', '--edition', '2021', TARGET], capture_output=True)


def tests():
    r = run(['cargo', 'test'], cwd=TESTDATA)
    out = (r.stdout or '') + (r.stderr or '')
    if 'error[' in out or 'could not compile' in out:
        return 'DOES NOT BUILD'
    for line in out.splitlines():
        if line.startswith('test result') and 'measured' in line:
            return line.split(':')[1].strip()
    return 'NO RESULT'


def code_lines(path):
    """Comparable lines: doc comments are dropped before diffing.

    Not because they do not matter -- they do, and Kernel losing them is a real
    cost of the dill route, recorded in STATUS.md. But a diff drowned in one
    front end's comments cannot show a difference in the code.
    """
    out = []
    for line in io.open(path, encoding='utf-8'):
        stripped = line.strip()
        if stripped.startswith('///') or stripped.startswith('//') or not stripped:
            continue
        out.append(stripped)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dill', required=True)
    parser.add_argument('--config', help='kernel package config; made if absent')
    parser.add_argument('--keep', choices=['analyzer', 'kernel'],
                        default='analyzer',
                        help='which output to leave installed (default analyzer)')
    args = parser.parse_args()

    scratch = os.path.join(TESTDATA, '..', '.agree')
    os.makedirs(scratch, exist_ok=True)
    config = args.config or os.path.join(scratch, 'kernel_package_config.json')
    if not os.path.exists(config):
        dill_tool.write_config(config, os.path.dirname(HERE))

    saved = io.open(TARGET, encoding='utf-8').read()
    results = {}
    outputs = {}
    try:
        for name, produce in (
            ('analyzer', lambda o: from_analyzer(o)),
            ('kernel', lambda o: from_kernel(o, args.dill, config)),
        ):
            path = os.path.join(scratch, 'alignment_%s.rs' % name)
            ok, log = produce(path)
            if not ok or not os.path.exists(path):
                print('%-9s FRONT END FAILED' % name)
                for line in log.strip().splitlines()[:5]:
                    print('         ', line)
                results[name] = 'FAILED'
                continue
            outputs[name] = path
            install(path)
            results[name] = tests()
            print('%-9s %s' % (name, results[name]))
    finally:
        io.open(TARGET, 'w', encoding='utf-8', newline='\n').write(saved)

    if len(outputs) == 2:
        a = code_lines(outputs['analyzer'])
        k = code_lines(outputs['kernel'])
        diff = [l for l in difflib.unified_diff(a, k, 'analyzer', 'kernel',
                                                lineterm='', n=0)
                if l.startswith(('+', '-')) and not l.startswith(('+++', '---'))]
        print()
        print('%d code lines differ (of %d analyzer / %d kernel)'
              % (len(diff), len(a), len(k)))
        for line in diff[:20]:
            print('  ', line[:120])

    if args.keep in outputs:
        install(outputs[args.keep])
        print()
        print('left installed:', args.keep)

    agreed = all(v.startswith('ok.') for v in results.values())
    print()
    print('AGREE' if agreed and len(results) == 2 else 'DISAGREE')
    return 0 if agreed and len(results) == 2 else 1


if __name__ == '__main__':
    sys.exit(main())
