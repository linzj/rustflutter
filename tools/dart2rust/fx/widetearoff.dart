// A tear-off whose parameter is *wider* than the slot's. Dart's
// `Set.contains(Object? element)` takes anything; `firstWhere(test)` wants a
// `bool Function(E)`. Dart is happy -- a function taking `Object?` is a
// function taking `E` -- and Rust is not: the closure the tear-off becomes
// declares the method's own parameter, and the prelude's `first_where` wants
// the element's ("type mismatch in closure arguments").
//
// `_ReadingOrderSortData.commonDirectionalityOf` ends in exactly this:
// `list.first.directionalAncestors.firstWhere(common.contains)`.
String use() {
  final Set<String> common = <String>{'b', 'c'};
  final List<String> xs = <String>['a', 'b', 'c'];
  final String found = xs.firstWhere(common.contains);
  // ..and where nothing matches, the `orElse` arm of the same shape.
  final String none = xs.firstWhere(
    <String>{'z'}.contains,
    orElse: () => 'none',
  );
  // The same tear-off through a plain `where`, so the shape is covered
  // twice: `any`/`where` take the same slot.
  final String kept = xs.where(common.contains).join('+');
  return '$found/$none/$kept';
}
