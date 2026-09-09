# -*- coding: utf-8 -*-
"""Which of the backend's mutable state a refusal would leave behind.

`RustBackend._member` runs one member's emission and, when it refuses, rolls
back both the text and the state -- because a refusal that leaves state behind
is charged to the *next* member. Its comment says "every scrap of state a
member's emission sets"; it named nine fields, and by 2026-09-09 thirteen more
had been added elsewhere without being added to it. That is how the list was
always going to fail: it is a list.

So it is checked instead. A field belongs in the guard when a member's
emission saves it, changes it, and puts it back around a call that can raise
`Unsupported` -- because that local restore is exactly what a refusal skips.

    python3 bin/statecheck.py          # report, exit 1 if the guard is short

The front end is checked by the opposite rule, because it solved the same
problem the other way: its per-member `catch` (`declarations.dart`) restores
nothing at all, so every one of its scopes is a `try/finally` -- 24 of them,
and this fails if a 25th is written without one.

`bin/check.sh` runs both. A field that is deliberately *not* per-member -- a
cache, the output buffer, the refusal set -- is never scoped this way and so
never shows up here.

What this does **not** check: a refusal caught *inside* one member still skips
a backend scope's own restore, and the guard does not run until the member
ends. Nothing does that today; if something starts to, the backend's scopes
need `try/finally` as well, not instead.
"""
import io
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)

# Calls that lower or emit something, and so can raise `Unsupported` from
# underneath. A save/restore that wraps one of these is a save/restore a
# refusal escapes.
LOWERS = re.compile(
    r'\b(expr|expression|stmt|statement|type|_body|_call|_closure|_let|'
    r'_constant|_construct|_emit\w+|_widened\w*|coerceInto|_raw\w+)\s*\(')
SAVE = re.compile(r'^\s*(?:final|var)\s+(\w+)\s*=\s*(_\w+);\s*$', re.M)


def _sources(name):
    root = os.path.join(TOOL, 'lib', name)
    return [os.path.join(TOOL, 'lib', name + '.dart')] + [
        os.path.join(root, n) for n in sorted(os.listdir(root)) if n.endswith('.dart')]


def scoped_fields(package='backend_rust', in_finally=False):
    """Fields saved, changed and restored around something that can refuse.

    `in_finally=False` selects the scopes that are *not* wrapped in a
    `try/finally` -- the back end's, which lean on `_member`'s rollback, and
    the front end's, which must not exist.
    """
    out = {}
    for path in _sources(package):
        lines = io.open(path, encoding='utf-8').read().split('\n')
        for i, line in enumerate(lines):
            m = SAVE.match(line)
            if not m:
                continue
            local, field = m.groups()
            window = lines[i + 1:i + 400]
            back = re.compile(r'^\s*%s = %s;\s*$' % (re.escape(field), re.escape(local)))
            end = next((k for k, l in enumerate(window) if back.match(l)), None)
            if end is None:
                continue
            span = '\n'.join(window[:end])
            if not LOWERS.search(span):
                continue
            if ('finally' in span) == in_finally:
                out.setdefault(field, (os.path.relpath(path, TOOL), i + 1))
    return out


def guarded_fields():
    text = io.open(os.path.join(TOOL, 'lib', 'backend_rust.dart'),
                   encoding='utf-8').read()
    body = text[text.index('  bool _member('):]
    body = body[:body.index('\n  void _doc(')]
    # Both halves count. Most state is put back in the `finally`, so the
    # success path leaves nothing either; `_indent` is put back only in the
    # `catch`, on purpose -- it belongs with the text rollback, and restoring
    # it on the way out of a *successful* member would hide an emitter that
    # had not closed its own braces.
    tail = body[body.index('} on Unsupported catch'):]
    return set(re.findall(r'^\s*(_\w+) = \w+;\s*$', tail, re.M))


def main():
    bad = 0
    loose = scoped_fields('frontend_kernel', in_finally=False)
    print('front end: %d scope(s) around a call that can refuse and no '
          '`finally`' % len(loose))
    if loose:
        bad = 1
        print()
        print("The front end's per-member `catch` restores nothing, so a scope")
        print('without a `try/finally` leaks into the next member:')
        for f in sorted(loose):
            path, line = loose[f]
            print('  %-24s %s:%d' % (f, path, line))
        print()

    scoped, guarded = scoped_fields(), guarded_fields()
    missing = sorted(set(scoped) - guarded)
    print('back end:  %d field(s) scoped around a call that can refuse; '
          '_member restores %d' % (len(scoped), len(guarded)))
    if not missing:
        return bad
    print()
    print('MISSING from `_member`\'s finally -- a refusal leaks these into the')
    print('next member, which is then charged for them:')
    for f in missing:
        path, line = scoped[f]
        print('  %-24s first scoped at %s:%d' % (f, path, line))
    print()
    print('Add them to the save and the restore in `_member`, then measure:')
    print('changing what a refusal leaves behind changes what is emitted.')
    return 1


if __name__ == '__main__':
    sys.exit(main())
