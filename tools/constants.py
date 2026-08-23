"""The sixth ruler: does a number the port attributes to upstream still say that?

The port's docs are full of claims of the form

    /// Upstream's `_kTabBarHeight`.
    pub const TAB_BAR_HEIGHT: f32 = 50.0;

and nothing has ever checked the two halves against each other. A number
copied correctly in 2026 and changed upstream in 2027 leaves the doc pointing
at a constant that no longer says what the port says it says -- and the doc is
the only place the correspondence is written down, so nothing else would
notice.

This reads the claim and the source and compares them.

What it does NOT do
-------------------
It only handles scalar constants -- `const double _kFoo = 20.0;` and its int
form. Upstream's `EdgeInsets`, `TextStyle` and `Color` constants are structured
values whose Rust spelling is a different shape, and comparing those would mean
parsing two languages' literals well enough to trust a mismatch. A ruler that
is right most of the time is worse than one with a stated blind spot, because
the wrong answers are the ones you would act on. So structured constants are
counted and reported as unchecked, not guessed at.

It also cannot see a constant the port cites under a name upstream has since
renamed: that shows up as MISSING, which is the honest answer -- the claim no
longer resolves.
"""

import os
import re
import sys

PORT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                    'src', 'flutter', 'rust', 'rustflutter', 'src')
UPSTREAM = r'K:\flutter\packages\flutter\lib\src'

# `const double _kFoo = 20.0;` / `const int _kBar = 3;`, allowing a leading
# `static` inside a class body.
DART_SCALAR = re.compile(
    r'^\s*(?:static\s+)?const\s+(?:double|int|num)\s+(_?k\w+)\s*=\s*'
    r'(-?[0-9][0-9_]*\.?[0-9]*(?:[eE][-+]?[0-9]+)?)\s*;',
    re.MULTILINE)

# Any declaration of the name we can see but will not compare. `final` is here
# as well as `const` because upstream writes some of these as fields --
# `search_field.dart` has `final BorderRadius _kDefaultBorderRadius = ...`
# inside a class, and a first cut of this tool that looked only for `const`
# reported it as no longer existing upstream. It exists; it is a field.
DART_ANY = re.compile(
    r'^\s*(?:static\s+)?(?:const|final)\s+[\w<>, ?]+\s+(_?k\w+)\s*=',
    re.MULTILINE)

# A Rust constant preceded by doc lines, one of which names an upstream
# constant in backticks. The doc block is whatever run of `///` lines sits
# directly above.
RUST_CONST = re.compile(
    r'((?:^[ \t]*///.*\n)+)[ \t]*pub const (\w+)\s*:\s*(\w+)\s*=\s*'
    r'([^;]+);',
    re.MULTILINE)

CITATION = re.compile(r'`(_k\w+)`')

# A Rust scalar literal: 50.0, -0.24, 255, 1_200_000.
RUST_SCALAR = re.compile(r'^-?[0-9][0-9_]*\.?[0-9]*$')


def read(path):
    with open(path, encoding='utf-8', errors='replace') as handle:
        return handle.read()


def upstream_constants():
    """Every upstream constant we can name, with its value where scalar.

    `structured` holds only the names that have a *non*-scalar declaration.
    Keying by where the declaration starts is what separates the two: a scalar
    `const double _kFoo = 20.0;` matches both patterns at the same offset and
    is not structured, while `const Radius _kThumbRadius = ...` matches only
    the broad one. Comparing the name sets instead would put every scalar in
    both, which is what a first cut of this did -- it then called all 158
    claims ambiguous and checked none of them, while still printing a
    reassuring `0 disagreeing`.
    """
    scalars = {}
    structured = set()
    for root, _dirs, files in os.walk(UPSTREAM):
        for name in files:
            if not name.endswith('.dart'):
                continue
            text = read(os.path.join(root, name))
            scalar_at = set()
            for match in DART_SCALAR.finditer(text):
                scalar_at.add(match.start())
                scalars.setdefault(match.group(1), set()).add(
                    match.group(2).replace('_', ''))
            for match in DART_ANY.finditer(text):
                if match.start() not in scalar_at:
                    structured.add(match.group(1))
    return scalars, structured


