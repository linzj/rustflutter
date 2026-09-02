// Kernel (`.dill`) -> IR.
//
// The second front end. `frontend.dart` reads analyzer's resolved AST; this
// reads what the Dart toolchain itself produced, which is the input a release
// would be built from: a whole linked program, with mixins applied, super calls
// resolved to their target member, and constants evaluated.
//
// The IR is unchanged, and that is the point of the exercise: if two front ends
// producing the same IR do not produce the same Rust, one of them is wrong.
//
// **Kernel is desugared, and guessing its shape from the Dart source is the
// mistake to avoid.** `x - other.x` is not a binary expression here; it is
// `InstanceInvocation(x, '-', [other.x])`. This file was written against a dump
// of the nodes that actually occur in `Alignment`, not against the source.
library;

import 'package:kernel/ast.dart';

import 'ir.dart';

/// Dart operators that are binary, spelled as Kernel names them.
const _binaryOperators = {
  '+', '-', '*', '/', '~/', '%', '==', '<', '>', '<=', '>=',
  '&', '|', '^', '<<', '>>', '>>>',
};

class KernelFrontend {
  KernelFrontend(this.library);

  final Library library;
  String? _superclass;

  // -- Types ------------------------------------------------------------------

  IrType _type(DartType type) {
    final nullable = type.nullability == Nullability.nullable;
    if (type is InterfaceType) {
      return IrType(type.classNode.name, nullable: nullable);
    }
    if (type is VoidType) return const IrType('void');
    if (type is DynamicType) return const IrType('dynamic');
    if (type is NullType) return const IrType('Null', nullable: true);
    if (type is TypeParameterType) {
      return IrType(type.parameter.name ?? 'T', nullable: nullable);
    }
    if (type is FunctionType) {
      return IrType.function(
        [for (final p in type.positionalParameters) _type(p)],
        _type(type.returnType),
        nullable: nullable,
      );
    }
    return IrType(type.runtimeType.toString());
  }

  // -- Expressions ------------------------------------------------------------

  IrExpr expression(Expression node) {
    if (node is IntLiteral) {
      return IrLiteral('${node.value}', const IrType('int'));
    }
    if (node is DoubleLiteral) {
      return IrLiteral('${node.value}', const IrType('double'));
    }
    if (node is BoolLiteral) {
      return IrLiteral('${node.value}', const IrType('bool'));
    }
    if (node is StringLiteral) {
      return IrLiteral(node.value, const IrType('String'));
    }
    if (node is NullLiteral) {
      return IrLiteral('null', const IrType('Null', nullable: true));
    }
    if (node is ThisExpression) return const IrThis();
    if (node is VariableGet) {
      // `cosmeticName` is Kernel's word for the name a human wrote; a variable
      // the CFE invented has none, and one whose name starts with `#` is a
      // temporary from its own lowering.
      final bound = _bound;
      if (bound != null && node.variable == bound) return const IrBound();
      if (_cascade != null && node.variable == _cascade) {
        return const IrLocal(_cascadeName);
      }
      final name = node.variable.cosmeticName;
      if (name == null || name.startsWith('#')) {
        throw Unsupported('synthetic variable', _sample(node));
      }
      return IrLocal(name);
    }
    if (node is InstanceGet) return _instanceGet(node);
    if (node is StaticGet) return _staticGet(node);
    if (node is InstanceInvocation) return _instanceInvocation(node);
    if (node is BlockExpression) return _blockValue(node);
    if (node is FunctionInvocation) {
      return IrCallValue(
          expression(node.receiver), _arguments(node.arguments));
    }
    if (node is LocalFunctionInvocation) {
      final name = node.variable.cosmeticName;
      if (name == null) {
        throw Unsupported('call of an unnamed local function', _sample(node));
      }
      return IrCallValue(IrLocal(name), _arguments(node.arguments));
    }
    if (node is FunctionExpression) return _closure(node.function, node);
    if (node is Let) return _let(node);
    if (node is EqualsNull) return IrIsNull(expression(node.expression));
    if (node is EqualsCall) {
      return IrBinary('==', expression(node.left), expression(node.right));
    }
    if (node is Not) {
      return IrUnary('!', expression(node.operand));
    }
    if (node is LogicalExpression) {
      return IrBinary(node.operatorEnum == LogicalExpressionOperator.AND
          ? '&&'
          : '||', expression(node.left), expression(node.right));
    }
    if (node is ConditionalExpression) {
      return IrConditional(expression(node.condition),
          expression(node.then), expression(node.otherwise));
    }
    if (node is IsExpression) {
      return IrIs(expression(node.operand), _type(node.type));
    }
    if (node is ConstructorInvocation) return _construct(node);
    if (node is StaticInvocation) return _staticInvocation(node);
    if (node is SuperMethodInvocation) {
      // The target member is already resolved -- this is the fact the analyzer
      // front end had to work out for itself.
      final owner = node.interfaceTarget.enclosingClass?.name;
      if (owner == null) {
        throw Unsupported('super call with no owner', '$node');
      }
      return IrSuperCall(owner, node.name.text, _arguments(node.arguments));
    }
    if (node is VariableSet) {
      throw Unsupported('assignment used for its value', _sample(node));
    }
    if (node is NullCheck) return IrNullCheck(expression(node.operand));
    if (node is AsExpression) return expression(node.operand);
    if (node is ConstantExpression) return _constant(node.constant, node);
    throw Unsupported('expression ${node.runtimeType}', _sample(node));
  }

