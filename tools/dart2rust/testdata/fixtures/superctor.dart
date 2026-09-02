// A fixture for `: super(...)`.
//
// Rust has no constructor inheritance, so the base's fields go into the
// subclass's struct and the base's constructor is inlined -- its parameters
// replaced by whatever the super call passed. Across package:flutter 80% of the
// 1888 super calls with arguments have an **abstract** base, whose fields a
// trait cannot hold at all, so flattening is not a style choice.
//
// The values are chosen so that a wrong substitution shows: `Middle` passes a
// *computed* argument up, so pairing the base's parameters with the wrong
// expressions changes the number rather than shuffling equal ones.

abstract class Shape {
  const Shape(this.width, this.height);

  final double width;
  final double height;

  double area() {
    return width * height;
  }
}

class Rectangle extends Shape {
  const Rectangle(double w, double h) : super(w, h);
}

/// Passes a computed argument up, so the substitution has to carry an
/// expression rather than just a name.
class Square extends Shape {
  const Square(this.side) : super(side, side);

  final double side;
}

/// Two levels: `Padded` -> `Square` -> `Shape`. Chains go six deep upstream.
class Padded extends Square {
  const Padded(double side, this.padding) : super(side);

  final double padding;

  double paddedArea() {
    return (width + padding) * (height + padding);
  }
}
