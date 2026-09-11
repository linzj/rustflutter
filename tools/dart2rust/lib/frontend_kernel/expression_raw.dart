part of '../frontend_kernel.dart';

// `_expressionRaw`: the dispatch, and the first runs it is cut into.
augment class KernelFrontend {
  /// The dispatch over every kind of Kernel expression.
  ///
  /// One `if (node is ..)` per kind. This was a single 1,717-line method;
  /// the runs below are that method cut at family boundaries, each answering
  /// for its own kinds and returning null for the rest. **The order across
  /// the runs is the order the one method had**, which is what a kind tested
  /// more than once depends on -- `DynamicInvocation` is asked three things
  /// inside `_rawEqualityOrDynamic`, and they still happen in that sequence.
  IrExpr _expressionRaw(Expression node) =>
      _rawLiteral(node) ??
      _rawReadOrInvoke(node) ??
      _rawFunctionValue(node) ??
      _rawEqualityOrDynamic(node) ??
      _rawLogicOrTest(node) ??
      _rawSuperOrRecord(node) ??
      _rawStringOrAwait(node) ??
      _rawWrite(node) ??
      _rawCast(node) ??
      _rawTearOff(node) ??
      // Last, on purpose: a `dynamic` access every other run has declined
      // is the one that has nothing but the object to ask (see
      // `_dynamicMemberCall`). Ahead of any of them it would take the
      // number arithmetic and the slot dispatch with it.
      _dynamicMemberCall(node) ??
      (throw Unsupported('expression ${node.runtimeType}', _sample(node)));

  /// Literals and `this`.
  ///
  /// One run of `_expressionRaw`; null for a node it does not answer for.
  IrExpr? _rawLiteral(Expression node) {
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
    return null;
  }

  /// Reading a local, a field or a static, and calling a method on one.
  ///
  /// One run of `_expressionRaw`; null for a node it does not answer for.
  IrExpr? _rawReadOrInvoke(Expression node) {
    if (node is VariableGet) {
      if (Platform.environment['DART2RUST_TRACE_PROMOTED'] ==
          (_member?.name.text ?? '')) {
        stderr.writeln(
          'TRACE_PROMOTED ${node.variable.cosmeticName} '
          'declared=${node.variable.type} '
          'promoted=${node.promotedType}',
        );
      }
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
          // `!late` and not `unwrap`: reading a `late` local before it is
          // written throws `LateInitializationError` in Dart and `try { ..
          // } catch (e)` around one is ordinary Dart, so the backend owns
          // what the empty `Option` becomes (`places.dart`'s `_lateRead`).
          // An `unwrap` here was a panic, and a panic is never a pass.
          return IrCall(
            IrCall(IrLocal(name), 'clone', const []),
            '!late',
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
      // Promoting a local whose declared type is a *type parameter* gives
      // an intersection, not a type: `if (v is double)` on a `T v` reads
      // `T & double`, and the branches below -- written for an
      // `InterfaceType`, a `TypeParameterType`, a `FunctionType` -- match
      // none of it, so the read went in as a bare `T`
      // (`IterableProperty.valueToString`, whose `debugFormatDouble(v)`
      // wanted an `f64`). What the promotion *says it now is* is the
      // right-hand side.
      if (promoted is IntersectionType) promoted = promoted.right;
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
      // ..and a parameter spelled at a *nullable bound* is held in an
      // `Option` too, whatever Kernel calls its nullability: `T extends
      // String?` is an `Option<String>` here, so the promotion unwraps it
      // as it would a `String?` (intl's `toBeginningOfSentenceCase`, whose
      // `input.substring(1)` after the guard was on an `Option<String>`,
      // ws964). Asked of the *spelling*, not of the declared type -- and
      // not of a projected `T?`, which is an associated type and not an
      // `Option` at all.
      final declaredIr = _recordedType(declared);
      final heldInOption =
          declared.nullability == Nullability.nullable ||
          (declaredIr != null && declaredIr.nullable && !declaredIr.projected);
      if (promoted != null &&
          declared is! DynamicType &&
          promoted.nullability != Nullability.nullable &&
          heldInOption) {
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
    return null;
  }

  /// Calling a function value, writing a static, and `let`.
  ///
  /// One run of `_expressionRaw`; null for a node it does not answer for.
  IrExpr? _rawFunctionValue(Expression node) {
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
      // A *generic* local function is written at its parameters' bounds
      // (see `FunctionDeclaration`), so the call has to speak those terms:
      // every argument into the erased slot, and the result back out of it
      // at the type this call instantiated `T` with. `node.functionType`
      // is the instantiated one and says neither -- reading the call's
      // type off it is how `resolvedForegroundBuilder` came out declared
      // `Option<Rc<dyn Fn(..)>>` over a value that is an `Rc<dyn Object>`.
      // A function value survives the round trip like any other object
      // (`dart_function_object`, the `dynfn` fixture).
      final declared = node.localFunction.function;
      if (declared.typeParameters.isNotEmpty) {
        final erased = declared.computeFunctionType(Nullability.nonNullable);
        final call = IrCallValue(
          IrLocal(name),
          _argumentsByType(node.arguments, erased),
        )..rustType = _type(erased).returns;
        final want = _type(node.functionType).returns;
        return want == null ? call : coerce(call, want);
      }
      // A local function is declared as a closure, so its named parameters
      // are in type order there too.
      // ..and its *omitted optional* arguments get their declared defaults,
      // as a method call's do. `node.functionType` describes the
      // invocation, so a call that passes nothing says nothing about the
      // parameter that was left out: `void takeMeasurementsInSourceRoute
      // ([Duration? _])` called as `takeMeasurementsInSourceRoute()` came
      // out as a no-argument call against a one-parameter closure
      // (`_OpenContainerRoute._takeMeasurements`). Only when the
      // declaration has more positionals than the call supplied -- reading
      // the declaration unconditionally would drop a supplied argument.
      final filled =
          declared.positionalParameters.length >
              node.arguments.positional.length
          ? _arguments(node.arguments, declared)
          : _argumentsByType(node.arguments, node.functionType);
      return IrCallValue(IrLocal(name), filled)
        ..rustType = _type(node.functionType).returns;
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
    return null;
  }
}
