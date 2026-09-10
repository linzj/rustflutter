// A type parameter bounded by `Iterable<E>` is spelled at its bound
// (`_type`), and the bound is `Rc<dyn DartIterable<E>>` since ws908 -- so a
// `Set` handed to such a slot has to become the handle, the way it does at
// a slot Dart spells `Iterable` outright.
//
// This is the shape `collection`'s `_UnorderedEquality<E, T extends
// Iterable<E>>` has: `DeepCollectionEquality.equals` casts its two objects
// to `Set` and hands them to `SetEquality`'s `equals(T? a, T? b)`.
abstract class Unordered<E, T extends Iterable<E>> {
  String same(T? a, T? b) {
    if (a == null || b == null) return 'null';
    return '${a.length}-${b.length}-${a.first == b.first}';
  }

  String only(T items) => items.map((e) => '$e').join('+');
}

class SetUnordered<E> extends Unordered<E, Set<E>> {}

class ListUnordered<E> extends Unordered<E, List<E>> {}

String use() {
  final Object a = <String>{'x', 'y'};
  final Object b = <String>{'x', 'z'};
  final set = SetUnordered<String>();
  final list = ListUnordered<String>();
  return '${set.same(a as Set<String>, b as Set<String>)}'
      '/${set.same(null, null)}'
      '/${set.only(a as Set<String>)}'
      '/${list.same(<String>['p'], <String>['p', 'q'])}'
      '/${list.only(<String>['m', 'n'])}';
}
