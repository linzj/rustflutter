/// `jsonDecode` on malformed text throws, and so does `jsonEncode` on a value
/// it cannot write.
///
/// Dart's decoder raises `FormatException` and its encoder
/// `JsonUnsupportedObjectError` -- both catchable. The prelude's JSON pair
/// panicked instead: the parser's `fail(&self, what) -> !` existed only to
/// do that, and the encoder's `json_key` had nowhere to put the error
/// (work.md step 6, the last three).
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

import 'dart:convert';

String decode(String text) {
  try {
    return '${jsonDecode(text)}';
  } on FormatException catch (_) {
    return 'bad-json';
  }
}

String encode(Object value) {
  try {
    return jsonEncode(value);
  } catch (_) {
    return 'cannot-encode';
  }
}

/// An object `jsonEncode` has no way to write and no `toEncodable` for.
class Unencodable {
  const Unencodable();
}

String use() {
  final out = <String>[];
  out.add(decode('{"a": 1}'));
  out.add(decode('{"a": '));
  out.add(decode('[1, 2, 3]'));
  out.add(decode('not json at all'));
  out.add(encode(<String, Object>{'a': 1, 'b': 'two'}));
  out.add(encode(<Object, Object>{1: 'one'}));
  // Nothing the encoder has a shape for: the fall-through at the end of
  // `json_write`, which was the last `panic!` in that pair.
  out.add(encode(Unencodable()));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
