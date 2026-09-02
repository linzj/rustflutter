# -*- coding: utf-8 -*-
"""Break the compiler on purpose, and check something goes red.

    python tools/dart2rust/bin/mutate.py sweep.json

A mutation that nothing notices is a claim nothing measures. This runs them,
but only over the fixtures a mutation can reach: a sweep used to re-run the
whole gate -- every fixture regenerated, the whole crate tested, every pair of
front ends compared -- once per mutation, which was most of a round's wall
clock for a check that only ever looks at two or three files.

A sweep file is a JSON list of objects:

    [{"name": "...", "file": "lib/backend_rust.dart",
      "from": "...", "to": "...", "fixtures": ["lists"]}]

`fixtures` names what the mutation can reach. Leave it out only when the change
really is global; then the whole gate runs, slowly, and on purpose.

Two rules the sweep enforces rather than trusts:

* **The mutated compiler still has to build.** A mutation that stops the Dart
  compiling proves nothing, and three times in this project a "kill" was really
  that. Reported as INVALID, not as a kill.
* **A hang is a kill.** One mutation turned a `continue` into an infinite loop;
  `cargo test` never returned. Saying so beats waiting.
"""
import io
import json
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
REPO = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))


def run(command, cwd=REPO, timeout=None):
    return subprocess.run(command, cwd=cwd, capture_output=True, text=True,
                          errors='replace', timeout=timeout)


def check(fixtures):
    names = list(fixtures or [])
    r = run([sys.executable, HERE + '/regen.py'] + names)
    lines = [l for l in r.stdout.splitlines()
             if l.strip() and l[1:3] != ':\\' and not l.startswith('E:')]
    bad = [l for l in lines if not l.rstrip().endswith(('ok', 'ok (kernel)'))]
    if bad:
        # The compiler itself failing to build is not a kill.
        if any('Error:' in l for l in bad):
            return 'INVALID', 'the mutated compiler does not build'
        return 'red', 'regen: ' + bad[0][:100]
    try:
        r = run(['cargo', 'test'], cwd=TOOL + '/testdata', timeout=240)
    except subprocess.TimeoutExpired:
        return 'red', 'cargo: a test never finished'
    if r.returncode != 0:
        out = (r.stdout + r.stderr).splitlines()
        first = [l for l in out if l.startswith('error') or 'panicked' in l
                 or l.startswith('---- ')]
        return 'red', 'cargo: ' + (first[0] if first else 'failed')
    r = run([sys.executable, HERE + '/fixtures.py'] + names)
    if 'every fixture translates the same' not in r.stdout:
        bad = [l for l in r.stdout.splitlines()
               if l.startswith('differ or failed:')]
        return 'red', 'fixtures: ' + (bad[0].strip()[:110] if bad
                                      else 'red elsewhere')
    return 'green', None


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    sweep = json.load(io.open(sys.argv[1], encoding='utf-8'))
    every = sorted({f for m in sweep for f in m.get('fixtures', [])})

    print('baseline...')
    state, why = check(every)
    if state != 'green':
        raise SystemExit('baseline is not green: %s' % why)
    print('  green\n')

    killed = 0
    for mutation in sweep:
        path = os.path.join(TOOL, mutation['file'])
        s = io.open(path, encoding='utf-8').read()
        if s.count(mutation['from']) != 1:
            print('SKIPPED (%d matches)  %s'
                  % (s.count(mutation['from']), mutation['name']))
            continue
        io.open(path, 'w', encoding='utf-8', newline='\n').write(
            s.replace(mutation['from'], mutation['to']))
        try:
            state, why = check(mutation.get('fixtures'))
        finally:
            io.open(path, 'w', encoding='utf-8', newline='\n').write(s)
        label = {'red': 'KILLED', 'green': 'SURVIVED',
                 'INVALID': 'INVALID'}[state]
        if state == 'red':
            killed += 1
        print('%-9s %s%s' % (label, mutation['name'],
                             '\n          ' + why if why else ''))

    print('\nrestoring...')
    state, why = check(every)
    print('  %s' % (why or 'green'))
    return 0 if state == 'green' else 1


if __name__ == '__main__':
    sys.exit(main())
