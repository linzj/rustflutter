// IR -> Rust source.
//
// For the value-type subset this backend targets, Rust has real answers rather
// than emulations: a Dart `operator +` becomes `impl Add`, not a vtable slot,
// and a `static const` becomes an `associated const`, not a lazily-initialised
// global. That is the whole reason to have a Rust backend instead of treating
// Rust as an assembler -- where the two languages agree, say so.
//
// Where they do not agree the backend stops. See `Unsupported`.
library;

import 'ir.dart';

/// Dart's primitives, in the spelling this project's crate uses.
///
/// `double` is `f32`, not `f64`: the hand port measures in `f32` throughout
/// because that is what the engine's geometry takes, and a translated value
/// type has to be able to sit beside one. This is the first place the backend
/// is opinionated about its target rather than neutral, and it is deliberate --
/// a compiler emitting `f64` here would produce code that cannot be called.
const _primitives = {
  'double': 'f32',
  'int': 'i64',
  'bool': 'bool',
  'String': 'String',
  'void': '()',
};

/// Dart operators that are Rust traits, and the trait's method name.
const _operatorTraits = {
  '+': ('Add', 'add'),
  '-': ('Sub', 'sub'),
  '*': ('Mul', 'mul'),
  '/': ('Div', 'div'),
  '%': ('Rem', 'rem'),
  'unary-': ('Neg', 'neg'),
};

String snake(String name) {
  final out = name.replaceAllMapped(
    RegExp(r'(?<!^)([A-Z])'),
    (m) => '_${m[1]}',
  ).toLowerCase();
  return out;
}

String screamingSnake(String name) => snake(name).toUpperCase();

class RustBackend {
  RustBackend(this.cls, {IrLibrary? library})
      : library = library ?? IrLibrary([cls]);

  final IrClass cls;

  /// The other classes in the same file.
  ///
  /// Needed for one question the backend cannot answer from `cls` alone: is
  /// this type name an abstract class? If it is, a value of that type is not a
  /// struct -- it is `dyn Trait`, and has to be behind a reference or a Box.
  final IrLibrary library;
  /// Lines, not a StringBuffer, so a member that turns out to be untranslatable
  /// can be rolled back. See [_member].
  final _out = <String>[];
  int _indent = 0;

  void _line(String text) {
    _out.add(text.isEmpty ? '' : '${'    ' * _indent}$text');
  }

  /// Emits one member, or a comment saying why it is missing.
  ///
  /// The front end has always refused member by member. The backend refused by
  /// *class*, so one member it could not emit took the whole class with it --
  /// and that only showed once super calls started working: `Alignment.add`
  /// stopped being refused for its super call and began being refused for the
  /// `is` beside it, which silently cost the entire class. Same lesson as the
  /// per-class fix one level up: the unit of refusal should be the unit of work.
  void _member(String what, void Function() body) {
    final mark = _out.length;
    final indent = _indent;
    try {
      body();
    } on Unsupported catch (error) {
      _out.removeRange(mark, _out.length);
      _indent = indent;
      _line('// NOT TRANSLATED: $what');
      _line('//   $error');
      _line('');
    }
  }

  void _doc(String? doc, {String prefix = '///'}) {
    if (doc == null || doc.isEmpty) return;
    for (final line in doc.split('\n')) {
      _line(line.isEmpty ? prefix : '$prefix ${line.trim()}');
    }
  }

  /// A Dart type in Rust.
  ///
  /// An abstract class has no storage of its own, so a value of that type
  /// cannot be a struct. It is `dyn Trait`, which is unsized, so it appears
  /// behind a `Box` when owned. Getting this wrong is not a style question:
  /// `fn add(other: AlignmentGeometry)` does not compile at all, because Rust
  /// has no way to know how big an `AlignmentGeometry` is.
  String type(IrType t, {bool owned = true}) {
    if (library.isAbstract(t.name)) {
      final dynamic_ = owned ? 'Box<dyn ${t.name}>' : '&dyn ${t.name}';
      return t.nullable ? 'Option<$dynamic_>' : dynamic_;
    }
    final mapped = _primitives[t.name] ?? t.name;
    return t.nullable ? 'Option<$mapped>' : mapped;
  }

  // -- Expressions ------------------------------------------------------------

