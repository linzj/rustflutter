// A `try` stops a throw, not a call.
//
// Two contagion sets spread across `this`-calls: a method that calls one
// taking the handle needs the handle (`_computeHandles`), and a method that
// calls one writing a field needs `&mut self` (`_mutating`). Both read
// `selfCalls`, which was recorded only outside a `try` -- and that guard
// belongs to a third question, whether a *failure* escapes, which `catch`
// genuinely stops.
//
// So `EditableTextState._pasteTextWithReporting`, which is
// `try { await pasteText(cause); } catch ..`, never recorded the call: it
// kept `&self` while `pasteText` takes `&Rc<Self>`, and
// `self.paste_text(..)` named no method.
//
// Both sets are exercised here, because the guard suppressed both.

class Box {
  Box(this.tag);

  final String tag;

  int _count = 0;

  // Makes the class counted: a closure in its body calls one of its own
  // methods, so `this` lives behind a handle.
  void Function() get bump =>
      () => tick();

  void tick() {
    _count = _count + 1;
  }

  // Takes the handle, because it hands one out.
  void Function() spawn() => bump;

  // Calls both of the above from inside a `try`. Without the fix neither
  // the handle nor the `&mut self` reaches this method.
  String guarded() {
    try {
      spawn()();
      tick();
    } catch (_) {
      return 'threw';
    }
    return '$tag$_count';
  }
}

String use() {
  final Box b = Box('b');
  return b.guarded();
}
