#!/usr/bin/env python3
"""Coverage of the Rust framework against upstream Flutter, per public class.

Upstream is the Flutter checkout this tree was forked from (see
PORTING_STATUS.md for the pin).  Every public class or mixin declared under
packages/flutter/lib/src/{layers} must end up in exactly one of five states:

  covered        a symbol with the same (snake_cased) name exists in the crate
  mapped         the ledger records a rename / merge / functional equivalent
  blocked-engine depends on an engine capability we do not bridge yet
  out-of-scope   web-only, no-host-platform, or debug-only per the ledger
  MISSING        none of the above -- this is the work queue

Private classes (leading underscore) and *_io.dart / *_web.dart variants are
not counted; platform-specific classes inside mixed files are ledgered
instead.  Name matching is done on comment-stripped sources so words in doc
comments do not count as coverage.

Usage:
  python tools/coverage.py                 # summary + per-file detail
  python tools/coverage.py widgets/basic   # only files matching a substring
  python tools/coverage.py --missing-only  # only the work queue
"""

import argparse
import json
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
UPSTREAM = os.environ.get('FLUTTER_UPSTREAM', r'K:\flutter')
CRATE = os.path.join(REPO, 'src', 'flutter', 'rust', 'rustflutter', 'src')
LEDGER = os.path.join(REPO, 'coverage_ledger.json')

LAYERS = [
    'widgets', 'rendering', 'painting', 'gestures', 'services',
    'animation', 'scheduler', 'foundation', 'material', 'cupertino',
]

CLASS_RE = re.compile(
    r'^(?:abstract\s+|base\s+|final\s+|sealed\s+|mixin\s+)*(?:class|mixin)\s+([A-Za-z0-9_]+)',
    re.M,
)


def strip_dart_comments(text):
    text = re.sub(r'/\*.*?\*/', '', text, flags=re.S)
    text = re.sub(r'//[^\n]*', '', text)
    return text


def strip_rust_comments(text):
    # Nested block comments are legal in Rust; strip innermost-outwards.
    while True:
        stripped = re.sub(r'/\*(?:[^*/]|\*(?!/)|/(?!\*))*\*/', '', text, flags=re.S)
        stripped = re.sub(r'//[^\n]*', '', stripped)
        if stripped == text:
            return text
        text = stripped


def rust_identifiers():
    """Declared symbols only (types, fns, aliases, impl targets) --
    locals named `element` must not count as covering upstream `Element`.

    Three things are deliberately *not* counted, each because it was caught
    crediting a class nobody had written:

    * `mod`. A module is a file, not a type, and the snake-case fold that
      lets `text_theme` answer for `TextTheme` also let `mod actions` answer
      for upstream's `Actions` widget.
    * A function that is not a free public one. This crate writes some widget
      facades as functions -- `pub fn spacer() -> AnyWidget` is a real port of
      upstream's `Spacer` -- so functions have to count for something. But a
      *method* named `element`, `title` or `window` is not a port of
      `Element`, `Title` or `Window`, and nor is a private helper named
      `cubic` a port of the curve. Free and `pub` is what separates the two:
      a method is indented inside its `impl`, and a helper is not `pub`.
    * A constant or static that is not a free public one, for the same
      reason.
    """
    # A type, an impl target, or a macro: counted wherever it is declared.
    decl = re.compile(
        r'\b(?:struct|enum|union|trait|type|macro_rules!)\s+([A-Za-z_][A-Za-z0-9_]*)'
        r'|\bimpl(?:\s*<[^{;]*>)?\s+(?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*([A-Za-z_][A-Za-z0-9_]*)'
    )
    # A function, constant or static: only at the top of a line and only
    # public, which is what a facade looks like and a method does not.
    free = re.compile(
        r'^pub(?:\([^)]*\))?\s+(?:const\s+|async\s+|unsafe\s+|extern\s+"[^"]*"\s+)*'
        r'(?:fn|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)',
        re.M,
    )
    ids = set()
    for root, _, files in os.walk(CRATE):
        for name in files:
            if not name.endswith('.rs'):
                continue
            path = os.path.join(root, name)
            text = strip_rust_comments(open(path, encoding='utf-8', errors='ignore').read())
            for m in decl.finditer(text):
                ids.update(x for x in m.groups() if x)
            ids.update(m.group(1) for m in free.finditer(text))
    return ids


