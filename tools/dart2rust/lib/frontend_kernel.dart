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
    if (type is FunctionType) return const IrType('Function');
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
      final name = node.variable.cosmeticName;
      if (name == null || name.startsWith('#')) {
        throw Unsupported('synthetic variable', _sample(node));
      }
      return IrLocal(name);
    }
    if (node is InstanceGet) return _instanceGet(node);
    if (node is StaticGet) return _staticGet(node);
    if (node is InstanceInvocation) return _instanceInvocation(node);
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
    if (node is AsExpression) return expression(node.operand);
    if (node is ConstantExpression) return _constant(node.constant, node);
    throw Unsupported('expression ${node.runtimeType}', _sample(node));
  }

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
    final owner = target.enclosingClass?.name;
    if (owner == null) {
      throw Unsupported('top-level `${target.name.text}`', _sample(node));
    }
    return IrStatic(owner, target.name.text);
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
    final args = _arguments(node.arguments);
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
    final refused = <String>[];
    for (final cls in library.classes) {
      // Anonymous mixin applications stay skipped: they are the CFE's own
      // synthetic classes, not something upstream wrote. Private classes do
      // not -- see the note where `_refusePrivate` used to be.
      if (cls.isAnonymousMixin) continue;
      final (lowered, problems) = lowerClass(cls);
      classes.add(lowered);
      refused.addAll(problems.map((p) => '${cls.name}: $p'));
    }
    return (IrLibrary(classes), refused);
  }

  (IrClass, List<String>) lowerClass(Class node) {
    // Kernel's superclass may be a synthetic mixin application; the class a
    // reader would name is the first one above that is not.
    var base = node.superclass;
    while (base != null && base.isAnonymousMixin) {
      base = base.superclass;
    }
    final cls = IrClass(
      node.name,
      superclass: base == null || base.name == 'Object' ? null : base.name,
      isAbstract: node.isAbstract,
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
    if (field.isStatic) {
      if (!field.isConst) {
        throw Unsupported('non-const static field', name);
      }
      final init = field.initializer;
      if (init == null) throw Unsupported('const without initialiser', name);
      cls.constants
          .add(IrConstDecl(name, _type(field.type), expression(init)));
    } else {
      cls.fields.add(
          IrFieldDecl(name, _type(field.type), isFinal: field.isFinal));
    }
  }

  void _lowerConstructor(IrClass cls, Constructor node) {
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
    for (final init in node.initializers) {
      if (init is FieldInitializer) {
        inits[init.field.name.text] = expression(init.value);
      } else if (init is AssertInitializer) {
        final statement = init.statement;
        asserts.add(_assert(statement.condition, statement.message));
      } else if (init is SuperInitializer) {
        if (init.arguments.positional.isNotEmpty ||
            init.arguments.named.isNotEmpty) {
          throw Unsupported('super constructor call with arguments',
              _sample(init));
        }
        // A no-argument super() adds nothing to a Rust struct literal.
      } else if (init is RedirectingInitializer) {
        throw Unsupported('redirecting constructor', _sample(init));
      } else {
        throw Unsupported('initialiser ${init.runtimeType}', _sample(init));
      }
    }
    cls.constructors.add(IrConstructor(
      params,
      inits,
      isConst: node.isConst,
      name: name.isEmpty ? null : name,
      asserts: asserts,
    ));
  }

  void _lowerProcedure(IrClass cls, Procedure node) {
    final name = node.name.text;
    if (node.isStatic && node.kind == ProcedureKind.Factory) {
      throw Unsupported('factory constructor', name);
    }
    if (node.kind == ProcedureKind.Setter) {
      throw Unsupported('setter', name);
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
      operator: isOperator ? name : null,
    );
    (node.isAbstract ? cls.abstractMethods : cls.methods).add(method);
  }
}
