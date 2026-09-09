// Type parameters the program uses covariantly.
//
// Dart's generics are covariant: a `Route<void>` is a `Route<dynamic>`, and
// the navigator keeps every route as one. Rust's trait objects are not: an
// `Rc<dyn Route<()>>` is no `Rc<dyn Route<Rc<dyn Object>>>`, and nothing
// turns one into the other short of a forwarding object -- a new identity,
// where routes are compared by identity. So a parameter the program ever
// uses covariantly -- a value of `C<A>` reaching a slot of `C<B>` with `A`
// and `B` different -- is *erased*: spelled as its bound, an `Rc<dyn
// Object>`, the values converted at the boundaries by the rules an erased
// bound already has (`_erasedParameter`, `FromDynamic`). Found once over
// the whole component, at every flow site: an argument into a parameter,
// an initialiser or an assignment into a declared type, a return into the
// function's, an element into a literal's, a conditional's arms. Not a
// cast: `this as Animation<double>` is a runtime check the cast table
// answers by instantiation, and counting it erased `Animation<T>`.
// Then handed down: a subclass parameter put in an erased position of its
// supertype (`MaterialPageRoute<T> extends PageRoute<T>`) is erased too.
import 'dart:io';

import 'package:kernel/ast.dart';
import 'package:kernel/class_hierarchy.dart';
import 'package:kernel/type_algebra.dart';
import 'package:kernel/type_environment.dart';

/// Whether a hierarchy whose parameters disagree is made consistent by
/// *raising* the unmarked side rather than dropping the marked one (see
/// the loop that uses it). `DART2RUST_RAISE=0` drops, as before ws938.
final bool _raise = Platform.environment['DART2RUST_RAISE'] != '0';

bool _translated(Class c) {
  final uri = c.enclosingLibrary.importUri;
  return uri.scheme != 'dart' || uri.toString() == 'dart:ui';
}

