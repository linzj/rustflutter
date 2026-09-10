// Promoting a local whose declared type is a *type parameter*.
//
// `if (v is double)` on a `T v` does not promote `v` to `double` in the
// kernel: it promotes it to the *intersection* `T & double`. The reads
// here are written for an `InterfaceType`, a `TypeParameterType` and a
// `FunctionType`, and an intersection is none of those -- so the read went
// in as a bare `T` and `debugFormatDouble(v)` was handed a type parameter
// where an `f64` was declared (`IterableProperty.valueToString`, and
// `DiagnosticsProperty.valueToString` beside it, whose `v is
// DiagnosticableTree` is the same shape at a trait).
//
// What the promotion says the value *now is* is the right-hand side of the
// intersection, and everything below already knows what to do with that.
//
// The answers are what this pins: each branch reads the value at a
// different type, so a read left at `T` -- or narrowed to the wrong side --
// shows up in the string.

abstract class Shape {
  String get tag;
}

class Round extends Shape {
  @override
  String get tag => 'round';
  double radius = 1.5;
}

class Boxy extends Shape {
  @override
  String get tag => 'boxy';
  int side = 2;
}

String describe<T>(T v) {
  // Promoted to a scalar: `T & double`.
  if (v is double) {
    return 'double:${v.toStringAsFixed(2)}';
  }
  // ..to another scalar, to show the branch is chosen by the test.
  if (v is int) {
    return 'int:${v + 1}';
  }
  // ..and to a translated class, whose member the bare `T` does not have.
  if (v is Round) {
    return 'round:${v.radius}';
  }
  if (v is Shape) {
    return 'shape:${v.tag}';
  }
  return 'other:$v';
}

String use() {
  return <String>[
    describe<double>(1.25),
    describe<int>(41),
    describe<Object>(2.5),
    describe<Object>(Round()),
    describe<Object>(Boxy()),
    describe<Object>('s'),
  ].join('|');
}
