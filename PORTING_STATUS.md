# 与上游 Flutter 的对齐差距

> 范围：widget 层到 paint 层（`framework.rs`、`render.rs`、`painting.rs`、`app.rs` 帧序）。
> 引擎侧、平台通道、无障碍、Android 宿主不在此范围（见 `git show fc265dc:PORTING_STATUS.md`）。
> 上游锚点：`flutter/flutter @ cf97bfbcb9f`，锚点给符号名不给行号。
> **每条都照上游实现写，不要照描述写。改完 Windows 和 Android 都要验。**

---

## 一、要做的（余账）

### 手势（`gestures.rs`）
1. **tap 的 `kPressTimeout` 死线**：上游构造即挂 100ms 死线，超时失去候选资格；需在 `tick` 里走定时路径。
2. **tap 预接收 slop 应自持 18px**（`kTouchSlop`），现误共用 pan 的 36px；与上一条一并拆。
3. **`on_drag_cancel`**：上游区分取消与 `onDragEnd`；这里取消只发零速 `on_drag_end`。
4. **scale 降级**：上游剩一指降为 pan 继续走，并按 `computeScaleSlop`/`computePanSlop` 起手；这里第二指落下即起手、抬起任一即结束。
5. **`SignalKind::ScrollInertiaCancel` 已解析未分发**：上游路由给拖拽识别器停惯性滚动；这里落到 `on_hover` 兜底。

### 滚动（`scrolling.rs`/`scrollbar.rs`）
6. **thumb 的 hover/drag 态**：上游 idle 0.3 / hover 0.75 / drag 0.65；这里只有 idle。做交互式滚动条时一并做（thumb 拖动整套）。
7. **容差常量按 dpr=1 写死**：上游从 `devicePixelRatio` 推（`toleranceFor`）；等 `Scroll` 门面知道 dpr 时改。

### 控件/外观
8. **`Container` 的 `constraints` 参数**；顺带 `foregroundDecoration`/`transform`/`clipBehavior`。
9. **`AppBar` actionsPadding**：上游有效缺省为零，这里内缩 16（代码已自陈为故意，改动需过一遍相册）。

### 已完成（备查，勿重做）
- 语义节点被祖先裁剪切掉（含与上游的取舍：本桥无 `hidden` 概念，出窗节点按剔除处理）。
- `ClipRRect` 逐角圆角（`BorderRadius`/`directional` 全套已接上，`f32` 保留为简写）。
- `TextOverflow::Fade`（saveLayer + modulate 渐变已落地）。
- 第二轮逐 API 审计修复：widget 簇 9 项、手势簇 3 项、滚动簇 6 项、控件簇 15 项（Material 3 默认值对齐等）。

---

## 二、明确不做的（连理由一起，免得被当成待办）

| 事项 | 理由 |
| --- | --- |
| `BoxConstraints::biggest()` 无界轴返回 `min`（上游返回 ∞） | 故意安全化：它是 `RenderDecoratedBox` 无子时的尺寸来源；要动先补 `computeSizeForNoChild` 对等物 |
| `RenderOpacity` 全透明不参与命中 | 比上游严，不是译错；放开会把点击交给正在淡出的东西 |
| `ListTile` 尾部 32 下限 | 仅当尾部窄于 16 才有差，这里尾部没有窄于 16 的 |
| 摩擦模拟一族（`constantDeceleration`/`BoundedFrictionSimulation`/`through`/`timeAtX`/`snapToEnd`/`ScrollSpringSimulation` 等） | 全系于未移植的弹跳物理；立项弹跳时一并补 |
| `ClampingScrollSimulation` 的 tolerance 注入面 | 上游该参数只进 debug assert，无行为差 |
| 模拟用 `f32`（上游 `f64`） | 只在极端拖拽暴露，逻辑像素下无感 |
| `scroll_by` 顶边发 `Overscroll` | 故意：让顶边拖动也能亮起滚动条 |
| 滚动条渐隐计时为帧粒度 | 通知监听没有时钟 |
| 越界弹簧模拟 | 同弹跳物理账；`set_extent` 布局时钳正 |
| sliver 的 center/anchor 反向增长、keepAlive | `render.rs` 相应处有自陈 |
| `LayoutBuilder`/`BuildScope` | 无"布局中回头 build"通路；等价物是 `RenderSizeReporter`（量约束、下一帧构建）。记录在案的形态差异 |
| forcePress 手势 | 宿主硬编码 pressure=1.0，照上游阈值会让每次点击都触发力按压；等引擎给真实压力数据 |
| `computeDryBaseline` | dry 路径用缓存湿基线顶替，常见场景更准；真要算得给 dry 协议加基线位 |

**`covered` 账上曾补完的大项**（已从缺口表清零）：元素层 top/bottom sync、GlobalKey/reparent、Wrap runAlignment、dry layout、paint/relayout 脏列表、留存层就地重录、sliver 协议全套、`ScrollMetrics` 第四字段、只收 child 的 `Scrollbar`。

---

## 三、不要弄坏的（已逐条比过，是回归线）

- **盒协议**：`BoxConstraints` 的 `enforce`/`deflate`/`loosen`/`constrain`/`tight*` 与 `box.dart` 逐字同义。
- **命中测试协议**：框里且（孩子命中或 `hit_test_self`）才加自己；覆盖 `hit_test` 的只有 `RenderIgnorePointer` 与 `RenderPointerRegion`。
- **`RenderRef::layout`/`mark_needs_layout` 早退**、`update_from` ≙ `updateRenderObject`、repaint boundary 与留存层。
- **照抄的算法**：`RenderAspectRatio`（六个 if 的顺序）、`RenderPadding`、`RenderAlign`、`RenderConstrainedBox`、`RenderWrap::break_into_runs`（布局与固有尺寸共用一份）、`RenderFlex` 三趟。
- **`Container` 层序**：Align → Padding → Decoration → Constraints → Margin；padding 在装饰里面，只有真有装饰才加 Decoration 层。
- **标题栏先量两头再给中间**（`RenderNavigationToolbar` 逐行照抄 `_ToolbarLayout`）；`ListTile` 同账走 flex。
- **动作用 `MainAxisSize::Min` 的 Row 包一层**：少了这层尾部吃掉整条、标题分到 0。
- **条是定高的**：`K_TOOLBAR_HEIGHT`=56，标题 `soft_wrap=false`+省略号+单行，字缩放夹 1.34——三样是一套。
- **`PaintContext` ≙ `PaintingContext`** 双重身份，懒起 picture。
- **语义三道闸门**：没人听、没人标、没变——任一成立则一帧一个字都不发。

---

## 四、执行器（`task.rs`，2026-08-23）

- 上游 `Future` 不是并发原语，是"稍后在这根线程上叫我"——回调译法丢的不是语义是组合。`core::future` 给词汇不给执行器，故加 `post_task`：七个宿主回调里唯一允许任意线程调用的。
- **线程亲和是编译期的**：future 不带 `+ Send`、任务表 thread_local；跨线程的只有一个 `TaskId`（信号可跨，状态不行）。
- 两处承重顺序：排空点在动画相位与构建相位之间（即上游 `FlushMicrotasksNow` 位）；`task::detach` 在 `services::detach` **之后**（后者结算等待中任务的 oneshot）。
- 只有真跑了东西才要帧——"在等"不是画的理由。
- 顺手修的两处静默失败：`services::MESSENGER` 与 `painting::IMAGES` 是 thread_local，worker 够到的是另一份空的；现都走单一入口加断言。

**记录在案的分歧**：`async_builder` 的 poll 形态保留（流/轮询硬件的正确形状）；`future_builder` 收 `Result<T,String>`（否则会失败不了的快照状态）；`StreamBuilder` 仍 `async_builder` 形；`TickerFuture::settled` 是方法（孤儿规则）；`RefreshIndicator` 无 future 门面；`sleep` 只给应用代码，框架 deadline 全留帧时钟；上游 future 不可取消、Rust drop 即取消，故 `detach` 是丢弃而非完成；`RfAppHost` 线程断言无单测（判定抽成值单测）。

---

## 五、有意留下的边界

