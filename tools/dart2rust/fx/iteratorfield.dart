// A *field* can satisfy a getter an interface declares.
//
// `Iterator<E>` declares `E get current`, and Dart lets a class implement
// that with a field: `_BoardIterator implements Iterator<BoardPoint?>`
// writes `BoardPoint? current;`. The backend already emits
// `impl DartIterator<E> for X` for a class that is an `Iterator`
// (`_emitPreludeInterfaces`), but it asked for every interface member to
// land on a *method*, so a field satisfying one withheld the whole impl --
// and then the class was not an iterator at all, which took `Board` and the
// transformations demo's `paint` with it.
//
// The field is read the way it is *held*: this class is counted (a closure
// in its body calls one of its own methods), so its mutable fields live in
// cells and the read is `borrow().clone()`, not a plain field access. That
// is the same shape the gallery's is.

class Steps implements Iterator<int> {
  Steps(this.limit);

  final int limit;

  int index = -1;

  // The field standing in for `Iterator.current`.
  @override
  int current = 0;

  // Makes the class counted, so `current` is a cell and the read above is
  // the one being tested. Called below, or TFA shakes it away.
  int Function() get restart =>
      () => reset();

  int reset() {
    index = -1;
    return limit;
  }

  @override
  bool moveNext() {
    index++;
    if (index >= limit) {
      return false;
    }
    current = index * 2;
    return true;
  }
}

class Evens extends Iterable<int> {
  Evens(this.limit);

  final int limit;

  @override
  Iterator<int> get iterator => Steps(limit);
}

String use() {
  final List<int> seen = <int>[];
  // A `for-in` over a class that is an `Iterable` whose `iterator` is a
  // class that is an `Iterator`: the whole chain this fixture is about.
  for (final int v in Evens(3)) {
    seen.add(v);
  }
  final Steps s = Steps(4);
  return '${seen.join(",")}/${s.restart()}/${Evens(2).length}';
}
