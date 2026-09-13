/// A field overriding a getter inherited through a concrete base, read
/// through the interface.
///
/// `NamedRoute.name` is a field; `Placeholder.name` is the getter it
/// overrides, two levels up through the abstract `Route`. Read as a
/// `Marker`, the interface impl for `NamedRoute` forwarded `name` to
/// `Placeholder::name(self)` -- the base's getter -- and answered
/// 'placeholder' for a route named 'home' (found beside the weakrefcovariant
/// fixture, ws1128). A silent wrong answer, not a stub.
library;

abstract class Marker {
  String get name;
}

class Placeholder implements Marker {
  const Placeholder();
  @override
  String get name => 'placeholder';
}

abstract class Route extends Placeholder {
  const Route();
}

class NamedRoute extends Route {
  const NamedRoute(this.name);
  @override
  final String name;
}

String describe(Marker m) => m.name;

String use() {
  const Marker a = Placeholder();
  const Route b = NamedRoute('home');
  final Marker c = NamedRoute('settings');
  return '${describe(a)}|${describe(b)}|${describe(c)}|${b.name}';
}