- **animated_icons 十四份美术数据未 port**：上游是 3.4 万行生成产物，誊抄无法校验；机器（插值/镜像/合成）完整可用，`AnimatedIcons::data` 返回 `None` 而非空图标。
- **`@Preview()` 注解无对应物**：Rust 侧应是 proc-macro，等有消费者再写。
- **`SemanticsConfiguration`/`SemanticsData` 字段取已建模子集**（platform view id、link URL、role 等约六项无对应物）；合并规则与断言是真正 port 的部分。
- **`SemanticsOwner` 不做脏节点增量**：这边是扁平列表的值，重走加 diff 到达同一效果。

---

## 六、历程概览（414 轮压缩）

| 阶段 | 内容 |
| --- | --- |
| 2026-08-17 起 | 完全覆盖计划第一簇：十层逐类移植 |
| 2026-08-20 | **1888/1888，MISSING 归零**；随后逐轮读上游细节（assert 的形状、"眼睛量的"数字 98 处其中 74 在 cupertino——Material 是公开规范、iOS 只能量） |
| 2026-08-21 | **尺子修正**：第一把尺子有四个盲区（层目录漏 3 个、不递归、数进测试代码、私有顶包），分母 1888→1930，二次归零；同日起 `covered` 桶审计与"未接线队列"（Badge、Tooltip、进度指示器、SnackBar、Icon、TabBar 等 16 条） |
| 2026-08-21~23 | 六把尺子陆续建立（未接线、顺序、重复移植、解析未用、注释出处核对）；38 个主题无人读 → 逐一接线或记档 |
| 2026-08-24 | **MISSING 再归零但不等于完成**：新增记账类别，清 81 条空映射；空洞谓词、空 stub、未测不变量成队列入账 |
| 第 151~228 轮 | 绘制层补课：unpainted 归零（画弧 stub、BoxDecoration、24 小时表盘高亮等）；语义/无障碍接线；**lerp 大审计**——371 处混合 284 处没人看着，逐族补方向测试 |
| 第 229~330 轮 | 主题接线收尾（snack bar、FAB、chip、AppBar 等）；文本选择与编辑手势逐平台对齐（粒度、手柄、浮动光标、IME 条） |
| 第 331~337 轮 | 追查"布局过的树报 0×0"——**结论：根本没有 bug**，是自己的测试没按帧的方式布局；21 处测试有同型写法 |
| 第 340~414 轮 | Scaffold 槽位（FAB 十九种摆法、底栏）、导航语义（读屏绕行抽屉）、`AnimatedSwitcher` 曲线与记录上一个孩子、`Actions` 往上找、`CapturedContext`（build 之外能问树的 context） |

**当前状态**：Rust 6476 测试通过、`cargo fmt` 干净；C++ 34 gtest 全过；gallery 354 通过；十六把尺子全部 exit 0。

**下一步**：造 `Shortcuts`（焦点节点收键 → `intent_for` → 交 `Actions`）。**先查两件事**：`shortcuts.rs` 的 `intent_for` 签名；`focus.rs` 能否注册"只收键、不接受焦点"的节点（上游 `Focus(canRequestFocus: false)`）——否则会往 Tab 遍历里塞空站。

---

## 七、方法论教训（反复出现，值得单独记）

1. **讲得通的读法 ≠ 上游那么写**——九条被测试/mutation 抓出的自己的错，每条都"讲得通"。
2. **尺子（审计脚本）本身会错**：每把尺子首次运行都先抓出自己的错（多报、漏报、锚点不唯一、盲区）；下结论前先审计尺子。
3. **"只有一个候选"的测试测不出"找的是哪一个"**；防"两条臂算出同一个数"要专门写测试。
4. **变异扫描作验收**：每条承重规则逐条强制改错，红了才算被钉住；活下来的变异要么是真 bug 要么是测试盲区。
5. **注释会过期**：`stale_notes.py` 专查"说自己没有某样东西"的注释。
6. **并发写文档的教训**：多工作线并行提交时，断言与记账都被踩过。

## 八、值得记的上游细节（精选）

- 两个 `TextDirection` 枚举顺序相反（dart:ui rtl=0，本 crate Ltr 在前）——直接 cast 会让读屏方向全反且不报错。
- `SemanticsTag` 按对象身份、`CustomSemanticsAction` 按值发 id，方向相反且都对。
- `AnimatedIcon` 关键帧是"列表里的位置"不是首尾比例；镜像是转 π 再平移不是水平翻转。
- `Page` 的两个无 key 页面是匹配的（`null == null`）——声明式 navigator 重排无 key 页面像"内容变了"的原因。
- Material 与 Cupertino 的 `maxLength`：一边用哨兵 -1 重载值，另一边加 `MaxLengthEnforcement` 模式——加模式的那个还能同时给真数字。
- `isDivider` 的意思是"别在我旁边画线"，`hasLeading` 的答案除了自己所有人都要读——接口讲的是邻居不是自己。
- `_kInnerRadius = 2.975`：四位有效数字、无注释——精度告诉你它是量的，只有注释能告诉你量的是什么。
- Cupertino 主题"没写 brightness"的意思是"跟着设备走"，这个"没写"必须穿过补默认层活到 `MediaQuery`。
- 数字精度即出处：眼睛量的是 2.0/300/10.0，尺子量的是 0.0835（方法写在注释里）。

---

## 第 415 轮：键终于能变成 intent——挡路的是**环境键盘只公开了四个 bool**

按"下一步"查两件事，两件都过：
`ShortcutRegistry::intent_for(event, keyboard)` 在；
`Focus::new(..).with_traversable(false).with_on_key(..)` 也在——
**能收键但不做 Tab 站**这种节点本项目本来就支持，
正是上游 `Focus(canRequestFocus: false)` 的效果。

但第三件事挡住了，而它是查这两件时才露出来的：
`with_on_key` 的处理器只拿得到 `&KeyEvent`，
而 `ShortcutActivator::accepts` 需要**整个 `Keyboard`**。
`keyboard/mod.rs` 里那个环境单例**只存了四个修饰键 bool**。

四个 bool 答不了 `KeySet` 要问的问题：它比的是**按下集合的大小**，
好让 Ctrl+Shift+A 按着的时候 Ctrl+A 不要触发。
而上游的 `HardwareKeyboard.instance` 本来就是**整个键盘**
（`logicalKeysPressed` / `physicalKeysPressed` / `lockModesEnabled`）。
那个字段的注释自己都写着"这就是那个单例，带着同样的三个问题"——**少的正是其余的问题**。

所以先把环境键盘变成整个键盘（`with_keyboard(|kb| ..)`、`note_keyboard`），
`modifiers()` 改成从它派生的窄视图而不是第二份状态。

### 于是 `shortcuts(id, registry, child)` 成型

一个 `traversable(false)` 的焦点节点，`on_key` 里
`intent_for` → intent → 用**第 414 轮捕获的 context** 交给
**第 413 轮的往上找** → `Actions`。
三样东西此前各自齐全、从没见过面，所以**这个 crate 里从来没有一个键变成过 intent**。

一条上游写着的落空规则：**匹配到 activator 但没有 action 接的键，不算 handled**。
上游 `Actions.invoke` 找不到时返回 `KeyEventResult.ignored`。
报 handled 的话，一个 scope 会把它叫得出名字却服务不了的快捷键**全吃掉**，键就消失了。

### 变异扫描 8 个，第一遍 6 红、2 绿

- "modifiers 从空键盘读"活了：`modifiers()` 唯一的调用者在 `editable.rs`，
  而**没有任何测试测过它跟着环境键盘走**——是我改写它时继承下来的旧空白。补了。
  写那条测试时自己先踩了一次：`control()` 读的是**物理**键
  `PhysicalKey::CONTROL_LEFT`，和逻辑常量不是一个值，
  拿逻辑值当物理值填，报的是"什么都没按"。
- 另一条"活着"的**是我自己写坏的变异**：`let _once = captured.clone();`
  加了个绑定、语义没变，**根本不是覆盖缺口**。
  记一条纪律：`assert old != new` 比的是**文本**不是**含义**，挡不住无效变异。
  去掉之后，七条真变异第一遍全红。

### 门这一轮起变宽

