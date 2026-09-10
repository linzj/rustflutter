// Whole-program census of *constant aggregates*: data written as code.
//
// See `/tmp/work.md` (2026-09-10). `codeviewer_code_segments.rs` is 13.16 MB
// of `.text` -- 15.4% of the program -- and it is 45,732 `TextSpan::new`
// calls building a tree that never varies. The same shape holds the date and
// number symbol tables. The plan's first layer is to *recognise* those trees;
// this file is that recognition, and nothing reads it yet but the census.
//
// Deliberately shaped like `ThrowsAnalysis`: a closed-world fixed point over
// the translated libraries, computed once, printed before it decides any code
// generation. `frontend_kernel.dart:736` records why that order matters -- a
// analysis wired into codegen before anyone has read its numbers cost two
// rounds of misdiagnosis, because it looked like an analysis and was a switch.
//
// Two definitions, and the second is the loose one:
//
//   * A **data constructor** writes only parameters, constant defaults, and
//     other data constructors into fields. No calls, no throw, no assert.
//     A greatest fixed point: assume every constructor qualifies, then strike
//     out the ones that reach something disqualifying, until nothing moves.
//     It has to be a fixed point rather than a syntactic test because
//     `TextSpan`'s field initialisers nest other constructions, and it has to
//     be whole-program because those live in other libraries.
//
//   * An **outer invariant** is a read the aggregate does not compute:
//     `codeStyle.commentStyle`, where `codeStyle` is a local nothing assigns
//     twice. This is the part that is a judgement rather than a proof -- a
//     getter can do anything -- so invariants are counted *separately* from
//     the purely constant leaves everywhere below, and never folded into
//     them. Whoever reads this census gets to see how much of the prize
//     rests on that judgement.
import 'package:kernel/kernel.dart';

/// One maximal constant aggregate found in a member's body.
class Aggregate {
  Aggregate(
    this.member,
    this.nodes,
    this.ctors,
    this.literalBytes,
    this.invariants,
    this.shape,
    this.stringLeaves,
    this.scalarLeaves,
  );

  final Member member;

  /// Every expression node in the tree, leaves included.
  final int nodes;

  /// Data-constructor invocations. This is the size proxy: each one is a
  /// call with its arguments spilled to the stack, measured at 304 bytes of
  /// `.text` per span in `code_segments_appbar_demo`.
  final int ctors;

  /// Bytes of string literal inside the tree -- what a blob would hold.
  final int literalBytes;

  /// Distinct outer-invariant reads. Small means the table gets a narrow
  /// enumerated column (the `u8` in `(u32, u32, u8)`); large means the tree
  /// is parameterised by too much to be a table.
  final Set<String> invariants;

  /// Canonical shape with literals and invariants punched out, so the
  /// whole program can be asked how many *builders* it would take.
  final String shape;

  /// Leaves that cost a heap allocation each (`"x".to_string()`) and leaves
  /// that cost only a store. A tree's `.text` follows these far more
  /// closely than it follows the construction count: the keyboard maps are
  /// 1,003 constructions and 35 KB, `dateSymbols` is 98 constructions and
  /// 1.16 MB. Anything that prices a tree by constructions alone is wrong
  /// by up to 8x, measured.
  final int stringLeaves;
  final int scalarLeaves;

  String get library => member.enclosingLibrary.importUri.toString();
}

class AggregateCensus {
  AggregateCensus._(
    this.dataConstructors,
    this.constructorsSeen,
    this.rounds,
    this.rejections,
    this.aggregates,
    this.membersScanned,
  );

  /// Constructors that only move data into fields.
  final Set<Constructor> dataConstructors;
  final int constructorsSeen;
  final int rounds;

  /// Why a constructor was struck out, by reason. The tail of this table is
  /// where the next widening lives.
  final Map<String, int> rejections;

  final List<Aggregate> aggregates;
  final int membersScanned;

  static bool _translated(Library lib, List<String> prefixes) =>
      prefixes.any(lib.importUri.toString().startsWith);

