/// A subclass whose own parameter is nullable where the superclass's is
/// not: `class RestorableEnumN<T extends Enum> extends RestorableValue<T?>`.
///
/// The super setter takes the superclass's `V`, instantiated here with
/// `T?` -- which for a kept type parameter is the projection
/// `<T as DartNullable>::Or`. Typed as the plain `T` instead, the
/// coercion put a null-assert in front of a value that is already the
/// nullable form:
///
///     `?` operator cannot convert from `T` to `<T as DartNullable>::Or`
///
/// (1 stub at ws1104, `RestorableEnumN.value=`.)
library;

enum Mode { up, down }

class Base<V> {
  V? _slot;

  V? get slot => _slot;

  set value(V v) {
    _slot = v;
  }

  String show() => '$_slot';
}

class MaybeEnum<T extends Enum> extends Base<T?> {
  @override
  set value(T? v) {
    super.value = v;
  }
}

String use() {
  final MaybeEnum<Mode> m = MaybeEnum<Mode>();
  final List<String> out = <String>[m.show()];
  m.value = Mode.up;
  out.add(m.show());
  m.value = null;
  out.add(m.show());
  m.value = Mode.down;
  out.add('${m.slot}');
  return out.join('|');
}
