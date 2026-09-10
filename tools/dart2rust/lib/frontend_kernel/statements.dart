part of '../frontend_kernel.dart';

augment class KernelFrontend {
  // -- Statements -------------------------------------------------------------

  IrStmt statement(Statement node) {
    if (node is YieldStatement) {
      final element = _syncStarElement;
      if (element == null) {
        throw Unsupported('yield outside a sync* body', _sample(node));
      }
      final listed = IrType('List', arguments: [_type(element)]);
      final out = IrLocal(_syncStarOut)..rustType = listed;
      if (node.isYieldStar) {
        return IrExprStmt(
          IrCall(out, 'extend', [coerce(expression(node.expression), listed)]),
        );
      }
      return IrExprStmt(
        IrCall(out, 'push', [
          _widened(node.expression, element, expression(node.expression)),
        ]),
      );
    }
    if (node is ReturnStatement) {
      final value = node.expression;
      // A bare `return` in a `sync*` body hands the collected list back.
      if (value == null && _syncStarElement != null) {
        return IrReturn(
          _yielded(IrType('List', arguments: [_type(_syncStarElement!)])),
        );
      }
      // `=> x = v` in a setter or a void closure: the CFE puts the assignment
      // in the `return`, and a void function has no value to carry out. The
      // assignment is the statement; the return is bare. Only when the value
      // is a variable -- reading it twice costs nothing and moves nothing.
      if (_voidReturn &&
          value is InstanceSet &&
          value.receiver is ThisExpression &&
          value.value is VariableGet) {
        return IrBlock([_instanceSet(value), const IrReturn(null)]);
      }
      // `return completer.future;` in an `async` body: Dart awaits the
      // future it returns, and an `async fn` returning `T` has to as well.
      // Before the `void` rule: `return c ? flush() : _readFile();` in an
      // `async` `Future<void>` ran the futures detached and returned
      // (`GetStorage.init`, an uncaught `FormatException` from the read
      // that then raced the write, run529).
      final valueType = value == null ? null : _staticType(value);
      final returnsFuture =
          _asyncBody &&
          valueType is InterfaceType &&
          valueType.classNode.name == 'Future';
      if (returnsFuture && _voidReturn) {
        return IrBlock([
          IrExprStmt(IrAwait(expression(value!))),
          const IrReturn(null),
        ]);
      }
      // Any other `return e;` in a `void` body -- `(x) => day = x` handed
      // to a `void Function(int)` -- runs `e` and returns nothing.
      // ..as the *statement* it would have been: `=> _map[v] = ..` on a
      // field of `this` is the in-place `insert`, where the value form
      // wrote into a clone of the map (the listgen fixture).
      if (_voidReturn && value != null) {
        return IrBlock([
          statement(ExpressionStatement(value)),
          const IrReturn(null),
        ]);
      }
      if (value == null) return const IrReturn(null);
      if (returnsFuture) {
        return IrReturn(IrAwait(expression(value)));
      }
      if (Platform.environment['DART2RUST_TRACE_RETURN'] ==
          (_member?.name.text ?? '')) {
        final lowered = expression(value!);
        stderr.writeln(
          'TRACE_RETURN ${_member?.name.text} have=${lowered.rustType} '
          'slot=$_returnsType edge=$_edgeReturn '
          'widened=${_widened(value, _returnsType, lowered).rustType}',
        );
      }
      return IrReturn(
        _acrossEdge(
          _widened(value, _returnsType, expression(value)),
          _edgeReturn,
          toOption: false,
        ),
      );
    }
    if (node is Block) {
      return IrBlock([for (final s in node.statements) statement(s)]);
    }
    if (node is IfStatement) {
      return IrIf(
        _condition(node.condition),
        statement(node.then),
        node.otherwise == null ? null : statement(node.otherwise!),
      );
    }
    if (node is VariableStatement) {
      return _declare(node.declaration.variable, node);
    }
    if (node is ExpressionStatement && node.expression is Rethrow) {
      // `rethrow`: the handler's own error, thrown again. `Result` has no
      // notion of "the current exception", so the name the handler bound is
      // what goes back out. `loadFontIfNecessary` -- and through it every
      // `google_fonts_text_style` call, 1709 of them -- stopped here.
      final caught = _caught;
      if (caught == null) {
        throw Unsupported('rethrow outside a catch', _sample(node));
      }
      return IrThrow(IrLocal(caught)..rustType = _caughtType);
    }
    if (node is ExpressionStatement && node.expression is Throw) {
      // Before the general `ExpressionStatement` case below, not after: the
      // general one lowers the expression, and a `throw` has no value to lower.
      // Placed after, this check never ran and every throwing method was
      // refused -- which the fixture comparison found at once, because the
      // analyzer front end had it right.
      final thrown = node.expression as Throw;
      if (_tfaUnreachable(thrown)) return IrExprStmt(_unreachable);
      return IrThrow(_thrownValue(thrown.expression));
    }
    if (node is ExpressionStatement) {
      // An assignment is a statement here, not an expression. Dart's `x = 1`
      // has the value 1 and Rust's has the value `()`, so one used for its
      // value cannot be translated this way -- and is refused below rather
      // than silently losing the value.
      final value = node.expression;
      // A narrow typed list's store carries a cast; the expression form
      // below knows how, so a statement of one goes through it.
      if (value is InstanceInvocation &&
          value.name.text == '[]=' &&
          _narrowElement(_staticType(value.receiver)) != null) {
        return IrExprStmt(expression(value));
      }
      if (value is InstanceInvocation &&
          value.name.text == '[]=' &&
          _isMapClass(value.interfaceTarget.enclosingClass?.name) &&
          value.arguments.positional.length == 2) {
        // The key and the value into the map's own types, as the
        // expression form's are: the CFE spells a map literal with a `for`
        // in it as `#t[k] = v` statements, and `SingleActivator(..)` went
        // into a `Map<ShortcutActivator, Intent>` unshared (121 "arguments
        // incorrect" on `DefaultTextEditingShortcuts`).
        return IrExprStmt(
          IrCall(
            expression(value.receiver),
            'insert',
            _mapEntry(
              value,
              _arguments(
                value.arguments,
                value.interfaceTarget.function,
                true,
                value.functionType,
              ),
            ),
          ),
        );
      }
      if (value is InstanceInvocation &&
          value.name.text == '[]=' &&
          value.interfaceTarget.enclosingClass?.name == 'List' &&
          value.arguments.positional.length == 2) {
        // The value widens into the element type: `_objects![i] = shader`
        // on a `List<Object?>` is `Some(Rc::new(shader))`.
        final listType = _staticType(value.receiver);
        final element =
            listType is InterfaceType && listType.typeArguments.isNotEmpty
            ? listType.typeArguments.first
            : null;
        final stored = value.arguments.positional[1];
        final list = expression(value.receiver);
        final lowered = _widened(stored, element, expression(stored));
        // ..and crosses into the element as the *list* spells it: inside a
        // generic class a `List<E?>` field holds `<E as DartNullable>::Or`,
        // and a body's `Option<E>` is not that (`_queue[index] = element`
        // in `HeapPriorityQueue._bubbleDown`, 3 at ws751).
        final slot = list.rustType?.arguments.length == 1
            ? list.rustType!.arguments.single
            : null;
        return IrIndexSet(
          list,
          expression(value.arguments.positional[0]),
          slot == null ? lowered : coerce(lowered, slot),
        );
      }
      // A top-level variable's assignment. `StaticSet` on a `Field` with no
      // enclosing class is exactly that, and it was reaching the general
      // refusal below.
      if (value is StaticSet) {
        final target = value.target;
        // Into the static's type: `_decomposeV = Vector3.zero()` on a
        // `static Vector3? _decomposeV` is `Some(..)`.
        if (target is Field && target.enclosingClass == null) {
          return IrAssignTopLevel(
            target.name.text,
            _widened(value.value, target.type, expression(value.value)),
          );
        }
        if (target is Field) {
          return IrAssignStatic(
            target.enclosingClass!.name,
            target.name.text,
            _widened(value.value, target.type, expression(value.value)),
          );
        }
        // `defaultLocale = systemLocale` on a top-level setter: a call of
        // the function `_lowerTopLevel` made of it.
        if (target is Procedure &&
            target.kind == ProcedureKind.Setter &&
            target.enclosingClass == null) {
          return IrExprStmt(
            IrStaticCall(null, _topLevelSetterName(target.name.text), [
              _widened(
                value.value,
                target.function.positionalParameters.single.type,
                expression(value.value),
              ),
            ]),
          );
        }
      }
      // A write to a field the enclosing closure captured: the cell is what
      // makes it writable from there, and the local is the handle.
      if (value is InstanceSet &&
          value.receiver is ThisExpression &&
          _captured.contains(value.name.text)) {
        return IrAssign(
          value.name.text,
          _widened(
            value.value,
            value.interfaceTarget.setterType,
            expression(value.value),
          ),
        );
      }
      if (value is InstanceSet) return _instanceSet(value);
      if (value is VariableSet) {
        // A temporary can be assigned to now that it has a name -- the same
        // reason its declaration stopped being refused. It has to be a name
        // this lowering already gave out, though: assigning to a temporary
        // that was never declared here would name a local nobody wrote.
        final written = value.variable.cosmeticName;
        final known = _temporaries[value.variable];
        if (known == null && (written == null || written.startsWith('#'))) {
          throw Unsupported(
            'assignment to a synthetic variable',
            _sample(value),
          );
        }
        // `targetWidth = width` into an `int?` local is `Some(width)`;
        // `howMany = truncated` into a declared `num` local casts the `int`.
        return IrAssign(
          known ?? written!,
          _intoDeclaredNum(
            value.value,
            _localType(value.variable),
            _widened(
              value.value,
              _localType(value.variable),
              expression(value.value),
              slotIr: _localIrType(value.variable),
            ),
          ),
        );
      }
      // A conditional whose value is discarded is an `if`: its arms need
      // no common type then. The CFE spells `tween.end ??= tween.begin`
      // as `let #t = tween in #t.end == null ? #t.end = .. : null`, whose
      // arms are the store's `Option<Rc<dyn Object>>` and the `Null`
      // object (`_constructTweens`, run612).
      final asIf = _conditionalStatement(value);
      if (asIf != null) return asIf;
      return IrExprStmt(expression(value));
    }
    if (node is AssertStatement) {
      return _assert(node.condition, node.message);
    }
    if (node is LabeledStatement) {
      final body = node.body;
      if (body is SwitchStatement) {
        // The CFE wraps a switch in a label so that `break` has something to
        // point at. In Rust a match arm simply ends, so that `break` is
        // nothing -- but only the one at the *end* of a case (through the
        // blocks a nested switch's case leaves it in). One in the middle
        // leaves the switch early, which a match arm cannot do on its own:
        // then the match sits in a labelled block and the break names it
        // (the generated `lookupGalleryLocalizations`, run581).
        _switchBreaks.add(node);
        final early = _SwitchBreakFinder.earlyBreaks(node, body);
        if (early.isEmpty) return statement(body);
        _labeledSwitches[node] = _labelFor(node);
        return IrLabeled(_labelFor(node), statement(body));
      }
      if (body is WhileStatement ||
          body is ForStatement ||
          body is DoStatement) {
        // A label wrapped around a loop is how the CFE spells a plain `break`.
        // Restored rather than transliterated: the analyzer front end sees the
        // `break` the programmer wrote, and a labelled block here would be the
        // same meaning in different words -- which is exactly what the two
        // front ends compare.
        //
        // Unless the loop's own body ends up labelled, for the `continue`
        // reason below. Rust will not let an unlabelled `break` cross a
        // labelled block, so then the loop is labelled and the break says so.
        final labelled =
            body is ForStatement &&
            body.updates.isNotEmpty &&
            body.body is LabeledStatement;
        _breakTargets[node] = labelled ? _labelFor(node) : null;
        _loopLabel = labelled ? _labelFor(node) : null;
        return statement(body);
      }
      // A label around anything else really is a labelled block, and Rust has
      // one: `break 'l` leaves it.
      return IrLabeled(_labelFor(node), statement(body));
    }
    if (node is BreakStatement) {
      final target = node.target;
      if (_switchBreaks.contains(target)) {
        if (_droppableBreaks.contains(node)) return const IrBlock([]);
        final label = _labeledSwitches[target];
        if (label == null) {
          throw Unsupported(
            'break out of a switch from inside a case',
            _sample(node),
          );
        }
        return IrBreak(label);
      }
      if (_continueTargets.contains(target)) return const IrContinue();
      if (_breakTargets.containsKey(target))
        return IrBreak(_breakTargets[target]);
      return IrBreak(_labelFor(target));
    }
    if (node is FunctionDeclaration) {
      // A named function written inside a body. Rust has no nested `fn` that
      // can see the enclosing locals, so it becomes a closure bound to a
      // local -- which is what Dart's is.
      // A temporary the CFE invented, as `_declare` treats one: `late
      // final x = ..` inside a body is a `#x#initializer()` local function
      // beside the cell and its flag, and `#` is not a character the
      // backend can carry. It gets the same `__tN` a temporary gets, by
      // identity, and both `LocalFunctionInvocation` and `VariableGet`
      // find it again the same way (7 refusals at ws811).
      final written = node.variable.cosmeticName;
      final name = (written == null || written.startsWith('#'))
          ? _nameFor(node.variable)
          : written;
      // `T effectiveValue<T>(..)` inside `ButtonStyleButton.build`: a local
      // function with type parameters of its own. A Rust closure cannot be
      // generic, and a nested `fn` cannot see the enclosing locals this one
      // reads, so the declaration erases the parameters to their bounds
      // (`_type`, ws832) and `LocalFunctionInvocation` above speaks those
      // erased terms -- which is the half that was missing until ws879.
      // A binding that is only *called* never outlives the body it is
      // written in, so its closure may borrow rather than own -- which is
      // how it reaches `this` without copying anything out of it. The
      // member's whole body is what decides: a read anywhere in it (passed
      // on, stored, torn off) makes it the `Rc<dyn Fn>` a function value
      // is (`popOrInvalidate` inside `_popPolicyDataIfNeeded`, 5 refusals
      // and 3 stubs at ws836).
      final escapes = _ValueRead(node.variable);
      final owner = _member;
      final ownerBody = owner is Procedure
          ? owner.function.body
          : owner is Constructor
          ? owner.function.body
          : null;
      ownerBody?.accept(escapes);
      // ..and a call from inside a closure written beside it is a use that
      // outlives the `let` a borrowing binding is (ws879).
      final nested = _CalledInNestedFunction(node.variable);
      ownerBody?.accept(nested);
      final lends = !escapes.found && !nested.found;
      final wasLending = _lendingLocal;
      if (lends) _lendingLocal = true;
      _erasedLocalParams.addAll(node.function.typeParameters);
      final IrClosure closure;
      try {
        closure = _closure(node.function, node) as IrClosure;
      } finally {
        _lendingLocal = wasLending;
      }
      // Recursive when the body names its own binding: a call
      // (`LocalFunctionInvocation`) or a read of it.
      final self = _SelfReference(node.variable);
      node.function.body?.accept(self);
      if (!self.found && !lends) _boxedFunctionLocals.add(name);
      return IrLocalFunction(
        name,
        closure,
        recursive: self.found,
        lends: lends && !self.found,
      );
    }
    if (node is SwitchStatement) {
      // The arms are numbered *before* the bodies are lowered, because a
      // body may say `continue <case>` and has to name the arm it means
      // (`IrContinueSwitch`). The numbering is the emission's: the
      // non-default cases in order, and `cases.length` for whatever ends
      // up as `otherwise` -- the `default`, or the last case a language-
      // exhaustive switch has instead of one (see below).
      final nonDefault = [
        for (final c in node.cases)
          if (!c.isDefault) c,
      ];
      final defaultCase = node.cases.where((c) => c.isDefault).firstOrNull;
      final pullsLast =
          defaultCase == null &&
          node.isExplicitlyExhaustive &&
          nonDefault.isNotEmpty;
      final armCount = pullsLast ? nonDefault.length - 1 : nonDefault.length;
      final savedArms = _switchArms;
      final savedLabel = _switchLabel;
      final savedThreaded = _switchThreaded;
      _switchArms = {
        for (var i = 0; i < armCount; i++) nonDefault[i]: i,
        if (defaultCase != null) defaultCase: armCount,
        if (pullsLast) nonDefault.last: armCount,
      };
      _switchLabel = '__sw${_nextSwitch++}';
      _switchThreaded = false;
      try {
        return _switchBody(node, defaultCase);
      } finally {
        _switchArms = savedArms;
        _switchLabel = savedLabel;
        _switchThreaded = savedThreaded;
      }
    }
    if (node is ContinueSwitchStatement) {
      final arm = _switchArms[node.target];
      final label = _switchLabel;
      if (arm == null || label == null) {
        throw Unsupported(
          'continue into a switch that is not the enclosing one',
          _sample(node),
        );
      }
      _switchThreaded = true;
      return IrContinueSwitch(label, arm);
    }
    if (node is WhileStatement) {
      final restored = _forInWhile(node);
      if (restored != null) return restored;
      // No updates, so a `continue` really is Rust's `continue`.
      return IrWhile(_condition(node.condition), _loopBody(node.body, false));
    }
    if (node is DoStatement) {
      // `do { .. } while (c)`: a `loop` whose body runs first and tests
      // last. The body is labelled as a `for` with updates is, so that a
      // `continue` inside it leaves the body block and still reaches the
      // test -- a bare `continue` would have skipped it. `package:characters`
      // is written with these, and its whole `StringCharacters` was refused.
      return IrWhile(
        IrLiteral('true', const IrType('bool')),
        IrBlock([
          _loopBody(node.body, true),
          IrIf(IrUnary('!', expression(node.condition)), const IrBreak(), null),
        ]),
      );
    }
    if (node is ForStatement) {
      final restored = _forIn(node);
      if (restored != null) return restored;
      // Kernel's `for` is already the three parts kept apart, so the block is
      // just those parts put in the order Rust wants them. `for (x in xs)`
      // arrives here too -- the CFE lowered it to an iterator loop long before
      // this -- which is 405 of the 592 in `package:flutter/`.
      final condition = node.condition;
      final label = _loopLabel;
      _loopLabel = null;
      return IrBlock([
        for (final v in node.variables) _declare(v.variable, node),
        IrWhile(
          // `for (;;)` has no condition and loops forever.
          condition == null
              ? IrLiteral('true', const IrType('bool'))
              : expression(condition),
          IrBlock([
            // A `for` runs its updates after a `continue`; Rust's `continue`
            // skips to the top of the loop, updates and all -- which is an
            // infinite loop, and was one for as long as it took to run the
            // test. So when there are updates the CFE's own shape is kept: the
            // body is a labelled block and the `continue` leaves it, landing
            // on the updates.
            _loopBody(node.body, node.updates.isNotEmpty),
            // Through `statement`, not `expression`: `i = i + 1` is an
            // assignment, which is a statement on both sides of this compiler.
            // Lowered as an expression it was refused, and the fixture said so
            // the first time it ran.
            for (final update in node.updates)
              statement(ExpressionStatement(update)),
          ]),
          label: label,
        ),
      ]);
    }
    if (node is TryCatch) return _tryCatch(node);
    if (node is TryFinally) {
      return IrTryFinally(statement(node.body), statement(node.finalizer));
    }
    if (node is AssertBlock) {
      return IrBlock([for (final s in node.statements) statement(s)]);
    }
    if (node is EmptyStatement) return const IrBlock([]);
    throw Unsupported('statement ${node.runtimeType}', _sample(node));
  }

  /// `try { .. } on A catch (e) { .. } on B catch (e) { .. }`: one catch
  /// whose handler dispatches on the type, which is what Dart's clauses
  /// are. The clauses are tried in order, the first whose guard matches
  /// handles it, and an unguarded one catches everything; with none
  /// matching the error goes back out, which is `rethrow`.
  ///
  /// Written as the nesting rather than as a new IR node, because that is
  /// what it *is*: `on A catch (e) H` after another clause is `if (e is A)
  /// H` inside the one catch (`MethodChannel._handleAsMethodCall` with
  /// three, `IOClient.send` with two; ws826).
  IrStmt _tryCatchMany(TryCatch node) {
    // The one binding the whole handler works with. A clause's own name is
    // bound to it inside that clause's branch, narrowed to its guard.
    final held = '__caught${_nextTemporary++}';
    // ..and one stack trace, bound where any clause asked for one.
    String? stack;
    for (final clause in node.catches) {
      stack ??= clause.stackTrace?.cosmeticName;
    }
    final outerCaught = _caught;
    final outerCaughtType = _caughtType;
    // Nothing matching is the error going back out, which is `rethrow`:
    // that is what the last clause's `else` is, until an unguarded clause
    // replaces it.
    IrStmt chain = IrThrow(IrLocal(held)..rustType = const IrType('Object'));
    // Backwards: each clause's `else` is what the clauses after it do.
    for (final clause in node.catches.reversed) {
      final guard = clause.guard;
      final guarded =
          guard is InterfaceType &&
          guard.classNode.name != 'Object' &&
          guard is! DynamicType;
      final name = clause.exception?.cosmeticName;
      IrType? guardIr;
      if (guarded) {
        try {
          guardIr = _type(guard);
        } on Unsupported {
          guardIr = null;
        }
      }
      _caught = name ?? held;
      _caughtType = guardIr;
      final IrStmt body;
      try {
        body = statement(clause.body);
      } finally {
        _caught = outerCaught;
        _caughtType = outerCaughtType;
      }
      // The clause's own name, narrowed the way a typed catch's binding is
      // (`IrTryCatch.errorType` in the backend): a trait by the cast, a
      // struct by `Any` -- `coerce` leaves a prelude class alone, and
      // `let e: StateError = __caught` did not type.
      final held0 = IrLocal(held)..rustType = const IrType('Object');
      final narrowed = guardIr == null
          ? held0
          : _abstractLike((guard as InterfaceType).classNode)
          ? (IrCastTo(held0, guardIr)..rustType = guardIr)
          : (IrCall(
              IrDowncast(held0, guardIr.name, arguments: const []),
              'clone',
              const [],
            )..rustType = guardIr);
      final bound = <IrStmt>[
        if (name != null && name != held) IrLocalDecl(name, guardIr, narrowed),
        // ..and its own stack trace name, when it is not the one bound.
        if (clause.stackTrace != null &&
            clause.stackTrace!.cosmeticName != null &&
            clause.stackTrace!.cosmeticName != stack)
          IrLocalDecl(
            clause.stackTrace!.cosmeticName!,
            null,
            IrLocal(stack ?? '__stack'),
          ),
        body,
      ];
      final branch = IrBlock(bound);
      if (!guarded || guardIr == null) {
        // An unguarded clause catches everything after it; a guard this
        // compiler cannot spell would silently skip its clause, so it
        // stops rather than guessing.
        if (!guarded) {
          chain = branch;
          continue;
        }
        throw Unsupported(
          '`on ${guard.toString()}` in a multi-clause try',
          _sample(node),
        );
      }
      chain = IrIf(
        IrIs(IrLocal(held)..rustType = const IrType('dynamic'), guardIr)
          ..rustType = const IrType('bool'),
        branch,
        chain,
      );
    }
    return IrTryCatch(statement(node.body), held, chain, stack: stack);
  }

  /// `try { .. } catch (e) { .. }`, when there is one clause.
  IrStmt _tryCatch(TryCatch node) {
    if (node.catches.length != 1) return _tryCatchMany(node);
    final clause = node.catches.single;
    final error = clause.exception?.cosmeticName ?? 'error';
    final stack = clause.stackTrace;
    // A read stack trace is bound to `StackTrace::current()` at the catch
    // (the backend does it): the *catch site's* stack, not the throw's --
    // a `Result` carries none. Recorded as the approximation it is; 38
    // members were refused for reading one, most of them to log it.

    final guard = clause.guard;
    final outerCaught = _caught;
    final outerCaughtType = _caughtType;
    _caught = error;
    // The type the clause narrowed to (`on FlutterError catch (e)`): a
    // `rethrow` throws that value, and the error type it goes back into is
    // `Rc<dyn Object>` -- untyped, the widening rule in `_boxedThrow` had
    // nothing to look at, and `return Err(error)` handed a `FlutterError`
    // where the handle goes (`AssetBundleImageProvider._loadAsync`, the
    // whole image path, run745).
    _caughtType = guard is InterfaceType && guard.classNode.name != 'Object'
        ? (() {
            try {
              return _type(guard);
            } on Unsupported {
              return null;
            }
          })()
        : null;
    final IrStmt handler;
    try {
      handler = statement(clause.body);
    } finally {
      _caught = outerCaught;
      _caughtType = outerCaughtType;
    }
    return IrTryCatch(
      statement(node.body),
      error,
      handler,
      errorType: guard is InterfaceType && guard.classNode.name != 'Object'
          ? guard.classNode.name
          : null,
      stack: stack?.cosmeticName,
    );
  }

  IrAssert _assert(Expression condition, Expression? message) {
    if (message is StringLiteral) {
      return IrAssert(expression(condition), literalMessage: message.value);
    }
    return IrAssert(
      expression(condition),
      message: message == null ? null : _sample(message),
    );
  }

  /// The error the enclosing `catch` bound, for a `rethrow` to name.
  String? _caught;

  /// The type the catch clause narrowed the caught value to, for `rethrow`.
  IrType? _caughtType;

  /// Whether the function whose body is being lowered returns nothing.
  var _voidReturn = false;

  IrStmt _body(FunctionNode function) {
    final body = function.body;
    // A `@Native` member after the AOT FFI transform is no longer
    // `external`: its body is the plumbing -- `_fromAddress(..)` and a call
    // to `___drawRect$Method$FfiNative` -- around what the engine provides.
    // The whole member is that slot; the plumbing is not worth translating.
    // 27 refusals on `_NativeCanvas` alone, and 70 callers of them.
    if (body != null && _callsFfiNative(body)) {
      final member = function.parent;
      final owner = member is Member ? member.enclosingClass?.name ?? '' : '';
      final name = member is Member ? member.name.text : '';
      if (member is Member) {
        final boundary = _nativeBoundary(function, member, '$owner.$name');
        if (boundary != null) return boundary;
      }
      return IrBlock([
        IrExprStmt(
          IrLiteral(
            'todo!("native `$owner.$name` is the engine\'s to provide")',
            const IrType('raw'),
          ),
        ),
      ]);
    }
    if (body == null) {
      // A redirecting factory -- `factory Foo() = Bar;` -- has no body in
      // Kernel: it is a call to its target with its own parameters. 66
      // "no body" refusals, `SemanticsConfiguration` alone 20 of them.
      final procedure = function.parent;
      if (procedure is Procedure && procedure.isRedirectingFactory) {
        final target = function.redirectingFactoryTarget?.target;
        if (target != null) {
          final args = <IrExpr>[
            for (final p in function.positionalParameters)
              IrLocal(_paramName(p)),
            for (final p in function.namedParameters) IrLocal(p.parameterName),
          ];
          final owner = target.enclosingClass?.name;
          final name = target.name.text;
          final call = target is Constructor
              ? IrNew(
                  IrType(owner!),
                  args,
                  constructor: name.isEmpty ? null : name,
                )
              : IrStaticCall(owner, name, args);
          return IrBlock([IrReturn(call)]);
        }
      }
      // An `external` member is the engine's to provide -- `dart:ui`'s
      // `_ImageFilter._constructor`, `_Logger._printString`. The refusal
      // moves to run time as a `todo!` naming it, so that the members and
      // classes around it compile: 9 constructors and every caller of
      // them were errors for a body that was never going to be here.
      final member = function.parent;
      if (member is Member && member.isExternal) {
        final name = '${member.enclosingClass?.name ?? ''}.${member.name.text}';
        // ..through the one boundary the runtime answers (`dart_native`
        // in the prelude): the `@Native` symbol the engine registers it
        // under, the arguments as objects, and whether a value comes
        // back. The generated code sees only the Dart signature; what
        // the symbol does is the native host's (run455: the first panic
        // past the bindings' constructors was `__nativeSetNeedsReport
        // Timings`).
        final boundary = _nativeBoundary(function, member, name);
        if (boundary != null) return boundary;
        if (_nativeSymbol(member) == null &&
            Platform.environment['DART2RUST_TRACE_NATIVE'] != null) {
          stderr.writeln(
            'TRACE_NATIVE $name no symbol: ${member.annotations.map((a) => a is ConstantExpression && a.constant is InstanceConstant ? (a.constant as InstanceConstant).fieldValues.entries.map((e) => '${e.key.asField.name.text}=${e.value.toString().substring(0, e.value.toString().length.clamp(0, 90))}').join(';') : a.runtimeType.toString()).join(' | ')}',
          );
        }
        return IrBlock([
          IrExprStmt(
            IrLiteral(
              'todo!("external `$name` is the engine\'s to provide")',
              const IrType('raw'),
            ),
          ),
        ]);
      }
      // A setter with no body is what `--tree-shake-write-only-fields`
      // leaves of a field that is only ever written (`ImmutableBuffer
      // ._length`): the stores stay and go nowhere. An empty body is that.
      final declaring = function.parent;
      if (declaring is Procedure &&
          declaring.kind == ProcedureKind.Setter &&
          !declaring.isAbstract) {
        return const IrBlock([]);
      }
      throw Unsupported('no body', function.toString());
    }
    return _lowerBody(function, body);
  }

  /// A function body, with `_voidReturn` set for it and restored after --
  /// closures included, since a closure inside a void method may well return
  /// something.
  IrStmt _lowerBody(FunctionNode function, Statement body) {
    final outer = _voidReturn;
    final outerType = _returnsType;
    final outerAsync = _asyncBody;
    // The expected return, when a parameter's function type set one, wins
    // over the closure's own; consumed here so nested bodies do not see it.
    // `void Function(int)` taking `(x) => day = x` returns nothing.
    // ..and for an `async` body the *awaited* one: a slot's `FutureOr<T>
    // Function()` taking `() async { .. return v; }` expects `T` of the
    // body's returns, the future around it being the closure's own
    // (`Future<bool>(() async {..})`, run430).
    final async = function.asyncMarker == AsyncMarker.Async;
    final expected = async ? _awaitedType(_expectedReturn) : _expectedReturn;
    _expectedReturn = null;
    // An `async` body's `return v` is the future's value: the returns are
    // widened into the awaited type (a `{'response': ..}` returned from a
    // `Future<dynamic>` goes behind a handle, run460).
    final own = async
        ? (_awaitedType(function.returnType) ?? function.returnType)
        : function.returnType;
    _voidReturn = (expected ?? own) is VoidType;
    _returnsType = expected ?? own;
    _asyncBody = async;
    // A `sync*` body collects what it yields into the list it returns:
    // `yield x` pushes, `yield* xs` extends, a bare `return` hands the
    // list back, and so does falling off the end. Eager where Dart is
    // lazy, which only an unbounded generator could tell apart
    // (`_OverlayEntryWidgetState._createChildIterable`, run655).
    final outerSyncStar = _syncStarElement;
    final DartType? syncStarElement;
    if (function.asyncMarker == AsyncMarker.SyncStar) {
      final declared = function.returnType;
      syncStarElement =
          declared is InterfaceType && declared.typeArguments.length == 1
          ? declared.typeArguments.single
          : const DynamicType();
    } else {
      syncStarElement = null;
    }
    _syncStarElement = syncStarElement;
    final outerEdge = _edgeReturn;
    // ..the awaited type for an `async` body, whose `return v` is the
    // future's value (`Future<T?> send()` returning `T?`, ws421).
    final declaredReturn = function.returnType;
    _edgeReturn = expected == null && function.parent is Member
        ? (function.asyncMarker == AsyncMarker.Async &&
                  declaredReturn is InterfaceType &&
                  declaredReturn.classNode.name == 'Future' &&
                  declaredReturn.typeArguments.length == 1
              ? declaredReturn.typeArguments.single
              : declaredReturn)
        : null;
    try {
      // A parameter a closure assigns lives in a cell, as a local one
      // does (`_capturedWrites`): rebound over itself before the body.
      // ..through a temporary: the cell's own name is a cell already by
      // the time its initializer prints.
      final rebound = <IrStmt>[];
      for (final p in [
        ...function.positionalParameters,
        ...function.namedParameters,
      ]) {
        if (!_capturedWrites.contains(p)) continue;
        final type = _localIrType(p);
        final held = '__p${_nextTemporary++}';
        rebound.add(
          IrLocalDecl(held, type, IrLocal(_paramName(p))..rustType = type),
        );
        rebound.add(
          IrLocalDecl(
            _paramName(p),
            type,
            IrLocal(held)..rustType = type,
            cell: true,
          ),
        );
      }
      if (syncStarElement != null) {
        final element = _type(syncStarElement);
        final listed = IrType('List', arguments: [element]);
        return IrBlock([
          ...rebound,
          IrLocalDecl(
            _syncStarOut,
            listed,
            IrListLiteral(const [], element)..rustType = listed,
          ),
          statement(body),
          IrReturn(_yielded(listed)),
        ]);
      }
      if (rebound.isEmpty) return statement(body);
      return IrBlock([...rebound, statement(body)]);
    } finally {
      _voidReturn = outer;
      _returnsType = outerType;
      _asyncBody = outerAsync;
      _edgeReturn = outerEdge;
      _syncStarElement = outerSyncStar;
    }
  }

  /// The element type of the `sync*` body being lowered, or null.
  DartType? _syncStarElement;

  /// The list a `sync*` body collects into.
  static const _syncStarOut = '__yielded';

  /// That list, handed back as the body's declared return: a `sync*`
  /// function returns Dart's `Iterable<E>`, which is a `Rc<dyn
  /// DartIterable<E>>` since ws908 (`_createChildIterable`, `Route
  /// .createOverlayEntries`).
  IrExpr _yielded(IrType listed) {
    final out = IrLocal(_syncStarOut)..rustType = listed;
    final declared = _returnsType;
    if (declared == null) return out;
    try {
      return coerce(out, _type(declared));
    } on Unsupported {
      return out;
    }
  }

  /// Whether the body being lowered is an `async` one.
  bool _asyncBody = false;

  /// `Future<T>` or `FutureOr<T>` -> `T`; anything else unchanged.
  static DartType? _awaitedType(DartType? t) {
    if (t is FutureOrType) return t.typeArgument;
    if (t is InterfaceType &&
        t.classNode.name == 'Future' &&
        t.typeArguments.length == 1) {
      return t.typeArguments.single;
    }
    return t;
  }

  /// The AOT compiler's own throw, planted where type flow analysis proved
  /// nothing arrives: `throw "Attempt to execute code removed by Dart AOT
  /// compiler (TFA)"`. It is not an exception the program raises but a
  /// claim that the line is dead, and `unreachable!` is that claim in Rust
  /// -- without making the method a failing one, which put `Result` on 8
  /// getters whose traits say otherwise.
  static bool _tfaUnreachable(Throw node) {
    final thrown = node.expression;
    // "code removed" in a body, "method removed" for a whole constructor
    // (`IconData`'s, whose every use upstream is a constant).
    return thrown is StringLiteral &&
        thrown.value.startsWith('Attempt to execute ') &&
        thrown.value.contains('removed by Dart AOT');
  }

  static final _unreachable = IrLiteral.unreachable;

  /// An `int` value stored into a variable *declared* `num` (an `f64`).
  IrExpr _intoDeclaredNum(Expression value, DartType declared, IrExpr lowered) {
    if (declared is! InterfaceType || declared.classNode.name != 'num')
      return lowered;
    // A literal says so itself: `num _n = 0` at the top level has no
    // context for `getStaticType` and came out as `RefCell<f64>::new(0)`.
    if (value is IntLiteral) return _toF64(lowered);
    final given = _staticType(value);
    if (given is InterfaceType &&
        given.classNode.name == 'int' &&
        given.nullability != Nullability.nullable) {
      return _toF64(lowered);
    }
    // A `num` whose static type is `num` -- TFA folded `1 is double ?
    // pow(2, 52) : 1.0e300.floor()` to its `int` branch and the
    // conditional's type stayed `num` -- is cast too: `f64 as f64` is a
    // no-op Rust accepts, and `i64 as f64` is the cast that was missing.
    if (given is InterfaceType &&
        given.classNode.name == 'num' &&
        given.nullability != Nullability.nullable &&
        value is! DoubleLiteral) {
      return _toF64(lowered);
    }
    return lowered;
  }

  /// The `num` members a `dynamic` receiver is downcast for.
  static const _dynamicNumMethods = {
    'abs',
    'isInfinite',
    'isNaN',
    'isFinite',
    'isNegative',
    'round',
    'floor',
    'ceil',
    'truncate',
    'toDouble',
    'toInt',
    'toStringAsFixed',
    'sign',
  };

  /// Whether an expression reads a variable, field or static *declared*
  /// `num` -- the one place the word can be trusted (see the operators).
  static bool _declaredNum(Expression e) {
    DartType? declared;
    if (e is VariableGet) declared = e.variable.type;
    if (e is InstanceGet) declared = e.interfaceTarget.getterType;
    if (e is StaticGet) declared = e.target.getterType;
    // `n % 10 == 1`: arithmetic on a declared `num` is a `num` still.
    if (e is InstanceInvocation &&
        const {'+', '-', '*', '/', '%', '~/'}.contains(e.name.text)) {
      return _declaredNum(e.receiver);
    }
    return declared is InterfaceType && declared.classNode.name == 'num';
  }

  /// Whether a local of this type is cloned when passed on (see `_widened`).
  static bool _clonedWhenPassed(DartType type) {
    // An extension type is its representation at run time: `_AxisSize` is
    // a `Size`, and passed without the clone it moved out of the local the
    // next argument reads (`RenderFlex._computeSizes`, ws716).
    if (type is ExtensionType) {
      return _clonedWhenPassed(type.extensionTypeErasure);
    }
    if (type is FunctionType || type is DynamicType) return true;
    // Every type parameter is bounded `Clone` in the output.
    if (type is TypeParameterType) return true;
    // An enum is `Copy`, and a `const fn` may not call `clone` (18 E0015s).
    if (type is InterfaceType && type.classNode.isEnum) return false;
    if (type is! InterfaceType) return false;
    const copied = {'int', 'double', 'bool', 'num', 'Null'};
    // A list or map is cloned too. Dart shares it; this output already
    // passes a `Vec` by value into every call, so the aliasing was lost
    // at the first argument and a copy at `left = mid` (11 E0382s in the
    // HCT solver) loses nothing more. Recorded as the approximation it is.
    final name = type.classNode.name;
    return !copied.contains(name);
  }

  /// The number the receiver of the call whose arguments are being lowered
  /// is (`double`/`int`), or null: Dart's `num` is not a type here, so a
  /// `num` parameter of a number's own method is the receiver's own.
  String? _numReceiver;

  /// The declared return type of the function being lowered, for `return`
  /// to widen into when it is nullable and the value is not.
  DartType? _returnsType;

  /// The innermost `switch`'s cases, numbered as the emission numbers its
  /// arms (see the `SwitchStatement` lowering). Empty outside one.
  Map<SwitchCase, int> _switchArms = const {};

  /// The Rust label of the loop that innermost switch's arms run inside,
  /// used by a `continue <case>` to go round again.
  String? _switchLabel;

  /// Whether a body of the innermost switch actually said `continue <case>`:
  /// only then is the loop worth emitting, and the ordinary `match` stands.
  bool _switchThreaded = false;

  int _nextSwitch = 0;

  /// The `switch` itself, once its arms are numbered.
  IrStmt _switchBody(SwitchStatement node, SwitchCase? defaultCase) {
    final cases = <IrCase>[];
    IrStmt? otherwise;
    for (final c in node.cases) {
      final body = _caseBody(c.body);
      if (c.isDefault) {
        otherwise = body;
        continue;
      }
      if (c.expressions.isEmpty) {
        throw Unsupported('empty switch case', _sample(node));
      }
      // A case value widens into the scrutinee's type: `switch (tileMode)`
      // over a `TileMode?` compares an `Option` with `Some(TileMode::Clamp)`.
      final scrutinee = _staticType(node.expression);
      cases.add(
        IrCase([
          for (final e in c.expressions) _widened(e, scrutinee, expression(e)),
        ], body),
      );
    }
    // A switch the language checked as exhaustive -- every `TileMode` and
    // `null` -- has no `default`, and Rust's `if` chain made of it has no
    // `else`: the chain's value is `()`, and a getter returning through it
    // does not type. The last case is what is left when none of the
    // others matched, so it is the `else`.
    if (otherwise == null && node.isExplicitlyExhaustive && cases.isNotEmpty) {
      otherwise = cases.removeLast().body;
    }
    return IrSwitch(
      expression(node.expression),
      cases,
      otherwise,
      threadLabel: _switchThreaded ? _switchLabel : null,
    );
  }
}
