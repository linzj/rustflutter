// The prelude's Rust, written out without translating anything.
//
// `lib/prelude.dart` declares one name, `const rustPrelude`, and the only
// thing done with it anywhere is to write it verbatim (see
// `bin/dart2rust_package.dart`). So when a sweep finds that the prelude is
// the *only* thing that moved, every fixture crate's `dart_prelude.rs` can
// be refreshed from here and the dill, the front end and the package
// emitter all stay unrun -- which is 10.6 of the 14 seconds a fixture used
// to cost (`bin/fx/fingerprint.sh`).
library;

import 'dart:io';

import '../../lib/prelude.dart';

void main(List<String> args) {
  if (args.length != 1) {
    stderr.writeln('usage: dart run bin/fx/emit_prelude.dart <path>');
    exit(2);
  }
  File(args.single).writeAsStringSync(rustPrelude);
}
