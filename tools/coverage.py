#!/usr/bin/env python3
"""Coverage of the Rust framework against upstream Flutter, per public class.

Upstream is the Flutter checkout this tree was forked from (see
PORTING_STATUS.md for the pin).  Every public class or mixin declared anywhere
under packages/flutter/lib/src -- every layer directory, subdirectories
included -- must end up in exactly one of five states:

  covered        a symbol with the same (snake_cased) name exists in the crate
  mapped         the ledger records a rename / merge / functional equivalent
  blocked-engine depends on an engine capability we do not bridge yet
  out-of-scope   web-only, no-host-platform, or debug-only per the ledger
  MISSING        none of the above -- this is the work queue

Private classes (leading underscore) and *_io.dart / *_web.dart variants are
not counted; platform-specific classes inside mixed files are ledgered
instead.  Name matching is done on comment-stripped sources so words in doc
comments do not count as coverage.

Usage:
  python tools/coverage.py                 # summary + per-file detail
  python tools/coverage.py widgets/basic   # only files matching a substring
  python tools/coverage.py --missing-only  # only the work queue
"""

import argparse
import json
import os
import re
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
UPSTREAM = os.environ.get('FLUTTER_UPSTREAM', r'K:\flutter')
CRATE = os.path.join(REPO, 'src', 'flutter', 'rust', 'rustflutter', 'src')
LEDGER = os.path.join(REPO, 'coverage_ledger.json')

# Every directory under packages/flutter/lib/src, not a chosen ten.
#
# `physics`, `semantics` and `widget_previews` were missing here for the whole
# sweep, and the omission hid work rather than flattering the number: the crate
# already ports most of `physics` (6 of 9) and part of `semantics`, so these are
# layers the project is in, not layers it declared out of scope. The plan's own
# scope line says "framework 全层" while its title says ten -- this resolves that
# in the direction that cannot hide anything.
LAYERS = [
    'widgets', 'rendering', 'painting', 'gestures', 'services',
    'animation', 'scheduler', 'foundation', 'material', 'cupertino',
    'physics', 'semantics', 'widget_previews',
]

# Dart's class modifiers, all of which may stack: `abstract interface class`,
# `base mixin class`, `final class`. `interface` was missing here and cost the
# ruler every `abstract interface class` in the tree -- a blind spot that hides
# work rather than flattering it, which is the worse of the two failures: a
# class the ruler cannot see can never be reported MISSING.
#
# `enum` was missing for the same reason and for longer. It cost 42 public
# types -- `ThemeMode`, `ImageRepeat`, `DeviceOrientation`, `WrapCrossAlignment`
# -- none of which could ever be reported, so the ruler said 0 MISSING while
# they were absent. An enum is not a lesser kind of class here: it is a named
# public type that the rest of the API branches on, which makes it exactly the
# kind of thing this ledger exists to count.
#
# `extension type` is included on the same grounds -- a named public type.
#
# Deliberately NOT counted, and this is a judgment rather than an oversight:
# `typedef` (411 of them) names a function signature, not a type to port, and
# `extension X on Y` adds methods to somebody else's type rather than
# introducing one. Counting either would inflate the denominator with things
# that have no Rust counterpart to be missing.
CLASS_RE = re.compile(
    r'^(?:abstract\s+|base\s+|final\s+|interface\s+|sealed\s+|mixin\s+)*'
    r'(?:class|mixin|enum|extension\s+type)\s+(?:const\s+)?([A-Za-z0-9_]+)',
    re.M,
)
# The `const` is optional and goes *after* `extension type`, not before it:
# `extension type const BaselineOffset(double? offset)`. Without that group the
# ruler captured `const` as the type's name -- and then, since `const` has no
# leading underscore, it sailed past the private filter and was reported
# MISSING four times, once per private extension type in the tree. A ruler's
# first run is worth exactly as much as the audit of its first run.


def strip_dart_comments(text):
    text = re.sub(r'/\*.*?\*/', '', text, flags=re.S)
    text = re.sub(r'//[^\n]*', '', text)
    return text


def strip_rust_comments(text):
    # Nested block comments are legal in Rust; strip innermost-outwards.
    while True:
        stripped = re.sub(r'/\*(?:[^*/]|\*(?!/)|/(?!\*))*\*/', '', text, flags=re.S)
        stripped = re.sub(r'//[^\n]*', '', stripped)
        if stripped == text:
            return text
        text = stripped


