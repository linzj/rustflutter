"""A screen, not a ruler: public enum variants no test module ever names.

The five rulers each answer a question with a number that has to reach zero.
**This does not, and must not be treated as though it did.** Naming a variant
in a test is not the same as exercising it, and not naming one is not proof it
is unexercised -- a variant can be reached through a `Default`, through a
`match` over an `ALL` array, or by a caller two modules away.

So this prints a work-list to *investigate*, and every entry has to be
confirmed by mutation before it counts. Tick 91 confirmed two of two sampled:
collapsing `ConstraintsTransform::MaxWidthUnconstrained` into a no-op and
swapping `HapticFeedbackType::Heavy`'s payload for the light one both left the
whole suite green.

It is deliberately not an acceptance gate. 262 entries is a queue to work
through beside the MISSING list, not a number to drive to zero in a hurry --
and some of them will turn out to be reached after all, which is the answer a
screen is allowed to give.

Its own first run was wrong, in the way these always are: it compared each
file against that file's own test modules, so `TextAlign`'s variants came back
untested because they are exercised from `painting.rs` rather than from
`engine.rs`. The probe was at fault, not the port -- the same shape of mistake
as scanning `packages/` and missing `dart:ui`.

  python tools/unwalked.py [how many enums to list]
"""
import os
import re
import sys

PORT = r'K:\rustflutter\src\flutter\rust\rustflutter\src'

ENUM = re.compile(r'^pub enum ([A-Za-z0-9_]+)\s*\{', re.M)
VARIANT = re.compile(r'^\s{4}([A-Z][A-Za-z0-9_]*)\s*(?:,|\{|\()', re.M)


def strip_comments(text):
    text = re.sub(r'//[^\n]*', '', text)
    return text


def test_spans(text):
    """(start, end) of every #[cfg(test)] module."""
    spans = []
    for match in re.finditer(r'#\[cfg\(test\)\]\s*mod\s+\w+\s*\{', text):
        brace = text.index('{', match.end() - 1)
        depth = 0
        for index in range(brace, len(text)):
            if text[index] == '{':
                depth += 1
            elif text[index] == '}':
                depth -= 1
                if depth == 0:
                    spans.append((match.start(), index + 1))
                    break
    return spans


# Every test module in the crate, not just the ones in the same file. A first
# cut compared each file against its own tests only, and reported TextAlign's
# variants as untested because they are exercised from painting.rs's tests
# rather than engine.rs's. The probe was wrong, not the port -- the same shape
# of error as scanning packages/ and missing dart:ui.
ALL_TESTS = []
for root, _dirs, files in os.walk(PORT):
    for name in files:
        if not name.endswith('.rs'):
            continue
        raw = open(os.path.join(root, name), encoding='utf-8', errors='replace').read()
        for a, b in test_spans(raw):
            ALL_TESTS.append(raw[a:b])
ALL_TESTS = strip_comments('\n'.join(ALL_TESTS))

total_variants = 0
unnamed = []

for root, _dirs, files in os.walk(PORT):
    for name in files:
        if not name.endswith('.rs'):
            continue
        path = os.path.join(root, name)
        raw = open(path, encoding='utf-8', errors='replace').read()
        spans = test_spans(raw)
        tests = ''.join(raw[a:b] for a, b in spans)
        body = raw
        for a, b in reversed(spans):
            body = body[:a] + body[b:]
        body = strip_comments(body)
        tests_stripped = strip_comments(tests)

        for enum_match in ENUM.finditer(body):
            enum_name = enum_match.group(1)
            # The enum's own braces.
            brace = body.index('{', enum_match.end() - 1)
            depth = 0
            end = len(body)
            for index in range(brace, len(body)):
                if body[index] == '{':
                    depth += 1
                elif body[index] == '}':
                    depth -= 1
                    if depth == 0:
                        end = index
                        break
            variants = VARIANT.findall(body[brace:end])
            for variant in variants:
                total_variants += 1
                qualified = f'{enum_name}::{variant}'
                if qualified not in ALL_TESTS:
                    unnamed.append((
                        os.path.relpath(path, PORT).replace(os.sep, '/'),
                        enum_name,
                        variant,
                    ))

print('%d public enum variants, %d never named in a test module'
      % (total_variants, len(unnamed)))
by_enum = {}
for path, enum_name, variant in unnamed:
    by_enum.setdefault((path, enum_name), []).append(variant)
# The interesting ones: enums where *some* variants are tested and some are not.
partial = []
for (path, enum_name), missing in sorted(by_enum.items()):
    partial.append((len(missing), path, enum_name, missing))
partial.sort(reverse=True)
for count, path, enum_name, missing in partial[:int(sys.argv[1]) if len(sys.argv) > 1 else 25]:
    print('  %-34s %-40s %s' % (enum_name, path, ', '.join(missing[:6])))