  /// A cascade, restored.
  ///
  /// The CFE writes `Paint()..color = c` as "bind #0, write to #0, produce #0",
  /// which is a Rust block expression exactly. Only that shape is taken: a
  /// `BlockExpression` whose statements are a switch in disguise is a different
  /// construct and waits for switch.
  IrExpr _blockValue(BlockExpression node) {
    final statements = node.body.statements;
    final value = node.value;
    if (statements.isEmpty) {
      throw Unsupported('block expression with no statements', _sample(node));
    }
    final first = statements.first;
    if (first is! VariableStatement) {
      throw Unsupported('block expression not binding first', _sample(node));
    }
    final bound = first.declaration.variable;
    if (!(value is VariableGet && value.variable == bound)) {
      throw Unsupported('block expression not producing its binding',
          _sample(node));
    }
    final initial = bound.initializer;
    if (initial == null) {
      throw Unsupported('cascade binding with no receiver', _sample(node));
    }

    final previous = _cascade;
    _cascade = bound;
    try {
      final steps = <IrStmt>[
        IrLocalDecl(_cascadeName, _type(bound.type), expression(initial)),
        for (final s in statements.skip(1)) statement(s),
      ];
      return IrBlockValue(steps, const IrLocal(_cascadeName));
    } finally {
      _cascade = previous;
    }
  }

  /// The receiver the enclosing cascade bound. Reads of it become a local.
  Variable? _cascade;
  static const _cascadeName = 'cascaded';

  /// A closure literal, when it captures nothing this compiler cannot give it.
  ///
  /// A closure reaching `this` is refused: it outlives the call that made it,
  /// and `this` is a borrow, so it needs an ownership arrangement rather than a
  /// translation. That is 60% of `package:flutter`'s closures and a round of
  /// its own.
  IrExpr _closure(FunctionNode fn, Node origin) {
    if (_reachesThis(fn)) {
      throw Unsupported('closure capturing `this`', _sample(origin));
    }
    final body = fn.body;
    if (body == null) throw Unsupported('closure with no body', _sample(origin));
    return IrClosure(
      [
        for (final p in fn.positionalParameters)
          IrParam(p.cosmeticName ?? '_', _type(p.type)),
      ],
      statement(body),
      _type(fn.returnType),
    );
  }

  bool _reachesThis(FunctionNode fn) {
    final finder = _ThisFinder();
    fn.accept(finder);
    return finder.found;
  }

