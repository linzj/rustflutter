// The members a body asks of a value whose static type is `Iterable<T>`.
//
// Measured across the gallery: 25 names over 501 sites, `toList` 148 of
// them. With `Iterable` a trait object the read materialises first
// (`_listReceiver`), and this is what says the materialising is right.
String use() {
  final Iterable<String> items = <String>['b', 'a', 'b'];
  final listed = items.toList();
  listed.sort();
  final matches = items.where((s) => s == 'b').length;
  final joined = items.map((s) => s.toUpperCase()).join('-');
  var seen = 0;
  for (final item in items) {
    seen += item.length;
  }
  return '${items.length}/${items.first}/${listed.join("-")}'
      '/$matches/$joined/$seen/${items.isEmpty}';
}
