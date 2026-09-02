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

import 'package:kernel/kernel.dart';

import '../lib/backend_rust.dart';
import '../lib/frontend_kernel.dart';

Future<void> main(List<String> args) async {
  if (args.length < 2) {
    stderr.writeln('usage: dart2rust_kernel.dart <app.dill> <library uri> '
        '[-o out.rs] [--list]');
    exit(2);
  }
  final dill = args[0];
  final wanted = args[1];
  String? out;
  for (var i = 2; i < args.length - 1; i++) {
    if (args[i] == '-o') out = args[i + 1];
  }

  final component = loadComponentFromBinary(dill);

  if (args.contains('--list')) {
    final matching = component.libraries
        .where((l) => l.importUri.toString().contains(wanted))
        .toList();
    for (final lib in matching.take(60)) {
      stdout.writeln('${lib.classes.length.toString().padLeft(4)}  '
          '${lib.importUri}');
    }
    stdout.writeln('${matching.length} libraries match');
    return;
  }

  final matches =
      component.libraries.where((l) => l.importUri.toString() == wanted);
  if (matches.isEmpty) {
    stderr.writeln('no library `$wanted` in $dill (try --list)');
    exit(1);
  }

  final (lib, refused) = KernelFrontend(matches.first).lowerLibrary();
  final (rust, backendRefused) = RustBackend.emitLibrary(lib, frontEndRefusals: refused);
  refused.addAll(backendRefused);

  stderr.writeln('${lib.classes.length} classes '
      '(${lib.classes.where((c) => c.isAbstract).length} abstract), '
      '${refused.length} refused');
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
