"""Generate the rustflutter DEPS by filtering the upstream flutter/flutter DEPS.

Everything Dart, Fuchsia and web-toolchain is dropped; the rendering / text /
build-toolchain dependencies are kept verbatim with their upstream revisions.
"""
import io, re
from collections import Counter

UPSTREAM = 'K:/flutter/DEPS'
OUT = 'K:/rustflutter/DEPS'

# A sentinel that cannot collide with CIPD's literal '${{platform}}' syntax,
# which also uses braces and would otherwise be rewritten into a Var() call.
SEP = chr(1)

ns = {'Var': lambda k: SEP + k + SEP, 'Str': str, '__builtins__': __builtins__}
exec(io.open(UPSTREAM, encoding='utf-8').read(), ns)

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
    if p.split('/')[-1] in ('cpu_features', 're2', 'sqlite', 'ai'):
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
    '# Derived from flutter/flutter DEPS @ cf97bfbcb9f by dropping every Dart SDK,',
    '# Dart pub, Fuchsia and web-toolchain entry. Revisions of the kept entries are',
    '# unchanged from upstream so they can still be rolled against it.',
    '#',
    '# Note: this file describes the intended dependency set. Locally the tree is',
    '# wired up with directory junctions into an existing flutter checkout rather',
    '# than a real `gclient sync`; see PORTING_STATUS.md.',
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
    if isinstance(spec, str):
        L.append('  ' + q(path) + ':\n  ' + interp(spec) + ',')
    else:
        L.append('  ' + q(path) + ': ' + fmt(dict(spec), 2) + ',')
L += ['}', '', 'hooks = [']

kept_hooks = 0
for h in ns['hooks']:
    if h.get('name') in DROP_HOOKS:
        continue
    kept_hooks += 1
    L.append('  ' + fmt(dict(h), 2) + ',')
L += [']', '']

io.open(OUT, 'w', encoding='utf-8', newline='\n').write('\n'.join(L))

print('vars:  %d -> %d' % (len(ns['vars']), len(kept_vars)))
print('deps:  %d -> %d  (dropped %d)' % (
    len(ns['deps']), len(ns['deps']) - len(dropped), len(dropped)))
print('hooks: %d -> %d' % (len(ns['hooks']), kept_hooks))
print('\ndropped deps by reason:')
for why, n in Counter(w for _, w in dropped).most_common():
    print('  %-24s %d' % (why, n))
