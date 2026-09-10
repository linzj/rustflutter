part of '../backend_rust.dart';

/// Finds, in one method body, whether it writes a field of `this` and which of
/// its own methods it calls.
///
/// Both answers are needed together and both need the *whole* body, statements
/// and expressions alike -- a mutating call can be buried in the middle of an
/// expression, and missing one would emit `&self` for a method that assigns.
class _WalkSelf {
  bool writesFields = false;

  /// Fields read on another object, by the declaring class the front end
  /// named (`IrField.owner`): a lazy `late` one needs its accessor
  /// (`_emitLazyAccessors`).
  final foreignFieldReads = <String, Set<String>>{};

  /// Whether a null-aware's bound value (`it`) is read: a closure made in
  /// such a body must own a clone of it (`_closure`).
  bool readsBound = false;
  int _nullAwareDepth = 0;
  final selfCalls = <String>{};

  /// The classes `super` calls resolve into (`IrSuperCall.base`), each
  /// with its type arguments (`baseArguments`).
  final superBases = <String, List<IrType>>{};

  /// The member names `super.x` reads or calls.
  final superMembers = <String>{};

  /// Whether anything walked can fail -- a `?` on a call, a constructor,
  /// an `await`.
  bool failing = false;

  /// Whether any of `elements` can fail.
  static bool failingIn(Iterable<IrExpr> elements) {
    final walk = _WalkSelf();
    elements.forEach(walk.expression);
    return walk.failing;
  }

  /// `Vec` methods that change what they are called on.
  ///
  /// The `!` ones are the markers the backend spells out; they mutate exactly
  /// as the renamed ones do, and leaving them off here left the receiver
  /// without its `mut`. The names live in `member_names.dart` with the other
  /// four copies of this knowledge, and with what this one is missing.
  static const _mutatingListMethods = mutatingWalkSelfRustNames;

  /// Locals a mutating call is made on -- `xs.insert(..)` needs `let mut xs`,
  /// and a parameter needs `mut xs` in the signature. Rust says this out loud
  /// where Dart says nothing at all.
  final mutatedLocals = <String>{};

  /// Locals that are the receiver of some method call.
  final receiverLocals = <String>{};

  /// Locals a call that mutates the receiver *in place* is made on, by
  /// either spelling.
  ///
  /// Wider than [mutatedLocals] on purpose: that set is
  /// [mutatingWalkSelfRustNames], which the `let mut` decision uses and in
  /// which `remove_where` is one of the gaps `member_names.dart` records
  /// rather than closes. Narrower than [receiverLocals], which counts any
  /// receiver at all -- `saved_recipes.index_of(recipe.clone())` marks
  /// `recipe` there, and a rule that reads a *semantic* answer out of that
  /// over-approximation fires where nothing is mutated (ws982).
  final inPlaceLocals = <String>{};

  static final Set<String> _inPlaceNames = {
    for (final name in mutatingNames)
      if (!noRustMutatorNames.contains(name)) snake(name),
    ...mutatingRustOnlyNames,
  };

  /// Fields set through a setter on another object, by the receiver's
  /// class (`tween.end = ..` on a `Tween<dynamic>`: `{'Tween': {'end'}}`).
  final setterWrites = <String, Set<String>>{};

  /// Whether a write target is `this`, or a chain of field reads from it.
  static bool _rootedAtThis(IrExpr? e) => switch (e) {
    null => true,
    IrThis() => true,
    IrField(:final target) => _rootedAtThis(target),
    _ => false,
  };

