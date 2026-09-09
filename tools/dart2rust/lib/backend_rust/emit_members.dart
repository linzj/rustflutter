part of '../backend_rust.dart';

// Constructors, statics, methods and operators.
augment class RustBackend {
  void _emitConstructor(IrConstructor ctor) {
    _here = '${cls.name}.${ctor.name?.isEmpty ?? true ? 'new' : ctor.name}';
    // Dart's named constructors are Rust's associated functions already --
    // `EdgeInsets.all(8)` and `EdgeInsets::all(8.0)` are the same call, and the
    // unnamed one is `new` by Rust's convention. Nothing has to be encoded, so
    // nothing is: this is one of the places the two languages simply agree.
    final name = _ctorName(ctor.name);
    _doc(ctor.doc);
    // A parameter the constructor assigns -- `cullRect ??= Rect.largest`
    // inside a field initialiser, or in the body -- is `mut` (E0384).
    final assigned = <String>{
      for (final init in ctor.fieldInits.values)
        ..._assignedIn(IrExprStmt(init)),
      if (ctor.body != null) ..._assignedIn(ctor.body!),
    };
    final params = ctor.params
        .map(
          (p) =>
              '${assigned.contains(p.name) ? 'mut ' : ''}${snake(p.name)}: ${type(p.type)}',
        )
        .join(', ');
    // ..and the body's locals are `mut` by the same reckoning (`let` in a
    // constructor body was never `mut` once locals stopped being so by
    // default: 4 E0384s in `ParagraphStyle`).
    _reassigned = assigned;
    _cellLocals = {};
    // `const fn` because the Dart constructor was `const`, which is what lets
    // the static constants below be associated consts rather than lazy statics.
    // `const fn` even when the constructor carries asserts. An earlier round
    // dropped `const` here, on the assumption that Rust would not accept a
    // `const fn` that could panic. That assumption was wrong -- const panic has
    // been stable since 1.57, `debug_assert!` inside a `const fn` compiles, and
    // the check still fires at runtime. Both were available all along.
    //
    // It mattered: `TextAlignVertical` has asserts in its constructor and
    // `static const` fields built from it, and dropping `const` made those
    // fields uncompilable. The two rounds' rules only met on real code.
    // A constructor with a body cannot be `const`: it builds the value into a
    // local and runs statements against it, and a `const fn` may not.
    // ..and one whose parameters are not all `Copy`: a `String` field is
    // initialised with `string.clone()` now, and a `const fn` may not call
    // it (E0015, 53 of them the round the clones arrived). The `static
    // const`s that needed `const fn` hold `Copy` values -- `Offset`,
    // `TextAlignVertical` -- and keep it.
    // ..nor one whose field initialisers clone -- `Color`, a `Copy` struct
    // the front end could not know is one, arrives as `color.clone()`.
    // ..and a redirecting one (`const BorderRadius.all(r) : this.only(..)`)
    // only when the constructor it hands its arguments to is one: a
    // `const fn` may not call a plain `fn` (run686).
    final constness = _constCtor(ctor, {}) ? 'const ' : '';
    // A counted class hands out a handle, not a value: everything that
    // holds one holds an `Rc`, so the constructor is where the first one is
    // made. A `const fn` cannot allocate, so a counted constructor is not one.
    final produces = cls.counted ? 'std::rc::Rc<Self>' : 'Self';
    final signatureAt = _out.length;
    _line(
      '${_vis(ctor.name ?? cls.name)}'
      '${cls.counted ? '' : constness}fn $name($params) -> ${_wrapped(produces)} {',
    );
    _indent++;
    // A value class registers its cast function as it is first made
    // (`dart_register`); a counted one does so in `dart_rc`.
    if (!cls.counted && constness.isEmpty) _line('dart_register::<Self>();');
    // A constructor fails like any function: its body's value is `Ok`.
    _failure = _resultModel ? _error : null;
    if (ctor.redirectTo == null) _line('Ok({');
    // This constructor's own temporaries first -- a `super(#t0)` passes them
    // -- and only then the base's, computed from them.
    for (final s in [...ctor.pre, ..._inheritedPre(ctor)]) {
      stmt(s);
    }
    final redirect = ctor.redirectTo;
    if (redirect != null) {
      // Everything this constructor does is hand its arguments to another one
      // of the same class. `Self::` because it is the same class; `_ctorName`
      // because the unnamed one is `new` here as it is above.
      final args = ctor.redirectArgs.map(expr).join(', ');
      _line('Self::${_ctorName(redirect.isEmpty ? null : redirect)}($args)');
      _indent--;
      _line('}');
      // The same clone check as a constructor with a body gets below: a
      // `Copy` argument the front end could not know is one arrives as
      // `radius.clone()` (`BorderRadius.all`, run686).
      if (constness.isNotEmpty &&
          _out.sublist(signatureAt + 1).any((l) => l.contains('.clone()'))) {
        _out[signatureAt] = _out[signatureAt].replaceFirst('const fn ', 'fn ');
      }
      _line('');
      return;
    }
    for (final check in ctor.asserts) {
      stmt(check);
    }
    final inits = {..._inheritedInits(ctor), ...ctor.fieldInits};
    // The handle is made around the value: a counted class's constructor is
    // the one place an `Rc` comes from, and everything that holds one after
    // that holds the handle.
    // A `late` field whose initialiser mentions `this` -- `late final
    // nativeFilter = _ImageFilter.matrix(this)` -- starts absent in the
    // literal and is written right after it, when `__new` exists to be
    // named. Not a `late` one: it has no absence to start from, and stays
    // refused below.
    final deferred = <String, IrExpr>{
      for (final field in _allFields(cls))
        if (field.isLate &&
            (inits[field.name] ?? field.initial) != null &&
            _mentionsThis((inits[field.name] ?? field.initial)!))
          field.name: (inits[field.name] ?? field.initial)!,
    };
    // The base constructors' bodies run too, deepest first, before this
    // one's: `BindingBase()` calls `initInstances()` and
    // `initServiceExtensions()` from its body, and no binding subclass
    // ran either until run441.
    final bases = _inheritedBodies(ctor);
    // `DART2RUST_TRACE_CTOR=<Class>`: the constructor chain a class's
    // constructor runs, with each body's statements, to stderr.
    if (Platform.environment['DART2RUST_TRACE_CTOR'] == cls.name) {
      String shape(IrStmt? body) => switch (body) {
        null => 'none',
        IrBlock(:final statements) =>
          statements
              .map(
                (s) =>
                    '${s.runtimeType}${s is IrExprStmt ? '(${s.expr.runtimeType})' : ''}',
              )
              .toList()
              .toString(),
        _ => body.runtimeType.toString(),
      };
      stderr.writeln(
        'TRACE_CTOR ${cls.name}.${ctor.name ?? 'new'} own=${shape(ctor.body)} '
        'bases=${[for (final (b, c, _) in bases) '${b.name}.${c.name ?? 'new'}:${shape(c.body)}']}',
      );
    }
    final built = ctor.body != null || deferred.isNotEmpty || bases.isNotEmpty;
    // A counted class is built *inside* its handle: the body's `this`
    // (`_recorder._canvas = this` in `_NativeCanvas`) is then the `Rc`
    // every holder wants, and the fields it writes are cells reached
    // through the handle just the same.
    final handleFirst = built && cls.counted;
    _line(
      !built
          ? (cls.counted ? 'dart_rc(Self {' : 'Self {')
          : handleFirst
          ? 'let __new = dart_rc(Self {'
          : 'let mut __new = Self {',
    );
    _indent++;
    if (cls.counted) _line('__self: DartSelf::new(),');
    for (final field in _allFields(cls)) {
      if (deferred.containsKey(field.name)) {
        _line(
          _inCell(field)
              ? '${snake(field.name)}: std::rc::Rc::new(std::cell::'
                    '${_isCopy(_heldDecl(field)) ? 'Cell' : 'RefCell'}'
                    '::new(None)),'
              : '${snake(field.name)}: None,',
        );
        continue;
      }
      // The constructor first, then the declaration's own value: Dart applies
      // the latter only where the former says nothing.
      var init = inits[field.name] ?? field.initial;
      if (init == null && field.type.nullable) {
        // A nullable Dart field with no initialiser *is* null. Rust needs the
        // value written down, and `None` is exactly it -- not a stand-in.
        // Into a projected `T?` slot (`<T as DartNullable>::Or`, a
        // `RestorableValue<T?>`'s `_value` seen from `RestorableEnumN<T>`)
        // the null crosses as every value does, by `from_option`
        // (`IrNullableOf`; a bare `None` was "expected associated type",
        // ws688).
        final absent = IrLiteral('null', const IrType('Null', nullable: true));
        init = field.type.projected
            ? IrNullableOf(absent, field.type.name, toOption: false)
            : absent;
      }
      if (init == null) {
        // Dart's `late`, which starts with no value at all. `None` is that,
        // and the reads unwrap. See `IrFieldDecl.isLate`.
        if (field.isLate) {
          _line(
            _inCell(field)
                ? '${snake(field.name)}: std::rc::Rc::new(std::cell::'
                      '${_isCopy(_heldDecl(field)) ? 'Cell' : 'RefCell'}'
                      '::new(None)),'
                : '${snake(field.name)}: None,',
          );
          continue;
        }
        // Not `late` and not nullable, so Dart guaranteed a value and this
        // compiler lost it -- a constructor it could not read, most often.
        throw Unsupported('field never initialised', field.name);
      }
      // A field whose declaration initialiser mentions `this`:
      // `late final nativeFilter = _ImageFilter.matrix(this)`. In Dart the
      // object already exists when that runs; in Rust the struct literal is
      // still being built and there is no `self` at all. 152 of these came
      // out as `*self` inside `Self { .. }`, which is not a thing.
      if (_mentionsThis(init)) {
        throw Unsupported(
          'a field initialised from `this`',
          '${cls.name}.${field.name}',
        );
      }
      final held = type(field.type);
      // A closure literal into a field of function type is an `Rc<dyn Fn>`
      // there, as a constant's is (see the statics): `DateFormat
      // .dateTimeConstructor` took a bare closure where the field's type
      // named the trait object.
      final rendered = field.type.isFunction && init is IrClosure && !init.boxed
          ? 'std::rc::Rc::new(${expr(init)})'
          : expr(init);
      // A `late` field is an `Option` (`_lateField`); one with an
      // initialiser that does not mention `this` starts with it, in
      // `Some` (`ObserverList._set = HashSet<T>()`, run434).
      final value = field.isLate ? 'Some($rendered)' : rendered;
      _line(
        _inCell(field)
            ? '${snake(field.name)}: std::rc::Rc::new(std::cell::'
                  '${_isCopy(held) ? 'Cell' : 'RefCell'}::new($value)),'
            : '${snake(field.name)}: $value,',
      );
    }
    // The phantom fields the struct declaration added. They hold nothing, and
    // leaving them out of the literal is a missing field rather than a
    // harmless omission.
    for (final unused in _unusedParameters(cls)) {
      _line('_phantom_${snake(unused)}: std::marker::PhantomData,');
    }
    _indent--;
    final body = ctor.body;
    if (!built) {
      _line(cls.counted ? '})' : '}');
    } else {
      _line(handleFirst ? '});' : '};');
      // `this` inside the body is the value being built, not a `self` that
      // does not exist yet. `_selfName` is the same lever a free function
      // uses, so the body's `this.x = v` comes out as `__new.x = v`.
      final saved = _selfName;
      _selfName = '__new';
      for (final entry in deferred.entries) {
        final field = _allFields(cls).firstWhere((f) => f.name == entry.key);
        // A lazy one stays absent: its first read fills it (`_lazyRead`).
        if (_lazyLate(field)) continue;
        final value = 'Some(${expr(entry.value)})';
        _line(
          _inCell(field)
              ? (_isCopy(_heldDecl(field))
                    ? '__new.${snake(field.name)}.set($value);'
                    : '*__new.${snake(field.name)}.borrow_mut() = $value;')
              : '__new.${snake(field.name)} = $value;',
        );
      }
      // Nested, nearest base outermost: a base's parameters are bound
      // from the arguments the class below it passed -- which name that
      // class's own parameters -- so the bindings go downward, one block
      // per base, each shadowing the last; the bodies run on the way back
      // out, deepest first, as Dart runs them. A flat block per base
      // evaluated `super(child)` where no `child` was bound (ws523).
      // A bodiless base at the far end binds nothing anyone reads: no
      // block for it (an empty `{ let h = h; }` in every value class's
      // `const fn`, ws531).
      final chain = bases.reversed.toList();
      while (chain.isNotEmpty && chain.last.$2.body == null) {
        chain.removeLast();
      }
      for (final (_, baseCtor, superArgs) in chain) {
        _line('{');
        _indent++;
        final assigned = baseCtor.body == null
            ? const <String>{}
            : _assignedIn(baseCtor.body!);
        for (var i = 0; i < baseCtor.params.length; i++) {
          // Typed by the parameter: an unused `None` inferred nothing
          // (`configuration` in `_ReusableRenderView`, E0282 at ws461).
          final p = baseCtor.params[i];
          _line(
            'let ${assigned.contains(p.name) ? 'mut ' : ''}${snake(p.name)}: ${type(_substituteType(p.type, _baseTypes(cls, const {})))} = ${expr(superArgs[i])};',
          );
        }
      }
      // ..the kept bases' bodies, innermost block first -- the ones the
      // blocks were opened for, not the deepest of `bases`: with bodiless
      // `Element` and `ComponentElement` trimmed off, `bases.take(kept)`
      // picked `DiagnosticableTree`'s (none) and `StatefulElement`'s
      // `state._element = this` never ran (run535: `State.widget` on a
      // `None`).
      for (final (_, baseCtor, _) in chain.reversed) {
        if (baseCtor.body != null) {
          final savedReassigned = _reassigned;
          _reassigned = {..._reassigned, ..._assignedIn(baseCtor.body!)};
          stmt(baseCtor.body!);
          _reassigned = savedReassigned;
        }
        _indent--;
        _line('}');
      }
      if (body != null) stmt(body);
      _selfName = saved;
      _line(handleFirst || !cls.counted ? '__new' : 'dart_rc(__new)');
    }
    _line('})');
    _indent--;
    _line('}');
    // A clone reached the body by a road the initialisers' check above
    // does not see (a super constructor's argument, a widened `Duration`):
    // a `const fn` may not call it (38 in `gestures_events` at ws278).
    if (constness.isNotEmpty &&
        _out.sublist(signatureAt + 1).any((l) => l.contains('.clone()'))) {
      _out[signatureAt] = _out[signatureAt].replaceFirst('const fn ', 'fn ');
    }
    _line('');
  }

  /// The class's `static final` fields, as module-level `LazyLock`s.
  ///
  /// Written outside the `impl` because Rust has no associated `static`, and
  /// named with the class in front so two classes' `defaults` do not collide.
  void _emitLazyStatics() {
    for (final constant in cls.constants) {
      if (!constant.isLazy) continue;
      _member('${cls.name}.${constant.name}', () {
        final held = type(constant.type);
        // Wrapped in `Isolate`, which is where "a Dart static is one per
        // isolate" is written down. A Rust `static` is one per process and so
        // must hold something `Sync`; `Box<dyn Fn(Image)>` is not, and that
        // was 94 `E0277`s. See the prelude for what the wrapper's `unsafe`
        // claims and when it stops being true.
        _doc(constant.doc);
        // Assignable, so a `RefCell` inside the `Isolate`: the same cell a
        // mutable top-level gets, read with `borrow` and written with
        // `borrow_mut` in `IrAssignStatic`.
        final cell = constant.isMutable ? 'std::cell::RefCell<$held>' : held;
        final made = constant.isMutable
            ? 'std::cell::RefCell::new(${constant.value is IrClosure && !(constant.value as IrClosure).boxed ? 'std::rc::Rc::new(${expr(constant.value)})' : expr(constant.value)})'
            : expr(constant.value);
        _line(
          '${_vis(constant.name)}static ${_lazyName(cls.name, constant.name)}: '
          'std::sync::LazyLock<Isolate<$cell>> = '
          'std::sync::LazyLock::new(|| Isolate($made));',
        );
        _line('');
      });
    }
  }

  void _emitConstants({String? prefix}) {
    for (final constant in cls.constants) {
      if (constant.isLazy) continue;
      // Each constant on its own: one that cannot be built is one constant
      // missing, not a class.
      _member(
        '${cls.name}.${constant.name}',
        () => _emitConstant(constant, prefix: prefix),
      );
    }
    if (cls.constants.isNotEmpty) _line('');
  }

  void _emitConstant(IrConstDecl constant, {String? prefix}) {
    if (!_constable(type(constant.type))) {
      throw Unsupported(
        'a `const` cannot hold a collection',
        '${cls.name}.${constant.name}',
      );
    }
    _doc(constant.doc);
    final spelled = prefix == null
        ? screamingSnake(constant.name)
        : screamingSnake('${prefix}_${constant.name}');
    _line(
      '${_vis(constant.name)}const $spelled: '
      '${type(constant.type)} = ${expr(constant.value)};',
    );
  }

  void _emitMethods() {
    for (final method in cls.methods) {
      if (method.operator != null) continue;
      if (method.isStatic && _freeStatics(cls.name)) continue;
      _member(
        '${cls.name}.${method.name}',
        () => _emitMethod(method),
        stub: (reason) => _emitMethod(method, stubbed: reason),
      );
    }
    // A concrete superclass's methods, on the subclass: `ValueNotifier
    // extends ChangeNotifier` has `ChangeNotifier`'s fields (flattened in)
    // and, in Dart, its methods -- `notifyListeners()` from `set value`. A
    // struct inherits nothing, so the body is emitted again here, over the
    // same field names. Only for an ancestor without type parameters (its
    // `T` is not this class's) and not overridden here.
    final have = <String>{
      for (final m in cls.methods) m.name,
      for (final f in _allFields(cls)) f.name,
    };
    for (final ancestor in _concreteAncestors()) {
      for (final method in ancestor.methods) {
        if (method.operator != null || method.isStatic) continue;
        // Nearest first: a name already seen is overridden below this one.
        if (!have.add(method.name)) continue;
        _member(
          '${cls.name}.${method.name} (from ${ancestor.name})',
          () => _emitMethod(method),
        );
      }
    }
  }

  /// The `extends` chain above this class, nearest first: the concrete,
  /// non-generic classes of this library whose methods a struct has to
  /// carry itself.
  List<IrClass> _concreteAncestors() {
    final out = <IrClass>[];
    var name = cls.superclass;
    final seen = <String>{cls.name};
    while (name != null && seen.add(name)) {
      final ancestor = library[name];
      if (ancestor == null || ancestor.isEnum) break;
      // Past an abstract ancestor, not stopped by it: `_SwitchPainter`
      // extends the abstract `ToggleablePainter`, which extends the
      // concrete `ChangeNotifier`, and `notifyListeners` is the latter's
      // (58 "no method named `notify_listeners`" in `cupertino`).
      if (ancestor.isAbstract) {
        name = ancestor.superclass;
        continue;
      }
      if (_generics(ancestor).isNotEmpty) break;
      out.add(ancestor);
      name = ancestor.superclass;
    }
    return out;
  }

  /// Whether the method being printed takes `&mut self` (`_sharedMutation`).
  /// A lending local function's closure has to reborrow one rather than
  /// move it (see `IrLocalFunction.lends`).
  var _selfIsMut = false;

  void _emitMethod(IrMethod method, {String? as, String? stubbed}) {
    _selfIsMut = !method.isStatic && _sharedMutation(method);
    {
      // A static `of<T>` inside `ScopedModel<T>`: Rust will not have the
      // name twice (E0403, the two errors outside any body once the
      // widgets crate passed). The signature is renamed and the body,
      // which would need the same rename, is a stub that says so.
      // ..an *instance* method's: a static one is a free function with
      // no class parameter in scope to collide with, and its body spells
      // its own `T` unrenamed (`InheritedModel.inheritFrom<T>`, the
      // `MediaQuery.maybeOf` every widget asks: run541; all 7 stubs of
      // this kind were statics).
      final renamed = method.isStatic ? null : _renamedShadowed(method);
      if (renamed != null) {
        method = renamed;
        stubbed ??=
            "a method whose type parameter shadows the class's: "
            '${cls.name}.${method.name}';
      }
      // Before the signature: whether a parameter needs `mut` is decided by the
      // body, and the signature is written first.
      _reassigned = _assignedIn(method.body);
      _mutRefParams = {
        for (final p in method.params)
          if (p.mutRef) p.name,
      };
      _cellLocals = {};
      _doc(method.doc);
      final params = [
        if (!method.isStatic) _receiverOf(method),
        // Parameters are a borrowed position: a function type there is
        // `impl Fn(..)`, which a closure literal can be passed to
        // directly, rather than `Box<dyn Fn(..)>`, which would need a
        // `Box::new` at every call site.
        ...method.params.map((p) => _param(p, owned: false)),
      ].join(', ');
      // A setter returns nothing: Dart's `set x(v)` has no return type, and
      // giving one a value would make `a.x = 1` an expression, which it is not.
      final returns = _returnType(method);
      _failure = _failureOf(method);
      _rustReturns = returns;
      _referenceParams = {
        for (final p in method.params)
          // Asked of the **emitted** type, not of the Dart name. `Object` is
          // the parameter of every `operator ==` and it is not one of this
          // package's abstract classes -- it is the prelude's trait -- so a
          // rule that consulted `library.isAbstract` missed all 251 of them
          // while `&dyn Object` was sitting in the signature. The same shape
          // as `_isCopy` two rounds ago: the ruler and its name disagreed.
          if (type(p.type, owned: false).startsWith('std::rc::Rc<dyn '))
            p.name: snake(p.name)
          // A counted class is an `Rc<Foo>` by value. The handle is not the
          // object, so the object is what gets asked.
          else if (library[p.type.name]?.counted ?? false)
            p.name: '&*${snake(p.name)}',
      };
      final name = as ?? _rustName(method);
      // An `async` method that translated is the body under `name__body`
      // and the spawning wrapper under `name` (`_emitAsyncWrapper`); one
      // that did not is the wrapper alone, panicking.
      final async = method.isAsync && stubbed == null;
      if (method.isAsync) {
        final mutable =
            !method.isStatic && _receiverOf(method).startsWith('&mut');
        final receiver = method.isStatic
            ? null
            : (
                'let ${mutable ? 'mut ' : ''}__self = ${_selfHandle()};',
                _selfIsHandle
                    ? '&__self'
                    : cls.counted
                    ? '&*__self'
                    : mutable
                    ? '&mut __self'
                    : '&__self',
              );
        if (stubbed != null) {
          _line(
            '${_vis(method.name)}fn $name${_generics(method)}($params) -> ${_futureOf(method)} {',
          );
          _indent++;
          _line('panic!("dart2rust: not translated: ${_stubText(stubbed)}")');
          _indent--;
          _line('}');
          _line('');
          return;
        }
        _emitAsyncWrapper(
          method,
          '${_vis(method.name)}fn $name${_generics(method)}($params) -> ${_futureOf(method)}',
          // A free static (an abstract class's) has no `Self` to go through
          // (E0433, 19 at ws465).
          method.isStatic && _freeStatics(cls.name)
              ? '${name}__body'
              : 'Self::${name}__body',
          receiver: receiver,
          turbofish: method.typeParameters.isEmpty
              ? ''
              : '::<${method.typeParameters.join(', ')}>',
        );
        _line('');
      }
      _line(
        '${_vis(method.name)}${async ? "async " : ""}fn '
        '${async ? '${name}__body' : name}${_generics(method)}($params) -> $returns {',
      );
      _indent++;
      _returns = method.returnType;
      _here = '${cls.name}.${method.name}';
      _asyncBody = method.isAsync;
      _methodTypeParams = method.typeParameters;
      // A failing `void` method that falls off its end still has to
      // produce its `Ok(())`: `_validateColorStops` ends in an `if`/`else`
      // that only ever returns `Err`, and the value of that `if` is `()`.
      // An async method's value is the awaited one: `Future<void>` falls
      // off into `Ok(())` too (54 in `widgets`).
      final produced = method.isAsync
          ? _awaited(method.returnType)
          : method.returnType;
      // The null the body's type falls into, the same one a closure's body
      // gets from `_body`: `()`, a `None` of the spelled `Option`, the
      // `Null` object of a `dynamic`. Only `()` was asked for here, so an
      // `async Future<dynamic>` whose body ends in an `if`/`else if` chain
      // -- `_handleTextInputInvocation`, `_handleUndoManagerInvocation`,
      // `_handlePlatformMessage` -- left the chain's `()` in the tail
      // position of a `Result<Rc<dyn Object>, ..>`, and a bare `return;`
      // inside one became `Ok(())` (ws874).
      final falling = _fallsOffValue(type(produced));
      final fallsOff =
          _failure != null && falling != null && !_alwaysReturns(method.body);
      final savedFalling = _fallsOff;
      _fallsOff = falling;
      if (stubbed != null) {
        _line('panic!("dart2rust: not translated: ${_stubText(stubbed)}")');
      } else {
        stmt(method.body, tail: !fallsOff);
        if (fallsOff) _line('Ok($falling)');
      }
      _fallsOff = savedFalling;
      // `TileMode` to text as an `if`/`else if` chain over every variant with
      // no final `else`: Dart lets the body fall off the end (returning null
      // it would then refuse at runtime); Rust wants the last `if` to be an
      // expression of the return type. The chain is exhaustive by the
      // author's reckoning, and the line after it says so.
      if (!fallsOff && stubbed == null) _closeOpenIf(method.body);
      _returns = null;
      _indent--;
      _line('}');
      _line('');
    }
  }

  /// After a body: the line that ends an open `if` chain, when the method
  /// has a value to return and the chain is how it returns it.
  void _closeOpenIf(IrStmt body) {
    final returns = _returns;
    if (returns == null || type(returns) == '()') return;
    if (_alwaysReturns(body) || !_endsInOpenIf(body)) return;
    _line('unreachable!("no branch of the if chain returned")');
  }

  /// Whether a body ends in an `if` chain that returns on every branch it
  /// has, and has no `else` to end it.
  bool _endsInOpenIf(IrStmt s) => switch (s) {
    IrBlock(:final statements) =>
      statements.isNotEmpty && _endsInOpenIf(statements.last),
    IrIf(:final then, :final otherwise) =>
      otherwise == null ? _alwaysReturns(then) : _endsInOpenIf(otherwise),
    _ => false,
  };

  void _emitOperators() {
    for (final method in cls.methods) {
      final op = method.operator;
      if (op == null) continue;
      _member('${cls.name} operator $op', () => _emitOperator(method, op));
    }
  }

  void _emitOperator(IrMethod method, String op) {
    {
      final mapping = operatorTraits[op];
      if (mapping == null) {
        // `~/` has no Rust trait. Emitted as an inherent method rather than
        // forced into one that means something else.
        // ..and as a method in every respect: it returns `Result` and
        // its body may `?`, as the trait's declaration of the same
        // operator does (`stdOperators`).
        _line('');
        _line('impl${_implGenerics(cls)} ${cls.name}${_generics(cls)} {');
        _indent++;
        _emitMethod(method, as: _operatorName(op));
        _indent--;
        _line('}');
        return;
      }
      final (trait, fn) = mapping;
      final rhs = method.params.isEmpty ? null : method.params.single;
      _line('');
      _doc(method.doc);
      final generic = rhs == null ? '' : '<${type(rhs.type)}>';
      _line(
        'impl${_implGenerics(cls)} std::ops::$trait$generic for '
        '${cls.name}${_generics(cls)} {',
      );
      _indent++;
      _line('type Output = ${type(method.returnType)};');
      _line('');
      final params = [
        'self',
        if (rhs != null) '${snake(rhs.name)}: ${type(rhs.type)}',
      ].join(', ');
      // The body lives in an inherent method the trait impl forwards to.
      // Inside `impl std::ops::Add for Matrix3`, the trait is in scope, and
      // `cascaded.add(arg)` in the body of `operator +` -- Dart's own
      // `add`, `&mut self` -- resolved to the by-value `Add::add` first:
      // 8 `E0382`s and an infinite recursion in vector_math.
      final own = _operatorName(method.operator!);
      _line(
        'fn $fn($params) -> Self::Output { '
        'Self::$own(${['self', if (rhs != null) snake(rhs.name)].join(', ')}) }',
      );
      _indent--;
      _line('}');
      _line('');
      _line('impl${_implGenerics(cls)} ${cls.name}${_generics(cls)} {');
      _indent++;
      _line('pub fn $own($params) -> ${type(method.returnType)} {');
      _indent++;
      _returns = method.returnType;
      _here = '${cls.name}.${method.name}';
      _asyncBody = method.isAsync;
      _methodTypeParams = method.typeParameters;
      _reassigned = _assignedIn(method.body);
      _mutRefParams = {
        for (final p in method.params)
          if (p.mutRef) p.name,
      };
      _cellLocals = {};
      // An operator's signature is `std::ops`'s and cannot say `Result`:
      // inside it a failing call unwraps.
      final savedFailure = _failure;
      _failure = null;
      // ..and takes `self` by value: `this` inside is `self`, not `*self`
      // (`Priority.operator -` doing `this + (-offset)`, E0614 at ws463).
      _selfByValue = true;
      final closed = _body(
        method.body,
        method.isAsync ? _awaited(method.returnType) : method.returnType,
      );
      _selfByValue = false;
      if (!closed) _closeOpenIf(method.body);
      _failure = savedFailure;
      _returns = null;
      _indent--;
      _line('}');
      _indent--;
      _line('}');
    }
  }

  /// A Rust-legal name for a Dart operator.
  ///
  /// The fallback used to be `op_` plus the code units, which turned `==` into
  /// `op_61_61` -- legal, but unreadable and unsearchable. Every operator Dart
  /// has is named here instead; anything genuinely unknown stops rather than
  /// being spelled in decimal.
  static String _operatorName(String op) => switch (op) {
    '+' => 'op_add',
    '-' => 'op_sub',
    '*' => 'op_mul',
    '/' => 'op_div',
    '%' => 'op_rem',
    'unary-' => 'op_neg',
    '~/' => 'int_div',
    '[]' => 'index_of',
    '[]=' => 'index_set',
    '==' => 'op_eq',
    '<' => 'lt',
    '>' => 'gt',
    '<=' => 'le',
    '>=' => 'ge',
    '&' => 'bit_and',
    '|' => 'bit_or',
    '^' => 'bit_xor',
    '~' => 'bit_not',
    '<<' => 'shl',
    '>>' => 'shr',
    '>>>' => 'ushr',
    // The name is quoted *and* described: an empty one said
    // "operator `` has no Rust name", 367 times, which names neither the
    // operator nor where it came from.
    '' => throw Unsupported('a member with no name', '<empty>'),
    _ => throw Unsupported('operator `$op` has no Rust name', op),
  };

  /// A Rust-legal identifier for any Dart member name.
  ///
  /// `superFn` pastes the name into another identifier, so an operator's own
  /// spelling cannot go through: `superFn('AlignmentGeometry', '==')` produced
  /// `alignment_geometry_super_`, a name with nothing on the end of it.
  static String _identifier(String name) =>
      // Any letters at all: `___sendPlatformMessage$Method$FfiNative`, the
      // AOT lowering of an `@Native` external, is a name for `snake` to
      // clean, not an operator, and refusing it took `PlatformDispatcher.
      // instance` with it (20 callers).
      _stdShadowed[name] ??
      (RegExp(r'[A-Za-z]').hasMatch(name) ? snake(name) : _operatorName(name));

  /// `dart:core` methods whose snake-cased name is an *unstable* inherent
  /// method of Rust's std, which outranks any trait's: spelled by the
  /// prelude's own name (`String.replaceFirst`, E0658 16 at ws465).
  static const _stdShadowed = {
    'replaceFirst': 'dart_replace_first',
    // `str::starts_with`/`ends_with` take a `Pattern`, which a `String`
    // is not (`_findFamilyWithVariantAssetPath`, ws579).
    'startsWith': 'dart_starts_with',
    'endsWith': 'dart_ends_with',
  };
}