/// The type parameters of `libraries`' classes used covariantly.
Set<TypeParameter> covariantParameters(
  Iterable<Library> libraries,
  TypeEnvironment environment,
) {
  final found = <TypeParameter>{};
  final scan = _FlowScan(environment, found);
  for (final library in libraries) {
    library.accept(scan);
  }
  // An override *is* a flow site, and the only one this shape has: Dart's
  // covariance lets `_SwitchDefaultsM3.thumbColor` answer a
  // `WidgetStateProperty<Color>` where `SwitchThemeData.thumbColor` is a
  // `WidgetStateProperty<Color?>?`, and every read is typed by the
  // declaration, so no expression ever shows the two meeting. 82 accessors
  // in the gallery are that shape, all of them the Material "defaults"
  // idiom (run703).
  for (final library in libraries) {
    for (final cls in library.classes) {
      if (!_translated(cls)) continue;
      final thisType = cls.getThisType(
        environment.coreTypes,
        Nullability.nonNullable,
      );
      for (final above in _ancestorsOf(cls)) {
        if (!_translated(above)) continue;
        final asAbove = environment.hierarchy.getTypeAsInstanceOf(
          thisType,
          above,
        );
        if (asAbove is! InterfaceType) continue;
        final substitution = Substitution.fromInterfaceType(asAbove);
        for (final name in _resultNames(cls)) {
          final declared = _resultTypeOf(above, name);
          final own = _resultTypeOf(cls, name);
          if (declared == null || own == null) continue;
          scan._compare(own, substitution.substituteType(declared));
        }
      }
    }
  }
  // Handed down to a subclass parameter in an erased position, to a fixpoint.
  final classes = [for (final library in libraries) ...library.classes];
  var changed = true;
  while (changed) {
    changed = false;
    for (final cls in classes) {
      for (final above in _supertypesOf(cls)) {
        final params = above.classNode.typeParameters;
        for (
          var i = 0;
          i < above.typeArguments.length && i < params.length;
          i++
        ) {
          if (!found.contains(params[i])) continue;
          final arg = above.typeArguments[i];
          if (arg is TypeParameterType &&
              cls.typeParameters.contains(arg.parameter) &&
              found.add(arg.parameter)) {
            changed = true;
          }
        }
      }
    }
  }
  // All or nothing along a hierarchy: a parameter erased while the
  // supertype's it is passed into keeps its own leaves `TweenImpl` no
  // `Animatable<f64>` (+99 at ws521); a subclass parameter that cannot be
  // erased (`RestorableEnum<T extends Enum>`, no handle to erase to) keeps
  // its supertype's too. Dropped, to a fixpoint: the sites that asked for
  // it stay as they were, which is fewer stubs than a half-erased chain.
  final hierarchy = environment.hierarchy;
  final subtypes = hierarchy is ClosedWorldClassHierarchy
      ? hierarchy.computeSubtypesInformation()
      : null;
  bool erasable(TypeParameter p) {
    final bound = p.bound;
    if (bound is DynamicType) return true;
    if (bound is! InterfaceType) return false;
    final cls = bound.classNode;
    if (cls.name == 'Object' &&
        cls.enclosingLibrary.importUri.toString() == 'dart:core') {
      return true;
    }
    if (!_translated(cls)) return false;
    return cls.isAbstract ||
        (subtypes != null && subtypes.getSubtypesOf(cls).length > 1);
  }

  // What the hierarchy rule dropped stays dropped: the member rule below
  // put `Entry.T` back each time the rule took it, forever (ws647).
  final dropped = <TypeParameter>{};
  changed = true;
  while (changed) {
    changed = false;
    for (final cls in classes) {
      for (final above in _supertypesOf(cls)) {
        if (!_translated(above.classNode)) continue;
        final params = above.classNode.typeParameters;
        for (
          var i = 0;
          i < above.typeArguments.length && i < params.length;
          i++
        ) {
          final arg = above.typeArguments[i];
          if (arg is! TypeParameterType ||
              !cls.typeParameters.contains(arg.parameter)) {
            continue;
          }
          final below = arg.parameter;
          final up = params[i];
          final belowMarked = found.contains(below) && erasable(below);
          final upMarked = found.contains(up) && erasable(up);
          if (belowMarked != upMarked) {
            // All or nothing along a hierarchy, and the direction is *on*
            // wherever both sides can be: a parameter marked below and
            // passed straight into the supertype's is the same parameter,
            // so erasing both keeps the chain whole where dropping both
            // throws away a mark a real flow site made.
            // `_InheritedProviderScopeElement<T> implements
            // InheritedContext<T>` was that: marked below (`element = this`
            // into an `_InheritedProviderScopeElement<T?>` slot), unmarked
            // above, both dropped, and six of provider's members then had
            // an `X<T>` where `X<T?>` was declared (`DART2RUST_RAISE=0`
            // turns this off and drops as before).
            // ..and only where *every* subtype that passes its own
            // parameter into this position is already marked. With one of
            // them unmarked, raising erases a supertype for a subtype that
            // never asked: `Animatable.T` went that way at ws938 (`Tween`
            // marked, `TweenSequence` and `_ChainedEvaluation` not), and
            // `Tween<double>`'s values went behind `Rc<dyn Object>` -- 7
            // more stubs against the 6 it cleared. `InheritedContext<T>`
            // has one subtype passing a parameter through, and it is the
            // marked one.
            bool everyBelowMarked(Class aboveClass, int at) {
              for (final c in classes) {
                for (final st in _supertypesOf(c)) {
                  if (st.classNode != aboveClass) continue;
                  if (at >= st.typeArguments.length) continue;
                  final a = st.typeArguments[at];
                  if (a is! TypeParameterType) continue;
                  if (!c.typeParameters.contains(a.parameter)) continue;
                  if (!found.contains(a.parameter) || !erasable(a.parameter)) {
                    return false;
                  }
                }
              }
              return true;
            }

            final unmarked = belowMarked ? up : below;
            if (_raise &&
                erasable(unmarked) &&
                !dropped.contains(unmarked) &&
                (!belowMarked || everyBelowMarked(above.classNode, i))) {
              if (Platform.environment['DART2RUST_TRACE_COVARIANT'] != null) {
                stderr.writeln(
                  'TRACE_COVARIANT_RAISE '
                  '${(belowMarked ? above.classNode : cls).name}'
                  '<${unmarked.name}> with '
                  '${(belowMarked ? cls : above.classNode).name}',
                );
              }
              found.add(unmarked);
              changed = true;
              continue;
            }
            if (Platform.environment['DART2RUST_TRACE_COVARIANT'] != null &&
                (found.contains(below) || found.contains(up))) {
              stderr.writeln(
                'TRACE_COVARIANT_DROP ${cls.name}<${below.name}> '
                '(${belowMarked ? "marked" : "not"}) vs '
                '${above.classNode.name}<${up.name}> '
                '(${upMarked ? "marked" : "not"})',
              );
            }
            dropped.add(below);
            dropped.add(up);
            if (found.remove(below)) changed = true;
            if (found.remove(up)) changed = true;
          }
        }
      }
    }
    for (final p in found.toList()) {
      if (!erasable(p) && found.remove(p)) changed = true;
    }
    bool admissible(TypeParameter p) => erasable(p) && !dropped.contains(p);
    // An erased parameter erases what its members spell it into: `Bag<T>`
    // erased holds `add(Entry<Object>)`, and the `Entry<S>` the program
    // hands it is a `C<A>` into a `C<B>` again (the outparam fixture).
    // Only into a parameter that can be erased, or the filter above and
    // this would take turns forever.
    // ..into a *struct*'s parameter only: a trait's wider instantiation
    // is a cast the object answers (see `_FlowScan._twinInto`).
    bool structLike(Class c) =>
        !c.isAbstract && (subtypes?.getSubtypesOf(c).length ?? 1) <= 1;
    for (final cls in classes) {
      if (!_translated(cls)) continue;
      final erased = [
        for (final p in cls.typeParameters)
          if (found.contains(p)) p,
      ];
      if (erased.isEmpty) continue;
      for (final t in _memberTypes(cls)) {
        if (_spelledInto(t, erased, admissible, structLike, found)) {
          changed = true;
        }
      }
    }
  }
  if (Platform.environment['DART2RUST_TRACE_COVARIANT'] != null) {
    for (final p in found) {
      final owner = p.declaration;
      stderr.writeln(
        'TRACE_COVARIANT ${owner is Class ? owner.name : owner} <${p.name}>',
      );
    }
  }
  return found;
}

