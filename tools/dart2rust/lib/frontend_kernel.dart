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

import 'coerce.dart';
import 'member_names.dart';

import 'dart:io' show Platform, stderr;

import 'ir.dart';
// Read, not emitted: the prelude is the only place that knows which of its
// own types it wrote generic (`_genericPreludeTypes`).
import 'prelude.dart';

// The class is one class in every file below but `visitors.dart`.
// Splitting it is the only thing this does: each part holds one of the
// sections the file already had (`// -- Expressions --`), moved without a
// character changed, and `augment` puts them back together. The sections
// share 35 of the class's 77 fields, so they are not separable objects;
// making them so is the state-object round, not this one.
//
// `visitors.dart` is the rest of the file: the `RecursiveVisitor`s and
// the whole-program queries, which are ordinary top-level declarations
// and were never part of the class.
//
// Named rather than counted: the sentence said "six files" until
// 2026-09-09, when there were eighteen of them.
//
// `augment` is behind `--enable-experiment=augmentations`, which
// `bin/experiments.sh` is the one place that names.
part 'frontend_kernel/types.dart';
part 'frontend_kernel/expressions.dart';
part 'frontend_kernel/expression_raw.dart';
part 'frontend_kernel/raw_operators.dart';
part 'frontend_kernel/raw_writes.dart';
part 'frontend_kernel/raw_casts.dart';
part 'frontend_kernel/closures.dart';
part 'frontend_kernel/locals.dart';
part 'frontend_kernel/reads_and_calls.dart';
part 'frontend_kernel/dispatch.dart';
part 'frontend_kernel/static_calls.dart';
part 'frontend_kernel/slots.dart';
part 'frontend_kernel/coercion.dart';
part 'frontend_kernel/parameters.dart';
part 'frontend_kernel/constants.dart';
part 'frontend_kernel/statements.dart';
part 'frontend_kernel/declarations.dart';
part 'frontend_kernel/visitors.dart';

/// `dart:math`'s functions that have no name to tear off: a *call* to one
/// becomes an inherent method of the receiver (`f64::max`, `Ord::max`), so
/// the value is the prelude's free function of the same meaning.
const _mathValueNames = {'max': 'dart_max_of', 'min': 'dart_min_of'};

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

/// The identifier a CFE-lowered top-level name becomes: an extension's or
/// extension type's member (`MediaQueryHinge|get#hinge`, `BaselineOffset|+`)
/// with its separators as `_` and an operator spelled by name -- `+` and
/// `<` both cleaned to `baseline_offset__` and redefined each other
/// (ws525). One spelling, at the declaration and at every call.
String _topLevelName(String text) {
  const operators = {
    '+': 'op_add',
    '-': 'op_sub',
    '*': 'op_mul',
    '/': 'op_div',
    '~/': 'op_truncdiv',
    '%': 'op_rem',
    'unary-': 'op_neg',
    '&': 'op_bitand',
    '|': 'op_bitor',
    '^': 'op_bitxor',
    '<<': 'op_shl',
    '>>': 'op_shr',
    '>>>': 'op_ushr',
    '<': 'op_lt',
    '>': 'op_gt',
    '<=': 'op_le',
    '>=': 'op_ge',
    '==': 'op_eq',
    '[]': 'op_index',
    '[]=': 'op_index_set',
    '~': 'op_not',
  };
  final bar = text.indexOf('|');
  if (bar >= 0) {
    final member = text.substring(bar + 1);
    final named = operators[member];
    if (named != null) return '${text.substring(0, bar)}_$named';
  }
  return text.replaceAll(RegExp(r'[|#]'), '_');
}

class KernelFrontend implements TypeWorld {
  KernelFrontend(
    this.library, {
    this.enumValues = const {},
    this.enumFields = const {},
    this.abstractElsewhere = const {},
    this.elsewhere = const {},
    this.typeEnvironment,
    this.dynamicSlots = const {},
    this.dynamicMembers = const {},
    this.identityObserved = const {},
    this.open = const {},
    this.erase = false,
    this.eraseObjectBounded = false,
    this.coerceByType = true,
    this.instantiations,
    this.applications = const {},
    this.moduleOf = const {},
    this.collidingClassNames = const {},
    this.aliasMutated = const {},
    this.covariantParameters = const {},
  });

