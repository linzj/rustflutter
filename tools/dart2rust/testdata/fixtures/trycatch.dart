// A fixture for `try { .. } catch (e) { .. }`.
//
// This is the other half of Result: `?` carries a failure outward, a catch
// stops it. Which makes the load-bearing question "does the catch actually
// stop it" -- a `?` written inline would return from the enclosing method,
// escaping the very catch meant to hold it, and the method would still
// compile because its signature already says Result.
//
// So `recovered` returns a plain double. If the failure escaped, that method
// could not have a plain return type at all, and if the catch merely ran
// without stopping the failure the number would be wrong rather than absent.
//
// REFUSES: return inside a try body
//
// 155 of upstream's 174 catch clauses catch `Object` -- `catch (e)` with no
// type -- so the untyped case is the one that matters most.

class Guarded {
  const Guarded(this.limit);

  final double limit;

  double checked(double value) {
    if (value > limit) {
      throw RangeError('over the limit');
    }
    return value;
  }

  /// Catches, so this method does **not** return a Result. That is the whole
  /// test: the failure stops here.
  double recovered(double value) {
    double result = 0.0;
    try {
      result = checked(value);
    } catch (e) {
      result = -1.0;
    }
    return result;
  }

  /// A catch that binds a stack trace and never reads it. Free, since ignoring
  /// something a Result does not carry costs nothing.
  double recoveredWithUnusedTrace(double value) {
    double result = 0.0;
    try {
      result = checked(value);
    } catch (e, stack) {
      result = -2.0;
    }
    return result;
  }

  /// Does not catch, so the failure keeps travelling and this one does return
  /// a Result. The pair is the point: catching and not catching have to give
  /// different signatures.
  double uncaught(double value) {
    return checked(value) + 1.0;
  }

  /// Refused, and this is the fixture's sharpest case. The try body is emitted
  /// as a closure, so this `return` would leave the *closure* and the method
  /// would go on to the line below it -- returning 0.0 for every value. That
  /// compiles, which is exactly why it has to be refused instead.
  double returnsFromInsideTry(double value) {
    try {
      return checked(value);
    } catch (e) {
      return -3.0;
    }
  }
}
