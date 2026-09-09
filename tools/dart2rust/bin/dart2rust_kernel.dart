// The Kernel driver: read a .dill, emit Rust for one library.
//
//     dart run --packages=<kernel config> \
//         tools/dart2rust/bin/dart2rust_kernel.dart \
//         app.dill package:flutter/src/painting/alignment.dart [-o out.rs]
//
// Deliberately the same backend, the same IR, and -- for the classes it can
// reach -- the same output as `dart2rust.dart`. That equality is the test: two
// front ends that agree on the IR should agree on the Rust, and if they do not,
// one of them has the language wrong.
//
// Build the dill and the package config with `bin/dill.py`.

import 'dart:io';

import 'package:kernel/binary/ast_from_binary.dart';
import 'package:kernel/class_hierarchy.dart';
import 'package:kernel/core_types.dart';
import 'package:kernel/kernel.dart';
import 'package:kernel/type_environment.dart';

import '../lib/backend_rust.dart';
import '../lib/frontend_kernel.dart';

Future<void> main(List<String> args) async {
  if (args.length < 2) {
    stderr.writeln(
      'usage: dart2rust_kernel.dart <app.dill> <library uri> '
      '[-o out.rs] [--list]',
    );
    exit(2);
  }
  final dill = args[0];
  final wanted = args[1];
  String? out;
  for (var i = 2; i < args.length - 1; i++) {
    if (args[i] == '-o') out = args[i + 1];
  }

  // Eagerly, bodies and all: `loadComponentFromBinary` leaves them behind a
  // `lazyBuilder`, and what this compiler emits depends on whether they were
  // read before lowering started (`dart2rust_package.dart` carries the
  // measurement). The two front ends are compared byte for byte, so they have
  // to read their input the same way.
  final component = Component();
  BinaryBuilder(
    File(dill).readAsBytesSync(),
    disableLazyReading: true,
  ).readComponent(component);

  if (args.contains('--list')) {
    final matching = component.libraries
        .where((l) => l.importUri.toString().contains(wanted))
        .toList();
    // 60 is a courtesy to a human reading the terminal. A tool asking for the
    // list wants all of them -- `compiles.py` measured the first 60
    // alphabetically and called it `package:flutter/`, which is animation and
    // cupertino and not much else.
    final shown = args.contains('--all') ? matching : matching.take(60);
    for (final lib in shown) {
      stdout.writeln(
        '${lib.classes.length.toString().padLeft(4)}  '
        '${lib.importUri}',
      );
    }
    stdout.writeln('${matching.length} libraries match');
    return;
  }

  final matches = component.libraries.where(
    (l) => l.importUri.toString() == wanted,
  );
  if (matches.isEmpty) {
    stderr.writeln('no library `$wanted` in $dill (try --list)');
    exit(1);
  }

  final (enumValues, enumFields) = enumsIn(component);
  // The program's types, as `dart2rust_package.dart` builds them. Without
  // this the front end runs with a third of its type knowledge switched off
  // -- 34 places read `typeEnvironment`, and every one of them takes the
  // null branch -- so what this driver writes is not what the compiler
  // writes. `_constantStaticType` is the one the golden shows: a
  // `DoubleConstant` has no type without `coreTypes`, so `const Spacing._
  // (3.0)` was widened as if the 3.0 were a `dynamic`, and came out as
  // `3.0.as_any().downcast_ref::<f64>().unwrap().clone()` inside a `const`
  // -- 24 of `testdata`'s 44 errors, in a file only this driver writes.
  //
  // "Deliberately the same backend, the same IR, and the same output" (the
  // line at the top of this file) was true of everything but the front
  // end's own configuration.
  final coreTypes = CoreTypes(component);
  final typeEnvironment = TypeEnvironment(
    coreTypes,
    ClassHierarchy(component, coreTypes),
  );
  final (lib, refused) = KernelFrontend(
    matches.first,
    enumValues: enumValues,
    enumFields: enumFields,
    typeEnvironment: typeEnvironment,
  ).lowerLibrary();
  final (rust, backendRefused) = RustBackend.emitLibrary(
    lib,
    frontEndRefusals: refused,
  );
  refused.addAll(backendRefused);

  stderr.writeln(
    '${lib.classes.length} classes '
    '(${lib.classes.where((c) => c.isAbstract).length} abstract), '
    '${refused.length} refused',
  );
  for (final r in refused.take(25)) {
    stderr.writeln('  REFUSED $r');
  }

  if (out != null) {
    File(out).writeAsStringSync(rust);
    stderr.writeln('-> $out');
  } else {
    stdout.write(rust);
  }
}
