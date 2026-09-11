# -*- coding: utf-8 -*-
"""Every way this workspace can abort, counted -- and the total reconciled.

The fifth ruler. `run_chain.sh` says how much of the gallery translates and
compiles, `fx.sh`/`allfx.sh` whether the two ends compute the same answer,
`render_ruler.py` whether the tree comes out, `size_ruler.py` what it costs.
This one says how much of the program can still *stop*, which is the rule
"a panic is never a pass" written as a number.

Why it exists, in one line: the count it replaces was a `grep -c
'panic!("uncaught Dart exception'`, and

    panic!(
        "uncaught Dart exception: TypeError: ..",
        value.runtime_type().name
    )

is not on one line, so that grep said zero while six call sites still
reached it (work.md section 六, 2026-09-11). The lesson is not "write a
better pattern": it is that a count of *one* string can only ever go wrong
silently. So this counts the **total** first and then splits it, and prints
`RECONCILED` only when the parts add up to the total.

    python3 bin/panic_ruler.py                  # .crate-ws
    python3 bin/panic_ruler.py --root .crate-ws --json

What is legitimately still an abort -- a fact about *this translator*
rather than about the program being translated:

  * `dart2rust: stubbed ..`        a member this compiler did not translate
  * `dart2rust: not translated ..` a refusal, by name
  * `dart2rust: a dynamic slot ..` the census did not predict the type
  * `unreachable!(..)`             TFA said the code is dead
  * `todo!(..)`                    an untranslated class
  * `native ..`                    the host answered nothing

That list is the whole of it, and each entry is matched in full. A
`dart2rust:` prefix is not itself a licence: `panic!("dart2rust: a {}
where a `{}` was wanted")` is a Dart `TypeError` with this compiler's name
written on it, and Dart programs catch those. Folding every other
`dart2rust:` message into one legitimate bucket is how 516 aborts read as
zero on the day the rule was declared finished (2026-09-11); each message
now stands under its own name, and a new one is illegitimate until someone
argues it onto the list.

Everything else in this report is a Dart-visible throw that the translated
program cannot take: it belongs in a `Result`.
"""
import argparse
import io
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)

# A macro that aborts, and the text it aborts with -- which may be on the
# next line, or the one after that.
ABORTS = ('panic!', 'unreachable!', 'todo!', 'unimplemented!', 'assert!',
          'assert_eq!', 'assert_ne!')
# ..and the methods that abort with a message of their own.
EXPECTS = ('.expect(', '.expect_err(')
# `\(\)` on purpose: `\.unwrap()` as an ERE has an empty group in it and
# swallows all 3,387 `.unwrap_or*`, which are total functions and no panic
# at all. That miscount is the twin of the one this file exists for.
UNWRAP = re.compile(r'\.unwrap\(\)')
UNWRAP_OR = re.compile(r'\.unwrap_or[a-z_]*\(')


def _first_string(text, at, stop=None):
    """The first Rust string literal at or after `at`, or None.

    Reads across newlines on purpose -- that is the whole point, see the
    module docstring -- but not past `stop`, which callers set to the end
    of the call: `panic!(message)` with no literal in it must read as one
    with no literal, not borrow the next function's.
    """
    i = text.find('"', at)
    if i < 0 or (stop is not None and i >= stop):
        return None
    out = []
    i += 1
    while i < len(text):
        c = text[i]
        if c == '\\':
            i += 2
            out.append(' ')
            continue
        if c == '"':
            return ''.join(out)
        out.append(c)
        i += 1
    return None


def _kind(macro, message):
    """The bucket a single abort falls in."""
    if macro != 'panic!':
        return macro
    if message is None:
        return 'panic! (no literal message)'
    return 'panic!("%s")' % _bucket(message)