  String expr(IrExpr e) {
    return switch (e) {
      IrLiteral(:final value, :final type) => _literal(value, type),
      IrLocal(:final name) => snake(name),
      IrThis() => '*$_selfName',
      IrField(:final target, :final name) =>
        '${_receiver(target)}.${snake(name)}',
      IrStatic(:final owner, :final name) => '$owner::${screamingSnake(name)}',
      IrBinary(:final op, :final left, :final right) => _binary(op, left, right),
      IrUnary(:final op, :final operand) => '($op${expr(operand)})',
      IrCall(:final target, :final name, :final args) => _call(target, name, args),
      IrStaticCall(:final owner, :final name, :final args) =>
        '$owner::${snake(name)}(${args.map(expr).join(', ')})',
      IrNew(:final type, :final args, :final constructor) =>
        _new(type, args, constructor),
      IrConditional(:final condition, :final then, :final otherwise) =>
        'if ${expr(condition)} { ${expr(then)} } else { ${expr(otherwise)} }',
      IrIs() => throw Unsupported('`is` in the value subset', '`is` needs the '
          'class hierarchy, which this backend does not model yet'),
      IrSuperCall(:final base, :final name, :final args) =>
        _superCall(base, name, args),
    };
  }

  /// Dart's binary operators in Rust's spelling.
  ///
  /// Most are the same token and pass straight through. The ones that are not
  /// are the reason this is a function and not string interpolation:
  ///
  /// * `~/` is truncating division and has no Rust operator at all. On floats
  ///   it is `(a / b).trunc()`; the `.toDouble()` Dart then needs is dropped
  ///   in `_call`, because the result is already an `f32`.
  /// * `??` takes the left unless it is null.
  ///
  /// An operator not listed and not passed through would be silently wrong, so
  /// anything unrecognised stops.
  String _binary(String op, IrExpr left, IrExpr right) {
    const passthrough = {
      '+', '-', '*', '/', '%', '==', '!=', '<', '>', '<=', '>=',
      '&&', '||', '&', '|', '^', '<<', '>>',
    };
    if (op == '~/') return '((${expr(left)} / ${expr(right)}).trunc())';
    if (op == '??') return '${expr(left)}.unwrap_or(${expr(right)})';
    if (!passthrough.contains(op)) {
      throw Unsupported('binary operator `$op`', '${expr(left)} $op ...');
    }
    return '(${expr(left)} $op ${expr(right)})';
  }

  /// A Dart string's contents, safe to sit inside a Rust `"..."`.
  ///
  /// The backslash has to be doubled *before* the quote is escaped, or the
  /// backslash this step just added would be doubled by the next one. Only
  /// these two characters need it: Rust and Dart agree on the rest.
  String _escape(String text) =>
      text.replaceAll('\\', '\\\\').replaceAll('"', '\\"');

  String _literal(String value, IrType t) {
    if (t.name == 'double') {
      // Rust needs the point: `1` is an integer literal even in an f32 context.
      return value.contains('.') || value.contains('e') ? value : '$value.0';
    }
    // Escaped for the same reason the assert message is: a Dart string holding
    // a quote or a backslash would otherwise end the Rust literal early or
    // start an escape that was never in the source.
    if (t.name == 'String') return '"${_escape(value)}".to_string()';
    if (t.name == 'Null') return 'None';
    return value;
  }

  /// The free function that holds a base class's own body for `name`.
  ///
  /// Rust has no `super`. Once an impl overrides a trait's default method the
  /// default is unreachable -- `Trait::name(self)` dispatches back to the
  /// override and the program hangs. So every concrete method on an abstract
  /// class is emitted twice: once as a free generic function holding the body,
  /// and once as the trait default, which calls it. `super.name(..)` then names
  /// the function, which is the one thing that cannot dispatch anywhere else.
  static String superFn(String base, String name) =>
      '${snake(base)}_super_${snake(name)}';

  String _superCall(String base, String name, List<IrExpr> args) {
    final baseClass = library[base];
    if (baseClass == null) {
      throw Unsupported('super call into `$base`, which is not in this file',
          'super.$name(...)');
    }
    final provides = baseClass.methods.any(
        (m) => m.operator == null && m.name == name && !m.isStatic);
    if (!provides) {
      // The base's own version was refused, or is abstract and has no body to
      // call. Emitting the call anyway would name a function that was never
      // written -- the `_stringify` shape from round one, one level up.
      throw Unsupported('super call to `$base.$name`, which was not translated',
          'super.$name(...)');
    }
    return '${superFn(base, name)}(${[_selfName, ...args.map(expr)].join(', ')})';
  }

