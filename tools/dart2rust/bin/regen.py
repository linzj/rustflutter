# -*- coding: utf-8 -*-
"""Regenerate every `testdata/src/*.rs` that has a fixture behind it.

    python tools/dart2rust/bin/regen.py           # all of them
    python tools/dart2rust/bin/regen.py loops     # one, by name

Two things this does that doing it by hand kept getting wrong:

* **Which front end.** Almost every file comes from the analyzer one, because
  that is what `dart2rust.dart` runs. `constinstance.rs` comes from the Kernel
  one -- the analyzer never meets an evaluated constant, so testing that work
  means testing Kernel's output. Forgetting which is which silently replaces a
  file with the other side's version.
* **The `use` line.** A generated file names `RangeError`, which lives in
  `lib.rs`, and nothing in the generator knows that. Regenerating by hand
  dropped the import and the crate stopped building. Decided by looking at the
  text rather than by remembering, so a new file gets it too.

Runs the fixtures at the same time: one Dart VM start each, serially, was most
of the wall clock.
"""
import argparse
import io
import os
import re
import subprocess
import sys
from concurrent import futures

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
REPO = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
FIXTURES = os.path.join(TOOL, 'testdata', 'fixtures')
SRC = os.path.join(TOOL, 'testdata', 'src')

sys.path.insert(0, HERE)
from paths import DART_EXPERIMENTS, FLUTTER_DART, FLUTTER_PKGS  # noqa: E402

import dill as dill_tool  # noqa: E402
import fixtures as fixtures_tool  # noqa: E402

# Files whose Rust must come from the Kernel front end.
FROM_KERNEL = {'constinstance'}

# Notes to put above a generated file. Written here rather than carried over
# from the previous version of the file: the generator writes comments of its
# own -- the refusal notices -- and a rule that kept "the comments at the top"
# copied those forward too, once per regeneration.
NOTES = {
    'constinstance': '''// Generated from fixtures/constinstance.dart by the **Kernel** front end.
//
// Every other file here comes from the analyzer one, because that is what
// dart2rust.dart runs. That round's work is Kernel-only -- the analyzer never
// meets an evaluated constant -- so testing it means testing that side's
// output. fixtures.py still checks the two agree on everything but the
// constants themselves, which the fixture declares with // DIFFERS:.

''',
}

# What a generated file may need from the crate root, and how to tell.
IMPORTS = {'RangeError': 'use crate::RangeError;',
           'Isolate': 'use crate::Isolate;',
           'DartAny': 'use crate::DartAny;',
           'Type': 'use crate::Type;',
           'Map': 'use crate::Map;',
           'Set': 'use crate::Set;',
           'Object': 'use crate::dart_prelude::Object;',
           'StackTrace': 'use crate::StackTrace;',
           'dart_iter': 'use crate::dart_iter;',
           'dart_str': 'use crate::dart_str;'}


def needed_imports(text):
    # The kernel front end's output names prelude traits (`DartEq`,
    # `DartString`, ..) by their bare names, as the workspace modules do
    # behind `use crate::dart_prelude::*`; the same glob here, once.
    glob = 'use crate::dart_prelude::*;'
    lines = [] if glob in text else [glob]
    for name, line in sorted(IMPORTS.items()):
        if name in text and line not in text:
            lines.append(line)
    return lines


def from_analyzer(fixture, out):
    r = subprocess.run(
        [FLUTTER_DART, 'run', *DART_EXPERIMENTS, FLUTTER_PKGS,
         os.path.join(HERE, 'dart2rust.dart'), fixture, '--all', '-o', out],
        cwd=REPO, capture_output=True, text=True, errors='replace')
    return r.returncode == 0, (r.stdout or '') + (r.stderr or '')


def write_prelude(dest):
    """The fixture crate gets the same prelude the package crate does.

    It was copied by hand before -- `Isolate`, `Completer` and `RangeError`
    each written twice -- which is a second source of truth for exactly the
    thing a fixture exists to hold still.
    """
    source = io.open(os.path.join(TOOL, 'lib', 'prelude.dart'),
                     encoding='utf-8').read()
    opening = "const rustPrelude = r" + "'''"
    closing = "'''" + ";"
    start = source.index(opening) + len(opening)
    end = source.index(closing, start)
    text = source[start:end].lstrip('\n')
    out = os.path.join(dest, 'dart_prelude.rs')
    if not os.path.exists(out) or io.open(out, encoding='utf-8').read() != text:
        io.open(out, 'w', encoding='utf-8', newline='\n').write(text)
        # The hook checks this crate with rustfmt. `prelude.dart` is written
        # for a reader rather than for rustfmt, so the copy is formatted here
        # instead of the source being bent to match.
        subprocess.run(['rustfmt', '--edition', '2021', out],
                       capture_output=True)


