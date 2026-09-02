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
  const IrType(this.name, {this.nullable = false, this.arguments = const []})
    : parameters = null,
      returns = null;

  /// A function type: `double Function(double, String)`.
  ///
  /// Structured rather than a name, because Rust needs the pieces: a parameter
  /// takes `impl Fn(f32) -> f32` and a field holds `Box<dyn Fn(f32) -> f32>`,
  /// and neither can be built from the string `Function`.
  const IrType.function(this.parameters, this.returns, {this.nullable = false})
    : name = 'Function',
      arguments = const [];

  final String name;
  final bool nullable;

  /// `List<double>`'s `double`. Empty for a type with no arguments.
  ///
  /// The IR carried none until round 32, so `List<double>` arrived as the bare
  /// name `List` -- fine while lists were refused, and useless the moment they
  /// were not, since `Vec` has to know what it holds.
  final List<IrType> arguments;

  /// Non-null only for a function type.
  final List<IrType>? parameters;
  final IrType? returns;

  bool get isFunction => parameters != null;
  bool get isNum => name == 'double' || name == 'int';

  @override
  String toString() {
    final args = arguments.isEmpty ? '' : '<${arguments.join(', ')}>';
    return nullable ? '$name$args?' : '$name$args';
  }
}

/// A parameter of a constructor or method.
class IrParam {
  const IrParam(
    this.name,
    this.type, {
    this.named = false,
    this.hasDefault = false,
  });

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

/// A read of a static field or enum value: `Alignment.topLeft`, `Axis.vertical`.
class IrStatic extends IrExpr {
  const IrStatic(this.owner, this.name, {this.isEnumValue = false});

  final String owner;
  final String name;

  /// Whether this names an enum's value rather than a static field.
  ///
  /// The two are spelled alike in Dart and differently in Rust -- an associated
  /// const is `Alignment::TOP_LEFT`, a variant is `Axis::Vertical` -- and the
  /// backend cannot tell them apart, because the owner is often declared in
  /// another file. So the front end, which resolved the reference, says which
  /// it is.
  final bool isEnumValue;
}

class IrBinary extends IrExpr {
  const IrBinary(this.op, this.left, this.right, {this.type});

  final String op;
  final IrExpr left;
  final IrExpr right;

  /// The type of the *result*, when the front end knows it.
  ///
  /// Only `+` needs it so far, and it needs it badly: Dart's `a + b` on two
  /// strings is Rust's `String + &str`, which `a + b` is not. 422 of these in
  /// `package:flutter/`, and they only started appearing once `for` statements
  /// translated -- until then the methods holding them were refused earlier.
  final IrType? type;
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

/// A `const` instance given by its field values rather than by a constructor
/// call: `Alignment { x: -1.0, y: -1.0 }`.
///
/// The Kernel front end meets constants already evaluated, and rebuilding them
/// as `Alignment::new(-1.0, -1.0)` reads better -- so that is still tried
/// first. But it depends on the constructor still being in the dill, and for a
/// `const` instance nothing ever calls the constructor, so the compiler is
/// free to delete it and does: 2965 of `package:flutter`'s 5602 const
/// instances are of a class whose constructors are all gone. The field values
/// are the one thing an `InstanceConstant` always carries.
class IrConstInstance extends IrExpr {
  const IrConstInstance(this.type, this.fields);

  final IrType type;

  /// Field name to value, by the field's Dart name. Order does not matter:
  /// the backend emits them in the struct's own order.
  final Map<String, IrExpr> fields;
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

/// `a?.b` -- do the thing only if the receiver is there.
///
/// Rust says this with `a.map(|x| ...)`, so the body needs a name for the value
/// that was bound. [IrBound] is that name: it stands where the receiver's
/// non-null value goes, and the backend supplies the closure parameter.
///
/// Keeping the body as an expression rather than as "a member and some
/// arguments" is what lets a chain work -- `a?.b.c()` binds once and does two
/// things with the binding, and 89 of upstream's are chained.
class IrNullAware extends IrExpr {
  const IrNullAware(this.receiver, this.body);

