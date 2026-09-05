// Coercion by type: one rule for a value entering a slot.
//
// Compare the value's Rust type (`IrExpr.rustType`) with the slot's, and
// adapt the difference -- an `Option` layer, a scalar widening, a handle up
// or down the trait hierarchy, a value put behind a handle, a collection
// rebuilt element by element. The front end lowers into it; the back end's
// forwarders (a trait impl delegating to the struct's own method) go through
// the same rule rather than matching the spelled Rust types by prefix.
//
// The rule needs a few facts about *names* that only its caller has -- which
// class is a trait here, which is counted, which is below which -- and asks
// for them through [TypeWorld], so it knows neither Kernel nor the backend.
library;

import 'ir.dart';

/// What `coerce` has to know about the classes a type names.
abstract class TypeWorld {
  /// A trait here: an abstract or open class, or `Object`.
  bool isTrait(String name);

  /// A counted class: its values are handles already.
  bool isCounted(String name);

  /// A translated class that is a struct here (not a trait, not an enum).
  bool isStruct(String name);

  /// Whether `sub` is `sup` or below it in the class hierarchy.
  bool isBelow(String sub, String sup);

  /// A generic value struct: a downcast to it is not cloned out of the
  /// reference `Any` hands back (its `T` has no `Clone` there).
  bool isGenericValueStruct(String name);
}

const scalarNames = {'int', 'double', 'num', 'bool', 'String'};
const collectionNames = {'List', 'Iterable', 'Set'};

/// The Rust spelling of a scalar class's name.
String rustScalar(String name) =>
    const {
      'num': 'f64',
      'double': 'f64',
      'int': 'i64',
      'bool': 'bool',
      'String': 'String',
    }[name] ??
    name;

String normalName(String name) => switch (name) {
  'Iterable' => 'List',
  'dynamic' => 'Object',
  'num' => 'double',
  _ => name,
};

/// Whether two IR types spell the same Rust type.
bool sameRust(IrType a, IrType b) => _sameNormal(_normal(a), _normal(b));

bool _sameNormal(IrType a, IrType b) {
  if (a.nullable != b.nullable) return false;
  if (a.isFunction || b.isFunction) {
    if (!(a.isFunction && b.isFunction)) return false;
    final ap = a.parameters!, bp = b.parameters!;
    if (ap.length != bp.length) return false;
    for (var i = 0; i < ap.length; i++) {
      if (!_sameNormal(ap[i], bp[i])) return false;
    }
    return _sameNormal(a.returns!, b.returns!);
  }
  if (normalName(a.name) != normalName(b.name)) return false;
  if (a.arguments.length != b.arguments.length) {
    // `List` alone is `List<dynamic>` to the backend.
    return a.arguments.isEmpty || b.arguments.isEmpty;
  }
  for (var i = 0; i < a.arguments.length; i++) {
    if (!_sameNormal(a.arguments[i], b.arguments[i])) return false;
  }
  return true;
}

IrType nonNull(IrType t) => t.isFunction
    ? IrType.function(t.parameters!, t.returns!)
    : IrType(t.name, arguments: t.arguments);

/// An `Option` layer is either the `nullable` flag or, when the type under
/// it is nullable itself, an explicit `Option` wrapper: `T?` with `T` bound
/// to `Color?` is `Option<Option<Rc<dyn Color>>>`, which Dart collapses and
/// Rust does not.
bool isNullable(IrType t) =>
    t.nullable || (t.name == 'Option' && t.arguments.length == 1);

IrType stripNull(IrType t) => t.name == 'Option' && t.arguments.length == 1
    ? t.arguments.single
    : nonNull(t);

IrType withNull(IrType t) {
  if (isNullable(t)) return IrType('Option', arguments: [t]);
  return t.isFunction
      ? IrType.function(t.parameters!, t.returns!, nullable: true)
      : IrType(t.name, nullable: true, arguments: t.arguments);
}

