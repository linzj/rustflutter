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
// `extends` rather than `implements`, and not for the reason it looks like:
// writing `implements Figure` here produced no `impl Figure for Tile` at all.
// The IR carries a superclass and a mixin list and no interface list, so a
// class that only *implements* an abstract one reaches none of its trait. A
// hole of its own, found by this fixture and not by this fixture's subject.

abstract class Figure {
  const Figure();

  double area();
}

class Tile extends Figure {
  const Tile(this.side);

  final double side;

  @override
  double area() {
    return side * side;
  }
}

class Wedge extends Figure {
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