  final IrExpr receiver;
  final IrExpr body;
}

/// The value bound by the enclosing [IrNullAware].
class IrBound extends IrExpr {
  const IrBound();
}

/// Statements, then a value: what Rust calls a block expression.
///
/// Dart has no such thing to write, but the CFE makes them -- a cascade
/// `Paint()..color = c..style = s` becomes "bind the receiver, do the steps,
/// produce the binding". Rust says exactly that with `{ let mut it = ...; ...;
/// it }`, so this is a translation rather than an encoding.
class IrBlockValue extends IrExpr {
  const IrBlockValue(this.statements, this.value);

  final List<IrStmt> statements;
  final IrExpr value;
}

/// Calling a function *value*: `f(x)`, where `f` is a variable.
///
/// Distinct from [IrCall], which names a method on a receiver. Rust spells this
/// one the same as Dart does, so the node exists to keep the two apart rather
/// than to encode anything -- a method call needs a receiver and this does not.
class IrCallValue extends IrExpr {
  const IrCallValue(this.target, this.args);

  final IrExpr target;
  final List<IrExpr> args;
}

/// A closure literal: `(x) => x * 2`.
///
/// Only the ones that capture nothing or read outer locals reach here. A
/// closure that captures `this` needs an ownership story -- it outlives the
/// call that made it, and `this` is a borrow -- and 60% of `package:flutter`'s
/// closures are that kind, which is a round of its own rather than a corner of
/// this one.
class IrClosure extends IrExpr {
  const IrClosure(this.params, this.body, this.returns);

  final List<IrParam> params;
  final IrStmt body;
  final IrType returns;
}

/// `a ?? b`.
///
/// Its own node because Rust needs a fact the IR does not otherwise carry: is
/// the **result** still nullable? `a ?? b` yields a non-null value only when `b`
/// is non-null, and Rust spells the two differently -- `a.unwrap_or_else(|| b)`
/// unwraps, `a.or_else(|| b)` does not. Nested `??` is where this shows: in
/// `a ?? b ?? c` the inner `b ?? c` is non-null but `a ?? b` on its own is not.
///
/// [eager] marks a right side with no effects, where the shorter `unwrap_or`
/// and `or` are safe. Dart's `??` is short-circuit and Rust's `unwrap_or` is
/// not, and only 23% of the 6764 `??` in `package:flutter` have a right side
/// that may be evaluated unconditionally.
class IrIfNull extends IrExpr {
  const IrIfNull(
    this.left,
    this.right, {
    required this.nullableResult,
    required this.eager,
  });

  final IrExpr left;
  final IrExpr right;
  final bool nullableResult;
  final bool eager;
}

/// `x == null`.
///
/// Its own node because Rust asks the question differently: a nullable value is
/// an `Option`, and the test is `x.is_none()` rather than a comparison against
/// a null that does not exist.
///
/// Both front ends must land here, and that is the point of the node. Kernel
/// hands over an `EqualsNull` -- the CFE has already recognised the shape --
/// while analyzer gives an ordinary `==` against a null literal. Left alone the
/// two would emit different Rust for the same Dart, and 2524 places in
/// `package:flutter` would have found out.
class IrIsNull extends IrExpr {
  const IrIsNull(this.operand);

  final IrExpr operand;
}

/// A reference to a top-level constant: `kMinInteractiveDimension`.
///
/// Dart has module-level names and so does Rust, so this needs no owner. It is
/// its own node rather than an [IrStatic] with an empty owner because a name
/// with no owner is not a static field with a missing one.
class IrTopLevel extends IrExpr {
  const IrTopLevel(this.name);

  final String name;
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
  const IrAssignField(this.name, this.value, {this.target});

  /// The object written to, or null for `this`.
  ///
  /// A cascade writes another object's field, which needs a mutable receiver
  /// rather than a mutable `self`. Inside a cascade that receiver is a local
  /// the block just bound, so it is the one case of the 3869 "field of another
  /// object" writes that needs nothing new.
  final IrExpr? target;

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

/// A `throw` written where a value was wanted: `a ?? throw StateError(..)`.
///
/// Rust has no throw, and it does not need one: `return Err(e)` is an
/// expression of type `!`, which fits wherever a value was expected. 151 of
/// these in `package:flutter/`.
class IrThrowValue extends IrExpr {
  const IrThrowValue(this.value);

