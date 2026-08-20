# 弹层线完整 port —— PORTING_PLAN.md P4「Overlay 完整化」的展开

本文只做一件事：把上游 `widgets/overlay.dart` 那条线**接进活的元素树**，
让弹层（对话框、菜单、tooltip、snackbar、拖拽反馈、选择手柄、放大镜）从
"调用方自己 `Stack`"回到"框架托管"。

它是 `PORTING_PLAN.md` 波次表 P4 里那个单条目的施工图，口径、工程接线、
验收门全部沿用母文件，不另立规矩。

---

## 一、现状与代价

### 1.1 现状是一个写在文档里的显式决定，不是疏忽

同一句声明出现在四处，措辞一致：

| 位置 | 原话 |
| --- | --- |
| `controls.rs` 模块头 | Overlays are the app's, not the framework's … **there is no overlay manager here — only the things that go in one** |
| `pickers.rs` 模块头 | 同上 |
| `menu.rs` 模块头 | This framework has no `Overlay` and no route for transient surfaces |
| `cupertino.rs` 模块头 | 同 `controls`：a dialog or context menu is a surface to put in a `Stack` over a scrim |

体现在 API 上：`show_date_picker` / `show_time_picker` / `show_date_range_picker`
不弹任何东西，**return 一个 `AnyWidget`**，由调用方叠在页面上、在
`on_confirm`/`on_cancel` 触发时自己从树里摘掉；抽屉的开合是
`Scaffold::with_drawer_open`，状态归 app。

而 `overlay.rs` —— `OverlayEntry` / `OverlayState` / `OverlayPortal` /
`OverlayPortalController` 的完整逻辑 port，连 opaque 截断构建、`maintain_state`、
mounted 通知、z 序时钟都建模了 —— **全 crate 零引用**。`autocomplete.rs` 模块头
自己记下了这一点："`crate::overlay` 有 entry 列表和 portal 的 z 序，**但还没有
任何东西托管这些 widget**"。

### 1.2 已经付出的可验证代价

| 症状 | 证据 |
| --- | --- |
| **锚定数学是死的** | `menu::popup_menu_offset(overlay: Size, anchor: Rect, …)` 要求锚点与 overlay 同坐标系，但 render 层没有 `local_to_global` / paint transform 查询。全 crate 只有它自己的单测调用过它 |
| **进出场动画被直接砍掉** | `drawer.rs` 删掉 246ms 滑入，理由原文是"没有 route 那样的所有者就没有 controller"；`menu.rs` 删掉 300ms 开合与逐项 `Interval` 淡入，同因 |
| **一整层交互能力悬空** | `modal_barrier.rs`、`raw_tooltip.rs`、`snack_bar.rs`、`scaffold_messenger.rs`、`navigator.rs` 均为纯逻辑 port（`use crate::framework` 计数 0）。活的组件库里**没有 Tooltip、没有 SnackBar**，只有 `component_themes.rs` 里对应的 `TooltipThemeData` / `SnackBarThemeData` 孤零零放着 |
| **逃不出祖先的裁剪与变换** | 弹层只能浮在调用方那个 `Stack` 之上。`RenderClipRect` / `RenderClipRRect` / `RenderClipPath` / `RenderClipOval` / `RenderTransform` 都在，按钮只要在其中之一里面，菜单就被切掉 |
| **封装破坏** | "开没开"这个 bool 必须住在页面根上，于是做不出"自带菜单的按钮"，每个使用方重抄一遍管线 |

---

## 二、判断：不用重写任何一层，缺三个接缝

活的框架层（`framework.rs` 元素树 arena + `GlobalKey` 注册表 + `Provider`/
`inherited` + `StateHandle`；`render.rs` 的 `RenderRef` / `RenderState` /
`flush_layout` / `flush_paint` 流水线）**已经足够表达上游的 Overlay 语义**。
缺的是 L0/L1/L2 三层接缝，L3/L4 是接缝就位后的纯增量。

| 层 | 内容 | 量级 | 风险 |
| --- | --- | --- | --- |
| L0 | 坐标：`apply_paint_transform` + `local_to_global` | 面广机械，48 处 | 低（有免费 oracle） |
| L1 | 宿主：`Overlay` + `RenderTheatre` + `Overlay::of` | 小 | 低（判定逻辑已 port） |
| L2 | `OverlayPortal`：就地 build、异地 render | 小而关键，~百行 | **中，是全案的赌点** |
| L3 | 模态语义：barrier / focus / dismiss | 中 | 低 |
| L4 | 消费者回填 | 按件计，逐个独立可交付 | 低 |

---