上一次 `ninja -C src/out/host_release/` 直接编不过（showcase 里 `component(Divider)`
在 `Divider` 长出字段之后就坏了，坏了好几轮没人发现），
因为**我的门只编 `rustflutter_engine` / `rust_lib` 和三个测试二进制，从来没编过示例**。
从这一轮起门是三个目录各跑一次**不带目标名的 `ninja`**。

顺带修了 pre-commit 钩子：它用 `--edition 2021` 查非 crate 的 Rust 文件，
而构建用的是 `rust_edition = "2024"`，两者对 import 排序的意见相反——
于是**任何按编译器的版次写的文件都提交不进去**。

尺子：十六把全部 exit 0。门：Rust 6483 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 354 通过；**三个目录的默认目标全部编过**；三个测试二进制重建。

**下一步**：`shortcuts` 有了，但**还没有人在树里用它**——
`app.rs` 的 Escape 仍然是手写的那条。
下一轮把根上那一段换掉：`RfApp` 装一个 `Actions`（`Dismiss` → 收 tooltip / 关模态框）
和一个 `shortcuts`（Escape → `Intent::Dismiss`），让第 412 轮那个顺序**由树的形状决定**
而不是由一个 `if` 决定。
但**先查一件事**：`RfApp::frame` 里那棵根树是**每帧都重建**还是只在
resize / images_arrived 时重建（第 405 轮量过一次，答案是后者）。
如果是后者，那么根上的 `Actions` 里那些 action 闭包**捕获的是哪一帧的东西**就要想清楚——
捕获了 `&mut self` 里的东西会在下一帧过期。
先确认根树重建的条件，再决定 action 该怎么拿到它要动的状态。

---

## 第 416 轮：对话框弹出来时**没人拿键盘**，所以它自己的按键处理器一个都不跑

按"下一步"查根树重建条件，顺着往下查，撞到的是更基础的一条。

上游 `KeyEventManager` 的分发是
`[primaryFocus!, ...primaryFocus!.ancestors]`——**从获得焦点的节点往上走**。
那个 `!` 不是疏忽：`FocusManager` 保证 primaryFocus 非空
（`_primaryFocus == null && _markedForFocus == null` 时回落到 `rootScope`）。
而真正让 app 根上的 `Shortcuts` 能收到键的，是**路由自己抢了焦点**——
`_ModalScope` 是 `FocusScope(node: ..., autofocus: true)`。

本项目 `focus.rs::dispatch_key` 在 `manager.focused` 是 `None` 时**直接返回空链**，
而 `theatre::show_modal` 只调 `trap_focus`（装个陷阱），**从不移动焦点**。
于是：**弹出一个对话框，键盘还在原地（多半是 None），
对话框自己的 `on_key` 一个都不会跑，直到用户按一下 Tab。**

补上上游那套**待批的 autofocus**：
`autofocus_in(trap)` 记下请求，`apply_pending_autofocus()` 批准。
是"待批"而不是当场生效，理由和上游 `_pendingAutofocuses` 一样——
`FocusScope(autofocus: true)` 是在**构建时**说出愿望的，
而那一刻它要聚焦的节点**一个都还没注册**（这个框架里焦点节点是靠 build 注册的）。
所以批准点在 `RfApp::frame` 的 **build 之后**，正对上游
`applyFocusChangesIfNeeded` 在帧里的位置。

三条按事实定的规矩：
- **只有正在生效的那个陷阱可以领**——否则一个在帧到达前就被关掉的对话框，
  它的请求会拿**页面**的停靠点去批（没有陷阱时 `traversal_order` 答的就是整页），
  把键盘塞到用户已经关掉的对话框后面那一页上。
- **不跟已经选好的抢**——上游 autofocus 的原话是"只在该 scope 还没有焦点时才聚焦"。
- **一个可聚焦停靠点都没有的对话框，不动键盘**——
  只有文字的确认框没有可给的地方，硬给会越过陷阱落到它盖住的那一页上。

变异扫描 7 个，第一遍 5 红。两条活的都**如实记下、没有编测试去糊**：
- "外层对话框把键盘从内层抢走"：那条 `pending.contains(&active)` 守卫
  **在现有规则下推不出可观察差异**——唯一能让"生效的陷阱没提过请求"的情形，
  是更外层的对话框还开着，而那时焦点已经在它里面，下面的 `already_inside` 照样拒绝。
  写进代码注释了。
- "帧里根本不批准"：`RfApp` 在测试里造不出来，
  `apply_pending_autofocus()` 这个函数本身测得很透，
  但"帧里有没有调它"这条接线**测不到**——和第 412 轮 `on_key` 那个守卫同一族。

### 顺带修了一把**永远清不掉的尺子**

改了 `rustflutter_host_win.cc` 之后 `stale_engines` 报两个 android 引擎陈旧，
而 `ninja` 说"无事可做"——因为那是**只有 Windows 才编的文件**，
android 目标从来没编过它，**任何重建都清不掉这个红**。
一把永远红的仪器比没有更糟：它训练人忽略它。
改成**每个引擎只和自己平台编的源比**，并做了标定：
碰共享的 `rust_app_api.h` → 三个全红；碰 android 专属源 → 只有两个 android 红。

尺子：十六把全部 exit 0。门：Rust 6488 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 354 通过；三个目录默认目标全部编过；三个引擎重建。

**下一步**：用户报了一个真缺陷，已经查到底了，下一轮修它。
右键点文本框：Windows host 此前**根本不转发右键**（已修，见 `72df0b8`），
现在事件带着 `buttons=2` 到达框架了，但菜单**还是不出来**。
探针打出命中路径：

    path target=40005 tap=true secondary=false   <- 吃掉了这次按下
    path target=302   tap=false secondary=false

`editable.rs:3727` 那个带 `on_secondary_tap` 的区域**根本不在路径上**，
而 `focus.rs:1649` 的 `Focus` 用**同一个 id** 也建了一个 `RenderPointerRegion`，
它只有 `on_tap`。**两个区域共用一个 id，外面那个把里面那个的处理器盖住了。**
先查清楚这两个区域的嵌套关系和 `RenderPointerRegion` 的命中规则
（为什么内层没有出现在路径里），再决定是合并成一个区域、还是让命中把两层都收进来。
另外还有第三件事，是同一个报告里的**外观**问题：
本项目写死 `MaterialTextSelectionControls`（手机上那种药丸条），
而上游桌面用的是 `DesktopTextSelectionControls`（截图里那种方角菜单）——
`text_selection_controls.rs` 里那些类现在还是没有生产者的数据。

---

## 第 417 轮：右键菜单这条路查到底了——**文本框自己的指针区域根本不在命中路径上**

这一轮**没有改代码**，是一轮诊断。用户报的缺陷分成三件事，第一件上一轮已修，
这一轮把第二件钉死到了具体位置。

### 已确认（第 416 轮修的那件）

Windows host（`rustflutter_host_win.cc`）**只处理 `WM_LBUTTONDOWN/UP`**，
`MakePointerData` 里 `data.buttons` 写死成 primary。右键**根本不产生指针事件**。
已修（`72df0b8`），并用探针验证：修之前什么都没有，修之后
`Down buttons=2` 确实到达框架。同时给 `tools/grab_window.py` 加了 `--srclick`，
这条路以后可复现。

### 这一轮钉死的第二件

事件到了，菜单**还是不出来**。把探针打进命中路径，在 Text fields 演示的
Name 字段上、沿 y 从 200 到 237 逻辑像素**扫了一遍**，每一次路径都一样：

    target=40005  tap=true long=false down=false secondary=false   <- 最内层
    target=302    ...
    target=128516096 ...

`editable.rs:3727` 那个区域挂着 `with_tap` + `with_long_press` +
`with_pointer_down` + `with_secondary_tap` 四样，
而命中到的这个**只有 tap**——所以它不是文本框的区域。

再把探针打进 `RenderPointerRegion::hit_test` 本身，只打 id 40005：

    region 40005 size=460x764 pos=(233, 544) contains=true secondary=false

**460×764 是整页的尺寸**，不是一个文本框。而且整个进程里 id 40005 的区域**只有这一个**。

