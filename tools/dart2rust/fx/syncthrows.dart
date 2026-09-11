/// `dart:io`'s synchronous calls and `utf8.decode` throw, and Dart catches
/// them.
///
/// `File.readAsStringSync()` on a file that is not there is a
/// `FileSystemException`, and `on FileSystemException catch` is how a program
/// reads a missing file. `utf8.decode(bad)` is a `FormatException`. Both were
/// `panic!("uncaught Dart exception: ..")` inside the prelude -- the file
/// calls through a `raise(self) -> !` that existed only to panic
/// (work.md step 6).
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

import 'dart:convert';
import 'dart:io';

String readMissing(String path) {
  // A bare `catch`, not `on FileSystemException`: this compiler refuses an
  // `is` against `FileSystemException` (it is not a translated class), and
  // that refusal is a separate thing from the rule under test here. What
  // this fixture asks is that the program *reaches* the catch at all.
  try {
    return File(path).readAsStringSync();
  } catch (_) {
    return 'no-such-file';
  }
}

String decode(List<int> bytes) {
  try {
    return utf8.decode(bytes);
  } on FormatException catch (_) {
    return 'not-utf8';
  }
}

String lossy(List<int> bytes) =>
    utf8.decode(bytes, allowMalformed: true).length.toString();

String use() {
  final out = <String>[];
  out.add(readMissing('a-file-that-is-not-here-1759'));
  out.add(decode(<int>[104, 105]));
  out.add(decode(<int>[0xff, 0xfe]));
  out.add(lossy(<int>[0xff, 0xfe]));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