  /// Each translated library's module name, from the driver: what a
  /// qualified top-level reference is spelled by (`IrStaticCall.module`).
  final Map<Library, String> moduleOf;

  /// Class names more than one translated library declares (the driver):
  /// a reference to another library's is spelled by module.
  final Set<String> collidingClassNames;

  /// The module a reference to `cls` carries (`IrType.module`): the
  /// declaring library's, when the name is one two libraries declare and
  /// the class is another library's (`ui.StrutStyle(..)` in `painting`'s
  /// `TextStyle.getParagraphStyle` resolved to painting's own
  /// `StrutStyle`, run679). Null otherwise.
  String? _moduleQualifier(Class cls) {
    if (!collidingClassNames.contains(cls.name)) return null;
    final home = cls.enclosingLibrary;
    if (identical(home, library)) return null;
    return moduleOf[home];
  }

  /// The module to qualify a top-level `target` by: another library's,
  /// when this library declares a top-level of the same name -- an
  /// unqualified call would resolve to this library's own
  /// (`defaultTargetPlatform` wrapping `_platform_io`'s, run481).
  String? _topLevelModule(Member target) {
    if (target.enclosingClass != null) return null;
    final home = target.enclosingLibrary;
    if (identical(home, library)) return null;
    final name = target.name.text;
    final shadowed =
        library.procedures.any((p) => p.name.text == name) ||
        library.fields.any((f) => f.name.text == name);
    return shadowed ? moduleOf[home] : null;
  }

  /// For each mixin declaration, an anonymous class the CFE applied it to,
  /// which holds the bodies the declaration lost (see the driver). A
  /// mixin's trait takes its default methods -- and so its super
  /// functions, which `super.x()` across a mixin chain reaches -- from
  /// there (`WidgetsBinding.framesEnabled` calling `SchedulerBinding`'s,
  /// run434).
  final Map<Class, List<Class>> applications;

  /// The closed world's census of generic trait-like classes' instantiations,
  /// shared by every library's lowering: each `_type` of a `Foo<X>` records
  /// it, and `addWiderImpls` reads it once every library is lowered.
  final Map<Class, Set<InterfaceType>>? instantiations;

  /// The Kernel class behind each lowered class, for `addWiderImpls`.
  final Map<String, Class> _kernelClasses = {};

  /// The classes a `super` call landed in, with the member and whether it
  /// is a setter.
  ///
  /// `super.foo()` is `render_box_super_foo` here, and which class holds the
  /// body is `_realOwner`'s answer: a super call in an applied mixin names
  /// its `on` constraint and dispatches to the mixin *before it in the
  /// chain*, which is neither the interface target's class nor the calling
  /// one. Nothing outside `_realOwner` can work that out, so it is written
  /// down where it is decided (`SchedulerBinding.initInstances` reaching
  /// `gesture_binding_super_init_instances`).
  final Set<(Class, String, bool)> superOwners = {};

  /// The classes a *census* made this library name.
  ///
  /// Two answers here are whole-program ones, and neither is in the library's
  /// own Kernel body. A wider impl comes from the instantiation census, so
  /// `foundation/diagnostics.dart` writes `impl DiagnosticsProperty<Object>
  /// for DiagnosticsPropertyImpl<Rc<dyn Color>>` because *somewhere else* a
  /// `DiagnosticsProperty<Color>` was named; a dynamic slot's arms come from
  /// the slot census, so `date_symbol_data_custom.dart` downcasts to
  /// `UninitializedLocaleData` for a slot it never declares. The reference is
  /// this library's all the same -- it is in the file that has to compile --
  /// so the census writes down what it made the library say. (`Color` was one
  /// `cannot find trait` that no function owned, and it stopped the workspace
  /// at 24 crates of 65.)
  final Set<Class> injectedClasses = {};

  /// The members a census made this library name (`injectedClasses`).
  ///
  /// `dateTimeSymbols[k] = v` reads through `dynamic get dateTimeSymbols =>
  /// _dateTimeSymbols` to the *private* field of another library, which no
  /// Dart source could have written and the emitter writes all the same.
  final Set<Member> injectedMembers = {};

  void _injected(DartType type) {
    if (type is! InterfaceType) return;
    injectedClasses.add(type.classNode);
    type.typeArguments.forEach(_injected);
  }

  /// Off while a type parameter's *bound* is spelled: `T extends
  /// _RRectLike<T>` names no instantiation anything holds, and an impl for
  /// it is noise.
  var _censusOff = false;

