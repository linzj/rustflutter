// A fixture for `late` fields.
//
// `late` is the one Dart state Rust has no word for: not null, not a value,
// but *nothing yet*, with a read before the first write an error. `Option<T>`
// holding `None` is the shape of it, and every read unwraps -- which panics
// where Dart throws `LateInitializationError`. The same event, reported by a
// different mechanism, which is the trade already made for index bounds.
//
// Round 52 measured 480 of these and put them down, because it assumed the
// read had to clone and the commonest types (`Animation`, and so `Box<dyn
// Animation>`) are not `Clone`. Unwrapping a *reference* asks for nothing of
// the kind. Only a field held in a cell -- where the borrow may not leave --
// still needs the clone, and that is the case this fixture keeps honest.

/// Something to be `late` about that is not `Copy`, so a read cannot just take
/// the value out.
class Engine {
  const Engine(this.name);

  final String name;

  double run(double x) {
    return x * 2.0;
  }
}

class Machine {
  Machine();

  /// A `Copy` one: the read takes the value out whole, which is what a field
  /// read does anyway.
  late int steps;

  /// A `late final`: assigned once, and never again. Still `Option<T>` -- what
  /// `final` promises is one write, not an early one.
  late final Engine engine;

  void start(Engine e) {
    engine = e;
    steps = 0;
  }

  /// Reads both kinds in one body: the reference one through `as_ref`, the
  /// value one by unwrapping the `Option` itself.
  double advance(double x) {
    steps = steps + 1;
    return engine.run(x);
  }

  int taken() {
    return steps;
  }

  /// A read that only borrows, which is the common case: a method call and a
  /// field read on the thing that was `late`.
  String describe() {
    return 'ran ${engine.name} $steps times';
  }
}
