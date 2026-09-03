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

/// A **generic method on an abstract class**, which is a trait here.
///
/// A trait with a generic method is not dyn-compatible, and every abstract
/// class in this compiler's output is reached through `dyn` -- so these were
/// refused outright, 302 of them. Rust leaves a `where Self: Sized` method out
/// of the vtable, which keeps the trait usable as `dyn` *and* keeps the method
/// on every concrete implementor. What is given up is calling it through a
/// trait object, which is a refusal where the call is rather than a member
/// deleted where it is declared.
abstract class Store {
  const Store();

  /// Generic, so it carries the bound.
  ///
  /// It counts rather than returning an element, and not for tidiness:
  /// `return items[0];` does not compile, because indexing a `Vec<T>` moves
  /// out of it and `T` is not `Copy`. A hole of its own -- a Dart list read is
  /// a copy of a reference and a Rust one is a move -- and not this fixture's
  /// subject.
  int countOf<T>(List<T> items, T ignored);

  /// Not generic, so it stays in the vtable and `dyn Store` can call it.
  int size();
}

class Shelf extends Store {
  const Shelf(this.width);

  final int width;

  @override
  int countOf<T>(List<T> items, T ignored) {
    return items.length + width;
  }

  @override
  int size() {
    return width;
  }
}

class Shelves {
  const Shelves();

  /// Through the trait object: only the non-generic half is reachable.
  static int sizeOf(Store s) {
    return s.size();
  }
}