  /// The receiver of a field read or a call.
  ///
  /// `this` is two different things in Rust depending on where it stands. As a
  /// *value* it is `*self`, a copy of the struct -- that is what `return this;`
  /// wants. As the *target* of a field or a call it is `self`, because `*self.x`
  /// parses as `*(self.x)` and dereferences the field instead of the receiver.
  ///
  /// Upstream's `copyWith` is where this surfaced: `left ?? this.left` became
  /// `left.unwrap_or(*self.left)`, which does not compile. It was found by
  /// building real upstream code rather than a fixture, which is the argument
  /// for keeping real code in the test crate.
  /// What `self` is called in the code currently being emitted.
  ///
  /// A free function has no `self`, so while one is being written the receiver
  /// is its first parameter instead.
  String _selfName = 'self';

  String _receiver(IrExpr? target) =>
      (target == null || target is IrThis) ? _selfName : expr(target);

  String _call(IrExpr? target, String name, List<IrExpr> args) {
    final receiver = _receiver(target);
    // Dart's `toDouble` on a value already stored as one is a no-op here.
    if (name == 'toDouble' && args.isEmpty) return receiver;
    return '$receiver.${snake(name)}(${args.map(expr).join(', ')})';
  }

  String _new(IrType t, List<IrExpr> args, String? constructor) {
    final name = type(t);
    final ctor = constructor == null ? 'new' : snake(constructor);
    return '$name::$ctor(${args.map(expr).join(', ')})';
  }

  // -- Statements -------------------------------------------------------------

  /// Emits a statement. `tail` marks the position whose value is the block's --
  /// Rust's trailing expression, which is how a `return` at the end of a method
  /// stops needing the keyword.
  void stmt(IrStmt s, {bool tail = false}) {
    switch (s) {
      case IrBlock(:final statements):
        for (var i = 0; i < statements.length; i++) {
          stmt(statements[i], tail: tail && i == statements.length - 1);
        }
      case IrReturn(:final value):
        if (value == null) {
          _line(tail ? '' : 'return;');
        } else {
          _line(tail ? expr(value) : 'return ${expr(value)};');
        }
      case IrLocalDecl(:final name, :final type, :final init):
        final annotation = type == null ? '' : ': ${this.type(type)}';
        _line('let ${snake(name)}$annotation = ${init == null ? "Default::default()" : expr(init)};');
      case IrIf(:final condition, :final then, :final otherwise):
        _line('if ${expr(condition)} {');
        _indent++;
        stmt(then, tail: tail);
        _indent--;
        if (otherwise == null) {
          _line('}');
        } else {
          _line('} else {');
          _indent++;
          stmt(otherwise, tail: tail);
          _indent--;
          _line('}');
        }
      case IrExprStmt(:final expr):
        _line('${this.expr(expr)};');
      case IrAssert(
          :final condition,
          :final literalMessage,
          :final message,
        ):
        // `debug_assert!`, not `assert!`: Dart's assert runs in debug builds
        // and is compiled out of release ones, and so is this. Using `assert!`
        // would keep every one of upstream's checks in a release binary, which
        // is a performance decision this compiler has no business making.
        if (message != null) {
          _line('// assert message, not translated: $message');
        }
        final text =
            literalMessage == null ? '' : ', "${_escape(literalMessage)}"';
        _line('debug_assert!(${expr(condition)}$text);');
    }
  }

  // -- The class --------------------------------------------------------------

  /// Every class in the library, traits first.
  ///
  /// Traits lead because a struct's `impl` mentions them, and a reader who
  /// meets `impl AlignmentGeometry for Alignment` before the trait has to
  /// scroll to find out what was promised.
  /// Returns the source, and what it could not emit.
  ///
  /// Per class, not all-or-nothing. The front end has always collected refusals
  /// member by member; the backend did not, so one class it could not emit
  /// threw away the whole file -- including the classes that were fine. A
  /// compiler that produces nothing because of one bad class is much less
  /// useful than one that produces the rest and says which is missing.
  static (String, List<String>) emitLibrary(IrLibrary library) {
    final out = StringBuffer();
    final refused = <String>[];
    for (final cls in library.classes) {
      try {
        out.write(RustBackend(cls, library: library).emit());
        out.writeln();
      } on Unsupported catch (error) {
        refused.add('${cls.name}: $error');
      }
    }
    return (out.toString(), refused);
  }

