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
    this.kept = false,
    this.defaultValue,
  });

  /// The declared default, when the front end could read it. A forwarder for
  /// an override that *widens* the base's signature -- `InputBorder.paint`
  /// adds `gapExtent = 0.0` -- passes this where the base has nothing.
  final IrExpr? defaultValue;

  /// Whether the callee does more with this parameter than call it.
  ///
  /// A function-typed parameter is `impl Fn(..)` -- borrowed -- unless the
  /// callee keeps it, in which case it has to be owned: `Box<dyn Fn(..)>`.
  /// `addListener` puts its argument in a list, and a list cannot hold a
  /// borrow. Measured at 394 of 1234 across the package.
  final bool kept;

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
  const IrField(this.target, this.name, {this.onEnum = false, this.owner});

  final IrExpr? target;
  final String name;

  /// The class that declares the field, when the read is on another object.
  ///
  /// A counted class keeps every non-final field in a cell, and a read of one
  /// from outside has to go through the cell too. The backend sees `entry.x`
  /// with no idea what `entry` is; the front end resolved the field, so it
  /// says whose it is.
  final String? owner;

  /// Whether the thing read belongs to an *enum*.
  ///
  /// A Dart enum can give each value its own final field, and the Rust for
  /// that is a getter over a `match` -- the value is a constant of the
  /// variant, not storage. So the read is a call, and only the front end knows
  /// it: the backend sees `state.value` with no idea what `state` is.
  final bool onEnum;
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
  const IrCall(
    this.target,
    this.name,
    this.args, {
    this.qualifier,
    this.receiverClass,
    this.fails = false,
    this.diverges = false,
  });

  /// Whether the callee can fail (`IrMethod.fails`): the call is `?`ed.
  final bool fails;

  /// Whether the callee never returns (Dart `Never`): its `Result` holds
  /// an `Infallible`, and the call is spelled as the `!` it is.
  final bool diverges;

  final IrExpr? target;
  final String name;
  final List<IrExpr> args;

  /// The trait to name the method through -- `RenderBox::constraints(&self)`
  /// -- when the receiver's hierarchy declares the name more than once: a
  /// Dart override in an abstract class, or a covariant return, is a second
  /// declaration in the subtrait, and the plain call sees both (975
  /// "multiple applicable items" in the workspace, 2026-09-04).
  final String? qualifier;

  /// The receiver's static class, for a qualified call: whether it is a
  /// handle (`Rc<..>`, a counted or abstract class) to reach through with
  /// `&*` rather than `&` is the backend's knowledge, not the front end's.
  final String? receiverClass;
}

/// A static method call: `Alignment.lerp(a, b, t)`, or a top-level one.
class IrStaticCall extends IrExpr {
  const IrStaticCall(
    this.owner,
    this.name,
    this.args, {
    this.fails = false,
    this.diverges = false,
  });

  /// Whether the callee can fail (`IrMethod.fails`).
  final bool fails;

  /// Whether the callee never returns. See `IrCall.diverges`.
  final bool diverges;

  /// Null for a top-level function, which needs no owner in either language.
  final String? owner;
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

/// `await x`.
///
/// Rust spells it after the expression and Dart before it, which is the whole
/// of the difference: `await f()` is `f().await`.
class IrAwait extends IrExpr {
  const IrAwait(this.operand);

  final IrExpr operand;
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
  const IrNullAware(this.receiver, this.body, {this.flatten = false});

  final IrExpr receiver;
  final IrExpr body;

  /// Whether the body is itself nullable: `a?.b` with `b` a `T?` is one
  /// `Option`, not two -- `and_then`, not `map`.
  final bool flatten;
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
  const IrClosure(
    this.params,
    this.body,
    this.returns, {
    this.captures = const [],
    this.locals = const [],
    this.boxed = false,
    this.holdsSelf = false,
    this.isAsync = false,
  });

  final List<IrParam> params;
  final IrStmt body;
  final IrType returns;

