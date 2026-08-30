"""What each component actually says to a screen reader.

# Not a ruler

This never fails. It runs the `spoken_census` test in `controls.rs` and prints
what came out, and the judgement of whether any line is wrong is a person's.
`descent.py` has the same shape and the same reason.

# Why it counts what it counts

Tick 369 surveyed which components never mention `semantics` in their `build`
and called the rest silent. Tick 377 found `Badge` on that list while being
perfectly audible: its count is a `Text`, and the walk annotates a paragraph by
itself. **Mentioning semantics and reaching a reader are different questions**,
and a survey of the first answers the second only by accident.

So this asks the second directly: mount the component, run the real semantics
walk, print every node that says anything -- words, a value, a flag, an action.

# How to read it

Silence is not automatically a fault. A container that adds nothing of its own
is correct, and a `Scaffold` that announced itself would be noise on every
screen. **Silence is a gap where the component has a role or state its children
cannot carry**: which tab you are on, whether a box is ticked, that a modal has
opened.

That is how `NavigationRail` was found in tick 378 -- it printed

    "H", "Home", "S", "Saved"

four bare stops, where its two siblings printed "Tab 1 of 2 ... +flags", and
the state a reader needs (which destination is current) was in none of them.
"""

import os
import subprocess
import sys

CRATE = os.path.join('src', 'flutter', 'rust', 'rustflutter')


def main():
    if not os.path.isdir(CRATE):
        print('run me from the repository root')
        return 0
    result = subprocess.run(
        ['cargo', 'test', '--lib', '--', 'spoken_census', '--nocapture'],
        cwd=CRATE, capture_output=True, encoding='utf-8', errors='replace')
    output = (result.stdout or '') + (result.stderr or '')
    lines = [line for line in output.splitlines() if line.startswith('SPOKEN ')]
    if not lines:
        print('the census did not run; `cargo test -- spoken_census` says:')
        for line in output.splitlines():
            if line.startswith('error'):
                print('   ', line)
        return 0
    for line in lines:
        print(line[len('SPOKEN '):])
    print()
    print('%d components. Silence is a gap only where a role or state is '
          'missing -- see this file.' % len(lines))
    return 0


if __name__ == '__main__':
    sys.exit(main())
