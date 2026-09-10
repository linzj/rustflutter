// A `for` loop variable a mutating method is called on needs `mut` in Rust.
//
// Dart says nothing: `for (final childSet in ..) childSet.removeWhere(..)`
// mutates the set the loop handed out, and the loop's `final` is about the
// binding, not the object. `SemanticsNode.detach` and `sendSemanticsUpdate`
// are that shape.
String use() {
  final groups = <Set<int>>{
    <int>{1, 2, 3, 4},
    <int>{5, 6, 7, 8},
  };
  for (final group in groups) {
    group.removeWhere((int n) => n.isEven);
  }
  final rows = <List<String>>[
    <String>['a', 'bb', 'ccc'],
    <String>['dddd', 'e'],
  ];
  for (final row in rows) {
    row.removeWhere((String s) => s.length > 2);
    row.add('+');
  }
  final sizes = <int>[];
  for (final group in groups) {
    sizes.add(group.length);
  }
  return '${sizes.join(",")}/${rows.map((r) => r.join("")).join("|")}';
}
