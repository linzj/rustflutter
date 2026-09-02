// Top-level functions.
//
// This compiler translated classes only until round 36, and upstream has 198
// top-level functions under `package:flutter/` called 522 times -- so every
// one of those calls took the member holding it down with it. A top-level
// function needs no owner in either language; it is a free `fn`.
//
// The numbers below are all different so a call wired to the wrong function
// cannot pass.

double halve(double v) {
  return v / 2.0;
}

double thrice(double v) {
  return v * 3.0;
}

class Gauge {
  const Gauge(this.value);

  final double value;

  double reduced() {
    return halve(value);
  }

  /// A top-level function calling another one, and a class calling both.
  double amplified() {
    return thrice(halve(value));
  }
}
