/// A generic base's initialiser, inlined into a subclass constructor that
/// fixes the base's `T`.
///
/// `Provider<T>.value(..) : _ran = _install<T>(value, start)` is run by
/// `ListenProvider extends Provider<Listen>`'s constructor, since the
/// base's fields are the subclass's. The walker that puts the `super(..)`
/// arguments in for the base's parameters copied a static call's type
/// arguments through as they were, and `_install::<T>` named a `T` the
/// subclass has no parameter for (E0425).
///
/// The same walker rebuilt every node without its recorded type. In the
/// gallery that reached `ChangeNotifierProvider.value`, whose inlined
/// `startListening: _startListening` adapter went into `Some(..)` with
/// nothing to unsize against -- `Some({ .. Rc::new(closure) }).clone()`
/// is an `Option<Rc<{closure}>>`, not the `Option<Rc<dyn Fn(..)>>` the
/// slot is (1 stub at ws1116; the only untyped `Some` of a closure in the
/// gallery, `DART2RUST_TRACE_SOME=1`). Here the adapter is typed either
/// way, so this fixture pins the type arguments and the gallery pins the
/// carried type.
library;

typedef Stop = void Function();
typedef Start<T> = Stop Function(Ctx ctx, T value);

/// Not generic, as the erased `InheritedContext` is on the other side.
abstract class Ctx {
  Object? get held;
}

class Box implements Ctx {
  Box(this.held);
  @override
  final Object? held;
}

/// Installs the callback the way `_ValueInheritedProvider` does: called
/// once with the value, and the stop it hands back called after.
String _install<T>(T value, Start<T>? start) {
  if (start == null) {
    return 'none';
  }
  final Stop stop = start(Box(value), value);
  stop();
  return 'ran';
}

class Provider<T> {
  Provider.value({required T value, Start<T>? start})
    : _ran = _install<T>(value, start);
  final String _ran;

  String run() => _ran;
}

abstract class Listen {
  void bump();
  int get count;
}

class Counter implements Listen {
  int _n = 0;
  @override
  void bump() {
    _n++;
  }

  @override
  int get count => _n;
}

class ListenProvider extends Provider<Listen> {
  ListenProvider.value({required Listen value})
    : super.value(value: value, start: _startListening);

  /// Wider than the slot on the value, as `ListenableProvider.
  /// _startListening`'s `Listenable?` is against the erased
  /// `StartListening<T>`: the tear-off is adapted, not cast.
  static Stop _startListening(Ctx e, Object? value) {
    (value as Listen?)?.bump();
    return () => (value as Listen?)?.bump();
  }
}

String use() {
  final Counter c = Counter();
  final ListenProvider p = ListenProvider.value(value: c);
  final String ran = p.run();
  final Provider<int> bare = Provider<int>.value(value: 3);
  return '$ran,${c.count},${bare.run()}';
}
