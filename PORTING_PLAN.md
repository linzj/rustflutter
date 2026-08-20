# 完全覆盖计划 —— 与上游逐类对齐

目标只有一个：`packages/flutter/lib/src` 的 widgets/rendering/painting/
gestures/services/animation/scheduler/foundation/material/cupertino 十层里，
**每个公共类**在 Rust 框架里有语义对齐的对应物，或有记录在案的处置。
尺子先行——完全覆盖定义为 **coverage 脚本报 0 个未记账缺失**：

```
python tools/coverage.py              # 全量五态报告
python tools/coverage.py painting/    # 单文件簇
python tools/coverage.py --missing-only
```

判定台账在 `coverage_ledger.json`，五态：

| 态 | 含义 |
| --- | --- |
| covered | 同名（snake_case 后）符号在 crate 里声明 |
| mapped | 台账记录的改名/合并/功能等价（如 `RenderAlign`≙`RenderPositionedBox`） |
| blocked-engine | 依赖引擎能力（external texture、platform view、restoration），挂账另立项 |
| out-of-scope | web-only / 无宿主平台 / debug-only，按宿主集合（Windows/Android/macOS）裁剪 |
| MISSING | 工作队列 |

私有类（`_` 前缀）与 `*_io.dart`/`*_web.dart` 变体不计。名字命中只是**入门**：
入库的验收是逐符号锚点比对（记入 PORTING_STATUS.md）+ 单测 + 双平台手验，
与既有轮次的纪律一致。

**基线（2026-08-17）：1,873 个公共类，covered 161 / mapped 7 /
blocked 11 / MISSING 1,694（90%）。**

**进度（2026-08-20）：1750 accounted / 138 MISSING（92.7%，总数因尺子修正 1873→1888）。painting/animation/foundation/services 四层已全覆盖。** 层别：painting
100%、animation 100%、foundation 100%、services 100%、gestures 98%、rendering
95%、material 74%、widgets 75%、cupertino 63%、scheduler 57%。

口径三则（2026-08-17 定）：

1. **范围**：framework 全层 + material + cupertino。
2. **对齐标准**：逐类**语义**对齐 + 台账。element 层保持单一 `ElementTree`
   形态，不重写为上游的类层级；每个上游 Element 子类的
   mount/update/rebuild 语义在 P4 逐项核对后入账。
3. **门控项**：依赖引擎的挂"阻塞-引擎"账（见台账与下表）；平台专属按宿主
   裁剪出范围。

## 门控项台账

| 上游面 | 缺什么 | 处置 |
| --- | --- | --- |
| force press | ABI 已有 `pressure: f64`，宿主恒填 1.0 | 框架侧可实现（测试桩喂压），宿主数据另立 |
| Texture / PlatformView / AndroidView | 渲染 ABI 无 external texture / platform view 通路 | blocked-engine |
| restoration 全家 | 引擎与框架两侧皆零 | blocked-engine |
| SystemMouseCursor 应答 | 通道已在（`services/system.rs`），宿主未实现 | 宿主侧另立 |
| web-only、iOS 专属实现 | 宿主集合 Windows/Android/macOS | out-of-scope |

## 横切地基 E（随波次前置，不单独立项）

- **E1** pipeline owner 等价物（PORTING_STATUS 点名多项目系于此）
- **E2** 布局中 build 通路（LayoutBuilder/SliverLayoutBuilder；做不成则
  正式分歧记录，现有替身 `RenderSizeReporter`）
- **E3** 语义 ABI 扩展（hidden 标记、custom actions；`RfSemanticsNode`）
- **E4** localizations + 资产：Localizations 机制、AssetBundle、FontLoader
  （现只有 `engine::register_font`）
- **E5** 图标进框架：Icon/IconData/IconTheme 现为零（图标=私有区码点+
  `Text::with_font_family`；gallery 靠自己的 catalog.rs 代码生成）——做进
  crate，gallery 改消费框架
- **E6** keep-alive（元素层保活 + AutomaticKeepAlive）
- **E7** 弹跳物理族（BouncingScrollPhysics/ClampedSimulation/
  ScrollSpringSimulation）
- **E8** foundation 响应式地基：Listenable/ChangeNotifier/ValueNotifier
  （现为零）、Key 家族补齐（现 `Key = Option<u64>`，无 ValueKey/ObjectKey/
  UniqueKey 语义区分）

## 波次（依赖序，每波独立可交付）

