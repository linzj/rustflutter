// A fixture for closure literals.
//
// Only the ones that capture nothing or read outer locals are translated. A
// closure reaching `this` outlives the call that made it while `this` is a
// borrow, and that is an ownership arrangement rather than a translation --
// 60% of package:flutter's closures and 45% of the gallery's, so a round of its
// own. The last method here exists to be refused, and there is a test for that.

class Closures {
  /// A static method rather than a top-level function: top-level functions are
  /// not translated yet (964 refusals of their own), and a fixture that needs
  /// an untranslated construct tests nothing.
  static double applyTwice(double Function(double) f, double x) {
    return f(f(x));
  }

  const Closures(this.factor);

  final double factor;

  /// Captures nothing: a plain function in Rust terms.
  double doubled(double x) {
    return Closures.applyTwice((double v) => v + 1.0, x);
  }

  /// Reads an outer local. Rust borrows it; nothing has to be said.
  double scaledBy(double amount, double x) {
    return Closures.applyTwice((double v) => v * amount, x);
  }

  /// Two captured locals, so pairing them wrongly changes the answer.
  double blend(double a, double b, double x) {
    return Closures.applyTwice((double v) => v * a + b, x);
  }

  /// A two-parameter closure whose arguments are **not** interchangeable: with
  /// one parameter, reversing the list is a no-op and a mutation that shuffles
  /// them survives. Round twenty-one learned the same thing about `super`.
  static double combine(double Function(double, double) f) {
    return f(10.0, 3.0);
  }

  double subtracted() {
    return Closures.combine((double a, double b) => a - b);
  }

  /// Reaches `this`, so it must be refused rather than translated with a
  /// borrow that cannot live long enough.
  double byFactor(double x) {
    return Closures.applyTwice((double v) => v * factor, x);
  }
}
