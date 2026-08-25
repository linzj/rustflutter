"""A screen: which fields of a theme's `lerp` does no test watch blend?

`tools/swap_lerps.py` asked a narrow question -- can the two ends of a lerp be
swapped unnoticed -- and found 134 sites, all of which now turn something red.
But its regex only matched bare `lerp(a, b, t)`. The far larger family in this
codebase is a theme's field-by-field walk:

    ThemeData {
        canvas_color: lerp_color(a.canvas_color, b.canvas_color, t),
        card_color: lerp_color(a.card_color, b.card_color, t),
        ...forty more lines of the same shape...
    }

387 such sites were never screened at all. And the characteristic defect of
this shape is not a swapped end -- it is a **copy-pasted line that still names
the field above it**. `card_color: lerp_color(a.canvas_color, ...)`. Upstream
Dart has had exactly that bug, in exactly this kind of method.

The mutation
------------

Swapping the ends is too weak here: it only fails if a test happens to sample
off the midpoint. This asks a blunter question instead. Each site's whole
right-hand side is replaced by its **first end**:

    card_color: lerp_color(a.card_color, b.card_color, t)   ->   a.card_color

so the field no longer blends at all. If the suite is still green, *nothing
anywhere reads this field through a lerp*. Not its direction, not its value,
not that it moves. A line that named the wrong field would be equally
invisible.

A green site is a candidate, not a defect
-----------------------------------------

Some of these are genuinely not worth a test on their own -- a theme with
sixty colour fields does not need sixty tests naming sixty colours. The point
is to know which ones are unwatched and to choose deliberately, rather than to
discover later that the one field that mattered was in the unwatched set.

It found three on its first run. `slider_theme.rs` had `padding`, `thumb_size`
and `value_indicator_text_style` stepping at the midpoint where upstream
blends them -- and the reason was that `EdgeInsetsGeometry.lerp`,
`Size.lerp`'s null arms and `TextStyle.lerp` did not exist in this port at
all. Sixteen of that file's colour lines were also unwatched, so a line naming
the field above it would have gone unnoticed; one assertion comparing two
sixteen-colour themes now covers them all.

A note on `t`
-------------

Freezing a site to its first end is invisible below the midpoint for any field
that steps, because the first end *is* the answer there. Eight of
`slider_theme.rs`'s sites stayed green until a test sampled past 0.5. If this
screen reports a stepping field, the missing test is one at `t >= 0.5`.

Some sites will not compile after the mutation (the lerp's return type differs
from the field's). Those are reported and not counted either way. A site that
took its first end by reference is retried as `a.x.clone()` before being
counted that way: on `theme.rs` the un-cloned spelling failed to build at 27
of 29 sites, so without the retry the screen read that file almost blind.

Usage: python tools/unlerped_fields.py <path-to-a-.rs-file>
"""
import io
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from rust_source import production  # noqa: E402

CRATE = r'K:\rustflutter\src\flutter\rust\rustflutter'
MSVC = (r'C:\Program Files\Microsoft Visual Studio\2022\Community'
        r'\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64')

SIDECAR = '.screen_orig'

IDENT = re.compile(r'[A-Za-z_][A-Za-z0-9_]*$')
FIELD = re.compile(r'(?P<indent>[ \t]*)(?P<field>[a-z_][a-z0-9_]*): $')


