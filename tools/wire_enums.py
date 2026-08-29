"""Enums whose *order* is the protocol, compared against upstream's order.

`variant_sweep.py` says in its own docs what it cannot see:

    It rewrites single-line match arms, so a table with no match in it is
    invisible. The one that turned up is the worst kind:
    `PlatformProvidedMenuItemType` goes over the channel as `menu_type as
    i32`, so the *declaration order* is the protocol and there are no arms at
    all -- a variant inserted in the middle renumbers eleven menu items and
    this sweep would report the file as having nothing to look at.

`unwalked.py` found that one, and the port answered it with a test. But look
at what the test can prove:

    for (position, menu_type) in PlatformProvidedMenuItemType::ALL.iter().enumerate() {
        assert_eq!(sent(*menu_type), position as i32);
    }

That is circular. It says `ALL` agrees with the declaration order, which two
lists written next to each other will always do. **Upstream's order lives in a
doc comment above it** -- twelve names in prose -- and nothing compares the
comment to Dart. A variant inserted upstream, or one this port dropped,
renumbers everything after it and every test here stays green.

This is the comparison the comment was standing in for. It is a *ruler*: the
number has to be zero.

# What is in scope

An enum qualifies when its numbering is observable outside this crate:

* it is sent over a channel as `Name as i32` -- the declaration order is then
  literally the wire format;
* it declares explicit discriminants (`= 0,`), which is a table saying its
  numbers matter and are not the compiler's to choose;
* or it has an `index()` method whose arms are literals **and whose result
  reaches a channel value**. That is this port's spelling of "a Dart enum's
  index". `ContentSensitivity` was declared twice in this crate with the two
  copies in *different orders*, and its own doc says how long that lasted:
  "nothing did cross -- no code converted between them, which is exactly why
  two copies could disagree for as long as they did".

The second half of that last rule was learned by leaving it out.
`HighlightType` has exactly such an `index()` and uses it to pick one of three
local slots, which is not a protocol at all -- reordering it would be
completely safe. An index nobody outside the crate can see is a private
matter, however much it looks like a Dart enum's.

Anything else may be reordered freely -- a `#[default]` arm hoisted to the
front is an ordinary and correct thing to do -- so including every enum would
bury the four that matter under a hundred that do not.

# How the upstream enum is found

By the port's own citation -- `Upstream \\`Name\\`` in the doc comment, the same
convention `coverage.py` and `constants.py` read -- and failing that, by the
port's own name.

The fallback is not laziness. Both of the first two enums found describe their
upstream in prose ("Upstream serialises `item.type.index`") without writing
the enum's name in backticks anywhere, and refusing to look them up on that
account would have left this reporting NO CITATION for the two tables it was
built for.

An enum that qualifies and matches no upstream enum by either route is
reported under NO CITATION: a table whose numbers are the protocol and whose
authority is nobody's, which is worse than a disagreement because there is
nothing to disagree with.

# A private variant at the end of upstream's list

`ContentSensitivity` has a fourth value, `_unknown`, private to that library.
There is nothing for the port to mirror -- nobody outside
`sensitive_content.dart` can name it -- and the three indices the port does
have are unaffected, so it is not a disagreement.

It is printed anyway, because the *platform can still send a 3*. Upstream
reads `values[result]` and throws an `UnsupportedError` saying to file an
issue; this port's `from_index` answers `None`, and its own doc says why that
is the same answer with nothing to throw to. Both are decisions. Silence would
not have been.

  python tools/wire_enums.py
"""
import os
import re
import sys

import paths

REPO = paths.REPO
PORT = paths.SRC
_UPSTREAM = paths.require_upstream()
DART = paths.upstream_src(_UPSTREAM)
ENGINE = paths.upstream_ui(_UPSTREAM)

# `pub enum Name {` with whatever doc comment sits above it.
ENUM = re.compile(
    r'(?P<doc>(?:^[ ]*///[^\n]*\n)*)^pub enum (?P<name>[A-Za-z0-9_]+)\s*\{',
    re.M)
VARIANT = re.compile(
    r'^\s{4}(?P<name>[A-Z][A-Za-z0-9_]*)\s*(?:=\s*(?P<value>[-\w]+)\s*)?(?:,|\{|\()',
    re.M)
CITATION = re.compile(r'Upstream `([A-Za-z0-9_.]+)`')


