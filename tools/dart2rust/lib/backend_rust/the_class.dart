part of '../backend_rust.dart';

augment class RustBackend {
  // -- The class --------------------------------------------------------------

  /// Every class in the library, traits first.
  ///
  /// Traits lead because a struct's `impl` mentions them, and a reader who
  /// meets `impl AlignmentGeometry for Alignment` before the trait has to
  /// scroll to find out what was promised.
  /// Returns the source, and what it could not emit.
  ///
  /// Per class, not all-or-nothing. The front end has always collected refusals
  /// member by member; the backend did not, so one class it could not emit
  /// threw away the whole file -- including the classes that were fine. A
  /// compiler that produces nothing because of one bad class is much less
  /// useful than one that produces the rest and says which is missing.
  static (String, List<String>) emitLibrary(
    IrLibrary library, {
    List<String> frontEndRefusals = const [],
  }) {
    final out = StringBuffer();
    final refused = <String>[];
    if (frontEndRefusals.isNotEmpty) {
      // The front end's refusals belong in the file too. The backend has always
      // left a `// NOT TRANSLATED` where it stopped, but a member the *front
      // end* refused never reaches the backend at all, so the output said
      // nothing about it and only stderr did. A reader with the file in front
      // of them should not have to have kept the console.
      out.writeln(
        '// The front end refused '
        '${frontEndRefusals.length} member(s) in this library:',
      );
      for (final refusal in frontEndRefusals) {
        out.writeln('// NOT TRANSLATED: $refusal');
      }
      out.writeln();
    }
    if (library.functions.isNotEmpty) {
      // Free functions, written before the classes so a class body reading one
      // is looking at something already declared -- Rust does not care, and a
      // reader does.
      final holder = RustBackend(IrClass('<library>'), library: library);
      for (final function in library.functions) {
        holder._member(
          'top-level ${function.name}',
          () {
            holder._emitFreeFunction(function);
          },
          stub: (reason) => holder._emitFreeFunction(function, stubbed: reason),
        );
      }
      out.write(holder._out.join('\n'));
      out.writeln();
      for (final line in holder._out) {
        if (line.startsWith('// NOT TRANSLATED:')) {
          refused.add(line.substring('// NOT TRANSLATED: '.length));
        }
      }
    }
    if (library.constants.isNotEmpty) {
      // Module constants first: Dart's top-level names become Rust's, needing
      // no owner on either side.
      final holder = RustBackend(IrClass('<library>'), library: library);
      for (final constant in library.constants) {
        holder._member('top-level ${constant.name}', () {
          // A mutable top-level variable is a `static` with a cell in it. Dart
          // gives each isolate its own, which `Isolate` says, and anything in
          // the library may assign it, which the `RefCell` says. A `const`
          // cannot be either, so the two are emitted differently.
          if (constant.isMutable) {
            final held = holder.type(constant.type);
            holder._line(
              '${holder._vis(constant.name)}static '
              '${screamingSnake(constant.name)}: '
              'std::sync::LazyLock<Isolate<std::cell::RefCell<$held>>> = '
              'std::sync::LazyLock::new(|| '
              'Isolate(std::cell::RefCell::new('
              '${holder.expr(constant.value)})));',
            );
            return;
          }
          // A `const` with a destructor -- a `Vec`, a `String`, a `Map` --
          // is not a Rust `const` (E0493): a lazily built `static`, read
          // with `.clone()` (`_isLazyConst`).
          if (holder._isLazyConst(constant.name)) {
            // Behind `Isolate`, as the mutable ones are: a `static` must be
            // `Sync`, and an `Rc<dyn Object>` (`Object()` as a zone key) is
            // not; `Isolate` says "one per isolate" and carries that.
            holder._line(
              'pub static ${screamingSnake(constant.name)}: '
              'std::sync::LazyLock<Isolate<${holder.type(constant.type)}>> = '
              'std::sync::LazyLock::new(|| Isolate(${holder.expr(constant.value)}));',
            );
            return;
          }
          holder._line(
            'pub const ${screamingSnake(constant.name)}: '
            '${holder.type(constant.type)} = ${holder.expr(constant.value)};',
          );
        });
      }
      out.write(holder._out.join('\n'));
      out.writeln();
      out.writeln();
    }
    for (final cls in library.classes) {
      try {
        out.write(RustBackend(cls, library: library).emit());
        out.writeln();
      } on Unsupported catch (error) {
        // Written into the file, not only counted. A class the backend
        // refused used to vanish from the output with nothing said -- the
        // count went up in a summary nobody reads next to the code, and
        // `CupertinoTheme` was simply absent, which is the one thing this
        // compiler is not allowed to do.
        refused.add('${cls.name}: $error');
        out.writeln('// NOT TRANSLATED: ${cls.name}');
        out.writeln('//   $error');
        out.writeln();
      }
    }
    return (out.toString(), refused);
  }

  String emit() {
    if (cls.isEnum) return _emitEnum();
    if (cls.isAbstract) return _emitTrait();
    return _emitStruct();
  }

  /// A Dart enum becomes a Rust enum, which is one of the few places the two
  /// languages need nothing said at all.
  ///
  /// The variants are renamed: Dart writes `Axis.vertical` and Rust writes
  /// `Axis::Vertical`. The name is otherwise left alone, so the output is still
  /// searchable against upstream.
  ///
  /// `Copy` because a Dart enum value is passed around freely and a Rust one
  /// that moved would need a `.clone()` at every use -- and `Eq`/`Hash` because
  /// upstream compares them and uses them as map keys.
  String _emitEnum() {
    _line('// Generated by tools/dart2rust from upstream `${cls.name}`');
    _line('// (Dart enum -> Rust enum).');
    _line('');
    _doc(cls.doc);
    if (cls.values.isEmpty && !cls.enumElementsDeclared) {
      // The tree shaker took every element: the dill declares none, so the
      // program this was shaken out of cannot make one of these either, and
      // an uninhabited Rust enum is exact rather than a refusal. The *type*
      // is still named -- fields and signatures want it -- so it is emitted
      // (`_StateLifecycle`, `PathOperation`, `TextGranularity` and five more
      // at ws824; every one of them has zero `isEnumElement` fields in
      // `app_aot_sig.dill`, checked before this rule was written).
      _line('// `${cls.name}` has no values in this dill: the tree shaker');
      _line('// took its elements, and nothing here can make one. Emitted');
      _line('// uninhabited so that the name still resolves.');
      _line('#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]');
      _line('${_vis(cls.name)}enum ${cls.name} {}');
      return _out.join('\n') + '\n';
    }
    if (cls.values.isEmpty) {
      // No values, but the elements *are* declared: the front end refused an
      // enhanced enum's members. That is a refusal, and it says so.
      _line('// NOT TRANSLATED: `${cls.name}` has no values here -- either');
      _line(
        '// an enhanced enum this compiler refused, or one the tree shaker',
      );
      _line('// emptied. Uninhabited, so that its name still resolves.');
      _line('#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]');
      _line('${_vis(cls.name)}enum ${cls.name} {}');
      return _out.join('\n') + '\n';
    }
    final variants = variantNames(cls.values);
    _line('#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]');
    if (variants.values.any((v) => v != variantName(v))) {
      // Dart's own spelling, kept because capitalising would have collapsed
      // two variants into one. See `variantNames`.
      _line('#[allow(non_camel_case_types)]');
    }
    _line('${_vis(cls.name)}enum ${cls.name} {');
    _indent++;
    for (final value in cls.values) {
      _line('${variants[value]},');
    }
    _indent--;
    _line('}');
    // `index`: a Dart enum value knows its position, and `x.index` was read
    // as a method nothing declared (6 in `dart:ui`'s `BlendMode`).
    _line('');
    _line('impl ${cls.name} {');
    _indent++;
    _line('pub fn index(&self) -> i64 {');
    _indent++;
    _line('*self as i64');
    _indent--;
    _line('}');
    // `name`: `dart:core`'s `EnumName` extension, the value's Dart name.
    _line('pub fn name(&self) -> String {');
    _indent++;
    _line('match self {');
    _indent++;
    for (final value in cls.values) {
      _line('${cls.name}::${variants[value]} => "$value".to_string(),');
    }
    _indent--;
    _line('}');
    _indent--;
    _line('}');
    _indent--;
    _line('}');
    _line('');
    // ..and both through the prelude's `DartEnum`, which is how a generic
    // `enum_name_get_name(e)` reaches them.
    _line('impl DartEnum for ${cls.name} {');
    _indent++;
    _line('fn name(&self) -> String { ${cls.name}::name(self) }');
    _line('fn index(&self) -> i64 { ${cls.name}::index(self) }');
    _indent--;
    _line('}');
    _line('');
    _emitDartNullable();
    _emitDartEq(body: 'self == other');
    _emitEnumDartAny();
    // An enhanced enum: its members go in an impl, where they lose nothing.
    // Refusing the whole enum was right only while the alternative was
    // emitting a plain one and dropping them.
    final members = cls.methods.where((m) => m.operator == null).toList();
    // The fields the Dart variants carried, as getters. `Tristate.value` is 0,
    // 1 or 2 depending on which variant it is -- a `match`, not a payload,
    // because the value is a constant *of* the variant.
    final carried = cls.values.isEmpty
        ? const <String>[]
        : (cls.valueFields[cls.values.first]?.keys.toList() ?? const []);
    if (members.isNotEmpty || carried.isNotEmpty) {
      _line('');
      _line('impl ${cls.name} {');
      _indent++;
      for (final field in carried) {
        final declared = cls.fields.where((f) => f.name == field).firstOrNull;
        final rust = declared != null
            ? type(declared.type)
            : _literalType(cls.valueFields[cls.values.first]![field]!);
        _line('${_vis(field)}fn ${snake(field)}(&self) -> $rust {');
        _indent++;
        _line('match self {');
        _indent++;
        for (final value in cls.values) {
          _line(
            '${cls.name}::${variants[value]} => '
            '${cls.valueFields[value]![field]},',
          );
        }
        _indent--;
        _line('}');
        _indent--;
        _line('}');
        _line('');
      }
      for (final method in members) {
        _member(
          '${cls.name}.${method.name}',
          () => _emitMethod(method),
          stub: (reason) => _emitMethod(method, stubbed: reason),
        );
      }
      _indent--;
      _line('}');
    }
    // An enum implementing an interface -- `WidgetState` is a
    // `WidgetStatesConstraint` -- gets the impl a struct would, forwarding
    // to the enhanced enum's own methods (20 "trait bound not satisfied").
    _emitBaseImpl();
    return _out.join('\n') + '\n';
  }

  /// The Rust type of a literal, when the enum's field declaration is gone.
  ///
  /// The dill drops an enum's fields along with its elements, so the type has
  /// to come from the value. Only the four literal shapes the recovery admits
  /// can arrive here.
  static String _literalType(String literal) {
    if (literal.endsWith('.to_string()')) return 'String';
    if (literal == 'true' || literal == 'false') return 'bool';
    return literal.contains('.') ? 'f64' : 'i64';
  }

  /// An abstract class becomes a trait.
  ///
  /// Its abstract members are the trait's requirements and its concrete members
  /// are the trait's defaults, which is exactly the split Dart already made --
  /// a member with a body is inherited, one without must be supplied. Rust
  /// spells that split the same way, so nothing has to be invented here.
  ///
  /// What does *not* come across is the fields: Dart's abstract classes may
  /// declare storage and Rust's traits may not. Any such field is reported
  /// rather than dropped.
  String _emitTrait() {
    _fieldsAreAccessors = true;
    _inTrait = true;
    _line('// Generated by tools/dart2rust from upstream `${cls.name}`');
    _line('// (abstract -> trait).');
    _line('');
    _doc(cls.doc);
    // The free functions go first: which of them failed decides whether the
    // trait's matching default can delegate or has to be a `todo!()`.
    _emitSuperFns();
    _line('');
    // `: DartAny` so a `Box<dyn ..>` of this trait can be asked what it holds,
    // which is what `x is Foo` needs and what a bare trait object cannot do.
    // ..and the traits of its own supertypes: a `SourceSpanMixin on
    // SourceSpan` calls `start` through `this_: &__Self` in its super
    // functions, and `__Self: SourceSpanMixin` had to imply `SourceSpan`
    // for that to resolve (6). Dart's implementers already implement all
    // of them, and the flattening below emits those impls.
    // Not `Object`: a mixin `implements Listenable` lists `Object` among its
    // interfaces, and a trait with `Object` above it makes `dyn Mixin` an
    // `Object` twice over -- once here, once from the impl below (E0371).
    // And each once: `on ListenableMixin implements ListenableMixin` named
    // it twice.
    // ..and `Debug`: every struct is one (derived, or "Instance of"), and
    // a super function's `this_: &__Self` prints itself in messages
    // (`MapBase.mapToString(this)`, `"Trying to read $x from $this"`).
    // As a supertrait, `dyn X` is `Debug` by itself, so no impl for it below.
    final supers = <String>{
      'DartAny',
      'std::fmt::Debug',
      if (cls.superclass != null && library.isAbstract(cls.superclass))
        _traitPath(IrType(cls.superclass!, arguments: cls.superclassArguments)),
      // Not the prelude's interfaces: `SourceSpan implements Comparable<
      // SourceSpan>` as a supertrait names the trait inside its own bound
      // ("cycle detected when computing the super predicates"). A concrete
      // class gets a forwarding impl instead (`_emitPreludeInterfaces`).
      // ..and the mixins: `FixedScrollMetrics with ScrollMetrics` is a
      // `ScrollMetrics`, and its super functions call the mixin's methods
      // on `this_: &__Self` (39 `__Self: ScrollMetrics` at ws301).
      for (final i in [...cls.interfaces, ...cls.mixins])
        if (library.isAbstract(i.name) &&
            i.name != 'Object' &&
            !_preludeInterfaces.containsKey(i.name))
          _traitPath(i),
      // ..and the prelude's after all, where the instantiation does not
      // name this trait: the cycle above is `Comparable<Self>`, and
      // `CharacterRange implements Iterator<String>` is not one. Without
      // it `current()` -- which `CharacterRange` inherits and does not
      // redeclare -- is on no `dyn CharacterRange` (4 at ws859). The
      // implementers' forwarding impls satisfy it (`_emitPreludeInterfaces`).
      for (final i in cls.interfaces)
        if (_preludeInterfaces.containsKey(i.name) &&
            !i.arguments.any((a) => a.name == cls.name))
          '${i.name}${i.arguments.isEmpty ? '' : '<${i.arguments.map(type).join(', ')}>'}',
    }.toList();
    // A trait object compares by identity (`DartEq`), as `dyn Object` does.
    _line(
      'impl${_generics(cls, static: true, clone: false)} DartEq for dyn ${cls.name}${cls.typeParameters.isEmpty ? '' : '<${cls.typeParameters.join(', ')}>'} {',
    );
    _indent++;
    _line(
      'fn dart_eq(&self, other: &Self) -> bool { std::ptr::addr_eq(self as *const Self, other as *const Self) }',
    );
    // ..and hashes by it, consistently (see the prelude's `DartEq`).
    _line(
      'fn dart_hash_code(&self) -> i64 { (self as *const Self as *const u8 as usize as i64) & 0x3fff_ffff }',
    );
    _indent--;
    _line('}');
    _line('');
    _line(
      // No `Clone` on the trait's parameters: a `dyn Foo<Pin<Box<dyn
      // Future>>>` is named (`_CallbackHookProvider<Future<bool>>`), and a
      // bound here shuts the whole trait. The default methods that clone
      // carry it instead (`_traitWhere`).
      '${_vis(cls.name)}trait ${cls.name}${_generics(cls, static: true, clone: false)}: ${supers.join(' + ')} {',
    );
    _indent++;
    // Guarded per member, like the struct path. The trait path was missed when
    // that changed, and it showed the moment private members started being
    // translated: one `toString` holding a string concatenation took the whole
    // `AlignmentGeometry` trait with it, and every `impl` of it stopped
    // compiling. Third time this has come up -- the unit of refusal should be
    // the unit of work everywhere, not only where it has been noticed.
    // A trait holds no storage, so the fields this class declares are reached
    // through required accessors. The fields themselves live on every
    // implementor, put there by `_allFields`; this is the other half of that.
    // Not a field an abstract supertype already declares: a mixin's
    // fields are lowered into the class applying it, and an open class
    // that then became a trait offered `_listeners` beside
    // `ChangeNotifier`'s -- every read of it ambiguous (50 E0034 at
    // ws305). The ancestor's impl block carries the accessor.
    final inherited = {
      for (final above in _supertypesOf(cls))
        if (library.isAbstract(above.name))
          for (final f in above.fields) f.name,
    };
    for (final field in cls.fields) {
      if (inherited.contains(field.name)) continue;
      _line('/// `${cls.name}.${field.name}`, which the implementor stores.');
      // An accessor is a function like any other under the Result model.
      _line('fn ${snake(field.name)}(&self) -> ${_wrapped(type(field.type))};');
      // ..and writes, when the mixin's own methods write it: `_length =
      // newLength` inside `_TypedDataBuffer._grow` goes through this. The
      // receiver is `&self`: an implementer of a mixin that writes is
      // counted, and its field is a cell.
      if (_writable(field)) {
        _line(
          'fn set_${snake(field.name)}(&self, value: ${type(field.type)}) -> ${_wrapped('()')};',
        );
      }
      if (_handsCell(field)) {
        _line(
          'fn ${snake(field.name)}_cell(&self) -> ${_wrapped(_cellType(_heldType(field)))};',
        );
      }
      _line('');
    }
    // A held collection an application keeps for this hollow mixin: its
    // cell, so a trait body mutates the place (`IrClass.appliedFields`).
    for (final field in cls.appliedFields) {
      if (inherited.contains(field.name) || !_handsCell(field)) continue;
      _line('/// `${cls.name}.${field.name}`, held by the implementor.');
      _line(
        'fn ${snake(field.name)}_cell(&self) -> ${_wrapped(_cellType(_heldType(field)))};',
      );
      _line('');
    }
    // `this` as a value inside the trait's own bodies (see `DartSelf`).
    _line(
      'fn dart_self_${snakeRaw(cls.name)}(&self) -> std::rc::Rc<dyn ${cls.name}${_useArguments(cls)}>;',
    );
    _line('');
    for (final method in cls.abstractMethods) {
      _member('${cls.name}.${method.name} (required)', () {
        _refuseShadowedGeneric(method);
        _doc(method.doc);
        _line(
          'fn ${_methodName(method)}${_generics(method)}(${_params(method)})'
          ' -> ${_wrapped(method.isAsync ? _futureOf(method) : _spelledReturn(type(method.returnType)))}${_sizedBound(method)};',
        );
        _line('');
        _emitErasedTwin(method, defaultBody: false);
      });
    }
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      _member('${cls.name}.${method.name} (default)', () {
        _refuseShadowedGeneric(method);
        _doc(method.doc);
        _line(
          'fn ${_methodName(method)}${_generics(method)}(${_params(method)})'
          ' -> ${_wrapped(method.isAsync ? _futureOf(method) : _spelledReturn(type(method.returnType)))}${_traitWhere(method)} {',
        );
        _indent++;
        // The default delegates to the free function rather than holding the
        // body, so that an override can still reach it. See `superFn`.
        if (_superFailed.contains(method.name)) {
          _line('todo!("${cls.name}.${method.name} did not translate")');
        } else {
          // An `async` super function is an `async fn`: its future is
          // boxed here, borrowing `self` for the `'_` the signature allows.
          // The trait's own parameters spelled, so a class implementing
          // it at two instantiations is not ambiguous (ws451).
          final spelled = [
            if (cls.typeParameters.isNotEmpty ||
                method.typeParameters.isNotEmpty)
              '::<_${[...cls.typeParameters, ...method.typeParameters].map((p) => ', $p').join()}>',
          ].join();
          final call =
              '${superFn(cls.name, method.name, isSetter: method.isSetter)}$spelled('
              '${['self', ...method.params.map((p) => snake(p.name))].join(', ')})';
          // An async super function is a future, not a `Result`: the
          // trait default returns it in `Ok`.
          _line(method.isAsync && _resultModel ? 'Ok($call)' : call);
        }
        _indent--;
        _line('}');
        _line('');
        _emitErasedTwin(
          method,
          defaultBody: !_superFailed.contains(method.name),
        );
      });
    }
    // An abstract class that *is* an `Iterable<E>` walks its own iterator,
    // as a struct that is one does (`_emitToList`) -- as a trait default,
    // since a `Rc<dyn Characters>` is what the callers hold and a trait
    // object has only the trait's methods (6 at ws782).
    _emitTraitToList();
    _indent--;
    _line('}');
    // A trait holds no storage, but the *class* still had its statics, and in
    // Rust those are module-level items rather than trait items. The struct
    // path has emitted them all along and this one had not, so an abstract
    // class's `static` simply vanished: `NavigatorObserver._navigators` was
    // read from three places and declared nowhere.
    // The trait object is an `Object` through `DartAny: Object`: a
    // supertrait, so that an `Rc<dyn Widget>` unsizes to an `Rc<dyn
    // Object>`. An `impl Object for dyn Widget` stood here before and gave
    // no coercion (75 non-primitive casts at ws293).
    // A trait object compares and hashes by identity, which is what Dart's
    // `Object.==` and `hashCode` do for anything not overriding them, and
    // prints as its class. Without these a struct holding an `Rc<dyn
    // EngineLayer>` could not derive `PartialEq` or `Debug` (9 + 9 in
    // dart:ui), and `Map<Rc<dyn DynamicScheme>, _>` could not be looked up.
    final generics = _generics(cls, static: true);
    final object = 'dyn ${cls.name}${_generics(cls)}';
    _line('impl$generics PartialEq for $object {');
    _indent++;
    _line(
      'fn eq(&self, other: &Self) -> bool { std::ptr::addr_eq(self, other) }',
    );
    _indent--;
    _line('}');
    _line('impl$generics Eq for $object {}');
    _line('impl$generics std::hash::Hash for $object {');
    _indent++;
    _line('fn hash<H: std::hash::Hasher>(&self, state: &mut H) {');
    _indent++;
    _line('(self as *const Self as *const u8 as usize).hash(state)');
    _indent--;
    _line('}');
    _indent--;
    _line('}');

    _line('');
    // Module-level, so they carry the class's name: `Contrast.ratio` is
    // `contrast_ratio(..)` and `Platform.numberOfProcessors` is
    // `PLATFORM_NUMBER_OF_PROCESSORS`. A bare `pub const` here was read as
    // `Platform::..` by every caller (E0782, 86 of them), and a static
    // *method* of an abstract class was not emitted at all. `_staticCall`
    // and `_staticRead` spell the same names for an abstract owner.
    for (final method in cls.methods) {
      if (!method.isStatic || method.operator != null) continue;
      _member(
        '${cls.name}.${method.name} (static)',
        () => _emitMethod(
          method,
          // A static setter beside its getter keeps the `set_` prefix
          // here as it does in an impl (`Manager.client = ..` / `Manager
          // .client` were two `manager_client`s, E0428, the staticset
          // fixture).
          as: _abstractStaticName(
            cls.name,
            method.name.isEmpty
                ? 'new'
                : method.isSetter
                ? _methodName(method)
                : method.name,
          ),
        ),
      );
    }
    _emitConstants(prefix: cls.name);
    _emitLazyStatics();
    return _out.join('\n') + '\n';
  }

  /// `DartNullable` on every type parameter after all: a bound only where
  /// a projection asks for it has to be repeated by everything that names
  /// the generic type (`SlottedRenderObjectElement<SlotType>` in a trait
  /// whose `SlotType` had none, ws401), and every type implements it -- a
  /// boxed future through a handle (see the prelude). What `_needsNullable`
  /// still decides is `Clone` on a *struct's* parameters: a projecting
  /// struct's `T?` field is `<Vec<T> as DartNullable>::Or` when `T` is put
  /// in for a `List`, and that asks `T: Clone`.
  /// ..and `FromDynamic` beside it, on the same footing: Dart's `cast<K,
  /// V>` takes any `K`, and a generic body's `result.cast<K, V>()` asks
  /// it of a bare `K` (`invokeMapMethod`, run494). Every type carries it
  /// (the prelude's, `_emitFromDynamic` for the translated ones).
  String _nb(IrClass c) =>
      ' + DartNullable<Or: Clone + DartEq + FromDynamic + DartAny> + DartEq + FromDynamic + DartAny';

  /// ..for one parameter of the class: a numeric one (`T extends num`)
  /// carries the prelude's `DartNum` as well (`min`/`max` on a `T`).
  String _nbp(IrClass c, String p) =>
      '${_nb(c)}${c.numericParameters.contains(p) ? ' + DartNum' : ''}'
      '${c.enumParameters.contains(p) ? ' + DartEnum' : ''}';

  String _nbm(IrMethod m) =>
      ' + DartNullable<Or: Clone + DartEq + FromDynamic + DartAny> + DartEq + FromDynamic + DartAny';

  /// `DartNullable` for this struct or enum (see the prelude): its `T?` is
  /// `Option<Self>`. With the class's own generics, as its `DartAny` is.
  /// A `late` field whose initialiser mentions `this` is Dart's lazy one:
  /// evaluated on the first read, in a cell so a `&self` read can fill
  /// it. (`late final _manifold = _BindingPipelineManifold(this)` read
  /// `_semanticsEnabled`, set by an `initInstances` that ran *after* the
  /// constructor's eager evaluation of it: run440's `None`.)
  bool _lazyLate(IrFieldDecl f) =>
      f.isLate &&
      f.initial != null &&
      _mentionsThis(f.initial!) &&
      _inCell(f) &&
      !_lazyExpanding.contains(f.name);

  /// The lazy fields whose initialiser is being printed: a read of the
  /// same field inside it (a closure the initialiser hands out reading it
  /// later) is the plain read, or the expansion never ends.
  final _lazyExpanding = <String>{};

  /// The class's lazy `late` field of this name, by its full declaration
  /// (the shared-field census carries no initialiser).
  IrFieldDecl? _lazyDecl(String name) =>
      _allFields(cls).where((f) => f.name == name && _lazyLate(f)).firstOrNull;

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

  void _emitFreeFunction(IrMethod method, {String? stubbed}) {
    _doc(method.doc);
    // Before the parameters are spelled: `_param` asks `_reassigned`
    // whether each is written, and it held the previous method's answer.
    _reassigned = _assignedIn(method.body);
    _mutRefParams = {
      for (final p in method.params)
        if (p.mutRef) p.name,
    };
    _cellLocals = {};
    final params = method.params.map((p) => _param(p, owned: false)).join(', ');
    final async = method.isAsync && stubbed == null;
    if (method.isAsync) {
      if (stubbed != null) {
        _line(
          '${_vis(method.name)}fn ${snake(method.name)}${_generics(method)}($params) -> ${_futureOf(method)} {',
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
        '${_vis(method.name)}fn ${snake(method.name)}${_generics(method)}($params) -> ${_futureOf(method)}',
        '${snake(method.name)}__body',
        turbofish: method.typeParameters.isEmpty
            ? ''
            : '::<${method.typeParameters.join(', ')}>',
      );
      _line('');
    }
    _line(
      '${_vis(method.name)}${async ? 'async ' : ''}fn '
      '${async ? '${snake(method.name)}__body' : snake(method.name)}${_generics(method)}'
      '($params) -> ${_returnType(method)} {',
    );
    _indent++;
    final saved = _selfName;
    // There is no receiver. Anything in the body that wanted one is a bug in
    // the front end, not something to paper over here.
    _selfName = '<no self>';
    _returns = method.returnType;
    _here = '${cls.name}.${method.name}';
    // The Rust return type too: a `try` body that returns carries
    // `Option<..>` of it out of its closure, and without it `_isLoopback`'s
    // `return address.isLoopback` came out as an `Option<()>`.
    _rustReturns = _returnType(method);
    _failure = _failureOf(method);
    _asyncBody = method.isAsync;
    _methodTypeParams = method.typeParameters;
    if (stubbed != null) {
      _line('panic!("dart2rust: not translated: ${_stubText(stubbed)}")');
    } else {
      if (!_body(
        method.body,
        method.isAsync ? _awaited(method.returnType) : method.returnType,
      )) {
        _closeOpenIf(method.body);
      }
    }
    _returns = null;
    _rustReturns = null;
    _selfName = saved;
    _indent--;
    _line('}');
    _line('');
  }

  /// The bodies of this abstract class's concrete methods, as free functions.
  ///
  /// Generic over the implementor and `?Sized`, so both the trait's own default
  /// and a subclass's override can call it -- the default has an unsized `Self`,
  /// and a subclass has a concrete one.
  /// Names whose free function could not be emitted.
  ///
  /// The trait's default for such a method cannot delegate to a function that
  /// does not exist, so it gets a `todo!()` instead -- the trait and every impl
  /// of it still line up, which a missing method would not.
  final _superFailed = <String>{};

  void _emitSuperFns() {
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      if (!_member(
        superFn(cls.name, method.name, isSetter: method.isSetter),
        () => _emitSuperFn(method),
      )) {
        _superFailed.add(method.name);
      }
    }
  }

  /// A generic method's type parameters read as `Object` (the erased
  /// twin's view; see the prelude's `CastErased`).
  Map<String, IrType> _erasure(IrMethod method) => {
    for (final p in method.typeParameters) p: const IrType('Object'),
  };

  String _erasedSignature(IrMethod method) {
    final erasure = _erasure(method);
    final params = [
      if (!method.isStatic) _sharedMutation(method) ? '&mut self' : '&self',
      ...method.params.map(
        (p) => _param(
          IrParam(
            p.name,
            _substituteType(p.type, erasure),
            named: p.named,
            hasDefault: p.hasDefault,
            kept: p.kept,
            mutRef: p.mutRef,
          ),
          owned: false,
        ),
      ),
    ].join(', ');
    final returns = _substituteType(method.returnType, erasure);
    final spelled = method.isAsync
        ? 'DartFuture<${type(_awaited(returns))}>'
        : _spelledReturn(type(returns));
    // The class's own parameters bounded as the trait's defaults bound
    // them (`_traitWhere`): the super function the default body reaches
    // asks `V: Clone` (`CanonicalizedMap.cast__erased`, ws483).
    final clauses = [
      for (final p in cls.typeParameters) '$p: Clone${_nbp(cls, p)}',
    ];
    final where = clauses.isEmpty ? '' : ' where ${clauses.join(', ')}';
    return 'fn ${_methodName(method)}__erased($params) -> ${_wrapped(spelled)}$where';
  }

  /// The erased twin of a generic trait method, in the trait: declared
  /// beside a required method, with the super function's body (its type
  /// parameters `Rc<dyn Object>`) beside a default one. Object-safe, so
  /// a `dyn` receiver reaches the method through it.
  void _emitErasedTwin(IrMethod method, {required bool defaultBody}) {
    if (method.typeParameters.isEmpty || method.isStatic) return;
    if (!defaultBody) {
      _line('${_erasedSignature(method)};');
      _line('');
      return;
    }
    _line('${_erasedSignature(method)} {');
    _indent++;
    final erased = method.typeParameters
        .map((_) => 'std::rc::Rc<dyn Object>')
        .join(', ');
    final spelled =
        '::<Self${[...cls.typeParameters].map((p) => ', $p').join()}, $erased>';
    final call =
        '${superFn(cls.name, method.name, isSetter: method.isSetter)}$spelled('
        '${['self', ...method.params.map((p) => snake(p.name))].join(', ')})';
    _line(method.isAsync && _resultModel ? 'Ok($call)' : call);
    _indent--;
    _line('}');
    _line('');
  }

  /// The erased twin in an implementer: through the class's own generic
  /// version, at `Rc<dyn Object>`.
  void _emitErasedImplTwin(IrMethod need, String trait) {
    if (need.typeParameters.isEmpty || need.isStatic) return;
    _line('${_erasedSignature(need)} {');
    _indent++;
    final erased = need.typeParameters
        .map((_) => 'std::rc::Rc<dyn Object>')
        .join(', ');
    _line(
      '<Self as $trait${_traitArgsOf(trait)}>::${_methodName(need)}::<$erased>'
      '(${['self', ...need.params.map((p) => snake(p.name))].join(', ')})',
    );
    _indent--;
    _line('}');
    _line('');
  }

  /// `where Self: Sized` for a generic method on a trait, or nothing.
  ///
  /// `RenderObject.invokeLayoutCallback<T extends Constraints>` is generic,
  /// and a generic method makes a trait dyn-incompatible -- so it used to be
  /// refused, on the reading that emitting it "would take `dyn RenderObject`
  /// away from the whole layer". That reading had a hole in it: Rust leaves a
  /// `where Self: Sized` method **out of the vtable**, so the trait stays
  /// dyn-compatible and every concrete implementor still has the method. It
  /// is the bound the standard library puts on `Iterator::by_ref` and friends
  /// for exactly this reason.
  ///
  /// What is given up is calling it *through* a trait object, which Dart does
  /// allow. That call is a refusal of its own where it happens, rather than
  /// 302 members deleted where they are declared.
  // A type parameter, or an `impl Future` parameter -- which is a type
  // parameter in a coat -- keeps a method out of the vtable, and a trait
  // used as `dyn` needs it kept out: `TransitionRoute` was "not dyn
  // compatible" for `_setSecondaryAnimation(.., Future<void>? disposed)`.
  /// A trait default method's `where`: `Self: Sized` when it needs it,
  /// and `T: Clone` for the class's parameters, which the super function
  /// holding its body asks for (see the trait header).
  String _traitWhere(IrMethod method) {
    final clauses = [
      if (_sizedBound(method).isNotEmpty) 'Self: Sized',
      for (final p in cls.typeParameters) '$p: Clone${_nbp(cls, p)}',
    ];
    return clauses.isEmpty ? '' : ' where ${clauses.join(', ')}';
  }

  static String _sizedBound(IrMethod method) =>
      method.typeParameters.isEmpty ? '' : ' where Self: Sized';

  /// A method whose type parameter has the same name as one of the class's.
  ///
  /// Dart allows the shadowing -- `Element.findAncestorStateOfType<T>` inside
  /// a `State<T>` -- and Rust does not: 44 `E0403`, all of them `T` inside a
  /// `T`. Renaming it would mean renaming it in the body too, which is a
  /// substitution this backend does not do, so the member is refused and says
  /// which name collided.
  void _refuseShadowedGeneric(IrMethod method) {
    for (final p in method.typeParameters) {
      if (cls.typeParameters.contains(p)) {
        throw Unsupported(
          "a method whose type parameter shadows the class's",
          '${cls.name}<$p>.${method.name}<$p>',
        );
      }
    }
  }

  /// Whether a super fn's body is being printed: `this` is a `&__Self`
  /// there, and so is `this` inside a closure of it, whose `_selfName` is
  /// the handle (`<Self as RendererBinding>` in `initMouseTracker`'s
  /// closure, E0411, run459).
  var _inSuperFn = false;

  void _emitSuperFn(IrMethod method) {
    final wasSuperFn = _inSuperFn;
    _inSuperFn = true;
    try {
      _emitSuperFnBody(method);
    } finally {
      _inSuperFn = wasSuperFn;
    }
  }

  void _emitSuperFnBody(IrMethod method) {
    {
      _line('');
      _line('/// The body of `${cls.name}.${method.name}`, reachable from an');
      _line('/// override the way Dart\'s `super.${method.name}` is.');
      final params = [
        // The body writes fields through `this_` when the method is one of
        // this class's mutating ones (or the trait's, for every class).
        // `&__Self` always: a write to a field in here goes through the
        // setter the trait declares, on `&self` (typed_data, 7 mismatches
        // once the trait's defaults went back to `&self`).
        'this_: &__Self',
        ...method.params.map(
          // `mut` when the body assigns it (`start = index + 1` in a loop).
          // A lent place (`IrParam.mutRef`) as the trait declares it.
          (p) =>
              '${_assignedIn(method.body).contains(p.name) ? 'mut ' : ''}'
              '${snake(p.name)}: ${p.mutRef ? '&mut ${type(p.type, owned: true)}' : type(p.type, owned: false)}',
        ),
      ].join(', ');
      // ..and by every trait a `super` call inside reaches that this
      // class is not below: a mixin's `super.initInstances()` dispatches
      // to the previous mixin of the application (`_realOwner`), which
      // its `on` clause never named (`SchedulerBinding`'s reaching
      // `GestureBinding`'s, 3 stubs on the start path at run448).
      final reached = _WalkSelf()..statement(method.body);
      final superBounds = [
        for (final MapEntry(key: base, value: arguments)
            in reached.superBases.entries)
          if (base != cls.name &&
              base != 'Object' &&
              _world.isTrait(base) &&
              !_world.isBelow(cls.name, base))
            ' + $base${arguments.isEmpty ? _traitArgsOf(base) : '<${arguments.map(type).join(', ')}>'}',
      ].join();
      final generics =
          '<__Self: ${cls.name}${_generics(cls)}$superBounds + ?Sized + \'static'
          '${cls.typeParameters.isEmpty ? '' : ', ${cls.typeParameters.map((p) => "$p: Clone${_nbp(cls, p)} + 'static").join(', ')}'}'
          '${method.typeParameters.isEmpty ? '' : ', ${method.typeParameters.map((p) => "$p: Clone${_nbm(method)} + 'static").join(', ')}'}'
          '>';
      final name = superFn(cls.name, method.name, isSetter: method.isSetter);
      if (method.isAsync) {
        // The wrapper holds the object through the trait's own handle
        // (`dart_self_<trait>()`, an `Rc<dyn Trait>`), and the body runs
        // on that: `__Self` there is the trait object.
        _emitAsyncWrapper(
          method,
          '${_vis(cls.name)}fn $name$generics($params) -> ${_futureOf(method)}',
          '${name}__body',
          receiver: (
            'let __self = this_.dart_self_${snakeRaw(cls.name)}();',
            '&*__self',
          ),
          turbofish:
              '::<_${cls.typeParameters.isEmpty ? '' : ', ${cls.typeParameters.join(', ')}'}${method.typeParameters.isEmpty ? '' : ', ${method.typeParameters.join(', ')}'}>',
        );
        _line('');
      }
      _line(
        '${_vis(cls.name)}${method.isAsync ? 'async ' : ''}fn '
        '${method.isAsync ? '${name}__body' : name}'
        '$generics($params) -> '
        // An `async fn` returns the awaited type. A boxed future returned by
        // a non-async one borrows `this_`: `+ '_`.
        '${_lifetimed(_returnType(method))} {',
      );
      _indent++;
      _selfName = 'this_';
      _returns = method.returnType;
      // ..and the Rust spelling, which a `try` that returns from inside
      // carries out through its closure (`Option<()>` carried an
      // `Rc<dyn Element>` in `inflateWidget`'s super function, ws475).
      final outerRustReturns = _rustReturns;
      _rustReturns = _returnType(method);
      _here = '${cls.name}.${method.name}';
      // A super function fails like the method whose body it holds.
      _failure = _failureOf(method);
      _asyncBody = method.isAsync;
      _methodTypeParams = method.typeParameters;
      _reassigned = _assignedIn(method.body);
      _mutRefParams = {
        for (final p in method.params)
          if (p.mutRef) p.name,
      };
      _cellLocals = {};
      // `this_` is a `&__Self: Trait`, and a trait has no fields: the base's
      // fields are its accessor methods here, as they are inside the trait
      // itself. `this_.start` was read as a field 6 times in `source_span`.
      final accessors = _fieldsAreAccessors;
      _fieldsAreAccessors = true;
      if (!_body(
        method.body,
        method.isAsync ? _awaited(method.returnType) : method.returnType,
      )) {
        _closeOpenIf(method.body);
      }
      _fieldsAreAccessors = accessors;
      _rustReturns = outerRustReturns;
      _returns = null;
      _selfName = 'self';
      _indent--;
      _line('}');
    }
  }

  /// A method's name, with Dart's operators mapped onto Rust's trait methods
  /// where one exists. Inside a trait there is no `impl std::ops::Add` to hang
  /// them on, so they become ordinary named methods.
  String _methodName(IrMethod method) {
    final op = method.operator;
    // A getter and a setter of the same Dart name are two members there and
    // one name here. The inherent path has always prefixed the setter; the
    // trait impls had not, so a mixin carrying `Ticker? get _ticker` beside
    // `set _ticker(v)` put two `fn _ticker` in one impl -- 839 `E0201`s.
    if (method.isSetter) return 'set_${snake(method.name)}';
    if (op == null) return snake(method.name);
    final mapping = operatorTraits[op];
    return mapping == null ? _operatorName(op) : 'op_${mapping.$2}';
  }

  /// A parameter's declaration, `mut` when the body reassigns it.
  ///
  /// Dart parameters are ordinary variables and get reassigned freely; Rust
  /// parameters are immutable unless the declaration says otherwise, and
  /// `mut x: f32` is where that is said. Without it,
  /// `shadow(start) { start = start + 1; }` emitted an assignment to something
  /// that cannot be assigned.
}