/// The declared types of a class's members.
Iterable<DartType> _memberTypes(Class cls) sync* {
  for (final f in cls.fields) {
    yield f.type;
  }
  for (final p in cls.procedures) {
    yield p.function.returnType;
    for (final v in p.function.positionalParameters) {
      yield v.type;
    }
    for (final v in p.function.namedParameters) {
      yield v.type;
    }
  }
  for (final c in cls.constructors) {
    for (final v in c.function.positionalParameters) {
      yield v.type;
    }
    for (final v in c.function.namedParameters) {
      yield v.type;
    }
  }
}

/// Marks the parameters of every translated class `t` instantiates at one
/// of `erased` (at any depth), when they can be erased; whether any was
/// new.
bool _spelledInto(
  DartType t,
  List<TypeParameter> erased,
  bool Function(TypeParameter) erasable,
  bool Function(Class) structLike,
  Set<TypeParameter> found, [
  int depth = 0,
]) {
  if (depth > 6) return false;
  var changed = false;
  bool into(DartType a) =>
      _spelledInto(a, erased, erasable, structLike, found, depth + 1);
  if (t is InterfaceType) {
    final params = t.classNode.typeParameters;
    for (var i = 0; i < t.typeArguments.length; i++) {
      final a = t.typeArguments[i];
      if (_translated(t.classNode) &&
          structLike(t.classNode) &&
          i < params.length &&
          _mentionsAny(a, erased) &&
          erasable(params[i]) &&
          found.add(params[i])) {
        changed = true;
      }
      if (into(a)) changed = true;
    }
  } else if (t is FunctionType) {
    if (into(t.returnType)) changed = true;
    for (final a in t.positionalParameters) {
      if (into(a)) changed = true;
    }
    for (final n in t.namedParameters) {
      if (into(n.type)) changed = true;
    }
  } else if (t is FutureOrType) {
    return into(t.typeArgument);
  }
  return changed;
}

