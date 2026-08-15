# 搬迁状态与待办清单

上游：`K:\flutter\engine\src`（commit `cf97bfbcb9f`）
搬迁时间：2026-08-14 ｜ 已拷入 2,967 个 C/C++ 文件 / 约 569,000 行 / 49 MB

---

## 一、已拷入的内容

### Tier A — 原样保留，零改动

对 Dart 的引用数为 0（`assets`、`common` 各有 1 个头文件例外，见 Tier B）。

| 目录 | 文件 | 行数 | 作用 |
|---|---:|---:|---|
| `impeller/` | 925 | 169,832 | GPU 渲染后端 |
| `display_list/` | 171 | 44,842 | 绘制指令录制与回放 |
| `flow/` | 123 | 22,373 | Layer 树、合成、raster cache |
| `txt/` | 51 | 3,633 | 文字排版（依赖 skparagraph） |
| `vulkan/` | 34 | 3,450 | Vulkan 封装 |
| `flutter_vma/` | 4 | 398 | Vulkan 内存分配器 |
| `shell/gpu/`, `shell/version/` | — | — | GPU surface 与版本信息 |
| `build/`, `testing/` | 114 | 9,680 | GN 辅助与测试框架 |
| `src/build/`, `src/build_overrides/`, `src/.gn`, `src/BUILD.gn` | — | — | GN 构建系统 |

### Tier B — 已拷入，少量文件待改

`shell/` 共 1,150 文件 / 264,992 行，`fml/` 198 文件 / 18,883 行。
**其中仅 10 个非测试文件引用 Dart：**

| 文件 | 需要做什么 |
|---|---|
| `shell/common/engine.cc` / `engine.h` | **核心改造点。** `Engine` 实现 `RuntimeDelegate`，持有 `RuntimeController`。需抽出接口，让 Rust 运行时替代 Dart 运行时 |
| `shell/common/shell.cc` | 去掉 `DartVM` 启动与生命周期管理 |
| `shell/common/animator.cc` | vsync → `BeginFrame`/`DrawFrame` 的时序，需改为驱动 Rust 侧 |
| `shell/platform/embedder/embedder.cc` | embedder C API 中的 Dart entrypoint 参数需替换 |
| `fml/trace_event.cc` / `.h` | 仅用 Dart Timeline API 记录 trace，换成独立实现即可 |
| `common/settings.h` | 配置结构体中的 Dart 相关字段 |
| `assets/native_assets.h` | native assets 清单，与 Dart FFI 绑定相关 |
| `shell/profiling/sampling_profiler.cc` | 采样 profiler 的 Dart 集成 |
| `BUILD.gn`（根） | 移除 Dart 相关 target |

`shell/platform/` 的各平台嵌入层（Android / darwin / linux / windows / common）
共 768 个文件，**仅 macOS 的 2 个测试辅助文件引用 Dart，96% 以上无需改动**。

`tools/` 保留了上游的构建工具（`engine_tool`、`clang_tidy`、`githooks` 等，共 168 个
`.dart` 文件）。这些是**构建期主机工具，不进产物**，短期可继续用 Dart 跑，无需优先替换。

### Tier C — `lib/ui/`：已拷入 C++，待去 Dart 化

149 个文件 / 23,061 行，**其中 73 个文件需要改造**。20 个 `.dart` 文件已丢弃。

> ⚠️ **修正一处此前的判断。** 我先前说这一层"只需把外层的 tonic 绑定换成 `extern "C"`"，
> 这低估了工作量。实际耦合是结构性的：88 处继承 `RefCountedDartWrappable<T>` /
> 使用 `DEFINE_WRAPPERTYPEINFO()` / 在构造函数签名里接收 `Dart_Handle`。例如
> `lib/ui/painting/path.h`：
>
> ```cpp
> class CanvasPath : public RefCountedDartWrappable<CanvasPath> {
>   DEFINE_WRAPPERTYPEINFO();
>   static fml::RefPtr<CanvasPath> Create(Dart_Handle wrapper) {
>     auto res = fml::MakeRefCounted<CanvasPath>();
>     res->AssociateWithDartWrapper(wrapper);   // ← Dart GC 挂钩
>     ...
> ```
>
> 好消息是**载荷逻辑是干净的**——`CanvasPath` 实际只持有一个 `SkPath sk_path_`，
> 方法体全是普通 C++。所以这是一次跨 73 个文件的**机械式去包装**
> （剥离基类、`Dart_Handle` 构造参数换成普通工厂、引用计数改由 Rust 侧 `Arc` 持有），
> 而不是重写。工作量可预估，但不是"改个绑定层"那么轻。

