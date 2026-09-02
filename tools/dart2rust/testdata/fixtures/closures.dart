// A fixture for closure literals.
//
// A closure reaching `this` used to be refused outright. Two questions decide
// whether it has to be, and this fixture has a case for each:
//
//   * **Where does it go?** A function-typed *parameter* is `impl Fn(..)` in
//     Rust, which borrows, so a closure written as a call argument lives
//     exactly as long as the call and may hold `this`. One that is returned,
//     or stored in an object being constructed, outlives it and may not.
//   * **What does it ask of `this`?** Reading a field takes a shared borrow,
//     which the enclosing method already holds. Calling a method on `this`
//     hands out the whole object.
//
// Half of the closures under `package:flutter` that reach `this` are call
// arguments (`bin/census_closures.dart`), so this is not a corner case.

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

  /// Reads a field of `this` from a closure written as a call argument. The
  /// borrow lasts exactly as long as `applyTwice` does, so this translates.
  double byFactor(double x) {
    return Closures.applyTwice((double v) => v * factor, x);
  }

  double scaled(double v) {
    return v * factor;
  }

  /// Calls a method on `this` rather than reading a field, which needs the
  /// whole object and not a borrow of one of its fields. Refused.
  double twiceScaled(double x) {
    return Closures.applyTwice((double v) => scaled(v), x);
  }

  /// Reads a field, like `byFactor`, but is **returned** instead of passed.
  /// Nothing bounds how long it lives, so the borrow cannot be given. Refused,
  /// and the pair of them is what shows the position is what decides it.
  double Function(double) scaler() {
    return (double v) => v * factor;
  }
}
