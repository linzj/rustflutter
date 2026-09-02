"""What is left to port in one upstream class, and a Rust skeleton for it.

Why this exists
---------------

Ticks 514-539 ported by hand at roughly one member cluster a round. The
inventory says why that will not finish: every one of upstream's 2107 public
classes is already *accounted for* -- `coverage.py` reports 0 MISSING -- so
nothing is waiting to be discovered. What is left is **depth inside classes**,
and `depth.py` only ever showed the twenty worst.

The slow part of a round was never the typing. It was answering, for one
member: what is its type, is it nullable, what is its default, and what does
upstream's own comment say about why. All four are in the source, and until
now every tool here read that source with regular expressions -- which can
count members and cannot answer any of those four.

So: `tools/dart/dump_ast.dart` reads upstream with the real analyzer, and this
joins that dump to the Rust side.

What it generates, and what it does not
---------------------------------------

It generates the part that is **mechanical**: the struct field, the `with_`
builder, the doc comment carried down from upstream, the signature of a method
that is missing. That is real work removed -- it is most of the keystrokes in a
round -- and none of it is a judgement call.

It does **not** generate behaviour, and it is built so it cannot pretend to.
Every generated body is `todo!()` carrying upstream's own words. The reason is
the record of this port: what took the rounds was deciding that a focus
overlay fades over 50ms and not 200 (tick 534), that `showCursor` defaults to
`!readOnly` rather than to a constant (tick 519), that resolving `{hovered}`
alone loses a chip's selection (tick 539). A transpiler emitting plausible
bodies would have produced wrong answers to all three, and wrong answers that
compile are worse than absent ones -- the `hollow` and `vacuous` rulers exist
because this project already learned that.

Usage
-----

    python tools/port.py --list                 # classes by how much is missing
    python tools/port.py SelectableText         # what is missing, with docs
    python tools/port.py SelectableText --emit  # ... and a Rust skeleton
"""

import argparse
import io
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import depth  # noqa: E402  -- the Rust-side reader, reused whole rather than
               # written a second time: one answer to "is this member ported".

AST = os.path.join(HERE, 'upstream_ast.json')

# The names of a Rust type's members: fields, methods and constants. Used to
# ask "does the crate answer for this member", which is a different question
# from `depth.RUST_MEMBER`'s "how many members are there".
RUST_NAME = re.compile(
    r'^\s{4}(?:pub(?:\([^)]*\))?\s+)?(?:fn|const)\s+(\w+)'
    r'|^\s{4}(?:pub(?:\([^)]*\))?\s+)?([a-z_]\w*)\s*:',
    re.M)

# Ceremony that is not a member to port. `depth.py` has the same list for the
# same reason; it is imported rather than repeated.
SKIP = depth.DART_CEREMONY


def load():
    if not os.path.exists(AST):
        raise SystemExit(
            'no %s -- run:\n'
            '  dart run --packages=<flutter>/.dart_tool/package_config.json \\\n'
            '      tools/dart/dump_ast.dart <flutter>/packages/flutter/lib/src '
            '%s' % (AST, AST))
    return json.load(io.open(AST, encoding='utf-8'))


def templates(declarations):
    """`{@template name} ... {@endtemplate}` bodies, by name.

    Flutter writes a doc once and points at it from every widget that shares
    the behaviour, so most of the interesting comments are not where the member
    is -- `SelectableText.showCursor`'s doc is the single line
    `{@macro flutter.widgets.editableText.showCursor}`. Resolving them is the
    difference between carrying the reason down and carrying a pointer.
    """
    found = {}
    pattern = re.compile(
        r'\{@template\s+(?P<name>[\w.]+)\}(?P<body>.*?)\{@endtemplate\}',
        re.S)
    def walk(node):
        doc = node.get('doc')
        if doc:
            for match in pattern.finditer(doc):
                found[match.group('name')] = match.group('body').strip()
        for member in node.get('members', ()):
            walk(member)
    for declaration in declarations:
        walk(declaration)
    return found


