// `Object.hashAllUnordered(xs)`: a hash that does *not* depend on the order
// the elements come in. The prelude had `Object.hashAll` and not this one,
// so the call named a function nobody wrote ("cannot find function
// `object_hash_all_unordered` in this scope", E0425).
//
// `RenderObject.hashCode` is one.
String use() {
  final List<int> forward = <int>[1, 2, 3];
  final List<int> shuffled = <int>[3, 1, 2];
  final List<int> other = <int>[1, 2, 4];
  // Same elements in another order: the same hash. Different elements:
  // (all but certainly) a different one.
  final bool sameUnordered =
      Object.hashAllUnordered(forward) == Object.hashAllUnordered(shuffled);
  final bool differs =
      Object.hashAllUnordered(forward) != Object.hashAllUnordered(other);
  // ..and the *ordered* one, which was already here, does depend on order.
  final bool ordered = Object.hashAll(forward) != Object.hashAll(shuffled);
  return '$sameUnordered/$differs/$ordered';
}
