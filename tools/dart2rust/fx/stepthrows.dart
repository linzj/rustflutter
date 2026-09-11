/// A `where` predicate or a `map` transform can throw, and Dart catches it.
///
/// Rust's iterator adapters take a closure that returns a plain value --
/// `filter` wants a `bool`, not a `Result<bool, _>` -- so this compiler
/// emitted the step bodies with no failure channel and a `throw` inside one
/// was a panic (work.md step 3). The chain is a loop now, which carries `?`.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

int _half(int x) {
  if (x == 0) throw StateError('cannot halve zero');
  return x ~/ 2;
}

bool _small(int x) {
  if (x > 100) throw StateError('too big to judge');
  return x < 10;
}

String mapped(List<int> xs) {
  try {
    return '${xs.map(_half).toList()}';
  } on StateError catch (e) {
    return '$e';
  }
}

String filtered(List<int> xs) {
  try {
    return '${xs.where(_small).toList()}';
  } on StateError catch (e) {
    return '$e';
  }
}

String expanded(List<int> xs) {
  try {
    return '${xs.expand((int x) => <int>[x, _half(x)]).toList()}';
  } on StateError catch (e) {
    return '$e';
  }
}

String visited(List<int> xs) {
  final seen = <int>[];
  try {
    xs.forEach((int x) => seen.add(_half(x)));
  } on StateError catch (_) {
    seen.add(-1);
  }
  return '$seen';
}

String use() {
  final out = <String>[];
  out.add(mapped(<int>[4, 6, 8]));
  out.add(mapped(<int>[4, 0, 8]));
  out.add(filtered(<int>[1, 20, 3]));
  out.add(filtered(<int>[1, 200, 3]));
  out.add(expanded(<int>[4, 6]));
  out.add(expanded(<int>[4, 0]));
  out.add(visited(<int>[4, 6]));
  out.add(visited(<int>[4, 0, 6]));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
