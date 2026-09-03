// What `x is T` is actually asking, across the package.
//
// 283 refusals say "`is` needs the class hierarchy, which this backend does
// not model yet", and modelling it is a large piece of work: a trait object
// in Rust cannot be asked what it is without something being written into the
// trait. So the question worth answering first is how many of the 283 really
// need that, and how many are asking something the compiler already knows.
//
// Three kinds, and they want three different answers:
//
//   * **Statically true.** `x is Foo` where `x` is declared `Foo`, which Dart
//     code writes anyway because the declared type is a supertype somewhere
//     up the chain. Rust wants the literal `true`.
//   * **A test against a concrete class.** `child is RenderBox`, where the
//     operand is a trait object and the target is a struct. This is the one
//     that needs the hierarchy.
//   * **A test against `Null` or a primitive.** `x is String`, `x is int` --
//     nothing to do with the class hierarchy at all, and for a nullable
//     operand `x is T` is `x.is_some()`.
//
//     dart run --packages=<kernel config> \
//         tools/dart2rust/bin/census_is.dart app.dill [package:] [--examples]

import 'dart:io';

import 'package:kernel/ast.dart';
import 'package:kernel/class_hierarchy.dart';
import 'package:kernel/core_types.dart';
import 'package:kernel/kernel.dart';
import 'package:kernel/type_environment.dart';

/// The kinds, ordered so the cheapest answer comes first.
enum Kind {
  alwaysTrue('the operand already has that type -- `true`'),
  nullable('the operand is `T?` and the target is `T` -- `is_some()`'),
  primitive('the target is a primitive or `String`'),
  hierarchy('a real test against a class -- needs the hierarchy'),
  other('something else');

  const Kind(this.label);
  final String label;
}

const _primitives = {
  'int',
  'double',
  'num',
  'bool',
  'String',
  'List',
  'Map',
  'Set',
  'Iterable',
};

class _Visit extends RecursiveVisitor {
  _Visit(this.context, this.environment, this.counts, this.samples);

  final StaticTypeContext context;
  final TypeEnvironment environment;
  final Map<Kind, int> counts;
  final Map<Kind, List<String>> samples;

  @override
  void visitIsExpression(IsExpression node) {
    super.visitIsExpression(node);
    final target = node.type;
    DartType operand;
    try {
      operand = node.operand.getStaticType(context);
    } catch (_) {
      _file(Kind.other, node, '<no static type>');
      return;
    }
    final name = target is InterfaceType ? target.classNode.name : '$target';
    // `x is T` where the declared type is already `T` or below it.
    if (target is InterfaceType &&
        operand is InterfaceType &&
        operand.nullability != Nullability.nullable &&
        environment.isSubtypeOf(operand, target)) {
      _file(Kind.alwaysTrue, node, name);
      return;
    }
    if (target is InterfaceType &&
        operand is InterfaceType &&
        operand.nullability == Nullability.nullable &&
        environment.isSubtypeOf(
          operand.withDeclaredNullability(Nullability.nonNullable),
          target,
        )) {
      _file(Kind.nullable, node, name);
      return;
    }
    if (_primitives.contains(name)) {
      _file(Kind.primitive, node, name);
      return;
    }
    if (target is InterfaceType) {
      _file(Kind.hierarchy, node, name);
      return;
    }
    _file(Kind.other, node, name);
  }

  void _file(Kind kind, IsExpression node, String what) {
    counts[kind] = counts[kind]! + 1;
    final where = samples[kind]!;
    if (where.length < 8) where.add('$node   [$what]');
  }
}

void main(List<String> args) {
  if (args.isEmpty) {
    stderr.writeln(
      'usage: census_is.dart <app.dill> [uri prefix] [--examples]',
    );
    exit(2);
  }
  final prefix = args.length > 1 && !args[1].startsWith('--')
      ? args[1]
      : 'package:';
  final examples = args.contains('--examples');

  final component = loadComponentFromBinary(args.first);
  final coreTypes = CoreTypes(component);
  final hierarchy = ClassHierarchy(component, coreTypes);
  final environment = TypeEnvironment(coreTypes, hierarchy);

  final counts = {for (final k in Kind.values) k: 0};
  final samples = {for (final k in Kind.values) k: <String>[]};

  for (final library in component.libraries) {
    if (!library.importUri.toString().startsWith(prefix)) continue;
    for (final member in [
      ...library.procedures,
      ...library.fields,
      for (final cls in library.classes) ...cls.members,
    ]) {
      final context = StaticTypeContext(member, environment);
      member.accept(_Visit(context, environment, counts, samples));
    }
  }

  // The target names, counted properly: the sample list only holds eight.
  final byTarget = <String, int>{};
  for (final library in component.libraries) {
    if (!library.importUri.toString().startsWith(prefix)) continue;
    for (final member in [
      ...library.procedures,
      ...library.fields,
      for (final cls in library.classes) ...cls.members,
    ]) {
      member.accept(_Targets(byTarget));
    }
  }

  final total = counts.values.fold(0, (a, b) => a + b);
  print('$prefix: $total `is` expressions\n');
  for (final kind in Kind.values) {
    print('${counts[kind]!.toString().padLeft(6)}  ${kind.label}');
    if (examples) {
      for (final sample in samples[kind]!) {
        print('        $sample');
      }
    }
  }

  print('\nby target type:');
  final names = byTarget.keys.toList()
    ..sort((a, b) => byTarget[b]!.compareTo(byTarget[a]!));
  for (final name in names.take(20)) {
    print('${byTarget[name]!.toString().padLeft(6)}  $name');
  }
}

class _Targets extends RecursiveVisitor {
  _Targets(this.counts);
  final Map<String, int> counts;

  @override
  void visitIsExpression(IsExpression node) {
    super.visitIsExpression(node);
    final target = node.type;
    final name = target is InterfaceType ? target.classNode.name : '$target';
    counts[name] = (counts[name] ?? 0) + 1;
  }
}