  static AggregateCensus of(Component component, List<String> prefixes) {
    final libs = [
      for (final lib in component.libraries)
        if (_translated(lib, prefixes)) lib,
    ];
    // The fixed point ranges over *every* library, not just the translated
    // ones. A super chain ends at `Object`, which lives in `dart:core`; asked
    // only about the translated libraries the analysis strikes out all 3,214
    // constructors on the first round, 2,594 of them blaming a super it
    // cannot see. What gets translated and what a data constructor may reach
    // are two different questions.
    final ctors = <Constructor>[];
    for (final lib in component.libraries) {
      for (final cls in lib.classes) {
        ctors.addAll(cls.constructors);
      }
    }
    // Greatest fixed point: everything qualifies until something disqualifies
    // it. Starting from the other end would never admit a constructor whose
    // field initialiser constructs another one, which is the whole case that
    // matters -- `TextSpan.mouse_cursor` nests two.
    final data = ctors.toSet();
    final rejections = <String, int>{};
    var rounds = 0;
    var changed = true;
    final blamed = <Constructor>{};
    while (changed) {
      changed = false;
      rounds++;
      for (final c in ctors.toList()) {
        if (!data.contains(c)) continue;
        final why = _disqualifies(c, data);
        if (why == null) continue;
        data.remove(c);
        changed = true;
        if (blamed.add(c)) {
          rejections[why] = (rejections[why] ?? 0) + 1;
        }
      }
    }
    final found = <Aggregate>[];
    var scanned = 0;
    for (final lib in libs) {
      final members = <Member>[
        ...lib.procedures,
        for (final f in lib.fields)
          if (f.initializer != null) f,
        for (final cls in lib.classes) ...[
          ...cls.procedures,
          ...cls.constructors,
          for (final f in cls.fields)
            if (f.initializer != null) f,
        ],
      ];
      for (final m in members) {
        scanned++;
        final finder = _AggregateFinder(m, data);
        m.accept(finder);
        found.addAll(finder.found);
      }
    }
    return AggregateCensus._(
      data,
      ctors.length,
      rounds,
      rejections,
      found,
      scanned,
    );
  }

  /// `null` when `c` only moves data into fields, else why it does not.
  static String? _disqualifies(Constructor c, Set<Constructor> data) {
    final body = c.function.body;
    if (!_emptyBody(body)) return 'body runs statements';
    for (final p in [
      ...c.function.positionalParameters,
      ...c.function.namedParameters,
    ]) {
      final d = p.defaultValue;
      if (d != null && !_pureData(d, data)) return 'parameter default computes';
    }
    // A class's *own* instance field initialisers run in every constructor,
    // and Kernel leaves them on `Field.initializer` -- they are not in
    // `c.initializers`. Looking only at the constructor let 77 constructors
    // through that compute in a field declaration:
    // `_AppBarDefaultsM2._theme = Theme.of(this.context)` is a call, and the
    // generated Rust has it inside `new`. None of the 77 reached a counted
    // tree, so no number this census reported moved -- but a fixed point
    // that drives code generation would flatten `_AppBarDefaultsM2(context)`
    // into a static table and delete the `Theme.of` call outright.
    final owner = c.enclosingClass;
    final assignedHere = <Field>{
      for (final init in c.initializers)
        if (init is FieldInitializer) init.field,
    };
    for (final f in owner.fields) {
      if (f.isStatic || assignedHere.contains(f)) continue;
      final declared = f.initializer;
      if (declared != null && !_pureData(declared, data)) {
        return 'class field initializer computes';
      }
    }
    for (final init in c.initializers) {
      switch (init) {
        case FieldInitializer(:final value):
          if (!_pureData(value, data)) return 'field initializer computes';
        case SuperInitializer(:final target, :final arguments):
          if (!data.contains(target)) return 'super is not a data constructor';
          if (!_pureArgs(arguments, data)) return 'super argument computes';
        case RedirectingInitializer(:final target, :final arguments):
          if (!data.contains(target)) {
            return 'redirects to a non-data constructor';
          }
          if (!_pureArgs(arguments, data)) return 'redirect argument computes';
        case AssertInitializer():
          return 'asserts';
        case LocalInitializer():
          return 'binds a local';
        default:
          return 'initializer of an unhandled kind';
      }
    }
    return null;
  }

