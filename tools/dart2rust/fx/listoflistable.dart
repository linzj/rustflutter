// `List.of(x)` where `x` is a translated class that *is* an `Iterable` goes
// through that class's `__to_list`, not its Dart `toList`.
//
// A translated Iterable class gets `__to_list()` -- a name no Dart member
// has -- generated for exactly this. Its own `toList` is a different thing:
// Dart's `Iterable.toList({bool growable = true})` can be overridden, and a
// named parameter becomes *positional* in Rust, so
// `ObserverList.toList({bool growable = true})` is
// `to_list(&self, growable: bool)`. Calling that with no arguments is
// `FocusManager.notifyListeners`, whose Dart is
// `List<ValueChanged<..>>.of(_listeners)`.
//
// The overriding `toList` is declared here on purpose: without it the class
// has nothing but the generated `__to_list` and the bug cannot appear.

class Ring extends Iterable<int> {
  Ring(this._items);

  final List<int> _items;

  @override
  Iterator<int> get iterator => _items.iterator;

  // The override that makes the class's own `to_list` take an argument.
  @override
  List<int> toList({bool growable = true}) => _items.toList(growable: growable);
}

String use() {
  final Ring r = Ring(<int>[3, 1, 2]);
  // The call that went wrong: `of` over a translated Iterable.
  final List<int> copied = List<int>.of(r);
  // ..and the class's own `toList`, which must keep working.
  final List<int> own = r.toList();
  copied.add(9);
  return '${copied.join(",")}/${own.join(",")}/${r.length}';
}
