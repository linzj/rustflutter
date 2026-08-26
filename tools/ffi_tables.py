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

# The other way a table crosses: a bare cast

`BlendMode` has no `To*` function to read. `rf_paint_set_blend_mode` guards
the range and then does `static_cast<flutter::DlBlendMode>(blend_mode)`, so
the C++ authority is not a switch anyone wrote for this port -- it is the
engine's own `enum class`, and the port's discriminants have to *be* its
numbering.

The port says so: "The discriminants match `flutter::DlBlendMode`, which in
turn matches `dart:ui`'s `BlendMode`." `wire_enums.py` checks the second half
of that sentence. This checks the first, which had nothing behind it -- and
the two upstream tables are maintained in different repositories, so agreeing
today is a fact about today.

`flutter::DlBlendMode` is a `using` alias for `impeller::BlendMode`, which is
followed -- and a **definition anywhere beats an alias anywhere**, because
following the first alias the walk reaches sends the search after a name
several other headers also alias, and it never arrives.

Enumerators initialised with another enumerator's *name* -- `kLastMode =
kLuminosity`, `kDefaultMode = kSrcOver` -- are aliases and not values, and are
skipped. Counting them would make the engine's list two longer than it is.

# By value, not by position

`SemanticsAction` is a bitmask on both sides: `kTap = 1 << 0`, so its third
entry is 4 and not 2. Comparing the two lists positionally reported twelve
disagreements that were an artefact of the question rather than an answer to
it. Both sides are read as value -> name, and a plain index table is the
special case where the values happen to run 0, 1, 2.

# What this found on its first honest run

Thirteen of the engine's twenty-six semantics actions are not in this port,
and its own doc says they have to be: "The discriminants are
`flutter::SemanticsAction`, which is in turn `SemanticsAction` in
`semantics.dart` and in every embedder. Four copies of one set of bits
upstream; this is the fifth, and it has to match." Nothing was checking that
sentence. It is checked now, and the answer is a list of thirteen.

So this ruler starts at one problem rather than zero, which is the honest
state and not a broken tool.

  python tools/ffi_tables.py
"""
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PORT = os.path.join(REPO, 'src', 'flutter', 'rust', 'rustflutter', 'src')
FFI = os.path.join(REPO, 'src', 'flutter', 'rust')
ENGINE = os.path.join(os.path.dirname(REPO), 'flutter', 'engine', 'src', 'flutter')

# `pub enum Name {` with explicit discriminants, and the doc above it.
# The doc, then any number of attribute lines -- `#[derive(...)]` sits between
# the two in every one of these, and requiring them adjacent found the enums
# and none of their documentation.
REPR_ENUM = re.compile(
    r'(?P<doc>(?:^[ ]*///[^\n]*\n)*)'
    r'(?:^#\[[^\n]*\]\n)*?'
    r'^#\[repr\(i32\)\]\n'
    r'(?:^#\[[^\n]*\]\n)*'
    r'pub enum (?P<name>\w+)\s*\{(?P<body>[^}]*)\}',
    re.M)
# The initialiser, whatever shape it takes: `Tap = 1 << 0` is a value of 1
# and `Clear = 0` is a value of 0, and reading only the first integer
# made every bit of a bitmask look like the number one.
REPR_VARIANT = re.compile(
    r'^\s{4}(?P<name>[A-Z]\w*)\s*=\s*(?P<code>[^,\n]+)', re.M)
# `flutter::DlBlendMode` in a doc comment.
CPP_ENUM_CITATION = re.compile(r'`((?:flutter|impeller)::\w+)`')

# `pub(crate) fn code(self) -> c_int {` and the body up to its closing brace.
CODE_FN = re.compile(
    r'(?P<doc>(?:^[ ]*///[^\n]*\n)*)'
    r'^[ ]*pub\(crate\) fn code\(self\)[^{]*\{(?P<body>.*?)^[ ]*\}',
    re.M | re.S)
ARM = re.compile(r'(?P<enum>[A-Z]\w*)::(?P<variant>\w+)\s*=>\s*(?P<code>-?\d+)')
# `` `ToTileMode` in `rustflutter_ffi_draw.cc` ``
CITATION = re.compile(r'`(?P<fn>\w+)` in\s*\n?\s*(?:///\s*)?`(?P<file>[\w.]+\.cc)`')


def literal(value):
    """`0`, `1 << 3`, or something this cannot read."""
    value = value.strip()
    shift = re.match(r'^(\d+)\s*<<\s*(\d+)$', value)
    if shift:
        return int(shift.group(1)) << int(shift.group(2))
    if re.match(r'^-?\d+$', value):
        return int(value)
    return None


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


def engine_headers():
    """Every engine header, read once. The walk is 22,000 files."""
    if not engine_headers.cache:
        for root, _dirs, files in os.walk(ENGINE):
            for entry in files:
                if entry.endswith(('.h', '.hpp')):
                    path = os.path.join(root, entry)
                    engine_headers.cache.append(
                        (path, open(path, encoding='utf-8',
                                    errors='replace').read()))
    return engine_headers.cache


engine_headers.cache = []


def cpp_enum(name):
    """An engine `enum class`'s enumerators in order, following `using`.

    A **definition anywhere beats an alias anywhere**, and that ordering is
    the whole of the difficulty. Following the first alias the walk happens
    to reach sends the search after a name several other headers also alias,
    and it never arrives -- which is what this did on its first run, with
    `flutter::DlBlendMode` sitting one `using` away from its answer.

    Aliases *inside* the enum -- `kLastMode = kLuminosity` -- are enumerators
    initialised with another enumerator's name rather than a number. They are
    not values and are skipped; counting them would make the engine's list two
    longer than it is.
    """
    bare = name.split('::')[-1]
    for _hop in range(4):
        definition = re.compile(
            r'enum class %s\b[^{;]*\{(?P<body>[^}]*)\}' % re.escape(bare))
        alias = re.compile(r'using %s\s*=\s*([\w:]+)\s*;' % re.escape(bare))
        followed = None
        for path, text in engine_headers():
            match = definition.search(text)
            if match:
                # Comments out first, *then* split on commas. `impeller`'s
                # prose has one -- "without extensions, and so they are" --
                # and splitting first cut a value in half and lost
                # `kModulate`, so every index from 13 up came back one out and
                # this reported fourteen disagreements that were all mine.
                #
                # This is the identical mistake `wire_enums.py` made with `;`
                # one tick earlier. A separator inside a comment is not a
                # separator, in any language, for any character.
                body = re.sub(r'//[^\n]*', '', match.group('body'))
                # By **value**, not by position. `SemanticsAction` is a
                # bitmask on both sides -- `kTap = 1 << 0` -- so its third
                # entry is 4 and not 2, and comparing the two lists
                # positionally reported twelve disagreements that were an
                # artefact of the question rather than an answer to it. A
                # plain index table is the special case where the values
                # happen to be 0, 1, 2.
                numbered, running, seen = {}, 0, set()
                for line in body.split(','):
                    line = line.strip()
                    entry = re.match(r'^(k?\w+)\s*(?:=\s*(.+))?$', line)
                    if not entry:
                        continue
                    value = (entry.group(2) or '').strip()
                    if value:
                        shift = re.match(r'^(\d+)\s*<<\s*(\d+)$', value)
                        if shift:
                            running = int(shift.group(1)) << int(shift.group(2))
                        elif re.match(r'^-?\d+$', value):
                            running = int(value)
                        else:
                            continue  # an alias for another enumerator
                    if entry.group(1) in seen:
                        continue
                    seen.add(entry.group(1))
                    numbered[running] = entry.group(1)
                    running += 1
                return numbered, os.path.relpath(path, os.path.dirname(REPO))
            if followed is None:
                hop = alias.search(text)
                if hop:
                    followed = hop.group(1)
        if followed is None:
            return None, None
        bare = followed.split('::')[-1]
    return None, None


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

    # -- Tables that cross by a bare cast ------------------------------------
    casts = []
    for root, _dirs, files in os.walk(PORT):
        for name in sorted(files):
            if not name.endswith('.rs'):
                continue
            where = os.path.relpath(os.path.join(root, name), PORT)
            text = open(os.path.join(root, name), encoding='utf-8',
                        errors='replace').read()
            for match in REPR_ENUM.finditer(text):
                ours = []
                for variant in REPR_VARIANT.finditer(match.group('body')):
                    ours.append((literal(variant.group('code')),
                                 variant.group('name')))
                ours = [(code, name) for code, name in ours if code is not None]
                if not ours:
                    continue
                cited = CPP_ENUM_CITATION.search(match.group('doc') or '')
                casts.append((match.group('name'), where, ours, cited))
                if cited is None:
                    problems.append(('NO C++ ENUM CITED', match.group('name'),
                                     where, ''))
                    continue
                theirs, source = cpp_enum(cited.group(1))
                if theirs is None:
                    problems.append(('C++ ENUM NOT FOUND', match.group('name'),
                                     where, cited.group(1)))
                    continue
                for code, mine in ours:
                    if code not in theirs:
                        problems.append((
                            'NO SUCH VALUE IN THE C++ ENUM', match.group('name'),
                            where, '%d is %s here and %s has no %d'
                            % (code, mine, cited.group(1), code)))
                    elif normalise(mine) != normalise(theirs[code]):
                        problems.append((
                            'DISAGREES', match.group('name'), where,
                            '%d is %s here and %s there'
                            % (code, mine, theirs[code])))
                missing = sorted(set(theirs) - {code for code, _ in ours})
                if missing:
                    problems.append((
                        'SHORTER THAN THE C++ ENUM', match.group('name'), where,
                        '%d values here against %d in %s -- missing %s'
                        % (len(ours), len(theirs), cited.group(1),
                           ', '.join('%d %s' % (code, theirs[code])
                                     for code in missing))))

    print('%d hand-written FFI number tables and %d crossing by a bare cast, '
          '%d problems' % (len(tables), len(casts), len(problems)))
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
    for enum, where, ours, cited in sorted(casts):
        target = cited.group(1) if cited else '-- nothing cited'
        print('  %-16s %-16s %d codes against %s (a bare cast)'
              % (enum, where, len(ours), target))
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
