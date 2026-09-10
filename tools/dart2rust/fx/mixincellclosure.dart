/// A mixin's closure writing a mixin field.
///
/// The CFE does not leave a mixin declaration's bodies in the declaration:
/// it copies them into each application and leaves the declaration hollow.
/// The closures go with them, so a mixin asked on its own which of its
/// fields a closure touches answers "none" -- and then every closure that
/// writes one is refused for capturing `this`, because the field does not
/// look shared and there is nothing to carry but the object.
///
/// `ListNotifierMixin.addListener` in `package:get` is this: it returns a
/// closure that removes the listener from `_updaters`, a non-final field.
///
/// What has to hold is that the closure and the object see one field, not
/// two: a write through the returned closure has to be visible to the next
/// read through the object, and a second closure taken later has to see the
/// first one's write.
library;

mixin Counting {
  List<String>? _log = <String>[];
  int _turns = 0;

  String Function() record(String tag) {
    return () {
      _turns = _turns + 1;
      _log!.add('$tag$_turns');
      return _log!.join(',');
    };
  }

  String get seen => '${_log!.join(",")}/$_turns';
}

class Machine with Counting {
  Machine(this.name);
  final String name;
}

String use() {
  final List<String> out = <String>[];
  final Machine m = Machine('m');
  final String Function() a = m.record('a');
  final String Function() b = m.record('b');
  out.add(a());
  out.add(b());
  // The object sees both writes.
  out.add(m.seen);
  // ..and a closure taken after them sees them too.
  out.add(m.record('c')());
  out.add(m.seen);
  return out.join('|');
}
