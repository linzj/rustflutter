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
import 'throws.dart';

import 'dart:io' show Platform, stderr;

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
    this.throws,
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

  /// `dynamic` anywhere in a type: the prelude spells it as translated
  /// code does (`Map<String, dynamic>` is `Map<String, Rc<dyn Object>>`),
  /// so such a slot of a prelude callee is coerced too (`Uri.replace(
  /// queryParameters: uri.queryParametersAll)`, 16 at ws421).
  static bool _mentionsDynamic(DartType t) {
    if (t is FutureOrType) return _mentionsDynamic(t.typeArgument);
    if (t is RecordType) {
      return t.positional.any(_mentionsDynamic) ||
          t.named.any((n) => _mentionsDynamic(n.type));
    }
    if (t is DynamicType) return true;
    if (t is InterfaceType) return t.typeArguments.any(_mentionsDynamic);
    if (t is FunctionType) {
      return _mentionsDynamic(t.returnType) ||
          t.positionalParameters.any(_mentionsDynamic) ||
          t.namedParameters.any((n) => _mentionsDynamic(n.type));
    }
    return false;
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
  final Map<Class, Map<String, Map<String, String>>> enumFields;
  String? _superclass;

  // -- Types ------------------------------------------------------------------

  /// How deep inside a type `_type` is: a `T?` *inside* a type -- a type
  /// argument, a function type's parameter or result -- is spelled
  /// projected (`<T as DartNullable>::Or`), the way a signature's is, so
  /// that `WidgetStateProperty<T?>` with `T` bound to `Color?` is one
  /// trait object type on both sides. A bare `T?` at the top is the
  /// `Option<T>` a body works with; `_edgeType` projects that one in
  /// signatures.
  var _typeDepth = 0;

  T _nested<T>(T Function() inside) {
    _typeDepth++;
    try {
      return inside();
    } finally {
      _typeDepth--;
    }
  }

  /// `_type`, as a type argument: a `T?` here is projected.
  IrType _typeNested(DartType type) => _nested(() => _type(type));

  IrType _type(DartType type) {
    final nullable = type.nullability == Nullability.nullable;
    // An applied mixin body's concrete argument back to the mixin's own
    // erased parameter (`_appliedBack`), which spells as its bound.
    if (_appliedBack.isNotEmpty && type is InterfaceType) {
      final back =
          _appliedBack[type.withDeclaredNullability(Nullability.nonNullable)];
      if (back != null) {
        // Once: the parameter spells as its bound, and a bound that
        // mentions a mapped type would send this straight back here
        // (a stack overflow in the front end, ws741).
        final was = _appliedBack;
        _appliedBack = const {};
        try {
          return _type(
            nullable
                ? back.withDeclaredNullability(Nullability.nullable)
                : back,
          );
        } finally {
          _appliedBack = was;
        }
      }
    }
    // An extension type is its representation type at runtime -- Dart
    // erases it -- and so it is here (`BaselineOffset(double? offset)`:
    // `RenderBoxContainerDefaultsMixin.defaultComputeDistanceToHighest
    // ActualBaseline` was refused whole, and `RenderFlex`'s baseline with
    // it, ws525).
    if (type is ExtensionType) {
      final erased = type.extensionTypeErasure;
      return _type(
        nullable
            ? erased.withDeclaredNullability(Nullability.nullable)
            : erased,
      );
    }
    if (type is InterfaceType) {
      // `dart:core`'s `Iterator` would shadow `std::iter::Iterator` in every
      // module: it is the prelude's `DartIterator`.
      final core =
          type.classNode.enclosingLibrary.importUri.toString() == 'dart:core';
      // `Object?` is `dynamic`: Dart's two top types are one type to its
      // subtyping (`LocalizationsDelegate<dynamic>` and
      // `LocalizationsDelegate<Object?>` are the same type, `WidgetsApp.
      // build`, ws497), and one representation here -- a `dynamic` holds
      // its null as the `Null` object. Everywhere, not only as a type
      // argument: an expression's Rust type follows its Dart static type,
      // and `m[k]` on a `Map<Object?, Object?>` is typed `Object?` by the
      // substitution Kernel already did.
      if (core && type.classNode.name == 'Object' && nullable) {
        return const IrType('dynamic');
      }
      final name = core && type.classNode.name == 'Iterator'
          ? 'DartIterator'
          : type.classNode.name;
      _census(type);
      // A class that *is* a `Future` (implements `dart:async`'s): the
      // prelude's future, since that is what every `Future<T>` slot holds
      // (`SynchronousFuture<T>`, ws482).
      if (_futureLike(type.classNode) && type.typeArguments.length == 1) {
        return IrType(
          'Future',
          nullable: nullable,
          arguments: _nested(() => [_type(type.typeArguments.single)]),
        );
      }
      return IrType(
        name,
        nullable: nullable,
        arguments: _erasedArguments(type.classNode, type.typeArguments),
        module: _moduleQualifier(type.classNode),
      );
    }
    if (type is RecordType) {
      // A named field is a tuple field too, after the positional ones and
      // in the record type's own order -- Kernel asserts that order is
      // lexicographic, which is Dart's canonical order for named fields, so
      // two spellings of one record type give one tuple
      // (`({OverlayEntry start, OverlayEntry end})? _handles` of
      // `SelectionOverlay`, 6 stubs and 5 refusals at ws751).
      return IrType(
        'Record',
        nullable: nullable,
        arguments: _nested(
          () => [
            for (final f in type.positional) _type(f),
            for (final n in type.named) _type(n.type),
          ],
        ),
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
        final asBound = _typeOfBound(type.parameter.bound);
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
      // Not `num`: `_RestorablePrimitiveValue<T extends num>` as an `f64`
      // took `RestorableInt`'s `i64`s in and gave `f64`s out (51 at
      // ws354); `T` stays the caller's type.
      final bound = type.parameter.bound;
      if (bound is InterfaceType &&
          const {
            'String',
            'int',
            'double',
            'bool',
          }.contains(bound.classNode.name)) {
        // The bound's own nullability comes along: intl's `T extends
        // String?` was a `String` here, and its `String?` return a
        // `String` (40 `Option<String>` <- `String`).
        return IrType(
          _typeOfBound(bound).name,
          nullable: nullable || bound.nullability == Nullability.nullable,
        );
      }
      // `T extends Iterable<E>`: the `Vec<E>` the bound is, since the body
      // iterates it (collection's `IterableEquality`, 6).
      if (bound is InterfaceType &&
          const {'Iterable', 'List'}.contains(bound.classNode.name)) {
        final asBound = _typeOfBound(bound);
        return IrType(
          asBound.name,
          nullable: nullable,
          arguments: asBound.arguments,
        );
      }
      return IrType(
        type.parameter.name ?? 'T',
        nullable: nullable,
        // ..of the declaration being lowered only: another declaration's
        // `T?` -- a callee's, reached before its instantiation is put in --
        // is not a name here, projected or not (24 `cannot find type`).
        projected: nullable && _typeDepth > 0 && _projectedSlot(type),
      );
    }
    // A generic function *type*'s own parameter (`E Function<E>(E)`),
    // spelled at its bound: a `dyn Fn` has no type parameters of its own,
    // and a value of the type is used at the bound (`HeapPriorityQueue`'s
    // comparator in `SchedulerBinding`, which refused the whole
    // `WidgetsFlutterBinding` constructor, run432).
    if (type is StructuralParameterType) {
      final bound = type.parameter.bound;
      return _type(
        nullable ? bound.withDeclaredNullability(Nullability.nullable) : bound,
      );
    }
    if (type is FunctionType && type.typeParameters.isNotEmpty) {
      return _type(
        FunctionTypeInstantiator.instantiate(type, [
          for (final p in type.typeParameters) p.bound,
        ]),
      );
    }
    if (type is FunctionType) {
      // Named parameters after the positional ones, **sorted by name**, as
      // a closure declares them: `LogWriterCallback = void Function(String
      // text, {bool isError})` is an `Fn(String, bool)`, and a field of
      // that type could not hold the two-parameter function (E0593).
      final named = [...type.namedParameters]
        ..sort((a, b) => a.name.compareTo(b.name));
      return _nested(
        () => IrType.function(
          [
            for (final p in type.positionalParameters) _paramType(p),
            for (final p in named) _paramType(p.type),
          ],
          _type(type.returnType),
          nullable: nullable,
        ),
      );
    }
    // `FutureOr<T>` is "a `T`, or a future of one": the prelude's enum of
    // the two, awaitable either way; a value crosses into it through
    // `FutureOr::value` / `FutureOr::future` (`coerceInto`). The gallery's
    // startup path needs it: `Future<bool>(() async {..})` in
    // `GetStorage._internal`, `SchedulerBinding.scheduleTask`.
    // Nullable only as *declared* (`FutureOr<int>?`): Dart computes
    // `FutureOr<void>` and `FutureOr<T?>` nullable from the argument,
    // whose null the `T` inside already carries here -- as an `Option`
    // around the whole, a void `then<void>` callback returned `None` and
    // the adapter into the `FutureOr<()>` slot unwrapped it (`Route.
    // didAdd` through `TickerFuture.then`, run652).
    if (type is FutureOrType) {
      return IrType(
        'FutureOr',
        nullable: type.declaredNullability == Nullability.nullable,
        arguments: [_type(type.typeArgument)],
      );
    }
    throw Unsupported('the type `$type`', '$type');
  }

  // -- Expressions ------------------------------------------------------------

  /// Every lowered expression knows its Rust type (`IrExpr.rustType`):
  /// what the lowering said when it knew better, and Dart's static type
  /// mapped through `_type` otherwise. A slot's coercion (`coerce`) reads
  /// this rather than re-deriving the value's shape at each site.
  IrExpr expression(Expression node) {
    var lowered = _expressionRaw(node);
    // A member read the type flow analysis narrowed past its declared
    // null (`widget.builder(..)` under `if (widget.builder != null)`, the
    // `!` rewritten away in an AOT dill): the Rust member is still the
    // `Option` it was declared, and the read unwraps it -- the proof,
    // spelled, as an argument's is (`_widenedInto`). A local's promotion
    // is handled where it is read (`_localRead`); a projected `T?` has no
    // `Option` to unwrap.
    if (node is InstanceGet ||
        node is InstanceInvocation ||
        node is StaticGet ||
        node is StaticInvocation) {
      final declared = _declaredTypeOf(node);
      // The node's *own* recorded type, which is where the analysis
      // writes its narrowing (`getStaticType` recomputes the declared).
      final narrowed = switch (node) {
        InstanceGet(:final resultType) => resultType,
        InstanceInvocation(:final functionType) => functionType.returnType,
        _ => _staticType(node),
      };
      if (declared != null &&
          declared is! TypeParameterType &&
          declared.nullability == Nullability.nullable &&
          narrowed != null &&
          narrowed is! DynamicType &&
          narrowed.nullability != Nullability.nullable &&
          lowered is! IrNullCheck) {
        // ..whatever the read's recorded type says: it follows the
        // narrowing, the Rust member does not.
        final declaredIr = _recordedType(declared);
        if (declaredIr != null && isNullable(declaredIr)) {
          lowered = IrNullCheck(lowered)..rustType = _recordedType(narrowed);
        }
      }
    }
    // Arithmetic is typed by its operands, not by Dart: the `double?` Dart
    // gives an inlined `lerpDouble` is an `f64` here (69 `unwrap` on an
    // `f64` at ws357).
    if (lowered is IrBinary && lowered.rustType == null) {
      lowered.rustType = _binaryType(lowered);
    }
    // A null check is its operand's type without the `Option`: Dart's
    // static type for `data.nextSibling!` is the clone's `RenderBox`, the
    // operand an erased `RenderObject?` (108 at ws380).
    if (lowered is IrNullCheck && lowered.rustType == null) {
      final inner = lowered.operand.rustType;
      if (inner != null) lowered.rustType = _nonNull(inner);
    }
    // A call or read whose declared type is the *declaring class's* type
    // parameter, through a receiver that puts a `dynamic` there: the value
    // is that `dynamic` whatever Kernel's substitution says. Recorded even
    // over a type the lowering already put on, because that type is the
    // lie (`_throughReceiver`).
    final erasedThrough = _erasureOff
        ? null
        : switch (node) {
            InstanceInvocation(:final interfaceTarget, :final receiver) =>
              _throughReceiver(
                receiver,
                interfaceTarget,
                interfaceTarget.function?.returnType,
              ),
            InstanceGet(:final interfaceTarget, :final receiver) =>
              _erasedRead(interfaceTarget, interfaceTarget.getterType) ??
                  _throughReceiver(
                    receiver,
                    interfaceTarget,
                    interfaceTarget.getterType,
                  ),
            _ => null,
          };
    if (erasedThrough != null) lowered.rustType = erasedThrough;
    if (lowered.rustType == null) {
      final static = _staticType(node);
      // A member declared `T?` with `T` bound to a top type reads as the
      // `Option<Rc<dyn Object>>` Rust's `Or` is for a handle, not as the
      // bare `dynamic` Kernel's substitution wrote (`_imageStream?.key`,
      // `raw[id]` on a `Map<String, Object?>`, ws499); `coerce` converts
      // it into a `dynamic` slot from there.
      final projected = switch (node) {
        InstanceInvocation(:final interfaceTarget) =>
          _topBound(interfaceTarget.function?.returnType, static) ??
              _erasedResult(interfaceTarget.function?.returnType),
        InstanceGet(:final interfaceTarget) =>
          _topBound(interfaceTarget.getterType, static) ??
              _erasedResult(interfaceTarget.getterType),
        _ => null,
      };
      // A generic callee's `T?` result, instantiated with a type
      // parameter of the code here, arrives as the callee's edge spells
      // it -- `<T as DartNullable>::Or` -- not as the body's `Option<T>`:
      // typed as the edge, so the coercion into a body slot converts it
      // (`dependOnInheritedWidgetOfExactType<T>()` returned into
      // `inheritFrom<T>`'s `T?`, wrapped in a `from_option` that took an
      // `Option`, ws543).
      // ..and a `super.m<T>()` reaching a generic base method is the
      // same edge (`super.get<T>()` returning `T?` was wrapped in a
      // `from_option` that took an `Option`, the gentrait fixture).
      final declaredReturn = switch (node) {
        InstanceInvocation(:final interfaceTarget) =>
          interfaceTarget.function?.returnType,
        StaticInvocation(:final target) => target.function.returnType,
        SuperMethodInvocation(:final interfaceTarget) =>
          interfaceTarget.function.returnType,
        _ => null,
      };
      final calleeParams = switch (node) {
        InstanceInvocation(:final interfaceTarget) =>
          interfaceTarget.function?.typeParameters ?? const <TypeParameter>[],
        StaticInvocation(:final target) => target.function.typeParameters,
        SuperMethodInvocation(:final interfaceTarget) =>
          interfaceTarget.function.typeParameters,
        _ => const <TypeParameter>[],
      };
      final edgeResult =
          declaredReturn is TypeParameterType &&
          declaredReturn.nullability == Nullability.nullable &&
          calleeParams.contains(declaredReturn.parameter) &&
          !_erasedParameter(declaredReturn.parameter) &&
          static is TypeParameterType &&
          static.nullability == Nullability.nullable &&
          _projectedSlot(static);
      // ..and a callee that hands back its *own* element -- a container's
      // `E`, not an `E?` -- hands back whatever its type argument was
      // spelled with, and a type argument is nested: `_options.elementAt
      // (i)` on an `Iterable<T?>` arrives as `<T as DartNullable>::Or`,
      // the slot `RadioListTile<T?>.value` is, and a `from_option` that
      // took an `Option` stubbed `_SettingsListItemState.build` (ws691).
      // Whose parameter it is does not matter: if the declared result is
      // the bare parameter and the result here is nullable, the argument
      // put in for it was nullable and is spelled projected. An `E?` is
      // *not* the same -- a prelude container spells its own `V?` as a
      // real `Option<V>`, one layer more than the argument.
      final elementResult =
          declaredReturn is TypeParameterType &&
          declaredReturn.nullability != Nullability.nullable &&
          !_erasedParameter(declaredReturn.parameter) &&
          !_spelledAsBound(declaredReturn.parameter) &&
          static is TypeParameterType &&
          static.nullability == Nullability.nullable &&
          _projectedSlot(static);
      // An `async` function returns the future it spawns, whatever it
      // was declared: `Future<flatten(R)>`, so `FutureOr<void> f() async`
      // hands back a `DartFuture<()>`, not a `FutureOr` (`_sendFontChange
      // Message` into `then`, run607). Static calls here; `_qualified`
      // does the same for instance ones.
      final spawned = node is StaticInvocation && _asyncMember(node.target)
          ? _spawnedFuture(static)
          : null;
      if (spawned != null) {
        lowered.rustType = spawned;
      } else if (projected != null) {
        lowered.rustType = projected;
      } else if (static != null) {
        try {
          lowered.rustType = edgeResult || elementResult
              ? _typeNested(static)
              : _type(static);
        } on Unsupported {
          // A type this compiler has no spelling for: the node stays
          // untyped, and a coercion into a slot falls back to the shape
          // rules.
        }
      }
    }
    return lowered;
  }

  /// The `Future<T>` an `async` member declared `FutureOr<T>` or
  /// `Future<T>?` actually returns (Dart's `flatten`); null when the
  /// declaration is a plain `Future<T>` already, or cannot be spelled.
  IrType? _spawnedFuture(DartType? declared) {
    final flattened =
        declared is FutureOrType ||
            (declared is InterfaceType &&
                declared.classNode.name == 'Future' &&
                declared.nullability == Nullability.nullable)
        ? _awaitedType(declared)
        : null;
    if (flattened == null) return null;
    try {
      return IrType('Future', arguments: [_type(flattened)]);
    } on Unsupported {
      return null;
    }
  }

  /// `dynamic?`, the `Option<Rc<dyn Object>>` a nullable type parameter is
  /// once bound to a top type (`Object?`, `dynamic`): what the Rust side
  /// holds for it (`<Rc<dyn Object> as DartNullable>::Or`), where the
  /// substituted Dart type says only `Object?` -- a `dynamic` here. Null
  /// for any other declared type or binding.
  /// A member whose declared result is an *erased* type parameter hands
  /// back the bound it was erased to, whatever the static type says: a
  /// `WidgetStateProperty<bool>.resolve(states)` is a `bool` to Dart and
  /// an `Rc<dyn Object>` here, and the coercion into the slot reads it
  /// back (`_MaterialScrollbar._thickness`, ws704).
  /// A condition is a `bool`: whatever the value in hand is spelled as --
  /// the `Rc<dyn Object>` an erased result hands back, say -- it goes in
  /// through the one coercion rule (`_MaterialScrollbar._thickness`,
  /// ws704).
  IrExpr _condition(Expression condition) =>
      coerce(expression(condition), const IrType('bool'));

  /// The result of a member whose declared type is the *declaring class's*
  /// type parameter, read through a receiver that puts a `dynamic` there.
  ///
  /// Kernel's substitution says `T`, because Dart kept the argument; this
  /// output erased it, so what comes back is the `Rc<dyn Object>` the
  /// erased slot holds. Typed as that, the coercion into the slot converts
  /// it -- untyped, `item.tween.transform(t)` on a `TweenSequenceItem`
  /// whose `T` is erased returned an `Rc<dyn Object>` where the function
  /// says `T` (`TweenSequence._evaluateAt`, the gallery's page transition,
  /// run765).
  IrType? _throughReceiver(
    Expression receiver,
    Member target,
    DartType? declared,
  ) {
    if (declared is! TypeParameterType) return null;
    // Strictly non-null: `Map<K, V>.[]` returns `V?`, which Kernel writes
    // `V%` -- *undetermined*, because `V`'s bound is nullable -- and the
    // top-bound rule already spells that `Option<Rc<dyn Object>>`. Let
    // through, the read lost its `Option` and every null-aware read around
    // it stopped compiling: round 3 at 336 against 246 with the rules off
    // (ws765 through ws768).
    if (declared.nullability == Nullability.nullable) return null;
    final owner = target.enclosingClass;
    final env = typeEnvironment;
    if (owner == null || env == null) return null;
    final at = owner.typeParameters.indexOf(declared.parameter);
    if (at < 0) return null;
    // The receiver as *this output* records it, not as Kernel wrote it: a
    // field whose own class erased a parameter reads at the erased
    // spelling, and Kernel's substitution put the caller's `T` there
    // (`element.tween` on a `TweenSequenceItem<T>`, run765).
    IrType? recorded;
    if (receiver is InstanceGet) {
      recorded = _erasedRead(
        receiver.interfaceTarget,
        receiver.interfaceTarget.getterType,
      );
    }
    if (recorded == null) {
      final receiverType = _staticType(receiver);
      if (receiverType is! InterfaceType) return null;
      final asOwner = env.hierarchy.getTypeAsInstanceOf(receiverType, owner);
      if (asOwner is! InterfaceType || at >= asOwner.typeArguments.length) {
        return null;
      }
      try {
        recorded = IrType(
          owner.name,
          arguments: [for (final a in asOwner.typeArguments) _typeNested(a)],
        );
      } on Unsupported {
        return null;
      }
    }
    if (at >= recorded.arguments.length) return null;
    final put = recorded.arguments[at];
    // Only where the erasure really put a `dynamic` there: anything the
    // output can still name is what the value is.
    if (put.name != 'dynamic') return null;
    return const IrType('dynamic');
  }

  /// A read whose declared type *mentions* a type parameter this output
  /// erased on the declaring class: the value is spelled the way the struct
  /// spells it, with the erased parameter at its bound.
  ///
  /// `TweenSequenceItem<T>` loses its `T`, so `item.tween` is an
  /// `Rc<dyn Animatable<Rc<dyn Object>>>` -- and Kernel's substitution says
  /// `Animatable<TweenSequence.T>`, which is what the caller wrote and not
  /// what is there (`TweenSequence._evaluateAt`, run765).
  /// A bisect switch: `DART2RUST_ERASURE_OFF=1` turns the two rules below
  /// off, to say whether a round's cost is theirs.
  static final bool _erasureOff =
      Platform.environment['DART2RUST_ERASURE_OFF'] == '1';

  /// Whether the *shape* of a lowering says the value is out of its
  /// `Option`, whatever type was recorded for it.
  ///
  /// A null check, a downcast and a cast all hand back the value itself;
  /// only a `Some` (and a plain read of a nullable place) is still in the
  /// `Option`. `_widenedInto` asks this where nothing recorded a type --
  /// `a!.dart_cast_any::<Rc<X>>()` reaching `_handleOf` records none, and
  /// treating that as "may already be an Option" left the `Some` off at
  /// every `lerp` (`BoxBorder.lerp`, ws772).
  static bool _unwrapped(IrExpr e) => switch (e) {
    IrNullCheck() => true,
    IrDowncast() => true,
    IrCastTo() => true,
    IrNew() => true,
    IrCall(:final target, :final name, :final args)
        when name == 'clone' && args.isEmpty && target != null =>
      _unwrapped(target),
    _ => false,
  };

  IrType? _erasedRead(Member target, DartType? declared) {
    if (_erasureOff) return null;
    if (declared == null || declared is TypeParameterType) return null;
    final owner = target.enclosingClass;
    if (owner == null) return null;
    // Only where the class kept *none* of them: the struct then has no
    // type parameters at all and every mention is at the bound, which is
    // what makes the read's spelling unambiguous. A class that kept some
    // still names them, and reading at the bound there was 87 more errors
    // in one round (ws765).
    final erased = owner.typeParameters.where(_erasedParameter).toList();
    if (erased.isEmpty ||
        erased.length != owner.typeParameters.length ||
        !_mentionsParametersOf(declared, erased)) {
      return null;
    }
    try {
      return _typeNested(declared);
    } on Unsupported {
      return null;
    }
  }

  IrType? _erasedResult(DartType? declared) {
    if (declared is! TypeParameterType) return null;
    if (!_erasedParameter(declared.parameter)) return null;
    try {
      return _type(declared);
    } on Unsupported {
      return null;
    }
  }

  IrType? _topBound(DartType? declared, DartType? substituted) {
    // Through a `Future`: `invokeMethod<T>` returns `Future<T?>`, and the
    // rule below reads its `T?` as the `Option<Rc<dyn Object>>` a handle's
    // `Or` is. Stopping at the `Future` recorded a bare `Future<dynamic>`,
    // and the erased twin's cast then asked for `DartFuture<Rc<dyn
    // Object>>` where the twin hands back `DartFuture<Option<..>>`
    // (`DefaultProcessTextService.queryTextActions`, the run's own panic
    // at run773).
    if (declared is InterfaceType &&
        declared.classNode.name == 'Future' &&
        declared.typeArguments.length == 1 &&
        substituted is InterfaceType &&
        substituted.classNode.name == 'Future' &&
        substituted.typeArguments.length == 1) {
      final inner = _topBound(
        declared.typeArguments.single,
        substituted.typeArguments.single,
      );
      return inner == null ? null : IrType('Future', arguments: [inner]);
    }
    if (declared is! TypeParameterType ||
        declared.nullability != Nullability.nullable ||
        _erasedParameter(declared.parameter)) {
      return null;
    }
    final top =
        substituted is DynamicType ||
        (substituted is InterfaceType &&
            substituted.classNode.name == 'Object' &&
            substituted.classNode.enclosingLibrary.importUri.toString() ==
                'dart:core' &&
            substituted.nullability == Nullability.nullable);
    return top ? const IrType('dynamic', nullable: true) : null;
  }

  /// Dart's `toString()` of `lowered`, typed `type`, as a `String`: the
  /// `DartAny` protocol for a translated class (`!dart_to_string`: its
  /// own override, an enum's `X.value`, `Instance of` otherwise), `null`
  /// or that for a nullable one, and the object's own answer
  /// (`dart_object_str`: the registry, the core values) for a `dynamic`,
  /// an `Object`, a type parameter, a core value this lowering does not
  /// spell. Under `explicit` -- an `x.toString()` call -- a `String`
  /// or number receiver is left to the ordinary call and null is
  /// returned. `dart_str` (Rust's `Debug`) printed `Some(1)`, `Type {
  /// name: .. }` and `Size { _width: .. }` where Dart says `1`, `Size`
  /// and `Size(800.0, 600.0)` (ws543).
  IrExpr? _stringOf(IrExpr lowered, DartType? type, {bool explicit = false}) {
    const text = IrType('String');
    IrExpr nullOr(IrExpr inner) => IrIfNull(
      IrNullAware(lowered, inner)
        ..rustType = const IrType('String', nullable: true),
      IrLiteral('"null".to_string()', const IrType('raw'))..rustType = text,
      nullableResult: false,
      eager: true,
    )..rustType = text;
    if (type is InterfaceType) {
      final node = type.classNode;
      final core = node.enclosingLibrary.importUri.toString() == 'dart:core';
      final nullable = type.nullability == Nullability.nullable;
      if (_translatedClass(node) && !core) {
        final own = IrCall(IrBound(), '!dart_to_string', const [])
          ..rustType = text;
        if (!nullable) {
          return IrCall(lowered, '!dart_to_string', const [])..rustType = text;
        }
        return nullOr(own);
      }
      // A `List`/`Set`: `[a, b]` / `{a, b}`, each element by this rule
      // (`join`'s element rule in the backend); a nullable one `null` or
      // that.
      if (core && const {'List', 'Set'}.contains(node.name)) {
        final open = node.name == 'List' ? '[' : '{';
        final close = node.name == 'List' ? ']' : '}';
        IrExpr joined(IrExpr list) => IrInterpolation([
          IrLiteral(open, const IrType('String')),
          IrCall(list, '!join', [IrLiteral(', ', const IrType('String'))])
            ..rustType = text,
          IrLiteral(close, const IrType('String')),
        ])..rustType = text;
        if (!nullable) return joined(lowered);
        return nullOr(
          joined(
            IrBound()
              ..rustType = _typeNested(
                type.withDeclaredNullability(Nullability.nonNullable),
              ),
          ),
        );
      }
      if (core &&
          const {'String', 'int', 'double', 'bool'}.contains(node.name)) {
        if (!nullable) return explicit ? null : lowered;
        // An `int?`/`bool?` through the Object protocol, as any other
        // value: the bound may be the erased handle a `Tween<int>.end`
        // is held as, whose `Debug` (`dart_str`) says `Instance of 'int'`
        // (the traitset fixture).
        final inner = switch (node.name) {
          'String' => IrCall(IrBound(), 'clone', const [])..rustType = text,
          'double' => IrStaticCall(null, 'dart_double_str', [
            IrBound(),
          ])..rustType = text,
          _ => IrCall(IrBound(), '!object_str', const [])..rustType = text,
        };
        return nullOr(inner);
      }
    }
    // A value the AOT compiler removed (`!`) is its own text: a call on
    // it would ask `!: DartAny` (11 "never type fallback" at ws545).
    // ..through `dart_str` (a `Debug` bound, which the `()` the block
    // falls back to has; bare, `{}` asked `Display` of it: 11 at ws546).
    if (lowered.rustType?.name == 'Never') {
      return IrStaticCall(null, 'dart_str', [lowered])..rustType = text;
    }
    return IrCall(lowered, '!object_str', const [])..rustType = text;
  }

  /// Dart's `null`, as the lowering writes it on its own -- an omitted
  /// argument, an uninitialised local, a constant -- *typed*, so that the
  /// coercion into its slot sees it: untyped, an omitted `Object? aspect`
  /// stayed `None` where the `Null` object went (85 at ws502).
  static IrExpr _nullLiteral() =>
      IrLiteral('null', const IrType('Null', nullable: true))
        ..rustType = const IrType('Null', nullable: true);

  /// `x!`, and every unwrap the lowering adds on Dart's word that a value
  /// is nullable: the value itself when its recorded Rust type is not an
  /// `Option` -- a `dynamic`, an `Object?` (one type here, see `_type`),
  /// arithmetic the type flow analysis typed non-nullable and Kernel still
  /// writes `double?` for (`lerpDouble`, ws331) -- and the unwrap
  /// otherwise. An operand with no recorded type is unwrapped as Dart says.
  IrExpr _nullChecked(IrExpr inner, [Expression? operand]) {
    // Unwrapped once: the read may have been already (`expression`).
    if (inner is IrNullCheck) return inner;
    // By the operand's *declared* type first: an AOT dill's type flow
    // analysis narrows `widget.builder` to non-null under `if (widget.
    // builder != null)`, and the recorded type follows it, while the Rust
    // field is the `Option` it was declared (`WidgetsApp.build`, ws506).
    final declared = operand == null ? null : _declaredTypeOf(operand);
    final declaredIr = declared == null ? null : _recordedType(declared);
    // Typed as the operand without its `Option`: a downcast of the checked
    // value asks a handle for its `Any` only when it knows it holds one
    // (`old.width` after `Painter? old` promoted to `Caret`, ws591).
    IrNullCheck checked() {
      final have = inner.rustType;
      return IrNullCheck(inner)
        ..rustType = have == null ? null : _nonNull(have);
    }

    if (declaredIr != null) {
      return isNullable(declaredIr) ? checked() : inner;
    }
    final have = inner.rustType;
    if (have != null && !isNullable(have)) return inner;
    return checked();
  }

  /// The type a member or variable was *declared* with, for the read
  /// `e` is of one: what the Rust side holds, whatever the flow analysis
  /// narrowed the read to. Null for any other expression.
  DartType? _declaredTypeOf(Expression e) {
    if (e is InstanceGet) return e.interfaceTarget.getterType;
    if (e is VariableGet) return e.variable.type;
    if (e is InstanceInvocation) return e.interfaceTarget.function?.returnType;
    if (e is StaticGet) return e.target.getterType;
    if (e is StaticInvocation) return e.target.function.returnType;
    return null;
  }

  /// A declared type as recorded on a value: `null` where this compiler
  /// has no spelling for it.
  IrType? _recordedType(DartType? declared) {
    if (declared == null) return null;
    try {
      return _type(declared);
    } on Unsupported {
      return null;
    }
  }

  static IrType? _binaryType(IrBinary b) {
    const comparisons = {'==', '!=', '<', '>', '<=', '>=', '&&', '||'};
    if (comparisons.contains(b.op)) return const IrType('bool');
    // Operands the lowering built itself (the inlined `lerpDouble`'s
    // `(b - a) * t`) never passed through `expression`: typed here first.
    final left = b.left;
    if (left is IrBinary && left.rustType == null) {
      left.rustType = _binaryType(left);
    }
    final right = b.right;
    if (right is IrBinary && right.rustType == null) {
      right.rustType = _binaryType(right);
    }
    final l = b.left.rustType?.name;
    final r = b.right.rustType?.name;
    if (l == null || r == null) return null;
    const numbers = {'int', 'double', 'num'};
    if (!numbers.contains(l) || !numbers.contains(r)) return null;
    if (b.op == '~/') return const IrType('int');
    if (b.op == '/' || l != 'int' || r != 'int') return const IrType('double');
    return const IrType('int');
  }

  IrExpr _expressionRaw(Expression node) {
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
      return _nullLiteral();
    }
    if (node is ThisExpression) return IrThis();
    if (node is VariableGet) {
      // `cosmeticName` is Kernel's word for the name a human wrote; a variable
      // the CFE invented has none, and one whose name starts with `#` is a
      // temporary from its own lowering.
      final bound = _bound;
      if (bound != null && node.variable == bound) {
        // The receiver's element as the body's static type names it: a
        // `child?.getDryLayout(..)` on a `child` the trait types
        // `RenderObject?` and the body `RenderBox?` (a mixin's `T extends
        // RenderBox`) reaches `RenderBox` through the object (ws476).
        final wanted = node.promotedType ?? node.variable.type;
        final have = _boundType;
        if (wanted is InterfaceType &&
            have != null &&
            have.name != wanted.classNode.name &&
            _abstractLike(wanted.classNode) &&
            !_scalarClass(wanted.classNode) &&
            _translatedClass(wanted.classNode) &&
            isBelow(wanted.classNode.name, have.name)) {
          // ..typed as the cast's own value, not Kernel's `RenderBox?`:
          // the cascade's `=>#t3` is the bound, never doubled, and the
          // untyped cast took the static type and a `.flatten()` with it
          // (`RenderProxyBoxMixin.performLayout` again, run657).
          final narrowed = _type(
            wanted.withDeclaredNullability(Nullability.nonNullable),
          );
          return IrCastTo(IrBound(), narrowed)..rustType = narrowed;
        }
        // Typed as the value the body binds: the receiver without its
        // `Option`. A `?..` cascade produces the bound (`=>#t3`, which
        // the CFE leaves unpromoted), and typed by Kernel's `RenderBox?`
        // the block got a `.flatten()` on a value that was never doubled
        // (`RenderProxyBoxMixin.performLayout`, `getTransformTo`, ws537).
        return IrBound()..rustType = have;
      }
      if (_cascade != null && node.variable == _cascade) {
        return _cascadeRead();
      }
      // A `let` temporary standing for a place its body mutates (see
      // `_let`): the place itself.
      final aliased = _letAliases[node.variable];
      if (aliased != null) return expression(aliased);
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
      if (_optionLocals.contains(node.variable)) {
        // ..a `late` local too: assigned on some path, read as the value
        // (`late double primaryDeltaFromDragStart` set in a `switch`,
        // `RawScrollbarState._getPrimaryDelta`, run666).
        if ((_tryWrites.contains(node.variable) || node.variable.isLate) &&
            node.variable.type.nullability != Nullability.nullable) {
          return IrCall(
            IrCall(IrLocal(name), 'clone', const []),
            'unwrap',
            const [],
          )..rustType = _type(node.variable.type);
        }
        return IrLocal(name)..rustType = _localIrType(node.variable);
      }
      // A constructor's projected parameter is read as it was declared,
      // the spelled `T?`: a constructor has no body prologue to re-bind
      // it, and its field initialisers store it as it is.
      final declaring = node.variable.parent;
      if (declaring is FunctionNode &&
          declaring.parent is Constructor &&
          _projectedSlot(node.variable.type)) {
        return IrLocal(name)..rustType = _edgeType(node.variable.type);
      }
      // A closure parameter retyped to an erased bound (`_closureParamType`)
      // reads as the class it was declared with.
      final declaredAs = _declaredParamTypes[node.variable];
      final retyped = _retyped[node.variable];
      final declaredVar = node.variable.type;
      if (retyped is TypeParameterType &&
          _erasedParameter(retyped.parameter) &&
          declaredVar is InterfaceType &&
          declaredVar.nullability != Nullability.nullable) {
        final bound = retyped.parameter.bound;
        if (bound is InterfaceType &&
            bound.classNode != declaredVar.classNode) {
          if (_abstractLike(declaredVar.classNode) &&
              !_scalarClass(declaredVar.classNode)) {
            return IrCastTo(IrLocal(name), _type(declaredVar));
          }
          return _narrowingCast(IrLocal(name), declaredVar);
        }
      }
      // Promoted to a concrete class the declaration does not name: the
      // read is a downcast. Promotion to the *same* class (nullable to
      // non-null) is not.
      var promoted = node.promotedType;
      // ..a copy's parameter as its declaration types it (`ChildType?`,
      // the bound here) rather than as the copy does (`RenderBox?`).
      final declared = declaredAs ?? node.variable.type;
      // Promoted to a type parameter (`if (item is T) return item;` in
      // `InheritedModel.inheritFrom<T>`, run541): a kept one is the
      // parameter's own conversion (`FromDynamic`, as `as T` is), an
      // erased one is its bound and takes the rules below.
      if (promoted is TypeParameterType) {
        final parameter = promoted.parameter;
        if (_erasedParameter(parameter)) {
          promoted = parameter.bound;
        } else if (!(declared is TypeParameterType &&
            declared.parameter == parameter)) {
          // Out of its `Option` first when the local is nullable (`final w
          // = map[T]; w is T ? w : null`, the typelit fixture).
          final read = declared.nullability == Nullability.nullable
              ? _nullChecked(IrLocal(name)..rustType = _recordedType(declared))
              : (IrLocal(name)..rustType = _recordedType(declared));
          return IrDowncast(read, parameter.name ?? 'T')
            ..rustType = _type(promoted);
        }
      }
      // The core scalars are abstract classes in Kernel and structs here:
      // an `Object?` promoted to `String` is a downcast to `String`.
      const scalars = {'String', 'int', 'double', 'bool', 'num'};
      // To `Object` (a pattern's `final Object? msg` on a `dynamic`
      // temporary) is to the root every value already is: no cast.
      final toObject =
          promoted is InterfaceType &&
          promoted.classNode.name == 'Object' &&
          promoted.classNode.enclosingLibrary.importUri.toString() ==
              'dart:core';
      // ..typed as what the local holds (`Rc<dyn Object>` for a
      // `dynamic`), not as the promotion says (`Object?`): the slot's
      // rule puts the `Some` on (`if` and `else` have incompatible
      // types, run453).
      // Only from a `dynamic`: an `Object?` promoted to `Object` is the
      // unwrap below (`key == keyOrNull` compared an `Rc` with an
      // `Option`, ws454).
      if (toObject && declared is DynamicType) {
        try {
          return IrLocal(name)..rustType = _type(declared);
        } on Unsupported {
          return IrLocal(name);
        }
      }
      if (promoted is InterfaceType &&
          !toObject &&
          (!_abstractLike(promoted.classNode) ||
              scalars.contains(promoted.classNode.name)) &&
          // An enum too: `switch (dependency)` after `is _MediaQueryAspect`
          // matched enum arms against an `Rc<dyn Object>` (29 at ws311).
          !(declared is InterfaceType &&
              declared.classNode == promoted.classNode)) {
        // With the struct's type arguments: `other is AsyncSnapshot<T>`
        // reads `other` as an `AsyncSnapshot<T>` (16 E0107 at ws425).
        final to = _type(promoted);
        // Out of its `Option` first when the local is nullable: a
        // `Painter? old` promoted to `Caret` asked the `Option` for its
        // `Any` (`shouldRepaint`'s `old.width`, ws590).
        // ..and typed as declared either way: untyped, the backend asked
        // the *handle's* `Any` (`resolvable.as_any()` on an `Rc<dyn
        // Color>`, never the `CupertinoDynamicColor` inside; `resolve`
        // unwrapped a `None` where `is` had just said yes, run621).
        final read = declared.nullability == Nullability.nullable
            ? _nullChecked(IrLocal(name)..rustType = _recordedType(declared))
            : (IrLocal(name)..rustType = _recordedType(declared));
        final downcast = IrDowncast(
          read,
          _rustScalar(to.name),
          arguments: to.arguments,
        );
        // ..and a promotion that is itself nullable -- `if (parent is
        // _NestedHookElement?)`, which matches null as well -- keeps the
        // absence: the downcast is null-aware, and what comes back is the
        // `Option` the slot takes. Null-checked, the read unwrapped the
        // `None` the test had just admitted
        // (`SingleChildWidgetElementMixin.mount`, 4 at ws793).
        if (promoted.nullability == Nullability.nullable &&
            declared.nullability == Nullability.nullable) {
          // The bound value is typed as what the local holds, so the
          // downcast asks the *object*'s `Any` and not the handle's
          // (`it.as_any()` on an `&Rc<dyn Base>` found no `Hook`).
          final held = _recordedType(declared);
          final bound = IrBound()
            ..rustType = held == null ? null : nonNull(held);
          final inner = IrCall(
            IrDowncast(bound, _rustScalar(to.name), arguments: to.arguments),
            'clone',
            const [],
          )..rustType = nonNull(to);
          return IrNullAware(
            IrLocal(name)..rustType = _recordedType(declared),
            inner,
          )..rustType = to;
        }
        // Cloned out of the reference `Any` hands back, and typed as what
        // the promotion says it is: untyped, a slot that takes an `Option`
        // could not tell a promoted value from one still in its `Option`
        // and left the `Some` off (`ShapeDecoration.lerpFrom`, 6 at ws764).
        return IrCall(downcast, 'clone', const [])..rustType = to;
      }
      // ..and to one of `dart:core`'s collections (`val is Map` on a
      // `dynamic`): the prelude's value, converted where its element
      // representation differs (`dart_cast_map`, as an `as Map<..>` is;
      // get's `_isNullOrEmpty`, run489).
      // ..and a typed list (`value is Float64List` in
      // `StandardMessageCodec.writeValue`, run505): the `Vec` of its
      // element the prelude names it.
      if (promoted is InterfaceType &&
          (_coreCollection(promoted.classNode) ||
              _typedList(promoted.classNode) ||
              (promoted.classNode.name == 'Iterable' &&
                  promoted.classNode.enclosingLibrary.importUri.toString() ==
                      'dart:core')) &&
          promoted.nullability != Nullability.nullable &&
          (declared is DynamicType ||
              (declared is InterfaceType &&
                  declared.classNode.name == 'Object'))) {
        final to = _type(promoted);
        // An `Iterable` promotion reads the value as the list it is here.
        final asName = promoted.classNode.name == 'Iterable' ? 'List' : to.name;
        return IrCall(
          IrDowncast(
            // A `dynamic` is a handle, never an `Option`; an `Object?` is.
            _nullChecked(IrLocal(name)..rustType = _recordedType(declared)),
            _rustScalar(asName),
            arguments: to.arguments,
          ),
          'clone',
          const [],
        )..rustType = IrType(asName, arguments: to.arguments);
      }
      // ..and to an abstract or open class: the trait cast every object
      // answers (`dart_cast_to`). Not from a nullable declaration, whose
      // `Option` the null-promotion below takes off first.
      // ..a *translated* one: `dart:core`'s `List` is abstract to Kernel
      // and a `Vec` here, the same `Vec` an `Iterable<T>` is (`newEntries
      // is List<OverlayEntry> ? newEntries : ..`, ws638).
      if (promoted is InterfaceType &&
          _abstractLike(promoted.classNode) &&
          _translatedClass(promoted.classNode) &&
          !scalars.contains(promoted.classNode.name) &&
          promoted.nullability != Nullability.nullable &&
          !(declared is InterfaceType &&
              declared.classNode == promoted.classNode) &&
          !(declared is InterfaceType &&
              declared.nullability == Nullability.nullable)) {
        // Typed as the promotion says, for the same reason the downcast
        // above is: a slot that takes an `Option` cannot otherwise tell a
        // promoted value from one still in its `Option`, and left the
        // `Some` off (`BoxBorder.lerp`'s `BorderDirectional::lerp(a, b)`,
        // ws770).
        return IrCastTo(IrLocal(name), _type(promoted))
          ..rustType = _type(promoted);
      }
      // Promoted from `T?` to `T` -- `if (x != null) f(x)` -- the read is
      // the value inside. A clone first, so the local is still there for
      // the next read: `&Option<Hct>` where `&Hct` was wanted, 12 times.
      // A type parameter's own nullability is *undetermined*, not
      // non-nullable: `value` after `if (value is! T) throw` in
      // `Provider.of` was returned still an `Option<T>`.
      if (promoted != null &&
          declared is! DynamicType &&
          promoted.nullability != Nullability.nullable &&
          declared.nullability == Nullability.nullable) {
        final inside = _nullChecked(
          IrCall(IrLocal(name), 'clone', const [])
            ..rustType = _recordedType(declared),
        );
        // ..and narrowed as well as unwrapped: `ancestor` after `ancestor
        // is StatefulElement`, on an `Element?`, read `.state` of an
        // `Rc<dyn Element>` (`findAncestorStateOfType`).
        if (promoted is InterfaceType &&
            _abstractLike(promoted.classNode) &&
            _translatedClass(promoted.classNode) &&
            !scalars.contains(promoted.classNode.name) &&
            declared is InterfaceType &&
            declared.classNode != promoted.classNode) {
          return IrCastTo(inside, _type(promoted));
        }
        return inside;
      }
      if (declaredAs != null) {
        return IrLocal(name)..rustType = _type(declaredAs);
      }
      // ..and as its slot retyped it (see `_staticType`).
      if (retyped != null) {
        final ir = _recordedType(retyped);
        if (ir != null) return IrLocal(name)..rustType = ir;
      }
      // A promotion no rule above reads through (`v is! S` on a `T`, both
      // parameters) still reads the local as declared: untyped, the slot
      // could not box it (`Entry<S>(v)` into an erased `Rc<dyn Object>`,
      // the outparam fixture).
      if (promoted != null) {
        return IrLocal(name)..rustType = _recordedType(declared);
      }
      // ..and any other read as the local is declared: untyped, two
      // handles in two locals compared by their slots' addresses
      // (`identical(b, loc)`, the dyncast fixture).
      return IrLocal(name)..rustType = _localIrType(node.variable);
    }
    if (node is InstanceGet) return _instanceGet(node);
    if (node is StaticGet) return _staticGet(node);
    if (node is InstanceInvocation) return _instanceInvocation(node);
    // A deferred import is linked like any other here: `loadLibrary()`
    // is a future already done with null, and the CFE's check before a
    // use of the library is null itself (the generated localizations'
    // `lookupGalleryLocalizations`, run579).
    if (node is LoadLibrary) {
      // `Future<dynamic>.value()`: the `null` of `dynamic` (the slot is
      // the projected `Option<Rc<dyn Object>>`, ws580).
      return IrStaticCall(
        'Future',
        'value',
        const [],
        typeArguments: const [IrType('dynamic')],
      )..rustType = const IrType('Future', arguments: [IrType('dynamic')]);
    }
    if (node is CheckLibraryIsLoaded) {
      return IrStaticCall(null, 'dart_null_object', const [])
        ..rustType = const IrType('dynamic');
    }
    if (node is BlockExpression) return _blockValue(node);
    if (node is FunctionInvocation) {
      // A call through a function value: no callee to read defaults from,
      // only its type -- which is enough to *order* named arguments, and the
      // closures on the other end are declared in that same order.
      final type = node.functionType;
      if (type == null) {
        // A call on a bare `Function` (or a `dynamic`): Dart's dynamic
        // call, which the prelude's function object answers with its
        // arguments as objects and its result as one (`dart_call_function`).
        if (node.arguments.named.isNotEmpty ||
            node.arguments.types.isNotEmpty) {
          throw Unsupported(
            'dynamic call with named or type arguments',
            _sample(node),
          );
        }
        return IrStaticCall(null, 'dart_call_function', [
          coerce(expression(node.receiver), const IrType('dynamic')),
          IrListLiteral([
            for (final a in node.arguments.positional)
              coerce(expression(a), const IrType('dynamic')),
          ], const IrType('dynamic')),
        ], fails: true)..rustType = const IrType('dynamic');
      }
      // ..unless the value is a tear-off the compiler resolved to a
      // constant (`GoogleFonts.libreFranklin(..)`, whose getter TFA folded
      // into `partL::libreFranklin`): that is the static call it names, and
      // its arguments go in the function's own order, not the type's sorted
      // one, which put a `FontWeight` in the `locale` slot (34 at ws321).
      var receiver = node.receiver;
      while (receiver is FileUriExpression) {
        receiver = receiver.expression;
      }
      final constantTarget =
          receiver is ConstantExpression &&
              receiver.constant is StaticTearOffConstant
          ? (receiver.constant as StaticTearOffConstant).target
          : receiver is StaticTearOff
          ? receiver.target
          : null;
      if (constantTarget != null &&
          constantTarget.function.typeParameters.isEmpty) {
        return IrStaticCall(
          constantTarget.enclosingClass?.name,
          constantTarget.enclosingClass == null
              ? _topLevelName(constantTarget.name.text)
              : constantTarget.name.text,
          _arguments(node.arguments, constantTarget.function),
          fails: _fails(constantTarget),
          diverges: _diverges(constantTarget),
        );
      }
      return IrCallValue(
        expression(node.receiver),
        _argumentsByType(node.arguments, type),
      )..rustType = _type(type).returns;
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
      )..rustType = _type(node.functionType).returns;
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
        final init = expression(node.value);
        final stored = _widened(
          node.value,
          target.function.positionalParameters.single.type,
          IrCall(IrLocal(held), 'clone', const [])..rustType = init.rustType,
        );
        return IrBlockValue([
          IrLocalDecl(held, null, init),
          IrExprStmt(
            IrStaticCall(null, _topLevelSetterName(target.name.text), [stored]),
          ),
        ], IrLocal(held));
      }
      // ..and a class's static setter: its static function (`UndoManager
      // .client = this` in `UndoHistoryState.initState`, run678).
      if (target is Procedure &&
          target.kind == ProcedureKind.Setter &&
          target.enclosingClass != null) {
        final held = '__t${_nextTemporary++}';
        final init = expression(node.value);
        final stored = _widened(
          node.value,
          target.function.positionalParameters.single.type,
          IrCall(IrLocal(held), 'clone', const [])..rustType = init.rustType,
        );
        return IrBlockValue([
          IrLocalDecl(held, null, init),
          IrExprStmt(
            IrStaticCall(
              target.enclosingClass!.name,
              'set_${target.name.text}',
              [stored],
            ),
          ),
        ], IrLocal(held));
      }
      if (target is! Field) {
        throw Unsupported('static setter used for its value', _sample(node));
      }
      final owner = target.enclosingClass;
      final held = '__t${_nextTemporary++}';
      final init = expression(node.value);
      // The store widens into the static's type: `_decomposeV ??=
      // Vector3.zero()` on a `Vector3?` stores `Some(..)`.
      final write = _widened(
        node.value,
        target.type,
        IrCall(IrLocal(held), 'clone', const [])..rustType = init.rustType,
      );
      return IrBlockValue([
        IrLocalDecl(held, null, init),
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
        final asDouble = (IrCall(IrDowncast(left, 'f64'), 'clone', const [])
          ..rustType = const IrType('double'));
        final other = numClass(rightType) == 'int' ? _toF64(right) : right;
        return IrBinary('==', asDouble, other);
      }
      if (rightType is DynamicType && number(leftType)) {
        final asDouble = (IrCall(IrDowncast(right, 'f64'), 'clone', const [])
          ..rustType = const IrType('double'));
        final other = numClass(leftType) == 'int' ? _toF64(left) : left;
        return IrBinary('==', other, asDouble);
      }
      // `lightOption == -1` on a `double`: the `int` side is cast, as the
      // arithmetic operators cast theirs.
      String? cls(DartType? t) => t is InterfaceType ? t.classNode.name : null;
      if (cls(leftType) == 'double' && cls(rightType) == 'int') {
        right = _toF64(right);
      } else if (cls(leftType) == 'int' && cls(rightType) == 'double') {
        left = _toF64(left);
      } else if (_declaredNum(node.left) &&
          (node.right is IntLiteral || cls(rightType) == 'int')) {
        right = _toF64(right);
      } else if (_declaredNum(node.right) &&
          (node.left is IntLiteral || cls(leftType) == 'int')) {
        left = _toF64(left);
      }
      // Operands of one Rust type: the right to the left's, else the left
      // to the right's (`data.previousSibling == after`, an erased
      // `RenderObject?` read against a `RenderBox?`, 47 at ws379).
      // ..and of one nullability: a `dynamic?` -- a `T?` read bound to a
      // top type, `raw[id]` -- against a `dynamic` (ws501); the bare side
      // goes into the `Option` (Dart's `null` there is `None`).
      final lt = left.rustType;
      final rt = right.rustType;
      if (coerceByType &&
          lt != null &&
          rt != null &&
          _normalName(lt.name) == _normalName(rt.name) &&
          isNullable(lt) != isNullable(rt)) {
        if (isNullable(lt)) {
          right = coerce(right, lt);
        } else {
          left = coerce(left, rt);
        }
      } else if (coerceByType &&
          lt != null &&
          rt != null &&
          _normalName(lt.name) != _normalName(rt.name)) {
        // The side *below* goes up into the other's type; the other way
        // is a downcast that fails on a value of the wider class
        // (`next?.route != entry.lastAnnouncedNextRoute`: a `Route?`
        // against the `_RoutePlaceholder?` it extends, run636). Two
        // unrelated traits are left as they are: the backend compares
        // them as objects.
        final hierarchy = typeEnvironment?.hierarchy;
        final lc = leftType is InterfaceType ? leftType.classNode : null;
        final rc = rightType is InterfaceType ? rightType.classNode : null;
        final related = lc != null && rc != null && lc != rc;
        final leftBelow =
            related && (hierarchy?.isSubInterfaceOf(lc, rc) ?? false);
        final rightBelow =
            related && (hierarchy?.isSubInterfaceOf(rc, lc) ?? false);
        if (leftBelow && !rightBelow) {
          left = coerce(left, rt);
        } else if (rightBelow && !leftBelow) {
          right = coerce(right, lt);
        } else if (related &&
            !leftBelow &&
            !rightBelow &&
            _abstractLike(lc) &&
            _abstractLike(rc)) {
          // Unrelated: as they are.
        } else {
          final r = coerce(right, lt);
          if (!identical(r, right)) {
            right = r;
          } else {
            left = coerce(left, rt);
          }
        }
      }
      return IrBinary('==', left, right);
    }
    // A truly dynamic call -- `number.abs()` on a `dynamic` in intl's
    // NumberFormat -- when the name is one of `num`'s: the receiver is
    // downcast to the `f64` a `num` is here (see the devirtualised case).
    const numMethods = _dynamicNumMethods;
    if (node is DynamicInvocation && numMethods.contains(node.name.text)) {
      final asDouble = (IrCall(
        IrDowncast(expression(node.receiver), 'f64'),
        'clone',
        const [],
      )..rustType = const IrType('double'));
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
      final asDouble = (IrCall(
        IrDowncast(expression(node.receiver), 'f64'),
        'clone',
        const [],
      )..rustType = const IrType('double'));
      var right = expression(node.arguments.positional.single);
      final rightType = _staticType(node.arguments.positional.single);
      if (rightType is DynamicType) {
        right = (IrCall(IrDowncast(right, 'f64'), 'clone', const [])
          ..rustType = const IrType('double'));
      } else if (rightType is InterfaceType &&
          rightType.classNode.name == 'int') {
        right = _toF64(right);
      }
      return IrBinary(node.name.text, asDouble, right);
    }
    if (node is DynamicGet && numMethods.contains(node.name.text)) {
      final asDouble = (IrCall(
        IrDowncast(expression(node.receiver), 'f64'),
        'clone',
        const [],
      )..rustType = const IrType('double'));
      return IrCall(asDouble, node.name.text, const []);
    }
    if (node is Not) {
      // `x is! T` is a `Not` around an `IsExpression` here; the analyzer keeps
      // it as one node with a flag. Folded so the two front ends write the
      // same Rust -- `is_none()`, not `!(..is_some())` -- which is the whole
      // point of having two of them.
      final inner = node.operand;
      if (inner is IsExpression) {
        final test = _isExpression(inner);
        if (test is IrIs) {
          return IrIs(test.expr, test.type, negated: true);
        }
        return IrUnary('!', test);
      }
      return IrUnary('!', _condition(node.operand));
    }
    if (node is LogicalExpression) {
      // Both operands are conditions (`_condition`): `a && prop.resolve(s)`
      // on an erased `resolve` had an `Rc<dyn Object>` where `&&` wants a
      // `bool` (`_MaterialScrollbar._thickness`, ws705).
      return IrBinary(
        node.operatorEnum == LogicalExpressionOperator.AND ? '&&' : '||',
        _condition(node.left),
        _condition(node.right),
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
      // ..only inside a string: elsewhere each branch widens into
      // `Object` below. Stringifying everywhere turned `slots != null ?
      // slots[i] : IndexedSlot(..)` -- an `Object?` returned from
      // `slotFor` -- into a `String` (`updateChildren`, ws475).
      final staticType = node.staticType;
      final thenType = _staticType(node.then);
      final elseType = _staticType(node.otherwise);
      if (_inStringPart &&
          staticType is InterfaceType &&
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
        _condition(condition),
        _widened(node.then, staticType, expression(node.then)),
        _widened(node.otherwise, staticType, expression(node.otherwise)),
      );
    }
    if (node is TypeLiteral) return _typeLiteral(node.type);
    if (node is IsExpression) return _isExpression(node);
    if (node is ConstructorInvocation) return _construct(node);
    if (node is StaticInvocation) return _staticInvocation(node);
    if (node is SuperMethodInvocation) {
      // The target member is already resolved -- this is the fact the analyzer
      // front end had to work out for itself.
      final ownerClass = _realOwner(node.interfaceTarget, node.name.text);
      final owner = ownerClass?.name;
      if (owner == null) {
        throw Unsupported('super call with no owner', '$node');
      }
      // The super target is resolved, so its parameter list orders the named
      // arguments -- 56 super calls with named arguments were refused for
      // want of a callee this line had all along.
      return IrSuperCall(
        owner,
        node.name.text,
        // Into the declaration's slots: a super call reaches the mixin's
        // super function, typed by the mixin with this class's arguments
        // put in (`super.insert(child, after: after)` in
        // `RenderSliverMultiBoxAdaptor`, ws479; `didPop(result)`'s `T?`
        // as the class's projected `T?`, ws492).
        _arguments(
          node.arguments,
          node.interfaceTarget.function,
          true,
          null,
          _superSlots(node.interfaceTarget).$1,
          _superSlots(node.interfaceTarget).$2,
        ),
        baseArguments: _superBaseArguments(ownerClass!),
        typeArguments: _typeArgumentsOf(node.arguments),
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
      final raw = expression(node.value);
      final stored = _widened(
        node.value,
        _localType(node.variable),
        raw,
        slotIr: _localIrType(node.variable),
      );
      // `(index = s.indexOf(p)) >= 0` with `int? index`: the store is
      // `Some(..)`, the value of the expression is not -- nor, for a
      // `dynamic` temporary assigned a `String` (a pattern's `#0#2 =
      // error.message`, run452), is it the boxed `Rc<String>`. Whatever
      // the store adapted, the value is the one before: held, stored
      // through the same adaptation of a clone, produced.
      // A literal is not held: the store re-lowers it against the slot's
      // element types, and holding it too would evaluate it twice.
      if (!identical(stored, raw) &&
          node.value is! MapLiteral &&
          node.value is! ListLiteral) {
        final held = '__t${_nextTemporary++}';
        final again = IrCall(IrLocal(held), 'clone', const [])
          ..rustType = raw.rustType;
        // The held value typed by Dart's static type when its own says
        // nothing Rust can infer from: a pattern cache's block whose read
        // TFA removed ends in `unreachable!()`, and `let __t = { ..;
        // unreachable!() }` has no type (`CupertinoDynamicColor.
        // resolveFrom`, run622).
        final rawType = raw.rustType;
        IrType? heldType;
        if (rawType == null || rawType.name == 'Never') {
          // The local's own type, without its `Option`: the value the
          // store wraps (`Some(__t.clone())`). Dart's static type of the
          // block is `Never` once TFA has been through it.
          final slot = _localIrType(node.variable);
          heldType = slot == null ? null : _nonNull(slot);
        }
        return IrBlockValue([
          IrLocalDecl(held, heldType, raw),
          IrAssign(
            name,
            _widened(
              node.value,
              _localType(node.variable),
              again,
              slotIr: _localIrType(node.variable),
            ),
          ),
        ], IrLocal(held))..rustType = raw.rustType;
      }
      return IrAssignValue(name, stored);
    }
    if (node is RecordIndexGet) {
      // `r.$1` in Dart is `r.0` in Rust -- Dart counts its positional record
      // fields from one and Rust counts tuple fields from zero.
      var held = expression(node.receiver);
      // Through the unwrap where the record is an `Option`: Dart lets a
      // field be read only once the record is promoted non-null, and the
      // `is` above it is that promotion (`_AscentDescent` is a
      // `(double, double)?`, `RenderFlex._computeSizes`, ws715).
      final receiverType = held.rustType;
      if (receiverType != null && isNullable(receiverType)) {
        held = _nullChecked(held);
      }
      final read = IrRecordField(held, node.index);
      // Typed as the *record* holds it, not as a promotion reads it: a
      // record pattern promotes a `(double, double)` to `(Object?,
      // Object?)` and reads `$1` at `Object?`, where the Rust tuple still
      // holds an `f64` -- and the coercion into the slot, seeing an
      // `Rc<dyn Object>` already, boxed nothing
      // (`RenderFlex._computeSizes`, run713).
      final record = held.rustType;
      if (record != null && node.index < record.arguments.length) {
        read.rustType = record.arguments[node.index];
      }
      return read;
    }
    if (node is RecordNameGet) {
      // A named field is the tuple field after the positional ones, at its
      // place in the type's (sorted) named list -- the same read as
      // `RecordIndexGet` above, by a different spelling of the index.
      var held = expression(node.receiver);
      final receiverType = held.rustType;
      if (receiverType != null && isNullable(receiverType)) {
        held = _nullChecked(held);
      }
      final where = node.receiverType.named.indexWhere(
        (n) => n.name == node.name,
      );
      final index = node.receiverType.positional.length + where;
      final read = IrRecordField(held, index);
      final record = held.rustType;
      if (where >= 0 && record != null && index < record.arguments.length) {
        read.rustType = record.arguments[index];
      }
      return read;
    }
    if (node is RecordLiteral) {
      return _recordLiteral(
        node,
        node.recordType.positional,
        node.recordType.named,
      );
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
        final outerPart = _inStringPart;
        _inStringPart = true;
        final IrExpr lowered;
        try {
          lowered = expression(e);
        } finally {
          _inStringPart = outerPart;
        }
        if (e is StringLiteral || lowered is IrLiteral) return lowered;
        final type = _staticType(e);
        final name = type is InterfaceType ? type.classNode.name : null;
        const plain = {'String', 'int', 'double', 'num', 'bool', 'Null'};
        if (name != null &&
            plain.contains(name) &&
            type!.nullability != Nullability.nullable) {
          // A double as Dart spells it (`3.0`, not Rust's `3`): the
          // prelude's `dart_double_str`.
          if (name == 'double') {
            return IrStaticCall(null, 'dart_double_str', [lowered])
              ..rustType = const IrType('String');
          }
          return lowered;
        }
        return _stringOf(lowered, type)!;
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
        // ..with the field's own class named: in a trait body a read on
        // `this` is the accessor, and this class may override it with a
        // getter (`Color get primaryColor => super.primaryColor ?? ..`
        // over `NoDefaultCupertinoThemeData`'s field, run618) -- the
        // backend then asks the base trait's accessor for the storage.
        return IrField(
          null,
          _memberName(node.interfaceTarget!),
          onEnum: node.interfaceTarget?.enclosingClass?.isEnum ?? false,
          owner: owner,
        );
      }
      // The class the read lands in (`_realOwner`), as a method call's
      // is: `super.popDisposition` in `ModalRoute` names the anonymous
      // application of `LocalHistoryRoute`, whose hollow declaration TFA
      // emptied and whose body the application holds (run648).
      final target = node.interfaceTarget;
      final ownerClass = target == null
          ? null
          : _realOwner(target, node.name.text);
      final base = ownerClass?.name ?? owner;
      // `super.paint` as a value (`context.pushLayer(layer, super.paint,
      // offset)`): the closure the tear-off is, calling the super
      // function -- as an instance tear-off is (ws649, 15 stubs the
      // moment the owner resolved).
      if (target is Procedure && target.kind == ProcedureKind.Method) {
        return _superTearOff(node, target);
      }
      // Typed by the getter, in this class's kept terms: an erased
      // `ChildType?` is the `RenderObject?` the super function hands
      // back, which the slot narrows (`_RenderTheater._firstOnstageChild`
      // reading `super.firstChild` into a `RenderBox?`, ws649).
      // ..the *mixin's* declaration where it still has one: the target
      // may be the application's copy, already at `RenderBox?`, while the
      // super function is written in the mixin's erased terms.
      final typed = target == null ? null : _superReturn(target);
      if (Platform.environment['DART2RUST_TRACE_SUPER'] == node.name.text) {
        stderr.writeln(
          'TRACE_SUPER get ${node.name.text} owner=${ownerClass?.name} '
          'typed=$typed',
        );
      }
      return IrSuperCall(
        base,
        node.name.text,
        const [],
        baseArguments: ownerClass == null
            ? const []
            : _superBaseArguments(ownerClass),
      )..rustType = typed;
    }
    if (node is SuperPropertySet) {
      // `super.value = value` in `_RestorablePrimitiveValue.value=`: the
      // base's setter, through its super function, the value kept as any
      // assignment's is (9 refusals at ws354). A base *field* is the same
      // storage as this class's (flattened): a plain write.
      final target = node.interfaceTarget;
      final ownerClass = target == null
          ? null
          : _realOwner(target, node.name.text);
      final owner = ownerClass?.name;
      if (target == null || owner == null) {
        throw Unsupported('super property set with no owner', _sample(node));
      }
      final slot = target is Field
          ? target.setterType
          : target is Procedure
          ? target.function.positionalParameters.single.type
          : null;
      final held = '__t${_nextTemporary++}';
      final init = expression(node.value);
      final stored = _widened(
        node.value,
        slot,
        IrCall(IrLocal(held), 'clone', const [])..rustType = init.rustType,
      );
      return IrBlockValue([
        IrLocalDecl(held, null, init),
        target is Field
            ? IrAssignField(_fieldNameOf(target, node.name.text), stored)
            : IrExprStmt(
                IrSuperCall(
                  owner,
                  node.name.text,
                  [stored],
                  isSetter: true,
                  baseArguments: _superBaseArguments(ownerClass!),
                ),
              ),
      ], IrLocal(held));
    }
    if (node is AwaitExpression) {
      // `await <throw>`: the tree shaker replaces a removed call with a
      // throw, and there is nothing to await in a throw -- `.await` on a
      // `return Err(..)` is what came out.
      if (node.operand is Throw) return expression(node.operand);
      // `await v` where `v` is *not* a future: Dart waits an event turn and
      // completes with the value. `await null` -- the idiom for letting the
      // microtask queue run -- is the whole of it here, and `None.await`
      // was what came out (`ImageProvider.resolve`, run724). The already
      // done future of the value, awaited: one turn, then the value.
      final awaited = _staticType(node.operand);
      if (awaited != null && !_couldBeFuture(awaited)) {
        IrType? spelled;
        try {
          spelled = _type(awaited);
        } on Unsupported {
          spelled = null;
        }
        return IrAwait(
          IrStaticCall(null, 'future_ready', [
              expression(node.operand),
            ], typeArguments: spelled == null ? const [] : [spelled])
            ..rustType = IrType(
              'Future',
              arguments: [if (spelled != null) spelled],
            ),
        );
      }
      // Typed as the operand's future says: `await channel.invokeMethod<T>()`
      // hands back the `Option<Rc<dyn Object>>` the erased twin's future
      // holds, and typed by Dart's static type alone it was a bare
      // `dynamic` -- `dart_nullable` was then put around a value already in
      // its `Option` (`DefaultProcessTextService.processTextAction`, the
      // run's own panic, ws776).
      final operand = expression(node.operand);
      final future = operand.rustType;
      final held =
          future != null &&
              future.name == 'Future' &&
              future.arguments.length == 1
          ? future.arguments.single
          : null;
      return IrAwait(operand)..rustType = held;
    }
    if (node is Throw) {
      if (_tfaUnreachable(node)) return _unreachable;
      // `a ?? throw StateError(..)`. Rust has no throw, but it does have an
      // expression that never produces a value: `return Err(e)` has type `!`,
      // which fits wherever a value was wanted. So the expression form is the
      // statement form, written where the value would have gone.
      // A *value* of a translated class thrown from a local or a call
      // (`throw error` with a `FlutterError` in hand) goes behind an
      // `Rc<dyn Object>` here; a constructed one the backend boxes itself
      // (`_boxedThrow`), a handle or a trait object unsizes on its own (18).
      return IrThrowValue(_thrownValue(node.expression));
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
            final init = expression(node.value);
            final clone = IrCall(IrLocal(held), 'clone', const [])
              ..rustType = init.rustType;
            // Into a nullable field the store is `Some(..)`.
            final stored = _widened(
              node.value,
              _writeSlot(node.interfaceTarget, receiver),
              clone,
              slotIr: _writeSlotIr(node.interfaceTarget, receiver),
            );
            return IrBlockValue([
              // Inferred: the field's *declared* type is the generic `T?` of
              // `Tween<T>`, and spelling it put a `T` into a class with none.
              IrLocalDecl(held, null, init),
              // A field goes through storage (a cell when the class is
              // counted); a setter is a call, on whatever the receiver is.
              target is Field
                  ? IrAssignField(
                      _fieldNameOf(target, node.name.text),
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
      if (node.interfaceTarget is! Field &&
          !_heldField(node.interfaceTarget, node.receiver)) {
        // `_firstChild = _lastChild = child` in a mixin's body: the mixin's
        // field is a setter on its trait. Called, and the value kept -- as
        // the field on another object is above. Refused before ws348, which
        // left `ContainerRenderObjectMixin._insertIntoChildList` out of
        // every applier (27 `todo!`s).
        final held = '__t${_nextTemporary++}';
        final init = expression(node.value);
        final stored = _widened(
          node.value,
          _writeSlot(node.interfaceTarget, node.receiver),
          IrCall(IrLocal(held), 'clone', const [])..rustType = init.rustType,
          slotIr: _writeSlotIr(node.interfaceTarget, node.receiver),
        );
        return IrBlockValue([
          IrLocalDecl(held, null, init),
          IrSetter(
            null,
            _fieldNameOf(node.interfaceTarget, node.name.text),
            stored,
            qualifier: _setterQualifier(null, node.interfaceTarget),
          ),
        ], IrLocal(held));
      }
      final stored = _widened(
        node.value,
        _writeSlot(node.interfaceTarget, node.receiver),
        expression(node.value),
        slotIr: _writeSlotIr(node.interfaceTarget, node.receiver),
      );
      if (stored is IrSome) {
        // `_cache = s` into a `String?` field, used for its value: the
        // store is `Some(s)`, the value is `s`.
        // ..and the value is the *stored* one, in the slot's type: a
        // `Semantics` behind the field's `Rc<dyn Widget>` already, not a
        // value to share again (`_modalScopeCache ??= Semantics(..)`
        // was `dart_object(dart_object(..))`, ws512).
        final held = '__t${_nextTemporary++}';
        final storedType = stored.value.rustType;
        return IrBlockValue([
          IrLocalDecl(held, null, stored.value),
          IrAssignField(
            _fieldNameOf(node.interfaceTarget, node.name.text),
            IrSome(
              IrCall(IrLocal(held), 'clone', const [])..rustType = storedType,
            ),
          ),
        ], IrLocal(held)..rustType = storedType)..rustType = storedType;
      }
      return IrSetValue(null, node.name.text, stored);
    }
    if (node is NullCheck) {
      return _nullChecked(expression(node.operand), node.operand);
    }
    if (node is AsExpression) {
      // `null as T`: the null of `T` -- `None` for a nullable `T`, a panic
      // (Dart's `TypeError`) for one with no null. Spelled through the
      // prelude, which asks `T` itself (`_queue[i] ?? (null as E)` in
      // `HeapPriorityQueue`, run436).
      // ..whether written as the literal or as the CFE's `let Null #t =
      // null in #t as E`: the operand's static type is `Null`.
      if (node.operand is NullLiteral ||
          _staticType(node.operand) is NullType) {
        return IrStaticCall(
          null,
          'dart_null_as',
          const [],
          typeArguments: [_type(node.type)],
        )..rustType = _type(node.type);
      }
      // A cast that only removes `?` -- the CFE's spelling of a promoted
      // private field, `_hct` after `if (_hct != null)` -- is a null check.
      // Any other cast is the operand: Rust's types are already the
      // concrete ones. 12 `&Option<Hct>` where `&Hct` was wanted.
      final from = _backHere(_staticType(node.operand));
      // ..and the target through `_appliedBack`: a mixin body borrowed
      // from an application casts to the application's `FlexParentData`
      // where the trait holds the erased bound (ws739).
      final to = _backHere(node.type)!;
      // ..of a function type too: TFA's `unsafeCast<Fn>(widget.builder)`
      // under `if (widget.builder != null)` is the `!` it rewrote away
      // (`WidgetsApp.build`, ws507).
      final removesNullOnly =
          from != null &&
          from.nullability == Nullability.nullable &&
          to.nullability == Nullability.nonNullable &&
          ((from is InterfaceType &&
                  to is InterfaceType &&
                  from.classNode == to.classNode) ||
              (from is FunctionType &&
                  to is FunctionType &&
                  from.withDeclaredNullability(Nullability.nonNullable) == to));
      if (removesNullOnly) {
        // `unsafeCast<double>(..)`, the tree shaker's form of `..!`: the
        // value when the operand is not an `Option` here (its recorded
        // type says), the unwrap otherwise.
        final inner = expression(node.operand);
        final have = inner.rustType;
        if (have != null && !isNullable(have)) return inner;
        return IrNullCheck(inner);
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
        // A `dynamic` is an `Rc<dyn Object>`, never an `Option`: no unwrap
        // (`codec.decodeEnvelope(result) as T?`, ws461).
        // By the operand's *recorded* type: an `Option` (a `Map<K,
        // Object?>` lookup) is unwrapped for a non-null `T` and kept for
        // a `T?`; a value the type flow analysis narrowed to a scalar
        // (`begin as T` on an `f64`) goes behind the object first, as any
        // value into a `dynamic` does (`Tween.lerp`, ws514).
        final asOption = to.nullability == Nullability.nullable;
        var operand = expression(node.operand);
        final have = operand.rustType;
        if (have != null) {
          if (isNullable(have) && !asOption && operand is! IrNullCheck) {
            operand = IrNullCheck(operand)..rustType = _nonNull(have);
          }
        } else if (from != null &&
            from is! DynamicType &&
            from.nullability == Nullability.nullable) {
          operand = _nullChecked(operand, node.operand);
        }
        final kept = operand.rustType;
        operand = coerce(
          operand,
          kept != null && isNullable(kept)
              ? const IrType('dynamic', nullable: true)
              : const IrType('dynamic'),
        );
        // `as T?`: the `Option` the downcast hands back, Dart's null for
        // a `Null` object or another type (`decodeEnvelope(result) as T?`
        // returning `T?`, ws482).
        if (asOption) {
          return IrCall(operand, '!as_opt', [
            IrLiteral(to.parameter.name ?? 'T', const IrType('raw')),
          ])..rustType = _type(to);
        }
        return IrCall(
          IrDowncast(operand, to.parameter.name ?? 'T'),
          'clone',
          const [],
        );
      }
      // `state as T?` from a class, `T` a type parameter: by id in the
      // backend (`dart_cast_any`). From the parameter's own nullable self
      // (`value as T` on a `T?`) only null is in question.
      if (to is TypeParameterType && !_erasedParameter(to.parameter)) {
        if (from is TypeParameterType && from.parameter == to.parameter) {
          if (to.nullability == Nullability.nullable) {
            return expression(node.operand);
          }
          // `_value as T` on a `T?`: the `T` inside, or `T`'s own null
          // when `T` has one (`RestorableValue<double?>.value`, run665) --
          // an unwrap said a null `double?` was no `double?`.
          return IrStaticCall(
            null,
            'dart_as_own',
            [_asOwnOption(expression(node.operand), to)],
            fails: true,
            typeArguments: [
              _type(to.withDeclaredNullability(Nullability.nonNullable)),
            ],
          )..rustType = _type(to);
        }
        if (from is InterfaceType) {
          return IrCastTo(expression(node.operand), _type(to));
        }
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
              to.classNode.name == 'String' ||
              // `dart:core`'s collections are abstract there and values
              // here: `systemMessage as Map<String, dynamic>` is the
              // `Map<String, Rc<dyn Object>>` the object holds (run459).
              _coreCollection(to.classNode)) &&
          to.classNode.name != 'Object' &&
          (from is! InterfaceType || from.classNode != to.classNode)) {
        // `dynamic` is an `Rc<dyn Object>`, never an `Option`, whatever
        // its nullability says.
        if ((from is DynamicType || from.nullability != Nullability.nullable) &&
            to.nullability != Nullability.nullable) {
          final target = _type(to);
          return IrCall(
            IrDowncast(
              expression(node.operand),
              _rustScalar(to.classNode.name),
              arguments: target.arguments,
            ),
            'clone',
            const [],
          );
        }
        // `Zone.current[#token] as Client?`: a `dynamic` (never an `Option`
        // here) to a nullable struct is a downcast that may fail: `cloned()`
        // of the `Option<&T>` `Any` gives.
        // ..and from an `Object?`, a `dynamic` here (ws503): by the
        // operand's recorded type, which is no `Option`.
        final operandLowered = expression(node.operand);
        final operandType = operandLowered.rustType;
        final dynamicOperand =
            from is DynamicType ||
            (operandType != null &&
                operandType.name == 'dynamic' &&
                !operandType.nullable);
        if (dynamicOperand && to.nullability == Nullability.nullable) {
          return IrCall(operandLowered, '!as_opt', [
            IrLiteral(_rustScalar(to.classNode.name), const IrType('raw')),
          ], typeArguments: _type(to).arguments);
        }
        // `_objects![2] as _ImageFilter?`: an `Option<Rc<dyn Object>>` to an
        // `Option<_ImageFilter>`, element by element.
        if (from.nullability == Nullability.nullable &&
            to.nullability == Nullability.nullable) {
          // The bound typed as the value inside: a trait handle is asked
          // through `as_ref()`, and untyped it was asked for the `Rc`'s
          // own `Any` (`?.widget as HeroControllerScope?` in
          // `NavigatorState.initState`, run624).
          return IrNullAware(
            operandLowered,
            IrCall(
              IrDowncast(
                IrBound()
                  ..rustType = _recordedType(
                    from.withDeclaredNullability(Nullability.nonNullable),
                  ),
                _rustScalar(to.classNode.name),
              ),
              'clone',
              const [],
            ),
          );
        }
      }
      // `ancestor as StatefulElement?` from an `Element?`: a downcast to
      // a trait, the `Option` kept when the target is nullable (the
      // prelude's `dart_cast_to` on an `Option`). An upcast stays the
      // operand: the value already is one.
      // Only a class that is a trait here: `List` and `TypedData` are
      // abstract to Kernel and prelude types or nothing here (18 "expected
      // trait, found struct" at ws340).
      // ..and `child.parentData! as ParentDataType` with the parameter
      // erased: the cast is to its bound, `ContainerParentDataMixin`
      // (13 `Rc<dyn ParentData>` where that was wanted, ws353).
      final toClass = to is InterfaceType
          ? to
          : to is TypeParameterType &&
                _erasedParameter(to.parameter) &&
                to.parameter.bound is InterfaceType
          ? (to.parameter.bound as InterfaceType).withDeclaredNullability(
              to.nullability == Nullability.nullable
                  ? Nullability.nullable
                  : Nullability.nonNullable,
            )
          : null;
      if (toClass != null &&
          from is InterfaceType &&
          _abstractLike(toClass.classNode) &&
          _translatedClass(toClass.classNode) &&
          !_scalarClass(toClass.classNode) &&
          toClass.classNode.name != 'Object' &&
          from.classNode != toClass.classNode &&
          !(typeEnvironment?.hierarchy.isSubInterfaceOf(
                from.classNode,
                toClass.classNode,
              ) ??
              true)) {
        return IrCastTo(expression(node.operand), _type(toClass));
      }
      // `x as dynamic` (and `as Object`): every value goes behind the
      // handle a `dynamic` is here, and a projected `T?` is not one --
      // `<T as DartNullable>::Or` was handed to the `dynamic` operator
      // rules, which asked it for its `Any` and found no `f64` inside
      // (`Tween.lerp`'s `(begin as dynamic) + ((end as dynamic) - ..)`,
      // the run's own panic at run796). A value already behind the handle
      // coerces to itself.
      if (to is DynamicType ||
          (to is InterfaceType &&
              to.classNode.name == 'Object' &&
              to.classNode.enclosingLibrary.importUri.toString() ==
                  'dart:core')) {
        return coerce(expression(node.operand), const IrType('dynamic'));
      }
      return expression(node.operand);
    }
    if (node is StaticTearOff) {
      return IrFunctionRef(
        node.target.enclosingClass?.name,
        node.target.name.text,
      )..rustType = _functionRefType(node.target);
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
      // ..and of a *constant* (`const GZipCodec().decode`): the closure
      // captures nothing, the constant is spelled inside it.
      final onConstant = node.receiver is ConstantExpression;
      // ..and any other receiver is *evaluated once* and captured, which
      // is what Dart does at the tear-off: bound outside the closure and
      // moved in. `PaintingBinding.instance.instantiateImageCodecWithSize`
      // handed to `loadImage` was refused for want of this (run745).
      final bindReceiver =
          !holds && !_borrowedArgument && !onLocal && !onConstant;
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
      // A tear-off of one of the prelude's collection methods
      // (`nodeScope._focusedChildren.remove` handed to `forEach`): the
      // call it stands for, lowered as an invocation, so that the
      // collection tables apply (`remove` is `remove_value`, not `Vec::
      // remove(usize)`, `FocusNode._removeChild`, run642). The callee's
      // own parameters name the closure's, as `params` does.
      final declaringClass = node.interfaceTarget.enclosingClass;
      if (declaringClass != null &&
          _coreCollections.contains(declaringClass.name) &&
          declaringClass.enclosingLibrary.importUri.scheme == 'dart' &&
          receiver is! ThisExpression) {
        // The receiver node itself (a clone would need the closure's free
        // variables mapped): borrowed into the call and given back to
        // the tear-off after.
        final call = InstanceInvocation(
          InstanceAccessKind.Instance,
          receiver,
          node.name,
          Arguments(
            [for (final p in fn.positionalParameters) VariableGet(p)],
            named: [
              for (final p in fn.namedParameters)
                NamedExpression(p.parameterName, VariableGet(p)),
            ],
          ),
          interfaceTarget: node.interfaceTarget,
          functionType: torn is FunctionType
              ? torn
              : fn.computeFunctionType(Nullability.nonNullable),
        );
        final tornReturns = _type(returnType);
        final IrExpr lowered;
        try {
          lowered = expression(call);
        } finally {
          receiver.parent = node;
        }
        return IrClosure(
            params,
            IrReturn(coerce(lowered, tornReturns)),
            tornReturns,
            locals: _freeLocalsIn(receiver, {}),
          )
          ..rustType = IrType.function([
            for (final p in params) p.type,
          ], tornReturns);
      }
      // The call typed by the member it reaches (`_qualified`) and coerced
      // into the tear-off's own return: a mixin's `ChildType? childAfter`
      // hands back the erased `RenderObject?` where the torn type says
      // `RenderSliver?` (`RenderViewport._attemptLayout`'s `advance:
      // childAfter`, ws527).
      // The receiver bound once (see `bindReceiver`): a closure that read
      // it again would read whatever it says the next time.
      final IrExpr? boundInit = bindReceiver && receiver is! ThisExpression
          ? expression(receiver)
          : null;
      final String? bound = boundInit == null ? null : '__t${_nextTemporary++}';
      final tornCall = _qualified(
        IrCall(
          receiver is ThisExpression
              ? null
              : bound != null
              ? (IrLocal(bound)..rustType = boundInit!.rustType)
              : expression(receiver),
          node.name.text,
          [
            for (var i = 0; i < fn.positionalParameters.length; i++)
              IrLocal(params[i].name),
            for (final p in fn.namedParameters) IrLocal(p.parameterName),
          ],
          // The adapter's call propagates like a written one would.
          fails: _fails(node.interfaceTarget),
          asyncFn: _inherentAsync(
            node.interfaceTarget,
            receiver is ThisExpression
                ? ((_member?.enclosingClass?.isAnonymousMixin ?? false)
                      ? _lowering
                      : _member?.enclosingClass)
                : _staticClass(receiver),
            null,
            onThis: receiver is ThisExpression,
          ),
          asyncTarget: _asyncMember(node.interfaceTarget),
        ),
        node.interfaceTarget,
        receiver,
      );
      final tornReturns = _type(returnType);
      final adapter = IrClosure(
        params,
        IrReturn(coerce(tornCall, tornReturns)),
        tornReturns,
        // A tear-off of `message.invoke` keeps `message`: cloned in, moved.
        locals: receiver is ThisExpression
            ? const []
            : bound != null
            ? [bound]
            : _freeLocalsIn(receiver, {}),
        holdsSelf: holds,
      );
      // Typed as the function it is, so the slot's coercion sees it: a
      // `Future<bool> Function(MethodCall)` handed to a `Future<dynamic>
      // Function(MethodCall)` slot gets its result mapped (run447).
      if (bound != null) {
        final typed = adapter
          ..rustType = IrType.function([
            for (final p in params) p.type,
          ], tornReturns);
        return IrBlockValue([
          IrLocalDecl(bound, boundInit!.rustType, boundInit),
        ], typed)..rustType = typed.rustType;
      }
      return adapter
        ..rustType = IrType.function([
          for (final p in params) p.type,
        ], _type(returnType));
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
      // A value declared without an initializer and read after a labelled
      // block -- a switch expression's arms, each `if (..) { #t = ..;
      // break; }` -- is definitely assigned, which Dart checked; so the
      // paths that leave the block without assigning it are dead, and
      // Rust is told so where it cannot see it (125 E0381 at ws425).
      final definite =
          bound != null &&
          initial == null &&
          value is VariableGet &&
          value.variable == bound;
      // The statements first: they declare the temporaries the value
      // reads (a switch expression's `#0`; 420 refusals the round the
      // value was lowered first, ws540).
      final lowered = [
        for (final s in statements)
          if (definite &&
              s is LabeledStatement &&
              _fallsOutUnassigned(s, bound))
            IrLabeled(
              _labelFor(s),
              IrBlock([statement(s.body), IrExprStmt(_noCaseMatched)]),
            )
          else
            statement(s),
      ];
      final produced = expression(value);
      // A block is typed as its value is, where that is known: Kernel's
      // type for the block may be wider (see the bound read above).
      return IrBlockValue(lowered, produced)..rustType = produced.rustType;
    }

    final previous = _cascade;
    final previousStatic = _cascadeStatic;
    _cascade = bound;
    // A cascade on a static filled in place acts on the static itself
    // (`log..add(b)..add(c)`, the statmut fixture): every step names it,
    // and nothing is bound.
    _cascadeStatic = _mutatedStaticOf(initial) ? expression(initial) : null;
    try {
      final steps = <IrStmt>[
        // A cascade on a local shares it: `v..setValues(..)` and `v` read
        // again after (`use of moved value: v`, vector_math).
        if (_cascadeStatic == null)
          IrLocalDecl(
            _cascadeName,
            _type(bound.type),
            _widened(initial, null, expression(initial)),
          ),
        for (final s in statements.skip(1)) statement(s),
      ];
      return IrBlockValue(steps, _cascadeRead())..rustType = _type(bound.type);
    } finally {
      _cascade = previous;
      _cascadeStatic = previousStatic;
    }
  }

  /// The cascade's receiver: the bound local, or the static it acts on.
  IrExpr _cascadeRead() => _cascadeStatic ?? IrLocal(_cascadeName);

  /// The element type of a `dart:core` list literal factory
  /// (`_GrowableList._literalN<E>(..)`, `_List._literalN`), or null for
  /// any other invocation.
  DartType? _coreListLiteral(StaticInvocation node) {
    final target = node.target;
    final owner = target.enclosingClass;
    if (owner == null ||
        !(owner.name == '_GrowableList' || owner.name == '_List') ||
        !target.name.text.startsWith('_literal') ||
        target.enclosingLibrary.importUri.toString() != 'dart:core') {
      return null;
    }
    return node.arguments.types.singleOrNull ?? const DynamicType();
  }

  /// `dynamic`, `Object?`, `void`: a slot that takes anything.
  static bool _isTopType(DartType t) =>
      t is DynamicType ||
      t is VoidType ||
      (t is InterfaceType &&
          t.classNode.name == 'Object' &&
          t.nullability == Nullability.nullable);

  /// `let` temporaries that stand for the place they were bound to.
  final _letAliases = <Variable, Expression>{};

  /// A local's read, or a static's that is filled in place.
  bool _isAliasablePlace(Expression e) {
    var bare = e;
    while (bare is FileUriExpression) {
      bare = bare.expression;
    }
    if (bare is VariableGet) return !_temporaries.containsKey(bare.variable);
    // A field of `this` is a place too: `=> _map[v] = ..` binds `this._map`
    // in a temporary and inserted into a clone of it (the listgen
    // fixture); acting on the field is acting on the object.
    if (bare is InstanceGet &&
        bare.receiver is ThisExpression &&
        bare.interfaceTarget is Field) {
      return true;
    }
    return _mutatedStaticOf(bare);
  }

  /// The static the cascade acts on directly, when it is one filled in
  /// place; null otherwise.
  IrExpr? _cascadeStatic;

  bool _mutatedStaticOf(Expression e) {
    var bare = e;
    while (bare is FileUriExpression) {
      bare = bare.expression;
    }
    if (bare is! StaticGet) return false;
    final target = bare.target;
    return target is Field &&
        target.isStatic &&
        _mutatedStatics.contains(target);
  }

  /// Whether control leaves the labelled block only by falling out of an
  /// else-less `if` at its end, with nothing before it assigning `bound`
  /// unconditionally: then the fall-out is the dead path of a definite
  /// assignment.
  static bool _fallsOutUnassigned(
    LabeledStatement node,
    DeclaredVariable bound,
  ) {
    final body = node.body;
    if (body is! Block || body.statements.isEmpty) return false;
    // The last arm may sit in a block of its own with the variables its
    // pattern binds (`{ final double lower; final double upper; if (..)
    // {..} }` for a record pattern, `scaleFontSize`, ws473): the block's
    // last statement is the arm, the ones before it are looked at too.
    // ..and an arm the AOT compiler removed is left as `{ ; }` after the
    // last real one (the `null` arm of `Typography._withPlatform`, whose
    // callers never pass null, run566): trailing empties say nothing.
    final before = <Statement>[];
    Statement? last = _lastMeaningful(body.statements, before);
    while (last is Block && last.statements.isNotEmpty) {
      last = _lastMeaningful(last.statements, before);
    }
    if (last is! IfStatement || last.otherwise != null) return false;
    for (final s in before) {
      if (s is ExpressionStatement) {
        final e = s.expression;
        if (e is VariableSet && e.variable == bound) return false;
      }
    }
    return true;
  }

  /// The last statement of `statements` that says anything, with the ones
  /// before it added to `before`; null when none does.
  static Statement? _lastMeaningful(
    List<Statement> statements,
    List<Statement> before,
  ) {
    var end = statements.length;
    while (end > 0 && _saysNothing(statements[end - 1])) {
      end--;
    }
    if (end == 0) return null;
    before.addAll(statements.take(end - 1));
    return statements[end - 1];
  }

  static bool _saysNothing(Statement s) =>
      s is EmptyStatement || (s is Block && s.statements.every(_saysNothing));

  static final _noCaseMatched = IrLiteral(
    'unreachable!("dart2rust: no case of an exhaustive switch matched")',
    IrType('raw'),
  );

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

  /// A nullable, kept type parameter of the declaration being lowered, as
  /// a slot: in a generic declaration's signature or field it is spelled
  /// `<T as DartNullable>::Or` (`IrType.projected`) -- Dart's `T?` with
  /// `T` bound to `X?` is `X?`, one `Option` layer -- and a value crosses
  /// it through `IrNullableOf`. Only the class's or the member's own
  /// parameters: another declaration's `T` is not a name here.
  bool _projectedSlot(DartType? t) {
    if (t is! TypeParameterType ||
        t.nullability != Nullability.nullable ||
        _erasedParameter(t.parameter)) {
      return false;
    }
    final declaration = t.parameter.declaration;
    final member = _member;
    if (!identical(declaration, _lowering) &&
        !identical(declaration, member) &&
        !(member != null && identical(declaration, member.function))) {
      return false;
    }
    return !_spelledAsBound(t.parameter);
  }

  /// A parameter `_type` spells as its bound rather than as itself (the
  /// scalar and the list bounds): no Rust type parameter to project.
  bool _spelledAsBound(TypeParameter p) {
    final bound = p.bound;
    return bound is InterfaceType &&
        const {
          'String',
          'int',
          'double',
          'bool',
          'Iterable',
          'List',
        }.contains(bound.classNode.name);
  }

  /// Whether a value crosses a projected slot at a *use*: the declaration
  /// says `T?` and what this use puts in for `T` is a non-nullable type
  /// parameter of the code here -- then the slot is `<U as DartNullable>::
  /// Or` where the code has an `Option<U>`. For `U?` or a concrete type
  /// the slot already *is* the `Option` the code has.
  bool _crossing(DartType? declared, DartType? binding) {
    if (declared is! TypeParameterType ||
        declared.nullability != Nullability.nullable ||
        _erasedParameter(declared.parameter)) {
      return false;
    }
    // A nullable `U?` put in crosses too, now that a type argument `U?`
    // is spelled projected (`_erasedArguments`): the slot is `<U as
    // DartNullable>::Or` either way, and the code has an `Option<U>`.
    if (binding is! TypeParameterType) return false;
    return _projectedSlot(
      binding.withDeclaredNullability(Nullability.nullable),
    );
  }

  IrExpr _acrossBinding(
    IrExpr value,
    DartType? declared,
    DartType? binding, {
    required bool toOption,
  }) {
    if (!_crossing(declared, binding)) return value;
    if (value.rustType?.projected == true) return value;
    final held = binding!.withDeclaredNullability(Nullability.nullable);
    return IrNullableOf(value, _type(binding).name, toOption: toOption)
      ..rustType = toOption ? _type(held) : _edgeType(held);
  }

  /// What a member access puts in for a declared type parameter: the
  /// member's own by the call's type arguments, the class's by the
  /// receiver's.
  DartType? _bindingOf(
    DartType? declared,
    Member member,
    Expression receiver, [
    Arguments? args,
  ]) {
    if (declared is! TypeParameterType) return null;
    final p = declared.parameter;
    final fn = member.function;
    if (fn != null && fn.typeParameters.contains(p)) {
      if (args == null || args.types.length != fn.typeParameters.length) {
        return null;
      }
      return args.types[fn.typeParameters.indexOf(p)];
    }
    final env = typeEnvironment;
    final receiverType = receiver is ThisExpression
        ? (env == null
              ? null
              : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable))
        : _staticType(receiver);
    return _keptFor(member.enclosingClass, receiverType)[p];
  }

  /// What an argument's slot puts in for the callee's type parameter: the
  /// dispatch's receiver for a class's, the call's type arguments for the
  /// callee's own.
  /// The constructor whose arguments are being lowered, with what the
  /// construction puts in for its class's parameters: `Foo<T>(..)` by the
  /// call's type arguments, `super(..)` by this class's supertype.
  FunctionNode? _constructedCallee;
  Map<TypeParameter, DartType> _constructedArgs = const {};

  List<IrExpr> _constructing(
    FunctionNode callee,
    Map<TypeParameter, DartType> args,
    List<IrExpr> Function() lower,
  ) {
    final wasCallee = _constructedCallee;
    final wasArgs = _constructedArgs;
    _constructedCallee = callee;
    _constructedArgs = args;
    try {
      return lower();
    } finally {
      _constructedCallee = wasCallee;
      _constructedArgs = wasArgs;
    }
  }

  /// What this class's supertype puts in for a base's parameters.
  Map<TypeParameter, DartType> _superBinding(Class cls, Class base) {
    final env = typeEnvironment;
    if (env == null || base.typeParameters.isEmpty) return const {};
    final asBase = env.hierarchy.getTypeAsInstanceOf(
      cls.getThisType(env.coreTypes, Nullability.nonNullable),
      base,
    );
    if (asBase is! InterfaceType) return const {};
    return {
      for (
        var i = 0;
        i < base.typeParameters.length && i < asBase.typeArguments.length;
        i++
      )
        base.typeParameters[i]: asBase.typeArguments[i],
    };
  }

  DartType? _argumentBinding(FunctionNode? callee, DartType? declared) {
    if (declared is! TypeParameterType || callee == null) return null;
    final p = declared.parameter;
    if (callee.typeParameters.contains(p)) {
      return identical(callee, _genericCallee) ? _genericArgs[p] : null;
    }
    // An erased parameter of the constructed class is spelled as its
    // bound, whatever the call put in for it (`Entry<S>(v)` with `Entry.T`
    // erased takes an `Rc<dyn Object>`, the outparam fixture).
    if (identical(callee, _constructedCallee)) {
      return _erasedParameter(p) ? null : _constructedArgs[p];
    }
    final landing = _dispatchMember;
    if (landing == null || !identical(callee, _dispatchInterface)) return null;
    return _keptFor(landing.enclosingClass, _dispatchReceiverType)[p];
  }

  /// The classes above one, nearest first, through `extends`, `with` and
  /// `implements`.
  Iterable<Class> _kernelAncestors(Class c) sync* {
    final seen = <Class>{};
    final work = [c];
    while (work.isNotEmpty) {
      final k = work.removeLast();
      for (final st in [
        if (k.supertype != null) k.supertype!,
        if (k.mixedInType != null) k.mixedInType!,
        ...k.implementedTypes,
      ]) {
        final a = st.classNode;
        if (seen.add(a)) {
          yield a;
          work.add(a);
        }
      }
    }
  }

  /// A signature's or a field's type: projected where `_projectedSlot`.
  IrType _edgeType(DartType t) {
    final ir = _type(t);
    return _projectedSlot(t)
        ? IrType(ir.name, nullable: true, projected: true)
        : ir;
  }

  IrType _edgeReturnType(FunctionNode function) =>
      function.returnType is NeverType
      ? const IrType('Never')
      : _edgeType(function.returnType);

  /// `value` across a projected slot: into the body's `Option<T>` from the
  /// spelled `T?` (`toOption`), or back out. Itself for any other slot.
  IrExpr _acrossEdge(IrExpr value, DartType? slot, {required bool toOption}) {
    if (!_projectedSlot(slot)) return value;
    // Already the spelled `T?` (a constructor parameter): nothing to cross.
    if (value.rustType?.projected == true) return value;
    return IrNullableOf(value, _type(slot!).name, toOption: toOption)
      ..rustType = toOption ? _type(slot) : _edgeType(slot);
  }

  /// A body behind its projected parameters: each re-bound, in the same
  /// scope, as the `Option<T>` the body reads and writes.
  IrStmt _withEdgeParams(
    FunctionNode fn,
    IrStmt body, {
    List<DartType>? positional,
  }) {
    final prologue = <IrStmt>[];
    void rebind(String name, DartType type) {
      if (!_projectedSlot(type)) return;
      final held = _type(type);
      prologue.add(
        IrLocalDecl(
          name,
          held,
          IrNullableOf(
            IrLocal(name)..rustType = _edgeType(type),
            held.name,
            toOption: true,
          )..rustType = held,
        ),
      );
    }

    // The type the *signature* declared, which is what the body has in
    // hand: a mixin copy takes its parameter types from the declaration it
    // was copied from (`_declaredParamTypes`), and those name the
    // declaration's own type parameters -- not this copy's, which is what
    // `_projectedSlot` asks about. Read from `p.type` instead, the prologue
    // unprojected a parameter the signature had spelled `Option<U>`
    // (`_OverridableActionMixin._getOverrideAction`, 3 at ws798).
    for (final (i, p) in fn.positionalParameters.indexed) {
      rebind(_paramName(p), positional?[i] ?? _declaredParamTypes[p] ?? p.type);
    }
    for (final p in fn.namedParameters) {
      if (!_inspectorOnly(p.parameterName)) {
        rebind(p.parameterName, _declaredParamTypes[p] ?? p.type);
      }
    }
    if (prologue.isEmpty) return body;
    return IrBlock([
      ...prologue,
      if (body is IrBlock) ...body.statements else body,
    ]);
  }

  /// Whether a callee is translated code, whose signature spells a `T?`
  /// projected; a prelude member's Rust is its own.
  bool _translatedCallee(FunctionNode? callee) {
    final member = callee?.parent;
    if (member is! Member) return false;
    final owner = member.enclosingClass;
    if (owner != null) return _translatedClass(owner);
    final uri = member.enclosingLibrary.importUri;
    return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
  }

  /// The declared return of the member whose body is being lowered, for
  /// its own `return`s to cross; null inside a closure, whose function
  /// type is spelled with `Option<T>`.
  DartType? _edgeReturn;

  /// A `dynamic` closure parameter takes the expected function type's, when
  /// there is one at that position (see `_expectedFunction`).
  /// Closure parameters whose Rust type is the *expected* one rather than
  /// the declared (see `_closureParamType`): a read of one has that type,
  /// not what Kernel says, and an argument made of it is widened from it.
  final Map<Variable, DartType> _retyped = {};

  /// A parameter of a mixin application's copy of a method, typed by the
  /// mixin's own declaration (see `_lowerProcedure`'s `signature`): its
  /// reads are of that type, the trait's.
  final Map<Variable, DartType> _declaredParamTypes = {};

  IrType _closureReturnType(FunctionType? expected, FunctionNode fn) {
    if (expected != null) {
      try {
        return _type(expected.returnType);
      } on Unsupported {
        // Fall through to the declared one.
      }
    }
    return _type(fn.returnType);
  }

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

  /// A tear-off's type: the function's own signature, so a slot of
  /// another function type gets its adapter from `coerce` (`TextStyle.lerp`
  /// handed to `WidgetStateProperty.lerp<TextStyle?>`, whose `T?` is one
  /// `Option` deeper; 161 tear-offs typed `dynamic` at ws387).
  IrType? _functionRefType(Member target) {
    final function = target.function;
    if (function == null) return null;
    try {
      var type = function.computeFunctionType(Nullability.nonNullable);
      // A generative constructor's function returns nothing in Kernel;
      // as a value it makes an instance of its class.
      final cls = target.enclosingClass;
      if (target is Constructor && cls != null) {
        type = FunctionType(
          type.positionalParameters,
          InterfaceType(cls, Nullability.nonNullable, [
            for (final p in cls.typeParameters)
              TypeParameterType(p, Nullability.nonNullable),
          ]),
          Nullability.nonNullable,
          namedParameters: type.namedParameters,
          requiredParameterCount: type.requiredParameterCount,
        );
      }
      return _type(type);
    } on Unsupported {
      return null;
    }
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
    // A closure's parameters are an edge like a method's: a `T?` of the
    // enclosing declaration is spelled projected (`<T as DartNullable>::
    // Or`), which is what every slot of function type says, and rebound to
    // the body's `Option<T>` in a prologue (`_withEdgeParams`). Spelled
    // `Option<T>` it did not fit `RadioListTile<T?>`'s `onChanged` when
    // the state's `T` was itself nullable (`_SettingsListItemState.build`,
    // run689).
    final positionalTypes = [
      for (final (i, p) in fn.positionalParameters.indexed)
        _retype(p, _closureParamType(expected, i, p.type)),
    ];
    try {
      final closure = IrClosure(
        [
          for (final (i, p) in fn.positionalParameters.indexed)
            IrParam(
              _paramName(p),
              _projectedSlot(positionalTypes[i])
                  ? _edgeType(positionalTypes[i])
                  : _paramType(positionalTypes[i]),
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
        _withEdgeParams(fn, _lowerBody(fn, body), positional: positionalTypes),
        // The return as the body was lowered against it: the slot's, when
        // a parameter's function type set one (`_lowerBody`'s expected
        // return), else the closure's own. Typing the closure by its own
        // `Color` while its returns were made `Option<..>` for the
        // `Color?` slot put a second `Some` on at the slot (ws448).
        _closureReturnType(expected, fn),
        isAsync: fn.asyncMarker == AsyncMarker.Async,
        captures: copies
            ? [for (final f in finals) IrParam(f.name.text, _type(f.type))]
            : const [],
        locals: _freeLocals(fn),
        holdsSelf: holds,
      );
      // A function value is typed by its own signature -- the parameters
      // as lowered (retyped to the expected ones where they were), the
      // return as declared -- so a slot of another function type gets its
      // adapter from `coerce` (1233 untyped closures at ws387).
      closure.rustType = IrType.function([
        for (final p in closure.params) p.type,
      ], closure.returns);
      return closure;
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
    // A type literal of the enclosing method's observed type parameter
    // reads the hidden `__ty_<i>` (`_typeLiteral`): a local of the method,
    // captured as one.
    final member = _member;
    if (member is Procedure) {
      final values = _typeValues(member);
      if (values.isNotEmpty) {
        final finder = _TypeLiteralFinder(
          member.function.typeParameters.toSet(),
        );
        node.accept(finder);
        for (final t in finder.found) {
          final index = member.function.typeParameters.indexOf(t);
          if (values.contains(index) && !names.contains('__ty_$index')) {
            names.add('__ty_$index');
          }
        }
      }
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

  /// Statics whose value's field is written somewhere in the package
  /// (`Owner.name`, or `.name` for a top-level): the driver marks them
  /// mutable so they live in a cell.
  static final staticFieldWrites = <String>{};

  /// The concrete copy of a mixin declaration's abstract procedure in an
  /// application of the mixin, if the CFE left one there.
  Procedure? _appliedBody(Class mixin, Procedure declared) =>
      _appliedProcedure(mixin, declared.name.text, kind: declared.kind);

  /// A concrete, non-static procedure named `name` in an application of
  /// `mixin` -- the mixin's own method, whether or not the hollow
  /// declaration still lists it (TFA drops the ones it does not need
  /// there: `SchedulerBinding.initInstances` was nowhere in the
  /// declaration and everywhere in the applications).
  Procedure? _appliedProcedure(
    Class mixin,
    String name, {
    ProcedureKind? kind,
  }) {
    // Deduplicated applications (`dart:mixin_deduplication`) may be hollow
    // themselves; the copy is in whichever application kept it.
    for (final application in applications[mixin] ?? const <Class>[]) {
      for (final p in application.procedures) {
        if (p.name.text == name &&
            (kind == null || p.kind == kind) &&
            !p.isAbstract &&
            !p.isStatic) {
          return p;
        }
      }
    }
    return null;
  }

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
  /// The type arguments a `super` call's base carries, in the terms of the
  /// declaration whose body this is (`IrSuperCall.baseArguments`).
  ///
  /// From an application's copy of a mixin body, the base is reached as
  /// the application is an instance of it (`ModalRoute<T>`'s application
  /// is a `TransitionRoute<T>`), and the application's parameters are
  /// mapped back onto the mixin's through the applied type
  /// (`LocalHistoryRoute<T>`). Elsewhere the base is a supertype of the
  /// class itself. Not expressible -- an application argument that is not
  /// a bare parameter -- is empty.
  List<IrType> _superBaseArguments(Class base) {
    if (base.typeParameters.isEmpty) return const [];
    final env = typeEnvironment;
    final lowering = _lowering;
    if (env == null || lowering == null) return const [];
    final enclosing = _member?.enclosingClass;
    final fromApplication = enclosing != null && enclosing.isAnonymousMixin;
    final from = fromApplication ? enclosing : lowering;
    final asBase = env.hierarchy.getTypeAsInstanceOf(
      from.getThisType(env.coreTypes, Nullability.nonNullable),
      base,
    );
    if (asBase is! InterfaceType) return const [];
    var arguments = asBase.typeArguments;
    if (fromApplication && !identical(enclosing, lowering)) {
      final mapped = _inMixinTerms(enclosing, lowering, arguments);
      if (mapped == null) return const [];
      arguments = mapped;
    }
    try {
      return _erasedArguments(base, arguments);
    } on Unsupported {
      return const [];
    }
  }

  /// `types`, spelled with the application's parameters, in the terms of
  /// the mixin's own: the applied type (`LocalHistoryRoute<T_app>`) maps
  /// each application parameter onto the mixin's. Null when an applied
  /// argument is not a bare parameter, or an application parameter is
  /// left over.
  List<DartType>? _inMixinTerms(
    Class application,
    Class mixin,
    List<DartType> types,
  ) {
    Supertype? applied;
    for (final t in application.implementedTypes) {
      if (t.classNode == mixin) applied = t;
    }
    if (applied == null) return null;
    final map = <TypeParameter, DartType>{};
    for (var i = 0; i < applied.typeArguments.length; i++) {
      final a = applied.typeArguments[i];
      if (a is! TypeParameterType ||
          !application.typeParameters.contains(a.parameter) ||
          i >= mixin.typeParameters.length) {
        return null;
      }
      map[a.parameter] = TypeParameterType(
        mixin.typeParameters[i],
        Nullability.nonNullable,
      );
    }
    final substitution = Substitution.fromMap(map);
    final mapped = [for (final t in types) substitution.substituteType(t)];
    for (final t in mapped) {
      if (_mentionsForeignParameter(t, application.typeParameters)) {
        return null;
      }
    }
    return mapped;
  }

  /// The traits a mixin is applied over in *every* application of it, as
  /// its trait's supertraits.
  ///
  /// A mixin's bodies come from an application (`_appliedBody`), and a
  /// `super` call in one dispatches to the previous mixin of that
  /// application (`_realOwner`), not to the `on` clause: the trait's
  /// default `init_instances` calling `scheduler_binding_super_init_
  /// instances(self)` needs `Self: GestureBinding`, which a method-level
  /// `where Self:` cannot say on a dispatchable method (E0038). What every
  /// application of the mixin puts under it, the trait can require --
  /// the closed world has no application that does otherwise. The
  /// arguments come from the first application, in the mixin's terms.
  List<IrType> _appliedOver(Class mixin) {
    final env = typeEnvironment;
    final apps = applications[mixin];
    if (env == null || apps == null || apps.isEmpty) return const [];
    List<Class> under(Class application) {
      final chain = <Class>[];
      var c = application.superclass;
      while (c != null) {
        if (c.isAnonymousMixin) {
          for (final t in c.implementedTypes) {
            chain.add(t.classNode);
          }
          c = c.superclass;
        } else {
          chain.add(c);
          break;
        }
      }
      return chain;
    }

    var common = under(apps.first).toSet();
    for (final a in apps.skip(1)) {
      common = common.intersection(under(a).toSet());
    }
    final already = {
      for (final t in mixin.implementedTypes) t.classNode,
      for (final t in mixin.onClause) t.classNode,
    };
    final first = apps.first;
    final thisType = first.getThisType(env.coreTypes, Nullability.nonNullable);
    final found = <IrType>[];
    for (final x in under(first)) {
      if (!common.contains(x) ||
          already.contains(x) ||
          x.name == 'Object' ||
          !_translatedClass(x) ||
          !_abstractLike(x)) {
        continue;
      }
      final asX = env.hierarchy.getTypeAsInstanceOf(thisType, x);
      if (asX is! InterfaceType) continue;
      final mapped = _inMixinTerms(first, mixin, asX.typeArguments);
      if (mapped == null) continue;
      try {
        found.add(_type(InterfaceType(x, Nullability.nonNullable, mapped)));
      } on Unsupported {
        continue;
      }
    }
    return found;
  }

  static bool _mentionsForeignParameter(
    DartType t,
    List<TypeParameter> foreign,
  ) {
    if (t is FutureOrType) {
      return _mentionsForeignParameter(t.typeArgument, foreign);
    }
    if (t is RecordType) {
      return t.positional.any((a) => _mentionsForeignParameter(a, foreign)) ||
          t.named.any((n) => _mentionsForeignParameter(n.type, foreign));
    }
    if (t is TypeParameterType) return foreign.contains(t.parameter);
    if (t is InterfaceType) {
      return t.typeArguments.any((a) => _mentionsForeignParameter(a, foreign));
    }
    if (t is FunctionType) {
      return _mentionsForeignParameter(t.returnType, foreign) ||
          t.positionalParameters.any(
            (a) => _mentionsForeignParameter(a, foreign),
          ) ||
          t.namedParameters.any(
            (n) => _mentionsForeignParameter(n.type, foreign),
          );
    }
    return false;
  }

  /// A call's own type arguments, spelled; empty when one cannot be.
  List<IrType> _typeArgumentsOf(Arguments arguments) {
    try {
      return [for (final t in arguments.types) _type(t)];
    } on Unsupported {
      return const [];
    }
  }

  /// The symbol an `external` member's `@Native` annotation registers it
  /// under (`PlatformConfigurationNativeApi::SetNeedsReportTimings`), as
  /// the CFE leaves it: a `pragma("cfe:ffi:native-marker", Native<..>(
  /// symbol: ..))`. Null for an external with no such annotation.
  /// A `@Native` member through the one boundary the runtime answers
  /// (`dart_native` in the prelude): the symbol the engine registers it
  /// under, the arguments as objects, and whether a value comes back. The
  /// generated code sees only the Dart signature; what the symbol does is
  /// the native host's (run455). Null where the member has no symbol or a
  /// signature the boundary cannot spell -- the caller's refusal then.
  /// Both the `external` member and the one the AOT FFI transform gave a
  /// body (`__sendPlatformMessage`, run497) come here: the transform
  /// leaves the marker on the member.
  IrStmt? _nativeBoundary(FunctionNode function, Member member, String name) {
    final symbol = _nativeSymbol(member);
    if (symbol == null) return null;
    {
      try {
        // As `dynamic` slots: a nullable handle (`oldLayer?._nativeLayer` into
        // `SceneBuilder._pushTransform`, run552) goes over as the `Null`
        // object where an `Object` slot unwrapped it.
        final args = [
          for (final p in function.positionalParameters)
            coerce(
              IrLocal(_paramName(p))..rustType = _type(p.type),
              IrType('dynamic'),
            ),
        ];
        final returns = function.returnType;
        final symbolText = IrLiteral(symbol, const IrType('String'));
        final passed = IrListLiteral(args, IrType('dynamic'));
        if (returns is VoidType || returns is NeverType) {
          final call = IrStaticCall(null, 'dart_native', [
            symbolText,
            passed,
            IrLiteral('false', const IrType('bool')),
          ], fails: true)..rustType = const IrType('dynamic');
          if (returns is VoidType) return IrBlock([IrExprStmt(call)]);
          return IrBlock([IrExprStmt(call), IrExprStmt(_unreachable)]);
        }
        // A value comes back as the declared type (`NativeAnswer`):
        // the host's object read as it, or the absent engine's value.
        final type = _type(returns);
        final valued = IrStaticCall(
          null,
          'dart_native_as',
          [symbolText, passed],
          fails: true,
          typeArguments: [type],
        )..rustType = type;
        return IrBlock([IrReturn(valued)]);
      } on Unsupported catch (error) {
        // A signature the boundary cannot spell: the refusal below.
        if (Platform.environment['DART2RUST_TRACE_NATIVE'] != null) {
          stderr.writeln('TRACE_NATIVE $name unsupported: $error');
        }
      }
    }
    return null;
  }

  String? _nativeSymbol(Member member) {
    for (final a in member.annotations) {
      if (a is! ConstantExpression) continue;
      final c = a.constant;
      if (c is! InstanceConstant || c.classNode.name != 'pragma') continue;
      InstanceConstant? options;
      var marker = false;
      for (final e in c.fieldValues.entries) {
        final field = e.key.asField.name.text;
        final v = e.value;
        // Two spellings: the marker the CFE leaves on the member written
        // (`cfe:ffi:native-marker`), and the pragma on the `$Method$
        // FfiNative` external its transform synthesizes (`vm:ffi:native`).
        if (field == 'name' &&
            v is StringConstant &&
            (v.value == 'cfe:ffi:native-marker' ||
                v.value == 'vm:ffi:native')) {
          marker = true;
        }
        if (field == 'options' && v is InstanceConstant) options = v;
      }
      if (!marker || options == null || options.classNode.name != 'Native') {
        continue;
      }
      for (final e in options.fieldValues.entries) {
        final v = e.value;
        if (e.key.asField.name.text == 'symbol' && v is StringConstant) {
          return v.value;
        }
      }
    }
    return null;
  }

  /// `super.m` as a value: a closure over the super call, its parameters
  /// the method's (see the `InstanceTearOff` case).
  IrExpr _superTearOff(SuperPropertyGet node, Procedure target) {
    final fn = target.function;
    if (fn.typeParameters.isNotEmpty) {
      throw Unsupported(
        'a generic super method used as a value',
        _sample(node),
      );
    }
    if (!_counted) {
      throw Unsupported(
        'a super method used as a value in a class with no handle',
        _sample(node),
      );
    }
    final torn = _staticType(node);
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
    final call = SuperMethodInvocation(
      ThisExpression(),
      node.name,
      Arguments(
        [for (final p in fn.positionalParameters) VariableGet(p)],
        named: [
          for (final p in fn.namedParameters)
            NamedExpression(p.parameterName, VariableGet(p)),
        ],
      ),
      target,
    );
    final tornReturns = _type(returnType);
    final lowered = expression(call);
    return IrClosure(
        params,
        IrReturn(coerce(lowered, tornReturns)),
        tornReturns,
        locals: const [],
        // Over a handle to `this`, as any closure calling into it is.
        holdsSelf: true,
      )
      ..rustType = IrType.function([
        for (final p in params) p.type,
      ], tornReturns);
  }

  /// The `Option<T>` a `T?` operand of `dart_as_own` is: a field's or a
  /// parameter's is the projected `<T as DartNullable>::Or` and goes
  /// through `option`; a local's is the `Option<T>` already (`arg as T`
  /// on a captured `T? arg`, the throttle fixture).
  IrExpr _asOwnOption(IrExpr operand, TypeParameterType parameter) {
    final held = operand.rustType;
    final name = parameter.parameter.name ?? 'T';
    if (held != null && held.projected) {
      return IrNullableOf(operand, name, toOption: true)
        ..rustType = IrType(name, nullable: true);
    }
    return operand;
  }

  Class? _realOwner(Member target, String name) {
    // A super call in a mixin's body names the `on` constraint's member
    // (`BindingBase.initInstances`), but dispatches to the *actual*
    // superclass of the application the body sits in: the walk starts
    // there, at the previous mixin in the chain (`RendererBinding`'s
    // `super.initInstances()` reaching `SemanticsBinding`'s, run438).
    final enclosing = _member?.enclosingClass;
    final fromApplication = enclosing?.isAnonymousMixin ?? false;
    var owner = fromApplication ? enclosing!.superclass : target.enclosingClass;
    while (owner != null) {
      if (owner.isAnonymousMixin) {
        // Not `mixedInClass`: with `--target=flutter` the CFE *applies*
        // the mixin, copying its members into this class and clearing
        // `mixedInType`, so that getter is null by the time a dill is
        // read. What survives is `implementedTypes` -- the applied
        // mixins, in the order they were written -- which is how `is
        // Scaled` still answers. Later mixins win, so the search runs
        // backwards.
        for (final applied in owner.implementedTypes.reversed) {
          final mixin = applied.classNode;
          // A hollow mixin declares the member when an application of it
          // holds the body (`_appliedBody`): `super.initInstances()` in
          // `WidgetsBinding` fell through every binding mixin to
          // `BindingBase`, and `SemanticsBinding.initInstances` never ran
          // (run438's `None` in `_semanticsEnabled`).
          if (mixin.members.any((m) => m.name.text == name && !m.isAbstract) ||
              _appliedProcedure(mixin, name) != null) {
            return mixin;
          }
        }
        owner = owner.superclass;
        continue;
      }
      // A real class: from an application's body, on up past the ones
      // that do not declare the member -- before *and* after the
      // anonymous applications in between (`RenderBox` for `attach`,
      // which `RenderObject` declares; `RenderSemanticsAnnotations`'
      // applied `super.describeSemanticsConfiguration` climbed
      // `RenderProxyBox`'s applications and stopped at `RenderBox`,
      // run575). A body of its own names the declaring class already.
      if (!fromApplication ||
          owner.members.any((m) => m.name.text == name && !m.isAbstract)) {
        return owner;
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
      final previousStatic = _cascadeStatic;
      _cascade = node.variable;
      _cascadeStatic = _mutatedStaticOf(initial) ? expression(initial) : null;
      try {
        return IrBlockValue([
          if (_cascadeStatic == null)
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
        ], _cascadeRead())..rustType = _type(node.variable.type);
      } finally {
        _cascade = previous;
        _cascadeStatic = previousStatic;
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
        // `null?.m` is `null`: type flow analysis folds an always-null
        // value into the literal, and walking the access from there left a
        // `None.as_ref().map(|it| ..)` whose closure parameter had no type
        // to be inferred from (8 "type annotations needed" at ws777). The
        // whole access is the null the receiver is.
        if (_isNull(value)) return _nullLiteral();
        final lowered = expression(value);
        // `x?.m` on a `dynamic` (an `Object?`, ws502): its null is the
        // `Null` object, asked by the prelude (`dart_nullable`), and the
        // value inside is what the body binds.
        final loweredType = lowered.rustType;
        final receiver =
            loweredType != null &&
                loweredType.name == 'dynamic' &&
                !loweredType.nullable
            ? (IrCall(lowered, '!nullable', const [])
                ..rustType = const IrType('dynamic', nullable: true))
            : lowered;
        final previous = _bound;
        final previousType = _boundType;
        _bound = node.variable;
        final receiverType = receiver.rustType;
        _boundType = receiverType == null
            ? null
            : IrType(receiverType.name, arguments: receiverType.arguments);
        try {
          // `oldLayer?._nativeLayer` with `_nativeLayer` a `T?`: one
          // `Option`, not two (8 `Option<Option<..>>` in dart:ui).
          final memberType = _staticType(otherwise);
          final body = expression(otherwise);
          // By the lowered body's own type where it has one: a cascade on
          // the bound (`child?..layout(..)`) is a `RenderBox` here whatever
          // the CFE's temporaries say (`RenderProxyBoxMixin.performLayout`,
          // ws485).
          final bodyType = body.rustType;
          // Typed as what the Rust value is -- the body's `Option` (one
          // layer, see `flatten`) -- not as Kernel's `T?`, which for a
          // `dynamic` body is a bare `dynamic` (`_imageStream?.key ==
          // key`, ws502).
          final flattened =
              bodyType != null &&
              bodyType.nullable &&
              bodyType.name != 'void' &&
              bodyType.name != '()';
          final IrType? resultType = bodyType == null
              ? null
              : flattened
              ? bodyType
              : _nullableIr(bodyType);
          return IrNullAware(
            receiver,
            body,
            // `void` is "nullable" to Kernel; `x?.addListener(..)` is a
            // `map`, not an `and_then` (`Option<_> <= ()`).
            // ..and a `T?` of a type parameter flattens too: `scope?.
            // localizationsState.resourcesFor<T?>(type)` is a `T?`, not an
            // `Option<Option<T>>` (`Localizations.of`, ws482).
            flatten: bodyType != null
                ? bodyType.nullable &&
                      bodyType.name != 'void' &&
                      bodyType.name != '()'
                : memberType != null &&
                      (memberType is InterfaceType ||
                          memberType is TypeParameterType) &&
                      memberType.nullability == Nullability.nullable,
          )..rustType = resultType;
        } finally {
          _boundType = previousType;
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
        // ..and `x as T` on a `T?` of the parameter's own, which the AOT
        // compiler writes in this same shape: the `T` inside, or `T`'s
        // own null where `T` has one (`RestorableValue<double?>.value`,
        // run665), by the prelude (`dart_as_own`).
        final asType = then.type;
        if (asType is TypeParameterType &&
            asType.nullability != Nullability.nullable &&
            !_erasedParameter(asType.parameter)) {
          final own = _type(
            asType.withDeclaredNullability(Nullability.nonNullable),
          );
          return IrStaticCall(
            null,
            'dart_as_own',
            [_asOwnOption(expression(value), asType)],
            fails: true,
            typeArguments: [own],
          )..rustType = own;
        }
        return _nullChecked(expression(value));
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
        // ..two *concrete* classes: a top-typed side (`Object?`, a
        // `dynamic`) takes the general path, where the other side goes
        // behind the handle (ws502).
        if (leftType is InterfaceType &&
            rightType is InterfaceType &&
            leftType.classNode != rightType.classNode &&
            leftType.classNode.name != 'Object' &&
            rightType.classNode.name != 'Object' &&
            body.staticType is InterfaceType &&
            (body.staticType as InterfaceType).classNode.name == 'Object') {
          return IrIfNull(
            IrNullAware(
              expression(value),
              IrStaticCall(null, 'dart_str', [IrBound()]),
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
        // ..and into a function type: a static tear-off whose named
        // parameters are declared in another order than the type sorts
        // them takes its adapter here too (`requestFocusCallback ??
        // FocusTraversalPolicy.defaultTraversalRequestFocusCallback`,
        // run522).
        // ..the type of the *whole* -- Dart's least upper bound -- when the
        // left side is narrower than it: `widget?.notifier ?? fallback`
        // is a `ValueListenable` where `notifier` is a `ValueNotifier`
        // and `fallback` implements only the interface; into the left's
        // type the fallback had no `ValueNotifier` to become
        // (`TickerMode.getValuesNotifier`, run610). Both sides go into
        // it: the left below, mapped through its `Option`.
        final resultType = body.staticType;
        // ..a class with a handle to go up into: `double? ?? 0` is a
        // `num` in Dart and an `f64` here, where the literal takes the
        // left's spelling as before (+6 the round `num` was taken, ws611).
        final lub =
            leftType is InterfaceType &&
                resultType is InterfaceType &&
                leftType.classNode != resultType.classNode &&
                resultType.classNode.name != 'Object' &&
                !scalarNames.contains(resultType.classNode.name)
            ? resultType
            : leftType;
        final into = lub is InterfaceType || lub is FunctionType
            ? (resultNullable
                  ? lub!.withDeclaredNullability(Nullability.nullable)
                  : lub!.withDeclaredNullability(Nullability.nonNullable))
            : null;
        var rightSide = expression(right);
        if (into != null) {
          final widened = _widened(right, into, rightSide);
          rightSide = widened is IrCall && widened.name == '!rc'
              ? IrUpcast(widened.target!, _type(into))
              : widened;
        }
        // A closure as the right arm of a function-typed `??` is boxed:
        // the left arm is the `Rc<dyn Fn>` the slot holds, and a `match`
        // arm does not coerce (`onNavigationNotification ??
        // _defaultOnNavigationNotification`, `WidgetsApp.build`, ws503).
        if (rightSide is IrClosure &&
            (leftType is FunctionType || rightType is FunctionType)) {
          rightSide.boxed = true;
        }
        // `x ?? y` on a `dynamic` (an `Object?`, ws502): its null is the
        // `Null` object, asked by the prelude; and whether the result is
        // still an `Option` is the *Rust* type's answer -- a `dynamic`
        // result is no `Option`.
        // The left side as Dart types it: an erased read (`route.result`
        // on a `Route<int>` whose `T` is erased) is recorded wider, and is
        // narrowed where it is consumed, as a receiver or an argument is.
        var leftSide = expression(value);
        if (leftType is InterfaceType && leftType.classNode.name != 'Object') {
          try {
            leftSide = coerce(leftSide, _type(leftType));
            // ..and up into the whole's type when that is wider (see
            // `lub`), still nullable: the arm that is `None` stays so.
            if (!identical(lub, leftType) && lub is InterfaceType) {
              leftSide = coerce(
                leftSide,
                _type(lub.withDeclaredNullability(Nullability.nullable)),
              );
            }
          } on Unsupported {
            // Unspelled: as it is.
          }
        }
        final leftIr = leftSide.rustType;
        final asked =
            leftIr != null && leftIr.name == 'dynamic' && !leftIr.nullable
            ? (IrCall(leftSide, '!nullable', const [])
                ..rustType = const IrType('dynamic', nullable: true))
            : leftSide;
        final resultIr = _recordedType(body.staticType);
        // A `dynamic` whole whose right side is still an `Option` (a
        // projected `T?` with `T` bound to `dynamic`: `tween.end ??
        // tween.begin` on a `Tween<dynamic>`) is the handle, its null the
        // `Null` object -- the arms agree on that, not on `Option` versus
        // `Rc` (`_constructTweens`, ws614).
        final rightIr = rightSide.rustType;
        if (body.staticType is DynamicType &&
            resultIr != null &&
            !resultIr.nullable &&
            rightIr != null &&
            isNullable(rightIr)) {
          rightSide = IrStaticCall(null, 'dart_option_object', [rightSide])
            ..rustType = const IrType('dynamic');
        }
        return IrIfNull(
          asked,
          rightSide,
          // Whether the whole thing is still nullable is the right side's
          // question: `a ?? b` is non-null exactly when `b` is.
          // The conditional carries its own static type, so no type context
          // has to be built to ask this.
          nullableResult: resultIr != null
              ? resultIr.nullable
              : body.staticType.nullability == Nullability.nullable,
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
    // A temporary bound to a *place* -- a local, a static filled in place
    // -- that the body mutates in place (`let #t = local in #t.clear()`,
    // what TFA leaves of `local?.clear()` once `local` is known non-null,
    // the nullmut fixture): the body acts on the place, and nothing is
    // bound, as a cascade on one does.
    if (_isAliasablePlace(initial) &&
        _TempMutationFinder.mutates(node.variable, node.body)) {
      _letAliases[node.variable] = initial;
      try {
        return expression(node.body);
      } finally {
        _letAliases.remove(node.variable);
      }
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
    final block = IrBlockValue(
      [
        IrLocalDecl(
          name,
          // The post-increment's middle binding is `void` (see `_declare`).
          node.variable.type is VoidType ? null : _type(node.variable.type),
          // A local bound here is shared, not moved: `let __t = key;` and
          // `key` read again two lines on (13 E0382s). Into a `dynamic`
          // binding it is shared into the `Rc<dyn Object>` (`__t: Rc<dyn
          // Object> = true`).
          // ..and widened into the binding's type: `double? t = size?.height`
          // after TFA holds a `double`, and the binding says `Some`.
          _widened(initial, node.variable.type, expression(initial)),
        ),
      ],
      promotedRead
          ? _nullChecked(
              IrLocal(name)..rustType = _recordedType(node.variable.type),
            )
          : expression(letBody),
    );
    // Typed as its value, so a slot adapts the block as it would the
    // value: TFA's `let #t = channel in SystemChannels.menu` (the `??`
    // decided) into a `MethodChannel` field wants the handle (run483).
    final letValue = block.value;
    if (!promotedRead && letValue.rustType != null) {
      block.rustType = letValue.rustType;
    }
    return block;
  }

  /// A conditional in statement position as an `if` (see the expression
  /// statement lowering), through the `Let` the CFE binds its receiver
  /// in. Not the `?.` shape (`#t == null ? null : #t.m()`) nor the `??`
  /// one (`#t == null ? b : #t`): `_let` gives those their own forms
  /// (a null-aware call on a place, a `match`). Null for anything else.
  IrStmt? _conditionalStatement(Expression value) {
    if (value is ConditionalExpression) {
      final then = value.then;
      final otherwise = value.otherwise;
      // A throw anywhere in it -- TFA's "code removed" in a dead tail --
      // keeps the expression form, which spelled it (`_callPopInvoked`,
      // ws613).
      if (value.condition is Throw || then is Throw || otherwise is Throw) {
        return null;
      }
      // An arm that only reads (`#t_isSet ? #t : (#t_isSet = true, #t =
      // ..)`, the CFE's pattern cache) does nothing as a statement, and
      // as one it *moved* the temporary (`__t8;`, +13 at ws613).
      final thenPure = _pureRead(then);
      final otherwisePure = _pureRead(otherwise);
      if (thenPure && otherwisePure) return null;
      IrStmt arm(Expression e) => statement(ExpressionStatement(e));
      // An empty `then` is the other arm under the negated test, as one
      // would write it.
      if (thenPure) {
        return IrIf(
          IrUnary('!', expression(value.condition)),
          arm(otherwise),
          null,
        );
      }
      return IrIf(
        expression(value.condition),
        arm(then),
        otherwisePure ? null : arm(otherwise),
      );
    }
    if (value is Let) {
      final body = value.body;
      final initial = value.variable.initializer;
      if (body is! ConditionalExpression || initial == null) return null;
      final condition = body.condition;
      if (condition is EqualsNull &&
          _isThe(condition.expression, value.variable) &&
          (body.then is NullLiteral ||
              _isThe(body.otherwise, value.variable))) {
        return null;
      }
      if (_isAliasablePlace(initial) &&
          _TempMutationFinder.mutates(value.variable, body)) {
        return null;
      }
      final name = _nameFor(value.variable);
      final declared = IrLocalDecl(
        name,
        value.variable.type is VoidType ? null : _type(value.variable.type),
        _widened(initial, value.variable.type, expression(initial)),
      );
      final rest = _conditionalStatement(body);
      if (rest == null) return null;
      return IrBlock([declared, rest]);
    }
    return null;
  }

  /// An expression with no effect: a read, a literal, `this`.
  static bool _pureRead(Expression e) =>
      e is VariableGet ||
      e is BasicLiteral ||
      e is NullLiteral ||
      e is ConstantExpression ||
      e is ThisExpression;

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
    // The value widens into the type the write lands on (`_writeSlot`): a
    // mixin clone's field, or the trait's setter -- `_cache = s` into a
    // `String?` field is `Some(s)`. A clone's field, being this struct's,
    // is written as a field, not through the trait's setter.
    final slot = _writeSlot(value.interfaceTarget, value.receiver);
    final landing = _landing(value.interfaceTarget, value.receiver);
    final declaredSlot = landing is Procedure && landing.isSetter
        ? landing.function.positionalParameters.single.type
        : landing.setterType;
    final written = _acrossBinding(
      _widened(
        value.value,
        slot,
        expression(value.value),
        slotIr: _writeSlotIr(value.interfaceTarget, value.receiver),
      ),
      declaredSlot,
      _bindingOf(declaredSlot, landing, value.receiver),
      toOption: false,
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
        return IrSetter(
          _cascadeRead(),
          _fieldNameOf(value.interfaceTarget, value.name.text),
          written,
        );
      }
      // The owner is the cascaded value's own class: on a counted one its
      // fields are cells, and without the owner the backend wrote
      // `cascaded.on_down = ..` into an `Rc<RefCell<..>>` (23+23 in
      // `widgets`).
      return IrAssignField(
        _fieldNameOf(value.interfaceTarget, value.name.text),
        written,
        target: IrLocal(_cascadeName),
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
      final receiverClassHere = _staticClass(receiver);
      // ..unless the local's own class is counted (its fields are cells)
      // or the field's class is a trait (its setter): `childParentData.
      // offset = Offset(..)` on a `_ToolbarParentData` local wrote to an
      // `Rc<Cell<Offset>>` (23 at ws325).
      if (receiver is VariableGet &&
          receiver.variable.parent is! FunctionNode &&
          !_closureCallsMethod(value.interfaceTarget.enclosingClass!) &&
          !(receiverClassHere != null &&
              _closureCallsMethod(receiverClassHere)) &&
          !_abstractLike(value.interfaceTarget.enclosingClass!)) {
        return IrAssignField(
          _fieldNameOf(value.interfaceTarget, value.name.text),
          written,
          target: expression(receiver),
          owner:
              receiverClassHere?.name ??
              value.interfaceTarget.enclosingClass?.name,
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
          _fieldNameOf(value.interfaceTarget, value.name.text),
          written,
          target: expression(receiver),
          owner: receiverClass?.name ?? declaring.name,
        );
      }
      // A trait's field on a value local: the setter the trait declares
      // (`IrAssignField.owner` abstract; 19 refusals at ws326).
      if (_abstractLike(declaring)) {
        return IrAssignField(
          _fieldNameOf(value.interfaceTarget, value.name.text),
          written,
          target: expression(receiver),
          owner: declaring.name,
        );
      }
      // A *static* holding a value: `GoogleFonts.config.allowRuntimeFetching
      // = false` writes a field of the value in the static's cell, which
      // makes that static mutable state (the driver flips its cell on:
      // `staticFieldWrites`). The first refusal on the gallery's startup
      // path, in `main` itself (2026-09-05).
      if (receiver is StaticGet &&
          receiver.target is Field &&
          !_closureCallsMethod(declaring) &&
          !_abstractLike(declaring)) {
        final place = expression(receiver);
        if (place is IrStatic || place is IrTopLevel) {
          staticFieldWrites.add(
            place is IrStatic
                ? '${place.owner}.${place.name}'
                : '.${(place as IrTopLevel).name}',
          );
          return IrAssignField(
            _fieldNameOf(value.interfaceTarget, value.name.text),
            written,
            target: place,
            owner: receiverClassHere?.name ?? declaring.name,
          );
        }
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
        _fieldNameOf(value.interfaceTarget, value.name.text),
        written,
        target: expression(value.receiver),
      );
    }
    if (value.interfaceTarget is! Field &&
        !_heldField(value.interfaceTarget, value.receiver)) {
      return IrSetter(
        null,
        _fieldNameOf(value.interfaceTarget, value.name.text),
        written,
        qualifier: _setterQualifier(null, value.interfaceTarget),
      );
    }
    return IrAssignField(
      _fieldNameOf(value.interfaceTarget, value.name.text),
      written,
    );
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
          IrCall(_listReceiver(init.receiver), 'clone', const []),
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
    // ..and a `late` local without an initialiser: assigned on some path
    // and read on another rustc cannot match up (`late Rect
    // floatingActionButtonRect` in `Scaffold`'s layout, E0381, ws527).
    if (init == null &&
        written != null &&
        (written.startsWith('#') ||
            _tryWrites.contains(variable) ||
            variable.isLate) &&
        variable.type is! VoidType &&
        variable.type.nullability != Nullability.nullable) {
      _optionLocals.add(variable);
      return IrLocalDecl(
        name,
        _localIrType(variable),
        _nullLiteral(),
        cell: _capturedWrites.contains(variable),
      );
    }
    // A local of a nullable type declared without an initializer holds
    // Dart's null from the start: `None`, or the `Null` object for a
    // `dynamic`. Left uninitialized, a read Dart guards with its own flag
    // (a pattern's `#0#2` behind `#0#2#isSet`) is one rustc cannot see
    // assigned (E0381, run454).
    // ..by the *Rust* type: an `Object?` is a `dynamic` (ws501).
    final type = variable.type;
    final IrType? startType = init != null || type is VoidType
        ? null
        : _recordedType(type);
    final IrExpr? nullStart = startType == null
        ? null
        : startType.name == 'dynamic' && !startType.nullable
        ? (IrStaticCall(null, 'dart_null_object', const [])
            ..rustType = startType)
        : startType.nullable
        ? IrLiteral('None', const IrType('raw'))
        : null;
    if (variable.type is FunctionType &&
        (init is FunctionExpression || init is InstanceTearOff)) {
      _boxedFunctionLocals.add(name);
    }
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
          ? nullStart
          : _intoDeclaredNum(
              init,
              variable.type,
              _widened(init, variable.type, expression(init)),
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

  /// The CFE's value half of a lowered `late` local: declared without an
  /// initialiser and assigned under its `#isSet` flag, which Rust's
  /// definite-assignment check cannot follow (129 E0381 at ws397). It is
  /// what a `late` local is in Rust terms, an `Option`: `None` until set,
  /// read as `T?` (the coercion rule unwraps where a `T` goes), written
  /// with `Some`.
  final _optionLocals = <Variable>{};

  /// The element type of the receiver `_bound` stands for (see the `?.`
  /// lowering), for narrowing a read of it.
  IrType? _boundType;

  /// Set while a string interpolation's part is lowered.
  bool _inStringPart = false;

  DartType _localType(Variable v) => _optionLocals.contains(v)
      ? v.type.withDeclaredNullability(Nullability.nullable)
      : v.type;

  /// A local's Rust type: an option local's is the `Option` of its
  /// declared type's, whatever Dart's nullable spelling of that type maps
  /// to (`late Object x` is an `Option<Rc<dyn Object>>`, where `Object?`
  /// itself is a `dynamic`, ws497).
  IrType _localIrType(Variable v) {
    final declared = _type(v.type);
    return _optionLocals.contains(v) ? _nullableIr(declared) : declared;
  }

  static IrType _nullableIr(IrType t) {
    if (t.nullable) return t;
    if (t.isFunction)
      return IrType.function(t.parameters!, t.returns!, nullable: true);
    return IrType(
      t.name,
      nullable: true,
      arguments: t.arguments,
      projected: t.projected,
    );
  }

  /// The argument slots of a member on a *typed list* receiver (`Vec<u8>`
  /// here): the element slots take the list's narrow element, not Dart's
  /// `int` -- `bytes.setRange(a, b, other)` on two `Uint8List`s handed the
  /// prelude a `Vec<i64>` (`WriteBuffer._append`, run505). Null for any
  /// other receiver or slot.
  List<IrType?>? _narrowSlots(InstanceInvocation node) {
    final narrow = _narrowElement(_staticType(node.receiver));
    final declaring = node.interfaceTarget.enclosingClass;
    final fn = node.interfaceTarget.function;
    if (narrow == null ||
        declaring == null ||
        fn == null ||
        declaring.enclosingLibrary.importUri.scheme != 'dart') {
      return null;
    }
    IrType? slot(DartType t) {
      if (t is TypeParameterType &&
          declaring.typeParameters.contains(t.parameter)) {
        return IrType(narrow);
      }
      if (t is InterfaceType &&
          (t.classNode.name == 'Iterable' || t.classNode.name == 'List') &&
          t.typeArguments.length == 1) {
        final e = t.typeArguments.single;
        if (e is TypeParameterType &&
            declaring.typeParameters.contains(e.parameter)) {
          return IrType('List', arguments: [IrType(narrow)]);
        }
      }
      return null;
    }

    final slots = [for (final p in fn.positionalParameters) slot(p.type)];
    return slots.any((s) => s != null) ? slots : null;
  }

  /// Whether `c` is one of `dart:typed_data`'s lists (`Uint8List`,
  /// `Float64List`, ..): a `Vec` of its element here.
  static bool _typedList(Class c) =>
      c.enclosingLibrary.importUri.toString() == 'dart:typed_data' &&
      c.name.endsWith('List') &&
      !c.name.startsWith('_');

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
    if (written.startsWith('#') || written == '_') return _nameFor(p);
    // Once renamed, always renamed: reads find the name through
    // `_temporaries` by identity, and a nested closure lowering under its
    // own captured set would otherwise name the same parameter twice.
    final already = _temporaries[p];
    if (already != null) return already;
    // A parameter of a closure that copied a field of `this` in under the
    // field's own name (`IrClosure.captures`): in Rust the parameter
    // shadows the copy, so the body's read of the *field* found the
    // parameter instead. Dart has no such collision -- there the field is
    // `this.child` -- so the copy keeps the name the reads use and the
    // parameter takes a temporary's (`this.child ?? child` in the gallery's
    // `FadeInImagePlaceholder.build`, ws808).
    if (_captured.contains(written)) return _nameFor(p);
    return written;
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

  /// A case body, with its trailing `break` marked as droppable -- the
  /// trailing statement through nested blocks (`{ { switch (..) {..}
  /// break #L; } }`, a nested switch's case).
  IrStmt _caseBody(Statement body) {
    final last = _trailingStatement(body);
    if (last is BreakStatement) _droppableBreaks.add(last);
    return statement(body);
  }

  static Statement _trailingStatement(Statement body) {
    var last = body;
    while (last is Block && last.statements.isNotEmpty) {
      last = last.statements.last;
    }
    return last;
  }

  /// Switches whose `break`s leave a labelled block around the match.
  final _labeledSwitches = <LabeledStatement, String>{};

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
        _listReceiver(iterable),
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
      _listReceiver(iterable),
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
    // ..and when the body holds a switch that leaves early through a
    // labelled block (see the labelled-switch lowering), Rust wants the
    // `continue` labelled too (E0695): the body is the labelled block the
    // CFE wrote, and the continue breaks out of it (`Navigator.
    // _flushHistoryUpdates`, ws582).
    if (hasUpdates || _holdsEarlySwitch(body.body)) {
      return IrLabeled(_labelFor(body), statement(body.body));
    }
    _continueTargets.add(body);
    return statement(body.body);
  }

  static bool _holdsEarlySwitch(Statement body) {
    final finder = _EarlySwitchFinder();
    body.accept(finder);
    return finder.found;
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

  IrExpr _instanceGet(InstanceGet node) => _instanceGetRaw(node);

  /// A member access's receiver, as Dart types it: a value whose recorded
  /// Rust type is wider -- an erased read, `Rc<dyn StatefulWidget>` where
  /// Dart says `Scaffold` -- is narrowed on the way in (`coerce`), which is
  /// what the erased-read narrowing used to do at every such read whether
  /// or not a member was then reached through it.
  /// A receiver asked as a list -- `Iterable`'s members on it, a `for-in`
  /// over it -- when it is a translated class that *is* an `Iterable<E>`
  /// (`Navigator`'s `_History extends Iterable<_RouteEntry>`, ws499): the
  /// list of its elements, `to_list`, which the backend writes from the
  /// class's `iterator`. Any other receiver is itself.
  /// Dart names of the list members that change the receiver: the
  /// receiver of one is the place itself, never a narrowing copy.
  static const _mutatingListNames = {
    '[]=',
    'add',
    'addAll',
    'insert',
    'insertAll',
    'remove',
    'removeAt',
    'removeLast',
    'removeWhere',
    'retainWhere',
    'clear',
    'setRange',
    'fillRange',
    'replaceRange',
    'removeRange',
    'setAll',
    'sort',
    'shuffle',
    'addFirst',
    'addLast',
    'removeFirst',
    'length',
  };

  IrExpr _listReceiver(Expression e, [String? member]) {
    // A mutating member's receiver as it is: an erased `List<ChildType>`
    // read as a `List<Sliver>` is a narrowing *copy*, and `children.add
    // (x)` pushed into it (the erased tear-off fixture, ws528). The element
    // goes in as the slot's type and rustc upcasts it to the erased one.
    if (member != null && _mutatingListNames.contains(member)) {
      return expression(e);
    }
    final lowered = _receiver(e);
    final static = _staticType(e);
    if (static is! InterfaceType || !_translatedClass(static.classNode)) {
      return lowered;
    }
    final element = _iterableElement(static);
    if (element == null) return lowered;
    return IrCall(lowered, '__to_list', const [])
      ..rustType = IrType('List', arguments: [_typeNested(element)]);
  }

  /// `IrClass.iterableElement`: the class's own `Iterable<E>` element.
  IrType? _iterableElementIr(Class node) {
    final env = typeEnvironment;
    if (env == null || !_translatedClass(node)) return null;
    final element = _iterableElement(
      node.getThisType(env.coreTypes, Nullability.nonNullable),
    );
    if (element == null) return null;
    try {
      return _typeNested(element);
    } on Unsupported {
      return null;
    }
  }

  /// The `E` of the `Iterable<E>` a translated class is, or null.
  DartType? _iterableElement(InterfaceType type) {
    final env = typeEnvironment;
    if (env == null) return null;
    final iterable = env.coreTypes.iterableClass;
    if (identical(type.classNode, iterable)) return null;
    final asIterable = env.hierarchy.getTypeAsInstanceOf(type, iterable);
    if (asIterable is! InterfaceType || asIterable.typeArguments.length != 1) {
      return null;
    }
    return asIterable.typeArguments.single;
  }

  IrExpr _receiver(Expression e) {
    final lowered = expression(e);
    var static = _staticType(e);
    // A receiver typed by a type parameter is its bound here (`_type`
    // says so too): `widget.duration` in `ImplicitlyAnimatedWidgetState<T
    // extends ImplicitlyAnimatedWidget>` reads `State<T>.widget` as the
    // `Rc<dyn StatefulWidget>` the trait returns, and is narrowed to the
    // `ImplicitlyAnimatedWidget` this class's `T` promises (`AnimatedTheme`
    // in `MaterialApp`, ws538).
    // ..only to a translated abstract bound, whose trait the member is
    // reached through. To `Object` there is nothing to narrow to, and
    // boxing `key` for `key.hashCode` moved it and hashed the box (the
    // hashtrie fixture, ws544).
    var hops = 0;
    while (static is TypeParameterType && hops++ < 8) {
      final bound = static.parameter.bound;
      if (bound is InterfaceType &&
          !(_translatedClass(bound.classNode) &&
              _abstractLike(bound.classNode))) {
        return lowered;
      }
      static = bound;
    }
    if (!coerceByType || static is! InterfaceType) return lowered;
    try {
      final out = coerce(lowered, _type(static));
      return out;
    } on Unsupported {
      return lowered;
    }
  }

  IrExpr _instanceGetRaw(InstanceGet node) {
    final name = _fieldNameOf(node.interfaceTarget, node.name.text);
    final listOwner = node.interfaceTarget.enclosingClass?.name;
    if (listOwner == 'List' || listOwner == 'Iterable') {
      final rust = listMethodNames[name];
      if (rust == null) throw Unsupported('`List.$name`', _sample(node));
      // A getter in Dart, a method in Rust: `xs.length` is `xs.len()`.
      return IrCall(_listReceiver(node.receiver), rust, const []);
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
      return IrCall(_receiver(node.receiver), rust, const []);
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
    final target = receiver is ThisExpression ? null : _receiver(receiver);
    // A getter whose landing member is a field this class holds (a mixin
    // clone's) is read as the field, typed as the clone declares it -- as
    // a write to it is stored (`_instanceSet`). Through the trait's getter
    // it came back as the erased bound (`_slotToChild`, ws373).
    if (node.interfaceTarget is Procedure &&
        !_heldField(node.interfaceTarget, receiver)) {
      final declared = node.interfaceTarget.getterType;
      return _acrossBinding(
        _qualified(
          IrCall(target, name, const []),
          node.interfaceTarget,
          receiver,
        ),
        declared,
        _bindingOf(declared, node.interfaceTarget, receiver),
        toOption: true,
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
    // The class the receiver *is here*: a closure parameter retyped to an
    // erased bound reads back through a cast to the class it was declared
    // with (`_localRead`), whatever the type flow analysis narrowed the
    // static type to. Narrowed to a concrete subclass, the read came out
    // as a field access on a value that is still a trait object
    // (`notification.metrics` on a `dyn ScrollNotification`, ws721).
    final receiverType = target == null
        ? null
        // ..and through `_appliedBack`: inside a mixin's body borrowed from
        // an application the copy says `FlexParentData` where the trait
        // holds the erased bound, and a field read on a `dyn` is the
        // accessor (ws735).
        : _backHere(
            receiver is VariableGet && _retyped.containsKey(receiver.variable)
                ? receiver.variable.type
                : _staticType(receiver),
          );
    final concrete =
        receiverType is InterfaceType &&
        !_abstractLike(receiverType.classNode) &&
        receiverType.classNode.enclosingLibrary.importUri.scheme != 'dart';
    if (target != null &&
        declaring != null &&
        _abstractLike(declaring) &&
        !declaring.isAnonymousMixin &&
        !concrete) {
      final declared = node.interfaceTarget.getterType;
      return _acrossBinding(
        _qualified(
          IrCall(target, name, const []),
          node.interfaceTarget,
          receiver,
        ),
        declared,
        _bindingOf(declared, node.interfaceTarget, receiver),
        toOption: true,
      );
    }
    final read =
        IrField(
            target,
            name,
            // `PerformanceOverlayOption.x.index` in a static initialiser resolves
            // to `_Enum.index`, a field of a class that is not an enum; the
            // receiver's own type says it is one (4 "attempted to take value of
            // method `index`" in `rendering`).
            onEnum:
                (node.interfaceTarget.enclosingClass?.isEnum ?? false) ||
                (receiverType is InterfaceType &&
                    receiverType.classNode.isEnum),
            owner: target == null
                ? null
                : concrete && declaring != null && _abstractLike(declaring)
                ? (receiverType as InterfaceType).classNode.name
                : node.interfaceTarget.enclosingClass?.name,
          )
          ..rustType = _memberRustType(
            _landing(node.interfaceTarget, receiver),
            receiver,
            asGetter: true,
          );
    // Out of a projected field: into the `Option<T>` the body works with.
    final declared = node.interfaceTarget.getterType;
    return _acrossBinding(
      read,
      declared,
      _bindingOf(declared, node.interfaceTarget, receiver),
      toOption: true,
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
    const known = {'[]', '[]=', 'containsKey', 'keys'};
    if (!known.contains(name)) return null;
    // A local handed in is shared, as an argument is (`_clonedWhenPassed`):
    // two dispatches in a row moved the key into the first.
    final args = [
      for (final e in positional)
        () {
          final lowered = expression(e);
          return lowered is IrLocal
              ? (IrCall(lowered, 'clone', const [])
                  ..rustType = lowered.rustType)
              : lowered;
        }(),
    ];
    final slot = IrLocal('__d');
    // A write into the slot's map (`dateTimeSymbols[locale] = symbols`,
    // intl's `initializeDateFormattingCustom`, run585): the map is a
    // value behind the slot's handle, so the copy the arm holds is
    // written and put back as the slot's object.
    final field = target;
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
          // The result is a `dynamic`, as the caller sees it (`as Sym`
          // on it, the isgeneric fixture): the object, or Dart's null.
          body = isMap
              ? IrStaticCall(null, 'dart_option_object', [
                  IrCall(slot, '!map_get', args),
                ])
              : hasMember
              ? IrStaticCall(null, 'dart_option_object', [
                  IrSome(IrCall(slot, '[]', args, fails: true)),
                ])
              : noSuch();
        case '[]=':
          if (!isMap) {
            body = noSuch();
            break;
          }
          final boxed = IrUpcast(
            IrLocal('__d')..rustType = _type(c),
            const IrType('Object'),
            handle: false,
            explicit: true,
          )..rustType = const IrType('Object');
          final store = field.enclosingClass == null
              ? IrAssignTopLevel(field.name.text, boxed)
              : IrAssignStatic(
                  field.enclosingClass!.name,
                  field.name.text,
                  boxed,
                );
          body = IrBlockValue([
            IrExprStmt(IrCall(slot, 'insert', args)),
            store,
          ], IrLiteral('()', const IrType('raw')));
        case 'containsKey':
          // The map's is the prelude's `contains_key(&k)`, spelled as the
          // `Map` lowering spells it so the backend passes the key by
          // reference; a class's is its own method, by value -- and a
          // translated method's `Result` (`UninitializedLocaleData.
          // containsKey` beside the map's `bool`, `DateFormat.localeExists`,
          // run590).
          body = isMap
              ? IrCall(slot, 'contains_key', args)
              : hasMember
              ? IrCall(slot, 'containsKey', args, fails: true)
              : noSuch();
        default:
          body = isMap
              ? IrCall(slot, name, args)
              : hasMember
              ? IrCall(slot, name, args, fails: true)
              : noSuch();
      }
      arms.add((_type(c), body));
    }
    return IrDynamicDispatch(expression(receiver), arms)
      ..rustType = switch (name) {
        '[]' => const IrType('dynamic'),
        'containsKey' => const IrType('bool'),
        _ => null,
      };
  }

  IrExpr _staticGet(StaticGet node) {
    final target = node.target;
    final enclosing = target.enclosingClass;
    if (enclosing == null) {
      // A top-level name. A `const` or `final` is a module constant in Rust
      // too; a computed `get foo => ...` is a function and stops here.
      // Mutable ones too, now that they are emitted. A read of one goes
      // through the cell, which the backend knows from the declaration.
      if (target is Field) {
        return IrTopLevel(target.name.text, module: _topLevelModule(target));
      }
      // A top-level getter is a function here, so reading it is calling it.
      if (target is Procedure && target.kind == ProcedureKind.Getter) {
        return IrStaticCall(
          null,
          target.name.text,
          const [],
          fails: _fails(target),
          diverges: _diverges(target),
          asyncFn: _asyncMember(target),
          module: _topLevelModule(target),
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
        asyncFn: _asyncMember(target),
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
    // The member the call lands on, for `_landingSlot`: an anonymous mixin
    // application's copy of `_addDiagnostics(ChildType child)` has
    // `RenderBox` written in it, and only the mixin's own -- the trait's
    // -- takes the erased bound (188 `RenderBox` <- `RenderObject`, ws342).
    // `this` typed as the class being lowered (`_bindingOf` does the
    // same): its static type is not on the node, and without it a call on
    // `this` bound none of the class's own parameters -- `didUpdateValue(
    // oldValue)` took an `Option<T>` where the edge is `<T as
    // DartNullable>::Or` (`RestorableValue.value=`, run633).
    final env = typeEnvironment;
    final receiverType = node.receiver is ThisExpression && env != null
        ? _lowering?.getThisType(env.coreTypes, Nullability.nonNullable)
        : _staticType(node.receiver);
    final dispatch = receiverType is InterfaceType
        ? typeEnvironment?.hierarchy.getDispatchTarget(
            receiverType.classNode,
            node.name,
          )
        : null;
    final wasDispatch = _dispatchMember;
    final wasReceiver = _dispatchReceiverType;
    final wasInterface = _dispatchInterface;
    // ..the mixin's own declaration behind a copy in an application, as
    // the copy is typed everywhere (`_originalOf`).
    final dispatchOriginal = dispatch == null ? null : _originalOf(dispatch);
    // An *abstract* target has no dispatch target; the interface member
    // is the landing then, and still binds the class's parameters for
    // the arguments (`didUpdateValue(oldValue)` on `RestorableValue<T>`,
    // the projarg fixture).
    final interfaceTarget = node.interfaceTarget;
    _dispatchMember = dispatchOriginal is Procedure
        ? dispatchOriginal
        : interfaceTarget is Procedure
        ? interfaceTarget
        : null;
    _dispatchReceiverType = receiverType;
    _dispatchInterface = node.interfaceTarget.function;
    // A prelude method's slots as its sibling declares them
    // (`preludeSiblings`: `Set.removeAll(Iterable<Object?>)` takes the
    // set's own `E` here, as `addAll` does), instantiated with the
    // receiver's type arguments.
    final interface = node.interfaceTarget;
    final sibling = interface is Procedure
        ? _preludeDeclaration(interface)
        : null;
    final calleeFunction = sibling?.function ?? interface.function;
    FunctionType? instantiated = node.functionType;
    if (sibling != null && !identical(sibling, interface)) {
      final own = sibling.function.computeFunctionType(Nullability.nonNullable);
      instantiated = receiverType is InterfaceType
          ? Substitution.fromInterfaceType(receiverType).substituteType(own)
                as FunctionType
          : own;
    } else if (interface is Procedure &&
        interface.enclosingClass != null &&
        preludeSiblings.containsKey(
          '${interface.enclosingClass!.name}.${interface.name.text}',
        ) &&
        receiverType is InterfaceType &&
        receiverType.typeArguments.isNotEmpty) {
      // The sibling itself was tree-shaken out of the dill: the slots it
      // would have declared, spelled directly -- an `Iterable<Object?>`
      // parameter takes the collection's own elements.
      final element = receiverType.typeArguments.first;
      final own = interface.function.computeFunctionType(
        Nullability.nonNullable,
      );
      DartType elements(DartType p) =>
          p is InterfaceType &&
              p.classNode.name == 'Iterable' &&
              p.typeArguments.length == 1 &&
              _isTopType(p.typeArguments.single)
          ? InterfaceType(p.classNode, p.nullability, [element])
          : p;
      instantiated = FunctionType(
        [for (final p in own.positionalParameters) elements(p)],
        own.returnType,
        Nullability.nonNullable,
        namedParameters: own.namedParameters,
        requiredParameterCount: own.requiredParameterCount,
      );
    }
    final List<IrExpr> args;
    // A number's own method takes its own type where Dart writes `num`:
    // `x.clamp(0, 1)` on a `double` is `clamp(0.0, 1.0)`, and Rust's
    // `f64::clamp` takes no integer literal. Dart's `num` is not a type
    // here, so the receiver says which one it is
    // (`_MobileCarouselState.builder`, run722).
    final wasNumReceiver = _numReceiver;
    _numReceiver = receiverType is InterfaceType
        ? (const {'double', 'int'}.contains(receiverType.classNode.name)
              ? receiverType.classNode.name
              : null)
        : null;
    try {
      // With the call's type arguments for the method's own parameters,
      // as a static generic call has them (`_withGenericArgs`): `pop<T>
      // (result)` inside `maybePop<T>` binds the callee's `T` to the
      // caller's, which a projected `T?` slot has to know (19 at ws421).
      args = _withGenericArgs(
        calleeFunction,
        node.arguments,
        () => _arguments(
          node.arguments,
          calleeFunction,
          true,
          instantiated,
          null,
          null,
          _narrowSlots(node),
        ),
      );
    } finally {
      _dispatchMember = wasDispatch;
      _dispatchReceiverType = wasReceiver;
      _dispatchInterface = wasInterface;
      _numReceiver = wasNumReceiver;
    }
    // The owner by the receiver's *static* class when that is one of the
    // prelude's collections: TFA devirtualises `Map.cast` onto the one
    // implementation it found (`CanonicalizedMap`, a generic method on a
    // trait), and the prelude's `Map` is what the receiver is here
    // (`invokeMapMethod`, run492).
    // The receiver's static type outright, not `_staticClass`, which
    // answers only translated classes and so never a `dart:core` one
    // (the rule was silent through ws494).
    final staticReceiver = _staticType(node.receiver);
    final receiverClass = staticReceiver is InterfaceType
        ? staticReceiver.classNode
        : null;
    final staticOwner = receiverClass?.name;
    final collectionReceiver =
        receiverClass != null &&
        (staticOwner == 'List' ||
            staticOwner == 'Iterable' ||
            staticOwner == 'Set' ||
            _isMapClass(staticOwner)) &&
        receiverClass.enclosingLibrary.importUri.scheme == 'dart';
    final generic = collectionReceiver ? null : _genericOnTrait(node, args);
    if (generic != null) return generic;
    // The owner the lowering tables are keyed by is the *declaring* class
    // (`Iterable` for a `Set`'s `any`, ws496) -- unless TFA devirtualised
    // the target onto a translated class (`CanonicalizedMap.cast`), where
    // the receiver's static collection is the owner.
    final declaringOwner = node.interfaceTarget.enclosingClass;
    final devirtualised =
        collectionReceiver &&
        declaringOwner != null &&
        declaringOwner.enclosingLibrary.importUri.scheme != 'dart';
    final owner = devirtualised ? staticOwner : declaringOwner?.name;
    // A `StreamView` subclass's inherited `listen` and friends act on the
    // `_stream` it carries (see `lowerClass`).
    final declaringStream = node.interfaceTarget.enclosingClass;
    if (node.receiver is ThisExpression &&
        declaringStream != null &&
        (declaringStream.name == 'Stream' ||
            declaringStream.name == 'StreamView') &&
        declaringStream.enclosingLibrary.importUri.toString() == 'dart:async') {
      return IrCall(IrField(null, '_stream'), name, args);
    }
    // `child.toString()` on a `Listenable?`: an `Option` has no
    // `to_string`, and `dart_str` prints `null` for the absent case as
    // Dart does.
    if (name == 'toString' && args.isEmpty) {
      final t = _staticType(node.receiver);
      // The receiver as it is, not narrowed to its bound (`_receiver`):
      // a `T` receiver went behind a fresh `Rc<T>` and printed as one.
      final asObject = _stringOf(expression(node.receiver), t, explicit: true);
      if (asObject != null) return asObject;
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
      return IrCall(_receiver(node.receiver), 'complete', [
        IrLiteral('()', const IrType('raw')),
      ]);
    }
    // `s[i]` on a String is a one-character String, not an index into a
    // list: `pattern[0] == "a"` in intl's date formatting (44 + 44).
    // `[3, 4, 5].contains(n % 100)` with `n` a `num`: Dart compares by
    // value (`3 == 3.0`), so the `double` is cast to the list's `int`.
    // `xs.cast<T2>()` / `m.cast<K2, V2>()` on any of the prelude's
    // collections: its `cast_to`, converting element representations
    // (`FromDynamic`) as `as List<T2>` does. TFA had devirtualised
    // `Map.cast` onto a `CanonicalizedMap` (`invokeMapMethod`, run491).
    if ((owner == 'List' ||
            owner == 'Iterable' ||
            owner == 'Set' ||
            _isMapClass(owner)) &&
        name == 'cast' &&
        args.isEmpty &&
        node.arguments.types.isNotEmpty) {
      return IrCall(
        _receiver(node.receiver),
        'cast_to',
        const [],
        typeArguments: [for (final t in node.arguments.types) _type(t)],
      );
    }
    // The prelude's `Set::remove` takes the value by reference, like the
    // map's key (`_tickers.remove(ticker)`, 46).
    if (owner == 'Set' && name == 'remove' && args.length == 1) {
      return IrCall(_listReceiver(node.receiver), '!map_remove', [
        _intoElement(
          args.single,
          node.arguments.positional.single,
          _staticType(node.receiver),
        ),
      ]);
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
        return IrCall(_listReceiver(node.receiver), '!contains', [
          IrCast(args.single, 'i64'),
        ]);
      }
      return IrCall(_listReceiver(node.receiver), '!contains', [
        _intoElement(args.single, node.arguments.positional.single, listType),
      ]);
    }
    if (owner == 'String' && name == '[]' && args.length == 1) {
      return IrCall(_listReceiver(node.receiver), 'char_at', args);
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
      return IrCall(_listReceiver(node.receiver), spelled[name]!, const []);
    }
    if (owner == 'String' && name == 'split' && args.length == 1) {
      // `s.split(p)`: Rust's `split` wants a `&str` and yields an iterator.
      return IrCall(_listReceiver(node.receiver), 'split_dart', args);
    }
    if (owner == 'String' && name == '*' && args.length == 1) {
      // `'0' * n`: Rust's `repeat` wants a `usize`.
      return IrCall(_listReceiver(node.receiver), 'repeat_dart', args);
    }
    if (owner == 'String' &&
        name == 'contains' &&
        (args.length == 1 || args.length == 2)) {
      // `contains(other, [start])`: `str::contains` is inherent, takes a
      // `&str`, and has no start; the prelude's `contains_dart` has both.
      return IrCall(_listReceiver(node.receiver), 'contains_dart', [
        args.first,
        if (args.length == 2) args[1] else IrLiteral('0', const IrType('int')),
      ]);
    }
    if (owner == 'String' && name == 'startsWith' && args.length == 2) {
      // `startsWith(pattern, index)`: `str::starts_with` takes one argument
      // and, being inherent, would win over a trait method of the same name.
      return IrCall(_listReceiver(node.receiver), 'starts_with_at', args);
    }
    if (owner == 'String' && name == 'replaceRange' && args.length == 3) {
      // Dart's `replaceRange` returns a new string; Rust's `String` has an
      // inherent `replace_range` that mutates in place and takes a range,
      // and an inherent method shadows a trait's. So the prelude's is named
      // apart.
      return IrCall(_listReceiver(node.receiver), 'replace_range_dart', args);
    }
    if (owner == 'Expando') {
      // `expando[object]` / `expando[object] = v`: identity-keyed, so the
      // prelude's `get`/`set` rather than an index. 6 uses.
      if (name == '[]' && args.length == 1) {
        return IrCall(_listReceiver(node.receiver), '!expando_get', [
          args.single,
        ]);
      }
      if (name == '[]=' && args.length == 2) {
        return IrCall(_listReceiver(node.receiver), '!expando_set', args);
      }
    }
    // A typed list with a narrow element -- `Float32List` is `Vec<f32>`,
    // `Int32List` is `Vec<i32>` -- takes Dart's `double`/`int` cast down on
    // the way in and up on the way out. 23 `f32 <= f64` and 14 `i32`/`i64`
    // in `dart:ui`'s colour and vertex code.
    final narrow = _narrowElement(_staticType(node.receiver));
    if (narrow != null && name == '[]' && args.length == 1) {
      return IrCast(
        IrIndex(_listReceiver(node.receiver), args.single),
        narrow.startsWith('f') ? 'f64' : 'i64',
      );
    }
    if (narrow != null && name == '[]=' && args.length == 2) {
      final held = '__t${_nextTemporary++}';
      return IrBlockValue([
        IrLocalDecl(held, null, args[1]),
        IrIndexSet(
          _listReceiver(node.receiver, name),
          args[0],
          IrCast(
            IrCall(IrLocal(held), 'clone', const [])
              ..rustType = args[1].rustType,
            narrow,
          ),
        ),
      ], IrLocal(held));
    }
    if (owner == 'List' || owner == 'Iterable') {
      if (name == '[]' && args.length == 1) {
        // Typed by the list's element, which a generic class's `List<E?>`
        // keeps projected (`<E as DartNullable>::Or`) where the static type
        // of the read says a plain `E?`.
        final list = _listReceiver(node.receiver, name);
        final element = list.rustType?.arguments.length == 1
            ? list.rustType!.arguments.single
            : null;
        return IrIndex(list, args.single)..rustType = element;
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
            _listReceiver(node.receiver, name),
            args[0],
            IrCall(IrLocal(held), 'clone', const [])
              ..rustType = args[1].rustType,
          ),
        ], IrLocal(held));
      }
      // `whereType<T>()`: the elements that are a `T`, by the cast table
      // (`_semantics` nodes filtered in `RenderObject`, run527).
      if (name == 'whereType' &&
          args.isEmpty &&
          node.arguments.types.length == 1) {
        final wantedDart = node.arguments.types.single;
        // `whereType<T>()` over an `Iterable<T?>` is the elements that are
        // there: no runtime test says more than that, and for a function
        // type there is no test at all -- `dart_cast_to::<Function>` named
        // a type nothing declares (`whereType<ImageErrorListener>()` over
        // the listeners' `onError` in `ImageStreamCompleter.reportError`,
        // ws762).
        final receiverType = _staticType(node.receiver);
        final element =
            receiverType is InterfaceType &&
                receiverType.typeArguments.length == 1
            ? receiverType.typeArguments.single
            : null;
        if (element != null &&
            element.nullability == Nullability.nullable &&
            element.withDeclaredNullability(Nullability.nonNullable) ==
                wantedDart) {
          final kept = _type(wantedDart);
          return IrCall(
            _listReceiver(node.receiver, name),
            '!where_present',
            const [],
          )..rustType = IrType('List', arguments: [kept]);
        }
        final wanted = _type(wantedDart);
        return IrCall(
          _listReceiver(node.receiver, name),
          '!where_type',
          const [],
          typeArguments: [wanted],
        )..rustType = IrType('List', arguments: [wanted]);
      }
      final step = iterStepNames[name];
      if (step != null && args.length == 1) {
        // A chain, extended rather than started again when the receiver is
        // already one: `xs.where(f).map(g)` is one `iter()`, not two.
        final source = _listReceiver(node.receiver, name);
        return source is IrIterChain
            ? IrIterChain(source.source, [...source.steps, (step, args.single)])
            : IrIterChain(source, [(step, args.single)]);
      }
      // `lastWhere` is the same shape read from the other end, and the
      // same two prelude methods (`NavigatorState.pop`, ws810).
      if ((name == 'firstWhere' || name == 'lastWhere') && args.length == 2) {
        final where = name == 'firstWhere' ? 'first_where' : 'last_where';
        // `firstWhere(test)` throws when nothing matches; with `orElse` it
        // calls that instead. The omitted `orElse` arrives as `None`, and a
        // generic `impl Fn` parameter cannot take a `None`, so the two are
        // two prelude methods. 25 calls.
        final orElse = args[1];
        final omitted = orElse is IrLiteral && orElse.type.name == 'Null';
        // ..and a *given* one goes in bare: Dart's slot is `E Function()?`
        // and the coercion wrapped it, where the prelude's parameter is a
        // plain `impl Fn()` (`FlutterErrorDetails.summary`, run730).
        var given = orElse is IrSome ? orElse.value : orElse;
        given = given is IrCall && given.name == '!rc' && given.args.isEmpty
            ? given.target!
            : given;
        given = _unboxed(given);
        return IrCall(
          _listReceiver(node.receiver, name),
          omitted ? where : '${where}_or',
          omitted ? [args[0]] : [args[0], given],
        );
      }
      if (name == 'sort') {
        // `sort()` is the natural order Dart's `Comparable` gives
        // (`sort_natural`); `sort(compare)` takes a Dart comparator
        // returning an `int`, which the prelude's `sort_by_dart` turns into
        // an `Ordering`. 36 of these. An *omitted* comparator arrives as
        // the `null` default and is the first of the two, not the second
        // with a `None` (`FlutterError.defaultStackFilter`, run728).
        final given = args.length == 1 ? args.single : null;
        final omitted =
            args.isEmpty || (given is IrLiteral && given.type.name == 'Null');
        return IrCall(
          _listReceiver(node.receiver, name),
          omitted ? 'sort_natural' : 'sort_by_dart',
          omitted ? const [] : args,
        );
      }
      final rust = listMethodNames[name];
      if (rust != null) {
        // An element handed to `remove`/`indexOf`: into the element type,
        // which the prelude's `&T` cannot coerce to (see `_intoElement`).
        final byElement =
            const {'remove', 'indexOf', 'lastIndexOf'}.contains(name) &&
            args.length == 1;
        return IrCall(
          _listReceiver(node.receiver, name),
          rust,
          byElement
              ? [
                  _intoElement(
                    args.single,
                    node.arguments.positional.single,
                    _staticType(node.receiver),
                  ),
                ]
              : args,
        );
      }
      throw Unsupported('`List.$name`', _sample(node));
    }
    if (_isMapClass(owner)) {
      // Its own name: the backend's `.get(&k).cloned()` was keyed on `get`
      // and fired on `ContrastCurve.get(double)` too (14 `&f64`).
      if (name == '[]' && args.length == 1) {
        // The read is typed by the map's own value type -- the receiver's
        // recorded `rustType`, which an erased map keeps as the bound --
        // and a slot it goes into coerces it (`_slotToChild[slot]` returned
        // as the `RenderBox?` `childForSlot` declares).
        IrExpr typed(IrExpr read) {
          final map = read is IrCall ? read.target?.rustType : null;
          if (map != null && map.name == 'Map' && map.arguments.length == 2) {
            final value = map.arguments[1];
            // ..with its signature kept: rebuilt by name, a `Map<String,
            // VoidCallback>`'s value read as a bare `Function` -- an
            // object -- where the Rust value is the typed `Rc<dyn Fn>`
            // (`_customActionCallbacks[id]`, ws515).
            read.rustType = value.isFunction
                ? IrType.function(
                    value.parameters!,
                    value.returns!,
                    nullable: true,
                  )
                : IrType(
                    value.name,
                    nullable: true,
                    arguments: value.arguments,
                  );
          }
          return read;
        }

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
          return typed(
            IrCall(_receiver(node.receiver), '!map_get', [
              IrCast(args.single, 'i64'),
            ]),
          );
        }
        // A nullable key into a map of non-nullable ones: `_views[_implicitViewId]`.
        // ..by the key's Rust type: an `Object?` key is a `dynamic`, no
        // `Option` (ws501).
        final keyIr = args.single.rustType;
        if (key != null &&
            key.nullability != Nullability.nullable &&
            keyIr != null &&
            isNullable(keyIr)) {
          return typed(IrCall(_receiver(node.receiver), '!map_get_opt', args));
        }
        // The key into the map's key type by the one rule: a `String` into
        // a `Map<Object?, ..>` goes behind a handle (15 at ws421).
        final keyed = key == null
            ? args.single
            : _intoArgument(node.arguments.positional.single, key, args.single);
        return typed(IrCall(_receiver(node.receiver), '!map_get', [keyed]));
      }
      // `m[k] = v`: `insert`, as a statement or for its value (Dart's is
      // `v`; here the old value, which no caller reads).
      if (name == '[]=' && args.length == 2) {
        return IrCall(
          _receiver(node.receiver),
          'insert',
          _mapEntry(node, args),
        );
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
          return IrCall(_receiver(node.receiver), rust, [
            IrCast(args.single, 'i64'),
          ]);
        }
        // ..and into the map's key type by the one rule, as `m[k]` is: a
        // `String` into a `Map<Object?, ..>.containsKey` goes behind a
        // handle (`decodeMethodCall`, ws491).
        if (key != null) {
          return IrCall(_receiver(node.receiver), rust, [
            _widened(node.arguments.positional.single, key, args.single),
          ]);
        }
      }
      return IrCall(_receiver(node.receiver), rust, args);
    }
    // A comparison (or any operator outside `stdOperators`) that a
    // translated class declares is that class's method here (`ge`, `lt`):
    // `getWindowType(context) >= AdaptiveWindowType.medium` was a Rust
    // `>=` on a struct with no `PartialOrd` (run643). Only the std
    // operators (`+`, `-`, ..) have an operator trait impl to reach.
    final operatorOwner = node.interfaceTarget.enclosingClass;
    final userOperator =
        operatorOwner != null &&
        _translatedClass(operatorOwner) &&
        !stdOperators.contains(name);
    if (_binaryOperators.contains(name) && args.length == 1 && !userOperator) {
      // `int * double` is a `double` in Dart and a type error in Rust: the
      // `int` side is cast. The receiver's class is the operator's owner;
      // the argument's is asked of the static types.
      var left = _receiver(node.receiver);
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
          if (leftClass == 'int') left = _toF64(left);
          if (rightClass == 'int') right = _toF64(right);
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
          left = _toF64(left);
        }
        if (leftClass == 'double' && rightClass == 'int') {
          right = _toF64(right);
        }
        // A *declared* `num` -- a variable, field or static whose declaration
        // says `num`, an `f64` here -- against an int literal: the literal
        // is cast. Not the static type: `getStaticType` says `num` for an
        // `int` assignment used as a value (`(index = next()) >= 0`), and a
        // cast on that went wrong 200 times (ws53).
        final argument = node.arguments.positional.single;
        if (_declaredNum(node.receiver) &&
            (argument is IntLiteral || classOf(argument) == 'int')) {
          right = _toF64(right);
        } else if (_declaredNum(argument) && leftClass == 'int') {
          left = _toF64(left);
        }
        // Dart's `/` is always a `double`, even on two `int`s (`~/` is the
        // integer one); Rust's `/` on two `i64`s is an `i64`.
        if (name == '/') {
          if (leftClass == 'int') left = _toF64(left);
          if (rightClass == 'int') right = _toF64(right);
          // `targetWidth! / (w / h)`: whatever the static type of the left
          // side says, a `/` with a `double` right side is a `double`
          // division, and Rust has no `i64 / f64`.
          // ..when the left side is a number: a class's own `/` takes
          // what it declares (`BoxConstraints / double` in
          // `ViewConfiguration.fromView`, cast to `f64`, ws474).
          if (rightClass == 'double' &&
              (leftClass == 'int' || leftClass == 'num')) {
            left = _toF64(left);
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
      return IrUnary('-', _receiver(node.receiver));
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
        IrDowncast(_receiver(node.receiver), 'f64'),
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
        return IrCast(IrCall(_receiver(node.receiver), name, const []), 'i64');
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
    final call = _qualified(
      IrCall(
        receiver is ThisExpression ? null : _receiver(receiver),
        name,
        args,
        typeArguments: withTypeArgs
            ? [for (final t in node.arguments.types) _type(t)]
            : const [],
      ),
      node.interfaceTarget,
      receiver,
    );
    // The call's own result type, on the call itself: the projection
    // below wraps it, and `expression` types only the wrapper, which left
    // a generic method's call untyped -- and the erased twin's cast back
    // (`dart_cast_erased`) spells that type (`find<T>()` returning `T?`,
    // ws496). A `T?` of this declaration's own parameter comes back
    // projected (`<T as DartNullable>::Or`), as the callee's declared `T?`
    // return does.
    if (withTypeArgs) {
      final static = _staticType(node);
      // ..projected only when the callee's `T` is put in as a *bare*
      // parameter of this declaration: `resourcesFor<T?>(..)` returns
      // `<Option<T> as DartNullable>::Or`, a plain `Option<T>`, and typed
      // projected it was wrapped twice (`Localizations.of`, ws503).
      final declaredReturn = target.function.returnType;
      final bareArgument =
          declaredReturn is TypeParameterType &&
          target.function.typeParameters.contains(declaredReturn.parameter) &&
          () {
            final index = target.function.typeParameters.indexOf(
              declaredReturn.parameter,
            );
            final argument = node.arguments.types[index];
            return argument is TypeParameterType &&
                argument.nullability != Nullability.nullable;
          }();
      if (static is TypeParameterType &&
          static.nullability == Nullability.nullable &&
          !_erasedParameter(static.parameter) &&
          bareArgument) {
        call.rustType = IrType(
          static.parameter.name ?? 'T',
          nullable: true,
          projected: true,
        );
      } else if (static != null) {
        try {
          // A `T?` bound to a top type is the `Option<Rc<dyn Object>>`
          // the callee hands back, as `expression` types a read
          // (`invokeMethod<dynamic>(..)` into a `dynamic` local, ws513).
          call.rustType = _topBound(declaredReturn, static) ?? _type(static);
        } on Unsupported {
          // Untyped, as `expression` leaves it.
        }
      }
    }
    // A projected result: into the `Option<T>` the caller works with.
    final declared = node.interfaceTarget.function?.returnType;
    return _acrossBinding(
      call,
      declared,
      _bindingOf(declared, node.interfaceTarget, receiver, node.arguments),
      toOption: true,
    );
  }

  /// See `IrCall.qualifier`: a member whose name two classes in the
  /// receiver's hierarchy declare is called through one of them by name.
  /// The member a call on `receiver` lands on in Rust: an inherent method
  /// of the receiver's class (a mixin clone with `RenderBox` written in
  /// it) when the class is a struct or an open class, the interface
  /// member (the trait's, erased) otherwise. `getDispatchTarget` answers
  /// for both: on an abstract class it finds the hollow mixin's own.
  Member _landing(Member interface, Expression receiver) {
    final hierarchy = typeEnvironment?.hierarchy;
    // ..through `_appliedBack`, as the read does: inside a mixin's body
    // borrowed from an application the copy says `FlexParentData` where
    // the trait holds the erased bound, and the qualifier is that bound's
    // trait, not the application's (ws736).
    final type = receiver is ThisExpression
        ? null
        : _backHere(_staticType(receiver));
    final on = receiver is ThisExpression
        ? (_lowering ?? _member?.enclosingClass)
        : type is InterfaceType
        ? type.classNode
        : null;
    if (hierarchy == null || on == null) return interface;
    final found = hierarchy.getDispatchTarget(
      on,
      interface.name,
      setter: interface is Procedure && interface.isSetter,
    );
    return found ?? interface;
  }

  /// The Rust type of `landing`'s value as reached through `receiver`:
  /// its declared type with the receiver's type arguments substituted for
  /// the parameters that are *kept*, the erased ones left to `_type`,
  /// which spells them as their bound. Dart's own static type substitutes
  /// every one, which is where the clone's `RenderBox` and the trait's
  /// `RenderObject` part ways. Null when the type names the method's own
  /// parameters (Dart's instantiated type is the better answer there) or
  /// has no spelling here.
  IrType? _memberRustType(
    Member reached,
    Expression receiver, {
    required bool asGetter,
  }) {
    // A copy in an anonymous application is typed by the mixin's own
    // declaration, as the copy itself is lowered (the kept parameters
    // substituted below, the erased ones their bounds).
    final landing = _originalOf(reached);
    var declared = asGetter || landing is! Procedure
        ? landing.getterType
        : landing.function.returnType;
    if (landing is Procedure &&
        !asGetter &&
        landing.function.typeParameters.isNotEmpty &&
        _mentionsParametersOf(declared, landing.function.typeParameters)) {
      return null;
    }
    final owner = landing.enclosingClass;
    final env = typeEnvironment;
    final receiverType = receiver is ThisExpression
        ? (env == null
              ? null
              : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable))
        : _staticType(receiver);
    if (owner != null &&
        owner.typeParameters.isNotEmpty &&
        env != null &&
        receiverType is InterfaceType) {
      final asOwner = env.hierarchy.getTypeAsInstanceOf(receiverType, owner);
      if (asOwner is InterfaceType) {
        final kept = <TypeParameter, DartType>{};
        for (
          var i = 0;
          i < owner.typeParameters.length && i < asOwner.typeArguments.length;
          i++
        ) {
          final p = owner.typeParameters[i];
          if (!_erasedParameter(p)) kept[p] = asOwner.typeArguments[i];
        }
        try {
          return _typeKept(declared, kept);
        } on Unsupported {
          return null;
        }
      }
    }
    try {
      return _type(declared);
    } on Unsupported {
      return null;
    }
  }

  static bool _mentionsParametersOf(DartType t, List<TypeParameter> ps) {
    // `FutureOr<R>` is its own node, not an `InterfaceType`: `then<R>`'s
    // `FutureOr<R> Function(void)` slot passed for a slot of no parameter,
    // and the callback was lowered against the declared `R`, its body
    // never closed (`Route.didAdd`, ws512).
    if (t is FutureOrType) return _mentionsParametersOf(t.typeArgument, ps);
    if (t is RecordType) {
      return t.positional.any((a) => _mentionsParametersOf(a, ps)) ||
          t.named.any((n) => _mentionsParametersOf(n.type, ps));
    }
    if (t is TypeParameterType) return ps.contains(t.parameter);
    if (t is InterfaceType) {
      return t.typeArguments.any((a) => _mentionsParametersOf(a, ps));
    }
    if (t is FunctionType) {
      return _mentionsParametersOf(t.returnType, ps) ||
          t.positionalParameters.any((a) => _mentionsParametersOf(a, ps)) ||
          t.namedParameters.any((n) => _mentionsParametersOf(n.type, ps));
    }
    return false;
  }

  IrCall _qualified(IrCall call, Member member, Expression receiver) {
    final out = _qualifiedRaw(call, member, receiver);
    // Typed by the member the call reaches in Rust: through a trait's
    // path (`RestorationMixin::restoration_id(self)`) it is the trait's
    // declaration, whatever the class's override narrowed it to (`String?`
    // there, `String` here: 72 dropped `!`s at ws390); a plain call lands
    // on the class's own.
    out.rustType ??= _memberRustType(
      out.qualifier != null ? member : _landing(member, receiver),
      receiver,
      asGetter: member is Field || (member is Procedure && member.isGetter),
    );
    // An `async` member declared `Future<T>?` still hands back the future
    // it spawns, never null: its wrapper is typed `DartFuture<T>`, and so
    // is a call to it (`sendWithPostfix` in `send`, ws474).
    final t = out.rustType;
    if (out.asyncTarget && t != null && t.name == 'Future' && t.nullable) {
      out.rustType = IrType('Future', arguments: t.arguments);
    }
    // ..and one declared `FutureOr<T>` hands back a `Future<T>` too
    // (Dart's `flatten`; see `_spawnedFuture`).
    if (out.asyncTarget && t != null && t.name == 'FutureOr') {
      out.rustType = IrType('Future', arguments: t.arguments);
    }
    return out;
  }

  IrCall _qualifiedRaw(IrCall call, Member member, Expression receiver) {
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
    // ..through `_appliedBack`, as the read does: inside a mixin's body
    // borrowed from an application the copy says `FlexParentData` where
    // the trait holds the erased bound, and the qualifier is that bound's
    // trait, not the application's (ws736).
    final type = receiver is ThisExpression
        ? null
        : _backHere(_staticType(receiver));
    // Inside a body borrowed from a mixin application (`_appliedBody`)
    // `this` is the mixin's trait, not the anonymous class the CFE copied
    // the body into (`ServicesBinding::x(this_)` named the trait as a
    // type, 9 E0782 at ws436).
    final enclosing = _member?.enclosingClass;
    // A receiver typed by a type parameter (`ChildType child` in a
    // mixin's copy, read by its declaration) is its bound's class, which
    // is what it is here: with no class at all the walk started at the
    // member's owner and `child.toDiagnosticsNode()` on a `dyn
    // RenderObject` was left for three traits to claim (4 E0034, ws536).
    final from = receiver is ThisExpression
        ? ((enclosing?.isAnonymousMixin ?? false) ? _lowering : enclosing)
        : _classOfType(type);
    var qualifier = _qualifierFor(from ?? owner, member);
    // ..and from the class the receiver *is here* when the Dart type said
    // nothing: a closure parameter retyped to an erased bound reads back
    // through a cast, and the wider type above it declares no member for
    // the walk to count -- `notification.depth` on a `ScrollNotification`
    // read out of a `Notification` slot was left for two traits to claim
    // (`_PageViewState.build`, ws719).
    if (qualifier == null &&
        receiver is VariableGet &&
        _retyped.containsKey(receiver.variable)) {
      final declaredClass = _classOfType(receiver.variable.type);
      if (declaredClass != null && !identical(declaredClass, from)) {
        qualifier = _qualifierFor(declaredClass, member);
      }
    }
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
    // `DART2RUST_TRACE_CALL=<name>`: the async-rule inputs of every call
    // to that member, to stderr.
    if (Platform.environment['DART2RUST_TRACE_CALL'] == member.name.text) {
      stderr.writeln(
        'TRACE_CALL ${member.name.text}: from=${from?.name} owner=${owner.name} '
        'enclosing=${_member?.enclosingClass?.name} lowering=${_lowering?.name} '
        'fails=$fails async=${_asyncMember(member)} qualifier=$qualifier '
        'applies=${from != null && _appliesMixin(from, owner)} '
        'abstract=${from != null && _abstractLike(from)} open=${from != null && _isOpen(from)}',
      );
    }
    if (qualifier == null && !fails && !renamed && !_asyncMember(member)) {
      return call;
    }
    return IrCall(
      call.target,
      renamed ? _dartName(call.name) : call.name,
      call.args,
      qualifier: qualifier,
      receiverClass: _classOfType(type)?.name,
      fails: fails,
      diverges: _diverges(member),
      // A struct's *own* async method, called plainly, is an `async fn`
      // reached as one; an inherited or a trait's goes through the trait
      // impl, which hands the future back inside the `Result`.
      // ..or a mixin's method the receiver's class applies, which is
      // inlined into that class as its own (`handlePopRoute()` inside
      // `WidgetsBinding.initInstances`, run445).
      asyncFn: _inherentAsync(
        member,
        from,
        qualifier,
        onThis: call.target == null,
      ),
      asyncTarget: _asyncMember(member),
      typeArguments: call.typeArguments,
    );
  }

  /// Whether a call to `member` from a receiver of class `from` reaches an
  /// `async fn` as one (`IrCall.asyncFn`): the member is async, and the
  /// receiver's own struct carries it inherently -- its own method, or a
  /// mixin's it applies -- with no trait on the path.
  ///
  /// On `this` (`onThis`) the receiver's own class is the struct or the
  /// trait body being emitted, and the backend knows which: an open or
  /// abstract class's own async method called on `this` counts as
  /// inherent here, and the trait bodies unwrap it (`_handleAsMethodCall`
  /// in `MethodChannel.setMethodCallHandler`'s super fn, run458).
  bool _inherentAsync(
    Member member,
    Class? from,
    String? qualifier, {
    bool onThis = false,
  }) {
    final owner = member.enclosingClass;
    if (Platform.environment['DART2RUST_TRACE_CALL'] == member.name.text) {
      stderr.writeln(
        'TRACE_ASYNC ${member.name.text}: from=${from?.name} owner=${owner?.name} '
        'fails=${_fails(member)} async=${_asyncMember(member)} qualifier=$qualifier '
        'applies=${from != null && owner != null && _appliesMixin(from, owner)} '
        'abstract=${from != null && _abstractLike(from)} open=${from != null && _isOpen(from)}',
      );
    }
    if (owner == null || from == null) return false;
    return (qualifier == null || qualifier == from.name) &&
        _asyncMember(member) &&
        (from == owner || _appliesMixin(from, owner)) &&
        (onThis || (!_abstractLike(from) && !_isOpen(from)));
  }

  /// Whether `from` applies `mixin` somewhere in its anonymous superclass
  /// chain, so that the mixin's methods are inlined into `from`'s struct.
  static bool _appliesMixin(Class from, Class mixin) {
    var t = from.supertype;
    while (t != null && t.classNode.isAnonymousMixin) {
      // The mixin itself, or the application class holding its copy (an
      // interface target inside an applied body names that one).
      if (t.classNode == mixin ||
          t.classNode.implementedTypes.any((i) => i.classNode == mixin)) {
        return true;
      }
      t = t.classNode.supertype;
    }
    return false;
  }

  /// A member declared `async`: emitted as an `async fn` where it is a
  /// free function, a static, or a struct's own method.
  /// By the marker the programmer wrote (`dartAsyncMarker`), which a hollow
  /// mixin declaration keeps where its `asyncMarker` says `Sync` for want
  /// of a body (`ServicesBinding.handleRequestAppExit`, run446).
  bool _asyncMember(Member m) =>
      m is Procedure && m.function.dartAsyncMarker == AsyncMarker.Async;

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
      // ..through `_appliedBack`, as the read's qualifier is (ws737).
      from = _classOfType(_backHere(_staticType(receiver)));
    }
    if (from == null) return null;
    return _qualifierFor(from, target);
  }

  String? _classNameOf(Expression receiver) =>
      _classOfType(_backHere(_staticType(receiver)))?.name;

  /// The class a value of `t` is: an interface's own, a type parameter's
  /// bound's (through a bound that is itself a parameter).
  Class? _classOfType(DartType? t) {
    var seen = 0;
    while (t is TypeParameterType && seen++ < 8) {
      t = t.parameter.bound;
    }
    return t is InterfaceType ? t.classNode : null;
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

  /// Whether `c` implements `dart:async`'s `Future` directly: such a
  /// class is the prelude's future here (`_type`), and constructing it
  /// with its value is a future already done (`future_ready`).
  bool _futureLike(Class c) =>
      c.typeParameters.length == 1 &&
      c.enclosingLibrary.importUri.scheme != 'dart' &&
      c.implementedTypes.any(
        (t) =>
            t.classNode.name == 'Future' &&
            t.classNode.enclosingLibrary.importUri.toString() == 'dart:async',
      );

  IrExpr _construct(ConstructorInvocation node) {
    final target = node.target;
    final name = target.name.text;
    if (_futureLike(target.enclosingClass) &&
        node.arguments.positional.length == 1 &&
        node.arguments.named.isEmpty) {
      final held = node.arguments.types.isNotEmpty
          ? _type(node.arguments.types.single)
          : null;
      final value = expression(node.arguments.positional.single);
      final ready = IrStaticCall(null, 'future_ready', [
        held == null
            ? value
            : _widened(
                node.arguments.positional.single,
                node.arguments.types.single,
                value,
              ),
      ]);
      if (held != null) ready.rustType = IrType('Future', arguments: [held]);
      return ready;
    }
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
        arguments: _censusOf(
          node.constructedType,
          _erasedArguments(cls, node.constructedType.typeArguments),
        ),
        module: _moduleQualifier(cls),
      ),
      _constructing(
        target.function,
        {
          for (
            var i = 0;
            i < cls.typeParameters.length && i < node.arguments.types.length;
            i++
          )
            cls.typeParameters[i]: node.arguments.types[i],
        },
        () => _arguments(
          node.arguments,
          target.function,
          false,
          _instantiatedConstructor(node),
        ),
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
  String _instanceName(Class cls) {
    // A private implementation class of a `dart:` library is constructed
    // as the public type it implements, which is what the prelude names
    // (`WeakReference(..)` devirtualised to `_WeakReference`, the
    // navigator's `_RouteEntry`, ws505).
    final uri = cls.enclosingLibrary.importUri;
    if (cls.name.startsWith('_') &&
        uri.scheme == 'dart' &&
        uri.toString() != 'dart:ui') {
      for (final above in [
        if (cls.supertype != null) cls.supertype!,
        ...cls.implementedTypes,
      ]) {
        final c = above.classNode;
        if (!c.name.startsWith('_') && c.name != 'Object') {
          return _instanceName(c);
        }
      }
    }
    return _isOpen(cls) ? implName(cls.name) : cls.name;
  }

  static FunctionType? _instantiatedConstructor(ConstructorInvocation node) {
    final cls = node.target.enclosingClass;
    if (cls.typeParameters.isEmpty) return null;
    final declared = node.target.function.computeThisFunctionType(
      Nullability.nonNullable,
    );
    // A constructor's function type carries the class's type parameters as
    // its own (structural ones, `E%`), which a substitution by the
    // constructed type's arguments does not reach: instantiated instead
    // (`HeapPriorityQueue<_TaskEntry<dynamic>>(_taskSorter)` in the
    // binding's constructor took a `Comparator<E%>`, run433).
    if (declared.typeParameters.isNotEmpty) {
      if (declared.typeParameters.length !=
          node.constructedType.typeArguments.length) {
        return null;
      }
      final instantiated = FunctionTypeInstantiator.instantiate(
        declared,
        node.constructedType.typeArguments,
      );
      return instantiated is FunctionType ? instantiated : null;
    }
    final substituted = Substitution.fromInterfaceType(node.constructedType)
        .substituteType(declared);
    return substituted is FunctionType ? substituted : null;
  }

  /// A generic function's type at this call: `_futurize<int>(callbacker)`
  /// takes a `String? Function(_Callback<int>)`, and the closure passed is
  /// typed against that, not against the `T` the declaration wrote.
  static FunctionType? _instantiated(
    StaticInvocation node, [
    FunctionNode? declaration,
  ]) {
    final fn = declaration ?? node.target.function;
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
        // ..reading the value back first: what is in hand may be the
        // bound an erased slot handed out, and `Some(x)` around an
        // `Rc<dyn Object>` is no `Option<Rc<dyn Cursor>>`
        // (`mouseCursor?.resolve(states)` in
        // `ToggleableStateMixin.buildToggleable`, ws707).
        IrExpr inner = lowered;
        try {
          inner = coerce(
            lowered,
            _type(to.withDeclaredNullability(Nullability.nonNullable)),
          );
        } on Unsupported {
          inner = lowered;
        }
        return IrSome(inner)..rustType = _type(to);
      }
      return lowered;
    }
    final target = node.target;
    final declaration = _preludeDeclaration(target).function;
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
        a = _toF64(a);
      } else if (cls(positional[0]) == 'double' &&
          cls(positional[1]) == 'int') {
        b = _toF64(b);
      }
      // On a `T extends num`: the prelude's numeric protocol (`DartNum`),
      // whose names do not collide with `Ord`'s on a known number
      // (`AnimationMin<T extends num>.value`, run676).
      final first = _staticType(positional[0]);
      if (first is TypeParameterType && !_erasedParameter(first.parameter)) {
        return IrCall(a, 'dart_$rust', [b])..rustType = _type(first);
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
    // `Uint8List.sublistView(data, [start, end])` / `ByteData.sublistView`:
    // a copy of the bytes here (a `TypedData` is its bytes: a `ByteData` or
    // a `Uint8List`), the prelude's free functions (`StandardMessageCodec`,
    // on every platform message; ws493).
    if ((owner == 'Uint8List' || owner == 'ByteData') &&
        target.name.text == 'sublistView' &&
        positional.isNotEmpty) {
      return IrStaticCall(
        null,
        owner == 'Uint8List'
            ? 'uint8_list_sublist_view'
            : 'byte_data_sublist_view',
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
      // With the element type spelled: for a projected `E?` (a generic
      // class's `List<E?>`) the prelude fills with `<E as DartNullable>::Or`
      // nulls, which nothing could infer (`HeapPriorityQueue._queue`, run436).
      final element = node.arguments.types.single;
      // ..and a `dynamic` element (`List<Object?>`, which is one) holds
      // its nulls as `Null` objects, not as `None`s (ws499).
      final elementIr = _type(element);
      if (elementIr.name == 'dynamic' && !elementIr.nullable) {
        return IrStaticCall(null, 'vec_of_nulls', [expression(positional[0])])
          ..rustType = IrType('List', arguments: [elementIr]);
      }
      return IrStaticCall(
        null,
        'vec_of_nones',
        [expression(positional[0])],
        typeArguments: [
          _type(element.withDeclaredNullability(Nullability.nonNullable)),
        ],
      );
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
          _widened(
            e,
            element,
            _withExpectedReturn(element, e, () => expression(e)),
          ),
      ], _type(element ?? const DynamicType()));
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
          ? _toF64(lowered)
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
        final toIr = _type(to);
        return IrCall(
          IrDowncast(
            expression(positional.single),
            _rustScalar(toIr.name),
            arguments: toIr.arguments,
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
      // ..with its type arguments: `HashSet<T>()` into an `Object` slot
      // has nothing else to say what `Set::new()` holds (run560).
      return IrNew(
        IrType(
          owner!.contains('Set') ? 'Set' : 'Map',
          arguments: [for (final t in node.arguments.types) _type(t)],
        ),
        const [],
      );
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
    // ..and `String.fromCharCode(code)`, one rune (`Icon.build` spells the
    // glyph of an `IconData.codePoint`, run695).
    if (owner == 'String' &&
        target.name.text == 'fromCharCode' &&
        positional.length == 1) {
      return IrStaticCall(null, 'string_from_char_code', [
        expression(positional[0]),
      ]);
    }
    // `scheduleMicrotask(f)`: the prelude's `_schedule_microtask` takes the
    // `Rc<dyn Fn()>` a translated closure is; the public-named one is the
    // prelude's own `Box<dyn FnOnce()>` entry.
    // A `dart:collection` extension getter on an iterable (`xs.lastOrNull`,
    // lowered by the CFE to `IterableExtensions|get#lastOrNull<T>(xs)`):
    // the prelude's method on the receiver, so that the receiver is
    // borrowed as any list method's is (`FocusScopeNode.focusedChild`,
    // run650). The table is the mapping.
    final coreExtension =
        _coreExtensionMethods[target.enclosingLibrary.importUri
            .toString()]?[target.name.text];
    if (coreExtension != null && owner == null && positional.isNotEmpty) {
      final args = _arguments(node.arguments, target.function);
      final element = node.arguments.types.isNotEmpty
          ? _type(node.arguments.types.first)
          : const IrType('dynamic');
      return IrCall(args.first, coreExtension, args.sublist(1))
        ..rustType = IrType(
          element.name,
          nullable: true,
          arguments: element.arguments,
        );
    }
    // `future.onError<E>(handle, test: ..)` -- `dart:async`'s extension on
    // `Future`, which the CFE lowers to `FutureExtensions|onError(future,
    // handle, test: ..)`. It is `catchError` with the error type as part
    // of the test, so at an `Object` `E` the prelude's `catch_error` is
    // the whole of it (`AssetImage.obtainKey`, run725). A narrower `E`
    // needs its `is` in the test and is refused rather than dropped.
    if (target.name.text == 'FutureExtensions|onError' &&
        target.enclosingLibrary.importUri.toString() == 'dart:async' &&
        owner == null &&
        positional.length >= 2) {
      final e = node.arguments.types.length > 1
          ? node.arguments.types[1]
          : null;
      final everyError =
          e is DynamicType ||
          (e is InterfaceType &&
              e.classNode.name == 'Object' &&
              e.classNode.enclosingLibrary.importUri.toString() == 'dart:core');
      if (!everyError) {
        throw Unsupported(
          '`Future.onError` with an error type of its own',
          _sample(node),
        );
      }
      // With the extension's own parameters put in: the handler returns
      // `FutureOr<T>`, and lowered against the declaration it spelled a
      // `T` nothing here declares.
      final args = _arguments(
        node.arguments,
        target.function,
        true,
        _instantiated(node),
      );
      final value = node.arguments.types.isNotEmpty
          ? _type(node.arguments.types.first)
          : const IrType('dynamic');
      return IrCall(args.first, 'catch_error', args.sublist(1))
        ..rustType = IrType('Future', arguments: [value]);
    }
    final coreFunction =
        _coreTopLevel[target.enclosingLibrary.importUri
            .toString()]?[target.name.text];
    if (coreFunction != null && owner == null) {
      final (fn, slots) = coreFunction;
      final args = _arguments(node.arguments, target.function);
      // A prelude callee's slots are not widened into by `_widened` (its
      // generics take the value as it is); the table's are spelled here.
      if (slots != null) {
        for (var i = 0; i < args.length && i < slots.length; i++) {
          args[i] = coerce(args[i], slots[i]);
        }
      }
      return IrStaticCall(null, fn, args);
    }
    if (target.name.text == 'identical' && positional.length == 2) {
      // `identical(x, null)` is `x == null`: the null test the value's
      // representation answers (`IrIsNull`), not a comparison against a
      // `None` -- an `Object?` list element is an `Rc<dyn Object>` holding
      // the `Null` object (`_CompressedNode.put`'s `identical(keyOrNull,
      // null)`, run538).
      if (_isNull(positional[1])) return IrIsNull(expression(positional[0]));
      if (_isNull(positional[0])) return IrIsNull(expression(positional[1]));
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
        _topLevelName(target.name.text),
        _withGenericArgs(
          declaration,
          node.arguments,
          () => _arguments(
            node.arguments,
            declaration,
            true,
            _instantiated(node, declaration),
          ),
        ),
        fails: _fails(target),
        diverges: _diverges(target),
        asyncFn: _asyncMember(target),
        typeArguments: _keptTypeArguments(declaration, node.arguments),
        module: _topLevelModule(target),
      );
    }
    // `Future<T>.value()` with no value: no argument, rather than the
    // omitted optional filled in as `None` -- the prelude's `future_none`
    // makes the `null` of `T` (`()` for `void`), which a `None` is not
    // once the slot is the projected `T?` (+4 at ws570).
    if (owner == 'Future' &&
        target.name.text == 'value' &&
        node.arguments.positional.isEmpty) {
      // Spelled even though the callee is the prelude's -- `_keptTypeArguments`
      // gives a prelude callee none -- because `future_none`'s `T` has
      // nothing else to infer it from: `Future<void>.value()` in an `async`
      // body left `!` to the never-type fallback (5 at ws757).
      List<IrType> spelled() {
        try {
          return [for (final t in node.arguments.types) _typeNested(t)];
        } on Unsupported {
          return const [];
        }
      }

      return IrStaticCall(owner, 'value', const [], typeArguments: spelled());
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
      _withGenericArgs(
        declaration,
        node.arguments,
        () => _arguments(
          node.arguments,
          declaration,
          true,
          _instantiated(node, declaration),
        ),
      ),
      fails: _fails(target),
      diverges: _diverges(target),
      asyncFn: _asyncMember(target),
      // The prelude's `Future` constructors are generic functions with
      // nothing but the type argument to say what `T` is when no value
      // is handed in (`Future<void>.delayed(Duration.zero)` fell back to
      // the never type, the thenvoid fixture): the class's arguments,
      // spelled.
      typeArguments: owner == 'Future'
          ? _recordedTypes(node.arguments.types)
          : _keptTypeArguments(declaration, node.arguments),
      module: owner == null ? _topLevelModule(target) : null,
    );
  }

  /// `types` spelled, or none when one cannot be.
  List<IrType> _recordedTypes(List<DartType> types) {
    try {
      return [for (final t in types) _type(t)];
    } on Unsupported {
      return const [];
    }
  }

  /// dart:core members the prelude implements as a *sibling* declares
  /// them. Dart's `List<E>.from(Iterable elements)` takes anything and
  /// casts element by element; the prelude copies, as `List.of(Iterable<
  /// E>)` does, and so its slot is `of`'s: coerced into the declared
  /// `Iterable<dynamic>`, the argument was upcast on the way in and nothing
  /// cast it back (`Vec<Hct> <= Vec<Rc<dyn Object>>`, 11 at ws424).
  static const preludeSiblings = {
    'List.from': 'of',
    'Set.from': 'of',
    'Map.from': 'of',
    'HashSet.from': 'of',
    'LinkedHashSet.from': 'of',
    'HashMap.from': 'of',
    'LinkedHashMap.from': 'of',
    // `List.unmodifiable(Iterable)` / `Map.unmodifiable(Map)`: copies,
    // as `of`. `Set.removeAll(Iterable<Object?>)` and its siblings take
    // the set's own elements here, as `addAll(Iterable<E>)` does (ws580).
    'List.unmodifiable': 'of',
    'Map.unmodifiable': 'of',
    'Set.removeAll': 'addAll',
    'Set.retainAll': 'addAll',
    'Set.containsAll': 'addAll',
  };

  Procedure _preludeDeclaration(Procedure target) {
    final owner = target.enclosingClass;
    if (owner == null || _translatedClass(owner)) return target;
    final sibling = preludeSiblings['${owner.name}.${target.name.text}'];
    if (sibling == null) return target;
    for (final p in owner.procedures) {
      // ..of the same kind: a factory's sibling is a factory, an instance
      // method's (`Set.removeAll` -> `addAll`) an instance method.
      if (p.isStatic == target.isStatic &&
          p.name.text == sibling &&
          p.function.positionalParameters.length ==
              target.function.positionalParameters.length) {
        return p;
      }
    }
    return target;
  }

  /// A translated generic callee's type arguments for the type parameters
  /// it keeps (an erased one is its bound and has no slot), spelled as a
  /// turbofish; nothing for a prelude callee, whose Rust signature is its
  /// own, or when one cannot be spelled.
  ///
  /// As type arguments (`_typeNested`), like a class's (`_erasedArguments`):
  /// a `T?` put in for the callee's `R` is the slot `<T as DartNullable>::
  /// Or`, so `makeBox<T?>(..)` returns the same `Box<<T as DartNullable>::
  /// Or>` the local declaring it is spelled with, and the closure it takes
  /// -- whose parameter is that same `T?` -- has the callee's parameter
  /// type. Spelled `Option<T>` the two disagreed (`showMenu<T?>(..).then`
  /// in `_PopupMenuButtonState.showButtonMenu`, ws690).
  List<IrType> _keptTypeArguments(FunctionNode fn, Arguments arguments) {
    final parameters = fn.typeParameters;
    if (parameters.isEmpty || arguments.types.length != parameters.length) {
      return const [];
    }
    if (!_calleeTranslated(fn, null)) return const [];
    try {
      return [
        for (var i = 0; i < parameters.length; i++)
          if (!_erasedParameter(parameters[i])) _typeNested(arguments.types[i]),
      ];
    } on Unsupported {
      return const [];
    }
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
    List<DartType>? positionalTypes,
    Map<String, DartType>? namedTypes,
    List<IrType?>? positionalSlots,
  ]) {
    final was = _borrowedArgument;
    _borrowedArgument = borrows;
    try {
      final out = _argumentList(
        node,
        callee,
        instantiated,
        positionalTypes,
        namedTypes,
        positionalSlots,
      );
      // The hidden `Type` arguments a generic method takes (`_typeValues`):
      // each observed type argument as a value -- a `Type::of("X")`, or the
      // enclosing method's own hidden parameter when the argument is its
      // type parameter (`_findModels<T>` calling `getElement..<T>`).
      final target = callee?.parent;
      if (target is Procedure) {
        for (final i in _typeValues(target)) {
          out.add(
            _typeLiteral(
              i < node.types.length ? node.types[i] : const DynamicType(),
            ),
          );
        }
      }
      return out;
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
  /// `lower()`, with the slot's owner known (`_slotTranslated`).
  /// Whether the slot being widened into is a callee's parameter (an
  /// edge, spelled projected for a bare `T?`) rather than a body's own.
  bool _argumentEdge = false;

  T _asArgument<T>(T Function() widen) {
    final was = _argumentEdge;
    _argumentEdge = true;
    try {
      return widen();
    } finally {
      _argumentEdge = was;
    }
  }

  IrExpr _forCallee(
    FunctionNode? callee,
    DartType? declared,
    IrExpr lowered,
    IrExpr Function(IrExpr lowered) widen,
  ) {
    final was = _slotTranslated;
    final wasPrelude = _slotPrelude;
    _slotTranslated = _calleeTranslated(callee, declared);
    _slotPrelude = !_translatedCallee(callee) && callee != null;
    try {
      return widen(lowered);
    } finally {
      _slotTranslated = was;
      _slotPrelude = wasPrelude;
    }
  }

  /// Whether the slot being filled is a prelude callee's. Its function
  /// parameters are its own Rust signatures (`first_where_or` takes the
  /// element, not Dart's `T?`), and a closure handed to one is not adapted
  /// by the declared Dart type (132 new at ws419).
  var _slotPrelude = false;

  /// The prelude members that write into a list argument, by class and
  /// name: the positional indices handed out as `&mut` (see `IrMutRef`).
  static const _outBufferArguments = <String, Map<String, Set<int>>>{
    'RandomAccessFile': {
      'readInto': {0},
      'readIntoSync': {0},
    },
  };

  IrExpr _argument(
    Expression value,
    FunctionNode? callee,
    int index, [
    FunctionType? instantiated,
    IrType? slotIr,
    DartType? declaredOverride,
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
    // A super call fills the mixin's declared slots, with this class's
    // arguments put in (`declaredOverride`, see the super-call lowering).
    final declaredType = declaredOverride ?? param?.type;
    // ..or any declared type naming one: `Entry<S>(v)` with `Entry.T`
    // erased holds an `Rc<dyn Object>`, not the `S` the call put in (the
    // outparam fixture).
    final paramType =
        (declaredOverride == null
            ? _landingSlot(callee: callee, index: index)
            : null) ??
        (declaredType != null && _mentionsErased(declaredType)
            ? declaredType
            : instantiated != null &&
                  index < instantiated.positionalParameters.length
            ? instantiated.positionalParameters[index]
            : declaredType);
    // `DART2RUST_TRACE_ARG=<callee name>`: the slot each argument lands
    // in, to stderr.
    final calleeMember = callee?.parent;
    // A prelude call that *fills* its argument takes the place as `&mut`
    // (`IrMutRef`), never a copy: the table names them.
    if (calleeMember is Member) {
      final outs =
          _outBufferArguments[calleeMember.enclosingClass?.name]?[calleeMember
              .name
              .text];
      if (outs != null && outs.contains(index)) {
        return IrMutRef(expression(value));
      }
      // ..and a translated callee that fills the slot (`_fillsParameter`)
      // takes the caller's place the same way.
      // ..converted into the slot's own type first where it differs
      // (`List<ContainerLayer>` lent to a `List<ContainerLayer?>`,
      // `FollowerLayer._pathsToCommonAncestor`): a lent temporary, filled
      // and dropped -- what the copy did before -- rather than a type
      // error.
      if (calleeMember is Procedure && _fillsParameter(calleeMember, index)) {
        final place = expression(value);
        IrType? slotType;
        try {
          slotType = paramType == null ? null : _type(paramType);
        } on Unsupported {
          slotType = null;
        }
        // The place itself when the types agree: `_widened` clones a
        // local on its way into a slot, and the fill went into the clone.
        if (slotType == null ||
            place.rustType == null ||
            '${place.rustType}' == '$slotType') {
          return IrMutRef(place);
        }
        return IrMutRef(
          _widened(
            value,
            paramType,
            place,
            slotIr:
                slotIr ??
                _landingSlotIr(callee: callee, index: index) ??
                _topBound(declaredType, paramType) ??
                _genericSlotIr(callee, declaredType) ??
                _constructedSlotIr(callee, declaredType),
          ),
        );
      }
    }
    final tracedArg = Platform.environment['DART2RUST_TRACE_ARG'];
    if (calleeMember is Member &&
        (tracedArg == '*' ||
            tracedArg ==
                (calleeMember.enclosingClass?.name ??
                    calleeMember.name.text))) {
      stderr.writeln(
        'TRACE_ARG ${calleeMember.enclosingClass?.name}.${calleeMember.name.text}[$index] in=${_member?.enclosingClass?.name}.${_member?.name.text} '
        'declared=$declaredType instantiated=${instantiated?.positionalParameters} '
        'param=$paramType slotIr=$slotIr landing=${_landingSlotIr(callee: callee, index: index)} generic=${_genericSlotIr(callee, declaredType)}',
      );
    }
    final argument = _numLiteral(
      value,
      paramType,
      callee,
      _forCallee(
        callee,
        declaredType,
        _withBorrowing(
          param,
          callee,
          () => _withExpectedReturn(
            _instantiatedSlot(callee, paramType),
            value,
            () => expression(value),
          ),
        ),
        (lowered) => _asArgument(
          () => _widened(
            value,
            paramType,
            lowered,
            // A `T?` slot with `T` bound to a top type is the `Option<Rc<dyn
            // Object>>` the callee holds (`_topBound`; `DiagnosticsProperty<
            // Object?>(value: ..)`, ws499).
            slotIr:
                slotIr ??
                _landingSlotIr(callee: callee, index: index) ??
                _topBound(declaredType, paramType) ??
                _genericSlotIr(callee, declaredType) ??
                _constructedSlotIr(callee, declaredType),
          ),
        ),
      ),
    );
    if (calleeMember is Member &&
        (tracedArg == '*' ||
            tracedArg ==
                (calleeMember.enclosingClass?.name ??
                    calleeMember.name.text))) {
      stderr.writeln(
        'TRACE_ARG_OUT ${calleeMember.enclosingClass?.name}.${calleeMember.name.text}[$index] '
        '${argument.runtimeType} type=${argument.rustType}',
      );
    }
    // Into a projected slot of a translated callee: the spelled `T?`.
    return _translatedCallee(callee)
        ? _acrossBinding(
            argument,
            declaredType,
            _argumentBinding(callee, declaredType),
            toOption: false,
          )
        : argument;
  }

  /// A closure literal handed to a function-typed parameter returns what
  /// the *parameter's* type says: `String? Function(String)` taking
  /// `(l) => "default"` returns `Some("default")`. The closure's own
  /// return type is what it wrote, not what it is for (5 in intl).
  /// A function-typed slot with the callee's own instantiation put in.
  ///
  /// A generic class's constructor parameter still names the class's `T`
  /// (`OpenContainerBuilder<T>` is `Widget Function(BuildContext, void
  /// Function([T?]))`), and a closure written for it declared a parameter
  /// no name here stands for -- 4 `cannot find type T` at ws761, each one
  /// a `build` that then did not compile. The erased parameters stay as
  /// they are: those slots hold the bound, not what the call put in.
  DartType? _instantiatedSlot(FunctionNode? callee, DartType? param) {
    if (param is! FunctionType || callee == null) return param;
    final Map<TypeParameter, DartType> kept;
    if (identical(callee, _constructedCallee) && _constructedArgs.isNotEmpty) {
      kept = _constructedArgs;
    } else if (identical(callee, _genericCallee) && _genericArgs.isNotEmpty) {
      kept = _genericArgs;
    } else {
      return param;
    }
    final map = {
      for (final e in kept.entries)
        if (!_erasedParameter(e.key)) e.key: e.value,
    };
    if (map.isEmpty) return param;
    try {
      return Substitution.fromMap(map).substituteType(param);
    } on Object {
      return param;
    }
  }

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
  /// `e as f64`, once: a value already an `f64` -- `coerce` cast it on
  /// the way into the operand slot -- is left alone (`((1 as f64) as f64)`,
  /// 430 at ws356).
  static IrExpr _toF64(IrExpr e) {
    if (e.rustType?.name == 'double') return e;
    if (e is IrCast && e.rust == 'f64') return e;
    // Inside the `Some` the widening already put on: the cast belongs to
    // the value, not to the `Option` (`(Some(0) as f64)`, ws779).
    if (e is IrSome) {
      return IrSome(_toF64(e.value))
        ..rustType = const IrType('double', nullable: true);
    }
    // An integer *literal* is written as a float rather than cast: an
    // unsuffixed literal under `as f64` is an `i32` to Rust, and
    // `1000000000000000000 as f64` does not fit one (`NumberFormat.
    // _numberOfIntegerDigits`, 4 at ws777).
    if (e is IrLiteral && e.type.name == 'int') {
      // Suffixed: two unsuffixed float literals make an *ambiguous*
      // `{float}`, and a method on that does not resolve
      // (`(1000000.0 / 60.0).round()`, E0689 at ws779).
      return IrLiteral('${e.value}.0_f64', const IrType('double'))
        ..rustType = const IrType('double');
    }
    return IrCast(e, 'f64')..rustType = const IrType('double');
  }

  IrExpr _numLiteral(
    Expression value,
    DartType? param,
    FunctionNode? callee,
    IrExpr lowered,
  ) {
    if (param is! InterfaceType) return lowered;
    final slot = param.classNode.name;
    if (slot != 'num' && slot != 'double') return lowered;
    // Already an `f64` -- `coerce` cast it (`((1 as f64) as f64)`, ws356).
    if (lowered.rustType?.name == 'double') return lowered;
    // An integer *literal* where a `double` goes is a double, whoever
    // declares the slot: that is Dart's rule about the literal, not about
    // the callee (`lerpDouble(split, 1, transformed)` in `Split.transform`,
    // ws763). A `num` slot keeps the callee test below, where an `int` is
    // still an `int` unless the callee's `num` is this output's `f64`.
    if (slot == 'double') {
      // A literal, whichever way it arrives: type flow analysis turns one
      // into a `ConstantExpression`, and testing only for `IntLiteral`
      // missed every `lerpDouble(a, 0, t)` in the shape code (9 at ws779).
      final literal =
          value is IntLiteral ||
          (value is ConstantExpression && value.constant is IntConstant);
      if (!literal) return lowered;
      return _toF64(lowered)..rustType = const IrType('double');
    }
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
    // A *translated* callee's `num` is an `f64` here, whichever library
    // declares it: `lerpDouble(a, 0, t)` lives in `dart:ui` and its body is
    // translated, so its `num?` slots really are `Option<f64>` and an `int`
    // there does not fit. Only a callee the prelude answers keeps its `num`
    // as it was (9 at ws779).
    if (!_translatedCallee(callee)) return lowered;
    return _toF64(lowered)..rustType = const IrType('double');
  }

  IrExpr _namedArgument(
    Expression value,
    Object param, [
    DartType? declaredOverride,
  ]) {
    final callee = _calleeOf(param);
    final declared =
        declaredOverride ?? (param is FunctionParameter ? param.type : null);
    final type =
        (declaredOverride == null
            ? _landingSlot(
                callee: callee,
                name: param is FunctionParameter ? param.parameterName : null,
              )
            : null) ??
        declared;
    final tracedNamed = Platform.environment['DART2RUST_TRACE_NAMED'];
    final argument = _numLiteral(
      value,
      type,
      callee,
      _forCallee(
        callee,
        declared,
        _withBorrowing(
          param,
          callee,
          () => _withExpectedReturn(
            _instantiatedSlot(callee, type),
            value,
            () => expression(value),
          ),
        ),
        (lowered) {
          if (tracedNamed != null &&
              param is FunctionParameter &&
              param.parameterName == tracedNamed) {
            stderr.writeln(
              'TRACE_NAMED ${param.parameterName} declared=$declared type=$type '
              'lowered=${lowered.runtimeType} rust=${lowered.rustType} '
              'slotIr=${_constructedSlotIr(callee, declared)} '
              'translated=${_calleeTranslated(callee, declared)}',
            );
          }
          return _widened(
            value,
            type,
            lowered,
            slotIr:
                _landingSlotIr(
                  callee: callee,
                  name: param is FunctionParameter ? param.parameterName : null,
                ) ??
                _topBound(declared, _argumentBinding(callee, declared)) ??
                _genericSlotIr(callee, declared) ??
                _constructedSlotIr(callee, declared),
          );
        },
      ),
    );
    return _translatedCallee(callee)
        ? _acrossBinding(
            argument,
            declared,
            _argumentBinding(callee, declared),
            toOption: false,
          )
        : argument;
  }

  /// A trait handle into a slot whose *erased* parameter is bounded by a
  /// wider trait: `child`, a `RenderBox`, into `ContainerRenderObjectMixin
  /// .insert(ChildType child, {ChildType? after})`, which is `Rc<dyn
  /// RenderObject>` here. Rust upcasts a bare handle at the call and not
  /// one inside an `Option` (24 `_insertIntoChildList` at ws340), so the
  /// handle is upcast by name, through `map` when it is optional.
  /// The member an instance call lands on (`_landing`), and the receiver's
  /// type, while its arguments are lowered: a parameter's slot is that
  /// member's declared type -- a mixin clone's `RenderBox`, or the trait's
  /// erased bound -- with the receiver's arguments put in for the class's
  /// kept parameters, exactly as a read is typed (`_memberRustType`).
  Procedure? _dispatchMember;
  DartType? _dispatchReceiverType;

  /// A generic callee's own type arguments at the call, while its
  /// arguments are lowered: `T?` under `lerp<Color?>` is `Option<Option<
  /// Rc<dyn Color>>>` here, which Dart's instantiated type collapses.
  FunctionNode? _genericCallee;
  Map<TypeParameter, DartType> _genericArgs = const {};

  T _withGenericArgs<T>(
    FunctionNode fn,
    Arguments arguments,
    T Function() lower,
  ) {
    if (fn.typeParameters.isEmpty ||
        arguments.types.length != fn.typeParameters.length) {
      return lower();
    }
    final wasCallee = _genericCallee;
    final wasArgs = _genericArgs;
    _genericCallee = fn;
    _genericArgs = {
      for (var i = 0; i < fn.typeParameters.length; i++)
        fn.typeParameters[i]: arguments.types[i],
    };
    try {
      return lower();
    } finally {
      _genericCallee = wasCallee;
      _genericArgs = wasArgs;
    }
  }

  IrType? _genericSlotIr(FunctionNode? callee, DartType? declared) {
    if (declared == null ||
        callee == null ||
        !identical(callee, _genericCallee) ||
        _genericArgs.isEmpty) {
      return null;
    }
    if (!_mentionsParametersOf(declared, callee.typeParameters)) return null;
    // The receiver class's own parameters put in first: `Future<String>.
    // then<R>(FutureOr<R> Function(T))` takes a `String`, and a bare `T`
    // left in collided with the caller's `T` (`loadStructuredData<T>`'s
    // parser adapter took a `T`, run600).
    var substituted = declared;
    final receiverType = _dispatchReceiverType;
    if (receiverType is InterfaceType &&
        identical(callee, _dispatchInterface) &&
        receiverType.typeArguments.isNotEmpty) {
      substituted = Substitution.fromInterfaceType(receiverType)
          .substituteType(declared);
    }
    try {
      return _typeKept(substituted, _genericArgs, byTurbofish: true);
    } on Unsupported {
      return null;
    }
  }

  /// The slot a constructor's parameter is at the class's instantiation
  /// (`_constructing`): `SettingsListItem<ThemeMode?>(selectedOption: x)`
  /// takes an `Option<ThemeMode>` where the declaration says `T`, and a
  /// bare `T` widened nothing (`_SettingsPageState.build`, run660).
  IrType? _constructedSlotIr(FunctionNode? callee, DartType? declared) {
    if (declared == null ||
        callee == null ||
        !identical(callee, _constructedCallee) ||
        _constructedArgs.isEmpty) {
      return null;
    }
    final kept = {
      for (final e in _constructedArgs.entries)
        if (!_erasedParameter(e.key)) e.key: e.value,
    };
    if (kept.isEmpty || !_mentionsParametersOf(declared, kept.keys.toList())) {
      return null;
    }
    try {
      return _typeKept(declared, kept);
    } on Unsupported {
      return null;
    }
  }

  /// The interface member whose arguments the dispatch above is for: a
  /// call nested inside one of those arguments has a callee of its own
  /// (`Matrix4.rotationY(angle)` as an argument took the outer call's
  /// first parameter, 31 at ws369).
  FunctionNode? _dispatchInterface;

  DartType? _landingSlot({
    required FunctionNode? callee,
    int? index,
    String? name,
  }) {
    final landing = _dispatchMember;
    if (landing == null || !identical(callee, _dispatchInterface)) return null;
    final fn = landing.function;
    DartType? declared;
    if (index != null && index < fn.positionalParameters.length) {
      declared = fn.positionalParameters[index].type;
    } else if (name != null) {
      for (final p in fn.namedParameters) {
        if (p.parameterName == name) declared = p.type;
      }
    }
    if (declared == null) return null;
    // The method's own parameters are instantiated at the call: Dart's
    // type is the better answer there.
    if (fn.typeParameters.isNotEmpty &&
        _mentionsParametersOf(declared, fn.typeParameters)) {
      return null;
    }
    return _substituteKept(
      declared,
      landing.enclosingClass,
      _dispatchReceiverType,
    );
  }

  /// `_landingSlot` in the IR, `Option` layers kept apart (`_typeKept`).
  IrType? _landingSlotIr({
    required FunctionNode? callee,
    int? index,
    String? name,
  }) {
    final declared = _landingSlot(callee: callee, index: index, name: name);
    final landing = _dispatchMember;
    if (declared == null || landing == null) return null;
    // The declared type again, unsubstituted, for the IR-level put-in.
    final fn = landing.function;
    DartType? raw;
    if (index != null && index < fn.positionalParameters.length) {
      raw = fn.positionalParameters[index].type;
    } else if (name != null) {
      for (final p in fn.namedParameters) {
        if (p.parameterName == name) raw = p.type;
      }
    }
    if (raw == null) return null;
    try {
      return _typeKept(
        raw,
        _keptFor(landing.enclosingClass, _dispatchReceiverType),
      );
    } on Unsupported {
      return null;
    }
  }

  /// The receiver's type arguments for `owner`'s *kept* parameters (the
  /// erased ones are left to `_type`, which spells them as their bound).
  Map<TypeParameter, DartType> _keptFor(Class? owner, DartType? receiverType) {
    final env = typeEnvironment;
    if (owner == null ||
        owner.typeParameters.isEmpty ||
        env == null ||
        receiverType is! InterfaceType) {
      return const {};
    }
    final asOwner = env.hierarchy.getTypeAsInstanceOf(receiverType, owner);
    if (asOwner is! InterfaceType) return const {};
    final kept = <TypeParameter, DartType>{};
    for (
      var i = 0;
      i < owner.typeParameters.length && i < asOwner.typeArguments.length;
      i++
    ) {
      final p = owner.typeParameters[i];
      if (!_erasedParameter(p)) kept[p] = asOwner.typeArguments[i];
    }
    return kept;
  }

  /// `declared` as a Rust type with `kept` put in for its parameters --
  /// in the IR, not in Kernel, because Dart collapses `T?` with `T` bound
  /// to `Color?` into `Color?` and Rust's `Option<T>` does not: that is
  /// `Option<Option<Rc<dyn Color>>>` here (the `WidgetStateProperty<
  /// Color?>.lerp` family, 66 mismatches at ws384).
  IrType _typeKept(
    DartType t,
    Map<TypeParameter, DartType> kept, {
    bool byTurbofish = false,
  }) {
    if (t is TypeParameterType && kept.containsKey(t.parameter)) {
      // What is put in is a type argument: a `U?` there is projected.
      final arg = _typeNested(kept[t.parameter]!);
      if (t.nullability != Nullability.nullable) {
        // A generic *method*'s own parameter is instantiated by what its
        // turbofish spells, and that is the plain `Option<T>` -- so the slot
        // is that and not the projection this declaration uses for its own
        // edges (`entry.complete<T?>(result)` in `Navigator.removeRoute`, 14
        // at ws761). A *class*'s instantiation is not spelled that way: the
        // struct is named `SettingsListItem<<T as DartNullable>::Or>` and
        // its fields keep the projection (ws763).
        return byTurbofish && arg.projected
            ? IrType(arg.name, nullable: true, arguments: arg.arguments)
            : arg;
      }
      // `T?` with `T` bound to `X?` is `X?`, as Dart collapses it and as
      // rustc normalises the projected signature to (`<Option<X> as
      // DartNullable>::Or` is `Option<X>`): the plain `Option`, projected
      // no more. With `T` bound to a bare `U` the slot stays `Or`.
      // ..unless what is put in is itself a projected `U?` of the code
      // here: `<<U as DartNullable>::Or as DartNullable>::Or` normalises
      // to `<U as DartNullable>::Or`, still projected (`Tile<T?>`'s slots
      // from a `Picker<T>`, fixture closureedge).
      if (isNullable(arg)) {
        return arg.projected
            ? arg
            : IrType(arg.name, nullable: true, arguments: arg.arguments);
      }
      // Projected only over a bare type parameter of the code here: over a
      // concrete class the slot normalises to the plain `Option` (and
      // `<GestureBinding as DartNullable>` names a trait as a type).
      final put = kept[t.parameter]!;
      // A function type keeps its signature (see the map read's typing).
      if (arg.isFunction) {
        return IrType.function(arg.parameters!, arg.returns!, nullable: true);
      }
      return IrType(
        arg.name,
        nullable: true,
        arguments: arg.arguments,
        projected: _projectedSlot(
          put.withDeclaredNullability(Nullability.nullable),
        ),
      );
    }
    if (t is InterfaceType && kept.isNotEmpty) {
      final base = _type(t);
      final cls = t.classNode;
      return IrType(
        base.name,
        nullable: base.nullable,
        arguments: [
          for (var i = 0; i < t.typeArguments.length; i++)
            if (i >= cls.typeParameters.length ||
                !_erasedParameter(cls.typeParameters[i]))
              _typeKept(t.typeArguments[i], kept),
        ],
      );
    }
    if (t is FunctionType && kept.isNotEmpty) {
      final named = [...t.namedParameters]
        ..sort((a, b) => a.name.compareTo(b.name));
      return IrType.function(
        [
          for (final p in t.positionalParameters) _typeKept(p, kept),
          for (final p in named) _typeKept(p.type, kept),
        ],
        _typeKept(t.returnType, kept),
        nullable: t.nullability == Nullability.nullable,
      );
    }
    // `FutureOr<T>` with the call's `T` put in: a parser slot `FutureOr<T>
    // Function(ByteData)` at `loadStructuredBinaryData<AssetManifest>(..)`
    // takes the `_AssetManifestBin` a factory returns *as* a `dyn
    // AssetManifest`, which a `T` left in said nothing about (run569).
    if (t is FutureOrType && kept.isNotEmpty) {
      final base = _type(t);
      return IrType(
        base.name,
        nullable: base.nullable,
        arguments: [_typeKept(t.typeArgument, kept)],
      );
    }
    return _type(t);
  }

  /// `declared` with the receiver's type arguments put in for `owner`'s
  /// *kept* parameters; the erased ones stay, for `_type` to spell as
  /// their bound.
  DartType _substituteKept(
    DartType declared,
    Class? owner,
    DartType? receiverType,
  ) {
    final env = typeEnvironment;
    if (owner == null ||
        owner.typeParameters.isEmpty ||
        env == null ||
        receiverType is! InterfaceType) {
      return declared;
    }
    final asOwner = env.hierarchy.getTypeAsInstanceOf(receiverType, owner);
    if (asOwner is! InterfaceType) return declared;
    final kept = <TypeParameter, DartType>{};
    for (
      var i = 0;
      i < owner.typeParameters.length && i < asOwner.typeArguments.length;
      i++
    ) {
      final p = owner.typeParameters[i];
      if (!_erasedParameter(p)) kept[p] = asOwner.typeArguments[i];
    }
    return Substitution.fromMap(kept).substituteType(declared);
  }

  /// Whether a member landing on a field is that field for this class: a
  /// struct holds its clones' fields; a trait body (a mixin, an abstract
  /// or an open class) reaches its own through accessors, and a direct
  /// field write there made the mutation analysis ask for `&mut self`
  /// (12 "incompatible type for trait", ws373).
  bool _heldField(Member interface, Expression receiver) {
    final on = receiver is ThisExpression ? _lowering : _staticClass(receiver);
    // ..and on another object only when that object is a struct: a handle
    // to an open class has accessors, not fields (12 "attempted to take
    // value of method", ws379).
    if (on == null || _abstractLike(on)) return false;
    return _landing(interface, receiver) is Field;
  }

  /// The type a write into `interface` on `receiver` must produce: the
  /// landing member's -- a mixin clone's field, or the trait's setter.
  /// The mixin's own member behind a copy the CFE made in an anonymous
  /// application (`_MixinApplication8&RenderBox&RenderObjectWithChildMixin
  /// .child=` for `RenderObjectWithChildMixin.child=`): what the trait
  /// declares, with the mixin's parameter (`ChildType?`) where the copy
  /// has the application's argument (`RenderBox?`).
  /// Whether `t` names a type parameter that is *kept* (not erased): a
  /// copy's type substituted for one is that application's own, and the
  /// declaration's cannot replace it (`LayoutInfoType get layoutInfo`
  /// returning `BoxConstraints` in `RenderLayoutBuilder`, ws477).
  bool _mentionsKeptParameter(DartType t) {
    if (t is FutureOrType) return _mentionsKeptParameter(t.typeArgument);
    if (t is RecordType) {
      return t.positional.any(_mentionsKeptParameter) ||
          t.named.any((n) => _mentionsKeptParameter(n.type));
    }
    if (t is TypeParameterType) return !_erasedParameter(t.parameter);
    if (t is InterfaceType) {
      return t.typeArguments.any(_mentionsKeptParameter);
    }
    if (t is FunctionType) {
      return _mentionsKeptParameter(t.returnType) ||
          t.positionalParameters.any(_mentionsKeptParameter) ||
          t.namedParameters.any((n) => _mentionsKeptParameter(n.type));
    }
    return false;
  }

  /// The declaration a copy in an anonymous application is lowered under
  /// (see `_lowerProcedure`'s `signature`), or null for a member that is
  /// its own declaration.
  Procedure? _cloneSignature(Procedure p) {
    final original = _originalOf(p);
    return identical(original, p) || original is! Procedure ? null : original;
  }

  Member _originalOf(Member m, {bool forWrite = false}) {
    final owner = m.enclosingClass;
    if (owner == null || !owner.isAnonymousMixin) return m;
    final setter = m is Procedure && m.isSetter;
    final getter = m is Procedure && m.isGetter;
    for (final st in [
      if (owner.mixedInType != null) owner.mixedInType!,
      ...owner.implementedTypes,
    ]) {
      for (final o in st.classNode.members) {
        if (o.name.text != m.name.text) continue;
        if (m is Field) {
          // A hollow declaration keeps a field as an abstract getter and
          // setter pair (`ChildType? get _lastChild` / `set _lastChild`
          // in `ContainerRenderObjectMixin`, ws478).
          if (o is Field) return o;
          if (o is Procedure && (forWrite ? o.isSetter : o.isGetter)) {
            return o;
          }
          continue;
        }
        if (o is Procedure && o.isSetter == setter && o.isGetter == getter) {
          return o;
        }
      }
    }
    return m;
  }

  /// A super call's slots as this class sees them: the declaration's
  /// parameter types (the mixin's, behind a copy) with this class's
  /// arguments put in for the mixin's kept parameters.
  ///
  /// A copy whose declaration no longer lists the member (TFA dropped it
  /// there) is typed the way the trait was: the application's arguments
  /// taken back out (`_unapplied`) and this class's put in. Untyped, the
  /// argument went to the mixin's super body as the copy's `Panel` where
  /// the trait's erased `S` says `Rc<dyn Widget>` (the unapply fixture).
  (List<DartType>?, Map<String, DartType>?) _superSlots(Member target) {
    final original = _originalOf(target);
    if (original is! Procedure) return (null, null);
    final Class? owner;
    final DartType Function(DartType) declared;
    if (identical(original, target)) {
      final application = target.enclosingClass;
      if (application == null || !application.isAnonymousMixin) {
        return (null, null);
      }
      // A deduplicated application (`dart:mixin_deduplication`) has no
      // `mixedInType`; the mixin is among its `implementedTypes`.
      final mixin =
          application.mixedInType?.classNode ??
          application.implementedTypes
              .map((st) => st.classNode)
              .where((c) => c.isMixinDeclaration)
              .firstOrNull;
      if (mixin == null) return (null, null);
      owner = mixin;
      declared = (t) => _unapplied(t, application, mixin);
    } else {
      owner = original.enclosingClass;
      declared = (t) => t;
    }
    final fn = original.function;
    if (Platform.environment['DART2RUST_TRACE_SUPER'] != null) {
      stderr.writeln(
        'TRACE_SUPER ${target.enclosingClass?.name}.${target.name.text} same=${identical(original, target)} owner=${owner?.name} '
        'slots=${[for (final p in fn.positionalParameters) '${p.type} -> ${declared(p.type)} -> ${_asApplied(declared(p.type), owner)}']}',
      );
    }
    return (
      [
        for (final p in fn.positionalParameters)
          _asApplied(declared(p.type), owner),
      ],
      {
        for (final p in fn.namedParameters)
          p.parameterName: _asApplied(declared(p.type), owner),
      },
    );
  }

  /// What a super read of `target` hands back, as this class sees it:
  /// the declaration's type (the mixin's, behind a copy; a copy whose
  /// declaration is gone unapplied, as `_superSlots` does) with this
  /// class's arguments put in for the kept parameters. Null where this
  /// compiler has no spelling for it.
  IrType? _superReturn(Member target) {
    final original = _originalOf(target);
    final Class? owner;
    final DartType Function(DartType) declared;
    if (identical(original, target)) {
      final application = target.enclosingClass;
      if (application != null && application.isAnonymousMixin) {
        final mixin =
            application.mixedInType?.classNode ??
            application.implementedTypes
                .map((st) => st.classNode)
                .where((c) => c.isMixinDeclaration)
                .firstOrNull;
        if (mixin == null) return null;
        owner = mixin;
        declared = (t) => _unapplied(t, application, mixin);
      } else {
        owner = target.enclosingClass;
        declared = (t) => t;
      }
    } else {
      owner = original.enclosingClass;
      declared = (t) => t;
    }
    final DartType? type = original is Procedure
        ? original.function.returnType
        : original is Field
        ? original.type
        : null;
    if (type == null) return null;
    return _recordedType(_asApplied(declared(type), owner));
  }

  /// The function whose parameters a call to `m` fills: the mixin's own
  /// declaration behind a copy (see `_originalOf`).
  FunctionNode _originalFunction(Member m) {
    final original = _originalOf(m);
    return original is Procedure ? original.function : m.function!;
  }

  /// A type of a copy in `application`, with the arguments the application
  /// put in for `mixin`'s parameters taken back out: `Slot` where the
  /// application implements `SlottedContainer<Slot, RenderBox>` reads as
  /// `SlotType` again. An erased parameter's argument is taken out too:
  /// the parameter reads as its bound, which is what the trait says
  /// everywhere -- left in, `RestorationMixin<S>.didUpdateWidget(S)` was
  /// declared on the trait with one application's `DatePickerDialog`, and
  /// every other implementor's forwarder mismatched (ws535). Structural,
  /// so an argument that also occurs on its own in the type is taken for
  /// the parameter too -- the copy is the CFE's substitution, and this is
  /// its inverse.
  DartType _unapplied(DartType t, Class application, Class mixin) {
    Supertype? applied;
    if (application.mixedInType?.classNode == mixin) {
      applied = application.mixedInType;
    } else {
      for (final st in application.implementedTypes) {
        if (st.classNode == mixin) applied = st;
      }
    }
    if (applied == null) return t;
    final back = <DartType, TypeParameter>{};
    for (var i = 0; i < mixin.typeParameters.length; i++) {
      if (i >= applied.typeArguments.length) break;
      final p = mixin.typeParameters[i];
      final a = applied.typeArguments[i].withDeclaredNullability(
        Nullability.nonNullable,
      );
      if (a is TypeParameterType && a.parameter == p) continue;
      back[a] = p;
    }
    if (back.isEmpty) return t;
    DartType walk(DartType x) {
      final bare = x.withDeclaredNullability(Nullability.nonNullable);
      final p = back[bare];
      if (p != null) return TypeParameterType(p, x.nullability);
      if (x is InterfaceType) {
        return InterfaceType(x.classNode, x.nullability, [
          for (final a in x.typeArguments) walk(a),
        ]);
      }
      if (x is FunctionType) {
        return FunctionType(
          [for (final a in x.positionalParameters) walk(a)],
          walk(x.returnType),
          x.nullability,
          namedParameters: [
            for (final n in x.namedParameters)
              NamedType(n.name, walk(n.type), isRequired: n.isRequired),
          ],
          typeParameters: x.typeParameters,
          requiredParameterCount: x.requiredParameterCount,
        );
      }
      return x;
    }

    return walk(t);
  }

  /// A declared type of `owner`'s (a mixin's) with `owner`'s parameters
  /// substituted by the class being lowered's arguments for them.
  /// A mixin's body comes from an *application* (`_appliedBody`), where the
  /// CFE has already put the application's arguments in for the mixin's
  /// parameters: `ContainerRenderObjectMixin.visitChildren` copied into
  /// `RenderFlex`'s application casts to `FlexParentData` where the mixin
  /// wrote `ParentDataType`. Lowered as the *trait's* default that body
  /// serves every application, so the argument goes back to the parameter
  /// -- and only for an **erased** one, whose spelling is its bound, the
  /// trait everything reads through anyway (`RenderSliverList` cast a
  /// `SliverMultiBoxAdaptorParentData` to `FlexParentData`, run734).
  Map<DartType, DartType> _appliedBack = const {};

  /// A type as this *body* holds it: an applied mixin body's concrete
  /// argument is the mixin's erased parameter here (`_appliedBack`).
  DartType? _backHere(DartType? t) {
    if (t is! InterfaceType || _appliedBack.isEmpty) return t;
    final back =
        _appliedBack[t.withDeclaredNullability(Nullability.nonNullable)];
    if (back == null) return t;
    return t.nullability == Nullability.nullable
        ? back.withDeclaredNullability(Nullability.nullable)
        : back;
  }

  Map<DartType, DartType> _appliedBackMap(Class mixin, Class? application) {
    if (application == null) return const {};
    Supertype? applied;
    for (final t in application.implementedTypes) {
      if (t.classNode == mixin) applied = t;
    }
    if (applied == null) return const {};
    final out = <DartType, DartType>{};
    for (var i = 0; i < applied.typeArguments.length; i++) {
      if (i >= mixin.typeParameters.length) break;
      final p = mixin.typeParameters[i];
      if (!_erasedParameter(p)) continue;
      final argument = applied.typeArguments[i];
      if (argument is! InterfaceType) continue;
      out[argument.withDeclaredNullability(Nullability.nonNullable)] =
          TypeParameterType(p, Nullability.nonNullable);
    }
    return out;
  }

  DartType _asApplied(DartType declared, Class? owner) {
    final env = typeEnvironment;
    final thisType = env == null
        ? null
        : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable);
    if (owner == null || thisType is! InterfaceType) return declared;
    // The *kept* parameters only (`_keptFor`): an erased one is its bound
    // everywhere, the trait included, and `ChildType` put in as `RenderBox`
    // brought the `RenderBox`-typed copies back (+47 at ws489).
    final kept = _keptFor(owner, thisType);
    if (kept.isEmpty) return declared;
    return Substitution.fromMap(kept).substituteType(declared);
  }

  /// A field's type as this class holds it: a copy's by the mixin's
  /// declaration with this class's arguments put in (`_asApplied`), its
  /// own otherwise. The same answer for the declaration and for the
  /// initialiser the CFE moved into the application's constructor.
  DartType _fieldTypeHere(Field field) {
    final declared = _declaredFieldType(field);
    if (declared == null) return field.type;
    return _asApplied(declared, _originalOf(field).enclosingClass);
  }

  /// The type a copy's field is declared with (see `_originalOf`), or
  /// null for a field that is its own declaration.
  DartType? _declaredFieldType(Field field) {
    final original = _originalOf(field);
    if (identical(original, field)) return null;
    if (original is Field) return original.type;
    if (original is Procedure && original.isGetter) {
      return original.function.returnType;
    }
    return null;
  }

  /// Where a write lands for its slot's type: through the trait's setter
  /// (a qualified write) it is the declaring member's, the mixin's own
  /// for a copy in an application (`this.child = child` in `RenderView`'s
  /// constructor, `RenderBox?` there and `RenderObject?` in the trait,
  /// ws476); a plain write lands on the class's own.
  Member _writeLanding(Member interface, Expression receiver) => _originalOf(
    _setterQualifier(receiver, interface) != null
        ? interface
        : _landing(interface, receiver),
    forWrite: true,
  );

  DartType _writeSlot(Member interface, Expression receiver) {
    final landing = _writeLanding(interface, receiver);
    final declared = landing is Procedure && landing.isSetter
        ? landing.function.positionalParameters.single.type
        : landing.setterType;
    final env = typeEnvironment;
    final receiverType = receiver is ThisExpression
        ? (env == null
              ? null
              : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable))
        : _staticType(receiver);
    return _substituteKept(declared, landing.enclosingClass, receiverType);
  }

  /// `_writeSlot` in the IR, `Option` layers kept apart (`_typeKept`).
  IrType? _writeSlotIr(Member interface, Expression receiver) {
    final landing = _writeLanding(interface, receiver);
    final declared = landing is Procedure && landing.isSetter
        ? landing.function.positionalParameters.single.type
        : landing.setterType;
    final env = typeEnvironment;
    final receiverType = receiver is ThisExpression
        ? (env == null
              ? null
              : _lowering?.getThisType(env.coreTypes, Nullability.nonNullable))
        : _staticType(receiver);
    try {
      return _typeKept(
        declared,
        _keptFor(landing.enclosingClass, receiverType),
      );
    } on Unsupported {
      return null;
    }
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
    // Typed as the map it is, so a slot of another type adapts it: one
    // returned where `dynamic` goes is put behind a handle
    // (`_handlePlatformMessage`'s `{'response': ..}`, run460).
    return IrMapLiteral(
      [
        for (final entry in node.entries)
          (
            _widened(entry.key, keyType, expression(entry.key)),
            _widened(entry.value, valueType, expression(entry.value)),
          ),
      ],
      _type(keyType),
      _type(valueType),
    )..rustType = IrType('Map', arguments: [_type(keyType), _type(valueType)]);
  }

  /// A list literal's elements into `element`. The CFE keeps a literal of
  /// more than eight elements as a node (the `_literalN` constructors stop
  /// there): its elements widen and share into the element type exactly as
  /// the short ones' do.
  /// A record literal's fields into `fields` -- the literal's own types,
  /// or the slot's when it lands in one of other field types (see
  /// `_widenedInto`): `(false, null)` returned as a `(bool, Object?)`
  /// holds the `Null` object, `(true, x)` boxes its `int` (ws502, ws509).
  /// As translated slots, whatever call the record sits in.
  IrExpr _recordLiteral(
    RecordLiteral node,
    List<DartType> fields, [
    List<NamedType> named = const [],
  ]) {
    // The named fields in the *slot's* order, found by name: a literal
    // writes them in source order and the type spells them sorted.
    final namedFields = named.isEmpty ? node.recordType.named : named;
    NamedExpression written(String name) =>
        node.named.firstWhere((e) => e.name == name);
    final wasTranslated = _slotTranslated;
    final wasPrelude = _slotPrelude;
    _slotTranslated = true;
    _slotPrelude = false;
    try {
      return IrRecord([
          for (var i = 0; i < node.positional.length; i++)
            i < fields.length
                ? _widened(
                    node.positional[i],
                    fields[i],
                    expression(node.positional[i]),
                  )
                : expression(node.positional[i]),
          for (final n in namedFields)
            _widened(
              written(n.name).value,
              n.type,
              expression(written(n.name).value),
            ),
        ])
        ..rustType = IrType(
          'Record',
          arguments: _nested(
            () => [
              for (var i = 0; i < node.positional.length; i++)
                _recordedType(
                      i < fields.length
                          ? fields[i]
                          : node.recordType.positional[i],
                    ) ??
                    const IrType('dynamic'),
              for (final n in namedFields)
                _recordedType(n.type) ?? const IrType('dynamic'),
            ],
          ),
        );
    } finally {
      _slotTranslated = wasTranslated;
      _slotPrelude = wasPrelude;
    }
  }

  IrExpr _listLiteral(ListLiteral node, DartType element) {
    return IrListLiteral([
      for (final e in node.expressions)
        _widened(
          e,
          element,
          _withExpectedReturn(element, e, () => expression(e)),
        ),
    ], _type(element));
  }

  // -- Coercion by type ------------------------------------------------------
  //
  // One rule for a value entering a slot: compare the value's Rust type
  // (`IrExpr.rustType`) with the slot's, and adapt the difference --
  // an `Option` layer, a scalar widening, a handle up or down the trait
  // hierarchy, a value put behind a handle, a collection rebuilt element by
  // element. The shape rules in `_widened` below each did one of these for
  // one syntactic shape; by ws355 there were 26 of them and they had begun
  // to disagree. This replaces them as it proves to cover them.

  Map<String, Class>? _classesByName;

  /// The static and top-level fields some body mutates in place: the
  /// receivers of a collection mutator (`_mutatingListNames`), of a field
  /// write, or a `List`/`Set` argument a callee fills -- also through a
  /// cascade's `let #t = field in #t.add(..)`. Once, over every translated
  /// library of the component: a library may fill another's.
  late final Set<Field> _mutatedStatics = () {
    final finder = _StaticFillFinder(this);
    final component = library.enclosingComponent;
    if (component != null) {
      for (final l in component.libraries) {
        if (!_translatedLibrary(l)) continue;
        l.accept(finder);
      }
    } else {
      library.accept(finder);
    }
    return finder.found;
  }();

  bool _translatedLibrary(Library l) {
    final uri = l.importUri;
    return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
  }

  Class? _classNamed(String name) {
    final index = _classesByName ??= () {
      final out = <String, Class>{};
      final component = library.enclosingComponent;
      if (component != null) {
        for (final l in component.libraries) {
          for (final c in l.classes) {
            out.putIfAbsent(c.name, () => c);
          }
        }
      }
      for (final c in library.classes) {
        out[c.name] = c;
      }
      return out;
    }();
    return index[name];
  }

  static const _scalarNames = {'int', 'double', 'num', 'bool', 'String'};
  static const _collectionNames = {'List', 'Iterable', 'Set'};

  bool _isTraitName(String name) {
    if (const {
      'Object',
      'dynamic',
      'Comparable',
      'DartIterator',
    }.contains(name)) {
      return true;
    }
    if (_scalarNames.contains(name) || _collectionNames.contains(name)) {
      return false;
    }
    // This library's own class first: `dart:ui`'s `Gradient` is a struct
    // while `package:flutter`'s is abstract, and the name alone said
    // trait (`Gradient as Rc<dyn Object>` on a value, ws456).
    final c = _classNamed(name);
    if (c != null && identical(c.enclosingLibrary, library)) {
      return _translatedClass(c) && _abstractLike(c);
    }
    if (abstractElsewhere.contains(name)) return true;
    return c != null && _translatedClass(c) && _abstractLike(c);
  }

  bool _isCountedName(String name) {
    final known = elsewhere[name];
    if (known != null) return known.counted;
    final c = _classNamed(name);
    return c != null && _translatedClass(c) && _countedClass(c);
  }

  /// Whether `c` is counted, decided once per class: the rule reads
  /// `_sharedFields`, which is the class *being lowered*'s, and asked of
  /// another class from inside a method it answered by the wrong fields
  /// (`Semantics` boxed twice from `routes.dart`, ws512).
  final _countedCache = <Class, bool>{};

  bool _countedClass(Class c) => _countedCache.putIfAbsent(c, () {
    final saved = _sharedFields;
    _sharedFields = _closureFields(c);
    try {
      return _closureCallsMethod(c);
    } finally {
      _sharedFields = saved;
    }
  });

  bool _isStructName(String name) {
    if (_isTraitName(name) || _scalarNames.contains(name)) return false;
    final c = _classNamed(name);
    return c != null && _translatedClass(c) && !c.isEnum;
  }

  bool _isEnumName(String name) {
    final c = _classNamed(name);
    return c != null && _translatedClass(c) && c.isEnum;
  }

  bool _isBelow(String sub, String sup) {
    final a = _classNamed(sub);
    final b = _classNamed(sup);
    final hierarchy = typeEnvironment?.hierarchy;
    if (a == null || b == null || hierarchy == null) return false;
    return a == b || hierarchy.isSubInterfaceOf(a, b);
  }

  // The rule itself lives in `coerce.dart`; this class is its `TypeWorld`.
  @override
  bool isTrait(String name) => _isTraitName(name);

  @override
  bool isCounted(String name) => _isCountedName(name);

  @override
  bool isStruct(String name) => _isStructName(name);

  @override
  bool isEnum(String name) => _isEnumName(name);

  @override
  bool isBelow(String sub, String sup) => _isBelow(sub, sup);

  @override
  bool isGenericValueStruct(String name) {
    final c = _classNamed(name);
    return c != null && !_closureCallsMethod(c) && c.typeParameters.isNotEmpty;
  }

  bool _sameRust(IrType a, IrType b) => sameRust(a, b);

  static IrType _nonNull(IrType t) => nonNull(t);

  static String _normalName(String name) => normalName(name);

  /// `value`, adapted to `slot`: see `coerceInto`.
  @override
  bool isTypeParameter(String name) {
    final member = _member;
    final fn = member is Procedure
        ? member.function
        : member is Constructor
        ? member.function
        : null;
    return (fn?.typeParameters.any((p) => p.name == name) ?? false) ||
        (_lowering?.typeParameters.any((p) => p.name == name) ?? false);
  }

  IrExpr coerce(IrExpr value, IrType slot, {bool inClosure = false}) =>
      coerceInto(value, slot, this, inClosure: inClosure);

  IrExpr _widened(
    Expression value,
    DartType? param,
    IrExpr lowered, {
    IrType? slotIr,
  }) {
    // The callee flag (`_slotTranslated`) is about this slot; whatever is
    // lowered underneath -- a literal's entries against the slot's element
    // types -- fills slots of its own, translated ones.
    final translated = _slotTranslated;
    final prelude = _slotPrelude;
    _slotTranslated = true;
    _slotPrelude = false;
    try {
      // ..except into a prelude callee's bare `Function` slot, which
      // takes the function *object* (see `_calleeTranslated`).
      return _widenedInto(
        value,
        param,
        lowered,
        translated:
            translated &&
            !(prelude && lowered is IrClosure && !_bareFunctionType(param)),
        prelude: prelude,
        slotIr: slotIr,
      );
    } finally {
      _slotTranslated = translated;
      _slotPrelude = prelude;
    }
  }

  /// A `let` is its body: the type flow analysis folds `a ?? b` with an
  /// always-null `a` into `let #t = a in b`, and the tear-off inside is
  /// what the slot takes (`requestFocusCallback ?? FocusTraversalPolicy.
  /// defaultTraversalRequestFocusCallback`, run523).
  static Expression _throughLets(Expression e) {
    var out = e;
    while (out is Let) {
      out = out.body;
    }
    return out;
  }

  /// A static tear-off into a slot whose type keeps named parameters: an
  /// adapter taking the type's (sorted) order and calling in the
  /// declaration's (see `_widenedInto`), or null when the orders agree.
  IrExpr? _namedOrderAdapter(
    Expression value,
    DartType? param,
    IrExpr lowered,
  ) {
    final bare = _throughLets(value);
    // ..a constructor's the same way (`RoundedRectangleBorder.new` as a
    // `ShapeBorder Function({side, borderRadius})`, ws570).
    final torn = bare is StaticTearOff
        ? bare.target
        : bare is ConstantExpression && bare.constant is TearOffConstant
        ? (bare.constant as TearOffConstant).target
        : null;
    final tornFunction = torn?.function;
    if (torn == null ||
        tornFunction == null ||
        param is! FunctionType ||
        param.namedParameters.isEmpty ||
        param.positionalParameters.length !=
            tornFunction.positionalParameters.length) {
      return null;
    }
    final declared = [
      for (final n in tornFunction.namedParameters) n.parameterName,
    ];
    final byType = [for (final n in param.namedParameters) n.name];
    if (declared.length != byType.length ||
        !declared.toSet().containsAll(byType) ||
        _sameOrder(declared, byType)) {
      return null;
    }
    final params = <IrParam>[];
    final positional = <IrExpr>[];
    for (var i = 0; i < param.positionalParameters.length; i++) {
      final name = '__a$i';
      params.add(IrParam(name, _paramType(param.positionalParameters[i])));
      positional.add(IrLocal(name));
    }
    final byName = <String, IrExpr>{};
    for (final n in param.namedParameters) {
      final name = '__n_${n.name}';
      params.add(IrParam(name, _paramType(n.type)));
      byName[n.name] = IrLocal(name);
    }
    IrType? slot;
    try {
      slot = _type(param);
    } on Unsupported {
      slot = null;
    }
    // The value bound first and moved in: emitted inside the closure it
    // borrowed the constructor's parameter (`request_focus_callback` in
    // the `let` the analysis left, "does not live long enough", run524).
    return IrBlockValue(
      [IrLocalDecl('__f', null, lowered)],
      IrCall(
        IrClosure(
          params,
          // ..the result into the slot's return: a constructor's instance
          // as the trait the slot returns (`Rounded` as a `dyn Shape`).
          IrReturn(
            coerce(
              IrCallValue(IrLocal('__f'), [
                ...positional,
                for (final n in tornFunction.namedParameters)
                  byName[n.parameterName]!,
              ])..rustType = _functionRefType(torn)?.returns,
              _type(param.returnType),
            ),
          ),
          _type(param.returnType),
          locals: const ['__f'],
        ),
        '!rc',
        const [],
      ),
    )..rustType = slot;
  }

  IrExpr _widenedInto(
    Expression value,
    DartType? param,
    IrExpr lowered, {
    required bool translated,
    bool prelude = false,
    IrType? slotIr,
  }) {
    // A literal into a collection slot of other element types is lowered
    // again against those: see `_mapLiteral`.
    // ..not into a prelude callee's slot whose element types are top
    // types: `List.unmodifiable(Iterable)`, `Set.removeAll(Iterable<
    // Object?>)` are generic over what they are given, and a `[3, 1, 2]`
    // re-lowered as `Vec<Rc<dyn Object>>` fit neither (ws580).
    final rawSlot =
        !translated &&
        param is InterfaceType &&
        param.typeArguments.isNotEmpty &&
        param.typeArguments.every(_isTopType);
    if (param is InterfaceType &&
        param.nullability != Nullability.nullable &&
        !rawSlot) {
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
      // ..and the AOT dill's spelling of one, `_GrowableList._literal3<
      // dynamic>(3, 1, 2)`: its elements lowered again against the slot's
      // element type (`List<int>.unmodifiable([3, 1, 2])`, ws580).
      final core = value is StaticInvocation ? _coreListLiteral(value) : null;
      if (core != null &&
          (param.classNode.name == 'List' ||
              param.classNode.name == 'Iterable') &&
          args.length == 1 &&
          args[0] != core) {
        final elements = (value as StaticInvocation).arguments.positional;
        return IrListLiteral([
          for (final e in elements)
            _widened(
              e,
              args[0],
              _withExpectedReturn(args[0], e, () => expression(e)),
            ),
        ], _type(args[0]));
      }
    }
    // ..and a record literal into a record slot of other field types: its
    // fields lowered again against the slot's.
    if (value is RecordLiteral &&
        param is RecordType &&
        param.named.isEmpty &&
        param.positional.length == value.positional.length &&
        param.positional.toString() != value.recordType.positional.toString()) {
      return _recordLiteral(value, param.positional);
    }
    // A tear-off's named-parameter order first, whatever its recorded
    // type says: the type is spelled sorted, the value is declared in its
    // own order, and no type rule can tell them apart (run523).
    final ordered = _namedOrderAdapter(value, param, lowered);
    if (ordered != null) lowered = ordered;
    if (coerceByType &&
        translated &&
        param != null &&
        lowered.rustType == null) {
      _untypedCensus.update(
        '${lowered.runtimeType}/${value.runtimeType}',
        (n) => n + 1,
        ifAbsent: () => 1,
      );
    }
    if (coerceByType &&
        translated &&
        param != null &&
        lowered.rustType != null) {
      IrType? slot = slotIr;
      if (slot == null) {
        try {
          slot = _type(param);
        } on Unsupported {
          slot = null;
        }
      }
      // A prelude callee's generic slot is its own Rust signature's
      // `Option<T>`, never the projected `<T as DartNullable>::Or`: the
      // projection is how *this* declaration spells its own edges, and a
      // callee's `T?` reached with this declaration's `T` put in is not one
      // of them (`ArgumentError.checkNotNull(other, 'other')` inside a
      // generic function, 29 at ws755).
      if (prelude && slot != null && slot.projected) {
        slot = IrType(slot.name, nullable: true, arguments: slot.arguments);
      }
      // A translated callee's `T?` is spelled `<T as DartNullable>::Or`
      // (`_edgeType`); `_type` spells the plain `Option<T>` a *body* works
      // with. At an argument edge the callee's own spelling is the slot --
      // without it the coercion below made the `Some(..)` a body wants and
      // returned, so the projection rule in this method's tail never ran
      // (`AsyncSnapshot.withData` through its redirecting `this._(..)`, and
      // `_OverridableActionMixin._getOverrideAction`; 7 at ws786).
      if (!prelude &&
          slot != null &&
          !slot.projected &&
          _argumentEdge &&
          _projectedSlot(param)) {
        slot = IrType(
          slot.name,
          nullable: true,
          projected: true,
          arguments: slot.arguments,
        );
      }
      if (slot != null) {
        // A local handed on is shared, as below: the clone comes first so
        // the coercion wraps the clone, not the local.
        var shared = lowered;
        if (value is VariableGet && _clonedWhenPassed(value.variable.type)) {
          shared = IrCall(lowered, 'clone', const [])
            ..rustType = lowered.rustType;
        }
        final coerced = coerce(shared, slot);
        if (!identical(coerced, shared)) return coerced;
      }
    }
    // A local handed on is shared in Dart and moved in Rust: `string` passed
    // to `StringCharacterRange` and then read again, `listener` moved into a
    // closure "in a previous iteration of loop" -- 21 `E0382`s. A clone of a
    // `String` or an `Rc` is the sharing Dart meant. A list or map is not
    // cloned: a copy of one would be a different list, and the aliasing
    // Dart meant is not something a clone can give.
    if (value is VariableGet && _clonedWhenPassed(value.variable.type)) {
      // A clone is its operand's type.
      lowered = IrCall(lowered, 'clone', const [])..rustType = lowered.rustType;
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
    // ..and an *instance* tear-off the same way: `Timer(delay,
    // _controller.reverse)` tears off `reverse({double? from})` into a
    // `void Function()`, and `showOnScreen`'s four optional named
    // parameters land in a `VoidCallback` (8 at ws793). The target is a
    // Member either way, and the adapter calls the tear-off -- which
    // already holds its receiver -- with the defaults filled in.
    final tearOffTarget = switch (value) {
      ConstantExpression(:final constant) when constant is TearOffConstant =>
        constant.target,
      StaticTearOff(:final target) => target,
      InstanceTearOff(:final interfaceTarget) => interfaceTarget,
      _ => null,
    };
    if (tearOffTarget != null &&
        tearOffTarget.function != null &&
        param is FunctionType &&
        given is FunctionType &&
        param.namedParameters.isEmpty &&
        given.namedParameters.isNotEmpty &&
        param.positionalParameters.length ==
            given.positionalParameters.length) {
      final target = tearOffTarget;
      final params = <IrParam>[];
      final args = <IrExpr>[];
      for (var i = 0; i < param.positionalParameters.length; i++) {
        final name = '__a$i';
        params.add(IrParam(name, _paramType(param.positionalParameters[i])));
        args.add(IrLocal(name));
      }
      // In the *type's* order, which Kernel sorts and the lowered tear-off
      // takes its parameters in -- not the declaration's, which is the
      // order the defaults are written in (`show({int? which, String tag =
      // 'd', bool loud = false})` was called `(None, "d", false)` against
      // `|loud, tag, which|`).
      for (final n in given.namedParameters) {
        final declared = target.function!.namedParameters
            .where((p) => p.parameterName == n.name)
            .firstOrNull;
        final init = declared?.initializer;
        args.add(init == null ? _nullLiteral() : expression(init));
      }
      final adapter =
          IrCall(
              IrClosure(
                params,
                IrReturn(IrCallValue(lowered, args)),
                _type(param.returnType),
                // An instance tear-off holds its receiver: the adapter
                // around it has to hold it too, or it borrows the local
                // the receiver came from and cannot outlive the call
                // (E0597, the tearopt fixture).
                locals: _freeLocalsIn(value, {}),
              ),
              '!rc',
              const [],
            )
            ..rustType = _type(
              param.withDeclaredNullability(Nullability.nonNullable),
            );
      // Into the slot as any other value is: returning here skips the
      // wrapping this method ends with, and a `VoidCallback?` field took a
      // bare `Rc<{closure}>` (`SemanticsNode.showOnScreen`, 4 at ws795).
      try {
        return coerce(adapter, slotIr ?? _type(param));
      } on Unsupported {
        return adapter;
      }
    }
    // A static tear-off into a slot whose type *keeps* named parameters: a
    // function value is called through its type, whose named parameters
    // Kernel sorts, while the function itself is declared in its own
    // order. `partLLibreFranklin(fontSize: 16, fontWeight: ..)` through
    // the tear-off landed a `FontWeight` in the `locale` slot (34 at ws321).
    // An adapter taking the type's order and calling in the declaration's.
    // Sorting every definition instead was 8789 (ws323).
    // (An untyped tear-off reaches here with its order adapted above.)
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
    // The downcasts, the sharing into a trait handle, the dropped `as`
    // and the element upcasts that used to be spelled here one shape at a
    // time are `coerce`'s now (ws362): a typed value never reaches this
    // point needing one of them.
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
      lowered = _toF64(lowered);
    }
    // A `num` parameter has no rule of its own -- `int.+(num other)` is
    // declared that way, and `index + 1` became `index + (1 as f64)`
    // (ws54, 85 in dart:ui alone) -- except on a number, where the
    // receiver says which number `num` is (`_numReceiver`).
    if (scalar(param) == 'num' &&
        _numReceiver == 'double' &&
        scalar(given) == 'int' &&
        given!.nullability != Nullability.nullable) {
      lowered = _toF64(lowered);
    }
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
          IrCall(IrBound(), '!widen_object', const []),
        );
      }
      final widened = IrCall(lowered, '!widen_object', const []);
      return param.nullability == Nullability.nullable
          ? IrSome(widened)
          : widened;
    }
    // A typed list handed to a `List<int>` parameter widens its elements
    // (`Response.bytes(body)` with a `Uint8List`).
    // ..unless the slot it lands in is itself a narrow list -- a typed
    // list's own member, `bytes.setRange(a, b, other)` on `Uint8List`s
    // (`_narrowSlots`, run505).
    final slotNarrow =
        slotIr != null &&
        slotIr.name == 'List' &&
        slotIr.arguments.length == 1 &&
        const {
          'u8',
          'i8',
          'i16',
          'u16',
          'i32',
          'u32',
          'u64',
          'f32',
          'f64',
        }.contains(slotIr.arguments.single.name);
    if (!slotNarrow &&
        param is InterfaceType &&
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
        return _nullChecked(lowered);
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
    // Dart's static type says the value may be null; the *value in hand*
    // says whether it is an `Option` here. A nullable one the lowering
    // unwrapped -- a `!`, a downcast -- is no `Option`, and skipping the
    // wrap on the static type alone handed a bare `ShapeDecoration` to a
    // slot that takes one (`Decoration.lerp`, `BoxBorder.lerp`, 13 of the
    // 433 stubbed at ws745).
    final atHand = lowered.rustType;
    if (actual.nullability == Nullability.nullable &&
        (atHand == null ? !_unwrapped(lowered) : isNullable(atHand))) {
      return lowered;
    }
    if (actual is DynamicType || actual is NullType) return lowered;
    // A value already in the slot's `Option` is not put in it twice: a
    // collection's key or element goes through `_widened` a second time,
    // for the collection's own slot rather than the callee's declared
    // type, and the first pass did the wrapping (`m[k] = Box()` on a
    // `Map<int, Box?>` came out `Some(Some(..))`, ws694).
    final inHand = lowered.rustType;
    if (inHand != null && isNullable(inHand) && !inHand.projected) {
      IrType? spelled = slotIr;
      if (spelled == null) {
        try {
          spelled = _type(param);
        } on Unsupported {
          spelled = null;
        }
      }
      if (spelled != null &&
          isNullable(spelled) &&
          spelled.name == inHand.name) {
        return lowered;
      }
    }
    // A `T?` slot over a bare kept type parameter of the code here is the
    // projection `<T as DartNullable>::Or`, not an `Option<T>` -- as the
    // declarations spell it (`_edgeType`) -- and the value goes in by
    // `from_option`. A prelude callee's `FutureOr<T>?` is the same slot
    // (`Completer<T>.complete(value)` in `CachingAssetBundle.
    // loadStructuredBinaryData<T>`, run569).
    // Only at an argument edge: a body's own `T? x = ..` local is the
    // `Option<T>` a body works with (`DiagnosticsProperty.getChildren`,
    // `_retrieveNewRouteInformation`, +2 at ws570).
    final awaited = !translated && param is FutureOrType
        ? param.typeArgument.withDeclaredNullability(Nullability.nullable)
        : param;
    if (_argumentEdge &&
        awaited is TypeParameterType &&
        _projectedSlot(awaited)) {
      return coerce(
        lowered,
        IrType(awaited.parameter.name ?? 'T', nullable: true, projected: true),
      );
    }
    return IrSome(lowered);
  }

  static IrExpr _unboxed(IrExpr e) => e is IrClosure && e.boxed
      ? (IrClosure(
          e.params,
          e.body,
          e.returns,
          captures: e.captures,
          locals: e.locals,
          holdsSelf: e.holdsSelf,
        ))
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
      // A function local -- a handle, since every function-typed local is
      // one (`IrLocalFunction`, a closure initialiser boxed by `coerce`)
      // -- into a parameter the callee only calls (`impl Fn`): the closure
      // behind the handle, lent (`memoize`'s `ifAbsent` into
      // `putIfAbsent`, ws549).
      // ..only where the parameter is known not to keep it: with no
      // parameter to ask (a named argument's), the slot is the owned
      // handle every unknown slot is (`addWithPaintOffset(hitTest: ..)`,
      // 3 `&{closure}` where `Rc<dyn Fn>` went, ws553).
      if (!kept &&
          param != null &&
          callee != null &&
          value is IrLocal &&
          (value.rustType?.isFunction ?? false) &&
          _boxedFunctionLocals.contains(value.name)) {
        return IrCall(value, '!fn_ref', const [])..rustType = value.rustType;
      }
      // The parameter is owned where it is kept, so the argument is boxed to
      // match: a closure's own type has no name.
      if (value is IrClosure) {
        // Typed as the closure it is (`rustType` carried), so the slot's
        // coercion sees it: a `Future<bool> Function(MethodCall)` tear-off
        // kept by `setMethodCallHandler` gets its result mapped into the
        // `Future<dynamic>` the slot declares (run447). Untyped from ws419
        // (2609 -> 2779 then) until the result rules -- `void` into
        // `Object`, a future into a future -- were in `coerce`.
        return IrClosure(
          value.params,
          value.body,
          value.returns,
          captures: value.captures,
          locals: value.locals,
          // Carried. Rebuilding a node without a flag it had is the shape
          // that lost `kept` in round 104 and `shared` in round 101 --
          // and `isAsync` here, until run430 (`await` in a closure that
          // was not `async`).
          holdsSelf: value.holdsSelf,
          boxed: true,
          isAsync: value.isAsync,
        )..rustType = value.rustType;
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

  /// The IR name of a field: its Dart name, unless it is *private* and an
  /// ancestor in another library declares a private field of the same
  /// text -- Dart's privacy is per library, so those are two fields, and
  /// the flattened struct held one (`_InheritedNotifierElement._dirty`
  /// took `Element._dirty`'s place, started `false`, and the element
  /// never built: run554). The lower declaration is renamed with its
  /// library's tag; every reference resolves through the member, so the
  /// name is one everywhere.
  String _memberName(Member member) {
    final text = member.name.text;
    final start = member.enclosingClass;
    if (start == null) return text;
    // The declaring class: a copy in an anonymous application (the CFE's
    // `_X&Base&Mixin`, deduplicated or not) is the mixin's, and every copy
    // and the mixin's own trait spell the field alike.
    final home = start.isAnonymousMixin ? (_mixinOf(start) ?? start) : start;
    final key = (home, text);
    final known = _memberNames[key];
    if (known != null) return known;
    var out = text;
    final accessor = switch (member) {
      Field() => true,
      Procedure(:final isGetter, :final isSetter) => isGetter || isSetter,
      _ => false,
    };
    final static = switch (member) {
      Field(:final isStatic) => isStatic,
      Procedure(:final isStatic) => isStatic,
      _ => false,
    };
    if (member.name.isPrivate && accessor && !static) {
      final library = home.enclosingLibrary;
      // From the class itself, or -- for a mixin's member, whose own
      // superclass is `Object` -- from every application of the mixin.
      final starts = <Class>[
        start,
        if (home.isMixinDeclaration) ...?applications[home],
      ];
      if (Platform.environment['DART2RUST_TRACE_MEMBER'] == text) {
        stderr.writeln(
          'TRACE_MEMBER $text home=${home.name} (${library.importUri}) starts=${starts.map((c) => c.name).toList()}',
        );
      }
      outer:
      for (final from in starts) {
        var above = from.superclass;
        while (above != null) {
          if (above.enclosingLibrary != library &&
              _declaresPrivateAccessor(above, text)) {
            out = '${text}_${_libraryTag(library)}';
            break outer;
          }
          above = above.superclass;
        }
      }
    }
    _memberNames[key] = out;
    return out;
  }

  /// The mixin an anonymous application applies (its `mixedInType`, or
  /// the mixin among a deduplicated application's `implementedTypes`).
  static Class? _mixinOf(Class application) =>
      application.mixedInType?.classNode ??
      application.implementedTypes
          .map((st) => st.classNode)
          .where((c) => c.isMixinDeclaration)
          .firstOrNull;

  /// Whether `c` declares a non-static private field, getter or setter
  /// named `text` -- its own, or a mixin's copy the CFE put in it.
  static bool _declaresPrivateAccessor(Class c, String text) =>
      c.fields.any(
        (f) => f.name.text == text && f.name.isPrivate && !f.isStatic,
      ) ||
      c.procedures.any(
        (p) =>
            p.name.text == text &&
            p.name.isPrivate &&
            !p.isStatic &&
            (p.isGetter || p.isSetter),
      );

  final Map<(Class, String), String> _memberNames = {};

  /// A field's IR name from its target, or the written name for anything
  /// else (a setter's).
  String _fieldNameOf(Member? target, String text) =>
      target is Field ||
          (target is Procedure && (target.isGetter || target.isSetter))
      ? _memberName(target!)
      : text;

  /// A short tag for a library, from its URI's last segment.
  static String _libraryTag(Library library) {
    final segments = library.importUri.pathSegments;
    final last = segments.isEmpty ? 'lib' : segments.last;
    final base = last.endsWith('.dart')
        ? last.substring(0, last.length - 5)
        : last;
    return base.replaceAll(RegExp(r'[^A-Za-z0-9_]'), '_');
  }

  /// `IrMethod.typeParameterBounds`: each kept type parameter whose bound
  /// is a translated abstract class, with the bound spelled.
  Map<String, IrType> _traitBounds(FunctionNode function) {
    final out = <String, IrType>{};
    for (final p in function.typeParameters) {
      if (_erasedParameter(p)) continue;
      final bound = p.bound;
      if (bound is! InterfaceType ||
          bound.nullability == Nullability.nullable ||
          !_translatedClass(bound.classNode) ||
          !_abstractLike(bound.classNode) ||
          _scalarClass(bound.classNode)) {
        continue;
      }
      try {
        out[p.name ?? 'T'] = _type(bound);
      } on Unsupported {
        // Unspelled: no bound.
      }
    }
    return out;
  }

  /// Whether `callee` fills its `index`th positional parameter: a `List`
  /// or `Set` it adds to, removes from or writes into (`_mutatingListNames`),
  /// directly or by lending it to a callee that does. Only a member with
  /// one body -- a static, a top-level function, a private method -- is
  /// asked: an override family would have to agree on the signature.
  /// Cached; a cycle (`_findModels` lending `results` to itself) is a
  /// "no" while it is being asked.
  bool _fillsParameter(Procedure callee, int index) {
    if (callee.isGetter || callee.isSetter) return false;
    // An abstract target is the interface's view of a member some class
    // does declare: the dispatch target's answer, not "no". A mixin's
    // private method reached through the mixin's trait was passed a copy,
    // and the copy is where the fill went
    // (`SlottedContainerRenderObjectMixin._addDiagnostics`, 5 at ws764).
    if (callee.isAbstract) {
      final owner = callee.enclosingClass;
      if (owner == null) return false;
      final env = typeEnvironment;
      final concrete = env == null
          ? null
          : env.hierarchy.getDispatchTarget(owner, callee.name);
      if (concrete is Procedure &&
          !concrete.isAbstract &&
          !identical(concrete, callee)) {
        return _fillsParameter(concrete, index);
      }
      // A mixin *declaration* keeps only the hollow signature -- the
      // dispatch target inside it is the abstract member itself -- and the
      // body the CFE moved into an application of the mixin is the one
      // that fills (`_appliedBody`, as the trait's default takes it). The
      // parameter came out `&mut` from the body and the call handed it a
      // copy (`SlottedContainerRenderObjectMixin._addDiagnostics`, 5 at
      // ws786).
      final applied = _appliedBody(owner, callee);
      if (applied != null) return _fillsParameter(applied, index);
      return false;
    }
    final own = callee.isStatic || callee.enclosingClass == null;
    if (!own && !callee.name.isPrivate) return false;
    final params = callee.function.positionalParameters;
    if (index >= params.length) return false;
    final param = params[index];
    final type = param.type;
    if (type is! InterfaceType ||
        type.nullability == Nullability.nullable ||
        !const {'List', 'Set'}.contains(type.classNode.name) ||
        type.classNode.enclosingLibrary.importUri.toString() != 'dart:core') {
      return false;
    }
    final key = (callee, index);
    final known = _fillsCache[key];
    if (known != null) return known;
    _fillsCache[key] = false;
    final finder = _FillFinder(param, this);
    callee.function.body?.accept(finder);
    _fillsCache[key] = finder.found;
    return finder.found;
  }

  final Map<(Procedure, int), bool> _fillsCache = {};

  bool _keeps(FunctionNode callee, Object param) {
    final known = _keepsCache[param];
    if (known != null) return known;
    // A constructor keeps what its initializers store (`this.onDismiss`,
    // `super(onTap: onTap)`): the body alone said nothing of a parameter
    // that never reaches it, and `_ModalBarrierGestureDetector(onDismiss:
    // handleDismiss)` was handed a borrow of the closure (run639).
    final parent = callee.parent;
    if (parent is Constructor) {
      for (final initializer in parent.initializers) {
        final walk = _ParameterEscapes(param);
        initializer.accept(walk);
        if (walk.escapes) return _keepsCache[param] = true;
      }
    }
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

  static bool _sameOrder(List<String> a, List<String> b) {
    if (a.length != b.length) return false;
    for (var i = 0; i < a.length; i++) {
      if (a[i] != b[i]) return false;
    }
    return true;
  }

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
    // ..as the function's Rust type spells them: a `T?` there is
    // projected, and the coercion rule converts into it.
    final ir = _type(type);
    final slots = ir.parameters ?? const <IrType>[];
    final out = [
      for (var i = 0; i < node.positional.length; i++)
        _argument(
          node.positional[i],
          null,
          i,
          type,
          i < slots.length ? slots[i] : null,
        ),
    ];
    final supplied = {for (final n in node.named) n.name: n.value};
    final named = [...type.namedParameters]
      ..sort((a, b) => a.name.compareTo(b.name));
    for (final param in type.namedParameters) {
      final value = supplied.remove(param.name);
      if (value != null) {
        final at = type.positionalParameters.length + named.indexOf(param);
        out.add(
          _widened(
            value,
            param.type,
            expression(value),
            slotIr: at < slots.length ? slots[at] : null,
          ),
        );
      } else if (param.type.nullability == Nullability.nullable) {
        out.add(_nullLiteral());
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
    List<DartType>? positionalTypes,
    Map<String, DartType>? namedTypes,
    List<IrType?>? positionalSlots,
  ]) {
    final positional = [
      for (var i = 0; i < node.positional.length; i++)
        _argument(
          node.positional[i],
          callee,
          i,
          instantiated,
          positionalSlots != null && i < positionalSlots.length
              ? positionalSlots[i]
              : null,
          positionalTypes != null && i < positionalTypes.length
              ? positionalTypes[i]
              : null,
        ),
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
        out.add(_namedArgument(value, param, namedTypes?[param.parameterName]));
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
      // Through `_intoDynamic`, which knows a prelude callee's `Object`
      // parameter is spelled as what it takes: `StringBuffer([content =
      // ''])` got its default as an `Rc<dyn Object>` (21 at ws327).
      // ..under the callee's gate, as a written argument is: a prelude
      // `dynamic` slot's `null` default is its `Null` object.
      final callee = _calleeOf(param);
      return _forCallee(
        callee,
        param.type,
        expression(initializer),
        (lowered) => _widened(initializer, param.type, lowered),
      );
    }
    if (param.type.nullability == Nullability.nullable) {
      // ..into the slot's Rust type: an omitted `Object? aspect` is the
      // `Null` object, not `None` (85 at ws501).
      // ..of a translated callee: a prelude callee's slot is its own Rust
      // signature (`_slotPrelude`), an `Option` where Dart says `Object?`.
      final absent = _nullLiteral();
      final slot = _recordedType(param.type);
      return slot == null || !_translatedCallee(_calleeOf(param))
          ? absent
          : coerce(absent, slot);
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
      return IrLiteral('0', const IrType('int'));
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
  /// The erased type parameters of an abstract class that its own bodies
  /// read as type literals (`T` as a value): each gets a getter on the
  /// trait, answered by every class under it (see `_typeArgumentGetters`).
  final _typeLiteralParamsCache = <Class, Set<TypeParameter>>{};

  Set<TypeParameter> _typeLiteralParams(Class c) =>
      _typeLiteralParamsCache.putIfAbsent(c, () {
        if (c.typeParameters.isEmpty) return const {};
        final finder = _TypeLiteralFinder(c.typeParameters.toSet());
        for (final m in c.members) {
          m.accept(finder);
        }
        return {
          for (final p in finder.found)
            if (_erasedParameter(p)) p,
        };
      });

  String _typeArgGetter(Class owner, TypeParameter p) =>
      '_typeArg${owner.name}${p.name ?? 'T'}';

  /// The getters for the erased type parameters read as literals: declared
  /// on the abstract class that reads them, and answered by every class
  /// under it with the argument its ancestry puts in (`_WidgetsLocalizations
  /// Delegate extends LocalizationsDelegate<WidgetsLocalizations>` answers
  /// `WidgetsLocalizations`).
  void _typeArgumentGetters(Class node, IrClass cls) {
    final hierarchy = typeEnvironment?.hierarchy;
    if (hierarchy == null || cls.isEnum) return;
    if (node.isAbstract || _isOpen(node)) {
      for (final p in _typeLiteralParams(node)) {
        cls.abstractMethods.add(
          IrMethod(
            _typeArgGetter(node, p),
            const [],
            const IrType('Type'),
            const IrBlock([]),
            isGetter: true,
          ),
        );
      }
    }
    final self = InterfaceType(node, Nullability.nonNullable, [
      for (final p in node.typeParameters)
        TypeParameterType(p, Nullability.nonNullable),
    ]);
    final seen = <Class>{};
    final work = <Class>[
      if (node.superclass != null) node.superclass!,
      for (final t in node.implementedTypes) t.classNode,
      if (node.mixedInType != null) node.mixedInType!.classNode,
    ];
    while (work.isNotEmpty) {
      final above = work.removeLast();
      if (!seen.add(above)) continue;
      work.addAll([
        if (above.superclass != null) above.superclass!,
        for (final t in above.implementedTypes) t.classNode,
        if (above.mixedInType != null) above.mixedInType!.classNode,
      ]);
      if (above.isAnonymousMixin || !_translatedClass(above)) continue;
      if (!(above.isAbstract || _isOpen(above))) continue;
      final used = _typeLiteralParams(above);
      if (used.isEmpty) continue;
      final asAbove = hierarchy.getInterfaceTypeAsInstanceOfClass(self, above);
      if (asAbove == null) continue;
      for (final p in used) {
        final index = above.typeParameters.indexOf(p);
        if (index < 0 || index >= asAbove.typeArguments.length) continue;
        final argument = asAbove.typeArguments[index];
        // An erased parameter of this class itself: left to the classes
        // under it, which know.
        if (argument is TypeParameterType &&
            _erasedParameter(argument.parameter)) {
          continue;
        }
        final IrExpr answer;
        try {
          answer = _typeLiteral(argument);
        } on Unsupported {
          continue;
        }
        cls.methods.add(
          IrMethod(
            _typeArgGetter(above, p),
            const [],
            const IrType('Type'),
            IrBlock([IrReturn(answer)]),
            isGetter: true,
          ),
        );
      }
    }
  }

  IrExpr _typeLiteral(DartType type) {
    // A type parameter's: what it was instantiated with, asked of the
    // Rust type (`dart_type_of::<T>()`); spelled as text it was the
    // Kernel node (`_inheritedElements[T]` in
    // `dependOnInheritedWidgetOfExactType<T>` found nothing, run555).
    // An erased one is its bound.
    if (type is TypeParameterType) {
      // A method's own parameter that travels as a value (`_typeValues`):
      // the hidden parameter, wherever in the body (a closure captures
      // it as a local, `_freeLocalsIn`).
      final member = _member;
      if (member is Procedure) {
        final index = member.function.typeParameters.indexOf(type.parameter);
        if (index >= 0 && _typeValues(member).contains(index)) {
          return IrLocal('__ty_$index')..rustType = const IrType('Type');
        }
      }
      if (_erasedParameter(type.parameter)) {
        // An erased parameter of an abstract class is answered by the
        // object: every class under it says what it put in (`Type get
        // type => T` in `LocalizationsDelegate<T>`, whose delegates all
        // answered `Object` and shared one map slot, run584).
        final owner = type.parameter.declaration;
        if (owner is Class &&
            (owner.isAbstract || _isOpen(owner)) &&
            _typeLiteralParams(owner).contains(type.parameter) &&
            _member != null &&
            !(_member is Procedure && (_member as Procedure).isStatic)) {
          return IrCall(
            IrThis(),
            _typeArgGetter(owner, type.parameter),
            const [],
            fails: true,
          )..rustType = const IrType('Type');
        }
        return _typeLiteral(type.parameter.bound);
      }
      return IrStaticCall(
        null,
        'dart_type_of',
        const [],
        typeArguments: [IrType(type.parameter.name ?? 'T')],
      )..rustType = const IrType('Type');
    }
    final name = type is InterfaceType ? type.classNode.name : '$type';
    return IrLiteral('Type::of("$name")', const IrType('raw'))
      ..rustType = const IrType('Type');
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
      // A collection constant is its class with its own element types: a
      // `const [BoxShadow(..)]` into a `List<BoxShadow>?` parameter was
      // `dynamic` here and never `Some`d (ws395).
      ListConstant() when core != null => InterfaceType(
        core.listClass,
        Nullability.nonNullable,
        [c.typeArgument],
      ),
      SetConstant() when core != null => InterfaceType(
        core.setClass,
        Nullability.nonNullable,
        [c.typeArgument],
      ),
      MapConstant() when core != null => InterfaceType(
        core.mapClass,
        Nullability.nonNullable,
        [c.keyType, c.valueType],
      ),
      RecordConstant() => c.recordType,
      _ => const DynamicType(),
    };
  }

  /// A constant, typed by its own class (`_constantStaticType`) so that a
  /// slot it goes into -- a `const <ShortcutActivator, Intent>{..}` entry --
  /// is coerced like any value (`coerce`).
  IrExpr _constant(Constant constant, Expression node) {
    final lowered = _constantRaw(constant, node);
    if (lowered.rustType == null) {
      try {
        lowered.rustType = _type(_constantStaticType(constant));
      } on Unsupported {
        // Left untyped.
      }
    }
    return lowered;
  }

  IrExpr _constantRaw(Constant constant, Expression node) {
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
      if (value.isNaN) return IrLiteral('f64::NAN', const IrType('raw'));
      if (value == double.infinity) {
        return IrLiteral('f64::INFINITY', const IrType('raw'));
      }
      if (value == double.negativeInfinity) {
        return IrLiteral('f64::NEG_INFINITY', const IrType('raw'));
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
      return _nullLiteral();
    }
    // Each element into the collection's element type, as a map constant's
    // entries are below.
    IrExpr element(Constant c, DartType into) {
      final value = ConstantExpression(c, _constantStaticType(c));
      return _widened(value, into, _constant(c, node));
    }

    if (constant is ListConstant) {
      final elementType = _type(constant.typeArgument);
      return IrListLiteral([
        for (final e in constant.entries) element(e, constant.typeArgument),
      ], elementType)..rustType = IrType('List', arguments: [elementType]);
    }
    if (constant is SetConstant) {
      // A const set: the prelude's `Set::from(vec![..])`, which is what a
      // set literal expression becomes too. 37 in the gallery's dill.
      return IrStaticCall('Set', 'from', [
        IrListLiteral([
          for (final e in constant.entries) element(e, constant.typeArgument),
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
        return _widened(value, into, _constant(c, node));
      }

      // Typed, so a slot of other element types adapts it: an empty
      // `const {}` into a copy's erased field (ws490).
      final keyType = _type(constant.keyType);
      final valueType = _type(constant.valueType);
      return IrMapLiteral(
        [
          for (final e in constant.entries)
            (
              entry(e.key, constant.keyType),
              entry(e.value, constant.valueType),
            ),
        ],
        keyType,
        valueType,
      )..rustType = IrType('Map', arguments: [keyType, valueType]);
    }
    if (constant is StaticTearOffConstant) {
      // A top-level or static function used as a value. Rust names the
      // function; nothing is captured, so none of the ownership question that
      // an *instance* tear-off raises applies here.
      return IrFunctionRef(
        constant.target.enclosingClass?.name,
        constant.target.name.text,
      )..rustType = _functionRefType(constant.target);
    }
    if (constant is ConstructorTearOffConstant) {
      // A constructor or factory used as a value: the associated function
      // the class has for it, by the name the backend declares it under
      // (`AssetManifest.loadFromAssetBundle` hands `_AssetManifestBin.
      // fromStandardMessageCodecMessage` to the bundle, run569).
      final target = constant.target;
      final cls = target.enclosingClass!;
      final text = target.name.text;
      final name = text.isEmpty
          ? 'new'
          : text == '_'
          ? 'new_'
          : text;
      return IrFunctionRef(_instanceName(cls), name)
        ..rustType = _functionRefType(target);
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
      // Typed, so a slot of another type adapts it: `const
      // OptionalMethodChannel('flutter/menu')` into a `MethodChannel`
      // field wants the handle (`DefaultPlatformMenuDelegate`, run482).
      // Each field's value into the field's declared type by the one
      // rule: an omitted `Object?` field of a `const` instance holds the
      // `Null` object (`ThemeData`'s constants, ws511).
      IrExpr fieldValue(String name, Constant value) {
        final field = cls.fields.where((f) => f.name.text == name).firstOrNull;
        final lowered = _constant(value, node);
        if (field == null) return lowered;
        return _widened(
          ConstantExpression(value, _constantStaticType(value)),
          field.type,
          lowered,
        );
      }

      // With the constant's type arguments (the kept ones): a `const
      // Uninit<Sym>(..)` boxed into a `dynamic` slot had nothing else to
      // say what its `PhantomData` was (ws588).
      final instanceType = IrType(
        _instanceName(cls),
        // ..and its module, when the name is one two libraries declare:
        // three of them have a `_UnspecifiedTextScaler`, and the default
        // `const _UnspecifiedTextScaler()` of `TextPainter`'s `textScaler`
        // named none of them (`_SwitchPainter`, run700).
        module: _moduleQualifier(cls),
        arguments: _erasedArguments(cls, constant.typeArguments),
      );
      final instance = IrConstInstance(instanceType, {
        for (final entry in byName.entries)
          entry.key: fieldValue(entry.key, entry.value),
      })..rustType = instanceType;
      return _isOpen(cls)
          ? (IrUpcast(instance, IrType(cls.name))..rustType = IrType(cls.name))
          : instance;
    }
    if (constant is RecordConstant) {
      // A const record: the tuple a record literal is (`IrRecord`), each
      // field into the record type's own field type. The `switch` over
      // `axisDirection` in `ScrollPosition._updateSemanticActions` yields
      // `const (SemanticsAction.scrollDown, SemanticsAction.scrollUp)`
      // (run684). Named fields are refused as a literal's are.
      if (constant.named.isNotEmpty) {
        throw Unsupported('a const record with named fields', _sample(node));
      }
      final fields = constant.recordType.positional;
      return IrRecord([
        for (var i = 0; i < constant.positional.length; i++)
          element(constant.positional[i], fields[i]),
      ])..rustType = _type(constant.recordType);
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
      // ..the parameter's type at the constant's own instantiation: `const
      // WidgetStatePropertyAll<OutlinedBorder?>(StadiumBorder())` takes an
      // `OutlinedBorder?`, not the bare `T` (ws463).
      // ..the *kept* parameters only: an erased one's slot is its bound,
      // `Rc<dyn Object>`, whatever the constant instantiates it at, and
      // substituting `double` in put an `f64` where the constructor takes
      // an object (`const WidgetStatePropertyAll<double>(24.0)`, ws704).
      final declaredParam = paramType[name];
      final kept = [
        for (final p in cls.typeParameters)
          if (!_erasedParameter(p)) p,
      ];
      final keptArguments = [
        for (var i = 0; i < cls.typeParameters.length; i++)
          if (!_erasedParameter(cls.typeParameters[i])) typeArguments[i],
      ];
      final t = declaredParam == null
          ? null
          : cls.typeParameters.isNotEmpty &&
                cls.typeParameters.length == typeArguments.length &&
                kept.isNotEmpty
          ? Substitution.fromPairs(
              kept,
              keptArguments,
            ).substituteType(declaredParam)
          : declaredParam;
      // Into the parameter's type by the coercion rule, under the
      // constructor's gate as a written argument is: a prelude `dynamic`
      // slot's null is its `Null` object (`const FormatException(..)`).
      if (coerceByType && t != null && lowered.rustType != null) {
        if (_calleeTranslated(function, t)) {
          try {
            lowered = coerce(lowered, _type(t));
          } on Unsupported {
            // An unspelled slot: the value as it is.
          }
        }
        args.add(lowered);
        continue;
      }
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
    module: _moduleQualifier(cls),
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
  static bool _scalarClass(Class c) =>
      const {'String', 'int', 'double', 'bool', 'num'}.contains(c.name) &&
      c.enclosingLibrary.importUri.toString() == 'dart:core';

  /// The `dart:` libraries' top-level functions the prelude provides, by
  /// library and name. `scheduleMicrotask` runs its callback now (see the
  /// prelude); `print` is Dart's, to stdout, through the Object
  /// protocol's `toString` (google_fonts' error path, run561).
  /// Each with the Rust slots its arguments are coerced into, or none.
  /// `dart:collection`'s extension getters on iterables, by the CFE's
  /// name for the lowered static, to the prelude's list method.
  static const _coreExtensionMethods = <String, Map<String, String>>{
    'dart:collection': {
      'IterableExtensions|get#firstOrNull': 'first_or_null',
      'IterableExtensions|get#lastOrNull': 'last_or_null',
      'IterableExtensions|get#singleOrNull': 'single_or_null',
      'IterableExtensions|elementAtOrNull': 'element_at_or_null',
    },
  };

  static const _coreTopLevel = <String, Map<String, (String, List<IrType>?)>>{
    'dart:core': {
      'print': ('dart_print', [IrType('dynamic')]),
    },
    'dart:async': {'scheduleMicrotask': ('_schedule_microtask', null)},
    'dart:convert': {'jsonDecode': ('json_decode', null)},
  };

  bool _erasedParameter(TypeParameter p) {
    if (!erase) return false;
    // Erasure is a property of the declarations this compiler writes: a
    // prelude class's parameter (`HashSet<E>`) is the prelude's own
    // generic, and dropping it left `Set::new()` with nothing to infer
    // `T` from once boxed into an `Object` slot (`InheritedModelElement.
    // updateDependencies`, run560).
    final owner = p.declaration;
    if (owner is Class && !_translatedClass(owner)) return false;
    // ..when its bound has a handle to erase to: a top type (`Rc<dyn
    // Object>`) or a translated trait. `RestorableEnum<T extends Enum>`
    // erased to a `dart:core` class this compiler does not spell took 107
    // crates down (ws520).
    if (covariantParameters.contains(p) && _erasableBound(p.bound)) {
      return true;
    }
    // An anonymous mixin application's parameter stands for the mixin's:
    // erased when that one is (`SlottedRenderObjectElement<SlotType>` kept
    // a `SlotType` nothing declared, ws315).
    final decl0 = p.declaration;
    if (decl0 is Class && decl0.isAnonymousMixin) {
      final mixedIn = decl0.mixedInType;
      if (mixedIn != null) {
        for (var j = 0; j < mixedIn.typeArguments.length; j++) {
          final a = mixedIn.typeArguments[j];
          if (a is TypeParameterType &&
              a.parameter == p &&
              j < mixedIn.classNode.typeParameters.length) {
            return _erasedParameter(mixedIn.classNode.typeParameters[j]);
          }
        }
      }
      // A deduplicated application (`dart:mixin_deduplication`) has no
      // `mixedInType`; the mixin is among its `implementedTypes`.
      for (final st in [
        if (decl0.supertype != null) decl0.supertype!,
        ...decl0.implementedTypes,
      ]) {
        for (var j = 0; j < st.typeArguments.length; j++) {
          final a = st.typeArguments[j];
          if (a is TypeParameterType &&
              a.parameter == p &&
              j < st.classNode.typeParameters.length) {
            return _erasedParameter(st.classNode.typeParameters[j]);
          }
        }
      }
      // No mapping found: judged by its own bound below, as before
      // (`ChildType extends RenderBox` on a mixin application, 54 at ws316).
    }
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
    // A closure's or local function's own type parameter: a Rust closure
    // cannot be generic, so it reads as its bound, as a generic function
    // *type* is instantiated at its bounds (`_type`) -- `<T extends
    // Object?>(settings, builder) => MaterialPageRoute<T>(..)` handed to
    // `WidgetsApp.pageRouteBuilder` named a `T` nothing declared (ws485).
    // ..declared by the closure itself in this Kernel (`FunctionExpression`,
    // `FunctionDeclaration`), or by its function node in another
    // (`pageRouteBuilder: <T>(..) => MaterialPageRoute<T>(..)` spelled a
    // `T` in `_MaterialAppState._buildWidgetApp`, ws503).
    final generic = p.declaration as TreeNode?;
    if (generic is FunctionExpression || generic is FunctionDeclaration) {
      return true;
    }
    if (generic is FunctionNode && generic.parent is! Member) return true;
    if (decl is! Class) return false;
    return _erasedCache.putIfAbsent(p, () {
      final bound = p.bound;
      if (bound is! InterfaceType) return false;
      if (bound.classNode.name != 'Object' &&
          bound.classNode.enclosingLibrary.importUri.scheme != 'dart' &&
          _abstractLike(bound.classNode)) {
        return true;
      }
      // An `Object`-bounded parameter of a trait-like class that some
      // subclass fixes to a concrete type: `AssetImage` is an
      // `ImageProvider<AssetBundleImageKey>` and stands where an
      // `ImageProvider<Object>` is wanted (121 bounds at ws313). A
      // parameter every subclass passes through (`Animation<T>`) stays.
      // Only a translated class: the prelude's `Sink<T>` keeps its
      // parameter (11 "missing generics" that killed a leaf crate, ws314).
      final uri = decl.enclosingLibrary.importUri.toString();
      // Gated (`DART2RUST_ERASE_OBJECT=1`): measured 4435 against 3995
      // at ws318 with every crate reached -- `Animation<double>`'s values
      // went behind `Rc<dyn Object>` and every read had to come back.
      return eraseObjectBounded &&
          bound.classNode.name == 'Object' &&
          (uri.startsWith('package:') || uri == 'dart:ui') &&
          _abstractLike(decl) &&
          _fixedBelow(decl, decl.typeParameters.indexOf(p));
    });
  }

  final Map<TypeParameter, bool> _erasedCache = {};

  /// Whether an erased parameter of this bound is spelled: a top type, or
  /// a translated abstract-like class (a trait object).
  bool _erasableBound(DartType bound) {
    if (bound is DynamicType) return true;
    if (bound is! InterfaceType) return false;
    final cls = bound.classNode;
    if (cls.name == 'Object' &&
        cls.enclosingLibrary.importUri.toString() == 'dart:core') {
      return true;
    }
    return _translatedClass(cls) && _abstractLike(cls);
  }

  /// Whether a subtype of `cls` supplies a concrete type (not one of its
  /// own parameters) for `cls`'s `i`th parameter.
  bool _fixedBelow(Class cls, int i) {
    final subtypes = _subtypes;
    final hierarchy = typeEnvironment?.hierarchy;
    if (subtypes == null || hierarchy == null || i < 0) return false;
    for (final sub in subtypes.getSubtypesOf(cls)) {
      if (sub == cls) continue;
      final asBase = hierarchy.getClassAsInstanceOf(sub, cls);
      if (asBase == null || i >= asBase.typeArguments.length) continue;
      final arg = asBase.typeArguments[i];
      if (arg is! TypeParameterType && arg is! DynamicType) {
        if (arg is InterfaceType && arg.classNode.name == 'Object') continue;
        return true;
      }
    }
    return false;
  }

  /// `type` as an instance of `base` the way Rust holds it: up the
  /// supertype clauses, each spelled with the declaring class's erased
  /// parameters at their bounds (`_atErasedBounds`) before `type`'s
  /// arguments go in. Null when `base` is not above `type`.
  InterfaceType? _asRustInstance(
    InterfaceType type,
    Class base, [
    int depth = 0,
  ]) {
    if (identical(type.classNode, base)) return type;
    if (depth > 40) return null;
    final cls = type.classNode;
    final substitution = Substitution.fromInterfaceType(type);
    for (final st in [
      if (cls.supertype != null) cls.supertype!,
      if (cls.mixedInType != null) cls.mixedInType!,
      ...cls.implementedTypes,
    ]) {
      final direct = _atErasedBounds(
        InterfaceType(st.classNode, Nullability.nonNullable, st.typeArguments),
        0,
      );
      final substituted = substitution.substituteType(direct);
      if (substituted is! InterfaceType) continue;
      final found = _asRustInstance(substituted, base, depth + 1);
      if (found != null) return found;
    }
    return null;
  }

  /// A type with each erased parameter (`_erasedParameter`) in it replaced
  /// by its bound, a few levels deep: the instantiation Rust holds for it.
  DartType _atErasedBounds(DartType t, int depth) {
    if (depth > 4) return t;
    if (t is TypeParameterType && _erasedParameter(t.parameter)) {
      final bound = t.parameter.bound;
      return _atErasedBounds(
        t.nullability == Nullability.nullable
            ? bound.withDeclaredNullability(Nullability.nullable)
            : bound,
        depth + 1,
      );
    }
    if (t is InterfaceType && t.typeArguments.isNotEmpty) {
      return InterfaceType(t.classNode, t.nullability, [
        for (final a in t.typeArguments) _atErasedBounds(a, depth + 1),
      ]);
    }
    return t;
  }

  /// A class's type arguments with the erased ones left out.
  ///
  /// As type arguments (`_nested`): a `T?` among them is projected, so
  /// `_SettingsListItemState<T?>()` with `T` bound to `double?` is the
  /// state over `double?` -- `Option<T>` there was `Option<Option<f64>>`,
  /// and the state's downcast of its widget found none (run687). The
  /// callers that lowered a class type did this themselves; the
  /// constructor, constant and cast sites did not.
  List<IrType> _erasedArguments(Class cls, List<DartType> arguments) => _nested(
    () => [
      for (var i = 0; i < arguments.length; i++)
        if (i >= cls.typeParameters.length ||
            !_erasedParameter(cls.typeParameters[i]))
          _type(arguments[i]),
    ],
  );

  /// `x is T`. Against a type parameter that is the operand's own type
  /// (`value is! T` on a `T?` in `Provider.of`) it asks only about null,
  /// which is all Rust's `T` can differ in; `null is T` is false for the
  /// non-nullable arguments the gallery passes (`of<EmailStore>`, and
  /// nothing `of<X?>`). Any other type parameter is asked by id in the
  /// backend (`dart_cast_any`).
  /// Whether a value of this type might be a future at run time: a
  /// `Future`, a `FutureOr`, a top type, a type parameter, or a class that
  /// implements `Future` (`SynchronousFuture`). Anything else, awaited, is
  /// a turn and the value.
  bool _couldBeFuture(DartType t) {
    if (t is FutureOrType || t is DynamicType || t is TypeParameterType) {
      return true;
    }
    if (t is! InterfaceType) return t is! NullType;
    final cls = t.classNode;
    if (cls.name == 'Object' &&
        cls.enclosingLibrary.importUri.toString() == 'dart:core') {
      return true;
    }
    final env = typeEnvironment;
    if (env == null) return true;
    return env.hierarchy.getTypeAsInstanceOf(t, env.coreTypes.futureClass) !=
        null;
  }

  IrExpr _isExpression(IsExpression node) {
    final asked = node.type;
    // A literal's runtime type is its static type: `<int?>[] is List<int>`
    // (provider's sound-mode probe) is Dart's subtyping, decided here.
    final operand = node.operand;
    // ..or the CFE's spelling of one, `_GrowableList<int?>(0)`: a
    // `dart:core` factory whose class is an implementation's.
    final coreFactory =
        operand is StaticInvocation &&
        operand.target.enclosingLibrary.importUri.toString() == 'dart:core' &&
        (operand.target.enclosingClass?.name.startsWith('_') ?? false);
    final literal =
        operand is ListLiteral ||
        operand is MapLiteral ||
        operand is SetLiteral ||
        coreFactory ||
        (operand is ConstantExpression &&
            (operand.constant is ListConstant ||
                operand.constant is MapConstant ||
                operand.constant is SetConstant));
    final env = typeEnvironment;
    if (literal && env != null && asked is InterfaceType) {
      final core = env.coreTypes;
      final DartType? on = switch (operand) {
        ListLiteral(:final typeArgument) => InterfaceType(
          core.listClass,
          Nullability.nonNullable,
          [typeArgument],
        ),
        SetLiteral(:final typeArgument) => InterfaceType(
          core.setClass,
          Nullability.nonNullable,
          [typeArgument],
        ),
        MapLiteral(:final keyType, :final valueType) => InterfaceType(
          core.mapClass,
          Nullability.nonNullable,
          [keyType, valueType],
        ),
        ConstantExpression(:final type) => type,
        StaticInvocation() => _staticType(operand),
        _ => null,
      };
      if (on is InterfaceType) {
        return IrLiteral(
          env.isSubtypeOf(on, asked) ? 'true' : 'false',
          const IrType('bool'),
        );
      }
    }
    if (asked is TypeParameterType && !_erasedParameter(asked.parameter)) {
      final on = _staticType(node.operand);
      if (on is TypeParameterType && on.parameter == asked.parameter) {
        return IrUnary('!', IrIsNull(expression(node.operand)));
      }
      if (node.operand is NullLiteral) {
        return IrLiteral('false', const IrType('bool'));
      }
    }
    // Against a method's own parameter that travels as a value
    // (`_typeValues`): the object asked by that value (`dart_is_type`).
    final member = _member;
    if (asked is TypeParameterType && member is Procedure) {
      final index = member.function.typeParameters.indexOf(asked.parameter);
      if (index >= 0 && _typeValues(member).contains(index)) {
        // A local is shared: boxed from a clone, the local stays.
        var operand = expression(node.operand);
        if (node.operand is VariableGet) {
          operand = IrCall(operand, 'clone', const [])
            ..rustType = operand.rustType;
        }
        return IrStaticCall(null, 'dart_is_type', [
          coerce(operand, const IrType('dynamic')),
          IrLocal('__ty_$index')..rustType = const IrType('Type'),
        ])..rustType = const IrType('bool');
      }
    }
    // A test Dart's own subtyping already answers: the operand's static
    // type is a subtype of the asked one, so every value it can hold is
    // one. The CFE writes these itself -- a record destructuring pattern
    // (`final (nextChild, topLeftChild) = flipMainAxis ? .. : ..;` in
    // `RenderFlex.performLayout`) becomes a test of each field against
    // the type the field already has, and `is` against a function type or
    // `Record` is nothing `Any` can be asked (run712). Answered here, as
    // a literal's is above.
    if (env != null) {
      // ..through an extension type's erasure: it *is* its representation
      // at run time, and `_AscentDescent` is a `(double, double)?`
      // (`RenderFlex._computeSizes`, run713).
      final declared = _staticType(node.operand);
      final on = declared is ExtensionType
          ? declared.extensionTypeErasure
          : declared;
      // The asked type erases too: an extension type has no run-time
      // identity, so `x is _AscentDescent` really asks the representation
      // (`_AscentDescent operator +`, run717).
      final wanted = asked is ExtensionType
          ? asked.extensionTypeErasure
          : asked;
      if (on != null &&
          on is! DynamicType &&
          on is! NeverType &&
          !_mentionsTypeParameter(on) &&
          !_mentionsTypeParameter(wanted)) {
        try {
          if (env.isSubtypeOf(on, wanted)) {
            return IrBlockValue(
              [IrExprStmt(expression(node.operand))],
              IrLiteral('true', const IrType('bool')),
            )..rustType = const IrType('bool');
          }
          // ..and where only the `?` stands between them, the test is the
          // null check alone: the CFE's record pattern asks this after its
          // own `== null` arm. Only when the value in hand is an `Option`:
          // a narrowed one is not, and `is_none` is no method of an
          // `Rc<Border>` (`CupertinoTextField.build`, ws715).
          // ..only where the asked type has no runtime test of its own: a
          // record or a function type is nothing `Any` can be asked, and
          // this is the whole of what Dart means there. A class is left to
          // the ordinary test below, whose narrowing this cannot see
          // (`is_none` on an `Rc<Border>`, `CupertinoTextField.build`,
          // ws716).
          if ((wanted is RecordType || wanted is FunctionType) &&
              on.nullability == Nullability.nullable &&
              wanted.nullability != Nullability.nullable &&
              env.isSubtypeOf(
                on.withDeclaredNullability(Nullability.nonNullable),
                wanted,
              )) {
            return IrUnary('!', IrIsNull(expression(node.operand)))
              ..rustType = const IrType('bool');
          }
        } catch (_) {
          // Not a relation this environment can decide: asked below.
        }
      }
    }
    // `x is T?` admits null as well -- it is `x == null || x is T`, which
    // is exactly what the CFE writes for `if (parent is _NestedHookElement?)`
    // (nested's `SingleChildWidgetElementMixin.mount`). Asked as a plain
    // `is T`, the test null-checked the operand and unwrapped the very
    // absence it was admitting (4 at ws793). Only on a local: the operand
    // is read twice, and `||` reads the second only when the first said no.
    if (asked.nullability == Nullability.nullable &&
        asked is InterfaceType &&
        node.operand is VariableGet) {
      final on = _staticType(node.operand);
      if (on != null && on.nullability == Nullability.nullable) {
        return IrBinary(
          '||',
          IrIsNull(expression(node.operand))..rustType = const IrType('bool'),
          IrIs(
            expression(node.operand),
            _type(asked.withDeclaredNullability(Nullability.nonNullable)),
          )..rustType = const IrType('bool'),
        )..rustType = const IrType('bool');
      }
    }
    return IrIs(expression(node.operand), _type(asked));
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
    if (Platform.environment['DART2RUST_TRACE_CALL'] == target.name.text) {
      stderr.writeln(
        'TRACE_CALL generic-on-trait ${declaring.name}.${target.name.text} '
        'from=${from.name} bodies=${bodies.map((c) => c.name).toList()} '
        'subtypes=${_subtypes != null}',
      );
    }
    if (bodies.length != 1) return null;
    final body = bodies.single;
    if (!_abstractLike(body)) return null;
    final hierarchy = typeEnvironment?.hierarchy;
    final below =
        from == body || (hierarchy?.isSubInterfaceOf(from, body) ?? false);
    return IrSuperDispatch(
      receiver is ThisExpression ? IrThis() : expression(receiver),
      body.name,
      target.name.text,
      args,
      [for (final t in node.arguments.types) _type(t)],
      body.typeParameters.where((p) => !_erasedParameter(p)).length,
      castTo: below ? null : body.name,
    );
  }

  /// The indices of a generic instance method's type parameters that some
  /// body in its *family* -- the topmost declaration and every override
  /// under it -- uses as a type literal (`_inheritedElements[T]` in
  /// `Element.getElementForInheritedWidgetOfExactType<T>`). Those travel
  /// as `Type` values in hidden trailing parameters `__ty_<i>`: a `dyn`
  /// receiver reaches the method through its erased twin, whose `T` is
  /// `Rc<dyn Object>` and whose `dart_type_of::<T>()` was therefore the
  /// wrong type (`MediaQuery._of` through provider's override, run623).
  /// Dart's own runtime passes type arguments this way; here only the
  /// observed ones are.
  final _typeValueIndices = <Procedure, List<int>>{};

  List<int> _typeValues(Procedure p) {
    if (p.isStatic ||
        p.kind != ProcedureKind.Method ||
        p.function.typeParameters.isEmpty ||
        p.enclosingClass == null ||
        !_translatedClass(p.enclosingClass!)) {
      return const [];
    }
    final root = _familyRoot(p);
    return _typeValueIndices.putIfAbsent(root, () {
      final arity = root.function.typeParameters.length;
      final found = <int>{};
      final classes = <Class>{root.enclosingClass!, ..._genericBodies(root)};
      for (final c in classes) {
        for (final q in c.procedures) {
          if (q.name.text != root.name.text ||
              q.isStatic ||
              q.kind != ProcedureKind.Method) {
            continue;
          }
          final params = q.function.typeParameters;
          if (params.length != arity) continue;
          final finder = _TypeLiteralFinder(params.toSet());
          q.function.body?.accept(finder);
          for (final t in finder.found) {
            found.add(params.indexOf(t));
          }
        }
      }
      return found.toList()..sort();
    });
  }

  /// The topmost declaration of an instance method's name above `p`'s
  /// class (through every supertype, declared or inherited), or `p`.
  Procedure _familyRoot(Procedure p) {
    final name = p.name.text;
    Procedure? best = p;
    final seen = <Class>{};
    final queue = <Class>[p.enclosingClass!];
    while (queue.isNotEmpty) {
      final c = queue.removeAt(0);
      if (!seen.add(c)) continue;
      for (final q in c.procedures) {
        if (q.name.text == name &&
            !q.isStatic &&
            q.kind == ProcedureKind.Method &&
            q.function.typeParameters.length ==
                p.function.typeParameters.length) {
          best = q;
        }
      }
      queue.addAll([
        if (c.superclass != null) c.superclass!,
        if (c.mixedInClass != null) c.mixedInClass!,
        for (final t in c.implementedTypes) t.classNode,
      ]);
    }
    return best!;
  }

  /// The hidden `Type` parameters `p` takes (see `_typeValues`).
  List<IrParam> _typeValueParams(Procedure p) => [
    for (final i in _typeValues(p)) IrParam('__ty_$i', const IrType('Type')),
  ];

  /// The classes below (and including) the target's that carry a body for
  /// its name, whole program.
  List<Class> _genericBodies(Procedure target) {
    final subtypes = _subtypes;
    if (subtypes == null) return const [];
    final declaring = target.enclosingClass!;
    final out = <Class>[];
    for (final c in [declaring, ...subtypes.getSubtypesOf(declaring)]) {
      if (c.isAnonymousMixin || out.contains(c)) continue;
      // The translated classes -- by the prefixes this run was given, not
      // `package:` by name: a fixture's `file:` classes had no bodies
      // here and every generic trait call went to the erased twin.
      if (!_translatedClass(c)) continue;
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

  /// What a `throw` hands to `Err`: a *value* of a translated class
  /// (`throw error` with a `FlutterError` in hand, `FlutterError(..)`
  /// whose constructor is a factory and so a static call) goes behind an
  /// `Rc<dyn Object>` here; a constructed one the backend boxes itself
  /// (`_boxedThrow`), a handle or a trait object unsizes on its own.
  IrExpr _thrownValue(Expression thrown) {
    final lowered = expression(thrown);
    final type = _staticType(thrown);
    if (type is InterfaceType &&
        thrown is! ConstructorInvocation &&
        thrown is! ConstantExpression &&
        thrown is! StringLiteral &&
        type.nullability != Nullability.nullable &&
        _translatedClass(type.classNode) &&
        !_abstractLike(type.classNode) &&
        !_closureCallsMethod(type.classNode) &&
        !type.classNode.isEnum &&
        !_scalarClass(type.classNode) &&
        lowered is! IrNew &&
        lowered is! IrUpcast) {
      return IrCall(lowered, '!rc_object', const []);
    }
    return lowered;
  }

  /// A collection's element argument (`set.remove(ticker)`, `contains`),
  /// shared into the element type when that is a trait and the argument a
  /// concrete class of it: the prelude takes `&T`, and a `&Rc<_WidgetTicker>`
  /// does not coerce to `&Rc<dyn Ticker>` through the reference (45 at
  /// ws311). A counted class's handle is upcast (`IrUpcast.handle`), a
  /// value put behind a fresh one. Only a named value: `this` may be a
  /// struct behind `&self` (ws312).
  /// An element handed to a list's `remove`/`indexOf`/`contains`: into
  /// the list's element type by the one rule, as an `add` is -- a
  /// `Disposer` into a `List<Disposer?>.remove` wants its `Some` (get's
  /// `ListNotifier.removeListener`, ws493), a subclass its handle.
  IrExpr _intoElement(IrExpr lowered, Expression value, DartType? collection) {
    if (collection is! InterfaceType || collection.typeArguments.isEmpty) {
      return lowered;
    }
    final element = collection.typeArguments.first;
    return _intoArgument(value, element, lowered);
  }

  /// A value into a collection's own slot -- an element, a key: the slot
  /// is a *type argument*, so a `T?` there is spelled projected (`<T as
  /// DartNullable>::Or`) and not the body's `Option<T>`. A read of the
  /// same slot arrives projected too, so nothing converts in between
  /// (`widget.optionsMap[widget.selectedOption]` put the key through an
  /// `option` the map's `get` would not take, run693).
  /// `m[k] = v`: the key and the value into the *map's own* slots, which
  /// are type arguments. The callee here is `Map.[]=`, a prelude member
  /// whose declared `K`, `V` coerce nothing, so a `Map<T?, ..>`'s key
  /// arrived as the body's `Option<T>` where the map holds the projected
  /// `<T as DartNullable>::Or` (run693).
  List<IrExpr> _mapEntry(InstanceInvocation node, List<IrExpr> args) {
    final mapType = _staticType(node.receiver);
    if (args.length != 2 ||
        node.arguments.positional.length != 2 ||
        mapType is! InterfaceType ||
        mapType.typeArguments.length != 2) {
      return args;
    }
    return [
      for (var i = 0; i < 2; i++)
        _intoArgument(
          node.arguments.positional[i],
          mapType.typeArguments[i],
          args[i],
        ),
    ];
  }

  IrExpr _intoArgument(Expression value, DartType slot, IrExpr lowered) {
    IrType? spelled;
    try {
      spelled = _typeNested(slot);
    } on Unsupported {
      spelled = null;
    }
    return _widened(value, slot, lowered, slotIr: spelled);
  }

  /// Whether a type names an erased parameter anywhere in it.
  bool _mentionsErased(DartType t) => switch (t) {
    TypeParameterType() => _erasedParameter(t.parameter),
    FutureOrType() => _mentionsErased(t.typeArgument),
    RecordType() =>
      t.positional.any(_mentionsErased) ||
          t.named.any((n) => _mentionsErased(n.type)),
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
    if (node is YieldStatement) {
      final element = _syncStarElement;
      if (element == null) {
        throw Unsupported('yield outside a sync* body', _sample(node));
      }
      final listed = IrType('List', arguments: [_type(element)]);
      final out = IrLocal(_syncStarOut)..rustType = listed;
      if (node.isYieldStar) {
        return IrExprStmt(
          IrCall(out, 'extend', [coerce(expression(node.expression), listed)]),
        );
      }
      return IrExprStmt(
        IrCall(out, 'push', [
          _widened(node.expression, element, expression(node.expression)),
        ]),
      );
    }
    if (node is ReturnStatement) {
      final value = node.expression;
      // A bare `return` in a `sync*` body hands the collected list back.
      if (value == null && _syncStarElement != null) {
        return IrReturn(
          IrLocal(_syncStarOut)
            ..rustType = IrType('List', arguments: [_type(_syncStarElement!)]),
        );
      }
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
      // `return completer.future;` in an `async` body: Dart awaits the
      // future it returns, and an `async fn` returning `T` has to as well.
      // Before the `void` rule: `return c ? flush() : _readFile();` in an
      // `async` `Future<void>` ran the futures detached and returned
      // (`GetStorage.init`, an uncaught `FormatException` from the read
      // that then raced the write, run529).
      final valueType = value == null ? null : _staticType(value);
      final returnsFuture =
          _asyncBody &&
          valueType is InterfaceType &&
          valueType.classNode.name == 'Future';
      if (returnsFuture && _voidReturn) {
        return IrBlock([
          IrExprStmt(IrAwait(expression(value!))),
          const IrReturn(null),
        ]);
      }
      // Any other `return e;` in a `void` body -- `(x) => day = x` handed
      // to a `void Function(int)` -- runs `e` and returns nothing.
      // ..as the *statement* it would have been: `=> _map[v] = ..` on a
      // field of `this` is the in-place `insert`, where the value form
      // wrote into a clone of the map (the listgen fixture).
      if (_voidReturn && value != null) {
        return IrBlock([
          statement(ExpressionStatement(value)),
          const IrReturn(null),
        ]);
      }
      if (value == null) return const IrReturn(null);
      if (returnsFuture) {
        return IrReturn(IrAwait(expression(value)));
      }
      return IrReturn(
        _acrossEdge(
          _widened(value, _returnsType, expression(value)),
          _edgeReturn,
          toOption: false,
        ),
      );
    }
    if (node is Block) {
      return IrBlock([for (final s in node.statements) statement(s)]);
    }
    if (node is IfStatement) {
      return IrIf(
        _condition(node.condition),
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
      return IrThrow(IrLocal(caught)..rustType = _caughtType);
    }
    if (node is ExpressionStatement && node.expression is Throw) {
      // Before the general `ExpressionStatement` case below, not after: the
      // general one lowers the expression, and a `throw` has no value to lower.
      // Placed after, this check never ran and every throwing method was
      // refused -- which the fixture comparison found at once, because the
      // analyzer front end had it right.
      final thrown = node.expression as Throw;
      if (_tfaUnreachable(thrown)) return IrExprStmt(_unreachable);
      return IrThrow(_thrownValue(thrown.expression));
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
            _mapEntry(
              value,
              _arguments(
                value.arguments,
                value.interfaceTarget.function,
                true,
                value.functionType,
              ),
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
        final list = expression(value.receiver);
        final lowered = _widened(stored, element, expression(stored));
        // ..and crosses into the element as the *list* spells it: inside a
        // generic class a `List<E?>` field holds `<E as DartNullable>::Or`,
        // and a body's `Option<E>` is not that (`_queue[index] = element`
        // in `HeapPriorityQueue._bubbleDown`, 3 at ws751).
        final slot = list.rustType?.arguments.length == 1
            ? list.rustType!.arguments.single
            : null;
        return IrIndexSet(
          list,
          expression(value.arguments.positional[0]),
          slot == null ? lowered : coerce(lowered, slot),
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
          _intoDeclaredNum(
            value.value,
            _localType(value.variable),
            _widened(
              value.value,
              _localType(value.variable),
              expression(value.value),
              slotIr: _localIrType(value.variable),
            ),
          ),
        );
      }
      // A conditional whose value is discarded is an `if`: its arms need
      // no common type then. The CFE spells `tween.end ??= tween.begin`
      // as `let #t = tween in #t.end == null ? #t.end = .. : null`, whose
      // arms are the store's `Option<Rc<dyn Object>>` and the `Null`
      // object (`_constructTweens`, run612).
      final asIf = _conditionalStatement(value);
      if (asIf != null) return asIf;
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
        // nothing -- but only the one at the *end* of a case (through the
        // blocks a nested switch's case leaves it in). One in the middle
        // leaves the switch early, which a match arm cannot do on its own:
        // then the match sits in a labelled block and the break names it
        // (the generated `lookupGalleryLocalizations`, run581).
        _switchBreaks.add(node);
        final early = _SwitchBreakFinder.earlyBreaks(node, body);
        if (early.isEmpty) return statement(body);
        _labeledSwitches[node] = _labelFor(node);
        return IrLabeled(_labelFor(node), statement(body));
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
        if (_droppableBreaks.contains(node)) return const IrBlock([]);
        final label = _labeledSwitches[target];
        if (label == null) {
          throw Unsupported(
            'break out of a switch from inside a case',
            _sample(node),
          );
        }
        return IrBreak(label);
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
      final closure = _closure(node.function, node) as IrClosure;
      // Recursive when the body names its own binding: a call
      // (`LocalFunctionInvocation`) or a read of it.
      final self = _SelfReference(node.variable);
      node.function.body?.accept(self);
      if (!self.found) _boxedFunctionLocals.add(name);
      return IrLocalFunction(name, closure, recursive: self.found);
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
      return IrWhile(_condition(node.condition), _loopBody(node.body, false));
    }
    if (node is DoStatement) {
      // `do { .. } while (c)`: a `loop` whose body runs first and tests
      // last. The body is labelled as a `for` with updates is, so that a
      // `continue` inside it leaves the body block and still reaches the
      // test -- a bare `continue` would have skipped it. `package:characters`
      // is written with these, and its whole `StringCharacters` was refused.
      return IrWhile(
        IrLiteral('true', const IrType('bool')),
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
              ? IrLiteral('true', const IrType('bool'))
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
    final outerCaughtType = _caughtType;
    _caught = error;
    // The type the clause narrowed to (`on FlutterError catch (e)`): a
    // `rethrow` throws that value, and the error type it goes back into is
    // `Rc<dyn Object>` -- untyped, the widening rule in `_boxedThrow` had
    // nothing to look at, and `return Err(error)` handed a `FlutterError`
    // where the handle goes (`AssetBundleImageProvider._loadAsync`, the
    // whole image path, run745).
    _caughtType = guard is InterfaceType && guard.classNode.name != 'Object'
        ? (() {
            try {
              return _type(guard);
            } on Unsupported {
              return null;
            }
          })()
        : null;
    final IrStmt handler;
    try {
      handler = statement(clause.body);
    } finally {
      _caught = outerCaught;
      _caughtType = outerCaughtType;
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

  /// The type the catch clause narrowed the caught value to, for `rethrow`.
  IrType? _caughtType;

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
      if (member is Member) {
        final boundary = _nativeBoundary(function, member, '$owner.$name');
        if (boundary != null) return boundary;
      }
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
        // ..through the one boundary the runtime answers (`dart_native`
        // in the prelude): the `@Native` symbol the engine registers it
        // under, the arguments as objects, and whether a value comes
        // back. The generated code sees only the Dart signature; what
        // the symbol does is the native host's (run455: the first panic
        // past the bindings' constructors was `__nativeSetNeedsReport
        // Timings`).
        final boundary = _nativeBoundary(function, member, name);
        if (boundary != null) return boundary;
        if (_nativeSymbol(member) == null &&
            Platform.environment['DART2RUST_TRACE_NATIVE'] != null) {
          stderr.writeln(
            'TRACE_NATIVE $name no symbol: ${member.annotations.map((a) => a is ConstantExpression && a.constant is InstanceConstant ? (a.constant as InstanceConstant).fieldValues.entries.map((e) => '${e.key.asField.name.text}=${e.value.toString().substring(0, e.value.toString().length.clamp(0, 90))}').join(';') : a.runtimeType.toString()).join(' | ')}',
          );
        }
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
    // ..and for an `async` body the *awaited* one: a slot's `FutureOr<T>
    // Function()` taking `() async { .. return v; }` expects `T` of the
    // body's returns, the future around it being the closure's own
    // (`Future<bool>(() async {..})`, run430).
    final async = function.asyncMarker == AsyncMarker.Async;
    final expected = async ? _awaitedType(_expectedReturn) : _expectedReturn;
    _expectedReturn = null;
    // An `async` body's `return v` is the future's value: the returns are
    // widened into the awaited type (a `{'response': ..}` returned from a
    // `Future<dynamic>` goes behind a handle, run460).
    final own = async
        ? (_awaitedType(function.returnType) ?? function.returnType)
        : function.returnType;
    _voidReturn = (expected ?? own) is VoidType;
    _returnsType = expected ?? own;
    _asyncBody = async;
    // A `sync*` body collects what it yields into the list it returns:
    // `yield x` pushes, `yield* xs` extends, a bare `return` hands the
    // list back, and so does falling off the end. Eager where Dart is
    // lazy, which only an unbounded generator could tell apart
    // (`_OverlayEntryWidgetState._createChildIterable`, run655).
    final outerSyncStar = _syncStarElement;
    final DartType? syncStarElement;
    if (function.asyncMarker == AsyncMarker.SyncStar) {
      final declared = function.returnType;
      syncStarElement =
          declared is InterfaceType && declared.typeArguments.length == 1
          ? declared.typeArguments.single
          : const DynamicType();
    } else {
      syncStarElement = null;
    }
    _syncStarElement = syncStarElement;
    final outerEdge = _edgeReturn;
    // ..the awaited type for an `async` body, whose `return v` is the
    // future's value (`Future<T?> send()` returning `T?`, ws421).
    final declaredReturn = function.returnType;
    _edgeReturn = expected == null && function.parent is Member
        ? (function.asyncMarker == AsyncMarker.Async &&
                  declaredReturn is InterfaceType &&
                  declaredReturn.classNode.name == 'Future' &&
                  declaredReturn.typeArguments.length == 1
              ? declaredReturn.typeArguments.single
              : declaredReturn)
        : null;
    try {
      // A parameter a closure assigns lives in a cell, as a local one
      // does (`_capturedWrites`): rebound over itself before the body.
      // ..through a temporary: the cell's own name is a cell already by
      // the time its initializer prints.
      final rebound = <IrStmt>[];
      for (final p in [
        ...function.positionalParameters,
        ...function.namedParameters,
      ]) {
        if (!_capturedWrites.contains(p)) continue;
        final type = _localIrType(p);
        final held = '__p${_nextTemporary++}';
        rebound.add(
          IrLocalDecl(held, type, IrLocal(_paramName(p))..rustType = type),
        );
        rebound.add(
          IrLocalDecl(
            _paramName(p),
            type,
            IrLocal(held)..rustType = type,
            cell: true,
          ),
        );
      }
      if (syncStarElement != null) {
        final element = _type(syncStarElement);
        final listed = IrType('List', arguments: [element]);
        return IrBlock([
          ...rebound,
          IrLocalDecl(
            _syncStarOut,
            listed,
            IrListLiteral(const [], element)..rustType = listed,
          ),
          statement(body),
          IrReturn(IrLocal(_syncStarOut)..rustType = listed),
        ]);
      }
      if (rebound.isEmpty) return statement(body);
      return IrBlock([...rebound, statement(body)]);
    } finally {
      _voidReturn = outer;
      _returnsType = outerType;
      _asyncBody = outerAsync;
      _edgeReturn = outerEdge;
      _syncStarElement = outerSyncStar;
    }
  }

  /// The element type of the `sync*` body being lowered, or null.
  DartType? _syncStarElement;

  /// The list a `sync*` body collects into.
  static const _syncStarOut = '__yielded';

  /// Whether the body being lowered is an `async` one.
  bool _asyncBody = false;

  /// `Future<T>` or `FutureOr<T>` -> `T`; anything else unchanged.
  static DartType? _awaitedType(DartType? t) {
    if (t is FutureOrType) return t.typeArgument;
    if (t is InterfaceType &&
        t.classNode.name == 'Future' &&
        t.typeArguments.length == 1) {
      return t.typeArguments.single;
    }
    return t;
  }

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

  static final _unreachable = IrLiteral.unreachable;

  /// An `int` value stored into a variable *declared* `num` (an `f64`).
  IrExpr _intoDeclaredNum(Expression value, DartType declared, IrExpr lowered) {
    if (declared is! InterfaceType || declared.classNode.name != 'num')
      return lowered;
    // A literal says so itself: `num _n = 0` at the top level has no
    // context for `getStaticType` and came out as `RefCell<f64>::new(0)`.
    if (value is IntLiteral) return _toF64(lowered);
    final given = _staticType(value);
    if (given is InterfaceType &&
        given.classNode.name == 'int' &&
        given.nullability != Nullability.nullable) {
      return _toF64(lowered);
    }
    // A `num` whose static type is `num` -- TFA folded `1 is double ?
    // pow(2, 52) : 1.0e300.floor()` to its `int` branch and the
    // conditional's type stayed `num` -- is cast too: `f64 as f64` is a
    // no-op Rust accepts, and `i64 as f64` is the cast that was missing.
    if (given is InterfaceType &&
        given.classNode.name == 'num' &&
        given.nullability != Nullability.nullable &&
        value is! DoubleLiteral) {
      return _toF64(lowered);
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
    // An extension type is its representation at run time: `_AxisSize` is
    // a `Size`, and passed without the clone it moved out of the local the
    // next argument reads (`RenderFlex._computeSizes`, ws716).
    if (type is ExtensionType) {
      return _clonedWhenPassed(type.extensionTypeErasure);
    }
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

  /// The number the receiver of the call whose arguments are being lowered
  /// is (`double`/`int`), or null: Dart's `num` is not a type here, so a
  /// `num` parameter of a number's own method is the receiver's own.
  String? _numReceiver;

  /// The declared return type of the function being lowered, for `return`
  /// to widen into when it is nullable and the value is not.
  DartType? _returnsType;

  // -- Declarations -----------------------------------------------------------

  static final Map<String, int> _untypedCensus = {};

  /// `DART2RUST_CENSUS=1`: what reached a slot untyped, by node kind.
  static void dumpUntyped() {
    if (Platform.environment['DART2RUST_CENSUS'] != '1') return;
    final items = _untypedCensus.entries.toList()
      ..sort((a, b) => b.value.compareTo(a.value));
    for (final e in items.take(25)) {
      print('UNTYPED ${e.value} ${e.key}');
    }
  }

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
      // ..and a `final` collection filled in place (`log.add(..)`,
      // `pendingFontFutures.remove(..)`) needs the cell as much: the
      // read was a clone, and the mutation went into the clone (the
      // supermix fixture's log stayed empty, ws576).
      final mutable =
          !field.isConst && (!field.isFinal || _mutatedStatics.contains(field));
      try {
        constants.add(
          IrConstDecl(
            field.name.text,
            _type(field.type),
            // Into the declared type: a `dynamic` top-level holding a
            // struct is an `Rc<dyn Object>` (intl's locale data).
            _intoDeclaredNum(
              init,
              field.type,
              _widened(init, field.type, expression(init)),
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
      // type's members into top-level functions taking the representation
      // value, exactly as a plain extension's are (`MediaQueryHinge|get#
      // hinge`), and the extension type itself is its representation type
      // (`_type`): translated as those are. Refused until ws525, which
      // took `RenderBoxContainerDefaultsMixin.defaultComputeDistanceTo
      // HighestActualBaseline` -- and every flex baseline -- with it.
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
    final clean = _topLevelName(name);
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
          : _topLevelName(node.name.text);
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
              _edgeType(p.type),
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
              _edgeType(p.type),
              named: true,
              hasDefault: p.defaultValue != null,
              defaultValue: _default(p),
            ),
        ],
        _edgeReturnType(fn),
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
        : _topLevelName(node.name.text);
    return IrMethod(
      name,
      [
        for (final (i, p) in node.function.positionalParameters.indexed)
          IrParam(
            _paramName(p),
            _edgeType(p.type),
            kept: _keeps(node.function, p),
            mutRef: _fillsParameter(node, i),
          ),
        for (final p in node.function.namedParameters)
          if (!_inspectorOnly(p.parameterName))
            IrParam(
              p.parameterName,
              _edgeType(p.type),
              named: true,
              kept: _keeps(node.function, p),
            ),
      ],
      _edgeReturnType(node.function),
      _withEdgeParams(node.function, _body(node.function)),
      typeParameters: [
        for (final p in node.function.typeParameters)
          if (!_erasedParameter(p)) p.name ?? 'T',
      ],
      typeParameterBounds: _traitBounds(node.function),
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
    // An enum is a value whatever its methods do with `this`.
    if (node.isEnum) return false;
    // An object whose `this` leaves as a value -- handed to a call
    // (`addObserver(this)`), stored in another object, put in a literal
    // -- is held by someone else afterwards, and only a handle can be:
    // counted (`Rc<dyn WidgetsBindingObserver> <= _WidgetsAppState`,
    // ws436). Not one that merely *returns* `this`: a value returned is
    // a copy, which is what it was before (counting those was +901
    // stubs at ws437, `Matrix4` and `WidgetState` among them).
    // ..and an `Object` slot is a handle slot too (`Rc<dyn Object>`): an
    // `Expando` keyed by `this`, a `Map<Object, ..>`, `identical(this, x)`
    // all keep the object by identity (`_profiledBinaryMessengers[this]`
    // in `BasicMessageChannel`, run461).
    final escapes = _ThisEscapes(
      (t) =>
          t is DynamicType ||
          (t is InterfaceType &&
              (_abstractLike(t.classNode) ||
                  (t.classNode.name == 'Object' &&
                      t.classNode.enclosingLibrary.importUri.toString() ==
                          'dart:core'))),
    );
    node.accept(escapes);
    if (escapes.found) return true;
    // ..and in the constructors above it, whose bodies run as this class's
    // (flattened in): `PlatformInterface`'s `_instanceTokens[this] = token`
    // keys an Expando by identity, which a value has none of (run491).
    for (final above in _kernelAncestors(node)) {
      if (above.enclosingLibrary.importUri.scheme == 'dart') continue;
      for (final k in above.constructors) {
        k.accept(escapes);
        if (escapes.found) return true;
      }
    }
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
    // ..or mutated through an alias anywhere in the program: a value
    // handed on is a copy, and Dart's is the one object.
    if (aliasMutated.contains(node)) return true;
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

  /// The classes mutated through an alias, whole program (see
  /// `alias_mutation.dart`): counted, since a Rust value handed to a
  /// method is a copy (`writeValue(buffer, ..)` filled a copy of the
  /// `WriteBuffer`, run509).
  final Set<Class> aliasMutated;

  /// The type parameters the program uses covariantly, whole program (see
  /// `covariance.dart`): erased, since a trait object of one instantiation
  /// is no trait object of another (`Route<void>` kept as a
  /// `Route<dynamic>` by the navigator).
  final Set<TypeParameter> covariantParameters;

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
      dartName: node.name,
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
    _counted = _countedClass(node);

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
    _kernelClasses[node.name] = node;
    // A supertype clause instantiates its base as much as a slot does
    // (`_census`): `CBuilder<C extends Constraints> extends Builder<C>` is
    // where `Builder<Constraints>` gets named, and it was spelled argument
    // by argument, past the census (the atbounds fixture).
    for (final st in [
      if (node.supertype != null) node.supertype!,
      if (node.mixedInType != null) node.mixedInType!,
      ...node.implementedTypes,
    ]) {
      if (st.typeArguments.isNotEmpty) {
        _census(
          InterfaceType(
            st.classNode,
            Nullability.nonNullable,
            st.typeArguments,
          ),
        );
      }
    }
    bool _enumBound(TypeParameter p) {
      final bound = p.bound;
      return bound is InterfaceType &&
          bound.classNode.name == 'Enum' &&
          bound.classNode.enclosingLibrary.importUri.toString() == 'dart:core';
    }

    bool numericBound(TypeParameter p) {
      final bound = p.bound;
      return bound is InterfaceType &&
          bound.classNode.enclosingLibrary.importUri.toString() ==
              'dart:core' &&
          const {'num', 'int', 'double'}.contains(bound.classNode.name);
    }

    final cls = IrClass(
      node.name,
      typeParameters: [
        for (final p in node.typeParameters)
          if (!_erasedParameter(p)) p.name ?? 'T',
      ],
      numericParameters: {
        for (final p in node.typeParameters)
          if (!_erasedParameter(p) && numericBound(p)) p.name ?? 'T',
      },
      enumParameters: {
        for (final p in node.typeParameters)
          if (!_erasedParameter(p) && _enumBound(p)) p.name ?? 'T',
      },
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
      // An enum's mixins too: `enum WidgetState with WidgetStatesConstraint`
      // is a Rust enum, and the mixin is the trait impl that lets its value
      // stand where the mixin's type goes -- dropped, the value had no
      // `impl WidgetStatesConstraint` to be cast through (5 at ws751). A
      // mixin that declares fields still has nowhere to put them on an
      // enum, and the emission refuses as it would for any other class.
      mixins: mixins,
      iterableElement: node.isEnum ? null : _iterableElementIr(node),
      // The class's own `implements` clause. The applied mixins reached
      // through `implementedTypes` above belong to the *synthetic* classes on
      // the way up, not to this one, so the two lists do not overlap.
      // A mixin's `on` types come along: `SourceSpanMixin on SourceSpan`
      // calls `start` on `this`, and the free function holding that body is
      // bounded by the trait, which had to say it is a `SourceSpan` too.
      // An enum's own `implements` clause too: the trait impl it gets is
      // what lets its value stand where the interface is (`_emitBaseImpl`;
      // an enum into an `Rc<dyn Ts>`, ws510).
      interfaces: node.isEnum
          ? [
              for (final t in node.implementedTypes)
                if (t.classNode.name != '_Enum' && t.classNode.name != 'Enum')
                  _type(t.asInterfaceType),
            ]
          : [
              for (final t in node.implementedTypes) _type(t.asInterfaceType),
              if (node.isMixinDeclaration)
                for (final t in node.onClause) _type(t.asInterfaceType),
              // ..and what every application puts under it
              // (`_appliedOver`).
              if (node.isMixinDeclaration) ..._appliedOver(node),
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

    // Each refusal names its member; with `DART2RUST_TRACE=<class>` the
    // stack of every refusal in that class goes to stderr (finding the
    // binding's constructor refusal took an hour without it, run432).
    final trace = Platform.environment['DART2RUST_TRACE'] == node.name;
    void refuse(String member, Unsupported error, StackTrace stack) {
      refused.add('$member: $error');
      if (trace) stderr.writeln('TRACE ${node.name}.$member: $error\n$stack');
    }

    for (final field in node.fields) {
      try {
        _lowerField(cls, field);
      } on Unsupported catch (error, stack) {
        refuse(field.name.text, error, stack);
      }
    }
    for (final ctor in node.constructors) {
      try {
        _lowerConstructor(cls, ctor);
      } on Unsupported catch (error, stack) {
        refuse(ctor.name.text.isEmpty ? 'new' : ctor.name.text, error, stack);
      }
    }
    for (final procedure in node.procedures) {
      // A hollow mixin method: its body, from an application of the mixin.
      final lowered = node.isMixinDeclaration && procedure.isAbstract
          ? _appliedBody(node, procedure) ?? procedure
          : procedure;
      final wasBack = _appliedBack;
      if (!identical(lowered, procedure)) {
        _appliedBack = _appliedBackMap(node, lowered.enclosingClass);
      }
      try {
        // ..under the declaration's own signature: the application's copy
        // has the mixin's parameter substituted (`RenderBox?` for
        // `ChildType?`), and the trait is the mixin's, not one
        // application's (`RenderObjectWithChildMixin.child`, ws475).
        _lowerProcedure(
          cls,
          lowered,
          signature: identical(lowered, procedure) ? null : procedure,
        );
      } on Unsupported catch (error, stack) {
        refuse(procedure.name.text, error, stack);
        final stub = _stubFor(procedure, '$error');
        if (stub != null) cls.methods.add(stub);
      } finally {
        _appliedBack = wasBack;
      }
    }
    _typeArgumentGetters(node, cls);
    // ..and the fields the declaration no longer lists, held by an
    // application: known to the trait for their cells only
    // (`IrClass.appliedFields`), typed by the declaration's own getter.
    if (node.isMixinDeclaration) {
      final own = {for (final f in node.fields) f.name.text};
      final seenApplied = <String>{};
      for (final application in applications[node] ?? const <Class>[]) {
        for (final f in application.fields) {
          if (f.isStatic || own.contains(f.name.text)) continue;
          if (!seenApplied.add(f.name.text)) continue;
          try {
            cls.appliedFields.add(
              IrFieldDecl(
                _memberName(f),
                _type(_declaredFieldType(f) ?? f.type),
                isFinal: f.isFinal,
                isLate: f.isLate,
              ),
            );
          } on Unsupported {
            // Unspelled: no cell to hand out.
          }
        }
      }
    }
    // ..and the mixin's methods the declaration no longer lists at all,
    // from an application that kept them (`_appliedProcedure`).
    if (node.isMixinDeclaration) {
      // By name *and* kind: a getter the declaration kept does not stand
      // for the setter of the same name it dropped
      // (`RenderAnimatedOpacityMixin.alwaysIncludeSemantics=`, written by
      // `RenderAnimatedOpacity`'s constructor, was left out: run537).
      String keyOf(Procedure p) => p.isSetter ? '${p.name.text}=' : p.name.text;
      final declared = {
        for (final p in node.procedures) keyOf(p),
        for (final f in node.fields) f.name.text,
        for (final f in node.fields)
          if (!f.isFinal) '${f.name.text}=',
      };
      final seen = <String>{};
      for (final application in applications[node] ?? const <Class>[]) {
        for (final p in application.procedures) {
          if (p.isAbstract || p.isStatic) continue;
          if (declared.contains(keyOf(p)) || !seen.add(keyOf(p))) {
            continue;
          }
          final wasBack = _appliedBack;
          _appliedBack = _appliedBackMap(node, application);
          try {
            // Typed by the mixin, with the application's arguments taken
            // back out (`_unapplied`): the trait's `SlotType`, not the
            // `Slot` this application put in (ws490). The *body* takes
            // them back out too, for the erased parameters
            // (`_appliedBack`): `visitChildren` cast to this application's
            // `FlexParentData` where the trait holds the bound (run740).
            _lowerProcedure(
              cls,
              p,
              retype: (t) => _unapplied(t, application, node),
            );
          } on Unsupported catch (error, stack) {
            refuse(p.name.text, error, stack);
          } finally {
            _appliedBack = wasBack;
          }
        }
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
        // ..typed by the mixin's declaration, as the trait is: a copy has
        // the application's arguments where the mixin's erased parameter
        // stood (`RenderBox?` for `ChildType?` in `RenderFlex`'s
        // `_lastChild`), and the trait says the bound (ws477).
        for (final field in anonymous.fields) {
          if (!own.add(field.name.text)) continue;
          try {
            _lowerField(cls, field, declaredType: _declaredFieldType(field));
          } on Unsupported catch (error, stack) {
            refuse(field.name.text, error, stack);
          }
        }
        // ..but not a `dart:` mixin's own bodies: `IterableMixin`'s `skip`,
        // `where` and `cast` build `SkipIterable`, `WhereIterable` and
        // `CastIterable`, which are `dart:collection`'s private classes and
        // are not translated -- the prelude answers `Iterable`'s members on
        // a class that is one, through `__to_list` (`Board extends
        // Iterable<BoardPoint?>`, 9 at ws774).
        final from = anonymous.mixedInClass ?? anonymous;
        final env = typeEnvironment;
        final preludeMixin =
            from.enclosingLibrary.importUri.scheme == 'dart' &&
            env != null &&
            _iterableElement(
                  node.getThisType(env.coreTypes, Nullability.nonNullable),
                ) !=
                null;
        for (final procedure in anonymous.procedures) {
          if (procedure.isAbstract || !own.add(procedure.name.text)) continue;
          if (preludeMixin) continue;
          try {
            _lowerProcedure(
              cls,
              procedure,
              signature: _cloneSignature(procedure),
            );
          } on Unsupported catch (error, stack) {
            refuse(procedure.name.text, error, stack);
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
      // An abstract class that *is* an `Iterable<E>` declares its
      // `iterator`, so the trait has one and the walk beside it
      // (`__to_list`) can run: a `Rc<dyn Characters>` had neither, and
      // every `Iterable` member on one was a call to nothing (6 at ws782).
      final element = cls.iterableElement;
      if (element != null && declared.add('iterator')) {
        cls.abstractMethods.add(
          IrMethod(
            'iterator',
            const [],
            IrType('DartIterator', arguments: [element]),
            const IrBlock([]),
            isGetter: true,
          ),
        );
      }
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

  void _lowerField(IrClass cls, Field field, {DartType? declaredType}) {
    _enter(field);
    final name = _memberName(field);
    // A copy's field under the mixin's declared type (see the applied
    // members' lowering), with the mixin's parameters substituted by
    // this class's arguments for them -- `StateMixin<T>`'s `T? _value`
    // is `Value<T>`'s own `T?`, projected by the same rule as an own
    // field's, where the erased `ChildType?` is `RenderObject?` and a
    // kept `LayoutInfoType` is the `BoxConstraints` the application
    // put in (get's `Value<T>._value`: held as `Option<T>` while read and
    // written as the edge's `Or`, run486).
    final type = declaredType != null ? _fieldTypeHere(field) : field.type;
    IrType fieldIrType() => _edgeType(type);
    // An enum's own members are its variants and the CFE's bookkeeping; neither
    // becomes a field or a constant on the Rust side.
    if (cls.isEnum) return;
    if (field.isStatic) {
      final init = field.initializer;
      if (init == null) throw Unsupported('static without initialiser', name);
      cls.constants.add(
        IrConstDecl(
          name,
          _type(type),
          _intoDeclaredNum(init, type, _widened(init, type, expression(init))),
          // A `static final` is computed once on first use, which is what
          // `LazyLock` is. It was refused while there was nothing to say it
          // with; there is now.
          isLazy: !field.isConst,
          // A plain `static` is assignable, so it needs a cell as well as the
          // lock -- the same shape a mutable top-level has. 73 writes to
          // these were refused as `expression StaticSet`, most of them a
          // `??=` caching something on the class.
          // ..or a `static final` collection filled in place (see the
          // top-level rule).
          isMutable:
              !field.isConst &&
              (!field.isFinal || _mutatedStatics.contains(field)),
        ),
      );
    } else {
      if (_inspectorOnly(name, type)) return;
      final initial = field.initializer;
      if (Platform.environment['DART2RUST_TRACE_FIELD'] == name &&
          initial != null) {
        final lowered = expression(initial);
        stderr.writeln(
          'TRACE_FIELD $name: initial=${initial.runtimeType} lowered=${lowered.runtimeType} type=${lowered.rustType} slot=$type translated=$_slotTranslated coerceByType=$coerceByType',
        );
      }
      cls.fields.add(
        IrFieldDecl(
          name,
          fieldIrType(),
          isFinal: field.isFinal,
          // Into the field's type, and across a projected one (`T? _result
          // = null` in a generic route, ws414).
          initial: initial == null
              ? null
              : _acrossEdge(
                  _widened(initial, type, expression(initial)),
                  type,
                  toOption: false,
                ),
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
      params.add(IrParam(_paramName(p), _edgeType(p.type)));
    }
    for (final p in node.function.namedParameters) {
      // The inspector's parameter is dropped with its field. See
      // `_inspectorOnly`.
      if (_inspectorOnly(p.parameterName)) continue;
      params.add(IrParam(p.parameterName, _edgeType(p.type), named: true));
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
        inits[_memberName(init.field)] = _acrossEdge(
          _widened(init.value, init.field.type, expression(init.value)),
          init.field.type,
          toOption: false,
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
                // Into the field's type, as a written initialiser is
                // (`_frameTimelineTask: TimelineTask? = TimelineTask()`
                // needed its `Some`, run433).
                // ..the field's type as this class holds it: a copy's
                // erased `Map<SlotType, ChildType>` takes the `const {}`
                // retyped (ws490).
                final movedType = _fieldTypeHere(moved.field);
                inits.putIfAbsent(
                  moved.field.name.text,
                  () => _acrossEdge(
                    _widened(moved.value, movedType, expression(moved.value)),
                    movedType,
                    toOption: false,
                  ),
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
          superArgs = _constructing(
            init.target.function,
            _superBinding(node.enclosingClass, init.target.enclosingClass),
            () => _arguments(init.arguments, init.target.function),
          );
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
        if (param != null) inits[_memberName(field)] = IrLocal(param);
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

  void _lowerProcedure(
    IrClass cls,
    Procedure node, {
    Procedure? signature,
    DartType Function(DartType)? retype,
  }) {
    _enter(node);
    // A copy with no declaration left to take a signature from: each of
    // its types re-typed (`retype`, the application's arguments taken out).
    if (signature == null && retype != null) {
      final own = node.function;
      for (final p in own.positionalParameters) {
        final t = retype(p.type);
        if (t != p.type) _declaredParamTypes[p] = t;
      }
      for (final p in own.namedParameters) {
        final t = retype(p.type);
        if (t != p.type) _declaredParamTypes[p] = t;
      }
      if (!node.isAbstract) _expectedReturn = retype(own.returnType);
    }
    // The declared signature's types for the body's parameters (see the
    // mixin lowering): a read of one is typed by the declaration.
    if (signature != null) {
      final own = node.function;
      final sig = signature.function;
      for (
        var i = 0;
        i < own.positionalParameters.length &&
            i < sig.positionalParameters.length;
        i++
      ) {
        final p = own.positionalParameters[i];
        final t = _asApplied(
          sig.positionalParameters[i].type,
          signature.enclosingClass,
        );
        if (t != p.type) _declaredParamTypes[p] = t;
      }
      for (final p in own.namedParameters) {
        for (final q in sig.namedParameters) {
          if (q.parameterName == p.parameterName) {
            final t = _asApplied(q.type, signature.enclosingClass);
            if (t != p.type) _declaredParamTypes[p] = t;
          }
        }
      }
      // ..and its returns widen into the declaration's return type.
      if (!node.isAbstract) {
        _expectedReturn = _asApplied(sig.returnType, signature.enclosingClass);
      }
    }
    DartType paramType(Variable p, DartType declared) =>
        _declaredParamTypes[p] ?? declared;
    // An unnamed factory has no name in Kernel; an empty identifier stopped
    // all 37 members of vector_math's classes through `_computeFailing`.
    // `new`, as the backend spells the call.
    final name = node.kind == ProcedureKind.Factory && node.name.text.isEmpty
        ? 'new'
        : node.name.isPrivate &&
              (node.kind == ProcedureKind.Getter ||
                  node.kind == ProcedureKind.Setter)
        ? _dartName(_memberName(node))
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
      // ..except a `toString` the programmer *wrote*: an enhanced enum may
      // override it, and dropped, `dart_to_string` fell back to the
      // `Kind.material` an enum prints by default where Dart printed
      // `MATERIAL` (`GalleryDemoCategory.displayTitle`, run732). The
      // implicit one is the CFE's and carries no body of its own.
      final written =
          name == 'toString' && !node.isSynthetic && node.function.body != null;
      if ((implicit.contains(name) && !written) || node.isSynthetic) return;
      // Not implicit: a method or getter the programmer wrote. It goes in the
      // enum's `impl`, where it loses nothing.
      if (cls.values.isEmpty) {
        throw Unsupported('member of an enum with no values', cls.name);
      }
    }

    final params = [
      for (final (i, p) in node.function.positionalParameters.indexed)
        IrParam(
          _paramName(p),
          _edgeType(paramType(p, p.type)),
          kept: _keeps(node.function, p),
          hasDefault: p.defaultValue != null,
          defaultValue: _default(p),
          mutRef: _fillsParameter(node, i),
        ),
      for (final p in node.function.namedParameters)
        if (!_inspectorOnly(p.parameterName))
          IrParam(
            p.parameterName,
            _edgeType(paramType(p, p.type)),
            named: true,
            kept: _keeps(node.function, p),
            hasDefault: p.defaultValue != null,
            defaultValue: _default(p),
          ),
      // The hidden `Type` parameters last (`_typeValues`).
      ..._typeValueParams(node),
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
      signature == null
          ? (retype == null
                ? _edgeReturnType(node.function)
                : (() {
                    final r = retype(node.function.returnType);
                    return r is NeverType
                        ? const IrType('Never')
                        : _edgeType(r);
                  })())
          : (() {
              final r = _asApplied(
                signature.function.returnType,
                signature.enclosingClass,
              );
              return r is NeverType ? const IrType('Never') : _edgeType(r);
            })(),
      node.isAbstract
          ? const IrBlock([])
          : _withEdgeParams(node.function, _body(node.function)),
      typeParameters: [
        for (final p in node.function.typeParameters)
          if (!_erasedParameter(p)) p.name ?? 'T',
      ],
      typeParameterBounds: _traitBounds(node.function),
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
        IrParam(_paramName(p), _edgeType(p.type)),
      for (final p in node.function.namedParameters)
        IrParam(p.parameterName, _edgeType(p.type), named: true),
    ];
    cls.methods.add(
      IrMethod(
        name,
        params,
        _edgeReturnType(node.function),
        IrBlock([
          // `noSuchMethod` yields `Never`, spelled `Infallible`, which does
          // not coerce to the forwarder's own return type; the prelude's
          // `never()` does the coercion `!` would have done.
          IrReturn(
            IrStaticCall(null, 'never', [
              // Always `?`: `noSuchMethod` yields `Never`, whether it is the
              // class's own or the `Object` default the prelude gives every
              // class (15 forwarders on `_DefaultSnapshotPainter` reached a
              // method the class does not declare, ws751).
              IrCall(IrThis(), 'noSuchMethod', [invocation], fails: true),
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
        KernelFrontend._mutatingListNames.contains(node.name.text) &&
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
    if (member == null) return;
    found.add(member.enclosingLibrary);
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
        KernelFrontend._mutatingListNames.contains(node.name.text) &&
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
        KernelFrontend._mutatingListNames.contains(node.name.text) &&
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
        KernelFrontend._mutatingListNames.contains(node.name.text) &&
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
