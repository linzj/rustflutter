// Does the gallery's own app.dill read, and does it contain what a translator
// would need? Three questions, in the order that matters:
//
//   1. does it parse at all
//   2. is the gallery's own code in there, not just the framework
//   3. are the things the analyzer front end has to guess already resolved --
//      mixins applied, super targets named, constants evaluated
//
// If any answer is no, the Kernel front end is not worth writing.

import 'dart:io';

import 'package:kernel/ast.dart';
import 'package:kernel/kernel.dart';

class SuperFinder extends RecursiveVisitor {
  int superCalls = 0;
  final targets = <String>[];

  @override
  void visitSuperMethodInvocation(SuperMethodInvocation node) {
    superCalls++;
    if (targets.length < 4) {
      targets.add('${node.name.text} -> '
          '${node.interfaceTarget.enclosingClass?.name}.'
          '${node.interfaceTarget.name.text}');
    }
    super.visitSuperMethodInvocation(node);
  }
}

void main(List<String> args) {
  final started = DateTime.now();
  final component = loadComponentFromBinary(args[0]);
  final elapsed = DateTime.now().difference(started);

  final byOrigin = <String, int>{};
  var classes = 0;
  for (final lib in component.libraries) {
    classes += lib.classes.length;
    final uri = lib.importUri.toString();
    final key = uri.startsWith('package:')
        ? 'package:${uri.split(':')[1].split('/')[0]}'
        : uri.split(':').first;
    byOrigin[key] = (byOrigin[key] ?? 0) + lib.classes.length;
  }

  print('read in ${elapsed.inSeconds}s');
  print('libraries: ${component.libraries.length}, classes: $classes');
  print('');
  final ranked = byOrigin.entries.toList()
    ..sort((a, b) => b.value.compareTo(a.value));
  for (final e in ranked.take(10)) {
    print('  ${e.value.toString().padLeft(6)}  ${e.key}');
  }

  final gallery = component.libraries
      .where((l) => l.importUri.toString().startsWith('package:gallery/'))
      .toList();
  print('');
  print('gallery libraries: ${gallery.length}, '
      'classes: ${gallery.fold(0, (a, l) => a + l.classes.length)}');
  for (final lib in gallery.take(3)) {
    final names = lib.classes.take(4).map((c) => c.name).join(', ');
    print('  ${lib.importUri}: $names');
  }

  // Mixins: the CFE turns `class X extends A with M` into a synthetic
  // application class. If these exist, the front end never has to apply one.
  final applied = component.libraries
      .expand((l) => l.classes)
      .where((c) => c.isAnonymousMixin)
      .length;
  print('');
  print('anonymous mixin applications already built: $applied');

  // Super calls come with their target member resolved, which is the thing the
  // analyzer front end has to look up by hand.
  final finder = SuperFinder();
  for (final lib in gallery) {
    lib.accept(finder);
  }
  print('super calls in gallery code: ${finder.superCalls}');
  for (final t in finder.targets) {
    print('  $t');
  }

  // Constants: evaluated by the CFE, which is the fact-class the hand port kept
  // getting wrong by eye.
  var constFields = 0;
  for (final lib in gallery) {
    for (final cls in lib.classes) {
      for (final f in cls.fields) {
        if (f.isConst && f.initializer is ConstantExpression) constFields++;
      }
    }
  }
  print('gallery const fields already evaluated: $constFields');
}
