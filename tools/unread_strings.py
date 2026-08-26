"""Localization strings with nobody to say them, or saying the wrong thing.

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
-- that is outside a `#[cfg(test)]` block **and outside a comment**. A test
that names a string proves the string is spelled a certain way; a doc comment
that names it proves someone wrote its name down. Neither proves anything says
it, which is the whole distinction, so tests and comments are both stripped
before counting.

The comment rule was added after the reader-chain below was checked by cutting
its only real caller and watching nothing happen: the file still mentioned the
helper in a doc link, and that counted. It does not now.

# Read at one remove

Some strings are consumed by a function in `material_app.rs` rather than by a
widget elsewhere -- `expansion_tile_hint` pairs four of them into a sentence,
and the crossing it performs is the reason they exist.  A rule of "mentioned
outside this file" called all four unread, which was the rule being too crude
rather than the strings being unused.

So a constant also counts as read when it is used inside a `pub fn` in this
file whose own name appears outside it.  That is one hop and no more: the
function has to have a caller of its own, or the chain is as unread as the
constant was.

# And what it says

Having a reader is half of it.  Eighteen strings arrived in one go from
`pickers.rs`, which had held them privately since before this crate had a
localization layer, and two of them had drifted: upstream's
`dateRangeStartLabel` and `dateRangeEndLabel` capitalise the D -- "Start Date",
"End Date" -- and the port had lowercased it.  Nothing noticed, because no test
named either string and there was nothing comparing them to upstream.

So this also reads upstream's `DefaultMaterialLocalizations` and compares.  The
Dart getter is derived from the Rust constant by the obvious rule --
`DATE_RANGE_START_LABEL` to `dateRangeStartLabel` -- and a constant whose getter
cannot be found is reported rather than skipped, because a name that does not
resolve is usually a name that is wrong.

Only the getters whose body is a plain string literal can be compared.  The
ones upstream builds with `switch` or interpolation are listed as uncompared,
the way `constants.py` lists its structured ones: an honest gap beats a check
that quietly passes.

# Two tables, two upstream files

`DefaultWidgetsLocalizations` is the other one, in `localizations.rs`, and it
answers to `widgets/localizations.dart` rather than the Material file.  Keeping
them apart matters in both directions: `scanTextButtonLabel` was declared on
this crate's widgets trait and belongs to Material's, which nothing noticed
because nothing read it either.

The widgets side is checked for **agreement and membership** but not for
readers.  Its strings are trait methods that exist to be implemented rather
than constants that exist to be used, so "nobody calls it" is not the same
finding there -- a `WidgetsLocalizations` implementation is a bundle, and a
bundle with a hole in it is the thing that would be wrong.

Usage:
  python tools/unread_strings.py
"""
import os
import re

CRATE = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                     '..', 'src', 'flutter', 'rust', 'rustflutter', 'src')
HOME = os.path.join(CRATE, 'material_app.rs')
UPSTREAM = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..',
                        'flutter', 'packages', 'flutter', 'lib', 'src',
                        'material', 'material_localizations.dart')
WIDGETS_UPSTREAM = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), '..', '..', 'flutter',
    'packages', 'flutter', 'lib', 'src', 'widgets', 'localizations.dart')
WIDGETS_HOME = os.path.join(CRATE, 'localizations.rs')

# `fn snake_name(&self) -> &str { "Value" }`, however rustfmt laid it out.
WIDGETS_STRING = re.compile(
    r'fn (?P<name>[a-z_0-9]+)\(&self\)[^{]*\{\s*"(?P<value>[^"]*)"\s*\}')

# `String get name => 'value';` -- only the ones that are a plain literal.
GETTER = re.compile(r"String get (?P<name>\w+) => '(?P<value>(?:[^'\\]|\\.)*)';")


def camel(constant):
    """`DATE_RANGE_START_LABEL` -> `dateRangeStartLabel`."""
    head, *rest = constant.lower().split('_')
    return head + ''.join(word.capitalize() for word in rest)


def upstream_strings():
    """Upstream's plain-literal getters, by name. Empty if it is not there."""
    if not os.path.exists(UPSTREAM):
        return None
    text = open(UPSTREAM, encoding='utf-8', errors='replace').read()
    return {m.group('name'): m.group('value').replace("\\'", "'")
            for m in GETTER.finditer(text)}

