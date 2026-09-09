part of '../frontend_kernel.dart';

/// Whether a body is the FFI transform's plumbing around a `@Native`.
bool _callsFfiNative(Statement body) {
  final finder = _FfiNativeFinder();
  body.accept(finder);
  return finder.found;
}

class _FfiNativeFinder extends RecursiveVisitor {
  bool found = false;

  @override
  void visitStaticInvocation(StaticInvocation node) {
    final name = node.target.name.text;
    if (name.contains(r'$Method$FfiNative') || name == '_fromAddress') {
      found = true;
    }
    super.visitStaticInvocation(node);
  }
}

/// The variables a function body reads and the ones it declares.
/// Finds the variables a member declares in one function and assigns in a
/// nested one.
/// The generic class types a body constructs (`_constructedIn`).
class _Constructed extends RecursiveVisitor {
  final types = <InterfaceType>[];

  @override
  void visitConstructorInvocation(ConstructorInvocation node) {
    if (node.constructedType.typeArguments.isNotEmpty) {
      types.add(node.constructedType);
    }
    super.visitConstructorInvocation(node);
  }

  @override
  void visitStaticInvocation(StaticInvocation node) {
    final owner = node.target.enclosingClass;
    if (node.target.isFactory &&
        owner != null &&
        node.arguments.types.isNotEmpty) {
      types.add(
        InterfaceType(owner, Nullability.nonNullable, node.arguments.types),
      );
    }
    super.visitStaticInvocation(node);
  }
}

class _CapturedWrites extends RecursiveVisitor {
  _CapturedWrites(this.fills);

  /// Whether a callee fills its positional parameter (`_fillsParameter`).
  final bool Function(Procedure, int) fills;
  final _declaredIn = <Variable, FunctionNode>{};
  final _stack = <FunctionNode>[];
  final found = <Variable>{};

  /// Read from inside a closure, and assigned in its own function *after*
  /// that closure was made (in source order): Dart's closure sees the
  /// variable, not its value when the closure was made (`completer`
  /// assigned after the `then` callbacks that complete it were written,
  /// `CachingAssetBundle.loadStructuredData`, run601). One assigned only
  /// before its captures (`Widget child; .. child = ..; builder: (_) =>
  /// child`) stays a plain local: its value is final by the time the
  /// closure exists, and a cell of a `dyn Widget` has no `Default` to
  /// start from (eight stubs, ws602).
  final _captured = <Variable>{};

  static Set<Variable> of(Member member, bool Function(Procedure, int) fills) {
    final v = _CapturedWrites(fills);
    member.accept(v);
    return v.found;
  }

  @override
  void visitVariableGet(VariableGet node) {
    if (_fromOutside(node.variable)) _captured.add(node.variable);
    super.visitVariableGet(node);
  }

  bool _fromOutside(Variable variable) {
    final home = _declaredIn[variable];
    return home != null && _stack.isNotEmpty && home != _stack.last;
  }

  /// A list or set *changed in place* from inside a closure -- added
  /// to, or lent to a callee that fills it -- is shared as an assigned
  /// one is: the closure's copy took the adds (`seen.add(..)` inside
  /// `each(..)`'s callback, the lend2 fixture, ws543).
  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    final receiver = node.receiver;
    if (receiver is VariableGet &&
        mutatingListNames.contains(node.name.text) &&
        node.name.text != 'length' &&
        _fromOutside(receiver.variable)) {
      found.add(receiver.variable);
    }
    super.visitInstanceInvocation(node);
  }

  @override
  void visitStaticInvocation(StaticInvocation node) {
    final positional = node.arguments.positional;
    for (var i = 0; i < positional.length; i++) {
      final arg = positional[i];
      if (arg is VariableGet &&
          _fromOutside(arg.variable) &&
          fills(node.target, i)) {
        found.add(arg.variable);
      }
    }
    super.visitStaticInvocation(node);
  }

  @override
  void visitFunctionNode(FunctionNode node) {
    _stack.add(node);
    // A parameter is declared by its function as a local is: `element`
    // in `attach(owner, [RootElement? element])`, assigned inside
    // `owner.lockState(() { element = createElement(); .. })`, was a
    // plain binding the closure's assignment never reached (run459).
    for (final p in node.positionalParameters) {
      _declaredIn[p] = node;
    }
    for (final p in node.namedParameters) {
      _declaredIn[p] = node;
    }
    super.visitFunctionNode(node);
    _stack.removeLast();
  }

  @override
  void visitVariableDeclaration(VariableDeclaration node) {
    if (_stack.isNotEmpty) _declaredIn[node.variable] = _stack.last;
    super.visitVariableDeclaration(node);
  }

  @override
  void visitVariableSet(VariableSet node) {
    final home = _declaredIn[node.variable];
    if (home != null && _stack.isNotEmpty && home != _stack.last) {
      found.add(node.variable);
    } else if (home != null && _captured.contains(node.variable)) {
      found.add(node.variable);
    }
    super.visitVariableSet(node);
  }
}

/// See `_tryWrites`: a variable declared outside a `try` and assigned
/// inside its body (or a catch or finally block).
class _TryWrites extends RecursiveVisitor {
  final _declaredIn = <Variable, TreeNode?>{};
  final _stack = <TreeNode>[];
  final found = <Variable>{};

  static Set<Variable> of(Member member) {
    final v = _TryWrites();
    member.accept(v);
    return v.found;
  }

  TreeNode? get _home => _stack.isEmpty ? null : _stack.last;

  @override
  void visitTryCatch(TryCatch node) {
    _stack.add(node);
    super.visitTryCatch(node);
    _stack.removeLast();
  }

  @override
  void visitTryFinally(TryFinally node) {
    _stack.add(node);
    super.visitTryFinally(node);
    _stack.removeLast();
  }

  @override
  void visitFunctionNode(FunctionNode node) {
    // A closure is a boundary of its own (`_CapturedWrites`): what it
    // assigns is not this `try`'s doing.
    _stack.add(node);
    super.visitFunctionNode(node);
    _stack.removeLast();
  }

