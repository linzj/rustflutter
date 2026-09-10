// A generic function *value* instantiated at a type. Dart writes `f<int>` --
// or infers it, where a generic method is torn off into a slot whose type is
// concrete -- and Kernel records an `Instantiation`.
//
// A Rust closure has no type parameters of its own, and the instantiation is
// exactly what the tear-off underneath was missing: the closure calls the
// method *with* those types. `showDialog` hands `Navigator.of(context).pop`
// over that way and the whole top-level function was refused for it.
class Formatter {
  const Formatter(this.open, this.close);

  final String open;
  final String close;

  String render<T>(T value) => '$open$value$close';
}

String use() {
  const Formatter f = Formatter('<', '>');
  // A generic method torn off into a *concrete* function type: instantiated.
  final String Function(String) renderString = f.render;
  final String Function(int) renderInt = f.render;
  final String Function(bool) renderBool = f.render;
  return '${renderString('a')}${renderInt(7)}'
      '${renderBool(true)}${renderString('b')}';
}
