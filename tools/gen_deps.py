"""Generate the rustflutter DEPS by filtering the upstream flutter/flutter DEPS.

Everything Dart, Fuchsia and web-toolchain is dropped; the rendering / text /
build-toolchain dependencies are kept verbatim with their upstream revisions.

Paths are rewritten out of the upstream monorepo layout and into rustflutter's,
which is the same tree minus the `engine/` level: upstream's checkout root
holds `engine/src/flutter`, ours holds `src/flutter`. Without the rewrite the
file describes a tree that does not exist here, and anything acting on it --
CI, or `gclient sync` in a fresh checkout -- would populate the wrong
directories. `tools/check_deps.py` verifies the result against the real tree.
"""
import argparse, io, os, re, subprocess
from collections import Counter

# Upstream's checkout root holds `engine/src/...`; ours holds `src/...`.
UPSTREAM_PREFIX = 'engine/src/'
LOCAL_PREFIX = 'src/'

_HERE = os.path.dirname(os.path.abspath(__file__))

ap = argparse.ArgumentParser(description=__doc__)
ap.add_argument('--upstream', required=True,
                help='the flutter/flutter DEPS to filter (path to its checkout)')
ap.add_argument('--out', default=os.path.join(os.path.dirname(_HERE), 'DEPS'),
                help='where to write the filtered DEPS (default: ../DEPS)')
args = ap.parse_args()
UPSTREAM, OUT = args.upstream, args.out

# A sentinel that cannot collide with CIPD's literal '${{platform}}' syntax,
# which also uses braces and would otherwise be rewritten into a Var() call.
SEP = chr(1)

ns = {'Var': lambda k: SEP + k + SEP, 'Str': str, '__builtins__': __builtins__}
exec(io.open(UPSTREAM, encoding='utf-8').read(), ns)


def localise(path):
    """Rewrites one upstream checkout-relative path into rustflutter's tree."""
    if path.startswith(UPSTREAM_PREFIX):
        return LOCAL_PREFIX + path[len(UPSTREAM_PREFIX):]
    return path


def upstream_revision():
    """The revision of the checkout `UPSTREAM` was read from, for the header.

    Read rather than hardcoded: a stale revision in the header is worse than
    none, because it is the only record of what this file was derived from.
    """
    root = os.path.dirname(os.path.abspath(UPSTREAM))
    try:
        out = subprocess.run(['git', '-C', root, 'rev-parse', '--short=11',
                              'HEAD'], capture_output=True, text=True)
        return out.stdout.strip() or 'an unknown revision'
    except OSError:
        return 'an unknown revision'

# ---------- classification ----------

def drop_dep(path):
    p = path.replace('engine/src/', '')
    if p == 'flutter/third_party/boringssl/src':
        return None  # kept: common/graphics hashes shader cache keys with it
    if '/dart' in p or p.endswith('/dart'):
        return 'Dart SDK'
    if 'fuchsia' in p:
        return 'Fuchsia'
    if 'emsdk' in p or 'esbuild' in p:
        return 'web toolchain'
    if p.startswith('flutter/third_party/pkg/'):
        return 'Dart pub package'
    # Match on any component, not just the last one: these are checked out at
    # `.../cpu_features/src`, so testing the leaf would only ever see `src`.
    if {'cpu_features', 're2', 'sqlite', 'ai'} & set(p.split('/')):
        return 'Dart SDK dependency'
    return None


def drop_var(name):
    if name == 'dart_boringssl_rev':
        return False  # boringssl is kept; only the var name is legacy
    if name.startswith('dart_') or name in ('download_dart_sdk', 'dart_git'):
        return True
    if name.startswith('upstream_'):
        return name[len('upstream_'):] in (
            'sdk', 'dart_style', 'dartdoc', 'ffi', 'gcloud', 'googleapis', 'http',
            'leak_tracker', 'mockito', 'process', 'protobuf', 'pub', 'pub_semver',
            'quiver-dart', 'shelf', 'test', 'usage', 'vector_math', 'webdev',
            'webkit_inspection_protocol', 'equatable', 'archive', 'tar',
            'process_runner', 'packages', 'node_preamble', 'io',
        )
    if 'fuchsia' in name or 'emsdk' in name or 'esbuild' in name:
        return True
    return name in ('checkout_llvm', 'build_devtools_from_sources',
                    'setup_githooks', 'release_candidate')


