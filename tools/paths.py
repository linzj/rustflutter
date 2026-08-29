"""Where the two trees are.

Every ruler in this directory reads two roots: this repository, and the
upstream Flutter checkout it is being measured against. Both used to be
written down, once per tool, as absolute `K:\\...` paths.

When the checkout moved off that drive, **nothing broke loudly**. A walk over
a directory that does not exist yields no files, so a ruler that counts
mismatches counted zero, and a ruler that counts missing members found no
members to miss. The suite went on printing a clean bill of health for a port
with six hundred and sixty-six under-covered classes in it, and `depth.py` --
the tool that picks what to work on next -- printed

    0 covered classes with 6+ upstream members

which reads exactly like "the queue is empty". A measuring instrument that
returns a passing number when it is unplugged is worse than no instrument.

Two rules follow, and this file exists to enforce them:

**The repository root is derived, never written down.** It is this file's own
parent's parent. It cannot be stale while this file is in the tree, and moving
the checkout to another drive changes nothing.

The *upstream* root cannot be derived that way, and one round of this fix
tried to have it both ways. Four tools did not write `K:` at all -- they wrote
`os.path.dirname(REPO)` and then `'flutter'`, which was true while the two
checkouts sat side by side. A grep for the dead drive letter did not find
them, so they went on reporting zero for another round: `wire_enums` said "0
disagreeing with upstream's order" and named four tables as citing nothing,
which is what an empty upstream looks like from the inside. **A path derived
from the wrong thing is still a path written down.** Upstream is asked for
here, by name, or not at all.

**A missing upstream is an error, not a zero.** `require_upstream` raises
rather than returning a path that will quietly walk to nothing. Tools that
genuinely can run without upstream should ask with `upstream_root()` and say
in their output that they did not compare -- but they must not report a count.
"""

import os

# This file's own location, which is the one thing that cannot go stale.
TOOLS = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(TOOLS)

# The Rust side of this repository.
CRATE = os.path.join(REPO, 'src', 'flutter', 'rust', 'rustflutter')
SRC = os.path.join(CRATE, 'src')
GALLERY = os.path.join(REPO, 'src', 'flutter', 'rust', 'examples', 'flutter_gallery')
OUT = os.path.join(REPO, 'src', 'out')

# Where upstream has been found before, newest first. `FLUTTER_UPSTREAM` wins
# over all of them; the list is a convenience, not a contract.
_CANDIDATES = (
    r'E:\source\flutter',
    r'K:\flutter',
)


def upstream_root():
    """The upstream Flutter checkout, or `None` if none of them is there."""
    named = os.environ.get('FLUTTER_UPSTREAM')
    if named:
        return named if os.path.isdir(named) else None
    for candidate in _CANDIDATES:
        if os.path.isdir(candidate):
            return candidate
    return None


def require_upstream():
    """The upstream checkout, or a loud failure.

    Use this wherever the tool's output is a *count*. A count computed against
    a directory that is not there is not a small number -- it is no number at
    all, and printing it as zero is how the suite lied for a whole move.
    """
    root = upstream_root()
    if root is None:
        raise SystemExit(
            'upstream Flutter not found.\n'
            'Set FLUTTER_UPSTREAM to the checkout root (the directory holding\n'
            "'packages' and 'engine'), or put it at one of: %s\n"
            'Refusing to report a count measured against nothing.'
            % ', '.join(_CANDIDATES)
        )
    return root


def upstream_src(root=None):
    """`packages/flutter/lib/src` -- the framework's Dart sources."""
    return os.path.join(root or require_upstream(), 'packages', 'flutter', 'lib', 'src')


def upstream_ui(root=None):
    """`engine/src/flutter/lib/ui` -- `dart:ui`."""
    return os.path.join(
        root or require_upstream(), 'engine', 'src', 'flutter', 'lib', 'ui'
    )


def upstream_engine(root=None):
    """`engine/src/flutter` -- the engine's own sources, C++ included."""
    return os.path.join(root or require_upstream(), 'engine', 'src', 'flutter')