# The gate's own ruler. It is not in `testdata/fixtures/` on purpose: what a
# gate measures must not be able to move the gate. A fixture is free to change
# shape -- that is what fixtures are for -- and on the day one of them stopped
# throwing, a check that read it would go on passing while proving nothing.
#
# `doubled` calls `checked`, `checked` throws, and neither line says so: that
# a call fails is computed, never written. Under the Result model the call in
# `doubled` must come out carrying `?`.
PROBE = '''class Probe {
  const Probe(this.limit);

  final double limit;

  double checked(double value) {
    if (value > limit) {
      throw RangeError('over the limit');
    }
    return value;
  }

  double doubled(double value) {
    return checked(value) * 2.0;
  }
}
'''

PROPAGATED = re.compile(r'checked\([^()]*\)\s*\?')
CALLED = re.compile(r'\.checked\(')


def propagates(scratch, config, drivers):
    """Whether the drivers about to write golden files can emit `?`.

    A golden file was accepted by *counting* it, and nothing was reading it:
    the fixtures regenerated, `rustfmt` was happy, the count of compiler errors
    went down, and 6,300 lines went in. On 2026-09-09 that let through a
    regeneration in which every method returned `Result` and no call
    propagated -- `Ok((self.doubled() + 1.0))`, adding a `Result<f64>` to a
    float. Fewer errors than the file it replaced, and further from correct.

    It is not a fixture bug and not a backend bug. It is a *configuration* one,
    and it is structural: `_resultModel` is `true`, so every method returns
    `Result`, while `_fails` opens with `if (throws == null) return false` and
    no driver here hands it a `ThrowsAnalysis`. Output from these drivers
    therefore cannot compile, whatever the fixture says, and regenerating
    cannot be the first move: the driver has to be able to propagate first, or
    the fresh golden re-embeds the same contradiction.

    Two weaker checks were tried before this one, and both were passed by
    output that was wrong:

    * Reading the *symptom* -- scanning the written files for a call with no
      `?` -- saw 3 of the 32 and let 29 through, because a fixture with no call
      between its own methods shows nothing while being just as wrong.
    * Reading the *source* -- `'fails:' not in frontend.dart` -- is a
      substring, and substrings do not know what a program does. A line of the
      form ``// TODO: pass `fails:` here``, which is the first thing anyone
      writes on the round that fixes this, opens it. Measured: it does.

    So the driver is asked by being run. Whatever the front ends are made of,
    a translation of `PROBE` that carries no `?` cannot compile, and that is
    the whole claim being made.
    """
    work = os.path.join(scratch, 'probe')
    os.makedirs(work, exist_ok=True)
    fixture = os.path.join(work, 'probe.dart')
    io.open(fixture, 'w', encoding='utf-8', newline='\n').write(PROBE)

    blocked = []
    for driver in drivers:
        out = os.path.join(work, driver + '.rs')
        if driver == 'kernel':
            dill = fixtures_tool.build_dill(fixture, work)
            if dill is None:
                blocked.append('kernel: the probe did not compile to a dill')
                continue
            ok, log = fixtures_tool.from_kernel(dill, fixture, out, config)
        else:
            ok, log = from_analyzer(fixture, out)
        if not ok:
            first = (log.strip().splitlines() or [''])[0]
            blocked.append('%s: the driver failed on the probe -- %s'
                           % (driver, first))
        else:
            text = io.open(out, encoding='utf-8').read()
            if not CALLED.search(text):
                # Refused, or lowered to something else: reporting "no `?` on
                # the call" would send the reader looking for a `?` in a file
                # that has no call in it. Asked as `.checked(`, because the
                # declaration `pub fn checked(` is in every version of this
                # output and matching it hides exactly this case.
                blocked.append('%s: the probe translated, but `doubled` never '
                               'calls `checked` -- refused, or lowered to '
                               'something else (%s)' % (driver, out))
            elif not PROPAGATED.search(text):
                blocked.append('%s: `doubled` calls `checked`, which throws, '
                               'and the call comes out with no `?` (%s)'
                               % (driver, out))
    return blocked


