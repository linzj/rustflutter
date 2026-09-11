/// A callback whose body only *throws*, put in a bare `Function` slot.
///
/// Its Dart return type is `Never`, which this compiler spells
/// `std::convert::Infallible` -- and an `Infallible` is not Rust's `!`, so
/// nothing coerces it. A bare `Function` slot is reached through the
/// dynamic adapter (`_dynamicFunction`), which calls the function and
/// boxes what comes back, and boxing one read:
///
///     expected `Null`, found `Infallible`
///
/// http's `IOClient.send` is the shape: `stream.handleError((error) {
/// throw ClientException(..); })`, and `Stream.handleError` declares a
/// bare `Function` (1 stub at ws1099). The prelude's `never` -- `match x
/// {}` -- is the one conversion an `Infallible` has.
library;

Never boom(Object e) => throw StateError('named:$e');

/// The bare `Function` slot: reaching it builds the adapter.
String held(Function f) => 'held:${f is Function}';

/// ..and the typed slot beside it, where the throw is actually caught.
String viaTyped(void Function(Object) f, Object arg) {
  try {
    f(arg);
    return 'returned';
  } on StateError catch (e) {
    return 'caught ${e.message}';
  }
}

String use() {
  final out = <String>[];
  out.add(held(boom));
  out.add(
    held((Object e) {
      throw StateError('closure:$e');
    }),
  );
  out.add(viaTyped(boom, 1));
  out.add(
    viaTyped((Object e) {
      throw StateError('closure:$e');
    }, 2),
  );
  // ..and one that returns, for the contrast.
  out.add(viaTyped((Object e) {}, 3));
  out.add('alive');
  return out.join('|');
}