def strip_test_modules(text):
    """Drop every `#[cfg(test)] mod ... { ... }` block.

    A struct written to stand something up in a test is not a port of anything.
    Caught crediting upstream's `Page` (navigator.dart) to a four-field `struct
    Page` inside `autocomplete_view.rs`'s test module -- written to give a
    portal something to hang off, and named for what it was, not for upstream.
    """
    out = []
    i = 0
    for m in re.finditer(r'#\[cfg\(test\)\]\s*mod\s+\w+\s*\{', text):
        brace = text.index('{', m.end() - 1)
        depth = 0
        end = len(text)
        for j in range(brace, len(text)):
            if text[j] == '{':
                depth += 1
            elif text[j] == '}':
                depth -= 1
                if depth == 0:
                    end = j + 1
                    break
        out.append(text[i:m.start()])
        i = end
    out.append(text[i:])
    return ''.join(out)


def rust_identifiers():
    """Declared symbols only (types, fns, aliases, impl targets) --
    locals named `element` must not count as covering upstream `Element`.

    Three things are deliberately *not* counted, each because it was caught
    crediting a class nobody had written:

    * `mod`. A module is a file, not a type, and the snake-case fold that
      lets `text_theme` answer for `TextTheme` also let `mod actions` answer
      for upstream's `Actions` widget.
    * A function that is not a free public one. This crate writes some widget
      facades as functions -- `pub fn spacer() -> AnyWidget` is a real port of
      upstream's `Spacer` -- so functions have to count for something. But a
      *method* named `element`, `title` or `window` is not a port of
      `Element`, `Title` or `Window`, and nor is a private helper named
      `cubic` a port of the curve. Free and `pub` is what separates the two:
      a method is indented inside its `impl`, and a helper is not `pub`.
    * A constant or static that is not a free public one, for the same
      reason.
    * A type that is not `pub`. An upstream *public* class is API an
      application can reach; a module-private Rust type with the same name is
      not that, whatever it does inside. This rule was missing while the same
      rule for functions and constants was present, and it credited upstream's
      `FocusManager`, `FocusScope`, `State` and `RenderObjectWidget` to private
      helpers.
    * A declaration inside a `#[cfg(test)]` module -- see
      [`strip_test_modules`].

    `impl` blocks still count, and have to: this crate generates whole families
    of types from macros (the eleven `caret_movement_intent!` intents, for one),
    and the ruler does not expand macros, so a hand-written `impl` on a
    macro-made type is the only evidence of it there is. But only at the start
    of a line -- `fn build_frame(&self, root: impl Widget)` is a parameter, not
    a declaration, and it was answering for upstream's `Widget`.
    """
    # A public type, or a macro. Line-anchored so `pub` is this item's own and
    # not a word that happened to precede it.
    decl = re.compile(
        r'^\s*pub(?:\([^)]*\))?\s+(?:struct|enum|union|trait|type)\s+([A-Za-z_][A-Za-z0-9_]*)'
        r'|^\s*macro_rules!\s+([A-Za-z_][A-Za-z0-9_]*)',
        re.M,
    )
    # An impl block, which stands in for the macro-generated type it is on.
    impl = re.compile(
        r'^impl(?:\s*<[^{;]*>)?\s+'
        r'(?:[A-Za-z_][A-Za-z0-9_:<>, ]*?\s+for\s+)?'
        r'(?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*([A-Za-z_][A-Za-z0-9_]*)',
        re.M,
    )
    # A function, constant or static: only at the top of a line and only
    # public, which is what a facade looks like and a method does not.
    free = re.compile(
        r'^pub(?:\([^)]*\))?\s+(?:const\s+|async\s+|unsafe\s+|extern\s+"[^"]*"\s+)*'
        r'(?:fn|const|static)\s+([A-Za-z_][A-Za-z0-9_]*)',
        re.M,
    )
    ids = set()
    for root, _, files in os.walk(CRATE):
        for name in files:
            if not name.endswith('.rs'):
                continue
            path = os.path.join(root, name)
            text = strip_test_modules(
                strip_rust_comments(open(path, encoding='utf-8', errors='ignore').read())
            )
            for m in decl.finditer(text):
                ids.update(x for x in m.groups() if x)
            ids.update(m.group(1) for m in impl.finditer(text))
            ids.update(m.group(1) for m in free.finditer(text))
    return ids


def snake(name):
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower().replace('__', '_')


