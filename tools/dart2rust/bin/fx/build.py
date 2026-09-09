# -*- coding: utf-8 -*-
"""Translate one fixture and write the crate that runs it.

Called by `bin/fx.sh`; see that file for what a fixture is and why the
comparison is against the Dart VM rather than against expected output.

    FX_AOT=1 FX_MAIN='print(fx.use());' python3 bin/fx/build.py <dir> <name>

`<dir>` is where the `<name>.dart` fixture lives and where everything this
writes goes -- the wrapper `main`, the dill, the Rust, the crate. It is a
build directory, not part of the repository.
"""
import io
import os
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(TOOL, 'bin'))

import dill as dill_tool  # noqa: E402
import fixtures  # noqa: E402


def module_name(path, crate):
    """The module a translated file lands in: its path, cleaned.

    The backend names a module after the file's URI, so the name depends on
    where the fixture is -- which is what tied the old harness to one absolute
    directory, through a template with the name written into it. Derived here,
    and then checked against what the crate actually declares, so a change to
    the backend's `_cleanIdentifier` fails loudly instead of emitting a `run.rs`
    that does not compile.
    """
    stem = os.path.splitext(os.path.abspath(path))[0]  # the backend drops `.dart`
    name = ''.join(c if c.isalnum() else '_' for c in stem)
    declared = io.open(os.path.join(crate, 'lib.rs'), encoding='utf-8').read()
    if 'pub mod %s;' % name not in declared:
        raise SystemExit('no `pub mod %s` in %s/lib.rs' % (name, crate))
    return name


def main():
    work, name = sys.argv[1], sys.argv[2]
    fixture = os.path.join(work, name + '.dart')
    if not os.path.exists(fixture):
        print('no fixture at', fixture)
        return 1
    aot = os.environ.get('FX_AOT') == '1'

    # A wrapper with the `main` the VM needs; the fixture itself is a library
    # so that the translated side is a library too.
    wrapper = os.path.join(work, 'entry_%s.dart' % name)
    io.open(wrapper, 'w', encoding='utf-8', newline='\n').write(
        "import '%s' as fx;\nvoid main() { %s }\n"
        % (fixtures.as_uri(fixture), os.environ.get('FX_MAIN', '')))

    dill = os.path.join(work, name + '.dill')
    code = dill_tool.build(fixtures.as_uri(wrapper), fixtures.APP_PACKAGES,
                           dill, aot=aot)
    if code != 0 or not os.path.exists(dill):
        print('dill None aot', aot)
        return 1
    print('dill', dill, 'aot', aot)

    config = os.path.join(TOOL, '.agree', 'kernel_package_config.json')
    ok, err = fixtures.from_kernel(dill, fixture, os.path.join(
        work, name + '.rs'), config)
    print('ok', ok)
    print(err[-1500:])
    if not ok:
        return 1

    # The whole `file:` package, so a fixture may span libraries.
    crate = os.path.join(work, 'pk_' + name)
    r = subprocess.run(
        [dill_tool.paths()['dart'], 'run', '--packages=' + config,
         os.path.join(TOOL, 'bin', 'dart2rust_package.dart'),
         dill, 'file:', crate])
    if r.returncode != 0:
        return r.returncode

    template = io.open(os.path.join(HERE, 'Cargo.tmpl'), encoding='utf-8').read()
    io.open(os.path.join(crate, 'Cargo.toml'), 'w', encoding='utf-8',
            newline='\n').write(template.replace('PKNAME', name))
    io.open(os.path.join(crate, 'run.rs'), 'w', encoding='utf-8',
            newline='\n').write(
        'fn main() {\n    println!("{}", pk_%s::%s::r#use().unwrap());\n}\n'
        % (name, module_name(fixture, crate)))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
