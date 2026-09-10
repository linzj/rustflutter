// `xs.fold<R>(initial, combine)` with the type argument written out and an
// initial value of a *narrower* type. Dart widens the initial value into
// `R`; here the prelude's `fold_dart<R>` infers `R` from the value it is
// handed, so it inferred the concrete class while the closure was written
// at the trait -- "expected closure signature `fn(Rc<Leaf>, ..)`, found
// `fn(Rc<dyn Shape>, ..)`" (E0631).
//
// `_CompoundBorder.dimensions` is exactly this: `borders.fold<
// EdgeInsetsGeometry>(EdgeInsets.zero, (previousValue, border) =>
// previousValue.add(border.dimensions))`, whose `EdgeInsets.zero` is an
// `EdgeInsets`.
abstract class Shape {
  int get size;
  Shape plus(Shape other);
}

class Leaf implements Shape {
  Leaf(this.size);

  @override
  final int size;

  @override
  Shape plus(Shape other) => Leaf(size + other.size);
}

String use() {
  final List<Shape> shapes = <Shape>[Leaf(1), Leaf(2), Leaf(4)];
  // The initial value is a `Leaf`, the type argument is `Shape`.
  final Shape total = shapes.fold<Shape>(
    Leaf(0),
    (Shape a, Shape b) => a.plus(b),
  );
  return '${total.size}';
}
