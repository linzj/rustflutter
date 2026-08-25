"""A screen: which early-return guards can be deleted with the suite still green?

Written after the same finding turned up four times in five ticks. Tick 185's
`hide` had a guard that could not be observed; so did tick 187's
`traversal_children`, tick 188's `handleEventLoopCallback` head check, and both
of tick 189's magnifier guards. Every one was found because I happened to
choose it as a hand-written mutation. The ones I did not choose are still
there.

This chooses all of them, for one file at a time. For each guard of the shape

    if <condition> {
        return <expr>;
    }

it deletes the three lines, runs the crate's unit tests, and restores the file.
A guard whose deletion leaves the suite green is a **candidate**, not a defect.

The count is not a target
-------------------------
Three quite different things come back green here and only one of them is
worth acting on.

* The guard is genuinely redundant -- something further down, or one call
  deeper, already stops there. That is the finding, and the fix is to delete
  the guard and leave the reason in a comment rather than a line that reads
  like a rule and decides nothing.
* The guard is load-bearing but nothing exercises it. That is a missing test,
  not a redundant guard, and deleting the guard would be exactly wrong.
* The guard defends against something the type system already prevents in
  every reachable state. Harmless either way.

Only reading tells them apart, so this prints candidates and stops. A tool
that decided for me would be wrong a third of the time, and the third it was
wrong about is the one I would have acted on.

What it does NOT see
--------------------
* Guards whose condition spans more than one line, and `let ... else`.
* Guards whose body is more than a single `return` / `continue` / `break`.
* Anything inside `#[cfg(test)]`, which is not the code under test.
* A deletion that stops compiling is reported apart, not counted: borrow
  checking, moved values and unreachable-code warnings-as-errors all land
  there, and "it would not build" is not the same answer as "it changes
  nothing".

  python tools/idle_guards.py <file.rs> [more.rs ...]
  python tools/idle_guards.py --changed      # files touched by HEAD
"""
import io
import os
import re
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CRATE = os.path.join(REPO, 'src', 'flutter', 'rust', 'rustflutter')
MSVC = (r'C:\Program Files\Microsoft Visual Studio\2022\Community'
        r'\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64')

GUARD = re.compile(r'^(\s*)if (?!let\b)(.+) \{$')
JUMP = re.compile(r'^\s*(return\b.*|continue|break)\s*;$')


def guards(lines):
    """Yield (index, indent, condition) for every three-line early return."""
    for i in range(len(lines) - 2):
        match = GUARD.match(lines[i])
        if not match:
            continue
        indent, condition = match.group(1), match.group(2)
        if not JUMP.match(lines[i + 1]):
            continue
        if lines[i + 2] != indent + '}':
            continue
        yield i, indent, condition


def test_module_ranges(lines):
    """Line indices inside `#[cfg(test)]`, roughly: from the attribute to EOF.

    Every test module in this crate is the tail of its file, so the first
    `#[cfg(test)]` is enough. A file that ever grows a test module in the
    middle will read as having fewer guards, which is the safe direction: it
    under-reports rather than pointing at test code.
    """
    for i, line in enumerate(lines):
        if line.strip() == '#[cfg(test)]':
            return i
    return len(lines)


def run_tests(env):
    result = subprocess.run(
        ['cargo', 'test', '--lib', '-q'],
        cwd=CRATE, env=env, capture_output=True, text=True)
    output = result.stdout + result.stderr
    if 'error[' in output or 'error: could not compile' in output:
        return 'no-build'
    return 'green' if result.returncode == 0 else 'red'

SIDECAR = '.screen_orig'


def recover(path):
    """Restore a file a killed run left mutated.

    The `finally` below cannot run if the process is killed. Tick 219 lost a
    `swap_lerps.py` run to a timeout that way, and the next run read the
    mutated file as its baseline and reported the repair as a finding. The
    sidecar holds the last known-good text, so put it back before reading.
    """
    if os.path.exists(path + SIDECAR):
        io.open(path, 'w', encoding='utf-8', newline='').write(
            io.open(path + SIDECAR, encoding='utf-8', newline='').read())
        os.remove(path + SIDECAR)
        print('  (recovered %s from a killed run)' % os.path.basename(path))


def screen(path, env):
    recover(path)
    original = open(path, encoding='utf-8', newline='').read()
    io.open(path + SIDECAR, 'w', encoding='utf-8', newline='').write(original)
    newline = '\r\n' if '\r\n' in original else '\n'
    lines = original.replace('\r\n', '\n').split('\n')
    limit = test_module_ranges(lines)
    found = [g for g in guards(lines) if g[0] < limit]

    rel = os.path.relpath(path, REPO).replace(os.sep, '/')
    print('%s: %d single-return guards' % (rel, len(found)))
    idle, unbuilt = [], 0
    try:
        for index, _, condition in found:
            cut = lines[:index] + lines[index + 3:]
            open(path, 'w', encoding='utf-8', newline='').write(
                newline.join(cut))
            verdict = run_tests(env)
            if verdict == 'green':
                idle.append((index + 1, condition))
                print('  line %-5d GREEN WITHOUT IT   if %s'
                      % (index + 1, condition[:70]))
            elif verdict == 'no-build':
                unbuilt += 1
    finally:
        open(path, 'w', encoding='utf-8', newline='').write(original)
        os.remove(path + SIDECAR)

    print('  %d of %d can be deleted with the suite still green (%d would not build)'
          % (len(idle), len(found), unbuilt))
    return idle


def changed_files():
    result = subprocess.run(
        ['git', 'show', '--name-only', '--pretty=format:', 'HEAD'],
        cwd=REPO, capture_output=True, text=True)
    out = []
    for name in result.stdout.split('\n'):
        name = name.strip()
        if name.endswith('.rs'):
            out.append(os.path.join(REPO, name.replace('/', os.sep)))
    return out


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    paths = changed_files() if argv[0] == '--changed' else [
        os.path.abspath(p) for p in argv]
    paths = [p for p in paths if os.path.exists(p)]
    if not paths:
        print('no files to screen')
        return 0

    env = dict(os.environ)
    env['PATH'] = MSVC + os.pathsep + env.get('PATH', '')

    total = 0
    for path in paths:
        total += len(screen(path, env))
    if len(paths) > 1:
        print()
        print('%d candidates across %d files' % (total, len(paths)))
    print()
    print('A green deletion is a candidate to read, not a defect. It is either')
    print('a redundant guard, a missing test, or something the types already')
    print('prevent -- and only the first is fixed by deleting the guard.')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
