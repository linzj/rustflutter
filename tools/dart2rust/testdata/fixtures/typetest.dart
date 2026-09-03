// A fixture for `is`.
//
// Dart asks a value what it is at run time. Rust does not, and a trait object
// cannot be asked at all unless the trait says it can -- so every translated
// trait inherits `DartAny`, whose single method hands out a `&dyn Any`, and
// `x is Foo` becomes a downcast to the *struct* `Foo`.
//
// Which is why the two halves are not the same size. `s is Tile` asks
// whether the value is one concrete thing, and `Any` answers that exactly.
// `s is Figure` asks whether it implements a trait, and `Any` cannot answer
// that at all -- there is no list of traits to consult -- so it stays refused.
// 747 `is` expressions under the package, and the concrete targets are the
// large majority (`bin/census_is.dart`).
//
// `implements`, which is how real code reaches an interface. When this
// fixture was first written it said `extends`, because `implements Figure`
// produced no `impl Figure for Tile` at all -- the IR carried a superclass
// and a mixin list and no interface list. Round 108 gave it one.

abstract class Figure {
  double area();
}

class Tile implements Figure {
  const Tile(this.side);

  final double side;

  @override
  double area() {
    return side * side;
  }
}

class Wedge implements Figure {
  const Wedge(this.width, this.height);

  final double width;
  final double height;

  @override
  double area() {
    return width * height / 2.0;
  }
}

class Figures {
  const Figures();

  /// The plain test: one concrete class, through a trait object.
  static bool isTile(Figure s) {
    return s is Tile;
  }

  /// `is!`, which is not "not `is`" in the IR -- it carries a flag, and
  /// answering it as `is_none()` keeps one `downcast_ref` rather than two.
  static bool isNotTile(Figure s) {
    return s is! Tile;
  }

  /// Two tests in one body, so a wrong receiver in either shows.
  static double areaOfTiles(Figure a, Figure b) {
    double total = 0.0;
    if (a is Tile) {
      total = total + a.area();
    }
    if (b is Tile) {
      total = total + b.area();
    }
    return total;
  }
}

/// A **class name where a value goes**: Dart's `Type`.
///
/// Not the same question as `is`. `x is Foo` asks about a value; `Foo` on its
/// own *is* a value, of type `Type`, and upstream compares it, prints it and
/// uses it as a map key. The prelude has had `Type::of(name)` for that all
/// along and nothing produced one -- which refused `Theme.of`, and `Theme.of`
/// is called 268 times. Four `of` methods stood on this one construct and
/// accounted for 464 of the 670 "called something that was not translated".
class Marker {
  const Marker();
}

class Types {
  const Types();

  static Type markerType() {
    return Marker;
  }

  /// Two `Type`s compare by what they name.
  static bool isMarker(Type t) {
    return t == Marker;
  }

  static bool isTile(Type t) {
    return t == Tile;
  }
}