  /// Locals of the enclosing function the body reads, cloned in before the
  /// closure is made and moved into it (see the front end's `_freeLocals`).
  final List<String> locals;

  /// A Dart `async` closure: `async |..| { .. }` in Rust.
  final bool isAsync;

  /// `final` fields of `this` the body reads, copied in when the closure is
  /// made rather than read through a `this` that will not live long enough.
  ///
  /// Sound only because the fields are `final`: round 57 wrote copying off in
  /// general, because Dart reads a field at *call* time and a copy is read at
  /// *creation* time, and between the two the field may have changed. A
  /// `final` field cannot, so for those two the two readings are the same
  /// value. 345 of the 1319 closures that reach `this` are on this side of
  /// that line -- measured by `bin/census_closures.dart`.
  ///
  /// The body reads them as locals; the front end that decides to capture is
  /// the one that rewrites the reads.
  final List<IrParam> captures;

  /// Whether this closure goes to a parameter that keeps it, and so has to be
  /// boxed at the call site to match the owned parameter.
  final bool boxed;

  /// Whether this closure keeps a counted handle to `this`.
  ///
  /// The answer for a closure that *calls a method* -- it cannot capture the
  /// method, only the object. See `IrClass.counted`.
  final bool holdsSelf;
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
/// A promoted read: `other` after `other is Matrix4`, where the variable's
/// own type is `Object`. Dart narrows the variable in place; Rust downcasts
/// the value -- `other.as_any().downcast_ref::<Matrix4>().unwrap()` -- and the
/// field reads that follow are on that. 36 `no field _m4storage on &dyn
/// Object` in `vector_math`'s `==` operators.
/// A call on a `dynamic` slot whose runtime types are known (see the
/// front end's `dynamicSlots`): `dateTimeSymbols[locale]` where the slot
/// starts as an `UninitializedLocaleData` and becomes a `Map`. The
/// receiver is bound to `__d`, and each arm is tried by downcast in order;
/// inside an arm `__d` is the value as that type. The last arm's type may be
/// null: it is the fallback, taken without a test.
class IrDynamicDispatch extends IrExpr {
  const IrDynamicDispatch(this.receiver, this.arms);

  final IrExpr receiver;
  final List<(IrType?, IrExpr)> arms;
}

class IrDowncast extends IrExpr {
  const IrDowncast(this.target, this.type, {this.arguments = const []});

  final IrExpr target;
  final String type;

  /// The struct's type arguments, when it has them: `RadioListTile<T>`.
  final List<IrType> arguments;
}

/// `Some(value)`: a non-null value handed to a nullable parameter. Dart
/// widens silently; Rust's `Option` does not. Emitted where the front end
/// can see both types.
class IrSome extends IrExpr {
  const IrSome(this.value);

  final IrExpr value;
}

/// `(value as f64)`: Dart promotes an `int` to `double` in mixed arithmetic;
/// Rust has no implicit numeric conversion at all (`cannot multiply i64 by
/// f64`, 32 times in the leaf crates).
/// A concrete value shared into a trait object with the target spelled:
/// `(Rc::new(v) as Rc<dyn Curve>)`. A `match` arm does not coerce the way a
/// call argument does, so `curve ?? Curves.ease` needs the `as` written.
class IrUpcast extends IrExpr {
  const IrUpcast(this.value, this.type);

  final IrExpr value;
  final IrType type;
}

class IrCast extends IrExpr {
  const IrCast(this.value, this.rust);

  final IrExpr value;
  final String rust;
}

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
  const IrLocalDecl(this.name, this.type, this.init, {this.cell = false});

