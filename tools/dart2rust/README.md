# dart2rust

A Dart compiler with a Rust backend, in the shape `dart2wasm` has: a resolved
front end, a small IR, and a backend that emits the target language.

This is **not** the hand port that lives in `src/flutter/rust/rustflutter`. That
one re-expresses upstream in idiomatic Rust, choosing a different structure
where Rust wants one (`RenderProxyBox` is dissolved; `RenderState` lives on the
handle, not the object) and recording why at each divergence. This one
translates. The two answer different questions and their outputs do not merge --
see PORTING_STATUS.md for the argument.

## Why the front end is `package:analyzer` and not `package:kernel`

`dart2wasm` consumes Kernel (`.dill`), which is the right input: it is resolved,
desugared, and constant-evaluated. It is also **not obtainable here**. Checked,
in this order:

| where | result |
|---|---|
| Flutter's `.dart_tool/package_config.json` | 250 packages, no `kernel` |
| pub.dev | the published `kernel` is an abandoned pre-null-safety squat |
| `bin/cache/dart-sdk/` | binary SDK, ships no `pkg/` sources |
| `engine/src/third_party/` | partially synced; no `dart` |
| `github.com/dart-lang/sdk`, `dart.googlesource.com` | unreachable from this machine |

So the front end is `package:analyzer` (10.1.0), which **is** available and which
has the precedent: DDC was analyzer-based for years before it moved to Kernel.

A probe over `material/ink_well.dart` confirmed it supplies the three things a
front end has to supply, before any of this was written:

- **full resolution** -- 1273 identifiers resolved, 0 diagnostics
- **constant evaluation** -- `_activationDuration` came back as
  `Duration(inMicroseconds: 100000)`, evaluated down to the primitive. This is
  the fact-class the hand port kept getting wrong by eye (50ms read as 200ms).
- **resolved `super` targets** -- needed to flatten the class hierarchy

What analyzer does not do is desugar. Mixins are not applied, `async` is not
lowered, implicit coercions are not made explicit. Those become this compiler's
job rather than the front end's. That is a real cost, and it is also why the IR
in `lib/ir.dart` is front-end agnostic: if `pkg/kernel` ever becomes reachable,
the front end is the only part that has to be rewritten.

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
