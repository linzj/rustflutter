/// A failing `as` throws, and Dart catches it.
///
/// `x as Foo` on a value that is not a `Foo` throws `TypeError` in Dart, and
/// `on TypeError catch` is ordinary Dart -- `try { .. } on TypeError { .. }`
/// is a shape real programs use. This compiler lowered the cast to
/// `dart_cast_to::<Foo>().unwrap()`, which is a *panic*: the program lost a
/// path it still had, and no ruler could see it because the process simply
/// died.
///
/// The rule this fixture belongs to: **a panic is never a pass.** A Dart
/// `throw` is a `Result` on this side, so a Rust panic means a translated
/// program cannot do something the Dart one can.
library;

class Animal {
  const Animal(this.name);
  final String name;
}

class Dog extends Animal {
  const Dog(super.name);
  String bark() => '$name!';
}

class Cat extends Animal {
  const Cat(super.name);
}

String speak(Animal a) {
  try {
    return (a as Dog).bark();
  } on TypeError catch (_) {
    return 'not a dog';
  }
}

String useCast(Object o) {
  try {
    final Dog d = o as Dog;
    return d.bark();
  } on TypeError catch (_) {
    return 'caught';
  }
}

String use() {
  final List<String> out = <String>[];
  // The cast that succeeds still succeeds.
  out.add(speak(const Dog('rex')));
  // The cast that fails is caught, and the program keeps going -- which is
  // the whole point: after a panic there is no "keeps going".
  out.add(speak(const Cat('tom')));
  out.add(useCast(const Dog('fido')));
  out.add(useCast('a string'));
  out.add(useCast(7));
  // ..and the program is still running to say so.
  out.add('alive');
  return out.join('|');
}
