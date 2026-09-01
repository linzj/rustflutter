"""The eighteenth ruler: which `update_from` guards compare a value with itself?

Tick 529 found this in `RenderProxySliver::update_from`:

    let mut effect = UpdateEffect::repaint_if(self.behavior != fresh.behavior);
    self.behavior = fresh.behavior;                       // <- assigned here
    effect = effect.and(UpdateEffect::relayout_if(
        ...  if self.behavior != fresh.behavior           // <- compared after
    ));

By the second comparison the two fields are the same value, so the guard is
reliably false and the relayout it protects never happens. A cross-axis limit
that moved was never laid out again, and the sliver stayed the width it used to
be.

**Nothing catches this.** It compiles; every test passes; the branch is simply
never taken. It is not a wrong answer that some assertion could see -- it is an
answer never computed. That is what a ruler is for, and it is why this one
exists after finding the bug by hand rather than before.

What this measures
------------------

Every `fn update_from` body in the crate. Inside one, for each assignment of
the shape

    self.<field> = fresh.<field>;

it looks for a **later** comparison in that same body between `self.<field>`
and any `fresh.<...>`. That is the whole signature: the assignment makes them
equal, and the comparison then asks whether they differ.

What it deliberately does not flag
----------------------------------

Reading `self.<field>` after assigning it is ordinary and usually right --
trimming a child list against the new count, building a staged object out of
the new values. Eleven such reads exist in this crate and every one of them is
correct. Flagging them would cost a false alarm every tick, which is worse than
not having the tool: a ruler nobody believes is a ruler nobody reads.

So only the comparison-against-`fresh` shape is reported. The narrower rule is
the one that was actually wrong, and it is the one that can be judged without
reading the surrounding intent.

The fix, where this fires
-------------------------

Read before writing:

    let was = self.field;
    let now = fresh.field;
    self.field = now;
    ... compare `was` with `now`

which is what `RenderProxySliver::update_from` does now.
"""

import io
import os
import re
import sys

ROOT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    'src', 'flutter', 'rust', 'rustflutter', 'src')

ASSIGN = re.compile(
    r'^[ \t]*self\.(\w+)\s*=\s*fresh\.\1\s*(?:\.clone\(\))?;', re.M)


def bodies(text):
    """Each `fn update_from` body, found by brace matching.

    A regex cannot bound a Rust block, and these bodies contain closures,
    `matches!` arms and nested blocks. Counting braces from the opening one is
    exact where a pattern would guess.
    """
    for match in re.finditer(r'fn update_from\b', text):
        start = text.index('{', match.end())
        depth = 0
        for index in range(start, len(text)):
            if text[index] == '{':
                depth += 1
            elif text[index] == '}':
                depth -= 1
                if depth == 0:
                    yield match.start(), text[start:index + 1]
                    break


def main():
    dead = []
    bodies_seen = 0
    files_seen = 0
    for name in sorted(os.listdir(ROOT)):
        if not name.endswith('.rs'):
            continue
        files_seen += 1
        text = io.open(os.path.join(ROOT, name), encoding='utf-8').read()
        for start, body in bodies(text):
            bodies_seen += 1
            first_line = text[:start].count('\n') + 1
            for assign in ASSIGN.finditer(body):
                field = assign.group(1)
                after = body[assign.end():]
                compare = re.compile(
                    r'self\.%s\s*(?:==|!=|<=|>=|<|>)\s*fresh\.\w+'
                    r'|fresh\.\w+\s*(?:==|!=|<=|>=|<|>)\s*self\.%s\b'
                    % (re.escape(field), re.escape(field)))
                hit = compare.search(after)
                if hit is None:
                    continue
                line = first_line + body[:assign.end()].count('\n')
                dead.append((name, line, field, hit.group(0).strip()))

    print(f'{bodies_seen} update_from bodies in {files_seen} files')
    print()
    if not dead:
        print('No guard compares a field with itself.')
        return 0

    print('These compare a field after making it equal, so the guard is dead:')
    for name, line, field, expr in dead:
        print(f'  {name}:{line}  self.{field} assigned, then `{expr}`')
    print()
    print(f'{len(dead)} dead guard(s). Read the old value into a local before')
    print('assigning, and compare that -- see RenderProxySliver::update_from.')
    return 1


if __name__ == '__main__':
    sys.exit(main())
