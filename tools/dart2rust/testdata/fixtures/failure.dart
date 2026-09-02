// A fixture for `throw` becoming `Result<T, E>`.
//
// The decision is that failure travels in the return value rather than by
// unwinding. It was measured before it was made: 717 members of
// package:flutter throw directly, 5906 (20%) end up returning Result once that
// propagates, and 709 of 721 throw exactly one error type -- so the error is a
// concrete type per function and no enum is needed.
//
// `try/catch` cuts almost nothing in that corpus: 20 members out of 5906. So
// the propagation runs, and each case below is paired with a success, because
// a test that only ever fails cannot tell `Err` from a panic.

class Bounds {
  const Bounds(this.limit);

  final double limit;

  /// Throws directly. Its Rust signature becomes `Result<f32, RangeError>`.
  double checked(double value) {
    if (value > limit) {
      throw RangeError('over the limit');
    }
    return value;
  }

  /// Calls one that can fail, so the failure spreads here. Nothing in the Dart
  /// says so -- it is computed.
  double doubled(double value) {
    return checked(value) * 2.0;
  }

  /// Two hops. One pass over the call graph would find `doubled` and miss this,
  /// the same way it did for `&mut self` in round twelve.
  double quadrupled(double value) {
    return doubled(value) * 2.0;
  }

  /// Cannot fail, and must not be given a `Result` it does not need.
  double halved(double value) {
    return value / 2.0;
  }
}
