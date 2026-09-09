part of '../frontend_kernel.dart';

augment class KernelFrontend {
  // -- Types ------------------------------------------------------------------

  /// How deep inside a type `_type` is: a `T?` *inside* a type -- a type
  /// argument, a function type's parameter or result -- is spelled
  /// projected (`<T as DartNullable>::Or`), the way a signature's is, so
  /// that `WidgetStateProperty<T?>` with `T` bound to `Color?` is one
  /// trait object type on both sides. A bare `T?` at the top is the
  /// `Option<T>` a body works with; `_edgeType` projects that one in
  /// signatures.
  var _typeDepth = 0;

  T _nested<T>(T Function() inside) {
    _typeDepth++;
    try {
      return inside();
    } finally {
      _typeDepth--;
    }
  }

  /// `_type`, as a type argument: a `T?` here is projected.
  IrType _typeNested(DartType type) => _nested(() => _type(type));

  IrType _type(DartType type) {
    final nullable = type.nullability == Nullability.nullable;
    // An applied mixin body's concrete argument back to the mixin's own
    // erased parameter (`_appliedBack`), which spells as its bound.
    if (_appliedBack.isNotEmpty && type is InterfaceType) {
      final back =
          _appliedBack[type.withDeclaredNullability(Nullability.nonNullable)];
      if (back != null) {
        // Once: the parameter spells as its bound, and a bound that
        // mentions a mapped type would send this straight back here
        // (a stack overflow in the front end, ws741).
        final was = _appliedBack;
        _appliedBack = const {};
        try {
          return _type(
            nullable
                ? back.withDeclaredNullability(Nullability.nullable)
                : back,
          );
        } finally {
          _appliedBack = was;
        }
      }
    }
    // An extension type is its representation type at runtime -- Dart
    // erases it -- and so it is here (`BaselineOffset(double? offset)`:
    // `RenderBoxContainerDefaultsMixin.defaultComputeDistanceToHighest
    // ActualBaseline` was refused whole, and `RenderFlex`'s baseline with
    // it, ws525).
    if (type is ExtensionType) {
      final erased = type.extensionTypeErasure;
      return _type(
        nullable
            ? erased.withDeclaredNullability(Nullability.nullable)
            : erased,
      );
    }
    if (type is InterfaceType) {
      // `dart:core`'s `Iterator` would shadow `std::iter::Iterator` in every
      // module: it is the prelude's `DartIterator`.
      final core =
          type.classNode.enclosingLibrary.importUri.toString() == 'dart:core';
      // `Object?` is `dynamic`: Dart's two top types are one type to its
      // subtyping (`LocalizationsDelegate<dynamic>` and
      // `LocalizationsDelegate<Object?>` are the same type, `WidgetsApp.
      // build`, ws497), and one representation here -- a `dynamic` holds
      // its null as the `Null` object. Everywhere, not only as a type
      // argument: an expression's Rust type follows its Dart static type,
      // and `m[k]` on a `Map<Object?, Object?>` is typed `Object?` by the
      // substitution Kernel already did.
      if (core && type.classNode.name == 'Object' && nullable) {
        return const IrType('dynamic');
      }
      final name = core && type.classNode.name == 'Iterator'
          ? 'DartIterator'
          : type.classNode.name;
      _census(type);
      // A class that *is* a `Future` (implements `dart:async`'s): the
      // prelude's future, since that is what every `Future<T>` slot holds
      // (`SynchronousFuture<T>`, ws482).
      if (_futureLike(type.classNode) && type.typeArguments.length == 1) {
        return IrType(
          'Future',
          nullable: nullable,
          arguments: _nested(() => [_type(type.typeArguments.single)]),
        );
      }
      return IrType(
        name,
        nullable: nullable,
        arguments: _erasedArguments(type.classNode, type.typeArguments),
        module: _moduleQualifier(type.classNode),
      );
    }
    if (type is RecordType) {
      // A named field is a tuple field too, after the positional ones and
      // in the record type's own order -- Kernel asserts that order is
      // lexicographic, which is Dart's canonical order for named fields, so
      // two spellings of one record type give one tuple
      // (`({OverlayEntry start, OverlayEntry end})? _handles` of
      // `SelectionOverlay`, 6 stubs and 5 refusals at ws751).
      return IrType(
        'Record',
        nullable: nullable,
        arguments: _nested(
          () => [
            for (final f in type.positional) _type(f),
            for (final n in type.named) _type(n.type),
          ],
        ),
      );
    }
    if (type is VoidType) return const IrType('void');
    // The bottom type. Thirty in the gallery's dill: `noSuchMethod`s declared
    // `Never`, and a few `Foo<Never>`. The backend spells it two ways.
    if (type is NeverType) return const IrType('Never');
    if (type is DynamicType) return const IrType('dynamic');
    if (type is NullType) return const IrType('Null', nullable: true);
    if (type is TypeParameterType) {
      // A *local function's* own parameter is always its bound: a Rust
      // closure cannot be generic, so the declaration is written at the
      // bound and the call site speaks those terms
      // (`LocalFunctionInvocation`, ws879). As a `FunctionType`'s own
      // parameter is, and by the same route, so the two spell one type:
      // `T?` at the bound `Object?` is the *non-nullable* `dynamic` this
      // compiler holds a null in, not an `Option` of it.
      if (_erasedLocalParams.contains(type.parameter)) {
        final bound = type.parameter.bound;
        return _type(
          nullable
              ? bound.withDeclaredNullability(Nullability.nullable)
              : bound,
        );
      }
      // An erased parameter is its bound (see `_erasedParameter`).
      if (_erasedParameter(type.parameter)) {
        final asBound = _typeOfBound(type.parameter.bound);
        return IrType(
          asBound.name,
          nullable: nullable || asBound.nullable,
          arguments: asBound.arguments,
        );
      }
      // `T extends String` is a `String` here: the bound is what the body
      // calls methods on, and a Rust type parameter has no such methods.
      // Only for the scalar bounds that are prelude types; `T extends
      // Comparable<T>` would recurse.
      // Not `num`: `_RestorablePrimitiveValue<T extends num>` as an `f64`
      // took `RestorableInt`'s `i64`s in and gave `f64`s out (51 at
      // ws354); `T` stays the caller's type.
      final bound = type.parameter.bound;
      if (bound is InterfaceType &&
          const {
            'String',
            'int',
            'double',
            'bool',
          }.contains(bound.classNode.name)) {
        // The bound's own nullability comes along: intl's `T extends
        // String?` was a `String` here, and its `String?` return a
        // `String` (40 `Option<String>` <- `String`).
        return IrType(
          _typeOfBound(bound).name,
          nullable: nullable || bound.nullability == Nullability.nullable,
        );
      }
      // `T extends Iterable<E>`: the `Vec<E>` the bound is, since the body
      // iterates it (collection's `IterableEquality`, 6).
      if (bound is InterfaceType &&
          const {'Iterable', 'List'}.contains(bound.classNode.name)) {
        final asBound = _typeOfBound(bound);
        return IrType(
          asBound.name,
          nullable: nullable,
          arguments: asBound.arguments,
        );
      }
      return IrType(
        type.parameter.name ?? 'T',
        nullable: nullable,
        // ..of the declaration being lowered only: another declaration's
        // `T?` -- a callee's, reached before its instantiation is put in --
        // is not a name here, projected or not (24 `cannot find type`).
        projected: nullable && _typeDepth > 0 && _projectedSlot(type),
      );
    }
    // A generic function *type*'s own parameter (`E Function<E>(E)`),
    // spelled at its bound: a `dyn Fn` has no type parameters of its own,
    // and a value of the type is used at the bound (`HeapPriorityQueue`'s
    // comparator in `SchedulerBinding`, which refused the whole
    // `WidgetsFlutterBinding` constructor, run432).
    if (type is StructuralParameterType) {
      final bound = type.parameter.bound;
      return _type(
        nullable ? bound.withDeclaredNullability(Nullability.nullable) : bound,
      );
    }
    if (type is FunctionType && type.typeParameters.isNotEmpty) {
      return _type(
        FunctionTypeInstantiator.instantiate(type, [
          for (final p in type.typeParameters) p.bound,
        ]),
      );
    }
    if (type is FunctionType) {
      // Named parameters after the positional ones, **sorted by name**, as
      // a closure declares them: `LogWriterCallback = void Function(String
      // text, {bool isError})` is an `Fn(String, bool)`, and a field of
      // that type could not hold the two-parameter function (E0593).
      final named = [...type.namedParameters]
        ..sort((a, b) => a.name.compareTo(b.name));
      return _nested(
        () => IrType.function(
          [
            for (final p in type.positionalParameters) _paramType(p),
            for (final p in named) _paramType(p.type),
          ],
          _type(type.returnType),
          nullable: nullable,
        ),
      );
    }
    // `FutureOr<T>` is "a `T`, or a future of one": the prelude's enum of
    // the two, awaitable either way; a value crosses into it through
    // `FutureOr::value` / `FutureOr::future` (`coerceInto`). The gallery's
    // startup path needs it: `Future<bool>(() async {..})` in
    // `GetStorage._internal`, `SchedulerBinding.scheduleTask`.
    // Nullable only as *declared* (`FutureOr<int>?`): Dart computes
    // `FutureOr<void>` and `FutureOr<T?>` nullable from the argument,
    // whose null the `T` inside already carries here -- as an `Option`
    // around the whole, a void `then<void>` callback returned `None` and
    // the adapter into the `FutureOr<()>` slot unwrapped it (`Route.
    // didAdd` through `TickerFuture.then`, run652).
    if (type is FutureOrType) {
      return IrType(
        'FutureOr',
        nullable: type.declaredNullability == Nullability.nullable,
        arguments: [_type(type.typeArgument)],
      );
    }
    throw Unsupported('the type `$type`', '$type');
  }
}