所以结论是确定的：**`TextField` 自己的 `RenderPointerRegion` 一次都没有出现在命中路径上。**
`RenderPointerRegion::hit_test` 的写法是
`hit_target = self.child.hit_test(..) || self.hit_test_self(..)`，
而路径里 40005 **排在最前**（路径是从内往外记的），
说明它的**孩子那一侧压根没命中**，它是靠自己 Opaque 才进的路径。
也就是说：**在这个页面上，那个页面级 Focus 区域底下的东西，命中测试走不下去。**

上一轮"两个区域共用一个 id、外面盖住里面"的猜测**是错的**，如实更正：
根本不是遮盖，是**下降在更上面就停了**。

### 没有确认的，不写成确认

- 这个演示页里**其它控件**（比如密码那个眼睛）是否也点不动——
  我跑了一次像素差对比，得到 0，但事后看截图发现那一跑**导航落在了 App bar 演示上**，
  点的根本不是眼睛。**这条不算数**，下一轮重做。
- 具体是哪一个祖先停住了下降，还没定位。

### 第三件（同一份报告里的外观问题）

上游桌面用 `DesktopTextSelectionControls`（截图里那种方角菜单），
本项目 `editable.rs` 写死 `MaterialTextSelectionControls`（手机上的药丸条），
`TargetPlatform::host()` 只被用来选按钮集合、没被用来选工具条形状。
`text_selection_controls.rs` 里那些桌面类目前仍是没有生产者的数据。

**下一步**：先把"这一页还有什么点得动"这件事**重做一遍并做对**——
导航要先截图确认落在了 Text fields 上再点，
然后点密码的眼睛、点 Life story 那个多行框，各做一次像素差。
这决定了故障有多大：如果整页都点不动，那这就不是右键的问题，
而是这一页的命中测试断了，右键菜单只是它的一个症状。
确认之后再往上找那个停住下降的祖先——
建议的探针是在 `HitTestResult` 记录时打印每一层的 id 与 size，
从页面级那个 460×764 的区域往下追。

---

## 第 418 轮：不是右键的问题——**演示页里的命中测试整个走不下去**

上一轮留的"下一步"要求把严重性重做一遍并做对。做了，结论比右键菜单大得多。

### 先做对的那件事：把它变成单元测试，而不是对着窗口点

上一轮那次像素对比之所以作废，是因为**导航落错了页**而我事后才发现。
这一轮换了办法：在 `flutter_gallery_unittests` 里把演示页装起来、按 460×764 布局，
**在整页上打网格做命中测试**，统计有多少个探测点能到达
想要 tap / long-press / secondary-tap 的区域。
用网格而不是一个坐标——**一个打偏的坐标什么都证明不了**，这正是上一轮翻车的地方。

**这个测试是绿的。** 也就是说：在一棵光秃秃的树里，
文本框自己那个带 `on_secondary_tap` 的区域**是够得着的**。
所以这**不是 `TextField` 的缺陷**，widget 树是对的。

### 那么问题在运行时的包裹上，而且比想象的大

把探针打进**每一个** `RenderPointerRegion::hit_test`，打 id、size、contains。
在真实 app 里点演示页内的任意位置，日志只出现两个区域：

    region id=302   size=460x764  contains=true
    region id=40005 size=460x764  contains=true

**两个都是整页大小，而 40005 底下再没有任何区域跑过 hit_test。**
对照之下，在首页点击时日志里出现了三十个不同 id 的区域——**首页的下降是正常的**。

佐证：演示页头部那个**返回箭头也点不动**（连点两轮都没退回去）。

所以事实是：**进了演示页之后，命中测试在页面级区域就停住了，
底下所有控件——返回箭头、密码的眼睛、七个文本框——一个都收不到指针。**
右键菜单出不来只是这件事的一个症状。

上一轮"文本框自己的区域不在命中路径上"没有说错，但把因果说小了：
不是那一个区域的问题，是**它上面某一层不往下走**。

### 没有确认的

具体是哪一个祖先停住了下降，还没定位。已知它在
`id=40005` 那个 460×764 的区域**里面**（40005 自己的 `hit_test` 跑了，
它的 `self.child.hit_test(..)` 没有让任何 `RenderPointerRegion` 跑起来）。

尺子：十六把全部 exit 0。门：Rust 6488 通过；C++ 34 个 gtest 全过；
gallery **355** 通过（新增那一条）；三个目录默认目标全部编过。

**下一步**：从 `id=40005` 那个区域往里追一层。
它的孩子是什么、那个孩子的 `hit_test` 怎么写的——
重点看**只画不测**的那一类：`RenderPointerRegion::hit_test` 自己是
`self.size.contains(position)` 把门，所以任何一个把孩子画在自己盒子**外面**
（或者画的时候加了偏移、测的时候没减）的祖先，都会让底下整片区域够不着。
建议的探针：在 `RenderBox::hit_test` 的默认实现和几个容器
（`RenderFlex`、`RenderStack`、裁剪类、滚动视口）里打点，
看哪一层收到了 contains=true 却没有把位置传下去。
另外别忘了第三件事仍然挂着：桌面上该用
`DesktopTextSelectionControls` 而不是写死的 `MaterialTextSelectionControls`。

---

## 第 419 轮：往下追了一层，排除了三个嫌疑，**没有定位到**——并且换掉复现的办法

这一轮**没有改产品代码**。把结果如实记下来，包括没做到的部分。

### 找到了包裹演示页的那一层

`pages/demo.rs::demo_wrapper` 的最外面是：

    Container::new()
        .with_padding(EdgeInsets::only(16, 0, 16, 16))
        .with_child(ClipRRect::new(10.0, rendered))

里面依次是 `provide(theme)` → `app::with_overlay` → `DemoArea`（一个
`ConstrainedBox`，`min_height = 页高 - 56 - 16`，max 是 `INFINITY`）→ 演示自己的 `stage()`。

### 读代码排除的三个

- `RenderPadding::hit_test_children` **是减了内边距的**
  （`position.translate(-insets.left, -insets.top)`），没问题。
- `RenderClipRect::hit_test_children` 直接下传，没问题。
- `RenderClipRRect` 同样没问题。

### 一个浪费了一轮探针的事实，记下来免得再犯

`widgets::ClipRRect::new(radius, child)` 返回的是 **`RenderClipRect`**，
不是 `RenderClipRRect`——那是另一个类型。
我第一次把探针打在 `RenderClipRRect` 上，跑出来 `count=0`，
差点据此断定"裁剪层根本没被走到"。**打错了类型的探针，报的是零，不是事实。**
和之前那些"仪器瞎了"的教训是同一族。

### 复现办法本身也得换

对着窗口点这件事**不可靠**：每次重启后列表的滚动量都不一样，
同样的 `--sclick 300,782` 这一轮进的是 Text fields、下一轮进的是 App bar，
我已经因此作废过两次测量。这一轮虽然加了"点之前先截图确认"的步骤，
但每验证一次就要多跑一遍应用，代价太高。

**正确的做法是把整个 gallery 装进单元测试里**：
第 418 轮那条网格命中测试证明了这条路可行（装 `stage()`、按 460×764 布局、
整页打网格），只是装的是**演示自己的根**，没有装外面那层包裹。
下一轮应当装 `pages::demo::page(...)`——它要一个
`GalleryState` 和 `StateHandle<GalleryState>`，
`GalleryState::default()` 有了，handle 得从一个 stateful 元素里拿，
所以多半要把整个 `GalleryApp` 装起来再导航到 demo 路由。
一旦装起来，网格扫描会**立刻**告诉我 secondary 计数是不是掉到零，
然后就可以在测试里一层层往下二分，秒级迭代，不用再开窗口。

尺子：十六把全部 exit 0。门：Rust 6488 通过；gallery 355 通过；
C++ 34 个 gtest 全过；三个目录默认目标全部编过。探针已全部还原。

**下一步**：把 `GalleryApp` 装进 `flutter_gallery_unittests`、导航到
`routes::DEMO` + slug `text-field`，跑第 418 轮那个网格扫描。
预期：`secondary` 会是 0（现在装光秃秃的 `stage()` 时不是 0）。
拿到这个"红"的测试之后，再从 `demo_wrapper` 往里逐层二分——
每一层加一个临时断言即可，不必再碰运行中的窗口。
另外仍然挂着第三件事：桌面上应当用 `DesktopTextSelectionControls`
而不是写死的 `MaterialTextSelectionControls`。

