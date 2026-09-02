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

  /// The fixture's sharpest case. The try body is emitted as a closure, so a
  /// plain `return` here would leave the *closure* and the method would carry
  /// on -- returning whatever came after, and compiling while it did it. So the
  /// closure carries the control flow out as a value instead. If it did not,
  /// this would return 0.0 for every input rather than the two below.
  double returnsFromInsideTry(double value) {
    try {
      return checked(value);
    } catch (e) {
      return -3.0;
    }
  }

  /// A `return` on only *one* path through the try body. The other path falls
  /// off the end of the closure, which is what `Ok(None)` is for -- without it
  /// this case and the one above cannot be told apart.
  double returnsOnOnePath(double value) {
    double result = 0.0;
    try {
      if (value < 0.0) {
        return -4.0;
      }
      result = checked(value);
    } catch (e) {
      result = -5.0;
    }
    return result;
  }
}

/// `finally` on its own class, because showing that the finalizer ran needs a
/// field that can change and `Guarded` is const.
class Tally {
  Tally(this.limit);

  final double limit;
  int runs = 0;

  double checked(double value) {
    if (value > limit) {
      throw RangeError('over the limit');
    }
    return value;
  }

  /// Three ways out of the body -- returned early, returned a value that may
  /// throw, and threw -- and `runs` has to go up on all three. A finalizer that
  /// ran only on the ordinary path would pass a test that used just one of
  /// them, which is why the test uses all three in sequence.
  double counted(double value) {
    try {
      if (value < 0.0) {
        return -6.0;
      }
      return checked(value);
    } finally {
      runs = runs + 1;
    }
  }
}
