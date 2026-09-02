// A constructor with a body, a factory, and a write through a field of `this`.
//
// Rust has no constructor phase to run statements in, and does not need one:
// the value is built into a local, the body runs against that local, and the
// local is returned. `this` inside the body is that local.
//
// A body used to be refused rather than dropped, and that was right at the
// time: `Shade(v) { opacity = v; }` with the body dropped compiles and
// ignores its argument. The values below are chosen so that the dropped-body
// version would give the declaration's default instead of the argument.

class Shade {
  /// A body that overwrites what the field declaration said. If the body were
  /// dropped this would be 1.0 for every argument.
  Shade(double v) {
    opacity = v * 2.0;
  }

  /// A factory is an associated function returning Self, which is what Dart's
  /// is -- `Shade.faint()` and `Shade::faint()` are the same call.
  factory Shade.faint() {
    return Shade(0.05);
  }

  double opacity = 1.0;
}

class Slot {
  Slot(this.tint);

  Shade tint;

  /// A write through a field of `this`: `self.tint.opacity = v` in Rust, which
  /// needs `&mut self` and nothing else. Through a *parameter* it would need
  /// `&mut` on the parameter and on every call site, including in other files,
  /// so that one is still refused.
  void fade(double v) {
    tint.opacity = v;
  }
}
