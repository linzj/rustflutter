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

A theme counts as consumed when `XTheme::of`, or any associated function of a
resolver that reads it, is called anywhere outside the file that defines them.
That is deliberately generous -- a resolver called by one widget marks the
whole theme consumed even if that widget reads two of its fifteen fields -- so
a name on this list is a theme with *no* reader at all, not a theme with a lazy
one.  The lazier case is `depth.py`'s to find.

The call-site pattern was `Resolver::of(` and is now `Resolver::<anything>(`.
`DropdownAlignment` reads `ButtonTheme` from a `from_theme` constructor and its
`of` takes plain flags, so the narrower pattern missed a resolver that was
being called -- the same blind spot the tool already carried once, when it
looked for a resolver name derived from the theme's instead of following the
resolvers it could see.

Deprecated upstream
-------------------

Not every unread theme is a gap.  `ButtonBarTheme` has exactly one reader
upstream, `ButtonBar`, and upstream marks **both** of them
`@Deprecated("Use OverflowBar instead")`; this port maps `ButtonBar` to
`OverflowBar`, which has never consulted a theme.  Writing a reader for it
would be a resolver for a widget that does not exist -- dead code added to
satisfy a ruler, which is the failure the ruler is supposed to prevent.

So the report separates themes whose upstream declaration carries
`@Deprecated` from the rest.  Those are listed and excluded from the count,
because the count is meant to be a queue and they are not work.

Usage:
  python tools/unwired.py            # the themes with no reader
  python tools/unwired.py --all      # every theme, with its readers
"""

import argparse
import io
import os
import re
import sys

import paths

REPO = paths.REPO
CRATE = paths.SRC
THEMES = os.path.join(CRATE, 'component_themes.rs')
UPSTREAM = paths.upstream_src()


def deprecated_upstream():
    """Theme wrappers whose upstream `class X` carries an `@Deprecated`.

    Read from the declaration rather than from a list here, so the answer
    follows upstream instead of following a note somebody wrote once.
    """
    # No `isdir` guard returning an empty set: this ran for a whole round
    # against a tree that was not there, answering "nothing is deprecated
    # upstream" and so reporting `ButtonBarTheme` -- which upstream *has*
    # deprecated -- as a theme nobody reads. `paths.upstream_src` raises
    # instead.
    out = set()
    for root, _dirs, files in os.walk(UPSTREAM):
        for name in files:
            if not name.endswith('.dart'):
                continue
            text = io.open(os.path.join(root, name),
                           encoding='utf-8', errors='replace').read()
            for match in re.finditer(r'^class (\w+Theme(?:Data)?)\b', text, re.M):
                head = text[max(0, match.start() - 240):match.start()]
                if '@Deprecated' not in head.rsplit('///', 1)[-1]:
                    continue
                # A wrapper exists to carry its data, so a deprecated
                # `XThemeData` retires `XTheme` whether or not upstream said
                # so. It did not say so for `ButtonBarTheme`: the data class
                # and the widget that reads it are both marked and the
                # `InheritedWidget` between them is not, so grepping the
                # wrapper alone would call it live.
                out.add(match.group(1).removesuffix('Data'))
    return out


def wrappers():
    """The theme wrappers: types in the themes file whose impl has an `of`.

    Not "types named `XTheme` minus those named `XThemeData`". That test is
    about the name, and it listed `TextTheme` -- which upstream declares as
    `class TextTheme with Diagnosticable`, a data class with no `.of` at all,
    reached through `Theme.of(context).textTheme`. It is consumed constantly
    and could never have had the reader the report was asking for.

    Having an `of` is what makes a type a wrapper, so that is the test. It is
    the same correction `resolvers()` already needed: this tool's mistakes have
    all been name-shaped heuristics standing in for behaviour-shaped ones.
    """
    text = io.open(THEMES, encoding='utf-8', errors='replace').read()
    out = []
    for match in re.finditer(r'^pub struct (\w+Theme)\b', text, re.M):
        name = match.group(1)
        if name.endswith('ThemeData'):
            continue
        impl = re.search(r'^impl ' + re.escape(name) + r' \{', text, re.M)
        if not impl:
            continue
        end = text.find(chr(10) + 'impl ', impl.end())
        body = text[impl.end():end if end > 0 else len(text)]
        if re.search(r'fn of\(', body):
            out.append(name)
    return sorted(set(out))


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
        pattern = re.compile(re.escape(resolver) + r'::\w+\(')
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

    retired = deprecated_upstream()
    # A scan that finds nothing looks exactly like a scan with nothing to find.
    # This one found nothing for a while because a mangled `` in its regex
    # asked for a literal backspace after the class name, and the report simply
    # listed one more theme than it should have. Say what was seen.
    if os.path.isdir(UPSTREAM) and not retired:
        print(f'warning: no @Deprecated theme found under {UPSTREAM} -- '
              f'upstream has at least one, so the scan is broken')
    unwired = [(theme, where) for theme, where in rows
               if not where and theme not in retired]
    dead = [theme for theme, where in rows if not where and theme in retired]

    print(f'{len(rows)} themes, {len(unwired)} with no reader anywhere'
          f' ({len(retired)} retired upstream and not counted)')
    print()
    for theme, where in rows:
        if where and not args.all:
            continue
        if theme in retired:
            continue
        note = ', '.join(where) if where else '-- nothing reads it'
        print(f'  {theme:<28} {note}')
    if dead:
        print()
        print('Deprecated upstream, so not a queue entry:')
        for theme in dead:
            print(f'  {theme:<28} -- upstream marks it @Deprecated')
    return 1 if unwired else 0


if __name__ == '__main__':
    sys.exit(main())
