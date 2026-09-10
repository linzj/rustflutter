// A callable into a *nullable* function slot has to be unsized inside the
// `Some`, not around it.
//
// `Option<Rc<fn item>>` is not `Option<Rc<dyn Fn(..)>>` and never coerces to
// it: unsizing applies to the pointer, and Rust will not reach through an
// `Option` to do it. `CupertinoRoute`'s constructor passes the top-level
// `_buildCupertinoDialogTransitions` to a `RouteTransitionsBuilder?`
// parameter and got `expected `dyn Fn`, found fn item` around a
// `Option<Rc<...>>`.
//
// Three callable shapes, because they arrive as different Rust types and
// only the first is a `dyn Fn` already:
//   * a top-level function -- a bare `fn` item;
//   * an instance method tear-off -- a closure capturing `this`;
//   * a closure literal.

typedef Trans = String Function(int);

String plain(int v) => 'p$v';

class Holder {
  Holder({this.trans, this.also, this.written});

  final Trans? trans;
  final Trans? also;
  final Trans? written;

  String run(int v) {
    final List<String> out = <String>[];
    out.add(trans == null ? '-' : trans!(v));
    out.add(also == null ? '-' : also!(v));
    out.add(written == null ? '-' : written!(v));
    return out.join(',');
  }
}

class Maker {
  Maker(this.tag);

  final String tag;

  // An instance method: its tear-off captures `this`, so it is a closure.
  String stamped(int v) => '$tag$v';

  Holder make() =>
      Holder(trans: plain, also: stamped, written: (int v) => 'w$v');
}

/// The shape the gallery actually has: `??` chooses between a nullable
/// callable and a default one, and the default is a bare `fn` item.
/// `CupertinoDialogRoute` writes
/// `transitionBuilder: transitionBuilder ?? _buildCupertinoDialogTransitions`.
Holder defaulted(Trans? given) => Holder(trans: given ?? plain);

/// ..and the path it actually travels: a `super(..)` call in an initializer
/// list, which coerces its arguments somewhere else again.
/// `CupertinoDialogRoute` is `super(transitionBuilder: transitionBuilder ??
/// _buildCupertinoDialogTransitions)`, and the class is generic on top.
class Base<T> {
  Base({this.trans});

  final Trans? trans;

  String show(int v) => trans == null ? '-' : trans!(v);
}

class Derived<T> extends Base<T> {
  Derived({Trans? given}) : super(trans: given ?? plain);
}

String use() {
  final Holder full = Maker('m').make();
  // ..and the absent case, so the `None` side is exercised too.
  final Holder empty = Holder();
  // ..and both sides of the `??`.
  final Holder fell = defaulted(null);
  final Holder kept = defaulted((int v) => 'k$v');
  final Base<int> supered = Derived<int>();
  final Base<int> superKept = Derived<int>(given: (int v) => 's$v');
  return '${full.run(3)}/${empty.run(3)}/${fell.run(3)}/${kept.run(3)}'
      '/${supered.show(3)},${superKept.show(3)}';
}