按子目录：

| 子目录 | 行数 | 内容 |
|---|---:|---|
| `lib/ui/painting/` | 10,650 | Canvas / Path / Paint / Gradient / ImageFilter / Picture / Codec / Vertices |
| `lib/ui/window/` | 3,297 | `PlatformConfiguration`（20 个入向回调所在地） |
| `lib/ui/text/` | 1,410 | ParagraphBuilder / Paragraph / FontCollection |
| `lib/ui/semantics/` | 941 | SemanticsUpdateBuilder |
| `lib/ui/compositing/` | 657 | **SceneBuilder / Scene —— 产出 `LayerTree` 的地方，M1 的目标** |

保留 `lib/ui/dart_ui.cc` 未删除：它是那 231 个绑定的完整清单，
是 Rust FFI 层要实现什么的**权威规格书**，改造完成后再删。

---

## 二、未拷入的内容

| 内容 | 规模 | 原因 |
|---|---:|---|
| `third_party/` 全部 | 13 GB | 上游未修改，应由 DEPS/gclient 拉取。含 skia 1.2G、icu 1.2G、harfbuzz 223M 等 |
| └ `third_party/dart` | 4.0 GB | **Dart SDK，本项目彻底不需要** |
| └ `third_party/tonic` | — | Dart↔C++ 胶水，随 Dart 一起消失 |
| `runtime/` | 9,248 行 | DartVM / DartIsolate / DartSnapshot / service isolate，整个目录删除 |
| `lib/ui/*.dart` | 27,437 行 | `dart:ui` 的 Dart 侧，由 Rust 重写 |
| `lib/web_ui/` | 186,995 行 | Web 引擎（Dart 实现），本阶段不涉及 |
| `lib/io/`, `lib/snapshot/`, `lib/gpu/` | — | dart:io 钩子 / Dart 快照 / dart:gpu |
| `shell/platform/fuchsia/` | — | 本质是一个 Dart runner 宿主 |
| `testing/dart/` | — | `dart:ui` 的测试套件 |
| `sky/`, `web_sdk/`, `skwasm/`, `wasm/` | — | Dart 包定义与 Web 产物 |
| `flutter_frontend_server/` | — | Dart kernel 编译器前端 |
| `out/` | 9.3 GB | 构建产物 |
| `packages/`（框架 63 万行） | — | 由 Rust 重写，不作为参考拷入 |

已拷入后又移除的纯 Dart 运行时插件：`lib/ui/isolate_name_server/`、`lib/ui/plugins/`、
`window/platform_isolate.*`、`window/platform_message_response_dart*.`、
`shell/platform/common/isolate_scope.*`、`shell/common/dart_native_benchmarks.cc`。

---

## 三、M0 —— 已完成 ✅

目标是让这棵树能编译，并接入 Rust 工具链。四项全部达成，Windows x64 host 构建实测通过。

### 1. 依赖裁剪

`DEPS` 由上游过滤生成（生成脚本见提交说明），保留条目的 revision 与上游一致，便于后续 roll：

| | 上游 | rustflutter |
|---|---:|---:|
| vars | 105 | 48 |
| deps | 118 | 67 |
| hooks | 14 | 6 |

丢弃的 51 个 deps：Dart SDK 30、Dart pub 包 10、web 工具链 5、Dart SDK 的 C 依赖 3
（cpu_features / re2 / sqlite）、Fuchsia 3。保留 boringssl——它不是 Dart 依赖，
`common/graphics/persistent_cache` 用它算 shader 缓存键。

**本地不执行 `gclient sync`**：13 GB 依赖用目录 junction 指向已有的
`K:\flutter\engine\src`，零下载、零额外磁盘。`DEPS` 描述的是 CI 应拉取的依赖集。

### 2. GN 去 Dart 化

