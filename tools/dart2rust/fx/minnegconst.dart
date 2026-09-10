// `math.min(a, b)` is `a.min(b)` in Rust, so `a` is a *receiver* -- and Rust
// resolves a method before it defaults an unsuffixed float literal, so a
// literal receiver has to say `f64` outright (E0689, already handled for a
// bare literal). `-_kFlingVelocity` is a literal too: the front end folds a
// negated `const double` to one, and the negation kept the suffix from being
// written (`_handleDragEnd` in reply's `adaptive_nav.dart`).
import 'dart:math' as math;

const double _kFlingVelocity = 2.0;

String use() {
  final double v = 3.5;
  // The receiver is the *negation* of a const double.
  final double a = math.min(-_kFlingVelocity, -v);
  // ..and of a literal written in the call.
  final double b = math.min(-1.5, v);
  // A bare literal receiver, which already worked: kept so the fixture
  // says both are covered.
  final double c = math.max(0.25, v);
  return '$a/$b/$c';
}
