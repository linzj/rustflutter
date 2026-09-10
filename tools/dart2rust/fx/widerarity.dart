// An override may *add* an optional named parameter, and a mixin's body
// still calls the member with the parameters the mixin declares.
//
// Dart allows it: `SemanticsNode.toDiagnosticsNode` takes `childOrder`
// beside the `name` and `style` that `DiagnosticableTree` declares, and
// `DiagnosticableTreeMixin.toString` calls `toDiagnosticsNode(style: ..)`.
// The mixin's body is emitted *into* the class -- `impl SemanticsNode` --
// where `self.to_diagnostics_node(name, style)` resolves to the class's own
// three-argument method and is one argument short (E0061, which stubbed
// `SemanticsNode.toString`).
//
// The trait's is the one the call means, and the forwarding impl beside it
// already fills the default. What this pins is that the *value* is right
// too: Dart dispatches to the override, so the answer must come from the
// class's own `describe`, with its extra parameter defaulted -- not from
// the mixin's.

mixin Describable {
  String describe({String? name, String style = 'plain'}) =>
      'base/${name ?? '-'}/$style';

  // The mixin's body, calling the member with the mixin's own parameters.
  String render() => describe(name: 'r');
}

class Node with Describable {
  @override
  String describe({String? name, String style = 'plain', int depth = 3}) =>
      'node/${name ?? '-'}/$style/$depth';
}

// A class that applies the mixin and does *not* override: the same call
// must still reach the mixin's own body.
class Plain with Describable {}

String use() {
  final Node node = Node();
  final Plain plain = Plain();
  // Through the mixin's body, and directly, with the extra parameter given.
  return '${node.render()}|${plain.render()}|${node.describe(name: 'd', depth: 9)}';
}
