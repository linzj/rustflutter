// `throw` where a value was wanted, and `break` out of a loop.
//
// Rust has neither construct under those names and needs neither: `return
// Err(e)` is an expression of type `!`, so it fits wherever a value was
// expected, and a labelled block is exactly what Kernel's `break` targets --
// Dart's `break` out of a loop arrives already wrapped in one.
//
// The numbers are all different so a wrong pairing cannot pass: a `break` that
// left one loop too many, or a throw that returned instead of failing, would
// give one of the other answers rather than the right one.

class Sieve {
  const Sieve(this.limit);

  final int limit;

  /// `throw` in expression position -- the right side of `??`.
  int atLeastOne(int? given) {
    return given ?? (throw RangeError('nothing given'));
  }

  /// `break` leaves the loop, and the statement after it still runs.
  int firstOver(int bound) {
    int found = -1;
    for (int i = 0; i < limit; i = i + 1) {
      if (i > bound) {
        found = i;
        break;
      }
      found = -2;
    }
    return found;
  }

  /// Both in one loop, which is the case that needs the labels to be told
  /// apart: the `continue` leaves the body block and lands on the update, and
  /// the `break` has to cross that block to leave the loop -- which Rust will
  /// not let an unlabelled `break` do.
  int firstOddOver(int bound) {
    int found = -1;
    for (int i = 0; i < limit; i = i + 1) {
      if (i % 2 == 0) {
        continue;
      }
      if (i > bound) {
        found = i;
        break;
      }
    }
    return found;
  }

  /// `continue` is the other half: Kernel spells it as a `break` out of a label
  /// wrapped around the body rather than around the loop.
  int oddsBelow() {
    int total = 0;
    for (int i = 0; i < limit; i = i + 1) {
      if (i % 2 == 0) {
        continue;
      }
      total = total + i;
    }
    return total;
  }
}
