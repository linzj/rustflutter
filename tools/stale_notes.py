"""A doc comment that says something is not here, checked against the crate.

Twenty-odd doc comments in this crate carry a sentence of the form

    `inputDecorationTheme` is not here: it is an `InputDecorationThemeData`,
    `Typography`, `TextTheme` and `IconThemeData` are not here yet
    upstream's `BackdropFilter` blur is not ported (see the module docs)

Each is a claim about the port, and each one can stop being true without
anything failing. A later tick ports the thing, the sentence stays, and the
next reader is told a gap exists where there is none -- which is worse than no
note at all, because it sends them to write it a second time.

That is the mistake of tick 284 seen from the other side: there, an answer
given under a different name was invisible to a search by name; here, the crate
says outright that it has no answer, and is wrong.

# What counts as the subject

Only the mechanical form: a doc-comment sentence naming one or more
**backticked identifiers** followed by `not here` or `not ported`. The subject
is the run of identifiers *before* that phrase, never the ones after it -- the
reason usually names types that do exist, and reading those as subjects would
make every note look stale.

# Where each subject is looked for

The corpus writes two kinds of subject and they have different scopes.

* **`lowerCamelCase`** is an upstream *member*, and the claim is about the item
  the note is attached to. `TimePickerThemeData` saying it has no
  `inputDecorationTheme` is not refuted by some other theme having one, so the
  lookup is that item's own fields and methods.
* **`UpperCamelCase`** is an upstream *type*, and the claim is about the crate.
  `theme.rs` saying `TextTheme` is not here is refuted by any module declaring
  it.

A module-level `//!` note is crate-scoped either way -- it has no item to
belong to.

A name that is only *mentioned* -- in prose, in another doc comment, in a
string -- never counts. The claim is that no such thing exists, and only a
declaration refutes it.

# What it does not reach

A note making the same claim without naming a subject:

    /// Shift-extend selection itself: nothing here reads this to widen a
    /// selection, because the selection model has no extend-from-anchor.

There is no identifier to look up, so this ruler is silent. That hole is real
and stated rather than papered over: the one in `tap_and_drag.rs` was found by
hand in tick 286, and it had expired too.

# The neighbouring ruler that was tried and thrown away (tick 358)

There is a second kind of note that can expire: one claiming coverage exists.
Three were found false by hand and fixed --

    The functions are exercised end to end by rust_ffi_unittests     (app.rs)
    under the stubs the protocol is exercised by the engine-backed runs
                                                                  (render.rs)
    The tree side is covered by `async_builder`'s own tests         (async.rs)

-- and a detector for that shape was built against those three as a corpus,
which is the calibration ticks 341 and 346 skipped. It reached 3 of 3, and
then flagged 13 across the tree. **Every one of the other ten was already
true**, and six were not claims at all: `covered by` and `pinned by` are
ordinary English, and it cannot tell "covered by a test" from "covered by a
transparent rectangle" or "pinned by their edges".

So it found nothing, and it is not here. The reason is worth more than the
tool: **the corpus had already been cleaned.** Rounds 356 and 357 found those
three by hand and fixed them, so the only skill the detector demonstrated was
recognising three sentences that no longer exist. A ruler calibrated on faults
that are already fixed is calibrated on nothing.

What would make it worth building is a corpus found *some other way* -- false
coverage claims nobody has fixed yet. Until such a set exists, a hand pass over
`covered by|exercised by|pinned by` is the whole of the technique, and it takes
one round.
"""
import io
import os
import re
import sys

CRATE = os.path.join('src', 'flutter', 'rust', 'rustflutter', 'src')

CLAIM = re.compile(
    r'((?:`[A-Za-z_][A-Za-z0-9_]*`(?:\s*,\s*|\s+and\s+)?)+)'
    r'[^.`]{0,40}?\b(?:is|are)\s+not\s+(?:here|ported)\b'
)
NAME = re.compile(r'`([A-Za-z_][A-Za-z0-9_]*)`')

