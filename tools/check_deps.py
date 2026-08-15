#!/usr/bin/env python3
"""Check that DEPS agrees with the tree the build actually compiles against.

rustflutter never runs `gclient sync`. `third_party` is wired up with directory
junctions into an existing flutter checkout, which means DEPS does not populate
anything -- it only describes what should be there. A description drifts. The
revision the build links against is whatever the junction points at, and
nothing but this script compares that against what DEPS claims.

The drift is silent and it is not hypothetical: it is what happens whenever the
flutter checkout is synced without regenerating DEPS, which is exactly the
sequence an engine upgrade goes through.

Exits non-zero when a linked dependency is at a revision DEPS does not name.
Dependencies that are declared but not linked are reported and do not fail:
several are darwin-only or test-runner infrastructure that this tree has no
junction for and does not build.
"""

import argparse
import io
import os
import subprocess
import sys


def load(deps_path):
    """Executes a DEPS file and returns its namespace, with Var() resolved.

    `Var` is looked up lazily so it can resolve against `vars`, which the file
    assigns before the `deps` literal that calls it.
    """
    ns = {}
    ns['Var'] = lambda key: ns['vars'][key]
    ns['Str'] = str
    exec(io.open(deps_path, encoding='utf-8').read(), ns)
    return ns


def declared_revision(spec):
    """The git revision a dep spec pins, or None if it is not a git dep."""
    if not isinstance(spec, str):
        return None  # a CIPD package; its version is not a git revision
    return spec.rsplit('@', 1)[-1] if '@' in spec else None


def rev_parse(path, rev):
    """Resolves `rev` inside the checkout at `path`, or None if it cannot be.

    Every revision is peeled to a commit with `^{commit}`. DEPS pins some
    dependencies by annotated tag -- harfbuzz is pinned at the tag object for
    13.2.1, not at the commit it points to -- and comparing a tag's hash
    against `HEAD`'s would report a drift that is not there.
    """
    try:
        out = subprocess.run(
            ['git', '-C', path, 'rev-parse', '--verify', '--quiet',
             rev + '^{commit}'], capture_output=True, text=True)
    except OSError:
        return None
    return out.stdout.strip() or None


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('--deps', default='K:/rustflutter/DEPS',
                    help='the DEPS to check; its directory is the checkout root')
    args = ap.parse_args()

    root = os.path.dirname(os.path.abspath(args.deps))
    ns = load(args.deps)

    agreed, drifted, unresolved, unlinked, foreign, cipd = [], [], [], [], [], 0

    for path, spec in ns['deps'].items():
        want = declared_revision(spec)
        if want is None:
            cipd += 1
            continue
        full = os.path.join(root, path)
        if not os.path.exists(full):
            unlinked.append(path)
            continue
        have = rev_parse(full, 'HEAD')
        if have is None:
            foreign.append(path)
        elif have == want:
            agreed.append(path)
        else:
            want_commit = rev_parse(full, want)
            if want_commit is None:
                # The object is not in this checkout at all, so the tree cannot
                # be sitting on it -- but say so separately from a plain drift,
                # because the reason is different and so is the fix.
                unresolved.append((path, want))
            elif want_commit == have:
                agreed.append(path)
            else:
                drifted.append((path, want_commit, have))

    print('%d git deps: %d agree, %d drifted, %d unresolved, %d not linked, '
          '%d not a git tree'
          % (len(agreed) + len(drifted) + len(unresolved) + len(unlinked)
             + len(foreign), len(agreed), len(drifted), len(unresolved),
             len(unlinked), len(foreign)))
    print('%d CIPD packages not checked (no git revision to compare)' % cipd)

    if drifted:
        print('\nDRIFTED -- the build links against a revision DEPS does not name:')
        for path, want, have in drifted:
            print('  %s\n    DEPS says %s\n    tree has  %s' % (path, want, have))

    if unresolved:
        print('\nUNRESOLVED -- DEPS names a revision this checkout does not have:')
        for path, want in unresolved:
            print('  %s\n    DEPS says %s' % (path, want))

    if unlinked:
        print('\nDeclared but not linked (not a failure):')
        for path in unlinked:
            print('  %s' % path)

    if foreign:
        print('\nPresent but not a git checkout:')
        for path in foreign:
            print('  %s' % path)

    return 1 if drifted or unresolved else 0


if __name__ == '__main__':
    sys.exit(main())
