"""The size ruler: how big the translated program is, and where that size is.

The other four rulers -- `run_chain.sh`, `fx.sh`, `allfx.sh`,
`render_ruler.py` -- say nothing about size, and the one hand measurement
taken without a script compared a *debug* binary against a *release* baseline
and had to be withdrawn (`8d3b5e51` -> `cda614f9`). This is that measurement
written down once.

    python3 bin/size_ruler.py                 # build, then report
    python3 bin/size_ruler.py --no-build      # report what is already built
    python3 bin/size_ruler.py --report FILE   # ..and write it there too

What it prints:

    .text / .rodata / .data.rel.ro / .eh_frame, and the file, stripped
    by crate:        the v0 mangling's `Cs<hash>_<len><name>`
    duplicate:       functions whose machine code is byte-identical
    monomorphic:     functions sharing a base name once `I..E` is stripped

Two rules this script exists to keep:

  * **release against release.** `run_main.sh` and `render_ruler.py` build
    *debug*; this builds *release*, and the two are never compared.
  * **one cargo at a time.** It builds, so it must not run beside
    `run_chain.sh`, `allfx.sh` or `render_ruler.py`.

Everything it writes goes under `.build/size/`, which is inside the project
(nothing of this repository's is written outside it).
"""

import argparse
import collections
import hashlib
import io
import os
import re
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(HERE)
WS = os.path.join(TOOL, '.crate-ws')
OUT = os.path.join(TOOL, '.build', 'size')


def run(args, **kw):
    return subprocess.run(args, capture_output=True, text=True, **kw)


def flags_for(unwind_tables, icf):
    """The release flags, and the line that says which reading this is.

    They stay *here* and out of the environment: `RUSTFLAGS` is part of
    cargo's fingerprint, and a flag set globally would throw away the
    chain's incremental debug build (`run_chain.sh:35-39`). Cargo has no
    profile key for either of these, so the ruler's own build carries them.
    """
    flags = []
    if not unwind_tables:
        # `panic = "abort"` is already set for this profile, so nothing
        # unwinds and the tables describe a thing that cannot happen.
        flags.append('-C force-unwind-tables=no')
    if icf and icf != 'none':
        # `ld.gold` because it is the linker on this machine that has
        # `--icf`; the `lld` on PATH is the Android SDK's, and `rust-lld`
        # is inside the toolchain rather than on the path.
        flags.append('-C link-arg=-fuse-ld=gold')
        flags.append('-C link-arg=-Wl,--icf=%s' % icf)
    return flags


def build(unwind_tables, icf):
    env = dict(os.environ)
    env['RUSTC_BOOTSTRAP'] = '1'
    env['PATH'] = os.path.expanduser('~/.cargo/bin') + os.pathsep + env['PATH']
    threads = env.get('DART2RUST_THREADS', '8')
    env['RUSTFLAGS'] = ' '.join(
        ['-Zthreads=%s' % threads] + flags_for(unwind_tables, icf))
    jobs = env.get('DART2RUST_JOBS', '8')
    print('building release (-j %s) RUSTFLAGS=%s' % (jobs, env['RUSTFLAGS']))
    sys.stdout.flush()
    r = subprocess.run(
        ['cargo', 'build', '--release', '-p', 'dart_main', '-j', jobs],
        cwd=WS, env=env, capture_output=True, text=True)
    if r.returncode != 0:
        sys.stderr.write(r.stdout[-4000:] + r.stderr[-4000:])
        sys.exit('release build failed')


def sections(path):
    """Section sizes, by name.

    `readelf`'s `[ 6]` is *two* awk fields for a one-digit number and one for
    two digits, so the bracketed column is stripped before anything is read
    off the line -- the plan's own note, and the reason a first cut of this
    read zeroes off `libapp.so` while working on `dart_main`.
    """
    out = run(['readelf', '-S', '-W', path]).stdout
    got = {}
    for line in out.splitlines():
        line = re.sub(r'^ *\[ *\d+\] *', '', line)
        parts = line.split()
        if len(parts) > 5 and parts[0].startswith('.'):
            try:
                got[parts[0]] = int(parts[4], 16)
            except ValueError:
                pass
    return got


def symbols(path):
    """(size, name) for every function symbol, sizes in decimal."""
    out = run(['nm', '--size-sort', '-t', 'd', path]).stdout
    got = []
    for line in out.splitlines():
        parts = line.split(None, 2)
        if len(parts) == 3 and parts[1] in 'tT':
            got.append((int(parts[0]), parts[2]))
    return got


CRATE = re.compile(r'Cs[0-9A-Za-z]+_([0-9]+)')


def by_crate(syms):
    counted = collections.Counter()
    for size, name in syms:
        m = CRATE.search(name)
        if m:
            at = m.end()
            counted[name[at:at + int(m.group(1))]] += size
        else:
            counted['(none)'] += size
    return counted


def base_name(name):
    """The name with its generic arguments stripped (v0's `I..E`)."""
    out = []
    depth = 0
    for c in name:
        if c == 'I':
            depth += 1
        elif c == 'E' and depth > 0:
            depth -= 1
        elif depth == 0:
            out.append(c)
    return ''.join(out)


