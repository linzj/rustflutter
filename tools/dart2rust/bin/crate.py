# -*- coding: utf-8 -*-
"""Emit a package as a crate and `cargo check` it, incrementally.

    python tools/dart2rust/bin/crate.py <app.dill> [--prefix package:flutter/]

Round 38 measured the whole of `package:flutter` by handing 525 modules to
`rustc` in one go. That works and it takes about twenty minutes, which made a
round that ran it four times cost more in waiting than in work.

So the crate lives in one place with a `target/` that survives between runs,
and this uses `cargo check`, which reuses it. A re-run after a small change
costs a fraction of the first one. `--fresh` throws the cache away when the
question really is "from nothing".

Reports the error histogram and what the errors say is missing, which is the
ruler round 37 and 38 established: the census counts members emitted, and this
counts what compiles.
"""
import argparse
import io
import os
import re
import subprocess
import sys
from collections import Counter

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
REPO = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
CRATE = os.path.join(TOOL, '.crate')

sys.path.insert(0, HERE)
import dill as dill_tool  # noqa: E402

CARGO_TOML = """[package]
name = "flutter_translated"
version = "0.0.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[profile.dev]
debug = false
"""

MISSING = re.compile(r"cannot find (?:type|value|function|struct|trait) "
                     r"`([^`]+)`|undeclared type `([^`]+)`|"
                     r"unlinked crate `([^`]+)`")
CODE = re.compile(r"^error\[([A-Z0-9]+)\]")


def run(command, cwd=REPO, timeout=None):
    return subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                          errors='replace', timeout=timeout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('dill')
    parser.add_argument('--prefix', default='package:flutter/')
    parser.add_argument('--fresh', action='store_true',
                        help='drop the incremental cache first')
    args = parser.parse_args()

    scratch = os.path.join(TOOL, '.agree')
    os.makedirs(scratch, exist_ok=True)
    config = os.path.join(scratch, 'kernel_package_config.json')
    if not os.path.exists(config):
        dill_tool.write_config(config, TOOL)

    src = os.path.join(CRATE, 'src')
    os.makedirs(src, exist_ok=True)
    if args.fresh:
        import shutil
        shutil.rmtree(os.path.join(CRATE, 'target'), ignore_errors=True)
    io.open(os.path.join(CRATE, 'Cargo.toml'), 'w', encoding='utf-8',
            newline='\n').write(CARGO_TOML)

    paths = dill_tool.paths()
    r = run([paths['dart'], 'run', '--packages=' + config,
             HERE + '/dart2rust_package.dart', args.dill, args.prefix, src])
    if r.returncode != 0:
        print(r.stdout)
        print(r.stderr[:2000])
        return 1
    print(r.stdout.strip())

    r = run(['cargo', 'check', '--message-format=short'], cwd=CRATE,
            timeout=3600)
    text = (r.stdout or '') + (r.stderr or '')
    codes = Counter()
    missing = Counter()
    errors = 0
    for line in text.splitlines():
        if ': error[' in line or line.startswith('error['):
            errors += 1
            match = re.search(r'error\[([A-Z0-9]+)\]', line)
            if match:
                codes[match.group(1)] += 1
        elif ': error:' in line:
            errors += 1
            codes['(no code)'] += 1
        found = MISSING.search(line)
        if found:
            missing[next(g for g in found.groups() if g)] += 1

    print('')
    print('  errors: %d' % errors)
    for code, count in codes.most_common(10):
        print('    %6d  %s' % (count, code))
    print('')
    print('  most wanted names:')
    for name, count in missing.most_common(15):
        print('    %6d  %s' % (count, name))
    return 0


if __name__ == '__main__':
    sys.exit(main())