  @override
  void visitVariableDeclaration(VariableDeclaration node) {
    _declaredIn[node.variable] = _home;
    super.visitVariableDeclaration(node);
  }

  @override
  void visitVariableSet(VariableSet node) {
    if (_declaredIn.containsKey(node.variable) &&
        _declaredIn[node.variable] != _home &&
        _home is! FunctionNode) {
      found.add(node.variable);
    }
    super.visitVariableSet(node);
  }
}

/// Whether a local function's body names the function itself.
class _SelfReference extends RecursiveVisitor {
  _SelfReference(this.variable);

  final Variable variable;
  bool found = false;

  @override
  void visitLocalFunctionInvocation(LocalFunctionInvocation node) {
    if (node.variable == variable) found = true;
    super.visitLocalFunctionInvocation(node);
  }

  @override
  void visitVariableGet(VariableGet node) {
    if (node.variable == variable) found = true;
    super.visitVariableGet(node);
  }
}

/// Whether a local function's binding is ever read as a *value* -- passed
/// on, stored, torn off -- as opposed to only being called.
///
/// A binding that is only called never outlives the body it is written in,
/// so its closure may borrow rather than own: it can reach `this` without
/// copying anything out of it. One that escapes has to be the `Rc<dyn Fn>`
/// every function value is.
class _ValueRead extends RecursiveVisitor {
  _ValueRead(this.variable);

  final Variable variable;
  bool found = false;

  @override
  void visitVariableGet(VariableGet node) {
    if (node.variable == variable) found = true;
    super.visitVariableGet(node);
  }
}

/// Whether a local function's binding is *called* from inside a closure
/// written in the same body.
///
/// A binding that is only called may borrow its captures (`lends`), and a
/// borrowing closure lives exactly as long as its `let`. A closure written
/// beside it that calls it is handed away as an `Rc<dyn Fn>` and outlives
/// that `let`: "`effective_value` does not live long enough", three of
/// `_ButtonStyleState.build`'s siblings at ws879. Called from inside one,
/// the binding owns, like any other function value.
class _CalledInNestedFunction extends RecursiveVisitor {
  _CalledInNestedFunction(this.variable);

  final Variable variable;
  int _depth = 0;
  bool found = false;

  @override
  void visitFunctionNode(FunctionNode node) {
    _depth++;
    super.visitFunctionNode(node);
    _depth--;
  }

  @override
  void visitLocalFunctionInvocation(LocalFunctionInvocation node) {
    // The walk starts at the member's body, so any `FunctionNode` above
    // the call is a closure (or the local function itself, whose own
    // recursion `_SelfReference` already answers).
    if (node.variable == variable && _depth > 0) found = true;
    super.visitLocalFunctionInvocation(node);
  }
}

class _LocalFinder extends RecursiveVisitor {
  final read = <Variable>[];
  final declared = <Variable>{};

  @override
  void visitVariableGet(VariableGet node) {
    read.add(node.variable);
    super.visitVariableGet(node);
  }

  /// Calling a local function written outside the closure reads its
  /// binding: Kernel spells that call without a `VariableGet`, so it was
  /// invisible here and the binding was neither cloned in nor moved --
  /// the closure simply borrowed it, and an `Rc<dyn Fn>` that borrows a
  /// local is "`effective_value` does not live long enough" (three of
  /// `_ButtonStyleState.build`'s siblings), while the ones that did move
  /// it left the enclosing body calling a moved value
  /// (`get_offset_for_theta` in `_DialPainter.paint`, ws892).
  @override
  void visitLocalFunctionInvocation(LocalFunctionInvocation node) {
    read.add(node.variable);
    super.visitLocalFunctionInvocation(node);
  }

  @override
  void visitVariableSet(VariableSet node) {
    read.add(node.variable);
    super.visitVariableSet(node);
  }

  @override
  void visitVariableDeclaration(VariableDeclaration node) {
    // The statement declares a `DeclaredVariable`; reads name the variable.
    declared.add(node.variable);
    super.visitVariableDeclaration(node);
  }

  @override
  void visitFunctionNode(FunctionNode node) {
    declared.addAll(node.positionalParameters);
    declared.addAll(node.namedParameters);
    super.visitFunctionNode(node);
  }

  @override
  void visitLet(Let node) {
    // A `Let` binds its variable without a declaration statement; counted
    // as an outer local, `__t0` was cloned in from a scope that had none.
    declared.add(node.variable);
    super.visitLet(node);
  }

  // The other binders without a declaration statement: a catch clause's
  // two, a `for-in`'s, a local function's. Counted as outer locals, a
  // closure around `catch (error, stack)` cloned `error` in from a scope
  // that had none (`lockEvents`, `registerServiceExtension`, ws452).
  @override
  void visitCatch(Catch node) {
    final exception = node.exception, stackTrace = node.stackTrace;
    if (exception != null) declared.add(exception);
    if (stackTrace != null) declared.add(stackTrace);
    super.visitCatch(node);
  }

  @override
  void visitForInStatement(ForInStatement node) {
    declared.add(node.variable);
    super.visitForInStatement(node);
  }

  @override
  void visitFunctionDeclaration(FunctionDeclaration node) {
    declared.add(node.variable);
    super.visitFunctionDeclaration(node);
  }
}

/// Finds `this.x = v` (implicit `this` included).
class _ThisWriteFinder extends RecursiveVisitor {
  bool found = false;

  @override
  void visitInstanceSet(InstanceSet node) {
    if (node.receiver is ThisExpression) found = true;
    super.visitInstanceSet(node);
  }

  /// `_listeners.remove(l)` on a field of `this`: a mutation in place is a
  /// write to the object as much as an assignment is (a `&mut self` method
  /// behind a `&self` trait, `_SystemFontsNotifier.removeListener`).
  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    final receiver = node.receiver;
    if (receiver is InstanceGet &&
        receiver.receiver is ThisExpression &&
        mutatingThisFieldNames.contains(node.name.text)) {
      found = true;
    }
    super.visitInstanceInvocation(node);
  }
}

/// Whether a function body mentions `this` anywhere inside it
/// (`_reachesThis`, which is the only caller).
class _ThisFinder extends RecursiveVisitor {
  bool found = false;