实测共 **62 条依赖边 / 19 个 BUILD.gn** 指向已删除的 Dart 目标，已全部清除。
另有若干处需要真实改造而非删除：

| 位置 | 处理 |
|---|---|
| `fml/trace_event.{h,cc}` | **只用了 `Dart_Timeline_Event_Type` 这个枚举，一个 Dart 函数都没调**。就地声明等价枚举（成员与顺序照搬，保证下游 Chrome-trace phase 字符不变），`display_list` 和 `flow` 对 `libdart_jit` 的链接边随之消失 |
| `testing/testing.gni` | 摘掉 3 个 Dart 快照模板与 `test_fixtures` 的 `dart_main` 分支；fixtures 定位 / 拷贝 / `enable_unittests` 原样保留 |
| `testing/BUILD.gn` | 删除 `source_set("dart")`、vmservice 快照、`fixture_test`（及对应 10 个源文件） |
| `runtime:test_font` | **抢救**——字体数据本身零 Dart 耦合，移到 `//flutter/test_font`，txt 的测试依赖它 |
| `shell/version` | 删除 `GetDartVersion()` 与 `DART_VERSION` define |
| `testing/testing.cc` | 删除 `GetDefaultKernelFilePath()`（返回 Dart kernel_blob.bin 路径） |
| `tools/gn` | 补 `strip_dart_args()` 过滤所有 `dart_*` GN 参数；`content_hash` 在缺少 monorepo 脚本时回落到 engine revision |
| ANGLE 的 `gclient_args.gni` | ANGLE 硬编码 import 了 Dart SDK 里的这个生成文件。在 GN **secondary source 树**里放了 shim，从而既不需要 `third_party/dart` 目录存在，也不必给 ANGLE 打补丁 |
| `lib/ui/BUILD.gn` | 桩化为空 group（原文件存为 `BUILD.gn.upstream`），使 `//flutter/lib/ui` 标签可解析而不把不可编译的源码拖进图 |

### 3. Tier A 构建通过

```
gn gen  →  985 targets from 264 files
ninja   →  2581/2581,  exit 0
```

产出 2,874 个编译单元，全树无任何 Dart 产物。

### 4. Rust 工具链已接入

- `src/build/toolchain/rust.gni`：`rustc_path` / `rust_edition` / `extra_rustflags` 三个 build arg
- `src/build/toolchain/win/BUILD.gn`：新增 `rust_rlib` / `rust_staticlib` / `rust_bin` 三个 GN tool
  （注意本仓库 pin 的 GN 是 2285，不支持 rust tool 上的 `depsformat` 与 `*_output_extension`，
  输出扩展名直接写死在 `outputs` 里）
- `flutter/rust/ffi`：Rust staticlib，导出 `extern "C"` 符号
- `flutter/rust:rust_ffi_unittests`：C++ 侧调用 Rust 并断言返回值

```
[1/1] RUST(STATICLIB) obj/flutter/rust/ffi.lib
[  PASSED  ] 2 tests.
```

**Rust ↔ C++ 边界已实测打通。**

> 当前 `rustc_path` 默认走 PATH（本机 rustc 1.93.0，edition 2024）。进 CI 前应像 clang 一样
> 用 CIPD 固定到 `//flutter/buildtools/$host_os-$host_cpu/rust`，保证构建可复现。
> 目前只写了 Windows toolchain 的 rust tool，mac/linux/android 需照做。

---

## 四、M1 —— 已完成 ✅

目标是 Dart-free 地出一帧。实际做到的比原计划多：不只出了帧，还有可用的 widget 层、
项目脚手架和窗口显示。

### 1. 引擎边界（`flutter/rust/ffi/`）

`rustflutter_ffi.h` / `.cc` 是一层 `extern "C"` ABI，覆盖 Paint / Canvas
（`DisplayListBuilder`）/ Paragraph（`txt` + skparagraph）/ LayerTree / 光栅化。
它只是包装引擎已有的 C++ 对象，`display_list`、`flow`、`txt` 一行未改。

出帧路径：

```
Rust widget 树
  → DisplayListBuilder            (display_list, 原样)
  → DisplayListLayer → LayerTree  (flow, 原样)
  → LayerTree::Flatten()          (flow, 原样)
  → SkSurface raster              (skia, 原样)
  → PNG / BGRA 缓冲
```

