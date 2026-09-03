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

  /// A callee that **keeps** what it is given, rather than calling it and
  /// being done. `applyTwice` calls its closure and returns; this one puts it
  /// somewhere that outlives the call, so a closure reaching `this` cannot
  /// borrow for it and is refused.
  ///
  /// The two together are the point: being an argument is not the question.
  /// The question is whether the callee is finished with it -- measured at
  /// 394 of 1234 across the package, and they are `addListener`,
  /// `scheduleMicrotask` and `Timer`.
  static double Function(double) keep(double Function(double) f) {
    return f;
  }

  double byRemembering(double x) {
    final double Function(double) f = Closures.keep((double v) => v * factor);
    return f(x);
  }

  /// A **method used as a value**: `applyTwice(scaled, x)` hands the method
  /// over without calling it. In Rust that is a closure that calls it, so it
  /// is the same question as any other closure and gets the same answer --
  /// here it is an argument, a borrowed position, so it may borrow `this`.
  double byTearOff(double x) {
    return Closures.applyTwice(scaled, x);
  }

  /// Reads a `final` field and is **returned**, so it outlives the call.
  ///
  /// It cannot hold `this`, and it does not have to: `factor` is `final`, so a
  /// copy taken when the closure is made is the same value a read at call time
  /// would give. That is what makes copying sound here and not in general --
  /// a field that can change would give two different answers.
  double Function(double) scaler() {
    return (double v) => v * factor;
  }
}

/// A **mutable** field that a closure both reads and writes, from a closure
/// that outlives the call.
///
/// A copy would be wrong: `count` changes, and the closure and the object have
/// to see the same one. So the field lives in a cell they both hold a handle
/// to -- which is what "shared" means here, and why it is only for the mutable
/// ones. A `final` field is cheaper copied, and round 97 does that.
class Ticks {
  Ticks();

  int count = 0;

  void Function() counter() {
    return () {
      count = count + 1;
    };
  }

  int seen() {
    return count;
  }
}