---

## 第 420 轮：把复现搬进单元测试，**排除了一整排嫌疑**——故障需要"导航"这一步

按"下一步"把整个 `Gallery` 装进 `flutter_gallery_unittests`，跑第 418 轮那个网格扫描。
迭代从"开一次窗口 + 手动确认落在哪一页"变成秒级。

`Gallery { theme_mode, route, slug }` 本来就能**直接开在 demo 路由上**
（无头渲染器就是这么用的），所以装起来很便宜。

### 一层层加上去，`secondary` 始终不掉

    只装 stage()                                  secondary > 0
    装整个 Gallery（route=DEMO, slug=text-field） secondary > 0
    + MediaQuery（475x857，app 的真实尺寸）        secondary = 1009
    + theatre::overlay                            secondary = 1009
    + TapRegionSurface                            secondary = 1009

`MediaQuery` 那一步是特意加的：`DemoArea` 要读它算卡片的最小高度，
**不给 MediaQuery 的树和 app 里的树布局不一样**——
第 418 轮那个测试之所以是绿的，一部分原因就在这儿。补上之后仍然是绿的。

也就是说：`demo_wrapper` 那一整条链
（`Container(padding)` → `ClipRRect` → `provide(theme)` → `with_overlay` →
`DemoArea` → `stage()`），加上 `RfApp` 外面那两层，**全都不是元凶**。

### 中途验了一次"构建到底跑没跑"

连着两次 `ninja: no work to do` 而数字一模一样，按自己的备忘录去查了——
把打印的前缀从 `SWEEP` 改成 `SWEEP2`，新前缀出现了，**构建是真的在跑**，
数字一样是因为**那两层确实没影响**。没有据"没变化"就下结论。

### 剩下的唯一差别：**app 是导航进去的，测试是直接开在那儿的**

`push_screen` 用的是 `navigator.push(route, Transition::SlideFromRight)`。
而真机日志里的形状正好对得上：**两个页面大小的区域嵌在一起**
（栈里的首页 + 进来的 demo 页），命中在最上面那个就停住。

一个滑入过渡会把页面包在一层平移里。如果那一层**画的时候偏移了、测的时候没有反算回去**，
或者过渡结束后把离场页留在了上面还继续接指针，下面整片就都够不着——
和观察到的现象完全吻合。

### 这一轮留下的东西

`pages/demo.rs` 里那条网格扫描测试留下了（356 通过）。它现在是**绿**的，
作用是把"这条链没问题"钉住：将来谁改了 `demo_wrapper` 把命中弄坏，它会红。
放在 `pages/demo.rs` 而不是 `app.rs`，因为 `app.rs` 工作区里
还压着一行别人未提交的改动（`reply_scroll.advance`），不该被我顺手带进提交。

尺子：十六把全部 exit 0。门：gallery 356 通过；三个目录默认目标全部编过。

**下一步**：直奔 `navigation.rs` 的 `Transition::SlideFromRight`。
先读它的渲染：过渡用什么包页面（平移？`RenderStack`？），
那个包装的 `hit_test` 有没有把位置反算回去，
以及**过渡结束之后离场的那一页有没有被摘掉**。
然后在 `pages/demo.rs` 那条测试旁边加一条：
先装 `route=HOME`，再走一次 `push_screen` 到 demo，跑同一个扫描——
预期 `secondary` 掉到 0。拿到红的那一条，就可以在测试里改一行验一次了。
另外仍然挂着：桌面上应当用 `DesktopTextSelectionControls`。

---

## 第 421 轮：过渡也洗清了——**被命中测试的那棵树，和最后一次构建出来的不是同一棵**

这一轮没有改产品代码。把上一轮指向的那个嫌疑查了个干净，结果是**它无罪**，
而洗清它的过程把假设逼到了一个更具体、也更值得查的位置。

### 三件读出来的事实

- `Presentation::offsets()` 在 `!is_transitioning()` 时直接返回 `SETTLED`，
  偏移和不透明度全是零/一。
- `page_stack` 在过渡结束后**直接返回 `current`**，
  `TransitionStack` 根本不进树。
- `TransitionStack::hit_test` 本身**是对的**：先按 `current_offset * size`
  把位置反算回去，再下传给 `current`。

### 一件在运行中的 app 里量到的事实

在 `page_stack` 里打点，从首页点进 Text fields，日志最后一行是：

    PROBE page_stack transitioning=false progress=1 motion=Pushing
      offsets=TransitionOffsets { current_dx: 0.0, ..., previous_opacity: 1.0 }

**过渡确实收敛了。** 所以稳定之后，app 里的 widget 树形状
和第 420 轮那条绿测试装的是同一棵。

### 于是矛盾就很尖锐了

第 418 轮在同一个 app 里量到的是：点演示页任意位置，
只有 `id=302` 和 `id=40005` 两个区域跑过 `hit_test`，**都是页面大小的**，
底下再没有任何区域被问到。

而"两个页面大小的区域嵌在一起"正是 `TransitionStack`（previous + current）的形状——
可过渡已经结束、`TransitionStack` 已经不在树里了。

**所以指向变了**：不是过渡算错了，而是**被拿去做命中测试的那棵渲染树，
不是最后一次构建产出的那一棵**。看得见的页面是稳定后的（画出来的是新树），
而手指问到的是别的东西。

`RfApp` 把树存在 `self.painted`（`app.rs:1234`，每帧 `insert`），
`dispatch_pointer` 从这里取。下一步要查的就是这条：
**存进去的 `root` 到底是不是这一帧构建出来的那棵。**

### 这一轮为什么没有直接量到底

预算用在了排除过渡上。剩下的那一步需要在 `dispatch_pointer`
和存树的地方各打一个点，比对同一帧的两棵树——是一次干净的实验，只是没跑成。

尺子：十六把全部 exit 0。门：gallery 356 通过；三个目录默认目标全部编过；
探针已全部还原。

**下一步**：只做一件事——**证明或推翻"命中用的树是旧的"**。
在 `RfApp::frame` 存树那一行前面打一个点，打出这一帧 `root` 里
`RenderPointerRegion` 的数量；在 `dispatch_pointer` 里打出
`self.painted` 里同样的数量。两个数字在稳定之后应当相等。
如果不相等，故障就是"存的树不对"，与文本框、右键、演示页统统无关，
而是一个会影响**所有**页面切换之后交互的框架级缺陷。
如果相等，那就得回头怀疑"两个 460x764 区域"的身份——
它们可能根本不是 previous/current，我此前一直是这么假设的，从没验证过。
另外仍然挂着：桌面上应当用 `DesktopTextSelectionControls`。

---

## 第 422 轮：**推翻第 418 轮的结论**——命中树是完整的，我一直在量错的那个演示页

这一轮最重要的产出是一条更正，而不是一个新发现。

### 第 418 轮说错了

第 418 轮的结论是"进了演示页之后命中测试在页面级区域就停住，
底下所有控件一个都收不到指针"。**这是错的。**

这一轮把探针改成"把命中用的那棵树里所有带 id 的区域全列出来"，
在**确认过确实是 Text fields 页**之后按下右键，列出来的是：

    d=10  460x764   id=302
    d=16  460x988   id=40005
    d=33  330x19    id=10000     <- 七个输入条
    d=34  330x19    id=10001
    d=33  330x19    id=10002
    d=31  368x57    id=10004
    d=32  336x19    id=10005
    d=33  322x19    id=10006
    d=32  40x40     id=10008     <- 密码那个眼睛
    d=32  370x19    id=10007
    d=31  83x40     id=10009
    d=9   48x48     id=1
    d=8   48x48     id=40000/40001/40002

**七个文本框、密码的眼睛、头部的图标，全都在树里。**
命中测试进得去演示页。第 418 轮那次"只有两个页面大小的区域"的测量，
是在 **App bar 演示**上做的——那一页在那个位置**本来就是空的**（只有一个 "Home" 文本），
所以"只有两个页面级区域"是**正确答案**，不是故障。

### 根因是我的复现手法，不是代码