  static bool _emptyBody(Statement? body) => switch (body) {
    null || EmptyStatement() => true,
    Block(:final statements) => statements.every(_emptyBody),
    _ => false,
  };

  static bool _pureArgs(Arguments a, Set<Constructor> data) =>
      a.positional.every((e) => _pureData(e, data)) &&
      a.named.every((n) => _pureData(n.value, data));

  /// Inside a *constructor*, where the only free names are its own
  /// parameters. This is stricter than what an aggregate leaf may be: no
  /// invariant reads, because a constructor that reads a getter is running
  /// someone else's code.
  static bool _pureData(Expression e, Set<Constructor> data) => switch (e) {
    StringLiteral() ||
    IntLiteral() ||
    DoubleLiteral() ||
    BoolLiteral() ||
    NullLiteral() ||
    ConstantExpression() ||
    TypeLiteral() ||
    VariableGet() => true,
    ListLiteral(:final expressions) => expressions.every(
      (x) => _pureData(x, data),
    ),
    SetLiteral(:final expressions) => expressions.every(
      (x) => _pureData(x, data),
    ),
    MapLiteral(:final entries) => entries.every(
      (x) => _pureData(x.key, data) && _pureData(x.value, data),
    ),
    ConstructorInvocation(:final target, :final arguments) =>
      data.contains(target) && _pureArgs(arguments, data),
    // Choosing between two data values is still moving data. `TextSpan`'s
    // `mouseCursor` is
    //
    //   let #0 = mouseCursor in
    //     recognizer == null ? const _DeferringMouseCursor{}
    //                        : const SystemMouseCursor{kind: "click"}
    //
    // and that one field is the whole reason `TextSpan` -- 45,732 of the
    // program's constructions -- did not qualify. None of these forms runs
    // anyone else's code: `EqualsNull` is a null test, not `operator ==`,
    // which is why `EqualsCall` is absent from this list. Nor does allowing
    // them weaken what a table may encode: the conditional lives *inside*
    // the constructor, which is called once per row either way.
    Let(:final value, :final body) =>
      _pureData(value, data) && _pureData(body, data),
    ConditionalExpression(:final condition, :final then, :final otherwise) =>
      _pureData(condition, data) &&
          _pureData(then, data) &&
          _pureData(otherwise, data),
    EqualsNull(:final expression) => _pureData(expression, data),
    Not(:final operand) => _pureData(operand, data),
    LogicalExpression(:final left, :final right) =>
      _pureData(left, data) && _pureData(right, data),
    IsExpression(:final operand) => _pureData(operand, data),
    _ => false,
  };
}

/// Finds *maximal* aggregates: when a tree qualifies, its subtrees are part
/// of it and are not reported again.
class _AggregateFinder extends RecursiveVisitor {
  _AggregateFinder(this.member, this.data) {
    final assigned = _AssignedVars();
    member.accept(assigned);
    _assigned = assigned.names;
  }

  final Member member;
  final Set<Constructor> data;
  late final Set<Variable> _assigned;
  final found = <Aggregate>[];

  /// A tree is worth reporting when it actually builds something.
  static const _minCtors = 2;

  @override
  void defaultExpression(Expression node) {
    final info = _aggregate(node);
    if (info != null && info.ctors >= _minCtors) {
      found.add(
        Aggregate(
          member,
          info.nodes,
          info.ctors,
          info.literalBytes,
          info.invariants,
          info.shape,
          info.stringLeaves,
          info.scalarLeaves,
        ),
      );
      return; // maximal: do not descend into what we just counted
    }
    super.defaultExpression(node);
  }

  @override
  void visitConstructorInvocation(ConstructorInvocation node) =>
      defaultExpression(node);

  @override
  void visitListLiteral(ListLiteral node) => defaultExpression(node);

  @override
  void visitMapLiteral(MapLiteral node) => defaultExpression(node);

  @override
  void visitSetLiteral(SetLiteral node) => defaultExpression(node);