`LayerTree::Flatten()` 不需要 GPU 上下文，所以无头渲染开箱可用。
Impeller 是生产路径，但它需要 Shell，而 Shell 还卡在 Engine/RuntimeController 改造上。

### 2. 框架 crate（`flutter/rust/rustflutter/`）

- `engine.rs` —— FFI 的安全封装。每个句柄由 Rust 值持有并 `Drop`，
  取代上游 `RefCountedDartWrappable` + Dart GC 的不确定回收。
- `widgets.rs` —— widget 层：`Constraints` / `Size` / `Offset` / `EdgeInsets`，
  以及 `Text` / `Container` / `Center` / `Column`。布局协议与上游 `RenderBox` 一致：
  **约束下行、尺寸上行、父级定位子级**。
- `lib.rs` —— `App`，跑一帧：layout → paint → `LayerTree` → 交给引擎。
  对应上游 `RenderView` + `SchedulerBinding.drawFrame` 到 `PlatformDispatcher.render()`。

### 3. 项目脚手架（`flutter/rust/cli/`）

```sh
rustflutter create <name> [--title <text>]   # 生成项目
rustflutter list                             # 列出项目
rustflutter build <name>                     # ninja 构建
rustflutter run <name> [-- <app args>]       # 构建并运行
```

`create` 写出 `BUILD.gn` / `main.cc` / `src/main.rs` / `README.md`，
并重写 `projects/BUILD.gn` 把新项目接进构建图。生成后需要跑一次 `gn gen`
让 GN 发现新目录。

### 4. 窗口显示（`flutter/rust/ffi/rustflutter_window_win.cc`）

Win32 窗口 + `StretchDIBits` 直接呈现 BGRA 缓冲。**这是权宜方案**，
不是生产路径——生产路径是 `shell/platform/windows` 驱动 Impeller，
仍然依赖 Engine/RuntimeController 改造。非 Windows 平台目前返回 `-100`。

### 5. 验证

| | 结果 |
|---|---|
| `gn gen` | 998 targets / 270 files |
| `ninja flutter:tier_a` | 2581/2581, exit 0 |
| `rustflutter_unittests`（Rust） | 7 passed |
| `rust_ffi_unittests`（C++ 集成） | 5 passed |
| `rustflutter create` → `run` | 生成项目构建通过并出帧 |
| 窗口显示 | 800x600 客户区截图与预期一致 |

FFI 集成测试不是烟雾测试：它构造 LayerTree、经引擎光栅化、再读回像素断言颜色，
并断言文字确实在表面上留下了墨迹。

### M1 期间修正的三个判断

1. **`SkPngEncoder::Encode` 的签名**与记忆中不同——这个 Skia 版本里
   `SkWStream*` 重载只接受 `SkPixmap`，用的是 `Encode(GrDirectContext*, SkImage*, Options)`
   （CPU 图像传 nullptr 上下文）。
2. **`txt::FontWeight` 是直接的 CSS 数值**（400 = normal），不是 0 基索引。
   我最初按索引映射，把 700 变成了 6，渲染出极细体。已改为透传。
3. **`Paragraph` 需要两趟排版**。第一趟在给定的 `max_width` 里量出墨迹宽度，
   第二趟按该宽度收紧段落盒。否则居中/右对齐的段落是相对 `max_width` 定位字形的，
   与调用方量到的盒子对不上——上游 `RenderParagraph` 在松约束下也是这么做的。

### 已知限制

- **GN 的 `rust_bin` 工具没有使用。** 让 rustc 主导最终链接，在 Windows 上要重新推导
  MSVC 链接器路径和系统库列表（而且 rustc 会给 GN 已带 `.lib` 后缀的库名再加一次
  `.lib`）。应用和 CLI 一律用 Rust staticlib + 三行 C++ shim 的模式，
  既绕开这些问题，也是应用本来就需要的（它们要链接 C++ 引擎）。
  Rust 单测则通过 `run_rust_tests.py` 显式指定 `lld-link` 来构建运行。
- Rust 工具链目前只在 Windows toolchain 里声明；mac/linux/android 需照做。
- 窗口层只有 Windows 实现，且是静态一帧，没有事件循环与重绘驱动。（M2 已解决）