在**同一次** `grab_window.py` 调用里既滚动又点击，落点不可靠：
同样的 `--sclick 300,782`，有时进 Text fields，有时进 App bar。
把滚动和点击**拆成两次调用**、中间截图确认，每次都对。
这个错误我已经犯了四次，这一次让一个跨了好几轮的结论作废。

### 那么真实情况是什么

在**确认过的** Text fields 页上右键，仍然没有菜单、字段也没有获得焦点。
所以缺陷是真的，但**不是"命中测试走不下去"**。

一条值得注意的尺寸：那些输入条只有 **19 个逻辑像素高**（330x19）。
固定坐标很容易在竖直方向上错过它——而"打偏的坐标什么都证明不了"
正是第 418 轮写下、这一轮又栽了一次的那句话。
另外 `id=40005` 的高度是 **988**，比 764 的视口高，说明这一页是滚动的。

### 这一轮没做到的

没有拿到**命中路径**（只拿到了树）。要判断字段区域到底有没有进路径，
需要的是路径探针 + 沿 y 扫一遍，而不是一个固定坐标。

尺子：十六把全部 exit 0。门：Rust 6488 通过；gallery 356 通过；
三个目录默认目标全部编过；探针已全部还原。

**下一步**：在**确认过的** Text fields 页上，把 `gestures.rs` 那个路径探针装回去，
**沿 y 从字段上沿到下沿逐像素扫**（不是一个点），
打印每次按下的完整路径。只需回答一个问题：
**`id=10000` 那条 330x19 的输入条，有没有出现在任何一次的路径里。**
- 出现了 → 命中没问题，故障在 `editable.rs` 收到 secondary 之后那一段
  （`toolbar_shown` 翻了但工具条没上屏），从那里往下查。
- 没出现 → 才轮到怀疑命中，而且这次有确凿的坐标范围可以说事。
把滚动与点击分开、点前先截图确认——这一条要写进步骤里，不要再省。

---

## 第 423 轮：在**确认过的**页面上重测——第 418 轮的结论是对的，第 422 轮的更正过头了

先把上一轮的更正再更正回来一半，这两条都要说清楚：

- 第 418 轮那次测量**确实是在错的演示页上做的**（App bar），这一点第 422 轮说对了。
- 但它的**结论**，在正确的页面上重测之后**成立**。第 422 轮把结论也一起推翻了，**过头了**。

用第 422 轮总结出来的可靠办法（滚动与点击分成两次调用、点前截图确认），
确认停在 Text fields 页之后，沿字段上下扫了六次右键（逻辑 y 从 208 到 235），
每一次的命中路径都是同样三个：

    target=40005  tap=true  long=false secondary=false
    target=302
    target=128516096

**`id=10000` 那条 330x19 的输入条，一次都没有进过路径。**

### 又往里逼了一层

把探针换成"每个 `RenderPointerRegion::hit_test` 被问到时打一行"，
在同一个确认过的页面上按右键，**整场只有两行**：

    ask id=302   size=460x764  pos=(200,165) contains=true
    ask id=40005 size=460x988  pos=(200,165) contains=true

也就是说：**字段那个区域的 `hit_test` 根本没有被调用过。**
它在树里（第 422 轮列出来了），但没人问它。

两个可以拿来说事的数字：

- 到达 40005 的位置是 **(200,165)**，而我按的是逻辑 (200,221)。
  差的 56 正是 `pages/demo.rs` 里那层 `SafeArea(Padding(top: 56))`——
  **祖先们的坐标换算是对的**，位置不是在这儿丢的。
- 40005 自己是 **460x988**，而视口只有 764。**这一页是滚动的。**

### 结论收窄到一句话

下降在 **40005 的孩子** 和 **字段区域** 之间停住。
40005 的盒子包含那个点（contains=true），它会调 `self.child.hit_test(..)`，
而这一路上再没有第二个 `RenderPointerRegion` 被问到。

**首选嫌疑是中间那个滚动容器**：988 的内容装在 764 的视口里，
如果那个视口/滚动对象没有实现 `hit_test_children`（trait 默认是**返回 false**），
它底下的一切就都问不到——而"画得出来、点不到"正是这个形状。
（`RenderBox::hit_test_children` 的默认实现在 `render.rs:1272`，`false`。）

需要留意的反例：首页那个列表也是滚动的，而且**点得动**。
所以要么它走的是另一个滚动对象，要么两者的实现不一样——**这正是下一轮要分辨的**。

尺子：十六把全部 exit 0。门：Rust 6488 通过；gallery 356 通过；
三个目录默认目标全部编过；探针已全部还原。

**下一步**：找出 40005 底下那个滚动容器**是哪个类型**，
看它有没有实现 `hit_test_children`。
最省事的探针：在 `RenderBox::hit_test` 的默认实现里，只在
`size().contains(position)` 为真、且 `hit_test_children` 返回 false、
且这个对象**有孩子**（`visit_children` 至少访问到一个）时打一行——
这正好把"够得着自己、却把孩子全挡在外面"的那一层单独挑出来。
挑出来之后与首页那个列表用的对象比对：
如果是同一个类型，那就去看它在两处的构造差别；如果是两个类型，
那就是其中一个没有把 `hit_test_children` 补上。

---

## 第 424 轮：挡路的那一层**点名了**——是演示表单那个 `RenderFlex` 列

在默认的 `RenderBox::hit_test` 里加了一个**只挑一种情形**的探针：
盒子包含这个点、**有孩子**、而 `hit_test_children` 仍然返回 false。
这正好把"够得着自己、却把孩子全挡在外面"的那一层单独捞出来。

在确认过的 Text fields 页上按右键，捞出来两层：

    blocked size=394x938 pos=(167,148) children=11 first=394x56   <- 最内层
    blocked size=428x972 pos=(184,165) children=1  first=394x938

**394x938、11 个孩子、第一个 394x56** —— 就是演示表单那个列：
`demos/material/mod.rs::column`，它建的是
`RenderFlex::column().with_main_axis_size(Min).with_cross_axis_alignment(Start)`。

所以：**`RenderFlex` 拿到了那个点，却对 11 个孩子一个都没命中。**

### 一个看着很像、但被实测否掉的猜想

`RenderFlex::hit_test_children` 是

    for (child, placement) in self.children.iter().zip(self.offsets.iter()).rev()

`zip` 在两边长度不等时**静默取短的那一边**——offsets 空了就一个孩子都测不到。
而 `update_from` 里 `self.children = mem::take(&mut fresh.children);`
**并不动 `offsets`**，注释写着"下一次 layout 会重建它"。
看上去是完美的嫌疑。

**实测否掉了。** 加了一条"两者长度不等就打印"的探针，
整场跑下来 **0 次**。长度是一致的。

所以问题不在**个数**，只可能在**偏移的值**、或者孩子自己的盒子上。

### 这一轮没做到的，以及又栽的那一跤

想打印那个 11 孩子列的偏移值，结果最后一次抓到的是**首页**的两个 flex
（n=25、行高 78，偏移 0/78/156/234 —— 完全正确）。
原因是我**这一次又省掉了"点完先截图确认"那一步**，
而这正是第 422 轮刚写进流程、第 423 轮照做才拿到正确数据的那一条。
同一个坑，这一轮第五次。

尺子：十六把全部 exit 0。门：Rust 6488 通过；gallery 356 通过；
三个目录默认目标全部编过；探针已全部还原。

**下一步**：只差一步，而且不许再省流程。
1. 滚动 → 截图确认 → 点击 → **截图确认停在 Text fields**；
2. 在 `RenderFlex::hit_test_children` 里，只对 `children.len() == 11` 的那个列
   打印**全部 11 个偏移**和**全部 11 个孩子的 size**，以及传进来的 position；
3. 对照着看：`pos=(167,148)` 落在哪一个孩子的 `[offset, offset+size)` 区间里，
   而那个孩子的 `hit_test` 为什么返回 false。

两种可能的收尾：偏移值是旧的（layout 没在 update 之后跑）——
那就去查那个 relayout 标记为什么没生效；
或者偏移是对的而孩子自己拒绝——那就再往里一层。

---

## 第 425 轮：**右键菜单是好的**——第 417 到 424 轮追的是一个由坐标造成的幻影

按流程把最后一步做完，结果是把前面七轮的方向整个推翻。

