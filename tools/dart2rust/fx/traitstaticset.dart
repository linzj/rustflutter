// A write through a static that holds a *trait* handle is a setter call.
//
// **This fixture does not reproduce ws1026's failure and is kept as a
// regression net only.** It compiles with or without the rule: these statics
// do not lower through the same path the gallery's do. The rule's warrant is
// the chain's stub diff (`cupertino_context_menu.rs _update_tween_rects`,
// 61 -> 60, over three chains whose error moved forward each time).
//
// A `dyn Trait` has no fields, only the accessor pair it declares.
// `_ContextMenuRoute` keeps `_rectTweenReverse` (a `RectTween?`, mutable)
// and `_sheetScaleTween` (a `Tween<double>`, not), and `..end = x` on
// either came out as a field write: "method, not a field".
//
// Both kinds are here because they take different branches and only one has
// a cell to borrow through: a mutable static is behind a `RefCell`, an
// immutable one is the handle itself and the trait's setter does its own
// interior mutation.

abstract class Span {
  double get end;
  set end(double value);

  String show();
}

class Reach implements Span {
  Reach(this._end);

  double _end;

  @override
  double get end => _end;

  @override
  set end(double value) {
    _end = value;
  }

  @override
  String show() => '$_end';
}

class Keeper {
  // Immutable: the handle itself.
  static final Span fixed = Reach(1.0);

  // Mutable: reassigned below, so it lives behind a cell.
  static Span? swapped;

  static String run() {
    // Through the immutable static: no cell to borrow.
    fixed.end = 2.5;
    // ..and through the mutable one, which has one.
    swapped = Reach(9.0);
    swapped!.end = 3.5;
    return '${fixed.show()}/${swapped!.show()}';
  }
}

String use() => Keeper.run();
