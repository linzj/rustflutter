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
import 'package:kernel/class_hierarchy.dart';
import 'package:kernel/type_algebra.dart';
import 'package:kernel/type_environment.dart';

import 'throws.dart';
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
    this.enumFields = const {},
    this.abstractElsewhere = const {},
    this.elsewhere = const {},
    this.typeEnvironment,
    this.dynamicSlots = const {},
    this.throws,
    this.open = const {},
    this.erase = false,
  });

  /// Whether type parameters bounded by translated abstract classes are
  /// erased (`_erasedParameter`). Gated while the cut was finished: on, ws281
  /// measured 5341 against the gate's 5247, with three follow-ups owed --
  /// a cast to an open class's trait for `widget` reads, field reads through
  /// a downcast of a generic value struct, and callbacks typed with the
  /// erased parameter. With those in, ws290 measured 5352/883 against the
  /// gate's 5358/887, and it is the default; `DART2RUST_ERASE=0` turns it
  /// off.
  final bool erase;

  /// Concrete classes with subclasses, lowered as a trait plus a struct
  /// (`XImpl`) for their own instances: a subclass instance can then sit in
  /// a slot typed by the base, which a value struct never allowed
  /// (`ParentData` 138 mismatches, `Color` 178, 2026-09-04).
  final Set<Class> open;

  bool _isOpen(Class c) => open.contains(c);

  /// Abstract on the Rust side: a Dart abstract class, or an open one.
  bool _abstractLike(Class c) => c.isAbstract || _isOpen(c);

  /// The struct behind an open class's own instances.
  static String implName(String name) => '${name}Impl';

  /// Which members can fail, over the whole program (`ThrowsAnalysis`);
  /// null while the Result model is off.
  final ThrowsAnalysis? throws;

  /// Whether a call to `target` yields a `Result` to propagate: a member
  /// of a translated library that is a function (a field's accessor is
  /// plain) and not `async` (its exceptions go into the future, and the
  /// `?` goes after the `.await`). Uniform model: the analysis (`throws`)
  /// no longer decides, it only gates the model.
  bool _fails(Member target) {
    if (throws == null) return false;
    // A field read through an accessor call is a function call too; an
    // enum's carried fields are plain methods on the enum.
    if (target is Field) {
      if (target.isStatic) return false;
      if (target.enclosingClass?.isEnum ?? false) return false;
    }
    if (target is Procedure &&
        target.function.asyncMarker == AsyncMarker.Async) {
      return false;
    }
    // A member cloned into a mixin application lives in a synthetic
    // library: translated like the mixin's (`current_down` on
    // `_TapStatusTrackerMixin`, 40 calls without `?`).
    if (target.enclosingClass?.isAnonymousMixin ?? false) return true;
    final uri = target.enclosingLibrary.importUri;
    return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
  }

  /// Top-level `dynamic` fields whose runtime types the driver worked out
  /// from the initialiser and every store into them (`dynamicSlotsIn`):
  /// `dateTimeSymbols` holds an `UninitializedLocaleData` and then a `Map`.
  /// A call on such a slot dispatches by downcast (`IrDynamicDispatch`).
  final Map<Field, List<InterfaceType>> dynamicSlots;

  /// The whole program's types, for `getStaticType`. Built once by the
  /// driver; null in the tools that lower a single library on its own, which
  /// then do without the two things it buys -- `Some(..)` around a non-null
  /// argument to a nullable parameter, and `as f64` in mixed arithmetic.
  final TypeEnvironment? typeEnvironment;
  StaticTypeContext? _typeContext;

  /// The member being lowered; its class is what `this` is.
  Member? _member;

  /// The class being lowered -- the named one, while the members of the
  /// anonymous mixin applications above it are lowered into it.
  Class? _lowering;

  void _enter(Member member) {
    _member = member;
    final env = typeEnvironment;
    _typeContext = env == null ? null : StaticTypeContext(member, env);
    _capturedWrites = _CapturedWrites.of(member);
  }

  /// The locals of the member being lowered that a closure inside it
  /// writes: `sum += v` inside a `forEach` callback. Each is declared as a
  /// shared cell (`IrLocalDecl.cell`), so the closure's write is the
  /// local's -- a plain `let` copied into the closure would have kept the
  /// sum to itself, and the fixture crate's `total` said 0.
  Set<Variable> _capturedWrites = const {};

  DartType? _staticType(Expression e) {
    // An instance constant is its own class before it is the slot's declared
    // type -- `getStaticType` answers `Curve` for `Curves.linear`, and the
    // `_Linear` value was never shared into the `Rc<dyn Curve>` (98).
    if (e is ConstantExpression && e.constant is InstanceConstant) {
      return _constantStaticType(e.constant);
    }
    final context = _typeContext;
    if (context != null) {
      try {
        return e.getStaticType(context);
      } catch (_) {
        // Fall through to what the node itself records.
      }
    }
    // Without a context (or when `getStaticType` gives up) the node still
    // carries its type: `Zone.current[_clockKey] as Clock?` had its `as`
    // dropped for want of knowing the operand was `dynamic`.
    if (e is InstanceInvocation) return e.functionType.returnType;
    if (e is InstanceGet) return e.resultType;
    if (e is VariableGet) return e.promotedType ?? e.variable.type;
    if (e is StaticGet) return e.target.getterType;
    if (e is StaticInvocation) return e.target.function.returnType;
    if (e is ConstructorInvocation) return e.constructedType;
    if (e is StaticTearOff) {
      return e.target.function.computeFunctionType(Nullability.nonNullable);
    }
    // A tear-off of a static method is a *constant* in Kernel
    // (`DateFormat.localeExists` as an argument), with its type on the node.
    // An instance constant is its own class, not the slot's declared type:
    // `Curves.linear` filling an omitted `Curve curve` arrives typed `Curve`
    // and was never shared into the `Rc<dyn Curve>` (109 `Cubic`, 88
    // `_Linear`, 75 `BorderRadius`).
    if (e is ConstantExpression) {
      final c = e.constant;
      return c is InstanceConstant ? _constantStaticType(c) : e.type;
    }
    if (e is NullLiteral) return const NullType();
    // Literals, from the core types when there are any: `return true` in a
    // closure returning `Object?` had nothing to say it was a `bool`.
    final core = typeEnvironment?.coreTypes;
    if (core != null) {
      if (e is BoolLiteral) return core.boolNonNullableRawType;
      if (e is IntLiteral) return core.intNonNullableRawType;
      if (e is DoubleLiteral) return core.doubleNonNullableRawType;
      if (e is StringLiteral) return core.stringNonNullableRawType;
      // An interpolation is a `String` too: without a type the return of
      // one into a `String?` was never wrapped in `Some` (40 in intl).
      if (e is StringConcatenation) return core.stringNonNullableRawType;
    }
    return null;
  }

  /// Classes in the rest of the crate. See [IrLibrary.elsewhere].
  final Map<String, IrClass> elsewhere;

  /// Abstract classes in the rest of the crate. See [IrLibrary].
  final Set<String> abstractElsewhere;

  final Library library;

  /// An enum class to its variant names, in `index` order.
  ///
  /// Empty by default, and then an enum whose fields the compiler dropped
  /// comes out with no values -- which the backend says plainly rather than
  /// emitting an enum with no variants. See `enumValuesIn`.
  final Map<Class, List<String>> enumValues;

  /// What each enum variant carries. See `enumsIn`.
  final Map<Class, Map<String, Map<String, String>>> enumFields;
  String? _superclass;

  // -- Types ------------------------------------------------------------------

  IrType _type(DartType type) {
    final nullable = type.nullability == Nullability.nullable;
    if (type is InterfaceType) {
      // `dart:core`'s `Iterator` would shadow `std::iter::Iterator` in every
      // module: it is the prelude's `DartIterator`.
      final core =
          type.classNode.enclosingLibrary.importUri.toString() == 'dart:core';
      final name = core && type.classNode.name == 'Iterator'
          ? 'DartIterator'
          : type.classNode.name;
      return IrType(
        name,
        nullable: nullable,
        arguments: _erasedArguments(type.classNode, type.typeArguments),
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
    // The bottom type. Thirty in the gallery's dill: `noSuchMethod`s declared
    // `Never`, and a few `Foo<Never>`. The backend spells it two ways.
    if (type is NeverType) return const IrType('Never');
    if (type is DynamicType) return const IrType('dynamic');
    if (type is NullType) return const IrType('Null', nullable: true);
    if (type is TypeParameterType) {
      // An erased parameter is its bound (see `_erasedParameter`).
      if (_erasedParameter(type.parameter)) {
        final asBound = _type(type.parameter.bound);
        return IrType(
          asBound.name,
          nullable: nullable || asBound.nullable,
          arguments: asBound.arguments,
        );
      }
      // `T extends String` is a `String` here: the bound is what the body
      // calls methods on, and a Rust type parameter has no such methods.
      // Only for the scalar bounds that are prelude types; `T extends
      // Comparable<T>` would recurse.
      final bound = type.parameter.bound;
      if (bound is InterfaceType &&
          const {
            'String',
            'num',
            'int',
            'double',
            'bool',
          }.contains(bound.classNode.name)) {
        // The bound's own nullability comes along: intl's `T extends
        // String?` was a `String` here, and its `String?` return a
        // `String` (40 `Option<String>` <- `String`).
        return IrType(
          _type(bound).name,
          nullable: nullable || bound.nullability == Nullability.nullable,
        );
      }
      // `T extends Iterable<E>`: the `Vec<E>` the bound is, since the body
      // iterates it (collection's `IterableEquality`, 6).
      if (bound is InterfaceType &&
          const {'Iterable', 'List'}.contains(bound.classNode.name)) {
        final asBound = _type(bound);
        return IrType(
          asBound.name,
          nullable: nullable,
          arguments: asBound.arguments,
        );
      }
      return IrType(type.parameter.name ?? 'T', nullable: nullable);
    }
    if (type is FunctionType) {
      // Named parameters after the positional ones, **sorted by name**, as
      // a closure declares them: `LogWriterCallback = void Function(String
      // text, {bool isError})` is an `Fn(String, bool)`, and a field of
      // that type could not hold the two-parameter function (E0593).
      final named = [...type.namedParameters]
        ..sort((a, b) => a.name.compareTo(b.name));
      return IrType.function(
        [
          for (final p in type.positionalParameters) _paramType(p),
          for (final p in named) _paramType(p.type),
        ],
        _type(type.returnType),
        nullable: nullable,
      );
    }
    // Kernel's own class name is not a Dart type name. `FutureOr<T>` arrived
    // as the type `FutureOrType`, which nothing declares and which reads, in
    // the output, exactly like a class the compiler had translated. A name
    // this compiler invented is worse than no name: refusing says where the
    // gap is, and `FutureOr` is a real gap -- it is "T, or a future of T",
    // which Rust would need an enum to say.
    throw Unsupported('the type `$type`', '$type');
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
      final written = node.variable.cosmeticName;
      if (temporary == null && (written == null || written.startsWith('#'))) {
        throw Unsupported('synthetic variable', _sample(node));
      }
      // A temporary is promoted like any local: `if (__t != null)
      // xs.add(__t)` after the CFE's lowering of `?.`/`??` (`String <=
      // Option<String>`). It used to return before the checks below.
      final name = temporary ?? written!;
      // A closure parameter retyped to an erased bound (`_closureParamType`)
      // reads as the class it was declared with.
      final retyped = _retyped[node.variable];
      final declaredVar = node.variable.type;
      if (retyped is TypeParameterType &&
          _erasedParameter(retyped.parameter) &&
          declaredVar is InterfaceType &&
          declaredVar.nullability != Nullability.nullable) {
        final bound = retyped.parameter.bound;
        if (bound is InterfaceType &&
            bound.classNode != declaredVar.classNode) {
          if (_abstractLike(declaredVar.classNode)) {
            return IrCastTo(IrLocal(name), _type(declaredVar));
          }
          return _narrowingCast(IrLocal(name), declaredVar);
        }
      }
      // Promoted to a concrete class the declaration does not name: the
      // read is a downcast. Promotion to the *same* class (nullable to
      // non-null) is not.
      final promoted = node.promotedType;
      final declared = node.variable.type;
      // The core scalars are abstract classes in Kernel and structs here:
      // an `Object?` promoted to `String` is a downcast to `String`.
      const scalars = {'String', 'int', 'double', 'bool', 'num'};
      if (promoted is InterfaceType &&
          (!_abstractLike(promoted.classNode) ||
              scalars.contains(promoted.classNode.name)) &&
          !promoted.classNode.isEnum &&
          !(declared is InterfaceType &&
              declared.classNode == promoted.classNode)) {
        final downcast = IrDowncast(
          IrLocal(name),
          _rustScalar(_type(promoted).name),
        );
        // Cloned out of the reference `Any` hands back.
        return IrCall(downcast, 'clone', const []);
      }
      // ..and to an abstract or open class: the trait cast every object
      // answers (`dart_cast_to`). Not from a nullable declaration, whose
      // `Option` the null-promotion below takes off first.
      if (promoted is InterfaceType &&
          _abstractLike(promoted.classNode) &&
          !scalars.contains(promoted.classNode.name) &&
          promoted.nullability != Nullability.nullable &&
          !(declared is InterfaceType &&
              declared.classNode == promoted.classNode) &&
          !(declared is InterfaceType &&
              declared.nullability == Nullability.nullable)) {
        return IrCastTo(IrLocal(name), _type(promoted));
      }
      // Promoted from `T?` to `T` -- `if (x != null) f(x)` -- the read is
      // the value inside. A clone first, so the local is still there for
      // the next read: `&Option<Hct>` where `&Hct` was wanted, 12 times.
      if (promoted != null &&
          declared is! DynamicType &&
          promoted.nullability == Nullability.nonNullable &&
          declared.nullability == Nullability.nullable) {
        return IrNullCheck(IrCall(IrLocal(name), 'clone', const []));
      }
      return IrLocal(name);
    }
    if (node is InstanceGet) return _instanceGet(node);
    if (node is StaticGet) return _staticGet(node);
    if (node is InstanceInvocation) return _instanceInvocation(node);
    if (node is BlockExpression) return _blockValue(node);
    if (node is FunctionInvocation) {
      // A call through a function value: no callee to read defaults from,
      // only its type -- which is enough to *order* named arguments, and the
      // closures on the other end are declared in that same order.
      final type = node.functionType;
      if (type == null) {
        throw Unsupported(
          'call of a function value with no type',
          _sample(node),
        );
      }
      return IrCallValue(
        expression(node.receiver),
        _argumentsByType(node.arguments, type),
      );
    }
    if (node is LocalFunctionInvocation) {
      final name = node.variable.cosmeticName;
      if (name == null) {
        throw Unsupported('call of an unnamed local function', _sample(node));
      }
      // A local function is declared as a closure, so its named parameters
      // are in type order there too.
      return IrCallValue(
        IrLocal(name),
        _argumentsByType(node.arguments, node.functionType),
      );
    }
    if (node is FunctionExpression) return _closure(node.function, node);
    if (node is StaticSet) {
      // `Owner.x = v` where the value is wanted -- the `??=` on a static, in
      // the CFE's `let #t = X in #t == null ? X = v : #t`. Bind the value,
      // store a clone, produce the binding: the store moves, and the value
      // still has to come out.
      final target = node.target;
      if (target is Procedure &&
          target.kind == ProcedureKind.Setter &&
          target.enclosingClass == null) {
        final held = '__t${_nextTemporary++}';
        final stored = _widened(
          node.value,
          target.function.positionalParameters.single.type,
          IrCall(IrLocal(held), 'clone', const []),
        );
        return IrBlockValue([
          IrLocalDecl(held, null, expression(node.value)),
          IrExprStmt(
            IrStaticCall(null, _topLevelSetterName(target.name.text), [stored]),
          ),
        ], IrLocal(held));
      }
      if (target is! Field) {
        throw Unsupported('static setter used for its value', _sample(node));
      }
      final owner = target.enclosingClass;
      final held = '__t${_nextTemporary++}';
      // The store widens into the static's type: `_decomposeV ??=
      // Vector3.zero()` on a `Vector3?` stores `Some(..)`.
      final write = _widened(
        node.value,
        target.type,
        IrCall(IrLocal(held), 'clone', const []),
      );
      return IrBlockValue([
        IrLocalDecl(held, null, expression(node.value)),
        owner == null
            ? IrAssignTopLevel(target.name.text, write)
            : IrAssignStatic(owner.name, target.name.text, write),
      ], IrLocal(held));
    }
    if (node is Let) return _let(node);
    if (node is EqualsNull) return IrIsNull(expression(node.expression));
    if (node is EqualsCall) {
      // `_argb == other._argb` with one side promoted: an `Option<i64>`
      // against an `i64` does not compare; the non-null side is `Some`d.
      var left = expression(node.left);
      var right = expression(node.right);
      final leftType = _staticType(node.left);
      final rightType = _staticType(node.right);
      bool nullable(DartType? t) =>
          t != null &&
          t is! DynamicType &&
          t.nullability == Nullability.nullable;
      bool plain(DartType? t) =>
          t != null &&
          t is! DynamicType &&
          t is! NullType &&
          t.nullability != Nullability.nullable;
      if (nullable(leftType) && plain(rightType) && !_isNull(node.right)) {
        right = _widened(node.right, leftType, right);
      } else if (plain(leftType) &&
          nullable(rightType) &&
          !_isNull(node.left)) {
        left = _widened(node.left, rightType, left);
      }
      // Two closures are equal when they are the same closure: `Rc<dyn Fn>`
      // has no `==`, and the prelude's `dart_eq` is identity (8 in `listen`).
      if (leftType is FunctionType || rightType is FunctionType) {
        return IrCall(left, '!dart_eq', [right]);
      }
      // `integer == 0` with `integer` a `dynamic` holding a number: the
      // `dynamic` side is the `f64` its arithmetic made it (see the
      // `numOperators` lowering), and the literal side is cast to match.
      bool number(DartType? t) =>
          t is InterfaceType &&
          (t.classNode.name == 'int' ||
              t.classNode.name == 'double' ||
              t.classNode.name == 'num');
      String? numClass(DartType? t) =>
          t is InterfaceType ? t.classNode.name : null;
      if (leftType is DynamicType && number(rightType)) {
        final asDouble = IrCall(IrDowncast(left, 'f64'), 'clone', const []);
        final other = numClass(rightType) == 'int'
            ? IrCast(right, 'f64')
            : right;
        return IrBinary('==', asDouble, other);
      }
      if (rightType is DynamicType && number(leftType)) {
        final asDouble = IrCall(IrDowncast(right, 'f64'), 'clone', const []);
        final other = numClass(leftType) == 'int' ? IrCast(left, 'f64') : left;
        return IrBinary('==', other, asDouble);
      }
      // `lightOption == -1` on a `double`: the `int` side is cast, as the
      // arithmetic operators cast theirs.
      String? cls(DartType? t) => t is InterfaceType ? t.classNode.name : null;
      if (cls(leftType) == 'double' && cls(rightType) == 'int') {
        right = IrCast(right, 'f64');
      } else if (cls(leftType) == 'int' && cls(rightType) == 'double') {
        left = IrCast(left, 'f64');
      } else if (_declaredNum(node.left) &&
          (node.right is IntLiteral || cls(rightType) == 'int')) {
        right = IrCast(right, 'f64');
      } else if (_declaredNum(node.right) &&
          (node.left is IntLiteral || cls(leftType) == 'int')) {
        left = IrCast(left, 'f64');
      }
      return IrBinary('==', left, right);
    }
    // A truly dynamic call -- `number.abs()` on a `dynamic` in intl's
    // NumberFormat -- when the name is one of `num`'s: the receiver is
    // downcast to the `f64` a `num` is here (see the devirtualised case).
    const numMethods = _dynamicNumMethods;
    if (node is DynamicInvocation && numMethods.contains(node.name.text)) {
      final asDouble = IrCall(
        IrDowncast(expression(node.receiver), 'f64'),
        'clone',
        const [],
      );
      final call = IrCall(asDouble, node.name.text, [
        for (final a in node.arguments.positional) expression(a),
      ]);
      return const {
            'round',
            'floor',
            'ceil',
            'truncate',
            'toInt',
          }.contains(node.name.text)
          ? IrCast(call, 'i64')
          : call;
    }
    // ..and its operators: `number - integerPart` on a `dynamic`.
    const numOperators = {'+', '-', '*', '/', '%', '<', '>', '<=', '>=', '~/'};
    if (node is DynamicInvocation || node is DynamicGet) {
      final dispatched = _dynamicSlotCall(node);
      if (dispatched != null) return dispatched;
    }
    if (node is DynamicInvocation &&
        numOperators.contains(node.name.text) &&
        node.arguments.positional.length == 1) {
      final asDouble = IrCall(
        IrDowncast(expression(node.receiver), 'f64'),
        'clone',
        const [],
      );
      var right = expression(node.arguments.positional.single);
      final rightType = _staticType(node.arguments.positional.single);
      if (rightType is DynamicType) {
        right = IrCall(IrDowncast(right, 'f64'), 'clone', const []);
      } else if (rightType is InterfaceType &&
          rightType.classNode.name == 'int') {
        right = IrCast(right, 'f64');
      }
      return IrBinary(node.name.text, asDouble, right);
    }
    if (node is DynamicGet && numMethods.contains(node.name.text)) {
      final asDouble = IrCall(
        IrDowncast(expression(node.receiver), 'f64'),
        'clone',
        const [],
      );
      return IrCall(asDouble, node.name.text, const []);
    }
    if (node is Not) {
      // `x is! T` is a `Not` around an `IsExpression` here; the analyzer keeps
      // it as one node with a flag. Folded so the two front ends write the
      // same Rust -- `is_none()`, not `!(..is_some())` -- which is the whole
      // point of having two of them.
      final inner = node.operand;
      if (inner is IsExpression) {
        return IrIs(
          expression(inner.operand),
          _type(inner.type),
          negated: true,
        );
      }
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
      // A condition the AOT compiler replaced by its "removed" throw: the
      // whole conditional is dead, and its branches' types no longer meet.
      final condition = node.condition;
      if (condition is Throw && _tfaUnreachable(condition)) return _unreachable;
      // `x != null ? Color(..) : "unspecified"` inside a string: the branches
      // are of different classes and the result is `Object`, so both go
      // through `dart_str` (see the `??` case).
      final staticType = node.staticType;
      final thenType = _staticType(node.then);
      final elseType = _staticType(node.otherwise);
      if (staticType is InterfaceType &&
          staticType.classNode.name == 'Object' &&
          thenType is InterfaceType &&
          elseType is InterfaceType &&
          thenType.classNode != elseType.classNode) {
        return IrConditional(
          expression(condition),
          IrStaticCall(null, 'dart_str', [expression(node.then)]),
          IrStaticCall(null, 'dart_str', [expression(node.otherwise)]),
        );
      }
      // Each branch widens into the conditional's own type: `m == null ?
      // null : hashAll(m)` is an `Option`, and the second branch an `i64`
      // until it is wrapped (4 `if` and `else` have incompatible types).
      return IrConditional(
        expression(condition),
        _widened(node.then, staticType, expression(node.then)),
        _widened(node.otherwise, staticType, expression(node.otherwise)),
      );
    }
    if (node is TypeLiteral) return _typeLiteral(node.type);
    if (node is IsExpression) {
      return IrIs(expression(node.operand), _type(node.type));
    }
    if (node is ConstructorInvocation) return _construct(node);
    if (node is StaticInvocation) return _staticInvocation(node);
    if (node is SuperMethodInvocation) {
      // The target member is already resolved -- this is the fact the analyzer
      // front end had to work out for itself.
      final owner = _realOwner(node.interfaceTarget, node.name.text)?.name;
      if (owner == null) {
        throw Unsupported('super call with no owner', '$node');
      }
      // The super target is resolved, so its parameter list orders the named
      // arguments -- 56 super calls with named arguments were refused for
      // want of a callee this line had all along.
      return IrSuperCall(
        owner,
        node.name.text,
        _arguments(node.arguments, node.interfaceTarget.function),
      );
    }
    if (node is VariableSet) {
      // A value the AOT compiler removed: the assignment never happens and
      // the temporary it would bind has no type (`let __t74 =
      // unreachable!(..)`, 115 "type annotations needed").
      if (node.value is Throw && _tfaUnreachable(node.value as Throw)) {
        return _unreachable;
      }
      // `x = v` used for its value. Rust's assignment produces `()`, so the
      // value is bound, assigned and produced -- the same shape a field write
      // used for its value takes.
      final written = node.variable.cosmeticName;
      final known = _temporaries[node.variable];
      if (known == null && (written == null || written.startsWith('#'))) {
        throw Unsupported('assignment used for its value', _sample(node));
      }
      final name = known ?? written!;
      // Into a `dynamic` local (`dynamic result = scaled(x)` in vector_math's
      // `operator *`) the value is shared into its `Rc<dyn Object>`.
      final stored = _intoObject(
        node.value,
        node.variable.type,
        _widened(node.value, node.variable.type, expression(node.value)),
      );
      // `(index = s.indexOf(p)) >= 0` with `int? index`: the store is
      // `Some(..)`, the value of the expression is not.
      if (stored is IrSome) {
        final held = '__t${_nextTemporary++}';
        return IrBlockValue([
          IrLocalDecl(held, null, stored.value),
          IrAssign(name, IrSome(IrCall(IrLocal(held), 'clone', const []))),
        ], IrLocal(held));
      }
      return IrAssignValue(name, stored);
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
      return _mapLiteral(node, node.keyType, node.valueType);
    }
    if (node is ListLiteral) {
      return _listLiteral(node, node.typeArgument);
    }
    if (node is StringConcatenation) {
      // A part that is neither text nor a number goes through `dart_str`
      // (the prelude's `Debug` rendering); the primitives print as they are.
      IrExpr part(Expression e) {
        final lowered = expression(e);
        if (e is StringLiteral || lowered is IrLiteral) return lowered;
        final type = _staticType(e);
        final name = type is InterfaceType ? type.classNode.name : null;
        const plain = {'String', 'int', 'double', 'num', 'bool', 'Null'};
        if (name != null &&
            plain.contains(name) &&
            type!.nullability != Nullability.nullable) {
          return lowered;
        }
        return IrStaticCall(null, 'dart_str', [lowered]);
      }

      return IrInterpolation([for (final e in node.expressions) part(e)]);
    }
    if (node is SuperPropertyGet) {
      final owner = node.interfaceTarget?.enclosingClass?.name;
      if (owner == null) {
        throw Unsupported('super property with no owner', _sample(node));
      }
      if (node.interfaceTarget is Field) {
        // A base field is copied into the subclass struct by the flattening,
        // so `super.x` and `this.x` are the same storage.
        return IrField(
          null,
          node.name.text,
          onEnum: node.interfaceTarget?.enclosingClass?.isEnum ?? false,
        );
      }
      return IrSuperCall(owner, node.name.text, const []);
    }
    if (node is AwaitExpression) {
      // `await <throw>`: the tree shaker replaces a removed call with a
      // throw, and there is nothing to await in a throw -- `.await` on a
      // `return Err(..)` is what came out.
      if (node.operand is Throw) return expression(node.operand);
      return IrAwait(expression(node.operand));
    }
    if (node is Throw) {
      if (_tfaUnreachable(node)) return _unreachable;
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
        // `entry.x = v` where the value is wanted, on a local or parameter:
        // the same two shapes the statement form takes -- a local owning a
        // value, or a handle to a counted class whose fields are cells --
        // bound first, written as a clone, produced last. 66 of these.
        final receiver = node.receiver;
        final target = node.interfaceTarget;
        final declaring = target.enclosingClass;
        final onLocal = receiver is VariableGet;
        // A local, a parameter, or a chain rooted at `this` -- the receivers
        // the statement form already takes.
        // A value the AOT compiler removed: the store never happens and
        // the expression has no type to bind (`let __t = unreachable!(..)`,
        // 56 "type annotations needed").
        if (node.value is Throw && _tfaUnreachable(node.value as Throw)) {
          return _unreachable;
        }
        if ((onLocal || _rootedAtThis(receiver)) && declaring != null) {
          final receiverClass = _staticClass(receiver);
          final counted =
              _closureCallsMethod(declaring) ||
              (receiverClass != null && _closureCallsMethod(receiverClass));
          final ownsValue =
              !onLocal || receiver.variable.parent is! FunctionNode;
          if (counted || ownsValue) {
            final held = '__t${_nextTemporary++}';
            final clone = IrCall(IrLocal(held), 'clone', const []);
            // Into a nullable field the store is `Some(..)`.
            final stored = _widened(node.value, target.setterType, clone);
            return IrBlockValue([
              // Inferred: the field's *declared* type is the generic `T?` of
              // `Tween<T>`, and spelling it put a `T` into a class with none.
              IrLocalDecl(held, null, expression(node.value)),
              // A field goes through storage (a cell when the class is
              // counted); a setter is a call, on whatever the receiver is.
              target is Field
                  ? IrAssignField(
                      node.name.text,
                      stored,
                      target: expression(receiver),
                      owner: counted
                          ? (_receiverClassName(receiver) ?? declaring.name)
                          : null,
                    )
                  : IrSetter(
                      expression(receiver),
                      node.name.text,
                      stored,
                      qualifier: _setterQualifier(receiver, target),
                      receiverClass: _classNameOf(receiver),
                    ),
            ], IrLocal(held));
          }
        }
        throw Unsupported(
          'assignment to another object used for its value '
          '(${_shape(node.receiver)})',
          _sample(node),
        );
      }
      if (node.interfaceTarget is! Field) {
        throw Unsupported('setter call used for its value', _sample(node));
      }
      final stored = _widened(
        node.value,
        node.interfaceTarget.setterType,
        expression(node.value),
      );
      if (stored is IrSome) {
        // `_cache = s` into a `String?` field, used for its value: the
        // store is `Some(s)`, the value is `s`.
        final held = '__t${_nextTemporary++}';
        return IrBlockValue([
          IrLocalDecl(held, null, stored.value),
          IrAssignField(
            node.name.text,
            IrSome(IrCall(IrLocal(held), 'clone', const [])),
          ),
        ], IrLocal(held));
      }
      return IrSetValue(null, node.name.text, stored);
    }
    if (node is NullCheck) {
      // `lerpDouble(a, b, t)!`: the special lowering of `lerpDouble` (see
      // `_staticInvocation`) is already a bare `f64`; nothing to unwrap.
      final operand = node.operand;
      if (operand is StaticInvocation &&
          operand.target.name.text == 'lerpDouble') {
        return expression(operand);
      }
      return IrNullCheck(expression(operand));
    }
    if (node is AsExpression) {
      // A cast that only removes `?` -- the CFE's spelling of a promoted
      // private field, `_hct` after `if (_hct != null)` -- is a null check.
      // Any other cast is the operand: Rust's types are already the
      // concrete ones. 12 `&Option<Hct>` where `&Hct` was wanted.
      final from = _staticType(node.operand);
      final to = node.type;
      if (from != null &&
          from.nullability == Nullability.nullable &&
          to.nullability == Nullability.nonNullable &&
          from is InterfaceType &&
          to is InterfaceType &&
          from.classNode == to.classNode) {
        return IrNullCheck(expression(node.operand));
      }
      // A cast down from an abstract class to a concrete one -- `path as
      // _NativePath` in front of every native taking one -- is a downcast
      // through `Any`, and the value is cloned out of the reference it
      // yields. 4 `_NativePath <= Rc<dyn Path>`.
      // `math.pow(10, v) as int`: a `num` (an `f64` here) to an `int`.
      if (to is InterfaceType &&
          to.classNode.name == 'int' &&
          from is InterfaceType &&
          (from.classNode.name == 'num' || from.classNode.name == 'double')) {
        return IrCast(expression(node.operand), 'i64');
      }
      // `_queue[index] ?? (null as E)`: Dart's way of saying the branch is
      // never taken for a non-nullable `E`. Rust's `E` has no null at all.
      if (node.operand is NullLiteral && to is TypeParameterType) {
        return _unreachable;
      }
      // `key as K` with `key` an `Object?`: a downcast to a type parameter,
      // which `Any` can do because every parameter is bounded `'static`.
      if (to is TypeParameterType &&
          (from is DynamicType ||
              (from is InterfaceType && from.classNode.name == 'Object'))) {
        final operand = from != null && from.nullability == Nullability.nullable
            ? IrNullCheck(expression(node.operand))
            : expression(node.operand);
        return IrCall(
          IrDowncast(operand, to.parameter.name ?? 'T'),
          'clone',
          const [],
        );
      }
      // `Object` and `dynamic` are trait objects here too (`Rc<dyn Object>`).
      // `num` and `double` are abstract in dart:core too, but they are
      // scalars here, not trait objects: `number as double` on a `num` is
      // already an `f64`, and `Any` has nothing to do.
      final fromObject =
          from is DynamicType ||
          (from is InterfaceType &&
              ((_abstractLike(from.classNode) &&
                      _rustScalar(from.classNode.name) ==
                          from.classNode.name) ||
                  from.classNode.name == 'Object'));
      if (fromObject &&
          from != null &&
          to is InterfaceType &&
          // `String` is abstract in dart:core, and `unsafeCast<String?>(Zone
          // .current[#Intl.locale])` wants the same `Any` downcast a struct
          // gets: the prelude's `String` is what an `Rc<dyn Object>` holds.
          (!_abstractLike(to.classNode) ||
              _rustScalar(to.classNode.name) != to.classNode.name ||
              to.classNode.name == 'String') &&
          to.classNode.name != 'Object' &&
          (from is! InterfaceType || from.classNode != to.classNode)) {
        // `dynamic` is an `Rc<dyn Object>`, never an `Option`, whatever
        // its nullability says.
        if ((from is DynamicType || from.nullability != Nullability.nullable) &&
            to.nullability != Nullability.nullable) {
          return IrCall(
            IrDowncast(
              expression(node.operand),
              _rustScalar(to.classNode.name),
            ),
            'clone',
            const [],
          );
        }
        // `Zone.current[#token] as Client?`: a `dynamic` (never an `Option`
        // here) to a nullable struct is a downcast that may fail: `cloned()`
        // of the `Option<&T>` `Any` gives.
        if (from is DynamicType && to.nullability == Nullability.nullable) {
          return IrCall(expression(node.operand), '!as_opt', [
            IrLiteral(_rustScalar(to.classNode.name), const IrType('raw')),
          ]);
        }
        // `_objects![2] as _ImageFilter?`: an `Option<Rc<dyn Object>>` to an
        // `Option<_ImageFilter>`, element by element.
        if (from.nullability == Nullability.nullable &&
            to.nullability == Nullability.nullable) {
          return IrNullAware(
            expression(node.operand),
            IrCall(
              IrDowncast(const IrBound(), to.classNode.name),
              'clone',
              const [],
            ),
          );
        }
      }
      return expression(node.operand);
    }
    if (node is StaticTearOff) {
      return IrFunctionRef(
        node.target.enclosingClass?.name,
        node.target.name.text,
      );
    }
    // An expression the CFE moved from another file -- a mixin field's
    // initialiser into the application's constructor -- is wrapped with
    // its origin; the wrapper is not the expression (`AnimationController`
    // was refused whole for one).
    if (node is FileUriExpression) return expression(node.expression);
    if (node is ConstantExpression) return _constant(node.constant, node);
    // A method used as a value: `Ticker(_tick)` hands `this._tick` over
    // without calling it. In Rust that is a closure that calls it, which makes
    // it the same question as any other closure -- and the same answer: in a
    // borrowed position (an argument, where the parameter is `impl Fn`) it can
    // borrow the receiver, and anywhere else it would have to own it and is
    // refused. 495 of these, and the closure rule already knew what to do with
    // them.
    if (node is InstanceTearOff) {
      // A counted class's tear-off keeps a handle, exactly as a closure that
      // calls a method does -- it *is* that closure, written shorter. Without
      // this the two shapes got different answers for the same question, and
      // the tear-offs stayed refused: 503 of them.
      // `this.controller.dispose` as a value is the same closure as
      // `this.dispose` is, reaching the field through the handle it keeps.
      final holds = _counted && _rootedAtThis(node.receiver);
      // A method of a *local or parameter* used as a value: the closure
      // below captures that variable the way any Rust closure captures a
      // local. `asset.endsWith` handed to `firstWhere` is one.
      final onLocal = node.receiver is VariableGet;
      if (!holds && !_borrowedArgument && !onLocal) {
        throw Unsupported(
          'a method used as a value (${_shape(node.receiver)})',
          _sample(node),
        );
      }
      final target = node.interfaceTarget;
      final fn = target.function;
      // The tear-off's own type is the instantiated one: `sink.add` on a
      // `Sink<List<int>>` takes a `List<int>`, not the `T` the method
      // declares (E0425 `T` in `ByteStream.toBytes`).
      // ..or, when the tear-off's type is out of reach, the receiver's type
      // arguments substituted into the method's declaration (`sink.add` on
      // a local `Sink<List<int>>`).
      // The receiver's instantiation first: `getStaticType` of the tear-off
      // still said `T` for `sink.add` on a `ByteConversionSink`.
      final torn =
          (() {
            final receiverType = _staticType(node.receiver);
            if (receiverType is! InterfaceType) return null;
            // As an instance of the *declaring* class: a `ByteConversionSink`
            // is a `Sink<List<int>>`, and `T` is `Sink`'s.
            final declaringClass = target.enclosingClass;
            final env = typeEnvironment;
            if (declaringClass == null || env == null) return null;
            final asDeclaring = env.hierarchy.getTypeAsInstanceOf(
              receiverType,
              declaringClass,
            );
            if (asDeclaring is! InterfaceType) return null;
            final declared = fn.computeFunctionType(Nullability.nonNullable);
            return Substitution.fromInterfaceType(asDeclaring)
                .substituteType(declared);
          })() ??
          _staticType(node);
      DartType positionalType(int i) =>
          torn is FunctionType && i < torn.positionalParameters.length
          ? torn.positionalParameters[i]
          : fn.positionalParameters[i].type;
      DartType namedType(String name, DartType declared) {
        if (torn is FunctionType) {
          for (final n in torn.namedParameters) {
            if (n.name == name) return n.type;
          }
        }
        return declared;
      }

      final returnType = torn is FunctionType ? torn.returnType : fn.returnType;
      if (fn.typeParameters.isNotEmpty) {
        throw Unsupported('a generic method used as a value', _sample(node));
      }
      // The closure's own parameters: positional as declared, then the named
      // ones **in name order** -- the order a call through the function type
      // uses (`_argumentsByType`). The call inside passes them on in the
      // *method's* declared order, which is the order the method was
      // emitted in. 23 tear-offs of methods with named parameters.
      final params = [
        for (var i = 0; i < fn.positionalParameters.length; i++)
          IrParam(
            _paramName(fn.positionalParameters[i], 'a$i'),
            _type(positionalType(i)),
          ),
        for (final p in _namedInTypeOrder(fn))
          IrParam(
            p.parameterName,
            _type(namedType(p.parameterName, p.type)),
            named: true,
          ),
      ];
      final receiver = node.receiver;
      return IrClosure(
        params,
        IrReturn(
          IrCall(
            receiver is ThisExpression ? null : expression(receiver),
            node.name.text,
            [
              for (var i = 0; i < fn.positionalParameters.length; i++)
                IrLocal(params[i].name),
              for (final p in fn.namedParameters) IrLocal(p.parameterName),
            ],
            // The adapter's call propagates like a written one would.
            fails: _fails(node.interfaceTarget),
          ),
        ),
        _type(returnType),
        // A tear-off of `message.invoke` keeps `message`: cloned in, moved.
        locals: receiver is ThisExpression
            ? const []
            : _freeLocalsIn(receiver, {}),
        holdsSelf: holds,
      );
    }
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
        // A cascade on a local shares it: `v..setValues(..)` and `v` read
        // again after (`use of moved value: v`, vector_math).
        IrLocalDecl(
          _cascadeName,
          _type(bound.type),
          _widened(initial, null, expression(initial)),
        ),
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
  /// A parameter's type: as `_type`, except that `void?` -- `_Callback<T>`
  /// is `void Function(T? result)`, and `_futurize<void>` instantiates it --
  /// is the `Option<()>` the generic `Option<T>` became there. A `void`
  /// *return* type is nullable in Kernel too and stays `()`.
  IrType _paramType(DartType t) =>
      t is VoidType && t.nullability == Nullability.nullable
      ? const IrType('void', nullable: true)
      : _type(t);

  /// A `dynamic` closure parameter takes the expected function type's, when
  /// there is one at that position (see `_expectedFunction`).
  /// Closure parameters whose Rust type is the *expected* one rather than
  /// the declared (see `_closureParamType`): a read of one has that type,
  /// not what Kernel says, and an argument made of it is widened from it.
  final Map<Variable, DartType> _retyped = {};

  DartType _closureParamType(FunctionType? expected, int i, DartType declared) {
    if (expected == null || i >= expected.positionalParameters.length)
      return declared;
    final wanted = expected.positionalParameters[i];
    // The slot's parameter is an erased one: the closure takes the bound
    // (`Rc<dyn Notification>`) and reads it as what it declared
    // (`expression` on a `VariableGet`), 111 closure signature mismatches
    // at ws281.
    if (wanted is TypeParameterType && _erasedParameter(wanted.parameter)) {
      return wanted;
    }
    if (declared is DynamicType) return wanted;
    // TFA narrows the closure's own `int? result` to `int` when no caller
    // passes null; the `Fn(Option<T>)` it is handed to did not change.
    if (declared is InterfaceType &&
        wanted is InterfaceType &&
        declared.classNode == wanted.classNode &&
        declared.nullability != Nullability.nullable &&
        wanted.nullability == Nullability.nullable) {
      return wanted;
    }
    if (declared is FunctionType && wanted is FunctionType) return wanted;
    // `_futurize<void>`: the callback the closure declares as `Object?`
    // is a `void?` -- `Option<()>` -- in the instantiated signature.
    if (wanted is VoidType) return wanted;
    return declared;
  }

  DartType _retype(Variable p, DartType chosen) {
    if (chosen != p.type) _retyped[p] = chosen;
    return chosen;
  }

  IrExpr _closure(FunctionNode fn, Node origin) {
    // Taken once, for this closure: a closure nested in the body is not the
    // one the context described.
    final expected = _expectedFunction;
    _expectedFunction = null;
    // A closure that only reads `final` fields of `this` copies them in
    // instead of holding `this`. A `final` field cannot change, so the copy
    // and the read are the same value -- see `IrClosure.captures`. This is
    // the one case where copying is sound, and it is 345 of the 1319 closures
    // that reach `this`.
    final finals = _finalFieldsRead(fn);
    final copies = finals != null && !_borrowedArgument;
    // A counted class's closure keeps a handle to the object, so `this` is
    // available to it and nothing has to be copied or borrowed.
    if (_reachesThis(fn) &&
        !_counted &&
        !copies &&
        !(_borrowedArgument && _onlyReadsThis(fn))) {
      TreeNode? up = origin is TreeNode ? origin : null;
      while (up != null && up is! Member) {
        up = up.parent;
      }
      final member = up as Member?;
      throw Unsupported(
        'closure capturing `this` in ${member?.enclosingClass?.name}.'
        '${member?.name.text} (${member.runtimeType}'
        '${member is Procedure ? " ${member.kind}" : ""}, '
        'static=${member is Procedure
            ? member.isStatic
            : member is Field
            ? member.isStatic
            : "?"})',
        _sample(origin),
      );
    }
    final body = fn.body;
    if (body == null)
      throw Unsupported('closure with no body', _sample(origin));
    final was = _captured;
    // A counted class's closure keeps the object itself, so nothing is copied
    // out of it: the fields are reached through the handle as usual.
    final holds = _counted && _reachesThis(fn) && !copies;
    if (copies) _captured = {for (final f in finals) f.name.text};
    try {
      return IrClosure(
        [
          for (final (i, p) in fn.positionalParameters.indexed)
            IrParam(
              _paramName(p),
              _paramType(_retype(p, _closureParamType(expected, i, p.type))),
            ),
          // Named parameters, **sorted by name**. A Rust closure has only
          // positions, and a call through a function value sees only the
          // function *type*, whose named parameters Dart keeps in name order
          // -- so that order is the one both ends can agree on. They used to
          // be left off entirely, which made every closure with a named
          // parameter a closure whose body read variables it did not have.
          for (final p in _namedInTypeOrder(fn))
            IrParam(p.parameterName, _type(p.type), named: true),
        ],
        _lowerBody(fn, body),
        _type(fn.returnType),
        isAsync: fn.asyncMarker == AsyncMarker.Async,
        captures: copies
            ? [for (final f in finals) IrParam(f.name.text, _type(f.type))]
            : const [],
        locals: _freeLocals(fn),
        holdsSelf: holds,
      );
    } finally {
      _captured = was;
    }
  }

  /// The locals of the enclosing function a closure reads: they are cloned
  /// in just before it is made, and the closure moves the clones. An
  /// `Rc<dyn Fn>` is `'static`, and a closure borrowing `callback` and
  /// `arg1` from the frame that made it was 9 "does not live long enough".
  List<String> _freeLocals(FunctionNode fn) =>
      _freeLocalsIn(fn, {...fn.positionalParameters, ...fn.namedParameters});

  /// The locals read anywhere under a node and declared nowhere under it.
  List<String> _freeLocalsIn(TreeNode node, Set<Variable> own) {
    final finder = _LocalFinder();
    node.accept(finder);
    final inside = {...finder.declared, ...own};
    final names = <String>[];
    for (final v in finder.read) {
      if (inside.contains(v)) continue;
      // The name the read itself uses: a temporary's given one, else what
      // the human wrote.
      final written = v.cosmeticName;
      final name =
          _temporaries[v] ??
          (written == null || written.startsWith('#') ? _nameFor(v) : written);
      if (!names.contains(name)) names.add(name);
    }
    return names;
  }

  /// Whether a supertype that becomes a trait declares a mutable field.
  static bool _inheritsMutableTraitField(Class node) {
    final seen = <Class>{};
    bool walk(Class c) {
      if (!seen.add(c)) return false;
      final supers = <Class>[
        if (c.superclass != null) c.superclass!,
        for (final t in c.implementedTypes) t.classNode,
        if (c.mixedInClass != null) c.mixedInClass!,
      ];
      for (final s in supers) {
        if ((s.isAbstract || s.isMixinDeclaration) &&
            s.fields.any((f) => !f.isStatic && !f.isFinal)) {
          return true;
        }
        if (walk(s)) return true;
      }
      return false;
    }

    return walk(node);
  }

  /// Whether any method body (not a constructor) writes a field of `this`.
  static bool _writesFieldInMethod(Class node) {
    final finder = _ThisWriteFinder();
    for (final p in node.procedures) {
      if (p.isStatic || p.isAbstract) continue;
      p.function.body?.accept(finder);
      if (finder.found) return true;
    }
    return false;
  }

  /// The fields a closure body reads on `this`, when **every** one is `final`
  /// and nothing else about `this` is touched. Null when it is not that shape.
  List<Field>? _finalFieldsRead(FunctionNode fn) {
    final use = _ThisUse();
    fn.accept(use);
    // A closure that *writes* a shared field is fine -- the cell is what makes
    // it fine -- so writing no longer makes it demanding when every field it
    // touches is either final or shared.
    final finder = _FinalFieldReads(_sharedFields);
    fn.accept(finder);
    if (use.demandingBeyondFields) return null;
    if (!finder.allCarried || finder.fields.isEmpty) return null;
    return finder.fields.values.toList();
  }

  /// The fields the closure being lowered copies in. A read of one is a read
  /// of the local, not of `this`.
  Set<String> _captured = const {};

  /// Whether an expression is `this`, or a chain of field reads from it.
  /// A receiver's shape, for a refusal to name: `this.field!`, `param`.
  static String _shape(Expression e) => switch (e) {
    ThisExpression() => 'this',
    InstanceGet(:final receiver) => '${_shape(receiver)}.field',
    NullCheck(:final operand) => '${_shape(operand)}!',
    Let() => 'let',
    VariableGet(:final variable) =>
      variable.parent is FunctionNode ? 'param' : 'local',
    StaticGet() => 'static',
    _ => '${e.runtimeType}',
  };

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

  /// The class a `super` call really lands in.
  ///
  /// `class X extends A with B` becomes, in Kernel, `X extends _A&B extends A`
  /// -- and `_A&B` is the CFE's, not anything upstream wrote, so this compiler
  /// skips it. A `super.foo()` inside `X` resolves to a member of `_A&B`, so
  /// asking the target which class encloses it gave a class that is not in the
  /// output: 180 refusals reading `super call into `_MixinApplication12&Rende-
  /// rBox&...`, which is not in this file`.
  ///
  /// The class a reader would name is the mixin that declares the member, or
  /// the first real superclass above it if none does.
  Class? _realOwner(Member target, String name) {
    var owner = target.enclosingClass;
    while (owner != null && owner.isAnonymousMixin) {
      // Not `mixedInClass`: with `--target=flutter` the CFE *applies* the
      // mixin, copying its members into this class and clearing `mixedInType`,
      // so that getter is null by the time a dill is read. What survives is
      // `implementedTypes` -- the applied mixins, in the order they were
      // written -- which is how `is Scaled` still answers. Later mixins win, so
      // the search runs backwards.
      for (final applied in owner.implementedTypes.reversed) {
        final mixin = applied.classNode;
        if (mixin.members.any((m) => m.name.text == name && !m.isAbstract)) {
          return mixin;
        }
      }
      owner = owner.superclass;
    }
    return owner;
  }

  /// Whether a closure only *reads* fields of `this`.
  ///
  /// The line that matters in Rust: reading takes a shared borrow, and the
  /// method the closure is written in already holds one. Writing a field would
  /// want `&mut self` while `self` is borrowed for the call the closure is an
  /// argument to, and calling a method on `this` hands out the whole object.
  /// Both stay refused; 296 of the 1319 closures that reach `this` are on this
  /// side of the line, measured by `bin/census_closures.dart`.
  bool _onlyReadsThis(FunctionNode fn) {
    final use = _ThisUse();
    fn.accept(use);
    return !use.demanding;
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
            // Shared, not moved, when the receiver is a local (see the
            // other cascade site).
            // Into the binding's own type: TFA proves `size?.width` non-null
            // and the CFE's `#t` is still a `double?` (`Some(..)`).
            _widened(initial, node.variable.type, expression(initial)),
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
          // `oldLayer?._nativeLayer` with `_nativeLayer` a `T?`: one
          // `Option`, not two (8 `Option<Option<..>>` in dart:ui).
          final memberType = _staticType(otherwise);
          return IrNullAware(
            receiver,
            expression(otherwise),
            // `void` is "nullable" to Kernel; `x?.addListener(..)` is a
            // `map`, not an `and_then` (`Option<_> <= ()`).
            flatten:
                memberType is InterfaceType &&
                memberType.nullability == Nullability.nullable,
          );
        } finally {
          _bound = previous;
        }
      }
      // `x!` -- the CFE writes it `let #0 = x in #0 == null ? #0 as T : #0`,
      // which is `??`'s shape with the temporary on *both* sides. Read as `??`
      // it took `#0 as T` for the right side and then met its own temporary
      // there with no name: 111 refusals reading `synthetic variable`, every
      // one an `x!` on a field.
      final then = body.then;
      if (condition is EqualsNull &&
          _isThe(condition.expression, node.variable) &&
          _isThe(otherwise, node.variable) &&
          then is AsExpression &&
          _isThe(then.operand, node.variable)) {
        final value = node.variable.initializer;
        if (value == null) {
          throw Unsupported('`!` with no operand', _sample(node));
        }
        return IrNullCheck(expression(value));
      }
      if (condition is EqualsNull &&
          _isThe(condition.expression, node.variable) &&
          _isThe(otherwise, node.variable)) {
        final value = node.variable.initializer;
        if (value == null) {
          throw Unsupported('`??` with no left side', _sample(node));
        }
        final right = body.then;
        // `locale ?? "unspecified"` inside a string: the two sides are of
        // different classes and the result is `Object`, so both go through
        // `dart_str` (6 `Option<Locale> <= String` shapes in dart:ui).
        final leftType = _staticType(value);
        final rightType = _staticType(right);
        if (leftType is InterfaceType &&
            rightType is InterfaceType &&
            leftType.classNode != rightType.classNode &&
            body.staticType is InterfaceType &&
            (body.staticType as InterfaceType).classNode.name == 'Object') {
          return IrIfNull(
            IrNullAware(
              expression(value),
              IrStaticCall(null, 'dart_str', const [IrBound()]),
            ),
            IrStaticCall(null, 'dart_str', [expression(right)]),
            nullableResult: false,
            eager: false,
          );
        }
        // The right side into the left's type: `curve ?? Curves.ease` shares
        // its `Cubic` into the `Rc<dyn Curve>` -- with the target spelled
        // (`IrUpcast`), since a `match` arm does not coerce (267 "arms have
        // incompatible types" the round it was a bare `Rc::new`).
        // ..into the type the *result* has: `a ?? b` with a nullable `b`
        // stays nullable, and the arm is `Some(..)`, not `.unwrap()` (292
        // "arms have incompatible types" the round it was always non-null).
        final resultNullable =
            body.staticType.nullability == Nullability.nullable;
        final into = leftType is InterfaceType
            ? (resultNullable
                  ? leftType.withDeclaredNullability(Nullability.nullable)
                  : leftType.withDeclaredNullability(Nullability.nonNullable))
            : null;
        var rightSide = expression(right);
        if (into != null) {
          final widened = _widened(right, into, rightSide);
          rightSide = widened is IrCall && widened.name == '!rc'
              ? IrUpcast(widened.target!, _type(into))
              : widened;
        }
        return IrIfNull(
          expression(value),
          rightSide,
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
    // `alpha ?? a` after type flow analysis proved `alpha` non-null: the
    // conditional is gone and the body is the bound variable, *promoted*
    // to `double` while the binding is still `double?`. The unwrap is the
    // proof (the `{ let __t: Option<f64> = alpha; __t }` shapes).
    final letBody = node.body;
    final promotedRead =
        letBody is VariableGet &&
        letBody.variable == node.variable &&
        node.variable.type.nullability == Nullability.nullable &&
        letBody.promotedType != null &&
        letBody.promotedType!.nullability != Nullability.nullable;
    return IrBlockValue([
      IrLocalDecl(
        name,
        // The post-increment's middle binding is `void` (see `_declare`).
        node.variable.type is VoidType ? null : _type(node.variable.type),
        // A local bound here is shared, not moved: `let __t = key;` and
        // `key` read again two lines on (13 E0382s). Into a `dynamic`
        // binding it is shared into the `Rc<dyn Object>` (`__t: Rc<dyn
        // Object> = true`).
        _intoObject(
          initial,
          node.variable.type,
          // ..and widened into the binding's type: `double? t = size?.height`
          // after TFA holds a `double`, and the binding says `Some`.
          _widened(initial, node.variable.type, expression(initial)),
        ),
      ),
    ], promotedRead ? IrNullCheck(IrLocal(name)) : expression(letBody));
  }

  /// One local declaration, wherever it is written.
  ///
  /// A `for`'s variables are `VariableDeclaration`s and not `Statement`s in
  /// this Kernel, so they cannot go through `statement` -- and the rule about
  /// what a declaration becomes should be in one place regardless.
  /// Whether a member or parameter is the widget inspector's, not upstream's.
  ///
  /// A debug build runs the widget-creation-tracking transform, which gives
  /// `Widget` a `_location` field of type `CreationLocation` and its
  /// constructor a `$creationLocationd_<hash>` parameter. `Widget` is the base
  /// of nearly everything, so flattening copies that field into every widget
  /// and every widget constructor passes the argument -- 627 refusals for a
  /// const instance of a class that is not in the program at all.
  ///
  /// Dropped rather than translated, and said here rather than silently:
  /// this is the compiler's own instrumentation, not something anybody wrote.
  static bool _inspectorOnly(String name, [DartType? type]) {
    if (name.startsWith(r'$creationLocation')) return true;
    if (name != '_location') return false;
    return type is InterfaceType &&
        (type.classNode.name == 'CreationLocation' ||
            type.classNode.name == '_Location');
  }

  /// `a.b = v` as a statement.
  ///
  /// Its own method because a `return a.b = v;` in a void function is this
  /// statement and then a bare return -- the CFE writes `=> x = v` that way,
  /// 171 times in the gallery's dill, every one in a setter or a void closure.
  /// The receiver's static class, when it is a translated one.
  Class? _staticClass(Expression receiver) {
    final t = _staticType(receiver);
    return t is InterfaceType && _translatedClass(t.classNode)
        ? t.classNode
        : null;
  }

  String? _receiverClassName(Expression receiver) =>
      _staticClass(receiver)?.name;

  /// A Dart member's name on the Rust side: `clone` would shadow
  /// `Clone::clone`, which the backend calls on every value it shares
  /// (`Matrix4.clone()` gave every `.clone()` a `Result`, 179).
  static String _dartName(String name) => name == 'clone' ? 'clone_' : name;

  IrStmt _instanceSet(InstanceSet value) {
    // The value widens into the field's or setter's type: `_cache = s`
    // into a `String?` field is `Some(s)`.
    final written = _widened(
      value.value,
      value.interfaceTarget.setterType,
      expression(value.value),
    );
    // A field on `this`, and a field rather than a setter. Kernel names the
    // target outright, so neither has to be inferred.
    // A write to the cascade's own binding: a local, so it needs a
    // mutable local rather than a mutable `self`.
    final receiver = value.receiver;
    if (_cascade != null &&
        receiver is VariableGet &&
        receiver.variable == _cascade) {
      if (value.interfaceTarget is! Field) {
        return IrSetter(const IrLocal(_cascadeName), value.name.text, written);
      }
      // The owner is the cascaded value's own class: on a counted one its
      // fields are cells, and without the owner the backend wrote
      // `cascaded.on_down = ..` into an `Rc<RefCell<..>>` (23+23 in
      // `widgets`).
      return IrAssignField(
        value.name.text,
        written,
        target: const IrLocal(_cascadeName),
        owner:
            _receiverClassName(receiver) ??
            value.interfaceTarget.enclosingClass?.name,
      );
    }
    if (value.receiver is! ThisExpression) {
      // Another object's *setter* is a call, which needs nothing from us
      // beyond a `&mut` receiver at the call site.
      if (value.interfaceTarget is! Field) {
        return IrSetter(
          expression(value.receiver),
          value.name.text,
          written,
          qualifier: _setterQualifier(value.receiver, value.interfaceTarget),
          receiverClass: _classNameOf(value.receiver),
        );
      }
      // A *field* is a write through a reference. Through a chain rooted
      // at `this` -- `this.child.x = v` -- that reference is `self`, and
      // `&mut self` is a thing this compiler already works out. Through a
      // parameter it would mean `&mut` on the parameter and on every call
      // site, including ones in other files, so that one still stops.
      // A *local* that owns a value: `final entry = _ChildEntry(..);
      // entry.x = v;` is `let mut entry` and a plain field write in Rust,
      // with no reference in between and nothing for a call site to know.
      // Measured on 2026-09-03: 107 of the 296 refusals here were exactly
      // this. A local holding a counted class's handle is not this -- its
      // fields would have to be cells -- and a parameter is not either.
      final receiver = value.receiver;
      if (receiver is VariableGet &&
          receiver.variable.parent is! FunctionNode &&
          !_closureCallsMethod(value.interfaceTarget.enclosingClass!)) {
        return IrAssignField(
          value.name.text,
          written,
          target: expression(receiver),
        );
      }
      // A local or a parameter holding a *counted* class's handle: every
      // non-final field of such a class is already a cell (the backend's
      // `_inCell`), so the write goes through the cell and needs no `&mut`
      // on anything. The owner rides on the node so the backend can find the
      // cell. 82 + 14 of the refusals here.
      final declaring = value.interfaceTarget.enclosingClass!;
      // ..and reached however it was reached: `_views[viewId]!.x = v` is a
      // handle out of a map, and the write goes through the cell just the
      // same (`PlatformDispatcher`, 1 refusal that took 3 callers).
      final receiverClass = _staticClass(receiver);
      if (_closureCallsMethod(declaring) ||
          (receiverClass != null && _closureCallsMethod(receiverClass))) {
        // The receiver's own class, where the cells are decided; the
        // declaring one may be an abstract base.
        return IrAssignField(
          value.name.text,
          written,
          target: expression(receiver),
          owner: receiverClass?.name ?? declaring.name,
        );
      }
      if (!_rootedAtThis(value.receiver)) {
        throw Unsupported(
          'assignment to a field of another object '
          '(${_shape(value.receiver)}, '
          '${_closureCallsMethod(value.interfaceTarget.enclosingClass!) ? "counted" : "value"})',
          _sample(value),
        );
      }
      return IrAssignField(
        value.name.text,
        written,
        target: expression(value.receiver),
      );
    }
    if (value.interfaceTarget is! Field) {
      return IrSetter(
        null,
        value.name.text,
        written,
        qualifier: _setterQualifier(null, value.interfaceTarget),
      );
    }
    return IrAssignField(value.name.text, written);
  }

  IrStmt _declare(Variable variable, Node at) {
    final init = variable.initializer;
    if (init is InstanceGet && init.name.text == 'iterator') {
      // Remembered, not lowered: if the loop below it is the CFE's `for-in`,
      // this binding is part of that shape and the restored loop names the
      // iterable itself.
      _iterators[variable] = init.receiver;
      // Declared as well as remembered: a loop the restoration recognises
      // ignores this binding, and a hand-driven one -- `final it =
      // xs.iterator; while (it.moveNext()) ..`, `equality.dart` -- needs it.
      // Swallowed, it left `iterator.move_next()` on nothing.
      final written = variable.cosmeticName;
      final name =
          (written == null ||
              written.startsWith('#') ||
              written.startsWith(':'))
          ? _nameFor(variable)
          : written;
      return IrLocalDecl(
        name,
        null,
        // A clone: the iterator owns its items, and the list is a field
        // behind `&self` more often than not (`self._children`, E0507).
        IrStaticCall(null, 'dart_iter', [
          IrCall(expression(init.receiver), 'clone', const []),
        ]),
      );
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
      // `void` is what the CFE gives the temporary of a post-increment whose
      // value is unused, and `let __t: () = { ..; __set }` then held an
      // `i64` (53 `() <= i64`). Unannotated, Rust infers what it holds.
      variable.type is VoidType ? null : _type(variable.type),
      // Into the declared type: `Int32List? x = encode(..)` is `Some(..)`;
      // `num divisor = pow(10, n).round()` casts the `int`.
      // ..and a `dynamic` local holding a scalar or struct shares it
      // (`var integer = number.floor()` on a `dynamic` number).
      init == null
          ? null
          : _intoObject(
              init,
              variable.type,
              _intoDeclaredNum(
                init,
                variable.type,
                _widened(init, variable.type, expression(init)),
              ),
            ),
      cell: _capturedWrites.contains(variable),
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

  /// The Rust element type of a typed list narrower than Dart's `double`
  /// and `int`, or null for anything else.
  static String? _narrowElement(DartType? type) {
    if (type is! InterfaceType) return null;
    return const {
      'Float32List': 'f32',
      'Int8List': 'i8',
      'Int16List': 'i16',
      'Int32List': 'i32',
      'Uint8List': 'u8',
      'Uint8ClampedList': 'u8',
      'Uint16List': 'u16',
      'Uint32List': 'u32',
    }[type.classNode.name];
  }

  /// A parameter's declared default, lowered -- or null when it has none or
  /// the default is not a shape this front end lowers.
  IrExpr? _default(FunctionParameter p) {
    final value = p.defaultValue;
    if (value == null) return null;
    try {
      return expression(value);
    } on Unsupported {
      return null;
    }
  }

  /// A parameter's name for the backend.
  ///
  /// The CFE gives its own parameters names no human wrote --
  /// `#externalFieldValue` on an external field's setter, `#typedDataBase` on
  /// a `Struct` constructor -- and `#` is not a character the backend can
  /// carry. Those get the same `__tN` a temporary gets, by identity, and
  /// `VariableGet` finds it again the same way. 128 refusals were these.
  String _paramName(Variable p, [String? fallback]) {
    final written = p.cosmeticName;
    // A parameter with no written name still has to be *nameable*: a super
    // forwarder passes it on by name, and `_` is a pattern in Rust, not a
    // value -- `super_set_first(self, _)` did not parse.
    if (written == null) return fallback ?? _nameFor(p);
    return written.startsWith('#') || written == '_' ? _nameFor(p) : written;
  }

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
    return _restoreForIn(node.condition, node.body);
  }

  /// The same loop written with `while`: the CFE's other spelling of
  /// `for (x in xs)` -- `while (:sync-for-iterator.moveNext())` -- which the
  /// `for (;;)` restoration never saw. The iterator binding above it had
  /// already been swallowed as "part of that shape", so the loop that came
  /// out named a variable nothing declared: 6 `_sync_for_iterator`s.
  IrStmt? _forInWhile(WhileStatement node) =>
      _restoreForIn(node.condition, node.body);

  IrStmt? _restoreForIn(Expression? condition, Statement body0) {
    if (condition is! InstanceInvocation || condition.name.text != 'moveNext') {
      return null;
    }
    final receiver = condition.receiver;
    if (receiver is! VariableGet) return null;
    final iterable = _iterators[receiver.variable];
    if (iterable == null) return null;

    var body = body0;
    if (body is LabeledStatement) body = body.body;
    if (body is! Block || body.statements.isEmpty) return null;
    final first = body.statements.first;
    if (first is! VariableStatement) {
      // No `x = it.current` at the top: the body reads `.current` where it
      // needs it. The element gets a name here and `_instanceGet` hands the
      // reads that name (see `_currentOf`). Without this the declaration
      // was swallowed above and the loop below named a variable that was
      // never declared -- `_sync_for_iterator`, 6 times.
      final element = '__t${_nextTemporary++}';
      _currentOf[receiver.variable] = element;
      _iteratorLoops.add(receiver.variable);
      return IrForIn(
        element,
        expression(iterable),
        IrBlock([for (final s in body.statements) statement(s)]),
      );
    }
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
    // The element's name is the binding's, or one of this front end's own
    // when the CFE gave it none -- a `for ((a, b) in pairs)` binds `#0`.
    final written = first.declaration.variable.cosmeticName;
    final name = (written == null || written.startsWith('#'))
        ? _nameFor(first.declaration.variable)
        : written;
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

  /// The element name standing in for `it.current` inside a restored loop
  /// whose body did not bind it first.
  final _currentOf = <Variable, String>{};

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
  /// `Map` and the `dart:collection` classes the prelude's `Map` stands
  /// for: `SplayTreeMap<double, String>` in `AssetImage` took the generic
  /// path and asked for `index_of` (50).
  static bool _isMapClass(String? owner) =>
      const {'Map', 'LinkedHashMap', 'HashMap', 'SplayTreeMap'}.contains(owner);

  IrExpr _instanceGet(InstanceGet node) =>
      _narrowedRead(node, _instanceGetRaw(node));

  IrExpr _instanceGetRaw(InstanceGet node) {
    final name = node.name.text;
    final listOwner = node.interfaceTarget.enclosingClass?.name;
    if (listOwner == 'List' || listOwner == 'Iterable') {
      final rust = listMethodNames[name];
      if (rust == null) throw Unsupported('`List.$name`', _sample(node));
      // A getter in Dart, a method in Rust: `xs.length` is `xs.len()`.
      return IrCall(expression(node.receiver), rust, const []);
    }
    if (_isMapClass(listOwner)) {
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
    // A field the enclosing closure copied in is a local now, not a field of
    // a `this` the closure does not hold. See `IrClosure.captures`.
    if (receiver is ThisExpression && _captured.contains(name)) {
      return IrLocal(name);
    }
    if (name == 'current' && receiver is VariableGet) {
      final element = _currentOf[receiver.variable];
      if (element != null) return IrLocal(element);
    }
    final target = receiver is ThisExpression ? null : expression(receiver);
    if (node.interfaceTarget is Procedure) {
      return _qualified(
        IrCall(target, name, const []),
        node.interfaceTarget,
        receiver,
      );
    }
    // A field of a `dart:` class the prelude re-expresses -- `Duration
    // .inMicroseconds` is a field in this SDK -- is a method there, as
    // every getter of such a class is.
    // Only where the prelude spells them as methods: `MapEntry.key` and
    // `SocketException.message` are fields there (11 E0599s when every
    // `dart:` class took this path, ws136).
    final owner = node.interfaceTarget.enclosingClass;
    if (owner != null &&
        owner.enclosingLibrary.importUri.scheme == 'dart' &&
        const {'Duration', 'DateTime'}.contains(owner.name)) {
      return IrCall(target, name, const []);
    }
    // A field declared on an *abstract* class is a trait accessor in Rust,
    // and a read through another object -- whatever its concrete class
    // stores -- goes through the accessor: `rc.x` took the value of a
    // method, 111 times in the leaf crates.
    final declaring = node.interfaceTarget.enclosingClass;
    // Not an anonymous mixin application: it is abstract to Kernel, and its
    // fields are flattened into the applying class's struct here
    // (`Get.isLogEnable` read as a getter call on a field, E0599).
    // ..unless the receiver's own static class is concrete: the struct has
    // the abstract base's field flattened in, and the field is read as one
    // (`Get.isLogEnable` on a `_GetImpl`, whose trait was not even in scope).
    final receiverType = target == null ? null : _staticType(receiver);
    final concrete =
        receiverType is InterfaceType &&
        !_abstractLike(receiverType.classNode) &&
        receiverType.classNode.enclosingLibrary.importUri.scheme != 'dart';
    if (target != null &&
        declaring != null &&
        _abstractLike(declaring) &&
        !declaring.isAnonymousMixin &&
        !concrete) {
      return _qualified(
        IrCall(target, name, const []),
        node.interfaceTarget,
        receiver,
      );
    }
    return IrField(
      target,
      name,
      // `PerformanceOverlayOption.x.index` in a static initialiser resolves
      // to `_Enum.index`, a field of a class that is not an enum; the
      // receiver's own type says it is one (4 "attempted to take value of
      // method `index`" in `rendering`).
      onEnum:
          (node.interfaceTarget.enclosingClass?.isEnum ?? false) ||
          (receiverType is InterfaceType && receiverType.classNode.isEnum),
      owner: target == null
          ? null
          : concrete && declaring != null && _abstractLike(declaring)
          ? (receiverType as InterfaceType).classNode.name
          : node.interfaceTarget.enclosingClass?.name,
    );
  }

  /// `dateTimeSymbols[k]`, `.containsKey(k)`, `.keys` on a `dynamic` slot
  /// with known types (see `dynamicSlots`): one arm per type, each giving
  /// the *same* Rust type -- what the Dart code does with the result is
  /// typed by the first arm's member. A `Map` arm answers as a map; a class
  /// arm calls its own member, or panics for one it does not have (Dart's
  /// `NoSuchMethodError`, which is what `UninitializedLocaleData` does).
  IrExpr? _dynamicSlotCall(Expression node) {
    final Expression receiver;
    final String name;
    final List<Expression> positional;
    if (node is DynamicInvocation) {
      receiver = node.receiver;
      name = node.name.text;
      positional = node.arguments.positional;
    } else if (node is DynamicGet) {
      receiver = node.receiver;
      name = node.name.text;
      positional = const [];
    } else {
      return null;
    }
    if (receiver is! StaticGet) return null;
    var target = receiver.target;
    // Through a getter that only reads the slot: `dynamic get
    // dateTimeSymbols => _dateTimeSymbols`.
    if (target is Procedure && target.isGetter) {
      final body = target.function.body;
      final read = body is ReturnStatement ? body.expression : null;
      if (read is StaticGet) target = read.target;
    }
    if (target is! Field) return null;
    final candidates = dynamicSlots[target];
    if (candidates == null || candidates.isEmpty) return null;
    const known = {'[]', 'containsKey', 'keys'};
    if (!known.contains(name)) return null;
    final args = [for (final e in positional) expression(e)];
    const slot = IrLocal('__d');
    IrExpr noSuch() => IrLiteral(
      'panic!("uncaught Dart exception: NoSuchMethodError: `$name` on an ${candidates.first.classNode.name}")',
      const IrType('raw'),
    );
    final arms = <(IrType?, IrExpr)>[];
    for (final c in candidates) {
      final isMap =
          c.classNode.name == 'Map' || c.classNode.name == 'LinkedHashMap';
      final hasMember = c.classNode.members.any(
        (m) => m.name.text == name && !m.isAbstract,
      );
      final IrExpr body;
      switch (name) {
        case '[]':
          body = isMap
              ? IrCall(slot, '!map_get', args)
              : hasMember
              ? IrSome(IrCall(slot, '[]', args))
              : noSuch();
        case 'containsKey':
          // The map's is the prelude's `contains_key(&k)`, spelled as the
          // `Map` lowering spells it so the backend passes the key by
          // reference; a class's is its own method, by value.
          body = isMap
              ? IrCall(slot, 'contains_key', args)
              : hasMember
              ? IrCall(slot, 'containsKey', args)
              : noSuch();
        default:
          body = isMap || hasMember ? IrCall(slot, name, args) : noSuch();
      }
      arms.add((_type(c), body));
    }
    return IrDynamicDispatch(expression(receiver), arms);
  }

  IrExpr _staticGet(StaticGet node) {
    final target = node.target;
    final enclosing = target.enclosingClass;
    if (enclosing == null) {
      // A top-level name. A `const` or `final` is a module constant in Rust
      // too; a computed `get foo => ...` is a function and stops here.
      // Mutable ones too, now that they are emitted. A read of one goes
      // through the cell, which the backend knows from the declaration.
      if (target is Field) return IrTopLevel(target.name.text);
      // A top-level getter is a function here, so reading it is calling it.
      if (target is Procedure && target.kind == ProcedureKind.Getter) {
        return IrStaticCall(
          null,
          target.name.text,
          const [],
          fails: _fails(target),
          diverges: _diverges(target),
        );
      }
      throw Unsupported('top-level `${target.name.text}`', _sample(node));
    }
    // A static *getter* is a function -- `PlatformDispatcher.instance` --
    // and reading it is calling it, as for a top-level getter above. As a
    // static it was spelled `PlatformDispatcher::INSTANCE`, a constant
    // nothing declared (20 times).
    if (target is Procedure && target.kind == ProcedureKind.Getter) {
      return IrStaticCall(
        enclosing.name,
        target.name.text,
        const [],
        fails: _fails(target),
        diverges: _diverges(target),
      );
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
    final args = _arguments(
      node.arguments,
      node.interfaceTarget.function,
      true,
      node.functionType,
    );
    final generic = _genericOnTrait(node, args);
    if (generic != null) return generic;
    final owner = node.interfaceTarget.enclosingClass?.name;
    // A `StreamView` subclass's inherited `listen` and friends act on the
    // `_stream` it carries (see `lowerClass`).
    final declaringStream = node.interfaceTarget.enclosingClass;
    if (node.receiver is ThisExpression &&
        declaringStream != null &&
        (declaringStream.name == 'Stream' ||
            declaringStream.name == 'StreamView') &&
        declaringStream.enclosingLibrary.importUri.toString() == 'dart:async') {
      return IrCall(const IrField(null, '_stream'), name, args);
    }
    // `child.toString()` on a `Listenable?`: an `Option` has no
    // `to_string`, and `dart_str` prints `null` for the absent case as
    // Dart does.
    if (name == 'toString' && args.isEmpty) {
      final t = _staticType(node.receiver);
      if (t != null &&
          t is! DynamicType &&
          t.nullability == Nullability.nullable) {
        return IrStaticCall(null, 'dart_str', [expression(node.receiver)]);
      }
    }
    if (owner == 'List' || _isMapClass(owner) || owner == 'Iterable') {
      // A collection member is a *Rust* method taking `impl Fn`, so a closure
      // given to one is not boxed. `_keeps` cannot say so: the callee is
      // `dart:core`'s, with no body to read, and it answers "kept" for want of
      // evidence. The analyzer front end has no such analysis and said
      // unboxed, so the two wrote different Rust for `m.forEach(..)`.
      for (var i = 0; i < args.length; i++) {
        args[i] = _unboxed(args[i]);
      }
    }
    // `completer.complete()` on a `Completer<void>`: the value is `()`.
    if (owner == 'Completer' &&
        name == 'complete' &&
        (args.isEmpty ||
            (args.length == 1 && node.arguments.positional.isEmpty))) {
      return IrCall(expression(node.receiver), 'complete', [
        IrSome(const IrLiteral('()', IrType('raw'))),
      ]);
    }
    // `s[i]` on a String is a one-character String, not an index into a
    // list: `pattern[0] == "a"` in intl's date formatting (44 + 44).
    // `[3, 4, 5].contains(n % 100)` with `n` a `num`: Dart compares by
    // value (`3 == 3.0`), so the `double` is cast to the list's `int`.
    // The prelude's `Set::remove` takes the value by reference, like the
    // map's key (`_tickers.remove(ticker)`, 46).
    if (owner == 'Set' && name == 'remove' && args.length == 1) {
      return IrCall(expression(node.receiver), '!map_remove', args);
    }
    if ((owner == 'List' || owner == 'Iterable' || owner == 'Set') &&
        name == 'contains' &&
        args.length == 1) {
      final listType = _staticType(node.receiver);
      final argType = _staticType(node.arguments.positional.single);
      final element =
          listType is InterfaceType && listType.typeArguments.isNotEmpty
          ? listType.typeArguments.first
          : null;
      if (element is InterfaceType &&
          element.classNode.name == 'int' &&
          argType is InterfaceType &&
          (argType.classNode.name == 'double' ||
              argType.classNode.name == 'num')) {
        return IrCall(expression(node.receiver), '!contains', [
          IrCast(args.single, 'i64'),
        ]);
      }
      return IrCall(expression(node.receiver), '!contains', args);
    }
    if (owner == 'String' && name == '[]' && args.length == 1) {
      return IrCall(expression(node.receiver), 'char_at', args);
    }
    // `trim()` and friends: `str::trim` hands back a `&str`, and being
    // inherent it wins over a trait method of the same name.
    if (owner == 'String' &&
        const {'trim', 'trimLeft', 'trimRight'}.contains(name) &&
        args.isEmpty) {
      const spelled = {
        'trim': 'trim_dart',
        'trimLeft': 'trim_left_dart',
        'trimRight': 'trim_right_dart',
      };
      return IrCall(expression(node.receiver), spelled[name]!, const []);
    }
    if (owner == 'String' && name == 'split' && args.length == 1) {
      // `s.split(p)`: Rust's `split` wants a `&str` and yields an iterator.
      return IrCall(expression(node.receiver), 'split_dart', args);
    }
    if (owner == 'String' && name == '*' && args.length == 1) {
      // `'0' * n`: Rust's `repeat` wants a `usize`.
      return IrCall(expression(node.receiver), 'repeat_dart', args);
    }
    if (owner == 'String' &&
        name == 'contains' &&
        (args.length == 1 || args.length == 2)) {
      // `contains(other, [start])`: `str::contains` is inherent, takes a
      // `&str`, and has no start; the prelude's `contains_dart` has both.
      return IrCall(expression(node.receiver), 'contains_dart', [
        args.first,
        if (args.length == 2) args[1] else const IrLiteral('0', IrType('int')),
      ]);
    }
    if (owner == 'String' && name == 'startsWith' && args.length == 2) {
      // `startsWith(pattern, index)`: `str::starts_with` takes one argument
      // and, being inherent, would win over a trait method of the same name.
      return IrCall(expression(node.receiver), 'starts_with_at', args);
    }
    if (owner == 'String' && name == 'replaceRange' && args.length == 3) {
      // Dart's `replaceRange` returns a new string; Rust's `String` has an
      // inherent `replace_range` that mutates in place and takes a range,
      // and an inherent method shadows a trait's. So the prelude's is named
      // apart.
      return IrCall(expression(node.receiver), 'replace_range_dart', args);
    }
    if (owner == 'Expando') {
      // `expando[object]` / `expando[object] = v`: identity-keyed, so the
      // prelude's `get`/`set` rather than an index. 6 uses.
      if (name == '[]' && args.length == 1) {
        return IrCall(expression(node.receiver), '!expando_get', [args.single]);
      }
      if (name == '[]=' && args.length == 2) {
        return IrCall(expression(node.receiver), 'set', args);
      }
    }
    // A typed list with a narrow element -- `Float32List` is `Vec<f32>`,
    // `Int32List` is `Vec<i32>` -- takes Dart's `double`/`int` cast down on
    // the way in and up on the way out. 23 `f32 <= f64` and 14 `i32`/`i64`
    // in `dart:ui`'s colour and vertex code.
    final narrow = _narrowElement(_staticType(node.receiver));
    if (narrow != null && name == '[]' && args.length == 1) {
      return IrCast(
        IrIndex(expression(node.receiver), args.single),
        narrow.startsWith('f') ? 'f64' : 'i64',
      );
    }
    if (narrow != null && name == '[]=' && args.length == 2) {
      final held = '__t${_nextTemporary++}';
      return IrBlockValue([
        IrLocalDecl(held, null, args[1]),
        IrIndexSet(
          expression(node.receiver),
          args[0],
          IrCast(IrCall(IrLocal(held), 'clone', const []), narrow),
        ),
      ], IrLocal(held));
    }
    if (owner == 'List' || owner == 'Iterable') {
      if (name == '[]' && args.length == 1) {
        return IrIndex(expression(node.receiver), args.single);
      }
      if (name == '[]=' && args.length == 2) {
        // `xs[i] = v` where the expression's value is wanted -- the CFE puts
        // `xs[i] += 1` into a `Let` whose body is this call. Bound, stored as
        // a clone, produced: the same shape every other assignment-as-value
        // takes here. 48 of them.
        final held = '__t${_nextTemporary++}';
        return IrBlockValue([
          IrLocalDecl(held, null, args[1]),
          IrIndexSet(
            expression(node.receiver),
            args[0],
            IrCall(IrLocal(held), 'clone', const []),
          ),
        ], IrLocal(held));
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
      if (name == 'firstWhere' && args.length == 2) {
        // `firstWhere(test)` throws when nothing matches; with `orElse` it
        // calls that instead. The omitted `orElse` arrives as `None`, and a
        // generic `impl Fn` parameter cannot take a `None`, so the two are
        // two prelude methods. 25 calls.
        final orElse = args[1];
        final omitted = orElse is IrLiteral && orElse.type.name == 'Null';
        return IrCall(
          expression(node.receiver),
          omitted ? 'first_where' : 'first_where_or',
          omitted ? [args[0]] : args,
        );
      }
      if (name == 'sort') {
        // `sort()` is `Vec::sort`; `sort(compare)` takes a Dart comparator
        // returning an `int`, which the prelude's `sort_by_dart` turns into
        // an `Ordering`. 36 of these.
        return IrCall(
          expression(node.receiver),
          args.isEmpty ? 'sort' : 'sort_by_dart',
          args,
        );
      }
      final rust = listMethodNames[name];
      if (rust != null) {
        return IrCall(expression(node.receiver), rust, args);
      }
      throw Unsupported('`List.$name`', _sample(node));
    }
    if (_isMapClass(owner)) {
      // Its own name: the backend's `.get(&k).cloned()` was keyed on `get`
      // and fired on `ContrastCurve.get(double)` too (14 `&f64`).
      if (name == '[]' && args.length == 1) {
        // `_cache[tone]` on a `Map<int, _>` with a `num` key: the key is an
        // `f64` here and the map's is `i64`, the same cast `contains` makes.
        final mapType = _staticType(node.receiver);
        final argType = _staticType(node.arguments.positional.single);
        final key = mapType is InterfaceType && mapType.typeArguments.isNotEmpty
            ? mapType.typeArguments.first
            : null;
        if (key is InterfaceType &&
            key.classNode.name == 'int' &&
            argType is InterfaceType &&
            (argType.classNode.name == 'double' ||
                argType.classNode.name == 'num')) {
          return IrCall(expression(node.receiver), '!map_get', [
            IrCast(args.single, 'i64'),
          ]);
        }
        // A nullable key into a map of non-nullable ones: `_views[_implicitViewId]`.
        if (key != null &&
            key.nullability != Nullability.nullable &&
            argType != null &&
            argType is! DynamicType &&
            argType.nullability == Nullability.nullable) {
          return IrCall(expression(node.receiver), '!map_get_opt', args);
        }
        return IrCall(expression(node.receiver), '!map_get', args);
      }
      // `m[k] = v`: `insert`, as a statement or for its value (Dart's is
      // `v`; here the old value, which no caller reads).
      if (name == '[]=' && args.length == 2) {
        return IrCall(expression(node.receiver), 'insert', args);
      }
      if (orderedMapMembers.contains(name)) {
        throw Unsupported(
          '`Map.$name`, which depends on insertion order',
          _sample(node),
        );
      }
      final rust = mapMethodNames[name];
      if (rust == null) throw Unsupported('`Map.$name`', _sample(node));
      // `Map<int, _>.containsKey(tone)` with a `double`: Dart's `3.0 == 3`
      // finds the key, so the `double` is cast to the map's `int`.
      if (const {'containsKey', 'remove', '[]'}.contains(name) &&
          args.length == 1) {
        final mapType = _staticType(node.receiver);
        final key = mapType is InterfaceType && mapType.typeArguments.isNotEmpty
            ? mapType.typeArguments.first
            : null;
        final argType = _staticType(node.arguments.positional.single);
        if (key is InterfaceType &&
            key.classNode.name == 'int' &&
            argType is InterfaceType &&
            argType.classNode.name == 'double') {
          return IrCall(expression(node.receiver), rust, [
            IrCast(args.single, 'i64'),
          ]);
        }
      }
      return IrCall(expression(node.receiver), rust, args);
    }
    if (_binaryOperators.contains(name) && args.length == 1) {
      // `int * double` is a `double` in Dart and a type error in Rust: the
      // `int` side is cast. The receiver's class is the operator's owner;
      // the argument's is asked of the static types.
      var left = expression(node.receiver);
      var right = args.single;
      // Comparisons too: `returnValue < 0` on a `double` is `f64 < integer`
      // in Rust until the literal is cast (6 in the colour code).
      // `targetWidth! ~/ (w / h)`: an `int ~/ double` is a `double`
      // division truncated to an `int` in Dart. Both sides go to `f64`
      // and the truncated result comes back to `i64`.
      if (name == '~/') {
        String? classOf(Expression e) {
          final t = _staticType(e);
          return t is InterfaceType ? t.classNode.name : null;
        }

        final leftClass = classOf(node.receiver);
        final rightClass = classOf(node.arguments.positional.single);
        if (leftClass == 'double' || rightClass == 'double') {
          if (leftClass == 'int') left = IrCast(left, 'f64');
          if (rightClass == 'int') right = IrCast(right, 'f64');
          return IrCast(
            IrBinary(name, left, right, type: const IrType('double')),
            'i64',
          );
        }
      }
      if (const {
        '+',
        '-',
        '*',
        '/',
        '%',
        '<',
        '>',
        '<=',
        '>=',
      }.contains(name)) {
        String? classOf(Expression e) {
          final t = _staticType(e);
          return t is InterfaceType ? t.classNode.name : null;
        }

        // The receiver's *static* class, not the operator's owner: an
        // `int * double` may resolve to `num.*`.
        final leftClass = classOf(node.receiver);
        final rightClass = classOf(node.arguments.positional.single);
        // Not `num`: a static type of `num` is an `i64` as often as an
        // `f64` in the output (round ws49: 580 casts the wrong way).
        if (leftClass == 'int' && rightClass == 'double') {
          left = IrCast(left, 'f64');
        }
        if (leftClass == 'double' && rightClass == 'int') {
          right = IrCast(right, 'f64');
        }
        // A *declared* `num` -- a variable, field or static whose declaration
        // says `num`, an `f64` here -- against an int literal: the literal
        // is cast. Not the static type: `getStaticType` says `num` for an
        // `int` assignment used as a value (`(index = next()) >= 0`), and a
        // cast on that went wrong 200 times (ws53).
        final argument = node.arguments.positional.single;
        if (_declaredNum(node.receiver) &&
            (argument is IntLiteral || classOf(argument) == 'int')) {
          right = IrCast(right, 'f64');
        } else if (_declaredNum(argument) && leftClass == 'int') {
          left = IrCast(left, 'f64');
        }
        // Dart's `/` is always a `double`, even on two `int`s (`~/` is the
        // integer one); Rust's `/` on two `i64`s is an `i64`.
        if (name == '/') {
          if (leftClass == 'int') left = IrCast(left, 'f64');
          if (rightClass == 'int') right = IrCast(right, 'f64');
          // `targetWidth! / (w / h)`: whatever the static type of the left
          // side says, a `/` with a `double` right side is a `double`
          // division, and Rust has no `i64 / f64`.
          if (rightClass == 'double' && leftClass != 'double') {
            left = IrCast(left, 'f64');
          }
        }
      }
      return IrBinary(
        name,
        left,
        right,
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
    // Dart's `double.floor()`/`ceil()`/`round()` are `int`s; Rust's are
    // `f64`s, inherent, and so not renameable through `DartDouble`. 10
    // `i64 <= f64` in material_color_utilities' HCT solver.
    // A `num` method on a `dynamic` receiver -- `number.isInfinite` in
    // intl's `format(dynamic number)`, devirtualised to `num.isInfinite` by
    // TFA: the receiver is downcast to the `f64` a `num` is here. An `int`
    // inside the `Rc<dyn Object>` would fail that downcast, loudly.
    final receiverStatic = _staticType(node.receiver);
    if ((receiverStatic is DynamicType ||
            (receiverStatic is InterfaceType &&
                receiverStatic.classNode.name == 'Object')) &&
        const {'num', 'int', 'double'}.contains(owner) &&
        const {
          'isInfinite',
          'isNaN',
          'isFinite',
          'round',
          'floor',
          'ceil',
          'truncate',
          'toDouble',
          'toInt',
          'abs',
          'toStringAsFixed',
        }.contains(name)) {
      final asDouble = IrCall(
        IrDowncast(expression(node.receiver), 'f64'),
        'clone',
        const [],
      );
      final rounds =
          const {'floor', 'ceil', 'round'}.contains(name) && args.isEmpty;
      final call = IrCall(asDouble, name, args);
      return rounds ? IrCast(call, 'i64') : call;
    }
    if (const {'floor', 'ceil', 'round'}.contains(name) && args.isEmpty) {
      final receiverType = _staticType(node.receiver);
      if (receiverType is InterfaceType &&
          (receiverType.classNode.name == 'double' ||
              receiverType.classNode.name == 'num')) {
        return IrCast(IrCall(expression(node.receiver), name, const []), 'i64');
      }
    }
    final receiver = node.receiver;
    // A translated generic method's type arguments (see `IrCall.
    // typeArguments`); a `dart:` class's method takes none in the prelude.
    final target = node.interfaceTarget;
    final withTypeArgs =
        target is Procedure &&
        target.function.typeParameters.isNotEmpty &&
        target.enclosingClass != null &&
        _translatedClass(target.enclosingClass!) &&
        node.arguments.types.length == target.function.typeParameters.length;
    return _qualified(
      IrCall(
        receiver is ThisExpression ? null : expression(receiver),
        name,
        args,
        typeArguments: withTypeArgs
            ? [for (final t in node.arguments.types) _type(t)]
            : const [],
      ),
      node.interfaceTarget,
      receiver,
    );
  }

  /// See `IrCall.qualifier`: a member whose name two classes in the
  /// receiver's hierarchy declare is called through one of them by name.
  IrCall _qualified(IrCall call, Member member, Expression receiver) {
    final owner = member.enclosingClass;
    if (owner == null || !_translatedClass(owner)) {
      return _fails(member)
          ? IrCall(
              call.target,
              call.name,
              call.args,
              fails: true,
              diverges: _diverges(member),
            )
          : call;
    }
    // From the receiver's own class: a mixin's `child` is declared again
    // by the trait of the class that mixes it in, *below* the owner.
    final type = receiver is ThisExpression ? null : _staticType(receiver);
    final from = receiver is ThisExpression
        ? _member?.enclosingClass
        : type is InterfaceType
        ? type.classNode
        : null;
    var qualifier = _qualifierFor(from ?? owner, member);
    // `this.x` where a trait declared `x` and this class overrides it: Rust
    // resolves `self.x()` to the inherent override, whose type may be
    // narrower than the declaration the kernel typed the read by (`String?
    // get restorationId` overridden as `String`: 72 `unwrap` on a `String`
    // at ws296). Through the trait, whose signature the kernel agrees with.
    // The *lowering* class, not the member's: a mixin's body is lowered
    // into the class applying it, and there `this` is that class.
    final host = _lowering ?? from;
    if (qualifier == null &&
        receiver is ThisExpression &&
        host != null &&
        host != owner &&
        _abstractLike(owner) &&
        !owner.isAnonymousMixin &&
        host.members.any(
          (m) =>
              m.name.text == member.name.text &&
              ((m is Procedure && !m.isStatic) || (m is Field && !m.isStatic)),
        )) {
      qualifier = owner.name;
    }
    final fails = _fails(member);
    final renamed = member is Procedure && member.name.text == 'clone';
    if (qualifier == null && !fails && !renamed) return call;
    return IrCall(
      call.target,
      renamed ? _dartName(call.name) : call.name,
      call.args,
      qualifier: qualifier,
      receiverClass: type is InterfaceType ? type.classNode.name : null,
      fails: fails,
      diverges: _diverges(member),
      typeArguments: call.typeArguments,
    );
  }

  /// A member declared to return `Never`.
  static bool _diverges(Member m) =>
      m is Procedure && m.function.returnType is NeverType;

  /// The trait to call `member` through from a value of class `from`, or
  /// null when only one class in the hierarchy declares it and the plain
  /// call is unambiguous.
  ///
  /// A member the CFE cloned into an anonymous mixin application
  /// (`_MixinApplication8&RenderBox&RenderObjectWithChildMixin.child`) is
  /// declared twice on the Rust side: by the mixin's trait and, flattened
  /// (ws112), by the trait of the class that applies it. That class is
  /// the name to call through -- the mixin's trait is not a supertrait of
  /// its, so inside a super function `__Self: ListNotifier` cannot reach
  /// `ListNotifierMixin::_updaters` (295 E0277s the round the mixin was
  /// named instead).
  /// The qualifier a setter call takes (see `IrSetter.qualifier`).
  String? _setterQualifier(Expression? receiver, Member target) {
    final owner = target.enclosingClass;
    if (owner == null || !_translatedClass(owner)) return null;
    final Class? from;
    if (receiver == null || receiver is ThisExpression) {
      from = _lowering ?? _member?.enclosingClass;
    } else {
      final t = _staticType(receiver);
      from = t is InterfaceType ? t.classNode : null;
    }
    if (from == null) return null;
    return _qualifierFor(from, target);
  }

  String? _classNameOf(Expression receiver) {
    final t = _staticType(receiver);
    return t is InterfaceType ? t.classNode.name : null;
  }

  String? _qualifierFor(Class from, Member member) {
    final owner = member.enclosingClass!;
    final name = member.name.text;
    final setter = member is Procedure && member.isSetter;
    final seen = <Class>{};
    var found = 0;
    String? applier;
    Member? declared(Class c) {
      for (final m in c.members) {
        if (m.name.text == name &&
            (m is Field || (m is Procedure && m.isSetter == setter))) {
          return m;
        }
      }
      return null;
    }

    // `named`: the nearest class with a name of its own on the superclass
    // path down to `c`, which is where an anonymous application's members
    // were flattened to.
    void walk(Class c, Class named) {
      if (!seen.add(c)) return;
      final m = _translatedClass(c) ? declared(c) : null;
      if (m != null) {
        if (c.isAnonymousMixin) {
          // A cloned *abstract* member (`ScrollMetrics.axisDirection` in
          // `ScrollPosition with ScrollMetrics`) is flattened nowhere: its
          // one declaration is the mixin trait's.
          if (m.isAbstract) {
            found += 1;
          } else {
            found += 2;
            applier ??= named.name;
          }
        } else {
          found += 1;
        }
      }
      final below = c.isAnonymousMixin ? named : c;
      final superclass = c.superclass;
      if (superclass != null) walk(superclass, below);
      for (final s in c.supers) {
        if (s.classNode != superclass) walk(s.classNode, s.classNode);
      }
    }

    // `this` inside a member cloned into an application is the named class
    // the application is lowered into.
    walk(from, from.isAnonymousMixin ? (_lowering ?? from) : from);
    if (found < 2) return null;
    // Through the applying class whenever an application declares it --
    // also when the resolved owner is the mixin itself (`this._notifyUpdate`
    // inside `ListNotifier with ListNotifierMixin`): the mixin's trait is
    // not among a subclass trait's supertraits, the applier's is. An
    // abstract member of an anonymous owner is the mixin's, named by the
    // application's last segment.
    final chosen =
        applier ??
        (owner.isAnonymousMixin ? owner.name.split('&').last : owner.name);
    // Never a synthetic name: 509 "expected value, found trait" the round
    // one got through.
    return chosen.contains('&') ? null : chosen;
  }

  bool _translatedClass(Class c) {
    // The CFE's deduplicated mixin applications (`_MixinApplication8&
    // RenderBox&RenderObjectWithChildMixin`) live in a synthetic library
    // whose scheme is not a package's; they are the mixin's members
    // under another name, translated like it.
    if (c.isAnonymousMixin) return true;
    final uri = c.enclosingLibrary.importUri;
    return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
  }

  IrExpr _construct(ConstructorInvocation node) {
    final target = node.target;
    final name = target.name.text;
    // `ListQueue([capacity])`: the prelude's `Queue` (a `VecDeque`), and
    // the capacity hint is dropped.
    if (const {
          'ListQueue',
          'Queue',
          'DoubleLinkedQueue',
        }.contains(target.enclosingClass.name) &&
        name.isEmpty) {
      return IrNew(const IrType('Queue'), const []);
    }
    // `HashMap(equals: .., hashCode: .., isValidKey: ..)` and `LinkedHashMap`
    // likewise: the prelude's one `Map`, and the custom key equality is
    // dropped -- recorded as the approximation it is (collection's
    // `MapEquality` builds such a map to count entries).
    if (const {
          'HashMap',
          'LinkedHashMap',
        }.contains(target.enclosingClass.name) &&
        name.isEmpty) {
      return IrNew(const IrType('Map'), const []);
    }
    // `Object()`: an identity and nothing else, the prelude's `new_object`.
    if (target.enclosingClass.name == 'Object' &&
        node.arguments.positional.isEmpty &&
        node.arguments.named.isEmpty) {
      return IrStaticCall(null, 'new_object', const []);
    }
    // The constructor's parameters in the constructed type's terms:
    // `Tween<double>(begin: 0)` takes a `T?`, which is a `double?` here.
    final cls = target.enclosingClass;
    // The type arguments come along, as a turbofish where the class is
    // generic: `_FooState<T>()` in `createState` says which `T` (27).
    final created = IrNew(
      IrType(
        _instanceName(cls),
        arguments: _erasedArguments(cls, node.constructedType.typeArguments),
      ),
      _arguments(
        node.arguments,
        target.function,
        false,
        _instantiatedConstructor(node),
      ),
      constructor: name.isEmpty ? null : name,
    );
    // An open class's instance is its `Impl` struct, and every slot typed
    // with the class is the trait handle: the construction leaves as one
    // (570 `SizeImpl` where an `Rc<dyn Size>` was wanted).
    return _isOpen(cls)
        ? IrUpcast(created, _type(node.constructedType))
        : created;
  }

  /// The struct an instance of `cls` is: the class's own name, or the
  /// `Impl` beside an open class's trait.
  String _instanceName(Class cls) =>
      _isOpen(cls) ? implName(cls.name) : cls.name;

  static FunctionType? _instantiatedConstructor(ConstructorInvocation node) {
    final cls = node.target.enclosingClass;
    if (cls.typeParameters.isEmpty) return null;
    final declared = node.target.function.computeThisFunctionType(
      Nullability.nonNullable,
    );
    final substituted = Substitution.fromInterfaceType(node.constructedType)
        .substituteType(declared);
    return substituted is FunctionType ? substituted : null;
  }

  /// A generic function's type at this call: `_futurize<int>(callbacker)`
  /// takes a `String? Function(_Callback<int>)`, and the closure passed is
  /// typed against that, not against the `T` the declaration wrote.
  static FunctionType? _instantiated(StaticInvocation node) {
    final fn = node.target.function;
    if (fn.typeParameters.isEmpty ||
        node.arguments.types.length != fn.typeParameters.length) {
      return null;
    }
    final declared = fn.computeFunctionType(Nullability.nonNullable);
    final instantiated = FunctionTypeInstantiator.instantiate(
      declared,
      node.arguments.types,
    );
    return instantiated is FunctionType ? instantiated : null;
  }

  IrExpr _staticInvocation(StaticInvocation node) {
    // TFA spells a cast it has proven, or one it cannot check, as
    // `unsafeCast<Clock?>(Zone.current[_clockKey])`. It is the `as` it
    // replaced, and lowers as one -- without this the operand stood in for
    // the whole and an `Rc<dyn Object>` landed in an `Option<Clock>`.
    if (node.target.name.text == 'unsafeCast' &&
        node.target.enclosingLibrary.importUri.toString() == 'dart:_internal' &&
        node.arguments.positional.length == 1 &&
        node.arguments.types.length == 1) {
      // ..and into the cast's type: `unsafeCast<double?>(#1{Size}.width)`
      // hands a `double` to a `double?` (`Some(..)`), which the CFE's `#0`
      // above it is declared as.
      final operand = node.arguments.positional.single;
      final to = node.arguments.types.single;
      final lowered = expression(AsExpression(operand, to));
      // Only that shape: a non-null `T` into a `T?`. Widening every
      // `unsafeCast` doubled `Option`s and unwrapped `dynamic`s (+17, ws159).
      final from = _staticType(operand);
      if (from is InterfaceType &&
          to is InterfaceType &&
          from.classNode == to.classNode &&
          from.nullability != Nullability.nullable &&
          to.nullability == Nullability.nullable) {
        return IrSome(lowered);
      }
      return lowered;
    }
    final target = node.target;
    final positional = node.arguments.positional;
    // Two of dart:math's, and one of Flutter's own. Rust has all three, and
    // `max` is the same spelling for floats and integers because `f32::max` is
    // inherent and `Ord::max` covers the rest. 372 `max` and 184 `clampDouble`.
    const arithmetic = {'max': 'max', 'min': 'min'};
    final rust = arithmetic[target.name.text];
    if (rust != null && positional.length == 2) {
      // `max(0, x)` with an `int` and a `double`: the `int` is cast, as
      // the operators cast theirs (`0.max(f64)` was 3 `found integer`s).
      String? cls(Expression e) {
        final t = _staticType(e);
        return t is InterfaceType ? t.classNode.name : null;
      }

      var a = expression(positional[0]);
      var b = expression(positional[1]);
      if (cls(positional[0]) == 'int' && cls(positional[1]) == 'double') {
        a = IrCast(a, 'f64');
      } else if (cls(positional[0]) == 'double' &&
          cls(positional[1]) == 'int') {
        b = IrCast(b, 'f64');
      }
      return IrCall(a, rust, [b]);
    }
    // The CFE lowers `<int>[3, 11, 29]` to `_GrowableList._literal3(..)`, so a
    // list literal never reaches this compiler as a ListLiteral. Restored
    // rather than transliterated, for the same reason `??` and cascades are:
    // the analyzer front end sees the literal, and the two have to agree.
    final owner = target.enclosingClass?.name;
    // `Uint8List.view(buffer, [offset, length])`: a `Vec<u8>` cannot carry
    // an associated function; the prelude's free one.
    if (owner == 'Uint8List' &&
        target.name.text == 'view' &&
        positional.isNotEmpty) {
      return IrStaticCall(
        null,
        'uint8_list_view',
        _arguments(node.arguments, target.function),
      );
    }
    // `int.parse(s)` / `double.tryParse(s)`: the prelude's four functions.
    // `intl`'s field parsers and 30-odd other sites.
    if ((owner == 'int' || owner == 'double') &&
        (target.name.text == 'parse' || target.name.text == 'tryParse') &&
        positional.length == 1 &&
        node.arguments.named.isEmpty) {
      final fn =
          '${target.name.text == 'parse' ? 'parse' : 'try_parse'}_$owner';
      return IrStaticCall(null, fn, [expression(positional[0])]);
    }
    // `_List<T?>(n)` -- `List.filled(n, null)` after the CFE -- is a list of
    // `n` nulls, which for a nullable element is exactly what it says: the
    // prelude's `vec_of_nones`. A non-nullable element has nothing to fill
    // with and stays refused in the backend. `_makeArray` in
    // `persistent_hash_map.dart`, and everything hashing through it.
    if (owner == '_List' &&
        target.name.text.isEmpty &&
        positional.length == 1 &&
        node.arguments.types.length == 1 &&
        node.arguments.types.single.nullability == Nullability.nullable) {
      return IrStaticCall(null, 'vec_of_nones', [expression(positional[0])]);
    }
    // `_GrowableList(0)` -- `List.empty(growable: true)` and `<T>[]` after
    // the CFE -- is an empty list. With a length it would be `n` nulls,
    // which for a non-nullable element has nothing to fill with; that one
    // still stops in the backend.
    if (owner == '_GrowableList' &&
        target.name.text.isEmpty &&
        positional.length == 1 &&
        positional.single is IntLiteral &&
        (positional.single as IntLiteral).value == 0) {
      return IrListLiteral(
        const [],
        _type(node.arguments.types.singleOrNull ?? const DynamicType()),
      );
    }
    if ((owner == '_GrowableList' || owner == '_List') &&
        target.name.text.startsWith('_literal')) {
      // Each element widens into the element type, and a local named as an
      // element is cloned (`[left, right]` moved `left`).
      final element = node.arguments.types.singleOrNull;
      // ..and into an `Object?` element (`Object.hashAll([isChecked, ..])`
      // over enums and structs) each is shared, as an argument would be.
      return IrListLiteral([
        for (final e in positional)
          _intoObject(
            e,
            element,
            _widened(
              e,
              element,
              _withExpectedReturn(element, e, () => expression(e)),
            ),
          ),
      ], _type(element ?? const DynamicType()));
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
    // The rest of `dart:math`'s functions are methods on `f64` in Rust,
    // spelled almost the same. `log` was refused as a top-level nothing
    // declared, and took `ClampingScrollSimulation._kDecelerationRate` and
    // everything reading it with it.
    const unary = {
      'log': 'ln',
      'exp': 'exp',
      'sqrt': 'sqrt',
      'sin': 'sin',
      'cos': 'cos',
      'tan': 'tan',
      'asin': 'asin',
      'acos': 'acos',
      'atan': 'atan',
    };
    // `dart:math` takes `num`s; Rust's are methods of `f64`, so an `int`
    // argument (`log(10)`, `pow(10, n)`) is cast first.
    IrExpr asDouble(Expression e) {
      final t = _staticType(e);
      final lowered = expression(e);
      // A literal has no static type without a context (`log(10)` in a
      // static's initialiser), and is an `int` by its spelling.
      return e is IntLiteral ||
              (t is InterfaceType && t.classNode.name == 'int')
          ? IrCast(lowered, 'f64')
          : lowered;
    }

    if ('${target.enclosingLibrary.importUri}' == 'dart:math') {
      final method = unary[target.name.text];
      if (method != null && positional.length == 1) {
        return IrCall(asDouble(positional[0]), method, const []);
      }
      if (target.name.text == 'atan2' && positional.length == 2) {
        return IrCall(asDouble(positional[0]), 'atan2', [
          asDouble(positional[1]),
        ]);
      }
    }
    if (target.name.text == 'pow' && positional.length == 2) {
      return IrCall(asDouble(positional[0]), 'powf', [asDouble(positional[1])]);
    }
    if (target.name.text == 'clampDouble' && positional.length == 3) {
      return IrCall(expression(positional[0]), 'clamp', [
        expression(positional[1]),
        expression(positional[2]),
      ]);
    }
    if (target.name.text == 'unsafeCast' && positional.length == 1) {
      // The CFE's own cast, inserted where it has already proved the type. It
      // does nothing at runtime in Dart; here a cast from a trait object to
      // the struct it proved -- `unsafeCast<_NativePath>(path)` in front of
      // every native taking one -- is the downcast through `Any`.
      final from = _staticType(positional.single);
      final to = node.arguments.types.singleOrNull;
      final fromTraitObject =
          from is DynamicType ||
          (from is InterfaceType &&
              from.nullability != Nullability.nullable &&
              (_abstractLike(from.classNode) ||
                  from.classNode.name == 'Object'));
      if (fromTraitObject &&
          to is InterfaceType &&
          to.nullability != Nullability.nullable &&
          !_abstractLike(to.classNode) &&
          to.classNode.name != 'Object' &&
          (from is! InterfaceType || from.classNode != to.classNode)) {
        // The Rust name: `double` is an `f64` (`arg is double` after TFA).
        return IrCall(
          IrDowncast(
            expression(positional.single),
            _rustScalar(_type(to).name),
          ),
          'clone',
          const [],
        );
      }
      return expression(positional.single);
    }
    // `LinkedHashMap(equals: .., hashCode: ..)` as a factory: the prelude's
    // `Map`, the custom equality dropped (see `_construct`).
    if (const {
          'HashMap',
          'LinkedHashMap',
          'LinkedHashSet',
          'HashSet',
        }.contains(owner) &&
        target.name.text.isEmpty &&
        target.kind == ProcedureKind.Factory) {
      return IrNew(IrType(owner!.contains('Set') ? 'Set' : 'Map'), const []);
    }
    // `String.fromCharCodes(codes)`: a free function of the prelude's, since
    // Rust's `String` takes no inherent additions.
    if (owner == 'String' &&
        target.name.text == 'fromCharCodes' &&
        positional.length >= 1) {
      return IrStaticCall(null, 'string_from_char_codes', [
        expression(positional[0]),
      ]);
    }
    // `scheduleMicrotask(f)`: the prelude's `_schedule_microtask` takes the
    // `Rc<dyn Fn()>` a translated closure is; the public-named one is the
    // prelude's own `Box<dyn FnOnce()>` entry.
    if (target.name.text == 'scheduleMicrotask' &&
        positional.length == 1 &&
        target.enclosingLibrary.importUri.toString() == 'dart:async') {
      return IrStaticCall(
        null,
        '_schedule_microtask',
        _arguments(node.arguments, target.function),
      );
    }
    if (target.name.text == 'identical' && positional.length == 2) {
      return IrIdentical(expression(positional[0]), expression(positional[1]));
    }
    if (owner == null) {
      // A top-level function, this library's or another's. Which of those it
      // is no longer decides anything here: whether the callee exists in the
      // output is a whole-crate question, and the crate is not known until
      // every library has been lowered, so the backend asks it instead. The
      // analyzer front end never made the distinction, so this is also one
      // fewer place the two of them could differ.
      // The same cleaning `_lowerTopLevel` gives the declaration: an
      // extension member's `Ext|get#name` has to be one identifier at both
      // ends, and the crate-wide "does the callee exist" check compares them.
      return IrStaticCall(
        null,
        target.name.text.replaceAll(RegExp(r'[|#]'), '_'),
        _arguments(node.arguments, target.function, true, _instantiated(node)),
        fails: _fails(target),
        diverges: _diverges(target),
      );
    }
    return IrStaticCall(
      owner,
      // An unnamed factory -- `factory Vector3(x, y, z)` -- has no name in
      // Kernel at all. The backend spells an empty static name `new`, for a
      // prelude class as much as a translated one, and `_lowerProcedure`
      // declares the factory under that name.
      target.name.text,
      // With the call's type arguments: `WidgetStateProperty.resolveWith<
      // Color?>((states) { .. })` expects the closure to return `Color?`,
      // and the declared `T` said nothing (63 `Option<Color>` <- `Color`).
      _arguments(node.arguments, target.function, true, _instantiated(node)),
      fails: _fails(target),
      diverges: _diverges(target),
    );
  }

  /// Arguments in the callee's declaration order.
  ///
  /// Kernel has already split them into positional and named, and a named one
  /// that was omitted is simply absent -- so the callee's own parameter list is
  /// still what decides the order, exactly as in the analyzer front end.
  List<IrExpr> _arguments(
    Arguments node, [
    FunctionNode? callee,
    bool borrows = true,
    FunctionType? instantiated,
  ]) {
    final was = _borrowedArgument;
    _borrowedArgument = borrows;
    try {
      return _argumentList(node, callee, instantiated);
    } finally {
      _borrowedArgument = was;
    }
  }

  /// One argument, lowered knowing what the callee does with it.
  ///
  /// A closure may borrow only if the callee is *finished with it* when it
  /// returns. Round 59 asked the weaker question -- "is this an argument" --
  /// and `bin/census_escapes.dart` measured what that costs: of 1234 closures
  /// handed to a call, 394 are kept by the callee. `addListener`,
  /// `scheduleMicrotask`, `Timer`, `WidgetStateProperty.resolveWith`: storing
  /// one needs `'static`, and a borrow cannot give it. Those go back to being
  /// refused, which is the truth about them until objects are counted.
  IrExpr _argument(
    Expression value,
    FunctionNode? callee,
    int index, [
    FunctionType? instantiated,
  ]) {
    final param = callee != null && index < callee.positionalParameters.length
        ? callee.positionalParameters[index]
        : null;
    // The *instantiated* parameter type when the call site has one: a
    // `List<Shadow>.add(E)` takes a `Shadow`, and the `E` alone could
    // widen nothing (`Shadow <= Option<Shadow>` after TFA dropped a `!`).
    // ..but the *declared* one when it is a function type naming an erased
    // parameter: the instantiated `bool Function(ScrollNotification)` is
    // not what the slot holds, `bool Function(T)` erased is.
    final declaredType = param?.type;
    final paramType =
        declaredType is FunctionType && _mentionsErased(declaredType)
        ? declaredType
        : instantiated != null &&
              index < instantiated.positionalParameters.length
        ? instantiated.positionalParameters[index]
        : declaredType;
    return _intoDynamic(
      value,
      paramType,
      callee,
      _numLiteral(
        value,
        paramType,
        callee,
        _widened(
          value,
          paramType,
          _withBorrowing(
            param,
            callee,
            () =>
                _withExpectedReturn(paramType, value, () => expression(value)),
          ),
        ),
      ),
      generic: param?.type is TypeParameterType,
    );
  }

  /// A closure literal handed to a function-typed parameter returns what
  /// the *parameter's* type says: `String? Function(String)` taking
  /// `(l) => "default"` returns `Some("default")`. The closure's own
  /// return type is what it wrote, not what it is for (5 in intl).
  IrExpr _withExpectedReturn(
    DartType? param,
    Expression value,
    IrExpr Function() lower,
  ) {
    // Through the cast the CFE wraps a closure argument in (`(chunk) =>
    // ..` as `void Function(List<int>)?` for `listen`): the closure under
    // it is what the expected type is for.
    var closure = value;
    while (closure is AsExpression) closure = closure.operand;
    if (param is! FunctionType || closure is! FunctionExpression)
      return lower();
    final was = _expectedReturn;
    final wasFunction = _expectedFunction;
    _expectedReturn = param.returnType;
    _expectedFunction = param;
    try {
      return lower();
    } finally {
      _expectedReturn = was;
      _expectedFunction = wasFunction;
    }
  }

  /// The function type the next lowered closure is expected to have: its
  /// parameters stand in for a closure's own `dynamic` ones. `(locale) =>
  /// ..` in a `List<String Function(String)>` literal is inferred with a
  /// `dynamic` parameter by the CFE, and the `Rc<dyn Fn(String) -> String>`
  /// the list holds does not take an `Rc<dyn Object>`.
  FunctionType? _expectedFunction;

  /// The return type the next lowered body should widen into, if a
  /// parameter's function type says so.
  DartType? _expectedReturn;

  /// An int literal into a parameter a *translated* callee declares `num`
  /// (an `f64` here) is cast. Not a `dart:` callee's: `int.+(num other)` is
  /// declared that way and its `num` is not an `f64` (ws54).
  IrExpr _numLiteral(
    Expression value,
    DartType? param,
    FunctionNode? callee,
    IrExpr lowered,
  ) {
    if (param is! InterfaceType || param.classNode.name != 'num')
      return lowered;
    // A literal, or a value whose static type is `int` (a translated
    // callee's `num` is an `f64`, so either is cast).
    final given = _staticType(value);
    final isInt =
        value is IntLiteral ||
        (given is InterfaceType &&
            given.classNode.name == 'int' &&
            given.nullability != Nullability.nullable);
    if (!isInt) return lowered;
    final member = callee?.parent;
    if (member is! Member) return lowered;
    if (member.enclosingLibrary.importUri.scheme == 'dart') return lowered;
    return IrCast(lowered, 'f64');
  }

  IrExpr _namedArgument(Expression value, Object param) {
    final callee = _calleeOf(param);
    final type = param is FunctionParameter ? param.type : null;
    return _intoDynamic(
      value,
      type,
      callee,
      _numLiteral(
        value,
        type,
        callee,
        _widened(
          value,
          type,
          _withBorrowing(
            param,
            callee,
            () => _withExpectedReturn(type, value, () => expression(value)),
          ),
        ),
      ),
    );
  }

  /// A value handed to a translated callee's `dynamic`/`Object` parameter
  /// is shared into the `Rc<dyn Object>` that parameter is: `Rc::new(..)`
  /// around a `bool` or an `Exception` (5 in dart:ui), and `Some(..)` too
  /// when the parameter is `Object?`. A prelude callee -- `print`,
  /// `StringBuffer.write`, `Object.hash` -- is generic over what it takes
  /// and is left alone; a counted class is already a handle and unsizes.
  IrExpr _intoDynamic(
    Expression value,
    DartType? param,
    FunctionNode? callee,
    IrExpr lowered, {
    bool generic = false,
  }) {
    if (param == null || callee == null) return lowered;
    final member = callee.parent;
    if (member is! Member) return lowered;
    final uri = member.enclosingLibrary.importUri;
    // The prelude's exceptions take their `Object?` as `Option<Rc<dyn
    // Object>>` like a translated class would: `FormatException(msg,
    // source)` with a `String` source shares it.
    // Only `FormatException`: `Exception(message)` and `ArgumentError
    // .value(..)` take strings in the prelude, and sharing into them was 27
    // mismatches (ws144).
    const preludeObjects = {'FormatException'};
    final owner = member.enclosingClass?.name;
    // ..but a *generic* slot of a prelude collection instantiated to
    // `Object?` holds what a translated class would: `<Object?>[..]` with
    // spreads is `list.add(e)` on a `List<Object?>`, and the `Vec<Option<
    // Rc<dyn Object>>>` wants each element shared and `Some`d (128
    // "expected `Option<Rc<dyn Object>>`" at ws277).
    if (uri.scheme == 'dart' &&
        uri.toString() != 'dart:ui' &&
        !generic &&
        !(owner != null && preludeObjects.contains(owner))) {
      return lowered;
    }
    // The prelude spells those exceptions' `dynamic source` as an
    // `Option<Rc<dyn Object>>`: into it as into an `Object?`.
    if (owner != null &&
        preludeObjects.contains(owner) &&
        param is DynamicType) {
      final env = typeEnvironment;
      if (env != null) {
        return _intoObject(value, env.coreTypes.objectNullableRawType, lowered);
      }
    }
    // `dart:core`'s `Pattern` is a `String` or a `RegExp`; the prelude's
    // struct holds either, and a call into *translated* code converts
    // (`FilteringTextInputFormatter.deny('\n')`). Not a `dart:` callee's:
    // the prelude's own `split`/`contains` take the `String`.
    if (param is InterfaceType &&
        param.classNode.name == 'Pattern' &&
        param.classNode.enclosingLibrary.importUri.toString() == 'dart:core') {
      final given = _staticType(value);
      if (given is InterfaceType && given.classNode.name == 'String') {
        final made = IrStaticCall('Pattern', 'of_string', [lowered]);
        return param.nullability == Nullability.nullable ? IrSome(made) : made;
      }
      if (given is InterfaceType && given.classNode.name == 'RegExp') {
        final made = IrStaticCall('Pattern', 'of_regexp', [lowered]);
        return param.nullability == Nullability.nullable ? IrSome(made) : made;
      }
    }
    return _intoObject(value, param, lowered);
  }

  /// The sharing `_intoDynamic` does, for any `Object`/`dynamic` slot.
  IrExpr _intoObject(Expression value, DartType? param, IrExpr lowered) {
    if (param == null) return lowered;
    final isObject =
        param is DynamicType ||
        (param is InterfaceType && param.classNode.name == 'Object');
    if (!isObject) return lowered;
    if (_isNull(value) || lowered is IrClosure) return lowered;
    final given = _staticType(value);
    // A `num` method called on a `dynamic` (`number.abs()`) was lowered to
    // a call on an `f64`: the value is a scalar, whatever Kernel says.
    if (given is DynamicType &&
        value is DynamicInvocation &&
        (_dynamicNumMethods.contains(value.name.text) ||
            _dynamicNumOperators.contains(value.name.text))) {
      final shared = IrCall(lowered, '!rc_object', const []);
      return param is! DynamicType && param.nullability == Nullability.nullable
          ? IrSome(shared)
          : shared;
    }
    if (given == null || given is DynamicType || given is NullType)
      return lowered;
    if (given is InterfaceType && given.classNode.name == 'Object')
      return lowered;
    final counted =
        given is InterfaceType && _closureCallsMethod(given.classNode);
    // An int literal into an `Object` slot is an `i64`, not the `i32`
    // inference would pick for `Rc::new(0)`.
    if (value is IntLiteral) lowered = IrCast(lowered, 'i64');
    if (given.nullability == Nullability.nullable) {
      // `ByteData? args` into an `Object?`: shared element by element.
      if (counted || param.nullability != Nullability.nullable) return lowered;
      final shared = IrNullAware(
        lowered,
        IrCall(
          IrCall(const IrBound(), 'clone', const []),
          '!rc_object',
          const [],
        ),
      );
      // A `dynamic` slot is never an `Option`: Dart's null there is the
      // `Null` object (`_isNullOrEmpty(_value)` with a `T?` in `StateMixin`).
      return param is DynamicType
          ? IrCall(shared, '!or_null', const [])
          : shared;
    }
    // A counted class's handle unsizes to `Rc<dyn Object>` only where the
    // target type is written; a `dynamic` static's initialiser names it.
    final shared = counted
        ? IrCall(lowered, '!as_object', const [])
        : IrCall(lowered, '!rc_object', const []);
    // `dynamic` is "nullable" to Kernel and is never an `Option` here.
    final wantsSome =
        param is! DynamicType && param.nullability == Nullability.nullable;
    return wantsSome ? IrSome(shared) : shared;
  }

  /// `Some(..)` around a non-null argument handed to a nullable parameter --
  /// Dart's silent widening, spelled. Only when the static type says the
  /// argument is not itself nullable, so a nullable variable passed on stays
  /// as it is.
  /// A map literal's entries, each key and value into the map's own types,
  /// sharing into an `Object?` value (`{'extension': name, 'value': value}`
  /// handed to `postEvent` as a `Map<String, Object?>`), as a list literal's
  /// are. The types are a parameter: a literal spread into a wider map --
  /// `<SingleActivator, Intent>{..}` into the `Map<ShortcutActivator,
  /// Intent>` of `DefaultTextEditingShortcuts` -- is lowered against the
  /// wider one's, so each key is shared into its `Rc<dyn ..>` (121
  /// "arguments incorrect" on one file in `widgets`).
  IrExpr _mapLiteral(MapLiteral node, DartType keyType, DartType valueType) {
    return IrMapLiteral(
      [
        for (final entry in node.entries)
          (
            _intoObject(
              entry.key,
              keyType,
              _widened(entry.key, keyType, expression(entry.key)),
            ),
            _intoObject(
              entry.value,
              valueType,
              _widened(entry.value, valueType, expression(entry.value)),
            ),
          ),
      ],
      _type(keyType),
      _type(valueType),
    );
  }

  /// A list literal's elements into `element`. The CFE keeps a literal of
  /// more than eight elements as a node (the `_literalN` constructors stop
  /// there): its elements widen and share into the element type exactly as
  /// the short ones' do.
  IrExpr _listLiteral(ListLiteral node, DartType element) {
    return IrListLiteral([
      for (final e in node.expressions)
        _intoObject(
          e,
          element,
          _widened(
            e,
            element,
            _withExpectedReturn(element, e, () => expression(e)),
          ),
        ),
    ], _type(element));
  }

  IrExpr _widened(Expression value, DartType? param, IrExpr lowered) {
    // A literal into a collection slot of other element types is lowered
    // again against those: see `_mapLiteral`.
    if (param is InterfaceType && param.nullability != Nullability.nullable) {
      final args = param.typeArguments;
      if (value is MapLiteral &&
          param.classNode.name == 'Map' &&
          args.length == 2 &&
          (args[0] != value.keyType || args[1] != value.valueType)) {
        return _mapLiteral(value, args[0], args[1]);
      }
      if (value is ListLiteral &&
          (param.classNode.name == 'List' ||
              param.classNode.name == 'Iterable') &&
          args.length == 1 &&
          args[0] != value.typeArgument) {
        return _listLiteral(value, args[0]);
      }
    }
    // A local handed on is shared in Dart and moved in Rust: `string` passed
    // to `StringCharacterRange` and then read again, `listener` moved into a
    // closure "in a previous iteration of loop" -- 21 `E0382`s. A clone of a
    // `String` or an `Rc` is the sharing Dart meant. A list or map is not
    // cloned: a copy of one would be a different list, and the aliasing
    // Dart meant is not something a clone can give.
    if (value is VariableGet && _clonedWhenPassed(value.variable.type)) {
      lowered = IrCall(lowered, 'clone', const []);
    }
    // Type flow analysis narrows a parameter to the one class that reaches
    // it -- `_pushClipPath(.., _NativePath path, ..)` -- and the caller
    // still holds a `Path`. Kernel writes no cast for that; the downcast
    // through `Any` is the same one `path as _NativePath` takes.
    // ..as the closure parameter was retyped, when it was.
    final given = value is VariableGet && _retyped.containsKey(value.variable)
        ? _retyped[value.variable]
        : _staticType(value);
    // A function whose parameter is *wider* than the slot's -- `callback`,
    // a `void Function(int?)`, handed to `_initFromAsset(.., void
    // Function(int))` -- is fine in Dart and a different `Fn` in Rust. An
    // adapter closure narrows each such parameter with `Some`.
    // ..and a function whose *result* is narrower than the slot's --
    // `_throwLocaleError`, a `String Function(String)`, as the default of a
    // `String? Function(String)` -- returns through `Some`. A static
    // tear-off (`canonicalizedLocale` in a list of fallbacks) as well as a
    // local.
    // A static function with *extra* optional named parameters as a value
    // of a narrower function type: `presentError = dumpErrorToConsole`,
    // where `dumpErrorToConsole(details, {forceReport = false})` fills a
    // `void Function(FlutterErrorDetails)` slot. The adapter passes the
    // defaults, as a call through the slot would.
    if (value is ConstantExpression &&
        value.constant is StaticTearOffConstant &&
        param is FunctionType &&
        given is FunctionType &&
        param.namedParameters.isEmpty &&
        given.namedParameters.isNotEmpty &&
        param.positionalParameters.length ==
            given.positionalParameters.length) {
      final target = (value.constant as StaticTearOffConstant).target;
      final params = <IrParam>[];
      final args = <IrExpr>[];
      for (var i = 0; i < param.positionalParameters.length; i++) {
        final name = '__a$i';
        params.add(IrParam(name, _paramType(param.positionalParameters[i])));
        args.add(IrLocal(name));
      }
      for (final n in target.function.namedParameters) {
        final init = n.initializer;
        args.add(
          init == null
              ? const IrLiteral('null', IrType('Null', nullable: true))
              : expression(init),
        );
      }
      return IrCall(
        IrClosure(
          params,
          IrReturn(IrCallValue(lowered, args)),
          _type(param.returnType),
        ),
        '!rc',
        const [],
      );
    }
    if ((value is VariableGet ||
            value is StaticTearOff ||
            (value is ConstantExpression &&
                value.constant is StaticTearOffConstant)) &&
        param is FunctionType &&
        given is FunctionType &&
        param.namedParameters.isEmpty &&
        given.namedParameters.isEmpty &&
        param.positionalParameters.length ==
            given.positionalParameters.length) {
      var adapts = false;
      final params = <IrParam>[];
      final args = <IrExpr>[];
      bool narrows(DartType g, DartType p) =>
          g is InterfaceType &&
          p is InterfaceType &&
          g.classNode == p.classNode &&
          g.nullability == Nullability.nullable &&
          p.nullability != Nullability.nullable;
      for (var i = 0; i < param.positionalParameters.length; i++) {
        final p = param.positionalParameters[i];
        final g = given.positionalParameters[i];
        final name = '__a$i';
        params.add(IrParam(name, _paramType(p)));
        if (narrows(g, p)) {
          adapts = true;
          args.add(IrSome(IrLocal(name)));
        } else {
          args.add(IrLocal(name));
        }
      }
      final widensResult = narrows(param.returnType, given.returnType);
      if (adapts || widensResult) {
        final call = IrCallValue(lowered, args);
        // Shared, as a closure argument is: the slot is an `Rc<dyn Fn>`.
        return IrCall(
          IrClosure(
            params,
            IrReturn(widensResult ? IrSome(call) : call),
            _type(param.returnType),
            locals: value is VariableGet ? _freeLocalsIn(value, {}) : const [],
          ),
          '!rc',
          const [],
        );
      }
    }
    // A `dynamic` value into a scalar or struct parameter: `number` (a
    // `num` upstream, `dynamic` after `is` checks) into `_formatExponential
    // (double)`. The downcast through `Any` is what Dart's implicit cast did.
    // A `dynamic` into a *nullable* struct slot -- `Clock? c = Zone.current
    // [#key]` -- is the downcast that may fail: `Option<T>` from `Any`.
    if (given is DynamicType &&
        param is InterfaceType &&
        param.nullability == Nullability.nullable &&
        param.classNode.name != 'Object' &&
        !_abstractLike(param.classNode) &&
        (param.classNode.enclosingLibrary.importUri.scheme != 'dart' ||
            param.classNode.enclosingLibrary.importUri.toString() ==
                'dart:ui')) {
      return IrCall(lowered, '!as_opt', [
        IrLiteral(_rustScalar(param.classNode.name), const IrType('raw')),
      ]);
    }
    // (`double` and `int` are abstract classes in dart:core, so no
    // `isAbstract` check here.)
    if (given is DynamicType &&
        param is InterfaceType &&
        param.nullability != Nullability.nullable &&
        const {
          'int',
          'double',
          'bool',
          'String',
        }.contains(param.classNode.name)) {
      return IrCall(
        IrDowncast(lowered, _rustScalar(_type(param).name)),
        'clone',
        const [],
      );
    }
    // Neither coercion for a `dart:` class other than dart:ui's: those are
    // the prelude's types, and `List`/`_GrowableList` is one `Vec`, not a
    // trait object and its struct (13 `Rc<Vec<f64>>`).
    bool translated(InterfaceType t) {
      final uri = t.classNode.enclosingLibrary.importUri;
      return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
    }

    if (param is InterfaceType &&
        given is InterfaceType &&
        translated(param) &&
        translated(given) &&
        param.nullability != Nullability.nullable &&
        given.nullability != Nullability.nullable &&
        !_abstractLike(param.classNode) &&
        // `Object` is not abstract in Kernel and is not a struct here.
        param.classNode.name != 'Object' &&
        _abstractLike(given.classNode) &&
        param.classNode != given.classNode) {
      return IrCall(
        IrDowncast(lowered, param.classNode.name),
        'clone',
        const [],
      );
    }
    // The other direction: a struct value handed to a parameter of one of
    // its abstract supertypes is shared into the `Rc<dyn ..>` that is. A
    // counted class is already a handle, and unsizes on its own.
    if (param is InterfaceType &&
        given is InterfaceType &&
        translated(param) &&
        translated(given) &&
        param.nullability != Nullability.nullable &&
        given.nullability != Nullability.nullable &&
        _abstractLike(param.classNode) &&
        !_abstractLike(given.classNode) &&
        !_closureCallsMethod(given.classNode) &&
        param.classNode != given.classNode) {
      return IrCall(lowered, '!rc', const []);
    }
    // ..and into a *nullable* slot of the supertype, through `Some`:
    // `ErrorDescription(..)` as the `DiagnosticsNode? context` of a
    // `FlutterErrorDetails` (59 in `foundation`).
    if (param is InterfaceType &&
        given is InterfaceType &&
        translated(param) &&
        translated(given) &&
        param.nullability == Nullability.nullable &&
        given.nullability != Nullability.nullable &&
        _abstractLike(param.classNode) &&
        !_abstractLike(given.classNode) &&
        !_closureCallsMethod(given.classNode) &&
        param.classNode != given.classNode) {
      return IrSome(IrCall(lowered, '!rc', const []));
    }
    // A `List<int>` handed to a `Uint8List` parameter (TFA narrowed it, or
    // Dart's typed list is a `List<int>` too): the elements are cast down,
    // `Vec<i64>` to `Vec<u8>`, as the typed-list index does.
    final narrow = _narrowElement(param);
    // The *declared* type of a variable, not its promotion: `if (input is
    // Uint8List) return input;` still holds a `Vec<i64>`.
    final held = value is VariableGet ? value.variable.type : given;
    if (narrow != null &&
        held is InterfaceType &&
        _narrowElement(held) == null &&
        (held.classNode.name == 'List' ||
            held.classNode.name == '_GrowableList' ||
            held.classNode.name == '_List')) {
      final cast = IrCall(lowered, '!narrow', [
        IrLiteral(narrow, const IrType('raw')),
      ]);
      return param!.nullability == Nullability.nullable &&
              held.nullability != Nullability.nullable
          ? IrSome(cast)
          : cast;
    }
    // An `int` into a `double`/`num` slot: `howMany = truncated` (Dart's
    // `num` is an `f64` here) -- the cast the operators take.
    String? scalar(DartType? t) => t is InterfaceType ? t.classNode.name : null;
    if (scalar(param) == 'double' &&
        scalar(given) == 'int' &&
        given!.nullability != Nullability.nullable) {
      lowered = IrCast(lowered, 'f64');
    }
    // No rule for a `num` parameter either: `int.+(num other)` is declared
    // that way, and `index + 1` became `index + (1 as f64)` (ws54, 85 in
    // dart:ui alone). `num` is not a type this output has.
    // A `List<String>` (any concrete element) into a `List<Object?>`: each
    // element shared into its `Rc<dyn Object>`.
    if (param is InterfaceType &&
        (param.classNode.name == 'List' ||
            param.classNode.name == 'Iterable') &&
        param.typeArguments.isNotEmpty &&
        param.typeArguments.first is InterfaceType &&
        (param.typeArguments.first as InterfaceType).classNode.name ==
            'Object' &&
        param.typeArguments.first.nullability == Nullability.nullable &&
        held is InterfaceType &&
        held.classNode.name == 'List' &&
        held.typeArguments.isNotEmpty &&
        held.typeArguments.first is InterfaceType &&
        (held.typeArguments.first as InterfaceType).classNode.name !=
            'Object' &&
        held.typeArguments.first.nullability != Nullability.nullable) {
      // A nullable list widens element by element under the `Option`.
      if (held.nullability == Nullability.nullable) {
        return IrNullAware(
          lowered,
          IrCall(const IrBound(), '!widen_object', const []),
        );
      }
      final widened = IrCall(lowered, '!widen_object', const []);
      return param.nullability == Nullability.nullable
          ? IrSome(widened)
          : widened;
    }
    // A typed list handed to a `List<int>` parameter widens its elements
    // (`Response.bytes(body)` with a `Uint8List`).
    if (param is InterfaceType &&
        _narrowElement(param) == null &&
        (param.classNode.name == 'List' ||
            param.classNode.name == 'Iterable') &&
        param.typeArguments.isNotEmpty &&
        param.typeArguments.first is InterfaceType &&
        (param.typeArguments.first as InterfaceType).classNode.name == 'int' &&
        held is InterfaceType &&
        _narrowElement(held) != null &&
        _narrowElement(held) != 'f32' &&
        _narrowElement(held) != 'f64') {
      final widened = IrCall(lowered, '!widen', const []);
      return param.nullability == Nullability.nullable &&
              held.nullability != Nullability.nullable
          ? IrSome(widened)
          : widened;
    }
    if (param == null || param.nullability != Nullability.nullable) {
      // A nullable value into a non-nullable parameter: Dart would not have
      // compiled it, so type flow analysis proved it non-null and rewrote
      // the check away (`alpha ?? a` became `alpha`). The unwrap is that
      // proof, spelled (7 `f64 <= Option<f64>` shapes).
      if (param is InterfaceType &&
          given is InterfaceType &&
          given.nullability == Nullability.nullable &&
          given.classNode == param.classNode) {
        return IrNullCheck(lowered);
      }
      return lowered;
    }
    // `Object?` and `dynamic` take anything: the widening there is into
    // `dyn Object`, a different coercion, and `Some(..)` around a `String`
    // handed to `StringBuffer.write(Object?)` was 57 `Display` errors.
    if (param is DynamicType ||
        (param is InterfaceType && param.classNode.name == 'Object')) {
      return lowered;
    }
    // A closure is wrapped like anything else now that a function-typed
    // parameter is `Rc<dyn Fn>` on both sides: `Option<Rc<dyn Fn(..)>>`
    // took a bare `Rc<{closure}>` 25 times in dart:ui.
    if (_isNull(value)) return lowered;
    final actual = _staticType(value);
    if (actual == null) return lowered;
    if (actual.nullability == Nullability.nullable) return lowered;
    if (actual is DynamicType || actual is NullType) return lowered;
    return IrSome(lowered);
  }

  static IrExpr _unboxed(IrExpr e) => e is IrClosure && e.boxed
      ? IrClosure(
          e.params,
          e.body,
          e.returns,
          captures: e.captures,
          locals: e.locals,
          holdsSelf: e.holdsSelf,
        )
      : e;

  FunctionNode? _calleeOf(Object param) {
    final parent = param is TreeNode ? param.parent : null;
    return parent is FunctionNode ? parent : null;
  }

  IrExpr _withBorrowing(
    Object? param,
    FunctionNode? callee,
    IrExpr Function() lower,
  ) {
    final was = _borrowedArgument;
    final kept = param != null && callee != null && _keeps(callee, param);
    if (kept) _borrowedArgument = false;
    try {
      final value = lower();
      // The parameter is owned where it is kept, so the argument is boxed to
      // match: a closure's own type has no name.
      if (value is IrClosure) {
        return IrClosure(
          value.params,
          value.body,
          value.returns,
          captures: value.captures,
          locals: value.locals,
          // Carried. Rebuilding a node without a flag it had is the shape
          // that lost `kept` in round 104 and `shared` in round 101.
          holdsSelf: value.holdsSelf,
          boxed: true,
        );
      }
      return value;
    } finally {
      _borrowedArgument = was;
    }
  }

  /// Whether the callee does anything with the parameter but call it.
  ///
  /// A body that is not there cannot be read, and "unknown" has to mean
  /// "keeps": guessing the other way is guessing that a borrow outlives its
  /// borrower.
  static final _keepsCache = <Object, bool>{};

  bool _keeps(FunctionNode callee, Object param) {
    final known = _keepsCache[param];
    if (known != null) return known;
    final body = callee.body;
    if (body == null) return _keepsCache[param] = true;
    final walk = _ParameterEscapes(param);
    body.accept(walk);
    return _keepsCache[param] = walk.escapes;
  }

  /// Whether a closure written here would land in a borrowed position.
  ///
  /// The backend emits a function-typed *parameter* as `impl Fn(..)`, so a
  /// closure passed to a call borrows and lives exactly as long as the call --
  /// which is all a closure reading `this` needs. A constructor argument is
  /// different: it is stored in the object being built, so it outlives
  /// everything here and stays refused.
  bool _borrowedArgument = false;

  /// A function's named parameters in the order its *type* lists them.
  static List<NamedParameter> _namedInTypeOrder(FunctionNode fn) =>
      [...fn.namedParameters]
        ..sort((a, b) => a.parameterName.compareTo(b.parameterName));

  /// Arguments to a function *value*, ordered by its type.
  ///
  /// Positional ones as written; then each named parameter of the type, in
  /// the type's (name) order, with what was supplied for it, or `None` when it
  /// is nullable and was left off. A function type carries no defaults, so an
  /// omitted non-nullable one has no value here and stops.
  List<IrExpr> _argumentsByType(Arguments node, FunctionType type) {
    // The function type's own parameter types widen the arguments, as a
    // callee's would: `onError(e, stack)` with `StackTrace? stackTrace`
    // takes `Some(stack)`.
    final out = [
      for (var i = 0; i < node.positional.length; i++)
        _argument(node.positional[i], null, i, type),
    ];
    final supplied = {for (final n in node.named) n.name: n.value};
    for (final param in type.namedParameters) {
      final value = supplied.remove(param.name);
      if (value != null) {
        out.add(_widened(value, param.type, expression(value)));
      } else if (param.type.nullability == Nullability.nullable) {
        out.add(const IrLiteral('null', IrType('Null', nullable: true)));
      } else {
        throw Unsupported(
          'omitted named argument `${param.name}` to a function value',
          _sample(node),
        );
      }
    }
    if (supplied.isNotEmpty) {
      throw Unsupported(
        'named argument `${supplied.keys.first}` not in the function type',
        _sample(node),
      );
    }
    return out;
  }

  List<IrExpr> _argumentList(
    Arguments node,
    FunctionNode? callee, [
    FunctionType? instantiated,
  ]) {
    final positional = [
      for (var i = 0; i < node.positional.length; i++)
        _argument(node.positional[i], callee, i, instantiated),
    ];
    if (node.named.isEmpty && callee == null) return positional;
    if (callee == null) {
      throw Unsupported(
        'named argument with no resolved callee '
        '(${node.parent.runtimeType})',
        _sample(node),
      );
    }
    final supplied = {for (final n in node.named) n.name: n.value};
    // Kernel names a named parameter through `parameterName`.
    final out = <IrExpr>[...positional];
    for (final param in callee.namedParameters) {
      // The inspector's own argument, dropped along with the parameter it
      // fills. See `_inspectorOnly`.
      if (_inspectorOnly(param.parameterName)) {
        supplied.remove(param.parameterName);
        continue;
      }
      final value = supplied.remove(param.parameterName);
      if (value != null) {
        out.add(_namedArgument(value, param));
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
      // ..and widened into the parameter like a written argument: `Curves.
      // linear` filling a `Curve` is a `_Linear` value into an `Rc<dyn
      // Curve>` (92 `_Linear`, 109 `Cubic`).
      return _intoObject(
        initializer,
        param.type,
        _widened(initializer, param.type, expression(initializer)),
      );
    }
    if (param.type.nullability == Nullability.nullable) {
      return const IrLiteral('null', IrType('Null', nullable: true));
    }
    // An *interface* member carries no default -- `Canvas.clipRect({bool
    // doAntiAlias = true})` is abstract, and the default lives on the class
    // that implements it (`_NativeCanvas`). Found there, through the
    // hierarchy: 19 refusals for `doAntiAlias` and `debugLabel`.
    final fromImplementer = _defaultFromImplementer(param);
    if (fromImplementer != null) return expression(fromImplementer);
    // A `dart:` member's `int` parameter whose default the minimal dill
    // dropped (`String.startsWith(pattern, [int index = 0])`): zero, which
    // is what every such default in the core library is.
    final owner = _calleeOf(param)?.parent;
    if (owner is Member &&
        owner.enclosingLibrary.importUri.scheme == 'dart' &&
        param.type is InterfaceType &&
        (param.type as InterfaceType).classNode.name == 'int') {
      return const IrLiteral('0', IrType('int'));
    }
    throw Unsupported(
      'omitted parameter `${param.cosmeticName}` has no default',
      _sample(site),
    );
  }

  /// The closed world's subtype relation, computed once on first use.
  late final ClassHierarchySubtypes? _subtypes = () {
    final hierarchy = typeEnvironment?.hierarchy;
    if (hierarchy is! ClosedWorldClassHierarchy) return null;
    return hierarchy.computeSubtypesInformation();
  }();

  /// The default an implementing class gives an interface member's
  /// parameter, when the interface itself gives none.
  Expression? _defaultFromImplementer(FunctionParameter param) {
    final callee = _calleeOf(param);
    final member = callee?.parent;
    if (member is! Procedure || member.enclosingClass == null) return null;
    final subtypes = _subtypes;
    if (subtypes == null) return null;
    final name = param.cosmeticName ?? param.parameterName;
    for (final sub in subtypes.getSubtypesOf(member.enclosingClass!)) {
      for (final p in sub.procedures) {
        if (p.name.text != member.name.text || p.isStatic) continue;
        for (final candidate in [
          ...p.function.positionalParameters,
          ...p.function.namedParameters,
        ]) {
          final candidateName =
              candidate.cosmeticName ?? candidate.parameterName;
          if (candidateName == name && candidate.defaultValue != null) {
            return candidate.defaultValue;
          }
        }
      }
    }
    return null;
  }

  /// `MaterialLocalizations` written where a value goes: Dart's `Type`.
  ///
  /// The prelude has had `Type::of(name)` all along -- a name, because that is
  /// what upstream does with one: compares it, prints it, uses it as a map
  /// key. Not having this refused `Theme.of`, and `Theme.of` is called 268
  /// times. Four `of` methods -- Theme, MaterialLocalizations,
  /// CupertinoLocalizations and the gallery's own -- account for 464 of the
  /// 670 "called something that was not translated".
  static IrExpr _typeLiteral(DartType type) {
    final name = type is InterfaceType ? type.classNode.name : '$type';
    return IrLiteral('Type::of("$name")', const IrType('raw'));
  }

  /// The static type of a constant, for the widening a literal's entry
  /// gets: an instance is its class, a literal its `dart:core` class.
  DartType _constantStaticType(Constant c) {
    final core = typeEnvironment?.coreTypes;
    return switch (c) {
      InstanceConstant() => InterfaceType(
        c.classNode,
        Nullability.nonNullable,
        c.typeArguments,
      ),
      IntConstant() when core != null => core.intNonNullableRawType,
      DoubleConstant() when core != null => core.doubleNonNullableRawType,
      BoolConstant() when core != null => core.boolNonNullableRawType,
      StringConstant() when core != null => core.stringNonNullableRawType,
      NullConstant() => const NullType(),
      _ => const DynamicType(),
    };
  }

  IrExpr _constant(Constant constant, Expression node) {
    if (constant is TypeLiteralConstant) return _typeLiteral(constant.type);
    if (constant is SymbolConstant) {
      // `#name`, spelled the way `Type::of` is: a name and nothing else. The
      // library a private symbol belongs to is dropped -- see the prelude's
      // `Symbol` for what that costs, which in this program is nothing.
      return IrLiteral('Symbol::of("${constant.name}")', const IrType('raw'));
    }
    if (constant is DoubleConstant) {
      // `double.infinity` prints as `Infinity`, which the literal emitter then
      // suffixed into `Infinity.0` -- a name nothing declares, 183 times.
      // Rust spells these three, and only these three, differently.
      final value = constant.value;
      // `f64`, because Dart's `double` is one. These three said `f32` since
      // before round 96 changed the mapping, and nothing caught it: they only
      // appear where an infinity is written down, and every one of those sites
      // was already inside something that did not compile.
      if (value.isNaN) return const IrLiteral('f64::NAN', IrType('raw'));
      if (value == double.infinity) {
        return const IrLiteral('f64::INFINITY', IrType('raw'));
      }
      if (value == double.negativeInfinity) {
        return const IrLiteral('f64::NEG_INFINITY', IrType('raw'));
      }
      return IrLiteral('$value', const IrType('double'));
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
    if (constant is SetConstant) {
      // A const set: the prelude's `Set::from(vec![..])`, which is what a
      // set literal expression becomes too. 37 in the gallery's dill.
      return IrStaticCall('Set', 'from', [
        IrListLiteral([
          for (final e in constant.entries) _constant(e, node),
        ], _type(constant.typeArgument)),
      ]);
    }
    if (constant is MapConstant) {
      // Each entry into the map's own types, as a map literal's are: the
      // `const <ShortcutActivator, Intent>{SingleActivator(..): ..}` tables
      // of `DefaultTextEditingShortcuts` put a `SingleActivator` where an
      // `Rc<dyn ShortcutActivator>` goes (74 "arguments incorrect" and 12
      // mismatched types on five statics in `widgets`).
      IrExpr entry(Constant c, DartType into) {
        final value = ConstantExpression(c, _constantStaticType(c));
        return _intoObject(
          value,
          into,
          _widened(value, into, _constant(c, node)),
        );
      }

      return IrMapLiteral(
        [
          for (final e in constant.entries)
            (
              entry(e.key, constant.keyType),
              entry(e.value, constant.valueType),
            ),
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
      // `Zone.root` (the `_RootZone` constant): the prelude's `Zone::root()`.
      // Fields initialised with it -- every `_onXZone` in
      // `PlatformDispatcher` -- kept its constructor refused, and with it
      // the static `instance` every hook goes through.
      final constClass = constant.classNode;
      if (constClass.enclosingLibrary.importUri.toString() == 'dart:async' &&
          (constClass.name == '_RootZone' || constClass.name == 'Zone')) {
        return IrStaticCall('Zone', 'root', const []);
      }
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
      if (rebuilt != null) {
        return _isOpen(cls)
            ? IrUpcast(
                rebuilt,
                IrType(
                  cls.name,
                  arguments: _erasedArguments(cls, constant.typeArguments),
                ),
              )
            : rebuilt;
      }
      final instance = IrConstInstance(IrType(_instanceName(cls)), {
        for (final entry in byName.entries)
          entry.key: _constant(entry.value, node),
      });
      return _isOpen(cls) ? IrUpcast(instance, IrType(cls.name)) : instance;
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
    // Each constant into its parameter's type: `const _ModifierSidePair(
    // ModifierKey.altModifier, KeyboardSide.left)` against a `KeyboardSide?
    // side` is `Some(..)` (20 in `RawKeyboard`'s modifier map).
    final paramType = <String, DartType>{
      for (final p in function.positionalParameters) _paramName(p): p.type,
      for (final p in function.namedParameters) p.parameterName: p.type,
    };
    for (final name in names) {
      final value = byName[name];
      if (value == null) return null;
      var lowered = _constant(value, node);
      final t = paramType[name];
      // A concrete constant into an abstract parameter is shared, as a
      // written argument would be: `Curves.linear` filling `Interval`'s
      // `curve` is a `_Linear` value where an `Rc<dyn Curve>` goes (39).
      if (t is InterfaceType &&
          value is InstanceConstant &&
          _abstractLike(t.classNode) &&
          !_abstractLike(value.classNode) &&
          t.classNode != value.classNode &&
          _translatedClass(t.classNode) &&
          _translatedClass(value.classNode) &&
          !_closureCallsMethod(value.classNode)) {
        lowered = IrCall(lowered, '!rc', const []);
      }
      final wraps =
          t != null &&
          t is! DynamicType &&
          t.nullability == Nullability.nullable &&
          !(t is InterfaceType && t.classNode.name == 'Object') &&
          value is! NullConstant;
      args.add(wraps ? IrSome(lowered) : lowered);
    }
    return IrNew(_constantType(cls, typeArguments), args);
  }

  /// The type of a rebuilt constant, type arguments and all.
  ///
  /// Dropped, `const Pair<int, double>(3, 4.5)` came out as `Pair::new(..)`
  /// against the analyzer front end's `Pair::<i64, f32>::new(..)`. Both are
  /// valid Rust -- inference would have got there -- but the two front ends
  /// saying different things is the one thing the fixtures exist to catch.
  IrType _constantType(Class cls, List<DartType> typeArguments) => IrType(
    _instanceName(cls),
    arguments: _erasedArguments(cls, typeArguments),
  );

  /// Whether a class's type parameter is erased to its bound.
  ///
  /// Rust's trait parameters are invariant: `impl State<Scaffold> for
  /// ScaffoldState` is no `State<StatefulWidget>`, and `createState` has to
  /// return one (377 stubs naming `State<..>` at ws279). Dart's `State<T
  /// extends StatefulWidget>` only ever *narrows* `T` in subclasses, so in
  /// a closed world the parameter can go: the trait is `State`, `T` inside
  /// it is `StatefulWidget`, and a read typed narrower than that is a
  /// downcast (`_narrowedRead`). A parameter is erased when its bound is a
  /// translated abstract-like class -- `Action<T extends Intent>`,
  /// `ParentDataWidget<T extends ParentData>`, `GlobalKey<T extends
  /// State>` -- and not when the bound is `Object` or a scalar, where the
  /// parameter is a real type variable (`Tween<T>`, `Animation<T>`).
  bool _erasedParameter(TypeParameter p) {
    if (!erase) return false;
    // A factory carries its own copies of the class's parameters: erased
    // with them, or `global_key_new<T>` kept a `T` nothing could infer (87).
    final decl = p.declaration;
    if (decl is Procedure && decl.isFactory) {
      final cls = decl.enclosingClass;
      final i = decl.function.typeParameters.indexOf(p);
      return cls != null &&
          i >= 0 &&
          i < cls.typeParameters.length &&
          _erasedParameter(cls.typeParameters[i]);
    }
    if (decl is! Class) return false;
    final bound = p.bound;
    return bound is InterfaceType &&
        bound.classNode.name != 'Object' &&
        bound.classNode.enclosingLibrary.importUri.scheme != 'dart' &&
        _abstractLike(bound.classNode);
  }

  /// A class's type arguments with the erased ones left out.
  List<IrType> _erasedArguments(Class cls, List<DartType> arguments) => [
    for (var i = 0; i < arguments.length; i++)
      if (i >= cls.typeParameters.length ||
          !_erasedParameter(cls.typeParameters[i]))
        _type(arguments[i]),
  ];

  /// A read whose declared type is an erased parameter and whose static
  /// type is a concrete class below the bound: `widget` in
  /// `_ScaffoldState` is a `Scaffold`, and the accessor hands out the
  /// `Rc<dyn StatefulWidget>` the erased trait declares.
  IrExpr _narrowedRead(InstanceGet node, IrExpr lowered) {
    final declared = node.interfaceTarget.getterType;
    if (declared is! TypeParameterType ||
        !_erasedParameter(declared.parameter)) {
      return lowered;
    }
    // ..or the reader's own erased parameter (`T` of
    // `ImplicitlyAnimatedWidgetState<T>` reading `State<T>.widget`): its
    // bound is what the read is typed as (80 reads on the erased handle).
    var result = node.resultType;
    if (result is TypeParameterType && _erasedParameter(result.parameter)) {
      result = result.parameter.bound;
    }
    final bound = declared.parameter.bound;
    if (result is! InterfaceType ||
        bound is! InterfaceType ||
        result.classNode == bound.classNode ||
        result.nullability == Nullability.nullable) {
      return lowered;
    }
    // ..to a trait, when the narrower class is open or abstract.
    if (_abstractLike(result.classNode)) {
      return IrCastTo(lowered, _type(result));
    }
    return _narrowingCast(lowered, result);
  }

  /// The downcast of `lowered` to the concrete class `to` names. A counted
  /// class comes back as its handle (`clone` on a downcast, ws279); a value
  /// class as a copy -- except a *generic* one, whose derived `Clone` wants
  /// `T: Clone` the impl never promised (ws281): that one is read as a
  /// reference, and every field read through a downcast clones the field.
  IrExpr _narrowingCast(IrExpr lowered, InterfaceType to) {
    final target = to.classNode;
    final cast = IrDowncast(
      lowered,
      _rustScalar(target.name),
      arguments: _erasedArguments(target, to.typeArguments),
    );
    if (!_closureCallsMethod(target) && target.typeParameters.isNotEmpty) {
      return cast;
    }
    return IrCall(cast, 'clone', const []);
  }

  /// A generic method called on a trait handle: see `IrSuperDispatch`.
  /// Only when the closed world holds exactly one body for it, in a class
  /// that is a trait here; a second body (`OptionalMethodChannel.
  /// invokeMethod`) or a body on a struct is left for a later round.
  IrExpr? _genericOnTrait(InstanceInvocation node, List<IrExpr> args) {
    final target = node.interfaceTarget;
    if (target is! Procedure ||
        target.kind != ProcedureKind.Method ||
        target.function.typeParameters.isEmpty) {
      return null;
    }
    final declaring = target.enclosingClass;
    if (declaring == null || !_abstractLike(declaring)) return null;
    final receiver = node.receiver;
    final Class? from;
    if (receiver is ThisExpression) {
      from = declaring;
    } else {
      final t = _staticType(receiver);
      from = t is InterfaceType ? t.classNode : null;
      if (from == null || !_abstractLike(from)) return null;
    }
    final bodies = _genericBodies(target);
    if (bodies.length != 1) return null;
    final body = bodies.single;
    if (!_abstractLike(body)) return null;
    final hierarchy = typeEnvironment?.hierarchy;
    final below =
        from == body || (hierarchy?.isSubInterfaceOf(from, body) ?? false);
    return IrSuperDispatch(
      receiver is ThisExpression ? const IrThis() : expression(receiver),
      body.name,
      target.name.text,
      args,
      [for (final t in node.arguments.types) _type(t)],
      body.typeParameters.where((p) => !_erasedParameter(p)).length,
      castTo: below ? null : body.name,
    );
  }

  /// The classes below (and including) the target's that carry a body for
  /// its name, whole program.
  List<Class> _genericBodies(Procedure target) {
    final subtypes = _subtypes;
    if (subtypes == null) return const [];
    final declaring = target.enclosingClass!;
    final out = <Class>[];
    for (final c in [declaring, ...subtypes.getSubtypesOf(declaring)]) {
      if (c.isAnonymousMixin || out.contains(c)) continue;
      final uri = c.enclosingLibrary.importUri.toString();
      if (!uri.startsWith('package:') && uri != 'dart:ui') continue;
      for (final p in c.procedures) {
        if (p.name.text == target.name.text &&
            !p.isStatic &&
            !p.isAbstract &&
            p.kind == ProcedureKind.Method &&
            p.function.body != null) {
          out.add(c);
          break;
        }
      }
    }
    return out;
  }

  /// Whether a type names an erased parameter anywhere in it.
  bool _mentionsErased(DartType t) => switch (t) {
    TypeParameterType() => _erasedParameter(t.parameter),
    InterfaceType() => t.typeArguments.any(_mentionsErased),
    FunctionType() =>
      _mentionsErased(t.returnType) ||
          t.positionalParameters.any(_mentionsErased) ||
          t.namedParameters.any((n) => _mentionsErased(n.type)),
    _ => false,
  };

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
      // Any other `return e;` in a `void` body -- `(x) => day = x` handed
      // to a `void Function(int)` -- runs `e` and returns nothing.
      if (_voidReturn && value != null) {
        return IrBlock([IrExprStmt(expression(value)), const IrReturn(null)]);
      }
      if (value == null) return const IrReturn(null);
      // `return completer.future;` in an `async` body: Dart awaits the
      // future it returns, and an `async fn` returning `T` has to as well.
      final valueType = _staticType(value);
      if (_asyncBody &&
          valueType is InterfaceType &&
          valueType.classNode.name == 'Future') {
        return IrReturn(IrAwait(expression(value)));
      }
      return IrReturn(
        _intoObject(
          value,
          _returnsType,
          _widened(value, _returnsType, expression(value)),
        ),
      );
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
    if (node is ExpressionStatement && node.expression is Rethrow) {
      // `rethrow`: the handler's own error, thrown again. `Result` has no
      // notion of "the current exception", so the name the handler bound is
      // what goes back out. `loadFontIfNecessary` -- and through it every
      // `google_fonts_text_style` call, 1709 of them -- stopped here.
      final caught = _caught;
      if (caught == null) {
        throw Unsupported('rethrow outside a catch', _sample(node));
      }
      return IrThrow(IrLocal(caught));
    }
    if (node is ExpressionStatement && node.expression is Throw) {
      // Before the general `ExpressionStatement` case below, not after: the
      // general one lowers the expression, and a `throw` has no value to lower.
      // Placed after, this check never ran and every throwing method was
      // refused -- which the fixture comparison found at once, because the
      // analyzer front end had it right.
      final thrown = node.expression as Throw;
      if (_tfaUnreachable(thrown)) return IrExprStmt(_unreachable);
      return IrThrow(expression(thrown.expression));
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
            _arguments(
              value.arguments,
              value.interfaceTarget.function,
              true,
              value.functionType,
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
        return IrIndexSet(
          expression(value.receiver),
          expression(value.arguments.positional[0]),
          _intoObject(
            stored,
            element,
            _widened(stored, element, expression(stored)),
          ),
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
          _intoObject(
            value.value,
            value.variable.type,
            _intoDeclaredNum(
              value.value,
              value.variable.type,
              _widened(
                value.value,
                value.variable.type,
                expression(value.value),
              ),
            ),
          ),
        );
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
      // `T effectiveValue<T>(..)` inside `ButtonStyleButton.build`: a local
      // function with type parameters of its own. A Rust closure cannot be
      // generic, and a nested `fn` cannot see the enclosing locals this one
      // reads. Emitted as a closure it named a `T` nothing declared -- 36
      // rustc errors that were really this one refusal.
      if (node.function.typeParameters.isNotEmpty) {
        throw Unsupported('generic local function', _sample(node));
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
        // A case value widens into the scrutinee's type: `switch (tileMode)`
        // over a `TileMode?` compares an `Option` with `Some(TileMode::Clamp)`.
        final scrutinee = _staticType(node.expression);
        cases.add(
          IrCase([
            for (final e in c.expressions)
              _widened(e, scrutinee, expression(e)),
          ], body),
        );
      }
      // A switch the language checked as exhaustive -- every `TileMode` and
      // `null` -- has no `default`, and Rust's `if` chain made of it has no
      // `else`: the chain's value is `()`, and a getter returning through it
      // does not type. The last case is what is left when none of the
      // others matched, so it is the `else`.
      if (otherwise == null &&
          node.isExplicitlyExhaustive &&
          cases.isNotEmpty) {
        otherwise = cases.removeLast().body;
      }
      return IrSwitch(expression(node.expression), cases, otherwise);
    }
    if (node is WhileStatement) {
      final restored = _forInWhile(node);
      if (restored != null) return restored;
      // No updates, so a `continue` really is Rust's `continue`.
      return IrWhile(expression(node.condition), _loopBody(node.body, false));
    }
    if (node is DoStatement) {
      // `do { .. } while (c)`: a `loop` whose body runs first and tests
      // last. The body is labelled as a `for` with updates is, so that a
      // `continue` inside it leaves the body block and still reaches the
      // test -- a bare `continue` would have skipped it. `package:characters`
      // is written with these, and its whole `StringCharacters` was refused.
      return IrWhile(
        const IrLiteral('true', IrType('bool')),
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
    // A read stack trace is bound to `StackTrace::current()` at the catch
    // (the backend does it): the *catch site's* stack, not the throw's --
    // a `Result` carries none. Recorded as the approximation it is; 38
    // members were refused for reading one, most of them to log it.

    final guard = clause.guard;
    final outerCaught = _caught;
    _caught = error;
    final IrStmt handler;
    try {
      handler = statement(clause.body);
    } finally {
      _caught = outerCaught;
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

  /// A function's return type, which is the one place `Never` is allowed.
  ///
  /// Dart's `Never` is Rust's `!`, and stable Rust accepts `!` as a function's
  /// return type and nowhere else: as a type argument it is "experimental",
  /// and the first round that mapped it everywhere got 22 of those from rustc.
  /// `noSuchMethod` declared `Never` is what this is for; a `Never` anywhere
  /// else still refuses, through `_type`.
  IrType _returnType(FunctionNode function) => function.returnType is NeverType
      ? const IrType('Never')
      : _type(function.returnType);

  /// The error the enclosing `catch` bound, for a `rethrow` to name.
  String? _caught;

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
    final expected = _expectedReturn;
    _expectedReturn = null;
    _voidReturn = (expected ?? function.returnType) is VoidType;
    _returnsType = expected ?? function.returnType;
    _asyncBody = function.asyncMarker == AsyncMarker.Async;
    try {
      return statement(body);
    } finally {
      _voidReturn = outer;
      _returnsType = outerType;
      _asyncBody = outerAsync;
    }
  }

  /// Whether the body being lowered is an `async` one.
  bool _asyncBody = false;

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

  static const _unreachable = IrLiteral(
    'unreachable!("removed by the AOT compiler (TFA)")',
    IrType('raw'),
  );

  /// An `int` value stored into a variable *declared* `num` (an `f64`).
  IrExpr _intoDeclaredNum(Expression value, DartType declared, IrExpr lowered) {
    if (declared is! InterfaceType || declared.classNode.name != 'num')
      return lowered;
    // A literal says so itself: `num _n = 0` at the top level has no
    // context for `getStaticType` and came out as `RefCell<f64>::new(0)`.
    if (value is IntLiteral) return IrCast(lowered, 'f64');
    final given = _staticType(value);
    if (given is InterfaceType &&
        given.classNode.name == 'int' &&
        given.nullability != Nullability.nullable) {
      return IrCast(lowered, 'f64');
    }
    // A `num` whose static type is `num` -- TFA folded `1 is double ?
    // pow(2, 52) : 1.0e300.floor()` to its `int` branch and the
    // conditional's type stayed `num` -- is cast too: `f64 as f64` is a
    // no-op Rust accepts, and `i64 as f64` is the cast that was missing.
    if (given is InterfaceType &&
        given.classNode.name == 'num' &&
        given.nullability != Nullability.nullable &&
        value is! DoubleLiteral) {
      return IrCast(lowered, 'f64');
    }
    return lowered;
  }

  /// The operators a `dynamic` receiver is downcast to `f64` for (see
  /// `expression`): their result is an `f64` whatever the static type says,
  /// and a `dynamic` slot taking it needs the sharing an `f64` gets.
  static const _dynamicNumOperators = {'+', '-', '*', '/', '%', '~/'};

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

  /// A downcast names a Rust type: the core scalars by their Rust names.
  static String _rustScalar(String name) =>
      const {
        'num': 'f64',
        'double': 'f64',
        'int': 'i64',
        'bool': 'bool',
        'String': 'String',
      }[name] ??
      name;

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

  /// The declared return type of the function being lowered, for `return`
  /// to widen into when it is nullable and the value is not.
  DartType? _returnsType;

  // -- Declarations -----------------------------------------------------------

  (IrLibrary, List<String>) lowerLibrary() {
    final classes = <IrClass>[];
    final constants = <IrConstDecl>[];
    final refused = <String>[];
    for (final field in library.fields) {
      var init = field.initializer;
      // `int? _implicitViewId;` -- a top-level variable with no initialiser
      // starts out null, and readers were left naming a static that was
      // never declared. A non-nullable one without an initialiser is `late`
      // and stays refused here.
      if (init == null && field.type.nullability == Nullability.nullable) {
        init = NullLiteral();
      }
      if (init == null) continue;
      // A mutable top-level variable is a `static` too -- Dart's is one per
      // isolate, which is what `Isolate` says -- and it needs a cell to be
      // assignable. Skipping them meant every read refused the member around
      // it.
      final mutable = !field.isConst && !field.isFinal;
      try {
        constants.add(
          IrConstDecl(
            field.name.text,
            _type(field.type),
            // Into the declared type: a `dynamic` top-level holding a
            // struct is an `Rc<dyn Object>` (intl's locale data).
            _intoObject(
              init,
              field.type,
              _intoDeclaredNum(
                init,
                field.type,
                _widened(init, field.type, expression(init)),
              ),
            ),
            isLazy: mutable,
            isMutable: mutable,
          ),
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
      // An *extension type*'s members have no representation here yet. A
      // plain extension's do: the CFE has already lowered `extension X on T {
      // get hinge => .. }` to a top-level function taking the receiver, and
      // the name it gave it (`MediaQueryHinge|get#hinge`) cleans to an
      // identifier the same way at the declaration and at every call.
      if (procedure.name.text.contains('|') &&
          procedure.isExtensionTypeMember) {
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
        final stub = _stubFor(procedure, '$error');
        if (stub != null) functions.add(stub);
      }
    }
    for (final cls in library.classes) {
      // Anonymous mixin applications stay skipped: they are the CFE's own
      // synthetic classes, not something upstream wrote. Private classes do
      // not -- see the note where `_refusePrivate` used to be.
      if (cls.isAnonymousMixin) continue;
      // `lowerClass` guards each *member*, and its own header is not a member:
      // the superclass's type arguments and the mixin list are lowered before
      // any member is, and a refusal there had nowhere to go but out of the
      // whole run. One extension type in `widgets` stopped the package
      // emitting at all. A class is the unit here, so it is where the refusal
      // stops.
      try {
        final (lowered, problems) = lowerClass(cls);
        classes.add(lowered);
        if (_isOpen(cls)) classes.add(_implOf(cls, lowered));
        refused.addAll(problems.map((p) => '${cls.name}: $p'));
      } on Unsupported catch (error) {
        refused.add('${cls.name}: $error');
      }
    }
    return (
      IrLibrary(
        classes,
        constants: constants,
        functions: functions,
        abstractElsewhere: abstractElsewhere,
        elsewhere: elsewhere,
      ),
      refused,
    );
  }

  /// A top-level function, as a method with no receiver.
  /// `set defaultLocale(..)` as a function name: `setDefaultLocale`, which
  /// `snake` spells `set_default_locale` at the declaration and every store.
  static String _topLevelSetterName(String name) {
    final clean = name.replaceAll(RegExp(r'[|#]'), '_');
    return 'set${clean[0].toUpperCase()}${clean.substring(1)}';
  }

  /// `dart:async`'s `StreamView`, the one prelude base a class extends.
  static bool _isStreamView(Class c) =>
      c.name == 'StreamView' &&
      c.enclosingLibrary.importUri.toString() == 'dart:async';

  /// A refused method or function, kept as its signature over a body that
  /// says so at runtime: `panic!("dart2rust: not translated: <why>")`.
  ///
  /// Without this a refusal took the member out of the output, and every
  /// reference to it -- `debugPrint = debugPrintThrottled` -- failed to
  /// compile, taking its crate and every crate above it out of the build.
  /// The refusal is still recorded (the `NOT TRANSLATED` line, the count);
  /// what changes is that the program links and fails where Dart would have
  /// run the missing member, not everywhere. A member whose *signature*
  /// cannot be spelled is still left out.
  IrMethod? _stubFor(Procedure node, String reason) {
    if (node.kind == ProcedureKind.Factory ||
        node.isNoSuchMethodForwarder ||
        node.isAbstract ||
        node.isExternal) {
      return null;
    }
    try {
      final fn = node.function;
      final isTopLevel = node.enclosingClass == null;
      // `Iterator<T>` is a Rust trait, not a type: a signature naming it
      // cannot be spelled here (`ObserverList.iterator`).
      bool unspeakable(DartType t) =>
          t is InterfaceType &&
          (t.classNode.name == 'Iterator' ||
              t.classNode.name == 'Iterable' && false);
      if (unspeakable(fn.returnType) ||
          fn.positionalParameters.any((p) => unspeakable(p.type)) ||
          fn.namedParameters.any((p) => unspeakable(p.type))) {
        return null;
      }
      final name = node.kind == ProcedureKind.Setter && isTopLevel
          ? _topLevelSetterName(node.name.text)
          : node.name.text.replaceAll(RegExp(r'[|#]'), '_');
      // Into a Rust string literal that is also a format string: braces
      // doubled, quotes and backslashes escaped, one line.
      final text = reason
          .replaceAll('\\', '\\\\')
          .replaceAll('"', '\\"')
          .replaceAll('{', '{{')
          .replaceAll('}', '}}')
          .replaceAll('\n', ' ');
      return IrMethod(
        name,
        [
          for (final p in fn.positionalParameters)
            IrParam(
              _paramName(p),
              _type(p.type),
              hasDefault: p.defaultValue != null,
              defaultValue: _default(p),
            ),
          for (final p in fn.namedParameters)
            // With their defaults: the impl forwarder for an override that
            // adds `gapExtent = 0` to `ShapeBorder.paint` passes the default,
            // and a stub without it was "a value the base has no value for"
            // (`CutCornersBorder`, the one error left in the gallery).
            IrParam(
              p.parameterName,
              _type(p.type),
              named: true,
              hasDefault: p.defaultValue != null,
              defaultValue: _default(p),
            ),
        ],
        _returnType(fn),
        IrBlock([
          IrExprStmt(
            IrLiteral(
              'panic!("dart2rust: not translated: $text")',
              const IrType('raw'),
            ),
          ),
        ]),
        typeParameters: [
          for (final p in fn.typeParameters)
            if (!_erasedParameter(p)) p.name ?? 'T',
        ],
        isStatic: node.isStatic && !isTopLevel,
        isGetter: node.kind == ProcedureKind.Getter && !(isTopLevel),
        isSetter: node.kind == ProcedureKind.Setter && !isTopLevel,
        isAsync: fn.asyncMarker == AsyncMarker.Async,
        operator: node.kind == ProcedureKind.Operator ? name : null,
      );
    } on Unsupported {
      return null;
    }
  }

  IrMethod _lowerTopLevel(Procedure node) {
    _enter(node);
    // A top-level **getter** is a function of no arguments, which is what it
    // becomes here. Dart writes `PluralCase get ONE => ..` and reads it as a
    // name; Rust writes `fn one() -> PluralCase` and reads it as a call, and
    // the difference is only in the spelling of the read.
    //
    // A setter is not the same shape -- it is an assignment that has to look
    // like one at every use -- and stays refused.
    // A top-level **setter** is a function of one argument named for the
    // store: `set defaultLocale(v)` is `fn set_default_locale(v)`, and
    // `defaultLocale = x` at every site is a call of it (see `StaticSet`).
    if (node.kind != ProcedureKind.Method &&
        node.kind != ProcedureKind.Getter &&
        node.kind != ProcedureKind.Setter) {
      throw Unsupported('a top-level ${node.kind.name}', node.name.text);
    }
    // An extension member's CFE name -- `StringCharacters|get#characters` --
    // read as an operator by the backend's `_identifier`, which then found no
    // Rust name for it. Cleaned here the way `snake` cleans it at every call
    // site, so the declaration and the calls spell one identifier.
    final name = node.kind == ProcedureKind.Setter
        ? _topLevelSetterName(node.name.text)
        : node.name.text.replaceAll(RegExp(r'[|#]'), '_');
    return IrMethod(
      name,
      [
        for (final p in node.function.positionalParameters)
          IrParam(_paramName(p), _type(p.type), kept: _keeps(node.function, p)),
        for (final p in node.function.namedParameters)
          if (!_inspectorOnly(p.parameterName))
            IrParam(
              p.parameterName,
              _type(p.type),
              named: true,
              kept: _keeps(node.function, p),
            ),
      ],
      _type(node.function.returnType),
      _body(node.function),
      typeParameters: [
        for (final p in node.function.typeParameters)
          if (!_erasedParameter(p)) p.name ?? 'T',
      ],
      isStatic: true,
      // A top-level function is `async` the same way a method is. Round 71
      // marked the methods and left these, so `await` came out inside a
      // function that was not one.
      isAsync: node.function.asyncMarker == AsyncMarker.Async,
    );
  }

  /// The class's mutable fields that some closure in it touches.
  ///
  /// Collected before anything is lowered, because the field's *declaration*
  /// has to know: its type, its reads, its writes, its initialiser and the
  /// closure's capture all have to agree, and they are written in that order.
  Set<String> _sharedFields = const {};

  /// Whether some closure in the class calls a method on `this`.
  ///
  /// Generous, like `_closureFields`: a class counted that need not be costs
  /// an `Rc`; one not counted that should be is a closure that cannot exist.
  /// Whether `from`'s instance fields reach `target` by value within a few
  /// steps (a field of a class type, or a type argument of one).
  bool _reachesItself(Class target, Class from, Set<Class> seen, int depth) {
    if (depth > 4 || !seen.add(from)) return false;
    for (final field in from.fields) {
      if (field.isStatic) continue;
      if (_mentions(field.type, target)) return true;
      final held = field.type;
      if (held is! InterfaceType) continue;
      final next = held.classNode;
      if (next != target &&
          next.enclosingLibrary.importUri.scheme != 'dart' &&
          _reachesItself(target, next, seen, depth + 1)) {
        return true;
      }
      for (final arg in held.typeArguments) {
        if (arg is InterfaceType &&
            arg.classNode != target &&
            arg.classNode.enclosingLibrary.importUri.scheme != 'dart' &&
            _reachesItself(target, arg.classNode, seen, depth + 1)) {
          return true;
        }
      }
    }
    return false;
  }

  bool _closureCallsMethod(Class node) {
    // A tear-off of `this.method` is that closure written shorter (see the
    // `InstanceTearOff` case), so it makes the class counted for the same
    // reason a closure calling a method does. 448 refusals were tear-offs in
    // classes that had no such closure -- `onPressed: _submit`, and every
    // `addListener(_handleChange)` -- and the handle they keep needs an `Rc`
    // to be kept in.
    final tearOffs = _TearOffFinder();
    node.accept(tearOffs);
    if (tearOffs.onThis) return true;
    // A class reached through an interface is reached through an
    // `Rc<dyn Trait>`, and a trait method on a shared handle cannot be
    // `&mut self`: `child.addListener(..)` on an `Rc<dyn Listenable>` was
    // E0596 however the trait was declared. So a class that implements an
    // interface (or extends an abstract class, or is a mixin) *and* writes
    // a field in a method is counted: its fields are cells, its methods
    // `&self`, and the mutation happens through the cell -- which is what
    // sharing an object that changes means.
    final reachedThroughTrait =
        node.implementedTypes.isNotEmpty ||
        node.isMixinDeclaration ||
        (node.superclass != null && _abstractLike(node.superclass!));
    if (reachedThroughTrait && _writesFieldInMethod(node)) return true;
    // ..or mixes in / extends an abstract class with a mutable field: that
    // class's own methods write it through the trait's setter, on `&self`,
    // so the storage here has to be a cell (`_TypedDataBuffer._length`).
    if (_inheritsMutableTraitField(node)) return true;
    // A class holding its own type in a field -- `FocusNode`'s children,
    // `_NotificationNode`'s parent -- cannot be a Rust value: the struct
    // would have infinite size (7 `E0072`s). A handle is the only shape it
    // has, so the class is counted. Direct self-reference only; a cycle
    // through another class (`OverlayEntry` <-> `_OverlayEntryWidget`) is
    // not seen from here yet.
    // ..and a cycle of any short length through value classes:
    // `_NativeCanvas` holds a `_NativePictureRecorder` holding a
    // `_NativeCanvas` (E0072); `TextPainter` holds a layout cache holding a
    // `_TextLayout` holding a `TextPainter` -- a struct of infinite size
    // unless it is a handle.
    if (_reachesItself(node, node, {}, 0)) return true;
    // ..and a class that is disposed has an identity to dispose.
    if (node.procedures.any((p) => p.name.text == 'dispose' && !p.isStatic)) {
      return true;
    }
    final closures = <FunctionNode>[];
    node.accept(_ClosureFinder(closures));
    for (final fn in closures) {
      // The same two questions `_closure` asks before it refuses -- does the
      // closure reach `this`, and would copying its final fields do instead.
      // Asked with the same detectors: `_ThisUse` answered "no" for 65
      // closures that `_ThisFinder` then found reaching `this`, because the
      // former does not look inside a field read's receiver.
      if (_reachesThis(fn) && _finalFieldsRead(fn) == null) return true;
    }
    return false;
  }

  /// Whether the class being lowered is reference counted.
  bool _counted = false;

  Set<String> _closureFields(Class node) {
    final closures = <FunctionNode>[];
    node.accept(_ClosureFinder(closures));
    final touched = <String>{};
    for (final fn in closures) {
      final walk = _FieldsTouched();
      fn.accept(walk);
      touched.addAll(walk.mutable);
    }
    return touched;
  }

  /// Whether a static const field of an enum is one of its variants.
  ///
  /// The variants are the fields typed as the enum. Anything else declared
  /// `static const` inside it is an ordinary constant that happens to live
  /// there, and counting it as a variant emits a name no value ever had.
  static bool _isVariantOf(Class node, Field field) {
    final type = field.type;
    return type is InterfaceType && type.classNode == node;
  }

  /// The struct beside an open class's trait: it extends the class (whose
  /// fields flatten into it and whose methods reach it as a subclass's
  /// do) and adds nothing but constructors forwarding to the base's.
  IrClass _implOf(Class node, IrClass lowered) {
    final impl = IrClass(
      implName(node.name),
      typeParameters: lowered.typeParameters,
      superclass: lowered.name,
      superclassArguments: [for (final p in lowered.typeParameters) IrType(p)],
      counted: lowered.counted,
      doc: 'The instances of `${node.name}` itself; see the trait.',
    );
    for (final ctor in lowered.constructors) {
      impl.constructors.add(
        IrConstructor(
          ctor.params,
          const {},
          isConst: ctor.isConst,
          name: ctor.name,
          superBase: lowered.name,
          superName: ctor.name,
          superArgs: [for (final p in ctor.params) IrLocal(p.name)],
        ),
      );
    }
    return impl;
  }

  (IrClass, List<String>) lowerClass(Class node) {
    _lowering = node;
    _sharedFields = _closureFields(node);
    _counted = _closureCallsMethod(node);

    // Kernel's superclass may be a synthetic mixin application; the class a
    // reader would name is the first one above that is not.
    // The **supertype**, not just the superclass: its type arguments are what
    // the base was instantiated with, and they are needed as much as the name.
    // `_AnimatedSizeState extends State<AnimatedSize> with
    // SingleTickerProviderStateMixin` puts a synthetic class in between, so
    // reading `node.supertype.typeArguments` gave the *mixin application's*
    // arguments -- none -- and `State`'s `T? _widget` was flattened in with
    // its `T` still standing.
    //
    // The mixins are picked up on the way past: skipping the synthetic class
    // silently dropped them too.
    var superType = node.supertype;
    final mixins = <IrType>[];
    while (superType != null && superType.classNode.isAnonymousMixin) {
      // `implementedTypes`, for the reason `_realOwner` gives: an applied
      // mixin has already been copied in and `mixedInType` cleared.
      for (final applied in superType.classNode.implementedTypes) {
        mixins.add(_type(applied.asInterfaceType));
      }
      superType = superType.classNode.supertype;
    }
    final base = superType?.classNode;
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
    // An enhanced enum with *methods* is a Rust enum plus an impl, and that
    // loses nothing -- which is what the old refusal was protecting against.
    // Only per-variant **state** is out of reach: a Dart enum can give each
    // value its own final fields, and a Rust enum would have to give every
    // variant a payload to say the same. 16 of the 284 enums here are
    // enhanced, and 5 of those carry fields.
    final carried = node.isEnum
        ? [
            for (final f in node.fields)
              if (!f.isStatic && !implicitEnumMembers.contains(f.name.text))
                f.name.text,
          ]
        : const <String>[];
    // Per-variant state is only out of reach when it cannot be *read off the
    // constants*. `enum Tristate { none(0), isTrue(1), isFalse(2) }` gives
    // each value a `value`, and those are constants of the variant, so the
    // Rust for them is a `match` in a getter rather than a payload on the
    // enum. When every variant's every field arrives as a literal, the enum
    // translates; when one does not -- a variant holding a list, say -- it
    // stays refused rather than half-translated.
    final carriedValues =
        enumFields[node] ?? const <String, Map<String, String>>{};
    final names = enumValues[node] ?? const <String>[];
    // A variant is a static const field **whose type is the enum itself**.
    // This used to read "every static const field except `values`", and an
    // enhanced enum may declare ordinary constants alongside its variants:
    // `_CupertinoMenuWidth` has four variants and a
    // `static const double _kTabletWidthThreshold = 768.0`, which counted as a
    // fifth. Nothing recovers per-variant state for something that is not a
    // variant, so the state map came up one key short.
    final declared = node.isEnum
        ? [
            for (final f in node.fields)
              if (f.isStatic &&
                  f.isConst &&
                  f.name.text != 'values' &&
                  _isVariantOf(node, f))
                f.name.text,
          ]
        : const <String>[];
    // The list that would actually be emitted: the declaration when the dill
    // still carries it, otherwise the names recovered from the constants. The
    // state recovery has to be judged on *that* list. Judged on `names` and
    // then emitted from `declared`, a variant the constants never named is a
    // key that is not there -- and it arrived as a crash rather than as a
    // refusal, which is the one thing this front end is not allowed to do.
    final variants = declared.isNotEmpty ? declared : names;
    final stateRecovered =
        carried.isNotEmpty &&
        variants.isNotEmpty &&
        variants.every(
          (v) =>
              carriedValues[v] != null &&
              carried.every(carriedValues[v]!.containsKey),
        );
    final enhanced = carried.isNotEmpty && !stateRecovered;
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
    final values = !node.isEnum || enhanced ? const <String>[] : declared;
    // Only when the enum is otherwise translatable. Recovering the variants of
    // an *enhanced* enum would emit it as a plain one and drop its members --
    // which is the thing the refusal exists to prevent, and which this
    // recovery quietly undid until the fixture said so.
    final recovered = values.isNotEmpty || enhanced || !node.isEnum
        ? values
        : names;
    final cls = IrClass(
      node.name,
      typeParameters: [
        for (final p in node.typeParameters)
          if (!_erasedParameter(p)) p.name ?? 'T',
      ],
      superclassArguments: superType == null
          ? const []
          : _erasedArguments(superType.classNode, superType.typeArguments),
      // `class ByteStream extends StreamView<List<int>>`: the prelude's
      // `StreamView` has no struct to flatten, so the subclass carries the
      // one field it would have inherited, `_stream` (added below), and
      // has no Rust superclass.
      superclass:
          node.isEnum ||
              base == null ||
              base.name == 'Object' ||
              _isStreamView(base)
          ? null
          : base.name,
      mixins: node.isEnum ? const [] : mixins,
      // The class's own `implements` clause. The applied mixins reached
      // through `implementedTypes` above belong to the *synthetic* classes on
      // the way up, not to this one, so the two lists do not overlap.
      // A mixin's `on` types come along: `SourceSpanMixin on SourceSpan`
      // calls `start` on `this`, and the free function holding that body is
      // bounded by the trait, which had to say it is a `SourceSpan` too.
      interfaces: node.isEnum
          ? const []
          : [
              for (final t in node.implementedTypes) _type(t.asInterfaceType),
              if (node.isMixinDeclaration)
                for (final t in node.onClause) _type(t.asInterfaceType),
            ],
      counted: _counted,
      isAbstract: node.isAbstract || _isOpen(node),
      isEnum: node.isEnum,
      values: recovered,
      valueFields: stateRecovered
          ? {for (final v in recovered) v: carriedValues[v]!}
          : const {},
    );
    _superclass = cls.superclass;
    final refused = <String>[];
    if (base != null && _isStreamView(base)) {
      cls.fields.add(
        IrFieldDecl(
          '_stream',
          IrType(
            'Stream',
            arguments: [
              for (final t in superType?.typeArguments ?? const <DartType>[])
                _type(t),
            ],
          ),
          isFinal: true,
        ),
      );
    }

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
        final stub = _stubFor(procedure, '$error');
        if (stub != null) cls.methods.add(stub);
      }
    }
    // The applied mixins' members. After TFA a mixin's fields and methods
    // live on the anonymous application classes between this class and its
    // written superclass (`_MixinApplication459&ListNotifier&StateMixin`
    // holds `StateMixin`'s `_value` and `refresh`), and the mixin
    // declaration itself is left hollow. Those classes are not lowered on
    // their own, so their members are this class's: a struct has no other
    // way to carry them. The class's own declarations override by name.
    if (!node.isEnum) {
      final own = {
        for (final f in node.fields) f.name.text,
        for (final p in node.procedures) p.name.text,
      };
      var applied = node.supertype;
      while (applied != null && applied.classNode.isAnonymousMixin) {
        final anonymous = applied.classNode;
        for (final field in anonymous.fields) {
          if (!own.add(field.name.text)) continue;
          try {
            _lowerField(cls, field);
          } on Unsupported catch (error) {
            refused.add('$error');
          }
        }
        for (final procedure in anonymous.procedures) {
          if (procedure.isAbstract || !own.add(procedure.name.text)) continue;
          try {
            _lowerProcedure(cls, procedure);
          } on Unsupported catch (error) {
            refused.add('$error');
          }
        }
        applied = anonymous.supertype;
      }
    }
    // An abstract class or mixin that `implements` a *concrete* class --
    // `SourceLocationMixin implements SourceLocation` -- reads that class's
    // fields through `this`. A trait has no fields, so the trait declares
    // the public ones as getters and every implementer answers with its own
    // (the backend forwards a struct's field for a trait getter). 7
    // `this_.source_url()` on an `&__Self` in source_span.
    if (node.isAbstract || node.isMixinDeclaration) {
      final declared = {
        for (final m in cls.methods) m.name,
        for (final m in cls.abstractMethods) m.name,
      };
      for (final t in node.implementedTypes) {
        final iface = t.classNode;
        if (iface.isAbstract) continue;
        for (final f in iface.fields) {
          if (f.isStatic || f.name.isPrivate) continue;
          if (!declared.add(f.name.text)) continue;
          cls.abstractMethods.add(
            IrMethod(
              f.name.text,
              const [],
              _type(f.type),
              const IrBlock([]),
              isGetter: true,
            ),
          );
        }
        for (final p in iface.procedures) {
          if (p.isStatic ||
              p.kind != ProcedureKind.Getter ||
              p.name.isPrivate) {
            continue;
          }
          if (!declared.add(p.name.text)) continue;
          cls.abstractMethods.add(
            IrMethod(
              p.name.text,
              const [],
              _type(p.function.returnType),
              const IrBlock([]),
              isGetter: true,
            ),
          );
        }
      }
    }
    return (cls, refused);
  }

  void _lowerField(IrClass cls, Field field) {
    _enter(field);
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
          _intoObject(
            init,
            field.type,
            _intoDeclaredNum(
              init,
              field.type,
              _widened(init, field.type, expression(init)),
            ),
          ),
          // A `static final` is computed once on first use, which is what
          // `LazyLock` is. It was refused while there was nothing to say it
          // with; there is now.
          isLazy: !field.isConst,
          // A plain `static` is assignable, so it needs a cell as well as the
          // lock -- the same shape a mutable top-level has. 73 writes to
          // these were refused as `expression StaticSet`, most of them a
          // `??=` caching something on the class.
          isMutable: !field.isConst && !field.isFinal,
        ),
      );
    } else {
      if (_inspectorOnly(name, field.type)) return;
      final initial = field.initializer;
      cls.fields.add(
        IrFieldDecl(
          name,
          _type(field.type),
          isFinal: field.isFinal,
          initial: initial == null ? null : expression(initial),
          shared: _sharedFields.contains(name),
          isLate: field.isLate,
        ),
      );
    }
  }

  void _lowerConstructor(IrClass cls, Constructor node) {
    _enter(node);
    if (cls.isEnum) return;
    final name = node.name.text;
    final params = <IrParam>[];
    for (final p in node.function.positionalParameters) {
      params.add(IrParam(_paramName(p), _type(p.type)));
    }
    for (final p in node.function.namedParameters) {
      // The inspector's parameter is dropped with its field. See
      // `_inspectorOnly`.
      if (_inspectorOnly(p.parameterName)) continue;
      params.add(IrParam(p.parameterName, _type(p.type), named: true));
    }

    final inits = <String, IrExpr>{};
    final asserts = <IrAssert>[];
    String? superBase;
    String? superName;
    var superArgs = const <IrExpr>[];
    String? redirectTo;
    var redirectArgs = const <IrExpr>[];
    var redirects = false;
    final pre = <IrStmt>[];
    for (final init in node.initializers) {
      if (init is FieldInitializer) {
        // Into the field's type: `creator = filter` with a `_GaussianBlur
        // ImageFilter` in hand and an `ImageFilter` field is `Rc::new(..)`,
        // a nullable field takes `Some(..)`.
        inits[init.field.name.text] = _widened(
          init.value,
          init.field.type,
          expression(init.value),
        );
      } else if (init is AssertInitializer) {
        final statement = init.statement;
        asserts.add(_assert(statement.condition, statement.message));
      } else if (init is SuperInitializer) {
        var base = node.enclosingClass.superclass;
        while (base != null && base.isAnonymousMixin) {
          // A mixin field's initialiser -- `AnimationLocalStatusListeners
          // Mixin._statusListeners = ObserverList()` -- was moved by the
          // CFE into the application's synthetic constructor, which this
          // walk passes over: its field initialisers come along (15
          // `AnimationController::new` missing, 6 `ProxyAnimation`).
          for (final synthetic in base.constructors) {
            for (final moved in synthetic.initializers) {
              if (moved is FieldInitializer) {
                inits.putIfAbsent(
                  moved.field.name.text,
                  () => expression(moved.value),
                );
              }
            }
          }
          base = base.superclass;
        }
        // A no-argument `super()` into a translated base is *not* nothing:
        // the CFE moves a field's initialiser into the constructor
        // (`Action._listeners = ObserverList()` arrives as a
        // `FieldInitializer` of `Action()`), and the subclass gets it only
        // through the call. `DoNothingAction`, `CallbackAction` and every
        // other `Action` were refused as "field never initialised" (13
        // "no associated function `new`" on `WidgetsApp.defaultActions`).
        final passesArguments =
            init.arguments.positional.isNotEmpty ||
            init.arguments.named.isNotEmpty;
        final translatedBase =
            base != null && base.enclosingLibrary.importUri.scheme != 'dart';
        if (passesArguments || translatedBase) {
          if (base == null) {
            throw Unsupported(
              'super constructor call with no base',
              _sample(init),
            );
          }
          // `super(stream)` into `StreamView`: the stream goes into the
          // `_stream` field the subclass carries (see `lowerClass`).
          if (_isStreamView(base) && init.arguments.positional.length == 1) {
            final stream = init.arguments.positional.single;
            inits['_stream'] = _widened(
              stream,
              init.target.function.positionalParameters.single.type,
              expression(stream),
            );
            continue;
          }
          superBase = base.name;
          superName = init.target.name.text.isEmpty
              ? null
              : init.target.name.text;
          superArgs = _arguments(init.arguments, init.target.function);
        }
        // A no-argument super() adds nothing to a Rust struct literal.
      } else if (init is LocalInitializer) {
        // `: final #t = e, super(#t, #t)` -- a temporary bound in the
        // initialiser list. A `let` before the fields are set, named like
        // every other CFE temporary, so the super arguments find it.
        pre.add(_declare(init.variable, init));
      } else if (init is RedirectingInitializer) {
        // `: this._(string, 0, 0)`. 94 in the gallery's dill. The target is
        // a constructor of this same class, so its parameter list is right
        // here to order the named arguments by.
        redirects = true;
        final target = init.target.name.text;
        redirectTo = target.isEmpty ? null : target;
        redirectArgs = _arguments(init.arguments, init.target.function);
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
    var real = statements.where((s) => s is! EmptyStatement).toList();
    // A constructor the AOT compiler gutted: every `IconData(..)` upstream
    // is a constant, so the runtime never runs the constructor and TFA
    // left it with no initialisers and a body that throws "code removed".
    // This output rebuilds those constants as constructor calls (74
    // `IconData::new` missing), so the constructor is rebuilt from its
    // signature: a field takes the parameter of the same name, which is
    // what `this.codePoint` meant.
    final gutted =
        real.length == 1 &&
        real.single is ExpressionStatement &&
        (real.single as ExpressionStatement).expression is Throw &&
        _tfaUnreachable(
          (real.single as ExpressionStatement).expression as Throw,
        );
    if (gutted && !node.initializers.any((i) => i is FieldInitializer)) {
      final byName = <String, String>{
        for (final p in node.function.positionalParameters)
          _paramName(p): _paramName(p),
        for (final p in node.function.namedParameters)
          p.parameterName: p.parameterName,
      };
      for (final field in node.enclosingClass.fields) {
        if (field.isStatic || field.initializer != null) continue;
        final param = byName[field.name.text];
        if (param != null) inits[field.name.text] = IrLocal(param);
      }
      real = const [];
    }
    cls.constructors.add(
      IrConstructor(
        params,
        inits,
        isConst: node.isConst,
        name: name.isEmpty ? null : name,
        asserts: asserts,
        superBase: superBase,
        superName: superName,
        superArgs: superArgs,
        redirectTo: redirects ? (redirectTo ?? '') : null,
        redirectArgs: redirectArgs,
        pre: pre,
        body: real.isEmpty
            ? null
            : IrBlock([for (final s in real) statement(s)]),
      ),
    );
  }

  void _lowerProcedure(IrClass cls, Procedure node) {
    _enter(node);
    // An unnamed factory has no name in Kernel; an empty identifier stopped
    // all 37 members of vector_math's classes through `_computeFailing`.
    // `new`, as the backend spells the call.
    final name = node.kind == ProcedureKind.Factory && node.name.text.isEmpty
        ? 'new'
        : _dartName(node.name.text);
    if (node.isNoSuchMethodForwarder) {
      _lowerForwarder(cls, node);
      return;
    }
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
      if (implicit.contains(name) || node.isSynthetic) return;
      // Not implicit: a method or getter the programmer wrote. It goes in the
      // enum's `impl`, where it loses nothing.
      if (cls.values.isEmpty) {
        throw Unsupported('member of an enum with no values', cls.name);
      }
    }

    final params = [
      for (final p in node.function.positionalParameters)
        IrParam(
          _paramName(p),
          _type(p.type),
          kept: _keeps(node.function, p),
          hasDefault: p.defaultValue != null,
          defaultValue: _default(p),
        ),
      for (final p in node.function.namedParameters)
        if (!_inspectorOnly(p.parameterName))
          IrParam(
            p.parameterName,
            _type(p.type),
            named: true,
            kept: _keeps(node.function, p),
            hasDefault: p.defaultValue != null,
            defaultValue: _default(p),
          ),
    ];
    final isOperator = node.kind == ProcedureKind.Operator;
    final thrown = <String>{};
    if (!node.isAbstract) {
      final finder = _ThrowFinder();
      node.function.accept(finder);
      thrown.addAll(finder.types);
      // Two error types were once two `Result`s. A throw is a panic now
      // (the backend's `_thrown`), and the method carries `Object` -- the
      // type every Dart throw has -- for the `try` bodies that still catch.
      if (thrown.length > 1) {
        thrown
          ..clear()
          ..add('Object');
      }
    }
    final method = IrMethod(
      name,
      params,
      _returnType(node.function),
      node.isAbstract ? const IrBlock([]) : _body(node.function),
      typeParameters: [
        for (final p in node.function.typeParameters)
          if (!_erasedParameter(p)) p.name ?? 'T',
      ],
      isStatic: node.isStatic,
      isGetter: node.kind == ProcedureKind.Getter,
      isSetter: node.kind == ProcedureKind.Setter,
      operator: isOperator ? name : null,
      fails: _fails(node),
      throws: thrown.isEmpty ? null : thrown.single,
      // Only plain `async`. `async*` and `sync*` are generators, which Rust
      // has no direct word for, and there are five of them in the package.
      isAsync: node.function.asyncMarker == AsyncMarker.Async,
    );
    (node.isAbstract ? cls.abstractMethods : cls.methods).add(method);
  }

  /// A `noSuchMethod` forwarder, lowered from what it *is* rather than from
  /// its body.
  ///
  /// A class that declares `noSuchMethod` and implements an interface gets one
  /// of these per interface member from the CFE -- `_WidgetTextStyleMapper`,
  /// three lines of Dart, arrives with thirty-four. The body it is given is
  /// `noSuchMethod(new _InvocationMirror._withType(#name, kind, ...))`, and
  /// `_InvocationMirror` is the VM's own private class: translating that line
  /// would name something no library here declares. What the forwarder means
  /// is fully said by its name and its kind, so that is what is emitted:
  /// `noSuchMethod(Invocation.getter(#name))`. 294 `SymbolConstant`
  /// refusals were these, every one.
  void _lowerForwarder(IrClass cls, Procedure node) {
    final name = node.name.text;
    final kind = switch (node.kind) {
      ProcedureKind.Getter => 'getter',
      ProcedureKind.Setter => 'setter',
      _ => 'method',
    };
    final invocation = IrStaticCall('Invocation', kind, [
      IrLiteral('Symbol::of("$name")', const IrType('raw')),
    ]);
    final params = [
      for (final p in node.function.positionalParameters)
        IrParam(_paramName(p), _type(p.type)),
      for (final p in node.function.namedParameters)
        IrParam(p.parameterName, _type(p.type), named: true),
    ];
    cls.methods.add(
      IrMethod(
        name,
        params,
        _returnType(node.function),
        IrBlock([
          // `noSuchMethod` yields `Never`, spelled `Infallible`, which does
          // not coerce to the forwarder's own return type; the prelude's
          // `never()` does the coercion `!` would have done.
          IrReturn(
            IrStaticCall(null, 'never', [
              IrCall(const IrThis(), 'noSuchMethod', [invocation]),
            ]),
          ),
        ]),
        typeParameters: [
          for (final p in node.function.typeParameters) p.name ?? 'T',
        ],
        isGetter: node.kind == ProcedureKind.Getter,
        isSetter: node.kind == ProcedureKind.Setter,
        operator: node.kind == ProcedureKind.Operator ? name : null,
      ),
    );
  }
}

/// Whether a function body mentions `this` anywhere inside it.
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
class _CapturedWrites extends RecursiveVisitor {
  final _declaredIn = <Variable, FunctionNode>{};
  final _stack = <FunctionNode>[];
  final found = <Variable>{};

  static Set<Variable> of(Member member) {
    final v = _CapturedWrites();
    member.accept(v);
    return v.found;
  }

  @override
  void visitFunctionNode(FunctionNode node) {
    _stack.add(node);
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
    }
    super.visitVariableSet(node);
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
    const mutating = {
      'add',
      'addAll',
      'remove',
      'removeAt',
      'removeLast',
      'removeWhere',
      'retainWhere',
      'clear',
      'insert',
      'insertAll',
      'sort',
      'shuffle',
      'addFirst',
      'addLast',
      'removeFirst',
      'putIfAbsent',
      'update',
      'setRange',
      'fillRange',
      'replaceRange',
      'setAll',
      '[]=',
    };
    final receiver = node.receiver;
    if (receiver is InstanceGet &&
        receiver.receiver is ThisExpression &&
        mutating.contains(node.name.text)) {
      found = true;
    }
    super.visitInstanceInvocation(node);
  }
}

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
  FunctionType(:final positionalParameters, :final returnType) =>
    positionalParameters.any((t) => _mentions(t, cls)) ||
        _mentions(returnType, cls),
  _ => false,
};

/// Whether a class tears off one of its own methods anywhere.
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
    final target = node.target;
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

/// The libraries a library actually names.
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
/// The class names a library mentions, and where each came from.
///
/// Filled by the same walk as [librariesReferencedBy] and returned beside it,
/// so the two cannot drift apart. See `_ReferenceCollector.namedClasses`.
Map<String, Set<Library>> classNamesReferencedBy(Library library) {
  final visitor = _ReferenceCollector(<Library>{});
  library.accept(visitor);
  for (final cls in library.classes) {
    // The same walk `librariesReferencedBy` does, for the same reason: a name
    // that arrives by flattening has to be resolved like any other, and the
    // two lists would drift if they were gathered differently.
    _climb(cls, (node) {
      visitor._class(node);
      node.accept(visitor);
    });
  }
  return visitor.namedClasses;
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

Set<Library> librariesReferencedBy(Library library) {
  final found = <Library>{};
  final visitor = _ReferenceCollector(found);
  library.accept(visitor);
  for (final cls in library.classes) {
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
      found.add(node.enclosingLibrary);
      node.accept(visitor);
    });
  }
  found.remove(library);
  return found;
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

  void _member(Member? member) {
    if (member != null) found.add(member.enclosingLibrary);
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
    super.visitConstructorInvocation(node);
  }

  @override
  void visitStaticInvocation(StaticInvocation node) {
    _member(node.target);
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