  IrType _typeOfBound(DartType bound) {
    final was = _censusOff;
    _censusOff = true;
    try {
      return _type(bound);
    } finally {
      _censusOff = was;
    }
  }

  /// Every class of `lowered` gets an impl for each wider instantiation of a
  /// generic trait it implements that the program names somewhere
  /// (`IrClass.extraImpls`): the one whose arguments differ from the class's
  /// own and that Dart's covariance admits (`Foo<X>` is a `Foo<Y>` for
  /// `X <: Y`). Only concrete instantiations: one naming a type parameter is
  /// another declaration's business.
  void addWiderImpls(IrLibrary lowered, Set<Library> visible) {
    final census = instantiations;
    final env = typeEnvironment;
    if (census == null || env == null) return;
    // Whether every class a type names is visible from this library: an
    // impl written here cannot name the gallery's enum from `material`.
    bool nameable(DartType t) {
      if (t is InterfaceType) {
        final lib = t.classNode.enclosingLibrary;
        if (lib.importUri.scheme != 'dart' && !visible.contains(lib)) {
          return false;
        }
        return t.typeArguments.every(nameable);
      }
      if (t is FunctionType) {
        return nameable(t.returnType) &&
            t.positionalParameters.every(nameable) &&
            t.namedParameters.every((n) => nameable(n.type));
      }
      return true;
    }

    for (final ir in lowered.classes) {
      if (ir.isAbstract || ir.isEnum) continue;
      // An open class's own struct (`NumValImpl`) is the Dart class too:
      // its wider impls are the class's (`dartName`; a `NumVal<int>`
      // handle into a `Prop<Object?>` slot found none, the restoreprop
      // fixture).
      final node = _kernelClasses[ir.dartName ?? ir.name];
      if (node == null) continue;
      // What has an impl already, by the *Rust* type (`sameRust`): `Object`
      // and `dynamic` are one spelling, and `ValueKey<Object>` beside
      // `ValueKey<dynamic>` was two impls of one trait (E0119, ws498).
      final spelled = <String, List<List<IrType>>>{};
      bool sameArgs(List<IrType> a, List<IrType> b) =>
          a.length == b.length &&
          [for (var i = 0; i < a.length; i++) sameRust(a[i], b[i])]
              .every((same) => same);
      bool seen(String name, List<IrType> args) {
        final list = spelled.putIfAbsent(name, () => []);
        if (list.any((s) => sameArgs(s, args))) return true;
        list.add(args);
        return false;
      }

      // A generic class per concrete instantiation the program names
      // (`IrClass.extraImplSelf`); a plain one once, as itself.
      final selves = <(InterfaceType, List<IrType>?)>[];
      if (node.typeParameters.isEmpty) {
        selves.add((
          node.getThisType(env.coreTypes, Nullability.nonNullable),
          null,
        ));
      } else {
        for (final inst in census[node] ?? const <InterfaceType>{}) {
          if (inst.typeArguments.any(_mentionsTypeParameter)) continue;
          if (!inst.typeArguments.every(nameable)) continue;
          final List<IrType> selfArgs;
          try {
            selfArgs = _erasedArguments(node, inst.typeArguments);
          } on Unsupported {
            continue;
          }
          // Every parameter erased: the class as declared (`MapEquality<>`
          // was spelled for one, ws627).
          _injected(inst);
          selves.add((inst, selfArgs.isEmpty ? null : selfArgs));
        }
      }
      for (final (thisType, selfArgs) in selves) {
        final selfKey = selfArgs == null ? '' : '${selfArgs.join(',')}|';
        for (final entry in census.entries) {
          final base = entry.key;
          // An open class's own trait is a base of its struct too:
          // `TweenImpl<f64>` answers for `Tween<Object>` as `ColorTween`
          // does (`_AnimatedPhysicalModelState.forEachTween`'s visitor
          // cast, run658).
          if ((identical(base, node) && !_isOpen(node)) ||
              base.typeParameters.isEmpty) {
            continue;
          }
          // As Rust holds it: a class reaches a base through its
          // supertype clauses, each spelled with its erased parameters at
          // their bounds (`BoxBuilder: CBuilder<BoxC>: Builder<C>` is
          // `Builder<Rc<dyn Constraints>>`, and Dart's `Builder<BoxC>` beside
          // it was the same impl twice, E0119, the atbounds fixture).
          final asBase = _asRustInstance(thisType, base);
          if (asBase == null) continue;
          // A generic class's own instantiation names its parameter
          // (`DefaultEquality<E>: Equality<E>`): a wider impl would overlap
          // it for the `E` that is the wider type (E0119).
          if (asBase.typeArguments.any(_mentionsTypeParameter)) continue;
          final own = _erasedArguments(base, asBase.typeArguments);
          if (own.isEmpty) continue;
          for (final wider in entry.value) {
            if (wider == asBase) continue;
            if (wider.typeArguments.any(_mentionsTypeParameter)) continue;
            if (!wider.typeArguments.every(nameable)) continue;
            // Not at `void`: the unit has no `Option` to forward through
            // (`DiagnosticsProperty<void>`, `FlagProperty._value`, ws629).
            if (wider.typeArguments.any((a) => a is VoidType)) continue;
            if (!env.isSubtypeOf(asBase, wider)) continue;
            // The trait and every generic trait above it, as the wider
            // instantiation reaches them: `impl Tween<Object> for IntTween`
            // asks `IntTween: Animatable<Object>` of its supertrait.
            for (final above in [base, ..._kernelAncestors(base)]) {
              // An anonymous mixin application is no trait anyone names.
              if (above.isAnonymousMixin) continue;
              if (above.typeParameters.isEmpty ||
                  !_translatedClass(above) ||
                  !_abstractLike(above)) {
                continue;
              }
              final asAbove = _asRustInstance(wider, above);
              final ownAbove = _asRustInstance(thisType, above);
              if (asAbove == null || ownAbove == null) continue;
              if (ownAbove.typeArguments.any(_mentionsTypeParameter)) continue;
              final List<IrType> args, ownArgs;
              try {
                args = _erasedArguments(above, asAbove.typeArguments);
                ownArgs = _erasedArguments(above, ownAbove.typeArguments);
              } on Unsupported {
                continue;
              }
              if (args.isEmpty) continue;
              if (sameArgs(args, ownArgs) ||
                  seen('$selfKey${above.name}', args)) {
                continue;
              }
              injectedClasses.add(above);
              _injected(asAbove);
              _injected(ownAbove);
              ir.extraImpls.add(IrType(above.name, arguments: args));
              ir.extraImplSelf.add(selfArgs);
            }
          }
        }
      }
    }
  }

