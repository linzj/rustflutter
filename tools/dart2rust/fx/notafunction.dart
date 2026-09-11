/// A `dynamic` in a function slot that is not a function of that shape --
/// or not a function at all.
///
/// `_typedFunction` (`coerce.dart`) asks `dart_function_same` for the very
/// handle the value was made from and, when it is a function of *another*
/// shape, builds an adapter that calls it dynamically. Handed something
/// that is no function at all it panicked:
///
///     panic!("dart2rust: a {} called as a function", ..)
///
/// Dart fails the coercion with a `TypeError` and a dynamic call on a
/// non-function with `NoSuchMethodError`. Both are values, so the answer
/// is split by who asked: `dart_function_same` says the cast failed,
/// `dart_call_function` says there is no `call`.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

String apply(int Function(int) f, int x) => 'r:${f(x)}';

String viaSlot(dynamic f, int x) {
  try {
    return apply(f, x);
  } on Error catch (_) {
    return 'not-a-function';
  }
}

String use() {
  final out = <String>[];
  out.add(viaSlot((int x) => x * 2, 3));
  out.add(viaSlot(7, 3));
  out.add(viaSlot('text', 3));
  out.add(viaSlot(<int>[1], 3));
  // A function, but of the wrong arity: Dart rejects the coercion, this
  // side rejects the call. Both are caught, and both say so here.
  out.add(viaSlot((int a, int b) => a + b, 3));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
