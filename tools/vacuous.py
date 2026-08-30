"""Find tests whose every claim is that nothing happened.

Three of these have turned up in the last four ticks, and each was found the
same way -- by pointing a test at the wrong mechanism and watching it pass:

  * `an_opacity_group_is_closed_too` asserted a save depth of zero. A paint
    that opens no layer at all satisfies that, and `RenderOpacity` opens
    none, so the test passed on a code path it was not about.
  * `the_highlight_is_the_first_mark_of_the_frame` asserted the highlight was
    at index 0, which held whether or not any glyphs followed it.
  * `an_empty_field_showing_a_hint_still_draws_its_caret` counted one
    rectangle and never set the hint it was named for.

The shape is searchable. A test whose assertions are *all* of the form

    assert!(x.is_empty())          assert!(!x.iter().any(..))
    assert_eq!(x.len(), 0)         assert_eq!(x, None)
    assert!(x.is_none())

is satisfied by a run in which the code under test did nothing whatsoever --
including one where the test built the wrong thing, or the feature was never
reached. Such a test is not automatically wrong: "an empty decoration draws
nothing" is exactly this shape and is worth having. What it needs is a
companion claim in the same test that something *did* happen, so the two
together say "this path ran, and produced nothing".

So this reports tests with no positive claim at all, for reading. The answer
is usually one line -- assert the thing that should be there beside the thing
that should not.

**The count moved from 10 to 31 at tick 301, and nothing got worse.** The
`POSITIVE` pattern used to treat `assert!`'s own trailing `!` as a negation --
`assert!(x.is_none())` read as a claim that something *was* there -- so the
simplest and commonest shape of all was excluded from the count. It reported
whichever tests happened to have a quote or a comma in the argument, because
those characters stopped the class from reaching the `is_none()`. The number
was an artifact of that.

**The count is not a target.** Some of these are right as they are: the
absence *is* the whole claim, and either a sibling test in the same module
makes the positive one or there is no positive one to make. Driving the count
to zero would mean padding them with assertions nobody needs, which is the
opposite of what the screen is for.

So a reading goes in `vacuous_examined.json` and this stops showing it -- the
same mechanism `depth.py` uses, and for the same reason. Eight rows were read
at tick 174 and the judgement lived in this paragraph, where the tool could
not consult it, so the count stayed put and every visit re-read the same
tests.

That file is a claim, not a suppression list. Each row says what makes the
absence sufficient, or where the positive claim lives instead -- and it
records the body that was read, so a test that is later rewritten comes back.

  python tools/vacuous.py            # the list, minus what has been read
  python tools/vacuous.py --all      # including those, and ones with 3+ claims
"""
import io
import json
import os
import re
import sys

import paths

PORT = paths.SRC

TEST = re.compile(r'^([ \t]*)#\[test\][ \t]*\n(?:[ \t]*#\[[^\]]*\][ \t]*\n)*'
                  r'[ \t]*(?:async )?fn (\w+)\(', re.M)

# Every assertion form the crate uses.
ASSERT = re.compile(r'\bassert(?:_eq|_ne)?!\s*\(', re.M)

# A claim that a collection or an option holds nothing.
#
# Narrow on purpose, and narrowed twice.
#
# The first draft also counted `assert_ne!`, `assert!(!x)` and
# `assert_eq!(x, 0)`, and reported 368 of 5253 tests -- which is not a queue
# but a mood. Those are positive claims: `assert_ne!` says two things differ,
# and a count of zero is usually the answer under test rather than evidence
# that nothing ran.
#
# The second draft still matched `, None` anywhere in an assertion, which
# catches `assert_eq!(x, None)` and also every `f(a, None, None)` in an
# argument list. It reported two tests that each carried a plain positive
# claim -- one of them an `.is_some()` in the very next line. A screen that
# names a test which is already doing the right thing costs the reader more
# than it saves, so the pattern is gone: a test spelled `assert_eq!(x, None)`
# rather than `assert!(x.is_none())` is missed, and that is the cheaper error.
NEGATIVE = re.compile(
    r'\.is_empty\(\)'
    r'|\.is_none\(\)'
    r'|\.len\(\)\s*,\s*0\b'
    r'|count\(\)\s*,\s*0\b'
    r'|!\s*[\w:.]+\s*\.\s*(?:iter\(\)\s*\.\s*)?any\('
)

