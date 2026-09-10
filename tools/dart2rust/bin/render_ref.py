# -*- coding: utf-8 -*-
"""Regenerate the render ruler's reference: Flutter's own walk of the gallery.

    python3 tools/dart2rust/bin/render_ref.py [--force]

Runs `test/dump_render_walk_settled_test.dart` in the app checkout under the
real Flutter, and keeps what it printed between its two markers as
`tools/dart2rust/.build/scratch/ref_render_walk_settled.txt`.

That file is the fourth ruler's other half: `bin/render_ruler.sh` diffs the
translated program's `DART2RUST_DUMP_RENDER_TREE=1` dump against it, ignoring
`size=` and `offset=`, and the reading is "N nodes, T type differences".

It lived in `~/dart2rust_build/scratch/` until 2026-09-10, outside the
repository and with nothing that said how to make it again -- so a cleanup of
that directory silently took the ruler's zero point away. The test that
produces it is upstream in the app; this is the two lines that run it.
"""
import io
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
sys.path.insert(0, HERE)

from paths import APP, FLUTTER, exe  # noqa: E402

OUT = os.path.join(TOOL, '.build', 'scratch', 'ref_render_walk_settled.txt')
TEST = os.path.join('test', 'dump_render_walk_settled_test.dart')
BEGIN = '=== DART2RUST RENDER WALK (SETTLED) BEGIN ==='
END = '=== DART2RUST RENDER WALK (SETTLED) END ==='


def main():
    force = '--force' in sys.argv[1:]
    if os.path.exists(OUT) and os.path.getsize(OUT) > 0 and not force:
        print('have', OUT, sum(1 for _ in io.open(OUT, encoding='utf-8')),
              'lines (--force to rebuild)')
        return 0
    if not os.path.exists(os.path.join(APP, TEST)):
        print('no', TEST, 'in', APP)
        return 1
    flutter = os.path.join(FLUTTER, 'bin', exe('flutter'))
    result = subprocess.run([flutter, 'test', TEST], cwd=APP,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    text = result.stdout.decode('utf-8', 'replace')
    if BEGIN not in text or END not in text:
        print('the test printed no walk (exit %d); last lines:' %
              result.returncode)
        print('\n'.join(text.splitlines()[-25:]))
        return 1
    body = text.split(BEGIN, 1)[1].split(END, 1)[0]
    # `print` puts a blank line around the buffer; the walk itself has none.
    lines = [line for line in body.splitlines() if line.strip()]
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    io.open(OUT, 'w', encoding='utf-8', newline='\n').write(
        '\n'.join(lines) + '\n')
    print('wrote', OUT, len(lines), 'lines')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
