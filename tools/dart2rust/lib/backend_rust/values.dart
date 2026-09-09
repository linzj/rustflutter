part of '../backend_rust.dart';

// Const instances, interpolation, chains, identity and `new`.
augment class RustBackend {
  /// `Alignment { x: -1.0, y: -1.0 }`.
  ///
  /// Only for a class this file emits. The struct literal names fields, and the
  /// only fields whose Rust names are known are the ones written here -- a
  /// `Duration { _duration: 1000 }` would be naming a field of a hand-written
  /// stub and would go wrong quietly the day the stub was spelled differently.
  String _constInstance(IrType t, Map<String, IrExpr> fields) {
    // The prelude's types are not in the IR, so nothing here knows their
    // fields -- and two of them account for 276 of the 305 refusals.
    //
    // `Duration` carries one field, `inMicroseconds`, which is the prelude's
    // `microseconds` under another name. `SentinelValue` carries none: it is
    // dart:core's "no argument was passed" marker, and what upstream does with
    // it is compare identities, so an empty struct says everything it says.
    if (t.name == 'Duration') {
      final micros = fields['inMicroseconds'] ?? fields['_duration'];
      if (micros != null) {
        return 'Duration { microseconds: ${expr(micros)} }';
      }
    }
    if (t.name == 'SentinelValue' && fields.isEmpty) {
      return 'SentinelValue';
    }
    // `Endian.little`/`Endian.big`: the prelude's enum, from the constant's
    // one field. Every `Paint` getter reads a `ByteData` with one (14).
    if (t.name == 'Zone' && fields.isEmpty) return 'Zone';
    // The prelude's unit codecs: `const Utf8Codec()`, `const JsonCodec()`.
    if (t.name == 'Utf8Codec' || t.name == 'JsonCodec') return t.name;
    if (t.name == 'Endian') {
      final little = fields['_littleEndian'];
      return little != null && expr(little) == 'true'
          ? 'Endian::Little'
          : 'Endian::Big';
    }
    // `dart:io`'s `FileMode` and `FileLock`: `const FileMode._internal(n)`
    // by the number it carries, the prelude's enums (`GetStorage`'s IO
    // backend, refused since it was reached, ws478).
    if (t.name == 'FileMode' || t.name == 'FileLock') {
      final carried = fields[t.name == 'FileMode' ? '_mode' : '_type'];
      final variants = t.name == 'FileMode'
          ? const ['Read', 'Write', 'Append', 'WriteOnly', 'WriteOnlyAppend']
          : const [
              'Shared',
              'Shared',
              'Exclusive',
              'BlockingShared',
              'BlockingExclusive',
            ];
      final index = carried == null ? null : int.tryParse(expr(carried));
      if (index != null && index >= 0 && index < variants.length) {
        return '${t.name}::${variants[index]}';
      }
    }
    // A prelude class with a `const` form of its own: `const Stream()` is
    // `dart:async`'s abstract base constructor, and a stream with no events
    // is what the prelude's ready stream is when nothing filled it
    // (`BaseRequest.finalize`, `LicenseRegistry.licenses`; 2 at ws830).
    // A table, like every other `dart:core` mapping here.
    final preludeConst = _preludeConstInstances[t.name];
    if (preludeConst != null && fields.isEmpty) return preludeConst;
    // By module where the constant carries one: the fields are that
    // class's, not another library's class of the same name.
    final cls = library.resolve(t);
    if (cls == null) {
      throw Unsupported(
        'const instance of `${t.name}`, which is not in this file',
        'const ${t.name}(..)',
      );
    }
    final wanted = _allFields(cls).map((f) => f.name).toList();
    final missing = wanted.where((f) => !fields.containsKey(f)).toList();
    final extra = fields.keys.where((f) => !wanted.contains(f)).toList();
    if (missing.isNotEmpty || extra.isNotEmpty) {
      // The constant and the struct disagree about what the class holds. That
      // is a fact about this compiler, not about the program, so it is said
      // plainly rather than patched over with a default.
      throw Unsupported(
        'const instance of `${t.name}`: the struct '
            '${missing.isEmpty ? "has no" : "wants"} '
            '${missing.isEmpty ? extra.join(", ") : missing.join(", ")}',
        'const ${t.name}(..)',
      );
    }
    // A field declared as a trait object takes the same `Rc::new` a return
    // does: `const _ClampTransform(_P3ToSrgbTransform())` holds its child
    // as an `Rc<dyn _ColorTransform>`.
    final parts = <String>[];
    for (final f in _allFields(cls)) {
      if (!fields.containsKey(f.name)) continue;
      final outer = _returns;
      _returns = f.type;
      final value = fields[f.name]!;
      // A constant into a nullable field goes in through `Some`: the
      // `bool? signed` of a `const TextInputType(..)` held `false` bare
      // (26 in `widgets`).
      final wrapped =
          f.type.nullable &&
          ((value is IrLiteral &&
                  value.type.name != 'Null' &&
                  !value.type.nullable) ||
              value is IrConstInstance ||
              value is IrNew ||
              value is IrUpcast ||
              value is IrListLiteral ||
              value is IrMapLiteral ||
              // An enum value into a `TextBaseline?` (141 in `material`).
              (value is IrStatic && value.isEnumValue));
      final text = _returned(value);
      final stored = wrapped ? 'Some($text)' : text;
      // Into the cell the struct holds it in (`_fieldType`).
      final celled = _inCellOf(cls, f)
          ? (_isCopy(_heldDecl(f))
                ? 'std::rc::Rc::new(std::cell::Cell::new($stored))'
                : 'std::rc::Rc::new(std::cell::RefCell::new($stored))')
          : stored;
      parts.add('${snake(f.name)}: $celled');
      _returns = outer;
    }
    if (cls.counted) parts.add('__self: DartSelf::new()');
    // The phantom fields a generic class carries (see the struct's
    // emission): `const PersistentHashMap<Type, InheritedElement>.empty()`
    // (ws475).
    for (final unused in _unusedParameters(cls)) {
      parts.add('_phantom_${snake(unused)}: std::marker::PhantomData');
    }
    // ..with the type arguments the constant carries, where the struct
    // is generic: nothing else infers a phantom's `T` in a `dynamic` slot.
    // ..unless one is `Never`, Dart's unconstrained default for a const
    // generic (`const DeepCollectionEquality()` is a `DefaultEquality<
    // Never>`): that one is the slot's to infer (ws588).
    final spellable = t.arguments.every((a) => !_mentionsNever(a));
    final turbofish = t.arguments.isEmpty || !spellable
        ? ''
        : '::<${t.arguments.map(type).join(', ')}>';
    return '${_spelled(t)}$turbofish { ${parts.join(', ')} }';
  }

  /// A constructor's Rust name. One function, used by both the definition and
  /// the call, because two spellings of the same rule is how a call ends up
  /// naming a function nobody wrote.
  ///
  /// Dart's `Foo._()` -- the private default constructor, and a common idiom --
  /// snakes to `_`, which Rust reserves. It becomes `new_`: still recognisable
  /// as the constructor, and a name Rust will take.
  static String _ctorName(String? dartName) {
    if (dartName == null) return 'new';
    final name = snake(dartName);
    return name == '_' ? 'new_' : name;
  }

  /// `a.b = v` where the value of the assignment is wanted.
  ///
  /// Rust's assignment produces `()`, so the value is bound first and produced
  /// after -- not re-read from the field, which would be a second read of
  /// something a setter or another thread could have changed.
  String _setValue(IrExpr? target, String name, IrExpr value) {
    final receiver = target == null ? _selfName : expr(target);
    // A counted class's field is a cell: `_count++` used for its value
    // wrote `self._count = __set` against an `Rc<Cell<i64>>`.
    final shared = (target == null || target is IrThis)
        ? _sharedField(name)
        : null;
    if (_fieldsAreAccessors && (target == null || target is IrThis)) {
      final through = _accessorQualifier(name, kind: 'write');
      final widened = '__set.clone()';
      return through == null
          ? '{ let __set = ${expr(value)}; $receiver.set_${snake(name)}($widened)$_propagate; __set }'
          : '{ let __set = ${expr(value)}; $through::set_${snake(name)}($receiver, $widened)$_propagate; __set }';
    }
    if (shared != null) {
      final copy = _isCopy(_heldDecl(shared));
      return copy
          ? '{ let __set = ${expr(value)}; $receiver.${snake(name)}.set(__set); __set }'
          : '{ let __set = ${expr(value)}; '
                '*$receiver.${snake(name)}.borrow_mut() = __set.clone(); __set }';
    }
    return '{ let __set = ${expr(value)}; '
        '$receiver.${snake(name)} = __set.clone(); __set }';
  }

  /// `'a \$b c'` as `format!`.
  ///
  /// The literal pieces become the format string and the rest its arguments.
  /// A literal's own braces are doubled, since `format!` reads them.
  String _interpolation(List<IrExpr> parts) {
    final pattern = StringBuffer();
    final args = <String>[];
    for (final part in parts) {
      if (part is IrLiteral && part.type.name == 'String') {
        pattern.write(part.value.replaceAll('{', '{{').replaceAll('}', '}}'));
        continue;
      }
      pattern.write('{}');
      args.add(expr(part));
    }
    // A backslash first, then a quote: doing it the other way round would
    // escape the backslash this line just added.
    final text = pattern
        .toString()
        .replaceAll(r'\', r'\\')
        .replaceAll('"', r'\"');
    return args.isEmpty
        ? '"$text".to_string()'
        : 'format!("$text", ${args.join(', ')})';
  }

  /// The iterator part of a chain, and whatever ends it (`tail`).
  ///
  /// A step whose callback is a *value* rather than a written closure is
  /// bound before the chain: inlined, it is rebuilt once per element, and
  /// its statements land in the closure the step makes rather than in the
  /// function that wrote them. A `switch` expression with a throwing arm
  /// reaching `where` put that arm's `return Err(..)` inside a `-> bool`
  /// closure (`_sortAndFilterHorizontally`, 8 at ws747). The bindings run
  /// before the source, which Dart evaluates first: both are `?` sites, so
  /// the only difference is which of two throws is reported.
  String _chain(IrIterChain chain, {String tail = ''}) {
    final bound = <String>[];
    // A bare `forEach` hands the closure each element by value, as Dart
    // does: `keys.forEach(_updateProperty)` gave it `&Rc<..>` (53).
    final owned =
        chain.steps.length == 1 && chain.steps.single.$1 == 'for_each';
    final steps = chain.steps.map((step) {
      String? name;
      if (step.$2 is! IrClosure) {
        name = '__f${bound.length}';
        bound.add('let $name = ${expr(step.$2)};');
      }
      return '.${step.$1}(${_stepClosure(step.$2, step: step.$1, bound: name, cloned: owned)})';
    }).join();
    final body =
        '${_asList(chain.source)}.iter()${owned ? '.cloned()' : ''}$steps$tail';
    return bound.isEmpty ? body : '{ ${bound.join(' ')} $body }';
  }

  /// A value read as a list: an `Iterable<T>` is a `Rc<dyn DartIterable<T>>`
  /// since ws908, and a Rust iterator starts at a list.
  String _asList(IrExpr e) =>
      e.rustType?.name == 'Iterable' ? '${expr(e)}.dart_to_list()' : expr(e);

  /// Whether an argument is the omitted one, written out.
  ///
  /// Kernel fills a default in and the analyzer leaves it off, so a member
  /// whose Rust says the absent case differently has to see through that.
  static bool _isDefault(IrExpr e, String? empty) =>
      e is IrLiteral &&
      (empty == null
          ? e.type.name == 'Null'
          : e.type.name == 'String' && e.value == empty);

  /// A chain step's closure, without its parameter types.
  ///
  /// `iter()` yields references, so the Dart type is the wrong annotation --
  /// `|m: i64|` against a `&i64` does not compile. Left off, Rust infers it,
  /// and the body reads the same either way.
  String _stepClosure(
    IrExpr e, {
    String step = '',
    String? bound,
    bool cloned = false,
  }) {
    // A function *value* as the step (`where(shouldNotSkip)`): called
    // from a closure of the step's own shape -- `filter` hands `&&T`,
    // the rest the item -- and its `Result` unwrapped, as a written
    // closure's is (E0631, 17 at ws464). By the name `_chain` bound it to,
    // so it is built once and outside.
    if (e is! IrClosure) {
      final item = step == 'filter' ? '(*__x).clone()' : '__x.clone()';
      return '|__x| (${bound ?? expr(e)})($item).unwrap()';
    }
    // `filter` hands `&&T`, and a body written for the item -- `asset.
    // endsWith(other)`, a tear-off's own parameter passed on bare --
    // does not read through two references (`dart_ends_with(&&String)`,
    // `_findFamilyWithVariantAssetPath`, run604). The item is cloned out
    // first, so the body sees what a `map` step's does.
    final owned = step == 'filter';
    // ..and a scalar parameter of any step is bound by value: `iter()`
    // yields `&i64`, and a body handing it on to a callee that takes an
    // `i64` -- `model.getProductById(id)` over `productsInCart.keys` --
    // has no deref to reach through the reference (3 at ws793). A scalar
    // is `Copy`, so the binding costs nothing.
    // ..unless the source already handed values out (`iter().cloned()`,
    // which `for_each`, `any` and `all` take): there is nothing to deref.
    bool byValue(IrParam p) => !cloned && (owned || _isCopy(type(p.type)));
    // The temporary's name is spelled from the *identifier*, not from the
    // Rust name: a parameter called `box` snakes to `r#box`, and
    // `__p_r#box` is a prefixed identifier, which Rust 2021 reserves
    // (`TextPainter.getBoxesForSelection`, ws797).
    String temp(IrParam p) => '__p_${snake(p.name).replaceAll('r#', '')}';
    final params = e.params
        .map((p) => byValue(p) ? temp(p) : snake(p.name))
        .join(', ');
    final unwrapped = e.params
        .where(byValue)
        .map(
          (p) =>
              'let ${_assignedIn(e.body).contains(p.name) ? 'mut ' : ''}'
              '${snake(p.name)} = (*${temp(p)}).clone(); ',
        )
        .join();
    final saved = _out.length;
    final savedIndent = _indent;
    _indent = 0;
    // A step of a std iterator chain (`all`, `map`, `filter`) returns a
    // plain value: a failing call inside unwraps, and the tail is bare.
    // Loud, and recorded: an exception in a `where` predicate panics.
    final savedFailure = _failure;
    _failure = null;
    // Which copies are cells, for the body: a shared field's copy is its
    // cell (`_copyOf`), read through `borrow()` as the boxed closure's is
    // (`_closure`); as a plain local it was asked the set's methods
    // (the stepcapture fixture).
    final savedCells = _cellLocals;
    _cellLocals = {
      ..._cellLocals,
      for (final c in e.captures)
        if (_sharedField(c.name) != null) c.name: _isCopy(type(c.type)),
    };
    final savedLateCells = _lateCellLocals;
    _lateCellLocals = {
      ..._lateCellLocals,
      for (final c in e.captures)
        if (_sharedField(c.name) != null && _lateField(c.name) != null) c.name,
    };
    final savedSpells = _spellsReturn;
    _spellsReturn = true;
    // ..and against the step's *own* return, as `_closure` does for the
    // boxed kind: left at the enclosing method's, a step returning a
    // concrete class got that method's trait around it, and the coercion
    // after the chain put a second one on top -- `dart_object` around an
    // `Rc<dyn Widget>`, whose pointee implements nothing
    // (`columns.map((c) => Expanded(child: c))` inside a `Widget build`,
    // the same shape as ws751 in the one closure it did not reach; 3
    // `build`s at ws866).
    final savedStepReturns = _returns;
    _returns = e.returns;
    stmt(e.body, tail: true);
    _returns = savedStepReturns;
    _spellsReturn = savedSpells;
    _cellLocals = savedCells;
    _lateCellLocals = savedLateCells;
    final body = _out.sublist(saved).map(_inlineSafe).join(' ');
    _out.removeRange(saved, _out.length);
    _indent = savedIndent;
    // The fields the closure copies in, as `_closure` does for the boxed
    // kind. A chain step that read `this.trashEmailIds` named a local that
    // this line had not declared.
    // ..spelled inside the step too, so a cell accessor's failure unwraps
    // as the body's calls do: with `?` it was "`?` in a closure that
    // returns no `Result`" (`MultiChildRenderObjectElement.children`'s
    // `where` over `_forgottenChildren`, run671).
    final copies = e.captures
        .map(
          (c) =>
              'let ${_assignedIn(e.body).contains(c.name) ? 'mut ' : ''}${snake(c.name)} = ${_copyOf(c)}; ',
        )
        .join();
    _failure = savedFailure;
    // A `forEach` step returns nothing whatever its closure's body is
    // worth: `xs.forEach(list.remove)` hands it a `bool`-returning
    // tear-off, which Dart's `void Function(T)` slot discards (the
    // tearcol fixture).
    if (step == 'for_each') {
      return '|$params| { $unwrapped${copies}let _ = { $body }; }';
    }
    // `expand(f)` is `flat_map`, which wants something Rust can iterate:
    // an `Iterable<T>` closure body is the handle since ws908, and its
    // list is what the chain goes on with.
    if (step == 'flat_map' && e.returns.name == 'Iterable') {
      return '|$params| { $unwrapped$copies{ $body }.dart_to_list() }';
    }
    return '|$params| { $unwrapped$copies$body }';
  }

  /// A read of a static, or of an enum value.
  ///
  /// A Dart `static final` becomes a module-level `LazyLock`, because an
  /// `impl` block may hold a `const` and not a `static`. So its name carries
  /// its class, and reading it dereferences the lock.
  String _staticRead(String owner, String name, bool isEnumValue) {
    // The owner's own spelling, which may be Dart's: see `variantNames`.
    if (isEnumValue) {
      final owned = library[owner];
      final names = owned == null ? null : variantNames(owned.values);
      return '$owner::${names?[name] ?? variantName(name)}';
    }
    // `dart:io`'s `Platform.version` and friends: the prelude's functions.
    if (owner == 'Platform') return 'platform_${snake(name)}()';
    // Two derefs: through the `LazyLock`, then through the `Isolate` that
    // carries "one per isolate, not one per process".
    if (_isMutableStatic(owner, name)) {
      return '({ let __r = (**${_lazyName(owner, name)}).borrow().clone(); __r })';
    }
    // A clone: the lock hands out a reference, and a read is a value.
    // `(**CHANGE_NOTIFIER__EMPTY_LISTENERS)` moved out of the lock (E0507).
    if (_isLazy(owner, name)) return '(**${_lazyName(owner, name)}).clone()';
    if (_freeStatics(owner)) return screamingSnake('${owner}_$name');
    return '$owner::${screamingSnake(name)}';
  }

  bool _isMutableStatic(String owner, String name) =>
      library[owner]?.constants.any((c) => c.name == name && c.isMutable) ??
      false;

  bool _isLazy(String owner, String name) =>
      library[owner]?.constants.any((c) => c.name == name && c.isLazy) ?? false;

  static String _lazyName(String owner, String name) =>
      screamingSnake('${owner}_$name');

  /// Whether a case value can be written as a Rust pattern.
  ///
  /// An enum variant and an integer or boolean literal can. A string cannot --
  /// `"x".to_string()` is a call -- and neither can anything computed.
  static bool _isPattern(IrExpr e) => switch (e) {
    IrStatic(:final isEnumValue) => isEnumValue,
    IrLiteral(:final type) => type.name == 'int' || type.name == 'bool',
    _ => false,
  };

  /// `identical(a, b)`.
  ///
  /// Only with `this` on one side. That is the `operator ==` fast path -- 140
  /// of upstream's 259 -- and there both sides really are references, so
  /// `std::ptr::eq` asks the question Dart asked. Between two locals it would
  /// not: a translated value type is a `Copy` struct, and two copies of the
  /// same value sit at different addresses while two names for one value may
  /// sit at the same one. Answering that with an address is worse than not
  /// answering.
  String _identical(IrExpr left, IrExpr right) {
    // Two nullable handles: identical when both null or both the same
    // object (the prelude asks; `&*a` on an `Option` was E0614, ws463).
    final leftType = left.rustType, rightType = right.rustType;
    // ..only handles: `dart_identical_opt` takes `Option<Rc<T>>`, and a
    // nullable slot of a class spelled by value holds the struct itself
    // (`BadgeThemeData? a` of every theme's `lerp`, 35 sites at ws747).
    // Those are two slots, and the prelude's value form answers them the
    // way two value locals are answered below: both absent, or the same
    // storage.
    bool nullableValue(IrExpr e) {
      final t = e.rustType;
      if (t == null || !isNullable(t) || t.isFunction) return false;
      final held = library[t.name];
      if (held != null) return !held.isAbstract && !held.counted;
      // Not a translated class at all: what the type *spells* decides,
      // since that is the representation `dart_identical_opt` has to take.
      // A nullable `List<E>`, `Map<K, V>` or `Set<E>` is the collection
      // itself, no `Rc` anywhere (`listEquals`'s and `lerpList`'s fast
      // path, ws873); `dynamic`, `Object` and every prelude interface do
      // spell `Rc`, and stay handles.
      final spelled = type(t);
      return spelled.startsWith('Option<') &&
          !spelled.startsWith('Option<std::rc::Rc<');
    }

    if (leftType != null &&
        rightType != null &&
        isNullable(leftType) &&
        isNullable(rightType)) {
      if (nullableValue(left) || nullableValue(right)) {
        return 'dart_identical_opt_value(&${expr(left)}, &${expr(right)})';
      }
      return 'dart_identical_opt(&${expr(left)}, &${expr(right)})';
    }
    // One side an absent-or-not handle (`identical(_cachedLocale, this)`
    // in `Locale.toString`, a static `Locale?`, run589): absent is never
    // identical, present is asked as two handles.
    bool nullableHandle(IrExpr e) {
      final t = e.rustType;
      if (t == null || !isNullable(t) || t.isFunction) return false;
      final held = library[t.name];
      return held != null && (held.isAbstract || held.counted);
    }

    String? handleText(IrExpr e) => e is IrThis
        ? _thisHandle()
        : nullableHandle(e)
        ? null
        : _handleLike(e) || _isReference(e)
        ? expr(e)
        : null;
    if (nullableHandle(left) || nullableHandle(right)) {
      final absent = nullableHandle(left) ? left : right;
      final other = identical(absent, left) ? right : left;
      final otherHandle = handleText(other);
      if (otherHandle != null) {
        // Matched through a reference: by value it moved the handle out of
        // a parameter read again below (`shouldRepaint`, ws590).
        return '(match &${expr(absent)} { Some(__o) => dart_identical_any(__o, &$otherHandle), None => false })';
      }
      if (nullableHandle(other)) {
        return 'dart_identical_opt(&${expr(absent)}, &${expr(other)})';
      }
    }
    // Two locals, or a local against a static: the addresses of the *slots*.
    // Two distinct slots are never the same address, so this says "not
    // identical" -- which is what Dart says of two distinct objects, and is
    // the fast-path answer `listEquals` and `setEquals` want before they
    // compare elements. What it cannot see is two handles to one `Rc`: those
    // read as distinct here where Dart would say identical. `_invoke`'s
    // `identical(zone, Zone.current)` is the one site that asks, and the
    // prelude has a single zone, so both branches run the callback the same
    // way. 36 call sites were behind this.
    // ..and a promoted nullable slot (`a!` of a `Map<T, U>? a`) of a
    // value type is that slot: two copies of a map have no identity
    // beyond the fast path (`mapEquals`' `identical(a, b)`, run574). A
    // promoted *handle* keeps the pointee rules below.
    bool slot(IrExpr e) =>
        e is IrLocal ||
        e is IrStatic ||
        (e is IrCall &&
            e.name == 'clone' &&
            e.args.isEmpty &&
            e.target != null &&
            slot(e.target!)) ||
        (e is IrNullCheck && slot(e.operand) && !_handleLike(e));
    // A slot against a static *call* -- `identical(zone, Zone.current)`,
    // the one site, in `_invoke` and its siblings (18 callers of those) --
    // binds the call and compares slots: distinct, as above.
    if (slot(left) && right is IrStaticCall) {
      return '{ let __i = ${expr(right)}; std::ptr::eq(&${expr(left)}, &__i) }';
    }
    if (left is IrStaticCall && slot(right)) {
      return '{ let __i = ${expr(left)}; std::ptr::eq(&__i, &${expr(right)}) }';
    }
    // Only when `_addressOf` has no better answer: a counted class's handle
    // is dereferenced below, and that path must keep winning for `Rc`s.
    if (slot(left) &&
        slot(right) &&
        (!_isReference(left) || !_isReference(right))) {
      return 'std::ptr::eq(&${expr(left)}, &${expr(right)})';
    }
    // `identical(x, 0)` / `identical(s, 'und')`: on a number, a string or
    // a bool Dart's `identical` is value equality (`KeyData._nonValueBits`,
    // `Locale.toString`).
    // `identical(_cachedLocale, this)` on a value class: the struct has no
    // identity to compare, so the cache never hits and is recomputed --
    // the same answer Dart gives for a fresh object, every time.
    if (!cls.counted &&
        ((left is IrStatic && right is IrThis) ||
            (left is IrThis && right is IrStatic))) {
      return 'false';
    }
    // Against a constant instance (`identical(_textScaler,
    // _kUnspecifiedTextScaler)`, `MediaQueryData.textScaler`, run546):
    // Dart canonicalises constants, so a value equal to the constant *is*
    // the constant -- the other side is asked for the constant's class and
    // compared by value; another class, or null, is not identical.
    if (left is IrConstInstance || right is IrConstInstance) {
      final constant = left is IrConstInstance ? left : right;
      final other = left is IrConstInstance ? right : left;
      final name = (constant as IrConstInstance).type.name;
      return '(match (${expr(other)}).dart_cast_any::<$name>() { Some(__c) => __c.dart_eq(&${expr(constant)}), None => false })';
    }
    if (left is IrLiteral || right is IrLiteral) {
      // TFA folds both sides to literals of different kinds: `identical(0,
      // 0.0)` is `false` in Dart, and `0 == 0.0` does not type in Rust.
      String side(IrExpr e, IrExpr other) =>
          e is IrLiteral &&
              e.type.name == 'int' &&
              other is IrLiteral &&
              other.type.name == 'double'
          ? '(${expr(e)} as f64)'
          : expr(e);
      return '(${side(left, right)} == ${side(right, left)})';
    }
    // A side typed `Object`/`dynamic` -- a list element, a `T` erased to
    // the object -- is an `Rc<dyn Object>`: the prelude asks (the same
    // object, or two canonical core values that are equal), the other side
    // shared into an object as any value is (ws557).
    bool objectTyped(IrExpr e) {
      final t = e.rustType;
      return t != null &&
          !isNullable(t) &&
          (t.name == 'Object' || t.name == 'dynamic') &&
          t.arguments.isEmpty;
    }

    if ((objectTyped(left) || objectTyped(right)) &&
        left.rustType != null &&
        right.rustType != null &&
        !isNullable(left.rustType!) &&
        !isNullable(right.rustType!)) {
      String asObject(IrExpr e) => objectTyped(e)
          ? '(${expr(e)}).clone()'
          : _handleLike(e)
          ? '(${expr(e)}.clone() as std::rc::Rc<dyn Object>)'
          : expr(
              IrUpcast(e, IrType('Object'), handle: false, explicit: true)
                ..rustType = const IrType('Object'),
            );
      return 'dart_identical(&${asObject(left)}, &${asObject(right)})';
    }
    // A handle *value* -- a getter's `Rc<BuildScope>`, a call's trait
    // object -- has an address once bound: `identical(element.buildScope,
    // this)` in `BuildScope._flushDirtyElements` was refused (run525).
    final bindLeft = !_isReference(left) && _handleLike(left);
    final bindRight = !_isReference(right) && _handleLike(right);
    if ((bindLeft || _isReference(left)) &&
        (bindRight || _isReference(right)) &&
        (bindLeft || bindRight)) {
      const asPtr = 'as *const u8 as *const ()';
      // A bare local is cloned into the binding: `other` in `operator ==`
      // was moved and read again (45 `_super_op_eq` at ws526).
      String bound(IrExpr e) => e is IrLocal ? '${expr(e)}.clone()' : expr(e);
      return '{ '
          '${bindLeft ? 'let __ha = ${bound(left)}; ' : ''}'
          '${bindRight ? 'let __hb = ${bound(right)}; ' : ''}'
          'std::ptr::eq('
          '${bindLeft ? 'std::rc::Rc::as_ptr(&__ha) $asPtr' : _asPointer(left)}, '
          '${bindRight ? 'std::rc::Rc::as_ptr(&__hb) $asPtr' : _asPointer(right)}) }';
    }
    if (!_isReference(left) || !_isReference(right)) {
      // The question is not "is one side `this`" -- it is whether both sides
      // are *references* in the emitted Rust. A parameter of a concrete type
      // arrives by value, because a translated value type is `Copy`, and the
      // address of a copy answers nothing: `identical(this, other)` there would
      // compile and always be false.
      // Which side, and what it is. "identical(.., ..)" 251 times said only
      // that something was wrong; the shapes are what the next round needs.
      String what(IrExpr e) => switch (e) {
        IrThis() => 'this',
        IrLocal(:final name) => name,
        IrField(:final name) => 'a field `$name`',
        IrNullCheck(:final operand) => 'a promoted ${what(operand)}',
        _ => '${e.runtimeType} (${e.rustType})',
      };
      throw Unsupported(
        '`identical` on something that is not a reference',
        '${_isReference(left) ? "" : "${what(left)} "}'
            '${_isReference(right) ? "" : what(right)}',
      );
    }
    // Through `*const ()` because the two sides have different Rust types --
    // `&Self` and `&dyn Trait` -- and identity is about the address, which both
    // of them have.
    return 'std::ptr::eq('
        '${_asPointer(left)}, '
        '${_asPointer(right)})';
  }

  /// Whether this expression is a reference in the emitted Rust.
  ///
  /// `self` always is. A local is one when it is a parameter whose Dart type is
  /// an abstract class, since that becomes `&dyn Trait` -- which is what
  /// upstream's `operator ==(Object other)` is.
  bool _isReference(IrExpr e) => _addressOf(e) != null;

  /// How to take the address of a value, or null when it has none to take.
  ///
  /// "Has an address" is not the same as "is written as a reference". A
  /// counted class arrives as an `Rc<Foo>` *by value*, and two handles to one
  /// object are two different addresses -- so the handle is dereferenced and
  /// the pointee's address is what identity is asked about. Getting that
  /// backwards answers the opposite of the question and compiles.
  /// One side of `identical` as a thin pointer. A parameter holding an
  /// `Rc<dyn X>` (see `_referenceParams`) is the handle, not a reference,
  /// and `Rc::as_ptr` is its address; `x as *const _` was an invalid cast.
  String _asPointer(IrExpr e) {
    final r = _ref(e);
    if (e is IrLocal && !r.startsWith('&')) {
      return 'std::rc::Rc::as_ptr(&$r) as *const u8 as *const ()';
    }
    return '$r as *const _ as *const ()';
  }

  /// See `IrDynamicDispatch`.
  String _dispatch(IrExpr receiver, List<(IrType?, IrExpr)> arms) {
    final out = StringBuffer('{ let __d = ${expr(receiver)}; ');
    final asAny = _handleLike(receiver)
        ? '__d.as_ref().as_any()'
        : '__d.as_any()';
    var first = true;
    for (final (t, body) in arms) {
      if (t == null) {
        out.write('${first ? '' : ' else '}{ ${expr(body)} }');
      } else {
        out.write(
          '${first ? '' : ' else '}if let Some(__t) = $asAny.downcast_ref::<${type(t)}>() '
          '{ let mut __d = __t.clone(); ${expr(body)} }',
        );
      }
      first = false;
    }
    if (arms.isEmpty || arms.last.$1 != null) {
      out.write(
        ' else { panic!("uncaught Dart exception: a dynamic slot held an unexpected type") }',
      );
    }
    out.write(' }');
    return out.toString();
  }

  String? _addressOf(IrExpr e) {
    if (e is IrThis) {
      // A closure's `__me` is a cloned handle, and a method that hands out
      // `this` takes `&Rc<Self>`. Both need one more deref than they look.
      if (_selfName == _countedSelf) return '&*$_countedSelf';
      return _selfIsHandle ? '&**$_selfName' : _selfName;
    }
    if (e is IrLocal) {
      final known = _referenceParams[e.name];
      if (known != null) return known;
      // A local holding a handle (`let b: Rc<Loc> = ..`): the object
      // behind it, as a parameter's is -- two locals compared by their
      // slots said "not identical" of one object (the dyncast fixture).
      final t = e.rustType;
      if (t != null &&
          !isNullable(t) &&
          !t.isFunction &&
          !_cellLocals.containsKey(e.name) &&
          !_closureCaptured.contains(e.name)) {
        final held = library[t.name];
        if (held != null && (held.counted || held.isAbstract)) {
          return '&*${snake(e.name)}';
        }
      }
      return null;
    }
    return null;
  }

  /// Parameters of the method being emitted that have an address, and how to
  /// take it.
  var _referenceParams = <String, String>{};

  /// Whether `self` here is `&Rc<Self>` rather than `&Self`.
  var _selfIsHandle = false;

  /// `self` is already a reference; anything else names one.
  String _ref(IrExpr e) => _addressOf(e)!;

  /// `dart:collection`'s internal implementation classes, by what they are.
  ///
  /// A Dart `<T>{}` resolves, in Kernel, to a constructor of `_Set`; a list
  /// literal that can grow is a `_GrowableList`. Those names are the runtime's
  /// own and nothing outside it declares them, so they came out as
  /// `_Set::new()` -- a module Rust has never heard of, 40 times.
  ///
  /// Only the empty constructors. `_GrowableList(n)` and `_List.filled` mean
  /// something else and are left to refuse rather than be guessed at.
  static const _collections = {
    'LinkedHashSet': 'Set',
    'HashSet': 'Set',
    'LinkedHashMap': 'Map',
    'HashMap': 'Map',
    // Sorted in Dart, the prelude's insertion-ordered ones here (the
    // alias says so): `SplayTreeMap<int, Element?>()` in
    // `SliverMultiBoxAdaptorElement` (run669).
    'SplayTreeMap': 'Map',
    'SplayTreeSet': 'Set',
    '_Set': 'Set',
    '_LinkedHashSet': 'Set',
    '_CompactLinkedHashSet': 'Set',
    '_HashSet': 'Set',
    '_Map': 'Map',
    '_LinkedHashMap': 'Map',
    '_InternalLinkedHashMap': 'Map',
    '_HashMap': 'Map',
    '_GrowableList': 'Vec',
    '_List': 'Vec',
  };

  String _new(IrType t, List<IrExpr> args, String? constructor) {
    final collection = _collections[t.name];
    if (collection != null) {
      // An omitted optional (`SplayTreeMap([compare, isValidKey])`) is no
      // argument: the front end fills it as `None` from the declaration.
      final real = args.where((a) => expr(a) != 'None').toList();
      if (real.isNotEmpty) {
        throw Unsupported(
          '`${t.name}` with arguments, which is not the empty collection',
          '${t.name}(..)',
        );
      }
      final arguments = t.arguments.isEmpty
          ? ''
          : '::<${t.arguments.map((a) => type(a)).join(', ')}>';
      return '$collection$arguments::new()';
    }
    // An abstract class is a trait here, and a trait has no constructor to
    // call. Dart's `Gradient.linear(..)` is a factory on an abstract class,
    // and the type of one is `Box<dyn Gradient>` -- so the call came out as
    // `Box<dyn Gradient>::linear(..)`, which does not even parse. What it
    // should name is whichever concrete class the factory redirects to, and
    // that is not known here.
    // `Object()` as an identity token -- a lock, a sentinel: a fresh unit
    // behind a handle, which `dyn Object` accepts (`_RenderObjectSemantics`,
    // 119 callers of a constructor refused for this).
    if (t.name == 'Object' && args.isEmpty) {
      return '(std::rc::Rc::new(()) as std::rc::Rc<dyn Object>)';
    }
    if (library.isAbstractType(t)) {
      throw Unsupported(
        'a constructor of `${t.name}`, which is abstract and became a trait',
        '${t.name}(..)',
      );
    }
    // `Pair::<i64, f32>::new(..)`, not `Pair<i64, f32>::new(..)`: in an
    // *expression* Rust wants the turbofish, and the plain form does not parse.
    // The *name*, not the type: a counted class's type is `Rc<Foo>` and its
    // constructor is `Foo::new`, which hands one out. Spelling the type here
    // wrote `Rc<Foo>::new()`, which does not parse.
    final counted = library.resolve(t)?.counted ?? false;
    final name = t.arguments.isEmpty
        // The bare name: `type(t)` of an argument-less `Map` fills in its
        // `Rc<dyn Object>` arguments, and `Map<K, V>::new()` needs a
        // turbofish to parse (`comparison operators cannot be chained`).
        ? (counted || type(t).contains('<') ? _spelled(t) : type(t))
        : '${_spelled(t)}::<${t.arguments.map((a) => type(a)).join(', ')}>';
    final ctor = _ctorName(constructor);
    return '$name::$ctor(${args.map(expr).join(', ')})';
  }
}