def _bucket(message):
    """The message, folded to the family it belongs to."""
    for prefix in ('dart2rust: stubbed', 'dart2rust: not translated',
                   'dart2rust: a dynamic slot'):
        if message.startswith(prefix):
            return prefix + ' ..'
    if message.startswith('native '):
        return 'native ..'
    if message.startswith('uncaught Dart exception'):
        return 'uncaught Dart exception ..'
    return message[:60]


def _closes(text, at):
    """The index just past the `(..)` that starts at `at`, or len(text).

    Strings and their escapes are skipped so that a `")"` inside a message
    does not close the call early.
    """
    depth = 0
    i = at
    while i < len(text):
        c = text[i]
        if c == '"':
            i += 1
            while i < len(text) and text[i] != '"':
                i += 2 if text[i] == '\\' else 1
        elif c == '(':
            depth += 1
        elif c == ')':
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return len(text)


# A fact about *this translator* rather than about the program, whichever
# macro or method says it.
LEGITIMATE = (
    'dart2rust: stubbed ..',
    'dart2rust: not translated ..',
    'dart2rust: a dynamic slot ..',
    'native ..',
)


#: Sites argued onto the list **one at a time, by their whole message** --
#: the only way onto it now that the `dart2rust:` prefix is not a licence.
#: Each of these is a single site stating a fact about this translator's own
#: machinery, and the argument for each is written next to it. A message
#: that covers many sites does not belong here: the 494 lazy-cell reads were
#: one emitter, and the answer to those was to stop emitting an abort at all
#: (`protocols.dart`, `_lazyRead`), not to name them.
NAMED = (
    # `(f ??= <>{}).add(x)`: the place was filled two tokens earlier. It
    # cannot be restructured the way the lazy cell was -- what is wanted is
    # a `&mut T`, and the `None` arm of a match on `as_mut()` cannot
    # re-borrow the place to fill it (NLL problem case 3). The shape with
    # no abort in it is `get_or_insert_with`, and that needs the value,
    # while `IrIfNull.assignsLeft`'s right side is the store.
    'dart2rust: an Option this function just tested with is_none',
    # The object protocol sets a handle's `Weak` at construction and reads
    # it afterwards. A read before the set is this compiler emitting the
    # two in the wrong order, which no program can ask for.
    'dart2rust: `this` taken before the object had a handle',
    # A `Future` polled after it completed is a Rust contract violation,
    # not a Dart one: `Poll::Ready` twice. The alternative spelling is
    # `Poll::Pending`, which hangs -- a wrong answer in place of a loud one.
    'dart2rust: a FutureOr polled after it was taken',
    # `Map`'s hash index, read on the line after it is assigned.
    'dart2rust: the index was written just above',
    # The prelude's own scheduler invariant. Nothing a program does reaches
    # it: `Completer.complete` on a settled future returns `Err` before it
    # would call this (`dart_state_error("Future already completed")`).
    'dart2rust: a Future resolved twice',
)


def _legitimate(kind):
    if kind in ('unreachable!', 'todo!'):
        return True
    tail = kind[kind.find('('):]
    # `_bucket` so that a NAMED entry is written in full here and still
    # matches the folded, 60-character form the report counts under.
    return any('("%s")' % _bucket(one) == tail for one in LEGITIMATE + NAMED)


def _uncommented(text):
    """The same text with every comment blanked out, offsets preserved.

    A ruler that reads comments counts the prose about a panic as a panic:
    this file's own doc comment quotes `panic!("uncaught` and was counted
    twice over before this existed. Blanks rather than deletions so that
    every index computed on the result still points into the original.
    """
    out = list(text)
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if c == '"':
            i += 1
            while i < n and text[i] != '"':
                i += 2 if text[i] == '\\' else 1
            i += 1
        elif c == "'" and i + 2 < n and (text[i + 1] != '\\' and text[i + 2] == "'"):
            i += 3                      # a char literal, not a lifetime
        elif text.startswith('//', i):
            while i < n and text[i] != '\n':
                out[i] = ' '
                i += 1
        elif text.startswith('/*', i):
            depth = 0
            while i < n:
                if text.startswith('/*', i):
                    depth += 1
                    out[i] = out[i + 1] = ' '
                    i += 2
                    continue
                if text.startswith('*/', i):
                    depth -= 1
                    out[i] = out[i + 1] = ' '
                    i += 2
                    if depth == 0:
                        break
                    continue
                if text[i] != '\n':
                    out[i] = ' '
                i += 1
        else:
            i += 1
    return ''.join(out)


