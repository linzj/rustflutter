/// A generic base's `T?` field, fixed to a `Map<K, V>` by a subclass that is
/// also used at the wider `Object?`.
///
/// `ShortcutMapProperty extends DiagnosticsProperty<Map<ShortcutActivator,
/// Intent>>` and is handed around as a `DiagnosticsProperty<Object?>`, so
/// the wider impl reads the `_value` field back as an `Object?`. The field
/// is a projected `Map<..>?`, and unprojecting it spelled the type by name
/// alone: `<Map<Rc<dyn DartAny>, Rc<dyn DartAny>> as DartNullable>::option
/// (__v)` for a `Map<Rc<dyn ShortcutActivator>, Rc<dyn Intent>>` ("expected
/// trait `DartAny`, found trait `ShortcutActivator`", 1 stub at ws1116).
library;

abstract class Named {
  String get name;
}

class Key implements Named {
  Key(this.name);
  @override
  final String name;
}

/// What `DiagnosticsNode` is to `DiagnosticsProperty<T>`: the base every
/// property is handed around as, whose `value` is an `Object?`.
abstract class Node {
  Object? get value;
  String show();
}

class Prop<T> extends Node {
  Prop(this._value);
  final T? _value;

  @override
  T? get value => _value;

  @override
  String show() => _value == null ? 'none' : 'some';
}

class MapProp extends Prop<Map<Named, int>> {
  MapProp(super.value);

  String get total {
    final Map<Named, int>? m = _value;
    if (m == null) {
      return '0';
    }
    int sum = 0;
    for (final int v in m.values) {
      sum += v;
    }
    // The names are read, so the field stays and two keys stay two: a
    // value struct with no field left compares equal to every other.
    final String names = m.keys.map((Named k) => k.name).join('+');
    return '$names=$sum';
  }
}

/// The wider view: every property is a `Node` here.
String describe(Node p) => '${p.show()}/${p.value == null}';

String use() {
  final MapProp m = MapProp(<Named, int>{Key('a'): 1, Key('b'): 2});
  final MapProp empty = MapProp(null);
  return '${describe(m)}|${describe(empty)}|${m.total}|${empty.total}|${describe(Prop<int>(3))}';
}