---

## 五、M2 —— 已完成 ✅

接管 `shell/common` 的 Engine / RuntimeController。这一步的价值不在代码量，
而在它**一次解锁四样东西**：真正的 vsync 帧调度、引擎自己的线程模型、
`Rasterizer` 流水线、以及平台嵌入层的接入点。

### 1. `lib/ui` 拆成两个目标（M2.1）

`lib/ui:ui_types` —— shell 需要的那部分，全部是普通 C++：
viewport metrics、pointer/key packet、platform message、semantics node、
font collection、image generator registry。实测 shell 依赖的 10 个头文件里
9 个对 Dart 的引用数为 0，只有 5 个文件需要动刀：

| 文件 | 改动 |
|---|---|
| `semantics/string_attribute.{h,cc}` | 删掉 `NativeStringAttribute`（纯 Dart peer 类），改为普通构造函数；.cc 删除 |
| `semantics/semantics_flags.{h,cc}` | 删掉 `NativeSemanticsFlags`；.cc 删除 |
| `semantics/custom_accessibility_action.h` | 去掉 3 个 tonic include |
| `text/font_collection.{h,cc}` | `LoadFontFromList(Dart_Handle, Dart_Handle, ...)` → `LoadFontFromBuffer(const uint8_t*, size_t, ...)` |

`lib/ui:ui` 仍是空 group——73 个 `RefCountedDartWrappable` 包装类是 M4 的活。

### 2. `//flutter/runtime` 重建（M2.2）

上游这个目录是 Dart VM 嵌入层（DartVM / DartIsolate / DartSnapshot /
IsolateConfiguration / service isolate / tonic），导入时整个删掉了。
现在回来的只有 shell 真正需要的部分：

- `runtime_delegate.h` —— 上游的契约减去四个 Dart VM 专有成员
  （`OnRootIsolateCreated`、`UpdateIsolateDescription`、`RequestDartDeferredLibrary`，
  以及随之而去的 `dart_api.h`）。**最要紧的那一个成员原样保留**：
  `Render(int64_t view_id, std::unique_ptr<LayerTree>, float dpr)`。
- `runtime_controller.{h,cc}` —— 持有一个 `RfApp`（静态链接的 Rust 框架实例）
  而不是 root isolate，通过 `rf_app_*` C ABI 而不是 20 个
  `tonic::DartPersistentValue` 回调与框架通信。**调用方向和时序完全不变**，
  这正是 Engine / Animator / Rasterizer / 各嵌入层能原样不动的原因。
- `platform_data.{h,cc}` —— 从上游原样拷回。

Rust 侧 `rustflutter/src/app.rs` 实现 `rf_app_*`：视图管理、begin/draw frame、
每个视图一次 `render` 回调交出 layer tree。应用实现 `Application` trait，
用 `register_application` 在**运行期**注册。

> 这里改过一次设计。最初让应用通过 `app!` 宏导出一个 `#[no_mangle]` 符号，
> 由链接期解析——结果是每个链接了框架却没有应用的二进制（单测、CLI 工具）
> 都链不上。改成运行期注册后，框架回到"一个普通库"的位置。

### 3. `shell/common` 去 Dart 化（M2.3）

| 文件 | 行数 | 主要改动 |
|---|---|---|
| `engine.{h,cc}` | 692 → 499 | 构造函数不再穿 DartVM / isolate snapshot / SkiaUnrefQueue / SnapshotDelegate / `UIDartState::Context`；`Run()` 调 `LaunchApplication` 而非 `LaunchRootIsolate`；`Restart()` 用自己的 `PlatformData` 重建控制器而不是 clone isolate |
| `shell.{h,cc}` | 2527 → 1881 | 去掉 `DartVMRef`，去掉整个 VM service protocol（Shell 曾是 `ServiceProtocol::Handler`，11 个端点全部注册在 VM 上）；`GetConcurrentWorkerTaskRunner` 自己持有线程池；`Shell::Spawn` 明确失败——spawn 共享的是 isolate group，这里没有这种东西 |
| `animator.cc` | — | `Dart_TimelineGetMicros()` → `fml::TimePoint::Now()`（同一个时钟） |
| `run_configuration.{h,cc}` | — | 去掉 `IsolateConfiguration`：没有快照要加载，所以配置恒为 valid |
| `platform_view.{h,cc}` | — | 删掉 deferred-library 钩子 |
| `switches.cc` | — | `--dart-flags` 接受并忽略，不再对着 allow-list 检查 |
| `shell/profiling/sampling_profiler.cc` | — | 不再给一个不存在的 service 命名线程 |