  @override
  void visitThisExpression(ThisExpression node) {
    found = true;
    super.visitThisExpression(node);
  }
}

/// Every closure written inside something.
class _ClosureFinder extends RecursiveVisitor {
  _ClosureFinder(this.found);

  final List<FunctionNode> found;

  @override
  void visitFunctionExpression(FunctionExpression node) {
    found.add(node.function);
    super.visitFunctionExpression(node);
  }

  @override
  void visitFunctionDeclaration(FunctionDeclaration node) {
    found.add(node.function);
    super.visitFunctionDeclaration(node);
  }
}

/// The **mutable** fields of `this` a closure reads or writes.
class _FieldsTouched extends RecursiveVisitor {
  final mutable = <String>{};

  void _look(Member? target) {
    if (target is Field && !target.isFinal) mutable.add(target.name.text);
  }

  @override
  void visitInstanceGet(InstanceGet node) {
    if (node.receiver is ThisExpression) {
      _look(node.interfaceTarget);
    } else {
      node.receiver.accept(this);
    }
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    if (node.receiver is ThisExpression) {
      _look(node.interfaceTarget);
    } else {
      node.receiver.accept(this);
    }
    node.value.accept(this);
  }
}

/// Every use of a parameter that is not "call it right here".
///
/// The one use a borrowed closure survives is being called. Anything else --
/// stored in a field, put in a list, handed on -- outlives the call, and a
/// borrow cannot.
class _ParameterEscapes extends RecursiveVisitor {
  _ParameterEscapes(this.param);

  final Object param;
  bool escapes = false;

  @override
  void visitLocalFunctionInvocation(LocalFunctionInvocation node) {
    if (identical(node.variable, param)) {
      node.arguments.accept(this);
      return;
    }
    super.visitLocalFunctionInvocation(node);
  }

  @override
  void visitFunctionInvocation(FunctionInvocation node) {
    final receiver = node.receiver;
    if (receiver is VariableGet && identical(receiver.variable, param)) {
      node.arguments.accept(this);
      return;
    }
    super.visitFunctionInvocation(node);
  }

  @override
  void visitVariableGet(VariableGet node) {
    if (identical(node.variable, param)) escapes = true;
  }
}

/// The `final` fields a closure reads on `this`, and whether they all are.
class _FinalFieldReads extends RecursiveVisitor {
  _FinalFieldReads(this.shared);

  /// The class's fields that live in a cell, which a closure may hold a
  /// handle to and both read and write.
  final Set<String> shared;

  final fields = <String, Field>{};

  /// Whether every field touched can be carried: `final` by copy, shared by
  /// handle. One that is neither means the closure would need `this`.
  bool allCarried = true;

  void _look(Member? target) {
    if (target is Field &&
        (target.isFinal || shared.contains(target.name.text))) {
      fields[target.name.text] = target;
    } else {
      allCarried = false;
    }
  }

  @override
  void visitInstanceGet(InstanceGet node) {
    if (node.receiver is ThisExpression) {
      _look(node.interfaceTarget);
    } else {
      node.receiver.accept(this);
    }
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    if (node.receiver is ThisExpression) {
      final target = node.interfaceTarget;
      // Writing is only carriable through a cell; a `final` field cannot be
      // written at all, so a write to one is not this shape.
      if (target is Field && shared.contains(target.name.text)) {
        fields[target.name.text] = target;
      } else {
        allCarried = false;
      }
    } else {
      node.receiver.accept(this);
    }
    node.value.accept(this);
  }
}

/// Whether a closure asks more of `this` than a shared borrow.
///
/// Reading a field of `this` is not demanding; writing one, calling a method on
/// it, tearing a method off it, or handing `this` itself to something all are.
/// `super` counts the same way -- it is the same object.
/// Whether a type names `cls`, at any depth of its arguments.
bool _mentions(DartType type, Class cls) => switch (type) {
  InterfaceType(:final classNode, :final typeArguments) =>
    classNode == cls || typeArguments.any((t) => _mentions(t, cls)),
  FutureOrType(:final typeArgument) => _mentions(typeArgument, cls),
  RecordType(:final positional, :final named) =>
    positional.any((t) => _mentions(t, cls)) ||
        named.any((n) => _mentions(n.type, cls)),
  FunctionType(:final positionalParameters, :final returnType) =>
    positionalParameters.any((t) => _mentions(t, cls)) ||
        _mentions(returnType, cls),
  _ => false,
};

/// Whether a class tears off one of its own methods anywhere.
/// Whether `this` is used as a *value* anywhere in the class: an argument,
/// a returned value, a stored value, a literal's element. Its use as a
/// receiver (`this.x`, `this.m()`) is not that.
class _ThisEscapes extends RecursiveVisitor {
  _ThisEscapes(this.handleSlot);

  /// Whether a slot of this type holds a handle (a trait object): `this`
  /// handed into one is kept by identity; into a value slot it is a copy.
  final bool Function(DartType) handleSlot;

  bool found = false;

  @override
  void visitThisExpression(ThisExpression node) {
    final parent = node.parent;
    if (parent is Arguments) {
      // A `dart:` library's top-level function (`identical(this, other)`,
      // `print(this)`) asks about the object and keeps nothing: not an
      // escape. Counting `Radius` for its `==` put its `+` on an `Rc`
      // (ws463).
      final call = parent.parent;
      if (call is StaticInvocation &&
          call.target.enclosingClass == null &&
          call.target.enclosingLibrary.importUri.scheme == 'dart') {
        return;
      }
      final slot = _slotOf(parent, node);
      // An unresolvable callee is taken to keep it.
      if (slot == null || handleSlot(slot)) found = true;
      return;
    }
    if (parent is ListLiteral ||
        parent is SetLiteral ||
        parent is MapLiteralEntry ||
        (parent is InstanceSet && identical(parent.value, node)) ||
        (parent is StaticSet && identical(parent.value, node))) {
      found = true;
    }
  }