DECL = re.compile(
    r'^\s*(?:pub(?:\([^)]*\))?\s+)?'
    r'(?:const\s+|static\s+|async\s+|unsafe\s+|extern\s+"[^"]*"\s+)*'
    r'(?:struct|enum|trait|type|fn|mod|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)'
)
MEMBER = re.compile(
    r'^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+|unsafe\s+)*'
    r'(?:fn\s+)?([a-z_][a-z0-9_]*)\s*[:(]'
)
ITEM = re.compile(
    r'^\s*(?:pub(?:\([^)]*\))?\s+)?'
    r'(?:struct|enum|trait|type|fn|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)'
)


def snake(name):
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()


def crate_files():
    files = []
    for root, _, names in os.walk(CRATE):
        for name in names:
            if name.endswith('.rs'):
                files.append(os.path.join(root, name))
    return sorted(files)


def declared_types(sources):
    """Every item the crate declares, by name."""
    names = set()
    for text in sources.values():
        for line in text.split('\n'):
            if line.lstrip().startswith('//'):
                continue
            found = DECL.match(line)
            if found:
                names.add(found.group(1))
    return names


def documented_item(lines, index):
    """The item a `///` note at `index` is attached to, and where it starts.

    Walks down over the rest of the doc block and any attributes. Returns
    `None` for a `//!` note, which belongs to the module rather than an item.
    """
    if lines[index].lstrip().startswith('//!'):
        return None
    cursor = index
    while cursor < len(lines):
        stripped = lines[cursor].lstrip()
        if stripped.startswith('///') or stripped.startswith('#[') or not stripped:
            cursor += 1
            continue
        found = ITEM.match(lines[cursor])
        return (found.group(1), cursor) if found else None
    return None


def members_of(lines, start):
    """The field and method names declared inside the item starting at `start`.

    Also follows every `impl <Name>` block elsewhere in the file, because a
    member can be a method rather than a field.
    """
    names = set()
    depth = 0
    cursor = start
    while cursor < len(lines):
        line = lines[cursor]
        if not line.lstrip().startswith('//'):
            found = MEMBER.match(line)
            if found and depth >= 1:
                names.add(found.group(1))
            depth += line.count('{') - line.count('}')
            if depth <= 0 and cursor > start and '{' in ''.join(lines[start:cursor + 1]):
                break
        cursor += 1
    return names


def main():
    files = crate_files()
    sources = {path: io.open(path, encoding='utf-8', errors='replace').read()
               for path in files}
    types = declared_types(sources)

    claims = 0
    stale = []
    for path in files:
        lines = sources[path].split('\n')
        for index, line in enumerate(lines):
            if '///' not in line and '//!' not in line:
                continue
            for match in CLAIM.finditer(line):
                subjects = NAME.findall(match.group(1))
                if not subjects:
                    continue
                claims += 1
                owner = documented_item(lines, index)
                for subject in subjects:
                    wanted = snake(subject)
                    if subject[:1].isupper():
                        # A type name: the claim is about the crate.
                        if subject in types:
                            stale.append((path, index + 1, subject,
                                          'the crate declares it', line.strip()))
                    elif owner is not None:
                        # A member name: the claim is about this item alone.
                        if wanted in members_of(lines, owner[1]):
                            stale.append((path, index + 1, subject,
                                          '`%s` declares it' % owner[0],
                                          line.strip()))
                    elif wanted in types:
                        # A module note naming a member: crate-scoped.
                        stale.append((path, index + 1, subject,
                                      'the crate declares it', line.strip()))

    for path, number, subject, why, line in stale:
        print('%s:%d' % (path.replace('\\', '/'), number))
        print('    the note says `%s` is not here, and %s' % (subject, why))
        print('    %s' % line)
    print()
    print('%d notes claim something is not here, %d of them no longer true'
          % (claims, len(stale)))
    return 1 if stale else 0


if __name__ == '__main__':
    sys.exit(main())
