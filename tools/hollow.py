"""Find predicates no input can make answer otherwise.

Three of these have turned up incidentally now -- labels_beside_rather_than_inside,
has_standard_key, is_superseded -- and one of the three was also wrong, not
merely hollow. The shape is searchable: a function whose parameter list has
nothing that could vary, returning a bare literal.

Two kinds, and they are not the same fault:

  * no arguments at all, or only `&self` on a type with no fields the body
    reads -- the body cannot depend on anything;
  * arguments present but the body is still a bare literal, which is worse,
    because the signature promises a question.

Reports both, with the returned literal, so each can be judged. A hollow
predicate is not automatically wrong -- `is_superseded` was true -- but it
states a fact while looking like a check, and the doc comment is where facts
belong.
"""
import os
import re

import paths

PORT = paths.SRC

# `pub fn name(args) -> bool {` then a lone `true`/`false` then `}`.
# Whether a doc comment sits directly above the function. That is the signal
# that matters: a constant answer with a paragraph saying why -- `is_enabled`
# explaining that a null callback does not disable an action button,
# `is_mini` noting upstream's mixin is literally `isMini() => true` -- has been
# examined. One with nothing above it has not.
DOCUMENTED = re.compile(r'///[^' + chr(10) + ']*' + chr(10) + r'[ 	]*(?:pub (?:const )?fn )')

ANY_BOOL_FN = re.compile(r'^[ 	]*pub (?:const )?fn (\w+)\([^)]*\)[ 	]*->[ 	]*bool[ 	]*\{', re.M)

HOLLOW = re.compile(
    r'^[ \t]*pub (?:const )?fn (\w+)\(([^)]*)\)[ \t]*->[ \t]*bool[ \t]*\{\s*\n'
    r'[ \t]*(true|false)\s*\n'
    r'[ \t]*\}',
    re.M)


def strip_tests(text):
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


no_args, with_args = [], []
# Every bool-returning function in the port, by name, so a constant leaf can be
# told from a constant that stands alone.
declared = {}
for root, _dirs, files in os.walk(PORT):
    for name in files:
        if not name.endswith('.rs'):
            continue
        path = os.path.join(root, name)
        where = os.path.relpath(path, PORT).replace(os.sep, '/')
        text = strip_tests(open(path, encoding='utf-8', errors='replace').read())
        for match in ANY_BOOL_FN.finditer(text):
            declared.setdefault((where, match.group(1)), []).append(match.start())
        for match in HOLLOW.finditer(text):
            fn, args, value = match.groups()
            line = text.count('\n', 0, match.start()) + 1
            varying = [a for a in args.split(',')
                       if a.strip() and not a.strip().startswith(('&self', 'self', '&mut self'))]
            head = text[max(0, match.start() - 400):match.start() + 40]
            documented = bool(DOCUMENTED.search(head))
            row = (where, line, fn, value, args.strip(), documented)
            (with_args if varying else no_args).append(row)

# A constant leaf of a dispatch that varies is not hollow. `is_rounded` is
# `true` on the rounded track shapes and `false` on the rectangular one, and the
# enum above them matches on the shape -- so the question is real and the answer
# is per-type, which is what a type-level fact looks like in a language with no
# inheritance to hang it on. Three of these were reported as unexplained
# constants until the screen could see the other arm.
#
# The test is deliberately weak: the same function name implemented with both
# literals somewhere in the port. It cannot tell a dispatch from a coincidence
# of naming, so it says "answers both ways" rather than "fine".
# Keyed by (file, name), because a name on its own is not enough: `is_empty` is
# implemented on a dozen unrelated types, and grouping all of them together
# buried a real one. A dispatching enum lives beside the leaves it dispatches
# to, so the file is the right scope.
answers, constant = {}, {}
for row in no_args + with_args:
    answers.setdefault((row[0], row[2]), set()).add(row[3])
    constant[row[0], row[2]] = constant.get((row[0], row[2]), 0) + 1
varies = {key for key, values in answers.items() if len(values) > 1}
# ...or the name also has an implementation that is not a bare literal. That is
# usually the enum above the leaves: `is_rounded` is `true` on both rounded
# track shapes and there is no third impl answering false, because the false is
# a match arm in the dispatching `is_rounded` on the enum.
varies |= {key for key in answers
           if len(declared.get(key, [])) > constant.get(key, 0)}

dispatched = [r for r in no_args + with_args if (r[0], r[2]) in varies]
with_args = [r for r in with_args if (r[0], r[2]) not in varies]
rows = [r for r in no_args + with_args if (r[0], r[2]) not in varies]
undocumented = [r for r in rows if not r[5]]
print('%d predicates return a bare literal; %d of them carry no doc comment'
      % (len(rows), len(undocumented)))
print()
print('-- constant and unexplained (the ones worth reading):')
for where, line, fn, value, args, _doc in undocumented:
    print('  %-44s %s:%d -> %s' % (fn, where, line, value))
print()
print('-- one arm each of a predicate that answers both ways (%d, not a queue):'
      % len(dispatched))
for key in sorted(varies):
    arms = ['%s:%d -> %s' % (r[0], r[1], r[3]) for r in dispatched
            if (r[0], r[2]) == key]
    if arms:
        print('  %-28s %s' % (key[1], ', '.join(arms)))
print()
print('-- takes arguments and ignores them (%d):' % len(with_args))
for where, line, fn, value, args, doc in with_args:
    print('  %-28s(%-24s) %s:%d -> %s%s'
          % (fn, args[:24], where, line, value, '' if doc else '   [undocumented]'))
