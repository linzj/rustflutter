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

/// Dart's postfix `!`: the value, asserted not to be null.
///
/// Its own node rather than an `IrUnary('!')`, because Dart's *prefix* `!` is
/// boolean negation and the two would otherwise share a spelling while meaning
/// nothing alike.
///
/// Rust's `unwrap()` is the same contract -- "I say this is not null; crash if
/// I am wrong" -- and, like Dart's `!`, it is still there in a release build.
/// It is not `unwrap_or_default()`: upstream wrote `!` where it had already
/// established the value was there, and turning a crash into a default would
/// replace a loud failure with a quiet wrong answer.
class IrNullCheck extends IrExpr {
  const IrNullCheck(this.operand);

  final IrExpr operand;
}

/// `this`.
class IrThis extends IrExpr {
  const IrThis();
}

/// `super.name(args)`.
///
/// Kept as its own node rather than lowered to a call on `this`, because in
/// Rust the two are not the same and the difference is a hang. Once an impl
/// overrides a trait's default method there is no way to reach the default
/// again -- `Trait::name(self)` dispatches straight back to the override. And
/// calling super from inside an override of the same name is not an edge case
/// in Flutter: every one of the 435 super calls in painting/ and rendering/
/// is exactly that.
class IrSuperCall extends IrExpr {
  const IrSuperCall(this.base, this.name, this.args);

  /// The class the call resolves into.
  final String base;
  final String name;
  final List<IrExpr> args;
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

/// `x = value` where `x` is a **local variable**.
///
/// Only a local. Assigning a field is a different problem with a different
/// answer -- it needs `&mut self`, and `&mut` spreads to every caller -- and
/// the two were one entry in the census until they were counted apart:
/// across `package:flutter` there are 10633 local assignments and 10089 field
/// ones, so planning the hard answer for all of them would have been planning
/// it for twice as many as need it.
class IrAssign extends IrStmt {
  const IrAssign(this.name, this.value);

  final String name;
  final IrExpr value;
}

/// `this.x = value` -- a write to one of this object's own fields.
///
/// Only `this`, and only a real field. Writing another object's field needs a
/// mutable *receiver*, which is a harder question than a mutable `self`, and
/// writing through a **setter** is not a write at all -- it is a call, and
/// which of the two it is cannot be seen from the assignment.
///
/// The split is measured, not guessed: across `package:flutter` 6220 field
/// writes go through `this` and 3869 through another object.
class IrAssignField extends IrStmt {
  const IrAssignField(this.name, this.value);

  final String name;
  final IrExpr value;
}

/// `target.x = value` where `x` is a **setter**, not a field.
///
/// A setter is a call, not a write, and which of the two an assignment is
/// cannot be seen from the assignment itself -- it depends on how the receiver's
/// class declared `x`. Both front ends can tell (Kernel names the target member;
/// analyzer answers with `isSynthetic`), so the distinction is made where it is
/// known rather than guessed at in the backend.
///
/// `target` is null for `this`.
class IrSetter extends IrStmt {
  const IrSetter(this.target, this.name, this.value);

  final IrExpr? target;
  final String name;
  final IrExpr value;
}

/// `assert(condition, message)`.
///
/// Dart's `assert` and Rust's `debug_assert!` are the same thing: a check that
/// runs in debug builds and is compiled out of release ones. That correspondence
/// is exact enough to translate rather than emulate.
///
/// [message] is the Dart source of the message expression when there was one and
/// it was not a plain string. It is not translated -- see the backend for why --
/// but it is carried so the emitted code can say what was dropped.
class IrAssert extends IrStmt {
  const IrAssert(this.condition, {this.literalMessage, this.message});

  final IrExpr condition;

  /// The message when it was a plain string literal, ready to emit.
  final String? literalMessage;

  /// The source of a message that was not a plain string.
  final String? message;
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
    this.isSetter = false,
    this.operator,
    this.doc,
  });

  final String name;
  final List<IrParam> params;
  final IrType returnType;
  final IrStmt body;
  final bool isStatic;
  final bool isGetter;

  /// A `set x(v)`. In Rust it is `set_x(&mut self, v)`, which is why it needs
  /// its own flag: a getter and a setter share a name in Dart and must not in
  /// Rust, and the mutability analysis keys on the Rust name for that reason.
  final bool isSetter;

  /// The Dart operator this method declares, if any: `+`, `unary-`, `[]`.
  final String? operator;
  final String? doc;
}

class IrConstructor {
  const IrConstructor(
    this.params,
    this.fieldInits, {
    required this.isConst,
    this.name,
    this.asserts = const [],
    this.doc,
  });

  /// The named constructor's name, or null for the unnamed one.
  ///
  /// Dart's `EdgeInsets.all` and Rust's `EdgeInsets::all` are the same shape --
  /// an associated function returning Self -- so this needs no encoding beyond
  /// the name itself. The unnamed constructor becomes `new`.
  final String? name;

  final List<IrParam> params;

  /// `assert`s from the initialiser list, which run before the fields are set.
  final List<IrAssert> asserts;

  /// Field name -> the expression it is initialised to. A `this.x` parameter
  /// contributes `x -> IrLocal('x')`.
  final Map<String, IrExpr> fieldInits;
  final bool isConst;
  final String? doc;
}

class IrClass {
  IrClass(this.name, {this.superclass, this.isAbstract = false, this.doc});

  final String name;
  final String? superclass;

  /// Whether Dart declared it `abstract`.
  ///
  /// This decides the whole shape of the output: an abstract class becomes a
  /// **trait**, because that is what Rust has for "a set of operations with no
  /// storage of its own". A concrete class becomes a struct, and one that
  /// extends an abstract class also gets an `impl`.
  final bool isAbstract;

  final String? doc;
  final List<IrFieldDecl> fields = [];
  final List<IrConstDecl> constants = [];
  final List<IrMethod> methods = [];
  final List<IrConstructor> constructors = [];

  /// Members declared `abstract` -- no body, so they are the trait's required
  /// methods rather than its defaults.
  final List<IrMethod> abstractMethods = [];
}

/// Every class in one file, lowered together.
///
/// The compiler used to take one class at a time, which is enough while a class
/// is a struct and nothing else. It is not enough for a hierarchy: to emit
/// `impl AlignmentGeometry for Alignment` the backend has to know that
/// `AlignmentGeometry` is abstract and what it requires, and neither fact is
/// visible from inside `Alignment`.
class IrLibrary {
  IrLibrary(this.classes);

  final List<IrClass> classes;

  IrClass? operator [](String? name) {
    if (name == null) return null;
    for (final c in classes) {
      if (c.name == name) return c;
    }
    return null;
  }

  bool isAbstract(String? name) => this[name]?.isAbstract ?? false;
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
