# -*- coding: utf-8 -*-
"""Split the translated crate into a Cargo workspace, and check it.

    python3 tools/dart2rust/bin/workspace.py [--src .crate/src] [--out .crate-ws]

One crate is one rustc, and rustc's front end is one thread: the whole
translation as a single crate took every change to a full serial re-check.
`/tmp/rustflutter-compile-analysis.md` made the case for the hand port; the
translated crate has the same shape and a worse size.

Measured on 2026-09-03 before splitting (tree-shaken dill): 924 modules,
10,628 `crate::` edges, one strongly connected component of 450 modules and
27% of the lines -- with `dart:ui` and the gallery *both* inside it. Most of
that cycle was the text-based importer's doing (`use crate::listen::{..}` in
`dart:ui` for a word that appeared in a doc comment); bounded by the Dart
graph it fell to 230 modules, all of them widgets/material/cupertino plus
the gallery modules the tree shaker's constant propagation pulled into
`widgets/view.dart`. That cycle is real and stays one crate.

The grouping, which the module graph proves acyclic before anything is
written:

  * a strongly connected component of 50k+ lines or 20+ modules is a crate;
  * a single module of 5k+ lines is a crate (the l10n tables: one locale is
    130k lines, and 70 of them are independent leaves);
  * everything else joins the crate of its Dart layer (`flutter/painting`,
    `package:intl`), split into the part below the big component and the
    part above it, so that the layer crates cannot close a cycle through it;
  * a cycle among crates that survives all that merges the crates in it.

Paths are rewritten textually: `crate::x` becomes `<crate of x>::x` across a
boundary, `pub(crate)` becomes `pub`, and the prelude is a crate of its own.
"""
import argparse
import collections
import io
import os
import re
import shutil
import subprocess
import sys
import time

sys.setrecursionlimit(100000)
HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)


def read(path):
    return io.open(path, encoding='utf-8', errors='replace').read()


def layer_of(uri):
    m = re.match(r'(package:[^/]+)/', uri)
    p = m.group(1) if m else uri.split('/')[0]
    if p != 'package:flutter':
        return re.sub(r'[^a-z0-9_]', '_', p.replace('package:', ''))
    m = re.match(r'package:flutter/src/([a-z_]+)/', uri)
    return 'flutter_' + (m.group(1) if m else 'root')


def tarjan(nodes, deps):
    index, low, onstack, stack, comps, counter = {}, {}, set(), [], [], [0]

    def strong(v):
        index[v] = low[v] = counter[0]
        counter[0] += 1
        stack.append(v)
        onstack.add(v)
        for w in deps.get(v, ()):
            if w not in index:
                strong(w)
                low[v] = min(low[v], low[w])
            elif w in onstack:
                low[v] = min(low[v], index[w])
        if low[v] == index[v]:
            comp = []
            while True:
                w = stack.pop()
                onstack.discard(w)
                comp.append(w)
                if w == v:
                    break
            comps.append(comp)

    for v in nodes:
        if v not in index:
            strong(v)
    return comps


def plan(src, big_lines=50000, big_modules=20, leaf_lines=5000):
    mods = {}
    for f in sorted(os.listdir(src)):
        if not f.endswith('.rs') or f in ('lib.rs', 'dart_prelude.rs'):
            continue
        name = f[:-3]
        text = read(os.path.join(src, f))
        uri = text.split('\n', 1)[0].replace('// Generated from ', '')
        body = re.sub(r'//[^\n]*', '', re.sub(r'/\*.*?\*/', '', text, flags=re.S))
        edges = set(re.findall(r'\bcrate::([a-z_][a-z_0-9]*)', body))
        edges.discard(name)
        edges.discard('dart_prelude')
        mods[name] = (edges, body.count('\n'), uri)
    deps = {m: {w for w in es if w in mods} for m, (es, _, _) in mods.items()}
    comps = tarjan(list(mods), deps)
    scc_of = {m: i for i, c in enumerate(comps) for m in c}
    cdeps = collections.defaultdict(set)
    for m in mods:
        for w in deps[m]:
            if scc_of[w] != scc_of[m]:
                cdeps[scc_of[m]].add(scc_of[w])
    big = {i for i, c in enumerate(comps)
           if sum(mods[m][1] for m in c) >= big_lines or len(c) > big_modules}
    reach = {}

    def reaches(c):
        if c in reach:
            return reach[c]
        reach[c] = set()
        out = set()
        for d in cdeps.get(c, ()):
            out.add(d)
            out |= reaches(d)
        reach[c] = out
        return out

    crate_of = {}
    for i, c in enumerate(comps):
        lines = sum(mods[m][1] for m in c)
        lay = collections.Counter(layer_of(mods[m][2]) for m in c).most_common(1)[0][0]
        if i in big:
            key = 'scc_%s' % lay
        elif len(c) == 1 and lines >= leaf_lines:
            key = 'leaf_%s' % c[0]
        else:
            key = '%s_%s' % (lay, 'above' if reaches(i) & big else 'below')
        for m in c:
            crate_of[m] = key
    # Two big components in one layer would share a name; number them.
    seen = collections.defaultdict(set)
    for i, c in enumerate(comps):
        if i in big:
            seen[crate_of[c[0]]].add(i)
    for key, ids in seen.items():
        if len(ids) > 1:
            for n, i in enumerate(sorted(ids)):
                for m in comps[i]:
                    crate_of[m] = '%s_%d' % (key, n)

    def crate_graph():
        g = collections.defaultdict(set)
        for m in mods:
            for w in deps[m]:
                if crate_of[w] != crate_of[m]:
                    g[crate_of[m]].add(crate_of[w])
        return g

    while True:
        g = crate_graph()
        cycles = [c for c in tarjan(list(set(crate_of.values())), g) if len(c) > 1]
        if not cycles:
            break
        for cyc in cycles:
            target = 'merged_' + '_'.join(sorted(x.replace('flutter_', '').split('_')[0] for x in cyc))[:48]
            for m, key in crate_of.items():
                if key in cyc:
                    crate_of[m] = target
    return mods, crate_of, crate_graph()


