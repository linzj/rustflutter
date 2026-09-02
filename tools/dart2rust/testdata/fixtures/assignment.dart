// A fixture for assigning a local.
//
// Rust needs `let mut` at the declaration, and whether one is needed is a fact
// about the whole body rather than about the line. So each case below is paired
// with a local that is *not* reassigned: marking everything `mut` would compile
// too, and a test that only checked the reassigned ones would pass on that.
//
// The compound cases matter for a second reason: analyzer keeps `x += 1` as one
// node while Kernel has already rewritten it to `x = x + 1`. Two front ends
// must still arrive at the same answer.
//
// No loops here on purpose -- `while` is not translated yet, and a fixture that
// needs an untranslated construct tests nothing.

class Assignment {
  const Assignment(this.step);

  final double step;

  /// Plain reassignment. `total` is reassigned, `factor` never is.
  double accumulate() {
    double total = 0.0;
    final double factor = 2.0;
    total = total + step * factor;
    total = total + step;
    return total;
  }

  /// Compound assignment, which Kernel has already expanded to `x = x + y`.
  double compound() {
    double total = 10.0;
    total += step;
    total -= 1.0;
    total *= 2.0;
    return total;
  }

  /// Assigned in one branch only -- still needs `mut`.
  double branch(bool big) {
    double value = 1.0;
    if (big) {
      value = 100.0;
    }
    return value;
  }

  /// A parameter reassigned in the body. Rust spells that `mut start: f32` in
  /// the signature, which is the parameter's own declaration.
  double shadow(double start) {
    start = start + 1.0;
    return start;
  }
}
