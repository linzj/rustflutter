/// A method with its own type parameter, called on a value whose type is
/// the trait.
///
/// A generic trait method is `where Self: Sized` here, so it cannot be
/// called on a `dyn` at all; the object-safe `__erased` twin beside it is
/// what such a call goes through. The rule asked that of `this` only, and
/// a receiver that is an ordinary value of the trait's type fell through
/// to the generic one:
///
///     the size for values of type `dyn Element` cannot be known at
///     compilation time
///
/// `provider` reaches it inside a `visitAncestorElements` callback, on the
/// `parent` the callback is handed (1 stub at ws1106).
library;

/// A second declaration of the same generic member, so a call on a value
/// has to name which trait it goes through -- which is the qualified path
/// the gallery's call takes.
/// Declared twice on the way down -- so a call on a value of `Store` has
/// to name which trait it goes through -- and with no body on `Store`, so
/// there is no free super function to fall back on. That is the gallery's
/// shape: `BuildContext` declares
/// `getElementForInheritedWidgetOfExactType`, `Element` re-declares it,
/// and the call is `Element::..::<T>(&*parent, ..)`.
abstract class Reads {
  T? firstOfType<T>();
}

abstract class Store implements Reads {
  @override
  T? firstOfType<T>();

  List<Object> get items;
}

class Bag extends Store {
  Bag(this.items);

  @override
  final List<Object> items;

  @override
  T? firstOfType<T>() {
    for (final Object it in items) {
      if (it is T) {
        return it as T;
      }
    }
    return null;
  }
}

/// The caller that holds the store by its trait type, not by `this`.
String pick(Store s) => '${s.firstOfType<int>()}/${s.firstOfType<String>()}';

/// ..and through a callback, as `visitAncestorElements` does.
String pickVia(Store s, String Function(Store) f) => f(s);

/// ..and the same read through the *other* trait's type.
String pickAsReads(Reads r) => '${r.firstOfType<int>()}';

String use() {
  final Store s = Bag(<Object>['a', 7, 2.5]);
  final List<String> out = <String>[];
  out.add(pick(s));
  out.add(pickAsReads(s));
  out.add(pickVia(s, (Store inner) => '${inner.firstOfType<double>()}'));
  // ..and on `this`, which already worked, for the contrast.
  out.add('${s.firstOfType<bool>()}');
  return out.join('|');
}
