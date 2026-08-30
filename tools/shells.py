# -*- coding: utf-8 -*-
"""Which ported types are **data with no widget behind them**?

`depth.py` picks the next class to work on by counting how many of upstream's
members this port has. It cannot tell a widget from a bag of numbers, and three
rounds running (386, 388, 390) opened by discovering that its queue head was
the second kind:

  * `tabs::TabBarView` -- `child_count`, `viewport_fraction`, `matches()`, and
    nothing that builds; the pages are assembled by the application.
  * `dropdown.rs` -- **no `Component` impl in the file at all**;
    `DropdownMenuItem.child` is a `u64` id rather than a widget.
  * `radio_group::RadioGroup` -- registration and the single-selection check,
    no build.
  * `magnifier.rs` -- `RawMagnifier`, `MagnifierDecoration`,
    `MagnifierController::shift_within_bounds`; nothing in the crate shows a
    loupe.
  * `selection_area::SelectableText` -- a `String`, a `max_lines`, and
    `is_editable()`.

Each cost a round's opening minutes to rediscover, and each time the answer
changed what the round could be: "port the six missing members" is a different
job from "port the widget these members describe".

**Not one of the sixteen rulers. It gates nothing and always exits 0.** A row
here is a question, not a fault -- the same footing as `descent.py` and
`spoken.py`. Plenty of upstream classes are legitimately data on both sides
(`BorderRadius`, `TextStyle`), and plenty of this port's helper structs are
resolvers by design (`ResolvedDivider`). What the report is for is the *third*
kind: a type named after an upstream **widget**, ported as a struct, with
nothing that could ever put it on the screen.

That is why the listing is split. A type is only interesting here if upstream's
class extends a widget, so the report reads upstream for `extends
StatelessWidget` / `StatefulWidget` / `RenderObjectWidget` and asks the port
whether the matching type has any of:

  * `impl Component for X` / `impl StatefulComponent for X` -- it builds;
  * `impl RenderBox for X` -- it *is* a render object;
  * a free function returning `AnyWidget` named after it;
  * a method handing back a type that itself builds, resolved to a fixed
    point -- this port splits three upstream widgets into a settings bag and a
    `ControlTile` that draws any of them, so the bag is a widget at one remove.

Calibrated before it was kept, against the shells above and against fourteen
types known to build (`Align`, `ClipOval`, `ColoredBox`, `Divider`,
`VerticalDivider`, `TabBar`, `PopupMenu`, `Spinner`, `ProgressBar`, `Checkbox`,
`Radio`, `Card`, `ListTile`, `AppBar`): **0 of the fourteen listed, 4 of the 5
shells listed**. A report that could not separate those two lists would be
measuring nothing, which is this project's standing rule for instruments.

The first two attempts failed that check and are worth recording, because both
failures were the same shape -- **this port has no single spelling of "widget"**:

  * v1 looked for `impl Component`/`impl RenderBox` and free functions, and
    called `Align`, `ClipOval` and `ColoredBox` data. They are *facades*:
    `pub struct Align;` with `Align::new(..) -> RenderAlign`, no trait at all.
  * v2 accounted for facades and still called `ClipOval` data, because its
    `new` returns `crate::render::RenderClipOval` -- the same shape written
    with a path.

It reads only the **first** `pub fn ... ->` in each `impl` block, so a type
whose *second* method is the one handing back a widget is under-reported. That
direction is the safe one: this report's cost is a false positive, which sends a
round to repair something that works -- round 393 nearly spent itself on
`CheckboxListTile` before reading the code and finding
`widget(id, title) -> ControlTile` already there.

The one known shell it misses is `DropdownMenuItem`, and the miss is on the
*upstream* side: it extends a private `_DropdownMenuItemContainer` rather than
a widget class, so the scan below never sees it as a widget. Indirect
subclasses are outside what this reads.

# What it found on its first run

`ActionChip`, `ChoiceChip`, `FilterChip` and `InputChip` are all
`pub struct X(pub ChipParts)` in `controls.rs` with **no `Component` impl
between them**. Only `controls::Chip` builds. Each variant can be constructed,
configured, and never put on the screen -- which is the fault this report
exists to make visible before a round starts rather than after.
"""
import os
import re

import paths

PORT = paths.SRC
# `require_upstream`, not a bare lookup: this report's headline is a *count*,
# and `paths.py` exists because a count measured against a directory that is
# not there prints as zero and reads as "nothing to do".
UPSTREAM = os.path.join(
    paths.require_upstream(), 'packages', 'flutter', 'lib', 'src'
)

WIDGET_CLASS = re.compile(
    r'^class (\w+)(?:<[^>]*>)?\s+extends\s+'
    r'(?:StatelessWidget|StatefulWidget|RenderObjectWidget|'
    r'SingleChildRenderObjectWidget|MultiChildRenderObjectWidget|'
    r'LeafRenderObjectWidget|InheritedWidget|ProxyWidget)',
    re.M)

