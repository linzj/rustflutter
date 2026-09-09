part of '../frontend_kernel.dart';

augment class KernelFrontend {
  // -- Coercion by type ------------------------------------------------------
  //
  // One rule for a value entering a slot: compare the value's Rust type
  // (`IrExpr.rustType`) with the slot's, and adapt the difference --
  // an `Option` layer, a scalar widening, a handle up or down the trait
  // hierarchy, a value put behind a handle, a collection rebuilt element by
  // element. The shape rules in `_widened` below each did one of these for
  // one syntactic shape; by ws355 there were 26 of them and they had begun
  // to disagree. This replaces them as it proves to cover them.

  Map<String, Class>? _classesByName;

  /// The static and top-level fields some body mutates in place: the
  /// receivers of a collection mutator (`mutatingListNames`), of a field
  /// write, or a `List`/`Set` argument a callee fills -- also through a
  /// cascade's `let #t = field in #t.add(..)`. Once, over every translated
  /// library of the component: a library may fill another's.
  late final Set<Field> _mutatedStatics = () {
    final finder = _StaticFillFinder(this);
    final component = library.enclosingComponent;
    if (component != null) {
      for (final l in component.libraries) {
        if (!_translatedLibrary(l)) continue;
        l.accept(finder);
      }
    } else {
      library.accept(finder);
    }
    return finder.found;
  }();

  bool _translatedLibrary(Library l) {
    final uri = l.importUri;
    return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
  }

  Class? _classNamed(String name) {
    final index = _classesByName ??= () {
      final out = <String, Class>{};
      final component = library.enclosingComponent;
      if (component != null) {
        for (final l in component.libraries) {
          for (final c in l.classes) {
            out.putIfAbsent(c.name, () => c);
          }
        }
      }
      for (final c in library.classes) {
        out[c.name] = c;
      }
      return out;
    }();
    return index[name];
  }

  bool _isTraitName(String name) {
    if (const {
      'Object',
      'dynamic',
      'Comparable',
      'DartIterator',
    }.contains(name)) {
      return true;
    }
    if (scalarNames.contains(name) || collectionNames.contains(name)) {
      return false;
    }
    // This library's own class first: `dart:ui`'s `Gradient` is a struct
    // while `package:flutter`'s is abstract, and the name alone said
    // trait (`Gradient as Rc<dyn Object>` on a value, ws456).
    final c = _classNamed(name);
    if (c != null && identical(c.enclosingLibrary, library)) {
      return _translatedClass(c) && _abstractLike(c);
    }
    if (abstractElsewhere.contains(name)) return true;
    return c != null && _translatedClass(c) && _abstractLike(c);
  }

  bool _isCountedName(String name) {
    final known = elsewhere[name];
    if (known != null) return known.counted;
    final c = _classNamed(name);
    return c != null && _translatedClass(c) && _countedClass(c);
  }

  /// Whether `c` is counted, decided once per class: the rule reads
  /// `_sharedFields`, which is the class *being lowered*'s, and asked of
  /// another class from inside a method it answered by the wrong fields
  /// (`Semantics` boxed twice from `routes.dart`, ws512).
  final _countedCache = <Class, bool>{};

  bool _countedClass(Class c) => _countedCache.putIfAbsent(c, () {
    final saved = _sharedFields;
    _sharedFields = _closureFields(c);
    try {
      return _closureCallsMethod(c);
    } finally {
      _sharedFields = saved;
    }
  });

  bool _isStructName(String name) {
    if (_isTraitName(name) || scalarNames.contains(name)) return false;
    final c = _classNamed(name);
    return c != null && _translatedClass(c) && !c.isEnum;
  }

  bool _isEnumName(String name) {
    final c = _classNamed(name);
    return c != null && _translatedClass(c) && c.isEnum;
  }

  bool _isBelow(String sub, String sup) {
    final a = _classNamed(sub);
    final b = _classNamed(sup);
    final hierarchy = typeEnvironment?.hierarchy;
    if (a == null || b == null || hierarchy == null) return false;
    return a == b || hierarchy.isSubInterfaceOf(a, b);
  }

  // The rule itself lives in `coerce.dart`; this class is its `TypeWorld`.
  @override
  bool isTrait(String name) => _isTraitName(name);

  @override
  bool isCounted(String name) => _isCountedName(name);

  @override
  bool isStruct(String name) => _isStructName(name);

  @override
  bool isEnum(String name) => _isEnumName(name);

  @override
  bool isBelow(String sub, String sup) => _isBelow(sub, sup);

  @override
  bool isGenericValueStruct(String name) {
    final c = _classNamed(name);
    return c != null && !_closureCallsMethod(c) && c.typeParameters.isNotEmpty;
  }

  static IrType _nonNull(IrType t) => nonNull(t);

  static String _normalName(String name) => normalName(name);

  /// `value`, adapted to `slot`: see `coerceInto`.
  @override
  bool isTypeParameter(String name) {
    final member = _member;
    final fn = member is Procedure
        ? member.function
        : member is Constructor
        ? member.function
        : null;
    return (fn?.typeParameters.any((p) => p.name == name) ?? false) ||
        (_lowering?.typeParameters.any((p) => p.name == name) ?? false);
  }

  IrExpr coerce(IrExpr value, IrType slot, {bool inClosure = false}) =>
      coerceInto(value, slot, this, inClosure: inClosure);

  IrExpr _widened(
    Expression value,
    DartType? param,
    IrExpr lowered, {
    IrType? slotIr,
  }) {
    // The callee flag (`_slotTranslated`) is about this slot; whatever is
    // lowered underneath -- a literal's entries against the slot's element
    // types -- fills slots of its own, translated ones.
    final translated = _slotTranslated;
    final prelude = _slotPrelude;
    _slotTranslated = true;
    _slotPrelude = false;
    try {
      // ..except into a prelude callee's bare `Function` slot, which
      // takes the function *object* (see `_calleeTranslated`).
      return _widenedInto(
        value,
        param,
        lowered,
        translated:
            translated &&
            !(prelude && lowered is IrClosure && !_bareFunctionType(param)),
        prelude: prelude,
        slotIr: slotIr,
      );
    } finally {
      _slotTranslated = translated;
      _slotPrelude = prelude;
    }
  }

  /// A `let` is its body: the type flow analysis folds `a ?? b` with an
  /// always-null `a` into `let #t = a in b`, and the tear-off inside is
  /// what the slot takes (`requestFocusCallback ?? FocusTraversalPolicy.
  /// defaultTraversalRequestFocusCallback`, run523).
  static Expression _throughLets(Expression e) {
    var out = e;
    while (out is Let) {
      out = out.body;
    }
    return out;
  }

  /// A static tear-off into a slot whose type keeps named parameters: an
  /// adapter taking the type's (sorted) order and calling in the
  /// declaration's (see `_widenedInto`), or null when the orders agree.
  IrExpr? _namedOrderAdapter(
    Expression value,
    DartType? param,
    IrExpr lowered,
  ) {
    final bare = _throughLets(value);
    // ..a constructor's the same way (`RoundedRectangleBorder.new` as a
    // `ShapeBorder Function({side, borderRadius})`, ws570).
    final torn = bare is StaticTearOff
        ? bare.target
        : bare is ConstantExpression && bare.constant is TearOffConstant
        ? (bare.constant as TearOffConstant).target
        : null;
    final tornFunction = torn?.function;
    if (torn == null ||
        tornFunction == null ||
        param is! FunctionType ||
        param.namedParameters.isEmpty ||
        param.positionalParameters.length !=
            tornFunction.positionalParameters.length) {
      return null;
    }
    final declared = [
      for (final n in tornFunction.namedParameters) n.parameterName,
    ];
    final byType = [for (final n in param.namedParameters) n.name];
    if (declared.length != byType.length ||
        !declared.toSet().containsAll(byType) ||
        _sameOrder(declared, byType)) {
      return null;
    }
    final params = <IrParam>[];
    final positional = <IrExpr>[];
    for (var i = 0; i < param.positionalParameters.length; i++) {
      final name = '__a$i';
      params.add(IrParam(name, _paramType(param.positionalParameters[i])));
      positional.add(IrLocal(name));
    }
    final byName = <String, IrExpr>{};
    for (final n in param.namedParameters) {
      final name = '__n_${n.name}';
      params.add(IrParam(name, _paramType(n.type)));
      byName[n.name] = IrLocal(name);
    }
    IrType? slot;
    try {
      slot = _type(param);
    } on Unsupported {
      slot = null;
    }
    // The value bound first and moved in: emitted inside the closure it
    // borrowed the constructor's parameter (`request_focus_callback` in
    // the `let` the analysis left, "does not live long enough", run524).
    return IrBlockValue(
      [IrLocalDecl('__f', null, lowered)],
      IrCall(
        IrClosure(
          params,
          // ..the result into the slot's return: a constructor's instance
          // as the trait the slot returns (`Rounded` as a `dyn Shape`).
          IrReturn(
            coerce(
              IrCallValue(IrLocal('__f'), [
                ...positional,
                for (final n in tornFunction.namedParameters)
                  byName[n.parameterName]!,
              ])..rustType = _functionRefType(torn)?.returns,
              _type(param.returnType),
            ),
          ),
          _type(param.returnType),
          locals: const ['__f'],
        ),
        '!rc',
        const [],
      ),
    )..rustType = slot;
  }

  IrExpr _widenedInto(
    Expression value,
    DartType? param,
    IrExpr lowered, {
    required bool translated,
    bool prelude = false,
    IrType? slotIr,
  }) {
    // A literal into a collection slot of other element types is lowered
    // again against those: see `_mapLiteral`.
    // ..not into a prelude callee's slot whose element types are top
    // types: `List.unmodifiable(Iterable)`, `Set.removeAll(Iterable<
    // Object?>)` are generic over what they are given, and a `[3, 1, 2]`
    // re-lowered as `Vec<Rc<dyn Object>>` fit neither (ws580).
    final rawSlot =
        !translated &&
        param is InterfaceType &&
        param.typeArguments.isNotEmpty &&
        param.typeArguments.every(_isTopType);
    if (param is InterfaceType &&
        param.nullability != Nullability.nullable &&
        !rawSlot) {
      final args = param.typeArguments;
      if (value is MapLiteral &&
          param.classNode.name == 'Map' &&
          args.length == 2 &&
          (args[0] != value.keyType || args[1] != value.valueType)) {
        return _mapLiteral(value, args[0], args[1]);
      }
      if (value is ListLiteral &&
          (param.classNode.name == 'List' ||
              param.classNode.name == 'Iterable') &&
          args.length == 1 &&
          args[0] != value.typeArgument) {
        return _listLiteral(value, args[0]);
      }
      // ..and the AOT dill's spelling of one, `_GrowableList._literal3<
      // dynamic>(3, 1, 2)`: its elements lowered again against the slot's
      // element type (`List<int>.unmodifiable([3, 1, 2])`, ws580).
      final core = value is StaticInvocation ? _coreListLiteral(value) : null;
      if (core != null &&
          (param.classNode.name == 'List' ||
              param.classNode.name == 'Iterable') &&
          args.length == 1 &&
          args[0] != core) {
        final elements = (value as StaticInvocation).arguments.positional;
        return IrListLiteral([
          for (final e in elements)
            _widened(
              e,
              args[0],
              _withExpectedReturn(args[0], e, () => expression(e)),
            ),
        ], _type(args[0]));
      }
    }
    // ..and a record literal into a record slot of other field types: its
    // fields lowered again against the slot's.
    if (value is RecordLiteral &&
        param is RecordType &&
        param.named.isEmpty &&
        param.positional.length == value.positional.length &&
        param.positional.toString() != value.recordType.positional.toString()) {
      return _recordLiteral(value, param.positional);
    }
    // A tear-off's named-parameter order first, whatever its recorded
    // type says: the type is spelled sorted, the value is declared in its
    // own order, and no type rule can tell them apart (run523).
    final ordered = _namedOrderAdapter(value, param, lowered);
    if (ordered != null) lowered = ordered;
    if (coerceByType &&
        translated &&
        param != null &&
        lowered.rustType == null) {
      _untypedCensus.update(
        '${lowered.runtimeType}/${value.runtimeType}',
        (n) => n + 1,
        ifAbsent: () => 1,
      );
    }
    if (coerceByType &&
        translated &&
        param != null &&
        lowered.rustType != null) {
      IrType? slot = slotIr;
      if (slot == null) {
        try {
          slot = _type(param);
        } on Unsupported {
          slot = null;
        }
      }
      // A prelude callee's generic slot is its own Rust signature's
      // `Option<T>`, never the projected `<T as DartNullable>::Or`: the
      // projection is how *this* declaration spells its own edges, and a
      // callee's `T?` reached with this declaration's `T` put in is not one
      // of them (`ArgumentError.checkNotNull(other, 'other')` inside a
      // generic function, 29 at ws755).
      if (prelude && slot != null && slot.projected) {
        slot = IrType(slot.name, nullable: true, arguments: slot.arguments);
      }
      // A translated callee's `T?` is spelled `<T as DartNullable>::Or`
      // (`_edgeType`); `_type` spells the plain `Option<T>` a *body* works
      // with. At an argument edge the callee's own spelling is the slot --
      // without it the coercion below made the `Some(..)` a body wants and
      // returned, so the projection rule in this method's tail never ran
      // (`AsyncSnapshot.withData` through its redirecting `this._(..)`, and
      // `_OverridableActionMixin._getOverrideAction`; 7 at ws786).
      if (!prelude &&
          slot != null &&
          !slot.projected &&
          _argumentEdge &&
          _projectedSlot(param)) {
        slot = IrType(
          slot.name,
          nullable: true,
          projected: true,
          arguments: slot.arguments,
        );
      }
      if (slot != null) {
        // A local handed on is shared, as below: the clone comes first so
        // the coercion wraps the clone, not the local.
        var shared = lowered;
        if (value is VariableGet && _clonedWhenPassed(value.variable.type)) {
          shared = IrCall(lowered, 'clone', const [])
            ..rustType = lowered.rustType;
        }
        final coerced = coerce(shared, slot);
        if (!identical(coerced, shared)) return coerced;
      }
    }
    // A local handed on is shared in Dart and moved in Rust: `string` passed
    // to `StringCharacterRange` and then read again, `listener` moved into a
    // closure "in a previous iteration of loop" -- 21 `E0382`s. A clone of a
    // `String` or an `Rc` is the sharing Dart meant. A list or map is not
    // cloned: a copy of one would be a different list, and the aliasing
    // Dart meant is not something a clone can give.
    if (value is VariableGet && _clonedWhenPassed(value.variable.type)) {
      // A clone is its operand's type.
      lowered = IrCall(lowered, 'clone', const [])..rustType = lowered.rustType;
    }
    // Type flow analysis narrows a parameter to the one class that reaches
    // it -- `_pushClipPath(.., _NativePath path, ..)` -- and the caller
    // still holds a `Path`. Kernel writes no cast for that; the downcast
    // through `Any` is the same one `path as _NativePath` takes.
    // ..as the closure parameter was retyped, when it was.
    final given = value is VariableGet && _retyped.containsKey(value.variable)
        ? _retyped[value.variable]
        : _staticType(value);
    // A function whose parameter is *wider* than the slot's -- `callback`,
    // a `void Function(int?)`, handed to `_initFromAsset(.., void
    // Function(int))` -- is fine in Dart and a different `Fn` in Rust. An
    // adapter closure narrows each such parameter with `Some`.
    // ..and a function whose *result* is narrower than the slot's --
    // `_throwLocaleError`, a `String Function(String)`, as the default of a
    // `String? Function(String)` -- returns through `Some`. A static
    // tear-off (`canonicalizedLocale` in a list of fallbacks) as well as a
    // local.
    // A static function with *extra* optional named parameters as a value
    // of a narrower function type: `presentError = dumpErrorToConsole`,
    // where `dumpErrorToConsole(details, {forceReport = false})` fills a
    // `void Function(FlutterErrorDetails)` slot. The adapter passes the
    // defaults, as a call through the slot would.
    // ..and an *instance* tear-off the same way: `Timer(delay,
    // _controller.reverse)` tears off `reverse({double? from})` into a
    // `void Function()`, and `showOnScreen`'s four optional named
    // parameters land in a `VoidCallback` (8 at ws793). The target is a
    // Member either way, and the adapter calls the tear-off -- which
    // already holds its receiver -- with the defaults filled in.
    final tearOffTarget = switch (value) {
      ConstantExpression(:final constant) when constant is TearOffConstant =>
        constant.target,
      StaticTearOff(:final target) => target,
      InstanceTearOff(:final interfaceTarget) => interfaceTarget,
      _ => null,
    };
    if (tearOffTarget != null &&
        tearOffTarget.function != null &&
        param is FunctionType &&
        given is FunctionType &&
        param.namedParameters.isEmpty &&
        given.namedParameters.isNotEmpty &&
        param.positionalParameters.length ==
            given.positionalParameters.length) {
      final target = tearOffTarget;
      final params = <IrParam>[];
      final args = <IrExpr>[];
      for (var i = 0; i < param.positionalParameters.length; i++) {
        final name = '__a$i';
        params.add(IrParam(name, _paramType(param.positionalParameters[i])));
        args.add(IrLocal(name));
      }
      // In the *type's* order, which Kernel sorts and the lowered tear-off
      // takes its parameters in -- not the declaration's, which is the
      // order the defaults are written in (`show({int? which, String tag =
      // 'd', bool loud = false})` was called `(None, "d", false)` against
      // `|loud, tag, which|`).
      for (final n in given.namedParameters) {
        final declared = target.function!.namedParameters
            .where((p) => p.parameterName == n.name)
            .firstOrNull;
        final init = declared?.initializer;
        args.add(init == null ? _nullLiteral() : expression(init));
      }
      // What the adapter hands back goes into the slot's return by the
      // coercion rule, as any value entering a slot does: the call inside
      // returns what the *method* returns, and the adapter is declared to
      // return what the *slot* does. Returned raw, a `TickerFuture` sat in
      // the `Ok(..)` of a `void Function()` (`Timer(delay, _controller
      // .reverse)`, 2 at ws854).
      IrExpr returned = IrCallValue(lowered, args)
        ..rustType = _type(given.returnType);
      try {
        returned = coerce(returned, _type(param.returnType));
      } on Unsupported {
        // Nothing to say about the two types: as it was.
      }
      // ..and the handle on `this`, for the same reason as the locals: a
      // tear-off on `this` is a closure holding one, and an adapter that
      // does not hold its own reads the `__me` of whatever encloses it --
      // a borrow inside the `Rc<dyn Fn>` a `Timer` keeps, where the two
      // `RawTooltip` members stopped (`show()` is a local function, so the
      // enclosing `__me` is a capture; ws855).
      final torn = lowered is IrCall && lowered.name == '!rc'
          ? lowered.target
          : lowered;
      final adapter =
          IrCall(
              IrClosure(
                params,
                IrReturn(returned),
                _type(param.returnType),
                // An instance tear-off holds its receiver: the adapter
                // around it has to hold it too, or it borrows the local
                // the receiver came from and cannot outlive the call
                // (E0597, the tearopt fixture).
                locals: _freeLocalsIn(value, {}),
                holdsSelf: torn is IrClosure && torn.holdsSelf,
              ),
              '!rc',
              const [],
            )
            ..rustType = _type(
              param.withDeclaredNullability(Nullability.nonNullable),
            );
      // Into the slot as any other value is: returning here skips the
      // wrapping this method ends with, and a `VoidCallback?` field took a
      // bare `Rc<{closure}>` (`SemanticsNode.showOnScreen`, 4 at ws795).
      try {
        return coerce(adapter, slotIr ?? _type(param));
      } on Unsupported {
        return adapter;
      }
    }
    // A static tear-off into a slot whose type *keeps* named parameters: a
    // function value is called through its type, whose named parameters
    // Kernel sorts, while the function itself is declared in its own
    // order. `partLLibreFranklin(fontSize: 16, fontWeight: ..)` through
    // the tear-off landed a `FontWeight` in the `locale` slot (34 at ws321).
    // An adapter taking the type's order and calling in the declaration's.
    // Sorting every definition instead was 8789 (ws323).
    // (An untyped tear-off reaches here with its order adapted above.)
    if ((value is VariableGet ||
            value is StaticTearOff ||
            (value is ConstantExpression &&
                value.constant is StaticTearOffConstant)) &&
        param is FunctionType &&
        given is FunctionType &&
        param.namedParameters.isEmpty &&
        given.namedParameters.isEmpty &&
        param.positionalParameters.length ==
            given.positionalParameters.length) {
      var adapts = false;
      final params = <IrParam>[];
      final args = <IrExpr>[];
      bool narrows(DartType g, DartType p) =>
          g is InterfaceType &&
          p is InterfaceType &&
          g.classNode == p.classNode &&
          g.nullability == Nullability.nullable &&
          p.nullability != Nullability.nullable;
      for (var i = 0; i < param.positionalParameters.length; i++) {
        final p = param.positionalParameters[i];
        final g = given.positionalParameters[i];
        final name = '__a$i';
        params.add(IrParam(name, _paramType(p)));
        if (narrows(g, p)) {
          adapts = true;
          args.add(IrSome(IrLocal(name)));
        } else {
          args.add(IrLocal(name));
        }
      }
      final widensResult = narrows(param.returnType, given.returnType);
      if (adapts || widensResult) {
        final call = IrCallValue(lowered, args);
        // Shared, as a closure argument is: the slot is an `Rc<dyn Fn>`.
        return IrCall(
          IrClosure(
            params,
            IrReturn(widensResult ? IrSome(call) : call),
            _type(param.returnType),
            locals: value is VariableGet ? _freeLocalsIn(value, {}) : const [],
          ),
          '!rc',
          const [],
        );
      }
    }
    // The downcasts, the sharing into a trait handle, the dropped `as`
    // and the element upcasts that used to be spelled here one shape at a
    // time are `coerce`'s now (ws362): a typed value never reaches this
    // point needing one of them.
    final narrow = _narrowElement(param);
    // The *declared* type of a variable, not its promotion: `if (input is
    // Uint8List) return input;` still holds a `Vec<i64>`.
    final held = value is VariableGet ? value.variable.type : given;
    if (narrow != null &&
        held is InterfaceType &&
        _narrowElement(held) == null &&
        (held.classNode.name == 'List' ||
            held.classNode.name == '_GrowableList' ||
            held.classNode.name == '_List')) {
      final cast = IrCall(lowered, '!narrow', [
        IrLiteral(narrow, const IrType('raw')),
      ]);
      return param!.nullability == Nullability.nullable &&
              held.nullability != Nullability.nullable
          ? IrSome(cast)
          : cast;
    }
    // An `int` into a `double`/`num` slot: `howMany = truncated` (Dart's
    // `num` is an `f64` here) -- the cast the operators take.
    String? scalar(DartType? t) => t is InterfaceType ? t.classNode.name : null;
    if (scalar(param) == 'double' &&
        scalar(given) == 'int' &&
        given!.nullability != Nullability.nullable) {
      lowered = _toF64(lowered);
    }
    // A `num` parameter has no rule of its own -- `int.+(num other)` is
    // declared that way, and `index + 1` became `index + (1 as f64)`
    // (ws54, 85 in dart:ui alone) -- except on a number, where the
    // receiver says which number `num` is (`_numReceiver`).
    if (scalar(param) == 'num' &&
        _numReceiver == 'double' &&
        scalar(given) == 'int' &&
        given!.nullability != Nullability.nullable) {
      lowered = _toF64(lowered);
    }
    // A `List<String>` (any concrete element) into a `List<Object?>`: each
    // element shared into its `Rc<dyn Object>`.
    if (param is InterfaceType &&
        (param.classNode.name == 'List' ||
            param.classNode.name == 'Iterable') &&
        param.typeArguments.isNotEmpty &&
        param.typeArguments.first is InterfaceType &&
        (param.typeArguments.first as InterfaceType).classNode.name ==
            'Object' &&
        param.typeArguments.first.nullability == Nullability.nullable &&
        held is InterfaceType &&
        held.classNode.name == 'List' &&
        held.typeArguments.isNotEmpty &&
        held.typeArguments.first is InterfaceType &&
        (held.typeArguments.first as InterfaceType).classNode.name !=
            'Object' &&
        held.typeArguments.first.nullability != Nullability.nullable) {
      // A nullable list widens element by element under the `Option` --
      // unless the value in hand is not one. A read promoted by a null
      // check is recorded by its *declaration* and unwrapped where it is
      // used, and mapping over what is already a `Vec` emitted
      // `as_ref()`, which names two `AsRef` impls (E0282; `Object.hashAll(
      // fallback)` under `fallback == null ? null : ..`, 6 at ws861).
      final inHand = lowered.rustType;
      if (held.nullability == Nullability.nullable &&
          (inHand == null || inHand.nullable)) {
        return IrNullAware(
          lowered,
          IrCall(IrBound(), '!widen_object', const []),
        );
      }
      final widened = IrCall(lowered, '!widen_object', const []);
      return param.nullability == Nullability.nullable
          ? IrSome(widened)
          : widened;
    }
    // A typed list handed to a `List<int>` parameter widens its elements
    // (`Response.bytes(body)` with a `Uint8List`).
    // ..unless the slot it lands in is itself a narrow list -- a typed
    // list's own member, `bytes.setRange(a, b, other)` on `Uint8List`s
    // (`_narrowSlots`, run505).
    final slotNarrow =
        slotIr != null &&
        slotIr.name == 'List' &&
        slotIr.arguments.length == 1 &&
        const {
          'u8',
          'i8',
          'i16',
          'u16',
          'i32',
          'u32',
          'u64',
          'f32',
          'f64',
        }.contains(slotIr.arguments.single.name);
    if (!slotNarrow &&
        param is InterfaceType &&
        _narrowElement(param) == null &&
        (param.classNode.name == 'List' ||
            param.classNode.name == 'Iterable') &&
        param.typeArguments.isNotEmpty &&
        param.typeArguments.first is InterfaceType &&
        (param.typeArguments.first as InterfaceType).classNode.name == 'int' &&
        held is InterfaceType &&
        _narrowElement(held) != null &&
        _narrowElement(held) != 'f32' &&
        _narrowElement(held) != 'f64') {
      final widened = IrCall(lowered, '!widen', const []);
      return param.nullability == Nullability.nullable &&
              held.nullability != Nullability.nullable
          ? IrSome(widened)
          : widened;
    }
    if (param == null || param.nullability != Nullability.nullable) {
      // A nullable value into a non-nullable parameter: Dart would not have
      // compiled it, so type flow analysis proved it non-null and rewrote
      // the check away (`alpha ?? a` became `alpha`). The unwrap is that
      // proof, spelled (7 `f64 <= Option<f64>` shapes).
      if (param is InterfaceType &&
          given is InterfaceType &&
          given.nullability == Nullability.nullable &&
          given.classNode == param.classNode) {
        return _nullChecked(lowered);
      }
      return lowered;
    }
    // `Object?` and `dynamic` take anything: the widening there is into
    // `dyn Object`, a different coercion, and `Some(..)` around a `String`
    // handed to `StringBuffer.write(Object?)` was 57 `Display` errors.
    if (param is DynamicType ||
        (param is InterfaceType && param.classNode.name == 'Object')) {
      return lowered;
    }
    // A closure is wrapped like anything else now that a function-typed
    // parameter is `Rc<dyn Fn>` on both sides: `Option<Rc<dyn Fn(..)>>`
    // took a bare `Rc<{closure}>` 25 times in dart:ui.
    if (_isNull(value)) return lowered;
    final actual = _staticType(value);
    if (actual == null) return lowered;
    // Dart's static type says the value may be null; the *value in hand*
    // says whether it is an `Option` here. A nullable one the lowering
    // unwrapped -- a `!`, a downcast -- is no `Option`, and skipping the
    // wrap on the static type alone handed a bare `ShapeDecoration` to a
    // slot that takes one (`Decoration.lerp`, `BoxBorder.lerp`, 13 of the
    // 433 stubbed at ws745).
    final atHand = lowered.rustType;
    if (actual.nullability == Nullability.nullable &&
        (atHand == null ? !_unwrapped(lowered) : isNullable(atHand))) {
      return lowered;
    }
    if (actual is DynamicType || actual is NullType) return lowered;
    // A value already in the slot's `Option` is not put in it twice: a
    // collection's key or element goes through `_widened` a second time,
    // for the collection's own slot rather than the callee's declared
    // type, and the first pass did the wrapping (`m[k] = Box()` on a
    // `Map<int, Box?>` came out `Some(Some(..))`, ws694).
    final inHand = lowered.rustType;
    if (inHand != null && isNullable(inHand) && !inHand.projected) {
      IrType? spelled = slotIr;
      if (spelled == null) {
        try {
          spelled = _type(param);
        } on Unsupported {
          spelled = null;
        }
      }
      if (spelled != null &&
          isNullable(spelled) &&
          spelled.name == inHand.name) {
        return lowered;
      }
    }
    // A `T?` slot over a bare kept type parameter of the code here is the
    // projection `<T as DartNullable>::Or`, not an `Option<T>` -- as the
    // declarations spell it (`_edgeType`) -- and the value goes in by
    // `from_option`. A prelude callee's `FutureOr<T>?` is the same slot
    // (`Completer<T>.complete(value)` in `CachingAssetBundle.
    // loadStructuredBinaryData<T>`, run569).
    // Only at an argument edge: a body's own `T? x = ..` local is the
    // `Option<T>` a body works with (`DiagnosticsProperty.getChildren`,
    // `_retrieveNewRouteInformation`, +2 at ws570).
    final awaited = !translated && param is FutureOrType
        ? param.typeArgument.withDeclaredNullability(Nullability.nullable)
        : param;
    if (_argumentEdge &&
        awaited is TypeParameterType &&
        _projectedSlot(awaited)) {
      return coerce(
        lowered,
        IrType(awaited.parameter.name ?? 'T', nullable: true, projected: true),
      );
    }
    return IrSome(lowered);
  }

  static IrExpr _unboxed(IrExpr e) => e is IrClosure && e.boxed
      ? (IrClosure(
          e.params,
          e.body,
          e.returns,
          captures: e.captures,
          locals: e.locals,
          holdsSelf: e.holdsSelf,
        ))
      : e;

  FunctionNode? _calleeOf(Object param) {
    final parent = param is TreeNode ? param.parent : null;
    return parent is FunctionNode ? parent : null;
  }

  IrExpr _withBorrowing(
    Object? param,
    FunctionNode? callee,
    IrExpr Function() lower,
  ) {
    final was = _borrowedArgument;
    final kept = param != null && callee != null && _keeps(callee, param);
    if (kept) _borrowedArgument = false;
    try {
      final value = lower();
      // A function-typed parameter of translated code is spelled
      // `Rc<dyn Fn(..)>` whatever the callee does with it ("one spelling,
      // both sides", `type`), so lending the closure behind the handle is
      // always the wrong shape there -- `&*layout_child.clone()` where
      // `_computeSizes` declares the handle (`RenderFlex`,
      // `_RenderTheater.hitTestChildren`; 5 at ws815). The prelude's
      // `impl Fn` slots are the ones that want the loan, and the backend
      // gives it to them (`_preludeLends`).
      // The parameter is owned where it is kept, so the argument is boxed to
      // match: a closure's own type has no name.
      if (value is IrClosure) {
        // Typed as the closure it is (`rustType` carried), so the slot's
        // coercion sees it: a `Future<bool> Function(MethodCall)` tear-off
        // kept by `setMethodCallHandler` gets its result mapped into the
        // `Future<dynamic>` the slot declares (run447). Untyped from ws419
        // (2609 -> 2779 then) until the result rules -- `void` into
        // `Object`, a future into a future -- were in `coerce`.
        return IrClosure(
          value.params,
          value.body,
          value.returns,
          captures: value.captures,
          locals: value.locals,
          // Carried. Rebuilding a node without a flag it had is the shape
          // that lost `kept` in round 104 and `shared` in round 101 --
          // and `isAsync` here, until run430 (`await` in a closure that
          // was not `async`).
          holdsSelf: value.holdsSelf,
          boxed: true,
          isAsync: value.isAsync,
        )..rustType = value.rustType;
      }
      return value;
    } finally {
      _borrowedArgument = was;
    }
  }

  /// Whether the callee does anything with the parameter but call it.
  ///
  /// A body that is not there cannot be read, and "unknown" has to mean
  /// "keeps": guessing the other way is guessing that a borrow outlives its
  /// borrower.
  static final _keepsCache = <Object, bool>{};

  /// The IR name of a field: its Dart name, unless it is *private* and an
  /// ancestor in another library declares a private field of the same
  /// text -- Dart's privacy is per library, so those are two fields, and
  /// the flattened struct held one (`_InheritedNotifierElement._dirty`
  /// took `Element._dirty`'s place, started `false`, and the element
  /// never built: run554). The lower declaration is renamed with its
  /// library's tag; every reference resolves through the member, so the
  /// name is one everywhere.
  String _memberName(Member member) {
    final text = member.name.text;
    final start = member.enclosingClass;
    if (start == null) return text;
    // The declaring class: a copy in an anonymous application (the CFE's
    // `_X&Base&Mixin`, deduplicated or not) is the mixin's, and every copy
    // and the mixin's own trait spell the field alike.
    final home = start.isAnonymousMixin ? (_mixinOf(start) ?? start) : start;
    final key = (home, text);
    final known = _memberNames[key];
    if (known != null) return known;
    var out = text;
    final accessor = switch (member) {
      Field() => true,
      Procedure(:final isGetter, :final isSetter) => isGetter || isSetter,
      _ => false,
    };
    final static = switch (member) {
      Field(:final isStatic) => isStatic,
      Procedure(:final isStatic) => isStatic,
      _ => false,
    };
    if (member.name.isPrivate && accessor && !static) {
      final library = home.enclosingLibrary;
      // From the class itself, or -- for a mixin's member, whose own
      // superclass is `Object` -- from every application of the mixin.
      final starts = <Class>[
        start,
        if (home.isMixinDeclaration) ...?applications[home],
      ];
      if (Platform.environment['DART2RUST_TRACE_MEMBER'] == text) {
        stderr.writeln(
          'TRACE_MEMBER $text home=${home.name} (${library.importUri}) starts=${starts.map((c) => c.name).toList()}',
        );
      }
      outer:
      for (final from in starts) {
        var above = from.superclass;
        while (above != null) {
          if (above.enclosingLibrary != library &&
              _declaresPrivateAccessor(above, text)) {
            out = '${text}_${_libraryTag(library)}';
            break outer;
          }
          above = above.superclass;
        }
      }
    }
    _memberNames[key] = out;
    return out;
  }

  /// The mixin an anonymous application applies (its `mixedInType`, or
  /// the mixin among a deduplicated application's `implementedTypes`).
  static Class? _mixinOf(Class application) =>
      application.mixedInType?.classNode ??
      application.implementedTypes
          .map((st) => st.classNode)
          .where((c) => c.isMixinDeclaration)
          .firstOrNull;

  /// Whether `c` declares a non-static private field, getter or setter
  /// named `text` -- its own, or a mixin's copy the CFE put in it.
  static bool _declaresPrivateAccessor(Class c, String text) =>
      c.fields.any(
        (f) => f.name.text == text && f.name.isPrivate && !f.isStatic,
      ) ||
      c.procedures.any(
        (p) =>
            p.name.text == text &&
            p.name.isPrivate &&
            !p.isStatic &&
            (p.isGetter || p.isSetter),
      );

  final Map<(Class, String), String> _memberNames = {};

  /// A field's IR name from its target, or the written name for anything
  /// else (a setter's).
  String _fieldNameOf(Member? target, String text) =>
      target is Field ||
          (target is Procedure && (target.isGetter || target.isSetter))
      ? _memberName(target!)
      : text;

  /// A short tag for a library, from its URI's last segment.
  static String _libraryTag(Library library) {
    final segments = library.importUri.pathSegments;
    final last = segments.isEmpty ? 'lib' : segments.last;
    final base = last.endsWith('.dart')
        ? last.substring(0, last.length - 5)
        : last;
    return base.replaceAll(RegExp(r'[^A-Za-z0-9_]'), '_');
  }

  /// `IrMethod.typeParameterBounds`: each kept type parameter whose bound
  /// is a translated abstract class, with the bound spelled.
  Map<String, IrType> _traitBounds(FunctionNode function) {
    final out = <String, IrType>{};
    for (final p in function.typeParameters) {
      if (_erasedParameter(p)) continue;
      final bound = p.bound;
      if (bound is! InterfaceType ||
          bound.nullability == Nullability.nullable ||
          !_translatedClass(bound.classNode) ||
          !_abstractLike(bound.classNode) ||
          _scalarClass(bound.classNode)) {
        continue;
      }
      try {
        out[p.name ?? 'T'] = _type(bound);
      } on Unsupported {
        // Unspelled: no bound.
      }
    }
    return out;
  }

  /// Whether `callee` fills its `index`th positional parameter: a `List`
  /// or `Set` it adds to, removes from or writes into (`mutatingListNames`),
  /// directly or by lending it to a callee that does. Only a member with
  /// one body -- a static, a top-level function, a private method -- is
  /// asked: an override family would have to agree on the signature.
  /// Cached; a cycle (`_findModels` lending `results` to itself) is a
  /// "no" while it is being asked.
  bool _fillsParameter(Procedure callee, int index) {
    if (callee.isGetter || callee.isSetter) return false;
    // An abstract target is the interface's view of a member some class
    // does declare: the dispatch target's answer, not "no". A mixin's
    // private method reached through the mixin's trait was passed a copy,
    // and the copy is where the fill went
    // (`SlottedContainerRenderObjectMixin._addDiagnostics`, 5 at ws764).
    if (callee.isAbstract) {
      final owner = callee.enclosingClass;
      if (owner == null) return false;
      final env = typeEnvironment;
      final concrete = env == null
          ? null
          : env.hierarchy.getDispatchTarget(owner, callee.name);
      if (concrete is Procedure &&
          !concrete.isAbstract &&
          !identical(concrete, callee)) {
        return _fillsParameter(concrete, index);
      }
      // A mixin *declaration* keeps only the hollow signature -- the
      // dispatch target inside it is the abstract member itself -- and the
      // body the CFE moved into an application of the mixin is the one
      // that fills (`_appliedBody`, as the trait's default takes it). The
      // parameter came out `&mut` from the body and the call handed it a
      // copy (`SlottedContainerRenderObjectMixin._addDiagnostics`, 5 at
      // ws786).
      final applied = _appliedBody(owner, callee);
      if (applied != null) return _fillsParameter(applied, index);
      return false;
    }
    final own = callee.isStatic || callee.enclosingClass == null;
    if (!own && !callee.name.isPrivate) return false;
    final params = callee.function.positionalParameters;
    if (index >= params.length) return false;
    final param = params[index];
    final type = param.type;
    if (type is! InterfaceType ||
        type.nullability == Nullability.nullable ||
        !const {'List', 'Set'}.contains(type.classNode.name) ||
        type.classNode.enclosingLibrary.importUri.toString() != 'dart:core') {
      return false;
    }
    final key = (callee, index);
    final known = _fillsCache[key];
    if (known != null) return known;
    _fillsCache[key] = false;
    final finder = _FillFinder(param, this);
    callee.function.body?.accept(finder);
    _fillsCache[key] = finder.found;
    return finder.found;
  }

  final Map<(Procedure, int), bool> _fillsCache = {};

  bool _keeps(FunctionNode callee, Object param) {
    final known = _keepsCache[param];
    if (known != null) return known;
    // A constructor keeps what its initializers store (`this.onDismiss`,
    // `super(onTap: onTap)`): the body alone said nothing of a parameter
    // that never reaches it, and `_ModalBarrierGestureDetector(onDismiss:
    // handleDismiss)` was handed a borrow of the closure (run639).
    final parent = callee.parent;
    if (parent is Constructor) {
      for (final initializer in parent.initializers) {
        final walk = _ParameterEscapes(param);
        initializer.accept(walk);
        if (walk.escapes) return _keepsCache[param] = true;
      }
    }
    final body = callee.body;
    if (body == null) return _keepsCache[param] = true;
    final walk = _ParameterEscapes(param);
    body.accept(walk);
    return _keepsCache[param] = walk.escapes;
  }

  /// Whether a closure written here would land in a borrowed position.
  ///
  /// The backend emits a function-typed *parameter* as `impl Fn(..)`, so a
  /// closure passed to a call borrows and lives exactly as long as the call --
  /// which is all a closure reading `this` needs. A constructor argument is
  /// different: it is stored in the object being built, so it outlives
  /// everything here and stays refused.
  bool _borrowedArgument = false;

  static bool _sameOrder(List<String> a, List<String> b) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) return false;
    }
    return true;
  }

  /// A function's named parameters in the order its *type* lists them.
  static List<NamedParameter> _namedInTypeOrder(FunctionNode fn) =>
      [...fn.namedParameters]
        ..sort((a, b) => a.parameterName.compareTo(b.parameterName));

  /// Arguments to a function *value*, ordered by its type.
  ///
  /// Positional ones as written; then each named parameter of the type, in
  /// the type's (name) order, with what was supplied for it, or `None` when it
  /// is nullable and was left off. A function type carries no defaults, so an
  /// omitted non-nullable one has no value here and stops.
  List<IrExpr> _argumentsByType(Arguments node, FunctionType type) {
    // The function type's own parameter types widen the arguments, as a
    // callee's would: `onError(e, stack)` with `StackTrace? stackTrace`
    // takes `Some(stack)`.
    // ..as the function's Rust type spells them: a `T?` there is
    // projected, and the coercion rule converts into it.
    final ir = _type(type);
    final slots = ir.parameters ?? const <IrType>[];
    final out = [
      for (var i = 0; i < node.positional.length; i++)
        _argument(
          node.positional[i],
          null,
          i,
          type,
          i < slots.length ? slots[i] : null,
        ),
    ];
    final supplied = {for (final n in node.named) n.name: n.value};
    final named = [...type.namedParameters]
      ..sort((a, b) => a.name.compareTo(b.name));
    for (final param in type.namedParameters) {
      final value = supplied.remove(param.name);
      if (value != null) {
        final at = type.positionalParameters.length + named.indexOf(param);
        out.add(
          _widened(
            value,
            param.type,
            expression(value),
            slotIr: at < slots.length ? slots[at] : null,
          ),
        );
      } else if (param.type.nullability == Nullability.nullable) {
        out.add(_nullLiteral());
      } else {
        throw Unsupported(
          'omitted named argument `${param.name}` to a function value',
          _sample(node),
        );
      }
    }
    if (supplied.isNotEmpty) {
      throw Unsupported(
        'named argument `${supplied.keys.first}` not in the function type',
        _sample(node),
      );
    }
    return out;
  }

  List<IrExpr> _argumentList(
    Arguments node,
    FunctionNode? callee, [
    FunctionType? instantiated,
    List<DartType>? positionalTypes,
    Map<String, DartType>? namedTypes,
    List<IrType?>? positionalSlots,
  ]) {
    final positional = [
      for (var i = 0; i < node.positional.length; i++)
        _argument(
          node.positional[i],
          callee,
          i,
          instantiated,
          positionalSlots != null && i < positionalSlots.length
              ? positionalSlots[i]
              : null,
          positionalTypes != null && i < positionalTypes.length
              ? positionalTypes[i]
              : null,
        ),
    ];
    if (node.named.isEmpty && callee == null) return positional;
    if (callee == null) {
      throw Unsupported(
        'named argument with no resolved callee '
        '(${node.parent.runtimeType})',
        _sample(node),
      );
    }
    final supplied = {for (final n in node.named) n.name: n.value};
    // Kernel names a named parameter through `parameterName`.
    final out = <IrExpr>[...positional];
    for (final param in callee.namedParameters) {
      // The inspector's own argument, dropped along with the parameter it
      // fills. See `_inspectorOnly`.
      if (_inspectorOnly(param.parameterName)) {
        supplied.remove(param.parameterName);
        continue;
      }
      final value = supplied.remove(param.parameterName);
      if (value != null) {
        out.add(_namedArgument(value, param, namedTypes?[param.parameterName]));
        continue;
      }
      out.add(_omitted(param, node));
    }
    // A positional optional that was left off still needs its default.
    for (
      var i = positional.length;
      i < callee.positionalParameters.length;
      i++
    ) {
      out.insert(i, _omitted(callee.positionalParameters[i], node));
    }
    if (supplied.isNotEmpty) {
      throw Unsupported(
        'named argument `${supplied.keys.first}` not in the callee',
        _sample(node),
      );
    }
    return out;
  }

  IrExpr _omitted(FunctionParameter param, Node site) {
    final initializer = param.defaultValue;
    if (initializer != null) {
      // Kernel holds the default as an expression, already evaluated when it is
      // constant -- better than the analyzer front end, which could only read
      // the source text and accept the literals it recognised.
      // ..and widened into the parameter like a written argument: `Curves.
      // linear` filling a `Curve` is a `_Linear` value into an `Rc<dyn
      // Curve>` (92 `_Linear`, 109 `Cubic`).
      // Through `_intoDynamic`, which knows a prelude callee's `Object`
      // parameter is spelled as what it takes: `StringBuffer([content =
      // ''])` got its default as an `Rc<dyn Object>` (21 at ws327).
      // ..under the callee's gate, as a written argument is: a prelude
      // `dynamic` slot's `null` default is its `Null` object.
      final callee = _calleeOf(param);
      return _forCallee(
        callee,
        param.type,
        expression(initializer),
        (lowered) => _widened(initializer, param.type, lowered),
      );
    }
    if (param.type.nullability == Nullability.nullable) {
      // ..into the slot's Rust type: an omitted `Object? aspect` is the
      // `Null` object, not `None` (85 at ws501).
      // ..of a translated callee: a prelude callee's slot is its own Rust
      // signature (`_slotPrelude`), an `Option` where Dart says `Object?`.
      final absent = _nullLiteral();
      final slot = _recordedType(param.type);
      return slot == null || !_translatedCallee(_calleeOf(param))
          ? absent
          : coerce(absent, slot);
    }
    // An *interface* member carries no default -- `Canvas.clipRect({bool
    // doAntiAlias = true})` is abstract, and the default lives on the class
    // that implements it (`_NativeCanvas`). Found there, through the
    // hierarchy: 19 refusals for `doAntiAlias` and `debugLabel`.
    final fromImplementer = _defaultFromImplementer(param);
    if (fromImplementer != null) return expression(fromImplementer);
    // A `dart:` member's `int` parameter whose default the minimal dill
    // dropped (`String.startsWith(pattern, [int index = 0])`): zero, which
    // is what every such default in the core library is.
    final owner = _calleeOf(param)?.parent;
    if (owner is Member &&
        owner.enclosingLibrary.importUri.scheme == 'dart' &&
        param.type is InterfaceType &&
        (param.type as InterfaceType).classNode.name == 'int') {
      return IrLiteral('0', const IrType('int'));
    }
    throw Unsupported(
      'omitted parameter `${param.cosmeticName}` has no default',
      _sample(site),
    );
  }

  /// The closed world's subtype relation, computed once on first use.
  late final ClassHierarchySubtypes? _subtypes = () {
    final hierarchy = typeEnvironment?.hierarchy;
    if (hierarchy is! ClosedWorldClassHierarchy) return null;
    return hierarchy.computeSubtypesInformation();
  }();

  /// The default an implementing class gives an interface member's
  /// parameter, when the interface itself gives none.
  Expression? _defaultFromImplementer(FunctionParameter param) {
    final callee = _calleeOf(param);
    final member = callee?.parent;
    if (member is! Procedure || member.enclosingClass == null) return null;
    final subtypes = _subtypes;
    if (subtypes == null) return null;
    final name = param.cosmeticName ?? param.parameterName;
    for (final sub in subtypes.getSubtypesOf(member.enclosingClass!)) {
      for (final p in sub.procedures) {
        if (p.name.text != member.name.text || p.isStatic) continue;
        for (final candidate in [
          ...p.function.positionalParameters,
          ...p.function.namedParameters,
        ]) {
          final candidateName =
              candidate.cosmeticName ?? candidate.parameterName;
          if (candidateName == name && candidate.defaultValue != null) {
            return candidate.defaultValue;
          }
        }
      }
    }
    return null;
  }

  /// `MaterialLocalizations` written where a value goes: Dart's `Type`.
  ///
  /// The prelude has had `Type::of(name)` all along -- a name, because that is
  /// what upstream does with one: compares it, prints it, uses it as a map
  /// key. Not having this refused `Theme.of`, and `Theme.of` is called 268
  /// times. Four `of` methods -- Theme, MaterialLocalizations,
  /// CupertinoLocalizations and the gallery's own -- account for 464 of the
  /// 670 "called something that was not translated".
  /// The erased type parameters of an abstract class that its own bodies
  /// read as type literals (`T` as a value): each gets a getter on the
  /// trait, answered by every class under it (see `_typeArgumentGetters`).
  final _typeLiteralParamsCache = <Class, Set<TypeParameter>>{};

  Set<TypeParameter> _typeLiteralParams(Class c) =>
      _typeLiteralParamsCache.putIfAbsent(c, () {
        if (c.typeParameters.isEmpty) return const {};
        final finder = _TypeLiteralFinder(c.typeParameters.toSet());
        for (final m in c.members) {
          m.accept(finder);
        }
        return {
          for (final p in finder.found)
            if (_erasedParameter(p)) p,
        };
      });

  String _typeArgGetter(Class owner, TypeParameter p) =>
      '_typeArg${owner.name}${p.name ?? 'T'}';

  /// The getters for the erased type parameters read as literals: declared
  /// on the abstract class that reads them, and answered by every class
  /// under it with the argument its ancestry puts in (`_WidgetsLocalizations
  /// Delegate extends LocalizationsDelegate<WidgetsLocalizations>` answers
  /// `WidgetsLocalizations`).
  void _typeArgumentGetters(Class node, IrClass cls) {
    final hierarchy = typeEnvironment?.hierarchy;
    if (hierarchy == null || cls.isEnum) return;
    if (node.isAbstract || _isOpen(node)) {
      for (final p in _typeLiteralParams(node)) {
        cls.abstractMethods.add(
          IrMethod(
            _typeArgGetter(node, p),
            const [],
            const IrType('Type'),
            const IrBlock([]),
            isGetter: true,
          ),
        );
      }
    }
    final self = InterfaceType(node, Nullability.nonNullable, [
      for (final p in node.typeParameters)
        TypeParameterType(p, Nullability.nonNullable),
    ]);
    final seen = <Class>{};
    final work = <Class>[
      if (node.superclass != null) node.superclass!,
      for (final t in node.implementedTypes) t.classNode,
      if (node.mixedInType != null) node.mixedInType!.classNode,
    ];
    while (work.isNotEmpty) {
      final above = work.removeLast();
      if (!seen.add(above)) continue;
      work.addAll([
        if (above.superclass != null) above.superclass!,
        for (final t in above.implementedTypes) t.classNode,
        if (above.mixedInType != null) above.mixedInType!.classNode,
      ]);
      if (above.isAnonymousMixin || !_translatedClass(above)) continue;
      if (!(above.isAbstract || _isOpen(above))) continue;
      final used = _typeLiteralParams(above);
      if (used.isEmpty) continue;
      final asAbove = hierarchy.getInterfaceTypeAsInstanceOfClass(self, above);
      if (asAbove == null) continue;
      for (final p in used) {
        final index = above.typeParameters.indexOf(p);
        if (index < 0 || index >= asAbove.typeArguments.length) continue;
        final argument = asAbove.typeArguments[index];
        // An erased parameter of this class itself: left to the classes
        // under it, which know.
        if (argument is TypeParameterType &&
            _erasedParameter(argument.parameter)) {
          continue;
        }
        final IrExpr answer;
        try {
          answer = _typeLiteral(argument);
        } on Unsupported {
          continue;
        }
        cls.methods.add(
          IrMethod(
            _typeArgGetter(above, p),
            const [],
            const IrType('Type'),
            IrBlock([IrReturn(answer)]),
            isGetter: true,
          ),
        );
      }
    }
  }

  IrExpr _typeLiteral(DartType type) {
    // A type parameter's: what it was instantiated with, asked of the
    // Rust type (`dart_type_of::<T>()`); spelled as text it was the
    // Kernel node (`_inheritedElements[T]` in
    // `dependOnInheritedWidgetOfExactType<T>` found nothing, run555).
    // An erased one is its bound.
    if (type is TypeParameterType) {
      // A method's own parameter that travels as a value (`_typeValues`):
      // the hidden parameter, wherever in the body (a closure captures
      // it as a local, `_freeLocalsIn`).
      final member = _member;
      if (member is Procedure) {
        final index = member.function.typeParameters.indexOf(type.parameter);
        if (index >= 0 && _typeValues(member).contains(index)) {
          return IrLocal('__ty_$index')..rustType = const IrType('Type');
        }
      }
      if (_erasedParameter(type.parameter)) {
        // An erased parameter of an abstract class is answered by the
        // object: every class under it says what it put in (`Type get
        // type => T` in `LocalizationsDelegate<T>`, whose delegates all
        // answered `Object` and shared one map slot, run584).
        final owner = type.parameter.declaration;
        if (owner is Class &&
            (owner.isAbstract || _isOpen(owner)) &&
            _typeLiteralParams(owner).contains(type.parameter) &&
            _member != null &&
            !(_member is Procedure && (_member as Procedure).isStatic)) {
          return IrCall(
            IrThis(),
            _typeArgGetter(owner, type.parameter),
            const [],
            fails: true,
          )..rustType = const IrType('Type');
        }
        return _typeLiteral(type.parameter.bound);
      }
      return IrStaticCall(
        null,
        'dart_type_of',
        const [],
        typeArguments: [IrType(type.parameter.name ?? 'T')],
      )..rustType = const IrType('Type');
    }
    final name = type is InterfaceType ? type.classNode.name : '$type';
    return IrLiteral('Type::of("$name")', const IrType('raw'))
      ..rustType = const IrType('Type');
  }

  /// The static type of a constant, for the widening a literal's entry
  /// gets: an instance is its class, a literal its `dart:core` class.
  DartType _constantStaticType(Constant c) {
    final core = typeEnvironment?.coreTypes;
    return switch (c) {
      InstanceConstant() => InterfaceType(
        c.classNode,
        Nullability.nonNullable,
        c.typeArguments,
      ),
      IntConstant() when core != null => core.intNonNullableRawType,
      DoubleConstant() when core != null => core.doubleNonNullableRawType,
      BoolConstant() when core != null => core.boolNonNullableRawType,
      StringConstant() when core != null => core.stringNonNullableRawType,
      NullConstant() => const NullType(),
      // A collection constant is its class with its own element types: a
      // `const [BoxShadow(..)]` into a `List<BoxShadow>?` parameter was
      // `dynamic` here and never `Some`d (ws395).
      ListConstant() when core != null => InterfaceType(
        core.listClass,
        Nullability.nonNullable,
        [c.typeArgument],
      ),
      SetConstant() when core != null => InterfaceType(
        core.setClass,
        Nullability.nonNullable,
        [c.typeArgument],
      ),
      MapConstant() when core != null => InterfaceType(
        core.mapClass,
        Nullability.nonNullable,
        [c.keyType, c.valueType],
      ),
      RecordConstant() => c.recordType,
      _ => const DynamicType(),
    };
  }

  /// A constant, typed by its own class (`_constantStaticType`) so that a
  /// slot it goes into -- a `const <ShortcutActivator, Intent>{..}` entry --
  /// is coerced like any value (`coerce`).
  IrExpr _constant(Constant constant, Expression node) {
    final lowered = _constantRaw(constant, node);
    if (lowered.rustType == null) {
      try {
        lowered.rustType = _type(_constantStaticType(constant));
      } on Unsupported {
        // Left untyped.
      }
    }
    return lowered;
  }

  IrExpr _constantRaw(Constant constant, Expression node) {
    if (constant is TypeLiteralConstant) return _typeLiteral(constant.type);
    if (constant is SymbolConstant) {
      // `#name`, spelled the way `Type::of` is: a name and nothing else. The
      // library a private symbol belongs to is dropped -- see the prelude's
      // `Symbol` for what that costs, which in this program is nothing.
      return IrLiteral('Symbol::of("${constant.name}")', const IrType('raw'));
    }
    if (constant is DoubleConstant) {
      // `double.infinity` prints as `Infinity`, which the literal emitter then
      // suffixed into `Infinity.0` -- a name nothing declares, 183 times.
      // Rust spells these three, and only these three, differently.
      final value = constant.value;
      // `f64`, because Dart's `double` is one. These three said `f32` since
      // before round 96 changed the mapping, and nothing caught it: they only
      // appear where an infinity is written down, and every one of those sites
      // was already inside something that did not compile.
      if (value.isNaN) return IrLiteral('f64::NAN', const IrType('raw'));
      if (value == double.infinity) {
        return IrLiteral('f64::INFINITY', const IrType('raw'));
      }
      if (value == double.negativeInfinity) {
        return IrLiteral('f64::NEG_INFINITY', const IrType('raw'));
      }
      return IrLiteral('$value', const IrType('double'));
    }
    if (constant is IntConstant) {
      return IrLiteral('${constant.value}', const IrType('int'));
    }
    if (constant is BoolConstant) {
      return IrLiteral('${constant.value}', const IrType('bool'));
    }
    if (constant is StringConstant) {
      return IrLiteral(constant.value, const IrType('String'));
    }
    if (constant is NullConstant) {
      return _nullLiteral();
    }
    // Each element into the collection's element type, as a map constant's
    // entries are below.
    IrExpr element(Constant c, DartType into) {
      final value = ConstantExpression(c, _constantStaticType(c));
      return _widened(value, into, _constant(c, node));
    }

    if (constant is ListConstant) {
      final elementType = _type(constant.typeArgument);
      return IrListLiteral([
        for (final e in constant.entries) element(e, constant.typeArgument),
      ], elementType)..rustType = IrType('List', arguments: [elementType]);
    }
    if (constant is SetConstant) {
      // A const set: the prelude's `Set::from(vec![..])`, which is what a
      // set literal expression becomes too. 37 in the gallery's dill.
      return IrStaticCall('Set', 'from', [
        IrListLiteral([
          for (final e in constant.entries) element(e, constant.typeArgument),
        ], _type(constant.typeArgument)),
      ]);
    }
    if (constant is MapConstant) {
      // Each entry into the map's own types, as a map literal's are: the
      // `const <ShortcutActivator, Intent>{SingleActivator(..): ..}` tables
      // of `DefaultTextEditingShortcuts` put a `SingleActivator` where an
      // `Rc<dyn ShortcutActivator>` goes (74 "arguments incorrect" and 12
      // mismatched types on five statics in `widgets`).
      IrExpr entry(Constant c, DartType into) {
        final value = ConstantExpression(c, _constantStaticType(c));
        return _widened(value, into, _constant(c, node));
      }

      // Typed, so a slot of other element types adapts it: an empty
      // `const {}` into a copy's erased field (ws490).
      final keyType = _type(constant.keyType);
      final valueType = _type(constant.valueType);
      return IrMapLiteral(
        [
          for (final e in constant.entries)
            (
              entry(e.key, constant.keyType),
              entry(e.value, constant.valueType),
            ),
        ],
        keyType,
        valueType,
      )..rustType = IrType('Map', arguments: [keyType, valueType]);
    }
    if (constant is StaticTearOffConstant) {
      // A top-level or static function used as a value. Rust names the
      // function; nothing is captured, so none of the ownership question that
      // an *instance* tear-off raises applies here.
      // ..`dart:math`'s `max`/`min` by the prelude's free functions: a
      // *call* to them is the receiver's own `max` (an inherent method of
      // `f64` and of `Ord`), and an inherent method is no name to hand on
      // (`_sliderPartSizes.map(..).reduce(math.max)`, 7 at ws811).
      final torn = constant.target;
      final mathName = _mathValueNames[torn.name.text];
      if (mathName != null &&
          torn.enclosingClass == null &&
          torn.enclosingLibrary.importUri.toString() == 'dart:math') {
        return IrFunctionRef(null, mathName)..rustType = _functionRefType(torn);
      }
      return IrFunctionRef(
        constant.target.enclosingClass?.name,
        constant.target.name.text,
      )..rustType = _functionRefType(constant.target);
    }
    if (constant is InstantiationConstant) {
      // A generic function torn off at a type (`math.max<double>` handed to
      // `reduce`): the tear-off itself. Rust's function items are named,
      // not instantiated at a value -- the slot's own type is what says
      // which instantiation this is (7 refusals at ws811).
      return _constant(constant.tearOffConstant, node);
    }
    if (constant is ConstructorTearOffConstant) {
      // A constructor or factory used as a value: the associated function
      // the class has for it, by the name the backend declares it under
      // (`AssetManifest.loadFromAssetBundle` hands `_AssetManifestBin.
      // fromStandardMessageCodecMessage` to the bundle, run569).
      final target = constant.target;
      final cls = target.enclosingClass!;
      final text = target.name.text;
      final name = text.isEmpty
          ? 'new'
          : text == '_'
          ? 'new_'
          : text;
      return IrFunctionRef(_instanceName(cls), name)
        ..rustType = _functionRefType(target);
    }
    if (constant is InstanceConstant) {
      // `Zone.root` (the `_RootZone` constant): the prelude's `Zone::root()`.
      // Fields initialised with it -- every `_onXZone` in
      // `PlatformDispatcher` -- kept its constructor refused, and with it
      // the static `instance` every hook goes through.
      final constClass = constant.classNode;
      if (constClass.enclosingLibrary.importUri.toString() == 'dart:async' &&
          (constClass.name == '_RootZone' || constClass.name == 'Zone')) {
        return IrStaticCall('Zone', 'root', const []);
      }
      // An enum value arrives as an instance of the enum class carrying the
      // CFE's own `#index` and `_name` fields. Walking its constructor for
      // those was 1125 refusals reading `const instance missing #index` -- a
      // bug in work reported finished two rounds ago, and one only the Kernel
      // census could see, because the analyzer front end never meets this
      // shape at all.
      if (constant.classNode.isEnum) {
        for (final entry in constant.fieldValues.entries) {
          if (entry.key.asField.name.text != '_name') continue;
          final value = entry.value;
          if (value is StringConstant) {
            return IrStatic(
              constant.classNode.name,
              value.value,
              isEnumValue: true,
            );
          }
        }
        throw Unsupported('enum constant with no `_name`', _sample(node));
      }
      // `const Alignment(-1, -1)` arrives already evaluated, as the class plus
      // its field values. Rebuilding it as `Alignment::new(-1.0, -1.0)` reads
      // like the source and keeps the two front ends saying the same thing, so
      // it is still what happens when it can.
      //
      // It often cannot. A `const` instance never calls its constructor -- the
      // value is materialised -- so the constructor is unreachable and gets
      // shaken out of the dill: `_Linear` in curves.dart has none left at all,
      // and 2965 of `package:flutter`'s 5602 const instances are like it. Four
      // more shapes defeat the name matching even when a constructor survives:
      // a field renamed by a super constructor (`Offset(dx, dy)` stores `_dx`),
      // a redirect (`Duration`, `Color`), a class with only named constructors
      // (`EdgeInsets`), and the inspector's injected `$creationLocation`.
      //
      // So the constructor is an optimisation, and the field values are the
      // answer. They are what an InstanceConstant always carries, and they are
      // already the computed values -- there is nothing left for a constructor
      // to work out.
      final cls = constant.classNode;
      final byName = {
        for (final e in constant.fieldValues.entries)
          e.key.asField.name.text: e.value,
      };
      final rebuilt = _asConstructorCall(
        cls,
        byName,
        node,
        constant.typeArguments,
      );
      if (rebuilt != null) {
        return _isOpen(cls)
            ? IrUpcast(
                rebuilt,
                IrType(
                  cls.name,
                  arguments: _erasedArguments(cls, constant.typeArguments),
                ),
              )
            : rebuilt;
      }
      // Typed, so a slot of another type adapts it: `const
      // OptionalMethodChannel('flutter/menu')` into a `MethodChannel`
      // field wants the handle (`DefaultPlatformMenuDelegate`, run482).
      // Each field's value into the field's declared type by the one
      // rule: an omitted `Object?` field of a `const` instance holds the
      // `Null` object (`ThemeData`'s constants, ws511).
      IrExpr fieldValue(String name, Constant value) {
        final field = cls.fields.where((f) => f.name.text == name).firstOrNull;
        final lowered = _constant(value, node);
        if (field == null) return lowered;
        return _widened(
          ConstantExpression(value, _constantStaticType(value)),
          field.type,
          lowered,
        );
      }

      // With the constant's type arguments (the kept ones): a `const
      // Uninit<Sym>(..)` boxed into a `dynamic` slot had nothing else to
      // say what its `PhantomData` was (ws588).
      final instanceType = IrType(
        _instanceName(cls),
        // ..and its module, when the name is one two libraries declare:
        // three of them have a `_UnspecifiedTextScaler`, and the default
        // `const _UnspecifiedTextScaler()` of `TextPainter`'s `textScaler`
        // named none of them (`_SwitchPainter`, run700).
        module: _moduleQualifier(cls),
        arguments: _erasedArguments(cls, constant.typeArguments),
      );
      final instance = IrConstInstance(instanceType, {
        for (final entry in byName.entries)
          entry.key: fieldValue(entry.key, entry.value),
      })..rustType = instanceType;
      return _isOpen(cls)
          ? (IrUpcast(instance, IrType(cls.name))..rustType = IrType(cls.name))
          : instance;
    }
    if (constant is RecordConstant) {
      // A const record: the tuple a record literal is (`IrRecord`), each
      // field into the record type's own field type. The `switch` over
      // `axisDirection` in `ScrollPosition._updateSemanticActions` yields
      // `const (SemanticsAction.scrollDown, SemanticsAction.scrollUp)`
      // (run684). Named fields are refused as a literal's are.
      if (constant.named.isNotEmpty) {
        throw Unsupported('a const record with named fields', _sample(node));
      }
      final fields = constant.recordType.positional;
      return IrRecord([
        for (var i = 0; i < constant.positional.length; i++)
          element(constant.positional[i], fields[i]),
      ])..rustType = _type(constant.recordType);
    }
    throw Unsupported('constant ${constant.runtimeType}', _sample(node));
  }

  /// `Alignment::new(-1.0, -1.0)`, when the constructor is still there and its
  /// parameters name the fields one for one. Null when it is not.
  IrNew? _asConstructorCall(
    Class cls,
    Map<String, Constant> byName,
    Expression node,
    List<DartType> typeArguments,
  ) {
    final ctor = cls.constructors.where((c) => c.name.text.isEmpty).toList();
    if (ctor.length != 1) return null;
    // Positional **and** named, in that order, because that is the order
    // `_lowerConstructor` puts them in and the backend emits them
    // positionally. Walking only the positional ones emitted
    // `TextAlignVertical::new()` against a one-parameter constructor -- its
    // sole parameter is `{required this.y}`.
    final function = ctor.single.function;
    final names = [
      for (final p in function.positionalParameters) p.cosmeticName,
      for (final p in function.namedParameters) p.parameterName,
    ];
    // Every parameter has to name a field *and* every field has to be named by
    // a parameter. Without the second half, a constructor that sets a field in
    // its initialiser list would be called without that field's value and the
    // instance would silently be a different one.

    final args = <IrExpr>[];
    // Each constant into its parameter's type: `const _ModifierSidePair(
    // ModifierKey.altModifier, KeyboardSide.left)` against a `KeyboardSide?
    // side` is `Some(..)` (20 in `RawKeyboard`'s modifier map).
    final paramType = <String, DartType>{
      for (final p in function.positionalParameters) _paramName(p): p.type,
      for (final p in function.namedParameters) p.parameterName: p.type,
    };
    for (final name in names) {
      final value = byName[name];
      if (value == null) return null;
      var lowered = _constant(value, node);
      // ..the parameter's type at the constant's own instantiation: `const
      // WidgetStatePropertyAll<OutlinedBorder?>(StadiumBorder())` takes an
      // `OutlinedBorder?`, not the bare `T` (ws463).
      // ..the *kept* parameters only: an erased one's slot is its bound,
      // `Rc<dyn Object>`, whatever the constant instantiates it at, and
      // substituting `double` in put an `f64` where the constructor takes
      // an object (`const WidgetStatePropertyAll<double>(24.0)`, ws704).
      final declaredParam = paramType[name];
      final kept = [
        for (final p in cls.typeParameters)
          if (!_erasedParameter(p)) p,
      ];
      final keptArguments = [
        for (var i = 0; i < cls.typeParameters.length; i++)
          if (!_erasedParameter(cls.typeParameters[i])) typeArguments[i],
      ];
      final t = declaredParam == null
          ? null
          : cls.typeParameters.isNotEmpty &&
                cls.typeParameters.length == typeArguments.length &&
                kept.isNotEmpty
          ? Substitution.fromPairs(
              kept,
              keptArguments,
            ).substituteType(declaredParam)
          : declaredParam;
      // Into the parameter's type by the coercion rule, under the
      // constructor's gate as a written argument is: a prelude `dynamic`
      // slot's null is its `Null` object (`const FormatException(..)`).
      if (coerceByType && t != null && lowered.rustType != null) {
        if (_calleeTranslated(function, t)) {
          try {
            lowered = coerce(lowered, _type(t));
          } on Unsupported {
            // An unspelled slot: the value as it is.
          }
        }
        args.add(lowered);
        continue;
      }
      // A concrete constant into an abstract parameter is shared, as a
      // written argument would be: `Curves.linear` filling `Interval`'s
      // `curve` is a `_Linear` value where an `Rc<dyn Curve>` goes (39).
      if (t is InterfaceType &&
          value is InstanceConstant &&
          _abstractLike(t.classNode) &&
          !_abstractLike(value.classNode) &&
          t.classNode != value.classNode &&
          _translatedClass(t.classNode) &&
          _translatedClass(value.classNode) &&
          !_closureCallsMethod(value.classNode)) {
        lowered = IrCall(lowered, '!rc', const []);
      }
      final wraps =
          t != null &&
          t is! DynamicType &&
          t.nullability == Nullability.nullable &&
          !(t is InterfaceType && t.classNode.name == 'Object') &&
          value is! NullConstant;
      args.add(wraps ? IrSome(lowered) : lowered);
    }
    return IrNew(_constantType(cls, typeArguments), args);
  }

  /// The type of a rebuilt constant, type arguments and all.
  ///
  /// Dropped, `const Pair<int, double>(3, 4.5)` came out as `Pair::new(..)`
  /// against the analyzer front end's `Pair::<i64, f32>::new(..)`. Both are
  /// valid Rust -- inference would have got there -- but the two front ends
  /// saying different things is the one thing the fixtures exist to catch.
  IrType _constantType(Class cls, List<DartType> typeArguments) => IrType(
    _instanceName(cls),
    arguments: _erasedArguments(cls, typeArguments),
    module: _moduleQualifier(cls),
  );

  /// Whether a class's type parameter is erased to its bound.
  ///
  /// Rust's trait parameters are invariant: `impl State<Scaffold> for
  /// ScaffoldState` is no `State<StatefulWidget>`, and `createState` has to
  /// return one (377 stubs naming `State<..>` at ws279). Dart's `State<T
  /// extends StatefulWidget>` only ever *narrows* `T` in subclasses, so in
  /// a closed world the parameter can go: the trait is `State`, `T` inside
  /// it is `StatefulWidget`, and a read typed narrower than that is a
  /// downcast (`_narrowedRead`). A parameter is erased when its bound is a
  /// translated abstract-like class -- `Action<T extends Intent>`,
  /// `ParentDataWidget<T extends ParentData>`, `GlobalKey<T extends
  /// State>` -- and not when the bound is `Object` or a scalar, where the
  /// parameter is a real type variable (`Tween<T>`, `Animation<T>`).
  static bool _scalarClass(Class c) =>
      const {'String', 'int', 'double', 'bool', 'num'}.contains(c.name) &&
      c.enclosingLibrary.importUri.toString() == 'dart:core';

  /// The `dart:` libraries' top-level functions the prelude provides, by
  /// library and name. `scheduleMicrotask` runs its callback now (see the
  /// prelude); `print` is Dart's, to stdout, through the Object
  /// protocol's `toString` (google_fonts' error path, run561).
  /// Each with the Rust slots its arguments are coerced into, or none.
  /// `dart:collection`'s extension getters on iterables, by the CFE's
  /// name for the lowered static, to the prelude's list method.
  static const _coreExtensionMethods = <String, Map<String, String>>{
    'dart:collection': {
      'IterableExtensions|get#firstOrNull': 'first_or_null',
      'IterableExtensions|get#lastOrNull': 'last_or_null',
      'IterableExtensions|get#singleOrNull': 'single_or_null',
      'IterableExtensions|elementAtOrNull': 'element_at_or_null',
    },
  };

  static const _coreTopLevel = <String, Map<String, (String, List<IrType>?)>>{
    'dart:core': {
      'print': ('dart_print', [IrType('dynamic')]),
    },
    'dart:async': {'scheduleMicrotask': ('_schedule_microtask', null)},
    'dart:convert': {'jsonDecode': ('json_decode', null)},
  };

  bool _erasedParameter(TypeParameter p) {
    if (!erase) return false;
    // Erasure is a property of the declarations this compiler writes: a
    // prelude class's parameter (`HashSet<E>`) is the prelude's own
    // generic, and dropping it left `Set::new()` with nothing to infer
    // `T` from once boxed into an `Object` slot (`InheritedModelElement.
    // updateDependencies`, run560).
    final owner = p.declaration;
    if (owner is Class && !_translatedClass(owner)) return false;
    // ..when its bound has a handle to erase to: a top type (`Rc<dyn
    // Object>`) or a translated trait. `RestorableEnum<T extends Enum>`
    // erased to a `dart:core` class this compiler does not spell took 107
    // crates down (ws520).
    if (covariantParameters.contains(p) && _erasableBound(p.bound)) {
      return true;
    }
    // An anonymous mixin application's parameter stands for the mixin's:
    // erased when that one is (`SlottedRenderObjectElement<SlotType>` kept
    // a `SlotType` nothing declared, ws315).
    final decl0 = p.declaration;
    if (decl0 is Class && decl0.isAnonymousMixin) {
      final mixedIn = decl0.mixedInType;
      if (mixedIn != null) {
        for (var j = 0; j < mixedIn.typeArguments.length; j++) {
          final a = mixedIn.typeArguments[j];
          if (a is TypeParameterType &&
              a.parameter == p &&
              j < mixedIn.classNode.typeParameters.length) {
            return _erasedParameter(mixedIn.classNode.typeParameters[j]);
          }
        }
      }
      // A deduplicated application (`dart:mixin_deduplication`) has no
      // `mixedInType`; the mixin is among its `implementedTypes`.
      for (final st in [
        if (decl0.supertype != null) decl0.supertype!,
        ...decl0.implementedTypes,
      ]) {
        for (var j = 0; j < st.typeArguments.length; j++) {
          final a = st.typeArguments[j];
          if (a is TypeParameterType &&
              a.parameter == p &&
              j < st.classNode.typeParameters.length) {
            return _erasedParameter(st.classNode.typeParameters[j]);
          }
        }
      }
      // No mapping found: judged by its own bound below, as before
      // (`ChildType extends RenderBox` on a mixin application, 54 at ws316).
    }
    // A factory carries its own copies of the class's parameters: erased
    // with them, or `global_key_new<T>` kept a `T` nothing could infer (87).
    final decl = p.declaration;
    if (decl is Procedure && decl.isFactory) {
      final cls = decl.enclosingClass;
      final i = decl.function.typeParameters.indexOf(p);
      return cls != null &&
          i >= 0 &&
          i < cls.typeParameters.length &&
          _erasedParameter(cls.typeParameters[i]);
    }
    // A closure's or local function's own type parameter: a Rust closure
    // cannot be generic, so it reads as its bound, as a generic function
    // *type* is instantiated at its bounds (`_type`) -- `<T extends
    // Object?>(settings, builder) => MaterialPageRoute<T>(..)` handed to
    // `WidgetsApp.pageRouteBuilder` named a `T` nothing declared (ws485).
    // ..declared by the closure itself in this Kernel (`FunctionExpression`,
    // `FunctionDeclaration`), or by its function node in another
    // (`pageRouteBuilder: <T>(..) => MaterialPageRoute<T>(..)` spelled a
    // `T` in `_MaterialAppState._buildWidgetApp`, ws503).
    final generic = p.declaration as TreeNode?;
    if (generic is FunctionExpression || generic is FunctionDeclaration) {
      return true;
    }
    if (generic is FunctionNode && generic.parent is! Member) return true;
    if (decl is! Class) return false;
    return _erasedCache.putIfAbsent(p, () {
      final bound = p.bound;
      if (bound is! InterfaceType) return false;
      if (bound.classNode.name != 'Object' &&
          bound.classNode.enclosingLibrary.importUri.scheme != 'dart' &&
          _abstractLike(bound.classNode)) {
        return true;
      }
      // An `Object`-bounded parameter of a trait-like class that some
      // subclass fixes to a concrete type: `AssetImage` is an
      // `ImageProvider<AssetBundleImageKey>` and stands where an
      // `ImageProvider<Object>` is wanted (121 bounds at ws313). A
      // parameter every subclass passes through (`Animation<T>`) stays.
      // Only a translated class: the prelude's `Sink<T>` keeps its
      // parameter (11 "missing generics" that killed a leaf crate, ws314).
      final uri = decl.enclosingLibrary.importUri.toString();
      // Gated (`DART2RUST_ERASE_OBJECT=1`): measured 4435 against 3995
      // at ws318 with every crate reached -- `Animation<double>`'s values
      // went behind `Rc<dyn Object>` and every read had to come back.
      return eraseObjectBounded &&
          bound.classNode.name == 'Object' &&
          (uri.startsWith('package:') || uri == 'dart:ui') &&
          _abstractLike(decl) &&
          _fixedBelow(decl, decl.typeParameters.indexOf(p));
    });
  }

  final Map<TypeParameter, bool> _erasedCache = {};

  /// Whether an erased parameter of this bound is spelled: a top type, or
  /// a translated abstract-like class (a trait object).
  bool _erasableBound(DartType bound) {
    if (bound is DynamicType) return true;
    if (bound is! InterfaceType) return false;
    final cls = bound.classNode;
    if (cls.name == 'Object' &&
        cls.enclosingLibrary.importUri.toString() == 'dart:core') {
      return true;
    }
    return _translatedClass(cls) && _abstractLike(cls);
  }

  /// Whether a subtype of `cls` supplies a concrete type (not one of its
  /// own parameters) for `cls`'s `i`th parameter.
  bool _fixedBelow(Class cls, int i) {
    final subtypes = _subtypes;
    final hierarchy = typeEnvironment?.hierarchy;
    if (subtypes == null || hierarchy == null || i < 0) return false;
    for (final sub in subtypes.getSubtypesOf(cls)) {
      if (sub == cls) continue;
      final asBase = hierarchy.getClassAsInstanceOf(sub, cls);
      if (asBase == null || i >= asBase.typeArguments.length) continue;
      final arg = asBase.typeArguments[i];
      if (arg is! TypeParameterType && arg is! DynamicType) {
        if (arg is InterfaceType && arg.classNode.name == 'Object') continue;
        return true;
      }
    }
    return false;
  }

  /// `type` as an instance of `base` the way Rust holds it: up the
  /// supertype clauses, each spelled with the declaring class's erased
  /// parameters at their bounds (`_atErasedBounds`) before `type`'s
  /// arguments go in. Null when `base` is not above `type`.
  InterfaceType? _asRustInstance(
    InterfaceType type,
    Class base, [
    int depth = 0,
  ]) {
    if (identical(type.classNode, base)) return type;
    if (depth > 40) return null;
    final cls = type.classNode;
    final substitution = Substitution.fromInterfaceType(type);
    for (final st in [
      if (cls.supertype != null) cls.supertype!,
      if (cls.mixedInType != null) cls.mixedInType!,
      ...cls.implementedTypes,
    ]) {
      final direct = _atErasedBounds(
        InterfaceType(st.classNode, Nullability.nonNullable, st.typeArguments),
        0,
      );
      final substituted = substitution.substituteType(direct);
      if (substituted is! InterfaceType) continue;
      final found = _asRustInstance(substituted, base, depth + 1);
      if (found != null) return found;
    }
    return null;
  }

  /// A type with each erased parameter (`_erasedParameter`) in it replaced
  /// by its bound, a few levels deep: the instantiation Rust holds for it.
  DartType _atErasedBounds(DartType t, int depth) {
    if (depth > 4) return t;
    if (t is TypeParameterType && _erasedParameter(t.parameter)) {
      final bound = t.parameter.bound;
      return _atErasedBounds(
        t.nullability == Nullability.nullable
            ? bound.withDeclaredNullability(Nullability.nullable)
            : bound,
        depth + 1,
      );
    }
    if (t is InterfaceType && t.typeArguments.isNotEmpty) {
      return InterfaceType(t.classNode, t.nullability, [
        for (final a in t.typeArguments) _atErasedBounds(a, depth + 1),
      ]);
    }
    return t;
  }

  /// A class's type arguments with the erased ones left out.
  ///
  /// As type arguments (`_nested`): a `T?` among them is projected, so
  /// `_SettingsListItemState<T?>()` with `T` bound to `double?` is the
  /// state over `double?` -- `Option<T>` there was `Option<Option<f64>>`,
  /// and the state's downcast of its widget found none (run687). The
  /// callers that lowered a class type did this themselves; the
  /// constructor, constant and cast sites did not.
  List<IrType> _erasedArguments(Class cls, List<DartType> arguments) => _nested(
    () => [
      for (var i = 0; i < arguments.length; i++)
        if (i >= cls.typeParameters.length ||
            !_erasedParameter(cls.typeParameters[i]))
          _type(arguments[i]),
    ],
  );

  /// `x is T`. Against a type parameter that is the operand's own type
  /// (`value is! T` on a `T?` in `Provider.of`) it asks only about null,
  /// which is all Rust's `T` can differ in; `null is T` is false for the
  /// non-nullable arguments the gallery passes (`of<EmailStore>`, and
  /// nothing `of<X?>`). Any other type parameter is asked by id in the
  /// backend (`dart_cast_any`).
  /// Whether a value of this type might be a future at run time: a
  /// `Future`, a `FutureOr`, a top type, a type parameter, or a class that
  /// implements `Future` (`SynchronousFuture`). Anything else, awaited, is
  /// a turn and the value.
  bool _couldBeFuture(DartType t) {
    if (t is FutureOrType || t is DynamicType || t is TypeParameterType) {
      return true;
    }
    if (t is! InterfaceType) return t is! NullType;
    final cls = t.classNode;
    if (cls.name == 'Object' &&
        cls.enclosingLibrary.importUri.toString() == 'dart:core') {
      return true;
    }
    final env = typeEnvironment;
    if (env == null) return true;
    return env.hierarchy.getTypeAsInstanceOf(t, env.coreTypes.futureClass) !=
        null;
  }

  IrExpr _isExpression(IsExpression node) {
    final asked = node.type;
    // A literal's runtime type is its static type: `<int?>[] is List<int>`
    // (provider's sound-mode probe) is Dart's subtyping, decided here.
    final operand = node.operand;
    // ..or the CFE's spelling of one, `_GrowableList<int?>(0)`: a
    // `dart:core` factory whose class is an implementation's.
    final coreFactory =
        operand is StaticInvocation &&
        operand.target.enclosingLibrary.importUri.toString() == 'dart:core' &&
        (operand.target.enclosingClass?.name.startsWith('_') ?? false);
    final literal =
        operand is ListLiteral ||
        operand is MapLiteral ||
        operand is SetLiteral ||
        coreFactory ||
        (operand is ConstantExpression &&
            (operand.constant is ListConstant ||
                operand.constant is MapConstant ||
                operand.constant is SetConstant));
    final env = typeEnvironment;
    if (literal && env != null && asked is InterfaceType) {
      final core = env.coreTypes;
      final DartType? on = switch (operand) {
        ListLiteral(:final typeArgument) => InterfaceType(
          core.listClass,
          Nullability.nonNullable,
          [typeArgument],
        ),
        SetLiteral(:final typeArgument) => InterfaceType(
          core.setClass,
          Nullability.nonNullable,
          [typeArgument],
        ),
        MapLiteral(:final keyType, :final valueType) => InterfaceType(
          core.mapClass,
          Nullability.nonNullable,
          [keyType, valueType],
        ),
        ConstantExpression(:final type) => type,
        StaticInvocation() => _staticType(operand),
        _ => null,
      };
      if (on is InterfaceType) {
        return IrLiteral(
          env.isSubtypeOf(on, asked) ? 'true' : 'false',
          const IrType('bool'),
        );
      }
    }
    if (asked is TypeParameterType && !_erasedParameter(asked.parameter)) {
      final on = _staticType(node.operand);
      if (on is TypeParameterType && on.parameter == asked.parameter) {
        return IrUnary('!', IrIsNull(expression(node.operand)));
      }
      if (node.operand is NullLiteral) {
        return IrLiteral('false', const IrType('bool'));
      }
    }
    // Against a method's own parameter that travels as a value
    // (`_typeValues`): the object asked by that value (`dart_is_type`).
    final member = _member;
    if (asked is TypeParameterType && member is Procedure) {
      final index = member.function.typeParameters.indexOf(asked.parameter);
      if (index >= 0 && _typeValues(member).contains(index)) {
        // A local is shared: boxed from a clone, the local stays.
        var operand = expression(node.operand);
        if (node.operand is VariableGet) {
          operand = IrCall(operand, 'clone', const [])
            ..rustType = operand.rustType;
        }
        return IrStaticCall(null, 'dart_is_type', [
          coerce(operand, const IrType('dynamic')),
          IrLocal('__ty_$index')..rustType = const IrType('Type'),
        ])..rustType = const IrType('bool');
      }
    }
    // A test Dart's own subtyping already answers: the operand's static
    // type is a subtype of the asked one, so every value it can hold is
    // one. The CFE writes these itself -- a record destructuring pattern
    // (`final (nextChild, topLeftChild) = flipMainAxis ? .. : ..;` in
    // `RenderFlex.performLayout`) becomes a test of each field against
    // the type the field already has, and `is` against a function type or
    // `Record` is nothing `Any` can be asked (run712). Answered here, as
    // a literal's is above.
    if (env != null) {
      // ..through an extension type's erasure: it *is* its representation
      // at run time, and `_AscentDescent` is a `(double, double)?`
      // (`RenderFlex._computeSizes`, run713).
      final declared = _staticType(node.operand);
      final on = declared is ExtensionType
          ? declared.extensionTypeErasure
          : declared;
      // The asked type erases too: an extension type has no run-time
      // identity, so `x is _AscentDescent` really asks the representation
      // (`_AscentDescent operator +`, run717).
      final wanted = asked is ExtensionType
          ? asked.extensionTypeErasure
          : asked;
      if (on != null &&
          on is! DynamicType &&
          on is! NeverType &&
          !_mentionsTypeParameter(on) &&
          !_mentionsTypeParameter(wanted)) {
        try {
          if (env.isSubtypeOf(on, wanted)) {
            return IrBlockValue(
              [IrExprStmt(expression(node.operand))],
              IrLiteral('true', const IrType('bool')),
            )..rustType = const IrType('bool');
          }
          // ..and where only the `?` stands between them, the test is the
          // null check alone: the CFE's record pattern asks this after its
          // own `== null` arm. Only when the value in hand is an `Option`:
          // a narrowed one is not, and `is_none` is no method of an
          // `Rc<Border>` (`CupertinoTextField.build`, ws715).
          // ..only where the asked type has no runtime test of its own: a
          // record or a function type is nothing `Any` can be asked, and
          // this is the whole of what Dart means there. A class is left to
          // the ordinary test below, whose narrowing this cannot see
          // (`is_none` on an `Rc<Border>`, `CupertinoTextField.build`,
          // ws716).
          if ((wanted is RecordType || wanted is FunctionType) &&
              on.nullability == Nullability.nullable &&
              wanted.nullability != Nullability.nullable &&
              env.isSubtypeOf(
                on.withDeclaredNullability(Nullability.nonNullable),
                wanted,
              )) {
            return IrUnary('!', IrIsNull(expression(node.operand)))
              ..rustType = const IrType('bool');
          }
        } catch (_) {
          // Not a relation this environment can decide: asked below.
        }
      }
    }
    // `x is T?` admits null as well -- it is `x == null || x is T`, which
    // is exactly what the CFE writes for `if (parent is _NestedHookElement?)`
    // (nested's `SingleChildWidgetElementMixin.mount`). Asked as a plain
    // `is T`, the test null-checked the operand and unwrapped the very
    // absence it was admitting (4 at ws793). Only on a local: the operand
    // is read twice, and `||` reads the second only when the first said no.
    if (asked.nullability == Nullability.nullable &&
        asked is InterfaceType &&
        node.operand is VariableGet) {
      final on = _staticType(node.operand);
      if (on != null && on.nullability == Nullability.nullable) {
        return IrBinary(
          '||',
          IrIsNull(expression(node.operand))..rustType = const IrType('bool'),
          IrIs(
            expression(node.operand),
            _type(asked.withDeclaredNullability(Nullability.nonNullable)),
          )..rustType = const IrType('bool'),
        )..rustType = const IrType('bool');
      }
    }
    return IrIs(expression(node.operand), _type(asked));
  }

  /// The downcast of `lowered` to the concrete class `to` names. A counted
  /// class comes back as its handle (`clone` on a downcast, ws279); a value
  /// class as a copy -- except a *generic* one, whose derived `Clone` wants
  /// `T: Clone` the impl never promised (ws281): that one is read as a
  /// reference, and every field read through a downcast clones the field.
  IrExpr _narrowingCast(IrExpr lowered, InterfaceType to) {
    final target = to.classNode;
    final cast = IrDowncast(
      lowered,
      rustScalar(target.name),
      arguments: _erasedArguments(target, to.typeArguments),
    );
    if (!_closureCallsMethod(target) && target.typeParameters.isNotEmpty) {
      return cast;
    }
    return IrCall(cast, 'clone', const []);
  }

  /// A generic method called on a trait handle: see `IrSuperDispatch`.
  /// Only when the closed world holds exactly one body for it, in a class
  /// that is a trait here; a second body (`OptionalMethodChannel.
  /// invokeMethod`) or a body on a struct is left for a later round.
  IrExpr? _genericOnTrait(InstanceInvocation node, List<IrExpr> args) {
    final target = node.interfaceTarget;
    if (target is! Procedure ||
        target.kind != ProcedureKind.Method ||
        target.function.typeParameters.isEmpty) {
      return null;
    }
    final declaring = target.enclosingClass;
    if (declaring == null || !_abstractLike(declaring)) return null;
    final receiver = node.receiver;
    final Class? from;
    if (receiver is ThisExpression) {
      from = declaring;
    } else {
      final t = _staticType(receiver);
      from = t is InterfaceType ? t.classNode : null;
      if (from == null || !_abstractLike(from)) return null;
    }
    final bodies = _genericBodies(target);
    if (Platform.environment['DART2RUST_TRACE_CALL'] == target.name.text) {
      stderr.writeln(
        'TRACE_CALL generic-on-trait ${declaring.name}.${target.name.text} '
        'from=${from.name} bodies=${bodies.map((c) => c.name).toList()} '
        'subtypes=${_subtypes != null}',
      );
    }
    if (bodies.length != 1) return null;
    final body = bodies.single;
    if (!_abstractLike(body)) return null;
    final hierarchy = typeEnvironment?.hierarchy;
    final below =
        from == body || (hierarchy?.isSubInterfaceOf(from, body) ?? false);
    return IrSuperDispatch(
      receiver is ThisExpression ? IrThis() : expression(receiver),
      body.name,
      target.name.text,
      args,
      [for (final t in node.arguments.types) _type(t)],
      body.typeParameters.where((p) => !_erasedParameter(p)).length,
      castTo: below ? null : body.name,
    );
  }

  /// The indices of a generic instance method's type parameters that some
  /// body in its *family* -- the topmost declaration and every override
  /// under it -- uses as a type literal (`_inheritedElements[T]` in
  /// `Element.getElementForInheritedWidgetOfExactType<T>`). Those travel
  /// as `Type` values in hidden trailing parameters `__ty_<i>`: a `dyn`
  /// receiver reaches the method through its erased twin, whose `T` is
  /// `Rc<dyn Object>` and whose `dart_type_of::<T>()` was therefore the
  /// wrong type (`MediaQuery._of` through provider's override, run623).
  /// Dart's own runtime passes type arguments this way; here only the
  /// observed ones are.
  final _typeValueIndices = <Procedure, List<int>>{};

  List<int> _typeValues(Procedure p) {
    if (p.isStatic ||
        p.kind != ProcedureKind.Method ||
        p.function.typeParameters.isEmpty ||
        p.enclosingClass == null ||
        !_translatedClass(p.enclosingClass!)) {
      return const [];
    }
    final root = _familyRoot(p);
    return _typeValueIndices.putIfAbsent(root, () {
      final arity = root.function.typeParameters.length;
      final found = <int>{};
      final classes = <Class>{root.enclosingClass!, ..._genericBodies(root)};
      for (final c in classes) {
        for (final q in c.procedures) {
          if (q.name.text != root.name.text ||
              q.isStatic ||
              q.kind != ProcedureKind.Method) {
            continue;
          }
          final params = q.function.typeParameters;
          if (params.length != arity) continue;
          final finder = _TypeLiteralFinder(params.toSet());
          q.function.body?.accept(finder);
          for (final t in finder.found) {
            found.add(params.indexOf(t));
          }
        }
      }
      return found.toList()..sort();
    });
  }

  /// The topmost declaration of an instance method's name above `p`'s
  /// class (through every supertype, declared or inherited), or `p`.
  Procedure _familyRoot(Procedure p) {
    final name = p.name.text;
    Procedure? best = p;
    final seen = <Class>{};
    final queue = <Class>[p.enclosingClass!];
    while (queue.isNotEmpty) {
      final c = queue.removeAt(0);
      if (!seen.add(c)) continue;
      for (final q in c.procedures) {
        if (q.name.text == name &&
            !q.isStatic &&
            q.kind == ProcedureKind.Method &&
            q.function.typeParameters.length ==
                p.function.typeParameters.length) {
          best = q;
        }
      }
      queue.addAll([
        if (c.superclass != null) c.superclass!,
        if (c.mixedInClass != null) c.mixedInClass!,
        for (final t in c.implementedTypes) t.classNode,
      ]);
    }
    return best!;
  }

  /// The hidden `Type` parameters `p` takes (see `_typeValues`).
  List<IrParam> _typeValueParams(Procedure p) => [
    for (final i in _typeValues(p)) IrParam('__ty_$i', const IrType('Type')),
  ];

  /// The classes below (and including) the target's that carry a body for
  /// its name, whole program.
  List<Class> _genericBodies(Procedure target) {
    final subtypes = _subtypes;
    if (subtypes == null) return const [];
    final declaring = target.enclosingClass!;
    final out = <Class>[];
    for (final c in [declaring, ...subtypes.getSubtypesOf(declaring)]) {
      if (c.isAnonymousMixin || out.contains(c)) continue;
      // The translated classes -- by the prefixes this run was given, not
      // `package:` by name: a fixture's `file:` classes had no bodies
      // here and every generic trait call went to the erased twin.
      if (!_translatedClass(c)) continue;
      for (final p in c.procedures) {
        if (p.name.text == target.name.text &&
            !p.isStatic &&
            !p.isAbstract &&
            p.kind == ProcedureKind.Method &&
            p.function.body != null) {
          out.add(c);
          break;
        }
      }
    }
    return out;
  }

  /// What a `throw` hands to `Err`: a *value* of a translated class
  /// (`throw error` with a `FlutterError` in hand, `FlutterError(..)`
  /// whose constructor is a factory and so a static call) goes behind an
  /// `Rc<dyn Object>` here; a constructed one the backend boxes itself
  /// (`_boxedThrow`), a handle or a trait object unsizes on its own.
  IrExpr _thrownValue(Expression thrown) {
    final lowered = expression(thrown);
    final type = _staticType(thrown);
    if (type is InterfaceType &&
        thrown is! ConstructorInvocation &&
        thrown is! ConstantExpression &&
        thrown is! StringLiteral &&
        type.nullability != Nullability.nullable &&
        _translatedClass(type.classNode) &&
        !_abstractLike(type.classNode) &&
        !_closureCallsMethod(type.classNode) &&
        !type.classNode.isEnum &&
        !_scalarClass(type.classNode) &&
        lowered is! IrNew &&
        lowered is! IrUpcast) {
      return IrCall(lowered, '!rc_object', const []);
    }
    return lowered;
  }

  /// A collection's element argument (`set.remove(ticker)`, `contains`),
  /// shared into the element type when that is a trait and the argument a
  /// concrete class of it: the prelude takes `&T`, and a `&Rc<_WidgetTicker>`
  /// does not coerce to `&Rc<dyn Ticker>` through the reference (45 at
  /// ws311). A counted class's handle is upcast (`IrUpcast.handle`), a
  /// value put behind a fresh one. Only a named value: `this` may be a
  /// struct behind `&self` (ws312).
  /// An element handed to a list's `remove`/`indexOf`/`contains`: into
  /// the list's element type by the one rule, as an `add` is -- a
  /// `Disposer` into a `List<Disposer?>.remove` wants its `Some` (get's
  /// `ListNotifier.removeListener`, ws493), a subclass its handle.
  IrExpr _intoElement(IrExpr lowered, Expression value, DartType? collection) {
    if (collection is! InterfaceType || collection.typeArguments.isEmpty) {
      return lowered;
    }
    final element = collection.typeArguments.first;
    return _intoArgument(value, element, lowered);
  }

  /// A value into a collection's own slot -- an element, a key: the slot
  /// is a *type argument*, so a `T?` there is spelled projected (`<T as
  /// DartNullable>::Or`) and not the body's `Option<T>`. A read of the
  /// same slot arrives projected too, so nothing converts in between
  /// (`widget.optionsMap[widget.selectedOption]` put the key through an
  /// `option` the map's `get` would not take, run693).
  /// `m[k] = v`: the key and the value into the *map's own* slots, which
  /// are type arguments. The callee here is `Map.[]=`, a prelude member
  /// whose declared `K`, `V` coerce nothing, so a `Map<T?, ..>`'s key
  /// arrived as the body's `Option<T>` where the map holds the projected
  /// `<T as DartNullable>::Or` (run693).
  List<IrExpr> _mapEntry(InstanceInvocation node, List<IrExpr> args) {
    final mapType = _staticType(node.receiver);
    if (args.length != 2 ||
        node.arguments.positional.length != 2 ||
        mapType is! InterfaceType ||
        mapType.typeArguments.length != 2) {
      return args;
    }
    return [
      for (var i = 0; i < 2; i++)
        _intoArgument(
          node.arguments.positional[i],
          mapType.typeArguments[i],
          args[i],
        ),
    ];
  }

  IrExpr _intoArgument(Expression value, DartType slot, IrExpr lowered) {
    IrType? spelled;
    try {
      spelled = _typeNested(slot);
    } on Unsupported {
      spelled = null;
    }
    return _widened(value, slot, lowered, slotIr: spelled);
  }

  /// Whether a type names an erased parameter anywhere in it.
  bool _mentionsErased(DartType t) => switch (t) {
    TypeParameterType() => _erasedParameter(t.parameter),
    FutureOrType() => _mentionsErased(t.typeArgument),
    RecordType() =>
      t.positional.any(_mentionsErased) ||
          t.named.any((n) => _mentionsErased(n.type)),
    InterfaceType() => t.typeArguments.any(_mentionsErased),
    FunctionType() =>
      _mentionsErased(t.returnType) ||
          t.positionalParameters.any(_mentionsErased) ||
          t.namedParameters.any((n) => _mentionsErased(n.type)),
    _ => false,
  };

  // There was a `_refusePrivate` here. It is gone, and its going is the point
  // of this round: skipping private members is right when translating one file
  // at a time -- nothing outside the library can name them -- and wrong for a
  // whole program, because that is where the program keeps its implementation.
  // Every StatefulWidget in Flutter does its work in a private State class, and
  // so do most of the gallery's 689 classes. A compiler that skips them
  // translates the surface and none of the substance, and reports a low refusal
  // count for having looked at less.
}
