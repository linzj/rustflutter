// A class is an `Iterable` by *mixing in* `IterableMixin`, and the members
// it gets that way are declared where no name test can see them.
//
// The front end routes `Iterable`'s members to the prelude's list only when
// the call's declaring class is named `List` or `Iterable`. `class Board
// extends Object with IterableMixin<BoardPoint?>` declares `elementAt` and
// `forEach` on `_MixinApplication386&Object&IterableMixin` in
// `dart:mixin_deduplication`, so both fell straight through to an ordinary
// method call: `board.element_at(i)` named nothing, and `board.for_each(f)`
// asked `Board` to be a Rust `Iterator`. Two stubs, and the whole
// transformations demo's `paint` behind them.
//
// The receiver side was already there -- a translated class that *is* an
// `Iterable<E>` reads as its list (`__to_list`, ws499). Only the way in was
// missing.

import 'dart:collection';

class Ladder extends Object with IterableMixin<int> {
  Ladder(this.rungs);

  final int rungs;

  @override
  Iterator<int> get iterator => _LadderIterator(rungs);
}

class _LadderIterator implements Iterator<int> {
  _LadderIterator(this.rungs);

  final int rungs;

  int at = -1;

  @override
  int get current => at * 3;

  @override
  bool moveNext() {
    at++;
    return at < rungs;
  }
}

// A class of its own that is also an `Iterable`, to pin the other half: a
// member the class declares itself stays an ordinary call and must not be
// read as a member of its list. `count` is not `length`.
class Rungs extends Object with IterableMixin<int> {
  Rungs(this.parts);

  final List<int> parts;

  int count() => parts.length * 10;

  @override
  Iterator<int> get iterator => parts.iterator;
}

String use() {
  final Ladder ladder = Ladder(4);
  // Declared on the mixin application: the two members this is about.
  final int third = ladder.elementAt(2);
  final List<int> seen = <int>[];
  ladder.forEach(seen.add);
  final Rungs rungs = Rungs(<int>[1, 2, 3]);
  // `count` is the class's own; `elementAt` is the mixin's.
  return '$third/${seen.join("-")}/${rungs.count()}/${rungs.elementAt(1)}';
}
