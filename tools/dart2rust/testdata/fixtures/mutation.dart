// A fixture for writing your own fields.
//
// Rust needs `&mut self` on a method that assigns a field, and `&mut` spreads:
// a method that calls a mutating method on itself is mutating too. That spread
// is a fixpoint, not one pass -- `outer` calls `middle` calls `inner`, and only
// `inner` writes.
//
// Each case is paired with a method that does *not* mutate, because making
// every method `&mut self` compiles as well and would pass a test that only
// looked at the mutating ones.

class Counter {
  Counter(this.value, this.step);

  double value;
  final double step;

  /// Writes a field: `&mut self`.
  void bump() {
    value = value + step;
  }

  /// Reads only: stays `&self`.
  double doubled() {
    return value * 2.0;
  }

  /// Two hops. Declared *before* `middle` on purpose: in declaration order a single pass
  /// reaches `middle` only after it has passed `outer`, so it leaves this one
  /// as `&self`. With the order the other way round the fixpoint is invisible
  /// and a mutation removing it survives -- which it did, the first time.
  void outer() {
    middle();
  }

  /// Calls a mutating method. Mutating by contagion, one hop.
  void middle() {
    bump();
  }

  /// Calls only a non-mutating method, so it stays `&self`.
  double quiet() {
    return doubled() + 1.0;
  }

  /// A compound write.
  void scale(double by) {
    value *= by;
  }
}
