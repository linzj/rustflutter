part of '../backend_rust.dart';

// The protocol impls every class gets: eq, hash, toString, FromDynamic.
augment class RustBackend {
  /// The read of a lazy `late` field through `receiver`: filled on the
  /// first read, the value each time.
  String _lazyRead(IrFieldDecl f, String receiver) {
    final name = snake(f.name);
    _lazyExpanding.add(f.name);
    // In the impl's terms when forwarded through one: `FormFieldState<T>`'s
    // `_value` under `impl FormFieldState<String>` (ws451).
    final init = expr(
      _implBinding.isEmpty
          ? f.initial!
          : _substitute(f.initial!, const {}, _implBinding),
    );
    _lazyExpanding.remove(f.name);
    return _isCopy(_heldDecl(f))
        ? '{ if $receiver.$name.get().is_none() { let __v = $init; $receiver.$name.set(Some(__v)); } $receiver.$name.get().unwrap() }'
        : '{ if $receiver.$name.borrow().is_none() { let __v = $init; *$receiver.$name.borrow_mut() = Some(__v); } let __r = $receiver.$name.borrow().clone().unwrap(); __r }';
  }

  /// A *generic* trait as the qualifier of a call on another object's
  /// handle: through the type the handle holds (`<dyn ModalRoute<T> as
  /// ModalRoute<T>>::add_local_history_entry(&*route)`), since a bare
  /// `ModalRoute::m(..)` is E0782 for a trait with parameters
  /// (`ScaffoldState._maybeBuildPersistentBottomSheet`, run672). Null
  /// where the handle's type or the arguments are unknown.
  String? _dynQualified(IrExpr? target, String qualifier) {
    if (target == null || target is IrThis) return null;
    final traced = Platform.environment['DART2RUST_TRACE_QUAL'] == qualifier;
    // The trait may be another module's (`ModalRoute` from `widgets`
    // called in `material`): `elsewhere` knows it.
    final declaring = library[qualifier] ?? library.elsewhere[qualifier];
    if (declaring == null ||
        !library.isAbstract(qualifier) ||
        declaring.typeParameters.isEmpty) {
      if (traced) {
        stderr.writeln(
          'TRACE_QUAL $qualifier: declaring=${declaring != null} abstract=${library.isAbstract(qualifier)} params=${declaring?.typeParameters}',
        );
      }
      return null;
    }
    final held = target.rustType;
    if (held == null || held.isFunction) {
      if (traced)
        stderr.writeln(
          'TRACE_QUAL $qualifier: untyped target ${target.runtimeType}',
        );
      return null;
    }
    final base = stripNull(held);
    final owned = library[base.name] ?? library.elsewhere[base.name];
    if (owned == null) {
      if (traced)
        stderr.writeln('TRACE_QUAL $qualifier: unknown class ${base.name}');
      return null;
    }
    // The base's arguments through the held class, in the *handle's*
    // terms: `RestorableEnum<X>` is a `RestorableProperty<X>`, not a
    // `RestorableProperty<T>` (E0425, ws673).
    final ownBinding = {
      for (
        var i = 0;
        i < owned.typeParameters.length && i < base.arguments.length;
        i++
      )
        owned.typeParameters[i]: base.arguments[i],
    };
    final through = base.name == qualifier
        ? base.arguments
        : _argumentsThrough(owned, const {}, declaring, {});
    final passed = through == null
        ? null
        : [for (final t in through) _substituteType(t, ownBinding)];
    if (passed == null || passed.length != declaring.typeParameters.length) {
      if (traced)
        stderr.writeln('TRACE_QUAL $qualifier: args $passed for ${base.name}');
      return null;
    }
    final args = passed.isEmpty ? '' : '<${passed.map(type).join(', ')}>';
    final heldArgs = base.arguments.isEmpty
        ? ''
        : '<${base.arguments.map(type).join(', ')}>';
    final holder = library.isAbstract(base.name)
        ? 'dyn ${base.name}$heldArgs'
        : '${base.name}$heldArgs';
    return '<$holder as $qualifier$args>';
  }

  /// The type arguments this class passes to the generic trait `name`, spelled
  /// (`<T>`), or nothing for a non-generic trait or one it cannot compute
  /// (151 E0107 `missing generics for trait` at ws445).
  String _traitArgsOf(String name) {
    final trait = library[name];
    if (trait == null || trait.typeParameters.isEmpty) return '';
    // This class's own trait: its own parameters (`<__Self as
    // CupertinoPageRoute<T>>` in its super fns, ws451).
    if (name == cls.name) return _generics(cls);
    final passed = _argumentsThrough(cls, const {}, trait, {});
    if (passed == null || passed.isEmpty) return '';
    return '<${passed.map(type).join(', ')}>';
  }

  /// The locals the closure being printed captured (see `IrLocal`).
  var _closureCaptured = <String>{};

  /// `DartEq` for the struct or enum (see the prelude's `DartEq`): `body`
  /// compares `self` and `other`; `extraBound` joins each type parameter's
  /// bounds, `where` follows the header.
  void _emitDartEq({
    required String body,
    String extraBound = '',
    String where = '',
  }) {
    final own = '${cls.name}${_generics(cls)}';
    final header = cls.typeParameters.isEmpty
        ? ''
        : '<${cls.typeParameters.map((p) => "$p: Clone${_nbp(cls, p)} + 'static$extraBound").join(', ')}>';
    _line('impl$header DartEq for $own$where {');
    _indent++;
    _line('fn dart_eq(&self, other: &Self) -> bool { $body }');
    final hash = _dartHashBody();
    if (hash != null) _line('fn dart_hash_code(&self) -> i64 { $hash }');
    _indent--;
    _line('}');
    _line('');
  }

  /// Dart's `hashCode` of this class for `DartEq::dart_hash_code`: the
  /// class's own override (or the nearest ancestor's through its trait),
  /// a counted class's identity otherwise -- as Dart's `Object.hashCode`
  /// -- and, for a value class with none, the default (`0`, consistent
  /// with its value equality). Null when the default stands.
  String? _dartHashBody() {
    final need = IrMethod(
      'hashCode',
      const [],
      const IrType('int'),
      IrBlock(const []),
      isGetter: true,
    );
    final own = _matching(need);
    String? call;
    if (own != null && !own.isStatic && own.params.isEmpty) {
      call = _inherentCall(own);
    } else {
      final inherited = _inherited(need);
      if (inherited != null && inherited.$2.params.isEmpty) {
        call = _inherentCall(inherited.$2, need, inherited.$1.name);
      }
    }
    // ..unless this class could not emit it: the stub panics, and every
    // collection that hashes its keys would panic with it. The default
    // below (nothing, or the handle's address) is consistent with any
    // equality, which is what the protocol promises.
    if (call != null && !_stubbed.contains('${cls.name}.hashCode')) {
      return _resultModel ? '$call.unwrap_or(0)' : call;
    }
    if (cls.counted) {
      return '(std::rc::Rc::as_ptr(&self.__self.get()) as *const u8 as usize as i64) & 0x3fff_ffff';
    }
    return null;
  }

  /// `FromDynamic` for the struct or enum (see the prelude's): the object
  /// asked for a value of this type (`dart_cast_any`), which is exact --
  /// a struct that cannot be cloned out of an object answers `None`.
  void _emitFromDynamic() {
    final own = '${cls.name}${_generics(cls)}';
    final body = _cloneable(cls) ? 'value.dart_cast_any::<Self>()' : 'None';
    _line(
      'impl${_generics(cls, static: true, clone: true)} FromDynamic for $own {',
    );
    _indent++;
    _line(
      'fn from_dynamic(value: &std::rc::Rc<dyn Object>) -> Option<Self> { $body }',
    );
    if (_cloneable(cls)) {
      _line(
        'fn from_same(value: &Self) -> Option<Self> { Some(value.clone()) }',
      );
    }
    _indent--;
    _line('}');
    _line('');
  }

  /// `to_list` for a struct that *is* an `Iterable<E>` (`IrClass.
  /// iterableElement`): its elements, walked off its own `iterator` --
  /// what `Iterable`'s members and a `for-in` on it read (the front end's
  /// `_listReceiver`; `Navigator`'s `_History`, ws499). Only where the
  /// struct carries the getter itself.
  void _emitToList() {
    final element = cls.iterableElement;
    if (element == null) return;
    final getter = cls.methods
        .where((m) => m.name == 'iterator' && m.isGetter && !m.isStatic)
        .firstOrNull;
    // ..declared as an `Iterator<E>`: a covariant `CharacterRange get
    // iterator` hands out its own trait handle, whose `move_next` is not
    // the prelude's (ws501).
    if (getter == null || getter.returnType.name != 'DartIterator') return;
    // Not a failing call: a `for-in` and a chain read the list where no
    // `?` can go, so a failing `iterator` getter is an uncaught exception
    // here, as it would be in Dart.
    final fetched = getter.fails
        ? 'match self.iterator() { Ok(__it) => __it, Err(__e) => panic!("uncaught Dart exception: {}", dart_str(&__e)) }'
        : 'self.iterator()';
    final element_ = type(element);
    // `__to_list`, a name no Dart member has: `ObserverList` overrides
    // `toList` itself, and the walker beside it was a duplicate (E0592,
    // ws500).
    _line(
      'pub fn __to_list(&self) -> Vec<$element_> { '
      'let __it = $fetched; let mut __out: Vec<$element_> = Vec::new(); '
      'while __it.move_next() { __out.push(__it.current()); } __out }',
    );
  }

  /// `__to_list` as a *trait* default, for an abstract class that is an
  /// `Iterable<E>`: the same walk `_emitToList` writes for a struct, over
  /// the trait's own `iterator` (which the front end declares for it).
  void _emitTraitToList() {
    final element = cls.iterableElement;
    if (element == null) return;
    final getter = [...cls.methods, ...cls.abstractMethods]
        .where((m) => m.name == 'iterator' && m.isGetter && !m.isStatic)
        .firstOrNull;
    if (getter == null || getter.returnType.name != 'DartIterator') return;
    // Through *this* trait: a supertrait may declare `iterator` too, and a
    // bare `self.iterator()` is then ambiguous (`TypedDataBuffer`, E0034 at
    // ws783).
    final own = '<Self as ${cls.name}${_useArguments(cls)}>::iterator(self)';
    final fetched = _resultModel
        ? 'match $own { Ok(__it) => __it, Err(__e) => panic!("uncaught Dart exception: {}", dart_str(&__e)) }'
        : own;
    final element_ = type(element);
    _line(
      'fn __to_list(&self) -> Vec<$element_> { '
      'let __it = $fetched; let mut __out: Vec<$element_> = Vec::new(); '
      'while __it.move_next() { __out.push(__it.current()); } __out }',
    );
    _line('');
  }

  /// `NativeAnswer` for the struct or enum (see the prelude's): a native
  /// declared to return one of its own (`dart:ui`'s `GlyphInfo`) reads the
  /// host's object as it; without one there is no value to give.
  void _emitNativeAnswer() {
    final own = '${cls.name}${_generics(cls)}';
    _line(
      'impl${_generics(cls, static: true, clone: true)} NativeAnswer for $own {',
    );
    _indent++;
    _line(
      'fn from_answer(answer: std::rc::Rc<dyn Object>, symbol: &str) -> Self { '
      'match answer.dart_cast_any::<Self>() { Some(value) => value, '
      'None => panic!("native `{}` answered {:?} where ${cls.name} was declared", symbol, answer) } }',
    );
    _line(
      'fn absent() -> Self { panic!("native answered nothing where ${cls.name} was declared") }',
    );
    _indent--;
    _line('}');
    _line('');
  }

  /// Dart's `toString()` of this class, for `DartAny::dart_to_string`: the
  /// class's own override, the nearest ancestor's through its trait, or
  /// `Object`'s `Instance of 'X'` -- an enum's `X.value`.
  String _dartToStringBody({bool enumForm = false}) {
    final need = IrMethod(
      'toString',
      const [],
      const IrType('String'),
      IrBlock(const []),
    );
    final own = _matching(need);
    final String? call;
    if (own != null && !own.isStatic && own.params.isEmpty) {
      call = _inherentCall(own);
    } else {
      final inherited = _inherited(need);
      call = inherited != null && inherited.$2.params.isEmpty
          ? _inherentCall(inherited.$2, need, inherited.$1.name)
          : null;
    }
    if (call != null) {
      return _resultModel ? '$call.unwrap_or_default()' : call;
    }
    if (enumForm) {
      return 'format!("${cls.name}.{}", DartEnum::name(self))';
    }
    return 'format!("Instance of \'{}\'", "${cls.name}")';
  }

  /// `DartAny` for an enum (see the struct's inline impl): its own type
  /// behind a fresh handle, and every interface it implements through the
  /// handle that impl keeps -- what lets `_emitBaseImpl`'s `impl Ts for U`
  /// compile, `Ts: DartAny` (an enum into an `Rc<dyn Ts>`, ws510).
  void _emitEnumDartAny() {
    _line('impl DartAny for ${cls.name} {');
    _indent++;
    _line(
      'fn dart_runtime_type(&self) -> Type { Type::of("${cls.dartName ?? cls.name}") }',
    );
    _line(
      'fn dart_to_string(&self) -> String { ${_dartToStringBody(enumForm: true)} }',
    );
    _line(
      'fn dart_eq_any(&self, other: &dyn std::any::Any) -> bool { match other.downcast_ref::<Self>() { Some(o) => self.dart_eq(o), None => false } }',
    );
    _line('fn dart_hash_any(&self) -> i64 { self.dart_hash_code() }');
    _line(
      'fn dart_cast(&self, __t: std::any::TypeId) -> Option<std::boxed::Box<dyn std::any::Any>> {',
    );
    _indent++;
    _line(
      'if __t == std::any::TypeId::of::<Self>() || __t == std::any::TypeId::of::<std::rc::Rc<Self>>() { return Some(std::boxed::Box::new(std::rc::Rc::new(self.clone()))); }',
    );
    for (final above in _abstractAncestors(cls)) {
      final arguments = _baseArguments(above);
      if (arguments == null) continue;
      _line(
        'if __t == std::any::TypeId::of::<dyn ${above.name}$arguments>() || __t == std::any::TypeId::of::<std::rc::Rc<dyn ${above.name}$arguments>>() { return Some(std::boxed::Box::new(self.dart_self_${snakeRaw(above.name)}())); }',
      );
    }
    // ..and `Object`, as a counted struct answers it: an erased twin's
    // `as T` is `dart_cast_any::<Rc<dyn Object>>()` (the gentrait
    // fixture's `found as T` through `get__erased`).
    _line(
      'if __t == std::any::TypeId::of::<dyn Object>() || __t == std::any::TypeId::of::<std::rc::Rc<dyn Object>>() { return Some(std::boxed::Box::new(std::rc::Rc::new(self.clone()) as std::rc::Rc<dyn Object>)); }',
    );
    _line('None');
    _indent--;
    _line('}');
    _indent--;
    _line('}');
    _line('');
  }

  void _emitDartNullable() {
    _emitFromDynamic();
    _emitNativeAnswer();
    // A `LinkedListEntry` subclass (dart:collection): the prelude's entry
    // protocol on its handle, and on no other -- `next()` on every `Rc`
    // shadowed `FocusTraversalPolicy.next(node)` (ws552).
    if (cls.superclass == 'LinkedListEntry') {
      _line('impl DartLinkedEntry for ${cls.name} {}');
      _line('');
    }
    final own = '${cls.name}${_generics(cls)}';
    // The struct's own bounds, not an impl's: `Or` is `Option<Self>` and
    // asks nothing of `T`, and a `T: Clone` here would have shut the
    // struct out of every `T?` slot in code that has no `Clone` (ws404).
    _line('impl${_generics(cls, static: true)} DartNullable for $own {');
    _indent++;
    _line('type Or = Option<Self>;');
    _line('fn option(or: Option<Self>) -> Option<Self> { or }');
    _line('fn from_option(option: Option<Self>) -> Option<Self> { option }');
    _indent--;
    _line('}');
    _line('');
  }

  static String _abstractStaticName(String owner, String name) =>
      _rustIdentifier('${snakeRaw(owner)}_${snakeRaw(name)}');

  /// Every module-level name a *static* member of `owner` can be given here.
  ///
  /// A static of an abstract or generic class does not live in an `impl`
  /// (`_freeStatics`): a method becomes a free function carrying the class's
  /// name, a constructor the same with `new`, and a field a module constant.
  /// Which of the three a member got is decided where it is emitted; an
  /// importer holds only the reference, so it proposes all of them and lets
  /// the module's own definitions say which was written.
  static Set<String> staticNamesFor(
    String owner,
    String name, {
    bool isSetter = false,
  }) => {
    _abstractStaticName(
      owner,
      name.isEmpty
          ? 'new'
          : isSetter
          ? 'set_${snake(name)}'
          : name,
    ),
    screamingSnake('${owner}_$name'),
  };
}
