#!/usr/bin/env python3
"""Stub the functions that do not compile, so the rest of the workspace can.

cargo checks a crate only when every crate it depends on compiled, so a
handful of errors in a leaf crate hides the state of the hundred crates
above it. This tool runs `cargo check --workspace --keep-going`, finds the
function around each error, and replaces that function's body with

    todo!("dart2rust: stubbed, did not compile: <the error>")

then checks again, until nothing new fails or a round changes nothing. An
error outside any function body -- a signature, a struct, a static -- is
not something a stub can answer and is left as it is and reported.

The stubs are a *measurement*, not a translation: every one is listed in the
report (`--report`), and the count is the number the branch has to drive to
zero. Nothing here touches the compiler; it edits the generated workspace in
place, which the next dart2rust_package.dart run overwrites.

    python tools/dart2rust/bin/stubs.py            # .crate-ws, up to 12 rounds
    python tools/dart2rust/bin/stubs.py --rounds 3 --report stubs.txt
"""

import argparse
import io
import json
import os
import re
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)

FN = re.compile(r'^(\s*)(pub(\([a-z]+\))?\s+)?(async\s+)?(const\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)')


def cargo_errors(ws):
    """(file, line, message) for every error's primary span, plus the count of
    errors that had no span in the workspace."""
    p = subprocess.run(
        ['cargo', 'check', '--workspace', '--keep-going', '--message-format=json'],
        cwd=ws, capture_output=True, text=True)
    found = []
    for raw in p.stdout.splitlines():
        try:
            m = json.loads(raw)
        except ValueError:
            continue
        if m.get('reason') != 'compiler-message':
            continue
        msg = m['message']
        if msg.get('level') != 'error':
            continue
        spans = [s for s in msg.get('spans', []) if s.get('is_primary')]
        if not spans:
            continue
        s = spans[0]
        found.append((s['file_name'], s['line_start'], msg['message']))
        # The whole diagnostic, notes and all: the headline says
        # "mismatched types" 2482 times and nothing about which two.
        RENDERED.append(msg.get('rendered') or '')
    return found


RENDERED = []


def enclosing_fn(lines, line):
    """(start, end) 0-based inclusive of the `fn` whose body holds `line`
    (1-based), or None. The body is the brace block after the signature."""
    i = line - 1
    # Walk up to a `fn` line, then check its block contains the error line.
    j = i
    while j >= 0:
        if FN.match(lines[j]):
            start = j
            # find the opening brace of the body (skip signature lines)
            k = j
            depth = 0
            opened = None
            while k < len(lines):
                for ch in lines[k]:
                    if ch == '{':
                        if opened is None:
                            opened = k
                        depth += 1
                    elif ch == '}':
                        depth -= 1
                        if opened is not None and depth == 0:
                            end = k
                            if opened <= i <= end:
                                return start, opened, end
                            # the error is not inside this fn: keep looking up
                            k = len(lines)
                            break
                if k == len(lines):
                    break
                k += 1
            if opened is None:
                return None
        j -= 1
    return None


STATIC = re.compile(r'^(\s*pub(\([a-z]+\))?\s+static\s+[A-Z_0-9]+\s*:.*?LazyLock::new\(\|\|\s*)(.*)\);\s*$')


def stub_static(lines, line, message):
    """A `static X: LazyLock<..> = LazyLock::new(|| ..);` on one line whose
    initialiser does not compile: the closure body becomes the panic. A
    static is not a function, but it is a body all the same, and one that
    fails keeps every crate above it unchecked (the widgets crate's
    `WidgetsApp.defaultActions`, seven errors, hid the gallery)."""
    i = line - 1
    m = STATIC.match(lines[i])
    if not m:
        return None
    text = (message.replace('\\', '\\\\').replace('"', '\\"')
            .replace('{', '{{').replace('}', '}}').replace('\n', ' '))
    lines[i] = '%spanic!("dart2rust: stubbed static, did not compile: %s"));' % (
        m.group(1), text)
    return lines


def stub(lines, start, opened, end, message):
    """Replace the body from the opening brace to the closing one."""
    head = lines[opened][:lines[opened].index('{')]
    indent = re.match(r'\s*', lines[start]).group(0)
    text = (message.replace('\\', '\\\\').replace('"', '\\"')
            .replace('{', '{{').replace('}', '}}').replace('\n', ' '))
    body = '%s{ panic!("dart2rust: stubbed, did not compile: %s") }' % (head, text)
    return lines[:opened] + [body] + lines[end + 1:]


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--ws', default=os.path.join(TOOL, '.crate-ws'))
    ap.add_argument('--rounds', type=int, default=12)
    ap.add_argument('--report', default=None)
    args = ap.parse_args()

    stubbed = []
    unstubbable = []
    seen = set()
    for rnd in range(1, args.rounds + 1):
        t0 = time.time()
        errors = cargo_errors(args.ws)
        dt = time.time() - t0
        print('round %d: %d errors (%.0fs)' % (rnd, len(errors), dt), flush=True)
        if not errors:
            break
        by_file = {}
        for f, line, msg in errors:
            by_file.setdefault(f, []).append((line, msg))
        changed = 0
        unstubbable = []
        for f, items in by_file.items():
            path = os.path.join(args.ws, f)
            try:
                lines = io.open(path, encoding='utf-8').read().split('\n')
            except OSError:
                continue
            # Highest line first, so earlier stubs do not shift later spans.
            done = set()
            for line, msg in sorted(items, reverse=True):
                if line < 1 or line > len(lines):
                    # A span past the end: the file changed under cargo
                    # (the same file reported under two spellings).
                    unstubbable.append((f, line, msg))
                    continue
                fn = enclosing_fn(lines, line)
                if fn is None:
                    if 'stubbed static' not in lines[line - 1]:
                        replaced = stub_static(lines, line, msg)
                        if replaced is not None:
                            lines = replaced
                            stubbed.append((f, '<static>', msg))
                            changed += 1
                            continue
                    unstubbable.append((f, line, msg))
                    continue
                start, opened, end = fn
                if start in done:
                    continue
                done.add(start)
                name = FN.match(lines[start]).group(6)
                # Stubbed already and still failing: the error is in the
                # signature or the trait it implements, not the body.
                if 'panic!("dart2rust: stubbed' in lines[opened]:
                    unstubbable.append((f, line, msg))
                    continue
                lines = stub(lines, start, opened, end, msg)
                stubbed.append((f, name, msg))
                changed += 1
            io.open(path, 'w', encoding='utf-8', newline='\n').write('\n'.join(lines))
        print('  stubbed %d function(s), %d error(s) outside any function' % (
            changed, len(unstubbable)), flush=True)
        if changed == 0:
            break

    print('total stubbed: %d' % len(stubbed))
    print('unstubbable: %d' % len(unstubbable))
    if args.report:
        with io.open(args.report, 'w', encoding='utf-8') as out:
            out.write('# stubbed functions: %d\n' % len(stubbed))
            for f, name, msg in stubbed:
                out.write('%s\t%s\t%s\n' % (f, name, msg))
            out.write('\n# errors outside any function: %d\n' % len(unstubbable))
            for f, line, msg in unstubbable:
                out.write('%s:%d\t%s\n' % (f, line, msg))
        with io.open(args.report + '.rendered.txt', 'w', encoding='utf-8') as out:
            out.write('\n'.join(RENDERED))
    return 0


if __name__ == '__main__':
    sys.exit(main())
