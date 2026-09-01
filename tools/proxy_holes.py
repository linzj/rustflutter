"""Find single-child render objects that let a trait default answer for them.

Written after two ticks in a row found the same shape of bug. `RenderBox`
gives four intrinsics, a baseline and a dry layout a **default**, and every
one of those defaults is the right answer for a box with *no child*:

    fn max_intrinsic_width(&self, _height: f32) -> f32 { 0.0 }
    fn distance_to_baseline(&self) -> Option<f32> { None }

For a box that wraps another one they are all wrong, and wrong silently.
Tick 466 lost a menu panel to it -- an `IntrinsicWidth` above a tap region
measured zero and the panel was drawn with no width. Tick 468 found the
baseline half: a label inside an opacity sat a few pixels off from the label
beside it, because a row aligning on the baseline treats a `None` child as
having no baseline and lines it up by its top.

Nothing else in the gate can see this. A missing method is not a compile
error, not a failing test, and not a warning: it is an answer, and a
plausible one.

  python tools/proxy_holes.py            # the report; exit 1 if any is new
  python tools/proxy_holes.py --all      # including the ones with a reason
"""
import io
import os
import re
import sys

import paths

CRATE = os.path.join(paths.REPO, 'src', 'flutter', 'rust', 'rustflutter', 'src')

# What a box that wraps another has to answer for itself.
#
# `compute_dry_layout` joined the list at tick 469, and `Container` was what it
# found: the default is `Size::ZERO`, so a parent measuring without committing
# -- a flex working out its free space, an `IntrinsicWidth` measuring before it
# lays out -- got nothing back for the most-used box in the crate.
#
# `hit_test_children` is deliberately **not** here. Thirteen wrappers do not
# implement it and every one of them overrides `hit_test` itself instead, which
# is the same job done a level up; a checker that could not tell those apart
# would report thirteen findings and no bugs.
WANTED = ['max_intrinsic_width', 'min_intrinsic_width', 'max_intrinsic_height',
          'min_intrinsic_height', 'distance_to_baseline', 'compute_dry_layout']

# A field of one of these types is what makes a render object a wrapper.
CHILD = re.compile(
    r'\bchild: (BoxedRender|Option<BoxedRender>|RenderRef|BoxedWidget|Option<BoxedWidget>)')

# Types that answer with a default **on purpose**, and why. A name here is a
# claim that the trait's answer is the right one for it -- not a note to come
# back to it.
EXCUSED = {
    # Slivers do not lay out like boxes at all: their protocol is a
    # `SliverConstraints` and a `SliverGeometry`, and a box intrinsic asked of
    # one is a question about the wrong axis in the wrong space.
    'RenderSliverToBoxAdapter': 'a sliver, measured by the sliver protocol',
    'RenderSliverFillRemaining': 'a sliver, measured by the sliver protocol',
    'RenderProxySliver': 'a sliver, measured by the sliver protocol',
    'RenderSliverPersistentHeader': 'a sliver, measured by the sliver protocol',
    # A viewport is as big as it is offered and its child scrolls inside it;
    # upstream's `RenderViewport` reports no baseline for the same reason.
    'RenderViewport': 'as big as it is offered; the child scrolls inside it',
    # Upstream's `RenderAnimatedSize` is mid-animation between two sizes, so
    # what it "would like" is a moving target rather than an intrinsic.
    'RenderAnimatedSize': 'between two sizes while it animates',
}

# Methods a particular type is excused from, where the rest of the list still
# applies to it. Narrower than EXCUSED, and preferred to it: a whole type waved
# through stops being watched for everything else too. Empty today -- the one
# entry a first draft put here turned out to be an excuse for something that
# was never missing, which is worse than no excuse at all.
EXCUSED_METHOD = {}


def blocks(source):
    """Every `impl RenderBox for X` in `source`, with its body.

    The body ends at the impl's own closing brace, found by counting them --
    **not** at the next `impl`. A first draft did the easy thing and cut at the
    next one, so a type whose impl was followed by an inherent block inherited
    that block's methods and the ruler reported it as answering questions it
    never answers. An instrument that reads the neighbours' answers is worse
    than none.
    """
    found = []
    for match in re.finditer(r'impl RenderBox for (\w+) \{', source):
        depth = 0
        index = source.index('{', match.start())
        while True:
            if source[index] == '{':
                depth += 1
            elif source[index] == '}':
                depth -= 1
                if depth == 0:
                    break
            index += 1
        found.append((match.group(1), source[match.start():index]))
    return found


def holes():
    """Each wrapper with the methods it never answers."""
    found = []
    for root, _, files in os.walk(CRATE):
        for name in sorted(files):
            if not name.endswith('.rs'):
                continue
            path = os.path.join(root, name)
            source = io.open(path, encoding='utf-8').read()
            structs = {
                match.group(1): match.group(2)
                for match in re.finditer(r'pub struct (\w+) \{(.*?)\n\}', source, re.S)
            }
            for kind, body in blocks(source):
                if not CHILD.search(structs.get(kind, '')):
                    continue
                missing = [
                    method for method in WANTED
                    if ('fn %s' % method) not in body
                    and (kind, method) not in EXCUSED_METHOD
                ]
                if missing:
                    found.append((os.path.relpath(path, paths.REPO), kind, missing))
    return found


def main():
    everything = '--all' in sys.argv
    found = holes()
    red = [row for row in found if row[1] not in EXCUSED]
    excused = [row for row in found if row[1] in EXCUSED]

    for path, kind, missing in red:
        print('  %-38s %s' % (kind, ', '.join(missing)))
        print('  %-38s %s' % ('', path))
    if everything:
        print()
        print('Answering with the default on purpose:')
        for _, kind, missing in excused:
            print('  %-38s %-28s %s' % (kind, ', '.join(missing)[:28], EXCUSED[kind]))

    print()
    print('%d wrappers, %d answering the default with no reason given'
          % (len(found), len(red)))
    if red:
        print()
        print('A wrapper that does not answer lets the trait answer for it, and')
        print('the trait answers for a box with no child: zero, and no baseline.')
        print('Forward to the child -- upstream `RenderProxyBox` -- or, if the')
        print("box moves its child, add the child's offset as `RenderShiftedBox`")
        print('does. If the default really is right, name it in EXCUSED with the')
        print('reason.')
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