  /// Restores the Dart a `Let` was lowered from.
  ///
  /// `Let` is not a Dart construct -- it is the CFE's own temporary, and there
  /// are 14946 of them under `package:flutter`. Emitting the temporary as
  /// written would produce Rust nobody could read against upstream, which is
  /// the judgement round eight already made for operators: restore, do not
  /// transliterate.
  ///
  /// The shape here is `a ?? b`:
  ///
  ///     let final T #0 = a in #0 == null ? b : #0
  ///
  /// recognised by the else branch being the temporary itself. 6764 of the
  /// lets are this, 45% of them. The rest still stop -- `a?.b` is 4838 more
  /// and is the next shape, not this one.
  IrExpr _let(Let node) {
    final body = node.body;
    // A cascade: the binding is on the `Let` and the steps are a block whose
    // value is that binding. The standalone `BlockExpression` shape exists too,
    // and the probe that measured these looked only at *it* -- so this shape,
    // which is the one upstream actually produces, was missed until the fixture
    // compared the two front ends.
    if (body is BlockExpression &&
        _isThe(body.value, node.variable)) {
      final initial = node.variable.initializer;
      if (initial == null) {
        throw Unsupported('cascade binding with no receiver', _sample(node));
      }
      final previous = _cascade;
      _cascade = node.variable;
      try {
        return IrBlockValue([
          IrLocalDecl(
              _cascadeName, _type(node.variable.type), expression(initial)),
          for (final s in body.body.statements) statement(s),
        ], const IrLocal(_cascadeName));
      } finally {
        _cascade = previous;
      }
    }
    if (body is ConditionalExpression) {
      final condition = body.condition;
      final otherwise = body.otherwise;
      // `a?.b` -- the null branch is null and the other branch uses the
      // temporary. Recognised before `??` reads more naturally but the two are
      // disjoint: `??` has the temporary in the *else*, `?.` has null in the
      // *then*.
      if (condition is EqualsNull &&
          _isThe(condition.expression, node.variable) &&
          _isNull(body.then)) {
        final value = node.variable.initializer;
        if (value == null) {
          throw Unsupported('`?.` with no receiver', _sample(node));
        }
        final receiver = expression(value);
        final previous = _bound;
        _bound = node.variable;
        try {
          return IrNullAware(receiver, expression(otherwise));
        } finally {
          _bound = previous;
        }
      }
      if (condition is EqualsNull &&
          _isThe(condition.expression, node.variable) &&
          _isThe(otherwise, node.variable)) {
        final value = node.variable.initializer;
        if (value == null) {
          throw Unsupported('`??` with no left side', _sample(node));
        }
        final right = body.then;
        return IrIfNull(
          expression(value),
          expression(right),
          // Whether the whole thing is still nullable is the right side's
          // question: `a ?? b` is non-null exactly when `b` is.
          // The conditional carries its own static type, so no type context
          // has to be built to ask this.
          nullableResult: body.staticType.nullability == Nullability.nullable,
          eager: right is BasicLiteral || right is ConstantExpression,
        );
      }
    }
    throw Unsupported('CFE `Let` temporary', _sample(node));
  }

  // A `Let`'s variable is a `SyntheticVariable`, not a `VariableDeclaration`:
  // the CFE made it, so it has no declaration to point at.
  bool _isThe(Expression e, Variable variable) =>
      e is VariableGet && e.variable == variable;

  bool _isNull(Expression e) =>
      e is NullLiteral ||
      (e is ConstantExpression && e.constant is NullConstant);

  /// The temporary the enclosing `?.` bound, if any. Reads of it become
  /// [IrBound] so the backend can name it as a closure parameter.
  Variable? _bound;

  String _sample(Node node) {
    final text = node.toString().replaceAll('\n', ' ');
    return text.length > 90 ? '${text.substring(0, 90)}...' : text;
  }

