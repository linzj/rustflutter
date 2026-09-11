/// The prelude's remaining panics, each of them a Dart `throw`.
///
/// `[].firstWhere(t)` is a `StateError`, `Map.fromIterables` with lists of
/// different lengths an `ArgumentError`, completing a `Completer` twice a
/// `StateError`, `RangeError.checkNotNegative(-1)` and
/// `ArgumentError.checkNotNull(null)` their own names. Every one of them
/// was a `panic!` inside the prelude (work.md section 五), and every one of
/// them is caught by ordinary Dart.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

import 'dart:async';

String firstWhere(List<int> xs) {
  try {
    return 'first:${xs.firstWhere((int x) => x > 10)}';
  } on StateError catch (e) {
    return 'no-element:${e.message}';
  }
}

String fromIterables(List<String> keys, List<int> values) {
  try {
    return 'map:${Map<String, int>.fromIterables(keys, values).length}';
  } on ArgumentError catch (_) {
    return 'length-mismatch';
  }
}

String fromIterable(List<int> xs) {
  try {
    final m = Map<int, int>.fromIterable(
      xs,
      key: (Object? e) => (e as int) < 0 ? throw StateError('negative') : e,
      value: (Object? e) => (e as int) * 2,
    );
    return 'built:${m.length}';
  } on StateError catch (e) {
    return 'key-threw:${e.message}';
  }
}

String completeTwice() {
  final c = Completer<int>();
  c.complete(1);
  try {
    c.complete(2);
    return 'completed-twice';
  } on StateError catch (_) {
    return 'already-completed';
  }
}

String notNegative(int n) {
  try {
    return 'ok:${RangeError.checkNotNegative(n, 'n')}';
  } on RangeError catch (_) {
    return 'negative';
  }
}

String notNull(int? v) {
  try {
    return 'ok:${ArgumentError.checkNotNull(v, 'v')}';
  } on ArgumentError catch (_) {
    return 'was-null';
  }
}

String use() {
  final out = <String>[];
  out.add(firstWhere(<int>[1, 20, 3]));
  out.add(firstWhere(<int>[1, 2, 3]));
  out.add(fromIterables(<String>['a', 'b'], <int>[1, 2]));
  out.add(fromIterables(<String>['a', 'b'], <int>[1]));
  out.add(fromIterable(<int>[1, 2]));
  out.add(fromIterable(<int>[1, -2]));
  out.add(completeTwice());
  out.add(notNegative(3));
  out.add(notNegative(-3));
  out.add(notNull(4));
  out.add(notNull(null));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
