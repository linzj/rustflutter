// DIFFERS: Kernel lowers `??` to a `Let`, which is not translated yet, so `withFallback` is refused there and emitted by the analyzer front end. 2046 `Let` refusals stand behind this one.
// A fixture for Dart's postfix `!`.
//
// `b!` says "this is not null; crash if I am wrong", and Rust's `unwrap()` says
// the same. Both are still there in a release build, so the translation keeps
// the check rather than replacing it.
//
// The cases below are chosen so that "the check is there" and "the check was
// replaced with a default" give different answers: a fixture that only ever
// passes non-null values would pass either way.

class NullCheck {
  const NullCheck(this.maybe, this.other);

  final double? maybe;
  final double? other;

  /// The plain case.
  double doubled() {
    return maybe! * 2.0;
  }

  /// Two of them in one expression, so an implementation that unwrapped only
  /// the first would still be caught.
  double summed() {
    return maybe! + other!;
  }

  /// `!` on the *result* of something, not on a field.
  double viaCall() {
    return pick(true)! + pick(false)!;
  }

  double? pick(bool first) {
    if (first) {
      return maybe;
    }
    return other;
  }

  /// A null-aware fallback beside a `!`, so the two are not confused: `??`
  /// supplies a default, `!` insists there is no need for one.
  double withFallback(double fallback) {
    return (other ?? fallback) + maybe!;
  }
}
