"""One-shot screen: which `lerp(a, b, t)` call sites can have their two ends
swapped with the suite still green?

A lerp is symmetric at t = 0.5, so a test that only ever checks the midpoint
cannot tell `lerp(a, b, t)` from `lerp(b, a, t)`. Tick 212 wrote exactly that
test and the mutation stayed green until the t moved off the middle.

Same shape as `tools/idle_guards.py`: mutate, run, restore, report. A green
swap is a candidate to read -- some of these are genuinely symmetric (both
ends the same field of the same two objects) and swapping them changes
nothing at all.
"""
import io
import os
import re
import subprocess
import sys

CRATE = r'K:\rustflutter\src\flutter\rust\rustflutter'
MSVC = (r'C:\Program Files\Microsoft Visual Studio\2022\Community'
        r'\VC\Tools\MSVC\14.44.35207\bin\Hostx64\x64')

CALL = re.compile(
    r'\blerp\(\s*([A-Za-z_][A-Za-z0-9_.]*)\s*,\s*([A-Za-z_][A-Za-z0-9_.]*)\s*,\s*t\s*\)')


def body_limit(text):
    lines = text.split('\n')
    for i, line in enumerate(lines):
        if line.strip() == '#[cfg(test)]':
            return sum(len(l) + 1 for l in lines[:i])
    return len(text)


def run(env):
    result = subprocess.run(['cargo', 'test', '--lib', '-q'],
                            cwd=CRATE, env=env, capture_output=True, text=True)
    out = result.stdout + result.stderr
    if 'error[' in out or 'error: could not compile' in out:
        return 'no-build'
    return 'green' if result.returncode == 0 else 'red'


def main(argv):
    path = argv[0]
    original = io.open(path, encoding='utf-8', newline='').read()
    newline = '\r\n' if '\r\n' in original else '\n'
    text = original.replace('\r\n', '\n')
    limit = body_limit(text)
    sites = [m for m in CALL.finditer(text) if m.start() < limit]
    sites = [m for m in sites if m.group(1) != m.group(2)]
    print('%s: %d swappable lerp calls' % (os.path.basename(path), len(sites)))

    env = dict(os.environ)
    env['PATH'] = MSVC + os.pathsep + env.get('PATH', '')
    green = []
    try:
        for index, match in enumerate(sites):
            swapped = 'lerp(%s, %s, t)' % (match.group(2), match.group(1))
            mutated = text[:match.start()] + swapped + text[match.end():]
            io.open(path, 'w', encoding='utf-8', newline='').write(
                mutated.replace('\n', newline))
            verdict = run(env)
            line = text.count('\n', 0, match.start()) + 1
            if verdict == 'green':
                green.append((line, match.group(0)))
                print('  line %-6d GREEN SWAPPED   %s' % (line, match.group(0)))
            elif verdict == 'no-build':
                print('  line %-6d (would not build)' % line)
    finally:
        io.open(path, 'w', encoding='utf-8', newline='').write(original)
    print('  %d of %d swap unnoticed' % (len(green), len(sites)))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