def scan(path):
    """One file: {kind: count}, plus the totals this reconciles against."""
    text = _uncommented(io.open(path, encoding='utf-8', errors='replace').read())
    kinds = {}
    total = 0
    for macro in ABORTS:
        at = 0
        while True:
            i = text.find(macro + '(', at)
            if i < 0:
                break
            at = i + len(macro)
            total += 1
            end = _closes(text, i + len(macro))
            k = _kind(macro, _first_string(text, i + len(macro), end))
            kinds[k] = kinds.get(k, 0) + 1
    expects = 0
    for method in EXPECTS:
        at = 0
        while True:
            i = text.find(method, at)
            if i < 0:
                break
            at = i + len(method)
            # `x.expect(msg)?` is not `Option::expect`: that one hands back
            # a `T`, and `?` on a `T` is not Rust. It is some other method
            # of the same name returning a `Result` -- the prelude's JSON
            # reader has one (`self.expect(":")?`) -- and it aborts nothing.
            after = _closes(text, i + len(method) - 1)
            rest = text[after:after + 2].lstrip()
            if rest.startswith('?'):
                continue
            expects += 1
            k = '%s("%s")' % (method.rstrip('('),
                              _bucket(_first_string(
                                  text, i + len(method), after) or ''))
            kinds[k] = kinds.get(k, 0) + 1
    total += expects
    unwraps = len(UNWRAP.findall(text))
    if unwraps:
        kinds['.unwrap()'] = kinds.get('.unwrap()', 0) + unwraps
    total += unwraps
    return kinds, total, len(UNWRAP_OR.findall(text))


def report(root):
    """Two halves -- the prelude and everything this compiler wrote."""
    halves = {'generated': {}, 'prelude': {}}
    totals = {'generated': 0, 'prelude': 0}
    total_or = {'generated': 0, 'prelude': 0}
    for base, dirs, names in os.walk(root):
        dirs.sort()
        for name in sorted(names):
            if not name.endswith('.rs'):
                continue
            path = os.path.join(base, name)
            half = 'prelude' if 'dart_prelude' in path else 'generated'
            kinds, total, ors = scan(path)
            for k, n in kinds.items():
                halves[half][k] = halves[half].get(k, 0) + n
            totals[half] += total
            total_or[half] += ors
    return halves, totals, total_or


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument('--root', default=os.path.join(TOOL, '.crate-ws'))
    ap.add_argument('--json', action='store_true')
    args = ap.parse_args(argv)
    halves, totals, total_or = report(args.root)
    if args.json:
        print(json.dumps({'kinds': halves, 'totals': totals,
                          'unwrap_or': total_or}, indent=2, sort_keys=True))
        return 0
    bad = 0
    for half in ('generated', 'prelude'):
        kinds = halves[half]
        print('== %s ==' % half)
        summed = 0
        for k in sorted(kinds, key=lambda k: (-kinds[k], k)):
            n = kinds[k]
            summed += n
            mark = ' ' if _legitimate(k) else '*'
            if mark == '*':
                bad += n
            print('%s %7d  %s' % (mark, n, k))
        print('  %7d  TOTAL' % totals[half])
        print('  %7d  .unwrap_or* (total functions, not a panic)'
              % total_or[half])
        if summed != totals[half]:
            print('  MISCOUNTED: the parts sum to %d, the total is %d'
                  % (summed, totals[half]))
            return 2
        print('  RECONCILED')
    print('* = a Dart-visible throw this program cannot take: %d' % bad)
    return 0


if __name__ == '__main__':
    sys.exit(main())
