/// `removeLast()` on an empty list throws, and Dart catches it.
///
/// Dart's `List.removeLast` on an empty list throws -- and not the
/// `Bad state: No element` an empty iterable's getters throw: the VM reads
/// `_list[_length - 1]`, so it is a `RangeError`, and `on RangeError catch`
/// around one is ordinary Dart. (The fixture is why that is known: the first
/// cut of this rule raised a `StateError` and the Dart side went unhandled.) This compiler lowered it to `pop().unwrap()`, which is a
/// *panic*: the program lost a path it still had.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

String drain(List<int> xs) {
  final out = <String>[];
  // One more than there are, so the last one throws.
  for (var i = 0; i <= xs.length; i++) {
    try {
      out.add('${xs.removeLast()}');
    } on RangeError catch (_) {
      out.add('empty');
    }
  }
  return out.join(',');
}

String use() {
  final out = <String>[];
  out.add(drain(<int>[1, 2, 3]));
  out.add(drain(<int>[]));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
