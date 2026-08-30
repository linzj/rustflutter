# -*- coding: utf-8 -*-
"""Which tests lay out a *rebuilt* tree by descending from the root?

Tick 337: a mark stops at a relayout boundary and leaves every ancestor
clean, so `root.layout(..)` early-returns at the first clean object and never
reaches what was marked.  A frame calls `schedule_root_layout` +
`flush_layout` instead (`app.rs`, beside the comment that says why a descent
cannot stand in).  Laying out once after a first build is safe -- nothing is
clean above anything yet.  Crossing a rebuild is not necessarily safe.

**Not one of the sixteen rulers, and it does not gate anything.**  A row here
is a question, not a fault: whether a descent matters depends on what the
test then asserts, and tick 338 converted the most suspicious row
(`a_subtree_that_did_change_is_laid_out_again`) to the frame's path and found
it still passed.  Wiring this to an exit code would mean claiming twenty
verdicts nobody has checked.

Tick 338 also tried to answer the question mechanically -- print whenever a
descent begins with boundaries still queued in `DIRTY_BOUNDARIES` -- and got
zero across the whole suite.  That zero was **not** trusted and the probe was
not kept: it was never shown to fire on the one case known to fail, so it
could as easily have been measuring nothing.  This project has been here
three times (ticks 288, 291, 301) and the rule earned there is that an
instrument nobody has seen catch a real fault reports a number, not a fact.

  python tools/descent.py        # the list; always exit 0
"""
import io
import os
import re

ROOT = r'D:\linzjUbuntu2204\rustflutter\src\flutter\rust\rustflutter\src'

FN = re.compile(r'^\s*(?:pub\s+)?fn\s+(\w+)', re.M)


def functions(text):
    """(name, body) for every fn, by brace matching."""
    for match in FN.finditer(text):
        start = text.find('{', match.end())
        if start < 0:
            continue
        depth = 0
        for index in range(start, len(text)):
            if text[index] == '{':
                depth += 1
            elif text[index] == '}':
                depth -= 1
                if depth == 0:
                    yield match.group(1), text[start:index]
                    break


risky = []
safe_rebuilders = 0
for base, _, names in os.walk(ROOT):
    for name in names:
        if not name.endswith('.rs'):
            continue
        path = os.path.join(base, name)
        text = io.open(path, encoding='utf-8', errors='ignore').read()
        for fn, body in functions(text):
            if 'build_render_tree' not in body:
                continue
            if not re.search(r'\.layout\(', body):
                continue
            # Did this test cross a rebuild?
            rebuilds = len(re.findall(r'\.rebuild\(', body))
            crossed = (
                rebuilds > 1
                or 'rebuild_dirty' in body
                or 'set_state' in body
                or re.search(r'\bpump\(', body)
            )
            if not crossed:
                continue
            if 'flush_layout' in body:
                safe_rebuilders += 1
                continue
            rel = os.path.relpath(path, ROOT).replace('\\', '/')
            risky.append((rel, fn, rebuilds, 'rebuild_dirty' in body,
                          'set_state' in body))

print('%d functions lay out a rebuilt tree by descent, with no flush_layout'
      % len(risky))
print('%d cross a rebuild and do call flush_layout' % safe_rebuilders)
print()
by_file = {}
for rel, fn, rebuilds, dirty, state in risky:
    by_file.setdefault(rel, []).append((fn, rebuilds, dirty, state))
for rel in sorted(by_file):
    print('%s  (%d)' % (rel, len(by_file[rel])))
    for fn, rebuilds, dirty, state in sorted(by_file[rel]):
        marks = []
        if rebuilds > 1:
            marks.append('%d rebuilds' % rebuilds)
        if dirty:
            marks.append('rebuild_dirty')
        if state:
            marks.append('set_state')
        print('    %-60s %s' % (fn, ', '.join(marks)))
