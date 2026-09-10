// Comparing two function values that take different numbers of arguments.
//
// No Dart function value has two arities, so two of them can never be the
// same object: `==` is false and `!=` is true, and there is nothing to
// compare. Rust's `dart_eq` takes `&Self`, and the two are different types
// (E0308, which stubbed the function).
//
// The gallery reaches this through upstream Flutter's own copy-paste:
// `_TimePickerModel.updateShouldNotifyDependent` compares
// `onHourMinuteModeChanged != oldWidget.onHourDoubleTapped` -- a
// `ValueChanged<_HourMinuteMode>` against a `VoidCallback` -- and the two
// lines below it read that same field on the left. It is written that way
// in `packages/flutter/lib/src/material/time_picker.dart` and carried that
// way in the kernel; answering it as Dart does is the faithful
// translation, so this fixture asks Dart what the answer is rather than
// assuming.

typedef Unary = void Function(int);
typedef Nullary = void Function();

class Model {
  Model(this.onValue, this.onTap, this.onOther);
  final Unary onValue;
  final Nullary onTap;
  final Nullary onOther;
}

String use() {
  void takesInt(int _) {}
  void takesNone() {}
  final Model a = Model(takesInt, takesNone, takesNone);
  final Model b = Model(takesInt, takesNone, () {});

  // Across arities: never the same object, whichever way round.
  final bool crossNe = a.onValue != b.onTap;
  final bool crossEq = a.onValue == b.onTap;
  final bool crossBack = b.onTap != a.onValue;

  // ..and the same arity still compares by identity, both ways.
  final bool sameTearOff = a.onTap == b.onTap;
  final bool differentClosures = a.onOther == b.onOther;
  final bool sameField = a.onValue == a.onValue;

  return '$crossNe/$crossEq/$crossBack/$sameTearOff/$differentClosures/$sameField';
}
