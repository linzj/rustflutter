// DIFFERS: the analyzer sees the constructor call in the source; Kernel sees
// the evaluated instance and writes it out field by field.
//
// A fixture for `const` instances whose constructor cannot rebuild them.
//
// The Kernel front end used to rebuild `const Alignment(-1, -1)` as
// `Alignment::new(-1.0, -1.0)` by matching the unnamed constructor's parameter
// names against the instance's field names. That reads well and it is still
// what happens when it works -- but it works for only 1021 of
// `package:flutter`'s 5602 const instances, and each of the three classes
// below is one of the ways it does not.
//
// The deeper reason is that a `const` instance never calls its constructor, so
// nothing keeps the constructor alive: `_Linear` in curves.dart has none left
// in the dill at all. A constructor is therefore an optimisation, and the
// field values -- which an InstanceConstant always carries, already computed --
// are the answer.
//
// The values are all different from each other on purpose. Two fields holding
// 4.0 would let a wrong pairing pass, which is how five earlier fixtures failed
// to test what they were for.

/// Only a *named* constructor, so there is no unnamed one to match against.
/// `EdgeInsets` (5 named, no unnamed) is upstream's version of this, 224 times.
class Spacing {
  const Spacing._(this.amount);

  final double amount;

  static const Spacing tight = Spacing._(3.0);
  static const Spacing wide = Spacing._(17.0);

  double twice() => amount * 2.0;
}

/// The base holds the fields under different names, so the subclass's
/// parameters name nothing. `Offset(dx, dy)` storing `_dx`/`_dy` on `OffsetBase`
/// is upstream's version, 272 times; `Size` another 164.
class InsetBase {
  const InsetBase(this._h, this._v);

  final double _h;
  final double _v;
}

class Inset extends InsetBase {
  const Inset(double h, double v) : super(h, v);

  static const Inset small = Inset(5.0, 7.0);
  static const Inset large = Inset(23.0, 29.0);

  // The fields, not inherited getters: methods on a *concrete* base are not
  // flattened into the subclass yet, which is a standing gap and not what this
  // fixture is for.
  double span() => _h + _v;
}

/// A parameter that is not a field: `length` never becomes one, and `end` is
/// worked out in the initialiser list. `TextStyle` and `IconThemeData` are
/// upstream's version.
class Span {
  const Span(this.start, int length) : end = start + length;

  final int start;
  final int end;

  static const Span first = Span(2, 11);
  static const Span second = Span(40, 60);

  int width() => end - start;
}