  /// Whether a value into a slot is adapted by comparing the two Rust
  /// types (`coerce`) before the shape rules of `_widened` get a look.
  /// `DART2RUST_COERCE=0` measures without.
  final bool coerceByType;

  /// Whether the slot being filled belongs to translated code. A prelude
  /// callee's Rust signature is its own (`AssertionError::new(String)`,
  /// `Object::hash` generic over what it takes), not Dart's declaration,
  /// so its arguments are not coerced by the declared type.
  var _slotTranslated = true;

  /// ..except a prelude callee's *generic* slot: `List<Widget>.add(E)` holds
  /// what the translated element type says, so that one is coerced.
  bool _calleeTranslated(FunctionNode? callee, DartType? declared) {
    final member = callee?.parent;
    if (member is! Member) return true;
    // By the class when there is one: a deduplicated mixin application's
    // constructor lives in `dart:mixin_deduplication` and is translated
    // code all the same (`_NotificationElement(super.widget)`, ws364).
    final owner = member.enclosingClass;
    if (owner != null && _translatedClass(owner)) return true;
    final uri = member.enclosingLibrary.importUri;
    if (owner == null &&
        (uri.scheme != 'dart' || uri.toString() == 'dart:ui')) {
      return true;
    }
    // A type parameter's spelling is the caller's; `dynamic` has one
    // spelling, an `Rc<dyn Object>`, and the prelude uses it.
    // ..but not a `FutureOr<T>` slot: the prelude takes the `T` there
    // (`Future.value`, `Completer.complete`), so it goes by shape, as it
    // did while the walkers could not see into `FutureOr` (+47 at ws514).
    // ..and not a slot whose every argument is `dynamic` -- Kernel's
    // spelling of a raw `Map` or `Iterable` parameter (`Map.unmodifiable(
    // Map other)`), which the prelude takes generically, as the value's
    // own `Map<K, V>`; widening the values to `Rc<dyn Object>` there was
    // a `Map<.., dyn Object>` where `Map<.., dyn ThemeExtension>` came
    // back out (`ThemeData._themeExtensionIterableToMap`, run567).
    // Only a collection constructor's own such slot: `postEvent(String,
    // Map)` and `Timeline.startSync(.., Map? arguments)` are spelled
    // `Map<Rc<dyn Object>, ..>` by the prelude and are coerced (+5 the
    // round every all-`dynamic` slot was exempted, ws568).
    if (declared is InterfaceType &&
        declared.typeArguments.isNotEmpty &&
        declared.typeArguments.every((a) => a is DynamicType) &&
        owner != null &&
        _collectionConstructor(member) &&
        _coreCollections.contains(declared.classNode.name)) {
      return false;
    }
    // A bare `Function` slot (`Future.then(onError: Function?)`) is the
    // prelude's function object (`dart_function_object`, an `Rc<dyn
    // Object>` carrying the arity `dart_call_error_handler` asks for):
    // coerced, as a `dynamic` slot is. A closure handed over bare was "a
    // {closure} called as a function" once the font manifest failed to
    // load (run572).
    if (_bareFunctionType(declared)) return true;
    // ..and `Object` is spelled as `dynamic` is, an `Rc<dyn Object>`:
    // `Completer.completeError(Object error)` took the bare `Exception`
    // struct the shape rules left it (`_futurize`, run608).
    return declared != null &&
        declared is! FutureOrType &&
        (_mentionsTop(declared) || _mentionsTypeParameter(declared));
  }

