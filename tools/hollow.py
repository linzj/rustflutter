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

PORT = r'K:\rustflutter\src\flutter\rust\rustflutter\src'

# `pub fn name(args) -> bool {` then a lone `true`/`false` then `}`.
# Whether a doc comment sits directly above the function. That is the signal
# that matters: a constant answer with a paragraph saying why -- `is_enabled`
# explaining that a null callback does not disable an action button,
# `is_mini` noting upstream's mixin is literally `isMini() => true` -- has been
# examined. One with nothing above it has not.
DOCUMENTED = re.compile(r'///[^' + chr(10) + ']*' + chr(10) + r'[ 	]*(?:pub (?:const )?fn )')

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
for root, _dirs, files in os.walk(PORT):
    for name in files:
        if not name.endswith('.rs'):
            continue
        path = os.path.join(root, name)
        text = strip_tests(open(path, encoding='utf-8', errors='replace').read())
        for match in HOLLOW.finditer(text):
            fn, args, value = match.groups()
            line = text.count('\n', 0, match.start()) + 1
            where = os.path.relpath(path, PORT).replace(os.sep, '/')
            varying = [a for a in args.split(',')
                       if a.strip() and not a.strip().startswith(('&self', 'self', '&mut self'))]
            head = text[max(0, match.start() - 400):match.start() + 40]
            documented = bool(DOCUMENTED.search(head))
            row = (where, line, fn, value, args.strip(), documented)
            (with_args if varying else no_args).append(row)

rows = no_args + with_args
undocumented = [r for r in rows if not r[5]]
print('%d predicates return a bare literal; %d of them carry no doc comment'
      % (len(rows), len(undocumented)))
print()
print('-- constant and unexplained (the ones worth reading):')
for where, line, fn, value, args, _doc in undocumented:
    print('  %-44s %s:%d -> %s' % (fn, where, line, value))
print()
print('-- takes arguments and ignores them (%d):' % len(with_args))
for where, line, fn, value, args, doc in with_args:
    print('  %-28s(%-24s) %s:%d -> %s%s'
          % (fn, args[:24], where, line, value, '' if doc else '   [undocumented]'))
