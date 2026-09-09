part of '../frontend_kernel.dart';

augment class KernelFrontend {
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
      // A class that *is* the prelude's future (`_futureLike`): `_type`
      // spells every value of it `Future<T>` and `_construct` makes one
      // with `future_ready`, so nothing here ever holds the struct and
      // nothing can call its members -- a `then` on one of its values is
      // the prelude's. Emitting it translated members no call reaches:
      // `SynchronousFuture.then`, refused for the `is Future<R>` its body
      // asks, and `whenComplete`, which returns `this` where the prelude's
      // future goes (ws853).
      if (_futureLike(cls)) continue;
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

  /// The name a top-level member carries into the IR.
  ///
  /// The same two functions the declaration uses, so an importer and a
  /// definition spell one identifier: an extension member's CFE name
  /// (`BaselineOffset|+`, `StringCharacters|get#characters`) is not a Rust
  /// one, and a setter is a function named for the store.
  static String topLevelIrName(Member member) =>
      member is Procedure && member.kind == ProcedureKind.Setter
      ? _topLevelSetterName(member.name.text)
      : _topLevelName(member.name.text);

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
    // An enhanced enum whose per-variant state could not be read off the
    // constants still gets its **variants**. Emitting nothing at all was
    // meant to stop it being emitted "as a plain one with its members
    // dropped" -- but an empty enum drops every member *and* the name, and
    // it does so silently: `KeyboardLockMode` came out `enum
    // KeyboardLockMode {}`, so `KeyboardLockMode::NumLock` named a variant
    // that is not there and `Set<KeyboardLockMode>` had no `DartEq`
    // (2 stubs and a refusal at ws939). With the variants in, only the
    // members that read the unrecovered state fail, and they fail the way
    // everything else does -- visibly, one stub each. `valueFields` stays
    // empty, so no getter is written for the state (`the_class`).
    final values = !node.isEnum ? const <String>[] : declared;
    final recovered = values.isNotEmpty || !node.isEnum ? values : names;
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
    )..enumElementsDeclared = node.fields.any((f) => f.isEnumElement);
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
    // all 37 members of vector_math's classes through the per-class
    // failure fixed point the uniform Result model replaced (ws886~1).
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
