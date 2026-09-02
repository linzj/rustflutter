// A fixture for cascades.
//
// `Paint()..color = c..style = s` is, in Kernel, "bind the receiver, do the
// steps, produce the binding" -- which is a Rust block expression exactly:
// `{ let mut it = ...; it.color = c; it.style = s; it }`.
//
// The steps set *different* fields to *different* values, because a cascade
// whose steps all did the same thing could not tell a dropped step from a
// duplicated one.

class Paint {
  Paint() ;

  /// Set at the declaration **and** never by the constructor.
  double width = 0.0;
  double alpha = 0.0;
  double blur = 0.0;

  void widen(double by) {
    width = width + by;
  }
}

/// A field set at its declaration **and** by a constructor, so which one wins
/// is observable. Dart says the constructor does. Without a case having both,
/// the order is never tested and a mutation reversing it survives -- which it
/// did, the first time this fixture was written.
///
/// An initialiser list, not a constructor *body*: a body is refused, because it
/// used to be dropped silently and `Tinted(v)` came out ignoring `v` entirely.
class Tinted {
  Tinted(double value) : opacity = value;

  /// Both a declaration value and a constructor that overrides it.
  double opacity = 1.0;

  /// Only a declaration value.
  double tint = 0.5;
}

class Painter {
  const Painter();

  /// One step.
  Paint thin() {
    return Paint()..width = 1.0;
  }

  /// Three steps, each a different field and value: dropping any one changes a
  /// different number.
  Paint styled() {
    return Paint()
      ..width = 2.0
      ..alpha = 3.0
      ..blur = 4.0;
  }

  /// A method call among the writes, so the two kinds of step are both covered.
  Paint widened() {
    return Paint()
      ..width = 5.0
      ..widen(2.0);
  }
}