## 三、L0 · 坐标

**其它两层都依赖它，必须先做。**

### 做什么

1. `RenderBox` trait 上加：

   ```rust
   /// 上游 `RenderObject.applyPaintTransform`。默认恒等。
   fn apply_paint_transform(&self, child: &RenderRef, transform: &mut [f32; 6]) {}
   ```

2. `RenderRef` 上加 `transform_to(ancestor: Option<&RenderRef>) -> [f32; 6]`：
   顺 `RenderState::parent`（`render.rs:1612`，layout 时认领的 `Weak`）向上走，
   逐级 `apply_paint_transform` 并用已有的 `compose_affine`（`render.rs:714`）
   叠乘。上游锚点 `RenderObject.getTransformTo`。

3. 在其上包 `local_to_global(point) -> Offset` 与 `global_rect() -> Rect`。
   上游锚点 `RenderBox.localToGlobal` / `MatrixUtils.transformPoint`。

### 为什么工作量比看上去小

需要实现的正是那 **48 个 override 了 `hit_test_children` 的 render object**
（全 crate 共 63 个 `pub struct Render*`）。它们每一个都已经在算
`position - offset` 了 —— `apply_paint_transform` 要的就是同一个数的正向。
`RenderStack` 甚至已经把 `offsets: Vec<Offset>` 存成字段（`render.rs:5732`），
`RenderFlex`、viewport 族同理。仿射约定 `[a,b,c,d,e,f]`（`x' = a*x + c*y + e`）
全仓已统一，`RenderRotatedBox::paint_transform` 与 `RenderTransform` 都用它。

### 怎么验（这层的 oracle 是免费的）

正反两个方向必须互逆：取一点 p，`transform_to` 正向映射到全局，再用
`hit_test` 从全局反查该对象，得到的局部坐标必须等于 p。对 48 个容器逐个跑
这条差分测试，能把实现错误全兜住。**不写这条差分测试就不算做完 L0。**

### 解锁什么

`popup_menu_offset` 终于有真实调用者；tooltip 的 target 矩形、magnifier 的
聚焦点、text selection handles、`drag_target` 的 feedback 起始位置、
`interactive_viewer` 的命中变换，全部依赖这一层。

---

## 四、L1 · 宿主：Overlay + Theatre

### 已经写完的部分

`overlay.rs` 的判定逻辑不需要动：`OverlayState::onstage()`（`overlay.rs:388`）
就是 opaque 截断 + `maintain_state` 的完整决策，`insert` / `insert_all` /
`rearrange` / `remove` / `flush_build` / `debug_is_visible` 齐全，
`OverlayEntry` 的 mounted 通知语义也在。

### 要接的线

1. **句柄与查找**

   ```rust
   #[derive(Clone, PartialEq)]
   pub struct OverlayHandle { handle: StateHandle<OverlayState> }
   ```

   用 `provide` 发布；`Overlay::of(ctx)` 即 `ctx.inherited::<OverlayHandle>()`
   （`framework.rs:1413`）。上游锚点 `Overlay.of` / `Overlay.maybeOf`。

   > ⚠️ **`OverlayHandle` 的 `PartialEq` 必须跨帧相等**（只比 `ElementId`）。
   > `inherited()` 会注册依赖，句柄若每帧不等，`ElementTree::publish` 会把
   > 全树依赖者标脏 —— 这是本层唯一的性能陷阱。

2. **`RenderTheatre`** = `RenderStack` + `onstage()` 过滤 + `can_size_overlay`
   规则（约束无界时，由最上面自愿的非 positioned entry 定尺寸，其余强制对齐）。
   上游锚点 `_RenderTheatre`。

3. **安装点**：`app.rs:552` 那行
   `MediaQuery::new(data, self.app.build(context))` 外面再包一层 Overlay。
   与上游 `WidgetsApp` 装 `Navigator` + `Overlay` 的位置一一对应。

### 怎么验

- `overlay.rs` 现有单测全部继续通过（它们测的是纯逻辑，不该被接线改动）；
- 新增树测：`insert` 之后下一帧的渲染树多一个 child；`remove` 之后少一个；
- opaque entry 之下的 entry **不被 build**（不是 build 了盖住），用元素树的
  `last_rebuilt` 断言。

---

## 五、L2 · OverlayPortal：就地 build、异地 render

**这是全案最关键、也最不显然的一步。先做这一层的垂直切片，再动 L0 的 48 处。**

### 这套架构能表达它，靠两个已有事实

1. `many(children, assemble)` 的 assemble 是 `Fn(Vec<BoxedRender>) -> R`
   （`framework.rs:653`），**能捕获一个 `Rc` 侧信道**。