bool _mentionsAny(DartType t, Iterable<TypeParameter> params) {
  if (t is TypeParameterType) return params.contains(t.parameter);
  if (t is InterfaceType)
    return t.typeArguments.any((a) => _mentionsAny(a, params));
  if (t is FutureOrType) return _mentionsAny(t.typeArgument, params);
  if (t is FunctionType) {
    return _mentionsAny(t.returnType, params) ||
        t.positionalParameters.any((a) => _mentionsAny(a, params)) ||
        t.namedParameters.any((n) => _mentionsAny(n.type, params));
  }
  return false;
}

/// A class's supertypes with anonymous mixin applications looked through:
/// `ModalRoute<T> extends TransitionRoute<T> with LocalHistoryRoute<T>` is
/// `ModalRoute<T> extends _App<T>`, and the application -- deduplicated,
/// in a library of its own -- broke the chain (`PageRoute<T>` dropped for
/// an unmarked `ModalRoute<T>`, ws521).
/// Every class above `cls`, transitively.
Set<Class> _ancestorsOf(Class cls) {
  final out = <Class>{};
  final queue = [cls];
  while (queue.isNotEmpty) {
    final here = queue.removeLast();
    for (final above in _supertypesOf(here)) {
      if (out.add(above.classNode)) queue.add(above.classNode);
    }
  }
  return out;
}

/// The names a class declares a *result* for: a field, a getter, a method.
Iterable<String> _resultNames(Class cls) sync* {
  for (final f in cls.fields) {
    yield f.name.text;
  }
  for (final p in cls.procedures) {
    if (p.kind != ProcedureKind.Setter) yield p.name.text;
  }
}

/// The declared result type of `cls`'s own `name`, in `cls`'s own terms.
DartType? _resultTypeOf(Class cls, String name) {
  for (final f in cls.fields) {
    if (f.name.text == name) return f.type;
  }
  for (final p in cls.procedures) {
    if (p.name.text == name && p.kind != ProcedureKind.Setter) {
      return p.kind == ProcedureKind.Getter
          ? p.function.returnType
          : p.function.computeFunctionType(Nullability.nonNullable);
    }
  }
  return null;
}

Iterable<Supertype> _supertypesOf(Class cls, [int depth = 0]) sync* {
  for (final above in [
    if (cls.supertype != null) cls.supertype!,
    if (cls.mixedInType != null) cls.mixedInType!,
    ...cls.implementedTypes,
  ]) {
    final target = above.classNode;
    if (target.isAnonymousMixin && depth < 8) {
      final substitution = Substitution.fromSupertype(above);
      for (final inner in _supertypesOf(target, depth + 1)) {
        yield substitution.substituteSupertype(inner);
      }
    } else {
      yield above;
    }
  }
}

class _FlowScan extends RecursiveVisitor {
  _FlowScan(this.environment, this.found);

  final TypeEnvironment environment;
  final Set<TypeParameter> found;

  /// The closed world's subtype table, computed once: per procedure it
  /// was a full recomputation (ws647 translated for 20 minutes).
  late final ClassHierarchySubtypes? _subtypes = () {
    final hierarchy = environment.hierarchy;
    return hierarchy is ClosedWorldClassHierarchy
        ? hierarchy.computeSubtypesInformation()
        : null;
  }();

  StaticTypeContext? _context;
  final List<FunctionNode> _functions = [];
  Member? _member;

  String _where() {
    final m = _member;
    if (m == null) return '?';
    return '${m.enclosingClass?.name ?? m.enclosingLibrary.name}.${m.name.text}';
  }

  @override
  void visitProcedure(Procedure node) {
    _context = StaticTypeContext(node, environment);
    _member = node;
    _throughTwin(node);
    super.visitProcedure(node);
    _context = null;
  }

