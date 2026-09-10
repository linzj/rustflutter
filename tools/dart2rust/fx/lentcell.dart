// Two things a trait body has to know about a field it *lends*.
//
// 1. Inside a trait body a field is an accessor, and the cell is what the
//    trait hands out. A *read* went through `_items_cell()`; lending it did
//    not -- it spelled `__me._items`, and on a handle that name is a method,
//    not a field ("attempted to take value of method", E0615). The closures
//    inside a super function are trait bodies too, which is where the
//    gallery's two sit.
//
// 2. A handle to a comparable thing compares as the thing does. A class that
//    `implements Comparable<T>` gets the impl on the *struct*, and every
//    list of it holds handles, so `list.sort()` asks
//    `Rc<Item>: Comparable<Rc<Item>>` and nothing answered (E0599 on
//    `sort_natural`). The bound that answers is `Comparable<Rc<T>>`, not
//    `Comparable<T>`: a class-typed parameter is lent as a handle, so what
//    the class implements already takes one.
//
// `_SliverAnimatedMultiBoxAdaptorState.insertItem`/`removeItem` are both,
// one behind the other: they lend `_incomingItems`/`_outgoingItems` from
// inside a closure and sort them.
//
// **This fixture tests (2), not (1).** It emits `_items_cell()` and so walks
// through (1)'s code, but it does not go red without it: in a program this
// small the mixin's body is flattened into `Left` and `Right`, so the
// closure's `this` is the struct and `_items` really is a field there. The
// gallery's is a trait handle because the closure is held by an animation
// that outlives the call. (1) is measured in the gallery alone -- both
// E0615s gone at ws973 -- and has no fixture; that gap is written down
// rather than papered over.

class Item implements Comparable<Item> {
  Item(this.rank);

  // Mutable, so the class is not a value type: the gallery's `_ActiveItem`
  // is held behind a handle, and it is `Rc<_ActiveItem>` the list holds and
  // the bound asks about. With a value type the list is a `Vec<Item>` and
  // the struct's own impl already answers -- nothing to see.
  int rank;

  @override
  int compareTo(Item other) => rank - other.rank;

  // A closure that calls one of this class's own methods is what makes the
  // class *counted* (`_countedClass`), and a counted class is held behind a
  // handle -- which is why the gallery's list is a `Vec<Rc<_ActiveItem>>`
  // and the bound is asked about the handle. Without this the list is a
  // `Vec<Item>` and the struct's own impl already answers.
  String label() {
    final String Function() render = () => _text();
    return render();
  }

  String _text() => '$rank';

  @override
  String toString() => _text();
}

mixin Bucket {
  final List<Item> _items = <Item>[];

  // The closure is *stored* before it runs, so it escapes with `this` and
  // captures the trait handle rather than a copy of the struct. That is
  // what makes its `this` an `Rc<dyn Bucket>`, where a field is an accessor
  // -- the gallery's closure is held by an animation the same way.
  void Function()? _pending;

  void insert(Item item) {
    _pending = () {
      _items.add(item);
      // A handle's list, sorted.
      _items.sort();
      // ..and lent to a callee that mutates it in place.
      _drop(_items, Item(0));
    };
    _flush();
  }

  void _flush() {
    final void Function()? body = _pending;
    _pending = null;
    if (body != null) {
      body();
    }
  }

  void _drop(List<Item> from, Item unwanted) {
    from.removeWhere((Item it) => it.rank == unwanted.rank);
  }

  String describe() => _items.join('/');
}

class Left with Bucket {}

class Right with Bucket {}

String use() {
  // Through the mixin's own type, so its members stay on the trait rather
  // than devirtualising into the two classes.
  final Bucket left = Left();
  final Bucket right = Right();
  left.insert(Item(3));
  left.insert(Item(1));
  left.insert(Item(0));
  left.insert(Item(2));
  right.insert(Item(9));
  // `label()` is called so TFA keeps it: the closure inside it is what makes
  // `Item` counted, and a shaken-away closure makes it a value type again.
  return '${left.describe()}/${right.describe()}/${Item(7).label()}';
}