  final IrExpr value;
}

/// A labelled block, and a `break` out of one.
///
/// Kernel's `break` points at a `LabeledStatement`, and Rust's labelled block
/// is the same construct: `break 'l` leaves it. Dart's `break` out of a loop
/// arrives wrapped this way, so this is also how a loop gets its `break`.
class IrLabeled extends IrStmt {
  const IrLabeled(this.label, this.body);

  final String label;
  final IrStmt body;
}

class IrBreak extends IrStmt {
  const IrBreak([this.label]);

  /// Null for a plain `break` out of the loop it is written in.
  final String? label;
}

class IrContinue extends IrStmt {
  const IrContinue();
}

/// `identical(a, b)` -- Dart's reference identity.
///
/// 259 of these in `package:flutter/`, and 140 have `this` on one side: the
/// fast path at the top of an `operator ==`. That is the case Rust can say
/// exactly, with `std::ptr::eq`. The rest compare two values, and a translated
/// value type is a `Copy` struct whose address says nothing about identity, so
/// they are refused rather than answered wrongly.
class IrIdentical extends IrExpr {
  const IrIdentical(this.left, this.right);

  final IrExpr left;
  final IrExpr right;
}

/// `for (final x in xs) { .. }`.
///
/// The CFE lowers this into an iterator loop -- bind `xs.iterator`, loop while
/// `moveNext()`, read `current` -- and 405 of `package:flutter/`'s 592 `for`
/// statements are really that. Restored rather than carried across in pieces,
/// because the analyzer front end sees what was written and the two have to
/// arrive at the same Rust.
class IrForIn extends IrStmt {
  const IrForIn(this.name, this.iterable, this.body);

  final String name;
  final IrExpr iterable;
  final IrStmt body;
}

/// `xs[i]`, and `xs[i] = v`.
class IrIndex extends IrExpr {
  const IrIndex(this.target, this.index);

  final IrExpr target;
  final IrExpr index;
}

class IrIndexSet extends IrStmt {
  const IrIndexSet(this.target, this.index, this.value);

  final IrExpr target;
  final IrExpr index;
  final IrExpr value;
}

/// `[a, b, c]`.
class IrListLiteral extends IrExpr {
  const IrListLiteral(this.elements, this.element);

  final List<IrExpr> elements;

  /// What the list holds, so an empty literal still knows its `Vec<T>`.
  final IrType element;
}

/// `(1, 'two')` -- a record, which is a Rust tuple.
///
/// Positional fields only: a named record field would need a struct with a
/// name, and there is no name to give it. 62 of these.
class IrRecord extends IrExpr {
  const IrRecord(this.fields);

  final List<IrExpr> fields;
}

/// `r.$1` -- a positional record field.
///
/// The index is Rust's, counted from zero. Dart counts its record fields from
/// one, and the front ends do that subtraction so the backend has one story.
class IrRecordField extends IrExpr {
  const IrRecordField(this.record, this.index);

  final IrExpr record;
  final int index;
}

/// `{'a': 1, 'b': 2}`.
///
/// `HashMap::from([..])`, which is what the lookup-only decision of round 33
/// leaves it as. A literal that is *iterated* would need the insertion order
/// a `HashMap` does not keep, and those members are refused wherever they are
/// reached -- so the literal itself is safe.
class IrMapLiteral extends IrExpr {
  const IrMapLiteral(this.entries, this.key, this.value);

  final List<(IrExpr, IrExpr)> entries;
  final IrType key;
  final IrType value;
}

/// `xs.map(f).toList()` -- a chain, recognised whole.
///
/// Not a name-by-name mapping, because Dart's `map` and Rust's are only the
/// same when the chain ends: `xs.iter().map(f)` is a lazy iterator whose
/// elements are references, and collecting it is what makes it a list again.
/// Measured: of `package:flutter/`'s 126 `map`/`where`/`expand` calls, 72 are
/// collected right there and 54 escape as a lazy Iterable. Only the collected
/// ones are translated; the rest are refused rather than guessed at.
class IrIterChain extends IrExpr {
  const IrIterChain(this.source, this.steps);