def strip_comments(text):
    return re.sub(r'//[^\n]*', '', text)


def enum_body(text, start):
    """The text between an enum's braces."""
    brace = text.index('{', start)
    depth = 0
    for pos in range(brace, len(text)):
        if text[pos] == '{':
            depth += 1
        elif text[pos] == '}':
            depth -= 1
            if depth == 0:
                return text[brace + 1:pos]
    return ''


def camel(name):
    """`HideOtherApplications` -> `hideOtherApplications`."""
    return name[0].lower() + name[1:]


def dart_enums():
    """Every Dart enum in the framework and in dart:ui, by name."""
    found = {}
    for root_dir in (DART, ENGINE):
        for root, _dirs, files in os.walk(root_dir):
            for name in files:
                if not name.endswith('.dart'):
                    continue
                path = os.path.join(root, name)
                text = open(path, encoding='utf-8', errors='replace').read()
                # `enum_name`, not `name`: the outer loop is over *files*, and
                # keying this table by the file name is how the first run of
                # this ruler reported both of its two enums as having no
                # upstream at all.
                for match in re.finditer(r'^enum ([A-Za-z0-9_]+)[^{]*\{', text,
                                         re.M):
                    enum_name = match.group(1)
                    body = enum_body(text, match.start())
                    # Comments first, *then* the split: everything up to the
                    # first `;` is the value list, and a `;` inside a doc
                    # comment is not that one. `BlendMode`'s prose has
                    # several, and splitting first cut its twenty-nine values
                    # down to thirteen -- which this ruler duly reported as
                    # the port having sixteen too many.
                    values = re.sub(r'//[^\n]*', '', body)
                    values = re.sub(r'/\*.*?\*/', '', values, flags=re.S)
                    values = values.split(';')[0]
                    # Strip any constructor arguments so `about('x')` reads
                    # as `about`.
                    names = []
                    depth = 0
                    current = ''
                    for char in values:
                        if char in '([{':
                            depth += 1
                        elif char in ')]}':
                            depth -= 1
                        elif char == ',' and depth == 0:
                            names.append(current)
                            current = ''
                            continue
                        if depth == 0:
                            current += char
                    names.append(current)
                    cleaned = []
                    for entry in names:
                        entry = entry.strip()
                        entry = re.sub(r'^@\w+(\([^)]*\))?\s*', '', entry).strip()
                        word = re.match(r'^([a-zA-Z_][A-Za-z0-9_]*)', entry)
                        if word:
                            cleaned.append(word.group(1))
                    if cleaned:
                        # Relative to the upstream root, not to a shared
                        # parent: the two checkouts no longer sit side by
                        # side, and on Windows `relpath` across drives raises
                        # rather than falling back to an absolute path.
                        found.setdefault(
                            enum_name,
                            (cleaned, os.path.relpath(path, _UPSTREAM)))
    return found


def port_enums():
    """Every `pub enum` in the port, with its variants, doc and file."""
    out = []
    for root, _dirs, files in os.walk(PORT):
        for name in sorted(files):
            if not name.endswith('.rs'):
                continue
            path = os.path.join(root, name)
            text = open(path, encoding='utf-8', errors='replace').read()
            where = os.path.relpath(path, PORT).replace(os.sep, '/')
            for match in ENUM.finditer(text):
                body = enum_body(text, match.start('name'))
                variants = [(m.group('name'), m.group('value'))
                            for m in VARIANT.finditer(body)]
                if variants:
                    out.append((match.group('name'), variants,
                                match.group('doc'), where, text))
    return out


