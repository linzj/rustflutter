part of '../backend_rust.dart';

// The entry point, failure propagation, `await` and block values.
augment class RustBackend {
  // -- Expressions ------------------------------------------------------------

  String expr(IrExpr e) {
    return switch (e) {
      // A `null` whose slot is known is a `None` of that type: type flow
      // analysis folds an always-null value into the literal, and the
      // `None` left behind had nothing to infer from -- `None.as_ref()
      // .map(|it| ..)` was E0282 (8 "type annotations needed" at ws762).
      IrLiteral(type: final literalType)
          when literalType.name == 'Null' &&
              e.rustType != null &&
              isNullable(e.rustType!) &&
              !e.rustType!.projected &&
              !_mentionsUnknown(e.rustType!) &&
              e.rustType!.name != 'Null' &&
              e.rustType!.name != 'dynamic' =>
        'None::<${type(nonNull(e.rustType!))}>',
      IrLiteral(:final value, :final type) => _literal(value, type),
      // A captured shared field is a cell handle, not the value: reading it
      // is `f.get()`. The local is only a local in the closure's own text.
      // A local a closure captured is the closure's own copy, and a `Fn`
      // closure may not give it away: read as a clone, so a use by value
      // (`instance.on_start = on_start`) moves the clone (107 E0507).
      IrLocal(:final name) =>
        _cellLocals.containsKey(name)
            ? '${_cellLocals[name]! ? '${snake(name)}.get()' : '{ let __r = ${snake(name)}.borrow().clone(); __r }'}'
                  '${_lateCellLocals.contains(name) ? '.unwrap()' : ''}'
            : _closureCaptured.contains(name)
            ? '${snake(name)}.clone()'
            : snake(name),
      // `this` in a counted class is the handle -- one more `Rc`, not the
      // value behind it. `*self` there moved out of a `&Rc<Self>`, and the
      // getter `get owner => this` came out returning a bare struct where
      // every other module spells that class `Rc<..>`: 18 `E0053`s.
      // `Matrix3.copy(this)` in `clone()`: `*self` moves out of a shared
      // reference unless the class is `Copy`.
      // In a constructor `this` is the local being built (`__new`), a
      // value and not a reference: no `*`.
      // `this_` is a `&__Self` whatever the mode: its clone is a reference
      // (91 lifetime errors at ws334), its handle is `dart_self_<trait>()`.
      IrThis() =>
        _selfByValue
            ? _selfName
            : _fieldsAreAccessors || _selfName == 'this_'
            ? '$_selfName.dart_self_${snakeRaw(cls.name)}()'
            // A counted object as a value is its own handle: a clone of
            // the struct would be a second object sharing one `DartSelf`
            // (`_RenderObjectSemantics(this)` in a lazy initializer,
            // run460).
            : cls.counted
            ? '$_selfName.dart_self_ref().get()'
            : _selfIsHandle || !_classIsCopy(cls, {}) || _selfName != 'self'
            ? '$_selfName.clone()'
            // Parenthesised: as the receiver of a call the bare `*` binds
            // to the call's result -- `*self.dart_to_string()` derefs the
            // `String` (`GalleryDemoCategory.displayTitle` returning
            // `toString()`, run732).
            : '(*$_selfName)',
      IrField(:final target, :final name, :final onEnum, :final owner) =>
        _fieldRead(target, name, onEnum, owner),
      IrStatic(:final owner, :final name, :final isEnumValue) => _staticRead(
        owner,
        name,
        isEnumValue,
      ),
      // A comparison has no expected type: an operand shared into
      // `Object` for it says so (`Some(Rc::new("dark"))` against an
      // `Option<Rc<dyn Object>>` scrutinee in a pattern switch was an
      // `Option<Rc<String>>`, `_updateUserSettingsData`, run472).
      IrBinary(:final op, :final left, :final right, :final type) => _binary(
        op,
        op == '==' || op == '!=' ? _explicitUpcast(_plain(left)) : left,
        op == '==' || op == '!=' ? _explicitUpcast(_plain(right)) : right,
        type,
      ),
      IrUnary(:final op, :final operand) => '($op${expr(operand)})',
      IrCall(
        :final target,
        :final name,
        :final args,
        :final qualifier,
        :final receiverClass,
        :final fails,
        :final diverges,
        :final typeArguments,
      ) =>
        _diverging(
          _call(
            target,
            name,
            args,
            qualifier: qualifier,
            receiverClass: receiverClass,
            fails: fails && !diverges,
            typeArguments: typeArguments,
            asyncFn: e.asyncFn,
            asyncTarget: e.asyncTarget,
            resultType: e.rustType,
          ),
          diverges && fails,
        ),
      IrStaticCall(
        :final owner,
        :final name,
        :final args,
        :final fails,
        :final diverges,
        :final typeArguments,
        :final module,
      ) =>
        _diverging(
          _staticCallFailing(
            owner,
            name,
            args,
            fails && !diverges,
            typeArguments,
            e.asyncFn,
            module,
          ),
          diverges && fails,
        ),
      IrNew(:final type, :final args, :final constructor) => _newFailing(
        type,
        args,
        constructor,
      ),
      // Parenthesised: a struct literal is not allowed bare in an `if`
      // condition, and `if self._state == _State { .. } {` did not parse.
      // ..and a counted class's constant is its handle, as its
      // constructor's result is (`const StandardMethodCodec()` holding a
      // `StandardMessageCodec`, 12 `Rc<X> <= X` at ws421).
      IrConstInstance(:final type, :final fields) =>
        (library[type.name]?.counted ?? false)
            ? 'dart_rc(${_constInstance(type, fields)})'
            : '(${_constInstance(type, fields)})',
      // Rust puts it after the expression and Dart before it, which is the
      // whole of the difference.
      // The future's output is a `Result`: the `?` goes after the await.
      // `await f` on a `Future<T>?`: null stays null (`await proxy.send(..)`
      // where `send` returns `Future<ByteData?>?`).
      // ..and `T?` of a `T` already nullable is `T`: awaiting a
      // `Future<ByteData?>?` is a `ByteData?`, not an `Option<Option<..>>`
      // (`BinaryMessenger.send`, ws474).
      // ..matched on a copy of the handle when the operand is a place:
      // matching the local itself moved the future out of it, and the
      // line after read it again (`loadFontIfNecessary`, ws562).
      // ..and awaiting one is that operand too: `()` is no future, and
      // the future it stands for was never made
      // (`_ContrastEvaluation._evaluate`, ws965).
      IrAwait(:final operand) when _neverReturns(operand) => expr(operand),
      IrAwait(:final operand)
          when operand.rustType?.name == 'Future' &&
              (operand.rustType?.nullable ?? false) =>
        (operand.rustType!.arguments.isNotEmpty &&
                operand.rustType!.arguments.first.nullable)
            ? '(match ${_awaitedPlace(operand)} { Some(__f) => __f.await$_propagate, None => None })'
            : '(match ${_awaitedPlace(operand)} { Some(__f) => Some(__f.await$_propagate), None => None })',
      // A class that *is* a `Future` (`TickerFuture implements Future<void>`)
      // is awaited through the `dart_into_future` its own `then` backs
      // (`_emitAwaitable`): `Rc<TickerFuture>` is no Rust future, and Dart's
      // `await` on any future is a `then` (E0277, 3 on the animation path at
      // ws972).
      // ..and a nullable one is asked first, as a nullable prelude future is
      // just above: `await x?.forward()` awaits nothing when there is
      // nothing there.
      IrAwait(:final operand)
          when _awaitedClass(operand) != null &&
              (operand.rustType?.nullable ?? false) =>
        '(match ${_awaitedPlace(operand)} '
            '{ Some(__f) => Some(__f.dart_into_future().await$_propagate), '
            'None => None })',
      IrAwait(:final operand) when _awaitedClass(operand) != null =>
        '${_awaitOperand(operand)}.dart_into_future().await$_propagate',
      IrAwait(:final operand) => '${_awaitOperand(operand)}.await$_propagate',
      IrMutRef(:final place) => _mutRef(place),
      IrIdentical(:final left, :final right) => _identical(left, right),
      // `return Err(e)` has type `!`, so it fits where a value was wanted.
      IrThrowValue(:final value) => _thrown(value),
      IrInterpolation(:final parts) => _interpolation(parts),
      // Dart indexes with an `int`; Rust wants a `usize`.
      // A clone: an indexed read is a value, and the element is behind the
      // list's reference (`cannot move out of index of Vec<..>`).
      // The target in parentheses when it is a block: `{ .. }[0]` reads
      // as a block statement and an array (`[{integer}; 1]`, ws511).
      IrIndex(:final target, :final index) => () {
        // The collection *in* its cell, borrowed: a read of the place
        // hands out a clone of the whole collection, and `_listeners[i]`
        // in `ChangeNotifier.removeListener`'s loop cloned the entire
        // listener list once per listener -- `addListener` did it again
        // per element while growing. The unmount walk of one page never
        // finished (run792 timed out before its first line of output).
        // The index bound first, so a read inside it happens before the
        // borrow, as `IrIndexSet` binds it.
        final place = _readPlace(target);
        if (place != null) {
          // ..and the clone bound before the block ends (`let __r`), so the
          // `Ref` the borrow made is dropped here rather than living to the
          // end of the enclosing statement. A block's tail temporaries are
          // extended to that statement, and where the cell is a local of a
          // shorter scope -- a closure prologue's `let _available_products =
          // ..` -- the borrow outlived the thing borrowed
          // (`AppStateModel.subtotalCost`, E0597). The map read standing next
          // to it in that same expression has been written this way all
          // along.
          return '{ let __i = ${expr(index)} as usize; '
              'let __r = $place[__i].clone(); __r }';
        }
        final t = expr(target);
        final wrapped = t.startsWith('{') ? '($t)' : t;
        return '$wrapped[${expr(index)} as usize].clone()';
      }(),
      // A closure literal among the elements of a list of functions is an
      // `Rc<dyn Fn>` there, as a field's or a constant's is: `DateFormat`'s
      // `_fieldConstructors` is a `vec!` of three of them.
      // A `vec![..]` is typed by its *first* element: an implicit upcast
      // there is spelled (`Rc::new(x) as Rc<dyn Object>`), or the second
      // element's other class does not fit (`Object.hashAll([isChecked,
      // isButton])`, 17 at ws421). The rest coerce to the first.
      // ..and one whose elements can fail is built element by element:
      // in `vec![a?, b?, ..]` every `?` exit drops every earlier element,
      // and a literal of 3038 failing constructors (the gallery's code
      // viewer) is 4.6 million drops of codegen -- one function held
      // `rustc` for half an hour at 27 GB (run429). Pushed one at a time,
      // a failure drops the one partial `Vec`.
      // ..with the element type spelled where it is known: `Vec::new()`
      // took the first push's type -- one closure's, which the next
      // closure was not (intl's `verifiedLocale`, run591).
      IrListLiteral(:final elements, :final element)
          when elements.isNotEmpty && _WalkSelf.failingIn(elements) =>
        '{ let mut __v${_mentionsUnknown(element) ? '' : ': Vec<${type(element)}>'} = Vec::new(); '
            '${elements.indexed.map((ix) => '__v.push(${_listElement(ix.$1, ix.$2, element)});').join(' ')}'
            ' __v }',
      IrListLiteral(:final elements, :final element) =>
        'vec![${elements.indexed.map((ix) => _listElement(ix.$1, ix.$2, element)).join(', ')}]',
      IrRecord(:final fields) => '(${fields.map(expr).join(', ')})',
      // A field read out of a record held in a place is a copy of it: by
      // value it moved the `TextTheme` out of the tuple the pattern's
      // cache temporaries read twice (`Typography._withPlatform`, run565).
      // ..and out of anything but a record built right here: a closure's
      // parameter is a reference (`|it| it.0` under `as_ref().map`), and a
      // field read out of one moves (`SelectionOverlay.showToolbar`, E0507
      // at ws754). A clone of a handle is a count, and of a `Copy` field
      // nothing.
      IrRecordField(:final record, :final index) =>
        record is IrRecord
            ? '${expr(record)}.$index'
            : '${expr(record)}.$index.clone()',
      // Spells its key and value types: nothing else says them when the
      // slot is an `Rc<dyn Object>` (E0283, `K` on `Map`), and a written
      // one typed by its first entry alone was untyped where that entry's
      // value is `null` (`{'a': null, 'b': 2}` into `Map<Object?, Object?>`,
      // ws495). Through `from_pairs`, whose array parameter is of the
      // spelled types, so every entry coerces to them; the first entry's
      // upcasts are spelled all the same.
      IrMapLiteral(:final entries, :final key, :final value) =>
        'Map::<${type(key)}, ${type(value)}>::from_pairs(['
            '${entries.indexed.map((ie) {
              final e = ie.$1 == 0 ? (_explicitUpcast(ie.$2.$1), _explicitUpcast(ie.$2.$2)) : ie.$2;
              return '(${expr(e.$1)}, ${expr(e.$2)})';
            }).join(', ')}'
            '])',
      // `for_each` consumes the chain and yields `()`: the one chain that is
      // whole without a `collect`.
      IrIterChain(:final steps)
          when steps.isNotEmpty && steps.last.$1 == 'for_each' =>
        _chain(e as IrIterChain),
      // ..any other chain used as a value is collected: the prelude's
      // `Iterable` is a `Vec` (see `EmptyIterable`), so what Dart keeps
      // lazy is eager here, and what was refused as "never collected"
      // (`Future.wait(pendingList.map(..))` in `_loadAll`, run578) is the
      // list it would have made.
      IrIterChain(:final steps) => _chain(
        e as IrIterChain,
        tail:
            '${steps.isNotEmpty && steps.last.$1 == 'filter' ? '.cloned()' : ''}'
            '.collect::<Vec<_>>()',
      ),
      // Boxed, because a function item is not a `Box<dyn Fn>` and that is what
      // a function-typed field or local is here. A `Box<dyn Fn>` also
      // implements `Fn`, so it still passes where `impl Fn` is wanted.
      IrFunctionRef(:final owner, :final name) => _functionRef(
        owner,
        name,
        e.rustType,
      ),
      // ..through the local's cell when it has one (`fired++` in a closure
      // that captured `fired`, the nullmut fixture), as `IrAssign` writes.
      IrAssignValue(:final name, :final value) =>
        // The stored copy is a clone: a non-`Copy` value moved into the
        // local was gone by the time the expression yielded it (E0382, 17).
        switch (_cellLocals[name]) {
          null =>
            '{ let __set = ${expr(value)}; ${snake(name)} = __set.clone(); __set }',
          true =>
            '{ let __set = ${expr(value)}; ${snake(name)}.set(${_lateCellLocals.contains(name) ? 'Some(__set.clone())' : '__set.clone()'}); __set }',
          false =>
            '{ let __set = ${expr(value)}; *${snake(name)}.borrow_mut() = ${_lateCellLocals.contains(name) ? 'Some(__set.clone())' : '__set.clone()'}; __set }',
        },
      IrSetValue(:final target, :final name, :final value) => _setValue(
        target,
        name,
        value,
      ),
      // The branches have no expected type from each other: an upcast in
      // one is explicit (`dart_object(FontWeight)` against
      // `Rc::new("unspecified")`, ws476).
      // A condition that asks whether a `null` the lowering itself wrote is
      // null: Dart answered that when it wrote it, and the other arm is
      // dead. Emitted whole, the dead arm still has to type, and a bare
      // `None` standing in it types nothing ("cannot infer type of the
      // type parameter `T` declared on the enum `Option`", E0282:
      // `_WidgetStateTextStyle.new`, where `TextStyle`'s constructor is
      // inlined with `package` omitted and computes
      // `'packages/$package/$fontFamily'` in the branch never taken, ws971).
      IrConditional(condition: IrIsNull(:final operand), :final then)
          when _writtenNull(operand) =>
        expr(_explicitUpcast(then)),
      // ..typed where one arm never arrives: `if c { None } else {
      // unreachable!() }` leaves `None`'s `T` to a never-type fallback
      // (`Object.hash(.., stops == null ? null : hashAll(stops!), ..)`
      // with the second arm removed by TFA, ws589).
      IrConditional(:final condition, :final then, :final otherwise)
          when (_diverges(then) || _diverges(otherwise)) &&
              e.rustType != null &&
              !_mentionsUnknown(e.rustType!) &&
              !_mentionsNever(e.rustType!) =>
        '{ let __c: ${type(e.rustType!)} = if ${expr(condition)} { ${expr(_explicitUpcast(then))} } else { ${expr(_explicitUpcast(otherwise))} }; __c }',
      IrConditional(:final condition, :final then, :final otherwise) =>
        'if ${expr(condition)} { ${expr(_explicitUpcast(then))} } else { ${expr(_explicitUpcast(otherwise))} }',
      IrIs(expr: final operand, :final type, :final negated) => _isTest(
        operand,
        type,
        negated,
      ),
      // A super function returns `Result`; `Object.toString` is the prelude's.
      // An async super function is a `DartFuture`, not a `Result`: no `?`
      // (`super.handleSystemMessage(..)` in `WidgetsBinding`, ws446).
      IrSuperCall(
        :final base,
        :final name,
        :final args,
        :final isSetter,
        :final baseArguments,
        :final typeArguments,
      ) =>
        base == 'Object'
            ? _superCall(base, name, args)
            : '${_superCall(base, name, args, isSetter: isSetter, baseArguments: baseArguments, typeArguments: typeArguments)}${(library[base]?.methods.any((m) => m.name == name && !m.isStatic && m.isAsync) ?? false) ? '' : _propagate}',
      // A local's `!` clones first: `a!.axis` and then `a!.value` moved
      // `a` at the first (E0382); a `Copy` local clones for free.
      // `null!` is Dart's `TypeError`, not a value: there is nothing to
      // unwrap and no type to unwrap it at (`cacheExtent!` in a viewport
      // arm the AOT compiler proved dead, once the base's default `null`
      // was substituted in for it -- `None.unwrap()` could infer nothing,
      // run696).
      IrNullCheck(operand: IrLiteral(type: IrType(name: 'Null'))) =>
        'dart_null_check_failed()',
      IrNullCheck(:final operand) =>
        operand is IrLocal
            ? '${expr(operand)}.clone().unwrap()'
            : '${expr(_plain(operand))}.unwrap()',
      // A closure inside `Some(..)` is the `Rc<dyn Fn>` its slot holds.
      // A local crossing is cloned: a closure's parameter rebound in its
      // prologue (`_withEdgeParams`) may be the `&T` a prelude iterator
      // hands out (`items.map((T? x) => ..)`, fixture closureedge).
      IrNullableOf(:final value, :final parameter, :final toOption) =>
        '<${_nullableOf(parameter)} as DartNullable>::${toOption ? 'option' : 'from_option'}(${expr(value)}${value is IrLocal ? '.clone()' : ''})',
      IrSome(:final value) => _some(value),
      // Inside `as_ref().map(|it| ..)` the bound value is a reference, and
      // a reference does not cast: `lerpDouble`'s `a as double` on an
      // `Option<f64>` (E0606).
      IrCast(:final value, :final rust) =>
        value is IrBound && !_boundByValue
            ? '(*${expr(value)} as $rust)'
            : '(${expr(value)} as $rust)',
      // `state as T?` with `T` a type parameter: by id, and the `Option`
      // stays one (see `dart_cast_any`).
      // A cast of an operand that never returns is that operand, as a call
      // on one is: there is nothing to cast, and the cast's own type is
      // what rustc could not infer (`DropdownMenuThemeData
      // .inputDecorationTheme` and `DatePickerThemeData`'s, ws965).
      IrCastTo(:final target) when _neverReturns(target) => expr(target),
      IrDowncast(:final target) when _neverReturns(target) => expr(target),
      IrCastTo(:final target, :final type) when _isTypeParam(type.name) =>
        '${expr(target)}.dart_cast_any::<${type.name}>()'
            '${type.nullable ? "" : ".unwrap()"}',
      // A nullable target keeps the `Option` the cast hands back.
      IrCastTo(:final target, :final type) =>
        '${expr(target)}.dart_cast_to::<${_dynOf(type)}>()'
            '${type.nullable ? "" : ".unwrap()"}',
      IrSuperDispatch(
        :final receiver,
        :final base,
        :final name,
        :final args,
        :final typeArguments,
        :final classArity,
        :final castTo,
      ) =>
        _superDispatch(
          receiver,
          base,
          name,
          args,
          typeArguments,
          classArity,
          castTo,
        ),
      // A collection of `dynamic`/`Object?`/scalars: the object's own
      // element representation may differ (`Map<String, dynamic>` from
      // `json.decode` cast `as Map<String, Object?>`), and Dart's runtime
      // type does not tell them apart; the prelude converts (ws473).
      IrDowncast(:final target, :final type, :final arguments)
          when (type == 'Map' || type == 'List') &&
              arguments.isNotEmpty &&
              arguments.every(_dynamicRepresentable) =>
        '${type == 'Map' ? 'dart_cast_map' : 'dart_cast_list'}::<${arguments.map(this.type).join(', ')}>(&${_optionRead(target) ?? expr(target)}).unwrap()',
      // A type parameter: its own conversion (`FromDynamic`, in every
      // bound), as `!as_opt` above -- `Any` knows one concrete type, and a
      // `T` bound to `Rc<dyn Object>` is none.
      IrDowncast(:final target, :final type, :final arguments)
          when arguments.isEmpty && _isTypeParam(type) =>
        // The handle cloned first: the `as` consumes it, and a local read
        // twice (`m is T && m.supports(..)`) was moved (E0382).
        '<$type as FromDynamic>::from_dynamic(&(${expr(target)}.clone() as ${dartHandle})).unwrap()',
      // A counted class out of a `dynamic`: the object's own handle
      // (`dart_cast_any` at `Rc<Self>`), not a copy of the struct.
      IrDowncast(:final target, :final type, :final arguments)
          when (library[type]?.counted ?? false) =>
        '${expr(target)}.dart_cast_any::<std::rc::Rc<$type${arguments.isEmpty ? '' : '<${arguments.map(this.type).join(', ')}>'}>>().unwrap()',
      // A prelude exception class: through the prelude's hierarchy, as
      // `is` asks (`_isTest`), so a subtype's value reads as it.
      IrDowncast(:final target, :final type, :final arguments)
          when arguments.isEmpty &&
              library[type] == null &&
              _preludeClasses.contains(type) =>
        '<$type as DartCoreAs>::dart_core_as(&${_optionRead(target) ?? expr(target)}).unwrap()',
      IrDowncast(:final target, :final type, :final arguments) =>
        '${_asAny(target)}.downcast_ref::<${_downcastNames[type] ?? type}'
            '${_downcastArguments(type, arguments)}>().unwrap()',
      IrDynamicDispatch(:final receiver, :final arms) => _dispatch(
        receiver,
        arms,
      ),
      // A mutable one is read through its cell: two derefs for the `LazyLock`
      // and the `Isolate`, then a `borrow`.
      IrTopLevel(:final name, :final module) => () {
        final spelled = module == null
            ? screamingSnake(name)
            : 'crate::$module::${screamingSnake(name)}';
        return _isMutableTopLevel(name)
            ? '({ let __r = (**$spelled).borrow().clone(); __r })'
            : _isLazyConst(name)
            ? '(**$spelled).clone()'
            : spelled;
      }(),
      // `x == null` on a `dynamic`: the handle is never an `Option`; Dart's
      // null is the `Null` object inside it (`dart_nullable`).
      // ..and on a type parameter: through its projection, since a `T`
      // bound to a nullable type holds its null as the `Or` says
      // (`x == null` on a mixin's `T x`, `is_none` on a bare `T`).
      IrIsNull(:final operand) =>
        operand.rustType != null &&
                !operand.rustType!.nullable &&
                (operand.rustType!.name == 'dynamic' ||
                    operand.rustType!.name == 'Object')
            ? 'dart_nullable(${expr(operand)}.clone()).is_none()'
            : operand.rustType != null &&
                  !operand.rustType!.nullable &&
                  !operand.rustType!.projected &&
                  operand.rustType!.arguments.isEmpty &&
                  _isTypeParam(operand.rustType!.name)
            ? '<${operand.rustType!.name} as DartNullable>::is_dart_null(&${expr(operand)})'
            // ..and on a value of a concrete non-null type, Dart's static
            // answer: a mixin's `T x` copied into a class with `int` put
            // in asked `is_none` of an `i64` (ws525).
            : operand.rustType != null &&
                  !isNullable(operand.rustType!) &&
                  !operand.rustType!.projected &&
                  !operand.rustType!.isFunction &&
                  operand.rustType!.name != 'dynamic' &&
                  operand.rustType!.name != 'Object' &&
                  operand.rustType!.name != 'Null' &&
                  operand.rustType!.name != 'raw' &&
                  operand.rustType!.name != '_' &&
                  !_isTypeParam(operand.rustType!.name) &&
                  (scalarNames.contains(operand.rustType!.name) ||
                      library[operand.rustType!.name] != null)
            ? '{ let _ = &${expr(operand)}; false }'
            : '${expr(_plain(operand))}.is_none()',
      IrIfNull() => _ifNullProjected(e as IrIfNull),
      // `as_ref()`: `a?.b` reads `a`, and `a` is a field or a loop variable
      // behind a reference far more often than an owned `Option` -- `.map`
      // alone moved out of `*child` (E0507). A body that needs the value
      // rather than a reference to it now says so at the use.
      // Under the Result model the body may `?`: it runs inside a closure
      // returning `Result`, and `transpose()?` lifts the error out of the
      // `Option` again.
      // ..except a scalar, which is `Copy`: the body gets the value
      // itself, and `it as f64` needs no dereference (`a?.toDouble()` in
      // `lerpDouble`, ws473).
      IrNullAware(:final receiver, :final body, :final flatten) => _nullAware(
        receiver,
        body,
        flatten,
      ),
      // A counted class's constructor already hands out an `Rc`.
      IrMapElements(:final collection, :final kind, :final body) =>
        kind == 'Future'
            ? '${expr(collection)}.map(|v| ${_mappedBody(body)})'
            : kind == 'Iterator'
            ? 'dart_iterator_map(${expr(collection)}, |v| ${_mappedBody(body)})'
            : kind == 'Set'
            ? 'Set::of(${expr(collection)}.into_iter().map(|v| ${_mappedBody(body)}).collect::<Vec<_>>())'
            : kind == 'Map'
            ? 'Map::from(${expr(collection)}.into_iter().map(|(k, v)| ${_mappedBody(body)}).collect::<Vec<_>>())'
            : '${expr(collection)}.into_iter().map(|v| ${_mappedBody(body)}).collect::<Vec<_>>()',
      // `this` shared as an object is its own handle (`!as_object`).
      IrUpcast(:final value, :final type)
          when value is IrThis && type.name == 'Object' =>
        _call(value, '!as_object', const []),
      // The value's class by its recorded type first (a local, a call), by
      // its shape (a constructor call) otherwise.
      IrUpcast(:final value, :final type, :final handle, :final explicit) =>
        handle ||
                (library[value.rustType?.name ?? _concreteType(value).name]
                        ?.counted ??
                    false)
            ? (explicit
                  ? '(${_castOperand(value)} as ${this.type(type)})'
                  : _handleOf(value))
            // ..an enum too, since `_emitEnumDartAny` (ws510): registered
            // as it is boxed, so `dart_object_str` finds its `X.value`.
            : (library[value.rustType?.name ?? _concreteType(value).name] !=
                  null)
            ? (explicit
                  ? '(dart_object(${expr(value)}) as ${this.type(type)})'
                  : 'dart_object(${expr(value)})')
            // A core value behind a plain handle. An `int` literal spelled
            // as the `i64` it is: boxed bare, Rust typed `3` an `i32`, and
            // the object printed as one (the tostr fixture, ws543).
            // ..and into an `Object` slot, the object the value *is*: a
            // handle of a type only a parameter names (`V` bound to
            // `Rc<dyn InheritedElement>`) is not boxed a second time.
            : (type.name == 'Object' || type.name == 'dynamic')
            ? (explicit
                  ? '(dart_boxed${_boxedAs(value)}(${_boxedLiteral(value)}) as ${this.type(type)})'
                  : 'dart_boxed${_boxedAs(value)}(${_boxedLiteral(value)})')
            : (explicit
                  ? '(std::rc::Rc::new(${_boxedLiteral(value)}) as ${this.type(type)})'
                  : 'std::rc::Rc::new(${_boxedLiteral(value)})'),
      IrBound() => _boundName,
      IrClosure() => _closure(e as IrClosure),
      // A function value returns `Result` like everything else.
      IrCallValue(:final target, :final args) =>
        '(${expr(target)})(${args.map(expr).join(', ')})$_propagate',
      IrBlockValue() => _blockValue(e as IrBlockValue),
    };
  }

  /// Statements then a value, as a Rust block expression.
  ///
  /// The binding is `mut` only when a step writes to it, for the reason
  /// `let mut` is not applied everywhere: the test crate denies `unused_mut`,
  /// so an unneeded one is a build error rather than a warning nobody reads.
  /// A call to a `Never` function: its `Result<Infallible, E>` is either
  /// the error, propagated, or a value that cannot exist -- so the
  /// expression is the `!` Dart meant, whatever the slot wants.
  String _diverging(String call, bool diverges) => !diverges
      ? call
      : _failure != null
      ? '(match $call { Ok(__n) => match __n {}, Err(__e) => return Err(__e) })'
      : '(match $call { Ok(__n) => match __n {}, Err(__e) => panic!("uncaught Dart exception: {:?}", __e) })';

  String _staticCallFailing(
    String? owner,
    String name,
    List<IrExpr> args,
    bool fails, [
    List<IrType> typeArguments = const [],
    bool asyncFn = false,
    String? module,
  ]) {
    final awaited = _awaiting;
    _awaiting = false;
    // A top-level of another module this module shadows by name is spelled
    // by its module (see `IrStaticCall.module`).
    final call = owner == null && module != null
        ? 'crate::$module::${_staticCall(owner, name, args, typeArguments)}'
        : _staticCall(owner, name, args, typeArguments);
    final failing =
        fails || (_resultModel && _preludeFailingStatics.contains(name));
    return _asyncValue(
      failing && !awaited && !asyncFn ? '$call$_propagate' : call,
      asyncFn && !awaited,
    );
  }

  /// An `async fn` called and not awaited: its future is the value, boxed
  /// as every `Future<T>` is here (`return _handleCommitBackGesture()`,
  /// ws428). Not `?`ed: an `async fn` fails inside its future.
  String _asyncValue(String call, bool boxed) => call;

  /// A translated class's constructor returns `Result` like any function;
  /// the prelude's do not.
  String _newFailing(IrType t, List<IrExpr> args, String? constructor) {
    final awaited = _awaiting;
    _awaiting = false;
    final call = _new(t, args, constructor);
    final translated = _resultModel && library[t.name] != null;
    return translated && !awaited ? '$call$_propagate' : call;
  }

  /// The operand of an `await`. A call reaching an `async fn` as one
  /// (`asyncFn`) is the future itself and its `?` goes after the `.await`;
  /// any other failing call returns its future inside the `Result` -- a
  /// trait method, a plain function that built a `Future<T>` -- and is
  /// unwrapped first, `f()?.await?` (118 "is not a future" at ws425).
  /// A nullable future to match on: a place's handle cloned, so the place
  /// keeps it; a value as it is.
  String _awaitedPlace(IrExpr operand) {
    final text = _awaitOperand(operand);
    return operand is IrLocal || operand is IrField ? '($text).clone()' : text;
  }

  String _awaitOperand(IrExpr operand) {
    final asyncFn = switch (operand) {
      IrCall(:final asyncFn) => asyncFn,
      IrStaticCall(:final asyncFn) => asyncFn,
      _ => false,
    };
    if (asyncFn) {
      _awaiting = true;
      final text = expr(operand);
      _awaiting = false;
      return text;
    }
    return expr(operand);
  }

  String _blockValue(IrBlockValue node) {
    // A binding of a value the AOT compiler removed makes the whole block
    // unreachable, and a `let __t = unreachable!(..)` has no type for the
    // `Some(__t.clone())` after it (115 "type annotations needed").
    for (final statement in node.statements) {
      if (statement is IrLocalDecl) {
        final init = statement.init;
        if (init is IrLiteral && init.value.startsWith('unreachable!')) {
          return init.value;
        }
      }
    }
    final saved = _out.length;
    final savedIndent = _indent;
    final savedReassigned = _reassigned;
    _indent = 0;
    // A cascade's steps write fields of the binding, which is a write to the
    // local rather than a reassignment of it -- so `_assignedIn` does not see
    // it and the declaration has to be told separately.
    _reassigned = {
      ..._reassigned,
      if (node.statements.any(_writesTheBinding)) _cascadeBinding,
    };
    for (final statement in node.statements) {
      stmt(statement);
    }
    final body = _out.sublist(saved).map(_inlineSafe).join(' ');
    _out.removeRange(saved, _out.length);
    _indent = savedIndent;
    _reassigned = savedReassigned;
    // The block's value is a *place* when it is a local the block did not
    // bind: producing it moves out of it, and the place lives on. `let
    // #t0 = fallback in decorate` -- what the AOT compiler leaves of
    // `fallback ?? decorate` once it knows the left is null -- moved
    // `resolvedForegroundBuilder` out from under the rest of
    // `ButtonStyleButton.build`, which reads it three lines on (ws892).
    // The block's *own* binding still moves: it was made here, and
    // nothing outside can read it.
    final produced = node.value;
    final tail =
        produced is IrLocal &&
            !node.statements.any(
              (s) => s is IrLocalDecl && s.name == produced.name,
            )
        ? '${expr(produced)}.clone()'
        : expr(produced);
    return '{ $body $tail }';
  }

  /// The name the front ends give a cascade's receiver.
  static const _cascadeBinding = 'cascaded';

  bool _writesTheBinding(IrStmt s) => switch (s) {
    IrAssignField(:final target) =>
      target is IrLocal && target.name == _cascadeBinding,
    IrSetter(:final target) =>
      target is IrLocal && target.name == _cascadeBinding,
    _ => false,
  };
}
