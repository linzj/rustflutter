// A fixture for Dart enums.
//
// A plain enum is a Rust enum and nothing more -- one of the few places the two
// languages need nothing said. The variants are renamed (`Axis.vertical` is
// `Axis::Vertical`) and nothing else is.
//
// The enhanced enum below is here to be *refused*: it carries a method, so it
// is a Rust enum plus an impl, which is a different job. Emitting it as a plain
// one would drop the method silently, and the test asserts the refusal rather
// than trusting it.

enum Axis { horizontal, vertical }

enum MainAxisAlignment { start, end, center, spaceBetween, spaceAround }

/// Enhanced: it has a method, so translating it as a plain enum would lose one.
enum Season {
  spring,
  summer;

  bool get isWarm => this == Season.summer;
}

class Layout {
  const Layout(this.axis, this.alignment);

  final Axis axis;
  final MainAxisAlignment alignment;

  bool get isHorizontal {
    return axis == Axis.horizontal;
  }

  /// Reads a value of another enum, so the two do not get confused.
  bool get isCentred {
    return alignment == MainAxisAlignment.center;
  }

  /// A multi-word value, where the renaming actually does something.
  bool get isSpaced {
    return alignment == MainAxisAlignment.spaceBetween;
  }
}
