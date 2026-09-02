// A fixture for `super`.
//
// Rust has no `super`. Once an impl overrides a trait's default method the
// default becomes unreachable -- `Trait::name(self)` dispatches straight back
// to the override and the program hangs. And calling super from inside an
// override of the *same name* is not an edge case in Flutter: every one of the
// 435 super calls in painting/ and rendering/ is exactly that shape.
//
// So the numbers below are chosen so that "the super call reached the base" and
// "the super call came back to the override" give different answers rather than
// a stack overflow, which a test cannot distinguish from a hang.

abstract class Shape {
  const Shape();

  /// The base's own answer. An override that calls `super.area()` must reach
  /// *this* body.
  double area(double scale) {
    return 100.0 * scale;
  }

  double perimeter();
}

class Doubled extends Shape {
  const Doubled();

  /// Adds to whatever the base said. If `super.area` came back here instead,
  /// this would recurse forever rather than return 201.
  @override
  double area(double scale) {
    return super.area(scale) + 1.0;
  }

  @override
  double perimeter() {
    return 7.0;
  }
}

class Untouched extends Shape {
  const Untouched();

  /// Does not override `area`, so it gets the trait's default -- which must be
  /// the same body the base declared.
  @override
  double perimeter() {
    return 3.0;
  }
}

/// A base whose method cannot be translated, so a subclass calling `super` on
/// it must be stopped rather than pointed at a function nobody wrote.
abstract class Untranslatable {
  const Untranslatable();

  /// Uses a `for` loop, which is not translated yet, so this method is
  /// refused. It was a cascade until round twenty-three made cascades work --
  /// a fixture whose "untranslatable" construct becomes translatable stops
  /// testing what it was for, and the two front ends disagreeing is what said
  /// so.
  String describe() {
    var out = '';
    for (var i = 0; i < 2; i = i + 1) {
      out = out + 'x';
    }
    return out;
  }

  double size();
}

class UsesIt extends Untranslatable {
  const UsesIt();

  /// Calls super on a method the base could not translate. The compiler must
  /// refuse this, not emit `untranslatable_super_describe(self)`.
  @override
  String describe() {
    return super.describe();
  }

  @override
  double size() {
    return 5.0;
  }
}

/// `Object` is the one base that is never in any file: every Dart class
/// already extends it, and there is nothing to emit for it. `super.toString()`
/// was refused for 198 members on that ground, which was the wrong ground --
/// Dart's own `Object.toString` returns `Instance of 'Foo'`, and so does this.
class NamesItself {
  const NamesItself();

  String describe() {
    return super.toString();
  }
}