def snake(name):
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower().replace('__', '_')


def upstream_classes():
    """{layer: {file: [class names]}} for public classes in counted files."""
    out = {}
    for layer in LAYERS:
        layer_dir = os.path.join(UPSTREAM, 'packages', 'flutter', 'lib', 'src', layer)
        files = {}
        for name in sorted(os.listdir(layer_dir)):
            if not name.endswith('.dart'):
                continue
            base = name[:-5]
            if base.startswith('_'):
                continue
            if base.endswith('_io') or base.endswith('_web'):
                continue  # io/web variants: not counted, see ledger notes
            text = open(os.path.join(layer_dir, name), encoding='utf-8', errors='ignore').read()
            classes = [c for c in CLASS_RE.findall(text) if not c.startswith('_')]
            if classes:
                files[name] = classes
        out[layer] = files
    return out


def load_ledger():
    if not os.path.exists(LEDGER):
        return {'equivalent': {}, 'blocked_engine': {}, 'out_of_scope_files': {},
                'out_of_scope_classes': {}}
    return json.load(open(LEDGER, encoding='utf-8'))


def classify(classes_by_file, rust_ids, ledger):
    """Yield (layer, file, class, state) rows."""
    eq = ledger.get('equivalent', {})
    blocked = ledger.get('blocked_engine', {})
    oos_files = ledger.get('out_of_scope_files', {})
    oos_classes = ledger.get('out_of_scope_classes', {})
    for layer, files in classes_by_file.items():
        for fname, classes in files.items():
            file_key = f'{layer}/{fname}'
            if file_key in oos_files or fname in oos_files:
                for c in classes:
                    yield layer, fname, c, 'out-of-scope'
                continue
            for c in classes:
                if c in eq:
                    yield layer, fname, c, 'mapped'
                elif c in blocked:
                    yield layer, fname, c, 'blocked-engine'
                elif c in oos_classes or f'{file_key}:{c}' in oos_classes:
                    yield layer, fname, c, 'out-of-scope'
                elif c in rust_ids or snake(c) in rust_ids:
                    yield layer, fname, c, 'covered'
                else:
                    yield layer, fname, c, 'MISSING'


ORDER = ['covered', 'mapped', 'blocked-engine', 'out-of-scope', 'MISSING']


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('filter', nargs='?', help='substring filter on layer/file')
    ap.add_argument('--missing-only', action='store_true')
    args = ap.parse_args()

    rows = list(classify(upstream_classes(), rust_identifiers(), load_ledger()))
    if args.filter:
        rows = [r for r in rows if args.filter in f'{r[0]}/{r[1]}']
    if args.missing_only:
        rows = [r for r in rows if r[3] == 'MISSING']

    by_state = {s: 0 for s in ORDER}
    by_file = {}
    for layer, fname, cls, state in rows:
        by_state[state] += 1
        by_file.setdefault((layer, fname), {s: [] for s in ORDER})[state].append(cls)

    total = len(rows)
    accounted = total - by_state['MISSING']
    print(f'{total} public classes across {len(by_file)} files '
          f'({accounted} accounted, {by_state["MISSING"]} MISSING)\n')
    for state in ORDER:
        print(f'  {state:<15} {by_state[state]:>5}  ({100.0 * by_state[state] / max(total, 1):.0f}%)')
    print()
    for (layer, fname), states in sorted(by_file.items()):
        counts = ' '.join(f'{s.split("-")[0]}:{len(v)}' for s, v in states.items() if v)
        print(f'{layer}/{fname}: {counts}')
        if not args.missing_only:
            for s in ('mapped', 'blocked-engine', 'out-of-scope'):
                if states[s]:
                    print(f'    [{s}] {", ".join(states[s])}')
        if states['MISSING']:
            print(f'    MISSING: {", ".join(states["MISSING"])}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