  String emit() {
    if (cls.isAbstract) return _emitTrait();
    return _emitStruct();
  }

  /// An abstract class becomes a trait.
  ///
  /// Its abstract members are the trait's requirements and its concrete members
  /// are the trait's defaults, which is exactly the split Dart already made --
  /// a member with a body is inherited, one without must be supplied. Rust
  /// spells that split the same way, so nothing has to be invented here.
  ///
  /// What does *not* come across is the fields: Dart's abstract classes may
  /// declare storage and Rust's traits may not. Any such field is reported
  /// rather than dropped.
  String _emitTrait() {
    _line('// Generated by tools/dart2rust from upstream `${cls.name}`');
    _line('// (abstract -> trait).');
    _line('');
    _doc(cls.doc);
    _line('pub trait ${cls.name} {');
    _indent++;
    for (final method in cls.abstractMethods) {
      _doc(method.doc);
      _line('fn ${_methodName(method)}(${_params(method)}) -> '
          '${type(method.returnType)};');
      _line('');
    }
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      _doc(method.doc);
      _line('fn ${_methodName(method)}(${_params(method)}) -> '
          '${type(method.returnType)} {');
      _indent++;
      // The default delegates to the free function rather than holding the
      // body, so that an override can still reach it. See `superFn`.
      _line('${superFn(cls.name, method.name)}('
          '${['self', ...method.params.map((p) => snake(p.name))].join(', ')})');
      _indent--;
      _line('}');
      _line('');
    }
    _indent--;
    _line('}');
    _emitSuperFns();
    if (cls.fields.isNotEmpty) {
      _line('');
      _line('// NOT TRANSLATED: `${cls.name}` declares '
          '${cls.fields.length} field(s), and a Rust trait holds no storage.');
      for (final field in cls.fields) {
        _line('//   ${field.name}: ${field.type}');
      }
    }
    return _out.join('\n') + '\n';
  }

  /// The bodies of this abstract class's concrete methods, as free functions.
  ///
  /// Generic over the implementor and `?Sized`, so both the trait's own default
  /// and a subclass's override can call it -- the default has an unsized `Self`,
  /// and a subclass has a concrete one.
  void _emitSuperFns() {
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      _line('');
      _line('/// The body of `${cls.name}.${method.name}`, reachable from an');
      _line('/// override the way Dart\'s `super.${method.name}` is.');
      final params = [
        'this_: &S',
        ...method.params.map(
            (p) => '${snake(p.name)}: ${type(p.type, owned: false)}'),
      ].join(', ');
      _line('pub fn ${superFn(cls.name, method.name)}'
          '<S: ${cls.name} + ?Sized>($params) -> '
          '${type(method.returnType)} {');
      _indent++;
      _selfName = 'this_';
      stmt(method.body, tail: true);
      _selfName = 'self';
      _indent--;
      _line('}');
    }
  }

  /// A method's name, with Dart's operators mapped onto Rust's trait methods
  /// where one exists. Inside a trait there is no `impl std::ops::Add` to hang
  /// them on, so they become ordinary named methods.
  String _methodName(IrMethod method) {
    final op = method.operator;
    if (op == null) return snake(method.name);
    final mapping = _operatorTraits[op];
    return mapping == null ? _operatorName(op) : 'op_${mapping.$2}';
  }

  String _params(IrMethod method) => [
        if (!method.isStatic) '&self',
        // A parameter is borrowed, not owned: passing a `Box<dyn Trait>` in
        // would move it, and upstream's callers do not give theirs away.
        ...method.params.map(
            (p) => '${snake(p.name)}: ${type(p.type, owned: false)}'),
      ].join(', ');

  String _emitStruct() {
    _line('// Generated by tools/dart2rust from upstream `${cls.name}`.');
    _line('//');
    _line('// Translated, not ported: this is the compiler\'s output, not a');
    _line('// hand-written re-expression. See tools/dart2rust/README.md.');
    _line('');
    _doc(cls.doc);
    _line('#[derive(Clone, Copy, Debug, PartialEq)]');
    _line('pub struct ${cls.name} {');
    _indent++;
    for (final field in cls.fields) {
      _doc(field.doc);
      _line('pub ${snake(field.name)}: ${type(field.type)},');
    }
    _indent--;
    _line('}');
    _line('');

    _line('impl ${cls.name} {');
    _indent++;
    _emitConstructors();
    _emitConstants();
    _emitMethods();
    _indent--;
    _line('}');
    _emitOperators();
    _emitBaseImpl();
    return _out.join('\n') + '\n';
  }

  /// `impl Base for This`, when this class extends an abstract one.
  ///
  /// The methods **delegate** to the inherent ones rather than repeating their
  /// bodies, and the reason is a real difference between the two languages:
  /// Dart allows a covariant return, so `Alignment operator -()` legally
  /// overrides one declared to return `AlignmentGeometry`. Rust requires the
  /// impl to return exactly what the trait declared. Emitting the body twice
  /// would mean emitting it at two different return types.
  ///
  /// Delegating keeps one body and one idiomatic surface: `Alignment` still has
  /// its `impl Neg` returning an `Alignment`, which is what a Rust caller wants,
  /// and the trait method boxes that up for callers who only know the base.
  void _emitBaseImpl() {
    final base = library[cls.superclass];
    if (base == null || !base.isAbstract) return;
    // Not just the abstract ones. A class that overrides a *concrete* base
    // method needs that override in the impl too, or dynamic dispatch reaches
    // the trait's default instead -- the inherent method would still be right,
    // so only a call through `dyn Base` can tell, which is why the tests make
    // that call.
    final overridden = base.methods
        .where((m) => !m.isStatic && _matching(m) != null)
        .toList();
    final required = [...base.abstractMethods, ...overridden];
    if (required.isEmpty) return;

    _line('');
    _line('impl ${base.name} for ${cls.name} {');
    _indent++;
    for (final need in required) {
      _member('impl ${base.name}::${need.operator ?? need.name} for ${cls.name}',
          () => _emitBaseMethod(need));
    }
    _indent--;
    _line('}');
  }

  void _emitBaseMethod(IrMethod need) {
    {
      final have = _matching(need);
      final returns = type(need.returnType);
      _line('fn ${_methodName(need)}(${_params(need)}) -> $returns {');
      _indent++;
      if (have == null) {
        // Reported in the output rather than silently skipped: a trait impl
        // missing a method does not compile, and the reader should learn why
        // from the file rather than from rustc.
        _line('todo!("${cls.name} does not translate '
            '${need.operator ?? need.name} yet")');
      } else {
        final call = _inherentCall(have);
        final concrete = type(have.returnType);
        _line(concrete == returns ? call : 'Box::new($call)');
      }
      _indent--;
      _line('}');
      _line('');
    }
  }

  /// This class's own version of a method the base requires.
  IrMethod? _matching(IrMethod need) {
    for (final method in cls.methods) {
      if (need.operator != null) {
        if (method.operator == need.operator) return method;
      } else if (method.operator == null && method.name == need.name) {
        return method;
      }
    }
    return null;
  }

  /// How to invoke this class's own version, in Rust's own spelling.
  ///
  /// An operator that became an `impl std::ops::*` is invoked as the operator,
  /// not as a method: that is the whole point of having emitted the trait impl.
  String _inherentCall(IrMethod method) {
    final args = method.params.map((p) => snake(p.name)).toList();
    final op = method.operator;
    if (op != null && _operatorTraits.containsKey(op)) {
      if (op == 'unary-') return '-*self';
      return '*self $op ${args.single}';
    }
    // `Type::method(self, ...)`, not `self.method(...)`. Inside `impl Base for
    // This` the trait's own method has the same name, and `self.method(...)`
    // leans on Rust preferring the inherent one -- true today, and an infinite
    // recursion the moment the inherent one is not emitted. The explicit path
    // says which one is meant.
    final name = op == null ? snake(method.name) : _operatorName(op);
    return '${cls.name}::$name(${['self', ...args].join(', ')})';
  }

  void _emitConstructors() {
    for (final ctor in cls.constructors) {
      _emitConstructor(ctor);
    }
  }

  void _emitConstructor(IrConstructor ctor) {
    // Dart's named constructors are Rust's associated functions already --
    // `EdgeInsets.all(8)` and `EdgeInsets::all(8.0)` are the same call, and the
    // unnamed one is `new` by Rust's convention. Nothing has to be encoded, so
    // nothing is: this is one of the places the two languages simply agree.
    final name = ctor.name == null ? 'new' : snake(ctor.name!);
    _doc(ctor.doc);
    final params = ctor.params
        .map((p) => '${snake(p.name)}: ${type(p.type)}')
        .join(', ');
    // `const fn` because the Dart constructor was `const`, which is what lets
    // the static constants below be associated consts rather than lazy statics.
    // `const fn` even when the constructor carries asserts. An earlier round
    // dropped `const` here, on the assumption that Rust would not accept a
    // `const fn` that could panic. That assumption was wrong -- const panic has
    // been stable since 1.57, `debug_assert!` inside a `const fn` compiles, and
    // the check still fires at runtime. Both were available all along.
    //
    // It mattered: `TextAlignVertical` has asserts in its constructor and
    // `static const` fields built from it, and dropping `const` made those
    // fields uncompilable. The two rounds' rules only met on real code.
    _line('pub ${ctor.isConst ? "const " : ""}fn $name($params) -> Self {');
    _indent++;
    for (final check in ctor.asserts) {
      stmt(check);
    }
    _line('Self {');
    _indent++;
    for (final field in cls.fields) {
      final init = ctor.fieldInits[field.name];
      if (init == null) {
        throw Unsupported('field never initialised', field.name);
      }
      _line('${snake(field.name)}: ${expr(init)},');
    }
    _indent--;
    _line('}');
    _indent--;
    _line('}');
    _line('');
  }

  void _emitConstants() {
    for (final constant in cls.constants) {
      _doc(constant.doc);
      _line('pub const ${screamingSnake(constant.name)}: ${type(constant.type)} '
          '= ${expr(constant.value)};');
    }
    if (cls.constants.isNotEmpty) _line('');
  }

  void _emitMethods() {
    for (final method in cls.methods) {
      if (method.operator != null) continue;
      _member('${cls.name}.${method.name}', () => _emitMethod(method));
    }
  }

  void _emitMethod(IrMethod method) {
    {
      _doc(method.doc);
      final params = [
        if (!method.isStatic) '&self',
        ...method.params.map((p) => '${snake(p.name)}: ${type(p.type)}'),
      ].join(', ');
      _line('pub fn ${snake(method.name)}($params) -> ${type(method.returnType)} {');
      _indent++;
      stmt(method.body, tail: true);
      _indent--;
      _line('}');
      _line('');
    }
  }

  void _emitOperators() {
    for (final method in cls.methods) {
      final op = method.operator;
      if (op == null) continue;
      _member('${cls.name} operator $op', () => _emitOperator(method, op));
    }
  }

  void _emitOperator(IrMethod method, String op) {
    {
      final mapping = _operatorTraits[op];
      if (mapping == null) {
        // `~/` has no Rust trait. Emitted as an inherent method rather than
        // forced into one that means something else.
        _line('');
        _line('impl ${cls.name} {');
        _indent++;
        _doc(method.doc);
        final params = [
          '&self',
          ...method.params.map((p) => '${snake(p.name)}: ${type(p.type)}'),
        ].join(', ');
        _line('pub fn ${_operatorName(op)}($params) -> ${type(method.returnType)} {');
        _indent++;
        stmt(method.body, tail: true);
        _indent--;
        _line('}');
        _indent--;
        _line('}');
        return;
      }
      final (trait, fn) = mapping;
      final rhs = method.params.isEmpty ? null : method.params.single;
      _line('');
      _doc(method.doc);
      final generic = rhs == null ? '' : '<${type(rhs.type)}>';
      _line('impl std::ops::$trait$generic for ${cls.name} {');
      _indent++;
      _line('type Output = ${type(method.returnType)};');
      _line('');
      final params = [
        'self',
        if (rhs != null) '${snake(rhs.name)}: ${type(rhs.type)}',
      ].join(', ');
      _line('fn $fn($params) -> Self::Output {');
      _indent++;
      stmt(method.body, tail: true);
      _indent--;
      _line('}');
      _indent--;
      _line('}');
    }
  }


  String _operatorName(String op) => switch (op) {
        '~/' => 'int_div',
        '[]' => 'index_of',
        '<' => 'lt',
        '>' => 'gt',
        '<=' => 'le',
        '>=' => 'ge',
        _ => 'op_${op.codeUnits.join("_")}',
      };
}