  final IrExpr source;

  /// Rust's name for each step, and the closure it takes.
  final List<(String, IrExpr)> steps;
}

/// Dart's `Map` methods in Rust's spelling.
///
/// Lookup only, and that is a decision with a measurement behind it: of the
/// 923 uses in `package:flutter/`, 623 look up and 109 iterate -- and a Dart
/// map literal is a LinkedHashMap, which iterates in **insertion order**,
/// while `HashMap` does not. Translating `keys`, `values`, `entries` or
/// `forEach` to `HashMap`'s would silently reorder those 109. They are
/// refused until there is a representation that keeps the order.
const mapMethodNames = <String, String>{
  'containsKey': 'contains_key',
  'remove': 'remove',
  'clear': 'clear',
  'isEmpty': 'is_empty',
  'length': 'len',
};

/// `Map` members that depend on iteration order.
const orderedMapMembers = <String>{
  'keys',
  'values',
  'entries',
  'forEach',
  'map',
  'putIfAbsent',
};

/// Dart's `List` and `Iterable` methods in Rust's spelling.
///
/// Measured before it was written: across `package:flutter/` the calls are
/// `[]` 687, `add` 548, `iterator` 410, `length` 343, `[]=` 141, `toList` 105,
/// `isEmpty`/`isNotEmpty` 181. All of them are `Vec`, which is what made the
/// representation an easy decision -- unlike `Map`, whose literal is insertion
/// ordered and whose 109 iterating uses `HashMap` would silently reorder.
///
/// Shared by both front ends so they cannot drift: a name mapped on one side
/// and not the other is a disagreement the fixtures would report but nothing
/// would explain.
const listMethodNames = <String, String>{
  'add': 'push',
  'addAll': 'extend',
  'clear': 'clear',
  'isEmpty': 'is_empty',
  'length': 'len',
  'removeLast': 'pop',
  'contains': 'contains',
  // Not renames: the backend spells these out, because Rust says them with
  // something other than a method of the same shape.
  'isNotEmpty': '!is_empty',
  'first': 'first',
  'last': 'last',
  'toList': 'to_list',
};

/// The steps of an iterator chain, in Rust's spelling.
const iterStepNames = <String, String>{'map': 'map', 'where': 'filter'};

/// `'a $b c'` -- a string built from pieces.
///
/// Rust's `format!` is the same thing said differently: the literal pieces
/// become the format string and the rest become its arguments. 99 of these.
class IrInterpolation extends IrExpr {
  const IrInterpolation(this.parts);

  /// Literal text and expressions, in order. A literal part carries its text
  /// in [IrLiteral.value].
  final List<IrExpr> parts;
}

/// A function used as a value: `Alignment.lerp` or a top-level `describe`.
///
/// Not a closure -- Rust names the function itself, which is why this is
/// separate from the tear-off of an *instance* method, where the receiver has
/// to be captured and the ownership question starts. 111 of these.
class IrFunctionRef extends IrExpr {
  const IrFunctionRef(this.owner, this.name);

  /// The class holding it, or null for a top-level function.
  final String? owner;
  final String name;
}

/// A local function: `void step() { .. }` written inside a body.
class IrLocalFunction extends IrStmt {
  const IrLocalFunction(this.name, this.closure);

  final String name;
  final IrClosure closure;
}

/// `switch (x) { case A: .. }`.
///
/// 628 in `package:flutter/`, and the shape is friendly: almost all switch on
/// an enum, only 20 have a default, none have an empty fall-through case and
/// exactly one uses `continue L`. So Rust's `match` is not an approximation of
/// this -- it is the same construct, including the exhaustiveness Dart's
/// enum switches already rely on.
class IrSwitch extends IrStmt {
  const IrSwitch(this.value, this.cases, this.otherwise);

  final IrExpr value;
  final List<IrCase> cases;