def is_wire(name, variants, whole_file):
    """Whether this enum's numbering is visible outside the crate."""
    if any(value is not None for _variant, value in variants):
        return 'explicit discriminants'
    # A `fn index` whose arms are literals *and whose result reaches a channel
    # value*. The second half was learned by leaving it out: `HighlightType`
    # has exactly such a method and uses it to index a local array of three
    # highlights, which is not a protocol at all -- reordering it would be
    # entirely safe. An index nobody outside the crate can see is a private
    # matter, however much it looks like a Dart enum index.
    if re.search(r'\b%s::\w+\s*=>\s*\d+' % re.escape(name), whole_file) \
            and re.search(r'fn index\(', whole_file) \
            and re.search(r'(Value::I32|ChannelValue::Int)\([^)]*\.index\(\)',
                          whole_file):
        return 'index() is the protocol'
    if re.search(r'\b%s::\w+ as i32|\b\w+\.?\w* as i32' % re.escape(name),
                 strip_comments(whole_file)):
        # The loose second alternative catches `provided.menu_type as i32`,
        # where the field's type is this enum but the name is not written. So
        # confirm the enum is actually a field or binding that reaches an
        # `as i32` in the same file.
        for cast in re.finditer(r'([A-Za-z_][\w.]*) as i32', strip_comments(whole_file)):
            expression = cast.group(1)
            leaf = expression.split('.')[-1]
            if leaf == name:
                return 'sent as i32'
            # `provided.menu_type as i32` with `menu_type: ThisEnum` declared.
            if re.search(r'\b%s: %s\b' % (re.escape(leaf), re.escape(name)),
                         whole_file):
                return 'sent as i32'
    return None


def main():
    upstream = dart_enums()
    rows = []
    for name, variants, doc, where, whole in port_enums():
        reason = is_wire(name, variants, whole)
        if reason is None:
            continue
        cited = CITATION.search(doc or '')
        citation = cited.group(1) if cited else None
        if citation is None or citation.split('.')[0] not in upstream:
            # Fall back to the port's own name: these tables are named after
            # the Dart enum they mirror.
            citation = name if name in upstream else citation
        rows.append((name, variants, citation, where, reason))

    disagreeing, uncited, unfound, private_tails = [], [], [], []
    for name, variants, citation, where, reason in rows:
        if citation is None:
            uncited.append((name, where, reason))
            continue
        target = citation.split('.')[0]
        if target not in upstream:
            unfound.append((name, where, citation))
            continue
        theirs, source = upstream[target]
        ours = [camel(variant) for variant, _value in variants]
        # A trailing *private* variant upstream is not a disagreement about
        # the numbering: it cannot be named by anyone outside that library, so
        # the port has nothing to mirror and the indices it does have are
        # unaffected. `ContentSensitivity._unknown` is the case -- upstream
        # reads `values[result]` and throws an `UnsupportedError` on it, and
        # this port's `from_index` answers `None`, which its own doc says is
        # the same answer with nothing to throw to.
        #
        # It is still worth printing, because the *platform can send that
        # index*: something has to decide what happens then, and silence is
        # not a decision.
        trailing_private = []
        while len(theirs) > len(ours) and theirs[len(theirs) - 1].startswith('_'):
            trailing_private.insert(0, theirs.pop())
        if ours != theirs:
            disagreeing.append((name, where, citation, ours, theirs, source))
        elif trailing_private:
            private_tails.append((name, where, len(ours), trailing_private))

    print('%d enums whose numbering is visible outside this crate, '
          '%d disagreeing with upstream\'s order, %d citing nothing, '
          '%d whose upstream enum could not be found '
          '(%d with a private trailing variant upstream, which is a note)'
          % (len(rows), len(disagreeing), len(uncited), len(unfound),
             len(private_tails)))
    print()
    for name, where, citation, ours, theirs, source in disagreeing:
        print('  DISAGREES %s (%s) against %s in %s' % (name, where, citation, source))
        for index in range(max(len(ours), len(theirs))):
            mine = ours[index] if index < len(ours) else '--'
            yours = theirs[index] if index < len(theirs) else '--'
            mark = '   ' if mine == yours else ' <-'
            print('    %2d  %-32s %-32s%s' % (index, mine, yours, mark))
    for name, where, count, tail in private_tails:
        print('  PRIVATE TAIL UPSTREAM     %-30s %s' % (name, where))
        print('    index %d upstream is %s -- private, so there is nothing to '
              'mirror, but the' % (count, ', '.join(tail)))
        print('    platform can still send that number, and something has to '
              'decide what happens.')
    for name, where, reason in uncited:
        print('  NO CITATION               %-32s %s (%s)' % (name, where, reason))
    for name, where, citation in unfound:
        print('  UPSTREAM ENUM NOT FOUND   %-32s %s (looked for %s)'
              % (name, where, citation))
    print()
    for name, variants, citation, where, reason in sorted(rows):
        print('  %-34s %-28s %-22s %d variants'
              % (name, where, reason, len(variants)))
    return 1 if disagreeing or uncited else 0


if __name__ == '__main__':
    sys.exit(main())
