// A line the AOT compiler proved dead still has a *type* where it stands.
//
// TFA plants `throw "Attempt to execute code removed by Dart AOT compiler
// (TFA)"` where type flow proved nothing arrives, and the front end lowers
// that to `unreachable!(..)` -- a claim, not an exception. In statement
// position that is right. In *expression* position Rust's never-type
// fallback fills the hole with `()`, and then the operator around it is
// asked for something no one implements:
//
//   `{ unreachable!(..) } == __me.selected_month.get().unwrap()`
//   error[E0277]: can't compare `()` with `i64`
//
// Three stubs in the gallery come from exactly this, in two shapes: a
// comparison (`_buildDayPicker`, `_adjustSelectionIndexBasedOnSelection
// Geometry`) and a `?` whose two sides then disagree (`_handleEntryMode
// Toggle`). Both are here.
//
// The guard in front of each one is false, so nothing here actually runs the
// dead line -- which is the point: it has to *compile*, and Dart and this
// have to print the same thing.

int _size = 3;

String use() {
  final List<String> out = <String>[];

  // A comparison whose left side is the dead line.
  final bool compared =
      _size < 0 &&
      (throw 'Attempt to execute code removed by Dart AOT compiler (TFA)') ==
          _size;
  out.add('$compared');

  // The same against a non-scalar, which fails differently: the trait bound
  // is on a translated type rather than on `i64`.
  final bool named =
      _size < 0 &&
      (throw 'Attempt to execute code removed by Dart AOT compiler (TFA)') ==
          Tag.b;
  out.add('$named');

  // In a value position with a declared type: what `?` does to the two
  // sides of a failing call.
  final int taken = _size < 0
      ? (throw 'Attempt to execute code removed by Dart AOT compiler (TFA)')
      : _size * 2;
  out.add('$taken');

  // ..and the shape the gallery actually has, which the three above do not
  // reach: the dead line is not written, it is what TFA leaves behind a
  // null-assert it proved can never run. `widget.minimumDate!.month ==
  // selectedMonth` in `_buildDayPicker`, where nothing in this program ever
  // gives `minimumDate` a value -- so `!` is dead, and the `.month` read on
  // top of it is swallowed into the block the placeholder becomes.
  final Bound bound = Bound();
  final bool bounded = bound.minimum?.size == 1 && bound.minimum!.size > _size;
  out.add('$bounded');

  return out.join('/');
}

class Held {
  Held(this.size);

  final int size;
}

class Bound {
  Bound({this.minimum});

  final Held? minimum;
}

enum Tag { a, b }
