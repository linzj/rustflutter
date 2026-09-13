/// A `const` constructor that redirects with shifted arguments.
///
/// `Color(int value) : this._fromARGBC(value >> 24, value >> 16, ..)`.
/// The redirect's arguments are put in for the target's parameters when
/// the constructor is written, and once they carried their type the shift
/// became the prelude's checked `dart_shr(..)?` -- right for a count that
/// may be negative, and impossible in a `const fn` ("destructor of
/// `ControlFlow<..>` cannot be evaluated at compile-time", E0493; the
/// gallery's `Color.new`, ws1117). A shift by a literal count under 64
/// cannot fail and is the operator in both languages.
library;

class Rgb {
  const Rgb(int value) : this._parts(value >> 16, value >> 8, value);

  const Rgb._parts(int r, int g, int b) : r = r & 255, g = g & 255, b = b & 255;

  final int r;
  final int g;
  final int b;

  @override
  String toString() => '$r,$g,$b';
}

const Rgb teal = Rgb(0x008080);

String use() {
  final Rgb runtime = Rgb(0x123456 + 1);
  return '$teal|$runtime|${Rgb(-1)}';
}