  /// A generic instance method of a trait-like class is reached through
  /// its *erased twin*, whose type parameters are spelled as their bounds:
  /// a `Bag<S>` declared on it arrives as a `Bag<Object>` -- one value of
  /// `C<A>` reaching a slot of `C<B>`, the flow this scan exists for,
  /// only the compiler's own. `Layer.findAnnotations<S>(AnnotationResult<
  /// S> result, ..)` fills the caller's result *in place*, and the twin's
  /// `AnnotationResult<Rc<dyn Object>>` was another struct (run646).
  void _throughTwin(Procedure node) {
    final cls = node.enclosingClass;
    if (cls == null ||
        node.isStatic ||
        node.kind != ProcedureKind.Method ||
        node.function.typeParameters.isEmpty ||
        !_translated(cls)) {
      return;
    }
    final traitLike =
        cls.isAbstract || (_subtypes?.getSubtypesOf(cls).length ?? 0) > 1;
    if (!traitLike) return;
    final fn = node.function;
    final own = fn.typeParameters.toSet();
    for (final t in [
      fn.returnType,
      for (final v in fn.positionalParameters) v.type,
      for (final v in fn.namedParameters) v.type,
    ]) {
      _twinInto(t, own);
    }
  }

  /// Marks the parameters of every *struct* `t` instantiates at one of a
  /// method's own parameters. A trait's are left: `drive<U>(Animatable<U>)`
  /// hands its twin an `Rc<dyn Animatable<Object>>`, which the object's
  /// wider impl answers for by a cast; erasing `Animatable.T` instead
  /// took `Tween<double>`'s values behind `Rc<dyn Object>` (+12, ws647).
  void _twinInto(DartType t, Set<TypeParameter> own, [int depth = 0]) {
    if (depth > 6) return;
    if (t is InterfaceType) {
      final cls = t.classNode;
      final params = cls.typeParameters;
      for (var i = 0; i < t.typeArguments.length; i++) {
        final a = t.typeArguments[i];
        if (i < params.length &&
            _translated(cls) &&
            _structLike(cls) &&
            _mentionsAny(a, own) &&
            found.add(params[i])) {
          if (Platform.environment['DART2RUST_TRACE_COVARIANT'] != null) {
            stderr.writeln(
              'TRACE_COVARIANT_TWIN ${cls.name}<${params[i].name}> '
              'in ${_where()}: $t',
            );
          }
        }
        _twinInto(a, own, depth + 1);
      }
    } else if (t is FunctionType) {
      _twinInto(t.returnType, own, depth + 1);
      for (final a in t.positionalParameters) {
        _twinInto(a, own, depth + 1);
      }
      for (final n in t.namedParameters) {
        _twinInto(n.type, own, depth + 1);
      }
    } else if (t is FutureOrType) {
      _twinInto(t.typeArgument, own, depth + 1);
    }
  }

  /// A class Rust holds as a struct: concrete, nothing under it.
  bool _structLike(Class c) =>
      !c.isAbstract && (_subtypes?.getSubtypesOf(c).length ?? 1) <= 1;

  @override
  void visitConstructor(Constructor node) {
    _context = StaticTypeContext(node, environment);
    _member = node;
    super.visitConstructor(node);
    _context = null;
  }

  @override
  void visitField(Field node) {
    _context = StaticTypeContext(node, environment);
    _member = node;
    final init = node.initializer;
    if (init != null) _flow(init, node.type);
    super.visitField(node);
    _context = null;
  }

  @override
  void visitFunctionNode(FunctionNode node) {
    _functions.add(node);
    super.visitFunctionNode(node);
    _functions.removeLast();
  }

  DartType? _typeOf(Expression e) {
    final context = _context;
    if (context == null) return null;
    try {
      return e.getStaticType(context);
    } catch (_) {
      return null;
    }
  }

  void _flow(Expression value, DartType slot) {
    final have = _typeOf(value);
    final traced = _traceFlow;
    if (traced != null &&
        (traced.startsWith('@')
            ? _where().contains(traced.substring(1))
            : '$slot'.contains(traced))) {
      // Printed *before* the null check: a slot this scan looked at whose
      // value has no static type is silent otherwise, and "not visited"
      // and "no type for the value" read the same from `_compare` alone.
      stderr.writeln(
        'TRACE_FLOW_SLOT ${_where()}: ${have ?? "<no type>"} -> $slot',
      );
    }
    if (have == null) return;
    _compare(have, slot);
  }

