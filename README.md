# rustflutter

**English** · [简体中文](README.zh-CN.md)

The Flutter framework rewritten in Rust, on top of the unmodified Flutter
engine — its text layout, rendering, compositing and threading model kept as-is.

- **Kept.** Impeller (GPU rendering), display_list (draw recording), flow (layer
  tree and compositing), txt + skparagraph (text shaping and layout), fml
  (threading, task runners, message loops), shell (Engine, Animator, Rasterizer,
  Pipeline).
- **Removed.** The Dart VM, DartIsolate, tonic, the Dart half of `dart:ui`, and
  all of `packages/flutter`.
- **Rewritten in Rust.** The framework layer: gestures, animation, painting,
  rendering, widgets, and a component library.

Forked from [flutter/flutter](https://github.com/flutter/flutter) at commit
`cf97bfbcb9f`.

> This project is not affiliated with, endorsed by, or sponsored by Google or
> the Flutter team. "Flutter" is a trademark of Google LLC and appears here to
> describe what this software is built from.

## Status

**M0–M7 complete, Impeller included, Flutter Gallery ported.** The whole shell
builds with no Dart anywhere. Applications run on the engine's own `Shell`, with
its real threading model, vsync-driven frame scheduling and `Rasterizer`
pipeline, drawing through **Impeller on the GPU** (GLES via ANGLE). The
framework has the full RenderBox protocol, an element tree with state, hit
testing and gestures, animation and a navigation stack, and a component library.

```
gn gen                        →  1011 targets from 276 files
ninja                         →  exit 0, no warnings
rustflutter_unittests         →  224 passed
flutter_gallery_unittests     →   21 passed
rust_ffi_unittests            →   15 passed
frame time (optimized build)  →  16.6–16.8 ms/frame (59.5–60.3 fps)
of which actual work          →  0.5 ms on the UI thread + 0.9 ms rasterising
```

![Components](docs/showcase_impeller.png)

*A real frame, read back from the GPU framebuffer — rendered by Impeller
through ANGLE.*

<p align="center">
  <img src="docs/gallery/home.png" width="24%">
  <img src="docs/gallery/study_rally.png" width="24%">
  <img src="docs/gallery/demo_cards.png" width="24%">
  <img src="docs/gallery/settings.png" width="24%">
</p>

*Flutter Gallery: home, Rally, components, settings. 26 screens, all laid out
by Rust.*

## Getting the dependencies

The engine's C++ and the whole Rust framework are committed here and arrive with
the clone. `third_party` and the build toolchains are not — `DEPS` names them
and `gclient sync` fetches them. You need `depot_tools` on PATH:

```sh
cp tools/gclient.template .gclient
gclient sync                  # about 13 GB
python tools/check_deps.py    # confirm what landed matches DEPS
```

That covers clang, gn, ninja, skia, icu, angle and the rest. Two things it does
not cover:

- **`rustc`, which you install yourself.** It resolves off PATH (see
  `src/build/toolchain/rust.gni`) and needs edition 2024; verified on 1.93.0.
- **A local Visual Studio on Windows**, with `DEPOT_TOOLS_WIN_TOOLCHAIN=0` set.
  Without it the `win_toolchain` hook goes looking for Google's internal
  toolchain package.

## Quick start

```sh
cd src

# once: generate the build files
vpython3 flutter/tools/gn --unoptimized --no-rbe
ninja -C out/host_debug_unopt

# the archive an application links against, and an application
vpython3 flutter/tools/gn --runtime-mode=release --no-rbe
ninja -C out/host_release flutter/rust:rustflutter_engine
./out/host_debug_unopt/rustflutter create my_app --title "My App" --path ~/code
```

## Applications

**An application is an ordinary Cargo project and lives wherever you want it.**
It is not a GN target and not part of this repository:

```sh
cd ~/code/my_app
cargo run                    # opens a window, frames driven by vsync
cargo run -- --png out.png   # one frame, headless, no shell
cargo test
```

That works because the engine build produces
`flutter/rust:rustflutter_engine` — one archive holding the whole C++ side,
which the generated `build.rs` links against. Cargo compiles the framework
crate and the application together on top of it, so nothing about an
application has to know that GN exists.

It has to be a **release** engine build: everything links the static CRT
(`/MT`), which is what rustc uses, and a debug engine build uses `/MTd`.

Applications used to be GN targets under `flutter/rust/projects`, because the
engine is built with GN and an application has to link it. That made every
application engine code, which it is not — it put unrelated projects in the
engine's build graph and its git history, and made every engine upgrade carry
them.

## What an application looks like

```rust
use rustflutter::prelude::*;

#[derive(Default)]
struct State {
    count: i32,
    pressed: Option<u64>,
}

struct Counter;

impl StatefulComponent for Counter {
    type State = State;

    fn build(
        &self,
        state: &State,
        handle: StateHandle<State>,
        _cx: &mut rustflutter::framework::BuildContext,
    ) -> AnyWidget {
        let count = state.count;
        stack_column(
            vec![
                component(Label::title(format!("{count}"))),
                component(
                    Button::new(1, "Increment")
                        .wired(handle, |s| &mut s.pressed, |s| s.count += 1),
                ),
            ],
            12.0,
        )
    }
}

struct App;

impl WidgetApplication for App {
    fn build(&mut self, _cx: &BuildContext) -> AnyWidget {
        provide(Theme::dark(), stateful(Counter))
    }
}

fn main() {
    register_application(|| Box::new(WidgetHost::new(App)));
    run(&RunOptions::default()).unwrap();
}
```

## Three layers, the same as upstream

```
Widget        cheap, immutable, thrown away and rebuilt every frame
Element       persistent: holds state, decides what to reuse
RenderObject  does layout, painting and hit testing
```

Layout follows the same protocol as Flutter's `RenderBox`: **constraints go
down, sizes come up, the parent positions its children.** `Text` shapes and
lays out through the engine's own `txt` / skparagraph, and paints into a real
`DisplayList`.

Every frame takes the engine's original path:

```
VsyncWaiter → Animator → Engine → RuntimeController
    → Application::build → layout → paint → LayerTree
    → Pipeline → Rasterizer → Surface → screen
```

A tap takes its mirror image:

```
Win32 → PlatformView → Engine → RuntimeController
    → hit test (against the previous frame's render tree) → gesture recognition
    → set_state → mark dirty → request a frame
```

A key takes the same road by a different door. It is a *platform message* on
`flutter/keydata` — which is what every Flutter embedder does, so no method on
`PlatformView`, `Shell` or `Engine` is key-shaped:

```
Win32 → KeyDataPacket → PlatformView::DispatchPlatformMessage → Engine
    → RuntimeController → Application::on_key
```

There is no focus tree yet, so a key has nobody to be addressed to. What exists
is the layer upstream runs *before* the focus walk — `FocusManager`'s early key
handlers — which is application-wide shortcuts and honestly only that.

Frames are on demand rather than free-running: with nothing requested the engine
goes idle after the last one, so a static screen costs nothing to keep up.

## The premise this rests on

The whole approach depends on one measured fact: **the engine's rendering stack
is already fully decoupled from Dart.** `flow`, `display_list` and `txt`
reference Dart zero times; `impeller` does so in one unit test. Every contract
between Dart and the engine collapses into two places:

| Direction | Where | Size |
|---|---|---|
| Dart → C++ | the binding table in `lib/ui/dart_ui.cc` | 231 bindings |
| C++ → Dart | `DartPersistentValue` in `lib/ui/window/platform_configuration.h` | 20 callbacks |

And the actual handover is a single line (`runtime/runtime_delegate.h`, kept
verbatim after this repository rebuilt it):

```cpp
virtual void Render(int64_t view_id,
                    std::unique_ptr<flutter::LayerTree> layer_tree,
                    float device_pixel_ratio) = 0;
```

The only thing the framework layer produces is a `LayerTree`. **If Rust can
build one, nothing downstream — rasterizer, display_list, Impeller — has to
change by a line.**

## Examples

| Example | What it demonstrates |
|---|---|
| `hello_world` | The pipeline end to end: Rust → DisplayList → LayerTree → raster |
| `gallery` | The render layer: flex, stack, viewport scrolling, gradients, clips |
| `counter` | Element tree, partial rebuild, tap handling |
| `showcase` | Components and theming: one toggle recolours the application |
| `flutter_gallery` | The upstream Gallery ported: 26 screens, navigation stack, slide transitions |
| `platform_channels` | Every channel the engine defines, both ways: clipboard, a `TextField` typed into, the mouse cursor, the reader's settings, and a close the application refuses. Against a real shell; checks itself and exits non-zero on a wrong answer |

<p align="center">
  <img src="docs/gallery_top.png" width="30%">
  <img src="docs/counter_clicked.png" width="30%">
  <img src="docs/showcase_light.png" width="30%">
</p>

## Release builds

```
vpython3 flutter/tools/gn --runtime-mode=release --no-rbe
ninja -C out/host_release flutter/rust/examples/flutter_gallery
python3 tools/package_gallery.py --zip
```

Produces `dist/rustflutter-gallery/`: a 20 MB executable plus `icudtl.dat`, and
nothing else. The fonts, icons, study artwork and 38 product photographs are all
`include_bytes!`-ed into the binary — no asset bundle, no `flutter_assets`
directory, and no CRT runtime dependency (the import table is system DLLs only;
ANGLE is linked statically).

## Layout

```
src/
├── .gn, BUILD.gn, build/, build_overrides/   GN build system (+ Rust toolchain)
└── flutter/
    ├── impeller/          GPU rendering        ── untouched
    ├── display_list/      draw record/replay   ── untouched
    ├── flow/              layer tree           ── untouched
    ├── txt/               text layout          ── untouched
    ├── fml/               threading            ── untouched but for Dart Timeline
    ├── shell/             shell and embedding  ── de-Dart-ified
    ├── runtime/           RuntimeController    ── rebuilt: drives Rust, not an isolate
    ├── lib/ui/            engine object wrappers ── :ui_types usable, see M4 for the rest
    └── rust/                                   ← the Rust side
        ├── ffi/           C ABI + C++ implementation (78 functions)
        ├── host/          window, threading, shell startup
        ├── rustflutter/   the framework crate
        │   ├── engine.rs      engine bindings
        │   ├── painting.rs    paths, gradients, images, canvas state
        │   ├── render.rs      the RenderBox protocol and render objects
        │   ├── widgets.rs     named facades
        │   ├── framework.rs   widget / element / state / provider
        │   ├── gestures.rs    pointer events and gesture recognition
        │   ├── keyboard/      key events, key tables, the pressed set
        │   ├── components.rs  component library and theming
        │   └── app.rs         the contract with the shell
        ├── cli/           the `rustflutter` command line tool
        └── examples/      example applications
```

Of the 4,559 engine files here, 64 are modified and the rest are byte-for-byte
upstream. A per-directory breakdown, the full record of what each milestone
changed, and what is worth doing next are in
**[PORTING_STATUS.md](PORTING_STATUS.md)** (Chinese).

## Known limitations

- **The render tree is rebuilt whole every frame.** Element reuse preserves
  state and skips `build`, but layout and paint still run. This is the largest
  outstanding performance debt.
- **The host is Windows-only.** Everything above `rf_host_run` — Shell,
  ThreadHost, Animator, Rasterizer, the software surface — is portable; each
  platform is missing only a window and a message loop.
- **The layer tree is flat.** The framework emits one layer and one DisplayList
  per frame: clips, opacity and transforms are recorded into the display list
  rather than becoming `ClipRectLayer`, `OpacityLayer` and `TransformLayer` the
  way upstream does. So there are no repaint boundaries, the raster cache has
  nothing to hit, and damage is the whole screen. Not a bottleneck yet
  (rasterising takes 1 ms) but it will be as scenes get heavier. The layer FFI
  is already in place; the framework just does not use it.
- **The keyboard reports but cannot consume.** Keys reach the framework, and
  `Application::on_key` returns whether it used one, but nothing acts on that
  answer: the platform sees every key too. Suppressing an unhandled key means
  re-posting it to the message queue after the framework has replied, which is
  the bulk of upstream's `KeyboardManager`. There is also no focus tree, so key
  handling is application-wide rather than per-widget.
- **No accessibility.** The channel is here; there is no semantics tree to
  describe over it.
- **Text input is a single-line field.** `TextField` works, IME included, but
  there is no selection painting, no scrolling past the edge of the field and
  no focus traversal -- a field is focused by being tapped. The IME's
  composition path follows the Windows embedder message for message and is not
  covered by an automated check: driving one needs an input context, which the
  machine this was built on does not have.
- **The host serves the engine's own channels, not a plugin ecosystem.**
  Platform messages work in both directions, with codecs, replies and
  buffering. The Windows host serves every channel the engine itself defines --
  `flutter/lifecycle`, `flutter/platform` (clipboard, sound, the cancelable
  exit handshake), `flutter/textinput`, `flutter/mousecursor`,
  `flutter/settings` and `flutter/localization`. What is missing is a plugin
  system: there is no registrar for a third party to add a channel to, so an
  application's own channel needs its native half written into the host. A
  channel nobody serves comes back empty, which the framework reads as "nobody
  implements this" -- the same answer a Flutter app gets for a plugin it did
  not install.
- **There will be no hot reload.** That is a Dart VM capability with no Rust
  equivalent.

## Diagnostics

| Environment variable | Effect |
|---|---|
| `RUSTFLUTTER_SOFTWARE=1` | Force the Skia software surface, bypassing Impeller |
| `RUSTFLUTTER_CAPTURE_FRAME=<path>` | Read the GPU framebuffer back to a PNG before the swap, overwritten each frame. `PrintWindow` cannot capture a GPU-composited window, so this is how to see what Impeller actually drew |
| `RUSTFLUTTER_FRAME_STATS=1` | Every 60 frames, report medians for the UI thread (build / layout / record) and the raster side (raster / swap / frame interval). This is what found the double wait |
| `RUSTFLUTTER_OUT=<dir>` | Point the CLI at a specific build output directory |

## License

BSD-3-Clause, the same terms as Flutter, whose engine makes up most of this
repository — see [LICENSE](LICENSE).

Some Gallery artwork is Apache 2.0 rather than BSD, and it is compiled into the
binaries, so a distributed executable carries it too. [NOTICE](NOTICE) says what
came from where.
