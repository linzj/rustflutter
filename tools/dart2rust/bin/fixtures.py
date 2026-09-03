# -*- coding: utf-8 -*-
"""Run every fixture through **both** front ends and diff the results.

`agree.py` compares the two front ends on one upstream library. That was enough
while they saw the same shapes, and round sixteen showed they do not: Kernel
carries `EqualsNull`, `Let`, `BlockExpression` and evaluated enum constants that
the analyzer front end never meets. A bug in one of those can survive any number
of rounds, because nothing exercises them -- and one did, for two rounds, until
a census happened to look.

So the fixtures get compiled to `.dill` too. Each one is translated by the
analyzer front end from its source and by the Kernel front end from the dill,
and the two outputs are compared. A fixture needs no `main`, so a generated
wrapper imports it and supplies one; frontend_server does not tree-shake, so
importing is enough to put the library in the dill.

    python tools/dart2rust/bin/fixtures.py            # all fixtures
    python tools/dart2rust/bin/fixtures.py enums      # one, by name
    python tools/dart2rust/bin/fixtures.py --keep     # leave the dills

The dills are large (about 10 MB each: they carry dart:core) and go to a
scratch directory, not the repo.
"""
import argparse
import difflib
from concurrent import futures
import io
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
REPO = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
FIXTURES = os.path.join(TOOL, 'testdata', 'fixtures')

sys.path.insert(0, HERE)
from paths import (  # noqa: E402
    APP_PACKAGES,
    FLUTTER_DART,
    FLUTTER_PKGS,
)

sys.path.insert(0, HERE)
import dill as dill_tool  # noqa: E402


def run(command, cwd=REPO):
    return subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                          errors='replace')


def as_uri(path):
    return 'file:///' + os.path.abspath(path).replace('\\', '/')


def build_dill(fixture, work):
    """A dill holding this fixture's library.

    The wrapper exists because frontend_server needs an entry point with a
    `main`, and a fixture should not grow one just to be compiled -- it would
    then be part of what is translated.
    """
    wrapper = os.path.join(work, 'entry.dart')
    io.open(wrapper, 'w', encoding='utf-8', newline='\n').write(
        "import '%s';\nvoid main() {}\n" % as_uri(fixture))
    out = os.path.join(work, 'fixture.dill')
    code = dill_tool.build(as_uri(wrapper), APP_PACKAGES, out)
    return out if code == 0 and os.path.exists(out) else None


def from_analyzer(fixture, out):
    r = run([FLUTTER_DART, 'run', FLUTTER_PKGS,
             'tools/dart2rust/bin/dart2rust.dart', fixture, '--all', '-o', out])
    return r.returncode == 0, (r.stderr or '')


def from_kernel(dill_path, fixture, out, config):
    paths = dill_tool.paths()
    r = run([paths['dart'], 'run', '--packages=' + config,
             'tools/dart2rust/bin/dart2rust_kernel.dart', dill_path,
             as_uri(fixture), '-o', out])
    return r.returncode == 0, (r.stderr or '')


def expected_difference(fixture):
    """A fixture may declare that the two front ends *should* differ.

    A tool that always reports failures stops being read. Two of the differences
    here are properties of Kernel rather than bugs -- it folds constants, and it
    lowers `??` to a `Let` this compiler does not translate yet -- so the reason
    lives at the top of the fixture that shows it, where whoever changes that
    fixture will see it.
    """
    for line in io.open(fixture, encoding='utf-8'):
        if line.startswith('// DIFFERS:'):
            return line[len('// DIFFERS:'):].strip()
        if not line.startswith('//') and line.strip():
            return None
    return None


def expected_refusals(fixture):
    """What this fixture declares it must *not* translate.

    Refusals were invisible to this tool: `code_lines` drops comments, and a
    refusal is a comment. So deleting a refusal rule -- the rule that keeps a
    silently-wrong translation from being emitted -- changed nothing any check
    could see. A fixture now says so itself:

        // REFUSES: return inside a try body

    and the line must appear in both front ends' output.
    """
    wanted = []
    for line in io.open(fixture, encoding='utf-8'):
        if line.startswith('// REFUSES:'):
            wanted.append(line[len('// REFUSES:'):].strip())
        elif not line.startswith('//') and line.strip():
            break
    return wanted


def refusals(path):
    """The refusal lines, *and* the reason lines under them.

    A refusal is written as two lines -- the member on one and why on the next
    -- and looking only at the first meant a declared reason never matched.
    `//   ` is the continuation; `///` is a doc comment carried over from the
    fixture, which must not count.
    """
    out = []
    for line in io.open(path, encoding='utf-8'):
        text = line.strip()
        if text.startswith('// NOT TRANSLATED:') or (
                text.startswith('//') and not text.startswith('///')
                and line.startswith(('    //  ', '//  '))):
            out.append(text)
    return out


