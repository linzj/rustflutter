part of '../frontend_kernel.dart';

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
    return lowered;
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

  IrExpr _expressionRaw(Expression node) {
    if (node is IntLiteral) {
      return IrLiteral('${node.value}', const IrType('int'));
    }
    if (node is DoubleLiteral) {
      return IrLiteral('${node.value}', const IrType('double'));
    }
    if (node is BoolLiteral) {
      return IrLiteral('${node.value}', const IrType('bool'));
    }
    if (node is StringLiteral) {
      return IrLiteral(node.value, const IrType('String'));
    }
    if (node is NullLiteral) {
      return _nullLiteral();
    }
    if (node is ThisExpression) return IrThis();
    if (node is VariableGet) {
      // `cosmeticName` is Kernel's word for the name a human wrote; a variable
      // the CFE invented has none, and one whose name starts with `#` is a
      // temporary from its own lowering.
      final bound = _bound;
      if (bound != null && node.variable == bound) {
        // The receiver's element as the body's static type names it: a
        // `child?.getDryLayout(..)` on a `child` the trait types
        // `RenderObject?` and the body `RenderBox?` (a mixin's `T extends
        // RenderBox`) reaches `RenderBox` through the object (ws476).
        final wanted = node.promotedType ?? node.variable.type;
        final have = _boundType;
        if (wanted is InterfaceType &&
            have != null &&
            have.name != wanted.classNode.name &&
            _abstractLike(wanted.classNode) &&
            !_scalarClass(wanted.classNode) &&
            _translatedClass(wanted.classNode) &&
            isBelow(wanted.classNode.name, have.name)) {
          // ..typed as the cast's own value, not Kernel's `RenderBox?`:
          // the cascade's `=>#t3` is the bound, never doubled, and the
          // untyped cast took the static type and a `.flatten()` with it
          // (`RenderProxyBoxMixin.performLayout` again, run657).
          final narrowed = _type(
            wanted.withDeclaredNullability(Nullability.nonNullable),
          );
          return IrCastTo(IrBound(), narrowed)..rustType = narrowed;
        }
        // Typed as the value the body binds: the receiver without its
        // `Option`. A `?..` cascade produces the bound (`=>#t3`, which
        // the CFE leaves unpromoted), and typed by Kernel's `RenderBox?`
        // the block got a `.flatten()` on a value that was never doubled
        // (`RenderProxyBoxMixin.performLayout`, `getTransformTo`, ws537).
        return IrBound()..rustType = have;
      }
      if (_cascade != null && node.variable == _cascade) {
        return _cascadeRead();
      }
      // A `let` temporary standing for a place its body mutates (see
      // `_let`): the place itself.
      final aliased = _letAliases[node.variable];
      if (aliased != null) return expression(aliased);
      // A temporary this lowering has already named. Asking the map rather
      // than the variable's own name is what makes two nested `#0`s two
      // different locals instead of one.
      final temporary = _temporaries[node.variable];
      final written = node.variable.cosmeticName;
      if (temporary == null && (written == null || written.startsWith('#'))) {
        throw Unsupported('synthetic variable', _sample(node));
      }
      // A temporary is promoted like any local: `if (__t != null)
      // xs.add(__t)` after the CFE's lowering of `?.`/`??` (`String <=
      // Option<String>`). It used to return before the checks below.
      final name = temporary ?? written!;
      if (_optionLocals.contains(node.variable)) {
        // ..a `late` local too: assigned on some path, read as the value
        // (`late double primaryDeltaFromDragStart` set in a `switch`,
        // `RawScrollbarState._getPrimaryDelta`, run666).
        if ((_tryWrites.contains(node.variable) || node.variable.isLate) &&
            node.variable.type.nullability != Nullability.nullable) {
          return IrCall(
            IrCall(IrLocal(name), 'clone', const []),
            'unwrap',
            const [],
          )..rustType = _type(node.variable.type);
        }
        return IrLocal(name)..rustType = _localIrType(node.variable);
      }
      // A constructor's projected parameter is read as it was declared,
      // the spelled `T?`: a constructor has no body prologue to re-bind
      // it, and its field initialisers store it as it is.
      final declaring = node.variable.parent;
      if (declaring is FunctionNode &&
          declaring.parent is Constructor &&
          _projectedSlot(node.variable.type)) {
        return IrLocal(name)..rustType = _edgeType(node.variable.type);
      }
      // A closure parameter retyped to an erased bound (`_closureParamType`)
      // reads as the class it was declared with.
      final declaredAs = _declaredParamTypes[node.variable];
      final retyped = _retyped[node.variable];
      final declaredVar = node.variable.type;
      if (retyped is TypeParameterType &&
          _erasedParameter(retyped.parameter) &&
          declaredVar is InterfaceType &&
          declaredVar.nullability != Nullability.nullable) {
        final bound = retyped.parameter.bound;
        if (bound is InterfaceType &&
            bound.classNode != declaredVar.classNode) {
          if (_abstractLike(declaredVar.classNode) &&
              !_scalarClass(declaredVar.classNode)) {
            return IrCastTo(IrLocal(name), _type(declaredVar));
          }
          return _narrowingCast(IrLocal(name), declaredVar);
        }
      }
      // Promoted to a concrete class the declaration does not name: the
      // read is a downcast. Promotion to the *same* class (nullable to
      // non-null) is not.
      var promoted = node.promotedType;
      // ..a copy's parameter as its declaration types it (`ChildType?`,
      // the bound here) rather than as the copy does (`RenderBox?`).
      final declared = declaredAs ?? node.variable.type;
      // Promoted to a type parameter (`if (item is T) return item;` in
      // `InheritedModel.inheritFrom<T>`, run541): a kept one is the
      // parameter's own conversion (`FromDynamic`, as `as T` is), an
      // erased one is its bound and takes the rules below.
      if (promoted is TypeParameterType) {
        final parameter = promoted.parameter;
        if (_erasedParameter(parameter)) {
          promoted = parameter.bound;
        } else if (!(declared is TypeParameterType &&
            declared.parameter == parameter)) {
          // Out of its `Option` first when the local is nullable (`final w
          // = map[T]; w is T ? w : null`, the typelit fixture).
          final read = declared.nullability == Nullability.nullable
              ? _nullChecked(IrLocal(name)..rustType = _recordedType(declared))
              : (IrLocal(name)..rustType = _recordedType(declared));
          return IrDowncast(read, parameter.name ?? 'T')
            ..rustType = _type(promoted);
        }
      }
      // The core scalars are abstract classes in Kernel and structs here:
      // an `Object?` promoted to `String` is a downcast to `String`.
      const scalars = {'String', 'int', 'double', 'bool', 'num'};
      // To `Object` (a pattern's `final Object? msg` on a `dynamic`
      // temporary) is to the root every value already is: no cast.
      final toObject =
          promoted is InterfaceType &&
          promoted.classNode.name == 'Object' &&
          promoted.classNode.enclosingLibrary.importUri.toString() ==
              'dart:core';
      // ..typed as what the local holds (`Rc<dyn Object>` for a
      // `dynamic`), not as the promotion says (`Object?`): the slot's
      // rule puts the `Some` on (`if` and `else` have incompatible
      // types, run453).
      // Only from a `dynamic`: an `Object?` promoted to `Object` is the
      // unwrap below (`key == keyOrNull` compared an `Rc` with an
      // `Option`, ws454).
      if (toObject && declared is DynamicType) {
        try {
          return IrLocal(name)..rustType = _type(declared);
        } on Unsupported {
          return IrLocal(name);
        }
      }
      // Promoted to a *function* type (`if (userMainFunction is
      // _ListStringArgFunction)` in `dart:ui`'s `_runMain`): what the
      // declaration holds is still the function *object*, and Rust will not
      // call an `Rc<dyn Object>`. The coercion rule builds the closure of
      // the promoted type that calls it dynamically -- sound because the
      // `is` that promoted it is exact (`dart_is_function_of` downcasts the
      // handle the object was made from, ws847).
      if (promoted is FunctionType) {
        final read = IrLocal(name)..rustType = _recordedType(declared);
        try {
          return coerce(read, _type(promoted));
        } on Unsupported {
          return read;
        }
      }
      if (promoted is InterfaceType &&
          !toObject &&
          (!_abstractLike(promoted.classNode) ||
              scalars.contains(promoted.classNode.name)) &&
          // An enum too: `switch (dependency)` after `is _MediaQueryAspect`
          // matched enum arms against an `Rc<dyn Object>` (29 at ws311).
          !(declared is InterfaceType &&
              declared.classNode == promoted.classNode)) {
        // With the struct's type arguments: `other is AsyncSnapshot<T>`
        // reads `other` as an `AsyncSnapshot<T>` (16 E0107 at ws425).
        final to = _type(promoted);
        // Out of its `Option` first when the local is nullable: a
        // `Painter? old` promoted to `Caret` asked the `Option` for its
        // `Any` (`shouldRepaint`'s `old.width`, ws590).
        // ..and typed as declared either way: untyped, the backend asked
        // the *handle's* `Any` (`resolvable.as_any()` on an `Rc<dyn
        // Color>`, never the `CupertinoDynamicColor` inside; `resolve`
        // unwrapped a `None` where `is` had just said yes, run621).
        final read = declared.nullability == Nullability.nullable
            ? _nullChecked(IrLocal(name)..rustType = _recordedType(declared))
            : (IrLocal(name)..rustType = _recordedType(declared));
        final downcast = IrDowncast(
          read,
          rustScalar(to.name),
          arguments: to.arguments,
        );
        // ..and a promotion that is itself nullable -- `if (parent is
        // _NestedHookElement?)`, which matches null as well -- keeps the
        // absence: the downcast is null-aware, and what comes back is the
        // `Option` the slot takes. Null-checked, the read unwrapped the
        // `None` the test had just admitted
        // (`SingleChildWidgetElementMixin.mount`, 4 at ws793).
        if (promoted.nullability == Nullability.nullable &&
            declared.nullability == Nullability.nullable) {
          // The bound value is typed as what the local holds, so the
          // downcast asks the *object*'s `Any` and not the handle's
          // (`it.as_any()` on an `&Rc<dyn Base>` found no `Hook`).
          final held = _recordedType(declared);
          final bound = IrBound()
            ..rustType = held == null ? null : nonNull(held);
          final inner = IrCall(
            IrDowncast(bound, rustScalar(to.name), arguments: to.arguments),
            'clone',
            const [],
          )..rustType = nonNull(to);
          return IrNullAware(
            IrLocal(name)..rustType = _recordedType(declared),
            inner,
          )..rustType = to;
        }
        // Cloned out of the reference `Any` hands back, and typed as what
        // the promotion says it is: untyped, a slot that takes an `Option`
        // could not tell a promoted value from one still in its `Option`
        // and left the `Some` off (`ShapeDecoration.lerpFrom`, 6 at ws764).
        return IrCall(downcast, 'clone', const [])..rustType = to;
      }
      // ..and to one of `dart:core`'s collections (`val is Map` on a
      // `dynamic`): the prelude's value, converted where its element
      // representation differs (`dart_cast_map`, as an `as Map<..>` is;
      // get's `_isNullOrEmpty`, run489).
      // ..and a typed list (`value is Float64List` in
      // `StandardMessageCodec.writeValue`, run505): the `Vec` of its
      // element the prelude names it.
      if (promoted is InterfaceType &&
          (_coreCollection(promoted.classNode) ||
              _typedList(promoted.classNode) ||
              (promoted.classNode.name == 'Iterable' &&
                  promoted.classNode.enclosingLibrary.importUri.toString() ==
                      'dart:core')) &&
          promoted.nullability != Nullability.nullable &&
          (declared is DynamicType ||
              (declared is InterfaceType &&
                  declared.classNode.name == 'Object'))) {
        final to = _type(promoted);
        // An `Iterable` promotion reads the value as the list it is here.
        final asName = promoted.classNode.name == 'Iterable' ? 'List' : to.name;
        return IrCall(
          IrDowncast(
            // A `dynamic` is a handle, never an `Option`; an `Object?` is.
            _nullChecked(IrLocal(name)..rustType = _recordedType(declared)),
            rustScalar(asName),
            arguments: to.arguments,
          ),
          'clone',
          const [],
        )..rustType = IrType(asName, arguments: to.arguments);
      }
      // ..and to an abstract or open class: the trait cast every object
      // answers (`dart_cast_to`). Not from a nullable declaration, whose
      // `Option` the null-promotion below takes off first.
      // ..a *translated* one: `dart:core`'s `List` is abstract to Kernel
      // and a `Vec` here, the same `Vec` an `Iterable<T>` is (`newEntries
      // is List<OverlayEntry> ? newEntries : ..`, ws638).
      if (promoted is InterfaceType &&
          _abstractLike(promoted.classNode) &&
          _translatedClass(promoted.classNode) &&
          !scalars.contains(promoted.classNode.name) &&
          promoted.nullability != Nullability.nullable &&
          !(declared is InterfaceType &&
              declared.classNode == promoted.classNode) &&
          !(declared is InterfaceType &&
              declared.nullability == Nullability.nullable)) {
        // Typed as the promotion says, for the same reason the downcast
        // above is: a slot that takes an `Option` cannot otherwise tell a
        // promoted value from one still in its `Option`, and left the
        // `Some` off (`BoxBorder.lerp`'s `BorderDirectional::lerp(a, b)`,
        // ws770).
        return IrCastTo(IrLocal(name), _type(promoted))
          ..rustType = _type(promoted);
      }
      // Promoted from `T?` to `T` -- `if (x != null) f(x)` -- the read is
      // the value inside. A clone first, so the local is still there for
      // the next read: `&Option<Hct>` where `&Hct` was wanted, 12 times.
      // A type parameter's own nullability is *undetermined*, not
      // non-nullable: `value` after `if (value is! T) throw` in
      // `Provider.of` was returned still an `Option<T>`.
      if (promoted != null &&
          declared is! DynamicType &&
          promoted.nullability != Nullability.nullable &&
          declared.nullability == Nullability.nullable) {
        final inside = _nullChecked(
          IrCall(IrLocal(name), 'clone', const [])
            ..rustType = _recordedType(declared),
        );
        // ..and narrowed as well as unwrapped: `ancestor` after `ancestor
        // is StatefulElement`, on an `Element?`, read `.state` of an
        // `Rc<dyn Element>` (`findAncestorStateOfType`).
        if (promoted is InterfaceType &&
            _abstractLike(promoted.classNode) &&
            _translatedClass(promoted.classNode) &&
            !scalars.contains(promoted.classNode.name) &&
            declared is InterfaceType &&
            declared.classNode != promoted.classNode) {
          return IrCastTo(inside, _type(promoted));
        }
        return inside;
      }
      if (declaredAs != null) {
        return IrLocal(name)..rustType = _type(declaredAs);
      }
      // ..and as its slot retyped it (see `_staticType`).
      if (retyped != null) {
        final ir = _recordedType(retyped);
        if (ir != null) return IrLocal(name)..rustType = ir;
      }
      // A promotion no rule above reads through (`v is! S` on a `T`, both
      // parameters) still reads the local as declared: untyped, the slot
      // could not box it (`Entry<S>(v)` into an erased `Rc<dyn Object>`,
      // the outparam fixture).
      if (promoted != null) {
        return IrLocal(name)..rustType = _recordedType(declared);
      }
      // ..and any other read as the local is declared: untyped, two
      // handles in two locals compared by their slots' addresses
      // (`identical(b, loc)`, the dyncast fixture).
      return IrLocal(name)..rustType = _localIrType(node.variable);
    }
    if (node is InstanceGet) return _instanceGet(node);
    if (node is StaticGet) return _staticGet(node);
    if (node is InstanceInvocation) return _instanceInvocation(node);
    // A deferred import is linked like any other here: `loadLibrary()`
    // is a future already done with null, and the CFE's check before a
    // use of the library is null itself (the generated localizations'
    // `lookupGalleryLocalizations`, run579).
    if (node is LoadLibrary) {
      // `Future<dynamic>.value()`: the `null` of `dynamic` (the slot is
      // the projected `Option<Rc<dyn Object>>`, ws580).
      return IrStaticCall(
        'Future',
        'value',
        const [],
        typeArguments: const [IrType('dynamic')],
      )..rustType = const IrType('Future', arguments: [IrType('dynamic')]);
    }
    if (node is CheckLibraryIsLoaded) {
      return IrStaticCall(null, 'dart_null_object', const [])
        ..rustType = const IrType('dynamic');
    }
    if (node is BlockExpression) return _blockValue(node);
    if (node is FunctionInvocation) {
      // A call through a function value: no callee to read defaults from,
      // only its type -- which is enough to *order* named arguments, and the
      // closures on the other end are declared in that same order.
      final type = node.functionType;
      if (type == null) {
        // A call on a bare `Function` (or a `dynamic`): Dart's dynamic
        // call, which the prelude's function object answers with its
        // arguments as objects and its result as one (`dart_call_function`).
        if (node.arguments.named.isNotEmpty ||
            node.arguments.types.isNotEmpty) {
          throw Unsupported(
            'dynamic call with named or type arguments',
            _sample(node),
          );
        }
        return IrStaticCall(null, 'dart_call_function', [
          coerce(expression(node.receiver), const IrType('dynamic')),
          IrListLiteral([
            for (final a in node.arguments.positional)
              coerce(expression(a), const IrType('dynamic')),
          ], const IrType('dynamic')),
        ], fails: true)..rustType = const IrType('dynamic');
      }
      // ..unless the value is a tear-off the compiler resolved to a
      // constant (`GoogleFonts.libreFranklin(..)`, whose getter TFA folded
      // into `partL::libreFranklin`): that is the static call it names, and
      // its arguments go in the function's own order, not the type's sorted
      // one, which put a `FontWeight` in the `locale` slot (34 at ws321).
      var receiver = node.receiver;
      while (receiver is FileUriExpression) {
        receiver = receiver.expression;
      }
      final constantTarget =
          receiver is ConstantExpression &&
              receiver.constant is StaticTearOffConstant
          ? (receiver.constant as StaticTearOffConstant).target
          : receiver is StaticTearOff
          ? receiver.target
          : null;
      if (constantTarget != null &&
          constantTarget.function.typeParameters.isEmpty) {
        return IrStaticCall(
          constantTarget.enclosingClass?.name,
          constantTarget.enclosingClass == null
              ? _topLevelName(constantTarget.name.text)
              : constantTarget.name.text,
          _arguments(node.arguments, constantTarget.function),
          fails: _fails(constantTarget),
          diverges: _diverges(constantTarget),
        );
      }
      return IrCallValue(
        expression(node.receiver),
        _argumentsByType(node.arguments, type),
      )..rustType = _type(type).returns;
    }
    if (node is LocalFunctionInvocation) {
      // The name the declaration gave it (see `FunctionDeclaration`): a
      // CFE temporary's is kept by identity, not by the text.
      final written = node.variable.cosmeticName;
      final name = (written == null || written.startsWith('#'))
          ? _nameFor(node.variable)
          : written;
      // A local function is declared as a closure, so its named parameters
      // are in type order there too.
      return IrCallValue(
        IrLocal(name),
        _argumentsByType(node.arguments, node.functionType),
      )..rustType = _type(node.functionType).returns;
    }
    if (node is FunctionExpression) return _closure(node.function, node);
    if (node is StaticSet) {
      // `Owner.x = v` where the value is wanted -- the `??=` on a static, in
      // the CFE's `let #t = X in #t == null ? X = v : #t`. Bind the value,
      // store a clone, produce the binding: the store moves, and the value
      // still has to come out.
      final target = node.target;
      if (target is Procedure &&
          target.kind == ProcedureKind.Setter &&
          target.enclosingClass == null) {
        final held = '__t${_nextTemporary++}';
        final init = expression(node.value);
        final stored = _widened(
          node.value,
          target.function.positionalParameters.single.type,
          IrCall(IrLocal(held), 'clone', const [])..rustType = init.rustType,
        );
        return IrBlockValue([
          IrLocalDecl(held, null, init),
          IrExprStmt(
            IrStaticCall(null, _topLevelSetterName(target.name.text), [stored]),
          ),
        ], IrLocal(held));
      }
      // ..and a class's static setter: its static function (`UndoManager
      // .client = this` in `UndoHistoryState.initState`, run678).
      if (target is Procedure &&
          target.kind == ProcedureKind.Setter &&
          target.enclosingClass != null) {
        final held = '__t${_nextTemporary++}';
        final init = expression(node.value);
        final stored = _widened(
          node.value,
          target.function.positionalParameters.single.type,
          IrCall(IrLocal(held), 'clone', const [])..rustType = init.rustType,
        );
        return IrBlockValue([
          IrLocalDecl(held, null, init),
          IrExprStmt(
            IrStaticCall(
              target.enclosingClass!.name,
              'set_${target.name.text}',
              [stored],
            ),
          ),
        ], IrLocal(held));
      }
      if (target is! Field) {
        throw Unsupported('static setter used for its value', _sample(node));
      }
      final owner = target.enclosingClass;
      final held = '__t${_nextTemporary++}';
      final init = expression(node.value);
      // The store widens into the static's type: `_decomposeV ??=
      // Vector3.zero()` on a `Vector3?` stores `Some(..)`.
      final write = _widened(
        node.value,
        target.type,
        IrCall(IrLocal(held), 'clone', const [])..rustType = init.rustType,
      );
      return IrBlockValue([
        IrLocalDecl(held, null, init),
        owner == null
            ? IrAssignTopLevel(target.name.text, write)
            : IrAssignStatic(owner.name, target.name.text, write),
      ], IrLocal(held));
    }
    if (node is Let) return _let(node);
    if (node is EqualsNull) return IrIsNull(expression(node.expression));
    if (node is EqualsCall) {
      // `_argb == other._argb` with one side promoted: an `Option<i64>`
      // against an `i64` does not compare; the non-null side is `Some`d.
      var left = expression(node.left);
      var right = expression(node.right);
      final leftType = _staticType(node.left);
      final rightType = _staticType(node.right);
      bool nullable(DartType? t) =>
          t != null &&
          t is! DynamicType &&
          t.nullability == Nullability.nullable;
      bool plain(DartType? t) =>
          t != null &&
          t is! DynamicType &&
          t is! NullType &&
          t.nullability != Nullability.nullable;
      if (nullable(leftType) && plain(rightType) && !_isNull(node.right)) {
        right = _widened(node.right, leftType, right);
      } else if (plain(leftType) &&
          nullable(rightType) &&
          !_isNull(node.left)) {
        left = _widened(node.left, rightType, left);
      }
      // Two closures are equal when they are the same closure: `Rc<dyn Fn>`
      // has no `==`, and the prelude's `dart_eq` is identity (8 in `listen`).
      if (leftType is FunctionType || rightType is FunctionType) {
        return IrCall(left, '!dart_eq', [right]);
      }
      // `integer == 0` with `integer` a `dynamic` holding a number: the
      // `dynamic` side is the `f64` its arithmetic made it (see the
      // `numOperators` lowering), and the literal side is cast to match.
      bool number(DartType? t) =>
          t is InterfaceType &&
          (t.classNode.name == 'int' ||
              t.classNode.name == 'double' ||
              t.classNode.name == 'num');
      String? numClass(DartType? t) =>
          t is InterfaceType ? t.classNode.name : null;
      if (leftType is DynamicType && number(rightType)) {
        final asDouble = (IrCall(IrDowncast(left, 'f64'), 'clone', const [])
          ..rustType = const IrType('double'));
        final other = numClass(rightType) == 'int' ? _toF64(right) : right;
        return IrBinary('==', asDouble, other);
      }
      if (rightType is DynamicType && number(leftType)) {
        final asDouble = (IrCall(IrDowncast(right, 'f64'), 'clone', const [])
          ..rustType = const IrType('double'));
        final other = numClass(leftType) == 'int' ? _toF64(left) : left;
        return IrBinary('==', other, asDouble);
      }
      // `lightOption == -1` on a `double`: the `int` side is cast, as the
      // arithmetic operators cast theirs.
      String? cls(DartType? t) => t is InterfaceType ? t.classNode.name : null;
      if (cls(leftType) == 'double' && cls(rightType) == 'int') {
        right = _toF64(right);
      } else if (cls(leftType) == 'int' && cls(rightType) == 'double') {
        left = _toF64(left);
      } else if (_declaredNum(node.left) &&
          (node.right is IntLiteral || cls(rightType) == 'int')) {
        right = _toF64(right);
      } else if (_declaredNum(node.right) &&
          (node.left is IntLiteral || cls(leftType) == 'int')) {
        left = _toF64(left);
      }
      // Operands of one Rust type: the right to the left's, else the left
      // to the right's (`data.previousSibling == after`, an erased
      // `RenderObject?` read against a `RenderBox?`, 47 at ws379).
      // ..and of one nullability: a `dynamic?` -- a `T?` read bound to a
      // top type, `raw[id]` -- against a `dynamic` (ws501); the bare side
      // goes into the `Option` (Dart's `null` there is `None`).
      final lt = left.rustType;
      final rt = right.rustType;
      if (coerceByType &&
          lt != null &&
          rt != null &&
          _normalName(lt.name) == _normalName(rt.name) &&
          isNullable(lt) != isNullable(rt)) {
        if (isNullable(lt)) {
          right = coerce(right, lt);
        } else {
          left = coerce(left, rt);
        }
      } else if (coerceByType &&
          lt != null &&
          rt != null &&
          _normalName(lt.name) != _normalName(rt.name)) {
        // The side *below* goes up into the other's type; the other way
        // is a downcast that fails on a value of the wider class
        // (`next?.route != entry.lastAnnouncedNextRoute`: a `Route?`
        // against the `_RoutePlaceholder?` it extends, run636). Two
        // unrelated traits are left as they are: the backend compares
        // them as objects.
        final hierarchy = typeEnvironment?.hierarchy;
        final lc = leftType is InterfaceType ? leftType.classNode : null;
        final rc = rightType is InterfaceType ? rightType.classNode : null;
        final related = lc != null && rc != null && lc != rc;
        final leftBelow =
            related && (hierarchy?.isSubInterfaceOf(lc, rc) ?? false);
        final rightBelow =
            related && (hierarchy?.isSubInterfaceOf(rc, lc) ?? false);
        if (leftBelow && !rightBelow) {
          left = coerce(left, rt);
        } else if (rightBelow && !leftBelow) {
          right = coerce(right, lt);
        } else if (related &&
            !leftBelow &&
            !rightBelow &&
            _abstractLike(lc) &&
            _abstractLike(rc)) {
          // Unrelated: as they are.
        } else {
          final r = coerce(right, lt);
          if (!identical(r, right)) {
            right = r;
          } else {
            left = coerce(left, rt);
          }
        }
      }
      return IrBinary('==', left, right);
    }
    // A truly dynamic call -- `number.abs()` on a `dynamic` in intl's
    // NumberFormat -- when the name is one of `num`'s: the receiver is
    // downcast to the `f64` a `num` is here (see the devirtualised case).
    const numMethods = _dynamicNumMethods;
    if (node is DynamicInvocation && numMethods.contains(node.name.text)) {
      final asDouble = (IrCall(
        IrDowncast(expression(node.receiver), 'f64'),
        'clone',
        const [],
      )..rustType = const IrType('double'));
      final call = IrCall(asDouble, node.name.text, [
        for (final a in node.arguments.positional) expression(a),
      ]);
      return const {
            'round',
            'floor',
            'ceil',
            'truncate',
            'toInt',
          }.contains(node.name.text)
          ? IrCast(call, 'i64')
          : call;
    }
    // ..and its operators: `number - integerPart` on a `dynamic`.
    const numOperators = {'+', '-', '*', '/', '%', '<', '>', '<=', '>=', '~/'};
    if (node is DynamicInvocation || node is DynamicGet) {
      final dispatched = _dynamicSlotCall(node);
      if (dispatched != null) return dispatched;
    }
    if (node is DynamicInvocation &&
        numOperators.contains(node.name.text) &&
        node.arguments.positional.length == 1) {
      final asDouble = (IrCall(
        IrDowncast(expression(node.receiver), 'f64'),
        'clone',
        const [],
      )..rustType = const IrType('double'));
      var right = expression(node.arguments.positional.single);
      final rightType = _staticType(node.arguments.positional.single);
      if (rightType is DynamicType) {
        right = (IrCall(IrDowncast(right, 'f64'), 'clone', const [])
          ..rustType = const IrType('double'));
      } else if (rightType is InterfaceType &&
          rightType.classNode.name == 'int') {
        right = _toF64(right);
      }
      return IrBinary(node.name.text, asDouble, right);
    }
    if (node is DynamicGet && numMethods.contains(node.name.text)) {
      final asDouble = (IrCall(
        IrDowncast(expression(node.receiver), 'f64'),
        'clone',
        const [],
      )..rustType = const IrType('double'));
      return IrCall(asDouble, node.name.text, const []);
    }
    if (node is Not) {
      // `x is! T` is a `Not` around an `IsExpression` here; the analyzer keeps
      // it as one node with a flag. Folded so the two front ends write the
      // same Rust -- `is_none()`, not `!(..is_some())` -- which is the whole
      // point of having two of them.
      final inner = node.operand;
      if (inner is IsExpression) {
        final test = _isExpression(inner);
        if (test is IrIs) {
          return IrIs(test.expr, test.type, negated: true);
        }
        return IrUnary('!', test);
      }
      return IrUnary('!', _condition(node.operand));
    }
    if (node is LogicalExpression) {
      // Both operands are conditions (`_condition`): `a && prop.resolve(s)`
      // on an erased `resolve` had an `Rc<dyn Object>` where `&&` wants a
      // `bool` (`_MaterialScrollbar._thickness`, ws705).
      return IrBinary(
        node.operatorEnum == LogicalExpressionOperator.AND ? '&&' : '||',
        _condition(node.left),
        _condition(node.right),
      );
    }
    if (node is ConditionalExpression) {
      // A condition the AOT compiler replaced by its "removed" throw: the
      // whole conditional is dead, and its branches' types no longer meet.
      final condition = node.condition;
      if (condition is Throw && _tfaUnreachable(condition)) return _unreachable;
      // A null test on a *literal* null: type flow analysis folds a value
      // it proved always null into the literal, and the test around it is
      // then a question with one answer. Both arms were lowered anyway,
      // and the dead one had nothing to infer its types from -- `None
      // .as_ref().map(|it| ..)` on a `None` with no element type (8 "type
      // annotations needed" at ws813). The branch that runs is the whole
      // conditional.
      if (condition is EqualsNull && _isNull(condition.expression)) {
        final taken = node.then;
        return _widened(taken, node.staticType, expression(taken));
      }
      if (condition is Not) {
        final inner = condition.operand;
        if (inner is EqualsNull && _isNull(inner.expression)) {
          final taken = node.otherwise;
          return _widened(taken, node.staticType, expression(taken));
        }
      }
      // `x != null ? Color(..) : "unspecified"` inside a string: the branches
      // are of different classes and the result is `Object`, so both go
      // through `dart_str` (see the `??` case).
      // ..only inside a string: elsewhere each branch widens into
      // `Object` below. Stringifying everywhere turned `slots != null ?
      // slots[i] : IndexedSlot(..)` -- an `Object?` returned from
      // `slotFor` -- into a `String` (`updateChildren`, ws475).
      final staticType = node.staticType;
      final thenType = _staticType(node.then);
      final elseType = _staticType(node.otherwise);
      if (_inStringPart &&
          staticType is InterfaceType &&
          staticType.classNode.name == 'Object' &&
          thenType is InterfaceType &&
          elseType is InterfaceType &&
          thenType.classNode != elseType.classNode) {
        return IrConditional(
          expression(condition),
          IrStaticCall(null, 'dart_str', [expression(node.then)]),
          IrStaticCall(null, 'dart_str', [expression(node.otherwise)]),
        );
      }
      // Each branch widens into the conditional's own type: `m == null ?
      // null : hashAll(m)` is an `Option`, and the second branch an `i64`
      // until it is wrapped (4 `if` and `else` have incompatible types).
      return IrConditional(
        _condition(condition),
        _widened(node.then, staticType, expression(node.then)),
        _widened(node.otherwise, staticType, expression(node.otherwise)),
      );
    }
    if (node is TypeLiteral) return _typeLiteral(node.type);
    if (node is IsExpression) return _isExpression(node);
    if (node is ConstructorInvocation) return _construct(node);
    if (node is StaticInvocation) return _staticInvocation(node);
    if (node is SuperMethodInvocation) {
      // The target member is already resolved -- this is the fact the analyzer
      // front end had to work out for itself.
      final ownerClass = _realOwner(node.interfaceTarget, node.name.text);
      final owner = ownerClass?.name;
      if (owner == null) {
        throw Unsupported('super call with no owner', '$node');
      }
      // The super target is resolved, so its parameter list orders the named
      // arguments -- 56 super calls with named arguments were refused for
      // want of a callee this line had all along.
      return IrSuperCall(
        owner,
        node.name.text,
        // Into the declaration's slots: a super call reaches the mixin's
        // super function, typed by the mixin with this class's arguments
        // put in (`super.insert(child, after: after)` in
        // `RenderSliverMultiBoxAdaptor`, ws479; `didPop(result)`'s `T?`
        // as the class's projected `T?`, ws492).
        _arguments(
          node.arguments,
          node.interfaceTarget.function,
          true,
          null,
          _superSlots(node.interfaceTarget).$1,
          _superSlots(node.interfaceTarget).$2,
        ),
        baseArguments: _superBaseArguments(ownerClass!),
        typeArguments: _typeArgumentsOf(node.arguments),
      );
    }
    if (node is VariableSet) {
      // A value the AOT compiler removed: the assignment never happens and
      // the temporary it would bind has no type (`let __t74 =
      // unreachable!(..)`, 115 "type annotations needed").
      if (node.value is Throw && _tfaUnreachable(node.value as Throw)) {
        return _unreachable;
      }
      // `x = v` used for its value. Rust's assignment produces `()`, so the
      // value is bound, assigned and produced -- the same shape a field write
      // used for its value takes.
      final written = node.variable.cosmeticName;
      final known = _temporaries[node.variable];
      if (known == null && (written == null || written.startsWith('#'))) {
        throw Unsupported('assignment used for its value', _sample(node));
      }
      final name = known ?? written!;
      // Into a `dynamic` local (`dynamic result = scaled(x)` in vector_math's
      // `operator *`) the value is shared into its `Rc<dyn Object>`.
      final raw = expression(node.value);
      final stored = _widened(
        node.value,
        _localType(node.variable),
        raw,
        slotIr: _localIrType(node.variable),
      );
      // `(index = s.indexOf(p)) >= 0` with `int? index`: the store is
      // `Some(..)`, the value of the expression is not -- nor, for a
      // `dynamic` temporary assigned a `String` (a pattern's `#0#2 =
      // error.message`, run452), is it the boxed `Rc<String>`. Whatever
      // the store adapted, the value is the one before: held, stored
      // through the same adaptation of a clone, produced.
      // A literal is not held: the store re-lowers it against the slot's
      // element types, and holding it too would evaluate it twice.
      if (!identical(stored, raw) &&
          node.value is! MapLiteral &&
          node.value is! ListLiteral) {
        final held = '__t${_nextTemporary++}';
        final again = IrCall(IrLocal(held), 'clone', const [])
          ..rustType = raw.rustType;
        // The held value typed by Dart's static type when its own says
        // nothing Rust can infer from: a pattern cache's block whose read
        // TFA removed ends in `unreachable!()`, and `let __t = { ..;
        // unreachable!() }` has no type (`CupertinoDynamicColor.
        // resolveFrom`, run622).
        final rawType = raw.rustType;
        IrType? heldType;
        if (rawType == null || rawType.name == 'Never') {
          // The local's own type, without its `Option`: the value the
          // store wraps (`Some(__t.clone())`). Dart's static type of the
          // block is `Never` once TFA has been through it.
          final slot = _localIrType(node.variable);
          heldType = slot == null ? null : _nonNull(slot);
        }
        return IrBlockValue([
          IrLocalDecl(held, heldType, raw),
          IrAssign(
            name,
            _widened(
              node.value,
              _localType(node.variable),
              again,
              slotIr: _localIrType(node.variable),
            ),
          ),
        ], IrLocal(held))..rustType = raw.rustType;
      }
      return IrAssignValue(name, stored);
    }
    if (node is RecordIndexGet) {
      // `r.$1` in Dart is `r.0` in Rust -- Dart counts its positional record
      // fields from one and Rust counts tuple fields from zero.
      var held = expression(node.receiver);
      // Through the unwrap where the record is an `Option`: Dart lets a
      // field be read only once the record is promoted non-null, and the
      // `is` above it is that promotion (`_AscentDescent` is a
      // `(double, double)?`, `RenderFlex._computeSizes`, ws715).
      final receiverType = held.rustType;
      if (receiverType != null && isNullable(receiverType)) {
        held = _nullChecked(held);
      }
      final read = IrRecordField(held, node.index);
      // Typed as the *record* holds it, not as a promotion reads it: a
      // record pattern promotes a `(double, double)` to `(Object?,
      // Object?)` and reads `$1` at `Object?`, where the Rust tuple still
      // holds an `f64` -- and the coercion into the slot, seeing an
      // `Rc<dyn Object>` already, boxed nothing
      // (`RenderFlex._computeSizes`, run713).
      final record = held.rustType;
      if (record != null && node.index < record.arguments.length) {
        read.rustType = record.arguments[node.index];
      }
      return read;
    }
    if (node is RecordNameGet) {
      // A named field is the tuple field after the positional ones, at its
      // place in the type's (sorted) named list -- the same read as
      // `RecordIndexGet` above, by a different spelling of the index.
      var held = expression(node.receiver);
      final receiverType = held.rustType;
      if (receiverType != null && isNullable(receiverType)) {
        held = _nullChecked(held);
      }
      final where = node.receiverType.named.indexWhere(
        (n) => n.name == node.name,
      );
      final index = node.receiverType.positional.length + where;
      final read = IrRecordField(held, index);
      final record = held.rustType;
      if (where >= 0 && record != null && index < record.arguments.length) {
        read.rustType = record.arguments[index];
      }
      return read;
    }
    if (node is RecordLiteral) {
      return _recordLiteral(
        node,
        node.recordType.positional,
        node.recordType.named,
      );
    }
    if (node is MapLiteral) {
      return _mapLiteral(node, node.keyType, node.valueType);
    }
    if (node is ListLiteral) {
      return _listLiteral(node, node.typeArgument);
    }
    if (node is StringConcatenation) {
      // A part that is neither text nor a number goes through `dart_str`
      // (the prelude's `Debug` rendering); the primitives print as they are.
      IrExpr part(Expression e) {
        final outerPart = _inStringPart;
        _inStringPart = true;
        final IrExpr lowered;
        try {
          lowered = expression(e);
        } finally {
          _inStringPart = outerPart;
        }
        if (e is StringLiteral || lowered is IrLiteral) return lowered;
        final type = _staticType(e);
        final name = type is InterfaceType ? type.classNode.name : null;
        const plain = {'String', 'int', 'double', 'num', 'bool', 'Null'};
        if (name != null &&
            plain.contains(name) &&
            type!.nullability != Nullability.nullable) {
          // A double as Dart spells it (`3.0`, not Rust's `3`): the
          // prelude's `dart_double_str`.
          if (name == 'double') {
            return IrStaticCall(null, 'dart_double_str', [lowered])
              ..rustType = const IrType('String');
          }
          return lowered;
        }
        return _stringOf(lowered, type)!;
      }

      return IrInterpolation([for (final e in node.expressions) part(e)]);
    }
    if (node is SuperPropertyGet) {
      final owner = node.interfaceTarget?.enclosingClass?.name;
      if (owner == null) {
        throw Unsupported('super property with no owner', _sample(node));
      }
      if (node.interfaceTarget is Field) {
        // A base field is copied into the subclass struct by the flattening,
        // so `super.x` and `this.x` are the same storage.
        // ..with the field's own class named: in a trait body a read on
        // `this` is the accessor, and this class may override it with a
        // getter (`Color get primaryColor => super.primaryColor ?? ..`
        // over `NoDefaultCupertinoThemeData`'s field, run618) -- the
        // backend then asks the base trait's accessor for the storage.
        return IrField(
          null,
          _memberName(node.interfaceTarget!),
          onEnum: node.interfaceTarget?.enclosingClass?.isEnum ?? false,
          owner: owner,
        );
      }
      // The class the read lands in (`_realOwner`), as a method call's
      // is: `super.popDisposition` in `ModalRoute` names the anonymous
      // application of `LocalHistoryRoute`, whose hollow declaration TFA
      // emptied and whose body the application holds (run648).
      final target = node.interfaceTarget;
      final ownerClass = target == null
          ? null
          : _realOwner(target, node.name.text);
      final base = ownerClass?.name ?? owner;
      // `super.paint` as a value (`context.pushLayer(layer, super.paint,
      // offset)`): the closure the tear-off is, calling the super
      // function -- as an instance tear-off is (ws649, 15 stubs the
      // moment the owner resolved).
      if (target is Procedure && target.kind == ProcedureKind.Method) {
        return _superTearOff(node, target);
      }
      // Typed by the getter, in this class's kept terms: an erased
      // `ChildType?` is the `RenderObject?` the super function hands
      // back, which the slot narrows (`_RenderTheater._firstOnstageChild`
      // reading `super.firstChild` into a `RenderBox?`, ws649).
      // ..the *mixin's* declaration where it still has one: the target
      // may be the application's copy, already at `RenderBox?`, while the
      // super function is written in the mixin's erased terms.
      final typed = target == null ? null : _superReturn(target);
      if (Platform.environment['DART2RUST_TRACE_SUPER'] == node.name.text) {
        stderr.writeln(
          'TRACE_SUPER get ${node.name.text} owner=${ownerClass?.name} '
          'typed=$typed',
        );
      }
      return IrSuperCall(
        base,
        node.name.text,
        const [],
        baseArguments: ownerClass == null
            ? const []
            : _superBaseArguments(ownerClass),
      )..rustType = typed;
    }
    if (node is SuperPropertySet) {
      // `super.value = value` in `_RestorablePrimitiveValue.value=`: the
      // base's setter, through its super function, the value kept as any
      // assignment's is (9 refusals at ws354). A base *field* is the same
      // storage as this class's (flattened): a plain write.
      final target = node.interfaceTarget;
      final ownerClass = target == null
          ? null
          : _realOwner(target, node.name.text);
      final owner = ownerClass?.name;
      if (target == null || owner == null) {
        throw Unsupported('super property set with no owner', _sample(node));
      }
      final slot = target is Field
          ? target.setterType
          : target is Procedure
          ? target.function.positionalParameters.single.type
          : null;
      final held = '__t${_nextTemporary++}';
      final init = expression(node.value);
      final stored = _widened(
        node.value,
        slot,
        IrCall(IrLocal(held), 'clone', const [])..rustType = init.rustType,
      );
      return IrBlockValue([
        IrLocalDecl(held, null, init),
        target is Field
            ? IrAssignField(_fieldNameOf(target, node.name.text), stored)
            : IrExprStmt(
                IrSuperCall(
                  owner,
                  node.name.text,
                  [stored],
                  isSetter: true,
                  baseArguments: _superBaseArguments(ownerClass!),
                ),
              ),
      ], IrLocal(held));
    }
    if (node is AwaitExpression) {
      // `await <throw>`: the tree shaker replaces a removed call with a
      // throw, and there is nothing to await in a throw -- `.await` on a
      // `return Err(..)` is what came out.
      if (node.operand is Throw) return expression(node.operand);
      // `await v` where `v` is *not* a future: Dart waits an event turn and
      // completes with the value. `await null` -- the idiom for letting the
      // microtask queue run -- is the whole of it here, and `None.await`
      // was what came out (`ImageProvider.resolve`, run724). The already
      // done future of the value, awaited: one turn, then the value.
      final awaited = _staticType(node.operand);
      if (awaited != null && !_couldBeFuture(awaited)) {
        IrType? spelled;
        try {
          spelled = _type(awaited);
        } on Unsupported {
          spelled = null;
        }
        return IrAwait(
          IrStaticCall(null, 'future_ready', [
              expression(node.operand),
            ], typeArguments: spelled == null ? const [] : [spelled])
            ..rustType = IrType(
              'Future',
              arguments: [if (spelled != null) spelled],
            ),
        );
      }
      // Typed as the operand's future says: `await channel.invokeMethod<T>()`
      // hands back the `Option<Rc<dyn Object>>` the erased twin's future
      // holds, and typed by Dart's static type alone it was a bare
      // `dynamic` -- `dart_nullable` was then put around a value already in
      // its `Option` (`DefaultProcessTextService.processTextAction`, the
      // run's own panic, ws776).
      final operand = expression(node.operand);
      final future = operand.rustType;
      final held =
          future != null &&
              future.name == 'Future' &&
              future.arguments.length == 1
          ? future.arguments.single
          : null;
      return IrAwait(operand)..rustType = held;
    }
    if (node is Throw) {
      if (_tfaUnreachable(node)) return _unreachable;
      // `a ?? throw StateError(..)`. Rust has no throw, but it does have an
      // expression that never produces a value: `return Err(e)` has type `!`,
      // which fits wherever a value was wanted. So the expression form is the
      // statement form, written where the value would have gone.
      // A *value* of a translated class thrown from a local or a call
      // (`throw error` with a `FlutterError` in hand) goes behind an
      // `Rc<dyn Object>` here; a constructed one the backend boxes itself
      // (`_boxedThrow`), a handle or a trait object unsizes on its own (18).
      return IrThrowValue(_thrownValue(node.expression));
    }
    if (node is InstanceSet) {
      // `a.b = v` where the value is wanted. Only a field on `this`: a setter
      // returns nothing to produce, and another object's field is the `&mut`
      // through a reference this compiler still refuses as a statement.
      if (node.receiver is! ThisExpression) {
        // `entry.x = v` where the value is wanted, on a local or parameter:
        // the same two shapes the statement form takes -- a local owning a
        // value, or a handle to a counted class whose fields are cells --
        // bound first, written as a clone, produced last. 66 of these.
        final receiver = node.receiver;
        final target = node.interfaceTarget;
        final declaring = target.enclosingClass;
        final onLocal = receiver is VariableGet;
        // A local, a parameter, or a chain rooted at `this` -- the receivers
        // the statement form already takes.
        // A value the AOT compiler removed: the store never happens and
        // the expression has no type to bind (`let __t = unreachable!(..)`,
        // 56 "type annotations needed").
        if (node.value is Throw && _tfaUnreachable(node.value as Throw)) {
          return _unreachable;
        }
        if ((onLocal || _rootedAtThis(receiver)) && declaring != null) {
          final receiverClass = _staticClass(receiver);
          final counted =
              _closureCallsMethod(declaring) ||
              (receiverClass != null && _closureCallsMethod(receiverClass));
          final ownsValue =
              !onLocal || receiver.variable.parent is! FunctionNode;
          if (counted || ownsValue) {
            final held = '__t${_nextTemporary++}';
            final init = expression(node.value);
            final clone = IrCall(IrLocal(held), 'clone', const [])
              ..rustType = init.rustType;
            // Into a nullable field the store is `Some(..)`.
            final stored = _widened(
              node.value,
              _writeSlot(node.interfaceTarget, receiver),
              clone,
              slotIr: _writeSlotIr(node.interfaceTarget, receiver),
            );
            return IrBlockValue([
              // Inferred: the field's *declared* type is the generic `T?` of
              // `Tween<T>`, and spelling it put a `T` into a class with none.
              IrLocalDecl(held, null, init),
              // A field goes through storage (a cell when the class is
              // counted); a setter is a call, on whatever the receiver is.
              target is Field
                  ? IrAssignField(
                      _fieldNameOf(target, node.name.text),
                      stored,
                      target: expression(receiver),
                      owner: counted
                          ? (_receiverClassName(receiver) ?? declaring.name)
                          : null,
                    )
                  : IrSetter(
                      expression(receiver),
                      node.name.text,
                      stored,
                      qualifier: _setterQualifier(receiver, target),
                      receiverClass: _classNameOf(receiver),
                    ),
            ], IrLocal(held));
          }
        }
        throw Unsupported(
          'assignment to another object used for its value '
          '(${_shape(node.receiver)})',
          _sample(node),
        );
      }
      if (node.interfaceTarget is! Field &&
          !_heldField(node.interfaceTarget, node.receiver)) {
        // `_firstChild = _lastChild = child` in a mixin's body: the mixin's
        // field is a setter on its trait. Called, and the value kept -- as
        // the field on another object is above. Refused before ws348, which
        // left `ContainerRenderObjectMixin._insertIntoChildList` out of
        // every applier (27 `todo!`s).
        final held = '__t${_nextTemporary++}';
        final init = expression(node.value);
        final stored = _widened(
          node.value,
          _writeSlot(node.interfaceTarget, node.receiver),
          IrCall(IrLocal(held), 'clone', const [])..rustType = init.rustType,
          slotIr: _writeSlotIr(node.interfaceTarget, node.receiver),
        );
        return IrBlockValue([
          IrLocalDecl(held, null, init),
          IrSetter(
            null,
            _fieldNameOf(node.interfaceTarget, node.name.text),
            stored,
            qualifier: _setterQualifier(null, node.interfaceTarget),
          ),
        ], IrLocal(held));
      }
      final stored = _widened(
        node.value,
        _writeSlot(node.interfaceTarget, node.receiver),
        expression(node.value),
        slotIr: _writeSlotIr(node.interfaceTarget, node.receiver),
      );
      if (stored is IrSome) {
        // `_cache = s` into a `String?` field, used for its value: the
        // store is `Some(s)`, the value is `s`.
        // ..and the value is the *stored* one, in the slot's type: a
        // `Semantics` behind the field's `Rc<dyn Widget>` already, not a
        // value to share again (`_modalScopeCache ??= Semantics(..)`
        // was `dart_object(dart_object(..))`, ws512).
        final held = '__t${_nextTemporary++}';
        final storedType = stored.value.rustType;
        return IrBlockValue([
          IrLocalDecl(held, null, stored.value),
          IrAssignField(
            _fieldNameOf(node.interfaceTarget, node.name.text),
            IrSome(
              IrCall(IrLocal(held), 'clone', const [])..rustType = storedType,
            ),
          ),
        ], IrLocal(held)..rustType = storedType)..rustType = storedType;
      }
      return IrSetValue(null, node.name.text, stored);
    }
    if (node is NullCheck) {
      return _nullChecked(expression(node.operand), node.operand);
    }
    if (node is AsExpression) {
      // `null as T`: the null of `T` -- `None` for a nullable `T`, a panic
      // (Dart's `TypeError`) for one with no null. Spelled through the
      // prelude, which asks `T` itself (`_queue[i] ?? (null as E)` in
      // `HeapPriorityQueue`, run436).
      // ..whether written as the literal or as the CFE's `let Null #t =
      // null in #t as E`: the operand's static type is `Null`.
      if (node.operand is NullLiteral ||
          _staticType(node.operand) is NullType) {
        return IrStaticCall(
          null,
          'dart_null_as',
          const [],
          typeArguments: [_type(node.type)],
        )..rustType = _type(node.type);
      }
      // A cast that only removes `?` -- the CFE's spelling of a promoted
      // private field, `_hct` after `if (_hct != null)` -- is a null check.
      // Any other cast is the operand: Rust's types are already the
      // concrete ones. 12 `&Option<Hct>` where `&Hct` was wanted.
      final from = _backHere(_staticType(node.operand));
      // ..and the target through `_appliedBack`: a mixin body borrowed
      // from an application casts to the application's `FlexParentData`
      // where the trait holds the erased bound (ws739).
      final to = _backHere(node.type)!;
      // ..of a function type too: TFA's `unsafeCast<Fn>(widget.builder)`
      // under `if (widget.builder != null)` is the `!` it rewrote away
      // (`WidgetsApp.build`, ws507).
      final removesNullOnly =
          from != null &&
          from.nullability == Nullability.nullable &&
          to.nullability == Nullability.nonNullable &&
          ((from is InterfaceType &&
                  to is InterfaceType &&
                  from.classNode == to.classNode) ||
              (from is FunctionType &&
                  to is FunctionType &&
                  from.withDeclaredNullability(Nullability.nonNullable) == to));
      if (removesNullOnly) {
        // `unsafeCast<double>(..)`, the tree shaker's form of `..!`: the
        // value when the operand is not an `Option` here (its recorded
        // type says), the unwrap otherwise.
        final inner = expression(node.operand);
        final have = inner.rustType;
        if (have != null && !isNullable(have)) return inner;
        return IrNullCheck(inner);
      }
      // A cast down from an abstract class to a concrete one -- `path as
      // _NativePath` in front of every native taking one -- is a downcast
      // through `Any`, and the value is cloned out of the reference it
      // yields. 4 `_NativePath <= Rc<dyn Path>`.
      // `math.pow(10, v) as int`: a `num` (an `f64` here) to an `int`.
      if (to is InterfaceType &&
          to.classNode.name == 'int' &&
          from is InterfaceType &&
          (from.classNode.name == 'num' || from.classNode.name == 'double')) {
        return IrCast(expression(node.operand), 'i64');
      }
      // `_queue[index] ?? (null as E)`: Dart's way of saying the branch is
      // never taken for a non-nullable `E`. Rust's `E` has no null at all.
      if (node.operand is NullLiteral && to is TypeParameterType) {
        return _unreachable;
      }
      // `key as K` with `key` an `Object?`: a downcast to a type parameter,
      // which `Any` can do because every parameter is bounded `'static`.
      if (to is TypeParameterType &&
          (from is DynamicType ||
              (from is InterfaceType && from.classNode.name == 'Object'))) {
        // A `dynamic` is an `Rc<dyn Object>`, never an `Option`: no unwrap
        // (`codec.decodeEnvelope(result) as T?`, ws461).
        // By the operand's *recorded* type: an `Option` (a `Map<K,
        // Object?>` lookup) is unwrapped for a non-null `T` and kept for
        // a `T?`; a value the type flow analysis narrowed to a scalar
        // (`begin as T` on an `f64`) goes behind the object first, as any
        // value into a `dynamic` does (`Tween.lerp`, ws514).
        final asOption = to.nullability == Nullability.nullable;
        var operand = expression(node.operand);
        final have = operand.rustType;
        if (have != null) {
          if (isNullable(have) && !asOption && operand is! IrNullCheck) {
            operand = IrNullCheck(operand)..rustType = _nonNull(have);
          }
        } else if (from != null &&
            from is! DynamicType &&
            from.nullability == Nullability.nullable) {
          operand = _nullChecked(operand, node.operand);
        }
        final kept = operand.rustType;
        operand = coerce(
          operand,
          kept != null && isNullable(kept)
              ? const IrType('dynamic', nullable: true)
              : const IrType('dynamic'),
        );
        // `as T?`: the `Option` the downcast hands back, Dart's null for
        // a `Null` object or another type (`decodeEnvelope(result) as T?`
        // returning `T?`, ws482).
        if (asOption) {
          return IrCall(operand, '!as_opt', [
            IrLiteral(to.parameter.name ?? 'T', const IrType('raw')),
          ])..rustType = _type(to);
        }
        return IrCall(
          IrDowncast(operand, to.parameter.name ?? 'T'),
          'clone',
          const [],
        );
      }
      // `state as T?` from a class, `T` a type parameter: by id in the
      // backend (`dart_cast_any`). From the parameter's own nullable self
      // (`value as T` on a `T?`) only null is in question.
      if (to is TypeParameterType && !_erasedParameter(to.parameter)) {
        if (from is TypeParameterType && from.parameter == to.parameter) {
          if (to.nullability == Nullability.nullable) {
            return expression(node.operand);
          }
          // `_value as T` on a `T?`: the `T` inside, or `T`'s own null
          // when `T` has one (`RestorableValue<double?>.value`, run665) --
          // an unwrap said a null `double?` was no `double?`.
          return IrStaticCall(
            null,
            'dart_as_own',
            [_asOwnOption(expression(node.operand), to)],
            fails: true,
            typeArguments: [
              _type(to.withDeclaredNullability(Nullability.nonNullable)),
            ],
          )..rustType = _type(to);
        }
        if (from is InterfaceType) {
          return IrCastTo(expression(node.operand), _type(to));
        }
      }
      // `Object` and `dynamic` are trait objects here too (`Rc<dyn Object>`).
      // `num` and `double` are abstract in dart:core too, but they are
      // scalars here, not trait objects: `number as double` on a `num` is
      // already an `f64`, and `Any` has nothing to do.
      final fromObject =
          from is DynamicType ||
          (from is InterfaceType &&
              ((_abstractLike(from.classNode) &&
                      rustScalar(from.classNode.name) == from.classNode.name) ||
                  from.classNode.name == 'Object'));
      if (fromObject &&
          from != null &&
          to is InterfaceType &&
          // `String` is abstract in dart:core, and `unsafeCast<String?>(Zone
          // .current[#Intl.locale])` wants the same `Any` downcast a struct
          // gets: the prelude's `String` is what an `Rc<dyn Object>` holds.
          (!_abstractLike(to.classNode) ||
              rustScalar(to.classNode.name) != to.classNode.name ||
              to.classNode.name == 'String' ||
              // `dart:core`'s collections are abstract there and values
              // here: `systemMessage as Map<String, dynamic>` is the
              // `Map<String, Rc<dyn Object>>` the object holds (run459).
              _coreCollection(to.classNode)) &&
          to.classNode.name != 'Object' &&
          (from is! InterfaceType || from.classNode != to.classNode)) {
        // `dynamic` is an `Rc<dyn Object>`, never an `Option`, whatever
        // its nullability says.
        if ((from is DynamicType || from.nullability != Nullability.nullable) &&
            to.nullability != Nullability.nullable) {
          final target = _type(to);
          return IrCall(
            IrDowncast(
              expression(node.operand),
              rustScalar(to.classNode.name),
              arguments: target.arguments,
            ),
            'clone',
            const [],
          );
        }
        // `Zone.current[#token] as Client?`: a `dynamic` (never an `Option`
        // here) to a nullable struct is a downcast that may fail: `cloned()`
        // of the `Option<&T>` `Any` gives.
        // ..and from an `Object?`, a `dynamic` here (ws503): by the
        // operand's recorded type, which is no `Option`.
        final operandLowered = expression(node.operand);
        final operandType = operandLowered.rustType;
        final dynamicOperand =
            from is DynamicType ||
            (operandType != null &&
                operandType.name == 'dynamic' &&
                !operandType.nullable);
        if (dynamicOperand && to.nullability == Nullability.nullable) {
          return IrCall(operandLowered, '!as_opt', [
            IrLiteral(rustScalar(to.classNode.name), const IrType('raw')),
          ], typeArguments: _type(to).arguments);
        }
        // `_objects![2] as _ImageFilter?`: an `Option<Rc<dyn Object>>` to an
        // `Option<_ImageFilter>`, element by element.
        if (from.nullability == Nullability.nullable &&
            to.nullability == Nullability.nullable) {
          // The bound typed as the value inside: a trait handle is asked
          // through `as_ref()`, and untyped it was asked for the `Rc`'s
          // own `Any` (`?.widget as HeroControllerScope?` in
          // `NavigatorState.initState`, run624).
          return IrNullAware(
            operandLowered,
            IrCall(
              IrDowncast(
                IrBound()
                  ..rustType = _recordedType(
                    from.withDeclaredNullability(Nullability.nonNullable),
                  ),
                rustScalar(to.classNode.name),
              ),
              'clone',
              const [],
            ),
          );
        }
      }
      // `ancestor as StatefulElement?` from an `Element?`: a downcast to
      // a trait, the `Option` kept when the target is nullable (the
      // prelude's `dart_cast_to` on an `Option`). An upcast stays the
      // operand: the value already is one.
      // Only a class that is a trait here: `List` and `TypedData` are
      // abstract to Kernel and prelude types or nothing here (18 "expected
      // trait, found struct" at ws340).
      // ..and `child.parentData! as ParentDataType` with the parameter
      // erased: the cast is to its bound, `ContainerParentDataMixin`
      // (13 `Rc<dyn ParentData>` where that was wanted, ws353).
      final toClass = to is InterfaceType
          ? to
          : to is TypeParameterType &&
                _erasedParameter(to.parameter) &&
                to.parameter.bound is InterfaceType
          ? (to.parameter.bound as InterfaceType).withDeclaredNullability(
              to.nullability == Nullability.nullable
                  ? Nullability.nullable
                  : Nullability.nonNullable,
            )
          : null;
      if (toClass != null &&
          from is InterfaceType &&
          _abstractLike(toClass.classNode) &&
          _translatedClass(toClass.classNode) &&
          !_scalarClass(toClass.classNode) &&
          toClass.classNode.name != 'Object' &&
          from.classNode != toClass.classNode &&
          !(typeEnvironment?.hierarchy.isSubInterfaceOf(
                from.classNode,
                toClass.classNode,
              ) ??
              true)) {
        return IrCastTo(expression(node.operand), _type(toClass));
      }
      // `x as dynamic` (and `as Object`): every value goes behind the
      // handle a `dynamic` is here, and a projected `T?` is not one --
      // `<T as DartNullable>::Or` was handed to the `dynamic` operator
      // rules, which asked it for its `Any` and found no `f64` inside
      // (`Tween.lerp`'s `(begin as dynamic) + ((end as dynamic) - ..)`,
      // the run's own panic at run796). A value already behind the handle
      // coerces to itself.
      if (to is DynamicType ||
          (to is InterfaceType &&
              to.classNode.name == 'Object' &&
              to.classNode.enclosingLibrary.importUri.toString() ==
                  'dart:core')) {
        return coerce(expression(node.operand), const IrType('dynamic'));
      }
      return expression(node.operand);
    }
    if (node is StaticTearOff) {
      return IrFunctionRef(
        node.target.enclosingClass?.name,
        node.target.name.text,
      )..rustType = _functionRefType(node.target);
    }
    // An expression the CFE moved from another file -- a mixin field's
    // initialiser into the application's constructor -- is wrapped with
    // its origin; the wrapper is not the expression (`AnimationController`
    // was refused whole for one).
    if (node is FileUriExpression) return expression(node.expression);
    if (node is ConstantExpression) return _constant(node.constant, node);
    // A method used as a value: `Ticker(_tick)` hands `this._tick` over
    // without calling it. In Rust that is a closure that calls it, which makes
    // it the same question as any other closure -- and the same answer: in a
    // borrowed position (an argument, where the parameter is `impl Fn`) it can
    // borrow the receiver, and anywhere else it would have to own it and is
    // refused. 495 of these, and the closure rule already knew what to do with
    // them.
    if (node is InstanceTearOff) {
      // A counted class's tear-off keeps a handle, exactly as a closure that
      // calls a method does -- it *is* that closure, written shorter. Without
      // this the two shapes got different answers for the same question, and
      // the tear-offs stayed refused: 503 of them.
      // `this.controller.dispose` as a value is the same closure as
      // `this.dispose` is, reaching the field through the handle it keeps.
      final holds = _counted && _rootedAtThis(node.receiver);
      // A method of a *local or parameter* used as a value: the closure
      // below captures that variable the way any Rust closure captures a
      // local. `asset.endsWith` handed to `firstWhere` is one.
      final onLocal = node.receiver is VariableGet;
      // ..and of a *constant* (`const GZipCodec().decode`): the closure
      // captures nothing, the constant is spelled inside it.
      final onConstant = node.receiver is ConstantExpression;
      // ..and any other receiver is *evaluated once* and captured, which
      // is what Dart does at the tear-off: bound outside the closure and
      // moved in. `PaintingBinding.instance.instantiateImageCodecWithSize`
      // handed to `loadImage` was refused for want of this (run745).
      final bindReceiver =
          !holds && !_borrowedArgument && !onLocal && !onConstant;
      final target = node.interfaceTarget;
      final fn = target.function;
      // The tear-off's own type is the instantiated one: `sink.add` on a
      // `Sink<List<int>>` takes a `List<int>`, not the `T` the method
      // declares (E0425 `T` in `ByteStream.toBytes`).
      // ..or, when the tear-off's type is out of reach, the receiver's type
      // arguments substituted into the method's declaration (`sink.add` on
      // a local `Sink<List<int>>`).
      // The receiver's instantiation first: `getStaticType` of the tear-off
      // still said `T` for `sink.add` on a `ByteConversionSink`.
      final torn =
          (() {
            final receiverType = _staticType(node.receiver);
            if (receiverType is! InterfaceType) return null;
            // As an instance of the *declaring* class: a `ByteConversionSink`
            // is a `Sink<List<int>>`, and `T` is `Sink`'s.
            final declaringClass = target.enclosingClass;
            final env = typeEnvironment;
            if (declaringClass == null || env == null) return null;
            final asDeclaring = env.hierarchy.getTypeAsInstanceOf(
              receiverType,
              declaringClass,
            );
            if (asDeclaring is! InterfaceType) return null;
            final declared = fn.computeFunctionType(Nullability.nonNullable);
            return Substitution.fromInterfaceType(asDeclaring)
                .substituteType(declared);
          })() ??
          _staticType(node);
      DartType positionalType(int i) =>
          torn is FunctionType && i < torn.positionalParameters.length
          ? torn.positionalParameters[i]
          : fn.positionalParameters[i].type;
      DartType namedType(String name, DartType declared) {
        if (torn is FunctionType) {
          for (final n in torn.namedParameters) {
            if (n.name == name) return n.type;
          }
        }
        return declared;
      }

      final returnType = torn is FunctionType ? torn.returnType : fn.returnType;
      if (fn.typeParameters.isNotEmpty) {
        throw Unsupported('a generic method used as a value', _sample(node));
      }
      // The closure's own parameters: positional as declared, then the named
      // ones **in name order** -- the order a call through the function type
      // uses (`_argumentsByType`). The call inside passes them on in the
      // *method's* declared order, which is the order the method was
      // emitted in. 23 tear-offs of methods with named parameters.
      final params = [
        for (var i = 0; i < fn.positionalParameters.length; i++)
          IrParam(
            _paramName(fn.positionalParameters[i], 'a$i'),
            _type(positionalType(i)),
          ),
        for (final p in _namedInTypeOrder(fn))
          IrParam(
            p.parameterName,
            _type(namedType(p.parameterName, p.type)),
            named: true,
          ),
      ];
      final receiver = node.receiver;
      // A tear-off of one of the prelude's collection methods
      // (`nodeScope._focusedChildren.remove` handed to `forEach`): the
      // call it stands for, lowered as an invocation, so that the
      // collection tables apply (`remove` is `remove_value`, not `Vec::
      // remove(usize)`, `FocusNode._removeChild`, run642). The callee's
      // own parameters name the closure's, as `params` does.
      final declaringClass = node.interfaceTarget.enclosingClass;
      if (declaringClass != null &&
          _coreCollections.contains(declaringClass.name) &&
          declaringClass.enclosingLibrary.importUri.scheme == 'dart' &&
          receiver is! ThisExpression) {
        // The receiver node itself (a clone would need the closure's free
        // variables mapped): borrowed into the call and given back to
        // the tear-off after.
        final call = InstanceInvocation(
          InstanceAccessKind.Instance,
          receiver,
          node.name,
          Arguments(
            [for (final p in fn.positionalParameters) VariableGet(p)],
            named: [
              for (final p in fn.namedParameters)
                NamedExpression(p.parameterName, VariableGet(p)),
            ],
          ),
          interfaceTarget: node.interfaceTarget,
          functionType: torn is FunctionType
              ? torn
              : fn.computeFunctionType(Nullability.nonNullable),
        );
        final tornReturns = _type(returnType);
        final IrExpr lowered;
        try {
          lowered = expression(call);
        } finally {
          receiver.parent = node;
        }
        return IrClosure(
            params,
            IrReturn(coerce(lowered, tornReturns)),
            tornReturns,
            locals: _freeLocalsIn(receiver, {}),
          )
          ..rustType = IrType.function([
            for (final p in params) p.type,
          ], tornReturns);
      }
      // The call typed by the member it reaches (`_qualified`) and coerced
      // into the tear-off's own return: a mixin's `ChildType? childAfter`
      // hands back the erased `RenderObject?` where the torn type says
      // `RenderSliver?` (`RenderViewport._attemptLayout`'s `advance:
      // childAfter`, ws527).
      // The receiver bound once (see `bindReceiver`): a closure that read
      // it again would read whatever it says the next time.
      final IrExpr? boundInit = bindReceiver && receiver is! ThisExpression
          ? expression(receiver)
          : null;
      final String? bound = boundInit == null ? null : '__t${_nextTemporary++}';
      final tornCall = _qualified(
        IrCall(
          receiver is ThisExpression
              ? null
              : bound != null
              ? (IrLocal(bound)..rustType = boundInit!.rustType)
              : expression(receiver),
          node.name.text,
          [
            for (var i = 0; i < fn.positionalParameters.length; i++)
              IrLocal(params[i].name),
            for (final p in fn.namedParameters) IrLocal(p.parameterName),
          ],
          // The adapter's call propagates like a written one would.
          fails: _fails(node.interfaceTarget),
          asyncFn: _inherentAsync(
            node.interfaceTarget,
            receiver is ThisExpression
                ? ((_member?.enclosingClass?.isAnonymousMixin ?? false)
                      ? _lowering
                      : _member?.enclosingClass)
                : _staticClass(receiver),
            null,
            onThis: receiver is ThisExpression,
          ),
          asyncTarget: _asyncMember(node.interfaceTarget),
        ),
        node.interfaceTarget,
        receiver,
      );
      final tornReturns = _type(returnType);
      final adapter = IrClosure(
        params,
        IrReturn(coerce(tornCall, tornReturns)),
        tornReturns,
        // A tear-off of `message.invoke` keeps `message`: cloned in, moved.
        locals: receiver is ThisExpression
            ? const []
            : bound != null
            ? [bound]
            : _freeLocalsIn(receiver, {}),
        holdsSelf: holds,
      );
      // Typed as the function it is, so the slot's coercion sees it: a
      // `Future<bool> Function(MethodCall)` handed to a `Future<dynamic>
      // Function(MethodCall)` slot gets its result mapped (run447).
      if (bound != null) {
        final typed = adapter
          ..rustType = IrType.function([
            for (final p in params) p.type,
          ], tornReturns);
        return IrBlockValue([
          IrLocalDecl(bound, boundInit!.rustType, boundInit),
        ], typed)..rustType = typed.rustType;
      }
      return adapter
        ..rustType = IrType.function([
          for (final p in params) p.type,
        ], _type(returnType));
    }
    throw Unsupported('expression ${node.runtimeType}', _sample(node));
  }

  /// A cascade, restored.
  ///
  /// The CFE writes `Paint()..color = c` as "bind #0, write to #0, produce #0",
  /// which is a Rust block expression exactly. Only that shape is taken: a
  /// `BlockExpression` whose statements are a switch in disguise is a different
  /// construct and waits for switch.
  IrExpr _blockValue(BlockExpression node) {
    final statements = node.body.statements;
    final value = node.value;
    if (statements.isEmpty) {
      throw Unsupported('block expression with no statements', _sample(node));
    }
    final first = statements.first;
    final bound = first is VariableStatement
        ? first.declaration.variable
        : null;
    final initial = bound?.initializer;
    if (bound == null ||
        initial == null ||
        !(value is VariableGet && value.variable == bound)) {
      // Not the cascade shape. It is still a block with a value, which is what
      // Rust's block expression is, so it needs no shape recognised -- the same
      // floor the general `Let` put under the three `Let` shapes.
      // A value declared without an initializer and read after a labelled
      // block -- a switch expression's arms, each `if (..) { #t = ..;
      // break; }` -- is definitely assigned, which Dart checked; so the
      // paths that leave the block without assigning it are dead, and
      // Rust is told so where it cannot see it (125 E0381 at ws425).
      final definite =
          bound != null &&
          initial == null &&
          value is VariableGet &&
          value.variable == bound;
      // The statements first: they declare the temporaries the value
      // reads (a switch expression's `#0`; 420 refusals the round the
      // value was lowered first, ws540).
      final lowered = [
        for (final s in statements)
          if (definite &&
              s is LabeledStatement &&
              _fallsOutUnassigned(s, bound))
            IrLabeled(
              _labelFor(s),
              IrBlock([statement(s.body), IrExprStmt(_noCaseMatched)]),
            )
          else
            statement(s),
      ];
      final produced = expression(value);
      // A block is typed as its value is, where that is known: Kernel's
      // type for the block may be wider (see the bound read above).
      return IrBlockValue(lowered, produced)..rustType = produced.rustType;
    }

    final previous = _cascade;
    final previousStatic = _cascadeStatic;
    _cascade = bound;
    // A cascade on a static filled in place acts on the static itself
    // (`log..add(b)..add(c)`, the statmut fixture): every step names it,
    // and nothing is bound.
    _cascadeStatic = _mutatedStaticOf(initial) ? expression(initial) : null;
    try {
      final steps = <IrStmt>[
        // A cascade on a local shares it: `v..setValues(..)` and `v` read
        // again after (`use of moved value: v`, vector_math).
        if (_cascadeStatic == null)
          IrLocalDecl(
            _cascadeName,
            _type(bound.type),
            _widened(initial, null, expression(initial)),
          ),
        for (final s in statements.skip(1)) statement(s),
      ];
      return IrBlockValue(steps, _cascadeRead())..rustType = _type(bound.type);
    } finally {
      _cascade = previous;
      _cascadeStatic = previousStatic;
    }
  }

  /// The cascade's receiver: the bound local, or the static it acts on.
  IrExpr _cascadeRead() => _cascadeStatic ?? IrLocal(_cascadeName);

  /// The element type of a `dart:core` list literal factory
  /// (`_GrowableList._literalN<E>(..)`, `_List._literalN`), or null for
  /// any other invocation.
  DartType? _coreListLiteral(StaticInvocation node) {
    final target = node.target;
    final owner = target.enclosingClass;
    if (owner == null ||
        !(owner.name == '_GrowableList' || owner.name == '_List') ||
        !target.name.text.startsWith('_literal') ||
        target.enclosingLibrary.importUri.toString() != 'dart:core') {
      return null;
    }
    return node.arguments.types.singleOrNull ?? const DynamicType();
  }

  /// `dynamic`, `Object?`, `void`: a slot that takes anything.
  static bool _isTopType(DartType t) =>
      t is DynamicType ||
      t is VoidType ||
      (t is InterfaceType &&
          t.classNode.name == 'Object' &&
          t.nullability == Nullability.nullable);

  /// `let` temporaries that stand for the place they were bound to.
  final _letAliases = <Variable, Expression>{};

  /// A local's read, or a static's that is filled in place.
  bool _isAliasablePlace(Expression e) {
    var bare = e;
    while (bare is FileUriExpression) {
      bare = bare.expression;
    }
    if (bare is VariableGet) return !_temporaries.containsKey(bare.variable);
    // A field of `this` is a place too: `=> _map[v] = ..` binds `this._map`
    // in a temporary and inserted into a clone of it (the listgen
    // fixture); acting on the field is acting on the object.
    if (bare is InstanceGet &&
        bare.receiver is ThisExpression &&
        bare.interfaceTarget is Field) {
      return true;
    }
    return _mutatedStaticOf(bare);
  }

  /// The static the cascade acts on directly, when it is one filled in
  /// place; null otherwise.
  IrExpr? _cascadeStatic;

  bool _mutatedStaticOf(Expression e) {
    var bare = e;
    while (bare is FileUriExpression) {
      bare = bare.expression;
    }
    if (bare is! StaticGet) return false;
    final target = bare.target;
    return target is Field &&
        target.isStatic &&
        _mutatedStatics.contains(target);
  }

  /// Whether control leaves the labelled block only by falling out of an
  /// else-less `if` at its end, with nothing before it assigning `bound`
  /// unconditionally: then the fall-out is the dead path of a definite
  /// assignment.
  static bool _fallsOutUnassigned(
    LabeledStatement node,
    DeclaredVariable bound,
  ) {
    final body = node.body;
    if (body is! Block || body.statements.isEmpty) return false;
    // The last arm may sit in a block of its own with the variables its
    // pattern binds (`{ final double lower; final double upper; if (..)
    // {..} }` for a record pattern, `scaleFontSize`, ws473): the block's
    // last statement is the arm, the ones before it are looked at too.
    // ..and an arm the AOT compiler removed is left as `{ ; }` after the
    // last real one (the `null` arm of `Typography._withPlatform`, whose
    // callers never pass null, run566): trailing empties say nothing.
    final before = <Statement>[];
    Statement? last = _lastMeaningful(body.statements, before);
    while (last is Block && last.statements.isNotEmpty) {
      last = _lastMeaningful(last.statements, before);
    }
    if (last is! IfStatement || last.otherwise != null) return false;
    for (final s in before) {
      if (s is ExpressionStatement) {
        final e = s.expression;
        if (e is VariableSet && e.variable == bound) return false;
      }
    }
    return true;
  }

  /// The last statement of `statements` that says anything, with the ones
  /// before it added to `before`; null when none does.
  static Statement? _lastMeaningful(
    List<Statement> statements,
    List<Statement> before,
  ) {
    var end = statements.length;
    while (end > 0 && _saysNothing(statements[end - 1])) {
      end--;
    }
    if (end == 0) return null;
    before.addAll(statements.take(end - 1));
    return statements[end - 1];
  }

  static bool _saysNothing(Statement s) =>
      s is EmptyStatement || (s is Block && s.statements.every(_saysNothing));

  static final _noCaseMatched = IrLiteral(
    'unreachable!("dart2rust: no case of an exhaustive switch matched")',
    IrType('raw'),
  );

  /// The receiver the enclosing cascade bound. Reads of it become a local.
  Variable? _cascade;
  static const _cascadeName = 'cascaded';

  /// A closure literal, when it captures nothing this compiler cannot give it.
  ///
  /// A closure reaching `this` is refused: it outlives the call that made it,
  /// and `this` is a borrow, so it needs an ownership arrangement rather than a
  /// translation. That is 60% of `package:flutter`'s closures and a round of
  /// its own.
  /// A parameter's type: as `_type`, except that `void?` -- `_Callback<T>`
  /// is `void Function(T? result)`, and `_futurize<void>` instantiates it --
  /// is the `Option<()>` the generic `Option<T>` became there. A `void`
  /// *return* type is nullable in Kernel too and stays `()`.
  IrType _paramType(DartType t) =>
      t is VoidType && t.nullability == Nullability.nullable
      ? const IrType('void', nullable: true)
      : _type(t);

  /// A nullable, kept type parameter of the declaration being lowered, as
  /// a slot: in a generic declaration's signature or field it is spelled
  /// `<T as DartNullable>::Or` (`IrType.projected`) -- Dart's `T?` with
  /// `T` bound to `X?` is `X?`, one `Option` layer -- and a value crosses
  /// it through `IrNullableOf`. Only the class's or the member's own
  /// parameters: another declaration's `T` is not a name here.
  bool _projectedSlot(DartType? t) {
    if (t is! TypeParameterType ||
        t.nullability != Nullability.nullable ||
        _erasedParameter(t.parameter)) {
      return false;
    }
    final declaration = t.parameter.declaration;
    final member = _member;
    if (!identical(declaration, _lowering) &&
        !identical(declaration, member) &&
        !(member != null && identical(declaration, member.function))) {
      return false;
    }
    return !_spelledAsBound(t.parameter);
  }

  /// A parameter `_type` spells as its bound rather than as itself (the
  /// scalar and the list bounds): no Rust type parameter to project.
  bool _spelledAsBound(TypeParameter p) {
    final bound = p.bound;
    return bound is InterfaceType &&
        const {
          'String',
          'int',
          'double',
          'bool',
          'Iterable',
          'List',
        }.contains(bound.classNode.name);
  }

  /// Whether a value crosses a projected slot at a *use*: the declaration
  /// says `T?` and what this use puts in for `T` is a non-nullable type
  /// parameter of the code here -- then the slot is `<U as DartNullable>::
  /// Or` where the code has an `Option<U>`. For `U?` or a concrete type
  /// the slot already *is* the `Option` the code has.
  bool _crossing(DartType? declared, DartType? binding) {
    if (declared is! TypeParameterType ||
        declared.nullability != Nullability.nullable ||
        _erasedParameter(declared.parameter)) {
      return false;
    }
    // A nullable `U?` put in crosses too, now that a type argument `U?`
    // is spelled projected (`_erasedArguments`): the slot is `<U as
    // DartNullable>::Or` either way, and the code has an `Option<U>`.
    if (binding is! TypeParameterType) return false;
    return _projectedSlot(
      binding.withDeclaredNullability(Nullability.nullable),
    );
  }

  IrExpr _acrossBinding(
    IrExpr value,
    DartType? declared,
    DartType? binding, {
    required bool toOption,
  }) {
    if (!_crossing(declared, binding)) return value;
    if (value.rustType?.projected == true) return value;
    final held = binding!.withDeclaredNullability(Nullability.nullable);
    return IrNullableOf(value, _type(binding).name, toOption: toOption)
      ..rustType = toOption ? _type(held) : _edgeType(held);
  }

  /// What a member access puts in for a declared type parameter: the
  /// member's own by the call's type arguments, the class's by the
  /// receiver's.
  DartType? _bindingOf(
    DartType? declared,
    Member member,
    Expression receiver, [
    Arguments? args,
  ]) {
    if (declared is! TypeParameterType) return null;
    final p = declared.parameter;
    final fn = member.function;
    if (fn != null && fn.typeParameters.contains(p)) {
      if (args == null || args.types.length != fn.typeParameters.length) {
        return null;
      }
      return args.types[fn.typeParameters.indexOf(p)];
    }
    final env = typeEnvironment;
    final receiverType = receiver is ThisExpression
        ? (env == null
              ? null
              : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable))
        : _staticType(receiver);
    return _keptFor(member.enclosingClass, receiverType)[p];
  }

  /// What an argument's slot puts in for the callee's type parameter: the
  /// dispatch's receiver for a class's, the call's type arguments for the
  /// callee's own.
  /// The constructor whose arguments are being lowered, with what the
  /// construction puts in for its class's parameters: `Foo<T>(..)` by the
  /// call's type arguments, `super(..)` by this class's supertype.
  FunctionNode? _constructedCallee;
  Map<TypeParameter, DartType> _constructedArgs = const {};

  List<IrExpr> _constructing(
    FunctionNode callee,
    Map<TypeParameter, DartType> args,
    List<IrExpr> Function() lower,
  ) {
    final wasCallee = _constructedCallee;
    final wasArgs = _constructedArgs;
    _constructedCallee = callee;
    _constructedArgs = args;
    try {
      return lower();
    } finally {
      _constructedCallee = wasCallee;
      _constructedArgs = wasArgs;
    }
  }

  /// What this class's supertype puts in for a base's parameters.
  Map<TypeParameter, DartType> _superBinding(Class cls, Class base) {
    final env = typeEnvironment;
    if (env == null || base.typeParameters.isEmpty) return const {};
    final asBase = env.hierarchy.getTypeAsInstanceOf(
      cls.getThisType(env.coreTypes, Nullability.nonNullable),
      base,
    );
    if (asBase is! InterfaceType) return const {};
    return {
      for (
        var i = 0;
        i < base.typeParameters.length && i < asBase.typeArguments.length;
        i++
      )
        base.typeParameters[i]: asBase.typeArguments[i],
    };
  }

  DartType? _argumentBinding(FunctionNode? callee, DartType? declared) {
    if (declared is! TypeParameterType || callee == null) return null;
    final p = declared.parameter;
    if (callee.typeParameters.contains(p)) {
      return identical(callee, _genericCallee) ? _genericArgs[p] : null;
    }
    // An erased parameter of the constructed class is spelled as its
    // bound, whatever the call put in for it (`Entry<S>(v)` with `Entry.T`
    // erased takes an `Rc<dyn Object>`, the outparam fixture).
    if (identical(callee, _constructedCallee)) {
      return _erasedParameter(p) ? null : _constructedArgs[p];
    }
    final landing = _dispatchMember;
    if (landing == null || !identical(callee, _dispatchInterface)) return null;
    return _keptFor(landing.enclosingClass, _dispatchReceiverType)[p];
  }

  /// The classes above one, nearest first, through `extends`, `with` and
  /// `implements`.
  Iterable<Class> _kernelAncestors(Class c) sync* {
    final seen = <Class>{};
    final work = [c];
    while (work.isNotEmpty) {
      final k = work.removeLast();
      for (final st in [
        if (k.supertype != null) k.supertype!,
        if (k.mixedInType != null) k.mixedInType!,
        ...k.implementedTypes,
      ]) {
        final a = st.classNode;
        if (seen.add(a)) {
          yield a;
          work.add(a);
        }
      }
    }
  }

  /// A signature's or a field's type: projected where `_projectedSlot`.
  IrType _edgeType(DartType t) {
    final ir = _type(t);
    return _projectedSlot(t)
        ? IrType(ir.name, nullable: true, projected: true)
        : ir;
  }

  IrType _edgeReturnType(FunctionNode function) =>
      function.returnType is NeverType
      ? const IrType('Never')
      : _edgeType(function.returnType);

  /// `value` across a projected slot: into the body's `Option<T>` from the
  /// spelled `T?` (`toOption`), or back out. Itself for any other slot.
  IrExpr _acrossEdge(IrExpr value, DartType? slot, {required bool toOption}) {
    if (!_projectedSlot(slot)) return value;
    // Already the spelled `T?` (a constructor parameter): nothing to cross.
    if (value.rustType?.projected == true) return value;
    return IrNullableOf(value, _type(slot!).name, toOption: toOption)
      ..rustType = toOption ? _type(slot) : _edgeType(slot);
  }

  /// A body behind its projected parameters: each re-bound, in the same
  /// scope, as the `Option<T>` the body reads and writes.
  IrStmt _withEdgeParams(
    FunctionNode fn,
    IrStmt body, {
    List<DartType>? positional,
  }) {
    final prologue = <IrStmt>[];
    void rebind(String name, DartType type) {
      if (!_projectedSlot(type)) return;
      final held = _type(type);
      prologue.add(
        IrLocalDecl(
          name,
          held,
          IrNullableOf(
            IrLocal(name)..rustType = _edgeType(type),
            held.name,
            toOption: true,
          )..rustType = held,
        ),
      );
    }

    // The type the *signature* declared, which is what the body has in
    // hand: a mixin copy takes its parameter types from the declaration it
    // was copied from (`_declaredParamTypes`), and those name the
    // declaration's own type parameters -- not this copy's, which is what
    // `_projectedSlot` asks about. Read from `p.type` instead, the prologue
    // unprojected a parameter the signature had spelled `Option<U>`
    // (`_OverridableActionMixin._getOverrideAction`, 3 at ws798).
    for (final (i, p) in fn.positionalParameters.indexed) {
      rebind(_paramName(p), positional?[i] ?? _declaredParamTypes[p] ?? p.type);
    }
    for (final p in fn.namedParameters) {
      if (!_inspectorOnly(p.parameterName)) {
        rebind(p.parameterName, _declaredParamTypes[p] ?? p.type);
      }
    }
    if (prologue.isEmpty) return body;
    return IrBlock([
      ...prologue,
      if (body is IrBlock) ...body.statements else body,
    ]);
  }

  /// Whether a callee is translated code, whose signature spells a `T?`
  /// projected; a prelude member's Rust is its own.
  bool _translatedCallee(FunctionNode? callee) {
    final member = callee?.parent;
    if (member is! Member) return false;
    final owner = member.enclosingClass;
    if (owner != null) return _translatedClass(owner);
    final uri = member.enclosingLibrary.importUri;
    return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
  }

  /// The declared return of the member whose body is being lowered, for
  /// its own `return`s to cross; null inside a closure, whose function
  /// type is spelled with `Option<T>`.
  DartType? _edgeReturn;

  /// A `dynamic` closure parameter takes the expected function type's, when
  /// there is one at that position (see `_expectedFunction`).
  /// Closure parameters whose Rust type is the *expected* one rather than
  /// the declared (see `_closureParamType`): a read of one has that type,
  /// not what Kernel says, and an argument made of it is widened from it.
  final Map<Variable, DartType> _retyped = {};

  /// A parameter of a mixin application's copy of a method, typed by the
  /// mixin's own declaration (see `_lowerProcedure`'s `signature`): its
  /// reads are of that type, the trait's.
  final Map<Variable, DartType> _declaredParamTypes = {};

  IrType _closureReturnType(FunctionType? expected, FunctionNode fn) {
    if (expected != null) {
      try {
        return _type(expected.returnType);
      } on Unsupported {
        // Fall through to the declared one.
      }
    }
    return _type(fn.returnType);
  }

  DartType _closureParamType(FunctionType? expected, int i, DartType declared) {
    if (expected == null || i >= expected.positionalParameters.length)
      return declared;
    final wanted = expected.positionalParameters[i];
    // The slot's parameter is an erased one: the closure takes the bound
    // (`Rc<dyn Notification>`) and reads it as what it declared
    // (`expression` on a `VariableGet`), 111 closure signature mismatches
    // at ws281.
    if (wanted is TypeParameterType && _erasedParameter(wanted.parameter)) {
      return wanted;
    }
    if (declared is DynamicType) return wanted;
    // TFA narrows the closure's own `int? result` to `int` when no caller
    // passes null; the `Fn(Option<T>)` it is handed to did not change.
    if (declared is InterfaceType &&
        wanted is InterfaceType &&
        declared.classNode == wanted.classNode &&
        declared.nullability != Nullability.nullable &&
        wanted.nullability == Nullability.nullable) {
      return wanted;
    }
    if (declared is FunctionType && wanted is FunctionType) return wanted;
    // `_futurize<void>`: the callback the closure declares as `Object?`
    // is a `void?` -- `Option<()>` -- in the instantiated signature.
    if (wanted is VoidType) return wanted;
    return declared;
  }

  DartType _retype(Variable p, DartType chosen) {
    if (chosen != p.type) _retyped[p] = chosen;
    return chosen;
  }

  /// A tear-off's type: the function's own signature, so a slot of
  /// another function type gets its adapter from `coerce` (`TextStyle.lerp`
  /// handed to `WidgetStateProperty.lerp<TextStyle?>`, whose `T?` is one
  /// `Option` deeper; 161 tear-offs typed `dynamic` at ws387).
  IrType? _functionRefType(Member target) {
    final function = target.function;
    if (function == null) return null;
    try {
      var type = function.computeFunctionType(Nullability.nonNullable);
      // A generative constructor's function returns nothing in Kernel;
      // as a value it makes an instance of its class.
      final cls = target.enclosingClass;
      if (target is Constructor && cls != null) {
        type = FunctionType(
          type.positionalParameters,
          InterfaceType(cls, Nullability.nonNullable, [
            for (final p in cls.typeParameters)
              TypeParameterType(p, Nullability.nonNullable),
          ]),
          Nullability.nonNullable,
          namedParameters: type.namedParameters,
          requiredParameterCount: type.requiredParameterCount,
        );
      }
      return _type(type);
    } on Unsupported {
      return null;
    }
  }

  IrExpr _closure(FunctionNode fn, Node origin) {
    // Taken once, for this closure: a closure nested in the body is not the
    // one the context described.
    final expected = _expectedFunction;
    _expectedFunction = null;
    // A closure that only reads `final` fields of `this` copies them in
    // instead of holding `this`. A `final` field cannot change, so the copy
    // and the read are the same value -- see `IrClosure.captures`. This is
    // the one case where copying is sound, and it is 345 of the 1319 closures
    // that reach `this`.
    final finals = _finalFieldsRead(fn);
    final copies = finals != null && !_borrowedArgument;
    // A counted class's closure keeps a handle to the object, so `this` is
    // available to it and nothing has to be copied or borrowed.
    if (_reachesThis(fn) &&
        !_counted &&
        !copies &&
        !_lendingLocal &&
        !(_borrowedArgument && _onlyReadsThis(fn))) {
      TreeNode? up = origin is TreeNode ? origin : null;
      while (up != null && up is! Member) {
        up = up.parent;
      }
      final member = up as Member?;
      throw Unsupported(
        'closure capturing `this` in ${member?.enclosingClass?.name}.'
        '${member?.name.text} (${member.runtimeType}'
        '${member is Procedure ? " ${member.kind}" : ""}, '
        'static=${member is Procedure
            ? member.isStatic
            : member is Field
            ? member.isStatic
            : "?"})',
        _sample(origin),
      );
    }
    final body = fn.body;
    if (body == null)
      throw Unsupported('closure with no body', _sample(origin));
    final was = _captured;
    // A counted class's closure keeps the object itself, so nothing is copied
    // out of it: the fields are reached through the handle as usual.
    final holds = _counted && _reachesThis(fn) && !copies;
    if (copies) _captured = {for (final f in finals) f.name.text};
    // A closure's parameters are an edge like a method's: a `T?` of the
    // enclosing declaration is spelled projected (`<T as DartNullable>::
    // Or`), which is what every slot of function type says, and rebound to
    // the body's `Option<T>` in a prologue (`_withEdgeParams`). Spelled
    // `Option<T>` it did not fit `RadioListTile<T?>`'s `onChanged` when
    // the state's `T` was itself nullable (`_SettingsListItemState.build`,
    // run689).
    final positionalTypes = [
      for (final (i, p) in fn.positionalParameters.indexed)
        _retype(p, _closureParamType(expected, i, p.type)),
    ];
    try {
      final closure = IrClosure(
        [
          for (final (i, p) in fn.positionalParameters.indexed)
            IrParam(
              _paramName(p),
              _projectedSlot(positionalTypes[i])
                  ? _edgeType(positionalTypes[i])
                  : _paramType(positionalTypes[i]),
            ),
          // Named parameters, **sorted by name**. A Rust closure has only
          // positions, and a call through a function value sees only the
          // function *type*, whose named parameters Dart keeps in name order
          // -- so that order is the one both ends can agree on. They used to
          // be left off entirely, which made every closure with a named
          // parameter a closure whose body read variables it did not have.
          for (final p in _namedInTypeOrder(fn))
            IrParam(p.parameterName, _type(p.type), named: true),
        ],
        _withEdgeParams(fn, _lowerBody(fn, body), positional: positionalTypes),
        // The return as the body was lowered against it: the slot's, when
        // a parameter's function type set one (`_lowerBody`'s expected
        // return), else the closure's own. Typing the closure by its own
        // `Color` while its returns were made `Option<..>` for the
        // `Color?` slot put a second `Some` on at the slot (ws448).
        _closureReturnType(expected, fn),
        isAsync: fn.asyncMarker == AsyncMarker.Async,
        captures: copies
            ? [for (final f in finals) IrParam(f.name.text, _type(f.type))]
            : const [],
        locals: _freeLocals(fn),
        holdsSelf: holds,
      );
      // A function value is typed by its own signature -- the parameters
      // as lowered (retyped to the expected ones where they were), the
      // return as declared -- so a slot of another function type gets its
      // adapter from `coerce` (1233 untyped closures at ws387).
      closure.rustType = IrType.function([
        for (final p in closure.params) p.type,
      ], closure.returns);
      return closure;
    } finally {
      _captured = was;
    }
  }

  /// The locals of the enclosing function a closure reads: they are cloned
  /// in just before it is made, and the closure moves the clones. An
  /// `Rc<dyn Fn>` is `'static`, and a closure borrowing `callback` and
  /// `arg1` from the frame that made it was 9 "does not live long enough".
  List<String> _freeLocals(FunctionNode fn) =>
      _freeLocalsIn(fn, {...fn.positionalParameters, ...fn.namedParameters});

  /// The locals read anywhere under a node and declared nowhere under it.
  List<String> _freeLocalsIn(TreeNode node, Set<Variable> own) {
    final finder = _LocalFinder();
    node.accept(finder);
    final inside = {...finder.declared, ...own};
    final names = <String>[];
    for (final v in finder.read) {
      if (inside.contains(v)) continue;
      // The name the read itself uses: a temporary's given one, else what
      // the human wrote.
      final written = v.cosmeticName;
      final name =
          _temporaries[v] ??
          (written == null || written.startsWith('#') ? _nameFor(v) : written);
      if (!names.contains(name)) names.add(name);
    }
    // A type literal of the enclosing method's observed type parameter
    // reads the hidden `__ty_<i>` (`_typeLiteral`): a local of the method,
    // captured as one.
    final member = _member;
    if (member is Procedure) {
      final values = _typeValues(member);
      if (values.isNotEmpty) {
        final finder = _TypeLiteralFinder(
          member.function.typeParameters.toSet(),
        );
        node.accept(finder);
        for (final t in finder.found) {
          final index = member.function.typeParameters.indexOf(t);
          if (values.contains(index) && !names.contains('__ty_$index')) {
            names.add('__ty_$index');
          }
        }
      }
    }
    return names;
  }

  /// Whether a supertype that becomes a trait declares a mutable field.
  static bool _inheritsMutableTraitField(Class node) {
    final seen = <Class>{};
    bool walk(Class c) {
      if (!seen.add(c)) return false;
      final supers = <Class>[
        if (c.superclass != null) c.superclass!,
        for (final t in c.implementedTypes) t.classNode,
        if (c.mixedInClass != null) c.mixedInClass!,
      ];
      for (final s in supers) {
        if ((s.isAbstract || s.isMixinDeclaration) &&
            s.fields.any((f) => !f.isStatic && !f.isFinal)) {
          return true;
        }
        if (walk(s)) return true;
      }
      return false;
    }

    return walk(node);
  }

  /// Whether any method body (not a constructor) writes a field of `this`.
  static bool _writesFieldInMethod(Class node) {
    final finder = _ThisWriteFinder();
    for (final p in node.procedures) {
      if (p.isStatic || p.isAbstract) continue;
      p.function.body?.accept(finder);
      if (finder.found) return true;
    }
    return false;
  }

  /// The fields a closure body reads on `this`, when **every** one is `final`
  /// and nothing else about `this` is touched. Null when it is not that shape.
  List<Field>? _finalFieldsRead(FunctionNode fn) {
    final use = _ThisUse();
    fn.accept(use);
    // A closure that *writes* a shared field is fine -- the cell is what makes
    // it fine -- so writing no longer makes it demanding when every field it
    // touches is either final or shared.
    final finder = _FinalFieldReads(_sharedFields);
    fn.accept(finder);
    if (use.demandingBeyondFields) return null;
    if (!finder.allCarried || finder.fields.isEmpty) return null;
    return finder.fields.values.toList();
  }

  /// The fields the closure being lowered copies in. A read of one is a read
  /// of the local, not of `this`.
  Set<String> _captured = const {};

  /// Whether an expression is `this`, or a chain of field reads from it.
  /// A receiver's shape, for a refusal to name: `this.field!`, `param`.
  static String _shape(Expression e) => switch (e) {
    ThisExpression() => 'this',
    InstanceGet(:final receiver) => '${_shape(receiver)}.field',
    NullCheck(:final operand) => '${_shape(operand)}!',
    Let() => 'let',
    VariableGet(:final variable) =>
      variable.parent is FunctionNode ? 'param' : 'local',
    StaticGet() => 'static',
    _ => '${e.runtimeType}',
  };

  /// Statics whose value's field is written somewhere in the package
  /// (`Owner.name`, or `.name` for a top-level): the driver marks them
  /// mutable so they live in a cell.
  static final staticFieldWrites = <String>{};

  /// The concrete copy of a mixin declaration's abstract procedure in an
  /// application of the mixin, if the CFE left one there.
  Procedure? _appliedBody(Class mixin, Procedure declared) =>
      _appliedProcedure(mixin, declared.name.text, kind: declared.kind);

  /// A concrete, non-static procedure named `name` in an application of
  /// `mixin` -- the mixin's own method, whether or not the hollow
  /// declaration still lists it (TFA drops the ones it does not need
  /// there: `SchedulerBinding.initInstances` was nowhere in the
  /// declaration and everywhere in the applications).
  Procedure? _appliedProcedure(
    Class mixin,
    String name, {
    ProcedureKind? kind,
  }) {
    // Deduplicated applications (`dart:mixin_deduplication`) may be hollow
    // themselves; the copy is in whichever application kept it.
    for (final application in applications[mixin] ?? const <Class>[]) {
      for (final p in application.procedures) {
        if (p.name.text == name &&
            (kind == null || p.kind == kind) &&
            !p.isAbstract &&
            !p.isStatic) {
          return p;
        }
      }
    }
    return null;
  }

  bool _rootedAtThis(Expression e) => switch (e) {
    ThisExpression() => true,
    InstanceGet(:final receiver) => _rootedAtThis(receiver),
    _ => false,
  };

  bool _reachesThis(FunctionNode fn) {
    final finder = _ThisFinder();
    fn.accept(finder);
    return finder.found;
  }

  /// The class a `super` call really lands in.
  ///
  /// `class X extends A with B` becomes, in Kernel, `X extends _A&B extends A`
  /// -- and `_A&B` is the CFE's, not anything upstream wrote, so this compiler
  /// skips it. A `super.foo()` inside `X` resolves to a member of `_A&B`, so
  /// asking the target which class encloses it gave a class that is not in the
  /// output: 180 refusals reading `super call into `_MixinApplication12&Rende-
  /// rBox&...`, which is not in this file`.
  ///
  /// The class a reader would name is the mixin that declares the member, or
  /// the first real superclass above it if none does.
  /// The type arguments a `super` call's base carries, in the terms of the
  /// declaration whose body this is (`IrSuperCall.baseArguments`).
  ///
  /// From an application's copy of a mixin body, the base is reached as
  /// the application is an instance of it (`ModalRoute<T>`'s application
  /// is a `TransitionRoute<T>`), and the application's parameters are
  /// mapped back onto the mixin's through the applied type
  /// (`LocalHistoryRoute<T>`). Elsewhere the base is a supertype of the
  /// class itself. Not expressible -- an application argument that is not
  /// a bare parameter -- is empty.
  List<IrType> _superBaseArguments(Class base) {
    if (base.typeParameters.isEmpty) return const [];
    final env = typeEnvironment;
    final lowering = _lowering;
    if (env == null || lowering == null) return const [];
    final enclosing = _member?.enclosingClass;
    final fromApplication = enclosing != null && enclosing.isAnonymousMixin;
    final from = fromApplication ? enclosing : lowering;
    final asBase = env.hierarchy.getTypeAsInstanceOf(
      from.getThisType(env.coreTypes, Nullability.nonNullable),
      base,
    );
    if (asBase is! InterfaceType) return const [];
    var arguments = asBase.typeArguments;
    if (fromApplication && !identical(enclosing, lowering)) {
      final mapped = _inMixinTerms(enclosing, lowering, arguments);
      if (mapped == null) return const [];
      arguments = mapped;
    }
    try {
      return _erasedArguments(base, arguments);
    } on Unsupported {
      return const [];
    }
  }

  /// `types`, spelled with the application's parameters, in the terms of
  /// the mixin's own: the applied type (`LocalHistoryRoute<T_app>`) maps
  /// each application parameter onto the mixin's. Null when an applied
  /// argument is not a bare parameter, or an application parameter is
  /// left over.
  List<DartType>? _inMixinTerms(
    Class application,
    Class mixin,
    List<DartType> types,
  ) {
    Supertype? applied;
    for (final t in application.implementedTypes) {
      if (t.classNode == mixin) applied = t;
    }
    if (applied == null) return null;
    final map = <TypeParameter, DartType>{};
    for (var i = 0; i < applied.typeArguments.length; i++) {
      final a = applied.typeArguments[i];
      if (a is! TypeParameterType ||
          !application.typeParameters.contains(a.parameter) ||
          i >= mixin.typeParameters.length) {
        return null;
      }
      map[a.parameter] = TypeParameterType(
        mixin.typeParameters[i],
        Nullability.nonNullable,
      );
    }
    final substitution = Substitution.fromMap(map);
    final mapped = [for (final t in types) substitution.substituteType(t)];
    for (final t in mapped) {
      if (_mentionsForeignParameter(t, application.typeParameters)) {
        return null;
      }
    }
    return mapped;
  }

  /// The traits a mixin is applied over in *every* application of it, as
  /// its trait's supertraits.
  ///
  /// A mixin's bodies come from an application (`_appliedBody`), and a
  /// `super` call in one dispatches to the previous mixin of that
  /// application (`_realOwner`), not to the `on` clause: the trait's
  /// default `init_instances` calling `scheduler_binding_super_init_
  /// instances(self)` needs `Self: GestureBinding`, which a method-level
  /// `where Self:` cannot say on a dispatchable method (E0038). What every
  /// application of the mixin puts under it, the trait can require --
  /// the closed world has no application that does otherwise. The
  /// arguments come from the first application, in the mixin's terms.
  List<IrType> _appliedOver(Class mixin) {
    final env = typeEnvironment;
    final apps = applications[mixin];
    if (env == null || apps == null || apps.isEmpty) return const [];
    List<Class> under(Class application) {
      final chain = <Class>[];
      var c = application.superclass;
      while (c != null) {
        if (c.isAnonymousMixin) {
          for (final t in c.implementedTypes) {
            chain.add(t.classNode);
          }
          c = c.superclass;
        } else {
          chain.add(c);
          break;
        }
      }
      return chain;
    }

    var common = under(apps.first).toSet();
    for (final a in apps.skip(1)) {
      common = common.intersection(under(a).toSet());
    }
    final already = {
      for (final t in mixin.implementedTypes) t.classNode,
      for (final t in mixin.onClause) t.classNode,
    };
    final first = apps.first;
    final thisType = first.getThisType(env.coreTypes, Nullability.nonNullable);
    final found = <IrType>[];
    for (final x in under(first)) {
      if (!common.contains(x) ||
          already.contains(x) ||
          x.name == 'Object' ||
          !_translatedClass(x) ||
          !_abstractLike(x)) {
        continue;
      }
      final asX = env.hierarchy.getTypeAsInstanceOf(thisType, x);
      if (asX is! InterfaceType) continue;
      final mapped = _inMixinTerms(first, mixin, asX.typeArguments);
      if (mapped == null) continue;
      try {
        found.add(_type(InterfaceType(x, Nullability.nonNullable, mapped)));
      } on Unsupported {
        continue;
      }
    }
    return found;
  }

  static bool _mentionsForeignParameter(
    DartType t,
    List<TypeParameter> foreign,
  ) {
    if (t is FutureOrType) {
      return _mentionsForeignParameter(t.typeArgument, foreign);
    }
    if (t is RecordType) {
      return t.positional.any((a) => _mentionsForeignParameter(a, foreign)) ||
          t.named.any((n) => _mentionsForeignParameter(n.type, foreign));
    }
    if (t is TypeParameterType) return foreign.contains(t.parameter);
    if (t is InterfaceType) {
      return t.typeArguments.any((a) => _mentionsForeignParameter(a, foreign));
    }
    if (t is FunctionType) {
      return _mentionsForeignParameter(t.returnType, foreign) ||
          t.positionalParameters.any(
            (a) => _mentionsForeignParameter(a, foreign),
          ) ||
          t.namedParameters.any(
            (n) => _mentionsForeignParameter(n.type, foreign),
          );
    }
    return false;
  }

  /// A call's own type arguments, spelled; empty when one cannot be.
  List<IrType> _typeArgumentsOf(Arguments arguments) {
    try {
      return [for (final t in arguments.types) _type(t)];
    } on Unsupported {
      return const [];
    }
  }

  /// The symbol an `external` member's `@Native` annotation registers it
  /// under (`PlatformConfigurationNativeApi::SetNeedsReportTimings`), as
  /// the CFE leaves it: a `pragma("cfe:ffi:native-marker", Native<..>(
  /// symbol: ..))`. Null for an external with no such annotation.
  /// A `@Native` member through the one boundary the runtime answers
  /// (`dart_native` in the prelude): the symbol the engine registers it
  /// under, the arguments as objects, and whether a value comes back. The
  /// generated code sees only the Dart signature; what the symbol does is
  /// the native host's (run455). Null where the member has no symbol or a
  /// signature the boundary cannot spell -- the caller's refusal then.
  /// Both the `external` member and the one the AOT FFI transform gave a
  /// body (`__sendPlatformMessage`, run497) come here: the transform
  /// leaves the marker on the member.
  IrStmt? _nativeBoundary(FunctionNode function, Member member, String name) {
    final symbol = _nativeSymbol(member);
    if (symbol == null) return null;
    {
      try {
        // As `dynamic` slots: a nullable handle (`oldLayer?._nativeLayer` into
        // `SceneBuilder._pushTransform`, run552) goes over as the `Null`
        // object where an `Object` slot unwrapped it.
        final args = [
          for (final p in function.positionalParameters)
            coerce(
              IrLocal(_paramName(p))..rustType = _type(p.type),
              IrType('dynamic'),
            ),
        ];
        final returns = function.returnType;
        final symbolText = IrLiteral(symbol, const IrType('String'));
        final passed = IrListLiteral(args, IrType('dynamic'));
        if (returns is VoidType || returns is NeverType) {
          final call = IrStaticCall(null, 'dart_native', [
            symbolText,
            passed,
            IrLiteral('false', const IrType('bool')),
          ], fails: true)..rustType = const IrType('dynamic');
          if (returns is VoidType) return IrBlock([IrExprStmt(call)]);
          return IrBlock([IrExprStmt(call), IrExprStmt(_unreachable)]);
        }
        // A value comes back as the declared type (`NativeAnswer`):
        // the host's object read as it, or the absent engine's value.
        final type = _type(returns);
        final valued = IrStaticCall(
          null,
          'dart_native_as',
          [symbolText, passed],
          fails: true,
          typeArguments: [type],
        )..rustType = type;
        return IrBlock([IrReturn(valued)]);
      } on Unsupported catch (error) {
        // A signature the boundary cannot spell: the refusal below.
        if (Platform.environment['DART2RUST_TRACE_NATIVE'] != null) {
          stderr.writeln('TRACE_NATIVE $name unsupported: $error');
        }
      }
    }
    return null;
  }

  String? _nativeSymbol(Member member) {
    for (final a in member.annotations) {
      if (a is! ConstantExpression) continue;
      final c = a.constant;
      if (c is! InstanceConstant || c.classNode.name != 'pragma') continue;
      InstanceConstant? options;
      var marker = false;
      for (final e in c.fieldValues.entries) {
        final field = e.key.asField.name.text;
        final v = e.value;
        // Two spellings: the marker the CFE leaves on the member written
        // (`cfe:ffi:native-marker`), and the pragma on the `$Method$
        // FfiNative` external its transform synthesizes (`vm:ffi:native`).
        if (field == 'name' &&
            v is StringConstant &&
            (v.value == 'cfe:ffi:native-marker' ||
                v.value == 'vm:ffi:native')) {
          marker = true;
        }
        if (field == 'options' && v is InstanceConstant) options = v;
      }
      if (!marker || options == null || options.classNode.name != 'Native') {
        continue;
      }
      for (final e in options.fieldValues.entries) {
        final v = e.value;
        if (e.key.asField.name.text == 'symbol' && v is StringConstant) {
          return v.value;
        }
      }
    }
    return null;
  }

  /// `super.m` as a value: a closure over the super call, its parameters
  /// the method's (see the `InstanceTearOff` case).
  IrExpr _superTearOff(SuperPropertyGet node, Procedure target) {
    final fn = target.function;
    if (fn.typeParameters.isNotEmpty) {
      throw Unsupported(
        'a generic super method used as a value',
        _sample(node),
      );
    }
    if (!_counted) {
      throw Unsupported(
        'a super method used as a value in a class with no handle',
        _sample(node),
      );
    }
    final torn = _staticType(node);
    DartType positionalType(int i) =>
        torn is FunctionType && i < torn.positionalParameters.length
        ? torn.positionalParameters[i]
        : fn.positionalParameters[i].type;
    DartType namedType(String name, DartType declared) {
      if (torn is FunctionType) {
        for (final n in torn.namedParameters) {
          if (n.name == name) return n.type;
        }
      }
      return declared;
    }

    final returnType = torn is FunctionType ? torn.returnType : fn.returnType;
    final params = [
      for (var i = 0; i < fn.positionalParameters.length; i++)
        IrParam(
          _paramName(fn.positionalParameters[i], 'a$i'),
          _type(positionalType(i)),
        ),
      for (final p in _namedInTypeOrder(fn))
        IrParam(
          p.parameterName,
          _type(namedType(p.parameterName, p.type)),
          named: true,
        ),
    ];
    final call = SuperMethodInvocation(
      ThisExpression(),
      node.name,
      Arguments(
        [for (final p in fn.positionalParameters) VariableGet(p)],
        named: [
          for (final p in fn.namedParameters)
            NamedExpression(p.parameterName, VariableGet(p)),
        ],
      ),
      target,
    );
    final tornReturns = _type(returnType);
    final lowered = expression(call);
    return IrClosure(
        params,
        IrReturn(coerce(lowered, tornReturns)),
        tornReturns,
        locals: const [],
        // Over a handle to `this`, as any closure calling into it is.
        holdsSelf: true,
      )
      ..rustType = IrType.function([
        for (final p in params) p.type,
      ], tornReturns);
  }

  /// The `Option<T>` a `T?` operand of `dart_as_own` is: a field's or a
  /// parameter's is the projected `<T as DartNullable>::Or` and goes
  /// through `option`; a local's is the `Option<T>` already (`arg as T`
  /// on a captured `T? arg`, the throttle fixture).
  IrExpr _asOwnOption(IrExpr operand, TypeParameterType parameter) {
    final held = operand.rustType;
    final name = parameter.parameter.name ?? 'T';
    if (held != null && held.projected) {
      return IrNullableOf(operand, name, toOption: true)
        ..rustType = IrType(name, nullable: true);
    }
    return operand;
  }

  Class? _realOwner(Member target, String name) {
    // A super call in a mixin's body names the `on` constraint's member
    // (`BindingBase.initInstances`), but dispatches to the *actual*
    // superclass of the application the body sits in: the walk starts
    // there, at the previous mixin in the chain (`RendererBinding`'s
    // `super.initInstances()` reaching `SemanticsBinding`'s, run438).
    final enclosing = _member?.enclosingClass;
    final fromApplication = enclosing?.isAnonymousMixin ?? false;
    var owner = fromApplication ? enclosing!.superclass : target.enclosingClass;
    while (owner != null) {
      if (owner.isAnonymousMixin) {
        // Not `mixedInClass`: with `--target=flutter` the CFE *applies*
        // the mixin, copying its members into this class and clearing
        // `mixedInType`, so that getter is null by the time a dill is
        // read. What survives is `implementedTypes` -- the applied
        // mixins, in the order they were written -- which is how `is
        // Scaled` still answers. Later mixins win, so the search runs
        // backwards.
        for (final applied in owner.implementedTypes.reversed) {
          final mixin = applied.classNode;
          // A hollow mixin declares the member when an application of it
          // holds the body (`_appliedBody`): `super.initInstances()` in
          // `WidgetsBinding` fell through every binding mixin to
          // `BindingBase`, and `SemanticsBinding.initInstances` never ran
          // (run438's `None` in `_semanticsEnabled`).
          if (mixin.members.any((m) => m.name.text == name && !m.isAbstract) ||
              _appliedProcedure(mixin, name) != null) {
            return mixin;
          }
        }
        owner = owner.superclass;
        continue;
      }
      // A real class: from an application's body, on up past the ones
      // that do not declare the member -- before *and* after the
      // anonymous applications in between (`RenderBox` for `attach`,
      // which `RenderObject` declares; `RenderSemanticsAnnotations`'
      // applied `super.describeSemanticsConfiguration` climbed
      // `RenderProxyBox`'s applications and stopped at `RenderBox`,
      // run575). A body of its own names the declaring class already.
      if (!fromApplication ||
          owner.members.any((m) => m.name.text == name && !m.isAbstract)) {
        return owner;
      }
      owner = owner.superclass;
    }
    return owner;
  }

  /// Whether a closure only *reads* fields of `this`.
  ///
  /// The line that matters in Rust: reading takes a shared borrow, and the
  /// method the closure is written in already holds one. Writing a field would
  /// want `&mut self` while `self` is borrowed for the call the closure is an
  /// argument to, and calling a method on `this` hands out the whole object.
  /// Both stay refused; 296 of the 1319 closures that reach `this` are on this
  /// side of the line, measured by `bin/census_closures.dart`.
  bool _onlyReadsThis(FunctionNode fn) {
    final use = _ThisUse();
    fn.accept(use);
    return !use.demanding;
  }

  /// Restores the Dart a `Let` was lowered from.
  ///
  /// `Let` is not a Dart construct -- it is the CFE's own temporary, and there
  /// are 14946 of them under `package:flutter`. Emitting the temporary as
  /// written would produce Rust nobody could read against upstream, which is
  /// the judgement round eight already made for operators: restore, do not
  /// transliterate.
  ///
  /// The shape here is `a ?? b`:
  ///
  ///     let final T #0 = a in #0 == null ? b : #0
  ///
  /// recognised by the else branch being the temporary itself. 6764 of the
  /// lets are this, 45% of them. The rest still stop -- `a?.b` is 4838 more
  /// and is the next shape, not this one.
  IrExpr _let(Let node) {
    final body = node.body;
    // A cascade: the binding is on the `Let` and the steps are a block whose
    // value is that binding. The standalone `BlockExpression` shape exists too,
    // and the probe that measured these looked only at *it* -- so this shape,
    // which is the one upstream actually produces, was missed until the fixture
    // compared the two front ends.
    if (body is BlockExpression && _isThe(body.value, node.variable)) {
      final initial = node.variable.initializer;
      if (initial == null) {
        throw Unsupported('cascade binding with no receiver', _sample(node));
      }
      final previous = _cascade;
      final previousStatic = _cascadeStatic;
      _cascade = node.variable;
      _cascadeStatic = _mutatedStaticOf(initial) ? expression(initial) : null;
      try {
        return IrBlockValue([
          if (_cascadeStatic == null)
            IrLocalDecl(
              _cascadeName,
              _type(node.variable.type),
              // Shared, not moved, when the receiver is a local (see the
              // other cascade site).
              // Into the binding's own type: TFA proves `size?.width` non-null
              // and the CFE's `#t` is still a `double?` (`Some(..)`).
              _widened(initial, node.variable.type, expression(initial)),
            ),
          for (final s in body.body.statements) statement(s),
        ], _cascadeRead())..rustType = _type(node.variable.type);
      } finally {
        _cascade = previous;
        _cascadeStatic = previousStatic;
      }
    }
    if (body is ConditionalExpression) {
      final condition = body.condition;
      final otherwise = body.otherwise;
      // `a?.b` -- the null branch is null and the other branch uses the
      // temporary. Recognised before `??` reads more naturally but the two are
      // disjoint: `??` has the temporary in the *else*, `?.` has null in the
      // *then*.
      if (condition is EqualsNull &&
          _isThe(condition.expression, node.variable) &&
          _isNull(body.then)) {
        final value = node.variable.initializer;
        if (value == null) {
          throw Unsupported('`?.` with no receiver', _sample(node));
        }
        // `null?.m` is `null`: type flow analysis folds an always-null
        // value into the literal, and walking the access from there left a
        // `None.as_ref().map(|it| ..)` whose closure parameter had no type
        // to be inferred from (8 "type annotations needed" at ws777). The
        // whole access is the null the receiver is.
        if (_isNull(value)) return _nullLiteral();
        final lowered = expression(value);
        // `x?.m` on a `dynamic` (an `Object?`, ws502): its null is the
        // `Null` object, asked by the prelude (`dart_nullable`), and the
        // value inside is what the body binds.
        final loweredType = lowered.rustType;
        final receiver =
            loweredType != null &&
                loweredType.name == 'dynamic' &&
                !loweredType.nullable
            ? (IrCall(lowered, '!nullable', const [])
                ..rustType = const IrType('dynamic', nullable: true))
            : lowered;
        final previous = _bound;
        final previousType = _boundType;
        _bound = node.variable;
        final receiverType = receiver.rustType;
        _boundType = receiverType == null
            ? null
            : IrType(receiverType.name, arguments: receiverType.arguments);
        try {
          // `oldLayer?._nativeLayer` with `_nativeLayer` a `T?`: one
          // `Option`, not two (8 `Option<Option<..>>` in dart:ui).
          final memberType = _staticType(otherwise);
          final body = expression(otherwise);
          // By the lowered body's own type where it has one: a cascade on
          // the bound (`child?..layout(..)`) is a `RenderBox` here whatever
          // the CFE's temporaries say (`RenderProxyBoxMixin.performLayout`,
          // ws485).
          final bodyType = body.rustType;
          // Typed as what the Rust value is -- the body's `Option` (one
          // layer, see `flatten`) -- not as Kernel's `T?`, which for a
          // `dynamic` body is a bare `dynamic` (`_imageStream?.key ==
          // key`, ws502).
          final flattened =
              bodyType != null &&
              bodyType.nullable &&
              bodyType.name != 'void' &&
              bodyType.name != '()';
          final IrType? resultType = bodyType == null
              ? null
              : flattened
              ? bodyType
              : _nullableIr(bodyType);
          return IrNullAware(
            receiver,
            body,
            // `void` is "nullable" to Kernel; `x?.addListener(..)` is a
            // `map`, not an `and_then` (`Option<_> <= ()`).
            // ..and a `T?` of a type parameter flattens too: `scope?.
            // localizationsState.resourcesFor<T?>(type)` is a `T?`, not an
            // `Option<Option<T>>` (`Localizations.of`, ws482).
            flatten: bodyType != null
                ? bodyType.nullable &&
                      bodyType.name != 'void' &&
                      bodyType.name != '()'
                : memberType != null &&
                      (memberType is InterfaceType ||
                          memberType is TypeParameterType) &&
                      memberType.nullability == Nullability.nullable,
          )..rustType = resultType;
        } finally {
          _boundType = previousType;
          _bound = previous;
        }
      }
      // `x!` -- the CFE writes it `let #0 = x in #0 == null ? #0 as T : #0`,
      // which is `??`'s shape with the temporary on *both* sides. Read as `??`
      // it took `#0 as T` for the right side and then met its own temporary
      // there with no name: 111 refusals reading `synthetic variable`, every
      // one an `x!` on a field.
      final then = body.then;
      if (condition is EqualsNull &&
          _isThe(condition.expression, node.variable) &&
          _isThe(otherwise, node.variable) &&
          then is AsExpression &&
          _isThe(then.operand, node.variable)) {
        final value = node.variable.initializer;
        if (value == null) {
          throw Unsupported('`!` with no operand', _sample(node));
        }
        // ..and `x as T` on a `T?` of the parameter's own, which the AOT
        // compiler writes in this same shape: the `T` inside, or `T`'s
        // own null where `T` has one (`RestorableValue<double?>.value`,
        // run665), by the prelude (`dart_as_own`).
        final asType = then.type;
        if (asType is TypeParameterType &&
            asType.nullability != Nullability.nullable &&
            !_erasedParameter(asType.parameter)) {
          final own = _type(
            asType.withDeclaredNullability(Nullability.nonNullable),
          );
          return IrStaticCall(
            null,
            'dart_as_own',
            [_asOwnOption(expression(value), asType)],
            fails: true,
            typeArguments: [own],
          )..rustType = own;
        }
        return _nullChecked(expression(value));
      }
      if (condition is EqualsNull &&
          _isThe(condition.expression, node.variable) &&
          _isThe(otherwise, node.variable)) {
        final value = node.variable.initializer;
        if (value == null) {
          throw Unsupported('`??` with no left side', _sample(node));
        }
        final right = body.then;
        // `locale ?? "unspecified"` inside a string: the two sides are of
        // different classes and the result is `Object`, so both go through
        // `dart_str` (6 `Option<Locale> <= String` shapes in dart:ui).
        final leftType = _staticType(value);
        final rightType = _staticType(right);
        // ..two *concrete* classes: a top-typed side (`Object?`, a
        // `dynamic`) takes the general path, where the other side goes
        // behind the handle (ws502).
        // ..and only *inside* a string, as the conditional's own rule is
        // (`_inStringPart` there): outside one the `Object` is an object,
        // and stringifying it put a `String` in a `ValueKey<Object>` and
        // in `InputDecorator`'s `label` (`KeyedSubtree.wrap`, ws876).
        if (_inStringPart &&
            leftType is InterfaceType &&
            rightType is InterfaceType &&
            leftType.classNode != rightType.classNode &&
            leftType.classNode.name != 'Object' &&
            rightType.classNode.name != 'Object' &&
            body.staticType is InterfaceType &&
            (body.staticType as InterfaceType).classNode.name == 'Object') {
          return IrIfNull(
            IrNullAware(
              expression(value),
              IrStaticCall(null, 'dart_str', [IrBound()]),
            ),
            IrStaticCall(null, 'dart_str', [expression(right)]),
            nullableResult: false,
            eager: false,
          );
        }
        // The right side into the left's type: `curve ?? Curves.ease` shares
        // its `Cubic` into the `Rc<dyn Curve>` -- with the target spelled
        // (`IrUpcast`), since a `match` arm does not coerce (267 "arms have
        // incompatible types" the round it was a bare `Rc::new`).
        // ..into the type the *result* has: `a ?? b` with a nullable `b`
        // stays nullable, and the arm is `Some(..)`, not `.unwrap()` (292
        // "arms have incompatible types" the round it was always non-null).
        final resultNullable =
            body.staticType.nullability == Nullability.nullable;
        // ..and into a function type: a static tear-off whose named
        // parameters are declared in another order than the type sorts
        // them takes its adapter here too (`requestFocusCallback ??
        // FocusTraversalPolicy.defaultTraversalRequestFocusCallback`,
        // run522).
        // ..the type of the *whole* -- Dart's least upper bound -- when the
        // left side is narrower than it: `widget?.notifier ?? fallback`
        // is a `ValueListenable` where `notifier` is a `ValueNotifier`
        // and `fallback` implements only the interface; into the left's
        // type the fallback had no `ValueNotifier` to become
        // (`TickerMode.getValuesNotifier`, run610). Both sides go into
        // it: the left below, mapped through its `Option`.
        final resultType = body.staticType;
        // ..a class with a handle to go up into: `double? ?? 0` is a
        // `num` in Dart and an `f64` here, where the literal takes the
        // left's spelling as before (+6 the round `num` was taken, ws611).
        // ..and `Object` itself is that type when the two sides are of
        // different classes: neither side is the other, and the whole is
        // the object both go behind (`child.key ?? childIndex` in
        // `KeyedSubtree.wrap`, ws876). Where they are of one class the
        // left's own spelling still wins, as it did.
        final differing =
            leftType is InterfaceType &&
            rightType is InterfaceType &&
            leftType.classNode != rightType.classNode;
        final lub =
            leftType is InterfaceType &&
                resultType is InterfaceType &&
                leftType.classNode != resultType.classNode &&
                (resultType.classNode.name != 'Object' || differing) &&
                !scalarNames.contains(resultType.classNode.name)
            ? resultType
            : leftType;
        final into = lub is InterfaceType || lub is FunctionType
            ? (resultNullable
                  ? lub!.withDeclaredNullability(Nullability.nullable)
                  : lub!.withDeclaredNullability(Nullability.nonNullable))
            : null;
        var rightSide = expression(right);
        if (into != null) {
          final widened = _widened(right, into, rightSide);
          rightSide = widened is IrCall && widened.name == '!rc'
              ? IrUpcast(widened.target!, _type(into))
              : widened;
        }
        // A closure as the right arm of a function-typed `??` is boxed:
        // the left arm is the `Rc<dyn Fn>` the slot holds, and a `match`
        // arm does not coerce (`onNavigationNotification ??
        // _defaultOnNavigationNotification`, `WidgetsApp.build`, ws503).
        if (rightSide is IrClosure &&
            (leftType is FunctionType || rightType is FunctionType)) {
          rightSide.boxed = true;
        }
        // `x ?? y` on a `dynamic` (an `Object?`, ws502): its null is the
        // `Null` object, asked by the prelude; and whether the result is
        // still an `Option` is the *Rust* type's answer -- a `dynamic`
        // result is no `Option`.
        // The left side as Dart types it: an erased read (`route.result`
        // on a `Route<int>` whose `T` is erased) is recorded wider, and is
        // narrowed where it is consumed, as a receiver or an argument is.
        var leftSide = expression(value);
        if (leftType is InterfaceType && leftType.classNode.name != 'Object') {
          try {
            leftSide = coerce(leftSide, _type(leftType));
            // ..and up into the whole's type when that is wider (see
            // `lub`), still nullable: the arm that is `None` stays so.
            if (!identical(lub, leftType) && lub is InterfaceType) {
              leftSide = coerce(
                leftSide,
                _type(lub.withDeclaredNullability(Nullability.nullable)),
              );
            }
          } on Unsupported {
            // Unspelled: as it is.
          }
        }
        final leftIr = leftSide.rustType;
        final asked =
            leftIr != null && leftIr.name == 'dynamic' && !leftIr.nullable
            ? (IrCall(leftSide, '!nullable', const [])
                ..rustType = const IrType('dynamic', nullable: true))
            : leftSide;
        final resultIr = _recordedType(body.staticType);
        // A `dynamic` whole whose right side is still an `Option` (a
        // projected `T?` with `T` bound to `dynamic`: `tween.end ??
        // tween.begin` on a `Tween<dynamic>`) is the handle, its null the
        // `Null` object -- the arms agree on that, not on `Option` versus
        // `Rc` (`_constructTweens`, ws614).
        final rightIr = rightSide.rustType;
        if (body.staticType is DynamicType &&
            resultIr != null &&
            !resultIr.nullable &&
            rightIr != null &&
            isNullable(rightIr)) {
          rightSide = IrStaticCall(null, 'dart_option_object', [rightSide])
            ..rustType = const IrType('dynamic');
        }
        final whole = IrIfNull(
          asked,
          rightSide,
          // Whether the whole thing is still nullable is the right side's
          // question: `a ?? b` is non-null exactly when `b` is.
          // The conditional carries its own static type, so no type context
          // has to be built to ask this.
          nullableResult: resultIr != null
              ? resultIr.nullable
              : body.staticType.nullability == Nullability.nullable,
          eager: right is BasicLiteral || right is ConstantExpression,
        );
        // The `??` says what it is where its two arms agree. Untyped, a
        // slot could not coerce it: `labelText ?? label` is an `Object` in
        // Dart and two `String`s here, and the `Rc<dyn Object>` it went
        // into never got the handle put on (`InputDecorator.build`,
        // `KeyedSubtree.wrap`; 4 at ws812). Only where they agree -- the
        // arms are what the block actually produces, whatever Dart calls
        // the whole.
        final leftArm = asked.rustType;
        final rightArm = rightSide.rustType;
        if (leftArm != null &&
            rightArm != null &&
            sameRust(nonNull(leftArm), nonNull(rightArm))) {
          whole.rustType = whole.nullableResult ? rightArm : nonNull(rightArm);
        }
        return whole;
      }
    }
    // Everything else is what a `Let` says it is: bind a name, then evaluate
    // the body with it in scope. Rust spells that a block expression, and it
    // needs no pattern recognised at all.
    //
    // The three shapes above are still tried first because they read like the
    // Dart that produced them and keep the two front ends agreeing. This is the
    // floor under them: 14476 `Let`s in `package:flutter/` are not any of the
    // three, and the largest group is simply the CFE binding a temporary for a
    // named argument -- `let #0 = radius * 2 in new CustomPaint(.., #0, ..)`.
    final initial = node.variable.initializer;
    if (initial == null) {
      // A `Let` with nothing to bind. Its body may still read the variable, and
      // there would be nothing to read.
      throw Unsupported('CFE `Let` with no initialiser', _sample(node));
    }
    // A temporary bound to a *place* -- a local, a static filled in place
    // -- that the body mutates in place (`let #t = local in #t.clear()`,
    // what TFA leaves of `local?.clear()` once `local` is known non-null,
    // the nullmut fixture): the body acts on the place, and nothing is
    // bound, as a cascade on one does.
    if (_isAliasablePlace(initial) &&
        _TempMutationFinder.mutates(node.variable, node.body)) {
      _letAliases[node.variable] = initial;
      try {
        return expression(node.body);
      } finally {
        _letAliases.remove(node.variable);
      }
    }
    final name = _nameFor(node.variable);
    // `alpha ?? a` after type flow analysis proved `alpha` non-null: the
    // conditional is gone and the body is the bound variable, *promoted*
    // to `double` while the binding is still `double?`. The unwrap is the
    // proof (the `{ let __t: Option<f64> = alpha; __t }` shapes).
    final letBody = node.body;
    final promotedRead =
        letBody is VariableGet &&
        letBody.variable == node.variable &&
        node.variable.type.nullability == Nullability.nullable &&
        letBody.promotedType != null &&
        letBody.promotedType!.nullability != Nullability.nullable;
    final block = IrBlockValue(
      [
        IrLocalDecl(
          name,
          // The post-increment's middle binding is `void` (see `_declare`).
          node.variable.type is VoidType ? null : _type(node.variable.type),
          // A local bound here is shared, not moved: `let __t = key;` and
          // `key` read again two lines on (13 E0382s). Into a `dynamic`
          // binding it is shared into the `Rc<dyn Object>` (`__t: Rc<dyn
          // Object> = true`).
          // ..and widened into the binding's type: `double? t = size?.height`
          // after TFA holds a `double`, and the binding says `Some`.
          _widened(initial, node.variable.type, expression(initial)),
        ),
      ],
      promotedRead
          ? _nullChecked(
              IrLocal(name)..rustType = _recordedType(node.variable.type),
            )
          : expression(letBody),
    );
    // Typed as its value, so a slot adapts the block as it would the
    // value: TFA's `let #t = channel in SystemChannels.menu` (the `??`
    // decided) into a `MethodChannel` field wants the handle (run483).
    final letValue = block.value;
    if (!promotedRead && letValue.rustType != null) {
      block.rustType = letValue.rustType;
    }
    return block;
  }

  /// A conditional in statement position as an `if` (see the expression
  /// statement lowering), through the `Let` the CFE binds its receiver
  /// in. Not the `?.` shape (`#t == null ? null : #t.m()`) nor the `??`
  /// one (`#t == null ? b : #t`): `_let` gives those their own forms
  /// (a null-aware call on a place, a `match`). Null for anything else.
  IrStmt? _conditionalStatement(Expression value) {
    if (value is ConditionalExpression) {
      final then = value.then;
      final otherwise = value.otherwise;
      // A throw anywhere in it -- TFA's "code removed" in a dead tail --
      // keeps the expression form, which spelled it (`_callPopInvoked`,
      // ws613).
      if (value.condition is Throw || then is Throw || otherwise is Throw) {
        return null;
      }
      // An arm that only reads (`#t_isSet ? #t : (#t_isSet = true, #t =
      // ..)`, the CFE's pattern cache) does nothing as a statement, and
      // as one it *moved* the temporary (`__t8;`, +13 at ws613).
      final thenPure = _pureRead(then);
      final otherwisePure = _pureRead(otherwise);
      if (thenPure && otherwisePure) return null;
      IrStmt arm(Expression e) => statement(ExpressionStatement(e));
      // An empty `then` is the other arm under the negated test, as one
      // would write it.
      if (thenPure) {
        return IrIf(
          IrUnary('!', expression(value.condition)),
          arm(otherwise),
          null,
        );
      }
      return IrIf(
        expression(value.condition),
        arm(then),
        otherwisePure ? null : arm(otherwise),
      );
    }
    if (value is Let) {
      final body = value.body;
      final initial = value.variable.initializer;
      if (body is! ConditionalExpression || initial == null) return null;
      final condition = body.condition;
      if (condition is EqualsNull &&
          _isThe(condition.expression, value.variable) &&
          (body.then is NullLiteral ||
              _isThe(body.otherwise, value.variable))) {
        return null;
      }
      if (_isAliasablePlace(initial) &&
          _TempMutationFinder.mutates(value.variable, body)) {
        return null;
      }
      final name = _nameFor(value.variable);
      final declared = IrLocalDecl(
        name,
        value.variable.type is VoidType ? null : _type(value.variable.type),
        _widened(initial, value.variable.type, expression(initial)),
      );
      final rest = _conditionalStatement(body);
      if (rest == null) return null;
      return IrBlock([declared, rest]);
    }
    return null;
  }

  /// An expression with no effect: a read, a literal, `this`.
  static bool _pureRead(Expression e) =>
      e is VariableGet ||
      e is BasicLiteral ||
      e is NullLiteral ||
      e is ConstantExpression ||
      e is ThisExpression;

  /// One local declaration, wherever it is written.
  ///
  /// A `for`'s variables are `VariableDeclaration`s and not `Statement`s in
  /// this Kernel, so they cannot go through `statement` -- and the rule about
  /// what a declaration becomes should be in one place regardless.
  /// Whether a member or parameter is the widget inspector's, not upstream's.
  ///
  /// A debug build runs the widget-creation-tracking transform, which gives
  /// `Widget` a `_location` field of type `CreationLocation` and its
  /// constructor a `$creationLocationd_<hash>` parameter. `Widget` is the base
  /// of nearly everything, so flattening copies that field into every widget
  /// and every widget constructor passes the argument -- 627 refusals for a
  /// const instance of a class that is not in the program at all.
  ///
  /// Dropped rather than translated, and said here rather than silently:
  /// this is the compiler's own instrumentation, not something anybody wrote.
  static bool _inspectorOnly(String name, [DartType? type]) {
    if (name.startsWith(r'$creationLocation')) return true;
    if (name != '_location') return false;
    return type is InterfaceType &&
        (type.classNode.name == 'CreationLocation' ||
            type.classNode.name == '_Location');
  }

  /// The receiver's static class, when it is a translated one.
  Class? _staticClass(Expression receiver) {
    final t = _staticType(receiver);
    return t is InterfaceType && _translatedClass(t.classNode)
        ? t.classNode
        : null;
  }

  String? _receiverClassName(Expression receiver) =>
      _staticClass(receiver)?.name;

  /// A Dart member's name on the Rust side: `clone` would shadow
  /// `Clone::clone`, which the backend calls on every value it shares
  /// (`Matrix4.clone()` gave every `.clone()` a `Result`, 179).
  static String _dartName(String name) => name == 'clone' ? 'clone_' : name;

  /// `a.b = v` as a statement.
  ///
  /// Its own method because a `return a.b = v;` in a void function is this
  /// statement and then a bare return -- the CFE writes `=> x = v` that way,
  /// 171 times in the gallery's dill, every one in a setter or a void closure.
  IrStmt _instanceSet(InstanceSet value) {
    // The value widens into the type the write lands on (`_writeSlot`): a
    // mixin clone's field, or the trait's setter -- `_cache = s` into a
    // `String?` field is `Some(s)`. A clone's field, being this struct's,
    // is written as a field, not through the trait's setter.
    final slot = _writeSlot(value.interfaceTarget, value.receiver);
    final landing = _landing(value.interfaceTarget, value.receiver);
    final declaredSlot = landing is Procedure && landing.isSetter
        ? landing.function.positionalParameters.single.type
        : landing.setterType;
    final written = _acrossBinding(
      _widened(
        value.value,
        slot,
        expression(value.value),
        slotIr: _writeSlotIr(value.interfaceTarget, value.receiver),
      ),
      declaredSlot,
      _bindingOf(declaredSlot, landing, value.receiver),
      toOption: false,
    );
    // A field on `this`, and a field rather than a setter. Kernel names the
    // target outright, so neither has to be inferred.
    // A write to the cascade's own binding: a local, so it needs a
    // mutable local rather than a mutable `self`.
    final receiver = value.receiver;
    if (_cascade != null &&
        receiver is VariableGet &&
        receiver.variable == _cascade) {
      if (value.interfaceTarget is! Field) {
        return IrSetter(
          _cascadeRead(),
          _fieldNameOf(value.interfaceTarget, value.name.text),
          written,
        );
      }
      // The owner is the cascaded value's own class: on a counted one its
      // fields are cells, and without the owner the backend wrote
      // `cascaded.on_down = ..` into an `Rc<RefCell<..>>` (23+23 in
      // `widgets`).
      return IrAssignField(
        _fieldNameOf(value.interfaceTarget, value.name.text),
        written,
        target: IrLocal(_cascadeName),
        owner:
            _receiverClassName(receiver) ??
            value.interfaceTarget.enclosingClass?.name,
      );
    }
    if (value.receiver is! ThisExpression) {
      // Another object's *setter* is a call, which needs nothing from us
      // beyond a `&mut` receiver at the call site.
      if (value.interfaceTarget is! Field) {
        return IrSetter(
          expression(value.receiver),
          value.name.text,
          written,
          qualifier: _setterQualifier(value.receiver, value.interfaceTarget),
          receiverClass: _classNameOf(value.receiver),
        );
      }
      // A *field* is a write through a reference. Through a chain rooted
      // at `this` -- `this.child.x = v` -- that reference is `self`, and
      // `&mut self` is a thing this compiler already works out. Through a
      // parameter it would mean `&mut` on the parameter and on every call
      // site, including ones in other files, so that one still stops.
      // A *local* that owns a value: `final entry = _ChildEntry(..);
      // entry.x = v;` is `let mut entry` and a plain field write in Rust,
      // with no reference in between and nothing for a call site to know.
      // Measured on 2026-09-03: 107 of the 296 refusals here were exactly
      // this. A local holding a counted class's handle is not this -- its
      // fields would have to be cells -- and a parameter is not either.
      final receiver = value.receiver;
      final receiverClassHere = _staticClass(receiver);
      // ..unless the local's own class is counted (its fields are cells)
      // or the field's class is a trait (its setter): `childParentData.
      // offset = Offset(..)` on a `_ToolbarParentData` local wrote to an
      // `Rc<Cell<Offset>>` (23 at ws325).
      if (receiver is VariableGet &&
          receiver.variable.parent is! FunctionNode &&
          !_closureCallsMethod(value.interfaceTarget.enclosingClass!) &&
          !(receiverClassHere != null &&
              _closureCallsMethod(receiverClassHere)) &&
          !_abstractLike(value.interfaceTarget.enclosingClass!)) {
        return IrAssignField(
          _fieldNameOf(value.interfaceTarget, value.name.text),
          written,
          target: expression(receiver),
          owner:
              receiverClassHere?.name ??
              value.interfaceTarget.enclosingClass?.name,
        );
      }
      // A local or a parameter holding a *counted* class's handle: every
      // non-final field of such a class is already a cell (the backend's
      // `_inCell`), so the write goes through the cell and needs no `&mut`
      // on anything. The owner rides on the node so the backend can find the
      // cell. 82 + 14 of the refusals here.
      final declaring = value.interfaceTarget.enclosingClass!;
      // ..and reached however it was reached: `_views[viewId]!.x = v` is a
      // handle out of a map, and the write goes through the cell just the
      // same (`PlatformDispatcher`, 1 refusal that took 3 callers).
      final receiverClass = _staticClass(receiver);
      if (_closureCallsMethod(declaring) ||
          (receiverClass != null && _closureCallsMethod(receiverClass))) {
        // The receiver's own class, where the cells are decided; the
        // declaring one may be an abstract base.
        return IrAssignField(
          _fieldNameOf(value.interfaceTarget, value.name.text),
          written,
          target: expression(receiver),
          owner: receiverClass?.name ?? declaring.name,
        );
      }
      // A trait's field on a value local: the setter the trait declares
      // (`IrAssignField.owner` abstract; 19 refusals at ws326).
      if (_abstractLike(declaring)) {
        return IrAssignField(
          _fieldNameOf(value.interfaceTarget, value.name.text),
          written,
          target: expression(receiver),
          owner: declaring.name,
        );
      }
      // A *static* holding a value: `GoogleFonts.config.allowRuntimeFetching
      // = false` writes a field of the value in the static's cell, which
      // makes that static mutable state (the driver flips its cell on:
      // `staticFieldWrites`). The first refusal on the gallery's startup
      // path, in `main` itself (2026-09-05).
      if (receiver is StaticGet &&
          receiver.target is Field &&
          !_closureCallsMethod(declaring) &&
          !_abstractLike(declaring)) {
        final place = expression(receiver);
        if (place is IrStatic || place is IrTopLevel) {
          staticFieldWrites.add(
            place is IrStatic
                ? '${place.owner}.${place.name}'
                : '.${(place as IrTopLevel).name}',
          );
          return IrAssignField(
            _fieldNameOf(value.interfaceTarget, value.name.text),
            written,
            target: place,
            owner: receiverClassHere?.name ?? declaring.name,
          );
        }
      }
      if (!_rootedAtThis(value.receiver)) {
        throw Unsupported(
          'assignment to a field of another object '
          '(${_shape(value.receiver)}, '
          '${_closureCallsMethod(value.interfaceTarget.enclosingClass!) ? "counted" : "value"})',
          _sample(value),
        );
      }
      return IrAssignField(
        _fieldNameOf(value.interfaceTarget, value.name.text),
        written,
        target: expression(value.receiver),
      );
    }
    if (value.interfaceTarget is! Field &&
        !_heldField(value.interfaceTarget, value.receiver)) {
      return IrSetter(
        null,
        _fieldNameOf(value.interfaceTarget, value.name.text),
        written,
        qualifier: _setterQualifier(null, value.interfaceTarget),
      );
    }
    return IrAssignField(
      _fieldNameOf(value.interfaceTarget, value.name.text),
      written,
    );
  }

  IrStmt _declare(Variable variable, Node at) {
    final init = variable.initializer;
    if (init is InstanceGet && init.name.text == 'iterator') {
      // Remembered, not lowered: if the loop below it is the CFE's `for-in`,
      // this binding is part of that shape and the restored loop names the
      // iterable itself.
      _iterators[variable] = init.receiver;
      // Declared as well as remembered: a loop the restoration recognises
      // ignores this binding, and a hand-driven one -- `final it =
      // xs.iterator; while (it.moveNext()) ..`, `equality.dart` -- needs it.
      // Swallowed, it left `iterator.move_next()` on nothing.
      final written = variable.cosmeticName;
      final name =
          (written == null ||
              written.startsWith('#') ||
              written.startsWith(':'))
          ? _nameFor(variable)
          : written;
      return IrLocalDecl(
        name,
        null,
        // A clone: the iterator owns its items, and the list is a field
        // behind `&self` more often than not (`self._children`, E0507).
        IrStaticCall(null, 'dart_iter', [
          IrCall(_listReceiver(init.receiver), 'clone', const []),
        ]),
      );
    }
    final written = variable.cosmeticName;
    // A temporary the CFE invented. It used to be refused, on the grounds that
    // translating one means translating the lowering it belongs to -- but that
    // was only true while there was nothing to call it. `_nameFor` gives it a
    // name, `VariableGet` finds that name again, and the lowering it belongs to
    // is then just the statements around it.
    final name = (written == null || written.startsWith('#'))
        ? _nameFor(variable)
        : written;
    // ..and a `late` local without an initialiser: assigned on some path
    // and read on another rustc cannot match up (`late Rect
    // floatingActionButtonRect` in `Scaffold`'s layout, E0381, ws527).
    if (init == null &&
        written != null &&
        (written.startsWith('#') ||
            _tryWrites.contains(variable) ||
            variable.isLate) &&
        variable.type is! VoidType &&
        variable.type.nullability != Nullability.nullable) {
      _optionLocals.add(variable);
      return IrLocalDecl(
        name,
        _localIrType(variable),
        _nullLiteral(),
        cell: _capturedWrites.contains(variable),
      );
    }
    // A local of a nullable type declared without an initializer holds
    // Dart's null from the start: `None`, or the `Null` object for a
    // `dynamic`. Left uninitialized, a read Dart guards with its own flag
    // (a pattern's `#0#2` behind `#0#2#isSet`) is one rustc cannot see
    // assigned (E0381, run454).
    // ..by the *Rust* type: an `Object?` is a `dynamic` (ws501).
    final type = variable.type;
    final IrType? startType = init != null || type is VoidType
        ? null
        : _recordedType(type);
    final IrExpr? nullStart = startType == null
        ? null
        : startType.name == 'dynamic' && !startType.nullable
        ? (IrStaticCall(null, 'dart_null_object', const [])
            ..rustType = startType)
        : startType.nullable
        ? IrLiteral('None', const IrType('raw'))
        : null;
    if (variable.type is FunctionType &&
        (init is FunctionExpression || init is InstanceTearOff)) {
      _boxedFunctionLocals.add(name);
    }
    return IrLocalDecl(
      name,
      // `void` is what the CFE gives the temporary of a post-increment whose
      // value is unused, and `let __t: () = { ..; __set }` then held an
      // `i64` (53 `() <= i64`). Unannotated, Rust infers what it holds.
      variable.type is VoidType ? null : _type(variable.type),
      // Into the declared type: `Int32List? x = encode(..)` is `Some(..)`;
      // `num divisor = pow(10, n).round()` casts the `int`.
      // ..and a `dynamic` local holding a scalar or struct shares it
      // (`var integer = number.floor()` on a `dynamic` number).
      init == null
          ? nullStart
          : _intoDeclaredNum(
              init,
              variable.type,
              _widened(init, variable.type, expression(init)),
            ),
      cell: _capturedWrites.contains(variable),
    );
  }

  /// A name for one of the CFE's temporaries.
  ///
  /// They are called `#0`, `#1` and so on, and the numbering restarts, so two
  /// nested `Let`s can both be `#0`. Rust would take the inner one as shadowing
  /// the outer, which is what Dart means too -- but the backend snakes names,
  /// and `#` is not a character it can carry. So each variable gets its own
  /// name, kept in a map by identity rather than by text.
  final _temporaries = <Variable, String>{};
  var _nextTemporary = 0;

  /// The CFE's value half of a lowered `late` local: declared without an
  /// initialiser and assigned under its `#isSet` flag, which Rust's
  /// definite-assignment check cannot follow (129 E0381 at ws397). It is
  /// what a `late` local is in Rust terms, an `Option`: `None` until set,
  /// read as `T?` (the coercion rule unwraps where a `T` goes), written
  /// with `Some`.
  final _optionLocals = <Variable>{};

  /// The element type of the receiver `_bound` stands for (see the `?.`
  /// lowering), for narrowing a read of it.
  IrType? _boundType;

  /// Set while a string interpolation's part is lowered.
  bool _inStringPart = false;

  DartType _localType(Variable v) => _optionLocals.contains(v)
      ? v.type.withDeclaredNullability(Nullability.nullable)
      : v.type;

  /// A local's Rust type: an option local's is the `Option` of its
  /// declared type's, whatever Dart's nullable spelling of that type maps
  /// to (`late Object x` is an `Option<Rc<dyn Object>>`, where `Object?`
  /// itself is a `dynamic`, ws497).
  IrType _localIrType(Variable v) {
    final declared = _type(v.type);
    return _optionLocals.contains(v) ? _nullableIr(declared) : declared;
  }

  static IrType _nullableIr(IrType t) {
    if (t.nullable) return t;
    if (t.isFunction)
      return IrType.function(t.parameters!, t.returns!, nullable: true);
    return IrType(
      t.name,
      nullable: true,
      arguments: t.arguments,
      projected: t.projected,
    );
  }

  /// The argument slots of a member on a *typed list* receiver (`Vec<u8>`
  /// here): the element slots take the list's narrow element, not Dart's
  /// `int` -- `bytes.setRange(a, b, other)` on two `Uint8List`s handed the
  /// prelude a `Vec<i64>` (`WriteBuffer._append`, run505). Null for any
  /// other receiver or slot.
  List<IrType?>? _narrowSlots(InstanceInvocation node) {
    final narrow = _narrowElement(_staticType(node.receiver));
    final declaring = node.interfaceTarget.enclosingClass;
    final fn = node.interfaceTarget.function;
    if (narrow == null ||
        declaring == null ||
        fn == null ||
        declaring.enclosingLibrary.importUri.scheme != 'dart') {
      return null;
    }
    IrType? slot(DartType t) {
      if (t is TypeParameterType &&
          declaring.typeParameters.contains(t.parameter)) {
        return IrType(narrow);
      }
      if (t is InterfaceType &&
          (t.classNode.name == 'Iterable' || t.classNode.name == 'List') &&
          t.typeArguments.length == 1) {
        final e = t.typeArguments.single;
        if (e is TypeParameterType &&
            declaring.typeParameters.contains(e.parameter)) {
          return IrType('List', arguments: [IrType(narrow)]);
        }
      }
      return null;
    }

    final slots = [for (final p in fn.positionalParameters) slot(p.type)];
    return slots.any((s) => s != null) ? slots : null;
  }

  /// Whether `c` is one of `dart:typed_data`'s lists (`Uint8List`,
  /// `Float64List`, ..): a `Vec` of its element here.
  static bool _typedList(Class c) =>
      c.enclosingLibrary.importUri.toString() == 'dart:typed_data' &&
      c.name.endsWith('List') &&
      !c.name.startsWith('_');

  /// The Rust element type of a typed list narrower than Dart's `double`
  /// and `int`, or null for anything else.
  static String? _narrowElement(DartType? type) {
    if (type is! InterfaceType) return null;
    return const {
      'Float32List': 'f32',
      'Int8List': 'i8',
      'Int16List': 'i16',
      'Int32List': 'i32',
      'Uint8List': 'u8',
      'Uint8ClampedList': 'u8',
      'Uint16List': 'u16',
      'Uint32List': 'u32',
    }[type.classNode.name];
  }

  /// A parameter's declared default, lowered -- or null when it has none or
  /// the default is not a shape this front end lowers.
  IrExpr? _default(FunctionParameter p) {
    final value = p.defaultValue;
    if (value == null) return null;
    try {
      return expression(value);
    } on Unsupported {
      return null;
    }
  }

  /// A parameter's name for the backend.
  ///
  /// The CFE gives its own parameters names no human wrote --
  /// `#externalFieldValue` on an external field's setter, `#typedDataBase` on
  /// a `Struct` constructor -- and `#` is not a character the backend can
  /// carry. Those get the same `__tN` a temporary gets, by identity, and
  /// `VariableGet` finds it again the same way. 128 refusals were these.
  String _paramName(Variable p, [String? fallback]) {
    final written = p.cosmeticName;
    // A parameter with no written name still has to be *nameable*: a super
    // forwarder passes it on by name, and `_` is a pattern in Rust, not a
    // value -- `super_set_first(self, _)` did not parse.
    if (written == null) return fallback ?? _nameFor(p);
    if (written.startsWith('#') || written == '_') return _nameFor(p);
    // Once renamed, always renamed: reads find the name through
    // `_temporaries` by identity, and a nested closure lowering under its
    // own captured set would otherwise name the same parameter twice.
    final already = _temporaries[p];
    if (already != null) return already;
    // A parameter of a closure that copied a field of `this` in under the
    // field's own name (`IrClosure.captures`): in Rust the parameter
    // shadows the copy, so the body's read of the *field* found the
    // parameter instead. Dart has no such collision -- there the field is
    // `this.child` -- so the copy keeps the name the reads use and the
    // parameter takes a temporary's (`this.child ?? child` in the gallery's
    // `FadeInImagePlaceholder.build`, ws808).
    if (_captured.contains(written)) return _nameFor(p);
    return written;
  }

  String _nameFor(Variable variable) =>
      _temporaries[variable] ??= '__t${_nextTemporary++}';

  /// A Rust label for one of Kernel's labelled statements.
  ///
  /// Kept by identity, like the temporaries, because a labelled statement has
  /// no name of its own -- a `break` points at the node.
  final _labels = <LabeledStatement, String>{};
  var _nextLabel = 0;

  String _labelFor(LabeledStatement node) =>
      _labels[node] ??= '__l${_nextLabel++}';

  /// Labels the CFE put there to spell `continue` and `break`.
  ///
  /// `continue` is a `break` out of a label wrapped around the loop *body*, and
  /// `break` is a `break` out of one wrapped around the loop itself. Both are
  /// the CFE saying in its own words something Dart already had a word for, and
  /// the analyzer front end sees the word -- so these are restored rather than
  /// carried across as labelled blocks.
  final _continueTargets = <LabeledStatement>{};

  /// Labels a switch's `break` points at, and the breaks that may be dropped --
  /// the last statement of a case body, and only that one.
  final _switchBreaks = <LabeledStatement>{};
  final _droppableBreaks = <BreakStatement>{};

  /// A case body, with its trailing `break` marked as droppable -- the
  /// trailing statement through nested blocks (`{ { switch (..) {..}
  /// break #L; } }`, a nested switch's case).
  IrStmt _caseBody(Statement body) {
    final last = _trailingStatement(body);
    if (last is BreakStatement) _droppableBreaks.add(last);
    return statement(body);
  }

  static Statement _trailingStatement(Statement body) {
    var last = body;
    while (last is Block && last.statements.isNotEmpty) {
      last = last.statements.last;
    }
    return last;
  }

  /// Switches whose `break`s leave a labelled block around the match.
  final _labeledSwitches = <LabeledStatement, String>{};

  /// Labelled statements a `break` should leave, and the Rust label to use --
  /// null when a bare `break` will do.
  final _breakTargets = <LabeledStatement, String?>{};

  /// The label to put on the loop about to be lowered, if it needs one.
  String? _loopLabel;

  /// `for (final x in xs)`, put back together.
  ///
  /// The CFE writes it as: bind `#0 = xs.iterator`, then `for (; #0.moveNext();)`
  /// with `final x = #0.current` as the body's first statement. The binding is
  /// a *sibling* of the loop, so it is spotted here from the loop's condition
  /// and the loop's body, and the binding statement is dropped by the block
  /// that holds it. Returns null when the shape is anything else.
  IrStmt? _forIn(ForStatement node) {
    if (node.variables.isNotEmpty || node.updates.isNotEmpty) return null;
    return _restoreForIn(node.condition, node.body);
  }

  /// The same loop written with `while`: the CFE's other spelling of
  /// `for (x in xs)` -- `while (:sync-for-iterator.moveNext())` -- which the
  /// `for (;;)` restoration never saw. The iterator binding above it had
  /// already been swallowed as "part of that shape", so the loop that came
  /// out named a variable nothing declared: 6 `_sync_for_iterator`s.
  IrStmt? _forInWhile(WhileStatement node) =>
      _restoreForIn(node.condition, node.body);

  IrStmt? _restoreForIn(Expression? condition, Statement body0) {
    if (condition is! InstanceInvocation || condition.name.text != 'moveNext') {
      return null;
    }
    final receiver = condition.receiver;
    if (receiver is! VariableGet) return null;
    final iterable = _iterators[receiver.variable];
    if (iterable == null) return null;

    var body = body0;
    if (body is LabeledStatement) body = body.body;
    if (body is! Block || body.statements.isEmpty) return null;
    final first = body.statements.first;
    if (first is! VariableStatement) {
      // No `x = it.current` at the top: the body reads `.current` where it
      // needs it. The element gets a name here and `_instanceGet` hands the
      // reads that name (see `_currentOf`). Without this the declaration
      // was swallowed above and the loop below named a variable that was
      // never declared -- `_sync_for_iterator`, 6 times.
      final element = '__t${_nextTemporary++}';
      _currentOf[receiver.variable] = element;
      _iteratorLoops.add(receiver.variable);
      return IrForIn(
        element,
        _listReceiver(iterable),
        IrBlock([for (final s in body.statements) statement(s)]),
      );
    }
    final initial = first.declaration.variable.initializer;
    if (initial is! InstanceGet ||
        initial.name.text != 'current' ||
        !(initial.receiver is VariableGet &&
            identical(
              (initial.receiver as VariableGet).variable,
              receiver.variable,
            ))) {
      return null;
    }
    // The element's name is the binding's, or one of this front end's own
    // when the CFE gave it none -- a `for ((a, b) in pairs)` binds `#0`.
    final written = first.declaration.variable.cosmeticName;
    final name = (written == null || written.startsWith('#'))
        ? _nameFor(first.declaration.variable)
        : written;
    _iteratorLoops.add(receiver.variable);
    return IrForIn(
      name,
      _listReceiver(iterable),
      IrBlock([for (final s in body.statements.skip(1)) statement(s)]),
    );
  }

  /// Temporaries bound to `<something>.iterator`, and the ones a restored
  /// `for-in` has consumed -- whose binding statement must then not be emitted.
  final _iterators = <Variable, Expression>{};
  final _iteratorLoops = <Variable>{};

  /// The element name standing in for `it.current` inside a restored loop
  /// whose body did not bind it first.
  final _currentOf = <Variable, String>{};

  IrStmt _loopBody(Statement body, bool hasUpdates) {
    if (body is! LabeledStatement) return statement(body);
    // ..and when the body holds a switch that leaves early through a
    // labelled block (see the labelled-switch lowering), Rust wants the
    // `continue` labelled too (E0695): the body is the labelled block the
    // CFE wrote, and the continue breaks out of it (`Navigator.
    // _flushHistoryUpdates`, ws582).
    if (hasUpdates || _holdsEarlySwitch(body.body)) {
      return IrLabeled(_labelFor(body), statement(body.body));
    }
    _continueTargets.add(body);
    return statement(body.body);
  }

  static bool _holdsEarlySwitch(Statement body) {
    final finder = _EarlySwitchFinder();
    body.accept(finder);
    return finder.found;
  }

  // A `Let`'s variable is a `SyntheticVariable`, not a `VariableDeclaration`:
  // the CFE made it, so it has no declaration to point at.
  bool _isThe(Expression e, Variable variable) =>
      e is VariableGet && e.variable == variable;

  bool _isNull(Expression e) =>
      e is NullLiteral ||
      (e is ConstantExpression && e.constant is NullConstant);

  /// The temporary the enclosing `?.` bound, if any. Reads of it become
  /// [IrBound] so the backend can name it as a closure parameter.
  Variable? _bound;

  String _sample(Node node) {
    final text = node.toString().replaceAll('\n', ' ');
    return text.length > 90 ? '${text.substring(0, 90)}...' : text;
  }

  /// A field read, or a getter call -- and the difference matters in Rust.
  ///
  /// Dart spells both `a.x`. Rust spells a field `a.x` and a getter `a.x()`,
  /// and getting it wrong does not compile: `_x` on `AlignmentGeometry` is an
  /// abstract getter, so it becomes a trait method, and reading it as a field
  /// gives "attempted to take value of method `_x`".
  ///
  /// Kernel says which it is outright -- the target is a `Field` or a
  /// `Procedure` -- so nothing has to be inferred.
  /// `Map` and the `dart:collection` classes the prelude's `Map` stands
  /// for: `SplayTreeMap<double, String>` in `AssetImage` took the generic
  /// path and asked for `index_of` (50).
  static bool _isMapClass(String? owner) =>
      const {'Map', 'LinkedHashMap', 'HashMap', 'SplayTreeMap'}.contains(owner);

  IrExpr _instanceGet(InstanceGet node) => _instanceGetRaw(node);

  /// A member access's receiver, as Dart types it: a value whose recorded
  /// Rust type is wider -- an erased read, `Rc<dyn StatefulWidget>` where
  /// Dart says `Scaffold` -- is narrowed on the way in (`coerce`), which is
  /// what the erased-read narrowing used to do at every such read whether
  /// or not a member was then reached through it.
  /// A receiver asked as a list -- `Iterable`'s members on it, a `for-in`
  /// over it -- when it is a translated class that *is* an `Iterable<E>`
  /// (`Navigator`'s `_History extends Iterable<_RouteEntry>`, ws499): the
  /// list of its elements, `to_list`, which the backend writes from the
  /// class's `iterator`. Any other receiver is itself.

  IrExpr _listReceiver(Expression e, [String? member]) {
    // A mutating member's receiver as it is: an erased `List<ChildType>`
    // read as a `List<Sliver>` is a narrowing *copy*, and `children.add
    // (x)` pushed into it (the erased tear-off fixture, ws528). The element
    // goes in as the slot's type and rustc upcasts it to the erased one.
    if (member != null && mutatingListNames.contains(member)) {
      return expression(e);
    }
    final lowered = _receiver(e);
    final static = _staticType(e);
    if (static is! InterfaceType || !_translatedClass(static.classNode)) {
      return lowered;
    }
    final element = _iterableElement(static);
    if (element == null) return lowered;
    return IrCall(lowered, '__to_list', const [])
      ..rustType = IrType('List', arguments: [_typeNested(element)]);
  }

  /// `IrClass.iterableElement`: the class's own `Iterable<E>` element.
  IrType? _iterableElementIr(Class node) {
    final env = typeEnvironment;
    if (env == null || !_translatedClass(node)) return null;
    final element = _iterableElement(
      node.getThisType(env.coreTypes, Nullability.nonNullable),
    );
    if (element == null) return null;
    try {
      return _typeNested(element);
    } on Unsupported {
      return null;
    }
  }

  /// The `E` of the `Iterable<E>` a translated class is, or null.
  DartType? _iterableElement(InterfaceType type) {
    final env = typeEnvironment;
    if (env == null) return null;
    final iterable = env.coreTypes.iterableClass;
    if (identical(type.classNode, iterable)) return null;
    final asIterable = env.hierarchy.getTypeAsInstanceOf(type, iterable);
    if (asIterable is! InterfaceType || asIterable.typeArguments.length != 1) {
      return null;
    }
    return asIterable.typeArguments.single;
  }

  IrExpr _receiver(Expression e) {
    final lowered = expression(e);
    var static = _staticType(e);
    // A receiver typed by a type parameter is its bound here (`_type`
    // says so too): `widget.duration` in `ImplicitlyAnimatedWidgetState<T
    // extends ImplicitlyAnimatedWidget>` reads `State<T>.widget` as the
    // `Rc<dyn StatefulWidget>` the trait returns, and is narrowed to the
    // `ImplicitlyAnimatedWidget` this class's `T` promises (`AnimatedTheme`
    // in `MaterialApp`, ws538).
    // ..only to a translated abstract bound, whose trait the member is
    // reached through. To `Object` there is nothing to narrow to, and
    // boxing `key` for `key.hashCode` moved it and hashed the box (the
    // hashtrie fixture, ws544).
    var hops = 0;
    while (static is TypeParameterType && hops++ < 8) {
      final bound = static.parameter.bound;
      if (bound is InterfaceType &&
          !(_translatedClass(bound.classNode) &&
              _abstractLike(bound.classNode))) {
        return lowered;
      }
      static = bound;
    }
    if (!coerceByType || static is! InterfaceType) return lowered;
    try {
      final out = coerce(lowered, _type(static));
      return out;
    } on Unsupported {
      return lowered;
    }
  }

  IrExpr _instanceGetRaw(InstanceGet node) {
    final name = _fieldNameOf(node.interfaceTarget, node.name.text);
    final listOwner = node.interfaceTarget.enclosingClass?.name;
    if (listOwner == 'List' || listOwner == 'Iterable') {
      final rust = listMethodNames[name];
      if (rust == null) throw Unsupported('`List.$name`', _sample(node));
      // A getter in Dart, a method in Rust: `xs.length` is `xs.len()`.
      return IrCall(_listReceiver(node.receiver), rust, const []);
    }
    if (_isMapClass(listOwner)) {
      if (orderedMapMembers.contains(name)) {
        throw Unsupported(
          '`Map.$name`, which depends on insertion order',
          _sample(node),
        );
      }
      final rust = mapMethodNames[name];
      if (rust == null) throw Unsupported('`Map.$name`', _sample(node));
      return IrCall(_receiver(node.receiver), rust, const []);
    }
    final receiver = node.receiver;
    // A field the enclosing closure copied in is a local now, not a field of
    // a `this` the closure does not hold. See `IrClosure.captures`.
    if (receiver is ThisExpression && _captured.contains(name)) {
      return IrLocal(name);
    }
    if (name == 'current' && receiver is VariableGet) {
      final element = _currentOf[receiver.variable];
      if (element != null) return IrLocal(element);
    }
    final target = receiver is ThisExpression ? null : _receiver(receiver);
    // A getter whose landing member is a field this class holds (a mixin
    // clone's) is read as the field, typed as the clone declares it -- as
    // a write to it is stored (`_instanceSet`). Through the trait's getter
    // it came back as the erased bound (`_slotToChild`, ws373).
    if (node.interfaceTarget is Procedure &&
        !_heldField(node.interfaceTarget, receiver)) {
      final declared = node.interfaceTarget.getterType;
      return _acrossBinding(
        _qualified(
          IrCall(target, name, const []),
          node.interfaceTarget,
          receiver,
        ),
        declared,
        _bindingOf(declared, node.interfaceTarget, receiver),
        toOption: true,
      );
    }
    // A field of a `dart:` class the prelude re-expresses -- `Duration
    // .inMicroseconds` is a field in this SDK -- is a method there, as
    // every getter of such a class is.
    // Only where the prelude spells them as methods: `MapEntry.key` and
    // `SocketException.message` are fields there (11 E0599s when every
    // `dart:` class took this path, ws136).
    final owner = node.interfaceTarget.enclosingClass;
    if (owner != null &&
        owner.enclosingLibrary.importUri.scheme == 'dart' &&
        const {'Duration', 'DateTime'}.contains(owner.name)) {
      return IrCall(target, name, const []);
    }
    // A field declared on an *abstract* class is a trait accessor in Rust,
    // and a read through another object -- whatever its concrete class
    // stores -- goes through the accessor: `rc.x` took the value of a
    // method, 111 times in the leaf crates.
    final declaring = node.interfaceTarget.enclosingClass;
    // Not an anonymous mixin application: it is abstract to Kernel, and its
    // fields are flattened into the applying class's struct here
    // (`Get.isLogEnable` read as a getter call on a field, E0599).
    // ..unless the receiver's own static class is concrete: the struct has
    // the abstract base's field flattened in, and the field is read as one
    // (`Get.isLogEnable` on a `_GetImpl`, whose trait was not even in scope).
    // The class the receiver *is here*: a closure parameter retyped to an
    // erased bound reads back through a cast to the class it was declared
    // with (`_localRead`), whatever the type flow analysis narrowed the
    // static type to. Narrowed to a concrete subclass, the read came out
    // as a field access on a value that is still a trait object
    // (`notification.metrics` on a `dyn ScrollNotification`, ws721).
    final receiverType = target == null
        ? null
        // ..and through `_appliedBack`: inside a mixin's body borrowed from
        // an application the copy says `FlexParentData` where the trait
        // holds the erased bound, and a field read on a `dyn` is the
        // accessor (ws735).
        : _backHere(
            receiver is VariableGet && _retyped.containsKey(receiver.variable)
                ? receiver.variable.type
                : _staticType(receiver),
          );
    final concrete =
        receiverType is InterfaceType &&
        !_abstractLike(receiverType.classNode) &&
        receiverType.classNode.enclosingLibrary.importUri.scheme != 'dart';
    if (target != null &&
        declaring != null &&
        _abstractLike(declaring) &&
        !declaring.isAnonymousMixin &&
        !concrete) {
      final declared = node.interfaceTarget.getterType;
      return _acrossBinding(
        _qualified(
          IrCall(target, name, const []),
          node.interfaceTarget,
          receiver,
        ),
        declared,
        _bindingOf(declared, node.interfaceTarget, receiver),
        toOption: true,
      );
    }
    final read =
        IrField(
            target,
            name,
            // `PerformanceOverlayOption.x.index` in a static initialiser resolves
            // to `_Enum.index`, a field of a class that is not an enum; the
            // receiver's own type says it is one (4 "attempted to take value of
            // method `index`" in `rendering`).
            onEnum:
                (node.interfaceTarget.enclosingClass?.isEnum ?? false) ||
                (receiverType is InterfaceType &&
                    receiverType.classNode.isEnum),
            owner: target == null
                ? null
                : concrete && declaring != null && _abstractLike(declaring)
                ? (receiverType as InterfaceType).classNode.name
                : node.interfaceTarget.enclosingClass?.name,
          )
          ..rustType = _memberRustType(
            _landing(node.interfaceTarget, receiver),
            receiver,
            asGetter: true,
          );
    // Out of a projected field: into the `Option<T>` the body works with.
    final declared = node.interfaceTarget.getterType;
    return _acrossBinding(
      read,
      declared,
      _bindingOf(declared, node.interfaceTarget, receiver),
      toOption: true,
    );
  }

  /// `dateTimeSymbols[k]`, `.containsKey(k)`, `.keys` on a `dynamic` slot
  /// with known types (see `dynamicSlots`): one arm per type, each giving
  /// the *same* Rust type -- what the Dart code does with the result is
  /// typed by the first arm's member. A `Map` arm answers as a map; a class
  /// arm calls its own member, or panics for one it does not have (Dart's
  /// `NoSuchMethodError`, which is what `UninitializedLocaleData` does).
  IrExpr? _dynamicSlotCall(Expression node) {
    final Expression receiver;
    final String name;
    final List<Expression> positional;
    if (node is DynamicInvocation) {
      receiver = node.receiver;
      name = node.name.text;
      positional = node.arguments.positional;
    } else if (node is DynamicGet) {
      receiver = node.receiver;
      name = node.name.text;
      positional = const [];
    } else {
      return null;
    }
    if (receiver is! StaticGet) return null;
    var target = receiver.target;
    // Through a getter that only reads the slot: `dynamic get
    // dateTimeSymbols => _dateTimeSymbols`.
    if (target is Procedure && target.isGetter) {
      final body = target.function.body;
      final read = body is ReturnStatement ? body.expression : null;
      if (read is StaticGet) target = read.target;
    }
    if (target is! Field) return null;
    final candidates = dynamicSlots[target];
    if (candidates == null || candidates.isEmpty) return null;
    const known = {'[]', '[]=', 'containsKey', 'keys'};
    if (!known.contains(name)) return null;
    // A local handed in is shared, as an argument is (`_clonedWhenPassed`):
    // two dispatches in a row moved the key into the first.
    final args = [
      for (final e in positional)
        () {
          final lowered = expression(e);
          return lowered is IrLocal
              ? (IrCall(lowered, 'clone', const [])
                  ..rustType = lowered.rustType)
              : lowered;
        }(),
    ];
    final slot = IrLocal('__d');
    // A write into the slot's map (`dateTimeSymbols[locale] = symbols`,
    // intl's `initializeDateFormattingCustom`, run585): the map is a
    // value behind the slot's handle, so the copy the arm holds is
    // written and put back as the slot's object.
    final field = target;
    IrExpr noSuch() => IrLiteral(
      'panic!("uncaught Dart exception: NoSuchMethodError: `$name` on an ${candidates.first.classNode.name}")',
      const IrType('raw'),
    );
    final arms = <(IrType?, IrExpr)>[];
    for (final c in candidates) {
      final isMap =
          c.classNode.name == 'Map' || c.classNode.name == 'LinkedHashMap';
      final hasMember = c.classNode.members.any(
        (m) => m.name.text == name && !m.isAbstract,
      );
      final IrExpr body;
      switch (name) {
        case '[]':
          // The result is a `dynamic`, as the caller sees it (`as Sym`
          // on it, the isgeneric fixture): the object, or Dart's null.
          body = isMap
              ? IrStaticCall(null, 'dart_option_object', [
                  IrCall(slot, '!map_get', args),
                ])
              : hasMember
              ? IrStaticCall(null, 'dart_option_object', [
                  IrSome(IrCall(slot, '[]', args, fails: true)),
                ])
              : noSuch();
        case '[]=':
          if (!isMap) {
            body = noSuch();
            break;
          }
          final boxed = IrUpcast(
            IrLocal('__d')..rustType = _type(c),
            const IrType('Object'),
            handle: false,
            explicit: true,
          )..rustType = const IrType('Object');
          final store = field.enclosingClass == null
              ? IrAssignTopLevel(field.name.text, boxed)
              : IrAssignStatic(
                  field.enclosingClass!.name,
                  field.name.text,
                  boxed,
                );
          body = IrBlockValue([
            IrExprStmt(IrCall(slot, 'insert', args)),
            store,
          ], IrLiteral('()', const IrType('raw')));
        case 'containsKey':
          // The map's is the prelude's `contains_key(&k)`, spelled as the
          // `Map` lowering spells it so the backend passes the key by
          // reference; a class's is its own method, by value -- and a
          // translated method's `Result` (`UninitializedLocaleData.
          // containsKey` beside the map's `bool`, `DateFormat.localeExists`,
          // run590).
          body = isMap
              ? IrCall(slot, 'contains_key', args)
              : hasMember
              ? IrCall(slot, 'containsKey', args, fails: true)
              : noSuch();
        default:
          body = isMap
              ? IrCall(slot, name, args)
              : hasMember
              ? IrCall(slot, name, args, fails: true)
              : noSuch();
      }
      arms.add((_type(c), body));
    }
    return IrDynamicDispatch(expression(receiver), arms)
      ..rustType = switch (name) {
        '[]' => const IrType('dynamic'),
        'containsKey' => const IrType('bool'),
        _ => null,
      };
  }

  IrExpr _staticGet(StaticGet node) {
    final target = node.target;
    final enclosing = target.enclosingClass;
    if (enclosing == null) {
      // A top-level name. A `const` or `final` is a module constant in Rust
      // too; a computed `get foo => ...` is a function and stops here.
      // Mutable ones too, now that they are emitted. A read of one goes
      // through the cell, which the backend knows from the declaration.
      if (target is Field) {
        return IrTopLevel(target.name.text, module: _topLevelModule(target));
      }
      // A top-level getter is a function here, so reading it is calling it.
      if (target is Procedure && target.kind == ProcedureKind.Getter) {
        return IrStaticCall(
          null,
          target.name.text,
          const [],
          fails: _fails(target),
          diverges: _diverges(target),
          asyncFn: _asyncMember(target),
          module: _topLevelModule(target),
        );
      }
      throw Unsupported('top-level `${target.name.text}`', _sample(node));
    }
    // A static *getter* is a function -- `PlatformDispatcher.instance` --
    // and reading it is calling it, as for a top-level getter above. As a
    // static it was spelled `PlatformDispatcher::INSTANCE`, a constant
    // nothing declared (20 times).
    if (target is Procedure && target.kind == ProcedureKind.Getter) {
      return IrStaticCall(
        enclosing.name,
        target.name.text,
        const [],
        fails: _fails(target),
        diverges: _diverges(target),
        asyncFn: _asyncMember(target),
      );
    }
    return IrStatic(
      enclosing.name,
      target.name.text,
      isEnumValue: enclosing.isEnum,
    );
  }

  /// The place Kernel's desugaring has to be undone.
  ///
  /// Every operator is a method call here, so `a + b` arrives as
  /// `InstanceInvocation(a, '+', [b])`. Left alone it would emit `a.add(b)`,
  /// which is neither what upstream wrote nor what the Rust backend's operator
  /// traits expect. Turning it back into a binary expression is what makes the
  /// two front ends produce the same IR.
  IrExpr _instanceInvocation(InstanceInvocation node) {
    final name = node.name.text;
    // The callee is passed so omitted optional arguments get their defaults.
    // Without it `weigh()` came out as a no-argument call against a
    // three-parameter function -- the same bug the analyzer front end had in
    // round two, living on here because nothing compared the two front ends on
    // a fixture that used defaults.
    // The member the call lands on, for `_landingSlot`: an anonymous mixin
    // application's copy of `_addDiagnostics(ChildType child)` has
    // `RenderBox` written in it, and only the mixin's own -- the trait's
    // -- takes the erased bound (188 `RenderBox` <- `RenderObject`, ws342).
    // `this` typed as the class being lowered (`_bindingOf` does the
    // same): its static type is not on the node, and without it a call on
    // `this` bound none of the class's own parameters -- `didUpdateValue(
    // oldValue)` took an `Option<T>` where the edge is `<T as
    // DartNullable>::Or` (`RestorableValue.value=`, run633).
    final env = typeEnvironment;
    final receiverType = node.receiver is ThisExpression && env != null
        ? _lowering?.getThisType(env.coreTypes, Nullability.nonNullable)
        : _staticType(node.receiver);
    final dispatch = receiverType is InterfaceType
        ? typeEnvironment?.hierarchy.getDispatchTarget(
            receiverType.classNode,
            node.name,
          )
        : null;
    final wasDispatch = _dispatchMember;
    final wasReceiver = _dispatchReceiverType;
    final wasInterface = _dispatchInterface;
    // ..the mixin's own declaration behind a copy in an application, as
    // the copy is typed everywhere (`_originalOf`).
    final dispatchOriginal = dispatch == null ? null : _originalOf(dispatch);
    // An *abstract* target has no dispatch target; the interface member
    // is the landing then, and still binds the class's parameters for
    // the arguments (`didUpdateValue(oldValue)` on `RestorableValue<T>`,
    // the projarg fixture).
    final interfaceTarget = node.interfaceTarget;
    _dispatchMember = dispatchOriginal is Procedure
        ? dispatchOriginal
        : interfaceTarget is Procedure
        ? interfaceTarget
        : null;
    _dispatchReceiverType = receiverType;
    _dispatchInterface = node.interfaceTarget.function;
    // A prelude method's slots as its sibling declares them
    // (`preludeSiblings`: `Set.removeAll(Iterable<Object?>)` takes the
    // set's own `E` here, as `addAll` does), instantiated with the
    // receiver's type arguments.
    final interface = node.interfaceTarget;
    final sibling = interface is Procedure
        ? _preludeDeclaration(interface)
        : null;
    final calleeFunction = sibling?.function ?? interface.function;
    FunctionType? instantiated = node.functionType;
    if (sibling != null && !identical(sibling, interface)) {
      final own = sibling.function.computeFunctionType(Nullability.nonNullable);
      instantiated = receiverType is InterfaceType
          ? Substitution.fromInterfaceType(receiverType).substituteType(own)
                as FunctionType
          : own;
    } else if (interface is Procedure &&
        interface.enclosingClass != null &&
        preludeSiblings.containsKey(
          '${interface.enclosingClass!.name}.${interface.name.text}',
        ) &&
        receiverType is InterfaceType &&
        receiverType.typeArguments.isNotEmpty) {
      // The sibling itself was tree-shaken out of the dill: the slots it
      // would have declared, spelled directly -- an `Iterable<Object?>`
      // parameter takes the collection's own elements.
      final element = receiverType.typeArguments.first;
      final own = interface.function.computeFunctionType(
        Nullability.nonNullable,
      );
      DartType elements(DartType p) =>
          p is InterfaceType &&
              p.classNode.name == 'Iterable' &&
              p.typeArguments.length == 1 &&
              _isTopType(p.typeArguments.single)
          ? InterfaceType(p.classNode, p.nullability, [element])
          : p;
      instantiated = FunctionType(
        [for (final p in own.positionalParameters) elements(p)],
        own.returnType,
        Nullability.nonNullable,
        namedParameters: own.namedParameters,
        requiredParameterCount: own.requiredParameterCount,
      );
    }
    final List<IrExpr> args;
    // A number's own method takes its own type where Dart writes `num`:
    // `x.clamp(0, 1)` on a `double` is `clamp(0.0, 1.0)`, and Rust's
    // `f64::clamp` takes no integer literal. Dart's `num` is not a type
    // here, so the receiver says which one it is
    // (`_MobileCarouselState.builder`, run722).
    final wasNumReceiver = _numReceiver;
    _numReceiver = receiverType is InterfaceType
        ? (const {'double', 'int'}.contains(receiverType.classNode.name)
              ? receiverType.classNode.name
              : null)
        : null;
    try {
      // With the call's type arguments for the method's own parameters,
      // as a static generic call has them (`_withGenericArgs`): `pop<T>
      // (result)` inside `maybePop<T>` binds the callee's `T` to the
      // caller's, which a projected `T?` slot has to know (19 at ws421).
      args = _withGenericArgs(
        calleeFunction,
        node.arguments,
        () => _arguments(
          node.arguments,
          calleeFunction,
          true,
          instantiated,
          null,
          null,
          _narrowSlots(node),
        ),
      );
    } finally {
      _dispatchMember = wasDispatch;
      _dispatchReceiverType = wasReceiver;
      _dispatchInterface = wasInterface;
      _numReceiver = wasNumReceiver;
    }
    // The owner by the receiver's *static* class when that is one of the
    // prelude's collections: TFA devirtualises `Map.cast` onto the one
    // implementation it found (`CanonicalizedMap`, a generic method on a
    // trait), and the prelude's `Map` is what the receiver is here
    // (`invokeMapMethod`, run492).
    // The receiver's static type outright, not `_staticClass`, which
    // answers only translated classes and so never a `dart:core` one
    // (the rule was silent through ws494).
    final staticReceiver = _staticType(node.receiver);
    final receiverClass = staticReceiver is InterfaceType
        ? staticReceiver.classNode
        : null;
    final staticOwner = receiverClass?.name;
    final collectionReceiver =
        receiverClass != null &&
        (staticOwner == 'List' ||
            staticOwner == 'Iterable' ||
            staticOwner == 'Set' ||
            _isMapClass(staticOwner)) &&
        receiverClass.enclosingLibrary.importUri.scheme == 'dart';
    final generic = collectionReceiver ? null : _genericOnTrait(node, args);
    if (generic != null) return generic;
    // The owner the lowering tables are keyed by is the *declaring* class
    // (`Iterable` for a `Set`'s `any`, ws496) -- unless TFA devirtualised
    // the target onto a translated class (`CanonicalizedMap.cast`), where
    // the receiver's static collection is the owner.
    final declaringOwner = node.interfaceTarget.enclosingClass;
    final devirtualised =
        collectionReceiver &&
        declaringOwner != null &&
        declaringOwner.enclosingLibrary.importUri.scheme != 'dart';
    final owner = devirtualised ? staticOwner : declaringOwner?.name;
    // A `StreamView` subclass's inherited `listen` and friends act on the
    // `_stream` it carries (see `lowerClass`).
    final declaringStream = node.interfaceTarget.enclosingClass;
    if (node.receiver is ThisExpression &&
        declaringStream != null &&
        (declaringStream.name == 'Stream' ||
            declaringStream.name == 'StreamView') &&
        declaringStream.enclosingLibrary.importUri.toString() == 'dart:async') {
      return IrCall(IrField(null, '_stream'), name, args);
    }
    // `child.toString()` on a `Listenable?`: an `Option` has no
    // `to_string`, and `dart_str` prints `null` for the absent case as
    // Dart does.
    if (name == 'toString' && args.isEmpty) {
      final t = _staticType(node.receiver);
      // The receiver as it is, not narrowed to its bound (`_receiver`):
      // a `T` receiver went behind a fresh `Rc<T>` and printed as one.
      final asObject = _stringOf(expression(node.receiver), t, explicit: true);
      if (asObject != null) return asObject;
    }
    if (owner == 'List' || _isMapClass(owner) || owner == 'Iterable') {
      // A collection member is a *Rust* method taking `impl Fn`, so a closure
      // given to one is not boxed. `_keeps` cannot say so: the callee is
      // `dart:core`'s, with no body to read, and it answers "kept" for want of
      // evidence. The analyzer front end has no such analysis and said
      // unboxed, so the two wrote different Rust for `m.forEach(..)`.
      for (var i = 0; i < args.length; i++) {
        args[i] = _unboxed(args[i]);
      }
    }
    // `completer.complete()` on a `Completer<void>`: the value is `()`.
    if (owner == 'Completer' &&
        name == 'complete' &&
        (args.isEmpty ||
            (args.length == 1 && node.arguments.positional.isEmpty))) {
      return IrCall(_receiver(node.receiver), 'complete', [
        IrLiteral('()', const IrType('raw')),
      ]);
    }
    // `s[i]` on a String is a one-character String, not an index into a
    // list: `pattern[0] == "a"` in intl's date formatting (44 + 44).
    // `[3, 4, 5].contains(n % 100)` with `n` a `num`: Dart compares by
    // value (`3 == 3.0`), so the `double` is cast to the list's `int`.
    // `xs.cast<T2>()` / `m.cast<K2, V2>()` on any of the prelude's
    // collections: its `cast_to`, converting element representations
    // (`FromDynamic`) as `as List<T2>` does. TFA had devirtualised
    // `Map.cast` onto a `CanonicalizedMap` (`invokeMapMethod`, run491).
    if ((owner == 'List' ||
            owner == 'Iterable' ||
            owner == 'Set' ||
            _isMapClass(owner)) &&
        name == 'cast' &&
        args.isEmpty &&
        node.arguments.types.isNotEmpty) {
      return IrCall(
        _receiver(node.receiver),
        'cast_to',
        const [],
        typeArguments: [for (final t in node.arguments.types) _type(t)],
      );
    }
    // The prelude's `Set::remove` takes the value by reference, like the
    // map's key (`_tickers.remove(ticker)`, 46).
    if (owner == 'Set' && name == 'remove' && args.length == 1) {
      return IrCall(_listReceiver(node.receiver), '!map_remove', [
        _intoElement(
          args.single,
          node.arguments.positional.single,
          _staticType(node.receiver),
        ),
      ]);
    }
    if ((owner == 'List' || owner == 'Iterable' || owner == 'Set') &&
        name == 'contains' &&
        args.length == 1) {
      final listType = _staticType(node.receiver);
      final argType = _staticType(node.arguments.positional.single);
      final element =
          listType is InterfaceType && listType.typeArguments.isNotEmpty
          ? listType.typeArguments.first
          : null;
      if (element is InterfaceType &&
          element.classNode.name == 'int' &&
          argType is InterfaceType &&
          (argType.classNode.name == 'double' ||
              argType.classNode.name == 'num')) {
        return IrCall(_listReceiver(node.receiver), '!contains', [
          IrCast(args.single, 'i64'),
        ]);
      }
      return IrCall(_listReceiver(node.receiver), '!contains', [
        _intoElement(args.single, node.arguments.positional.single, listType),
      ]);
    }
    if (owner == 'String' && name == '[]' && args.length == 1) {
      return IrCall(_listReceiver(node.receiver), 'char_at', args);
    }
    // `trim()` and friends: `str::trim` hands back a `&str`, and being
    // inherent it wins over a trait method of the same name.
    if (owner == 'String' &&
        const {'trim', 'trimLeft', 'trimRight'}.contains(name) &&
        args.isEmpty) {
      const spelled = {
        'trim': 'trim_dart',
        'trimLeft': 'trim_left_dart',
        'trimRight': 'trim_right_dart',
      };
      return IrCall(_listReceiver(node.receiver), spelled[name]!, const []);
    }
    if (owner == 'String' && name == 'split' && args.length == 1) {
      // `s.split(p)`: Rust's `split` wants a `&str` and yields an iterator.
      return IrCall(_listReceiver(node.receiver), 'split_dart', args);
    }
    if (owner == 'String' && name == '*' && args.length == 1) {
      // `'0' * n`: Rust's `repeat` wants a `usize`.
      return IrCall(_listReceiver(node.receiver), 'repeat_dart', args);
    }
    if (owner == 'String' &&
        name == 'contains' &&
        (args.length == 1 || args.length == 2)) {
      // `contains(other, [start])`: `str::contains` is inherent, takes a
      // `&str`, and has no start; the prelude's `contains_dart` has both.
      return IrCall(_listReceiver(node.receiver), 'contains_dart', [
        args.first,
        if (args.length == 2) args[1] else IrLiteral('0', const IrType('int')),
      ]);
    }
    if (owner == 'String' && name == 'startsWith' && args.length == 2) {
      // `startsWith(pattern, index)`: `str::starts_with` takes one argument
      // and, being inherent, would win over a trait method of the same name.
      return IrCall(_listReceiver(node.receiver), 'starts_with_at', args);
    }
    if (owner == 'String' && name == 'replaceRange' && args.length == 3) {
      // Dart's `replaceRange` returns a new string; Rust's `String` has an
      // inherent `replace_range` that mutates in place and takes a range,
      // and an inherent method shadows a trait's. So the prelude's is named
      // apart.
      return IrCall(_listReceiver(node.receiver), 'replace_range_dart', args);
    }
    if (owner == 'Expando') {
      // `expando[object]` / `expando[object] = v`: identity-keyed, so the
      // prelude's `get`/`set` rather than an index. 6 uses.
      if (name == '[]' && args.length == 1) {
        return IrCall(_listReceiver(node.receiver), '!expando_get', [
          args.single,
        ]);
      }
      if (name == '[]=' && args.length == 2) {
        return IrCall(_listReceiver(node.receiver), '!expando_set', args);
      }
    }
    // A typed list with a narrow element -- `Float32List` is `Vec<f32>`,
    // `Int32List` is `Vec<i32>` -- takes Dart's `double`/`int` cast down on
    // the way in and up on the way out. 23 `f32 <= f64` and 14 `i32`/`i64`
    // in `dart:ui`'s colour and vertex code.
    final narrow = _narrowElement(_staticType(node.receiver));
    if (narrow != null && name == '[]' && args.length == 1) {
      return IrCast(
        IrIndex(_listReceiver(node.receiver), args.single),
        narrow.startsWith('f') ? 'f64' : 'i64',
      );
    }
    if (narrow != null && name == '[]=' && args.length == 2) {
      final held = '__t${_nextTemporary++}';
      return IrBlockValue([
        IrLocalDecl(held, null, args[1]),
        IrIndexSet(
          _listReceiver(node.receiver, name),
          args[0],
          IrCast(
            IrCall(IrLocal(held), 'clone', const [])
              ..rustType = args[1].rustType,
            narrow,
          ),
        ),
      ], IrLocal(held));
    }
    if (owner == 'List' || owner == 'Iterable') {
      if (name == '[]' && args.length == 1) {
        // Typed by the list's element, which a generic class's `List<E?>`
        // keeps projected (`<E as DartNullable>::Or`) where the static type
        // of the read says a plain `E?`.
        final list = _listReceiver(node.receiver, name);
        final element = list.rustType?.arguments.length == 1
            ? list.rustType!.arguments.single
            : null;
        return IrIndex(list, args.single)..rustType = element;
      }
      if (name == '[]=' && args.length == 2) {
        // `xs[i] = v` where the expression's value is wanted -- the CFE puts
        // `xs[i] += 1` into a `Let` whose body is this call. Bound, stored as
        // a clone, produced: the same shape every other assignment-as-value
        // takes here. 48 of them.
        final held = '__t${_nextTemporary++}';
        return IrBlockValue([
          IrLocalDecl(held, null, args[1]),
          IrIndexSet(
            _listReceiver(node.receiver, name),
            args[0],
            IrCall(IrLocal(held), 'clone', const [])
              ..rustType = args[1].rustType,
          ),
        ], IrLocal(held));
      }
      // `whereType<T>()`: the elements that are a `T`, by the cast table
      // (`_semantics` nodes filtered in `RenderObject`, run527).
      if (name == 'whereType' &&
          args.isEmpty &&
          node.arguments.types.length == 1) {
        final wantedDart = node.arguments.types.single;
        // `whereType<T>()` over an `Iterable<T?>` is the elements that are
        // there: no runtime test says more than that, and for a function
        // type there is no test at all -- `dart_cast_to::<Function>` named
        // a type nothing declares (`whereType<ImageErrorListener>()` over
        // the listeners' `onError` in `ImageStreamCompleter.reportError`,
        // ws762).
        final receiverType = _staticType(node.receiver);
        final element =
            receiverType is InterfaceType &&
                receiverType.typeArguments.length == 1
            ? receiverType.typeArguments.single
            : null;
        if (element != null &&
            element.nullability == Nullability.nullable &&
            element.withDeclaredNullability(Nullability.nonNullable) ==
                wantedDart) {
          final kept = _type(wantedDart);
          return IrCall(
            _listReceiver(node.receiver, name),
            '!where_present',
            const [],
          )..rustType = IrType('List', arguments: [kept]);
        }
        final wanted = _type(wantedDart);
        return IrCall(
          _listReceiver(node.receiver, name),
          '!where_type',
          const [],
          typeArguments: [wanted],
        )..rustType = IrType('List', arguments: [wanted]);
      }
      final step = iterStepNames[name];
      if (step != null && args.length == 1) {
        // A chain, extended rather than started again when the receiver is
        // already one: `xs.where(f).map(g)` is one `iter()`, not two.
        final source = _listReceiver(node.receiver, name);
        return source is IrIterChain
            ? IrIterChain(source.source, [...source.steps, (step, args.single)])
            : IrIterChain(source, [(step, args.single)]);
      }
      // `lastWhere` is the same shape read from the other end, and the
      // same two prelude methods (`NavigatorState.pop`, ws810).
      if ((name == 'firstWhere' || name == 'lastWhere') && args.length == 2) {
        final where = name == 'firstWhere' ? 'first_where' : 'last_where';
        // `firstWhere(test)` throws when nothing matches; with `orElse` it
        // calls that instead. The omitted `orElse` arrives as `None`, and a
        // generic `impl Fn` parameter cannot take a `None`, so the two are
        // two prelude methods. 25 calls.
        final orElse = args[1];
        final omitted = orElse is IrLiteral && orElse.type.name == 'Null';
        // ..and a *given* one goes in bare: Dart's slot is `E Function()?`
        // and the coercion wrapped it, where the prelude's parameter is a
        // plain `impl Fn()` (`FlutterErrorDetails.summary`, run730).
        var given = orElse is IrSome ? orElse.value : orElse;
        given = given is IrCall && given.name == '!rc' && given.args.isEmpty
            ? given.target!
            : given;
        given = _unboxed(given);
        return IrCall(
          _listReceiver(node.receiver, name),
          omitted ? where : '${where}_or',
          omitted ? [args[0]] : [args[0], given],
        );
      }
      if (name == 'sort') {
        // `sort()` is the natural order Dart's `Comparable` gives
        // (`sort_natural`); `sort(compare)` takes a Dart comparator
        // returning an `int`, which the prelude's `sort_by_dart` turns into
        // an `Ordering`. 36 of these. An *omitted* comparator arrives as
        // the `null` default and is the first of the two, not the second
        // with a `None` (`FlutterError.defaultStackFilter`, run728).
        final given = args.length == 1 ? args.single : null;
        final omitted =
            args.isEmpty || (given is IrLiteral && given.type.name == 'Null');
        return IrCall(
          _listReceiver(node.receiver, name),
          omitted ? 'sort_natural' : 'sort_by_dart',
          omitted ? const [] : args,
        );
      }
      final rust = listMethodNames[name];
      if (rust != null) {
        // An element handed to `remove`/`indexOf`: into the element type,
        // which the prelude's `&T` cannot coerce to (see `_intoElement`).
        final byElement =
            const {'remove', 'indexOf', 'lastIndexOf'}.contains(name) &&
            args.length == 1;
        final receiver = _listReceiver(node.receiver, name);
        final call = IrCall(
          receiver,
          rust,
          byElement
              ? [
                  _intoElement(
                    args.single,
                    node.arguments.positional.single,
                    _staticType(node.receiver),
                  ),
                ]
              : args,
        );
        return call;
      }
      throw Unsupported('`List.$name`', _sample(node));
    }
    if (_isMapClass(owner)) {
      // Its own name: the backend's `.get(&k).cloned()` was keyed on `get`
      // and fired on `ContrastCurve.get(double)` too (14 `&f64`).
      if (name == '[]' && args.length == 1) {
        // The read is typed by the map's own value type -- the receiver's
        // recorded `rustType`, which an erased map keeps as the bound --
        // and a slot it goes into coerces it (`_slotToChild[slot]` returned
        // as the `RenderBox?` `childForSlot` declares).
        IrExpr typed(IrExpr read) {
          final map = read is IrCall ? read.target?.rustType : null;
          if (map != null && map.name == 'Map' && map.arguments.length == 2) {
            final value = map.arguments[1];
            // ..with its signature kept: rebuilt by name, a `Map<String,
            // VoidCallback>`'s value read as a bare `Function` -- an
            // object -- where the Rust value is the typed `Rc<dyn Fn>`
            // (`_customActionCallbacks[id]`, ws515).
            read.rustType = value.isFunction
                ? IrType.function(
                    value.parameters!,
                    value.returns!,
                    nullable: true,
                  )
                : IrType(
                    value.name,
                    nullable: true,
                    arguments: value.arguments,
                  );
          }
          return read;
        }

        // `_cache[tone]` on a `Map<int, _>` with a `num` key: the key is an
        // `f64` here and the map's is `i64`, the same cast `contains` makes.
        final mapType = _staticType(node.receiver);
        final argType = _staticType(node.arguments.positional.single);
        final key = mapType is InterfaceType && mapType.typeArguments.isNotEmpty
            ? mapType.typeArguments.first
            : null;
        if (key is InterfaceType &&
            key.classNode.name == 'int' &&
            argType is InterfaceType &&
            (argType.classNode.name == 'double' ||
                argType.classNode.name == 'num')) {
          return typed(
            IrCall(_receiver(node.receiver), '!map_get', [
              IrCast(args.single, 'i64'),
            ]),
          );
        }
        // A nullable key into a map of non-nullable ones: `_views[_implicitViewId]`.
        // ..by the key's Rust type: an `Object?` key is a `dynamic`, no
        // `Option` (ws501).
        final keyIr = args.single.rustType;
        if (key != null &&
            key.nullability != Nullability.nullable &&
            keyIr != null &&
            isNullable(keyIr)) {
          return typed(IrCall(_receiver(node.receiver), '!map_get_opt', args));
        }
        // The key into the map's key type by the one rule: a `String` into
        // a `Map<Object?, ..>` goes behind a handle (15 at ws421).
        final keyed = key == null
            ? args.single
            : _intoArgument(node.arguments.positional.single, key, args.single);
        return typed(IrCall(_receiver(node.receiver), '!map_get', [keyed]));
      }
      // `m[k] = v`: `insert`, as a statement or for its value (Dart's is
      // `v`; here the old value, which no caller reads).
      if (name == '[]=' && args.length == 2) {
        return IrCall(
          _receiver(node.receiver),
          'insert',
          _mapEntry(node, args),
        );
      }
      if (orderedMapMembers.contains(name)) {
        throw Unsupported(
          '`Map.$name`, which depends on insertion order',
          _sample(node),
        );
      }
      final rust = mapMethodNames[name];
      if (rust == null) throw Unsupported('`Map.$name`', _sample(node));
      // `Map<int, _>.containsKey(tone)` with a `double`: Dart's `3.0 == 3`
      // finds the key, so the `double` is cast to the map's `int`.
      if (const {'containsKey', 'remove', '[]'}.contains(name) &&
          args.length == 1) {
        final mapType = _staticType(node.receiver);
        final key = mapType is InterfaceType && mapType.typeArguments.isNotEmpty
            ? mapType.typeArguments.first
            : null;
        final argType = _staticType(node.arguments.positional.single);
        if (key is InterfaceType &&
            key.classNode.name == 'int' &&
            argType is InterfaceType &&
            argType.classNode.name == 'double') {
          return IrCall(_receiver(node.receiver), rust, [
            IrCast(args.single, 'i64'),
          ]);
        }
        // ..and into the map's key type by the one rule, as `m[k]` is: a
        // `String` into a `Map<Object?, ..>.containsKey` goes behind a
        // handle (`decodeMethodCall`, ws491).
        if (key != null) {
          return IrCall(_receiver(node.receiver), rust, [
            _widened(node.arguments.positional.single, key, args.single),
          ]);
        }
      }
      return IrCall(_receiver(node.receiver), rust, args);
    }
    // A comparison (or any operator outside `stdOperators`) that a
    // translated class declares is that class's method here (`ge`, `lt`):
    // `getWindowType(context) >= AdaptiveWindowType.medium` was a Rust
    // `>=` on a struct with no `PartialOrd` (run643). Only the std
    // operators (`+`, `-`, ..) have an operator trait impl to reach.
    final operatorOwner = node.interfaceTarget.enclosingClass;
    final userOperator =
        operatorOwner != null &&
        _translatedClass(operatorOwner) &&
        !stdOperators.contains(name);
    if (_binaryOperators.contains(name) && args.length == 1 && !userOperator) {
      // `int * double` is a `double` in Dart and a type error in Rust: the
      // `int` side is cast. The receiver's class is the operator's owner;
      // the argument's is asked of the static types.
      var left = _receiver(node.receiver);
      var right = args.single;
      // Comparisons too: `returnValue < 0` on a `double` is `f64 < integer`
      // in Rust until the literal is cast (6 in the colour code).
      // `targetWidth! ~/ (w / h)`: an `int ~/ double` is a `double`
      // division truncated to an `int` in Dart. Both sides go to `f64`
      // and the truncated result comes back to `i64`.
      if (name == '~/') {
        String? classOf(Expression e) {
          final t = _staticType(e);
          return t is InterfaceType ? t.classNode.name : null;
        }

        final leftClass = classOf(node.receiver);
        final rightClass = classOf(node.arguments.positional.single);
        if (leftClass == 'double' || rightClass == 'double') {
          if (leftClass == 'int') left = _toF64(left);
          if (rightClass == 'int') right = _toF64(right);
          return IrCast(
            IrBinary(name, left, right, type: const IrType('double')),
            'i64',
          );
        }
      }
      if (const {
        '+',
        '-',
        '*',
        '/',
        '%',
        '<',
        '>',
        '<=',
        '>=',
      }.contains(name)) {
        String? classOf(Expression e) {
          final t = _staticType(e);
          return t is InterfaceType ? t.classNode.name : null;
        }

        // The receiver's *static* class, not the operator's owner: an
        // `int * double` may resolve to `num.*`.
        final leftClass = classOf(node.receiver);
        final rightClass = classOf(node.arguments.positional.single);
        // Not `num`: a static type of `num` is an `i64` as often as an
        // `f64` in the output (round ws49: 580 casts the wrong way).
        if (leftClass == 'int' && rightClass == 'double') {
          left = _toF64(left);
        }
        if (leftClass == 'double' && rightClass == 'int') {
          right = _toF64(right);
        }
        // A *declared* `num` -- a variable, field or static whose declaration
        // says `num`, an `f64` here -- against an int literal: the literal
        // is cast. Not the static type: `getStaticType` says `num` for an
        // `int` assignment used as a value (`(index = next()) >= 0`), and a
        // cast on that went wrong 200 times (ws53).
        final argument = node.arguments.positional.single;
        if (_declaredNum(node.receiver) &&
            (argument is IntLiteral || classOf(argument) == 'int')) {
          right = _toF64(right);
        } else if (_declaredNum(argument) && leftClass == 'int') {
          left = _toF64(left);
        }
        // Dart's `/` is always a `double`, even on two `int`s (`~/` is the
        // integer one); Rust's `/` on two `i64`s is an `i64`.
        if (name == '/') {
          if (leftClass == 'int') left = _toF64(left);
          if (rightClass == 'int') right = _toF64(right);
          // `targetWidth! / (w / h)`: whatever the static type of the left
          // side says, a `/` with a `double` right side is a `double`
          // division, and Rust has no `i64 / f64`.
          // ..when the left side is a number: a class's own `/` takes
          // what it declares (`BoxConstraints / double` in
          // `ViewConfiguration.fromView`, cast to `f64`, ws474).
          if (rightClass == 'double' &&
              (leftClass == 'int' || leftClass == 'num')) {
            left = _toF64(left);
          }
        }
      }
      return IrBinary(
        name,
        left,
        right,
        // The invocation's own function type says what the operator returns.
        // `getStaticType` would need a StaticTypeContext this lowering does
        // not build, and the function type is already here.
        type: node.functionType == null
            ? null
            : _type(node.functionType!.returnType),
      );
    }
    if (name == 'unary-' && args.isEmpty) {
      return IrUnary('-', _receiver(node.receiver));
    }
    // Dart's `double.floor()`/`ceil()`/`round()` are `int`s; Rust's are
    // `f64`s, inherent, and so not renameable through `DartDouble`. 10
    // `i64 <= f64` in material_color_utilities' HCT solver.
    // A `num` method on a `dynamic` receiver -- `number.isInfinite` in
    // intl's `format(dynamic number)`, devirtualised to `num.isInfinite` by
    // TFA: the receiver is downcast to the `f64` a `num` is here. An `int`
    // inside the `Rc<dyn Object>` would fail that downcast, loudly.
    final receiverStatic = _staticType(node.receiver);
    if ((receiverStatic is DynamicType ||
            (receiverStatic is InterfaceType &&
                receiverStatic.classNode.name == 'Object')) &&
        const {'num', 'int', 'double'}.contains(owner) &&
        const {
          'isInfinite',
          'isNaN',
          'isFinite',
          'round',
          'floor',
          'ceil',
          'truncate',
          'toDouble',
          'toInt',
          'abs',
          'toStringAsFixed',
        }.contains(name)) {
      final asDouble = IrCall(
        IrDowncast(_receiver(node.receiver), 'f64'),
        'clone',
        const [],
      );
      final rounds =
          const {'floor', 'ceil', 'round'}.contains(name) && args.isEmpty;
      final call = IrCall(asDouble, name, args);
      return rounds ? IrCast(call, 'i64') : call;
    }
    if (const {'floor', 'ceil', 'round'}.contains(name) && args.isEmpty) {
      final receiverType = _staticType(node.receiver);
      if (receiverType is InterfaceType &&
          (receiverType.classNode.name == 'double' ||
              receiverType.classNode.name == 'num')) {
        return IrCast(IrCall(_receiver(node.receiver), name, const []), 'i64');
      }
    }
    final receiver = node.receiver;
    // A translated generic method's type arguments (see `IrCall.
    // typeArguments`); a `dart:` class's method takes none in the prelude.
    final target = node.interfaceTarget;
    final withTypeArgs =
        target is Procedure &&
        target.function.typeParameters.isNotEmpty &&
        target.enclosingClass != null &&
        _translatedClass(target.enclosingClass!) &&
        node.arguments.types.length == target.function.typeParameters.length;
    final call = _qualified(
      IrCall(
        receiver is ThisExpression ? null : _receiver(receiver),
        name,
        args,
        typeArguments: withTypeArgs
            ? [for (final t in node.arguments.types) _type(t)]
            : const [],
      ),
      node.interfaceTarget,
      receiver,
    );
    // The call's own result type, on the call itself: the projection
    // below wraps it, and `expression` types only the wrapper, which left
    // a generic method's call untyped -- and the erased twin's cast back
    // (`dart_cast_erased`) spells that type (`find<T>()` returning `T?`,
    // ws496). A `T?` of this declaration's own parameter comes back
    // projected (`<T as DartNullable>::Or`), as the callee's declared `T?`
    // return does.
    if (withTypeArgs) {
      final static = _staticType(node);
      // ..projected only when the callee's `T` is put in as a *bare*
      // parameter of this declaration: `resourcesFor<T?>(..)` returns
      // `<Option<T> as DartNullable>::Or`, a plain `Option<T>`, and typed
      // projected it was wrapped twice (`Localizations.of`, ws503).
      final declaredReturn = target.function.returnType;
      final bareArgument =
          declaredReturn is TypeParameterType &&
          target.function.typeParameters.contains(declaredReturn.parameter) &&
          () {
            final index = target.function.typeParameters.indexOf(
              declaredReturn.parameter,
            );
            final argument = node.arguments.types[index];
            return argument is TypeParameterType &&
                argument.nullability != Nullability.nullable;
          }();
      if (static is TypeParameterType &&
          static.nullability == Nullability.nullable &&
          !_erasedParameter(static.parameter) &&
          bareArgument) {
        call.rustType = IrType(
          static.parameter.name ?? 'T',
          nullable: true,
          projected: true,
        );
      } else if (static != null) {
        try {
          // A `T?` bound to a top type is the `Option<Rc<dyn Object>>`
          // the callee hands back, as `expression` types a read
          // (`invokeMethod<dynamic>(..)` into a `dynamic` local, ws513).
          call.rustType = _topBound(declaredReturn, static) ?? _type(static);
        } on Unsupported {
          // Untyped, as `expression` leaves it.
        }
      }
    }
    // A projected result: into the `Option<T>` the caller works with.
    final declared = node.interfaceTarget.function?.returnType;
    return _acrossBinding(
      call,
      declared,
      _bindingOf(declared, node.interfaceTarget, receiver, node.arguments),
      toOption: true,
    );
  }

  /// See `IrCall.qualifier`: a member whose name two classes in the
  /// receiver's hierarchy declare is called through one of them by name.
  /// The member a call on `receiver` lands on in Rust: an inherent method
  /// of the receiver's class (a mixin clone with `RenderBox` written in
  /// it) when the class is a struct or an open class, the interface
  /// member (the trait's, erased) otherwise. `getDispatchTarget` answers
  /// for both: on an abstract class it finds the hollow mixin's own.
  Member _landing(Member interface, Expression receiver) {
    final hierarchy = typeEnvironment?.hierarchy;
    // ..through `_appliedBack`, as the read does: inside a mixin's body
    // borrowed from an application the copy says `FlexParentData` where
    // the trait holds the erased bound, and the qualifier is that bound's
    // trait, not the application's (ws736).
    final type = receiver is ThisExpression
        ? null
        : _backHere(_staticType(receiver));
    final on = receiver is ThisExpression
        ? (_lowering ?? _member?.enclosingClass)
        : type is InterfaceType
        ? type.classNode
        : null;
    if (hierarchy == null || on == null) return interface;
    final found = hierarchy.getDispatchTarget(
      on,
      interface.name,
      setter: interface is Procedure && interface.isSetter,
    );
    return found ?? interface;
  }

  /// The Rust type of `landing`'s value as reached through `receiver`:
  /// its declared type with the receiver's type arguments substituted for
  /// the parameters that are *kept*, the erased ones left to `_type`,
  /// which spells them as their bound. Dart's own static type substitutes
  /// every one, which is where the clone's `RenderBox` and the trait's
  /// `RenderObject` part ways. Null when the type names the method's own
  /// parameters (Dart's instantiated type is the better answer there) or
  /// has no spelling here.
  IrType? _memberRustType(
    Member reached,
    Expression receiver, {
    required bool asGetter,
  }) {
    // A copy in an anonymous application is typed by the mixin's own
    // declaration, as the copy itself is lowered (the kept parameters
    // substituted below, the erased ones their bounds).
    final landing = _originalOf(reached);
    var declared = asGetter || landing is! Procedure
        ? landing.getterType
        : landing.function.returnType;
    if (landing is Procedure &&
        !asGetter &&
        landing.function.typeParameters.isNotEmpty &&
        _mentionsParametersOf(declared, landing.function.typeParameters)) {
      return null;
    }
    final owner = landing.enclosingClass;
    final env = typeEnvironment;
    final receiverType = receiver is ThisExpression
        ? (env == null
              ? null
              : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable))
        : _staticType(receiver);
    if (owner != null &&
        owner.typeParameters.isNotEmpty &&
        env != null &&
        receiverType is InterfaceType) {
      final asOwner = env.hierarchy.getTypeAsInstanceOf(receiverType, owner);
      if (asOwner is InterfaceType) {
        final kept = <TypeParameter, DartType>{};
        for (
          var i = 0;
          i < owner.typeParameters.length && i < asOwner.typeArguments.length;
          i++
        ) {
          final p = owner.typeParameters[i];
          if (!_erasedParameter(p)) kept[p] = asOwner.typeArguments[i];
        }
        try {
          return _typeKept(declared, kept);
        } on Unsupported {
          return null;
        }
      }
    }
    try {
      return _type(declared);
    } on Unsupported {
      return null;
    }
  }

  static bool _mentionsParametersOf(DartType t, List<TypeParameter> ps) {
    // `FutureOr<R>` is its own node, not an `InterfaceType`: `then<R>`'s
    // `FutureOr<R> Function(void)` slot passed for a slot of no parameter,
    // and the callback was lowered against the declared `R`, its body
    // never closed (`Route.didAdd`, ws512).
    if (t is FutureOrType) return _mentionsParametersOf(t.typeArgument, ps);
    if (t is RecordType) {
      return t.positional.any((a) => _mentionsParametersOf(a, ps)) ||
          t.named.any((n) => _mentionsParametersOf(n.type, ps));
    }
    if (t is TypeParameterType) return ps.contains(t.parameter);
    if (t is InterfaceType) {
      return t.typeArguments.any((a) => _mentionsParametersOf(a, ps));
    }
    if (t is FunctionType) {
      return _mentionsParametersOf(t.returnType, ps) ||
          t.positionalParameters.any((a) => _mentionsParametersOf(a, ps)) ||
          t.namedParameters.any((n) => _mentionsParametersOf(n.type, ps));
    }
    return false;
  }

  IrCall _qualified(IrCall call, Member member, Expression receiver) {
    final out = _qualifiedRaw(call, member, receiver);
    // Typed by the member the call reaches in Rust: through a trait's
    // path (`RestorationMixin::restoration_id(self)`) it is the trait's
    // declaration, whatever the class's override narrowed it to (`String?`
    // there, `String` here: 72 dropped `!`s at ws390); a plain call lands
    // on the class's own.
    out.rustType ??= _memberRustType(
      out.qualifier != null ? member : _landing(member, receiver),
      receiver,
      asGetter: member is Field || (member is Procedure && member.isGetter),
    );
    // An `async` member declared `Future<T>?` still hands back the future
    // it spawns, never null: its wrapper is typed `DartFuture<T>`, and so
    // is a call to it (`sendWithPostfix` in `send`, ws474).
    final t = out.rustType;
    if (out.asyncTarget && t != null && t.name == 'Future' && t.nullable) {
      out.rustType = IrType('Future', arguments: t.arguments);
    }
    // ..and one declared `FutureOr<T>` hands back a `Future<T>` too
    // (Dart's `flatten`; see `_spawnedFuture`).
    if (out.asyncTarget && t != null && t.name == 'FutureOr') {
      out.rustType = IrType('Future', arguments: t.arguments);
    }
    return out;
  }

  IrCall _qualifiedRaw(IrCall call, Member member, Expression receiver) {
    final owner = member.enclosingClass;
    if (owner == null || !_translatedClass(owner)) {
      return _fails(member)
          ? IrCall(
              call.target,
              call.name,
              call.args,
              fails: true,
              diverges: _diverges(member),
            )
          : call;
    }
    // From the receiver's own class: a mixin's `child` is declared again
    // by the trait of the class that mixes it in, *below* the owner.
    // ..through `_appliedBack`, as the read does: inside a mixin's body
    // borrowed from an application the copy says `FlexParentData` where
    // the trait holds the erased bound, and the qualifier is that bound's
    // trait, not the application's (ws736).
    final type = receiver is ThisExpression
        ? null
        : _backHere(_staticType(receiver));
    // Inside a body borrowed from a mixin application (`_appliedBody`)
    // `this` is the mixin's trait, not the anonymous class the CFE copied
    // the body into (`ServicesBinding::x(this_)` named the trait as a
    // type, 9 E0782 at ws436).
    final enclosing = _member?.enclosingClass;
    // A receiver typed by a type parameter (`ChildType child` in a
    // mixin's copy, read by its declaration) is its bound's class, which
    // is what it is here: with no class at all the walk started at the
    // member's owner and `child.toDiagnosticsNode()` on a `dyn
    // RenderObject` was left for three traits to claim (4 E0034, ws536).
    final from = receiver is ThisExpression
        ? ((enclosing?.isAnonymousMixin ?? false) ? _lowering : enclosing)
        : _classOfType(type);
    var qualifier = _qualifierFor(from ?? owner, member);
    // ..and from the class the receiver *is here* when the Dart type said
    // nothing: a closure parameter retyped to an erased bound reads back
    // through a cast, and the wider type above it declares no member for
    // the walk to count -- `notification.depth` on a `ScrollNotification`
    // read out of a `Notification` slot was left for two traits to claim
    // (`_PageViewState.build`, ws719).
    if (qualifier == null &&
        receiver is VariableGet &&
        _retyped.containsKey(receiver.variable)) {
      final declaredClass = _classOfType(receiver.variable.type);
      if (declaredClass != null && !identical(declaredClass, from)) {
        qualifier = _qualifierFor(declaredClass, member);
      }
    }
    // `this.x` where a trait declared `x` and this class overrides it: Rust
    // resolves `self.x()` to the inherent override, whose type may be
    // narrower than the declaration the kernel typed the read by (`String?
    // get restorationId` overridden as `String`: 72 `unwrap` on a `String`
    // at ws296). Through the trait, whose signature the kernel agrees with.
    // The *lowering* class, not the member's: a mixin's body is lowered
    // into the class applying it, and there `this` is that class.
    final host = _lowering ?? from;
    if (qualifier == null &&
        receiver is ThisExpression &&
        host != null &&
        host != owner &&
        _abstractLike(owner) &&
        !owner.isAnonymousMixin &&
        host.members.any(
          (m) =>
              m.name.text == member.name.text &&
              ((m is Procedure && !m.isStatic) || (m is Field && !m.isStatic)),
        )) {
      qualifier = owner.name;
    }
    final fails = _fails(member);
    final renamed = member is Procedure && member.name.text == 'clone';
    // `DART2RUST_TRACE_CALL=<name>`: the async-rule inputs of every call
    // to that member, to stderr.
    if (Platform.environment['DART2RUST_TRACE_CALL'] == member.name.text) {
      stderr.writeln(
        'TRACE_CALL ${member.name.text}: from=${from?.name} owner=${owner.name} '
        'enclosing=${_member?.enclosingClass?.name} lowering=${_lowering?.name} '
        'fails=$fails async=${_asyncMember(member)} qualifier=$qualifier '
        'applies=${from != null && _appliesMixin(from, owner)} '
        'abstract=${from != null && _abstractLike(from)} open=${from != null && _isOpen(from)}',
      );
    }
    if (qualifier == null && !fails && !renamed && !_asyncMember(member)) {
      return call;
    }
    return IrCall(
      call.target,
      renamed ? _dartName(call.name) : call.name,
      call.args,
      qualifier: qualifier,
      receiverClass: _classOfType(type)?.name,
      fails: fails,
      diverges: _diverges(member),
      // A struct's *own* async method, called plainly, is an `async fn`
      // reached as one; an inherited or a trait's goes through the trait
      // impl, which hands the future back inside the `Result`.
      // ..or a mixin's method the receiver's class applies, which is
      // inlined into that class as its own (`handlePopRoute()` inside
      // `WidgetsBinding.initInstances`, run445).
      asyncFn: _inherentAsync(
        member,
        from,
        qualifier,
        onThis: call.target == null,
      ),
      asyncTarget: _asyncMember(member),
      typeArguments: call.typeArguments,
    );
  }

  /// Whether a call to `member` from a receiver of class `from` reaches an
  /// `async fn` as one (`IrCall.asyncFn`): the member is async, and the
  /// receiver's own struct carries it inherently -- its own method, or a
  /// mixin's it applies -- with no trait on the path.
  ///
  /// On `this` (`onThis`) the receiver's own class is the struct or the
  /// trait body being emitted, and the backend knows which: an open or
  /// abstract class's own async method called on `this` counts as
  /// inherent here, and the trait bodies unwrap it (`_handleAsMethodCall`
  /// in `MethodChannel.setMethodCallHandler`'s super fn, run458).
  bool _inherentAsync(
    Member member,
    Class? from,
    String? qualifier, {
    bool onThis = false,
  }) {
    final owner = member.enclosingClass;
    if (Platform.environment['DART2RUST_TRACE_CALL'] == member.name.text) {
      stderr.writeln(
        'TRACE_ASYNC ${member.name.text}: from=${from?.name} owner=${owner?.name} '
        'fails=${_fails(member)} async=${_asyncMember(member)} qualifier=$qualifier '
        'applies=${from != null && owner != null && _appliesMixin(from, owner)} '
        'abstract=${from != null && _abstractLike(from)} open=${from != null && _isOpen(from)}',
      );
    }
    if (owner == null || from == null) return false;
    return (qualifier == null || qualifier == from.name) &&
        _asyncMember(member) &&
        (from == owner || _appliesMixin(from, owner)) &&
        (onThis || (!_abstractLike(from) && !_isOpen(from)));
  }

  /// Whether `from` applies `mixin` somewhere in its anonymous superclass
  /// chain, so that the mixin's methods are inlined into `from`'s struct.
  static bool _appliesMixin(Class from, Class mixin) {
    var t = from.supertype;
    while (t != null && t.classNode.isAnonymousMixin) {
      // The mixin itself, or the application class holding its copy (an
      // interface target inside an applied body names that one).
      if (t.classNode == mixin ||
          t.classNode.implementedTypes.any((i) => i.classNode == mixin)) {
        return true;
      }
      t = t.classNode.supertype;
    }
    return false;
  }

  /// A member declared `async`: emitted as an `async fn` where it is a
  /// free function, a static, or a struct's own method.
  /// By the marker the programmer wrote (`dartAsyncMarker`), which a hollow
  /// mixin declaration keeps where its `asyncMarker` says `Sync` for want
  /// of a body (`ServicesBinding.handleRequestAppExit`, run446).
  bool _asyncMember(Member m) =>
      m is Procedure && m.function.dartAsyncMarker == AsyncMarker.Async;

  /// A member declared to return `Never`.
  static bool _diverges(Member m) =>
      m is Procedure && m.function.returnType is NeverType;

  /// The trait to call `member` through from a value of class `from`, or
  /// null when only one class in the hierarchy declares it and the plain
  /// call is unambiguous.
  ///
  /// A member the CFE cloned into an anonymous mixin application
  /// (`_MixinApplication8&RenderBox&RenderObjectWithChildMixin.child`) is
  /// declared twice on the Rust side: by the mixin's trait and, flattened
  /// (ws112), by the trait of the class that applies it. That class is
  /// the name to call through -- the mixin's trait is not a supertrait of
  /// its, so inside a super function `__Self: ListNotifier` cannot reach
  /// `ListNotifierMixin::_updaters` (295 E0277s the round the mixin was
  /// named instead).
  /// The qualifier a setter call takes (see `IrSetter.qualifier`).
  String? _setterQualifier(Expression? receiver, Member target) {
    final owner = target.enclosingClass;
    if (owner == null || !_translatedClass(owner)) return null;
    final Class? from;
    if (receiver == null || receiver is ThisExpression) {
      from = _lowering ?? _member?.enclosingClass;
    } else {
      // ..through `_appliedBack`, as the read's qualifier is (ws737).
      from = _classOfType(_backHere(_staticType(receiver)));
    }
    if (from == null) return null;
    return _qualifierFor(from, target);
  }

  String? _classNameOf(Expression receiver) =>
      _classOfType(_backHere(_staticType(receiver)))?.name;

  /// The class a value of `t` is: an interface's own, a type parameter's
  /// bound's (through a bound that is itself a parameter).
  Class? _classOfType(DartType? t) {
    var seen = 0;
    while (t is TypeParameterType && seen++ < 8) {
      t = t.parameter.bound;
    }
    return t is InterfaceType ? t.classNode : null;
  }

  String? _qualifierFor(Class from, Member member) {
    final owner = member.enclosingClass!;
    final name = member.name.text;
    final setter = member is Procedure && member.isSetter;
    final seen = <Class>{};
    var found = 0;
    String? applier;
    Member? declared(Class c) {
      for (final m in c.members) {
        if (m.name.text == name &&
            (m is Field || (m is Procedure && m.isSetter == setter))) {
          return m;
        }
      }
      return null;
    }

    // `named`: the nearest class with a name of its own on the superclass
    // path down to `c`, which is where an anonymous application's members
    // were flattened to.
    void walk(Class c, Class named) {
      if (!seen.add(c)) return;
      final m = _translatedClass(c) ? declared(c) : null;
      if (m != null) {
        if (c.isAnonymousMixin) {
          // A cloned *abstract* member (`ScrollMetrics.axisDirection` in
          // `ScrollPosition with ScrollMetrics`) is flattened nowhere: its
          // one declaration is the mixin trait's.
          if (m.isAbstract) {
            found += 1;
          } else {
            found += 2;
            applier ??= named.name;
          }
        } else {
          found += 1;
        }
      }
      final below = c.isAnonymousMixin ? named : c;
      final superclass = c.superclass;
      if (superclass != null) walk(superclass, below);
      for (final s in c.supers) {
        if (s.classNode != superclass) walk(s.classNode, s.classNode);
      }
    }

    // `this` inside a member cloned into an application is the named class
    // the application is lowered into.
    walk(from, from.isAnonymousMixin ? (_lowering ?? from) : from);
    if (found < 2) return null;
    // Through the applying class whenever an application declares it --
    // also when the resolved owner is the mixin itself (`this._notifyUpdate`
    // inside `ListNotifier with ListNotifierMixin`): the mixin's trait is
    // not among a subclass trait's supertraits, the applier's is. An
    // abstract member of an anonymous owner is the mixin's, named by the
    // application's last segment.
    final chosen =
        applier ??
        (owner.isAnonymousMixin ? owner.name.split('&').last : owner.name);
    // Never a synthetic name: 509 "expected value, found trait" the round
    // one got through.
    return chosen.contains('&') ? null : chosen;
  }

  bool _translatedClass(Class c) {
    // The CFE's deduplicated mixin applications (`_MixinApplication8&
    // RenderBox&RenderObjectWithChildMixin`) live in a synthetic library
    // whose scheme is not a package's; they are the mixin's members
    // under another name, translated like it.
    if (c.isAnonymousMixin) return true;
    final uri = c.enclosingLibrary.importUri;
    return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
  }

  /// Whether `c` implements `dart:async`'s `Future` directly: such a
  /// class is the prelude's future here (`_type`), and constructing it
  /// with its value is a future already done (`future_ready`).
  bool _futureLike(Class c) =>
      c.typeParameters.length == 1 &&
      c.enclosingLibrary.importUri.scheme != 'dart' &&
      c.implementedTypes.any(
        (t) =>
            t.classNode.name == 'Future' &&
            t.classNode.enclosingLibrary.importUri.toString() == 'dart:async',
      );

  IrExpr _construct(ConstructorInvocation node) {
    final target = node.target;
    final name = target.name.text;
    if (_futureLike(target.enclosingClass) &&
        node.arguments.positional.length == 1 &&
        node.arguments.named.isEmpty) {
      final held = node.arguments.types.isNotEmpty
          ? _type(node.arguments.types.single)
          : null;
      final value = expression(node.arguments.positional.single);
      final ready = IrStaticCall(null, 'future_ready', [
        held == null
            ? value
            : _widened(
                node.arguments.positional.single,
                node.arguments.types.single,
                value,
              ),
      ]);
      if (held != null) ready.rustType = IrType('Future', arguments: [held]);
      return ready;
    }
    // `ListQueue([capacity])`: the prelude's `Queue` (a `VecDeque`), and
    // the capacity hint is dropped.
    if (const {
          'ListQueue',
          'Queue',
          'DoubleLinkedQueue',
        }.contains(target.enclosingClass.name) &&
        name.isEmpty) {
      return IrNew(const IrType('Queue'), const []);
    }
    // `HashMap(equals: .., hashCode: .., isValidKey: ..)` and `LinkedHashMap`
    // likewise: the prelude's one `Map`, and the custom key equality is
    // dropped -- recorded as the approximation it is (collection's
    // `MapEquality` builds such a map to count entries).
    if (const {
          'HashMap',
          'LinkedHashMap',
        }.contains(target.enclosingClass.name) &&
        name.isEmpty) {
      return IrNew(const IrType('Map'), const []);
    }
    // `Object()`: an identity and nothing else, the prelude's `new_object`.
    if (target.enclosingClass.name == 'Object' &&
        node.arguments.positional.isEmpty &&
        node.arguments.named.isEmpty) {
      return IrStaticCall(null, 'new_object', const []);
    }
    // The constructor's parameters in the constructed type's terms:
    // `Tween<double>(begin: 0)` takes a `T?`, which is a `double?` here.
    final cls = target.enclosingClass;
    // The type arguments come along, as a turbofish where the class is
    // generic: `_FooState<T>()` in `createState` says which `T` (27).
    final created = IrNew(
      IrType(
        _instanceName(cls),
        arguments: _censusOf(
          node.constructedType,
          _erasedArguments(cls, node.constructedType.typeArguments),
        ),
        module: _moduleQualifier(cls),
      ),
      _constructing(
        target.function,
        {
          for (
            var i = 0;
            i < cls.typeParameters.length && i < node.arguments.types.length;
            i++
          )
            cls.typeParameters[i]: node.arguments.types[i],
        },
        () => _arguments(
          node.arguments,
          target.function,
          false,
          _instantiatedConstructor(node),
        ),
      ),
      constructor: name.isEmpty ? null : name,
    );
    // An open class's instance is its `Impl` struct, and every slot typed
    // with the class is the trait handle: the construction leaves as one
    // (570 `SizeImpl` where an `Rc<dyn Size>` was wanted).
    return _isOpen(cls)
        ? IrUpcast(created, _type(node.constructedType))
        : created;
  }

  /// The struct an instance of `cls` is: the class's own name, or the
  /// `Impl` beside an open class's trait.
  String _instanceName(Class cls) {
    // A private implementation class of a `dart:` library is constructed
    // as the public type it implements, which is what the prelude names
    // (`WeakReference(..)` devirtualised to `_WeakReference`, the
    // navigator's `_RouteEntry`, ws505).
    final uri = cls.enclosingLibrary.importUri;
    if (cls.name.startsWith('_') &&
        uri.scheme == 'dart' &&
        uri.toString() != 'dart:ui') {
      for (final above in [
        if (cls.supertype != null) cls.supertype!,
        ...cls.implementedTypes,
      ]) {
        final c = above.classNode;
        if (!c.name.startsWith('_') && c.name != 'Object') {
          return _instanceName(c);
        }
      }
    }
    return _isOpen(cls) ? implName(cls.name) : cls.name;
  }

  static FunctionType? _instantiatedConstructor(ConstructorInvocation node) {
    final cls = node.target.enclosingClass;
    if (cls.typeParameters.isEmpty) return null;
    final declared = node.target.function.computeThisFunctionType(
      Nullability.nonNullable,
    );
    // A constructor's function type carries the class's type parameters as
    // its own (structural ones, `E%`), which a substitution by the
    // constructed type's arguments does not reach: instantiated instead
    // (`HeapPriorityQueue<_TaskEntry<dynamic>>(_taskSorter)` in the
    // binding's constructor took a `Comparator<E%>`, run433).
    if (declared.typeParameters.isNotEmpty) {
      if (declared.typeParameters.length !=
          node.constructedType.typeArguments.length) {
        return null;
      }
      final instantiated = FunctionTypeInstantiator.instantiate(
        declared,
        node.constructedType.typeArguments,
      );
      return instantiated is FunctionType ? instantiated : null;
    }
    final substituted = Substitution.fromInterfaceType(node.constructedType)
        .substituteType(declared);
    return substituted is FunctionType ? substituted : null;
  }

  /// A generic function's type at this call: `_futurize<int>(callbacker)`
  /// takes a `String? Function(_Callback<int>)`, and the closure passed is
  /// typed against that, not against the `T` the declaration wrote.
  static FunctionType? _instantiated(
    StaticInvocation node, [
    FunctionNode? declaration,
  ]) {
    final fn = declaration ?? node.target.function;
    if (fn.typeParameters.isEmpty ||
        node.arguments.types.length != fn.typeParameters.length) {
      return null;
    }
    final declared = fn.computeFunctionType(Nullability.nonNullable);
    final instantiated = FunctionTypeInstantiator.instantiate(
      declared,
      node.arguments.types,
    );
    return instantiated is FunctionType ? instantiated : null;
  }

  IrExpr _staticInvocation(StaticInvocation node) {
    // TFA spells a cast it has proven, or one it cannot check, as
    // `unsafeCast<Clock?>(Zone.current[_clockKey])`. It is the `as` it
    // replaced, and lowers as one -- without this the operand stood in for
    // the whole and an `Rc<dyn Object>` landed in an `Option<Clock>`.
    if (node.target.name.text == 'unsafeCast' &&
        node.target.enclosingLibrary.importUri.toString() == 'dart:_internal' &&
        node.arguments.positional.length == 1 &&
        node.arguments.types.length == 1) {
      // ..and into the cast's type: `unsafeCast<double?>(#1{Size}.width)`
      // hands a `double` to a `double?` (`Some(..)`), which the CFE's `#0`
      // above it is declared as.
      final operand = node.arguments.positional.single;
      final to = node.arguments.types.single;
      final lowered = expression(AsExpression(operand, to));
      // Only that shape: a non-null `T` into a `T?`. Widening every
      // `unsafeCast` doubled `Option`s and unwrapped `dynamic`s (+17, ws159).
      final from = _staticType(operand);
      if (from is InterfaceType &&
          to is InterfaceType &&
          from.classNode == to.classNode &&
          from.nullability != Nullability.nullable &&
          to.nullability == Nullability.nullable) {
        // ..reading the value back first: what is in hand may be the
        // bound an erased slot handed out, and `Some(x)` around an
        // `Rc<dyn Object>` is no `Option<Rc<dyn Cursor>>`
        // (`mouseCursor?.resolve(states)` in
        // `ToggleableStateMixin.buildToggleable`, ws707).
        IrExpr inner = lowered;
        try {
          inner = coerce(
            lowered,
            _type(to.withDeclaredNullability(Nullability.nonNullable)),
          );
        } on Unsupported {
          inner = lowered;
        }
        return IrSome(inner)..rustType = _type(to);
      }
      return lowered;
    }
    final target = node.target;
    final declaration = _preludeDeclaration(target).function;
    final positional = node.arguments.positional;
    // Two of dart:math's, and one of Flutter's own. Rust has all three, and
    // `max` is the same spelling for floats and integers because `f32::max` is
    // inherent and `Ord::max` covers the rest. 372 `max` and 184 `clampDouble`.
    const arithmetic = {'max': 'max', 'min': 'min'};
    final rust = arithmetic[target.name.text];
    if (rust != null && positional.length == 2) {
      // `max(0, x)` with an `int` and a `double`: the `int` is cast, as
      // the operators cast theirs (`0.max(f64)` was 3 `found integer`s).
      String? cls(Expression e) {
        final t = _staticType(e);
        return t is InterfaceType ? t.classNode.name : null;
      }

      var a = expression(positional[0]);
      var b = expression(positional[1]);
      if (cls(positional[0]) == 'int' && cls(positional[1]) == 'double') {
        a = _toF64(a);
      } else if (cls(positional[0]) == 'double' &&
          cls(positional[1]) == 'int') {
        b = _toF64(b);
      }
      // On a `T extends num`: the prelude's numeric protocol (`DartNum`),
      // whose names do not collide with `Ord`'s on a known number
      // (`AnimationMin<T extends num>.value`, run676).
      final first = _staticType(positional[0]);
      if (first is TypeParameterType && !_erasedParameter(first.parameter)) {
        return IrCall(a, 'dart_$rust', [b])..rustType = _type(first);
      }
      return IrCall(a, rust, [b]);
    }
    // The CFE lowers `<int>[3, 11, 29]` to `_GrowableList._literal3(..)`, so a
    // list literal never reaches this compiler as a ListLiteral. Restored
    // rather than transliterated, for the same reason `??` and cascades are:
    // the analyzer front end sees the literal, and the two have to agree.
    final owner = target.enclosingClass?.name;
    // `Uint8List.view(buffer, [offset, length])`: a `Vec<u8>` cannot carry
    // an associated function; the prelude's free one.
    if (owner == 'Uint8List' &&
        target.name.text == 'view' &&
        positional.isNotEmpty) {
      return IrStaticCall(
        null,
        'uint8_list_view',
        _arguments(node.arguments, target.function),
      );
    }
    // `Uint8List.sublistView(data, [start, end])` / `ByteData.sublistView`:
    // a copy of the bytes here (a `TypedData` is its bytes: a `ByteData` or
    // a `Uint8List`), the prelude's free functions (`StandardMessageCodec`,
    // on every platform message; ws493).
    if ((owner == 'Uint8List' || owner == 'ByteData') &&
        target.name.text == 'sublistView' &&
        positional.isNotEmpty) {
      return IrStaticCall(
        null,
        owner == 'Uint8List'
            ? 'uint8_list_sublist_view'
            : 'byte_data_sublist_view',
        _arguments(node.arguments, target.function),
      );
    }
    // `int.parse(s)` / `double.tryParse(s)`: the prelude's four functions.
    // `intl`'s field parsers and 30-odd other sites.
    if ((owner == 'int' || owner == 'double') &&
        (target.name.text == 'parse' || target.name.text == 'tryParse') &&
        positional.length == 1 &&
        node.arguments.named.isEmpty) {
      final fn =
          '${target.name.text == 'parse' ? 'parse' : 'try_parse'}_$owner';
      return IrStaticCall(null, fn, [expression(positional[0])]);
    }
    // `_List<T?>(n)` -- `List.filled(n, null)` after the CFE -- is a list of
    // `n` nulls, which for a nullable element is exactly what it says: the
    // prelude's `vec_of_nones`. A non-nullable element has nothing to fill
    // with and stays refused in the backend. `_makeArray` in
    // `persistent_hash_map.dart`, and everything hashing through it.
    if (owner == '_List' &&
        target.name.text.isEmpty &&
        positional.length == 1 &&
        node.arguments.types.length == 1 &&
        node.arguments.types.single.nullability == Nullability.nullable) {
      // With the element type spelled: for a projected `E?` (a generic
      // class's `List<E?>`) the prelude fills with `<E as DartNullable>::Or`
      // nulls, which nothing could infer (`HeapPriorityQueue._queue`, run436).
      final element = node.arguments.types.single;
      // ..and a `dynamic` element (`List<Object?>`, which is one) holds
      // its nulls as `Null` objects, not as `None`s (ws499).
      final elementIr = _type(element);
      if (elementIr.name == 'dynamic' && !elementIr.nullable) {
        return IrStaticCall(null, 'vec_of_nulls', [expression(positional[0])])
          ..rustType = IrType('List', arguments: [elementIr]);
      }
      return IrStaticCall(
        null,
        'vec_of_nones',
        [expression(positional[0])],
        typeArguments: [
          _type(element.withDeclaredNullability(Nullability.nonNullable)),
        ],
      );
    }
    // `_GrowableList(0)` -- `List.empty(growable: true)` and `<T>[]` after
    // the CFE -- is an empty list. With a length it would be `n` nulls,
    // which for a non-nullable element has nothing to fill with; that one
    // still stops in the backend.
    if (owner == '_GrowableList' &&
        target.name.text.isEmpty &&
        positional.length == 1 &&
        positional.single is IntLiteral &&
        (positional.single as IntLiteral).value == 0) {
      return IrListLiteral(
        const [],
        _type(node.arguments.types.singleOrNull ?? const DynamicType()),
      );
    }
    if ((owner == '_GrowableList' || owner == '_List') &&
        target.name.text.startsWith('_literal')) {
      // Each element widens into the element type, and a local named as an
      // element is cloned (`[left, right]` moved `left`).
      final element = node.arguments.types.singleOrNull;
      // ..and into an `Object?` element (`Object.hashAll([isChecked, ..])`
      // over enums and structs) each is shared, as an argument would be.
      return IrListLiteral([
        for (final e in positional)
          _widened(
            e,
            element,
            _withExpectedReturn(element, e, () => expression(e)),
          ),
      ], _type(element ?? const DynamicType()));
    }
    // The rest of `dart:math`'s functions are methods on `f64` in Rust,
    // spelled almost the same. `log` was refused as a top-level nothing
    // declared, and took `ClampingScrollSimulation._kDecelerationRate` and
    // everything reading it with it.
    const unary = {
      'log': 'ln',
      'exp': 'exp',
      'sqrt': 'sqrt',
      'sin': 'sin',
      'cos': 'cos',
      'tan': 'tan',
      'asin': 'asin',
      'acos': 'acos',
      'atan': 'atan',
    };
    // `dart:math` takes `num`s; Rust's are methods of `f64`, so an `int`
    // argument (`log(10)`, `pow(10, n)`) is cast first.
    IrExpr asDouble(Expression e) {
      final t = _staticType(e);
      final lowered = expression(e);
      // A literal has no static type without a context (`log(10)` in a
      // static's initialiser), and is an `int` by its spelling.
      return e is IntLiteral ||
              (t is InterfaceType && t.classNode.name == 'int')
          ? _toF64(lowered)
          : lowered;
    }

    if ('${target.enclosingLibrary.importUri}' == 'dart:math') {
      final method = unary[target.name.text];
      if (method != null && positional.length == 1) {
        return IrCall(asDouble(positional[0]), method, const []);
      }
      if (target.name.text == 'atan2' && positional.length == 2) {
        return IrCall(asDouble(positional[0]), 'atan2', [
          asDouble(positional[1]),
        ]);
      }
    }
    if (target.name.text == 'pow' && positional.length == 2) {
      return IrCall(asDouble(positional[0]), 'powf', [asDouble(positional[1])]);
    }
    if (target.name.text == 'clampDouble' && positional.length == 3) {
      return IrCall(expression(positional[0]), 'clamp', [
        expression(positional[1]),
        expression(positional[2]),
      ]);
    }
    if (target.name.text == 'unsafeCast' && positional.length == 1) {
      // The CFE's own cast, inserted where it has already proved the type. It
      // does nothing at runtime in Dart; here a cast from a trait object to
      // the struct it proved -- `unsafeCast<_NativePath>(path)` in front of
      // every native taking one -- is the downcast through `Any`.
      final from = _staticType(positional.single);
      final to = node.arguments.types.singleOrNull;
      final fromTraitObject =
          from is DynamicType ||
          (from is InterfaceType &&
              from.nullability != Nullability.nullable &&
              (_abstractLike(from.classNode) ||
                  from.classNode.name == 'Object'));
      if (fromTraitObject &&
          to is InterfaceType &&
          to.nullability != Nullability.nullable &&
          !_abstractLike(to.classNode) &&
          to.classNode.name != 'Object' &&
          (from is! InterfaceType || from.classNode != to.classNode)) {
        // The Rust name: `double` is an `f64` (`arg is double` after TFA).
        final toIr = _type(to);
        return IrCall(
          IrDowncast(
            expression(positional.single),
            rustScalar(toIr.name),
            arguments: toIr.arguments,
          ),
          'clone',
          const [],
        );
      }
      return expression(positional.single);
    }
    // `LinkedHashMap(equals: .., hashCode: ..)` as a factory: the prelude's
    // `Map`, the custom equality dropped (see `_construct`).
    if (const {
          'HashMap',
          'LinkedHashMap',
          'LinkedHashSet',
          'HashSet',
        }.contains(owner) &&
        target.name.text.isEmpty &&
        target.kind == ProcedureKind.Factory) {
      // ..with its type arguments: `HashSet<T>()` into an `Object` slot
      // has nothing else to say what `Set::new()` holds (run560).
      return IrNew(
        IrType(
          owner!.contains('Set') ? 'Set' : 'Map',
          arguments: [for (final t in node.arguments.types) _type(t)],
        ),
        const [],
      );
    }
    // `String.fromCharCodes(codes)`: a free function of the prelude's, since
    // Rust's `String` takes no inherent additions.
    if (owner == 'String' &&
        target.name.text == 'fromCharCodes' &&
        positional.length >= 1) {
      return IrStaticCall(null, 'string_from_char_codes', [
        expression(positional[0]),
      ]);
    }
    // ..and `String.fromCharCode(code)`, one rune (`Icon.build` spells the
    // glyph of an `IconData.codePoint`, run695).
    if (owner == 'String' &&
        target.name.text == 'fromCharCode' &&
        positional.length == 1) {
      return IrStaticCall(null, 'string_from_char_code', [
        expression(positional[0]),
      ]);
    }
    // `scheduleMicrotask(f)`: the prelude's `_schedule_microtask` takes the
    // `Rc<dyn Fn()>` a translated closure is; the public-named one is the
    // prelude's own `Box<dyn FnOnce()>` entry.
    // A `dart:collection` extension getter on an iterable (`xs.lastOrNull`,
    // lowered by the CFE to `IterableExtensions|get#lastOrNull<T>(xs)`):
    // the prelude's method on the receiver, so that the receiver is
    // borrowed as any list method's is (`FocusScopeNode.focusedChild`,
    // run650). The table is the mapping.
    final coreExtension =
        _coreExtensionMethods[target.enclosingLibrary.importUri
            .toString()]?[target.name.text];
    if (coreExtension != null && owner == null && positional.isNotEmpty) {
      final args = _arguments(node.arguments, target.function);
      final element = node.arguments.types.isNotEmpty
          ? _type(node.arguments.types.first)
          : const IrType('dynamic');
      return IrCall(args.first, coreExtension, args.sublist(1))
        ..rustType = IrType(
          element.name,
          nullable: true,
          arguments: element.arguments,
        );
    }
    // `future.onError<E>(handle, test: ..)` -- `dart:async`'s extension on
    // `Future`, which the CFE lowers to `FutureExtensions|onError(future,
    // handle, test: ..)`. It is `catchError` with the error type as part
    // of the test, so at an `Object` `E` the prelude's `catch_error` is
    // the whole of it (`AssetImage.obtainKey`, run725). A narrower `E`
    // needs its `is` in the test and is refused rather than dropped.
    if (target.name.text == 'FutureExtensions|onError' &&
        target.enclosingLibrary.importUri.toString() == 'dart:async' &&
        owner == null &&
        positional.length >= 2) {
      final e = node.arguments.types.length > 1
          ? node.arguments.types[1]
          : null;
      final everyError =
          e is DynamicType ||
          (e is InterfaceType &&
              e.classNode.name == 'Object' &&
              e.classNode.enclosingLibrary.importUri.toString() == 'dart:core');
      if (!everyError) {
        throw Unsupported(
          '`Future.onError` with an error type of its own',
          _sample(node),
        );
      }
      // With the extension's own parameters put in: the handler returns
      // `FutureOr<T>`, and lowered against the declaration it spelled a
      // `T` nothing here declares.
      final args = _arguments(
        node.arguments,
        target.function,
        true,
        _instantiated(node),
      );
      final value = node.arguments.types.isNotEmpty
          ? _type(node.arguments.types.first)
          : const IrType('dynamic');
      return IrCall(args.first, 'catch_error', args.sublist(1))
        ..rustType = IrType('Future', arguments: [value]);
    }
    final coreFunction =
        _coreTopLevel[target.enclosingLibrary.importUri
            .toString()]?[target.name.text];
    if (coreFunction != null && owner == null) {
      final (fn, slots) = coreFunction;
      final args = _arguments(node.arguments, target.function);
      // A prelude callee's slots are not widened into by `_widened` (its
      // generics take the value as it is); the table's are spelled here.
      if (slots != null) {
        for (var i = 0; i < args.length && i < slots.length; i++) {
          args[i] = coerce(args[i], slots[i]);
        }
      }
      return IrStaticCall(null, fn, args);
    }
    if (target.name.text == 'identical' && positional.length == 2) {
      // `identical(x, null)` is `x == null`: the null test the value's
      // representation answers (`IrIsNull`), not a comparison against a
      // `None` -- an `Object?` list element is an `Rc<dyn Object>` holding
      // the `Null` object (`_CompressedNode.put`'s `identical(keyOrNull,
      // null)`, run538).
      if (_isNull(positional[1])) return IrIsNull(expression(positional[0]));
      if (_isNull(positional[0])) return IrIsNull(expression(positional[1]));
      return IrIdentical(expression(positional[0]), expression(positional[1]));
    }
    if (owner == null) {
      // A top-level function, this library's or another's. Which of those it
      // is no longer decides anything here: whether the callee exists in the
      // output is a whole-crate question, and the crate is not known until
      // every library has been lowered, so the backend asks it instead. The
      // analyzer front end never made the distinction, so this is also one
      // fewer place the two of them could differ.
      // The same cleaning `_lowerTopLevel` gives the declaration: an
      // extension member's `Ext|get#name` has to be one identifier at both
      // ends, and the crate-wide "does the callee exist" check compares them.
      return IrStaticCall(
        null,
        _topLevelName(target.name.text),
        _withGenericArgs(
          declaration,
          node.arguments,
          () => _arguments(
            node.arguments,
            declaration,
            true,
            _instantiated(node, declaration),
          ),
        ),
        fails: _fails(target),
        diverges: _diverges(target),
        asyncFn: _asyncMember(target),
        typeArguments: _keptTypeArguments(declaration, node.arguments),
        module: _topLevelModule(target),
      );
    }
    // `Future<T>.value()` with no value: no argument, rather than the
    // omitted optional filled in as `None` -- the prelude's `future_none`
    // makes the `null` of `T` (`()` for `void`), which a `None` is not
    // once the slot is the projected `T?` (+4 at ws570).
    if (owner == 'Future' &&
        target.name.text == 'value' &&
        node.arguments.positional.isEmpty) {
      // Spelled even though the callee is the prelude's -- `_keptTypeArguments`
      // gives a prelude callee none -- because `future_none`'s `T` has
      // nothing else to infer it from: `Future<void>.value()` in an `async`
      // body left `!` to the never-type fallback (5 at ws757).
      List<IrType> spelled() {
        try {
          return [for (final t in node.arguments.types) _typeNested(t)];
        } on Unsupported {
          return const [];
        }
      }

      return IrStaticCall(owner, 'value', const [], typeArguments: spelled());
    }
    return IrStaticCall(
      owner,
      // An unnamed factory -- `factory Vector3(x, y, z)` -- has no name in
      // Kernel at all. The backend spells an empty static name `new`, for a
      // prelude class as much as a translated one, and `_lowerProcedure`
      // declares the factory under that name.
      target.name.text,
      // With the call's type arguments: `WidgetStateProperty.resolveWith<
      // Color?>((states) { .. })` expects the closure to return `Color?`,
      // and the declared `T` said nothing (63 `Option<Color>` <- `Color`).
      _withGenericArgs(
        declaration,
        node.arguments,
        () => _arguments(
          node.arguments,
          declaration,
          true,
          _instantiated(node, declaration),
        ),
      ),
      fails: _fails(target),
      diverges: _diverges(target),
      asyncFn: _asyncMember(target),
      // The prelude's `Future` constructors are generic functions with
      // nothing but the type argument to say what `T` is when no value
      // is handed in (`Future<void>.delayed(Duration.zero)` fell back to
      // the never type, the thenvoid fixture): the class's arguments,
      // spelled.
      typeArguments: owner == 'Future'
          ? _recordedTypes(node.arguments.types)
          : _keptTypeArguments(declaration, node.arguments),
      module: owner == null ? _topLevelModule(target) : null,
    );
  }

  /// `types` spelled, or none when one cannot be.
  List<IrType> _recordedTypes(List<DartType> types) {
    try {
      return [for (final t in types) _type(t)];
    } on Unsupported {
      return const [];
    }
  }

  /// dart:core members the prelude implements as a *sibling* declares
  /// them. Dart's `List<E>.from(Iterable elements)` takes anything and
  /// casts element by element; the prelude copies, as `List.of(Iterable<
  /// E>)` does, and so its slot is `of`'s: coerced into the declared
  /// `Iterable<dynamic>`, the argument was upcast on the way in and nothing
  /// cast it back (`Vec<Hct> <= Vec<Rc<dyn Object>>`, 11 at ws424).
  static const preludeSiblings = {
    'List.from': 'of',
    'Set.from': 'of',
    'Map.from': 'of',
    'HashSet.from': 'of',
    'LinkedHashSet.from': 'of',
    'HashMap.from': 'of',
    'LinkedHashMap.from': 'of',
    // `List.unmodifiable(Iterable)` / `Map.unmodifiable(Map)`: copies,
    // as `of`. `Set.removeAll(Iterable<Object?>)` and its siblings take
    // the set's own elements here, as `addAll(Iterable<E>)` does (ws580).
    'List.unmodifiable': 'of',
    'Map.unmodifiable': 'of',
    'Set.removeAll': 'addAll',
    'Set.retainAll': 'addAll',
    'Set.containsAll': 'addAll',
  };

  Procedure _preludeDeclaration(Procedure target) {
    final owner = target.enclosingClass;
    if (owner == null || _translatedClass(owner)) return target;
    final sibling = preludeSiblings['${owner.name}.${target.name.text}'];
    if (sibling == null) return target;
    for (final p in owner.procedures) {
      // ..of the same kind: a factory's sibling is a factory, an instance
      // method's (`Set.removeAll` -> `addAll`) an instance method.
      if (p.isStatic == target.isStatic &&
          p.name.text == sibling &&
          p.function.positionalParameters.length ==
              target.function.positionalParameters.length) {
        return p;
      }
    }
    return target;
  }

  /// A translated generic callee's type arguments for the type parameters
  /// it keeps (an erased one is its bound and has no slot), spelled as a
  /// turbofish; nothing for a prelude callee, whose Rust signature is its
  /// own, or when one cannot be spelled.
  ///
  /// As type arguments (`_typeNested`), like a class's (`_erasedArguments`):
  /// a `T?` put in for the callee's `R` is the slot `<T as DartNullable>::
  /// Or`, so `makeBox<T?>(..)` returns the same `Box<<T as DartNullable>::
  /// Or>` the local declaring it is spelled with, and the closure it takes
  /// -- whose parameter is that same `T?` -- has the callee's parameter
  /// type. Spelled `Option<T>` the two disagreed (`showMenu<T?>(..).then`
  /// in `_PopupMenuButtonState.showButtonMenu`, ws690).
  List<IrType> _keptTypeArguments(FunctionNode fn, Arguments arguments) {
    final parameters = fn.typeParameters;
    if (parameters.isEmpty || arguments.types.length != parameters.length) {
      return const [];
    }
    if (!_calleeTranslated(fn, null)) return const [];
    try {
      return [
        for (var i = 0; i < parameters.length; i++)
          if (!_erasedParameter(parameters[i])) _typeNested(arguments.types[i]),
      ];
    } on Unsupported {
      return const [];
    }
  }

  /// Arguments in the callee's declaration order.
  ///
  /// Kernel has already split them into positional and named, and a named one
  /// that was omitted is simply absent -- so the callee's own parameter list is
  /// still what decides the order, exactly as in the analyzer front end.
  List<IrExpr> _arguments(
    Arguments node, [
    FunctionNode? callee,
    bool borrows = true,
    FunctionType? instantiated,
    List<DartType>? positionalTypes,
    Map<String, DartType>? namedTypes,
    List<IrType?>? positionalSlots,
  ]) {
    final was = _borrowedArgument;
    _borrowedArgument = borrows;
    try {
      final out = _argumentList(
        node,
        callee,
        instantiated,
        positionalTypes,
        namedTypes,
        positionalSlots,
      );
      // The hidden `Type` arguments a generic method takes (`_typeValues`):
      // each observed type argument as a value -- a `Type::of("X")`, or the
      // enclosing method's own hidden parameter when the argument is its
      // type parameter (`_findModels<T>` calling `getElement..<T>`).
      final target = callee?.parent;
      if (target is Procedure) {
        for (final i in _typeValues(target)) {
          out.add(
            _typeLiteral(
              i < node.types.length ? node.types[i] : const DynamicType(),
            ),
          );
        }
      }
      return out;
    } finally {
      _borrowedArgument = was;
    }
  }

  /// One argument, lowered knowing what the callee does with it.
  ///
  /// A closure may borrow only if the callee is *finished with it* when it
  /// returns. Round 59 asked the weaker question -- "is this an argument" --
  /// and `bin/census_escapes.dart` measured what that costs: of 1234 closures
  /// handed to a call, 394 are kept by the callee. `addListener`,
  /// `scheduleMicrotask`, `Timer`, `WidgetStateProperty.resolveWith`: storing
  /// one needs `'static`, and a borrow cannot give it. Those go back to being
  /// refused, which is the truth about them until objects are counted.
  /// `lower()`, with the slot's owner known (`_slotTranslated`).
  /// Whether the slot being widened into is a callee's parameter (an
  /// edge, spelled projected for a bare `T?`) rather than a body's own.
  bool _argumentEdge = false;

  T _asArgument<T>(T Function() widen) {
    final was = _argumentEdge;
    _argumentEdge = true;
    try {
      return widen();
    } finally {
      _argumentEdge = was;
    }
  }

  IrExpr _forCallee(
    FunctionNode? callee,
    DartType? declared,
    IrExpr lowered,
    IrExpr Function(IrExpr lowered) widen,
  ) {
    final was = _slotTranslated;
    final wasPrelude = _slotPrelude;
    _slotTranslated = _calleeTranslated(callee, declared);
    _slotPrelude = !_translatedCallee(callee) && callee != null;
    try {
      return widen(lowered);
    } finally {
      _slotTranslated = was;
      _slotPrelude = wasPrelude;
    }
  }

  /// Whether the slot being filled is a prelude callee's. Its function
  /// parameters are its own Rust signatures (`first_where_or` takes the
  /// element, not Dart's `T?`), and a closure handed to one is not adapted
  /// by the declared Dart type (132 new at ws419).
  var _slotPrelude = false;

  /// The prelude members that write into a list argument, by class and
  /// name: the positional indices handed out as `&mut` (see `IrMutRef`).
  static const _outBufferArguments = <String, Map<String, Set<int>>>{
    'RandomAccessFile': {
      'readInto': {0},
      'readIntoSync': {0},
    },
  };

  IrExpr _argument(
    Expression value,
    FunctionNode? callee,
    int index, [
    FunctionType? instantiated,
    IrType? slotIr,
    DartType? declaredOverride,
  ]) {
    final param = callee != null && index < callee.positionalParameters.length
        ? callee.positionalParameters[index]
        : null;
    // The *instantiated* parameter type when the call site has one: a
    // `List<Shadow>.add(E)` takes a `Shadow`, and the `E` alone could
    // widen nothing (`Shadow <= Option<Shadow>` after TFA dropped a `!`).
    // ..but the *declared* one when it is a function type naming an erased
    // parameter: the instantiated `bool Function(ScrollNotification)` is
    // not what the slot holds, `bool Function(T)` erased is.
    // A super call fills the mixin's declared slots, with this class's
    // arguments put in (`declaredOverride`, see the super-call lowering).
    final declaredType = declaredOverride ?? param?.type;
    // ..or any declared type naming one: `Entry<S>(v)` with `Entry.T`
    // erased holds an `Rc<dyn Object>`, not the `S` the call put in (the
    // outparam fixture).
    final paramType =
        (declaredOverride == null
            ? _landingSlot(callee: callee, index: index)
            : null) ??
        (declaredType != null && _mentionsErased(declaredType)
            ? declaredType
            : instantiated != null &&
                  index < instantiated.positionalParameters.length
            ? instantiated.positionalParameters[index]
            : declaredType);
    // `DART2RUST_TRACE_ARG=<callee name>`: the slot each argument lands
    // in, to stderr.
    final calleeMember = callee?.parent;
    // A prelude call that *fills* its argument takes the place as `&mut`
    // (`IrMutRef`), never a copy: the table names them.
    if (calleeMember is Member) {
      final outs =
          _outBufferArguments[calleeMember.enclosingClass?.name]?[calleeMember
              .name
              .text];
      if (outs != null && outs.contains(index)) {
        return IrMutRef(expression(value));
      }
      // ..and a translated callee that fills the slot (`_fillsParameter`)
      // takes the caller's place the same way.
      // ..converted into the slot's own type first where it differs
      // (`List<ContainerLayer>` lent to a `List<ContainerLayer?>`,
      // `FollowerLayer._pathsToCommonAncestor`): a lent temporary, filled
      // and dropped -- what the copy did before -- rather than a type
      // error.
      if (calleeMember is Procedure && _fillsParameter(calleeMember, index)) {
        final place = expression(value);
        IrType? slotType;
        try {
          slotType = paramType == null ? null : _type(paramType);
        } on Unsupported {
          slotType = null;
        }
        // The place itself when the types agree: `_widened` clones a
        // local on its way into a slot, and the fill went into the clone.
        if (slotType == null ||
            place.rustType == null ||
            '${place.rustType}' == '$slotType') {
          return IrMutRef(place);
        }
        return IrMutRef(
          _widened(
            value,
            paramType,
            place,
            slotIr:
                slotIr ??
                _landingSlotIr(callee: callee, index: index) ??
                _topBound(declaredType, paramType) ??
                _genericSlotIr(callee, declaredType) ??
                _constructedSlotIr(callee, declaredType),
          ),
        );
      }
    }
    final tracedArg = Platform.environment['DART2RUST_TRACE_ARG'];
    if (calleeMember is Member &&
        (tracedArg == '*' ||
            tracedArg ==
                (calleeMember.enclosingClass?.name ??
                    calleeMember.name.text))) {
      stderr.writeln(
        'TRACE_ARG ${calleeMember.enclosingClass?.name}.${calleeMember.name.text}[$index] in=${_member?.enclosingClass?.name}.${_member?.name.text} '
        'declared=$declaredType instantiated=${instantiated?.positionalParameters} '
        'param=$paramType slotIr=$slotIr landing=${_landingSlotIr(callee: callee, index: index)} generic=${_genericSlotIr(callee, declaredType)}',
      );
    }
    final argument = _numLiteral(
      value,
      paramType,
      callee,
      _forCallee(
        callee,
        declaredType,
        _withBorrowing(
          param,
          callee,
          () => _withExpectedReturn(
            _instantiatedSlot(callee, paramType),
            value,
            () => expression(value),
          ),
        ),
        (lowered) => _asArgument(
          () => _widened(
            value,
            paramType,
            lowered,
            // A `T?` slot with `T` bound to a top type is the `Option<Rc<dyn
            // Object>>` the callee holds (`_topBound`; `DiagnosticsProperty<
            // Object?>(value: ..)`, ws499).
            slotIr:
                slotIr ??
                _landingSlotIr(callee: callee, index: index) ??
                _topBound(declaredType, paramType) ??
                _genericSlotIr(callee, declaredType) ??
                _constructedSlotIr(callee, declaredType),
          ),
        ),
      ),
    );
    if (calleeMember is Member &&
        (tracedArg == '*' ||
            tracedArg ==
                (calleeMember.enclosingClass?.name ??
                    calleeMember.name.text))) {
      stderr.writeln(
        'TRACE_ARG_OUT ${calleeMember.enclosingClass?.name}.${calleeMember.name.text}[$index] '
        '${argument.runtimeType} type=${argument.rustType}',
      );
    }
    // Into a projected slot of a translated callee: the spelled `T?`.
    return _translatedCallee(callee)
        ? _acrossBinding(
            argument,
            declaredType,
            _argumentBinding(callee, declaredType),
            toOption: false,
          )
        : argument;
  }

  /// A closure literal handed to a function-typed parameter returns what
  /// the *parameter's* type says: `String? Function(String)` taking
  /// `(l) => "default"` returns `Some("default")`. The closure's own
  /// return type is what it wrote, not what it is for (5 in intl).
  /// A function-typed slot with the callee's own instantiation put in.
  ///
  /// A generic class's constructor parameter still names the class's `T`
  /// (`OpenContainerBuilder<T>` is `Widget Function(BuildContext, void
  /// Function([T?]))`), and a closure written for it declared a parameter
  /// no name here stands for -- 4 `cannot find type T` at ws761, each one
  /// a `build` that then did not compile. The erased parameters stay as
  /// they are: those slots hold the bound, not what the call put in.
  DartType? _instantiatedSlot(FunctionNode? callee, DartType? param) {
    if (param is! FunctionType || callee == null) return param;
    final Map<TypeParameter, DartType> kept;
    if (identical(callee, _constructedCallee) && _constructedArgs.isNotEmpty) {
      kept = _constructedArgs;
    } else if (identical(callee, _genericCallee) && _genericArgs.isNotEmpty) {
      kept = _genericArgs;
    } else {
      return param;
    }
    final map = {
      for (final e in kept.entries)
        if (!_erasedParameter(e.key)) e.key: e.value,
    };
    if (map.isEmpty) return param;
    try {
      return Substitution.fromMap(map).substituteType(param);
    } on Object {
      return param;
    }
  }

  IrExpr _withExpectedReturn(
    DartType? param,
    Expression value,
    IrExpr Function() lower,
  ) {
    // Through the cast the CFE wraps a closure argument in (`(chunk) =>
    // ..` as `void Function(List<int>)?` for `listen`): the closure under
    // it is what the expected type is for.
    var closure = value;
    while (closure is AsExpression) closure = closure.operand;
    if (param is! FunctionType || closure is! FunctionExpression)
      return lower();
    final was = _expectedReturn;
    final wasFunction = _expectedFunction;
    _expectedReturn = param.returnType;
    _expectedFunction = param;
    try {
      return lower();
    } finally {
      _expectedReturn = was;
      _expectedFunction = wasFunction;
    }
  }

  /// The function type the next lowered closure is expected to have: its
  /// parameters stand in for a closure's own `dynamic` ones. `(locale) =>
  /// ..` in a `List<String Function(String)>` literal is inferred with a
  /// `dynamic` parameter by the CFE, and the `Rc<dyn Fn(String) -> String>`
  /// the list holds does not take an `Rc<dyn Object>`.
  FunctionType? _expectedFunction;

  /// The return type the next lowered body should widen into, if a
  /// parameter's function type says so.
  DartType? _expectedReturn;

  /// An int literal into a parameter a *translated* callee declares `num`
  /// (an `f64` here) is cast. Not a `dart:` callee's: `int.+(num other)` is
  /// declared that way and its `num` is not an `f64` (ws54).
  /// `e as f64`, once: a value already an `f64` -- `coerce` cast it on
  /// the way into the operand slot -- is left alone (`((1 as f64) as f64)`,
  /// 430 at ws356).
  static IrExpr _toF64(IrExpr e) {
    // Each arm of a conditional, not the whole of it: `n > 3 ? 30.0 * s : 0`
    // is a `double` in Dart, and `if .. { 30.0 * s } else { 0 } as f64`
    // is two types Rust will not unify before the cast is reached
    // (`ExpandingBottomSheet._mobileWidthFor` and two more, ws867).
    if (e is IrConditional) {
      final then = _toF64(e.then);
      final otherwise = _toF64(e.otherwise);
      if (identical(then, e.then) && identical(otherwise, e.otherwise)) {
        return e;
      }
      return IrConditional(e.condition, then, otherwise)
        ..rustType = const IrType('double');
    }
    if (e.rustType?.name == 'double') return e;
    if (e is IrCast && e.rust == 'f64') return e;
    // Inside the `Some` the widening already put on: the cast belongs to
    // the value, not to the `Option` (`(Some(0) as f64)`, ws779).
    if (e is IrSome) {
      return IrSome(_toF64(e.value))
        ..rustType = const IrType('double', nullable: true);
    }
    // An integer *literal* is written as a float rather than cast: an
    // unsuffixed literal under `as f64` is an `i32` to Rust, and
    // `1000000000000000000 as f64` does not fit one (`NumberFormat.
    // _numberOfIntegerDigits`, 4 at ws777).
    if (e is IrLiteral && e.type.name == 'int') {
      // Suffixed: two unsuffixed float literals make an *ambiguous*
      // `{float}`, and a method on that does not resolve
      // (`(1000000.0 / 60.0).round()`, E0689 at ws779).
      return IrLiteral('${e.value}.0_f64', const IrType('double'))
        ..rustType = const IrType('double');
    }
    return IrCast(e, 'f64')..rustType = const IrType('double');
  }

  IrExpr _numLiteral(
    Expression value,
    DartType? param,
    FunctionNode? callee,
    IrExpr lowered,
  ) {
    if (param is! InterfaceType) return lowered;
    final slot = param.classNode.name;
    if (slot != 'num' && slot != 'double') return lowered;
    // Already an `f64` -- `coerce` cast it (`((1 as f64) as f64)`, ws356).
    if (lowered.rustType?.name == 'double') return lowered;
    // An integer *literal* where a `double` goes is a double, whoever
    // declares the slot: that is Dart's rule about the literal, not about
    // the callee (`lerpDouble(split, 1, transformed)` in `Split.transform`,
    // ws763). A `num` slot keeps the callee test below, where an `int` is
    // still an `int` unless the callee's `num` is this output's `f64`.
    if (slot == 'double') {
      // A literal, whichever way it arrives: type flow analysis turns one
      // into a `ConstantExpression`, and testing only for `IntLiteral`
      // missed every `lerpDouble(a, 0, t)` in the shape code (9 at ws779).
      final literal =
          value is IntLiteral ||
          (value is ConstantExpression && value.constant is IntConstant);
      if (!literal) return lowered;
      return _toF64(lowered)..rustType = const IrType('double');
    }
    // A literal, or a value whose static type is `int` (a translated
    // callee's `num` is an `f64`, so either is cast).
    final given = _staticType(value);
    final isInt =
        value is IntLiteral ||
        (given is InterfaceType &&
            given.classNode.name == 'int' &&
            given.nullability != Nullability.nullable);
    if (!isInt) return lowered;
    final member = callee?.parent;
    if (member is! Member) return lowered;
    // A *translated* callee's `num` is an `f64` here, whichever library
    // declares it: `lerpDouble(a, 0, t)` lives in `dart:ui` and its body is
    // translated, so its `num?` slots really are `Option<f64>` and an `int`
    // there does not fit. Only a callee the prelude answers keeps its `num`
    // as it was (9 at ws779).
    if (!_translatedCallee(callee)) return lowered;
    return _toF64(lowered)..rustType = const IrType('double');
  }

  IrExpr _namedArgument(
    Expression value,
    Object param, [
    DartType? declaredOverride,
  ]) {
    final callee = _calleeOf(param);
    final declared =
        declaredOverride ?? (param is FunctionParameter ? param.type : null);
    final type =
        (declaredOverride == null
            ? _landingSlot(
                callee: callee,
                name: param is FunctionParameter ? param.parameterName : null,
              )
            : null) ??
        declared;
    final tracedNamed = Platform.environment['DART2RUST_TRACE_NAMED'];
    final argument = _numLiteral(
      value,
      type,
      callee,
      _forCallee(
        callee,
        declared,
        _withBorrowing(
          param,
          callee,
          () => _withExpectedReturn(
            _instantiatedSlot(callee, type),
            value,
            () => expression(value),
          ),
        ),
        (lowered) {
          if (tracedNamed != null &&
              param is FunctionParameter &&
              param.parameterName == tracedNamed) {
            stderr.writeln(
              'TRACE_NAMED ${param.parameterName} declared=$declared type=$type '
              'lowered=${lowered.runtimeType} rust=${lowered.rustType} '
              'slotIr=${_constructedSlotIr(callee, declared)} '
              'translated=${_calleeTranslated(callee, declared)}',
            );
          }
          return _widened(
            value,
            type,
            lowered,
            slotIr:
                _landingSlotIr(
                  callee: callee,
                  name: param is FunctionParameter ? param.parameterName : null,
                ) ??
                _topBound(declared, _argumentBinding(callee, declared)) ??
                _genericSlotIr(callee, declared) ??
                _constructedSlotIr(callee, declared),
          );
        },
      ),
    );
    return _translatedCallee(callee)
        ? _acrossBinding(
            argument,
            declared,
            _argumentBinding(callee, declared),
            toOption: false,
          )
        : argument;
  }

  /// A trait handle into a slot whose *erased* parameter is bounded by a
  /// wider trait: `child`, a `RenderBox`, into `ContainerRenderObjectMixin
  /// .insert(ChildType child, {ChildType? after})`, which is `Rc<dyn
  /// RenderObject>` here. Rust upcasts a bare handle at the call and not
  /// one inside an `Option` (24 `_insertIntoChildList` at ws340), so the
  /// handle is upcast by name, through `map` when it is optional.
  /// The member an instance call lands on (`_landing`), and the receiver's
  /// type, while its arguments are lowered: a parameter's slot is that
  /// member's declared type -- a mixin clone's `RenderBox`, or the trait's
  /// erased bound -- with the receiver's arguments put in for the class's
  /// kept parameters, exactly as a read is typed (`_memberRustType`).
  Procedure? _dispatchMember;
  DartType? _dispatchReceiverType;

  /// A generic callee's own type arguments at the call, while its
  /// arguments are lowered: `T?` under `lerp<Color?>` is `Option<Option<
  /// Rc<dyn Color>>>` here, which Dart's instantiated type collapses.
  FunctionNode? _genericCallee;
  Map<TypeParameter, DartType> _genericArgs = const {};

  T _withGenericArgs<T>(
    FunctionNode fn,
    Arguments arguments,
    T Function() lower,
  ) {
    if (fn.typeParameters.isEmpty ||
        arguments.types.length != fn.typeParameters.length) {
      return lower();
    }
    final wasCallee = _genericCallee;
    final wasArgs = _genericArgs;
    _genericCallee = fn;
    _genericArgs = {
      for (var i = 0; i < fn.typeParameters.length; i++)
        fn.typeParameters[i]: arguments.types[i],
    };
    try {
      return lower();
    } finally {
      _genericCallee = wasCallee;
      _genericArgs = wasArgs;
    }
  }

  IrType? _genericSlotIr(FunctionNode? callee, DartType? declared) {
    if (declared == null ||
        callee == null ||
        !identical(callee, _genericCallee) ||
        _genericArgs.isEmpty) {
      return null;
    }
    if (!_mentionsParametersOf(declared, callee.typeParameters)) return null;
    // The receiver class's own parameters put in first: `Future<String>.
    // then<R>(FutureOr<R> Function(T))` takes a `String`, and a bare `T`
    // left in collided with the caller's `T` (`loadStructuredData<T>`'s
    // parser adapter took a `T`, run600).
    var substituted = declared;
    final receiverType = _dispatchReceiverType;
    if (receiverType is InterfaceType &&
        identical(callee, _dispatchInterface) &&
        receiverType.typeArguments.isNotEmpty) {
      substituted = Substitution.fromInterfaceType(receiverType)
          .substituteType(declared);
    }
    try {
      return _typeKept(substituted, _genericArgs, byTurbofish: true);
    } on Unsupported {
      return null;
    }
  }

  /// The slot a constructor's parameter is at the class's instantiation
  /// (`_constructing`): `SettingsListItem<ThemeMode?>(selectedOption: x)`
  /// takes an `Option<ThemeMode>` where the declaration says `T`, and a
  /// bare `T` widened nothing (`_SettingsPageState.build`, run660).
  IrType? _constructedSlotIr(FunctionNode? callee, DartType? declared) {
    if (declared == null ||
        callee == null ||
        !identical(callee, _constructedCallee) ||
        _constructedArgs.isEmpty) {
      return null;
    }
    final kept = {
      for (final e in _constructedArgs.entries)
        if (!_erasedParameter(e.key)) e.key: e.value,
    };
    if (kept.isEmpty || !_mentionsParametersOf(declared, kept.keys.toList())) {
      return null;
    }
    try {
      return _typeKept(declared, kept);
    } on Unsupported {
      return null;
    }
  }

  /// The interface member whose arguments the dispatch above is for: a
  /// call nested inside one of those arguments has a callee of its own
  /// (`Matrix4.rotationY(angle)` as an argument took the outer call's
  /// first parameter, 31 at ws369).
  FunctionNode? _dispatchInterface;

  DartType? _landingSlot({
    required FunctionNode? callee,
    int? index,
    String? name,
  }) {
    final landing = _dispatchMember;
    if (landing == null || !identical(callee, _dispatchInterface)) return null;
    final fn = landing.function;
    DartType? declared;
    if (index != null && index < fn.positionalParameters.length) {
      declared = fn.positionalParameters[index].type;
    } else if (name != null) {
      for (final p in fn.namedParameters) {
        if (p.parameterName == name) declared = p.type;
      }
    }
    if (declared == null) return null;
    // The method's own parameters are instantiated at the call: Dart's
    // type is the better answer there.
    if (fn.typeParameters.isNotEmpty &&
        _mentionsParametersOf(declared, fn.typeParameters)) {
      return null;
    }
    return _substituteKept(
      declared,
      landing.enclosingClass,
      _dispatchReceiverType,
    );
  }

  /// `_landingSlot` in the IR, `Option` layers kept apart (`_typeKept`).
  IrType? _landingSlotIr({
    required FunctionNode? callee,
    int? index,
    String? name,
  }) {
    final declared = _landingSlot(callee: callee, index: index, name: name);
    final landing = _dispatchMember;
    if (declared == null || landing == null) return null;
    // The declared type again, unsubstituted, for the IR-level put-in.
    final fn = landing.function;
    DartType? raw;
    if (index != null && index < fn.positionalParameters.length) {
      raw = fn.positionalParameters[index].type;
    } else if (name != null) {
      for (final p in fn.namedParameters) {
        if (p.parameterName == name) raw = p.type;
      }
    }
    if (raw == null) return null;
    try {
      return _typeKept(
        raw,
        _keptFor(landing.enclosingClass, _dispatchReceiverType),
      );
    } on Unsupported {
      return null;
    }
  }

  /// The receiver's type arguments for `owner`'s *kept* parameters (the
  /// erased ones are left to `_type`, which spells them as their bound).
  Map<TypeParameter, DartType> _keptFor(Class? owner, DartType? receiverType) {
    final env = typeEnvironment;
    if (owner == null ||
        owner.typeParameters.isEmpty ||
        env == null ||
        receiverType is! InterfaceType) {
      return const {};
    }
    final asOwner = env.hierarchy.getTypeAsInstanceOf(receiverType, owner);
    if (asOwner is! InterfaceType) return const {};
    final kept = <TypeParameter, DartType>{};
    for (
      var i = 0;
      i < owner.typeParameters.length && i < asOwner.typeArguments.length;
      i++
    ) {
      final p = owner.typeParameters[i];
      if (!_erasedParameter(p)) kept[p] = asOwner.typeArguments[i];
    }
    return kept;
  }

  /// `declared` as a Rust type with `kept` put in for its parameters --
  /// in the IR, not in Kernel, because Dart collapses `T?` with `T` bound
  /// to `Color?` into `Color?` and Rust's `Option<T>` does not: that is
  /// `Option<Option<Rc<dyn Color>>>` here (the `WidgetStateProperty<
  /// Color?>.lerp` family, 66 mismatches at ws384).
  IrType _typeKept(
    DartType t,
    Map<TypeParameter, DartType> kept, {
    bool byTurbofish = false,
  }) {
    if (t is TypeParameterType && kept.containsKey(t.parameter)) {
      // What is put in is a type argument: a `U?` there is projected.
      final arg = _typeNested(kept[t.parameter]!);
      if (t.nullability != Nullability.nullable) {
        // A generic *method*'s own parameter is instantiated by what its
        // turbofish spells, and that is the plain `Option<T>` -- so the slot
        // is that and not the projection this declaration uses for its own
        // edges (`entry.complete<T?>(result)` in `Navigator.removeRoute`, 14
        // at ws761). A *class*'s instantiation is not spelled that way: the
        // struct is named `SettingsListItem<<T as DartNullable>::Or>` and
        // its fields keep the projection (ws763).
        return byTurbofish && arg.projected
            ? IrType(arg.name, nullable: true, arguments: arg.arguments)
            : arg;
      }
      // `T?` with `T` bound to `X?` is `X?`, as Dart collapses it and as
      // rustc normalises the projected signature to (`<Option<X> as
      // DartNullable>::Or` is `Option<X>`): the plain `Option`, projected
      // no more. With `T` bound to a bare `U` the slot stays `Or`.
      // ..unless what is put in is itself a projected `U?` of the code
      // here: `<<U as DartNullable>::Or as DartNullable>::Or` normalises
      // to `<U as DartNullable>::Or`, still projected (`Tile<T?>`'s slots
      // from a `Picker<T>`, fixture closureedge).
      if (isNullable(arg)) {
        return arg.projected
            ? arg
            : IrType(arg.name, nullable: true, arguments: arg.arguments);
      }
      // Projected only over a bare type parameter of the code here: over a
      // concrete class the slot normalises to the plain `Option` (and
      // `<GestureBinding as DartNullable>` names a trait as a type).
      final put = kept[t.parameter]!;
      // A function type keeps its signature (see the map read's typing).
      if (arg.isFunction) {
        return IrType.function(arg.parameters!, arg.returns!, nullable: true);
      }
      return IrType(
        arg.name,
        nullable: true,
        arguments: arg.arguments,
        projected: _projectedSlot(
          put.withDeclaredNullability(Nullability.nullable),
        ),
      );
    }
    if (t is InterfaceType && kept.isNotEmpty) {
      final base = _type(t);
      final cls = t.classNode;
      return IrType(
        base.name,
        nullable: base.nullable,
        arguments: [
          for (var i = 0; i < t.typeArguments.length; i++)
            if (i >= cls.typeParameters.length ||
                !_erasedParameter(cls.typeParameters[i]))
              _typeKept(t.typeArguments[i], kept),
        ],
      );
    }
    if (t is FunctionType && kept.isNotEmpty) {
      final named = [...t.namedParameters]
        ..sort((a, b) => a.name.compareTo(b.name));
      return IrType.function(
        [
          for (final p in t.positionalParameters) _typeKept(p, kept),
          for (final p in named) _typeKept(p.type, kept),
        ],
        _typeKept(t.returnType, kept),
        nullable: t.nullability == Nullability.nullable,
      );
    }
    // `FutureOr<T>` with the call's `T` put in: a parser slot `FutureOr<T>
    // Function(ByteData)` at `loadStructuredBinaryData<AssetManifest>(..)`
    // takes the `_AssetManifestBin` a factory returns *as* a `dyn
    // AssetManifest`, which a `T` left in said nothing about (run569).
    if (t is FutureOrType && kept.isNotEmpty) {
      final base = _type(t);
      return IrType(
        base.name,
        nullable: base.nullable,
        arguments: [_typeKept(t.typeArgument, kept)],
      );
    }
    return _type(t);
  }

  /// `declared` with the receiver's type arguments put in for `owner`'s
  /// *kept* parameters; the erased ones stay, for `_type` to spell as
  /// their bound.
  DartType _substituteKept(
    DartType declared,
    Class? owner,
    DartType? receiverType,
  ) {
    final env = typeEnvironment;
    if (owner == null ||
        owner.typeParameters.isEmpty ||
        env == null ||
        receiverType is! InterfaceType) {
      return declared;
    }
    final asOwner = env.hierarchy.getTypeAsInstanceOf(receiverType, owner);
    if (asOwner is! InterfaceType) return declared;
    final kept = <TypeParameter, DartType>{};
    for (
      var i = 0;
      i < owner.typeParameters.length && i < asOwner.typeArguments.length;
      i++
    ) {
      final p = owner.typeParameters[i];
      if (!_erasedParameter(p)) kept[p] = asOwner.typeArguments[i];
    }
    return Substitution.fromMap(kept).substituteType(declared);
  }

  /// Whether a member landing on a field is that field for this class: a
  /// struct holds its clones' fields; a trait body (a mixin, an abstract
  /// or an open class) reaches its own through accessors, and a direct
  /// field write there made the mutation analysis ask for `&mut self`
  /// (12 "incompatible type for trait", ws373).
  bool _heldField(Member interface, Expression receiver) {
    final on = receiver is ThisExpression ? _lowering : _staticClass(receiver);
    // ..and on another object only when that object is a struct: a handle
    // to an open class has accessors, not fields (12 "attempted to take
    // value of method", ws379).
    if (on == null || _abstractLike(on)) return false;
    return _landing(interface, receiver) is Field;
  }

  /// The declaration a copy in an anonymous application is lowered under
  /// (see `_lowerProcedure`'s `signature`), or null for a member that is
  /// its own declaration.
  Procedure? _cloneSignature(Procedure p) {
    final original = _originalOf(p);
    return identical(original, p) || original is! Procedure ? null : original;
  }

  Member _originalOf(Member m, {bool forWrite = false}) {
    final owner = m.enclosingClass;
    if (owner == null || !owner.isAnonymousMixin) return m;
    final setter = m is Procedure && m.isSetter;
    final getter = m is Procedure && m.isGetter;
    for (final st in [
      if (owner.mixedInType != null) owner.mixedInType!,
      ...owner.implementedTypes,
    ]) {
      for (final o in st.classNode.members) {
        if (o.name.text != m.name.text) continue;
        if (m is Field) {
          // A hollow declaration keeps a field as an abstract getter and
          // setter pair (`ChildType? get _lastChild` / `set _lastChild`
          // in `ContainerRenderObjectMixin`, ws478).
          if (o is Field) return o;
          if (o is Procedure && (forWrite ? o.isSetter : o.isGetter)) {
            return o;
          }
          continue;
        }
        if (o is Procedure && o.isSetter == setter && o.isGetter == getter) {
          return o;
        }
      }
    }
    return m;
  }

  /// A super call's slots as this class sees them: the declaration's
  /// parameter types (the mixin's, behind a copy) with this class's
  /// arguments put in for the mixin's kept parameters.
  ///
  /// A copy whose declaration no longer lists the member (TFA dropped it
  /// there) is typed the way the trait was: the application's arguments
  /// taken back out (`_unapplied`) and this class's put in. Untyped, the
  /// argument went to the mixin's super body as the copy's `Panel` where
  /// the trait's erased `S` says `Rc<dyn Widget>` (the unapply fixture).
  (List<DartType>?, Map<String, DartType>?) _superSlots(Member target) {
    final original = _originalOf(target);
    if (original is! Procedure) return (null, null);
    final Class? owner;
    final DartType Function(DartType) declared;
    if (identical(original, target)) {
      final application = target.enclosingClass;
      if (application == null || !application.isAnonymousMixin) {
        return (null, null);
      }
      // A deduplicated application (`dart:mixin_deduplication`) has no
      // `mixedInType`; the mixin is among its `implementedTypes`.
      final mixin =
          application.mixedInType?.classNode ??
          application.implementedTypes
              .map((st) => st.classNode)
              .where((c) => c.isMixinDeclaration)
              .firstOrNull;
      if (mixin == null) return (null, null);
      owner = mixin;
      declared = (t) => _unapplied(t, application, mixin);
    } else {
      owner = original.enclosingClass;
      declared = (t) => t;
    }
    final fn = original.function;
    if (Platform.environment['DART2RUST_TRACE_SUPER'] != null) {
      stderr.writeln(
        'TRACE_SUPER ${target.enclosingClass?.name}.${target.name.text} same=${identical(original, target)} owner=${owner?.name} '
        'slots=${[for (final p in fn.positionalParameters) '${p.type} -> ${declared(p.type)} -> ${_asApplied(declared(p.type), owner)}']}',
      );
    }
    return (
      [
        for (final p in fn.positionalParameters)
          _asApplied(declared(p.type), owner),
      ],
      {
        for (final p in fn.namedParameters)
          p.parameterName: _asApplied(declared(p.type), owner),
      },
    );
  }

  /// What a super read of `target` hands back, as this class sees it:
  /// the declaration's type (the mixin's, behind a copy; a copy whose
  /// declaration is gone unapplied, as `_superSlots` does) with this
  /// class's arguments put in for the kept parameters. Null where this
  /// compiler has no spelling for it.
  IrType? _superReturn(Member target) {
    final original = _originalOf(target);
    final Class? owner;
    final DartType Function(DartType) declared;
    if (identical(original, target)) {
      final application = target.enclosingClass;
      if (application != null && application.isAnonymousMixin) {
        final mixin =
            application.mixedInType?.classNode ??
            application.implementedTypes
                .map((st) => st.classNode)
                .where((c) => c.isMixinDeclaration)
                .firstOrNull;
        if (mixin == null) return null;
        owner = mixin;
        declared = (t) => _unapplied(t, application, mixin);
      } else {
        owner = target.enclosingClass;
        declared = (t) => t;
      }
    } else {
      owner = original.enclosingClass;
      declared = (t) => t;
    }
    final DartType? type = original is Procedure
        ? original.function.returnType
        : original is Field
        ? original.type
        : null;
    if (type == null) return null;
    return _recordedType(_asApplied(declared(type), owner));
  }

  /// A type of a copy in `application`, with the arguments the application
  /// put in for `mixin`'s parameters taken back out: `Slot` where the
  /// application implements `SlottedContainer<Slot, RenderBox>` reads as
  /// `SlotType` again. An erased parameter's argument is taken out too:
  /// the parameter reads as its bound, which is what the trait says
  /// everywhere -- left in, `RestorationMixin<S>.didUpdateWidget(S)` was
  /// declared on the trait with one application's `DatePickerDialog`, and
  /// every other implementor's forwarder mismatched (ws535). Structural,
  /// so an argument that also occurs on its own in the type is taken for
  /// the parameter too -- the copy is the CFE's substitution, and this is
  /// its inverse.
  DartType _unapplied(DartType t, Class application, Class mixin) {
    Supertype? applied;
    if (application.mixedInType?.classNode == mixin) {
      applied = application.mixedInType;
    } else {
      for (final st in application.implementedTypes) {
        if (st.classNode == mixin) applied = st;
      }
    }
    if (applied == null) return t;
    final back = <DartType, TypeParameter>{};
    for (var i = 0; i < mixin.typeParameters.length; i++) {
      if (i >= applied.typeArguments.length) break;
      final p = mixin.typeParameters[i];
      final a = applied.typeArguments[i].withDeclaredNullability(
        Nullability.nonNullable,
      );
      if (a is TypeParameterType && a.parameter == p) continue;
      back[a] = p;
    }
    if (back.isEmpty) return t;
    DartType walk(DartType x) {
      final bare = x.withDeclaredNullability(Nullability.nonNullable);
      final p = back[bare];
      if (p != null) return TypeParameterType(p, x.nullability);
      if (x is InterfaceType) {
        return InterfaceType(x.classNode, x.nullability, [
          for (final a in x.typeArguments) walk(a),
        ]);
      }
      if (x is FunctionType) {
        return FunctionType(
          [for (final a in x.positionalParameters) walk(a)],
          walk(x.returnType),
          x.nullability,
          namedParameters: [
            for (final n in x.namedParameters)
              NamedType(n.name, walk(n.type), isRequired: n.isRequired),
          ],
          typeParameters: x.typeParameters,
          requiredParameterCount: x.requiredParameterCount,
        );
      }
      return x;
    }

    return walk(t);
  }

  /// A declared type of `owner`'s (a mixin's) with `owner`'s parameters
  /// substituted by the class being lowered's arguments for them.
  /// A mixin's body comes from an *application* (`_appliedBody`), where the
  /// CFE has already put the application's arguments in for the mixin's
  /// parameters: `ContainerRenderObjectMixin.visitChildren` copied into
  /// `RenderFlex`'s application casts to `FlexParentData` where the mixin
  /// wrote `ParentDataType`. Lowered as the *trait's* default that body
  /// serves every application, so the argument goes back to the parameter
  /// -- and only for an **erased** one, whose spelling is its bound, the
  /// trait everything reads through anyway (`RenderSliverList` cast a
  /// `SliverMultiBoxAdaptorParentData` to `FlexParentData`, run734).
  Map<DartType, DartType> _appliedBack = const {};

  /// A type as this *body* holds it: an applied mixin body's concrete
  /// argument is the mixin's erased parameter here (`_appliedBack`).
  DartType? _backHere(DartType? t) {
    if (t is! InterfaceType || _appliedBack.isEmpty) return t;
    final back =
        _appliedBack[t.withDeclaredNullability(Nullability.nonNullable)];
    if (back == null) return t;
    return t.nullability == Nullability.nullable
        ? back.withDeclaredNullability(Nullability.nullable)
        : back;
  }

  Map<DartType, DartType> _appliedBackMap(Class mixin, Class? application) {
    if (application == null) return const {};
    Supertype? applied;
    for (final t in application.implementedTypes) {
      if (t.classNode == mixin) applied = t;
    }
    if (applied == null) return const {};
    final out = <DartType, DartType>{};
    for (var i = 0; i < applied.typeArguments.length; i++) {
      if (i >= mixin.typeParameters.length) break;
      final p = mixin.typeParameters[i];
      if (!_erasedParameter(p)) continue;
      final argument = applied.typeArguments[i];
      if (argument is! InterfaceType) continue;
      out[argument.withDeclaredNullability(Nullability.nonNullable)] =
          TypeParameterType(p, Nullability.nonNullable);
    }
    return out;
  }

  DartType _asApplied(DartType declared, Class? owner) {
    final env = typeEnvironment;
    final thisType = env == null
        ? null
        : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable);
    if (owner == null || thisType is! InterfaceType) return declared;
    // The *kept* parameters only (`_keptFor`): an erased one is its bound
    // everywhere, the trait included, and `ChildType` put in as `RenderBox`
    // brought the `RenderBox`-typed copies back (+47 at ws489).
    final kept = _keptFor(owner, thisType);
    if (kept.isEmpty) return declared;
    return Substitution.fromMap(kept).substituteType(declared);
  }

  /// A field's type as this class holds it: a copy's by the mixin's
  /// declaration with this class's arguments put in (`_asApplied`), its
  /// own otherwise. The same answer for the declaration and for the
  /// initialiser the CFE moved into the application's constructor.
  DartType _fieldTypeHere(Field field) {
    final declared = _declaredFieldType(field);
    if (declared == null) return field.type;
    return _asApplied(declared, _originalOf(field).enclosingClass);
  }

  /// The type a copy's field is declared with (see `_originalOf`), or
  /// null for a field that is its own declaration.
  DartType? _declaredFieldType(Field field) {
    final original = _originalOf(field);
    if (identical(original, field)) return null;
    if (original is Field) return original.type;
    if (original is Procedure && original.isGetter) {
      return original.function.returnType;
    }
    return null;
  }

  /// Where a write lands for its slot's type: through the trait's setter
  /// (a qualified write) it is the declaring member's, the mixin's own
  /// for a copy in an application (`this.child = child` in `RenderView`'s
  /// constructor, `RenderBox?` there and `RenderObject?` in the trait,
  /// ws476); a plain write lands on the class's own.
  Member _writeLanding(Member interface, Expression receiver) => _originalOf(
    _setterQualifier(receiver, interface) != null
        ? interface
        : _landing(interface, receiver),
    forWrite: true,
  );

  /// The type a write into `interface` on `receiver` must produce: the
  /// landing member's -- a mixin clone's field, or the trait's setter.
  /// The mixin's own member behind a copy the CFE made in an anonymous
  /// application (`_MixinApplication8&RenderBox&RenderObjectWithChildMixin
  /// .child=` for `RenderObjectWithChildMixin.child=`): what the trait
  /// declares, with the mixin's parameter (`ChildType?`) where the copy
  /// has the application's argument (`RenderBox?`).
  DartType _writeSlot(Member interface, Expression receiver) {
    final landing = _writeLanding(interface, receiver);
    final declared = landing is Procedure && landing.isSetter
        ? landing.function.positionalParameters.single.type
        : landing.setterType;
    final env = typeEnvironment;
    final receiverType = receiver is ThisExpression
        ? (env == null
              ? null
              : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable))
        : _staticType(receiver);
    return _substituteKept(declared, landing.enclosingClass, receiverType);
  }

  /// `_writeSlot` in the IR, `Option` layers kept apart (`_typeKept`).
  IrType? _writeSlotIr(Member interface, Expression receiver) {
    final landing = _writeLanding(interface, receiver);
    final declared = landing is Procedure && landing.isSetter
        ? landing.function.positionalParameters.single.type
        : landing.setterType;
    final env = typeEnvironment;
    final receiverType = receiver is ThisExpression
        ? (env == null
              ? null
              : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable))
        : _staticType(receiver);
    try {
      return _typeKept(
        declared,
        _keptFor(landing.enclosingClass, receiverType),
      );
    } on Unsupported {
      return null;
    }
  }

  /// `Some(..)` around a non-null argument handed to a nullable parameter --
  /// Dart's silent widening, spelled. Only when the static type says the
  /// argument is not itself nullable, so a nullable variable passed on stays
  /// as it is.
  /// A map literal's entries, each key and value into the map's own types,
  /// sharing into an `Object?` value (`{'extension': name, 'value': value}`
  /// handed to `postEvent` as a `Map<String, Object?>`), as a list literal's
  /// are. The types are a parameter: a literal spread into a wider map --
  /// `<SingleActivator, Intent>{..}` into the `Map<ShortcutActivator,
  /// Intent>` of `DefaultTextEditingShortcuts` -- is lowered against the
  /// wider one's, so each key is shared into its `Rc<dyn ..>` (121
  /// "arguments incorrect" on one file in `widgets`).
  IrExpr _mapLiteral(MapLiteral node, DartType keyType, DartType valueType) {
    // Typed as the map it is, so a slot of another type adapts it: one
    // returned where `dynamic` goes is put behind a handle
    // (`_handlePlatformMessage`'s `{'response': ..}`, run460).
    return IrMapLiteral(
      [
        for (final entry in node.entries)
          (
            _widened(entry.key, keyType, expression(entry.key)),
            _widened(entry.value, valueType, expression(entry.value)),
          ),
      ],
      _type(keyType),
      _type(valueType),
    )..rustType = IrType('Map', arguments: [_type(keyType), _type(valueType)]);
  }

  /// A list literal's elements into `element`. The CFE keeps a literal of
  /// more than eight elements as a node (the `_literalN` constructors stop
  /// there): its elements widen and share into the element type exactly as
  /// the short ones' do.
  /// A record literal's fields into `fields` -- the literal's own types,
  /// or the slot's when it lands in one of other field types (see
  /// `_widenedInto`): `(false, null)` returned as a `(bool, Object?)`
  /// holds the `Null` object, `(true, x)` boxes its `int` (ws502, ws509).
  /// As translated slots, whatever call the record sits in.
  IrExpr _recordLiteral(
    RecordLiteral node,
    List<DartType> fields, [
    List<NamedType> named = const [],
  ]) {
    // The named fields in the *slot's* order, found by name: a literal
    // writes them in source order and the type spells them sorted.
    final namedFields = named.isEmpty ? node.recordType.named : named;
    NamedExpression written(String name) =>
        node.named.firstWhere((e) => e.name == name);
    final wasTranslated = _slotTranslated;
    final wasPrelude = _slotPrelude;
    _slotTranslated = true;
    _slotPrelude = false;
    try {
      return IrRecord([
          for (var i = 0; i < node.positional.length; i++)
            i < fields.length
                ? _widened(
                    node.positional[i],
                    fields[i],
                    expression(node.positional[i]),
                  )
                : expression(node.positional[i]),
          for (final n in namedFields)
            _widened(
              written(n.name).value,
              n.type,
              expression(written(n.name).value),
            ),
        ])
        ..rustType = IrType(
          'Record',
          arguments: _nested(
            () => [
              for (var i = 0; i < node.positional.length; i++)
                _recordedType(
                      i < fields.length
                          ? fields[i]
                          : node.recordType.positional[i],
                    ) ??
                    const IrType('dynamic'),
              for (final n in namedFields)
                _recordedType(n.type) ?? const IrType('dynamic'),
            ],
          ),
        );
    } finally {
      _slotTranslated = wasTranslated;
      _slotPrelude = wasPrelude;
    }
  }

  IrExpr _listLiteral(ListLiteral node, DartType element) {
    return IrListLiteral([
      for (final e in node.expressions)
        _widened(
          e,
          element,
          _withExpectedReturn(element, e, () => expression(e)),
        ),
    ], _type(element));
  }
}