  /// The declared type of the parameter `argument` lands in, if the call's
  /// callee can be read off the arguments' parent.
  static DartType? _slotOf(Arguments arguments, Expression argument) {
    final call = arguments.parent;
    final FunctionNode? fn = switch (call) {
      InstanceInvocation(:final interfaceTarget) => interfaceTarget.function,
      StaticInvocation(:final target) => target.function,
      ConstructorInvocation(:final target) => target.function,
      SuperMethodInvocation(:final interfaceTarget) => interfaceTarget.function,
      _ => null,
    };
    if (fn == null) return null;
    final index = arguments.positional.indexOf(argument);
    if (index >= 0) {
      return index < fn.positionalParameters.length
          ? fn.positionalParameters[index].type
          : null;
    }
    for (final named in arguments.named) {
      if (identical(named.value, argument)) {
        for (final p in fn.namedParameters) {
          if (p.parameterName == named.name) return p.type;
        }
      }
    }
    return null;
  }
}

class _TearOffFinder extends RecursiveVisitor {
  bool onThis = false;

  @override
  void visitInstanceTearOff(InstanceTearOff node) {
    if (node.receiver is ThisExpression) onThis = true;
    super.visitInstanceTearOff(node);
  }
}

class _ThisUse extends RecursiveVisitor {
  bool demanding = false;

  /// Everything demanding except *writing a field*, which a shared field's
  /// cell answers. `_FinalFieldReads` decides that part.
  bool demandingBeyondFields = false;

  @override
  void visitThisExpression(ThisExpression node) {
    demanding = true;
    demandingBeyondFields = true;
  }

  @override
  void visitInstanceGet(InstanceGet node) {
    // A read *of* `this` is the one thing allowed, so the receiver is not
    // walked -- walking it would find the `ThisExpression` and refuse.
    if (node.receiver is! ThisExpression) node.receiver.accept(this);
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    // The receiver is not walked when it is `this`: walking it reaches the
    // `ThisExpression` itself, which reads as "hands the whole object over"
    // and is exactly what a field write is not. `visitInstanceGet` has always
    // skipped it for the same reason; this one did not, and it made every
    // field write look like the object escaping.
    if (node.receiver is ThisExpression) {
      demanding = true;
    } else {
      node.receiver.accept(this);
    }
    node.value.accept(this);
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    if (node.receiver is ThisExpression) {
      demanding = true;
      demandingBeyondFields = true;
    }
    super.visitInstanceInvocation(node);
  }

  @override
  void visitInstanceTearOff(InstanceTearOff node) {
    if (node.receiver is ThisExpression) {
      demanding = true;
      demandingBeyondFields = true;
    }
    super.visitInstanceTearOff(node);
  }

  @override
  void visitSuperMethodInvocation(SuperMethodInvocation node) {
    demanding = true;
    demandingBeyondFields = true;
  }

  @override
  void visitSuperPropertySet(SuperPropertySet node) {
    demanding = true;
    demandingBeyondFields = true;
  }
}

/// The error types a function body throws.
class _ThrowFinder extends RecursiveVisitor {
  final types = <String>{};

  @override
  void visitThrow(Throw node) {
    final value = node.expression;
    types.add(switch (value) {
      ConstructorInvocation() => value.target.enclosingClass.name,
      StaticInvocation() => value.target.enclosingClass?.name ?? 'Object',
      _ => 'Object',
    });
    super.visitThrow(node);
  }
}

/// Every enum's variants, recovered from the constants that name them.
///
/// Walk the whole component once: an `InstanceConstant` of an enum class
/// carries the CFE's own `index` and `_name` fields, which is exactly the
/// ordinal and the name. Sorted by index, so `Axis::Horizontal` keeps the
/// position Dart gave it.
///
/// Only the variants something actually mentions are found. A variant no code
/// refers to leaves a gap, and a gap is worth knowing about: `enumValuesIn`
/// reports the indices it saw so the caller can tell a complete enum from a
/// partial one.
/// Top-level `dynamic` fields of the libraries given, with the types they
/// hold: the initialiser's, then every value stored into them anywhere in
/// those libraries. Only slots whose every value has a class are kept.
Map<Field, List<InterfaceType>> dynamicSlotsIn(
  Iterable<Library> libraries,
  TypeEnvironment env,
) {
  final slots = <Field, List<InterfaceType>>{};
  for (final library in libraries) {
    for (final field in library.fields) {
      final init = field.initializer;
      if (field.type is! DynamicType || init == null) continue;
      final t = init.getStaticType(StaticTypeContext(field, env));
      if (t is! InterfaceType) continue;
      slots[field] = [t];
      // intl's `UninitializedLocaleData<F>` is the placeholder for a
      // `Map<String, F>` that `initializeDateFormatting` stores later
      // through a `Function` call whose static type says nothing. The map
      // is the slot's other type, and this is where that is written down.
      if (t.classNode.name == 'UninitializedLocaleData' &&
          t.typeArguments.length == 1 &&
          t.classNode.enclosingLibrary.importUri.toString().startsWith(
            'package:intl/',
          )) {
        slots[field]!.add(
          InterfaceType(env.coreTypes.mapClass, Nullability.nonNullable, [
            env.coreTypes.stringNonNullableRawType,
            t.typeArguments.single,
          ]),
        );
      }
    }
  }
  final finder = _SlotStores(slots, env);
  for (final library in libraries) {
    library.accept(finder);
  }
  return slots;
}

class _SlotStores extends RecursiveVisitor {
  _SlotStores(this.slots, this.env);
  final Map<Field, List<InterfaceType>> slots;
  final TypeEnvironment env;
  Member? _member;

  @override
  void defaultMember(Member node) {
    _member = node;
    super.defaultMember(node);
    _member = null;
  }

  @override
  void visitStaticSet(StaticSet node) {
    var target = node.target;
    // Through a setter that only stores its value into the slot (`set
    // dateTimeSymbols(dynamic symbols) { ..; _dateTimeSymbols = symbols; }`):
    // the store is the field's, as the read through a getter is.
    if (target is Procedure && target.isSetter) {
      final stored = _storedField(target);
      if (stored != null) target = stored;
    }
    final held = slots[target is Field ? target : null];
    final member = _member;
    if (held != null && member != null) {
      final t = node.value.getStaticType(StaticTypeContext(member, env));
      if (t is InterfaceType) {
        if (!held.any((h) => h.classNode == t.classNode)) held.add(t);
      }
    }
    super.visitStaticSet(node);
  }

