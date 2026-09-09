part of '../frontend_kernel.dart';

// Coercion by type: widening a value into the slot it lands in.
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

  /// The type parameters of the local functions being lowered right now:
  /// each is its bound wherever it is named (see `_type`).
  final Set<TypeParameter> _erasedLocalParams = {};

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
    // ..and a slot that *spells* as one of those: a local function's own
    // `T?` at the implicit bound `Object?` is that same non-nullable
    // `dynamic`, and `Some(..)` around it is an `Option` nothing declared
    // (`effectiveValue<Color>`, ws879).
    if (param is TypeParameterType &&
        _erasedLocalParams.contains(param.parameter)) {
      IrType? spelled;
      try {
        spelled = _type(param);
      } on Unsupported {
        spelled = null;
      }
      if (spelled != null && !isNullable(spelled)) return lowered;
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
}
