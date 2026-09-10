// `(field ??= C()).add(x)` on a nullable collection field.
//
// The `??=` reads the field, and the read is a *copy*: the `add` went into
// the copy and the field stayed empty. `Element.dependOnInheritedElement`
// does exactly this, so `_dependencies` was always empty,
// `_ensureDeactivated`'s loop never ran, and a defunct element stayed a
// dependent -- `notifyClients` then reached one whose render object was
// gone (`lifecycle=Defunct`, run906).
class Box {
  Set<String>? tags;

  void tag(String t) {
    (tags ??= <String>{}).add(t);
  }
}

String use() {
  final box = Box();
  box.tag('a');
  box.tag('b');
  box.tag('a');
  final tags = box.tags;
  return '${tags == null ? -1 : tags.length}';
}
