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
  '+',
  '-',
  '*',
  '/',
  '~/',
  '%',
  '==',
  '<',
  '>',
  '<=',
  '>=',
  '&',
  '|',
  '^',
  '<<',
  '>>',
  '>>>',
};

class KernelFrontend {
  KernelFrontend(
    this.library, {
    this.enumValues = const {},
    this.abstractElsewhere = const {},
  });

  /// Abstract classes in the rest of the crate. See [IrLibrary].
  final Set<String> abstractElsewhere;

  final Library library;

  /// An enum class to its variant names, in `index` order.
  ///
  /// Empty by default, and then an enum whose fields the compiler dropped
  /// comes out with no values -- which the backend says plainly rather than
  /// emitting an enum with no variants. See `enumValuesIn`.
  final Map<Class, List<String>> enumValues;
  String? _superclass;

  // -- Types ------------------------------------------------------------------

  IrType _type(DartType type) {
    final nullable = type.nullability == Nullability.nullable;
    if (type is InterfaceType) {
      return IrType(
        type.classNode.name,
        nullable: nullable,
        arguments: [for (final a in type.typeArguments) _type(a)],
      );
    }
    if (type is RecordType) {
      if (type.named.isNotEmpty) {
        throw Unsupported('a record type with named fields', '$type');
      }
      return IrType(
        'Record',
        nullable: nullable,
        arguments: [for (final f in type.positional) _type(f)],
      );
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
      // A temporary this lowering has already named. Asking the map rather
      // than the variable's own name is what makes two nested `#0`s two
      // different locals instead of one.
      final temporary = _temporaries[node.variable];
      if (temporary != null) return IrLocal(temporary);
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
      return IrCallValue(expression(node.receiver), _arguments(node.arguments));
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
      return IrBinary(
        node.operatorEnum == LogicalExpressionOperator.AND ? '&&' : '||',
        expression(node.left),
        expression(node.right),
      );
    }
    if (node is ConditionalExpression) {
      return IrConditional(
        expression(node.condition),
        expression(node.then),
        expression(node.otherwise),
      );
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
      // `x = v` used for its value. Rust's assignment produces `()`, so the
      // value is bound, assigned and produced -- the same shape a field write
      // used for its value takes.
      final written = node.variable.cosmeticName;
      final known = _temporaries[node.variable];
      if (known == null && (written == null || written.startsWith('#'))) {
        throw Unsupported('assignment used for its value', _sample(node));
      }
      return IrAssignValue(known ?? written!, expression(node.value));
    }
    if (node is RecordIndexGet) {
      // `r.$1` in Dart is `r.0` in Rust -- Dart counts its positional record
      // fields from one and Rust counts tuple fields from zero.
      return IrRecordField(expression(node.receiver), node.index);
    }
    if (node is RecordLiteral) {
      if (node.named.isNotEmpty) {
        throw Unsupported('a record with named fields', _sample(node));
      }
      return IrRecord([for (final e in node.positional) expression(e)]);
    }
    if (node is MapLiteral) {
      return IrMapLiteral(
        [
          for (final entry in node.entries)
            (expression(entry.key), expression(entry.value)),
        ],
        _type(node.keyType),
        _type(node.valueType),
      );
    }
    if (node is ListLiteral) {
      return IrListLiteral([
        for (final e in node.expressions) expression(e),
      ], _type(node.typeArgument));
    }
    if (node is StringConcatenation) {
      return IrInterpolation([for (final e in node.expressions) expression(e)]);
    }
    if (node is SuperPropertyGet) {
      final owner = node.interfaceTarget?.enclosingClass?.name;
      if (owner == null) {
        throw Unsupported('super property with no owner', _sample(node));
      }
      if (node.interfaceTarget is Field) {
        // A base field is copied into the subclass struct by the flattening,
        // so `super.x` and `this.x` are the same storage.
        return IrField(null, node.name.text);
      }
      return IrSuperCall(owner, node.name.text, const []);
    }
    if (node is Throw) {
      // `a ?? throw StateError(..)`. Rust has no throw, but it does have an
      // expression that never produces a value: `return Err(e)` has type `!`,
      // which fits wherever a value was wanted. So the expression form is the
      // statement form, written where the value would have gone.
      return IrThrowValue(expression(node.expression));
    }
    if (node is InstanceSet) {
      // `a.b = v` where the value is wanted. Only a field on `this`: a setter
      // returns nothing to produce, and another object's field is the `&mut`
      // through a reference this compiler still refuses as a statement.
      if (node.receiver is! ThisExpression) {
        throw Unsupported(
          'assignment to another object used for its value',
          _sample(node),
        );
      }
      if (node.interfaceTarget is! Field) {
        throw Unsupported('setter call used for its value', _sample(node));
      }
      return IrSetValue(null, node.name.text, expression(node.value));
    }
    if (node is NullCheck) return IrNullCheck(expression(node.operand));
    if (node is AsExpression) return expression(node.operand);
    if (node is StaticTearOff) {
      return IrFunctionRef(
        node.target.enclosingClass?.name,
        node.target.name.text,
      );
    }
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
    final bound = first is VariableStatement
        ? first.declaration.variable
        : null;
    final initial = bound?.initializer;
    if (bound == null ||
        initial == null ||
        !(value is VariableGet && value.variable == bound)) {
      // Not the cascade shape. It is still a block with a value, which is what
      // Rust's block expression is, so it needs no shape recognised -- the same
      // floor the general `Let` put under the three `Let` shapes.
      return IrBlockValue([
        for (final s in statements) statement(s),
      ], expression(value));
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
    if (body == null)
      throw Unsupported('closure with no body', _sample(origin));
    return IrClosure(
      [
        for (final p in fn.positionalParameters)
          IrParam(p.cosmeticName ?? '_', _type(p.type)),
      ],
      statement(body),
      _type(fn.returnType),
    );
  }

  /// Whether an expression is `this`, or a chain of field reads from it.
  bool _rootedAtThis(Expression e) => switch (e) {
    ThisExpression() => true,
    InstanceGet(:final receiver) => _rootedAtThis(receiver),
    _ => false,
  };

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
    if (body is BlockExpression && _isThe(body.value, node.variable)) {
      final initial = node.variable.initializer;
      if (initial == null) {
        throw Unsupported('cascade binding with no receiver', _sample(node));
      }
      final previous = _cascade;
      _cascade = node.variable;
      try {
        return IrBlockValue([
          IrLocalDecl(
            _cascadeName,
            _type(node.variable.type),
            expression(initial),
          ),
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
    // Everything else is what a `Let` says it is: bind a name, then evaluate
    // the body with it in scope. Rust spells that a block expression, and it
    // needs no pattern recognised at all.
    //
    // The three shapes above are still tried first because they read like the
    // Dart that produced them and keep the two front ends agreeing. This is the
    // floor under them: 14476 `Let`s in `package:flutter/` are not any of the
    // three, and the largest group is simply the CFE binding a temporary for a
    // named argument -- `let #0 = radius * 2 in new CustomPaint(.., #0, ..)`.
    final initial = node.variable.initializer;
    if (initial == null) {
      // A `Let` with nothing to bind. Its body may still read the variable, and
      // there would be nothing to read.
      throw Unsupported('CFE `Let` with no initialiser', _sample(node));
    }
    final name = _nameFor(node.variable);
    return IrBlockValue([
      IrLocalDecl(name, _type(node.variable.type), expression(initial)),
    ], expression(node.body));
  }

  /// One local declaration, wherever it is written.
  ///
  /// A `for`'s variables are `VariableDeclaration`s and not `Statement`s in
  /// this Kernel, so they cannot go through `statement` -- and the rule about
  /// what a declaration becomes should be in one place regardless.
  IrStmt _declare(Variable variable, Node at) {
    final init = variable.initializer;
    if (init is InstanceGet && init.name.text == 'iterator') {
      // Remembered, not lowered: if the loop below it is the CFE's `for-in`,
      // this binding is part of that shape and the restored loop names the
      // iterable itself.
      _iterators[variable] = init.receiver;
      return const IrBlock([]);
    }
    final written = variable.cosmeticName;
    // A temporary the CFE invented. It used to be refused, on the grounds that
    // translating one means translating the lowering it belongs to -- but that
    // was only true while there was nothing to call it. `_nameFor` gives it a
    // name, `VariableGet` finds that name again, and the lowering it belongs to
    // is then just the statements around it.
    final name = (written == null || written.startsWith('#'))
        ? _nameFor(variable)
        : written;
    return IrLocalDecl(
      name,
      _type(variable.type),
      init == null ? null : expression(init),
    );
  }

  /// A name for one of the CFE's temporaries.
  ///
  /// They are called `#0`, `#1` and so on, and the numbering restarts, so two
  /// nested `Let`s can both be `#0`. Rust would take the inner one as shadowing
  /// the outer, which is what Dart means too -- but the backend snakes names,
  /// and `#` is not a character it can carry. So each variable gets its own
  /// name, kept in a map by identity rather than by text.
  final _temporaries = <Variable, String>{};
  var _nextTemporary = 0;

  String _nameFor(Variable variable) =>
      _temporaries[variable] ??= '__t${_nextTemporary++}';

  /// A Rust label for one of Kernel's labelled statements.
  ///
  /// Kept by identity, like the temporaries, because a labelled statement has
  /// no name of its own -- a `break` points at the node.
  final _labels = <LabeledStatement, String>{};
  var _nextLabel = 0;

  String _labelFor(LabeledStatement node) =>
      _labels[node] ??= '__l${_nextLabel++}';

  /// Labels the CFE put there to spell `continue` and `break`.
  ///
  /// `continue` is a `break` out of a label wrapped around the loop *body*, and
  /// `break` is a `break` out of one wrapped around the loop itself. Both are
  /// the CFE saying in its own words something Dart already had a word for, and
  /// the analyzer front end sees the word -- so these are restored rather than
  /// carried across as labelled blocks.
  final _continueTargets = <LabeledStatement>{};

  /// Labels a switch's `break` points at, and the breaks that may be dropped --
  /// the last statement of a case body, and only that one.
  final _switchBreaks = <LabeledStatement>{};
  final _droppableBreaks = <BreakStatement>{};

  /// A case body, with its trailing `break` marked as droppable.
  IrStmt _caseBody(Statement body) {
    final last = body is Block && body.statements.isNotEmpty
        ? body.statements.last
        : body;
    if (last is BreakStatement) _droppableBreaks.add(last);
    return statement(body);
  }

  /// Labelled statements a `break` should leave, and the Rust label to use --
  /// null when a bare `break` will do.
  final _breakTargets = <LabeledStatement, String?>{};

  /// The label to put on the loop about to be lowered, if it needs one.
  String? _loopLabel;

  /// `for (final x in xs)`, put back together.
  ///
  /// The CFE writes it as: bind `#0 = xs.iterator`, then `for (; #0.moveNext();)`
  /// with `final x = #0.current` as the body's first statement. The binding is
  /// a *sibling* of the loop, so it is spotted here from the loop's condition
  /// and the loop's body, and the binding statement is dropped by the block
  /// that holds it. Returns null when the shape is anything else.
  IrStmt? _forIn(ForStatement node) {
    if (node.variables.isNotEmpty || node.updates.isNotEmpty) return null;
    final condition = node.condition;
    if (condition is! InstanceInvocation || condition.name.text != 'moveNext') {
      return null;
    }
    final receiver = condition.receiver;
    if (receiver is! VariableGet) return null;
    final iterable = _iterators[receiver.variable];
    if (iterable == null) return null;

    var body = node.body;
    if (body is LabeledStatement) body = body.body;
    if (body is! Block || body.statements.isEmpty) return null;
    final first = body.statements.first;
    if (first is! VariableStatement) return null;
    final initial = first.declaration.variable.initializer;
    if (initial is! InstanceGet ||
        initial.name.text != 'current' ||
        !(initial.receiver is VariableGet &&
            identical(
              (initial.receiver as VariableGet).variable,
              receiver.variable,
            ))) {
      return null;
    }
    final name = first.declaration.variable.cosmeticName;
    if (name == null || name.startsWith('#')) return null;
    _iteratorLoops.add(receiver.variable);
    return IrForIn(
      name,
      expression(iterable),
      IrBlock([for (final s in body.statements.skip(1)) statement(s)]),
    );
  }

  /// Temporaries bound to `<something>.iterator`, and the ones a restored
  /// `for-in` has consumed -- whose binding statement must then not be emitted.
  final _iterators = <Variable, Expression>{};
  final _iteratorLoops = <Variable>{};

  IrStmt _loopBody(Statement body, bool hasUpdates) {
    if (body is! LabeledStatement) return statement(body);
    if (hasUpdates) {
      return IrLabeled(_labelFor(body), statement(body.body));
    }
    _continueTargets.add(body);
    return statement(body.body);
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
    final listOwner = node.interfaceTarget.enclosingClass?.name;
    if (listOwner == 'List' || listOwner == 'Iterable') {
      final rust = listMethodNames[name];
      if (rust == null) throw Unsupported('`List.$name`', _sample(node));
      // A getter in Dart, a method in Rust: `xs.length` is `xs.len()`.
      return IrCall(expression(node.receiver), rust, const []);
    }
    if (listOwner == 'Map') {
      if (orderedMapMembers.contains(name)) {
        throw Unsupported(
          '`Map.$name`, which depends on insertion order',
          _sample(node),
        );
      }
      final rust = mapMethodNames[name];
      if (rust == null) throw Unsupported('`Map.$name`', _sample(node));
      return IrCall(expression(node.receiver), rust, const []);
    }
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
    return IrStatic(
      enclosing.name,
      target.name.text,
      isEnumValue: enclosing.isEnum,
    );
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
    final owner = node.interfaceTarget.enclosingClass?.name;
    if (owner == 'List' || owner == 'Iterable') {
      if (name == '[]' && args.length == 1) {
        return IrIndex(expression(node.receiver), args.single);
      }
      final step = iterStepNames[name];
      if (step != null && args.length == 1) {
        // A chain, extended rather than started again when the receiver is
        // already one: `xs.where(f).map(g)` is one `iter()`, not two.
        final source = expression(node.receiver);
        return source is IrIterChain
            ? IrIterChain(source.source, [...source.steps, (step, args.single)])
            : IrIterChain(source, [(step, args.single)]);
      }
      final rust = listMethodNames[name];
      if (rust != null) {
        return IrCall(expression(node.receiver), rust, args);
      }
      throw Unsupported('`List.$name`', _sample(node));
    }
    if (owner == 'Map') {
      if (name == '[]' && args.length == 1) {
        return IrCall(expression(node.receiver), 'get', args);
      }
      if (orderedMapMembers.contains(name)) {
        throw Unsupported(
          '`Map.$name`, which depends on insertion order',
          _sample(node),
        );
      }
      final rust = mapMethodNames[name];
      if (rust == null) throw Unsupported('`Map.$name`', _sample(node));
      return IrCall(expression(node.receiver), rust, args);
    }
    if (_binaryOperators.contains(name) && args.length == 1) {
      return IrBinary(
        name,
        expression(node.receiver),
        args.single,
        // The invocation's own function type says what the operator returns.
        // `getStaticType` would need a StaticTypeContext this lowering does
        // not build, and the function type is already here.
        type: node.functionType == null
            ? null
            : _type(node.functionType!.returnType),
      );
    }
    if (name == 'unary-' && args.isEmpty) {
      return IrUnary('-', expression(node.receiver));
    }
    final receiver = node.receiver;
    return IrCall(
      receiver is ThisExpression ? null : expression(receiver),
      name,
      args,
    );
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
    final positional = node.arguments.positional;
    // Two of dart:math's, and one of Flutter's own. Rust has all three, and
    // `max` is the same spelling for floats and integers because `f32::max` is
    // inherent and `Ord::max` covers the rest. 372 `max` and 184 `clampDouble`.
    const arithmetic = {'max': 'max', 'min': 'min'};
    final rust = arithmetic[target.name.text];
    if (rust != null && positional.length == 2) {
      return IrCall(expression(positional[0]), rust, [
        expression(positional[1]),
      ]);
    }
    // The CFE lowers `<int>[3, 11, 29]` to `_GrowableList._literal3(..)`, so a
    // list literal never reaches this compiler as a ListLiteral. Restored
    // rather than transliterated, for the same reason `??` and cascades are:
    // the analyzer front end sees the literal, and the two have to agree.
    final owner = target.enclosingClass?.name;
    if ((owner == '_GrowableList' || owner == '_List') &&
        target.name.text.startsWith('_literal')) {
      return IrListLiteral([
        for (final e in positional) expression(e),
      ], _type(node.arguments.types.singleOrNull ?? const DynamicType()));
    }
    if (target.name.text == 'lerpDouble' && positional.length == 3) {
      // `a + (b - a) * t`, which is what dart:ui's lerpDouble computes for
      // non-null arguments. 67 calls.
      final a = expression(positional[0]);
      final b = expression(positional[1]);
      return IrBinary(
        '+',
        a,
        IrBinary('*', IrBinary('-', b, a), expression(positional[2])),
      );
    }
    if (target.name.text == 'pow' && positional.length == 2) {
      return IrCall(expression(positional[0]), 'powf', [
        expression(positional[1]),
      ]);
    }
    if (target.name.text == 'clampDouble' && positional.length == 3) {
      return IrCall(expression(positional[0]), 'clamp', [
        expression(positional[1]),
        expression(positional[2]),
      ]);
    }
    if (target.name.text == 'unsafeCast' && positional.length == 1) {
      // The CFE's own cast, inserted where it has already proved the type. It
      // does nothing at runtime and there is nothing for it to do here either.
      return expression(positional.single);
    }
    if (target.name.text == 'identical' && positional.length == 2) {
      return IrIdentical(expression(positional[0]), expression(positional[1]));
    }
    if (owner == null) {
      // A top-level function of *this* library. One from elsewhere is still
      // refused: the backend checks the name against what this file emits, and
      // a call to something outside it would name a function nobody wrote.
      if (target.enclosingLibrary != library) {
        throw Unsupported(
          'top-level call `${target.name.text}`',
          _sample(node),
        );
      }
      return IrStaticCall(
        null,
        target.name.text,
        _arguments(node.arguments, target.function),
      );
    }
    return IrStaticCall(
      owner,
      target.name.text,
      _arguments(node.arguments, target.function),
    );
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
      throw Unsupported(
        'named argument with no resolved callee',
        _sample(node),
      );
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
    for (
      var i = positional.length;
      i < callee.positionalParameters.length;
      i++
    ) {
      out.insert(i, _omitted(callee.positionalParameters[i], node));
    }
    if (supplied.isNotEmpty) {
      throw Unsupported(
        'named argument `${supplied.keys.first}` not in the callee',
        _sample(node),
      );
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
      _sample(site),
    );
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
    if (constant is ListConstant) {
      return IrListLiteral([
        for (final e in constant.entries) _constant(e, node),
      ], _type(constant.typeArgument));
    }
    if (constant is MapConstant) {
      return IrMapLiteral(
        [
          for (final e in constant.entries)
            (_constant(e.key, node), _constant(e.value, node)),
        ],
        _type(constant.keyType),
        _type(constant.valueType),
      );
    }
    if (constant is StaticTearOffConstant) {
      // A top-level or static function used as a value. Rust names the
      // function; nothing is captured, so none of the ownership question that
      // an *instance* tear-off raises applies here.
      return IrFunctionRef(
        constant.target.enclosingClass?.name,
        constant.target.name.text,
      );
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
            return IrStatic(
              constant.classNode.name,
              value.value,
              isEnumValue: true,
            );
          }
        }
        throw Unsupported('enum constant with no `_name`', _sample(node));
      }
      // `const Alignment(-1, -1)` arrives already evaluated, as the class plus
      // its field values. Rebuilding it as `Alignment::new(-1.0, -1.0)` reads
      // like the source and keeps the two front ends saying the same thing, so
      // it is still what happens when it can.
      //
      // It often cannot. A `const` instance never calls its constructor -- the
      // value is materialised -- so the constructor is unreachable and gets
      // shaken out of the dill: `_Linear` in curves.dart has none left at all,
      // and 2965 of `package:flutter`'s 5602 const instances are like it. Four
      // more shapes defeat the name matching even when a constructor survives:
      // a field renamed by a super constructor (`Offset(dx, dy)` stores `_dx`),
      // a redirect (`Duration`, `Color`), a class with only named constructors
      // (`EdgeInsets`), and the inspector's injected `$creationLocation`.
      //
      // So the constructor is an optimisation, and the field values are the
      // answer. They are what an InstanceConstant always carries, and they are
      // already the computed values -- there is nothing left for a constructor
      // to work out.
      final cls = constant.classNode;
      final byName = {
        for (final e in constant.fieldValues.entries)
          e.key.asField.name.text: e.value,
      };
      final rebuilt = _asConstructorCall(
        cls,
        byName,
        node,
        constant.typeArguments,
      );
      if (rebuilt != null) return rebuilt;
      return IrConstInstance(IrType(cls.name), {
        for (final entry in byName.entries)
          entry.key: _constant(entry.value, node),
      });
    }
    throw Unsupported('constant ${constant.runtimeType}', _sample(node));
  }

  /// `Alignment::new(-1.0, -1.0)`, when the constructor is still there and its
  /// parameters name the fields one for one. Null when it is not.
  IrNew? _asConstructorCall(
    Class cls,
    Map<String, Constant> byName,
    Expression node,
    List<DartType> typeArguments,
  ) {
    final ctor = cls.constructors.where((c) => c.name.text.isEmpty).toList();
    if (ctor.length != 1) return null;
    // Positional **and** named, in that order, because that is the order
    // `_lowerConstructor` puts them in and the backend emits them
    // positionally. Walking only the positional ones emitted
    // `TextAlignVertical::new()` against a one-parameter constructor -- its
    // sole parameter is `{required this.y}`.
    final function = ctor.single.function;
    final names = [
      for (final p in function.positionalParameters) p.cosmeticName,
      for (final p in function.namedParameters) p.parameterName,
    ];
    // Every parameter has to name a field *and* every field has to be named by
    // a parameter. Without the second half, a constructor that sets a field in
    // its initialiser list would be called without that field's value and the
    // instance would silently be a different one.

    final args = <IrExpr>[];
    for (final name in names) {
      final value = byName[name];
      if (value == null) return null;
      args.add(_constant(value, node));
    }
    return IrNew(_constantType(cls, typeArguments), args);
  }

  /// The type of a rebuilt constant, type arguments and all.
  ///
  /// Dropped, `const Pair<int, double>(3, 4.5)` came out as `Pair::new(..)`
  /// against the analyzer front end's `Pair::<i64, f32>::new(..)`. Both are
  /// valid Rust -- inference would have got there -- but the two front ends
  /// saying different things is the one thing the fixtures exist to catch.
  IrType _constantType(Class cls, List<DartType> typeArguments) =>
      IrType(cls.name, arguments: [for (final t in typeArguments) _type(t)]);

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
      return _declare(node.declaration.variable, node);
    }
    if (node is ExpressionStatement && node.expression is Throw) {
      // Before the general `ExpressionStatement` case below, not after: the
      // general one lowers the expression, and a `throw` has no value to lower.
      // Placed after, this check never ran and every throwing method was
      // refused -- which the fixture comparison found at once, because the
      // analyzer front end had it right.
      return IrThrow(expression((node.expression as Throw).expression));
    }
    if (node is ExpressionStatement) {
      // An assignment is a statement here, not an expression. Dart's `x = 1`
      // has the value 1 and Rust's has the value `()`, so one used for its
      // value cannot be translated this way -- and is refused below rather
      // than silently losing the value.
      final value = node.expression;
      if (value is InstanceInvocation &&
          value.name.text == '[]=' &&
          value.interfaceTarget.enclosingClass?.name == 'Map' &&
          value.arguments.positional.length == 2) {
        return IrExprStmt(
          IrCall(expression(value.receiver), 'insert', [
            expression(value.arguments.positional[0]),
            expression(value.arguments.positional[1]),
          ]),
        );
      }
      if (value is InstanceInvocation &&
          value.name.text == '[]=' &&
          value.interfaceTarget.enclosingClass?.name == 'List' &&
          value.arguments.positional.length == 2) {
        return IrIndexSet(
          expression(value.receiver),
          expression(value.arguments.positional[0]),
          expression(value.arguments.positional[1]),
        );
      }
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
            return IrSetter(
              const IrLocal(_cascadeName),
              value.name.text,
              expression(value.value),
            );
          }
          return IrAssignField(
            value.name.text,
            expression(value.value),
            target: const IrLocal(_cascadeName),
          );
        }
        if (value.receiver is! ThisExpression) {
          // Another object's *setter* is a call, which needs nothing from us
          // beyond a `&mut` receiver at the call site.
          if (value.interfaceTarget is! Field) {
            return IrSetter(
              expression(value.receiver),
              value.name.text,
              expression(value.value),
            );
          }
          // A *field* is a write through a reference. Through a chain rooted
          // at `this` -- `this.child.x = v` -- that reference is `self`, and
          // `&mut self` is a thing this compiler already works out. Through a
          // parameter it would mean `&mut` on the parameter and on every call
          // site, including ones in other files, so that one still stops.
          if (!_rootedAtThis(value.receiver)) {
            throw Unsupported(
              'assignment to a field of another object',
              _sample(value),
            );
          }
          return IrAssignField(
            value.name.text,
            expression(value.value),
            target: expression(value.receiver),
          );
        }
        if (value.interfaceTarget is! Field) {
          return IrSetter(null, value.name.text, expression(value.value));
        }
        return IrAssignField(value.name.text, expression(value.value));
      }
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
        return IrAssign(known ?? written!, expression(value.value));
      }
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
        // nothing -- but only the one at the *end* of a case. One in the
        // middle would be leaving the switch early, which a match arm cannot
        // do, so it is refused rather than dropped.
        _switchBreaks.add(node);
        return statement(body);
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
        if (!_droppableBreaks.contains(node)) {
          throw Unsupported(
            'break out of a switch from inside a case',
            _sample(node),
          );
        }
        return const IrBlock([]);
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
      final name = node.variable.cosmeticName;
      if (name == null || name.startsWith('#')) {
        throw Unsupported('local function with no name', _sample(node));
      }
      return IrLocalFunction(name, _closure(node.function, node) as IrClosure);
    }
    if (node is SwitchStatement) {
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
        cases.add(IrCase([for (final e in c.expressions) expression(e)], body));
      }
      return IrSwitch(expression(node.expression), cases, otherwise);
    }
    if (node is WhileStatement) {
      // No updates, so a `continue` really is Rust's `continue`.
      return IrWhile(expression(node.condition), _loopBody(node.body, false));
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
              ? const IrLiteral('true', IrType('bool'))
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

  /// `try { .. } catch (e) { .. }`, when there is one clause.
  ///
  /// Two clauses is two type tests, and only two `try`s in the corpus have
  /// them; the general answer waits for a reason to exist.
  IrStmt _tryCatch(TryCatch node) {
    if (node.catches.length != 1) {
      throw Unsupported(
        'try with ${node.catches.length} catch clauses',
        _sample(node),
      );
    }
    final clause = node.catches.single;
    final error = clause.exception?.cosmeticName ?? 'error';
    final stack = clause.stackTrace;
    if (stack != null && _reads(clause.body, stack)) {
      // A `Result` carries no stack trace. Binding one and ignoring it costs
      // nothing; reading one cannot be honoured, so it stops here rather than
      // being handed an empty stack that looks like a real one.
      throw Unsupported('catch reading its stack trace', _sample(node));
    }
    final guard = clause.guard;
    return IrTryCatch(
      statement(node.body),
      error,
      statement(clause.body),
      errorType: guard is InterfaceType && guard.classNode.name != 'Object'
          ? guard.classNode.name
          : null,
      stack: stack?.cosmeticName,
    );
  }

  bool _returnsEarly(Statement body) {
    final finder = _EarlyExit();
    body.accept(finder);
    return finder.found;
  }

  bool _reads(Statement body, Variable variable) {
    final finder = _VariableReader(variable);
    body.accept(finder);
    return finder.found;
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
        constants.add(
          IrConstDecl(field.name.text, _type(field.type), expression(init)),
        );
      } on Unsupported catch (error) {
        refused.add('top-level ${field.name.text}: $error');
      }
    }
    final functions = <IrMethod>[];
    for (final procedure in library.procedures) {
      // `BaselineOffset|+` and friends are the CFE's lowering of an extension
      // type's members into top-level functions. They are not what upstream
      // wrote and they are not translated as though they were.
      if (procedure.name.text.contains('|')) {
        refused.add(
          'top-level ${procedure.name.text}: '
          'an extension-type member lowered to a function',
        );
        continue;
      }
      try {
        functions.add(_lowerTopLevel(procedure));
      } on Unsupported catch (error) {
        refused.add('top-level ${procedure.name.text}: $error');
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
    return (
      IrLibrary(
        classes,
        constants: constants,
        functions: functions,
        abstractElsewhere: abstractElsewhere,
      ),
      refused,
    );
  }

  /// A top-level function, as a method with no receiver.
  IrMethod _lowerTopLevel(Procedure node) {
    if (node.kind != ProcedureKind.Method) {
      throw Unsupported('a top-level ${node.kind.name}', node.name.text);
    }
    return IrMethod(
      node.name.text,
      [
        for (final p in node.function.positionalParameters)
          IrParam(p.cosmeticName ?? '_', _type(p.type)),
        for (final p in node.function.namedParameters)
          IrParam(p.parameterName, _type(p.type), named: true),
      ],
      _type(node.function.returnType),
      _body(node.function),
      isStatic: true,
    );
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
      'index',
      'values',
      '_name',
      'toString',
      'hashCode',
      '==',
      'name',
      '_enumToString',
      'compareTo',
    };
    final enhanced =
        node.isEnum &&
        (node.procedures.any(
              (p) =>
                  !implicitEnumMembers.contains(p.name.text) && !p.isSynthetic,
            ) ||
            node.fields.any(
              (f) => !f.isStatic && !implicitEnumMembers.contains(f.name.text),
            ));
    // An enum's values are its static const fields -- except that in a real
    // dill they are not there at all. Measured in round 39: of the 200 enums
    // in `package:flutter/`, exactly **one** still has any field. Nothing
    // reads `Axis.vertical` as a field once the constants are materialised,
    // so the fields are unreachable and the compiler drops them, leaving a
    // class that looks like an enum with no values.
    //
    // That is round 26's vanished constructor wearing another face, and the
    // answer is the same: the values survive in the *constants* that name
    // them. `enumValues` is that recovery, done once over the whole component
    // by the driver, because a constant naming this enum can be in any
    // library.
    final values = !node.isEnum || enhanced
        ? const <String>[]
        : [
            for (final f in node.fields)
              if (f.isStatic && f.isConst && f.name.text != 'values')
                f.name.text,
          ];
    // Only when the enum is otherwise translatable. Recovering the variants of
    // an *enhanced* enum would emit it as a plain one and drop its members --
    // which is the thing the refusal exists to prevent, and which this
    // recovery quietly undid until the fixture said so.
    final recovered = values.isNotEmpty || enhanced || !node.isEnum
        ? values
        : (enumValues[node] ?? const <String>[]);
    final cls = IrClass(
      node.name,
      typeParameters: [for (final p in node.typeParameters) p.name ?? 'T'],
      superclass: node.isEnum || base == null || base.name == 'Object'
          ? null
          : base.name,
      isAbstract: node.isAbstract,
      isEnum: node.isEnum,
      values: recovered,
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
      final init = field.initializer;
      if (init == null) throw Unsupported('static without initialiser', name);
      cls.constants.add(
        IrConstDecl(
          name,
          _type(field.type),
          expression(init),
          // A `static final` is computed once on first use, which is what
          // `LazyLock` is. It was refused while there was nothing to say it
          // with; there is now.
          isLazy: !field.isConst,
        ),
      );
    } else {
      final initial = field.initializer;
      cls.fields.add(
        IrFieldDecl(
          name,
          _type(field.type),
          isFinal: field.isFinal,
          initial: initial == null ? null : expression(initial),
        ),
      );
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
            throw Unsupported(
              'super constructor call with no base',
              _sample(init),
            );
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
    // The CFE gives every constructor a body, empty or not, so "has a body" is
    // not the question -- "has a statement in it" is. Asking the first refused
    // every constructor in the corpus, which the fixture comparison caught
    // immediately.
    final body = node.function.body;
    final statements = switch (body) {
      null => const <Statement>[],
      Block(:final statements) => statements,
      EmptyStatement() => const <Statement>[],
      _ => [body],
    };
    final real = statements.where((s) => s is! EmptyStatement).toList();
    cls.constructors.add(
      IrConstructor(
        params,
        inits,
        isConst: node.isConst,
        name: name.isEmpty ? null : name,
        asserts: asserts,
        superBase: superBase,
        superArgs: superArgs,
        body: real.isEmpty
            ? null
            : IrBlock([for (final s in real) statement(s)]),
      ),
    );
  }

  void _lowerProcedure(IrClass cls, Procedure node) {
    final name = node.name.text;
    if (cls.isEnum) {
      // A plain enum has only the implicit members. One with anything else is
      // an enhanced enum -- a Rust enum plus an impl -- and stops here rather
      // than being emitted as a plain one with its methods quietly missing.
      const implicit = {
        'index',
        'values',
        '_name',
        'toString',
        'hashCode',
        '==',
        'name',
        '_enumToString',
        'compareTo',
      };
      if (!implicit.contains(name) && !node.isSynthetic) {
        throw Unsupported('enhanced enum member `$name`', cls.name);
      }
      return;
    }

    final params = [
      for (final p in node.function.positionalParameters)
        IrParam(p.cosmeticName ?? '_', _type(p.type)),
      for (final p in node.function.namedParameters)
        IrParam(p.parameterName, _type(p.type), named: true),
    ];
    final isOperator = node.kind == ProcedureKind.Operator;
    final thrown = <String>{};
    if (!node.isAbstract) {
      final finder = _ThrowFinder();
      node.function.accept(finder);
      thrown.addAll(finder.types);
      if (thrown.length > 1) {
        throw Unsupported('method throwing ${thrown.length} error types', name);
      }
    }
    final method = IrMethod(
      name,
      params,
      _type(node.function.returnType),
      node.isAbstract ? const IrBlock([]) : _body(node.function),
      typeParameters: [
        for (final p in node.function.typeParameters) p.name ?? 'T',
      ],
      isStatic: node.isStatic,
      isGetter: node.kind == ProcedureKind.Getter,
      isSetter: node.kind == ProcedureKind.Setter,
      operator: isOperator ? name : null,
      throws: thrown.isEmpty ? null : thrown.single,
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

/// Whether a statement reads a particular variable.
class _VariableReader extends RecursiveVisitor {
  _VariableReader(this.variable);

  final Variable variable;
  bool found = false;

  @override
  void visitVariableGet(VariableGet node) {
    if (node.variable == variable) found = true;
    super.visitVariableGet(node);
  }
}

/// Finds a `return` that belongs to the enclosing method, not to a closure
/// written inside it -- hence the empty `visitFunctionNode`.
class _EarlyExit extends RecursiveVisitor {
  bool found = false;

  @override
  void visitFunctionNode(FunctionNode node) {}

  @override
  void visitReturnStatement(ReturnStatement node) {
    found = true;
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
Map<Class, List<String>> enumValuesIn(Component component) {
  final byIndex = <Class, Map<int, String>>{};
  final finder = _EnumConstantFinder(byIndex);
  for (final library in component.libraries) {
    library.accept(finder);
  }
  return {
    for (final entry in byIndex.entries)
      entry.key: (entry.value.keys.toList()..sort())
          .map((i) => entry.value[i]!)
          .toList(),
  };
}

class _EnumConstantFinder extends RecursiveVisitor {
  _EnumConstantFinder(this.byIndex);

  final Map<Class, Map<int, String>> byIndex;

  void _look(Constant constant) {
    if (constant is! InstanceConstant) return;
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
