// A fixture for `a?.b`.
//
// Rust says this with `a.map(|it| ...)`, and the risk is the same family as
// `??`: the body must not run when the receiver is null. `boom()` makes that
// observable by asserting -- a fixture whose body was an ordinary read could
// not tell "skipped" from "ran and the answer happened to match".

class Leaf {
  const Leaf(this.size);

  final double size;

  double doubled() {
    return size * 2.0;
  }

  double boom() {
    assert(false, 'the body of ?. was evaluated');
    return 0.0;
  }
}

class Branch {
  const Branch(this.leaf);

  final Leaf? leaf;

  /// A field read through `?.`.
  double? leafSize() {
    return leaf?.size;
  }

  /// A method call through `?.`.
  double? leafDoubled() {
    return leaf?.doubled();
  }

  /// The body must not run when the receiver is null.
  double? leafBoom() {
    return leaf?.boom();
  }

  /// `?.` beside `??`, so the two lowerings are not confused with each other:
  /// one has null in the then, the other has the temporary in the else.
  double sizeOr(double fallback) {
    return leaf?.size ?? fallback;
  }
}
