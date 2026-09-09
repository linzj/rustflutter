# -*- coding: utf-8 -*-
"""`dart format`, around a formatter that cannot read `augment class`.

`RustBackend` and `KernelFrontend` are each one class spread over a directory
of `part` files (see `bin/experiments.sh`). `dart format` has no way to be told
about the experiment -- its `--enable-experiment` is there but hits a bug in
dart_style -- so on those eleven files, **plain `dart format` silently formats
nothing**: it counts them and reports "0 changed".

That silence is the reason this exists. The wrapper line is one line that this
repository writes itself:

    augment class RustBackend {   ->   class RustBackend {

and with it swapped the file parses and formats exactly as it did before it was
split (verified: right after the split every part came back "already
formatted"). So: format a copy with the keyword removed, put the keyword back,
write the file only if the body actually changed.

    python3 bin/fmt.py                    # format lib, bin and test
    python3 bin/fmt.py --check            # exit 1 if anything would change
    python3 bin/fmt.py --check a.dart b/  # named files or directories

When dart_style learns `augment`, delete this and go back to `dart format`.
"""
import argparse
import io
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
AUGMENT = re.compile(r'^augment (class [A-Za-z_][A-Za-z0-9_]* \{)$', re.M)


def _dart():
    for path in os.environ.get('PATH', '').split(os.pathsep):
        exe = os.path.join(path, 'dart')
        if os.path.isfile(exe) and os.access(exe, os.X_OK):
            return exe
    sys.exit('no dart on PATH')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--check', action='store_true',
                    help='report what would change, and exit 1 if anything would')
    ap.add_argument('paths', nargs='*', default=['lib', 'bin', 'test'],
                    help='files or directories (default: lib bin test)')
    args = ap.parse_args()

    sources = []
    for p in args.paths:
        full = p if os.path.isabs(p) else os.path.join(TOOL, p)
        if os.path.isfile(full):
            sources.append(full)
            continue
        for root, _, names in os.walk(full):
            for n in names:
                if n.endswith('.dart'):
                    sources.append(os.path.join(root, n))
    sources.sort()
    if not sources:
        print('no .dart files in: ' + ' '.join(args.paths))
        return 0

    work = tempfile.mkdtemp(prefix='dart2rust-fmt-')
    try:
        # A mirror of the tree with the keyword taken out, formatted in one
        # `dart format` run -- one VM start rather than one per file.
        for i, src in enumerate(sources):
            text = io.open(src, encoding='utf-8').read()
            io.open(os.path.join(work, '%04d.dart' % i), 'w',
                    encoding='utf-8', newline='\n').write(AUGMENT.sub(r'\1', text))
        r = subprocess.run([_dart(), 'format', work],
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        out = r.stdout.decode('utf-8', 'replace')
        if r.returncode != 0 or 'Could not format' in out:
            sys.stdout.write(out)
            return 1

        changed = []
        for i, src in enumerate(sources):
            text = io.open(src, encoding='utf-8').read()
            formatted = io.open(os.path.join(work, '%04d.dart' % i),
                                encoding='utf-8').read()
            # Put the keyword back on the line it came off.
            restored = re.sub(r'^(class [A-Za-z_][A-Za-z0-9_]* \{)$',
                              r'augment \1', formatted, flags=re.M) \
                if AUGMENT.search(text) else formatted
            if restored != text:
                changed.append(os.path.relpath(src, TOOL))
                if not args.check:
                    io.open(src, 'w', encoding='utf-8',
                            newline='\n').write(restored)
    finally:
        shutil.rmtree(work, ignore_errors=True)

    verb = 'would change' if args.check else 'formatted'
    print('%d files, %d %s' % (len(sources), len(changed), verb))
    for c in changed:
        print('  ' + c)
    return 1 if (args.check and changed) else 0


if __name__ == '__main__':
    raise SystemExit(main())