def upstream_classes():
    """{layer: {file: [class names]}} for public classes in counted files.

    Walks subdirectories. `os.listdir` was doing the walking, which meant
    `material/animated_icons/` -- `AnimatedIcon` and its two data classes --
    was never counted at all.
    """
    out = {}
    for layer in LAYERS:
        layer_dir = os.path.join(UPSTREAM, 'packages', 'flutter', 'lib', 'src', layer)
        files = {}
        for root, _, names in os.walk(layer_dir):
            for name in sorted(names):
                if not name.endswith('.dart'):
                    continue
                base = name[:-5]
                if base.startswith('_'):
                    continue
                if base.endswith('_io') or base.endswith('_web'):
                    continue  # io/web variants: not counted, see ledger notes
                path = os.path.join(root, name)
                text = open(path, encoding='utf-8', errors='ignore').read()
                classes = [c for c in CLASS_RE.findall(text) if not c.startswith('_')]
                if classes:
                    rel = os.path.relpath(path, layer_dir).replace(os.sep, '/')
                    files[rel] = classes
        out[layer] = files
    return out


def load_ledger():
    if not os.path.exists(LEDGER):
        return {'equivalent': {}, 'blocked_engine': {}, 'out_of_scope_files': {},
                'out_of_scope_classes': {}}
    return json.load(open(LEDGER, encoding='utf-8'))


def rust_module_names():
    """Every module and file name in the crate.

    Kept apart from [`rust_identifiers`], which excludes `mod` on purpose: the
    snake-case fold that lets `text_theme` answer for `TextTheme` also let `mod
    actions` answer for upstream's `Actions` widget, and a module is a file
    rather than a type.

    But that rule is about an *accident* -- a name colliding on its own. A
    ledger entry that deliberately writes `painting::matrix_utils` is a person
    saying where the port lives, and a module is a perfectly good answer to
    that question when upstream's class is a bag of static methods. Two
    different questions, so two different sets.
    """
    names = set()
    for root, _dirs, files in os.walk(CRATE):
        for name in files:
            if name.endswith('.rs'):
                names.add(name[:-3])
        names.add(os.path.basename(root))
    for root, _dirs, files in os.walk(CRATE):
        for name in files:
            if not name.endswith('.rs'):
                continue
            text = strip_rust_comments(
                open(os.path.join(root, name), encoding='utf-8', errors='ignore').read())
            names.update(re.findall(r'^\s*(?:pub\s+)?mod\s+([a-z0-9_]+)', text, re.M))
    return names


def mapping_resolves(entry, rust_ids):
    """Whether a ledger `equivalent` entry names something the crate has.

    Until this existed the `rust:` field was never read. An `equivalent` entry
    counted as accounted purely because the key was present, so 444 of the
    2102 upstream types -- a fifth of the ledger -- were resting on an
    assertion in a JSON file that nothing compared against the code. The same
    species as the invented `_kFoo` citations, at the largest scale in the
    tree: a claim of correspondence that no reader and no tool ever checked.

    What this checks is **presence, not correctness**. The entries name paths
    like `render::FlexChild::tight`, whose last segment is a struct field and
    not a declared symbol, so requiring the leaf would reject good mappings.
    Requiring *some* segment to be a declared symbol catches the failure that
    matters -- a mapping pointing at nothing at all, because the Rust side was
    renamed or never written -- and does not pretend to verify that the thing
    it points at means what the note says it means. That part is still a human
    judgment, and the note is where it lives.
    """
    if isinstance(entry, str):
        target = entry
    else:
        target = entry.get('rust', '')
    if not target:
        return False
    # The `rust:` values are prose, not strict paths -- `animation::Animation
    # trait`, `render::FlexChild::tight`, `painting::matrix_utils`. Splitting on
    # `::` was the first cut and it reported 225 of 444 mappings as naming
    # nothing, because `Animation trait` is not an identifier and `animation` is
    # a module, which `rust_identifiers` deliberately excludes. Pulling every
    # identifier-shaped token out of the string is what actually asks the
    # question: does the crate contain *anything* this entry names.
    tokens = re.findall(r'[A-Za-z_][A-Za-z0-9_]*', target)
    return any(t in rust_ids or snake(t) in rust_ids for t in tokens)


