part of '../frontend_kernel.dart';

// Reading a member and calling one on an instance.
augment class KernelFrontend {
  /// What a `num` method leaves in hand once the receiver has been narrowed
  /// to an `f64` (see the `DynamicInvocation` lowering below), by the
  /// prelude's own signatures (`DartDouble`). `floor`/`ceil`/`round` are not
  /// here: those are wrapped in a cast to `i64` where they are built.
  static const _narrowedNumResult = <String, IrType>{
    'isInfinite': IrType('bool'),
    'isNaN': IrType('bool'),
    'isFinite': IrType('bool'),
    'toDouble': IrType('double'),
    'abs': IrType('double'),
    'truncate': IrType('int'),
    'toInt': IrType('int'),
    'toStringAsFixed': IrType('String'),
  };

  IrExpr _instanceGetRaw(InstanceGet node) {
    final name = _fieldNameOf(node.interfaceTarget, node.name.text);
    if (Platform.environment['DART2RUST_TRACE_FIELDREAD'] ==
        (_member?.name.text ?? '')) {
      stderr.writeln(
        'TRACE_FIELDREAD wrote=${node.name.text} '
        'target=${node.interfaceTarget.name.text} name=$name '
        'receiver=${node.receiver.runtimeType}',
      );
    }
    final staticGot = _staticType(node.receiver);
    if (staticGot is InterfaceType && staticGot.classNode.name == 'Iterable') {
      iterableMembers.update(
        'get:${node.name.text}',
        (n) => n + 1,
        ifAbsent: () => 1,
      );
    }
    final listOwner = node.interfaceTarget.enclosingClass?.name;
    if (listOwner == 'List' || listOwner == 'Iterable') {
      final rust = listMethodNames[name];
      if (rust == null) throw Unsupported('`List.$name`', _sample(node));
      // A getter in Dart, a method in Rust: `xs.length` is `xs.len()`.
      return IrCall(_listReceiver(node.receiver), rust, const []);
    }
    if (_isMapClass(listOwner)) {
      if (orderedMapMembers.contains(name)) {
        throw Unsupported(
          '`Map.$name`, which depends on insertion order',
          _sample(node),
        );
      }
      final rust = mapMethodNames[name];
      if (rust == null) throw Unsupported('`Map.$name`', _sample(node));
      return IrCall(_receiver(node.receiver), rust, const []);
    }
    final receiver = node.receiver;
    // A field the enclosing closure copied in is a local now, not a field of
    // a `this` the closure does not hold. See `IrClosure.captures`.
    if (receiver is ThisExpression && _captured.contains(name)) {
      return IrLocal(name);
    }
    if (name == 'current' && receiver is VariableGet) {
      final element = _currentOf[receiver.variable];
      if (element != null) return IrLocal(element);
    }
    final target = receiver is ThisExpression
        ? null
        : _receiver(
            receiver,
            keepErased:
                _throughReceiver(
                  receiver,
                  node.interfaceTarget,
                  node.interfaceTarget.getterType,
                ) !=
                null,
          );
    // A getter whose landing member is a field this class holds (a mixin
    // clone's) is read as the field, typed as the clone declares it -- as
    // a write to it is stored (`_instanceSet`). Through the trait's getter
    // it came back as the erased bound (`_slotToChild`, ws373).
    if (node.interfaceTarget is Procedure &&
        !_heldField(node.interfaceTarget, receiver)) {
      final declared = node.interfaceTarget.getterType;
      return _acrossBinding(
        _qualified(
          IrCall(target, name, const []),
          node.interfaceTarget,
          receiver,
        ),
        declared,
        _bindingOf(declared, node.interfaceTarget, receiver),
        toOption: true,
      );
    }
    // A field of a `dart:` class the prelude re-expresses -- `Duration
    // .inMicroseconds` is a field in this SDK -- is a method there, as
    // every getter of such a class is.
    // Only where the prelude spells them as methods: `MapEntry.key` and
    // `SocketException.message` are fields there (11 E0599s when every
    // `dart:` class took this path, ws136).
    final owner = node.interfaceTarget.enclosingClass;
    if (owner != null &&
        owner.enclosingLibrary.importUri.scheme == 'dart' &&
        const {'Duration', 'DateTime'}.contains(owner.name)) {
      return IrCall(target, name, const []);
    }
    // A field declared on an *abstract* class is a trait accessor in Rust,
    // and a read through another object -- whatever its concrete class
    // stores -- goes through the accessor: `rc.x` took the value of a
    // method, 111 times in the leaf crates.
    final declaring = node.interfaceTarget.enclosingClass;
    // Not an anonymous mixin application: it is abstract to Kernel, and its
    // fields are flattened into the applying class's struct here
    // (`Get.isLogEnable` read as a getter call on a field, E0599).
    // ..unless the receiver's own static class is concrete: the struct has
    // the abstract base's field flattened in, and the field is read as one
    // (`Get.isLogEnable` on a `_GetImpl`, whose trait was not even in scope).
    // The class the receiver *is here*: a closure parameter retyped to an
    // erased bound reads back through a cast to the class it was declared
    // with (`_localRead`), whatever the type flow analysis narrowed the
    // static type to. Narrowed to a concrete subclass, the read came out
    // as a field access on a value that is still a trait object
    // (`notification.metrics` on a `dyn ScrollNotification`, ws721).
    final receiverType = target == null
        ? null
        // ..and through `_appliedBack`: inside a mixin's body borrowed from
        // an application the copy says `FlexParentData` where the trait
        // holds the erased bound, and a field read on a `dyn` is the
        // accessor (ws735).
        : _backHere(
            receiver is VariableGet && _retyped.containsKey(receiver.variable)
                ? receiver.variable.type
                : _staticType(receiver),
          );
    final concrete =
        receiverType is InterfaceType &&
        !_abstractLike(receiverType.classNode) &&
        receiverType.classNode.enclosingLibrary.importUri.scheme != 'dart';
    if (target != null &&
        declaring != null &&
        _abstractLike(declaring) &&
        !declaring.isAnonymousMixin &&
        !concrete) {
      final declared = node.interfaceTarget.getterType;
      return _acrossBinding(
        _qualified(
          IrCall(target, name, const []),
          node.interfaceTarget,
          receiver,
        ),
        declared,
        _bindingOf(declared, node.interfaceTarget, receiver),
        toOption: true,
      );
    }
    final read =
        IrField(
            target,
            name,
            // `PerformanceOverlayOption.x.index` in a static initialiser resolves
            // to `_Enum.index`, a field of a class that is not an enum; the
            // receiver's own type says it is one (4 "attempted to take value of
            // method `index`" in `rendering`).
            onEnum:
                (node.interfaceTarget.enclosingClass?.isEnum ?? false) ||
                (receiverType is InterfaceType &&
                    receiverType.classNode.isEnum),
            owner: target == null
                ? null
                : concrete && declaring != null && _abstractLike(declaring)
                ? (receiverType as InterfaceType).classNode.name
                : node.interfaceTarget.enclosingClass?.name,
          )
          ..rustType = _memberRustType(
            _landing(node.interfaceTarget, receiver),
            receiver,
            asGetter: true,
          );
    // Out of a projected field: into the `Option<T>` the body works with.
    final declared = node.interfaceTarget.getterType;
    return _acrossBinding(
      read,
      declared,
      _bindingOf(declared, node.interfaceTarget, receiver),
      toOption: true,
    );
  }

  /// Whether the callee takes a parameter whose declared type *is* its
  /// `i`th type parameter -- the one shape where the value's spelling at
  /// the edge and the type argument's have to agree (see the call above).
  static bool _spelledAsParameter(FunctionNode? callee, int i) {
    if (callee == null || i >= callee.typeParameters.length) return false;
    final p = callee.typeParameters[i];
    bool names(DartType t) =>
        t is TypeParameterType &&
        t.parameter == p &&
        t.nullability != Nullability.nullable;
    return callee.positionalParameters.any((v) => names(v.type)) ||
        callee.namedParameters.any((v) => names(v.type));
  }

  /// `dateTimeSymbols[k]`, `.containsKey(k)`, `.keys` on a `dynamic` slot
  /// with known types (see `dynamicSlots`): one arm per type, each giving
  /// the *same* Rust type -- what the Dart code does with the result is
  /// typed by the first arm's member. A `Map` arm answers as a map; a class
  /// arm calls its own member, or panics for one it does not have (Dart's
  /// `NoSuchMethodError`, which is what `UninitializedLocaleData` does).
  IrExpr? _dynamicSlotCall(Expression node) {
    final Expression receiver;
    final String name;
    final List<Expression> positional;
    if (node is DynamicInvocation) {
      receiver = node.receiver;
      name = node.name.text;
      positional = node.arguments.positional;
    } else if (node is DynamicGet) {
      receiver = node.receiver;
      name = node.name.text;
      positional = const [];
    } else {
      return null;
    }
    if (receiver is! StaticGet) return null;
    var target = receiver.target;
    // Through a getter that only reads the slot: `dynamic get
    // dateTimeSymbols => _dateTimeSymbols`.
    if (target is Procedure && target.isGetter) {
      final body = target.function.body;
      final read = body is ReturnStatement ? body.expression : null;
      if (read is StaticGet) target = read.target;
    }
    if (target is! Field) return null;
    final candidates = dynamicSlots[target];
    if (candidates == null || candidates.isEmpty) return null;
    // The slot itself is named here, through the getter (`injectedMembers`).
    injectedMembers.add(target);
    const known = {'[]', '[]=', 'containsKey', 'keys'};
    if (!known.contains(name)) return null;
    // A local handed in is shared, as an argument is (`_clonedWhenPassed`):
    // two dispatches in a row moved the key into the first.
    final args = [
      for (final e in positional)
        () {
          final lowered = expression(e);
          return lowered is IrLocal
              ? (IrCall(lowered, 'clone', const [])
                  ..rustType = lowered.rustType)
              : lowered;
        }(),
    ];
    final slot = IrLocal('__d');
    // A write into the slot's map (`dateTimeSymbols[locale] = symbols`,
    // intl's `initializeDateFormattingCustom`, run585): the map is a
    // value behind the slot's handle, so the copy the arm holds is
    // written and put back as the slot's object.
    final field = target;
    IrExpr noSuch() => IrLiteral(
      'panic!("uncaught Dart exception: NoSuchMethodError: `$name` on an ${candidates.first.classNode.name}")',
      const IrType('raw'),
    );
    final arms = <(IrType?, IrExpr)>[];
    for (final c in candidates) {
      // The arm's type is the slot census's answer, not this library's
      // (`injectedClasses`).
      _injected(c);
      final isMap =
          c.classNode.name == 'Map' || c.classNode.name == 'LinkedHashMap';
      final hasMember = c.classNode.members.any(
        (m) => m.name.text == name && !m.isAbstract,
      );
      final IrExpr body;
      switch (name) {
        case '[]':
          // The result is a `dynamic`, as the caller sees it (`as Sym`
          // on it, the isgeneric fixture): the object, or Dart's null.
          body = isMap
              ? IrStaticCall(null, 'dart_option_object', [
                  IrCall(slot, '!map_get', args),
                ])
              : hasMember
              ? IrStaticCall(null, 'dart_option_object', [
                  IrSome(IrCall(slot, '[]', args, fails: true)),
                ])
              : noSuch();
        case '[]=':
          if (!isMap) {
            body = noSuch();
            break;
          }
          final boxed = IrUpcast(
            IrLocal('__d')..rustType = _type(c),
            const IrType('Object'),
            handle: false,
            explicit: true,
          )..rustType = const IrType('Object');
          final store = field.enclosingClass == null
              ? IrAssignTopLevel(field.name.text, boxed)
              : IrAssignStatic(
                  field.enclosingClass!.name,
                  field.name.text,
                  boxed,
                );
          body = IrBlockValue([
            IrExprStmt(IrCall(slot, 'insert', args)),
            store,
          ], IrLiteral('()', const IrType('raw')));
        case 'containsKey':
          // The map's is the prelude's `contains_key(&k)`, spelled as the
          // `Map` lowering spells it so the backend passes the key by
          // reference; a class's is its own method, by value -- and a
          // translated method's `Result` (`UninitializedLocaleData.
          // containsKey` beside the map's `bool`, `DateFormat.localeExists`,
          // run590).
          body = isMap
              ? IrCall(slot, 'contains_key', args)
              : hasMember
              ? IrCall(slot, 'containsKey', args, fails: true)
              : noSuch();
        default:
          body = isMap
              ? IrCall(slot, name, args)
              : hasMember
              ? IrCall(slot, name, args, fails: true)
              : noSuch();
      }
      arms.add((_type(c), body));
    }
    return IrDynamicDispatch(expression(receiver), arms)
      ..rustType = switch (name) {
        '[]' => const IrType('dynamic'),
        'containsKey' => const IrType('bool'),
        _ => null,
      };
  }

  IrExpr _staticGet(StaticGet node) {
    final target = node.target;
    final enclosing = target.enclosingClass;
    if (enclosing == null) {
      // A top-level name. A `const` or `final` is a module constant in Rust
      // too; a computed `get foo => ...` is a function and stops here.
      // Mutable ones too, now that they are emitted. A read of one goes
      // through the cell, which the backend knows from the declaration.
      if (target is Field) {
        return IrTopLevel(target.name.text, module: _topLevelModule(target));
      }
      // A top-level getter is a function here, so reading it is calling it.
      if (target is Procedure && target.kind == ProcedureKind.Getter) {
        return IrStaticCall(
          null,
          target.name.text,
          const [],
          fails: _fails(target),
          diverges: _diverges(target),
          asyncFn: _asyncMember(target),
          module: _topLevelModule(target),
        );
      }
      throw Unsupported('top-level `${target.name.text}`', _sample(node));
    }
    // A static *getter* is a function -- `PlatformDispatcher.instance` --
    // and reading it is calling it, as for a top-level getter above. As a
    // static it was spelled `PlatformDispatcher::INSTANCE`, a constant
    // nothing declared (20 times).
    if (target is Procedure && target.kind == ProcedureKind.Getter) {
      return IrStaticCall(
        enclosing.name,
        target.name.text,
        const [],
        fails: _fails(target),
        diverges: _diverges(target),
        asyncFn: _asyncMember(target),
      );
    }
    return IrStatic(
      enclosing.name,
      target.name.text,
      // A *variant*, not merely a static of an enum: a Dart enum may
      // declare a static of its own, and spelling that as a variant
      // (`KeyboardLockMode::_knownLockModes`) named something the Rust enum
      // has never had.
      isEnumValue:
          enclosing.isEnum &&
          target is Field &&
          _isVariantOf(enclosing, target),
    );
  }

  /// Whether a call is an `Iterable`'s own member reaching a translated
  /// class that *is* an `Iterable`.
  ///
  /// Three questions, and all three had to be asked.
  ///
  /// *Where the member is declared*: a `dart:` class. `class Board extends
  /// Object with IterableMixin<BoardPoint?>` declares `elementAt` and
  /// `forEach` on `_MixinApplication386&Object&IterableMixin` in
  /// `dart:mixin_deduplication`, which no name test can catch -- which is
  /// why both fell through to an ordinary call: `board.element_at(i)` named
  /// nothing and `board.for_each(f)` asked `Board` to be a Rust `Iterator`.
  ///
  /// *Which member*: one `Iterable` itself declares. `_History extends
  /// Iterable with ChangeNotifier` has `notifyListeners` declared on a
  /// `dart:mixin_deduplication` class that is an `Iterable` too, and that is
  /// not an `Iterable` member at all (4 refusals reading
  /// "`List.notifyListeners`" when this asked only about the declaring
  /// class). `ObserverList.add` is the same shape.
  ///
  /// *Which receiver*: a translated class, which `_listReceiver` reads as
  /// its list (`__to_list`, ws499) -- the way in has to match the way the
  /// receiver is read. `Set`, `Queue`, `ListQueue` and `LinkedList` are
  /// `dart:` classes that are `Iterable`s and have their own branches
  /// below; routing them here refused 25 members `List` does not have
  /// (`Set.difference`, `ListQueue.addLast`, `Queue.removeFirst`).
  bool _dartIterableCall(Expression receiver, Class? declaring, String name) {
    if (declaring == null) return false;
    if (declaring.enclosingLibrary.importUri.scheme != 'dart') return false;
    final env = typeEnvironment;
    if (env == null) return false;
    final static = _staticType(receiver);
    if (static is! InterfaceType ||
        !_translatedClass(static.classNode) ||
        _iterableElement(static) == null) {
      return false;
    }
    return env.hierarchy.getInterfaceMember(
          env.coreTypes.iterableClass,
          Name(name),
        ) !=
        null;
  }

  /// The place Kernel's desugaring has to be undone.
  ///
  /// Every operator is a method call here, so `a + b` arrives as
  /// `InstanceInvocation(a, '+', [b])`. Left alone it would emit `a.add(b)`,
  /// which is neither what upstream wrote nor what the Rust backend's operator
  /// traits expect. Turning it back into a binary expression is what makes the
  /// two front ends produce the same IR.
  IrExpr _instanceInvocation(InstanceInvocation node) {
    final name = node.name.text;
    // The callee is passed so omitted optional arguments get their defaults.
    // Without it `weigh()` came out as a no-argument call against a
    // three-parameter function -- the same bug the analyzer front end had in
    // round two, living on here because nothing compared the two front ends on
    // a fixture that used defaults.
    // The member the call lands on, for `_landingSlot`: an anonymous mixin
    // application's copy of `_addDiagnostics(ChildType child)` has
    // `RenderBox` written in it, and only the mixin's own -- the trait's
    // -- takes the erased bound (188 `RenderBox` <- `RenderObject`, ws342).
    // `this` typed as the class being lowered (`_bindingOf` does the
    // same): its static type is not on the node, and without it a call on
    // `this` bound none of the class's own parameters -- `didUpdateValue(
    // oldValue)` took an `Option<T>` where the edge is `<T as
    // DartNullable>::Or` (`RestorableValue.value=`, run633).
    final env = typeEnvironment;
    final receiverType = node.receiver is ThisExpression && env != null
        ? _lowering?.getThisType(env.coreTypes, Nullability.nonNullable)
        : _staticType(node.receiver);
    final dispatch = receiverType is InterfaceType
        ? typeEnvironment?.hierarchy.getDispatchTarget(
            receiverType.classNode,
            node.name,
          )
        : null;
    final wasDispatch = _dispatchMember;
    final wasReceiver = _dispatchReceiverType;
    final wasInterface = _dispatchInterface;
    // ..the mixin's own declaration behind a copy in an application, as
    // the copy is typed everywhere (`_originalOf`).
    final dispatchOriginal = dispatch == null ? null : _originalOf(dispatch);
    // An *abstract* target has no dispatch target; the interface member
    // is the landing then, and still binds the class's parameters for
    // the arguments (`didUpdateValue(oldValue)` on `RestorableValue<T>`,
    // the projarg fixture).
    final interfaceTarget = node.interfaceTarget;
    _dispatchMember = dispatchOriginal is Procedure
        ? dispatchOriginal
        : interfaceTarget is Procedure
        ? interfaceTarget
        : null;
    _dispatchReceiverType = receiverType;
    _dispatchInterface = node.interfaceTarget.function;
    // A prelude method's slots as its sibling declares them
    // (`preludeSiblings`: `Set.removeAll(Iterable<Object?>)` takes the
    // set's own `E` here, as `addAll` does), instantiated with the
    // receiver's type arguments.
    final interface = node.interfaceTarget;
    final sibling = interface is Procedure
        ? _preludeDeclaration(interface)
        : null;
    final calleeFunction = sibling?.function ?? interface.function;
    FunctionType? instantiated = node.functionType;
    if (sibling != null && !identical(sibling, interface)) {
      final own = sibling.function.computeFunctionType(Nullability.nonNullable);
      instantiated = receiverType is InterfaceType
          ? Substitution.fromInterfaceType(receiverType).substituteType(own)
                as FunctionType
          : own;
    } else if (interface is Procedure &&
        interface.enclosingClass != null &&
        preludeSiblings.containsKey(
          '${interface.enclosingClass!.name}.${interface.name.text}',
        ) &&
        receiverType is InterfaceType &&
        receiverType.typeArguments.isNotEmpty) {
      // The sibling itself was tree-shaken out of the dill: the slots it
      // would have declared, spelled directly -- an `Iterable<Object?>`
      // parameter takes the collection's own elements.
      final element = receiverType.typeArguments.first;
      final own = interface.function.computeFunctionType(
        Nullability.nonNullable,
      );
      DartType elements(DartType p) =>
          p is InterfaceType &&
              p.classNode.name == 'Iterable' &&
              p.typeArguments.length == 1 &&
              _isTopType(p.typeArguments.single)
          ? InterfaceType(p.classNode, p.nullability, [element])
          : p;
      instantiated = FunctionType(
        [for (final p in own.positionalParameters) elements(p)],
        own.returnType,
        Nullability.nonNullable,
        namedParameters: own.namedParameters,
        requiredParameterCount: own.requiredParameterCount,
      );
    }
    final List<IrExpr> args;
    // A number's own method takes its own type where Dart writes `num`:
    // `x.clamp(0, 1)` on a `double` is `clamp(0.0, 1.0)`, and Rust's
    // `f64::clamp` takes no integer literal. Dart's `num` is not a type
    // here, so the receiver says which one it is
    // (`_MobileCarouselState.builder`, run722).
    final wasNumReceiver = _numReceiver;
    _numReceiver = receiverType is InterfaceType
        ? (const {'double', 'int'}.contains(receiverType.classNode.name)
              ? receiverType.classNode.name
              : null)
        : null;
    try {
      // With the call's type arguments for the method's own parameters,
      // as a static generic call has them (`_withGenericArgs`): `pop<T>
      // (result)` inside `maybePop<T>` binds the callee's `T` to the
      // caller's, which a projected `T?` slot has to know (19 at ws421).
      args = _withGenericArgs(
        calleeFunction,
        node.arguments,
        () => _arguments(
          node.arguments,
          calleeFunction,
          true,
          instantiated,
          null,
          null,
          _preludeSlots(node),
        ),
      );
    } finally {
      _dispatchMember = wasDispatch;
      _dispatchReceiverType = wasReceiver;
      _dispatchInterface = wasInterface;
      _numReceiver = wasNumReceiver;
    }
    // The owner by the receiver's *static* class when that is one of the
    // prelude's collections: TFA devirtualises `Map.cast` onto the one
    // implementation it found (`CanonicalizedMap`, a generic method on a
    // trait), and the prelude's `Map` is what the receiver is here
    // (`invokeMapMethod`, run492).
    // The receiver's static type outright, not `_staticClass`, which
    // answers only translated classes and so never a `dart:core` one
    // (the rule was silent through ws494).
    final staticReceiver = _staticType(node.receiver);
    final receiverClass = staticReceiver is InterfaceType
        ? staticReceiver.classNode
        : null;
    final staticOwner = receiverClass?.name;
    final collectionReceiver =
        receiverClass != null &&
        (staticOwner == 'List' ||
            staticOwner == 'Iterable' ||
            staticOwner == 'Set' ||
            _isMapClass(staticOwner)) &&
        receiverClass.enclosingLibrary.importUri.scheme == 'dart';
    final generic = collectionReceiver ? null : _genericOnTrait(node, args);
    if (generic != null) return generic;
    // The owner the lowering tables are keyed by is the *declaring* class
    // (`Iterable` for a `Set`'s `any`, ws496) -- unless TFA devirtualised
    // the target onto a translated class (`CanonicalizedMap.cast`), where
    // the receiver's static collection is the owner.
    final declaringOwner = node.interfaceTarget.enclosingClass;
    final devirtualised =
        collectionReceiver &&
        declaringOwner != null &&
        declaringOwner.enclosingLibrary.importUri.scheme != 'dart';
    if (staticOwner == 'Iterable') {
      iterableMembers.update(node.name.text, (n) => n + 1, ifAbsent: () => 1);
    }
    final owner = devirtualised ? staticOwner : declaringOwner?.name;
    // A `StreamView` subclass's inherited `listen` and friends act on the
    // `_stream` it carries (see `lowerClass`).
    final declaringStream = node.interfaceTarget.enclosingClass;
    if (node.receiver is ThisExpression &&
        declaringStream != null &&
        (declaringStream.name == 'Stream' ||
            declaringStream.name == 'StreamView') &&
        declaringStream.enclosingLibrary.importUri.toString() == 'dart:async') {
      return IrCall(IrField(null, '_stream'), name, args);
    }
    // `child.toString()` on a `Listenable?`: an `Option` has no
    // `to_string`, and `dart_str` prints `null` for the absent case as
    // Dart does.
    if (name == 'toString' && args.isEmpty) {
      final t = _staticType(node.receiver);
      // The receiver as it is, not narrowed to its bound (`_receiver`):
      // a `T` receiver went behind a fresh `Rc<T>` and printed as one.
      final asObject = _stringOf(expression(node.receiver), t, explicit: true);
      if (asObject != null) return asObject;
    }
    if (owner == 'List' || _isMapClass(owner) || owner == 'Iterable') {
      // A collection member is a *Rust* method taking `impl Fn`, so a closure
      // given to one is not boxed. `_keeps` cannot say so: the callee is
      // `dart:core`'s, with no body to read, and it answers "kept" for want of
      // evidence. The analyzer front end has no such analysis and said
      // unboxed, so the two wrote different Rust for `m.forEach(..)`.
      for (var i = 0; i < args.length; i++) {
        args[i] = _unboxed(args[i]);
      }
    }
    // `completer.complete()` with the argument left off is Dart's
    // `complete(null)`, and the prelude's `complete` takes `<T as
    // DartNullable>::Or` -- so what to pass is the *null of the completer's
    // own type argument*, which this used to assume was always `void`.
    // `Route<T>._disposeCompleter` is a `Completer<T?>`, its `Or` is an
    // `Option`, and `()` there was E0308 (the completervoid fixture; the
    // stub that stopped run901 once microtasks began running).
    if (owner == 'Completer' &&
        name == 'complete' &&
        (args.isEmpty ||
            (args.length == 1 && node.arguments.positional.isEmpty))) {
      final held = _staticType(node.receiver);
      final completed = held is InterfaceType && held.typeArguments.length == 1
          ? held.typeArguments.single
          : null;
      final slot = completed == null ? null : _edgeType(completed);
      return IrCall(_receiver(node.receiver), 'complete', [
        // `void`'s only value is the unit, and its `Or` is the unit too.
        if (completed == null || completed is VoidType)
          IrLiteral('()', const IrType('raw'))
        // Into a projected `T?` the null crosses by `from_option`, as it
        // does into any such slot (`IrNullableOf`; a bare `None` there is
        // "expected associated type").
        else if (slot != null && slot.projected)
          IrNullableOf(_nullLiteral(), slot.name, toOption: false)
        else
          _nullLiteral(),
      ]);
    }
    // `s[i]` on a String is a one-character String, not an index into a
    // list: `pattern[0] == "a"` in intl's date formatting (44 + 44).
    // `[3, 4, 5].contains(n % 100)` with `n` a `num`: Dart compares by
    // value (`3 == 3.0`), so the `double` is cast to the list's `int`.
    // `xs.cast<T2>()` / `m.cast<K2, V2>()` on any of the prelude's
    // collections: its `cast_to`, converting element representations
    // (`FromDynamic`) as `as List<T2>` does. TFA had devirtualised
    // `Map.cast` onto a `CanonicalizedMap` (`invokeMapMethod`, run491).
    if ((owner == 'List' ||
            owner == 'Iterable' ||
            owner == 'Set' ||
            _isMapClass(owner)) &&
        name == 'cast' &&
        args.isEmpty &&
        node.arguments.types.isNotEmpty) {
      return IrCall(
        // The list it is read as: `cast_to` is written over one, and an
        // `Iterable` receiver is a handle since ws908 (`_listReceiver`).
        _listReceiver(node.receiver, name),
        'cast_to',
        const [],
        typeArguments: [for (final t in node.arguments.types) _type(t)],
      );
    }
    // The prelude's `Set::remove` takes the value by reference, like the
    // map's key (`_tickers.remove(ticker)`, 46).
    if (owner == 'Set' && name == 'remove' && args.length == 1) {
      return IrCall(_listReceiver(node.receiver), '!map_remove', [
        _intoElement(
          args.single,
          node.arguments.positional.single,
          _staticType(node.receiver),
        ),
      ]);
    }
    if ((owner == 'List' || owner == 'Iterable' || owner == 'Set') &&
        name == 'contains' &&
        args.length == 1) {
      final listType = _staticType(node.receiver);
      final argType = _staticType(node.arguments.positional.single);
      final element =
          listType is InterfaceType && listType.typeArguments.isNotEmpty
          ? listType.typeArguments.first
          : null;
      if (element is InterfaceType &&
          element.classNode.name == 'int' &&
          argType is InterfaceType &&
          (argType.classNode.name == 'double' ||
              argType.classNode.name == 'num')) {
        return IrCall(_listReceiver(node.receiver), '!contains', [
          IrCast(args.single, 'i64'),
        ]);
      }
      return IrCall(_listReceiver(node.receiver), '!contains', [
        _intoElement(args.single, node.arguments.positional.single, listType),
      ]);
    }
    // A `String` member whose `Pattern` arrived as a `RegExp`: the member
    // is the regular expression's, with the string as its first argument.
    // Only the *static* type of the argument tells the two apart, and only
    // here is it known -- by the time the call reaches the backend the
    // pattern is an expression with no Dart type on it
    // (`_DateFormatQuotedField._patchQuotes`).
    final patternMember = regexpPatternMember[name];
    if (owner == 'String' && patternMember != null && args.isNotEmpty) {
      final pattern = _staticType(node.arguments.positional.first);
      if (pattern is InterfaceType && pattern.classNode.name == 'RegExp') {
        return IrCall(args.first, patternMember, [
          _receiver(node.receiver),
          ...args.skip(1),
        ]);
      }
    }
    if (owner == 'String' && name == '[]' && args.length == 1) {
      return IrCall(_listReceiver(node.receiver), 'char_at', args);
    }
    // `trim()` and friends: `str::trim` hands back a `&str`, and being
    // inherent it wins over a trait method of the same name.
    if (owner == 'String' &&
        const {'trim', 'trimLeft', 'trimRight'}.contains(name) &&
        args.isEmpty) {
      const spelled = {
        'trim': 'trim_dart',
        'trimLeft': 'trim_left_dart',
        'trimRight': 'trim_right_dart',
      };
      return IrCall(_listReceiver(node.receiver), spelled[name]!, const []);
    }
    if (owner == 'String' && name == 'split' && args.length == 1) {
      // `s.split(p)`: Rust's `split` wants a `&str` and yields an iterator.
      return IrCall(_listReceiver(node.receiver), 'split_dart', args);
    }
    if (owner == 'String' && name == '*' && args.length == 1) {
      // `'0' * n`: Rust's `repeat` wants a `usize`.
      return IrCall(_listReceiver(node.receiver), 'repeat_dart', args);
    }
    if (owner == 'String' &&
        name == 'contains' &&
        (args.length == 1 || args.length == 2)) {
      // `contains(other, [start])`: `str::contains` is inherent, takes a
      // `&str`, and has no start; the prelude's `contains_dart` has both.
      return IrCall(_listReceiver(node.receiver), 'contains_dart', [
        args.first,
        if (args.length == 2) args[1] else IrLiteral('0', const IrType('int')),
      ]);
    }
    if (owner == 'String' && name == 'startsWith' && args.length == 2) {
      // `startsWith(pattern, index)`: `str::starts_with` takes one argument
      // and, being inherent, would win over a trait method of the same name.
      return IrCall(_listReceiver(node.receiver), 'starts_with_at', args);
    }
    if (owner == 'String' && name == 'replaceRange' && args.length == 3) {
      // Dart's `replaceRange` returns a new string; Rust's `String` has an
      // inherent `replace_range` that mutates in place and takes a range,
      // and an inherent method shadows a trait's. So the prelude's is named
      // apart.
      return IrCall(_listReceiver(node.receiver), 'replace_range_dart', args);
    }
    if (owner == 'Expando') {
      // `expando[object]` / `expando[object] = v`: identity-keyed, so the
      // prelude's `get`/`set` rather than an index. 6 uses.
      if (name == '[]' && args.length == 1) {
        return IrCall(_listReceiver(node.receiver), '!expando_get', [
          args.single,
        ]);
      }
      if (name == '[]=' && args.length == 2) {
        return IrCall(_listReceiver(node.receiver), '!expando_set', args);
      }
    }
    // A typed list with a narrow element -- `Float32List` is `Vec<f32>`,
    // `Int32List` is `Vec<i32>` -- takes Dart's `double`/`int` cast down on
    // the way in and up on the way out. 23 `f32 <= f64` and 14 `i32`/`i64`
    // in `dart:ui`'s colour and vertex code.
    final narrow = _narrowElement(_staticType(node.receiver));
    if (narrow != null && name == '[]' && args.length == 1) {
      return IrCast(
        IrIndex(_listReceiver(node.receiver), args.single),
        narrow.startsWith('f') ? 'f64' : 'i64',
      );
    }
    if (narrow != null && name == '[]=' && args.length == 2) {
      final held = '__t${_nextTemporary++}';
      return IrBlockValue([
        IrLocalDecl(held, null, args[1]),
        IrIndexSet(
          _listReceiver(node.receiver, name),
          args[0],
          IrCast(
            IrCall(IrLocal(held), 'clone', const [])
              ..rustType = args[1].rustType,
            narrow,
          ),
        ),
      ], IrLocal(held));
    }
    if (owner == 'List' ||
        owner == 'Iterable' ||
        _dartIterableCall(node.receiver, declaringOwner, name)) {
      if (name == '[]' && args.length == 1) {
        // Typed by the list's element, which a generic class's `List<E?>`
        // keeps projected (`<E as DartNullable>::Or`) where the static type
        // of the read says a plain `E?`.
        final list = _listReceiver(node.receiver, name);
        final element = list.rustType?.arguments.length == 1
            ? list.rustType!.arguments.single
            : null;
        return IrIndex(list, args.single)..rustType = element;
      }
      if (name == '[]=' && args.length == 2) {
        // `xs[i] = v` where the expression's value is wanted -- the CFE puts
        // `xs[i] += 1` into a `Let` whose body is this call. Bound, stored as
        // a clone, produced: the same shape every other assignment-as-value
        // takes here. 48 of them.
        final held = '__t${_nextTemporary++}';
        return IrBlockValue([
          IrLocalDecl(held, null, args[1]),
          IrIndexSet(
            _listReceiver(node.receiver, name),
            args[0],
            IrCall(IrLocal(held), 'clone', const [])
              ..rustType = args[1].rustType,
          ),
        ], IrLocal(held));
      }
      // `whereType<T>()`: the elements that are a `T`, by the cast table
      // (`_semantics` nodes filtered in `RenderObject`, run527).
      if (name == 'whereType' &&
          args.isEmpty &&
          node.arguments.types.length == 1) {
        final wantedDart = node.arguments.types.single;
        // `whereType<T>()` over an `Iterable<T?>` is the elements that are
        // there: no runtime test says more than that, and for a function
        // type there is no test at all -- `dart_cast_to::<Function>` named
        // a type nothing declares (`whereType<ImageErrorListener>()` over
        // the listeners' `onError` in `ImageStreamCompleter.reportError`,
        // ws762).
        final receiverType = _staticType(node.receiver);
        final element =
            receiverType is InterfaceType &&
                receiverType.typeArguments.length == 1
            ? receiverType.typeArguments.single
            : null;
        if (element != null &&
            element.nullability == Nullability.nullable &&
            element.withDeclaredNullability(Nullability.nonNullable) ==
                wantedDart) {
          final kept = _type(wantedDart);
          return IrCall(
            _listReceiver(node.receiver, name),
            '!where_present',
            const [],
          )..rustType = IrType('List', arguments: [kept]);
        }
        final wanted = _type(wantedDart);
        return IrCall(
          _listReceiver(node.receiver, name),
          '!where_type',
          const [],
          typeArguments: [wanted],
        )..rustType = IrType('List', arguments: [wanted]);
      }
      final step = iterStepNames[name];
      if (step != null && args.length == 1) {
        // A chain, extended rather than started again when the receiver is
        // already one: `xs.where(f).map(g)` is one `iter()`, not two.
        final source = _listReceiver(node.receiver, name);
        return source is IrIterChain
            ? IrIterChain(source.source, [...source.steps, (step, args.single)])
            : IrIterChain(source, [(step, args.single)]);
      }
      // `lastWhere` is the same shape read from the other end, and the
      // same two prelude methods (`NavigatorState.pop`, ws810).
      if ((name == 'firstWhere' || name == 'lastWhere') && args.length == 2) {
        final where = name == 'firstWhere' ? 'first_where' : 'last_where';
        // `firstWhere(test)` throws when nothing matches; with `orElse` it
        // calls that instead. The omitted `orElse` arrives as `None`, and a
        // generic `impl Fn` parameter cannot take a `None`, so the two are
        // two prelude methods. 25 calls.
        final orElse = args[1];
        final omitted = orElse is IrLiteral && orElse.type.name == 'Null';
        // ..and a *given* one goes in bare: Dart's slot is `E Function()?`
        // and the coercion wrapped it, where the prelude's parameter is a
        // plain `impl Fn()` (`FlutterErrorDetails.summary`, run730).
        var given = orElse is IrSome ? orElse.value : orElse;
        given = given is IrCall && given.name == '!rc' && given.args.isEmpty
            ? given.target!
            : given;
        given = _unboxed(given);
        return IrCall(
          _listReceiver(node.receiver, name),
          omitted ? where : '${where}_or',
          omitted ? [args[0]] : [args[0], given],
        );
      }
      if (name == 'sort') {
        // `sort()` is the natural order Dart's `Comparable` gives
        // (`sort_natural`); `sort(compare)` takes a Dart comparator
        // returning an `int`, which the prelude's `sort_by_dart` turns into
        // an `Ordering`. 36 of these. An *omitted* comparator arrives as
        // the `null` default and is the first of the two, not the second
        // with a `None` (`FlutterError.defaultStackFilter`, run728).
        final given = args.length == 1 ? args.single : null;
        final omitted =
            args.isEmpty || (given is IrLiteral && given.type.name == 'Null');
        return IrCall(
          _listReceiver(node.receiver, name),
          omitted ? 'sort_natural' : 'sort_by_dart',
          omitted ? const [] : args,
        );
      }
      final rust = listMethodNames[name];
      if (rust != null) {
        // An element handed to one of these goes into the element type,
        // which the prelude's `T`/`&T` cannot coerce to (see `_intoElement`
        // and `listElementArgument`). Which argument it is depends on the
        // member: `insert(index, element)` is the second.
        final at = listElementArgument[name];
        final receiver = _listReceiver(node.receiver, name);
        final call = IrCall(
          receiver,
          rust,
          at == null || at >= args.length
              ? args
              : [
                  for (var i = 0; i < args.length; i++)
                    if (i == at)
                      _intoElement(
                        args[i],
                        node.arguments.positional[i],
                        _staticType(node.receiver),
                      )
                    else
                      args[i],
                ],
        );
        return call;
      }
      throw Unsupported('`List.$name`', _sample(node));
    }
    if (_isMapClass(owner)) {
      // Its own name: the backend's `.get(&k).cloned()` was keyed on `get`
      // and fired on `ContrastCurve.get(double)` too (14 `&f64`).
      if (name == '[]' && args.length == 1) {
        // The read is typed by the map's own value type -- the receiver's
        // recorded `rustType`, which an erased map keeps as the bound --
        // and a slot it goes into coerces it (`_slotToChild[slot]` returned
        // as the `RenderBox?` `childForSlot` declares).
        IrExpr typed(IrExpr read) {
          final map = read is IrCall ? read.target?.rustType : null;
          if (map != null && map.name == 'Map' && map.arguments.length == 2) {
            final value = map.arguments[1];
            // ..with its signature kept: rebuilt by name, a `Map<String,
            // VoidCallback>`'s value read as a bare `Function` -- an
            // object -- where the Rust value is the typed `Rc<dyn Fn>`
            // (`_customActionCallbacks[id]`, ws515).
            read.rustType = value.isFunction
                ? IrType.function(
                    value.parameters!,
                    value.returns!,
                    nullable: true,
                  )
                : IrType(
                    value.name,
                    nullable: true,
                    arguments: value.arguments,
                  );
          }
          return read;
        }

        // `_cache[tone]` on a `Map<int, _>` with a `num` key: the key is an
        // `f64` here and the map's is `i64`, the same cast `contains` makes.
        final mapType = _staticType(node.receiver);
        final argType = _staticType(node.arguments.positional.single);
        final key = mapType is InterfaceType && mapType.typeArguments.isNotEmpty
            ? mapType.typeArguments.first
            : null;
        if (key is InterfaceType &&
            key.classNode.name == 'int' &&
            argType is InterfaceType &&
            (argType.classNode.name == 'double' ||
                argType.classNode.name == 'num')) {
          return typed(
            IrCall(_receiver(node.receiver), '!map_get', [
              IrCast(args.single, 'i64'),
            ]),
          );
        }
        // A nullable key into a map of non-nullable ones: `_views[_implicitViewId]`.
        // ..by the key's Rust type: an `Object?` key is a `dynamic`, no
        // `Option` (ws501).
        final keyIr = args.single.rustType;
        if (key != null &&
            key.nullability != Nullability.nullable &&
            keyIr != null &&
            isNullable(keyIr)) {
          return typed(IrCall(_receiver(node.receiver), '!map_get_opt', args));
        }
        // The key into the map's key type by the one rule: a `String` into
        // a `Map<Object?, ..>` goes behind a handle (15 at ws421).
        final keyed = key == null
            ? args.single
            : _intoArgument(node.arguments.positional.single, key, args.single);
        return typed(IrCall(_receiver(node.receiver), '!map_get', [keyed]));
      }
      // `m[k] = v`: `insert`, as a statement or for its value (Dart's is
      // `v`; here the old value, which no caller reads).
      if (name == '[]=' && args.length == 2) {
        return IrCall(
          _receiver(node.receiver),
          'insert',
          _mapEntry(node, args),
        );
      }
      if (orderedMapMembers.contains(name)) {
        throw Unsupported(
          '`Map.$name`, which depends on insertion order',
          _sample(node),
        );
      }
      final rust = mapMethodNames[name];
      if (rust == null) throw Unsupported('`Map.$name`', _sample(node));
      // `Map<int, _>.containsKey(tone)` with a `double`: Dart's `3.0 == 3`
      // finds the key, so the `double` is cast to the map's `int`.
      if (const {'containsKey', 'remove', '[]'}.contains(name) &&
          args.length == 1) {
        final mapType = _staticType(node.receiver);
        final key = mapType is InterfaceType && mapType.typeArguments.isNotEmpty
            ? mapType.typeArguments.first
            : null;
        final argType = _staticType(node.arguments.positional.single);
        if (key is InterfaceType &&
            key.classNode.name == 'int' &&
            argType is InterfaceType &&
            argType.classNode.name == 'double') {
          return IrCall(_receiver(node.receiver), rust, [
            IrCast(args.single, 'i64'),
          ]);
        }
        // ..and into the map's key type by the one rule, as `m[k]` is: a
        // `String` into a `Map<Object?, ..>.containsKey` goes behind a
        // handle (`decodeMethodCall`, ws491).
        if (key != null) {
          return IrCall(_receiver(node.receiver), rust, [
            _widened(node.arguments.positional.single, key, args.single),
          ]);
        }
      }
      return IrCall(_receiver(node.receiver), rust, args);
    }
    // A comparison (or any operator outside `stdOperators`) that a
    // translated class declares is that class's method here (`ge`, `lt`):
    // `getWindowType(context) >= AdaptiveWindowType.medium` was a Rust
    // `>=` on a struct with no `PartialOrd` (run643). Only the std
    // operators (`+`, `-`, ..) have an operator trait impl to reach.
    final operatorOwner = node.interfaceTarget.enclosingClass;
    final userOperator =
        operatorOwner != null &&
        _translatedClass(operatorOwner) &&
        !stdOperators.contains(name);
    if (_binaryOperators.contains(name) && args.length == 1 && !userOperator) {
      // `int * double` is a `double` in Dart and a type error in Rust: the
      // `int` side is cast. The receiver's class is the operator's owner;
      // the argument's is asked of the static types.
      var left = _receiver(node.receiver);
      var right = args.single;
      // Comparisons too: `returnValue < 0` on a `double` is `f64 < integer`
      // in Rust until the literal is cast (6 in the colour code).
      // `targetWidth! ~/ (w / h)`: an `int ~/ double` is a `double`
      // division truncated to an `int` in Dart. Both sides go to `f64`
      // and the truncated result comes back to `i64`.
      if (name == '~/') {
        String? classOf(Expression e) {
          final t = _staticType(e);
          return t is InterfaceType ? t.classNode.name : null;
        }

        final leftClass = classOf(node.receiver);
        final rightClass = classOf(node.arguments.positional.single);
        if (leftClass == 'double' || rightClass == 'double') {
          if (leftClass == 'int') left = _toF64(left);
          if (rightClass == 'int') right = _toF64(right);
          return IrCast(
            IrBinary(name, left, right, type: const IrType('double')),
            'i64',
          );
        }
      }
      if (const {
        '+',
        '-',
        '*',
        '/',
        '%',
        '<',
        '>',
        '<=',
        '>=',
      }.contains(name)) {
        String? classOf(Expression e) {
          final t = _staticType(e);
          return t is InterfaceType ? t.classNode.name : null;
        }

        // The receiver's *static* class, not the operator's owner: an
        // `int * double` may resolve to `num.*`.
        final leftClass = classOf(node.receiver);
        final rightClass = classOf(node.arguments.positional.single);
        // Not `num`: a static type of `num` is an `i64` as often as an
        // `f64` in the output (round ws49: 580 casts the wrong way).
        if (leftClass == 'int' && rightClass == 'double') {
          left = _toF64(left);
        }
        if (leftClass == 'double' && rightClass == 'int') {
          right = _toF64(right);
        }
        // A *declared* `num` -- a variable, field or static whose declaration
        // says `num`, an `f64` here -- against an int literal: the literal
        // is cast. Not the static type: `getStaticType` says `num` for an
        // `int` assignment used as a value (`(index = next()) >= 0`), and a
        // cast on that went wrong 200 times (ws53).
        final argument = node.arguments.positional.single;
        if (_declaredNum(node.receiver) &&
            (argument is IntLiteral || classOf(argument) == 'int')) {
          right = _toF64(right);
        } else if (_declaredNum(argument) && leftClass == 'int') {
          left = _toF64(left);
        }
        // Dart's `/` is always a `double`, even on two `int`s (`~/` is the
        // integer one); Rust's `/` on two `i64`s is an `i64`.
        if (name == '/') {
          if (leftClass == 'int') left = _toF64(left);
          if (rightClass == 'int') right = _toF64(right);
          // `targetWidth! / (w / h)`: whatever the static type of the left
          // side says, a `/` with a `double` right side is a `double`
          // division, and Rust has no `i64 / f64`.
          // ..when the left side is a number: a class's own `/` takes
          // what it declares (`BoxConstraints / double` in
          // `ViewConfiguration.fromView`, cast to `f64`, ws474).
          if (rightClass == 'double' &&
              (leftClass == 'int' || leftClass == 'num')) {
            left = _toF64(left);
          }
        }
      }
      return IrBinary(
        name,
        left,
        right,
        // The invocation's own function type says what the operator returns.
        // `getStaticType` would need a StaticTypeContext this lowering does
        // not build, and the function type is already here.
        type: node.functionType == null
            ? null
            : _type(node.functionType!.returnType),
      );
    }
    if (name == 'unary-' && args.isEmpty) {
      return IrUnary('-', _receiver(node.receiver));
    }
    // Dart's `double.floor()`/`ceil()`/`round()` are `int`s; Rust's are
    // `f64`s, inherent, and so not renameable through `DartDouble`. 10
    // `i64 <= f64` in material_color_utilities' HCT solver.
    // A `num` method on a `dynamic` receiver -- `number.isInfinite` in
    // intl's `format(dynamic number)`, devirtualised to `num.isInfinite` by
    // TFA: the receiver is downcast to the `f64` a `num` is here. An `int`
    // inside the `Rc<dyn Object>` would fail that downcast, loudly.
    final receiverStatic = _staticType(node.receiver);
    if ((receiverStatic is DynamicType ||
            (receiverStatic is InterfaceType &&
                receiverStatic.classNode.name == 'Object')) &&
        const {'num', 'int', 'double'}.contains(owner) &&
        const {
          'isInfinite',
          'isNaN',
          'isFinite',
          'round',
          'floor',
          'ceil',
          'truncate',
          'toDouble',
          'toInt',
          'abs',
          'toStringAsFixed',
        }.contains(name)) {
      final asDouble = IrCall(
        IrDowncast(_receiver(node.receiver), 'f64'),
        'clone',
        const [],
      );
      final rounds =
          const {'floor', 'ceil', 'round'}.contains(name) && args.isEmpty;
      final call = IrCall(asDouble, name, args);
      // The value in hand is what the emitted Rust produced, not what Dart's
      // static type says. The receiver was narrowed to an `f64` right here,
      // so this is `f64`'s method (or the prelude's `DartDouble`), and a
      // slot that takes an `Object` has to box the result. Left untyped, the
      // `Let` the CFE binds an interpolation's argument in declared its
      // temporary at the *static* type -- `dynamic`, an `Rc<dyn Object>` --
      // over an `f64`, and nothing coerced between them (`NumberFormat
      // .format` and `_formatFixed`, ws895).
      if (rounds) {
        call.rustType = const IrType('double');
        return IrCast(call, 'i64')..rustType = const IrType('int');
      }
      final produced = _narrowedNumResult[name];
      if (produced != null) call.rustType = produced;
      return call;
    }
    if (const {'floor', 'ceil', 'round'}.contains(name) && args.isEmpty) {
      final receiverType = _staticType(node.receiver);
      if (receiverType is InterfaceType &&
          (receiverType.classNode.name == 'double' ||
              receiverType.classNode.name == 'num')) {
        return IrCast(IrCall(_receiver(node.receiver), name, const []), 'i64');
      }
    }
    final receiver = node.receiver;
    // A translated generic method's type arguments (see `IrCall.
    // typeArguments`); a `dart:` class's method takes none in the prelude.
    final target = node.interfaceTarget;
    final withTypeArgs =
        target is Procedure &&
        target.function.typeParameters.isNotEmpty &&
        target.enclosingClass != null &&
        _translatedClass(target.enclosingClass!) &&
        node.arguments.types.length == target.function.typeParameters.length;
    final call = _qualified(
      IrCall(
        receiver is ThisExpression
            ? null
            : _receiver(
                receiver,
                keepErased:
                    _throughReceiver(
                      receiver,
                      node.interfaceTarget,
                      node.interfaceTarget.function.returnType,
                    ) !=
                    null,
              ),
        name,
        args,
        // At the *edge*, as the arguments are -- but only where the two
        // spellings actually meet: a callee parameter whose declared type
        // *is* that type parameter. `complete<T>(T result)` called with
        // `T?` takes the value projected (`<T as DartNullable>::Or`), so
        // the turbofish has to say the same or the two disagree
        // (`entry.complete::<Option<T>>(<T as DartNullable>::from_option
        // (result))`, `NavigatorState.removeRoute`).
        //
        // Not otherwise. Where the type parameter only shapes the
        // *return*, the projected spelling puts the nesting out by one:
        // `resourcesFor<T>(..)` hands back a `T?`, and the caller flattens
        // an `Option<Option<T>>` that an `Option<Or>` is not
        // (`Localizations.of`, +1 stub when this was unconditional).
        typeArguments: withTypeArgs
            ? [
                for (final (i, t) in node.arguments.types.indexed)
                  _spelledAsParameter(calleeFunction, i)
                      ? _edgeType(t)
                      : _type(t),
              ]
            : const [],
      ),
      node.interfaceTarget,
      receiver,
    );
    // The call's own result type, on the call itself: the projection
    // below wraps it, and `expression` types only the wrapper, which left
    // a generic method's call untyped -- and the erased twin's cast back
    // (`dart_cast_erased`) spells that type (`find<T>()` returning `T?`,
    // ws496). A `T?` of this declaration's own parameter comes back
    // projected (`<T as DartNullable>::Or`), as the callee's declared `T?`
    // return does.
    if (withTypeArgs) {
      final static = _staticType(node);
      // ..projected only when the callee's `T` is put in as a *bare*
      // parameter of this declaration: `resourcesFor<T?>(..)` returns
      // `<Option<T> as DartNullable>::Or`, a plain `Option<T>`, and typed
      // projected it was wrapped twice (`Localizations.of`, ws503).
      final declaredReturn = target.function.returnType;
      final bareArgument =
          declaredReturn is TypeParameterType &&
          target.function.typeParameters.contains(declaredReturn.parameter) &&
          () {
            final index = target.function.typeParameters.indexOf(
              declaredReturn.parameter,
            );
            final argument = node.arguments.types[index];
            return argument is TypeParameterType &&
                argument.nullability != Nullability.nullable;
          }();
      if (static is TypeParameterType &&
          static.nullability == Nullability.nullable &&
          !_erasedParameter(static.parameter) &&
          bareArgument) {
        call.rustType = IrType(
          static.parameter.name ?? 'T',
          nullable: true,
          projected: true,
        );
      } else if (static != null) {
        try {
          // A `T?` bound to a top type is the `Option<Rc<dyn Object>>`
          // the callee hands back, as `expression` types a read
          // (`invokeMethod<dynamic>(..)` into a `dynamic` local, ws513).
          call.rustType = _topBound(declaredReturn, static) ?? _type(static);
        } on Unsupported {
          // Untyped, as `expression` leaves it.
        }
      }
    }
    // A projected result: into the `Option<T>` the caller works with.
    final declared = node.interfaceTarget.function?.returnType;
    return _acrossBinding(
      call,
      declared,
      _bindingOf(declared, node.interfaceTarget, receiver, node.arguments),
      toOption: true,
    );
  }
}