  /// A local a closure writes: declared as a shared cell (`Rc<Cell<T>>` or
  /// `Rc<RefCell<T>>`), so the closure's writes are the local's, as Dart's
  /// are. Reads and writes of a cell local go through it (`_cellLocals`).
  final bool cell;

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

/// `topLevel = value` -- a write to a library's own mutable variable.
///
/// Its own node because the storage is its own thing: a Dart top-level
/// variable is one per isolate, and the Rust for it is a `static` with a cell
/// in it, so the write goes through the cell rather than to a name.
/// `Owner.name = value` for a class's mutable static, which lives in a cell
/// the way a mutable top-level does. See `IrConstDecl.isMutable`.
class IrAssignStatic extends IrStmt {
  const IrAssignStatic(this.owner, this.name, this.value);

  final String owner;
  final String name;
  final IrExpr value;
}

class IrAssignTopLevel extends IrStmt {
  const IrAssignTopLevel(this.name, this.value);

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
  const IrAssignField(this.name, this.value, {this.target, this.owner});

  /// The class that declares the field, for a write on another object. See
  /// [IrField.owner]: the write goes through the cell when there is one.
  final String? owner;

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
/// It used to be lookup only: of the 923 uses in `package:flutter/`, 623 look
/// up and 109 iterate, and a Dart map literal is a LinkedHashMap, which walks
/// in **insertion order** while `std::collections::HashMap` does not. Rather
/// than reorder those 109 quietly, the five ordered members were refused --
/// 97 of them across the package.
///
/// The prelude's `Map` keeps the order now (a `Vec` of pairs, the same trade
/// `Set<T>` next door has always made), so they are here.
const mapMethodNames = <String, String>{
  'containsKey': 'contains_key',
  // Its own name: the prelude's takes the key by reference, and a
  // translated class's `remove(String)` was handed a `&String` for
  // sharing the name (56 in `widgets`).
  'remove': '!map_remove',
  'clear': 'clear',
  'isEmpty': 'is_empty',
  'length': 'len',
  'addAll': 'extend',
  // `isNotEmpty` was on the List map and not this one, for no reason anyone
  // wrote down: 10 refusals of a name that was already spelled next door.
  'isNotEmpty': '!is_empty',
  'cast': '!cast',
  'keys': 'keys',
  'values': 'values',
  'entries': 'entries',
  'forEach': 'for_each',
  'putIfAbsent': 'put_if_absent',
};

/// `Map` members that depend on iteration order and are still not translated.
///
/// `map` builds a new map out of `MapEntry`s the closure returns, which is a
/// shape of its own rather than a name; the ordered container did not settle
/// it. The other five are translated now.
const orderedMapMembers = <String>{'map'};

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
  // Dart's `remove(value)` removes the first equal element and says
  // whether it found one; `Vec::remove` takes an index. The prelude's
  // `DartList` supplies the Dart one. 46 calls.
  'remove': 'remove_value',
  // `setRange(start, end, from, [skip])`, in the prelude's `DartList`. 23.
  'setRange': 'set_range',
  // `indexOf(value)` is -1 when absent; the prelude's `DartList` says so. 19.
  'indexOf': 'index_of',
  // `skip(n)`/`take(n)` are lazy Iterables upstream; a `Vec` here, as
  // `reversed` is, since every use ends in a loop or a `toList`.
  'skip': 'skip_dart',
  'take': 'take_dart',
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
  'any': '!any',
  'every': '!every',
  'toSet': '!to_set',
  'join': '!join',
  'insert': '!insert',
  'removeAt': '!remove_at',
  'elementAt': '!element_at',
  'sublist': '!sublist',
  'reversed': '!reversed',
  // Dart's `cast` re-types a list and copies nothing. Rust's types are
  // already what they are, so it is the receiver.
  'cast': '!cast',
};

/// The steps of an iterator chain, in Rust's spelling.
/// `forEach` is the one step that consumes the chain: `for_each` returns
/// `()`, so a chain ending in it is a statement, not a lazy value -- see the
/// backend, which refuses every other uncollected chain. 70 in the gallery.
const iterStepNames = <String, String>{
  'map': 'map',
  'where': 'filter',
  'forEach': 'for_each',
  // `expand(f)` is `flat_map`. 14.
  'expand': 'flat_map',
};

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
    this.shared = false,
    this.isLate = false,
  });

  /// Dart's `late`: declared with no value, and an error to read before one is
  /// assigned.
  ///
  /// Rust has no such state, so the field is `Option<T>` holding `None` until
  /// something writes it, and every read unwraps. That panics where Dart would
  /// have thrown `LateInitializationError` -- the same event, reported by a
  /// different mechanism, which is the trade this compiler already makes for
  /// index bounds.
  ///
  /// Round 52 measured 480 of these and put them down, because a read wanted
  /// `Clone` and the commonest types were `Box<dyn Animation>`, which is not.
  /// Unwrapping a reference wants nothing of the kind: `as_ref().unwrap()` is
  /// a borrow, and only a field held in a cell -- where the borrow cannot
  /// leave -- still needs the clone.
  final bool isLate;

  /// Whether a closure has to see this field change.
  ///
  /// A closure that outlives its call cannot borrow `this`. For a `final`
  /// field a copy *is* the field (see `IrClosure.captures`); for a mutable one
  /// it is not, and the closure and the object have to hold the same cell. So
  /// the field becomes `Rc<Cell<T>>` -- or `Rc<RefCell<T>>` where the value is
  /// not `Copy` -- and the closure keeps a handle.
  ///
  /// Marked for *any* closure that touches it, not only the ones that escape.
  /// Sharing a field that did not need it costs an indirection; not sharing
  /// one that did is a borrow that outlives its borrower, and this compiler
  /// has been on the wrong side of that once already (round 99). 404 fields
  /// across the package, measured by `bin/census_shared.dart`.
  final bool shared;

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

  /// The same field with its type put through [by].
  ///
  /// Flattening copies a base's fields into the subclass, and a generic base
  /// declares them in terms of its own parameters: `DiagnosticsProperty<T>`
  /// has a `T? _value`, and an `ErrorDescription extends
  /// DiagnosticsProperty<String>` has a `String? _value`. Copying without
  /// substituting left the parameter's name standing in a struct that has no
  /// such parameter.
  static IrFieldDecl substituted(IrFieldDecl f, IrType Function(IrType) by) =>
      IrFieldDecl(
        f.name,
        by(f.type),
        isFinal: f.isFinal,
        initial: f.initial,
        doc: f.doc,
        // Carried. Rebuilding a field without it is how `kept` was lost one
        // round ago, in the same shape: the declaration would share and the
        // reads would not.
        shared: f.shared,
        isLate: f.isLate,
      );
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
    this.isMutable = false,
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

  /// A top-level variable that is neither `const` nor `final`: Dart's
  /// `int _n;` at library scope, which anything in the library may assign.
  ///
  /// It is a `static` like the others, and needs interior mutability on top of
  /// the `Isolate` wrapper -- 32 of these were skipped entirely, and reading
  /// one refused the member that read it, 74 times.
  final bool isMutable;
}