2. `build_render` 是**自底向上**的 —— `build_render_tree`
   （`framework.rs:2574`）的注释自己写明了："a render object is built from
   its children's and they are not built until now"。

### 做法

portal 写成 `many(vec![in_place_child, overlay_child], …)`：assemble 拿到两个
`BoxedRender`，**自己只返回 `rendered[0]`，把 `rendered[1]` 通过 `OverlayHandle`
交给 theatre**。

Overlay 是祖先，它的 assemble 在后代之后跑 —— 所以**同一帧内** theatre 就能
收到 portal 交上来的 `RenderRef`，不差一帧、不需要二次布局。

- `RenderRef` 是 `Rc<RefCell<Box<dyn RenderBox>>>`，被两处持有天然可行。
- 关键约束：**只能有一个父亲给它 layout**。portal 自己不放它，theatre 就在
  layout 时认领 `RenderState::parent`，`mark_needs_layout` 也自然顺着 theatre
  往上走 —— 这正是上游要的语义。
- `OverlayPortalController` 连 `z_order_index` 的时钟排序都已 port 好
  （`overlay.rs`），直接消费。

### 为什么必须这样，而不是把 widget 塞进 overlay 的 children

child 在 **portal 的位置** build，才继承按钮处的 `Theme` / `Directionality` /
`MediaQuery`。这就是 `OverlayPortal` 存在的全部理由，也是 `overlay.rs` 模块头
自己写下的那句话：*"Building it in the overlay instead would give a tooltip the
overlay's inherited context rather than the button's."*

### 怎么验（这一个测试就是整层的正确性判据）

在 portal 上方、Overlay 下方 `provide` 一个值，断言 portal 的 overlay child
读到的是 **portal 处的值**而不是根的值。附加断言：该 child 画在 theatre 给的
位置而不是 portal 处；portal 自身的渲染树里不含它。

### 退路

若 `many` 侧信道方案在 layout/paint 的父子归属上站不住，退到 **theatre 持有
builder 闭包 + 显式传递继承环境快照**。代价是丢掉真正的 in-place build（继承
上下文要手工搬运，`Theme` 的局部覆盖会失真），因此只作退路，不作首选。
**一旦走退路，必须在 `overlay.rs` 模块头记为正式分歧**（照 E2 的先例）。

---

## 六、L3 · 模态语义

`modal_barrier.rs` 目前是纯逻辑 port，接上两个已经是活的模块：

| 要素 | 接给谁 | 上游锚点 |
| --- | --- | --- |
| 吞点击 / 外部点击关闭 | `tap_region.rs`（活） | `ModalBarrier`、`TapRegion` |
| 焦点困住 | `focus.rs`（活） | `FocusScope`、`ModalRoute` 的 focus scope |
| barrier 之下语义不可达 | `semantics.rs`（活） | `ExcludeSemantics`、`barrierSemanticsDismissible` |
| Esc 关闭 | 暂走 `app.rs` 的 `on_key` | `DismissIntent` |

> `actions.rs` / `shortcuts.rs` 目前也是纯逻辑 port（`use crate::framework`
> 计数 0）。Esc 走完整 intent 体系是 P4 另一条目的事，**不阻塞本线**，先用
> `on_key` 接出来，等 intent 体系落地后替换。

---

## 七、L4 · 消费者回填

L0–L3 就位后，以下全是纯增量、每件独立可交付、每件都能单独进相册 demo：

| 消费者 | 现状 | 补的是 | 上游锚点 |
| --- | --- | --- | --- |
| ✅ Tooltip | `tooltip.rs` | portal + 定位 | `Tooltip`、`RawTooltip` |
| ✅ SnackBar / ScaffoldMessenger | `messenger.rs` | 宿主 | `ScaffoldMessengerState.showSnackBar` |
| ✅ PopupMenu | `popup.rs`；`menu.rs` 那段「没有 owner」已删 | owner 有了，动画回来了 | `_PopupMenuRoute` |
| ✅ Drawer | `drawer_host.rs`；`drawer.rs` 那段已删 | 246ms 滑入回来了 | `DrawerController` |
| ✅ showDialog / show*Picker | `dialogs.rs` | 已是命令式 API | `showDialog`、`showDatePicker` |
| ✅ Autocomplete options view | `autocomplete_view.rs` | portal | `RawAutocomplete` 的 `OverlayPortal` |
| ✅ 文本选择手柄 / 工具条 | `selection_host.rs` | 三个 entry + L0 坐标 | `TextSelectionOverlay` |
| ✅ Magnifier | `magnifier_host.rs` | entry + L0 坐标；放大本身待引擎 | `MagnifierController` |
| ✅ DragTarget feedback | `drag_feedback.rs` | entry + L0 坐标 | `Draggable` 的 overlay entry |
| ⏸ Hero | `heroes.rs` 已 port | overlay + **Navigator** | `HeroController` |