  /// `have` reaching a slot of `slot`: every parameter whose arguments
  /// differ between the two, at any depth, is used covariantly.
  /// `DART2RUST_TRACE_FLOW=<ClassName>`: every value-into-slot this scan
  /// looks at where either side names that class, whether or not it marks
  /// anything. `TRACE_COVARIANT_SITE` only says what *was* marked, which
  /// leaves "why was this one not" with nothing to read.
  static final String? _traceFlow =
      Platform.environment['DART2RUST_TRACE_FLOW'];

  void _compare(DartType have, DartType slot, [int depth = 0]) {
    if (depth > 6) return;
    final traced = _traceFlow;
    if (traced != null &&
        ('$have'.contains(traced) || '$slot'.contains(traced))) {
      stderr.writeln('TRACE_FLOW ${_where()}: $have -> $slot (depth $depth)');
    }
    if (have is FutureOrType) return _compare(have.typeArgument, slot, depth);
    if (slot is FutureOrType) return _compare(have, slot.typeArgument, depth);
    if (have is InterfaceType && slot is InterfaceType) {
      final asSlot = environment.hierarchy.getTypeAsInstanceOf(
        have,
        slot.classNode,
      );
      if (asSlot == null) return;
      final params = slot.classNode.typeParameters;
      for (
        var i = 0;
        i < asSlot.typeArguments.length &&
            i < slot.typeArguments.length &&
            i < params.length;
        i++
      ) {
        final a = asSlot.typeArguments[i];
        final b = slot.typeArguments[i];
        if (_same(a, b)) continue;
        if (_translated(slot.classNode) && found.add(params[i])) {
          if (Platform.environment['DART2RUST_TRACE_COVARIANT'] != null) {
            stderr.writeln(
              'TRACE_COVARIANT_SITE ${slot.classNode.name}<${params[i].name}> '
              'in ${_where()}: $have -> $slot',
            );
          }
        }
        _compare(a, b, depth + 1);
      }
      return;
    }
    if (have is FunctionType && slot is FunctionType) {
      _compare(have.returnType, slot.returnType, depth + 1);
      for (
        var i = 0;
        i < have.positionalParameters.length &&
            i < slot.positionalParameters.length;
        i++
      ) {
        _compare(
          have.positionalParameters[i],
          slot.positionalParameters[i],
          depth + 1,
        );
      }
    }
  }

  /// The same type; Dart's two top types are one type here (`Object?` is
  /// `dynamic`).
  ///
  /// Nullability counts: this compares *type arguments*, and `Color` and
  /// `Color?` are `Rc<dyn Color>` and `Option<Rc<dyn Color>>` -- a
  /// `WidgetStateProperty<Color>` is no `WidgetStateProperty<Color?>`
  /// here, however freely Dart's covariance passes one for the other
  /// (run703). At the top the two are the same slot and this is not
  /// asked.
  static bool _same(DartType a, DartType b) {
    if (_top(a) && _top(b)) return true;
    // A closure's own parameter and its structural copy in the closure's
    // type are one parameter (`<T>(..) => MaterialPageRoute<T>(..)` into
    // a `PageRoute<T> Function<T>(..)` slot).
    final aName = _parameterName(a), bName = _parameterName(b);
    if (aName != null && bName != null) {
      return aName == bName && a.nullability == b.nullability;
    }
    return a == b;
  }

  static String? _parameterName(DartType t) => t is TypeParameterType
      ? t.parameter.name
      : t is StructuralParameterType
      ? t.parameter.name
      : null;

  static bool _top(DartType t) =>
      t is DynamicType ||
      (t is InterfaceType &&
          t.classNode.name == 'Object' &&
          t.classNode.enclosingLibrary.importUri.toString() == 'dart:core');

  void _arguments(Arguments arguments, FunctionType type) {
    final positional = type.positionalParameters;
    for (
      var i = 0;
      i < arguments.positional.length && i < positional.length;
      i++
    ) {
      _flow(arguments.positional[i], positional[i]);
    }
    for (final named in arguments.named) {
      for (final p in type.namedParameters) {
        if (p.name == named.name) _flow(named.value, p.type);
      }
    }
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    _arguments(node.arguments, node.functionType);
    super.visitInstanceInvocation(node);
  }