/// A class's or method's type parameters, as names.
///
/// Measured before it was built: of `package:flutter/`'s 2743 classes, 234
/// are generic, 221 of those with exactly one parameter and 13 with two; 98
/// methods carry their own. So the feature needed is small.
///
/// Bounds are **dropped**, and that is a stated cost rather than an oversight:
/// 72 of the 247 parameters have one, and a Dart bound names a class where
/// Rust wants a trait. Only an abstract class is a trait here, so most bounds
/// have nothing to become -- and dropping one is more permissive than Dart,
/// which loses a check and cannot make correct code fail.
typedef IrTypeParams = List<String>;

class IrMethod {
  const IrMethod(
    this.name,
    this.params,
    this.returnType,
    this.body, {
    this.fails = false,
    this.typeParameters = const [],
    this.isStatic = false,
    this.isGetter = false,
    this.isSetter = false,
    this.operator,
    this.throws,
    this.doc,
    this.isAsync = false,
  });

  final String name;
  final IrTypeParams typeParameters;
  final List<IrParam> params;
  final IrType returnType;
  final IrStmt body;
  final bool isStatic;
  final bool isGetter;

  /// Whether the member can complete with a Dart exception -- it, or any
  /// member sharing its signature through overriding, throws or calls
  /// something that does (`ThrowsAnalysis.familyFails`). Such a member
  /// returns `Result<T, Rc<dyn Object>>`. See STATUS, 决定 2026-09-04.
  final bool fails;

