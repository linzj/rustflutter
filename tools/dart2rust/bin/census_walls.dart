// What the four remaining walls are actually made of.
//
// Round 65 named four things that stop a translated gallery from running: no
// `dart:core` runtime, an object model Rust rejects, `async` and its event
// loop, and the engine boundary. Those were stated as conclusions with no
// measurement under them, which is exactly the mistake this file exists to
// stop being repeated.
//
// Each question here is one a number can answer:
//
//   * **async** -- how much of it is there, and has the CFE already desugared
//     it? If the dill carries state machines rather than `await`, the work is
//     a runtime, not a transform.
//   * **dart:core** -- which members does the translated code actually call?
//     A hand-written prelude only has to cover what is reached.
//   * **ownership** -- how much of the object graph is shared and cyclic?
//     `Rc` alone cannot hold a cycle; whether `Weak` is needed is a fact about
//     the graph, not a matter of taste.
//   * **the engine** -- how wide is the `dart:ui` surface, and how much of it
//     is `external`?
//
//     dart run --packages=<kernel config> \
//         tools/dart2rust/bin/census_walls.dart app.dill [package:]
//
// Build the dill and the config with `bin/dill.py`.

import 'dart:io';

import 'package:kernel/ast.dart';
import 'package:kernel/kernel.dart';

/// Counts, printed as "N name" newest first.
void report(String title, Map<String, int> counts, {int limit = 20}) {
  final total = counts.values.fold(0, (a, b) => a + b);
  print('\n$title  (${counts.length} distinct, $total uses)');
  final sorted = counts.entries.toList()
    ..sort((a, b) => b.value.compareTo(a.value));
  var running = 0;
  for (var i = 0; i < sorted.length; i++) {
    running += sorted[i].value;
    if (i < limit) {
      final share = (running * 100 / total).round();
      print(
        '  ${sorted[i].value.toString().padLeft(6)}  '
        '${share.toString().padLeft(3)}%  ${sorted[i].key}',
      );
    }
  }
  // How few of them would have to be written by hand to cover most of the
  // uses. The long tail is the question a prelude has to answer, and it is
  // usually shorter than it looks.
  running = 0;
  for (final target in [50, 80, 90, 95, 99]) {
    var n = 0;
    var covered = 0;
    while (covered * 100 < target * total && n < sorted.length) {
      covered += sorted[n].value;
      n++;
    }
    print('    $target% of the uses are $n distinct members');
  }
}

class _Walls extends RecursiveVisitor {
  _Walls(this.inScope);

  /// Whether a library is one this compiler translates.
  final bool Function(Library) inScope;

  // async
  int awaits = 0;
  final asyncMarkers = <String, int>{};

  // what the translated code calls, outside itself
  final external_ = <String, int>{};
  final ui = <String, int>{};

  void _target(Member? member) {
    if (member == null) return;
    final library = member.enclosingLibrary;
    if (inScope(library)) return;
    final uri = library.importUri.toString();
    if (!uri.startsWith('dart:')) return;
    final owner = member.enclosingClass?.name;
    final name =
        '${uri.substring(5)}  ${owner == null ? '' : '$owner.'}'
        '${member.name.text}';
    (uri == 'dart:ui' ? ui : external_).update(
      name,
      (n) => n + 1,
      ifAbsent: () => 1,
    );
  }

  @override
  void visitAwaitExpression(AwaitExpression node) {
    awaits++;
    super.visitAwaitExpression(node);
  }

  @override
  void visitProcedure(Procedure node) {
    final marker = node.function.asyncMarker;
    if (marker != AsyncMarker.Sync) {
      asyncMarkers.update(marker.name, (n) => n + 1, ifAbsent: () => 1);
    }
    super.visitProcedure(node);
  }

  @override
  void visitStaticInvocation(StaticInvocation node) {
    _target(node.target);
    super.visitStaticInvocation(node);
  }

  @override
  void visitStaticGet(StaticGet node) {
    _target(node.target);
    super.visitStaticGet(node);
  }

  @override
  void visitConstructorInvocation(ConstructorInvocation node) {
    _target(node.target);
    super.visitConstructorInvocation(node);
  }

  @override
  void visitInstanceInvocation(InstanceInvocation node) {
    _target(node.interfaceTarget);
    super.visitInstanceInvocation(node);
  }

  @override
  void visitInstanceGet(InstanceGet node) {
    _target(node.interfaceTarget);
    super.visitInstanceGet(node);
  }

  @override
  void visitInstanceSet(InstanceSet node) {
    _target(node.interfaceTarget);
    super.visitInstanceSet(node);
  }
}