  /// A field read, or a getter call -- and the difference matters in Rust.
  ///
  /// Dart spells both `a.x`. Rust spells a field `a.x` and a getter `a.x()`,
  /// and getting it wrong does not compile: `_x` on `AlignmentGeometry` is an
  /// abstract getter, so it becomes a trait method, and reading it as a field
  /// gives "attempted to take value of method `_x`".
  ///
  /// Kernel says which it is outright -- the target is a `Field` or a
  /// `Procedure` -- so nothing has to be inferred.
  IrExpr _instanceGet(InstanceGet node) {
    final name = node.name.text;
    final receiver = node.receiver;
    final target = receiver is ThisExpression ? null : expression(receiver);
    if (node.interfaceTarget is Procedure) {
      return IrCall(target, name, const []);
    }
    return IrField(target, name);
  }

  IrExpr _staticGet(StaticGet node) {
    final target = node.target;
    final enclosing = target.enclosingClass;
    if (enclosing == null) {
      // A top-level name. A `const` or `final` is a module constant in Rust
      // too; a computed `get foo => ...` is a function and stops here.
      if (target is Field && (target.isConst || target.isFinal)) {
        return IrTopLevel(target.name.text);
      }
      throw Unsupported('top-level `${target.name.text}`', _sample(node));
    }
    return IrStatic(enclosing.name, target.name.text,
        isEnumValue: enclosing.isEnum);
  }

  /// The place Kernel's desugaring has to be undone.
  ///
  /// Every operator is a method call here, so `a + b` arrives as
  /// `InstanceInvocation(a, '+', [b])`. Left alone it would emit `a.add(b)`,
  /// which is neither what upstream wrote nor what the Rust backend's operator
  /// traits expect. Turning it back into a binary expression is what makes the
  /// two front ends produce the same IR.
  IrExpr _instanceInvocation(InstanceInvocation node) {
    final name = node.name.text;
    // The callee is passed so omitted optional arguments get their defaults.
    // Without it `weigh()` came out as a no-argument call against a
    // three-parameter function -- the same bug the analyzer front end had in
    // round two, living on here because nothing compared the two front ends on
    // a fixture that used defaults.
    final args = _arguments(node.arguments, node.interfaceTarget.function);
    if (_binaryOperators.contains(name) && args.length == 1) {
      return IrBinary(name, expression(node.receiver), args.single);
    }
    if (name == 'unary-' && args.isEmpty) {
      return IrUnary('-', expression(node.receiver));
    }
    final receiver = node.receiver;
    return IrCall(
        receiver is ThisExpression ? null : expression(receiver), name, args);
  }

  IrExpr _construct(ConstructorInvocation node) {
    final target = node.target;
    final name = target.name.text;
    return IrNew(
      IrType(target.enclosingClass.name),
      _arguments(node.arguments, target.function),
      constructor: name.isEmpty ? null : name,
    );
  }

  IrExpr _staticInvocation(StaticInvocation node) {
    final target = node.target;
    final owner = target.enclosingClass?.name;
    if (owner == null) {
      throw Unsupported('top-level call `${target.name.text}`', _sample(node));
    }
    return IrStaticCall(
        owner, target.name.text, _arguments(node.arguments, target.function));
  }

  /// Arguments in the callee's declaration order.
  ///
  /// Kernel has already split them into positional and named, and a named one
  /// that was omitted is simply absent -- so the callee's own parameter list is
  /// still what decides the order, exactly as in the analyzer front end.
  List<IrExpr> _arguments(Arguments node, [FunctionNode? callee]) {
    final positional = [for (final a in node.positional) expression(a)];
    if (node.named.isEmpty && callee == null) return positional;
    if (callee == null) {
      throw Unsupported('named argument with no resolved callee', _sample(node));
    }
    final supplied = {for (final n in node.named) n.name: n.value};
    // Kernel names a named parameter through `parameterName`.
    final out = <IrExpr>[...positional];
    for (final param in callee.namedParameters) {
      final value = supplied.remove(param.parameterName);
      if (value != null) {
        out.add(expression(value));
        continue;
      }
      out.add(_omitted(param, node));
    }
    // A positional optional that was left off still needs its default.
    for (var i = positional.length;
        i < callee.positionalParameters.length;
        i++) {
      out.insert(i, _omitted(callee.positionalParameters[i], node));
    }
    if (supplied.isNotEmpty) {
      throw Unsupported(
          'named argument `${supplied.keys.first}` not in the callee',
          _sample(node));
    }
    return out;
  }

