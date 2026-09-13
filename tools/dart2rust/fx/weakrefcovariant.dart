/// A `WeakReference<Sub>` stored into a `WeakReference<Base>` field.
///
/// `_RouteEntry.lastAnnouncedPoppedNextRoute` is declared a
/// `WeakReference<_RoutePlaceholder>` and `handleDidPopNext` stores a
/// `WeakReference<Route<dynamic>>(poppedRoute)` into it -- Dart's
/// covariance on a one-element wrapper, where the prelude's is a plain
/// generic: "expected trait `_RoutePlaceholder`, found trait `Route`"
/// (1 stub, the function's next line once its `then` compiled, ws1127).
/// The reference is re-made at the field's type with its target converted,
/// as a `Future<A>` is mapped into a `Future<B>`.
library;

/// Memberless, as `_RoutePlaceholder` is. (A `name` getter on the
/// placeholder, overridden by `NamedRoute`'s field two levels down, read
/// the placeholder's -- a forwarding bug of its own, noted in STATUS.)
abstract class Marker {}

class Placeholder implements Marker {
  const Placeholder();
}

abstract class Route extends Placeholder {
  const Route();
  String get name;
}

class NamedRoute extends Route {
  const NamedRoute(this.name);
  @override
  final String name;
}

class Entry {
  WeakReference<Marker> last = WeakReference<Marker>(const Placeholder());

  void announce(Route route) {
    last = WeakReference<Route>(route);
  }

  String get shown {
    final Marker? target = last.target;
    return target is Route ? target.name : 'placeholder';
  }
}

String use() {
  final Entry e = Entry();
  final String before = e.shown;
  e.announce(const NamedRoute('home'));
  final String after = e.shown;
  return '$before|$after';
}