  /// The field a setter stores its parameter into, or null.
  static Field? _storedField(Procedure setter) {
    final param = setter.function.positionalParameters.singleOrNull;
    if (param == null) return null;
    final finder = _ParamStoreFinder(param);
    setter.function.body?.accept(finder);
    return finder.field;
  }
}

class _ParamStoreFinder extends RecursiveVisitor {
  _ParamStoreFinder(this.param);
  final Variable param;
  Field? field;

  @override
  void visitStaticSet(StaticSet node) {
    final value = node.value;
    final target = node.target;
    if (target is Field && value is VariableGet && value.variable == param) {
      field = target;
    }
    super.visitStaticSet(node);
  }
}

Map<Class, List<String>> enumValuesIn(Component component) =>
    enumsIn(component).$1;

/// The variants, and what each one carries.
///
/// A Dart enum can give every value its own final fields --
/// `enum Tristate { none(0), isTrue(1), isFalse(2); final int value; }` -- and
/// that used to be refused outright, on the grounds that a Rust enum would
/// need a payload per variant to say the same thing. It would not: the values
/// are **constants of the variant**, so the Rust for them is a `match` in a
/// method. The constants carry them, and this is where they are picked up.
(Map<Class, List<String>>, Map<Class, Map<String, Map<String, String>>>)
enumsIn(Component component) {
  final byIndex = <Class, Map<int, String>>{};
  final fields = <Class, Map<String, Map<String, String>>>{};
  final finder = _EnumConstantFinder(byIndex, fields);
  for (final library in component.libraries) {
    library.accept(finder);
  }
  return (
    {
      for (final entry in byIndex.entries)
        entry.key: (entry.value.keys.toList()..sort())
            .map((i) => entry.value[i]!)
            .toList(),
    },
    fields,
  );
}

class _EnumConstantFinder extends RecursiveVisitor {
  _EnumConstantFinder(this.byIndex, this.fields);

  final Map<Class, Map<int, String>> byIndex;

  /// Class -> variant name -> field name -> the Rust literal for it.
  ///
  /// Only literals. A variant carrying a `List` or another object is state
  /// this cannot write as a `match` arm, and the enum stays refused rather
  /// than half-translated.
  final Map<Class, Map<String, Map<String, String>>> fields;

  static const _implicit = {'index', '_name', 'hashCode'};

  static String? _literal(Constant value) => switch (value) {
    IntConstant(:final value) => '$value',
    DoubleConstant(:final value) => '$value',
    BoolConstant(:final value) => '$value',
    // `replaceAll(r'\', r'\\')`. It was written once as `replaceAll(r'', ..)`
    // -- replacing the *empty* string, which inserts a backslash before every
    // character -- and `Variant.monochrome` came out as `"\m\o\n\o..."`, which
    // stops the whole crate at the lexer. The analyzer side had it right, so
    // the two front ends disagreed and no fixture noticed, because no fixture
    // had an enum variant carrying a string. One does now.
    StringConstant(:final value) =>
      '"${value.replaceAll(r'\', r'\\').replaceAll('"', r'\"')}".to_string()',
    _ => null,
  };

  final _seen = <Constant>{};

  void _look(Constant constant) {
    if (!_seen.add(constant)) return;
    // Inside, not just on top. No enum in the dill carries its element fields
    // -- the CFE strips them, so a variant is only knowable from a constant
    // that *is* one -- and those constants are often nested: `Tristate.isTrue`
    // appears as a field value of a `SemanticsFlags` constant and nowhere on
    // its own, which is why that enum came out with no variants at all while
    // `Axis` next to it was fine.
    if (constant is ListConstant) {
      constant.entries.forEach(_look);
    } else if (constant is SetConstant) {
      constant.entries.forEach(_look);
    } else if (constant is MapConstant) {
      for (final entry in constant.entries) {
        _look(entry.key);
        _look(entry.value);
      }
    } else if (constant is InstantiationConstant) {
      _look(constant.tearOffConstant);
    } else if (constant is RecordConstant) {
      constant.positional.forEach(_look);
      constant.named.values.forEach(_look);
    }
    if (constant is! InstanceConstant) return;
    constant.fieldValues.values.forEach(_look);
    if (!constant.classNode.isEnum) return;
    int? index;
    String? name;
    for (final entry in constant.fieldValues.entries) {
      final field = entry.key.asField.name.text;
      final value = entry.value;
      if (field == 'index' && value is IntConstant) index = value.value;
      if (field == '_name' && value is StringConstant) name = value.value;
    }
    if (index == null || name == null) return;
    (byIndex[constant.classNode] ??= <int, String>{})[index] = name;
    final own = <String, String>{};
    for (final entry in constant.fieldValues.entries) {
      final field = entry.key.asField.name.text;
      if (_implicit.contains(field)) continue;
      final literal = _literal(entry.value);
      if (literal == null) return;
      own[field] = literal;
    }
    (fields[constant.classNode] ??= <String, Map<String, String>>{})[name] =
        own;
  }

  @override
  void visitConstantExpression(ConstantExpression node) {
    _look(node.constant);
    super.visitConstantExpression(node);
  }
}

/// Every abstract class in the component, by name.
///
/// The backend decides `dyn Trait` against a plain struct from this. A library
/// only knows its own classes, which was fine while one library was emitted at
/// a time and is not once a whole package shares a crate.
/// The default gate for open classes: every one. The gate began as the
/// `ParentData` family (8 names, 7827 stubs) while the lowering was measured;
/// once an open class's construction left as its trait handle, opening all
/// 145 measured 7397 (STATUS, ws270). `DART2RUST_OPEN=a,b` narrows it.
const defaultOpenClasses = 'all';