/// One spelling for each type: an `Option` wrapper over a non-nullable
/// type is that type's `nullable` flag.
IrType _normal(IrType t) {
  if (t.name == 'Option' && t.arguments.length == 1) {
    final inner = _normal(t.arguments.single);
    return isNullable(inner)
        ? IrType('Option', arguments: [inner])
        : withNull(inner);
  }
  if (t.isFunction) {
    return IrType.function(
      [for (final p in t.parameters!) _normal(p)],
      _normal(t.returns!),
      nullable: t.nullable,
    );
  }
  if (t.arguments.isEmpty) return t;
  return IrType(
    t.name,
    nullable: t.nullable,
    arguments: [for (final a in t.arguments) _normal(a)],
  );
}

/// `value`, adapted to `slot`; `value` itself when nothing is known (an
/// untyped value) or nothing needs doing. `inClosure`: the value stands
/// where nothing expects a type (a closure body), so a cast is spelled.
IrExpr coerceInto(
  IrExpr value,
  IrType slot,
  TypeWorld world, {
  bool inClosure = false,
}) {
  final have0 = value.rustType;
  if (have0 == null || sameRust(have0, slot)) return value;
  final have = _normal(have0);
  slot = _normal(slot);
  // The `Option` layer first: on, off, or mapped through.
  if (isNullable(slot) && !isNullable(have)) {
    final inner = coerceInto(
      value,
      stripNull(slot),
      world,
      inClosure: inClosure,
    );
    if (identical(inner, value) && !sameRust(have, stripNull(slot))) {
      return value;
    }
    return IrSome(inner)..rustType = slot;
  }
  if (!isNullable(slot) && isNullable(have)) {
    if (slot.name == 'dynamic') {
      // `dynamic` admits null: absent is the `Null` object.
      final element = IrCall(IrBound(), 'clone', const [])
        ..rustType = stripNull(have);
      final shared = coerceInto(element, slot, world, inClosure: true);
      final mapped = identical(shared, element)
          ? value
          : (IrNullAware(value, shared)
              ..rustType = IrType('dynamic', nullable: true));
      return IrCall(mapped, '!or_null', const [])..rustType = slot;
    }
    final inner = IrNullCheck(value)..rustType = stripNull(have);
    return coerceInto(inner, slot, world, inClosure: inClosure);
  }
  if (isNullable(slot) && isNullable(have)) {
    // Two layers into one: Dart's `T?` with `T` bound to `Color?` is one
    // `Color?`, so `Option<Option<..>>` into `Option<..>` is `flatten`
    // (both `None` and `Some(None)` are Dart's null), not an unwrap.
    if (have.name == 'Option' && slot.name != 'Option') {
      final flat = IrCall(value, 'flatten', const [])
        ..rustType = have.arguments.single;
      return coerceInto(flat, slot, world, inClosure: inClosure);
    }
    final element = IrCall(IrBound(), 'clone', const [])
      ..rustType = stripNull(have);
    final inner = coerceInto(element, stripNull(slot), world, inClosure: true);
    if (identical(inner, element)) return value;
    return IrNullAware(value, inner)..rustType = slot;
  }
  // `()` where an `Option` goes: nothing, then `None` (`Action.invoke`
  // overridden as `void` under a trait returning `Object?`).
  if (have.name == 'void' && slot.nullable) {
    return IrBlockValue([
      IrExprStmt(value),
    ], IrLiteral('None', const IrType('raw')))..rustType = slot;
  }
  // Both present. Scalars: `int` into a `double` slot is cast; a `num`
  // slot is `dart:core`'s polymorphic one (`num.+` takes `num`, and `i +
  // 1` stays an `i64`), and a translated callee's `num` is the front
  // end's `_numLiteral` to make an `f64`.
  if (slot.name == 'num') return value;
  if (have.name == 'int' && slot.name == 'double') {
    return IrCast(value, 'f64')..rustType = slot;
  }
  final haveObject = have.name == 'Object' || have.name == 'dynamic';
  final slotObject = slot.name == 'Object' || slot.name == 'dynamic';
  if (scalarNames.contains(have.name) && !slotObject) return value;
  if (scalarNames.contains(slot.name) && !haveObject) return value;
  // Function types: an adapter closure, each parameter coerced from the
  // slot's type to the function's and the result back (`lerp<Color?>`'s
  // `T? Function(T?, T?, double)` wants `Option<Option<..>>` parameters
  // where the closure written takes `Option<..>`).
  if (have.isFunction && slot.isFunction) {
    final hp = have.parameters!, sp = slot.parameters!;
    if (hp.length != sp.length) return value;
    final params = <IrParam>[];
    final args = <IrExpr>[];
    var adapted = false;
    for (var i = 0; i < sp.length; i++) {
      final name = '__a$i';
      params.add(IrParam(name, sp[i]));
      final given = IrLocal(name)..rustType = sp[i];
      final arg = coerceInto(given, hp[i], world, inClosure: true);
      if (!identical(arg, given)) adapted = true;
      args.add(arg);
    }
    final call = IrCallValue(value, args)..rustType = have.returns;
    final result = coerceInto(call, slot.returns!, world, inClosure: true);
    if (!adapted && identical(result, call)) return value;
    return IrCall(
      IrClosure(params, IrReturn(result), slot.returns!),
      '!rc',
      const [],
    )..rustType = slot;
  }
  if (have.isFunction || slot.isFunction) return value;
  // Collections, element by element.
  if (collectionNames.contains(have.name) &&
      collectionNames.contains(slot.name) &&
      normalName(have.name) == normalName(slot.name) &&
      have.arguments.length == 1 &&
      slot.arguments.length == 1) {
    final element = IrLocal('v')..rustType = have.arguments.single;
    final body = coerceInto(
      element,
      slot.arguments.single,
      world,
      inClosure: true,
    );
    if (identical(body, element)) return value;
    return IrMapElements(value, normalName(slot.name), body)..rustType = slot;
  }
  if (have.name == 'Map' || slot.name == 'Map') return value;
  final haveTrait = world.isTrait(have.name);
  final slotTrait = world.isTrait(slot.name);
  // Into `Object`: a handle unsizes, a value goes behind a fresh,
  // registered one.
  if (slotObject) {
    if (haveObject || have.name == 'Null') return value;
    return IrUpcast(
      value,
      IrType('Object'),
      handle: haveTrait || world.isCounted(have.name),
      explicit: inClosure,
    )..rustType = slot;
  }
  // Out of `Object`: a scalar by `Any`, cloned out of the reference.
  if (haveObject && scalarNames.contains(slot.name)) {
    return IrCall(IrDowncast(value, rustScalar(slot.name)), 'clone', const [])
      ..rustType = slot;
  }
  if (haveTrait && slotTrait) {
    // The same trait with other arguments (`Tween<f64>` into a
    // `Tween<Object>`) has no cast.
    if (have.name == slot.name) return value;
    if (world.isBelow(have.name, slot.name)) {
      return IrUpcast(value, slot, handle: true, explicit: inClosure)
        ..rustType = slot;
    }
    return IrCastTo(value, slot)..rustType = slot;
  }
  if (slotTrait && world.isStruct(have.name)) {
    return IrUpcast(
      value,
      slot,
      handle: world.isCounted(have.name),
      explicit: inClosure,
    )..rustType = slot;
  }
  if (haveTrait && world.isStruct(slot.name)) {
    // Down to a struct through `Any`, with the slot's kept type arguments.
    // A generic value struct is not cloned out of the reference; a counted
    // one and a plain one are.
    final cast = IrDowncast(
      value,
      rustScalar(slot.name),
      arguments: slot.arguments,
    );
    final out = world.isGenericValueStruct(slot.name)
        ? cast
        : IrCall(cast, 'clone', const []);
    return out..rustType = slot;
  }
  return value;
}
