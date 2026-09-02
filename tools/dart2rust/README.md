# dart2rust

A Dart compiler with a Rust backend, in the shape `dart2wasm` has: a resolved
front end, a small IR, and a backend that emits the target language.

This is **not** the hand port that lives in `src/flutter/rust/rustflutter`. That
one re-expresses upstream in idiomatic Rust, choosing a different structure
where Rust wants one (`RenderProxyBox` is dissolved; `RenderState` lives on the
handle, not the object) and recording why at each divergence. This one
translates. The two answer different questions and their outputs do not merge --
see PORTING_STATUS.md for the argument.

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
    bin/dart2rust.dart    the driver

## Running

    dart run --packages="E:/source/flutter/.dart_tool/package_config.json" \
        tools/dart2rust/bin/dart2rust.dart <file.dart> <ClassName>

Output is not formatted. Pipe it through `rustfmt --edition 2021` before use --
the backend spends its effort on being right about what to emit, and layout is
a solved problem it should not be re-solving.

## What it refuses to do

Every construct the front end does not understand raises `Unsupported` with the
source it choked on, and the driver reports it. It does not emit a plausible
guess. A compiler that silently emits something for input it did not understand
is worse than one that stops, because its output compiles.
