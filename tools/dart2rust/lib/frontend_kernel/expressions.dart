part of '../frontend_kernel.dart';

// The entry point, and the small readers a lowering asks first.
augment class KernelFrontend {
  // -- Expressions ------------------------------------------------------------

  /// Every lowered expression knows its Rust type (`IrExpr.rustType`):
  /// what the lowering said when it knew better, and Dart's static type
  /// mapped through `_type` otherwise. A slot's coercion (`coerce`) reads
  /// this rather than re-deriving the value's shape at each site.
  IrExpr expression(Expression node) {
    var lowered = _expressionRaw(node);
    // A member read the type flow analysis narrowed past its declared
    // null (`widget.builder(..)` under `if (widget.builder != null)`, the
    // `!` rewritten away in an AOT dill): the Rust member is still the
    // `Option` it was declared, and the read unwraps it -- the proof,
    // spelled, as an argument's is (`_widenedInto`). A local's promotion
    // is handled where it is read (`_localRead`); a projected `T?` has no
    // `Option` to unwrap.
    if (node is InstanceGet ||
        node is InstanceInvocation ||
        node is StaticGet ||
        node is StaticInvocation) {
      final declared = _declaredTypeOf(node);
      // The node's *own* recorded type, which is where the analysis
      // writes its narrowing (`getStaticType` recomputes the declared).
      final narrowed = switch (node) {
        InstanceGet(:final resultType) => resultType,
        InstanceInvocation(:final functionType) => functionType.returnType,
        _ => _staticType(node),
      };
      if (declared != null &&
          declared is! TypeParameterType &&
          declared.nullability == Nullability.nullable &&
          narrowed != null &&
          narrowed is! DynamicType &&
          narrowed.nullability != Nullability.nullable &&
          lowered is! IrNullCheck) {
        // ..whatever the read's recorded type says: it follows the
        // narrowing, the Rust member does not.
        final declaredIr = _recordedType(declared);
        if (declaredIr != null && isNullable(declaredIr)) {
          lowered = IrNullCheck(lowered)..rustType = _recordedType(narrowed);
        }
      }
    }
    // Arithmetic is typed by its operands, not by Dart: the `double?` Dart
    // gives an inlined `lerpDouble` is an `f64` here (69 `unwrap` on an
    // `f64` at ws357).
    if (lowered is IrBinary && lowered.rustType == null) {
      lowered.rustType = _binaryType(lowered);
    }
    // A null check is its operand's type without the `Option`: Dart's
    // static type for `data.nextSibling!` is the clone's `RenderBox`, the
    // operand an erased `RenderObject?` (108 at ws380).
    if (lowered is IrNullCheck && lowered.rustType == null) {
      final inner = lowered.operand.rustType;
      if (inner != null) lowered.rustType = _nonNull(inner);
    }
    // A call or read whose declared type is the *declaring class's* type
    // parameter, through a receiver that puts a `dynamic` there: the value
    // is that `dynamic` whatever Kernel's substitution says. Recorded even
    // over a type the lowering already put on, because that type is the
    // lie (`_throughReceiver`).
    final erasedThrough = _erasureOff
        ? null
        : switch (node) {
            InstanceInvocation(:final interfaceTarget, :final receiver) =>
              _throughReceiver(
                receiver,
                interfaceTarget,
                interfaceTarget.function?.returnType,
              ),
            InstanceGet(:final interfaceTarget, :final receiver) =>
              _erasedRead(interfaceTarget, interfaceTarget.getterType) ??
                  _throughReceiver(
                    receiver,
                    interfaceTarget,
                    interfaceTarget.getterType,
                  ),
            _ => null,
          };
    if (erasedThrough != null) lowered.rustType = erasedThrough;
    if (lowered.rustType == null) {
      final static = _staticType(node);
      // A member declared `T?` with `T` bound to a top type reads as the
      // `Option<Rc<dyn Object>>` Rust's `Or` is for a handle, not as the
      // bare `dynamic` Kernel's substitution wrote (`_imageStream?.key`,
      // `raw[id]` on a `Map<String, Object?>`, ws499); `coerce` converts
      // it into a `dynamic` slot from there.
      final projected = switch (node) {
        InstanceInvocation(:final interfaceTarget) =>
          _topBound(interfaceTarget.function?.returnType, static) ??
              _erasedResult(interfaceTarget.function?.returnType),
        InstanceGet(:final interfaceTarget) =>
          _topBound(interfaceTarget.getterType, static) ??
              _erasedResult(interfaceTarget.getterType),
        _ => null,
      };
      // A generic callee's `T?` result, instantiated with a type
      // parameter of the code here, arrives as the callee's edge spells
      // it -- `<T as DartNullable>::Or` -- not as the body's `Option<T>`:
      // typed as the edge, so the coercion into a body slot converts it
      // (`dependOnInheritedWidgetOfExactType<T>()` returned into
      // `inheritFrom<T>`'s `T?`, wrapped in a `from_option` that took an
      // `Option`, ws543).
      // ..and a `super.m<T>()` reaching a generic base method is the
      // same edge (`super.get<T>()` returning `T?` was wrapped in a
      // `from_option` that took an `Option`, the gentrait fixture).
      final declaredReturn = switch (node) {
        InstanceInvocation(:final interfaceTarget) =>
          interfaceTarget.function?.returnType,
        StaticInvocation(:final target) => target.function.returnType,
        SuperMethodInvocation(:final interfaceTarget) =>
          interfaceTarget.function.returnType,
        _ => null,
      };
      final calleeParams = switch (node) {
        InstanceInvocation(:final interfaceTarget) =>
          interfaceTarget.function?.typeParameters ?? const <TypeParameter>[],
        StaticInvocation(:final target) => target.function.typeParameters,
        SuperMethodInvocation(:final interfaceTarget) =>
          interfaceTarget.function.typeParameters,
        _ => const <TypeParameter>[],
      };
      final edgeResult =
          declaredReturn is TypeParameterType &&
          declaredReturn.nullability == Nullability.nullable &&
          calleeParams.contains(declaredReturn.parameter) &&
          !_erasedParameter(declaredReturn.parameter) &&
          static is TypeParameterType &&
          static.nullability == Nullability.nullable &&
          _projectedSlot(static);
      // ..and a callee that hands back its *own* element -- a container's
      // `E`, not an `E?` -- hands back whatever its type argument was
      // spelled with, and a type argument is nested: `_options.elementAt
      // (i)` on an `Iterable<T?>` arrives as `<T as DartNullable>::Or`,
      // the slot `RadioListTile<T?>.value` is, and a `from_option` that
      // took an `Option` stubbed `_SettingsListItemState.build` (ws691).
      // Whose parameter it is does not matter: if the declared result is
      // the bare parameter and the result here is nullable, the argument
      // put in for it was nullable and is spelled projected. An `E?` is
      // *not* the same -- a prelude container spells its own `V?` as a
      // real `Option<V>`, one layer more than the argument.
      final elementResult =
          declaredReturn is TypeParameterType &&
          declaredReturn.nullability != Nullability.nullable &&
          !_erasedParameter(declaredReturn.parameter) &&
          !_spelledAsBound(declaredReturn.parameter) &&
          static is TypeParameterType &&
          static.nullability == Nullability.nullable &&
          _projectedSlot(static);
      // An `async` function returns the future it spawns, whatever it
      // was declared: `Future<flatten(R)>`, so `FutureOr<void> f() async`
      // hands back a `DartFuture<()>`, not a `FutureOr` (`_sendFontChange
      // Message` into `then`, run607). Static calls here; `_qualified`
      // does the same for instance ones.
      final spawned = node is StaticInvocation && _asyncMember(node.target)
          ? _spawnedFuture(static)
          : null;
      if (spawned != null) {
        lowered.rustType = spawned;
      } else if (projected != null) {
        lowered.rustType = projected;
      } else if (static != null) {
        try {
          lowered.rustType = edgeResult || elementResult
              ? _typeNested(static)
              : _type(static);
        } on Unsupported {
          // A type this compiler has no spelling for: the node stays
          // untyped, and a coercion into a slot falls back to the shape
          // rules.
        }
      }
    }
    // The prelude's own `Iterable<T>` is a `Vec<T>`. `where`, `map` and
    // `followedBy` come out of a Rust iterator chain this backend
    // collects; `keys` is the map's own list. Only a *translated* member
    // hands back the handle an `Iterable<T>` slot is here (ws908), so the
    // boundary says so once, rather than each of the places a produced
    // list lands saying it again -- and `sameRust` stops calling the two
    // one type in the same round.
    final produced = lowered.rustType;
    if (produced != null &&
        produced.name == 'Iterable' &&
        produced.arguments.length == 1 &&
        _preludeProduced(node)) {
      lowered.rustType = IrType(
        'List',
        nullable: produced.nullable,
        arguments: produced.arguments,
        module: produced.module,
      );
    }
    return lowered;
  }

  /// Whether `node` reads or calls a member this compiler does not
  /// translate: the prelude's, whose Rust signature is the truth about
  /// what comes back (`_widenedInto` says the same about what goes in).
  bool _preludeProduced(Expression node) {
    final Member? target = switch (node) {
      InstanceInvocation(:final interfaceTarget) => interfaceTarget,
      InstanceGet(:final interfaceTarget) => interfaceTarget,
      SuperMethodInvocation(:final interfaceTarget) => interfaceTarget,
      SuperPropertyGet(:final interfaceTarget) => interfaceTarget,
      StaticInvocation(:final target) => target,
      StaticGet(:final target) => target,
      _ => null,
    };
    if (target == null) return false;
    final owner = target.enclosingClass;
    if (owner != null) return !_translatedClass(owner);
    return !_translatedLibrary(target.enclosingLibrary);
  }

  /// The `Future<T>` an `async` member declared `FutureOr<T>` or
  /// `Future<T>?` actually returns (Dart's `flatten`); null when the
  /// declaration is a plain `Future<T>` already, or cannot be spelled.
  IrType? _spawnedFuture(DartType? declared) {
    final flattened =
        declared is FutureOrType ||
            (declared is InterfaceType &&
                declared.classNode.name == 'Future' &&
                declared.nullability == Nullability.nullable)
        ? _awaitedType(declared)
        : null;
    if (flattened == null) return null;
    try {
      return IrType('Future', arguments: [_type(flattened)]);
    } on Unsupported {
      return null;
    }
  }

  /// `dynamic?`, the `Option<Rc<dyn Object>>` a nullable type parameter is
  /// once bound to a top type (`Object?`, `dynamic`): what the Rust side
  /// holds for it (`<Rc<dyn Object> as DartNullable>::Or`), where the
  /// substituted Dart type says only `Object?` -- a `dynamic` here. Null
  /// for any other declared type or binding.
  /// A member whose declared result is an *erased* type parameter hands
  /// back the bound it was erased to, whatever the static type says: a
  /// `WidgetStateProperty<bool>.resolve(states)` is a `bool` to Dart and
  /// an `Rc<dyn Object>` here, and the coercion into the slot reads it
  /// back (`_MaterialScrollbar._thickness`, ws704).
  /// A condition is a `bool`: whatever the value in hand is spelled as --
  /// the `Rc<dyn Object>` an erased result hands back, say -- it goes in
  /// through the one coercion rule (`_MaterialScrollbar._thickness`,
  /// ws704).
  IrExpr _condition(Expression condition) =>
      coerce(expression(condition), const IrType('bool'));

  /// The result of a member whose declared type is the *declaring class's*
  /// type parameter, read through a receiver that puts a `dynamic` there.
  ///
  /// Kernel's substitution says `T`, because Dart kept the argument; this
  /// output erased it, so what comes back is the `Rc<dyn Object>` the
  /// erased slot holds. Typed as that, the coercion into the slot converts
  /// it -- untyped, `item.tween.transform(t)` on a `TweenSequenceItem`
  /// whose `T` is erased returned an `Rc<dyn Object>` where the function
  /// says `T` (`TweenSequence._evaluateAt`, the gallery's page transition,
  /// run765).
  IrType? _throughReceiver(
    Expression receiver,
    Member target,
    DartType? declared,
  ) {
    if (declared is! TypeParameterType) return null;
    // Strictly non-null: `Map<K, V>.[]` returns `V?`, which Kernel writes
    // `V%` -- *undetermined*, because `V`'s bound is nullable -- and the
    // top-bound rule already spells that `Option<Rc<dyn Object>>`. Let
    // through, the read lost its `Option` and every null-aware read around
    // it stopped compiling: round 3 at 336 against 246 with the rules off
    // (ws765 through ws768).
    if (declared.nullability == Nullability.nullable) return null;
    final owner = target.enclosingClass;
    final env = typeEnvironment;
    if (owner == null || env == null) return null;
    final at = owner.typeParameters.indexOf(declared.parameter);
    if (at < 0) return null;
    // The receiver as *this output* records it, not as Kernel wrote it: a
    // field whose own class erased a parameter reads at the erased
    // spelling, and Kernel's substitution put the caller's `T` there
    // (`element.tween` on a `TweenSequenceItem<T>`, run765).
    IrType? recorded;
    if (receiver is InstanceGet) {
      recorded = _erasedRead(
        receiver.interfaceTarget,
        receiver.interfaceTarget.getterType,
      );
    }
    if (recorded == null) {
      final receiverType = _staticType(receiver);
      if (receiverType is! InterfaceType) return null;
      final asOwner = env.hierarchy.getTypeAsInstanceOf(receiverType, owner);
      if (asOwner is! InterfaceType || at >= asOwner.typeArguments.length) {
        return null;
      }
      try {
        recorded = IrType(
          owner.name,
          arguments: [for (final a in asOwner.typeArguments) _typeNested(a)],
        );
      } on Unsupported {
        return null;
      }
    }
    if (at >= recorded.arguments.length) return null;
    final put = recorded.arguments[at];
    // Only where the erasure really put a `dynamic` there: anything the
    // output can still name is what the value is.
    if (put.name != 'dynamic') return null;
    return const IrType('dynamic');
  }

  /// A read whose declared type *mentions* a type parameter this output
  /// erased on the declaring class: the value is spelled the way the struct
  /// spells it, with the erased parameter at its bound.
  ///
  /// `TweenSequenceItem<T>` loses its `T`, so `item.tween` is an
  /// `Rc<dyn Animatable<Rc<dyn Object>>>` -- and Kernel's substitution says
  /// `Animatable<TweenSequence.T>`, which is what the caller wrote and not
  /// what is there (`TweenSequence._evaluateAt`, run765).
  /// A bisect switch: `DART2RUST_ERASURE_OFF=1` turns the two rules below
  /// off, to say whether a round's cost is theirs.
  static final bool _erasureOff =
      Platform.environment['DART2RUST_ERASURE_OFF'] == '1';

  /// Whether the *shape* of a lowering says the value is out of its
  /// `Option`, whatever type was recorded for it.
  ///
  /// A null check, a downcast and a cast all hand back the value itself;
  /// only a `Some` (and a plain read of a nullable place) is still in the
  /// `Option`. `_widenedInto` asks this where nothing recorded a type --
  /// `a!.dart_cast_any::<Rc<X>>()` reaching `_handleOf` records none, and
  /// treating that as "may already be an Option" left the `Some` off at
  /// every `lerp` (`BoxBorder.lerp`, ws772).
  static bool _unwrapped(IrExpr e) => switch (e) {
    IrNullCheck() => true,
    IrDowncast() => true,
    IrCastTo() => true,
    IrNew() => true,
    IrCall(:final target, :final name, :final args)
        when name == 'clone' && args.isEmpty && target != null =>
      _unwrapped(target),
    _ => false,
  };

  IrType? _erasedRead(Member target, DartType? declared) {
    if (_erasureOff) return null;
    if (declared == null || declared is TypeParameterType) return null;
    final owner = target.enclosingClass;
    if (owner == null) return null;
    // Only where the class kept *none* of them: the struct then has no
    // type parameters at all and every mention is at the bound, which is
    // what makes the read's spelling unambiguous. A class that kept some
    // still names them, and reading at the bound there was 87 more errors
    // in one round (ws765).
    final erased = owner.typeParameters.where(_erasedParameter).toList();
    if (erased.isEmpty ||
        erased.length != owner.typeParameters.length ||
        !_mentionsParametersOf(declared, erased)) {
      return null;
    }
    try {
      return _typeNested(declared);
    } on Unsupported {
      return null;
    }
  }

  IrType? _erasedResult(DartType? declared) {
    if (declared is! TypeParameterType) return null;
    if (!_erasedParameter(declared.parameter)) return null;
    try {
      return _type(declared);
    } on Unsupported {
      return null;
    }
  }

  IrType? _topBound(DartType? declared, DartType? substituted) {
    // Through a `Future`: `invokeMethod<T>` returns `Future<T?>`, and the
    // rule below reads its `T?` as the `Option<Rc<dyn Object>>` a handle's
    // `Or` is. Stopping at the `Future` recorded a bare `Future<dynamic>`,
    // and the erased twin's cast then asked for `DartFuture<Rc<dyn
    // Object>>` where the twin hands back `DartFuture<Option<..>>`
    // (`DefaultProcessTextService.queryTextActions`, the run's own panic
    // at run773).
    if (declared is InterfaceType &&
        declared.classNode.name == 'Future' &&
        declared.typeArguments.length == 1 &&
        substituted is InterfaceType &&
        substituted.classNode.name == 'Future' &&
        substituted.typeArguments.length == 1) {
      final inner = _topBound(
        declared.typeArguments.single,
        substituted.typeArguments.single,
      );
      return inner == null ? null : IrType('Future', arguments: [inner]);
    }
    if (declared is! TypeParameterType ||
        declared.nullability != Nullability.nullable ||
        _erasedParameter(declared.parameter)) {
      return null;
    }
    final top =
        substituted is DynamicType ||
        (substituted is InterfaceType &&
            substituted.classNode.name == 'Object' &&
            substituted.classNode.enclosingLibrary.importUri.toString() ==
                'dart:core' &&
            substituted.nullability == Nullability.nullable);
    return top ? const IrType('dynamic', nullable: true) : null;
  }

  /// Dart's `toString()` of `lowered`, typed `type`, as a `String`: the
  /// `DartAny` protocol for a translated class (`!dart_to_string`: its
  /// own override, an enum's `X.value`, `Instance of` otherwise), `null`
  /// or that for a nullable one, and the object's own answer
  /// (`dart_object_str`: the registry, the core values) for a `dynamic`,
  /// an `Object`, a type parameter, a core value this lowering does not
  /// spell. Under `explicit` -- an `x.toString()` call -- a `String`
  /// or number receiver is left to the ordinary call and null is
  /// returned. `dart_str` (Rust's `Debug`) printed `Some(1)`, `Type {
  /// name: .. }` and `Size { _width: .. }` where Dart says `1`, `Size`
  /// and `Size(800.0, 600.0)` (ws543).
  IrExpr? _stringOf(IrExpr lowered, DartType? type, {bool explicit = false}) {
    const text = IrType('String');
    IrExpr nullOr(IrExpr inner) => IrIfNull(
      IrNullAware(lowered, inner)
        ..rustType = const IrType('String', nullable: true),
      IrLiteral('"null".to_string()', const IrType('raw'))..rustType = text,
      nullableResult: false,
      eager: true,
    )..rustType = text;
    if (type is InterfaceType) {
      final node = type.classNode;
      final core = node.enclosingLibrary.importUri.toString() == 'dart:core';
      final nullable = type.nullability == Nullability.nullable;
      if (_translatedClass(node) && !core) {
        final own = IrCall(IrBound(), '!dart_to_string', const [])
          ..rustType = text;
        if (!nullable) {
          return IrCall(lowered, '!dart_to_string', const [])..rustType = text;
        }
        return nullOr(own);
      }
      // A `List`/`Set`: `[a, b]` / `{a, b}`, each element by this rule
      // (`join`'s element rule in the backend); a nullable one `null` or
      // that.
      if (core && const {'List', 'Set'}.contains(node.name)) {
        final open = node.name == 'List' ? '[' : '{';
        final close = node.name == 'List' ? ']' : '}';
        IrExpr joined(IrExpr list) => IrInterpolation([
          IrLiteral(open, const IrType('String')),
          IrCall(list, '!join', [IrLiteral(', ', const IrType('String'))])
            ..rustType = text,
          IrLiteral(close, const IrType('String')),
        ])..rustType = text;
        if (!nullable) return joined(lowered);
        return nullOr(
          joined(
            IrBound()
              ..rustType = _typeNested(
                type.withDeclaredNullability(Nullability.nonNullable),
              ),
          ),
        );
      }
      if (core &&
          const {'String', 'int', 'double', 'bool'}.contains(node.name)) {
        if (!nullable) return explicit ? null : lowered;
        // An `int?`/`bool?` through the Object protocol, as any other
        // value: the bound may be the erased handle a `Tween<int>.end`
        // is held as, whose `Debug` (`dart_str`) says `Instance of 'int'`
        // (the traitset fixture).
        final inner = switch (node.name) {
          'String' => IrCall(IrBound(), 'clone', const [])..rustType = text,
          'double' => IrStaticCall(null, 'dart_double_str', [
            IrBound(),
          ])..rustType = text,
          _ => IrCall(IrBound(), '!object_str', const [])..rustType = text,
        };
        return nullOr(inner);
      }
    }
    // A value the AOT compiler removed (`!`) is its own text: a call on
    // it would ask `!: DartAny` (11 "never type fallback" at ws545).
    // ..through `dart_str` (a `Debug` bound, which the `()` the block
    // falls back to has; bare, `{}` asked `Display` of it: 11 at ws546).
    if (lowered.rustType?.name == 'Never') {
      return IrStaticCall(null, 'dart_str', [lowered])..rustType = text;
    }
    return IrCall(lowered, '!object_str', const [])..rustType = text;
  }

  /// Dart's `null`, as the lowering writes it on its own -- an omitted
  /// argument, an uninitialised local, a constant -- *typed*, so that the
  /// coercion into its slot sees it: untyped, an omitted `Object? aspect`
  /// stayed `None` where the `Null` object went (85 at ws502).
  static IrExpr _nullLiteral() =>
      IrLiteral('null', const IrType('Null', nullable: true))
        ..rustType = const IrType('Null', nullable: true);

  /// `x!`, and every unwrap the lowering adds on Dart's word that a value
  /// is nullable: the value itself when its recorded Rust type is not an
  /// `Option` -- a `dynamic`, an `Object?` (one type here, see `_type`),
  /// arithmetic the type flow analysis typed non-nullable and Kernel still
  /// writes `double?` for (`lerpDouble`, ws331) -- and the unwrap
  /// otherwise. An operand with no recorded type is unwrapped as Dart says.
  IrExpr _nullChecked(IrExpr inner, [Expression? operand]) {
    // Unwrapped once: the read may have been already (`expression`).
    if (inner is IrNullCheck) return inner;
    // By the operand's *declared* type first: an AOT dill's type flow
    // analysis narrows `widget.builder` to non-null under `if (widget.
    // builder != null)`, and the recorded type follows it, while the Rust
    // field is the `Option` it was declared (`WidgetsApp.build`, ws506).
    final declared = operand == null ? null : _declaredTypeOf(operand);
    final declaredIr = declared == null ? null : _recordedType(declared);
    // Typed as the operand without its `Option`: a downcast of the checked
    // value asks a handle for its `Any` only when it knows it holds one
    // (`old.width` after `Painter? old` promoted to `Caret`, ws591).
    IrNullCheck checked() {
      final have = inner.rustType;
      return IrNullCheck(inner)
        ..rustType = have == null ? null : _nonNull(have);
    }

    if (declaredIr != null) {
      return isNullable(declaredIr) ? checked() : inner;
    }
    final have = inner.rustType;
    if (have != null && !isNullable(have)) return inner;
    return checked();
  }

  /// The type a member or variable was *declared* with, for the read
  /// `e` is of one: what the Rust side holds, whatever the flow analysis
  /// narrowed the read to. Null for any other expression.
  DartType? _declaredTypeOf(Expression e) {
    if (e is InstanceGet) return e.interfaceTarget.getterType;
    if (e is VariableGet) return e.variable.type;
    if (e is InstanceInvocation) return e.interfaceTarget.function?.returnType;
    if (e is StaticGet) return e.target.getterType;
    if (e is StaticInvocation) return e.target.function.returnType;
    return null;
  }

  /// A declared type as recorded on a value: `null` where this compiler
  /// has no spelling for it.
  IrType? _recordedType(DartType? declared) {
    if (declared == null) return null;
    try {
      return _type(declared);
    } on Unsupported {
      return null;
    }
  }

  static IrType? _binaryType(IrBinary b) {
    const comparisons = {'==', '!=', '<', '>', '<=', '>=', '&&', '||'};
    if (comparisons.contains(b.op)) return const IrType('bool');
    // Operands the lowering built itself (the inlined `lerpDouble`'s
    // `(b - a) * t`) never passed through `expression`: typed here first.
    final left = b.left;
    if (left is IrBinary && left.rustType == null) {
      left.rustType = _binaryType(left);
    }
    final right = b.right;
    if (right is IrBinary && right.rustType == null) {
      right.rustType = _binaryType(right);
    }
    final l = b.left.rustType?.name;
    final r = b.right.rustType?.name;
    if (l == null || r == null) return null;
    const numbers = {'int', 'double', 'num'};
    if (!numbers.contains(l) || !numbers.contains(r)) return null;
    if (b.op == '~/') return const IrType('int');
    if (b.op == '/' || l != 'int' || r != 'int') return const IrType('double');
    return const IrType('int');
  }
}