### 4. 真正的 Shell 跑起来（M2.4）

新增 `//flutter/rust/host` —— 上游由 `shell/platform/<os>` 承担的角色，
但小得多：没有 platform channel、没有 plugin registrar、没有 embedder C API，
只有一个窗口、线程模型和 shell。

```
主线程     Win32 窗口 + 消息循环（窗口消息只投递给创建它的线程）
platform   Shell 生命周期、PlatformView
ui         Animator → Engine → RuntimeController → Rust
raster     Rasterizer → GPUSurfaceSoftware → 帧缓冲
io         ShellIOManager
```

窗口线程刻意**不是** platform 线程：让它兼任意味着要把 `fml::MessageLoop`
和 `GetMessage` 交错起来，那是 Windows 嵌入层花掉真实复杂度的地方。
窗口学到的（尺寸、关闭）投递给 platform task runner；
raster 线程产出的帧用 `PostMessage` 投回窗口线程，两边不直接碰对方的状态。

**验证**：

```
帧率       175 帧 / 2.917 秒 = 60.0 fps，帧间隔精确 16,666 µs
像素       #0E1626（背景）/ #1B2A3A（卡片），与 headless PNG 一致
重排       系统把窗口缩到 644×461 后自动重新布局
           → WM_SIZE → SetViewportMetrics → ScheduleFrame → build → render 全通
构建       ninja 全绿，7 个 Rust 单测 + 5 个 FFI 单测通过
脚手架     rustflutter create → gn → run 全流程可用，生成的应用同样跑在 Shell 上
```

![Hello World on the Shell](docs/hello_world_shell.png)

### M2 未做的：Impeller

呈现走的是 `GPUSurfaceSoftware`（Skia CPU 光栅）。Impeller 需要窗口上有
GL 或 Vulkan 上下文——在 Windows 上就是 ANGLE，也就是上游
`AngleSurfaceManager` 那一块。这是独立的一步，不影响 M2 已经解锁的其余部分：
帧从 `Animator` 出发，经 `Pipeline` 到 `Rasterizer`，换 `Surface` 实现即可。

### M2 已知限制

- 呈现是软件光栅，见上。
- 指针事件已接到 `rf_app_dispatch_pointer_packet`，但没有可命中测试的渲染树
  可路由（M5），当前收下即丢。
- 平台通道、语义、deferred loading 在 `RuntimeController` 里都是接受即返回。
- host 只有 Windows 实现；`rf_host_run` 之上的一切（Shell、ThreadHost、
  Animator、Rasterizer、软件 surface）都是可移植的，每个平台缺的只是
  一个窗口和一个消息循环。

---

## 六、M4 —— 已完成 ✅

补齐 dart:ui 等价层。引擎 C ABI 从 29 个函数长到 78 个：

| 类别 | 内容 |
|---|---|
| paint | 不透明度、混合模式、线帽/线接、遮罩模糊，线性/径向/扫描渐变 |
| path | 命令式构建器（move/line/quadratic/cubic/close）+ 矩形/椭圆/圆/圆角矩形 |
| canvas | line、oval、path、arc、image、image_rect；save/saveLayer/restore + save count；translate/scale/rotate/skew/2D 仿射；裁剪矩形、圆角矩形、路径 |
| layers | transform、offset、clip rect/rrect/path、opacity、背景模糊、子树模糊，以 push/pop 栈的形式覆盖 flow 的各 Layer 类型 |
| images | 解码 PNG/JPEG/WebP/GIF/BMP 并绘制 |

**这里做了一个明确的取舍**：没有去 Dart 化 `lib/ui/painting` 里那 73 个
`RefCountedDartWrappable` 类。它们存在的意义就是被 Dart 调用，每个的实际载荷
不过是 display_list 之上的几行。直接从 C ABI 触达 display_list 得到同样的能力，
而不必背上一个没有调用者的绑定层。

