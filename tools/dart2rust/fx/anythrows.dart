/// `any` and `all` with a predicate that throws.
///
/// `where` was lowered to a `for` loop at ws1078 so that a throwing test
/// comes out as an `Err`; `any` and `all` were left on the adapter, whose
/// closure returns a plain `bool` and so ends in `.unwrap()` -- 27 of them
/// across the gallery (work.md step 4). Dart propagates the throw out of
/// `any` to the caller, which catches it.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

bool isBig(int x) {
  if (x < 0) throw ArgumentError('negative: $x');
  return x > 10;
}

String anyBig(List<int> xs) {
  try {
    return 'any:${xs.any(isBig)}';
  } on ArgumentError catch (e) {
    return 'any-threw:${e.message}';
  }
}

String allBig(List<int> xs) {
  try {
    return 'all:${xs.every(isBig)}';
  } on ArgumentError catch (e) {
    return 'all-threw:${e.message}';
  }
}

String use() {
  final out = <String>[];
  out.add(anyBig(<int>[1, 2, 30]));
  out.add(anyBig(<int>[1, 2, 3]));
  out.add(anyBig(<int>[1, -2, 30]));
  out.add(allBig(<int>[30, 40]));
  out.add(allBig(<int>[30, -1]));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