  /// The key of an outer invariant, or `null` when `e` computes something.
  ///
  /// A read the tree does not compute: a parameter, a local nothing assigns
  /// twice, a static final, and getter chains rooted at one of those. The
  /// getter is the loose end -- it is *assumed* to return the same thing
  /// each time. Every count that depends on this is reported apart from the
  /// counts that do not.
  String? _invariant(Expression e) => switch (e) {
    VariableGet(:final variable) =>
      _assigned.contains(variable) ? null : 'v:${_varName(variable)}',
    ThisExpression() => 'this',
    StaticGet(:final target) =>
      target is Field && target.isFinal ? 's:${target.name.text}' : null,
    InstanceGet(:final receiver, :final name) => switch (_invariant(receiver)) {
      final String root => '$root.${name.text}',
      _ => null,
    },
    _ => null,
  };

  _Info? _aggregate(Expression e) {
    switch (e) {
      case StringLiteral(:final value):
        return _Info(1, 0, value.length, const {}, 'L', stringLeaves: 1);
      case IntLiteral() ||
          DoubleLiteral() ||
          BoolLiteral() ||
          NullLiteral() ||
          TypeLiteral():
        return _Info(1, 0, 0, const {}, 'L', scalarLeaves: 1);
      // A `const` in the source is *not* a leaf in the generated Rust.
      // `const <String>["J", "F", ...]` comes out `vec!["J".to_string(),
      // ...]` -- one allocation and one store per element, which is code.
      // Counting the whole constant as one node is what made this census
      // put `generated_date_localizations` at 0.03 MB while its symbol in
      // the binary is 1.11 MB.
      case ConstantExpression(:final constant):
        return _constant(constant);
      case ListLiteral(:final expressions):
        return _collection(expressions, '[', ']');
      case SetLiteral(:final expressions):
        return _collection(expressions, '{', '}');
      case MapLiteral(:final entries):
        return _collection(
          [
            for (final entry in entries) ...[entry.key, entry.value],
          ],
          '{:',
          ':}',
        );
      case ConstructorInvocation(:final target, :final arguments):
        if (!data.contains(target)) return null;
        var nodes = 1, ctors = 1, bytes = 0, strs = 0, scalars = 0;
        final inv = <String>{};
        final parts = <String>[];
        for (final a in arguments.positional) {
          final sub = _aggregate(a);
          if (sub == null) return null;
          nodes += sub.nodes;
          ctors += sub.ctors;
          bytes += sub.literalBytes;
          strs += sub.stringLeaves;
          scalars += sub.scalarLeaves;
          inv.addAll(sub.invariants);
          parts.add(sub.shape);
        }
        final named = arguments.named.toList()
          ..sort((a, b) => a.name.compareTo(b.name));
        for (final a in named) {
          final sub = _aggregate(a.value);
          if (sub == null) return null;
          nodes += sub.nodes;
          ctors += sub.ctors;
          bytes += sub.literalBytes;
          strs += sub.stringLeaves;
          scalars += sub.scalarLeaves;
          inv.addAll(sub.invariants);
          parts.add('${a.name}:${sub.shape}');
        }
        return _Info(
          nodes,
          ctors,
          bytes,
          inv,
          '${target.enclosingClass.name}(${parts.join(',')})',
          stringLeaves: strs,
          scalarLeaves: scalars,
        );
      default:
        final key = _invariant(e);
        if (key == null) return null;
        // A read of something already built: a move, never an allocation.
        return _Info(1, 0, 0, {key}, 'I', scalarLeaves: 1);
    }
  }

  _Info _constant(Constant c) {
    switch (c) {
      case StringConstant(:final value):
        return _Info(1, 0, value.length, const {}, 'L', stringLeaves: 1);
      case ListConstant(:final entries):
        return _constParts([for (final e in entries) e], '[', ']', 0);
      case SetConstant(:final entries):
        return _constParts([for (final e in entries) e], '{', '}', 0);
      case MapConstant(:final entries):
        return _constParts(
          [
            for (final e in entries) ...[e.key, e.value],
          ],
          '{:',
          ':}',
          0,
        );
      case InstanceConstant(:final classNode, :final fieldValues):
        // A const instance is still constructed: the Rust is `Foo::new(..)`
        // or a struct literal, and its fields are stored one by one.
        final names = fieldValues.keys.toList()
          ..sort((a, b) => a.asField.name.text.compareTo(b.asField.name.text));
        return _constParts(
          [for (final n in names) fieldValues[n]!],
          '${classNode.name}(',
          ')',
          1,
        );
      case InstantiationConstant(:final tearOffConstant):
        return _constant(tearOffConstant);
      default:
        return _Info(1, 0, 0, const {}, 'L', scalarLeaves: 1);
    }
  }

