part of '../frontend_kernel.dart';

// Evaluated constants, erased parameters and type tests.
augment class KernelFrontend {
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

  /// The bodies `_genericOnTrait` sent a call to, as (class, member).
  ///
  /// The call becomes `superFn(body, member)` -- a free function in the
  /// body's module -- and *which* class holds the body is `_genericBodies`'
  /// whole-program answer. A per-library reference walk cannot reach it, so
  /// it is written down where it is decided.
  final Set<(Class, String)> genericBodies = {};

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
    genericBodies.add((body, target.name.text));
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
