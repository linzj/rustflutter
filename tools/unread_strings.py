"""Localization strings with nobody to say them.

`DefaultMaterialLocalizations` upstream has a hundred and fifty-eight members.
Copying that table down would put a hundred and thirty unread strings in the
crate, and an unread string is indistinguishable from a wrong one -- the same
argument `unwired.py` makes about theme fields, and the reason the strings here
have been added one at a time, each from a caller that already wanted it.

That discipline was stated in a commit and broken in the same commit:
`SEARCH_FIELD_LABEL` went in with the other three and had no caller for a tick.
Nothing noticed, because nothing was looking. This looks.

# What counts as a reader

A mention anywhere outside `material_app.rs` -- the file the constants live in
-- and outside a `#[cfg(test)]` block. A test that names a string proves the
string is spelled a certain way; it does not prove anything says it. That is
the whole distinction, so the test modules are stripped before counting.

A constant read only by another constant in the same file would slip through,
but no such thing exists here and the shape is not one this crate reaches for.

Usage:
  python tools/unread_strings.py
"""
import os
import re

CRATE = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                     '..', 'src', 'flutter', 'rust', 'rustflutter', 'src')
HOME = os.path.join(CRATE, 'material_app.rs')

# `pub const NAME: &'static str = "...";` inside the localizations impl.
STRING = re.compile(r"pub const (?P<name>[A-Z][A-Z0-9_]*): &'static str = \"(?P<value>[^\"]*)\"")


def strip_tests(text):
    out, index = [], 0
    for match in re.finditer(r'#\[cfg\(test\)\]\s*mod\s+\w+\s*\{', text):
        brace = text.index('{', match.end() - 1)
        depth, end = 0, len(text)
        for pos in range(brace, len(text)):
            if text[pos] == '{':
                depth += 1
            elif text[pos] == '}':
                depth -= 1
                if depth == 0:
                    end = pos + 1
                    break
        out.append(text[index:match.start()])
        index = end
    out.append(text[index:])
    return ''.join(out)


def localization_strings():
    """The constants declared on `DefaultMaterialLocalizations`."""
    text = open(HOME, encoding='utf-8', errors='replace').read()
    start = text.index('impl DefaultMaterialLocalizations')
    return [(m.group('name'), m.group('value'))
            for m in STRING.finditer(text[start:])]


readers = {}
for root, _dirs, files in os.walk(CRATE):
    for name in sorted(files):
        if not name.endswith('.rs') or os.path.join(root, name) == HOME:
            continue
        path = os.path.join(root, name)
        body = strip_tests(open(path, encoding='utf-8', errors='replace').read())
        where = os.path.relpath(path, CRATE).replace(os.sep, '/')
        for constant, _value in localization_strings():
            if constant in body:
                readers.setdefault(constant, []).append(where)

strings = localization_strings()
unread = [(name, value) for name, value in strings if name not in readers]

print('%d localization strings, %d with nothing to say them'
      % (len(strings), len(unread)))
print()
for name, value in strings:
    where = readers.get(name)
    mark = ', '.join(sorted(set(where))) if where else '-- NOBODY SAYS THIS'
    print('  %-30s %-22s %s' % (name, '"' + value + '"', mark))