def write_workspace(src, out, mods, crate_of, graph):
    if os.path.isdir(out):
        for entry in os.listdir(out):
            if entry != 'target':
                p = os.path.join(out, entry)
                shutil.rmtree(p) if os.path.isdir(p) else os.remove(p)
    os.makedirs(out, exist_ok=True)
    members = sorted(set(crate_of.values())) + ['dart_prelude']
    io.open(os.path.join(out, 'Cargo.toml'), 'w', encoding='utf-8').write(
        '[workspace]\nresolver = "2"\nmembers = [\n%s]\n\n[profile.dev]\ndebug = false\n'
        % ''.join('    "%s",\n' % m for m in members))
    # the prelude crate
    pd = os.path.join(out, 'dart_prelude', 'src')
    os.makedirs(pd, exist_ok=True)
    prelude = read(os.path.join(src, 'dart_prelude.rs')).replace('pub(crate) ', 'pub ')
    prelude = prelude.replace('macro_rules! dart_error', '#[macro_export]\nmacro_rules! dart_error')
    io.open(os.path.join(pd, 'lib.rs'), 'w', encoding='utf-8').write(prelude)
    io.open(os.path.join(out, 'dart_prelude', 'Cargo.toml'), 'w', encoding='utf-8').write(
        '[package]\nname = "dart_prelude"\nversion = "0.0.0"\nedition = "2021"\nautobins = false\n\n[lib]\npath = "src/lib.rs"\n')
    by_crate = collections.defaultdict(list)
    for m, key in crate_of.items():
        by_crate[key].append(m)
    path_re = re.compile(r'\bcrate::([a-z_][a-z_0-9]*)')
    for key, names in by_crate.items():
        d = os.path.join(out, key, 'src')
        os.makedirs(d, exist_ok=True)
        deps = sorted(graph.get(key, ()))
        io.open(os.path.join(out, key, 'Cargo.toml'), 'w', encoding='utf-8').write(
            '[package]\nname = "%s"\nversion = "0.0.0"\nedition = "2021"\nautobins = false\n\n[lib]\npath = "src/lib.rs"\n\n'
            '[dependencies]\ndart_prelude = { path = "../dart_prelude" }\n%s'
            % (key, ''.join('%s = { path = "../%s" }\n' % (dep, dep) for dep in deps)))
        io.open(os.path.join(d, 'lib.rs'), 'w', encoding='utf-8').write(
            '#![allow(warnings)]\n' + ''.join('pub mod %s;\n' % m for m in sorted(names)))
        for m in names:
            text = read(os.path.join(src, m + '.rs'))
            text = text.replace('use crate::dart_prelude::*;', 'use dart_prelude::*;')
            text = text.replace('pub(crate) ', 'pub ')

            def rewrite(match):
                target = match.group(1)
                owner = crate_of.get(target)
                if owner is None or owner == key:
                    return match.group(0)
                return '%s::%s' % (owner, target)

            text = path_re.sub(rewrite, text)
            io.open(os.path.join(d, m + '.rs'), 'w', encoding='utf-8').write(text)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--src', default=os.path.join(TOOL, '.crate', 'src'))
    ap.add_argument('--out', default=os.path.join(TOOL, '.crate-ws'))
    ap.add_argument('--no-check', action='store_true')
    ap.add_argument('--fresh', action='store_true', help='throw the target dir away first')
    args = ap.parse_args()

    mods, crate_of, graph = plan(args.src)
    sizes = collections.Counter()
    files = collections.Counter()
    for m, (_, lines, _) in mods.items():
        sizes[crate_of[m]] += lines
        files[crate_of[m]] += 1
    print('%d modules, %d lines -> %d crates; largest:' % (len(mods), sum(sizes.values()), len(sizes)))
    for key, lines in sizes.most_common(6):
        print('  %-36s %8d lines %4d files  deps %d' % (key, lines, files[key], len(graph.get(key, ()))))
    if args.fresh and os.path.isdir(os.path.join(args.out, 'target')):
        shutil.rmtree(os.path.join(args.out, 'target'))
    write_workspace(args.src, args.out, mods, crate_of, graph)
    print('wrote', args.out)
    if args.no_check:
        return
    started = time.time()
    r = subprocess.run(['cargo', 'check', '--workspace', '--keep-going', '--message-format=short'],
                       cwd=args.out, capture_output=True, text=True, errors='replace')
    elapsed = time.time() - started
    errors = collections.Counter()
    codes = collections.Counter()
    for line in r.stderr.splitlines():
        m = re.match(r'([^ :]+):\d+:\d+: error(\[(E\d+)\])?', line)
        if m:
            crate = m.group(1).split('/')[0]
            errors[crate] += 1
            codes[m.group(3) or '(no code)'] += 1
    print('cargo check --workspace: %.0fs, %d errors' % (elapsed, sum(errors.values())))
    for code, n in codes.most_common(8):
        print('  %6d  %s' % (n, code))
    print('  by crate:')
    for crate, n in errors.most_common(8):
        print('  %6d  %s' % (n, crate))


if __name__ == '__main__':
    main()
