part of '../frontend_kernel.dart';

// Dispatch runs: `==`, logic, `?:`, super calls, records and literals.
augment class KernelFrontend {
  /// `==`, and what a `dynamic` receiver is downcast to.
  ///
  /// One run of `_expressionRaw`; null for a node it does not answer for.
  IrExpr? _rawEqualityOrDynamic(Expression node) {
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
      // What the emitted Rust leaves in hand, which is not what Dart's
      // static type says: the receiver was narrowed to an `f64` right here,
      // so this is `f64`'s method (or the prelude's `DartDouble`). Untyped,
      // the `Let` the CFE binds an interpolation's argument in declared its
      // temporary at the static type -- `dynamic`, an `Rc<dyn Object>` --
      // over an `f64`, and nothing coerced between them (`NumberFormat
      // .format` and `_formatFixed`, ws895).
      final produced = _narrowedNumResult[node.name.text];
      if (produced != null) call.rustType = produced;
      return const {
            'round',
            'floor',
            'ceil',
            'truncate',
            'toInt',
          }.contains(node.name.text)
          ? (IrCast(call, 'i64')..rustType = const IrType('int'))
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
      final read = IrCall(asDouble, node.name.text, const []);
      final produced = _narrowedNumResult[node.name.text];
      if (produced != null) read.rustType = produced;
      return read;
    }
    return null;
  }

  /// Boolean logic, `?:`, type literals and type tests.
  ///
  /// One run of `_expressionRaw`; null for a node it does not answer for.
  IrExpr? _rawLogicOrTest(Expression node) {
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
    return null;
  }

  /// Super method calls, writing a local, records and collection literals.
  ///
  /// One run of `_expressionRaw`; null for a node it does not answer for.
  IrExpr? _rawSuperOrRecord(Expression node) {
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
      superOwners.add((ownerClass!, node.name.text, false));
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
    return null;
  }
}
