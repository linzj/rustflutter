// `Iterable<E>.generate(count, [generator])` without a generator: the
// elements are the indices, and `E` is spelled only on the constructor.
//
// Rust has nothing to infer the prelude's `T` from -- the `None` generator
// says nothing and the count is an `i64` whatever the element is -- so a
// call whose result is consumed by an iterator chain rather than stored in
// a typed local left the element open ("type annotations needed for `&_`",
// the starter study's `home.dart`). The type argument is spelled instead.
String use() {
  // Consumed by a chain, with no annotated local in between: nothing
  // downstream tells Rust the element type either.
  final String mapped = Iterable<int>.generate(4)
      .toList()
      .map((int i) => '${i * i}')
      .join(',');
  // With a generator, which the element type *is* inferable from: unchanged.
  final String made = Iterable<String>.generate(3, (int i) => 'x$i').join('|');
  // Stored first, so the local's type is what Rust reads.
  final List<int> stored = Iterable<int>.generate(3).toList();
  return '$mapped/$made/${stored.length}${stored.last}';
}
