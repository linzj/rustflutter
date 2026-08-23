"""The fifth ruler: which theme fields does resolution read that no test ever moves?

Two ticks running, the one mutation that survived was the same species. The
Material 2 bottom-app-bar colour branch read `theme.brightness`, and no test
built under a dark theme -- so the arm was unreachable and deleting it was
invisible. `ThemeData::lerp` carried `use_material3` from the nearer end, and no
test interpolated two themes that disagreed about it -- so pinning it to `true`
was invisible.

Both are the same shape: **the resolution reads a field, and the suite never
gives it two different values.** A test that sets one side of a decision cannot
see the decision, and a test that sets neither cannot see the field at all.
That was worth finding twice by hand; it is worth a tool the third time.

What this measures
------------------

For each `pub` field of `ThemeData`:

* Does non-test code read it as `theme.<field>` or `ThemeData::of(..).<field>`?
* Does any `#[cfg(test)]` block *write* it -- `<field>:` inside a struct
  literal, or `.<field> = ` -- with at least two distinct values, or once
  alongside a default that differs?

The second question is answered loosely on purpose. Proving two values differ
means evaluating them; what this can see is how many syntactically distinct
right-hand sides a field is given across the tests. One is a field the suite
pins rather than varies, which is the case both survivors were in.

Counting the settings
---------------------

The obvious count -- distinct right-hand sides written in tests -- got both of
its own founding cases wrong on the first run. `use_material3` is varied by
`ThemeData { use_material3: false, ..ThemeData::fallback() }`, which names the
field **once**; the second value is the default, and the default is not written
anywhere a regex over the tests can see. `brightness` is varied by
`ThemeData::dark()` against `ThemeData::light()`, which never names the field at
all.

Both are real variation, so both count here:

* the **default** from `ThemeData`'s own construction is one setting, taken from
  `from_color_scheme` in theme.rs;
* `ThemeData::dark()` and `ThemeData::light()` in a test are settings of
  `brightness`, since choosing between them is how the suite says which one it
  means.

A ruler that flagged the two things it was built to catch would cost a
false alarm every tick from here on, which is worse than not having it.

What it does not measure
------------------------

Reading is not the same as branching. A field the resolution merely copies into
its output is defended by any test that checks the output, and this tool will
still list it if the suite only ever sees one value. That is a true statement
about the suite and a weak complaint about the code, so the report separates
**never written** (nothing sets it anywhere) from **written once**. What earns
attention is a field that *decides* something -- a bool, an enum, a brightness
-- read by resolution and never given two settings.

Nor can it see a field varied through some other helper of its own. The count is
a floor on the variation, not a measurement of it: a field it calls varied is
varied, and a field it lists may still be fine.

The read pattern matches `theme.<field>` by name and not by type, so a
same-named field on a different type reads as a hit --
`CupertinoThemeData::scaffold_background_color` is the one in hand. That
inflates the "read by non-test code" count and can put a field on the list that
`ThemeData`'s own copy has nothing to do with. It cannot invent a *branching*
finding, since the branch would have to be real to be reported, but the listing
is looser than it looks.
"""

import collections
import os
import re
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..')
SRC = os.path.join(ROOT, 'src', 'flutter', 'rust', 'rustflutter', 'src')
THEME = os.path.join(SRC, 'theme.rs')

FIELD = re.compile(r'^    pub (\w+): ([^,]+),\s*$')
TEST_MOD = re.compile(r'^\s*#\[cfg\(test\)\]\s*$')


def theme_fields():
    """The `pub` fields of `ThemeData`, in declaration order."""
    fields = []
    inside = False
    with open(THEME, encoding='utf-8') as handle:
        for line in handle:
            if line.startswith('pub struct ThemeData {'):
                inside = True
                continue
            if inside:
                if line.startswith('}'):
                    break
                match = FIELD.match(line)
                if match:
                    fields.append((match.group(1), match.group(2).strip()))
    return fields


def split_test_code(text):
    """(non-test, test) halves of a file, by `#[cfg(test)]` blocks.

    Brace counting from the `mod` line: a `#[cfg(test)]` on a function rather
    than a module is rare here and would only make the test half larger, which
    is the safe direction -- it can hide a finding but not invent one.
    """
    lines = text.splitlines(keepends=True)
    plain, tests = [], []
    index = 0
    while index < len(lines):
        if TEST_MOD.match(lines[index]):
            depth = 0
            started = False
            while index < len(lines):
                tests.append(lines[index])
                depth += lines[index].count('{') - lines[index].count('}')
                if '{' in lines[index]:
                    started = True
                index += 1
                if started and depth <= 0:
                    break
            continue
        plain.append(lines[index])
        index += 1
    return ''.join(plain), ''.join(tests)