def missing_refusals(fixture, out):
    found = refusals(out)
    return [w for w in expected_refusals(fixture)
            if not any(w in line for line in found)]


def code_lines(path):
    """Comparable lines: comments dropped, since only one side carries docs."""
    out = []
    for line in io.open(path, encoding='utf-8'):
        stripped = line.strip()
        if not stripped or stripped.startswith('//'):
            continue
        out.append(stripped)
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('names', nargs='*', help='fixture names; default all')
    parser.add_argument('--config', help='kernel package config; made if absent')
    parser.add_argument('--keep', action='store_true',
                        help='leave the dills and outputs in place')
    args = parser.parse_args()

    scratch = os.path.join(TOOL, '.agree')
    os.makedirs(scratch, exist_ok=True)
    config = args.config or os.path.join(scratch, 'kernel_package_config.json')
    if not os.path.exists(config):
        dill_tool.write_config(config, TOOL)

    wanted = set(args.names)
    fixtures = sorted(f for f in os.listdir(FIXTURES) if f.endswith('.dart'))
    if wanted:
        fixtures = [f for f in fixtures if f[:-5] in wanted]
    if not fixtures:
        raise SystemExit('no fixtures matched')

    work = args.keep and scratch or tempfile.mkdtemp(prefix='d2r_fx_')

    def examine(name):
        """One fixture, start to finish. Returns (stem, lines, ok).

        Nothing here touches anything another fixture touches -- each gets its
        own directory -- so they run at the same time. Serially this was 21
        Dart VM starts and 21 ten-megabyte dills one after another, which was
        most of the round's wall clock.
        """
        lines = []
        fixture = os.path.join(FIXTURES, name)
        stem = name[:-5]
        holder = os.path.join(work, stem)
        os.makedirs(holder, exist_ok=True)

        dill_path = build_dill(fixture, holder)
        if dill_path is None:
            return stem, ['%-12s DILL FAILED' % stem], False

        a_out = os.path.join(holder, 'analyzer.rs')
        k_out = os.path.join(holder, 'kernel.rs')
        a_ok, a_log = from_analyzer(fixture, a_out)
        k_ok, k_log = from_kernel(dill_path, fixture, k_out, config)
        if not a_ok or not k_ok:
            lines.append('%-12s FRONT END FAILED (analyzer=%s kernel=%s)'
                         % (stem, a_ok, k_ok))
            for line in (a_log if not a_ok else k_log).strip().splitlines()[:4]:
                lines.append('              ' + line)
            return stem, lines, False

        absent = (missing_refusals(fixture, a_out)
                  + missing_refusals(fixture, k_out))
        if absent:
            return stem, ['%-12s TRANSLATED WHAT IT DECLARES IT REFUSES: %s'
                          % (stem, '; '.join(sorted(set(absent))))], False

        a, k = code_lines(a_out), code_lines(k_out)
        diff = [l for l in difflib.unified_diff(a, k, lineterm='', n=0)
                if l.startswith(('+', '-'))
                and not l.startswith(('+++', '---'))]
        expected = expected_difference(fixture)
        ok = True
        if not diff:
            status = 'same'
            if expected is not None:
                # The fixture says they should differ and they do not. Either
                # the difference was fixed and this note is stale, or the
                # fixture stopped exercising what it was for.
                status = 'SAME, but the fixture expects a difference'
                ok = False
        elif expected is not None:
            status = '%d lines differ, expected: %s' % (len(diff), expected)
        else:
            status = '%d lines differ' % len(diff)
            ok = False
        lines.append('%-12s analyzer %3d lines, kernel %3d lines, %s'
                     % (stem, len(a), len(k), status))
        if expected is None:
            for line in diff[:8]:
                lines.append('                ' + line[:110])
        return stem, lines, ok

    disagreed = []
    workers = min(len(fixtures), 16)
    with futures.ThreadPoolExecutor(max_workers=workers) as pool:
        # Reported in fixture order however they finish, so a run is comparable
        # with the one before it.
        for stem, lines, ok in pool.map(examine, fixtures):
            for line in lines:
                print(line)
            if not ok:
                disagreed.append(stem)

    print()
    if disagreed:
        print('differ or failed: %s' % ', '.join(disagreed))
    else:
        print('every fixture translates the same through both front ends')
    if args.keep:
        print('kept in', work)
    return 1 if disagreed else 0


if __name__ == '__main__':
    sys.exit(main())
