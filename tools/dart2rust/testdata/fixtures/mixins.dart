// A fixture for `with`.
//
// Kernel does not keep `class X extends A with B`. It writes
// `X extends _A&B extends A` and copies B's members into the synthetic
// `_A&B`, which is the CFE's own class and not something upstream wrote --
// so this compiler skips it. What it must not do is let a `super.foo()`
// inside X name that synthetic class: 180 members were refused with
// `super call into _MixinApplication12&RenderBox&..., which is not in this
// file`, and the class a reader would name is the mixin itself.
//
// The analyzer front end never sees the synthetic class at all, so this
// fixture is also where the two are held to the same answer: both read the
// class off the *resolved target*, not off the `extends` clause.
//
// The base is `Measured` and not `Sized` for a reason worth writing down: a
// Dart class named `Sized` collides with Rust's own marker trait, and the
// generated `<S: Sized + ?Sized>` stops meaning anything. Nothing guards
// against that yet.

abstract class Measured {
  const Measured();

  double base() {
    return 2.0;
  }
}

/// An `abstract mixin class` rather than a bare `mixin`: a `mixin`
/// declaration is not a class in the analyzer's AST and this compiler does not
/// lower one yet, so a fixture using it would be testing that hole instead of
/// this one. Abstract because that is what a mixin declaration is in Kernel,
/// and because only an abstract class emits the free functions a `super` call
/// names -- a concrete base has no trait for one to be generic over.
abstract mixin class Scaled {
  double scale() {
    return 3.0;
  }

  double base() {
    return 5.0;
  }
}

class Panel extends Measured with Scaled {
  const Panel();

  /// `base` is declared by the mixin, not by `Measured`. The `extends` clause
  /// says `Measured`, and answering from it would call the wrong body.
  double fromMixin() {
    return super.base();
  }

  /// Declared only by the mixin, so there is nothing else it could mean.
  double fromMixinOnly() {
    return super.scale();
  }
}
