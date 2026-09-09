part of '../frontend_kernel.dart';

// Dispatch runs: `as`, evaluated constants and tear-offs.
augment class KernelFrontend {
  /// `as`, static tear-offs and evaluated constants.
  ///
  /// One run of `_expressionRaw`; null for a node it does not answer for.
  IrExpr? _rawCast(Expression node) {
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
    return null;
  }

  /// A method used as a value.
  ///
  /// One run of `_expressionRaw`; null for a node it does not answer for.
  /// The type arguments an [Instantiation] just above a tear-off supplies
  /// (`f<int>` as a value). Null outside one, which is what makes a generic
  /// method used as a value a refusal rather than a guess.
  List<DartType>? _tearOffTypes;

  IrExpr? _rawTearOff(Expression node) {
    // `f<int>` as a *value*: Dart instantiates a generic function value at
    // the types written -- and those types are exactly what the tear-off
    // underneath is missing. A Rust closure has no type parameters of its
    // own, so the closure the tear-off becomes calls the method *with* them
    // (`IrCall.typeArguments`). `showDialog` hands `Navigator.of(context)
    // .pop` over that way, and the whole function was refused for it
    // (ws951).
    if (node is Instantiation) {
      final saved = _tearOffTypes;
      _tearOffTypes = node.typeArguments;
      try {
        return expression(node.expression);
      } finally {
        _tearOffTypes = saved;
      }
    }
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
      final tornRaw =
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
      // A *generic* method torn off: only with an `Instantiation` above it
      // saying at which types. Without one there is nothing to put in --
      // a Rust closure cannot be generic -- and it stays refused.
      final tearOffTypes = _tearOffTypes;
      final generic = fn.typeParameters.isNotEmpty;
      if (generic &&
          (tearOffTypes == null ||
              tearOffTypes.length != fn.typeParameters.length)) {
        throw Unsupported('a generic method used as a value', _sample(node));
      }
      // The method's own parameters put in, on top of the receiver's: the
      // closure takes a `String`, not the `T` the method declares.
      final method = generic
          ? Substitution.fromPairs(fn.typeParameters, tearOffTypes!)
          : null;
      final torn = tornRaw;
      // A generic method's torn type still names the method's own
      // parameters -- and as *structural* copies, which no substitution
      // over the declaration's `TypeParameter`s reaches. So where the
      // `Instantiation` said which types, the declaration is what gets
      // them put in: `note<T>(T value)` at `String` takes a `String`.
      DartType positionalType(int i) {
        final declared = fn.positionalParameters[i].type;
        if (method != null) return method.substituteType(declared);
        return torn is FunctionType && i < torn.positionalParameters.length
            ? torn.positionalParameters[i]
            : declared;
      }

      DartType namedType(String name, DartType declared) {
        if (method != null) return method.substituteType(declared);
        if (torn is FunctionType) {
          for (final n in torn.namedParameters) {
            if (n.name == name) return n.type;
          }
        }
        return declared;
      }

      final returnType = method != null
          ? method.substituteType(fn.returnType)
          : (torn is FunctionType ? torn.returnType : fn.returnType);
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
      // The callee's slot for a generic method's `T?` parameter is the
      // *projected* `<T as DartNullable>::Or`, which at the instantiated
      // `T` is an `Option` -- while the closure takes what the slot it
      // lands in declares (`Object?` is a bare `Rc<dyn Object>` here). So
      // the value is put into the callee's spelling on the way through,
      // as an ordinary call's argument is (`NavigatorState.pop<T>([T?
      // result])` torn off in `showDialog`, ws951).
      IrType? calleeSlot(DartType declared) {
        if (method == null ||
            declared is! TypeParameterType ||
            !fn.typeParameters.contains(declared.parameter) ||
            declared.nullability != Nullability.nullable) {
          return null;
        }
        final put = _type(
          method.substituteType(
            declared.withDeclaredNullability(Nullability.nonNullable),
          ),
        );
        return IrType(
          put.name,
          nullable: true,
          projected: true,
          arguments: put.arguments,
        );
      }

      IrExpr passedOn(String name, IrType held, DartType declared) {
        final arg = IrLocal(name)..rustType = held;
        final slot = calleeSlot(declared);
        return slot == null ? arg : coerce(arg, slot);
      }

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
            types: tearOffTypes ?? const <DartType>[],
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
              passedOn(
                params[i].name,
                params[i].type,
                fn.positionalParameters[i].type,
              ),
            for (final p in fn.namedParameters)
              passedOn(
                p.parameterName,
                _type(namedType(p.parameterName, p.type)),
                p.type,
              ),
          ],
          // ..with the types the `Instantiation` above supplied, which is
          // what makes a generic method tearable at all.
          typeArguments: [
            if (method != null)
              for (final t in tearOffTypes!) _type(t),
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
    return null;
  }
}