def resolve_doc(doc, macros, depth_left=4):
    """Upstream's comment with its macros substituted."""
    if not doc:
        return doc
    if depth_left <= 0:
        return doc
    def swap(match):
        return macros.get(match.group('name'), match.group(0))
    resolved = re.sub(r'\{@macro\s+(?P<name>[\w.]+)\}', swap, doc)
    if resolved != doc:
        return resolve_doc(resolved, macros, depth_left - 1)
    # The remaining directives are dartdoc's own machinery, not prose: a
    # `{@youtube}` is a video embed, and a `{@template}` marker only says where
    # the text was *defined*. Neither means anything once the words are here.
    return re.sub(
        r'\{@(?:template|endtemplate|youtube|animation|tool|end-?tool)[^}]*\}',
        '', resolved).strip()


def field_types(cls):
    """Declared type of each field, so `this.x` parameters can be given one.

    A constructor written `this.showCursor = false` carries no type at all --
    it is on the field. Any tool that reads only the constructor sees a
    parameter with a default and no idea what it holds.
    """
    types = {}
    for member in cls.get('members', ()):
        if member.get('kind') != 'field':
            continue
        for name in member.get('names', ()):
            types[name['name']] = member.get('type')
    return types


def surface(cls, macros):
    """The members of `cls` a port would have to answer for.

    Constructor parameters count, and count *first*: a Flutter widget's
    surface is its constructor, and `this.x` parameters are the fields.
    """
    types = field_types(cls)
    docs = {}
    for member in cls.get('members', ()):
        if member.get('kind') == 'field':
            for name in member.get('names', ()):
                docs[name['name']] = member.get('doc')

    out = []
    seen = set()
    for member in cls.get('members', ()):
        kind = member.get('kind')
        if kind == 'constructor':
            for param in member.get('params', ()):
                name = param.get('name')
                if not name or name in seen or name.startswith('_'):
                    continue
                seen.add(name)
                out.append({
                    'name': name,
                    'kind': 'parameter',
                    'type': param.get('type') or types.get(name),
                    'default': param.get('default'),
                    'named': param.get('named'),
                    'required': param.get('required'),
                    'doc': resolve_doc(docs.get(name), macros),
                })
        elif kind in ('getter', 'method'):
            name = member.get('name')
            if (not name or name in seen or name.startswith('_')
                    or name in SKIP or member.get('override')):
                continue
            seen.add(name)
            out.append({
                'name': name,
                'kind': kind,
                'type': member.get('return'),
                'params': member.get('params', ()),
                'doc': resolve_doc(member.get('doc'), macros),
            })
        elif kind == 'field' and member.get('static'):
            for name in member.get('names', ()):
                if name['name'] in seen or name['name'].startswith('_'):
                    continue
                seen.add(name['name'])
                out.append({
                    'name': name['name'],
                    'kind': 'constant',
                    'type': member.get('type'),
                    'default': name.get('default'),
                    'doc': resolve_doc(member.get('doc'), macros),
                })
    return out


# Dart names that are Rust keywords. `type` is the common one -- it is an
# ordinary getter (`LocalizationsDelegate.type`) and an ordinary parameter
# (`BottomNavigationBarThemeData.type`) in Dart -- and `fn`, `ref` and `move`
# turn up as parameter names.
#
# They take a trailing underscore rather than becoming raw identifiers. `r#type`
# would keep upstream's spelling exactly, which is tempting, but these names get
# *composed*: the builder for a field is `with_` + the name, and `with_r#type`
# is not an identifier at all. A suffix survives being pasted into another name.
RUST_KEYWORDS = {
    'as', 'break', 'const', 'continue', 'dyn', 'else', 'enum', 'extern',
    'false', 'fn', 'for', 'if', 'impl', 'in', 'let', 'loop', 'match', 'mod',
    'move', 'mut', 'pub', 'ref', 'return', 'static', 'struct', 'trait', 'true',
    'type', 'unsafe', 'use', 'where', 'while', 'async', 'await', 'box',
    'become', 'do', 'final', 'macro', 'override', 'priv', 'try', 'typeof',
    'unsized', 'virtual', 'yield',
}
NOT_RAW = {'crate', 'self', 'super'}

# Dart operator declarations. Their "name" is the operator itself, so passing
# them through gives `pub fn -()`. Rust spells these as trait impls, and which
# trait is a decision for whoever ports the class -- so they are emitted as
# plainly-named methods with the operator kept in the comment above, and the
# porter turns them into `impl Sub` or leaves them.
OPERATORS = {
    '+': 'op_add', '-': 'op_sub', '*': 'op_mul', '/': 'op_div',
    '~/': 'op_int_div', '%': 'op_rem', '==': 'op_eq', '<': 'op_lt',
    '>': 'op_gt', '<=': 'op_le', '>=': 'op_ge', '[]': 'op_index',
    '[]=': 'op_index_set', '&': 'op_bit_and', '|': 'op_bit_or',
    '^': 'op_bit_xor', '~': 'op_bit_not', '<<': 'op_shl', '>>': 'op_shr',
    'unary-': 'op_neg',
}