def sites(text):
    """Every `field: <something>lerp<something>(<first>, <second>, t),`.

    Scanned rather than matched: rustfmt breaks the long ones across four
    lines, and a line-anchored regex finds none of those. `component_themes.rs`
    reported zero sites that way while holding 284 of them.

    Each site is returned as (start, end, indent, field, first) where the span
    covers the field name through the comma after the call.
    """
    found = []
    cursor = 0
    while True:
        at = text.find(', t)', cursor)
        if at < 0:
            return found
        cursor = at + 4
        if not text.startswith(',', at + 4):
            continue

        # Walk back from the `)` to the `(` that opens this call.
        depth = 1
        i = at
        while i > 0 and depth:
            i -= 1
            if text[i] == ')':
                depth += 1
            elif text[i] == '(':
                depth -= 1
        if depth:
            continue
        open_paren = i

        callee = IDENT.search(text[:open_paren])
        if not callee or 'lerp' not in callee.group(0).lower():
            continue

        # The callee may be `Type::lerp`; take the whole path back.
        head = callee.start()
        while head >= 2 and text[head - 2:head] == '::':
            prior = IDENT.search(text[:head - 2])
            if not prior:
                break
            head = prior.start()

        label = FIELD.search(text[:head])
        if not label:
            continue

        # The call's first argument, up to its first top-level comma.
        depth = 0
        j = open_paren + 1
        while j < at:
            if text[j] in '([{':
                depth += 1
            elif text[j] in ')]}':
                depth -= 1
            elif text[j] == ',' and depth == 0:
                break
            j += 1
        first = text[open_paren + 1:j].strip().lstrip('&').strip()
        if not first or not re.fullmatch(r'[A-Za-z_][A-Za-z0-9_.]*', first):
            continue
        if first == label.group('field'):
            continue        # `x: lerp(x, ...)` -- freezing it changes nothing

        found.append((label.start('field'), at + 5,
                      label.group('indent'), label.group('field'), first))


def recover(path):
    """Restore a file a killed run left mutated.

    The `finally` below cannot run if the process is killed. Tick 219 lost a
    `swap_lerps.py` run to a timeout that way, and the next run read the
    mutated file as its baseline and reported the repair as a finding.
    """
    if os.path.exists(path + SIDECAR):
        io.open(path, 'w', encoding='utf-8', newline='').write(
            io.open(path + SIDECAR, encoding='utf-8', newline='').read())
        os.remove(path + SIDECAR)
        print('  (recovered %s from a killed run)' % os.path.basename(path))


def run(env):
    result = subprocess.run(['cargo', 'test', '--lib', '-q'],
                            cwd=CRATE, env=env, capture_output=True, text=True)
    out = result.stdout + result.stderr
    if 'error[' in out or 'error: could not compile' in out:
        return 'no-build'
    return 'green' if result.returncode == 0 else 'red'


def main(argv):
    path = argv[0]
    recover(path)
    original = io.open(path, encoding='utf-8', newline='').read()
    newline = '\r\n' if '\r\n' in original else '\n'
    io.open(path + SIDECAR, 'w', encoding='utf-8', newline='').write(original)
    text = original.replace('\r\n', '\n')

    outside = production(text)
    found = [s for s in sites(text) if outside(s[0])]
    print('%s: %d blended fields' % (os.path.basename(path), len(found)))

    env = dict(os.environ)
    env['PATH'] = MSVC + os.pathsep + env.get('PATH', '')
    unwatched, unbuilt = [], []
    try:
        for begin, finish, _indent, field, first in found:
            # `x: a.x` does not typecheck when the blend took `&a.x` and the
            # field is not `Copy`, which on `theme.rs` was 27 of 29 sites --
            # the screen was all but blind there. Try the borrowed spelling
            # too before giving up on a site.
            for spelling in ('%s: %s,' % (field, first),
                             '%s: %s.clone(),' % (field, first)):
                mutated = text[:begin] + spelling + text[finish:]
                io.open(path, 'w', encoding='utf-8', newline='').write(
                    mutated.replace('\n', newline))
                verdict = run(env)
                if verdict != 'no-build':
                    break
            line = text.count('\n', 0, begin) + 1
            if verdict == 'green':
                unwatched.append((line, field))
                print('  line %-6d GREEN UNBLENDED  %s' % (line, field))
            elif verdict == 'no-build':
                unbuilt.append(line)
    finally:
        io.open(path, 'w', encoding='utf-8', newline='').write(original)
        os.remove(path + SIDECAR)

    print('  %d of %d blend unwatched (%d would not build)'
          % (len(unwatched), len(found), len(unbuilt)))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