### 打印出来的那 11 个偏移，全是对的

在确认停在 Text fields 页之后按右键，那个 11 孩子的列打印出来：

    col11 size=394x938 pos=(167.0,148.3)
      [0] off=(0,0)   size=394x56
      [1] off=(0,80)  size=394x60
      [2] off=(0,164) size=394x60
      [3] off=(0,248) size=394x60
      ...

偏移和尺寸**完全正常**。而 `pos.dy = 148.3` 落在
**[1] 的下沿 140 和 [2] 的上沿 164 之间**——正是 `column(children, 24.0)` 的那 24 像素行距。

**我一直点在两行之间的空隙里。** `RenderFlex` 报"没命中"是**正确答案**。

### 换一个 y 再点，菜单就出来了

把物理 y 从 332 改成 275（换算过去列内 y 从 148.3 变成 110.3，落进 [1] 那一行），
再按一次右键——**菜单出来了**：字段获得焦点（紫色下划线、光标、两个选择手柄），
上面浮着一个 "Paste" 气泡。

### 所以真实情况是

1. 用户报的缺陷是真的，**根因是 Windows host 根本不转发右键**
   （`WM_RBUTTONDOWN/UP` 没有 case，`buttons` 写死 primary）。
   已在第 416 轮修好（`72df0b8`），并且**从那一刻起就已经好了**。
2. 第 417–424 轮所有"菜单还是不出来 / 命中测试走不下去 / 下降被挡住"的结论，
   **全部作废**。它们都源自同一个事实：我的测试坐标落在行距里。

七轮，一个坐标。教训不是"要小心"，是**测量前先证明测量点是有效的**——
第 418 轮我自己写下"一个打偏的坐标什么都证明不了"，然后连着七轮都在用一个打偏的坐标。
正确的做法本该是：先用**左键**点同一个坐标、确认字段获得焦点，
证明这个点确实在控件上，再拿它去测右键。这一步一次都没做。

### 仍然成立的那一件事：外观

上游桌面弹的是方角菜单、`Cut / Copy / Select all`；
本项目弹的是手机那套——药丸形气泡 + 两个拖拽手柄。
`editable.rs` 写死 `MaterialTextSelectionControls`，
上游桌面用的是 `DesktopTextSelectionControls`，
而 `text_selection_controls.rs` 里那些桌面类至今没有生产者。
**用户报告里的第三件事，是唯一还没修的。**

尺子：十六把全部 exit 0。门：Rust 6488 通过；gallery 356 通过；
三个目录默认目标全部编过；探针已全部还原。

**下一步**：做那件真的还缺的事——**桌面上用桌面那套选择控件**。
先查三点再动手：
1. `text_selection_controls.rs` 里 `DesktopTextSelectionControls` /
   `CupertinoDesktopTextSelectionToolbar` 现在是纯数据还是有 build；
2. 上游 `SelectionArea` / `EditableText` 是**在哪一层**按 `TargetPlatform`
   选 controls 的（是 `TextSelectionControls` 的静态选择，还是 theme 里）；
3. 桌面工具条与手机工具条**在按钮集合上的差别**
   （桌面有 Select all、没有拖拽手柄；手机相反），以及手柄该不该跟着平台关掉。
另外把"先用左键验证坐标有效"写进复现步骤，别再省。

---

## 第 426 轮：桌面上终于弹的是**桌面那套**选择控件

用户报告里的第三件事——"长得不一样"——这一轮修掉了。

### 先查的三件事

1. `text_selection_controls.rs` 里 `DesktopTextSelectionControls` **在**，
   而且已经是对的：`handle_size` 返回 `Size::ZERO`，注释写明"桌面有鼠标，
   选区直接拖，手柄是没人需要的触摸补偿、还多一个能被误点的东西"。
   `DesktopTextSelectionToolbar` / `Button` 也在，但**只有常量、没有 build**。
2. 上游在**哪一层**按平台选：`material/text_field.dart` 那个
   `switch (theme.platform)`（1607 行附近）——
   Linux/Windows 用 `desktopTextSelectionHandleControls`，
   Android/Fuchsia 用 `materialTextSelectionHandleControls`，
   两个 Apple 平台用 Cupertino 那两套。
3. 形状差在哪：上游 `_defaultToolbarBuilder` 是
   `SizedBox(width: 222) → Material(radius 7, elevation 1, card) → Column(min)`。

### 三处差别，都是**结构性**的

- **固定宽度 222**（`_kToolbarWidth`，上游注释说是照着 TextEdit 的截图量的）。
  固定而不是随内容——命令随选区增减时菜单不改变形状；Material 那条是缩到刚好包住按钮。
- **是列不是行**。桌面菜单把命令**往下**排。
- **圆角 7**，而 Material 那条的圆角是自身高度的一半——那才是它成为药丸的原因。

补上 `text_toolbars::desktop_selection_toolbar`，
并在 `editable.rs` 里按平台选控件与工具条（抽成 `selection_toolbar_for`，
因为**选择本身就是那个主张**，而 `toolbar_builder` 要一个活的 `StateHandle`、测试里造不出来）。

### 变异扫描 7 个，第一遍 5 红、2 绿，处理如下

- "菜单从来不被选中"活了：因为那个分支埋在 `toolbar_builder` 的闭包里，测不到。
  抽成 `selection_toolbar_for` 之后补了一条"Windows 得到 222 宽的菜单、
  Android 得到又宽又矮的条"的测试——**光断言谓词返回 true 是不够的**，
  一个无视谓词、永远造 bar 的 builder 照样能过。转红。
- "builder 无视传进来的平台"**仍然活着，如实记下**：
  那一行在闭包里，需要活的 `StateHandle` 才能调，和第 412 轮 `on_key` 那个守卫、
  第 416 轮帧里那次调用是同一族的结构性不可测。
- "桌面选区仍然画手柄"也活着，而这条**是被包含的**：
  桌面控件的 `handle_size` 本来就是 `Size::ZERO`，
  所以 `set_handles_visible(true)` 也画不出东西。写进代码注释了。

### 在真机上验了，而且这次先证明了坐标

按第 425 轮的教训，**先用左键点 (300,275)、确认字段获得焦点**（紫色标签、
紫色下划线、光标都出来了），证明这个点确实落在控件上；**再**按右键。
弹出来的是**方角卡片、命令竖排、没有拖拽手柄**——和用户截图里上游那个形状一致。
（命令是 Paste / Select all 而不是 Cut / Copy / Select all，因为字段是空的、
没有选区——`toolbar_commands` 本来就按选区决定按钮集合。）

尺子：十六把全部 exit 0。门：Rust 6494 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标全部编过。

**下一步**：用户报告的三件事全部结清了
（右键不出菜单 = host 不转发，第 416 轮修；"命中测试坏了" = 我的坐标问题，
第 425 轮澄清；外观 = 这一轮）。
回到 `shells.py` 的表继续挑下一个真缺口。
挑之前先跑 `python tools/depth.py` 看队头——这几轮一直在追一个报告，
队列已经很久没看过了。
另外把这一条写进复现步骤并**照做**：
**任何"点了没反应"的结论，先用左键在同一坐标验证控件确实在那里。**

---

## 第 427 轮：字段可以只读了——这是 `SelectableText` 缺的那块地基

回到队列。`depth.py` 队头仍然是几张数据表（`Icons` 8826 行、`CupertinoIcons`、
两份 localizations），头一个真控件是 **`SelectableText`（0.24，8/34）**。

查下去它是个**纯数据壳**：字段、两个谓词，**没有 build**。
而它自己的文档写得很清楚：
"A read-only `EditableText` …… 选择机制、手柄、工具条、放大镜都已经为编辑存在了，
可选而不可改的文本就是把'改'关掉的同一套机制。"

### 按"先查再动手"，查出的地基是缺的

`editable::TextField` **没有 `read_only`**。
`editable.rs:3710` 那里构造选词规则时是写死的 `read_only: false`，
注释还留着"本项目还没有只读字段"。

所以这一轮不是造 `SelectableText`，是补它要站的那块地基：**字段的只读**。

### 三处，一处是行为、两处是它带来的后果

