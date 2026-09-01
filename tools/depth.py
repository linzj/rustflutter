#!/usr/bin/env python3
"""How *deep* each covered class is ported, not merely whether the name exists.

`coverage.py` answers "does a symbol with this name exist".  The plan has
always said that is only the entry ticket -- the acceptance standard is
per-symbol comparison -- and this session found the difference matters:
`RenderListWheel` matched upstream's `RenderListWheelViewport` by every
reasonable reading and was missing five of its properties, three of which the
widget above it had been declaring and never passing down.

So this is a second ruler, over the bucket the first one calls `covered`.  For
each upstream class it counts the public members upstream declares, finds the
Rust type that answered for it, counts what that type exposes, and reports the
ratio.

# It is a heuristic, and the ways it lies are known

  * A Dart getter/setter pair is one Rust field, or one accessor, or two.
  * Upstream's `operator ==`, `hashCode`, `toString`, `debugFillProperties` and
    `copyWith` become derives, `impl Display`, or nothing at all.
  * A closed Rust enum can replace a whole family of Dart subclasses, so the
    member counts do not line up in either direction.
  * A member may be answered somewhere other than on the type -- a free
    function, a trait, another module.

Every one of those makes the ratio *understate* how well something is ported.
That is the right direction for a tool whose output is a list to go and read:
it over-reports suspects and does not hide them.  A low ratio is a question,
never a verdict.

# It has no memory of its own, which is the other half of the problem

Because every way it lies makes a ratio too low, most of what it reports is
something to read once and dismiss.  Dismissing it in your head means reading
it again three ticks later; `depth_examined.json` is where a reading goes so
that it counts.  A row lands there with the reason the shortfall is not a gap,
and this tool then stops showing it.

That file is a claim, not a suppression list: each row has to name the
mechanism that answers for the missing members, and where the reasoning lives.

Usage:
  python tools/depth.py                  # the shallowest not yet examined
  python tools/depth.py --top 50
  python tools/depth.py --all            # including what has been examined
  python tools/depth.py --name RenderFlex
  python tools/depth.py --examined       # what has been read, and what it found
"""

import argparse
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import coverage  # noqa: E402  -- the first ruler, reused whole

CRATE = coverage.CRATE
UPSTREAM = coverage.UPSTREAM

EXAMINED = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                        'depth_examined.json')


def load_examined():
    """Rows already read against upstream, by class name.

    Missing file is not an error -- it means nothing has been examined yet,
    which is a true statement about a fresh checkout.
    """
    if not os.path.exists(EXAMINED):
        return {}
    with open(EXAMINED, encoding='utf-8') as handle:
        return {row['class']: row for row in json.load(handle)['examined']}

# Members that exist upstream because Dart needs them written out, and that a
# Rust port answers with a derive or a trait impl.  Counting them would make
# every value type look half-ported.
DART_KEYWORDS = {
    'return', 'if', 'for', 'while', 'switch', 'case', 'assert', 'final',
    'const', 'var', 'new', 'this', 'super', 'else', 'break', 'continue',
    'true', 'false', 'null', 'throw', 'yield', 'await', 'try', 'catch',
    'default', 'do', 'rethrow',
}

DART_CEREMONY = {
    'toString', 'hashCode', 'operator ==', 'debugFillProperties',
    'debugDescribeChildren', 'toStringShort', 'toDiagnosticsNode',
    'noSuchMethod', 'runtimeType',
}

MEMBER = re.compile(
    r'^\s{2}(?!//)'                       # a class member, two-space indented
    r'(?:@\w+\s+)*'                       # annotations
    r'(?:static\s+|final\s+|const\s+|late\s+|covariant\s+)*'
    r'(?:[\w<>?,\s\[\]]+\s+)?'            # return type, maybe
    r'(?:get\s+|set\s+)?'                 # accessor
    r'(?P<name>[a-zA-Z_]\w*)'             # the name
    r'\s*(?:\(|=>|;|=[^=])',              # method, arrow, field or initialiser
    re.M,
)

