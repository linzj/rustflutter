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

`tools/dart2rust/runtime/` **does not exist yet**, and `embedder_api.py` prints
`0 implemented` rather than skipping the line, because the distance is the point
of the ruler. Its first contents are already written: the 1248 lines of
`lib/prelude.dart`, a hand-written subset of `dart:core` and `dart:typed_data`
that is currently emitted as a string alongside the generated code. Round 44
measured why it is hand-written rather than translated -- feeding the dill's own
`dart:core` through the translator took the error count from 6608 to 16955,
because its members are `external` and come out as empty traits.

## The front end: analyzer today, Kernel next

`dart2wasm` consumes Kernel (`.dill`), which is the right input: resolved,
desugared, constant-evaluated, and -- the part that matters for shipping -- a
whole *program* rather than a pile of files. An app.dill is what the toolchain
actually builds and what a release would be translated from.

**An earlier version of this file said Kernel was unobtainable here. That was
wrong.** `package:kernel` is in the engine checkout:

    E:/source/flutter/engine/src/flutter/third_party/dart/pkg/kernel/

The earlier search looked under `engine/src/third_party/` and used a depth limit
one level short of `engine/src/flutter/third_party/dart/pkg/kernel`, and the
conclusion was written down as if it were a fact about the machine. It is
recorded here rather than quietly deleted, because a wrong reason left in place
is how a project keeps making the same choice.

Verified working, in this order:

1. `pkg/kernel` reading the SDK cache's `dart2js_platform.dill` fails with
   `Unexpected Kernel Format Version 140 (expected 139)` -- it read the file and
   parsed the header; the checkout is one revision behind the Flutter SDK.
2. The same checkout has a revision-matched dill and toolchain:
   `engine/src/out/host_release/flutter_patched_sdk/platform_strong.dill` reads
   cleanly -- 20 libraries, 1374 classes, with `isAbstract` and resolved
   superclasses -- and `engine/src/out/host_release/gen/frontend_server_aot.dart.snapshot`
   can produce matching dills for an app.

So the front end is `package:analyzer` **for now**, and the IR was written
front-end agnostic from the first commit precisely so this swap costs only the
front end. Everything in `lib/backend_rust.dart`, the census, and the tests
carries over unchanged.

What analyzer gives that Kernel does not: source-shaped output, which is easier
to read and to check against upstream by eye. What Kernel gives that analyzer
does not: the whole linked program, mixins applied, `async` lowered, implicit
coercions explicit, and reachability -- so a release translates what the app
uses instead of every class in the framework.

## Layout

    lib/ir.dart           the IR. Knows nothing about analyzer or about Rust.
    lib/frontend.dart     analyzer's resolved AST -> IR
    lib/backend_rust.dart IR -> Rust source
    lib/prelude.dart      the hand-written `dart:core` subset the output needs
    bin/dart2rust.dart    the driver
    bin/census.dart       the ruler for the translation half: refusals, queued
    bin/embedder_api.py   the ruler for the runtime half: what the engine asks
    runtime/              the Rust VM. Not written yet; see STATUS.md.

## Running

    dart run --packages="$RUSTFLUTTER_FLUTTER/.dart_tool/package_config.json" \
        tools/dart2rust/bin/dart2rust.dart <file.dart> <ClassName>

Paths come from `bin/paths.py`: `RUSTFLUTTER_FLUTTER`, `RUSTFLUTTER_APP` and
`RUSTFLUTTER_ENGINE`, defaulting to `~/flutter_sdk`, `~/gallery_upstream` and
`$FLUTTER/engine/src` on Linux, and to the Windows box's drive letters there.

Output is not formatted. Pipe it through `rustfmt --edition 2021` before use --
the backend spends its effort on being right about what to emit, and layout is
a solved problem it should not be re-solving.

## What it refuses to do

Every construct the front end does not understand raises `Unsupported` with the
source it choked on, and the driver reports it. It does not emit a plausible
guess. A compiler that silently emits something for input it did not understand
is worse than one that stops, because its output compiles.
