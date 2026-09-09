# -*- coding: utf-8 -*-
"""Writes `.dart_tool/package_config.json` for *this* package.

The compiler has never had one. Its own scripts pass `--packages=` at every
call (`bin/dill.py write_config`), which is enough to *run* it and nothing
else: `dart analyze` and `dart test` both resolve through the package config
of the directory they are in, so with no config here neither worked, and the
compiler grew to 51k lines with no analyzer and no tests.

What this writes is the union of the three places its imports come from:

    dart2rust               this directory, so `package:dart2rust/ir.dart`
                            resolves as well as the relative imports `bin/`
                            already uses
    kernel, _fe_analyzer_shared
                            the engine checkout, via `bin/dill.py`
    everything else         the Flutter SDK's own package_config, which is
                            where `package:test` and its dependencies already
                            live (no `pub get`, no network, no lockfile)

Absolute paths, so the file belongs to this machine and is not committed --
the same reason `dill.py` writes its config rather than committing it.

    python3 bin/devsetup.py
"""
import io
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import dill  # noqa: E402  (same directory, and it owns the engine paths)
import paths  # noqa: E402

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _uri(path):
    return 'file://' + os.path.abspath(path).replace('\\', '/')


def main():
    found = dill.paths()
    packages = {
        'dart2rust': {
            'name': 'dart2rust',
            'rootUri': _uri(HERE),
            'packageUri': 'lib/',
            'languageVersion': '3.13',
        },
    }
    # The Flutter SDK's config first: it is the wide one (`test`, `matcher`,
    # `path`, ...), and the checkout's `kernel` must win over any it names.
    sdk_config = os.path.join(paths.FLUTTER, '.dart_tool', 'package_config.json')
    if os.path.exists(sdk_config):
        for entry in json.load(io.open(sdk_config, encoding='utf-8'))['packages']:
            if entry['name'] not in packages:
                root = entry['rootUri']
                if not root.startswith('file:'):  # relative to the SDK config
                    root = _uri(os.path.join(os.path.dirname(sdk_config), root))
                packages[entry['name']] = dict(entry, rootUri=root)
    else:
        print('warning: no Flutter package_config at', sdk_config)
    for name, root in (('kernel', found['kernel']),
                       ('_fe_analyzer_shared', found['fe_shared'])):
        if not os.path.exists(root):
            print('warning: no', name, 'at', root)
            continue
        packages[name] = {'name': name, 'rootUri': _uri(root),
                          'packageUri': 'lib/', 'languageVersion': '3.13'}

    target = os.path.join(HERE, '.dart_tool', 'package_config.json')
    os.makedirs(os.path.dirname(target), exist_ok=True)
    io.open(target, 'w', encoding='utf-8', newline='\n').write(
        json.dumps({'configVersion': 2,
                    'generated': '(bin/devsetup.py)',
                    'generator': 'dart2rust',
                    'packages': sorted(packages.values(),
                                       key=lambda p: p['name'])},
                   indent=2) + '\n')
    print('wrote', target, '--', len(packages), 'packages')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
