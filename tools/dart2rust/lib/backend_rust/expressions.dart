part of '../backend_rust.dart';

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
      IrAwait(:final operand)
          when operand.rustType?.name == 'Future' &&
              (operand.rustType?.nullable ?? false) =>
        (operand.rustType!.arguments.isNotEmpty &&
                operand.rustType!.arguments.first.nullable)
            ? '(match ${_awaitedPlace(operand)} { Some(__f) => __f.await$_propagate, None => None })'
            : '(match ${_awaitedPlace(operand)} { Some(__f) => Some(__f.await$_propagate), None => None })',
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
          return '{ let __i = ${expr(index)} as usize; $place[__i].clone() }';
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
        '<$type as FromDynamic>::from_dynamic(&(${expr(target)}.clone() as std::rc::Rc<dyn Object>)).unwrap()',
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
        '${_asAny(target)}.downcast_ref::<${_downcastNames[type] ?? type}${arguments.isEmpty ? '' : '<${arguments.map(this.type).join(', ')}>'}>().unwrap()',
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
      IrIfNull() => _ifNull(_plainIfNull(e as IrIfNull)),
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
                  ? '(${_handleOf(value)} as ${this.type(type)})'
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
    return '{ $body ${expr(node.value)} }';
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

  /// A closure literal.
  ///
  /// The parameter types are written out rather than inferred: a closure passed
  /// straight into a call would usually infer, but one stored or returned would
  /// not, and a compiler that emits both spellings depending on where the
  /// closure lands is two rules where one will do.
  String _closure(IrClosure node) {
    // A parameter the body assigns is `mut`, as a method's is (E0384 on
    // `decodeError = ..` inside `_getNextFrame`'s callback).
    final assigned = _assignedIn(node.body);
    final params = node.params
        // Spelled as the function type spells them: a parameter of an abstract
        // class is `&dyn X` there, and a closure declaring `Rc<dyn X>` did not
        // match the `Fn(&dyn X)` it was handed to -- 133 `E0631`s.
        // ..except a `Future`, which as a borrowed `impl Future` is not
        // allowed in a closure's parameters (E0562); owned it is `Pin<Box<..>>`.
        .map(
          (p) =>
              '${assigned.contains(p.name) ? 'mut ' : ''}${snake(p.name)}: '
              '${type(p.type, owned: p.type.name == 'Future' || p.type.isFunction)}',
        )
        .join(', ');
    // A closure that copies `final` fields in is a `move` closure with the
    // copies bound just before it. It borrows `self` not at all, which is the
    // whole point: it outlives the call that made it.
    // A closure made inside a null-aware's body that reads the bound
    // value (`it`, a reference the `map` hands in) keeps its own clone and
    // moves it: an adapter made under `handler == null ? null : (m) async
    // {..}` borrowed `it` past the statement (E0716, ws486).
    final usesBound = (_WalkSelf()..statement(node.body)).readsBound;
    final bindings = [
      // The handle first: a closure that calls a method keeps the object.
      // The handle, not a clone of a reference: inside a trait body `this`
      // is `dart_self_<trait>()`, on a counted class `dart_self_ref().get()`
      // (`let __me = this_.clone()` captured a `&__Self` into a `'static`
      // closure, 91 lifetime errors at ws334).
      if (node.holdsSelf) 'let $_countedSelf = ${_selfHandle()};',
      // A lending local function inside a `&mut self` method: the closure
      // is a `move` one (it owns the locals it copied in), and a `&mut Self`
      // is not `Copy` -- so it moves `self` rather than borrowing it. A
      // reborrow bound here is what it moves instead, and it lasts exactly
      // as long as the closure (`_popPolicyDataIfNeeded`, ws838).
      if (_lendingClosure && _selfIsMut && !node.holdsSelf)
        'let $_lentSelf = &mut *$_selfName;',
      if (usesBound) 'let $_boundName = $_boundName.clone();',
      // `mut` when the body writes or lends the copy (`&mut keys` inside
      // `visitAncestorElements`'s callback, `PageStorageBucket._allKeys`).
      ...node.captures.map(
        (c) =>
            'let ${_assignedIn(node.body).contains(c.name) ? 'mut ' : ''}${snake(c.name)} = ${_copyOf(c)};',
      ),
      ...node.locals.map((l) => 'let ${snake(l)} = ${snake(l)}.clone();'),
    ].join(' ');
    // Which of them are cells, for the body that is about to be written.
    final savedCells = _cellLocals;
    _cellLocals = {
      ..._cellLocals,
      for (final c in node.captures)
        if (_sharedField(c.name) != null) c.name: _isCopy(type(c.type)),
    };
    // ..and which of those cells hold a `late` field: read unwrapped, as
    // the field is on the object (`_localizationsResolver` in
    // `WidgetsApp.build`'s closure, ws482).
    final savedLateCells = _lateCellLocals;
    _lateCellLocals = {
      ..._lateCellLocals,
      for (final c in node.captures)
        if (_sharedField(c.name) != null && _lateField(c.name) != null) c.name,
    };
    final saved = _out.length;
    final savedIndent = _indent;
    final savedSelf = _selfName;
    // A closure is a panic boundary: its own signature carries no `Result`,
    // whatever the method around it promised (`_thrown`).
    final savedFailure = _failure;
    final savedFlow = _inFlowClosure;
    // ..under the uniform Result model it does: a closure fails like a
    // function and says so in its type.
    _failure = _resultModel ? _error : null;
    // ..and a `try` inside it that returns carries the *closure's* value
    // out, not the enclosing method's (`registerExtension`'s callback in
    // `BindingBase.registerServiceExtension`, run485).
    final savedRustReturns = _rustReturns;
    final closureReturns = node.isAsync ? _awaited(node.returns) : node.returns;
    _rustReturns = _resultModel && closureReturns.name != 'raw'
        ? 'Result<${type(closureReturns)}, $_error>'
        : _rustReturns;
    // ..and so does the *declared* return `_returned` wraps against: left
    // at the enclosing method's, a closure returning a concrete class got
    // that method's trait around it -- `getIcon: (context) => Icons.menu`
    // inside a `Widget build` was `dart_object(IconData::new(..)) as
    // Rc<dyn Widget>` (`_ActionIcon`, 4 at ws751).
    final savedReturns = _returns;
    _returns = closureReturns;
    // Nor is it inside the try body's flow closure: a `return` in it is
    // the closure's own (`Ok(Some(..))` in `|x| builder.setDay(x)`).
    _inFlowClosure = false;
    // ..and the reborrow is what `this` is inside that closure.
    final lentSelf = _lendingClosure && _selfIsMut && !node.holdsSelf;
    final savedLendingBody = _lendingClosure;
    _lendingClosure = false;
    if (node.holdsSelf) {
      _selfName = _countedSelf;
    } else if (lentSelf) {
      _selfName = _lentSelf;
    }
    final savedCaptured = _closureCaptured;
    _closureCaptured = {
      ..._closureCaptured,
      for (final c in node.captures)
        if (_sharedField(c.name) == null) c.name,
      ...node.locals,
    };
    _indent = 0;
    // The body's own asyncness: a `try` inside an `async` closure of a
    // sync method wrapped its body in a closure that cannot `await`
    // (`setMessageHandler`'s handler, E0728 at ws461).
    final savedAsyncBody = _asyncBody;
    _asyncBody = node.isAsync;
    // A closure of its own is a slot's value again: its return type comes
    // from the `Rc<dyn Fn(..)>` it goes into, so the upcasts inside it are
    // left to Rust as they were (see `_spellsReturn`).
    final savedSpells = _spellsReturn;
    _spellsReturn = false;
    _body(node.body, node.isAsync ? _awaited(node.returns) : node.returns);
    _spellsReturn = savedSpells;
    _asyncBody = savedAsyncBody;
    _failure = savedFailure;
    _rustReturns = savedRustReturns;
    _returns = savedReturns;
    _inFlowClosure = savedFlow;
    _selfName = savedSelf;
    _lendingClosure = savedLendingBody;
    _closureCaptured = savedCaptured;
    final body = _out.sublist(saved).map(_inlineSafe).join(' ');
    _out.removeRange(saved, _out.length);
    _indent = savedIndent;
    final owns =
        node.captures.isNotEmpty ||
        node.locals.isNotEmpty ||
        node.holdsSelf ||
        usesBound;
    // `async |..|` is stable since Rust 1.85. A Dart `async` closure keeps
    // its `await`s, and a closure emitted without the word put every one of
    // them outside an async context: 79 `E0728`s.
    // Annotated: a `?` inside needs the error type spelled, and the body
    // ends in `Ok(..)`.
    // The value type is left to inference: naming it pulled types into
    // modules that never imported them (248 "cannot find type"), and a
    // closure signature may say `_`. The error type is what `?` needs.
    // An `async` closure is a plain closure returning the spawned future
    // of its body, as an `async` function is (`_emitAsyncWrapper`): the
    // captures it owns are cloned again inside, since the body moves them
    // into a `'static` future and a `Fn` closure keeps its own.
    final again = [
      if (node.holdsSelf) 'let $_countedSelf = $_countedSelf.clone();',
      ...node.captures.map(
        (c) => 'let ${snake(c.name)} = ${snake(c.name)}.clone();',
      ),
      ...node.locals.map((l) => 'let ${snake(l)} = ${snake(l)}.clone();'),
    ].join(' ');
    // An async closure is a function value like any other, returning
    // `Result`: its future inside `Ok` where the slot wants the future
    // (`Fn(..) -> Result<DartFuture<T>, E>`), and `Ok(())` after spawning
    // it where the slot wants nothing (`void Function(..)` handed an
    // `async` closure, `setMessageHandler`'s at run459). A bare
    // `-> DartFuture<_>` matched neither (12 stubs on the start path).
    final spawned =
        'DartFuture::spawn_named("${cls.name} closure", std::boxed::Box::pin(async move { $body }))';
    final wantsFuture =
        node.returns.name == 'Future' || node.returns.name == 'FutureOr';
    // ..and where the slot says `FutureOr<T>`, the future as that
    // (`Future<bool>(() async {..})` in `GetStorage`, ws470: the stub that
    // kept `main` waiting without a word).
    final asFutureOr = node.returns.name == 'FutureOr';
    // ..and where it says `Future<T>?` (a `MessageHandler`'s
    // `Future<ByteData?>? Function(ByteData?)`), the future inside `Some`
    // (`setMessageHandler`'s closure, ws474).
    final asNullable = node.returns.name == 'Future' && node.returns.nullable;
    final closure = node.isAsync
        ? (wantsFuture
              ? (asFutureOr
                    ? '${owns ? 'move ' : ''}|$params| -> Result<FutureOr<_>, $_error> { $again Ok(FutureOr::future($spawned)) }'
                    : asNullable
                    ? '${owns ? 'move ' : ''}|$params| -> Result<Option<DartFuture<_>>, $_error> { $again Ok(Some($spawned)) }'
                    : '${owns ? 'move ' : ''}|$params| -> Result<DartFuture<_>, $_error> { $again Ok($spawned) }')
              : '${owns ? 'move ' : ''}|$params| -> Result<_, $_error> { $again let _ = $spawned; Ok(()) }')
        : '${owns ? 'move ' : ''}|$params|${_resultModel ? ' -> Result<${_closureReturnSpelled(node.returns)}, $_error>' : ''} { $body }';
    _cellLocals = savedCells;
    _lateCellLocals = savedLateCells;
    final whole = owns ? '{ $bindings $closure }' : closure;
    if (!node.boxed) return whole;
    // Unsized to the function type it is typed as, where that is
    // spelled: inside a `.map(|__f| ..)` there is no slot to infer
    // `Rc<dyn Fn>` from, and the `Rc<{closure}>` stayed one (a
    // conditional tear-off into `VoidCallback?`, ws549).
    // Plain: the slot it lands in unsizes it. Spelling the handle type
    // here (an `as`, then a typed `let`) named type parameters out of
    // scope and turned iterator closures into handles (+49 at ws551);
    // the one place with no slot to infer from, a null-aware `.map`,
    // spells its own return (`_nullAware`).
    return 'std::rc::Rc::new($whole)';
  }

  /// A field's type, wrapped when a closure has to see it change.
  ///
  /// `Rc<Cell<T>>` where `T` is `Copy` and `Rc<RefCell<T>>` where it is not:
  /// `Cell` needs no borrow flag and cannot panic, so it is the better answer
  /// wherever it fits. See `IrFieldDecl.shared`.
  String _fieldType(IrFieldDecl field) {
    final held = _heldType(field);
    // Every mutable field of a counted class, not only the ones a closure
    // names: an `Rc` hands out shared *immutable* access, so a method that
    // assigns a field cannot take `&mut self` and has to go through a cell.
    if (!_inCell(field)) return held;
    final cell = _isCopy(held) ? 'Cell' : 'RefCell';
    return 'std::rc::Rc<std::cell::$cell<$held>>';
  }

  /// Whether a field of *this* class is shared. Named rather than passed
  /// around: reads and writes reach it from several places.
  /// Whether a field is held in a cell: marked shared, or mutable in a
  /// counted class.
  bool _inCell(IrFieldDecl field) => _inCellOf(cls, field);

  /// ..for a field of `owner`, which a constant instance of another class
  /// needs to know (`MaterialColor { _swatch: .. }`, 81 at ws273).
  bool _inCellOf(IrClass owner, IrFieldDecl field) =>
      field.shared ||
      (owner.counted && _mutableOnCounted(field)) ||
      _handedByTraitOf(owner, field) ||
      _setThroughTraitOf(owner, field);

  /// A mutable field a trait this class implements declares: the trait's
  /// setter (`set_x(&self, ..)`, every non-final field has one) is how a
  /// base's body writes it, and `&self` can only write a cell. It was a
  /// `todo!` (`_DefaultRootPipelineOwner._manifold`, written by
  /// `PipelineOwner.attach`'s super function, run479).
  bool _setThroughTraitOf(IrClass owner, IrFieldDecl field) =>
      _writable(field) &&
      _supertypesOf(owner).any(
        (t) =>
            library.isAbstract(t.name) &&
            t.fields.any((f) => f.name == field.name && _writable(f)) &&
            _fieldsWrittenBy(t).contains(field.name),
      );

  /// A field some body may assign: not `final`, or a `late final` without
  /// an initialiser, which is assigned once somewhere (`late final
  /// ScrollbarPainter scrollbarPainter` set in `RawScrollbarState.
  /// initState`, run666: the trait had no setter for it).
  static bool _writable(IrFieldDecl field) =>
      !field.isFinal || (field.isLate && field.initial == null);

  /// The fields a trait's own bodies assign on `this` (`_manifold =
  /// manifold` in `PipelineOwner.attach`): those writes reach an
  /// implementer through the setter, so only those fields need the cell.
  /// Every trait-declared mutable field was a cell for one round (ws480),
  /// which made `Cell`s of what `_isCopy` misjudged and took the widgets
  /// crate down.
  Set<String> _fieldsWrittenBy(IrClass trait) =>
      _traitWrites.putIfAbsent(trait.name, () {
        final found = <String>{};
        void walk(IrStmt s) {
          switch (s) {
            case IrAssignField(:final name, :final target):
              if (target == null || target is IrThis) found.add(name);
            case IrBlock(:final statements):
              statements.forEach(walk);
            case IrIf(:final then, :final otherwise):
              walk(then);
              if (otherwise != null) walk(otherwise);
            case IrTryCatch(:final body, :final handler):
              walk(body);
              walk(handler);
            case IrTryFinally(:final body, :final finalizer):
              walk(body);
              walk(finalizer);
            case IrWhile(:final body):
              walk(body);
            case IrLabeled(:final body):
              walk(body);
            case IrSwitch(:final cases, :final otherwise):
              for (final one in cases) {
                walk(one.body);
              }
              if (otherwise != null) walk(otherwise);
            case IrForIn(:final body):
              walk(body);
            default:
              break;
          }
        }

        for (final m in trait.methods) {
          walk(m.body);
        }
        // ..and the writes through a handle of the trait's type from any
        // body in the program: `tween.end ??= tween.begin` on a
        // `Tween<dynamic>` in `_constructTweens` reaches `ThemeDataTween`
        // through `set_end`, which was a `todo!` there (run615).
        found.addAll(_writtenThroughHandles(trait));
        return found;
      });

  /// The fields set on a handle typed as `trait` (or a subtype of it)
  /// anywhere in the program, by name. Each class's bodies are walked
  /// once for the whole run (`_setterWritesIn`), and this unions them for
  /// one trait, once per backend.
  final Map<String, Set<String>> _externalWrites = {};

  Set<String> _writtenThroughHandles(IrClass trait) =>
      _externalWrites.putIfAbsent(trait.name, () {
        final found = <String>{};
        final classes = <IrClass>{
          ...library.elsewhere.values,
          ...library.classes,
        };
        for (final c in classes) {
          for (final entry in _setterWritesIn(c).entries) {
            final receiver = entry.key;
            if (receiver == trait.name) {
              found.addAll(entry.value);
              continue;
            }
            final receiverClass = library[receiver];
            if (receiverClass != null &&
                _supertypesOf(receiverClass).any((t) => t.name == trait.name)) {
              found.addAll(entry.value);
            }
          }
        }
        return found;
      });

  /// `_WalkSelf.setterWrites` over one class's bodies, memoised on the
  /// class object: the program's classes are shared by every module's
  /// backend, and walking them once per module was the whole program
  /// times the module count.
  static final _setterWritesOf = Expando<Map<String, Set<String>>>();

  static Map<String, Set<String>> _setterWritesIn(IrClass c) {
    final cached = _setterWritesOf[c];
    if (cached != null) return cached;
    final walk = _WalkSelf();
    for (final m in c.methods) {
      walk.statement(m.body);
    }
    for (final k in c.constructors) {
      final body = k.body;
      if (body != null) walk.statement(body);
    }
    return _setterWritesOf[c] = walk.setterWrites;
  }

  final Map<String, Set<String>> _traitWrites = {};

  /// A field that a trait this class implements hands out as a cell
  /// (`_handsCell`): the implementer holds it as one, so the trait body's
  /// in-place write lands on the object (66 `todo!`s at ws272 -- `_Theater.
  /// children`, every `ChangeNotifier._listeners` on a plain struct).
  bool _handedByTraitOf(IrClass owner, IrFieldDecl field) =>
      _handsCell(field) &&
      _supertypesOf(owner).any(
        (t) =>
            library.isAbstract(t.name) &&
            t.fields.any((f) => f.name == field.name),
      );

  /// Every class above `of`: superclasses, mixins and interfaces, transitively.
  List<IrClass> _supertypesOf(IrClass of) {
    final seen = <String>{};
    final out = <IrClass>[];
    void walk(String? name) {
      if (name == null || !seen.add(name)) return;
      final c = library[name];
      if (c == null) return;
      out.add(c);
      walk(c.superclass);
      for (final m in c.mixins) {
        walk(m.name);
      }
      for (final i in c.interfaces) {
        walk(i.name);
      }
    }

    walk(of.superclass);
    for (final m in of.mixins) {
      walk(m.name);
    }
    for (final i in of.interfaces) {
      walk(i.name);
    }
    return out;
  }

  /// On a counted class, what has to live in a cell: a field that is
  /// assigned, a `late` one (assigned after construction by definition), and
  /// a `final` collection -- `final Set<Image> _handles = {}` is never
  /// reassigned and is added to from `Image`'s constructor, which through a
  /// plain field behind an `Rc` cannot borrow mutably (E0596).
  bool _mutableOnCounted(IrFieldDecl field) =>
      !field.isFinal || field.isLate || _isMutableCollection(type(field.type));

  /// A trait's collection field that a trait body mutates in place is handed
  /// out as its cell: the value accessor clones, and `this_._trackers
  /// .borrow_mut().insert(..)` on a clone inserted into a copy (125 at ws271).
  /// ..and a field a closure in a trait body writes (`shared`): the
  /// closure captures the cell (`_copyOf`), which the trait must hand out
  /// (`_fadeoutTimer = null` inside `RawScrollbarState`'s timer callback,
  /// run666).
  /// A `late` one too when shared -- the copy a trait body's closure
  /// takes asks for the cell (`_copyOf`), and no trait declared it
  /// (`_configuration` inside `ScrollableState.setCanDrag`'s recognizer
  /// factory, run685); its cell holds the `Option` the struct holds
  /// (`_heldType`). A late collection stays a value: the in-place writes
  /// through `_cellPlace` do not look inside an `Option`.
  bool _handsCell(IrFieldDecl field) =>
      field.shared || (!field.isLate && _isMutableCollection(type(field.type)));

  /// The cell a handed-out field lives in: a `Cell` for a `Copy` value,
  /// as the struct holds it (a shared `int` counter a trait body's
  /// closure bumps, the closurefield fixture), a `RefCell` otherwise.
  String _cellType(String held) => _isCopy(held)
      ? 'std::rc::Rc<std::cell::Cell<$held>>'
      : 'std::rc::Rc<std::cell::RefCell<$held>>';

  static bool _isMutableCollection(String rust) =>
      // ..or an absent-or-not one (`Map<K, V>?` in a cell, mutated under
      // `?.`).
      (rust.startsWith('Option<') &&
          rust.endsWith('>') &&
          _isMutableCollection(rust.substring(7, rust.length - 1))) ||
      rust.startsWith('Vec<') ||
      rust.startsWith('Set<') ||
      rust.startsWith('Map<') ||
      rust.startsWith('Queue<') ||
      rust.startsWith('std::collections::VecDeque<') ||
      // The prelude's aliases of a `Vec` (`Float64List`, a counted
      // `Matrix4`'s storage, ws510) and the byte view written in place.
      const {
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
      }.contains(rust);

  /// What a field holds, `Option`-wrapped when it is `late`.
  ///
  /// The wrapper goes *inside* the cell: a `late` field that a closure watches
  /// is `Rc<RefCell<Option<T>>>`, one cell holding one absent value, not two
  /// nested absences.
  String _heldType(IrFieldDecl field) {
    final held = type(field.type);
    return field.isLate ? 'Option<$held>' : held;
  }

  /// `_heldType` of a type already spelled (substituted for an impl).
  String _lateWrapped(IrFieldDecl field, String held) =>
      field.isLate ? 'Option<$held>' : held;

  /// The held type as the *declaration* spells it, for deciding a cell's
  /// kind: inside a wider impl for one instantiation (`_selfBinding`) a
  /// `T` field spells `i64`, but the struct's cell is the `RefCell` a
  /// `T` got (`NumVal<i64>.value.get()` on a `RefCell`, the restoreprop
  /// fixture).
  String _heldDecl(IrFieldDecl field) => _declSpelling(() => _heldType(field));

  String _declSpelling(String Function() spell) {
    final saved = _selfBinding;
    _selfBinding = const {};
    try {
      return spell();
    } finally {
      _selfBinding = saved;
    }
  }

  /// The `late` field of *this* class by that name, or null.
  IrFieldDecl? _lateField(String name) {
    for (final f in _allFields(cls)) {
      if (f.name == name) return f.isLate ? f : null;
    }
    return null;
  }

  /// Another class's field, when a read or write of it goes through a cell.
  ///
  /// The same question `_sharedField` answers for this class, asked of the
  /// class the front end named on the node: shared, or non-final on a counted
  /// class. Null when the owner is not in the crate, or the field is plain.
  IrFieldDecl? _cellFieldOf(String owner, String name) {
    final owned = library[owner];
    if (owned == null) return null;
    for (final f in _allFields(owned)) {
      if (f.name != name) continue;
      // The one predicate the struct's own reads use (`_inCellOf`): a
      // collection a trait hands out as a cell is one here too, and
      // `(widget as _Theater).children[i]` indexed the cell (ws547).
      return _inCellOf(owned, f) ? f : null;
    }
    return null;
  }

  IrFieldDecl? _sharedField(String name) {
    for (final f in _allFields(cls)) {
      if (f.name == name) return _inCell(f) ? f : null;
    }
    return null;
  }

  /// Whether a method's body makes a closure that keeps `this`.
  ///
  /// The whole body, not the three shapes a closure most often sits in: one
  /// written as an *argument* -- `applyTwice(() => scaled(v), x)` -- is the
  /// commonest of all, and missing it left the method taking `&self` while
  /// its closure cloned that, which clones the struct rather than the handle.
  static bool _handsOutSelf(IrMethod method) {
    final walk = _WalkSelf();
    walk.statement(method.body);
    return walk.holdsSelfClosure || walk.passesSelf;
  }

  /// The methods of a counted class that take `self: &Rc<Self>`: those
  /// that hand `this` out, and those that call one of them on `this` --
  /// `self.addPattern(..)` from a `&self` method could not reach a method
  /// wanting the handle (intl, 3). The same contagion as `_mutating`.
  late final Set<String> _handles = _computeHandles();

  Set<String> _computeHandles() {
    final handles = <String>{};
    final calls = <String, Set<String>>{};
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      final key = _rustName(method);
      if (_handsOutSelf(method)) handles.add(key);
      final walk = _WalkSelf();
      walk.statement(method.body);
      calls[key] = walk.selfCalls;
    }
    var changed = true;
    while (changed) {
      changed = false;
      for (final entry in calls.entries) {
        if (handles.contains(entry.key)) continue;
        if (entry.value.any((c) => handles.contains(snake(c)))) {
          handles.add(entry.key);
          changed = true;
        }
      }
    }
    return handles;
  }

  /// The name a counted closure gives its handle to `this`.
  static const _countedSelf = '__me';

  /// Captured locals that hold a cell, and whether it is a `Cell` (`true`)
  /// or a `RefCell`.
  var _cellLocals = <String, bool>{};

  /// A copy of a field, for a closure to keep.
  ///
  /// `clone()` unless the type is `Copy`, where it would only be noise.
  String _copyOf(IrParam field) {
    // Inside a closure that copied the field already: its copy, the
    // local of that name -- reading `self.x` again borrowed `self` into a
    // nested `'static` closure (`_AnimatedCarousel.build`'s builder inside
    // its `LayoutBuilder` builder, run675).
    if (_closureCaptured.contains(field.name)) {
      return '${snake(field.name)}.clone()';
    }
    final read = '$_selfName.${snake(field.name)}';
    // A copied `late` field is unwrapped here rather than in the body: the
    // closure holds a `T`, so the reads inside it are ordinary local reads.
    // It takes the value the field has when the closure is *made*, which is
    // the same trade round 97 made for every copied field.
    final late = _lateField(field.name);
    // Inside a trait body (`this_: &__Self`) a field is an accessor, as
    // `_fieldRead` spells every read there: a shared field's cell through
    // its `_cell()` accessor, so the closure and the object keep one map
    // (`CachingAssetBundle.loadStructuredBinaryData`'s callbacks, run571).
    if (_fieldsAreAccessors) {
      if (_sharedField(field.name) != null) {
        return '${read}_cell()$_propagate';
      }
      final value = '$read()$_propagate';
      return late != null ? '$value.unwrap()' : value;
    }
    if (late != null && _sharedField(field.name) == null) {
      return _isCopy(_declSpelling(() => type(late.type)))
          ? '$read.unwrap()'
          : '$read.clone().unwrap()';
    }
    // A shared field is carried as a *handle*: the closure and the object must
    // see the same cell, which is the whole reason it is shared. Cloning an
    // `Rc` is cloning the handle, not the value.
    if (_sharedField(field.name) != null) return '$read.clone()';
    return _isCopy(type(field.type)) ? read : '$read.clone()';
  }

  /// The closure parameter a `?.` binds.
  ///
  /// One fixed name, not a fresh one per nesting level: a chained `a?.b?.c`
  /// nests the closures, and the inner one shadows the outer -- which is what
  /// the Dart means, since the inner access is about the inner value.
  static const _boundName = 'it';

  /// An erased twin's result back to what the call declared (see the
  /// prelude's `CastErased`). A projected `T?` -- this declaration's own
  /// parameter, nullable -- has no `CastErased` of its own: the value
  /// comes back as the `Option<T>` and goes out through `from_option`,
  /// as any projected value does (`find<T>()` returning `T?`, ws496).
  /// A class's method by name, this class's or one of its ancestors'.
  IrMethod? _methodOf(String? className, String name) {
    var c = className == null ? null : library[className];
    while (c != null) {
      final own = [
        ...c.methods,
        ...c.abstractMethods,
      ].where((m) => m.name == name && !m.isStatic).firstOrNull;
      if (own != null) return own;
      c = c.superclass == null ? null : library[c.superclass!];
    }
    return null;
  }

  String _erasedCast(IrType resultType, String call, {IrMethod? method}) {
    // A twin whose return does not mention the method's own parameters
    // (`getElementForInheritedWidgetOfExactType<T>` returns an
    // `InheritedElement?`) hands back the declared type already: no cast
    // (`CastErased<Option<Rc<dyn InheritedElement>>>` asked of itself,
    // ws503).
    if (method != null &&
        type(_substituteType(method.returnType, _erasure(method))) ==
            type(method.returnType)) {
      return call;
    }
    if (resultType.projected && resultType.nullable) {
      final inner = type(
        IrType(resultType.name, arguments: resultType.arguments),
      );
      return '<$inner as DartNullable>::from_option('
          'dart_cast_erased::<Option<$inner>, _>($call))';
    }
    return 'dart_cast_erased::<${type(resultType)}, _>($call)';
  }

  /// `a ?? b`, in the one of four spellings Rust needs.
  ///
  /// Two questions decide it, and both come from the front end because the IR
  /// carries no expression types:
  ///
  /// * **Is the result still nullable?** `a ?? b` is non-null exactly when `b`
  ///   is. `unwrap_or_else` produces a value, `or_else` produces an Option, and
  ///   using the wrong one does not type-check -- which is how nested `??`
  ///   found this, since `a ?? b ?? c` has a nullable `a ?? b` inside it.
  /// * **May the right side be evaluated eagerly?** Dart's `??` is
  ///   short-circuit and Rust's `unwrap_or`/`or` are not. Only a literal is
  ///   safe; 77% of upstream's right-hand sides are calls, constructors or
  ///   throws.
  String _ifNull(IrIfNull node) {
    // A `match` on a place moves out of it, and the place lives on
    // (`final child = inactive ?? create(); if (inactive != null) ..` in
    // `Element.inflateWidget`, ws494): a local or a field of `this` is
    // read by clone.
    final operand = node.left;
    // The scrutinee bound first: a `match` keeps its scrutinee's
    // temporaries -- the `Ref` of a `.borrow().clone()` -- alive through
    // every arm, and the `None` arm of `_instance ??= X()` on a static
    // cell wrote through `borrow_mut()` into it ("already borrowed",
    // run516). A `let` drops them at its own end.
    final read = operand is IrLocal
        ? '${expr(operand)}.clone()'
        : expr(operand);
    final left = '{ let __scrutinee = $read; __scrutinee }';
    if (node.right is IrThrowValue) {
      // `a ?? throw e`. The closure forms are wrong here for the reason a try
      // body could not hold a `?`: the `return Err(e)` inside `unwrap_or_else`
      // would return from the *closure*. A match has no closure to escape
      // from, and the arm that throws simply diverges.
      return 'match $left { '
          'Some(__value) => __value, '
          'None => ${expr(node.right)} }';
    }
    final right = expr(node.right);
    // The lazy side as a `match`, not an `or_else(|| ..)`: a closure is its
    // own function, and an `.await` inside one -- `a ?? await b()` -- is
    // "await outside async". `match` keeps the laziness and stays in the
    // enclosing function. 13 `E0728`s.
    if (node.nullableResult) {
      return node.eager
          ? '$left.or($right)'
          : 'match $left { Some(__value) => Some(__value), None => $right }';
    }
    if (!node.eager) {
      return 'match $left { Some(__value) => __value, None => $right }';
    }
    return node.eager
        ? '$left.unwrap_or($right)'
        : '$left.unwrap_or_else(|| $right)';
  }

  /// Dart's binary operators in Rust's spelling.
  ///
  /// Most are the same token and pass straight through. The ones that are not
  /// are the reason this is a function and not string interpolation:
  ///
  /// * `~/` is truncating division and has no Rust operator at all. On floats
  ///   it is `(a / b).trunc()`; the `.toDouble()` Dart then needs is dropped
  ///   in `_call`, because the result is already an `f32`.
  /// * `??` takes the left unless it is null.
  ///
  /// An operator not listed and not passed through would be silently wrong, so
  /// anything unrecognised stops.
  String _binary(String op, IrExpr left, IrExpr right, [IrType? type]) {
    if (op == '+' && type?.name == 'String') {
      // `String + String` is not Rust. `format!` is, it needs no borrow worked
      // out at either end, and it is what Dart's `+` on two strings means.
      return 'format!("{}{}", ${expr(left)}, ${expr(right)})';
    }
    const passthrough = {
      '+',
      '-',
      '*',
      '/',
      '%',
      '==',
      '!=',
      '<',
      '>',
      '<=',
      '>=',
      '&&',
      '||',
      '&',
      '|',
      '^',
      '<<',
      '>>',
    };
    if (op == '~/') return '((${expr(left)} / ${expr(right)}).trunc())';
    if (op == '??') {
      // Dart's `??` is short-circuit: the right side is evaluated only when the
      // left is null. Rust's `unwrap_or` evaluates it **always**, so it is right
      // only for a value that has no effects and costs nothing -- and this used
      // `unwrap_or` for everything from round two until the corpus was counted.
      //
      // Of 6764 `??` in package:flutter only 23% have a literal or constant on
      // the right. The rest are calls, constructors, and in six places a
      // `throw` -- where eager evaluation does not give a wrong answer, it
      // throws unconditionally.
      //
      // A literal keeps the shorter form because it reads better and is
      // provably safe; everything else defers.
      if (right is IrLiteral) {
        return '${expr(left)}.unwrap_or(${expr(right)})';
      }
      return '${expr(left)}.unwrap_or_else(|| ${expr(right)})';
    }
    // Dart's `>>>` is the logical shift on the 64-bit pattern; Rust's `>>`
    // on `i64` is arithmetic, and on `u64` it is this (`_TrieNode.
    // _trieIndex`, `_bitCount`: the `PersistentHashMap` every
    // `InheritedElement` mounts through, run537).
    // Dart's shifts on an `int`, by count: the prelude's, which give 0 (or
    // the sign) past 63 where Rust's operators panic (`_trieIndex` at bit
    // index 65, ws557). Only on an `int` left operand: the byte and mask
    // arithmetic on other widths keeps the operator.
    final leftInt =
        left.rustType?.name == 'int' ||
        (left is IrLiteral && left.type.name == 'int');
    if (op == '>>>' || ((op == '<<' || op == '>>') && leftInt)) {
      final helper = switch (op) {
        '<<' => 'dart_shl',
        '>>' => 'dart_shr',
        _ => 'dart_ushr',
      };
      return '$helper((${expr(left)}) as i64, (${expr(right)}) as i64)';
    }
    if (!passthrough.contains(op)) {
      throw Unsupported('binary operator `$op`', '${expr(left)} $op ...');
    }
    // An operator on an open class's handle: `Rc<dyn Size> * f64` has no
    // `impl std::ops::Mul` to land on (the orphan rule: `Rc` is not
    // fundamental), so it is the trait's method, which fails like any
    // method (`Size.lerp`, ws473).
    final leftName = left.rustType?.name;
    final mapping = operatorTraits[op];
    if (mapping != null &&
        leftName != null &&
        library[leftName] != null &&
        library.isAbstract(leftName)) {
      return '${expr(left)}.op_${mapping.$2}(${expr(right)})$_propagate';
    }
    // ..and on a counted class's handle: the `impl std::ops::Mul` is
    // `for Struct`, so the *left* operand is the value the handle holds,
    // cloned (`Rc<Matrix4> * Rc<Matrix4>`, ws511). The right is not: the
    // impl's `Rhs` is the operator's parameter as it was declared, and a
    // counted class named in a parameter is its handle -- `Mul<Rc<Matrix3>>
    // for Matrix3`, `Mul<Rc<dyn Object>>` where the parameter is `dynamic`.
    // Dereferencing it too handed `Vector3` to a `Rc<Vector3>` slot (17 at
    // ws747, across Vector3, _Vector, OffsetPair, AttributedString).
    if (mapping != null) {
      final name = left.rustType?.name;
      final counted = name != null && (library[name]?.counted ?? false);
      if (counted && !left.rustType!.nullable) {
        return '((*${expr(left)}).clone() $op ${expr(right)})';
      }
    }
    // `==` on a type parameter's values (`T`, `T?`) is Dart's `==`, the
    // prelude's `DartEq`, which every parameter carries; `PartialEq` is
    // not asked of one (`selected == value` on a `T?` in
    // `CupertinoSegmentedControl`, 7 at ws460).
    // ..and on any object that is not a primitive: Dart's `==` is the
    // class's `operator ==`, which is `DartEq` here, taken by reference
    // (`==` on two `Rc<dyn Size>` moved its operand, E0382, 53 at ws464).
    if ((op == '==' || op == '!=') &&
        (_ownsParameter(left.rustType) ||
            _ownsParameter(right.rustType) ||
            (_objectLike(left.rustType) && _objectLike(right.rustType)))) {
      // `DartEq` compares two of the *left's* type: the right operand
      // was shared into `Object` for Dart's `operator ==(Object)`, and is
      // shared into the left's trait instead (`&Rc<dyn Object>` where
      // `&Rc<dyn Color>` was wanted, 12 at ws467).
      final leftType = left.rustType;
      final bare = right is IrUpcast && right.type.name == 'Object'
          ? right.value
          : right;
      // ..and where this module's world cannot classify the value (a
      // struct of another library), the `Object` sharing stands: the
      // prelude's `dart_object` takes the trait the comparison wants.
      // Two handles of one trait at different instantiations (`Route<T>`
      // against the navigator's `Route<dynamic>`, ws505): Dart's `==` on
      // them is identity, and only a thin pointer can compare the two.
      final rightType = right.rustType;
      if (leftType != null &&
          rightType != null &&
          leftType.name == rightType.name &&
          !leftType.nullable &&
          !rightType.nullable &&
          library.isAbstract(leftType.name) &&
          leftType.arguments.isNotEmpty &&
          leftType.arguments.toString() != rightType.arguments.toString()) {
        final same = 'dart_identical_any(&${expr(left)}, &${expr(right)})';
        return op == '==' ? same : '(!$same)';
      }
      // Two handles of different traits: the one *below* goes up into the
      // other's type -- never the other way, which is a cast that fails
      // (`next?.route != entry.lastAnnouncedNextRoute`, a `Route?` against
      // a `_RoutePlaceholder?` above it, run636); unrelated ones compare
      // as the objects they are.
      final bareType = bare.rustType;
      if (leftType != null && bareType != null) {
        final ln = stripNull(leftType).name;
        final rn = stripNull(bareType).name;
        if (ln != rn && library.isAbstract(ln) && library.isAbstract(rn)) {
          if (_world.isBelow(ln, rn) && !_world.isBelow(rn, ln)) {
            final lifted = coerceInto(left, bareType, _world, inClosure: true);
            final eq = '${expr(lifted)}.dart_eq(&${expr(bare)})';
            return op == '==' ? eq : '(!$eq)';
          }
          if (!_world.isBelow(rn, ln)) {
            final eq =
                'dart_option_object(${_asOption(left)}).dart_eq(&dart_option_object(${_asOption(bare)}))';
            return op == '==' ? eq : '(!$eq)';
          }
        }
      }
      final coerced = leftType != null && bare.rustType != null
          ? coerceInto(bare, leftType, _world, inClosure: true)
          : bare;
      final other = identical(coerced, bare) ? right : coerced;
      final eq = '${expr(left)}.dart_eq(&${expr(other)})';
      return op == '==' ? eq : '(!$eq)';
    }
    return '(${expr(left)} $op ${expr(right)})';
  }

  /// A value as an `Option` for `dart_option_object`: itself when its
  /// recorded type is nullable, `Some(..)` otherwise.
  String _asOption(IrExpr e) {
    final t = e.rustType;
    return t != null && isNullable(t) && !t.projected
        ? expr(e)
        : 'Some(${expr(e)})';
  }

  /// The type a `<T as DartNullable>` projection names: the parameter
  /// itself when it is one in scope, else the concrete type it was
  /// substituted with, spelled (`<Rc<dyn Object> as DartNullable>`, not
  /// `<Object as ..>`: E0782 26 and `dynamic` 25 at ws465).
  String _nullableOf(String parameter) {
    // ..bound to one instantiation inside a wider impl (`_selfBinding`).
    final bound = _selfBinding[parameter];
    if (bound != null) return type(bound);
    if (cls.typeParameters.contains(parameter) ||
        _methodTypeParams.contains(parameter)) {
      return parameter;
    }
    return type(IrType(parameter));
  }

  /// `::<i64>` for the class's own parameters inside a wider impl for one
  /// instantiation (`_selfBinding`), so that `ConstantTween::lerp(self,
  /// t)` names `ConstantTween::<f64>` and rustc does not infer the class's
  /// `T` from the trait's return type instead (ws627); empty otherwise.
  String _selfTurbofish() {
    if (_selfBinding.isEmpty) return '';
    final args = [
      for (final p in cls.typeParameters)
        if (_selfBinding[p] != null) type(_selfBinding[p]!),
    ];
    if (args.length != cls.typeParameters.length) return '';
    return '::<${args.join(', ')}>';
  }

  /// A type in the class's own terms, as the wider impl being written
  /// binds them (`_selfBinding`), for the coercion rule to compare with
  /// the trait's side; the type itself outside one.
  IrType _selfBound(IrType t) =>
      _selfBinding.isEmpty ? t : _substituteType(t, _selfBinding);

  /// Whether a type is a translated class's, a trait's, or a collection's
  /// -- anything `==` compares by `DartEq` rather than by value.
  bool _objectLike(IrType? t) {
    if (t == null || t.isFunction) return false;
    final name = t.name;
    // A `dynamic` (an `Object?`) compares by `DartEq` too: the raw `==`
    // on two `Rc<dyn Object>` moved its right operand (E0382, a pattern
    // switch on `data['platformBrightness']`, run502), and the prelude's
    // `DartEq for dyn Object` is the value comparison either way.
    if (const {
      'int',
      'double',
      'num',
      'bool',
      'String',
      'void',
      '()',
      'Null',
      'Type',
      'Option',
    }.contains(name)) {
      return false;
    }
    if (name == 'dynamic' || name == 'Object') return true;
    final c = library[name];
    if (c != null && c.isEnum) return false;
    return c != null || const {'List', 'Map', 'Set', 'Iterable'}.contains(name);
  }

  /// Whether a type is a type parameter of the class or method being
  /// printed, or its nullable form.
  bool _ownsParameter(IrType? t) =>
      t != null &&
      t.arguments.isEmpty &&
      !t.isFunction &&
      (cls.typeParameters.contains(t.name) ||
          _methodTypeParams.contains(t.name));

  /// A Dart string's contents, safe to sit inside a Rust `"..."`.
  ///
  /// The backslash has to be doubled *before* the quote is escaped, or the
  /// backslash this step just added would be doubled by the next one. Only
  /// these two characters need it: Rust and Dart agree on the rest.
  /// A Dart string as a Rust literal.
  ///
  /// The backslash and the quote were escaped from the start. The control
  /// characters were not, and a carriage return written raw into a Rust
  /// literal is a hard error -- `bare CR not allowed in string` -- 108 times
  /// across upstream, which mostly writes them inside `\r\n`.
  String _escape(String text) => text
      .replaceAll('\\', '\\\\')
      .replaceAll('"', '\\"')
      .replaceAll('\r', '\\r')
      .replaceAll('\n', '\\n')
      .replaceAll('\t', '\\t')
      .replaceAll('\u0000', '\\0')
      // Text-direction controls (the l10n files have them) are rejected raw
      // by rustc's `text_direction_codepoint_in_literal`; written as escapes
      // they are the same string. 23 literals.
      .replaceAllMapped(
        RegExp('[\u200E\u200F\u202A-\u202E\u2066-\u2069]'),
        (m) => '\\u{${m[0]!.codeUnitAt(0).toRadixString(16)}}',
      );

  String _literal(String value, IrType t) {
    if (t.name == 'double') {
      // Rust needs the point: `1` is an integer literal even in an f32 context.
      return value.contains('.') || value.contains('e') ? value : '$value.0';
    }
    // Escaped for the same reason the assert message is: a Dart string holding
    // a quote or a backslash would otherwise end the Rust literal early or
    // start an escape that was never in the source.
    if (t.name == 'String') return '"${_escape(value)}".to_string()';
    if (t.name == 'Null') return 'None';
    return value;
  }

  /// The free function that holds a base class's own body for `name`.
  ///
  /// Rust has no `super`. Once an impl overrides a trait's default method the
  /// default is unreachable -- `Trait::name(self)` dispatches back to the
  /// override and the program hangs. So every concrete method on an abstract
  /// class is emitted twice: once as a free generic function holding the body,
  /// and once as the trait default, which calls it. `super.name(..)` then names
  /// the function, which is the one thing that cannot dispatch anywhere else.
  /// A getter and a setter share a Dart name and must not share a Rust one.
  ///
  /// `RenderBox` has `Size get size` and `set size(Size)`, and both produced
  /// `render_box_super_size` -- the same collision round 62 found in the trait
  /// impls, one level over in the free functions that hold the bodies.
  static String superFn(
    String base,
    String name, {
    bool isSetter = false,
  }) => _rustIdentifier(
    '${snakeRaw(base)}_super_${isSetter ? 'set_' : ''}'
    '${RegExp(r'^[A-Za-z_][A-Za-z0-9_]*$').hasMatch(name) ? snakeRaw(name) : _operatorName(name)}',
  );

  /// A static call, checked against the IR when it lands in this library.
  ///
  /// `Alignment._stringify(x, y)` was emitted for a method the front end had
  /// refused, so the output named a function nobody wrote. That is round one's
  /// bug in a new shape: it was masked then by refusing every private reference,
  /// and removing that blunt rule brought it back. The precise rule is the same
  /// one `_superCall` uses -- if the callee is in this file, it has to be in the
  /// IR.
  /// Whether a top-level name is one of the library's mutable variables.
  bool _isMutableTopLevel(String name) =>
      library.constants.any((c) => c.name == name && c.isMutable) ||
      // Another module's: `numberFormatSymbols` read from `NumberFormat`
      // was a bare `NUMBER_FORMAT_SYMBOLS.get(..)` against its `LazyLock`.
      (library.constantsElsewhere[name]?.isMutable ?? false);

  /// `List.generate(n, f)` and friends, which are Dart's list constructors
  /// wearing a static's clothes. Rust builds a `Vec` from an iterator.
  static const _listStatics = {
    'generate',
    'filled',
    'from',
    'of',
    'unmodifiable',
  };

  /// A value spelled projected (`<T as DartNullable>::Or`, a read out of
  /// a `Vec<T?>` or a generic accessor) where an `Option` operation --
  /// `!`, `== null`, `?.`, `??`, `==` -- wants the `Option<T>` a body works
  /// with: the prelude's conversion first.
  /// A type the prelude's `FromDynamic` converts: what a `dynamic` holds
  /// as itself, the scalars, and collections of those.
  bool _dynamicRepresentable(IrType t) {
    if (t.isFunction) return false;
    if (const {
      'Object',
      'dynamic',
      'String',
      'int',
      'double',
      'bool',
    }.contains(t.name)) {
      return true;
    }
    return (t.name == 'List' || t.name == 'Map') &&
        t.arguments.isNotEmpty &&
        t.arguments.every(_dynamicRepresentable);
  }

  /// `.flatten()` after a map lookup whose value type is itself nullable.
  String _flattenedValue(IrExpr? map) {
    final t = map?.rustType;
    if (t == null || t.arguments.length != 2) return '';
    return t.arguments[1].nullable ? '.flatten()' : '';
  }

  /// A closure's return type, spelled where inference has nothing to go
  /// on: a body ending in `Ok(None)` -- a `Null`-returning closure handed
  /// to the prelude's `then` -- left `Option<_>` open (`_initKeyboard`,
  /// run477). Elsewhere `_`, as before.
  String _closureReturnSpelled(IrType returns) {
    if (returns.isFunction || returns.name == 'raw') return '_';
    if (returns.name == 'Null' || returns.nullable) return type(returns);
    // A trait, spelled: then the `Ok(..)` around the body is a coercion
    // site, and a concrete handle unsizes into the trait object there.
    // Left `_`, `List<Widget>.generate(n, (i) => _VisibilityScope(..))`
    // collected a `Vec<Rc<_VisibilityScope>>` where `Vec<Rc<dyn Widget>>`
    // went -- the cast has nowhere to be written (12 at ws747).
    if (library.isAbstract(returns.name) && !_mentionsUnknown(returns)) {
      return type(returns);
    }
    return '_';
  }

  /// The captured cell locals that hold a `late` field (see `_cellLocals`).
  Set<String> _lateCellLocals = const {};

  /// `Some(v)`, with a closure inside it unsized to the function type it
  /// is typed as. A struct literal's field spells the slot
  /// (`Option<Rc<dyn Fn(..)>>`) and Rust still does not unsize a closure
  /// through the `Some` on the way in: the `Rc<{closure}>` stayed one
  /// where a `FormField<T>`'s erased validator went (ws856). Only where
  /// the type is spelled -- a closure whose own type this is -- since
  /// spelling it everywhere named type parameters out of scope (ws551).
  String _some(IrExpr value) {
    final held = value is IrClosure && !value.boxed
        ? 'std::rc::Rc::new(${expr(value)})'
        : expr(value);
    final t = value.rustType;
    if (t != null && t.isFunction && _closureLike(value)) {
      return 'Some({ let __f: ${type(t)} = $held; __f })';
    }
    return 'Some($held)';
  }

  /// A mapped element's body: the closure around it is one the prelude
  /// calls, and its return is a plain value. A failing call inside
  /// unwraps, as a chain step's does (`_stepClosure`); left propagating,
  /// the `?` had no `Result` to come out of -- "the `?` operator can only
  /// be used in a closure that returns `Result`" (3 in `Navigator` at
  /// ws871).
  String _mappedBody(IrExpr body) {
    final saved = _failure;
    _failure = null;
    final text = expr(body);
    _failure = saved;
    return text;
  }

  /// A closure, possibly behind the wrappers coerce puts on one (a
  /// `Some`, an upcast, a clone).
  bool _closureLike(IrExpr e) => switch (e) {
    IrClosure() => true,
    IrUpcast(:final value) => _closureLike(value),
    IrSome(:final value) => _closureLike(value),
    // The adapter `coerce` makes of a bound function value (`{ let __f =
    // ..; Rc::new(move |..| ..) }`): its map's return is spelled, or the
    // `Rc<{closure}>` never unsized (a conditional tear-off into
    // `VoidCallback?`, ws551).
    IrBlockValue(:final value) => _closureLike(value),
    IrCall(:final target, :final name, :final args) =>
      (name == 'clone' || name == '!rc') &&
          args.isEmpty &&
          target != null &&
          _closureLike(target),
    _ => false,
  };

  /// A type that cannot be spelled as a return (`_`, a placeholder, a
  /// method's own parameter nothing declares here).
  bool _mentionsUnknown(IrType t) {
    if (t.name == '_' || t.name == 'raw' || t.name.isEmpty) return true;
    if (t.isFunction) {
      return t.parameters!.any(_mentionsUnknown) ||
          (t.returns != null && _mentionsUnknown(t.returns!));
    }
    return t.arguments.any(_mentionsUnknown);
  }

  /// Whether the null-aware body being printed binds its value by value
  /// (a scalar receiver) rather than by reference.
  bool _boundByValue = false;

  /// Whether a null-aware body hands the binding itself back: a `?..`
  /// cascade's block, whose last expression is the bound name.
  static bool _endsAtBound(IrExpr body) => switch (body) {
    IrBound() => true,
    IrBlockValue(:final value) => _endsAtBound(value),
    _ => false,
  };

  /// That body with the binding cloned where it is produced.
  String _clonedBound(IrExpr body) => switch (body) {
    IrBound() => '$_boundName.clone()',
    IrBlockValue(:final statements, :final value) => () {
      final saved = _out.length;
      final savedIndent = _indent;
      _indent = 0;
      for (final s in statements) {
        stmt(s);
      }
      final written = _out.sublist(saved).join(' ');
      _out.removeRange(saved, _out.length);
      _indent = savedIndent;
      return '{ $written ${_clonedBound(value)} }';
    }(),
    _ => expr(body),
  };

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

  String _superCall(
    String base,
    String name,
    List<IrExpr> args, {
    bool isSetter = false,
    List<IrType> baseArguments = const [],
    List<IrType> typeArguments = const [],
  }) {
    // `Object` is not a class this compiler has, and it never will be -- it is
    // the root every Dart class already inherits from. So `super.toString()`
    // was refused as "not in this file", 198 times, when the truth is that
    // there is no file. Dart's own `Object.toString` returns
    // `Instance of 'Foo'`, so that is what it translates to; upstream prints
    // exactly this for a class that overrides nothing.
    //
    // Only `toString`. `super.hashCode` and `super.==` are identity on the
    // object, and identity is what a copied value class does not have --
    // routing them through `_identical` at ws828 refused them again, at the
    // argument (`IrCall (Object)`, not a reference). They stay refused, and
    // the census that says why is in STATUS: 239 classes have their
    // identity observed and only two reach `super` into `Object`.
    if (base == 'Object' && name == 'toString' && args.isEmpty) {
      return 'format!("Instance of \'{}\'", "${cls.name}")';
    }
    final baseClass = library[base];
    if (baseClass == null) {
      throw Unsupported(
        'super call into `$base`, which is not in this file',
        'super.$name(...)',
      );
    }
    // An operator counts: `_emitSuperFns` writes a free function for every
    // non-static member, an operator's under the name `superFn` gives it
    // (`inline_span_super_op_eq`), and whether that function came out is
    // `_superFnEmits`'s question below -- which is the one this used to
    // pre-empt by refusing every `super.==` outright (5 refusals: `TextSpan`,
    // `WidgetSpan`, `_BodyBoxConstraints`, `_FileSpan`, `ColorSwatch`, ws877).
    final provides = baseClass.methods.any(
      (m) => m.name == name && !m.isStatic && m.isSetter == isSetter,
    );
    if (!provides) {
      // The base's own version was refused, or is abstract and has no body to
      // call. Emitting the call anyway would name a function that was never
      // written -- the `_stringify` shape from round one, one level up.
      throw Unsupported(
        'super call to `$base.$name`, which was not translated',
        'super.$name(...)',
      );
    }
    if (!_superFnEmits(baseClass, name, isSetter: isSetter)) {
      // The base *has* the method, and the free function holding its body still
      // could not be emitted -- so the name this call would use is not written
      // anywhere. `Alignment.toString` called `alignment_geometry_super_to_-
      // string` for exactly this reason, and the Kernel side of the library did
      // not build for two rounds while `agree.py` was recorded as green.
      //
      // The question is answered by emitting the function and seeing, rather
      // than by a second rule about when it works: a second rule is a thing
      // that can disagree with the first one.
      throw Unsupported(
        'super call to `$base.$name`, whose body did not translate',
        'super.$name(...)',
      );
    }
    // The receiver as the super function takes it, `&__Self`: `self` is
    // already a reference -- one more deref when it is the handle
    // (`_receiverOf`) -- and a closure's `__me` is a handle when the class
    // is counted, a value otherwise (510 `Rc<X>: Trait` bounds at ws294).
    final receiver = _selfName == 'self'
        ? (_selfIsHandle ? '&**self' : 'self')
        : _selfName == 'this_'
        ? 'this_'
        : cls.counted
        ? '&*$_selfName'
        : '&$_selfName';
    // The base's type arguments spelled: a class implementing the trait
    // at two instantiations (`Animation<f64>` and the wider `Animation<
    // Option<f64>>`) left `T` ambiguous (E0283, 3 at ws451). The method's
    // own stay inferred.
    final method = baseClass.methods.firstWhere(
      (m) => m.name == name && !m.isStatic && m.isSetter == isSetter,
    );
    final own = typeArguments.length == method.typeParameters.length
        ? typeArguments.map(type).toList()
        : List.filled(method.typeParameters.length, '_');
    final turbofish = baseArguments.isEmpty && own.every((a) => a == '_')
        ? ''
        : '::<_${[...baseArguments.map(type), ...own].map((a) => ', $a').join()}>';
    final call =
        '${superFn(base, name, isSetter: isSetter)}$turbofish(${[receiver, ...args.map(expr)].join(', ')})';
    // KNOWN GAP (found by the analyzer 2026-09-09, never measured): an async
    // super function is an `async fn`, and the caller's trait wants the boxed
    // future every `Future<T>` is here -- so this call should be awaited and
    // boxed. The flag that says so was computed here and never read, which is
    // why nothing has been awaiting it. Closing it changes what is emitted at
    // every async super call, so it is a round of its own.
    return call;
  }

  /// Whether `base`'s free function for [name] can actually be emitted.
  ///
  /// `_superFailed` answers this for the class being emitted, but a super call
  /// is made from the *subclass*, whose backend never sees the base's set.
  static final _superFnProbes = <String, bool>{};

  bool _superFnEmits(IrClass baseClass, String name, {bool isSetter = false}) {
    // Only an abstract class writes them. `_emitSuperFns` is called from
    // `_emitTrait` and nowhere else, because the free function is generic over
    // the trait -- there is nothing to make it generic over when the base is a
    // struct, since flattening copies the base's fields into each subclass
    // rather than leaving them anywhere shared. Probing without asking this
    // first said yes and the call named a function nobody wrote; the mixin
    // fixture is what walked into it.
    if (!baseClass.isAbstract) return false;
    final key = '${baseClass.name}.${isSetter ? 'set:' : ''}$name';
    final known = _superFnProbes[key];
    if (known != null) return known;
    final method = baseClass.methods.firstWhere(
      (m) => m.name == name && !m.isStatic && m.isSetter == isSetter,
    );
    final probe = RustBackend(baseClass, library: library);
    final ok = probe._member(key, () => probe._emitSuperFn(method));
    return _superFnProbes[key] = ok;
  }

  /// Whether a field of *this* class is reachable as a field right now.
  ///
  /// Inside a trait it is not. The class's fields were flattened into every
  /// implementor, so the trait -- and the free functions holding its method
  /// bodies -- can only reach them through an accessor the trait requires.
  /// Reading them as fields gives "no field `width` on type `&S`".
  var _fieldsAreAccessors = false;

  /// Whether the signature being written belongs to a trait.
  var _inTrait = false;

  /// `this` as an owned handle, from wherever the body is: a trait body's
  /// `dart_self_<trait>()`, a counted class's stored handle, else a clone.
  String _selfHandle() => _fieldsAreAccessors
      ? '$_selfName.dart_self_${snakeRaw(cls.name)}()'
      : cls.counted
      ? '$_selfName.dart_self_ref().get()'
      : '$_selfName.clone()';

  /// In a trait body, the trait an accessor is reached through when more
  /// than one trait in the chain declares it (`textTheme` on
  /// `CupertinoThemeData` over `NoDefaultCupertinoThemeData`; 13 E0034 at
  /// ws308): this trait when it declares the name, which is the override
  /// Dart would dispatch to, else the nearest abstract supertype that does.
  String? _accessorQualifier(String name, {String kind = 'read', IrClass? on}) {
    // ..on this class by default, or on the class a handle is typed by
    // (a trait object's field read, `_fieldRead`).
    final cls = on ?? this.cls;
    // What each kind of accessor a trait declares (`_emitTrait`): a read
    // for any field or getter, a write for a mutable field or a setter, a
    // cell for a held collection. Naming a trait that lacks the item was
    // "expected a type, found a trait" (22 at ws309).
    bool field(IrClass c) =>
        c.fields.any(
          (f) =>
              f.name == name &&
              switch (kind) {
                'write' => !f.isFinal,
                'cell' => _handsCell(f),
                _ => true,
              },
        ) ||
        // A cell an application holds for a hollow mixin is the mixin's
        // to hand out (`appliedFields`).
        (kind == 'cell' &&
            c.appliedFields.any((f) => f.name == name && _handsCell(f)));
    bool method(IrClass c) =>
        c.methods.any(
          (m) =>
              m.name == name && !m.isStatic && m.isSetter == (kind == 'write'),
        ) ||
        c.abstractMethods.any(
          (m) => m.name == name && m.isSetter == (kind == 'write'),
        );
    if (kind == 'cell' && !field(cls) && !_supertypesOf(cls).any(field)) {
      return null;
    }
    final chain = [
      cls,
      ..._supertypesOf(cls).where((t) => library.isAbstract(t.name)),
    ];
    final declaring = chain.where((c) => field(c) || method(c)).toList();
    if (declaring.length < 2) return null;
    // A getter override is the nearest trait's own; a field's accessor is
    // declared once, by the topmost trait holding the field (`_emitTrait`
    // leaves it to the ancestor), which no other declarer is above.
    if (method(cls)) return cls.name;
    final fields = declaring.where(field).toList();
    if (fields.isEmpty) return declaring.first.name;
    return fields
        .firstWhere(
          (c) => !fields.any((o) => o != c && _supertypesOf(c).contains(o)),
          orElse: () => fields.last,
        )
        .name;
  }

  /// A field an application holds for `of` or one of its abstract
  /// ancestors (`IrClass.appliedFields`), by name.
  IrFieldDecl? _appliedFieldOf(IrClass of, String name) {
    for (final c in [of, ..._abstractAncestors(of)]) {
      for (final f in c.appliedFields) {
        if (f.name == name) return f;
      }
    }
    return null;
  }

  /// The trait, `from` or one above it, that declares the Rust item
  /// `rustName` (a method, a setter, a field's accessor); null when none
  /// of them does.
  String? _declaringTrait(String from, String rustName) {
    // Another module's trait too (`ModalRoute` from `widgets`, whose
    // `addLocalHistoryEntry` is `LocalHistoryRoute`'s: `ModalRoute::add_
    // local_history_entry(..)` was E0782 in `material`, run672).
    final start = library[from] ?? library.elsewhere[from];
    if (start == null) return null;
    bool declares(IrClass c) =>
        c.methods.any((m) => !m.isStatic && _methodName(m) == rustName) ||
        c.abstractMethods.any((m) => _methodName(m) == rustName) ||
        c.fields.any(
          (f) =>
              snake(f.name) == rustName || 'set_${snake(f.name)}' == rustName,
        );
    for (final c in [start, ..._abstractAncestors(start)]) {
      if (declares(c)) return c.name;
    }
    return null;
  }

  /// `this` as the handle the object already has -- the trait's own in a
  /// trait body, the counted struct's otherwise -- or null when the class
  /// has none (a plain value struct).
  String? _thisHandle() {
    if (_fieldsAreAccessors || _selfName == 'this_') {
      return '$_selfName.dart_self_${snakeRaw(cls.name)}()';
    }
    if (cls.counted) return '$_selfName.dart_self_ref().get()';
    return null;
  }

  /// A value shared as a handle: `this` by its own handle (`self.clone()`
  /// was a struct where `Rc<dyn RendererBinding>` went, `_manifold`'s
  /// lazy initializer, run459), anything else as spelled.
  String _handleOf(IrExpr value) {
    // A clone of `this` (the front end's) is `this`.
    final bare =
        value is IrCall &&
            value.name == 'clone' &&
            value.args.isEmpty &&
            (value.target == null || value.target is IrThis)
        ? IrThis()
        : value;
    return bare is IrThis ? (_thisHandle() ?? expr(value)) : expr(value);
  }

  String _fieldRead(
    IrExpr? target,
    String name, [
    bool onEnum = false,
    String? owner,
  ]) {
    final receiver = _receiver(target);
    // A field of an *enum* is a getter here, not storage: the value is a
    // constant of the variant and lives in a `match`. Only the front end knows
    // -- the backend sees `state.value` with no idea what `state` is -- so it
    // says so on the node.
    if (onEnum) return '$receiver.${snake(name)}()';

    // Inside a trait every read on `this` is an accessor call: a trait has
    // no fields, and a mixin's `this_.source_url` names a getter of the
    // implementer's, declared in an interface the mixin never sees (7).
    if (_fieldsAreAccessors && (target == null || target is IrThis)) {
      // `super.x` of a base's *field* (the front end names the base on
      // the node): the base trait's accessor, which is the storage --
      // this trait's own may be a getter over it (`CupertinoThemeData.
      // primaryColor` over `NoDefaultCupertinoThemeData`'s field, run618).
      if (owner != null && owner != cls.name && library.isAbstract(owner)) {
        return '${_implementedAs(owner)}::${snake(name)}($receiver)$_propagate';
      }
      final through =
          _accessorQualifier(name) ?? _wideTraitFor(cls, name)?.name;
      return through == null
          ? '$receiver.${snake(name)}()$_propagate'
          : '${_implementedAs(through)}::${snake(name)}($receiver)$_propagate';
    }
    // A shared field is read through its cell. `get` copies, which is what a
    // Dart read does; `borrow().clone()` is the same for a value that is not
    // `Copy`.
    if (target == null || target is IrThis) {
      final shared = _sharedField(name);
      if (shared != null) {
        final lazy = _lazyDecl(name);
        if (lazy != null) return _lazyRead(lazy, receiver);
        // The guard bound and dropped in its own statement (as another
        // object's field is read below): a bare `.borrow().clone()` keeps
        // its `Ref` to the statement's end, into a `borrow_mut()` of the
        // same cell on the left (`_file = _file.setPosition(0)`, run517).
        // Parenthesised: a block at a statement's start is a statement.
        final read = _isCopy(_heldDecl(shared))
            ? '$receiver.${snake(name)}.get()'
            : '({ let __r = $receiver.${snake(name)}.borrow().clone(); __r })';
        // Out of the cell it is a value, so the `late` unwrap is on a value
        // too. This is the one shape that does need `T: Clone`.
        return shared.isLate ? '$read.unwrap()' : read;
      }
      final late = _lateField(name);
      if (late != null) {
        // `as_ref()` rather than a clone: a read of a field is a place in
        // Rust, and `&T` is what the sites around it already expect. Only a
        // `Copy` value is taken out whole, which is what a place does anyway.
        // Cloned out, as every other field read is now: `as_ref()` handed
        // back a `&_ImageFilter` where the getter returns one by value (4).
        return _isCopy(_declSpelling(() => type(late.type)))
            ? '$receiver.${snake(name)}.unwrap()'
            : '$receiver.${snake(name)}.clone().unwrap()';
      }
    }
    // Another object's field, when the front end named its class and that
    // class keeps the field in a cell: read through the cell, as the write
    // side does. Without this the read was `entry.x` against a `RefCell`.
    if (owner != null) {
      final cell = _cellFieldOf(owner, name);
      // Its own class's accessor runs the initialiser on the right
      // object (`_emitLazyAccessors`); `_lazyRead` here printed it
      // against *this* class's `self`.
      if (cell != null && _lazyFieldOf(owner, name)) {
        return '$receiver.${_lazyAccessor(name)}()$_propagate';
      }
      if (cell != null) {
        // The `borrow()` guard is a temporary, and a temporary in a block's
        // tail expression outlives the block's locals: `Ok(data.next_sibling
        // .borrow().clone())` on a local `data` was "does not live long
        // enough" 17 times (ws376). Bound and handed out, the guard dies
        // in its own statement.
        final read = _fieldIsCopy(cell, owner == null ? null : library[owner])
            ? '$receiver.${snake(name)}.get()'
            : '{ let __r = $receiver.${snake(name)}.borrow().clone(); __r }';
        return cell.isLate ? '$read.unwrap()' : read;
      }
      // Another object's `late` field: `other._argb` in `Hct.==` is an
      // `Option<i64>` on that side too, and reads unwrap it as `this`'s do.
      final owned = library[owner];
      if (owned != null) {
        for (final f in _allFields(owned)) {
          if (f.name != name || !f.isLate) continue;
          return _isCopy(_declSpelling(() => type(f.type)))
              ? '$receiver.${snake(name)}.unwrap()'
              : '$receiver.${snake(name)}.clone().unwrap()';
        }
      }
    }
    // A read of one of this class's own fields is a *value*, and behind
    // `&self` a value that is not `Copy` has to be cloned out: `self._value`
    // moved out of a shared reference, 134 times in the leaf crates. A
    // method call on the clone or a borrow of it costs a clone and nothing
    // else.
    if (target == null || target is IrThis) {
      for (final f in _allFields(cls)) {
        if (f.name == name) {
          return _isCopy(_declSpelling(() => type(f.type)))
              ? '$receiver.${snake(name)}'
              : '$receiver.${snake(name)}.clone()';
        }
      }
    }
    // A field of a local: cloned out, as a field of `self` is -- `r._m3storage`
    // moved out of `r` and `r.clone()` two lines later was a partial move (9).
    // As a *receiver* the field is a place; `_receiver` spells that.
    // Another object of *this* class (`other as Hct`): its `late` field is
    // the same `Option`, unwrapped the same way.
    if (target is IrDowncast && target.type == cls.name) {
      final late = _lateField(name);
      if (late != null) {
        return _isCopy(_declSpelling(() => type(late.type)))
            ? '$receiver.${snake(name)}.unwrap()'
            : '$receiver.${snake(name)}.clone().unwrap()';
      }
    }
    // A field of a *trait object*: the accessor the trait declares, since
    // a `dyn` has no fields (`childParentData.nextSibling` on an `Rc<dyn
    // StackParentData>`, the mixin's field, ws523).
    final held = target?.rustType;
    if (held != null && !isNullable(held) && library.isAbstract(held.name)) {
      final owned = library[held.name];
      if (owned != null && _allFields(owned).any((f) => f.name == name)) {
        // Qualified when two of the handle's traits declare it
        // (`next_sibling` on `ContainerBoxParentData` and on the mixin,
        // E0034 at ws526), through the trait object the handle holds.
        final through = _accessorQualifier(name, on: owned);
        final declaring = through == null ? null : library[through];
        if (declaring == null) return '$receiver.${snake(name)}()$_propagate';
        final passed = _argumentsThrough(owned, const {}, declaring, {});
        final traitArgs = passed == null || passed.isEmpty
            ? ''
            : '<${passed.map(type).join(', ')}>';
        final heldArgs = held.arguments.isEmpty
            ? ''
            : '<${held.arguments.map(type).join(', ')}>';
        return '<dyn ${held.name}$heldArgs as $through$traitArgs>::${snake(name)}(&*$receiver)$_propagate';
      }
    }
    // Any other object's field: cloned out, as a field of `self` or of a
    // local is (`..get().child` handed to `updateChild` moved out of the
    // handle, E0507, run459).
    return '$receiver.${snake(name)}.clone()';
  }

  /// `x is Foo`.
  ///
  /// Rust answers it with `Any`, which downcasts to a *concrete* type: the
  /// trait object says what it holds, and holding is always a struct. So a
  /// target that is itself abstract has no answer here -- `x is RenderBox`
  /// asks whether the thing implements a trait, which `Any` cannot say -- and
  /// is still refused, now under a name that says which half is missing.
  /// See `IrSuperDispatch`. The super function's generics are `<__Self,
  /// class parameters.., method parameters..>`: the first two kinds are
  /// inferred from the receiver, the method's own are spelled.
  String _superDispatch(
    IrExpr receiver,
    String base,
    String name,
    List<IrExpr> args,
    List<IrType> typeArguments,
    int classArity,
    String? castTo,
  ) {
    // A generic trait cast to with its arguments inferred (`dyn
    // CanonicalizedMap<_, _, _>`; E0107 on the bare name, run459).
    final castArity = library[castTo ?? '']?.typeParameters.length ?? 0;
    final castSpelled = castArity == 0
        ? castTo
        : '$castTo<${List.filled(castArity, '_').join(', ')}>';
    final on = castTo == null
        ? expr(receiver)
        : '${expr(receiver)}.dart_cast_to::<dyn $castSpelled>().unwrap()';
    final generics = [
      '_',
      for (var i = 0; i < classArity; i++) '_',
      ...typeArguments.map(type),
    ];
    // An async base method's super function is its future, not a
    // `Result` (`invokeMethod` reaching `_invokeMethod<T>`, ws482).
    final baseMethod = library[base]?.methods
        .where((m) => m.name == name && !m.isStatic)
        .firstOrNull;
    final suffix = (baseMethod?.isAsync ?? false) ? '' : _propagate;
    return '${superFn(base, name)}::<${generics.join(', ')}>'
        '(${['&*$on', ...args.map(expr)].join(', ')})$suffix';
  }

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
      final shared = _sharedField(place.name);
      if (shared != null && !_isCopy(_heldDecl(shared))) {
        return '&mut *$_selfName.${snake(place.name)}.borrow_mut()';
      }
      if (shared == null) return '&mut $_selfName.${snake(place.name)}';
    }
    // A mutable static lent to a callee that fills it (`fill(log)` on a
    // top-level list, the statmut fixture): through its cell.
    if (place is IrTopLevel && _isMutableTopLevel(place.name)) {
      return '&mut *(**${screamingSnake(place.name)}).borrow_mut()';
    }
    if (place is IrStatic &&
        !place.isEnumValue &&
        _isMutableStatic(place.owner, place.name)) {
      return '&mut *(**${_lazyName(place.owner, place.name)}).borrow_mut()';
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
  String? _mutPlace(IrExpr? target) {
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
    if (target is IrNullCheck) {
      final inner = _mutPlace(target.operand);
      if (inner != null) return '$inner.as_mut().unwrap()';
      // ..a plain local `Option<..>` promoted: the local itself.
      var operand = target.operand;
      if (operand is IrCall &&
          operand.name == 'clone' &&
          operand.args.isEmpty &&
          operand.target != null) {
        operand = operand.target!;
      }
      if (operand is IrLocal && !_cellLocals.containsKey(operand.name)) {
        return '${snake(operand.name)}.as_mut().unwrap()';
      }
      return null;
    }
    final cell = _cellPlace(target);
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
        return '$cell.borrow_mut().as_mut().unwrap()';
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
            : '$place.get_mut(&${_borrowed(inner.args.single)}).unwrap()';
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
        return '$cell.borrow().as_ref().unwrap()';
      }
    }
    return '$cell.borrow()';
  }

  /// The cell a field read would go through, as a place -- `self.x` or
  /// `other.x` -- when the field is kept in a `RefCell`; null otherwise.
  String? _cellPlace(IrExpr? target) {
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
        return '(**${screamingSnake(target.name)})';
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
        return '(**${_lazyName(target.owner, target.name)})';
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
    final held = _heldType(cell);
    if (!_isMutableCollection(held)) return null;
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
  };

  /// The prelude methods whose callback parameter is `impl Fn`: it is
  /// called and dropped, never kept. The rest (`remove_where`, `update`,
  /// `put_if_absent`) declare an `Rc<dyn Fn>` and take the handle.
  static const _preludeLends = {
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
  static const _preludeFailingStatics = {'generate', '_invoke1_with_return'};

  /// `?` when a function surrounds the expression, `.unwrap()` otherwise.
  String get _propagate => _failure != null ? '?' : '.unwrap()';

  /// Set while the operand of an `await` is printed: the call's own `?`
  /// belongs after the `.await`.
  bool _awaiting = false;

  String _call(
    IrExpr? target,
    String name,
    List<IrExpr> args, {
    String? qualifier,
    String? receiverClass,
    bool fails = false,
    List<IrType> typeArguments = const [],
    bool asyncFn = false,
    bool asyncTarget = false,
    IrType? resultType,
  }) {
    final turbofish = typeArguments.isEmpty
        ? ''
        : '::<${typeArguments.map(type).join(', ')}>';
    // Cleared before the receiver and arguments print: a call inside them
    // would otherwise take this `await`'s flag.
    _awaiting = false;
    // Before the receiver is rendered: rendering a chain on its own is
    // refused, and this is the one place a chain is not on its own.
    // Any arguments, not none: Dart's `toList({bool growable = true})` has a
    // named parameter, and the Kernel front end fills in its default -- so the
    // chain was collected on one side and refused on the other.
    if (name == 'to_list' && target is IrIterChain) {
      // `where(..).toList()`: `filter` keeps references, and the list
      // wants the items (`Vec<&Rc<dyn FocusNode>>`, 8 at ws467).
      final cloned = target.steps.isNotEmpty && target.steps.last.$1 == 'filter'
          ? '.cloned()'
          : '';
      return _chain(target, tail: '$cloned.collect::<Vec<_>>()');
    }
    // `0.29.powf(x)`: a float literal as a receiver is an "ambiguous numeric
    // type" until it says which (21 `E0689`s in the HCT colour code).
    // `self._handles.add(x)` on a field kept in a cell: the cell is the
    // place, and a mutating call goes through `borrow_mut()`. Read out as a
    // value first -- `.borrow().clone().push(x)` -- it compiled and pushed
    // onto a copy: 27 such silent no-ops in the leaf crates.
    // `recorder as _NativePictureRecorder` where the class is counted: the
    // downcast through `Any` yields the struct inside the `Rc<dyn Trait>`,
    // and every holder of that class wants an `Rc<_NativePictureRecorder>`.
    // A new handle around a clone: the fields are cells, so the state is
    // still shared; only the handle's identity is new.
    if (name == 'clone' &&
        args.isEmpty &&
        target is IrDowncast &&
        (library[target.type]?.counted ?? false)) {
      // ..the object's own handle, now that it keeps one (`DartSelf`).
      return '${expr(target)}.dart_self_ref().get()';
    }
    // `runtimeType` on a super function's `this_` (see `DartAny`).
    // ..and on a struct method's `self` too: through `&mut self`, `Object::
    // runtime_type` resolved on the *reference*, which the blanket impl
    // asks to be `'static` (E0521, `WriteBuffer.done`, run505).
    if ((name == 'runtimeType' || name == 'runtime_type') &&
        args.isEmpty &&
        (target == null || target is IrThis) &&
        (_selfName == 'this_' || _selfName == 'self')) {
      return '$_selfName.dart_runtime_type()';
    }
    // ..and on a value of a translated class (a struct, an enum, a trait
    // handle -- `Rc` derefs): the class's own name, where the blanket
    // `Object::runtime_type` on the handle said `Rc` (the tostr fixture,
    // ws543).
    if ((name == 'runtimeType' || name == 'runtime_type') &&
        args.isEmpty &&
        target != null &&
        target is! IrThis &&
        library[target.rustType?.name ?? ''] != null &&
        !(target.rustType?.nullable ?? false)) {
      return '${expr(target)}.dart_runtime_type()';
    }
    // `hashCode` on a value typed by a type parameter is the Object
    // protocol's (`DartEq::dart_hash_code`, which every type argument
    // implements as it implements `==`): `key.hash_code()` on a `K`
    // named no trait (`PersistentHashMap.put`, run537).
    if ((name == 'hashCode' || name == 'hash_code') &&
        args.isEmpty &&
        target != null &&
        target is! IrThis &&
        _isTypeParam(target.rustType?.name ?? '')) {
      return 'DartEq::dart_hash_code(&${expr(target)})';
    }
    // ..and on `this` in a class that declares none (`OrdinalSortKey(1.0,
    // name: hashCode.toString())` in `_InputDecoratorState`, ws654): the
    // protocol's, which every struct implements.
    if ((name == 'hashCode' || name == 'hash_code') &&
        args.isEmpty &&
        (target == null || target is IrThis) &&
        !_declaresHashCode(cls)) {
      return 'DartEq::dart_hash_code(&*$_selfName)';
    }
    // ..and on a *handle* to a class that declares one, through the value:
    // the prelude gives every `Rc<T>` a `hash_code` of its own (`RcHashCode`,
    // a shared object's identity) and that is the one the call reached --
    // an `i64` where the class's own hands back a `Result`, so the `?` had
    // nothing to come out of (`TextSelection` in `TextEditingValue
    // .hashCode`, `BorderSide` in `StadiumBorder`'s, 2 at ws872).
    if ((name == 'hashCode' || name == 'hash_code') &&
        args.isEmpty &&
        target != null &&
        target is! IrThis) {
      final held = target.rustType;
      final owned = held == null || held.isFunction || isNullable(held)
          ? null
          : library[held.name];
      if (held != null &&
          owned != null &&
          _declaresHashCode(owned) &&
          (owned.counted || library.isAbstract(held.name))) {
        return '(*${expr(target)}).hash_code()$_propagate';
      }
    }
    // ..and on a value of no translated class: a scalar, a prelude type,
    // a handle to a trait (`Object.hash(canUndo, canRedo, ..)` on two
    // `bool` fields, `UndoHistoryValue.hashCode`, run677).
    if ((name == 'hashCode' || name == 'hash_code') &&
        args.isEmpty &&
        target != null &&
        target is! IrThis) {
      final held = target.rustType;
      final owned = held == null || held.isFunction || isNullable(held)
          ? null
          : library[held.name];
      if (held != null &&
          !held.isFunction &&
          !isNullable(held) &&
          (owned == null || !_declaresHashCode(owned))) {
        return 'DartEq::dart_hash_code(&${expr(target)})';
      }
    }
    // A read that does not need the collection itself: through the cell's
    // `borrow()`, not through the clone a read of the place hands out.
    // `_listeners.length` in `ImageStreamCompleter.removeListener`'s loop
    // cloned the whole listener list once per iteration, and the run spent
    // its whole budget there (run799). Inside a block, so the `Ref` guard
    // drops with the `let` that made it and cannot outlive a `borrow_mut()`
    // later in the same statement -- which is why a read of the place is
    // written `({ let __r = ..borrow().clone(); __r })` to begin with.
    final borrowedRead = _borrowedRead(target, name, args);
    if (borrowedRead != null) return borrowedRead;
    final cellPlace = _mutatesInPlace(name) ? _mutPlace(target) : null;
    // The arguments first, bound: the receiver's `borrow_mut()` is taken
    // before the arguments are evaluated, and an argument reading the same
    // cell panicked ("already mutably borrowed": `counts['x'] = (counts['x']
    // ?? 0) + 1` on a static, the statmut fixture). A closure literal and a
    // plain literal read nothing when made and stay in place.
    if (cellPlace != null &&
        args.any((a) => !_closureLike(a) && a is! IrLiteral)) {
      final binds = <String>[];
      final rebound = <IrExpr>[];
      for (var i = 0; i < args.length; i++) {
        final a = args[i];
        if (_closureLike(a) || a is IrLiteral) {
          rebound.add(a);
          continue;
        }
        // ..with its implicit upcast spelled: a `let` has no slot to
        // unsize against (`_tickers!.remove(ticker)` bound a
        // `Rc<_WidgetTicker>` where the set holds `Rc<dyn Ticker>`, ws577).
        // A place bound is shared, not moved: `slot` read again two lines
        // on had been moved into the temporary
        // (`SlottedContainerRenderObjectMixin._setChild`, ws738).
        final held = a.rustType;
        final shared =
            (a is IrLocal || a is IrField) &&
            (held == null || !_isCopy(type(held)));
        binds.add(
          'let __a$i = ${expr(_explicitUpcast(a))}${shared ? '.clone()' : ''};',
        );
        rebound.add(
          IrLiteral('__a$i', const IrType('raw'))..rustType = a.rustType,
        );
      }
      final inner = _call(
        target,
        name,
        rebound,
        qualifier: qualifier,
        receiverClass: receiverClass,
        fails: fails,
        typeArguments: typeArguments,
        asyncFn: asyncFn,
        asyncTarget: asyncTarget,
        resultType: resultType,
      );
      return '{ ${binds.join(' ')} $inner }';
    }
    // A mutating call on a field of `this` in a struct's own method acts
    // on the field, not on the clone a value read takes: `_buffer.setRange
    // (..)` on a clone left `WriteBuffer` empty and every platform message
    // without a byte (run507).
    // ..a field of this struct's own, held as a plain collection: a cell
    // (`Rc<RefCell<..>>`) has its place above, and a handle's method that
    // shares a mutator's name (`AnimationController.reverse`) is not a
    // mutation of the field (+44 at ws509).
    final ownPlace =
        cellPlace == null &&
            _mutatesInPlace(name) &&
            target is IrField &&
            (target.target == null || target.target is IrThis) &&
            !_fieldsAreAccessors &&
            // ..and in a constructor body, where `this` is the value being
            // built (`__new`): `_items.addAll(items)` there extended a
            // clone and the field stayed empty -- `TweenSequence` then had
            // no intervals and threw on the first frame (run763).
            (_selfName == 'self' || _selfName == '__new') &&
            _ownCollectionField(target.name)
        ? '$_selfName.${snake(target.name)}'
        : null;
    // A method of this class that takes `self: &Rc<Self>` (`_receiverOf`),
    // called on `this` from an operator: `std::ops` fixes the receiver by
    // value, so the contagion that makes every other caller take the handle
    // has nowhere to put it. `this` is the object's own handle instead
    // (`Vector4::op_mul` calling `clone`, 12 at ws747).
    final selfHandle =
        (target == null || target is IrThis) &&
            _selfByValue &&
            cls.counted &&
            (_handles.contains(snake(name)) ||
                _handles.contains(_identifier(name)))
        ? _thisHandle()
        : null;
    final receiver = selfHandle != null
        ? selfHandle
        : cellPlace != null
        ? cellPlace
        : ownPlace != null
        ? ownPlace
        : target is IrLiteral && target.type.name == 'double'
        ? (() {
            // ..once: a literal the front end already suffixed (an integer
            // written as a double, ws779) would read `0.0_f64_f64`.
            final text = _receiver(target);
            return text.endsWith('_f64') ? '($text)' : '(${text}_f64)';
          }())
        : _receiver(target);
    // `HashMap` looks up by reference, and gives back a reference to the
    // value. Dart's `m[k]` is a `V?`, so the borrow is cloned away rather
    // than leaked into every caller's type.
    // A value shared into a trait object (see `_widened`).
    // `this` shared: the object's own handle, not a fresh `Rc` around a
    // reference (`Rc::new(this_)` wanted `'static`, 168 lifetime errors)
    // or a copy (a new identity).
    if (name == '!rc' && args.isEmpty && target is IrThis) {
      final own = _thisHandle();
      if (own != null) return own;
    }
    if (name == '!rc' && args.isEmpty) {
      // ..a value that already *is* a handle is one: `dart_object` around
      // an `Rc<dyn Widget>` made an `Rc<Rc<dyn Widget>>`, whose pointee
      // implements nothing (`picker = dart_object(inputDatePicker())`,
      // ws751).
      if (target != null && _handleLike(target)) return expr(target);
      // A closure behind its handle, unsized to the function type it is
      // typed as where that is spelled: inside a `.map(|__f| ..)` there
      // is no slot to infer `Rc<dyn Fn>` from, and the `Rc<{closure}>`
      // stayed one (a conditional tear-off into `VoidCallback?`, ws549;
      // a closure into a `FormFieldValidator<String>?`, ws856).
      if (resultType != null && resultType.isFunction) {
        return '{ let __f: ${type(resultType)} = std::rc::Rc::new($receiver); __f }';
      }
      return 'std::rc::Rc::new($receiver)';
    }
    // An `Option<Rc<dyn Object>>` into a `dynamic` slot: absent is `Null`.
    if (name == '!or_null' && args.isEmpty) {
      return '$receiver.unwrap_or_else(|| std::rc::Rc::new(Null) as std::rc::Rc<dyn Object>)';
    }
    // The other way: a `dynamic` as an `Option`, `None` for the `Null` object.
    if (name == '!nullable' && args.isEmpty) return 'dart_nullable($receiver)';
    if (name == '!widen_object' && args.isEmpty) {
      // `iter().cloned()`: the receiver may be the `&Vec` a null-aware
      // `as_ref().map(|it| ..)` binds, and `into_iter` on that yields
      // references (E0282 in `ColorFilter.hashCode`).
      return '$receiver.iter().cloned().map(|v| Some(std::rc::Rc::new(v) as std::rc::Rc<dyn Object>)).collect::<Vec<_>>()';
    }
    if (name == '!widen' && args.isEmpty) {
      return '$receiver.into_iter().map(|v| v as i64).collect::<Vec<i64>>()';
    }
    if (name == '!narrow' && args.length == 1) {
      final to = expr(args.single);
      return '$receiver.into_iter().map(|v| v as $to).collect::<Vec<$to>>()';
    }
    // Into `Rc<dyn Object>` by name: inside a `.map(|it| ..)` the unsizing
    // has nothing to infer it from.
    // A `dynamic` asked whether it is a `T`: the `Option<T>` `Any` gives.
    // ..by the parameter's own conversion, not `Any`'s one concrete
    // type: a `T` instantiated with `Rc<dyn Object>` (`invokeMethod<
    // dynamic>`) is no object's type, and `decodeEnvelope(..) as T?` gave
    // null for every reply -- `MissingPlatformDirectoryException` at
    // run513. Dart's null (the `Null` object) is `None` first.
    if (name == '!as_opt' && args.length == 1) {
      final spelledArgs = typeArguments.isEmpty
          ? ''
          : '<${typeArguments.map(type).join(', ')}>';
      // A counted class is held by its handle: `locale as Locale?` on a
      // `dynamic` is the `Rc<Locale>` the object is, not a copy of the
      // struct (`_getLocaleOptions`, run664).
      final spelledName = expr(args.single);
      final held = (library[spelledName]?.counted ?? false)
          ? 'std::rc::Rc<$spelledName$spelledArgs>'
          : '$spelledName$spelledArgs';
      final asked = '<$held as FromDynamic>::from_dynamic';
      // ..on an `Option` already (a `dynamic?`): through it.
      final targetIr = target?.rustType;
      if (targetIr != null && isNullable(targetIr)) {
        return '$receiver.and_then(|__v| dart_nullable(__v)).as_ref().and_then(|__v| $asked(__v))';
      }
      return 'dart_nullable($receiver).as_ref().and_then(|__v| $asked(__v))';
    }
    if (name == '!as_object' && args.isEmpty) {
      // `this` into an `Object` slot: the handle when the method holds
      // one, a fresh `Rc` of a clone when it does not.
      if (target is IrThis) {
        // ..and the object's own handle where it has one (`_selfHandle`):
        // `Rc::new(this_.clone())` boxed a reference (the last 20 lifetime
        // errors at ws335).
        if (_fieldsAreAccessors || _selfName == 'this_') {
          return '($_selfName.dart_self_${snakeRaw(cls.name)}() as std::rc::Rc<dyn Object>)';
        }
        if (cls.counted) {
          return '($_selfName.dart_self_ref().get() as std::rc::Rc<dyn Object>)';
        }
        return _selfIsHandle
            ? '($_selfName.clone() as std::rc::Rc<dyn Object>)'
            : '(std::rc::Rc::new($_selfName.clone()) as std::rc::Rc<dyn Object>)';
      }
      return '($receiver as std::rc::Rc<dyn Object>)';
    }
    if (name == '!rc_object' && args.isEmpty) {
      // `this` shared as an `Object` is behind `&self`: a handle is cloned,
      // a value is cloned into a fresh one -- `Rc::new(self)` was a handle
      // to a borrow, and "lifetime may not live long enough" 329 times.
      // `this_` in a super function is `&__Self: ?Sized` and stays.
      // ..and now that every object keeps its own handle (`DartSelf`): a
      // trait body's `this` is `dart_self_<trait>()`, a counted class's is
      // `dart_self_ref().get()` -- not a copy with a new identity (8703
      // `Rc::new(self.clone())`, 82 `Rc::new(this_)` at ws292).
      if (target == null || target is IrThis) {
        if (_fieldsAreAccessors) {
          return '($_selfName.dart_self_${snakeRaw(cls.name)}() as std::rc::Rc<dyn Object>)';
        }
        if (cls.counted) {
          return '($_selfName.dart_self_ref().get() as std::rc::Rc<dyn Object>)';
        }
        if (_selfName == 'self') {
          return _selfIsHandle
              ? '(self.clone() as std::rc::Rc<dyn Object>)'
              : '(std::rc::Rc::new(self.clone()) as std::rc::Rc<dyn Object>)';
        }
      }
      return '(std::rc::Rc::new($receiver) as std::rc::Rc<dyn Object>)';
    }
    if (name == '!dart_eq' && args.length == 1) {
      return '$receiver.dart_eq(&${_borrowed(args.single)})';
    }
    // `Vec::contains` takes a reference; Dart's takes the value. Only the
    // List's: `Path.contains(Offset)` is a method of its own.
    if (name == '!contains' && args.length == 1) {
      return '$receiver.dart_contains(&${_borrowed(args.single)})';
    }
    if (name == '!expando_get' && args.length == 1) {
      return '$receiver.get(&${_borrowed(args.single)})';
    }
    // `expando[object] = v`: keyed by identity, so the object's handle
    // (`this` by its own; `PlatformInterface`'s token registry, run482).
    if (name == '!expando_set' && args.length == 2) {
      return '$receiver.set(${_handleOf(args[0])}, ${expr(args[1])})';
    }
    // `m[k]` is a `V?`, and Dart's `V?` of a nullable `V` is `V` itself:
    // `data['platformBrightness']` on a `Map<String, Object?>` is an
    // `Object?`, not an `Option<Option<..>>` (`_updateUserSettingsData`,
    // ws472).
    if (name == '!map_get' && args.length == 1) {
      return '$receiver.get(&${_borrowed(args.single)}).cloned()${_flattenedValue(target)}';
    }
    // `_views[_implicitViewId]` with an `int?` key: Dart looks up `null`
    // and finds nothing; here the absent key is the absent value.
    // The map is built outside the closure: an element that can fail (a
    // `?` in a literal's constructor) has no `Result` to leave through
    // inside an `and_then` returning `Option` (30 E0277 at ws441).
    if (name == '!map_get_opt' && args.length == 1) {
      return '{ let __m = $receiver; ${expr(args.single)}.as_ref().and_then(|__k| __m.get(__k).cloned()${_flattenedValue(target)}) }';
    }
    if (name == '!map_remove' && args.length == 1) {
      return '$receiver.remove(&${_borrowed(args.single)})';
    }
    if (name == 'contains_key' && args.length == 1) {
      return '$receiver.$name(&${_borrowed(args.single)})';
    }
    // The List and Map members Rust says differently rather than renames.
    if (name == '!is_empty' && args.isEmpty) return '!$receiver.is_empty()';
    // `iter()` yields references and the closure is written for values, so
    // the parameter types come off exactly as `_chain` takes them off.
    // `cloned()`, because the Dart closure is written for a value and
    // `iter()` yields a reference: `|x| x > limit` against a `&i64` is
    // `expected &i64, found i64`. The chain steps get away with `iter()`
    // because what they produce is collected, not compared.
    if (name == '!any' && args.length == 1) {
      return '$receiver.iter().cloned().any(${_stepClosure(args.single, cloned: true)})';
    }
    if (name == '!every' && args.length == 1) {
      return '$receiver.iter().cloned().all(${_stepClosure(args.single, cloned: true)})';
    }
    if (name == '!to_set' && args.isEmpty) {
      return 'Set::from($receiver.clone())';
    }
    // Dart joins with the empty string when nothing is given -- and the
    // Kernel front end fills that default in while the analyzer one leaves it
    // off, so the omitted argument has to be recognised rather than trusted to
    // be absent. The fixtures said so: the two sides wrote `join("")` and
    // `join(&"".to_string())` for one line of Dart.
    // `removeLast()`: `pop()` answers an `Option`, Dart's throws on an
    // empty list -- the unwrap is that (`ModalRoute.didPop`'s
    // `_localHistory.removeLast()`, ws551).
    if (name == 'pop' && args.isEmpty && target != null) {
      return '$receiver.pop().unwrap()';
    }
    // `Object.toString()`: the `DartAny` protocol's (a struct's own
    // override, an enum's `X.value`, `Instance of` otherwise).
    if (name == '!dart_to_string' && args.isEmpty && target != null) {
      return '${expr(target)}.dart_to_string()';
    }
    // ..and of a value whose type only the object knows (`dart_object_str`
    // as a method: a reference derefs to it).
    if (name == '!object_str' && args.isEmpty && target != null) {
      return '${expr(target)}.dart_object_str()';
    }
    if (name == '!join' && args.length < 2) {
      final given = args.where((a) => !_isDefault(a, '')).toList();
      final separator = given.isEmpty ? '""' : '&${expr(given.single)}';
      // Each element as Dart's `toString()` gives it, by the rule the
      // front end applies to an interpolation's parts: a string is
      // itself, a number or a bool prints as it is, and only the rest
      // goes through `dart_str` (the `Debug` rendering). `dart_str` on
      // every element put quotes around each string: `['a', 'b'].join(',')`
      // came out as `"a","b"` (the midover fixture, ws535).
      final element = target?.rustType?.arguments.firstOrNull;
      final shown = _elementText(element);
      return '$receiver.iter().map(|__e| $shown)'
          '.collect::<Vec<_>>().join($separator)';
    }
    if (name == '!insert' && args.length == 2) {
      return '$receiver.insert(${expr(args[0])} as usize, ${expr(args[1])})';
    }
    if (name == '!remove_at' && args.length == 1) {
      return '$receiver.remove(${expr(args.single)} as usize)';
    }
    if (name == '!element_at' && args.length == 1) {
      // A clone, as `IrIndex` is: the element is behind the list's
      // reference, and `elementAt` on an `Iterable<T?>` moved out of it
      // once the element's type was the slot's rather than `Option<T>`
      // (`cannot move out of index of Vec<<T as DartNullable>::Or>`,
      // ws691).
      final wrapped = receiver.startsWith('{') ? '($receiver)' : receiver;
      return '$wrapped[${expr(args.single)} as usize].clone()';
    }
    if (name == '!sublist' && args.isNotEmpty && args.length < 3) {
      // `sublist(from)` arrives with an explicit `null` end from Kernel and
      // with nothing from the analyzer. Both mean "to the end".
      final given = args.where((a) => !_isDefault(a, null)).toList();
      // The end is `int?` upstream, so it arrives as `Some(e)`: the value.
      final endValue = given.length == 1 ? null : given[1];
      final end = endValue == null
          ? ''
          : '${expr(endValue is IrSome ? endValue.value : endValue)} as usize';
      return '$receiver[${expr(given[0])} as usize..$end].to_vec()';
    }
    // Dart's `reversed` is a lazy Iterable and nearly every use ends in
    // `toList`. A `Vec` is what that produces, and `to_list` on one clones.
    // `whereType<T>()`: each element asked for a `T` through the cast
    // table -- a trait object, a struct's own handle -- or `Any` for a
    // scalar; the ones that answer, collected.
    // `whereType<T>()` over an `Iterable<T?>`: the elements that are there
    // (see the front end). A clone, because `iter()` hands out references.
    if (name == '!where_present' && args.isEmpty) {
      return '$receiver.iter().filter_map(|v| v.clone()).collect::<Vec<_>>()';
    }
    if (name == '!where_type' && args.isEmpty && typeArguments.length == 1) {
      final wanted = typeArguments.single;
      final spelledArgs = wanted.arguments.isEmpty
          ? ''
          : '<${wanted.arguments.map(type).join(', ')}>';
      final String test;
      if (scalarNames.contains(wanted.name)) {
        test =
            'v.as_ref().as_any().downcast_ref::<${rustScalar(wanted.name)}>().cloned()';
      } else if (library.isAbstract(wanted.name)) {
        test = 'v.dart_cast_to::<dyn ${wanted.name}$spelledArgs>()';
      } else {
        test = 'v.dart_cast_to::<${wanted.name}$spelledArgs>()';
      }
      return '$receiver.iter().filter_map(|v| $test).collect::<Vec<_>>()';
    }
    if (name == '!reversed' && args.isEmpty) {
      return '{ let mut __r = $receiver.clone(); __r.reverse(); __r }';
    }
    if (name == '!cast' && args.isEmpty) return receiver;
    // `first` is an index on a list and a method on a translated class
    // with a getter of that name (`PriorityQueue.first`, E0608 at ws460).
    // ..and a method on the prelude's queues and the intrusive
    // `LinkedList` (`DartQueueRead`), which cannot be indexed (ws549).
    // ..and a `Set`, whose ends are its own methods: a set is no `Vec` and
    // `visibleColors[0]` did not index (`BorderDirectional.paint`, run743).
    const queueLike = {
      'Queue',
      'ListQueue',
      'DoubleLinkedQueue',
      'LinkedList',
      'Set',
      'LinkedHashSet',
      'HashSet',
    };
    if ((name == 'first' || name == 'last') &&
        args.isEmpty &&
        queueLike.contains(target?.rustType?.name ?? '')) {
      return '$receiver.$name()';
    }
    if (name == 'first' &&
        args.isEmpty &&
        library[receiverClass ?? ''] == null) {
      return '$receiver[0].clone()';
    }
    // Cloned out, as `first` is: an element used by value moved out of
    // the `Vec` (`_requestTabTraversalFocus(sortedNodes.last)`, ws522).
    if (name == 'last' && args.isEmpty) {
      return '$receiver[$receiver.len() - 1].clone()';
    }
    // A function local behind its handle, lent to an `impl Fn` slot: the
    // closure inside (`&*f`; a handle is not a function to Rust).
    if (name == '!fn_ref' && args.isEmpty && target != null) {
      return '&*${expr(target)}';
    }
    // Dart's `toList` on a list copies it, which is `clone`.
    // `toList()` on a list is the list again; on any other collection --
    // a `Set`, whose static owner is `Iterable` (ws496) -- the prelude's
    // `to_list()`. Its `growable` is dropped either way.
    if (name == 'to_list') {
      // ..by what the receiver's type spells: an `Iterable` -- a map's
      // `values`, a `reversed` -- is a `Vec` here too (ws497).
      final held = target?.rustType;
      final spelled = held == null ? null : type(held);
      return spelled == null || spelled.startsWith('Vec<')
          ? '$receiver.clone()'
          : '$receiver.to_list()';
    }
    // `Vec::len` gives a `usize` and Dart's `length` an `int`. Without the
    // cast every comparison against a loop counter fails to compile.
    if (name == 'len' && args.isEmpty) return '($receiver.len() as i64)';
    // Dart's `toDouble`. This used to return the receiver unchanged, on the
    // reasoning that a value already stored as a double needs nothing -- true,
    // and it is not the only receiver `toDouble` has. `total + i.toDouble()`
    // with an `int` i came out as `total + i`, which does not compile in Rust
    // and does in Dart. `as f32` is right for both: on an f32 it is the no-op
    // the old rule assumed.
    if (name == 'toDouble' && args.isEmpty) return '($receiver as f64)';
    // A call to a method of this class that can fail carries the failure
    // outward with `?`. That is the propagation the measurement counted, and
    // the caller's own signature was widened by the same fixpoint, so the two
    // always agree.
    // A callee failing with its own type inside a method failing with
    // `Object` (see `_computeFailing`): the error is boxed on the way up.
    // A translated callee returns `Result`: `?` inside a function, and
    // `.unwrap()` where there is none around (a static's initialiser).
    // An awaited call is not `?`ed here but at the `.await`.
    // By the *Rust* name as well as the Dart one: a prelude method the
    // front end maps by table arrives spelled `first_where`, and one the
    // backend only snake-cases arrives spelled `replaceAllMapped` -- and
    // the second kind never matched (`MediaType.toString`, ws811).
    final failing =
        fails ||
        (_resultModel &&
            (_preludeFailing.contains(name) ||
                _preludeFailing.contains(_identifier(name))));
    // The prelude's callback slots that only *call* what they are given
    // are `impl Fn`, and an `Rc<dyn Fn>` is not one. A closure written at
    // the call site is already the closure; a function *value* -- a
    // tear-off `coerce` put behind an `Rc`, or a function-typed local or
    // parameter, which is always a handle here -- is the function itself
    // or a loan of it (`_history.lastWhere(_RouteEntry.isPresentPredicate)`
    // and `_History.indexWhere(test)`, 2 at ws811).
    // ..only on a receiver the prelude owns: a translated class may
    // declare a method of the same name, and `_History.indexWhere(test)`
    // takes the handle its own signature spells (`NavigatorState
    // .finalizeRoute`, +1 at ws812).
    final preludeReceiver =
        target != null &&
        target is! IrThis &&
        (receiverClass == null || library[receiverClass] == null);
    if (preludeReceiver && _preludeLends.contains(_identifier(name))) {
      args = [for (final a in args) _lentFunction(a)];
    }
    // A call reaching an `async fn` *inherently* is its `DartFuture`, no
    // `?`; one reaching it through a trait (`qualifier`, `asTrait` below)
    // gets the trait's `Result<DartFuture<T>, E>` and is unwrapped first,
    // awaited or not (`OptionalMethodChannel.invokeMethod<T>` through its
    // trait, ws432). The front end's `asyncFn` is a guess at the path the
    // backend decides here.
    // ..and through the trait it is a `Result` whether or not the method
    // itself fails: the trait's declaration wraps every async method.
    String suffixFor(bool viaTrait) =>
        (failing || asyncTarget) && !(asyncFn && !viaTrait) ? _propagate : '';
    final boxed = false;
    // `_identifier`, not `snake`: an *operator* called as a method -- `~x` is
    // `x.~()` in Kernel -- has no letters for `snake` to keep, and it came out
    // as `x._()`, which does not parse and stopped the whole crate at the
    // lexer. `_identifier` gives the operator the same name its definition
    // got, and refuses the ones with no Rust name at all.
    // A concrete class's own method is inherent, and an inherent method
    // wins over any trait's: the plain call is unambiguous, and the
    // qualified one passed `&**self` to a `self: &Rc<Self>` receiver (47
    // in `WidgetsFlutterBinding` alone).
    if (qualifier != null && !(library[qualifier]?.isAbstract ?? true)) {
      qualifier = null;
    }
    // A method of a trait the receiver's class implements more than once
    // (`IrClass.extraImpls`): the call names the class's own instantiation.
    final owner = target == null || target is IrThis
        ? cls
        : receiverClass == null
        ? null
        : library[receiverClass];
    // ..not when the class has the method inherently: that is what Dart
    // calls, and it may widen the trait's signature (`MapEquality.equals(
    // Map? e1, ..)` over `Equality<Map>.equals(Map e1, ..)`, ws627).
    var wide =
        owner != null && owner.methods.any((m) => m.name == name && !m.isStatic)
        ? null
        : _wideTraitFor(owner, name);
    // ..or one of *two* traits the receiver's class implements that both
    // declare the method (`RenderBox` re-declares `RenderObject`'s
    // `markNeedsLayout`), with no inherent method to win: the nearest is
    // named (21 E0034 at ws416).
    if (wide == null &&
        owner != null &&
        qualifier == null &&
        !owner.methods.any((m) => m.name == name && !m.isStatic)) {
      // A getter or a method, not a setter of the same Dart name (`value`
      // and `value=`: the setter is `set_value` here, and naming a trait
      // that has only the setter was 49 "cannot find method", ws418).
      bool declares(IrMethod m) => m.name == name && !m.isStatic && !m.isSetter;
      final declaring = _abstractAncestors(owner).where(
        (a) =>
            a.methods.any(declares) ||
            a.abstractMethods.any(declares) ||
            a.fields.any((f) => f.name == name),
      );
      if (declaring.length > 1) wide = declaring.first;
    }
    String? asTrait;
    if (wide != null &&
        owner != null &&
        (qualifier == null || qualifier == wide.name)) {
      // A generic receiver's own instantiation is read off its recorded
      // type (`SetEquality<Rc<dyn Object>>` calling `equals`: plain, it
      // resolved to the wider `Equality<Vec<..>>` impl, ws628).
      final recv = target == null || target is IrThis ? null : target.rustType;
      final ownBinding = <String, IrType>{
        if (!identical(owner, cls) && recv != null)
          for (
            var i = 0;
            i < owner.typeParameters.length && i < recv.arguments.length;
            i++
          )
            owner.typeParameters[i]: recv.arguments[i],
      };
      final passed = _argumentsThrough(owner, ownBinding, wide, {});
      final concrete =
          identical(owner, cls) ||
          owner.typeParameters.isEmpty ||
          (ownBinding.length == owner.typeParameters.length &&
              passed != null &&
              !passed.any((a) => owner.typeParameters.contains(a.name)));
      if (passed != null && concrete) {
        final spelledArgs = passed.isEmpty
            ? ''
            : '<${passed.map((a) => type(a)).join(', ')}>';
        final self = identical(owner, cls)
            ? (_inSuperFn ? '__Self' : 'Self')
            : ownBinding.isEmpty
            ? owner.name
            : '${owner.name}<${owner.typeParameters.map((p) => type(ownBinding[p]!)).join(', ')}>';
        asTrait = '<$self as ${wide.name}$spelledArgs>';
        qualifier = wide.name;
      }
    }
    // On a closure's handle a name two traits declare is ambiguous where
    // `self.name()` was not (`__me.child()` on a struct implementing
    // both `RenderObjectWithChildMixin` and `RenderProxyBox`, E0034 at
    // ws462): the declaring trait, as an accessor read chooses it.
    if (qualifier == null &&
        _selfName == _countedSelf &&
        (target == null || target is IrThis)) {
      final chosen = _accessorQualifier(name);
      if (chosen != null && library.isAbstract(chosen)) qualifier = chosen;
    }
    // A trait with one of the prelude's above it declares a name the
    // supertrait declares too -- `CharacterRange.moveNext([count])` over
    // `Iterator`'s `moveNext()` -- and a call through the object names
    // both, with no inherent method to win (E0034, the iterwide fixture).
    // The receiver's own trait says which is meant, as it does for two
    // translated traits just above (ws859).
    if (qualifier == null && target != null) {
      final on = receiverClass ?? target.rustType?.name;
      final owner = on == null ? null : library[on];
      // Only where the trait *widened* the name: the call takes arguments
      // the supertrait's does not, so both are in scope and neither wins.
      // A name it merely inherits (`current`) is on the supertrait alone,
      // is not ambiguous, and naming the subtrait for it would not even
      // resolve.
      if (owner != null && owner.isAbstract) {
        for (final i in _preludeInterfacesOf(owner)) {
          if (i.arguments.any((a) => a.name == on)) continue;
          for (final s in _preludeInterfaces[i.name] ?? const <String>[]) {
            if (!s.startsWith('${_identifier(name)}(')) continue;
            final inside = s.substring(s.indexOf('(') + 1, s.indexOf(')'));
            final theirs = inside.split(',').length - 1;
            if (args.length != theirs) qualifier = on;
            break;
          }
          if (qualifier != null) break;
        }
      }
    }
    if (qualifier != null) {
      // See `IrCall.qualifier`. `self`/`this_` are already references; a
      // closure's `__me` is a handle, as is any receiver typed by a trait
      // or a counted class.
      // ..and `self` is `&Rc<Self>` in a method that hands out a closure
      // holding it (`_receiverOf`), reached through twice.
      final through = target == null || target is IrThis
          ? (_selfName == 'self'
                ? (_selfIsHandle ? '&**self' : 'self')
                : _selfName == 'this_'
                ? 'this_'
                : cls.counted
                ? '&*$_selfName'
                : '&$_selfName')
          // A cast result is always a handle; a null-aware binding (`it`)
          // is a reference to the value, one deref short of a handle's
          // object and already a reference to a value (79 + 17, ws295).
          : target is IrCastTo
          ? '&*${expr(target)}'
          : target is IrBound
          ? (_isHandle(receiverClass) ? '&**${expr(target)}' : expr(target))
          // ..or a handle by its recorded type, when the class went
          // unrecorded (`widget.toStringShort()` on an `Rc<dyn
          // StatefulWidget>` was `&handle`, ws523).
          : _isHandle(receiverClass) || _handleLike(target)
          ? '&*${expr(target)}'
          : '&${expr(target)}';
      // A trait as the qualifier of a call on `this` is spelled through
      // the type that implements it (`<Self as RenderProxyBox>::set_child`):
      // a bare `Trait::method` is E0782 since edition 2021, and a base
      // constructor's body inlined into a subclass (`_inheritedBodies`)
      // writes the base's fields through the trait's setters (100 at ws443).
      // On a closure's handle (`__me`, an `Rc<dyn Trait>` in a trait body
      // or an `Rc<Struct>`), the plain call: `<__Self as Trait>::m(&*__me)`
      // wanted a `&__Self` where `__me` is the trait object
      // (`initMouseTracker`'s closure, ws461).
      if (_selfName == _countedSelf &&
          (target == null || target is IrThis) &&
          library.isAbstract(qualifier)) {
        // Still qualified -- the plain call was ambiguous where two traits
        // declare the name (`hit_test`, 16 at ws462) -- through the type
        // the handle is: the trait object in a trait body, the struct
        // otherwise.
        final declaring = _declaringTrait(qualifier, _identifier(name));
        final through = declaring ?? qualifier;
        final selfType = _fieldsAreAccessors
            ? 'dyn ${cls.name}${_useArguments(cls)}'
            : 'Self';
        if (typeArguments.isNotEmpty &&
            resultType != null &&
            _fieldsAreAccessors) {
          return _erasedCast(
            resultType,
            '<$selfType as $through${_traitArgsOf(through)}>::${_identifier(name)}__erased'
            '(&*$_selfName${args.isEmpty ? '' : ', '}${args.map(expr).join(', ')})$_propagate',
            method: _methodOf(through, name),
          );
        }
        return _asyncValue(
          '<$selfType as $through${_traitArgsOf(through)}>::${_identifier(name)}$turbofish'
          '(&*$_selfName${args.isEmpty ? '' : ', '}${args.map(expr).join(', ')})${suffixFor(_fieldsAreAccessors)}',
          boxed,
        );
      }
      // The trait named has to be the one *declaring* the item: a base
      // constructor's body inlined into a subclass wrote `this.child = x`
      // as `<Self as RenderView>::set_child`, and `set_child` is the
      // mixin's (`RenderObjectWithChildMixin`), which `RenderView` only
      // inherits (78 E0576 at ws460).
      if (asTrait == null && library.isAbstract(qualifier)) {
        final declaring = _declaringTrait(qualifier, _identifier(name));
        if (declaring != null) qualifier = declaring;
      }
      // ..and a trait the front end named to call through (`asTrait`),
      // when it is generic, through the handle's type as well (a bare
      // `ModalRoute::add_local_history_entry(&*route, ..)` was E0782,
      // run672).
      final path =
          asTrait ??
          _throughOwnInstantiation(target, receiverClass, qualifier) ??
          (library.isAbstract(qualifier) && (target == null || target is IrThis)
              ? '<${_inSuperFn ? '__Self' : 'Self'} as $qualifier${_traitArgsOf(qualifier)}>'
              : _dynQualified(target, qualifier) ?? qualifier);
      // A generic method of the trait, on `this` in a trait body through
      // the qualified path: its erased twin, as the plain call goes (the
      // method is `where Self: Sized` in the trait, and `__Self` may be
      // the trait object -- `getInheritedWidgetOfExactType<T>` calling
      // `getElementForInheritedWidgetOfExactType<T>`, ws494).
      if (typeArguments.isNotEmpty &&
          resultType != null &&
          asTrait == null &&
          library.isAbstract(qualifier) &&
          (target == null || target is IrThis) &&
          (_inSuperFn || _fieldsAreAccessors)) {
        return _erasedCast(
          resultType,
          '$path::${_identifier(name)}__erased'
          '($through${args.isEmpty ? '' : ', '}${args.map(expr).join(', ')})$_propagate',
          method: _methodOf(qualifier, name),
        );
      }
      return _asyncValue(
        '$path::${_identifier(name)}$turbofish'
        '($through${args.isEmpty ? '' : ', '}${args.map(expr).join(', ')})'
        '${suffixFor(true)}',
        boxed,
      );
    }
    // A plain call on `this` inside a trait body -- a super fn's `this_:
    // &__Self`, a trait default's `&self`, a closure's `dart_self_<trait>()`
    // handle in either -- dispatches through the trait, whose async
    // methods return `Result<DartFuture<T>, E>` (`_handleAsMethodCall` in
    // `MethodChannel.setMethodCallHandler`'s super fn, run458).
    final viaTrait =
        _fieldsAreAccessors && (target == null || target is IrThis);
    // `f.then(cb)`: the prelude's takes any callback whose result is a
    // `FutureOr<R>` or an `R` (`IntoFutureOr`), and `R` is what this call
    // declared -- spelled, since a callback returning `FutureOr` leaves it
    // ambiguous (`_LocalizationsState.load`, ws482).
    if (name == 'then' &&
        resultType != null &&
        resultType.name == 'Future' &&
        resultType.arguments.length == 1 &&
        !resultType.arguments.single.isFunction &&
        (receiverClass == null || library[receiverClass] == null)) {
      return '$receiver.then::<${type(resultType.arguments.single)}, _>'
          '(${args.map(expr).join(', ')})${suffixFor(viaTrait)}';
    }
    // A generic method through a trait object: its erased twin, and the
    // result cast back to what this call declared (see the prelude's
    // `CastErased`).
    // ..and on `this` inside a trait body -- a super fn's `this_: &__Self
    // + ?Sized`, a default's `&self` -- where the receiver names no class:
    // the generic method is `where Self: Sized` in the trait, and `__Self`
    // may well be the trait object (`invokeMapMethod` calling
    // `invokeMethod<Map>`, run494).
    if (typeArguments.isNotEmpty &&
        resultType != null &&
        ((receiverClass != null &&
                library.isAbstract(receiverClass) &&
                (target is! IrThis || _fieldsAreAccessors)) ||
            (receiverClass == null && viaTrait))) {
      return _erasedCast(
        resultType,
        '$receiver.${_identifier(name)}__erased'
        '(${args.map(expr).join(', ')})$_propagate',
        method: _methodOf(receiverClass ?? cls.name, name),
      );
    }
    if (Platform.environment['DART2RUST_TRACE_BACKEND'] == name) {
      stderr.writeln(
        'TRACE_BACKEND $name asyncFn=$asyncFn fails=$fails failing=$failing accessors=$_fieldsAreAccessors target=${target.runtimeType} self=$_selfName cls=${cls.name} receiverClass=$receiverClass resultType=$resultType typeArguments=$typeArguments qualifier=$qualifier',
      );
    }
    return _asyncValue(
      '$receiver.${_identifier(name)}$turbofish'
      '(${args.map(expr).join(', ')})${suffixFor(viaTrait)}',
      boxed,
    );
  }

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
        '${expr(chain.source)}.iter()${owned ? '.cloned()' : ''}$steps$tail';
    return bound.isEmpty ? body : '{ ${bound.join(' ')} $body }';
  }

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
