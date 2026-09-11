/// A `covariant` parameter is a downcast at the call, and it throws.
///
/// `void handle(covariant Dog d)` implementing `void handle(Animal a)` is
/// ordinary Dart: the call through the interface checks the argument and
/// throws `TypeError` when it is not a `Dog`. The translated forwarder is
/// where that check lives here -- `DogHandler::handle(self, a.dart_cast_to::
/// <dyn Dog>().ok_or_else(|| dart_cast_failed("Dog")).unwrap())` -- and the
/// `.unwrap()` is a *panic*: the whole trait-impl block was emitted with no
/// failure channel, so every cast in it aborted instead of propagating.
///
/// The rule this fixture belongs to: **a panic is never a pass.**
library;

abstract class Animal {
  String get name;
}

class Dog implements Animal {
  Dog(this.name);
  @override
  final String name;
  String bark() => '$name says woof';
}

class Cat implements Animal {
  Cat(this.name);
  @override
  final String name;
}

abstract class Handler {
  String handle(Animal a);
}

class DogHandler implements Handler {
  @override
  String handle(covariant Dog d) => d.bark();
}

String through(Handler h, Animal a) {
  try {
    return h.handle(a);
  } on TypeError catch (_) {
    return 'not-a-dog';
  }
}

String use() {
  final out = <String>[];
  final Handler h = DogHandler();
  out.add(through(h, Dog('rex')));
  out.add(through(h, Cat('tom')));
  out.add(through(h, Dog('fido')));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
