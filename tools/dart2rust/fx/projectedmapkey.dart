/// A map looked up with a key that is a generic declaration's own `T?`.
///
/// A `T?` in a declaration's field or edge is the projection
/// `<T as DartNullable>::Or`, not the plain `Option<T>` a body works with,
/// and the projection has no `as_ref` -- which is what the nullable map
/// read asks the key for:
///
///     no method named `as_ref` found for associated type
///     `<T as DartNullable>::Or`
///
/// `CupertinoSlidingSegmentedControl`'s `onEnd` does
/// `_segmentKeys[highlighted]` with a `T? highlighted` field (1 stub at
/// ws1105).
library;

class Board<T> {
  Board(this.marks);

  /// The projected field: a `T?` this declaration holds.
  T? picked;

  final Map<T, String> marks;

  /// The read under test.
  String? markOfPicked() => marks[picked];

  /// ..and the same read on a local, for the contrast.
  String? markOf(T? key) => marks[key];
}

String use() {
  final Board<String> b = Board<String>(<String, String>{'a': 'A', 'b': 'B'});
  final List<String> out = <String>[];
  out.add('${b.markOfPicked()}');
  b.picked = 'a';
  out.add('${b.markOfPicked()}');
  b.picked = 'z';
  out.add('${b.markOfPicked()}');
  out.add('${b.markOf('b')}');
  out.add('${b.markOf(null)}');
  return out.join('|');
}
