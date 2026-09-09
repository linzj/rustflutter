part of '../frontend_kernel.dart';

// Parameters, defaults, borrowing and type literals.
augment class KernelFrontend {
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

  /// The parameters a class's own bodies read as a type literal, before
  /// asking whether they are erased.
  ///
  /// Separate from [_typeLiteralParams] because `_erasedParameter` reads
  /// *this* one: a class that reads its own `T` as a type has a say in
  /// whether `T` survives, and the filtered set below would be circular.
  final _typeLiteralUsesCache = <Class, Set<TypeParameter>>{};

  Set<TypeParameter> _typeLiteralUses(Class c) =>
      _typeLiteralUsesCache.putIfAbsent(c, () {
        if (c.typeParameters.isEmpty) return const {};
        final finder = _TypeLiteralFinder(c.typeParameters.toSet());
        for (final m in c.members) {
          m.accept(finder);
        }
        return finder.found;
      });

  Set<TypeParameter> _typeLiteralParams(Class c) =>
      _typeLiteralParamsCache.putIfAbsent(c, () {
        return {
          for (final p in _typeLiteralUses(c))
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
}
