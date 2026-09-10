// A `for` loop variable a mutating method is called on needs `mut` in Rust.
//
// Dart says nothing: `for (final childSet in ..) childSet.removeWhere(..)`
// mutates the set the loop handed out, and the loop's `final` is about the
// binding, not the object. `SemanticsNode.detach` and `sendSemanticsUpdate`
// are that shape.
// The two shapes below are the ones this rule got *wrong* on the way in,
// and neither was caught by the loops above -- they stayed green through
// two versions that made the gallery worse (81 -> 88, then 81 -> 81).
//
// Deciding "the body mutates the loop variable" from the `let mut`
// answer is what broke them: that answer counts every call receiver, so
// `rs.indexOf(r)` and `n.describe()` mark their locals as mutated and the
// loop lends a collection nothing writes to.

// A body that *reads* the collection it iterates. Lent, this is
// "cannot borrow `rs` as immutable because it is also borrowed as
// mutable" (E0502) -- the `_SharedZAxisTransition` shape.
String readsWhileIterating() {
  final List<String> rs = <String>['a', 'bb', 'ccc'];
  final List<int> out = <int>[];
  for (final String r in rs) {
    out.add(rs.indexOf(r));
  }
  return out.join(',');
}

// Elements behind a handle. Cloning one already aliases, so there is
// nothing to lend; lent anyway, the binding becomes `&mut Rc<T>` and stops
// coercing where the object is passed as its trait (E0277, the
// `FocusNode::dispose` shape).
abstract class Node {
  String describe();
}

class Leaf implements Node {
  Leaf(this.label);

  final String label;

  @override
  String describe() => label;
}

String handlesAreNotLent() {
  final List<Node> nodes = <Node>[Leaf('x'), Leaf('y')];
  final List<String> seen = <String>[];
  for (final Node n in nodes) {
    seen.add(n.describe());
  }
  return seen.join('');
}

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
  return '${sizes.join(",")}/${rows.map((r) => r.join("")).join("|")}'
      '/${readsWhileIterating()}/${handlesAreNotLent()}';
}
