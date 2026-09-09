part of '../frontend_kernel.dart';

// `let`, writes, declarations, labels and loops: binding a value to a name.
augment class KernelFrontend {
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
  /// Whether `right` stores into the very place `left` read: the shape
  /// Kernel gives `x ??= v` (`let #t = x in #t == null ? x = v : #t`).
  static bool _storesInto(Expression left, Expression right) {
    bool sameReceiver(Expression a, Expression b) =>
        (a is ThisExpression && b is ThisExpression) ||
        (a is VariableGet && b is VariableGet && a.variable == b.variable);
    if (left is InstanceGet && right is InstanceSet) {
      return left.name == right.name &&
          sameReceiver(left.receiver, right.receiver);
    }
    if (left is StaticGet && right is StaticSet) {
      return left.target == right.target;
    }
    if (left is VariableGet && right is VariableSet) {
      return left.variable == right.variable;
    }
    return false;
  }

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
        // ..and where the two arms are of *one class* whose arguments
        // differ, the left's spelling wins only when the right can
        // actually go into it. `children ?? buttonItems` is a
        // `List<Widget>` and a `List<ContextMenuButtonItem>`; Dart calls
        // the whole a `List<Object>`, and mapped into the left's spelling
        // the elements were upcast to a `Widget` they do not implement
        // (`AdaptiveTextSelectionToolbar.build`, in both toolbars, ws943).
        final typeEnv = typeEnvironment;
        final rightFits =
            typeEnv == null ||
            leftType is! InterfaceType ||
            rightType is! InterfaceType ||
            typeEnv.isSubtypeOf(
              rightType.withDeclaredNullability(Nullability.nonNullable),
              leftType.withDeclaredNullability(Nullability.nonNullable),
            );
        final lub =
            leftType is InterfaceType &&
                resultType is InterfaceType &&
                (leftType.classNode != resultType.classNode || !rightFits) &&
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
          // `x ??= v`: the right side stores into the place the left read
          // (`IrIfNull.assignsLeft`).
          assignsLeft: _storesInto(value, right),
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
    // A temporary bound to `null`: nothing is bound. `null` has no place,
    // no identity and no effect, so reading the literal where the body
    // reads the variable is the same program -- and the binding was
    // actively wrong, because a variable of static type `Null` is spelled
    // `Option<Null>` while the slot the body puts it in wants its own
    // `Option<T>` (`registry!.onChanged(null)` through a projected `T?`,
    // `RawRadio._handleChanged`, ws957). Left to the literal, the slot
    // types it, as an argument written in place would be.
    if (_isNull(initial)) {
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

  /// One local declaration, wherever it is written.
  ///
  /// A `for`'s variables are `VariableDeclaration`s and not `Statement`s in
  /// this Kernel, so they cannot go through `statement` -- and the rule about
  /// what a declaration becomes should be in one place regardless.
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
          IrCall(_asListValue(_listReceiver(init.receiver)), 'clone', const []),
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

  /// A `dart:` method's parameter declared as one of the *method's own*
  /// type parameters: the slot is the type argument the call wrote out.
  ///
  /// The rest of a prelude callee's slots go by shape, because its Rust
  /// signature is its own (`_calleeTranslated`); this one does not, because
  /// the prelude is generic over exactly what Dart is -- `fold<R>(R
  /// initial, ..)` is `fold_dart<R>(initial: R, ..)` -- and Rust infers `R`
  /// from the value it is handed. A value narrower than the type argument
  /// therefore fixes `R` to the wrong thing: `borders.fold<
  /// EdgeInsetsGeometry>(EdgeInsets.zero, ..)` inferred `Rc<EdgeInsets>`
  /// and the combine, written at the trait, no longer fitted (E0631,
  /// `_CompoundBorder.dimensions`, ws959).
  ///
  /// Only a parameter spelled as the type parameter *itself*: a
  /// `FutureOr<T>?` or a function type over it is the prelude's own shape
  /// again (`Completer.complete`, `Iterable.map`).
  List<IrType?>? _ownParameterSlots(InstanceInvocation node) {
    final target = node.interfaceTarget;
    final fn = target.function;
    final declaring = target.enclosingClass;
    if (declaring == null ||
        declaring.enclosingLibrary.importUri.scheme != 'dart' ||
        fn.typeParameters.isEmpty ||
        node.arguments.types.length != fn.typeParameters.length) {
      return null;
    }
    IrType? slot(DartType t) {
      if (t is! TypeParameterType) return null;
      final at = fn.typeParameters.indexOf(t.parameter);
      if (at < 0) return null;
      try {
        return _type(node.arguments.types[at]);
      } on Unsupported {
        return null;
      }
    }

    final slots = [for (final p in fn.positionalParameters) slot(p.type)];
    return slots.any((s) => s != null) ? slots : null;
  }

  /// The positional slots a `dart:` call's arguments go into: the narrow
  /// element of a typed list, and the method's own type arguments.
  List<IrType?>? _preludeSlots(InstanceInvocation node) {
    final narrow = _narrowSlots(node);
    final own = _ownParameterSlots(node);
    if (narrow == null) return own;
    if (own == null) return narrow;
    return [
      for (var i = 0; i < narrow.length; i++)
        narrow[i] ?? (i < own.length ? own[i] : null),
    ];
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

  /// The `E` of a receiver this compiler spells `Rc<dyn DartIterable<E>>`:
  /// `dart:core`'s own `Iterable<E>` written outright, or a type parameter
  /// bounded by one, which `_type` spells at that bound (`T extends
  /// Iterable<E>` in `collection`'s `_UnorderedEquality`; the
  /// iterablebound fixture). Null for anything else -- a `T extends
  /// List<E>` included, since a `List` is the `Vec` a body reads already.
  DartType? _iterableSpelling(DartType? type) {
    var seen = type;
    var hops = 0;
    while (seen is TypeParameterType && hops++ < 8) {
      if (!_spelledAsBound(seen.parameter)) return null;
      seen = seen.parameter.bound;
    }
    if (seen is InterfaceType &&
        seen.classNode.name == 'Iterable' &&
        seen.classNode.enclosingLibrary.importUri.scheme == 'dart' &&
        seen.typeArguments.length == 1) {
      return seen.typeArguments.single;
    }
    return null;
  }

  IrExpr _listReceiver(Expression e, [String? member]) {
    // A mutating member's receiver as it is: an erased `List<ChildType>`
    // read as a `List<Sliver>` is a narrowing *copy*, and `children.add
    // (x)` pushed into it (the erased tear-off fixture, ws528). The element
    // goes in as the slot's type and rustc upcasts it to the erased one.
    if (member != null && mutatingListNames.contains(member)) {
      return expression(e);
    }
    final static = _staticType(e);
    // `dart:core`'s own `Iterable<E>`, which is a `Rc<dyn DartIterable<E>>`
    // here (`type()`): its members are written over a list, so the read is
    // the list.
    //
    // The value as it stands, never through `_receiver`: that coerces to
    // the *static* type, and a `where` chain -- a `Vec` the prelude hands
    // back -- would be boxed into the handle only to be materialised
    // again on the same line (`items.where(..).length`, the iterableread
    // fixture).
    final iterableElement = _iterableSpelling(static);
    if (iterableElement != null) {
      final value = expression(e);
      final handle = value.rustType;
      // The list Dart's members are written over. Materialised at the
      // *handle's* own element -- `dart_to_list` on a `Rc<dyn
      // DartIterable<E>>` gives a `Vec<E>` for that same `E` -- and then
      // narrowed to the element Dart names, which is not always the same
      // one: an erased `Iterable<ChildType>` read through a mixin's trait
      // hands out `RenderObject` where Dart says `RenderBox`, and
      // everything after this is written in Dart's terms
      // (`_RenderChip.visitChildren`, whose `forEach` adapter was built
      // for `RenderBox`; five of them at ws908).
      final wanted = IrType('List', arguments: [_typeNested(iterableElement)]);
      if (handle?.name == 'Iterable') {
        final element = handle!.arguments.length == 1
            ? handle.arguments.single
            : wanted.arguments.single;
        final listed = IrCall(value, 'dart_to_list', const [])
          ..rustType = IrType('List', arguments: [element]);
        return coerce(listed, wanted);
      }
      // ..and a value that is not one already goes in as `_receiver`
      // sends it, into the list rather than the handle: a `dynamic`
      // reaching an `Iterable` receiver is the prelude's checked
      // conversion (`AssetManifest.listAssets`, ws908).
      if (!coerceByType) return value;
      try {
        return coerce(value, wanted);
      } on Unsupported {
        return value;
      }
    }
    final lowered = _receiver(e);
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

  /// A value read as a list: an `Iterable<T>` is a `Rc<dyn DartIterable<T>>`
  /// since ws908, and `dart_iter` -- like every Rust iterator -- starts at
  /// one. Nothing when the value is a list already.
  IrExpr _asListValue(IrExpr value) {
    final handle = value.rustType;
    if (handle == null ||
        handle.name != 'Iterable' ||
        handle.arguments.length != 1) {
      return value;
    }
    return IrCall(value, 'dart_to_list', const [])
      ..rustType = IrType('List', arguments: handle.arguments);
  }

  /// Whether `have` is `want` with an erased parameter still at its bound:
  /// the same class, the same arity, and every argument either the same or
  /// a top type where `want` names something else.
  bool _sameButErased(IrType? have, IrType want) {
    if (have == null || have.name != want.name) return false;
    if (have.arguments.length != want.arguments.length) return false;
    if (have.arguments.isEmpty) return false;
    for (var i = 0; i < have.arguments.length; i++) {
      final a = have.arguments[i];
      final b = want.arguments[i];
      final top =
          (a.name == 'Object' || a.name == 'dynamic') && a.arguments.isEmpty;
      if (!top && !sameRust(a, b)) return false;
    }
    return true;
  }

  /// `keepErased`: the call this receiver is for hands its result back from
  /// the bound (`_throughReceiver` typed it `dynamic`, and the slot put a
  /// `from_dynamic` around it). Casting the receiver back to the
  /// instantiation Kernel's substitution wrote would make the callee hand
  /// back its own `T` instead, under a conversion written for the erased
  /// spelling -- `item.tween.transform(t)` in `TweenSequence._evaluateAt`,
  /// which is the one site the erased-instantiation cast broke (ws934).
  IrExpr _receiver(Expression e, {bool keepErased = false}) {
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
      final want = _type(static);
      if (keepErased && _sameButErased(lowered.rustType, want)) return lowered;
      final out = coerce(lowered, want);
      return out;
    } on Unsupported {
      return lowered;
    }
  }
}
