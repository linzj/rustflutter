part of '../backend_rust.dart';

// Type tests, and the place a value is read from or mutated through.
augment class RustBackend {
  /// `dyn Foo<A, B>`: the trait object a trait-typed `IrType` names.
  String _dynOf(IrType t) => t.arguments.isEmpty
      ? 'dyn ${_spelled(t)}'
      : 'dyn ${_spelled(t)}<${t.arguments.map(type).join(', ')}>';

  String _isTest(IrExpr operand, IrType target, bool negated) {
    final name = target.name;
    // `x is C` on a nullable `x`: null is no `C` (a non-nullable one), so
    // the test is asked of the value inside the `Option` and answers
    // `false` for `None` -- `_asAny` unwrapped it and panicked on the
    // null `Color?` `CupertinoDynamicColor.maybeResolve` is given
    // (run619). `is Object?`/`dynamic` below know about null themselves.
    // Only a *plain* read of an `Option` (`_optionRead`): a promoted
    // read is recorded nullable while its value is already unwrapped
    // (`border` under `border is Border`, `CupertinoTextField.build`,
    // ws620). Matched by value, so that the inner is the handle itself and
    // not a borrow the test would have to keep (`_maybeAddKey`, ws620).
    final held = operand.rustType;
    if (held != null &&
        _optionRead(operand) != null &&
        !target.nullable &&
        name != 'Object' &&
        name != 'dynamic') {
      final inner = IrLocal('__v')..rustType = stripNull(held);
      final test = _isTest(inner, target, negated);
      return '(match ${expr(operand)}.clone() { Some(__v) => $test, None => $negated })';
    }
    // `x is Future` where `x` is a `FutureOr<T>`: the prelude spells Dart's
    // sum as an enum of its two cases, so the question is which case the
    // value holds. Not the rule below: the blanket `runtime_type` of a
    // `FutureOr` reports the sum itself (`FutureOr`), so asking it named
    // no future and answered `false` for one (ws853).
    final sum = operand.rustType;
    if (sum != null &&
        sum.name == 'FutureOr' &&
        name == 'Future' &&
        library[name] == null) {
      final read = _optionRead(operand) ?? expr(operand);
      final test = 'matches!(&$read, FutureOr::Future(_))';
      return negated ? '!$test' : test;
    }
    // A prelude *generic* class answers `is` by the runtime type it
    // reports: a `DartFuture<T>` is a `Future` whatever `T` is, and a
    // downcast would have to name the one instantiation it was boxed as.
    // Only for a test with no type arguments -- `x is Future<int>` asks
    // about `T`, which this cannot answer, and stops as before
    // (`SynchronousFuture.then`'s `result is Future`, 2 at ws850).
    // ..`x is Future` is `Future<dynamic>` by the time it gets here, so a
    // top argument counts as none.
    final preludeGeneric = _preludeGenerics[name];
    if (library[name] == null &&
        preludeGeneric != null &&
        target.arguments.every(
          (a) => a.name == 'dynamic' || a.name == 'Object',
        )) {
      final read = _optionRead(operand) ?? expr(operand);
      final test = '($read.runtime_type().name == "$preludeGeneric")';
      return negated ? '!$test' : test;
    }
    // `x is R Function(..)`: the function object keeps the handle it was
    // made from, whose Rust type is this signature -- a downcast, not a
    // guess at the arity (the prelude's `dart_is_function_of`). A bare
    // `Function` is any of them. Both are the prelude's, so a translated
    // class of the same name is left alone.
    if (library[name] == null && (target.isFunction || name == 'Function')) {
      final read = _optionRead(operand) ?? expr(operand);
      final test = target.isFunction && target.parameters != null
          ? 'dart_is_function_of::<dyn Fn('
                '${target.parameters!.map((p) => type(p, owned: false)).join(', ')}'
                ') -> ${_wrapped(type(target.returns!))}>(&$read)'
          : 'dart_is_function(&$read)';
      return negated ? '!$test' : test;
    }
    // A type parameter: whatever the caller passed for it, asked by id
    // (`dart_cast_any`). `ancestor.state is T` in `findAncestorStateOfType`,
    // refused as "`is` against `T`" since the first round.
    if (_isTypeParam(name)) {
      return '${expr(operand)}.dart_cast_any::<$name>()'
          '.${negated ? "is_none" : "is_some"}()';
    }
    // A trait: asked of the object itself (`dart_cast`), which knows what
    // it implements. Refused since the first round (`_isTest`).
    if (library.isAbstract(name)) {
      return '${expr(operand)}.dart_cast_to::<${_dynOf(target)}>()'
          '.${negated ? "is_none" : "is_some"}()';
    }
    // `x is num` / `is int` / `is String` on a `dynamic`: the prelude's
    // scalar types, asked of `Any`. A `num` is either an `f64` or an `i64`.
    const scalars = {
      'int': ['i64'],
      'double': ['f64'],
      'num': ['f64', 'i64'],
      'bool': ['bool'],
      'String': ['String'],
    };
    // `is Object` holds of every value but null, `is Object?` of every
    // value: the pattern `final Object? value` a switch's last case
    // binds is lowered to one (`_updateUserSettingsData`, run472).
    if (name == 'Object' || name == 'dynamic') {
      final always = target.nullable || operand.rustType?.nullable != true;
      if (always) return negated ? 'false' : 'true';
      return '${expr(operand)}.${negated ? "is_none" : "is_some"}()';
    }
    if (scalars.containsKey(name)) {
      final tests = scalars[name]!
          .map((t) => '${_asAny(operand)}.downcast_ref::<$t>().is_some()')
          .join(' || ');
      return negated ? '!($tests)' : '($tests)';
    }
    // `x is Map` / `is List` / `is Set` / `is Iterable` on a `dynamic`: the
    // prelude's collections are generic structs, and `Any` cannot ask for
    // "some `Map<_, _>`"; their runtime type names can (`dart_is_kind`,
    // get's `_isNullOrEmpty`, run489).
    const collections = {
      'Map': ['Map'],
      'List': ['Vec'],
      'Set': ['Set'],
      'Queue': ['VecDeque', 'Queue'],
      'Iterable': ['Vec', 'Set', 'VecDeque', 'Queue'],
    };
    final kinds = collections[name];
    if (kinds != null && library[name] == null) {
      final test =
          'dart_is_kind(&${expr(operand)}, &[${kinds.map((k) => '"$k"').join(', ')}])';
      return negated ? '!$test' : test;
    }
    // `x is Uint8List`: the typed lists are `Vec`s of their element here
    // (the front end's `_narrowElement`), asked of `Any` exactly
    // (`StandardMessageCodec.writeValue`, run504).
    const typedData = {
      'Float32List': 'Vec<f32>',
      'Float64List': 'Vec<f64>',
      'Int8List': 'Vec<i8>',
      'Int16List': 'Vec<i16>',
      'Int32List': 'Vec<i32>',
      'Int64List': 'Vec<i64>',
      'Uint8List': 'Vec<u8>',
      'Uint8ClampedList': 'Vec<u8>',
      'Uint16List': 'Vec<u16>',
      'Uint32List': 'Vec<u32>',
      'Uint64List': 'Vec<u64>',
    };
    final typed = typedData[name];
    if (typed != null && library[name] == null) {
      return '${_asAny(operand)}.downcast_ref::<$typed>()'
          '.${negated ? "is_none" : "is_some"}()';
    }
    // A prelude class answers `is` through `Any` like a translated one:
    // every `'static` type is an `Object` there (`is StateError` in
    // `BindingBase._initListenable`, run433).
    // ..through the prelude's own hierarchy (`DartCoreAs`): its exception
    // structs are unrelated to Rust, and a `RangeError` was no
    // `ArgumentError` to `Any` (fixture oncatch).
    if (library[name] == null && _preludeClasses.contains(name)) {
      final test =
          '<$name as DartCoreAs>::dart_core_as(&${_optionRead(operand) ?? expr(operand)}).is_some()';
      return negated ? '!$test' : test;
    }
    if (library[name] == null) {
      throw Unsupported('`is` against `$name`, which was not translated', name);
    }
    // `x is C<dynamic>` against a translated generic struct: true of every
    // instantiation, which `Any` cannot ask for (`UninitializedLocaleData<
    // DateSymbols>` never was an `UninitializedLocaleData<Rc<dyn Object>>`,
    // and intl's date symbols stayed uninitialised, run587); the runtime
    // type's name can, as for the prelude's collections above.
    final generic = library[name];
    if (generic != null &&
        !generic.isAbstract &&
        generic.typeParameters.isNotEmpty &&
        target.arguments.isNotEmpty &&
        target.arguments.every(
          (a) => a.name == 'dynamic' || a.name == 'Object',
        )) {
      final test = 'dart_is_kind(&${expr(operand)}, &["$name"])';
      return negated ? '!$test' : test;
    }
    final arguments = target.arguments.isEmpty
        ? ''
        : '<${target.arguments.map(type).join(', ')}>';
    return '${_asAny(operand)}'
        '.downcast_ref::<$name$arguments>().${negated ? "is_none" : "is_some"}()';
  }

  /// Whether a value of this recorded type is a handle: an `Rc<dyn Trait>`,
  /// a `dynamic`, a counted class's `Rc<Struct>`.
  /// Not a `dynamic`: the blanket `as_any` looks through an `Rc<dyn
  /// Object>` itself, and a call on a `dynamic` the type flow analysis
  /// narrowed (`x.isNegative` on an `f64`) is recorded `dynamic` while its
  /// value is a plain `bool` (intl's `_floor`, ws522).
  bool _handleLike(IrExpr e) {
    final t = e.rustType;
    if (t == null || e is IrThis || t.isFunction || isNullable(t)) return false;
    return library.isAbstract(t.name) || (library[t.name]?.counted ?? false);
  }

  /// A place as `&mut`: a local by name (through its cell when it has
  /// one), a field of `this` through its cell, anything else as a
  /// temporary the callee fills and nobody reads.
  /// The parameters of the body being written that are lent places
  /// (`IrParam.mutRef`): lent on again as a reborrow.
  Set<String> _mutRefParams = const {};

  String _mutRef(IrExpr place) {
    if (place is IrLocal) {
      if (_mutRefParams.contains(place.name)) {
        return '&mut *${snake(place.name)}';
      }
      final cell = _cellLocals[place.name];
      if (cell == null) return '&mut ${snake(place.name)}';
      return cell
          ? '&mut ${snake(place.name)}'
          : '&mut *${snake(place.name)}.borrow_mut()';
    }
    if (place is IrField && (place.target == null || place.target is IrThis)) {
      // Inside a trait body a field is an accessor, and its cell is what
      // the trait hands out (`_cellPlace`) -- the same spelling a *read* of
      // it takes. Lent as a plain field it was `__me._incoming_items`,
      // which is a method there and not a field ("attempted to take value
      // of method", E0615: `_SliverAnimatedMultiBoxAdaptorState`'s
      // `insertItem`/`removeItem`, ws973). The closures inside a super
      // function are trait bodies too, which is where these two sit.
      if (_fieldsAreAccessors) {
        final through = _cellPlace(place);
        if (through != null) return '&mut *$through.borrow_mut()';
      }
      final shared = _sharedField(place.name);
      if (shared != null && !_isCopy(_heldDecl(shared))) {
        return '&mut *$_selfName.${snake(place.name)}.borrow_mut()';
      }
      if (shared == null) return '&mut $_selfName.${snake(place.name)}';
    }
    // A mutable static lent to a callee that fills it (`fill(log)` on a
    // top-level list, the statmut fixture): through its cell.
    if (place is IrTopLevel && _isMutableTopLevel(place.name)) {
      return '&mut *${screamingSnake(place.name)}.get()$_propagate.borrow_mut()';
    }
    if (place is IrStatic &&
        !place.isEnumValue &&
        _isMutableStatic(place.owner, place.name)) {
      return '&mut *${_lazyName(place.owner, place.name)}.get()$_propagate.borrow_mut()';
    }
    return '&mut ${expr(place)}';
  }

  /// `x.as_any()` for a downcast or an `is`: through the handle when `x` is
  /// one. The blanket `Object` on the `Rc` itself answers with the
  /// *handle's* `Any` -- an `Rc<dyn Widget>`, never a `RootWidget` -- so
  /// `widget is RootWidget` was always false and `RootElement.mount`
  /// unwrapped a `None` (run521). `this` and a value are asked directly.
  /// A plain read of an `Option` (a local, a field, a map lookup), taken
  /// out of it: the value a cast asks about (`_availableSkeletons[
  /// inputPattern]` as a `String`, a `Map<dynamic, dynamic>` read,
  /// run596). A projected `T?` is no `Option`, and a promoted read's
  /// recorded type is the declaration's while its value is already
  /// unwrapped (`tween_super_lerp`, `CupertinoTextField.build`, ws597).
  /// Null when the read is not such a value.
  String? _optionRead(IrExpr e) {
    final t = e.rustType;
    final plain =
        e is IrLocal ||
        e is IrField ||
        e is IrTopLevel ||
        e is IrStatic ||
        e is IrIndex ||
        (e is IrCall &&
            (e.name == '!map_get' ||
                (e.name == 'clone' &&
                    (e.target is IrLocal ||
                        e.target is IrField ||
                        e.target is IrIndex))));
    if (plain &&
        t != null &&
        isNullable(t) &&
        !t.projected &&
        e is! IrThis &&
        !t.isFunction) {
      return '${expr(e)}.clone().unwrap()';
    }
    return null;
  }

  /// The `<..>` of a downcast target. A collection written *raw* -- Dart's
  /// bare `List` is a `List<dynamic>` -- reaches here with no arguments at
  /// all, and `downcast_ref::<Vec>()` is no Rust type ("missing generics
  /// for struct `Vec`", E0107; `PredictiveBackEvent.fromMap` casts a
  /// platform message's field `as List?`, ws961). What a raw collection
  /// holds is `dynamic`, so that is what is spelled.
  static const _collectionArity = {'List': 1, 'Set': 1, 'Map': 2};

  String _downcastArguments(String name, List<IrType> arguments) {
    if (arguments.isNotEmpty) {
      return '<${arguments.map(type).join(', ')}>';
    }
    final arity = _collectionArity[name];
    if (arity == null) return '';
    final spelled = type(const IrType('dynamic'));
    return '<${List.filled(arity, spelled).join(', ')}>';
  }

  /// Whether an expression never returns: the one line Dart's AOT compiler
  /// proved dead (`IrLiteral.unreachable`), reached through the block that
  /// leads to it. `coerce.dart` asks the same question of a value; this is
  /// the backend's copy, for the places that build an expression *around*
  /// one.
  ///
  /// By the literal's text, not by identity with `IrLiteral.unreachable`:
  /// the flattening pass rebuilds the tree, so the instance that reaches
  /// the backend is a copy (`expressions.dart` reads it the same way where
  /// it drops a binding of one).
  static bool _neverReturns(IrExpr? e) =>
      (e is IrLiteral && e.value.startsWith('unreachable!')) ||
      (e is IrBlockValue && _neverReturns(e.value));

  String _asAny(IrExpr e) {
    final read = _optionRead(e);
    if (read != null) {
      final t = e.rustType!;
      final held = library[t.name];
      final handle = held != null && (held.isAbstract || held.counted);
      return '$read${handle ? '.as_ref()' : ''}.as_any()';
    }
    return _handleLike(e)
        ? '${expr(e)}.as_ref().as_any()'
        : '${expr(e)}.as_any()';
  }

  /// Whether `name` is a field of this struct's own, not in a cell, whose
  /// Rust type is one of the prelude's collections by value.
  bool _ownCollectionField(String name) {
    if (_sharedField(name) != null) return false;
    final decl = cls.fields.where((f) => f.name == name).firstOrNull;
    if (decl == null || decl.type.nullable) return false;
    // By the IR's name: a typed list (`Uint8List`) is a prelude alias of
    // its `Vec` and spells as one.
    return const {
      'List',
      'Vec',
      'Iterable',
      'Map',
      'Set',
      'Queue',
      'Int8List',
      'Int16List',
      'Int32List',
      'Int64List',
      'Uint8List',
      'Uint8ClampedList',
      'Uint16List',
      'Uint32List',
      'Uint64List',
      'Float32List',
      'Float64List',
      'ByteData',
    }.contains(decl.type.name);
  }

  /// The receiver of a field read or a call.
  ///
  /// `this` is two different things in Rust depending on where it stands. As a
  /// *value* it is `*self`, a copy of the struct -- that is what `return this;`
  /// wants. As the *target* of a field or a call it is `self`, because `*self.x`
  /// parses as `*(self.x)` and dereferences the field instead of the receiver.
  ///
  /// Upstream's `copyWith` is where this surfaced: `left ?? this.left` became
  /// `left.unwrap_or(*self.left)`, which does not compile. It was found by
  /// building real upstream code rather than a fixture, which is the argument
  /// for keeping real code in the test crate.
  /// The return type of the function currently being emitted.
  ///
  /// Needed for one thing Dart does implicitly and Rust does not: returning a
  /// concrete value where an abstract type is declared.
  /// `AlignmentGeometry.add` ends in `_MixedAlignment(...)` and is declared to
  /// return `AlignmentGeometry`, which in Rust is `Box<dyn AlignmentGeometry>`.
  /// That is the same coercion the trait impls needed at their boundary, met
  /// again inside a body.
  IrType? _returns;

  /// Whether the method being emitted is `async`, for the constructs that
  /// must not wrap an `.await` in a closure.
  var _asyncBody = false;

  /// The type parameters of the method being emitted: a name among them
  /// is a Rust type parameter, not a class (`_isTest`, `IrCastTo`).
  var _methodTypeParams = const <String>[];

  bool _isTypeParam(String name) =>
      _methodTypeParams.contains(name) || cls.typeParameters.contains(name);

  /// Wraps a returned expression when the declared return is a trait object.
  ///
  /// Only an `IrNew` is wrapped, because only a constructor call is *known* to
  /// produce that concrete type. Anything else could already be a box, and a
  /// double `Box::new` compiles into something quietly wrong.
  String _returned(IrExpr value) {
    final declared = _returns;
    // `this` returned where a handle of this class goes is the object's own
    // handle, not a copy: inside an operator `self` is the value `std::ops`
    // fixed, and `return this` gave an `AttributedString` where
    // `Rc<AttributedString>` was declared (`operator +`, 7 at ws756).
    if (value is IrThis && declared != null && !declared.isFunction) {
      final held = library[declared.name];
      if (declared.name == cls.name &&
          (held?.counted ?? false) &&
          !isNullable(declared)) {
        final own = _thisHandle();
        if (own != null) return own;
      }
    }
    // A closure whose return type Rust reads off its body -- a step of an
    // iterator chain, which no slot expects a type from -- is not a
    // coercion site, so an implicit upcast there left the chain collecting
    // the concrete element (`Vec<Rc<Sq>>` where `Vec<Rc<dyn Shape>>` was
    // declared, the mapret fixture; 2 at ws808).
    final text = expr(_spellsReturn ? _explicitUpcast(value) : value);
    // A closure returned from a function is an *owned* position, and a
    // closure's own type has no name -- so the declared type is
    // `Box<dyn Fn(..)>` and the value has to be boxed to match. This only
    // came up once closures that outlive their call stopped being refused.
    if (declared != null && declared.isFunction && value is IrClosure) {
      return 'std::rc::Rc::new($text)';
    }
    // `dynamic` and `Object` are trait objects too (`Rc<dyn Object>`):
    // `error = Exception(..)` into a `dynamic` local needs the same `Rc::new`.
    if (declared != null &&
        (library.isAbstract(declared.name) ||
            declared.name == 'dynamic' ||
            declared.name == 'Object') &&
        (value is IrNew || value is IrConstInstance) &&
        !library.isAbstract(_concreteType(value).name) &&
        // A counted class's constructor already hands out an `Rc`, which
        // unsizes on its own; wrapping it again was `Rc<Rc<X>>`.
        !(library[_concreteType(value).name]?.counted ?? false)) {
      // Registered for `dart_cast_to` through `dyn Object` on the way.
      // ..with the cast spelled: an `if` arm has no expected type of its
      // own where the `let` is unannotated, and two arms of different
      // classes did not unify (`ThemeData`'s `splashFactory`, ws523).
      final spelled = isNullable(declared) ? null : type(declared);
      return spelled == null
          ? 'dart_object($text)'
          : '(dart_object($text) as $spelled)';
    }
    // Each branch of a conditional on its own: `s.isEmpty ? StringCharacters
    // ("") : StringCharacters(s)` returned as a `Characters`.
    // ..each with its implicit upcast spelled, as `expr`'s conditional
    // does: an arm has no expected type of its own under an unannotated
    // `let` (the `??=` temporary holding `ThemeData`'s `splashFactory`,
    // three const classes into one trait, run564).
    if (value is IrConditional) {
      return 'if ${expr(value.condition)} { ${_returned(_explicitUpcast(value.then))} } '
          'else { ${_returned(_explicitUpcast(value.otherwise))} }';
    }
    // ..and any other value that is not an object yet. The branch above
    // knows two ways to make one (`IrNew`, `IrConstInstance`); a *call* is
    // a third, and the front end narrows `n.abs()` on a `dynamic` receiver
    // to `f64`'s method, so what is in hand there is an `f64` while the
    // slot holds an `Rc<dyn Object>` (`NumberFormat.format` and
    // `_formatFixed`, ws895). Only where the value says what it is: an
    // expression with no recorded type is left alone, as it was.
    if (declared != null &&
        (declared.name == 'dynamic' || declared.name == 'Object') &&
        declared.arguments.isEmpty &&
        !isNullable(declared)) {
      final have = value.rustType;
      if (have != null &&
          have.name != 'dynamic' &&
          have.name != 'Object' &&
          !have.isFunction) {
        final adapted = coerceInto(value, declared, _world);
        if (!identical(adapted, value)) return expr(adapted);
      }
    }
    return text;
  }

  IrType _concreteType(IrExpr e) => switch (e) {
    IrNew(:final type) => type,
    IrConstInstance(:final type) => type,
    _ => const IrType('void'),
  };

  /// What `self` is called in the code currently being emitted.
  ///
  /// A free function has no `self`, so while one is being written the receiver
  /// is its first parameter instead.
  String _selfName = 'self';

  /// The member whose body is being printed, `Class.member`, for runtime
  /// diagnostics that name their creator (`Completer::new_named`).
  String _here = '';

  /// Whether `self` is held by value (an `std::ops` operator's body).
  var _selfByValue = false;

  /// Rust names of the collection methods that change their receiver: every
  /// Dart mutator that has a Rust method of the same name, snaked, plus the
  /// prelude's and `Vec`'s own (`member_names.dart`).
  static final Set<String> _inPlace = {
    for (final name in mutatingNames)
      if (!noRustMutatorNames.contains(name)) snake(name),
    ...mutatingRustOnlyNames,
  };

  // ..by either spelling: a prelude collection's method arrives under its
  // Dart name (`addAll`) and is snaked at the call (`add_all`, ws578).
  static bool _mutatesInPlace(String name) =>
      _inPlace.contains(name) || _inPlace.contains(snake(name));

  /// The place a mutating call acts on, borrowed mutably: a cell's
  /// `borrow_mut()`, and through a promoted read (`_sizes!.add(..)`,
  /// `_sizes![k] = v`) the value inside it (`as_mut().unwrap()`); null
  /// when the target has no such place (the nullmut fixture, ws577).
  String? _mutPlace(IrExpr? target, {bool anyHeld = false}) {
    // A read's clone is the place it read.
    if (target is IrCall &&
        target.name == 'clone' &&
        target.args.isEmpty &&
        target.target != null) {
      return _mutPlace(target.target!);
    }
    // A value a collection *holds* is mutated where the collection keeps
    // it: `m[k]!.add(v)` and `xs[i].add(v)` reach the set and the list
    // inside, and reading one out took a copy -- the call compiled and
    // changed nothing. `_groupIdToRegions[region.groupId]!.add(region)`
    // left every group empty; the emptied group was then dropped as
    // "empty", and the next unregistration found no key at all
    // (`RenderTapRegionSurface`, run787).
    final held = _heldSlot(target);
    if (held != null) return held;
    // `(f ??= <>{}).add(x)`: the value handed back is a copy of what the
    // field holds, and mutating it changes nothing. The place is the left
    // side's, once the right side has made sure something is there
    // (`IrIfNull.assignsLeft`).
    if (target is IrIfNull && target.assignsLeft) {
      final place = _mutPlace(target.left);
      if (place != null) {
        return '{ if ${expr(target.left)}.is_none() { ${expr(target.right)}; } '
            '$place.as_mut().unwrap() }';
      }
    }
    if (target is IrNullCheck) {
      final inner = _mutPlace(target.operand);
      if (inner != null) {
        return '$inner.as_mut().ok_or_else(dart_null_check_failed)$_propagate';
      }
      // ..a plain local `Option<..>` promoted: the local itself.
      var operand = target.operand;
      if (operand is IrCall &&
          operand.name == 'clone' &&
          operand.args.isEmpty &&
          operand.target != null) {
        operand = operand.target!;
      }
      if (operand is IrLocal && !_cellLocals.containsKey(operand.name)) {
        return '${snake(operand.name)}.as_mut()'
            '.ok_or_else(dart_null_check_failed)$_propagate';
      }
      return null;
    }
    final cell = _cellPlace(target, anyHeld: anyHeld);
    if (cell == null) return null;
    // A `late` field's cell holds an `Option`: the value inside it
    // (`ObserverList._set.clear()`, ws577).
    if (target is IrField) {
      final atThis = target.target == null || target.target is IrThis;
      final decl = atThis
          ? _lateField(target.name)
          : target.owner == null
          ? null
          : _cellFieldOf(target.owner!, target.name);
      if (decl != null && decl.isLate) {
        return '$cell.borrow_mut().as_mut()${_lateRead(target.name)}';
      }
    }
    return '$cell.borrow_mut()';
  }

  /// The place a collection keeps one of its values in, borrowed mutably.
  ///
  /// Dart's `[]` hands back the object the collection holds; a `Vec` and a
  /// `Map` here hand back a value, and a clone of it is not the collection's.
  /// A map's `[]` is a `V?`, so the shape is the `!` the Dart wrote --
  /// `get_mut` answers the same absence.
  /// Null when the collection itself has no place (a call's result, a
  /// parameter read): nothing can be mutated in place there.
  String? _heldSlot(IrExpr? read) {
    if (read is IrCall &&
        read.name == 'clone' &&
        read.args.isEmpty &&
        read.target != null) {
      return _heldSlot(read.target!);
    }
    if (read is IrNullCheck) {
      var inner = read.operand;
      if (inner is IrCall &&
          inner.name == 'clone' &&
          inner.args.isEmpty &&
          inner.target != null) {
        inner = inner.target!;
      }
      if (inner is IrCall &&
          inner.name == '!map_get' &&
          inner.args.length == 1 &&
          inner.target != null) {
        final place = _collectionPlace(inner.target!);
        return place == null
            ? null
            : '$place.get_mut(&${_borrowed(inner.args.single)})'
                  '.ok_or_else(dart_null_check_failed)$_propagate';
      }
      return null;
    }
    if (read is IrIndex) {
      final place = _collectionPlace(read.target);
      // The index first, as `IrIndexSet` takes it: `xs[self.i()]` inside a
      // `borrow_mut()` would borrow the same cell twice.
      return place == null ? null : '$place[${expr(read.index)} as usize]';
    }
    return null;
  }

  /// `m[k]` as an `Option<&mut V>`, for a null-aware call that mutates what
  /// the map holds.
  ///
  /// `_heldSlot` answers the null-*asserted* shape, `m[k]!.add(v)`, whose
  /// place is a value: `get_mut(..).unwrap()`. The null-*aware* one is a
  /// different type -- `m[k]?.remove(v)` has to keep the absence, so the
  /// place is the `Option` `get_mut` already hands back and the caller maps
  /// over it directly instead of adding `.as_mut()`
  /// (`_childrenToAdd[child.restorationId]?.remove(child)` in
  /// `RestorationBucket._removeChildData`, which mutated a copy of the list
  /// and left the bucket's own untouched).
  String? _optionHeldSlot(IrExpr? read) {
    if (read is IrCall &&
        read.name == 'clone' &&
        read.args.isEmpty &&
        read.target != null) {
      return _optionHeldSlot(read.target!);
    }
    if (read is IrCall &&
        read.name == '!map_get' &&
        read.args.length == 1 &&
        read.target != null) {
      final place = _collectionPlace(read.target!);
      return place == null
          ? null
          : '$place.get_mut(&${_borrowed(read.args.single)})';
    }
    return null;
  }

  /// A collection as a place: a cell's `borrow_mut()`, one of this struct's
  /// own fields, or a local.
  String? _collectionPlace(IrExpr collection) {
    final cell = _mutPlace(collection);
    if (cell != null) return cell;
    if (collection is IrField &&
        (collection.target == null || collection.target is IrThis) &&
        !_fieldsAreAccessors &&
        (_selfName == 'self' || _selfName == '__new') &&
        _ownCollectionField(collection.name)) {
      return '$_selfName.${snake(collection.name)}';
    }
    if (collection is IrLocal && !_cellLocals.containsKey(collection.name)) {
      return snake(collection.name);
    }
    return null;
  }

  /// The collection members whose answer does not need the collection --
  /// a length, an emptiness, one value out of a map -- read through the
  /// cell rather than through a clone of the whole thing.
  String? _borrowedRead(IrExpr? target, String name, List<IrExpr> args) {
    const noArgument = {'len', 'is_empty', '!is_empty', 'keys', 'values'};
    if (!noArgument.contains(name) &&
        !(name == '!map_get' && args.length == 1)) {
      return null;
    }
    if (args.isNotEmpty && name != '!map_get') return null;
    final place = _readPlace(target);
    if (place == null) return null;
    if (name == '!map_get') {
      // The key by reference, as the ordinary emission takes it: bound to a
      // local it was *moved*, and a caller reading it again afterwards had
      // nothing left (`SlottedContainerRenderObjectMixin._setChild`, ws800).
      return '({ let __r = $place.get(&${_borrowed(args.single)}).cloned()'
          '${_flattenedValue(target)}; __r })';
    }
    // `length` is a `usize` here and an `int` in Dart, as the ordinary
    // emission spells it.
    final call = switch (name) {
      '!is_empty' => '!$place.is_empty()',
      'len' => '($place.len() as i64)',
      _ => '$place.$name()',
    };
    return '({ let __r = $call; __r })';
  }

  /// The place a read goes through, borrowed shared: `_mutPlace`'s other
  /// half. Null when the target is not kept in a cell.
  String? _readPlace(IrExpr? target) {
    if (target is IrCall &&
        target.name == 'clone' &&
        target.args.isEmpty &&
        target.target != null) {
      return _readPlace(target.target!);
    }
    final cell = _cellPlace(target);
    if (cell == null) return null;
    // A `late` field's cell holds an `Option`: the value inside it.
    if (target is IrField) {
      final atThis = target.target == null || target.target is IrThis;
      final decl = atThis
          ? _lateField(target.name)
          : target.owner == null
          ? null
          : _cellFieldOf(target.owner!, target.name);
      if (decl != null && decl.isLate) {
        return '$cell.borrow().as_ref()${_lateRead(target.name)}';
      }
    }
    return '$cell.borrow()';
  }

  /// The cell a field read would go through, as a place -- `self.x` or
  /// `other.x` -- when the field is kept in a `RefCell`; null otherwise.
  /// Whether the method `name` on a receiver of type `holder` is one this
  /// compiler gives `&mut self`.
  ///
  /// The question `_mutatesInPlace` approximates by name, answered instead
  /// from the callee's own declaration -- the same two tests `_receiverOf`
  /// makes for the class being emitted, asked about another class: a
  /// counted class takes `&self` because its fields are in cells, and
  /// otherwise the receiver is `&mut self` exactly when the method writes a
  /// field (`_mutating`) or shares a trait signature with one that does.
  ///
  /// This is what lets a `?.` on a cell field tell `dragEnd`, which mutates
  /// the controller, from `reverse` on a held `AnimationController`, which
  /// does not mutate the cell -- the distinction the collection test in
  /// `_cellPlace` was standing in for.
  bool _mutatesSelf(IrType? holder, String name) {
    if (holder == null) return false;
    final other = library[holder.name];
    if (other == null || other.counted) return false;
    final method = other.methods
        .where((m) => m.name == name && !m.isStatic)
        .firstOrNull;
    if (method == null) return false;
    return _sharedMutation(method) ||
        _mutatingOf(other).contains(_rustName(method));
  }

  String? _cellPlace(IrExpr? target, {bool anyHeld = false}) {
    // A local in a cell (`IrLocalDecl.cell`: captured and changed in a
    // closure): the cell itself, whose `borrow_mut()` the call takes. The
    // value read cloned it and `seen.add(..)` pushed into the clone (the
    // lend2 fixture, ws544).
    // ..a collection in the cell: a handle's method that shares a
    // mutator's name (`controller.reverse()` on an `AnimationController`
    // local) is not a mutation of the local (3 at ws546).
    if (target is IrLocal &&
        _cellLocals[target.name] == false &&
        target.rustType != null &&
        _isMutableCollection(type(target.rustType!))) {
      return snake(target.name);
    }
    // A mutable static's cell (`LazyLock<Isolate<RefCell<..>>>`), when it
    // holds a collection: the read was a clone, and `log.add(..)` filled
    // the clone (the supermix fixture, ws576).
    if (target is IrTopLevel && _isMutableTopLevel(target.name)) {
      final held =
          library.constants
              .where((c) => c.name == target.name)
              .firstOrNull
              ?.type ??
          library.constantsElsewhere[target.name]?.type;
      if (held != null && _isMutableCollection(type(held))) {
        return '${screamingSnake(target.name)}.get()$_propagate';
      }
    }
    if (target is IrStatic &&
        !target.isEnumValue &&
        _isMutableStatic(target.owner, target.name)) {
      final held = library[target.owner]?.constants
          .where((c) => c.name == target.name)
          .firstOrNull
          ?.type;
      if (held != null && _isMutableCollection(type(held))) {
        return '${_lazyName(target.owner, target.name)}.get()$_propagate';
      }
    }
    // A hollow mixin's field is read through the declaration's abstract
    // getter -- an accessor *call* on `this` in the trait body -- where the
    // application holds the field (`IrClass.appliedFields`): the same
    // place as the field read (`_viewIdToRenderView[id] = view` in
    // `RendererBinding.addRenderView`, ws532).
    if (target is IrCall &&
        target.args.isEmpty &&
        target.typeArguments.isEmpty &&
        (target.target == null || target.target is IrThis) &&
        _fieldsAreAccessors &&
        _appliedFieldOf(cls, target.name) != null) {
      return _cellPlace(IrField(target.target, target.name));
    }
    // ..and on a *handle* whose class is a trait: the front end reads a
    // trait's field through its accessor (`owner!._nodesNeedingLayout`),
    // whose value is a clone -- `scheduleInitialLayout` pushed the root
    // into the copy and no layout ever ran (run656). The trait hands the
    // cell out too (`_handsCell`).
    if (target is IrCall &&
        target.args.isEmpty &&
        target.typeArguments.isEmpty &&
        target.target != null &&
        target.target is! IrThis) {
      final held = target.target!.rustType;
      final owned = held == null || isNullable(held)
          ? null
          : library[held.name];
      if (owned != null && library.isAbstract(held!.name)) {
        final decl =
            _allFields(owned).where((f) => f.name == target.name).firstOrNull ??
            _appliedFieldOf(owned, target.name);
        if (decl != null && _handsCell(decl)) {
          return '${expr(target.target!)}.${snake(target.name)}_cell()$_propagate';
        }
      }
    }
    if (target is! IrField) return null;
    final base = target.target;
    final atThis = base == null || base is IrThis;
    // Through the trait's cell accessor: inside a trait body, or on a
    // handle whose owner is a trait (`cascaded.children.add(x)`).
    final owner = target.owner;
    if ((atThis && _fieldsAreAccessors) ||
        (!atThis && owner != null && library.isAbstract(owner))) {
      final owned = atThis ? cls : library[owner!];
      final decl = owned == null
          ? null
          : _allFields(owned).where((f) => f.name == target.name).firstOrNull ??
                _appliedFieldOf(owned, target.name);
      if (decl == null || !_handsCell(decl)) return null;
      // `this` as the accessor's `&self`: inside a closure it is the
      // handle `__me`, dereferenced (`ListNotifierMixin::_updaters_cell(
      // __me)` handed the `Rc`, ws578).
      final through = atThis
          ? _accessorQualifier(target.name, kind: 'cell')
          : null;
      // The dereferenced handle only where it is an argument (`Trait::
      // x_cell(&*__me)`): as a method receiver it auto-derefs, and `&*`
      // in front of the whole chain dereferenced the `remove(..)` result
      // instead (`SliverMultiBoxAdaptorElement.createChild`'s closure,
      // ws670).
      final holder = atThis
          ? (through == null ? _selfName : (_addressOf(IrThis()) ?? _selfName))
          : expr(base);
      return through == null
          ? '$holder.${snake(target.name)}_cell()$_propagate'
          : '$through::${snake(target.name)}_cell($holder)$_propagate';
    }
    final IrFieldDecl? cell;
    if (base == null || base is IrThis) {
      cell = _sharedField(target.name);
    } else if (target.owner != null) {
      cell = _cellFieldOf(target.owner!, target.name);
    } else {
      cell = null;
    }
    if (cell == null ||
        _fieldIsCopy(
          cell,
          base == null || base is IrThis
              ? cls
              : (target.owner == null ? null : library[target.owner!]),
        )) {
      return null;
    }
    // Only a collection is mutated through the cell: `reverse` on an
    // `Rc<RefCell<Option<Rc<AnimationController>>>>` is the controller's
    // method, not `Vec::reverse` (51 in `widgets`).
    // ..the same set every other in-place site uses: a typed list is an
    // alias of its `Vec` and spelled by name, and `WriteBuffer._add`'s
    // `_buffer[i] = b` went into a clone -- every platform message was
    // 35 zero bytes (run512).
    // ..unless the caller has already established that the *callee* takes
    // `&mut self`. The collection test stands in for that question, because
    // the gate above it is a name test and `reverse` on a held
    // `AnimationController` is the controller's method rather than
    // `Vec::reverse`. Where the callee is known, the stand-in is not needed,
    // and refusing here is what left `x?.dragEnd(0)` with no place at all.
    final held = _heldType(cell);
    if (!anyHeld && !_isMutableCollection(held)) return null;
    final holder = base == null || base is IrThis ? _selfName : expr(base);
    return '$holder.${snake(target.name)}';
  }

  String _receiver(IrExpr? target) {
    if (target == null || target is IrThis) return _selfName;
    // `local.field.method(..)`: the field is the place the method acts on,
    // not the clone a value read takes.
    if (target is IrField && target.target is IrLocal && target.owner == null) {
      return '${expr(target.target!)}.${snake(target.name)}';
    }
    // A receiver is not a coercion site: an implicit upcast under it, even
    // through a `Some`, is spelled (`Some(dart_object(EdgeInsets {..}))
    // .clone()` into an `Option<Rc<dyn EdgeInsetsGeometry>>`, 26 at ws426).
    return expr(_explicitUpcast(target));
  }

  /// Whether a value of this class is held as an `Rc`: a counted class, or
  /// an abstract one (`Rc<dyn ..>`).
  bool _isHandle(String? className) {
    final c = library[className];
    return c != null && (c.counted || c.isAbstract);
  }

  /// The prelude's methods that take a callback and so return `Result`
  /// themselves (see `DartError` there).
  static const _preludeFailing = {
    // `Stream.listen` calls the Dart closures it is handed, and a Dart
    // closure can throw; their slots say `-> Result` because every
    // closure this compiler emits does.
    'listen',
    // `convert` on every prelude converter: a `Converter` runs a Dart
    // closure, and the two fixed ones (`JsonUtf8Encoder`, `Utf8Decoder`)
    // return `Result` to match (`JSONMessageCodec.decodeMessage`, ws506).
    'convert',
    'put_if_absent',
    // `map.update`: the callback's failure comes out, and so does the
    // `ArgumentError` for a key that is not there with no `ifAbsent`.
    'update',
    'for_each',
    'sort_by_dart',
    'first_where',
    'first_where_or',
    'last_where',
    'last_where_or',
    // `fold`/`reduce`/`indexWhere`/`skipWhile`/`takeWhile`: the combine's
    // or the test's failure comes out, as `firstWhere`'s does (ws810).
    'fold_dart',
    'reduce_dart',
    'index_where',
    'skip_while_dart',
    'take_while_dart',
    // `Map.map`: the transform's failure comes out (ws811).
    'map_entries',
    // `replaceAllMapped`: the callback's failure comes out (ws811).
    'replace_all_mapped',
    // `removeWhere`/`retainWhere`: the test's failure comes out.
    'remove_where',
    'retain_where',
    // Not `then`: the prelude's returns the future it spawns, and the
    // callback's own failure lands in that future (`_initKeyboard`, run476).
    'run',
    'run_guarded',
    'run_unary_guarded',
    'run_unary',
    // `first`/`last`/`single` on an empty iterable, `reduce` on one, and
    // `fill_range(.., null)` on a list of non-nullables: Dart throws for
    // each, and the prelude says `Result` for each since ws1076.
    'first',
    'last',
    'single',
    'fill_range',
    // `LinkedListEntry.insertAfter`/`insertBefore` on an entry that is in
    // no list: Dart's `StateError`.
    'insert_after',
    'insert_before',
    // `Queue.removeFirst()`/`removeLast()` on an empty queue.
    'remove_first',
    'remove_last',
    // `dart:io`'s synchronous file calls. Dart throws `FileSystemException`
    // from every one of them and `on FileSystemException catch` is how
    // programs read a missing file; the prelude used to `panic!` through a
    // `raise(self) -> !` that no longer exists (ws1077).
    'create_sync',
    'open_sync',
    'close_sync',
    'length_sync',
    'read_as_bytes_sync',
    'read_as_string_sync',
    'write_as_string_sync',
    'write_as_bytes_sync',
    'delete_sync',
    'position_sync',
    'set_position_sync',
    'read_sync',
    'read_into_sync',
    'write_from_sync',
    'write_string_sync',
    'write_byte_sync',
    'truncate_sync',
    'flush_sync',
    // `Error.throwWithStackTrace(e, st)`, which is a `throw`.
    'throw_with_stack_trace',
  };

  /// The prelude methods whose callback parameter is `impl Fn`: it is
  /// called and dropped, never kept. The rest (`remove_where`, `update`,
  /// `put_if_absent`) declare an `Rc<dyn Fn>` and take the handle.
  static const _preludeLends = {
    'time_sync',
    'first_where',
    'first_where_or',
    'last_where',
    'last_where_or',
    'index_where',
    'fold_dart',
    'reduce_dart',
    'skip_while_dart',
    'take_while_dart',
    'map_entries',
    'remove_where',
    'retain_where',
    'for_each',
    'put_if_absent',
  };

  /// A function value at one of those slots: the function behind an `Rc`
  /// `coerce` added, or a loan of the handle. A closure is already one.
  static IrExpr _lentFunction(IrExpr a) {
    // A closure written at the call site is the closure -- unboxed, since
    // an `Rc<dyn Fn>` is no `impl Fn`.
    if (a is IrClosure) {
      return a.boxed
          ? (IrClosure(
              a.params,
              a.body,
              a.returns,
              captures: a.captures,
              locals: a.locals,
              holdsSelf: a.holdsSelf,
              isAsync: a.isAsync,
            )..rustType = a.rustType)
          : a;
    }
    final t = a.rustType;
    if (t == null || !t.isFunction) return a;
    // `Rc::new(f)` -> `f`: a function item is an `impl Fn` already.
    if (a is IrCall && a.name == '!rc' && a.args.isEmpty && a.target != null) {
      return a.target!;
    }
    // ..and a handle is lent: `&dyn Fn(..)` implements `Fn(..)`.
    return IrCall(a, '!fn_ref', const [])..rustType = t;
  }

  /// ..and its static functions.
  /// `runZonedGuarded` under both spellings: this set is asked with the
  /// Dart name at a top-level call and with the snake one elsewhere, and
  /// `_invoke1_with_return` beside it is already the snake spelling.
  static const _preludeFailingStatics = {
    // `int.parse`/`double.parse`: Dart's `FormatException` (ws1076).
    'dart_parse_int',
    'dart_parse_int_radix',
    'dart_parse_double',
    'generate',
    '_invoke1_with_return',
    'runZonedGuarded',
    'run_zoned_guarded',
  };

  /// `?` when a function surrounds the expression, `.unwrap()` otherwise.
  String get _propagate => _failure != null ? '?' : '.unwrap()';

  /// A read of a `late` field or local that has no value yet.
  ///
  /// Dart throws `LateInitializationError` there, and it is an `Error`:
  /// `try { .. } catch (e)` around a `late` read is ordinary Dart. This
  /// was `.unwrap()`, which is a panic -- a path the program still had,
  /// and the process died instead of taking it.
  ///
  /// The `Option` a `late` is held in is the one place that can tell, so
  /// this is appended wherever that `Option` is opened. `fallible` is for
  /// the trait accessors, which are written outside any method body and
  /// carry no `_failure` of their own -- they return `Result` when the
  /// result model is on, and say so themselves.
  /// [kind] is Dart's own word for what went unwritten -- it says "Field
  /// 'x' has not been initialized." for a field and "Local 'x' .." for a
  /// local, and the message is the only thing a `catch` can read.
  String _lateRead(String name, {bool? fallible, String kind = 'Field'}) =>
      '.ok_or_else(|| dart_late_init_failed("$kind", "$name"))'
      '${(fallible ?? _failure != null) ? '?' : '.unwrap()'}';

  /// Set while the operand of an `await` is printed: the call's own `?`
  /// belongs after the `.await`.
  bool _awaiting = false;
}