  /// The `default:` body, or null. Rust needs a `_` arm when the arms are not
  /// exhaustive, and will say so itself when one is missing.
  final IrStmt? otherwise;
}

class IrCase {
  const IrCase(this.values, this.body);

  /// One arm may match several values: `case A: case B: ..` is `A | B =>`.
  final List<IrExpr> values;
  final IrStmt body;
}

/// `x = v` where the value of the assignment is wanted. The local's twin of
/// [IrSetValue].
class IrAssignValue extends IrExpr {
  const IrAssignValue(this.name, this.value);

  final String name;
  final IrExpr value;
}

/// `a.b = v` where the value of the assignment is wanted.
///
/// 202 of these. Rust's assignment has the value `()`, so the value has to be
/// kept: bind it, assign it, produce it.
class IrSetValue extends IrExpr {
  const IrSetValue(this.target, this.name, this.value);

  final IrExpr? target;
  final String name;
  final IrExpr value;
}

/// `while (condition) { .. }`.
///
/// A `for` becomes one of these wrapped in a block holding its declarations,
/// with the updates at the end of the body. Kernel's `for` is already that
/// shape -- separate lists of variables, a condition and updates -- and so is
/// Dart's `for (x in xs)`, which the CFE lowers to an iterator loop before this
/// compiler ever sees it: 405 of `package:flutter`'s 592 `for` statements are
/// really that.
class IrWhile extends IrStmt {
  const IrWhile(this.condition, this.body, {this.label});

  final IrExpr condition;
  final IrStmt body;

  /// A label, when the loop needs one.
  ///
  /// It needs one exactly when its body is a labelled block -- which is how a
  /// `continue` in a `for` reaches the updates -- because Rust will not let an
  /// unlabelled `break` cross a labelled block.
  final String? label;
}

/// `try { .. } finally { .. }`.
///
/// Separate from [IrTryCatch] because it answers a different question: a catch
/// *stops* a failure, a finalizer only has to run on the way past. Kernel keeps
/// them apart too -- `try/catch/finally` arrives as a TryFinally wrapping a
/// TryCatch -- so nesting the two nodes is what the source said.
class IrTryFinally extends IrStmt {
  const IrTryFinally(this.body, this.finalizer);

  final IrStmt body;
  final IrStmt finalizer;
}

/// `try { .. } catch (e) { .. }`.
///
/// The other half of `Result`: `?` carries a failure outward, this stops it.
/// Measured first -- 155 of upstream's 174 catch clauses catch `Object`, which
/// is `catch (e)` with no type at all, so the common case needs no type test.
///
/// [stack] is the stack-trace variable when the clause binds one. 133 clauses
/// (76%) do, and a `Result` carries no stack; binding one and never using it
/// costs nothing, so only a clause that **reads** it is refused.
class IrTryCatch extends IrStmt {
  const IrTryCatch(
    this.body,
    this.error,
    this.handler, {
    this.errorType,
    this.stack,
  });

  final IrStmt body;
  final String error;
  final String? errorType;
  final String? stack;
  final IrStmt handler;
}

/// `throw e`.
///
/// Becomes `return Err(e)`, on the decision that failure travels in the return
/// value rather than by unwinding. Measured before choosing: 717 members of
/// `package:flutter` throw directly, 5906 (20%) end up returning `Result` once
/// that propagates, and **709 of 721 throw exactly one error type** -- so the
/// error type is a concrete one per function and no enum is needed.
class IrThrow extends IrStmt {
  const IrThrow(this.value);

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
  const IrFieldDecl(
    this.name,
    this.type, {
    required this.isFinal,
    this.initial,
    this.doc,
  });

  final String name;
  final IrType type;
  final bool isFinal;

  /// A value given at the declaration -- `double width = 0.0;`.
  ///
  /// Dart applies it to every constructor that does not set the field itself,
  /// and Rust has no such thing, so the constructor has to write it out. Not
  /// having this was 257 refusals reading "field never initialised".
  final IrExpr? initial;

