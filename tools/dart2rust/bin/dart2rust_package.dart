// Emit a whole package as one crate: a module per library, and `use` between
// them.
//
//     dart run --packages=<kernel config> \
//         tools/dart2rust/bin/dart2rust_package.dart \
//         app.dill package:flutter/ out/
//
// Round 37 measured why this exists. Of `package:flutter`'s 525 libraries, 115
// compile on their own; of the 410 that do not, the commonest missing names
// are `BuildContext` (91 libraries), `Widget` (71), `BoxConstraints` (60) and
// `RenderBox` (43) -- classes in *other flutter libraries*, many of which this
// compiler already translates. It emitted one library at a time and never
// wrote a `use`, so each file reached for neighbours it could not see.
//
// The first attempt was the flattest thing that could work: `lib.rs`
// re-exported every module and each module opened with `use crate::*`. That
// was measured rather than reasoned about, and it lost: 143 `E0428` name
// collisions -- two libraries defining `_Painter` -- and rustc had still not
// finished after twenty-five minutes, because every module could see every
// name in the package.
//
// So the imports follow the Dart ones. Kernel keeps each library's
// dependencies, and a `use crate::<module>::*` is written for each of them
// that is inside the package. That is both smaller and more faithful: two
// libraries that never imported each other cannot collide, in Rust for the
// same reason they could not in Dart.

import 'dart:io';

import 'package:kernel/class_hierarchy.dart';
import 'package:kernel/core_types.dart';
import 'package:kernel/kernel.dart';
import 'package:kernel/type_environment.dart';

import '../lib/backend_rust.dart';
import '../lib/ir.dart';
import '../lib/frontend_kernel.dart';
import '../lib/prelude.dart';

/// `package:flutter/src/painting/alignment.dart` -> `painting_alignment`.
///
/// Flat, not nested: a nested module tree would need every `use` to know how
/// far up to go, and nothing here needs the hierarchy.
String moduleName(String uri) {
  // `dart:ui` has no path at all, and `dart:async` would flatten to the same
  // name as an `async.dart` in a package.
  if (uri.startsWith('dart:')) {
    return 'dart_${uri.substring('dart:'.length).replaceAll(RegExp(r'[^A-Za-z0-9]+'), '_')}';
  }
  var path = uri;
  final marker = path.indexOf('/src/');
  path = marker >= 0
      ? path.substring(marker + '/src/'.length)
      : path.substring(path.indexOf('/') + 1);
  path = path.replaceAll('.dart', '');
  final name = path.replaceAll(RegExp(r'[^A-Za-z0-9]+'), '_').toLowerCase();
  return RegExp(r'^[0-9]').hasMatch(name) ? 'm_$name' : name;
}

/// The item names another module could import: the `pub` ones only.
///
/// A glob import never brought a private item across either, so naming one
/// explicitly does not lose anything -- it just says out loud what was already
/// true, instead of 81 `E0603`s.
Set<String> _publicItemsIn(String text) => {
  for (final m in RegExp(
    r'^pub(?:\(crate\))? (?:fn|struct|trait|enum|const|static|type) '
    // `r#break` is one name, not `r` followed by `break`. Without the `r#`
    // the scan recorded a name `r`, every module that used the raw identifier
    // imported it, and `no `r` in ...` was 212 unresolved imports.
    r'((?:r#)?[A-Za-z_]\w*)',
    multiLine: true,
  ).allMatches(text))
    m.group(1)!,
};

/// Every item name a module declares, public or not.
Set<String> _itemsIn(String text) => {
  for (final m in RegExp(
    r'^(?:pub(?:\(crate\))? )?(?:fn|struct|trait|enum|const|static|type) '
    r'((?:r#)?[A-Za-z_]\w*)',
    multiLine: true,
  ).allMatches(text))
    m.group(1)!,
};

/// Every identifier the text uses.
///
/// Deliberately blunt: a word in a comment or a string counts too. Importing a
/// name that turns out to be unused is free -- the file allows unused imports
/// -- and missing one is not, so the net is cast wide.
Set<String> _identifiersIn(String text) => {
  for (final m in RegExp(r'(?:r#)?[A-Za-z_]\w*').allMatches(text)) m.group(0)!,
};

