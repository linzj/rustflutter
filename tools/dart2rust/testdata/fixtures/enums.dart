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

/// An **enhanced** enum whose values carry state of their own.
///
/// `none(0)` gives each variant a `value`, and that used to be refused: a Rust
/// enum would need a payload per variant to say the same thing. It would not.
/// The value is a constant *of* the variant, so the Rust is a `match` in a
/// getter -- which is also what makes it free to read.
enum Tristate {
  none(0),
  isTrue(1),
  isFalse(2);

  const Tristate(this.value);

  final int value;

  bool get isSet {
    return value > 0;
  }
}

class UsesTristate {
  const UsesTristate();

  int weigh(Tristate state) {
    return state.value * 10;
  }

  /// Names all three values. The Kernel front end recovers an enum's variants
  /// from the *constants* that name them -- the dill keeps no fields for them
  /// -- so a fixture that declares an enum and never uses it tests nothing on
  /// that side, and the two front ends came out 18 lines apart until this
  /// method existed.
  Tristate pick(int n) {
    if (n > 0) {
      return Tristate.isTrue;
    }
    if (n < 0) {
      return Tristate.isFalse;
    }
    return Tristate.none;
  }
}
