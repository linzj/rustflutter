"""A ruler: which theme field does no widget ever read?

`unwired.py` asks whether each *theme* has a reader. It reads zero and has for
a long time. But a theme with forty fields can have a reader that touches
thirty-nine of them, and the fortieth is then a field that is declared,
documented, carried through `copy_with`, interpolated by `lerp`, and answered
by nothing. A caller setting it gets silence.

Tick 228 found one by hand: `DividerThemeData::radius` is ported, blended, and
covered by a test that watches the blend -- and no widget asks for it, so
neither `Divider` nor `VerticalDivider` can round its corners. Upstream's
`Divider.build` reads `radius ?? dividerTheme.radius ?? defaults.radius`.

# What counts as a reader

Anything except the field's own paperwork. Specifically, these regions of the
declaring file are cut out before the search:

  * the `pub struct` that declares it,
  * its `lerp`, its `copy_with`, and its `with_*` builders,
  * the test modules.

Everything else in the crate counts, including the rest of the declaring file.
That last part matters: the `Resolved*` structs that turn a theme into the
values a widget draws with live *beside* the themes they resolve.
`ResolvedSlider::of` reads `data.track_shape` ten lines below the struct that
declares it.

# This rule took three tries, and the first two flattered the port's problems

The first version excluded all four theme files from being readers of any of
them, and reported 224 unread fields -- thirty of which `component_themes.rs`
reads perfectly well. The second excluded only the declaring file, and still
called `ResolvedSlider`'s reads invisible. Each correction found the tool at
fault rather than the port, which is the right way round for a ruler nobody
should trust more than they read it.

The check is by field name rather than by type, because the resolution
usually copies the field across under the same name. That makes it
under-report: a field named `color` is "read" by anything anywhere that says
`.color`. Every name it *does* report appears nowhere outside its own
paperwork at all.

# Not a queue to drive to zero blindly

Upstream itself carries theme fields nothing in the framework reads -- a
field can exist for an application to read. So a reported field is a question:
does upstream's own widget read it? If it does, this port is missing a wire.
"""
import io
import os
import re

SRC = os.path.join('K:', os.sep, 'rustflutter', 'src', 'flutter', 'rust',
                   'rustflutter', 'src')
# The files that declare themes, and so do not count as readers of them.
DECLARING = {'component_themes.rs', 'slider_theme.rs', 'theme.rs',
             'color_scheme.rs'}


def theme_fields():
    """Every `pub` field of every `*ThemeData` / `*Style` that has a `lerp`."""
    found = {}
    for name in sorted(DECLARING):
        path = os.path.join(SRC, name)
        if not os.path.exists(path):
            continue
        text = io.open(path, encoding='utf-8').read().replace('\r\n', '\n')
        blended = set(re.findall(
            r'pub fn lerp\(\s*a: &(\w+),', text))
        for m in re.finditer(r'\npub struct (\w+) \{', text):
            theme = m.group(1)
            if theme not in blended:
                continue
            body = text[m.end():]
            body = body[:body.index('\n}\n')]
            for f in re.findall(r'^\s*pub (\w+): ', body, re.MULTILINE):
                found.setdefault(theme, []).append((name, f))
    return found


def paperwork(text, theme):
    """The character spans that do not count as reads of this theme's fields."""
    spans = []
    marker = 'pub struct %s {' % theme
    if marker in text:
        start = text.index(marker)
        spans.append((start, text.index('\n}\n', start) + 3))
    for pattern in (r'pub fn lerp\(\s*a: &%s,' % theme,
                    r'impl %s \{' % theme):
        for m in re.finditer(pattern, text):
            end = text.find('\n    }\n', m.end())
            if pattern.startswith('impl'):
                end = text.find('\n}\n', m.end())
            if end > 0:
                spans.append((m.start(), end))
    for m in re.finditer(r'^#\[cfg\(test\)\]', text, re.MULTILINE):
        spans.append((m.start(), len(text)))
        break
    return spans


def reads(theme, field, files):
    """Is this field named anywhere that is not its own paperwork?"""
    for name, text in files.items():
        spans = paperwork(text, theme)
        for m in re.finditer(r'\.%s\b' % re.escape(field), text):
            if not any(start <= m.start() < end for start, end in spans):
                return True
    return False


def main():
    fields = theme_fields()
    files = {}
    for name in sorted(os.listdir(SRC)):
        if name.endswith('.rs'):
            files[name] = io.open(
                os.path.join(SRC, name), encoding='utf-8').read().replace(
                    '\r\n', '\n')
    total = 0
    unread = []
    for theme in sorted(fields):
        for where, field in fields[theme]:
            total += 1
            if not reads(theme, field, files):
                unread.append((theme, field, where))
    print('%d public theme fields across %d themes' % (total, len(fields)))
    if not unread:
        print('0 of them are named nowhere outside their own paperwork')
        return 0
    print('%d are named nowhere outside their own paperwork:' % len(unread))
    for theme, field, where in unread:
        print('  %-34s %-30s %s' % (theme, field, where))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
