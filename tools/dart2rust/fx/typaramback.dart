// A trait handle read back through the type parameter that promised more.
//
// A generic class whose parameter is bounded by a translated abstract class
// is emitted with the *bound* in its field: `_InheritedModel<T extends
// Model>` holds `final T model` and comes out as `model: Rc<dyn Model>`.
// `ScopedModel.of<T>`'s `return (widget as _InheritedModel<T>).model;` then
// hands an `Rc<dyn Model>` to a slot spelled `T` -- "expected type
// parameter `T`, found `Rc<dyn Model>`", which stubbed the function. The
// same shape stubbed `LayoutInfoType get layoutInfo => constraints as
// LayoutInfoType`.
//
// The coercion into a type parameter's slot existed and only took a value
// that was already `Object`. A trait handle goes to `Object` first -- it
// unsizes -- and the type parameter's own `FromDynamic` takes it back,
// which is Dart's implicit downcast, failing as one does.
//
// What this pins is that the value that comes back out is the *same
// object*, narrowed to what the caller asked for: the two models here are
// different classes with different answers, so picking the wrong one, or
// handing back the bound, shows up in the printed string.

abstract class Model {
  String get label;
}

class Counter extends Model {
  Counter(this.count);
  final int count;
  @override
  String get label => 'counter/$count';
  String twice() => 'counter/${count * 2}';
}

class Named extends Model {
  Named(this.name);
  final String name;
  @override
  String get label => 'named/$name';
  String shout() => name.toUpperCase();
}

class _Holder<T extends Model> {
  _Holder(this.model);
  final T model;
}

// The shape that was stubbed: the field is declared `T`, the class is
// erased to the bound, and the read is returned as `T`.
T unwrap<T extends Model>(_Holder<T> holder) => holder.model;

// ..and through a cast of the holder itself, as `of` does.
T unwrapCast<T extends Model>(Object holder) => (holder as _Holder<T>).model;

String use() {
  final Counter counter = unwrap<Counter>(_Holder<Counter>(Counter(21)));
  final Named named = unwrap<Named>(_Holder<Named>(Named('ada')));
  // A member the *bound* does not have: the value really came back as `T`.
  final String doubled = counter.twice();
  final String shouted = named.shout();
  final Counter viaCast = unwrapCast<Counter>(_Holder<Counter>(Counter(3)));
  return '${counter.label}|${named.label}|$doubled|$shouted|${viaCast.twice()}';
}