/// Writes only when the text differs.
///
/// Cargo decides what to recheck from file timestamps, so rewriting 525
/// identical files made every run a full run and the incremental cache
/// worthless. A compiler change that touches three modules should cost three
/// modules.
Future<void> _writeIfChanged(String path, String text) async {
  final file = File(path);
  if (file.existsSync() && await file.readAsString() == text) return;
  await file.writeAsString(text);
}

Future<void> main(List<String> args) async {
  if (args.length < 3) {
    stderr.writeln(
      'usage: dart2rust_package.dart <app.dill> <prefix> <out dir>',
    );
    exit(2);
  }
  final component = loadComponentFromBinary(args[0]);
  // Once for the whole component: an enum's variants live in the
  // constants that name them, which can be in any library.
  final (enumValues, enumFields) = enumsIn(component);
  // The program's types, once: `getStaticType` needs a class hierarchy, and
  // building one over 924 libraries is a few seconds, not a few seconds per
  // library.
  final coreTypes = CoreTypes(component);
  final typeEnvironment = TypeEnvironment(
    coreTypes,
    ClassHierarchy(component, coreTypes),
  );
  // A comma-separated list: `package:flutter/,dart:ui`. `dart:ui` holds
  // Color, Offset, Size and Rect, which round 39 measured as the four names
  // the translated package reaches for most and never finds -- and it is in
  // the same dill, with bodies.
  final prefixes = args[1].split(',').where((p) => p.isNotEmpty).toList();
  // Which names are traits, across the whole crate. A library only knows its
  // own, and a trait named without `dyn` was 802 of the errors.
  final abstractNames = abstractClassesIn(component, prefixes)
    // `Object` is every Dart class's base and lives in `dart:core`, which is
    // not translated -- 543 uses of a name nothing declares. The prelude gives
    // it a trait with a blanket impl, so `&dyn Object` accepts anything, and
    // it is listed here so the backend spells it `dyn`.
    ..add('Object')
    // Two prelude traits standing for `dart:core` interfaces, spelled
    // `dyn` like any abstract class: `Comparable<T>` and `Iterator<E>`
    // (`DartIterator`, renamed by the front end).
    ..add('Comparable')
    ..add('DartIterator');
  final prefix = args[1];
  final out = Directory(args[2]);
  await out.create(recursive: true);

  final modules = <String>[];

  /// Each module's text, held until every module is known: what a module has
  /// to import cannot be decided before the others have said what they define.
  final written =
      <String, (String, String, List<String>, List<String>, Set<String>)>{};
  var libraries = 0;
  var classes = 0;
  var refusals = 0;
  final taken = <String, String>{};

  // Names first, so a module can name the ones it imports.
  final inPackage = <Library>[];
  final nameOf = <Library, String>{};
  for (final library in component.libraries) {
    final uri = library.importUri.toString();
    if (!prefixes.any(uri.startsWith)) continue;
    var name = moduleName(uri);
    // Two libraries can flatten to one module name. The second keeps its own
    // file rather than overwriting the first, which is how a whole library
    // would disappear without a word.
    if (taken.containsKey(name)) {
      var n = 2;
      while (taken.containsKey('${name}_$n')) {
        n++;
      }
      name = '${name}_$n';
    }
    taken[name] = uri;
    nameOf[library] = name;
    inPackage.add(library);
  }

  // Lowered once, all of them, before any is emitted. A class needs its base
  // class's fields and constructor to flatten it, and the base is usually in
  // another module -- which is why 1300 classes were refused with "the base is
  // not in this file". In one crate it is.
  final lowered = <Library, (IrLibrary, List<String>)>{};
  final everyClass = <String, IrClass>{};

  /// Each library's own classes, by name.
  ///
  /// `everyClass` keeps the first class it meets under a name, and two
  /// libraries can use the same one: `dart:ui`'s `Gradient` is a concrete
  /// class and `painting`'s is abstract. Asking the crate-wide map whether
  /// `Gradient` is abstract answers for whichever was lowered first, and the
  /// backend then wrote `Option<Gradient>` where it needed
  /// `Option<Box<dyn Gradient>>`. Which one a name means depends on the
  /// library doing the naming, so the lookup has to as well.
  final classesOf = <Library, Map<String, IrClass>>{};
  final everyFunction = <String>{};
  final everyConstant = <String, IrConstDecl>{};

  /// Which modules define each class name. Ten names are defined by more than
  /// one -- `TextStyle`, `Image`, `Path`, `Gradient` and `StrutStyle` each come
  /// once from `dart:ui` and once from `painting` -- and a glob import of both
  /// makes every use of them ambiguous. 800 `E0659`s from ten names.
  final definedIn = <String, Set<String>>{};
  final dynamicSlots = typeEnvironment == null
      ? const <Field, List<InterfaceType>>{}
      : dynamicSlotsIn(inPackage, typeEnvironment);
  for (final library in inPackage) {
    final result = KernelFrontend(
      library,
      enumValues: enumValues,
      enumFields: enumFields,
      abstractElsewhere: abstractNames,
      typeEnvironment: typeEnvironment,
      dynamicSlots: dynamicSlots,
    ).lowerLibrary();
    lowered[library] = result;
    for (final cls in result.$1.classes) {
      everyClass.putIfAbsent(cls.name, () => cls);
      (classesOf[library] ??= <String, IrClass>{})[cls.name] = cls;
    }
    everyFunction.addAll(result.$1.functions.map((f) => f.name));
    for (final c in result.$1.constants) {
      everyConstant.putIfAbsent(c.name, () => c);
    }
    for (final cls in result.$1.classes) {
      (definedIn[cls.name] ??= <String>{}).add(nameOf[library]!);
    }
  }

  for (final library in inPackage) {
    final uri = library.importUri.toString();
    final name = nameOf[library]!;

    // What the library *names*, not what it declared it imports.
    //
    // `library.dependencies` looked like the import graph and is not one: the
    // CFE resolves flutter's barrel libraries away -- there are none in the
    // dill -- without splicing their re-exports into the importer. So
    // `cupertino/nav_bar.dart` depends on no painting library while using
    // `TextStyle` 348 times. See `librariesReferencedBy`.
    final imports = <String>{};
    final exports = <String>{};
    for (final referenced in librariesReferencedBy(library)) {
      final target = nameOf[referenced];
      if (target != null && target != name) imports.add(target);
    }
    for (final dependency in library.dependencies) {
      final target = nameOf[dependency.targetLibrary];
      if (target == null || target == name) continue;
      if (!dependency.isExport) {
        // The Dart file's own `import` lines, beside the references: a
        // default argument the compiler filled in (`VerticalDirection.down`
        // for `Column`) names a class the source never wrote, reached only
        // through `material.dart`'s re-exports.
        if (!exports.contains(target)) imports.add(target);
        continue;
      }
      // Dart's `export` is a re-export, and a library importing this one gets
      // what it exports. The edges that survive are still worth keeping.
      exports.add(target);
      imports.remove(target);
    }

    final (own, refused) = lowered[library]!;
    // The same IR, plus a way to look up the rest of the crate.
    final ir = IrLibrary(
      own.classes,
      constants: own.constants,
      functions: own.functions,
      abstractElsewhere: abstractNames,
      constantsElsewhere: everyConstant,
      elsewhere: {
        ...everyClass,
        // What *this* library's names resolve to wins over the crate-wide
        // first-come map.
        for (final entry in classNamesReferencedBy(library).entries)
          if (entry.value.length == 1 &&
              classesOf[entry.value.single]?[entry.key] != null)
            entry.key: classesOf[entry.value.single]![entry.key]!,
      },
      functionsElsewhere: everyFunction,
    );
    final (text, more) = RustBackend.emitLibrary(ir, frontEndRefusals: refused);
    // Counted from what is written, not from what the two lists happen to
    // hold. Rounds 53 and 54 wrapped more emission sites in `_member`, and
    // those refusals reach the file without reaching `more` -- so this said
    // 2758 for a file that carried 3337. A ruler and the thing it measures
    // have to be the same thing.
    refusals += '// NOT TRANSLATED:'.allMatches(text).length;
    classes += ir.classes.length;
    libraries++;
    modules.add(name);
    // A name two imported modules both define is ambiguous under globs, and
    // Rust says so at every use. An explicit `use` outranks a glob, so the one
    // the Dart actually meant is named. Skipped when this module defines the
    // name itself -- its own item already outranks both globs, and a `use`
    // beside it would be a redefinition.
    final resolved = <String>[];
    final ownNames = {for (final cls in own.classes) cls.name};
    classNamesReferencedBy(library).forEach((className, from) {
      if ((definedIn[className]?.length ?? 0) < 2) return;
      if (ownNames.contains(className)) return;
      final owners = from.map((l) => nameOf[l]).nonNulls.toSet();
      // `TextStyle` and `Image` exist in `dart:ui` and again in the
      // framework, and a library that names both -- `painting/text_style.dart`
      // itself -- cannot import either by its bare name. The framework's is
      // the one the *text* means at nearly every site (the `dart:ui` one is
      // written `ui.TextStyle`, and the prefix is gone here); importing it
      // turns 20 "cannot find" into a type mismatch at the few `ui.` sites,
      // which rustc still reports.
      final String owner;
      if (owners.length == 1) {
        owner = owners.single;
      } else if (owners.length == 2 && owners.contains('dart_ui')) {
        owner = owners.firstWhere((o) => o != 'dart_ui');
      } else {
        return;
      }
      if (owner == name) return;
      // Resolved from the Kernel reference, so neither the Dart import list
      // nor the name's privacy is asked: the tree shaker inlines a library's
      // constants into libraries that never imported it, and three libraries
      // each declare a `_UnspecifiedTextScaler` -- the reference says which.
      // A private class is `pub(crate)` here, so the import is a real one.
      // 15 `E0422`s.
      resolved.add('use crate::$owner::$className;');
    });
    written[name] = (
      uri,
      text,
      exports.toList()..sort(),
      resolved..sort(),
      imports,
    );
  }

  // The `use` lines, decided from the emitted Rust rather than from the Dart.
  //
  // Every module used to open every module it referenced with
  // `use crate::X::*`. That compiles, and it costs: `-Ztime-passes` puts 72 of
  // a 73-second `cargo check` inside `resolve_crate`, because with hundreds of
  // modules glob-importing each other every name is looked for in an enormous
  // scope -- and every error message then searches that scope again for
  // something to suggest, which was 30 of those seconds by itself.
  //
  // What a module needs is knowable exactly: the identifiers in the text it
  // emitted. Deciding from the text rather than from the Dart AST is the
  // point -- the text is the thing that has to compile, so no name can arrive
  // by a route the importer did not think of.
  // Every module that defines each public item. One definer: import from it.
  // Several: this used to give up, and `default_target_platform` -- defined
  // by `platform.dart` *and* by the `_platform_io.dart` it wraps -- went
  // unimported 73 times. The Dart import graph settles it the way it settles
  // the classes above: of the definers, the one this library imports is the
  // one it meant.
  // The abstract classes above a class, by name, through superclass,
  // mixins and interfaces -- the traits its values' methods live in.
  final ancestorsCache = <String, Set<String>>{};
  Set<String> abstractAncestors(String className) {
    final cached = ancestorsCache[className];
    if (cached != null) return cached;
    final out = <String>{};
    ancestorsCache[className] = out;
    final cls = everyClass[className];
    if (cls == null) return out;
    for (final above in [
      cls.superclass,
      for (final m in cls.mixins) m.name,
      for (final i in cls.interfaces) i.name,
    ]) {
      if (above == null) continue;
      final aboveClass = everyClass[above];
      if (aboveClass == null) continue;
      if (aboveClass.isAbstract) out.add(above);
      out.addAll(abstractAncestors(above));
    }
    return out;
  }

  final definers = <String, Set<String>>{};
  for (final entry in written.entries) {
    for (final item in _publicItemsIn(entry.value.$2)) {
      (definers[item] ??= {}).add(entry.key);
    }
  }

  for (final entry in written.entries) {
    final name = entry.key;
    final (uri, text, exports, resolved, imports) = entry.value;
    final mine = _itemsIn(text);
    // What the Dart import graph makes visible: the imports, and what
    // each imported module re-exports, transitively -- `VerticalDirection`
    // reaches a widget through `flutter/widgets.dart`, which exports
    // `rendering/flex.dart` (164 "cannot find type"). Still the Dart
    // graph, so no new module cycle.
    final visible = <String>{};
    void see(String m) {
      if (!visible.add(m)) return;
      for (final e in written[m]?.$3 ?? const <String>[]) {
        see(e);
      }
    }

    imports.forEach(see);
    // What `resolved` (the class path) already imports by name, so the same
    // name is not imported twice -- 64 `E0252`s, `Path` 41 of them.
    final already = {
      for (final line in resolved)
        line.substring(line.lastIndexOf(':') + 1, line.length - 1),
    };
    final wanted = <String, String>{};
    final used = _identifiersIn(text).toSet();
    for (final used in used) {
      if (mine.contains(used)) continue;
      // `_` is a pattern, not a name. A *private* name (`_Linear`) is
      // imported when exactly one module defines it: Dart's library-private
      // is `pub(crate)` here, and the tree shaker inlines constants across
      // libraries -- `_AlwaysDismissedAnimation {}` turned up in
      // `package:animations`, 250 `E0422`s and the top of the E0425 list.
      if (used == '_') continue;
      if (already.contains(used)) continue;
      final candidates = definers[used];
      if (candidates == null) continue;
      // Only from a module this library reaches in the Dart graph. Text
      // alone imported `locale` from the gallery's formatters into
      // `widgets/basic.dart` for a *parameter* of that name, and `Dialog`
      // and `MenuItem` from material and the gallery into `dart:ui` -- edges
      // no Dart import made. rustc read them as unused imports; the module
      // graph read them as one 450-module cycle, 27% of the crate, with
      // `dart:ui` and `package:gallery` in it. Measured 2026-09-03.
      final imported = candidates.where(visible.contains).toList();
      if (imported.length != 1) continue;
      final from = imported.single;
      if (from == name) continue;
      wanted[used] = from;
    }
    // A class's abstract ancestors too: `rrect.left()` is `_RRectLike`'s
    // accessor, and a method of a trait not in scope does not exist
    // (209 "no method named", `_RRectLike` alone 70). The text names the
    // class, never the trait, so the trait comes with the class.
    for (final cls in used) {
      for (final ancestor in abstractAncestors(cls)) {
        if (mine.contains(ancestor) ||
            already.contains(ancestor) ||
            wanted.containsKey(ancestor)) {
          continue;
        }
        final candidates = definers[ancestor];
        if (candidates == null || candidates.length != 1) continue;
        final from = candidates.single;
        if (from == name) continue;
        wanted[ancestor] = from;
      }
    }
    final byModule = <String, List<String>>{};
    for (final e in wanted.entries) {
      (byModule[e.value] ??= []).add(e.key);
    }
    final uses = [
      'use crate::dart_prelude::*;',
      for (final m in exports) 'pub use crate::$m::*;',
      for (final m in byModule.keys.toList()..sort())
        'use crate::$m::{${(byModule[m]!..sort()).join(', ')}};',
      ...resolved,
    ].join('\n');
    await _writeIfChanged(
      '${out.path}/$name.rs',
      '// Generated from $uri\n'
          '//\n'
          '// The `use` lines name exactly what this module\'s own text uses:\n'
          '// see dart2rust_package.dart for what the glob imports cost.\n'
          '#![allow(unused_imports, dead_code, non_snake_case)]\n'
          '$uses\n'
          '\n'
          '$text',
    );
  }

  final lib = StringBuffer()
    ..writeln('// Generated by tools/dart2rust from $prefix')
    ..writeln('//')
    ..writeln('// One module per Dart library. The modules import each other')
    ..writeln('// the way the Dart libraries did, so two that never met')
    ..writeln('// cannot collide.')
    ..writeln('#![allow(unused_imports, dead_code, non_snake_case)]')
    ..writeln();
  lib.writeln('pub mod dart_prelude;');
  for (final name in modules) {
    lib.writeln('pub mod $name;');
  }
  await _writeIfChanged('${out.path}/lib.rs', lib.toString());
  await _writeIfChanged('${out.path}/dart_prelude.rs', rustPrelude);

  // Anything left from an earlier run goes. `_writeIfChanged` only writes, so
  // a module that stopped being emitted -- because the dill changed, or the
  // prefix did -- stayed on disk and kept being counted: 931 files and 3347
  // refusals reported for a run that emitted 920 and 2758. `cargo` never saw
  // them, since `lib.rs` did not name them, but every ruler that reads the
  // directory did.
  final wrote = {
    'lib.rs',
    'dart_prelude.rs',
    for (final name in modules) '$name.rs',
  };
  for (final entry in out.listSync()) {
    final base = entry.path.split(RegExp(r'[/\\]')).last;
    if (entry is File && base.endsWith('.rs') && !wrote.contains(base)) {
      await entry.delete();
    }
  }

  stdout.writeln('$prefix -> ${out.path}');
  stdout.writeln(
    '  $libraries libraries, $classes classes, '
    '$refusals refusals',
  );
}