def default_settings():
    """Each field's default, from `ThemeData::from_color_scheme`'s literal.

    The default is a setting like any other -- it is what a test gets when it
    does not name the field -- and leaving it out is what made the first run of
    this tool flag `use_material3`, which is varied by naming it once against a
    default of the other value.
    """
    with open(THEME, encoding='utf-8') as handle:
        text = handle.read()
    start = text.index('pub fn from_color_scheme')
    literal = text.index('ThemeData {', start)
    depth, index = 0, literal + len('ThemeData ')
    while index < len(text):
        depth += text[index] == '{'
        depth -= text[index] == '}'
        if depth == 0:
            break
        index += 1
    body = text[literal:index]

    defaults = {}
    for match in re.finditer(r'^\s{12}(\w+): (.+?),\s*$', body, re.MULTILINE):
        defaults[match.group(1)] = match.group(2).strip()
    # Field-shorthand lines (`brightness,`) take their value from a parameter,
    # so the default is whatever the caller passed -- name it as such rather
    # than pretending there is a literal.
    for match in re.finditer(r'^\s{12}(\w+),\s*$', body, re.MULTILINE):
        defaults[match.group(1)] = '<from the caller>'
    return defaults


def scan():
    reads = collections.Counter()
    writes = collections.defaultdict(set)
    read_sites = collections.defaultdict(set)

    fields = theme_fields()
    defaults = default_settings()
    names = [name for name, _ in fields]
    read_patterns = {
        name: re.compile(r'\b(?:theme|data)\s*\.\s*' + name + r'\b')
        for name in names
    }
    # `field: <value>` in a literal, or `x.field = <value>;`
    write_patterns = {
        name: re.compile(
            r'(?:^\s*' + name + r':\s*(?P<literal>[^\n,]+),?\s*$'
            r'|\.\s*' + name + r'\s*=\s*(?P<assigned>[^;\n]+);)',
            re.MULTILINE,
        )
        for name in names
    }

    for entry in sorted(os.listdir(SRC)):
        if not entry.endswith('.rs'):
            continue
        path = os.path.join(SRC, entry)
        with open(path, encoding='utf-8') as handle:
            text = handle.read()
        plain, tests = split_test_code(text)
        for name in names:
            found = read_patterns[name].findall(plain)
            if found:
                reads[name] += len(found)
                read_sites[name].add(entry)
            for match in write_patterns[name].finditer(tests):
                value = match.group('literal') or match.group('assigned')
                writes[name].add(value.strip().rstrip(','))
        # Choosing between the two named constructors is how a test says which
        # brightness it means; neither of them names the field.
        for constructor in ('ThemeData::dark()', 'ThemeData::light()'):
            if constructor in tests:
                writes['brightness'].add(constructor)

    # The default is a setting: a test that names a field once has varied it if
    # what it named differs from what it would otherwise have got.
    #
    # It goes in unadorned. Labelling it "default X" made it a distinct string
    # from a test writing plain `X`, so a suite that set a field to exactly its
    # default read as two settings -- which is how this tool first passed a
    # check it should have failed. The default is not a different setting for
    # being the default.
    for name, _ in fields:
        if writes[name] and name in defaults:
            writes[name].add(defaults[name])

    return fields, reads, writes, read_sites


def main():
    fields, reads, writes, read_sites = scan()

    # A field the resolution never reads is not this tool's business: nothing
    # depends on it, so there is nothing for a test to fail to vary.
    live = [(name, kind) for name, kind in fields if reads[name]]

    never = [(n, k) for n, k in live if not writes[n]]
    once = [(n, k) for n, k in live if len(writes[n]) == 1]

    print(f'{len(fields)} ThemeData fields, {len(live)} read by non-test code')
    print(f'  never set by a test      {len(never):3}')
    print(f'  set to a single value    {len(once):3}')
    print()

    def report(title, rows):
        if not rows:
            return
        print(title)
        for name, kind in rows:
            where = ', '.join(sorted(read_sites[name])[:3])
            print(f'  {name:34} {kind:26} read in {where}')
        print()

    report('Never set by any test:', never)
    report('Set to a single value:', once)

    # The ones that decide something. A colour copied through is defended by
    # whatever checks the output; a branch nobody varies is a branch nobody has
    # taken both ways.
    def branching(kind):
        return kind in ('bool', 'Brightness', 'TargetPlatform') or kind.startswith('Option<bool>')

    suspect = [(n, k) for n, k in never + once if branching(k)]
    if suspect:
        print('Of those, the ones that decide something:')
        for name, kind in suspect:
            count = len(writes[name])
            state = 'never varied' if count <= 1 else f'{count} settings'
            print(f'  {name:34} {kind:26} {state}')
        print()
    print(f'{len(suspect)} branching theme fields the suite does not vary')
    return 0


if __name__ == '__main__':
    sys.exit(main())
