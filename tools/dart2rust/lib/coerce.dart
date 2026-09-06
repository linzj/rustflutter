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

import 'dart:io' show Platform, stderr;

import 'ir.dart';

/// What `coerce` has to know about the classes a type names.
abstract class TypeWorld {
  /// A trait here: an abstract or open class, or `Object`.
  bool isTrait(String name);

  /// A translated enum: a value that goes behind a fresh handle into a
  /// trait slot, as a struct's does.
  bool isEnum(String name);

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

/// `dart:core` interfaces the prelude spells as a sum of their cases: the
/// case's class to the constructor that wraps it. `Pattern` is a `String`
/// or a `RegExp`, and the prelude's struct holds either.
const preludeSums = {
  'Pattern': {'String': 'of_string', 'RegExp': 'of_regexp'},
};

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

/// Whether two types the same by `sameRust` differ in a projection
/// somewhere -- a function type whose parameter is `<T as DartNullable>::
/// Or` on one side and `Option<T>` on the other needs an adapter.
bool projectionDiffers(IrType a, IrType b) {
  if (a.projected != b.projected) return true;
  if (a.isFunction && b.isFunction) {
    final ap = a.parameters!, bp = b.parameters!;
    if (ap.length != bp.length) return false;
    for (var i = 0; i < ap.length; i++) {
      if (projectionDiffers(ap[i], bp[i])) return true;
    }
    return projectionDiffers(a.returns!, b.returns!);
  }
  if (a.arguments.length != b.arguments.length) return false;
  for (var i = 0; i < a.arguments.length; i++) {
    if (projectionDiffers(a.arguments[i], b.arguments[i])) return true;
  }
  return false;
}

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
/// `void?` is `void`: never an `Option` (the prelude's unit says so).
bool isNullable(IrType t) =>
    t.name != 'void' &&
    t.name != '()' &&
    (t.nullable || (t.name == 'Option' && t.arguments.length == 1));

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
  // `void?` is `void`: the unit, never an `Option` (the prelude's unit
  // says so; `complete(null)` on a `Completer<void>` is `complete(())`).
  if ((t.name == 'void' || t.name == '()') && t.nullable) {
    return IrType(t.name);
  }
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
  if (have0 == null) return value;
  // A `T?` spelled projected (`<T as DartNullable>::Or`, a generic
  // declaration's edge) against the plain `Option<T>` a body works with:
  // the prelude's conversion, one way or the other (`IrNullableOf`).
  if (have0.projected != slot.projected &&
      !have0.isFunction &&
      !slot.isFunction &&
      have0.nullable &&
      slot.nullable &&
      have0.arguments.isEmpty &&
      slot.arguments.isEmpty &&
      have0.name == slot.name) {
    return IrNullableOf(value, slot.name, toOption: !slot.projected)
      ..rustType = slot;
  }
  if (sameRust(have0, slot) && !projectionDiffers(have0, slot)) {
    // A closure literal of exactly the slot's type still goes behind the
    // handle every function slot is (`Listenable.onError = (..) {..}`
    // into a static's `Rc<dyn Fn>`, run453), unless it already is one.
    if (value is IrClosure && !value.boxed && slot.isFunction) {
      return IrCall(value, '!rc', const [])..rustType = slot;
    }
    return value;
  }
  // Anything else into a projected slot: into the plain `Option<T>` first,
  // then the conversion (`None` into a `Vec<T?>` element is `<T as
  // DartNullable>::from_option(None)`; rustc cannot unify an `Option<_>`
  // with the associated type). Out of a projected value the other way.
  if (slot.projected && slot.arguments.isEmpty && !slot.isFunction) {
    final plainSlot = IrType(slot.name, nullable: true);
    final inner = coerceInto(value, plainSlot, world, inClosure: inClosure);
    return IrNullableOf(inner, slot.name, toOption: false)..rustType = slot;
  }
  if (have0.projected && have0.arguments.isEmpty && !have0.isFunction) {
    final plain = IrNullableOf(value, have0.name, toOption: true)
      ..rustType = IrType(have0.name, nullable: true);
    return coerceInto(plain, slot, world, inClosure: inClosure);
  }
  final have = _normal(have0);
  slot = _normal(slot);
  // `DART2RUST_TRACE_COERCE=<name>`: every adaptation whose slot or value
  // names it, to stderr.
  final traced = Platform.environment['DART2RUST_TRACE_COERCE'];
  if (traced != null && (slot.name == traced || have.name == traced)) {
    stderr.writeln(
      'TRACE_COERCE have=$have0 (${have0.projected ? 'projected' : ''}) '
      'slot=$slot (${slot.projected ? 'projected' : ''}) value=${value.runtimeType}',
    );
  }
  // Null into `void?`, which is `void`: the unit (`Completer<void>`'s
  // `complete(null)`, a `SynchronousFuture<void>`'s `_value`, ws461).
  if ((slot.name == 'void' || slot.name == '()') && have.name == 'Null') {
    return IrLiteral('()', const IrType('raw'))..rustType = slot;
  }
  // Dart's `null` where a `dynamic` goes: the `Null` object behind a
  // handle (an omitted `Object? aspect`, a `Object? value = null`; 91
  // `None` where an `Rc<dyn Object>` went once `Object?` was `dynamic`,
  // ws499). Before the `Option` rules: the literal is a nullable `Null`,
  // and mapping it through them made `None.as_ref().map(..)`.
  if ((slot.name == 'dynamic' || slot.name == 'Object') &&
      !isNullable(slot) &&
      have.name == 'Null') {
    return IrStaticCall(null, 'dart_null_object', const [])..rustType = slot;
  }
  // ..and into any `Option` it is the `None` it already is: mapped through
  // the rules below, `null` into a `dynamic?` was `None.as_ref().map(..)`,
  // which types nothing (E0282, ws501).
  if (isNullable(slot) && !slot.projected && have.name == 'Null') return value;
  // The `Option` layer first: on, off, or mapped through.
  // A `dynamic` into a `T?` is null when it holds the `Null` object: the
  // prelude asks (`dart_nullable`), and the value inside goes on by the
  // rule below (a native's answer into `RootIsolateToken?`, run462).
  if (isNullable(slot) && have.name == 'dynamic' && !slot.projected) {
    final asked = IrCall(value, '!nullable', const [])
      ..rustType = const IrType('Object', nullable: true);
    return coerceInto(asked, slot, world, inClosure: inClosure);
  }
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
  // A case into the prelude's sum of it.
  final wrap = preludeSums[slot.name]?[have.name];
  if (wrap != null) {
    return IrStaticCall(slot.name, wrap, [value])..rustType = slot;
  }
  // A future of one type into a future of another (`Future<bool>` into
  // the `Future<dynamic>` a handler slot declares, `setMethodCallHandler(
  // _handleNavigationInvocation)`, run447): the value mapped through the
  // same rule when it arrives (`DartFuture::map`).
  if (have.name == 'Future' &&
      slot.name == 'Future' &&
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
    return IrMapElements(value, 'Future', body)..rustType = slot;
  }
  // `void` into `Object`: the unit value behind a fresh handle, as any
  // value is (the prelude's `Object` is blanket over every `'static`
  // type). A `None` there had no type to infer (65 at ws448).
  if ((have.name == 'void' || have.name == '()') &&
      (slot.name == 'Object' || slot.name == 'dynamic')) {
    return IrUpcast(value, IrType('Object'), handle: false, explicit: true)
      ..rustType = slot;
  }
  // `FutureOr<T>`: a future goes in as one, anything else as a `T`.
  if (slot.name == 'FutureOr' &&
      slot.arguments.length == 1 &&
      have.name != 'FutureOr') {
    if (have.name == 'Future' && have.arguments.length == 1) {
      return IrStaticCall('FutureOr', 'future', [value])..rustType = slot;
    }
    final inner = coerceInto(
      value,
      slot.arguments.single,
      world,
      inClosure: inClosure,
    );
    return IrStaticCall('FutureOr', 'value', [inner])..rustType = slot;
  }
  // A bare `Function` (`dart:core`'s, no signature) is spelled as the
  // object it is (`Rc<dyn Object>`, the backend's type table), so into
  // `Object` it is itself, not a value to put behind a handle.
  final haveObject =
      have.name == 'Object' ||
      have.name == 'dynamic' ||
      (have.name == 'Function' && !have.isFunction);
  final slotObject = slot.name == 'Object' || slot.name == 'dynamic';
  if (scalarNames.contains(have.name) && !slotObject) return value;
  if (scalarNames.contains(slot.name) && !haveObject) return value;
  // Into `Object`: a handle unsizes, a value goes behind a fresh,
  // registered one.
  if (slotObject) {
    if (haveObject) return value;
    if (have.name == 'Null') {
      return IrStaticCall(null, 'dart_null_object', const [])..rustType = slot;
    }
    return IrUpcast(
      value,
      IrType('Object'),
      handle: world.isTrait(have.name) || world.isCounted(have.name),
      explicit: inClosure,
    )..rustType = slot;
  }
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
    if (!adapted && identical(result, call)) {
      // The same function type: a closure literal still goes behind the
      // handle every function slot is (`Rc<dyn Fn>`), unless it already
      // is one (`boxed`).
      if (value is IrClosure && !value.boxed) {
        return IrCall(value, '!rc', const [])..rustType = slot;
      }
      return value;
    }
    // A closure *literal* adapted: the adapter takes over what the
    // literal owns -- the copied fields, the cloned locals, the handle on
    // `this` -- and binds them where it is made, so it is a `move` closure
    // of its own that borrows nothing; the literal inside it, called in
    // place, re-clones from those bindings. Wrapping the literal as it
    // stood put its bindings (`let x = self.x.clone()`) inside the
    // adapter's body, a borrow of `self` in a closure a `'static` slot
    // keeps (ws448).
    if (value is IrClosure) {
      final inner = IrClosure(
        value.params,
        value.body,
        value.returns,
        locals: [for (final c in value.captures) c.name, ...value.locals],
        holdsSelf: value.holdsSelf,
        isAsync: value.isAsync,
      )..rustType = value.rustType;
      final called = IrCallValue(inner, args)..rustType = have.returns;
      final shaped = coerceInto(called, slot.returns!, world, inClosure: true);
      return IrCall(
        IrClosure(
          params,
          IrReturn(shaped),
          slot.returns!,
          captures: value.captures,
          locals: value.locals,
          holdsSelf: value.holdsSelf,
        ),
        '!rc',
        const [],
      )..rustType = slot;
    }
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
    // An empty literal holds whatever the slot holds (`x ?? const []`):
    // retyped, since a `vec![]` mapped element by element has no element
    // type for rustc to infer from.
    if (value is IrListLiteral && value.elements.isEmpty) {
      return IrListLiteral(const [], slot.arguments.single)..rustType = slot;
    }
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
  // A map, key by key and value by value.
  if (have.name == 'Map' &&
      slot.name == 'Map' &&
      have.arguments.length == 2 &&
      slot.arguments.length == 2) {
    // An empty literal holds whatever the slot holds, as a list's does:
    // `Map<SlotType, ChildType> _slotToChild = {}` copied into a class
    // with the mixin's `ChildType` erased (ws490).
    if (value is IrMapLiteral && value.entries.isEmpty) {
      return IrMapLiteral(const [], slot.arguments[0], slot.arguments[1])
        ..rustType = slot;
    }
    final k = IrLocal('k')..rustType = have.arguments[0];
    final v = IrLocal('v')..rustType = have.arguments[1];
    final kb = coerceInto(k, slot.arguments[0], world, inClosure: true);
    final vb = coerceInto(v, slot.arguments[1], world, inClosure: true);
    if (identical(kb, k) && identical(vb, v)) return value;
    return IrMapElements(value, 'Map', IrRecord([kb, vb]))..rustType = slot;
  }
  if (have.name == 'Map' || slot.name == 'Map') return value;
  final haveTrait = world.isTrait(have.name);
  final slotTrait = world.isTrait(slot.name);
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
  // An enum implementing an interface (`_UnspecifiedTextScaler`, an
  // `Ts` here): the value behind a fresh handle (ws510).
  if (slotTrait && world.isEnum(have.name)) {
    return IrUpcast(value, slot, handle: false, explicit: inClosure)
      ..rustType = slot;
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
