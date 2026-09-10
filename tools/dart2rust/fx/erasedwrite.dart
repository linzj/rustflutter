// A field *write*'s receiver is a member access's receiver: an erased read
// is narrowed to the type Dart gives it.
//
// `InheritedNotifier<T extends Listenable>` declares `notifier` as `T`, and
// `_InheritedResetNotifier extends InheritedNotifier<_ResetNotifier>` writes
// `inheritedNotifier.notifier!._wasCalled = false`. The read one line above
// it was already narrowed and compiled; the write saw the declaration's
// bound, `Rc<dyn Listenable>`, which has no such field.
//
// Both statements are here, adjacent, because that pair is the whole point:
// the same receiver, one compiling and one not.

abstract class Signal {
  String get label;
}

class Reset implements Signal {
  Reset(this.label);

  @override
  final String label;

  // Declared on the concrete class only -- not on `Signal` -- so a receiver
  // erased to the bound cannot reach it.
  bool fired = false;

  int _count = 0;

  // ..and a real setter, which takes a different path in the lowering than
  // a field write does: `IrSetter` rather than `IrAssignField`. ws1027
  // narrowed the field write and ws1029 the setter call, and only doing
  // both makes this fixture green.
  int get count => _count;

  set count(int value) {
    _count = value * 2;
  }
}

class Holder<T extends Signal> {
  Holder(this.signal);

  final T? signal;
}

class ResetHolder extends Holder<Reset> {
  ResetHolder(Reset super.signal);

  String take() {
    // The read: already narrowed before this round.
    final bool was = signal!.fired;
    // The write: the statement that did not compile.
    signal!.fired = false;
    // ..and a setter *call* through the same erased receiver.
    signal!.count = 21;
    return '$was${signal!.fired}${signal!.label}${signal!.count}';
  }
}

String use() {
  final Reset r = Reset('r');
  r.fired = true;
  final ResetHolder h = ResetHolder(r);
  return '${h.take()}/${r.fired}';
}