def mapping_names_anything(entry, rust_ids, module_names):
    """Whether the entry names anything, ignoring its leading path component.

    The leading component is context, not the claim. Letting it count was the
    first cut, and a mutation caught it: pointing `GestureDisposition` at
    `gestures::NoSuchTypeAnywhere` still resolved, because `gestures` is a
    module and that alone satisfied the check. Every entry in the ledger begins
    with a layer name, so the test would have passed for all of them no matter
    what followed.
    """
    target = entry if isinstance(entry, str) else entry.get('rust', '')
    tokens = re.findall(r'[A-Za-z_][A-Za-z0-9_]*', target or '')
    if len(tokens) > 1 and (tokens[0] in module_names or snake(tokens[0]) in module_names):
        tokens = tokens[1:]
    return any(t in rust_ids or snake(t) in rust_ids
               or t in module_names or snake(t) in module_names
               for t in tokens)


def classify(classes_by_file, rust_ids, ledger, module_names):
    """Yield (layer, file, class, state) rows."""
    eq = ledger.get('equivalent', {})
    blocked = ledger.get('blocked_engine', {})
    blocked_work = ledger.get('blocked_unported_dependency', {})
    oos_files = ledger.get('out_of_scope_files', {})
    oos_classes = ledger.get('out_of_scope_classes', {})
    for layer, files in classes_by_file.items():
        for fname, classes in files.items():
            file_key = f'{layer}/{fname}'
            if file_key in oos_files or fname in oos_files:
                for c in classes:
                    yield layer, fname, c, 'out-of-scope'
                continue
            for c in classes:
                if c in eq:
                    yield layer, fname, c, (
                        'mapped'
                        if mapping_names_anything(eq[c], rust_ids, module_names)
                        else 'mapping-unresolved')
                elif c in blocked_work:
                    yield layer, fname, c, 'blocked-work'
                elif c in blocked:
                    yield layer, fname, c, 'blocked-engine'
                elif c in oos_classes or f'{file_key}:{c}' in oos_classes:
                    yield layer, fname, c, 'out-of-scope'
                elif c in rust_ids or snake(c) in rust_ids:
                    yield layer, fname, c, 'covered'
                else:
                    yield layer, fname, c, 'MISSING'


# `blocked-work` is not `out-of-scope`, and the difference is a claim about
# intent rather than a shade of meaning. Out-of-scope says this port will
# never want the thing -- web-only, iOS-only, a debug channel it has no
# counterpart for. Blocked-work says it wants it and has not built what it
# stands on. Filing the second under the first would put a false statement
# in the ledger and quietly retire work that is merely not done yet.
ORDER = ['covered', 'mapped', 'blocked-engine', 'blocked-work', 'out-of-scope',
         'mapping-unresolved', 'MISSING']
# `mapping-unresolved` is not accounted. A ledger entry that names a Rust
# symbol the crate does not have is a claim, not a port.


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('filter', nargs='?', help='substring filter on layer/file')
    ap.add_argument('--missing-only', action='store_true')
    args = ap.parse_args()

    rows = list(classify(upstream_classes(), rust_identifiers(), load_ledger(),
                         rust_module_names()))
    if args.filter:
        rows = [r for r in rows if args.filter in f'{r[0]}/{r[1]}']
    if args.missing_only:
        rows = [r for r in rows if r[3] == 'MISSING']

    by_state = {s: 0 for s in ORDER}
    by_file = {}
    for layer, fname, cls, state in rows:
        by_state[state] += 1
        by_file.setdefault((layer, fname), {s: [] for s in ORDER})[state].append(cls)

    total = len(rows)
    accounted = total - by_state['MISSING'] - by_state['mapping-unresolved']
    print(f'{total} public classes across {len(by_file)} files '
          f'({accounted} accounted, {by_state["MISSING"]} MISSING)\n')
    for state in ORDER:
        print(f'  {state:<15} {by_state[state]:>5}  ({100.0 * by_state[state] / max(total, 1):.0f}%)')
    print()
    for (layer, fname), states in sorted(by_file.items()):
        counts = ' '.join(f'{s.split("-")[0]}:{len(v)}' for s, v in states.items() if v)
        print(f'{layer}/{fname}: {counts}')
        if not args.missing_only:
            for s in ('mapped', 'blocked-engine', 'blocked-work', 'out-of-scope'):
                if states[s]:
                    print(f'    [{s}] {", ".join(states[s])}')
        if states['mapping-unresolved']:
            print(f'    MAPPING NAMES NOTHING: {", ".join(states["mapping-unresolved"])}')
        if states['MISSING']:
            print(f'    MISSING: {", ".join(states["MISSING"])}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