  /// A `set x(v)`. In Rust it is `set_x(&mut self, v)`, which is why it needs
  /// its own flag: a getter and a setter share a name in Dart and must not in
  /// Rust, and the mutability analysis keys on the Rust name for that reason.
  final bool isSetter;

  /// The Dart operator this method declares, if any: `+`, `unary-`, `[]`.
  final String? operator;

  /// A Dart `async` function. Rust has the same word for the same thing, and
  /// the CFE leaves `await` in the body rather than desugaring it, so the two
  /// line up almost exactly. `async*` and `sync*` do not and stay refused.
  ///
  /// The declared Dart return type is `Future<T>`; a Rust `async fn` returning
  /// `T` already *is* a future, so the wrapper is dropped from the signature.
  final bool isAsync;

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
    this.superName,
    this.superArgs = const [],
    this.doc,
    this.body,
    this.redirectTo,
    this.redirectArgs = const [],
    this.pre = const [],
  });

  /// Statements that run before the fields are set: the temporaries the CFE
  /// binds in the initialiser list (`LocalInitializer`) for a `super(..)`
  /// argument used twice, and which the tree shaker's AOT lowering writes
  /// far more of -- 67 constructors in the gallery's tree-shaken dill.
  final List<IrStmt> pre;

  /// `Foo.bar(a) : this(a, 0);` -- the constructor this one hands its
  /// arguments to, by name, with the unnamed one as `null`. Dart forbids any
  /// other initialiser beside a redirect, so `fieldInits` is empty when this
  /// is set, and the Rust is one call: `Self::new(a, 0)`.
  final String? redirectTo;
  final List<IrExpr> redirectArgs;

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

  /// The base constructor `super(..)` names -- `super._(..)` -- or null for
  /// the unnamed one.
  final String? superName;
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
    this.typeParameters = const [],
    this.superclass,
    this.superclassArguments = const [],
    this.mixins = const [],
    this.interfaces = const [],
    this.counted = false,
    this.isAbstract = false,
    this.isEnum = false,
    this.values = const [],
    this.valueFields = const {},
    this.doc,
  });

  final String name;

  /// `class Foo<T>` -- the names, in order. See [IrTypeParams].
  final IrTypeParams typeParameters;
  final String? superclass;

  /// The classes mixed in: `class Panel extends Measured with Scaled` -- the
  /// `Scaled`. A mixin is a base like any other as far as Rust is concerned --
  /// its methods have to be reachable through a trait the class implements --
  /// but it does not sit on the `extends` chain, so nothing found it. Kernel
  /// hides it further still: it writes `Panel extends _Measured&Scaled`, and
  /// that synthetic class is skipped, taking the mixin with it.
  /// Carried as types, not names: a mixin is often generic --
  /// `ContainerRenderObjectMixin<ChildType, ParentDataType>` -- and the `impl`
  /// needs the arguments as much as an extended class does. Recording only the
  /// name refused 554 impls with "the base is generic and its arguments are
  /// not known here", which was true and avoidable.
  final List<IrType> mixins;

  /// The `implements` clause.
  ///
  /// A third way to reach a base, and the one nothing here had: Dart's
  /// `implements` promises the members without inheriting the bodies, which in
  /// Rust is exactly an `impl Trait for Struct` whose methods all have to be
  /// written. Carrying only the superclass and the mixins meant a class that
  /// implemented an abstract one got **no impl block at all** -- so nothing
  /// could hold it as that trait, and the fixture for `is` is what showed it.
  /// 216 classes under the package implement something they do not otherwise
  /// reach, and 214 of those interfaces are abstract.
  final List<IrType> interfaces;

  /// Whether instances of this class are reference counted.
  ///
  /// A closure cannot capture a method, only an object -- so a closure that
  /// calls a method on `this` and outlives the call has to keep a handle to
  /// it. 414 closures across the package do that, in 197 classes.
  ///
  /// It is the *type* that carries this, not the sites: `Rc<Ticker>` wherever
  /// `Ticker` appears. Round 102 counted 1150 places where one of these
  /// classes is constructed, held, passed or returned, and one rule in
  /// `type()` answers all of them.
  final bool counted;

  /// `class _Linear extends ParametricCurve<double>` -- the `double`.
  ///
  /// Needed for the `impl`: Rust wants `impl ParametricCurve<f32> for _Linear`,
  /// and the base's name alone does not say what to put in the angle brackets.
  final List<IrType> superclassArguments;

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

  /// What each variant carries, when a Dart enum gave its values fields of
  /// their own: variant name -> field name -> the Rust literal.
  ///
  /// Constants of the variant, not runtime state, so the Rust is a `match` in
  /// a method rather than a payload on the enum.
  final Map<String, Map<String, String>> valueFields;

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
  IrLibrary(
    this.classes, {
    this.constants = const [],
    this.functions = const [],
    this.abstractElsewhere = const {},
    this.elsewhere = const {},
    this.functionsElsewhere = const {},
    this.constantsElsewhere = const {},
  });

  /// Top-level constants of every other module, by name: a read of one
  /// here has to know whether it is a lazily built `static` (`_isLazyConst`).
  final Map<String, IrConstDecl> constantsElsewhere;

  /// Top-level functions in the *other* modules of the same crate, by name.
  ///
  /// `isDisplayDesktop(context)`, `mergeSort(list)`, `sqrt(3)` -- a top-level
  /// function is reachable wherever it lives, and `use crate::<module>::*`
  /// already brings it in. Only the check was still asking whether the callee
  /// was in *this* library, which it stopped needing to be once the whole
  /// package became one crate.
  ///
  /// Holds only what was lowered. `dart:ffi`'s `_fromAddress` is not in the
  /// crate at all, so a call to it stays refused, which is right.
  final Set<String> functionsElsewhere;

  /// Classes in the *other* modules of the same crate, by name.
  ///
  /// A base class is a base class wherever it lives. While one library was
  /// emitted at a time this map was empty and a base from elsewhere was a
  /// hand-written stub whose fields and constructors could not be known -- so
  /// every subclass of one was refused, 1300 of them. In one crate the base
  /// really is there, and `use crate::<module>::*` reaches it.
  final Map<String, IrClass> elsewhere;

  /// Abstract classes declared in *other* libraries of the same crate.
  ///
  /// Whether a class is abstract decides whether its name is a struct or a
  /// `dyn Trait`, and a library only knows its own classes. One library at a
  /// time that was harmless -- a name from elsewhere was a hand-written stub
  /// anyway. A whole package in one crate is a different matter: 802 `E0782`
  /// errors, every one a trait named without `dyn`, because the library
  /// holding the trait was not the library using it.
  final Set<String> abstractElsewhere;

  final List<IrClass> classes;

  /// Top-level functions. 198 of them under `package:flutter/`, called 522
  /// times -- and until round 36 this compiler translated classes only, so
  /// every one of those calls took its member down with it.
  final List<IrMethod> functions;

  /// Top-level `const`s and `final`s. Dart puts 241 of them under
  /// `package:flutter` alone -- `kMinInteractiveDimension`, `kIsWeb` -- and
  /// they are referred to 507 times.
  final List<IrConstDecl> constants;

  IrClass? operator [](String? name) {
    if (name == null) return null;
    for (final c in classes) {
      if (c.name == name) return c;
    }
    return elsewhere[name];
  }

  bool isAbstract(String? name) =>
      this[name]?.isAbstract ??
      (name != null && abstractElsewhere.contains(name));
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
