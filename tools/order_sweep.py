#!/usr/bin/env python3
r"""Mutates the *order* of a decision, and reports the swaps nothing catches.

Two ticks running lost time to the same shape: a resolver picks by some order,
every test sets one field at a time, and the order between fields is invisible
to all of them.  Each test looks complete on its own, which is exactly why
reading them does not turn it up.

So this asks mechanically, in the two places order lives:

  * `if states.contains(WidgetState::A) { ... } else if states.contains(B)` --
    swap the two conditions and leave the bodies alone, which is exactly a
    reordering and nothing else;
  * `x.field.or(y.field)` -- swap the two sides of a fallback chain.

A survivor is a question, not a verdict: either the order genuinely cannot
matter, or nothing is testing it.

# What it cannot see

Matches inside comments are skipped -- a doc comment quoting the expression it
explains would otherwise be "mutated" to no effect and reported as a survivor,
which happened three times on the first run.

The chain pattern allows a newline between a receiver, its field and the `.or`,
because rustfmt breaks long chains and the first version could only see the
ones that fit on a line: of `SystemUiOverlayStyle::copy_with`'s eight fields it
saw two.  It still cannot see a chain whose sides are anything but
`receiver.field` -- a method call, an index, a longer path.

Usage:
  python tools/order_sweep.py                # every file with either shape
  python tools/order_sweep.py src/foo.rs
"""

import io
import os
import re
import shutil
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CRATE = os.path.join(REPO, 'src', 'flutter', 'rust', 'rustflutter')

BREAK = r'\s*(?:\r?\n\s*)?'

BRANCH = re.compile(
    r'(if|\}\s*else if)\s+states\.contains\(WidgetState::(\w+)\)\s*\{')

CHAIN = re.compile(
    r'\b(\w+)' + BREAK + r'\.(\w+)' + BREAK + r'\.or\(' + BREAK
    + r'(\w+)' + BREAK + r'\.(\w+)' + BREAK + r'\)')


def branch_pairs(text):
    """Adjacent (earlier, later) branches of one if/else chain."""
    found = list(BRANCH.finditer(text))
    return [(a, b) for a, b in zip(found, found[1:])
            if b.group(1).startswith('}')]


def in_comment(text, at):
    """Whether `at` falls after a `//` on its own line."""
    line_start = text.rfind(chr(10), 0, at) + 1
    marker = text.find('//', line_start)
    return 0 <= marker < at


def chain_pairs(text):
    return [m for m in CHAIN.finditer(text) if not in_comment(text, m.start())]


def check(path, swapped, label, line_no):
    io.open(path, 'w', encoding='utf-8', newline='').write(swapped)
    result = subprocess.run(['cargo', 'test', '--lib'],
                            cwd=CRATE, capture_output=True, text=True)
    line = next((l for l in result.stdout.splitlines()
                 if l.startswith('test result')), '')
    caught = 'FAILED' in line or not line
    print(f'  {"caught" if caught else "SURVIVED":>8}  line {line_no}: {label}')
    return 0 if caught else 1


def run(paths):
    failures = 0
    for relative in paths:
        path = os.path.join(CRATE, relative)
        original = io.open(path, encoding='utf-8', newline='').read()
        branches = branch_pairs(original)
        chains = chain_pairs(original)
        if not branches and not chains:
            continue
        print(f'--- {relative}: {len(branches)} branch pairs, '
              f'{len(chains)} or-chains')
        backup = path + '.sweep'
        shutil.copyfile(path, backup)
        try:
            for a, b in branches:
                swapped = (original[:a.start(2)] + b.group(2)
                           + original[a.end(2):b.start(2)] + a.group(2)
                           + original[b.end(2):])
                failures += check(
                    path, swapped, f'{a.group(2)} <-> {b.group(2)}',
                    original[:a.start()].count(chr(10)) + 1)
            for match in chains:
                swapped = (
                    original[:match.start()]
                    + f'{match.group(3)}.{match.group(4)}'
                      f'.or({match.group(1)}.{match.group(2)})'
                    + original[match.end():])
                failures += check(
                    path, swapped,
                    f'{match.group(1)}.{match.group(2)} <-> '
                    f'{match.group(3)}.{match.group(4)}',
                    original[:match.start()].count(chr(10)) + 1)
        finally:
            shutil.copyfile(backup, path)
            os.remove(backup)
    print()
    print(f'{failures} swaps nothing noticed')
    return 1 if failures else 0


def main():
    if len(sys.argv) > 1:
        return run(sys.argv[1:])
    targets = []
    for root, _dirs, files in os.walk(os.path.join(CRATE, 'src')):
        for name in files:
            if not name.endswith('.rs'):
                continue
            path = os.path.join(root, name)
            text = io.open(path, encoding='utf-8', errors='replace').read()
            if BRANCH.search(text) or CHAIN.search(text):
                targets.append(os.path.relpath(path, CRATE))
    return run(sorted(targets))


if __name__ == '__main__':
    sys.exit(main())
