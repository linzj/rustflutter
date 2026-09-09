part of '../frontend_kernel.dart';

// `_expressionRaw`: the dispatch over every kind of Kernel expression.
augment class KernelFrontend {
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
}
