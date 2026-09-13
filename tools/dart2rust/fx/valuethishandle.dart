/// `this` of a *value* class, handed from a mixin's body to a slot typed
/// by a trait it implements.
///
/// A class this compiler keeps by value -- fields, but no closure that
/// calls a method on it, so no `DartSelf` handle -- has nothing to hand
/// out as itself. Into a trait handle the shortcut for `this` gave the
/// bare struct:
///
///     expected `Rc<dyn DiagnosticableTree>`,
///     found `_LargeTitleNavigationBarSliverDelegate`
///
/// `_LargeTitleNavigationBarSliverDelegate.toDiagnosticsNode` builds a
/// `DiagnosticableTreeNode(value: this)` (1 stub at ws1115). A value class
/// copied behind a fresh handle is what every other value goes into a
/// trait slot as; what the holder reads back is this object's own fields.
library;

abstract class Shape {
  String get name;

  double get area;
}

class Holder {
  Holder(this.shape, this.label);

  final Shape shape;

  final String label;

  String show() => '$label:${shape.name}/${shape.area}';
}

/// The site under test lives in a *mixin*, as `toDiagnosticsNode` does in
/// `DiagnosticableTreeMixin`: its body is copied into the class that mixes
/// it in, and `this` there is the value class.
mixin Wraps on Shape {
  Holder wrap(String label) => Holder(this, label);
}

class Square extends Shape with Wraps {
  Square(this.side);

  final double side;

  @override
  String get name => 'square';

  @override
  double get area => side * side;
}

String use() {
  final Square s = Square(3);
  final List<String> out = <String>[];
  out.add(s.wrap('a').show());
  out.add(Square(1.5).wrap('b').show());
  // ..and through a typed local, which already worked, for the contrast.
  final Shape asShape = s;
  out.add(Holder(asShape, 'c').show());
  return out.join('|');
}
