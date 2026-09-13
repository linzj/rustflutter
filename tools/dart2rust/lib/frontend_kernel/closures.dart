part of '../frontend_kernel.dart';

// Block values and cascades, what crosses an edge, and closures.
augment class KernelFrontend {
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
  ///
  /// The same set of bodies `lowerClass` lowers into the class, not just the
  /// ones the declaration still lists. The CFE copies a mixin's members into
  /// the anonymous *application* above the class and leaves the declaration
  /// hollow, so `class Counter extends Policy with CacheMixin` has no
  /// procedures of its own and `CacheMixin.invalidate` -- which does
  /// `_cache.remove(k)` -- was invisible here. The backend saw the mutation
  /// all the same (`_mutating` reads the lowered IR), made the trait method
  /// `&mut self`, and every call through the `Rc<dyn Policy>` was E0596 with
  /// nothing this census could have told it. The two have to read the same
  /// bodies (`WidgetOrderTraversalPolicy.invalidateScopeData`, the panic that
  /// stopped run900).
  bool _writesFieldInMethod(Class node) {
    final finder = _ThisWriteFinder();
    bool walk(Class owner) {
      for (final p in owner.procedures) {
        if (p.isStatic || p.isAbstract) continue;
        p.function.body?.accept(finder);
        if (finder.found) return true;
      }
      return false;
    }

    if (walk(node)) return true;
    var above = node.supertype;
    while (above != null && above.classNode.isAnonymousMixin) {
      if (walk(above.classNode)) return true;
      above = above.classNode.supertype;
    }
    // A mixin declaration TFA has emptied keeps its body in an application
    // (`_appliedProcedure`), and that is the body lowered into the trait.
    if (node.isMixinDeclaration) {
      for (final application in applications[node] ?? const <Class>[]) {
        if (walk(application)) return true;
      }
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
    final finder = _FinalFieldReads(_sharedFields, _fieldBehind);
    fn.accept(finder);
    // `DART2RUST_TRACE_CARRIED` names the field that made this closure
    // demand `this`. "A closure captures `this`" never said which field
    // forced it, and the answer here was one word: `_updaters(Procedure)`
    // -- a mixin's field arrives as the accessor the CFE left in its place
    // (see `FieldBehind`).
    if (Platform.environment['DART2RUST_TRACE_CARRIED'] == '1') {
      stderr.writeln(
        'TRACE_CARRIED demanding=${use.demandingBeyondFields} '
        'allCarried=${finder.allCarried} '
        'carried=${finder.fields.keys.join(",")} '
        'refused=${finder.refused.join(",")} '
        'shared=${_sharedFields.join(",")}',
      );
    }
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

  /// Whether an expression is `this`, or a chain of field reads from it.
  ///
  /// A null check on the way (`this.child!.hitTest`) reads the same object:
  /// the tear-off of it was rooted nowhere -- not held, not bound -- and
  /// borrowed `this_` into a `'static` slot ("lifetime may not live long
  /// enough", `RenderSliverEdgeInsetsPadding.hitTestChildren`, ws1115).
  bool _rootedAtThis(Expression e) => switch (e) {
    ThisExpression() => true,
    InstanceGet(:final receiver) => _rootedAtThis(receiver),
    NullCheck(:final operand) => _rootedAtThis(operand),
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
}
