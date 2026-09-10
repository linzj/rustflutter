part of '../backend_rust.dart';

augment class RustBackend {
  // -- Statements -------------------------------------------------------------

  /// Locals assigned somewhere in the body currently being emitted.
  ///
  /// Rust needs `let mut` at the *declaration*, and whether one is needed is a
  /// fact about the whole body, not about the line. So the body is walked once
  /// before it is emitted. Marking every local `mut` would compile too, and
  /// would bury the ones that really are reassigned under a warning apiece.
  var _reassigned = <String>{};

  /// The place a `for` loop should iterate mutably, or null when the
  /// iterable hands out a value of its own.
  ///
  /// `m.values` is the one that has to be spelled apart: it collects a
  /// fresh `Vec` of clones, so the loop must go through the map itself.
  String? _iterMutPlace(IrExpr iterable) {
    final it = _withoutClone(iterable);
    if (it is IrCall &&
        it.name == 'values' &&
        it.args.isEmpty &&
        it.target != null) {
      final owner = _iterMutTarget(it.target!);
      return owner == null ? null : '$owner.values_mut()';
    }
    final place = _iterMutTarget(it);
    return place == null ? null : '$place.iter_mut()';
  }

  String? _iterMutTarget(IrExpr e) {
    final it = _withoutClone(e);
    // A local is the one thing this backend spells bare, and it is already
    // `mut` when `_reassigned` says so.
    if (it is IrLocal && !_cellLocals.containsKey(it.name)) {
      return snake(it.name);
    }
    return _mutPlace(it);
  }

  IrExpr _withoutClone(IrExpr e) =>
      e is IrCall && e.name == 'clone' && e.args.isEmpty && e.target != null
      ? _withoutClone(e.target!)
      : e;

  Set<String> _assignedIn(IrStmt statement) {
    final found = <String>{};
    // An assignment can also be an *expression* -- `f(total = x)` -- and the
    // local it writes needs `mut` just the same. Walking only statements left
    // one immutable and the file did not compile.
    final inExpressions = _WalkSelf();
    inExpressions.statement(statement);
    found.addAll(inExpressions.assignedLocals);
    found.addAll(inExpressions.mutatedLocals);
    found.addAll(inExpressions.receiverLocals);
    void walk(IrStmt s) {
      switch (s) {
        case IrAssign(:final name):
          found.add(name);
        case IrBlock(:final statements):
          statements.forEach(walk);
        case IrIf(:final then, :final otherwise):
          walk(then);
          if (otherwise != null) walk(otherwise);
        case IrTryCatch(:final body, :final handler):
          // Walked into, not skipped: a local assigned inside a `try` still
          // needs `mut` at its declaration outside it.
          walk(body);
          walk(handler);
        case IrTryFinally(:final body, :final finalizer):
          walk(body);
          walk(finalizer);
        case IrWhile(:final body):
          walk(body);
        case IrLabeled(:final body):
          walk(body);
        case IrContinueSwitch():
          break;
        case IrSwitch(:final cases, :final otherwise):
          for (final one in cases) {
            walk(one.body);
          }
          if (otherwise != null) walk(otherwise);
        case IrForIn(:final body):
          walk(body);
        // `xs[i] = v` needs `xs` mutable, which nothing here said. It only
        // shows on a list written through a name rather than through `self`.
        case IrIndexSet(:final target):
          if (target is IrLocal) found.add(target.name);
        case IrLocalFunction():
        case IrBreak():
        case IrContinue():
        case IrReturn():
        case IrLocalDecl():
        case IrExprStmt():
        case IrAssert():
        case IrSetter():
        case IrThrow():
        case IrAssignField():
        case IrAssignTopLevel():
        case IrAssignStatic():
      }
    }

    walk(statement);
    return found;
  }

  /// Emits a statement. `tail` marks the position whose value is the block's --
  /// Rust's trailing expression, which is how a `return` at the end of a method
  /// stops needing the keyword.
  /// A body under the Result model: a `void` one that falls off its end
  /// ends in `Ok(())`, since the signature says `Result<(), E>`.
  /// `DART2RUST_RUNTIME_TRACE=a,b,c`: every member whose `Class.name`
  /// contains one of the names prints its name to stderr on entry, in the
  /// translated program. Chosen when translating; nothing at run time.
  static final _runtimeTraced =
      (Platform.environment['DART2RUST_RUNTIME_TRACE'] ?? '')
          .split(',')
          .where((s) => s.isNotEmpty)
          .toList();

  /// Emits a body, and says whether it ended with the null its type falls
  /// into: a caller that closes an open `if` chain has nothing left to close
  /// when it did (the two tails did not parse, the fallnull fixture).
  bool _body(IrStmt body, IrType returnType) {
    if (_runtimeTraced.any(_here.contains)) {
      _line('eprintln!("dart2rust trace: $_here");');
    }
    final rendered = type(returnType);
    // A body that falls off its end returns the null of its type: `()`,
    // `None` for an `Option` (a `Null` callback's, 52 in `widgets`), and
    // the done `FutureOr` of either (a `then` callback's `FutureOr<void>`,
    // `Route.didAdd`, ws504; `FutureOr<Null>` once `FutureOr` stopped
    // being an `Option` of its own, the thenvoid fixture).
    final falling = _fallsOffValue(rendered);
    final fallsOff =
        _failure != null && falling != null && !_alwaysReturns(body);
    // ..and a bare `return;` in the body hands back that same value: it
    // is the language's `return null`, not a `void`'s nothing (a `then`
    // callback's `FutureOr<void>` with an `if (!mounted) { return; }`
    // guard, `_SettingsListItemState._handleExpansion`, run692).
    final savedFalling = _fallsOff;
    _fallsOff = falling;
    stmt(body, tail: !fallsOff);
    _fallsOff = savedFalling;
    if (fallsOff) _line('Ok($falling)');
    return fallsOff;
  }

  /// The null of the return type of the body being emitted: what a bare
  /// `return;` in it hands back, as falling off its end does.
  String? _fallsOff;

  /// The value a body of the rendered return type falls off into, or null
  /// when the type has no null to fall into.
  static String? _fallsOffValue(String rendered) {
    if (rendered == '()') return '()';
    // Spelled: nothing else says what the `None` is when the body's own
    // return type is a `_` the context cannot fill -- a `catch_error`
    // handler is held as an `Rc<dyn Object>` and constrains nothing
    // (`AssetImage.obtainKey`, ws726).
    if (rendered.startsWith('Option<') && rendered.endsWith('>')) {
      return 'None::<${rendered.substring('Option<'.length, rendered.length - 1)}>';
    }
    // A `dynamic` (a `Function` slot's callback, `RestorableBool.value =
    // ..` inside one, `_AnimatedHomePageState.build`, run675): Dart's
    // null, the `Null` object.
    if (rendered == 'std::rc::Rc<dyn Object>') return 'dart_null_object()';
    if (rendered.startsWith('FutureOr<') && rendered.endsWith('>')) {
      final inner = _fallsOffValue(
        rendered.substring('FutureOr<'.length, rendered.length - 1),
      );
      return inner == null ? null : 'FutureOr::value($inner)';
    }
    return null;
  }

  void stmt(IrStmt s, {bool tail = false}) {
    switch (s) {
      case IrBlock(:final statements):
        for (var i = 0; i < statements.length; i++) {
          stmt(statements[i], tail: tail && i == statements.length - 1);
          // Nothing after a `return`: type flow analysis leaves the dead
          // tail of a body in place, and emitting it put an `else`-less
          // `if` in the function's tail position -- E0317, and a panic at
          // runtime once the function was stubbed for it
          // (`BaseTapAndDragGestureRecognizer._resetDragUpdateThrottle`,
          // run785).
          if (_alwaysReturns(statements[i])) break;
        }
      case IrReturn(:final value):
        // In a failing method every ordinary return is a success: Rust needs
        // the `Ok`, and leaving it off is a type error rather than a quiet
        // wrong answer, which is the one comfort here.
        final wrap = _failure != null;
        final falls = _fallsOff ?? '()';
        final returned = value == null
            ? (wrap ? 'Ok($falls)' : (falls == '()' ? '' : falls))
            : (wrap ? 'Ok(${_returned(value)})' : _returned(value));
        if (_inFlowClosure) {
          // Inside the try closure this is not a return from the method yet --
          // it is a value handed to the `match` outside, which does the real
          // returning. `tail` does not apply: the closure's own tail is the
          // `Ok(None)` that says the body fell off the end.
          _line('return Ok(Some(${returned.isEmpty ? '()' : returned}));');
        } else {
          _line(tail ? returned : 'return $returned;');
        }
      case IrThrow(:final value):
        // A thrown string where the function's error type is `Object` -- the
        // tree shaker's "code removed by TFA" throws, 36 of them -- is boxed
        // into the error type rather than left as a `String` in an `Rc`'s
        // place.
        _line('${_thrown(value)};');
      case IrTryFinally(:final body, :final finalizer):
        // The finalizer has to run on the way out however the body leaves, so
        // the body's exits are all collected into one value first and only
        // dispatched after it has run. `Drop` is the usual Rust answer and is
        // the wrong one here: a guard's `drop` cannot use `?` or `return`, and
        // the finalizer often does neither but the *dispatch* does both.
        //
        // Nothing here catches: an `Err` is handed straight back on. A
        // `try/catch/finally` is a TryCatch inside this node, so the catching
        // has already happened by the time the value gets here.
        final flows = _returnsEarly(body);
        final carried = flows ? 'Option<${_rustReturns ?? '()'}>' : '()';
        final failure = _failure ?? 'std::convert::Infallible';
        // In an `async fn` the body goes in an `async` block, not a closure:
        // a closure is its own function and an `.await` inside it is
        // "outside async" -- 13 `E0728`s, every one a `try` around an
        // `await`. The block has the same `return` semantics.
        _line(
          _asyncBody
              ? 'let __finally: Result<$carried, $failure> = async {'
              : 'let __finally = (|| -> Result<$carried, $failure> {',
        );
        _indent++;
        final wasFlowing = _inFlowClosure;
        _inFlowClosure = flows;
        stmt(body);
        _inFlowClosure = wasFlowing;
        _line('#[allow(unreachable_code)]');
        _line(flows ? 'Ok(None)' : 'Ok(())');
        _indent--;
        _line(_asyncBody ? '}.await;' : '})();');
        stmt(finalizer);
        _line('match __finally {');
        _indent++;
        if (flows) {
          // Inside an outer try's closure the return is that closure's
          // value again (`inflateWidget`'s try in a try, ws482).
          _line(
            wasFlowing
                ? 'Ok(Some(__returned)) => return Ok(Some(__returned)),'
                : 'Ok(Some(__returned)) => return __returned,',
          );
          _line(
            _alwaysReturns(body)
                ? "Ok(None) => unreachable!(\"the try body always returns\"),"
                : 'Ok(None) => {}',
          );
        } else {
          _line('Ok(()) => {}');
        }
        // The failure keeps going: this method's signature already says
        // `Result`, as every method's does, and a `finally` catches
        // nothing that would stop it.
        // A method that cannot fail wrapped its body in `Infallible`, and
        // the arm is impossible: matching the empty enum says so, where a
        // `return Err(..)` did not type in a `()` method (E0308).
        _line(
          _failure == null
              ? 'Err(__failed) => match __failed {},'
              : 'Err(__failed) => return Err(__failed),',
        );
        _indent--;
        _line('}');
      case IrTryCatch(
        :final body,
        :final error,
        :final errorType,
        :final handler,
        :final stack,
      ):
        // The body goes inside an immediately-invoked closure, and that is the
        // load-bearing part: a failing call inside it is spelled `?`, and `?`
        // returns from the function it is written in. In a closure it returns
        // from the closure -- which is what `try` means -- and written inline
        // it would return from the enclosing method, escaping the very `catch`
        // that was supposed to stop it.
        // The closure's error type comes from the try *body*, not from the
        // enclosing method: a method that catches does not fail, so it has no
        // error type of its own, and `Result<(), _>` cannot be inferred.
        // A body with nothing that fails -- `listener()` behind a catch-all
        // in `ChangeNotifier.notifyListeners` -- leaves `_` with nothing to
        // infer it from (E0282). A catch-all catches an `Object`.
        // A typed clause (`on ArgumentError catch (e)`) does not narrow
        // the closure: every failure travels as the model's one error
        // type, and a closure declared `Result<_, ArgumentError>` could
        // not take the `?` of a callee inside it (`TextSpan.build` around
        // `builder.addText`, stubbed, run683). The arm below asks the
        // error whether it is the caught type and hands the rest back on.
        final failure = _failure ?? 'std::rc::Rc<dyn Object>';
        // The closure catches `?`, and it would catch a `return` too: written
        // plainly, `return x` in the body returns from the *closure* and the
        // method carries on, which compiles and is wrong. So when the body
        // returns, the closure carries the control flow out as a value --
        // `Some(x)` for "the body returned x", `None` for "it fell off the
        // end" -- and the match below does the returning for real.
        final flows = _returnsEarly(body);
        final carried = flows ? 'Option<${_rustReturns ?? '()'}>' : '()';
        // The same async-block rule as `try/finally` above: the handler
        // wrapper must not be a closure when the body awaits.
        _line(
          _asyncBody
              ? 'match async { let __r: Result<$carried, $failure> = {'
              : 'match (|| -> Result<$carried, $failure> {',
        );
        _indent++;
        final outer = _inFlowClosure;
        _inFlowClosure = flows;
        stmt(body);
        _inFlowClosure = outer;
        final always = flows && _alwaysReturns(body);
        if (flows) {
          // A body whose every path returns never reaches this, and Rust says
          // so; the line is still needed for the bodies where some path does
          // not.
          _line('#[allow(unreachable_code)]');
          _line('Ok(None)');
        } else {
          _line('Ok(())');
        }
        _indent--;
        _line(_asyncBody ? '}; __r }.await {' : '})() {');
        _indent++;
        if (flows) {
          // Inside an outer try's closure the return is that closure's
          // value again (`inflateWidget`'s try/catch inside its
          // try/finally, ws485).
          _line(
            outer
                ? 'Ok(Some(__returned)) => return Ok(Some(__returned)),'
                : 'Ok(Some(__returned)) => return __returned,',
          );
          // `{}` has type `()`, and when every path through the body returns
          // there is nothing after the match to give the method its value --
          // so the arm has to say it cannot happen rather than fall through.
          _line(
            always
                ? "Ok(None) => unreachable!(\"the try body always returns\"),"
                : 'Ok(None) => {}',
          );
        } else if (_alwaysReturns(body)) {
          // Every path through the body throws, so the body never completes
          // normally -- and with the handler's every path returning too, the
          // `match` is the method's tail and `{}` is a `()` where its value
          // goes. `unreachable!` is `!` and coerces to whatever the tail
          // wants, as the `flows` arm above already says for returns (a
          // typed catch on a body that only throws, ws826).
          _line('Ok(()) => unreachable!("the try body always throws"),');
        } else {
          _line('Ok(()) => {}');
        }
        _line(
          errorType == null
              ? 'Err(${snake(error)}) => {'
              : 'Err(__caught) => {',
        );
        _indent++;
        if (errorType != null) {
          // The test is the language's own `is` (`_isTest`: a trait by
          // `dart_cast_to`, a struct by `Any`), and the binding its `as`;
          // an error of another type is thrown on as `throw` is.
          final caught = IrLocal('__caught')..rustType = const IrType('Object');
          final target = IrType(errorType);
          _line('if ${_isTest(caught, target, true)} { ${_thrown(caught)}; }');
          final bound = library.isAbstract(errorType)
              ? IrCastTo(caught, target)
              : IrDowncast(caught, errorType, arguments: const []);
          _line('let ${snake(error)} = ${expr(bound)}.clone();');
        }
        // The catch clause's stack trace: the catch site's own, since a
        // `Result` carries none (see the front end's note).
        if (stack != null) {
          _line('let mut ${snake(stack)} = StackTrace::current();');
        }
        stmt(handler);
        _indent--;
        _line('}');
        _indent--;
        _line('}');
      case IrForIn(:final name, :final iterable, :final body):
        // Borrowed, not moved: Dart's loop does not consume the list, and a
        // body that changed it while borrowing would be refused by rustc --
        // which is the same thing Dart refuses at runtime.
        // Each element cloned out, as a field read is: Dart's loop variable
        // is the element, not a reference to it, and `&xs` handed out
        // `&f64` where `f64` was wanted (14 in the colour code). The list
        // itself is only borrowed, as before.
        // ..*unless* the body mutates the loop variable. Dart's `final` is
        // about the binding, not the object: `for (final childSet in ..)
        // childSet.removeWhere(..)` changes the set the loop handed out,
        // and against a clone it changes nothing. Adding `mut` to the
        // binding only makes that compile -- the mutation still lands on
        // the copy, which is a wrong answer where there used to be a stub,
        // and `fx/forinmut` reads `4,4/abbccc|dddde` against Dart's
        // `2,2/abb+|e+` when it is done that way.
        //
        // So iterate the place instead, and only when there *is* a place:
        // a call's result or a parameter read owns what it hands out and
        // nothing can be written back through it. Where there is none this
        // falls through to the clone, and the body that mutates stays a
        // stub rather than becoming a silent no-op.
        // Asked of the loop's own body, and only about calls that really
        // mutate. `_reassigned` is the member-wide `let mut` answer and
        // counts every receiver -- `recipe.clone()` and `fields.clone()
        // .len()` mark their locals there -- so reading a semantic
        // decision out of it lent two collections that nothing writes
        // (E0502 on a body that also reads the list, E0596 on one captured
        // by a `Fn` closure; ws982 measured both, +7 stubs).
        //
        // This predicate is the whole rule. Two further guards were written
        // while the cause was still misread -- skip when the element is a
        // handle, skip when its type is unknown -- and each was ablated
        // against the chain afterwards: 79 stubs either way, stub set
        // byte-identical. They fired nowhere once the question being asked
        // was the right one, so neither survives (the ws966 standard).
        final mutated = _WalkSelf()..statement(body);
        final mutPlace = mutated.inPlaceLocals.contains(name)
            ? _iterMutPlace(iterable)
            : null;
        _line(
          mutPlace != null
              ? 'for ${snake(name)} in $mutPlace {'
              : 'for ${snake(name)} in ${_asList(iterable)}.iter().cloned() {',
        );
        _indent++;
        stmt(body);
        _indent--;
        _line('}');
      case IrIndexSet(:final target, :final index, :final value):
        // A write into one of this class's own lists is a write into the
        // place, not into the clone a field *read* takes out:
        // `self._m4storage.clone()[14] = v` changed nothing, 17 times in
        // vector_math, and left the method `&self`.
        // ..and into one held in a cell -- a counted class's storage, its
        // own or another object's (`cascaded._m4storage[i] = 1.0` on a
        // counted `Matrix4`, ws511) -- through the cell's `borrow_mut`.
        final cellPlace = _mutPlace(target);
        final place = cellPlace != null
            ? cellPlace
            : target is IrField &&
                  (target.target == null || target.target is IrThis) &&
                  _sharedField(target.name) == null &&
                  !_fieldsAreAccessors &&
                  _allFields(cls).any((f) => f.name == target.name)
            ? '${_receiver(target.target)}.${snake(target.name)}'
            : expr(target);
        // The index first: `self.f[self.index(r, c)] = v` borrows `self`
        // twice at once (5 E0502s in vector_math).
        _line(
          '{ let __i = ${expr(index)} as usize; $place[__i] = ${expr(value)}; }',
        );
      case IrLocalFunction(
        :final name,
        :final closure,
        :final recursive,
        :final lends,
      ):
        // A binding that is only called is a plain `let`: no handle, so the
        // closure may borrow what it reads (`IrLocalFunction.lends`).
        if (lends) {
          final savedLending = _lendingClosure;
          _lendingClosure = true;
          // `mut`: a closure that borrows anything mutably -- through the
          // reborrow of `&mut self`, or a local it changes -- is an `FnMut`,
          // and calling one needs the binding to be mutable. Where it is not
          // one rustc says only that the `mut` was not needed.
          _line('let mut ${snake(name)} = ${expr(closure)};');
          _lendingClosure = savedLending;
          break;
        }
        if (!recursive) {
          // Behind the handle every function value is (`Rc<dyn Fn>`): a
          // bare closure bound to `listener` could not be handed to
          // `property.addListener(listener)` (`registerForRestoration`,
          // ws547); a call on the handle reads the same.
          final boxed = closure.boxed ? '' : 'std::rc::Rc::new';
          _line('let ${snake(name)} = $boxed(${expr(closure)});');
          break;
        }
        // A closure cannot name itself: the binding is a cell, filled
        // after the closure is made with a handle to the same cell, and
        // every read of the name -- inside the body and after it -- goes
        // through the cell (`_cellLocals`, unwrapped as a `late` local).
        final fnType = type(
          IrType.function([
            for (final p in closure.params) p.type,
          ], closure.returns),
        );
        _line(
          'let ${snake(name)}: std::rc::Rc<std::cell::RefCell<Option<$fnType>>> = std::rc::Rc::new(std::cell::RefCell::new(None));',
        );
        _cellLocals = {..._cellLocals, name: false};
        _lateCellLocals = {..._lateCellLocals, name};
        // The closure moves a handle to the cell, not the cell's binding.
        final boxed = closure.boxed ? '' : 'std::rc::Rc::new';
        _line(
          '*${snake(name)}.borrow_mut() = Some($boxed({ let ${snake(name)} = ${snake(name)}.clone(); ${expr(closure)} }));',
        );
      case IrLabeled(:final label, :final body):
        _line("'$label: {");
        _indent++;
        stmt(body);
        _indent--;
        _line('}');
      case IrBreak(:final label):
        _line(label == null ? 'break;' : "break '$label;");
      case IrContinue():
        _line('continue;');
      case IrContinueSwitch(:final label, :final arm):
        // `continue <case>;`: set the arm number the loop matches on and go
        // round again (see `IrSwitch.threadLabel`).
        _line('$label = $arm;');
        _line("continue '$label;");
      case IrSwitch(
            :final value,
            :final cases,
            :final otherwise,
            :final threadLabel,
          )
          when threadLabel != null:
        // A switch one of whose cases says `continue <case>`. Rust's `match`
        // runs one arm and is done, so the arms are *numbered* and run
        // inside a labelled loop: the value picks the first number, an arm
        // that continues picks the next, and falling out of the match ends
        // the switch. The value is matched once, as Dart evaluates it once.
        // The arm number, by the same two spellings the ordinary switch
        // has: a `match` where every case value is a Rust pattern, an
        // if-chain where one is not -- `case '[':` is a `String`, and a
        // `"[".to_string()` arm is "expected a pattern, found an
        // expression" (`LicenseEntryWithLineBreaks.paragraphs` again, the
        // shape that made this loop necessary in the first place).
        if (cases.every((c) => c.values.every(_isPattern))) {
          _line('let mut $threadLabel: i64 = match ${expr(value)} {');
          _indent++;
          for (var i = 0; i < cases.length; i++) {
            _line('${cases[i].values.map(expr).join(' | ')} => $i,');
          }
          _line('_ => ${cases.length},');
          _indent--;
          _line('};');
        } else {
          final held = '${threadLabel}_value';
          _line('let $held = ${expr(value)};');
          _line('let mut $threadLabel: i64 = ');
          _indent++;
          for (var i = 0; i < cases.length; i++) {
            final test = cases[i].values
                .map((v) => '$held == ${expr(v)}')
                .join(' || ');
            _line('if $test { $i } else');
          }
          _line('{ ${cases.length} };');
          _indent--;
        }
        _line("'$threadLabel: loop {");
        _indent++;
        _line('match $threadLabel {');
        _indent++;
        for (var i = 0; i < cases.length; i++) {
          _line('$i => {');
          _indent++;
          stmt(cases[i].body);
          _indent--;
          _line('}');
        }
        _line('_ => {');
        _indent++;
        if (otherwise != null) stmt(otherwise);
        _indent--;
        _line('}');
        _indent--;
        _line('}');
        _line("break '$threadLabel;");
        _indent--;
        _line('}');
      case IrSwitch(:final value, :final cases, :final otherwise):
        // Rust's `match` takes *patterns*, and only some Dart case values are
        // one. An enum variant and an integer are; a string is not, and
        // `"x".to_string()` in an arm is "expected a pattern, found an
        // expression" -- 266 of those. Those switches become the if-else chain
        // they always were.
        if (!cases.every((c) => c.values.every(_isPattern))) {
          var first = true;
          for (final one in cases) {
            final test = one.values
                .map((v) => '${expr(value)} == ${expr(v)}')
                .join(' || ');
            _line('${first ? 'if' : '} else if'} $test {');
            first = false;
            _indent++;
            stmt(one.body);
            _indent--;
          }
          if (otherwise != null) {
            _line(first ? '{' : '} else {');
            _indent++;
            stmt(otherwise);
            _indent--;
          }
          _line('}');
          return;
        }
        _line('match ${expr(value)} {');
        _indent++;
        for (final one in cases) {
          _line('${one.values.map(expr).join(' | ')} => {');
          _indent++;
          stmt(one.body);
          _indent--;
          _line('}');
        }
        if (otherwise != null) {
          _line('_ => {');
          _indent++;
          stmt(otherwise);
          _indent--;
          _line('}');
        } else {
          // Dart's `switch` with no `default` does nothing for a value no
          // case names; Rust's `match` on an `i64` has to say so (E0004 on
          // `switch (data.getInt32(..)) { case 0: .. case 1: .. }`). On an
          // enum every variant is named and the arm is only unreachable.
          _line('_ => {}');
        }
        _indent--;
        _line('}');
      case IrWhile(:final condition, :final body, :final label):
        final head = label == null ? '' : "'" + label + ': ';
        // `while (true)` is `loop`: its type is `!`, so a method whose body
        // ends in one and returns from inside it type-checks (`bool <= ()`).
        _line(
          condition is IrLiteral && condition.value == 'true'
              ? '${head}loop {'
              : '${head}while ${expr(condition)} {',
        );
        _indent++;
        stmt(body);
        _indent--;
        _line('}');
      case IrLocalDecl(:final name, :final type, :final init, :final cell):
        // A local a closure writes lives in a cell the closure clones a
        // handle to (see `IrLocalDecl.cell`); every read and write below
        // goes through `_cellLocals`.
        if (cell) {
          final rust = type == null ? null : this.type(type);
          final copy = rust != null && _isCopy(rust);
          _cellLocals = {..._cellLocals, name: copy};
          // A `dynamic` cell starts as Dart's null, the `Null` object.
          final inner = init != null
              ? expr(init)
              : rust == 'std::rc::Rc<dyn Object>'
              ? 'std::rc::Rc::new(Null) as std::rc::Rc<dyn Object>'
              : rust != null && rust.startsWith('Option<')
              ? 'None'
              : 'Default::default()';
          final held = rust == null
              ? ''
              : ': std::rc::Rc<std::cell::${copy ? 'Cell' : 'RefCell'}<$rust>>';
          _line(
            'let ${snake(name)}$held = std::rc::Rc::new(std::cell::${copy ? 'Cell' : 'RefCell'}::new($inner));',
          );
          return;
        }
        final annotation = type == null ? '' : ': ${this.type(type)}';
        // `mut` when the body writes the local, or calls a method on it
        // (`_assignedIn` counts receivers too, since a method in another
        // module may take `&mut self` -- `brk.next_break()` was 30
        // `E0596`s). A local only read stays immutable: the fixture crate
        // denies `unused_mut` to keep that claim checkable.
        // A cascade's temporary is written by construction (`..add(x)`),
        // in a static's initialiser as anywhere else.
        final mutable = _reassigned.contains(name) || name == 'cascaded'
            ? 'mut '
            : '';
        // A declared function type is `Box<dyn Fn(..)>`, and a closure's own
        // type is not that. `_returned` boxes for the same reason one line
        // further out; a `let` is the other half of it.
        // ..and an inferred one too: `final listener = () { .. }` is a
        // function-typed local whether or not the type was written (38
        // closures handed to an `Rc<dyn Fn()>` slot in the gallery).
        final boxed = init is IrClosure && (type == null || type.isFunction);
        // A local with no initialiser is assigned before it is read -- Dart
        // checks that, and so does Rust for a `let x: T;` -- so it needs no
        // value; `Default::default()` asked `Color` for a default it does
        // not have. A nullable one Dart starts at null.
        if (init == null) {
          final nullable =
              type != null && (type.nullable || type.name == 'Option');
          _line(
            nullable
                ? 'let $mutable${snake(name)}$annotation = None;'
                : 'let $mutable${snake(name)}$annotation;',
          );
          return;
        }
        // The same coercion a `return` takes: `let l: Rc<dyn EngineLayer> =
        // _NativeEngineLayer::new_()` needs the `Rc::new` (9 in dart:ui).
        final outer = _returns;
        _returns = type;
        // A local read whole as another's initialiser is a clone, as it
        // is as an argument: `let __t = node;` moved a parameter a closure
        // then read (`_requestFocus`, ws522). `Clone` on a `Copy` type is
        // the copy.
        final value = boxed
            ? 'std::rc::Rc::new(${expr(init)})'
            : init is IrLocal &&
                  !_cellLocals.containsKey(init.name) &&
                  !_closureCaptured.contains(init.name)
            ? '${_returned(init)}.clone()'
            : _returned(init);
        _returns = outer;
        _line('let $mutable${snake(name)}$annotation = $value;');
      case IrAssign(:final name, :final value):
        final cell = _cellLocals[name];
        // A captured `late` field's cell holds the `Option` the struct
        // does (`_lateCellLocals`): the write goes in wrapped, as the
        // reads come out unwrapped.
        final written = _lateCellLocals.contains(name)
            ? 'Some(${expr(value)})'
            : expr(value);
        _line(
          cell == null
              ? '${snake(name)} = $written;'
              : cell
              ? '${snake(name)}.set($written);'
              : '*${snake(name)}.borrow_mut() = $written;',
        );
      case IrAssignField(
        :final target,
        :final name,
        :final value,
        :final owner,
      ):
        final receiver = target == null ? _selfName : expr(target);
        final shared = target == null || target is IrThis
            ? _sharedField(name)
            : owner == null
            ? null
            : _cellFieldOf(owner, name);
        // Assigning a `late` field is what takes it out of `None`, so the
        // value goes in wrapped. This is the only place that happens.
        final own = target == null || target is IrThis
            ? _lateField(name)
            : null;
        final written = own != null || (shared?.isLate ?? false)
            ? 'Some(${expr(value)})'
            : expr(value);
        // A field of the value in a static's cell (`staticFieldWrites`).
        // ..or, when the static holds a *counted* object, through the
        // field's own cell on the handle: the object is not in a cell, its
        // fields are (`GoogleFonts.config.allowRuntimeFetching = false` on
        // a counted `Config`, run510).
        final staticHolder = target is IrStatic
            ? '(**${_lazyName(target.owner, target.name)})'
            : target is IrTopLevel
            ? '(**${screamingSnake(target.name)})'
            : null;
        if (staticHolder != null &&
            shared != null &&
            owner != null &&
            (library[owner]?.counted ?? false)) {
          _line(
            _fieldIsCopy(shared, library[owner])
                ? '$staticHolder.${snake(name)}.set($written);'
                : '*$staticHolder.${snake(name)}.borrow_mut() = $written;',
          );
        } else if (staticHolder != null) {
          _line('$staticHolder.borrow_mut().${snake(name)} = $written;');
        }
        // Inside a trait's body there is no field, only the setter it
        // declares (`this_.set__length(v)` in a mixin's super function).
        else if (_fieldsAreAccessors && (target == null || target is IrThis)) {
          // Through the setter, which takes the plain value and does its
          // own `Some` for a `late` field (106 `f64` <- `Option<f64>`
          // on `_globalDistanceMoved`, ws384).
          final through = _accessorQualifier(name, kind: 'write');
          final widened = expr(value);
          _line(
            through == null
                ? '$receiver.set_${snake(name)}($widened)$_propagate;'
                : '$through::set_${snake(name)}($receiver, $widened)$_propagate;',
          );
        } else if (target != null &&
            target is! IrThis &&
            owner != null &&
            library.isAbstract(owner)) {
          // A write on a trait handle (`cascaded.tolerance = t` on an
          // `Rc<dyn Simulation>`) is the setter the trait declares (113).
          _line('$receiver.set_${snake(name)}($written)$_propagate;');
        } else if (shared != null) {
          // Through the cell, which is why the field can be written from a
          // closure that does not hold `self` at all.
          _line(
            _fieldIsCopy(
                  shared,
                  target == null || target is IrThis
                      ? cls
                      : (owner == null ? null : library[owner]),
                )
                ? '$receiver.${snake(name)}.set($written);'
                : '*$receiver.${snake(name)}.borrow_mut() = $written;',
          );
        } else {
          _line('$receiver.${snake(name)} = $written;');
        }
      case IrAssignTopLevel(:final name, :final value):
        // Through the cell: two derefs for the `LazyLock` and the `Isolate`,
        // then `borrow_mut`. The read side does the same with `borrow`.
        _line('*(**${screamingSnake(name)}).borrow_mut() = ${expr(value)};');
      case IrAssignStatic(:final owner, :final name, :final value):
        _line('*(**${_lazyName(owner, name)}).borrow_mut() = ${expr(value)};');
      case IrSetter(
        :final target,
        :final name,
        :final value,
        :final qualifier,
        :final receiverClass,
      ):
        // A setter is a method and returns `Result` like one. Through the
        // trait when two declare it (`IrSetter.qualifier`).
        if (qualifier != null) {
          final argument = value;
          _line(
            '${_call(target, 'set_${snake(name)}', [argument], qualifier: qualifier, receiverClass: receiverClass, fails: true)};',
          );
        } else {
          // `xs.last = v` writes through the *place*, not the clone a field
          // read takes out of its cell. The call path already decides this
          // (`_mutatesInPlace` and a receiver that really is a collection);
          // a setter never reached it, because a setter is an `IrSetter` and
          // not an `IrCall`. Without this the write compiles and lands on
          // the copy -- `listsetlast` reads `1,2,3/6/6` against Dart's
          // `1,2,9/12/12`.
          final setter = 'set_${snake(name)}';
          final receiverType = target?.rustType;
          final place =
              _mutatesInPlace(setter) &&
                  (receiverType == null ||
                      _isMutableCollection(type(receiverType)))
              ? _mutPlace(target)
              : null;
          _line(
            '${place ?? _receiver(target)}.$setter(${expr(value)})$_propagate;',
          );
        }
      case IrIf(:final condition, :final then, :final otherwise):
        _line('if ${expr(condition)} {');
        _indent++;
        stmt(then, tail: tail);
        _indent--;
        if (otherwise == null) {
          _line('}');
        } else {
          _line('} else {');
          _indent++;
          stmt(otherwise, tail: tail);
          _indent--;
          _line('}');
        }
      case IrExprStmt(:final expr):
        // `onCreate?.call(this)` after TFA proved `onCreate` null is a bare
        // `null` in statement position: nothing to do, and `None;` alone
        // cannot even be typed (E0282).
        if (expr is IrLiteral && expr.value == 'null') break;
        if (expr is IrBlockValue &&
            expr.value is IrLiteral &&
            (expr.value as IrLiteral).value == 'null') {
          for (final s in expr.statements) stmt(s);
          break;
        }
        // A block at a statement's start is a block *statement* to Rust,
        // and what follows it -- `[i].clone().m()` after a cell read --
        // starts another (`r.props.last.initWithValue(..)`, the
        // restoreprop fixture). Parenthesised, it is the expression.
        final text = this.expr(expr);
        _line(text.startsWith('{') ? '($text);' : '$text;');
      case IrAssert(:final condition, :final literalMessage, :final message):
        // `debug_assert!`, not `assert!`: Dart's assert runs in debug builds
        // and is compiled out of release ones, and so is this. Using `assert!`
        // would keep every one of upstream's checks in a release binary, which
        // is a performance decision this compiler has no business making.
        if (message != null) {
          _line('// assert message, not translated: $message');
        }
        final text = literalMessage == null
            ? ''
            : ', "${_escape(literalMessage)}"';
        _line('debug_assert!(${expr(condition)}$text);');
    }
  }
}