/// Concrete translated classes with a translated concrete subclass, within
/// the gate (`all`, or a comma-separated list of names).
Set<Class> openClassesIn(
  Component component,
  List<String> prefixes,
  String gate,
) {
  final allowed = gate == 'all'
      ? null
      : gate.split(',').map((s) => s.trim()).toSet();
  bool translated(Library l) => prefixes.any(l.importUri.toString().startsWith);
  final hasSubclass = <Class>{};
  void opens(Class? base) {
    if (base != null &&
        !base.isAbstract &&
        !base.isEnum &&
        translated(base.enclosingLibrary)) {
      hasSubclass.add(base);
    }
  }

  for (final library in component.libraries) {
    if (!translated(library)) continue;
    for (final cls in library.classes) {
      if (cls.isAnonymousMixin || cls.isEnum) continue;
      // The subclass may itself be abstract (`ColorSwatch extends Color`),
      // and a concrete class is a supertype through `implements` as much as
      // through `extends` (`CupertinoDynamicColor .. implements Color`,
      // 170 of the 3623 mismatches at ws270). Both make the base open.
      var base = cls.superclass;
      while (base != null && base.isAnonymousMixin) {
        base = base.superclass;
      }
      opens(base);
      for (final i in cls.implementedTypes) {
        opens(i.classNode);
      }
    }
  }
  return {
    for (final c in hasSubclass)
      if (allowed == null || allowed.contains(c.name)) c,
  };
}

Set<String> abstractClassesIn(Component component, List<String> prefixes) {
  final names = <String>{};
  for (final library in component.libraries) {
    final uri = library.importUri.toString();
    if (!prefixes.any(uri.startsWith)) continue;
    for (final cls in library.classes) {
      if (cls.isAbstract && !cls.isAnonymousMixin) names.add(cls.name);
    }
  }
  return names;
}

/// Everything the walk of a library can say about what it names, said once.
///
/// `library.dependencies` looked like the import graph and is not one. The
/// CFE resolves `import 'package:flutter/painting.dart'` -- a barrel that only
/// re-exports -- away entirely: there are **no** flutter barrels in the dill,
/// and the edges they carried are not spliced into the importer. So
/// `cupertino/nav_bar.dart` depends on no painting library at all while using
/// `TextStyle` 348 times.
///
/// What a library needs is not what it declared it imports; it is what it
/// mentions. This walks the body and collects the library of every class and
/// member it reaches -- which is exactly the set of `use` lines that make it
/// compile, and no more.
///
/// The three answers come from one walk because they are one walk: each of
/// the two callers below used to make its own `_ReferenceCollector` and its
/// own `_climb`, and each said in its comment that gathering them separately
/// would let them drift. A third answer would have been a third walk.
({
  Set<Library> libraries,
  Map<String, Set<Library>> classNames,
  Set<Member> members,
})
referencesOf(
  Library library, {
  Map<Class, List<Class>> applications = const {},
}) {
  final found = <Library>{};
  final visitor = _ReferenceCollector(found);
  library.accept(visitor);
  for (final cls in library.classes) {
    // A mixin declaration is emitted here with the *applications*' members
    // in it: TFA drops a body from the declaration and leaves it in every
    // application, and `lowerClass` lowers it back into the mixin's own
    // module. Those Kernel nodes live in whichever library wrote
    // `with AnimationLocalListenersMixin`, so walking this library alone
    // missed every name they use -- `FlutterError` and the whole of
    // `foundation/diagnostics.dart` for `notifyListeners`, and the binding
    // chain's `super.initInstances` for `SchedulerBinding`.
    if (cls.isMixinDeclaration) {
      // The *first* application that has each member, as `lowerClass` takes
      // it: every application holds a copy, and walking all of them read one
      // mixin's body through every library that applies it -- 924 modules
      // fell into one 599-module cycle.
      String keyOf(Procedure p) => p.isSetter ? '${p.name.text}=' : p.name.text;
      final own = {for (final f in cls.fields) f.name.text};
      final declared = {
        for (final p in cls.procedures)
          if (p.function.body != null) keyOf(p),
        for (final f in cls.fields) f.name.text,
        for (final f in cls.fields)
          if (!f.isFinal) '${f.name.text}=',
      };
      final seenProcedure = <String>{};
      final seenField = <String>{};
      for (final application in applications[cls] ?? const <Class>[]) {
        for (final p in application.procedures) {
          if (p.isStatic || p.isAbstract) continue;
          final key = keyOf(p);
          if (declared.contains(key) || !seenProcedure.add(key)) continue;
          p.accept(visitor);
        }
        for (final f in application.fields) {
          if (f.isStatic || own.contains(f.name.text)) continue;
          if (!seenField.add(f.name.text)) continue;
          f.accept(visitor);
        }
      }
    }
    // The whole ancestry, not just the direct supertype. The backend flattens
    // a base class's fields into the subclass and emits an `impl` for every
    // abstract *ancestor*, so a grandparent two modules away is named in the
    // output even though nothing in the body mentions it -- 1008 "cannot find
    // trait" until this walked the chain.
    // And each ancestor's own *declarations*, not just the ancestor. Flattening
    // copies a base's fields into the subclass, so `Widget`'s `Key? key` lands
    // in every widget struct -- and `Key` lives in `foundation/key.dart`, which
    // a widget library never names for itself. 1104 of the 1467 "cannot find
    // trait" were that one field's type; 1463 of them were this in total.
    //
    // The whole ancestor is walked rather than just its field types: a method
    // signature copied into an `impl` names types the same way, and one rule
    // that covers both cannot disagree with itself.
    _climb(cls, (node) {
      visitor._class(node);
      node.accept(visitor);
    });
  }
  return (
    libraries: {...found}..remove(library),
    classNames: visitor.namedClasses,
    members: visitor.members,
  );
}

/// Every class in an ancestry, each visited once.
void _climb(Class start, void Function(Class) visit) {
  final seen = <Class>{};
  void walk(Class node) {
    if (!seen.add(node)) return;
    visit(node);
    for (final type in [
      if (node.supertype != null) node.supertype!,
      if (node.mixedInType != null) node.mixedInType!,
      ...node.implementedTypes,
    ]) {
      walk(type.classNode);
    }
  }

  walk(start);
}

class _ReferenceCollector extends RecursiveVisitor {
  _ReferenceCollector(this.found);

  final Set<Library> found;

