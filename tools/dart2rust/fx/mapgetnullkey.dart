// A map looked up by a *nullable* key, inside a loop.
//
// `m[k]` where `k` may be null lowers to a block that binds the map and
// then asks it only if the key is there:
//
//   { let __m = m; k.as_ref().and_then(|__k| __m.get(__k).cloned()) }
//
// The binding took the map *by value*. `get` needs only a reference and
// `cloned()` hands back an owned value, so the move bought nothing and
// cost the second turn of the loop: "use of moved value ... in previous
// iteration of loop" (E0382). `_SlottedRenderObjectElement._updateChildren`
// reads `oldKeyedElements` that way, and `Table.update` reads
// `oldKeyedRows`.
//
// The loop is the whole point: one lookup moves the map and compiles fine.

String use() {
  final Map<String, int> counts = <String, int>{'a': 1, 'b': 2, 'c': 3};
  // A nullable key, which is what sends the lookup down this path rather
  // than the plain `get`.
  final List<String?> wanted = <String?>['a', null, 'c', 'a', null];
  final List<String> seen = <String>[];
  for (final String? key in wanted) {
    final int? found = counts[key];
    seen.add(found == null ? '-' : '$found');
  }
  // Read again after the loop, so nothing may have consumed it.
  return '${seen.join(",")}/${counts.length}/${counts['b']}';
}
