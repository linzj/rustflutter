/// `compareTo`, `moveNext` and `current` throw, through the prelude traits
/// a class implements.
///
/// A class that `implements Comparable<T>` or `implements Iterator<T>` gets
/// an `impl` of the prelude's trait whose body forwards to the class's own
/// method (`_emitPreludeInterfaces`). Those trait signatures were written
/// `-> i64`, `-> bool`, `-> __A0` -- by this compiler, for itself -- so the
/// forwarder had the callee's `Result` in its hand and nowhere to put it,
/// and ended `.unwrap()`: 11 panics in the gallery whose only cause was a
/// signature (work.md 七.5 / 八.1). Dart lets all three throw, and
/// `list.sort()` runs `compareTo` for every pair.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

class Priority implements Comparable<Priority> {
  Priority(this.rank);

  final int rank;

  @override
  int compareTo(Priority other) {
    if (rank < 0 || other.rank < 0) {
      throw ArgumentError('a negative rank has no order');
    }
    return rank < other.rank ? -1 : (rank == other.rank ? 0 : 1);
  }

  @override
  String toString() => 'p$rank';
}

class Cursor implements Iterator<int> {
  Cursor(this.upTo, this.breakAt);

  final int upTo;
  final int breakAt;
  int at = -1;

  @override
  bool moveNext() {
    at++;
    if (at == breakAt) throw StateError('the cursor broke at $at');
    return at < upTo;
  }

  @override
  int get current {
    if (at >= upTo) throw StateError('read past the end');
    return at * 10;
  }
}

String sorted(List<Priority> ps) {
  try {
    ps.sort();
    return 'sorted:$ps';
  } on ArgumentError catch (e) {
    return 'compare-threw:${e.message}';
  }
}

String walk(Iterator<int> it) {
  final out = <String>[];
  try {
    while (it.moveNext()) {
      out.add('${it.current}');
    }
    return 'walked:${out.join(",")}';
  } on StateError catch (e) {
    return 'walk-threw:${e.message} after ${out.join(",")}';
  }
}

String use() {
  final out = <String>[];
  out.add(sorted(<Priority>[Priority(3), Priority(1), Priority(2)]));
  out.add(sorted(<Priority>[Priority(3), Priority(-1)]));
  out.add(walk(Cursor(3, -1)));
  out.add(walk(Cursor(3, 1)));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
