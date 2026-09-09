# dart2rust

A Dart compiler with a Rust backend, in the shape `dart2wasm` has: a resolved
front end, a small IR, and a backend that emits the target language -- plus the
runtime that output runs on, which is a plain Dart VM written in Rust.

The goal, as of 2026-09-03: **run `~/gallery_upstream` -- the real
flutter/gallery -- on the upstream Flutter engine, in the position AOT mode
occupies.** The engine is not modified. What gets replaced is the two halves on
the app's side of it:

| AOT mode's two halves | Upstream | Here |
|---|---|---|
| the compiled code | `libapp.so` from `gen_snapshot`, holding `kDartSnapshotData` and `kDartSnapshotText`, loaded by `Dart_LoadELF` | the Rust crate dart2rust emits |
| the runtime | `libdart` -- the Dart VM, `dart_component_kind = "static_library"`, linked into the engine | a plain Dart VM, written in Rust |

Translating the whole framework is still the work; the runtime is what was
added. Translated Rust is not a self-contained program: something has to give it
an object model, `dart:core`, an event loop, exceptions, type tests, and both
directions of `dart:ui`. Upstream all of that lives in the VM and the snapshot.
Here it is written in Rust and **wired into the modules dart2rust generates**.

This is **not** the hand port that lives in `src/flutter/rust/rustflutter`. That
one re-expresses upstream in idiomatic Rust, choosing a different structure
where Rust wants one (`RenderProxyBox` is dissolved; `RenderState` lives on the
handle, not the object) and recording why at each divergence. This one
translates. The two answer different questions and their outputs do not merge --
see PORTING_STATUS.md for the argument.

## The slot, measured

`bin/embedder_api.py` reads the engine checkout and reports what will be asked
of whatever stands in libdart's place. Against `0c2d270c5a9`, built
`out/host_profile`, linux x64, `flutter_runtime_mode = "profile"` -- profile is
AOT:

- **168** distinct `Dart_*` functions actually called by the engine, across 945
  call sites, out of 312 the C API headers declare. The loading points are few:
  `Dart_LoadELF` once, `Dart_Initialize` twice, `Dart_CreateIsolateGroup` five
  times, `Dart_SetFfiNativeResolver` once.
- **231** `dart:ui` natives the app calls down through (`dart_ui.cc`'s
  `FFI_FUNCTION_LIST` 57 + `FFI_METHOD_LIST` 174).
- **19** `PlatformConfiguration` handles the engine calls back up through --
  begin frame, pointer packets, window metrics.

That number is an upper bound: it counts call sites, not the ones a headless run
reaches. Narrowing it to the boot path is a round of its own.

The alternative was a thin embedder-side ABI, which the `rustflutter` line
already proved can run the whole gallery
(`src/flutter/runtime/rust_app_api.h`, 539 lines). It is much cheaper and it is
not this: it is a *modified* engine, and upstream's AOT mode is the thing being
aimed at. If the `Dart_*` route stalls, falling back to that is allowed --
saying so out loud is not.

## The runtime crate

`tools/dart2rust/runtime/` exists: 547 lines of Rust, and `embedder_api.py`
counts them against the 168 `Dart_*` functions above. **An earlier version of
this file said it did not exist yet, and said the prelude was 1248 lines. Both
were true when they were written and neither was updated** -- the prelude is
10,519 lines of Rust now, and it is still emitted as a string alongside the
generated code (`lib/prelude.dart`) rather than compiled from `runtime/`.

Round 44 measured why the prelude is hand-written rather than translated:
feeding the dill's own `dart:core` through the translator took the error count
from 6608 to 16955, because its members are `external` and come out as empty
traits. That measurement stands.

## The front end: Kernel

`dart2wasm` consumes Kernel (`.dill`), which is the right input: resolved,
desugared, constant-evaluated, and -- the part that matters for shipping -- a
whole *program* rather than a pile of files. An app.dill is what the toolchain
actually builds and what a release would be translated from.

`lib/frontend_kernel.dart` is that front end and is the one everything uses:
`bin/dart2rust_package.dart`, which `bin/run_chain.sh` runs, goes through it,
and so does every fixture. `package:kernel` comes from the engine checkout
(`bin/dill.py` finds it); the SDK cache's own dill is a revision behind and
fails with `Unexpected Kernel Format Version`.