  IrExpr _omitted(FunctionParameter param, Node site) {
    final initializer = param.defaultValue;
    if (initializer != null) {
      // Kernel holds the default as an expression, already evaluated when it is
      // constant -- better than the analyzer front end, which could only read
      // the source text and accept the literals it recognised.
      return expression(initializer);
    }
    if (param.type.nullability == Nullability.nullable) {
      return const IrLiteral('null', IrType('Null', nullable: true));
    }
    throw Unsupported(
        'omitted parameter `${param.cosmeticName}` has no default',
        _sample(site));
  }

  IrExpr _constant(Constant constant, Expression node) {
    if (constant is DoubleConstant) {
      return IrLiteral('${constant.value}', const IrType('double'));
    }
    if (constant is IntConstant) {
      return IrLiteral('${constant.value}', const IrType('int'));
    }
    if (constant is BoolConstant) {
      return IrLiteral('${constant.value}', const IrType('bool'));
    }
    if (constant is StringConstant) {
      return IrLiteral(constant.value, const IrType('String'));
    }
    if (constant is NullConstant) {
      return const IrLiteral('null', IrType('Null', nullable: true));
    }
    if (constant is InstanceConstant) {
      // An enum value arrives as an instance of the enum class carrying the
      // CFE's own `#index` and `_name` fields. Walking its constructor for
      // those was 1125 refusals reading `const instance missing #index` -- a
      // bug in work reported finished two rounds ago, and one only the Kernel
      // census could see, because the analyzer front end never meets this
      // shape at all.
      if (constant.classNode.isEnum) {
        for (final entry in constant.fieldValues.entries) {
          if (entry.key.asField.name.text != '_name') continue;
          final value = entry.value;
          if (value is StringConstant) {
            return IrStatic(constant.classNode.name, value.value,
                isEnumValue: true);
          }
        }
        throw Unsupported(
            'enum constant with no `_name`', _sample(node));
      }
      // `const Alignment(-1, -1)` arrives already evaluated, as the class plus
      // its field values. Rebuilt as a constructor call so the emitted Rust
      // still reads as `Alignment::new(-1.0, -1.0)` rather than as a literal
      // struct -- the value is the same and the source stays recognisable.
      final cls = constant.classNode;
      final ctor = cls.constructors.where((c) => c.name.text.isEmpty).toList();
      if (ctor.length != 1) {
        throw Unsupported('const instance of `${cls.name}` with '
            '${ctor.length} unnamed constructors', _sample(node));
      }
      final byName = {
        for (final e in constant.fieldValues.entries)
          e.key.asField.name.text: e.value
      };
      // Positional **and** named, in that order, because that is the order
      // `_lowerConstructor` puts them in and the backend emits them
      // positionally. Walking only the positional ones emitted
      // `TextAlignVertical::new()` against a one-parameter constructor -- its
      // sole parameter is `{required this.y}`.
      final args = <IrExpr>[];
      final function = ctor.single.function;
      final names = [
        for (final p in function.positionalParameters) p.cosmeticName,
        for (final p in function.namedParameters) p.parameterName,
      ];
      for (final name in names) {
        final value = byName[name];
        if (value == null) {
          throw Unsupported('const instance missing `$name`', _sample(node));
        }
        args.add(_constant(value, node));
      }
      return IrNew(IrType(cls.name), args);
    }
    throw Unsupported('constant ${constant.runtimeType}', _sample(node));
  }

