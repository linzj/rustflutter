// An expression built on an operand that never returns. Dart evaluates the
// operand first, so the expression is not reached either -- and in Rust a
// method call or an `.await` on a value of type `!` has nothing to resolve
// against ("type annotations needed ... cannot infer type", E0282, and
// "`()` is not a future", E0277).
//
// Dart's AOT compiler plants such an operand wherever it proves a value
// cannot exist: `throw 'Attempt to execute code removed by Dart AOT
// compiler (TFA)'`, which the front end reads as "this line is dead" and
// lowers to `unreachable!()`. That marker is written out here, because a
// fixture-sized program is not big enough for TFA to plant one -- it folds
// the whole construct away instead.
//
// `DropdownMenuThemeData.inputDecorationTheme` reads as `{ .. unreachable!
// (..) }.data(..)`, `DatePickerThemeData`'s beside it, and
// `_ContrastEvaluation._evaluate` as `{ .. unreachable!(..) }.await`.
const String _tfa =
    'Attempt to execute code removed by Dart AOT compiler (TFA)';

class Wrapped {
  Wrapped(this.text);
  final String text;
}

/// A *method call* on a dead operand.
String deadReceiver(bool reached) {
  if (reached) {
    return ((throw _tfa) as Wrapped).text;
  }
  return 'alive';
}

/// ..and an `await` on one.
Future<String> deadAwait(bool reached) async {
  if (reached) {
    return await ((throw _tfa) as Future<String>);
  }
  return 'awaited';
}

String use() {
  // `reached` is false, but it comes from a list built at run time so
  // nothing folds the branch away.
  final List<bool> flags = <bool>[];
  final bool reached = flags.isNotEmpty;
  final String head = deadReceiver(reached);
  return head;
}