  /// `dynamic` or `dart:core`'s non-nullable `Object` anywhere in a
  /// type: both are the handle `Rc<dyn Object>` on the Rust side, so a
  /// prelude slot mentioning either is coerced by type. Not `Object?`:
  /// the prelude takes that generically, as the value's own type
  /// (`Iterable.contains(Object? element)` is `contains(&T)`,
  /// `AssertionError([Object? message])`, `log(error: Object?)`; +29
  /// stubs when it was coerced, ws609).
  static bool _mentionsTop(DartType t) {
    if (t is FutureOrType) return _mentionsTop(t.typeArgument);
    if (t is RecordType) {
      return t.positional.any(_mentionsTop) ||
          t.named.any((n) => _mentionsTop(n.type));
    }
    if (t is DynamicType) return true;
    if (t is InterfaceType) {
      if (t.classNode.name == 'Object' &&
          t.nullability != Nullability.nullable &&
          t.classNode.enclosingLibrary.importUri.toString() == 'dart:core') {
        return true;
      }
      return t.typeArguments.any(_mentionsTop);
    }
    if (t is FunctionType) {
      return _mentionsTop(t.returnType) ||
          t.positionalParameters.any(_mentionsTop) ||
          t.namedParameters.any((n) => _mentionsTop(n.type));
    }
    return false;
  }

  /// `dart:core`'s `Function` (nullable or not): a slot with no signature.
  static bool _bareFunctionType(DartType? t) =>
      t is InterfaceType &&
      t.classNode.name == 'Function' &&
      t.classNode.enclosingLibrary.importUri.toString() == 'dart:core';

  /// The `dart:core` / `dart:collection` classes the prelude has one
  /// generic collection for each of.
  static const _coreCollections = {
    'Map',
    'List',
    'Set',
    'Iterable',
    'HashMap',
    'LinkedHashMap',
    'HashSet',
    'LinkedHashSet',
    'Queue',
    'ListQueue',
  };

  /// A constructor or factory of one of those (`Map.unmodifiable(Map)`,
  /// `List.from(Iterable)`): generic over what it is given.
  static bool _collectionConstructor(Member member) {
    final owner = member.enclosingClass;
    if (owner == null || !_coreCollections.contains(owner.name)) return false;
    return member is Constructor || (member is Procedure && member.isFactory);
  }

  /// The instantiations a named one *implies*: `BasicMessageChannel<
  /// String?>` holds a `MessageCodec<T> codec`, so the program names
  /// `MessageCodec<String?>` without spelling it anywhere, and the
  /// `StringCodec` handed to it needs the wider impl (`addWiderImpls`;
  /// `StringCodec: MessageCodec<Option<String>>` unsatisfied, run448).
  /// Each member's declared types, the instantiation substituted in, go
  /// through the same census; one level at a time, each instantiation once.
  static final _censused = <InterfaceType>{};

