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

import 'coerce.dart';

import 'dart:io' show Platform, stderr;

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
/// `dart:core` collections a value is downcast to, by their spelling here.
const _downcastNames = {'List': 'Vec'};

const _operatorTraits = {
  '+': ('Add', 'add'),
  '-': ('Sub', 'sub'),
  '*': ('Mul', 'mul'),
  '/': ('Div', 'div'),
  '%': ('Rem', 'rem'),
  'unary-': ('Neg', 'neg'),
  // The bitwise ones too: `Offset & Size` is `Rect` upstream, and without
  // the impl the call site's `a & b` had no operator to land on (103 at
  // ws300).
  '&': ('BitAnd', 'bitand'),
  '|': ('BitOr', 'bitor'),
  '^': ('BitXor', 'bitxor'),
  '<<': ('Shl', 'shl'),
  '>>': ('Shr', 'shr'),
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

  late final TypeWorld _world = _BackendWorld(this);

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
  bool _member(
    String what,
    void Function() body, {
    void Function(String reason)? stub,
  }) {
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
      // The refusal stays written (it is what the count reads); under it,
      // when the caller can, the member's signature over a body that says
      // so at runtime -- so a reference to it still compiles (see the front
      // end's `_stubFor` for the same policy one stage earlier).
      if (stub != null) {
        final after = _out.length;
        try {
          stub('$error');
        } on Unsupported {
          _out.removeRange(after, _out.length);
          _indent = indent;
        }
      }
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
    // Dart's `void?` is `void`, and the prelude's unit says so (`<() as
    // DartNullable>::Or = ()`): no `Option` around it.
    if (t.name == 'void' && t.nullable) return '()';
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
      final returns = _wrapped(type(t.returns!));
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
    // A projected `T?` (`IrType.projected`): `<T as DartNullable>::Or`,
    // with whatever stands for `T` -- a type parameter in a declaration,
    // the type put in for it in an impl's signature, where rustc compares
    // the spelling with the trait's rather than the normalised type.
    if (t.projected) {
      final inner = type(IrType(t.name, arguments: t.arguments));
      return '<$inner as DartNullable>::Or';
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
    // ..a `DartFuture<T>` (the prelude's shared, clonable, eager future),
    // owned or borrowed alike, since the runtime ruler's second panic
    // (run430): a `Future<bool>` read out of a field was cloned, and a
    // `Pin<Box<dyn Future>>` cannot be.
    if (t.name == 'Future' && t.arguments.length == 1) {
      final future = 'DartFuture<${type(t.arguments.single)}>';
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
    // A nullable type parameter in a signature: the associated type that
    // collapses `T?` with `T` bound to `X?` (see `IrType.projected`).
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
      // A local a closure captured is the closure's own copy, and a `Fn`
      // closure may not give it away: read as a clone, so a use by value
      // (`instance.on_start = on_start`) moves the clone (107 E0507).
      IrLocal(:final name) =>
        _cellLocals.containsKey(name)
            ? '${_cellLocals[name]! ? '${snake(name)}.get()' : '{ let __r = ${snake(name)}.borrow().clone(); __r }'}'
                  '${_lateCellLocals.contains(name) ? '.unwrap()' : ''}'
            : _closureCaptured.contains(name)
            ? '${snake(name)}.clone()'
            : snake(name),
      // `this` in a counted class is the handle -- one more `Rc`, not the
      // value behind it. `*self` there moved out of a `&Rc<Self>`, and the
      // getter `get owner => this` came out returning a bare struct where
      // every other module spells that class `Rc<..>`: 18 `E0053`s.
      // `Matrix3.copy(this)` in `clone()`: `*self` moves out of a shared
      // reference unless the class is `Copy`.
      // In a constructor `this` is the local being built (`__new`), a
      // value and not a reference: no `*`.
      // `this_` is a `&__Self` whatever the mode: its clone is a reference
      // (91 lifetime errors at ws334), its handle is `dart_self_<trait>()`.
      IrThis() =>
        _selfByValue
            ? _selfName
            : _fieldsAreAccessors || _selfName == 'this_'
            ? '$_selfName.dart_self_${snakeRaw(cls.name)}()'
            // A counted object as a value is its own handle: a clone of
            // the struct would be a second object sharing one `DartSelf`
            // (`_RenderObjectSemantics(this)` in a lazy initializer,
            // run460).
            : cls.counted
            ? '$_selfName.dart_self_ref().get()'
            : _selfIsHandle || !_classIsCopy(cls, {}) || _selfName != 'self'
            ? '$_selfName.clone()'
            : '*$_selfName',
      IrField(:final target, :final name, :final onEnum, :final owner) =>
        _fieldRead(target, name, onEnum, owner),
      IrStatic(:final owner, :final name, :final isEnumValue) => _staticRead(
        owner,
        name,
        isEnumValue,
      ),
      // A comparison has no expected type: an operand shared into
      // `Object` for it says so (`Some(Rc::new("dark"))` against an
      // `Option<Rc<dyn Object>>` scrutinee in a pattern switch was an
      // `Option<Rc<String>>`, `_updateUserSettingsData`, run472).
      IrBinary(:final op, :final left, :final right, :final type) => _binary(
        op,
        op == '==' || op == '!=' ? _explicitUpcast(_plain(left)) : left,
        op == '==' || op == '!=' ? _explicitUpcast(_plain(right)) : right,
        type,
      ),
      IrUnary(:final op, :final operand) => '($op${expr(operand)})',
      IrCall(
        :final target,
        :final name,
        :final args,
        :final qualifier,
        :final receiverClass,
        :final fails,
        :final diverges,
        :final typeArguments,
      ) =>
        _diverging(
          _call(
            target,
            name,
            args,
            qualifier: qualifier,
            receiverClass: receiverClass,
            fails: fails && !diverges,
            typeArguments: typeArguments,
            asyncFn: e.asyncFn,
            asyncTarget: e.asyncTarget,
            resultType: e.rustType,
          ),
          diverges && fails,
        ),
      IrStaticCall(
        :final owner,
        :final name,
        :final args,
        :final fails,
        :final diverges,
        :final typeArguments,
        :final module,
      ) =>
        _diverging(
          _staticCallFailing(
            owner,
            name,
            args,
            fails && !diverges,
            typeArguments,
            e.asyncFn,
            module,
          ),
          diverges && fails,
        ),
      IrNew(:final type, :final args, :final constructor) => _newFailing(
        type,
        args,
        constructor,
      ),
      // Parenthesised: a struct literal is not allowed bare in an `if`
      // condition, and `if self._state == _State { .. } {` did not parse.
      // ..and a counted class's constant is its handle, as its
      // constructor's result is (`const StandardMethodCodec()` holding a
      // `StandardMessageCodec`, 12 `Rc<X> <= X` at ws421).
      IrConstInstance(:final type, :final fields) =>
        (library[type.name]?.counted ?? false)
            ? 'dart_rc(${_constInstance(type, fields)})'
            : '(${_constInstance(type, fields)})',
      // Rust puts it after the expression and Dart before it, which is the
      // whole of the difference.
      // The future's output is a `Result`: the `?` goes after the await.
      // `await f` on a `Future<T>?`: null stays null (`await proxy.send(..)`
      // where `send` returns `Future<ByteData?>?`).
      // ..and `T?` of a `T` already nullable is `T`: awaiting a
      // `Future<ByteData?>?` is a `ByteData?`, not an `Option<Option<..>>`
      // (`BinaryMessenger.send`, ws474).
      IrAwait(:final operand)
          when operand.rustType?.name == 'Future' &&
              (operand.rustType?.nullable ?? false) =>
        (operand.rustType!.arguments.isNotEmpty &&
                operand.rustType!.arguments.first.nullable)
            ? '(match ${_awaitOperand(operand)} { Some(__f) => __f.await$_propagate, None => None })'
            : '(match ${_awaitOperand(operand)} { Some(__f) => Some(__f.await$_propagate), None => None })',
      IrAwait(:final operand) => '${_awaitOperand(operand)}.await$_propagate',
      IrIdentical(:final left, :final right) => _identical(left, right),
      // `return Err(e)` has type `!`, so it fits where a value was wanted.
      IrThrowValue(:final value) => _thrown(value),
      IrInterpolation(:final parts) => _interpolation(parts),
      // Dart indexes with an `int`; Rust wants a `usize`.
      // A clone: an indexed read is a value, and the element is behind the
      // list's reference (`cannot move out of index of Vec<..>`).
      // The target in parentheses when it is a block: `{ .. }[0]` reads
      // as a block statement and an array (`[{integer}; 1]`, ws511).
      IrIndex(:final target, :final index) => () {
        final t = expr(target);
        final wrapped = t.startsWith('{') ? '($t)' : t;
        return '$wrapped[${expr(index)} as usize].clone()';
      }(),
      // A closure literal among the elements of a list of functions is an
      // `Rc<dyn Fn>` there, as a field's or a constant's is: `DateFormat`'s
      // `_fieldConstructors` is a `vec!` of three of them.
      // A `vec![..]` is typed by its *first* element: an implicit upcast
      // there is spelled (`Rc::new(x) as Rc<dyn Object>`), or the second
      // element's other class does not fit (`Object.hashAll([isChecked,
      // isButton])`, 17 at ws421). The rest coerce to the first.
      // ..and one whose elements can fail is built element by element:
      // in `vec![a?, b?, ..]` every `?` exit drops every earlier element,
      // and a literal of 3038 failing constructors (the gallery's code
      // viewer) is 4.6 million drops of codegen -- one function held
      // `rustc` for half an hour at 27 GB (run429). Pushed one at a time,
      // a failure drops the one partial `Vec`.
      IrListLiteral(:final elements, :final element)
          when elements.isNotEmpty && _WalkSelf.failingIn(elements) =>
        '{ let mut __v = Vec::new(); '
            '${elements.indexed.map((ix) => '__v.push(${_listElement(ix.$1, ix.$2, element)});').join(' ')}'
            ' __v }',
      IrListLiteral(:final elements, :final element) =>
        'vec![${elements.indexed.map((ix) => _listElement(ix.$1, ix.$2, element)).join(', ')}]',
      IrRecord(:final fields) => '(${fields.map(expr).join(', ')})',
      IrRecordField(:final record, :final index) => '${expr(record)}.$index',
      // Spells its key and value types: nothing else says them when the
      // slot is an `Rc<dyn Object>` (E0283, `K` on `Map`), and a written
      // one typed by its first entry alone was untyped where that entry's
      // value is `null` (`{'a': null, 'b': 2}` into `Map<Object?, Object?>`,
      // ws495). Through `from_pairs`, whose array parameter is of the
      // spelled types, so every entry coerces to them; the first entry's
      // upcasts are spelled all the same.
      IrMapLiteral(:final entries, :final key, :final value) =>
        'Map::<${type(key)}, ${type(value)}>::from_pairs(['
            '${entries.indexed.map((ie) {
              final e = ie.$1 == 0 ? (_explicitUpcast(ie.$2.$1), _explicitUpcast(ie.$2.$2)) : ie.$2;
              return '(${expr(e.$1)}, ${expr(e.$2)})';
            }).join(', ')}'
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
      IrFunctionRef(:final owner, :final name) => _functionRef(
        owner,
        name,
        e.rustType,
      ),
      IrAssignValue(:final name, :final value) =>
        // The stored copy is a clone: a non-`Copy` value moved into the
        // local was gone by the time the expression yielded it (E0382, 17).
        '{ let __set = ${expr(value)}; ${snake(name)} = __set.clone(); __set }',
      IrSetValue(:final target, :final name, :final value) => _setValue(
        target,
        name,
        value,
      ),
      // The branches have no expected type from each other: an upcast in
      // one is explicit (`dart_object(FontWeight)` against
      // `Rc::new("unspecified")`, ws476).
      IrConditional(:final condition, :final then, :final otherwise) =>
        'if ${expr(condition)} { ${expr(_explicitUpcast(then))} } else { ${expr(_explicitUpcast(otherwise))} }',
      IrIs(expr: final operand, :final type, :final negated) => _isTest(
        operand,
        type,
        negated,
      ),
      // A super function returns `Result`; `Object.toString` is the prelude's.
      // An async super function is a `DartFuture`, not a `Result`: no `?`
      // (`super.handleSystemMessage(..)` in `WidgetsBinding`, ws446).
      IrSuperCall(
        :final base,
        :final name,
        :final args,
        :final isSetter,
        :final baseArguments,
        :final typeArguments,
      ) =>
        base == 'Object'
            ? _superCall(base, name, args)
            : '${_superCall(base, name, args, isSetter: isSetter, baseArguments: baseArguments, typeArguments: typeArguments)}${(library[base]?.methods.any((m) => m.name == name && !m.isStatic && m.isAsync) ?? false) ? '' : _propagate}',
      // A local's `!` clones first: `a!.axis` and then `a!.value` moved
      // `a` at the first (E0382); a `Copy` local clones for free.
      IrNullCheck(:final operand) =>
        operand is IrLocal
            ? '${expr(operand)}.clone().unwrap()'
            : '${expr(_plain(operand))}.unwrap()',
      // A closure inside `Some(..)` is the `Rc<dyn Fn>` its slot holds.
      IrNullableOf(:final value, :final parameter, :final toOption) =>
        '<${_nullableOf(parameter)} as DartNullable>::${toOption ? 'option' : 'from_option'}(${expr(value)})',
      IrSome(:final value) =>
        value is IrClosure && !value.boxed
            ? 'Some(std::rc::Rc::new(${expr(value)}))'
            : 'Some(${expr(value)})',
      // Inside `as_ref().map(|it| ..)` the bound value is a reference, and
      // a reference does not cast: `lerpDouble`'s `a as double` on an
      // `Option<f64>` (E0606).
      IrCast(:final value, :final rust) =>
        value is IrBound && !_boundByValue
            ? '(*${expr(value)} as $rust)'
            : '(${expr(value)} as $rust)',
      // `state as T?` with `T` a type parameter: by id, and the `Option`
      // stays one (see `dart_cast_any`).
      IrCastTo(:final target, :final type) when _isTypeParam(type.name) =>
        '${expr(target)}.dart_cast_any::<${type.name}>()'
            '${type.nullable ? "" : ".unwrap()"}',
      // A nullable target keeps the `Option` the cast hands back.
      IrCastTo(:final target, :final type) =>
        '${expr(target)}.dart_cast_to::<${_dynOf(type)}>()'
            '${type.nullable ? "" : ".unwrap()"}',
      IrSuperDispatch(
        :final receiver,
        :final base,
        :final name,
        :final args,
        :final typeArguments,
        :final classArity,
        :final castTo,
      ) =>
        _superDispatch(
          receiver,
          base,
          name,
          args,
          typeArguments,
          classArity,
          castTo,
        ),
      // A collection of `dynamic`/`Object?`/scalars: the object's own
      // element representation may differ (`Map<String, dynamic>` from
      // `json.decode` cast `as Map<String, Object?>`), and Dart's runtime
      // type does not tell them apart; the prelude converts (ws473).
      IrDowncast(:final target, :final type, :final arguments)
          when (type == 'Map' || type == 'List') &&
              arguments.isNotEmpty &&
              arguments.every(_dynamicRepresentable) =>
        '${type == 'Map' ? 'dart_cast_map' : 'dart_cast_list'}::<${arguments.map(this.type).join(', ')}>(&${expr(target)}).unwrap()',
      // A type parameter: its own conversion (`FromDynamic`, in every
      // bound), as `!as_opt` above -- `Any` knows one concrete type, and a
      // `T` bound to `Rc<dyn Object>` is none.
      IrDowncast(:final target, :final type, :final arguments)
          when arguments.isEmpty && _isTypeParam(type) =>
        '<$type as FromDynamic>::from_dynamic(&(${expr(target)} as std::rc::Rc<dyn Object>)).unwrap()',
      IrDowncast(:final target, :final type, :final arguments) =>
        '${_asAny(target)}.downcast_ref::<${_downcastNames[type] ?? type}${arguments.isEmpty ? '' : '<${arguments.map(this.type).join(', ')}>'}>().unwrap()',
      IrDynamicDispatch(:final receiver, :final arms) => _dispatch(
        receiver,
        arms,
      ),
      // A mutable one is read through its cell: two derefs for the `LazyLock`
      // and the `Isolate`, then a `borrow`.
      IrTopLevel(:final name) =>
        _isMutableTopLevel(name)
            ? '({ let __r = (**${screamingSnake(name)}).borrow().clone(); __r })'
            : _isLazyConst(name)
            ? '(**${screamingSnake(name)}).clone()'
            : screamingSnake(name),
      // `x == null` on a `dynamic`: the handle is never an `Option`; Dart's
      // null is the `Null` object inside it (`dart_nullable`).
      IrIsNull(:final operand) =>
        operand.rustType != null &&
                !operand.rustType!.nullable &&
                (operand.rustType!.name == 'dynamic' ||
                    operand.rustType!.name == 'Object')
            ? 'dart_nullable(${expr(operand)}.clone()).is_none()'
            : '${expr(_plain(operand))}.is_none()',
      IrIfNull() => _ifNull(_plainIfNull(e as IrIfNull)),
      // `as_ref()`: `a?.b` reads `a`, and `a` is a field or a loop variable
      // behind a reference far more often than an owned `Option` -- `.map`
      // alone moved out of `*child` (E0507). A body that needs the value
      // rather than a reference to it now says so at the use.
      // Under the Result model the body may `?`: it runs inside a closure
      // returning `Result`, and `transpose()?` lifts the error out of the
      // `Option` again.
      // ..except a scalar, which is `Copy`: the body gets the value
      // itself, and `it as f64` needs no dereference (`a?.toDouble()` in
      // `lerpDouble`, ws473).
      IrNullAware(:final receiver, :final body, :final flatten) => _nullAware(
        receiver,
        body,
        flatten,
      ),
      // A counted class's constructor already hands out an `Rc`.
      IrMapElements(:final collection, :final kind, :final body) =>
        kind == 'Future'
            ? '${expr(collection)}.map(|v| ${expr(body)})'
            : kind == 'Set'
            ? 'Set::of(${expr(collection)}.into_iter().map(|v| ${expr(body)}).collect::<Vec<_>>())'
            : kind == 'Map'
            ? 'Map::from(${expr(collection)}.into_iter().map(|(k, v)| ${expr(body)}).collect::<Vec<_>>())'
            : '${expr(collection)}.into_iter().map(|v| ${expr(body)}).collect::<Vec<_>>()',
      // `this` shared as an object is its own handle (`!as_object`).
      IrUpcast(:final value, :final type)
          when value is IrThis && type.name == 'Object' =>
        _call(value, '!as_object', const []),
      // The value's class by its recorded type first (a local, a call), by
      // its shape (a constructor call) otherwise.
      IrUpcast(:final value, :final type, :final handle, :final explicit) =>
        handle ||
                (library[value.rustType?.name ?? _concreteType(value).name]
                        ?.counted ??
                    false)
            ? (explicit
                  ? '(${_handleOf(value)} as ${this.type(type)})'
                  : _handleOf(value))
            // An enum is not a `DartAny`: a plain handle (49 `_ScaffoldSlot:
            // DartAny` at ws367).
            : (library[value.rustType?.name ?? _concreteType(value).name]
                      ?.isEnum ==
                  false)
            ? (explicit
                  ? '(dart_object(${expr(value)}) as ${this.type(type)})'
                  : 'dart_object(${expr(value)})')
            : (explicit
                  ? '(std::rc::Rc::new(${expr(value)}) as ${this.type(type)})'
                  : 'std::rc::Rc::new(${expr(value)})'),
      IrBound() => _boundName,
      IrClosure() => _closure(e as IrClosure),
      // A function value returns `Result` like everything else.
      IrCallValue(:final target, :final args) =>
        '(${expr(target)})(${args.map(expr).join(', ')})$_propagate',
      IrBlockValue() => _blockValue(e as IrBlockValue),
    };
  }

  /// Statements then a value, as a Rust block expression.
  ///
  /// The binding is `mut` only when a step writes to it, for the reason
  /// `let mut` is not applied everywhere: the test crate denies `unused_mut`,
  /// so an unneeded one is a build error rather than a warning nobody reads.
  /// A call to a `Never` function: its `Result<Infallible, E>` is either
  /// the error, propagated, or a value that cannot exist -- so the
  /// expression is the `!` Dart meant, whatever the slot wants.
  String _diverging(String call, bool diverges) => !diverges
      ? call
      : _failure != null
      ? '(match $call { Ok(__n) => match __n {}, Err(__e) => return Err(__e) })'
      : '(match $call { Ok(__n) => match __n {}, Err(__e) => panic!("uncaught Dart exception: {:?}", __e) })';

  String _staticCallFailing(
    String? owner,
    String name,
    List<IrExpr> args,
    bool fails, [
    List<IrType> typeArguments = const [],
    bool asyncFn = false,
    String? module,
  ]) {
    final awaited = _awaiting;
    _awaiting = false;
    // A top-level of another module this module shadows by name is spelled
    // by its module (see `IrStaticCall.module`).
    final call = owner == null && module != null
        ? 'crate::$module::${_staticCall(owner, name, args, typeArguments)}'
        : _staticCall(owner, name, args, typeArguments);
    final failing =
        fails || (_resultModel && _preludeFailingStatics.contains(name));
    return _asyncValue(
      failing && !awaited && !asyncFn ? '$call$_propagate' : call,
      asyncFn && !awaited,
    );
  }

  /// An `async fn` called and not awaited: its future is the value, boxed
  /// as every `Future<T>` is here (`return _handleCommitBackGesture()`,
  /// ws428). Not `?`ed: an `async fn` fails inside its future.
  String _asyncValue(String call, bool boxed) => call;

  /// A translated class's constructor returns `Result` like any function;
  /// the prelude's do not.
  String _newFailing(IrType t, List<IrExpr> args, String? constructor) {
    final awaited = _awaiting;
    _awaiting = false;
    final call = _new(t, args, constructor);
    final translated = _resultModel && library[t.name] != null;
    return translated && !awaited ? '$call$_propagate' : call;
  }

  /// The operand of an `await`. A call reaching an `async fn` as one
  /// (`asyncFn`) is the future itself and its `?` goes after the `.await`;
  /// any other failing call returns its future inside the `Result` -- a
  /// trait method, a plain function that built a `Future<T>` -- and is
  /// unwrapped first, `f()?.await?` (118 "is not a future" at ws425).
  String _awaitOperand(IrExpr operand) {
    final asyncFn = switch (operand) {
      IrCall(:final asyncFn) => asyncFn,
      IrStaticCall(:final asyncFn) => asyncFn,
      _ => false,
    };
    if (asyncFn) {
      _awaiting = true;
      final text = expr(operand);
      _awaiting = false;
      return text;
    }
    return expr(operand);
  }

  String _blockValue(IrBlockValue node) {
    // A binding of a value the AOT compiler removed makes the whole block
    // unreachable, and a `let __t = unreachable!(..)` has no type for the
    // `Some(__t.clone())` after it (115 "type annotations needed").
    for (final statement in node.statements) {
      if (statement is IrLocalDecl) {
        final init = statement.init;
        if (init is IrLiteral && init.value.startsWith('unreachable!')) {
          return init.value;
        }
      }
    }
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
      if (usesBound) 'let $_boundName = $_boundName.clone();',
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
    // Nor is it inside the try body's flow closure: a `return` in it is
    // the closure's own (`Ok(Some(..))` in `|x| builder.setDay(x)`).
    _inFlowClosure = false;
    if (node.holdsSelf) _selfName = _countedSelf;
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
    _body(node.body, node.isAsync ? _awaited(node.returns) : node.returns);
    _asyncBody = savedAsyncBody;
    _failure = savedFailure;
    _rustReturns = savedRustReturns;
    _inFlowClosure = savedFlow;
    _selfName = savedSelf;
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
    _lateCellLocals = savedLateCells;
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
      !field.isFinal &&
      _supertypesOf(owner).any(
        (t) =>
            library.isAbstract(t.name) &&
            t.fields.any((f) => f.name == field.name && !f.isFinal) &&
            _fieldsWrittenBy(t).contains(field.name),
      );

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
        return found;
      });

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
  bool _handsCell(IrFieldDecl field) =>
      !field.isLate && _isMutableCollection(type(field.type));

  String _cellType(String held) => 'std::rc::Rc<std::cell::RefCell<$held>>';

  static bool _isMutableCollection(String rust) =>
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

  /// An erased twin's result back to what the call declared (see the
  /// prelude's `CastErased`). A projected `T?` -- this declaration's own
  /// parameter, nullable -- has no `CastErased` of its own: the value
  /// comes back as the `Option<T>` and goes out through `from_option`,
  /// as any projected value does (`find<T>()` returning `T?`, ws496).
  /// A class's method by name, this class's or one of its ancestors'.
  IrMethod? _methodOf(String? className, String name) {
    var c = className == null ? null : library[className];
    while (c != null) {
      final own = [
        ...c.methods,
        ...c.abstractMethods,
      ].where((m) => m.name == name && !m.isStatic).firstOrNull;
      if (own != null) return own;
      c = c.superclass == null ? null : library[c.superclass!];
    }
    return null;
  }

  String _erasedCast(IrType resultType, String call, {IrMethod? method}) {
    // A twin whose return does not mention the method's own parameters
    // (`getElementForInheritedWidgetOfExactType<T>` returns an
    // `InheritedElement?`) hands back the declared type already: no cast
    // (`CastErased<Option<Rc<dyn InheritedElement>>>` asked of itself,
    // ws503).
    if (method != null &&
        type(_substituteType(method.returnType, _erasure(method))) ==
            type(method.returnType)) {
      return call;
    }
    if (resultType.projected && resultType.nullable) {
      final inner = type(
        IrType(resultType.name, arguments: resultType.arguments),
      );
      return '<$inner as DartNullable>::from_option('
          'dart_cast_erased::<Option<$inner>, _>($call))';
    }
    return 'dart_cast_erased::<${type(resultType)}, _>($call)';
  }

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
    // A `match` on a place moves out of it, and the place lives on
    // (`final child = inactive ?? create(); if (inactive != null) ..` in
    // `Element.inflateWidget`, ws494): a local or a field of `this` is
    // read by clone.
    final operand = node.left;
    // The scrutinee bound first: a `match` keeps its scrutinee's
    // temporaries -- the `Ref` of a `.borrow().clone()` -- alive through
    // every arm, and the `None` arm of `_instance ??= X()` on a static
    // cell wrote through `borrow_mut()` into it ("already borrowed",
    // run516). A `let` drops them at its own end.
    final read = operand is IrLocal
        ? '${expr(operand)}.clone()'
        : expr(operand);
    final left = '{ let __scrutinee = $read; __scrutinee }';
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
    // An operator on an open class's handle: `Rc<dyn Size> * f64` has no
    // `impl std::ops::Mul` to land on (the orphan rule: `Rc` is not
    // fundamental), so it is the trait's method, which fails like any
    // method (`Size.lerp`, ws473).
    final leftName = left.rustType?.name;
    final mapping = _operatorTraits[op];
    if (mapping != null &&
        leftName != null &&
        library[leftName] != null &&
        library.isAbstract(leftName)) {
      return '${expr(left)}.op_${mapping.$2}(${expr(right)})$_propagate';
    }
    // ..and on a counted class's handle: the `impl std::ops::Mul` is the
    // struct's, taking values, so each handle operand is the value it
    // holds, cloned (`Rc<Matrix4> * Rc<Matrix4>`, ws511).
    if (mapping != null) {
      String operand(IrExpr e) {
        final name = e.rustType?.name;
        final counted = name != null && (library[name]?.counted ?? false);
        return counted && !e.rustType!.nullable
            ? '(*${expr(e)}).clone()'
            : expr(e);
      }

      final l = operand(left);
      final r = operand(right);
      if (l != expr(left) || r != expr(right)) return '($l $op $r)';
    }
    // `==` on a type parameter's values (`T`, `T?`) is Dart's `==`, the
    // prelude's `DartEq`, which every parameter carries; `PartialEq` is
    // not asked of one (`selected == value` on a `T?` in
    // `CupertinoSegmentedControl`, 7 at ws460).
    // ..and on any object that is not a primitive: Dart's `==` is the
    // class's `operator ==`, which is `DartEq` here, taken by reference
    // (`==` on two `Rc<dyn Size>` moved its operand, E0382, 53 at ws464).
    if ((op == '==' || op == '!=') &&
        (_ownsParameter(left.rustType) ||
            _ownsParameter(right.rustType) ||
            (_objectLike(left.rustType) && _objectLike(right.rustType)))) {
      // `DartEq` compares two of the *left's* type: the right operand
      // was shared into `Object` for Dart's `operator ==(Object)`, and is
      // shared into the left's trait instead (`&Rc<dyn Object>` where
      // `&Rc<dyn Color>` was wanted, 12 at ws467).
      final leftType = left.rustType;
      final bare = right is IrUpcast && right.type.name == 'Object'
          ? right.value
          : right;
      // ..and where this module's world cannot classify the value (a
      // struct of another library), the `Object` sharing stands: the
      // prelude's `dart_object` takes the trait the comparison wants.
      // Two handles of one trait at different instantiations (`Route<T>`
      // against the navigator's `Route<dynamic>`, ws505): Dart's `==` on
      // them is identity, and only a thin pointer can compare the two.
      final rightType = right.rustType;
      if (leftType != null &&
          rightType != null &&
          leftType.name == rightType.name &&
          !leftType.nullable &&
          !rightType.nullable &&
          library.isAbstract(leftType.name) &&
          leftType.arguments.isNotEmpty &&
          leftType.arguments.toString() != rightType.arguments.toString()) {
        final same = 'dart_identical_any(&${expr(left)}, &${expr(right)})';
        return op == '==' ? same : '(!$same)';
      }
      final coerced = leftType != null && bare.rustType != null
          ? coerceInto(bare, leftType, _world, inClosure: true)
          : bare;
      final other = identical(coerced, bare) ? right : coerced;
      final eq = '${expr(left)}.dart_eq(&${expr(other)})';
      return op == '==' ? eq : '(!$eq)';
    }
    return '(${expr(left)} $op ${expr(right)})';
  }

  /// The type a `<T as DartNullable>` projection names: the parameter
  /// itself when it is one in scope, else the concrete type it was
  /// substituted with, spelled (`<Rc<dyn Object> as DartNullable>`, not
  /// `<Object as ..>`: E0782 26 and `dynamic` 25 at ws465).
  String _nullableOf(String parameter) {
    if (cls.typeParameters.contains(parameter) ||
        _methodTypeParams.contains(parameter)) {
      return parameter;
    }
    return type(IrType(parameter));
  }

  /// Whether a type is a translated class's, a trait's, or a collection's
  /// -- anything `==` compares by `DartEq` rather than by value.
  bool _objectLike(IrType? t) {
    if (t == null || t.isFunction) return false;
    final name = t.name;
    // A `dynamic` (an `Object?`) compares by `DartEq` too: the raw `==`
    // on two `Rc<dyn Object>` moved its right operand (E0382, a pattern
    // switch on `data['platformBrightness']`, run502), and the prelude's
    // `DartEq for dyn Object` is the value comparison either way.
    if (const {
      'int',
      'double',
      'num',
      'bool',
      'String',
      'void',
      '()',
      'Null',
      'Type',
      'Option',
    }.contains(name)) {
      return false;
    }
    if (name == 'dynamic' || name == 'Object') return true;
    final c = library[name];
    if (c != null && c.isEnum) return false;
    return c != null || const {'List', 'Map', 'Set', 'Iterable'}.contains(name);
  }

  /// Whether a type is a type parameter of the class or method being
  /// printed, or its nullable form.
  bool _ownsParameter(IrType? t) =>
      t != null &&
      t.arguments.isEmpty &&
      !t.isFunction &&
      (cls.typeParameters.contains(t.name) ||
          _methodTypeParams.contains(t.name));

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
    return 'Fn($args) -> ${_wrapped(type(t.returns!))}';
  }

  /// `List.generate(n, f)` and friends, which are Dart's list constructors
  /// wearing a static's clothes. Rust builds a `Vec` from an iterator.
  static const _listStatics = {'generate', 'filled', 'from', 'of'};

  /// A value spelled projected (`<T as DartNullable>::Or`, a read out of
  /// a `Vec<T?>` or a generic accessor) where an `Option` operation --
  /// `!`, `== null`, `?.`, `??`, `==` -- wants the `Option<T>` a body works
  /// with: the prelude's conversion first.
  /// A type the prelude's `FromDynamic` converts: what a `dynamic` holds
  /// as itself, the scalars, and collections of those.
  bool _dynamicRepresentable(IrType t) {
    if (t.isFunction) return false;
    if (const {
      'Object',
      'dynamic',
      'String',
      'int',
      'double',
      'bool',
    }.contains(t.name)) {
      return true;
    }
    return (t.name == 'List' || t.name == 'Map') &&
        t.arguments.isNotEmpty &&
        t.arguments.every(_dynamicRepresentable);
  }

  /// `.flatten()` after a map lookup whose value type is itself nullable.
  String _flattenedValue(IrExpr? map) {
    final t = map?.rustType;
    if (t == null || t.arguments.length != 2) return '';
    return t.arguments[1].nullable ? '.flatten()' : '';
  }

  /// A closure's return type, spelled where inference has nothing to go
  /// on: a body ending in `Ok(None)` -- a `Null`-returning closure handed
  /// to the prelude's `then` -- left `Option<_>` open (`_initKeyboard`,
  /// run477). Elsewhere `_`, as before.
  String _closureReturnSpelled(IrType returns) {
    if (returns.isFunction || returns.name == 'raw') return '_';
    if (returns.name == 'Null' || returns.nullable) return type(returns);
    return '_';
  }

  /// The captured cell locals that hold a `late` field (see `_cellLocals`).
  Set<String> _lateCellLocals = const {};

  /// A closure, possibly behind the wrappers coerce puts on one (a
  /// `Some`, an upcast, a clone).
  bool _closureLike(IrExpr e) => switch (e) {
    IrClosure() => true,
    IrUpcast(:final value) => _closureLike(value),
    IrSome(:final value) => _closureLike(value),
    IrCall(:final target, :final name, :final args) =>
      (name == 'clone' || name == '!rc') &&
          args.isEmpty &&
          target != null &&
          _closureLike(target),
    _ => false,
  };

  /// A type that cannot be spelled as a return (`_`, a placeholder, a
  /// method's own parameter nothing declares here).
  bool _mentionsUnknown(IrType t) {
    if (t.name == '_' || t.name == 'raw' || t.name.isEmpty) return true;
    if (t.isFunction) {
      return t.parameters!.any(_mentionsUnknown) ||
          (t.returns != null && _mentionsUnknown(t.returns!));
    }
    return t.arguments.any(_mentionsUnknown);
  }

  /// Whether the null-aware body being printed binds its value by value
  /// (a scalar receiver) rather than by reference.
  bool _boundByValue = false;

  String _nullAware(IrExpr receiver, IrExpr body, bool flatten) {
    final scalar = const {
      'int',
      'double',
      'num',
      'bool',
    }.contains(receiver.rustType?.name);
    final outer = _boundByValue;
    _boundByValue = scalar;
    try {
      final at = scalar ? '' : '.as_ref()';
      // The body's type spelled where it is known: an adapter closure
      // made in the body (`handler == null ? null : (m) async {..}` into
      // a `MessageHandler?` slot) unsizes against a spelled return and
      // not against an inferred `_` (ws486).
      // ..only for a closure body: the IR type of anything else is not
      // exact enough to spell as a return (`Infallible` for a body TFA
      // removed, `Option<()>` for a `void?`; +35 at ws487).
      final bodyType = body.rustType;
      final closureBody = _closureLike(body);
      if (Platform.environment['DART2RUST_TRACE_NULLAWARE'] == '1') {
        stderr.writeln(
          'TRACE_NULLAWARE body=${body.runtimeType} type=${body.rustType} closure=$closureBody',
        );
      }
      final spelled =
          closureBody &&
              bodyType != null &&
              bodyType.name != 'raw' &&
              !_mentionsUnknown(bodyType)
          ? type(bodyType)
          : '_';
      return _failure == null
          ? '${expr(_plain(receiver))}$at.${flatten ? 'and_then' : 'map'}(|$_boundName| ${expr(body)})'
          : '${expr(receiver)}$at.map(|$_boundName| -> Result<$spelled, $_error> { Ok(${expr(body)}) }).transpose()?${flatten ? '.flatten()' : ''}';
    } finally {
      _boundByValue = outer;
    }
  }

  IrExpr _plain(IrExpr e) {
    final t = e.rustType;
    if (t == null || !t.projected) return e;
    return IrNullableOf(e, t.name, toOption: true)
      ..rustType = IrType(t.name, nullable: true, arguments: t.arguments);
  }

  IrIfNull _plainIfNull(IrIfNull e) => e.left.rustType?.projected == true
      ? (IrIfNull(
          _plain(e.left),
          e.right,
          nullableResult: e.nullableResult,
          eager: e.eager,
        )..rustType = e.rustType)
      : e;

  /// The trait declaring `name` that `owner` implements at more than one
  /// instantiation (`IrClass.extraImpls`), or null: a plain call of such a
  /// method is ambiguous to rustc and is qualified by the class's own
  /// instantiation (`<Self as Tween<i64>>::begin(self)`).
  IrClass? _wideTraitFor(IrClass? owner, String name) {
    if (owner == null || owner.extraImpls.isEmpty) return null;
    for (final above in _abstractAncestors(owner)) {
      if (!owner.extraImpls.any((w) => w.name == above.name)) continue;
      final declares =
          above.methods.any((m) => m.name == name && !m.isStatic) ||
          above.abstractMethods.any((m) => m.name == name) ||
          above.fields.any((f) => f.name == name);
      if (declares) return above;
    }
    return null;
  }

  /// `<Self as Trait<args>>` for a trait this class implements more than
  /// once, `Trait` otherwise: what a qualified call on `self` has to say.
  String _implementedAs(String trait) {
    final base = library[trait];
    if (base == null || !cls.extraImpls.any((w) => w.name == trait)) {
      return trait;
    }
    final args = _baseArguments(base) ?? '';
    final self = _inSuperFn ? '__Self' : 'Self';
    return '<$self as $trait$args>';
  }

  /// One element of a list literal: the first with its upcast spelled, a
  /// bare closure behind an `Rc` where the list holds functions.
  String _listElement(int index, IrExpr e, IrType element) {
    final first = index == 0 ? _explicitUpcast(e) : e;
    return element.isFunction && first is IrClosure && !first.boxed
        ? 'std::rc::Rc::new(${expr(first)})'
        : expr(first);
  }

  /// An implicit upcast made explicit, through any `Some` around it: the
  /// first element of a `vec![..]` decides the `Vec`'s type.
  IrExpr _explicitUpcast(IrExpr e) => switch (e) {
    IrUpcast(:final value, :final type, :final handle, :final explicit)
        when !explicit =>
      IrUpcast(value, type, handle: handle, explicit: true)
        ..rustType = e.rustType,
    IrSome(:final value) => IrSome(
      _explicitUpcast(value),
    )..rustType = e.rustType,
    // A function item behind an `Rc` is not yet the `Rc<dyn Fn>` its slot
    // holds; spelled where nothing else will unsize it.
    IrFunctionRef() when e.rustType?.isFunction ?? false => IrLiteral(
      '(${expr(e)} as ${type(e.rustType!)})',
      e.rustType!,
    )..rustType = e.rustType,
    _ => e,
  };

  /// An operand under a borrow: its implicit upcast is spelled, since
  /// unsizing does not happen behind a `&` (`map.get(&Rc::new(key))` was
  /// a `&Rc<String>` where `&Rc<dyn Object>` was wanted, 19 at ws425).
  String _borrowed(IrExpr e) => expr(_explicitUpcast(e));

  /// `::<A, B>` for a call's type arguments; nothing when there are none.
  String _turbofish(List<IrType> typeArguments) =>
      typeArguments.isEmpty ? '' : '::<${typeArguments.map(type).join(', ')}>';

  String _staticCall(
    String? owner,
    String name,
    List<IrExpr> args, [
    List<IrType> typeArguments = const [],
  ]) {
    final fish = _turbofish(typeArguments);
    // `Future.value(v)` is a future that is already done, which Rust spells
    // `ready`. `Future.delayed` and `Future.wait` need a runtime to be delayed
    // or joined *by*, and there is none, so they say so.
    // ..the prelude has one now (`SCHEDULER`, `dart_spawn`): the other
    // constructors are its functions (2026-09-05, the runtime ruler's
    // first refusal after `main`: `Future<bool>(() async {..})`).
    if (owner == 'Future') {
      if (name == 'value' && args.length <= 1) {
        // The type spelled: `Future<void>.value()` alone left `T` to
        // inference (E0283, ws462).
        return 'future_value$fish(${args.isEmpty ? 'None' : expr(args.single)})';
      }
      if ((name == '' || name == 'new') && args.length == 1) {
        return 'future_new(${expr(args.single)})';
      }
      if (name == 'microtask' && args.length == 1) {
        return 'future_microtask(${expr(args.single)})';
      }
      if (name == 'sync' && args.length == 1) {
        return 'future_sync(${expr(args.single)})';
      }
      if (name == 'delayed' && (args.length == 1 || args.length == 2)) {
        return 'future_delayed(${expr(args[0])}, ${args.length == 2 ? expr(args[1]) : 'None'})';
      }
      if (name == 'error' && args.isNotEmpty) {
        return 'DartFuture::ready(Err(${expr(args[0])}))';
      }
      if (name == 'wait' && args.isNotEmpty) {
        return 'future_wait(${expr(args[0])})';
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
        // The generator returns `Result`: the collection does too, and the
        // `?` the name rule appends unwraps it (69 `?` on a `Vec`).
        return _resultModel
            ? '(0..${expr(args[0])}).map($f).collect::<Result<Vec<_>, $_error>>()'
            : '(0..${expr(args[0])}).map($f).collect::<Vec<_>>()';
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
      return '${snake(name)}$fish(${args.map(expr).join(', ')})';
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
      // `dart:async`'s `Completer` records where it was made, so a run
      // stuck on a future nobody completes can say whose (run466: "0
      // future(s) still pending").
      if (owner == 'Completer' && args.isEmpty) {
        return 'Completer::new_named("$_here")';
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
    if (_freeStatics(owner) && (name.isNotEmpty || library.isAbstract(owner))) {
      // A *factory* on an abstract class -- `Characters(s)` -- is the static
      // named `new` here, as the struct path names an unnamed constructor.
      final spelled = name.isEmpty ? 'new' : name;
      return '${_abstractStaticName(owner, spelled)}$fish(${args.map(expr).join(', ')})';
    }
    return '$owner::${_identifier(name)}$fish(${args.map(expr).join(', ')})';
  }

  String _superCall(
    String base,
    String name,
    List<IrExpr> args, {
    bool isSetter = false,
    List<IrType> baseArguments = const [],
    List<IrType> typeArguments = const [],
  }) {
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
      (m) =>
          m.operator == null &&
          m.name == name &&
          !m.isStatic &&
          m.isSetter == isSetter,
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
    if (!_superFnEmits(baseClass, name, isSetter: isSetter)) {
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
    // The receiver as the super function takes it, `&__Self`: `self` is
    // already a reference -- one more deref when it is the handle
    // (`_receiverOf`) -- and a closure's `__me` is a handle when the class
    // is counted, a value otherwise (510 `Rc<X>: Trait` bounds at ws294).
    final receiver = _selfName == 'self'
        ? (_selfIsHandle ? '&**self' : 'self')
        : _selfName == 'this_'
        ? 'this_'
        : cls.counted
        ? '&*$_selfName'
        : '&$_selfName';
    // The base's type arguments spelled: a class implementing the trait
    // at two instantiations (`Animation<f64>` and the wider `Animation<
    // Option<f64>>`) left `T` ambiguous (E0283, 3 at ws451). The method's
    // own stay inferred.
    final method = baseClass.methods.firstWhere(
      (m) => m.name == name && !m.isStatic && m.isSetter == isSetter,
    );
    final own = typeArguments.length == method.typeParameters.length
        ? typeArguments.map(type).toList()
        : List.filled(method.typeParameters.length, '_');
    final turbofish = baseArguments.isEmpty && own.every((a) => a == '_')
        ? ''
        : '::<_${[...baseArguments.map(type), ...own].map((a) => ', $a').join()}>';
    final call =
        '${superFn(base, name, isSetter: isSetter)}$turbofish(${[receiver, ...args.map(expr)].join(', ')})';
    // An async super function is an `async fn`; the caller's trait wants
    // the boxed future every `Future<T>` is here.
    final isAsync = baseClass.methods.any(
      (m) => m.name == name && !m.isStatic && m.isAsync,
    );
    return call;
  }

  /// Whether `base`'s free function for [name] can actually be emitted.
  ///
  /// `_superFailed` answers this for the class being emitted, but a super call
  /// is made from the *subclass*, whose backend never sees the base's set.
  static final _superFnProbes = <String, bool>{};

  bool _superFnEmits(IrClass baseClass, String name, {bool isSetter = false}) {
    // Only an abstract class writes them. `_emitSuperFns` is called from
    // `_emitTrait` and nowhere else, because the free function is generic over
    // the trait -- there is nothing to make it generic over when the base is a
    // struct, since flattening copies the base's fields into each subclass
    // rather than leaving them anywhere shared. Probing without asking this
    // first said yes and the call named a function nobody wrote; the mixin
    // fixture is what walked into it.
    if (!baseClass.isAbstract) return false;
    final key = '${baseClass.name}.${isSetter ? 'set:' : ''}$name';
    final known = _superFnProbes[key];
    if (known != null) return known;
    final method = baseClass.methods.firstWhere(
      (m) =>
          m.operator == null &&
          m.name == name &&
          !m.isStatic &&
          m.isSetter == isSetter,
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

  /// `this` as an owned handle, from wherever the body is: a trait body's
  /// `dart_self_<trait>()`, a counted class's stored handle, else a clone.
  String _selfHandle() => _fieldsAreAccessors
      ? '$_selfName.dart_self_${snakeRaw(cls.name)}()'
      : cls.counted
      ? '$_selfName.dart_self_ref().get()'
      : '$_selfName.clone()';

  /// In a trait body, the trait an accessor is reached through when more
  /// than one trait in the chain declares it (`textTheme` on
  /// `CupertinoThemeData` over `NoDefaultCupertinoThemeData`; 13 E0034 at
  /// ws308): this trait when it declares the name, which is the override
  /// Dart would dispatch to, else the nearest abstract supertype that does.
  String? _accessorQualifier(String name, {String kind = 'read'}) {
    // What each kind of accessor a trait declares (`_emitTrait`): a read
    // for any field or getter, a write for a mutable field or a setter, a
    // cell for a held collection. Naming a trait that lacks the item was
    // "expected a type, found a trait" (22 at ws309).
    bool field(IrClass c) => c.fields.any(
      (f) =>
          f.name == name &&
          switch (kind) {
            'write' => !f.isFinal,
            'cell' => _handsCell(f),
            _ => true,
          },
    );
    bool method(IrClass c) =>
        c.methods.any(
          (m) =>
              m.name == name && !m.isStatic && m.isSetter == (kind == 'write'),
        ) ||
        c.abstractMethods.any(
          (m) => m.name == name && m.isSetter == (kind == 'write'),
        );
    if (kind == 'cell' && !field(cls) && !_supertypesOf(cls).any(field)) {
      return null;
    }
    final chain = [
      cls,
      ..._supertypesOf(cls).where((t) => library.isAbstract(t.name)),
    ];
    final declaring = chain.where((c) => field(c) || method(c)).toList();
    if (declaring.length < 2) return null;
    // A getter override is the nearest trait's own; a field's accessor is
    // declared once, by the topmost trait holding the field (`_emitTrait`
    // leaves it to the ancestor), which no other declarer is above.
    if (method(cls)) return cls.name;
    final fields = declaring.where(field).toList();
    if (fields.isEmpty) return declaring.first.name;
    return fields
        .firstWhere(
          (c) => !fields.any((o) => o != c && _supertypesOf(c).contains(o)),
          orElse: () => fields.last,
        )
        .name;
  }

  /// The trait, `from` or one above it, that declares the Rust item
  /// `rustName` (a method, a setter, a field's accessor); null when none
  /// of them does.
  String? _declaringTrait(String from, String rustName) {
    final start = library[from];
    if (start == null) return null;
    bool declares(IrClass c) =>
        c.methods.any((m) => !m.isStatic && _methodName(m) == rustName) ||
        c.abstractMethods.any((m) => _methodName(m) == rustName) ||
        c.fields.any(
          (f) =>
              snake(f.name) == rustName || 'set_${snake(f.name)}' == rustName,
        );
    for (final c in [start, ..._abstractAncestors(start)]) {
      if (declares(c)) return c.name;
    }
    return null;
  }

  /// `this` as the handle the object already has -- the trait's own in a
  /// trait body, the counted struct's otherwise -- or null when the class
  /// has none (a plain value struct).
  String? _thisHandle() {
    if (_fieldsAreAccessors || _selfName == 'this_') {
      return '$_selfName.dart_self_${snakeRaw(cls.name)}()';
    }
    if (cls.counted) return '$_selfName.dart_self_ref().get()';
    return null;
  }

  /// A value shared as a handle: `this` by its own handle (`self.clone()`
  /// was a struct where `Rc<dyn RendererBinding>` went, `_manifold`'s
  /// lazy initializer, run459), anything else as spelled.
  String _handleOf(IrExpr value) {
    // A clone of `this` (the front end's) is `this`.
    final bare =
        value is IrCall &&
            value.name == 'clone' &&
            value.args.isEmpty &&
            (value.target == null || value.target is IrThis)
        ? IrThis()
        : value;
    return bare is IrThis ? (_thisHandle() ?? expr(value)) : expr(value);
  }

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
      final through =
          _accessorQualifier(name) ?? _wideTraitFor(cls, name)?.name;
      return through == null
          ? '$receiver.${snake(name)}()$_propagate'
          : '${_implementedAs(through)}::${snake(name)}($receiver)$_propagate';
    }
    // A shared field is read through its cell. `get` copies, which is what a
    // Dart read does; `borrow().clone()` is the same for a value that is not
    // `Copy`.
    if (target == null || target is IrThis) {
      final shared = _sharedField(name);
      if (shared != null) {
        final lazy = _lazyDecl(name);
        if (lazy != null) return _lazyRead(lazy, receiver);
        final held = _heldType(shared);
        // The guard bound and dropped in its own statement (as another
        // object's field is read below): a bare `.borrow().clone()` keeps
        // its `Ref` to the statement's end, into a `borrow_mut()` of the
        // same cell on the left (`_file = _file.setPosition(0)`, run517).
        // Parenthesised: a block at a statement's start is a statement.
        final read = _isCopy(held)
            ? '$receiver.${snake(name)}.get()'
            : '({ let __r = $receiver.${snake(name)}.borrow().clone(); __r })';
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
      if (cell != null && _lazyDecl(name) != null) {
        return _lazyRead(_lazyDecl(name)!, receiver);
      }
      if (cell != null) {
        // The `borrow()` guard is a temporary, and a temporary in a block's
        // tail expression outlives the block's locals: `Ok(data.next_sibling
        // .borrow().clone())` on a local `data` was "does not live long
        // enough" 17 times (ws376). Bound and handed out, the guard dies
        // in its own statement.
        final read = _fieldIsCopy(cell, owner == null ? null : library[owner])
            ? '$receiver.${snake(name)}.get()'
            : '{ let __r = $receiver.${snake(name)}.borrow().clone(); __r }';
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
    // A field of a *trait object*: the accessor the trait declares, since
    // a `dyn` has no fields (`childParentData.nextSibling` on an `Rc<dyn
    // StackParentData>`, the mixin's field, ws523).
    final held = target?.rustType;
    if (held != null && !isNullable(held) && library.isAbstract(held.name)) {
      final owned = library[held.name];
      if (owned != null && _allFields(owned).any((f) => f.name == name)) {
        return '$receiver.${snake(name)}()$_propagate';
      }
    }
    // Any other object's field: cloned out, as a field of `self` or of a
    // local is (`..get().child` handed to `updateChild` moved out of the
    // handle, E0507, run459).
    return '$receiver.${snake(name)}.clone()';
  }

  /// `x is Foo`.
  ///
  /// Rust answers it with `Any`, which downcasts to a *concrete* type: the
  /// trait object says what it holds, and holding is always a struct. So a
  /// target that is itself abstract has no answer here -- `x is RenderBox`
  /// asks whether the thing implements a trait, which `Any` cannot say -- and
  /// is still refused, now under a name that says which half is missing.
  /// See `IrSuperDispatch`. The super function's generics are `<__Self,
  /// class parameters.., method parameters..>`: the first two kinds are
  /// inferred from the receiver, the method's own are spelled.
  String _superDispatch(
    IrExpr receiver,
    String base,
    String name,
    List<IrExpr> args,
    List<IrType> typeArguments,
    int classArity,
    String? castTo,
  ) {
    // A generic trait cast to with its arguments inferred (`dyn
    // CanonicalizedMap<_, _, _>`; E0107 on the bare name, run459).
    final castArity = library[castTo ?? '']?.typeParameters.length ?? 0;
    final castSpelled = castArity == 0
        ? castTo
        : '$castTo<${List.filled(castArity, '_').join(', ')}>';
    final on = castTo == null
        ? expr(receiver)
        : '${expr(receiver)}.dart_cast_to::<dyn $castSpelled>().unwrap()';
    final generics = [
      '_',
      for (var i = 0; i < classArity; i++) '_',
      ...typeArguments.map(type),
    ];
    // An async base method's super function is its future, not a
    // `Result` (`invokeMethod` reaching `_invokeMethod<T>`, ws482).
    final baseMethod = library[base]?.methods
        .where((m) => m.name == name && !m.isStatic)
        .firstOrNull;
    final suffix = (baseMethod?.isAsync ?? false) ? '' : _propagate;
    return '${superFn(base, name)}::<${generics.join(', ')}>'
        '(${['&*$on', ...args.map(expr)].join(', ')})$suffix';
  }

  /// `dyn Foo<A, B>`: the trait object a trait-typed `IrType` names.
  String _dynOf(IrType t) => t.arguments.isEmpty
      ? 'dyn ${t.name}'
      : 'dyn ${t.name}<${t.arguments.map(type).join(', ')}>';

  String _isTest(IrExpr operand, IrType target, bool negated) {
    final name = target.name;
    // A type parameter: whatever the caller passed for it, asked by id
    // (`dart_cast_any`). `ancestor.state is T` in `findAncestorStateOfType`,
    // refused as "`is` against `T`" since the first round.
    if (_isTypeParam(name)) {
      return '${expr(operand)}.dart_cast_any::<$name>()'
          '.${negated ? "is_none" : "is_some"}()';
    }
    // A trait: asked of the object itself (`dart_cast`), which knows what
    // it implements. Refused since the first round (`_isTest`).
    if (library.isAbstract(name)) {
      return '${expr(operand)}.dart_cast_to::<${_dynOf(target)}>()'
          '.${negated ? "is_none" : "is_some"}()';
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
    // `is Object` holds of every value but null, `is Object?` of every
    // value: the pattern `final Object? value` a switch's last case
    // binds is lowered to one (`_updateUserSettingsData`, run472).
    if (name == 'Object' || name == 'dynamic') {
      final always = target.nullable || operand.rustType?.nullable != true;
      if (always) return negated ? 'false' : 'true';
      return '${expr(operand)}.${negated ? "is_none" : "is_some"}()';
    }
    if (scalars.containsKey(name)) {
      final tests = scalars[name]!
          .map((t) => '${_asAny(operand)}.downcast_ref::<$t>().is_some()')
          .join(' || ');
      return negated ? '!($tests)' : '($tests)';
    }
    // `x is Map` / `is List` / `is Set` / `is Iterable` on a `dynamic`: the
    // prelude's collections are generic structs, and `Any` cannot ask for
    // "some `Map<_, _>`"; their runtime type names can (`dart_is_kind`,
    // get's `_isNullOrEmpty`, run489).
    const collections = {
      'Map': ['Map'],
      'List': ['Vec'],
      'Set': ['Set'],
      'Queue': ['VecDeque', 'Queue'],
      'Iterable': ['Vec', 'Set', 'VecDeque', 'Queue'],
    };
    final kinds = collections[name];
    if (kinds != null && library[name] == null) {
      final test =
          'dart_is_kind(&${expr(operand)}, &[${kinds.map((k) => '"$k"').join(', ')}])';
      return negated ? '!$test' : test;
    }
    // `x is Uint8List`: the typed lists are `Vec`s of their element here
    // (the front end's `_narrowElement`), asked of `Any` exactly
    // (`StandardMessageCodec.writeValue`, run504).
    const typedData = {
      'Float32List': 'Vec<f32>',
      'Float64List': 'Vec<f64>',
      'Int8List': 'Vec<i8>',
      'Int16List': 'Vec<i16>',
      'Int32List': 'Vec<i32>',
      'Int64List': 'Vec<i64>',
      'Uint8List': 'Vec<u8>',
      'Uint8ClampedList': 'Vec<u8>',
      'Uint16List': 'Vec<u16>',
      'Uint32List': 'Vec<u32>',
      'Uint64List': 'Vec<u64>',
    };
    final typed = typedData[name];
    if (typed != null && library[name] == null) {
      return '${_asAny(operand)}.downcast_ref::<$typed>()'
          '.${negated ? "is_none" : "is_some"}()';
    }
    // A prelude class answers `is` through `Any` like a translated one:
    // every `'static` type is an `Object` there (`is StateError` in
    // `BindingBase._initListenable`, run433).
    if (library[name] == null && !_preludeClasses.contains(name)) {
      throw Unsupported('`is` against `$name`, which was not translated', name);
    }
    final arguments = target.arguments.isEmpty
        ? ''
        : '<${target.arguments.map(type).join(', ')}>';
    return '${_asAny(operand)}'
        '.downcast_ref::<$name$arguments>().${negated ? "is_none" : "is_some"}()';
  }

  /// Whether a value of this recorded type is a handle: an `Rc<dyn Trait>`,
  /// a `dynamic`, a counted class's `Rc<Struct>`.
  /// Not a `dynamic`: the blanket `as_any` looks through an `Rc<dyn
  /// Object>` itself, and a call on a `dynamic` the type flow analysis
  /// narrowed (`x.isNegative` on an `f64`) is recorded `dynamic` while its
  /// value is a plain `bool` (intl's `_floor`, ws522).
  bool _handleLike(IrExpr e) {
    final t = e.rustType;
    if (t == null || e is IrThis || t.isFunction || isNullable(t)) return false;
    return library.isAbstract(t.name) || (library[t.name]?.counted ?? false);
  }

  /// `x.as_any()` for a downcast or an `is`: through the handle when `x` is
  /// one. The blanket `Object` on the `Rc` itself answers with the
  /// *handle's* `Any` -- an `Rc<dyn Widget>`, never a `RootWidget` -- so
  /// `widget is RootWidget` was always false and `RootElement.mount`
  /// unwrapped a `None` (run521). `this` and a value are asked directly.
  String _asAny(IrExpr e) =>
      _handleLike(e) ? '${expr(e)}.as_ref().as_any()' : '${expr(e)}.as_any()';

  /// Whether `name` is a field of this struct's own, not in a cell, whose
  /// Rust type is one of the prelude's collections by value.
  bool _ownCollectionField(String name) {
    if (_sharedField(name) != null) return false;
    final decl = cls.fields.where((f) => f.name == name).firstOrNull;
    if (decl == null || decl.type.nullable) return false;
    // By the IR's name: a typed list (`Uint8List`) is a prelude alias of
    // its `Vec` and spells as one.
    return const {
      'List',
      'Vec',
      'Iterable',
      'Map',
      'Set',
      'Queue',
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
    }.contains(decl.type.name);
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

  /// The type parameters of the method being emitted: a name among them
  /// is a Rust type parameter, not a class (`_isTest`, `IrCastTo`).
  var _methodTypeParams = const <String>[];

  bool _isTypeParam(String name) =>
      _methodTypeParams.contains(name) || cls.typeParameters.contains(name);

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
      // Registered for `dart_cast_to` through `dyn Object` on the way.
      // ..with the cast spelled: an `if` arm has no expected type of its
      // own where the `let` is unannotated, and two arms of different
      // classes did not unify (`ThemeData`'s `splashFactory`, ws523).
      final spelled = isNullable(declared) ? null : type(declared);
      return spelled == null
          ? 'dart_object($text)'
          : '(dart_object($text) as $spelled)';
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

  /// The member whose body is being printed, `Class.member`, for runtime
  /// diagnostics that name their creator (`Completer::new_named`).
  String _here = '';

  /// Whether `self` is held by value (an `std::ops` operator's body).
  var _selfByValue = false;

  /// Rust names of the collection methods that change their receiver.
  static const _inPlace = {
    'push',
    'insert',
    'remove',
    '!map_remove',
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
    // `ByteData`'s setters: a byte view written in place (`WriteBuffer.
    // putUint16` on its `_eightBytes`, run509).
    'set_int8',
    'set_uint8',
    'set_int16',
    'set_uint16',
    'set_int32',
    'set_uint32',
    'set_int64',
    'set_uint64',
    'set_float32',
    'set_float64',
  };

  static bool _mutatesInPlace(String name) => _inPlace.contains(name);

  /// The cell a field read would go through, as a place -- `self.x` or
  /// `other.x` -- when the field is kept in a `RefCell`; null otherwise.
  String? _cellPlace(IrExpr? target) {
    if (target is! IrField) return null;
    final base = target.target;
    final atThis = base == null || base is IrThis;
    // Through the trait's cell accessor: inside a trait body, or on a
    // handle whose owner is a trait (`cascaded.children.add(x)`).
    final owner = target.owner;
    if ((atThis && _fieldsAreAccessors) ||
        (!atThis && owner != null && library.isAbstract(owner))) {
      final owned = atThis ? cls : library[owner!];
      final decl = owned == null
          ? null
          : _allFields(owned).where((f) => f.name == target.name).firstOrNull;
      if (decl == null || !_handsCell(decl)) return null;
      final holder = atThis ? _selfName : expr(base);
      final through = atThis
          ? _accessorQualifier(target.name, kind: 'cell')
          : null;
      return through == null
          ? '$holder.${snake(target.name)}_cell()$_propagate'
          : '$through::${snake(target.name)}_cell($holder)$_propagate';
    }
    final IrFieldDecl? cell;
    if (base == null || base is IrThis) {
      cell = _sharedField(target.name);
    } else if (target.owner != null) {
      cell = _cellFieldOf(target.owner!, target.name);
    } else {
      cell = null;
    }
    if (cell == null ||
        _fieldIsCopy(
          cell,
          base == null || base is IrThis
              ? cls
              : (target.owner == null ? null : library[target.owner!]),
        )) {
      return null;
    }
    // Only a collection is mutated through the cell: `reverse` on an
    // `Rc<RefCell<Option<Rc<AnimationController>>>>` is the controller's
    // method, not `Vec::reverse` (51 in `widgets`).
    // ..the same set every other in-place site uses: a typed list is an
    // alias of its `Vec` and spelled by name, and `WriteBuffer._add`'s
    // `_buffer[i] = b` went into a clone -- every platform message was
    // 35 zero bytes (run512).
    final held = _heldType(cell);
    if (!_isMutableCollection(held)) return null;
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
    // A receiver is not a coercion site: an implicit upcast under it, even
    // through a `Some`, is spelled (`Some(dart_object(EdgeInsets {..}))
    // .clone()` into an `Option<Rc<dyn EdgeInsetsGeometry>>`, 26 at ws426).
    return expr(_explicitUpcast(target));
  }

  /// Whether a value of this class is held as an `Rc`: a counted class, or
  /// an abstract one (`Rc<dyn ..>`).
  bool _isHandle(String? className) {
    final c = library[className];
    return c != null && (c.counted || c.isAbstract);
  }

  /// The prelude's methods that take a callback and so return `Result`
  /// themselves (see `DartError` there).
  static const _preludeFailing = {
    // `convert` on every prelude converter: a `Converter` runs a Dart
    // closure, and the two fixed ones (`JsonUtf8Encoder`, `Utf8Decoder`)
    // return `Result` to match (`JSONMessageCodec.decodeMessage`, ws506).
    'convert',
    'put_if_absent',
    'for_each',
    'sort_by_dart',
    'first_where',
    'first_where_or',
    // Not `then`: the prelude's returns the future it spawns, and the
    // callback's own failure lands in that future (`_initKeyboard`, run476).
    'run',
    'run_guarded',
    'run_unary_guarded',
    'run_unary',
  };

  /// ..and its static functions.
  static const _preludeFailingStatics = {'generate', '_invoke1_with_return'};

  /// `?` when a function surrounds the expression, `.unwrap()` otherwise.
  String get _propagate => _failure != null ? '?' : '.unwrap()';

  /// Set while the operand of an `await` is printed: the call's own `?`
  /// belongs after the `.await`.
  bool _awaiting = false;

  String _call(
    IrExpr? target,
    String name,
    List<IrExpr> args, {
    String? qualifier,
    String? receiverClass,
    bool fails = false,
    List<IrType> typeArguments = const [],
    bool asyncFn = false,
    bool asyncTarget = false,
    IrType? resultType,
  }) {
    final turbofish = typeArguments.isEmpty
        ? ''
        : '::<${typeArguments.map(type).join(', ')}>';
    // Read before the receiver and arguments print: a call inside them
    // would otherwise take the `await`'s flag.
    final awaited = _awaiting;
    _awaiting = false;
    // Before the receiver is rendered: rendering a chain on its own is
    // refused, and this is the one place a chain is not on its own.
    // Any arguments, not none: Dart's `toList({bool growable = true})` has a
    // named parameter, and the Kernel front end fills in its default -- so the
    // chain was collected on one side and refused on the other.
    if (name == 'to_list' && target is IrIterChain) {
      // `where(..).toList()`: `filter` keeps references, and the list
      // wants the items (`Vec<&Rc<dyn FocusNode>>`, 8 at ws467).
      final cloned = target.steps.isNotEmpty && target.steps.last.$1 == 'filter'
          ? '.cloned()'
          : '';
      return '${_chain(target)}$cloned.collect::<Vec<_>>()';
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
      // ..the object's own handle, now that it keeps one (`DartSelf`).
      return '${expr(target)}.dart_self_ref().get()';
    }
    // `runtimeType` on a super function's `this_` (see `DartAny`).
    // ..and on a struct method's `self` too: through `&mut self`, `Object::
    // runtime_type` resolved on the *reference*, which the blanket impl
    // asks to be `'static` (E0521, `WriteBuffer.done`, run505).
    if ((name == 'runtimeType' || name == 'runtime_type') &&
        args.isEmpty &&
        (target == null || target is IrThis) &&
        (_selfName == 'this_' || _selfName == 'self')) {
      return '$_selfName.dart_runtime_type()';
    }
    final cellPlace = _mutatesInPlace(name) ? _cellPlace(target) : null;
    // A mutating call on a field of `this` in a struct's own method acts
    // on the field, not on the clone a value read takes: `_buffer.setRange
    // (..)` on a clone left `WriteBuffer` empty and every platform message
    // without a byte (run507).
    // ..a field of this struct's own, held as a plain collection: a cell
    // (`Rc<RefCell<..>>`) has its place above, and a handle's method that
    // shares a mutator's name (`AnimationController.reverse`) is not a
    // mutation of the field (+44 at ws509).
    final ownPlace =
        cellPlace == null &&
            _mutatesInPlace(name) &&
            target is IrField &&
            (target.target == null || target.target is IrThis) &&
            !_fieldsAreAccessors &&
            _selfName == 'self' &&
            _ownCollectionField(target.name)
        ? '$_selfName.${snake(target.name)}'
        : null;
    final receiver = cellPlace != null
        ? '$cellPlace.borrow_mut()'
        : ownPlace != null
        ? ownPlace
        : target is IrLiteral && target.type.name == 'double'
        ? '(${_receiver(target)}_f64)'
        : _receiver(target);
    // `HashMap` looks up by reference, and gives back a reference to the
    // value. Dart's `m[k]` is a `V?`, so the borrow is cloned away rather
    // than leaked into every caller's type.
    // A value shared into a trait object (see `_widened`).
    // `this` shared: the object's own handle, not a fresh `Rc` around a
    // reference (`Rc::new(this_)` wanted `'static`, 168 lifetime errors)
    // or a copy (a new identity).
    if (name == '!rc' && args.isEmpty && target is IrThis) {
      final own = _thisHandle();
      if (own != null) return own;
    }
    if (name == '!rc' && args.isEmpty) return 'std::rc::Rc::new($receiver)';
    // An `Option<Rc<dyn Object>>` into a `dynamic` slot: absent is `Null`.
    if (name == '!or_null' && args.isEmpty) {
      return '$receiver.unwrap_or_else(|| std::rc::Rc::new(Null) as std::rc::Rc<dyn Object>)';
    }
    // The other way: a `dynamic` as an `Option`, `None` for the `Null` object.
    if (name == '!nullable' && args.isEmpty) return 'dart_nullable($receiver)';
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
    // ..by the parameter's own conversion, not `Any`'s one concrete
    // type: a `T` instantiated with `Rc<dyn Object>` (`invokeMethod<
    // dynamic>`) is no object's type, and `decodeEnvelope(..) as T?` gave
    // null for every reply -- `MissingPlatformDirectoryException` at
    // run513. Dart's null (the `Null` object) is `None` first.
    if (name == '!as_opt' && args.length == 1) {
      final spelledArgs = typeArguments.isEmpty
          ? ''
          : '<${typeArguments.map(type).join(', ')}>';
      final asked =
          '<${expr(args.single)}$spelledArgs as FromDynamic>::from_dynamic';
      // ..on an `Option` already (a `dynamic?`): through it.
      final targetIr = target?.rustType;
      if (targetIr != null && isNullable(targetIr)) {
        return '$receiver.and_then(|__v| dart_nullable(__v)).as_ref().and_then(|__v| $asked(__v))';
      }
      return 'dart_nullable($receiver).as_ref().and_then(|__v| $asked(__v))';
    }
    if (name == '!as_object' && args.isEmpty) {
      // `this` into an `Object` slot: the handle when the method holds
      // one, a fresh `Rc` of a clone when it does not.
      if (target is IrThis) {
        // ..and the object's own handle where it has one (`_selfHandle`):
        // `Rc::new(this_.clone())` boxed a reference (the last 20 lifetime
        // errors at ws335).
        if (_fieldsAreAccessors || _selfName == 'this_') {
          return '($_selfName.dart_self_${snakeRaw(cls.name)}() as std::rc::Rc<dyn Object>)';
        }
        if (cls.counted) {
          return '($_selfName.dart_self_ref().get() as std::rc::Rc<dyn Object>)';
        }
        return _selfIsHandle
            ? '($_selfName.clone() as std::rc::Rc<dyn Object>)'
            : '(std::rc::Rc::new($_selfName.clone()) as std::rc::Rc<dyn Object>)';
      }
      return '($receiver as std::rc::Rc<dyn Object>)';
    }
    if (name == '!rc_object' && args.isEmpty) {
      // `this` shared as an `Object` is behind `&self`: a handle is cloned,
      // a value is cloned into a fresh one -- `Rc::new(self)` was a handle
      // to a borrow, and "lifetime may not live long enough" 329 times.
      // `this_` in a super function is `&__Self: ?Sized` and stays.
      // ..and now that every object keeps its own handle (`DartSelf`): a
      // trait body's `this` is `dart_self_<trait>()`, a counted class's is
      // `dart_self_ref().get()` -- not a copy with a new identity (8703
      // `Rc::new(self.clone())`, 82 `Rc::new(this_)` at ws292).
      if (target == null || target is IrThis) {
        if (_fieldsAreAccessors) {
          return '($_selfName.dart_self_${snakeRaw(cls.name)}() as std::rc::Rc<dyn Object>)';
        }
        if (cls.counted) {
          return '($_selfName.dart_self_ref().get() as std::rc::Rc<dyn Object>)';
        }
        if (_selfName == 'self') {
          return _selfIsHandle
              ? '(self.clone() as std::rc::Rc<dyn Object>)'
              : '(std::rc::Rc::new(self.clone()) as std::rc::Rc<dyn Object>)';
        }
      }
      return '(std::rc::Rc::new($receiver) as std::rc::Rc<dyn Object>)';
    }
    if (name == '!dart_eq' && args.length == 1) {
      return '$receiver.dart_eq(&${_borrowed(args.single)})';
    }
    // `Vec::contains` takes a reference; Dart's takes the value. Only the
    // List's: `Path.contains(Offset)` is a method of its own.
    if (name == '!contains' && args.length == 1) {
      return '$receiver.dart_contains(&${_borrowed(args.single)})';
    }
    if (name == '!expando_get' && args.length == 1) {
      return '$receiver.get(&${_borrowed(args.single)})';
    }
    // `expando[object] = v`: keyed by identity, so the object's handle
    // (`this` by its own; `PlatformInterface`'s token registry, run482).
    if (name == '!expando_set' && args.length == 2) {
      return '$receiver.set(${_handleOf(args[0])}, ${expr(args[1])})';
    }
    // `m[k]` is a `V?`, and Dart's `V?` of a nullable `V` is `V` itself:
    // `data['platformBrightness']` on a `Map<String, Object?>` is an
    // `Object?`, not an `Option<Option<..>>` (`_updateUserSettingsData`,
    // ws472).
    if (name == '!map_get' && args.length == 1) {
      return '$receiver.get(&${_borrowed(args.single)}).cloned()${_flattenedValue(target)}';
    }
    // `_views[_implicitViewId]` with an `int?` key: Dart looks up `null`
    // and finds nothing; here the absent key is the absent value.
    // The map is built outside the closure: an element that can fail (a
    // `?` in a literal's constructor) has no `Result` to leave through
    // inside an `and_then` returning `Option` (30 E0277 at ws441).
    if (name == '!map_get_opt' && args.length == 1) {
      return '{ let __m = $receiver; ${expr(args.single)}.as_ref().and_then(|__k| __m.get(__k).cloned()${_flattenedValue(target)}) }';
    }
    if (name == '!map_remove' && args.length == 1) {
      return '$receiver.remove(&${_borrowed(args.single)})';
    }
    if (name == 'contains_key' && args.length == 1) {
      return '$receiver.$name(&${_borrowed(args.single)})';
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
    // `first` is an index on a list and a method on a translated class
    // with a getter of that name (`PriorityQueue.first`, E0608 at ws460).
    if (name == 'first' &&
        args.isEmpty &&
        library[receiverClass ?? ''] == null) {
      return '$receiver[0].clone()';
    }
    // Cloned out, as `first` is: an element used by value moved out of
    // the `Vec` (`_requestTabTraversalFocus(sortedNodes.last)`, ws522).
    if (name == 'last' && args.isEmpty) {
      return '$receiver[$receiver.len() - 1].clone()';
    }
    // Dart's `toList` on a list copies it, which is `clone`.
    // `toList()` on a list is the list again; on any other collection --
    // a `Set`, whose static owner is `Iterable` (ws496) -- the prelude's
    // `to_list()`. Its `growable` is dropped either way.
    if (name == 'to_list') {
      // ..by what the receiver's type spells: an `Iterable` -- a map's
      // `values`, a `reversed` -- is a `Vec` here too (ws497).
      final held = target?.rustType;
      final spelled = held == null ? null : type(held);
      return spelled == null || spelled.startsWith('Vec<')
          ? '$receiver.clone()'
          : '$receiver.to_list()';
    }
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
    // A translated callee returns `Result`: `?` inside a function, and
    // `.unwrap()` where there is none around (a static's initialiser).
    // An awaited call is not `?`ed here but at the `.await`.
    final failing = fails || (_resultModel && _preludeFailing.contains(name));
    // A call reaching an `async fn` *inherently* is its `DartFuture`, no
    // `?`; one reaching it through a trait (`qualifier`, `asTrait` below)
    // gets the trait's `Result<DartFuture<T>, E>` and is unwrapped first,
    // awaited or not (`OptionalMethodChannel.invokeMethod<T>` through its
    // trait, ws432). The front end's `asyncFn` is a guess at the path the
    // backend decides here.
    // ..and through the trait it is a `Result` whether or not the method
    // itself fails: the trait's declaration wraps every async method.
    String suffixFor(bool viaTrait) =>
        (failing || asyncTarget) && !(asyncFn && !viaTrait) ? _propagate : '';
    final boxed = false;
    // `_identifier`, not `snake`: an *operator* called as a method -- `~x` is
    // `x.~()` in Kernel -- has no letters for `snake` to keep, and it came out
    // as `x._()`, which does not parse and stopped the whole crate at the
    // lexer. `_identifier` gives the operator the same name its definition
    // got, and refuses the ones with no Rust name at all.
    // A concrete class's own method is inherent, and an inherent method
    // wins over any trait's: the plain call is unambiguous, and the
    // qualified one passed `&**self` to a `self: &Rc<Self>` receiver (47
    // in `WidgetsFlutterBinding` alone).
    if (qualifier != null && !(library[qualifier]?.isAbstract ?? true)) {
      qualifier = null;
    }
    // A method of a trait the receiver's class implements more than once
    // (`IrClass.extraImpls`): the call names the class's own instantiation.
    final owner = target == null || target is IrThis
        ? cls
        : receiverClass == null
        ? null
        : library[receiverClass];
    var wide = _wideTraitFor(owner, name);
    // ..or one of *two* traits the receiver's class implements that both
    // declare the method (`RenderBox` re-declares `RenderObject`'s
    // `markNeedsLayout`), with no inherent method to win: the nearest is
    // named (21 E0034 at ws416).
    if (wide == null &&
        owner != null &&
        qualifier == null &&
        !owner.methods.any((m) => m.name == name && !m.isStatic)) {
      // A getter or a method, not a setter of the same Dart name (`value`
      // and `value=`: the setter is `set_value` here, and naming a trait
      // that has only the setter was 49 "cannot find method", ws418).
      bool declares(IrMethod m) => m.name == name && !m.isStatic && !m.isSetter;
      final declaring = _abstractAncestors(owner).where(
        (a) =>
            a.methods.any(declares) ||
            a.abstractMethods.any(declares) ||
            a.fields.any((f) => f.name == name),
      );
      if (declaring.length > 1) wide = declaring.first;
    }
    String? asTrait;
    if (wide != null &&
        owner != null &&
        (qualifier == null || qualifier == wide.name)) {
      final passed = _argumentsThrough(owner, const {}, wide, {});
      if (passed != null &&
          (identical(owner, cls) || owner.typeParameters.isEmpty)) {
        final spelledArgs = passed.isEmpty
            ? ''
            : '<${passed.map((a) => type(a)).join(', ')}>';
        final self = identical(owner, cls)
            ? (_inSuperFn ? '__Self' : 'Self')
            : owner.name;
        asTrait = '<$self as ${wide.name}$spelledArgs>';
        qualifier = wide.name;
      }
    }
    // On a closure's handle a name two traits declare is ambiguous where
    // `self.name()` was not (`__me.child()` on a struct implementing
    // both `RenderObjectWithChildMixin` and `RenderProxyBox`, E0034 at
    // ws462): the declaring trait, as an accessor read chooses it.
    if (qualifier == null &&
        _selfName == _countedSelf &&
        (target == null || target is IrThis)) {
      final chosen = _accessorQualifier(name);
      if (chosen != null && library.isAbstract(chosen)) qualifier = chosen;
    }
    if (qualifier != null) {
      // See `IrCall.qualifier`. `self`/`this_` are already references; a
      // closure's `__me` is a handle, as is any receiver typed by a trait
      // or a counted class.
      // ..and `self` is `&Rc<Self>` in a method that hands out a closure
      // holding it (`_receiverOf`), reached through twice.
      final through = target == null || target is IrThis
          ? (_selfName == 'self'
                ? (_selfIsHandle ? '&**self' : 'self')
                : _selfName == 'this_'
                ? 'this_'
                : cls.counted
                ? '&*$_selfName'
                : '&$_selfName')
          // A cast result is always a handle; a null-aware binding (`it`)
          // is a reference to the value, one deref short of a handle's
          // object and already a reference to a value (79 + 17, ws295).
          : target is IrCastTo
          ? '&*${expr(target)}'
          : target is IrBound
          ? (_isHandle(receiverClass) ? '&**${expr(target)}' : expr(target))
          // ..or a handle by its recorded type, when the class went
          // unrecorded (`widget.toStringShort()` on an `Rc<dyn
          // StatefulWidget>` was `&handle`, ws523).
          : _isHandle(receiverClass) || _handleLike(target)
          ? '&*${expr(target)}'
          : '&${expr(target)}';
      // A trait as the qualifier of a call on `this` is spelled through
      // the type that implements it (`<Self as RenderProxyBox>::set_child`):
      // a bare `Trait::method` is E0782 since edition 2021, and a base
      // constructor's body inlined into a subclass (`_inheritedBodies`)
      // writes the base's fields through the trait's setters (100 at ws443).
      // On a closure's handle (`__me`, an `Rc<dyn Trait>` in a trait body
      // or an `Rc<Struct>`), the plain call: `<__Self as Trait>::m(&*__me)`
      // wanted a `&__Self` where `__me` is the trait object
      // (`initMouseTracker`'s closure, ws461).
      if (_selfName == _countedSelf &&
          (target == null || target is IrThis) &&
          library.isAbstract(qualifier)) {
        // Still qualified -- the plain call was ambiguous where two traits
        // declare the name (`hit_test`, 16 at ws462) -- through the type
        // the handle is: the trait object in a trait body, the struct
        // otherwise.
        final declaring = _declaringTrait(qualifier, _identifier(name));
        final through = declaring ?? qualifier;
        final selfType = _fieldsAreAccessors
            ? 'dyn ${cls.name}${_useArguments(cls)}'
            : 'Self';
        if (typeArguments.isNotEmpty &&
            resultType != null &&
            _fieldsAreAccessors) {
          return _erasedCast(
            resultType,
            '<$selfType as $through${_traitArgsOf(through)}>::${_identifier(name)}__erased'
            '(&*$_selfName${args.isEmpty ? '' : ', '}${args.map(expr).join(', ')})$_propagate',
            method: _methodOf(through, name),
          );
        }
        return _asyncValue(
          '<$selfType as $through${_traitArgsOf(through)}>::${_identifier(name)}$turbofish'
          '(&*$_selfName${args.isEmpty ? '' : ', '}${args.map(expr).join(', ')})${suffixFor(_fieldsAreAccessors)}',
          boxed,
        );
      }
      // The trait named has to be the one *declaring* the item: a base
      // constructor's body inlined into a subclass wrote `this.child = x`
      // as `<Self as RenderView>::set_child`, and `set_child` is the
      // mixin's (`RenderObjectWithChildMixin`), which `RenderView` only
      // inherits (78 E0576 at ws460).
      if (asTrait == null && library.isAbstract(qualifier)) {
        final declaring = _declaringTrait(qualifier, _identifier(name));
        if (declaring != null) qualifier = declaring;
      }
      final path =
          asTrait ??
          (library.isAbstract(qualifier) && (target == null || target is IrThis)
              ? '<${_inSuperFn ? '__Self' : 'Self'} as $qualifier${_traitArgsOf(qualifier)}>'
              : qualifier);
      // A generic method of the trait, on `this` in a trait body through
      // the qualified path: its erased twin, as the plain call goes (the
      // method is `where Self: Sized` in the trait, and `__Self` may be
      // the trait object -- `getInheritedWidgetOfExactType<T>` calling
      // `getElementForInheritedWidgetOfExactType<T>`, ws494).
      if (typeArguments.isNotEmpty &&
          resultType != null &&
          asTrait == null &&
          library.isAbstract(qualifier) &&
          (target == null || target is IrThis) &&
          (_inSuperFn || _fieldsAreAccessors)) {
        return _erasedCast(
          resultType,
          '$path::${_identifier(name)}__erased'
          '($through${args.isEmpty ? '' : ', '}${args.map(expr).join(', ')})$_propagate',
          method: _methodOf(qualifier, name),
        );
      }
      return _asyncValue(
        '$path::${_identifier(name)}$turbofish'
        '($through${args.isEmpty ? '' : ', '}${args.map(expr).join(', ')})'
        '${suffixFor(true)}',
        boxed,
      );
    }
    // A plain call on `this` inside a trait body -- a super fn's `this_:
    // &__Self`, a trait default's `&self`, a closure's `dart_self_<trait>()`
    // handle in either -- dispatches through the trait, whose async
    // methods return `Result<DartFuture<T>, E>` (`_handleAsMethodCall` in
    // `MethodChannel.setMethodCallHandler`'s super fn, run458).
    final viaTrait =
        _fieldsAreAccessors && (target == null || target is IrThis);
    // `f.then(cb)`: the prelude's takes any callback whose result is a
    // `FutureOr<R>` or an `R` (`IntoFutureOr`), and `R` is what this call
    // declared -- spelled, since a callback returning `FutureOr` leaves it
    // ambiguous (`_LocalizationsState.load`, ws482).
    if (name == 'then' &&
        resultType != null &&
        resultType.name == 'Future' &&
        resultType.arguments.length == 1 &&
        !resultType.arguments.single.isFunction &&
        (receiverClass == null || library[receiverClass] == null)) {
      return '$receiver.then::<${type(resultType.arguments.single)}, _>'
          '(${args.map(expr).join(', ')})${suffixFor(viaTrait)}';
    }
    // A generic method through a trait object: its erased twin, and the
    // result cast back to what this call declared (see the prelude's
    // `CastErased`).
    // ..and on `this` inside a trait body -- a super fn's `this_: &__Self
    // + ?Sized`, a default's `&self` -- where the receiver names no class:
    // the generic method is `where Self: Sized` in the trait, and `__Self`
    // may well be the trait object (`invokeMapMethod` calling
    // `invokeMethod<Map>`, run494).
    if (typeArguments.isNotEmpty &&
        resultType != null &&
        ((receiverClass != null &&
                library.isAbstract(receiverClass) &&
                (target is! IrThis || _fieldsAreAccessors)) ||
            (receiverClass == null && viaTrait))) {
      return _erasedCast(
        resultType,
        '$receiver.${_identifier(name)}__erased'
        '(${args.map(expr).join(', ')})$_propagate',
        method: _methodOf(receiverClass ?? cls.name, name),
      );
    }
    if (Platform.environment['DART2RUST_TRACE_BACKEND'] == name) {
      stderr.writeln(
        'TRACE_BACKEND $name asyncFn=$asyncFn fails=$fails failing=$failing accessors=$_fieldsAreAccessors target=${target.runtimeType} self=$_selfName cls=${cls.name} receiverClass=$receiverClass resultType=$resultType typeArguments=$typeArguments qualifier=$qualifier',
      );
    }
    return _asyncValue(
      '$receiver.${_identifier(name)}$turbofish'
      '(${args.map(expr).join(', ')})${suffixFor(viaTrait)}',
      boxed,
    );
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
    // The prelude's unit codecs: `const Utf8Codec()`, `const JsonCodec()`.
    if (t.name == 'Utf8Codec' || t.name == 'JsonCodec') return t.name;
    if (t.name == 'Endian') {
      final little = fields['_littleEndian'];
      return little != null && expr(little) == 'true'
          ? 'Endian::Little'
          : 'Endian::Big';
    }
    // `dart:io`'s `FileMode` and `FileLock`: `const FileMode._internal(n)`
    // by the number it carries, the prelude's enums (`GetStorage`'s IO
    // backend, refused since it was reached, ws478).
    if (t.name == 'FileMode' || t.name == 'FileLock') {
      final carried = fields[t.name == 'FileMode' ? '_mode' : '_type'];
      final variants = t.name == 'FileMode'
          ? const ['Read', 'Write', 'Append', 'WriteOnly', 'WriteOnlyAppend']
          : const [
              'Shared',
              'Shared',
              'Exclusive',
              'BlockingShared',
              'BlockingExclusive',
            ];
      final index = carried == null ? null : int.tryParse(expr(carried));
      if (index != null && index >= 0 && index < variants.length) {
        return '${t.name}::${variants[index]}';
      }
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
      final value = fields[f.name]!;
      // A constant into a nullable field goes in through `Some`: the
      // `bool? signed` of a `const TextInputType(..)` held `false` bare
      // (26 in `widgets`).
      final wrapped =
          f.type.nullable &&
          ((value is IrLiteral &&
                  value.type.name != 'Null' &&
                  !value.type.nullable) ||
              value is IrConstInstance ||
              value is IrNew ||
              value is IrUpcast ||
              value is IrListLiteral ||
              value is IrMapLiteral ||
              // An enum value into a `TextBaseline?` (141 in `material`).
              (value is IrStatic && value.isEnumValue));
      final text = _returned(value);
      final stored = wrapped ? 'Some($text)' : text;
      // Into the cell the struct holds it in (`_fieldType`).
      final celled = _inCellOf(cls, f)
          ? (_isCopy(_heldType(f))
                ? 'std::rc::Rc::new(std::cell::Cell::new($stored))'
                : 'std::rc::Rc::new(std::cell::RefCell::new($stored))')
          : stored;
      parts.add('${snake(f.name)}: $celled');
      _returns = outer;
    }
    if (cls.counted) parts.add('__self: DartSelf::new()');
    // The phantom fields a generic class carries (see the struct's
    // emission): `const PersistentHashMap<Type, InheritedElement>.empty()`
    // (ws475).
    for (final unused in _unusedParameters(cls)) {
      parts.add('_phantom_${snake(unused)}: std::marker::PhantomData');
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
      final through = _accessorQualifier(name, kind: 'write');
      final widened = '__set.clone()';
      return through == null
          ? '{ let __set = ${expr(value)}; $receiver.set_${snake(name)}($widened)$_propagate; __set }'
          : '{ let __set = ${expr(value)}; $through::set_${snake(name)}($receiver, $widened)$_propagate; __set }';
    }
    if (shared != null) {
      final copy = _isCopy(_heldType(shared));
      return copy
          ? '{ let __set = ${expr(value)}; $receiver.${snake(name)}.set(__set); __set }'
          : '{ let __set = ${expr(value)}; '
                '*$receiver.${snake(name)}.borrow_mut() = __set.clone(); __set }';
    }
    return '{ let __set = ${expr(value)}; '
        '$receiver.${snake(name)} = __set.clone(); __set }';
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
        .map((step) => '.${step.$1}(${_stepClosure(step.$2, step: step.$1)})')
        .join();
    // A bare `forEach` hands the closure each element by value, as Dart
    // does: `keys.forEach(_updateProperty)` gave it `&Rc<..>` (53).
    final owned =
        chain.steps.length == 1 && chain.steps.single.$1 == 'for_each';
    return '${expr(chain.source)}.iter()${owned ? '.cloned()' : ''}$steps';
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
  String _stepClosure(IrExpr e, {String step = ''}) {
    // A function *value* as the step (`where(shouldNotSkip)`): called
    // from a closure of the step's own shape -- `filter` hands `&&T`,
    // the rest the item -- and its `Result` unwrapped, as a written
    // closure's is (E0631, 17 at ws464).
    if (e is! IrClosure) {
      final item = step == 'filter' ? '(*__x).clone()' : '__x.clone()';
      return '|__x| (${expr(e)})($item).unwrap()';
    }
    final params = e.params.map((p) => snake(p.name)).join(', ');
    final saved = _out.length;
    final savedIndent = _indent;
    _indent = 0;
    // A step of a std iterator chain (`all`, `map`, `filter`) returns a
    // plain value: a failing call inside unwraps, and the tail is bare.
    // Loud, and recorded: an exception in a `where` predicate panics.
    final savedFailure = _failure;
    _failure = null;
    stmt(e.body, tail: true);
    _failure = savedFailure;
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
      return '({ let __r = (**${_lazyName(owner, name)}).borrow().clone(); __r })';
    }
    // A clone: the lock hands out a reference, and a read is a value.
    // `(**CHANGE_NOTIFIER__EMPTY_LISTENERS)` moved out of the lock (E0507).
    if (_isLazy(owner, name)) return '(**${_lazyName(owner, name)}).clone()';
    if (_freeStatics(owner)) return screamingSnake('${owner}_$name');
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
    // Two nullable handles: identical when both null or both the same
    // object (the prelude asks; `&*a` on an `Option` was E0614, ws463).
    final leftType = left.rustType, rightType = right.rustType;
    if (leftType != null &&
        rightType != null &&
        isNullable(leftType) &&
        isNullable(rightType)) {
      return 'dart_identical_opt(&${expr(left)}, &${expr(right)})';
    }
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

  /// See `IrDynamicDispatch`.
  String _dispatch(IrExpr receiver, List<(IrType?, IrExpr)> arms) {
    final out = StringBuffer('{ let __d = ${expr(receiver)}; ');
    final asAny = _handleLike(receiver)
        ? '__d.as_ref().as_any()'
        : '__d.as_any()';
    var first = true;
    for (final (t, body) in arms) {
      if (t == null) {
        out.write('${first ? '' : ' else '}{ ${expr(body)} }');
      } else {
        out.write(
          '${first ? '' : ' else '}if let Some(__t) = $asAny.downcast_ref::<${type(t)}>() '
          '{ let __d = __t.clone(); ${expr(body)} }',
        );
      }
      first = false;
    }
    if (arms.isEmpty || arms.last.$1 != null) {
      out.write(
        ' else { panic!("uncaught Dart exception: a dynamic slot held an unexpected type") }',
      );
    }
    out.write(' }');
    return out.toString();
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
    'LinkedHashSet': 'Set',
    'HashSet': 'Set',
    'LinkedHashMap': 'Map',
    'HashMap': 'Map',
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
    // `Object()` as an identity token -- a lock, a sentinel: a fresh unit
    // behind a handle, which `dyn Object` accepts (`_RenderObjectSemantics`,
    // 119 callers of a constructor refused for this).
    if (t.name == 'Object' && args.isEmpty) {
      return '(std::rc::Rc::new(()) as std::rc::Rc<dyn Object>)';
    }
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
  /// A body under the Result model: a `void` one that falls off its end
  /// ends in `Ok(())`, since the signature says `Result<(), E>`.
  void _body(IrStmt body, IrType returnType) {
    final rendered = type(returnType);
    // A `Null?` return (a `FutureOr<void>` callback's) falls off into
    // `Ok(None)` as `()` does into `Ok(())` (52 in `widgets`).
    final unit = rendered == '()';
    final optional = rendered.startsWith('Option<');
    // ..and a `FutureOr<void>` one (a `then` callback's, `Route.didAdd`,
    // ws504) into the done `FutureOr` of `()`.
    final futureOrUnit = rendered == 'FutureOr<()>';
    final fallsOff =
        _failure != null &&
        (unit || optional || futureOrUnit) &&
        !_alwaysReturns(body);
    stmt(body, tail: !fallsOff);
    if (fallsOff) {
      _line(
        unit
            ? 'Ok(())'
            : futureOrUnit
            ? 'Ok(FutureOr::value(()))'
            : 'Ok(None)',
      );
    }
  }

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
          // Inside an outer try's closure the return is that closure's
          // value again (`inflateWidget`'s try in a try, ws482).
          _line(
            wasFlowing
                ? 'Ok(Some(__returned)) => return Ok(Some(__returned)),'
                : 'Ok(Some(__returned)) => return __returned,',
          );
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
          // Inside an outer try's closure the return is that closure's
          // value again (`inflateWidget`'s try/catch inside its
          // try/finally, ws485).
          _line(
            outer
                ? 'Ok(Some(__returned)) => return Ok(Some(__returned)),'
                : 'Ok(Some(__returned)) => return __returned,',
          );
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
        // ..and into one held in a cell -- a counted class's storage, its
        // own or another object's (`cascaded._m4storage[i] = 1.0` on a
        // counted `Matrix4`, ws511) -- through the cell's `borrow_mut`.
        final cellPlace = _cellPlace(target);
        final place = cellPlace != null
            ? '$cellPlace.borrow_mut()'
            : target is IrField &&
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
      case IrLocalFunction(:final name, :final closure, :final recursive):
        if (!recursive) {
          _line('let ${snake(name)} = ${expr(closure)};');
          break;
        }
        // A closure cannot name itself: the binding is a cell, filled
        // after the closure is made with a handle to the same cell, and
        // every read of the name -- inside the body and after it -- goes
        // through the cell (`_cellLocals`, unwrapped as a `late` local).
        final fnType = type(
          IrType.function([
            for (final p in closure.params) p.type,
          ], closure.returns),
        );
        _line(
          'let ${snake(name)}: std::rc::Rc<std::cell::RefCell<Option<$fnType>>> = std::rc::Rc::new(std::cell::RefCell::new(None));',
        );
        _cellLocals = {..._cellLocals, name: false};
        _lateCellLocals = {..._lateCellLocals, name};
        // The closure moves a handle to the cell, not the cell's binding.
        final boxed = closure.boxed ? '' : 'std::rc::Rc::new';
        _line(
          '*${snake(name)}.borrow_mut() = Some($boxed({ let ${snake(name)} = ${snake(name)}.clone(); ${expr(closure)} }));',
        );
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
          // A `dynamic` cell starts as Dart's null, the `Null` object.
          final inner = init != null
              ? expr(init)
              : rust == 'std::rc::Rc<dyn Object>'
              ? 'std::rc::Rc::new(Null) as std::rc::Rc<dyn Object>'
              : rust != null && rust.startsWith('Option<')
              ? 'None'
              : 'Default::default()';
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
        // A cascade's temporary is written by construction (`..add(x)`),
        // in a static's initialiser as anywhere else.
        final mutable = _reassigned.contains(name) || name == 'cascaded'
            ? 'mut '
            : '';
        // A declared function type is `Box<dyn Fn(..)>`, and a closure's own
        // type is not that. `_returned` boxes for the same reason one line
        // further out; a `let` is the other half of it.
        // ..and an inferred one too: `final listener = () { .. }` is a
        // function-typed local whether or not the type was written (38
        // closures handed to an `Rc<dyn Fn()>` slot in the gallery).
        final boxed = init is IrClosure && (type == null || type.isFunction);
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
        // A local read whole as another's initialiser is a clone, as it
        // is as an argument: `let __t = node;` moved a parameter a closure
        // then read (`_requestFocus`, ws522). `Clone` on a `Copy` type is
        // the copy.
        final value = boxed
            ? 'std::rc::Rc::new(${expr(init)})'
            : init is IrLocal &&
                  !_cellLocals.containsKey(init.name) &&
                  !_closureCaptured.contains(init.name)
            ? '${_returned(init)}.clone()'
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
        // A field of the value in a static's cell (`staticFieldWrites`).
        // ..or, when the static holds a *counted* object, through the
        // field's own cell on the handle: the object is not in a cell, its
        // fields are (`GoogleFonts.config.allowRuntimeFetching = false` on
        // a counted `Config`, run510).
        final staticHolder = target is IrStatic
            ? '(**${_lazyName(target.owner, target.name)})'
            : target is IrTopLevel
            ? '(**${screamingSnake(target.name)})'
            : null;
        if (staticHolder != null &&
            shared != null &&
            owner != null &&
            (library[owner]?.counted ?? false)) {
          _line(
            _fieldIsCopy(shared, library[owner])
                ? '$staticHolder.${snake(name)}.set($written);'
                : '*$staticHolder.${snake(name)}.borrow_mut() = $written;',
          );
        } else if (staticHolder != null) {
          _line('$staticHolder.borrow_mut().${snake(name)} = $written;');
        }
        // Inside a trait's body there is no field, only the setter it
        // declares (`this_.set__length(v)` in a mixin's super function).
        else if (_fieldsAreAccessors && (target == null || target is IrThis)) {
          // Through the setter, which takes the plain value and does its
          // own `Some` for a `late` field (106 `f64` <- `Option<f64>`
          // on `_globalDistanceMoved`, ws384).
          final through = _accessorQualifier(name, kind: 'write');
          final widened = expr(value);
          _line(
            through == null
                ? '$receiver.set_${snake(name)}($widened)$_propagate;'
                : '$through::set_${snake(name)}($receiver, $widened)$_propagate;',
          );
        } else if (target != null &&
            target is! IrThis &&
            owner != null &&
            library.isAbstract(owner)) {
          // A write on a trait handle (`cascaded.tolerance = t` on an
          // `Rc<dyn Simulation>`) is the setter the trait declares (113).
          _line('$receiver.set_${snake(name)}($written)$_propagate;');
        } else if (shared != null) {
          // Through the cell, which is why the field can be written from a
          // closure that does not hold `self` at all.
          _line(
            _fieldIsCopy(
                  shared,
                  target == null || target is IrThis
                      ? cls
                      : (owner == null ? null : library[owner]),
                )
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
      case IrSetter(
        :final target,
        :final name,
        :final value,
        :final qualifier,
        :final receiverClass,
      ):
        // A setter is a method and returns `Result` like one. Through the
        // trait when two declare it (`IrSetter.qualifier`).
        if (qualifier != null) {
          final argument = value;
          _line(
            '${_call(target, 'set_${snake(name)}', [argument], qualifier: qualifier, receiverClass: receiverClass, fails: true)};',
          );
        } else {
          _line(
            '${_receiver(target)}.set_${snake(name)}(${expr(value)})$_propagate;',
          );
        }
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
        holder._member(
          'top-level ${function.name}',
          () {
            holder._emitFreeFunction(function);
          },
          stub: (reason) => holder._emitFreeFunction(function, stubbed: reason),
        );
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
    // `name`: `dart:core`'s `EnumName` extension, the value's Dart name.
    _line('pub fn name(&self) -> String {');
    _indent++;
    _line('match self {');
    _indent++;
    for (final value in cls.values) {
      _line('${cls.name}::${variants[value]} => "$value".to_string(),');
    }
    _indent--;
    _line('}');
    _indent--;
    _line('}');
    _indent--;
    _line('}');
    _line('');
    // ..and both through the prelude's `DartEnum`, which is how a generic
    // `enum_name_get_name(e)` reaches them.
    _line('impl DartEnum for ${cls.name} {');
    _indent++;
    _line('fn name(&self) -> String { ${cls.name}::name(self) }');
    _line('fn index(&self) -> i64 { ${cls.name}::index(self) }');
    _indent--;
    _line('}');
    _line('');
    _emitDartNullable();
    _emitDartEq(body: 'self == other');
    _emitEnumDartAny();
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
        _member(
          '${cls.name}.${method.name}',
          () => _emitMethod(method),
          stub: (reason) => _emitMethod(method, stubbed: reason),
        );
      }
      _indent--;
      _line('}');
    }
    // An enum implementing an interface -- `WidgetState` is a
    // `WidgetStatesConstraint` -- gets the impl a struct would, forwarding
    // to the enhanced enum's own methods (20 "trait bound not satisfied").
    _emitBaseImpl();
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
      // Not the prelude's interfaces: `SourceSpan implements Comparable<
      // SourceSpan>` as a supertrait names the trait inside its own bound
      // ("cycle detected when computing the super predicates"). A concrete
      // class gets a forwarding impl instead (`_emitPreludeInterfaces`).
      // ..and the mixins: `FixedScrollMetrics with ScrollMetrics` is a
      // `ScrollMetrics`, and its super functions call the mixin's methods
      // on `this_: &__Self` (39 `__Self: ScrollMetrics` at ws301).
      for (final i in [...cls.interfaces, ...cls.mixins])
        if (library.isAbstract(i.name) &&
            i.name != 'Object' &&
            !_preludeInterfaces.containsKey(i.name))
          _traitPath(i),
    }.toList();
    // A trait object compares by identity (`DartEq`), as `dyn Object` does.
    _line(
      'impl${_generics(cls, static: true, clone: false)} DartEq for dyn ${cls.name}${cls.typeParameters.isEmpty ? '' : '<${cls.typeParameters.join(', ')}>'} {',
    );
    _indent++;
    _line(
      'fn dart_eq(&self, other: &Self) -> bool { std::ptr::addr_eq(self as *const Self, other as *const Self) }',
    );
    _indent--;
    _line('}');
    _line('');
    _line(
      // No `Clone` on the trait's parameters: a `dyn Foo<Pin<Box<dyn
      // Future>>>` is named (`_CallbackHookProvider<Future<bool>>`), and a
      // bound here shuts the whole trait. The default methods that clone
      // carry it instead (`_traitWhere`).
      '${_vis(cls.name)}trait ${cls.name}${_generics(cls, static: true, clone: false)}: ${supers.join(' + ')} {',
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
    // Not a field an abstract supertype already declares: a mixin's
    // fields are lowered into the class applying it, and an open class
    // that then became a trait offered `_listeners` beside
    // `ChangeNotifier`'s -- every read of it ambiguous (50 E0034 at
    // ws305). The ancestor's impl block carries the accessor.
    final inherited = {
      for (final above in _supertypesOf(cls))
        if (library.isAbstract(above.name))
          for (final f in above.fields) f.name,
    };
    for (final field in cls.fields) {
      if (inherited.contains(field.name)) continue;
      _line('/// `${cls.name}.${field.name}`, which the implementor stores.');
      // An accessor is a function like any other under the Result model.
      _line('fn ${snake(field.name)}(&self) -> ${_wrapped(type(field.type))};');
      // ..and writes, when the mixin's own methods write it: `_length =
      // newLength` inside `_TypedDataBuffer._grow` goes through this. The
      // receiver is `&self`: an implementer of a mixin that writes is
      // counted, and its field is a cell.
      if (!field.isFinal) {
        _line(
          'fn set_${snake(field.name)}(&self, value: ${type(field.type)}) -> ${_wrapped('()')};',
        );
      }
      if (_handsCell(field)) {
        _line(
          'fn ${snake(field.name)}_cell(&self) -> ${_wrapped(_cellType(type(field.type)))};',
        );
      }
      _line('');
    }
    // `this` as a value inside the trait's own bodies (see `DartSelf`).
    _line(
      'fn dart_self_${snakeRaw(cls.name)}(&self) -> std::rc::Rc<dyn ${cls.name}${_useArguments(cls)}>;',
    );
    _line('');
    for (final method in cls.abstractMethods) {
      _member('${cls.name}.${method.name} (required)', () {
        _refuseShadowedGeneric(method);
        _doc(method.doc);
        _line(
          'fn ${_methodName(method)}${_generics(method)}(${_params(method)})'
          ' -> ${_wrapped(method.isAsync ? _futureOf(method) : _spelledReturn(type(method.returnType)))}${_sizedBound(method)};',
        );
        _line('');
        _emitErasedTwin(method, defaultBody: false);
      });
    }
    for (final method in cls.methods) {
      if (method.isStatic) continue;
      _member('${cls.name}.${method.name} (default)', () {
        _refuseShadowedGeneric(method);
        _doc(method.doc);
        _line(
          'fn ${_methodName(method)}${_generics(method)}(${_params(method)})'
          ' -> ${_wrapped(method.isAsync ? _futureOf(method) : _spelledReturn(type(method.returnType)))}${_traitWhere(method)} {',
        );
        _indent++;
        // The default delegates to the free function rather than holding the
        // body, so that an override can still reach it. See `superFn`.
        if (_superFailed.contains(method.name)) {
          _line('todo!("${cls.name}.${method.name} did not translate")');
        } else {
          // An `async` super function is an `async fn`: its future is
          // boxed here, borrowing `self` for the `'_` the signature allows.
          // The trait's own parameters spelled, so a class implementing
          // it at two instantiations is not ambiguous (ws451).
          final spelled = [
            if (cls.typeParameters.isNotEmpty ||
                method.typeParameters.isNotEmpty)
              '::<_${[...cls.typeParameters, ...method.typeParameters].map((p) => ', $p').join()}>',
          ].join();
          final call =
              '${superFn(cls.name, method.name, isSetter: method.isSetter)}$spelled('
              '${['self', ...method.params.map((p) => snake(p.name))].join(', ')})';
          // An async super function is a future, not a `Result`: the
          // trait default returns it in `Ok`.
          _line(method.isAsync && _resultModel ? 'Ok($call)' : call);
        }
        _indent--;
        _line('}');
        _line('');
        _emitErasedTwin(
          method,
          defaultBody: !_superFailed.contains(method.name),
        );
      });
    }
    _indent--;
    _line('}');
    // A trait holds no storage, but the *class* still had its statics, and in
    // Rust those are module-level items rather than trait items. The struct
    // path has emitted them all along and this one had not, so an abstract
    // class's `static` simply vanished: `NavigatorObserver._navigators` was
    // read from three places and declared nowhere.
    // The trait object is an `Object` through `DartAny: Object`: a
    // supertrait, so that an `Rc<dyn Widget>` unsizes to an `Rc<dyn
    // Object>`. An `impl Object for dyn Widget` stood here before and gave
    // no coercion (75 non-primitive casts at ws293).
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

  /// Whether a class's own declaration spells a projected `T?` anywhere:
  /// a field, a constructor's or a method's parameter, a result.
  bool _usesProjection(IrClass c) =>
      _allFields(c).any((f) => f.type.projected) ||
      c.constructors.any((k) => k.params.any((p) => p.type.projected)) ||
      [...c.methods, ...c.abstractMethods].any(
        (m) => m.returnType.projected || m.params.any((p) => p.type.projected),
      );

  /// Whether a class's type parameters need `DartNullable`: it or a class
  /// it implements spells `<T as DartNullable>::Or`. Not every class: the
  /// bound shuts a future out (`_CallbackHookProvider<Future<bool>>`), and
  /// only a projection asks for it.
  bool _needsNullable(IrClass c) {
    final seen = <String>{};
    bool walk(IrClass k) {
      if (!seen.add(k.name)) return false;
      if (_usesProjection(k)) return true;
      for (final name in [
        if (k.superclass != null) k.superclass!,
        ...k.mixins.map((m) => m.name),
        ...k.interfaces.map((i) => i.name),
      ]) {
        final other = library[name];
        if (other != null && walk(other)) return true;
      }
      return false;
    }

    return walk(c);
  }

  /// `DartNullable` on every type parameter after all: a bound only where
  /// a projection asks for it has to be repeated by everything that names
  /// the generic type (`SlottedRenderObjectElement<SlotType>` in a trait
  /// whose `SlotType` had none, ws401), and every type implements it -- a
  /// boxed future through a handle (see the prelude). What `_needsNullable`
  /// still decides is `Clone` on a *struct's* parameters: a projecting
  /// struct's `T?` field is `<Vec<T> as DartNullable>::Or` when `T` is put
  /// in for a `List`, and that asks `T: Clone`.
  /// ..and `FromDynamic` beside it, on the same footing: Dart's `cast<K,
  /// V>` takes any `K`, and a generic body's `result.cast<K, V>()` asks
  /// it of a bare `K` (`invokeMapMethod`, run494). Every type carries it
  /// (the prelude's, `_emitFromDynamic` for the translated ones).
  String _nb(IrClass c) =>
      ' + DartNullable<Or: Clone + DartEq + FromDynamic> + DartEq + FromDynamic';

  String _nbm(IrMethod m) =>
      ' + DartNullable<Or: Clone + DartEq + FromDynamic> + DartEq + FromDynamic';

  /// `DartNullable` for this struct or enum (see the prelude): its `T?` is
  /// `Option<Self>`. With the class's own generics, as its `DartAny` is.
  /// A `late` field whose initialiser mentions `this` is Dart's lazy one:
  /// evaluated on the first read, in a cell so a `&self` read can fill
  /// it. (`late final _manifold = _BindingPipelineManifold(this)` read
  /// `_semanticsEnabled`, set by an `initInstances` that ran *after* the
  /// constructor's eager evaluation of it: run440's `None`.)
  bool _lazyLate(IrFieldDecl f) =>
      f.isLate &&
      f.initial != null &&
      _mentionsThis(f.initial!) &&
      _inCell(f) &&
      !_lazyExpanding.contains(f.name);

  /// The lazy fields whose initialiser is being printed: a read of the
  /// same field inside it (a closure the initialiser hands out reading it
  /// later) is the plain read, or the expansion never ends.
  final _lazyExpanding = <String>{};

  /// The class's lazy `late` field of this name, by its full declaration
  /// (the shared-field census carries no initialiser).
  IrFieldDecl? _lazyDecl(String name) =>
      _allFields(cls).where((f) => f.name == name && _lazyLate(f)).firstOrNull;

  /// The read of a lazy `late` field through `receiver`: filled on the
  /// first read, the value each time.
  String _lazyRead(IrFieldDecl f, String receiver) {
    final name = snake(f.name);
    _lazyExpanding.add(f.name);
    // In the impl's terms when forwarded through one: `FormFieldState<T>`'s
    // `_value` under `impl FormFieldState<String>` (ws451).
    final init = expr(
      _implBinding.isEmpty
          ? f.initial!
          : _substitute(f.initial!, const {}, _implBinding),
    );
    _lazyExpanding.remove(f.name);
    return _isCopy(_heldType(f))
        ? '{ if $receiver.$name.get().is_none() { let __v = $init; $receiver.$name.set(Some(__v)); } $receiver.$name.get().unwrap() }'
        : '{ if $receiver.$name.borrow().is_none() { let __v = $init; *$receiver.$name.borrow_mut() = Some(__v); } let __r = $receiver.$name.borrow().clone().unwrap(); __r }';
  }

  /// The type arguments this class passes to the generic trait `name`, spelled
  /// (`<T>`), or nothing for a non-generic trait or one it cannot compute
  /// (151 E0107 `missing generics for trait` at ws445).
  String _traitArgsOf(String name) {
    final trait = library[name];
    if (trait == null || trait.typeParameters.isEmpty) return '';
    // This class's own trait: its own parameters (`<__Self as
    // CupertinoPageRoute<T>>` in its super fns, ws451).
    if (name == cls.name) return _generics(cls);
    final passed = _argumentsThrough(cls, const {}, trait, {});
    if (passed == null || passed.isEmpty) return '';
    return '<${passed.map(type).join(', ')}>';
  }

  /// The locals the closure being printed captured (see `IrLocal`).
  var _closureCaptured = <String>{};

  /// `DartEq` for the struct or enum (see the prelude's `DartEq`): `body`
  /// compares `self` and `other`; `extraBound` joins each type parameter's
  /// bounds, `where` follows the header.
  void _emitDartEq({
    required String body,
    String extraBound = '',
    String where = '',
  }) {
    final own = '${cls.name}${_generics(cls)}';
    final header = cls.typeParameters.isEmpty
        ? ''
        : '<${cls.typeParameters.map((p) => "$p: Clone${_nb(cls)} + 'static$extraBound").join(', ')}>';
    _line('impl$header DartEq for $own$where {');
    _indent++;
    _line('fn dart_eq(&self, other: &Self) -> bool { $body }');
    _indent--;
    _line('}');
    _line('');
  }

  /// `FromDynamic` for the struct or enum (see the prelude's): the object
  /// asked for a value of this type (`dart_cast_any`), which is exact --
  /// a struct that cannot be cloned out of an object answers `None`.
  void _emitFromDynamic() {
    final own = '${cls.name}${_generics(cls)}';
    final body = _cloneable(cls) ? 'value.dart_cast_any::<Self>()' : 'None';
    _line(
      'impl${_generics(cls, static: true, clone: true)} FromDynamic for $own {',
    );
    _indent++;
    _line(
      'fn from_dynamic(value: &std::rc::Rc<dyn Object>) -> Option<Self> { $body }',
    );
    if (_cloneable(cls)) {
      _line(
        'fn from_same(value: &Self) -> Option<Self> { Some(value.clone()) }',
      );
    }
    _indent--;
    _line('}');
    _line('');
  }

  /// `to_list` for a struct that *is* an `Iterable<E>` (`IrClass.
  /// iterableElement`): its elements, walked off its own `iterator` --
  /// what `Iterable`'s members and a `for-in` on it read (the front end's
  /// `_listReceiver`; `Navigator`'s `_History`, ws499). Only where the
  /// struct carries the getter itself.
  void _emitToList() {
    final element = cls.iterableElement;
    if (element == null) return;
    final getter = cls.methods
        .where((m) => m.name == 'iterator' && m.isGetter && !m.isStatic)
        .firstOrNull;
    // ..declared as an `Iterator<E>`: a covariant `CharacterRange get
    // iterator` hands out its own trait handle, whose `move_next` is not
    // the prelude's (ws501).
    if (getter == null || getter.returnType.name != 'DartIterator') return;
    // Not a failing call: a `for-in` and a chain read the list where no
    // `?` can go, so a failing `iterator` getter is an uncaught exception
    // here, as it would be in Dart.
    final fetched = getter.fails
        ? 'match self.iterator() { Ok(__it) => __it, Err(__e) => panic!("uncaught Dart exception: {}", dart_str(&__e)) }'
        : 'self.iterator()';
    final element_ = type(element);
    // `__to_list`, a name no Dart member has: `ObserverList` overrides
    // `toList` itself, and the walker beside it was a duplicate (E0592,
    // ws500).
    _line(
      'pub fn __to_list(&self) -> Vec<$element_> { '
      'let __it = $fetched; let mut __out: Vec<$element_> = Vec::new(); '
      'while __it.move_next() { __out.push(__it.current()); } __out }',
    );
  }

  /// `NativeAnswer` for the struct or enum (see the prelude's): a native
  /// declared to return one of its own (`dart:ui`'s `GlyphInfo`) reads the
  /// host's object as it; without one there is no value to give.
  void _emitNativeAnswer() {
    final own = '${cls.name}${_generics(cls)}';
    _line(
      'impl${_generics(cls, static: true, clone: true)} NativeAnswer for $own {',
    );
    _indent++;
    _line(
      'fn from_answer(answer: std::rc::Rc<dyn Object>, symbol: &str) -> Self { '
      'match answer.dart_cast_any::<Self>() { Some(value) => value, '
      'None => panic!("native `{}` answered {:?} where ${cls.name} was declared", symbol, answer) } }',
    );
    _line(
      'fn absent() -> Self { panic!("native answered nothing where ${cls.name} was declared") }',
    );
    _indent--;
    _line('}');
    _line('');
  }

  /// `DartAny` for an enum (see the struct's inline impl): its own type
  /// behind a fresh handle, and every interface it implements through the
  /// handle that impl keeps -- what lets `_emitBaseImpl`'s `impl Ts for U`
  /// compile, `Ts: DartAny` (an enum into an `Rc<dyn Ts>`, ws510).
  void _emitEnumDartAny() {
    _line('impl DartAny for ${cls.name} {');
    _indent++;
    _line(
      'fn dart_runtime_type(&self) -> Type { Type { name: "${cls.name}" } }',
    );
    _line(
      'fn dart_cast(&self, __t: std::any::TypeId) -> Option<std::boxed::Box<dyn std::any::Any>> {',
    );
    _indent++;
    _line(
      'if __t == std::any::TypeId::of::<Self>() || __t == std::any::TypeId::of::<std::rc::Rc<Self>>() { return Some(std::boxed::Box::new(std::rc::Rc::new(self.clone()))); }',
    );
    for (final above in _abstractAncestors(cls)) {
      final arguments = _baseArguments(above);
      if (arguments == null) continue;
      _line(
        'if __t == std::any::TypeId::of::<dyn ${above.name}$arguments>() || __t == std::any::TypeId::of::<std::rc::Rc<dyn ${above.name}$arguments>>() { return Some(std::boxed::Box::new(self.dart_self_${snakeRaw(above.name)}())); }',
      );
    }
    _line('None');
    _indent--;
    _line('}');
    _indent--;
    _line('}');
    _line('');
  }

  void _emitDartNullable() {
    _emitFromDynamic();
    _emitNativeAnswer();
    final own = '${cls.name}${_generics(cls)}';
    // The struct's own bounds, not an impl's: `Or` is `Option<Self>` and
    // asks nothing of `T`, and a `T: Clone` here would have shut the
    // struct out of every `T?` slot in code that has no `Clone` (ws404).
    _line('impl${_generics(cls, static: true)} DartNullable for $own {');
    _indent++;
    _line('type Or = Option<Self>;');
    _line('fn option(or: Option<Self>) -> Option<Self> { or }');
    _line('fn from_option(option: Option<Self>) -> Option<Self> { option }');
    _indent--;
    _line('}');
    _line('');
  }

  static String _abstractStaticName(String owner, String name) =>
      _rustIdentifier('${snakeRaw(owner)}_${snakeRaw(name)}');

  /// A function used as a value, behind the handle every function slot
  /// is. An `async` function's item returns its future bare, where a
  /// function value returns `Result` like everything else: a closure
  /// around it puts the `Ok` on (`registerServiceExtension(callback:
  /// _exitApplication)`, run453).
  String _functionRef(String? owner, String name, IrType? type) {
    final path = owner == null
        ? snake(name)
        : _freeStatics(owner)
        ? _abstractStaticName(owner, name)
        : '$owner::${snake(name)}';
    final target = owner == null
        ? library.functions.where((f) => f.name == name).firstOrNull
        : library[owner]?.methods
              .where((m) => m.name == name && m.isStatic)
              .firstOrNull;
    if (target == null || !target.isAsync) return 'std::rc::Rc::new($path)';
    // The parameters and the error spelled, as a closure literal's are:
    // nothing else infers them behind the `Rc` (E0282, ws454).
    final params = type?.parameters ?? [for (final p in target.params) p.type];
    final args = [for (var i = 0; i < params.length; i++) '__a$i'];
    final spelled = [
      for (var i = 0; i < params.length; i++)
        '${args[i]}: ${this.type(params[i], owned: params[i].name == 'Future' || params[i].isFunction)}',
    ].join(', ');
    return 'std::rc::Rc::new(|$spelled| -> Result<_, $_error> { Ok($path(${args.join(', ')})) })';
  }

  /// Whether a class's statics live at module level under the class's
  /// name: an abstract class is a trait and has nowhere else to put them;
  /// a *generic* class's `impl<T> Foo<T>` would make every static call
  /// name a `T` the static never mentions (`RadioGroup.maybeOf<T>()`, 12
  /// "cannot infer type" at ws397).
  bool _freeStatics(String owner) =>
      library.isAbstract(owner) ||
      (library[owner]?.typeParameters.isNotEmpty ?? false);

  /// `<T>` for a class or method that has parameters, and nothing otherwise.
  /// Whether the struct derives `Clone`: nothing it holds is a bare future.
  bool _cloneable(IrClass of) =>
      !_allFields(of)
          .any((f) => _fieldType(f).contains('dyn std::future::Future'));

  /// A class's parameters as a use: `<T, U>`, or nothing.
  String _useArguments(IrClass of) =>
      of.typeParameters.isEmpty ? '' : '<${of.typeParameters.join(', ')}>';

  /// `clone` puts `Clone` on a class's parameters. Off by default: a
  /// declaration -- struct, trait, the marker impls -- needs no bound to
  /// exist, and one there is demanded wherever the type is *named* (a
  /// trait accessor returning `Vec<TweenSequenceItem<T>>` under `T:
  /// 'static`, ws303). The impl blocks whose bodies clone ask for it
  /// themselves (`_implGenerics`, the super functions, `_traitWhere`).
  String _generics(Object owner, {bool static = false, bool clone = false}) {
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
    // A method's own parameter keeps `Clone`: `binarySearch<T>` clones
    // its `T` (148 ".clone on T"), and nothing instantiates a method's
    // parameter with a future. A class's does not (`_CallbackHookProvider<
    // Future<bool>>`), see `bound` in `_boundedGenerics`.
    final bound = owner is IrMethod
        ? params.map((p) => "$p: Clone${_nbm(owner)} + 'static")
        : static
        // `Clone` on a class's parameters after all (ws301): every held
        // `T` is read by `.clone()`, and 240 stubs said so; the one shape
        // that is not `Clone`, a bare future, is measured against that.
        ? params.map(
            (p) => clone
                ? "$p: Clone${owner is IrClass ? _nb(owner) : ''} + 'static"
                : "$p: DartNullable<Or: DartEq + FromDynamic> + DartEq + FromDynamic + 'static",
          )
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

  /// Prelude classes whose constructors are not `const fn`.
  static const _preludeTypes = {
    'Stopwatch',
    'DateTime',
    'Duration',
    'Completer',
    'StringBuffer',
    'RegExp',
    'Uri',
    'Random',
    'Expando',
    'Zone',
    'Map',
    'Set',
    'Queue',
    'Stream',
  };

  bool _constEvaluable(IrExpr e) => switch (e) {
    IrLiteral() => true,
    IrStatic() => true,
    IrTopLevel() => true,
    IrBinary(:final left, :final right) =>
      _constEvaluable(left) && _constEvaluable(right),
    IrUnary(:final operand) => _constEvaluable(operand),
    IrCast(:final value) => _constEvaluable(value),
    IrConstInstance(:final fields) => fields.values.every(_constEvaluable),
    // A translated class's constructor is a `const fn`; the prelude's
    // (`Stopwatch::new()`) are not, and a `const` holding one does not
    // compile (E0015 in `foundation_print`).
    // ..and only a `const` constructor is one: `SpringDescription.
    // withDampingRatio` takes a square root, and the `final` top-level
    // holding one was emitted as a `const` (E0015, `animation`).
    // Under the Result model a constructor call is a `Result`, which no
    // `const` item can unwrap: every such initialiser is lazy.
    IrNew(:final type, :final args, :final constructor) =>
      !_resultModel &&
          !_preludeTypes.contains(type.name) &&
          _constConstructor(type.name, constructor) &&
          args.every(_constEvaluable),
    _ => false,
  };

  bool _constConstructor(String className, String? name) {
    final c = library[className];
    if (c == null) return true;
    final k = c.constructors.where((k) => k.name == name);
    return k.isEmpty || k.first.isConst;
  }

  /// Whether every translated class named in a type text can be compared.
  bool _comparableType(String rust, Set<String> seen) {
    if (rust.contains('dyn Fn') || rust.contains('dyn std::future::Future'))
      return false;
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

  /// The generics of an `impl` block: every parameter `Clone + DartNullable<Or: Clone> + 'static`.
  ///
  /// A method body clones what it reads (`self._map.clone()`), and a
  /// `Map<K, V>` is `Clone` only when `K` and `V` are; an `Rc<dyn ..>` held
  /// in a `T` slot wants `'static`. 30 "trait bounds were not satisfied"
  /// and 12 E0310s in `collection`. The struct and trait declarations stay
  /// unbounded, so a type argument that is neither is still a type -- only
  /// its methods are missing, which is loud where it matters.
  String _implGenerics(IrClass cls, {bool keyed = true}) {
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
      final key = RegExp('(Map|Set)<$p[,>]').hasMatch(fields);
      // `PartialEq`: `self._value == new_value` on a `T` (`ValueNotifier`).
      // `Clone + DartNullable<Or: Clone> + 'static` only (2026-09-04): `PartialEq + Debug` on every
      // type parameter shut out closures and futures -- `ObserverList<
      // VoidCallback>`, a `Set<Future>` -- at the class, not at the one
      // method that compares or prints. A method that does is what fails
      // now, and the stub count says how many.
      // The prelude's `Map` and `Set` are ordered and compare keys with
      // `==`: `PartialEq + Clone` is all they ask, and `Eq + Hash` shut
      // closures out of `ObserverList<VoidCallback>` (48 in `widgets`).
      // ..and no `PartialEq` for a key parameter either (2026-09-06): the
      // prelude's `Map` and `Set` compare keys by `DartEq`, which every
      // parameter already carries; the `PartialEq` block shut
      // `ObserverList<VoidCallback>.add` out (run459).
      return "$p: Clone${_nb(cls)} + 'static";
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

  /// Whether a field's cell is a `Cell` (its held type is `Copy`) as seen
  /// from anywhere: a field of another class typed by *that* class's
  /// parameter (`Holder<T>.value`, written as `h.value = ..` from outside,
  /// ws510) is not `Copy` -- the parameter is no name known here.
  bool _fieldIsCopy(IrFieldDecl field, IrClass? owner) {
    final held = _heldType(field);
    if (owner != null && _namesIn(held).any(owner.typeParameters.contains)) {
      return false;
    }
    return _isCopy(held);
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
      // *That* class's parameters: `Tween<T>` holds an `Option<T>`, and
      // asked from `AnimatedPositionedState` its `T` read as a class name
      // nobody knew, so the field went into a `Cell` (3 "Tween<f64>: Copy
      // is not satisfied" in `widgets`).
      if (_namesIn(held).any(other.typeParameters.contains)) return false;
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
  /// A refusal reason as the text of a `todo!` (a Rust format string).
  static String _stubText(String reason) => reason
      .replaceAll('\\', '\\\\')
      .replaceAll('"', '\\"')
      .replaceAll('{', '{{')
      .replaceAll('}', '}}')
      .replaceAll('\n', ' ');

  void _emitFreeFunction(IrMethod method, {String? stubbed}) {
    _doc(method.doc);
    // Before the parameters are spelled: `_param` asks `_reassigned`
    // whether each is written, and it held the previous method's answer.
    _reassigned = _assignedIn(method.body);
    _cellLocals = {};
    final params = method.params.map((p) => _param(p, owned: false)).join(', ');
    final async = method.isAsync && stubbed == null;
    if (method.isAsync) {
      if (stubbed != null) {
        _line(
          '${_vis(method.name)}fn ${snake(method.name)}${_generics(method)}($params) -> ${_futureOf(method)} {',
        );
        _indent++;
        _line('panic!("dart2rust: not translated: ${_stubText(stubbed)}")');
        _indent--;
        _line('}');
        _line('');
        return;
      }
      _emitAsyncWrapper(
        method,
        '${_vis(method.name)}fn ${snake(method.name)}${_generics(method)}($params) -> ${_futureOf(method)}',
        '${snake(method.name)}__body',
        turbofish: method.typeParameters.isEmpty
            ? ''
            : '::<${method.typeParameters.join(', ')}>',
      );
      _line('');
    }
    _line(
      '${_vis(method.name)}${async ? 'async ' : ''}fn '
      '${async ? '${snake(method.name)}__body' : snake(method.name)}${_generics(method)}'
      '($params) -> ${_returnType(method)} {',
    );
    _indent++;
    final saved = _selfName;
    // There is no receiver. Anything in the body that wanted one is a bug in
    // the front end, not something to paper over here.
    _selfName = '<no self>';
    _returns = method.returnType;
    _here = '${cls.name}.${method.name}';
    // The Rust return type too: a `try` body that returns carries
    // `Option<..>` of it out of its closure, and without it `_isLoopback`'s
    // `return address.isLoopback` came out as an `Option<()>`.
    _rustReturns = _returnType(method);
    _failure = _failureOf(method);
    _asyncBody = method.isAsync;
    _methodTypeParams = method.typeParameters;
    if (stubbed != null) {
      _line('panic!("dart2rust: not translated: ${_stubText(stubbed)}")');
    } else {
      _body(
        method.body,
        method.isAsync ? _awaited(method.returnType) : method.returnType,
      );
      _closeOpenIf(method.body);
    }
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

  /// A generic method's type parameters read as `Object` (the erased
  /// twin's view; see the prelude's `CastErased`).
  Map<String, IrType> _erasure(IrMethod method) => {
    for (final p in method.typeParameters) p: const IrType('Object'),
  };

  String _erasedSignature(IrMethod method) {
    final erasure = _erasure(method);
    final params = [
      if (!method.isStatic) _sharedMutation(method) ? '&mut self' : '&self',
      ...method.params.map(
        (p) => _param(
          IrParam(
            p.name,
            _substituteType(p.type, erasure),
            named: p.named,
            hasDefault: p.hasDefault,
            kept: p.kept,
          ),
          owned: false,
        ),
      ),
    ].join(', ');
    final returns = _substituteType(method.returnType, erasure);
    final spelled = method.isAsync
        ? 'DartFuture<${type(_awaited(returns))}>'
        : _spelledReturn(type(returns));
    // The class's own parameters bounded as the trait's defaults bound
    // them (`_traitWhere`): the super function the default body reaches
    // asks `V: Clone` (`CanonicalizedMap.cast__erased`, ws483).
    final clauses = [
      for (final p in cls.typeParameters) '$p: Clone${_nb(cls)}',
    ];
    final where = clauses.isEmpty ? '' : ' where ${clauses.join(', ')}';
    return 'fn ${_methodName(method)}__erased($params) -> ${_wrapped(spelled)}$where';
  }

  /// The erased twin of a generic trait method, in the trait: declared
  /// beside a required method, with the super function's body (its type
  /// parameters `Rc<dyn Object>`) beside a default one. Object-safe, so
  /// a `dyn` receiver reaches the method through it.
  void _emitErasedTwin(IrMethod method, {required bool defaultBody}) {
    if (method.typeParameters.isEmpty || method.isStatic) return;
    if (!defaultBody) {
      _line('${_erasedSignature(method)};');
      _line('');
      return;
    }
    _line('${_erasedSignature(method)} {');
    _indent++;
    final erased = method.typeParameters
        .map((_) => 'std::rc::Rc<dyn Object>')
        .join(', ');
    final spelled =
        '::<Self${[...cls.typeParameters].map((p) => ', $p').join()}, $erased>';
    final call =
        '${superFn(cls.name, method.name, isSetter: method.isSetter)}$spelled('
        '${['self', ...method.params.map((p) => snake(p.name))].join(', ')})';
    _line(method.isAsync && _resultModel ? 'Ok($call)' : call);
    _indent--;
    _line('}');
    _line('');
  }

  /// The erased twin in an implementer: through the class's own generic
  /// version, at `Rc<dyn Object>`.
  void _emitErasedImplTwin(IrMethod need, String trait) {
    if (need.typeParameters.isEmpty || need.isStatic) return;
    _line('${_erasedSignature(need)} {');
    _indent++;
    final erased = need.typeParameters
        .map((_) => 'std::rc::Rc<dyn Object>')
        .join(', ');
    _line(
      '<Self as $trait${_traitArgsOf(trait)}>::${_methodName(need)}::<$erased>'
      '(${['self', ...need.params.map((p) => snake(p.name))].join(', ')})',
    );
    _indent--;
    _line('}');
    _line('');
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
  /// A trait default method's `where`: `Self: Sized` when it needs it,
  /// and `T: Clone` for the class's parameters, which the super function
  /// holding its body asks for (see the trait header).
  String _traitWhere(IrMethod method) {
    final clauses = [
      if (_sizedBound(method).isNotEmpty) 'Self: Sized',
      for (final p in cls.typeParameters) '$p: Clone${_nb(cls)}',
    ];
    return clauses.isEmpty ? '' : ' where ${clauses.join(', ')}';
  }

  static String _sizedBound(IrMethod method) =>
      method.typeParameters.isEmpty ? '' : ' where Self: Sized';

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

  /// Whether a super fn's body is being printed: `this` is a `&__Self`
  /// there, and so is `this` inside a closure of it, whose `_selfName` is
  /// the handle (`<Self as RendererBinding>` in `initMouseTracker`'s
  /// closure, E0411, run459).
  var _inSuperFn = false;

  void _emitSuperFn(IrMethod method) {
    final wasSuperFn = _inSuperFn;
    _inSuperFn = true;
    try {
      _emitSuperFnBody(method);
    } finally {
      _inSuperFn = wasSuperFn;
    }
  }

  void _emitSuperFnBody(IrMethod method) {
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
      // ..and by every trait a `super` call inside reaches that this
      // class is not below: a mixin's `super.initInstances()` dispatches
      // to the previous mixin of the application (`_realOwner`), which
      // its `on` clause never named (`SchedulerBinding`'s reaching
      // `GestureBinding`'s, 3 stubs on the start path at run448).
      final reached = _WalkSelf()..statement(method.body);
      final superBounds = [
        for (final MapEntry(key: base, value: arguments)
            in reached.superBases.entries)
          if (base != cls.name &&
              base != 'Object' &&
              _world.isTrait(base) &&
              !_world.isBelow(cls.name, base))
            ' + $base${arguments.isEmpty ? _traitArgsOf(base) : '<${arguments.map(type).join(', ')}>'}',
      ].join();
      final generics =
          '<__Self: ${cls.name}${_generics(cls)}$superBounds + ?Sized + \'static'
          '${cls.typeParameters.isEmpty ? '' : ', ${cls.typeParameters.map((p) => "$p: Clone${_nb(cls)} + 'static").join(', ')}'}'
          '${method.typeParameters.isEmpty ? '' : ', ${method.typeParameters.map((p) => "$p: Clone${_nbm(method)} + 'static").join(', ')}'}'
          '>';
      final name = superFn(cls.name, method.name, isSetter: method.isSetter);
      if (method.isAsync) {
        // The wrapper holds the object through the trait's own handle
        // (`dart_self_<trait>()`, an `Rc<dyn Trait>`), and the body runs
        // on that: `__Self` there is the trait object.
        _emitAsyncWrapper(
          method,
          '${_vis(cls.name)}fn $name$generics($params) -> ${_futureOf(method)}',
          '${name}__body',
          receiver: (
            'let __self = this_.dart_self_${snakeRaw(cls.name)}();',
            '&*__self',
          ),
          turbofish:
              '::<_${cls.typeParameters.isEmpty ? '' : ', ${cls.typeParameters.join(', ')}'}${method.typeParameters.isEmpty ? '' : ', ${method.typeParameters.join(', ')}'}>',
        );
        _line('');
      }
      _line(
        '${_vis(cls.name)}${method.isAsync ? 'async ' : ''}fn '
        '${method.isAsync ? '${name}__body' : name}'
        '$generics($params) -> '
        // An `async fn` returns the awaited type. A boxed future returned by
        // a non-async one borrows `this_`: `+ '_`.
        '${_lifetimed(_returnType(method))} {',
      );
      _indent++;
      _selfName = 'this_';
      _returns = method.returnType;
      // ..and the Rust spelling, which a `try` that returns from inside
      // carries out through its closure (`Option<()>` carried an
      // `Rc<dyn Element>` in `inflateWidget`'s super function, ws475).
      final outerRustReturns = _rustReturns;
      _rustReturns = _returnType(method);
      _here = '${cls.name}.${method.name}';
      // A super function fails like the method whose body it holds.
      _failure = _failureOf(method);
      _asyncBody = method.isAsync;
      _methodTypeParams = method.typeParameters;
      _reassigned = _assignedIn(method.body);
      _cellLocals = {};
      // `this_` is a `&__Self: Trait`, and a trait has no fields: the base's
      // fields are its accessor methods here, as they are inside the trait
      // itself. `this_.start` was read as a field 6 times in `source_span`.
      final accessors = _fieldsAreAccessors;
      _fieldsAreAccessors = true;
      _body(
        method.body,
        method.isAsync ? _awaited(method.returnType) : method.returnType,
      );
      _closeOpenIf(method.body);
      _fieldsAreAccessors = accessors;
      _rustReturns = outerRustReturns;
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

  /// Crate-wide, not this file's: the trait's declaration and an impl in
  /// another file must agree on `&mut self`, and each was deciding from
  /// the implementers it could see (`DirectionalFocusTraversalPolicyMixin
  /// .inDirection`: `&mut` in `focus_traversal`, `&self` in `radio_group`,
  /// 4 errors outside any function at ws373).
  List<IrClass> _implementersOf(String trait) {
    final cache = _implementersCache[library] ??= {};
    return cache[trait] ??= [
      for (final c in {
        for (final c in library.elsewhere.values) c.name: c,
        for (final c in library.classes) c.name: c,
      }.values)
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
        IrFieldDecl.substituted(
          f,
          (t) => _substituteType(t, bound),
          initial: bound.isEmpty
              ? null
              : (e) => _substitute(e, const {}, bound),
        ),
      // A mixin's fields arrive with the class's own: the front end lowers
      // the anonymous application classes' members into it (ws112). Adding
      // them here as well declared `PointerEvent`'s fields twice (845
      // struct errors the round the gesture crates became reachable).
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
  IrType _substituteType(IrType t, Map<String, IrType> bound) {
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
      // ..but a *projected* `T?` stays projected over `dynamic` too: the
      // trait's side normalises `<Rc<dyn Object> as DartNullable>::Or` to
      // `Option<Rc<dyn Object>>`, and the impl has to say the same.
      if (to.name == 'dynamic' && !t.projected) return to;
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
      // ..and now it does collapse, as Dart does: a signature's `T?` is
      // `<T as DartNullable>::Or` (`IrType.projected`), which *is* `X?`
      // for `T = X?`. Put in for another parameter it stays projected.
      // A projected slot stays projected over what is put in: an impl's
      // signature has to spell the trait's `<X as DartNullable>::Or`.
      // ..unless what is put in is nullable itself: then the projection
      // *is* that `Option`, and rustc normalises the trait's side to it.
      if (to.nullable) return to;
      if (t.projected) {
        return IrType(
          to.name,
          nullable: true,
          arguments: to.arguments,
          projected: true,
        );
      }
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
  String? _failureOf(IrMethod method) => _resultModel ? _error : null;

  String _boxedThrow(IrExpr value) {
    final thrown = expr(value);
    // ..and any constructed error: a `FormatException` thrown where the
    // signature says `Rc<dyn Object>` is boxed into it too.
    // ..and a prelude exception made by its constructor function
    // (`Exception::new(..)` is a static call, not an `IrNew`; 29 at ws325).
    const preludeExceptions = _preludeClasses;
    final boxed =
        (_failure == 'Object' || _failure == 'std::rc::Rc<dyn Object>') &&
        ((value is IrLiteral && value.type.name == 'String') ||
            value is IrNew ||
            value is IrConstInstance ||
            (value is IrStaticCall &&
                value.owner != null &&
                preludeExceptions.contains(value.owner)));
    // ..spelled as the error type: inside an `async` block the `Err` has
    // no signature to infer from, and `Rc<Exception>` was the block's
    // whole error type (`initServiceExtensions`, ws479).
    final asError =
        _failure == 'Object' || _failure == 'std::rc::Rc<dyn Object>';
    if (boxed) {
      return asError
          ? '(std::rc::Rc::new($thrown) as std::rc::Rc<dyn Object>)'
          : 'std::rc::Rc::new($thrown)';
    }
    final valueType = value.rustType;
    if (asError &&
        valueType != null &&
        valueType.name != 'Object' &&
        valueType.name != 'dynamic' &&
        !valueType.nullable &&
        !valueType.isFunction &&
        (library[valueType.name] != null ||
            _preludeClasses.contains(valueType.name))) {
      return '(${_handleOf(value)} as std::rc::Rc<dyn Object>)';
    }
    return thrown;
  }

  /// Free functions the prelude provides, which the front end calls by name
  /// and no library declares: the crate-wide "was it translated" check has
  /// to know them, or `vec_of_nones(..)` reads as a call to nothing.
  static const _preludeFunctions = {
    'dart_null_object',
    'dart_function_object',
    'dart_call_function',
    'dart_function_same',
    'dart_from_dynamic',
    'vec_of_nulls',
    'dart_native',
    'dart_native_as',
    'future_ready',
    'dart_cast_erased',
    'dart_is_kind',
    'uint8_list_sublist_view',
    'byte_data_sublist_view',
    // By their Dart names, as the call names them (`postEvent`, not the
    // `post_event` it is spelled as).
    'exit',
    'dart_null_as',
    'postEvent',
    'registerExtension',
    'EnumName_get_name',
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

  /// The prelude's classes that `is` can ask about and a `throw` boxes.
  static const _preludeClasses = {
    'Exception',
    'FormatException',
    'StateError',
    'ArgumentError',
    'RangeError',
    'UnsupportedError',
    'UnimplementedError',
    'ConcurrentModificationError',
    'TypeError',
    'AssertionError',
    'Error',
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
  /// What a class puts in for its base's type parameters, from the
  /// `superclassArguments` of the class the chain started at, composed
  /// down the chain.
  Map<String, IrType> _baseTypes(IrClass from, Map<String, IrType> types) {
    final baseName = from.superclass;
    final base = baseName == null ? null : library[baseName];
    if (base == null) return const {};
    return {
      for (
        var i = 0;
        i < base.typeParameters.length && i < from.superclassArguments.length;
        i++
      )
        base.typeParameters[i]: _substituteType(
          from.superclassArguments[i],
          types,
        ),
    };
  }

  /// The bodies a constructor runs on the way in: its base's chain, from
  /// the deepest base down to the direct one, each with the constructor it
  /// reaches and the arguments the `super(..)` passes. A generic base is
  /// left out: its body names the base's `T`, which this constructor
  /// cannot (it stays what it was: not run).
  List<(IrClass, IrConstructor, List<IrExpr>)> _inheritedBodies(
    IrConstructor ctor, [
    IrClass? from,
  ]) {
    final baseName = ctor.superBase;
    if (baseName == null) return const [];
    final base = library[baseName];
    if (base == null || base.typeParameters.isNotEmpty) return const [];
    final baseCtors = base.constructors
        .where((c) => c.name == ctor.superName)
        .toList();
    if (baseCtors.length != 1) return const [];
    final baseCtor = baseCtors.single;
    if (baseCtor.params.length != ctor.superArgs.length) return const [];
    // A bodiless base too: its parameters are what the next base's
    // arguments name (`RenderProxyBoxWithHitTestBehavior({child}) :
    // super(child)`, whose `child` was never bound, ws523).
    return [
      ..._inheritedBodies(baseCtor, base),
      (base, baseCtor, ctor.superArgs),
    ];
  }

  List<IrStmt> _inheritedPre(
    IrConstructor ctor, [
    IrClass? from,
    Map<String, IrType> types = const {},
  ]) {
    final baseName = ctor.superBase;
    if (baseName == null) return const [];
    final base = library[baseName];
    if (base == null) return const [];
    final here = from ?? cls;
    final binding = _baseTypes(here, types);
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
      ..._inheritedPre(baseCtor, base, binding),
      for (final s in baseCtor.pre)
        if (s is IrLocalDecl)
          IrLocalDecl(
            _baseTempName(base, s.name),
            // A parameter's binding is typed by the parameter: `let
            // configuration = None` inferred nothing (E0282, ws461).
            s.type ??
                baseCtor.params
                    .where((p) => p.name == s.name)
                    .map((p) => _substituteType(p.type, binding))
                    .firstOrNull,
            s.init == null ? null : _substitute(s.init!, substitution, binding),
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

  Map<String, IrExpr> _inheritedInits(
    IrConstructor ctor, [
    IrClass? from,
    Map<String, IrType> types = const {},
  ]) {
    final baseName = ctor.superBase;
    if (baseName == null) return const {};
    final base = library[baseName];
    final binding = _baseTypes(from ?? cls, types);
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
    var substitution = <String, IrExpr>{
      for (var i = 0; i < baseCtor.params.length; i++)
        baseCtor.params[i].name: ctor.superArgs[i],
      ..._baseTempRenames(base, baseCtor),
    };
    // Through the base's redirects: `_SemanticsBase()` is `this.
    // fromProperties(.., properties: SemanticsProperties(..))`, and the
    // field initialisers are the target's, in terms of its parameters,
    // which the redirect's arguments fill (`Semantics.new`, 119 callers,
    // "field never initialised: properties").
    var reached = baseCtor;
    while (reached.redirectTo != null) {
      final targetName = reached.redirectTo!.isEmpty
          ? null
          : reached.redirectTo;
      final targets = base.constructors
          .where((c) => c.name == targetName)
          .toList();
      if (targets.length != 1 ||
          targets.single.params.length != reached.redirectArgs.length) {
        throw Unsupported(
          'super constructor call into `$baseName`, whose constructor '
              'redirects to one this compiler cannot follow',
          'super(...)',
        );
      }
      final target = targets.single;
      final through = substitution;
      substitution = {
        for (var i = 0; i < target.params.length; i++)
          target.params[i].name: _substitute(reached.redirectArgs[i], through),
        ..._baseTempRenames(base, target),
      };
      reached = target;
    }
    final reachedSubstitution = substitution;
    return {
      // The base's own inherited initialisers first, so a chain resolves from
      // the top down and a nearer class can override nothing -- Dart does not
      // let it, and neither does this.
      ..._inheritedInits(reached, base, binding).map(
        (k, v) => MapEntry(k, _substitute(v, reachedSubstitution, binding)),
      ),
      ...reached.fieldInits.map(
        (k, v) => MapEntry(k, _substitute(v, reachedSubstitution, binding)),
      ),
    };
  }

  /// Replaces references to a constructor's parameters with the expressions a
  /// `super(...)` passed for them.
  IrExpr _substitute(
    IrExpr e,
    Map<String, IrExpr> by, [
    Map<String, IrType> types = const {},
  ]) {
    IrExpr go(IrExpr node) => _substitute(node, by, types);
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
      IrCastTo(:final target, :final type) => IrCastTo(
        go(target),
        _substituteType(type, types),
      ),
      IrSuperDispatch(
        :final receiver,
        :final base,
        :final name,
        :final args,
        :final typeArguments,
        :final classArity,
        :final castTo,
      ) =>
        IrSuperDispatch(
          go(receiver),
          base,
          name,
          args.map(go).toList(),
          typeArguments,
          classArity,
          castTo: castTo,
        ),
      IrDowncast(:final target, :final type, :final arguments) => IrDowncast(
        go(target),
        type,
        arguments: [for (final a in arguments) _substituteType(a, types)],
      ),
      IrDynamicDispatch(:final receiver, :final arms) => IrDynamicDispatch(
        go(receiver),
        [for (final (t, b) in arms) (t, go(b))],
      ),
      IrSome(:final value) => IrSome(go(value)),
      // An edge conversion inlined from a base (`_inheritedInits`): the
      // base's `T` is this class's own parameter, renamed, or a concrete
      // type -- and for one of those the conversion is the identity.
      IrNullableOf(:final value, :final parameter, :final toOption) =>
        switch (types[parameter]) {
          null => IrNullableOf(go(value), parameter, toOption: toOption),
          final to
              when to.arguments.isEmpty &&
                  !to.nullable &&
                  cls.typeParameters.contains(to.name) =>
            IrNullableOf(go(value), to.name, toOption: toOption),
          _ => go(value),
        },
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
      IrCall(
        :final target,
        :final name,
        :final args,
        :final qualifier,
        :final receiverClass,
        :final fails,
        :final diverges,
        :final asyncFn,
        :final asyncTarget,
        :final typeArguments,
      ) =>
        IrCall(
          target == null ? null : go(target),
          name,
          args.map(go).toList(),
          qualifier: qualifier,
          receiverClass: receiverClass,
          fails: fails,
          diverges: diverges,
          asyncFn: asyncFn,
          asyncTarget: asyncTarget,
          typeArguments: typeArguments,
        ),
      IrStaticCall(
        :final owner,
        :final name,
        :final args,
        :final fails,
        :final diverges,
        :final asyncFn,
        :final typeArguments,
        :final module,
      ) =>
        IrStaticCall(
          owner,
          name,
          args.map(go).toList(),
          fails: fails,
          diverges: diverges,
          asyncFn: asyncFn,
          typeArguments: typeArguments,
          module: module,
        ),
      IrNew(:final type, :final args, :final constructor) => IrNew(
        type,
        args.map(go).toList(),
        constructor: constructor,
      ),
      IrConditional(:final condition, :final then, :final otherwise) =>
        IrConditional(go(condition), go(then), go(otherwise)),
      IrSuperCall(
        :final base,
        :final name,
        :final args,
        :final isSetter,
        :final baseArguments,
        :final typeArguments,
      ) =>
        IrSuperCall(
          base,
          name,
          args.map(go).toList(),
          isSetter: isSetter,
          baseArguments: baseArguments,
          typeArguments: typeArguments,
        ),
      IrAwait(:final operand) => IrAwait(go(operand)),
      IrUpcast(:final value, :final type, :final handle, :final explicit) =>
        IrUpcast(go(value), type, handle: handle, explicit: explicit),
      IrMapElements(:final collection, :final kind, :final body) =>
        IrMapElements(go(collection), kind, go(body)),
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
  /// Every function returns `Result<T, _error>` (STATUS, 决定 2026-09-04,
  /// 修正): a Dart exception is an object, and one type for them all is
  /// what lets `?` propagate through every call form alike.
  static const _resultModel = true;
  static const _error = 'std::rc::Rc<dyn Object>';

  Map<String, String> _computeFailing() {
    // The uniform model needs no per-class fixed point: every method fails.
    if (_resultModel || !_resultModel) return const {};
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
    if (error == null) return value;
    // `Result<!, E>` is unstable; `Infallible` says the same.
    final inner = value == '!' ? 'std::convert::Infallible' : value;
    return 'Result<$inner, ${type(IrType(error))}>';
  }

  /// A boxed future in return position may borrow the receiver: `+ '_`.
  /// A trait's async method is `fn f(&self) -> Pin<Box<dyn Future<..> +
  /// '_>>`, the manual spelling of `async fn` in a trait, and the default
  /// that boxes `super_fn(self, ..)` needs exactly that lifetime.
  /// A return type as a signature spells it: `!` for `Never` (the general
  /// spelling `Infallible` is for value positions), and a boxed future's
  /// receiver lifetime.
  /// `Result<T, E>` around a rendered return type.
  /// The `DartFuture<T>` an `async` function returns: `T` the awaited
  /// type, `()` for a `void` one.
  String _futureOf(IrMethod method) =>
      'DartFuture<${type(_awaited(method.returnType))}>';

  /// An `async` function is emitted twice: its body as a private `async
  /// fn` (`name__body`, the lazy borrowing future Rust makes), and under
  /// its own name a plain function that clones the receiver's handle,
  /// moves the arguments, and spawns the body on the scheduler --
  /// Dart's future: eager, shared, `'static`, a value (`DartFuture`).
  /// `receiver` is the handle binding (`let __self = ..;`) and the
  /// argument that reaches the body, or null for a function with none.
  void _emitAsyncWrapper(
    IrMethod method,
    String signature,
    String bodyName, {
    (String, String)? receiver,
    String turbofish = '',
  }) {
    _line('$signature {');
    _indent++;
    if (receiver != null) _line(receiver.$1);
    final args = [
      if (receiver != null) receiver.$2,
      ...method.params.map((p) => snake(p.name)),
    ].join(', ');
    _line(
      'DartFuture::spawn_named("${cls.name}.${method.name}", std::boxed::Box::pin(async move { $bodyName$turbofish($args).await }))',
    );
    _indent--;
    _line('}');
  }

  static String _wrapped(String rendered) => !_resultModel
      ? rendered
      // `Result<!, E>`: the never type is unstable as a type argument, and
      // `Infallible` is the stable spelling of a value that cannot be.
      : 'Result<${rendered == '!' ? 'std::convert::Infallible' : rendered}, $_error>';

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
    final copyable =
        !cls.counted && _allFields(cls).every((f) => _isCopy(_fieldType(f)));
    // `Debug` and `PartialEq` cannot be derived over a function-typed field
    // (a `dyn Fn` is neither), and a struct holding one got 15 `E0369`s and
    // 14 `E0277`s for the derive alone. Left off there: a `==` on such a
    // class is then an error at the use, which says what it is.
    // Nested too: a `Vec<Option<Rc<dyn Fn()>>>` field cannot be printed.
    final printable = _allFields(cls).every(
      (f) =>
          !f.type.isFunction &&
          !_fieldType(f).contains('dyn Fn') &&
          // A boxed future prints and compares no better than a closure.
          !_fieldType(f).contains('dyn std::future::Future'),
    );
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
    // ..and not over a projected `T?` field: `<T as DartNullable>::Or:
    // PartialEq` is a where clause a derive cannot write.
    final comparable =
        printable &&
        byIdentity.isEmpty &&
        _allFields(cls).every((f) => !f.type.projected) &&
        _allFields(cls)
            .every((f) => _comparableType(_fieldType(f), {cls.name}));
    // A boxed future is not `Clone`, and a struct holding one (an
    // `AssetBundle`'s caches) cannot derive it; its handle, the `Rc` every
    // counted class is passed by, still is. A value class holding one
    // would have to be cloned by value somewhere and is left to say so.
    final cloneable = _cloneable(cls);
    // A struct with a projected `T?` field writes its `Clone` out below:
    // the derive cannot say `<T as DartNullable>::Or: Clone`, and a bound
    // on the struct's own parameters would have to be repeated by every
    // declaration naming it (`_FutureBuilderState<T>` holding an
    // `AsyncSnapshot<T>`, ws403).
    final projecting = _allFields(cls).any((f) => f.type.projected);
    // ..and every *generic* struct's, for the same reason one step removed:
    // a derive over a field holding a projecting struct (`Option<
    // DropdownMenuItem<T>>`) needs that struct's `Clone`, whose bound is
    // `<T as DartNullable>::Or: Clone` -- said on the impl, where nothing
    // has to repeat it.
    final writesClone =
        cloneable && (projecting || cls.typeParameters.isNotEmpty);
    // A derived `Debug` on `ValueKey<T>` holds only for `T: Debug`, and
    // the `Key` trait it implements has `Debug` above it for every `T:
    // Clone + DartNullable<Or: Clone> + 'static` (18 E0277s the moment the type parameters lost
    // their `Debug` bound). A generic class prints as its class instead;
    // `PartialEq` can still be derived, that impl carries its own `T:
    // PartialEq` and no trait asks for it unconditionally.
    final derivesDebug = printable && cls.typeParameters.isEmpty;
    final derives = [
      if (cloneable && !writesClone) 'Clone',
      if (copyable && !writesClone) 'Copy',
      if (derivesDebug) 'Debug',
      if (comparable) 'PartialEq',
    ];
    if (derives.isNotEmpty) _line('#[derive(${derives.join(', ')})]');
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
    // A counted object knows its own handle (`DartSelf`): a trait body's
    // `this` is `self.dart_self_<trait>()`, 117 `&__Self` where an
    // `Rc<dyn X>` was wanted at ws271.
    // `pub`: a constant instance of the class is spelled as a struct
    // literal wherever it is used (`dart_rc(Struct {..})`), other modules
    // included (E0451 in `SemanticsService`, ws432).
    if (cls.counted) _line('pub __self: DartSelf<Self>,');
    // A Dart class can name a type parameter it never stores -- `Tween<T>`
    // holds `begin` and `end` of type `T?`, but plenty do not. Rust will not
    // have an unused parameter, and `PhantomData` is what it offers instead.
    for (final unused in _unusedParameters(cls)) {
      _line(
        'pub _phantom_${snake(unused)}: '
        'std::marker::PhantomData<$unused>,',
      );
    }
    _indent--;
    _line('}');
    _line('');
    if (writesClone) {
      _line(
        'impl${_implGenerics(cls, keyed: false)} Clone for ${cls.name}${_generics(cls)} {',
      );
      _indent++;
      final copied = [
        for (final f in _allFields(cls))
          '${snake(f.name)}: self.${snake(f.name)}.clone()',
        if (cls.counted) '__self: self.__self.clone()',
        for (final unused in _unusedParameters(cls))
          '_phantom_${snake(unused)}: std::marker::PhantomData',
      ];
      _line('fn clone(&self) -> Self { ${cls.name} { ${copied.join(', ')} } }');
      _indent--;
      _line('}');
      _line('');
      // `Copy` alongside, under the same bound: a derived one asks `Clone`
      // of every `T: Copy`, which the impl above does not give.
      if (copyable) {
        final generics = _implGenerics(
          cls,
          keyed: false,
        ).replaceAll("'static", "'static + Copy");
        _line('impl$generics Copy for ${cls.name}${_generics(cls)} {}');
        _line('');
      }
    }
    if (cls.counted) {
      _line(
        'impl${_implGenerics(cls, keyed: false)} DartSelfRef for ${cls.name}${_generics(cls)} {',
      );
      _indent++;
      _line('fn dart_self_ref(&self) -> &DartSelf<Self> { &self.__self }');
      _indent--;
      _line('}');
      _line('');
    }
    // A struct holding a closure still has to print -- `Rc<DynamicColor>`
    // in a struct that derives `Debug` -- so it prints as its class.
    if (!derivesDebug) {
      // The struct's own bounds, not the impl's: `_MapEntry` holds a
      // `MapEquality<Rc<dyn Object>, ..>` and derives `Debug` over it, and
      // `dyn Object` is no `Hash` -- the key bound the methods need is
      // theirs alone.
      _line(
        'impl${_generics(cls, static: true)} std::fmt::Debug for ${cls.name}${_generics(cls)} {',
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
      final projected = {
        for (final f in _allFields(cls))
          if (f.type.projected)
            type(IrType(f.type.name, arguments: f.type.arguments)),
      };
      final bounds = cls.typeParameters.isEmpty
          ? ''
          : ' where ${[for (final p in cls.typeParameters) '$p: PartialEq', for (final p in projected) '<$p as DartNullable>::Or: PartialEq'].join(', ')}';
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

    // Constructors and constants in a block of their own when the methods
    // carry a key bound (`T: PartialEq`, `_implGenerics`): an
    // `ObserverList<Rc<dyn Fn()>>` can then be *made* wherever it is held,
    // and only the methods comparing its items are out of reach.
    final keyed = _implGenerics(cls);
    final unkeyed = _implGenerics(cls, keyed: false);
    _line('impl$unkeyed ${cls.name}${_generics(cls)} {');
    _indent++;
    _emitConstructors();
    if (!_freeStatics(cls.name)) _emitConstants();
    if (keyed != unkeyed) {
      _indent--;
      _line('}');
      _line('');
      _line('impl$keyed ${cls.name}${_generics(cls)} {');
      _indent++;
    }
    _emitMethods();
    _emitToList();
    _indent--;
    _line('}');
    if (_freeStatics(cls.name)) {
      // A generic class's statics and constants at module level, named
      // with the class, as an abstract class's are (`_freeStatics`).
      _line('');
      _emitConstants(prefix: cls.name);
      for (final method in cls.methods) {
        if (!method.isStatic || method.operator != null) continue;
        _member(
          '${cls.name}.${method.name} (static)',
          () => _emitMethod(
            method,
            as: _abstractStaticName(cls.name, method.name),
          ),
        );
      }
    }
    // One line per struct rather than one blanket impl over everything: see
    // `DartAny` in the prelude for why the blanket one is quietly wrong.
    _line('');
    _emitDartNullable();
    // `DartEq`, by the `PartialEq` the struct has -- derived, or the
    // manual one above with its bounds -- and by identity when it has none.
    // A generic struct compares field by field through `DartEq`, which
    // every field type has (the `T: DartEq` bound is the struct's own); a
    // `T: PartialEq` bound shut out every `ObserverList<VoidCallback>`
    // (48 at ws445).
    String fieldWise() {
      final parts = [
        for (final f in _allFields(cls))
          'self.${snake(f.name)}.dart_eq(&other.${snake(f.name)})',
      ];
      return parts.isEmpty ? 'true' : parts.join(' && ');
    }

    if (comparable || byIdentity.isNotEmpty) {
      _emitDartEq(
        body: cls.typeParameters.isEmpty ? 'self == other' : fieldWise(),
      );
    } else {
      _emitDartEq(body: 'std::ptr::eq(self, other)');
    }
    _line(
      // The bounds the inherent impl has: `dart_cast` calls the trait
      // impls, whose `E: Clone` a bare `'static` cannot meet (ws304).
      'impl${_implGenerics(cls, keyed: false)} DartAny for '
      '${cls.name}${_generics(cls)} {',
    );
    _indent++;
    _line('fn dart_runtime_type(&self) -> Type {');
    _indent++;
    _line('Type { name: "${cls.name}" }');
    _indent--;
    _line('}');
    // What this object is (`dart_cast_to`): its own struct, and every
    // trait it has an impl for, each through the handle that impl keeps.
    _line(
      'fn dart_cast(&self, __t: std::any::TypeId) -> Option<std::boxed::Box<dyn std::any::Any>> {',
    );
    _indent++;
    final own = cls.counted
        ? 'self.dart_self_ref().get()'
        : cls.typeParameters.isEmpty && _cloneable(cls)
        ? 'std::rc::Rc::new(self.clone())'
        : null;
    // ..and for the handle type itself (`dart_cast_any`): a type parameter
    // is instantiated with `Rc<ScaffoldState>`, not `ScaffoldState`.
    if (own != null) {
      _line(
        'if __t == std::any::TypeId::of::<Self>() || __t == std::any::TypeId::of::<std::rc::Rc<Self>>() { return Some(std::boxed::Box::new($own)); }',
      );
    }
    for (final above in _abstractAncestors(cls)) {
      final arguments = _baseArguments(above);
      if (arguments == null) continue;
      final handle = cls.extraImpls.any((w) => w.name == above.name)
          ? '<Self as ${above.name}$arguments>::dart_self_${snakeRaw(above.name)}(self)'
          : 'self.dart_self_${snakeRaw(above.name)}()';
      _line(
        'if __t == std::any::TypeId::of::<dyn ${above.name}$arguments>() || __t == std::any::TypeId::of::<std::rc::Rc<dyn ${above.name}$arguments>>() { return Some(std::boxed::Box::new($handle)); }',
      );
    }
    for (final wider in cls.extraImpls) {
      final arguments = '<${wider.arguments.map((a) => type(a)).join(', ')}>';
      _line(
        'if __t == std::any::TypeId::of::<dyn ${wider.name}$arguments>() || __t == std::any::TypeId::of::<std::rc::Rc<dyn ${wider.name}$arguments>>() { return Some(std::boxed::Box::new(<Self as ${wider.name}$arguments>::dart_self_${snakeRaw(wider.name)}(self))); }',
      );
    }
    _line('None');
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
  /// The prelude's traits for `dart:core` interfaces, with the methods each
  /// asks for: the impl forwards to the class's own.
  static const _preludeInterfaces = {
    'Comparable': [
      'compare_to(&self, other: __A0) -> i64',
      'compare_to(other)',
    ],
    'DartIterator': [
      'move_next(&self) -> bool',
      'move_next()',
      'current(&self) -> __A0',
      'current()',
    ],
  };

  void _emitPreludeInterfaces() {
    for (final i in cls.interfaces) {
      final methods = _preludeInterfaces[i.name];
      if (methods == null) continue;
      final args = i.arguments.map((a) => type(a)).toList();
      final generic = args.isEmpty ? '' : '<${args.join(', ')}>';
      _member('impl ${i.name} for ${cls.name}', () {
        _line(
          'impl${_implGenerics(cls)} ${i.name}$generic for '
          '${cls.name}${_generics(cls)} {',
        );
        _indent++;
        for (var k = 0; k + 1 < methods.length; k += 2) {
          final signature = methods[k].replaceAll(
            '__A0',
            args.isEmpty ? 'std::rc::Rc<dyn Object>' : args[0],
          );
          _line('fn $signature {');
          _indent++;
          // The class's own method returns `Result`; the prelude trait's
          // signature is fixed.
          _line('self.${methods[k + 1]}${_resultModel ? '.unwrap()' : ''}');
          _indent--;
          _line('}');
        }
        _indent--;
        _line('}');
        _line('');
      });
    }
  }

  void _emitBaseImpl() {
    _emitPreludeInterfaces();
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
    // The wider instantiations the program names (`IrClass.extraImpls`):
    // an impl each, its signatures in the wider terms, forwarding to the
    // class's own methods through the coercion rule.
    for (final wider in cls.extraImpls) {
      final base = library[wider.name];
      if (base == null) continue;
      _member(
        'impl ${wider.name}<${wider.arguments.join(', ')}> for ${cls.name}',
        () => _emitImplFor(base, passedOverride: wider.arguments),
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
    return _argumentsThrough(cls, const {}, base, {});
  }

  /// Walk up from `current`, carrying the arguments through each step.
  /// `_Linear extends Curve` and `Curve extends ParametricCurve<double>`, so
  /// reaching ParametricCurve means going through Curve -- and a Curve that
  /// had parameters of its own would need ours substituted into what it
  /// passes on. A mixin or interface is a direct base of whichever class
  /// named it, so its arguments are read off the `with`/`implements` clause
  /// -- and followed through: `_SelectableFragment with Selectable`, where
  /// `Selectable implements SelectionHandler` and that `extends
  /// ValueListenable<SelectionGeometry>` (the impl was refused as "arguments
  /// not known here" while the `Selectable` trait required it, 2 E0277).
  List<IrType>? _argumentsThrough(
    IrClass current,
    Map<String, IrType> bound,
    IrClass base,
    Set<String> seen,
  ) {
    if (!seen.add(current.name)) return null;
    Map<String, IrType> binding(IrClass of, List<IrType> passed) => {
      for (var i = 0; i < passed.length; i++) of.typeParameters[i]: passed[i],
    };
    for (final mixin in [...current.mixins, ...current.interfaces]) {
      // Substituted *inside* the argument, not only when the argument is a
      // bare parameter: `FormFieldState<T> extends State<FormField<T>>`
      // passes `FormField<T>`, and `bound[a.name]` left that `T` standing --
      // 28 `E0747`s reading it as a constant.
      final passed = [
        for (final a in mixin.arguments) _substituteType(a, bound),
      ];
      if (mixin.name == base.name) {
        return passed.length == base.typeParameters.length ? passed : null;
      }
      final via = library[mixin.name];
      if (via == null || passed.length != via.typeParameters.length) continue;
      final found = _argumentsThrough(via, binding(via, passed), base, seen);
      if (found != null) return found;
    }
    final next = library[current.superclass];
    if (next == null) return null;
    final passed = [
      for (final a in current.superclassArguments) _substituteType(a, bound),
    ];
    if (next.name == base.name) {
      return passed.length == base.typeParameters.length ? passed : null;
    }
    if (passed.length != next.typeParameters.length) return null;
    return _argumentsThrough(next, binding(next, passed), base, seen);
  }

  /// The trait whose impl block is being printed (`_emitImplFor`).
  String? _implFor;

  void _emitImplFor(IrClass base, {List<IrType>? passedOverride}) {
    _implFor = base.name;
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
    // ..less the ones an abstract supertype of the base declares, which
    // the trait left to that ancestor (see `_emitTrait`) and whose impl
    // block for this class carries them.
    final inheritedByBase = {
      for (final above in _supertypesOf(base))
        if (library.isAbstract(above.name))
          for (final f in above.fields) f.name,
    };
    final accessors = [
      for (final f in ownFields)
        if (!inheritedByBase.contains(f.name)) f,
    ];
    // No early return when both are empty. A Dart subclass *is* its base
    // whether or not it changes anything, so the impl has to exist even with
    // nothing in it -- `Panel extends Measured with Scaled` overrides neither
    // and the mixin has no fields, and without `impl Scaled for Panel {}` the
    // free function holding `Scaled`'s body cannot be called on a `Panel`:
    // "the trait bound `Panel: Scaled` is not satisfied". An empty impl block
    // is the whole statement that it is one.

    final arguments = passedOverride != null
        ? '<${passedOverride.map((a) => type(a)).join(', ')}>'
        : _baseArguments(base);
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
    final passed = passedOverride ?? _baseTypeArguments(base) ?? const [];
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
    // The handle a trait body's `this` is. A struct that is not counted has
    // no identity to give, and a fresh handle around a copy is what the
    // rest of its translation does with it too.
    _line(
      'fn dart_self_${snakeRaw(base.name)}(&self) -> std::rc::Rc<dyn ${base.name}$arguments> {',
    );
    _indent++;
    // ..and a generic value class cannot even be cloned here: its derived
    // `Clone` wants `T: Clone`, which the impl's `T: 'static` does not
    // promise (192 `&X<T>: Trait` bounds at ws275).
    _line(
      cls.counted
          ? 'self.__self.get()'
          : cls.typeParameters.isEmpty && _cloneable(cls)
          ? 'std::rc::Rc::new(self.clone())'
          : 'todo!("${cls.name} has no handle of its own")',
    );
    _indent--;
    _line('}');
    _line('');
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
      // The cell accessor first, before a getter of the class's own can
      // take the value accessor's place: the trait asks for both.
      if (_handsCell(field)) {
        final cell = held.contains(field.name)
            ? _sharedField(field.name)
            : null;
        final substituted = type(_substituteType(field.type, _implBinding));
        _line(
          'fn ${snake(field.name)}_cell(&self) -> ${_wrapped(_cellType(substituted))} {',
        );
        _indent++;
        _line(
          cell != null
              ? (_resultModel
                    ? 'Ok(self.${snake(field.name)}.clone())'
                    : 'self.${snake(field.name)}.clone()')
              : 'todo!("${cls.name}.${field.name} is mutated through a trait but is not a cell")',
        );
        _indent--;
        _line('}');
        _line('');
      }
      if (taken.contains(snake(field.name))) continue;
      // Cloned out: the accessor returns a value and the field is behind
      // `&self` -- `fn _buffer(&self) -> Vec<i64> { self._buffer }` moved it.
      // ..and through the cell when the field is in one (a counted class):
      // `self.parent.clone()` handed out the `Rc<RefCell<..>>` itself.
      final cell = _sharedField(field.name);
      // A `late` field is held as an `Option` and the trait's accessor
      // gives the declared type: the read unwraps (Dart's read of an unset
      // `late` throws; this panics), the write wraps. `RenderObject`'s
      // `late bool _needsCompositing` alone was 363 mismatches in
      // `rendering` (194 `set`, 169 reads).
      final late = field.isLate ? '.unwrap()' : '';
      final reads = held.contains(field.name)
          ? (cell != null
                ? (_lazyLate(field)
                      ? _lazyRead(field, 'self')
                      : _isCopy(_heldType(cell))
                      ? 'self.${snake(field.name)}.get()$late'
                      : 'self.${snake(field.name)}.borrow().clone()$late')
                : _isCopy(type(_substituteType(field.type, _implBinding)))
                ? 'self.${snake(field.name)}$late'
                : 'self.${snake(field.name)}.clone()$late')
          : cls.methods.any((m) => m.name == field.name && !m.isStatic)
          // A getter is a method and returns `Result`; the accessor the
          // trait asks for cannot, and unwraps.
          ? 'self.${snake(field.name)}()${_resultModel ? '?' : ''}'
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
      _line(
        'fn ${snake(field.name)}(&self) -> ${_wrapped(type(substituted))} {',
      );
      _indent++;
      // The field holds one `Option`; a trait asking for the doubled one
      // gets it wrapped -- and the whole in `Ok`.
      // ..and any other difference between this class's field and the
      // trait's -- a `Matrix4` field under a `Matrix4?` accessor
      // (`_TransformedPointerCancelEvent.transform`, 15 at ws463) -- by
      // the one rule, as a method's result is.
      final own = _allFields(cls)
          .where((f) => f.name == field.name)
          .firstOrNull;
      String value;
      if (reads != null && own != null && substituted.name != 'Option') {
        final held = IrLocal('__v')..rustType = own.type;
        final shaped = coerceInto(held, substituted, _world);
        value = identical(shaped, held)
            ? body
            : '{ let __v = $body; ${expr(shaped)} }';
      } else {
        value = substituted.name == 'Option' && reads != null
            ? 'Some($body)'
            : body;
      }
      _line(reads != null && _resultModel ? 'Ok($value)' : value);
      _indent--;
      _line('}');
      _line('');
      // The setter the trait asks for on a mutable field (see `_emitTrait`).
      // Every setter the trait declares, held or not: an impl missing one
      // is "not all trait items implemented", and a whole crate with it
      // (`SnapshotController with ChangeNotifier`, the round the gate opened).
      if (!field.isFinal) {
        final cell = held.contains(field.name)
            ? _sharedField(field.name)
            : null;
        _line(
          'fn set_${snake(field.name)}(&self, value: ${type(substituted)}) -> ${_wrapped('()')} {',
        );
        _indent++;
        if (cell != null) {
          // The trait's view of the field may be wider than this class's
          // (`Tween<T>.begin` as `T?` erased against `ColorTween`'s
          // `Color?`): the value is adapted into what the field holds.
          final own = cell.type;
          final adapted = type(substituted) == type(own)
              ? 'value'
              : expr(
                  coerceInto(
                    IrLocal('value')..rustType = substituted,
                    own,
                    _world,
                    inClosure: true,
                  ),
                );
          final stored = field.isLate ? 'Some($adapted)' : adapted;
          _line(
            _isCopy(_heldType(cell))
                ? 'self.${snake(field.name)}.set($stored);'
                : '*self.${snake(field.name)}.borrow_mut() = $stored;',
          );
          if (_resultModel) _line('Ok(())');
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

  /// The method with each type parameter that shadows one of the class's
  /// renamed `T_` in its signature, or null when none does. The body is
  /// not rewritten: only a forwarder or a stub may use the result.
  IrMethod? _renamedShadowed(IrMethod need) {
    final shadowed = {
      for (final p in need.typeParameters)
        if (cls.typeParameters.contains(p)) p: IrType('${p}_'),
    };
    if (shadowed.isEmpty) return null;
    return IrMethod(
      need.name,
      [
        for (final p in need.params)
          IrParam(
            p.name,
            _substituteType(p.type, shadowed),
            named: p.named,
            hasDefault: p.hasDefault,
            kept: p.kept,
          ),
      ],
      _substituteType(need.returnType, shadowed),
      need.body,
      typeParameters: [
        for (final p in need.typeParameters)
          shadowed.containsKey(p) ? '${p}_' : p,
      ],
      isStatic: need.isStatic,
      isGetter: need.isGetter,
      isSetter: need.isSetter,
      operator: need.operator,
      throws: need.throws,
      doc: need.doc,
      isAsync: need.isAsync,
    );
  }

  void _emitBaseMethod(IrMethod need) {
    {
      // A method type parameter named like one of the class's --
      // `ParentDataElement<T>` implementing `BuildContext.
      // dependOnInheritedWidgetOfExactType<T>` -- is renamed here rather
      // than refused: this forwarder's body is the backend's own line and
      // never spells the parameter, so only the signature has to change
      // (4 "not all trait items implemented" in `widgets`, one per
      // generic `Element`).
      need = _renamedShadowed(need) ?? need;
      // A forwarder has parameters, not locals: the last body's cell locals
      // printed a parameter `child` as `child.borrow()` (7 at ws383).
      _cellLocals = {};
      // ..and the inherent method it reaches spelled with the same renaming,
      // so that its `T?` and the trait's `T_?` compare as one type and not
      // as two the coercion rule converts between (22 at ws411).
      var have = _matching(need);
      if (have != null && have.typeParameters.isNotEmpty) {
        have = _renamedShadowed(have) ?? have;
      }
      String? via;
      if (have == null) {
        final inherited = _inherited(need);
        if (inherited != null) {
          via = inherited.$1.name;
          have = inherited.$2;
          if (have.typeParameters.isNotEmpty) {
            have = _renamedShadowed(have) ?? have;
          }
        }
      }
      // Rust does not collapse `Option<Option<X>>` the way Dart collapses
      // `T?` for a nullable `T`: `MessageCodec<Object?>.decodeMessage` is
      // `-> Option<T>` in the trait and the impl must say `Option<Option<..>>`
      // -- 16 `E0053`s, the "14 members" `_substituteType`'s comment gave up
      // on. Spelled out here, with the body wrapped to match below.
      final returns = _spelledReturn(
        type(_substituteType(need.returnType, _implBinding)),
      );
      final wrappedReturns = _wrapped(returns);
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
        '$wrappedReturns${_sizedBound(need)} {',
      );
      _indent++;
      // A mixin's field is an abstract getter and setter on its trait,
      // and the struct holds the field (flattened from the application):
      // read and written here, as an interface's field is above. 2747
      // `todo!`s at ws345 were these (`_tickerModeNotifier` 198, `_child`
      // 180, `_bucket` 110).
      final field = have == null
          ? _allFields(cls).where((f) => f.name == need.name).firstOrNull
          : null;
      if (field != null &&
          !need.isStatic &&
          (need.isSetter ? need.params.length == 1 : need.params.isEmpty)) {
        final cell = _sharedField(field.name);
        final late = field.isLate ? '.unwrap()' : '';
        final name = snake(field.name);
        if (need.isSetter) {
          if (cell != null) {
            // The trait's type is the erased bound (`Option<Rc<dyn
            // RenderObject>>`), the field's the narrower one (`RenderBox?`):
            // the trait cast narrows on the way in, and an `Option` is
            // taken off or put on (+319 mismatched at ws346).
            final given = _substituteType(
              need.params.single.type,
              _implBinding,
            );
            final held = field.type;
            var value = 'value';
            if (given.name != held.name &&
                library.isAbstract(given.name) &&
                library.isAbstract(held.name) &&
                held.name != 'Object') {
              final target = _dynOf(
                IrType(held.name, arguments: held.arguments),
              );
              value =
                  'value.dart_cast_to::<$target>()${held.nullable ? '' : '.unwrap()'}';
            } else if (held.nullable && !given.nullable) {
              value = 'Some(value)';
            } else if (!held.nullable && given.nullable) {
              value = 'value.unwrap()';
            }
            final stored = field.isLate ? 'Some($value)' : value;
            _line(
              _isCopy(_heldType(cell))
                  ? 'self.$name.set($stored);'
                  : '*self.$name.borrow_mut() = $stored;',
            );
            if (_resultModel) _line('Ok(())');
          } else {
            _line(
              'todo!("${cls.name}.${field.name} is written through a trait but is not a cell")',
            );
          }
        } else {
          final read = cell != null
              ? (_lazyLate(field)
                    ? _lazyRead(field, 'self')
                    : _isCopy(_heldType(cell))
                    ? 'self.$name.get()$late'
                    : 'self.$name.borrow().clone()$late')
              : _isCopy(type(field.type))
              ? 'self.$name$late'
              : 'self.$name.clone()$late';
          // ..and widened on the way out (`_shaped`), as a method's
          // result is.
          // ..and widened on the way out by the one rule (`coerceInto`),
          // as a method's result is.
          final held = IrLocal('__v')..rustType = field.type;
          if (Platform.environment['DART2RUST_TRACE_FWD'] == field.name) {
            stderr.writeln(
              'TRACE_FWD ${cls.name}.${field.name} field=${field.type} need=${need.returnType} for=${_implFor}',
            );
          }
          final shaped = coerceInto(
            held,
            _substituteType(need.returnType, _implBinding),
            _world,
          );
          final value = identical(shaped, held)
              ? read
              : '{ let __v = $read; ${expr(shaped)} }';
          _line(_resultModel ? 'Ok($value)' : value);
        }
      } else if (have == null) {
        // Reported in the output rather than silently skipped: a trait impl
        // missing a method does not compile, and the reader should learn why
        // from the file rather than from rustc.
        _line(
          'todo!("${cls.name} does not translate '
          '${need.operator ?? need.name} yet")',
        );
      } else {
        // ..and an async inherent method is a future the forwarder wraps
        // in `Ok` (49 `Pin<Box<impl Future>>` where `Result<..>` goes).
        final inherent = _inherentCall(have, need, via);
        final call = have.isAsync && _resultModel ? 'Ok($inherent)' : inherent;
        // One `Option` short -- the override narrowed `T?` to `T`, which Dart
        // allows, or the trait's `T?` doubled up above -- is a `Some`.
        // The trait's future carries `+ '_` (see `_lifetimed`); the
        // inherent one is the same future without the spelling.
        // An `Rc<Concrete>` returned where the trait says `Rc<dyn Base>`
        // unsizes on its own at the return (47 `Box<Rc<dyn State>>`s).
        // ..a *value* returned there is put behind a fresh handle (`impl
        // BorderRadiusGeometry for BorderRadius`'s `op_mul`, 79), and a
        // `()` where the trait says `Option<..>` is `None` (`Action.invoke`
        // overridden as `void`, 46).
        // ..all by the one rule (`coerceInto`) inside the `Result`'s `map`.
        // A future is the same future under a lifetime spelling and is
        // left alone.
        final held = IrLocal('__v')..rustType = have.returnType;
        if (Platform.environment['DART2RUST_TRACE_FWD'] == need.name) {
          stderr.writeln(
            'TRACE_FWD ${cls.name}.${need.name} have=${have.returnType} need=${need.returnType} method',
          );
        }
        final needReturns = _substituteType(need.returnType, _implBinding);
        final shaped = have.isAsync
            ? held
            : coerceInto(held, needReturns, _world, inClosure: true);
        // An override may return where the trait returns nothing
        // (`Disposer addListener(..)` over `void addListener(..)` in get's
        // `ListNotifier`): the value is dropped (run489).
        final dropsValue =
            !have.isAsync &&
            type(needReturns) == '()' &&
            type(have.returnType) != '()';
        _line(
          dropsValue
              ? '$call.map(|_| ())'
              : identical(shaped, held)
              ? call
              : '$call.map(|__v| ${expr(shaped)})',
        );
      }
      _indent--;
      _line('}');
      _line('');
      final implFor = _implFor;
      if (implFor != null) _emitErasedImplTwin(need, implFor);
    }
  }

  /// This class's own version of a method the base requires.
  IrMethod? _matching(IrMethod need) {
    for (final method in cls.methods) {
      if (need.operator != null) {
        if (method.operator == need.operator) return method;
      } else if (method.operator == null &&
          method.name == need.name &&
          method.isSetter == need.isSetter) {
        // The getter and the setter share a name: `ValueListenable.value`
        // asked for its getter and got `TextEditingController`'s setter,
        // whose `newValue` "the base has no value for", and the impl
        // block came out without `value` at all.
        return method;
      }
    }
    return null;
  }

  /// How to invoke this class's own version, in Rust's own spelling.
  ///
  /// An operator that became an `impl std::ops::*` is invoked as the operator,
  /// not as a method: that is the whole point of having emitted the trait impl.
  /// The nearest class above this one with a body for `need`: an open
  /// class's `Impl` struct has none of its own (`_implOf`), and a subclass
  /// inherits the base's -- both reach the base trait's default through
  /// `Base::name(self, ..)`. Until ws345 every such method was a
  /// `todo!("X does not translate Y yet")`: 26199 of them, `insert`,
  /// `perform_layout` and `first_child` of `RenderFlexImpl` and all 796
  /// getters of each `GalleryLocalizationsXxImpl` -- compiled, never ran.
  (IrClass, IrMethod)? _inherited(IrMethod need) {
    var above = cls.superclass;
    final seen = <String>{cls.name};
    while (above != null && seen.add(above)) {
      final base = library[above];
      if (base == null || !library.isAbstract(base.name)) return null;
      for (final method in base.methods) {
        if (need.operator != null) {
          if (method.operator == need.operator) return (base, method);
        } else if (method.operator == null &&
            method.name == need.name &&
            method.isSetter == need.isSetter &&
            method.isStatic == need.isStatic) {
          return (base, method);
        }
      }
      above = base.superclass;
    }
    return null;
  }

  String _inherentCall(IrMethod method, [IrMethod? through, String? via]) {
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
        final traitType = _substituteType(from.type, _implBinding);
        final doubled = traitType.name == 'Option';
        final flattened = doubled && traitType.arguments.length == 1
            ? IrType(
                traitType.arguments.single.name,
                nullable: true,
                arguments: traitType.arguments.single.arguments,
              )
            : traitType;
        // The argument as the trait typed it, into the inherent method's
        // parameter, by the one rule (`coerceInto`): a widened override
        // (`equals(Object? e1, ..)` under `Equality<E>.equals(E, ..)`) is
        // shared into `Object`, a covariant one (`RenderClipRect` under
        // `RenderObject`) downcast, an erased bound narrowed to the body's
        // trait, an `Option` put on.
        final IrExpr passed = doubled
            ? (IrLiteral('${snake(from.name)}.flatten()', const IrType('raw'))
                ..rustType = flattened)
            : (IrLocal(from.name)..rustType = flattened);
        return expr(coerceInto(passed, p.type, _world));
      }
      // The override's own default is the value the base "has no value for".
      final fallback = p.defaultValue;
      if (fallback != null) return expr(fallback);
      // A `dynamic` (an `Object?`) has no value as the `Null` object.
      if (p.type.name == 'dynamic' && !p.type.nullable) {
        return 'dart_null_object()';
      }
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
    // An inherent method that takes `self: &Rc<Self>` (`_receiverOf`) is
    // reached from the trait's `&self` through the stored handle (1297
    // "expected `&Rc<X>`, found `&X`" at ws276).
    final receiver = cls.counted && _handles.contains(_rustName(method))
        ? '&self.__self.get()'
        : 'self';
    // A generic method's type parameters go along: the forwarder declares
    // the trait's, and the inherent one it reaches names its own only in
    // its result (`getElementForInheritedWidgetOfExactType<T>()`, 36
    // "cannot infer type of the type parameter `T`" at ws397).
    final generics = through?.typeParameters ?? method.typeParameters;
    final fish = generics.length == method.typeParameters.length
        ? _turbofish([for (final g in generics) IrType(g)])
        : '';
    final call =
        '${via == null ? cls.name : _implementedAs(via)}::$name$fish(${[receiver, ...args].join(', ')})';
    // An inherent method the analysis typed `Never` (`throw
    // UnimplementedError()` for a body) returns `Result<Infallible, E>`;
    // the trait's signature wants its own `T`, which the impossible value
    // maps into (`_UnspecifiedTextScaler.clamp`, ws503).
    if (method.returnType.name == 'Never') {
      return '$call.map(|__never| match __never {})';
    }
    // An `async fn` yields its own future type; the trait wants the boxed
    // one every `Future<T>` is here (`_NativeCodec::get_next_frame(self)`).
    return call;
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
    _here = '${cls.name}.${ctor.name?.isEmpty ?? true ? 'new' : ctor.name}';
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
    final signatureAt = _out.length;
    _line(
      '${_vis(ctor.name ?? cls.name)}'
      '${cls.counted ? '' : constness}fn $name($params) -> ${_wrapped(produces)} {',
    );
    _indent++;
    // A value class registers its cast function as it is first made
    // (`dart_register`); a counted one does so in `dart_rc`.
    if (!cls.counted && constness.isEmpty) _line('dart_register::<Self>();');
    // A constructor fails like any function: its body's value is `Ok`.
    _failure = _resultModel ? _error : null;
    if (ctor.redirectTo == null) _line('Ok({');
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
    // The base constructors' bodies run too, deepest first, before this
    // one's: `BindingBase()` calls `initInstances()` and
    // `initServiceExtensions()` from its body, and no binding subclass
    // ran either until run441.
    final bases = _inheritedBodies(ctor);
    final built = ctor.body != null || deferred.isNotEmpty || bases.isNotEmpty;
    // A counted class is built *inside* its handle: the body's `this`
    // (`_recorder._canvas = this` in `_NativeCanvas`) is then the `Rc`
    // every holder wants, and the fields it writes are cells reached
    // through the handle just the same.
    final handleFirst = built && cls.counted;
    _line(
      !built
          ? (cls.counted ? 'dart_rc(Self {' : 'Self {')
          : handleFirst
          ? 'let __new = dart_rc(Self {'
          : 'let mut __new = Self {',
    );
    _indent++;
    if (cls.counted) _line('__self: DartSelf::new(),');
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
        init = IrLiteral('null', const IrType('Null', nullable: true));
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
      final rendered = field.type.isFunction && init is IrClosure && !init.boxed
          ? 'std::rc::Rc::new(${expr(init)})'
          : expr(init);
      // A `late` field is an `Option` (`_lateField`); one with an
      // initialiser that does not mention `this` starts with it, in
      // `Some` (`ObserverList._set = HashSet<T>()`, run434).
      final value = field.isLate ? 'Some($rendered)' : rendered;
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
        // A lazy one stays absent: its first read fills it (`_lazyRead`).
        if (_lazyLate(field)) continue;
        final value = 'Some(${expr(entry.value)})';
        _line(
          _inCell(field)
              ? (_isCopy(_heldType(field))
                    ? '__new.${snake(field.name)}.set($value);'
                    : '*__new.${snake(field.name)}.borrow_mut() = $value;')
              : '__new.${snake(field.name)} = $value;',
        );
      }
      // Nested, nearest base outermost: a base's parameters are bound
      // from the arguments the class below it passed -- which name that
      // class's own parameters -- so the bindings go downward, one block
      // per base, each shadowing the last; the bodies run on the way back
      // out, deepest first, as Dart runs them. A flat block per base
      // evaluated `super(child)` where no `child` was bound (ws523).
      final chain = bases.reversed.toList();
      for (final (_, baseCtor, superArgs) in chain) {
        _line('{');
        _indent++;
        final assigned = baseCtor.body == null
            ? const <String>{}
            : _assignedIn(baseCtor.body!);
        for (var i = 0; i < baseCtor.params.length; i++) {
          // Typed by the parameter: an unused `None` inferred nothing
          // (`configuration` in `_ReusableRenderView`, E0282 at ws461).
          final p = baseCtor.params[i];
          _line(
            'let ${assigned.contains(p.name) ? 'mut ' : ''}${snake(p.name)}: ${type(_substituteType(p.type, _baseTypes(cls, const {})))} = ${expr(superArgs[i])};',
          );
        }
      }
      for (final (_, baseCtor, _) in bases) {
        if (baseCtor.body != null) {
          final savedReassigned = _reassigned;
          _reassigned = {..._reassigned, ..._assignedIn(baseCtor.body!)};
          stmt(baseCtor.body!);
          _reassigned = savedReassigned;
        }
        _indent--;
        _line('}');
      }
      if (body != null) stmt(body);
      _selfName = saved;
      _line(handleFirst || !cls.counted ? '__new' : 'dart_rc(__new)');
    }
    _line('})');
    _indent--;
    _line('}');
    // A clone reached the body by a road the initialisers' check above
    // does not see (a super constructor's argument, a widened `Duration`):
    // a `const fn` may not call it (38 in `gestures_events` at ws278).
    if (constness.isNotEmpty &&
        _out.sublist(signatureAt + 1).any((l) => l.contains('.clone()'))) {
      _out[signatureAt] = _out[signatureAt].replaceFirst('const fn ', 'fn ');
    }
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
      if (method.isStatic && _freeStatics(cls.name)) continue;
      _member(
        '${cls.name}.${method.name}',
        () => _emitMethod(method),
        stub: (reason) => _emitMethod(method, stubbed: reason),
      );
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
      if (ancestor == null || ancestor.isEnum) break;
      // Past an abstract ancestor, not stopped by it: `_SwitchPainter`
      // extends the abstract `ToggleablePainter`, which extends the
      // concrete `ChangeNotifier`, and `notifyListeners` is the latter's
      // (58 "no method named `notify_listeners`" in `cupertino`).
      if (ancestor.isAbstract) {
        name = ancestor.superclass;
        continue;
      }
      if (_generics(ancestor).isNotEmpty) break;
      out.add(ancestor);
      name = ancestor.superclass;
    }
    return out;
  }

  void _emitMethod(IrMethod method, {String? as, String? stubbed}) {
    {
      // A static `of<T>` inside `ScopedModel<T>`: Rust will not have the
      // name twice (E0403, the two errors outside any body once the
      // widgets crate passed). The signature is renamed and the body,
      // which would need the same rename, is a stub that says so.
      final renamed = _renamedShadowed(method);
      if (renamed != null) {
        method = renamed;
        stubbed ??=
            "a method whose type parameter shadows the class's: "
            '${cls.name}.${method.name}';
      }
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
      final name = as ?? _rustName(method);
      // An `async` method that translated is the body under `name__body`
      // and the spawning wrapper under `name` (`_emitAsyncWrapper`); one
      // that did not is the wrapper alone, panicking.
      final async = method.isAsync && stubbed == null;
      if (method.isAsync) {
        final mutable =
            !method.isStatic && _receiverOf(method).startsWith('&mut');
        final receiver = method.isStatic
            ? null
            : (
                'let ${mutable ? 'mut ' : ''}__self = ${_selfHandle()};',
                _selfIsHandle
                    ? '&__self'
                    : cls.counted
                    ? '&*__self'
                    : mutable
                    ? '&mut __self'
                    : '&__self',
              );
        if (stubbed != null) {
          _line(
            '${_vis(method.name)}fn $name${_generics(method)}($params) -> ${_futureOf(method)} {',
          );
          _indent++;
          _line('panic!("dart2rust: not translated: ${_stubText(stubbed)}")');
          _indent--;
          _line('}');
          _line('');
          return;
        }
        _emitAsyncWrapper(
          method,
          '${_vis(method.name)}fn $name${_generics(method)}($params) -> ${_futureOf(method)}',
          // A free static (an abstract class's) has no `Self` to go through
          // (E0433, 19 at ws465).
          method.isStatic && _freeStatics(cls.name)
              ? '${name}__body'
              : 'Self::${name}__body',
          receiver: receiver,
          turbofish: method.typeParameters.isEmpty
              ? ''
              : '::<${method.typeParameters.join(', ')}>',
        );
        _line('');
      }
      _line(
        '${_vis(method.name)}${async ? "async " : ""}fn '
        '${async ? '${name}__body' : name}${_generics(method)}($params) -> $returns {',
      );
      _indent++;
      _returns = method.returnType;
      _here = '${cls.name}.${method.name}';
      _asyncBody = method.isAsync;
      _methodTypeParams = method.typeParameters;
      // A failing `void` method that falls off its end still has to
      // produce its `Ok(())`: `_validateColorStops` ends in an `if`/`else`
      // that only ever returns `Err`, and the value of that `if` is `()`.
      // An async method's value is the awaited one: `Future<void>` falls
      // off into `Ok(())` too (54 in `widgets`).
      final produced = method.isAsync
          ? _awaited(method.returnType)
          : method.returnType;
      final fallsOff =
          _failure != null &&
          type(produced) == '()' &&
          !_alwaysReturns(method.body);
      if (stubbed != null) {
        _line('panic!("dart2rust: not translated: ${_stubText(stubbed)}")');
      } else {
        stmt(method.body, tail: !fallsOff);
        if (fallsOff) _line('Ok(())');
      }
      // `TileMode` to text as an `if`/`else if` chain over every variant with
      // no final `else`: Dart lets the body fall off the end (returning null
      // it would then refuse at runtime); Rust wants the last `if` to be an
      // expression of the return type. The chain is exhaustive by the
      // author's reckoning, and the line after it says so.
      if (!fallsOff && stubbed == null) _closeOpenIf(method.body);
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
        // ..and as a method in every respect: it returns `Result` and
        // its body may `?`, as the trait's declaration of the same
        // operator does (`stdOperators`).
        _line('');
        _line('impl${_implGenerics(cls)} ${cls.name}${_generics(cls)} {');
        _indent++;
        _emitMethod(method, as: _operatorName(op));
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
      _here = '${cls.name}.${method.name}';
      _asyncBody = method.isAsync;
      _methodTypeParams = method.typeParameters;
      _reassigned = _assignedIn(method.body);
      _cellLocals = {};
      // An operator's signature is `std::ops`'s and cannot say `Result`:
      // inside it a failing call unwraps.
      final savedFailure = _failure;
      _failure = null;
      // ..and takes `self` by value: `this` inside is `self`, not `*self`
      // (`Priority.operator -` doing `this + (-offset)`, E0614 at ws463).
      _selfByValue = true;
      _body(
        method.body,
        method.isAsync ? _awaited(method.returnType) : method.returnType,
      );
      _selfByValue = false;
      _closeOpenIf(method.body);
      _failure = savedFailure;
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
      _stdShadowed[name] ??
      (RegExp(r'[A-Za-z]').hasMatch(name) ? snake(name) : _operatorName(name));

  /// `dart:core` methods whose snake-cased name is an *unstable* inherent
  /// method of Rust's std, which outranks any trait's: spelled by the
  /// prelude's own name (`String.replaceFirst`, E0658 16 at ws465).
  static const _stdShadowed = {'replaceFirst': 'dart_replace_first'};
}

/// Finds, in one method body, whether it writes a field of `this` and which of
/// its own methods it calls.
///
/// Both answers are needed together and both need the *whole* body, statements
/// and expressions alike -- a mutating call can be buried in the middle of an
/// expression, and missing one would emit `&self` for a method that assigns.
class _WalkSelf {
  bool writesFields = false;

  /// Whether a null-aware's bound value (`it`) is read: a closure made in
  /// such a body must own a clone of it (`_closure`).
  bool readsBound = false;
  int _nullAwareDepth = 0;
  final selfCalls = <String>{};

  /// The classes `super` calls resolve into (`IrSuperCall.base`), each
  /// with its type arguments (`baseArguments`).
  final superBases = <String, List<IrType>>{};

  /// Whether anything walked can fail -- a `?` on a call, a constructor,
  /// an `await`.
  bool failing = false;

  /// Whether any of `elements` can fail.
  static bool failingIn(Iterable<IrExpr> elements) {
    final walk = _WalkSelf();
    elements.forEach(walk.expression);
    return walk.failing;
  }

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
    '!map_remove',
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
      case IrCall(:final target, :final name, :final args, :final fails):
        if (fails) failing = true;
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
      case IrDynamicDispatch(:final receiver, :final arms):
        expression(receiver);
        for (final (_, b) in arms) {
          expression(b);
        }
      case IrDowncast(:final target):
        expression(target);
      case IrCastTo(:final target):
        expression(target);
      case IrSuperDispatch(:final receiver, :final args):
        expression(receiver);
        args.forEach(expression);
      case IrNullableOf(:final value):
        expression(value);
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
        // The body binds its own `it`: a read of the bound in there is
        // not a read of an enclosing null-aware's.
        _nullAwareDepth++;
        expression(body);
        _nullAwareDepth--;
      case IrConditional(:final condition, :final then, :final otherwise):
        expression(condition);
        expression(then);
        expression(otherwise);
      case IrStaticCall(:final args, :final fails):
        if (fails) failing = true;
        // `FlutterView(id, this, ..)` from a counted class hands out the handle.
        if (args.any((a) => a is IrThis)) passesSelf = true;
        args.forEach(expression);
      case IrNew(:final args):
        // A translated class's constructor returns `Result`; the walker
        // does not know which classes are translated, and a prelude one
        // built in steps is the same value.
        failing = true;
        if (args.any((a) => a is IrThis)) passesSelf = true;
        args.forEach(expression);
      case IrSuperCall(:final base, :final args, :final baseArguments):
        superBases.putIfAbsent(base, () => baseArguments);
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
      case IrUpcast(:final value):
        expression(value);
      case IrMapElements(:final collection, :final body):
        expression(collection);
        expression(body);
      case IrAwait(:final operand):
        failing = true;
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
      // Its own case: the empty cases above it would fall through into a
      // body placed after them (every closure cloned `it`, ws486).
      case IrBound():
        if (_nullAwareDepth == 0) readsBound = true;
      case IrLiteral():
      case IrLocal():
      case IrStatic():
      case IrTopLevel():
    }
  }
}

/// The backend's view of the classes a type names, for `coerceInto`: the
/// `IrLibrary` knows which are traits, counted, enums, and how they sit in
/// the hierarchy.
class _BackendWorld implements TypeWorld {
  _BackendWorld(this.backend);

  final RustBackend backend;

  IrLibrary get library => backend.library;

  @override
  bool isTrait(String name) =>
      const {
        'Object',
        'dynamic',
        'Comparable',
        'DartIterator',
      }.contains(name) ||
      library.isAbstract(name);

  @override
  bool isCounted(String name) => library[name]?.counted ?? false;

  @override
  bool isEnum(String name) => library[name]?.isEnum ?? false;

  @override
  bool isStruct(String name) {
    final c = library[name];
    return c != null && !c.isAbstract && !c.isEnum;
  }

  @override
  bool isBelow(String sub, String sup) {
    final c = library[sub];
    return c != null && backend._isSubtypeOf(c, sup, {});
  }

  @override
  bool isGenericValueStruct(String name) {
    final c = library[name];
    return c != null && !c.counted && c.typeParameters.isNotEmpty;
  }

  @override
  bool isTypeParameter(String name) => backend._isTypeParam(name);
}
