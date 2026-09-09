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
import 'member_names.dart';

import 'dart:io' show Platform, stderr;

import 'ir.dart';
import 'prelude.dart';

// The class is one class in seven files. Splitting it is the only thing
// this does: each part holds one of the sections the file already had
// (`// -- Expressions --`), moved without a character changed, and
// `augment` puts them back together. Nothing here is a boundary -- the
// sections share 42 of the class's 69 fields, so they are not separable
// objects yet; making them so is the state-object round, not this one.
//
// `augment` is behind `--enable-experiment=augmentations`, which
// `bin/experiments.sh` is the one place that names.
part 'backend_rust/expressions.dart';
part 'backend_rust/closures.dart';
part 'backend_rust/operators.dart';
part 'backend_rust/nullaware.dart';
part 'backend_rust/super_calls.dart';
part 'backend_rust/places.dart';
part 'backend_rust/calls.dart';
part 'backend_rust/values.dart';
part 'backend_rust/statements.dart';
part 'backend_rust/the_class.dart';
part 'backend_rust/protocols.dart';
part 'backend_rust/generics.dart';
part 'backend_rust/free_fns.dart';
part 'backend_rust/mutability.dart';
part 'backend_rust/flattening.dart';
part 'backend_rust/failure.dart';
part 'backend_rust/emit_struct.dart';
part 'backend_rust/emit_impl.dart';
part 'backend_rust/emit_members.dart';
part 'backend_rust/walk_self.dart';

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
  // thing. A float is the choice that keeps arithmetic working and matches
  // what `double` already maps to -- 2511 uses of the bare name `num`, three
  // quarters of every "cannot find" in the package, and every one of them a
  // parameter or return that takes either.
  //
  // `f32` at first, and `f64` since: this comment argued for `f32` long after
  // the map said `f64` (corrected 2026-09-09), which is how a reader ends up
  // believing a precision the code does not have.
  //
  // The cost, written down rather than discovered: an `int` beyond 2^53 does
  // not survive the round trip, and a `num` used as an index needs a cast that
  // an `i64` would not. Upstream's `num`s are sizes, offsets and factors, so
  // neither has come up -- but this is where to look when one does.
  'num': 'f64',
  'bool': 'bool',
  'String': 'String',
  'void': '()',
};

/// `dart:core` collections a value is downcast to, by their spelling here.
const _downcastNames = {'List': 'Vec'};

/// Dart operators that are Rust traits, and the trait's method name.
///
/// The same operators `ir.dart`'s `stdOperators` names, which is what the
/// front end decides propagation by. Public so that `test/naming_test.dart`
/// can hold the two lists to each other, which is what the comment in each
/// file pointing at the other used to stand in for.
const operatorTraits = {
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
  /// The members this class could not emit, by the `what` they were
  /// announced with: a protocol impl written afterwards must not call one.
  final _stubbed = <String>{};

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
      // Named, so the protocol impls below do not call what this member
      // became: a stubbed `hashCode` panics, and `DartEq::dart_hash_code`
      // must not (`_dartHashBody`; `_IdentityThemeDataCacheKey`, run802).
      _stubbed.add(what);
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

  /// A class name as this module spells it: by module when the type
  /// carries one (`IrType.module`: `crate::dart_ui::TextStyle` beside
  /// painting's own `TextStyle`), bare otherwise.
  String _spelled(IrType t) =>
      t.module == null ? t.name : 'crate::${t.module}::${t.name}';

  String type(IrType t, {bool owned = true}) {
    // Dart's `void?` is `void`, and the prelude's unit says so (`<() as
    // DartNullable>::Or = ()`): no `Option` around it.
    if (t.name == 'void' && t.nullable) return '()';
    // Inside a wider impl written for one concrete instantiation of a
    // generic class (`_selfBinding`), the class's own parameter spells as
    // what that instantiation put in for it.
    if (_selfBinding.isNotEmpty && !t.isFunction && t.arguments.isEmpty) {
      final bound = _selfBinding[t.name];
      if (bound != null && bound.name != t.name) {
        return type(
          IrType(
            bound.name,
            nullable: t.nullable || bound.nullable,
            arguments: bound.arguments,
            projected: t.projected,
          ),
          owned: owned,
        );
      }
    }
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
    if (library.isAbstractType(t)) {
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
      final dynamic_ = 'std::rc::Rc<dyn ${_spelled(t)}$args>';
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
    final owner = library.resolve(t);
    // Its own name included: a counted class's fields, parameters and
    // returns that name the class itself are handles too, as they are from
    // every other module. `impl` headers and constructors do not come
    // through here.
    if (owner != null && owner.counted) {
      final spelled =
          'std::rc::Rc<${_spelled(t)}${t.arguments.isEmpty ? '' : '<'
                    '${t.arguments.map((a) => type(a)).join(', ')}>'}>';
      return t.nullable ? 'Option<$spelled>' : spelled;
    }
    // A nullable type parameter in a signature: the associated type that
    // collapses `T?` with `T` bound to `X?` (see `IrType.projected`).
    final mapped = _primitives[t.name] ?? _spelled(t);
    // `Foo<int>` was coming out as a bare `Foo`, which is a different type.
    final spelled = t.arguments.isEmpty || _primitives.containsKey(t.name)
        ? mapped
        : '$mapped<${t.arguments.map((a) => type(a)).join(', ')}>';
    return t.nullable ? 'Option<$spelled>' : spelled;
  }
}