  /// The census (`addWiderImpls`): a generic trait-like class named with
  /// arguments, as the closed world names it -- and a concrete generic
  /// class's, as the instantiations its wider impls are written for
  /// (`RestorableNum<int>` as a `RestorableProperty<Object?>`, run626).
  /// One naming an *erased* parameter counts as Rust holds it, at the
  /// parameter's bound: `ConstrainedLayoutBuilder<ConstraintType extends
  /// Constraints>` is `AbstractLayoutBuilder<ConstraintType>`, which puts
  /// `LayoutBuilder` into `AbstractLayoutBuilder<Rc<dyn Constraints>>`;
  /// its element then asks its render object for
  /// `RenderAbstractLayoutBuilderMixin<Constraints, ..>`, an instantiation
  /// nothing spelled (`Option::unwrap()` on the cast, run644).
  void _census(InterfaceType type) {
    final census = instantiations;
    if (census == null || _censusOff || type.typeArguments.isEmpty) return;
    final core = type.classNode.enclosingLibrary.importUri.scheme == 'dart';
    if (!core && _translatedClass(type.classNode)) {
      census
          .putIfAbsent(type.classNode, () => {})
          .add(type.withDeclaredNullability(Nullability.nonNullable));
    }
    _censusMembers(type, census);
    if (_mentionsTypeParameter(type)) {
      final atBounds = _atErasedBounds(type, 0);
      if (atBounds is InterfaceType && !_mentionsTypeParameter(atBounds)) {
        _census(atBounds);
      }
    }
  }

  void _censusMembers(
    InterfaceType type,
    Map<Class, Set<InterfaceType>> census,
  ) {
    final cls = type.classNode;
    if (cls.enclosingLibrary.importUri.scheme == 'dart') return;
    if (_mentionsTypeParameter(type)) return;
    if (!_translatedClass(cls)) return;
    final key = type.withDeclaredNullability(Nullability.nonNullable);
    if (!_censused.add(key)) return;
    final substitution = Substitution.fromInterfaceType(key);
    // Whether the type being walked is one the bodies *construct*: such a
    // class is instantiated at this instantiation and at no other name in
    // the program, so it is censused whether or not it is trait-like --
    // the census is also what says which instantiations of a generic
    // class get wider impls. `Animatable<T>.chain` builds a
    // `_ChainedEvaluation<T>`, and `_ChainedEvaluation<f64>` had no
    // `Animatable<Object>` for the erased `TweenSequenceItem<T>` field to
    // hold: the cast came back `None` (run708). A type merely *named* in a
    // member's signature stays as it was -- censusing those as well put
    // `provider`'s `_DelegateState.element` two Rust types apart (+2,
    // ws709).
    var built = false;
    void walk(DartType t) {
      if (t is InterfaceType) {
        if (t.typeArguments.isNotEmpty &&
            !_mentionsTypeParameter(t) &&
            t.classNode.enclosingLibrary.importUri.scheme != 'dart' &&
            _translatedClass(t.classNode)) {
          if (_abstractLike(t.classNode) || built) {
            census
                .putIfAbsent(t.classNode, () => {})
                .add(t.withDeclaredNullability(Nullability.nonNullable));
          }
          _censusMembers(t, census);
        }
        t.typeArguments.forEach(walk);
      } else if (t is FunctionType) {
        walk(t.returnType);
        t.positionalParameters.forEach(walk);
        for (final n in t.namedParameters) {
          walk(n.type);
        }
      }
    }

    for (final f in cls.fields) {
      walk(substitution.substituteType(f.type));
    }
    for (final p in cls.procedures) {
      final fn = p.function;
      walk(substitution.substituteType(fn.returnType));
      for (final v in fn.positionalParameters) {
        walk(substitution.substituteType(v.type));
      }
      for (final v in fn.namedParameters) {
        walk(substitution.substituteType(v.type));
      }
    }
    for (final c in cls.constructors) {
      for (final v in c.function.positionalParameters) {
        walk(substitution.substituteType(v.type));
      }
      for (final v in c.function.namedParameters) {
        walk(substitution.substituteType(v.type));
      }
    }
    // ..and what its bodies construct: `AbstractLayoutBuilder<T>.
    // createElement` makes a `_LayoutBuilderElement<T>`, so the
    // instantiation at `Constraints` makes one at `Constraints`, whose
    // `renderObject` is asked for `RenderAbstractLayoutBuilderMixin<
    // Constraints, ..>` -- named nowhere else once the AOT dill has shaken
    // `createRenderObject`'s override (run645).
    built = true;
    for (final t in _constructedIn(cls)) {
      walk(substitution.substituteType(t));
    }
    built = false;
  }