def snake(name):
    """`showCursor` -> `show_cursor`, which is what the crate calls it.

    Also the one place that guarantees the result is a legal Rust identifier,
    because every caller emits it straight into source.
    """
    if name in OPERATORS:
        return OPERATORS[name]
    out = re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()
    if out in RUST_KEYWORDS or out in NOT_RAW:
        return out + '_'
    # A Dart name that is not an identifier at all -- a private `_foo`, or an
    # operator this map does not know -- must not reach the output as itself.
    if not re.match(r'^[A-Za-z_][A-Za-z0-9_]*$', out):
        return 'op_' + re.sub(r'[^A-Za-z0-9_]', '_', out).strip('_')
    return out


# Dart types this crate already has a name for. Deliberately short: a guess
# that is wrong is worse than a `TODO`, because a wrong type compiles.
TYPES = {
    'bool': 'bool',
    'int': 'i32',
    'double': 'f32',
    'String': 'String',
    'Color': 'Color',
    'Widget': 'AnyWidget',
    'EdgeInsets': 'EdgeInsets',
    'EdgeInsetsGeometry': 'EdgeInsetsGeometry',
    'Duration': 'i64',
    'TextStyle': 'TextStyle',
    'TextAlign': 'TextAlign',
    'TextDirection': 'TextDirection',
    'Alignment': 'Alignment',
    'AlignmentGeometry': 'Alignment',
    'BorderRadius': 'BorderRadius',
    'Offset': 'Offset',
    'Size': 'Size',
    'Rect': 'Rect',
    'Curve': 'Curve',
    'VoidCallback': 'Rc<dyn Fn()>',
}


def rust_type(dart):
    """A Rust type for a Dart one, or `None` when this cannot say.

    `None` is a real answer and is left in the output as a `TODO`. The
    alternative -- reaching for something plausible -- produces code that
    compiles and is wrong, which is the failure mode this whole file is
    arranged to avoid.
    """
    if not dart:
        return None
    text = dart.strip()
    optional = text.endswith('?')
    if optional:
        text = text[:-1]
    mapped = TYPES.get(text)
    if mapped is None:
        return None
    return 'Option<%s>' % mapped if optional else mapped


def wrap(text, width=72, indent='    /// '):
    """Upstream's comment as a Rust doc comment, reflowed.

    Paragraphs are joined before being re-wrapped. Wrapping each *source* line
    on its own keeps upstream's line breaks, and because those were chosen for
    a different margin the result is a column of ragged half-lines -- readable
    enough to skim past, which is how a carried-down comment stops being read.

    Lines that are structure rather than prose -- list items, code fences,
    indented samples -- are passed through whole, since reflowing them destroys
    the thing that made them legible.
    """
    out = []
    paragraph = []
    hang = ['']

    def flush():
        if not paragraph:
            return
        prefix = indent
        current = ''
        for word in ' '.join(paragraph).split():
            if current and len(prefix) + len(current) + 1 + len(word) > width:
                out.append(prefix + current)
                # Continuations of a list item line up under its text, so the
                # bullets stay the thing the eye finds.
                prefix = indent + hang[0]
                current = word
            else:
                current = word if not current else current + ' ' + word
        if current:
            out.append(prefix + current)
        del paragraph[:]
        hang[0] = ''

    for line in (text or '').split('\n'):
        stripped = line.strip()
        if not stripped:
            flush()
            # Removing a `{@template}` marker leaves the blank line that was
            # around it; two of those in a row is a hole in the prose, not a
            # paragraph break upstream asked for.
            if out and out[-1].strip().strip('/'):
                out.append(indent.rstrip())
        elif hang[0] and not stripped.startswith(('*', '-')):
            # Inside a bullet, an indented line is that bullet's second line --
            # checked before the sample rule below, which would otherwise read
            # upstream's four-space list continuation as preformatted text.
            paragraph.append(stripped)
        elif stripped.startswith(('```', '#', '|')) or line.startswith('    '):
            flush()
            out.append((indent + stripped).rstrip())
        elif stripped.startswith(('*', '-')):
            # A new bullet ends the previous one; the lines under it belong to
            # it until a blank line, so they are gathered, not flushed alone.
            flush()
            paragraph.append(stripped)
            hang[0] = '  '
        else:
            paragraph.append(stripped)
    flush()
    # Trailing blank lines read as a gap between the doc and the item it is on.
    while out and not out[-1].strip().strip('/'):
        out.pop()
    return '\n'.join(out)