1. `TextField::with_read_only(bool)`，并且传进选词规则
   （Android 上只读字段的**回捞上一个词**那条规则本来就在等这个值）。
2. **工具条的按钮集合**：只读拿掉 **cut 和 paste，但不拿掉 copy**。
   上游 `cutEnabled` / `pasteEnabled` 都以 `!widget.readOnly` 开头，
   而 `copyEnabled` 没有。对称地把 copy 也拿掉是"看起来更整齐"的那个选择，
   也是**错的**——读一段改不了的文字，正是最想复制它的时候。
3. `toolbar_extent` 也得按同一个集合量：只读只有两个按钮，
   拿可编辑的四个去量，画出来的条会比放置用的尺寸窄。

还有输入本身：`FieldClient::update_editing_value` 在只读时
**收下选区、退回文本**，并且**不触发 `on_changed`**——没有发生的改变不该报告。
这两半是相反方向的：只读字段**必须**可选（那是它的全部意义），但**不能**被改。

### 变异扫描 7 个，第一遍 5 红

两条活着的都是同一个结构性限制，**如实记进代码注释**：
测试用 `StateHandle::detached()` 驱动 client，而 detached 的 `set_state`
**按设计什么都不做**，所以"把选区写进 widget state"那一行、
以及"把标志传给 client"那一行（在聚焦闭包里）都看不见。
和第 412 轮 `on_key` 的守卫、第 416 轮帧里那次调用、第 426 轮 builder 里那一行是同一族。
`client.last` 是这条路上测得到的那一半，测试断言的就是它。

尺子：十六把全部 exit 0。门：Rust 6497 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标全部编过。

**下一步**：地基有了，下一轮把 `SelectableText` 造出来——
`stateful(TextField::new(id).with_read_only(true))`，
不带装饰、不带占位符，`data` 作为初始文本。
但**先查两件事**：
1. `TextField` 现在**怎么拿到初始文本**？看下来它的文本活在
   `TextFieldState.value` 里，而 `new(id)` 不收文本——
   所以多半还缺一个"以某段文字开局"的入口（上游是 `controller` 或 `initialValue`）。
   缺的话，那才是下一轮真正该补的，`SelectableText` 再往后排一轮。
2. `SelectableText` 的 `max_lines` 默认是 `Some(1)`，而它有个 `wraps()` 谓词——
   确认上游 `SelectableText` 的 `maxLines` 默认到底是不是 1
   （`TextField` 是 1，但只读文本块常常不是），别照抄错。

---

## 第 428 轮：字段终于能被交给一段文字——顺带改掉一条**测试写反了的规则**

上一轮留的两个"先查"，两个都查出了东西。

### 查一：`maxLines` 的默认，**本项目抄错了**

上游 `TextField` 写的是 `this.maxLines = 1`（显式默认一行）；
而 `SelectableText` 写的是 `this.maxLines`，**没有默认**——
它是 `int?`，到手是 null，build 里再退到 `defaultTextStyle.maxLines`（通常也是 null）。

本项目 `SelectableText::new` 却填了 `max_lines: Some(1)`。
更糟的是**有一条测试在断言这个错的规则**：
`one_line_is_the_default_and_none_means_as_many_as_it_takes` 里
`assert!(!single.wraps())`——把"默认一行"当成了事实。
和第 389 轮"三条测试的名字断言了错误的规则"是同一族。

改掉了：默认 `None`，补 `with_max_lines`，测试重写成
`a_selectable_text_wraps_by_default_where_a_field_does_not`，
并把两者的差别写进字段文档——**这个差别就是这个控件的用途**：
字段是拿来打字的一行，可选文本是拿来读的一段，
一段读到第一行就断的文字，几乎对所有用法都是错的形状。

### 查二：`TextField` **根本没法被交一段文字**

它没有 `initial_state`，所以永远从 `TextFieldState::default()`（空）开始，
而 `new(id)` 不收文本。也就是说
`SelectableText`——上游称之为"只读的 `EditableText`"——**没有东西可显示**。

补上 `with_initial_text`，并且**放在 `initial_state` 里**，
这一点是规则不是位置：字段的文本是它的**状态**，
所以重建时带着不同的初始文本**不会**覆盖读者已经打的字。
上游 `initialValue` 就是这条规则，而"每次 build 都读一遍"正是丢表单的经典写法。
光标停在末尾，跟上游 `TextEditingController` 的
`selection: TextSelection.collapsed(offset: text.length)` 一致。

变异扫描 6 个，**第一遍全红**——包括"每次 build 都读"那一条
（加一个 `did_update_widget` 去覆盖状态，测试立刻转红）。

尺子：十六把全部 exit 0。门：Rust 6501 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标全部编过。

**下一步**：地基齐了（只读 + 初始文本 + 正确的 `maxLines`），
下一轮把 `SelectableText` 真正造出来：
`stateful(TextField::new(id).with_read_only(true).with_initial_text(data))`，
`max_lines` 按自己的字段传下去，不带装饰、不带占位符。
但**先查一件事**：`TextField` 的 build 里那一圈**装饰**——
`RenderEditable` 外面是不是套着边框、下划线、内边距之类只属于输入框的东西。
上游 `SelectableText` 是直接建 `EditableText`、**不经过 `InputDecorator`** 的。
如果本项目的 `TextField` 把装饰焊死在里面，那这一轮该补的是
"能不能要一个没有装饰的字段"，而不是把一个带下划线的输入框
当成一段可选文本交出去——那看起来就不对。

---

## 第 429 轮：`SelectableText` 真的能上屏了

先查那件事：`TextField` 有没有把装饰焊死在里面。**没有。**
它的 build 从里到外是 `RenderEditable` → 指针区域 → `Focus` →
`TextFieldTapRegion` → 语义，**没有边框、没有下划线、没有内边距、没有容器**。
gallery 里那些下划线和标签全部来自演示自己的 `field_group`。

也就是说本项目的 `TextField` 本来就是上游称作 `EditableText` 的那个光板，
而上游 `SelectableText` 正是直接建 `EditableText`、**不经过 `InputDecorator`** 的。
所以可以直接造。

### 造出来了

`SelectableText::widget(id)` = `stateful(TextField::new(id)
.with_read_only(true).with_initial_text(data))`，行数按自己的字段映射过去。

一处如实记下的缺席：**`show_cursor` 没有被照做**。
上游默认 false，而本项目没有办法压掉一个获得焦点的字段的光标，
所以被点过的可选文本会显示光标。缺的那块在字段里，不在这里。

### 顺带补上自己上一轮留的尾巴

第 427 轮加了 `read_only`，却**没有把它接进语义**。
上游 `RenderEditable` 是 `..isReadOnly = readOnly`，
而 `SemanticsProperties.flags.is_read_only` 本项目早就有了、一直没人写。
补上：读屏用户遇到只读字段会被**告知**，而不是靠往里打字才发现。

### 变异扫描 6 个，第一遍 3 红，两轮补测才全红

- "行数映射成一行"两条活着：测试只看了**文字**有没有出来，
  没看行数模式。行数活在 widget 上、状态里读不到——
  抽成 `field_max_lines()` 并直接断言三种映射，转红。
- 抽完之后又有一条活着：**映射对了不等于 widget 照做了**。
  `Growing => field.multiline()` 那一臂被删掉照样过。
  加了一条从**布局高度**看的测试：同一段放不进 60 像素的文字，
  按"生长"排版比按"一行"排版**更高**——这是从外面唯一看得见这个差别的地方。
  第三遍全红。

这两步是同一个教训的两半：**先测到"映射"，再测到"映射被用上"**。

尺子：十六把全部 exit 0。门：Rust 6507 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标全部编过。

**下一步**：`SelectableText` 上屏了，但**没有任何调用者**——
和这些轮反复拆的"规则齐了、生产者缺席"是同一个形状，只是这次反过来：
生产者有了、消费者没有。
回 `depth.py` 队头挑下一个之前，**先查一件事**：
gallery 里有没有现成的地方本该用它（比如演示页的说明文字、about 对话框里的许可证文本），
有的话接一个真实调用者比再造一个新控件值钱。
没有的话就回队头，`TextSelectionOverlay`（0.24，6/25）和
`SearchAnchor`（0.24，8/33）是下两个真控件。