  // There was a `_refusePrivate` here. It is gone, and its going is the point
  // of this round: skipping private members is right when translating one file
  // at a time -- nothing outside the library can name them -- and wrong for a
  // whole program, because that is where the program keeps its implementation.
  // Every StatefulWidget in Flutter does its work in a private State class, and
  // so do most of the gallery's 689 classes. A compiler that skips them
  // translates the surface and none of the substance, and reports a low refusal
  // count for having looked at less.

  // -- Statements -------------------------------------------------------------

  IrStmt statement(Statement node) {
    if (node is ReturnStatement) {
      final value = node.expression;
      return IrReturn(value == null ? null : expression(value));
    }
    if (node is Block) {
      return IrBlock([for (final s in node.statements) statement(s)]);
    }
    if (node is IfStatement) {
      return IrIf(
        expression(node.condition),
        statement(node.then),
        node.otherwise == null ? null : statement(node.otherwise!),
      );
    }
    if (node is VariableStatement) {
      final variable = node.declaration.variable;
      final name = variable.cosmeticName;
      if (name == null || name.startsWith('#')) {
        // A synthetic temporary from the CFE's own lowering. Translating one
        // means translating the lowering it belongs to, which is a decision for
        // whichever construct produced it, not for this statement.
        throw Unsupported('synthetic variable', _sample(node));
      }
      final init = variable.initializer;
      return IrLocalDecl(
          name, _type(variable.type), init == null ? null : expression(init));
    }
    if (node is ExpressionStatement) {
      // An assignment is a statement here, not an expression. Dart's `x = 1`
      // has the value 1 and Rust's has the value `()`, so one used for its
      // value cannot be translated this way -- and is refused below rather
      // than silently losing the value.
      final value = node.expression;
      if (value is InstanceSet) {
        // A field on `this`, and a field rather than a setter. Kernel names the
        // target outright, so neither has to be inferred.
        // A write to the cascade's own binding: a local, so it needs a
        // mutable local rather than a mutable `self`.
        final receiver = value.receiver;
        if (_cascade != null &&
            receiver is VariableGet &&
            receiver.variable == _cascade) {
          if (value.interfaceTarget is! Field) {
            return IrSetter(const IrLocal(_cascadeName), value.name.text,
                expression(value.value));
          }
          return IrAssignField(value.name.text, expression(value.value),
              target: const IrLocal(_cascadeName));
        }
        if (value.receiver is! ThisExpression) {
          // Another object's *setter* is a call, which needs nothing from us
          // beyond a `&mut` receiver at the call site; another object's *field*
          // is a write through a reference, which does. So one is translated
          // and the other still stops.
          if (value.interfaceTarget is! Field) {
            return IrSetter(expression(value.receiver), value.name.text,
                expression(value.value));
          }
          throw Unsupported(
              'assignment to a field of another object', _sample(value));
        }
        if (value.interfaceTarget is! Field) {
          return IrSetter(null, value.name.text, expression(value.value));
        }
        return IrAssignField(value.name.text, expression(value.value));
      }
      if (value is VariableSet) {
        final name = value.variable.cosmeticName;
        if (name == null || name.startsWith('#')) {
          throw Unsupported('assignment to a synthetic variable',
              _sample(value));
        }
        return IrAssign(name, expression(value.value));
      }
      return IrExprStmt(expression(value));
    }
    if (node is AssertStatement) {
      return _assert(node.condition, node.message);
    }
    if (node is AssertBlock) {
      return IrBlock([for (final s in node.statements) statement(s)]);
    }
    if (node is EmptyStatement) return const IrBlock([]);
    throw Unsupported('statement ${node.runtimeType}', _sample(node));
  }

  IrAssert _assert(Expression condition, Expression? message) {
    if (message is StringLiteral) {
      return IrAssert(expression(condition), literalMessage: message.value);
    }
    return IrAssert(expression(condition),
        message: message == null ? null : _sample(message));
  }

  IrStmt _body(FunctionNode function) {
    final body = function.body;
    if (body == null) throw Unsupported('no body', function.toString());
    return statement(body);
  }

