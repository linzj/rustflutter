part of '../backend_rust.dart';

// Closures, and how a field is spelled: cells, `late`, sharing.
augment class RustBackend {
  /// A closure literal.
  ///
  /// The parameter types are written out rather than inferred: a closure passed
  /// straight into a call would usually infer, but one stored or returned would
  /// not, and a compiler that emits both spellings depending on where the
  /// closure lands is two rules where one will do.
  String _closure(IrClosure node) {
    // A parameter the body assigns is `mut`, as a method's is (E0384 on
    // `decodeError = ..` inside `_getNextFrame`'s callback).
    final assigned = _assignedIn(node.body);
    final params = node.params
        // Spelled as the function type spells them: a parameter of an abstract
        // class is `&dyn X` there, and a closure declaring `Rc<dyn X>` did not
        // match the `Fn(&dyn X)` it was handed to -- 133 `E0631`s.
        // ..except a `Future`, which as a borrowed `impl Future` is not
        // allowed in a closure's parameters (E0562); owned it is `Pin<Box<..>>`.
        .map(
          (p) =>
              '${assigned.contains(p.name) ? 'mut ' : ''}${snake(p.name)}: '
              '${type(p.type, owned: p.type.name == 'Future' || p.type.isFunction)}',
        )
        .join(', ');
    // A closure that copies `final` fields in is a `move` closure with the
    // copies bound just before it. It borrows `self` not at all, which is the
    // whole point: it outlives the call that made it.
    // A closure made inside a null-aware's body that reads the bound
    // value (`it`, a reference the `map` hands in) keeps its own clone and
    // moves it: an adapter made under `handler == null ? null : (m) async
    // {..}` borrowed `it` past the statement (E0716, ws486).
    final usesBound = (_WalkSelf()..statement(node.body)).readsBound;
    final bindings = [
      // The handle first: a closure that calls a method keeps the object.
      // The handle, not a clone of a reference: inside a trait body `this`
      // is `dart_self_<trait>()`, on a counted class `dart_self_ref().get()`
      // (`let __me = this_.clone()` captured a `&__Self` into a `'static`
      // closure, 91 lifetime errors at ws334).
      if (node.holdsSelf) 'let $_countedSelf = ${_selfHandle()};',
      // A lending local function inside a `&mut self` method: the closure
      // is a `move` one (it owns the locals it copied in), and a `&mut Self`
      // is not `Copy` -- so it moves `self` rather than borrowing it. A
      // reborrow bound here is what it moves instead, and it lasts exactly
      // as long as the closure (`_popPolicyDataIfNeeded`, ws838).
      if (_lendingClosure && _selfIsMut && !node.holdsSelf)
        'let $_lentSelf = &mut *$_selfName;',
      if (usesBound) 'let $_boundName = $_boundName.clone();',
      // `mut` when the body writes or lends the copy (`&mut keys` inside
      // `visitAncestorElements`'s callback, `PageStorageBucket._allKeys`).
      ...node.captures.map(
        (c) =>
            'let ${_assignedIn(node.body).contains(c.name) ? 'mut ' : ''}${snake(c.name)} = ${_copyOf(c)};',
      ),
      ...node.locals.map((l) => 'let ${snake(l)} = ${snake(l)}.clone();'),
    ].join(' ');
    // ..and the body's own locals are `mut` by its own reckoning, as a
    // method body's are (`_reassigned` is set per member in
    // `emit_members`). Without this a local declared *and* assigned inside
    // a closure was judged by whatever set the enclosing member left
    // behind: `Widget dialog = themes?.wrap(..) ?? pageChild;` followed by
    // `dialog = SafeArea(child: dialog)` in `_DialogRoute`'s `pageBuilder`
    // came out `let dialog` and would not compile (E0384).
    //
    // Unioned rather than replaced: the closure still reads and writes the
    // locals it captured, and those were decided outside.
    final savedReassigned = _reassigned;
    _reassigned = {..._reassigned, ...assigned};
    // Which of them are cells, for the body that is about to be written.
    final savedCells = _cellLocals;
    _cellLocals = {
      ..._cellLocals,
      for (final c in node.captures)
        if (_sharedField(c.name) != null) c.name: _isCopy(type(c.type)),
    };
    // ..and which of those cells hold a `late` field: read unwrapped, as
    // the field is on the object (`_localizationsResolver` in
    // `WidgetsApp.build`'s closure, ws482).
    final savedLateCells = _lateCellLocals;
    _lateCellLocals = {
      ..._lateCellLocals,
      for (final c in node.captures)
        if (_sharedField(c.name) != null && _lateField(c.name) != null) c.name,
    };
    final saved = _out.length;
    final savedIndent = _indent;
    final savedSelf = _selfName;
    // A closure is a panic boundary: its own signature carries no `Result`,
    // whatever the method around it promised (`_thrown`).
    final savedFailure = _failure;
    final savedFlow = _inFlowClosure;
    // ..under the uniform Result model it does: a closure fails like a
    // function and says so in its type.
    _failure = _resultModel ? _error : null;
    // ..and a `try` inside it that returns carries the *closure's* value
    // out, not the enclosing method's (`registerExtension`'s callback in
    // `BindingBase.registerServiceExtension`, run485).
    final savedRustReturns = _rustReturns;
    final closureReturns = node.isAsync ? _awaited(node.returns) : node.returns;
    _rustReturns = _resultModel && closureReturns.name != 'raw'
        ? 'Result<${type(closureReturns)}, $_error>'
        : _rustReturns;
    // ..and so does the *declared* return `_returned` wraps against: left
    // at the enclosing method's, a closure returning a concrete class got
    // that method's trait around it -- `getIcon: (context) => Icons.menu`
    // inside a `Widget build` was `dart_object(IconData::new(..)) as
    // Rc<dyn Widget>` (`_ActionIcon`, 4 at ws751).
    final savedReturns = _returns;
    _returns = closureReturns;
    // Nor is it inside the try body's flow closure: a `return` in it is
    // the closure's own (`Ok(Some(..))` in `|x| builder.setDay(x)`).
    _inFlowClosure = false;
    // ..and the reborrow is what `this` is inside that closure.
    final lentSelf = _lendingClosure && _selfIsMut && !node.holdsSelf;
    final savedLendingBody = _lendingClosure;
    _lendingClosure = false;
    if (node.holdsSelf) {
      _selfName = _countedSelf;
    } else if (lentSelf) {
      _selfName = _lentSelf;
    }
    final savedCaptured = _closureCaptured;
    _closureCaptured = {
      ..._closureCaptured,
      for (final c in node.captures)
        if (_sharedField(c.name) == null) c.name,
      ...node.locals,
    };
    _indent = 0;
    // The body's own asyncness: a `try` inside an `async` closure of a
    // sync method wrapped its body in a closure that cannot `await`
    // (`setMessageHandler`'s handler, E0728 at ws461).
    final savedAsyncBody = _asyncBody;
    _asyncBody = node.isAsync;
    // A closure of its own is a slot's value again: its return type comes
    // from the `Rc<dyn Fn(..)>` it goes into, so the upcasts inside it are
    // left to Rust as they were (see `_spellsReturn`).
    final savedSpells = _spellsReturn;
    _spellsReturn = false;
    _body(node.body, node.isAsync ? _awaited(node.returns) : node.returns);
    _spellsReturn = savedSpells;
    _asyncBody = savedAsyncBody;
    _failure = savedFailure;
    _rustReturns = savedRustReturns;
    _returns = savedReturns;
    _inFlowClosure = savedFlow;
    _selfName = savedSelf;
    _lendingClosure = savedLendingBody;
    _closureCaptured = savedCaptured;
    final body = _out.sublist(saved).map(_inlineSafe).join(' ');
    _out.removeRange(saved, _out.length);
    _indent = savedIndent;
    final owns =
        node.captures.isNotEmpty ||
        node.locals.isNotEmpty ||
        node.holdsSelf ||
        usesBound;
    // `async |..|` is stable since Rust 1.85. A Dart `async` closure keeps
    // its `await`s, and a closure emitted without the word put every one of
    // them outside an async context: 79 `E0728`s.
    // Annotated: a `?` inside needs the error type spelled, and the body
    // ends in `Ok(..)`.
    // The value type is left to inference: naming it pulled types into
    // modules that never imported them (248 "cannot find type"), and a
    // closure signature may say `_`. The error type is what `?` needs.
    // An `async` closure is a plain closure returning the spawned future
    // of its body, as an `async` function is (`_emitAsyncWrapper`): the
    // captures it owns are cloned again inside, since the body moves them
    // into a `'static` future and a `Fn` closure keeps its own.
    final again = [
      if (node.holdsSelf) 'let $_countedSelf = $_countedSelf.clone();',
      ...node.captures.map(
        (c) => 'let ${snake(c.name)} = ${snake(c.name)}.clone();',
      ),
      ...node.locals.map((l) => 'let ${snake(l)} = ${snake(l)}.clone();'),
    ].join(' ');
    // An async closure is a function value like any other, returning
    // `Result`: its future inside `Ok` where the slot wants the future
    // (`Fn(..) -> Result<DartFuture<T>, E>`), and `Ok(())` after spawning
    // it where the slot wants nothing (`void Function(..)` handed an
    // `async` closure, `setMessageHandler`'s at run459). A bare
    // `-> DartFuture<_>` matched neither (12 stubs on the start path).
    final spawned =
        'DartFuture::spawn_named("${cls.name} closure", std::boxed::Box::pin(async move { $body }))';
    final wantsFuture =
        node.returns.name == 'Future' || node.returns.name == 'FutureOr';
    // ..and where the slot says `FutureOr<T>`, the future as that
    // (`Future<bool>(() async {..})` in `GetStorage`, ws470: the stub that
    // kept `main` waiting without a word).
    final asFutureOr = node.returns.name == 'FutureOr';
    // ..and where it says `Future<T>?` (a `MessageHandler`'s
    // `Future<ByteData?>? Function(ByteData?)`), the future inside `Some`
    // (`setMessageHandler`'s closure, ws474).
    final asNullable = node.returns.name == 'Future' && node.returns.nullable;
    final closure = node.isAsync
        ? (wantsFuture
              ? (asFutureOr
                    ? '${owns ? 'move ' : ''}|$params| -> Result<FutureOr<_>, $_error> { $again Ok(FutureOr::future($spawned)) }'
                    : asNullable
                    ? '${owns ? 'move ' : ''}|$params| -> Result<Option<DartFuture<_>>, $_error> { $again Ok(Some($spawned)) }'
                    : '${owns ? 'move ' : ''}|$params| -> Result<DartFuture<_>, $_error> { $again Ok($spawned) }')
              : '${owns ? 'move ' : ''}|$params| -> Result<_, $_error> { $again let _ = $spawned; Ok(()) }')
        : '${owns ? 'move ' : ''}|$params|${_resultModel ? ' -> Result<${_closureReturnSpelled(node.returns)}, $_error>' : ''} { $body }';
    _cellLocals = savedCells;
    _reassigned = savedReassigned;
    _lateCellLocals = savedLateCells;
    final whole = owns ? '{ $bindings $closure }' : closure;
    if (!node.boxed) return whole;
    // Unsized to the function type it is typed as, where that is
    // spelled: inside a `.map(|__f| ..)` there is no slot to infer
    // `Rc<dyn Fn>` from, and the `Rc<{closure}>` stayed one (a
    // conditional tear-off into `VoidCallback?`, ws549).
    // Plain: the slot it lands in unsizes it. Spelling the handle type
    // here (an `as`, then a typed `let`) named type parameters out of
    // scope and turned iterator closures into handles (+49 at ws551);
    // the one place with no slot to infer from, a null-aware `.map`,
    // spells its own return (`_nullAware`).
    return 'std::rc::Rc::new($whole)';
  }

  /// A field's type, wrapped when a closure has to see it change.
  ///
  /// `Rc<Cell<T>>` where `T` is `Copy` and `Rc<RefCell<T>>` where it is not:
  /// `Cell` needs no borrow flag and cannot panic, so it is the better answer
  /// wherever it fits. See `IrFieldDecl.shared`.
  String _fieldType(IrFieldDecl field) {
    final held = _heldType(field);
    // Every mutable field of a counted class, not only the ones a closure
    // names: an `Rc` hands out shared *immutable* access, so a method that
    // assigns a field cannot take `&mut self` and has to go through a cell.
    if (!_inCell(field)) return held;
    final cell = _isCopy(held) ? 'Cell' : 'RefCell';
    return 'std::rc::Rc<std::cell::$cell<$held>>';
  }

  /// Whether a field of *this* class is shared. Named rather than passed
  /// around: reads and writes reach it from several places.
  /// Whether a field is held in a cell: marked shared, or mutable in a
  /// counted class.
  bool _inCell(IrFieldDecl field) => _inCellOf(cls, field);

  /// ..for a field of `owner`, which a constant instance of another class
  /// needs to know (`MaterialColor { _swatch: .. }`, 81 at ws273).
  bool _inCellOf(IrClass owner, IrFieldDecl field) =>
      field.shared ||
      (owner.counted && _mutableOnCounted(field)) ||
      _handedByTraitOf(owner, field) ||
      _setThroughTraitOf(owner, field);

  /// A mutable field a trait this class implements declares: the trait's
  /// setter (`set_x(&self, ..)`, every non-final field has one) is how a
  /// base's body writes it, and `&self` can only write a cell. It was a
  /// `todo!` (`_DefaultRootPipelineOwner._manifold`, written by
  /// `PipelineOwner.attach`'s super function, run479).
  bool _setThroughTraitOf(IrClass owner, IrFieldDecl field) =>
      _writable(field) &&
      _supertypesOf(owner).any(
        (t) =>
            library.isAbstract(t.name) &&
            t.fields.any((f) => f.name == field.name && _writable(f)) &&
            _fieldsWrittenBy(t).contains(field.name),
      );

  /// A field some body may assign: not `final`, or a `late final` without
  /// an initialiser, which is assigned once somewhere (`late final
  /// ScrollbarPainter scrollbarPainter` set in `RawScrollbarState.
  /// initState`, run666: the trait had no setter for it).
  static bool _writable(IrFieldDecl field) =>
      !field.isFinal || (field.isLate && field.initial == null);

  /// The fields a trait's own bodies assign on `this` (`_manifold =
  /// manifold` in `PipelineOwner.attach`): those writes reach an
  /// implementer through the setter, so only those fields need the cell.
  /// Every trait-declared mutable field was a cell for one round (ws480),
  /// which made `Cell`s of what `_isCopy` misjudged and took the widgets
  /// crate down.
  Set<String> _fieldsWrittenBy(IrClass trait) =>
      _traitWrites.putIfAbsent(trait.name, () {
        final found = <String>{};
        void walk(IrStmt s) {
          switch (s) {
            case IrAssignField(:final name, :final target):
              if (target == null || target is IrThis) found.add(name);
            case IrBlock(:final statements):
              statements.forEach(walk);
            case IrIf(:final then, :final otherwise):
              walk(then);
              if (otherwise != null) walk(otherwise);
            case IrTryCatch(:final body, :final handler):
              walk(body);
              walk(handler);
            case IrTryFinally(:final body, :final finalizer):
              walk(body);
              walk(finalizer);
            case IrWhile(:final body):
              walk(body);
            case IrLabeled(:final body):
              walk(body);
            case IrSwitch(:final cases, :final otherwise):
              for (final one in cases) {
                walk(one.body);
              }
              if (otherwise != null) walk(otherwise);
            case IrForIn(:final body):
              walk(body);
            default:
              break;
          }
        }

        for (final m in trait.methods) {
          walk(m.body);
        }
        // ..and the writes through a handle of the trait's type from any
        // body in the program: `tween.end ??= tween.begin` on a
        // `Tween<dynamic>` in `_constructTweens` reaches `ThemeDataTween`
        // through `set_end`, which was a `todo!` there (run615).
        found.addAll(_writtenThroughHandles(trait));
        return found;
      });

  /// The fields set on a handle typed as `trait` (or a subtype of it)
  /// anywhere in the program, by name. Each class's bodies are walked
  /// once for the whole run (`_setterWritesIn`), and this unions them for
  /// one trait, once per backend.
  final Map<String, Set<String>> _externalWrites = {};

  Set<String> _writtenThroughHandles(IrClass trait) =>
      _externalWrites.putIfAbsent(trait.name, () {
        final found = <String>{};
        final classes = <IrClass>{
          ...library.elsewhere.values,
          ...library.classes,
        };
        for (final c in classes) {
          for (final entry in _setterWritesIn(c).entries) {
            final receiver = entry.key;
            if (receiver == trait.name) {
              found.addAll(entry.value);
              continue;
            }
            final receiverClass = library[receiver];
            if (receiverClass != null &&
                _supertypesOf(receiverClass).any((t) => t.name == trait.name)) {
              found.addAll(entry.value);
            }
          }
        }
        return found;
      });

  /// `_WalkSelf.setterWrites` over one class's bodies, memoised on the
  /// class object: the program's classes are shared by every module's
  /// backend, and walking them once per module was the whole program
  /// times the module count.
  static final _setterWritesOf = Expando<Map<String, Set<String>>>();

  static Map<String, Set<String>> _setterWritesIn(IrClass c) {
    final cached = _setterWritesOf[c];
    if (cached != null) return cached;
    final walk = _WalkSelf();
    for (final m in c.methods) {
      walk.statement(m.body);
    }
    for (final k in c.constructors) {
      final body = k.body;
      if (body != null) walk.statement(body);
    }
    return _setterWritesOf[c] = walk.setterWrites;
  }

  final Map<String, Set<String>> _traitWrites = {};

  /// A field that a trait this class implements hands out as a cell
  /// (`_handsCell`): the implementer holds it as one, so the trait body's
  /// in-place write lands on the object (66 `todo!`s at ws272 -- `_Theater.
  /// children`, every `ChangeNotifier._listeners` on a plain struct).
  bool _handedByTraitOf(IrClass owner, IrFieldDecl field) =>
      _handsCell(field) &&
      _supertypesOf(owner).any(
        (t) =>
            library.isAbstract(t.name) &&
            t.fields.any((f) => f.name == field.name),
      );

  /// Every class above `of`: superclasses, mixins and interfaces, transitively.
  List<IrClass> _supertypesOf(IrClass of) {
    final seen = <String>{};
    final out = <IrClass>[];
    void walk(String? name) {
      if (name == null || !seen.add(name)) return;
      final c = library[name];
      if (c == null) return;
      out.add(c);
      walk(c.superclass);
      for (final m in c.mixins) {
        walk(m.name);
      }
      for (final i in c.interfaces) {
        walk(i.name);
      }
    }

    walk(of.superclass);
    for (final m in of.mixins) {
      walk(m.name);
    }
    for (final i in of.interfaces) {
      walk(i.name);
    }
    return out;
  }

  /// On a counted class, what has to live in a cell: a field that is
  /// assigned, a `late` one (assigned after construction by definition), and
  /// a `final` collection -- `final Set<Image> _handles = {}` is never
  /// reassigned and is added to from `Image`'s constructor, which through a
  /// plain field behind an `Rc` cannot borrow mutably (E0596).
  bool _mutableOnCounted(IrFieldDecl field) =>
      !field.isFinal || field.isLate || _isMutableCollection(type(field.type));

  /// A trait's collection field that a trait body mutates in place is handed
  /// out as its cell: the value accessor clones, and `this_._trackers
  /// .borrow_mut().insert(..)` on a clone inserted into a copy (125 at ws271).
  /// ..and a field a closure in a trait body writes (`shared`): the
  /// closure captures the cell (`_copyOf`), which the trait must hand out
  /// (`_fadeoutTimer = null` inside `RawScrollbarState`'s timer callback,
  /// run666).
  /// A `late` one too when shared -- the copy a trait body's closure
  /// takes asks for the cell (`_copyOf`), and no trait declared it
  /// (`_configuration` inside `ScrollableState.setCanDrag`'s recognizer
  /// factory, run685); its cell holds the `Option` the struct holds
  /// (`_heldType`). A late collection stays a value: the in-place writes
  /// through `_cellPlace` do not look inside an `Option`.
  bool _handsCell(IrFieldDecl field) =>
      field.shared || (!field.isLate && _isMutableCollection(type(field.type)));

  /// The cell a handed-out field lives in: a `Cell` for a `Copy` value,
  /// as the struct holds it (a shared `int` counter a trait body's
  /// closure bumps, the closurefield fixture), a `RefCell` otherwise.
  String _cellType(String held) => _isCopy(held)
      ? 'std::rc::Rc<std::cell::Cell<$held>>'
      : 'std::rc::Rc<std::cell::RefCell<$held>>';

  static bool _isMutableCollection(String rust) =>
      // ..or an absent-or-not one (`Map<K, V>?` in a cell, mutated under
      // `?.`).
      (rust.startsWith('Option<') &&
          rust.endsWith('>') &&
          _isMutableCollection(rust.substring(7, rust.length - 1))) ||
      rust.startsWith('Vec<') ||
      rust.startsWith('Set<') ||
      rust.startsWith('Map<') ||
      rust.startsWith('Queue<') ||
      rust.startsWith('std::collections::VecDeque<') ||
      // The prelude's aliases of a `Vec` (`Float64List`, a counted
      // `Matrix4`'s storage, ws510) and the byte view written in place.
      const {
        'Int8List',
        'Int16List',
        'Int32List',
        'Int64List',
        'Uint8List',
        'Uint8ClampedList',
        'Uint16List',
        'Uint32List',
        'Uint64List',
        'Float32List',
        'Float64List',
        'ByteData',
      }.contains(rust);

  /// What a field holds, `Option`-wrapped when it is `late`.
  ///
  /// The wrapper goes *inside* the cell: a `late` field that a closure watches
  /// is `Rc<RefCell<Option<T>>>`, one cell holding one absent value, not two
  /// nested absences.
  String _heldType(IrFieldDecl field) {
    final held = type(field.type);
    return field.isLate ? 'Option<$held>' : held;
  }

  /// `_heldType` of a type already spelled (substituted for an impl).
  String _lateWrapped(IrFieldDecl field, String held) =>
      field.isLate ? 'Option<$held>' : held;

  /// The held type as the *declaration* spells it, for deciding a cell's
  /// kind: inside a wider impl for one instantiation (`_selfBinding`) a
  /// `T` field spells `i64`, but the struct's cell is the `RefCell` a
  /// `T` got (`NumVal<i64>.value.get()` on a `RefCell`, the restoreprop
  /// fixture).
  String _heldDecl(IrFieldDecl field) => _declSpelling(() => _heldType(field));

  String _declSpelling(String Function() spell) {
    final saved = _selfBinding;
    _selfBinding = const {};
    try {
      return spell();
    } finally {
      _selfBinding = saved;
    }
  }

  /// The `late` field of *this* class by that name, or null.
  IrFieldDecl? _lateField(String name) {
    for (final f in _allFields(cls)) {
      if (f.name == name) return f.isLate ? f : null;
    }
    return null;
  }

  /// Another class's field, when a read or write of it goes through a cell.
  ///
  /// The same question `_sharedField` answers for this class, asked of the
  /// class the front end named on the node: shared, or non-final on a counted
  /// class. Null when the owner is not in the crate, or the field is plain.
  IrFieldDecl? _cellFieldOf(String owner, String name) {
    final owned = library[owner];
    if (owned == null) return null;
    for (final f in _allFields(owned)) {
      if (f.name != name) continue;
      // The one predicate the struct's own reads use (`_inCellOf`): a
      // collection a trait hands out as a cell is one here too, and
      // `(widget as _Theater).children[i]` indexed the cell (ws547).
      return _inCellOf(owned, f) ? f : null;
    }
    return null;
  }

  IrFieldDecl? _sharedField(String name) {
    for (final f in _allFields(cls)) {
      if (f.name == name) return _inCell(f) ? f : null;
    }
    // ..and a mixin's own fields, which the declaration no longer lists:
    // `IrClass.appliedFields` holds them "for their cells only", and this
    // is the one question that is about their cells. Without it a closure
    // in a mixin captured the field's *value* through the plain accessor
    // and then could not write it back (ws1056).
    for (final f in cls.appliedFields) {
      if (f.name == name) return _inCell(f) ? f : null;
    }
    return null;
  }

  /// Whether a method's body makes a closure that keeps `this`.
  ///
  /// The whole body, not the three shapes a closure most often sits in: one
  /// written as an *argument* -- `applyTwice(() => scaled(v), x)` -- is the
  /// commonest of all, and missing it left the method taking `&self` while
  /// its closure cloned that, which clones the struct rather than the handle.
  static bool _handsOutSelf(IrMethod method) {
    final walk = _WalkSelf();
    walk.statement(method.body);
    return walk.holdsSelfClosure || walk.passesSelf;
  }

  /// The methods of a counted class that take `self: &Rc<Self>`: those
  /// that hand `this` out, and those that call one of them on `this` --
  /// `self.addPattern(..)` from a `&self` method could not reach a method
  /// wanting the handle (intl, 3). The same contagion as `_mutating`.
  late final Set<String> _handles = _computeHandles();

  Set<String> _computeHandles() {
    final handles = <String>{};
    final calls = <String, Set<String>>{};
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      final key = _rustName(method);
      if (_handsOutSelf(method)) handles.add(key);
      final walk = _WalkSelf();
      walk.statement(method.body);
      calls[key] = walk.selfCalls;
    }
    var changed = true;
    while (changed) {
      changed = false;
      for (final entry in calls.entries) {
        if (handles.contains(entry.key)) continue;
        if (entry.value.any((c) => handles.contains(snake(c)))) {
          handles.add(entry.key);
          changed = true;
        }
      }
    }
    return handles;
  }

  /// The name a counted closure gives its handle to `this`.
  static const _countedSelf = '__me';

  /// Captured locals that hold a cell, and whether it is a `Cell` (`true`)
  /// or a `RefCell`.
  var _cellLocals = <String, bool>{};

  /// The locals a chain step binds *by reference*: `xs.iter().map(|child|
  /// ..)` hands the body a `&Rc<dyn X>`, one deref short of the handle,
  /// and a receiver spelled from one needs `&**` where a value needs `&*`
  /// -- the same thing a null-aware binding (`IrBound`) already gets.
  /// Recorded here because only `_stepClosure` knows which parameters it
  /// bound that way (`FocusNode.toDiagnosticsNode` on a `.map`'s child).
  var _refLocals = <String>{};

  /// A copy of a field, for a closure to keep.
  ///
  /// `clone()` unless the type is `Copy`, where it would only be noise.
  String _copyOf(IrParam field) {
    // Inside a closure that copied the field already: its copy, the
    // local of that name -- reading `self.x` again borrowed `self` into a
    // nested `'static` closure (`_AnimatedCarousel.build`'s builder inside
    // its `LayoutBuilder` builder, run675).
    if (_closureCaptured.contains(field.name)) {
      return '${snake(field.name)}.clone()';
    }
    final read = '$_selfName.${snake(field.name)}';
    // A copied `late` field is unwrapped here rather than in the body: the
    // closure holds a `T`, so the reads inside it are ordinary local reads.
    // It takes the value the field has when the closure is *made*, which is
    // the same trade round 97 made for every copied field.
    final late = _lateField(field.name);
    // Inside a trait body (`this_: &__Self`) a field is an accessor, as
    // `_fieldRead` spells every read there: a shared field's cell through
    // its `_cell()` accessor, so the closure and the object keep one map
    // (`CachingAssetBundle.loadStructuredBinaryData`'s callbacks, run571).
    if (_fieldsAreAccessors) {
      if (_sharedField(field.name) != null) {
        return '${read}_cell()$_propagate';
      }
      final value = '$read()$_propagate';
      return late != null ? '$value.unwrap()' : value;
    }
    if (late != null && _sharedField(field.name) == null) {
      return _isCopy(_declSpelling(() => type(late.type)))
          ? '$read.unwrap()'
          : '$read.clone().unwrap()';
    }
    // A shared field is carried as a *handle*: the closure and the object must
    // see the same cell, which is the whole reason it is shared. Cloning an
    // `Rc` is cloning the handle, not the value.
    if (_sharedField(field.name) != null) return '$read.clone()';
    return _isCopy(type(field.type)) ? read : '$read.clone()';
  }

  /// The closure parameter a `?.` binds.
  ///
  /// One fixed name, not a fresh one per nesting level: a chained `a?.b?.c`
  /// nests the closures, and the inner one shadows the outer -- which is what
  /// the Dart means, since the inner access is about the inner value.
  static const _boundName = 'it';
}