# Where propagation is decided, for the reader of a failing probe.
#
# This replaced a `hints()` that read the source and reported on it --
# `'fails:' not in frontend.dart`, `'throws:' not in dart2rust_kernel.dart` --
# printed under the probe and explicitly subordinate to it. Subordinate was not
# enough. ws889 fixed the propagation by *deleting* the `throws` parameter,
# because it was a switch wearing an analysis's name that no driver should ever
# have passed; from that day the second hint was true forever and named the one
# fix that must never be made. A fifth review caught it before it misled
# anyone. So this points at where the decision lives and claims nothing about
# what is there: the failure mode of naming a function is that the name goes
# stale and a grep finds it, which is not the failure mode of asserting a fact
# about a file.
DECIDED_IN = (
    'propagation is decided by `_fails` in lib/frontend_kernel.dart and '
    '`_callFails` in lib/frontend.dart; the half they share is '
    '`translatedLibrary` in lib/ir.dart'
)


def regenerate(stem, config, work, dest):
    fixture = os.path.join(FIXTURES, stem + '.dart')
    out = os.path.join(dest, stem + '.rs')
    header = NOTES.get(stem, '')
    if stem in FROM_KERNEL:
        holder = os.path.join(work, stem)
        os.makedirs(holder, exist_ok=True)
        dill_path = fixtures_tool.build_dill(fixture, holder)
        if dill_path is None:
            return stem, 'DILL FAILED'
        ok, log = fixtures_tool.from_kernel(dill_path, fixture, out, config)
    else:
        ok, log = from_analyzer(fixture, out)
    if not ok:
        return stem, log.strip().splitlines()[:1]

    text = io.open(out, encoding='utf-8').read()
    imports = needed_imports(header + text)
    prefix = ('\n'.join(imports) + '\n\n') if imports else ''
    io.open(out, 'w', encoding='utf-8', newline='\n').write(
        prefix + header + text)
    subprocess.run(['rustfmt', '--edition', '2021', out], capture_output=True)
    return stem, 'ok%s' % (' (kernel)' if stem in FROM_KERNEL else '')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('names', nargs='*', help='fixture names; default all')
    parser.add_argument('--anyway', action='store_true',
                        help='regenerate a blocked run into .agree/anyway/ to '
                             'look at it -- which is all this was ever for, '
                             'and now all it can do')
    args = parser.parse_args()

    stems = sorted(
        f[:-5] for f in os.listdir(FIXTURES)
        if f.endswith('.dart')
        and os.path.exists(os.path.join(SRC, f[:-5] + '.rs')))
    if args.names:
        stems = [s for s in stems if s in args.names]
    if not stems:
        raise SystemExit('nothing to regenerate')

    scratch = os.path.join(TOOL, '.agree')
    os.makedirs(scratch, exist_ok=True)
    config = os.path.join(scratch, 'kernel_package_config.json')
    if not os.path.exists(config):
        dill_tool.write_config(config, TOOL)

    # Only the drivers this run would actually write with: the Kernel probe
    # costs a dill build, and a `regen.py loops` does not use that side.
    drivers = ([] if all(s in FROM_KERNEL for s in stems) else ['analyzer'])
    drivers += (['kernel'] if any(s in FROM_KERNEL for s in stems) else [])

    dest = SRC
    blocked = propagates(scratch, config, drivers)
    if blocked:
        print('these golden files would be written by a driver that cannot '
              'emit the `?` the Result model needs, so they could not compile '
              'whatever the fixtures say:')
        for line in blocked:
            print('  ' + line)
        print('  where to look: ' + DECIDED_IN)
        if not args.anyway:
            print('`propagates` in this file says what has to be true first. '
                  'To look at the output without it reaching testdata/src: '
                  '--anyway')
            return 1
        # `.agree/` is not in git, so output from a run that has just been
        # told it is wrong cannot become a commit by being forgotten about.
        # The flag said "not for committing it" from the day it was written;
        # it wrote to `testdata/src` anyway, which is the one path a commit
        # picks up.
        dest = os.path.join(scratch, 'anyway')
        os.makedirs(dest, exist_ok=True)
        print('--anyway: writing to %s, not testdata/src.' % dest)

    write_prelude(dest)

    failed = []
    workers = min(len(stems), 16)
    with futures.ThreadPoolExecutor(max_workers=workers) as pool:
        for stem, status in pool.map(
                lambda s: regenerate(s, config, scratch, dest), stems):
            print('%-14s %s' % (stem, status))
            if status != 'ok' and status != 'ok (kernel)':
                failed.append(stem)
    return 1 if failed else 0


if __name__ == '__main__':
    sys.exit(main())