| 波 | 内容 | 量级 |
| --- | --- | --- |
| P0 | 尺子+台账+本文 | 已完成 |
| P1 | painting+foundation 地基：borders/BorderRadius/ShapeBorder 全家、shape_decoration、notched_shapes、fractional_offset、matrix_utils、clip、image provider 家族、colors、strut_style；E8；AssetBundle/FontLoader | ~160 类 |
| P2 | rendering 盒族：CustomPaint、Table、Flow、ListBody、RotatedBox、ConstraintsTransformBox、Custom\*ChildLayout、proxy_box 余下（ShaderMask/BackdropFilter/ClipRRect/Oval/PhysicalModel/Offstage/AbsorbPointer/MouseRegion 拆分/semantics 代理族/Leader-Follower）、AnimatedSize；layer.dart 按引擎留存层记 mapped | ~120 类 |
| P3 | sliver 全家：grid、fill 四式、fixed/varied extent、cross/main axis group、persistent header（pinned/floating/snap）、tree、shrink-wrapping viewport、cacheExtent、proxy_sliver | ~60 类 |
| P4 | element/widget 机制：逐 Element 子类语义核对入台账；InheritedModel/Notifier/Theme、LookupBoundary、LayoutBuilder(E2)、SlottedRenderObjectWidget、ParentDataWidget 全集；Async 三件、ValueListenableBuilder、TweenAnimationBuilder；actions/shortcuts 完整 intent 体系；focus 全量；Notification 扩展；PageStorage；Overlay 完整化；WidgetState | ~200 类 |
| P5 | 动画：先立 `Animation<T>` 对象图（现无——曲线只能进 `Controller`、无 listener/status 回调、无 Ticker/TickerProvider、无 fling）；再 CurvedAnimation、listener_helpers、TweenSequence/Interval、AnimatedBuilder、隐式动画全家、transitions 全家、Heroes；现有 Controller/implicit.rs 保留为门面记 mapped | ~35 类 |
| P6 | 滚动+手势：page_view、nested/single_child/list_wheel、reorderable、dismissible、draggable_scrollable_sheet、2D 滚动；scroll_controller 多位置、overscroll_indicator、ScrollBehavior 全量；multidrag、team、pointer_router、resampler、eager、tap_and_drag；force_press 按门控表 | ~90 类 |
| P7 | 文本：TextPainter 对象化（现为自由函数 `shape()`+双代缓存）、strut、按字重/变体注册字体、RenderEditable 对齐、selection.dart 17 类、selectable_region、context menus、magnifier、undo_history、widget_span（富文本嵌 widget） | ~80 类 |
| P8 | material 四波：M1 主题/基建 → M2 结构件（AppBar/Scaffold/导航件/GridView/RefreshIndicator） → M3 表单/复合（TextField/Dropdown/Chip/Date-Time pickers/Dialog 函数族/MenuAnchor/Stepper…） → M4 杂项终裁 | 386 类 |

**M1 的第一步已落地**（`color_scheme.rs` 全角色 + `colors.rs` 全色板 +
`widget_state.rs` 的 `WidgetStateProperty` 体系）。**M1 的下一步是
`ThemeData`，它要单独立项**：此侧现有的 `components::Theme` 是十四个字段的
简化版，components/controls/cupertino/pickers 与相册全在读它；上游 `ThemeData`
是上百个字段加四十来对 `*Theme`/`*ThemeData` 组件主题，且每个组件主题的回退
都穿过 `ThemeData`。做法是先立 `ThemeData`（持 `ColorScheme` + 组件主题表），
让 `Theme` 成为它的门面，再逐组件把回退接上去——半路停下会让两个主题类型并存，
比不做更糟。`ColorScheme.fromSeed` 需要 M3 色调调色板（HCT/CAM16），同样另立。
| P9 | cupertino 两波：C-M1 基础件 → C-M2 复合件 | 82 类 |
| P10 | 收尾：debug/inspector 类终裁、_window 家族按宿主、restoration 挂账核对、终验 0 MISSING | — |

## 工程接线（每个新模块固定三步，漏了测试不编）

1. `lib.rs` 加 `pub mod x;`（字母序）→ `pub use x::{...}` → 需要时进 prelude；
2. `src/flutter/rust/BUILD.gn` 里 `rustflutter_unittests` 的 `sources` 补一行；
3. 测试 inline 写文件尾 `#[cfg(test)] mod tests`，跑法
   `src/flutter/rust/run_rust_tests.py`。子目录模块照 `services/` 模式。

## 每波验收门

1. `tools/coverage.py`：该波文件 MISSING → 0；
2. 逐符号锚点比对记录进 PORTING_STATUS.md（锚点给上游符号名，不写行号）；
3. 单测对照上游 `packages/flutter/test/` 挑核心行为；`run_rust_tests.py` 全绿；
4. Windows + Android 双平台手验（相册 + 每波新增 demo 页承载新件）；
5. cargo fmt（pre-commit 已挂钩）。

## 量级与节奏

上游约 1,873 个公共类；现有 Rust 框架 ~47k 行覆盖不足一成。全量预计
Rust 150-220k 行，**以季度计**。按波交付，每波结束都是可用状态。节奏：
每轮会话吃一个波次的 1-2 个文件簇，从 P1 的 borders 簇开始。