**回填顺序建议**：Tooltip 第一（最小闭环，验证 L0+L1+L2 三层同时正确）→
PopupMenu（验证 `popup_menu_offset` 与 L0 对接）→ SnackBar → Dialog 函数族 →
其余按需。

**实际回填完毕**：按上面的顺序走完，九件全部落地；Hero 按 §8 留给 Navigator 那
条线。三个「portal + L0 坐标」的消费者共用同一条缝——全局进、overlay 局部出——
为此补齐了 L0 缺的那一半 `RenderRef::global_to_local`。过程中挖出两个真缺陷：
`RenderMetaData` 缺上游 `MetaData.behavior`（`deferToChild` 之下 `DragTarget`
的注解永远读不到），以及宿主写共享 cell 不会让 entry 重建（`EntryRefresh`）。
详见 `PORTING_STATUS.md`。

---

## 八、与 Navigator 的关系

**单独一步，放在本线之后。**

上游是 Navigator 依赖 Overlay，不是反过来。先落 Overlay 就能解锁 L4 一整列；
Navigator 那条线带来的是路由栈、返回键、`Future` 结果语义与 `ModalRoute` 的
过渡动画，`routes.rs` / `navigator.rs` / `widgets_app.rs` 的已 port 逻辑到那时
才有落点。Hero 是唯一跨在两条线上的消费者，排在 Navigator 之后。

---

## 九、施工顺序（不按层号顺序开工）

| 步 | 内容 | 出口判据 |
| --- | --- | --- |
| **S1** | **L2 的 20 行垂直切片**：一个 portal 把一个 `Container` 交给一个手搭的 theatre | 它画在屏幕角落而不是 portal 处，且读到 portal 处的 `Theme` |
| S2 | L0 全量 + 差分测试 | 48 个容器的正反变换互逆 |
| S3 | L1 宿主接线 | `overlay.rs` 旧单测全绿 + 新树测 |
| S4 | L2 正式实现（`OverlayPortal` 公开 API） | 继承上下文测试 + z 序测试 |
| S5 | L3 模态语义 | barrier 吞点击、焦点不外泄、Esc 关闭 |
| S6 | L4 逐件回填 | 每件一个相册 demo 页 |

**S1 先行的理由**：它是全案唯一的赌点。切片成立，后面全是量的问题；不成立，
S2 的 48 处机械工作会白做一半（退路方案对 L0 的需求不同）。

---

## 十、工程接线与验收门

工程接线照 `PORTING_PLAN.md`「每个新模块固定三步」：

1. `lib.rs` 加 `pub mod x;`（字母序）→ `pub use x::{...}` → 需要时进 prelude；
2. `src/flutter/rust/BUILD.gn` 里 `rustflutter_unittests` 的 `sources` 补一行；
3. 测试 inline 写文件尾 `#[cfg(test)] mod tests`，跑法
   `src/flutter/rust/run_rust_tests.py`。

验收门照母文件五条，本线额外两条：

6. **L0 差分测试覆盖全部 48 个 `hit_test_children` 实现者**，不允许"这个容器
   不会承载弹层所以跳过"—— `transform_to` 是通用设施，缺一处就是一处静默错位；
7. **每个从「纯逻辑 port」转为「活模块」的文件，模块头的「What is not here」
   段必须同步更新**，删掉已经补上的条目。`drawer.rs` 和 `menu.rs` 里那两段
   "没有 owner 所以没有动画"的说明，是本线完成时必须消失的文字。

---

## 十一、台账影响

`PORTING_PLAN.md` 波次表 P4 的「Overlay 完整化」条目由本文取代。
本线完成后需要在 `coverage_ledger.json` 里改判的上游类（非穷举）：

`Overlay`、`OverlayState`、`OverlayEntry`、`OverlayPortal`、
`OverlayPortalController`、`ModalBarrier`、`AnimatedModalBarrier`、
`Tooltip`、`SnackBar`、`ScaffoldMessenger`、`ScaffoldMessengerState`。

`RenderObject.applyPaintTransform` / `getTransformTo` / `RenderBox.localToGlobal`
属 rendering 层，L0 完成后一并改判。

---

*本文行号引用为 2026-08-20 的快照，会随代码漂移；符号名是稳定锚点。*
