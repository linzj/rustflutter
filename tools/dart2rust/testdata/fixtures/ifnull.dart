// A fixture for `a ?? b`.
//
// Dart's `??` is short-circuit: `b` is evaluated only when `a` is null. Rust's
// `unwrap_or(b)` evaluates it **always**; `unwrap_or_else(|| b)` does not. So
// the eager form is right only for a value with no effects, and of 6764 `??` in
// package:flutter only 23% qualify -- six of them have a `throw` on the right,
// where eager evaluation does not give a wrong answer, it throws every time.
//
// `boom()` is how that becomes observable: if the right side is evaluated when
// it should not be, the test panics instead of quietly passing. A fixture whose
// default was an ordinary number could not tell the two forms apart at all.

class IfNull {
  const IfNull(this.value);

  final double? value;

  /// Throws if it is ever called. Standing in for the calls, allocations and
  /// `throw`s that make up 77% of upstream's right-hand sides.
  double boom() {
    // `assert` rather than `throw`: throw is not translated yet, and a fixture
    // that needs an untranslated construct tests nothing. Tests run in debug,
    // where a failed assert panics.
    assert(false, 'the right side of ?? was evaluated');
    return 0.0;
  }

  /// A literal on the right: safe to evaluate eagerly, and it reads better.
  double withLiteral() {
    return value ?? 1.0;
  }

  /// A call on the right: must not run when `value` is present.
  double withCall() {
    return value ?? boom();
  }

  /// Nested, so the inner one has to be restored too.
  double nested(double? second) {
    return value ?? second ?? 2.0;
  }
}
