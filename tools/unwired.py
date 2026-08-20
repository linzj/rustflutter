#!/usr/bin/env python3
"""Themes that are ported and that nothing reads.

The third ruler.  `coverage.py` asks whether a class exists, `depth.py` asks
how much of it exists, and this asks a narrower question that kept turning up
the same answer by hand: **is anything consuming it?**

Three widgets in a row -- RenderListWheelViewport, ListTile, Switch -- were
found with their theme data ported in full, their `Theme::of` resolver in
place, and the widget reading none of it.  A theme nobody reads is worse than
one that is absent: it looks finished, it type-checks, and a caller who sets it
watches nothing happen.

A theme counts as consumed when `XTheme::of` or `ResolvedX::of` is called
anywhere outside the file that defines them.  That is deliberately generous --
a resolver called by one widget marks the whole theme consumed even if that
widget reads two of its fifteen fields -- so a name on this list is a theme
with *no* reader at all, not a theme with a lazy one.  The lazier case is
`depth.py`'s to find.

Usage:
  python tools/unwired.py            # the themes with no reader
  python tools/unwired.py --all      # every theme, with its readers
"""

import argparse
import io
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CRATE = os.path.join(REPO, 'src', 'flutter', 'rust', 'rustflutter', 'src')
THEMES = os.path.join(CRATE, 'component_themes.rs')


def wrappers():
    """`pub struct XTheme` in the themes file, minus the data types."""
    text = io.open(THEMES, encoding='utf-8', errors='replace').read()
    found = re.findall(r'^pub struct (\w+Theme)\b', text, re.M)
    return sorted(set(name for name in found if not name.endswith('ThemeData')))


def readers():
    """{file: text} for every source file but the one defining the themes."""
    out = {}
    for root, _dirs, files in os.walk(CRATE):
        for name in files:
            if not name.endswith('.rs') or name == 'component_themes.rs':
                continue
            path = os.path.join(root, name)
            out[os.path.relpath(path, CRATE)] = io.open(
                path, encoding='utf-8', errors='replace').read()
    return out


def resolvers():
    """{resolver name: [themes it reads]} for every `X::of` in the themes file.

    A resolver is any type in the themes file whose `of` calls one or more
    `XTheme::of`.  Following these is what the first version of this tool did
    not do, and it over-reported by a third: `ResolvedButton::of` reads three
    button themes and is called from `components.rs`, but the tool looked for
    `ResolvedFilledButton` -- a name derived from the theme's -- and found
    nothing.  A resolver may be called anything, and several themes may share
    one.
    """
    text = io.open(THEMES, encoding='utf-8', errors='replace').read()
    out = {}
    for match in re.finditer(r'^impl (\w+) \{', text, re.M):
        name = match.group(1)
        end = text.find(chr(10) + 'impl ', match.end())
        body = text[match.end():end if end > 0 else len(text)]
        if not re.search(r'fn of\(', body):
            continue
        reads = sorted(set(re.findall(r'(\w+Theme)::of\(', body)))
        if reads:
            out[name] = reads
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--all', action='store_true')
    args = parser.parse_args()

    sources = readers()
    # A resolver called from outside marks every theme it reads as read.
    through = {}
    for resolver, themes in resolvers().items():
        pattern = re.compile(re.escape(resolver) + r'::of\(')
        where = sorted(name for name, text in sources.items() if pattern.search(text))
        for theme in themes:
            through.setdefault(theme, []).extend(
                f'{name} (via {resolver})' for name in where)

    rows = []
    for theme in wrappers():
        pattern = re.compile(re.escape(theme) + r'::of\(')
        where = sorted(name for name, text in sources.items() if pattern.search(text))
        where += sorted(through.get(theme, []))
        rows.append((theme, where))

    unwired = [(theme, where) for theme, where in rows if not where]
    print(f'{len(rows)} themes, {len(unwired)} with no reader anywhere')
    print()
    for theme, where in rows:
        if where and not args.all:
            continue
        note = ', '.join(where) if where else '-- nothing reads it'
        print(f'  {theme:<28} {note}')
    return 1 if unwired else 0


if __name__ == '__main__':
    sys.exit(main())