  /// The collection a receiver reads a held value out of -- `m[k]!`, `xs[i]`
  /// -- which is the place a mutating call on that value acts on
  /// (`_heldSlot`); null when the receiver is not such a read.
  ///
  /// The *outermost* one, so a nested read answers the place the mutation
  /// reaches: `rawCells[y][x].add(child)` changes `rawCells`
  /// (`RenderTable.assembleSemanticsNode`, ws788).
  static IrExpr? _heldIn(IrExpr? e) => switch (e) {
    IrIndex(:final target) => _heldIn(target) ?? target,
    IrNullCheck(:final operand) => _heldIn(operand),
    IrCall(:final target, name: '!map_get') when target != null =>
      _heldIn(target) ?? target,
    IrCall(:final target, name: 'clone', args: []) when target != null =>
      _heldIn(target),
    _ => null,
  };

  /// The place under a promotion: `x!` and `x.clone()!` both name `x`.
  /// Exactly the two peels `_mutPlace` makes to find what a mutating call
  /// acts on; null when the expression is not a promotion at all.
  static IrExpr? _underPromotion(IrExpr? e) {
    if (e is! IrNullCheck) return null;
    final operand = e.operand;
    if (operand is IrCall &&
        operand.name == 'clone' &&
        operand.args.isEmpty &&
        operand.target != null) {
      return operand.target;
    }
    return operand;
  }

  /// Locals written by an assignment used for its value.
  final assignedLocals = <String>{};

  /// Whether `this` is read anywhere in what was walked.
  bool readsThis = false;

  /// Whether a closure in what was walked keeps a counted handle to `this`.
  bool holdsSelfClosure = false;

  /// Whether `this` is passed as an argument anywhere in what was walked.
  bool passesSelf = false;

  void statement(IrStmt s) {
    switch (s) {
      // `continue <case>` reads and writes nothing of `this`.
      case IrContinueSwitch():
        break;
      case IrAssignField(:final target, :final name, :final owner):
        // Only a write to `this` makes the method mutating. A cascade writes a
        // *local* it just bound, which needs `let mut` and not `&mut self` --
        // and counting it made every method holding a cascade take `&mut self`.
        //
        // A *chain* rooted at `this` counts too: `self.tint.opacity = v` is a
        // write through `self`, and without this it came out `&self` and did
        // not compile.
        if (_rootedAtThis(target)) writesFields = true;
        // A write on another object's field, by the class that declares
        // it (`_fieldsWrittenBy`): on a trait it is that trait's setter,
        // landing on every implementer (`#t.end = ..` on a `Tween`
        // temporary in `_constructTweens`, run616).
        if (!_rootedAtThis(target)) {
          final written = owner ?? target?.rustType?.name;
          if (written != null) {
            setterWrites.putIfAbsent(written, () => {}).add(name);
          }
        }
        // A write through a local -- `entry.x = v` on a value the local owns
        // -- is what makes that local `let mut`. The cascade binding used to
        // be told separately; this covers it and the plain local alike.
        if (target is IrLocal) mutatedLocals.add(target.name);
        expression(s.value);
      case IrAssignTopLevel(:final value):
        // A library's own variable, not this object's: it goes through a cell
        // of its own, so writing one says nothing about `self`.
        expression(value);
      case IrAssignStatic(:final value):
        // A library's own variable, not this object's: it goes through a cell
        // of its own, so writing one says nothing about `self`.
        expression(value);
      case IrAssign(:final name):
        // Recorded here as well as in `_assignedIn`'s own walk, because this
        // one descends into closures and that one does not: `m.forEach((k, v)
        // { sum = sum + v; })` writes an outer local from inside a closure,
        // and nothing declared it `mut`.
        assignedLocals.add(name);
        expression(s.value);
      case IrSetter(
        :final target,
        :final name,
        :final value,
        :final receiverClass,
      ):
        // A setter call on `this` spreads `&mut` exactly as a method call does.
        if (target == null || target is IrThis) selfCalls.add('set_$name');
        // ..and one on another object is a write through that object's
        // class (`_fieldsWrittenBy`): the setter lands on every
        // implementer of the trait that declares the field.
        final written = receiverClass ?? target?.rustType?.name;
        if (target != null && target is! IrThis && written != null) {
          setterWrites.putIfAbsent(written, () => {}).add(name);
        }
        if (target != null) expression(target);
        expression(value);
      case IrBlock(:final statements):
        statements.forEach(statement);
      case IrIf(:final condition, :final then, :final otherwise):
        expression(condition);
        statement(then);
        if (otherwise != null) statement(otherwise);
      case IrReturn(:final value):
        // `return this` from a counted class hands out the handle.
        if (value is IrThis) passesSelf = true;
        if (value != null) expression(value);
      case IrLocalDecl(:final init):
        if (init != null) expression(init);
      case IrExprStmt(:final expr):
        expression(expr);
      case IrAssert(:final condition):
        expression(condition);
      case IrThrow(:final value):
        expression(value);
      case IrTryCatch(:final body, :final handler):
        // Calls in the body are caught, so they do not make this method fail --
        // that is what `catch` means, and it is the only thing that stops the
        // propagation. Without this, a method that catches still had `Result`
        // in its signature, which compiles and says the opposite of the truth.
        // Calls in the *handler* are not caught and still count.
        _caught++;
        statement(body);
        _caught--;
        statement(handler);
      case IrTryFinally(:final body, :final finalizer):
        // No `_caught` here, and that is the difference between the two nodes:
        // a finalizer runs on the way past a failure, it does not stop one. A
        // failing call in this body still makes the method fail.
        statement(body);
        statement(finalizer);
      case IrWhile(:final condition, :final body):
        expression(condition);
        statement(body);
      case IrForIn(:final iterable, :final body):
        expression(iterable);
        statement(body);
      case IrLocalFunction(:final closure):
        expression(closure);
      case IrIndexSet(:final target, :final index, :final value):
        // Writing through an index is writing through the thing indexed.
        if (_rootedAtThis(target)) writesFields = true;
        if (target is IrLocal) mutatedLocals.add(target.name);
        expression(target);
        expression(index);
        expression(value);
      case IrLabeled(:final body):
        statement(body);
      case IrSwitch(:final value, :final cases, :final otherwise):
        expression(value);
        for (final one in cases) {
          one.values.forEach(expression);
          statement(one.body);
        }
        if (otherwise != null) statement(otherwise);
      case IrBreak():
      case IrContinue():
    }
  }

