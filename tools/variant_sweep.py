"""Delete one match arm's body and see whether anything notices.

`unwalked.py` asked a name-shaped question -- is this variant written down in a
test -- and its first sampled *enum* was a complete false positive.
`WidgetStatesConstraint` looked untested in all five arms and every one of them
turned out to be exercised, because tests build the values through `.and()`,
`.or()` and `.not()` rather than by naming `WidgetStatesConstraint::And`.

That is the fourth time in this project a name-shaped heuristic has stood in
for a behaviour-shaped one and been wrong (`unwired.py` managed it four times
by itself). So this asks the behaviour-shaped question instead, the way
`order_sweep.py` does: change what an arm *does*, run the suite, and see.

An arm is rewritten to return the value of the arm above it. That is a real
change with a real consequence -- two states of the enum become
indistinguishable -- and if the suite stays green, nothing is looking at the
difference.

It takes a file at a time on purpose. Each mutation triggers a full recompile,
so an arm costs about twenty seconds and the whole crate would be some hours;
the point is to work a module's worth at a time beside the other queues, not to
produce a number to drive to zero.

Where to point it first, in the order the two runs so far suggest:

* **Tables sent over a channel.** `SystemMouseCursor::kind` had seventeen of
  eighteen rows able to take their neighbour's string with the suite green, and
  `HapticFeedbackType` the same. Nothing on this side reads these strings -- the
  embedder does -- so a wrong row is invisible here by construction.
* **Arithmetic that differs by a hair between arms.** `ConstraintsTransform`'s
  width pair differ only in whether the minimum survives.
* **Bit and index assignments.** `WidgetState::bit` had three states able to
  collide with a neighbour, which would make a set unable to tell them apart.

Least worth sweeping: arms whose bodies are already distinct types, or which
some other test exercises end to end. The sweep will tell you, but it will
spend twenty seconds doing it.

What this cannot see, and `unwalked.py` can
-------------------------------------------

It rewrites **single-line match arms**, so a table with no match in it is
invisible. The one that turned up is the worst kind:
`PlatformProvidedMenuItemType` goes over the channel as `menu_type as i32`,
so the *declaration order* is the protocol and there are no arms at all --
a variant inserted in the middle renumbers eleven menu items and this sweep
would report the file as having nothing to look at.

`unwalked.py` is what found that one. The name-shaped question is a worse
question in general and was the right one there, so the two queues are worth
keeping side by side rather than replacing one with the other.

  python tools/variant_sweep.py src/widget_state.rs [more files...]
"""

import io
import os
import re
import shutil
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CRATE = os.path.join(REPO, 'src', 'flutter', 'rust', 'rustflutter')

# `Enum::Variant => expression,` on one line: the cheap, unambiguous shape.
# Multi-line and block-bodied arms are skipped rather than guessed at -- a
# sweep that mangles a body proves nothing about the arm.
ARM = re.compile(
    r'^(?P<indent>[ ]+)(?P<pattern>[A-Z]\w*::[A-Z]\w*(?:\([^)]*\))?)'
    r'\s*=>\s*(?P<body>[^\n{}]+?),\s*$',
    re.M)


def suite_is_green():
    result = subprocess.run(['cargo', 'test', '--lib'],
                            cwd=CRATE, capture_output=True, text=True)
    line = next((l for l in result.stdout.splitlines()
                 if l.startswith('test result')), '')
    return line.startswith('test result: ok')


def sweep(relative):
    path = os.path.join(CRATE, relative)
    original = io.open(path, encoding='utf-8', newline='').read()
    arms = list(ARM.finditer(original))
    # Only arms with a neighbour above them in the same match, whose body
    # differs -- copying an identical body would change nothing and prove
    # nothing.
    pairs = []
    for above, below in zip(arms, arms[1:]):
        if above.end() + 1 >= below.start() and \
                above.group('body').strip() != below.group('body').strip():
            pairs.append((above, below))
    if not pairs:
        print(f'--- {relative}: no single-line arm pairs')
        return 0

    print(f'--- {relative}: {len(pairs)} arm pairs', flush=True)
    backup = path + '.sweep'
    # A leftover backup means a previous run was killed between applying a
    # mutation and restoring it, so the file on disk is a mutant and the backup
    # is the truth. `finally` does not run when the process is killed, and a
    # sweep is exactly the kind of long job somebody stops early -- so recover
    # here rather than sweeping a corrupted file and reporting on it.
    if os.path.exists(backup):
        print(f'    recovering {relative} from a killed run', flush=True)
        shutil.copyfile(backup, path)
        original = io.open(path, encoding='utf-8', newline='').read()
        arms = list(ARM.finditer(original))
        pairs = [(a, b) for a, b in zip(arms, arms[1:])
                 if a.end() + 1 >= b.start()
                 and a.group('body').strip() != b.group('body').strip()]
    shutil.copyfile(path, backup)
    survived = 0
    try:
        for above, below in pairs:
            mutated = (original[:below.start('body')]
                       + above.group('body')
                       + original[below.end('body'):])
            io.open(path, 'w', encoding='utf-8', newline='').write(mutated)
            line_no = original.count('\n', 0, below.start()) + 1
            green = suite_is_green()
            survived += green
            label = below.group('pattern')
            # Flushed per arm: a sweep of a large file takes minutes and its
            # output is worth watching, but Python buffers stdout when it is
            # redirected, so without this the whole run appears at the end and
            # a watcher cannot tell progress from a hang.
            print(f'  {"SURVIVED" if green else "caught":>8}  '
                  f'line {line_no}: {label} answers as the arm above it',
                  flush=True)
    finally:
        shutil.copyfile(backup, path)
        os.remove(backup)
    return survived


def main():
    targets = sys.argv[1:]
    if not targets:
        print(__doc__)
        return 2
    total = sum(sweep(relative) for relative in targets)
    print(f'{total} arms nothing noticed')
    return 1 if total else 0


if __name__ == '__main__':
    sys.exit(main())
