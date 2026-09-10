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
import 'package:kernel/binary/ast_from_binary.dart';
import 'package:kernel/type_environment.dart';

import '../lib/backend_rust.dart';
import '../lib/ir.dart';
import '../lib/frontend_kernel.dart';
import '../lib/alias_mutation.dart';
import '../lib/covariance.dart';
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

/// How the compiler spells a top-level member.
///
/// The one place the ledger has to cross into naming: a Dart name is not a
/// Rust one, and `referencesOf` deliberately hands back the members rather
/// than guessing at the spelling itself. A field is a module constant
/// (`screamingSnake`); a getter, a setter and a plain function are all free
/// functions here, since reading a computed `get foo` is calling it (see
/// `_staticGet`) -- named by the front end's own `topLevelIrName`, so an
/// import and a declaration cannot disagree.
String _rustNameOf(Member member) => member is Field
    ? screamingSnake(member.name.text)
    : snake(KernelFrontend.topLevelIrName(member));

/// The item names another module could import: the `pub` ones only.
///
/// A glob import never brought a private item across either, so naming one
/// explicitly does not lose anything -- it just says out loud what was already
/// true, instead of 81 `E0603`s.
Set<String> _publicItemsIn(String text) => {
  for (final m in RegExp(
    // `pub async fn` and `pub const fn` are items too: an `async` free
    // function was never a definer, and every module calling one imported
    // nothing for it (`haptic_feedback_selection_click`, ws336).
    r'^pub(?:\(crate\))? (?:(?:async|const|unsafe) )*(?:fn|struct|trait|enum|const|static|type) '
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
  // Read the whole dill, bodies included, before anything looks at it.
  //
  // `loadComponentFromBinary` leaves every function body behind a
  // `lazyBuilder` that the first reader of `FunctionNode.body` runs. That is
  // meant to be invisible, and it is not: what this compiler emits depends on
  // whether the bodies were all read before lowering started. Measured at
  // ws889, on the same dill and the same sources, the only difference being
  // when the bodies were read -- 32,653 lines of the 926 modules, of which
  // 32,026 are a `?` that is there when the bodies were read up front and
  // gone when they were not, and 627 are wider than that: a call that comes
  // out `<AnimationController as Animation<f64>>::drive::<f64>(..)` one way
  // and `.drive(..)` the other, a turbofish that is `then::<()>` one way and
  // `then::<(), _>` the other. The eager reading is the one every output
  // since ws885 was measured against.
  //
  // *Why* it differs is not known yet, and this is not the explanation --
  // it is the reproduction, and the guard. Until ws889 the reading was done
  // by accident: `ThrowsAnalysis.of` walked every member of every translated
  // library, and its answers were read by one line of `KernelFrontend`, a
  // null check. Deleting an analysis that decided nothing moved 32,653 lines
  // of output, which is how this was found.
  final component = Component();
  BinaryBuilder(
    File(args[0]).readAsBytesSync(),
    disableLazyReading: true,
  ).readComponent(component);
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
  // Open classes: concrete classes with translated subclasses, emitted as
  // a trait beside a struct for their own instances (STATUS, ws265). Gated
  // by name while it is measured; `DART2RUST_OPEN=all` opens every one.
  final openClasses = openClassesIn(
    component,
    prefixes,
    Platform.environment['DART2RUST_OPEN'] ?? defaultOpenClasses,
  );
  stderr.writeln('open classes: ${openClasses.length}');
  final abstractNames = abstractClassesIn(component, prefixes)
    ..addAll(openClasses.map((c) => c.name))
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
  final written = <String, (String, String, List<String>)>{};

  /// Each module's `use` lines as the Kernel references give them: the Rust
  /// name, and the module that declares it. Built in the emit loop from
  /// `referencesOf`, read in the import loop below.
  final ledger = <String, Map<String, Set<String>>>{};

  /// What the *emitter* wrote that it did not declare, per module
  /// (`RustBackend.namedElsewhere`): the names no Kernel node carries.
  final emitted = <String, Set<String>>{};
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
  final dynamicSlots = dynamicSlotsIn(inPackage, typeEnvironment);
  // The census behind a dynamic member access, printed rather than used
  // while its shape is being decided: how many names a program really asks
  // of a `dynamic`, and how many classes would have to answer each.
  final dynamicMembers = dynamicMembersIn(component.libraries);
  // The classes whose identity the program asks about (`identityObservedIn`).
  // Only these carry a token, and only the value ones need it -- a counted
  // class already has an address.
  final identityObserved = identityObservedIn(
    component.libraries,
    typeEnvironment,
  );
  if (Platform.environment['DART2RUST_TRACE_DYNMEMBER'] == '1') {
    final rows = dynamicMembers.entries.toList()
      ..sort((a, b) => b.value.length.compareTo(a.value.length));
    stderr.writeln('TRACE_DYNMEMBER names=${rows.length}');
    for (final row in rows) {
      stderr.writeln(
        'TRACE_DYNMEMBER ${row.key} classes=${row.value.length} '
        '${row.value.take(6).map((c) => c.name).join(",")}',
      );
    }
  }
  // The closed world's instantiations of generic traits, gathered while
  // every library is lowered and read back for the wider impls
  // (`KernelFrontend.addWiderImpls`).
  final instantiations = <Class, Set<InterfaceType>>{};
  final frontends = <Library, KernelFrontend>{};
  // Where a mixin's bodies went: the CFE applies a mixin by copying its
  // members into an anonymous application class and leaves the declaration
  // hollow (abstract). Any application will do -- they are copies -- and
  // the first found is the one a mixin's trait takes its defaults from
  // (`KernelFrontend.applications`).
  // Not every anonymous mixin class is an application: a mixin
  // declaration's `on` clause is held by one too (`mixin ServicesBinding
  // on BindingBase, SchedulerBinding` sits on `Object&BindingBase&
  // SchedulerBinding`), and mixin deduplication leaves ones nothing
  // extends. Neither holds bodies, and neither puts the mixin over
  // anything (`_appliedOver`); both are left out.
  final applications = <Class, List<Class>>{};
  final holders = <Class>{};
  final extended = <Class>{};
  for (final library in component.libraries) {
    for (final cls in library.classes) {
      final base = cls.superclass;
      if (base == null) continue;
      extended.add(base);
      if (cls.isMixinDeclaration && base.isAnonymousMixin) holders.add(base);
    }
  }
  for (final library in component.libraries) {
    for (final cls in library.classes) {
      if (!cls.isAnonymousMixin) continue;
      if (holders.contains(cls) || !extended.contains(cls)) continue;
      for (final applied in cls.implementedTypes) {
        (applications[applied.classNode] ??= []).add(cls);
      }
    }
  }
  // Class names two translated libraries both declare (`TextStyle`,
  // `StrutStyle`, `Gradient`, `Image` in dart:ui and in flutter's painting
  // and widgets): a reference from another library is spelled by module
  // (`crate::dart_ui::StrutStyle`), since by the bare name whichever the
  // module imported won (run679).
  // ..a *private* name too: it is library-local upstream and `pub(crate)`
  // here, one module each, so a reference from another module resolves by
  // whatever that module imported. Three libraries declare an
  // `_UnspecifiedTextScaler`, and `TextPainter`'s default `const
  // _UnspecifiedTextScaler()` named none of them (`_SwitchPainter`,
  // run700).
  final classNameCount = <String, int>{};
  for (final library in inPackage) {
    for (final cls in library.classes) {
      if (cls.isAnonymousMixin) continue;
      classNameCount[cls.name] = (classNameCount[cls.name] ?? 0) + 1;
    }
  }
  final collidingClassNames = {
    for (final e in classNameCount.entries)
      if (e.value > 1) e.key,
  };
  // The classes mutated through an alias (`alias_mutation.dart`): counted.
  // ..with the deduplicated mixin applications' bodies scanned too: a
  // hollow mixin's methods live there (`LocalHistoryRoute.addLocalHistory
  // Entry` writing `entry._owner`, run672), and the census saw none of
  // them.
  final aliasScanned = [
    ...inPackage,
    ...component.libraries.where(
      (l) => l.importUri.toString() == 'dart:mixin_deduplication',
    ),
  ];
  final aliasMutated = aliasMutatedClasses(aliasScanned);
  // The type parameters used covariantly (`covariance.dart`): erased.
  // ..over `aliasScanned`, not `inPackage`: a deduplicated mixin
  // application's body is where a hollow mixin's methods live, and it is in
  // `dart:mixin_deduplication`, whose uri no prefix matches. The alias
  // census learned this at run672 and this one did not --
  // `SchedulerBinding.scheduleTask` adds a `_TaskEntry<T>` to a
  // `PriorityQueue<_TaskEntry<dynamic>>` and the scan never saw the member
  // at all (`DART2RUST_TRACE_FLOW=@scheduleTask` printed nothing, ws953).
  final covariant = covariantParameters(aliasScanned, typeEnvironment);
  for (final library in inPackage) {
    final frontend = KernelFrontend(
      library,
      aliasMutated: aliasMutated,
      covariantParameters: covariant,
      enumValues: enumValues,
      enumFields: enumFields,
      abstractElsewhere: abstractNames,
      collidingClassNames: collidingClassNames,
      typeEnvironment: typeEnvironment,
      dynamicSlots: dynamicSlots,
      dynamicMembers: dynamicMembers,
      identityObserved: identityObserved,
      open: openClasses,
      erase: Platform.environment['DART2RUST_ERASE'] != '0',
      eraseObjectBounded: Platform.environment['DART2RUST_ERASE_OBJECT'] == '1',
      coerceByType: Platform.environment['DART2RUST_COERCE'] != '0',
      instantiations: instantiations,
      applications: applications,
      moduleOf: nameOf,
    );
    final result = frontend.lowerLibrary();
    frontends[library] = frontend;
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

  // What each library can name: itself and everything it references,
  // transitively. A wider impl may only be written where the types it
  // names are visible (`addWiderImpls`).
  final references = <Library, Set<Library>>{
    for (final library in inPackage)
      library: referencesOf(library, applications: applications).libraries,
  };
  final reachable = <Library, Set<Library>>{};
  for (final library in inPackage) {
    final seen = <Library>{library};
    final work = [library];
    while (work.isNotEmpty) {
      for (final next in references[work.removeLast()] ?? const <Library>{}) {
        if (seen.add(next)) work.add(next);
      }
    }
    reachable[library] = seen;
  }
  for (final library in inPackage) {
    frontends[library]!.addWiderImpls(
      lowered[library]!.$1,
      reachable[library]!,
    );
  }
  // A static whose value's field is written anywhere is mutable state and
  // lives in a cell, wherever it is declared (`staticFieldWrites`).
  IrConstDecl inCell(IrConstDecl c) => IrConstDecl(
    c.name,
    c.type,
    c.value,
    doc: c.doc,
    isLazy: c.isLazy,
    isMutable: true,
  );
  for (final key in KernelFrontend.staticFieldWrites) {
    final dot = key.indexOf('.');
    final owner = key.substring(0, dot);
    final name = key.substring(dot + 1);
    for (final result in lowered.values) {
      final ir = result.$1;
      if (owner.isEmpty) {
        for (var i = 0; i < ir.constants.length; i++) {
          if (ir.constants[i].name == name && !ir.constants[i].isMutable) {
            ir.constants[i] = inCell(ir.constants[i]);
          }
        }
        continue;
      }
      for (final cls in ir.classes) {
        if (cls.name != owner) continue;
        for (var i = 0; i < cls.constants.length; i++) {
          if (cls.constants[i].name == name && !cls.constants[i].isMutable) {
            cls.constants[i] = inCell(cls.constants[i]);
          }
        }
      }
    }
    if (owner.isEmpty) {
      final c = everyConstant[name];
      if (c != null && !c.isMutable) everyConstant[name] = inCell(c);
    }
  }
  KernelFrontend.dumpUntyped();

  for (final library in inPackage) {
    final uri = library.importUri.toString();
    final name = nameOf[library]!;

    // What the library *names*, not what it declared it imports.
    //
    // `library.dependencies` looked like the import graph and is not one: the
    // CFE resolves flutter's barrel libraries away -- there are none in the
    // dill -- without splicing their re-exports into the importer. So
    // `cupertino/nav_bar.dart` depends on no painting library while using
    // `TextStyle` 348 times. See `referencesOf`.
    final refs = referencesOf(library, applications: applications);
    // What the Kernel references say, in Rust's spelling: the name a module
    // has to import, and the module that declares it. The emitter knows this
    // and used to throw it away, leaving `use` lines to be guessed back out
    // of the text it had just written. See `_ReferenceCollector.namedMembers`.
    final book = <String, Set<String>>{};
    void note(String rustName, Library owner) {
      final module = nameOf[owner];
      if (module == null || module == name) return;
      (book[rustName] ??= {}).add(module);
    }

    refs.classNames.forEach((className, from) {
      for (final owner in from) {
        note(className, owner);
        // The concrete struct an open class gets (`KernelFrontend.implName`):
        // the Kernel node says `Color`, and the emitter writes `ColorImpl`.
        note(KernelFrontend.implName(className), owner);
      }
    });
    for (final member in refs.members) {
      final owner = member.enclosingClass;
      if (owner == null) {
        note(_rustNameOf(member), member.enclosingLibrary);
        continue;
      }
      // An instance member is reached through its trait, which comes with the
      // class. A static is a name of its own here, and which of the emitter's
      // spellings it got is what the module defines (`staticNamesFor`).
      if (member.isInstanceMember) continue;
      for (final proposed in RustBackend.staticNamesFor(
        owner.name,
        member.name.text,
        isSetter: member is Procedure && member.kind == ProcedureKind.Setter,
      )) {
        note(proposed, member.enclosingLibrary);
      }
    }
    // What a census made this library name (`injectedClasses`): a wider
    // impl's types and a dynamic slot's arms are whole-program answers, and
    // no Kernel node in this library carries them.
    for (final cls in frontends[library]!.injectedClasses) {
      note(cls.name, cls.enclosingLibrary);
      note(KernelFrontend.implName(cls.name), cls.enclosingLibrary);
    }
    for (final member in frontends[library]!.injectedMembers) {
      if (member.enclosingClass != null) continue;
      note(_rustNameOf(member), member.enclosingLibrary);
    }
    // The generic calls that went to a body rather than through a trait
    // (`_genericOnTrait`): the front end wrote down which class holds it.
    for (final (body, member) in frontends[library]!.genericBodies) {
      note(RustBackend.superFn(body.name, member), body.enclosingLibrary);
    }
    for (final (owner, member, isSetter) in frontends[library]!.superOwners) {
      note(
        RustBackend.superFn(owner.name, member, isSetter: isSetter),
        owner.enclosingLibrary,
      );
      // ..and the class itself: the free function's `__Self` is bound by the
      // traits its body reaches through `super` (`_superBoundTraits`), which
      // is how `scheduler_binding_super_init_instances` names
      // `GestureBinding` -- a trait `SchedulerBinding` is not below.
      note(owner.name, owner.enclosingLibrary);
    }
    ledger[name] = book;

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
        for (final entry in refs.classNames.entries)
          if (entry.value.length == 1 &&
              classesOf[entry.value.single]?[entry.key] != null)
            entry.key: classesOf[entry.value.single]![entry.key]!,
      },
      functionsElsewhere: everyFunction,
      byModule: {
        for (final e in classesOf.entries)
          if (nameOf[e.key] != null) nameOf[e.key]!: e.value,
      },
    );
    final (text, more) = RustBackend.emitLibrary(ir, frontEndRefusals: refused);
    final wrote = {...RustBackend.namedElsewhere};
    void header(IrType type) {
      wrote.add(type.name);
      type.arguments.forEach(header);
    }

    for (final cls in own.classes) {
      final above = cls.superclass;
      if (above != null) wrote.add(above);
      cls.interfaces.forEach(header);
      cls.mixins.forEach(header);
      cls.extraImpls.forEach(header);
    }
    emitted[name] = wrote;
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
    refs.classNames.forEach((className, from) {
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
    written[name] = (uri, text, resolved..sort());
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
    final (uri, text, resolved) = entry.value;
    final mine = _itemsIn(text);
    // What `resolved` (the class path) already imports by name, so the same
    // name is not imported twice -- 64 `E0252`s, `Path` 41 of them.
    final already = {
      for (final line in resolved)
        line.substring(line.lastIndexOf(':') + 1, line.length - 1),
    };
    // What the module imports, from the ledger.
    final wanted = <String, String>{};
    void take(String used, String from) {
      if (from == name) return;
      if (mine.contains(used)) return;
      if (already.contains(used)) return;
      // The ledger says which library *declared* the name; whether that
      // library's module emitted it is the definition scan's answer, and an
      // import of a name a module does not define is an unresolved import.
      if (!(definers[used]?.contains(from) ?? false)) return;
      wanted[used] = from;
    }

    for (final used in emitted[name] ?? const <String>{}) {
      final owners = definers[used];
      if (owners != null && owners.length == 1) take(used, owners.single);
    }
    ledger[name]!.forEach((used, owners) {
      // Two libraries reached with something of this name: which of them the
      // crate *defines* it in settles it, and when both do, the explicit
      // `resolved` line above has already named the one that was meant.
      final defined = owners
          .where((m) => definers[used]?.contains(m) ?? false)
          .toSet();
      if (defined.length != 1) return;
      final from = defined.single;
      take(used, from);
      // A class's abstract ancestors too: `rrect.left()` is `_RRectLike`'s
      // accessor, and a method of a trait not in scope does not exist
      // (209 "no method named", `_RRectLike` alone 70).
      for (final ancestor in abstractAncestors(used)) {
        final owners = definers[ancestor];
        if (owners != null && owners.length == 1) take(ancestor, owners.single);
      }
    });
    final byModule = <String, List<String>>{};
    for (final e in wanted.entries) {
      (byModule[e.value] ??= []).add(e.key);
    }
    final uses = [
      'use crate::dart_prelude::*;',
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

  stdout.writeln('  Iterable slots spelled: $iterableSlots');
  final members = iterableMembers.entries.toList()
    ..sort((a, b) => b.value.compareTo(a.value));
  stdout.writeln(
    '  Iterable members called (${members.length} names): '
    '${members.map((e) => '${e.key}=${e.value}').join(' ')}',
  );
  stdout.writeln('$prefix -> ${out.path}');
  stdout.writeln(
    '  $libraries libraries, $classes classes, '
    '$refusals refusals',
  );
}
