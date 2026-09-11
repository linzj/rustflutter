part of '../backend_rust.dart';

// `_call`: one method call, from receiver to turbofish.
augment class RustBackend {
  /// A receiver whose *value* is a `double` literal, however it is spelled.
  /// Rust resolves a method before it defaults an unsuffixed float, so such
  /// a receiver has to say `f64` outright ("can't call method `min` on
  /// ambiguous numeric type `{float}`", E0689 -- 21 of them in the HCT
  /// colour code, and the negated ones below).
  ///
  /// Through a negation: the front end folds `-_kFlingVelocity` on a `const
  /// double` to `IrUnary('-', literal)`, which is a literal as far as rustc
  /// is concerned but was not one here (`_handleDragEnd` in reply's
  /// `adaptive_nav.dart`, ws956).
  static bool _floatLiteralValue(IrExpr? e) => switch (e) {
    IrLiteral(:final type) => type.name == 'double',
    IrUnary(op: '-', :final operand) => _floatLiteralValue(operand),
    // ..and arithmetic over them: `(1.5 * 0.35).sin()` is every bit as
    // unpinned as `1.5.sin()`, since nothing in it says which float it is
    // (`InkSparkle._updateFragmentShader`, ws969). Only when *both* sides
    // are literals -- with a typed operand anywhere, inference has it.
    IrBinary(:final op, :final left, :final right)
        when const {'+', '-', '*', '/'}.contains(op) =>
      _floatLiteralValue(left) && _floatLiteralValue(right),
    _ => false,
  };

  /// That receiver with the type written on the *literal*: `(-(2.0_f64))`,
  /// not `(-2.0)_f64` -- a suffix belongs to the literal, not to the
  /// expression around it. Once: a literal the front end already suffixed
  /// (an integer written as a double, ws779) would read `0.0_f64_f64`.
  String _suffixedFloat(IrExpr e) {
    if (e is IrUnary && e.op == '-') return '(-${_suffixedFloat(e.operand)})';
    // One suffix pins the whole expression, so it goes on the left-hand
    // literal and the rest is spelled as it stands.
    if (e is IrBinary) {
      return '(${_suffixedFloat(e.left)} ${e.op} ${expr(e.right)})';
    }
    final text = _receiver(e);
    return text.endsWith('_f64') ? '($text)' : '(${text}_f64)';
  }

  String _call(
    IrExpr? target,
    String name0,
    List<IrExpr> args, {
    String? qualifier,
    String? receiverClass,
    bool fails = false,
    List<IrType> typeArguments = const [],
    bool asyncFn = false,
    bool asyncTarget = false,
    IrType? resultType,
  }) {
    // Dart's `Completer.complete` takes a `FutureOr<T>`, not just a `T`:
    // handed a future, the completer's own future settles when that one
    // does. `SchedulerBinding.scheduleTask`'s `TaskCallback<T>` returns a
    // `FutureOr<T>` and hands its result straight over ("`?` operator has
    // incompatible types", E0308, ws977). The prelude keeps the two apart
    // (`complete_or`), so that `complete`'s parameter stays the projected
    // `T?` that every other call site passes.
    // The `FutureOr` sits *inside* the `Some` the projected `T?` slot puts
    // round it (`IrSome`), so the wrapper's own type says nothing -- reading
    // it was a rule that never fired (82 -> 82, byte-identical, ws977).
    // `complete_or` takes the `Option` too, which is Dart's own signature:
    // `complete([FutureOr<T>? value])`.
    final completeArg = args.length == 1 ? args.single : null;
    final name =
        name0 == 'complete' &&
            (completeArg is IrSome
                    ? completeArg.value.rustType?.name
                    : completeArg?.rustType?.name) ==
                'FutureOr'
        ? 'complete_or'
        : name0;
    final turbofish = typeArguments.isEmpty
        ? ''
        : '::<${typeArguments.map(type).join(', ')}>';
    // Cleared before the receiver and arguments print: a call inside them
    // would otherwise take this `await`'s flag.
    _awaiting = false;
    // A call on a receiver that never returns is that receiver. Dart
    // evaluates the receiver first, so the call is not reached either --
    // and in Rust a method on a `!` has no type to resolve against ("type
    // annotations needed ... cannot infer type", E0282). The AOT compiler
    // plants such a receiver wherever it proves a value cannot exist, and
    // `DropdownMenuThemeData.inputDecorationTheme` reads as one
    // (`{ let __t1 = ..; unreachable!(..) }.data(..)`, ws965).
    if (_neverReturns(target)) return expr(target!);
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
    // ..and only where the receiver really *is* a collection. A handle's
    // method that merely shares a mutator's name is not a mutation of a
    // place: `entry!.remove()` on a `LocalHistoryEntry` took this path
    // because `remove` is what a `List` does, and asked a closure's
    // captured binding for a `&mut` it was never given ("cannot borrow
    // `entry` as mutable", E0596: `ScaffoldState._buildBottomSheet`,
    // ws974). The same lesson as ws509 one line below, which learned it
    // for a *field* and left the general case here alone.
    //
    // Unknown type: the path stands, as it did before. Guessing "not a
    // collection" where nothing is recorded would silently move a real
    // mutation onto a clone, which is what ws944 cost.
    final receiverType = target?.rustType;
    final mutatesACollection =
        receiverType == null || _isMutableCollection(type(receiverType));
    final cellPlace = _mutatesInPlace(name) && mutatesACollection
        ? _mutPlace(target)
        : null;
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
        : _floatLiteralValue(target)
        ? _suffixedFloat(target!)
        : _receiver(target);
    // `HashMap` looks up by reference, and gives back a reference to the
    // value. Dart's `m[k]` is a `V?`, so the borrow is cloned away rather
    // than leaked into every caller's type.
    // A value shared into a trait object (see `_widened`).
    // `this` shared: the object's own handle, not a fresh `Rc` around a
    // reference (`Rc::new(this_)` wanted `'static`, 168 lifetime errors)
    // or a copy (a new identity).
    // A `late` local read: the front end spells it `x.clone().!late`, and
    // the `Option` it opens is the one place that can tell whether Dart
    // would have thrown here. Dart calls it a `Local`.
    if (name == '!late' && args.isEmpty) {
      final read = target is IrCall ? target.target : target;
      return '$receiver'
          '${_lateRead(read is IrLocal ? read.name : '', kind: 'Local')}';
    }
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
      return '$receiver.unwrap_or_else(|| std::rc::Rc::new(Null) as ${dartHandle})';
    }
    // The other way: a `dynamic` as an `Option`, `None` for the `Null` object.
    //
    // The handle goes *in* and comes back inside the `Option`, so a bare
    // local is moved by it and a body that reads the same local again is
    // E0382. `_MasterDetailScaffold.build` writes `value ?? ..` twice in one
    // expression and was a stub for it. Cloned the way an argument is
    // (`passed`), which for an `Rc` clones the handle; the sibling shape in
    // `IrIsNull` has cloned here all along.
    if (name == '!nullable' && args.isEmpty) {
      final held =
          target is IrLocal &&
          !_cellLocals.containsKey(target.name) &&
          !_closureCaptured.contains(target.name);
      return 'dart_nullable($receiver${held ? '.clone()' : ''})';
    }
    if (name == '!widen_object' && args.isEmpty) {
      // `iter().cloned()`: the receiver may be the `&Vec` a null-aware
      // `as_ref().map(|it| ..)` binds, and `into_iter` on that yields
      // references (E0282 in `ColorFilter.hashCode`).
      return '$receiver.iter().cloned().map(|v| Some(std::rc::Rc::new(v) as ${dartHandle})).collect::<Vec<_>>()';
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
          return '($_selfName.dart_self_${snakeRaw(cls.name)}() as ${dartHandle})';
        }
        if (cls.counted) {
          return '($_selfName.dart_self_ref().get() as ${dartHandle})';
        }
        return _selfIsHandle
            ? '($_selfName.clone() as ${dartHandle})'
            : '(std::rc::Rc::new($_selfName.clone()) as ${dartHandle})';
      }
      return '($receiver as ${dartHandle})';
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
          return '($_selfName.dart_self_${snakeRaw(cls.name)}() as ${dartHandle})';
        }
        if (cls.counted) {
          return '($_selfName.dart_self_ref().get() as ${dartHandle})';
        }
        if (_selfName == 'self') {
          return _selfIsHandle
              ? '(self.clone() as ${dartHandle})'
              : '(std::rc::Rc::new(self.clone()) as ${dartHandle})';
        }
      }
      return '(std::rc::Rc::new($receiver) as ${dartHandle})';
    }
    if (name == '!dart_eq' && args.length == 1) {
      // Two function values that take a *different number of arguments*
      // are never the same object -- no Dart function value has two
      // arities -- so Dart's `==` on them is false, and there is nothing
      // to compare. `dart_eq` takes `&Self`, and the two are different
      // types (E0308).
      //
      // `_TimePickerModel.updateShouldNotifyDependent` compares
      // `onHourMinuteModeChanged != oldWidget.onHourDoubleTapped`: a
      // `ValueChanged<_HourMinuteMode>` against a `VoidCallback`. That is
      // upstream Flutter's own copy-paste -- the two lines below it read
      // the same field on the left, in
      // `packages/flutter/lib/src/material/time_picker.dart` and in the
      // kernel, which carries it as written. Answering it the way Dart
      // does is the faithful translation.
      final held = target?.rustType;
      final given = args.single.rustType;
      if (held != null &&
          given != null &&
          held.isFunction &&
          given.isFunction &&
          held.parameters!.length != given.parameters!.length) {
        return 'false';
      }
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
      // Borrowed, not moved. `get` needs only a reference and `cloned()`
      // hands back an owned value, so binding the map by value consumed it:
      // inside a loop that is "use of moved value ... in previous iteration
      // of loop" (E0382, `_updateChildren`'s `oldKeyedElements` and
      // `Table.update`'s `oldKeyedRows`). A `let` binding a borrow extends
      // the temporary to the end of the block, so a receiver that is itself
      // a call still lives long enough.
      return '{ let __m = &$receiver; ${expr(args.single)}.as_ref().and_then(|__k| __m.get(__k).cloned()${_flattenedValue(target)}) }';
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
    // A collection into an `Iterable` slot: the trait handle
    // (`DartIterable`, ws908).
    if (name == '!as_iterable' && args.isEmpty) {
      // Cloned only when the receiver is a *place* someone else holds.
      // The rule used to be "always", on the grounds that the receiver is
      // a borrow as often as a value; counted against the output, that is
      // 26 `&mut Vec<T>` parameters and no `&Vec<T>`/`&Set<T>` at all,
      // while 267 of the 274 boxings clone a temporary nobody holds --
      // and 56 of those had already been cloned by `expr` itself
      // (`(x.clone()).clone()`).
      final owned = _ownedWhenSpelled(target);
      // The *value's* own element, not the slot's: the two are the same
      // by the time this runs (the coercion shapes the list element by
      // element first), and a callee's slot may spell a type parameter
      // that is not a name here -- a named parameter's type is the
      // declared one, uninstantiated (`RestorableEnumN<Orientation>(..,
      // values: Orientation.values)`, 14 `cannot find type T`).
      final element =
          target?.rustType?.arguments.singleOrNull ??
          resultType?.arguments.singleOrNull;
      final spelled = element == null ? '_' : type(element);
      final held = owned ? receiver : '($receiver).clone()';
      return '(std::rc::Rc::new($held) as '
          'std::rc::Rc<dyn DartIterable<$spelled>>)';
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
      // ..and an `Iterable` is the handle, whose list is `dart_to_list`
      // (named apart from `DartList::to_list`, which `Vec` also has).
      if (held != null && held.name == 'Iterable') {
        return '$receiver.dart_to_list()';
      }
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
    // `Object`: the error is boxed on the way up. (One error type for
    // them all now, so the two are the same type -- `_resultModel`.)
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
      bool declarer(IrClass a) =>
          a.methods.any(declares) ||
          a.abstractMethods.any(declares) ||
          a.fields.any((f) => f.name == name);
      final declaring = _abstractAncestors(owner).where(declarer).toList();
      // ..or once along Dart's chain and again in a trait only Rust put
      // above it (`_boundOnlyTraits`): a super function's `__Self` carries
      // the bounds its ancestors' super calls asked for, and a name one of
      // those declares too is a second candidate Dart never saw
      // (`constraints` on `RenderAbstractLayoutBuilderMixin`, whose
      // `RenderObjectWithLayoutCallbackMixin` reaches `RenderBox` through
      // `super`: 2 E0034 at ws970). The answer is still Dart's -- the one
      // trait its chain declares the name on.
      if (declaring.length > 1 ||
          (declaring.length == 1 && _boundOnlyTraits(owner).any(declarer))) {
        wide = declaring.first;
      }
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
            ? (_inSuperFn ? _superSelf : 'Self')
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
    // A class may override a mixin's member and *add* an optional named
    // parameter, which Dart allows: `SemanticsNode.toDiagnosticsNode`
    // takes `childOrder` beside the `name` and `style` that
    // `DiagnosticableTree` declares. The mixin's body is emitted into the
    // class -- `impl SemanticsNode` -- so `self.to_diagnostics_node(name,
    // style)` resolves to the class's own three-argument method and is one
    // argument short. The trait's is the one the call means, and the
    // forwarding impl beside it already fills the default
    // (`SemanticsNode::to_diagnostics_node(.., TraversalOrder)`).
    //
    // By the *count*, which is the whole disagreement: a name whose
    // arities agree resolves inherently as before.
    if (qualifier == null && (target == null || target is IrThis)) {
      final own = cls.methods
          .where((m) => m.name == name && !m.isStatic && !m.isSetter)
          .firstOrNull;
      if (own != null && own.params.length != args.length) {
        for (final t in _supertypesOf(cls)) {
          if (!library.isAbstract(t.name)) continue;
          final theirs = [
            ...t.methods.where((m) => !m.isStatic && !m.isSetter),
            ...t.abstractMethods.where((m) => !m.isSetter),
          ].where((m) => m.name == name).firstOrNull;
          if (theirs != null && theirs.params.length == args.length) {
            qualifier = t.name;
            break;
          }
        }
      }
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
          // ..and a chain step's parameter, which `iter()` hands over as a
          // reference to the handle rather than the handle
          // (`_refLocals`; `FocusNode.toDiagnosticsNode` on a `.map`'s
          // child was `&*child`, one deref short).
          : target is IrLocal &&
                _refLocals.contains(snake(target.name)) &&
                _isHandle(receiverClass)
          ? '&**${expr(target)}'
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
              ? '<${_inSuperFn ? _superSelf : 'Self'} as $qualifier${_traitArgsOf(qualifier)}>'
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
}