  /// The generic class types a class's bodies construct, once per class.
  static final _constructedCache = <Class, List<InterfaceType>>{};

  static List<InterfaceType> _constructedIn(Class cls) =>
      _constructedCache.putIfAbsent(cls, () {
        final finder = _Constructed();
        for (final p in cls.procedures) {
          p.function.body?.accept(finder);
        }
        for (final c in cls.constructors) {
          c.function.body?.accept(finder);
          for (final i in c.initializers) {
            i.accept(finder);
          }
        }
        for (final f in cls.fields) {
          f.initializer?.accept(finder);
        }
        return finder.types;
      });

  /// A constructed type through the census (`_census`), its arguments
  /// unchanged: `Elem<L>(this)` names `Elem<L>` as a slot would.
  List<IrType> _censusOf(InterfaceType type, List<IrType> arguments) {
    _census(type);
    return arguments;
  }

  /// `dart:core`'s `List`, `Set`, `Map`: abstract there, values here.
  static bool _coreCollection(Class c) =>
      c.enclosingLibrary.importUri.toString() == 'dart:core' &&
      const {'List', 'Set', 'Map'}.contains(c.name);

  static bool _mentionsTypeParameter(DartType t) {
    if (t is FutureOrType) return _mentionsTypeParameter(t.typeArgument);
    if (t is RecordType) {
      return t.positional.any(_mentionsTypeParameter) ||
          t.named.any((n) => _mentionsTypeParameter(n.type));
    }
    if (t is TypeParameterType) return true;
    if (t is InterfaceType) return t.typeArguments.any(_mentionsTypeParameter);
    if (t is FunctionType) {
      return _mentionsTypeParameter(t.returnType) ||
          t.positionalParameters.any(_mentionsTypeParameter) ||
          t.namedParameters.any((n) => _mentionsTypeParameter(n.type));
    }
    return false;
  }

  /// Whether `Object`-bounded parameters a subclass fixes are erased too
  /// (see `_erasedParameter`). Off: ws318.
  final bool eraseObjectBounded;

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

  /// Whether a call to `target` yields a `Result` to propagate: a member
  /// of a translated library that is a function (a field's accessor is
  /// plain) and not `async` (its exceptions go into the future, and the
  /// `?` goes after the `.await`).
  ///
  /// Nothing here asks whether the callee can throw, and the name is the
  /// last of when it did. Under the uniform model every translated function
  /// returns `Result` whether or not anything in it can fail, so what a
  /// caller needs to know is only whether the callee is one of those. It is
  /// a question about the callee's *library and kind*, and it is answered
  /// from those.
  ///
  /// This opened with `if (throws == null) return false` until ws889, taking
  /// a `ThrowsAnalysis` the rest of the method never read. It cost two rounds
  /// of misdiagnosis: a driver that did not build the analysis emitted every
  /// signature as `Result` and propagated through none of them, and the
  /// reading -- twice, mine and a reviewer's -- was "the driver must build
  /// the analysis". It must not. There was no information in that argument,
  /// only a switch wearing an analysis's name, and a front end that does not
  /// mark its calls is wrong beside a backend whose `_resultModel` is a
  /// `const true`. `ThrowsAnalysis` itself is a census and lives in
  /// `bin/throws_census.dart`, which is the only thing that reads its answers.
  bool _fails(Member target) {
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
    // An operator keeps its std signature (`_emitOperator`: no `Result`),
    // so a call of one never propagates: `self[i] * a[i]` in `_Vector`'s
    // own `operator *` unwrapped a bare `f64` (73 at ws329).
    // ..a *std* operator; `[]` and the comparisons are methods here, and a
    // trait's `index_of` returned `Result` while the call did not `?` it
    // (20 `Rc<dyn Color> <= Option<..>` on `MaterialColor.shade50`, ws423).
    if (target is Procedure &&
        target.kind == ProcedureKind.Operator &&
        stdOperators.contains(target.name.text)) {
      return false;
    }
    // A member cloned into a mixin application lives in a synthetic
    // library: translated like the mixin's (`current_down` on
    // `_TapStatusTrackerMixin`, 40 calls without `?`).
    if (target.enclosingClass?.isAnonymousMixin ?? false) return true;
    return translatedLibrary(target.enclosingLibrary.importUri);
  }

