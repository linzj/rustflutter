# -*- coding: utf-8 -*-
"""Compile `bin/vtable_probe.rs` against the real prelude, plus a control.

    python3 tools/dart2rust/bin/vtable_probe.py

Step 0 of the "Object protocol: registry -> vtable" plan. Changes nothing
and builds nothing the chain uses: it copies `.crate-ws/dart_prelude/src/
lib.rs`, appends the probe, and asks `rustc`.

Prints PASS only when *both* halves hold: the probe compiles, and the
control -- the same file with `DartAny` dropped from the probe trait's
supertraits -- fails. A probe that compiles for some other reason would
prove nothing about the vtable, and the control is what rules that out.
"""
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
PRELUDE = os.path.join(TOOL, '.crate-ws', 'dart_prelude', 'src', 'lib.rs')
PROBE = os.path.join(HERE, 'vtable_probe.rs')
SUPERTRAIT = 'pub trait ProbeKey: DartAny + std::fmt::Debug {}'
CONTROL = 'pub trait ProbeKey: std::fmt::Debug {}'


def compile_source(text, out):
    path = os.path.join(out, 'probe.rs')
    with open(path, 'w', encoding='utf-8') as f:
        f.write(text)
    env = dict(os.environ, RUSTC_BOOTSTRAP='1')
    done = subprocess.run(
        ['rustc', '--edition', '2021', '--crate-type', 'lib',
         '-C', 'debuginfo=0', '--out-dir', out, path],
        env=env, capture_output=True, text=True)
    errors = [l for l in done.stderr.splitlines() if l.startswith('error')]
    return done.returncode, errors


def main():
    if not os.path.exists(PRELUDE):
        print('no prelude at', PRELUDE, '-- run bin/run_chain.sh first')
        return 1
    base = open(PRELUDE, encoding='utf-8').read()
    probe = open(PROBE, encoding='utf-8').read()
    out = tempfile.mkdtemp(prefix='vtable-probe-')
    try:
        code, errors = compile_source(base + '\n' + probe, out)
        if code != 0:
            print('PROBE FAILED -- the vtable route is not reachable:')
            for e in errors[:10]:
                print('   ', e)
            return 1
        print('probe: compiles (a `&dyn Trait` answers the protocol from its '
              'own vtable; `Rc<dyn Trait>` upcasts to `Rc<dyn Object>`)')
        code, errors = compile_source(
            base + '\n' + probe.replace(SUPERTRAIT, CONTROL), out)
        if code == 0:
            print('CONTROL FAILED -- it compiles without the `DartAny` '
                  'supertrait too, so the probe proves nothing about why')
            return 1
        print('control: fails as it must, with %d error(s):' % len(errors))
        for e in sorted(set(errors)):
            print('   ', e)
        print('PASS')
        return 0
    finally:
        shutil.rmtree(out, ignore_errors=True)


if __name__ == '__main__':
    sys.exit(main())
