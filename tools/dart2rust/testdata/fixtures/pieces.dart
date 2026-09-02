// String interpolation, a function used as a value, a local function, and an
// assignment used for its value.
//
// Four small things that share one property: Rust already has each of them, so
// none needs a construct invented. `'$a and $b'` is `format!`, a static
// function used as a value is the function's own name, a local function is a
// closure bound to a local, and `x = v` used for its value is the value bound
// and produced -- Rust's assignment produces `()`.
//
// The helper is a *static method* rather than a top-level function because
// this compiler does not emit top-level functions yet; that is a separate
// refusal and would swallow this fixture whole.
//
// Note what is not here: a closure that captures `this`. That one is the
// ownership problem, and putting it here would hide it rather than show it.

class Label {
  const Label(this.name, this.count);

  final String name;
  final int count;

  static double twice(double x) => x * 2.0;

  /// `format!`, with the literal pieces becoming the pattern.
  String describe() {
    return '$name has $count';
  }

  /// A literal brace has to survive, since `format!` reads braces.
  String braced() {
    return '{$name}';
  }

  /// A static method used as a value. Nothing is captured, so none of the
  /// ownership question an instance tear-off raises applies.
  double doubled(double x) {
    final double Function(double) f = Label.twice;
    return f(x);
  }

  /// A local function, which is a closure bound to a local.
  double stepped(double x) {
    double step(double v) {
      return v + 1.0;
    }

    return step(step(x));
  }

  /// `total = ..` used for its value, inside the argument of another call.
  /// 7.0 doubled is 14.0, plus the 7.0 the assignment left behind.
  double running(double x) {
    double total = 0.0;
    return Label.twice(total = x + 3.0) + total;
  }
}