def port_claims():
    """Every `pub const` whose doc names an upstream constant."""
    claims = []
    for root, _dirs, files in os.walk(PORT):
        for name in files:
            if not name.endswith('.rs'):
                continue
            path = os.path.join(root, name)
            text = read(path)
            for match in RUST_CONST.finditer(text):
                doc, rust_name, rust_type, value = match.groups()
                cited = CITATION.findall(doc)
                if not cited:
                    continue
                line = text.count('\n', 0, match.start()) + 1
                claims.append({
                    'path': os.path.relpath(path, PORT).replace('\\', '/'),
                    'line': line,
                    'name': rust_name,
                    'type': rust_type,
                    'value': value.strip(),
                    'cited': cited,
                })
    return claims


def same_number(rust, dart):
    try:
        return abs(float(rust.replace('_', '')) - float(dart)) < 1e-9
    except ValueError:
        return False


def main():
    if not os.path.isdir(UPSTREAM):
        print('upstream not present; nothing to check')
        return 0

    scalars, structured = upstream_constants()
    if not scalars:
        # The same guard `unwired.py` grew: an empty scan is indistinguishable
        # from a clean one, and the clean answer is the one that gets believed.
        print('ERROR: upstream is present but no scalar constants were found; '
              'the pattern is broken, not the tree', file=sys.stderr)
        return 2

    claims = port_claims()
    mismatched = []
    missing = []
    unchecked = 0

    for claim in claims:
        # A name upstream defines more than one way cannot be compared. The
        # first cut of this tool did compare them, and got `_kThumbRadius`
        # wrong: `switch.dart` has `const double _kThumbRadius = 14.0` and
        # `sliding_segmented_control.dart` has `const Radius _kThumbRadius =
        # Radius.circular(7)`. The port's 7.0 is the segmented control's and is
        # right; only the scalar one was visible here, so the tool called it a
        # mismatch. Picking the definition in the nearer-looking file would be
        # a guess about names again -- the honest answer is that the citation
        # is ambiguous and this ruler cannot settle it.
        if any(name in structured for name in claim['cited']) and \
                any(name in scalars for name in claim['cited']):
            unchecked += 1
            continue
        comparable = [name for name in claim['cited'] if name in scalars]
        if not comparable:
            if any(name in structured for name in claim['cited']):
                unchecked += 1
            else:
                missing.append(claim)
            continue
        if not RUST_SCALAR.match(claim['value']):
            unchecked += 1
            continue
        # A cited name can be defined in more than one upstream file; the
        # claim holds if it matches any of them.
        values = set()
        for name in comparable:
            values |= scalars[name]
        if not any(same_number(claim['value'], dart) for dart in values):
            claim['upstream'] = sorted(values)
            mismatched.append(claim)

    for claim in mismatched:
        print('  MISMATCH {path}:{line} {name} = {value}, but {cited} is {up}'
              .format(path=claim['path'], line=claim['line'],
                      name=claim['name'], value=claim['value'],
                      cited='/'.join(claim['cited']),
                      up='/'.join(claim['upstream'])))
    for claim in missing:
        print('  MISSING  {path}:{line} {name} cites {cited}, which upstream '
              'no longer defines'
              .format(path=claim['path'], line=claim['line'],
                      name=claim['name'], cited='/'.join(claim['cited'])))

    print('{total} cited constants, {bad} disagreeing, {gone} no longer '
          'upstream ({unchecked} structured, not compared)'
          .format(total=len(claims), bad=len(mismatched), gone=len(missing),
                  unchecked=unchecked))
    return 1 if mismatched or missing else 0


if __name__ == '__main__':
    sys.exit(main())
