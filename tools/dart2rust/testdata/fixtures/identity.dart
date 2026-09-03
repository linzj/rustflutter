// A fixture for `identical`.
//
// Dart's `identical(a, b)` asks whether two names reach one object. Rust asks
// it of an address -- but only where there *is* an object with an address, and
// this compiler does not give every class one: a translated value class is a
// `Copy` struct, and the address of a copy answers nothing.
//
// So the question the backend really asks is "does the emitted Rust for this
// expression have an identity to take", and the answer has three shapes:
//
//   * `&dyn Node` -- already a reference, and its address is the object's.
//   * `Rc<Foo>` -- a *handle*, and two handles to one object sit at two
//     different addresses. The handle has to be dereferenced first, or the
//     test compiles and answers the opposite of the question.
//   * a plain struct passed by value -- no identity at all, and refused.
//
// The middle one is the case that is easy to get wrong, because it compiles
// either way. `watching` below is written so that it does not.

abstract class Node {
  const Node();

  int id();
}

class Watcher extends Node {
  Watcher(this.tag);

  final int tag;

  @override
  int id() {
    return tag;
  }

  void bump() {}

  /// Hands out a closure that calls a method on `this`, so this method's
  /// receiver is `&Rc<Self>` rather than `&Self` -- and `identical(this, ..)`
  /// in the same body has to look through the handle to the object.
  bool watching(Node other) {
    final void Function() f = () {
      bump();
    };
    f();
    return identical(this, other);
  }
}

class Nodes {
  const Nodes();

  /// Two trait objects: both sides are already references.
  static bool same(Node a, Node b) {
    return identical(a, b);
  }

  static bool different(Node a, Node b) {
    return !identical(a, b);
  }
}
