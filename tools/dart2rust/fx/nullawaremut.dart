// `x?.m(..)` where `m` mutates the receiver has to act on the place, not on
// a copy of it.
//
// Two shapes, and they need different places because they have different
// *types*:
//
//   * a field held in a cell -- `_backGestureController?.dragEnd(0)` in
//     `_CupertinoBackGestureDetectorState._handleDragCancel`. The place is
//     the cell's `borrow_mut()`, and the value inside is an `Option`, so the
//     call maps over `.as_mut()`.
//
//   * a value a *map* holds -- `_childrenToAdd[id]?.remove(child)` in
//     `RestorationBucket._removeChildData`. `get_mut` already hands back an
//     `Option<&mut V>`, so the call maps over that directly; `.as_mut()`
//     there would be one layer too many.
//
// Both were stubs in the gallery (E0596, "cannot borrow `*it` as mutable"),
// and the map one was worse than a stub: it mutated a copy, so the map kept
// the value the Dart had removed. The `1,3` in the output is what catches
// that -- a fixture that only asked "does it compile" would have passed on
// the copy.
//
// Checked red against the compiler as it stood before ws1017: one E0596.
// Checking that meant `git checkout <commit>~1 -- <files>`, not `git stash`
// -- the rounds were already committed, so a stash saved nothing and the
// "red" run was the fixed compiler tested against itself, which of course
// passed.

class Counter {
  Counter(this.value, this.label);

  int value;

  // A non-`Copy` field, so the holder keeps the counter in a `RefCell`
  // rather than a `Cell` -- the shape `_backGestureController` has. With a
  // `Cell` the read is a copy and the whole question does not arise.
  final String label;

  // Writes a field, so this compiler gives it `&mut self`.
  void bump(int by) {
    value += by;
  }
}

class Holder {
  Holder();

  Counter? single;

  final Map<String, List<int>> lists = <String, List<int>>{};

  // A closure over `this` makes the class counted, so `single` lives in a
  // cell -- the shape the gallery's has.
  void Function() get bumper =>
      () => touch();

  void touch() {
    single?.bump(1);
  }

  void discard(String key, int item) {
    lists[key]?.remove(item);
  }
}

String use() {
  final Holder h = Holder();
  h.single = Counter(10, 'c');

  // Through the cell: the counter the holder keeps must change.
  h.touch();
  h.bumper();
  // ..and the absent case must stay quiet rather than panic.
  final Holder empty = Holder();
  empty.touch();

  // Through the map: the list the map holds must change, not a copy.
  h.lists['a'] = <int>[1, 2, 3];
  h.discard('a', 2);
  // ..and a key that is not there is a no-op.
  h.discard('missing', 9);

  return '${h.single!.value}${h.single!.label}'
      '/${h.lists['a']!.join(",")}/${empty.single == null}';
}
