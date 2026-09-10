part of '../backend_rust.dart';

// Super calls, accessor qualifiers and handles.
augment class RustBackend {
  String _superCall(
    String base,
    String name,
    List<IrExpr> args, {
    bool isSetter = false,
    List<IrType> baseArguments = const [],
    List<IrType> typeArguments = const [],
  }) {
    // `Object` is not a class this compiler has, and it never will be -- it is
    // the root every Dart class already inherits from. So `super.toString()`
    // was refused as "not in this file", 198 times, when the truth is that
    // there is no file. Dart's own `Object.toString` returns
    // `Instance of 'Foo'`, so that is what it translates to; upstream prints
    // exactly this for a class that overrides nothing.
    //
    // Only `toString`. `super.hashCode` and `super.==` are identity on the
    // object, and identity is what a copied value class does not have --
    // routing them through `_identical` at ws828 refused them again, at the
    // argument (`IrCall (Object)`, not a reference). They stay refused, and
    // the census that says why is in STATUS: 239 classes have their
    // identity observed and only two reach `super` into `Object`.
    if (base == 'Object' && name == 'toString' && args.isEmpty) {
      return 'format!("Instance of \'{}\'", "${cls.name}")';
    }
    final baseClass = library[base];
    if (baseClass == null) {
      throw Unsupported(
        'super call into `$base`, which is not in this file',
        'super.$name(...)',
      );
    }
    // An operator counts: `_emitSuperFns` writes a free function for every
    // non-static member, an operator's under the name `superFn` gives it
    // (`inline_span_super_op_eq`), and whether that function came out is
    // `_superFnEmits`'s question below -- which is the one this used to
    // pre-empt by refusing every `super.==` outright (5 refusals: `TextSpan`,
    // `WidgetSpan`, `_BodyBoxConstraints`, `_FileSpan`, `ColorSwatch`, ws877).
    final provides = baseClass.methods.any(
      (m) => m.name == name && !m.isStatic && m.isSetter == isSetter,
    );
    if (!provides) {
      // The base's own version was refused, or is abstract and has no body to
      // call. Emitting the call anyway would name a function that was never
      // written -- the `_stringify` shape from round one, one level up.
      throw Unsupported(
        'super call to `$base.$name`, which was not translated',
        'super.$name(...)',
      );
    }
    if (!_superFnEmits(baseClass, name, isSetter: isSetter)) {
      // The base *has* the method, and the free function holding its body still
      // could not be emitted -- so the name this call would use is not written
      // anywhere. `Alignment.toString` called `alignment_geometry_super_to_-
      // string` for exactly this reason, and the Kernel side of the library did
      // not build for two rounds while `agree.py` was recorded as green.
      //
      // The question is answered by emitting the function and seeing, rather
      // than by a second rule about when it works: a second rule is a thing
      // that can disagree with the first one.
      throw Unsupported(
        'super call to `$base.$name`, whose body did not translate',
        'super.$name(...)',
      );
    }
    // The receiver as the super function takes it, `&__Self`: `self` is
    // already a reference -- one more deref when it is the handle
    // (`_receiverOf`) -- and a closure's `__me` is a handle when the class
    // is counted, a value otherwise (510 `Rc<X>: Trait` bounds at ws294).
    // The super function takes `&dyn Base` now (`_emitSuperFnBody`), and the
    // borrow comes from the base trait's own `dart_as_` -- method resolution
    // derefs whatever `this` is here (a `&Self`, an `Rc<Self>` handle, a
    // closure's `__me`) to find it.
    // The base's type arguments spelled: a class implementing the trait
    // at two instantiations (`Animation<f64>` and the wider `Animation<
    // Option<f64>>`) left `T` ambiguous (E0283, 3 at ws451). The method's
    // own stay inferred.
    final method = baseClass.methods.firstWhere(
      (m) => m.name == name && !m.isStatic && m.isSetter == isSetter,
    );
    final baseDyn = _superTakesDyn(baseClass, method);
    final receiver = baseDyn
        ? '$_selfName.dart_as_${snakeRaw(base)}()'
        : _selfName == 'self'
        ? (_selfIsHandle ? '&**self' : 'self')
        : _selfName == 'this_'
        ? 'this_'
        : cls.counted
        ? '&*$_selfName'
        : '&$_selfName';
    final own = typeArguments.length == method.typeParameters.length
        ? typeArguments.map(type).toList()
        : List.filled(method.typeParameters.length, '_');
    // No leading `_`: `__Self` is gone from the super function's generics.
    final spelledArgs = [if (!baseDyn) '_', ...baseArguments.map(type), ...own];
    final turbofish = baseArguments.isEmpty && own.every((a) => a == '_')
        ? ''
        : '::<${spelledArgs.join(', ')}>';
    RustBackend.namedElsewhere
      ..add(base)
      ..add(superFn(base, name, isSetter: isSetter));
    final call =
        '${superFn(base, name, isSetter: isSetter)}$turbofish(${[receiver, ...args.map(expr)].join(', ')})';
    // KNOWN GAP (found by the analyzer 2026-09-09, never measured): an async
    // super function is an `async fn`, and the caller's trait wants the boxed
    // future every `Future<T>` is here -- so this call should be awaited and
    // boxed. The flag that says so was computed here and never read, which is
    // why nothing has been awaiting it. Closing it changes what is emitted at
    // every async super call, so it is a round of its own.
    return call;
  }

  /// Whether `base`'s free function for [name] can actually be emitted.
  ///
  /// `_superFailed` answers this for the class being emitted, but a super call
  /// is made from the *subclass*, whose backend never sees the base's set.
  static final _superFnProbes = <String, bool>{};

  bool _superFnEmits(IrClass baseClass, String name, {bool isSetter = false}) {
    // Only an abstract class writes them. `_emitSuperFns` is called from
    // `_emitTrait` and nowhere else, because the free function is generic over
    // the trait -- there is nothing to make it generic over when the base is a
    // struct, since flattening copies the base's fields into each subclass
    // rather than leaving them anywhere shared. Probing without asking this
    // first said yes and the call named a function nobody wrote; the mixin
    // fixture is what walked into it.
    if (!baseClass.isAbstract) return false;
    final key = '${baseClass.name}.${isSetter ? 'set:' : ''}$name';
    final known = _superFnProbes[key];
    if (known != null) return known;
    final method = baseClass.methods.firstWhere(
      (m) => m.name == name && !m.isStatic && m.isSetter == isSetter,
    );
    final probe = RustBackend(baseClass, library: library);
    final ok = probe._member(key, () => probe._emitSuperFn(method));
    return _superFnProbes[key] = ok;
  }

  /// Whether a field of *this* class is reachable as a field right now.
  ///
  /// Inside a trait it is not. The class's fields were flattened into every
  /// implementor, so the trait -- and the free functions holding its method
  /// bodies -- can only reach them through an accessor the trait requires.
  /// Reading them as fields gives "no field `width` on type `&S`".
  var _fieldsAreAccessors = false;

  /// Whether the signature being written belongs to a trait.
  var _inTrait = false;

  /// `this` as an owned handle, from wherever the body is: a trait body's
  /// `dart_self_<trait>()`, a counted class's stored handle, else a clone.
  String _selfHandle() => _fieldsAreAccessors
      ? '$_selfName.dart_self_${snakeRaw(cls.name)}()'
      : cls.counted
      ? '$_selfName.dart_self_ref().get()'
      : '$_selfName.clone()';

  /// In a trait body, the trait an accessor is reached through when more
  /// than one trait in the chain declares it (`textTheme` on
  /// `CupertinoThemeData` over `NoDefaultCupertinoThemeData`; 13 E0034 at
  /// ws308): this trait when it declares the name, which is the override
  /// Dart would dispatch to, else the nearest abstract supertype that does.
  String? _accessorQualifier(String name, {String kind = 'read', IrClass? on}) {
    // ..on this class by default, or on the class a handle is typed by
    // (a trait object's field read, `_fieldRead`).
    final cls = on ?? this.cls;
    // What each kind of accessor a trait declares (`_emitTrait`): a read
    // for any field or getter, a write for a mutable field or a setter, a
    // cell for a held collection. Naming a trait that lacks the item was
    // "expected a type, found a trait" (22 at ws309).
    bool field(IrClass c) =>
        c.fields.any(
          (f) =>
              f.name == name &&
              switch (kind) {
                'write' => !f.isFinal,
                'cell' => _handsCell(f),
                _ => true,
              },
        ) ||
        // A cell an application holds for a hollow mixin is the mixin's
        // to hand out (`appliedFields`).
        (kind == 'cell' &&
            c.appliedFields.any((f) => f.name == name && _handsCell(f)));
    bool method(IrClass c) =>
        c.methods.any(
          (m) =>
              m.name == name && !m.isStatic && m.isSetter == (kind == 'write'),
        ) ||
        c.abstractMethods.any(
          (m) => m.name == name && m.isSetter == (kind == 'write'),
        );
    if (kind == 'cell' && !field(cls) && !_supertypesOf(cls).any(field)) {
      return null;
    }
    final chain = [
      cls,
      ..._supertypesOf(cls).where((t) => library.isAbstract(t.name)),
    ];
    final declaring = chain.where((c) => field(c) || method(c)).toList();
    if (declaring.length < 2) return null;
    // A getter override is the nearest trait's own; a field's accessor is
    // declared once, by the topmost trait holding the field (`_emitTrait`
    // leaves it to the ancestor), which no other declarer is above.
    if (method(cls)) return cls.name;
    final fields = declaring.where(field).toList();
    if (fields.isEmpty) return declaring.first.name;
    return fields
        .firstWhere(
          (c) => !fields.any((o) => o != c && _supertypesOf(c).contains(o)),
          orElse: () => fields.last,
        )
        .name;
  }

  /// A field an application holds for `of` or one of its abstract
  /// ancestors (`IrClass.appliedFields`), by name.
  IrFieldDecl? _appliedFieldOf(IrClass of, String name) {
    for (final c in [of, ..._abstractAncestors(of)]) {
      for (final f in c.appliedFields) {
        if (f.name == name) return f;
      }
    }
    return null;
  }

  /// The trait, `from` or one above it, that declares the Rust item
  /// `rustName` (a method, a setter, a field's accessor); null when none
  /// of them does.
  String? _declaringTrait(String from, String rustName) {
    // Another module's trait too (`ModalRoute` from `widgets`, whose
    // `addLocalHistoryEntry` is `LocalHistoryRoute`'s: `ModalRoute::add_
    // local_history_entry(..)` was E0782 in `material`, run672).
    final start = library[from] ?? library.elsewhere[from];
    if (start == null) return null;
    bool declares(IrClass c) =>
        c.methods.any((m) => !m.isStatic && _methodName(m) == rustName) ||
        c.abstractMethods.any((m) => _methodName(m) == rustName) ||
        c.fields.any(
          (f) =>
              snake(f.name) == rustName || 'set_${snake(f.name)}' == rustName,
        );
    for (final c in [start, ..._abstractAncestors(start)]) {
      if (declares(c)) return c.name;
    }
    return null;
  }

  /// `this` as the handle the object already has -- the trait's own in a
  /// trait body, the counted struct's otherwise -- or null when the class
  /// has none (a plain value struct).
  String? _thisHandle() {
    if (_fieldsAreAccessors || _selfName == 'this_') {
      return '$_selfName.dart_self_${snakeRaw(cls.name)}()';
    }
    if (cls.counted) return '$_selfName.dart_self_ref().get()';
    return null;
  }

  /// A value shared as a handle: `this` by its own handle (`self.clone()`
  /// was a struct where `Rc<dyn RendererBinding>` went, `_manifold`'s
  /// lazy initializer, run459), anything else as spelled.
  String _handleOf(IrExpr value) {
    // A clone of `this` (the front end's) is `this`.
    final bare =
        value is IrCall &&
            value.name == 'clone' &&
            value.args.isEmpty &&
            (value.target == null || value.target is IrThis)
        ? IrThis()
        : value;
    return bare is IrThis ? (_thisHandle() ?? expr(value)) : expr(value);
  }

  String _fieldRead(
    IrExpr? target,
    String name, [
    bool onEnum = false,
    String? owner,
  ]) {
    final receiver = _receiver(target);
    // A field of an *enum* is a getter here, not storage: the value is a
    // constant of the variant and lives in a `match`. Only the front end knows
    // -- the backend sees `state.value` with no idea what `state` is -- so it
    // says so on the node.
    if (onEnum) return '$receiver.${snake(name)}()';

    // Inside a trait every read on `this` is an accessor call: a trait has
    // no fields, and a mixin's `this_.source_url` names a getter of the
    // implementer's, declared in an interface the mixin never sees (7).
    if (_fieldsAreAccessors && (target == null || target is IrThis)) {
      // `super.x` of a base's *field* (the front end names the base on
      // the node): the base trait's accessor, which is the storage --
      // this trait's own may be a getter over it (`CupertinoThemeData.
      // primaryColor` over `NoDefaultCupertinoThemeData`'s field, run618).
      if (owner != null && owner != cls.name && library.isAbstract(owner)) {
        return '${_implementedAs(owner)}::${snake(name)}($receiver)$_propagate';
      }
      final through =
          _accessorQualifier(name) ?? _wideTraitFor(cls, name)?.name;
      return through == null
          ? '$receiver.${snake(name)}()$_propagate'
          : '${_implementedAs(through)}::${snake(name)}($receiver)$_propagate';
    }
    // A shared field is read through its cell. `get` copies, which is what a
    // Dart read does; `borrow().clone()` is the same for a value that is not
    // `Copy`.
    if (target == null || target is IrThis) {
      final shared = _sharedField(name);
      if (shared != null) {
        final lazy = _lazyDecl(name);
        if (lazy != null) return _lazyRead(lazy, receiver);
        // The guard bound and dropped in its own statement (as another
        // object's field is read below): a bare `.borrow().clone()` keeps
        // its `Ref` to the statement's end, into a `borrow_mut()` of the
        // same cell on the left (`_file = _file.setPosition(0)`, run517).
        // Parenthesised: a block at a statement's start is a statement.
        final read = _isCopy(_heldDecl(shared))
            ? '$receiver.${snake(name)}.get()'
            : '({ let __r = $receiver.${snake(name)}.borrow().clone(); __r })';
        // Out of the cell it is a value, so the `late` unwrap is on a value
        // too. This is the one shape that does need `T: Clone`.
        return shared.isLate ? '$read.unwrap()' : read;
      }
      final late = _lateField(name);
      if (late != null) {
        // `as_ref()` rather than a clone: a read of a field is a place in
        // Rust, and `&T` is what the sites around it already expect. Only a
        // `Copy` value is taken out whole, which is what a place does anyway.
        // Cloned out, as every other field read is now: `as_ref()` handed
        // back a `&_ImageFilter` where the getter returns one by value (4).
        return _isCopy(_declSpelling(() => type(late.type)))
            ? '$receiver.${snake(name)}.unwrap()'
            : '$receiver.${snake(name)}.clone().unwrap()';
      }
    }
    // Another object's field, when the front end named its class and that
    // class keeps the field in a cell: read through the cell, as the write
    // side does. Without this the read was `entry.x` against a `RefCell`.
    if (owner != null) {
      final cell = _cellFieldOf(owner, name);
      // Its own class's accessor runs the initialiser on the right
      // object (`_emitLazyAccessors`); `_lazyRead` here printed it
      // against *this* class's `self`.
      if (cell != null && _lazyFieldOf(owner, name)) {
        return '$receiver.${_lazyAccessor(name)}()$_propagate';
      }
      if (cell != null) {
        // The `borrow()` guard is a temporary, and a temporary in a block's
        // tail expression outlives the block's locals: `Ok(data.next_sibling
        // .borrow().clone())` on a local `data` was "does not live long
        // enough" 17 times (ws376). Bound and handed out, the guard dies
        // in its own statement.
        final read = _fieldIsCopy(cell, owner == null ? null : library[owner])
            ? '$receiver.${snake(name)}.get()'
            : '{ let __r = $receiver.${snake(name)}.borrow().clone(); __r }';
        return cell.isLate ? '$read.unwrap()' : read;
      }
      // Another object's `late` field: `other._argb` in `Hct.==` is an
      // `Option<i64>` on that side too, and reads unwrap it as `this`'s do.
      final owned = library[owner];
      if (owned != null) {
        for (final f in _allFields(owned)) {
          if (f.name != name || !f.isLate) continue;
          return _isCopy(_declSpelling(() => type(f.type)))
              ? '$receiver.${snake(name)}.unwrap()'
              : '$receiver.${snake(name)}.clone().unwrap()';
        }
      }
    }
    // A read of one of this class's own fields is a *value*, and behind
    // `&self` a value that is not `Copy` has to be cloned out: `self._value`
    // moved out of a shared reference, 134 times in the leaf crates. A
    // method call on the clone or a borrow of it costs a clone and nothing
    // else.
    if (target == null || target is IrThis) {
      for (final f in _allFields(cls)) {
        if (f.name == name) {
          return _isCopy(_declSpelling(() => type(f.type)))
              ? '$receiver.${snake(name)}'
              : '$receiver.${snake(name)}.clone()';
        }
      }
    }
    // A field of a local: cloned out, as a field of `self` is -- `r._m3storage`
    // moved out of `r` and `r.clone()` two lines later was a partial move (9).
    // As a *receiver* the field is a place; `_receiver` spells that.
    // Another object of *this* class (`other as Hct`): its `late` field is
    // the same `Option`, unwrapped the same way.
    if (target is IrDowncast && target.type == cls.name) {
      final late = _lateField(name);
      if (late != null) {
        return _isCopy(_declSpelling(() => type(late.type)))
            ? '$receiver.${snake(name)}.unwrap()'
            : '$receiver.${snake(name)}.clone().unwrap()';
      }
    }
    // A field of a *trait object*: the accessor the trait declares, since
    // a `dyn` has no fields (`childParentData.nextSibling` on an `Rc<dyn
    // StackParentData>`, the mixin's field, ws523).
    final held = target?.rustType;
    if (held != null && !isNullable(held) && library.isAbstract(held.name)) {
      final owned = library[held.name];
      if (owned != null && _allFields(owned).any((f) => f.name == name)) {
        // Qualified when two of the handle's traits declare it
        // (`next_sibling` on `ContainerBoxParentData` and on the mixin,
        // E0034 at ws526), through the trait object the handle holds.
        final through = _accessorQualifier(name, on: owned);
        final declaring = through == null ? null : library[through];
        if (declaring == null) return '$receiver.${snake(name)}()$_propagate';
        final passed = _argumentsThrough(owned, const {}, declaring, {});
        final traitArgs = passed == null || passed.isEmpty
            ? ''
            : '<${passed.map(type).join(', ')}>';
        final heldArgs = held.arguments.isEmpty
            ? ''
            : '<${held.arguments.map(type).join(', ')}>';
        return '<dyn ${held.name}$heldArgs as $through$traitArgs>::${snake(name)}(&*$receiver)$_propagate';
      }
    }
    // Any other object's field: cloned out, as a field of `self` or of a
    // local is (`..get().child` handed to `updateChild` moved out of the
    // handle, E0507, run459).
    return '$receiver.${snake(name)}.clone()';
  }

  /// `x is Foo`.
  ///
  /// Rust answers it with `Any`, which downcasts to a *concrete* type: the
  /// trait object says what it holds, and holding is always a struct. So a
  /// target that is itself abstract has no answer here -- `x is RenderBox`
  /// asks whether the thing implements a trait, which `Any` cannot say -- and
  /// is still refused, now under a name that says which half is missing.
  /// See `IrSuperDispatch`. The super function's generics are `<__Self,
  /// class parameters.., method parameters..>`: the first two kinds are
  /// inferred from the receiver, the method's own are spelled.
  String _superDispatch(
    IrExpr receiver,
    String base,
    String name,
    List<IrExpr> args,
    List<IrType> typeArguments,
    int classArity,
    String? castTo,
  ) {
    // A generic trait cast to with its arguments inferred (`dyn
    // CanonicalizedMap<_, _, _>`; E0107 on the bare name, run459).
    final castArity = library[castTo ?? '']?.typeParameters.length ?? 0;
    final castSpelled = castArity == 0
        ? castTo
        : '$castTo<${List.filled(castArity, '_').join(', ')}>';
    final on = castTo == null
        ? expr(receiver)
        : '${expr(receiver)}.dart_cast_to::<dyn $castSpelled>().unwrap()';
    final generics = [
      for (var i = 0; i < classArity; i++) '_',
      ...typeArguments.map(type),
    ];
    // An async base method's super function is its future, not a
    // `Result` (`invokeMethod` reaching `_invokeMethod<T>`, ws482).
    final baseMethod = library[base]?.methods
        .where((m) => m.name == name && !m.isStatic)
        .firstOrNull;
    final suffix = (baseMethod?.isAsync ?? false) ? '' : _propagate;
    RustBackend.namedElsewhere
      ..add(base)
      ..add(superFn(base, name));
    final spelled = generics.isEmpty ? '' : '::<${generics.join(', ')}>';
    return '${superFn(base, name)}$spelled'
        '(${['$on.dart_as_${snakeRaw(base)}()', ...args.map(expr)].join(', ')})$suffix';
  }
}
