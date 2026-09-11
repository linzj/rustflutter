part of '../backend_rust.dart';

// The trait impls, and the base methods they inherit or shadow.
augment class RustBackend {
  void _emitImplFor(
    IrClass base, {
    List<IrType>? passedOverride,
    List<IrType>? selfOverride,
  }) {
    _implFor = base.name;
    _selfBinding = selfOverride == null
        ? const {}
        : {
            for (
              var i = 0;
              i < cls.typeParameters.length && i < selfOverride.length;
              i++
            )
              cls.typeParameters[i]: selfOverride[i],
          };
    // Not just the abstract ones. A class that overrides a *concrete* base
    // method needs that override in the impl too, or dynamic dispatch reaches
    // the trait's default instead -- the inherent method would still be right,
    // so only a call through `dyn Base` can tell, which is why the tests make
    // that call.
    // ..and one that an ancestor nearer than the base overrides
    // (`_overriddenAbove`), for the same reason.
    final overridden = base.methods
        .where((m) => !m.isStatic && _overriddenAbove(base, m))
        .toList();
    // Accessors come from this base alone here; a farther ancestor gets its own
    // impl block and its own.
    final ownFields = base.fields;
    final required = [...base.abstractMethods, ...overridden];
    // Accessors count as a reason to emit the impl. A base with no abstract
    // methods and nothing overridden still has fields, and without them the
    // subclass does not implement the trait at all -- so its inherited methods
    // are unreachable, which is how `area()` went missing.
    // ..less the ones an abstract supertype of the base declares, which
    // the trait left to that ancestor (see `_emitTrait`) and whose impl
    // block for this class carries them.
    final inheritedByBase = {
      for (final above in _supertypesOf(base))
        if (library.isAbstract(above.name))
          for (final f in above.fields) f.name,
    };
    final accessors = [
      for (final f in ownFields)
        if (!inheritedByBase.contains(f.name)) f,
    ];
    // No early return when both are empty. A Dart subclass *is* its base
    // whether or not it changes anything, so the impl has to exist even with
    // nothing in it -- `Panel extends Measured with Scaled` overrides neither
    // and the mixin has no fields, and without `impl Scaled for Panel {}` the
    // free function holding `Scaled`'s body cannot be called on a `Panel`:
    // "the trait bound `Panel: Scaled` is not satisfied". An empty impl block
    // is the whole statement that it is one.

    final arguments = passedOverride != null
        ? '<${passedOverride.map((a) => type(a)).join(', ')}>'
        : _baseArguments(base);
    if (arguments == null) {
      // A generic ancestor whose arguments cannot be worked out from here.
      // Emitting `impl Base for This` without them does not compile; saying so
      // is better than leaving rustc to.
      _line('');
      _line('// NOT TRANSLATED: impl ${base.name} for ${cls.name}');
      _line('//   the base is generic and its arguments are not known here');
      return;
    }
    // Every signature in the block is the trait's, so it is spelled the
    // trait's way -- a callback parameter is `&dyn Fn`, not `impl Fn`, or the
    // impl declares a type parameter the trait method does not have.
    _inTrait = true;
    // Bound for the whole block: every signature inside is written in the
    // base's terms and has to come out in this class's.
    final passed = passedOverride ?? _baseTypeArguments(base) ?? const [];
    _implBinding = {
      if (passed.length == base.typeParameters.length)
        for (var i = 0; i < passed.length; i++)
          base.typeParameters[i]: passed[i],
    };
    _line('');
    // The parameters are *declared* on the impl before they are used.
    // `impl Trait<T> for Foo<T>` does not compile -- nothing introduced the
    // first `T` -- and leaving the declaration off was 428 `cannot find type
    // T` in the widget layer alone, one for every generic class's every trait
    // impl. The struct's own inherent impl had it right all along, which is
    // why it took a slice big enough to hold a generic class to show.
    // `'static` on the parameters, because the trait requires `DartAny` and
    // `DartAny` hands out a `&dyn Any`. A generic class implementing a trait
    // is the commonest shape in the widget layer, so leaving the bound off
    // here was 620 `E0310` in one go.
    // For one concrete instantiation (`selfOverride`): no impl generics,
    // the class spelled with those arguments.
    _line(
      selfOverride != null
          ? 'impl ${base.name}$arguments for '
                '${cls.name}<${selfOverride.map((a) => type(a)).join(', ')}> {'
          : 'impl${_implGenerics(cls)} ${base.name}$arguments for '
                '${cls.name}${_generics(cls)} {',
    );
    _indent++;
    // The handle a trait body's `this` is. A struct that is not counted has
    // no identity to give, and a fresh handle around a copy is what the
    // rest of its translation does with it too.
    _line(
      'fn dart_self_${snakeRaw(base.name)}(&self) -> std::rc::Rc<dyn ${base.name}$arguments> {',
    );
    _indent++;
    // ..and a generic value class can be cloned here too: the impl's
    // generics carry `T: Clone` since ws595 (`_implGenerics`), so the
    // derived `Clone` holds (`_ModalScope<T>` had "no handle of its own"
    // where `createElement` wanted one, run640).
    _line(
      cls.counted
          ? 'self.__self.get()'
          : _cloneable(cls)
          ? 'std::rc::Rc::new(self.clone())'
          : 'todo!("${cls.name} has no handle of its own")',
    );
    _indent--;
    _line('}');
    _line('');
    // ..and the borrow the super functions take. `Self` is concrete here, so
    // this is just the unsizing coercion, written once per implementer
    // instead of a whole method body per implementer.
    _line(
      'fn dart_as_${snakeRaw(base.name)}(&self) -> &(dyn ${base.name}$arguments + \'static) { self }',
    );
    _line('');
    // A field and a method of the same name are one item in Rust. A mixin
    // routinely has both -- `Ticker? _ticker;` beside a getter that reads it --
    // and emitting the accessor as well as the method put two `fn _ticker` in
    // one impl: 839 `E0201`s the moment mixins started being implemented. The
    // method wins, because it is the one that may have a body worth keeping.
    final taken = {for (final need in required) _methodName(need)};
    // The base's field is only *this* class's field when this class inherited
    // it. `class X extends A with M implements B` does not: a mixin's `on`
    // clause puts its constraint on the extends chain, so `B` is reached as an
    // ancestor while `X` satisfies it by implementing -- `viewId` there is a
    // getter of X's own, forwarding to something else, and reading
    // `self.view_id` names a field the struct does not have. 345 of those in
    // `PointerEvent` alone.
    final held = {for (final f in _allFields(cls)) f.name};
    for (final field in base.appliedFields) {
      if (!_handsCell(field) || accessors.any((a) => a.name == field.name)) {
        continue;
      }
      final cell = held.contains(field.name) ? _sharedField(field.name) : null;
      final substituted = _lateWrapped(
        field,
        type(_substituteType(field.type, _implBinding)),
      );
      _line(
        'fn ${snake(field.name)}_cell(&self) -> ${_wrapped(_cellType(substituted))} {',
      );
      _indent++;
      _line(
        cell != null
            ? (_resultModel
                  ? 'Ok(self.${snake(field.name)}.clone())'
                  : 'self.${snake(field.name)}.clone()')
            : 'todo!("${cls.name}.${field.name} is mutated through a trait but is not a cell")',
      );
      _indent--;
      _line('}');
      _line('');
    }
    for (final field in accessors) {
      // The cell accessor first, before a getter of the class's own can
      // take the value accessor's place: the trait asks for both.
      if (_handsCell(field)) {
        final cell = held.contains(field.name)
            ? _sharedField(field.name)
            : null;
        final substituted = _lateWrapped(
          field,
          type(_substituteType(field.type, _implBinding)),
        );
        _line(
          'fn ${snake(field.name)}_cell(&self) -> ${_wrapped(_cellType(substituted))} {',
        );
        _indent++;
        _line(
          cell != null
              ? (_resultModel
                    ? 'Ok(self.${snake(field.name)}.clone())'
                    : 'self.${snake(field.name)}.clone()')
              : 'todo!("${cls.name}.${field.name} is mutated through a trait but is not a cell")',
        );
        _indent--;
        _line('}');
        _line('');
      }
      if (taken.contains(snake(field.name))) continue;
      // Cloned out: the accessor returns a value and the field is behind
      // `&self` -- `fn _buffer(&self) -> Vec<i64> { self._buffer }` moved it.
      // ..and through the cell when the field is in one (a counted class):
      // `self.parent.clone()` handed out the `Rc<RefCell<..>>` itself.
      final cell = _sharedField(field.name);
      // A `late` field is held as an `Option` and the trait's accessor
      // gives the declared type: the read unwraps (Dart's read of an unset
      // `late` throws; this panics), the write wraps. `RenderObject`'s
      // `late bool _needsCompositing` alone was 363 mismatches in
      // `rendering` (194 `set`, 169 reads).
      final late = field.isLate
          ? _lateRead(field.name, fallible: _resultModel)
          : '';
      // A getter this class declares *overrides* the base's field: Dart
      // resolves the name to the getter, and a call through the trait is
      // the one path that can tell (`_SwitchDefaultsM3.padding` is
      // `EdgeInsets.symmetric(horizontal: 4)` where `SwitchThemeData`'s
      // field is null, and the switch's size read the field, run698).
      // Only when the getter's result is the trait's own type: a Dart
      // override may narrow it (`WidgetStateProperty<Color>` for a
      // `WidgetStateProperty<Color?>`), and that is a different Rust type
      // and a debt of its own -- those keep reading the storage.
      final ownGetter = cls.methods
          .where(
            (m) =>
                m.name == field.name &&
                !m.isStatic &&
                !m.isSetter &&
                m.params.isEmpty,
          )
          .firstOrNull;
      // ..and not a getter that reads `super.<name>` itself: the base's
      // accessor *is* the storage a `super` read reaches, so routing it
      // to such a getter is a cycle (`ListenableBuilder.listenable` and
      // `AnimatedBuilder.listenable` are both `=> super.listenable`, for
      // a doc comment, and the program overflowed its stack, run699).
      // Such a getter is the base's `x` anyway.
      // ..and only a getter this block can *call* as `self.x()`: an
      // inherent method taking `&self`. A counted class's method that
      // hands out `this` takes `&Rc<Self>` (`_receiverOf`), which a trait
      // body has no handle for; one an abstract supertype declares as a
      // *method* was emitted into that trait's impl, where the call is
      // ambiguous with the one being written here (`_TimePickerDefaults`
      // and `TimePickerThemeData` both declare `hourMinuteTextColor`,
      // ws702). Those keep reading the storage, with the covariant ones.
      final reachable =
          ownGetter != null &&
          !(cls.counted && _handles.contains(_rustName(ownGetter))) &&
          !_supertypesOf(cls)
              .where((t) => library.isAbstract(t.name))
              .any(
                (t) =>
                    t.methods.any(
                      (m) => m.name == field.name && !m.isStatic && !m.isSetter,
                    ) ||
                    t.abstractMethods.any(
                      (m) => m.name == field.name && !m.isSetter,
                    ),
              );
      final overrides =
          ownGetter != null &&
          reachable &&
          !_readsSuper(ownGetter, field.name) &&
          _fitsAccessor(
            _selfBound(ownGetter.returnType),
            _substituteType(field.type, _implBinding),
          );
      final reads = overrides
          ? 'self.${snake(field.name)}()${_resultModel ? '?' : ''}'
          : held.contains(field.name)
          ? (cell != null
                ? (_lazyLate(field)
                      ? _lazyRead(field, 'self')
                      : _isCopy(_heldDecl(cell))
                      ? 'self.${snake(field.name)}.get()$late'
                      : 'self.${snake(field.name)}.borrow().clone()$late')
                : _isCopy(type(_substituteType(field.type, _implBinding)))
                ? 'self.${snake(field.name)}$late'
                : 'self.${snake(field.name)}.clone()$late')
          : cls.methods.any((m) => m.name == field.name && !m.isStatic)
          // A getter is a method and returns `Result`; the accessor the
          // trait asks for cannot, and unwraps.
          ? 'self.${snake(field.name)}()${_resultModel ? '?' : ''}'
          : null;
      // The accessor's type is the *trait's*, so it is written in this
      // class's terms like every other signature in the block. Round 73
      // substituted the methods and left the accessors behind, which put a
      // `T` no impl declares in front of 103 field reads.
      // `todo!()`, not a refusal. A refused accessor leaves the trait
      // unimplemented -- 18 `E0046`s, one of them naming twenty-three at once
      // -- and the method path next door has always written a `todo!()` for
      // exactly this. The two owe the same answer.
      //
      // The case is real: `_TransformedPointerAddedEvent` gets `viewId` from a
      // mixin, and the IR does not copy a mixin's methods into the class, so
      // nothing here can see the getter that does exist. Reaching it means
      // going through the mixin's own trait, which is a round of its own.
      final body =
          reads ?? 'todo!("${cls.name} does not translate ${field.name} yet")';
      final substituted = _substituteType(field.type, _implBinding);
      _line(
        'fn ${snake(field.name)}(&self) -> ${_wrapped(type(substituted))} {',
      );
      _indent++;
      // The field holds one `Option`; a trait asking for the doubled one
      // gets it wrapped -- and the whole in `Ok`.
      // ..and any other difference between this class's field and the
      // trait's -- a `Matrix4` field under a `Matrix4?` accessor
      // (`_TransformedPointerCancelEvent.transform`, 15 at ws463) -- by
      // the one rule, as a method's result is.
      final own = _allFields(cls)
          .where((f) => f.name == field.name)
          .firstOrNull;
      String value;
      // What the body hands back: the getter's own result when the
      // accessor calls it, the field as this *instantiation* holds it
      // otherwise -- under `impl ValueKey<Option<i64>> for ValueKeyImpl
      // <i64>` the `T value` is an `i64`, and typed `T` the rule could
      // not see the `Some` it needed (ws659).
      final handed = overrides
          ? _selfBound(ownGetter!.returnType)
          : own == null
          ? null
          : _selfBound(own.type);
      if (reads != null &&
          handed != null &&
          (overrides || substituted.name != 'Option')) {
        final held = IrLocal('__v')..rustType = handed;
        final shaped = coerceInto(held, substituted, _world);
        // Nothing bridged the two and they are not the same type: a
        // *wider* impl of a generic trait whose accessor is a struct at
        // another instantiation (`_DelegateState<Object>.element` over a
        // `_InheritedProviderScopeElement<Listenable?>` field, ws710).
        // `todo!()` rather than code that does not compile: the method
        // path next door has always said it that way, and a body that
        // does not compile takes the whole function with it.
        value = identical(shaped, held)
            ? (sameRust(handed, substituted)
                  ? body
                  : 'todo!("${cls.name}.${field.name} is ${type(handed)} '
                        'and ${_implFor ?? base.name} asks '
                        '${type(substituted)}")')
            : '{ let __v = $body; ${expr(shaped)} }';
      } else {
        value = substituted.name == 'Option' && reads != null
            ? 'Some($body)'
            : body;
      }
      // ..and a getter the trait asks for that this class writes as a
      // *method* hands back that method's type, which an override may have
      // narrowed (`_FileSpan.end` is a `FileLocation` where
      // `SourceSpanBase.end` is `Rc<dyn SourceLocation>`, 6 at ws757). Only
      // where the rule has something to say: no conversion leaves the body
      // as it was, since this branch used to have no type to compare at all.
      if (handed == null && reads != null) {
        final ownMethod = cls.methods
            .where((m) => m.name == field.name && !m.isStatic && !m.isSetter)
            .firstOrNull;
        if (ownMethod != null) {
          final from = IrLocal('__v')
            ..rustType = _selfBound(ownMethod.returnType);
          final shaped = coerceInto(from, substituted, _world);
          if (!identical(shaped, from)) {
            value = '{ let __v = $body; ${expr(shaped)} }';
          }
        }
      }
      _line(reads != null && _resultModel ? 'Ok($value)' : value);
      _indent--;
      _line('}');
      _line('');
      // The setter the trait asks for on a mutable field (see `_emitTrait`).
      // Every setter the trait declares, held or not: an impl missing one
      // is "not all trait items implemented", and a whole crate with it
      // (`SnapshotController with ChangeNotifier`, the round the gate opened).
      if (_writable(field)) {
        final cell = held.contains(field.name)
            ? _sharedField(field.name)
            : null;
        _line(
          'fn set_${snake(field.name)}(&self, value: ${type(substituted)}) -> ${_wrapped('()')} {',
        );
        _indent++;
        if (cell != null) {
          // The trait's view of the field may be wider than this class's
          // (`Tween<T>.begin` as `T?` erased against `ColorTween`'s
          // `Color?`): the value is adapted into what the field holds.
          final own = cell.type;
          final given = IrLocal('value')..rustType = substituted;
          final into = type(substituted) == type(own)
              ? given
              : coerceInto(given, own, _world, inClosure: true);
          // ..and nothing bridged them: `todo!()`, as the read above says
          // it (`_DelegateState<Object>.element` over a
          // `_InheritedProviderScopeElement<Listenable?>` field, ws711).
          if (identical(into, given) && !sameRust(substituted, own)) {
            _line(
              'todo!("${cls.name}.${field.name} is ${type(own)} and '
              '${_implFor ?? base.name} writes ${type(substituted)}")',
            );
          } else {
            final adapted = expr(into);
            final stored = field.isLate ? 'Some($adapted)' : adapted;
            _line(
              _isCopy(_heldDecl(cell))
                  ? 'self.${snake(field.name)}.set($stored);'
                  : '*self.${snake(field.name)}.borrow_mut() = $stored;',
            );
            if (_resultModel) _line('Ok(())');
          }
        } else {
          _line(
            'todo!("${cls.name}.${field.name} is written through a trait but is not a cell")',
          );
        }
        _indent--;
        _line('}');
        _line('');
      }
    }
    for (final need in required) {
      _member(
        'impl ${base.name}::${need.operator ?? need.name} for ${cls.name}',
        () => _emitBaseMethod(need),
      );
    }
    _indent--;
    _line('}');
    _selfBinding = const {};
  }

  /// The base's type parameters, bound to what this class passed them.
  ///
  /// A trait method is declared in the base's terms -- `_RRectLike<T>` has
  /// `fn _create(..) -> T` -- and `impl _RRectLike<RRect> for RRect` has to
  /// say `-> RRect`. Copying the declaration through left a `T` no impl
  /// declares, which is the same mistake flattening made with fields one level
  /// down.
  var _implBinding = <String, IrType>{};

  /// The class's own type parameters bound to one concrete instantiation,
  /// while its wider impl for that instantiation is written (see
  /// `IrClass.extraImplSelf`); empty otherwise.
  Map<String, IrType> _selfBinding = const {};

  /// The method with each type parameter that shadows one of the class's
  /// renamed `T_` in its signature, or null when none does. The body is
  /// not rewritten: only a forwarder or a stub may use the result.
  IrMethod? _renamedShadowed(IrMethod need) {
    final shadowed = {
      for (final p in need.typeParameters)
        if (cls.typeParameters.contains(p)) p: IrType('${p}_'),
    };
    if (shadowed.isEmpty) return null;
    return IrMethod(
      need.name,
      [
        for (final p in need.params)
          IrParam(
            p.name,
            _substituteType(p.type, shadowed),
            named: p.named,
            hasDefault: p.hasDefault,
            kept: p.kept,
            mutRef: p.mutRef,
          ),
      ],
      _substituteType(need.returnType, shadowed),
      need.body,
      typeParameters: [
        for (final p in need.typeParameters)
          shadowed.containsKey(p) ? '${p}_' : p,
      ],
      isStatic: need.isStatic,
      isGetter: need.isGetter,
      isSetter: need.isSetter,
      operator: need.operator,
      throws: need.throws,
      doc: need.doc,
      isAsync: need.isAsync,
    );
  }

  void _emitBaseMethod(IrMethod need) {
    {
      // A method type parameter named like one of the class's --
      // `ParentDataElement<T>` implementing `BuildContext.
      // dependOnInheritedWidgetOfExactType<T>` -- is renamed here rather
      // than refused: this forwarder's body is the backend's own line and
      // never spells the parameter, so only the signature has to change
      // (4 "not all trait items implemented" in `widgets`, one per
      // generic `Element`).
      need = _renamedShadowed(need) ?? need;
      // A forwarder has parameters, not locals: the last body's cell locals
      // printed a parameter `child` as `child.borrow()` (7 at ws383).
      _cellLocals = {};
      // ..and the inherent method it reaches spelled with the same renaming,
      // so that its `T?` and the trait's `T_?` compare as one type and not
      // as two the coercion rule converts between (22 at ws411).
      var have = _matching(need);
      if (have != null && have.typeParameters.isNotEmpty) {
        have = _renamedShadowed(have) ?? have;
      }
      String? via;
      if (have == null) {
        final inherited = _inherited(need);
        if (inherited != null) {
          via = inherited.$1.name;
          have = inherited.$2;
          if (have.typeParameters.isNotEmpty) {
            have = _renamedShadowed(have) ?? have;
          }
        }
      }
      // Rust does not collapse `Option<Option<X>>` the way Dart collapses
      // `T?` for a nullable `T`: `MessageCodec<Object?>.decodeMessage` is
      // `-> Option<T>` in the trait and the impl must say `Option<Option<..>>`
      // -- 16 `E0053`s, the "14 members" `_substituteType`'s comment gave up
      // on. Spelled out here, with the body wrapped to match below.
      final returns = _spelledReturn(
        type(_substituteType(need.returnType, _implBinding)),
      );
      final wrappedReturns = _wrapped(returns);
      final params = [
        // The forwarder's receiver is the trait's: `&mut self` when any
        // implementer writes in this method, or `ChangeNotifier::
        // add_listener(self, ..)` under `&self` is a mutability mismatch.
        if (!need.isStatic) _sharedMutation(need) ? '&mut self' : '&self',
        ...need.params.map((p) {
          // A parameter whose type *is* one of the base's type parameters has
          // to be written the way the impl header wrote that parameter, which
          // is owned: Rust substitutes `ChildType` with the
          // `Box<dyn RenderBox>` in `impl RenderObjectWithChildMixin<Box<dyn
          // RenderBox>>`, and a borrowed `&dyn RenderBox` here is a different
          // type from the one the trait declared.
          final substituted = _substituteType(p.type, _implBinding);
          final fromParameter = _implBinding.containsKey(p.type.name);
          return _param(
            IrParam(
              p.name,
              substituted,
              named: p.named,
              hasDefault: p.hasDefault,
              // Carried, or the impl writes `&dyn Fn` where the trait it
              // implements declared `Box<dyn Fn>`.
              kept: p.kept,
              mutRef: p.mutRef,
            ),
            owned: fromParameter,
          );
        }),
      ].join(', ');
      _line(
        'fn ${_methodName(need)}${_generics(need)}($params) -> '
        '$wrappedReturns${_sizedBound(need)} {',
      );
      _indent++;
      // A mixin's field is an abstract getter and setter on its trait,
      // and the struct holds the field (flattened from the application):
      // read and written here, as an interface's field is above. 2747
      // `todo!`s at ws345 were these (`_tickerModeNotifier` 198, `_child`
      // 180, `_bucket` 110).
      final field = have == null
          ? _allFields(cls).where((f) => f.name == need.name).firstOrNull
          : null;
      if (field != null &&
          !need.isStatic &&
          (need.isSetter ? need.params.length == 1 : need.params.isEmpty)) {
        final cell = _sharedField(field.name);
        final late = field.isLate
            ? _lateRead(field.name, fallible: _resultModel)
            : '';
        final name = snake(field.name);
        if (need.isSetter) {
          if (cell != null) {
            // The trait's type is the erased bound (`Option<Rc<dyn
            // RenderObject>>`), the field's the narrower one (`RenderBox?`):
            // the trait cast narrows on the way in, and an `Option` is
            // taken off or put on (+319 mismatched at ws346).
            final given = _substituteType(
              need.params.single.type,
              _implBinding,
            );
            final held = field.type;
            var value = 'value';
            if (given.name != held.name &&
                library.isAbstract(given.name) &&
                library.isAbstract(held.name) &&
                held.name != 'Object') {
              final target = _dynOf(
                IrType(held.name, arguments: held.arguments),
              );
              value =
                  'value.dart_cast_to::<$target>()${held.nullable ? '' : '.unwrap()'}';
            } else if (held.nullable && !given.nullable) {
              value = 'Some(value)';
            } else if (!held.nullable && given.nullable) {
              value = 'value.unwrap()';
            }
            final stored = field.isLate ? 'Some($value)' : value;
            _line(
              _isCopy(_heldDecl(cell))
                  ? 'self.$name.set($stored);'
                  : '*self.$name.borrow_mut() = $stored;',
            );
            if (_resultModel) _line('Ok(())');
          } else {
            _line(
              'todo!("${cls.name}.${field.name} is written through a trait but is not a cell")',
            );
          }
        } else {
          final read = cell != null
              ? (_lazyLate(field)
                    ? _lazyRead(field, 'self')
                    : _isCopy(_heldDecl(cell))
                    ? 'self.$name.get()$late'
                    : 'self.$name.borrow().clone()$late')
              : _isCopy(type(field.type))
              ? 'self.$name$late'
              : 'self.$name.clone()$late';
          // ..and widened on the way out (`_shaped`), as a method's
          // result is.
          // ..and widened on the way out by the one rule (`coerceInto`),
          // as a method's result is.
          // ..in *this* class's terms first, as the method path does
          // (`_selfBound`): inside a wider impl for one instantiation the
          // field's `T` is the class's and the trait's `T` is the wider
          // argument, and left unresolved the two read as the same name
          // and the rule found nothing to do -- `Ok(self.value)` where an
          // `Option<f64>` goes (`AlwaysStoppedAnimation`, 2 at ws870).
          final held = IrLocal('__v')..rustType = _selfBound(field.type);
          if (Platform.environment['DART2RUST_TRACE_FWD'] == field.name) {
            stderr.writeln(
              'TRACE_FWD ${cls.name}.${field.name} field=${field.type} need=${need.returnType} for=${_implFor}',
            );
          }
          final shaped = coerceInto(
            held,
            _substituteType(need.returnType, _implBinding),
            _world,
          );
          final value = identical(shaped, held)
              ? read
              : '{ let __v = $read; ${expr(shaped)} }';
          _line(_resultModel ? 'Ok($value)' : value);
        }
      } else if (have == null) {
        // Reported in the output rather than silently skipped: a trait impl
        // missing a method does not compile, and the reader should learn why
        // from the file rather than from rustc.
        _line(
          'todo!("${cls.name} does not translate '
          '${need.operator ?? need.name} yet")',
        );
      } else {
        // ..and an async inherent method is a future the forwarder wraps
        // in `Ok` (49 `Pin<Box<impl Future>>` where `Result<..>` goes).
        // ..unless it is reached through an ancestor's trait (`via`): the
        // trait's method already returns the `Result`, and `Ok(ModalRoute::
        // will_pop(self))` doubled it (30 `will_pop` forwarders at ws535).
        // The arguments this forwarder passes on are *Dart's* casts: a
        // `covariant` parameter is a downcast at the call and Dart throws
        // there, so the cast propagates rather than unwrapping. Only here --
        // the forwarder's own value goes on through `.map(|__v| ..)` below,
        // whose closure returns a plain value and cannot carry a `?`.
        final savedFailure = _failure;
        _failure = _resultModel && _returnType(need).startsWith('Result<')
            ? _error
            : null;
        final inherent = _inherentCall(have, need, via);
        _failure = savedFailure;
        final call = have.isAsync && _resultModel && via == null
            ? 'Ok($inherent)'
            : inherent;
        // One `Option` short -- the override narrowed `T?` to `T`, which Dart
        // allows, or the trait's `T?` doubled up above -- is a `Some`.
        // The trait's future carries `+ '_` (see `_lifetimed`); the
        // inherent one is the same future without the spelling.
        // An `Rc<Concrete>` returned where the trait says `Rc<dyn Base>`
        // unsizes on its own at the return (47 `Box<Rc<dyn State>>`s).
        // ..a *value* returned there is put behind a fresh handle (`impl
        // BorderRadiusGeometry for BorderRadius`'s `op_mul`, 79), and a
        // `()` where the trait says `Option<..>` is `None` (`Action.invoke`
        // overridden as `void`, 46).
        // ..all by the one rule (`coerceInto`) inside the `Result`'s `map`.
        // A future is the same future under a lifetime spelling and is
        // left alone.
        final held = IrLocal('__v')
          ..rustType = _selfBound(_inThisClassTerms(have.returnType, via));
        if (Platform.environment['DART2RUST_TRACE_FWD'] == need.name) {
          stderr.writeln(
            'TRACE_FWD ${cls.name}.${need.name} have=${have.returnType} need=${need.returnType} method',
          );
        }
        // ..an async one too: the same future is left alone by the rule
        // (identical types), and a future of a value where the trait's
        // erased twin says `dyn Object` is mapped (`LocaleNamesLocalizations
        // Delegate.load` returning `Future<LocaleNames>` under
        // `LocalizationsDelegate<T>`, run598).
        final needReturns = _substituteType(need.returnType, _implBinding);
        final shaped = coerceInto(held, needReturns, _world, inClosure: true);
        // An override may return where the trait returns nothing
        // (`Disposer addListener(..)` over `void addListener(..)` in get's
        // `ListNotifier`): the value is dropped (run489).
        final dropsValue =
            !have.isAsync &&
            type(needReturns) == '()' &&
            type(have.returnType) != '()';
        // An inherent *operator* is a `std::ops` method: it takes `self` by
        // value and hands back the `Output` itself, not a `Result`. So the
        // conversion binds the value rather than mapping a `Result`, and
        // the trait's own `Result` is put on here (`impl EdgeInsetsGeometry
        // for EdgeInsets`'s `op_mul` got `*self * other.map(|__v| ..)`,
        // which mapped the *operand*, 6 at ws757).
        final infallible =
            have.operator != null && operatorTraits.containsKey(have.operator);
        final wrapsOk = _returnType(need).startsWith('Result<');
        String infallibleText(String inner) => wrapsOk ? 'Ok($inner)' : inner;
        _line(
          dropsValue
              ? (infallible
                    ? infallibleText('{ let _ = $call; () }')
                    : '$call.map(|_| ())')
              : identical(shaped, held)
              ? (infallible ? infallibleText(call) : call)
              : infallible
              ? '{ let __v = $call; ${infallibleText(expr(shaped))} }'
              : '$call.map(|__v| ${expr(shaped)})',
        );
      }
      _indent--;
      _line('}');
      _line('');
      final implFor = _implFor;
      if (implFor != null) _emitErasedImplTwin(need, implFor);
    }
  }

  /// This class's own version of a method the base requires.
  IrMethod? _matching(IrMethod need) {
    for (final method in cls.methods) {
      if (need.operator != null) {
        if (method.operator == need.operator) return method;
      } else if (method.operator == null &&
          method.name == need.name &&
          method.isSetter == need.isSetter) {
        // The getter and the setter share a name: `ValueListenable.value`
        // asked for its getter and got `TextEditingController`'s setter,
        // whose `newValue` "the base has no value for", and the impl
        // block came out without `value` at all.
        return method;
      }
    }
    return null;
  }

  /// How to invoke this class's own version, in Rust's own spelling.
  ///
  /// An operator that became an `impl std::ops::*` is invoked as the operator,
  /// not as a method: that is the whole point of having emitted the trait impl.
  /// The nearest class above this one with a body for `need`: an open
  /// class's `Impl` struct has none of its own (`_implOf`), and a subclass
  /// inherits the base's -- both reach the base trait's default through
  /// `Base::name(self, ..)`. Until ws345 every such method was a
  /// `todo!("X does not translate Y yet")`: 26199 of them, `insert`,
  /// `perform_layout` and `first_child` of `RenderFlexImpl` and all 796
  /// getters of each `GalleryLocalizationsXxImpl` -- compiled, never ran.
  ///
  /// The walk is Dart's own lookup order: a class's members, then its mixins
  /// nearest-applied first, then the superclass -- and so on up. Walking the
  /// `extends` chain alone passed over a mixin's override, and `class X
  /// extends Element with M` reached `Element.mount` where Dart runs
  /// `M.mount`.
  (IrClass, IrMethod)? _inherited(IrMethod need) {
    IrMethod? declared(IrClass at) {
      for (final method in at.methods) {
        if (need.operator != null) {
          if (method.operator == need.operator) return method;
        } else if (method.operator == null &&
            method.name == need.name &&
            method.isSetter == need.isSetter &&
            method.isStatic == need.isStatic) {
          return method;
        }
      }
      return null;
    }

    final seen = <String>{cls.name};
    IrClass? at = cls;
    var own = true;
    while (at != null) {
      // This class's own members are `_matching`'s; the walk starts at its
      // mixins.
      if (!own) {
        if (!library.isAbstract(at.name)) return null;
        final method = declared(at);
        if (method != null) return (at, method);
      }
      own = false;
      for (final applied in at.mixins.reversed) {
        final mixin = library[applied.name];
        if (mixin == null || !seen.add(mixin.name)) continue;
        if (!library.isAbstract(mixin.name)) continue;
        final method = declared(mixin);
        if (method != null) return (mixin, method);
      }
      final above = at.superclass;
      if (above == null || !seen.add(above)) return null;
      at = library[above];
    }
    return null;
  }

  /// Whether a body nearer than `base`'s answers `need` for this class: an
  /// abstract class between the two, or a mixin, overrides it.
  ///
  /// A Rust trait's default method does not replace the one a supertrait
  /// declared: `ComponentElement::mount` is what a `dyn ComponentElement`
  /// reaches, and a `dyn Element` still reaches `Element::mount`. The
  /// struct's `impl Element` has to route the method to the nearest override
  /// itself, exactly as it routes an abstract method to the nearest body.
  /// Without it, `StatefulElementImpl.mount` ran `Element.mount` alone and
  /// never built a child (run534: the tree ended at `View`).
  bool _overriddenAbove(IrClass base, IrMethod need) {
    if (_matching(need) != null) return true;
    final inherited = _inherited(need);
    if (Platform.environment['DART2RUST_TRACE_FWD'] == need.name) {
      stderr.writeln(
        'TRACE_FWD ${cls.name}.${need.name} above=${inherited?.$1.name} base=${base.name} super=${cls.superclass} abstract=${library.isAbstract(cls.superclass)}',
      );
    }
    return inherited != null &&
        !identical(inherited.$1, base) &&
        inherited.$1.name != base.name;
  }

  /// A type of an inherited method (`via`, the ancestor declaring it) in
  /// this class's terms: the ancestor's parameters replaced by what this
  /// class passes it (`RestorableEnumN<T> extends RestorableValue<T?>`:
  /// `RestorableValue`'s `T` is `Option<T>` here, and the forwarder
  /// handed `initWithValue` a bare `Orientation`, ws628).
  IrType _inThisClassTerms(IrType t, String? via) {
    if (via == null) return t;
    final base = library[via];
    if (base == null || base.typeParameters.isEmpty) return t;
    final passed = _argumentsThrough(cls, const {}, base, {});
    if (passed == null || passed.length != base.typeParameters.length) {
      return t;
    }
    return _substituteType(t, {
      for (var i = 0; i < passed.length; i++) base.typeParameters[i]: passed[i],
    });
  }

  String _inherentCall(IrMethod method, [IrMethod? through, String? via]) {
    // Dart lets an override *widen* an optional signature:
    // `OutlinedBorder.copyWith({side})` is overridden by
    // `BeveledRectangleBorder.copyWith({side, borderRadius})`. Rust does not,
    // so the trait method has fewer parameters than the inherent one it
    // delegates to -- and passing the inherent one's names through named a
    // `border_radius` that is not in scope, 30 times.
    //
    // What a caller reaching this through the trait would get in Dart is the
    // extra optionals *absent*, so that is what is passed: `None`. An extra
    // parameter that is not optional cannot be answered that way and the
    // delegation is refused instead of guessed at.
    // Positional parameters line up by **position**, not by name. Dart lets an
    // override rename them -- `Simulation.x(double time)` is overridden by
    // `x(double timeInSeconds)` -- and matching on the name called that a
    // widening and refused it, which left the trait unimplemented: 31 `E0046`s
    // for what is only a different word.
    final named = through == null
        ? null
        : {for (final p in through.params.where((p) => p.named)) p.name};
    final positional = through == null
        ? 0
        : through.params.where((p) => !p.named).length;
    // And the name to pass is the **caller's**, not the callee's. The
    // signature being written is the trait's, so `time` is what is in scope;
    // passing the inherent method's `timeInSeconds` names nothing.
    var at = -1;
    final args = method.params.map((p) {
      if (!p.named) at++;
      if (through == null) return snake(p.name);
      final supplied = p.named ? named!.contains(p.name) : at < positional;
      if (supplied) {
        final from = p.named
            ? through.params.firstWhere((q) => q.named && q.name == p.name)
            : through.params.where((q) => !q.named).elementAt(at);
        // A trait parameter doubled to `Option<Option<..>>` arrives one
        // `Option` deeper than the inherent method takes it.
        final traitType = _substituteType(from.type, _implBinding);
        final doubled = traitType.name == 'Option';
        final flattened = doubled && traitType.arguments.length == 1
            ? IrType(
                traitType.arguments.single.name,
                nullable: true,
                arguments: traitType.arguments.single.arguments,
              )
            : traitType;
        // The argument as the trait typed it, into the inherent method's
        // parameter, by the one rule (`coerceInto`): a widened override
        // (`equals(Object? e1, ..)` under `Equality<E>.equals(E, ..)`) is
        // shared into `Object`, a covariant one (`RenderClipRect` under
        // `RenderObject`) downcast, an erased bound narrowed to the body's
        // trait, an `Option` put on.
        final IrExpr passed = doubled
            ? (IrLiteral('${snake(from.name)}.flatten()', const IrType('raw'))
                ..rustType = flattened)
            : (IrLocal(from.name)..rustType = flattened);
        return expr(
          coerceInto(
            passed,
            _selfBound(_inThisClassTerms(p.type, via)),
            _world,
          ),
        );
      }
      // The override's own default is the value the base "has no value for".
      final fallback = p.defaultValue;
      if (fallback != null) return expr(fallback);
      // A `dynamic` (an `Object?`) has no value as the `Null` object.
      if (p.type.name == 'dynamic' && !p.type.nullable) {
        return 'dart_null_object()';
      }
      if (p.type.nullable) return 'None';
      throw Unsupported(
        'override widens `${method.name}` with `${p.name}`, '
            'which the base has no value for',
        '${cls.name}.${method.name}',
      );
    }).toList();
    final op = method.operator;
    if (op != null && operatorTraits.containsKey(op)) {
      // `std::ops` takes its operands by value, so `*self` moves -- which is
      // free for a `Copy` struct and an error for one that is not. A class
      // carrying an identity token is never `Copy` (the token is an `Rc`),
      // and `BorderRadius * other` said so: "cannot move out of `*self`
      // which is behind a shared reference" (ws1064).
      final own = cls.identityToken ? 'self.clone()' : '*self';
      if (op == 'unary-') return '-$own';
      return '$own $op ${args.single}';
    }
    // `Type::method(self, ...)`, not `self.method(...)`. Inside `impl Base for
    // This` the trait's own method has the same name, and `self.method(...)`
    // leans on Rust preferring the inherent one -- true today, and an infinite
    // recursion the moment the inherent one is not emitted. The explicit path
    // says which one is meant.
    // A setter's inherent name is `set_x` (see `_methodName`): the trait's
    // `set__status` forwarded to `Value::_status`, which is the getter.
    final name = op == null
        ? (method.isSetter ? 'set_${snake(method.name)}' : snake(method.name))
        : _operatorName(op);
    // An inherent method that takes `self: &Rc<Self>` (`_receiverOf`) is
    // reached from the trait's `&self` through the stored handle (1297
    // "expected `&Rc<X>`, found `&X`" at ws276).
    final receiver = cls.counted && _handles.contains(_rustName(method))
        ? '&self.__self.get()'
        : 'self';
    // A generic method's type parameters go along: the forwarder declares
    // the trait's, and the inherent one it reaches names its own only in
    // its result (`getElementForInheritedWidgetOfExactType<T>()`, 36
    // "cannot infer type of the type parameter `T`" at ws397).
    final generics = through?.typeParameters ?? method.typeParameters;
    final fish = generics.length == method.typeParameters.length
        ? _turbofish([for (final g in generics) IrType(g)])
        : '';
    final call =
        '${via == null ? '${cls.name}${_selfTurbofish()}' : _implementedAs(via)}::$name$fish(${[receiver, ...args].join(', ')})';
    // An inherent method the analysis typed `Never` (`throw
    // UnimplementedError()` for a body) returns `Result<Infallible, E>`;
    // the trait's signature wants its own `T`, which the impossible value
    // maps into (`_UnspecifiedTextScaler.clamp`, ws503).
    if (method.returnType.name == 'Never') {
      return '$call.map(|__never| match __never {})';
    }
    // An `async fn` yields its own future type; the trait wants the boxed
    // one every `Future<T>` is here (`_NativeCodec::get_next_frame(self)`).
    return call;
  }

  /// A lazy `late` field's accessor (`_lazyLate`): the field filled on
  /// the first read, through whatever handle holds the object. A read
  /// from another object (`it._paintOrderIterable` in `_TheaterParentData`)
  /// has no `self` to run the initialiser on, and read the empty cell
  /// (run653's render walk).
  void _emitLazyAccessors() {
    final wanted = _foreignReadsOf(cls.name);
    for (final f in _allFields(cls)) {
      // Only where some body reads it from outside: an accessor nobody
      // calls is a body that may not compile for nothing (+4 at ws654).
      if (!_lazyLate(f) || !wanted.contains(f.name)) continue;
      _member('${cls.name}.${f.name}', () {
        _here = '${cls.name}.${f.name}';
        final savedFailure = _failure;
        final savedReturns = _rustReturns;
        final savedAsync = _asyncBody;
        final savedParams = _methodTypeParams;
        final savedReassigned = _reassigned;
        _failure = _resultModel ? _error : null;
        _asyncBody = false;
        _methodTypeParams = const [];
        _reassigned = {};
        final held = _declSpelling(() => type(f.type));
        final returns = _wrapped(held);
        _rustReturns = returns;
        _line('pub fn ${_lazyAccessor(f.name)}(&self) -> $returns {');
        _indent++;
        final read = _lazyRead(f, 'self');
        _line(_failure != null ? 'Ok($read)' : read);
        _indent--;
        _line('}');
        _line('');
        _failure = savedFailure;
        _rustReturns = savedReturns;
        _asyncBody = savedAsync;
        _methodTypeParams = savedParams;
        _reassigned = savedReassigned;
      });
    }
  }

  static String _lazyAccessor(String field) => '__lazy_${snake(field)}';

  /// Whether a class or a base of it writes its own `hashCode`.
  bool _declaresHashCode(IrClass c) =>
      c.methods.any((m) => m.name == 'hashCode' && !m.isStatic) ||
      _abstractAncestors(
        c,
      ).any((a) => a.methods.any((m) => m.name == 'hashCode' && !m.isStatic)) ||
      _superclassChain(
        c,
      ).any((a) => a.methods.any((m) => m.name == 'hashCode' && !m.isStatic));

  /// The concrete superclasses above `c`, nearest first.
  Iterable<IrClass> _superclassChain(IrClass c) sync* {
    var name = c.superclass;
    final seen = <String>{};
    while (name != null && seen.add(name)) {
      final above = library[name];
      if (above == null) return;
      yield above;
      name = above.superclass;
    }
  }

  /// The fields of `owner` some body in the program reads on another
  /// object (`_WalkSelf.foreignFieldReads`), over every module's classes.
  Set<String> _foreignReadsOf(String owner) =>
      _foreignReads.putIfAbsent(owner, () {
        final found = <String>{};
        final classes = <IrClass>{
          ...library.elsewhere.values,
          ...library.classes,
        };
        for (final c in classes) {
          final reads = _foreignReadsIn(c)[owner];
          if (reads != null) found.addAll(reads);
        }
        return found;
      });

  final Map<String, Set<String>> _foreignReads = {};

  static final _foreignReadsOfClass = Expando<Map<String, Set<String>>>();

  static Map<String, Set<String>> _foreignReadsIn(IrClass c) {
    final cached = _foreignReadsOfClass[c];
    if (cached != null) return cached;
    final walk = _WalkSelf();
    for (final m in c.methods) {
      walk.statement(m.body);
    }
    for (final k in c.constructors) {
      final body = k.body;
      if (body != null) walk.statement(body);
    }
    for (final f in c.fields) {
      final init = f.initial;
      if (init != null) walk.expression(init);
    }
    return _foreignReadsOfClass[c] = walk.foreignFieldReads;
  }

  /// Whether `owner`'s field `name` is a lazy `late` (see `_lazyLate`),
  /// read through its accessor from outside.
  bool _lazyFieldOf(String owner, String name) {
    final owned = library[owner];
    if (owned == null) return false;
    for (final f in _allFields(owned)) {
      if (f.name != name) continue;
      return f.isLate &&
          f.initial != null &&
          _mentionsThis(f.initial!) &&
          _inCellOf(owned, f);
    }
    return false;
  }

  void _emitConstructors() {
    for (final ctor in cls.constructors) {
      // Through `_member`, like every other member. Without it an
      // `Unsupported` from one constructor came out of `_emitStruct` and took
      // the **whole class** with it -- 410 classes that vanished because one
      // field was `late`. That is round 21's lesson, at a site it never
      // reached: the unit of refusal has to be the unit of work.
      _member(
        '${cls.name}.${ctor.name ?? "new"}',
        () => _emitConstructor(ctor),
      );
    }
  }

  /// Whether a constructor is written as a `const fn`: see the note at
  /// `constness` in `_emitConstructor`. Through a redirect chain, each
  /// step's own rule and the target's (`seen` stops a cycle).
  bool _constCtor(IrConstructor ctor, Set<IrConstructor> seen) {
    if (!seen.add(ctor)) return false;
    if (!ctor.isConst ||
        ctor.body != null ||
        !ctor.params.every((p) => _isCopy(type(p.type))) ||
        ctor.fieldInits.values.any((e) => expr(e).contains('.clone()'))) {
      return false;
    }
    final redirect = ctor.redirectTo;
    if (redirect == null) return true;
    final target = cls.constructors
        .where((c) => (c.name ?? '') == redirect)
        .firstOrNull;
    return target != null && _constCtor(target, seen);
  }
}
