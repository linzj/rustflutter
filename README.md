# rustflutter

用 Rust 重写 Flutter 框架层，保留 Flutter engine 的排版、渲染、合成与线程模型。

- **保留**：Impeller（GPU 渲染）、display_list（绘制录制）、flow（Layer 树与合成）、
  txt + skparagraph（文字整形排版）、fml（线程模型 / TaskRunner / MessageLoop）、
  各平台嵌入层（Android / iOS / macOS / Linux / Windows / embedder C API）。
- **删除**：Dart VM、DartIsolate、tonic、`dart:ui` 的 Dart 侧、整个 `packages/flutter`。
- **重写**：框架层（foundation / gestures / scheduler / animation / painting /
  rendering / widgets / semantics / services）改用 Rust。

上游来源：`K:\flutter`（flutter/flutter monorepo，commit `cf97bfbcb9f`）。

## 当前状态

**M0 + M1 + M2 完成**：整个 shell 在无 Dart 的情况下构建通过，
应用跑在引擎自己的 Shell 上——真正的线程模型、vsync 驱动的帧调度、
`Rasterizer` 流水线。`create` / `run` 项目流程可用。

```
gn gen  →  1001 targets from 272 files
ninja   →  exit 0
rustflutter_unittests   →  7 passed
rust_ffi_unittests      →  5 passed
帧率                    →  175 帧 / 2.917 秒 = 60.0 fps（帧间隔 16,666 µs）
```

![Hello World](docs/hello_world_shell.png)

## 快速开始

```sh
cd src

# 一次性：生成构建文件
vpython3 flutter/tools/gn --unoptimized --no-rbe
ninja -C out/host_debug_unopt

# 创建并运行一个应用
./out/host_debug_unopt/rustflutter create my_app --title "My App"
vpython3 flutter/tools/gn --unoptimized --no-rbe   # 让新 target 进入构建图
./out/host_debug_unopt/rustflutter run my_app
```

`run` 会用 ninja 构建并打开窗口，帧由 vsync 驱动。
加 `-- --png out.png` 走无头单帧渲染（CI 用，不起 shell）。
删除应用用 `rustflutter remove my_app`——直接删目录会在 `projects/BUILD.gn` 里
留下悬空标签，之后每次 `gn gen` 都会失败。

需要 PATH 上有 `rustc`（本机验证于 1.93.0，edition 2024）。

## 应用长什么样

```rust
use rustflutter::prelude::*;

struct AppRoot;

impl Application for AppRoot {
    fn build(&mut self, _context: &BuildContext) -> BoxedWidget {
        Box::new(Center::new(
            Container::new()
                .with_color(Color::rgb(0x1B, 0x2A, 0x3A))
                .with_corner_radius(16.0)
                .with_padding(EdgeInsets::symmetric(48.0, 36.0))
                .with_child(
                    Text::new("Hello, World!")
                        .with_size(52.0)
                        .with_weight(700)
                        .with_color(Color::WHITE),
                ),
        ))
    }
}

fn main() {
    register_application(|| Box::new(AppRoot));
    run(&RunOptions::default()).unwrap();
}
```

布局遵循与 Flutter `RenderBox` 相同的协议：**约束下行、尺寸上行、父级定位子级**。
`Text` 的整形排版走引擎自己的 `txt` / skparagraph，绘制进真实的 `DisplayList`。

每一帧的路径是引擎原本的那条：

```
VsyncWaiter → Animator → Engine → RuntimeController
    → Application::build → 布局 → 绘制 → LayerTree
    → Pipeline → Rasterizer → Surface → 屏幕
```

帧是按需的，不是自由运行的：没有请求时引擎在最后一帧之后进入空闲，
所以一个静态界面留在屏幕上不花任何代价。动画通过
`FrameScheduler::request_frame` 继续（等价于 dart:ui 的
`PlatformDispatcher.scheduleFrame`）。

## 关键设计前提

整个方案成立的依据是一个实测结论：**引擎的渲染栈已经与 Dart 完全解耦**。
`flow`、`display_list`、`txt` 对 Dart 的引用数为 0，`impeller` 仅一个单测引用。
Dart 与引擎之间的全部契约收敛在两处：

| 方向 | 位置 | 规模 |
|---|---|---|
| Dart → C++ | `lib/ui/dart_ui.cc` 绑定表 | 231 个绑定 |
| C++ → Dart | `lib/ui/window/platform_configuration.h` 的 `DartPersistentValue` | 20 个回调 |

而真正的交接点只有一行（上游 `runtime/runtime_delegate.h`，已随 `runtime/` 删除）：

```cpp
virtual void Render(int64_t view_id,
                    std::unique_ptr<flutter::LayerTree> layer_tree,
                    float device_pixel_ratio) = 0;
```

框架层唯一的产出就是一棵 `LayerTree`。**Rust 只要能构造 `LayerTree`，
下游 rasterizer → display_list → Impeller 一行都不用改**——这正是本仓库现在在做的事。

## 目录结构

```
src/
├── .gn, BUILD.gn, build/, build_overrides/   GN 构建系统（+ Rust 工具链）
└── flutter/
    ├── impeller/          GPU 渲染          ── 原样保留
    ├── display_list/      绘制录制/回放      ── 原样保留
    ├── flow/              Layer 树与合成     ── 原样保留
    ├── txt/               文字排版           ── 原样保留
    ├── fml/               线程模型           ── 原样保留（去 Dart Timeline）
    ├── shell/             壳层与平台嵌入      ── 已去 Dart 化
    ├── runtime/           RuntimeController   ── 重建：驱动 Rust 而非 isolate
    ├── lib/ui/            引擎对象包装层      ── :ui_types 可用，其余待去 Dart 化（73 文件）
    └── rust/                                 ← Rust 侧
        ├── ffi/           C ABI + C++ 实现（引擎边界）
        ├── host/          窗口 + 线程模型 + Shell 启动
        ├── rustflutter/   框架 crate（engine 绑定 + app + widgets）
        ├── cli/           `rustflutter` 命令行工具
        ├── examples/      示例应用
        └── projects/      `rustflutter create` 生成的应用
```

逐目录分级、待改文件清单、以及各里程碑的完整改动记录，
见 **[PORTING_STATUS.md](PORTING_STATUS.md)**。
