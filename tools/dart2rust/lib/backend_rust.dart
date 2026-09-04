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

import 'dart:convert';

import 'ir.dart';
import 'prelude.dart';

/// Dart's primitives, in the spelling this project's crate uses.
///
/// `double` is `f64`, and the note that used to stand here said `f32`.
///
/// The old reason was that the hand port measures in `f32` because that is
/// what the engine's geometry takes, so a translated value type has to sit
/// beside one. That is a fact about the *hand port*, and it was allowed to
/// decide what a `double` is -- which is not a choice a translator gets to
/// make. Dart's `double` is IEEE-754 double precision; the language says so.
/// Where a translated value has to meet the engine's `f32`, the cast belongs
/// at that boundary, not in the meaning of the type.
const _primitives = {
  // `f64`, not `f32`. Dart's `double` is IEEE-754 **double** precision -- the
  // language specifies it -- and this compiler mapped it to `f32` from its
  // first round to its eighty-eighth. Nothing caught it: every fixture value
  // is exactly representable in both, so the tests passed, the two front ends
  // agreed, and the output compiled. It was wrong the whole time, in the way
  // that matters least until it matters completely: 0.1 + 0.2 is a different
  // number in the two widths, and every layout arithmetic in Flutter is
  // doubles.
  // `std::boxed::Box` is spelled out wherever the backend writes a `Box`:
  // `material_color_utilities` declares a class named `Box`, the import
  // tracker imports it into every file that mentions the word, and it took
  // `Box<dyn Fn(..)>` with it -- 2849 `E0107`s from one name.
  'double': 'f64',
  'int': 'i64',
  // Dart's bare `Function` type: a callable of unknown shape. Held, not
  // called -- `Map<Function, CallbackHandle>` keys it -- so the widest
  // owned thing there is. A call through one would not compile, and says so.
  'Function': 'std::rc::Rc<dyn Object>',
  // Dart's `Never` has two spellings in stable Rust: `!` as a function's bare
  // return type, and `std::convert::Infallible` everywhere else -- a type
  // argument, a `Result<Never, E>`, a `PopupMenuEntry<Never>`. The first
  // round mapped it to `!` everywhere and rustc called 22 of them
  // "experimental". The map holds the general spelling; the signature
  // emitter substitutes `!` for the one position that takes it.
  'Never': 'std::convert::Infallible',
  // Dart's `num` is the supertype of `int` and `double`, and Rust has no such
  // thing. `f32` is the choice that keeps arithmetic working and matches what
  // `double` already maps to -- 2511 uses of the bare name `num`, three
  // quarters of every "cannot find" in the package, and every one of them a
  // parameter or return that takes either.
  //
  // The cost, written down rather than discovered: an `int` beyond 2^24 does
  // not survive the round trip, and a `num` used as an index needs a cast that
  // an `i64` would not. Upstream's `num`s are sizes, offsets and factors, so
  // neither has come up -- but this is where to look when one does.
  'num': 'f64',
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

String snake(String name) => _rustIdentifier(snakeRaw(name));

/// `snake` before the keyword escape. For a name that is only ever a *part*
/// of a longer identifier, or is about to be upper-cased: `r#` belongs at the
/// front of a whole identifier, and `R#LOOP` and `theme_extension_super_r#type`
/// were what escaping the parts produced -- 16 files that did not parse.
String snakeRaw(String name) => name
    .replaceAllMapped(RegExp(r'(?<!^)([A-Z])'), (m) => '_${m[1]}')
    .toLowerCase();

/// Rust's keywords. A Dart name that happens to be one has to be spelled
/// differently, and `r#type` is how Rust spells it -- the raw form keeps the
/// name searchable against upstream, which renaming to `type_` would not.
const _rustKeywords = {
  'as',
  'break',
  'const',
  'continue',
  'crate',
  'dyn',
  'else',
  'enum',
  'extern',
  'false',
  'fn',
  'for',
  'if',
  'impl',
  'in',
  'let',
  'loop',
  'match',
  'mod',
  'move',
  'mut',
  'pub',
  'ref',
  'return',
  'static',
  'struct',
  'trait',
  'true',
  'type',
  'unsafe',
  'use',
  'where',
  'while',
  'async',
  'await',
  'union',
  // Reserved rather than in use, and just as fatal: `box.left` on a local
  // named `box` -- which `TextPainter` has -- does not parse at all.
  'box',
  'abstract',
  'become',
  'do',
  'final',
  'macro',
  'override',
  'priv',
  'typeof',
  'unsized',
  'virtual',
  'yield',
  'try',
  'gen',
};

/// Reserved words that cannot even be raw identifiers.
const _rustNeverRaw = {'crate', 'self', 'super', 'Self'};

/// A name Rust will take.
///
/// Two things reach here that Rust will not accept, and both come from the
/// CFE rather than from anything upstream wrote:
///
/// * `_#wc0#formal` -- a synthetic parameter name. `#` is not an identifier
///   character, and the whole 525-module crate failed to *parse* on three of
///   these before the characters were stripped.
/// * `type`, `match`, `where` -- ordinary Dart names that are Rust keywords.
String _rustIdentifier(String name) {
  final out = _cleanIdentifier(name);
  if (_rustNeverRaw.contains(out)) return '${out}_';
  return _rustKeywords.contains(out) ? 'r#$out' : out;
}

/// The character-level half of `_rustIdentifier`: no `$`, `#` or `|`, no
/// leading digit, never empty. Without the keyword escape, which is the half
/// an upper-cased name does not need -- `snakeRaw` skipped *both* halves and
/// `_$ADD_EVENT` and `_Rect::#SIZE_OF` reached rustc.
String _cleanIdentifier(String name) {
  var out = name.replaceAll(RegExp(r'[^A-Za-z0-9_]'), '_');
  if (out.isEmpty) out = '_';
  if (RegExp(r'^[0-9]').hasMatch(out)) out = '_$out';
  return out;
}

// Rust's keywords are lowercase, so an upper-cased name is never one.
String screamingSnake(String name) =>
    _cleanIdentifier(snakeRaw(name)).toUpperCase();

/// `spaceBetween` -> `SpaceBetween`: an enum variant as Rust spells it.
///
/// Only the first letter changes. Rewriting the rest would make the output
/// impossible to search against upstream, which is the same reason private
/// members keep their leading underscore.
String variantName(String name) =>
    name.isEmpty ? name : name[0].toUpperCase() + name.substring(1);

/// The variants of one enum, spelled so that no two of them are the same.
///
/// Capitalising the first letter is right for `Axis.vertical` and wrong for
/// `HourFormat { HH, H, h }`, where `H` and `h` are two different values that
/// become one name. When that happens the *whole* enum keeps Dart's spelling:
/// mixing the two conventions inside one enum would be worse than either, and
/// the Dart names are the ones a reader can search for.
Map<String, String> variantNames(List<String> values) {
  final capitalised = {for (final v in values) v: variantName(v)};
  final distinct = capitalised.values.toSet().length == values.toSet().length;
  return distinct ? capitalised : {for (final v in values) v: v};
}

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

  /// One emitted line, made safe to stand beside others on a single line.
  ///
  /// A closure body becomes an expression by joining its lines with a space,
  /// and a `//` comment on any of them takes the rest of the line with it --
  /// including the braces that close the closure. `dart_ui.rs` stopped parsing
  /// at 1803 because one refused assert message commented out the 11,000 lines
  /// after it, and rustc reported it as an unclosed delimiter 11,000 lines
  /// later. The note is worth keeping; the line comment is not the way to keep
  /// it here.
  static String _inlineSafe(String line) {
    final text = line.trim();
    if (!text.startsWith('//')) return text;
    return '/* ${text.substring(2).trim().replaceAll('*/', '* /')} */';
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
    // Every scrap of state a member's emission sets, so a refusal leaves none
    // of it behind.
    //
    // Rolling back only the text was not enough. A constructor sets
    // `_selfName` to `__new` -- `this.x = v` inside a constructor body is a
    // write to the value being built -- and restores it afterwards. When the
    // body threw, the restore never ran, and every later method in that class
    // read its fields off a `__new` that does not exist there: 97 `E0425`s in
    // `SemanticsFlags` alone, all from one refused constructor.
    //
    // This is the same rule as "the unit of refusal must equal the unit of
    // work", one level down: a refusal has to undo the *state* as well as the
    // output, and listing it here is cheaper than remembering a `finally` at
    // each of the dozen places that set some.
    final selfName = _selfName;
    final fieldsAreAccessors = _fieldsAreAccessors;
    final inTrait = _inTrait;
    final referenceParams = _referenceParams;
    final selfIsHandle = _selfIsHandle;
    final reassigned = _reassigned;
    final failure = _failure;
    final rustReturns = _rustReturns;
    final implBinding = _implBinding;
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
    } finally {
      _selfName = selfName;
      _fieldsAreAccessors = fieldsAreAccessors;
      _inTrait = inTrait;
      _referenceParams = referenceParams;
      _selfIsHandle = selfIsHandle;
      _reassigned = reassigned;
      _failure = failure;
      _rustReturns = rustReturns;
      _implBinding = implBinding;
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
  /// `pub `, or `pub(crate) ` when the Dart name was private.
  ///
  /// Dart's privacy is per *library* and Rust's is per *module*, so emitting a
  /// `_`-prefixed member with no `pub` looked like the faithful thing. It is
  /// not quite, because Dart lets a private name escape its library without
  /// being public: `abstract class Path { factory Path() = _NativePath; }`
  /// hands every library a `_NativePath`, and Kernel resolves the factory, so
  /// the translated `painting` names a struct `dart_ui` kept to itself -- 28
  /// `cannot find _NativePath`, and the same shape for `_NullWidget` and
  /// `_MaterialLocalizationsDelegate`.
  ///
  /// `pub(crate)` is what "private to its library, in a program that is one
  /// crate" actually means here. The name still starts with `_`, so a reader
  /// can still see what upstream considered private.
  String _vis(String dartName) =>
      dartName.startsWith('_') ? 'pub(crate) ' : 'pub ';

  String type(IrType t, {bool owned = true}) {
    if (t.isFunction) {
      // A parameter takes `impl Fn(..)`, which needs no allocation and lets the
      // caller pass a closure literal; anything owned -- a field, a return --
      // has to be `Box<dyn Fn(..)>`, since a closure's own type has no name.
      // A function type *inside* a function type's parameters cannot be
      // `impl Fn`: `Fn(impl Fn())` is not allowed in a trait bound. `&dyn Fn`
      // is, and borrows the same way.
      // A function type inside a function type's parameters is spelled as
      // every function type is now, `Rc<dyn Fn>`: a `&dyn Fn` there took
      // no `Rc` a caller had (`callbacker(Rc::new(|t, e| ..))`).
      final args = t.parameters!.map((p) => type(p, owned: false)).join(', ');
      final returns = type(t.returns!);
      final signature = 'Fn($args) -> $returns';
      // Inside a trait, `impl Fn(..)` is a generic parameter, and a trait with
      // one cannot be made into an object: `&dyn Element` stops compiling
      // everywhere the trait is used that way. 796 `E0038`s came from one
      // method, `Element.visitAncestorElements`, taking a callback. `&dyn Fn`
      // borrows exactly the same way and keeps the trait dyn-compatible, and
      // an `impl Fn` parameter elsewhere still accepts one.
      final spelled = owned
          // `Rc`, not `Box`: a Dart closure is an object, held by every
          // listener list it was added to at once, and `Box` claimed an
          // ownership Dart never gave -- `listener` was moved into a closure
          // "in a previous iteration" adding it to each child (E0382), and a
          // field holding one could not be cloned out.
          ? 'std::rc::Rc<dyn $signature>'
          // Shared, like every closure here: an `Rc<dyn Fn>` argument cannot
          // stand where `impl Fn` is asked for (`Rc` does not implement `Fn`),
          // and a borrowed `&dyn Fn` cannot be kept. One spelling, both sides.
          : 'std::rc::Rc<dyn $signature>';
      return t.nullable ? 'Option<$spelled>' : spelled;
    }
    // Dart's `dynamic` is "anything", which is what the prelude's `Object`
    // trait is here. Emitted as the bare word it was a type nothing declares,
    // 259 times.
    if (t.name == 'dynamic') {
      // Shared, like an abstract class below: a `Box` could not be cloned
      // out of a field (`SourceSpanException.source`), and a borrow could
      // not be kept.
      const anything = 'std::rc::Rc<dyn Object>';
      return t.nullable ? 'Option<$anything>' : anything;
    }
    if (library.isAbstract(t.name)) {
      // With the arguments: an abstract `Animatable<T>` is `dyn Animatable<T>`,
      // and dropping them made 477 uses wrong the moment traits became
      // generic. The name alone was consistent only while nothing had
      // parameters.
      final args = t.arguments.isEmpty
          ? ''
          : '<${t.arguments.map((a) => type(a)).join(', ')}>';
      // One spelling, both sides, as for closures: a parameter that was
      // `&dyn DynamicScheme` could not be the key of the `Map<Rc<dyn
      // DynamicScheme>, Hct>` the method caches into, and the `Rc` every
      // caller holds could not be passed to it -- 7 `E0308`s each way.
      final dynamic_ = 'std::rc::Rc<dyn ${t.name}$args>';
      return t.nullable ? 'Option<$dynamic_>' : dynamic_;
    }
    if (t.name == 'Record') {
      final tuple = '(${t.arguments.map((a) => type(a)).join(', ')})';
      return t.nullable ? 'Option<$tuple>' : tuple;
    }
    if (t.name == 'Map' && t.arguments.length == 2) {
      final map =
          'Map<${type(t.arguments[0])}, '
          '${type(t.arguments[1])}>';
      return t.nullable ? 'Option<$map>' : map;
    }
    // `dart:collection`'s internal classes as *types*, not just as
    // constructors. Round 77 mapped `_Set()` and left `_GrowableList<Color>`
    // standing in a `let`, which is the same name in the other position.
    final internal = _collections[t.name];
    if (internal != null) {
      final spelled = t.arguments.isEmpty
          ? internal
          : '$internal<${t.arguments.map((a) => type(a)).join(', ')}>';
      return t.nullable ? 'Option<$spelled>' : spelled;
    }
    // A bare `List` or `Future` -- Dart's, with no argument written -- holds
    // anything, which here is the prelude's `Object`. Without this the name
    // came out unadorned and Rust has no `List`.
    const anything = IrType('dynamic');
    if ((t.name == 'List' || t.name == 'Iterable' || t.name == 'Set') &&
        t.arguments.isEmpty) {
      return type(
        IrType(t.name, nullable: t.nullable, arguments: const [anything]),
        owned: owned,
      );
    }
    if (t.name == 'Map' && t.arguments.isEmpty) {
      return type(
        IrType(
          'Map',
          nullable: t.nullable,
          arguments: const [anything, anything],
        ),
        owned: owned,
      );
    }
    if (t.name == 'Future' && t.arguments.isEmpty) {
      return type(
        IrType('Future', nullable: t.nullable, arguments: const [anything]),
        owned: owned,
      );
    }
    if ((t.name == 'List' || t.name == 'Iterable') && t.arguments.length == 1) {
      final vec = 'Vec<${type(t.arguments.single)}>';
      return t.nullable ? 'Option<$vec>' : vec;
    }
    // `Future<T>` as a *type*, which is not the same as an `async fn`: a Rust
    // `async fn` returning `T` is already a future and drops the wrapper, but
    // a field that holds one, or a plain function that returns one, has to
    // name it. A future's own type has no name, so an owned position is
    // `Pin<Box<dyn Future>>` and a borrowed one is `impl Future` -- exactly
    // the split a function type already takes here.
    if (t.name == 'Future' && t.arguments.length == 1) {
      final output = type(t.arguments.single);
      final future = owned
          ? 'std::pin::Pin<std::boxed::Box<dyn std::future::Future<Output = $output>>>'
          : 'impl std::future::Future<Output = $output>';
      return t.nullable ? 'Option<$future>' : future;
    }
    // The doubled `Option` from `_substituteType`.
    if (t.name == 'Option' && t.arguments.length == 1) {
      return 'Option<${type(t.arguments.single)}>';
    }
    // A counted class is `Rc<Name>` everywhere it is named -- fields,
    // parameters, returns, locals. One rule here rather than 1150 edits.
    final owner = library[t.name];
    // Its own name included: a counted class's fields, parameters and
    // returns that name the class itself are handles too, as they are from
    // every other module. `impl` headers and constructors do not come
    // through here.
    if (owner != null && owner.counted) {
      final spelled =
          'std::rc::Rc<${t.name}${t.arguments.isEmpty ? '' : '<'
                    '${t.arguments.map((a) => type(a)).join(', ')}>'}>';
      return t.nullable ? 'Option<$spelled>' : spelled;
    }
    final mapped = _primitives[t.name] ?? t.name;
    // `Foo<int>` was coming out as a bare `Foo`, which is a different type.
    final spelled = t.arguments.isEmpty || _primitives.containsKey(t.name)
        ? mapped
        : '$mapped<${t.arguments.map((a) => type(a)).join(', ')}>';
    return t.nullable ? 'Option<$spelled>' : spelled;
  }

  // -- Expressions ------------------------------------------------------------

  String expr(IrExpr e) {
    return switch (e) {
      IrLiteral(:final value, :final type) => _literal(value, type),
      // A captured shared field is a cell handle, not the value: reading it
      // is `f.get()`. The local is only a local in the closure's own text.
      IrLocal(:final name) =>
        _cellLocals.containsKey(name)
            ? '${snake(name)}.${_cellLocals[name]! ? 'get()' : 'borrow().clone()'}'
            : snake(name),
      // `this` in a counted class is the handle -- one more `Rc`, not the
      // value behind it. `*self` there moved out of a `&Rc<Self>`, and the
      // getter `get owner => this` came out returning a bare struct where
      // every other module spells that class `Rc<..>`: 18 `E0053`s.
      // `Matrix3.copy(this)` in `clone()`: `*self` moves out of a shared
      // reference unless the class is `Copy`.
      // In a constructor `this` is the local being built (`__new`), a
      // value and not a reference: no `*`.
      IrThis() =>
        _selfIsHandle || !_classIsCopy(cls, {}) || _selfName != 'self'
            ? '$_selfName.clone()'
            : '*$_selfName',
      IrField(:final target, :final name, :final onEnum, :final owner) =>
        _fieldRead(target, name, onEnum, owner),
      IrStatic(:final owner, :final name, :final isEnumValue) => _staticRead(
        owner,
        name,
        isEnumValue,
      ),
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
      // Parenthesised: a struct literal is not allowed bare in an `if`
      // condition, and `if self._state == _State { .. } {` did not parse.
      IrConstInstance(:final type, :final fields) =>
        '(${_constInstance(type, fields)})',
      // Rust puts it after the expression and Dart before it, which is the
      // whole of the difference.
      IrAwait(:final operand) => '${expr(operand)}.await',
      IrIdentical(:final left, :final right) => _identical(left, right),
      // `return Err(e)` has type `!`, so it fits where a value was wanted.
      IrThrowValue(:final value) => _thrown(value),
      IrInterpolation(:final parts) => _interpolation(parts),
      // Dart indexes with an `int`; Rust wants a `usize`.
      // A clone: an indexed read is a value, and the element is behind the
      // list's reference (`cannot move out of index of Vec<..>`).
      IrIndex(:final target, :final index) =>
        '${expr(target)}[${expr(index)} as usize].clone()',
      // A closure literal among the elements of a list of functions is an
      // `Rc<dyn Fn>` there, as a field's or a constant's is: `DateFormat`'s
      // `_fieldConstructors` is a `vec!` of three of them.
      IrListLiteral(:final elements, :final element) =>
        'vec![${elements.map((x) => element.isFunction && x is IrClosure && !x.boxed ? 'std::rc::Rc::new(${expr(x)})' : expr(x)).join(', ')}]',
      IrRecord(:final fields) => '(${fields.map(expr).join(', ')})',
      IrRecordField(:final record, :final index) => '${expr(record)}.$index',
      IrMapLiteral(:final entries) =>
        'Map::from(['
            '${entries.map((e) => '(${expr(e.$1)}, ${expr(e.$2)})').join(', ')}'
            '])',
      // `for_each` consumes the chain and yields `()`: the one chain that is
      // whole without a `collect`.
      IrIterChain(:final steps)
          when steps.isNotEmpty && steps.last.$1 == 'for_each' =>
        _chain(e as IrIterChain),
      IrIterChain() => throw Unsupported(
        'a lazy Iterable that is never collected',
        'xs.map(..) with no toList()',
      ),
      // Boxed, because a function item is not a `Box<dyn Fn>` and that is what
      // a function-typed field or local is here. A `Box<dyn Fn>` also
      // implements `Fn`, so it still passes where `impl Fn` is wanted.
      IrFunctionRef(:final owner, :final name) =>
        owner == null
            ? 'std::rc::Rc::new(${snake(name)})'
            : (library[owner]?.isAbstract ?? false)
            ? 'std::rc::Rc::new(${_abstractStaticName(owner, name)})'
            : 'std::rc::Rc::new($owner::${snake(name)})',
      IrAssignValue(:final name, :final value) =>
        '{ let __set = ${expr(value)}; ${snake(name)} = __set; __set }',
      IrSetValue(:final target, :final name, :final value) => _setValue(
        target,
        name,
        value,
      ),
      IrConditional(:final condition, :final then, :final otherwise) =>
        'if ${expr(condition)} { ${expr(then)} } else { ${expr(otherwise)} }',
      IrIs(expr: final operand, :final type, :final negated) => _isTest(
        operand,
        type,
        negated,
      ),
      IrSuperCall(:final base, :final name, :final args) => _superCall(
        base,
        name,
        args,
      ),
      // A local's `!` clones first: `a!.axis` and then `a!.value` moved
      // `a` at the first (E0382); a `Copy` local clones for free.
      IrNullCheck(:final operand) =>
        operand is IrLocal
            ? '${expr(operand)}.clone().unwrap()'
            : '${expr(operand)}.unwrap()',
      // A closure inside `Some(..)` is the `Rc<dyn Fn>` its slot holds.
      IrSome(:final value) =>
        value is IrClosure && !value.boxed
            ? 'Some(std::rc::Rc::new(${expr(value)}))'
            : 'Some(${expr(value)})',
      // Inside `as_ref().map(|it| ..)` the bound value is a reference, and
      // a reference does not cast: `lerpDouble`'s `a as double` on an
      // `Option<f64>` (E0606).
      IrCast(:final value, :final rust) =>
        value is IrBound
            ? '(*${expr(value)} as $rust)'
            : '(${expr(value)} as $rust)',
      IrDowncast(:final target, :final type) =>
        '${expr(target)}.as_any().downcast_ref::<$type>().unwrap()',
      // A mutable one is read through its cell: two derefs for the `LazyLock`
      // and the `Isolate`, then a `borrow`.
      IrTopLevel(:final name) =>
        _isMutableTopLevel(name)
            ? '(**${screamingSnake(name)}).borrow().clone()'
            : _isLazyConst(name)
            ? '(**${screamingSnake(name)}).clone()'
            : screamingSnake(name),
      IrIsNull(:final operand) => '${expr(operand)}.is_none()',
      IrIfNull() => _ifNull(e as IrIfNull),
      // `as_ref()`: `a?.b` reads `a`, and `a` is a field or a loop variable
      // behind a reference far more often than an owned `Option` -- `.map`
      // alone moved out of `*child` (E0507). A body that needs the value
      // rather than a reference to it now says so at the use.
      IrNullAware(:final receiver, :final body, :final flatten) =>
        '${expr(receiver)}.as_ref().${flatten ? 'and_then' : 'map'}(|$_boundName| ${expr(body)})',
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
    final body = _out.sublist(saved).map(_inlineSafe).join(' ');
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
    final bindings = [
      // The handle first: a closure that calls a method keeps the object.
      if (node.holdsSelf) 'let $_countedSelf = $_selfName.clone();',
      ...node.captures.map((c) => 'let ${snake(c.name)} = ${_copyOf(c)};'),
      ...node.locals.map((l) => 'let ${snake(l)} = ${snake(l)}.clone();'),
    ].join(' ');
    // Which of them are cells, for the body that is about to be written.
    final savedCells = _cellLocals;
    _cellLocals = {
      ..._cellLocals,
      for (final c in node.captures)
        if (_sharedField(c.name) != null) c.name: _isCopy(type(c.type)),
    };
    final saved = _out.length;
    final savedIndent = _indent;
    final savedSelf = _selfName;
    // A closure is a panic boundary: its own signature carries no `Result`,
    // whatever the method around it promised (`_thrown`).
    final savedFailure = _failure;
    final savedFlow = _inFlowClosure;
    _failure = null;
    // Nor is it inside the try body's flow closure: a `return` in it is
    // the closure's own (`Ok(Some(..))` in `|x| builder.setDay(x)`).
    _inFlowClosure = false;
    if (node.holdsSelf) _selfName = _countedSelf;
    _indent = 0;
    stmt(node.body, tail: true);
    _failure = savedFailure;
    _inFlowClosure = savedFlow;
    _selfName = savedSelf;
    final body = _out.sublist(saved).map(_inlineSafe).join(' ');
    _out.removeRange(saved, _out.length);
    _indent = savedIndent;
    final owns =
        node.captures.isNotEmpty || node.locals.isNotEmpty || node.holdsSelf;
    // `async |..|` is stable since Rust 1.85. A Dart `async` closure keeps
    // its `await`s, and a closure emitted without the word put every one of
    // them outside an async context: 79 `E0728`s.
    final closure =
        '${node.isAsync ? 'async ' : ''}${owns ? 'move ' : ''}|$params| { $body }';
    _cellLocals = savedCells;
    final whole = owns ? '{ $bindings $closure }' : closure;
    return node.boxed ? 'std::rc::Rc::new($whole)' : whole;
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
  bool _inCell(IrFieldDecl field) =>
      field.shared || (cls.counted && _mutableOnCounted(field));

  /// On a counted class, what has to live in a cell: a field that is
  /// assigned, a `late` one (assigned after construction by definition), and
  /// a `final` collection -- `final Set<Image> _handles = {}` is never
  /// reassigned and is added to from `Image`'s constructor, which through a
  /// plain field behind an `Rc` cannot borrow mutably (E0596).
  bool _mutableOnCounted(IrFieldDecl field) =>
      !field.isFinal || field.isLate || _isMutableCollection(type(field.type));

  static bool _isMutableCollection(String rust) =>
      rust.startsWith('Vec<') ||
      rust.startsWith('Set<') ||
      rust.startsWith('Map<') ||
      rust.startsWith('Queue<') ||
      rust.startsWith('std::collections::VecDeque<');

  /// What a field holds, `Option`-wrapped when it is `late`.
  ///
  /// The wrapper goes *inside* the cell: a `late` field that a closure watches
  /// is `Rc<RefCell<Option<T>>>`, one cell holding one absent value, not two
  /// nested absences.
  String _heldType(IrFieldDecl field) {
    final held = type(field.type);
    return field.isLate ? 'Option<$held>' : held;
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
      return f.shared || (owned.counted && _mutableOnCounted(f)) ? f : null;
    }
    return null;
  }

  IrFieldDecl? _sharedField(String name) {
    for (final f in _allFields(cls)) {
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

  /// A copy of a field, for a closure to keep.
  ///
  /// `clone()` unless the type is `Copy`, where it would only be noise.
  String _copyOf(IrParam field) {
    final read = '$_selfName.${snake(field.name)}';
    // A copied `late` field is unwrapped here rather than in the body: the
    // closure holds a `T`, so the reads inside it are ordinary local reads.
    // It takes the value the field has when the closure is *made*, which is
    // the same trade round 97 made for every copied field.
    final late = _lateField(field.name);
    if (late != null && _sharedField(field.name) == null) {
      return _isCopy(type(late.type))
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
    // The lazy side as a `match`, not an `or_else(|| ..)`: a closure is its
    // own function, and an `.await` inside one -- `a ?? await b()` -- is
    // "await outside async". `match` keeps the laziness and stays in the
    // enclosing function. 13 `E0728`s.
    if (node.nullableResult) {
      return node.eager
          ? '$left.or($right)'
          : 'match $left { Some(__value) => Some(__value), None => $right }';
    }
    if (!node.eager) {
      return 'match $left { Some(__value) => __value, None => $right }';
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
  /// A Dart string as a Rust literal.
  ///
  /// The backslash and the quote were escaped from the start. The control
  /// characters were not, and a carriage return written raw into a Rust
  /// literal is a hard error -- `bare CR not allowed in string` -- 108 times
  /// across upstream, which mostly writes them inside `\r\n`.
  String _escape(String text) => text
      .replaceAll('\\', '\\\\')
      .replaceAll('"', '\\"')
      .replaceAll('\r', '\\r')
      .replaceAll('\n', '\\n')
      .replaceAll('\t', '\\t')
      .replaceAll('\u0000', '\\0')
      // Text-direction controls (the l10n files have them) are rejected raw
      // by rustc's `text_direction_codepoint_in_literal`; written as escapes
      // they are the same string. 23 literals.
      .replaceAllMapped(
        RegExp('[\u200E\u200F\u202A-\u202E\u2066-\u2069]'),
        (m) => '\\u{${m[0]!.codeUnitAt(0).toRadixString(16)}}',
      );

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
  /// A getter and a setter share a Dart name and must not share a Rust one.
  ///
  /// `RenderBox` has `Size get size` and `set size(Size)`, and both produced
  /// `render_box_super_size` -- the same collision round 62 found in the trait
  /// impls, one level over in the free functions that hold the bodies.
  static String superFn(
    String base,
    String name, {
    bool isSetter = false,
  }) => _rustIdentifier(
    '${snakeRaw(base)}_super_${isSetter ? 'set_' : ''}'
    '${RegExp(r'^[A-Za-z_][A-Za-z0-9_]*$').hasMatch(name) ? snakeRaw(name) : _operatorName(name)}',
  );

  /// A static call, checked against the IR when it lands in this library.
  ///
  /// `Alignment._stringify(x, y)` was emitted for a method the front end had
  /// refused, so the output named a function nobody wrote. That is round one's
  /// bug in a new shape: it was masked then by refusing every private reference,
  /// and removing that blunt rule brought it back. The precise rule is the same
  /// one `_superCall` uses -- if the callee is in this file, it has to be in the
  /// IR.
  /// Whether a top-level name is one of the library's mutable variables.
  bool _isMutableTopLevel(String name) =>
      library.constants.any((c) => c.name == name && c.isMutable) ||
      // Another module's: `numberFormatSymbols` read from `NumberFormat`
      // was a bare `NUMBER_FORMAT_SYMBOLS.get(..)` against its `LazyLock`.
      (library.constantsElsewhere[name]?.isMutable ?? false);

  /// `Fn(..) -> ..` for a function type, without the `impl`/`dyn`/`Box`.
  String _fnSignature(IrType t) {
    final args = t.parameters!
        .map(
          (p) =>
              p.isFunction ? '&dyn ${_fnSignature(p)}' : type(p, owned: false),
        )
        .join(', ');
    return 'Fn($args) -> ${type(t.returns!)}';
  }

  /// `List.generate(n, f)` and friends, which are Dart's list constructors
  /// wearing a static's clothes. Rust builds a `Vec` from an iterator.
  static const _listStatics = {'generate', 'filled', 'from', 'of'};

  String _staticCall(String? owner, String name, List<IrExpr> args) {
    // `Future.value(v)` is a future that is already done, which Rust spells
    // `ready`. `Future.delayed` and `Future.wait` need a runtime to be delayed
    // or joined *by*, and there is none, so they say so.
    if (owner == 'Future') {
      if (name == 'value' && args.length == 1) {
        return 'Box::pin(std::future::ready(${expr(args.single)}))';
      }
      throw Unsupported(
        '`Future.$name`, which needs an executor',
        'Future.$name(..)',
      );
    }
    // `int.parse` and `double.parse`. Dart's throw on bad input and its
    // `tryParse` returns null, which is `ok()`; `unwrap()` keeps the throw
    // loud rather than turning it into a zero.
    if (owner == 'int' || owner == 'double') {
      final rust = owner == 'int' ? 'i64' : 'f64';
      if (name == 'parse' && args.length == 1) {
        return '${expr(args.single)}.parse::<$rust>().unwrap()';
      }
      if (name == 'tryParse' && args.length == 1) {
        return '${expr(args.single)}.parse::<$rust>().ok()';
      }
      throw Unsupported('`$owner.$name`', '$owner.$name(..)');
    }
    // The runtime's own list classes reached as statics, the way
    // `_GrowableList.filled` is. Same names as the constructors, same answer.
    if ((_collections[owner] == 'Vec' || owner == 'List') &&
        _listStatics.contains(name)) {
      if (name == 'generate' && args.length == 2) {
        // `map` wants the closure itself, not the `Rc<dyn Fn>` a function
        // parameter would (E0277 in `plural_rules`); a function *value*
        // is called through one.
        final generator = args[1];
        final rendered = expr(generator);
        const boxed = 'std::rc::Rc::new(';
        // A closure renders boxed when it captures (`Rc::new({ let x =
        // x.clone(); move |i| .. })`); `map` wants the closure itself.
        String unboxed(String r) => r.startsWith(boxed) && r.endsWith(')')
            ? r.substring(boxed.length, r.length - 1)
            : r;
        final f =
            generator is IrCall &&
                generator.name == '!rc' &&
                generator.args.isEmpty
            ? unboxed(expr(generator.target!))
            : generator is IrClosure || rendered.startsWith(boxed)
            ? unboxed(rendered)
            : '|__i| ($rendered)(__i)';
        return '(0..${expr(args[0])}).map($f).collect::<Vec<_>>()';
      }
      if (name == 'filled' && args.length == 2) {
        return 'vec![${expr(args[1])}; ${expr(args[0])} as usize]';
      }
      if ((name == 'from' || name == 'of') && args.length == 1) {
        return '${expr(args[0])}.clone()';
      }
      // `List.from(xs, growable: false)`: a `Vec` is always growable and a
      // copy is a copy; the flag changes nothing that can be said here.
      if ((name == 'from' || name == 'of') && args.length == 2) {
        return '${expr(args[0])}.clone()';
      }
      if (name == 'empty' && args.isEmpty) return 'Vec::new()';
      throw Unsupported(
        '`$owner.$name` with ${args.length} arguments',
        '$owner.$name(..)',
      );
    }
    // `Float64List(9)`: a typed list of a length is that many zeros, which
    // is what Dart gives it. The untyped `_List(n)` is a list of *nulls*
    // and is handled in the front end; these cannot hold null at all.
    if (_typedLists.contains(owner) && name.isEmpty && args.length == 1) {
      // A typed zero: `Default::default()` left the element to inference,
      // and a `.map(|v| v as i64)` after it had nothing to go on (E0282).
      const zero = {
        'Float32List': '0.0f32',
        'Float64List': '0.0f64',
        'Int8List': '0i8',
        'Int16List': '0i16',
        'Int32List': '0i32',
        'Int64List': '0i64',
        'Uint8List': '0u8',
        'Uint8ClampedList': '0u8',
        'Uint16List': '0u16',
        'Uint32List': '0u32',
        'Uint64List': '0u64',
      };
      return 'vec![${zero[owner]}; ${expr(args.single)} as usize]';
    }
    // `Uint8List.fromList(xs)`: a typed list *is* a `Vec` here, so a copy.
    if (_typedLists.contains(owner) && name == 'fromList' && args.length == 1) {
      // `Float32List.fromList(doubles)`: a `Vec<f64>` narrowed element by
      // element (E0308 `Vec<f32>` vs `Vec<f64>` in `_MatrixImageFilter`).
      // The 64-bit ones are already what a `List<double>`/`List<int>` is.
      const element = {
        'Float32List': 'f32',
        'Int8List': 'i8',
        'Int16List': 'i16',
        'Int32List': 'i32',
        'Uint8List': 'u8',
        'Uint8ClampedList': 'u8',
        'Uint16List': 'u16',
        'Uint32List': 'u32',
        'Uint64List': 'u64',
      };
      final narrow = element[owner];
      if (narrow != null) {
        return '${expr(args.single)}.iter().map(|v| *v as $narrow).collect::<Vec<$narrow>>()';
      }
      return '${expr(args.single)}.clone()';
    }
    if (owner == null) {
      // A top-level function: no owner in either language. Checked against
      // what this file emits, for the same reason a static call is -- a call
      // to something refused would name a function nobody wrote.
      if (!library.functions.any((f) => f.name == name) &&
          !library.functionsElsewhere.contains(name) &&
          !_preludeFunctions.contains(name)) {
        throw Unsupported(
          'call to top-level `$name`, which was not translated',
          '$name(...)',
        );
      }
      return '${snake(name)}(${args.map(expr).join(', ')})';
    }
    // An **unnamed factory** is a `Procedure` whose name is the empty string,
    // and Kernel calls it like a static: `RegExp('..')` arrives as
    // `_staticCall('RegExp', '', ..)`. Its Rust name is the one every unnamed
    // constructor gets. Without this it reached the operator table and said
    // "operator `` has no Rust name" -- 367 times, naming neither the member
    // nor where it came from.
    if (name.isEmpty) {
      // The runtime's own collections first: `[]` is `_GrowableList(0)` in
      // Kernel, and the unnamed-factory rule below would spell that
      // `_GrowableList::new(0)` -- a module Rust has never heard of, 129
      // times. A length other than zero is a list of nulls, which is not an
      // empty `Vec`, so it is refused rather than flattened to one.
      final collection = _collections[owner];
      if (collection != null) {
        final empty =
            args.isEmpty ||
            (args.length == 1 &&
                args.single is IrLiteral &&
                (args.single as IrLiteral).value == '0');
        // `vec![]` for a list, which is what the list-literal path already
        // writes. One thing, one spelling: the two front ends reach an empty
        // list by different routes -- Kernel through `_GrowableList(0)` and
        // the analyzer through a literal -- and a fixture that compares text
        // sees any difference at all.
        if (empty) {
          return collection == 'Vec' ? 'vec![]' : '$collection::new()';
        }
        throw Unsupported(
          '`$owner` with a length, which is a list of nulls',
          '$owner(..)',
        );
      }
      // A factory of an abstract class -- `Characters(s)` -- is the static
      // named `new` of the trait's, a free function (see below); the struct
      // spelling `Characters::new` named a trait as a type.
      if (library.isAbstract(owner)) {
        return '${_abstractStaticName(owner, 'new')}(${args.map(expr).join(', ')})';
      }
      return '$owner::${_ctorName(null)}(${args.map(expr).join(', ')})';
    }
    final target = library[owner];
    if (target != null &&
        !target.methods.any((m) => m.name == name && m.operator == null)) {
      throw Unsupported(
        'call to `$owner.$name`, which was not translated',
        '$owner.$name(...)',
      );
    }
    if (owner == 'Object' && name == 'hashAll' && args.length == 1) {
      return 'object_hash_all(${expr(args.single)})';
    }
    if (owner == 'Object' && name == 'hash') {
      return 'object_hash(${args.map(expr).join(', ')})';
    }
    // `library.isAbstract`, not `library[owner]?.isAbstract`: an abstract
    // class of another module is in `abstractElsewhere` and nowhere else
    // (`Characters::new(..)` -- "expected a type, found a trait").
    if (library.isAbstract(owner)) {
      // A *factory* on an abstract class -- `Characters(s)` -- is the static
      // named `new` here, as the struct path names an unnamed constructor.
      final spelled = name.isEmpty ? 'new' : name;
      return '${_abstractStaticName(owner, spelled)}(${args.map(expr).join(', ')})';
    }
    return '$owner::${_identifier(name)}(${args.map(expr).join(', ')})';
  }

  String _superCall(String base, String name, List<IrExpr> args) {
    // `Object` is not a class this compiler has, and it never will be -- it is
    // the root every Dart class already inherits from. So `super.toString()`
    // was refused as "not in this file", 198 times, when the truth is that
    // there is no file. Dart's own `Object.toString` returns
    // `Instance of 'Foo'`, so that is what it translates to; upstream prints
    // exactly this for a class that overrides nothing.
    //
    // Only `toString`. `super.hashCode` and `super.==` are identity on the
    // object, which is a question about how objects are held -- the same
    // ownership question as the closures -- and they are two calls between
    // them, so they stay refused rather than guessed at.
    if (base == 'Object' && name == 'toString' && args.isEmpty) {
      return 'format!("Instance of \'{}\'", "${cls.name}")';
    }
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
    final call =
        '${superFn(base, name)}(${[_selfName, ...args.map(expr)].join(', ')})';
    // An async super function is an `async fn`; the caller's trait wants
    // the boxed future every `Future<T>` is here.
    final isAsync = baseClass.methods.any(
      (m) => m.name == name && !m.isStatic && m.isAsync,
    );
    return isAsync ? 'std::boxed::Box::pin($call)' : call;
  }

  /// Whether `base`'s free function for [name] can actually be emitted.
  ///
  /// `_superFailed` answers this for the class being emitted, but a super call
  /// is made from the *subclass*, whose backend never sees the base's set.
  static final _superFnProbes = <String, bool>{};

  bool _superFnEmits(IrClass baseClass, String name) {
    // Only an abstract class writes them. `_emitSuperFns` is called from
    // `_emitTrait` and nowhere else, because the free function is generic over
    // the trait -- there is nothing to make it generic over when the base is a
    // struct, since flattening copies the base's fields into each subclass
    // rather than leaving them anywhere shared. Probing without asking this
    // first said yes and the call named a function nobody wrote; the mixin
    // fixture is what walked into it.
    if (!baseClass.isAbstract) return false;
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

  /// Whether the signature being written belongs to a trait.
  var _inTrait = false;

  String _fieldRead(
    IrExpr? target,
    String name, [
    bool onEnum = false,
    String? owner,
  ]) {
    final receiver = _receiver(target);
    // A field of an *enum* is a getter here, not storage: the value is a
    // constant of the variant and lives in a `match`. Only the front end knows
    // -- the backend sees `state.value` with no idea what `state` is -- so it
    // says so on the node.
    if (onEnum) return '$receiver.${snake(name)}()';

    // Inside a trait every read on `this` is an accessor call: a trait has
    // no fields, and a mixin's `this_.source_url` names a getter of the
    // implementer's, declared in an interface the mixin never sees (7).
    if (_fieldsAreAccessors && (target == null || target is IrThis)) {
      return '$receiver.${snake(name)}()';
    }
    // A shared field is read through its cell. `get` copies, which is what a
    // Dart read does; `borrow().clone()` is the same for a value that is not
    // `Copy`.
    if (target == null || target is IrThis) {
      final shared = _sharedField(name);
      if (shared != null) {
        final held = _heldType(shared);
        final read = _isCopy(held)
            ? '$receiver.${snake(name)}.get()'
            : '$receiver.${snake(name)}.borrow().clone()';
        // Out of the cell it is a value, so the `late` unwrap is on a value
        // too. This is the one shape that does need `T: Clone`.
        return shared.isLate ? '$read.unwrap()' : read;
      }
      final late = _lateField(name);
      if (late != null) {
        // `as_ref()` rather than a clone: a read of a field is a place in
        // Rust, and `&T` is what the sites around it already expect. Only a
        // `Copy` value is taken out whole, which is what a place does anyway.
        // Cloned out, as every other field read is now: `as_ref()` handed
        // back a `&_ImageFilter` where the getter returns one by value (4).
        return _isCopy(type(late.type))
            ? '$receiver.${snake(name)}.unwrap()'
            : '$receiver.${snake(name)}.clone().unwrap()';
      }
    }
    // Another object's field, when the front end named its class and that
    // class keeps the field in a cell: read through the cell, as the write
    // side does. Without this the read was `entry.x` against a `RefCell`.
    if (owner != null) {
      final cell = _cellFieldOf(owner, name);
      if (cell != null) {
        final read = _isCopy(_heldType(cell))
            ? '$receiver.${snake(name)}.get()'
            : '$receiver.${snake(name)}.borrow().clone()';
        return cell.isLate ? '$read.unwrap()' : read;
      }
      // Another object's `late` field: `other._argb` in `Hct.==` is an
      // `Option<i64>` on that side too, and reads unwrap it as `this`'s do.
      final owned = library[owner];
      if (owned != null) {
        for (final f in _allFields(owned)) {
          if (f.name != name || !f.isLate) continue;
          return _isCopy(type(f.type))
              ? '$receiver.${snake(name)}.unwrap()'
              : '$receiver.${snake(name)}.clone().unwrap()';
        }
      }
    }
    // A read of one of this class's own fields is a *value*, and behind
    // `&self` a value that is not `Copy` has to be cloned out: `self._value`
    // moved out of a shared reference, 134 times in the leaf crates. A
    // method call on the clone or a borrow of it costs a clone and nothing
    // else.
    if (target == null || target is IrThis) {
      for (final f in _allFields(cls)) {
        if (f.name == name) {
          return _isCopy(type(f.type))
              ? '$receiver.${snake(name)}'
              : '$receiver.${snake(name)}.clone()';
        }
      }
    }
    // A field of a local: cloned out, as a field of `self` is -- `r._m3storage`
    // moved out of `r` and `r.clone()` two lines later was a partial move (9).
    // As a *receiver* the field is a place; `_receiver` spells that.
    // Another object of *this* class (`other as Hct`): its `late` field is
    // the same `Option`, unwrapped the same way.
    if (target is IrDowncast && target.type == cls.name) {
      final late = _lateField(name);
      if (late != null) {
        return _isCopy(type(late.type))
            ? '$receiver.${snake(name)}.unwrap()'
            : '$receiver.${snake(name)}.clone().unwrap()';
      }
    }
    if (target is IrLocal || target is IrBound || target is IrDowncast) {
      return '$receiver.${snake(name)}.clone()';
    }
    return '$receiver.${snake(name)}';
  }

  /// `x is Foo`.
  ///
  /// Rust answers it with `Any`, which downcasts to a *concrete* type: the
  /// trait object says what it holds, and holding is always a struct. So a
  /// target that is itself abstract has no answer here -- `x is RenderBox`
  /// asks whether the thing implements a trait, which `Any` cannot say -- and
  /// is still refused, now under a name that says which half is missing.
  String _isTest(IrExpr operand, IrType target, bool negated) {
    final name = target.name;
    if (library.isAbstract(name)) {
      throw Unsupported('`is` against an abstract class', name);
    }
    // `x is num` / `is int` / `is String` on a `dynamic`: the prelude's
    // scalar types, asked of `Any`. A `num` is either an `f64` or an `i64`.
    const scalars = {
      'int': ['i64'],
      'double': ['f64'],
      'num': ['f64', 'i64'],
      'bool': ['bool'],
      'String': ['String'],
    };
    if (scalars.containsKey(name)) {
      final tests = scalars[name]!
          .map(
            (t) => '${expr(operand)}.as_any().downcast_ref::<$t>().is_some()',
          )
          .join(' || ');
      return negated ? '!($tests)' : '($tests)';
    }
    if (library[name] == null) {
      throw Unsupported('`is` against `$name`, which was not translated', name);
    }
    final arguments = target.arguments.isEmpty
        ? ''
        : '<${target.arguments.map(type).join(', ')}>';
    return '${expr(operand)}.as_any()'
        '.downcast_ref::<$name$arguments>().${negated ? "is_none" : "is_some"}()';
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

  /// Whether the method being emitted is `async`, for the constructs that
  /// must not wrap an `.await` in a closure.
  var _asyncBody = false;

  /// Wraps a returned expression when the declared return is a trait object.
  ///
  /// Only an `IrNew` is wrapped, because only a constructor call is *known* to
  /// produce that concrete type. Anything else could already be a box, and a
  /// double `Box::new` compiles into something quietly wrong.
  String _returned(IrExpr value) {
    final declared = _returns;
    final text = expr(value);
    // A closure returned from a function is an *owned* position, and a
    // closure's own type has no name -- so the declared type is
    // `Box<dyn Fn(..)>` and the value has to be boxed to match. This only
    // came up once closures that outlive their call stopped being refused.
    if (declared != null && declared.isFunction && value is IrClosure) {
      return 'std::rc::Rc::new($text)';
    }
    // `dynamic` and `Object` are trait objects too (`Rc<dyn Object>`):
    // `error = Exception(..)` into a `dynamic` local needs the same `Rc::new`.
    if (declared != null &&
        (library.isAbstract(declared.name) ||
            declared.name == 'dynamic' ||
            declared.name == 'Object') &&
        (value is IrNew || value is IrConstInstance) &&
        !library.isAbstract(_concreteType(value).name) &&
        // A counted class's constructor already hands out an `Rc`, which
        // unsizes on its own; wrapping it again was `Rc<Rc<X>>`.
        !(library[_concreteType(value).name]?.counted ?? false)) {
      return 'std::rc::Rc::new($text)';
    }
    // Each branch of a conditional on its own: `s.isEmpty ? StringCharacters
    // ("") : StringCharacters(s)` returned as a `Characters`.
    if (value is IrConditional) {
      return 'if ${expr(value.condition)} { ${_returned(value.then)} } '
          'else { ${_returned(value.otherwise)} }';
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

  /// Rust names of the collection methods that change their receiver.
  static const _inPlace = {
    'push',
    'insert',
    'remove',
    'clear',
    'extend',
    'add',
    'retain',
    'truncate',
    'pop',
    'sort',
    'sort_by',
    'reverse',
    'swap',
    'drain',
    'remove_at',
    'insert_all',
    'remove_where',
    'retain_where',
    'add_all',
    'remove_last',
    'remove_first',
    'push_back',
    'push_front',
    'pop_front',
    'pop_back',
    'remove_all',
    'set_range',
    'fill_range',
    'shuffle',
    'add_first',
    'add_last',
    'put_if_absent',
    'update',
    'remove_range',
    'replace_range',
    'set_all',
  };

  static bool _mutatesInPlace(String name) => _inPlace.contains(name);

  /// The cell a field read would go through, as a place -- `self.x` or
  /// `other.x` -- when the field is kept in a `RefCell`; null otherwise.
  String? _cellPlace(IrExpr? target) {
    if (target is! IrField) return null;
    final base = target.target;
    final IrFieldDecl? cell;
    if (base == null || base is IrThis) {
      cell = _sharedField(target.name);
    } else if (target.owner != null) {
      cell = _cellFieldOf(target.owner!, target.name);
    } else {
      cell = null;
    }
    if (cell == null || _isCopy(_heldType(cell))) return null;
    final holder = base == null || base is IrThis ? _selfName : expr(base);
    return '$holder.${snake(target.name)}';
  }

  String _receiver(IrExpr? target) {
    if (target == null || target is IrThis) return _selfName;
    // `local.field.method(..)`: the field is the place the method acts on,
    // not the clone a value read takes.
    if (target is IrField && target.target is IrLocal && target.owner == null) {
      return '${expr(target.target!)}.${snake(target.name)}';
    }
    return expr(target);
  }

  String _call(IrExpr? target, String name, List<IrExpr> args) {
    // Before the receiver is rendered: rendering a chain on its own is
    // refused, and this is the one place a chain is not on its own.
    // Any arguments, not none: Dart's `toList({bool growable = true})` has a
    // named parameter, and the Kernel front end fills in its default -- so the
    // chain was collected on one side and refused on the other.
    if (name == 'to_list' && target is IrIterChain) {
      return '${_chain(target)}.collect::<Vec<_>>()';
    }
    // `0.29.powf(x)`: a float literal as a receiver is an "ambiguous numeric
    // type" until it says which (21 `E0689`s in the HCT colour code).
    // `self._handles.add(x)` on a field kept in a cell: the cell is the
    // place, and a mutating call goes through `borrow_mut()`. Read out as a
    // value first -- `.borrow().clone().push(x)` -- it compiled and pushed
    // onto a copy: 27 such silent no-ops in the leaf crates.
    // `recorder as _NativePictureRecorder` where the class is counted: the
    // downcast through `Any` yields the struct inside the `Rc<dyn Trait>`,
    // and every holder of that class wants an `Rc<_NativePictureRecorder>`.
    // A new handle around a clone: the fields are cells, so the state is
    // still shared; only the handle's identity is new.
    if (name == 'clone' &&
        args.isEmpty &&
        target is IrDowncast &&
        (library[target.type]?.counted ?? false)) {
      return 'std::rc::Rc::new(${expr(target)}.clone())';
    }
    // `runtimeType` on a super function's `this_` (see `DartAny`).
    if ((name == 'runtimeType' || name == 'runtime_type') &&
        args.isEmpty &&
        (target == null || target is IrThis) &&
        _selfName == 'this_') {
      return 'this_.dart_runtime_type()';
    }
    final cellPlace = _mutatesInPlace(name) ? _cellPlace(target) : null;
    final receiver = cellPlace != null
        ? '$cellPlace.borrow_mut()'
        : target is IrLiteral && target.type.name == 'double'
        ? '(${_receiver(target)}_f64)'
        : _receiver(target);
    // `HashMap` looks up by reference, and gives back a reference to the
    // value. Dart's `m[k]` is a `V?`, so the borrow is cloned away rather
    // than leaked into every caller's type.
    // A value shared into a trait object (see `_widened`).
    if (name == '!rc' && args.isEmpty) return 'std::rc::Rc::new($receiver)';
    // An `Option<Rc<dyn Object>>` into a `dynamic` slot: absent is `Null`.
    if (name == '!or_null' && args.isEmpty) {
      return '$receiver.unwrap_or_else(|| std::rc::Rc::new(Null) as std::rc::Rc<dyn Object>)';
    }
    // The other way: a `Uint8List` handed to a `List<int>` parameter.
    // A `List<String>` into a `List<Object?>`: each element shared.
    if (name == '!widen_object' && args.isEmpty) {
      // `iter().cloned()`: the receiver may be the `&Vec` a null-aware
      // `as_ref().map(|it| ..)` binds, and `into_iter` on that yields
      // references (E0282 in `ColorFilter.hashCode`).
      return '$receiver.iter().cloned().map(|v| Some(std::rc::Rc::new(v) as std::rc::Rc<dyn Object>)).collect::<Vec<_>>()';
    }
    if (name == '!widen' && args.isEmpty) {
      return '$receiver.into_iter().map(|v| v as i64).collect::<Vec<i64>>()';
    }
    if (name == '!narrow' && args.length == 1) {
      final to = expr(args.single);
      return '$receiver.into_iter().map(|v| v as $to).collect::<Vec<$to>>()';
    }
    // Into `Rc<dyn Object>` by name: inside a `.map(|it| ..)` the unsizing
    // has nothing to infer it from.
    // A `dynamic` asked whether it is a `T`: the `Option<T>` `Any` gives.
    if (name == '!as_opt' && args.length == 1) {
      return '$receiver.as_any().downcast_ref::<${expr(args.single)}>().cloned()';
    }
    if (name == '!as_object' && args.isEmpty) {
      // `this` into an `Object` slot: the handle when the method holds
      // one, a fresh `Rc` of a clone when it does not.
      if (target is IrThis) {
        return _selfIsHandle
            ? '($_selfName.clone() as std::rc::Rc<dyn Object>)'
            : '(std::rc::Rc::new($_selfName.clone()) as std::rc::Rc<dyn Object>)';
      }
      return '($receiver as std::rc::Rc<dyn Object>)';
    }
    if (name == '!rc_object' && args.isEmpty) {
      return '(std::rc::Rc::new($receiver) as std::rc::Rc<dyn Object>)';
    }
    if (name == '!dart_eq' && args.length == 1) {
      return '$receiver.dart_eq(&${expr(args.single)})';
    }
    // `Vec::contains` takes a reference; Dart's takes the value. Only the
    // List's: `Path.contains(Offset)` is a method of its own.
    if (name == '!contains' && args.length == 1) {
      return '$receiver.contains(&${expr(args.single)})';
    }
    if (name == '!expando_get' && args.length == 1) {
      return '$receiver.get(&${expr(args.single)})';
    }
    if (name == '!map_get' && args.length == 1) {
      return '$receiver.get(&${expr(args.single)}).cloned()';
    }
    // `_views[_implicitViewId]` with an `int?` key: Dart looks up `null`
    // and finds nothing; here the absent key is the absent value.
    if (name == '!map_get_opt' && args.length == 1) {
      return '${expr(args.single)}.as_ref().and_then(|__k| $receiver.get(__k).cloned())';
    }
    if ((name == 'contains_key' || name == 'remove') && args.length == 1) {
      return '$receiver.$name(&${expr(args.single)})';
    }
    // The List and Map members Rust says differently rather than renames.
    if (name == '!is_empty' && args.isEmpty) return '!$receiver.is_empty()';
    // `iter()` yields references and the closure is written for values, so
    // the parameter types come off exactly as `_chain` takes them off.
    // `cloned()`, because the Dart closure is written for a value and
    // `iter()` yields a reference: `|x| x > limit` against a `&i64` is
    // `expected &i64, found i64`. The chain steps get away with `iter()`
    // because what they produce is collected, not compared.
    if (name == '!any' && args.length == 1) {
      return '$receiver.iter().cloned().any(${_stepClosure(args.single)})';
    }
    if (name == '!every' && args.length == 1) {
      return '$receiver.iter().cloned().all(${_stepClosure(args.single)})';
    }
    if (name == '!to_set' && args.isEmpty) {
      return 'Set::from($receiver.clone())';
    }
    // Dart joins with the empty string when nothing is given -- and the
    // Kernel front end fills that default in while the analyzer one leaves it
    // off, so the omitted argument has to be recognised rather than trusted to
    // be absent. The fixtures said so: the two sides wrote `join("")` and
    // `join(&"".to_string())` for one line of Dart.
    if (name == '!join' && args.length < 2) {
      final given = args.where((a) => !_isDefault(a, '')).toList();
      final separator = given.isEmpty ? '""' : '&${expr(given.single)}';
      return '$receiver.iter().map(|__e| dart_str(__e))'
          '.collect::<Vec<_>>().join($separator)';
    }
    if (name == '!insert' && args.length == 2) {
      return '$receiver.insert(${expr(args[0])} as usize, ${expr(args[1])})';
    }
    if (name == '!remove_at' && args.length == 1) {
      return '$receiver.remove(${expr(args.single)} as usize)';
    }
    if (name == '!element_at' && args.length == 1) {
      return '$receiver[${expr(args.single)} as usize]';
    }
    if (name == '!sublist' && args.isNotEmpty && args.length < 3) {
      // `sublist(from)` arrives with an explicit `null` end from Kernel and
      // with nothing from the analyzer. Both mean "to the end".
      final given = args.where((a) => !_isDefault(a, null)).toList();
      // The end is `int?` upstream, so it arrives as `Some(e)`: the value.
      final endValue = given.length == 1 ? null : given[1];
      final end = endValue == null
          ? ''
          : '${expr(endValue is IrSome ? endValue.value : endValue)} as usize';
      return '$receiver[${expr(given[0])} as usize..$end].to_vec()';
    }
    // Dart's `reversed` is a lazy Iterable and nearly every use ends in
    // `toList`. A `Vec` is what that produces, and `to_list` on one clones.
    if (name == '!reversed' && args.isEmpty) {
      return '{ let mut __r = $receiver.clone(); __r.reverse(); __r }';
    }
    if (name == '!cast' && args.isEmpty) return receiver;
    if (name == 'first' && args.isEmpty) return '$receiver[0].clone()';
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
    if (name == 'toDouble' && args.isEmpty) return '($receiver as f64)';
    // A call to a method of this class that can fail carries the failure
    // outward with `?`. That is the propagation the measurement counted, and
    // the caller's own signature was widened by the same fixpoint, so the two
    // always agree.
    // A callee failing with its own type inside a method failing with
    // `Object` (see `_computeFailing`): the error is boxed on the way up.
    final calleeError = _failing[snake(name)];
    final propagates =
        (target == null || target is IrThis) &&
        calleeError != null &&
        !_traitDeclares(name);
    final widens =
        propagates &&
        calleeError != 'Object' &&
        (_failure == 'Object' || _failure == 'std::rc::Rc<dyn Object>');
    final suffix = !propagates
        ? ''
        : widens
        ? '.map_err(|e| std::rc::Rc::new(e) as std::rc::Rc<dyn Object>)?'
        : '?';
    // `_identifier`, not `snake`: an *operator* called as a method -- `~x` is
    // `x.~()` in Kernel -- has no letters for `snake` to keep, and it came out
    // as `x._()`, which does not parse and stopped the whole crate at the
    // lexer. `_identifier` gives the operator the same name its definition
    // got, and refuses the ones with no Rust name at all.
    return '$receiver.${_identifier(name)}'
        '(${args.map(expr).join(', ')})$suffix';
  }

  /// `Alignment { x: -1.0, y: -1.0 }`.
  ///
  /// Only for a class this file emits. The struct literal names fields, and the
  /// only fields whose Rust names are known are the ones written here -- a
  /// `Duration { _duration: 1000 }` would be naming a field of a hand-written
  /// stub and would go wrong quietly the day the stub was spelled differently.
  String _constInstance(IrType t, Map<String, IrExpr> fields) {
    // The prelude's types are not in the IR, so nothing here knows their
    // fields -- and two of them account for 276 of the 305 refusals.
    //
    // `Duration` carries one field, `inMicroseconds`, which is the prelude's
    // `microseconds` under another name. `SentinelValue` carries none: it is
    // dart:core's "no argument was passed" marker, and what upstream does with
    // it is compare identities, so an empty struct says everything it says.
    if (t.name == 'Duration') {
      final micros = fields['inMicroseconds'] ?? fields['_duration'];
      if (micros != null) {
        return 'Duration { microseconds: ${expr(micros)} }';
      }
    }
    if (t.name == 'SentinelValue' && fields.isEmpty) {
      return 'SentinelValue';
    }
    // `Endian.little`/`Endian.big`: the prelude's enum, from the constant's
    // one field. Every `Paint` getter reads a `ByteData` with one (14).
    if (t.name == 'Zone' && fields.isEmpty) return 'Zone';
    if (t.name == 'Utf8Codec') return 'Utf8Codec';
    if (t.name == 'Endian') {
      final little = fields['_littleEndian'];
      return little != null && expr(little) == 'true'
          ? 'Endian::Little'
          : 'Endian::Big';
    }
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
    // A field declared as a trait object takes the same `Rc::new` a return
    // does: `const _ClampTransform(_P3ToSrgbTransform())` holds its child
    // as an `Rc<dyn _ColorTransform>`.
    final parts = <String>[];
    for (final f in _allFields(cls)) {
      if (!fields.containsKey(f.name)) continue;
      final outer = _returns;
      _returns = f.type;
      parts.add('${snake(f.name)}: ${_returned(fields[f.name]!)}');
      _returns = outer;
    }
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
    // A counted class's field is a cell: `_count++` used for its value
    // wrote `self._count = __set` against an `Rc<Cell<i64>>`.
    final shared = (target == null || target is IrThis)
        ? _sharedField(name)
        : null;
    if (_fieldsAreAccessors && (target == null || target is IrThis)) {
      return '{ let __set = ${expr(value)}; $receiver.set_${snake(name)}(__set.clone()); __set }';
    }
    if (shared != null) {
      final copy = _isCopy(_heldType(shared));
      return copy
          ? '{ let __set = ${expr(value)}; $receiver.${snake(name)}.set(__set); __set }'
          : '{ let __set = ${expr(value)}; '
                '*$receiver.${snake(name)}.borrow_mut() = __set.clone(); __set }';
    }
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

  /// Whether an argument is the omitted one, written out.
  ///
  /// Kernel fills a default in and the analyzer leaves it off, so a member
  /// whose Rust says the absent case differently has to see through that.
  static bool _isDefault(IrExpr e, String? empty) =>
      e is IrLiteral &&
      (empty == null
          ? e.type.name == 'Null'
          : e.type.name == 'String' && e.value == empty);

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
    final body = _out.sublist(saved).map(_inlineSafe).join(' ');
    _out.removeRange(saved, _out.length);
    _indent = savedIndent;
    // The fields the closure copies in, as `_closure` does for the boxed
    // kind. A chain step that read `this.trashEmailIds` named a local that
    // this line had not declared.
    final copies = e.captures
        .map((c) => 'let ${snake(c.name)} = ${_copyOf(c)}; ')
        .join();
    return '|$params| { $copies$body }';
  }

  /// A read of a static, or of an enum value.
  ///
  /// A Dart `static final` becomes a module-level `LazyLock`, because an
  /// `impl` block may hold a `const` and not a `static`. So its name carries
  /// its class, and reading it dereferences the lock.
  String _staticRead(String owner, String name, bool isEnumValue) {
    // The owner's own spelling, which may be Dart's: see `variantNames`.
    if (isEnumValue) {
      final owned = library[owner];
      final names = owned == null ? null : variantNames(owned.values);
      return '$owner::${names?[name] ?? variantName(name)}';
    }
    // `dart:io`'s `Platform.version` and friends: the prelude's functions.
    if (owner == 'Platform') return 'platform_${snake(name)}()';
    // Two derefs: through the `LazyLock`, then through the `Isolate` that
    // carries "one per isolate, not one per process".
    if (_isMutableStatic(owner, name)) {
      return '(**${_lazyName(owner, name)}).borrow().clone()';
    }
    // A clone: the lock hands out a reference, and a read is a value.
    // `(**CHANGE_NOTIFIER__EMPTY_LISTENERS)` moved out of the lock (E0507).
    if (_isLazy(owner, name)) return '(**${_lazyName(owner, name)}).clone()';
    if (library[owner]?.isAbstract ?? false) {
      return screamingSnake('${owner}_$name');
    }
    return '$owner::${screamingSnake(name)}';
  }

  bool _isMutableStatic(String owner, String name) =>
      library[owner]?.constants.any((c) => c.name == name && c.isMutable) ??
      false;

  bool _isLazy(String owner, String name) =>
      library[owner]?.constants.any((c) => c.name == name && c.isLazy) ?? false;

  static String _lazyName(String owner, String name) =>
      screamingSnake('${owner}_$name');

  /// Whether a case value can be written as a Rust pattern.
  ///
  /// An enum variant and an integer or boolean literal can. A string cannot --
  /// `"x".to_string()` is a call -- and neither can anything computed.
  static bool _isPattern(IrExpr e) => switch (e) {
    IrStatic(:final isEnumValue) => isEnumValue,
    IrLiteral(:final type) => type.name == 'int' || type.name == 'bool',
    _ => false,
  };

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
    // Two locals, or a local against a static: the addresses of the *slots*.
    // Two distinct slots are never the same address, so this says "not
    // identical" -- which is what Dart says of two distinct objects, and is
    // the fast-path answer `listEquals` and `setEquals` want before they
    // compare elements. What it cannot see is two handles to one `Rc`: those
    // read as distinct here where Dart would say identical. `_invoke`'s
    // `identical(zone, Zone.current)` is the one site that asks, and the
    // prelude has a single zone, so both branches run the callback the same
    // way. 36 call sites were behind this.
    bool slot(IrExpr e) => e is IrLocal || e is IrStatic;
    // A slot against a static *call* -- `identical(zone, Zone.current)`,
    // the one site, in `_invoke` and its siblings (18 callers of those) --
    // binds the call and compares slots: distinct, as above.
    if (slot(left) && right is IrStaticCall) {
      return '{ let __i = ${expr(right)}; std::ptr::eq(&${expr(left)}, &__i) }';
    }
    if (left is IrStaticCall && slot(right)) {
      return '{ let __i = ${expr(left)}; std::ptr::eq(&__i, &${expr(right)}) }';
    }
    // Only when `_addressOf` has no better answer: a counted class's handle
    // is dereferenced below, and that path must keep winning for `Rc`s.
    if (slot(left) &&
        slot(right) &&
        (!_isReference(left) || !_isReference(right))) {
      return 'std::ptr::eq(&${expr(left)}, &${expr(right)})';
    }
    // `identical(x, 0)` / `identical(s, 'und')`: on a number, a string or
    // a bool Dart's `identical` is value equality (`KeyData._nonValueBits`,
    // `Locale.toString`).
    // `identical(_cachedLocale, this)` on a value class: the struct has no
    // identity to compare, so the cache never hits and is recomputed --
    // the same answer Dart gives for a fresh object, every time.
    if (!cls.counted &&
        ((left is IrStatic && right is IrThis) ||
            (left is IrThis && right is IrStatic))) {
      return 'false';
    }
    if (left is IrLiteral || right is IrLiteral) {
      // TFA folds both sides to literals of different kinds: `identical(0,
      // 0.0)` is `false` in Dart, and `0 == 0.0` does not type in Rust.
      String side(IrExpr e, IrExpr other) =>
          e is IrLiteral &&
              e.type.name == 'int' &&
              other is IrLiteral &&
              other.type.name == 'double'
          ? '(${expr(e)} as f64)'
          : expr(e);
      return '(${side(left, right)} == ${side(right, left)})';
    }
    if (!_isReference(left) || !_isReference(right)) {
      // The question is not "is one side `this`" -- it is whether both sides
      // are *references* in the emitted Rust. A parameter of a concrete type
      // arrives by value, because a translated value type is `Copy`, and the
      // address of a copy answers nothing: `identical(this, other)` there would
      // compile and always be false.
      // Which side, and what it is. "identical(.., ..)" 251 times said only
      // that something was wrong; the shapes are what the next round needs.
      String what(IrExpr e) => switch (e) {
        IrThis() => 'this',
        IrLocal(:final name) => name,
        IrField(:final name) => 'a field `$name`',
        _ => e.runtimeType.toString(),
      };
      throw Unsupported(
        '`identical` on something that is not a reference',
        '${_isReference(left) ? "" : "${what(left)} "}'
            '${_isReference(right) ? "" : what(right)}',
      );
    }
    // Through `*const ()` because the two sides have different Rust types --
    // `&Self` and `&dyn Trait` -- and identity is about the address, which both
    // of them have.
    return 'std::ptr::eq('
        '${_asPointer(left)}, '
        '${_asPointer(right)})';
  }

  /// Whether this expression is a reference in the emitted Rust.
  ///
  /// `self` always is. A local is one when it is a parameter whose Dart type is
  /// an abstract class, since that becomes `&dyn Trait` -- which is what
  /// upstream's `operator ==(Object other)` is.
  bool _isReference(IrExpr e) => _addressOf(e) != null;

  /// How to take the address of a value, or null when it has none to take.
  ///
  /// "Has an address" is not the same as "is written as a reference". A
  /// counted class arrives as an `Rc<Foo>` *by value*, and two handles to one
  /// object are two different addresses -- so the handle is dereferenced and
  /// the pointee's address is what identity is asked about. Getting that
  /// backwards answers the opposite of the question and compiles.
  /// One side of `identical` as a thin pointer. A parameter holding an
  /// `Rc<dyn X>` (see `_referenceParams`) is the handle, not a reference,
  /// and `Rc::as_ptr` is its address; `x as *const _` was an invalid cast.
  String _asPointer(IrExpr e) {
    final r = _ref(e);
    if (e is IrLocal && !r.startsWith('&')) {
      return 'std::rc::Rc::as_ptr(&$r) as *const u8 as *const ()';
    }
    return '$r as *const _ as *const ()';
  }

  String? _addressOf(IrExpr e) {
    if (e is IrThis) {
      // A closure's `__me` is a cloned handle, and a method that hands out
      // `this` takes `&Rc<Self>`. Both need one more deref than they look.
      if (_selfName == _countedSelf) return '&*$_countedSelf';
      return _selfIsHandle ? '&**$_selfName' : _selfName;
    }
    if (e is IrLocal) return _referenceParams[e.name];
    return null;
  }

  /// Parameters of the method being emitted that have an address, and how to
  /// take it.
  var _referenceParams = <String, String>{};

  /// Whether `self` here is `&Rc<Self>` rather than `&Self`.
  var _selfIsHandle = false;

  /// `self` is already a reference; anything else names one.
  String _ref(IrExpr e) => _addressOf(e)!;

  /// `dart:collection`'s internal implementation classes, by what they are.
  ///
  /// A Dart `<T>{}` resolves, in Kernel, to a constructor of `_Set`; a list
  /// literal that can grow is a `_GrowableList`. Those names are the runtime's
  /// own and nothing outside it declares them, so they came out as
  /// `_Set::new()` -- a module Rust has never heard of, 40 times.
  ///
  /// Only the empty constructors. `_GrowableList(n)` and `_List.filled` mean
  /// something else and are left to refuse rather than be guessed at.
  static const _collections = {
    '_Set': 'Set',
    '_LinkedHashSet': 'Set',
    '_CompactLinkedHashSet': 'Set',
    '_HashSet': 'Set',
    '_Map': 'Map',
    '_LinkedHashMap': 'Map',
    '_InternalLinkedHashMap': 'Map',
    '_HashMap': 'Map',
    '_GrowableList': 'Vec',
    '_List': 'Vec',
  };

  String _new(IrType t, List<IrExpr> args, String? constructor) {
    final collection = _collections[t.name];
    if (collection != null) {
      if (args.isNotEmpty) {
        throw Unsupported(
          '`${t.name}` with arguments, which is not the empty collection',
          '${t.name}(..)',
        );
      }
      final arguments = t.arguments.isEmpty
          ? ''
          : '::<${t.arguments.map((a) => type(a)).join(', ')}>';
      return '$collection$arguments::new()';
    }
    // An abstract class is a trait here, and a trait has no constructor to
    // call. Dart's `Gradient.linear(..)` is a factory on an abstract class,
    // and the type of one is `Box<dyn Gradient>` -- so the call came out as
    // `Box<dyn Gradient>::linear(..)`, which does not even parse. What it
    // should name is whichever concrete class the factory redirects to, and
    // that is not known here.
    if (library.isAbstract(t.name)) {
      throw Unsupported(
        'a constructor of `${t.name}`, which is abstract and became a trait',
        '${t.name}(..)',
      );
    }
    // `Pair::<i64, f32>::new(..)`, not `Pair<i64, f32>::new(..)`: in an
    // *expression* Rust wants the turbofish, and the plain form does not parse.
    // The *name*, not the type: a counted class's type is `Rc<Foo>` and its
    // constructor is `Foo::new`, which hands one out. Spelling the type here
    // wrote `Rc<Foo>::new()`, which does not parse.
    final counted = library[t.name]?.counted ?? false;
    final name = t.arguments.isEmpty
        // The bare name: `type(t)` of an argument-less `Map` fills in its
        // `Rc<dyn Object>` arguments, and `Map<K, V>::new()` needs a
        // turbofish to parse (`comparison operators cannot be chained`).
        ? (counted || type(t).contains('<') ? t.name : type(t))
        : '${t.name}::<${t.arguments.map((a) => type(a)).join(', ')}>';
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
    found.addAll(inExpressions.mutatedLocals);
    found.addAll(inExpressions.receiverLocals);
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
        // `xs[i] = v` needs `xs` mutable, which nothing here said. It only
        // shows on a list written through a name rather than through `self`.
        case IrIndexSet(:final target):
          if (target is IrLocal) found.add(target.name);
        case IrLocalFunction():
        case IrBreak():
        case IrContinue():
        case IrReturn():
        case IrLocalDecl():
        case IrExprStmt():
        case IrAssert():
        case IrSetter():
        case IrThrow():
        case IrAssignField():
        case IrAssignTopLevel():
        case IrAssignStatic():
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
        // A thrown string where the function's error type is `Object` -- the
        // tree shaker's "code removed by TFA" throws, 36 of them -- is boxed
        // into the error type rather than left as a `String` in an `Rc`'s
        // place.
        _line('${_thrown(value)};');
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
        // In an `async fn` the body goes in an `async` block, not a closure:
        // a closure is its own function and an `.await` inside it is
        // "outside async" -- 13 `E0728`s, every one a `try` around an
        // `await`. The block has the same `return` semantics.
        _line(
          _asyncBody
              ? 'let __finally: Result<$carried, $failure> = async {'
              : 'let __finally = (|| -> Result<$carried, $failure> {',
        );
        _indent++;
        final wasFlowing = _inFlowClosure;
        _inFlowClosure = flows;
        stmt(body);
        _inFlowClosure = wasFlowing;
        _line('#[allow(unreachable_code)]');
        _line(flows ? 'Ok(None)' : 'Ok(())');
        _indent--;
        _line(_asyncBody ? '}.await;' : '})();');
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
        // A method that cannot fail wrapped its body in `Infallible`, and
        // the arm is impossible: matching the empty enum says so, where a
        // `return Err(..)` did not type in a `()` method (E0308).
        _line(
          _failure == null
              ? 'Err(__failed) => match __failed {},'
              : 'Err(__failed) => return Err(__failed),',
        );
        _indent--;
        _line('}');
      case IrTryCatch(
        :final body,
        :final error,
        :final errorType,
        :final handler,
        :final stack,
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
        // A body with nothing that fails -- `listener()` behind a catch-all
        // in `ChangeNotifier.notifyListeners` -- leaves `_` with nothing to
        // infer it from (E0282). A catch-all catches an `Object`.
        final failure =
            errorType ??
            _errorIn(body) ??
            _failure ??
            'std::rc::Rc<dyn Object>';
        // The closure catches `?`, and it would catch a `return` too: written
        // plainly, `return x` in the body returns from the *closure* and the
        // method carries on, which compiles and is wrong. So when the body
        // returns, the closure carries the control flow out as a value --
        // `Some(x)` for "the body returned x", `None` for "it fell off the
        // end" -- and the match below does the returning for real.
        final flows = _returnsEarly(body);
        final carried = flows ? 'Option<${_rustReturns ?? '()'}>' : '()';
        // The same async-block rule as `try/finally` above: the handler
        // wrapper must not be a closure when the body awaits.
        _line(
          _asyncBody
              ? 'match async { let __r: Result<$carried, $failure> = {'
              : 'match (|| -> Result<$carried, $failure> {',
        );
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
        _line(_asyncBody ? '}; __r }.await {' : '})() {');
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
        // The catch clause's stack trace: the catch site's own, since a
        // `Result` carries none (see the front end's note).
        if (stack != null) {
          _indent++;
          _line('let mut ${snake(stack)} = StackTrace::current();');
          _indent--;
        }
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
        // Each element cloned out, as a field read is: Dart's loop variable
        // is the element, not a reference to it, and `&xs` handed out
        // `&f64` where `f64` was wanted (14 in the colour code). The list
        // itself is only borrowed, as before.
        _line('for ${snake(name)} in ${expr(iterable)}.iter().cloned() {');
        _indent++;
        stmt(body);
        _indent--;
        _line('}');
      case IrIndexSet(:final target, :final index, :final value):
        // A write into one of this class's own lists is a write into the
        // place, not into the clone a field *read* takes out:
        // `self._m4storage.clone()[14] = v` changed nothing, 17 times in
        // vector_math, and left the method `&self`.
        final place =
            target is IrField &&
                (target.target == null || target.target is IrThis) &&
                _sharedField(target.name) == null &&
                !_fieldsAreAccessors &&
                _allFields(cls).any((f) => f.name == target.name)
            ? '${_receiver(target.target)}.${snake(target.name)}'
            : expr(target);
        // The index first: `self.f[self.index(r, c)] = v` borrows `self`
        // twice at once (5 E0502s in vector_math).
        _line(
          '{ let __i = ${expr(index)} as usize; $place[__i] = ${expr(value)}; }',
        );
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
        // Rust's `match` takes *patterns*, and only some Dart case values are
        // one. An enum variant and an integer are; a string is not, and
        // `"x".to_string()` in an arm is "expected a pattern, found an
        // expression" -- 266 of those. Those switches become the if-else chain
        // they always were.
        if (!cases.every((c) => c.values.every(_isPattern))) {
          var first = true;
          for (final one in cases) {
            final test = one.values
                .map((v) => '${expr(value)} == ${expr(v)}')
                .join(' || ');
            _line('${first ? 'if' : '} else if'} $test {');
            first = false;
            _indent++;
            stmt(one.body);
            _indent--;
          }
          if (otherwise != null) {
            _line(first ? '{' : '} else {');
            _indent++;
            stmt(otherwise);
            _indent--;
          }
          _line('}');
          return;
        }
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
        } else {
          // Dart's `switch` with no `default` does nothing for a value no
          // case names; Rust's `match` on an `i64` has to say so (E0004 on
          // `switch (data.getInt32(..)) { case 0: .. case 1: .. }`). On an
          // enum every variant is named and the arm is only unreachable.
          _line('_ => {}');
        }
        _indent--;
        _line('}');
      case IrWhile(:final condition, :final body, :final label):
        final head = label == null ? '' : "'" + label + ': ';
        // `while (true)` is `loop`: its type is `!`, so a method whose body
        // ends in one and returns from inside it type-checks (`bool <= ()`).
        _line(
          condition is IrLiteral && condition.value == 'true'
              ? '${head}loop {'
              : '${head}while ${expr(condition)} {',
        );
        _indent++;
        stmt(body);
        _indent--;
        _line('}');
      case IrLocalDecl(:final name, :final type, :final init, :final cell):
        // A local a closure writes lives in a cell the closure clones a
        // handle to (see `IrLocalDecl.cell`); every read and write below
        // goes through `_cellLocals`.
        if (cell) {
          final rust = type == null ? null : this.type(type);
          final copy = rust != null && _isCopy(rust);
          _cellLocals = {..._cellLocals, name: copy};
          final inner = init == null ? 'Default::default()' : expr(init);
          final held = rust == null
              ? ''
              : ': std::rc::Rc<std::cell::${copy ? 'Cell' : 'RefCell'}<$rust>>';
          _line(
            'let ${snake(name)}$held = std::rc::Rc::new(std::cell::${copy ? 'Cell' : 'RefCell'}::new($inner));',
          );
          return;
        }
        final annotation = type == null ? '' : ': ${this.type(type)}';
        // `mut` when the body writes the local, or calls a method on it
        // (`_assignedIn` counts receivers too, since a method in another
        // module may take `&mut self` -- `brk.next_break()` was 30
        // `E0596`s). A local only read stays immutable: the fixture crate
        // denies `unused_mut` to keep that claim checkable.
        final mutable = _reassigned.contains(name) ? 'mut ' : '';
        // A declared function type is `Box<dyn Fn(..)>`, and a closure's own
        // type is not that. `_returned` boxes for the same reason one line
        // further out; a `let` is the other half of it.
        final boxed = type != null && type.isFunction && init is IrClosure;
        // A local with no initialiser is assigned before it is read -- Dart
        // checks that, and so does Rust for a `let x: T;` -- so it needs no
        // value; `Default::default()` asked `Color` for a default it does
        // not have. A nullable one Dart starts at null.
        if (init == null) {
          final nullable =
              type != null && (type.nullable || type.name == 'Option');
          _line(
            nullable
                ? 'let $mutable${snake(name)}$annotation = None;'
                : 'let $mutable${snake(name)}$annotation;',
          );
          return;
        }
        // The same coercion a `return` takes: `let l: Rc<dyn EngineLayer> =
        // _NativeEngineLayer::new_()` needs the `Rc::new` (9 in dart:ui).
        final outer = _returns;
        _returns = type;
        final value = boxed
            ? 'std::rc::Rc::new(${expr(init)})'
            : _returned(init);
        _returns = outer;
        _line('let $mutable${snake(name)}$annotation = $value;');
      case IrAssign(:final name, :final value):
        final cell = _cellLocals[name];
        _line(
          cell == null
              ? '${snake(name)} = ${expr(value)};'
              : cell
              ? '${snake(name)}.set(${expr(value)});'
              : '*${snake(name)}.borrow_mut() = ${expr(value)};',
        );
      case IrAssignField(
        :final target,
        :final name,
        :final value,
        :final owner,
      ):
        final receiver = target == null ? _selfName : expr(target);
        final shared = target == null || target is IrThis
            ? _sharedField(name)
            : owner == null
            ? null
            : _cellFieldOf(owner, name);
        // Assigning a `late` field is what takes it out of `None`, so the
        // value goes in wrapped. This is the only place that happens.
        final own = target == null || target is IrThis
            ? _lateField(name)
            : null;
        final written = own != null || (shared?.isLate ?? false)
            ? 'Some(${expr(value)})'
            : expr(value);
        // Inside a trait's body there is no field, only the setter it
        // declares (`this_.set__length(v)` in a mixin's super function).
        if (_fieldsAreAccessors && (target == null || target is IrThis)) {
          _line('$receiver.set_${snake(name)}($written);');
        } else if (shared != null) {
          // Through the cell, which is why the field can be written from a
          // closure that does not hold `self` at all.
          _line(
            _isCopy(_heldType(shared))
                ? '$receiver.${snake(name)}.set($written);'
                : '*$receiver.${snake(name)}.borrow_mut() = $written;',
          );
        } else {
          _line('$receiver.${snake(name)} = $written;');
        }
      case IrAssignTopLevel(:final name, :final value):
        // Through the cell: two derefs for the `LazyLock` and the `Isolate`,
        // then `borrow_mut`. The read side does the same with `borrow`.
        _line('*(**${screamingSnake(name)}).borrow_mut() = ${expr(value)};');
      case IrAssignStatic(:final owner, :final name, :final value):
        _line('*(**${_lazyName(owner, name)}).borrow_mut() = ${expr(value)};');
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
        // `onCreate?.call(this)` after TFA proved `onCreate` null is a bare
        // `null` in statement position: nothing to do, and `None;` alone
        // cannot even be typed (E0282).
        if (expr is IrLiteral && expr.value == 'null') break;
        if (expr is IrBlockValue &&
            expr.value is IrLiteral &&
            (expr.value as IrLiteral).value == 'null') {
          for (final s in expr.statements) stmt(s);
          break;
        }
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
    if (library.functions.isNotEmpty) {
      // Free functions, written before the classes so a class body reading one
      // is looking at something already declared -- Rust does not care, and a
      // reader does.
      final holder = RustBackend(IrClass('<library>'), library: library);
      for (final function in library.functions) {
        holder._member('top-level ${function.name}', () {
          holder._emitFreeFunction(function);
        });
      }
      out.write(holder._out.join('\n'));
      out.writeln();
      for (final line in holder._out) {
        if (line.startsWith('// NOT TRANSLATED:')) {
          refused.add(line.substring('// NOT TRANSLATED: '.length));
        }
      }
    }
    if (library.constants.isNotEmpty) {
      // Module constants first: Dart's top-level names become Rust's, needing
      // no owner on either side.
      final holder = RustBackend(IrClass('<library>'), library: library);
      for (final constant in library.constants) {
        holder._member('top-level ${constant.name}', () {
          // A mutable top-level variable is a `static` with a cell in it. Dart
          // gives each isolate its own, which `Isolate` says, and anything in
          // the library may assign it, which the `RefCell` says. A `const`
          // cannot be either, so the two are emitted differently.
          if (constant.isMutable) {
            final held = holder.type(constant.type);
            holder._line(
              '${holder._vis(constant.name)}static '
              '${screamingSnake(constant.name)}: '
              'std::sync::LazyLock<Isolate<std::cell::RefCell<$held>>> = '
              'std::sync::LazyLock::new(|| '
              'Isolate(std::cell::RefCell::new('
              '${holder.expr(constant.value)})));',
            );
            return;
          }
          // A `const` with a destructor -- a `Vec`, a `String`, a `Map` --
          // is not a Rust `const` (E0493): a lazily built `static`, read
          // with `.clone()` (`_isLazyConst`).
          if (holder._isLazyConst(constant.name)) {
            // Behind `Isolate`, as the mutable ones are: a `static` must be
            // `Sync`, and an `Rc<dyn Object>` (`Object()` as a zone key) is
            // not; `Isolate` says "one per isolate" and carries that.
            holder._line(
              'pub static ${screamingSnake(constant.name)}: '
              'std::sync::LazyLock<Isolate<${holder.type(constant.type)}>> = '
              'std::sync::LazyLock::new(|| Isolate(${holder.expr(constant.value)}));',
            );
            return;
          }
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
        // Written into the file, not only counted. A class the backend
        // refused used to vanish from the output with nothing said -- the
        // count went up in a summary nobody reads next to the code, and
        // `CupertinoTheme` was simply absent, which is the one thing this
        // compiler is not allowed to do.
        refused.add('${cls.name}: $error');
        out.writeln('// NOT TRANSLATED: ${cls.name}');
        out.writeln('//   $error');
        out.writeln();
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
      // No values: either the front end refused an enhanced enum's members,
      // or the tree shaker dropped every value because nothing reads one
      // (`KeyboardLockMode`, held in a `Set` nothing fills). The *type* is
      // still named -- 5 fields and signatures wanted it -- so it is emitted
      // uninhabited, which is exact: no value of it is ever made, and any
      // code that tries does not compile. The note keeps the distinction.
      _line('// NOT TRANSLATED: `${cls.name}` has no values here -- either');
      _line(
        '// an enhanced enum this compiler refused, or one the tree shaker',
      );
      _line('// emptied. Uninhabited, so that its name still resolves.');
      _line('#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]');
      _line('${_vis(cls.name)}enum ${cls.name} {}');
      return _out.join('\n') + '\n';
    }
    final variants = variantNames(cls.values);
    _line('#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]');
    if (variants.values.any((v) => v != variantName(v))) {
      // Dart's own spelling, kept because capitalising would have collapsed
      // two variants into one. See `variantNames`.
      _line('#[allow(non_camel_case_types)]');
    }
    _line('${_vis(cls.name)}enum ${cls.name} {');
    _indent++;
    for (final value in cls.values) {
      _line('${variants[value]},');
    }
    _indent--;
    _line('}');
    // `index`: a Dart enum value knows its position, and `x.index` was read
    // as a method nothing declared (6 in `dart:ui`'s `BlendMode`).
    _line('');
    _line('impl ${cls.name} {');
    _indent++;
    _line('pub fn index(&self) -> i64 {');
    _indent++;
    _line('*self as i64');
    _indent--;
    _line('}');
    _indent--;
    _line('}');
    // An enhanced enum: its members go in an impl, where they lose nothing.
    // Refusing the whole enum was right only while the alternative was
    // emitting a plain one and dropping them.
    final members = cls.methods.where((m) => m.operator == null).toList();
    // The fields the Dart variants carried, as getters. `Tristate.value` is 0,
    // 1 or 2 depending on which variant it is -- a `match`, not a payload,
    // because the value is a constant *of* the variant.
    final carried = cls.values.isEmpty
        ? const <String>[]
        : (cls.valueFields[cls.values.first]?.keys.toList() ?? const []);
    if (members.isNotEmpty || carried.isNotEmpty) {
      _line('');
      _line('impl ${cls.name} {');
      _indent++;
      for (final field in carried) {
        final declared = cls.fields.where((f) => f.name == field).firstOrNull;
        final rust = declared != null
            ? type(declared.type)
            : _literalType(cls.valueFields[cls.values.first]![field]!);
        _line('${_vis(field)}fn ${snake(field)}(&self) -> $rust {');
        _indent++;
        _line('match self {');
        _indent++;
        for (final value in cls.values) {
          _line(
            '${cls.name}::${variants[value]} => '
            '${cls.valueFields[value]![field]},',
          );
        }
        _indent--;
        _line('}');
        _indent--;
        _line('}');
        _line('');
      }
      for (final method in members) {
        _member('${cls.name}.${method.name}', () => _emitMethod(method));
      }
      _indent--;
      _line('}');
    }
    return _out.join('\n') + '\n';
  }

  /// The Rust type of a literal, when the enum's field declaration is gone.
  ///
  /// The dill drops an enum's fields along with its elements, so the type has
  /// to come from the value. Only the four literal shapes the recovery admits
  /// can arrive here.
  static String _literalType(String literal) {
    if (literal.endsWith('.to_string()')) return 'String';
    if (literal == 'true' || literal == 'false') return 'bool';
    return literal.contains('.') ? 'f64' : 'i64';
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
    _inTrait = true;
    _line('// Generated by tools/dart2rust from upstream `${cls.name}`');
    _line('// (abstract -> trait).');
    _line('');
    _doc(cls.doc);
    // The free functions go first: which of them failed decides whether the
    // trait's matching default can delegate or has to be a `todo!()`.
    _emitSuperFns();
    _line('');
    // `: DartAny` so a `Box<dyn ..>` of this trait can be asked what it holds,
    // which is what `x is Foo` needs and what a bare trait object cannot do.
    // ..and the traits of its own supertypes: a `SourceSpanMixin on
    // SourceSpan` calls `start` through `this_: &__Self` in its super
    // functions, and `__Self: SourceSpanMixin` had to imply `SourceSpan`
    // for that to resolve (6). Dart's implementers already implement all
    // of them, and the flattening below emits those impls.
    // Not `Object`: a mixin `implements Listenable` lists `Object` among its
    // interfaces, and a trait with `Object` above it makes `dyn Mixin` an
    // `Object` twice over -- once here, once from the impl below (E0371).
    // And each once: `on ListenableMixin implements ListenableMixin` named
    // it twice.
    // ..and `Debug`: every struct is one (derived, or "Instance of"), and
    // a super function's `this_: &__Self` prints itself in messages
    // (`MapBase.mapToString(this)`, `"Trying to read $x from $this"`).
    // As a supertrait, `dyn X` is `Debug` by itself, so no impl for it below.
    final supers = <String>{
      'DartAny',
      'std::fmt::Debug',
      if (cls.superclass != null && library.isAbstract(cls.superclass))
        _traitPath(IrType(cls.superclass!, arguments: cls.superclassArguments)),
      for (final i in cls.interfaces)
        if (library.isAbstract(i.name) && i.name != 'Object') _traitPath(i),
    }.toList();
    _line(
      '${_vis(cls.name)}trait ${cls.name}${_generics(cls, static: true)}: ${supers.join(' + ')} {',
    );
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
      // ..and writes, when the mixin's own methods write it: `_length =
      // newLength` inside `_TypedDataBuffer._grow` goes through this. The
      // receiver is `&self`: an implementer of a mixin that writes is
      // counted, and its field is a cell.
      if (!field.isFinal) {
        _line(
          'fn set_${snake(field.name)}(&self, value: ${type(field.type)});',
        );
      }
      _line('');
    }
    for (final method in cls.abstractMethods) {
      _member('${cls.name}.${method.name} (required)', () {
        _refuseShadowedGeneric(method);
        _doc(method.doc);
        _line(
          'fn ${_methodName(method)}${_generics(method)}(${_params(method)})'
          ' -> ${_spelledReturn(type(method.returnType))}${_sizedBound(method)};',
        );
        _line('');
      });
    }
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      _member('${cls.name}.${method.name} (default)', () {
        _refuseShadowedGeneric(method);
        _doc(method.doc);
        _line(
          'fn ${_methodName(method)}${_generics(method)}(${_params(method)})'
          ' -> ${_spelledReturn(type(method.returnType))}${_sizedBound(method)} {',
        );
        _indent++;
        // The default delegates to the free function rather than holding the
        // body, so that an override can still reach it. See `superFn`.
        if (_superFailed.contains(method.name)) {
          _line('todo!("${cls.name}.${method.name} did not translate")');
        } else {
          // An `async` super function is an `async fn`: its future is
          // boxed here, borrowing `self` for the `'_` the signature allows.
          final call =
              '${superFn(cls.name, method.name, isSetter: method.isSetter)}('
              '${['self', ...method.params.map((p) => snake(p.name))].join(', ')})';
          _line(method.isAsync ? 'std::boxed::Box::pin($call)' : call);
        }
        _indent--;
        _line('}');
        _line('');
      });
    }
    _indent--;
    _line('}');
    // A trait holds no storage, but the *class* still had its statics, and in
    // Rust those are module-level items rather than trait items. The struct
    // path has emitted them all along and this one had not, so an abstract
    // class's `static` simply vanished: `NavigatorObserver._navigators` was
    // read from three places and declared nowhere.
    // The trait object is an `Object` too, through the `DartAny` every
    // translated trait requires. Without this an `Rc<dyn Widget>` could not
    // stand where an `Rc<dyn Object>` is wanted, now that `Object` carries
    // `as_any` and is implemented only for sized types.
    _line(
      'impl${_generics(cls, static: true)} Object for dyn ${cls.name}${_generics(cls)} {',
    );
    _indent++;
    _line('fn as_any(&self) -> &dyn std::any::Any {');
    _indent++;
    _line('DartAny::as_any(self)');
    _indent--;
    _line('}');
    _line('fn runtime_type(&self) -> Type {');
    _indent++;
    // The struct's own name, not the trait's: `runtimeType` of an `Rc<dyn
    // Widget>` is the widget's class.
    _line('DartAny::dart_runtime_type(self)');
    _indent--;
    _line('}');
    _indent--;
    _line('}');
    // A trait object compares and hashes by identity, which is what Dart's
    // `Object.==` and `hashCode` do for anything not overriding them, and
    // prints as its class. Without these a struct holding an `Rc<dyn
    // EngineLayer>` could not derive `PartialEq` or `Debug` (9 + 9 in
    // dart:ui), and `Map<Rc<dyn DynamicScheme>, _>` could not be looked up.
    final generics = _generics(cls, static: true);
    final object = 'dyn ${cls.name}${_generics(cls)}';
    _line('impl$generics PartialEq for $object {');
    _indent++;
    _line(
      'fn eq(&self, other: &Self) -> bool { std::ptr::addr_eq(self, other) }',
    );
    _indent--;
    _line('}');
    _line('impl$generics Eq for $object {}');
    _line('impl$generics std::hash::Hash for $object {');
    _indent++;
    _line('fn hash<H: std::hash::Hasher>(&self, state: &mut H) {');
    _indent++;
    _line('(self as *const Self as *const u8 as usize).hash(state)');
    _indent--;
    _line('}');
    _indent--;
    _line('}');

    _line('');
    // Module-level, so they carry the class's name: `Contrast.ratio` is
    // `contrast_ratio(..)` and `Platform.numberOfProcessors` is
    // `PLATFORM_NUMBER_OF_PROCESSORS`. A bare `pub const` here was read as
    // `Platform::..` by every caller (E0782, 86 of them), and a static
    // *method* of an abstract class was not emitted at all. `_staticCall`
    // and `_staticRead` spell the same names for an abstract owner.
    for (final method in cls.methods) {
      if (!method.isStatic || method.operator != null) continue;
      _member(
        '${cls.name}.${method.name} (static)',
        () => _emitMethod(
          method,
          as: _abstractStaticName(
            cls.name,
            method.name.isEmpty ? 'new' : method.name,
          ),
        ),
      );
    }
    _emitConstants(prefix: cls.name);
    _emitLazyStatics();
    return _out.join('\n') + '\n';
  }

  static String _abstractStaticName(String owner, String name) =>
      _rustIdentifier('${snakeRaw(owner)}_${snakeRaw(name)}');

  /// `<T>` for a class or method that has parameters, and nothing otherwise.
  String _generics(Object owner, {bool static = false}) {
    final params = switch (owner) {
      IrClass(:final typeParameters) => typeParameters,
      IrMethod(:final typeParameters) => typeParameters,
      _ => const <String>[],
    };
    if (params.isEmpty) return '';
    // `&dyn Any` is `&dyn Any + 'static`, so a generic struct can only hand
    // one out when its parameters outlive the borrow. Nothing this compiler
    // emits holds a borrow, so the bound costs nothing and is not written
    // anywhere else.
    // A method's own parameters carry what a body needs of them, as an
    // impl's do (`_implGenerics`): `listEquals<T>` clones its `Option<Vec<T>>`
    // (9 "trait bounds were not satisfied" in dart:ui).
    // One bound for every declaration -- struct, trait, impl, method, super
    // function: a trait's default method calls the super function with the
    // trait's own `E`, so the trait has to promise what the function asks
    // (147 E0277s from asking it of the function alone).
    final bound = owner is IrMethod || static
        ? params.map((p) => "$p: Clone + PartialEq + std::fmt::Debug + 'static")
        : params;
    return '<${bound.join(', ')}>';
  }

  /// A top-level constant whose Rust type has a destructor, kept as a
  /// lazily built `static` rather than a `const`.
  bool _isLazyConst(String name) {
    for (final c in library.constants) {
      if (c.name == name) return _lazy(c);
    }
    final other = library.constantsElsewhere[name];
    if (other != null) return _lazy(other);
    return false;
  }

  /// A `const` only when Rust can evaluate the initialiser at compile
  /// time: a `Copy` value built from literals. `"0".codeUnitAt(0)` is an
  /// `i64` and still a call (E0015).
  bool _lazy(IrConstDecl c) =>
      !c.isMutable && (!_isCopy(type(c.type)) || !_constEvaluable(c.value));

  static bool _constEvaluable(IrExpr e) => switch (e) {
    IrLiteral() => true,
    IrStatic() => true,
    IrTopLevel() => true,
    IrBinary(:final left, :final right) =>
      _constEvaluable(left) && _constEvaluable(right),
    IrUnary(:final operand) => _constEvaluable(operand),
    IrCast(:final value) => _constEvaluable(value),
    IrConstInstance(:final fields) => fields.values.every(_constEvaluable),
    IrNew(:final args) => args.every(_constEvaluable),
    _ => false,
  };

  /// Whether every translated class named in a type text can be compared.
  bool _comparableType(String rust, Set<String> seen) {
    if (rust.contains('dyn Fn')) return false;
    for (final name in _namesIn(rust)) {
      final other = library[name];
      if (other == null || !seen.add(name)) continue;
      if (other.isAbstract) continue;
      for (final f in _allFields(other)) {
        // A closure field compares by address in the manual `PartialEq`
        // the struct gets (see `byIdentity`), so it does not make the
        // class incomparable: `Vec<PointerData>` in `PointerDataPacket`.
        if (f.type.isFunction) continue;
        if (!_comparableType(_fieldType(f), seen)) return false;
      }
    }
    return true;
  }

  /// A trait named as a bound: `Foo<T>`, not the `Rc<dyn Foo<T>>` a value
  /// of it is.
  String _traitPath(IrType t) => t.arguments.isEmpty
      ? t.name
      : '${t.name}<${t.arguments.map((a) => type(a)).join(', ')}>';

  /// The generics of an `impl` block: every parameter `Clone + 'static`.
  ///
  /// A method body clones what it reads (`self._map.clone()`), and a
  /// `Map<K, V>` is `Clone` only when `K` and `V` are; an `Rc<dyn ..>` held
  /// in a `T` slot wants `'static`. 30 "trait bounds were not satisfied"
  /// and 12 E0310s in `collection`. The struct and trait declarations stay
  /// unbounded, so a type argument that is neither is still a type -- only
  /// its methods are missing, which is loud where it matters.
  String _implGenerics(IrClass cls) {
    if (cls.typeParameters.isEmpty) return '';
    // A parameter that keys a `Map` or fills a `Set` in one of the fields
    // needs what the prelude's `Map` asks of a key: `keys()` and `get()`
    // "exist but their trait bounds were not satisfied", 9 in `collection`.
    final fields = [
      ..._allFields(cls).map(_fieldType),
      for (final m in cls.methods) ...[
        type(m.returnType),
        for (final p in m.params) type(p.type),
      ],
    ].join(' ');
    String bound(String p) {
      final keyed = RegExp('(Map|Set)<$p[,>]').hasMatch(fields);
      // `PartialEq`: `self._value == new_value` on a `T` (`ValueNotifier`).
      return "$p: Clone + PartialEq + std::fmt::Debug + 'static${keyed ? ' + std::hash::Hash + Eq' : ''}";
    }

    return '<${cls.typeParameters.map(bound).join(', ')}>';
  }

  /// Type parameters no field mentions.
  ///
  /// Rust refuses an unused parameter; Dart does not care. Anything the fields
  /// do not name gets a `PhantomData` so the declaration stays legal without
  /// changing what the class holds.
  List<String> _unusedParameters(IrClass of) {
    if (of.typeParameters.isEmpty) return const [];
    final used = <String>{};
    void mark(IrType t) {
      used.add(t.name);
      t.arguments.forEach(mark);
      t.parameters?.forEach(mark);
      final returns = t.returns;
      if (returns != null) mark(returns);
    }

    for (final field in _allFields(of)) {
      mark(field.type);
    }
    return [
      for (final p in of.typeParameters)
        if (!used.contains(p)) p,
    ];
  }

  /// Whether an expression reads `this`.
  ///
  /// Used where `this` does not exist yet -- inside the struct literal a
  /// constructor builds.
  static bool _mentionsThis(IrExpr e) {
    var found = false;
    final walk = _WalkSelf();
    walk.expression(e);
    found = walk.readsThis;
    return found;
  }

  /// Whether a Rust type is `Copy`.
  ///
  /// Asked of the rendered text rather than the IR, because that is what the
  /// derive has to be true of. Owning types are the ones that are not.
  /// A `const` needs a value Rust can build at compile time, and neither
  /// `vec![]` nor `HashMap::from([..])` is one. Said here rather than left to
  /// rustc, because one broken constant takes the whole file with it.
  static bool _constable(String rust) =>
      !rust.contains('Vec<') && !rust.contains('Map<');

  /// Whether an emitted Rust type derives `Copy`.
  ///
  /// The containers are decided by the text. A **class name** is not, and was
  /// assumed `Copy` -- right for `Offset`, wrong for anything holding a
  /// `String`, and the ruler said "is this Copy" while measuring "does the
  /// text mention a container that is not". So a named class is asked the same
  /// question its own derive is asked, which is what makes the two agree.
  bool _isCopy(String rust) => _isCopyIn(rust, {});

  bool _isCopyIn(String rust, Set<String> seen) {
    if (!_copyText(rust)) return false;
    for (final name in _namesIn(rust)) {
      // A type parameter is not known to be `Copy`, and a read of a `T`
      // field behind `&self` has to clone it: `ValueNotifier.value`.
      if (cls.typeParameters.contains(name)) return false;
      final prelude = _preludeCopy[name];
      if (prelude == false) return false;
      if (prelude != null) continue;
      final other = library[name];
      if (other != null && !_classIsCopy(other, seen)) return false;
    }
    return true;
  }

  /// Which of the prelude's own types are `Copy`, read out of the prelude.
  ///
  /// `WriteBuffer` holds a `Uint8List`, whose Rust name says nothing about
  /// what it is -- `_copyText` saw an identifier and passed it, and the struct
  /// derived `Copy` around a `Vec`. Listing the names here would be a second
  /// source of truth for something the prelude already states in its own
  /// derives, which is the thing `regen.py` exists to avoid.
  static final Map<String, bool> _preludeCopy = _readPrelude();

  static Map<String, bool> _readPrelude() {
    final answers = <String, bool>{};
    final aliases = <String, String>{};
    final lines = const LineSplitter().convert(rustPrelude);
    for (var i = 0; i < lines.length; i++) {
      final line = lines[i];
      final alias = RegExp(r'^pub type (\w+)[^=]*= *(.*);').firstMatch(line);
      if (alias != null) {
        aliases[alias[1]!] = alias[2]!;
        continue;
      }
      final decl = RegExp(r'^pub (?:struct|enum) (\w+)').firstMatch(line);
      if (decl == null) continue;
      // The derive sits on the line above, under any doc comment.
      final above = i > 0 ? lines[i - 1] : '';
      answers[decl[1]!] =
          above.startsWith('#[derive(') && above.contains('Copy');
    }
    // An alias is as `Copy` as what it stands for, which may be another alias.
    String resolve(String text, int depth) {
      if (depth > 4) return text;
      for (final name in _namesIn(text)) {
        final next = aliases[name];
        if (next != null)
          return resolve(text.replaceAll(name, next), depth + 1);
      }
      return text;
    }

    for (final entry in aliases.entries) {
      final text = resolve(entry.value, 0);
      answers[entry.key] =
          _copyText(text) && _namesIn(text).every((n) => answers[n] ?? true);
    }
    return answers;
  }

  static bool _copyText(String rust) =>
      !rust.contains('String') &&
      !rust.contains('std::boxed::Box<') &&
      !rust.contains('Vec<') &&
      !rust.contains('Map<') &&
      // A shared field's `Rc` is not `Copy` however copyable its contents,
      // and a `RefCell` is not either. Without these a struct holding one
      // derived `Copy` and did not compile.
      !rust.contains('Rc<') &&
      !rust.contains('RefCell<') &&
      !rust.contains('Cell<') &&
      !rust.contains('VecDeque') &&
      !rust.contains('dyn ');

  static final _typeName = RegExp(r'[A-Za-z_][A-Za-z_0-9]*');

  static Iterable<String> _namesIn(String rust) =>
      _typeName.allMatches(rust).map((m) => m[0]!);

  /// Answers by class name, once per *library* rather than once per class:
  /// there is one backend per class, and 4123 of them each walking the whole
  /// hierarchy is the shape of a compiler that got slower for no reason.
  static final _copyableIn = Expando<Map<String, bool>>('copyable');

  Map<String, bool> get _copyable => _copyableIn[library] ??= <String, bool>{};

  bool _classIsCopy(IrClass other, Set<String> seen) {
    final known = _copyable[other.name];
    if (known != null) return known;
    // Reached from itself. A value type cannot really contain itself -- the
    // struct would have no size -- so this is a hierarchy that says something
    // impossible, and `Clone` is the half that costs nothing but a clone.
    if (!seen.add(other.name)) return false;
    // A class emitted as a trait has no fields of its own here; its uses are
    // `Box<dyn ..>`, which `_copyText` has already turned down.
    final answer = _allFields(other).every((f) {
      if (f.shared || (other.counted && _mutableOnCounted(f))) return false;
      final held = f.isLate ? 'Option<${type(f.type)}>' : type(f.type);
      return _isCopyIn(held, seen);
    });
    _copyable[other.name] = answer;
    seen.remove(other.name);
    return answer;
  }

  /// A top-level function.
  ///
  /// The same body machinery a method uses, with no `self` -- `_selfName` is
  /// the lever for that, as it is for a constructor body and for the free
  /// functions an abstract class's methods become.
  void _emitFreeFunction(IrMethod method) {
    _doc(method.doc);
    // Before the parameters are spelled: `_param` asks `_reassigned`
    // whether each is written, and it held the previous method's answer.
    _reassigned = _assignedIn(method.body);
    _cellLocals = {};
    final params = method.params.map((p) => _param(p, owned: false)).join(', ');
    _line(
      '${_vis(method.name)}${method.isAsync ? 'async ' : ''}fn '
      '${snake(method.name)}${_generics(method)}'
      '($params) -> ${_returnType(method)} {',
    );
    _indent++;
    final saved = _selfName;
    // There is no receiver. Anything in the body that wanted one is a bug in
    // the front end, not something to paper over here.
    _selfName = '<no self>';
    _returns = method.returnType;
    // The Rust return type too: a `try` body that returns carries
    // `Option<..>` of it out of its closure, and without it `_isLoopback`'s
    // `return address.isLoopback` came out as an `Option<()>`.
    _rustReturns = _returnType(method);
    _asyncBody = method.isAsync;
    stmt(method.body, tail: true);
    _closeOpenIf(method.body);
    _returns = null;
    _rustReturns = null;
    _selfName = saved;
    _indent--;
    _line('}');
    _line('');
  }

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
        superFn(cls.name, method.name, isSetter: method.isSetter),
        () => _emitSuperFn(method),
      )) {
        _superFailed.add(method.name);
      }
    }
  }

  /// `where Self: Sized` for a generic method on a trait, or nothing.
  ///
  /// `RenderObject.invokeLayoutCallback<T extends Constraints>` is generic,
  /// and a generic method makes a trait dyn-incompatible -- so it used to be
  /// refused, on the reading that emitting it "would take `dyn RenderObject`
  /// away from the whole layer". That reading had a hole in it: Rust leaves a
  /// `where Self: Sized` method **out of the vtable**, so the trait stays
  /// dyn-compatible and every concrete implementor still has the method. It
  /// is the bound the standard library puts on `Iterator::by_ref` and friends
  /// for exactly this reason.
  ///
  /// What is given up is calling it *through* a trait object, which Dart does
  /// allow. That call is a refusal of its own where it happens, rather than
  /// 302 members deleted where they are declared.
  // A type parameter, or an `impl Future` parameter -- which is a type
  // parameter in a coat -- keeps a method out of the vtable, and a trait
  // used as `dyn` needs it kept out: `TransitionRoute` was "not dyn
  // compatible" for `_setSecondaryAnimation(.., Future<void>? disposed)`.
  static String _sizedBound(IrMethod method) =>
      method.typeParameters.isEmpty &&
          !method.params.any((p) => p.type.name == 'Future')
      ? ''
      : ' where Self: Sized';

  /// A method whose type parameter has the same name as one of the class's.
  ///
  /// Dart allows the shadowing -- `Element.findAncestorStateOfType<T>` inside
  /// a `State<T>` -- and Rust does not: 44 `E0403`, all of them `T` inside a
  /// `T`. Renaming it would mean renaming it in the body too, which is a
  /// substitution this backend does not do, so the member is refused and says
  /// which name collided.
  void _refuseShadowedGeneric(IrMethod method) {
    for (final p in method.typeParameters) {
      if (cls.typeParameters.contains(p)) {
        throw Unsupported(
          "a method whose type parameter shadows the class's",
          '${cls.name}<$p>.${method.name}<$p>',
        );
      }
    }
  }

  void _emitSuperFn(IrMethod method) {
    {
      _line('');
      _line('/// The body of `${cls.name}.${method.name}`, reachable from an');
      _line('/// override the way Dart\'s `super.${method.name}` is.');
      final params = [
        // The body writes fields through `this_` when the method is one of
        // this class's mutating ones (or the trait's, for every class).
        // `&__Self` always: a write to a field in here goes through the
        // setter the trait declares, on `&self` (typed_data, 7 mismatches
        // once the trait's defaults went back to `&self`).
        'this_: &__Self',
        ...method.params.map(
          // `mut` when the body assigns it (`start = index + 1` in a loop).
          (p) =>
              '${_assignedIn(method.body).contains(p.name) ? 'mut ' : ''}'
              '${snake(p.name)}: ${type(p.type, owned: false)}',
        ),
      ].join(', ');
      _line(
        '${_vis(cls.name)}${method.isAsync ? 'async ' : ''}fn '
        '${superFn(cls.name, method.name, isSetter: method.isSetter)}'
        // The class's parameters come too: a body of `ParametricCurve<T>`
        // returns a `T`, and the free function holding it has to say where
        // that `T` comes from.
        // `__Self`, not `S`: a Dart method's own type parameter is often
        // named `S`, and round 78 started carrying those onto this function --
        // where it collided with the receiver's. A generated name cannot.
        // `Debug` too: a mixin's `toString` hands `this` to `MapBase.
        // mapToString`, which prints it, and every implementer prints.
        '<__Self: ${cls.name}${_generics(cls)} + ?Sized + \'static'
        '${cls.typeParameters.isEmpty ? '' : ', ${cls.typeParameters.map((p) => "$p: Clone + PartialEq + std::fmt::Debug + 'static").join(', ')}'}'
        // And the *method's* own, for a generic method like
        // `invokeLayoutCallback<T extends Constraints>`. A free function can
        // carry them; the trait method it belongs to cannot, and says so.
        '${method.typeParameters.isEmpty ? '' : ', ${method.typeParameters.join(', ')}'}'
        '>($params) -> '
        // An `async fn` returns the awaited type: `Future<Response>` on an
        // `async` super function was a future of a boxed future (E0308).
        // A boxed future returned by a non-async one borrows `this_`
        // (`get(url) => _sendUnstreamed(..)` in `BaseClient`): `+ '_`.
        '${_lifetimed(_returnType(method))} {',
      );
      _indent++;
      _selfName = 'this_';
      _returns = method.returnType;
      _asyncBody = method.isAsync;
      _reassigned = _assignedIn(method.body);
      _cellLocals = {};
      // `this_` is a `&__Self: Trait`, and a trait has no fields: the base's
      // fields are its accessor methods here, as they are inside the trait
      // itself. `this_.start` was read as a field 6 times in `source_span`.
      final accessors = _fieldsAreAccessors;
      _fieldsAreAccessors = true;
      stmt(method.body, tail: true);
      _closeOpenIf(method.body);
      _fieldsAreAccessors = accessors;
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
    // A getter and a setter of the same Dart name are two members there and
    // one name here. The inherent path has always prefixed the setter; the
    // trait impls had not, so a mixin carrying `Ticker? get _ticker` beside
    // `set _ticker(v)` put two `fn _ticker` in one impl -- 839 `E0201`s.
    if (method.isSetter) return 'set_${snake(method.name)}';
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
        if (entry.value.any((c) => writes.contains(snake(c)))) {
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
    // A method that hands out a closure holding `this` takes the handle, not
    // a borrow: the closure keeps a clone of it, and only an `Rc` clones into
    // something that outlives the call.
    if (cls.counted && _handles.contains(_rustName(method))) {
      _selfIsHandle = true;
      return 'self: &std::rc::Rc<Self>';
    }
    _selfIsHandle = false;
    // A counted class never takes `&mut self`: an `Rc` hands out shared
    // access, and every mutable field of one is in a cell for that reason.
    if (cls.counted) return '&self';
    // A trait's method has one signature for every class implementing it,
    // so it is `&mut self` when *any* of them writes a field in it -- and
    // so is every implementation and forwarder, whether or not that one
    // writes. Decided per class, `ChangeNotifier::add_listener(self, ..)`
    // under a `&self` forwarder was 10 "types differ in mutability" and 17
    // E0596s.
    if (_sharedMutation(method)) return '&mut self';
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

  /// `_mutating` of every class, by class name, computed once per library.
  static final Expando<Map<String, Set<String>>> _mutatingCache = Expando();

  /// The concrete, uncounted classes under each trait, once per library.
  static final Expando<Map<String, List<IrClass>>> _implementersCache =
      Expando();

  Set<String> _mutatingOf(IrClass other) {
    if (identical(other, cls)) return _mutating;
    final cache = _mutatingCache[library] ??= {};
    return cache[other.name] ??= RustBackend(other, library: library)._mutating;
  }

  Iterable<String?> _supertypeNames(IrClass c) => [
    c.superclass,
    for (final m in c.mixins) m.name,
    for (final i in c.interfaces) i.name,
  ];

  bool _isSubtypeOf(IrClass c, String trait, Set<String> seen) {
    if (c.name == trait) return true;
    if (!seen.add(c.name)) return false;
    for (final name in _supertypeNames(c)) {
      if (name == null) continue;
      if (name == trait) return true;
      final s = library[name];
      if (s != null && _isSubtypeOf(s, trait, seen)) return true;
    }
    return false;
  }

  List<IrClass> _implementersOf(String trait) {
    final cache = _implementersCache[library] ??= {};
    return cache[trait] ??= [
      for (final c in library.classes)
        if (!c.isAbstract &&
            !c.counted &&
            c.name != trait &&
            _isSubtypeOf(c, trait, {}))
          c,
    ];
  }

  /// Whether a class implementing a trait that declares this method writes
  /// a field in it. See `_receiverOf`.
  bool _sharedMutation(IrMethod method) {
    if (method.isStatic) return false;
    final name = _rustName(method);
    final traits = <String>{};
    void collect(IrClass c, Set<String> seen) {
      if (!seen.add(c.name)) return;
      if (c.isAbstract &&
          (c.methods.any((m) => m.name == method.name) ||
              c.abstractMethods.any((m) => m.name == method.name))) {
        traits.add(c.name);
      }
      for (final n in _supertypeNames(c)) {
        final s = library[n];
        if (s != null) collect(s, seen);
      }
    }

    collect(cls, {});
    return traits.any(
      (t) => _implementersOf(t).any((c) => _mutatingOf(c).contains(name)),
    );
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
  List<IrFieldDecl> _allFields(
    IrClass of, [
    Map<String, IrType> bound = const {},
  ]) {
    final own = [
      for (final f in of.fields)
        IrFieldDecl.substituted(f, (t) => _substituteType(t, bound)),
      // A mixin's fields are the class's too: `Value<T> extends ListNotifier
      // with StateMixin<T>` has `StateMixin`'s `late T _value`, and came out
      // as a struct of nothing but a `PhantomData<T>`.
      for (final mixin in of.mixins)
        if (library[mixin.name] case final m?)
          for (final f in _allFields(m, {
            if (mixin.arguments.length == m.typeParameters.length)
              for (var i = 0; i < mixin.arguments.length; i++)
                m.typeParameters[i]: _substituteType(mixin.arguments[i], bound)
            else if (mixin.arguments.isEmpty)
              for (final p in m.typeParameters) p: const IrType('dynamic'),
          }))
            if (!of.fields.any((o) => o.name == f.name)) f,
    ];
    final base = library[of.superclass];
    if (base == null) return own;
    // The base's type parameters, bound to what this class passed it.
    // `ErrorDescription extends DiagnosticsProperty<String>` inherits a
    // `T? _value`, and copying it in unsubstituted left a field of type `T` in
    // a struct with no `T` -- 32 `cannot find type T`, and every one of them a
    // field this compiler had claimed to translate.
    final passed = of.superclassArguments;
    // `class _DialogRoute extends PopupRoute` -- no arguments written -- is
    // `PopupRoute<dynamic>` in Dart. Leaving the base's `T` unbound copied
    // `Option<T>` fields into a struct with no `T`: 25 `cannot find type T`.
    final next = <String, IrType>{
      if (passed.length == base.typeParameters.length)
        for (var i = 0; i < passed.length; i++)
          base.typeParameters[i]: _substituteType(passed[i], bound)
      else if (passed.isEmpty)
        for (final p in base.typeParameters) p: const IrType('dynamic'),
    };
    // A subclass may redeclare a field the base already has -- Dart lets it
    // shadow -- and one struct cannot hold two `_color`s (13 `E0124`s). The
    // nearer declaration is the one the class's own code names, so it wins.
    final inherited = _allFields(base, next);
    final ownNames = {for (final f in own) f.name};
    return [
      for (final f in inherited)
        if (!ownNames.contains(f.name)) f,
      ...own,
    ];
  }

  /// `T` -> whatever `T` was bound to, inside a type and its arguments.
  static IrType _substituteType(IrType t, Map<String, IrType> bound) {
    // A function type keeps its parameters and result beside `arguments`,
    // not in it: `FormFieldBuilder<T>` copied into `TextFormField` kept its
    // `T` -- 25 `cannot find type T`, every one inside a `dyn Fn(..)`.
    final params = t.parameters;
    if (params != null) {
      return IrType.function(
        [for (final p in params) _substituteType(p, bound)],
        _substituteType(t.returns!, bound),
        nullable: t.nullable,
      );
    }
    if (t.arguments.isEmpty) {
      final to = bound[t.name];
      if (to == null) return t;
      // `T?` with `T` bound to `dynamic` is `dynamic`: Dart's `dynamic`
      // already admits null, and spelling the `?` again made the impl say
      // `Option<Rc<dyn Object>>` where the trait, bound the same way, said
      // `Rc<dyn Object>` -- `decodeMessage` on every codec, 16 `E0053`s.
      if (to.name == 'dynamic') return to;
      // The `?` belongs to the *use*, not to what is put in its place:
      // `ChildType? _child` with `ChildType` bound to `RenderBox` is a
      // `RenderBox?`, and dropping the question mark made the accessor return
      // a `Box<dyn RenderBox>` where the trait it implements wants an
      // `Option<Box<dyn RenderBox>>` -- 575 `E0053`s.
      // `T?` where `T` is *already* nullable collapses, as it does in Dart:
      // `bool??` is `bool?`. Rust does not collapse -- with
      // `RestorableValue<bool?>` the trait's `Option<T>` is
      // `Option<Option<bool>>`, and its two `None`s are distinguishable in a
      // way Dart cannot express -- so 14 members come out with a type the
      // trait will not accept.
      //
      // Refusing them instead was tried and measured *worse*: an accessor's
      // refusal takes the whole `impl` with it, and the slice went 273 errors
      // to 278. Saying it properly means the IR carrying nullability as
      // something richer than a flag, which it does not. Until then these 14
      // stay visible and explained rather than turned into a cascade.
      if (!t.nullable) return to;
      // Dead code in the first cut: the old collapse (`|| to.nullable`)
      // returned one line earlier, and r139 still read `Option<T>` where
      // rustc wanted `Option<Option<..>>`.
      if (to.nullable) return IrType('Option', arguments: [to]);
      return IrType(to.name, nullable: true, arguments: to.arguments);
    }
    return IrType(
      t.name,
      nullable: t.nullable,
      arguments: [for (final a in t.arguments) _substituteType(a, bound)],
    );
  }

  /// The field initialisers a `super(...)` stands for.
  ///
  /// The base's own constructor is *inlined*: its parameters are replaced by
  /// the arguments the super call passed, and its field initialisers become
  /// this constructor's. That is what flattening means once it reaches storage,
  /// and it recurses, since a base may call `super` in turn -- the chains go six
  /// deep in places.
  /// A thrown value, boxed into the function's error type when that is
  /// `Object` and the value is a string -- the tree shaker's "code removed
  /// by TFA" throws, in statement and in expression position alike.
  /// A throw, spelled for where it is. Inside a failing method it is the
  /// `Err` the signature promised. Anywhere else -- a method a trait
  /// declares (whose signature is the trait's, one for every class), a
  /// static, a top-level function, a closure -- there is no `Result` to
  /// carry it, and it is a panic: Dart's exception, uncaught on this side.
  /// Not a quiet wrong answer; a loud one, at the site. 50 of ws32's 361
  /// errors were `Result`s meeting signatures that never said so.
  String _thrown(IrExpr value) => _failure == null
      ? 'panic!("uncaught Dart exception: {:?}", ${expr(value)})'
      : 'return Err(${_boxedThrow(value)})';

  /// Whether an abstract class this one is or descends from declares the
  /// method: its signature is then the trait's, and cannot be widened to
  /// a `Result` by this class alone.
  bool _traitDeclares(String dartName) {
    var found = false;
    void collect(IrClass c, Set<String> seen) {
      if (found || !seen.add(c.name)) return;
      if (c.isAbstract &&
          (c.methods.any((m) => m.name == dartName) ||
              c.abstractMethods.any((m) => m.name == dartName))) {
        found = true;
        return;
      }
      for (final n in _supertypeNames(c)) {
        final s = library[n];
        if (s != null) collect(s, seen);
      }
    }

    collect(cls, {});
    return found;
  }

  /// The error type a method's signature carries, if any: see `_thrown`.
  String? _failureOf(IrMethod method) =>
      _traitDeclares(method.name) ? null : _failing[_rustName(method)];

  String _boxedThrow(IrExpr value) {
    final thrown = expr(value);
    // ..and any constructed error: a `FormatException` thrown where the
    // signature says `Rc<dyn Object>` is boxed into it too.
    final boxed =
        (_failure == 'Object' || _failure == 'std::rc::Rc<dyn Object>') &&
        ((value is IrLiteral && value.type.name == 'String') ||
            value is IrNew ||
            value is IrConstInstance);
    return boxed ? 'std::rc::Rc::new($thrown)' : thrown;
  }

  /// Free functions the prelude provides, which the front end calls by name
  /// and no library declares: the crate-wide "was it translated" check has
  /// to know them, or `vec_of_nones(..)` reads as a call to nothing.
  static const _preludeFunctions = {
    'never',
    'new_object',
    'string_from_char_codes',
    'vec_of_nones',
    'dart_iter',
    'post_event',
    '_print',
    '_print_debug',
    '_schedule_microtask',
    'object_hash_all',
    '_invoke1_with_return',
    '_get_callback_handle',
    '_get_callback_from_handle',
    'object_hash',
    'dart_str',
    'log',
    'parse_int',
    'try_parse_int',
    'parse_double',
    'try_parse_double',
    'schedule_microtask',
    'uint8_list_view',
  };

  static const _typedLists = {
    'Float32List',
    'Float64List',
    'Int8List',
    'Int16List',
    'Int32List',
    'Int64List',
    'Uint8List',
    'Uint16List',
    'Uint32List',
    'Uint64List',
    'Uint8ClampedList',
  };

  /// The statements a `super(...)` chain runs before its fields are set --
  /// the temporaries the CFE binds in a base's initialiser list -- with the
  /// base's parameters replaced by what this constructor passed, exactly as
  /// `_inheritedInits` does for the field initialisers. Without them a base
  /// field init named a `__t0` this constructor never bound.
  List<IrStmt> _inheritedPre(IrConstructor ctor) {
    final baseName = ctor.superBase;
    if (baseName == null) return const [];
    final base = library[baseName];
    if (base == null) return const [];
    final baseCtors = base.constructors
        .where((c) => c.name == ctor.superName)
        .toList();
    if (baseCtors.length != 1) return const [];
    final baseCtor = baseCtors.single;
    if (baseCtor.params.length != ctor.superArgs.length) return const [];
    final substitution = <String, IrExpr>{
      for (var i = 0; i < baseCtor.params.length; i++)
        baseCtor.params[i].name: ctor.superArgs[i],
      ..._baseTempRenames(base, baseCtor),
    };
    return [
      ..._inheritedPre(baseCtor),
      for (final s in baseCtor.pre)
        if (s is IrLocalDecl)
          IrLocalDecl(
            _baseTempName(base, s.name),
            s.type,
            s.init == null ? null : _substitute(s.init!, substitution),
          )
        else
          s,
    ];
  }

  /// A base constructor's temporaries, renamed for the constructor they are
  /// inlined into. Each library numbers its own `__tN`, so a base in another
  /// library and the subclass both have a `__t0` -- and the subclass passes
  /// its `__t0` as the super argument the base's `__t0` is computed from:
  /// `let __t0 = __t0.to_int()`, 9 times.
  static String _baseTempName(IrClass base, String name) =>
      '${snakeRaw(base.name)}_$name';

  static Map<String, IrExpr> _baseTempRenames(
    IrClass base,
    IrConstructor ctor,
  ) => {
    for (final s in ctor.pre)
      if (s is IrLocalDecl) s.name: IrLocal(_baseTempName(base, s.name)),
  };

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
    final baseCtors = base.constructors
        .where((c) => c.name == ctor.superName)
        .toList();
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
      ..._baseTempRenames(base, baseCtor),
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
      IrField(:final target, :final name, :final onEnum, :final owner) =>
        IrField(
          target == null ? null : go(target),
          name,
          onEnum: onEnum,
          owner: owner,
        ),
      IrBinary(:final op, :final left, :final right, :final type) => IrBinary(
        op,
        go(left),
        go(right),
        type: type,
      ),
      IrUnary(:final op, :final operand) => IrUnary(op, go(operand)),
      IrNullCheck(:final operand) => IrNullCheck(go(operand)),
      IrDowncast(:final target, :final type) => IrDowncast(go(target), type),
      IrSome(:final value) => IrSome(go(value)),
      IrCast(:final value, :final rust) => IrCast(go(value), rust),
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
      IrNullAware(:final receiver, :final body, :final flatten) => IrNullAware(
        go(receiver),
        go(body),
        flatten: flatten,
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
      IrAwait(:final operand) => IrAwait(go(operand)),
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
        // The bindings too: a base constructor's `errorPalette ??
        // TonalPalette.of(..)` is `let __t = error_palette; ..`, and the
        // parameter it names is the subclass's super argument (9).
        [
          for (final s in statements)
            if (s is IrLocalDecl)
              IrLocalDecl(s.name, s.type, s.init == null ? null : go(s.init!))
            else
              s,
        ],
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
      IrRecord(:final fields) => IrRecord(fields.map(go).toList()),
      IrRecordField(:final record, :final index) => IrRecordField(
        go(record),
        index,
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

  /// Whether a method that throws carries a `Result` in its signature.
  ///
  /// Off since 2026-09-04. The propagation was never modular: a `Result`
  /// on a method is visible to callers on `this` (which add `?`) and to
  /// nobody else -- a getter read through another object, a trait's
  /// declaration, a closure, a static -- and every one of those was a
  /// type error at the caller (ws59: `Rc<Image> <= Result<Rc<Image>,
  /// StateError>` on `_image`, 15 such in dart:ui alone). Without it a
  /// `throw` is a panic (`_thrown`), except inside a `try` body, where the
  /// flow closure still turns it into the `Err` the handler catches. What
  /// is lost: an exception thrown *by a callee* inside a `try` panics
  /// instead of being caught. That is a loud loss, at the site, and the
  /// runtime will say so; the quiet one was a signature nobody could see.
  static const _resultModel = false;

  Map<String, String> _computeFailing() {
    if (!_resultModel) return const {};
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
          // `_WalkSelf` records the Dart name; the keys are Rust names.
          // Compared raw, `setFromTranslationRotation` never matched
          // `set_from_translation_rotation`, and neither contagion --
          // this one nor `_computeMutating`'s -- ever crossed a camelCase
          // call. The 16 E0596s that survived every receiver rule were this.
          final error = failing[snake(callee)];
          if (error != null) {
            failing[entry.key] = error;
            changed = true;
            break;
          }
        }
      }
      // A method that throws its own type and calls one failing with
      // another cannot carry both in one `Result`: it carries `Object`, the
      // type every Dart throw already has (5 "couldn't convert the error").
      for (final entry in calls.entries) {
        final own = failing[entry.key];
        if (own == null || own == 'Object') continue;
        for (final callee in entry.value) {
          final other = failing[snake(callee)];
          if (other != null && other != own) {
            failing[entry.key] = 'Object';
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
    IrSwitch(:final cases, :final otherwise) =>
      otherwise != null &&
          _alwaysReturns(otherwise) &&
          cases.every((c) => _alwaysReturns(c.body)),
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
      if (_traitDeclares(name)) continue;
      final error = _failing[snake(name)];
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
    final error = _failureOf(method);
    // A Rust `async fn` returning `T` already is a future, so the `Future<T>`
    // Dart declared is the wrapper, not the value: `Future<void> f() async`
    // is `async fn f()`, and writing the wrapper as well would make it a
    // future of a future.
    final declared = method.isAsync
        ? _awaited(method.returnType)
        : method.returnType;
    final value = method.isSetter
        ? '()'
        : type(declared) == 'std::convert::Infallible'
        ? '!'
        : type(declared);
    // The error type goes through `type()` like any other: an abstract one --
    // `Object` is the commonest, since a `throw` with no declared type lands
    // there -- is a trait, and a trait is not a type. `Box<dyn Object>` is.
    return error == null ? value : 'Result<$value, ${type(IrType(error))}>';
  }

  /// A boxed future in return position may borrow the receiver: `+ '_`.
  /// A trait's async method is `fn f(&self) -> Pin<Box<dyn Future<..> +
  /// '_>>`, the manual spelling of `async fn` in a trait, and the default
  /// that boxes `super_fn(self, ..)` needs exactly that lifetime.
  /// A return type as a signature spells it: `!` for `Never` (the general
  /// spelling `Infallible` is for value positions), and a boxed future's
  /// receiver lifetime.
  static String _spelledReturn(String rendered) =>
      rendered == 'std::convert::Infallible' ? '!' : _lifetimed(rendered);

  static String _lifetimed(String rendered) {
    const prefix =
        'std::pin::Pin<std::boxed::Box<dyn std::future::Future<Output = ';
    if (!rendered.startsWith(prefix) || !rendered.endsWith('>>'))
      return rendered;
    return "${rendered.substring(0, rendered.length - 2)} + '_>>";
  }

  /// `Future<T>` -> `T`; anything else unchanged.
  static IrType _awaited(IrType t) =>
      t.name == 'Future' && t.arguments.length == 1 ? t.arguments.single : t;

  String _param(IrParam p, {bool owned = true}) =>
      '${_reassigned.contains(p.name) ? "mut " : ""}'
      // A *function-typed* parameter the callee keeps is owned however it was
      // reached: a list of listeners cannot hold a borrow. Only function
      // types: "keeps it" is measured as "does more than call it", and for an
      // ordinary parameter that includes merely comparing it -- which made
      // `identical(this, other)` take its argument by value and stop being a
      // question about references at all.
      '${snake(p.name)}: '
      '${type(p.type, owned: owned || (p.kept && p.type.isFunction))}';

  String _params(IrMethod method) => [
    // A trait method is `&mut self` when any implementer writes a field in
    // it; see `_receiverOf`. This is the trait's own declaration.
    // Not the trait's own default body writing a field: that write goes
    // through the setter the trait declares (`set_x(&self, ..)`), since the
    // implementers of a writing trait are counted.
    if (!method.isStatic) _sharedMutation(method) ? '&mut self' : '&self',
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
    // Asked of the *emitted* type: a shared field is an `Rc<Cell<..>>`, which
    // is not `Copy` however copyable the value inside it is. Asking the Dart
    // type instead derived `Copy` for a struct that cannot have it.
    final copyable = _allFields(cls).every((f) => _isCopy(_fieldType(f)));
    // `Debug` and `PartialEq` cannot be derived over a function-typed field
    // (a `dyn Fn` is neither), and a struct holding one got 15 `E0369`s and
    // 14 `E0277`s for the derive alone. Left off there: a `==` on such a
    // class is then an error at the use, which says what it is.
    // Nested too: a `Vec<Option<Rc<dyn Fn()>>>` field cannot be printed.
    final printable = _allFields(cls)
        .every((f) => !f.type.isFunction && !_fieldType(f).contains('dyn Fn'));
    // A trait-object field compares by identity, and a derived `PartialEq`
    // cannot say so: `self.f == other.f` on an `Rc<dyn Object>` moved the
    // right-hand side (E0507, rustc 1.98 -- reproduced on four lines). The
    // `impl` is written out below instead, field by field, with the
    // prelude's `dart_eq` on those.
    // ..and a counted class's handle: `Rc<DynamicColor>` compares by
    // identity too, which is what Dart says of two references.
    // The field *is* a handle -- `Rc<..>`, or an `Option`/`Vec` of one --
    // not a struct that merely holds one somewhere in its type arguments
    // (`MapEquality<K, V>` compares by value).
    final handle = RegExp(r'^(Option<|Vec<)*std::rc::Rc<');
    // A closure field too: `PointerData._onRespond` is an `Rc<dyn Fn>`,
    // which `DartEq` compares by address as Dart compares closures. Left
    // out, the struct had no `PartialEq` at all and nothing generic over
    // it could be called (`_invoke1<PointerDataPacket>`).
    final byIdentity = _allFields(cls)
        .where((f) => f.type.isFunction || handle.hasMatch(_fieldType(f)))
        .toList();
    // ..and every field's own class comparable, recursively: a
    // `VecDeque<_StoredMessage>` of a struct holding a closure derives
    // nothing (`==` cannot be applied, 3).
    final comparable =
        printable &&
        byIdentity.isEmpty &&
        _allFields(cls)
            .every((f) => _comparableType(_fieldType(f), {cls.name}));
    _line(
      '#[derive(Clone, ${copyable ? 'Copy, ' : ''}'
      '${printable ? 'Debug' : ''}${comparable ? ', PartialEq' : ''})]',
    );
    // `'static` on the struct: an `Rc<dyn Equality<Option<E>>>` field needs
    // its `E` to outlive the trait object (8 E0310s in `collection`).
    _line(
      '${_vis(cls.name)}struct ${cls.name}${_generics(cls, static: true)} {',
    );
    _indent++;
    for (final field in _allFields(cls)) {
      _doc(field.doc);
      _line('${_vis(field.name)}${snake(field.name)}: ${_fieldType(field)},');
    }
    // A Dart class can name a type parameter it never stores -- `Tween<T>`
    // holds `begin` and `end` of type `T?`, but plenty do not. Rust will not
    // have an unused parameter, and `PhantomData` is what it offers instead.
    for (final unused in _unusedParameters(cls)) {
      _line(
        '_phantom_${snake(unused)}: '
        'std::marker::PhantomData<$unused>,',
      );
    }
    _indent--;
    _line('}');
    _line('');
    // A struct holding a closure still has to print -- `Rc<DynamicColor>`
    // in a struct that derives `Debug` -- so it prints as its class.
    if (!printable) {
      _line(
        'impl${_implGenerics(cls)} std::fmt::Debug for ${cls.name}${_generics(cls)} {',
      );
      _indent++;
      _line(
        "fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {",
      );
      _indent++;
      _line('write!(f, "Instance of \'${cls.name}\'")');
      _indent--;
      _line('}');
      _indent--;
      _line('}');
    }
    if (byIdentity.isNotEmpty) {
      final bounds = cls.typeParameters.isEmpty
          ? ''
          : ' where ${cls.typeParameters.map((p) => '$p: PartialEq').join(', ')}';
      _line(
        'impl${_implGenerics(cls)} PartialEq for ${cls.name}${_generics(cls)}$bounds {',
      );
      _indent++;
      // By name: `_allFields` builds its list afresh each call, so the
      // `IrField`s are not the same objects (`source` came out as `==`).
      final identityNames = byIdentity.map((f) => f.name).toSet();
      final terms = [
        for (final f in _allFields(cls))
          identityNames.contains(f.name)
              ? 'self.${snake(f.name)}.dart_eq(&other.${snake(f.name)})'
              : 'self.${snake(f.name)} == other.${snake(f.name)}',
      ];
      _line(
        'fn eq(&self, other: &Self) -> bool { '
        '${terms.isEmpty ? 'true' : terms.join(' && ')} }',
      );
      _indent--;
      _line('}');
      _line('');
    }

    _line('impl${_implGenerics(cls)} ${cls.name}${_generics(cls)} {');
    _indent++;
    _emitConstructors();
    _emitConstants();
    _emitMethods();
    _indent--;
    _line('}');
    // One line per struct rather than one blanket impl over everything: see
    // `DartAny` in the prelude for why the blanket one is quietly wrong.
    _line('');
    _line(
      'impl${_generics(cls, static: true)} DartAny for '
      '${cls.name}${_generics(cls)} {',
    );
    _indent++;
    _line('fn as_any(&self) -> &dyn std::any::Any {');
    _indent++;
    _line('self');
    _indent--;
    _line('}');
    _line('fn dart_runtime_type(&self) -> Type {');
    _indent++;
    _line('Type { name: "${cls.name}" }');
    _indent--;
    _line('}');
    _indent--;
    _line('}');
    _emitOperators();
    _emitBaseImpl();
    _emitLazyStatics();
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
      // Wrapped, like every other member. A `super` call or an `is` inside one
      // delegating method used to travel out of `_emitStruct` and take the
      // class with it -- the same gap round 53 found in the constructors.
      _member(
        'impl ${ancestor.name} for ${cls.name}',
        () => _emitImplFor(ancestor),
      );
    }
  }

  /// The abstract classes above this one, nearest first.
  ///
  /// Mixins count. `class Panel extends Measured with Scaled` has to implement
  /// `Scaled` for `Scaled`'s methods to be reachable through it, exactly as it
  /// implements an abstract superclass -- a mixin is a base that does not sit
  /// on the `extends` chain, and looking only along that chain found none of
  /// them.
  List<IrClass> _abstractAncestors(IrClass of) {
    final found = <String, IrClass>{};
    // Bases are reached by **name**, and `library[...]` resolves a name against
    // this module and then the rest of the crate -- where two libraries are
    // allowed to declare the same one. `NetworkImage` in `image_provider.dart`
    // is abstract and hands its construction to a `NetworkImage` in
    // `_network_image_io.dart`, which implements it; `BitField` does the same.
    // Under a name lookup the second reaches the first, which is the second,
    // and the walk recursed until the stack ended. `seen` is what stops it --
    // and it earns its keep on plain diamonds too, where an ancestor was
    // re-walked once per path that reached it.
    final seen = <IrClass>{};
    void climb(IrClass? from) {
      if (from == null || !seen.add(from)) return;
      for (final name in [
        from.superclass,
        ...from.mixins.map((m) => m.name),
        // `implements` reaches a base too. Dart promises the members without
        // the bodies, which is what a Rust `impl` is.
        ...from.interfaces.map((i) => i.name),
      ]) {
        final above = library[name];
        if (above == null) continue;
        // A class is not its own ancestor. Had the recursion above not ended
        // the run, this is what the same name collision would have emitted:
        // `impl NetworkImage for NetworkImage`.
        if (identical(above, from) || identical(above, of)) continue;
        if (above.isAbstract) found.putIfAbsent(above.name, () => above);
        climb(above);
      }
    }

    climb(of);
    return found.values.toList();
  }

  /// `<f32>` for `impl ParametricCurve<f32> for _Linear`, or nothing.
  ///
  /// Only the *direct* superclass's arguments are known. For a generic
  /// ancestor further up the chain they would have to be composed through each
  /// step, so that impl is refused rather than emitted with the wrong ones.
  String? _baseArguments(IrClass base) {
    if (base.typeParameters.isEmpty) return '';
    final passed = _baseTypeArguments(base);
    if (passed == null) return null;
    return '<${passed.map((a) => type(a)).join(', ')}>';
  }

  /// What this class passed the base's type parameters, or null when it cannot
  /// be worked out from here.
  List<IrType>? _baseTypeArguments(IrClass base) {
    if (base.typeParameters.isEmpty) return const [];
    // Walk up from this class, carrying the arguments through each step.
    // `_Linear extends Curve` and `Curve extends ParametricCurve<double>`, so
    // reaching ParametricCurve means going through Curve -- and a Curve that
    // had parameters of its own would need ours substituted into what it
    // passes on.
    var current = cls;
    var bound = <String, IrType>{};
    while (true) {
      // A mixin is a direct base of whichever class named it, so its arguments
      // are read off the `with` clause rather than composed through a chain --
      // with whatever this step has already bound substituted in.
      for (final mixin in [...current.mixins, ...current.interfaces]) {
        if (mixin.name != base.name) continue;
        // Substituted *inside* the argument, not only when the argument is a
        // bare parameter: `FormFieldState<T> extends State<FormField<T>>`
        // passes `FormField<T>`, and `bound[a.name]` left that `T` standing --
        // 28 `E0747`s reading it as a constant.
        final passed = [
          for (final a in mixin.arguments) _substituteType(a, bound),
        ];
        if (passed.length != base.typeParameters.length) return null;
        return passed;
      }
      final next = library[current.superclass];
      if (next == null) return null;
      final passed = [
        for (final a in current.superclassArguments) _substituteType(a, bound),
      ];
      if (next.name == base.name) {
        if (passed.length != base.typeParameters.length) return null;
        return passed;
      }
      if (passed.length != next.typeParameters.length) return null;
      bound = {
        for (var i = 0; i < passed.length; i++)
          next.typeParameters[i]: passed[i],
      };
      current = next;
    }
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
    // No early return when both are empty. A Dart subclass *is* its base
    // whether or not it changes anything, so the impl has to exist even with
    // nothing in it -- `Panel extends Measured with Scaled` overrides neither
    // and the mixin has no fields, and without `impl Scaled for Panel {}` the
    // free function holding `Scaled`'s body cannot be called on a `Panel`:
    // "the trait bound `Panel: Scaled` is not satisfied". An empty impl block
    // is the whole statement that it is one.

    final arguments = _baseArguments(base);
    if (arguments == null) {
      // A generic ancestor whose arguments cannot be worked out from here.
      // Emitting `impl Base for This` without them does not compile; saying so
      // is better than leaving rustc to.
      _line('');
      _line('// NOT TRANSLATED: impl ${base.name} for ${cls.name}');
      _line('//   the base is generic and its arguments are not known here');
      return;
    }
    // Every signature in the block is the trait's, so it is spelled the
    // trait's way -- a callback parameter is `&dyn Fn`, not `impl Fn`, or the
    // impl declares a type parameter the trait method does not have.
    _inTrait = true;
    // Bound for the whole block: every signature inside is written in the
    // base's terms and has to come out in this class's.
    final passed = _baseTypeArguments(base) ?? const [];
    _implBinding = {
      if (passed.length == base.typeParameters.length)
        for (var i = 0; i < passed.length; i++)
          base.typeParameters[i]: passed[i],
    };
    _line('');
    // The parameters are *declared* on the impl before they are used.
    // `impl Trait<T> for Foo<T>` does not compile -- nothing introduced the
    // first `T` -- and leaving the declaration off was 428 `cannot find type
    // T` in the widget layer alone, one for every generic class's every trait
    // impl. The struct's own inherent impl had it right all along, which is
    // why it took a slice big enough to hold a generic class to show.
    // `'static` on the parameters, because the trait requires `DartAny` and
    // `DartAny` hands out a `&dyn Any`. A generic class implementing a trait
    // is the commonest shape in the widget layer, so leaving the bound off
    // here was 620 `E0310` in one go.
    _line(
      'impl${_implGenerics(cls)} ${base.name}$arguments for '
      '${cls.name}${_generics(cls)} {',
    );
    _indent++;
    // A field and a method of the same name are one item in Rust. A mixin
    // routinely has both -- `Ticker? _ticker;` beside a getter that reads it --
    // and emitting the accessor as well as the method put two `fn _ticker` in
    // one impl: 839 `E0201`s the moment mixins started being implemented. The
    // method wins, because it is the one that may have a body worth keeping.
    final taken = {for (final need in required) _methodName(need)};
    // The base's field is only *this* class's field when this class inherited
    // it. `class X extends A with M implements B` does not: a mixin's `on`
    // clause puts its constraint on the extends chain, so `B` is reached as an
    // ancestor while `X` satisfies it by implementing -- `viewId` there is a
    // getter of X's own, forwarding to something else, and reading
    // `self.view_id` names a field the struct does not have. 345 of those in
    // `PointerEvent` alone.
    final held = {for (final f in _allFields(cls)) f.name};
    for (final field in accessors) {
      if (taken.contains(snake(field.name))) continue;
      // Cloned out: the accessor returns a value and the field is behind
      // `&self` -- `fn _buffer(&self) -> Vec<i64> { self._buffer }` moved it.
      // ..and through the cell when the field is in one (a counted class):
      // `self.parent.clone()` handed out the `Rc<RefCell<..>>` itself.
      final cell = _sharedField(field.name);
      final reads = held.contains(field.name)
          ? (cell != null
                ? (_isCopy(_heldType(cell))
                      ? 'self.${snake(field.name)}.get()'
                      : 'self.${snake(field.name)}.borrow().clone()')
                : _isCopy(type(_substituteType(field.type, _implBinding)))
                ? 'self.${snake(field.name)}'
                : 'self.${snake(field.name)}.clone()')
          : cls.methods.any((m) => m.name == field.name && !m.isStatic)
          ? 'self.${snake(field.name)}()'
          : null;
      // The accessor's type is the *trait's*, so it is written in this
      // class's terms like every other signature in the block. Round 73
      // substituted the methods and left the accessors behind, which put a
      // `T` no impl declares in front of 103 field reads.
      // `todo!()`, not a refusal. A refused accessor leaves the trait
      // unimplemented -- 18 `E0046`s, one of them naming twenty-three at once
      // -- and the method path next door has always written a `todo!()` for
      // exactly this. The two owe the same answer.
      //
      // The case is real: `_TransformedPointerAddedEvent` gets `viewId` from a
      // mixin, and the IR does not copy a mixin's methods into the class, so
      // nothing here can see the getter that does exist. Reaching it means
      // going through the mixin's own trait, which is a round of its own.
      final body =
          reads ?? 'todo!("${cls.name} does not translate ${field.name} yet")';
      final substituted = _substituteType(field.type, _implBinding);
      _line('fn ${snake(field.name)}(&self) -> ${type(substituted)} {');
      _indent++;
      // The field holds one `Option`; a trait asking for the doubled one
      // gets it wrapped.
      _line(
        substituted.name == 'Option' && reads != null ? 'Some($body)' : body,
      );
      _indent--;
      _line('}');
      _line('');
      // The setter the trait asks for on a mutable field (see `_emitTrait`).
      if (!field.isFinal && held.contains(field.name)) {
        final cell = _sharedField(field.name);
        _line(
          'fn set_${snake(field.name)}(&self, value: ${type(substituted)}) {',
        );
        _indent++;
        if (cell != null) {
          _line(
            _isCopy(_heldType(cell))
                ? 'self.${snake(field.name)}.set(value);'
                : '*self.${snake(field.name)}.borrow_mut() = value;',
          );
        } else {
          _line(
            'todo!("${cls.name}.${field.name} is written through a trait but is not a cell")',
          );
        }
        _indent--;
        _line('}');
        _line('');
      }
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

  /// The base's type parameters, bound to what this class passed them.
  ///
  /// A trait method is declared in the base's terms -- `_RRectLike<T>` has
  /// `fn _create(..) -> T` -- and `impl _RRectLike<RRect> for RRect` has to
  /// say `-> RRect`. Copying the declaration through left a `T` no impl
  /// declares, which is the same mistake flattening made with fields one level
  /// down.
  var _implBinding = <String, IrType>{};

  void _emitBaseMethod(IrMethod need) {
    {
      _refuseShadowedGeneric(need);
      final have = _matching(need);
      // Rust does not collapse `Option<Option<X>>` the way Dart collapses
      // `T?` for a nullable `T`: `MessageCodec<Object?>.decodeMessage` is
      // `-> Option<T>` in the trait and the impl must say `Option<Option<..>>`
      // -- 16 `E0053`s, the "14 members" `_substituteType`'s comment gave up
      // on. Spelled out here, with the body wrapped to match below.
      final returns = _spelledReturn(
        type(_substituteType(need.returnType, _implBinding)),
      );
      final params = [
        // The forwarder's receiver is the trait's: `&mut self` when any
        // implementer writes in this method, or `ChangeNotifier::
        // add_listener(self, ..)` under `&self` is a mutability mismatch.
        if (!need.isStatic) _sharedMutation(need) ? '&mut self' : '&self',
        ...need.params.map((p) {
          // A parameter whose type *is* one of the base's type parameters has
          // to be written the way the impl header wrote that parameter, which
          // is owned: Rust substitutes `ChildType` with the
          // `Box<dyn RenderBox>` in `impl RenderObjectWithChildMixin<Box<dyn
          // RenderBox>>`, and a borrowed `&dyn RenderBox` here is a different
          // type from the one the trait declared.
          final substituted = _substituteType(p.type, _implBinding);
          final fromParameter = _implBinding.containsKey(p.type.name);
          return _param(
            IrParam(
              p.name,
              substituted,
              named: p.named,
              hasDefault: p.hasDefault,
              // Carried, or the impl writes `&dyn Fn` where the trait it
              // implements declared `Box<dyn Fn>`.
              kept: p.kept,
            ),
            owned: fromParameter,
          );
        }),
      ].join(', ');
      _line(
        'fn ${_methodName(need)}${_generics(need)}($params) -> '
        '$returns${_sizedBound(need)} {',
      );
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
        final call = _inherentCall(have, need);
        final concrete = type(have.returnType);
        // One `Option` short -- the override narrowed `T?` to `T`, which Dart
        // allows, or the trait's `T?` doubled up above -- is a `Some`.
        // The trait's future carries `+ '_` (see `_lifetimed`); the
        // inherent one is the same future without the spelling.
        _line(
          concrete == returns || _lifetimed(concrete) == returns
              ? call
              : returns == 'Option<$concrete>'
              ? 'Some($call)'
              : 'Box::new($call)',
        );
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
  String _inherentCall(IrMethod method, [IrMethod? through]) {
    // Dart lets an override *widen* an optional signature:
    // `OutlinedBorder.copyWith({side})` is overridden by
    // `BeveledRectangleBorder.copyWith({side, borderRadius})`. Rust does not,
    // so the trait method has fewer parameters than the inherent one it
    // delegates to -- and passing the inherent one's names through named a
    // `border_radius` that is not in scope, 30 times.
    //
    // What a caller reaching this through the trait would get in Dart is the
    // extra optionals *absent*, so that is what is passed: `None`. An extra
    // parameter that is not optional cannot be answered that way and the
    // delegation is refused instead of guessed at.
    // Positional parameters line up by **position**, not by name. Dart lets an
    // override rename them -- `Simulation.x(double time)` is overridden by
    // `x(double timeInSeconds)` -- and matching on the name called that a
    // widening and refused it, which left the trait unimplemented: 31 `E0046`s
    // for what is only a different word.
    final named = through == null
        ? null
        : {for (final p in through.params.where((p) => p.named)) p.name};
    final positional = through == null
        ? 0
        : through.params.where((p) => !p.named).length;
    // And the name to pass is the **caller's**, not the callee's. The
    // signature being written is the trait's, so `time` is what is in scope;
    // passing the inherent method's `timeInSeconds` names nothing.
    var at = -1;
    final args = method.params.map((p) {
      if (!p.named) at++;
      if (through == null) return snake(p.name);
      final supplied = p.named ? named!.contains(p.name) : at < positional;
      if (supplied) {
        final from = p.named
            ? through.params.firstWhere((q) => q.named && q.name == p.name)
            : through.params.where((q) => !q.named).elementAt(at);
        // A trait parameter doubled to `Option<Option<..>>` arrives one
        // `Option` deeper than the inherent method takes it.
        final doubled =
            _substituteType(from.type, _implBinding).name == 'Option';
        final passed = '${snake(from.name)}${doubled ? '.flatten()' : ''}';
        // Dart lets an override *widen* a parameter: `Equality<E>.equals(E,
        // E)` is implemented by `equals(Object? e1, Object? e2)`. The trait's
        // `E` arrives here and the inherent method wants the wider type, so
        // it is shared into `Rc<dyn Object>` (and `Some`d) on the way in.
        final traitType = _substituteType(from.type, _implBinding);
        final wider = p.type.name == 'Object' || p.type.name == 'dynamic';
        final narrowerGiven =
            traitType.name != 'Object' && traitType.name != 'dynamic';
        if (wider && narrowerGiven) {
          final shared = traitType.nullable || traitType.name == 'Option'
              ? '$passed.map(|v| std::rc::Rc::new(v) as std::rc::Rc<dyn Object>)'
              : '(std::rc::Rc::new($passed) as std::rc::Rc<dyn Object>)';
          return p.type.nullable &&
                  !(traitType.nullable || traitType.name == 'Option')
              ? 'Some($shared)'
              : shared;
        }
        if (p.type.nullable &&
            !traitType.nullable &&
            traitType.name != 'Option') {
          return 'Some($passed)';
        }
        return passed;
      }
      // The override's own default is the value the base "has no value for".
      final fallback = p.defaultValue;
      if (fallback != null) return expr(fallback);
      if (p.type.nullable) return 'None';
      throw Unsupported(
        'override widens `${method.name}` with `${p.name}`, '
            'which the base has no value for',
        '${cls.name}.${method.name}',
      );
    }).toList();
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
    // A setter's inherent name is `set_x` (see `_methodName`): the trait's
    // `set__status` forwarded to `Value::_status`, which is the getter.
    final name = op == null
        ? (method.isSetter ? 'set_${snake(method.name)}' : snake(method.name))
        : _operatorName(op);
    final call = '${cls.name}::$name(${['self', ...args].join(', ')})';
    // An `async fn` yields its own future type; the trait wants the boxed
    // one every `Future<T>` is here (`_NativeCodec::get_next_frame(self)`).
    return method.isAsync ? 'std::boxed::Box::pin($call)' : call;
  }

  void _emitConstructors() {
    for (final ctor in cls.constructors) {
      // Through `_member`, like every other member. Without it an
      // `Unsupported` from one constructor came out of `_emitStruct` and took
      // the **whole class** with it -- 410 classes that vanished because one
      // field was `late`. That is round 21's lesson, at a site it never
      // reached: the unit of refusal has to be the unit of work.
      _member(
        '${cls.name}.${ctor.name ?? "new"}',
        () => _emitConstructor(ctor),
      );
    }
  }

  void _emitConstructor(IrConstructor ctor) {
    // Dart's named constructors are Rust's associated functions already --
    // `EdgeInsets.all(8)` and `EdgeInsets::all(8.0)` are the same call, and the
    // unnamed one is `new` by Rust's convention. Nothing has to be encoded, so
    // nothing is: this is one of the places the two languages simply agree.
    final name = _ctorName(ctor.name);
    _doc(ctor.doc);
    // A parameter the constructor assigns -- `cullRect ??= Rect.largest`
    // inside a field initialiser, or in the body -- is `mut` (E0384).
    final assigned = <String>{
      for (final init in ctor.fieldInits.values)
        ..._assignedIn(IrExprStmt(init)),
      if (ctor.body != null) ..._assignedIn(ctor.body!),
    };
    final params = ctor.params
        .map(
          (p) =>
              '${assigned.contains(p.name) ? 'mut ' : ''}${snake(p.name)}: ${type(p.type)}',
        )
        .join(', ');
    // ..and the body's locals are `mut` by the same reckoning (`let` in a
    // constructor body was never `mut` once locals stopped being so by
    // default: 4 E0384s in `ParagraphStyle`).
    _reassigned = assigned;
    _cellLocals = {};
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
    // ..and one whose parameters are not all `Copy`: a `String` field is
    // initialised with `string.clone()` now, and a `const fn` may not call
    // it (E0015, 53 of them the round the clones arrived). The `static
    // const`s that needed `const fn` hold `Copy` values -- `Offset`,
    // `TextAlignVertical` -- and keep it.
    // ..nor one whose field initialisers clone -- `Color`, a `Copy` struct
    // the front end could not know is one, arrives as `color.clone()`.
    final constness =
        ctor.isConst &&
            ctor.body == null &&
            ctor.params.every((p) => _isCopy(type(p.type))) &&
            !ctor.fieldInits.values.any((e) => expr(e).contains('.clone()'))
        ? 'const '
        : '';
    // A counted class hands out a handle, not a value: everything that
    // holds one holds an `Rc`, so the constructor is where the first one is
    // made. A `const fn` cannot allocate, so a counted constructor is not one.
    final produces = cls.counted ? 'std::rc::Rc<Self>' : 'Self';
    _line(
      '${_vis(ctor.name ?? cls.name)}'
      '${cls.counted ? '' : constness}fn $name($params) -> $produces {',
    );
    _indent++;
    // This constructor's own temporaries first -- a `super(#t0)` passes them
    // -- and only then the base's, computed from them.
    for (final s in [...ctor.pre, ..._inheritedPre(ctor)]) {
      stmt(s);
    }
    final redirect = ctor.redirectTo;
    if (redirect != null) {
      // Everything this constructor does is hand its arguments to another one
      // of the same class. `Self::` because it is the same class; `_ctorName`
      // because the unnamed one is `new` here as it is above.
      final args = ctor.redirectArgs.map(expr).join(', ');
      _line('Self::${_ctorName(redirect.isEmpty ? null : redirect)}($args)');
      _indent--;
      _line('}');
      return;
    }
    for (final check in ctor.asserts) {
      stmt(check);
    }
    final inits = {..._inheritedInits(ctor), ...ctor.fieldInits};
    // The handle is made around the value: a counted class's constructor is
    // the one place an `Rc` comes from, and everything that holds one after
    // that holds the handle.
    // A `late` field whose initialiser mentions `this` -- `late final
    // nativeFilter = _ImageFilter.matrix(this)` -- starts absent in the
    // literal and is written right after it, when `__new` exists to be
    // named. Not a `late` one: it has no absence to start from, and stays
    // refused below.
    final deferred = <String, IrExpr>{
      for (final field in _allFields(cls))
        if (field.isLate &&
            (inits[field.name] ?? field.initial) != null &&
            _mentionsThis((inits[field.name] ?? field.initial)!))
          field.name: (inits[field.name] ?? field.initial)!,
    };
    final built = ctor.body != null || deferred.isNotEmpty;
    // A counted class is built *inside* its handle: the body's `this`
    // (`_recorder._canvas = this` in `_NativeCanvas`) is then the `Rc`
    // every holder wants, and the fields it writes are cells reached
    // through the handle just the same.
    final handleFirst = built && cls.counted;
    _line(
      !built
          ? (cls.counted ? 'std::rc::Rc::new(Self {' : 'Self {')
          : handleFirst
          ? 'let __new = std::rc::Rc::new(Self {'
          : 'let mut __new = Self {',
    );
    _indent++;
    for (final field in _allFields(cls)) {
      if (deferred.containsKey(field.name)) {
        _line(
          _inCell(field)
              ? '${snake(field.name)}: std::rc::Rc::new(std::cell::'
                    '${_isCopy(_heldType(field)) ? 'Cell' : 'RefCell'}'
                    '::new(None)),'
              : '${snake(field.name)}: None,',
        );
        continue;
      }
      // The constructor first, then the declaration's own value: Dart applies
      // the latter only where the former says nothing.
      var init = inits[field.name] ?? field.initial;
      if (init == null && field.type.nullable) {
        // A nullable Dart field with no initialiser *is* null. Rust needs the
        // value written down, and `None` is exactly it -- not a stand-in.
        init = const IrLiteral('null', IrType('Null', nullable: true));
      }
      if (init == null) {
        // Dart's `late`, which starts with no value at all. `None` is that,
        // and the reads unwrap. See `IrFieldDecl.isLate`.
        if (field.isLate) {
          _line(
            _inCell(field)
                ? '${snake(field.name)}: std::rc::Rc::new(std::cell::'
                      '${_isCopy(_heldType(field)) ? 'Cell' : 'RefCell'}'
                      '::new(None)),'
                : '${snake(field.name)}: None,',
          );
          continue;
        }
        // Not `late` and not nullable, so Dart guaranteed a value and this
        // compiler lost it -- a constructor it could not read, most often.
        throw Unsupported('field never initialised', field.name);
      }
      // A field whose declaration initialiser mentions `this`:
      // `late final nativeFilter = _ImageFilter.matrix(this)`. In Dart the
      // object already exists when that runs; in Rust the struct literal is
      // still being built and there is no `self` at all. 152 of these came
      // out as `*self` inside `Self { .. }`, which is not a thing.
      if (_mentionsThis(init)) {
        throw Unsupported(
          'a field initialised from `this`',
          '${cls.name}.${field.name}',
        );
      }
      final held = type(field.type);
      // A closure literal into a field of function type is an `Rc<dyn Fn>`
      // there, as a constant's is (see the statics): `DateFormat
      // .dateTimeConstructor` took a bare closure where the field's type
      // named the trait object.
      final value = field.type.isFunction && init is IrClosure && !init.boxed
          ? 'std::rc::Rc::new(${expr(init)})'
          : expr(init);
      _line(
        _inCell(field)
            ? '${snake(field.name)}: std::rc::Rc::new(std::cell::'
                  '${_isCopy(held) ? 'Cell' : 'RefCell'}::new($value)),'
            : '${snake(field.name)}: $value,',
      );
    }
    // The phantom fields the struct declaration added. They hold nothing, and
    // leaving them out of the literal is a missing field rather than a
    // harmless omission.
    for (final unused in _unusedParameters(cls)) {
      _line('_phantom_${snake(unused)}: std::marker::PhantomData,');
    }
    _indent--;
    final body = ctor.body;
    if (!built) {
      _line(cls.counted ? '})' : '}');
    } else {
      _line(handleFirst ? '});' : '};');
      // `this` inside the body is the value being built, not a `self` that
      // does not exist yet. `_selfName` is the same lever a free function
      // uses, so the body's `this.x = v` comes out as `__new.x = v`.
      final saved = _selfName;
      _selfName = '__new';
      for (final entry in deferred.entries) {
        final field = _allFields(cls).firstWhere((f) => f.name == entry.key);
        final value = 'Some(${expr(entry.value)})';
        _line(
          _inCell(field)
              ? (_isCopy(_heldType(field))
                    ? '__new.${snake(field.name)}.set($value);'
                    : '*__new.${snake(field.name)}.borrow_mut() = $value;')
              : '__new.${snake(field.name)} = $value;',
        );
      }
      if (body != null) stmt(body);
      _selfName = saved;
      _line(handleFirst || !cls.counted ? '__new' : 'std::rc::Rc::new(__new)');
    }
    _indent--;
    _line('}');
    _line('');
  }

  /// The class's `static final` fields, as module-level `LazyLock`s.
  ///
  /// Written outside the `impl` because Rust has no associated `static`, and
  /// named with the class in front so two classes' `defaults` do not collide.
  void _emitLazyStatics() {
    for (final constant in cls.constants) {
      if (!constant.isLazy) continue;
      _member('${cls.name}.${constant.name}', () {
        final held = type(constant.type);
        // Wrapped in `Isolate`, which is where "a Dart static is one per
        // isolate" is written down. A Rust `static` is one per process and so
        // must hold something `Sync`; `Box<dyn Fn(Image)>` is not, and that
        // was 94 `E0277`s. See the prelude for what the wrapper's `unsafe`
        // claims and when it stops being true.
        _doc(constant.doc);
        // Assignable, so a `RefCell` inside the `Isolate`: the same cell a
        // mutable top-level gets, read with `borrow` and written with
        // `borrow_mut` in `IrAssignStatic`.
        final cell = constant.isMutable ? 'std::cell::RefCell<$held>' : held;
        final made = constant.isMutable
            ? 'std::cell::RefCell::new(${constant.value is IrClosure && !(constant.value as IrClosure).boxed ? 'std::rc::Rc::new(${expr(constant.value)})' : expr(constant.value)})'
            : expr(constant.value);
        _line(
          '${_vis(constant.name)}static ${_lazyName(cls.name, constant.name)}: '
          'std::sync::LazyLock<Isolate<$cell>> = '
          'std::sync::LazyLock::new(|| Isolate($made));',
        );
        _line('');
      });
    }
  }

  void _emitConstants({String? prefix}) {
    for (final constant in cls.constants) {
      if (constant.isLazy) continue;
      // Each constant on its own: one that cannot be built is one constant
      // missing, not a class.
      _member(
        '${cls.name}.${constant.name}',
        () => _emitConstant(constant, prefix: prefix),
      );
    }
    if (cls.constants.isNotEmpty) _line('');
  }

  void _emitConstant(IrConstDecl constant, {String? prefix}) {
    if (!_constable(type(constant.type))) {
      throw Unsupported(
        'a `const` cannot hold a collection',
        '${cls.name}.${constant.name}',
      );
    }
    _doc(constant.doc);
    final spelled = prefix == null
        ? screamingSnake(constant.name)
        : screamingSnake('${prefix}_${constant.name}');
    _line(
      '${_vis(constant.name)}const $spelled: '
      '${type(constant.type)} = ${expr(constant.value)};',
    );
  }

  void _emitMethods() {
    for (final method in cls.methods) {
      if (method.operator != null) continue;
      _member('${cls.name}.${method.name}', () => _emitMethod(method));
    }
    // A concrete superclass's methods, on the subclass: `ValueNotifier
    // extends ChangeNotifier` has `ChangeNotifier`'s fields (flattened in)
    // and, in Dart, its methods -- `notifyListeners()` from `set value`. A
    // struct inherits nothing, so the body is emitted again here, over the
    // same field names. Only for an ancestor without type parameters (its
    // `T` is not this class's) and not overridden here.
    final have = <String>{
      for (final m in cls.methods) m.name,
      for (final f in _allFields(cls)) f.name,
    };
    for (final ancestor in _concreteAncestors()) {
      for (final method in ancestor.methods) {
        if (method.operator != null || method.isStatic) continue;
        // Nearest first: a name already seen is overridden below this one.
        if (!have.add(method.name)) continue;
        _member(
          '${cls.name}.${method.name} (from ${ancestor.name})',
          () => _emitMethod(method),
        );
      }
    }
  }

  /// The `extends` chain above this class, nearest first: the concrete,
  /// non-generic classes of this library whose methods a struct has to
  /// carry itself.
  List<IrClass> _concreteAncestors() {
    final out = <IrClass>[];
    var name = cls.superclass;
    final seen = <String>{cls.name};
    while (name != null && seen.add(name)) {
      final ancestor = library[name];
      if (ancestor == null || ancestor.isAbstract || ancestor.isEnum) break;
      if (_generics(ancestor).isNotEmpty) break;
      out.add(ancestor);
      name = ancestor.superclass;
    }
    return out;
  }

  void _emitMethod(IrMethod method, {String? as}) {
    {
      // Before the signature: whether a parameter needs `mut` is decided by the
      // body, and the signature is written first.
      _reassigned = _assignedIn(method.body);
      _cellLocals = {};
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
      _failure = _failureOf(method);
      _rustReturns = returns;
      _referenceParams = {
        for (final p in method.params)
          // Asked of the **emitted** type, not of the Dart name. `Object` is
          // the parameter of every `operator ==` and it is not one of this
          // package's abstract classes -- it is the prelude's trait -- so a
          // rule that consulted `library.isAbstract` missed all 251 of them
          // while `&dyn Object` was sitting in the signature. The same shape
          // as `_isCopy` two rounds ago: the ruler and its name disagreed.
          if (type(p.type, owned: false).startsWith('std::rc::Rc<dyn '))
            p.name: snake(p.name)
          // A counted class is an `Rc<Foo>` by value. The handle is not the
          // object, so the object is what gets asked.
          else if (library[p.type.name]?.counted ?? false)
            p.name: '&*${snake(p.name)}',
      };
      _line(
        '${_vis(method.name)}${method.isAsync ? "async " : ""}fn '
        '${as ?? _rustName(method)}${_generics(method)}($params) -> $returns {',
      );
      _indent++;
      _returns = method.returnType;
      _asyncBody = method.isAsync;
      // A failing `void` method that falls off its end still has to
      // produce its `Ok(())`: `_validateColorStops` ends in an `if`/`else`
      // that only ever returns `Err`, and the value of that `if` is `()`.
      final fallsOff =
          _failure != null &&
          type(method.returnType) == '()' &&
          !_alwaysReturns(method.body);
      stmt(method.body, tail: !fallsOff);
      if (fallsOff) _line('Ok(())');
      // `TileMode` to text as an `if`/`else if` chain over every variant with
      // no final `else`: Dart lets the body fall off the end (returning null
      // it would then refuse at runtime); Rust wants the last `if` to be an
      // expression of the return type. The chain is exhaustive by the
      // author's reckoning, and the line after it says so.
      if (!fallsOff) _closeOpenIf(method.body);
      _returns = null;
      _indent--;
      _line('}');
      _line('');
    }
  }

  /// After a body: the line that ends an open `if` chain, when the method
  /// has a value to return and the chain is how it returns it.
  void _closeOpenIf(IrStmt body) {
    final returns = _returns;
    if (returns == null || type(returns) == '()') return;
    if (_alwaysReturns(body) || !_endsInOpenIf(body)) return;
    _line('unreachable!("no branch of the if chain returned")');
  }

  /// Whether a body ends in an `if` chain that returns on every branch it
  /// has, and has no `else` to end it.
  bool _endsInOpenIf(IrStmt s) => switch (s) {
    IrBlock(:final statements) =>
      statements.isNotEmpty && _endsInOpenIf(statements.last),
    IrIf(:final then, :final otherwise) =>
      otherwise == null ? _alwaysReturns(then) : _endsInOpenIf(otherwise),
    _ => false,
  };

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
        _line('impl${_implGenerics(cls)} ${cls.name}${_generics(cls)} {');
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
        _asyncBody = method.isAsync;
        _reassigned = _assignedIn(method.body);
        _cellLocals = {};
        stmt(method.body, tail: true);
        _closeOpenIf(method.body);
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
      _line(
        'impl${_implGenerics(cls)} std::ops::$trait$generic for '
        '${cls.name}${_generics(cls)} {',
      );
      _indent++;
      _line('type Output = ${type(method.returnType)};');
      _line('');
      final params = [
        'self',
        if (rhs != null) '${snake(rhs.name)}: ${type(rhs.type)}',
      ].join(', ');
      // The body lives in an inherent method the trait impl forwards to.
      // Inside `impl std::ops::Add for Matrix3`, the trait is in scope, and
      // `cascaded.add(arg)` in the body of `operator +` -- Dart's own
      // `add`, `&mut self` -- resolved to the by-value `Add::add` first:
      // 8 `E0382`s and an infinite recursion in vector_math.
      final own = _operatorName(method.operator!);
      _line(
        'fn $fn($params) -> Self::Output { '
        'Self::$own(${['self', if (rhs != null) snake(rhs.name)].join(', ')}) }',
      );
      _indent--;
      _line('}');
      _line('');
      _line('impl${_implGenerics(cls)} ${cls.name}${_generics(cls)} {');
      _indent++;
      _line('pub fn $own($params) -> ${type(method.returnType)} {');
      _indent++;
      _returns = method.returnType;
      _asyncBody = method.isAsync;
      _reassigned = _assignedIn(method.body);
      _cellLocals = {};
      stmt(method.body, tail: true);
      _closeOpenIf(method.body);
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
    // The name is quoted *and* described: an empty one said
    // "operator `` has no Rust name", 367 times, which names neither the
    // operator nor where it came from.
    '' => throw Unsupported('a member with no name', '<empty>'),
    _ => throw Unsupported('operator `$op` has no Rust name', op),
  };

  /// A Rust-legal identifier for any Dart member name.
  ///
  /// `superFn` pastes the name into another identifier, so an operator's own
  /// spelling cannot go through: `superFn('AlignmentGeometry', '==')` produced
  /// `alignment_geometry_super_`, a name with nothing on the end of it.
  static String _identifier(String name) =>
      // Any letters at all: `___sendPlatformMessage$Method$FfiNative`, the
      // AOT lowering of an `@Native` external, is a name for `snake` to
      // clean, not an operator, and refusing it took `PlatformDispatcher.
      // instance` with it (20 callers).
      RegExp(r'[A-Za-z]').hasMatch(name) ? snake(name) : _operatorName(name);
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
  ///
  /// The `!` ones are the markers the backend spells out; they mutate exactly
  /// as the renamed ones do, and leaving them off here left the receiver
  /// without its `mut`.
  static const _mutatingListMethods = {
    'push',
    'extend',
    'clear',
    'pop',
    'insert',
    'remove',
    '!insert',
    '!remove_at',
    // The ordered `Map`'s own mutators. `put_if_absent` may write, so it
    // takes `&mut self`, and its receiver needs to say so.
    'put_if_absent',
  };

  /// Locals a mutating call is made on -- `xs.insert(..)` needs `let mut xs`,
  /// and a parameter needs `mut xs` in the signature. Rust says this out loud
  /// where Dart says nothing at all.
  final mutatedLocals = <String>{};

  /// Locals that are the receiver of some method call.
  final receiverLocals = <String>{};

  /// Whether a write target is `this`, or a chain of field reads from it.
  static bool _rootedAtThis(IrExpr? e) => switch (e) {
    null => true,
    IrThis() => true,
    IrField(:final target) => _rootedAtThis(target),
    _ => false,
  };

  /// Locals written by an assignment used for its value.
  final assignedLocals = <String>{};

  /// Whether `this` is read anywhere in what was walked.
  bool readsThis = false;

  /// Whether a closure in what was walked keeps a counted handle to `this`.
  bool holdsSelfClosure = false;

  /// Whether `this` is passed as an argument anywhere in what was walked.
  bool passesSelf = false;

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
        // A write through a local -- `entry.x = v` on a value the local owns
        // -- is what makes that local `let mut`. The cascade binding used to
        // be told separately; this covers it and the plain local alike.
        if (target is IrLocal) mutatedLocals.add(target.name);
        expression(s.value);
      case IrAssignTopLevel(:final value):
        // A library's own variable, not this object's: it goes through a cell
        // of its own, so writing one says nothing about `self`.
        expression(value);
      case IrAssignStatic(:final value):
        // A library's own variable, not this object's: it goes through a cell
        // of its own, so writing one says nothing about `self`.
        expression(value);
      case IrAssign(:final name):
        // Recorded here as well as in `_assignedIn`'s own walk, because this
        // one descends into closures and that one does not: `m.forEach((k, v)
        // { sum = sum + v; })` writes an outer local from inside a closure,
        // and nothing declared it `mut`.
        assignedLocals.add(name);
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
        // `return this` from a counted class hands out the handle.
        if (value is IrThis) passesSelf = true;
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
        if (target is IrLocal) mutatedLocals.add(target.name);
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
        // `this` handed to a call keeps the object, as a closure would:
        // `paragraph._paint(this, ..)` from a counted class needs the handle.
        // So does `this` shared into an `Object` slot (`!as_object`/`!rc`).
        if (args.any((a) => a is IrThis)) passesSelf = true;
        if (target is IrThis && (name == '!as_object' || name == '!rc')) {
          passesSelf = true;
        }
        // An *implicit* `this` reads it just as surely as a written one.
        // Dart lets a member be named without `this`, and a field initialiser
        // that says `transformPosition(transform, position)` is reading two
        // of them -- 110 `self` values in a constructor that has none, all of
        // them Flutter's `late final x = <something about this>`.
        if (target == null) readsThis = true;
        // `self.marks.push(x)` mutates a field, so the method takes
        // `&mut self` -- the same rule as writing the field outright, which is
        // what a `Vec` method that changes it amounts to.
        // Any local a method is called on may be changed by it: the callee's
        // receiver is unknown here, and `rotation.setFromRotation(r)` on an
        // immutable parameter was E0596. An unneeded `mut` is a warning.
        if (target is IrLocal) receiverLocals.add(target.name);
        if (_mutatingListMethods.contains(name)) {
          if (_rootedAtThis(target)) writesFields = true;
          if (target is IrLocal) mutatedLocals.add(target.name);
        }
        if (target != null) expression(target);
        args.forEach(expression);
      case IrField(:final target):
        if (target == null) readsThis = true;
        if (target != null) expression(target);
      case IrBinary(:final left, :final right):
        expression(left);
        expression(right);
      case IrUnary(:final operand):
        expression(operand);
      case IrNullCheck(:final operand):
        expression(operand);
      case IrDowncast(:final target):
        expression(target);
      case IrSome(:final value):
        expression(value);
      case IrCast(:final value):
        expression(value);
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
        // `FlutterView(id, this, ..)` from a counted class hands out the handle.
        if (args.any((a) => a is IrThis)) passesSelf = true;
        args.forEach(expression);
      case IrNew(:final args):
        if (args.any((a) => a is IrThis)) passesSelf = true;
        args.forEach(expression);
      case IrSuperCall(:final args):
        args.forEach(expression);
      case IrIs(:final expr):
        expression(expr);
      case IrClosure(:final body, :final holdsSelf):
        if (holdsSelf) holdsSelfClosure = true;
        statement(body);
      case IrCallValue(:final target, :final args):
        // `this` handed to a closure call keeps the object too.
        if (args.any((a) => a is IrThis)) passesSelf = true;
        expression(target);
        args.forEach(expression);
      case IrBlockValue(:final statements, :final value):
        statements.forEach(statement);
        expression(value);
      case IrConstInstance(:final fields):
        fields.values.forEach(expression);
      case IrAwait(:final operand):
        expression(operand);
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
      case IrRecord(:final fields):
        fields.forEach(expression);
      case IrRecordField(:final record):
        expression(record);
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
      case IrThis():
        readsThis = true;
      case IrLiteral():
      case IrLocal():
      case IrStatic():
      case IrTopLevel():
      case IrBound():
    }
  }
}
