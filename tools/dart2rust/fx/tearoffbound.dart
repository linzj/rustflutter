// A tear-off whose receiver is an erased read, handed to a slot that keeps
// it.
//
// `RestorableChangeNotifier<T extends ChangeNotifier>` extends
// `RestorableListenable<T extends Listenable>`, and the field is the
// *base's*: read through the erased trait it comes back as the base's
// bound. `scheduleMicrotask(_value!.dispose)` then had two things wrong at
// once:
//
// * the receiver was an `Rc<dyn Listenable>`, which has no `dispose` -- an
//   erased read has to be narrowed to the type Dart gives it, as a call's
//   receiver and a field write already are (ws1027, ws1029);
// * the tear-off *binds* that receiver first, so its value is a block that
//   hands the closure back, and the boxing a kept function argument gets
//   (`Rc<dyn Fn()>`) only looked at a bare closure. The same tear-off
//   rooted at `this` binds nothing and was boxed all along, which is why
//   this shape and no other was wrong.
//
// The effect is what is checked: the torn method has to land on the object
// the receiver names, and the kept closures have to still work afterwards.

abstract class Listen {
  String get name;
}

abstract class Notifier extends Listen {
  void bump();
}

class Counter extends Notifier {
  Counter(this.name);
  @override
  final String name;
  int count = 0;
  @override
  void bump() {
    count += 1;
  }

  String get label => '$name/$count';
}

// The base's bound is the wider one, and the field is the base's.
abstract class Base<T extends Listen> {
  T? value;
}

// The subclass narrows it, and tears a member off the base's field.
abstract class Sub<T extends Notifier> extends Base<T> {
  void twice() {
    run(value!.bump);
    run(value!.bump);
  }
}

class Holder extends Sub<Counter> {
  Holder(Counter c) {
    value = c;
  }
}

final List<void Function()> _kept = <void Function()>[];

void run(void Function() f) {
  _kept.add(f);
  f();
}

String use() {
  final Counter a = Counter('a');
  Holder(a).twice();
  // ..and the kept ones called again, to show they really were kept.
  for (final void Function() f in _kept) {
    f();
  }
  return a.label;
}