# `pub const NAME: &'static str = "...";` inside the localizations impl.
STRING = re.compile(r"pub const (?P<name>[A-Z][A-Z0-9_]*): &'static str = \"(?P<value>[^\"]*)\"")


def strip_comments(text):
    """Line comments removed, string literals left alone.

    Crude on purpose -- it walks the text once tracking whether it is inside a
    string, so a `//` inside one survives and a comment containing a quote does
    not confuse it.
    """
    out, index, in_string = [], 0, False
    while index < len(text):
        char = text[index]
        if in_string:
            if char == '\\':
                out.append(text[index:index + 2])
                index += 2
                continue
            if char == '"':
                in_string = False
            out.append(char)
            index += 1
            continue
        if char == '"':
            in_string = True
            out.append(char)
            index += 1
            continue
        if text.startswith('//', index):
            end = text.find('\n', index)
            index = len(text) if end < 0 else end
            continue
        out.append(char)
        index += 1
    return ''.join(out)


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


# `pub fn name(...) { ... }` in the localizations file, body included, so a
# constant used by one can be credited to that function's own callers.
LOCAL_FN = re.compile(
    r'^    pub fn (?P<name>\w+)\([^)]*\)[^{]*\{(?P<body>.*?)^    \}',
    re.M | re.S)


# `pub const NAME: &'static str = "...";` -- a declaration, which is not a
# reader of the constant of that name somewhere else.
DECLARATION = re.compile(r"^\s*pub const \w+: &'static str = .*$", re.M)


def local_helpers():
    """Which constants each `pub fn` in the localizations file mentions."""
    text = strip_tests(open(HOME, encoding='utf-8', errors='replace').read())
    return {m.group('name'): strip_comments(m.group('body'))
            for m in LOCAL_FN.finditer(text)}


readers = {}
helper_callers = {}
for root, _dirs, files in os.walk(CRATE):
    for name in sorted(files):
        if not name.endswith('.rs') or os.path.join(root, name) == HOME:
            continue
        path = os.path.join(root, name)
        body = strip_comments(
            strip_tests(open(path, encoding='utf-8', errors='replace').read()))
        where = os.path.relpath(path, CRATE).replace(os.sep, '/')
        # A file that *declares* a constant of the same name is not thereby
        # a reader of this one. `cupertino_app.rs` grew its own
        # `LOOK_UP_BUTTON_LABEL`, and the Material constant immediately had a
        # second "reader" that says nothing about it. A declaration is not a
        # use, and the two are the same six words to a substring search.
        body = DECLARATION.sub('', body)
        helper_callers[where] = body
        for constant, _value in localization_strings():
            if constant in body:
                readers.setdefault(constant, []).append(where)

# A constant used by a local helper counts as read when the helper does.
helpers = local_helpers()
for helper, body in helpers.items():
    called_from = [where for where, seen in helper_callers.items()
                   if helper in seen]
    if not called_from:
        continue
    for constant, _value in localization_strings():
        if constant in body:
            readers.setdefault(constant, []).extend(
                '%s (via %s)' % (where, helper) for where in called_from)

strings = localization_strings()
unread = [(name, value) for name, value in strings if name not in readers]

upstream = upstream_strings()
disagreeing, uncompared, unresolved = [], [], []
if upstream is not None:
    for name, value in strings:
        getter = camel(name)
        if getter not in upstream:
            (uncompared if getter in open(UPSTREAM, encoding='utf-8',
                                         errors='replace').read()
             else unresolved).append((name, getter))
        elif upstream[getter] != value:
            disagreeing.append((name, value, upstream[getter]))

print('%d localization strings, %d with nothing to say them, %d disagreeing '
      'with upstream' % (len(strings), len(unread), len(disagreeing)))
if upstream is None:
    print('(upstream not found, so nothing was compared)')
else:
    print('%d not compared -- upstream builds them rather than declaring a '
          'literal; %d whose upstream getter could not be found'
          % (len(uncompared), len(unresolved)))
print()
for name, value, theirs in disagreeing:
    print('  DISAGREES %-28s port %-22s upstream "%s"'
          % (name, '"' + value + '"', theirs))
for name, getter in unresolved:
    print('  NO SUCH GETTER UPSTREAM   %-28s looked for %s' % (name, getter))
