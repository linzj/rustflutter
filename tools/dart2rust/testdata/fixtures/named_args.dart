// A fixture whose whole purpose is to tell a right answer from a plausible one.
//
// Rust has no named arguments, so dart2rust flattens a named call to a
// positional one. The dangerous way to do that is in *call-site* order, which
// works on every call that happens to name its arguments in declaration order
// -- which is most of them. So the calls below deliberately do not.
//
// The weights are powers of ten so that a wrong permutation cannot land on the
// right total by accident.

class NamedArgs {
  const NamedArgs(this.a, this.b, this.c);

  final double a;
  final double b;
  final double c;

  double weigh({double first = 1.0, double second = 2.0, double third = 4.0}) {
    return a * first + b * second + c * third;
  }

  /// Named in an order that is not the declaration's.
  ///
  /// Correct: first=1, second=10, third=100.
  /// Call-site order would give first=100, second=1, third=10 -- a different
  /// number, which is the point.
  double outOfOrder() {
    return weigh(third: 100.0, first: 1.0, second: 10.0);
  }

  /// `second` is omitted and must fall back to its declared default of 2.0,
  /// not to zero and not to the next argument along.
  double withOmission() {
    return weigh(first: 1.0, third: 1.0);
  }

  /// All defaults: 1, 2, 4.
  double allDefaults() {
    return weigh();
  }
}
