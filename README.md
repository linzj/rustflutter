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

本目录目前**只完成了源码搬迁**，尚不可构建。已拷入 2,967 个 C/C++ 文件、约 56.9 万行，
占用 49 MB。third_party（13 GB，其中 Dart SDK 独占 4 GB）不拷贝，后续由 DEPS 拉取。

逐目录的分级、待改文件清单、以及下一步任务，见 **[PORTING_STATUS.md](PORTING_STATUS.md)**。

## 关键设计前提

整个方案成立的依据是一个实测结论：**引擎的渲染栈已经与 Dart 完全解耦**。
`flow`、`display_list`、`txt` 对 Dart 的引用数为 0，`impeller` 仅一个单测引用。
Dart 与引擎之间的全部契约收敛在两处：

| 方向 | 位置 | 规模 |
|---|---|---|
| Dart → C++ | `lib/ui/dart_ui.cc` 绑定表 | 231 个绑定 |
| C++ → Dart | `lib/ui/window/platform_configuration.h` 的 `DartPersistentValue` | 20 个回调 |

而真正的交接点只有一行（`runtime/runtime_delegate.h`，已随 `runtime/` 一并删除）：

```cpp
virtual void Render(int64_t view_id,
                    std::unique_ptr<flutter::LayerTree> layer_tree,
                    float device_pixel_ratio) = 0;
```

框架层唯一的产出就是一棵 `LayerTree`。**Rust 只要能构造 `LayerTree`，
下游 rasterizer → display_list → Impeller 一行都不用改。**

## 目录结构

```
src/
├── .gn, BUILD.gn, build/, build_overrides/   GN 构建系统（原样自上游）
└── flutter/
    ├── impeller/          GPU 渲染          ── 原样保留
    ├── display_list/      绘制录制/回放      ── 原样保留
    ├── flow/              Layer 树与合成     ── 原样保留
    ├── txt/               文字排版           ── 原样保留
    ├── fml/               线程模型           ── 原样保留（2 文件待改）
    ├── shell/             壳层与平台嵌入      ── 保留（4 文件待改）
    ├── lib/ui/            引擎对象包装层      ── 待去 Dart 化（87 文件）
    ├── testing/, tools/   测试与构建工具
    └── rust/              ← Rust 侧新代码
        ├── ffi/           extern "C" ABI 绑定层
        └── framework/     Rust 框架层
```
