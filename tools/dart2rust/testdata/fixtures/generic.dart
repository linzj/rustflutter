// Generics.
//
// Measured before it was built: of `package:flutter/`'s 2743 classes, 234 are
// generic -- 221 with one type parameter and 13 with two -- and 98 methods
// carry their own. So the feature is small, and Rust says all of it directly.
//
// Bounds are dropped. 72 of the 247 parameters have one, and a Dart bound
// names a *class* where Rust wants a trait; only an abstract class is a trait
// here, so most bounds have nothing to become. Dropping one is more permissive
// than Dart -- it loses a check and cannot turn correct code into an error.

class Pair<A, B> {
  const Pair(this.first, this.second);

  final A first;
  final B second;
}

/// A parameter no field mentions. Dart does not mind; Rust will not have an
/// unused parameter at all, so it gets a `PhantomData`.
class Tagged<T> {
  const Tagged(this.count);

  final int count;

  int doubled() {
    return count * 2;
  }
}

class Boxes {
  const Boxes();

  /// A method with its own parameter, which is separate from the class's.
  T firstOf<T>(T a, T b) {
    return a;
  }

  Pair<int, double> made() {
    return const Pair<int, double>(3, 4.5);
  }
}
