// A class that merely *holds* a generic must ask for the bound that generic
// needs.
//
// The generated `PartialEq` gets its `where` clause from *this* class's own
// projected fields. A class with none of its own, holding a generic whose
// own `PartialEq` requires `<T as DartNullable>::Or: PartialEq`, never asked
// for it -- "binary operation `==` cannot be applied" (E0369).
//
// And the clause has to keep the bound for each projected field's *base*,
// which is not always a type parameter: `IterableProperty<T>._value` is
// `<Rc<dyn DartIterable<T>> as DartNullable>::Or`, and it is that whole
// handle whose `Or` must be comparable. Asking only per type parameter
// cleared one and broke the other; both sets are needed.
//
// `_MenuItem<T>` holds an `Option<DropdownMenuItem<T>>` and is the gallery's.
//
// What is compared here is deliberately never two *distinct but equal*
// values. A class with no `operator ==` compares by identity in Dart and by
// fields in this compiler, so `Outer(a) == Outer(a)` is false there and true
// here -- the value-class identity gap under 已知欠账, which is a different
// thing from the bound this fixture is about. Comparing a value with itself
// and with a plainly different one answers the same either way.

class Inner<T> {
  Inner(this.value, this.label);

  // A projected field: `T?` spelled with a type parameter.
  final T? value;
  final String label;
}

class Outer<T> {
  Outer(this.inner, this.onTap);

  // Holds a generic whose `PartialEq` needs the projected bound, while none
  // of `Outer`'s own fields is projected.
  final Inner<T>? inner;

  // A function field, so the generated `==` is written out rather than
  // derived -- `Rc<dyn Fn>` has no `PartialEq` and is compared by identity.
  final void Function()? onTap;
}

// ..and a field projected over something that is *not* a type parameter:
// `Iterable<T>?` is a handle, and its `Or` is what must be comparable.
class Listed<T> {
  Listed(this.items, this.onTap);

  final Iterable<T>? items;
  final void Function()? onTap;
}

String use() {
  final Outer<String> one = Outer<String>(Inner<String>('a', 'x'), null);
  final Outer<String> other = Outer<String>(Inner<String>('b', 'y'), null);
  final Listed<String> listed = Listed<String>(<String>['a'], null);
  // The fields are read, so TFA keeps them. Left unread, `Inner<T>` comes
  // out holding nothing but a `PhantomData`, every `Inner` compares equal,
  // and the bound this fixture is about is never needed at all.
  final String seen =
      '${one.inner?.value}${one.inner?.label}${listed.items?.length}';
  return '$seen/${one == one}/${one == other}/${listed == listed}';
}