**`lib/frontend.dart`, the analyzer front end this started with, no longer
compiles.** `dart analyze` reports 49 errors in it, every one API drift against
the current analyzer element model (`ClassDeclaration.name`, `isSynthetic`,
`DefaultFormalParameter`, `NamedExpression`). So do its two drivers,
`bin/dart2rust.dart` and `bin/census.dart`. What that costs: `bin/regen.py`
regenerates `testdata/src/*.rs` through `dart2rust.dart`, so only
`constinstance.rs` -- the one file that comes from the Kernel side -- can be
regenerated today. It is left in the tree rather than deleted because deleting
a second front end is a decision about the project, not a cleanup; but nothing
should be added to it, and `bin/check.sh` exempts it by name.

What analyzer gave that Kernel does not: source-shaped output, easier to read
and to check against upstream by eye. What Kernel gives that analyzer does not:
the whole linked program, mixins applied, `async` lowered, implicit coercions
explicit, and reachability -- so a release translates what the app uses instead
of every class in the framework.

## Layout

    lib/ir.dart            the IR. Knows nothing about Kernel or about Rust.
    lib/frontend_kernel.dart  Kernel -> IR. The front end in use. One class
      frontend_kernel/       spread over 19 files; none over 1,460 lines.
    lib/frontend.dart      analyzer -> IR. Superseded, and does not compile.
    lib/backend_rust.dart  IR -> Rust source. One class over 20 files.
    lib/coerce.dart        one rule for a value entering a slot, both ways
    lib/covariance.dart    where an override widens what a slot takes
    lib/throws.dart        which members can fail, over the whole program
    lib/alias_mutation.dart  which classes are mutated through an alias
    lib/member_names.dart  the members that change their receiver
    lib/prelude.dart       the hand-written `dart:core` subset the output needs
    runtime/               the Rust VM
    bin/dart2rust_package.dart  the driver: a dill in, a crate workspace out
    bin/run_chain.sh       the ruler: how much translates and compiles
    bin/fx.sh              the other ruler: whether Rust and Dart agree
    bin/run_main.sh        runs the translated gallery headlessly
    bin/check.sh           formatting, the analyzer and the unit tests
    bin/fmt.py             `dart format`, around dart_style's `augment` gap
    bin/experiments.sh     the one place the Dart experiments are named
    bin/embedder_api.py    what the engine asks of whatever replaces libdart
    testdata/              33 fixtures and the Rust they translate to
    STATUS.md              every round, with its numbers

## Running

Once per machine, so that `dart analyze` and the tests can resolve imports:

    python3 bin/devsetup.py

There is deliberately **no `pubspec.yaml`**. With one present, `dart run` and
`dart test` perform an implicit `pub get` that overwrites
`.dart_tool/package_config.json` with a resolution that has no
`package:kernel` in it, and the compiler then cannot start. `package:kernel`
is not a published package at the revision this reads -- it comes from the
engine checkout -- so resolution is written, not resolved.

The two rulers:

    bin/run_chain.sh $S/stubs.txt $S/ws.log    # translate + 9 cargo rounds
    bin/fx.sh <fixture>                        # one fixture, both ends, diff

`cargo` runs in both, and two `cargo check`s side by side once took the whole
WSL2 VM down -- so they must not run at the same time. `run_chain.sh` watches
`MemAvailable` and kills the compile rather than the machine;
`DART2RUST_MIN_FREE_GB` is the floor.

The compiler itself, on one library:

    dart run --packages=<config> bin/dart2rust_package.dart <app.dill> <prefix> <out>

Paths come from `bin/paths.py`: `RUSTFLUTTER_FLUTTER`, `RUSTFLUTTER_APP` and
`RUSTFLUTTER_ENGINE`, defaulting to `~/flutter_sdk`, `~/gallery_upstream` and
`$FLUTTER/engine/src` on Linux, and to the Windows box's drive letters there.

Output is not formatted. Pipe it through `rustfmt --edition 2021` before use --
the backend spends its effort on being right about what to emit, and layout is
a solved problem it should not be re-solving.

## The two big classes are each one class in a directory

