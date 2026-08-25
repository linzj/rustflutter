"""Re-test only the lerp sites a previous screen left green."""
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


def main(argv):
    path, report = argv[0], argv[1]
    wanted = set()
    for line in io.open(report, encoding='utf-8', errors='ignore'):
        m = re.match(r'\s*line (\d+)\s+(GREEN SWAPPED|STILL GREEN)', line)
        if m:
            wanted.add(int(m.group(1)))
    print('%d sites to re-check' % len(wanted))

    original = io.open(path, encoding='utf-8', newline='').read()
    newline = '\r\n' if '\r\n' in original else '\n'
    text = original.replace('\r\n', '\n')

    env = dict(os.environ)
    env['PATH'] = MSVC + os.pathsep + env.get('PATH', '')

    still = []
    try:
        for match in CALL.finditer(text):
            line = text.count('\n', 0, match.start()) + 1
            if line not in wanted or match.group(1) == match.group(2):
                continue
            swapped = 'lerp(%s, %s, t)' % (match.group(2), match.group(1))
            mutated = text[:match.start()] + swapped + text[match.end():]
            io.open(path, 'w', encoding='utf-8', newline='').write(
                mutated.replace('\n', newline))
            result = subprocess.run(['cargo', 'test', '--lib', '-q'],
                                    cwd=CRATE, env=env, capture_output=True, text=True)
            out = result.stdout + result.stderr
            if 'error[' in out or 'error: could not compile' in out:
                print('  line %-6d (would not build)' % line)
            elif result.returncode == 0:
                still.append((line, match.group(0)))
                print('  line %-6d STILL GREEN   %s' % (line, match.group(0)))
    finally:
        io.open(path, 'w', encoding='utf-8', newline='').write(original)
    print('  %d of %d still unnoticed' % (len(still), len(wanted)))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
