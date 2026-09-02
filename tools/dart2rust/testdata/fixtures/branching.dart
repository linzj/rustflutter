// `switch`, and the small library calls beside it.
//
// Rust's `match` is not an approximation of Dart's `switch` on an enum -- it is
// the same construct, exhaustiveness included. What needed deciding is the
// `break` Dart puts at the end of every case: it means "leave the switch",
// which a match arm does by ending, so it is dropped. Only the one at the end.
// One in the middle would be leaving early, which an arm cannot do, and is
// refused instead.
//
// 628 switches in `package:flutter/`, almost all on an enum, 20 with a default,
// none with an empty fall-through case.
//
// REFUSES: break out of a switch from inside a case
//
// Every number below is different from the others so a case matched to the
// wrong arm cannot pass.

import 'dart:math' as math;

enum Corner { topLeft, topRight, bottomLeft, bottomRight }

class Placement {
  const Placement(this.width, this.height);

  final double width;
  final double height;

  /// Exhaustive, no default -- which Rust checks and Dart does not.
  double offsetX(Corner corner) {
    switch (corner) {
      case Corner.topLeft:
        return 0.0;
      case Corner.topRight:
        return width;
      case Corner.bottomLeft:
        return 0.0;
      case Corner.bottomRight:
        return width;
    }
  }

  /// Two values on one arm, a default, and a `break` at the end of each case.
  double depth(Corner corner) {
    double result = 0.0;
    switch (corner) {
      case Corner.topLeft:
      case Corner.topRight:
        result = 3.0;
        break;
      case Corner.bottomLeft:
        result = 11.0;
        break;
      default:
        result = 29.0;
    }
    return result;
  }

  /// `max` and `clamp`, which Rust spells the same way for floats. `max` is
  /// one spelling for integers too, since `f32::max` is inherent and `Ord::max`
  /// covers the rest -- 372 calls upstream.
  double bounded(double value) {
    return math.max(value.clamp(width, height).toDouble(), 1.0);
  }

  /// Refused. The `break` is not the last statement of its case, so it means
  /// "leave the switch early" -- which a Rust match arm cannot do. Dropping it
  /// would compile and would run the line after it.
  double leavesEarly(Corner corner) {
    double result = 0.0;
    switch (corner) {
      case Corner.topLeft:
        if (width > 0.0) {
          break;
        }
        result = 41.0;
      default:
        result = 43.0;
    }
    return result;
  }
}