`RfLayerTree` 从单一 root 改为持有一个打开中的容器层栈，push/pop 因此与
`SceneBuilder.push*` 语义一致。

Skia 编解码器注册移进了 `rf_image_decode`：本构建定义了
`SK_DISABLE_LEGACY_INIT_DECODERS`，而解码必须在没有 shell 的情况下也能工作。

---

## 七、M5 —— 已完成 ✅

渲染层。`render.rs` 完整实现 RenderBox 协议——**约束下行、尺寸上行、父级定位子级**
——外加 intrinsics、基线和命中测试：

```
RenderDecoratedBox     纯色/渐变填充、圆角、描边
RenderParagraph        文本，含真实基线与固有宽度
RenderImage            Contain / Cover / Fill / None
RenderConstrainedBox   SizedBox 与 Container 的 width/height
RenderPadding
RenderAlign            Center 与 Align，支持收缩因子
RenderFlex             Row / Column：flex 因子、紧/松适配、
                       6 种主轴对齐、5 种交叉轴对齐（含 Baseline）
RenderStack            边缘锚定与双边拉伸
RenderTransform        绕支点的 2D 仿射，不影响布局
RenderOpacity          经 save layer，避免子树内部互相透视
RenderClipRect/Path
RenderViewport         滚动：子节点无界布局，按偏移显示并裁剪
RenderPointerRegion    命中测试身份
```

`widgets.rs` 成为它们的具名门面：`Center` 是 `RenderAlign`，`Row` 和 `Column`
都是 `RenderFlex`，`ListView` 是 `RenderFlex` 之上的 `RenderViewport`。
`Container` 采用组合而非自实现：按需叠加 margin / 尺寸 / 装饰 / padding / 对齐。

### gallery 示例抓出的两个真实缺陷

1. **`RenderFlex` 把自己的交叉轴最小约束传给了子节点**，于是 64px 行里的
   44px 头像被拉成 64px——一个椭圆。只有 `Stretch` 该强制交叉轴最小值。
2. **`HitTestResult` 给每个包含指针的盒子都记了一条**，包括匿名的，
   把真正的最内层目标埋在了后面。身份为 0 的条目现在在唯一入口处被丢弃。

---

## 八、M6 + M3 —— 已完成 ✅

### widgets 层（M6）

`framework.rs` 补上了 widget 与 render object 之间缺失的一层：

```
Widget        廉价、不可变，每帧丢弃重建
Element       持久：持有状态，决定复用什么
RenderObject  做布局、绘制与命中测试
```

**为什么用 arena**：上游 `Element` 持有可变的父指针、子列表和 render object 引用，
全是循环的。这个形状过不了 Rust 的借用检查——满地 `Rc<RefCell<Element>>`
能编译，然后在第一次构建触及自己祖先时 panic。所以元素住在一个 slab 里，
以 `ElementId` 为键，每条链接都是索引：循环不可表达，过期句柄是一次返回
`None` 的查找而不是悬垂指针。

**复用规则只有一条**：新 widget 与已有 element 的具体类型相同且 `Key` 相同，
就原地更新（状态保留），否则卸载重建（状态丢弃）。有 key 的子节点按 key 匹配
（无论移动到哪），其余按位置匹配——这正是重排后的列表还能保住状态的原因。

`set_state` 在状态空闲时立即应用、在构建期间被借出时排队，所以处理器能读回
刚写的值，而构建期内的 `set_state` 也不会造成重入借用。两种情况都会标脏并请求一帧。

三个组合子 `leaf` / `single` / `many` 覆盖了 render widget，不必把整个组件目录
重写成 widget 类型。

### 输入（M3）

`gestures.rs` 识别点击与拖拽。**处理器挂在 render object 上而不是 widget 上**
——指针到达时，声明它的那个 widget 早已不存在；而且是 `Rc<dyn Fn>` 而非 `FnMut`，
因为命中测试是在共享引用下遍历树的。处理器改动的是构建时捕获的 `StateHandle`。
这就是整个闭环。

真正起作用的仲裁只有一个距离判断：按压移动超过 `kTouchSlop` 就不再是点击候选，
转为拖拽。只有两个识别器时，完整的 gesture arena 是比决策本身更多的机械。

