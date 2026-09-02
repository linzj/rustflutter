// A fixture for `assert`.
//
// The thing worth checking is not that an assert compiles -- an assert that is
// silently dropped also compiles, and every test that does not trip it passes.
// So each check below has a partner that trips it, because "the assert is
// there" and "the assert is gone" are otherwise the same observation.

class Asserts {
  /// The constructor's own check, from the initialiser list.
  Asserts(this.value) : assert(value >= 0.0, 'value must not be negative');

  final double value;

  /// A check in a body, with a message.
  double halved() {
    assert(value > 0.0, 'halving zero is not useful');
    return value / 2.0;
  }

  /// A check whose message is an interpolation, which is not translated -- the
  /// condition is the contract, the message is diagnostics.
  double doubled() {
    assert(value < 1000.0, 'value $value is too large to double');
    return value * 2.0;
  }

  /// No message at all.
  double squared() {
    assert(value >= 0.0);
    return value * value;
  }
}
