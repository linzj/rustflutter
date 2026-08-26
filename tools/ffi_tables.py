"""The number tables that are hand-written on *both* sides of the FFI.

`wire_enums.py` compares this port to upstream Dart. This compares it to the
C++ beside it, which is a different question with a different failure mode.

Three enums in this crate have a `pub(crate) fn code(self) -> c_int` whose
whole job is to produce a number the engine reads, and each of their doc
comments says the same uncomfortable thing:

    The number the engine reads, and **nothing on this side reads it**:
    `ToTileMode` in `rustflutter_ffi_draw.cc` is the other half, and the two
    are hand-written mirrors of one ABI. A row that took its neighbour's
    number would tile a gradient the wrong way with nothing here to notice.

The Rust tests pin the Rust numbers. The C++ pins the C++ numbers. **Nothing
was comparing the two**, so both could be internally consistent and mean
different things -- which is precisely the shape of bug that survives every
test on either side.

# What it reads

A `fn code` in the port whose doc names its opposite number, in this crate's
usual citation form:

    `ToTileMode` in `rustflutter_ffi_draw.cc`

Then that function's `switch`, arm by arm. The names are normalised across the
two languages by dropping a `k` prefix, a `::` namespace and any underscores,
and comparing case-insensitively -- so `kAntiAliasWithSaveLayer`,
`txt::TextAlign::justify` and `AntiAliasWithSaveLayer` all reduce to
comparable words.

# The `default:` arm is a number too

Every one of these switches ends in a `default:`, and that arm is not a
fallback in any meaningful sense -- it is *the row for whichever number the
cases did not list*. `ToTileMode` names three cases and clamps everything
else, so `kClamp` is index 0 by omission, and the port's `TileMode::Clamp =>
0` is what makes that true. `ToClipBehavior` is the one where this is easy to
misread: it lists 0, 1 and 3 and defaults to `kAntiAlias`, so the *missing*
case is 2, and 2 had better be `AntiAlias` on this side.

So a default arm is checked against whatever number the port assigned that the
C++ did not name. A switch with more than one such number is reported rather
than guessed at.

  python tools/ffi_tables.py
"""
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PORT = os.path.join(REPO, 'src', 'flutter', 'rust', 'rustflutter', 'src')
FFI = os.path.join(REPO, 'src', 'flutter', 'rust')

# `pub(crate) fn code(self) -> c_int {` and the body up to its closing brace.
CODE_FN = re.compile(
    r'(?P<doc>(?:^[ ]*///[^\n]*\n)*)'
    r'^[ ]*pub\(crate\) fn code\(self\)[^{]*\{(?P<body>.*?)^[ ]*\}',
    re.M | re.S)
ARM = re.compile(r'(?P<enum>[A-Z]\w*)::(?P<variant>\w+)\s*=>\s*(?P<code>-?\d+)')
# `` `ToTileMode` in `rustflutter_ffi_draw.cc` ``
CITATION = re.compile(r'`(?P<fn>\w+)` in\s*\n?\s*(?:///\s*)?`(?P<file>[\w.]+\.cc)`')


def normalise(name):
    """`kAntiAlias`, `txt::TextAlign::justify` and `AntiAlias` alike."""
    name = name.split('::')[-1]
    if len(name) > 1 and name[0] == 'k' and name[1].isupper():
        name = name[1:]
    return name.replace('_', '').lower()


def switch_arms(text, function):
    """`case N:` -> the name that arm produces, plus the default's name."""
    start = text.find(function)
    if start < 0:
        return None, None
    brace = text.index('{', start)
    depth = 0
    for pos in range(brace, len(text)):
        if text[pos] == '{':
            depth += 1
        elif text[pos] == '}':
            depth -= 1
            if depth == 0:
                body = text[brace:pos]
                break
    else:
        return None, None
    # Each `case N:` or `default:` runs to the next one.
    pieces = re.split(r'\n\s*(case\s+(-?\d+)\s*:|default\s*:)', body)
    cases, fallback = {}, None
    index = 1
    while index < len(pieces):
        label, number = pieces[index], pieces[index + 1]
        arm = pieces[index + 2] if index + 2 < len(pieces) else ''
        produced = re.search(r'([A-Za-z_][\w:]*::k?[A-Za-z_]\w*)', arm)
        name = produced.group(1) if produced else None
        if label.startswith('case'):
            if name:
                cases[int(number)] = name
        elif name:
            fallback = name
        index += 3
    return cases, fallback


def main():
    ffi_sources = {}
    for root, _dirs, files in os.walk(FFI):
        for name in files:
            if name.endswith('.cc'):
                ffi_sources.setdefault(
                    name, open(os.path.join(root, name), encoding='utf-8',
                               errors='replace').read())

    tables, problems = [], []
    for root, _dirs, files in os.walk(PORT):
        for name in sorted(files):
            if not name.endswith('.rs'):
                continue
            where = os.path.relpath(os.path.join(root, name), PORT)
            text = open(os.path.join(root, name), encoding='utf-8',
                        errors='replace').read()
            for match in CODE_FN.finditer(text):
                arms = list(ARM.finditer(match.group('body')))
                if not arms:
                    continue
                enum = arms[0].group('enum')
                ours = {int(arm.group('code')): arm.group('variant')
                        for arm in arms}
                cited = CITATION.search(match.group('doc') or '')
                tables.append((enum, where, ours, cited))
                if cited is None:
                    problems.append(('NO C++ CITATION', enum, where, ''))
                    continue
                source = ffi_sources.get(cited.group('file'))
                if source is None:
                    problems.append(('NO SUCH FFI FILE', enum, where,
                                     cited.group('file')))
                    continue
                cases, fallback = switch_arms(source, cited.group('fn'))
                if cases is None:
                    problems.append(('FUNCTION NOT FOUND', enum, where,
                                     cited.group('fn')))
                    continue
                for code, theirs in sorted(cases.items()):
                    mine = ours.get(code)
                    if mine is None:
                        problems.append((
                            'C++ HAS A CODE THIS SIDE DOES NOT', enum, where,
                            '%d -> %s' % (code, theirs)))
                    elif normalise(mine) != normalise(theirs):
                        problems.append((
                            'DISAGREES', enum, where,
                            '%d is %s here and %s there' % (code, mine, theirs)))
                unlisted = sorted(set(ours) - set(cases))
                if fallback is None:
                    if unlisted:
                        problems.append((
                            'NO DEFAULT ARM', enum, where,
                            'nothing answers for %s' % unlisted))
                elif len(unlisted) != 1:
                    problems.append((
                        'DEFAULT ARM IS AMBIGUOUS', enum, where,
                        '%d codes fall to it: %s' % (len(unlisted), unlisted)))
                elif normalise(ours[unlisted[0]]) != normalise(fallback):
                    problems.append((
                        'DISAGREES', enum, where,
                        '%d is %s here and the default arm is %s'
                        % (unlisted[0], ours[unlisted[0]], fallback)))

    print('%d hand-written FFI number tables, %d problems'
          % (len(tables), len(problems)))
    print()
    for kind, enum, where, detail in problems:
        print('  %-34s %-22s %-20s %s' % (kind, enum, where, detail))
    if problems:
        print()
    for enum, where, ours, cited in sorted(tables):
        target = ('%s in %s' % (cited.group('fn'), cited.group('file'))
                  if cited else '-- nothing cited')
        print('  %-16s %-16s %d codes against %s'
              % (enum, where, len(ours), target))
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
