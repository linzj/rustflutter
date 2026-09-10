// A value read off a receiver whose class parameter the compiler *erased*
// comes back spelled at the bound -- `Box<Object>`, not `Box<T>` -- and the
// code that kept its own `T` wants a `Box<T>`. Rust's trait objects are not
// covariant, so the two are different types and nothing unsizes one into the
// other: the object answers for its own instantiation by id, which is what a
// cast asks it. `_InheritedProviderScopeElement` reading `widget.owner.
// _delegate` off the erased `InheritedProvider` is the shape (7 stubs).
abstract class Box<T> {
  T get value;
}

class Full<T> implements Box<T> {
  Full(this.value);

  @override
  final T value;
}

class Fallback<T> implements Box<T> {
  Fallback(this.spare);

  final T spare;

  @override
  T get value => spare;
}

/// `T` here is used covariantly below (an `Owner<int>` reaching an
/// `Owner<Object>` slot), so it is erased: the struct has no parameter and
/// `box` reads back as a `Box<Object>`.
class Owner<T> {
  Owner(this.box);

  final Box<T> box;
}

/// `T` here is kept: nothing ever puts a `Reader<A>` where a `Reader<B>` is
/// wanted. So `owner.box` -- a `Box<Object>` -- lands in a `Box<T>` slot.
class Reader<T> {
  Reader(this.owner);

  final Owner<T> owner;

  Box<T> get box => owner.box;

  T read() => box.value;
}

String use() {
  final wide = <Owner<Object>>[Owner<Object>(Full<Object>('wide'))];
  final narrow = Owner<int>(Full<int>(7));
  wide.add(narrow);
  final reader = Reader<int>(narrow);
  final spare = Reader<int>(Owner<int>(Fallback<int>(3)));
  return '${reader.read()}/${spare.read()}/${wide.length}/${reader.box.value}';
}