  /// Which library each class *name* came from.
  ///
  /// `use crate::<module>::*` for every referenced module is ambiguous whenever
  /// two of them define the same name, and ten names do -- `TextStyle`,
  /// `Image`, `Path` and `Gradient` are each defined once in `dart:ui` and
  /// again in `painting`, which is 800 `E0659`s between them. An explicit
  /// `use` beats a glob in Rust, so the fix is to name the one that was meant.
  /// This records which that is; a name seen from two libraries at once stays
  /// out of it, because no single `use` would be right.
  final Map<String, Set<Library>> namedClasses = {};

  /// Every member reached, not just the library each one came from.
  ///
  /// The same answer as [namedClasses], for the names that are not classes.
  /// `_member` had it in hand and dropped it, so a top-level function, a
  /// top-level constant and a class's statics were the references the emitter
  /// had to guess back out of its own text -- and `locale` the parameter and
  /// `locale` the top-level function are the same seven characters there.
  ///
  /// The members themselves, not names: how one is spelled in Rust is the
  /// backend's to say, and this file does not know it.
  final Set<Member> members = {};

  void _member(Member? member) {
    if (member == null) return;
    found.add(member.enclosingLibrary);
    members.add(member);
    // The class a constructor or static belongs to is named by the call
    // (`Image(..)` in `ImageIcon.build` named `widgets/image.dart`'s
    // `Image`, which two modules define; without the class here the
    // import chose neither, E0433, 18 at ws464).
    _class(member.enclosingClass);
  }

  void _class(Class? cls) {
    if (cls == null) return;
    found.add(cls.enclosingLibrary);
    final name = cls.name;
    (namedClasses[name] ??= {}).add(cls.enclosingLibrary);
  }

  @override
  void visitInterfaceType(InterfaceType node) {
    _class(node.classNode);
    super.visitInterfaceType(node);
  }

  @override
  void visitConstructorInvocation(ConstructorInvocation node) {
    _member(node.target);
    _defaults(node.target, node.arguments);
    super.visitConstructorInvocation(node);
  }

  @override
  void visitStaticInvocation(StaticInvocation node) {
    _member(node.target);
    _defaults(node.target, node.arguments);
    super.visitStaticInvocation(node);
  }

  @override
  void visitStaticGet(StaticGet node) {
    _member(node.target);
    super.visitStaticGet(node);
  }

  @override
  void visitStaticSet(StaticSet node) {
    _member(node.target);
    super.visitStaticSet(node);
  }

  @override
  void visitStaticTearOff(StaticTearOff node) {
    _member(node.target);
    super.visitStaticTearOff(node);
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    _member(node.interfaceTarget);
    _defaults(node.interfaceTarget, node.arguments);
    super.visitInstanceInvocation(node);
  }

  @override
  void visitInstanceGet(InstanceGet node) {
    _member(node.interfaceTarget);
    super.visitInstanceGet(node);
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    _member(node.interfaceTarget);
    super.visitInstanceSet(node);
  }

  // A `super` call's own Rust name is not decided here: which class holds
  // the body is `_realOwner`'s answer, and the front end writes it down
  // (`KernelFrontend.superOwners`). This records only the member, as any
  // other reference to it would be.
  @override
  void visitSuperMethodInvocation(SuperMethodInvocation node) {
    _member(node.interfaceTarget);
    _defaults(node.interfaceTarget, node.arguments);
    super.visitSuperMethodInvocation(node);
  }

  @override
  void visitSuperPropertyGet(SuperPropertyGet node) {
    _member(node.interfaceTarget);
    super.visitSuperPropertyGet(node);
  }

  @override
  void visitSuperPropertySet(SuperPropertySet node) {
    _member(node.interfaceTarget);
    super.visitSuperPropertySet(node);
  }

  /// The default arguments a call leaves off.
  ///
  /// Dart applies them in the callee; Rust has no defaults, so the front end
  /// copies the callee's initializer into *this* library (`_omitted`) and the
  /// emitter writes it here. `TwoPane(..)` never mentioned
  /// `VerticalDirection`, and `VerticalDirection::Down` is in the line it
  /// emitted -- the reference is this library's, made on its behalf. Reading
  /// it here is the same rule the emitter follows, asked one step earlier.
  final Set<FunctionNode> _inDefaults = {};

  void _defaults(Member? target, Arguments arguments) {
    final callee = target?.function;
    if (callee == null || !_inDefaults.add(callee)) return;
    final supplied = {for (final n in arguments.named) n.name};
    for (final param in callee.namedParameters) {
      if (supplied.contains(param.parameterName)) continue;
      param.initializer?.accept(this);
    }
    for (
      var i = arguments.positional.length;
      i < callee.positionalParameters.length;
      i++
    ) {
      callee.positionalParameters[i].initializer?.accept(this);
    }
    _inDefaults.remove(callee);
  }

  @override
  void visitConstantExpression(ConstantExpression node) {
    _constant(node.constant);
    super.visitConstantExpression(node);
  }

  void _constant(Constant constant) {
    if (constant is InstanceConstant) {
      _class(constant.classNode);
      constant.fieldValues.values.forEach(_constant);
    } else if (constant is StaticTearOffConstant) {
      _member(constant.target);
    } else if (constant is ConstructorTearOffConstant) {
      _member(constant.target);
    } else if (constant is ListConstant) {
      constant.entries.forEach(_constant);
    } else if (constant is MapConstant) {
      for (final entry in constant.entries) {
        _constant(entry.key);
        _constant(entry.value);
      }
    }
  }
}

/// The class type parameters a body reads as type literals (`T` as a
/// value; see `_typeLiteralParams`).
class _TypeLiteralFinder extends RecursiveVisitor {
  _TypeLiteralFinder(this.parameters);

  final Set<TypeParameter> parameters;
  final Set<TypeParameter> found = {};

  @override
  void visitTypeLiteral(TypeLiteral node) {
    _use(node.type);
    super.visitTypeLiteral(node);
  }

  // `v is S` / `v as S` ask the same question of the parameter: through
  // an erased twin, `S` is `Rc<dyn Object>` and `is` said yes to
  // everything (`TagLayer.findAnnotations<S>`, the outparam fixture).
  @override
  void visitIsExpression(IsExpression node) {
    _use(node.type);
    super.visitIsExpression(node);
  }

