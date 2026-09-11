/// A `dynamic` that does not fit the slot it is handed to.
///
/// Dart inserts a downcast at every one of these and throws `TypeError`
/// when it fails -- a value, and one that ordinary Dart catches. This
/// compiler lowers the same coercion to `dart_from_dynamic::<T>`
/// (`coerce.dart`, three branches: a prelude value type, a type parameter,
/// a translated class), and that function read
///
///     panic!("dart2rust: a {} where a `{}` was wanted", ..)
///
/// until 2026-09-11. The `dart2rust:` prefix made it read as a fact about
/// the translator; it is a fact about the program, and there are 128 of
/// them in the gallery.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

class Box {
  Box(this.n);

  final int n;
}

String takeInt(int n) => 'int:$n';

String takeString(String s) => 'str:$s';

String takeBox(Box b) => 'box:${b.n}';

String takeList(List<int> xs) => 'list:${xs.length}';

String asInt(dynamic d) {
  try {
    return takeInt(d);
  } on TypeError catch (_) {
    return 'not-an-int';
  }
}

String asString(dynamic d) {
  try {
    return takeString(d);
  } on TypeError catch (_) {
    return 'not-a-string';
  }
}

String asBox(dynamic d) {
  try {
    return takeBox(d);
  } on TypeError catch (_) {
    return 'not-a-box';
  }
}

String asList(dynamic d) {
  try {
    return takeList(d);
  } on TypeError catch (_) {
    return 'not-a-list';
  }
}

String use() {
  final out = <String>[];
  out.add(asInt(7));
  out.add(asInt('seven'));
  out.add(asString('s'));
  out.add(asString(7));
  out.add(asBox(Box(3)));
  out.add(asBox('not a box'));
  out.add(asList(<int>[1, 2]));
  out.add(asList(<String>['a']));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
