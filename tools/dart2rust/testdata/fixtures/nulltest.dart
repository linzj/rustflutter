// A fixture for `x == null`.
//
// Rust asks the question differently: a nullable value is an `Option`, and the
// test is `x.is_none()` rather than a comparison against a null that does not
// exist. Both front ends have to land on the same answer -- Kernel hands over
// an `EqualsNull` because the CFE already recognised the shape, while analyzer
// gives an ordinary `==` against a null literal.
//
// Each case is paired with its opposite, because a test that only ever passes
// non-null values cannot tell `is_none` from `is_some`.

class Maybe {
  const Maybe(this.value, this.other);

  final double? value;
  final double? other;

  bool get isMissing {
    return value == null;
  }

  /// `!=`, which Kernel wraps in a negation.
  bool get isPresent {
    return value != null;
  }

  /// `null` on the left, which Dart allows and which reads the same.
  bool get missingOnTheLeft {
    return null == other;
  }

  /// Two of them, so an implementation that only looked at the first would be
  /// caught.
  bool get bothMissing {
    return value == null && other == null;
  }

  /// A null test guarding a null assertion -- the shape upstream uses most.
  double resolve(double fallback) {
    if (value == null) {
      return fallback;
    }
    return value!;
  }
}
