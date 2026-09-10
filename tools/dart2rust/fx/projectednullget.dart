// A `T?` that crosses the projected slot at every hop: read out of another
// object through `?.`, `??`ed with a second `T?`, and handed to a function
// *value* whose parameter is one. In a signature a `T?` is
// `<T as DartNullable>::Or`; in a body it is an `Option<T>`, and something
// has to convert at each boundary.
//
// `_RadioListTileState.effectiveGroupValue` is
// `registry?.groupValue ?? widget.groupValue`, and the registry beside it is
// `T? get groupValue => state.effectiveGroupValue` with
// `ValueChanged<T?> get onChanged => state.handleChange`; `RawRadio
// ._handleChanged` calls that value with a `T` and with `null`.
abstract class Registry<T> {
  T? get groupValue;
  void Function(T?) get onChanged;
}

class Client<T> {
  Client(this.registry, this.fallback);

  final Registry<T>? registry;
  final T? fallback;

  /// Two crossings in one expression: `?.` on a `T?` getter, then `??`.
  T? get effective => registry?.groupValue ?? fallback;

  void handle(T? value) {
    registry?.onChanged(value);
  }
}

class Group<T> implements Registry<T> {
  Group(this.client);

  final Client<T> client;
  final List<String> seen = <String>[];

  /// A `T?` getter whose body is another object's `T?`.
  @override
  T? get groupValue => client.fallback;

  void changed(T? value) {
    seen.add('$value');
  }

  /// A method taking `T?`, torn off into a `void Function(T?)`.
  @override
  void Function(T?) get onChanged => changed;
}

String use() {
  final Client<int> lone = Client<int>(null, 7);
  final Group<int> group = Group<int>(lone);
  final Client<int> joined = Client<int>(group, null);
  joined.handle(3);
  // The value called with a non-null `T` and with `null`, as
  // `registry!.onChanged(widget.value)` / `(null)` does.
  final void Function(int?) sink = group.onChanged;
  sink(9);
  sink(null);
  return '${lone.effective}/${joined.effective}/${group.groupValue}'
      '/${group.seen.join(',')}';
}