print()
for name, value in strings:
    where = readers.get(name)
    mark = ', '.join(sorted(set(where))) if where else '-- NOBODY SAYS THIS'
    print('  %-32s %-22s %s' % (name, '"' + value + '"', mark))

# -- The widgets table ------------------------------------------------------

widgets_upstream = None
if os.path.exists(WIDGETS_UPSTREAM):
    widgets_upstream = {
        m.group('name'): m.group('value').replace("\\'", "'")
        for m in GETTER.finditer(
            open(WIDGETS_UPSTREAM, encoding='utf-8', errors='replace').read())}

widgets_port = {
    m.group('name'): m.group('value')
    for m in WIDGETS_STRING.finditer(
        open(WIDGETS_HOME, encoding='utf-8', errors='replace').read())}
# `resource_type` is the delegate's own tag rather than a localized string.
widgets_port.pop('resource_type', None)

print()
if widgets_upstream is None:
    print('DefaultWidgetsLocalizations: upstream not found, nothing compared')
else:
    wrong, elsewhere, absent = [], [], []
    for snake, value in sorted(widgets_port.items()):
        getter = camel(snake)
        if getter not in widgets_upstream:
            elsewhere.append((snake, getter))
        elif widgets_upstream[getter] != value:
            wrong.append((snake, value, widgets_upstream[getter]))
    for getter in sorted(widgets_upstream):
        if not any(camel(s) == getter for s in widgets_port):
            absent.append(getter)
    print('DefaultWidgetsLocalizations: %d strings, %d disagreeing, %d not on '
          'upstream\'s widgets class, %d of upstream\'s missing here'
          % (len(widgets_port), len(wrong), len(elsewhere), len(absent)))
    for snake, ours, theirs in wrong:
        print('  DISAGREES %-24s port "%s"  upstream "%s"' % (snake, ours, theirs))
    for snake, getter in elsewhere:
        print('  NOT A WIDGETS STRING      %-24s looked for %s' % (snake, getter))
    for getter in absent:
        print('  MISSING FROM THE BUNDLE   %-24s "%s"'
              % (getter, widgets_upstream[getter]))


# -- The Cupertino table ----------------------------------------------------
#
# Added when `cupertino_label` brought seven strings in. Without it those seven
# could drift from upstream exactly as `dateRangeStartLabel` did, which is the
# case that made this file exist -- and the two tables disagree *on purpose*
# in two places ("Select all"/"Select All", "Share"/"Share..."), so a reader
# comparing them by eye would talk themselves out of both.

CUPERTINO_HOME = os.path.join(CRATE, 'cupertino_app.rs')
CUPERTINO_UPSTREAM = os.path.join(
    os.path.dirname(os.path.abspath(__file__)), '..', '..', 'flutter',
    'packages', 'flutter', 'lib', 'src', 'cupertino', 'localizations.dart')

cupertino_upstream = None
if os.path.exists(CUPERTINO_UPSTREAM):
    cupertino_upstream = {
        m.group('name'): m.group('value').replace("\\'", "'")
        for m in GETTER.finditer(
            open(CUPERTINO_UPSTREAM, encoding='utf-8', errors='replace').read())}

cupertino_text = open(CUPERTINO_HOME, encoding='utf-8', errors='replace').read()
cupertino_start = cupertino_text.index('impl DefaultCupertinoLocalizations')
cupertino_port = {m.group('name'): m.group('value')
                  for m in STRING.finditer(cupertino_text[cupertino_start:])}

print()
if cupertino_upstream is None:
    print('DefaultCupertinoLocalizations: upstream not found, nothing compared')
else:
    wrong, elsewhere = [], []
    for snake, value in sorted(cupertino_port.items()):
        getter = camel(snake)
        if getter not in cupertino_upstream:
            elsewhere.append((snake, getter))
        elif cupertino_upstream[getter] != value:
            wrong.append((snake, value, cupertino_upstream[getter]))
    print('DefaultCupertinoLocalizations: %d strings, %d disagreeing, %d whose '
          'upstream getter could not be found' % (len(cupertino_port), len(wrong),
                                                  len(elsewhere)))
    for snake, ours, theirs in wrong:
        print('  DISAGREES %-24s port "%s"  upstream "%s"' % (snake, ours, theirs))
    for snake, getter in elsewhere:
        print('  NO SUCH GETTER UPSTREAM   %-24s looked for %s' % (snake, getter))
