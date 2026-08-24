"""Which paint methods hand the canvas numbers nobody checks.

Until tick 126 the test stubs took every `rf_canvas_draw_*` call and dropped
it, so every draw call in the crate agreed with every possible implementation
of itself.  A `paint` that drew the wrong rectangle, the wrong part of an
image, or nothing at all, passed exactly as well as one that was right.  Two
real defects were sitting in that blind spot when it was opened -- an image
source rect in the wrong units, and a slider track whose two halves could not
be told apart.

The stubs record now (`engine_test_stubs::drawn`), which turns "unobservable"
into "unobserved".  This counts what is still the second.

# It is a screen, not a gate

It cannot read zero any time soon, and it should not be made to.  Some draw
calls are worth pinning and some are a rectangle handed straight through from
a caller who already decided everything.  What it is for is making the
remainder countable, and making it obvious when a file with real geometry in
it has never once been looked at.

The check is per **file** rather than per method, because a test module sits at
the bottom of the file it tests.  A file that calls `drawn()` anywhere has been
thought about; one that never does has not.  That is coarse in the forgiving
direction: a file with twenty draw calls and one recorder test counts as
covered here while nineteen of them may still be unchecked.

Usage:
  python tools/unpainted.py
"""
import os
import re

PORT = os.path.join(os.path.dirname(os.path.abspath(__file__)),
                    '..', 'src', 'flutter', 'rust', 'rustflutter', 'src')

# `canvas.draw_foo(` or `context.canvas().draw_foo(` -- the calls that put
# something on the glass.  save/restore/clip are transform bookkeeping rather
# than marks, and are not counted.
DRAW = re.compile(r'\bcanvas(?:\(\))?\s*\n?\s*\.\s*(draw_\w+)\s*\(')

# A test that reads what the canvas was told.
OBSERVES = re.compile(r'\bdrawn\s*\(\s*\)')


def strip_tests(text):
    """The file without its `#[cfg(test)] mod` blocks."""
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


rows = []
for root, _dirs, files in os.walk(PORT):
    for name in sorted(files):
        if not name.endswith('.rs'):
            continue
        path = os.path.join(root, name)
        text = open(path, encoding='utf-8', errors='replace').read()
        calls = DRAW.findall(strip_tests(text))
        if not calls:
            continue
        where = os.path.relpath(path, PORT).replace(os.sep, '/')
        rows.append((len(calls), where, bool(OBSERVES.search(text)), sorted(set(calls))))

rows.sort(reverse=True)
watched = [row for row in rows if row[2]]
total = sum(row[0] for row in rows)
seen = sum(row[0] for row in watched)

print('%d draw calls across %d files; %d of those files have a test that reads '
      'back what the canvas was told' % (total, len(rows), len(watched)))
print('%d draw calls sit in files nothing observes' % (total - seen))
print()
print('-- files with draw calls and no recorder test:')
for count, where, observed, kinds in rows:
    if observed:
        continue
    print('  %3d  %-26s %s' % (count, where, ' '.join(k[5:] for k in kinds)))
print()
print('-- files with at least one:')
for count, where, observed, _kinds in rows:
    if observed:
        print('  %3d  %s' % (count, where))
