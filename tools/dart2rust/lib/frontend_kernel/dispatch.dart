part of '../frontend_kernel.dart';

// Where a call lands, how it is qualified, and construction.
augment class KernelFrontend {
  /// See `IrCall.qualifier`: a member whose name two classes in the
  /// receiver's hierarchy declare is called through one of them by name.
  /// The member a call on `receiver` lands on in Rust: an inherent method
  /// of the receiver's class (a mixin clone with `RenderBox` written in
  /// it) when the class is a struct or an open class, the interface
  /// member (the trait's, erased) otherwise. `getDispatchTarget` answers
  /// for both: on an abstract class it finds the hollow mixin's own.
  Member _landing(Member interface, Expression receiver) {
    final hierarchy = typeEnvironment?.hierarchy;
    // ..through `_appliedBack`, as the read does: inside a mixin's body
    // borrowed from an application the copy says `FlexParentData` where
    // the trait holds the erased bound, and the qualifier is that bound's
    // trait, not the application's (ws736).
    final type = receiver is ThisExpression
        ? null
        : _backHere(_staticType(receiver));
    final on = receiver is ThisExpression
        ? (_lowering ?? _member?.enclosingClass)
        : type is InterfaceType
        ? type.classNode
        : null;
    if (hierarchy == null || on == null) return interface;
    final found = hierarchy.getDispatchTarget(
      on,
      interface.name,
      setter: interface is Procedure && interface.isSetter,
    );
    return found ?? interface;
  }

  /// The Rust type of `landing`'s value as reached through `receiver`:
  /// its declared type with the receiver's type arguments substituted for
  /// the parameters that are *kept*, the erased ones left to `_type`,
  /// which spells them as their bound. Dart's own static type substitutes
  /// every one, which is where the clone's `RenderBox` and the trait's
  /// `RenderObject` part ways. Null when the type names the method's own
  /// parameters (Dart's instantiated type is the better answer there) or
  /// has no spelling here.
  IrType? _memberRustType(
    Member reached,
    Expression receiver, {
    required bool asGetter,
  }) {
    // A copy in an anonymous application is typed by the mixin's own
    // declaration, as the copy itself is lowered (the kept parameters
    // substituted below, the erased ones their bounds).
    final landing = _originalOf(reached);
    var declared = asGetter || landing is! Procedure
        ? landing.getterType
        : landing.function.returnType;
    if (landing is Procedure &&
        !asGetter &&
        landing.function.typeParameters.isNotEmpty &&
        _mentionsParametersOf(declared, landing.function.typeParameters)) {
      return null;
    }
    final owner = landing.enclosingClass;
    final env = typeEnvironment;
    final receiverType = receiver is ThisExpression
        ? (env == null
              ? null
              : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable))
        : _staticType(receiver);
    if (owner != null &&
        owner.typeParameters.isNotEmpty &&
        env != null &&
        receiverType is InterfaceType) {
      final asOwner = env.hierarchy.getTypeAsInstanceOf(receiverType, owner);
      if (asOwner is InterfaceType) {
        final kept = <TypeParameter, DartType>{};
        for (
          var i = 0;
          i < owner.typeParameters.length && i < asOwner.typeArguments.length;
          i++
        ) {
          final p = owner.typeParameters[i];
          if (!_erasedParameter(p)) kept[p] = asOwner.typeArguments[i];
        }
        try {
          return _typeKept(declared, kept);
        } on Unsupported {
          return null;
        }
      }
    }
    try {
      return _type(declared);
    } on Unsupported {
      return null;
    }
  }

  static bool _mentionsParametersOf(DartType t, List<TypeParameter> ps) {
    // `FutureOr<R>` is its own node, not an `InterfaceType`: `then<R>`'s
    // `FutureOr<R> Function(void)` slot passed for a slot of no parameter,
    // and the callback was lowered against the declared `R`, its body
    // never closed (`Route.didAdd`, ws512).
    if (t is FutureOrType) return _mentionsParametersOf(t.typeArgument, ps);
    if (t is RecordType) {
      return t.positional.any((a) => _mentionsParametersOf(a, ps)) ||
          t.named.any((n) => _mentionsParametersOf(n.type, ps));
    }
    if (t is TypeParameterType) return ps.contains(t.parameter);
    if (t is InterfaceType) {
      return t.typeArguments.any((a) => _mentionsParametersOf(a, ps));
    }
    if (t is FunctionType) {
      return _mentionsParametersOf(t.returnType, ps) ||
          t.positionalParameters.any((a) => _mentionsParametersOf(a, ps)) ||
          t.namedParameters.any((n) => _mentionsParametersOf(n.type, ps));
    }
    return false;
  }

  IrCall _qualified(IrCall call, Member member, Expression receiver) {
    final out = _qualifiedRaw(call, member, receiver);
    // Typed by the member the call reaches in Rust: through a trait's
    // path (`RestorationMixin::restoration_id(self)`) it is the trait's
    // declaration, whatever the class's override narrowed it to (`String?`
    // there, `String` here: 72 dropped `!`s at ws390); a plain call lands
    // on the class's own.
    out.rustType ??= _memberRustType(
      out.qualifier != null ? member : _landing(member, receiver),
      receiver,
      asGetter: member is Field || (member is Procedure && member.isGetter),
    );
    // An `async` member declared `Future<T>?` still hands back the future
    // it spawns, never null: its wrapper is typed `DartFuture<T>`, and so
    // is a call to it (`sendWithPostfix` in `send`, ws474).
    final t = out.rustType;
    if (out.asyncTarget && t != null && t.name == 'Future' && t.nullable) {
      out.rustType = IrType('Future', arguments: t.arguments);
    }
    // ..and one declared `FutureOr<T>` hands back a `Future<T>` too
    // (Dart's `flatten`; see `_spawnedFuture`).
    if (out.asyncTarget && t != null && t.name == 'FutureOr') {
      out.rustType = IrType('Future', arguments: t.arguments);
    }
    return out;
  }

  IrCall _qualifiedRaw(IrCall call, Member member, Expression receiver) {
    final owner = member.enclosingClass;
    if (owner == null || !_translatedClass(owner)) {
      return _fails(member)
          ? IrCall(
              call.target,
              call.name,
              call.args,
              fails: true,
              diverges: _diverges(member),
            )
          : call;
    }
    // From the receiver's own class: a mixin's `child` is declared again
    // by the trait of the class that mixes it in, *below* the owner.
    // ..through `_appliedBack`, as the read does: inside a mixin's body
    // borrowed from an application the copy says `FlexParentData` where
    // the trait holds the erased bound, and the qualifier is that bound's
    // trait, not the application's (ws736).
    final type = receiver is ThisExpression
        ? null
        : _backHere(_staticType(receiver));
    // Inside a body borrowed from a mixin application (`_appliedBody`)
    // `this` is the mixin's trait, not the anonymous class the CFE copied
    // the body into (`ServicesBinding::x(this_)` named the trait as a
    // type, 9 E0782 at ws436).
    final enclosing = _member?.enclosingClass;
    // A receiver typed by a type parameter (`ChildType child` in a
    // mixin's copy, read by its declaration) is its bound's class, which
    // is what it is here: with no class at all the walk started at the
    // member's owner and `child.toDiagnosticsNode()` on a `dyn
    // RenderObject` was left for three traits to claim (4 E0034, ws536).
    final from = receiver is ThisExpression
        ? ((enclosing?.isAnonymousMixin ?? false) ? _lowering : enclosing)
        : _classOfType(type);
    var qualifier = _qualifierFor(from ?? owner, member);
    // ..and from the class the receiver *is here* when the Dart type said
    // nothing: a closure parameter retyped to an erased bound reads back
    // through a cast, and the wider type above it declares no member for
    // the walk to count -- `notification.depth` on a `ScrollNotification`
    // read out of a `Notification` slot was left for two traits to claim
    // (`_PageViewState.build`, ws719).
    if (qualifier == null &&
        receiver is VariableGet &&
        _retyped.containsKey(receiver.variable)) {
      final declaredClass = _classOfType(receiver.variable.type);
      if (declaredClass != null && !identical(declaredClass, from)) {
        qualifier = _qualifierFor(declaredClass, member);
      }
    }
    // `this.x` where a trait declared `x` and this class overrides it: Rust
    // resolves `self.x()` to the inherent override, whose type may be
    // narrower than the declaration the kernel typed the read by (`String?
    // get restorationId` overridden as `String`: 72 `unwrap` on a `String`
    // at ws296). Through the trait, whose signature the kernel agrees with.
    // The *lowering* class, not the member's: a mixin's body is lowered
    // into the class applying it, and there `this` is that class.
    final host = _lowering ?? from;
    if (qualifier == null &&
        receiver is ThisExpression &&
        host != null &&
        host != owner &&
        _abstractLike(owner) &&
        !owner.isAnonymousMixin &&
        host.members.any(
          (m) =>
              m.name.text == member.name.text &&
              ((m is Procedure && !m.isStatic) || (m is Field && !m.isStatic)),
        )) {
      qualifier = owner.name;
    }
    final fails = _fails(member);
    final renamed = member is Procedure && member.name.text == 'clone';
    // `DART2RUST_TRACE_CALL=<name>`: the async-rule inputs of every call
    // to that member, to stderr.
    if (Platform.environment['DART2RUST_TRACE_CALL'] == member.name.text) {
      stderr.writeln(
        'TRACE_CALL ${member.name.text}: from=${from?.name} owner=${owner.name} '
        'enclosing=${_member?.enclosingClass?.name} lowering=${_lowering?.name} '
        'fails=$fails async=${_asyncMember(member)} qualifier=$qualifier '
        'applies=${from != null && _appliesMixin(from, owner)} '
        'abstract=${from != null && _abstractLike(from)} open=${from != null && _isOpen(from)}',
      );
    }
    if (qualifier == null && !fails && !renamed && !_asyncMember(member)) {
      return call;
    }
    return IrCall(
      call.target,
      renamed ? _dartName(call.name) : call.name,
      call.args,
      qualifier: qualifier,
      receiverClass: _classOfType(type)?.name,
      fails: fails,
      diverges: _diverges(member),
      // A struct's *own* async method, called plainly, is an `async fn`
      // reached as one; an inherited or a trait's goes through the trait
      // impl, which hands the future back inside the `Result`.
      // ..or a mixin's method the receiver's class applies, which is
      // inlined into that class as its own (`handlePopRoute()` inside
      // `WidgetsBinding.initInstances`, run445).
      asyncFn: _inherentAsync(
        member,
        from,
        qualifier,
        onThis: call.target == null,
      ),
      asyncTarget: _asyncMember(member),
      typeArguments: call.typeArguments,
    );
  }

  /// Whether a call to `member` from a receiver of class `from` reaches an
  /// `async fn` as one (`IrCall.asyncFn`): the member is async, and the
  /// receiver's own struct carries it inherently -- its own method, or a
  /// mixin's it applies -- with no trait on the path.
  ///
  /// On `this` (`onThis`) the receiver's own class is the struct or the
  /// trait body being emitted, and the backend knows which: an open or
  /// abstract class's own async method called on `this` counts as
  /// inherent here, and the trait bodies unwrap it (`_handleAsMethodCall`
  /// in `MethodChannel.setMethodCallHandler`'s super fn, run458).
  bool _inherentAsync(
    Member member,
    Class? from,
    String? qualifier, {
    bool onThis = false,
  }) {
    final owner = member.enclosingClass;
    if (Platform.environment['DART2RUST_TRACE_CALL'] == member.name.text) {
      stderr.writeln(
        'TRACE_ASYNC ${member.name.text}: from=${from?.name} owner=${owner?.name} '
        'fails=${_fails(member)} async=${_asyncMember(member)} qualifier=$qualifier '
        'applies=${from != null && owner != null && _appliesMixin(from, owner)} '
        'abstract=${from != null && _abstractLike(from)} open=${from != null && _isOpen(from)}',
      );
    }
    if (owner == null || from == null) return false;
    return (qualifier == null || qualifier == from.name) &&
        _asyncMember(member) &&
        (from == owner || _appliesMixin(from, owner)) &&
        (onThis || (!_abstractLike(from) && !_isOpen(from)));
  }

  /// Whether `from` applies `mixin` somewhere in its anonymous superclass
  /// chain, so that the mixin's methods are inlined into `from`'s struct.
  static bool _appliesMixin(Class from, Class mixin) {
    var t = from.supertype;
    while (t != null && t.classNode.isAnonymousMixin) {
      // The mixin itself, or the application class holding its copy (an
      // interface target inside an applied body names that one).
      if (t.classNode == mixin ||
          t.classNode.implementedTypes.any((i) => i.classNode == mixin)) {
        return true;
      }
      t = t.classNode.supertype;
    }
    return false;
  }

  /// A member declared `async`: emitted as an `async fn` where it is a
  /// free function, a static, or a struct's own method.
  /// By the marker the programmer wrote (`dartAsyncMarker`), which a hollow
  /// mixin declaration keeps where its `asyncMarker` says `Sync` for want
  /// of a body (`ServicesBinding.handleRequestAppExit`, run446).
  bool _asyncMember(Member m) =>
      m is Procedure && m.function.dartAsyncMarker == AsyncMarker.Async;

  /// A member declared to return `Never`.
  static bool _diverges(Member m) =>
      m is Procedure && m.function.returnType is NeverType;

  /// The trait to call `member` through from a value of class `from`, or
  /// null when only one class in the hierarchy declares it and the plain
  /// call is unambiguous.
  ///
  /// A member the CFE cloned into an anonymous mixin application
  /// (`_MixinApplication8&RenderBox&RenderObjectWithChildMixin.child`) is
  /// declared twice on the Rust side: by the mixin's trait and, flattened
  /// (ws112), by the trait of the class that applies it. That class is
  /// the name to call through -- the mixin's trait is not a supertrait of
  /// its, so inside a super function `__Self: ListNotifier` cannot reach
  /// `ListNotifierMixin::_updaters` (295 E0277s the round the mixin was
  /// named instead).
  /// The qualifier a setter call takes (see `IrSetter.qualifier`).
  String? _setterQualifier(Expression? receiver, Member target) {
    final owner = target.enclosingClass;
    if (owner == null || !_translatedClass(owner)) return null;
    final Class? from;
    if (receiver == null || receiver is ThisExpression) {
      from = _lowering ?? _member?.enclosingClass;
    } else {
      // ..through `_appliedBack`, as the read's qualifier is (ws737).
      from = _classOfType(_backHere(_staticType(receiver)));
    }
    if (from == null) return null;
    return _qualifierFor(from, target);
  }

  String? _classNameOf(Expression receiver) =>
      _classOfType(_backHere(_staticType(receiver)))?.name;

  /// The class a value of `t` is: an interface's own, a type parameter's
  /// bound's (through a bound that is itself a parameter).
  Class? _classOfType(DartType? t) {
    var seen = 0;
    while (t is TypeParameterType && seen++ < 8) {
      t = t.parameter.bound;
    }
    return t is InterfaceType ? t.classNode : null;
  }

  String? _qualifierFor(Class from, Member member) {
    final owner = member.enclosingClass!;
    final name = member.name.text;
    final setter = member is Procedure && member.isSetter;
    final seen = <Class>{};
    var found = 0;
    String? applier;
    Member? declared(Class c) {
      for (final m in c.members) {
        if (m.name.text == name &&
            (m is Field || (m is Procedure && m.isSetter == setter))) {
          return m;
        }
      }
      return null;
    }

    // `named`: the nearest class with a name of its own on the superclass
    // path down to `c`, which is where an anonymous application's members
    // were flattened to.
    void walk(Class c, Class named) {
      if (!seen.add(c)) return;
      final m = _translatedClass(c) ? declared(c) : null;
      if (m != null) {
        if (c.isAnonymousMixin) {
          // A cloned *abstract* member (`ScrollMetrics.axisDirection` in
          // `ScrollPosition with ScrollMetrics`) is flattened nowhere: its
          // one declaration is the mixin trait's.
          if (m.isAbstract) {
            found += 1;
          } else {
            found += 2;
            applier ??= named.name;
          }
        } else {
          found += 1;
        }
      }
      final below = c.isAnonymousMixin ? named : c;
      final superclass = c.superclass;
      if (superclass != null) walk(superclass, below);
      for (final s in c.supers) {
        if (s.classNode != superclass) walk(s.classNode, s.classNode);
      }
    }

    // `this` inside a member cloned into an application is the named class
    // the application is lowered into.
    walk(from, from.isAnonymousMixin ? (_lowering ?? from) : from);
    if (found < 2) return null;
    // Through the applying class whenever an application declares it --
    // also when the resolved owner is the mixin itself (`this._notifyUpdate`
    // inside `ListNotifier with ListNotifierMixin`): the mixin's trait is
    // not among a subclass trait's supertraits, the applier's is. An
    // abstract member of an anonymous owner is the mixin's, named by the
    // application's last segment.
    final chosen =
        applier ??
        (owner.isAnonymousMixin ? owner.name.split('&').last : owner.name);
    // Never a synthetic name: 509 "expected value, found trait" the round
    // one got through.
    return chosen.contains('&') ? null : chosen;
  }

  bool _translatedClass(Class c) {
    // The CFE's deduplicated mixin applications (`_MixinApplication8&
    // RenderBox&RenderObjectWithChildMixin`) live in a synthetic library
    // whose scheme is not a package's; they are the mixin's members
    // under another name, translated like it.
    if (c.isAnonymousMixin) return true;
    final uri = c.enclosingLibrary.importUri;
    return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
  }

  /// Whether a future-like class's `then` calls the callback on its own
  /// stack -- `SynchronousFuture.then` runs `onValue(_value)` right there,
  /// which is the entire point of the class, and Flutter asserts on it
  /// (`_RootRestorationScopeState._replaceRootBucket`: "Ensure that load
  /// finished synchronously"). A class that hands the callback to another
  /// future instead -- `package:async`'s `DelegatingFuture` -- does not,
  /// and keeps the prelude's ordinary `future_ready`.
  ///
  /// Asked of the body rather than of the name: the callback parameter
  /// being *called* inside `then` is the property, and it is the one the
  /// prelude's `synchronous` flag stands for.
  bool _callsBackHere(Class c) {
    final then = c.procedures
        .where((p) => p.name.text == 'then' && !p.isStatic)
        .firstOrNull;
    final body = then?.function.body;
    final callback = then?.function.positionalParameters.firstOrNull;
    if (body == null || callback == null) return false;
    final finder = _CallsParameter(callback);
    body.accept(finder);
    return finder.found;
  }

  /// Whether `c` implements `dart:async`'s `Future` directly: such a
  /// class is the prelude's future here (`_type`), and constructing it
  /// with its value is a future already done (`future_ready`).
  bool _futureLike(Class c) =>
      c.typeParameters.length == 1 &&
      c.enclosingLibrary.importUri.scheme != 'dart' &&
      c.implementedTypes.any(
        (t) =>
            t.classNode.name == 'Future' &&
            t.classNode.enclosingLibrary.importUri.toString() == 'dart:async',
      );

  IrExpr _construct(ConstructorInvocation node) {
    final target = node.target;
    final name = target.name.text;
    if (_futureLike(target.enclosingClass) &&
        node.arguments.positional.length == 1 &&
        node.arguments.named.isEmpty) {
      final held = node.arguments.types.isNotEmpty
          ? _type(node.arguments.types.single)
          : null;
      final value = expression(node.arguments.positional.single);
      final ready = IrStaticCall(
        null,
        _callsBackHere(target.enclosingClass)
            ? 'future_synchronous'
            : 'future_ready',
        [
          held == null
              ? value
              : _widened(
                  node.arguments.positional.single,
                  node.arguments.types.single,
                  value,
                ),
        ],
      );
      if (held != null) ready.rustType = IrType('Future', arguments: [held]);
      return ready;
    }
    // `ListQueue([capacity])`: the prelude's `Queue` (a `VecDeque`), and
    // the capacity hint is dropped.
    if (const {
          'ListQueue',
          'Queue',
          'DoubleLinkedQueue',
        }.contains(target.enclosingClass.name) &&
        name.isEmpty) {
      return IrNew(const IrType('Queue'), const []);
    }
    // `HashMap(equals: .., hashCode: .., isValidKey: ..)` and `LinkedHashMap`
    // likewise: the prelude's one `Map`, and the custom key equality is
    // dropped -- recorded as the approximation it is (collection's
    // `MapEquality` builds such a map to count entries).
    if (const {
          'HashMap',
          'LinkedHashMap',
        }.contains(target.enclosingClass.name) &&
        name.isEmpty) {
      return IrNew(const IrType('Map'), const []);
    }
    // `Object()`: an identity and nothing else, the prelude's `new_object`.
    if (target.enclosingClass.name == 'Object' &&
        node.arguments.positional.isEmpty &&
        node.arguments.named.isEmpty) {
      return IrStaticCall(null, 'new_object', const []);
    }
    // The constructor's parameters in the constructed type's terms:
    // `Tween<double>(begin: 0)` takes a `T?`, which is a `double?` here.
    final cls = target.enclosingClass;
    // The type arguments come along, as a turbofish where the class is
    // generic: `_FooState<T>()` in `createState` says which `T` (27).
    final created = IrNew(
      IrType(
        _instanceName(cls),
        arguments: _censusOf(
          node.constructedType,
          _erasedArguments(cls, node.constructedType.typeArguments),
        ),
        module: _moduleQualifier(cls),
      ),
      _constructing(
        target.function,
        {
          for (
            var i = 0;
            i < cls.typeParameters.length && i < node.arguments.types.length;
            i++
          )
            cls.typeParameters[i]: node.arguments.types[i],
        },
        () => _arguments(
          node.arguments,
          target.function,
          false,
          _instantiatedConstructor(node),
        ),
      ),
      constructor: name.isEmpty ? null : name,
      // The private class behind a collapsed public name, kept for the
      // backend to tell two implementations' constructors apart
      // (`IrNew.implementation`).
      implementation: _instanceName(cls) != cls.name ? cls.name : null,
    );
    // An open class's instance is its `Impl` struct, and every slot typed
    // with the class is the trait handle: the construction leaves as one
    // (570 `SizeImpl` where an `Rc<dyn Size>` was wanted).
    return _isOpen(cls)
        ? IrUpcast(created, _type(node.constructedType))
        : created;
  }

  /// The struct an instance of `cls` is: the class's own name, or the
  /// `Impl` beside an open class's trait.
  String _instanceName(Class cls) {
    // A private implementation class of a `dart:` library is constructed
    // as the public type it implements, which is what the prelude names
    // (`WeakReference(..)` devirtualised to `_WeakReference`, the
    // navigator's `_RouteEntry`, ws505).
    final uri = cls.enclosingLibrary.importUri;
    if (cls.name.startsWith('_') &&
        uri.scheme == 'dart' &&
        uri.toString() != 'dart:ui') {
      for (final above in [
        if (cls.supertype != null) cls.supertype!,
        ...cls.implementedTypes,
      ]) {
        final c = above.classNode;
        if (!c.name.startsWith('_') && c.name != 'Object') {
          return _instanceName(c);
        }
      }
    }
    return _isOpen(cls) ? implName(cls.name) : cls.name;
  }

  static FunctionType? _instantiatedConstructor(ConstructorInvocation node) {
    final cls = node.target.enclosingClass;
    if (cls.typeParameters.isEmpty) return null;
    final declared = node.target.function.computeThisFunctionType(
      Nullability.nonNullable,
    );
    // A constructor's function type carries the class's type parameters as
    // its own (structural ones, `E%`), which a substitution by the
    // constructed type's arguments does not reach: instantiated instead
    // (`HeapPriorityQueue<_TaskEntry<dynamic>>(_taskSorter)` in the
    // binding's constructor took a `Comparator<E%>`, run433).
    if (declared.typeParameters.isNotEmpty) {
      if (declared.typeParameters.length !=
          node.constructedType.typeArguments.length) {
        return null;
      }
      final instantiated = FunctionTypeInstantiator.instantiate(
        declared,
        node.constructedType.typeArguments,
      );
      return instantiated is FunctionType ? instantiated : null;
    }
    final substituted = Substitution.fromInterfaceType(node.constructedType)
        .substituteType(declared);
    return substituted is FunctionType ? substituted : null;
  }

  /// A generic function's type at this call: `_futurize<int>(callbacker)`
  /// takes a `String? Function(_Callback<int>)`, and the closure passed is
  /// typed against that, not against the `T` the declaration wrote.
  static FunctionType? _instantiated(
    StaticInvocation node, [
    FunctionNode? declaration,
  ]) {
    final fn = declaration ?? node.target.function;
    if (fn.typeParameters.isEmpty ||
        node.arguments.types.length != fn.typeParameters.length) {
      return null;
    }
    final declared = fn.computeFunctionType(Nullability.nonNullable);
    final instantiated = FunctionTypeInstantiator.instantiate(
      declared,
      node.arguments.types,
    );
    return instantiated is FunctionType ? instantiated : null;
  }
}