  // -- Declarations -----------------------------------------------------------

  (IrLibrary, List<String>) lowerLibrary() {
    final classes = <IrClass>[];
    final constants = <IrConstDecl>[];
    final refused = <String>[];
    for (final field in library.fields) {
      if (!field.isConst && !field.isFinal) continue;
      final init = field.initializer;
      if (init == null) continue;
      try {
        constants.add(IrConstDecl(
            field.name.text, _type(field.type), expression(init)));
      } on Unsupported catch (error) {
        refused.add('top-level ${field.name.text}: $error');
      }
    }
    for (final cls in library.classes) {
      // Anonymous mixin applications stay skipped: they are the CFE's own
      // synthetic classes, not something upstream wrote. Private classes do
      // not -- see the note where `_refusePrivate` used to be.
      if (cls.isAnonymousMixin) continue;
      final (lowered, problems) = lowerClass(cls);
      classes.add(lowered);
      refused.addAll(problems.map((p) => '${cls.name}: $p'));
    }
    return (IrLibrary(classes, constants: constants), refused);
  }

  (IrClass, List<String>) lowerClass(Class node) {
    // Kernel's superclass may be a synthetic mixin application; the class a
    // reader would name is the first one above that is not.
    var base = node.superclass;
    while (base != null && base.isAnonymousMixin) {
      base = base.superclass;
    }
    // An enum's values are its static const fields, in declaration order,
    // minus the synthetic `values` list the CFE adds.
    //
    // An **enhanced** enum carries none, on purpose. It is a Rust enum plus an
    // impl, and emitting it as a plain one drops its methods -- which is what
    // the analyzer front end refuses and what this one was quietly doing, since
    // round fourteen's test only ever read the analyzer's output. The fixture
    // comparison is what found it.
    const implicitEnumMembers = {
      'index', 'values', '_name', 'toString', 'hashCode', '==', 'name',
      '_enumToString', 'compareTo',
    };
    final enhanced = node.isEnum &&
        (node.procedures.any((p) =>
                !implicitEnumMembers.contains(p.name.text) && !p.isSynthetic) ||
            node.fields.any((f) =>
                !f.isStatic && !implicitEnumMembers.contains(f.name.text)));
    final values = node.isEnum && !enhanced
        ? [
            for (final f in node.fields)
              if (f.isStatic && f.isConst && f.name.text != 'values')
                f.name.text
          ]
        : const <String>[];
    final cls = IrClass(
      node.name,
      superclass: node.isEnum || base == null || base.name == 'Object'
          ? null
          : base.name,
      isAbstract: node.isAbstract,
      isEnum: node.isEnum,
      values: values,
    );
    _superclass = cls.superclass;
    final refused = <String>[];

    for (final field in node.fields) {
      try {
        _lowerField(cls, field);
      } on Unsupported catch (error) {
        refused.add('$error');
      }
    }
    for (final ctor in node.constructors) {
      try {
        _lowerConstructor(cls, ctor);
      } on Unsupported catch (error) {
        refused.add('$error');
      }
    }
    for (final procedure in node.procedures) {
      try {
        _lowerProcedure(cls, procedure);
      } on Unsupported catch (error) {
        refused.add('$error');
      }
    }
    return (cls, refused);
  }

  void _lowerField(IrClass cls, Field field) {
    final name = field.name.text;
    // An enum's own members are its variants and the CFE's bookkeeping; neither
    // becomes a field or a constant on the Rust side.
    if (cls.isEnum) return;
    if (field.isStatic) {
      if (!field.isConst) {
        throw Unsupported('non-const static field', name);
      }
      final init = field.initializer;
      if (init == null) throw Unsupported('const without initialiser', name);
      cls.constants
          .add(IrConstDecl(name, _type(field.type), expression(init)));
    } else {
      final initial = field.initializer;
      cls.fields.add(IrFieldDecl(name, _type(field.type),
          isFinal: field.isFinal,
          initial: initial == null ? null : expression(initial)));
    }
  }

