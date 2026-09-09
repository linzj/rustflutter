part of '../backend_rust.dart';

// Null-aware chains, upcasts through an instantiation, static calls.
augment class RustBackend {
  String _nullAware(IrExpr receiver, IrExpr body, bool flatten) {
    final scalar = const {
      'int',
      'double',
      'num',
      'bool',
    }.contains(receiver.rustType?.name);
    final outer = _boundByValue;
    _boundByValue = scalar;
    try {
      // A mutating call on the bound value (`_cachedDryLayoutSizes?.
      // clear()`) acts on the place, borrowed mutably -- through the
      // cell the field is kept in, or the struct's own field -- not on
      // the clone a read takes (`_LayoutCacheStorage.clear`, run576).
      final mutating =
          body is IrCall &&
          body.target is IrBound &&
          _mutatesInPlace(body.name) &&
          !scalar;
      final cellPlace = mutating ? _mutPlace(receiver) : null;
      final ownPlace =
          mutating &&
              cellPlace == null &&
              receiver is IrField &&
              (receiver.target == null || receiver.target is IrThis) &&
              !_fieldsAreAccessors &&
              _selfName == 'self' &&
              _sharedField(receiver.name) == null
          ? '$_selfName.${snake(receiver.name)}'
          : null;
      final place =
          cellPlace ??
          ownPlace ??
          (mutating && receiver is IrLocal ? snake(receiver.name) : null);
      if (place != null) {
        return _failure == null
            ? '$place.as_mut().map(|$_boundName| ${expr(body)})'
            : '$place.as_mut().map(|$_boundName| -> Result<_, $_error> { Ok(${expr(body)}) }).transpose()?';
      }
      // The body's *value* being the binding itself -- a `?..` cascade,
      // whose steps mutate through the reference and whose result is the
      // object -- is a clone: `as_ref()` binds a `&T` and the slot takes
      // the `T` (`ImplicitlyAnimatedWidgetState.didUpdateWidget`, the
      // run's own panic at run780).
      // The receiver's `Option` shape, in every branch that spells one: a
      // projected `T?` is the associated type `<T as DartNullable>::Or`,
      // which is an `Option` only after the prelude converts it, and
      // `.as_ref()` on it resolved to no method at all
      // (`RestorableValue.value?.name`, `DiagnosticsProperty.value?.
      // toString()`, 3 at ws789).
      final plain = expr(_plain(receiver));
      if (!scalar && _endsAtBound(body)) {
        return _failure == null
            ? '$plain.as_ref().map(|$_boundName| ${_clonedBound(body)})'
            : '$plain.as_ref().map(|$_boundName| -> Result<_, $_error> '
                  '{ Ok(${_clonedBound(body)}) }).transpose()?';
      }
      final at = scalar ? '' : '.as_ref()';
      // The body's type spelled where it is known: an adapter closure
      // made in the body (`handler == null ? null : (m) async {..}` into
      // a `MessageHandler?` slot) unsizes against a spelled return and
      // not against an inferred `_` (ws486).
      // ..only for a closure body: the IR type of anything else is not
      // exact enough to spell as a return (`Infallible` for a body TFA
      // removed, `Option<()>` for a `void?`; +35 at ws487).
      final bodyType = body.rustType;
      final closureBody = _closureLike(body);
      if (Platform.environment['DART2RUST_TRACE_NULLAWARE'] == '1') {
        stderr.writeln(
          'TRACE_NULLAWARE body=${body.runtimeType} type=${body.rustType} closure=$closureBody',
        );
      }
      final spelled =
          closureBody &&
              bodyType != null &&
              bodyType.name != 'raw' &&
              !_mentionsUnknown(bodyType)
          ? type(bodyType)
          : '_';
      // ..and the *body*'s, when the two are flattened: a body handing
      // back a projected `T?` makes an `Option<<T as DartNullable>::Or>`,
      // which is not two `Option` layers and has no `flatten`
      // (`Provider._inheritedElementOf(context)?.value`, 3 at ws789).
      // What comes out is then the plain `Option<T>`, and this expression
      // is recorded as the projection its Dart type is: put back, so the
      // spelling and the recorded type agree and a reader of it does not
      // unproject a second time (`RawRadio.value`, `registry?.groupValue`,
      // 5 at ws798).
      final projectedBody = flatten && (body.rustType?.projected ?? false);
      final inner = expr(projectedBody ? _plain(body) : body);
      // The same spelling where the body cannot fail: there is no
      // `Result` to carry it, so the closure's own return says it. A
      // `FormField<T>`'s validator adapter is made here, and unsized
      // against nothing it stayed an `Rc<{closure}>` (2 at ws858).
      final annotated = spelled != '_' && !flatten ? ' -> $spelled' : '';
      final whole = _failure == null
          ? '$plain$at.${flatten ? 'and_then' : 'map'}'
                '(|$_boundName|$annotated ${annotated.isEmpty ? inner : '{ $inner }'})'
          : '$plain$at.map(|$_boundName| -> Result<$spelled, $_error> { Ok($inner) }).transpose()?${flatten ? '.flatten()' : ''}';
      return projectedBody
          ? '<${body.rustType!.name} as DartNullable>::from_option($whole)'
          : whole;
    } finally {
      _boundByValue = outer;
    }
  }

  IrExpr _plain(IrExpr e) {
    final t = e.rustType;
    if (t == null || !t.projected) return e;
    return IrNullableOf(e, t.name, toOption: true)
      ..rustType = IrType(t.name, nullable: true, arguments: t.arguments);
  }

  IrIfNull _plainIfNull(IrIfNull e) => e.left.rustType?.projected == true
      ? (IrIfNull(
          _plain(e.left),
          e.right,
          nullableResult: e.nullableResult,
          eager: e.eager,
        )..rustType = e.rustType)
      : e;

  /// The trait declaring `name` that `owner` implements at more than one
  /// instantiation (`IrClass.extraImpls`), or null: a plain call of such a
  /// method is ambiguous to rustc and is qualified by the class's own
  /// instantiation (`<Self as Tween<i64>>::begin(self)`).
  IrClass? _wideTraitFor(IrClass? owner, String name) {
    if (owner == null || owner.extraImpls.isEmpty) return null;
    for (final above in _abstractAncestors(owner)) {
      if (!owner.extraImpls.any((w) => w.name == above.name)) continue;
      final declares =
          above.methods.any((m) => m.name == name && !m.isStatic) ||
          above.abstractMethods.any((m) => m.name == name) ||
          above.fields.any((f) => f.name == name);
      if (declares) return above;
    }
    return null;
  }

  /// `<Recv<args> as Trait<traitArgs>>` for a call on another object
  /// whose class implements the trait more than once (`extraImpls`): a
  /// bare `Trait::m(&*x)` cannot say which (`RestorableProperty::dispose`
  /// on a `RestorableEnumN<Orientation>`, E0283 at ws627). Null when the
  /// plain spelling is unambiguous.
  String? _throughOwnInstantiation(
    IrExpr? target,
    String? receiverClass,
    String trait,
  ) {
    if (target == null || target is IrThis || receiverClass == null) {
      return null;
    }
    final owner = library[receiverClass];
    if (owner == null ||
        owner.isAbstract ||
        !owner.extraImpls.any((w) => w.name == trait)) {
      return null;
    }
    final base = library[trait];
    if (base == null) return null;
    final recv = target.rustType;
    final binding = <String, IrType>{
      if (recv != null)
        for (
          var i = 0;
          i < owner.typeParameters.length && i < recv.arguments.length;
          i++
        )
          owner.typeParameters[i]: recv.arguments[i],
    };
    final passed = _argumentsThrough(owner, binding, base, {});
    if (passed == null ||
        passed.any((a) => owner.typeParameters.contains(a.name))) {
      return null;
    }
    final self = recv != null && recv.arguments.isNotEmpty
        ? '${owner.name}<${recv.arguments.map((a) => type(a)).join(', ')}>'
        : owner.name;
    final args = passed.isEmpty
        ? ''
        : '<${passed.map((a) => type(a)).join(', ')}>';
    return '<$self as $trait$args>';
  }

  /// `<Self as Trait<args>>` for a trait this class implements more than
  /// once, `Trait` otherwise: what a qualified call on `self` has to say.
  String _implementedAs(String trait) {
    final base = library[trait];
    if (base == null || !cls.extraImpls.any((w) => w.name == trait)) {
      return trait;
    }
    final args = _baseArguments(base) ?? '';
    final self = _inSuperFn ? '__Self' : 'Self';
    return '<$self as $trait$args>';
  }

  /// One element of a list literal: the first with its upcast spelled, a
  /// bare closure behind an `Rc` where the list holds functions.
  String _listElement(int index, IrExpr e, IrType element) {
    final first = index == 0 ? _explicitUpcast(e) : e;
    return element.isFunction && first is IrClosure && !first.boxed
        ? 'std::rc::Rc::new(${expr(first)})'
        : expr(first);
  }

  /// The reborrow a lending local function's closure moves in place of a
  /// `&mut self` it cannot copy.
  static const _lentSelf = '__mut_me';

  /// Set while a lending local function's closure is printed.
  var _lendingClosure = false;

  /// Whether a `return` in the body being printed has to spell its upcast:
  /// set for a step closure of an iterator chain, whose return type Rust
  /// reads off the body rather than from a slot.
  var _spellsReturn = false;

  /// An implicit upcast made explicit, through any `Some` around it: the
  /// first element of a `vec![..]` decides the `Vec`'s type.
  IrExpr _explicitUpcast(IrExpr e) => switch (e) {
    IrUpcast(:final value, :final type, :final handle, :final explicit)
        when !explicit =>
      IrUpcast(value, type, handle: handle, explicit: true)
        ..rustType = e.rustType,
    IrSome(:final value) => IrSome(
      _explicitUpcast(value),
    )..rustType = e.rustType,
    // A function item behind an `Rc` is not yet the `Rc<dyn Fn>` its slot
    // holds; spelled where nothing else will unsize it.
    IrFunctionRef() when e.rustType?.isFunction ?? false => IrLiteral(
      '(${expr(e)} as ${type(e.rustType!)})',
      e.rustType!,
    )..rustType = e.rustType,
    _ => e,
  };

  /// An operand under a borrow: its implicit upcast is spelled, since
  /// unsizing does not happen behind a `&` (`map.get(&Rc::new(key))` was
  /// a `&Rc<String>` where `&Rc<dyn Object>` was wanted, 19 at ws425).
  String _borrowed(IrExpr e) => expr(_explicitUpcast(e));

  /// `::<A, B>` for a call's type arguments; nothing when there are none.
  String _turbofish(List<IrType> typeArguments) =>
      typeArguments.isEmpty ? '' : '::<${typeArguments.map(type).join(', ')}>';

  String _staticCall(
    String? owner,
    String name,
    List<IrExpr> args, [
    List<IrType> typeArguments = const [],
  ]) {
    final fish = _turbofish(typeArguments);
    // `Future.value(v)` is a future that is already done, which Rust spells
    // `ready`. `Future.delayed` and `Future.wait` need a runtime to be delayed
    // or joined *by*, and there is none, so they say so.
    // ..the prelude has one now (`SCHEDULER`, `dart_spawn`): the other
    // constructors are its functions (2026-09-05, the runtime ruler's
    // first refusal after `main`: `Future<bool>(() async {..})`).
    if (owner == 'Future') {
      if (name == 'value' && args.length <= 1) {
        // The type spelled: `Future<void>.value()` alone left `T` to
        // inference (E0283, ws462).
        // ..and no value is the projected `None` of that type (`()` for
        // a `Future<void>`), since the slot is `<T as DartNullable>::Or`.
        if (args.isEmpty) return 'future_none$fish()';
        return 'future_value$fish(${expr(args.single)})';
      }
      if ((name == '' || name == 'new') && args.length == 1) {
        return 'future_new(${expr(args.single)})';
      }
      if (name == 'microtask' && args.length == 1) {
        return 'future_microtask(${expr(args.single)})';
      }
      if (name == 'sync' && args.length == 1) {
        return 'future_sync(${expr(args.single)})';
      }
      // With its type argument, as `value` has it: without a computation
      // there is nothing to infer `T` from (`Future<void>.delayed(Duration
      // .zero)` fell back to the never type, the thenvoid fixture).
      if (name == 'delayed' && (args.length == 1 || args.length == 2)) {
        return 'future_delayed$fish(${expr(args[0])}, ${args.length == 2 ? expr(args[1]) : 'None'})';
      }
      if (name == 'error' && args.isNotEmpty) {
        return 'DartFuture::ready(Err(${expr(args[0])}))';
      }
      if (name == 'wait' && args.isNotEmpty) {
        return 'future_wait(${expr(args[0])})';
      }
      throw Unsupported(
        '`Future.$name`, which needs an executor',
        'Future.$name(..)',
      );
    }
    // `int.parse` and `double.parse`. Dart's throw on bad input and its
    // `tryParse` returns null, which is `ok()`; `unwrap()` keeps the throw
    // loud rather than turning it into a zero.
    if (owner == 'int' || owner == 'double') {
      final rust = owner == 'int' ? 'i64' : 'f64';
      if (name == 'parse' && args.length == 1) {
        return '${expr(args.single)}.parse::<$rust>().unwrap()';
      }
      if (name == 'tryParse' && args.length == 1) {
        return '${expr(args.single)}.parse::<$rust>().ok()';
      }
      throw Unsupported('`$owner.$name`', '$owner.$name(..)');
    }
    // The runtime's own list classes reached as statics, the way
    // `_GrowableList.filled` is. Same names as the constructors, same answer.
    if ((_collections[owner] == 'Vec' || owner == 'List') &&
        _listStatics.contains(name)) {
      // `List.generate(n, f, growable: ..)`: the flag changes nothing for
      // a `Vec` (`HashedObserverList.toList` under `notifyListeners`,
      // run634).
      if (name == 'generate' && (args.length == 2 || args.length == 3)) {
        // `map` wants the closure itself, not the `Rc<dyn Fn>` a function
        // parameter would (E0277 in `plural_rules`); a function *value*
        // is called through one.
        final generator = args[1];
        final rendered = expr(generator);
        const boxed = 'std::rc::Rc::new(';
        // A closure renders boxed when it captures (`Rc::new({ let x =
        // x.clone(); move |i| .. })`); `map` wants the closure itself.
        String unboxed(String r) => r.startsWith(boxed) && r.endsWith(')')
            ? r.substring(boxed.length, r.length - 1)
            : r;
        final f =
            generator is IrCall &&
                generator.name == '!rc' &&
                generator.args.isEmpty
            ? unboxed(expr(generator.target!))
            : generator is IrClosure || rendered.startsWith(boxed)
            ? unboxed(rendered)
            : '|__i| ($rendered)(__i)';
        // The generator returns `Result`: the collection does too, and the
        // `?` the name rule appends unwraps it (69 `?` on a `Vec`).
        return _resultModel
            ? '(0..${expr(args[0])}).map($f).collect::<Result<Vec<_>, $_error>>()'
            : '(0..${expr(args[0])}).map($f).collect::<Vec<_>>()';
      }
      if (name == 'filled' && args.length == 2) {
        return 'vec![${expr(args[1])}; ${expr(args[0])} as usize]';
      }
      // ..and `List.unmodifiable(xs)`: a copy that nothing here writes to.
      if ((name == 'from' || name == 'of' || name == 'unmodifiable') &&
          args.length == 1) {
        return '${expr(args[0])}.clone()';
      }
      // `List.from(xs, growable: false)`: a `Vec` is always growable and a
      // copy is a copy; the flag changes nothing that can be said here.
      if ((name == 'from' || name == 'of' || name == 'unmodifiable') &&
          args.length == 2) {
        return '${expr(args[0])}.clone()';
      }
      if (name == 'empty' && args.isEmpty) return 'Vec::new()';
      throw Unsupported(
        '`$owner.$name` with ${args.length} arguments',
        '$owner.$name(..)',
      );
    }
    // `Float64List(9)`: a typed list of a length is that many zeros, which
    // is what Dart gives it. The untyped `_List(n)` is a list of *nulls*
    // and is handled in the front end; these cannot hold null at all.
    if (_typedLists.contains(owner) && name.isEmpty && args.length == 1) {
      // A typed zero: `Default::default()` left the element to inference,
      // and a `.map(|v| v as i64)` after it had nothing to go on (E0282).
      const zero = {
        'Float32List': '0.0f32',
        'Float64List': '0.0f64',
        'Int8List': '0i8',
        'Int16List': '0i16',
        'Int32List': '0i32',
        'Int64List': '0i64',
        'Uint8List': '0u8',
        'Uint8ClampedList': '0u8',
        'Uint16List': '0u16',
        'Uint32List': '0u32',
        'Uint64List': '0u64',
      };
      return 'vec![${zero[owner]}; ${expr(args.single)} as usize]';
    }
    // `Uint8List.fromList(xs)`: a typed list *is* a `Vec` here, so a copy.
    if (_typedLists.contains(owner) && name == 'fromList' && args.length == 1) {
      // `Float32List.fromList(doubles)`: a `Vec<f64>` narrowed element by
      // element (E0308 `Vec<f32>` vs `Vec<f64>` in `_MatrixImageFilter`).
      // The 64-bit ones are already what a `List<double>`/`List<int>` is.
      const element = {
        'Float32List': 'f32',
        'Int8List': 'i8',
        'Int16List': 'i16',
        'Int32List': 'i32',
        'Uint8List': 'u8',
        'Uint8ClampedList': 'u8',
        'Uint16List': 'u16',
        'Uint32List': 'u32',
        'Uint64List': 'u64',
      };
      final narrow = element[owner];
      if (narrow != null) {
        return '${expr(args.single)}.iter().map(|v| *v as $narrow).collect::<Vec<$narrow>>()';
      }
      return '${expr(args.single)}.clone()';
    }
    if (owner == null) {
      // `identityHashCode(x)`: Dart's hash of the object's *identity*, which
      // here is the address behind the handle -- the same address
      // `identical` compares (`_identical`) and the same one `Expando` keys
      // by. Only a handle has one: a value class is copied into each slot,
      // so two copies of one Dart object would answer differently, and the
      // call stays refused there, as `identical` on one does (ws878).
      if (name == 'identityHashCode' && args.length == 1) {
        final only = args.single;
        // A handle, an absent-or-handle, or the object every `Object` and
        // `dynamic` slot holds. `_handleLike` is the wrong question here:
        // it says no to a `dynamic` on purpose (a narrowed call is recorded
        // one while its value is a scalar), and `Object value` is what
        // `GlobalObjectKey` and `ObjectKey` hash.
        bool identityBearing(IrExpr e) {
          final t = e.rustType;
          if (t == null || t.isFunction) return false;
          if (t.name == 'Object' || t.name == 'dynamic') return true;
          return library.isAbstract(t.name) ||
              (library[t.name]?.counted ?? false);
        }

        if (only is IrThis
            ? cls.counted || cls.isAbstract
            : identityBearing(only)) {
          return '(${_handleOf(only)}).dart_identity_hash_code()';
        }
      }
      // A top-level function: no owner in either language. Checked against
      // what this file emits, for the same reason a static call is -- a call
      // to something refused would name a function nobody wrote.
      if (!library.functions.any((f) => f.name == name) &&
          !library.functionsElsewhere.contains(name) &&
          !_preludeFunctions.contains(name)) {
        throw Unsupported(
          'call to top-level `$name`, which was not translated',
          '$name(...)',
        );
      }
      // A bare local is cloned in, as a translated callee's argument is
      // by the front end: `dart_str(font_family)` moved a parameter the
      // constructor read again (`TextStyle`, ws525).
      String passed(IrExpr a) =>
          a is IrLocal &&
              !_cellLocals.containsKey(a.name) &&
              !_closureCaptured.contains(a.name)
          ? '${expr(a)}.clone()'
          : expr(a);
      return '${snake(name)}$fish(${args.map(passed).join(', ')})';
    }
    // An **unnamed factory** is a `Procedure` whose name is the empty string,
    // and Kernel calls it like a static: `RegExp('..')` arrives as
    // `_staticCall('RegExp', '', ..)`. Its Rust name is the one every unnamed
    // constructor gets. Without this it reached the operator table and said
    // "operator `` has no Rust name" -- 367 times, naming neither the member
    // nor where it came from.
    if (name.isEmpty) {
      // The runtime's own collections first: `[]` is `_GrowableList(0)` in
      // Kernel, and the unnamed-factory rule below would spell that
      // `_GrowableList::new(0)` -- a module Rust has never heard of, 129
      // times. A length other than zero is a list of nulls, which is not an
      // empty `Vec`, so it is refused rather than flattened to one.
      final collection = _collections[owner];
      if (collection != null) {
        final empty =
            args.isEmpty ||
            (args.length == 1 &&
                args.single is IrLiteral &&
                (args.single as IrLiteral).value == '0');
        // `vec![]` for a list, which is what the list-literal path already
        // writes. One thing, one spelling: the two front ends reach an empty
        // list by different routes -- Kernel through `_GrowableList(0)` and
        // the analyzer through a literal -- and a fixture that compares text
        // sees any difference at all.
        if (empty) {
          return collection == 'Vec' ? 'vec![]' : '$collection::new()';
        }
        throw Unsupported(
          '`$owner` with a length, which is a list of nulls',
          '$owner(..)',
        );
      }
      // A factory of an abstract class -- `Characters(s)` -- is the static
      // named `new` of the trait's, a free function (see below); the struct
      // spelling `Characters::new` named a trait as a type.
      if (library.isAbstract(owner)) {
        return '${_abstractStaticName(owner, 'new')}(${args.map(expr).join(', ')})';
      }
      // `dart:async`'s `Completer` records where it was made, so a run
      // stuck on a future nobody completes can say whose (run466: "0
      // future(s) still pending").
      if (owner == 'Completer' && args.isEmpty) {
        return 'Completer::new_named("$_here")';
      }
      return '$owner::${_ctorName(null)}(${args.map(expr).join(', ')})';
    }
    // `dart:io`'s `Platform` statics are the prelude's functions whether
    // upstream spells them as fields (`isMacOS`, read through
    // `_staticRead`) or as getters (`environment`, a static call here:
    // google_fonts' `isTest`, run605).
    if (owner == 'Platform' && args.isEmpty) {
      return 'platform_${snake(name)}()';
    }
    final target = library[owner];
    if (target != null &&
        !target.methods.any(
          (m) =>
              (m.name == name || _methodName(m) == name) && m.operator == null,
        )) {
      throw Unsupported(
        'call to `$owner.$name`, which was not translated',
        '$owner.$name(...)',
      );
    }
    if (owner == 'Object' && name == 'hashAll' && args.length == 1) {
      return 'object_hash_all(${expr(args.single)})';
    }
    if (owner == 'Object' && name == 'hash') {
      return 'object_hash(${args.map(expr).join(', ')})';
    }
    // `library.isAbstract`, not `library[owner]?.isAbstract`: an abstract
    // class of another module is in `abstractElsewhere` and nowhere else
    // (`Characters::new(..)` -- "expected a type, found a trait").
    if (_freeStatics(owner) && (name.isNotEmpty || library.isAbstract(owner))) {
      // A *factory* on an abstract class -- `Characters(s)` -- is the static
      // named `new` here, as the struct path names an unnamed constructor.
      final spelled = name.isEmpty ? 'new' : name;
      return '${_abstractStaticName(owner, spelled)}$fish(${args.map(expr).join(', ')})';
    }
    return '$owner::${_identifier(name)}$fish(${args.map(expr).join(', ')})';
  }
}
