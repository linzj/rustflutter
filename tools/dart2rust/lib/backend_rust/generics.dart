part of '../backend_rust.dart';

// Type parameters, the bounds they carry, and what is `Copy`.
augment class RustBackend {
  /// A function used as a value, behind the handle every function slot
  /// is. An `async` function's item returns its future bare, where a
  /// function value returns `Result` like everything else: a closure
  /// around it puts the `Ok` on (`registerServiceExtension(callback:
  /// _exitApplication)`, run453).
  String _functionRef(String? owner, String name, IrType? type) {
    final path = owner == null
        ? snake(name)
        : _freeStatics(owner)
        ? _abstractStaticName(owner, name)
        : '$owner::${snake(name)}';
    final target = owner == null
        ? library.functions.where((f) => f.name == name).firstOrNull
        : library[owner]?.methods
              .where((m) => m.name == name && m.isStatic)
              .firstOrNull;
    if (target == null || !target.isAsync) return 'std::rc::Rc::new($path)';
    // The parameters and the error spelled, as a closure literal's are:
    // nothing else infers them behind the `Rc` (E0282, ws454).
    final params = type?.parameters ?? [for (final p in target.params) p.type];
    final args = [for (var i = 0; i < params.length; i++) '__a$i'];
    final spelled = [
      for (var i = 0; i < params.length; i++)
        '${args[i]}: ${this.type(params[i], owned: params[i].name == 'Future' || params[i].isFunction)}',
    ].join(', ');
    return 'std::rc::Rc::new(|$spelled| -> Result<_, $_error> { Ok($path(${args.join(', ')})) })';
  }

  /// Whether a class's statics live at module level under the class's
  /// name: an abstract class is a trait and has nowhere else to put them;
  /// a *generic* class's `impl<T> Foo<T>` would make every static call
  /// name a `T` the static never mentions (`RadioGroup.maybeOf<T>()`, 12
  /// "cannot infer type" at ws397).
  bool _freeStatics(String owner) =>
      library.isAbstract(owner) ||
      (library[owner]?.typeParameters.isNotEmpty ?? false);

  /// `<T>` for a class or method that has parameters, and nothing otherwise.
  /// Whether the struct derives `Clone`: nothing it holds is a bare future.
  bool _cloneable(IrClass of) =>
      !_allFields(of)
          .any((f) => _fieldType(f).contains('dyn std::future::Future'));

  /// A class's parameters as a use: `<T, U>`, or nothing.
  String _useArguments(IrClass of) =>
      of.typeParameters.isEmpty ? '' : '<${of.typeParameters.join(', ')}>';

  /// `clone` puts `Clone` on a class's parameters. Off by default: a
  /// declaration -- struct, trait, the marker impls -- needs no bound to
  /// exist, and one there is demanded wherever the type is *named* (a
  /// trait accessor returning `Vec<TweenSequenceItem<T>>` under `T:
  /// 'static`, ws303). The impl blocks whose bodies clone ask for it
  /// themselves (`_implGenerics`, the super functions, `_traitWhere`).
  String _generics(Object owner, {bool static = false, bool clone = false}) {
    final params = switch (owner) {
      IrClass(:final typeParameters) => typeParameters,
      IrMethod(:final typeParameters) => typeParameters,
      _ => const <String>[],
    };
    if (params.isEmpty) return '';
    // `&dyn Any` is `&dyn Any + 'static`, so a generic struct can only hand
    // one out when its parameters outlive the borrow. Nothing this compiler
    // emits holds a borrow, so the bound costs nothing and is not written
    // anywhere else.
    // A method's own parameters carry what a body needs of them, as an
    // impl's do (`_implGenerics`): `listEquals<T>` clones its `Option<Vec<T>>`
    // (9 "trait bounds were not satisfied" in dart:ui).
    // One bound for every declaration -- struct, trait, impl, method, super
    // function: a trait's default method calls the super function with the
    // trait's own `E`, so the trait has to promise what the function asks
    // (147 E0277s from asking it of the function alone).
    // A method's own parameter keeps `Clone`: `binarySearch<T>` clones
    // its `T` (148 ".clone on T"), and nothing instantiates a method's
    // parameter with a future. A class's does not (`_CallbackHookProvider<
    // Future<bool>>`), see `bound` in `_boundedGenerics`.
    // ..and the trait its Dart bound names (`IrMethod.typeParameterBounds`),
    // so the body can call the bound's members on it.
    // Not the trait its Dart bound names: the erased twin instantiates
    // the parameter with `Rc<dyn Object>` and a handle is not the trait
    // (+252 at ws544). A member of the bound is reached through the
    // object instead (`_receiver`'s narrowing, the Object protocol).
    final bound = owner is IrMethod
        ? params.map((p) => "$p: Clone${_nbm(owner)} + 'static")
        : static
        // `Clone` on a class's parameters after all (ws301): every held
        // `T` is read by `.clone()`, and 240 stubs said so; the one shape
        // that is not `Clone`, a bare future, is measured against that.
        ? params.map(
            (p) => clone
                ? "$p: Clone${owner is IrClass ? _nbp(owner, p) : ''} + 'static"
                // `Clone` on a trait's parameters too: a `Vec<E>` is
                // `DartAny` only for a `Clone` element now that a list
                // answers a cast to its dynamic form (`_UnorderedEquality<
                // E>: Equality<Vec<E>>`, ws595).
                : "$p: Clone + DartNullable<Or: Clone + DartEq + FromDynamic + DartAny> + DartEq + FromDynamic + DartAny${owner is IrClass && owner.enumParameters.contains(p) ? ' + DartEnum' : ''} + 'static",
          )
        : params;
    return '<${bound.join(', ')}>';
  }

  /// Dart's `toString()` of a collection element `__e` (a reference into
  /// the collection) typed `element`: a string is itself, a number or a
  /// bool prints as it is, a translated class by the Object protocol
  /// (`DartAny::dart_to_string`), a nullable one `null` or that, and the
  /// rest by what the object knows (`dart_object_str`). `dart_str` on
  /// every element put quotes around each string (ws535) and `Instance
  /// of 'Vec'` around a list (ws543).
  String _elementText(IrType? element) {
    if (element == null) return 'dart_object_str(__e.clone())';
    if (element.nullable) {
      final inner = _elementText(
        IrType(element.name, arguments: element.arguments),
      );
      return '__e.as_ref().map(|__e| $inner).unwrap_or_else(|| "null".to_string())';
    }
    return switch (element.name) {
      'String' => '__e.clone()',
      'f64' => 'dart_double_str(*__e)',
      'i64' || 'bool' => '__e.to_string()',
      final name when library[name] != null => '__e.dart_to_string()',
      _ => 'dart_object_str(__e.clone())',
    };
  }

  /// Whether the value never arrives: a throw, the AOT compiler's dead
  /// line, a block ending in one. Rust types it `!`, which no trait
  /// bound accepts, so a generic boxing names its type parameter.
  bool _diverges(IrExpr value) =>
      identical(value, IrLiteral.unreachable) ||
      value is IrThrowValue ||
      value.rustType?.name == 'Never' ||
      (value is IrBlockValue && _diverges(value.value)) ||
      // ..and inside the `Some` a nullable slot puts on: an arm TFA
      // removed is wrapped before it is an arm, so the conditional read
      // as one that arrives and its `None` was left to the never-type
      // fallback (`hashCode` over a `List<Shadow>?` the tree shaker
      // emptied, 3 at ws868).
      (value is IrSome && _diverges(value.value));

  /// The turbofish `dart_boxed` needs where nothing else says the type:
  /// `Null` for a value that never arrives, a literal collection's own
  /// (`vec![1, 2]` boxed bare is a `Vec<i32>`, printed `Instance of
  /// 'int'`), nothing otherwise.
  String _boxedAs(IrExpr value) {
    if (_diverges(value)) return '::<Null>';
    final own = switch (value) {
      IrListLiteral(:final element) when !_mentionsUnknown(element) =>
        'Vec<${type(element)}>',
      IrMapLiteral(:final key, :final value)
          when !_mentionsUnknown(key) && !_mentionsUnknown(value) =>
        'Map<${type(key)}, ${type(value)}>',
      _ => null,
    };
    return own == null ? '' : '::<$own>';
  }

  /// Whether a type names `Never` anywhere in it.
  static bool _mentionsNever(IrType t) =>
      t.name == 'Never' ||
      t.arguments.any(_mentionsNever) ||
      (t.isFunction &&
          ((t.parameters ?? const []).any(_mentionsNever) ||
              (t.returns != null && _mentionsNever(t.returns!))));

  /// A value on its way behind a fresh handle: an `int` literal with its
  /// `i64` suffix, anything else as it is.
  String _boxedLiteral(IrExpr value) {
    final text = expr(value);
    if (value is IrLiteral &&
        value.type.name == 'int' &&
        RegExp(r'^-?[0-9]+$').hasMatch(text)) {
      return '${text}i64';
    }
    return text;
  }

  /// A top-level constant whose Rust type has a destructor, kept as a
  /// lazily built `static` rather than a `const`.
  bool _isLazyConst(String name) {
    for (final c in library.constants) {
      if (c.name == name) return _lazy(c);
    }
    final other = library.constantsElsewhere[name];
    if (other != null) return _lazy(other);
    return false;
  }

  /// A `const` only when Rust can evaluate the initialiser at compile
  /// time: a `Copy` value built from literals. `"0".codeUnitAt(0)` is an
  /// `i64` and still a call (E0015).
  bool _lazy(IrConstDecl c) =>
      !c.isMutable && (!_isCopy(type(c.type)) || !_constEvaluable(c.value));

  /// Prelude classes whose constructors are not `const fn`.
  static const _preludeTypes = {
    'Stopwatch',
    'DateTime',
    'Duration',
    'Completer',
    'StringBuffer',
    'RegExp',
    'Uri',
    'Random',
    'Expando',
    'Zone',
    'Map',
    'Set',
    'Queue',
    'Stream',
  };

  bool _constEvaluable(IrExpr e) => switch (e) {
    IrLiteral() => true,
    IrStatic() => true,
    IrTopLevel() => true,
    IrBinary(:final left, :final right) =>
      _constEvaluable(left) && _constEvaluable(right),
    IrUnary(:final operand) => _constEvaluable(operand),
    IrCast(:final value) => _constEvaluable(value),
    IrConstInstance(:final fields) => fields.values.every(_constEvaluable),
    // A translated class's constructor is a `const fn`; the prelude's
    // (`Stopwatch::new()`) are not, and a `const` holding one does not
    // compile (E0015 in `foundation_print`).
    // ..and only a `const` constructor is one: `SpringDescription.
    // withDampingRatio` takes a square root, and the `final` top-level
    // holding one was emitted as a `const` (E0015, `animation`).
    // Under the Result model a constructor call is a `Result`, which no
    // `const` item can unwrap: every such initialiser is lazy.
    IrNew(:final type, :final args, :final constructor) =>
      !_resultModel &&
          !_preludeTypes.contains(type.name) &&
          _constConstructor(type.name, constructor) &&
          args.every(_constEvaluable),
    _ => false,
  };

  bool _constConstructor(String className, String? name) {
    final c = library[className];
    if (c == null) return true;
    final k = c.constructors.where((k) => k.name == name);
    return k.isEmpty || k.first.isConst;
  }

  /// Whether every translated class named in a type text can be compared.
  bool _comparableType(String rust, Set<String> seen) {
    if (rust.contains('dyn Fn') || rust.contains('dyn std::future::Future'))
      return false;
    for (final name in _namesIn(rust)) {
      final other = library[name];
      if (other == null || !seen.add(name)) continue;
      if (other.isAbstract) continue;
      for (final f in _allFields(other)) {
        // A closure field compares by address in the manual `PartialEq`
        // the struct gets (see `byIdentity`), so it does not make the
        // class incomparable: `Vec<PointerData>` in `PointerDataPacket`.
        if (f.type.isFunction) continue;
        // A *projected* field leaves the class with no `PartialEq` at all:
        // a derive cannot write `<T as DartNullable>::Or: PartialEq`, which
        // is why `comparable` refuses to derive over one -- and unless the
        // class also has a handle field there is no manual impl either. So
        // a holder of such a class cannot compare it with `==`
        // ("binary operation `==` cannot be applied to `Option<Inner<T>>`",
        // E0369, the `heldgenericeq` fixture, ws981).
        if (f.type.projected) return false;
        if (!_comparableType(_fieldType(f), seen)) return false;
      }
    }
    return true;
  }

  /// A trait named as a bound: `Foo<T>`, not the `Rc<dyn Foo<T>>` a value
  /// of it is.
  String _traitPath(IrType t) => t.arguments.isEmpty
      ? t.name
      : '${t.name}<${t.arguments.map((a) => type(a)).join(', ')}>';

  /// The generics of an `impl` block: every parameter `Clone + DartNullable<Or: Clone> + 'static`.
  ///
  /// A method body clones what it reads (`self._map.clone()`), and a
  /// `Map<K, V>` is `Clone` only when `K` and `V` are; an `Rc<dyn ..>` held
  /// in a `T` slot wants `'static`. 30 "trait bounds were not satisfied"
  /// and 12 E0310s in `collection`. The struct and trait declarations stay
  /// unbounded, so a type argument that is neither is still a type -- only
  /// its methods are missing, which is loud where it matters.
  String _implGenerics(IrClass cls, {bool keyed = true}) {
    if (cls.typeParameters.isEmpty) return '';
    String bound(String p) {
      // `PartialEq`: `self._value == new_value` on a `T` (`ValueNotifier`).
      // `Clone + DartNullable<Or: Clone> + 'static` only (2026-09-04): `PartialEq + Debug` on every
      // type parameter shut out closures and futures -- `ObserverList<
      // VoidCallback>`, a `Set<Future>` -- at the class, not at the one
      // method that compares or prints. A method that does is what fails
      // now, and the stub count says how many.
      // The prelude's `Map` and `Set` are ordered and compare keys with
      // `==`: `PartialEq + Clone` is all they ask, and `Eq + Hash` shut
      // closures out of `ObserverList<VoidCallback>` (48 in `widgets`).
      // ..and no `PartialEq` for a key parameter either (2026-09-06): the
      // prelude's `Map` and `Set` compare keys by `DartEq`, which every
      // parameter already carries; the `PartialEq` block shut
      // `ObserverList<VoidCallback>.add` out (run459).
      return "$p: Clone${_nbp(cls, p)} + 'static";
    }

    return '<${cls.typeParameters.map(bound).join(', ')}>';
  }

  /// Type parameters no field mentions.
  ///
  /// Rust refuses an unused parameter; Dart does not care. Anything the fields
  /// do not name gets a `PhantomData` so the declaration stays legal without
  /// changing what the class holds.
  List<String> _unusedParameters(IrClass of) {
    if (of.typeParameters.isEmpty) return const [];
    final used = <String>{};
    void mark(IrType t) {
      used.add(t.name);
      t.arguments.forEach(mark);
      t.parameters?.forEach(mark);
      final returns = t.returns;
      if (returns != null) mark(returns);
    }

    for (final field in _allFields(of)) {
      mark(field.type);
    }
    return [
      for (final p in of.typeParameters)
        if (!used.contains(p)) p,
    ];
  }

  /// Whether an expression reads `this`.
  ///
  /// Used where `this` does not exist yet -- inside the struct literal a
  /// constructor builds.
  static bool _mentionsThis(IrExpr e) {
    var found = false;
    final walk = _WalkSelf();
    walk.expression(e);
    found = walk.readsThis;
    return found;
  }

  /// Whether a Rust type is `Copy`.
  ///
  /// Asked of the rendered text rather than the IR, because that is what the
  /// derive has to be true of. Owning types are the ones that are not.
  /// A `const` needs a value Rust can build at compile time, and neither
  /// `vec![]` nor `HashMap::from([..])` is one. Said here rather than left to
  /// rustc, because one broken constant takes the whole file with it.
  static bool _constable(String rust) =>
      !rust.contains('Vec<') && !rust.contains('Map<');

  /// Whether an emitted Rust type derives `Copy`.
  ///
  /// The containers are decided by the text. A **class name** is not, and was
  /// assumed `Copy` -- right for `Offset`, wrong for anything holding a
  /// `String`, and the ruler said "is this Copy" while measuring "does the
  /// text mention a container that is not". So a named class is asked the same
  /// question its own derive is asked, which is what makes the two agree.
  bool _isCopy(String rust) => _isCopyIn(rust, {});

  bool _isCopyIn(String rust, Set<String> seen) {
    if (!_copyText(rust)) return false;
    for (final name in _namesIn(rust)) {
      // A type parameter is not known to be `Copy`, and a read of a `T`
      // field behind `&self` has to clone it: `ValueNotifier.value`.
      // ..a method's own parameter too: a captured `T? arg` local in
      // `_throttle<T>` was a `Cell<Option<T>>` read by `get()` (run677).
      if (cls.typeParameters.contains(name) ||
          _methodTypeParams.contains(name)) {
        return false;
      }
      final prelude = _preludeCopy[name];
      if (prelude == false) return false;
      if (prelude != null) continue;
      final other = library[name];
      if (other != null && !_classIsCopy(other, seen)) return false;
    }
    return true;
  }

  /// Whether a field's cell is a `Cell` (its held type is `Copy`) as seen
  /// from anywhere: a field of another class typed by *that* class's
  /// parameter (`Holder<T>.value`, written as `h.value = ..` from outside,
  /// ws510) is not `Copy` -- the parameter is no name known here.
  bool _fieldIsCopy(IrFieldDecl field, IrClass? owner) {
    final held = _heldDecl(field);
    if (owner != null && _namesIn(held).any(owner.typeParameters.contains)) {
      return false;
    }
    return _isCopy(held);
  }

  /// Which of the prelude's own types are `Copy`, read out of the prelude.
  ///
  /// `WriteBuffer` holds a `Uint8List`, whose Rust name says nothing about
  /// what it is -- `_copyText` saw an identifier and passed it, and the struct
  /// derived `Copy` around a `Vec`. Listing the names here would be a second
  /// source of truth for something the prelude already states in its own
  /// derives, which is the thing `regen.py` exists to avoid.
  static final Map<String, bool> _preludeCopy = _readPrelude();

  static Map<String, bool> _readPrelude() {
    final answers = <String, bool>{};
    final aliases = <String, String>{};
    final lines = const LineSplitter().convert(rustPrelude);
    for (var i = 0; i < lines.length; i++) {
      final line = lines[i];
      final alias = RegExp(r'^pub type (\w+)[^=]*= *(.*);').firstMatch(line);
      if (alias != null) {
        aliases[alias[1]!] = alias[2]!;
        continue;
      }
      final decl = RegExp(r'^pub (?:struct|enum) (\w+)').firstMatch(line);
      if (decl == null) continue;
      // The derive sits on the line above, under any doc comment.
      final above = i > 0 ? lines[i - 1] : '';
      answers[decl[1]!] =
          above.startsWith('#[derive(') && above.contains('Copy');
    }
    // An alias is as `Copy` as what it stands for, which may be another alias.
    String resolve(String text, int depth) {
      if (depth > 4) return text;
      for (final name in _namesIn(text)) {
        final next = aliases[name];
        if (next != null)
          return resolve(text.replaceAll(name, next), depth + 1);
      }
      return text;
    }

    for (final entry in aliases.entries) {
      final text = resolve(entry.value, 0);
      answers[entry.key] =
          _copyText(text) && _namesIn(text).every((n) => answers[n] ?? true);
    }
    return answers;
  }

  static bool _copyText(String rust) =>
      !rust.contains('String') &&
      !rust.contains('std::boxed::Box<') &&
      !rust.contains('Vec<') &&
      !rust.contains('Map<') &&
      // A shared field's `Rc` is not `Copy` however copyable its contents,
      // and a `RefCell` is not either. Without these a struct holding one
      // derived `Copy` and did not compile.
      !rust.contains('Rc<') &&
      !rust.contains('RefCell<') &&
      !rust.contains('Cell<') &&
      !rust.contains('VecDeque') &&
      !rust.contains('dyn ');

  static final _typeName = RegExp(r'[A-Za-z_][A-Za-z_0-9]*');

  static Iterable<String> _namesIn(String rust) =>
      _typeName.allMatches(rust).map((m) => m[0]!);

  /// Answers by class name, once per *library* rather than once per class:
  /// there is one backend per class, and 4123 of them each walking the whole
  /// hierarchy is the shape of a compiler that got slower for no reason.
  static final _copyableIn = Expando<Map<String, bool>>('copyable');

  Map<String, bool> get _copyable => _copyableIn[library] ??= <String, bool>{};

  bool _classIsCopy(IrClass other, Set<String> seen) {
    final known = _copyable[other.name];
    if (known != null) return known;
    // Reached from itself. A value type cannot really contain itself -- the
    // struct would have no size -- so this is a hierarchy that says something
    // impossible, and `Clone` is the half that costs nothing but a clone.
    if (!seen.add(other.name)) return false;
    // A class emitted as a trait has no fields of its own here; its uses are
    // `Box<dyn ..>`, which `_copyText` has already turned down.
    final answer = _allFields(other).every((f) {
      // Any cell (`_inCellOf`: shared, mutable on a counted class, handed
      // out or set through a trait) is an `Rc`, and no `Copy`. Only the
      // first two were asked, and `BorderRadiusTween` -- whose `end` a
      // `Tween` handle writes -- went into a `Cell` (ws617: 34 crates).
      if (_inCellOf(other, f)) return false;
      final held = f.isLate ? 'Option<${type(f.type)}>' : type(f.type);
      // *That* class's parameters: `Tween<T>` holds an `Option<T>`, and
      // asked from `AnimatedPositionedState` its `T` read as a class name
      // nobody knew, so the field went into a `Cell` (3 "Tween<f64>: Copy
      // is not satisfied" in `widgets`).
      if (_namesIn(held).any(other.typeParameters.contains)) return false;
      return _isCopyIn(held, seen);
    });
    _copyable[other.name] = answer;
    seen.remove(other.name);
    return answer;
  }

  /// A top-level function.
  ///
  /// The same body machinery a method uses, with no `self` -- `_selfName` is
  /// the lever for that, as it is for a constructor body and for the free
  /// functions an abstract class's methods become.
  /// A refusal reason as the text of a `todo!` (a Rust format string).
  static String _stubText(String reason) => reason
      .replaceAll('\\', '\\\\')
      .replaceAll('"', '\\"')
      .replaceAll('{', '{{')
      .replaceAll('}', '}}')
      .replaceAll('\n', ' ');
}
