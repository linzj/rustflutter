/// A function value through a `dynamic` slot and back.
///
/// The erased half of a generic local function stands on this: `T?` at the
/// bound `Object?` holds whatever the call site instantiated `T` with, and
/// one of those is a function type (`ButtonStyle.foregroundBuilder`).
String use() {
  final String Function(String) f = (String s) => '[$s]';
  final Object? boxed = f;
  final String Function(String) back = boxed as String Function(String);
  final List<Object?> slot = <Object?>[f];
  final String Function(String) fromList = slot[0] as String Function(String);
  return '${back('a')}/${fromList('b')}/${boxed == f}';
}
