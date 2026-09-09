part of '../frontend_kernel.dart';

// Slots, kept type parameters, write landings and literals.
augment class KernelFrontend {
  /// A trait handle into a slot whose *erased* parameter is bounded by a
  /// wider trait: `child`, a `RenderBox`, into `ContainerRenderObjectMixin
  /// .insert(ChildType child, {ChildType? after})`, which is `Rc<dyn
  /// RenderObject>` here. Rust upcasts a bare handle at the call and not
  /// one inside an `Option` (24 `_insertIntoChildList` at ws340), so the
  /// handle is upcast by name, through `map` when it is optional.
  /// The member an instance call lands on (`_landing`), and the receiver's
  /// type, while its arguments are lowered: a parameter's slot is that
  /// member's declared type -- a mixin clone's `RenderBox`, or the trait's
  /// erased bound -- with the receiver's arguments put in for the class's
  /// kept parameters, exactly as a read is typed (`_memberRustType`).
  Procedure? _dispatchMember;
  DartType? _dispatchReceiverType;

  /// A generic callee's own type arguments at the call, while its
  /// arguments are lowered: `T?` under `lerp<Color?>` is `Option<Option<
  /// Rc<dyn Color>>>` here, which Dart's instantiated type collapses.
  FunctionNode? _genericCallee;
  Map<TypeParameter, DartType> _genericArgs = const {};

  T _withGenericArgs<T>(
    FunctionNode fn,
    Arguments arguments,
    T Function() lower,
  ) {
    if (fn.typeParameters.isEmpty ||
        arguments.types.length != fn.typeParameters.length) {
      return lower();
    }
    final wasCallee = _genericCallee;
    final wasArgs = _genericArgs;
    _genericCallee = fn;
    _genericArgs = {
      for (var i = 0; i < fn.typeParameters.length; i++)
        fn.typeParameters[i]: arguments.types[i],
    };
    try {
      return lower();
    } finally {
      _genericCallee = wasCallee;
      _genericArgs = wasArgs;
    }
  }

  IrType? _genericSlotIr(FunctionNode? callee, DartType? declared) {
    if (declared == null ||
        callee == null ||
        !identical(callee, _genericCallee) ||
        _genericArgs.isEmpty) {
      return null;
    }
    if (!_mentionsParametersOf(declared, callee.typeParameters)) return null;
    // The receiver class's own parameters put in first: `Future<String>.
    // then<R>(FutureOr<R> Function(T))` takes a `String`, and a bare `T`
    // left in collided with the caller's `T` (`loadStructuredData<T>`'s
    // parser adapter took a `T`, run600).
    var substituted = declared;
    final receiverType = _dispatchReceiverType;
    if (receiverType is InterfaceType &&
        identical(callee, _dispatchInterface) &&
        receiverType.typeArguments.isNotEmpty) {
      substituted = Substitution.fromInterfaceType(receiverType)
          .substituteType(declared);
    }
    try {
      return _typeKept(substituted, _genericArgs, byTurbofish: true);
    } on Unsupported {
      return null;
    }
  }

  /// The slot a constructor's parameter is at the class's instantiation
  /// (`_constructing`): `SettingsListItem<ThemeMode?>(selectedOption: x)`
  /// takes an `Option<ThemeMode>` where the declaration says `T`, and a
  /// bare `T` widened nothing (`_SettingsPageState.build`, run660).
  IrType? _constructedSlotIr(FunctionNode? callee, DartType? declared) {
    if (declared == null ||
        callee == null ||
        !identical(callee, _constructedCallee) ||
        _constructedArgs.isEmpty) {
      return null;
    }
    final kept = {
      for (final e in _constructedArgs.entries)
        if (!_erasedParameter(e.key)) e.key: e.value,
    };
    if (kept.isEmpty || !_mentionsParametersOf(declared, kept.keys.toList())) {
      return null;
    }
    try {
      return _typeKept(declared, kept);
    } on Unsupported {
      return null;
    }
  }

  /// The interface member whose arguments the dispatch above is for: a
  /// call nested inside one of those arguments has a callee of its own
  /// (`Matrix4.rotationY(angle)` as an argument took the outer call's
  /// first parameter, 31 at ws369).
  FunctionNode? _dispatchInterface;

  DartType? _landingSlot({
    required FunctionNode? callee,
    int? index,
    String? name,
  }) {
    final landing = _dispatchMember;
    if (landing == null || !identical(callee, _dispatchInterface)) return null;
    final fn = landing.function;
    DartType? declared;
    if (index != null && index < fn.positionalParameters.length) {
      declared = fn.positionalParameters[index].type;
    } else if (name != null) {
      for (final p in fn.namedParameters) {
        if (p.parameterName == name) declared = p.type;
      }
    }
    if (declared == null) return null;
    // The method's own parameters are instantiated at the call: Dart's
    // type is the better answer there.
    if (fn.typeParameters.isNotEmpty &&
        _mentionsParametersOf(declared, fn.typeParameters)) {
      return null;
    }
    return _substituteKept(
      declared,
      landing.enclosingClass,
      _dispatchReceiverType,
    );
  }

  /// `_landingSlot` in the IR, `Option` layers kept apart (`_typeKept`).
  IrType? _landingSlotIr({
    required FunctionNode? callee,
    int? index,
    String? name,
  }) {
    final declared = _landingSlot(callee: callee, index: index, name: name);
    final landing = _dispatchMember;
    if (declared == null || landing == null) return null;
    // The declared type again, unsubstituted, for the IR-level put-in.
    final fn = landing.function;
    DartType? raw;
    if (index != null && index < fn.positionalParameters.length) {
      raw = fn.positionalParameters[index].type;
    } else if (name != null) {
      for (final p in fn.namedParameters) {
        if (p.parameterName == name) raw = p.type;
      }
    }
    if (raw == null) return null;
    try {
      return _typeKept(
        raw,
        _keptFor(landing.enclosingClass, _dispatchReceiverType),
      );
    } on Unsupported {
      return null;
    }
  }

  /// The receiver's type arguments for `owner`'s *kept* parameters (the
  /// erased ones are left to `_type`, which spells them as their bound).
  Map<TypeParameter, DartType> _keptFor(Class? owner, DartType? receiverType) {
    final env = typeEnvironment;
    if (owner == null ||
        owner.typeParameters.isEmpty ||
        env == null ||
        receiverType is! InterfaceType) {
      return const {};
    }
    final asOwner = env.hierarchy.getTypeAsInstanceOf(receiverType, owner);
    if (asOwner is! InterfaceType) return const {};
    final kept = <TypeParameter, DartType>{};
    for (
      var i = 0;
      i < owner.typeParameters.length && i < asOwner.typeArguments.length;
      i++
    ) {
      final p = owner.typeParameters[i];
      if (!_erasedParameter(p)) kept[p] = asOwner.typeArguments[i];
    }
    return kept;
  }

  /// `declared` as a Rust type with `kept` put in for its parameters --
  /// in the IR, not in Kernel, because Dart collapses `T?` with `T` bound
  /// to `Color?` into `Color?` and Rust's `Option<T>` does not: that is
  /// `Option<Option<Rc<dyn Color>>>` here (the `WidgetStateProperty<
  /// Color?>.lerp` family, 66 mismatches at ws384).
  IrType _typeKept(
    DartType t,
    Map<TypeParameter, DartType> kept, {
    bool byTurbofish = false,
  }) {
    if (t is TypeParameterType && kept.containsKey(t.parameter)) {
      // What is put in is a type argument: a `U?` there is projected.
      final arg = _typeNested(kept[t.parameter]!);
      if (t.nullability != Nullability.nullable) {
        // A generic *method*'s own parameter is instantiated by what its
        // turbofish spells, and that is the plain `Option<T>` -- so the slot
        // is that and not the projection this declaration uses for its own
        // edges (`entry.complete<T?>(result)` in `Navigator.removeRoute`, 14
        // at ws761). A *class*'s instantiation is not spelled that way: the
        // struct is named `SettingsListItem<<T as DartNullable>::Or>` and
        // its fields keep the projection (ws763).
        return byTurbofish && arg.projected
            ? IrType(arg.name, nullable: true, arguments: arg.arguments)
            : arg;
      }
      // `T?` with `T` bound to `X?` is `X?`, as Dart collapses it and as
      // rustc normalises the projected signature to (`<Option<X> as
      // DartNullable>::Or` is `Option<X>`): the plain `Option`, projected
      // no more. With `T` bound to a bare `U` the slot stays `Or`.
      // ..unless what is put in is itself a projected `U?` of the code
      // here: `<<U as DartNullable>::Or as DartNullable>::Or` normalises
      // to `<U as DartNullable>::Or`, still projected (`Tile<T?>`'s slots
      // from a `Picker<T>`, fixture closureedge).
      if (isNullable(arg)) {
        return arg.projected
            ? arg
            : IrType(arg.name, nullable: true, arguments: arg.arguments);
      }
      // Projected only over a bare type parameter of the code here: over a
      // concrete class the slot normalises to the plain `Option` (and
      // `<GestureBinding as DartNullable>` names a trait as a type).
      final put = kept[t.parameter]!;
      // A function type keeps its signature (see the map read's typing).
      if (arg.isFunction) {
        return IrType.function(arg.parameters!, arg.returns!, nullable: true);
      }
      return IrType(
        arg.name,
        nullable: true,
        arguments: arg.arguments,
        projected: _projectedSlot(
          put.withDeclaredNullability(Nullability.nullable),
        ),
      );
    }
    if (t is InterfaceType && kept.isNotEmpty) {
      final base = _type(t);
      final cls = t.classNode;
      return IrType(
        base.name,
        nullable: base.nullable,
        arguments: [
          for (var i = 0; i < t.typeArguments.length; i++)
            if (i >= cls.typeParameters.length ||
                !_erasedParameter(cls.typeParameters[i]))
              _typeKept(t.typeArguments[i], kept),
        ],
      );
    }
    if (t is FunctionType && kept.isNotEmpty) {
      final named = [...t.namedParameters]
        ..sort((a, b) => a.name.compareTo(b.name));
      return IrType.function(
        [
          for (final p in t.positionalParameters) _typeKept(p, kept),
          for (final p in named) _typeKept(p.type, kept),
        ],
        _typeKept(t.returnType, kept),
        nullable: t.nullability == Nullability.nullable,
      );
    }
    // `FutureOr<T>` with the call's `T` put in: a parser slot `FutureOr<T>
    // Function(ByteData)` at `loadStructuredBinaryData<AssetManifest>(..)`
    // takes the `_AssetManifestBin` a factory returns *as* a `dyn
    // AssetManifest`, which a `T` left in said nothing about (run569).
    if (t is FutureOrType && kept.isNotEmpty) {
      final base = _type(t);
      return IrType(
        base.name,
        nullable: base.nullable,
        arguments: [_typeKept(t.typeArgument, kept)],
      );
    }
    return _type(t);
  }

  /// `declared` with the receiver's type arguments put in for `owner`'s
  /// *kept* parameters; the erased ones stay, for `_type` to spell as
  /// their bound.
  DartType _substituteKept(
    DartType declared,
    Class? owner,
    DartType? receiverType,
  ) {
    final env = typeEnvironment;
    if (owner == null ||
        owner.typeParameters.isEmpty ||
        env == null ||
        receiverType is! InterfaceType) {
      return declared;
    }
    final asOwner = env.hierarchy.getTypeAsInstanceOf(receiverType, owner);
    if (asOwner is! InterfaceType) return declared;
    final kept = <TypeParameter, DartType>{};
    for (
      var i = 0;
      i < owner.typeParameters.length && i < asOwner.typeArguments.length;
      i++
    ) {
      final p = owner.typeParameters[i];
      if (!_erasedParameter(p)) kept[p] = asOwner.typeArguments[i];
    }
    return Substitution.fromMap(kept).substituteType(declared);
  }

  /// Whether a member landing on a field is that field for this class: a
  /// struct holds its clones' fields; a trait body (a mixin, an abstract
  /// or an open class) reaches its own through accessors, and a direct
  /// field write there made the mutation analysis ask for `&mut self`
  /// (12 "incompatible type for trait", ws373).
  bool _heldField(Member interface, Expression receiver) {
    final on = receiver is ThisExpression ? _lowering : _staticClass(receiver);
    // ..and on another object only when that object is a struct: a handle
    // to an open class has accessors, not fields (12 "attempted to take
    // value of method", ws379).
    if (on == null || _abstractLike(on)) return false;
    return _landing(interface, receiver) is Field;
  }

  /// The declaration a copy in an anonymous application is lowered under
  /// (see `_lowerProcedure`'s `signature`), or null for a member that is
  /// its own declaration.
  Procedure? _cloneSignature(Procedure p) {
    final original = _originalOf(p);
    return identical(original, p) || original is! Procedure ? null : original;
  }

  Member _originalOf(Member m, {bool forWrite = false}) {
    final owner = m.enclosingClass;
    if (owner == null || !owner.isAnonymousMixin) return m;
    final setter = m is Procedure && m.isSetter;
    final getter = m is Procedure && m.isGetter;
    for (final st in [
      if (owner.mixedInType != null) owner.mixedInType!,
      ...owner.implementedTypes,
    ]) {
      for (final o in st.classNode.members) {
        if (o.name.text != m.name.text) continue;
        if (m is Field) {
          // A hollow declaration keeps a field as an abstract getter and
          // setter pair (`ChildType? get _lastChild` / `set _lastChild`
          // in `ContainerRenderObjectMixin`, ws478).
          if (o is Field) return o;
          if (o is Procedure && (forWrite ? o.isSetter : o.isGetter)) {
            return o;
          }
          continue;
        }
        if (o is Procedure && o.isSetter == setter && o.isGetter == getter) {
          return o;
        }
      }
    }
    return m;
  }

  /// A super call's slots as this class sees them: the declaration's
  /// parameter types (the mixin's, behind a copy) with this class's
  /// arguments put in for the mixin's kept parameters.
  ///
  /// A copy whose declaration no longer lists the member (TFA dropped it
  /// there) is typed the way the trait was: the application's arguments
  /// taken back out (`_unapplied`) and this class's put in. Untyped, the
  /// argument went to the mixin's super body as the copy's `Panel` where
  /// the trait's erased `S` says `Rc<dyn Widget>` (the unapply fixture).
  (List<DartType>?, Map<String, DartType>?) _superSlots(Member target) {
    final original = _originalOf(target);
    if (original is! Procedure) return (null, null);
    final Class? owner;
    final DartType Function(DartType) declared;
    if (identical(original, target)) {
      final application = target.enclosingClass;
      if (application == null || !application.isAnonymousMixin) {
        return (null, null);
      }
      // A deduplicated application (`dart:mixin_deduplication`) has no
      // `mixedInType`; the mixin is among its `implementedTypes`.
      final mixin =
          application.mixedInType?.classNode ??
          application.implementedTypes
              .map((st) => st.classNode)
              .where((c) => c.isMixinDeclaration)
              .firstOrNull;
      if (mixin == null) return (null, null);
      owner = mixin;
      declared = (t) => _unapplied(t, application, mixin);
    } else {
      owner = original.enclosingClass;
      declared = (t) => t;
    }
    final fn = original.function;
    if (Platform.environment['DART2RUST_TRACE_SUPER'] != null) {
      stderr.writeln(
        'TRACE_SUPER ${target.enclosingClass?.name}.${target.name.text} same=${identical(original, target)} owner=${owner?.name} '
        'slots=${[for (final p in fn.positionalParameters) '${p.type} -> ${declared(p.type)} -> ${_asApplied(declared(p.type), owner)}']}',
      );
    }
    return (
      [
        for (final p in fn.positionalParameters)
          _asApplied(declared(p.type), owner),
      ],
      {
        for (final p in fn.namedParameters)
          p.parameterName: _asApplied(declared(p.type), owner),
      },
    );
  }

  /// What a super read of `target` hands back, as this class sees it:
  /// the declaration's type (the mixin's, behind a copy; a copy whose
  /// declaration is gone unapplied, as `_superSlots` does) with this
  /// class's arguments put in for the kept parameters. Null where this
  /// compiler has no spelling for it.
  IrType? _superReturn(Member target) {
    final original = _originalOf(target);
    final Class? owner;
    final DartType Function(DartType) declared;
    if (identical(original, target)) {
      final application = target.enclosingClass;
      if (application != null && application.isAnonymousMixin) {
        final mixin =
            application.mixedInType?.classNode ??
            application.implementedTypes
                .map((st) => st.classNode)
                .where((c) => c.isMixinDeclaration)
                .firstOrNull;
        if (mixin == null) return null;
        owner = mixin;
        declared = (t) => _unapplied(t, application, mixin);
      } else {
        owner = target.enclosingClass;
        declared = (t) => t;
      }
    } else {
      owner = original.enclosingClass;
      declared = (t) => t;
    }
    final DartType? type = original is Procedure
        ? original.function.returnType
        : original is Field
        ? original.type
        : null;
    if (type == null) return null;
    return _recordedType(_asApplied(declared(type), owner));
  }

  /// A type of a copy in `application`, with the arguments the application
  /// put in for `mixin`'s parameters taken back out: `Slot` where the
  /// application implements `SlottedContainer<Slot, RenderBox>` reads as
  /// `SlotType` again. An erased parameter's argument is taken out too:
  /// the parameter reads as its bound, which is what the trait says
  /// everywhere -- left in, `RestorationMixin<S>.didUpdateWidget(S)` was
  /// declared on the trait with one application's `DatePickerDialog`, and
  /// every other implementor's forwarder mismatched (ws535). Structural,
  /// so an argument that also occurs on its own in the type is taken for
  /// the parameter too -- the copy is the CFE's substitution, and this is
  /// its inverse.
  DartType _unapplied(DartType t, Class application, Class mixin) {
    Supertype? applied;
    if (application.mixedInType?.classNode == mixin) {
      applied = application.mixedInType;
    } else {
      for (final st in application.implementedTypes) {
        if (st.classNode == mixin) applied = st;
      }
    }
    if (applied == null) return t;
    final back = <DartType, TypeParameter>{};
    for (var i = 0; i < mixin.typeParameters.length; i++) {
      if (i >= applied.typeArguments.length) break;
      final p = mixin.typeParameters[i];
      final a = applied.typeArguments[i].withDeclaredNullability(
        Nullability.nonNullable,
      );
      if (a is TypeParameterType && a.parameter == p) continue;
      back[a] = p;
    }
    if (back.isEmpty) return t;
    DartType walk(DartType x) {
      final bare = x.withDeclaredNullability(Nullability.nonNullable);
      final p = back[bare];
      if (p != null) return TypeParameterType(p, x.nullability);
      if (x is InterfaceType) {
        return InterfaceType(x.classNode, x.nullability, [
          for (final a in x.typeArguments) walk(a),
        ]);
      }
      if (x is FunctionType) {
        return FunctionType(
          [for (final a in x.positionalParameters) walk(a)],
          walk(x.returnType),
          x.nullability,
          namedParameters: [
            for (final n in x.namedParameters)
              NamedType(n.name, walk(n.type), isRequired: n.isRequired),
          ],
          typeParameters: x.typeParameters,
          requiredParameterCount: x.requiredParameterCount,
        );
      }
      return x;
    }

    return walk(t);
  }

  /// A declared type of `owner`'s (a mixin's) with `owner`'s parameters
  /// substituted by the class being lowered's arguments for them.
  /// A mixin's body comes from an *application* (`_appliedBody`), where the
  /// CFE has already put the application's arguments in for the mixin's
  /// parameters: `ContainerRenderObjectMixin.visitChildren` copied into
  /// `RenderFlex`'s application casts to `FlexParentData` where the mixin
  /// wrote `ParentDataType`. Lowered as the *trait's* default that body
  /// serves every application, so the argument goes back to the parameter
  /// -- and only for an **erased** one, whose spelling is its bound, the
  /// trait everything reads through anyway (`RenderSliverList` cast a
  /// `SliverMultiBoxAdaptorParentData` to `FlexParentData`, run734).
  Map<DartType, DartType> _appliedBack = const {};

  /// A type as this *body* holds it: an applied mixin body's concrete
  /// argument is the mixin's erased parameter here (`_appliedBack`).
  DartType? _backHere(DartType? t) {
    if (t is! InterfaceType || _appliedBack.isEmpty) return t;
    final back =
        _appliedBack[t.withDeclaredNullability(Nullability.nonNullable)];
    if (back == null) return t;
    return t.nullability == Nullability.nullable
        ? back.withDeclaredNullability(Nullability.nullable)
        : back;
  }

  Map<DartType, DartType> _appliedBackMap(Class mixin, Class? application) {
    if (application == null) return const {};
    Supertype? applied;
    for (final t in application.implementedTypes) {
      if (t.classNode == mixin) applied = t;
    }
    if (applied == null) return const {};
    final out = <DartType, DartType>{};
    for (var i = 0; i < applied.typeArguments.length; i++) {
      if (i >= mixin.typeParameters.length) break;
      final p = mixin.typeParameters[i];
      if (!_erasedParameter(p)) continue;
      final argument = applied.typeArguments[i];
      if (argument is! InterfaceType) continue;
      out[argument.withDeclaredNullability(Nullability.nonNullable)] =
          TypeParameterType(p, Nullability.nonNullable);
    }
    return out;
  }

  DartType _asApplied(DartType declared, Class? owner) {
    final env = typeEnvironment;
    final thisType = env == null
        ? null
        : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable);
    if (owner == null || thisType is! InterfaceType) return declared;
    // The *kept* parameters only (`_keptFor`): an erased one is its bound
    // everywhere, the trait included, and `ChildType` put in as `RenderBox`
    // brought the `RenderBox`-typed copies back (+47 at ws489).
    final kept = _keptFor(owner, thisType);
    if (kept.isEmpty) return declared;
    return Substitution.fromMap(kept).substituteType(declared);
  }

  /// A field's type as this class holds it: a copy's by the mixin's
  /// declaration with this class's arguments put in (`_asApplied`), its
  /// own otherwise. The same answer for the declaration and for the
  /// initialiser the CFE moved into the application's constructor.
  DartType _fieldTypeHere(Field field) {
    final declared = _declaredFieldType(field);
    if (declared == null) return field.type;
    return _asApplied(declared, _originalOf(field).enclosingClass);
  }

  /// The type a copy's field is declared with (see `_originalOf`), or
  /// null for a field that is its own declaration.
  DartType? _declaredFieldType(Field field) {
    final original = _originalOf(field);
    if (identical(original, field)) return null;
    if (original is Field) return original.type;
    if (original is Procedure && original.isGetter) {
      return original.function.returnType;
    }
    return null;
  }

  /// Where a write lands for its slot's type: through the trait's setter
  /// (a qualified write) it is the declaring member's, the mixin's own
  /// for a copy in an application (`this.child = child` in `RenderView`'s
  /// constructor, `RenderBox?` there and `RenderObject?` in the trait,
  /// ws476); a plain write lands on the class's own.
  Member _writeLanding(Member interface, Expression receiver) => _originalOf(
    _setterQualifier(receiver, interface) != null
        ? interface
        : _landing(interface, receiver),
    forWrite: true,
  );

  /// The type a write into `interface` on `receiver` must produce: the
  /// landing member's -- a mixin clone's field, or the trait's setter.
  /// The mixin's own member behind a copy the CFE made in an anonymous
  /// application (`_MixinApplication8&RenderBox&RenderObjectWithChildMixin
  /// .child=` for `RenderObjectWithChildMixin.child=`): what the trait
  /// declares, with the mixin's parameter (`ChildType?`) where the copy
  /// has the application's argument (`RenderBox?`).
  DartType _writeSlot(Member interface, Expression receiver) {
    final landing = _writeLanding(interface, receiver);
    final declared = landing is Procedure && landing.isSetter
        ? landing.function.positionalParameters.single.type
        : landing.setterType;
    final env = typeEnvironment;
    final receiverType = receiver is ThisExpression
        ? (env == null
              ? null
              : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable))
        : _staticType(receiver);
    return _substituteKept(declared, landing.enclosingClass, receiverType);
  }

  /// `_writeSlot` in the IR, `Option` layers kept apart (`_typeKept`).
  IrType? _writeSlotIr(Member interface, Expression receiver) {
    final landing = _writeLanding(interface, receiver);
    final declared = landing is Procedure && landing.isSetter
        ? landing.function.positionalParameters.single.type
        : landing.setterType;
    final env = typeEnvironment;
    final receiverType = receiver is ThisExpression
        ? (env == null
              ? null
              : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable))
        : _staticType(receiver);
    try {
      return _typeKept(
        declared,
        _keptFor(landing.enclosingClass, receiverType),
      );
    } on Unsupported {
      return null;
    }
  }

  /// `Some(..)` around a non-null argument handed to a nullable parameter --
  /// Dart's silent widening, spelled. Only when the static type says the
  /// argument is not itself nullable, so a nullable variable passed on stays
  /// as it is.
  /// A map literal's entries, each key and value into the map's own types,
  /// sharing into an `Object?` value (`{'extension': name, 'value': value}`
  /// handed to `postEvent` as a `Map<String, Object?>`), as a list literal's
  /// are. The types are a parameter: a literal spread into a wider map --
  /// `<SingleActivator, Intent>{..}` into the `Map<ShortcutActivator,
  /// Intent>` of `DefaultTextEditingShortcuts` -- is lowered against the
  /// wider one's, so each key is shared into its `Rc<dyn ..>` (121
  /// "arguments incorrect" on one file in `widgets`).
  IrExpr _mapLiteral(MapLiteral node, DartType keyType, DartType valueType) {
    // Typed as the map it is, so a slot of another type adapts it: one
    // returned where `dynamic` goes is put behind a handle
    // (`_handlePlatformMessage`'s `{'response': ..}`, run460).
    return IrMapLiteral(
      [
        for (final entry in node.entries)
          (
            _widened(entry.key, keyType, expression(entry.key)),
            _widened(entry.value, valueType, expression(entry.value)),
          ),
      ],
      _type(keyType),
      _type(valueType),
    )..rustType = IrType('Map', arguments: [_type(keyType), _type(valueType)]);
  }

  /// A list literal's elements into `element`. The CFE keeps a literal of
  /// more than eight elements as a node (the `_literalN` constructors stop
  /// there): its elements widen and share into the element type exactly as
  /// the short ones' do.
  /// A record literal's fields into `fields` -- the literal's own types,
  /// or the slot's when it lands in one of other field types (see
  /// `_widenedInto`): `(false, null)` returned as a `(bool, Object?)`
  /// holds the `Null` object, `(true, x)` boxes its `int` (ws502, ws509).
  /// As translated slots, whatever call the record sits in.
  IrExpr _recordLiteral(
    RecordLiteral node,
    List<DartType> fields, [
    List<NamedType> named = const [],
  ]) {
    // The named fields in the *slot's* order, found by name: a literal
    // writes them in source order and the type spells them sorted.
    final namedFields = named.isEmpty ? node.recordType.named : named;
    NamedExpression written(String name) =>
        node.named.firstWhere((e) => e.name == name);
    final wasTranslated = _slotTranslated;
    final wasPrelude = _slotPrelude;
    _slotTranslated = true;
    _slotPrelude = false;
    try {
      return IrRecord([
          for (var i = 0; i < node.positional.length; i++)
            i < fields.length
                ? _widened(
                    node.positional[i],
                    fields[i],
                    expression(node.positional[i]),
                  )
                : expression(node.positional[i]),
          for (final n in namedFields)
            _widened(
              written(n.name).value,
              n.type,
              expression(written(n.name).value),
            ),
        ])
        ..rustType = IrType(
          'Record',
          arguments: _nested(
            () => [
              for (var i = 0; i < node.positional.length; i++)
                _recordedType(
                      i < fields.length
                          ? fields[i]
                          : node.recordType.positional[i],
                    ) ??
                    const IrType('dynamic'),
              for (final n in namedFields)
                _recordedType(n.type) ?? const IrType('dynamic'),
            ],
          ),
        );
    } finally {
      _slotTranslated = wasTranslated;
      _slotPrelude = wasPrelude;
    }
  }

  IrExpr _listLiteral(ListLiteral node, DartType element) {
    return IrListLiteral([
      for (final e in node.expressions)
        _widened(
          e,
          element,
          _withExpectedReturn(element, e, () => expression(e)),
        ),
    ], _type(element));
  }
}
