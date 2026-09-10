# -*- coding: utf-8 -*-
"""The fourth ruler: run the translated gallery and diff its render tree.

    python3 tools/dart2rust/bin/render_ruler.py <log prefix> [samples]

Runs `bin/run_main.sh` with `DART2RUST_OS=android DART2RUST_DUMP_RENDER_TREE=1`
`samples` times (default 5) and prints one line per run:

    run1 nodes=707 panics=0 typediff=0

`nodes` is how many render objects the translated program walked, `panics` how
many times it hit a stub or a refusal on the path, and `typediff` how many
lines differ from Flutter's own walk of the same page once `size=` and
`offset=` are dropped -- the structural half of the ruler, which closed at
run773. The `size=` half is a runtime gap (no `Paragraph::*` native), not a
translation gap; see STATUS.md.

Five samples rather than one because the reading has been flaky before: a
frame budget that ends mid-layout gives a short tree, and a single green run
would have hidden it.

The reference comes from `bin/render_ref.py`. Both used to live outside the
repository, which is how they were lost on 2026-09-10; the loop that produced
these lines was typed by hand each round and was never written down at all.
"""
import io
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)

REF = os.path.join(TOOL, '.build', 'scratch', 'ref_render_walk_settled.txt')
BEGIN = '=== DART2RUST RENDER TREE BEGIN ==='
END = '=== DART2RUST RENDER TREE END ==='

#: `size=` and `offset=` carry the layout numbers the runtime cannot produce
#: headlessly; the structural ruler is the rest of the line.
#:
#: The value is parenthesised and has a space in it -- `size=Size(800.0,
#: 600.0)`. Written as `[^ ]*` this cut at the comma and left ` 600.0)`
#: behind, which is a layout number, and the first reading was 254
#: differences in a tree that had none (2026-09-10).
_MEASURED = re.compile(r' (?:size|offset)=\w*\([^)]*\)')


def types(lines):
    return [_MEASURED.sub('', line).rstrip() for line in lines if line.strip()]


def read(path):
    if not os.path.exists(path):
        return []
    return io.open(path, encoding='utf-8', errors='replace').read().splitlines()


def main():
    if len(sys.argv) < 2:
        print(__doc__.strip().splitlines()[2].strip())
        return 2
    prefix, samples = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 5
    if not os.path.exists(REF):
        print('no reference at', REF, '-- run bin/render_ref.py')
        return 1
    want = types(read(REF))
    env = dict(os.environ, DART2RUST_OS='android',
               DART2RUST_DUMP_RENDER_TREE='1')
    worst = 0
    for i in range(1, samples + 1):
        log = '%s%d' % (prefix, i)
        subprocess.run([os.path.join(HERE, 'run_main.sh'), log], env=env,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        text = '\n'.join(read(log + '.run'))
        panics = text.count('panicked at')
        if BEGIN in text and END in text:
            got = types(text.split(BEGIN, 1)[1].split(END, 1)[0].splitlines())
        else:
            got = []
        diff = sum(1 for a, b in zip(got, want) if a != b) \
            + abs(len(got) - len(want))
        worst = max(worst, diff + panics)
        print('run%d nodes=%d panics=%d typediff=%d'
              % (i, len(got), panics, diff))
        sys.stdout.flush()
    return 0 if worst == 0 else 1


if __name__ == '__main__':
    raise SystemExit(main())