  final String? doc;
}

/// A `static const` whose value the front end evaluated.
///
/// Held as an already-built expression rather than as source text: the whole
/// reason to have a resolving front end is that `Alignment(-1.0, -1.0)` arrives
/// knowing what it is.
class IrConstDecl {
  const IrConstDecl(
    this.name,
    this.type,
    this.value, {
    this.doc,
    this.isLazy = false,
  });

  final String name;
  final IrType type;
  final IrExpr value;
  final String? doc;

  /// A Dart `static final`, computed once on first use rather than at compile
  /// time. 140 of these in `package:flutter/`.
  ///
  /// Rust says it with `LazyLock`, and says it at *module* scope: an `impl`
  /// block may hold a `const` but not a `static`, so the name carries its
  /// class -- `Foo.bar` becomes `FOO_BAR` -- and a read of it dereferences.
  final bool isLazy;
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
    this.throws,
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

  /// The error type this method throws, if it throws exactly one.
  ///
  /// Two or more is refused rather than widened to a common supertype: 98% of
  /// upstream's throwing members throw one type, and inventing an enum for the
  /// other 2% would put it in every signature the failure reaches.
  final String? throws;
  final String? doc;
}

class IrConstructor {
  const IrConstructor(
    this.params,
    this.fieldInits, {
    required this.isConst,
    this.name,
    this.asserts = const [],
    this.superBase,
    this.superArgs = const [],
    this.doc,
    this.body,
  });

  /// Statements the constructor runs after the fields are set.
  ///
  /// Rust has no such phase, but it does not need one: the value is built into
  /// a local, the body runs against that local, and the local is returned.
  /// 84 of upstream's constructors have one, and they were refused rather than
  /// dropped -- a dropped body compiles and ignores its arguments.
  final IrStmt? body;

  /// The named constructor's name, or null for the unnamed one.
  ///
  /// Dart's `EdgeInsets.all` and Rust's `EdgeInsets::all` are the same shape --
  /// an associated function returning Self -- so this needs no encoding beyond
  /// the name itself. The unnamed constructor becomes `new`.
  final String? name;

  final List<IrParam> params;

  /// `assert`s from the initialiser list, which run before the fields are set.
  final List<IrAssert> asserts;

  /// `: super(a, b)` -- the base class and what it was passed.
  ///
  /// Rust has no constructor inheritance, so this cannot be a call. The base's
  /// own field initialisers are inlined into this constructor instead, with its
  /// parameters replaced by these arguments -- which is what "flattening the
  /// hierarchy" means when it reaches storage.
  ///
  /// It has to be recorded rather than resolved here because the base's lowered
  /// form is not available while this class is being lowered; the backend has
  /// the whole library and does the substitution.
  final String? superBase;
  final List<IrExpr> superArgs;

  /// Field name -> the expression it is initialised to. A `this.x` parameter
  /// contributes `x -> IrLocal('x')`.
  final Map<String, IrExpr> fieldInits;
  final bool isConst;
  final String? doc;
}

class IrClass {
  IrClass(
    this.name, {
    this.superclass,
    this.isAbstract = false,
    this.isEnum = false,
    this.values = const [],
    this.doc,
  });

  final String name;
  final String? superclass;

  /// Whether Dart declared it `abstract`.
  ///
  /// This decides the whole shape of the output: an abstract class becomes a
  /// **trait**, because that is what Rust has for "a set of operations with no
  /// storage of its own". A concrete class becomes a struct, and one that
  /// extends an abstract class also gets an `impl`.
  final bool isAbstract;

  /// A Dart `enum`. Rust has one too, so this is a translation rather than an
  /// encoding -- but only for the plain kind. Across `package:flutter` 232 of
  /// the 249 enums are plain and 17 carry fields or methods; the enhanced ones
  /// are a Rust enum *plus* an impl, which is a different job.
  final bool isEnum;

  /// The enum's values, in declaration order, under their Dart names.
  final List<String> values;

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
  IrLibrary(this.classes, {this.constants = const []});

  final List<IrClass> classes;

  /// Top-level `const`s and `final`s. Dart puts 241 of them under
  /// `package:flutter` alone -- `kMinInteractiveDimension`, `kIsWeb` -- and
  /// they are referred to 507 times.
  final List<IrConstDecl> constants;

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
