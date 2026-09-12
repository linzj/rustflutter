/// An erased type parameter whose *bound* names another parameter of the
/// same declaration.
///
/// An erased parameter is spelled as its bound. When that bound mentions a
/// parameter the declaration keeps, the kept one has to be put in as well,
/// or its name leaks into an `impl` that does not bind it.
///
/// `provider`'s `_DelegateState<T, D extends _Delegate<T>>` is the shape:
/// `D` is erased, so `willUpdateDelegate(D)` came out taking a
/// `_Delegate<T>` inside `impl _InheritedProviderScopeElement`, which has
/// no `T` at all -- "cannot find type `T` in this scope" (1 stub at
/// ws1103).
library;

class Carrier<T> {
  Carrier(this.v);

  final T v;

  String show() => 'c$v';
}

class NumCarrier extends Carrier<int> {
  NumCarrier(super.v);

  @override
  String show() => 'n$v';
}

/// `D`'s bound names `T`, which this declaration keeps.
abstract class Holder<T, D extends Carrier<T>> {
  D get carrier;

  String describe(D other) => '${carrier.show()}+${other.show()}';
}

class IntHolder extends Holder<int, NumCarrier> {
  IntHolder(this.carrier);

  @override
  final NumCarrier carrier;
}

/// The caller, whose *own* parameter is erased too -- as
/// `_InheritedProviderScopeElement<T>` is. Its `impl` binds no `S`, so a
/// slot spelled `Carrier<S>` has no `S` to name.
class Scope<S> {
  Scope(this.held, this.tag);

  final Holder<S, Carrier<S>> held;

  final S tag;

  String go(Carrier<S> other) => held.describe(other);
}

/// The flow site that erases `Scope.S`.
final List<Scope<dynamic>> seen = <Scope<dynamic>>[];

String use() {
  final IntHolder h = IntHolder(NumCarrier(1));
  // The flow site that erases `D`: a `Holder<int, NumCarrier>` into a
  // `Holder<int, Carrier<int>>` slot.
  final Scope<int> s = Scope<int>(h, 0);
  // ..and the one that erases `Scope.S`, so its `impl` binds nothing.
  seen.add(s);
  // Only values of the holder's own `D`: Dart checks the covariant
  // parameter at run time, and a plain `Carrier<int>` through the erased
  // spelling is a `TypeError` there -- the erasure's known price, not this
  // rule's subject.
  return '${s.go(NumCarrier(2))}|${h.describe(NumCarrier(4))}|${seen.length}';
}