  @override
  void visitAsExpression(AsExpression node) {
    _use(node.type);
    super.visitAsExpression(node);
  }

  void _use(DartType t) {
    if (t is TypeParameterType && parameters.contains(t.parameter)) {
      found.add(t.parameter);
    }
  }
}

/// Whether a statement holds a labelled switch that some `break` leaves
/// early (see `_loopBody`).
class _EarlySwitchFinder extends RecursiveVisitor {
  bool found = false;

  @override
  void visitLabeledStatement(LabeledStatement node) {
    if (found) return;
    final body = node.body;
    if (body is SwitchStatement &&
        _SwitchBreakFinder.earlyBreaks(node, body).isNotEmpty) {
      found = true;
      return;
    }
    super.visitLabeledStatement(node);
  }

  @override
  void visitFunctionNode(FunctionNode node) {}
}

/// The `break`s out of a labelled switch that are not the trailing
/// statement of their case body (see `_caseBody`).
class _SwitchBreakFinder extends RecursiveVisitor {
  _SwitchBreakFinder(this.label, this.trailing);

  final LabeledStatement label;
  final Set<BreakStatement> trailing;
  final List<BreakStatement> found = [];

  static List<BreakStatement> earlyBreaks(
    LabeledStatement label,
    SwitchStatement body,
  ) {
    final trailing = <BreakStatement>{};
    for (final c in body.cases) {
      final last = KernelFrontend._trailingStatement(c.body);
      if (last is BreakStatement) trailing.add(last);
    }
    final finder = _SwitchBreakFinder(label, trailing);
    body.accept(finder);
    return finder.found;
  }

  @override
  void visitBreakStatement(BreakStatement node) {
    if (node.target == label && !trailing.contains(node)) found.add(node);
    super.visitBreakStatement(node);
  }

  @override
  void visitFunctionNode(FunctionNode node) {
    // A closure's own breaks are its own.
  }
}

/// Whether a body mutates a variable's value in place: a collection
/// mutator called on it, or an index written (see `_let`).
class _TempMutationFinder extends RecursiveVisitor {
  _TempMutationFinder(this.variable);

  final Variable variable;
  bool found = false;

  static bool mutates(Variable variable, Expression body) {
    final finder = _TempMutationFinder(variable);
    body.accept(finder);
    return finder.found;
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    if (found) return;
    final receiver = node.receiver;
    if (receiver is VariableGet &&
        receiver.variable == variable &&
        mutatingListNames.contains(node.name.text) &&
        node.name.text != 'length') {
      found = true;
      return;
    }
    super.visitInstanceInvocation(node);
  }
}

/// Finds the static fields filled in place (see `_mutatedStatics`).
class _StaticFillFinder extends RecursiveVisitor {
  _StaticFillFinder(this.frontend);

  final KernelFrontend frontend;
  final Set<Field> found = {};

  /// A cascade binds the receiver first: `let #t = log in #t..add(..)`.
  final Map<Variable, Field> _aliases = {};

  Field? _staticOf(Expression e) {
    var bare = e;
    while (bare is FileUriExpression) {
      bare = bare.expression;
    }
    if (bare is StaticGet) {
      final target = bare.target;
      return target is Field && target.isStatic ? target : null;
    }
    if (bare is VariableGet) return _aliases[bare.variable];
    return null;
  }

  @override
  void visitLet(Let node) {
    final init = node.variable.initializer;
    final field = init == null ? null : _staticOf(init);
    if (field != null) _aliases[node.variable] = field;
    super.visitLet(node);
  }

  @override
  void visitVariableDeclaration(VariableDeclaration node) {
    final variable = node.variable;
    final init = variable.initializer;
    // A `final` local holding a static collection is the same list.
    final field = init == null || !variable.isFinal ? null : _staticOf(init);
    if (field != null) _aliases[variable] = field;
    super.visitVariableDeclaration(node);
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    final field = _staticOf(node.receiver);
    if (field != null &&
        mutatingListNames.contains(node.name.text) &&
        node.name.text != 'length') {
      found.add(field);
    }
    super.visitInstanceInvocation(node);
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    // A field written through a static holding a *value* struct changes
    // the static; through a counted class's handle it changes the shared
    // object, and the static stays what it was (`GoogleFonts.config.
    // allowRuntimeFetching = false`, ws577).
    final field = _staticOf(node.receiver);
    final type = field?.type;
    if (field != null &&
        type is InterfaceType &&
        frontend._translatedClass(type.classNode) &&
        !frontend._isCountedName(type.classNode.name)) {
      found.add(field);
    }
    super.visitInstanceSet(node);
  }

  @override
  void visitStaticInvocation(StaticInvocation node) {
    final positional = node.arguments.positional;
    for (var i = 0; i < positional.length; i++) {
      final field = _staticOf(positional[i]);
      if (field != null && frontend._fillsParameter(node.target, i)) {
        found.add(field);
      }
    }
    super.visitStaticInvocation(node);
  }
}

/// Finds a `List`/`Set` parameter being filled (see `_fillsParameter`).
class _FillFinder extends RecursiveVisitor {
  _FillFinder(this.param, this.frontend);

  final Variable param;
  final KernelFrontend frontend;
  bool found = false;

  bool _isParam(Expression e) => e is VariableGet && e.variable == param;

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    if (found) return;
    if (_isParam(node.receiver) &&
        mutatingListNames.contains(node.name.text) &&
        node.name.text != 'length') {
      found = true;
      return;
    }
    super.visitInstanceInvocation(node);
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    if (found) return;
    if (_isParam(node.receiver)) {
      found = true;
      return;
    }
    super.visitInstanceSet(node);
  }

  @override
  void visitStaticInvocation(StaticInvocation node) {
    if (found) return;
    final positional = node.arguments.positional;
    for (var i = 0; i < positional.length; i++) {
      if (_isParam(positional[i]) && frontend._fillsParameter(node.target, i)) {
        found = true;
        return;
      }
    }
    super.visitStaticInvocation(node);
  }
}