# `assert!(!sheet.is_empty())` is a claim that something **is** there, and the
# third draft of this file reported a test for asking it. A negation in front
# of an emptiness check reverses its meaning, and a claim carrying one is
# evidence the path ran.
#
# `!x.any(..)` is not in here on purpose: that one really is an absence, and
# it is the form the recorder tests use.
# The class holds whitespace and commas because `rustfmt` wraps a long
# receiver across lines, and a claim does not stop being positive for being
# three lines tall.
#
# `(?<!assert)` is what makes that safe, and leaving it out is how this first
# went wrong: **`assert!` ends in a `!` too.** With whitespace allowed in the
# class, that `!` reached across the whole argument list to the `is_none()`
# inside it, and every `assert!(x.is_none())` in the crate read as a positive
# claim -- seven of the ten rows vanished at once and the ruler went blind
# while its count went down.
#
# Non-greedy so it ends at the first `is_empty`/`is_none` after the `!`.
POSITIVE = re.compile(
    r'(?<!assert)!\s*[\w:.()\[\]&*,\s]*?\.\s*is_(?:empty|none)\(\)', re.S)


def body_of(text, start):
    """The source of the function whose signature begins at `start`."""
    open_brace = text.index('{', start)
    depth = 0
    for index in range(open_brace, len(text)):
        if text[index] == '{':
            depth += 1
        elif text[index] == '}':
            depth -= 1
            if depth == 0:
                return text[open_brace:index + 1]
    return text[open_brace:]


def assertions(body):
    """Each assertion's source, from `assert` to its closing bracket."""
    found = []
    for match in ASSERT.finditer(body):
        depth = 0
        for index in range(match.end() - 1, len(body)):
            if body[index] == '(':
                depth += 1
            elif body[index] == ')':
                depth -= 1
                if depth == 0:
                    found.append(body[match.start():index + 1])
                    break
    return found


EXAMINED = os.path.join(paths.TOOLS, 'vacuous_examined.json')


def load_examined():
    """The `(file, test)` pairs that have been read, with the reason.

    Keyed by name, as `depth.py` keys its own by class -- and carrying the
    same caveat: a test that is rewritten keeps the verdict its old body
    earned until somebody reads it again. Recording a fingerprint would catch
    that, and would need this tool to write its own data file back; it is not
    worth the surprise, and renaming a test is enough to bring the row home.
    """
    if not os.path.exists(EXAMINED):
        return {}
    with io.open(EXAMINED, encoding='utf-8') as handle:
        rows = json.load(handle)['examined']
    return {(row['file'], row['test']) for row in rows}


def main():
    show_all = '--all' in sys.argv
    examined = load_examined()
    rows = []
    examined_seen = 0
    total = 0
    for dirpath, _, files in os.walk(PORT):
        for name in sorted(files):
            if not name.endswith('.rs'):
                continue
            path = os.path.join(dirpath, name)
            text = io.open(path, encoding='utf-8').read().replace('\r\n', '\n')
            relative = os.path.relpath(path, PORT).replace(os.sep, '/')
            for match in TEST.finditer(text):
                total += 1
                body = body_of(text, match.end())
                claims = assertions(body)
                if not claims:
                    continue
                negative = [
                    claim
                    for claim in claims
                    if NEGATIVE.search(claim) and not POSITIVE.search(claim)
                ]
                if len(negative) == len(claims):
                    if len(claims) < 3 or show_all:
                        name = match.group(2)
                        if (relative, name) in examined:
                            examined_seen += 1
                            if not show_all:
                                continue
                        rows.append((relative, name, len(claims)))

    print('%d tests; %d claim only that nothing happened (%d read and left)'
          % (total, len(rows), examined_seen))
    print('%-34s %-6s %s' % ('file', 'claims', 'test'))
    for relative, name, count in sorted(rows):
        print('%-34s %-6d %s' % (relative, count, name))


if __name__ == '__main__':
    main()