def emit(cls, missing, macros):
    """A Rust skeleton for what is missing. Scaffolding, never behaviour."""
    name = cls['name']
    out = []
    out.append('// GENERATED SCAFFOLDING for upstream `%s` (%s).' % (name, cls['file']))
    out.append('//')
    out.append('// Every body below is `todo!()` on purpose: this tool carries')
    out.append('// signatures, defaults and upstream\'s own words down, and stops')
    out.append('// there. What each one should *do* is the part the rounds were')
    out.append('// spent on -- see the header of tools/port.py.')
    out.append('//')
    out.append('// This holds only the members the crate does NOT already answer')
    out.append('// for, so it is something to merge into the existing type, not')
    out.append('// something to paste beside it. Pasted whole it would declare a')
    out.append('// second `%s` next to the real one.' % name)
    out.append('')
    if cls.get('doc'):
        # Resolved like the members' docs are: a class comment is as full of
        # `{@macro}` as any of them, and an unresolved one carries a pointer
        # into a file the reader of this skeleton does not have open.
        out.append(wrap(resolve_doc(cls['doc'], macros), indent='/// '))
    out.append('pub struct %s {' % name)
    for member in missing:
        if member['kind'] != 'parameter':
            continue
        mapped = rust_type(member.get('type'))
        # When the field itself cannot be emitted it is left commented out, and
        # then its doc must be commented out with it. A `///` above a commented
        # line does not document nothing -- it silently attaches to whatever
        # real field comes *next*, which is how a port acquires a field
        # carrying another field's explanation.
        marker = '    /// ' if mapped is not None else '    // '
        if member.get('doc'):
            out.append(wrap(member['doc'], indent=marker))
        if member.get('default'):
            out.append('%sUpstream default: `%s`' % (marker, member['default']))
        if mapped is None:
            out.append('    // TODO(type): upstream `%s`' % (member.get('type') or '?'))
            out.append('    // %s: ...,' % snake(member['name']))
        else:
            out.append('    %s: %s,' % (snake(member['name']), mapped))
        out.append('')
    out.append('}')
    out.append('')
    out.append('impl %s {' % name)
    for member in missing:
        if member['kind'] != 'parameter':
            continue
        mapped = rust_type(member.get('type'))
        if mapped is None:
            continue
        out.append('    pub fn with_%s(mut self, %s: %s) -> Self {'
                   % (snake(member['name']), snake(member['name']), mapped))
        out.append('        self.%s = %s;' % (snake(member['name']), snake(member['name'])))
        out.append('        self')
        out.append('    }')
        out.append('')
    for member in missing:
        if member['kind'] not in ('getter', 'method'):
            continue
        if member.get('doc'):
            out.append(wrap(member['doc']))
        mapped = rust_type(member.get('type')) or '()'
        args = ''.join(
            ', %s: /* %s */ ()' % (snake(p['name'] or 'arg'), p.get('type') or '?')
            for p in member.get('params', ()) if p.get('name'))
        out.append('    // upstream: %s %s(%s)' % (
            member.get('type') or 'void', member['name'],
            ', '.join('%s %s' % (p.get('type') or '?', p.get('name') or '')
                      for p in member.get('params', ()))))
        out.append('    pub fn %s(&self%s) -> %s {' % (snake(member['name']), args, mapped))
        out.append('        todo!("%s.%s")' % (name, member['name']))
        out.append('    }')
        out.append('')
    out.append('}')
    return '\n'.join(out)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('klass', nargs='?', help='upstream class name')
    parser.add_argument('--list', action='store_true',
                        help='every class with missing members, worst first')
    parser.add_argument('--emit', action='store_true',
                        help='print a Rust skeleton for what is missing')
    parser.add_argument('--limit', type=int, default=40)
    parser.add_argument('--out', metavar='FILE',
                        help='write the skeleton here as UTF-8, rather than '
                             'to a console whose encoding may not hold it')
    args = parser.parse_args()

    # Upstream's docs contain characters this console cannot encode -- the
    # grapheme-cluster explanations in `EditableText` are written with actual
    # emoji. Printing must not be what decides whether a class can be ported,
    # so stdout is widened and `--out` writes the file directly.
    if hasattr(sys.stdout, 'reconfigure'):
        sys.stdout.reconfigure(encoding='utf-8', errors='replace')

    data = load()
    declarations = data['declarations']
    macros = templates(declarations)
    classes = {d['name']: d for d in declarations
               if d['kind'] in ('class', 'mixin') and d.get('name')}
    # Generated tables are not ported member by member -- this crate generates
    # its own from the same upstream source (see host/tools/gen_key_map.py),
    # so counting their thousands of constants as "missing" would bury every
    # real row under them. Named rather than pattern-matched: a rule that
    # silently swallowed a real class would be worse than a list of four.
    GENERATED = {'Icons', 'CupertinoIcons', 'LogicalKeyboardKey',
                 'PhysicalKeyboardKey'}

    bodies = depth.rust_bodies()
    aliases = depth.rust_aliases()

    def ported(name, member_names):
        """Which of `member_names` the crate already answers for.

        `depth.RUST_MEMBER` is not reused here and that is not an oversight:
        it matches *where* a member starts, because counting is all a ratio
        needs. This needs the member's **name**, to say which particular one
        is missing, so it names them itself.
        """
        # `companion_body` joins its candidates with `''.join`, so a class
        # with no Rust counterpart comes back as the empty string rather than
        # as `None`. Read as "found, nothing in it", that reports every
        # unported class as `0 of N` -- which is the same row a *fully missing*
        # class would get, and the two are not the same fact.
        body = depth.companion_body(bodies, name, aliases)
        if not body.strip():
            return set(), False
        have = {name for pair in RUST_NAME.findall(body)
                for name in pair if name}
        hit = set()
        for member in member_names:
            candidate = snake(member)
            if (candidate in have
                    or member.lower() in have
                    or ('with_' + candidate) in have
                    or ('set_' + candidate) in have
                    or ('is_' + candidate) in have):
                hit.add(member)
        return hit, True

    if args.list:
        rows = []
        for name, cls in classes.items():
            # Upstream's private classes are not a port's business: nothing
            # outside their own library can name them.
            if name.startswith('_') or name in GENERATED:
                continue
            members = surface(cls, macros)
            if len(members) < 4:
                continue
            names = [m['name'] for m in members]
            have, present = ported(name, names)
            if not present:
                continue
            gap = len(names) - len(have)
            if gap <= 0:
                continue
            rows.append((gap, len(have), len(names), name, cls['file']))
        rows.sort(reverse=True)
        print('%5s %5s %5s  %s' % ('gap', 'have', 'all', 'class / file'))
        for gap, have, total, name, path in rows[:args.limit]:
            print('%5d %5d %5d  %s  (%s)' % (gap, have, total, name, path))
        print()
        print('%d classes with members still to answer for' % len(rows))
        return 0

    if not args.klass:
        parser.print_help()
        return 2

    cls = classes.get(args.klass)
    if cls is None:
        raise SystemExit('no upstream class %r in the dump' % args.klass)
    members = surface(cls, macros)
    have, present = ported(args.klass, [m['name'] for m in members])
    missing = [m for m in members if m['name'] not in have]

    if args.emit:
        skeleton = emit(cls, missing, macros)
        if args.out:
            io.open(args.out, 'w', encoding='utf-8', newline='\n').write(
                skeleton + '\n')
            print('%s -> %s (%d members)'
                  % (cls['name'], args.out, len(missing)))
        else:
            print(skeleton)
        return 0

    print('%s  (%s)' % (cls['name'], cls['file']))
    print('%d of %d members already answered for%s'
          % (len(have), len(members), '' if present else ' -- NO RUST TYPE FOUND'))
    print()
    for member in missing:
        default = (' = %s' % member['default']) if member.get('default') else ''
        print('  %-10s %-28s %s%s'
              % (member['kind'], member['name'], member.get('type') or '?', default))
        if member.get('doc'):
            first = [line for line in member['doc'].split('\n') if line.strip()]
            if first:
                print('             %s' % first[0][:96])
    return 0


if __name__ == '__main__':
    sys.exit(main())
