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

  /// A type parameter in scope where the coercion is emitted: the class's
  /// own or the member's. One has a `FromDynamic` bound; a name that is
  /// neither a class nor in scope (a super constructor's `T`) has nothing.
  bool isTypeParameter(String name);
}

const scalarNames = {'int', 'double', 'num', 'bool', 'String'};
const collectionNames = {'List', 'Iterable', 'Set'};

/// The `dart:` map classes, all the prelude's one `Map`.
const mapNames = {'Map', 'LinkedHashMap', 'HashMap', 'SplayTreeMap'};

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

/// The one Rust name two Dart names spell. `Iterable` is *not* here: it
/// is `Rc<dyn DartIterable<T>>` since ws908, a different type from the
/// `Vec<T>` a `List` is, and saying they were the same is what let a
/// `Vec` reach an `Iterable` slot unconverted at every site `sameRust`
/// guards (`Some(vec![])` into an `Option<Rc<dyn DartIterable<String>>>`,
/// 13 of them in one round).
String normalName(String name) => switch (name) {
  'dynamic' => 'Object',
  'num' => 'double',
  // The `dart:` set classes are the prelude's one `Set`, as the map
  // classes are its one `Map` (`mapNames`). Unnamed here, a
  // `LinkedHashSet` reached every `Set` rule as a stranger and went into
  // a `Vec` slot unconverted -- which is `OverlayState.rearrange`'s
  // `_entries.insertAll(index, old)`, a stub since ws638.
  'LinkedHashSet' || 'HashSet' || 'SplayTreeSet' || '_Set' => 'Set',
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

/// Whether this compiler can place `t` as something other than one of the
/// prelude's collections: a trait, an enum, a struct, a counted class, a
/// type parameter, a function, a scalar, a map, a record, a future, a
/// stream, an iterator, a top or bottom type.
///
/// Asked this way round because the collections have no closed list of
/// names: an AOT dill types a list literal `_GrowableList`, a `Queue` is
/// the prelude's `VecDeque`, a `LinkedHashSet` its `Set`. What is left
/// after everything placeable is placed is a collection, and a name that
/// slips through fails loudly at the one site that uses it rather than
/// quietly going in unconverted.
bool _placedElsewhere(IrType t, TypeWorld world) =>
    t.isFunction ||
    scalarNames.contains(t.name) ||
    mapNames.contains(t.name) ||
    const {
      'dynamic',
      'Object',
      'Null',
      'Never',
      'void',
      '()',
      'raw',
      '_',
      'Record',
      'Future',
      'Stream',
      'DartIterator',
    }.contains(t.name) ||
    world.isTypeParameter(t.name) ||
    world.isTrait(t.name) ||
    world.isEnum(t.name) ||
    world.isStruct(t.name) ||
    world.isCounted(t.name);

/// Whether the value is the one line Dart's AOT compiler proved dead
/// (`IrLiteral.unreachable`), reached through the block that leads to it.
bool _diverges(IrExpr e) =>
    identical(e, IrLiteral.unreachable) ||
    (e is IrBlockValue && _diverges(e.value));

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
  // An integer *literal* where a `double` goes is a double: that is Dart's
  // rule about the literal, and it holds whoever declares the slot -- a
  // prelude callee's slots are filled here rather than by `_widened`, so
  // `lerpDouble(a, 0, t)` never saw it (9 at ws779). Before the early
  // return below, because a literal carries no recorded type of its own.
  // Suffixed, or two unsuffixed float literals make an ambiguous `{float}`.
  if (value is IrLiteral &&
      value.type.name == 'int' &&
      nonNull(slot).name == 'double') {
    final spelled = IrLiteral('${value.value}.0_f64', const IrType('double'))
      ..rustType = const IrType('double');
    return isNullable(slot) ? (IrSome(spelled)..rustType = slot) : spelled;
  }
  final have0 = value.rustType;
  if (have0 == null) return value;
  // A block that produces a closure -- a tear-off that binds its receiver
  // first -- is adapted as the closure is, and rewrapped: the slot wants
  // the `Rc<dyn Fn>` a closure literal goes behind (run745).
  if (value is IrBlockValue && value.value is IrClosure) {
    final inner = coerceInto(value.value, slot, world, inClosure: inClosure);
    if (identical(inner, value.value)) return value;
    return IrBlockValue(value.statements, inner)..rustType = inner.rustType;
  }
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
  // ..unless a closure literal is written with more parameters than its
  // recorded type says (a tear-off adapter typed by its slot): that one
  // goes on to the function rule, which adapts the arity (ws549).
  final arityDiffers =
      value is IrClosure &&
      slot.isFunction &&
      slot.parameters != null &&
      value.params.length != slot.parameters!.length;
  if (sameRust(have0, slot) &&
      !projectionDiffers(have0, slot) &&
      !arityDiffers) {
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
  // Any other value into `void`: evaluated and dropped, as Dart drops it
  // (`_paintChildWithTransform`, returning `TransformLayer?`, torn off
  // into `pushClipRect`'s `void Function(PaintingContext, Offset)`:
  // the adapter returned the layer where `()` went, ws538).
  if ((slot.name == 'void' || slot.name == '()') &&
      have.name != 'void' &&
      have.name != '()' &&
      have.name != 'raw' &&
      have.name != '_' &&
      have.name != 'Never' &&
      !have.projected) {
    return IrBlockValue([
      IrExprStmt(value),
    ], IrLiteral('()', const IrType('raw')))..rustType = slot;
  }
  // Dart's `null` where a `dynamic` goes: the `Null` object behind a
  // handle (an omitted `Object? aspect`, a `Object? value = null`; 91
  // `None` where an `Rc<dyn Object>` went once `Object?` was `dynamic`,
  // ws499). Before the `Option` rules: the literal is a nullable `Null`,
  // and mapping it through them made `None.as_ref().map(..)`.
  if ((slot.name == 'dynamic' || slot.name == 'Object') &&
      !isNullable(slot) &&
      have.name == 'Null') {
    final nullObject = IrStaticCall(null, 'dart_null_object', const [])
      ..rustType = slot;
    // A `Null`-typed *call* still runs (the `Function` adapter around a
    // void callback, the dynfall fixture): its value is the null object.
    if (value is IrLiteral) return nullObject;
    return IrBlockValue([IrExprStmt(value)], nullObject)..rustType = slot;
  }
  // ..and into any `Option` it is the `None` it already is: mapped through
  // the rules below, `null` into a `dynamic?` was `None.as_ref().map(..)`,
  // which types nothing (E0282, ws501).
  if (isNullable(slot) && !slot.projected && have.name == 'Null') return value;
  // The `Option` layer first: on, off, or mapped through.
  // A `dynamic` into a `T?` is null when it holds the `Null` object: the
  // prelude asks (`dart_nullable`), and the value inside goes on by the
  // rule below (a native's answer into `RootIsolateToken?`, run462).
  // ..a `dynamic` that is not an `Option` already: a `dynamic?` (an erased
  // `T?` read) is one, and maps through the rule below.
  if (isNullable(slot) &&
      have.name == 'dynamic' &&
      !isNullable(have) &&
      !slot.projected) {
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
  // A `void` value where a `dynamic` goes: Dart's `void` has one value and
  // it is `null`, so the expression runs and the null object is what comes
  // out. `return switch (m) { 'commit' => _handleCommit(), .. }` out of a
  // `Future<dynamic>` has three `void` arms and one `bool`
  // (`WidgetsBinding._handleBackGestureInvocation`, two stubs).
  if ((slot.name == 'dynamic' || slot.name == 'Object') &&
      !isNullable(slot) &&
      (have.name == 'void' || have.name == '()')) {
    final nullObject = IrStaticCall(null, 'dart_null_object', const [])
      ..rustType = slot;
    return IrBlockValue([IrExprStmt(value)], nullObject)..rustType = slot;
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
  // An iterator of one element type into an iterator of another
  // (`Iterator<Sub>` where `Iterator<Base>?` is declared): the prelude's
  // mapped iterator, each element through this rule (`dart_iterator_map`,
  // run638).
  if (have.name == 'DartIterator' &&
      slot.name == 'DartIterator' &&
      have.arguments.length == 1 &&
      slot.arguments.length == 1 &&
      !sameRust(have.arguments.single, slot.arguments.single)) {
    final element = IrLocal('v')..rustType = have.arguments.single;
    final body = coerceInto(
      element,
      slot.arguments.single,
      world,
      inClosure: true,
    );
    if (identical(body, element)) return value;
    return IrMapElements(value, 'Iterator', body)..rustType = slot;
  }
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
    // With the slot's `T` spelled: inferred from a concrete value inside
    // (`Rc<GalleryLocalizationsZu>` where `Rc<dyn GalleryLocalizations>`
    // was the slot), the `FutureOr` was the wrong one for `then`'s bound.
    final held = slot.arguments.single;
    final spelled = _spellable(held) ? [held] : const <IrType>[];
    if (have.name == 'Future' && have.arguments.length == 1) {
      return IrStaticCall(null, 'future_or_future', [
        value,
      ], typeArguments: spelled)..rustType = slot;
    }
    final inner = coerceInto(value, held, world, inClosure: inClosure);
    return IrStaticCall(null, 'future_or_value', [
      inner,
    ], typeArguments: spelled)..rustType = slot;
  }
  // A bare `Function` (`dart:core`'s, no signature) is spelled as the
  // object it is (`Rc<dyn Object>`, the backend's type table), so into
  // `Object` it is itself, not a value to put behind a handle.
  final haveObject =
      have.name == 'Object' ||
      have.name == 'dynamic' ||
      (have.name == 'Function' && !have.isFunction);
  // ..and a bare `Function` slot is spelled as the object too.
  final slotObject =
      slot.name == 'Object' ||
      slot.name == 'dynamic' ||
      (slot.name == 'Function' && !slot.isFunction);
  if (scalarNames.contains(have.name) && !slotObject) {
    // ..unless the slot is an interface the scalar implements: Dart's
    // `num implements Comparable<num>`, and a `Comparable<num>` slot is
    // an `Rc<dyn Comparable<f64>>` (`_sort`'s field getter in the data
    // table demo). The value goes behind a fresh handle, as an enum's
    // does below; the prelude's impls say which scalars can (ws875).
    if (world.isTrait(slot.name) && !slot.isFunction && !isNullable(slot)) {
      return IrUpcast(value, slot, handle: false, explicit: inClosure)
        ..rustType = slot;
    }
    return value;
  }
  if (scalarNames.contains(slot.name) && !haveObject) return value;
  // Into `Object`: a handle unsizes, a value goes behind a fresh,
  // registered one.
  if (slotObject) {
    if (haveObject) return value;
    if (have.name == 'Null') {
      final nullObject = IrStaticCall(null, 'dart_null_object', const [])
        ..rustType = slot;
      // A `Null`-typed *call* still runs: the `Function` adapter around a
      // void callback returned the null object and never called it
      // (`flag.value = true` inside, the dynfall fixture).
      if (value is IrLiteral) return nullObject;
      return IrBlockValue([IrExprStmt(value)], nullObject)..rustType = slot;
    }
    // A typed function value into a bare `Function`: the prelude's
    // function object, called dynamically -- an adapter taking its
    // arguments as objects, each coerced into the function's own type, and
    // handing its result back as one. The function itself is bound first
    // and moved in, so the adapter borrows nothing.
    if (have.isFunction) {
      return _dynamicFunction(value, have, world)..rustType = slot;
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
    final sp = slot.parameters!;
    // The value's parameters: a closure literal's own (a tear-off adapter
    // is typed by its slot while written with the method's, ws549), else
    // its type's.
    final literal = value is IrClosure
        ? value
        : value is IrCall &&
              value.name == '!rc' &&
              value.args.isEmpty &&
              value.target is IrClosure
        ? value.target as IrClosure
        : null;
    final hp = literal != null
        ? [for (final p in literal.params) p.type]
        : have.parameters!;
    // More parameters than the slot takes, the extra ones nullable: Dart
    // lets `focusNode.requestFocus` (one optional `FocusNode?`) stand as
    // a `VoidCallback`, and the extra parameters are absent (`_FocusState.
    // build`, ws547; `cond ? f.requestFocus : null`, a conditional, ws549).
    final extra = hp.length > sp.length && hp.skip(sp.length).every(isNullable)
        ? hp.length - sp.length
        : 0;
    if (hp.length != sp.length && extra == 0) return value;
    final params = <IrParam>[];
    final args = <IrExpr>[];
    var adapted = extra > 0;
    for (var i = 0; i < sp.length; i++) {
      final name = '__a$i';
      params.add(IrParam(name, sp[i]));
      final given = IrLocal(name)..rustType = sp[i];
      final arg = coerceInto(given, hp[i], world, inClosure: true);
      if (!identical(arg, given)) adapted = true;
      args.add(arg);
    }
    for (var i = sp.length; i < hp.length; i++) {
      args.add(IrLiteral('None', const IrType('raw'))..rustType = hp[i]);
    }
    if (literal != null && extra > 0) {
      // The literal behind its handle, if it had one, is what the adapter
      // calls: rebuilt below as any literal is.
      value = literal;
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
    // Any other function value -- a field read, a `??` of two, a
    // tear-off -- is bound first and moved in, as the literal's bindings
    // are: emitted inside the adapter's body, a tear-off's `let __me =
    // self..` borrowed `self` in a closure a widget keeps (`WidgetsApp.
    // build`'s `onNavigationNotification`, ws517).
    final bound = IrLocal('__f')..rustType = have;
    final rebound = IrCallValue(bound, args)..rustType = have.returns;
    final reshaped = coerceInto(rebound, slot.returns!, world, inClosure: true);
    return IrBlockValue(
      [IrLocalDecl('__f', null, value)],
      IrCall(
        IrClosure(
          params,
          IrReturn(reshaped),
          slot.returns!,
          locals: const ['__f'],
        ),
        '!rc',
        const [],
      ),
    )..rustType = slot;
  }
  // A bare `Function` (the object a function value went behind) into a
  // typed function slot: a closure of the slot's type calling it
  // dynamically, each argument as an object, the result coerced back.
  if (slot.isFunction && haveObject) {
    return _typedFunction(value, slot, world)..rustType = slot;
  }
  if (have.isFunction || slot.isFunction) return value;
  // Two `Iterable` handles whose elements differ: materialised, mapped as
  // the list it is, and handed back as a handle. The trait has no `map`
  // and the handle no `into_iter`, so the element-by-element rule below
  // cannot be asked directly.
  if (have.name == 'Iterable' &&
      slot.name == 'Iterable' &&
      have.arguments.length == 1 &&
      slot.arguments.length == 1 &&
      !sameRust(have.arguments.single, slot.arguments.single)) {
    final listed = IrCall(value, 'dart_to_list', const [])
      ..rustType = IrType('List', arguments: have.arguments);
    return coerceInto(listed, slot, world, inClosure: inClosure);
  }
  // Collections, element by element.
  if (collectionNames.contains(normalName(have.name)) &&
      collectionNames.contains(normalName(slot.name)) &&
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
  // A record into a record slot, field by field, as a list's elements are:
  // a record literal is typed by what was *written* in it, and the slot may
  // spell a field wider (`(inside: <RenderTapRegion>{..}, outside: [..])`
  // returned where the typedef says `Iterable`, `_classifyRegions` at
  // ws754). Only a literal: a record already in a place would have to be
  // taken apart and rebuilt, and nothing asks for that yet.
  if (have.name == 'Record' &&
      slot.name == 'Record' &&
      value is IrRecord &&
      have.arguments.length == slot.arguments.length &&
      value.fields.length == slot.arguments.length) {
    final fields = [
      for (var i = 0; i < value.fields.length; i++)
        coerceInto(
          value.fields[i],
          slot.arguments[i],
          world,
          inClosure: inClosure,
        ),
    ];
    var same = true;
    for (var i = 0; i < fields.length; i++) {
      if (!identical(fields[i], value.fields[i])) same = false;
    }
    if (same) return value;
    return IrRecord(fields)..rustType = slot;
  }
  // A map, key by key and value by value -- the prelude's one `Map`
  // under every `dart:` map name (`LinkedHashMap<Locale, X>` from
  // `_getLocaleOptions()` into a `T := Locale?` slot, run661).
  if (mapNames.contains(have.name) &&
      mapNames.contains(slot.name) &&
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
  // A `dynamic` into a collection slot: the prelude's checked conversion
  // (`dart_cast_map`/`dart_cast_list`, as `as Map<..>` takes): a wider
  // impl's forwarder handed `_HistoryProperty.initWithValue` the bare
  // handle where its `Map<String?, List<Object>>?` went (run629).
  if (haveObject &&
      (slot.name == 'Map' || slot.name == 'List') &&
      !isNullable(slot) &&
      slot.arguments.isNotEmpty) {
    return IrDowncast(value, slot.name, arguments: slot.arguments)
      ..rustType = slot;
  }
  if (mapNames.contains(have.name) || mapNames.contains(slot.name)) {
    return value;
  }
  // Into an `Iterable<T>` slot: the list it would be read as, then the
  // handle. `Iterable` is `Rc<dyn DartIterable<T>>` since ws908 -- the
  // trait every collection here implements -- so a `Vec`, a `Set`, a
  // `VecDeque` and a `LinkedList` all reach it the same way, which is
  // what Dart says and what the one `Vec` spelling could never say.
  //
  // Which values those are is asked the honest way round: a name this
  // compiler can place as something else -- a trait, an enum, a struct,
  // a type parameter, a scalar, a map, a function, a top type -- is not
  // one of the prelude's collections, and everything left is. Naming the
  // collections instead misses the dill's own spellings, and there is no
  // list of them to keep: `<String>[]` is a `_GrowableList` there, a
  // `Queue` is a `VecDeque`, a `LinkedHashSet` is a `Set`.
  if (slot.name == 'Iterable' &&
      have.name != 'Iterable' &&
      slot.arguments.length == 1 &&
      // ..and a value that is really there: a body TFA removed produces
      // the one diverging literal, and `Rc::new(unreachable!().clone())`
      // has no type to infer (E0282, `CanonicalizedMap.keys` and three
      // more).
      !_diverges(value) &&
      !_placedElsewhere(have, world)) {
    final listed = coerceInto(
      value,
      IrType('List', arguments: slot.arguments),
      world,
      inClosure: inClosure,
    );
    if (Platform.environment['DART2RUST_TRACE_BOX'] != null) {
      final frames = StackTrace.current.toString().split('\n');
      stderr.writeln(
        'TRACE_BOX have=${have.name} slot=${slot.name} :: '
        '${frames.take(8).map((f) => f.trim()).join(' | ')}',
      );
    }
    return IrCall(listed, '!as_iterable', const [])..rustType = slot;
  }
  // ..and out of one, the list it materialises into, shaped from there:
  // the trait carries `iterator` and `to_list`, and every other member a
  // body asks for is written over a list.
  if (have.name == 'Iterable' && slot.name != 'Iterable') {
    final listed = IrCall(value, 'dart_to_list', const [])
      ..rustType = IrType('List', arguments: have.arguments);
    if (slot.name == 'List' && sameRust(listed.rustType!, slot)) return listed;
    return coerceInto(listed, slot, world, inClosure: inClosure);
  }
  // `List` and `Set` are the prelude's own structs; a `List` where a `List`
  // goes is the value itself.
  if (collectionNames.contains(normalName(have.name)) &&
      collectionNames.contains(normalName(slot.name)) &&
      (normalName(have.name) == 'Set') == (normalName(slot.name) == 'Set')) {
    return value;
  }
  // A `Set` where an `Iterable`/`List` goes: its elements, in order
  // (`_entries.insertAll(index, old)` with a `LinkedHashSet`, ws642).
  if (normalName(have.name) == 'Set' &&
      (slot.name == 'List' || slot.name == 'Iterable') &&
      !isNullable(have) &&
      !isNullable(slot)) {
    final listed = IrCall(value, 'to_list', const [])
      ..rustType = IrType('List', arguments: have.arguments);
    return coerceInto(listed, slot, world, inClosure: inClosure);
  }
  final haveTrait = world.isTrait(have.name);
  final slotTrait = world.isTrait(slot.name);
  // Out of `Object`: a scalar by `Any`, cloned out of the reference.
  if (haveObject && scalarNames.contains(slot.name)) {
    return IrCall(IrDowncast(value, rustScalar(slot.name)), 'clone', const [])
      ..rustType = slot;
  }
  if (haveTrait && slotTrait) {
    // The same trait with other arguments: a cast, which the object
    // answers through its wider impl (`Render<BoxC>` returned where the
    // erased trait says `Render<Rc<dyn Constraints>>`, the atbounds
    // fixture). At a type parameter there is no `TypeId` to ask for, and
    // the value goes as it is, as it did before wider impls existed.
    if (have.name == slot.name) {
      final same =
          have.arguments.length == slot.arguments.length &&
          [
            for (var i = 0; i < have.arguments.length; i++)
              sameRust(have.arguments[i], slot.arguments[i]),
          ].every((s) => s);
      // ..and a value the *erased* spelling handed back -- `_Delegate<Rc<dyn
      // Object>>` read off a `dyn InheritedProvider`, whose parameter is
      // erased -- into the instantiation the code here works with: the
      // object answers for its own by id, and a declaration's type
      // parameter is a type with an id, since every one this compiler
      // writes is `'static` (`_InheritedProviderScopeElement._delegate`,
      // 7 stubs at ws934).
      final erasedBack =
          !same &&
          have.arguments.length == slot.arguments.length &&
          have.arguments.isNotEmpty &&
          [
            for (var i = 0; i < have.arguments.length; i++)
              sameRust(have.arguments[i], slot.arguments[i]) ||
                  _isTopType(have.arguments[i]),
          ].every((s) => s);
      if (same || (!_concreteArguments(slot, world) && !erasedBack)) {
        return value;
      }
      return IrCastTo(value, slot)..rustType = slot;
    }
    // A supertrait *without* arguments unsizes (`Rc<dyn Sub>` as `Rc<dyn
    // Base>`); with arguments the wider instantiation is another trait
    // (`RestorableNum<i64>` into `RestorableProperty<Rc<dyn Object>>`,
    // run626), which the object answers for through its wider impl.
    // ..spelled concretely: a slot naming the callee's own type parameter
    // (`ValueListenable<T>` of `ValueListenableBuilder<T>`) has no
    // `TypeId` to ask for, and unsizes as before (+8 at ws629).
    if (world.isBelow(have.name, slot.name) &&
        (slot.arguments.isEmpty || !_concreteArguments(slot, world))) {
      return IrUpcast(value, slot, handle: true, explicit: inClosure)
        ..rustType = slot;
    }
    return IrCastTo(value, slot)..rustType = slot;
  }
  // A trait handle into a *struct* slot (`Leaf? child = super.firstNode`
  // on a mixin's erased `ChildType?`): the object behind the handle, when
  // it is one -- Dart's implicit downcast, failing as one does (the
  // supertear fixture).
  if (haveTrait &&
      world.isStruct(slot.name) &&
      !isNullable(have) &&
      !isNullable(slot) &&
      !slot.isFunction) {
    return IrCall(
      IrDowncast(value, slot.name, arguments: slot.arguments),
      'clone',
      const [],
    )..rustType = slot;
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
  // Out of `Object` into anything else with a conversion of its own -- an
  // enum, a prelude type, a collection -- by that type's `FromDynamic`
  // (every type has one): Dart's cast, failing as one does. Last, so the
  // rules above keep their shapes (a `dynamic`'s arguments into a
  // listener's `AnimationStatus`, ws515).
  // ..only into a type the world can name: a type parameter of another
  // declaration (a super constructor's `T`, `DiagnosticsDebugCreator`,
  // ws516) is nothing to convert into.
  // A type parameter's value into a *prelude value type's* slot. The mirror
  // of the two rules below: Rust cannot spell `T: DateTime` as a bound, so
  // a `T` carries none of `DateTime`'s members, and the object is asked for
  // the bound Dart promised (`CalendarDelegate<T extends DateTime>
  // .isSameDay`, whose `dateA.year` found no `year` on a `T`).
  //
  // The prelude's value types and no others. Reaching a *translated* struct
  // or enum this way cost two stubs at ws1048: `SlottedContainerRenderObject
  // Mixin<SlotType>` holds a `Map<SlotType, ..>` whose key slot is spelled
  // by the one instantiation there is (`_ChipSlot`), and converting the
  // `SlotType` into it handed the map an argument of the wrong type.
  if (world.isTypeParameter(have.name) &&
      have.arguments.isEmpty &&
      !have.isFunction &&
      !isNullable(have) &&
      !isNullable(slot) &&
      !slotObject &&
      !slot.isFunction &&
      !world.isTypeParameter(slot.name) &&
      preludeValueTypes.contains(slot.name)) {
    final boxed = coerceInto(
      value,
      const IrType('Object'),
      world,
      inClosure: inClosure,
    );
    return IrStaticCall(
      null,
      'dart_from_dynamic',
      [boxed],
      typeArguments: [slot],
    )..rustType = slot;
  }
  // A trait handle into a *type parameter's* slot: the mirror of the rule
  // below it. The value goes to `Object` first -- a handle unsizes -- and
  // the type parameter's own `FromDynamic` (in every bound) takes it back.
  // That is Dart's implicit downcast, failing as one does.
  //
  // Reached where a generic class was erased to its bound and read back
  // through the parameter that promised more: `_InheritedModel<T extends
  // Model>` holds `T model` and is emitted with `model: Rc<dyn Model>`, so
  // `ScopedModel.of<T>`'s `return (widget as _InheritedModel<T>).model`
  // handed an `Rc<dyn Model>` to a `T`. Same shape for `LayoutInfoType get
  // layoutInfo => constraints as LayoutInfoType`, whose cast resolved to
  // the one instantiation there is.
  if (haveTrait &&
      world.isTypeParameter(slot.name) &&
      slot.arguments.isEmpty &&
      !slot.isFunction &&
      !isNullable(have) &&
      !isNullable(slot)) {
    final boxed = coerceInto(
      value,
      const IrType('Object'),
      world,
      inClosure: inClosure,
    );
    return IrStaticCall(
      null,
      'dart_from_dynamic',
      [boxed],
      typeArguments: [slot],
    )..rustType = slot;
  }
  // A type parameter's value into a trait slot (`model` of `T extends
  // InheritedModel` asked for `isSupportedAspect`): the object's own cast
  // (`dart_cast_to`, on every `DartAny`), which a handle forwards and a
  // struct answers for its traits. Nullable either side keeps its shape.
  if (world.isTypeParameter(have.name) &&
      have.arguments.isEmpty &&
      !have.isFunction &&
      !isNullable(have) &&
      !isNullable(slot) &&
      !slotObject &&
      world.isTrait(slot.name)) {
    return IrCastTo(value, slot)..rustType = slot;
  }
  if (haveObject &&
      !slotObject &&
      !slot.isFunction &&
      !isNullable(slot) &&
      (world.isStruct(slot.name) ||
          world.isEnum(slot.name) ||
          world.isTypeParameter(slot.name) ||
          preludeValueTypes.contains(slot.name))) {
    return IrStaticCall(
      null,
      'dart_from_dynamic',
      [value],
      typeArguments: [slot],
    )..rustType = slot;
  }
  return value;
}

/// The prelude's value types with a `FromDynamic` of their own (its
/// `dart_nullable!` list, the collections, the typed lists, the unit).
const preludeValueTypes = {
  'List',
  'Iterable',
  'Set',
  'Map',
  'Queue',
  'Int8List',
  'Int16List',
  'Int32List',
  'Int64List',
  'Uint8List',
  'Uint8ClampedList',
  'Uint16List',
  'Uint32List',
  'Uint64List',
  'Float32List',
  'Float64List',
  'ByteData',
  'Duration',
  'DateTime',
  'RegExp',
  'RegExpMatch',
  'StackTrace',
  'Stopwatch',
  'StringBuffer',
  'Symbol',
  'Type',
  'Uri',
  'Random',
  'Timer',
  'Invocation',
  'ArgumentError',
  'RangeError',
  'IndexError',
  'FormatException',
  'Exception',
  'void',
  '()',
};

const _dynamicType = IrType('dynamic');

/// A top type: what an erased parameter is spelled as.
bool _isTopType(IrType t) =>
    (t.name == 'Object' || t.name == 'dynamic') && t.arguments.isEmpty;

/// Whether every name in a type's arguments is a class, scalar or prelude
/// type the world knows -- not a type parameter of some declaration, which
/// a cast's `TypeId` could not be taken for.
bool _concreteArguments(IrType t, TypeWorld world) {
  const known = {
    'Object',
    'dynamic',
    'Null',
    'Type',
    'void',
    '()',
    'Future',
    'FutureOr',
    'Function',
    'Vec',
  };
  bool ok(IrType a) {
    if (a.isFunction) {
      return (a.parameters ?? const []).every(ok) &&
          (a.returns == null || ok(a.returns!));
    }
    final name = a.name;
    final knownName =
        known.contains(name) ||
        scalarNames.contains(name) ||
        preludeValueTypes.contains(name) ||
        world.isTrait(name) ||
        world.isStruct(name) ||
        world.isEnum(name);
    return knownName && a.arguments.every(ok);
  }

  return t.arguments.every(ok);
}

/// Whether a type can be written as a turbofish: no placeholder in it.
bool _spellable(IrType t) {
  if (t.name == '_' || t.name == 'raw' || t.name.isEmpty) return false;
  if (t.isFunction) {
    return (t.parameters ?? const []).every(_spellable) &&
        (t.returns == null || _spellable(t.returns!));
  }
  return t.arguments.every(_spellable);
}

/// A function value as the prelude's `DartFunction` (see
/// `dart_function_object`): bound first (a closure literal shared, with its
/// own bindings made where it stood), then the dynamic entry -- `(args:
/// Vec<Rc<dyn Object>>) -> Rc<dyn Object>` -- moving the handle in.
IrExpr _dynamicFunction(IrExpr value, IrType have, TypeWorld world) {
  final hp = have.parameters!;
  final argsList = IrLocal('__args')
    ..rustType = IrType('List', arguments: const [_dynamicType]);
  final args = <IrExpr>[
    for (var i = 0; i < hp.length; i++)
      coerceInto(
        IrIndex(argsList, IrLiteral('$i', const IrType('int')))
          ..rustType = _dynamicType,
        hp[i],
        world,
        inClosure: true,
      ),
  ];
  final params = [IrParam('__args', argsList.rustType!)];
  final arity = IrLiteral('${hp.length}', const IrType('raw'));
  // The value itself, unboxed: the binding below is declared with the
  // function's type, and a closure or a tear-off boxes into it there. Boxed
  // here as well it was an `Rc<Rc<..>>` (ws847).
  final handle = value is IrClosure && value.boxed
      ? (IrClosure(
          value.params,
          value.body,
          value.returns,
          captures: value.captures,
          locals: value.locals,
          holdsSelf: value.holdsSelf,
          isAsync: value.isAsync,
        )..rustType = value.rustType)
      : value;
  final function = IrLocal('__f')..rustType = have;
  final called = IrCallValue(function, args)..rustType = have.returns;
  final result = coerceInto(called, _dynamicType, world, inClosure: true);
  return IrBlockValue(
    // Declared, so the handle is the `Rc<dyn Fn(..)>` of the function's own
    // type and not the `Rc<{fn item}>` a tear-off would infer: what the
    // object keeps is what `dart_function_same` and `dart_is_function_of`
    // ask it for, and the two have to spell the same type (ws847).
    [IrLocalDecl('__f', have, handle)],
    IrStaticCall(null, 'dart_function_object', [
      arity,
      // A clone: the entry after it moves the binding in.
      IrCall(IrLocal('__f'), 'clone', const []),
      IrCall(
        IrClosure(
          params,
          IrReturn(result),
          _dynamicType,
          locals: const ['__f'],
        ),
        '!rc',
        const [],
      ),
    ]),
  );
}

/// A bare `Function` as a function of `slot`'s type: the very handle it was
/// made from when that is of the type (`dart_function_same`), else a closure
/// of the slot's type calling it dynamically -- each argument as an object
/// into `dart_call_function`, the result coerced into the slot's.
IrExpr _typedFunction(IrExpr value, IrType slot, TypeWorld world) {
  final sp = slot.parameters!;
  final params = <IrParam>[];
  final args = <IrExpr>[];
  for (var i = 0; i < sp.length; i++) {
    final name = '__a$i';
    params.add(IrParam(name, sp[i]));
    args.add(
      coerceInto(
        IrLocal(name)..rustType = sp[i],
        _dynamicType,
        world,
        inClosure: true,
      ),
    );
  }
  final function = IrLocal('__f')..rustType = _dynamicType;
  final call = IrStaticCall(null, 'dart_call_function', [
    function,
    IrListLiteral(args, _dynamicType),
  ], fails: true)..rustType = _dynamicType;
  final result = coerceInto(call, slot.returns!, world, inClosure: true);
  final adapter = IrUpcast(
    IrCall(
      IrClosure(params, IrReturn(result), slot.returns!, locals: const ['__f']),
      '!rc',
      const [],
    ),
    slot,
    handle: true,
    explicit: true,
  );
  return IrBlockValue(
    [IrLocalDecl('__f', null, value)],
    IrIfNull(
      // A clone: the adapter after it moves the binding in.
      IrStaticCall(null, 'dart_function_same', [
        IrCall(IrLocal('__f'), 'clone', const []),
      ])..rustType = IrType(slot.name, nullable: true),
      adapter,
      nullableResult: false,
      eager: false,
    ),
  );
}
