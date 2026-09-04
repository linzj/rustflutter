# -*- coding: utf-8 -*-
"""What the upstream engine demands of whatever sits in the AOT slot.

    python3 tools/dart2rust/bin/embedder_api.py [--top N] [--missing-only]

The goal changed on 2026-09-03: the gallery is to run on the *upstream* engine
in its AOT mode, with dart2rust's Rust where `gen_snapshot`'s `libapp.so` goes
and a hand-written Rust runtime where `libdart` goes. That goal needs a ruler
of its own. `census.dart` counts what translates; nothing counted what the
engine will ask for the moment the translation runs.

Three surfaces, all read out of the engine checkout ($RUSTFLUTTER_ENGINE):

  * `Dart_*`      the embedding API. A Rust VM standing where libdart stands
                  has to answer every one of these the engine reaches.
  * dart:ui down  the natives the app calls into the engine, from `dart_ui.cc`'s
                  FFI_FUNCTION_LIST + FFI_METHOD_LIST.
  * dart:ui up    the `PlatformConfiguration` handles the engine calls back
                  through -- begin frame, pointer packets, window metrics.

Counted at the call site, then intersected with what `dart_api.h` and its
siblings actually declare. Two reasons for the intersection: `dart_api.h`
declares far more than this engine uses, and a bare regex over call sites also
matches the API's *types* (`Dart_Handle(...)` reads like a call). The number
that matters is the one both agree on.

Excluded: `*_unittests.cc`, `testing/`, `fixtures/`, benchmarks. None of them
are in the shipped binary, and counting them would make the target larger than
it is.

The last column is the Rust side: `pub extern "C" fn Dart_*` in the runtime
crate (`tools/dart2rust/runtime/`). Until that crate exists the answer is 0 --
printed rather than skipped, because the distance is the point of the ruler.
"""
import argparse
import os
import re
import sys
from collections import Counter

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import paths  # noqa: E402

TOOL = os.path.dirname(HERE)
RUNTIME = os.path.join(TOOL, 'runtime')

#: Engine directories that end up in the shipped binary and talk to the VM.
ENGINE_DIRS = ['runtime', 'lib/ui', 'shell', 'third_party/tonic']

#: Where the C API is declared. `dart_api.h` is the bulk; the rest are the
#: message-port, DevTools and dynamically-linked surfaces the shell also calls,
#: plus `elf_loader.h` -- which is not an afterthought here, since `Dart_LoadELF`
#: is precisely how the AOT slot gets filled upstream.
API_HEADERS = [
    'third_party/dart/runtime/include/dart_api.h',
    'third_party/dart/runtime/include/dart_native_api.h',
    'third_party/dart/runtime/include/dart_tools_api.h',
    'third_party/dart/runtime/include/dart_api_dl.h',
    'third_party/dart/runtime/bin/elf_loader.h',
]

CALL_RE = re.compile(r'\b(Dart_[A-Za-z_][A-Za-z0-9_]*)\s*\(')
# A declaration's name is not always on the `DART_EXPORT` line: `Dart_SetField`
# has its return type there and its name on the next one. Reading line by line
# missed 18 of them, which is how a ruler quietly makes its target smaller.
DECL_RE = re.compile(r'DART_EXPORT[^;{]*?\b(Dart_[A-Za-z0-9_]+)\s*\(', re.S)
RUST_RE = re.compile(r'extern\s+"C"\s+fn\s+(Dart_[A-Za-z0-9_]*)')
SKIP = ('unittest', '/testing/', '/fixtures/', 'benchmark')


def engine_root():
    root = os.path.join(paths.ENGINE, 'flutter')
    if not os.path.isdir(root):
        sys.exit('no engine at %s -- set RUSTFLUTTER_ENGINE' % root)
    return root


def sources(root):
    for d in ENGINE_DIRS:
        for dirpath, _, names in os.walk(os.path.join(root, d)):
            for name in names:
                if not name.endswith(('.cc', '.h', '.mm')):
                    continue
                p = os.path.join(dirpath, name).replace('\\', '/')
                if any(s in p for s in SKIP):
                    continue
                yield p


def read(path):
    with open(path, 'r', encoding='utf-8', errors='replace') as f:
        return f.read()


def declared(root):
    """Every `Dart_*` the C API declares -- the set a VM could be asked for."""
    names = set()
    for rel in API_HEADERS:
        path = os.path.join(root, rel)
        if not os.path.exists(path):
            continue
        names.update(DECL_RE.findall(read(path)))
    return names


def called(root):
    """Every `Dart_*` the engine reaches, with how often, and from where."""
    counts = Counter()
    where = {}
    for path in sources(root):
        rel = os.path.relpath(path, root).replace('\\', '/')
        for name in CALL_RE.findall(read(path)):
            counts[name] += 1
            where.setdefault(name, set()).add(rel.split('/')[0])
    return counts, where


def implemented():
    """`Dart_*` the Rust runtime crate answers today."""
    if not os.path.isdir(RUNTIME):
        return None
    names = set()
    for dirpath, _, files in os.walk(RUNTIME):
        for name in files:
            if name.endswith('.rs'):
                names.update(RUST_RE.findall(read(os.path.join(dirpath, name))))
    return names


def dart_ui(root):
    """The two directions of dart:ui, counted where they are listed."""
    ui = read(os.path.join(root, 'lib/ui/dart_ui.cc'))
    down = 0
    for macro in ('FFI_FUNCTION_LIST', 'FFI_METHOD_LIST'):
        block = re.search(r'#define %s\(V\)(.*?)\n\n' % macro, ui, re.S)
        if block:
            down += len(re.findall(r'^\s*V\(', block.group(1), re.M))
    pc = read(os.path.join(root, 'lib/ui/window/platform_configuration.h'))
    up = len(re.findall(r'tonic::DartPersistentValue\s+(\w+);', pc))
    return down, up


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--top', type=int, default=20,
                    help='how many of the most-called to list (0 for all)')
    ap.add_argument('--missing-only', action='store_true',
                    help='only what the engine calls and the runtime lacks')
    args = ap.parse_args()

    root = engine_root()
    decls = declared(root)
    counts, where = called(root)
    api = {n: c for n, c in counts.items() if n in decls}
    have = implemented()
    missing = sorted(api, key=lambda n: -api[n])
    if have is not None:
        missing = [n for n in missing if n not in have]

    if not args.missing_only:
        print('engine   %s' % paths.ENGINE)
        print('declared %d in %d headers' % (len(decls), len(API_HEADERS)))
        print('called   %d distinct, %d call sites, in %s'
              % (len(api), sum(api.values()), ', '.join(ENGINE_DIRS)))
        print('dropped  %d identifiers that read like calls but are types'
              % (len(counts) - len(api)))
        down, up = dart_ui(root)
        print('dart:ui  %d natives down, %d handles up' % (down, up))
        print('runtime  %s'
              % ('no crate at %s yet' % os.path.relpath(RUNTIME, TOOL)
                 if have is None
                 else '%d of %d implemented' % (len(api) - len(missing),
                                                len(api))))
        print()

    listing = missing if args.missing_only else sorted(api, key=lambda n: -api[n])
    if args.top:
        listing = listing[:args.top]
    for name in listing:
        print('%5d  %-40s %s' % (api[name], name, ' '.join(sorted(where[name]))))
    if not args.missing_only and args.top and len(api) > args.top:
        print('... %d more, --top 0 for all' % (len(api) - args.top))


if __name__ == '__main__':
    main()