shell 在跨 ABI 前把 `flutter::PointerData` 收窄到 15 个字段，
让结构体布局只活在一种语言里。Windows host 处理
`WM_LBUTTONDOWN/UP/MOUSEMOVE` 并做鼠标捕获，`WM_CAPTURECHANGED` 时取消按压，
按钮因此不会卡在按下态。

### 测试抓出的缺陷

`ElementTree::rebuild` 在末尾清空脏集合，把它刚刚那次重建过程中产生的
`set_state` 一并丢掉了。改为在开始时清空——全量重建涵盖此前所有待处理项，
但重建期间提出的请求是给下一帧的。

---

## 九、M7 —— 已完成 ✅（按重新设计而非移植）

上游这一层是 material（176,201 行）+ cupertino（48,253 行），
合计占框架总量 36%。两者都没有移植，原因不是工作量：它们是两种具体设计语言的
实现，直译成 Rust 会得到一个既不是 Flutter 的 Material、也不是 Rust API 的东西。
`components.rs` 提供的是应用真正会用到的那一组，为这个框架设计：

```
Theme（dark/light）  AppBar   Scaffold   Card     ListTile   Divider
Button（4 种样式）    Switch   Slider     ProgressBar
Label（3 级）         Badge    Gap        IdSource
```

主题通过 `Provider` 传递——`framework.rs` 新增的 InheritedWidget 等价物。
`provide(Theme::light(), child)` 向子树发布一个值，
`context.inherited::<T>()` 沿元素的父链向上查找最近的一个。
与上游的差别在于值变化时：上游记录依赖并只重建读取它的 widget，
这里 Provider 重建则其子树跟着重建——正确，但比上游做得多。追踪读者是下一步。

**刻意缺席的是文本输入。** 一个可用的输入框需要平台输入法——组合区间、候选窗、
系统能定位的光标——那是平台通道的活，不是 widget 的活。一个只处理 ASCII 按键的
`TextField` 会看起来完成而实际上对世界上一半的语言不可用，所以没有。

`Slider` 上还修了一个真实缺陷：被拉伸的父级会让命中区域比轨道宽，
而取值来自 `local_position.dx / width`，于是滑块还没到头值就到 100% 了。
用 `Align` 松开约束把区域钉回轨道宽度。

---

## 十、示例

| 示例 | 证明了什么 |
|---|---|
| `hello_world` | 流水线端到端：Rust → DisplayList → LayerTree → 光栅化 |
| `gallery` | 渲染层：flex、stack、viewport 滚动、渐变、裁剪、命中测试 |
| `counter` | element 树 + 局部重建 + 点击。header 显示自己被构建的次数；点按钮后它仍是 1 |
| `showcase` | 组件库与主题：一次点击开关，整个应用换配色 |

每个都有 `--png` 无头路径供 CI 使用。

---

## 十一、下一步

按价值排序：

1. **持久化 render object。** 元素复用现在保住了状态、跳过了 `build`，
   但 render tree 每帧仍整棵重建，布局和绘制照跑不误。这是最大的一笔性能欠账。
2. **Impeller。** 呈现走的是 `GPUSurfaceSoftware`。Impeller 需要窗口上的
   GL 或 Vulkan 上下文——Windows 上就是 ANGLE，即上游 `AngleSurfaceManager`
   那一块。换的是 `Surface` 实现，其余不动。
3. **InheritedWidget 的依赖追踪。** 让 `Provider` 只重建真正读了它的 widget。
4. **多平台。** Rust toolchain 与 host 只有 Windows；`rf_host_run` 之上的一切
   都是可移植的，每个平台缺的只是一个窗口和一个消息循环。
5. **平台通道。** services 3 万行。好消息：`PlatformMessage` 是语言无关的
   二进制通道，现存插件的 Android/iOS 原生侧全部可复用。
6. **文本编辑与输入法**、**无障碍**、**rustc 用 CIPD 固定**。

**不会有的东西：hot reload。** 它是 Dart VM 的能力，Rust 没有对等物。
桌面上或许能做 dylib 热加载，但状态保持做不到，iOS 上禁止动态代码。
这是这条路线的固定成本，不是待办事项。
