/// A `dynamic` local read twice through `??`.
///
/// `x ?? y` on a `dynamic` is `dart_nullable(x)` matched against `Some`, and
/// `dart_nullable` takes the handle *in* and gives it back inside the
/// `Option`. So a bare local is moved by the first `??`, and a second one in
/// the same expression reads a moved value.
///
/// `_MasterDetailScaffold.build` in the gallery is exactly this shape -- it
/// writes `value ?? widget.initialArguments` twice, once for a key and once
/// for the page it builds -- and it was a stub for it (ws1056).
library;

/// Both sides `dynamic`, so the two arms of the `??` agree on the object and
/// nothing but the move is under test. A `dynamic ?? String` needs the
/// fallback boxed and does not type here yet; that is a separate gap, found
/// by this fixture and left named rather than folded in.
String twice(dynamic value, dynamic fallback) {
  return '${value ?? fallback}:${value ?? fallback}';
}

String use() {
  final List<String> out = <String>[];
  out.add(twice('here', 'gone'));
  out.add(twice(null, 'gone'));
  // ..and a non-string dynamic, so the handle really is the object.
  out.add(twice(7, 'gone'));
  // A local, not a parameter, read the same way.
  final dynamic held = twice('h', 'g');
  final String a = '${held ?? 'x'}${held ?? 'y'}';
  out.add(a);
  return out.join('|');
}
