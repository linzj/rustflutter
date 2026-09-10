// An operator whose body assigns to its own parameter. A method's parameter
// takes `mut` when the body writes to it; an operator's did not, and rustc
// said so ("cannot assign to immutable argument", E0384).
//
// `Priority operator +(int offset)` in the scheduler is written that way:
// it clamps `offset` before using it.
class Level {
  const Level(this.n);

  final int n;

  Level operator +(int offset) {
    if (offset > 5) {
      offset = 5 * offset.sign;
    }
    return Level(n + offset);
  }

  /// A binary operator that writes to its parameter and reads it after,
  /// and one that does not, so both spellings are covered.
  Level operator -(int offset) => Level(n - offset);

  @override
  String toString() => 'L$n';
}

String use() {
  const Level base = Level(10);
  return '${base + 2}/${base + 40}/${base - 3}';
}
