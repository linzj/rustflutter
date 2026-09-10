// The prelude's `Map` keeps a lazily built index over its entries and used
// to decide the index was still valid by comparing the entry *count* it was
// built at with the count now. A count comes back.
//
// Below eight entries the lookup is a scan and the index is not maintained
// at all, so: fill a map past that threshold (the index is built), empty it
// below it, fill it again to the same count -- and the index says "valid"
// while every position in it belongs to an entry that is no longer there.
//
// `InheritedElement._dependents` does exactly this every frame.
String use() {
  final Map<String, int> m = <String, int>{};
  for (var i = 0; i < 12; i++) {
    m['k$i'] = i;
  }
  // A lookup on a map of twelve is what builds the index.
  final int? built = m['k11'];
  for (var i = 0; i < 8; i++) {
    m.remove('k$i');
  }
  for (var i = 0; i < 8; i++) {
    m['n$i'] = 100 + i;
  }
  final found = <String>[];
  for (var i = 0; i < 8; i++) {
    if (m.containsKey('n$i')) {
      found.add('n$i');
    }
  }
  final int? gone = m.remove('n3');
  final int? kept = m['n7'];
  return '$built/${m.length}/${found.length}/$gone/$kept/${m['k11']}';
}
