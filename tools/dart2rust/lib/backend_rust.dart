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
  final out = name
      .replaceAllMapped(RegExp(r'(?<!^)([A-Z])'), (m) => '_${m[1]}')
      .toLowerCase();
  return out;
}

String screamingSnake(String name) => snake(name).toUpperCase();

/// `spaceBetween` -> `SpaceBetween`: an enum variant as Rust spells it.
///
/// Only the first letter changes. Rewriting the rest would make the output
/// impossible to search against upstream, which is the same reason private
/// members keep their leading underscore.
String variantName(String name) =>
    name.isEmpty ? name : name[0].toUpperCase() + name.substring(1);

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
  /// Returns whether the member was emitted, because a caller sometimes has to
  /// know: a trait default cannot delegate to a free function that was refused.
  bool _member(String what, void Function() body) {
    final mark = _out.length;
    final indent = _indent;
    try {
      body();
      return true;
    } on Unsupported catch (error) {
      _out.removeRange(mark, _out.length);
      _indent = indent;
      _line('// NOT TRANSLATED: $what');
      _line('//   $error');
      _line('');
      return false;
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
  /// `pub `, or nothing when the Dart name was private.
  ///
  /// Dart's privacy is per *library* and Rust's is per *module*, so a file's
  /// classes emitted into one module keep the same reachability: a `_`-prefixed
  /// member is visible to its neighbours and to nobody else. The name is left
  /// alone -- `_x` is a legal Rust identifier -- because changing it would make
  /// the output unsearchable against upstream.
  String _vis(String dartName) => dartName.startsWith('_') ? '' : 'pub ';

  String type(IrType t, {bool owned = true}) {
    if (t.isFunction) {
      // A parameter takes `impl Fn(..)`, which needs no allocation and lets the
      // caller pass a closure literal; anything owned -- a field, a return --
      // has to be `Box<dyn Fn(..)>`, since a closure's own type has no name.
      final args = t.parameters!.map((p) => type(p, owned: false)).join(', ');
      final returns = type(t.returns!);
      final signature = 'Fn($args) -> $returns';
      final spelled = owned ? 'Box<dyn $signature>' : 'impl $signature';
      return t.nullable ? 'Option<$spelled>' : spelled;
    }
    if (library.isAbstract(t.name)) {
      final dynamic_ = owned ? 'Box<dyn ${t.name}>' : '&dyn ${t.name}';
      return t.nullable ? 'Option<$dynamic_>' : dynamic_;
    }
    if (t.name == 'Map' && t.arguments.length == 2) {
      final map =
          'std::collections::HashMap<${type(t.arguments[0])}, '
          '${type(t.arguments[1])}>';
      return t.nullable ? 'Option<$map>' : map;
    }
    if ((t.name == 'List' || t.name == 'Iterable') && t.arguments.length == 1) {
      final vec = 'Vec<${type(t.arguments.single)}>';
      return t.nullable ? 'Option<$vec>' : vec;
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
      IrField(:final target, :final name) => _fieldRead(target, name),
      IrStatic(:final owner, :final name, :final isEnumValue) =>
        '$owner::${isEnumValue ? variantName(name) : screamingSnake(name)}',
      IrBinary(:final op, :final left, :final right, :final type) => _binary(
        op,
        left,
        right,
        type,
      ),
      IrUnary(:final op, :final operand) => '($op${expr(operand)})',
      IrCall(:final target, :final name, :final args) => _call(
        target,
        name,
        args,
      ),
      IrStaticCall(:final owner, :final name, :final args) => _staticCall(
        owner,
        name,
        args,
      ),
      IrNew(:final type, :final args, :final constructor) => _new(
        type,
        args,
        constructor,
      ),
      IrConstInstance(:final type, :final fields) => _constInstance(
        type,
        fields,
      ),
      IrIdentical(:final left, :final right) => _identical(left, right),
      // `return Err(e)` has type `!`, so it fits where a value was wanted.
      IrThrowValue(:final value) => 'return Err(${expr(value)})',
      IrInterpolation(:final parts) => _interpolation(parts),
      // Dart indexes with an `int`; Rust wants a `usize`.
      IrIndex(:final target, :final index) =>
        '${expr(target)}[${expr(index)} as usize]',
      IrListLiteral(:final elements) =>
        'vec![${elements.map(expr).join(', ')}]',
      IrMapLiteral(:final entries) =>
        'std::collections::HashMap::from(['
            '${entries.map((e) => '(${expr(e.$1)}, ${expr(e.$2)})').join(', ')}'
            '])',
      IrIterChain() => throw Unsupported(
        'a lazy Iterable that is never collected',
        'xs.map(..) with no toList()',
      ),
      // Boxed, because a function item is not a `Box<dyn Fn>` and that is what
      // a function-typed field or local is here. A `Box<dyn Fn>` also
      // implements `Fn`, so it still passes where `impl Fn` is wanted.
      IrFunctionRef(:final owner, :final name) =>
        owner == null
            ? 'Box::new(${snake(name)})'
            : 'Box::new($owner::${snake(name)})',
      IrAssignValue(:final name, :final value) =>
        '{ let __set = ${expr(value)}; ${snake(name)} = __set; __set }',
      IrSetValue(:final target, :final name, :final value) => _setValue(
        target,
        name,
        value,
      ),
      IrConditional(:final condition, :final then, :final otherwise) =>
        'if ${expr(condition)} { ${expr(then)} } else { ${expr(otherwise)} }',
      IrIs() => throw Unsupported(
        '`is` in the value subset',
        '`is` needs the '
            'class hierarchy, which this backend does not model yet',
      ),
      IrSuperCall(:final base, :final name, :final args) => _superCall(
        base,
        name,
        args,
      ),
      IrNullCheck(:final operand) => '${expr(operand)}.unwrap()',
      IrTopLevel(:final name) => screamingSnake(name),
      IrIsNull(:final operand) => '${expr(operand)}.is_none()',
      IrIfNull() => _ifNull(e as IrIfNull),
      IrNullAware(:final receiver, :final body) =>
        '${expr(receiver)}.map(|$_boundName| ${expr(body)})',
      IrBound() => _boundName,
      IrClosure() => _closure(e as IrClosure),
      IrCallValue(:final target, :final args) =>
        '(${expr(target)})(${args.map(expr).join(', ')})',
      IrBlockValue() => _blockValue(e as IrBlockValue),
    };
  }

  /// Statements then a value, as a Rust block expression.
  ///
  /// The binding is `mut` only when a step writes to it, for the reason
  /// `let mut` is not applied everywhere: the test crate denies `unused_mut`,
  /// so an unneeded one is a build error rather than a warning nobody reads.
  String _blockValue(IrBlockValue node) {
    final saved = _out.length;
    final savedIndent = _indent;
    final savedReassigned = _reassigned;
    _indent = 0;
    // A cascade's steps write fields of the binding, which is a write to the
    // local rather than a reassignment of it -- so `_assignedIn` does not see
    // it and the declaration has to be told separately.
    _reassigned = {
      ..._reassigned,
      if (node.statements.any(_writesTheBinding)) _cascadeBinding,
    };
    for (final statement in node.statements) {
      stmt(statement);
    }
    final body = _out.sublist(saved).map((l) => l.trim()).join(' ');
    _out.removeRange(saved, _out.length);
    _indent = savedIndent;
    _reassigned = savedReassigned;
    return '{ $body ${expr(node.value)} }';
  }

  /// The name the front ends give a cascade's receiver.
  static const _cascadeBinding = 'cascaded';

  bool _writesTheBinding(IrStmt s) => switch (s) {
    IrAssignField(:final target) =>
      target is IrLocal && target.name == _cascadeBinding,
    IrSetter(:final target) =>
      target is IrLocal && target.name == _cascadeBinding,
    _ => false,
  };

  /// A closure literal.
  ///
  /// The parameter types are written out rather than inferred: a closure passed
  /// straight into a call would usually infer, but one stored or returned would
  /// not, and a compiler that emits both spellings depending on where the
  /// closure lands is two rules where one will do.
  String _closure(IrClosure node) {
    final params = node.params
        .map((p) => '${snake(p.name)}: ${type(p.type)}')
        .join(', ');
    final saved = _out.length;
    final savedIndent = _indent;
    _indent = 0;
    stmt(node.body, tail: true);
    final body = _out.sublist(saved).map((l) => l.trim()).join(' ');
    _out.removeRange(saved, _out.length);
    _indent = savedIndent;
    return '|$params| { $body }';
  }

  /// The closure parameter a `?.` binds.
  ///
  /// One fixed name, not a fresh one per nesting level: a chained `a?.b?.c`
  /// nests the closures, and the inner one shadows the outer -- which is what
  /// the Dart means, since the inner access is about the inner value.
  static const _boundName = 'it';

  /// `a ?? b`, in the one of four spellings Rust needs.
  ///
  /// Two questions decide it, and both come from the front end because the IR
  /// carries no expression types:
  ///
  /// * **Is the result still nullable?** `a ?? b` is non-null exactly when `b`
  ///   is. `unwrap_or_else` produces a value, `or_else` produces an Option, and
  ///   using the wrong one does not type-check -- which is how nested `??`
  ///   found this, since `a ?? b ?? c` has a nullable `a ?? b` inside it.
  /// * **May the right side be evaluated eagerly?** Dart's `??` is
  ///   short-circuit and Rust's `unwrap_or`/`or` are not. Only a literal is
  ///   safe; 77% of upstream's right-hand sides are calls, constructors or
  ///   throws.
  String _ifNull(IrIfNull node) {
    final left = expr(node.left);
    if (node.right is IrThrowValue) {
      // `a ?? throw e`. The closure forms are wrong here for the reason a try
      // body could not hold a `?`: the `return Err(e)` inside `unwrap_or_else`
      // would return from the *closure*. A match has no closure to escape
      // from, and the arm that throws simply diverges.
      return 'match $left { '
          'Some(__value) => __value, '
          'None => ${expr(node.right)} }';
    }
    final right = expr(node.right);
    if (node.nullableResult) {
      return node.eager ? '$left.or($right)' : '$left.or_else(|| $right)';
    }
    return node.eager
        ? '$left.unwrap_or($right)'
        : '$left.unwrap_or_else(|| $right)';
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
  String _binary(String op, IrExpr left, IrExpr right, [IrType? type]) {
    if (op == '+' && type?.name == 'String') {
      // `String + String` is not Rust. `format!` is, it needs no borrow worked
      // out at either end, and it is what Dart's `+` on two strings means.
      return 'format!("{}{}", ${expr(left)}, ${expr(right)})';
    }
    const passthrough = {
      '+',
      '-',
      '*',
      '/',
      '%',
      '==',
      '!=',
      '<',
      '>',
      '<=',
      '>=',
      '&&',
      '||',
      '&',
      '|',
      '^',
      '<<',
      '>>',
    };
    if (op == '~/') return '((${expr(left)} / ${expr(right)}).trunc())';
    if (op == '??') {
      // Dart's `??` is short-circuit: the right side is evaluated only when the
      // left is null. Rust's `unwrap_or` evaluates it **always**, so it is right
      // only for a value that has no effects and costs nothing -- and this used
      // `unwrap_or` for everything from round two until the corpus was counted.
      //
      // Of 6764 `??` in package:flutter only 23% have a literal or constant on
      // the right. The rest are calls, constructors, and in six places a
      // `throw` -- where eager evaluation does not give a wrong answer, it
      // throws unconditionally.
      //
      // A literal keeps the shorter form because it reads better and is
      // provably safe; everything else defers.
      if (right is IrLiteral) {
        return '${expr(left)}.unwrap_or(${expr(right)})';
      }
      return '${expr(left)}.unwrap_or_else(|| ${expr(right)})';
    }
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
      '${snake(base)}_super_${_identifier(name)}';

  /// A static call, checked against the IR when it lands in this library.
  ///
  /// `Alignment._stringify(x, y)` was emitted for a method the front end had
  /// refused, so the output named a function nobody wrote. That is round one's
  /// bug in a new shape: it was masked then by refusing every private reference,
  /// and removing that blunt rule brought it back. The precise rule is the same
  /// one `_superCall` uses -- if the callee is in this file, it has to be in the
  /// IR.
  String _staticCall(String owner, String name, List<IrExpr> args) {
    final target = library[owner];
    if (target != null &&
        !target.methods.any((m) => m.name == name && m.operator == null)) {
      throw Unsupported(
        'call to `$owner.$name`, which was not translated',
        '$owner.$name(...)',
      );
    }
    return '$owner::${_identifier(name)}(${args.map(expr).join(', ')})';
  }

  String _superCall(String base, String name, List<IrExpr> args) {
    final baseClass = library[base];
    if (baseClass == null) {
      throw Unsupported(
        'super call into `$base`, which is not in this file',
        'super.$name(...)',
      );
    }
    final provides = baseClass.methods.any(
      (m) => m.operator == null && m.name == name && !m.isStatic,
    );
    if (!provides) {
      // The base's own version was refused, or is abstract and has no body to
      // call. Emitting the call anyway would name a function that was never
      // written -- the `_stringify` shape from round one, one level up.
      throw Unsupported(
        'super call to `$base.$name`, which was not translated',
        'super.$name(...)',
      );
    }
    if (!_superFnEmits(baseClass, name)) {
      // The base *has* the method, and the free function holding its body still
      // could not be emitted -- so the name this call would use is not written
      // anywhere. `Alignment.toString` called `alignment_geometry_super_to_-
      // string` for exactly this reason, and the Kernel side of the library did
      // not build for two rounds while `agree.py` was recorded as green.
      //
      // The question is answered by emitting the function and seeing, rather
      // than by a second rule about when it works: a second rule is a thing
      // that can disagree with the first one.
      throw Unsupported(
        'super call to `$base.$name`, whose body did not translate',
        'super.$name(...)',
      );
    }
    return '${superFn(base, name)}(${[_selfName, ...args.map(expr)].join(', ')})';
  }

  /// Whether `base`'s free function for [name] can actually be emitted.
  ///
  /// `_superFailed` answers this for the class being emitted, but a super call
  /// is made from the *subclass*, whose backend never sees the base's set.
  static final _superFnProbes = <String, bool>{};

  bool _superFnEmits(IrClass baseClass, String name) {
    final key = '${baseClass.name}.$name';
    final known = _superFnProbes[key];
    if (known != null) return known;
    final method = baseClass.methods.firstWhere(
      (m) => m.operator == null && m.name == name && !m.isStatic,
    );
    final probe = RustBackend(baseClass, library: library);
    final ok = probe._member(key, () => probe._emitSuperFn(method));
    return _superFnProbes[key] = ok;
  }

  /// Whether a field of *this* class is reachable as a field right now.
  ///
  /// Inside a trait it is not. The class's fields were flattened into every
  /// implementor, so the trait -- and the free functions holding its method
  /// bodies -- can only reach them through an accessor the trait requires.
  /// Reading them as fields gives "no field `width` on type `&S`".
  var _fieldsAreAccessors = false;

  String _fieldRead(IrExpr? target, String name) {
    final receiver = _receiver(target);
    if (_fieldsAreAccessors &&
        (target == null || target is IrThis) &&
        cls.fields.any((f) => f.name == name)) {
      return '$receiver.${snake(name)}()';
    }
    return '$receiver.${snake(name)}';
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
  /// The return type of the function currently being emitted.
  ///
  /// Needed for one thing Dart does implicitly and Rust does not: returning a
  /// concrete value where an abstract type is declared.
  /// `AlignmentGeometry.add` ends in `_MixedAlignment(...)` and is declared to
  /// return `AlignmentGeometry`, which in Rust is `Box<dyn AlignmentGeometry>`.
  /// That is the same coercion the trait impls needed at their boundary, met
  /// again inside a body.
  IrType? _returns;

  /// Wraps a returned expression when the declared return is a trait object.
  ///
  /// Only an `IrNew` is wrapped, because only a constructor call is *known* to
  /// produce that concrete type. Anything else could already be a box, and a
  /// double `Box::new` compiles into something quietly wrong.
  String _returned(IrExpr value) {
    final declared = _returns;
    final text = expr(value);
    if (declared != null &&
        library.isAbstract(declared.name) &&
        (value is IrNew || value is IrConstInstance) &&
        !library.isAbstract(_concreteType(value).name)) {
      return 'Box::new($text)';
    }
    return text;
  }

  IrType _concreteType(IrExpr e) => switch (e) {
    IrNew(:final type) => type,
    IrConstInstance(:final type) => type,
    _ => const IrType('void'),
  };

  /// What `self` is called in the code currently being emitted.
  ///
  /// A free function has no `self`, so while one is being written the receiver
  /// is its first parameter instead.
  String _selfName = 'self';

  String _receiver(IrExpr? target) =>
      (target == null || target is IrThis) ? _selfName : expr(target);

  String _call(IrExpr? target, String name, List<IrExpr> args) {
    // Before the receiver is rendered: rendering a chain on its own is
    // refused, and this is the one place a chain is not on its own.
    // Any arguments, not none: Dart's `toList({bool growable = true})` has a
    // named parameter, and the Kernel front end fills in its default -- so the
    // chain was collected on one side and refused on the other.
    if (name == 'to_list' && target is IrIterChain) {
      return '${_chain(target)}.collect::<Vec<_>>()';
    }
    final receiver = _receiver(target);
    // `HashMap` looks up by reference, and gives back a reference to the
    // value. Dart's `m[k]` is a `V?`, so the borrow is cloned away rather
    // than leaked into every caller's type.
    if (name == 'get' && args.length == 1) {
      return '$receiver.get(&${expr(args.single)}).cloned()';
    }
    if ((name == 'contains_key' || name == 'remove') && args.length == 1) {
      return '$receiver.$name(&${expr(args.single)})';
    }
    // The three List members Rust says differently rather than renames.
    if (name == '!is_empty' && args.isEmpty) return '!$receiver.is_empty()';
    if (name == 'first' && args.isEmpty) return '$receiver[0]';
    if (name == 'last' && args.isEmpty) {
      return '$receiver[$receiver.len() - 1]';
    }
    // Dart's `toList` on a list copies it, which is `clone`.
    if (name == 'to_list') return '$receiver.clone()';
    // `Vec::len` gives a `usize` and Dart's `length` an `int`. Without the
    // cast every comparison against a loop counter fails to compile.
    if (name == 'len' && args.isEmpty) return '($receiver.len() as i64)';
    // Dart's `toDouble`. This used to return the receiver unchanged, on the
    // reasoning that a value already stored as a double needs nothing -- true,
    // and it is not the only receiver `toDouble` has. `total + i.toDouble()`
    // with an `int` i came out as `total + i`, which does not compile in Rust
    // and does in Dart. `as f32` is right for both: on an f32 it is the no-op
    // the old rule assumed.
    if (name == 'toDouble' && args.isEmpty) return '($receiver as f32)';
    // A call to a method of this class that can fail carries the failure
    // outward with `?`. That is the propagation the measurement counted, and
    // the caller's own signature was widened by the same fixpoint, so the two
    // always agree.
    final suffix =
        (target == null || target is IrThis) &&
            _failing.containsKey(snake(name))
        ? '?'
        : '';
    return '$receiver.${snake(name)}(${args.map(expr).join(', ')})$suffix';
  }

  /// `Alignment { x: -1.0, y: -1.0 }`.
  ///
  /// Only for a class this file emits. The struct literal names fields, and the
  /// only fields whose Rust names are known are the ones written here -- a
  /// `Duration { _duration: 1000 }` would be naming a field of a hand-written
  /// stub and would go wrong quietly the day the stub was spelled differently.
  String _constInstance(IrType t, Map<String, IrExpr> fields) {
    final cls = library[t.name];
    if (cls == null) {
      throw Unsupported(
        'const instance of `${t.name}`, which is not in this file',
        'const ${t.name}(..)',
      );
    }
    final wanted = _allFields(cls).map((f) => f.name).toList();
    final missing = wanted.where((f) => !fields.containsKey(f)).toList();
    final extra = fields.keys.where((f) => !wanted.contains(f)).toList();
    if (missing.isNotEmpty || extra.isNotEmpty) {
      // The constant and the struct disagree about what the class holds. That
      // is a fact about this compiler, not about the program, so it is said
      // plainly rather than patched over with a default.
      throw Unsupported(
        'const instance of `${t.name}`: the struct '
            '${missing.isEmpty ? "has no" : "wants"} '
            '${missing.isEmpty ? extra.join(", ") : missing.join(", ")}',
        'const ${t.name}(..)',
      );
    }
    final parts = [
      for (final field in wanted) '${snake(field)}: ${expr(fields[field]!)}',
    ];
    return '${t.name} { ${parts.join(', ')} }';
  }

  /// A constructor's Rust name. One function, used by both the definition and
  /// the call, because two spellings of the same rule is how a call ends up
  /// naming a function nobody wrote.
  ///
  /// Dart's `Foo._()` -- the private default constructor, and a common idiom --
  /// snakes to `_`, which Rust reserves. It becomes `new_`: still recognisable
  /// as the constructor, and a name Rust will take.
  static String _ctorName(String? dartName) {
    if (dartName == null) return 'new';
    final name = snake(dartName);
    return name == '_' ? 'new_' : name;
  }

  /// `a.b = v` where the value of the assignment is wanted.
  ///
  /// Rust's assignment produces `()`, so the value is bound first and produced
  /// after -- not re-read from the field, which would be a second read of
  /// something a setter or another thread could have changed.
  String _setValue(IrExpr? target, String name, IrExpr value) {
    final receiver = target == null ? _selfName : expr(target);
    return '{ let __set = ${expr(value)}; '
        '$receiver.${snake(name)} = __set; __set }';
  }

  /// `'a \$b c'` as `format!`.
  ///
  /// The literal pieces become the format string and the rest its arguments.
  /// A literal's own braces are doubled, since `format!` reads them.
  String _interpolation(List<IrExpr> parts) {
    final pattern = StringBuffer();
    final args = <String>[];
    for (final part in parts) {
      if (part is IrLiteral && part.type.name == 'String') {
        pattern.write(part.value.replaceAll('{', '{{').replaceAll('}', '}}'));
        continue;
      }
      pattern.write('{}');
      args.add(expr(part));
    }
    // A backslash first, then a quote: doing it the other way round would
    // escape the backslash this line just added.
    final text = pattern
        .toString()
        .replaceAll(r'\', r'\\')
        .replaceAll('"', r'\"');
    return args.isEmpty
        ? '"$text".to_string()'
        : 'format!("$text", ${args.join(', ')})';
  }

  /// The iterator part of a chain, without the collect that ends it.
  String _chain(IrIterChain chain) {
    final steps = chain.steps
        .map((step) => '.${step.$1}(${_stepClosure(step.$2)})')
        .join();
    return '${expr(chain.source)}.iter()$steps';
  }

  /// A chain step's closure, without its parameter types.
  ///
  /// `iter()` yields references, so the Dart type is the wrong annotation --
  /// `|m: i64|` against a `&i64` does not compile. Left off, Rust infers it,
  /// and the body reads the same either way.
  String _stepClosure(IrExpr e) {
    if (e is! IrClosure) return expr(e);
    final params = e.params.map((p) => snake(p.name)).join(', ');
    final saved = _out.length;
    final savedIndent = _indent;
    _indent = 0;
    stmt(e.body, tail: true);
    final body = _out.sublist(saved).map((l) => l.trim()).join(' ');
    _out.removeRange(saved, _out.length);
    _indent = savedIndent;
    return '|$params| { $body }';
  }

  /// `identical(a, b)`.
  ///
  /// Only with `this` on one side. That is the `operator ==` fast path -- 140
  /// of upstream's 259 -- and there both sides really are references, so
  /// `std::ptr::eq` asks the question Dart asked. Between two locals it would
  /// not: a translated value type is a `Copy` struct, and two copies of the
  /// same value sit at different addresses while two names for one value may
  /// sit at the same one. Answering that with an address is worse than not
  /// answering.
  String _identical(IrExpr left, IrExpr right) {
    if (!_isReference(left) || !_isReference(right)) {
      // The question is not "is one side `this`" -- it is whether both sides
      // are *references* in the emitted Rust. A parameter of a concrete type
      // arrives by value, because a translated value type is `Copy`, and the
      // address of a copy answers nothing: `identical(this, other)` there would
      // compile and always be false.
      throw Unsupported(
        '`identical` on something that is not a reference',
        'identical(.., ..)',
      );
    }
    // Through `*const ()` because the two sides have different Rust types --
    // `&Self` and `&dyn Trait` -- and identity is about the address, which both
    // of them have.
    return 'std::ptr::eq('
        '${_ref(left)} as *const _ as *const (), '
        '${_ref(right)} as *const _ as *const ())';
  }

  /// Whether this expression is a reference in the emitted Rust.
  ///
  /// `self` always is. A local is one when it is a parameter whose Dart type is
  /// an abstract class, since that becomes `&dyn Trait` -- which is what
  /// upstream's `operator ==(Object other)` is.
  bool _isReference(IrExpr e) =>
      e is IrThis || (e is IrLocal && _referenceParams.contains(e.name));

  /// Parameters of the method being emitted that are references.
  var _referenceParams = <String>{};

  /// `self` is already a reference; anything else names one.
  String _ref(IrExpr e) => e is IrThis ? _selfName : expr(e);

  String _new(IrType t, List<IrExpr> args, String? constructor) {
    final name = type(t);
    final ctor = _ctorName(constructor);
    return '$name::$ctor(${args.map(expr).join(', ')})';
  }

  // -- Statements -------------------------------------------------------------

  /// Locals assigned somewhere in the body currently being emitted.
  ///
  /// Rust needs `let mut` at the *declaration*, and whether one is needed is a
  /// fact about the whole body, not about the line. So the body is walked once
  /// before it is emitted. Marking every local `mut` would compile too, and
  /// would bury the ones that really are reassigned under a warning apiece.
  var _reassigned = <String>{};

  Set<String> _assignedIn(IrStmt statement) {
    final found = <String>{};
    // An assignment can also be an *expression* -- `f(total = x)` -- and the
    // local it writes needs `mut` just the same. Walking only statements left
    // one immutable and the file did not compile.
    final inExpressions = _WalkSelf();
    inExpressions.statement(statement);
    found.addAll(inExpressions.assignedLocals);
    void walk(IrStmt s) {
      switch (s) {
        case IrAssign(:final name):
          found.add(name);
        case IrBlock(:final statements):
          statements.forEach(walk);
        case IrIf(:final then, :final otherwise):
          walk(then);
          if (otherwise != null) walk(otherwise);
        case IrTryCatch(:final body, :final handler):
          // Walked into, not skipped: a local assigned inside a `try` still
          // needs `mut` at its declaration outside it.
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
        case IrLocalFunction():
        case IrIndexSet():
        case IrBreak():
        case IrContinue():
        case IrReturn():
        case IrLocalDecl():
        case IrExprStmt():
        case IrAssert():
        case IrSetter():
        case IrThrow():
        case IrAssignField():
      }
    }

    walk(statement);
    return found;
  }

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
        // In a failing method every ordinary return is a success: Rust needs
        // the `Ok`, and leaving it off is a type error rather than a quiet
        // wrong answer, which is the one comfort here.
        final wrap = _failure != null;
        final returned = value == null
            ? (wrap ? 'Ok(())' : '')
            : (wrap ? 'Ok(${_returned(value)})' : _returned(value));
        if (_inFlowClosure) {
          // Inside the try closure this is not a return from the method yet --
          // it is a value handed to the `match` outside, which does the real
          // returning. `tail` does not apply: the closure's own tail is the
          // `Ok(None)` that says the body fell off the end.
          _line('return Ok(Some(${returned.isEmpty ? '()' : returned}));');
        } else {
          _line(tail ? returned : 'return $returned;');
        }
      case IrThrow(:final value):
        _line('return Err(${expr(value)});');
      case IrTryFinally(:final body, :final finalizer):
        // The finalizer has to run on the way out however the body leaves, so
        // the body's exits are all collected into one value first and only
        // dispatched after it has run. `Drop` is the usual Rust answer and is
        // the wrong one here: a guard's `drop` cannot use `?` or `return`, and
        // the finalizer often does neither but the *dispatch* does both.
        //
        // Nothing here catches: an `Err` is handed straight back on. A
        // `try/catch/finally` is a TryCatch inside this node, so the catching
        // has already happened by the time the value gets here.
        final flows = _returnsEarly(body);
        final carried = flows ? 'Option<${_rustReturns ?? '()'}>' : '()';
        final failure =
            _errorIn(body) ?? _failure ?? 'std::convert::Infallible';
        _line('let __finally = (|| -> Result<$carried, $failure> {');
        _indent++;
        final wasFlowing = _inFlowClosure;
        _inFlowClosure = flows;
        stmt(body);
        _inFlowClosure = wasFlowing;
        _line('#[allow(unreachable_code)]');
        _line(flows ? 'Ok(None)' : 'Ok(())');
        _indent--;
        _line('})();');
        stmt(finalizer);
        _line('match __finally {');
        _indent++;
        if (flows) {
          _line('Ok(Some(__returned)) => return __returned,');
          _line(
            _alwaysReturns(body)
                ? "Ok(None) => unreachable!(\"the try body always returns\"),"
                : 'Ok(None) => {}',
          );
        } else {
          _line('Ok(()) => {}');
        }
        // The failure keeps going. `_failing` already put `Result` on this
        // method's signature, because a `finally` catches nothing and so the
        // walk that spreads failure never stopped at it.
        _line('Err(__failed) => return Err(__failed),');
        _indent--;
        _line('}');
      case IrTryCatch(
        :final body,
        :final error,
        :final errorType,
        :final handler,
      ):
        // The body goes inside an immediately-invoked closure, and that is the
        // load-bearing part: a failing call inside it is spelled `?`, and `?`
        // returns from the function it is written in. In a closure it returns
        // from the closure -- which is what `try` means -- and written inline
        // it would return from the enclosing method, escaping the very `catch`
        // that was supposed to stop it.
        // The closure's error type comes from the try *body*, not from the
        // enclosing method: a method that catches does not fail, so it has no
        // error type of its own, and `Result<(), _>` cannot be inferred.
        final failure = errorType ?? _errorIn(body) ?? _failure ?? '_';
        // The closure catches `?`, and it would catch a `return` too: written
        // plainly, `return x` in the body returns from the *closure* and the
        // method carries on, which compiles and is wrong. So when the body
        // returns, the closure carries the control flow out as a value --
        // `Some(x)` for "the body returned x", `None` for "it fell off the
        // end" -- and the match below does the returning for real.
        final flows = _returnsEarly(body);
        final carried = flows ? 'Option<${_rustReturns ?? '()'}>' : '()';
        _line('match (|| -> Result<$carried, $failure> {');
        _indent++;
        final outer = _inFlowClosure;
        _inFlowClosure = flows;
        stmt(body);
        _inFlowClosure = outer;
        final always = flows && _alwaysReturns(body);
        if (flows) {
          // A body whose every path returns never reaches this, and Rust says
          // so; the line is still needed for the bodies where some path does
          // not.
          _line('#[allow(unreachable_code)]');
          _line('Ok(None)');
        } else {
          _line('Ok(())');
        }
        _indent--;
        _line('})() {');
        _indent++;
        if (flows) {
          _line('Ok(Some(__returned)) => return __returned,');
          // `{}` has type `()`, and when every path through the body returns
          // there is nothing after the match to give the method its value --
          // so the arm has to say it cannot happen rather than fall through.
          _line(
            always
                ? "Ok(None) => unreachable!(\"the try body always returns\"),"
                : 'Ok(None) => {}',
          );
        } else {
          _line('Ok(()) => {}');
        }
        _line('Err(${snake(error)}) => {');
        _indent++;
        stmt(handler);
        _indent--;
        _line('}');
        _indent--;
        _line('}');
      case IrForIn(:final name, :final iterable, :final body):
        // Borrowed, not moved: Dart's loop does not consume the list, and a
        // body that changed it while borrowing would be refused by rustc --
        // which is the same thing Dart refuses at runtime.
        _line('for ${snake(name)} in &${expr(iterable)} {');
        _indent++;
        stmt(body);
        _indent--;
        _line('}');
      case IrIndexSet(:final target, :final index, :final value):
        _line('${expr(target)}[${expr(index)} as usize] = ${expr(value)};');
      case IrLocalFunction(:final name, :final closure):
        _line('let ${snake(name)} = ${expr(closure)};');
      case IrLabeled(:final label, :final body):
        _line("'$label: {");
        _indent++;
        stmt(body);
        _indent--;
        _line('}');
      case IrBreak(:final label):
        _line(label == null ? 'break;' : "break '$label;");
      case IrContinue():
        _line('continue;');
      case IrSwitch(:final value, :final cases, :final otherwise):
        _line('match ${expr(value)} {');
        _indent++;
        for (final one in cases) {
          _line('${one.values.map(expr).join(' | ')} => {');
          _indent++;
          stmt(one.body);
          _indent--;
          _line('}');
        }
        if (otherwise != null) {
          _line('_ => {');
          _indent++;
          stmt(otherwise);
          _indent--;
          _line('}');
        }
        _indent--;
        _line('}');
      case IrWhile(:final condition, :final body, :final label):
        final head = label == null ? '' : "'" + label + ': ';
        _line('${head}while ${expr(condition)} {');
        _indent++;
        stmt(body);
        _indent--;
        _line('}');
      case IrLocalDecl(:final name, :final type, :final init):
        final annotation = type == null ? '' : ': ${this.type(type)}';
        final mutable = _reassigned.contains(name) ? 'mut ' : '';
        _line(
          'let $mutable${snake(name)}$annotation = '
          '${init == null ? "Default::default()" : expr(init)};',
        );
      case IrAssign(:final name, :final value):
        _line('${snake(name)} = ${expr(value)};');
      case IrAssignField(:final target, :final name, :final value):
        final receiver = target == null ? _selfName : expr(target);
        _line('$receiver.${snake(name)} = ${expr(value)};');
      case IrSetter(:final target, :final name, :final value):
        _line('${_receiver(target)}.set_${snake(name)}(${expr(value)});');
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
      case IrAssert(:final condition, :final literalMessage, :final message):
        // `debug_assert!`, not `assert!`: Dart's assert runs in debug builds
        // and is compiled out of release ones, and so is this. Using `assert!`
        // would keep every one of upstream's checks in a release binary, which
        // is a performance decision this compiler has no business making.
        if (message != null) {
          _line('// assert message, not translated: $message');
        }
        final text = literalMessage == null
            ? ''
            : ', "${_escape(literalMessage)}"';
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
  static (String, List<String>) emitLibrary(
    IrLibrary library, {
    List<String> frontEndRefusals = const [],
  }) {
    final out = StringBuffer();
    final refused = <String>[];
    if (frontEndRefusals.isNotEmpty) {
      // The front end's refusals belong in the file too. The backend has always
      // left a `// NOT TRANSLATED` where it stopped, but a member the *front
      // end* refused never reaches the backend at all, so the output said
      // nothing about it and only stderr did. A reader with the file in front
      // of them should not have to have kept the console.
      out.writeln(
        '// The front end refused '
        '${frontEndRefusals.length} member(s) in this library:',
      );
      for (final refusal in frontEndRefusals) {
        out.writeln('// NOT TRANSLATED: $refusal');
      }
      out.writeln();
    }
    if (library.constants.isNotEmpty) {
      // Module constants first: Dart's top-level names become Rust's, needing
      // no owner on either side.
      final holder = RustBackend(IrClass('<library>'), library: library);
      for (final constant in library.constants) {
        holder._member('top-level ${constant.name}', () {
          holder._line(
            'pub const ${screamingSnake(constant.name)}: '
            '${holder.type(constant.type)} = ${holder.expr(constant.value)};',
          );
        });
      }
      out.write(holder._out.join('\n'));
      out.writeln();
      out.writeln();
    }
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
    if (cls.isEnum) return _emitEnum();
    if (cls.isAbstract) return _emitTrait();
    return _emitStruct();
  }

  /// A Dart enum becomes a Rust enum, which is one of the few places the two
  /// languages need nothing said at all.
  ///
  /// The variants are renamed: Dart writes `Axis.vertical` and Rust writes
  /// `Axis::Vertical`. The name is otherwise left alone, so the output is still
  /// searchable against upstream.
  ///
  /// `Copy` because a Dart enum value is passed around freely and a Rust one
  /// that moved would need a `.clone()` at every use -- and `Eq`/`Hash` because
  /// upstream compares them and uses them as map keys.
  String _emitEnum() {
    _line('// Generated by tools/dart2rust from upstream `${cls.name}`');
    _line('// (Dart enum -> Rust enum).');
    _line('');
    _doc(cls.doc);
    if (cls.values.isEmpty) {
      // The front end refused it -- an enhanced enum, whose values it did not
      // carry. An empty Rust enum is legal and uninhabited, so emitting one
      // would turn "not translated" into "has no values", which is a different
      // and false statement.
      _line('// NOT TRANSLATED: `${cls.name}` is an enhanced enum -- a Rust');
      _line('// enum plus an impl, which is a separate job.');
      return _out.join('\n') + '\n';
    }
    _line('#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]');
    _line('${_vis(cls.name)}enum ${cls.name} {');
    _indent++;
    for (final value in cls.values) {
      _line('${variantName(value)},');
    }
    _indent--;
    _line('}');
    return _out.join('\n') + '\n';
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
    _fieldsAreAccessors = true;
    _line('// Generated by tools/dart2rust from upstream `${cls.name}`');
    _line('// (abstract -> trait).');
    _line('');
    _doc(cls.doc);
    // The free functions go first: which of them failed decides whether the
    // trait's matching default can delegate or has to be a `todo!()`.
    _emitSuperFns();
    _line('');
    _line('${_vis(cls.name)}trait ${cls.name} {');
    _indent++;
    // Guarded per member, like the struct path. The trait path was missed when
    // that changed, and it showed the moment private members started being
    // translated: one `toString` holding a string concatenation took the whole
    // `AlignmentGeometry` trait with it, and every `impl` of it stopped
    // compiling. Third time this has come up -- the unit of refusal should be
    // the unit of work everywhere, not only where it has been noticed.
    // A trait holds no storage, so the fields this class declares are reached
    // through required accessors. The fields themselves live on every
    // implementor, put there by `_allFields`; this is the other half of that.
    for (final field in cls.fields) {
      _line('/// `${cls.name}.${field.name}`, which the implementor stores.');
      _line('fn ${snake(field.name)}(&self) -> ${type(field.type)};');
      _line('');
    }
    for (final method in cls.abstractMethods) {
      _member('${cls.name}.${method.name} (required)', () {
        _doc(method.doc);
        _line(
          'fn ${_methodName(method)}(${_params(method)}) -> '
          '${type(method.returnType)};',
        );
        _line('');
      });
    }
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      _member('${cls.name}.${method.name} (default)', () {
        _doc(method.doc);
        _line(
          'fn ${_methodName(method)}(${_params(method)}) -> '
          '${type(method.returnType)} {',
        );
        _indent++;
        // The default delegates to the free function rather than holding the
        // body, so that an override can still reach it. See `superFn`.
        if (_superFailed.contains(method.name)) {
          _line('todo!("${cls.name}.${method.name} did not translate")');
        } else {
          _line(
            '${superFn(cls.name, method.name)}('
            '${['self', ...method.params.map((p) => snake(p.name))].join(', ')})',
          );
        }
        _indent--;
        _line('}');
        _line('');
      });
    }
    _indent--;
    _line('}');
    return _out.join('\n') + '\n';
  }

  /// Whether a Rust type is `Copy`.
  ///
  /// Asked of the rendered text rather than the IR, because that is what the
  /// derive has to be true of. Owning types are the ones that are not.
  /// A `const` needs a value Rust can build at compile time, and neither
  /// `vec![]` nor `HashMap::from([..])` is one. Said here rather than left to
  /// rustc, because one broken constant takes the whole file with it.
  static bool _constable(String rust) =>
      !rust.contains('Vec<') && !rust.contains('HashMap<');

  static bool _isCopy(String rust) =>
      !rust.contains('String') &&
      !rust.contains('Box<') &&
      !rust.contains('Vec<') &&
      !rust.contains('HashMap<') &&
      !rust.contains('dyn ');

  /// The bodies of this abstract class's concrete methods, as free functions.
  ///
  /// Generic over the implementor and `?Sized`, so both the trait's own default
  /// and a subclass's override can call it -- the default has an unsized `Self`,
  /// and a subclass has a concrete one.
  /// Names whose free function could not be emitted.
  ///
  /// The trait's default for such a method cannot delegate to a function that
  /// does not exist, so it gets a `todo!()` instead -- the trait and every impl
  /// of it still line up, which a missing method would not.
  final _superFailed = <String>{};

  void _emitSuperFns() {
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      if (!_member(
        superFn(cls.name, method.name),
        () => _emitSuperFn(method),
      )) {
        _superFailed.add(method.name);
      }
    }
  }

  void _emitSuperFn(IrMethod method) {
    {
      _line('');
      _line('/// The body of `${cls.name}.${method.name}`, reachable from an');
      _line('/// override the way Dart\'s `super.${method.name}` is.');
      final params = [
        'this_: &S',
        ...method.params.map(
          (p) => '${snake(p.name)}: ${type(p.type, owned: false)}',
        ),
      ].join(', ');
      _line(
        '${_vis(cls.name)}fn ${superFn(cls.name, method.name)}'
        '<S: ${cls.name} + ?Sized>($params) -> '
        '${type(method.returnType)} {',
      );
      _indent++;
      _selfName = 'this_';
      _returns = method.returnType;
      _reassigned = _assignedIn(method.body);
      stmt(method.body, tail: true);
      _returns = null;
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

  /// A parameter's declaration, `mut` when the body reassigns it.
  ///
  /// Dart parameters are ordinary variables and get reassigned freely; Rust
  /// parameters are immutable unless the declaration says otherwise, and
  /// `mut x: f32` is where that is said. Without it,
  /// `shadow(start) { start = start + 1; }` emitted an assignment to something
  /// that cannot be assigned.

  // -- Mutability -------------------------------------------------------------

  /// Methods of this class that need `&mut self`.
  ///
  /// Seeded with the ones that write a field of `this`, then closed over calls:
  /// a method that calls a mutating method **on itself** is mutating too. That
  /// closure is the answer to "who decides how far `&mut` spreads" -- nobody
  /// decides, it is computed, and it stops at the class boundary because a call
  /// on another object is refused for other reasons already.
  ///
  /// A fixpoint rather than one pass: `a` may call `b` which calls `c`, and only
  /// `c` writes. One pass would find `b` and miss `a`.
  late final Set<String> _mutating = _computeMutating();

  Set<String> _computeMutating() {
    final writes = <String>{};
    final calls = <String, Set<String>>{};
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      final key = _rustName(method);
      final found = _WalkSelf();
      found.statement(method.body);
      // No special case for setters. One was written here -- "a setter exists
      // to change something, so mark it mutating" -- and the mutation sweep
      // could not kill it: a setter that only delegates is already reached by
      // the contagion below, and one that writes nothing needs no `&mut self`.
      // A rule with no observable difference should not be written.
      if (found.writesFields) writes.add(key);
      calls[key] = found.selfCalls;
    }
    var changed = true;
    while (changed) {
      changed = false;
      for (final entry in calls.entries) {
        if (writes.contains(entry.key)) continue;
        if (entry.value.any(writes.contains)) {
          writes.add(entry.key);
          changed = true;
        }
      }
    }
    return writes;
  }

  /// `&self` or `&mut self`, and a refusal where the signature is not ours.
  ///
  /// Two shapes have a fixed receiver and cannot become `&mut self`:
  ///
  /// * an operator that became an `impl std::ops::*`, whose method takes `self`
  ///   by value because the trait says so;
  /// * a method an abstract base declares, whose receiver is the trait's, not
  ///   this class's -- changing it would have to change the trait and every
  ///   other implementor.
  ///
  /// Both are refused rather than emitted with the wrong receiver. Upstream's
  /// operators do not assign, so the first is a guard rather than a loss.
  String _receiverOf(IrMethod method) {
    if (!_mutating.contains(_rustName(method))) return '&self';
    if (method.operator != null &&
        _operatorTraits.containsKey(method.operator)) {
      throw Unsupported(
        'a field write inside `operator ${method.operator}`',
        'std::ops takes `self`, so the receiver is not this class\'s to change',
      );
    }
    final base = library[cls.superclass];
    if (base != null &&
        base.isAbstract &&
        (base.abstractMethods.any((m) => m.name == method.name) ||
            base.methods.any((m) => m.name == method.name))) {
      throw Unsupported(
        'a field write inside `${method.name}`, which `${base.name}` declares',
        'the receiver belongs to the trait, not to this class',
      );
    }
    return '&mut self';
  }

  /// A member's name in Rust.
  ///
  /// `get x` and `set x` are the same name in Dart and cannot be in Rust, so a
  /// setter becomes `set_x`. Everything keyed by member -- the mutability set
  /// especially -- keys on *this* name, because keying on the Dart name would
  /// make a getter and its setter one entry and mark the getter `&mut self`.
  String _rustName(IrMethod method) =>
      method.isSetter ? 'set_${snake(method.name)}' : _identifier(method.name);

  // -- Flattening the hierarchy -----------------------------------------------

  /// This class's fields, with its bases' in front of them.
  ///
  /// Rust has no inheritance, so a subclass's struct has to carry what the base
  /// declared. Round five turned an abstract class into a trait and reported its
  /// fields as untranslated; this is that bill coming due, because 80% of the
  /// 1888 `super(...)` calls in package:flutter have an abstract base.
  ///
  /// Base first, in declaration order, so the layout reads the way upstream's
  /// class hierarchy does.
  List<IrFieldDecl> _allFields(IrClass of) {
    final base = library[of.superclass];
    if (base == null) return of.fields;
    return [..._allFields(base), ...of.fields];
  }

  /// The field initialisers a `super(...)` stands for.
  ///
  /// The base's own constructor is *inlined*: its parameters are replaced by
  /// the arguments the super call passed, and its field initialisers become
  /// this constructor's. That is what flattening means once it reaches storage,
  /// and it recurses, since a base may call `super` in turn -- the chains go six
  /// deep in places.
  Map<String, IrExpr> _inheritedInits(IrConstructor ctor) {
    final baseName = ctor.superBase;
    if (baseName == null) return const {};
    final base = library[baseName];
    if (base == null) {
      throw Unsupported(
        'super constructor call into `$baseName`, which is not in this file',
        'super(...)',
      );
    }
    final baseCtors = base.constructors.where((c) => c.name == null).toList();
    if (baseCtors.length != 1) {
      throw Unsupported(
        'super constructor call into `$baseName`, which has '
            '${baseCtors.length} unnamed constructors',
        'super(...)',
      );
    }
    final baseCtor = baseCtors.single;
    if (baseCtor.params.length != ctor.superArgs.length) {
      throw Unsupported(
        'super(...) passes ${ctor.superArgs.length} arguments to a '
            'constructor taking ${baseCtor.params.length}',
        'super(...)',
      );
    }
    final substitution = <String, IrExpr>{
      for (var i = 0; i < baseCtor.params.length; i++)
        baseCtor.params[i].name: ctor.superArgs[i],
    };
    return {
      // The base's own inherited initialisers first, so a chain resolves from
      // the top down and a nearer class can override nothing -- Dart does not
      // let it, and neither does this.
      ..._inheritedInits(baseCtor)
          .map((k, v) => MapEntry(k, _substitute(v, substitution))),
      ...baseCtor.fieldInits.map(
        (k, v) => MapEntry(k, _substitute(v, substitution)),
      ),
    };
  }

  /// Replaces references to a constructor's parameters with the expressions a
  /// `super(...)` passed for them.
  IrExpr _substitute(IrExpr e, Map<String, IrExpr> by) {
    IrExpr go(IrExpr node) => _substitute(node, by);
    return switch (e) {
      IrLocal(:final name) => by[name] ?? e,
      IrField(:final target, :final name) => IrField(
        target == null ? null : go(target),
        name,
      ),
      IrBinary(:final op, :final left, :final right, :final type) => IrBinary(
        op,
        go(left),
        go(right),
        type: type,
      ),
      IrUnary(:final op, :final operand) => IrUnary(op, go(operand)),
      IrNullCheck(:final operand) => IrNullCheck(go(operand)),
      IrIsNull(:final operand) => IrIsNull(go(operand)),
      IrIfNull(
        :final left,
        :final right,
        :final nullableResult,
        :final eager,
      ) =>
        IrIfNull(
          go(left),
          go(right),
          nullableResult: nullableResult,
          eager: eager,
        ),
      IrNullAware(:final receiver, :final body) => IrNullAware(
        go(receiver),
        go(body),
      ),
      IrCall(:final target, :final name, :final args) => IrCall(
        target == null ? null : go(target),
        name,
        args.map(go).toList(),
      ),
      IrStaticCall(:final owner, :final name, :final args) => IrStaticCall(
        owner,
        name,
        args.map(go).toList(),
      ),
      IrNew(:final type, :final args, :final constructor) => IrNew(
        type,
        args.map(go).toList(),
        constructor: constructor,
      ),
      IrConditional(:final condition, :final then, :final otherwise) =>
        IrConditional(go(condition), go(then), go(otherwise)),
      IrSuperCall(:final base, :final name, :final args) => IrSuperCall(
        base,
        name,
        args.map(go).toList(),
      ),
      IrIs(:final expr, :final type, :final negated) => IrIs(
        go(expr),
        type,
        negated: negated,
      ),
      IrCallValue(:final target, :final args) => IrCallValue(
        go(target),
        args.map(go).toList(),
      ),
      IrBlockValue(:final statements, :final value) => IrBlockValue(
        statements,
        go(value),
      ),
      IrConstInstance(:final type, :final fields) => IrConstInstance(type, {
        for (final entry in fields.entries) entry.key: go(entry.value),
      }),
      IrIdentical(:final left, :final right) => IrIdentical(
        go(left),
        go(right),
      ),
      IrThrowValue(:final value) => IrThrowValue(go(value)),
      IrInterpolation(:final parts) => IrInterpolation(parts.map(go).toList()),
      IrIndex(:final target, :final index) => IrIndex(go(target), go(index)),
      IrIterChain(:final source, :final steps) => IrIterChain(go(source), [
        for (final step in steps) (step.$1, go(step.$2)),
      ]),
      IrListLiteral(:final elements, :final element) => IrListLiteral(
        elements.map(go).toList(),
        element,
      ),
      IrMapLiteral(:final entries, :final key, :final value) => IrMapLiteral(
        [for (final entry in entries) (go(entry.$1), go(entry.$2))],
        key,
        value,
      ),
      IrFunctionRef() => e,
      IrAssignValue(:final name, :final value) => IrAssignValue(
        name,
        go(value),
      ),
      IrSetValue(:final target, :final name, :final value) => IrSetValue(
        target == null ? null : go(target),
        name,
        go(value),
      ),
      IrClosure() ||
      IrLiteral() ||
      IrStatic() ||
      IrTopLevel() ||
      IrThis() ||
      IrBound() => e,
    };
  }

  // -- Failure in the return value --------------------------------------------

  /// Methods of this class whose Rust signature returns `Result`.
  ///
  /// Seeded with the ones that throw, then closed over calls, the same shape as
  /// `_mutating`. Measured before it was built: across `package:flutter` 717
  /// members throw directly and 5906 -- 20% of all members -- return `Result`
  /// once that has spread. Not "almost everything", which is what made the
  /// decision affordable.
  ///
  /// It stops at the class boundary here. A call into another class would carry
  /// the failure further, and 20% is the whole-program figure; what this
  /// computes is the part visible in one file. The rest waits for the compiler
  /// to see more than a file at a time, which is the same wall the stubs are at.
  late final Map<String, String> _failing = _computeFailing();

  Map<String, String> _computeFailing() {
    final failing = <String, String>{};
    final calls = <String, Set<String>>{};
    for (final method in cls.methods) {
      final key = _rustName(method);
      if (method.throws != null) failing[key] = method.throws!;
      final found = _WalkSelf();
      found.statement(method.body);
      calls[key] = found.selfCalls;
    }
    var changed = true;
    while (changed) {
      changed = false;
      for (final entry in calls.entries) {
        if (failing.containsKey(entry.key)) continue;
        for (final callee in entry.value) {
          final error = failing[callee];
          if (error != null) {
            failing[entry.key] = error;
            changed = true;
            break;
          }
        }
      }
    }
    return failing;
  }

  /// Whether a statement returns from the method it is written in.
  ///
  /// Not from a closure written inside it -- `IrClosure` is not descended into,
  /// for the same reason the front ends' version skips nested functions.
  bool _returnsEarly(IrStmt s) {
    var found = false;
    void walk(IrStmt s) {
      if (found) return;
      switch (s) {
        case IrReturn():
          found = true;
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
        case IrForIn(:final body):
          walk(body);
        case IrLabeled(:final body):
          walk(body);
        case IrSwitch(:final cases, :final otherwise):
          for (final one in cases) {
            walk(one.body);
          }
          if (otherwise != null) walk(otherwise);
        default:
      }
    }

    walk(s);
    return found;
  }

  /// Whether every path through a statement leaves the method.
  ///
  /// Deliberately conservative: it says yes only where it can see that it must
  /// be so. Saying yes wrongly would emit an `unreachable!()` that is reached,
  /// which is a panic at runtime; saying no wrongly costs nothing but a `{}`
  /// arm the compiler then complains about, which is loud and cheap.
  bool _alwaysReturns(IrStmt s) => switch (s) {
    IrReturn() => true,
    IrThrow() => true,
    IrBlock(:final statements) => statements.any(_alwaysReturns),
    IrIf(:final then, :final otherwise) =>
      otherwise != null && _alwaysReturns(then) && _alwaysReturns(otherwise),
    IrTryCatch(:final body, :final handler) =>
      _alwaysReturns(body) && _alwaysReturns(handler),
    IrTryFinally(:final body, :final finalizer) =>
      _alwaysReturns(body) || _alwaysReturns(finalizer),
    // A labelled block can be left by its `break`, so it does not count as
    // always returning even when its body would.
    IrLabeled() => false,
    _ => false,
  };

  /// The error type a statement can produce, taken from the failing methods of
  /// this class that it calls.
  String? _errorIn(IrStmt body) {
    final found = _WalkSelf();
    found.statement(body);
    for (final name in found.selfCalls) {
      final error = _failing[name];
      if (error != null) return error;
    }
    return null;
  }

  /// The Rust return type of the method currently being emitted, as written in
  /// its signature -- `Result<..>` and all. A `return` inside a try body has to
  /// carry a value of exactly this type out of the closure.
  String? _rustReturns;

  /// Set while emitting a try body that contains a `return`.
  ///
  /// Inside one, `return x` cannot be a Rust `return`: it would return from the
  /// closure, and the method would carry on. It becomes `Ok(Some(x))` instead,
  /// which the `match` outside turns back into a real return.
  bool _inFlowClosure = false;

  /// The error type of the method currently being emitted, if it can fail.
  String? _failure;

  /// A method's return type, wrapped when it can fail.
  String _returnType(IrMethod method) {
    final error = _failing[_rustName(method)];
    final value = method.isSetter ? '()' : type(method.returnType);
    return error == null ? value : 'Result<$value, $error>';
  }

  String _param(IrParam p, {bool owned = true}) =>
      '${_reassigned.contains(p.name) ? "mut " : ""}'
      '${snake(p.name)}: ${type(p.type, owned: owned)}';

  String _params(IrMethod method) => [
    if (!method.isStatic) '&self',
    // A parameter is borrowed, not owned: passing a `Box<dyn Trait>` in
    // would move it, and upstream's callers do not give theirs away.
    ...method.params.map((p) => _param(p, owned: false)),
  ].join(', ');

  String _emitStruct() {
    _line('// Generated by tools/dart2rust from upstream `${cls.name}`.');
    _line('//');
    _line('// Translated, not ported: this is the compiler\'s output, not a');
    _line('// hand-written re-expression. See tools/dart2rust/README.md.');
    _line('');
    _doc(cls.doc);
    // `Copy` only when every field is. A `String` field is not, and deriving
    // it anyway does not compile -- which is loud, but the derive is this
    // compiler's own line and it should not write one it knows is wrong.
    final copyable = _allFields(cls).every((f) => _isCopy(type(f.type)));
    _line('#[derive(Clone, ${copyable ? 'Copy, ' : ''}Debug, PartialEq)]');
    _line('${_vis(cls.name)}struct ${cls.name} {');
    _indent++;
    for (final field in _allFields(cls)) {
      _doc(field.doc);
      _line('${_vis(field.name)}${snake(field.name)}: ${type(field.type)},');
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
    // Every abstract **ancestor**, not just a direct abstract base. `Padded`
    // extends the concrete `Square`, which extends the abstract `Shape`; with
    // only the direct base considered, `Padded` implemented nothing and
    // `Shape`'s methods were unreachable from it.
    for (final ancestor in _abstractAncestors(cls)) {
      _emitImplFor(ancestor);
    }
  }

  /// The abstract classes above this one, nearest first.
  List<IrClass> _abstractAncestors(IrClass of) {
    final above = library[of.superclass];
    if (above == null) return const [];
    return [if (above.isAbstract) above, ..._abstractAncestors(above)];
  }

  void _emitImplFor(IrClass base) {
    // Not just the abstract ones. A class that overrides a *concrete* base
    // method needs that override in the impl too, or dynamic dispatch reaches
    // the trait's default instead -- the inherent method would still be right,
    // so only a call through `dyn Base` can tell, which is why the tests make
    // that call.
    final overridden = base.methods
        .where((m) => !m.isStatic && _matching(m) != null)
        .toList();
    // Accessors come from this base alone here; a farther ancestor gets its own
    // impl block and its own.
    final ownFields = base.fields;
    final required = [...base.abstractMethods, ...overridden];
    // Accessors count as a reason to emit the impl. A base with no abstract
    // methods and nothing overridden still has fields, and without them the
    // subclass does not implement the trait at all -- so its inherited methods
    // are unreachable, which is how `area()` went missing.
    final accessors = ownFields;
    if (required.isEmpty && accessors.isEmpty) return;

    _line('');
    _line('impl ${base.name} for ${cls.name} {');
    _indent++;
    for (final field in accessors) {
      _line('fn ${snake(field.name)}(&self) -> ${type(field.type)} {');
      _indent++;
      _line('self.${snake(field.name)}');
      _indent--;
      _line('}');
      _line('');
    }
    for (final need in required) {
      _member(
        'impl ${base.name}::${need.operator ?? need.name} for ${cls.name}',
        () => _emitBaseMethod(need),
      );
    }
    _indent--;
    _line('}');
  }

  /// The base the impl block currently being written is for.
  late IrClass _implBase;

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
        _line(
          'todo!("${cls.name} does not translate '
          '${need.operator ?? need.name} yet")',
        );
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
    final name = _ctorName(ctor.name);
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
    // A constructor with a body cannot be `const`: it builds the value into a
    // local and runs statements against it, and a `const fn` may not.
    final constness = ctor.isConst && ctor.body == null ? 'const ' : '';
    _line(
      '${_vis(ctor.name ?? cls.name)}${constness}fn $name($params) -> Self {',
    );
    _indent++;
    for (final check in ctor.asserts) {
      stmt(check);
    }
    final inits = {..._inheritedInits(ctor), ...ctor.fieldInits};
    _line(ctor.body == null ? 'Self {' : 'let mut __new = Self {');
    _indent++;
    for (final field in _allFields(cls)) {
      // The constructor first, then the declaration's own value: Dart applies
      // the latter only where the former says nothing.
      final init = inits[field.name] ?? field.initial;
      if (init == null) {
        throw Unsupported('field never initialised', field.name);
      }
      _line('${snake(field.name)}: ${expr(init)},');
    }
    _indent--;
    final body = ctor.body;
    if (body == null) {
      _line('}');
    } else {
      _line('};');
      // `this` inside the body is the value being built, not a `self` that
      // does not exist yet. `_selfName` is the same lever a free function
      // uses, so the body's `this.x = v` comes out as `__new.x = v`.
      final saved = _selfName;
      _selfName = '__new';
      stmt(body);
      _selfName = saved;
      _line('__new');
    }
    _indent--;
    _line('}');
    _line('');
  }

  void _emitConstants() {
    for (final constant in cls.constants) {
      if (!_constable(type(constant.type))) {
        _line('// NOT TRANSLATED: ${cls.name}.${constant.name}');
        _line('//   a `const` cannot hold a collection');
        _line('');
        continue;
      }
      _doc(constant.doc);
      _line(
        '${_vis(constant.name)}const ${screamingSnake(constant.name)}: ${type(constant.type)} '
        '= ${expr(constant.value)};',
      );
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
      // Before the signature: whether a parameter needs `mut` is decided by the
      // body, and the signature is written first.
      _reassigned = _assignedIn(method.body);
      _doc(method.doc);
      final params = [
        if (!method.isStatic) _receiverOf(method),
        // Parameters are a borrowed position: a function type there is
        // `impl Fn(..)`, which a closure literal can be passed to
        // directly, rather than `Box<dyn Fn(..)>`, which would need a
        // `Box::new` at every call site.
        ...method.params.map((p) => _param(p, owned: false)),
      ].join(', ');
      // A setter returns nothing: Dart's `set x(v)` has no return type, and
      // giving one a value would make `a.x = 1` an expression, which it is not.
      final returns = _returnType(method);
      _failure = _failing[_rustName(method)];
      _rustReturns = returns;
      _referenceParams = {
        for (final p in method.params)
          if (library.isAbstract(p.type.name)) p.name,
      };
      _line(
        '${_vis(method.name)}fn ${_rustName(method)}($params) -> $returns {',
      );
      _indent++;
      _returns = method.returnType;
      stmt(method.body, tail: true);
      _returns = null;
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
          ...method.params.map((p) => _param(p, owned: false)),
        ].join(', ');
        _line(
          '${_vis(method.name)}fn ${_operatorName(op)}($params) -> ${type(method.returnType)} {',
        );
        _indent++;
        _returns = method.returnType;
        _reassigned = _assignedIn(method.body);
        stmt(method.body, tail: true);
        _returns = null;
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
      _returns = method.returnType;
      _reassigned = _assignedIn(method.body);
      stmt(method.body, tail: true);
      _returns = null;
      _indent--;
      _line('}');
      _indent--;
      _line('}');
    }
  }

  /// A Rust-legal name for a Dart operator.
  ///
  /// The fallback used to be `op_` plus the code units, which turned `==` into
  /// `op_61_61` -- legal, but unreadable and unsearchable. Every operator Dart
  /// has is named here instead; anything genuinely unknown stops rather than
  /// being spelled in decimal.
  static String _operatorName(String op) => switch (op) {
    '+' => 'op_add',
    '-' => 'op_sub',
    '*' => 'op_mul',
    '/' => 'op_div',
    '%' => 'op_rem',
    'unary-' => 'op_neg',
    '~/' => 'int_div',
    '[]' => 'index_of',
    '[]=' => 'index_set',
    '==' => 'op_eq',
    '<' => 'lt',
    '>' => 'gt',
    '<=' => 'le',
    '>=' => 'ge',
    '&' => 'bit_and',
    '|' => 'bit_or',
    '^' => 'bit_xor',
    '~' => 'bit_not',
    '<<' => 'shl',
    '>>' => 'shr',
    '>>>' => 'ushr',
    _ => throw Unsupported('operator `$op` has no Rust name', op),
  };

  /// A Rust-legal identifier for any Dart member name.
  ///
  /// `superFn` pastes the name into another identifier, so an operator's own
  /// spelling cannot go through: `superFn('AlignmentGeometry', '==')` produced
  /// `alignment_geometry_super_`, a name with nothing on the end of it.
  static String _identifier(String name) =>
      RegExp(r'^[A-Za-z_][A-Za-z0-9_]*$').hasMatch(name)
      ? snake(name)
      : _operatorName(name);
}

/// Finds, in one method body, whether it writes a field of `this` and which of
/// its own methods it calls.
///
/// Both answers are needed together and both need the *whole* body, statements
/// and expressions alike -- a mutating call can be buried in the middle of an
/// expression, and missing one would emit `&self` for a method that assigns.
class _WalkSelf {
  bool writesFields = false;
  final selfCalls = <String>{};

  /// `Vec` methods that change what they are called on.
  static const _mutatingListMethods = {
    'push',
    'extend',
    'clear',
    'pop',
    'insert',
    'remove',
  };

  /// Whether a write target is `this`, or a chain of field reads from it.
  static bool _rootedAtThis(IrExpr? e) => switch (e) {
    null => true,
    IrThis() => true,
    IrField(:final target) => _rootedAtThis(target),
    _ => false,
  };

  /// Locals written by an assignment used for its value.
  final assignedLocals = <String>{};

  void statement(IrStmt s) {
    switch (s) {
      case IrAssignField(:final target):
        // Only a write to `this` makes the method mutating. A cascade writes a
        // *local* it just bound, which needs `let mut` and not `&mut self` --
        // and counting it made every method holding a cascade take `&mut self`.
        //
        // A *chain* rooted at `this` counts too: `self.tint.opacity = v` is a
        // write through `self`, and without this it came out `&self` and did
        // not compile.
        if (_rootedAtThis(target)) writesFields = true;
        expression(s.value);
      case IrAssign():
        expression(s.value);
      case IrSetter(:final target, :final name, :final value):
        // A setter call on `this` spreads `&mut` exactly as a method call does.
        if (target == null || target is IrThis) selfCalls.add('set_$name');
        if (target != null) expression(target);
        expression(value);
      case IrBlock(:final statements):
        statements.forEach(statement);
      case IrIf(:final condition, :final then, :final otherwise):
        expression(condition);
        statement(then);
        if (otherwise != null) statement(otherwise);
      case IrReturn(:final value):
        if (value != null) expression(value);
      case IrLocalDecl(:final init):
        if (init != null) expression(init);
      case IrExprStmt(:final expr):
        expression(expr);
      case IrAssert(:final condition):
        expression(condition);
      case IrThrow(:final value):
        expression(value);
      case IrTryCatch(:final body, :final handler):
        // Calls in the body are caught, so they do not make this method fail --
        // that is what `catch` means, and it is the only thing that stops the
        // propagation. Without this, a method that catches still had `Result`
        // in its signature, which compiles and says the opposite of the truth.
        // Calls in the *handler* are not caught and still count.
        _caught++;
        statement(body);
        _caught--;
        statement(handler);
      case IrTryFinally(:final body, :final finalizer):
        // No `_caught` here, and that is the difference between the two nodes:
        // a finalizer runs on the way past a failure, it does not stop one. A
        // failing call in this body still makes the method fail.
        statement(body);
        statement(finalizer);
      case IrWhile(:final condition, :final body):
        expression(condition);
        statement(body);
      case IrForIn(:final iterable, :final body):
        expression(iterable);
        statement(body);
      case IrLocalFunction(:final closure):
        expression(closure);
      case IrIndexSet(:final target, :final index, :final value):
        // Writing through an index is writing through the thing indexed.
        if (_rootedAtThis(target)) writesFields = true;
        expression(target);
        expression(index);
        expression(value);
      case IrLabeled(:final body):
        statement(body);
      case IrSwitch(:final value, :final cases, :final otherwise):
        expression(value);
        for (final one in cases) {
          one.values.forEach(expression);
          statement(one.body);
        }
        if (otherwise != null) statement(otherwise);
      case IrBreak():
      case IrContinue():
    }
  }

  /// How many `try` bodies deep the walk is. A call inside one is caught.
  int _caught = 0;

  void expression(IrExpr e) {
    switch (e) {
      case IrCall(:final target, :final name, :final args):
        if (_caught == 0 && (target == null || target is IrThis)) {
          selfCalls.add(name);
        }
        // `self.marks.push(x)` mutates a field, so the method takes
        // `&mut self` -- the same rule as writing the field outright, which is
        // what a `Vec` method that changes it amounts to.
        if (_mutatingListMethods.contains(name) && _rootedAtThis(target)) {
          writesFields = true;
        }
        if (target != null) expression(target);
        args.forEach(expression);
      case IrField(:final target):
        if (target != null) expression(target);
      case IrBinary(:final left, :final right):
        expression(left);
        expression(right);
      case IrUnary(:final operand):
        expression(operand);
      case IrNullCheck(:final operand):
        expression(operand);
      case IrIsNull(:final operand):
        expression(operand);
      case IrIfNull(:final left, :final right):
        expression(left);
        expression(right);
      case IrNullAware(:final receiver, :final body):
        expression(receiver);
        expression(body);
      case IrConditional(:final condition, :final then, :final otherwise):
        expression(condition);
        expression(then);
        expression(otherwise);
      case IrStaticCall(:final args):
        args.forEach(expression);
      case IrNew(:final args):
        args.forEach(expression);
      case IrSuperCall(:final args):
        args.forEach(expression);
      case IrIs(:final expr):
        expression(expr);
      case IrClosure(:final body):
        statement(body);
      case IrCallValue(:final target, :final args):
        expression(target);
        args.forEach(expression);
      case IrBlockValue(:final statements, :final value):
        statements.forEach(statement);
        expression(value);
      case IrConstInstance(:final fields):
        fields.values.forEach(expression);
      case IrIdentical(:final left, :final right):
        expression(left);
        expression(right);
      case IrThrowValue(:final value):
        expression(value);
      case IrInterpolation(:final parts):
        parts.forEach(expression);
      case IrIndex(:final target, :final index):
        expression(target);
        expression(index);
      case IrListLiteral(:final elements):
        elements.forEach(expression);
      case IrMapLiteral(:final entries):
        for (final entry in entries) {
          expression(entry.$1);
          expression(entry.$2);
        }
      case IrIterChain(:final source, :final steps):
        expression(source);
        for (final step in steps) {
          expression(step.$2);
        }
      case IrFunctionRef():
      case IrAssignValue():
        if (e is IrAssignValue) {
          assignedLocals.add(e.name);
          expression(e.value);
        }
      case IrSetValue(:final target, :final value):
        // Same rule as the statement form: only a write to `this` makes the
        // method mutating.
        if (target == null || target is IrThis) writesFields = true;
        if (target != null) expression(target);
        expression(value);
      case IrLiteral():
      case IrLocal():
      case IrStatic():
      case IrTopLevel():
      case IrBound():
      case IrThis():
    }
  }
}