  /// The declared parameter types, substituted: `computeFunctionType`
  /// copies a callee's own parameters into structural ones, which then
  /// compare unequal to the declaration's and marked every generic
  /// constructor call covariant.
  void _declaredArguments(
    Arguments arguments,
    FunctionNode fn,
    Substitution substitution,
  ) {
    final positional = fn.positionalParameters;
    for (
      var i = 0;
      i < arguments.positional.length && i < positional.length;
      i++
    ) {
      _flow(
        arguments.positional[i],
        substitution.substituteType(positional[i].type),
      );
    }
    for (final named in arguments.named) {
      for (final p in fn.namedParameters) {
        if (p.parameterName == named.name) {
          _flow(named.value, substitution.substituteType(p.type));
        }
      }
    }
  }

  @override
  void visitStaticInvocation(StaticInvocation node) {
    final fn = node.target.function;
    final substitution = fn.typeParameters.isEmpty
        ? Substitution.empty
        : Substitution.fromPairs(
            fn.typeParameters,
            fn.typeParameters.length == node.arguments.types.length
                ? node.arguments.types
                : [for (final p in fn.typeParameters) p.bound],
          );
    _declaredArguments(node.arguments, fn, substitution);
    super.visitStaticInvocation(node);
  }

  @override
  void visitConstructorInvocation(ConstructorInvocation node) {
    _declaredArguments(
      node.arguments,
      node.target.function,
      Substitution.fromInterfaceType(node.constructedType),
    );
    super.visitConstructorInvocation(node);
  }

  @override
  void visitFunctionInvocation(FunctionInvocation node) {
    final type = node.functionType;
    if (type != null) _arguments(node.arguments, type);
    super.visitFunctionInvocation(node);
  }

  @override
  void visitVariableDeclaration(VariableDeclaration node) {
    final init = node.initializer;
    if (init != null) _flow(init, node.variable.type);
    super.visitVariableDeclaration(node);
  }

  @override
  void visitVariableSet(VariableSet node) {
    _flow(node.value, node.variable.type);
    super.visitVariableSet(node);
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    final target = node.interfaceTarget;
    final owner = target.enclosingClass;
    final receiverType = _typeOf(node.receiver);
    var slot = target.setterType;
    if (owner != null && receiverType is InterfaceType) {
      final asOwner = environment.hierarchy.getTypeAsInstanceOf(
        receiverType,
        owner,
      );
      if (asOwner != null) {
        slot = Substitution.fromTypeDeclarationType(asOwner)
            .substituteType(slot);
      }
    }
    _flow(node.value, slot);
    super.visitInstanceSet(node);
  }

  @override
  void visitStaticSet(StaticSet node) {
    _flow(node.value, node.target.setterType);
    super.visitStaticSet(node);
  }

  @override
  void visitFieldInitializer(FieldInitializer node) {
    _flow(node.value, node.field.type);
    super.visitFieldInitializer(node);
  }

  @override
  void visitReturnStatement(ReturnStatement node) {
    final value = node.expression;
    if (value != null && _functions.isNotEmpty) {
      final fn = _functions.last;
      var slot = fn.returnType;
      if (fn.asyncMarker == AsyncMarker.Async) {
        slot = slot is InterfaceType && slot.typeArguments.length == 1
            ? slot.typeArguments.single
            : slot is FutureOrType
            ? slot.typeArgument
            : const DynamicType();
      }
      if (fn.asyncMarker == AsyncMarker.Sync) _flow(value, slot);
    }
    super.visitReturnStatement(node);
  }

  @override
  void visitListLiteral(ListLiteral node) {
    for (final e in node.expressions) {
      _flow(e, node.typeArgument);
    }
    super.visitListLiteral(node);
  }

  @override
  void visitSetLiteral(SetLiteral node) {
    for (final e in node.expressions) {
      _flow(e, node.typeArgument);
    }
    super.visitSetLiteral(node);
  }

  @override
  void visitMapLiteral(MapLiteral node) {
    for (final e in node.entries) {
      _flow(e.key, node.keyType);
      _flow(e.value, node.valueType);
    }
    super.visitMapLiteral(node);
  }

  @override
  void visitConditionalExpression(ConditionalExpression node) {
    _flow(node.then, node.staticType);
    _flow(node.otherwise, node.staticType);
    super.visitConditionalExpression(node);
  }
}
