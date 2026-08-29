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

It does not extend a doc block to the constants that follow the one it
documents. Eleven doc blocks in the port are followed by a run of constants
with no doc of their own -- `_kDialogActionsSectionMinHeight` is documented
and `CORNER_RADIUS` sits under it undocumented -- and attaching the citation
to the whole run would manufacture seventeen claims nobody made, each of them
false. Those constants are undocumented, not miscited, and that is a different
ruler's business.

What it used to miss
--------------------
Two spellings the port uses were invisible to it, and both were found by the
one MISMATCH it reported turning out to be its own:

    /// Upstream `TextSelectionToolbarTextButton._kMiddlePadding` and
    /// `_kEndPadding`, ...
    pub const BUTTON_MIDDLE_PADDING: f32 = 9.5;
    pub const BUTTON_END_PADDING: f32 = 14.5;

The citation pattern required a backtick directly before `_k`, so the
class-qualified `` `TextSelectionToolbarTextButton._kMiddlePadding` `` was not
a citation at all. Only `_kEndPadding` was read, and 9.5 was duly reported as
disagreeing with 14.5 -- a correct port, a wrong ruler, and the kind of report
that sends the next person to break working code. Three claims were qualified
that way.

Separately, only `pub const` was matched, so twenty-seven constants that are
private to their module were never checked at all while the summary line still
said "179 cited constants" as though that were all of them.
"""

import os
import re
import sys

import paths

PORT = paths.SRC
UPSTREAM = paths.upstream_src(paths.upstream_root() or '')

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
#
# `pub` is optional: a constant private to its module makes exactly the same
# claim about upstream as an exported one, and the visibility of the Rust name
# has nothing to do with whether the number is still right.
RUST_CONST = re.compile(
    r'((?:^[ \t]*///.*\n)+)[ \t]*(?:pub )?const (\w+)\s*:\s*(\w+)\s*=\s*'
    r'([^;]+);',
    re.MULTILINE)

# The owner prefix is optional and **kept**. The port writes both
# `_kEndPadding` and `TextSelectionToolbarTextButton._kMiddlePadding`, and the
# owner is not decoration: it is the one piece of information that settles a
# name upstream declares more than once. `_kPadding` is 20.0 in
# `_ContextMenuRouteStaticState`, 8.0 at the top of `slider.dart`, and an
# `EdgeInsetsDirectional` in `list_tile.dart`; a bare citation of it is
# genuinely ambiguous and this ruler refuses to guess, but a citation that says
# which class it means is not ambiguous at all. Throwing the prefix away turned
# an answerable question into an unchecked one.
CITATION = re.compile(r'`(?:(\w+)\.)?(_k\w+)`')

# `class _Foo extends Bar`, `mixin Baz`, `abstract class Qux` -- enough to say
# which class a `static const` sits inside. Dart has no nested classes, so the
# nearest declaration above an offset is its owner.
DART_OWNER = re.compile(
    r'^(?:abstract\s+|base\s+|final\s+|sealed\s+|mixin\s+)*'
    r'(?:class|mixin|enum)\s+(\w+)',
    re.MULTILINE)

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

    `owned` is the same thing keyed by `(owner, name)` for declarations that
    sit inside a class, so a citation that names its owner can be resolved
    exactly instead of being called ambiguous. Dart has no nested classes, so
    the nearest `class`/`mixin`/`enum` declaration above the offset is the
    owner; a top-level constant has none and never appears here.
    """
    scalars = {}
    structured = set()
    owned = {}
    owned_structured = set()
    for root, _dirs, files in os.walk(UPSTREAM):
        for name in files:
            if not name.endswith('.dart'):
                continue
            text = read(os.path.join(root, name))
            owners = [(m.start(), m.group(1)) for m in DART_OWNER.finditer(text)]

            def owner_of(offset):
                found = None
                for start, owner in owners:
                    if start < offset:
                        found = owner
                    else:
                        break
                return found

            scalar_at = set()
            for match in DART_SCALAR.finditer(text):
                scalar_at.add(match.start())
                value = match.group(2).replace('_', '')
                scalars.setdefault(match.group(1), set()).add(value)
                owner = owner_of(match.start())
                if owner is not None:
                    owned.setdefault((owner, match.group(1)), set()).add(value)
            for match in DART_ANY.finditer(text):
                if match.start() not in scalar_at:
                    structured.add(match.group(1))
                    owner = owner_of(match.start())
                    if owner is not None:
                        owned_structured.add((owner, match.group(1)))
    return scalars, structured, owned, owned_structured


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
    # Not `print and return 0`. A count measured against a tree that is
    # not there is not a clean bill of health, and reporting it as one is
    # how this whole suite went quiet for a drive move.
    paths.require_upstream()

    scalars, structured, owned, owned_structured = upstream_constants()
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

    def resolve(citation):
        """One citation, as `('scalar', values)` / `('structured', None)` /
        `('missing', None)`.

        A citation that names its owner is looked up as the pair first. That
        lookup is *exact* -- it can only find the one declaration the port
        actually pointed at -- so it settles names an unqualified citation
        cannot. An owner the pattern does not know (an extension, a mixin
        applied elsewhere, or a class the port names loosely) falls back to the
        bare name rather than being called MISSING: the claim is no worse off
        than an unqualified one, and reporting a live constant as gone would be
        a false alarm about the port.
        """
        owner, name = citation
        if owner:
            if (owner, name) in owned:
                return 'scalar', owned[(owner, name)]
            if (owner, name) in owned_structured:
                return 'structured', None
        if name in scalars and name in structured:
            # Upstream declares this name both ways. `switch.dart` has
            # `const double _kThumbRadius = 14.0` and
            # `sliding_segmented_control.dart` has
            # `const Radius _kThumbRadius = Radius.circular(7)`; the port's 7.0
            # is the segmented control's and is right, but only the scalar one
            # is visible here, so comparing would report a mismatch that is not
            # one. Picking the nearer-looking file would be a guess. An owner
            # would have settled it, which is why the branch above exists.
            return 'ambiguous', None
        if name in scalars:
            return 'scalar', scalars[name]
        if name in structured:
            return 'structured', None
        return 'missing', None

    for claim in claims:
        resolved = [resolve(citation) for citation in claim['cited']]
        kinds = {kind for kind, _ in resolved}
        if 'ambiguous' in kinds:
            unchecked += 1
            continue
        values = set()
        for kind, found in resolved:
            if kind == 'scalar':
                values |= found
        if not values:
            if 'structured' in kinds:
                unchecked += 1
            else:
                missing.append(claim)
            continue
        if not RUST_SCALAR.match(claim['value']):
            unchecked += 1
            continue
        # A cited name can be defined in more than one upstream file; the
        # claim holds if it matches any of them.
        if not any(same_number(claim['value'], dart) for dart in values):
            claim['upstream'] = sorted(values)
            mismatched.append(claim)

    def spell(cited):
        return '/'.join(
            '{}.{}'.format(owner, name) if owner else name
            for owner, name in cited)

    for claim in mismatched:
        print('  MISMATCH {path}:{line} {name} = {value}, but {cited} is {up}'
              .format(path=claim['path'], line=claim['line'],
                      name=claim['name'], value=claim['value'],
                      cited=spell(claim['cited']),
                      up='/'.join(claim['upstream'])))
    for claim in missing:
        print('  MISSING  {path}:{line} {name} cites {cited}, which upstream '
              'no longer defines'
              .format(path=claim['path'], line=claim['line'],
                      name=claim['name'], cited=spell(claim['cited'])))

    print('{total} cited constants, {bad} disagreeing, {gone} no longer '
          'upstream ({unchecked} structured, not compared)'
          .format(total=len(claims), bad=len(mismatched), gone=len(missing),
                  unchecked=unchecked))
    return 1 if mismatched or missing else 0


if __name__ == '__main__':
    sys.exit(main())