DROP_HOOKS = {
    'Generate .dart_tool/package_confg.json', 'Generate sdk/version',
    'pub get --offline', 'Download Fuchsia SDK', 'Activate Emscripten SDK',
    'Setup githooks', 'Download Fuchsia system images',
    'Generate Fuchsia GN build rules',
}

# ---------- emit ----------

def q(s):
    return "'" + s.replace("'", "\\'") + "'"


def fmt(v, indent):
    pad = ' ' * indent
    if isinstance(v, bool):
        return 'True' if v else 'False'
    if isinstance(v, str):
        return interp(v)
    if isinstance(v, list):
        body = ',\n'.join(pad + '  ' + fmt(x, indent + 2) for x in v)
        return '[\n' + body + ',\n' + pad + ']'
    if isinstance(v, dict):
        body = ',\n'.join(pad + '  ' + q(k) + ': ' + fmt(x, indent + 2)
                          for k, x in v.items())
        return '{\n' + body + ',\n' + pad + '}'
    return repr(v)


def interp(s):
    """Rebuild Var('x') concatenation from the sentinel-wrapped placeholders."""
    parts = re.split('(' + SEP + '[a-z_0-9]+' + SEP + ')', s)
    out, lit = [], ''
    for p in parts:
        if len(p) > 2 and p[0] == SEP and p[-1] == SEP:
            if lit:
                out.append(q(lit)); lit = ''
            out.append("Var('" + p[1:-1] + "')")
        else:
            lit += p
    if lit:
        out.append(q(lit))
    return ' + '.join(out) if out else "''"


L = [
    '# The dependencies referenced by rustflutter.',
    '#',
    '# Generated by tools/gen_deps.py from flutter/flutter DEPS @ %s by'
    % upstream_revision(),
    '# dropping every Dart SDK, Dart pub, Fuchsia and web-toolchain entry.',
    '# Revisions of the kept entries are unchanged from upstream so they can',
    '# still be rolled against it. Paths are rewritten from upstream\'s',
    '# `engine/src/...` into this tree\'s `src/...`.',
    '#',
    '# Note: this file describes the intended dependency set. Locally the tree is',
    '# wired up with directory junctions into an existing flutter checkout rather',
    '# than a real `gclient sync`, so what the build links against is whatever',
    '# those junctions point at -- not what is written here. The two drift apart',
    '# silently; `tools/check_deps.py` is what catches it. See PORTING_STATUS.md.',
    '',
]

kept_vars = {k: v for k, v in ns['vars'].items() if not drop_var(k)}
L.append('vars = {')
for k, v in kept_vars.items():
    L.append('  ' + q(k) + ': ' + fmt(v, 2) + ',')
L += ['}', '', 'allowed_hosts = [']
for h in ns['allowed_hosts']:
    if h != 'dart.googlesource.com':
        L.append('  ' + q(h) + ',')
L += [']', '', 'deps = {']

dropped = []
for path, spec in ns['deps'].items():
    why = drop_dep(path)
    if why:
        dropped.append((path, why))
        continue
    local = localise(path)
    if isinstance(spec, str):
        L.append('  ' + q(local) + ':\n  ' + interp(spec) + ',')
    else:
        L.append('  ' + q(local) + ': ' + fmt(dict(spec), 2) + ',')
L += ['}', '', 'hooks = [']

kept_hooks = 0
for h in ns['hooks']:
    if h.get('name') in DROP_HOOKS:
        continue
    kept_hooks += 1
    # A hook's action is an argv whose entries are checkout-relative paths to
    # the scripts it runs; they need the same rewrite the deps keys got.
    h = dict(h)
    if isinstance(h.get('action'), list):
        h['action'] = [localise(a) for a in h['action']]
    L.append('  ' + fmt(h, 2) + ',')
L += [']', '']

io.open(OUT, 'w', encoding='utf-8', newline='\n').write('\n'.join(L))

print('vars:  %d -> %d' % (len(ns['vars']), len(kept_vars)))
print('deps:  %d -> %d  (dropped %d)' % (
    len(ns['deps']), len(ns['deps']) - len(dropped), len(dropped)))
print('hooks: %d -> %d' % (len(ns['hooks']), kept_hooks))
print('\ndropped deps by reason:')
for why, n in Counter(w for _, w in dropped).most_common():
    print('  %-24s %d' % (why, n))
