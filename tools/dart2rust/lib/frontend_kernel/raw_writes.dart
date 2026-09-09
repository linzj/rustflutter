part of '../frontend_kernel.dart';

// Dispatch runs: strings, `await`, `throw`, and writing a field.
augment class KernelFrontend {
  /// String concatenation, super properties, `await` and `throw`.
  ///
  /// One run of `_expressionRaw`; null for a node it does not answer for.
  IrExpr? _rawStringOrAwait(Expression node) {
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
    return null;
  }

  /// Writing a field, and `!`.
  ///
  /// One run of `_expressionRaw`; null for a node it does not answer for.
  IrExpr? _rawWrite(Expression node) {
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
    return null;
  }
}
