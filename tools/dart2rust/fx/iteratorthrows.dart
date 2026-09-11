/// A Dart `get iterator` can throw, and `toList()` on it must not panic.
///
/// A translated class that is an `Iterable` gets a walker this compiler
/// names `__to_list` -- a name no Dart member has, so its signature is the
/// compiler's own. It used to be `-> Vec<E>`, which left the `Result` from
/// `iterator` nowhere to go: the shim wrote
/// `panic!("uncaught Dart exception: ..")` instead, seven of them in the
/// gallery (work.md step 5).
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

class Counting extends Iterable<int> {
  Counting(this.upTo, this.broken);

  final int upTo;
  final bool broken;

  @override
  Iterator<int> get iterator {
    if (broken) throw StateError('no iterator today');
    return List<int>.generate(upTo, (int i) => i).iterator;
  }
}

String walk(Counting c) {
  try {
    return '${c.toList()}';
  } on StateError catch (e) {
    return '$e';
  }
}

String use() {
  final out = <String>[];
  out.add(walk(Counting(3, false)));
  out.add(walk(Counting(3, true)));
  out.add(walk(Counting(1, false)));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