`KernelFrontend` was 14,272 lines and 594 members in one file; `RustBackend`
was 12,242. Each is now spread over the `part` files under
`lib/frontend_kernel/` and `lib/backend_rust/`, with `augment class` putting
them back together. Not one character of the moved code changed, and the
generated Rust is byte for byte what it was before the split.

It took three cuts. The first followed the section comments the files already
had (`// -- Expressions --`). That left two files over 5,000 lines, so the
second went inside them at member boundaries. That left one file that was a
*single method* -- `_expressionRaw`, 1,717 lines of `if (node is ..)` -- so the
third cut that method into ten runs, each answering for a family of node kinds
and returning null for the rest, chained by `??` in the order the one method
had. **38 part files, median 641 lines, none over 1,460**, and each carries a
line at the top saying what it holds.

The third cut is the only one that changed code rather than moving it, and it
is still checked the same way: extracting a method cannot change what is
emitted, so the generated Rust is byte for byte what it was before any of the
three cuts.

One thing the cuts turned up: `// -- Failure in the return value --` headed
2,841 lines of which only the first 295 were about failure -- the rest was the
whole class emitter, grown in under a heading that had stopped describing it.
Those are `emit_struct.dart`, `emit_impl.dart` and `emit_members.dart` now.

**They are not separable objects, and the split does not pretend they are.**
Measured 2026-09-09: the sections share 42 of `RustBackend`'s 69 fields and 35
of `KernelFrontend`'s 77, because the state those fields hold is passed
implicitly between sections rather than as arguments. Until that state is
explicit, a section cannot become a collaborator, and mixins cannot express
the split either -- the sections call each other in cycles. So the review that
asked for the god classes to be split first and the implicit state second had
the two the wrong way round; this is the half that could be done without
changing behaviour.

`augment` costs a flag (`bin/experiments.sh`), and it costs `dart format`:
dart_style cannot parse `augment class` and exits 65 on those eleven files, so
plain `dart format` reports "0 changed" and formats nothing. `bin/fmt.py` is
the way in -- it takes the keyword off a copy, runs the same formatter from
the same SDK, and puts it back. `bin/check.sh` and the pre-commit hook both go
through it.

## What a refusal leaves behind

The compiler refuses what it does not understand, which means a member's
lowering can stop anywhere. Whatever state it had set is then charged to the
*next* member unless something puts it back, and this has cost real rounds: a
refused constructor once left `_selfName` as `__new` and every later method in
that class read its fields off a name that does not exist there -- 97 `E0425`s
in `SemanticsFlags` alone.

The two halves answer it in opposite ways, and `bin/statecheck.py` holds each
to its own rule:

* The **front end**'s per-member `catch` restores nothing, so every scope it
  opens is a `try/finally` -- 24 of them, and the check fails on a 25th
  written without one.
* The **back end** rolls back centrally in `RustBackend._member`, which is
  cheaper than remembering a `finally` at each of the sixteen places that set
  something -- but it is a list, and a list goes stale. Its comment said
  "every scrap of state a member's emission sets" and named nine; thirteen
  more had been added elsewhere by 2026-09-09. The check now recomputes what
  belongs there and fails when the list is short.

Closing that gap changed nothing the gallery can see -- the generated Rust is
byte for byte what it was -- so it was a trap rather than a live bug. It is
still shut.

## Checks

    bin/check.sh

Formatting, the refusal-rollback check, `dart analyze` and `test/`. Neither existed before 2026-09-09: with no
`.dart_tool/package_config.json` in this directory the analyzer could not
resolve `package:kernel`, so it had never been run, and the compiler reached
51k lines with it off. The first run found 165 issues, among them a dropped
`isAsync` at a super call, four sets of receiver-changing member names that had
drifted apart, and 2500 lines of a front end that no longer compiles.

The tests are plain Dart programs, not `package:test`, for the pubspec reason
above. They cover what is pure and what everything else is decided by: the type
algebra in `coerce.dart`, the name mangling in `backend_rust.dart`, and the
member-name tables in `member_names.dart`.

## What it refuses to do

Every construct the front end does not understand raises `Unsupported` with the
source it choked on, and the driver reports it. It does not emit a plausible
guess. A compiler that silently emits something for input it did not understand
is worse than one that stops, because its output compiles.
