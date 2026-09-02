// The IR dart2rust compiles through.
//
// It knows nothing about `package:analyzer` and nothing about Rust. That is
// the point: the front end is the part most likely to be replaced (see
// README.md -- if `pkg/kernel` ever becomes reachable here, Kernel is the
// better input), and a backend written against analyzer's AST would have to be
// rewritten with it.
//
// It is deliberately small. Every node here is one this compiler can already
// emit; nodes are added when the thing they describe is actually translated,
// not in anticipation. An IR node with no backend is a promise the compiler
// does not keep.
library;

/// A type, as the IR sees it: a name and whether it admits null.
///
/// Dart's `double`/`int`/`bool`/`String` are kept under their Dart names and
/// mapped in the backend, because what they map to is a Rust question.
class IrType {
  const IrType(this.name, {this.nullable = false});

  final String name;
  final bool nullable;

  bool get isNum => name == 'double' || name == 'int';

  @override
  String toString() => nullable ? '$name?' : name;
}

/// A parameter of a constructor or method.
class IrParam {
  const IrParam(this.name, this.type, {this.named = false, this.hasDefault = false});

  final String name;
  final IrType type;
  final bool named;
  final bool hasDefault;
}

// -- Expressions --------------------------------------------------------------

sealed class IrExpr {
  const IrExpr();
}

/// A literal whose value is already known. `value` is the Dart source text for
/// primitives; the backend decides how to spell it.
class IrLiteral extends IrExpr {
  const IrLiteral(this.value, this.type);

  final String value;
  final IrType type;
}

/// A reference to a local variable or parameter.
class IrLocal extends IrExpr {
  const IrLocal(this.name);

  final String name;
}

/// A field read. `target` is null for an implicit `this`.
class IrField extends IrExpr {
  const IrField(this.target, this.name);

  final IrExpr? target;
  final String name;
}

/// A read of a static field or enum value: `Alignment.topLeft`.
class IrStatic extends IrExpr {
  const IrStatic(this.owner, this.name);

  final String owner;
  final String name;
}

class IrBinary extends IrExpr {
  const IrBinary(this.op, this.left, this.right);

  final String op;
  final IrExpr left;
  final IrExpr right;
}

class IrUnary extends IrExpr {
  const IrUnary(this.op, this.operand);

  final String op;
  final IrExpr operand;
}

/// An instance method call. `target` null means `this`.
class IrCall extends IrExpr {
  const IrCall(this.target, this.name, this.args);

  final IrExpr? target;
  final String name;
  final List<IrExpr> args;
}

/// A static method call: `Alignment.lerp(a, b, t)`.
class IrStaticCall extends IrExpr {
  const IrStaticCall(this.owner, this.name, this.args);

  final String owner;
  final String name;
  final List<IrExpr> args;
}

/// A constructor invocation. Named constructors carry their name.
class IrNew extends IrExpr {
  const IrNew(this.type, this.args, {this.constructor});

  final IrType type;
  final List<IrExpr> args;
  final String? constructor;
}

class IrConditional extends IrExpr {
  const IrConditional(this.condition, this.then, this.otherwise);

  final IrExpr condition;
  final IrExpr then;
  final IrExpr otherwise;
}

/// `expr is Type`.
class IrIs extends IrExpr {
  const IrIs(this.expr, this.type, {this.negated = false});

  final IrExpr expr;
  final IrType type;
  final bool negated;
}

/// `this`.
class IrThis extends IrExpr {
  const IrThis();
}

// -- Statements ---------------------------------------------------------------

sealed class IrStmt {
  const IrStmt();
}

class IrReturn extends IrStmt {
  const IrReturn(this.value);

  final IrExpr? value;
}

class IrLocalDecl extends IrStmt {
  const IrLocalDecl(this.name, this.type, this.init);

  final String name;
  final IrType? type;
  final IrExpr? init;
}

class IrIf extends IrStmt {
  const IrIf(this.condition, this.then, this.otherwise);

  final IrExpr condition;
  final IrStmt then;
  final IrStmt? otherwise;
}

class IrBlock extends IrStmt {
  const IrBlock(this.statements);

  final List<IrStmt> statements;
}

class IrExprStmt extends IrStmt {
  const IrExprStmt(this.expr);

  final IrExpr expr;
}

// -- Declarations -------------------------------------------------------------

class IrFieldDecl {
  const IrFieldDecl(this.name, this.type, {required this.isFinal, this.doc});

  final String name;
  final IrType type;
  final bool isFinal;
  final String? doc;
}

/// A `static const` whose value the front end evaluated.
///
/// Held as an already-built expression rather than as source text: the whole
/// reason to have a resolving front end is that `Alignment(-1.0, -1.0)` arrives
/// knowing what it is.
class IrConstDecl {
  const IrConstDecl(this.name, this.type, this.value, {this.doc});

  final String name;
  final IrType type;
  final IrExpr value;
  final String? doc;
}

class IrMethod {
  const IrMethod(
    this.name,
    this.params,
    this.returnType,
    this.body, {
    this.isStatic = false,
    this.isGetter = false,
    this.operator,
    this.doc,
  });

  final String name;
  final List<IrParam> params;
  final IrType returnType;
  final IrStmt body;
  final bool isStatic;
  final bool isGetter;

  /// The Dart operator this method declares, if any: `+`, `unary-`, `[]`.
  final String? operator;
  final String? doc;
}

class IrConstructor {
  const IrConstructor(this.params, this.fieldInits, {required this.isConst, this.doc});

  final List<IrParam> params;

  /// Field name -> the expression it is initialised to. A `this.x` parameter
  /// contributes `x -> IrLocal('x')`.
  final Map<String, IrExpr> fieldInits;
  final bool isConst;
  final String? doc;
}

class IrClass {
  IrClass(this.name, {this.superclass, this.doc});

  final String name;
  final String? superclass;
  final String? doc;
  final List<IrFieldDecl> fields = [];
  final List<IrConstDecl> constants = [];
  final List<IrMethod> methods = [];
  final List<IrConstructor> constructors = [];
}

/// Raised when the front end meets Dart it cannot lower.
///
/// It carries the source so the report says what stopped it, not merely that
/// something did.
class Unsupported implements Exception {
  Unsupported(this.what, this.source);

  final String what;
  final String source;

  @override
  String toString() => 'unsupported $what: $source';
}
