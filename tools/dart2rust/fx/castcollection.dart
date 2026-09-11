/// `as List<int>` and `as Map<String, int>` on a value that is neither.
///
/// Dart throws `TypeError` there, exactly as a plain `as` does, and
/// `on TypeError catch` is ordinary Dart. Those two casts and the type
/// parameter's `from_dynamic` were written `.unwrap()` at their emission
/// sites (`expressions.dart:447/455`) instead of going through the one
/// lever every other cast uses -- 105 of them across the gallery
/// (work.md step 2).
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

String asList(Object o) {
  try {
    final xs = o as List<int>;
    return 'list:${xs.length}';
  } on TypeError catch (_) {
    return 'not-a-list';
  }
}

String asMap(Object o) {
  try {
    final m = o as Map<String, int>;
    return 'map:${m.length}';
  } on TypeError catch (_) {
    return 'not-a-map';
  }
}

String asParam<T>(Object o) {
  try {
    final v = o as T;
    return 'param:$v';
  } on TypeError catch (_) {
    return 'not-a-$T';
  }
}

String use() {
  final out = <String>[];
  final Object ints = <int>[1, 2, 3];
  final Object strings = <String>['a'];
  final Object counts = <String, int>{'a': 1};
  out.add(asList(ints));
  out.add(asList(strings));
  out.add(asList(counts));
  out.add(asMap(counts));
  out.add(asMap(ints));
  out.add(asParam<int>(7));
  out.add(asParam<int>('seven'));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
