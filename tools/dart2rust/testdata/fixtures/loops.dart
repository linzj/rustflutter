// REFUSES: `identical` on something that is not a reference
//
// `for`, `while`, and `identical`.
//
// The `for` is the shape both front ends see the same way: declarations, a
// condition, updates. Kernel also lowers `for (x in xs)` into this shape, which
// is 405 of `package:flutter`'s 592 `for` statements -- but the analyzer keeps
// the source shape there, so that one is refused on this side and the two only
// meet on the loops they agree about.
//
// `identical` is only answered with `this` on one side. That is the fast path
// at the top of an `operator ==`, where both sides really are references. The
// values below are all different so a wrong pairing cannot pass.

/// A trait to hold the reference. `implements` is not this compiler's shape
/// yet -- only `extends` produces an `impl` -- and upstream's version of this
/// is `Alignment extends AlignmentGeometry`, which is the same thing.
abstract class Rung {
  const Rung();

  int get steps;
}

class Ladder extends Rung {
  const Ladder(this.steps);

  final int steps;

  /// A plain counting loop.
  double climbed() {
    double total = 0.0;
    for (int i = 0; i < steps; i = i + 1) {
      total = total + i.toDouble();
    }
    return total;
  }

  /// A `while` with the same work, so the two lowerings can be compared: a
  /// `for` is a block holding the declaration with the update at the end of the
  /// body, which is exactly this.
  double climbedTheLongWay() {
    double total = 0.0;
    int i = 0;
    while (i < steps) {
      total = total + i.toDouble();
      i = i + 1;
    }
    return total;
  }

  /// Two updates, and a condition that stops before the declaration would.
  double paired() {
    double total = 0.0;
    for (int i = 0, j = 10; i < j; i = i + 1) {
      total = total + 1.0;
      if (total > 3.0) {
        return total;
      }
    }
    return total;
  }

  /// `identical(this, other)` -- the fast path at the top of an `operator ==`,
  /// which is where 140 of upstream's 259 live.
  ///
  /// The parameter is the abstract `Rung`, not `Ladder`, and that is not
  /// decoration: a `Ladder` parameter arrives by value, because a translated
  /// value type is `Copy`, and the address of a copy is not the address of
  /// anything the caller has. Only a reference can be asked this question, so
  /// only a reference is allowed to be.
  bool isThe(Rung other) {
    return identical(this, other);
  }

  /// Refused, and it has to be: `other` is a `Ladder`, so it arrives by value.
  /// Comparing the address of a copy would compile and would always say false.
  bool isTheCopy(Ladder other) {
    return identical(this, other);
  }
}
