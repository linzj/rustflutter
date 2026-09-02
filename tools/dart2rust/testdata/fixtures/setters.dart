// A fixture for `set x(v)`.
//
// A setter is a call, not a write. Dart spells it `a.x = 1` and Rust spells it
// `a.set_x(1)`, and which of the two an assignment is depends on how the
// receiver's class declared `x` -- so the distinction has to be made where that
// is known, not guessed at the assignment.
//
// The getter beside each setter is the point: `get x` and `set x` are one name
// in Dart and two in Rust, and the mutability analysis keys on names. Keying on
// the Dart name would make the getter `&mut self` too, which the reading tests
// below would refuse to compile against.

class Temperature {
  Temperature(this._celsius);

  double _celsius;

  /// Reads only. Must stay `&self`.
  double get celsius => _celsius;

  /// Writes. Must become `set_celsius(&mut self, ..)`.
  set celsius(double value) {
    _celsius = value;
  }

  /// A setter with real logic, not a plain field write -- which is why a setter
  /// cannot be translated as an assignment.
  set fahrenheit(double value) {
    _celsius = (value - 32.0) / 1.8;
  }

  double get fahrenheit => _celsius * 1.8 + 32.0;

  /// Assigns through this object's own setter. Mutating by contagion.
  void warmBy(double degrees) {
    celsius = celsius + degrees;
  }

  /// A compound assignment through a setter: the "current value" comes from the
  /// getter, since there may be no field of that name at all.
  void heatUp() {
    fahrenheit += 18.0;
  }

  /// Reads through both getters. Stays `&self`.
  double difference() {
    return fahrenheit - celsius;
  }
}