  _Info _constParts(List<Constant> parts, String open, String close, int self) {
    var nodes = 1, ctors = self, bytes = 0, strs = 0, scalars = 0;
    final shapes = <String>{};
    for (final p in parts) {
      final sub = _constant(p);
      nodes += sub.nodes;
      ctors += sub.ctors;
      bytes += sub.literalBytes;
      strs += sub.stringLeaves;
      scalars += sub.scalarLeaves;
      shapes.add(sub.shape);
    }
    final shape = switch (shapes.length) {
      0 => '$open$close',
      1 => '$open${shapes.single}$close',
      _ => '${open}mixed:${shapes.length}$close',
    };
    return _Info(
      nodes,
      ctors,
      bytes,
      const {},
      shape,
      stringLeaves: strs,
      scalarLeaves: scalars,
    );
  }

  _Info? _collection(List<Expression> parts, String open, String close) {
    var nodes = 1, ctors = 0, bytes = 0, strs = 0, scalars = 0;
    final inv = <String>{};
    final shapes = <String>{};
    for (final p in parts) {
      final sub = _aggregate(p);
      if (sub == null) return null;
      nodes += sub.nodes;
      ctors += sub.ctors;
      bytes += sub.literalBytes;
      strs += sub.stringLeaves;
      scalars += sub.scalarLeaves;
      inv.addAll(sub.invariants);
      shapes.add(sub.shape);
    }
    // One shape for every element is the case a table encodes directly:
    // one row per element, one builder for the lot. Anything else needs
    // either several builders or a tagged row, so it is named as mixed.
    final shape = switch (shapes.length) {
      0 => '$open$close',
      1 => '$open${shapes.single}$close',
      _ => '${open}mixed:${shapes.length}$close',
    };
    return _Info(
      nodes,
      ctors,
      bytes,
      inv,
      shape,
      stringLeaves: strs,
      scalarLeaves: scalars,
    );
  }
}

/// A variable's source name. `Variable` is a sealed hierarchy and each arm
/// spells the name differently; `cosmeticName` would say all of them at once
/// but is deprecated, and `bin/check.sh` holds the analyzer at a fixed count.
String _varName(Variable v) => switch (v) {
  LocalVariable(:final name) => name,
  LocalFunctionVariable(:final name) => name,
  LateVariable(:final name) => name,
  ConstVariable(:final name) => name,
  // A synthetic variable has no source name at all; identity is what
  // distinguishes two of them, and these keys are only ever compared
  // within one run.
  SyntheticVariable() => '#${identityHashCode(v)}',
  CatchVariable(:final catchVariableName) => catchVariableName,
  FunctionParameter(:final parameterName) => parameterName,
  ThisVariable() => 'this',
};

class _Info {
  _Info(
    this.nodes,
    this.ctors,
    this.literalBytes,
    this.invariants,
    this.shape, {
    this.stringLeaves = 0,
    this.scalarLeaves = 0,
  });
  final int nodes;
  final int ctors;
  final int literalBytes;
  final Set<String> invariants;
  final String shape;

  /// Leaves that must be *allocated* at run time: every Dart string becomes
  /// a `String::to_string()` call, a heap allocation and a store.
  final int stringLeaves;

  /// Leaves that are just a value moved into place: ints, doubles, bools,
  /// nulls, enum-like const instances.
  final int scalarLeaves;
}

/// Variables the member assigns after their declaration. A local assigned
/// once at its declaration and never again is what `codeStyle` is.
class _AssignedVars extends RecursiveVisitor {
  final names = <Variable>{};

  @override
  void visitVariableSet(VariableSet node) {
    names.add(node.variable);
    super.visitVariableSet(node);
  }
}
