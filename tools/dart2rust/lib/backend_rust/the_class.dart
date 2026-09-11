part of '../backend_rust.dart';

// The class: what a translated class becomes, enums and traits first.
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
  /// The names this emit wrote that it did not declare.
  ///
  /// A `super` call's free function and the traits it puts in that
  /// function's bound (`_superBoundTraits`) are the emitter's own
  /// inventions: `super.initInstances()` is `gesture_binding_super_init_
  /// instances`, and `TextSelectionDelegate`'s trait ends up bounded by
  /// `State` -- a class nothing in `services/text_input.dart` names. The
  /// importer used to read them back out of the text; this hands them over.
  /// Cleared per library, so the driver reads it right after the call.
  static final Set<String> namedElsewhere = {};

  static (String, List<String>) emitLibrary(
    IrLibrary library, {
    List<String> frontEndRefusals = const [],
  }) {
    namedElsewhere.clear();
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
            // Fallible, like a class's: Dart re-runs an initialiser that
            // threw (`DartLazy`).
            holder._failure = _resultModel ? _error : null;
            final made = holder.expr(constant.value);
            holder._failure = null;
            holder._line(
              '${holder._vis(constant.name)}static '
              '${screamingSnake(constant.name)}: '
              'DartLazy<std::cell::RefCell<$held>> = DartLazy::new(|| '
              'Ok(std::cell::RefCell::new($made)));',
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
            holder._failure = _resultModel ? _error : null;
            final made = holder.expr(constant.value);
            holder._failure = null;
            holder._line(
              'pub static ${screamingSnake(constant.name)}: '
              'DartLazy<${holder.type(constant.type)}> = '
              'DartLazy::new(|| Ok($made));',
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
        final first = cls.valueFields[cls.values.first]![field]!;
        final held = first.rustType;
        final rust = declared != null
            ? type(declared.type)
            : held != null
            ? type(held)
            : _literalType(expr(first));
        _line('${_vis(field)}fn ${snake(field)}(&self) -> $rust {');
        _indent++;
        _line('match self {');
        _indent++;
        for (final value in cls.values) {
          _line(
            '${cls.name}::${variants[value]} => '
            '${expr(cls.valueFields[value]![field]!)},',
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
    // The enum's own statics, as any class's: outside the impl, because
    // Rust has no associated `static`, and named with the class in front.
    _emitConstants(prefix: cls.name);
    _emitLazyStatics();
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
    // ..and whatever the super functions just written need `__Self` to be
    // beyond this trait (`_superBoundTraits`): the defaults below hand
    // `self` to them, and `Self` promises only what stands here. Skipped
    // where the name is already above, whatever the two spell their
    // arguments -- Rust takes one supertrait per trait.
    String bare(String t) => t.split('<').first.split('::').last;
    final already = {for (final s in supers) bare(s)};
    for (final bound in _superBoundTraits) {
      if (already.add(bare(bound))) supers.add(bound);
    }
    // A trait object compares as `dyn Object` does: the *object's* own
    // `operator ==`, by the registry, and identity for a class that
    // declares none (the prelude's `dart_any_eq`). By address instead --
    // which this wrote until ws934 -- `GlobalObjectKey(this) ==
    // GlobalObjectKey(this)` was false, so `Widget.canUpdate` said no and
    // `MaterialApp`'s `WidgetsApp` element was thrown away and re-inflated
    // on every rebuild: 193 times in a 60s run, each one a fresh
    // `_LocalizationsState` whose locale had to load again, which is what
    // emptied the render tree between frames.
    _line(
      'impl${_generics(cls, static: true, clone: false)} DartEq for dyn ${cls.name}${cls.typeParameters.isEmpty ? '' : '<${cls.typeParameters.join(', ')}>'} {',
    );
    _indent++;
    // ..and asks the object through its *own vtable*, not the registry.
    // Every translated trait has `DartAny` as a supertrait
    // (`pub trait Key: DartAny + std::fmt::Debug`), so `dyn Key` already
    // carries these four methods; `dart_any_eq` was hashing a `TypeId` and
    // probing a thread-local table to reach the very same per-class
    // implementation. `bin/vtable_probe.py` is the compiled proof that the
    // vtable route resolves, with a control that fails without the
    // supertrait.
    //
    // The answer must not change: `dart_eq_any` on a concrete class is what
    // the registry's closure called, so ws934 stands or falls with the
    // render tree being byte-identical.
    _line(
      'fn dart_eq(&self, other: &Self) -> bool { self.dart_eq_any(other.dart_any_ref()) }',
    );
    // ..and hashes the same way, consistently (see the prelude's `DartEq`).
    _line('fn dart_hash_code(&self) -> i64 { self.dart_hash_any() }');
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
    // ..and as a *borrow*, which is what a super function takes now that its
    // body is written once against `&dyn Trait` rather than monomorphised
    // per implementer (`_emitSuperFnBody`). A trait default cannot unsize
    // its own `&self` -- `Self` is `?Sized` there -- and the `Rc` twin above
    // would cost a refcount round trip on every super call. This is a
    // vtable slot and nothing else.
    _line(
      'fn dart_as_${snakeRaw(cls.name)}(&self) -> &(dyn ${cls.name}${_useArguments(cls)} + \'static);',
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
          // No leading `_`: the super function is not generic over `__Self`
          // any more, it takes `&dyn Trait` (`_emitSuperFnBody`). A default
          // cannot unsize its own `&self` -- `Self` is `?Sized` in a trait --
          // so it hands over the borrow the trait declares for exactly this.
          final own = [...cls.typeParameters, ...method.typeParameters];
          final dyn = _superTakesDyn(cls, method);
          final spelled = dyn
              ? (own.isEmpty ? '' : '::<${own.join(', ')}>')
              : '::<_${own.map((p) => ', $p').join()}>';
          final receiver = dyn
              ? 'self.dart_as_${snakeRaw(cls.name)}()'
              : 'self';
          final call =
              '${superFn(cls.name, method.name, isSetter: method.isSetter)}$spelled('
              '${[receiver, ...method.params.map((p) => snake(p.name))].join(', ')})';
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
}