/// Tarjan, over "a field of A holds a B".
///
/// A component with more than one class in it is a cycle in the object graph:
/// `Rc` cannot express one without `Weak`, so the size of the largest is a
/// fact the ownership design has to answer to.
List<List<String>> components(Map<String, Set<String>> edges) {
  final index = <String, int>{};
  final low = <String, int>{};
  final onStack = <String>{};
  final stack = <String>[];
  final found = <List<String>>[];
  var next = 0;

  void strongConnect(String v) {
    // Iterative: the graph is 4000 classes deep in places and a recursive
    // Tarjan overflows the stack on it.
    final work = <(String, int)>[(v, 0)];
    while (work.isNotEmpty) {
      var (node, i) = work.removeLast();
      if (i == 0) {
        index[node] = next;
        low[node] = next;
        next++;
        stack.add(node);
        onStack.add(node);
      }
      var recursed = false;
      final children = (edges[node] ?? const <String>{}).toList();
      for (var j = i; j < children.length; j++) {
        final child = children[j];
        if (!index.containsKey(child)) {
          work.add((node, j + 1));
          work.add((child, 0));
          recursed = true;
          break;
        } else if (onStack.contains(child)) {
          low[node] = low[node]! < index[child]! ? low[node]! : index[child]!;
        }
      }
      if (recursed) continue;
      if (low[node] == index[node]) {
        final group = <String>[];
        while (true) {
          final w = stack.removeLast();
          onStack.remove(w);
          group.add(w);
          if (w == node) break;
        }
        found.add(group);
      }
      if (work.isNotEmpty) {
        final parent = work.last.$1;
        low[parent] = low[parent]! < low[node]! ? low[parent]! : low[node]!;
      }
    }
  }

  for (final v in edges.keys) {
    if (!index.containsKey(v)) strongConnect(v);
  }
  return found;
}

void main(List<String> args) {
  if (args.isEmpty) {
    stderr.writeln('usage: census_walls.dart <app.dill> [uri prefix]');
    exit(2);
  }
  final prefixes = (args.length > 1 ? args[1] : 'package:,dart:ui').split(',');
  final component = loadComponentFromBinary(args.first);
  bool inScope(Library l) => prefixes.any(l.importUri.toString().startsWith);

  final walls = _Walls(inScope);
  final classes = <String, Class>{};
  var members = 0;
  var mutableFields = 0;
  var finalFields = 0;
  var callbackFields = 0;
  final edges = <String, Set<String>>{};

  for (final library in component.libraries) {
    if (!inScope(library)) continue;
    library.accept(walls);
    for (final cls in library.classes) {
      classes[cls.name] = cls;
      members += cls.members.length;
      for (final field in cls.fields) {
        if (field.isStatic) continue;
        if (field.isFinal) {
          finalFields++;
        } else {
          mutableFields++;
        }
        final type = field.type;
        if (type is FunctionType) callbackFields++;
        if (type is InterfaceType && inScope(type.classNode.enclosingLibrary)) {
          (edges[cls.name] ??= {}).add(type.classNode.name);
        }
        // A `List<Widget>` holds widgets as surely as a `Widget` field does.
        if (type is InterfaceType) {
          for (final argument in type.typeArguments) {
            if (argument is InterfaceType &&
                inScope(argument.classNode.enclosingLibrary)) {
              (edges[cls.name] ??= {}).add(argument.classNode.name);
            }
          }
        }
      }
    }
  }

  print(
    '${classes.length} classes, $members members in '
    '${prefixes.join(',')}',
  );

  print('\n=== 1. async ===');
  print('  await expressions: ${walls.awaits}');
  walls.asyncMarkers.forEach((marker, n) {
    print('  ${n.toString().padLeft(6)}  $marker functions');
  });
  print('  (an `await` still present means the CFE did NOT desugar it here,');
  print('   so translating async needs the transform as well as a runtime)');

  print('\n=== 2. the object graph ===');
  print('  instance fields: $finalFields final, $mutableFields mutable');
  print('  fields holding a function: $callbackFields');
  final groups = components(edges)
    ..sort((a, b) => b.length.compareTo(a.length));
  final cyclic = groups.where((g) => g.length > 1).toList();
  final inCycles = cyclic.fold(0, (a, g) => a + g.length);
  print('  classes reachable from another class by a field: ${edges.length}');
  print('  cycles in that graph: ${cyclic.length}, holding $inCycles classes');
  for (final g in cyclic.take(3)) {
    print(
      '    ${g.length}: ${g.take(8).join(', ')}'
      '${g.length > 8 ? ' ...' : ''}',
    );
  }

  print('\n=== 3. what is called outside the translated set ===');
  report('dart: libraries other than dart:ui', walls.external_, limit: 25);

  print('\n=== 4. the engine boundary ===');
  var externals = 0;
  var withBodies = 0;
  for (final library in component.libraries) {
    if (library.importUri.toString() != 'dart:ui') continue;
    for (final cls in library.classes) {
      for (final member in cls.members) {
        if (member is Procedure) {
          if (member.isExternal) {
            externals++;
          } else if (member.function.body != null) {
            withBodies++;
          }
        }
      }
    }
  }
  print('  dart:ui procedures: $externals external, $withBodies with bodies');
  report('dart:ui members the translated code calls', walls.ui, limit: 25);
}