  /// Top-level `dynamic` fields whose runtime types the driver worked out
  /// from the initialiser and every store into them (`dynamicSlotsIn`):
  /// `dateTimeSymbols` holds an `UninitializedLocaleData` and then a `Map`.
  /// A call on such a slot dispatches by downcast (`IrDynamicDispatch`).
  final Map<Field, List<InterfaceType>> dynamicSlots;

  /// The member names some part of the program reads or calls through a
  /// `dynamic`, with the classes that declare a member of that name
  /// (`dynamicMembersIn`).
  ///
  /// `demo.slug` on a `dynamic` names no struct, so nothing but the object
  /// itself can answer -- and the set of things it could *be* is the closed
  /// world's answer to "who declares `slug`". That is two classes here, and
  /// the dispatch is the same one a `dynamic` *slot* gets
  /// (`IrDynamicDispatch`), with the candidates found by member name rather
  /// than by which slot the value came out of.
  final Map<String, Set<Class>> dynamicMembers;

  /// The classes whose identity the program observes (`identityObservedIn`),
  /// whole program. A value class in here carries an identity token; see
  /// `IrClass.identityToken`.
  final Set<Class> identityObserved;

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
    // Per member: a copy lowered twice (into the mixin's trait under the
    // declaration's types, into the applying struct under its own) kept
    // the first lowering's parameter types for the second (ws491).
    _declaredParamTypes.clear();
    // A slot's expected return is consumed by the body it was set for; a
    // refusal before that left it for the next member (`RxStatus.loading`
    // returned `()`, ws478).
    _expectedReturn = null;
    final env = typeEnvironment;
    _typeContext = env == null ? null : StaticTypeContext(member, env);
    _capturedWrites = _CapturedWrites.of(member, _fillsParameter);
    _boxedFunctionLocals.clear();
    _tryWrites = _TryWrites.of(member);
  }

  /// The locals of the member being lowered that a closure inside it
  /// writes: `sum += v` inside a `forEach` callback. Each is declared as a
  /// shared cell (`IrLocalDecl.cell`), so the closure's write is the
  /// local's -- a plain `let` copied into the closure would have kept the
  /// sum to itself, and the fixture crate's `total` said 0.
  Set<Variable> _capturedWrites = const {};

  /// The function-typed locals of the member being lowered that are
  /// handles (`Rc<..>`): local functions, and locals with a closure
  /// initialiser (see `_withBorrowing`).
  final Set<String> _boxedFunctionLocals = {};

  /// Set while a local function whose binding never escapes is lowered:
  /// its closure may borrow `this` (see `IrLocalFunction.lends`).
  bool _lendingLocal = false;

  /// Locals assigned inside a `try` body they are declared outside of.
  /// The backend lowers a `try` into a closure called on the spot, and
  /// Rust will not let a closure assign a binding that is not yet
  /// initialized: such a local starts as `None` and is read unwrapped
  /// (`late Widget built; try { built = build(); } ..` in
  /// `ComponentElement.performRebuild`, ws475).
  Set<Variable> _tryWrites = const {};

  DartType? _staticType(Expression e) {
    // A copy's parameter is typed by its declaration (`_declaredParamTypes`):
    // the mixin's `S`, erased to its bound, not the `Dialog` this
    // application put in. Typed by the copy, `oldWidget.label` in the
    // mixin's super body downcast the `Rc<dyn Widget>` it holds to the one
    // application's class and panicked on every other (the unapply
    // fixture, ws536).
    if (e is VariableGet && e.promotedType == null) {
      final declaredAs = _declaredParamTypes[e.variable];
      if (declaredAs != null) return declaredAs;
      // A closure parameter retyped by the slot it fills (`_retype`): a
      // `(locale) => f(locale)` in a `String Function(String)` list reads
      // as the `String` it is, not the `dynamic` it was written as
      // (intl's `verifiedLocale`, ws592).
      final retyped = _retyped[e.variable];
      if (retyped != null) return retyped;
    }
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
  final Map<Class, Map<String, Map<String, Constant>>> enumFields;
}