# What counts as one member on the Rust side.  Four alternatives, because
# `pub` is not the marker: a trait's methods and an enum's variants carry no
# visibility of their own and are public with the type.  Counting only `pub`
# made every trait and every enum in the crate read as zero, which was the
# tool lying rather than the crate being empty.
RUST_MEMBER = re.compile(
    r'^\s{4}(?:'
    r'(?:pub(?:\([^)]*\))?\s+)?(?:fn|const|async\s+fn)\s'   # method or constant
    r'|pub(?:\([^)]*\))?\s+[a-z_]\w*\s*:'                   # public field
    r'|[A-Z]\w*(?:\s*[,({]|\s*$)'                             # enum variant
    r')',
    re.M,
)


def dart_members(body):
    """Public members a Dart class declares, minus the ceremony.

    Only lines at the class's own brace depth count.  The first version matched
    on indentation alone and swept up every local variable and constructor call
    inside every method body -- `ListTile` came out with 87 members including
    `break`, `false` and `InkWell`, which made the ratio a measure of how long
    upstream's methods are.
    """
    names = set()
    depth = 0
    for line in body.splitlines():
        stripped = line.strip()
        if depth == 1 and stripped and not stripped.startswith('//'):
            match = MEMBER.match('  ' + stripped)
            if match:
                name = match.group('name')
                if not name.startswith('_') and name not in DART_CEREMONY                         and name not in DART_KEYWORDS:
                    names.add(name)
        depth += line.count('{') + line.count('(') - line.count('}') - line.count(')')
        depth = max(depth, 0)
    return names


def class_bodies(path):
    """Every `class X { ... }` in a Dart file, by name, comment-stripped."""
    text = coverage.strip_dart_comments(
        open(path, encoding='utf-8', errors='replace').read())
    bodies = {}
    for match in re.finditer(
            r'^(?:abstract\s+|base\s+|final\s+|sealed\s+|interface\s+|mixin\s+|@immutable\s+)*'
            r'(?:class|mixin)\s+(?P<name>[A-Z]\w*)', text, re.M):
        name = match.group('name')
        start = text.find('{', match.end())
        if start < 0:
            continue
        depth, index = 0, start
        while index < len(text):
            if text[index] == '{':
                depth += 1
            elif text[index] == '}':
                depth -= 1
                if depth == 0:
                    break
            index += 1
        bodies[name] = text[start:index]
    return bodies


def rust_aliases():
    """`pub type Upstream = Whatever;`, by the name upstream would use.

    `Text` is the case that found this: the crate spells it
    `pub type Text = RenderParagraph`, so a search for a struct by that name
    returns nothing and the ratio came out 0 of 18 for a type that is ported
    whole. An alias is exactly the shape this ruler exists to see through --
    the crate saying "upstream's name for this is X" in as many words.
    """
    aliases = {}
    for root, _dirs, files in os.walk(CRATE):
        for filename in files:
            if not filename.endswith('.rs'):
                continue
            text = coverage.strip_rust_comments(
                open(os.path.join(root, filename),
                     encoding='utf-8', errors='replace').read())
            for match in re.finditer(
                    r'^pub type (?P<name>\w+)\s*=\s*(?P<target>\w+)', text, re.M):
                aliases[match.group('name')] = match.group('target')
    return aliases


def rust_bodies():
    """Every `pub struct/enum/trait X` in the crate, by name, with its impls."""
    bodies = {}
    for root, _dirs, files in os.walk(CRATE):
        for filename in files:
            if not filename.endswith('.rs'):
                continue
            path = os.path.join(root, filename)
            text = coverage.strip_test_modules(
                coverage.strip_rust_comments(
                    open(path, encoding='utf-8', errors='replace').read()))
            for match in re.finditer(
                    r'^pub (?:struct|enum|trait) (?P<name>\w+)', text, re.M):
                name = match.group('name')
                start = text.find('{', match.end())
                if start < 0:
                    bodies.setdefault(name, '')
                    continue
                depth, index = 0, start
                while index < len(text):
                    if text[index] == '{':
                        depth += 1
                    elif text[index] == '}':
                        depth -= 1
                        if depth == 0:
                            break
                    index += 1
                bodies.setdefault(name, '')
                bodies[name] += text[start:index]
            # Every `impl ... Name` block counts towards Name.
            for match in re.finditer(
                    r'^impl(?:<[^>]*>)?\s+(?:(?P<trait>[\w:<>, ]+?)\s+for\s+)?'
                    r'(?P<name>\w+)', text, re.M):
                name = match.group('name')
                start = text.find('{', match.end())
                if start < 0:
                    continue
                depth, index = 0, start
                while index < len(text):
                    if text[index] == '{':
                        depth += 1
                    elif text[index] == '}':
                        depth -= 1
                        if depth == 0:
                            break
                    index += 1
                bodies.setdefault(name, '')
                bodies[name] += text[start:index]
    return bodies


