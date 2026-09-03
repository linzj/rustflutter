// A fixture for the object that a closure has to hold.
//
// Round 101 gave a closure the *fields* it touches, in cells both it and the
// object see. That answers a closure that reads and writes; it does not answer
// one that **calls a method** on `this` -- a closure cannot capture a method,
// only an object. 414 closures across the package do that, in 197 classes.
//
// The answer is that such a class is reference counted: its type is
// `Rc<Ticker>` wherever it appears, its constructor hands one out, and a
// method that gives away a closure takes `self: &Rc<Self>` so the closure can
// keep a handle.
//
// Round 102 priced the alternative -- 1150 places where one of these classes
// is constructed, held, passed or returned -- and this is why the type itself
// carries it: every one of those places then follows from `type()`, rather
// than being 1150 edits.

class Ticker {
  Ticker(this.step);

  final int step;

  int fired = 0;

  void fire() {
    fired = fired + step;
  }

  /// Hands out a closure that **calls a method** on `this`. The closure
  /// outlives this call, so it cannot borrow; it keeps a counted handle.
  void Function() trigger() {
    return () {
      fire();
    };
  }

  int seen() {
    return fired;
  }
}