  void _lowerConstructor(IrClass cls, Constructor node) {
    if (cls.isEnum) return;
    final name = node.name.text;
    final params = <IrParam>[];
    for (final p in node.function.positionalParameters) {
      params.add(IrParam(p.cosmeticName ?? '_', _type(p.type)));
    }
    for (final p in node.function.namedParameters) {
      params.add(IrParam(p.parameterName, _type(p.type), named: true));
    }

    final inits = <String, IrExpr>{};
    final asserts = <IrAssert>[];
    String? superBase;
    var superArgs = const <IrExpr>[];
    for (final init in node.initializers) {
      if (init is FieldInitializer) {
        inits[init.field.name.text] = expression(init.value);
      } else if (init is AssertInitializer) {
        final statement = init.statement;
        asserts.add(_assert(statement.condition, statement.message));
      } else if (init is SuperInitializer) {
        if (init.arguments.positional.isNotEmpty ||
            init.arguments.named.isNotEmpty) {
          var base = node.enclosingClass.superclass;
          while (base != null && base.isAnonymousMixin) {
            base = base.superclass;
          }
          if (base == null) {
            throw Unsupported('super constructor call with no base',
                _sample(init));
          }
          superBase = base.name;
          superArgs = _arguments(init.arguments, init.target.function);
        }
        // A no-argument super() adds nothing to a Rust struct literal.
      } else if (init is RedirectingInitializer) {
        throw Unsupported('redirecting constructor', _sample(init));
      } else {
        throw Unsupported('initialiser ${init.runtimeType}', _sample(init));
      }
    }
    // A constructor *body* is not lowered, and dropping it silently would emit
    // a constructor that ignores its arguments -- `Tinted(v) { opacity = v; }`
    // came out setting the declaration's default instead. Refusing says so.
    final body = node.function.body;
    if (body != null && !(body is Block && body.statements.isEmpty)) {
      throw Unsupported('constructor with a body', _sample(node));
    }
    cls.constructors.add(IrConstructor(
      params,
      inits,
      isConst: node.isConst,
      name: name.isEmpty ? null : name,
      asserts: asserts,
      superBase: superBase,
      superArgs: superArgs,
    ));
  }

  void _lowerProcedure(IrClass cls, Procedure node) {
    final name = node.name.text;
    if (cls.isEnum) {
      // A plain enum has only the implicit members. One with anything else is
      // an enhanced enum -- a Rust enum plus an impl -- and stops here rather
      // than being emitted as a plain one with its methods quietly missing.
      const implicit = {
        'index', 'values', '_name', 'toString', 'hashCode', '==', 'name',
        '_enumToString', 'compareTo',
      };
      if (!implicit.contains(name) && !node.isSynthetic) {
        throw Unsupported('enhanced enum member `$name`', cls.name);
      }
      return;
    }
    if (node.isStatic && node.kind == ProcedureKind.Factory) {
      throw Unsupported('factory constructor', name);
    }


    final params = [
      for (final p in node.function.positionalParameters)
        IrParam(p.cosmeticName ?? '_', _type(p.type)),
      for (final p in node.function.namedParameters)
        IrParam(p.parameterName, _type(p.type), named: true),
    ];
    final isOperator = node.kind == ProcedureKind.Operator;
    final method = IrMethod(
      name,
      params,
      _type(node.function.returnType),
      node.isAbstract ? const IrBlock([]) : _body(node.function),
      isStatic: node.isStatic,
      isGetter: node.kind == ProcedureKind.Getter,
      isSetter: node.kind == ProcedureKind.Setter,
      operator: isOperator ? name : null,
    );
    (node.isAbstract ? cls.abstractMethods : cls.methods).add(method);
  }
}

/// Whether a function body mentions `this` anywhere inside it.
class _ThisFinder extends RecursiveVisitor {
  bool found = false;

  @override
  void visitThisExpression(ThisExpression node) {
    found = true;
    super.visitThisExpression(node);
  }
}