PORT_STRUCT = re.compile(r'^pub struct (\w+)', re.M)

# `impl Foo {` ... `pub fn bar(..) -> Baz`, for the transitive rule: a
# settings type whose method hands back the widget that draws it.
RETURNS = re.compile(
    r'^impl(?:<[^>]*>)?\s+(\w+)[^{\n]*\{'
    r'(?:(?!^impl)[\s\S])*?'
    r'pub fn \w+\([^)]*\)\s*->\s*([\w:]+)',
    re.M)

# `impl Foo {` ... `pub fn bar(..) -> RenderBaz` / `-> AnyWidget`, anywhere
# in that block. Matched with the impl header and the return type in one
# pass so a nested block cannot detach the two.
FACADE = re.compile(
    r'^impl(?:<[^>]*>)?\s+(\w+)[^\n]*\{'
    r'(?:(?!^impl)[\s\S])*?'
    r'pub fn \w+\([^)]*\)\s*->\s*(?:[\w:]*Render\w+|[\w:]*AnyWidget|impl RenderBox)',
    re.M)


def upstream_widgets():
    """Every upstream class that is a widget, by name."""
    names = {}
    for root, _dirs, files in os.walk(UPSTREAM):
        for name in files:
            if not name.endswith('.dart'):
                continue
            path = os.path.join(root, name)
            try:
                text = open(path, encoding='utf-8').read()
            except OSError:
                continue
            for match in WIDGET_CLASS.finditer(text):
                names.setdefault(match.group(1), os.path.relpath(path, UPSTREAM))
    return names


def port_files():
    for root, _dirs, files in os.walk(PORT):
        for name in files:
            if name.endswith('.rs'):
                yield os.path.join(root, name)


def main():
    widgets = upstream_widgets()
    if not widgets:
        raise SystemExit('no upstream widgets found -- see paths.py')

    builds = set()
    declared = {}
    returns = {}
    for path in port_files():
        try:
            text = open(path, encoding='utf-8').read()
        except OSError:
            continue
        where = os.path.relpath(path, PORT)
        for match in PORT_STRUCT.finditer(text):
            declared.setdefault(match.group(1), where)
        for pattern in (
            r'impl(?:<[^>]*>)?\s+Component\s+for\s+(\w+)',
            r'impl(?:<[^>]*>)?\s+StatefulComponent\s+for\s+(\w+)',
            r'impl(?:<[^>]*>)?\s+RenderBox\s+for\s+(\w+)',
        ):
            for match in re.finditer(pattern, text):
                builds.add(match.group(1))
        # A free function that hands back a widget, named after the type it
        # stands for: `pub fn ink_well(..) -> AnyWidget`.
        for match in re.finditer(r'pub fn (\w+)\([^)]*\)\s*->\s*AnyWidget', text):
            builds.add(''.join(part.title() for part in match.group(1).split('_')))
        # The fourth spelling, and the one that broke the first version of
        # this report: a **facade** -- `pub struct Align;` whose `new` hands
        # back a `RenderAlign`. No trait is involved, so nothing above sees it,
        # and `Align`, `ClipOval` and `ColoredBox` were all listed as data with
        # nothing behind them.
        for match in FACADE.finditer(text):
            builds.add(match.group(1))
        # What each type's methods hand back, for the transitive pass below.
        for match in RETURNS.finditer(text):
            owner, returned = match.group(1), match.group(2)
            returns.setdefault(owner, set()).add(returned.split('::')[-1])

    # A type also counts as building when one of its own methods hands back a
    # type that builds. `CheckboxListTile::widget(id, title) -> ControlTile` is
    # the case: this port splits three upstream widgets into a settings bag and
    # one `ControlTile` that draws any of them, so the bag is a widget at one
    # remove. Resolved to a fixed point, because that chain can be longer than
    # one link.
    #
    # Round 393 added this after the report sent that round at the three list
    # tiles as though they could not reach the screen. They can; the API is two
    # steps. **A report with false positives spends rounds repairing what is
    # not broken**, which is worse than one that lists too little.
    while True:
        grew = False
        for owner, returned in returns.items():
            if owner in builds:
                continue
            if any(name in builds for name in returned):
                builds.add(owner)
                grew = True
        if not grew:
            break

    shells = sorted(
        (name, declared[name], widgets[name])
        for name in declared
        if name in widgets and name not in builds
    )
    building = sum(1 for name in declared if name in widgets and name in builds)

    print('%d types named after an upstream widget; %d of them build, '
          '%d are data with nothing behind them'
          % (building + len(shells), building, len(shells)))
    print()
    for name, where, source in shells:
        print('  %-34s %-26s upstream %s' % (name, where, source))
    print()
    print('A row is a question, not a fault: some of these are deliberately')
    print('the data half of a mechanism whose walk is not ported. What the')
    print('report is for is knowing that before a round starts, not after.')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