  /// How many `try` bodies deep the walk is. A call inside one is caught.
  int _caught = 0;

  void expression(IrExpr e) {
    switch (e) {
      case IrCall(:final target, :final name, :final args, :final fails):
        if (fails) failing = true;
        // Not guarded by `_caught`: that counts whether a *failure*
        // escapes, and both readers of `selfCalls` ask a different
        // question -- does this method call that one, so that the handle
        // (`_computeHandles`) or the `&mut self` (`_mutating`) has to
        // spread to it. A `try` stops a throw, not a call.
        // `EditableTextState._pasteTextWithReporting` is `try { await
        // pasteText(cause); } catch ..`, and `pasteText` takes the handle:
        // the call went unrecorded, the caller kept `&self`, and
        // `self.paste_text(..)` had no such method.
        if (target == null || target is IrThis) {
          selfCalls.add(name);
        }
        // `this` handed to a call keeps the object, as a closure would:
        // `paragraph._paint(this, ..)` from a counted class needs the handle.
        // So does `this` shared into an `Object` slot (`!as_object`/`!rc`).
        if (args.any((a) => a is IrThis)) passesSelf = true;
        if (target is IrThis && (name == '!as_object' || name == '!rc')) {
          passesSelf = true;
        }
        // An *implicit* `this` reads it just as surely as a written one.
        // Dart lets a member be named without `this`, and a field initialiser
        // that says `transformPosition(transform, position)` is reading two
        // of them -- 110 `self` values in a constructor that has none, all of
        // them Flutter's `late final x = <something about this>`.
        if (target == null) readsThis = true;
        // `self.marks.push(x)` mutates a field, so the method takes
        // `&mut self` -- the same rule as writing the field outright, which is
        // what a `Vec` method that changes it amounts to.
        // Any local a method is called on may be changed by it: the callee's
        // receiver is unknown here, and `rotation.setFromRotation(r)` on an
        // immutable parameter was E0596. An unneeded `mut` is a warning.
        // ..and a receiver reached through a promotion is still that
        // local. `resolvedPadding!.add(x)` is emitted as the *place* it
        // names -- `resolved_padding.as_mut().unwrap()`, which `_mutPlace`
        // reaches by peeling the `!` and the clone a read is -- and nothing
        // here peeled them, so the binding was not written `let mut`
        // (`ButtonStyleButton.build`, ws893). It only shows where the local
        // is read nowhere else: one plain receiver anywhere else marks it
        // and hides this, which is how the fixture passed on its first try.
        final receiver = _underPromotion(target) ?? target;
        if (receiver is IrLocal) receiverLocals.add(receiver.name);
        if (_inPlaceNames.contains(name) ||
            _inPlaceNames.contains(snake(name))) {
          final on = _heldIn(target) ?? target;
          if (on is IrLocal) inPlaceLocals.add(on.name);
        }
        if (_mutatingListMethods.contains(name)) {
          // A call on a value read out of one of this object's collections
          // acts on the collection (`_heldSlot`): `m[k]!.add(v)` writes the
          // field the map is, and `xs[i].push(v)` needs `let mut xs`.
          final on = _heldIn(target) ?? target;
          if (_rootedAtThis(on)) writesFields = true;
          if (on is IrLocal) mutatedLocals.add(on.name);
        }
        if (target != null) expression(target);
        args.forEach(expression);
      case IrField(:final target, :final name, :final owner):
        if (target == null) readsThis = true;
        if (target != null && target is! IrThis && owner != null) {
          foreignFieldReads.putIfAbsent(owner, () => {}).add(name);
        }
        if (target != null) expression(target);
      case IrBinary(:final left, :final right):
        expression(left);
        expression(right);
      case IrUnary(:final operand):
        expression(operand);
      case IrNullCheck(:final operand):
        expression(operand);
      case IrDynamicDispatch(:final receiver, :final arms):
        expression(receiver);
        for (final (_, b) in arms) {
          expression(b);
        }
      case IrDowncast(:final target):
        expression(target);
      case IrCastTo(:final target):
        expression(target);
      case IrSuperDispatch(:final receiver, :final args):
        expression(receiver);
        args.forEach(expression);
      case IrNullableOf(:final value):
        expression(value);
      case IrSome(:final value):
        expression(value);
      case IrCast(:final value):
        expression(value);
      case IrIsNull(:final operand):
        expression(operand);
      case IrIfNull(:final left, :final right):
        expression(left);
        expression(right);
      case IrNullAware(:final receiver, :final body):
        expression(receiver);
        // The body binds its own `it`: a read of the bound in there is
        // not a read of an enclosing null-aware's.
        _nullAwareDepth++;
        expression(body);
        _nullAwareDepth--;
      case IrConditional(:final condition, :final then, :final otherwise):
        expression(condition);
        expression(then);
        expression(otherwise);
      case IrStaticCall(:final args, :final fails):
        if (fails) failing = true;
        // `FlutterView(id, this, ..)` from a counted class hands out the handle.
        if (args.any((a) => a is IrThis)) passesSelf = true;
        args.forEach(expression);
      case IrNew(:final args):
        // A translated class's constructor returns `Result`; the walker
        // does not know which classes are translated, and a prelude one
        // built in steps is the same value.
        failing = true;
        if (args.any((a) => a is IrThis)) passesSelf = true;
        args.forEach(expression);
      case IrSuperCall(
        :final base,
        :final name,
        :final args,
        :final baseArguments,
      ):
        superBases.putIfAbsent(base, () => baseArguments);
        superMembers.add(name);
        args.forEach(expression);
      case IrIs(:final expr):
        expression(expr);
      case IrClosure(:final body, :final holdsSelf):
        if (holdsSelf) holdsSelfClosure = true;
        statement(body);
      case IrCallValue(:final target, :final args):
        // `this` handed to a closure call keeps the object too.
        if (args.any((a) => a is IrThis)) passesSelf = true;
        expression(target);
        args.forEach(expression);
      case IrBlockValue(:final statements, :final value):
        statements.forEach(statement);
        expression(value);
      case IrConstInstance(:final fields):
        fields.values.forEach(expression);
      case IrUpcast(:final value):
        expression(value);
      case IrMapElements(:final collection, :final body):
        expression(collection);
        expression(body);
      case IrAwait(:final operand):
        failing = true;
        expression(operand);
      case IrMutRef(:final place):
        // Written through: the local is `mut`, a field of `this` makes
        // the method mutating.
        if (place is IrLocal) {
          mutatedLocals.add(place.name);
          assignedLocals.add(place.name);
        }
        if (_rootedAtThis(place)) writesFields = true;
        expression(place);
      case IrIdentical(:final left, :final right):
        expression(left);
        expression(right);
      case IrThrowValue(:final value):
        expression(value);
      case IrInterpolation(:final parts):
        parts.forEach(expression);
      case IrIndex(:final target, :final index):
        expression(target);
        expression(index);
      case IrListLiteral(:final elements):
        elements.forEach(expression);
      case IrRecord(:final fields):
        fields.forEach(expression);
      case IrRecordField(:final record):
        expression(record);
      case IrMapLiteral(:final entries):
        for (final entry in entries) {
          expression(entry.$1);
          expression(entry.$2);
        }
      case IrIterChain(:final source, :final steps):
        expression(source);
        for (final step in steps) {
          expression(step.$2);
        }
      case IrFunctionRef():
      case IrAssignValue():
        if (e is IrAssignValue) {
          assignedLocals.add(e.name);
          expression(e.value);
        }
      case IrSetValue(:final target, :final value, :final name):
        // Same rule as the statement form: only a write to `this` makes the
        // method mutating.
        if (target == null || target is IrThis) writesFields = true;
        final written = target?.rustType?.name;
        if (target != null && target is! IrThis && written != null) {
          setterWrites.putIfAbsent(written, () => {}).add(name);
        }
        if (target != null) expression(target);
        expression(value);
      case IrThis():
        readsThis = true;
      // Its own case: the empty cases above it would fall through into a
      // body placed after them (every closure cloned `it`, ws486).
      case IrBound():
        if (_nullAwareDepth == 0) readsBound = true;
      case IrLiteral():
      case IrLocal():
      case IrStatic():
      case IrTopLevel():
    }
  }
}

/// The backend's view of the classes a type names, for `coerceInto`: the
/// `IrLibrary` knows which are traits, counted, enums, and how they sit in
/// the hierarchy.
class _BackendWorld implements TypeWorld {
  _BackendWorld(this.backend);

  final RustBackend backend;

  IrLibrary get library => backend.library;

  @override
  bool isTrait(String name) =>
      const {
        'Object',
        'dynamic',
        'Comparable',
        'DartIterator',
      }.contains(name) ||
      library.isAbstract(name);

  @override
  bool isCounted(String name) => library[name]?.counted ?? false;

  @override
  bool isEnum(String name) => library[name]?.isEnum ?? false;

  @override
  bool isStruct(String name) {
    final c = library[name];
    return c != null && !c.isAbstract && !c.isEnum;
  }

  @override
  bool isBelow(String sub, String sup) {
    final c = library[sub];
    return c != null && backend._isSubtypeOf(c, sup, {});
  }

  @override
  bool isGenericValueStruct(String name) {
    final c = library[name];
    return c != null && !c.counted && c.typeParameters.isNotEmpty;
  }

  @override
  bool isTypeParameter(String name) => backend._isTypeParam(name);
}