# Where a class's members plausibly live besides the type of the same name.
#
# The single biggest source of false positives is a member answered somewhere
# else: `MediaQuery`'s sixty accessors are `MediaQueryData`'s fields, `ListTile`
# reads its defaults off `ResolvedListTile`, and half of `widgets/basic.dart` is
# ported as free functions with no type at all.  Crediting these does not make
# the ruler strict -- it still cannot see a member answered on a type whose name
# shares nothing with upstream's -- but it stops it reporting the same four
# shapes over and over.
def companion_body(rust, name, aliases=None):
    candidates = [name, name + 'Data', name + 'State',
                  'Resolved' + name, 'Render' + name]
    # An alias is the crate naming the correspondence itself, so it outranks
    # the guesses above -- but both are counted, because a type can be aliased
    # *and* have a companion.
    target = (aliases or {}).get(name)
    if target and target not in candidates:
        candidates.append(target)
    return ''.join(rust.get(candidate, '') for candidate in candidates)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--top', type=int, default=20)
    parser.add_argument('--name', default=None)
    parser.add_argument('--min-members', type=int, default=6,
                        help='ignore classes too small for the ratio to mean anything')
    parser.add_argument('--all', action='store_true',
                        help='include classes already read against upstream')
    parser.add_argument('--examined', action='store_true',
                        help='print what has been read, and what the reading found')
    args = parser.parse_args()

    examined = load_examined()
    if args.examined:
        print(f'{len(examined)} classes read against upstream member by member')
        for name, row in sorted(examined.items()):
            print(f'{chr(10)}  {name}  ({row["file"]}), tick {row["tick"]}')
            print(f'    {row["finding"]}')
            # `at` arrived later than the first rows, so a few predate it. A
            # ruler that crashes on its own oldest entries is a ruler nobody
            # runs -- and this printer is the only place those readings can be
            # read back.
            if row.get('at'):
                print(f'    where: {row["at"]}')
        return

    classes_by_file = coverage.upstream_classes()
    rust_ids = coverage.rust_identifiers()
    ledger = coverage.load_ledger()
    rust = rust_bodies()
    aliases = rust_aliases()

    covered = {}
    for layer, fname, name, state in coverage.classify(
        classes_by_file, rust_ids, ledger, coverage.rust_module_names()
    ):
        if state == 'covered':
            covered.setdefault(f'{layer}/{fname}', []).append(name)

    rows = []
    for relative, names in sorted(covered.items()):
        path = os.path.join(UPSTREAM, 'packages', 'flutter', 'lib', 'src', relative)
        if not os.path.exists(path):
            continue
        bodies = None
        for name in names:
            if args.name and name != args.name:
                continue
            if bodies is None:
                bodies = class_bodies(path)
            body = bodies.get(name)
            if body is None:
                continue
            wanted = dart_members(body)
            if len(wanted) < args.min_members:
                continue
            have = len(RUST_MEMBER.findall(companion_body(rust, name, aliases)))
            rows.append((have / len(wanted), have, len(wanted), name, relative))

    rows.sort()
    hidden = 0
    if not args.all and not args.name:
        before = len(rows)
        rows = [row for row in rows if row[3] not in examined]
        hidden = before - len(rows)
    shown = rows if args.name else rows[:args.top]
    print(f'{len(rows)} covered classes with {args.min_members}+ upstream members'
          + (f' ({hidden} examined, hidden -- --all to see them)' if hidden else ''))
    print(f'{"ratio":>6}  {"rust":>4} {"dart":>4}  class / file')
    for ratio, have, wanted, name, relative in shown:
        print(f'{ratio:6.2f}  {have:4} {wanted:4}  {name}  ({relative})')


if __name__ == '__main__':
    main()