def duplicates(path):
    """Functions whose machine code is byte-identical, and what they cost.

    The `.text` bytes themselves, by `md5`: two functions that differ only in
    a type the code never mentions compile to the same instructions, and
    nothing but the bytes can tell.
    """
    out = run(['nm', '-S', '--defined-only', path]).stdout
    text = None
    for line in run(['readelf', '-S', '-W', path]).stdout.splitlines():
        parts = re.sub(r'^ *\[ *\d+\] *', '', line).split()
        if len(parts) > 5 and parts[0] == '.text':
            text = (int(parts[2], 16), int(parts[3], 16), int(parts[4], 16))
            break
    if text is None:
        return 0, 0, 0
    addr, off, size = text
    blob = io.open(path, 'rb').read()
    groups = collections.defaultdict(list)
    total = 0
    for line in out.splitlines():
        parts = line.split(None, 3)
        if len(parts) != 4 or parts[2] not in 'tT':
            continue
        a, n = int(parts[0], 16), int(parts[1], 16)
        if n == 0 or a < addr or a + n > addr + size:
            continue
        at = off + a - addr
        groups[hashlib.md5(blob[at:at + n]).digest()].append(n)
        total += n
    waste = sum(v[0] * (len(v) - 1) for v in groups.values() if len(v) > 1)
    count = sum(len(v) - 1 for v in groups.values() if len(v) > 1)
    return count, waste, total


def monomorphic(syms):
    groups = collections.defaultdict(list)
    total = 0
    for size, name in syms:
        total += size
        groups[base_name(name)].append(size)
    waste = sum(sum(sorted(v, reverse=True)[1:])
                for v in groups.values() if len(v) > 1)
    count = sum(len(v) - 1 for v in groups.values() if len(v) > 1)
    return count, waste, total


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--no-build', action='store_true')
    ap.add_argument(
        '--unwind-tables', action='store_true',
        help='keep `.eh_frame` (the default is to build without it)')
    ap.add_argument(
        '--icf', choices=['safe', 'all', 'none'], default='all',
        help='fold identical functions at link time (ld.gold). `all` is the '
             'default since ws1086, when `--run` gave a release-only flag a '
             'ruler at last: 707 nodes, 0 panics, 0 type differences')
    ap.add_argument('--report')
    ap.add_argument('--crates', type=int, default=10)
    ap.add_argument(
        '--run', action='store_true',
        help='run the release binary under the render-tree environment and '
             'diff its walk against the reference, the way render_ruler.py '
             'does for debug -- the only way a release-only flag (`--icf`) '
             'can be checked at all')
    args = ap.parse_args()

    if not args.no_build:
        build(args.unwind_tables, args.icf)
    binary = os.path.join(WS, 'target', 'release', 'dart_main')
    if not os.path.exists(binary):
        sys.exit('no %s -- run without --no-build' % binary)
    os.makedirs(OUT, exist_ok=True)
    # Copied out before anything touches it: `strip` rewrites in place, and
    # the workspace's own artefact is cargo's to own.
    rel = os.path.join(OUT, 'rel')
    stripped = os.path.join(OUT, 'rel.stripped')
    shutil.copy2(binary, rel)
    run(['strip', rel, '-o', stripped])

    lines = []
    def say(text):
        lines.append(text)
        print(text)

    sec = sections(stripped)
    say('flags: %s' % (' '.join(flags_for(args.unwind_tables, args.icf)) or '(none)'))
    say('file %d   stripped %d   (release)'
        % (os.path.getsize(rel), os.path.getsize(stripped)))
    say('.text %d   .rodata %d   .data.rel.ro %d   .eh_frame %d   .eh_frame_hdr %d'
        % (sec.get('.text', 0), sec.get('.rodata', 0),
           sec.get('.data.rel.ro', 0), sec.get('.eh_frame', 0),
           sec.get('.eh_frame_hdr', 0)))

    syms = symbols(rel)
    total = sum(s for s, _ in syms)
    say('symbols %d   %d bytes' % (len(syms), total))
    for name, size in by_crate(syms).most_common(args.crates):
        say('  %12d %5.1f%% %s' % (size, size * 100.0 / max(total, 1), name))

    count, waste, seen = duplicates(rel)
    say('duplicate %d copies   %d bytes   %.1f%%'
        % (count, waste, waste * 100.0 / max(seen, 1)))
    count, waste, seen = monomorphic(syms)
    say('monomorphic %d extra   %d bytes   %.1f%%'
        % (count, waste, waste * 100.0 / max(seen, 1)))

    if args.run:
        # The same environment `run_main.sh` gives the debug binary, and the
        # same comparison `render_ruler.py` makes: `size=`/`offset=` are the
        # layout numbers the headless runtime cannot produce, so the ruler
        # is the rest of the line.
        sys.path.insert(0, HERE)
        import render_ruler
        want = render_ruler.types(render_ruler.read(render_ruler.REF))
        env = dict(os.environ, DART2RUST_OS='android',
                   DART2RUST_DUMP_RENDER_TREE='1', RUST_BACKTRACE='1')
        env.setdefault(
            'DART2RUST_ASSETS',
            os.path.expanduser('~/gallery_upstream/build/flutter_assets'))
        env.setdefault('DART2RUST_RUN_SECONDS', '60')
        r = subprocess.run(['timeout', '180', binary], cwd=WS, env=env,
                           capture_output=True, text=True)
        text = r.stdout + r.stderr
        panics = text.count('panicked at')
        got = (render_ruler.types(
            text.split(render_ruler.BEGIN, 1)[1]
                .split(render_ruler.END, 1)[0].splitlines())
            if render_ruler.BEGIN in text and render_ruler.END in text else [])
        diff = sum(1 for a, b in zip(got, want) if a != b) \
            + abs(len(got) - len(want))
        say('release run: nodes=%d panics=%d typediff=%d'
            % (len(got), panics, diff))

    if args.report:
        io.open(args.report, 'w', encoding='utf-8').write('\n'.join(lines) + '\n')


if __name__ == '__main__':
    main()
