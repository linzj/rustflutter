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
  RustBackend(this.cls);

  final IrClass cls;
  final _out = StringBuffer();
  int _indent = 0;

  void _line(String text) {
    if (text.isEmpty) {
      _out.writeln();
    } else {
      _out.writeln('${'    ' * _indent}$text');
    }
  }

  void _doc(String? doc, {String prefix = '///'}) {
    if (doc == null || doc.isEmpty) return;
    for (final line in doc.split('\n')) {
      _line(line.isEmpty ? prefix : '$prefix ${line.trim()}');
    }
  }

  String type(IrType t) {
    final mapped = _primitives[t.name] ?? t.name;
    return t.nullable ? 'Option<$mapped>' : mapped;
  }

  // -- Expressions ------------------------------------------------------------

  String expr(IrExpr e) {
    return switch (e) {
      IrLiteral(:final value, :final type) => _literal(value, type),
      IrLocal(:final name) => snake(name),
      IrThis() => '*self',
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
  String _receiver(IrExpr? target) =>
      (target == null || target is IrThis) ? 'self' : expr(target);

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

  String emit() {
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
    return _out.toString();
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
    // A constructor carrying asserts cannot stay `const fn`: `debug_assert!`
    // expands to a branch on `cfg!(debug_assertions)`, and a `const fn` that
    // could panic is only usable in a const context if the check passes at
    // compile time -- which is exactly the constraint Dart's const constructors
    // have, but Rust will not let the *definition* through. Dropping `const`
    // keeps the check; keeping `const` would mean dropping the check.
    final constable = ctor.isConst && ctor.asserts.isEmpty;
    _line('pub ${constable ? "const " : ""}fn $name($params) -> Self {');
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
        continue;
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
