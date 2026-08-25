"""The tenth ruler: does a protocol string this port declares still exist upstream?

`constants.py` compares numbers the port attributes to upstream. This does the
same for the other kind of borrowed value: the strings that are **protocol**
rather than ours to choose.

    pub const TEXT_INPUT: &str = "flutter/textinput";
    pub const ONE_TIME_CODE: &str = "oneTimeCode";
    HapticFeedbackType::Heavy => "HapticFeedbackType.heavyImpact"

Every one of these is matched by name at the far end -- an embedder, an
operating system's autofill service, the engine's channel table. A wrong one
**fails silently**: nothing errors, the channel simply has nobody on it and
the autofill offer simply never appears. That is what makes them worth a
ruler; a number that is wrong usually looks wrong on the screen.

What it checks
--------------
That the literal appears somewhere in upstream's Dart. Upstream renaming a
channel or retiring an autofill hint is what this catches.

What it does NOT check
----------------------
**That the string is the right one for the constant that holds it.** A typo
which happens to spell another upstream string passes here. Pairing each
constant with its upstream declaration would mean a ledger with one row per
name, and a ledger nobody maintains is worse than a stated blind spot -- the
per-value checks in each table's own test are where that correspondence
lives, and they are hand-written on purpose.

It also only sees constants declared as `pub const NAME: &str = "..."`. Match
arms that produce strings -- `HapticFeedbackType::Heavy => "..."` -- are the
other common shape and are not read here, because the arm's value is only a
protocol string by convention and no pattern separates it from an ordinary
message. Those tables are walked by their own tests instead.

  python tools/wire_strings.py           # the report; exit 1 on a disagreement
"""
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PORT = os.path.join(REPO, 'src', 'flutter', 'rust', 'rustflutter', 'src')
UPSTREAM = [
    os.path.join('K:', os.sep, 'flutter', 'packages', 'flutter', 'lib', 'src'),
    os.path.join('K:', os.sep, 'flutter', 'engine', 'src', 'flutter', 'lib', 'ui'),
]

DECL = re.compile(
    r'pub const ([A-Z][A-Z0-9_]*)\s*:\s*&(?:\'static\s+)?str\s*=\s*"([^"]+)"\s*;')

# A value that looks like protocol rather than like prose: a channel path, or
# a lowerCamel identifier, optionally dotted (`TextAffinity.downstream` is
# capitalised on the left of the dot and is caught by the second branch).
WIRE = re.compile(
    r'^(?:flutter/[a-z_]+'
    r'|[a-z][A-Za-z0-9]*(?:\.[A-Za-z0-9]+)*'
    r'|[A-Z][A-Za-z0-9]*\.[a-z][A-Za-z0-9]*)$')


def upstream_text():
    chunks = []
    for root_dir in UPSTREAM:
        for root, _, files in os.walk(root_dir):
            for name in files:
                if name.endswith('.dart'):
                    chunks.append(open(os.path.join(root, name),
                                       encoding='utf-8', errors='ignore').read())
    return '\n'.join(chunks)


def main():
    if not all(os.path.isdir(path) for path in UPSTREAM):
        print('upstream not found; nothing to compare against')
        return 0
    haystack = upstream_text()

    total = 0
    skipped = 0
    missing = []
    for root, _, files in os.walk(PORT):
        for name in sorted(files):
            if not name.endswith('.rs'):
                continue
            path = os.path.join(root, name)
            text = open(path, encoding='utf-8', errors='ignore').read()
            for const, value in DECL.findall(text):
                if not WIRE.match(value):
                    skipped += 1
                    continue
                total += 1
                if ("'%s'" % value) in haystack or ('"%s"' % value) in haystack:
                    continue
                rel = os.path.relpath(path, PORT).replace(os.sep, '/')
                missing.append((rel, const, value))

    print('%d protocol strings cited, %d no longer upstream '
          '(%d string constants read as prose, not compared)'
          % (total, len(missing), skipped))
    for rel, const, value in missing:
        print('  %-34s %-32s %s' % (rel, const, value))
    if missing:
        print()
        print('A protocol string upstream no longer has is a message nobody')
        print('answers: the channel has no listener, the autofill offer never')
        print('appears, and nothing anywhere reports an error.')
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
