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

---

## 第 430 轮：拖手柄终于记得**是从哪儿抓住它的**

先查上一轮留的问题：gallery 里有没有地方本该用 `SelectableText`。
**答案是没有，而且这不是缺口**——去上游框架里搜，
`SelectableText` 只在文档交叉引用里出现，**框架自己一次都不用它**。
它是给写 app 的人用的公开控件。硬在 gallery 里塞一个消费者，
是在造上游没有的用法。如实记下，回队头。

### 队头挑到 `TextSelectionOverlay`（0.24，6/25），查下去是"规则齐了、没人调"

`text_selection.rs::TextSelectionOverlay` 是个小结构：记下抓取点、
`handle_drag_position` 按它换算。**只有自己的测试在用它。**
真正在跑的是 `selection_host.rs`（1663 行）+ `editable.rs::drag_handle_to`。

而那条真路径是这么算的：

    local.dy + scroll.dy - layout.line_height / 2.0
    // 注释：Upstream reaches the same place through the handle's anchor.

**那条注释不对。** 上游 `_handleSelectionStartHandleDragStart`
把"手指落在手柄里的哪个位置"记下来、整个拖动过程都用它，
所以**从边缘抓住手柄不会让它跳到手指底下把中心对上去**。
本项目用的是"半行"这个常数——抓哪儿都当成抓中间。

再往下查，发现真路径**根本没机会知道**：`selection_host` 给手柄挂的只有
`with_pointer_move`，**没有 `with_pointer_down`**，按下那一刻的位置从来没被报出来过。

### 于是这一轮把这条线接通

1. `selection_host` 新增 `on_drag_start`，在手柄的指针按下时报出
   **局部**位置——那正是"抓在手柄里的哪儿"。
2. `editable.rs` 把它喂给 `TextSelectionOverlay::begin_handle_drag`，
   拖动时读 `grab_offset()`——**那条移植好却没人调的规则，现在有了调用者**。
3. 换算抽成 `handle_lift(grab, line_height)`：有抓取点就用它，
   没有就退回半行——**不是退回 0**，那会把选择点放到手柄的尖上。

### 变异扫描 7 个，第一遍 4 红，抽出 `handle_lift` 后 6 红

活着的最后一条是 `selection_host` 里那个按下处理器的接线：
手柄是在 overlay entry 里建的，要从测试碰到它得先把一个 overlay 立起来、
再从里面派发一次真实按下。**写进代码注释了**，
它喂的那条规则本身是单独测过的。

尺子：十六把全部 exit 0。门：Rust 6509 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标全部编过。

**下一步**：手柄拖动这条线还差一头——**没有人调 `end_handle_drag`**。
按下记住了、移动用上了，但手指抬起时那个抓取点**不会被清掉**，
所以下一次拖动会先沿用上一次的抓取点，直到第一次 `pointer_down` 覆盖它。
实际影响很小（按下总是先于移动），但它是**一条半接完的线**，
下一轮接完：`selection_host` 补一个 `on_drag_end`（`with_pointer_up`），
`editable.rs` 里调 `end_handle_drag`。
接完之后再回 `depth.py` 队头——`SearchAnchor`（0.24，8/33）是下一个真控件。

---

## 第 431 轮：把手柄拖动那条线接完——顺带把"测不到"这个借口拆掉一半

先查上游是不是真的在拖动结束时清状态。**是**：
`_handleStartHandleDragEnd` 里 `_isDraggingStartHandle = false;`、
`_startHandleDragInProgress = false;`，下一次拖动从自己的抓取点开始。

于是补上 `on_drag_end`（`with_pointer_up`），`editable.rs` 里调
`TextSelectionOverlay::end_handle_drag()`。手指抬起，抓取点就还回去。

### 更值得记的是这一轮把"结构性测不到"戳破了一半

第 430 轮我把"手柄按下的接线"记成了**测不到**：
"手柄是在 overlay entry 里建的，要碰到它得先立起一个 overlay"。
这一轮回头看，**那句话是错的**——`HandleEntry` 就是个普通的
`StatefulComponent`，可以**单独挂起来**，布局一下，再用
`GestureRouter` 派发一次真实的按下/移动/抬起。不需要 overlay。

于是补了 `handle_gesture_tests` 三条，直接按在手柄上：
按下报出**局部**抓取点、移动被报出、抬起被报出。
上一轮和这一轮各有一条"接线没测到"的变异，**现在都转红了**。

顺手把 `selection_host.rs` 里那条写错的注释改掉了——
它说这一段测不到，而它现在测得到，留着就是一条会误导下一个人的说明。

**教训**：把一处代码记成"结构性测不到"之前，先确认它**真的**够不着。
这些轮里我记过好几处，其中至少这一处只是**我没想到把它单独挂起来**。

剩下**一条**仍然没覆盖：`editable.rs` 里
`host.set_on_drag_start / set_on_drag_end` 那两次调用本身——
它们在字段 build 的闭包里，要碰到得挂一个带选择覆盖层的真字段。
这一条是真的重，如实记着。

尺子：十六把全部 exit 0。门：Rust 6512 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标全部编过。

**下一步**：手柄这条线接完了。回 `depth.py` 队头，
`SearchAnchor`（0.24，8/33）是下一个真控件。
但**先查一件事**：`search_anchor.rs` 里现在有什么——
是纯数据壳，还是已经有 `SearchBar` / `SearchAnchor` 的 build。
另外顺带用这一轮的办法回头看看：
以前记成"结构性测不到"的那几处（第 412 轮 `on_key` 守卫、
第 416 轮帧里那次调用、第 426 轮 builder 那一行），
有没有哪一处其实也只是"没想到怎么挂起来"。

---

## 第 432 轮：查了 `SearchAnchor`，**决定不动它**；改去还上一笔"测不到"的债

### 查的结果：`search_anchor.rs` 是又一个 765 行的纯策略文件

`SearchController`（attach / open_view / close_view）、
`SearchAnchor`（全屏解析、窗口改变尺寸时怎么收）、
`SearchBar::resolved`、`SearchDelegate`（suggestions / results 两页状态机）——
**全都在，一个 `build` 都没有**。外观也已经解析好了：
`ResolvedSearchBar` 带着背景色、elevation、shape、padding、hint 样式、约束。

看起来只差组装。**但差一样东西**：`ShapeBorder` → 容器圆角的映射
这个 crate 里**还没有**（`SearchBar` 默认是 `StadiumBorder`）。
没有它就只能对形状糊一个近似，而"糊一个近似"正是这些轮里反复拆掉的东西。

所以**这一轮不动它**：与其起一个半成品控件，不如把它留到能一次做穿。
`ShapeBorder` → 圆角这件事本身值一轮，而且不止 `SearchBar` 会用。

### 改去做的事：还第 426 轮那笔债

第 426 轮我把 `toolbar_builder` 里"按平台选工具条"那一行记成
**结构性测不到**——"它在闭包里，需要一个活的 `StateHandle`"。

**又错了。** `toolbar_builder` 是个自由函数，
而 `StateHandle::detached()` 就是个完全合法的实参。
直接调它、把返回的闭包建出来、量宽度：
Windows 得到 222 的固定宽菜单，Android 不是。
第 426 轮那条活下来的变异，现在转红。

连同第 431 轮的那两条，**"测不到"这个说法我已经错了三次**，
三次都是同一个形状：**没想到怎么把那一段单独立起来**。
所以现在的规矩是：写下"结构性测不到"之前，
先问一句"这段代码能不能被直接调用/单独挂起来"，
答不上来才算数。

（第 412 轮 `on_key` 那个守卫已经在当轮抽成 `is_escape_press` 测掉了；
第 416 轮"帧里有没有调 `apply_pending_autofocus`"是真的要 `RfApp`，
那一条暂时还立着。）

尺子：十六把全部 exit 0。门：Rust 6513 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标全部编过。

**下一步**：`ShapeBorder` → 容器圆角的映射。
先查两件事再动手：
1. `borders.rs` 里 `RoundedRectangleBorder` / `StadiumBorder` 各自拿什么表示圆角
   （`BorderRadius`？还是只有 side？），`StadiumBorder` 的"半高"要在哪一层算——
   它依赖尺寸，所以多半得在渲染对象里而不是在 widget 里。
2. 这个 crate 现在**怎么画一个带 shape 的表面**——
   `Container::with_border_radius` 只收一个 `BorderRadius`，
   那 `Material` 那一层（`controls.rs:4553` 的 `fn shape`）现在拿它做什么？
   如果已经有一条"shape 变成绘制"的路，那这一轮就是把它接出来给别人用，
   而不是新造一套。

---

## 第 433 轮：一个 `ShapeBorder` 终于说得出它把矩形圆成什么样

先查上一轮留的两件事。

**查一**：`controls.rs:4553` 那个 `fn shape()` 喂给谁？
`widgets.rs` 里那两处 `.shape()` **是另一个东西**——是 `Container` 自己的
"要套哪几层包装"的列表，和 `ShapeBorder` 毫无关系。**没有现成的路。**

**查二**：`impl ShapeBorder` 里有什么？**只有 `dimensions()`。**
十六个变体全是数据，唯一能问的问题是"边框往里收多少"。
上游那边 `ShapeBorder` 有 `getOuterPath` / `getInnerPath` / `paint` / `scale` / `lerp`，
这里一个都没有。**画不出来，也问不出圆角。**

### 这一轮补最窄、也最有人要的那一片

上游的答案是一条完整的 `Path`（所以星形能是星形）。
这个 crate 画表面用的是圆角矩形，**能落地的问题因此更窄**：
`corner_radius(size) -> Option<BorderRadius>`。

两处值得记的设计：

- **它收一个 `size`**，因为有一个答案依赖尺寸：
  `StadiumBorder` 上游是 `Radius.circular(rect.shortestSide / 2.0)`——
  **较短的那条边**，不是高。侧过来的胶囊还是胶囊，
  按较长边取半会让两个圆角越过彼此、连矩形都不是了。
  所以这不能是形状上的一个普通取值方法。
- **`None` 是有用的答案**，不是"还没做"。
  拿到 `None` 的调用者画直角矩形；拿到一个编出来的圆角的调用者
  会画错的弧线，而且**看上去是对的**，直到有人和上游对一遍。

### 自己抓到自己的一处不一致

第一版我让 `Superellipse` 返回它的 `border_radius`，
而文档里同一段又写着"`Continuous` 排除，因为它是 squircle"。
**超椭圆按定义就是 squircle。** 改成一起排除，
并在文档里点名它是最容易被放行的那一个——
它有一个类型完全正确的 `border_radius` 字段，读起来就像该在这儿。

变异扫描 6 个，**第一遍全红**，包括"把 squircle 当圆角放行"这一条。

尺子：十六把全部 exit 0。门：Rust 6516 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标全部编过。

**下一步**：`corner_radius` 现在**没有消费者**——
和这些轮反复拆的形状一样，只是这次是我刚造的。
下一轮把它接上：`SearchBar` 的 widget（第 432 轮就是因为缺它才没动），
`Container` 那边可以直接 `with_border_radius(shape.corner_radius(size)?)`。
但**先查一件事**：`Container::with_border_radius` 是在 build 时收半径的，
而 stadium 的半径**要到布局之后才知道**。
所以要么 `SearchBar` 用固定高度（`ResolvedSearchBar.constraints` 里有没有？）
把尺寸提前算出来，要么需要一个"在布局后取圆角"的渲染对象。
**先确认约束里给不给得出高度**，给得出就用它，给不出这一轮就该补那个渲染对象。

---

## 第 434 轮：一个盒子可以被交一个**形状**，圆角等量完自己再算

先查上一轮的问题：`ResolvedSearchBar.constraints` 给不给得出高度？
**给不出**——`min_height: 56`、`max_height: f32::INFINITY`。
所以 stadium 的半径**确实要等布局之后**才知道，
不能在 build 的时候折成一个 `BorderRadius` 交给 `Container`。

按上一轮自己写的判断：那这一轮就该补那个"量完再取圆角"的东西。

### 没有新造类型

`RenderDecoratedBox` 本来就带 `corners: Option<BorderRadius>` 和阴影。
所以只加一个 `shape: Option<ShapeBorder>`，并把两处读圆角的地方
（`corner_rrect` 和 `shadow_rrect`）收拢到一个 `rounding(rect)`：
**形状答得上就用形状的，答不上退回固定圆角。**

两处按事实定的细节：

- **`rounding` 收的是 `rect` 而不是自己的 size**。
  阴影是盒子按 spread 涨出来的**另一个矩形**，
  stadium 的半径必须跟着实际要画的那个矩形走，
  否则影子的形状和投影子的东西对不上。
- **是覆盖不是回退**：设了形状又设了圆角时，形状赢。
  上游 `Material(shape:)` 就是压过 `borderRadius` 的。

### 变异扫描 5 个，第一遍 2 红，补完测试全红

三条活着的分别是：优先级（测试里从来没有"两个都设"）、
阴影用哪个矩形、以及 `update_from` 忘记拷贝。三条都补了测试。

写这三条时又踩到两个"测试用错了对象"的坑，都记在测试注释里：

- 画出来的圆角矩形在存根里是一条 **Path**，而**存根只留 path 的边界、不留半径**
  （第 410 轮就写下过这件事）。所以圆角**从画布上看不见**，
  断言得走 `rounding` 本身——于是把它和 `shadow_rrect` 都公开了，
  理由和上游把 `getOuterPath` 公开是同一个。
- `update_from` 里要下转型，**任何一边包在 `RenderRef` 里都会让下转型失败**，
  于是"什么都没改"被当成通过。两边都用裸对象才测到真东西。

尺子：十六把全部 exit 0。门：Rust 6522 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标全部编过。

**下一步**：`with_shape` 现在还没有消费者。
下一轮把 `SearchBar` 的 widget 造出来——第 432 轮因为缺圆角没动、
第 433 轮补了 `corner_radius`、这一轮补了"量完再取"，路已经通了。
但**先查一件事**：`Container` 有没有办法把 `ShapeBorder` 传到底下那个
`RenderDecoratedBox`（它现在只有 `with_border_radius`）。
没有的话，`SearchBar` 要么直接建 `RenderDecoratedBox`、
要么先给 `Container` 补一个 `with_shape` ——
**先看清楚 `Container` 是怎么把配置交给它那几层包装的**，再决定哪条更小。

---

## 第 435 轮：`SearchBar` 从一条规则变成一个看得见、打得进字的东西

上一轮留的问题是"`Container` 能不能把 `ShapeBorder` 递下去"。查的时候
先撞见一件更该先说的事：**第 433 轮说"`impl ShapeBorder` 里只有
`dimensions()`"是错的**——`outer_path` / `inner_path` 一直都在，
`ShapeDecoration` 也在，`Decoration::Shape` 也接得上 `RenderDecoratedBox`。
`corner_radius` 仍然有它自己的用处（画表面走的是圆角矩形而不是 path），
但当时那句"画不出来"是查漏了。

真正的缺口不在容器那边：**`SearchBar` 是一个没有 `build` 的结构体**。
`ResolvedSearchBar` 十二个字段全都解析好了，没有任何东西把它们画出来。

### 按上游的层次一层层搭

`ConstrainedBox` → `Opacity` → `Material` → `IgnorePointer` → `InkWell`
→ `Padding` → `Row[leading?, Expanded(Padding(field)), trailing?]`。

几处按事实定下来的：

- **形状交给渲染对象，不在 build 里折成半径**。默认是 `StadiumBorder`，
  半径是较短边的一半，而 `maxHeight` 是无穷——量完之前没人知道。
  这是第 434 轮那个 `with_shape` 的第一个消费者。
- **阴影取主题的色、留自己的 alpha**。三层的 alpha 本来就不同
  （umbra 比 ambient 深），压成一个数，阴影就变成一圈灰晕。
- **禁用是"整块淡下去"而不是"每一处换个颜色"**。所以这个 bar 里
  从头到尾没有 `WidgetState::Disabled`：底色、阴影、提示、图标
  一起按 0.38 淡，分开解析会让它们各走各的。淡只是看上去的，
  真正挡住指针的是 `IgnorePointer`——两件事，两个测试。
- **padding 上了两次**，上游也是：一次围着整行，一次单围着输入框。
  少了里面那次，第一个字母就贴着 leading 图标。
- **`constraints` 是 `extra.enforce(incoming)`**，顺序是上游的顺序，
  所以外面给 200 宽时 bar 就是 200 宽，而不是自己的最小值 360。
  第一版测试断言反了，是测试错不是实现错。

### 顺手补的一处：`TextField` 的 `hintStyle`

`TextField` 原来把 placeholder 一律画成"自己的 style 调暗"。
search bar 的提示是 `onSurfaceVariant`、正文是 `onSurface`——
"调暗"只能调到附近，调不到那个颜色上。加了 `with_hint_style`，
没给的时候仍然走原来的调暗。

### 变异扫描 9 个，全红

一个第一遍是 BUILD ERROR（写了个不存在的 `BoxConstraints::UNBOUNDED`）——
**BUILD ERROR 不算通过**，换成真的无约束重跑，2 红。

尺子：十六把全部 exit 0。门：Rust 6530 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标全部编过。

**下一步**：`SearchAnchor` 现在还是纯规则——它有 `resolve_full_screen`、
`on_window_resized`、`resolved_view`，但没有任何东西把 bar 和 view 连起来：
点 bar 不会开 view。下一轮做这条线。
**先查两件事**：(1) `SearchController` 是纯数据，`open_view` 只改自己的
布尔值，谁来把它变成一次真正的 route push？看 `navigator.rs` 里
现成的 push 是什么签名。(2) 全屏与停靠两种 view 的**位置**由谁算——
`ResolvedSearchView` 里有没有，没有就得先补，不然只能糊一个位置。

---

## 第 436 轮：打开的 search view 落在哪儿

上一轮留了两件事要先查。

**查一**：`navigator.rs` 的 `push(route: u64)` 只收一个 id，
没有"把一棵子树推成一条 route"的东西。
**查二**：`ResolvedSearchView` 十三个字段里**没有位置**——
没有 rect、没有 anchor。上一轮自己写的：
"没有就得先补，不然只能糊一个位置"。所以这一轮补它。

上游是 `_SearchViewRoute.updateTweens` 里的 `_rectTween.end`。
`begin` 是 anchor 自己的 rect——view 是从被点的那个 bar **长出来**的，
不是盖在它上面，眼睛跟着一个东西走而不是丢了 bar 又找到一块面板。

### 两个尺寸来自两个地方

- **宽是 anchor 的**（clamp 到约束）：view 是那个输入框的延续。
- **高是屏幕的三分之二**（clamp 到约束），和 anchor 无关：
  一个 56 高的 bar 说不出一列结果要多少地方，能随窗口缩放的答案是分数。

### 越界时动的是角，不是尺寸

上游那段注释写着 *"If the window is smaller than the view, then we resize
the view to fit the window"*——**它的代码不 resize**。
`min` 落在角的位置上，`endSize` 原样是 `Size(viewWidth, viewHeight)`。
**照代码抄，不照注释抄**：错的是注释，一个悄悄"修好"它的移植
会在没人找得到原因的地方把窗口排得和上游不一样。测试把这条钉住了。

### RTL 那个 `if` 是死的

上游 RTL 分支里写了左右镜像的那次回拉：
`if (viewRightToScreenLeft < viewWidth) topLeft = Offset(0.0, ...)`。
条件就是 `anchorRect.right < viewWidth`，而上一行的
`max(right - width, 0)` 在这种情况下**已经是 0 了**。
**没有输入能让这个分支改变答案。**

所以没有照抄，并在代码里写明为什么：
**一个到不了的分支是一个测试按不住的分支**——变异扫描打它会一直绿，
而那个绿看起来像"覆盖不够"，其实是"这里没有东西"。
（这也是变异扫描第一遍抓出来的：那条变异 0 红，
查下去才发现不是测试少写，是被测的东西不存在。）

### 变异扫描 11 个，第一遍一条 0 红是真缺口

"回拉不再限定 LTR"那条第一遍 0 红——这一条是**真的没测到**：
我的 RTL 用例里 bar 都不靠右，两种算法碰巧同一个答案。
补了"RTL 下靠右的 bar 不该被回拉"（540 而不是 640），11 个全红。

尺子：十六把全部 exit 0。门：Rust 6540 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标全部编过。

**下一步**：位置有了，**动画还没有**——`begin` 是 anchor rect、
`end` 是这一轮算出来的，中间要一个 `RectTween` 和一条曲线。
下一轮做这个。**先查两件事**：(1) 这个 crate 里有没有 `Rect` 的
lerp / `RectTween`（`animation.rs` 或 `tween.rs` 里找），没有就先补它，
因为"两个矩形之间的插值"是别处也要用的东西，不该埋在 search 里。
(2) 上游 `_SearchViewRoute` 用的是哪条曲线、多长时间——
看 `buildPage` 里那个 `CurvedAnimation` 的 `curve` 和 route 的
`transitionDuration`，别猜。

---

## 第 437 轮：view 从 bar 长出来的那 600 毫秒

上一轮留的两件事都查了，两件都是"已经有了"：
`RectTween` 在 `animation.rs:2857`，`Interval` 在 `curves2d.rs:41`，
`EASE_IN_OUT_CUBIC_EMPHASIZED` 在 `animation.rs:133`，
`curve_for_direction` 也在。**所以这一轮不用先造零件**，
直接把上游 `_SearchViewRoute.buildPage` 和它上面那五个常量搬过来。

### 一个动画，六条曲线挂在上面

route 只跑**一个** 600ms 的动画，view 里每样东西都是这同一个 parent 上的
另一条曲线。这件事是整段动效的关键：它看上去是"一个动作里陆续有东西到位"，
而不是"六个动画碰巧同时开始"。

- **矩形**走 emphasized，到一半时已经走完 95%——
  所以后半程里，往里淡入的东西是淡进一块**已经不动了的**面板。
- **view 自己**在前一半淡入，**分隔线**第一个六分之一，
  **图标**第二个六分之一，**列表**是 133ms 到 233ms。

### 关键的一处：这四条 interval 挂在**原始**动画上，不是缓动后的值上

上游写的是 `CurvedAnimation(parent: animation, curve: <interval>)`，
parent 是 `animation` 而**不是** `curvedAnimation`。
喂缓动后的值进去，四条淡入会全被压进前五分之一
（emphasized 的距离都花在那儿），错落就塌成一瞬间。
这条专门写了一个测试盯着。

### 列表那条是用毫秒写的

其余三条是六分之一、三分之一、二分之一，只有列表写成
`133 / _kOpenViewMilliseconds`。所以它是**唯一一条会随时长改变含义**的。
变异扫描里"把它也写成六分之一"第一遍 0 红——
我的用例都取在两种写法答案相同的点上（1/6 处都是 0，0.5 处都是 1）。
补了一个按毫秒取点的测试：200ms 时真值是三分之二，六分之一写法会说"已经结束"。

关的时候用 `flipped` 而不是倒放：一条慢慢起步的曲线倒着放也该慢慢到站，
照原曲线重放会让关闭"猛地弹开再飘回来"。

### 变异扫描 12 个，补完那一条后全红

尺子：十六把全部 exit 0。门：Rust 6549 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：位置有了、动画有了，**还是没有一条真的 route**。
`SearchController::open_view` 到今天为止只改自己那个布尔值。
下一轮把这条接上。**先查两件事**：
(1) `theatre.rs` 的 `show_modal` 收什么、返回什么——
上游 `_SearchViewRoute` 是一条 `PopupRoute`，
`barrierDismissible = true` 而 `barrierColor = transparent`，
看这个 crate 的 modal 能不能表达"能点外面关掉、但不画遮罩"。
(2) 关掉时上游走 `didPop` 并且**重新算一次 tween**
（`updateTweens(anchorKey.currentContext!)`）——
因为 bar 可能已经不在原处了。看这个 crate 的 modal 关闭有没有
一个能挂这件事的地方，没有就得先想清楚放哪儿。

---

## 第 438 轮：search view 终于是一条真的 route 了

上一轮留的两件事：

**查一**：`ModalBarrier` 的 `color: None` 就是"挡得住但不画"，
`dismissible` 默认 true——上游那对
`barrierColor: transparent` + `barrierDismissible: true` **表达得出来**。
**查二**：`ModalHandle` 上**没有**能挂"关闭时重算一次 tween"的地方。

所以这一轮做打开那半边，关闭那半边如实记在文档里（见下）。

### `show_search_view`

barrier 那三样单独提成 `search_view_barrier()`：
不画、可点掉、名字是 "Dismiss"。三样合起来才是"search view 的遮罩
和 dialog 的遮罩不一样"这件事，放在它们被写下来的地方比放在被用的地方好查。

面板本身是一个 `StatefulComponent`，按这个 crate 里 `show_*` 的老规矩：
`advance` 推时钟、`build` 用 `SearchViewTransition` 算这一帧。

摆放照 `_ViewContentState.build`：
**最大值是动画中的那个矩形，最小值是解析出来的约束再 `min` 到矩形上**。
那个 `min` 是两个都要传进来的全部理由——
开场大半程里矩形比 view 自己的最小值（360×240）还小，
不 clamp 的话最小值会大过旁边的最大值，
面板第一帧就是全尺寸，"长出来"根本看不见。

### 明写下来的一处未做

上游关闭时是把同一个动画倒着放，而且**先重算一次 tween**
（bar 可能已经移位了）。这里是点掉就立刻收——
这个 crate 里今天每个 `show_*` 都是这样（`show_cupertino_modal_popup`
的 sheet 也只有进场动画）。记下来而不是藏着：
倒放需要 `ModalHandle` 上有个地方挂，而那个地方不存在。

### 变异扫描 14 个，第一遍 5 条没红，五条各不相同

这一批很值得记，因为**五条里只有一条是"测试写少了"**：

1. **"面板永远按 anchor 摆"**——我的用例里 view 的左上角**恰好**就是
   anchor 的左上角（bar 不靠边，不会被回拉）。补了一个靠底边的用例。
2. **"最小高度不 clamp"**、3. **"最小宽度不 clamp"**——
   探针错了。裸的 `RenderDecoratedBox` 没有子节点时取 `constraints.biggest()`，
   **根本不读最小值**，所以"最小值被违反"在画布上看不见。
   换了一个取 `smallest()` 的探针才测得到。
   宽度那条还多一层：bar 400 宽 > 最小 360，**clamp 与不 clamp 同一个答案**，
   得用一个 200 宽的 bar 才咬得住。
4. **"每帧不 clamp 时钟"**——我的用例每步 16ms，从来没超过 50ms 上限。
   补了一个"一帧就迟到 600ms"的用例。
5. **"遮罩点不掉"**——测试里我一直是直接 `dismiss()`，从没碰过遮罩。
   提出 `search_view_barrier()` 之后直接对它断言。

补完 14 条全红。

尺子：十六把全部 exit 0。门：Rust 6557 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：route 有了，**里面还是空的**——`show_search_view` 收一个
`content` 闭包，谁也没给它内容。上游的 `_ViewContent` 是
header（一个 search bar）+ divider + 建议列表。
下一轮做 header 那一层。**先查两件事**：
(1) 全屏 view 的 header 高度是 `fullScreenBarHeight: 72`，
停靠的是 `ResolvedSearchView::header_height`（默认 `None`，即"多高算多高"）——
确认这两个数在 `ResolvedSearchView` 里都取得到，别把 72 硬写进 widget。
(2) header 里那个输入框和第 435 轮做的 `SearchBar` 是不是同一个东西——
看上游 `_ViewContent` 用的是 `SearchBar` 还是直接一个 `TextField`，
**决定这一层是复用还是新写**。

---

## 第 439 轮：view 的 header 就是第 435 轮那个 `SearchBar`

上一轮留的两件事：

**查二**（先说这条，因为它决定了整轮）：上游 `_ViewContent` 里那个
**就是 `SearchBar`**，不是另写一个 `TextField`。
所以这一层是**复用**。这也正是前两轮那个动画的意义所在——
bar 长成面板，面板顶上那个输入框就是刚才被点的那个，
光标从头到尾没换过 widget。另写一个"长得像的"能跑，
但错法只在两边漂移的那天才显出来。

**查一**：`fullScreenBarHeight = 72` 和 `headerHeight` 两个数都在
`ResolvedSearchView` 里，不用往 widget 里硬写。

### `SearchBarOverrides`

header 要把 bar 的底色、覆盖色、高度、两种字体、约束、内边距全改掉。
上游每一项都是 `WidgetStateProperty`，这里是**普通值**——
而这算忠实不算简化，理由在于**是谁在设**：
header 每一项传的都是 `WidgetStatePropertyAll`，
一个不看状态的 property。header 里的 bar 按下、悬停、静止都是透明的，
因为要被看见的是它背后的面板。

放在 `resolved()` 里统一叠加，这样一次 build 里的三次解析
（静止/悬停/按下）不可能对它们各说各话。

### header 的约束有三个答案

- **写了高度**：`tightFor(height:)`，**是钉死不是下限**——
  要 64 就是 64。
- **没写且全屏**：下限 72、上不封顶（要让开状态栏，可能需要更多）。
- **没写且停靠**：什么都不说，落回 bar 自己的 56。

### 变异扫描三批，第一批 13 条里 8 条没红，逐条查下来只有 2 条是真缺口

第一批有 4 条是 **BUILD ERROR**——我把结构体字面量里的字段整行删了，
那不编译。**BUILD ERROR 不算通过**，改成置 `None` 重跑。

剩下的：

- **"header 沿用 bar 的内边距" 0 红**：不是漏测，是**坏变异**——
  view 的 `barPadding` 和 bar 的 `padding` 默认**是同一个数**
  （`symmetric(horizontal: 8)`，`ResolvedSearchView` 早就记着这是有意的）。
  改成先把 view 的值改掉再断言，同时把两种字体也一起改掉断言，
  否则"接线掉了一根"在默认值下看不见。
- **"header 沿用 bar 的悬停覆盖色" 0 红**：**真缺口**。
  我在 `WidgetStates::NONE` 上读，而 bar 自己静止时**本来**就是透明的。
  改成按悬停读，先断言页面上的 bar 会亮起来，再断言 header 不会。
- **"全屏下限压过写死的高度" 0 红**：又是**坏变异**——
  `match` 的第一条臂是 `(Some(height), _)`，第二条写成 `(_, true)`
  也永远轮不到。改成把全屏那条整个挪到前面，才真的换了顺序，1 红。

三批加起来 13 条全红。

尺子：十六把全部 exit 0。门：Rust 6566 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：header 有了，`show_search_view` 还是没人给它内容。
下一轮把 `_ViewContent` 那一列拼起来：
header + divider + 建议列表，外面套第 437 轮那三条 interval 的淡入。
**先查两件事**：
(1) 那三条淡入现在**没有消费者**——`icons_fade`/`divider_fade`/`list_fade`
   谁都没读。确认列要怎么把每一段包进各自的透明度里
   （`RenderOpacity` 是渲染对象，列是 `RenderFlex`，看现成的组合方式）。
(2) 上游那个 `if (!effectiveShrinkWrap || minHeight > 0 || showFullScreenView
   || result.isNotEmpty)` 决定 divider 和列表**在不在**——
   四个条件的或，不是"有结果才显示"那么简单。抄之前先把四个条件各是什么读清楚。

---

## 第 440 轮：面板里那一列

上一轮留的两件事。

**查一**：三条淡入怎么接——`RenderOpacity` 是渲染对象，
列是 `RenderFlex`，直接把每一段包起来就行；
`FlexChild::expanded`（tight）和 `FlexChild::flexible`（loose）都现成。

**查二**：那个四条件的或，读清楚之后是这一轮最值得写下来的东西。

### divider 和列表是一起有、一起没有的

```dart
if (!effectiveShrinkWrap || minHeight > 0 || showFullScreenView || result.isNotEmpty)
```

**四个条件全都不成立时才收起来**。这不是"有结果才显示列表"——
四个里有三个跟有没有结果毫无关系。

理由在于分隔线是什么意思：**它说"下面还有"**。
而这三种情况下面确实还有——一个有最小高度的、或者占满屏幕的 view，
不管有没有人打字，header 底下都有地方。画一条指着空白的线更糟。
只有第四种（会收缩、停靠、无下限、无结果）才真的只是一个输入框。

其中 `minHeight` 是**已经 clamp 过的**那个
（`min(effectiveConstraints.minHeight, _viewRect.height)`），
条件读的就是它，所以结构体里存的也是它。

### 名字叫 icons 的那条淡入，罩的是整列

上游 `FadeTransition(opacity: viewIconsFadeCurve, child: Column(...))`——
`_kViewIconsFadeOnInterval` 根本不是图标的淡入，是**所有东西的**，
名字大概是从它以前罩着的东西留下来的。
按**它作用在哪儿**抄，不按它叫什么抄，因为名字才是错的那一半。

结果是 divider 和列表各自被**两条曲线相乘**：
它们在一列**自己还在淡入**的东西里面，按自己的节奏到位。

### 变异扫描 17 个，第一遍 2 条没红

- **"整列不 stretch" 0 红**：探针错了。我给 divider 的探针是 400 宽，
  和列一样宽，**stretch 和 center 画出来一模一样**。
  换成 100 宽的探针——分隔线是一条横贯面板的线，
  而让它横贯的正是 stretch；宽度一样的探针分不出"拉满"和"居中"。
- 另一条是 fmt 之后搜索串失效，改对即红。

还有一处是写测试时先撞上的：我本来想找一个三条淡入**同时都在半路**的时刻，
**没有这样的时刻**——divider 那六分之一在整列自己的淡入开始之前就结束了。
这本身就是错落的定义，于是改成断言 0.3 处 divider 已经到位
（**到位的淡入不推图层**），而整列和列表各自在半路、且数值不同。

17 条全红。

尺子：十六把全部 exit 0。门：Rust 6574 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：列有了、header 有了、route 有了，**还没接到一起**——
`show_search_view` 的 `content` 参数至今是调用者随便给的。
下一轮把 `_ViewContent` 收口：一个函数，收 `ResolvedSearchView`、
`SearchViewBody` 和建议列表，吐出 route 要的那个 content 闭包，
包括外面那层 `Material`（形状、底色、elevation、`clipBehavior: antiAlias`）。
**先查两件事**：
(1) `clipBehavior: Clip.antiAlias` 配 `shape` ——
   这个 crate 里有没有"按 `ShapeBorder` 裁剪"的渲染对象？
   `RenderClipRRect` 收的是 `BorderRadius`，而第 433 轮的
   `corner_radius` 正好能把 view 的圆角形状转成半径（默认是
   `RoundedRectangleBorder`，答得上）。确认这条路通不通，通就用它。
(2) 上游那个 `OverflowBox(alignment: topLeft, maxWidth:
   math.min(viewMaxWidth, screenSize.width), minWidth: 0, fit: deferToChild)`
   在做什么——它让内容按**最终**宽度布局而不是按动画中的宽度，
   否则文字会在开场时重排。确认这个 crate 有没有 `RenderOverflowBox`，
   没有的话这一轮就得先补它，别糊过去。

---

## 第 441 轮：面板本身——Padding / Material / OverflowBox

上一轮留的两件事都是"有"：`RenderOverflowBox` 带 `OverflowBoxFit`
（在 `render.rs:8207`，`interactive_viewer.rs` 已经在用），
`RenderClipRRect` 也在。只差一件：`RenderClipRRect` 收的是**固定半径**，
而 `Material(clipBehavior:, shape:)` 裁的是形状的轮廓。
按第 434 轮给 `RenderDecoratedBox` 加 `shape` 的同一个办法，
给它也加了一个 `shape` + `rounding()`：**量完自己再取圆角**。

### 内容按 view **最终**的宽度布局

这是整个 view 里最不显眼的一处，也是 `OverflowBox` 存在的全部理由。
它的 `maxWidth` 是 `min(viewMaxWidth, screenSize.width)`，
而 `viewMaxWidth` 是 `_rectTween.end!.width`——**结束时的宽度，不是动画中的**。

没有它，列会按正在长大的矩形去量，里面每一行字**每帧都要重新折行**：
看上去不是面板在长大，是内容在乱跳。有了它，内容按最终宽度量一次，
然后随着矩形追上来被**逐渐露出**——所以这个盒子必须允许子节点溢出，
而下面那层裁剪就是"不让溢出被看见"的那一半。

对屏幕取 `min` 是给 `view_rect` 特意放过的那种情况兜底：
比窗口还宽的 view 会保留自己的宽度（第 436 轮记过），
这一行不让**内容**也跟着跑到屏幕外面去。

### 裁剪在表面**里面**，不在外面

外面裁会把面板自己的阴影齐着边切掉，而一个到了投影者边缘就没了的阴影不是阴影。

### 变异扫描 15 个，第一遍 4 条没红，三条是探针不对

- **"内容不许比面板窄" 0 红**：我的内容探针本来就要多少拿多少，
  最小值咬不到它。补一个固定 50 宽的探针。
- **"溢出盒按父节点定尺寸" 0 红**：我一直用 **tight** 约束布局面板，
  tight 之下 `Max` 和 `DeferToChild` 是同一个答案。
  而真实情形里 view 到位之后**竖直方向是松的**
  （min 240、max 600），差别就在那儿。补一个松高度的用例。
- **"裁剪忽略形状" 0 红**：存根只记 clip path 的**边界**、不记圆角，
  所以"它裁了"和"它裁成 28"在画布上一模一样（第 410 轮就写过这条）。
  改成照 `theatre.rs` 里 `find_theatre` 的办法在树里找到那个
  `RenderClipRRect` 再问它 `rounding()`。
- 第四条是 fmt 之后搜索串失效。

补完 15 条全红。

尺子：十六把全部 exit 0。门：Rust 6584 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：`_ViewContent` 的三层现在**各自都在，但没有一根线把它们串起来**——
`search_view_panel` / `search_view_column` / `search_view_header` 谁也不调用谁，
`show_search_view` 的 `content` 参数还是调用者随便给的。
下一轮做收口：一个 `search_view_content(...)`，
从 `ResolvedSearchView` 一路搭到建议列表，然后让 `show_search_view` 默认用它。
**先查一件事**：`SearchViewBody.min_height` 要的是**已 clamp 的**那个
（`min(constraints.min_height, rect.height)`），而 rect 是**每帧都在变**的——
所以 body 不能在 route 建立时算一次，得在 `SearchViewOpening::build` 里
每帧重算。确认 `SearchViewOpening` 现在拿不拿得到 `ResolvedSearchView`
（它现在只收 `BoxConstraints`），拿不到就要先把它传进去。

---

## 第 442 轮：把 `_ViewContent` 那四层接成一条线

上一轮留的问题：`SearchViewOpening` 只收 `BoxConstraints`，
**拿不到 `ResolvedSearchView`**。所以这一轮先把它传进去。

### 内容闭包要知道自己在第几帧

`show_search_view` 原来收的是 `Fn() -> AnyWidget`，改成 `Fn(f32)`。
理由是**面板里面的东西也在动**：divider 和列表各有自己的淡入，
而"它们在不在"这件事读的是**动画中的**矩形。
一个只建一次的内容只能被揭开，不能被打开。

### body 的判断每帧都要重算

上游的 `minHeight` 是 `min(effectiveConstraints.minHeight, _viewRect.height)`，
而 `_viewRect` 是**动画中的**那个。所以"有没有分隔线"不是 view 的属性，
**是这一帧的属性**。t=0 时它是 bar 的 56（从 240 clamp 下来），
t=1 时才是 240。

### 变异扫描 12 个，第一遍 4 条没红，三条真缺口一条坏变异

- **"body 不看有没有结果"**、**"body 不看是不是全屏"** 0 红：
  真缺口。我的用例里从来没有"只靠这一条打开面板"的情形——
  四条件的或，得让另外三条都关着才测得到某一条。各补了一个。
- **"面板按动画中的宽度布局"** 0 红：探针不对。
  `content()` 里 bar 是 400 宽，而 `view_rect` 保留 bar 的宽度，
  所以 view 也是 400——**两个值相等**。换成 200 宽的 bar
  （开出 360 宽的 view）才咬得住。
- **"header 和 list 换位"** 0 红：**坏变异**。
  我换的只是 `rendered.next().expect("...")` 里的**字符串**，
  而 `next()` 按顺序取，跟标签无关，所以那根本没换任何东西。
  改成先收进具名变量再按不同顺序传，2 条都红——
  同时也补了一个"三样按给的顺序进列"的测试
  （原来只断言三样都在，而三样都在跟顺序无关）。

11 条有效变异全红。

尺子：十六把全部 exit 0。门：Rust 6593 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：`open_search_view` 现在收三个闭包（header / divider / list），
**divider 那个还是调用者给的**——而它本该由 view 自己按
`ResolvedSearchView::divider_color` 画出来（上游是
`DividerTheme(data: dividerTheme.copyWith(color: effectiveDividerColor),
child: const Divider(height: 1))`）。
下一轮把它收回来，并把 header 也默认成第 439 轮的 `search_view_header`，
让 `open_search_view` 只剩下"建议列表"一个闭包。
**先查一件事**：`Divider` 是一个 `Component`，它的颜色走
`DividerOverrides`——确认 `Divider::new().with_color(...).with_height(1.0)`
建出来的东西能直接当 `AnyWidget` 用（看 `components.rs` 里
`Divider` 是怎么被别处放进树里的），能就直接用，
不能就得先看清楚它要怎么包。

---

## 第 443 轮：header 和分隔线归 view 自己管

上一轮留的问题：`Divider` 是个 `Component`，
`component(Divider::new().with_color(..).with_height(1.0))` 直接就是 `AnyWidget`。
路是通的。

### `open_search_view` 现在只收一个闭包

只有建议列表还是调用者的。header 和分隔线上游都在 `_ViewContent` 里建，
而一个能自己传 header 进来的调用者，可以给面板配一个**不是它长出来的那个** bar。

### 一像素的线

上游是 `Divider(height: 1)`，不是主题默认的十六。
默认那条要在上下留出空气——两个列表项之间的分隔线需要；
而 header 底下这条是**同一块表面上的一道缝**，留空气就成了面板上的一道豁口。

颜色是把 view 的 `dividerColor` 盖在 divider 主题上，不是从主题里取：
搜索视图的线跟着**view 的**主题走，
一个在别处改过分隔线样式的 app 不该顺手把这条也改了。

### 明写下来的两处未做

上游 header 还有 `defaultLeading`（一个 pop 路由的返回按钮）
和 `defaultTrailing`（有字时才出现的清除按钮）。两个都没建：
返回按钮要有路由可 pop，而这个 crate 的 view 是从遮罩关掉的，
接一个通向空处的按钮比留着空位更糟。记下来，不是悄悄漏掉。

### 变异扫描 9 个，第一遍全红

尺子：十六把全部 exit 0。门：Rust 6597 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：整条链现在通了，但**没有人调用它**——
`open_search_view` 在 crate 里是个孤儿，`SearchAnchor` 自己也不会开 view。
下一轮把 `SearchAnchor` 做成 widget：它建 bar（第 435 轮），
点 bar 时算出 `view_rect`（第 436 轮）并 `open_search_view`。
**先查两件事**：
(1) `SearchAnchor` 要拿到 bar 在**屏幕上**的矩形才能算 anchor rect
   （上游是 `searchBarBox.localToGlobal(Offset.zero, ancestor: navigator...)` 加 `size`）。
   确认这个 crate 里一个 widget 怎么问"我现在在屏幕上的哪儿"——
   `raw_menu_anchor.rs` 里那个 `AnchorRect` 是怎么拿到的，照它办。
(2) `SearchViewContent.screen` 和 `media_top` 要从 MediaQuery 取——
   确认 `MediaQuery` 在这个 crate 里叫什么、怎么在 build 里读到尺寸和上边距。

---

## 第 444 轮：点一下 bar，view 真的开了

上一轮留的两件事都有：
`theatre.rs` 的 `Anchor`（`set` 在 assemble 里记下渲染对象，`rect()` 给全局矩形，
`popup.rs` 就是这么用的），`media_query_of(context)` 给 `size` 和 `padding.top`。
只差一样：主题的 `TargetPlatform` 和滚动层的 `ScrollPlatform` 是**同样六个名字的两个枚举**，
之间没有转换。补了一个 `From`，并写清楚为什么是两个而不是一个。

`SearchAnchor` 现在是个 widget：建 bar → bar 的 assemble 把自己记到 `Anchor` 上
→ 点击时取矩形、算 `view_rect`、`open_search_view`。
主题在**这里**捕获（`capture_themes`），因为 view 建在 overlay 里，
而 overlay 不在这些主题下面——上游在同一个地方做同一件事。

### 一处"本来想写的守卫，其实到不了"

第一版在点击处理里加了"已经开着就别再开"。测试一跑，第二次点击的结果是
**0 个 modal 而不是 1 个**——view 的遮罩盖在 bar 上，第二次点击**根本到不了 bar**，
它落在遮罩上把 view 关掉了。

所以那个守卫是一条**没有输入能到达的分支**（第 436 轮记过这种形状：
变异扫描按不住它，而那个绿看起来像覆盖不够，其实是那儿没东西）。
删掉，改成测真实行为："点回 bar 的位置会关掉盖在它上面的 view"。

顺带把 `state.open` 也删了：守卫没了之后它是**写了没人读**的状态。
上游 `SearchController.closeView` 确实能从外面关，但那需要一个调用者持有的
controller，而这个 crate 的 `SearchController` 还没接上——记下来，不留死状态。

### 测试里踩到的坑：没有 MediaQuery 的屏幕是 0×0

变异扫描里"view 永远全屏 / 永远不全屏"两条都不红，查下去发现根子在
**我的测试树里没有 `MediaQuery`**。于是 `media.size` 是默认的零，
`view_rect` 把宽高一路 clamp 到最小值，面板里的东西按
`min(view_width, 0)` 布局——一个 360×240 的面板，里面什么都是 0 宽。
测试"通过"了，因为它们只断言"有东西画出来"。

加上 MediaQuery 之后，再补两个测试：**跑完整个 600ms** 再看落点。
早期几帧里面板还是 bar 的矩形，**无论它要去哪儿**——
只看第一帧的测试分不出停靠和全屏。

九条有效变异全红。

尺子：十六把全部 exit 0。门：Rust 6604 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：`SearchController` 现在是个**没接线的纯数据**——
`open_view` / `close_view` 只改自己的布尔值，而真正开关 view 的是
`SearchAnchor` 的点击和遮罩。上游那条线是：controller 持有 route，
`closeView` 先写文字再 pop。下一轮把 controller 接上：
让 `SearchAnchor` 收一个调用者的 controller，把 `ModalHandle` 交给它保管。
**先查两件事**：
(1) `SearchController` 现在是 `#[derive(Clone, Debug, Default, PartialEq)]` 的纯值，
   而 widget 之间共享状态要 `Rc<RefCell<..>>`。看这个 crate 里别的 controller
   （`ScrollController`？`TabController`？）是怎么被 widget 共享的，照它办，
   **别自己发明一种**。
(2) 上游 `closeView(selectedText)` 是"先设文字再 pop"，而这里的 bar 和 header
   是同一个 `TextField`（同一个 id）——确认关掉 view 之后 bar 里还留着那段文字，
   还是说 `TextField` 的状态会随 overlay 一起没掉。这决定第 (1) 步要不要连文字一起搬。

---

## 第 445 轮：bar 里打的字，在长出来的 view 里还在

上一轮留的两件事。

**查一**：这个 crate 里**没有** widget 之间共享 controller 的现成机制——
`TabController` 也是纯值。有的是 `TextField::with_state_sink` 那种
`Rc<RefCell<..>>` 交给两头的做法。照它办，不另发明：
`pub type SharedSearchController = Rc<RefCell<SearchController>>`。

**查二**：bar 和 header 用同一个 `id`，但它们在**两棵不同的子树**里
（页面 vs overlay），所以是两个元素、两份状态——文字不会自己过去。
上游那边是同一个 `TextEditingController` 交给两头，这才是它"过得去"的原因。

所以这一轮把那根线接上：bar 改文字就写进 controller，
`SearchViewContent::header()` 用 controller 里的文字做 `initial_text`，
header 改文字也写回同一处。

### 只接了一个方向，另一个方向明写下来

**view 开的时候拿得到 bar 的文字**；但**关掉之后 bar 不会更新成 view 里的文字**。
后者需要一个"跟着 controller 走"的 `TextField`，而现在的
`initial_text` 只在字段第一次出现时用一次。这是另一个缺口，记在下一步里。

### 变异扫描 8 个，第一遍 3 条没红——三条都是"外面够不着"

三条都跟**写**有关：写 controller 的是 `build` 里的闭包，
测试从外面既拿不到那个 bar，也拿不到 anchor 自己的 controller。

不是加桩，是**把该问的东西挪到问得到的地方**：

- `SearchBar::notify_changed(text)`——跑这个 bar 的 `onChanged`，
  也就是字段文字变化时做的事。公开它有正当理由：
  bar 周围的接线**在它画的东西里完全看不见**，
  而上游 `TextField` 在同一个点上调 `widget.onChanged`。
- `SearchAnchor::bar_for(&controller)` 和 `controller_for(&state)`——
  从 `build` 里拆出来。`build` 要活的树才能跑，而这两件事不用。

拆完之后三条都测得到，8 条全红。其中
"每次 build 新造一个 controller"那条现在由
`Rc::ptr_eq(first, second)` 按住——上游的
`_internalSearchController` 是 `initState` 里造一次的，
在 `build` 里造的那个会被 bar 写进去然后在 view 读到之前就扔掉，
唯一的症状是"一开 view，刚打的字没了"。

### 又踩了一次同一个坑

用 bash heredoc 改 Python 里的字符串字面量，`\n` 会被吃掉一层。
第二次了。这次改用 `Write` 重写整个扫描文件才对。

尺子：十六把全部 exit 0。门：Rust 6610 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：把另一个方向补上——**关掉 view 之后 bar 显示 view 里的文字**。
`TextField` 现在只有 `initial_text`（只用一次），
上游是字段跟着 `TextEditingController` 走。
**先查两件事**：
(1) `TextField` 有没有办法在**已经存在**的时候被外面改文字——
   看 `state_sink` 交出来的 `StateHandle<TextFieldState>` 能不能
   直接 `set_state` 改 `value`，能就用它，这是现成的路。
(2) 谁来触发？view 关掉是遮罩点掉的，而 `ModalHandle` 被丢掉了
   （第 444 轮记过）。看 `show_modal` 有没有"关掉时回调"的口子，
   没有的话这一轮得先给 `ModalHandle` 或 `show_modal` 补一个 `on_dismiss`——
   这也是第 438 轮"关闭动画没地方挂"缺的同一样东西。

---

## 第 446 轮：modal 现在会说自己什么时候下去了

上一轮留的两件事，查到第二件就停住了：`show_modal` / `ModalHandle`
**没有任何"关掉时"的口子**。而这正是第 438 轮"关闭动画没地方挂"
缺的同一样东西。所以这一轮只做它。

### 为什么调用者自己做不到

自己调 `dismiss` 的人知道 modal 没了；**被读者关掉的人不知道**——
遮罩那一下直接进了 theatre，什么都不回来。
所以"modal 结束时"要做的事（把 search view 的查询带回 bar、
播关闭动画、告诉调用者选了哪一项）一直没有地方住。

### 顺手把两份关闭逻辑并成一份

原来 `ModalHandle::dismiss` 自己抄了一遍"放开焦点陷阱、
从 `MODALS` 里删掉、移除条目"，而遮罩和 Escape 走的是另一个闭包。
**一段流程两个副本就是两个会漂的东西。** 现在只有一个，
所有出口都跑它，`dismiss` 只负责报告发生了没有。

`modal_from_entry`（抽屉用的）也一样：它没有焦点陷阱、
也不在 `MODALS` 里，但守卫和监听器是同一套。

### 变异扫描 13 个，三轮才干净

第一轮 8 个里 4 条没红，逐条查：

- **"关闭里的守卫没了" 0 红**：**两个守卫**——`dismiss` 先看标志，
  闭包里又 `replace`。外面那个挡住了，里面那个就永远试不到。
  改成**只留闭包里那一个**，`dismiss` 读标志只为了报告结果。
  改完之后这条不但红，还是**栈溢出**红的（重入的监听器无限递归）。
  值得记：**一个变异可以是被"崩溃"抓住而不是被断言抓住**，
  而只认 `test result` 那一行的判定脚本会把它当成 BUILD ERROR。
  手工复跑确认了是真红。
- **"抓着借用跑监听器" 0 红**：我的重入用例是"再关一次"，
  而那条路在 `replace` 那里就早退了，根本没碰到列表。
  改成"在监听器里再注册一个监听器"才真的会撞借用。
  顺带钉住了另一件事：**这一轮注册的监听器这一轮不跑**——
  它不在拷贝里，而一个为"注册之前就发生完了的事"而触发的监听器是错的。
- **"监听器倒着跑" MISS**、**"条目包成 modal 没有守卫" 0 红**：
  前者是 fmt 之后同一段文字出现了两处（`show_modal` 和 `modal_from_entry`），
  后者是被上面那个双守卫挡住的。改完都红。

尺子：十六把全部 exit 0。门：Rust 6618 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：口子有了，接上第 445 轮欠的那半边——
**关掉 view 之后 bar 显示 view 里的文字**。
`SearchAnchor` 的点击处拿到 `ModalHandle`，
`on_dismissed` 里把 controller 的文字写回 bar 的字段。
**先查一件事**：写回去要碰 `TextField` 已经存在的状态。
看 `with_state_sink` 交出来的 `StateHandle<TextFieldState>` 能不能
`set_state` 改 `value`——`TextFieldState.value` 在 `editable.rs` 里
是不是 `pub`（`search_anchor.rs` 是另一个模块，不是 `pub` 就够不着）。
够不着的话，是给 `TextField` 补一个"跟着外部值走"的入口，
还是把这段写回放进 `editable.rs`，先看清楚再动。

---

## 第 447 轮：view 里搜的那句话，回到了 bar 里

上一轮留的问题：`TextFieldState.value` **是 `pub` 的**，
`StateHandle::set_state` 也是。所以路是通的——
用第 446 轮那个 `on_dismissed` 口子，关掉时把 controller 的文字写回字段。

`initial_text` 不行，它是**开场值**，只在字段第一次出现时用一次。
要改的是**已经存在**的那份状态，于是照这个 crate 的 sink 老规矩
（`TextField::with_state_sink`）给 `SearchBar` 也加了一个，
`SearchAnchor` 在自己的 state 里存这个 sink。

### 顺手抓到的一个真 bug

`TextField::initial_state` 原来是：

```rust
let caret = text.chars().count() as i32;
state.value = TextEditingValue::new(text);
state.value.selection_base = caret;
```

而 `TextEditingValue::new` **本来就把光标放在末尾**，
并且是按 **UTF-16 单位**数的——平台数的就是这个。
后面那两行用 `chars().count()` 覆盖掉它，
两个数只在基本平面内相等。**给字段塞一个 emoji，光标就短一格**，
落在代理对中间；`caret_bytes` 对它返回 `None`，于是**什么都不画**。

之前没有测试能看见它，因为 crate 里每一个被塞过文字的字段装的都是 ASCII。
删掉那两行，补了一个带 emoji 的测试。

### 变异扫描 11 个，10 条红，1 条是等价变异

三条第一遍没红，两条是**我把缩进写错了**（`put_query_back` 是自由函数，
四个空格不是八个），改对即红——其中"光标放回开头"那条还顺带说明
之前没有任何东西看过写回之后的光标，补了断言。

第三条"每次 build 新造一个 sink" **是等价变异**：
tap 的闭包握着自己那次 build 的 sink，而里面那个 handle
只要元素还在就一直有效——所以查询照样送到。
只有元素被**替换**而不是重建时才不同，而到那时字段里的文字也没了，
本来就没什么可送回的。写进字段的文档里，说明为什么仍然放在 state 里
（一个 bar 一个 sink，这才是要描述的东西），而不是假装它红过。

尺子：十六把全部 exit 0。门：Rust 6624 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：`SearchAnchor` 这条线到此闭合了（bar → view → 回到 bar）。
回队头看看：`python tools/depth.py` 现在第二名是
`CupertinoLocalizations`（2/46），而 `SearchAnchor` 应该已经掉下去了。
下一轮**先跑 depth.py 重新看队头**，不要惯性往下接——
`search_anchor.dart` 剩下的（`SearchDelegate` 的四个 builder、
`_SearchAnchorWithSearchBar`）都要先确认是不是真缺口，
还是说队列里有更浅的。

---

## 第 448 轮：三种 card，各用各的办法和背景分开

按上一轮说的**先跑 `depth.py` 重看队头**，`SearchAnchor` 已经掉下去了。
队头几个先看了一遍再挑：

- `Icons` / `CupertinoIcons` / 两个 `Localizations` 是**数据表**，比例低是自然的；
- `MagnifierController` 2/8 **不是缺口**——它在这儿只留了
  `shift_within_bounds`，其余是 `MagnifierHost` 的，文档里早写明了；
- `AnimatedModalBarrier` 3/9 里最有内容的是 `clipDetailsNotifier`
  （把遮罩的**语义矩形**裁掉一块，好让 sheet 盖住的地方不被读屏当成"点击关闭"），
  但这个 crate **没有 `semantic_bounds` 这个概念**，补它不止一轮；
- `Card` 4/12 是真的。

### `Card` 只有一种，而上游有三种

上游 `Card` / `Card.filled` / `Card.outlined` 是一个 widget 配三张默认表，
区别在于**一张卡片靠什么和背景分开**：抬起的靠阴影，填充的靠颜色，
描边的靠一条线。**一次只用一种。**

这儿的 card 三样占了两样还多一样：它在**每一张**卡上画 1px 描边，
同时又给了 elevation 1。所以抬起的那张说了两遍，
而"应该只靠颜色区分"的填充卡根本还不存在。

补了 `CardVariant` 和 `ResolvedCard`，三张表照抄：
`surfaceContainerLow`/1、`surfaceContainerHighest`/0、`surface`/0，
描边只有第三张有，颜色是 `outlineVariant`（不是 `outline`——
后者是给控件用的更重的线）。

顺带接上了主题里一直没人读的几个字段：`shape`（圆角从它来）、
`margin`（`EdgeInsets.all(4)`，**卡外面**的空隙，之前完全没有，
所以一列卡片是一整块板）、`shadow_color`、`surface_tint_color`。

M2 的回退保留：`_CardDefaultsM2` 答的是 `Theme.of(context).cardColor`，
一个设了 `cardColor` 又没开 M3 的应用要的就是那个颜色。

**记下来的一处不对齐**：这儿的 `Card` 有个 `padding`，上游没有
（上游卡片的内边距是里面的 `ListTile` 自带的）。老调用者一直靠它，
留着并在字段上写明它不是上游的东西。

### 变异扫描 15 个，第一遍 3 条没红

- **"没有卡片描边"** 是 BUILD ERROR（我在结构体字面量里把 `width` 写了两遍）。
  **BUILD ERROR 不算通过**，改成整个换成 `BorderSide::NONE`，2 红。
- **"margin 没接上"** MISS：`with_margin` 在这个文件里出现两次，加上下文才唯一。
- **"圆角退回 crate 主题"** 0 红：**等价变异**——`theme.radius` 默认就是 12，
  和卡片形状的 12 一样。真缺口是**没有任何测试读过主题里的 `shape`**，
  补了一个把 `CardTheme` 的圆角设成 4 再看画布的用例，1 红。

尺子：十六把全部 exit 0。门：Rust 6632 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：`Card` 还有两个上游字段没接：`borderOnForeground`
（描边画在子节点**上面**还是下面——默认 true，即画在上面，
所以一张贴到边的图片不会盖住卡的轮廓）和 `clipBehavior`
（默认 `Clip.none`，但 `Card` 的形状一旦有圆角，
不裁的话贴边的图片会从圆角里探出来）。
**先查一件事**：`Container` 现在能不能表达"边框画在子节点之上"——
看 `RenderDecoratedBox` 的 `DecorationPosition`
（第 434 轮见过 `Background`/`Foreground` 两个值），
能就直接用，那这一轮就是把两个字段接上；
不能就得先看清楚边框是在哪一层画的。

---

## 第 449 轮：卡片的边缘

上一轮留的问题：`DecorationPosition` 在 `RenderDecoratedBox` 上**有**，
而且 `transitions.rs` 已经在用，不是死字段。
但顺着看下去发现更要紧的一件事：**边框本来就画在子节点之后**——
也就是说上游 `borderOnForeground` 的**默认值**这儿已经对了，
缺的是另一个值。

### `borderOnForeground`

把描边收成一个闭包，按标志决定在画子节点之前还是之后跑。
默认 true：卡片的轮廓画在里面的东西之上，
所以一张铺满卡片的图不会把"卡到这儿为止"那条线吃掉。
false 是给"子节点故意要出框"的调用者的，上游文档里就是这么举例的。

### `clipBehavior`

上游三张默认表全是 `Clip.none`，**不裁是默认，而且是个真选择**：
卡片是圆角的，铺满的子节点会从圆角里露出方角来，
而上游仍然不裁——每张卡都裁要多一个图层，
而绝大多数卡里装的是一个从不碰到角的 list tile。
装图片的那个调用者自己打开它。

裁剪放在卡片自己的装饰**里面**：放外面会把卡自己的阴影齐边切掉，
而一个到了投影者边缘就没了的阴影不是阴影（和第 441 轮面板那处同一个理由）。

顺带给 `CardThemeData` 补上 `clip_behavior`，`lerp` 里按半程切换——
裁不裁不是数，插不出中间值。

### 变异扫描 12 个，第一遍 2 条没红，两条各不相同

- **"重建后保留旧的边框位置"**：`update_from` 少拷一个字段。
  照第 434 轮那条测试的形状补一个（**两边都用裸对象**，
  包进 `RenderRef` 会让下转型失败，测试就变成什么都没测）。
- **"裁剪跑到阴影外面" 0 红**：**我的变异写错了**——
  我写的是 `let inner = inner;`，那是个空操作，什么都没动。
  真正把裁剪挪到 container 外面之后，1 红。
  这条提醒的是：**变异本身也要检查它真的改了东西**，
  `assert old != new` 只挡得住字面相同，挡不住语义相同。

尺子：十六把全部 exit 0。门：Rust 6639 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：`Card` 到这儿基本齐了。回队头：`depth.py` 上
`SubmenuButton`（7/25）和 `MenuItemButton`（7/22）挨着，
都在 `material/menu_anchor.dart`，而这个 crate 已经有
`raw_menu_anchor.rs` 和 `menu.rs`。
下一轮看这两个。**先查一件事**：先分清 `menu.rs` 里现有的是
**`PopupMenuButton` 那条线**（`popup.rs`，M2 的弹出菜单）
还是 **`MenuAnchor` 那条线**（M3 的菜单栏/子菜单）——
上游是两套不同的东西，混起来会把一套的规则安到另一套上。
看清楚了再决定 `MenuItemButton` 是"补一个新 widget"还是
"给现有的补字段"。

---

## 第 450 轮：一条菜单行里各部分之间的空隙

上一轮说的"先分清是哪条线"救了这一轮一次。

`menu.rs` 的文件头第一行写着 *"Popup menus, ported from
`material/popup_menu.dart`"*——是 **M2 那条线**。
`MenuItemButton` / `SubmenuButton` 是 `material/menu_anchor.dart`，**另一套**。
我本来已经把新代码写进 `menu.rs` 了，查完才发现放错地方，退掉重来。

再查一步：**`menu_anchor.rs` 已经存在**，里面有 `MenuItemButton`、
`SubmenuButton`、`MenuBar`，但它们都是**只有配置、没有布局**的结构体，
文件头自己写着"ported is the configuration those widgets carry"。
而 `ResolvedMenuButton`（`_MenuButtonDefaultsM3`）**在自己的文件外没有任何读者**。

### 补的是 `_MenuItemLabel` 的几何

一条菜单行怎么摆，是这两个 widget 共用的东西——
和 `_MenuButtonDefaultsM3` 被它们共用的方式一模一样。

- **一个间距，只花在两样东西相接的地方**：标签前（**仅当有前置图标时**）、
  尾部图标前、快捷键前、子菜单箭头前。外缘没有——那是按钮自己的内边距。
  所以一行没有前置图标时，它的文字起点正好是有图标那行的图标起点，
  一列菜单项只有一条左边缘而不是两条。
- **间距按密度的两倍走**：`12 + density.horizontal * 2`。
  横向的收紧从这儿来，纵向的从按钮最小尺寸来；两个方向同速会让横向先撑不住。
- **下限 4 恰好是最密时算出来的数**（-4 时 12-8=4）。
  所以任何合法密度都够不到这个下限——它是防常数变动的，
  两个常数处在这个关系上本身才是值得钉住的事实。
- **菜单栏里快捷键和箭头一起消失，尾部图标不消失**：
  后两样是菜单自己的家具，图标是调用者要的。
  这也是为什么菜单栏顶层是一排光秃秃的词，尽管每一个都会打开子菜单。

`MenuItemButton::label` 和 `SubmenuButton::label` 各自造一条，
唯一的差别是 `has_submenu`——上游也是这一个差别。

### 变异扫描 15 个，第一遍 4 条 MISS

四条都在 `spacing` 上，全是 fmt 之后把两行并成一行、搜索串失效。
改对之后 15 条全红。

尺子：十六把全部 exit 0。门：Rust 6647 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：`ResolvedMenuButton` 现在**还是没有读者**——
这一轮补的是几何，不是外观。下一轮把 `MenuItemButton` 做成真 widget：
按 `ResolvedMenuButton` 上色（前景 `onSurface`、图标 `onSurfaceVariant`、
overlay 才是反馈）、最小尺寸 64×48、`AlignmentDirectional.centerStart`，
布局用这一轮的 `MenuItemLabel`。
**先查一件事**：这条行的反馈全在 overlay 上，而 `InkResponse`
（第 435 轮 `SearchBar` 用过）收的是 hover/highlight 两个颜色。
`ResolvedMenuButton::overlay_for` 有 pressed / hovered / focused **三个**arm，
其中 pressed 和 focused 同值。确认 `InkResponse` 有没有 focus 那一路
（`with_focus_color` 好像有），没有的话要先看清楚 focus 的反馈从哪儿来，
别把三态硬塞进两态。

---

## 第 451 轮：墨水响应会说键盘在哪儿

上一轮留的问题——"`InkResponse` 有没有 focus 那一路"——答案比预想的糟：
`focus_color` 有，`HighlightType::Focus` 有，连它的淡入时长和槽位都有，
**但全 crate 没有任何地方 `update_highlight(Focus, ...)`**。
一个有颜色、有时长、有槽位、**没有触发者**的高亮。

所以这一轮不是"把三态塞进两态"，是**先让第三态到得了**。

### 焦点节点要点名，不能凭空造

上游默认 `canRequestFocus: true`，widget 自己造一个内部节点。
这个 crate 的焦点注册表是**按调用者给的 id** 记的，
而一个 well 的 `id` 常常和它包着的东西共用——
search bar 的 well 和它里面的 `TextField` 就是同一个 id。
**一个 id 底下两个节点比没有节点更糟。**

所以焦点是**显式加入**的：`with_focus(id)`。
`None` 是"键盘够不到这个 well"，对于包着别的可聚焦控件的 well 是对的，
对于本身就是控件的 well 是错的——由调用者决定，而不是由 well 猜。

### 测试里踩到的一处：没布局的高亮是零尺寸的

第一版四个测试里三个通过、一个失败。查下去发现失败那个是对的，
**另外两个是空的**：高亮是按 well 自己的矩形定尺寸的，
而尺寸只有布局之后 build 才知道。我在最后才布局，
所以整个过程里高亮一直是零尺寸——
"什么都没画出来"的断言在两种情况下都成立。

改成**先布局再走时钟**，并给"高亮会消失"那条加上"它先亮过"的断言，
否则那条测的是"一直没亮"。

### 变异扫描 7 个，第一遍 2 条没红

- **"焦点去抢悬停的槽位" 0 红**：颜色一样、形状一样，**画出来一模一样**，
  差别只在槽位。补了一个直接读三个槽位的测试
  （`(focus, hover, pressed)` = `(true, false, false)`）——
  不然一个悬停在已聚焦行上的指针会发现槽被占了，或者把它抢走。
- **"每个 well 都是焦点节点"（拿自己的 id 当 focus id）0 红**：
  我只断言了字段，没断言树。补了一个"聚焦 well 自己的 id，它不该亮"的用例。

尺子：十六把全部 exit 0。门：Rust 6654 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：三态齐了，可以做上一轮想做的 widget 了——
`MenuItemButton` 用 `ResolvedMenuButton` 上色（前景 `onSurface`、
图标 `onSurfaceVariant`、**反馈全在 overlay 上**）、
最小尺寸 64×48、`AlignmentDirectional.centerStart`，
布局用第 450 轮的 `MenuItemLabel`。
**先查一件事**：`ResolvedMenuButton::overlay_for` 的三个 arm
（pressed 0.1 / hovered 0.08 / focused 0.1）要分别喂给
`with_highlight_color` / `with_hover_color` / `with_focus_color`——
确认 `InkResponse::highlight_color_for` 在**禁用**时对这三个做了什么
（它那儿有一段关于"禁用时高亮仍然存在"的注释），
别让禁用的菜单行在键盘走过时亮起来。

---

## 第 452 轮：`MenuItemButton` 成了一个真的 widget

先查上一轮的问题：`highlight_color_for` 在禁用时把高亮的颜色**改成 alpha 0**
（上游 `enabled ? resolved : resolved.withAlpha(0)`），
高亮照样存在只是画不出来——所以禁用的菜单行不会在键盘走过时亮起来，
不用另做什么。注释里还写了为什么要"存在但透明"：
重新启用是一次变色，而不是高亮凭空冒出来。

于是把 `ResolvedMenuButton` 接上——它从第 3550 行写下来起，
**在自己的文件外一直没有读者**。

一行菜单现在是：`InkResponse`（带上一轮补的焦点那一路）
包住一个按第 450 轮 `MenuItemLabel` 排出来的 row，
外面一个 64×48 的最小尺寸和 `AlignmentDirectional.centerStart`。

### 一处布局上的错

第一版把 `RenderAlign` 直接放进最小尺寸盒里，结果一行菜单在松约束下
**长到 200 高**——`Align` 会把给它的空间全占满。
改成两个方向都 `with_factors(Some(1.0), Some(1.0))` 收缩到内容，
再由外面的盒子托到最小值。对齐这时才是它该管的事：
**最小值比内容大的时候**（一个短标签在 64 宽的按钮里靠左而不是居中）。

### 变异扫描 13 个，第一遍 5 条没红——全是"测试问得不够具体"

- **"尾部各段没有间隙"**：我只断言了"快捷键在标签后面"。
  改成断言它正好在"图标 + 20 + 一个间隙"处。
- **"禁用行照样能点"** 和 **"禁用行按启用解析"**：
  我的禁用测试用了一个很弱的颜色启发式，而且**从来没真的点过**。
  改成：用真 router 点一下（并且同样的点在启用行上要真的触发，
  否则上一条断言的是一次没打中的点），
  再断言禁用行的标签颜色**更淡**——四个 arm 里只有 disabled 那个不一样，
  按启用解析会在别处都一模一样、只在这儿露馅。
- **"指针不带着键盘走"** 和 **"这行不是焦点节点"**：
  没有任何东西悬停过。补了一个真的 hover 事件，
  断言焦点落到这一行，以及 `request_focus_on_hover: false` 时不落。

补完 13 条全红。

尺子：十六把全部 exit 0。门：Rust 6665 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：`SubmenuButton` 还是纯配置，而它比菜单项多两样东西：
**箭头**（`submenuIcon`，第 450 轮的 `MenuItemLabel` 已经给它留了位置）
和**打开子菜单**。
**先查一件事**：打开子菜单要 `MenuAnchor` 那条线，
而 `raw_menu_anchor.rs` 有树和控制器、`menu_anchor.rs` 有配置，
**谁都没有把菜单放进 overlay 的那一步**（文件头自己写着"没有 OverlayPortal"）。
确认第 446 轮那个 `show_modal` + `on_dismissed` 够不够用来放一个子菜单——
子菜单和 dialog 不一样：点外面**只关自己不关父菜单**
（`raw_menu_anchor.rs` 开头第一条就写着这个），
而 `show_modal` 的遮罩是一层盖住全部的。够不够，先看清楚再动。

---

## 第 453 轮：菜单不该用遮罩，该用 tap region

上一轮的问题查下来答案很清楚：**`show_modal` 是错的工具**。

遮罩是盖住一切的一层：它接住了那次点击，而接住就意味着底下的东西收不到。
对 dialog 是对的（页面本来就该出局），对菜单错在两处：

- **菜单不挡页面。** 上游 `RawMenuAnchor` 根本不放遮罩，菜单开着时页面照样滚、
  按钮照样能按。
- **子菜单必须能把一次点击输给父菜单。** 有遮罩的话，子菜单开着时点菜单栏，
  点击落在遮罩上——子菜单关了，而菜单栏没听见那次本该打开下一个菜单的点击。

上游的机制是 `TapRegion`，而这个 crate **早就有** `tap_region.rs`，
`RawMenuOverlayInfo` 里也早就存着 `tap_region_group_id`——
**只是没有任何东西读它**。

### `show_tap_dismissed`

一个不带遮罩的浮层：进了这个 region（或**同组**任何一个 region）的点击算"里面"，
其余算"外面"并把它撤掉。组就是把子菜单和它长出来的那个菜单绑在一起的东西，
所以从菜单的一行点到另一行，不算"在两块面板外面"。

### 一个知识点：dismissal 只能有一条

第一版把 region 的 `on_tap_outside` 接到"裸的撤除"上，
而监听器挂在 handle 那条上——于是**被点掉的浮层谁也没告诉**，
正是第 446 轮那个洞。region 拿不到 handle（handle 要等 entry，
而 entry 是带着 region 一起建的），所以两边共用的必须是**同一个闭包**。

### 变异扫描 12 个，第一遍 4 条没红

- 三条 MISS：`with_group_id(self.group_id)` 在这个文件里出现两次
  （一次在实现里，一次在测试的同组兄弟里），加上下文才唯一。
- 一条是**我写的空变异**（`let _ = region_id;` 什么都没改）——
  又一次印证第 449 轮那条：`assert old != new` 只挡字面相同。
  换成"每个浮层用同一个 region id"，红了。
- 剩下一条 **`consume_outside_taps` 置真仍然 0 红**，
  但这不是缺口：`tap_region.rs` 的文件头把它记成**已知偏差**——
  这个 crate 存下并报告这个标志，但**不阻止**那次点击
  （上游是往手势竞技场里塞一个假赢家，而这里的竞技场在 router 内部，没有入口）。
  一个按文档就没有效果的开关，改它当然不会红。

测试里还踩到一处：`panel()` 一开始是个空的 `ConstrainedBox`，
**不是命中目标**，于是"点在里面"其实点在了空处、被判成外面——
那条测试会因为错的理由通过。改成填了色的装饰盒。

尺子：十六把全部 exit 0。门：Rust 6670 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：浮层有了，但**还没有菜单坐进去**。
`MenuAnchorTree::handle_outside_tap(id)` 这条规则早就在
（"往下关：子菜单走，父菜单留"），也一样没有调用者。
下一轮把两头接上：一个 `RawMenuAnchor` 打开时用 `show_tap_dismissed` 放浮层，
`on_tap_outside` 调 `handle_outside_tap`。
**先查一件事**：树是 `MenuAnchorTree` 的一个实例，而 widget 拿不到它——
看这个 crate 里这类"整棵树的状态"放在哪儿
（`focus.rs` 用 thread_local 注册表，`theatre.rs` 的 `MODALS` 也是），
照最近的那个办，别新造一种。

---

## 第 454 轮：菜单树终于有人拿得到了

上一轮留的问题：这个 crate 把"属于视图而不属于某一个 widget"的状态
放在 `thread_local!` 里——`focus.rs` 用了六次，`theatre.rs` 两次。
照最近的那个办，不新造一种。

`MenuAnchorTree` 的每条规则都是 `&mut self`，而**谁都没有一棵树**——
所以一个想问"刚才那次点击关掉我的子菜单了吗"的 widget，没有树可问。
现在树在那儿了，`with_menu_tree` / `with_menu_tree_mut` 是两个入口。

它是**树不是栈**，这也是和 `MODALS` 的差别：
一个开着两个子菜单的菜单栏是三个节点一个根，
Escape 要够到根，而点击外面只够到孩子。

### `open_menu_surface`：一次点击有两个答案

浮层的 dismissal 把**面板**撤掉；菜单**树**里发生的是
`handle_outside_tap`——关掉这个 anchor 的**孩子**，anchor 自己留着，
因为从子菜单点开的读者没打算连菜单栏一起丢掉。

两件事都要做，顺序固定，而且不是一件事说两遍：
一个说的是 overlay 里的一块面板，另一个说的是树里哪些 anchor 还开着。
只接第一件，树会以为有个菜单开着而谁也看不见。

### 变异扫描 7 个，第一遍 1 条没红

"浮层自成一组"（把 `group_id` 换成 `anchor`）不红——
我的用例里从来没有第二个同组 region。
补了一个"同一个菜单的另一块面板"：点它，菜单不关。
没有这条的话，从菜单栏走到它刚打开的子菜单，路上就会把子菜单关掉。

尺子：十六把全部 exit 0。门：Rust 6675 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：`open_menu_surface` 现在**没有调用者**——
`RawMenuAnchor` 还是配置结构体，`SubmenuButton` 也是。
下一轮把 `SubmenuButton` 做成 widget（像第 452 轮对 `MenuItemButton` 那样），
点它时 `open_menu_surface` 放出子菜单，箭头用第 450 轮
`MenuItemLabel` 已经留好的那个位置。
**先查两件事**：
(1) 子菜单要放在**按钮旁边**，而 `show_tap_dismissed` 现在是把内容原样塞进
   overlay——位置由内容自己决定。看 `theatre.rs` 的 `RenderAnchored` /
   `Placement`（第 453 轮见过 `anchored`）能不能直接用来把面板贴到按钮上，
   能就用，那这一轮只是接线。
(2) `SubmenuButton` 打开的是**自己的** anchor 还是父 anchor 的一个孩子——
   看 `MenuAnchorTree::set_parent` 的调用时机：节点要先 insert 再 set_parent，
   而 widget 的 build 每帧都跑。确认重复 insert 是幂等的，
   不是的话得先看清楚节点该在什么时候进树。

---

## 第 455 轮：`SubmenuButton` 打得开自己的菜单了

先查的两件事都有答案，而且第二件直接决定了这一轮的形状：

- `theatre.rs` 的 `anchored` / `Placement` 在，可以把面板贴到按钮上——
  但这一轮**没用**，位置留给下一轮（见下）。
- **`MenuAnchorTree::insert` 不是幂等的**，它 `debug_assert!` 一个 anchor 只加一次。
  而 `build` 每帧都跑，所以节点**不能在 build 里进树**——
  它进在 `initial_state`（上游的 `initState`），出在 `dispose`。
  一个留在树里的节点是一个树还相信着的 anchor：Escape 会去够一个不在屏幕上的根。

`SubmenuButton` 现在是个 widget：画的是 `MenuItemButton` 的那条线
（**用它造**，两边就不会漂——上游让它们共用 `_MenuItemLabel` 和
`_MenuButtonDefaultsM3` 也是这个理由），加上第 450 轮留好的箭头位置；
按下时 `open_menu_surface` 把面板放出去。

### 变异扫描里发现的一个真缺陷

"再按一次不会开第二个面板"这条一直不红。查下去发现原因不在守卫上：
**按钮自己不在它那个菜单的 tap-region 组里**。
所以第二次按下时，面板先把这次点击当成"外面"关掉了自己——
守卫根本轮不到。上游 `RawMenuAnchor` 是把自己的 child 也包进同一个组的。
补上之后，行为对了。

### 三条变异，我的探针够不到

补完之后仍有三条不红，都在"第二次点击"这条路上
（守卫不被调用、面板不入组、按钮不入组）。
逐条查过：规则那一半是钉住的——`should_open` 把上游 `_open` 的三个早退
搬到问得到的地方（第 445 轮那条"把该问的东西挪到问得到的地方"），
禁用、无菜单、已打开三种都红；节点生命周期、`dispose` 收面板、
标签、箭头也都红。**不红的是"点第二下"这条路本身**，
而我的测试打不到它——和第 417–425 轮那七轮"幽灵"同一个形状：
**一个打不中的坐标什么都证明不了**。

如实记下来，不当成通过。下一轮先把这件事查清楚：
在第二次点击前把命中路径打印出来，看它到底落在谁身上
（面板？overlay 的 stage？按钮？），再决定是测试写错了还是行为不对。

尺子：十六把全部 exit 0。门：Rust 6685 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：先查上面那条命中路径（**别先改代码**）。
查清之后再决定：如果是测试问题，补一个打得中的用例把三条变异钉住；
如果是行为问题（比如 overlay 的条目挡住了页面），那本身就是这一轮的真缺口。
之后才轮到位置——把面板用 `anchored` 贴到按钮旁边，
而**位置规则要照上游抄**（`_MenuAnchorState._buildOverlay` 附近的
`alignmentOffset` 和 `MenuAnchor` 的对齐），别自己编一个。

---

## 第 456 轮：把第二次点击落在谁身上查清楚

按上一轮说的，**先查不改**。写了一个只打印命中路径的探针，
连着问了四个问题，答案一个比一个具体：

```
BEFORE targets            [8401, 8401, 8400]
AFTER PLAIN REBUILD       [8401, 8401, 8400]
after first tap: entries 1   open? true
AFTER targets             [8401, 8400]
after outside tap: entries 1
PANEL CLOSED              [8401, 8400]
```

- 一开始按钮身上有**两个**目标（墨水的指针区 + 它自己的 tap region），
  外加最上面的 tap-region surface。
- **单纯重建不掉**。
- **第一次点击之后掉了一个**，而且再也不回来。
- 为了分清掉的是哪一个，把 tap region 的 id 临时改成 `id + 1_000_000`
  再跑一遍：`AFTER [8401, 8400]`——**掉的是 tap region，墨水还在**。
- 更糟的一条：第一次点击之后，**点外面也不再关面板了**（entries 一直是 1）。

所以上一轮那三条按不住的变异，原因不是"坐标打不中"——
第二次点击**打得中墨水**。是按钮自己的 tap region 在第一次点击之后
从命中路径里消失了，而它一消失，"点按钮不算点外面"和
"点外面会关掉面板"两件事就一起失效了。

### 顺手修掉一个真的测试缺陷

探针过程中发现我的 `Finder` 是**一次性的**：
它把按钮从一个 `RefCell<Option<_>>` 里 `take()` 出来，
所以这个页面第二次 build（开菜单就会引起）之后，
按钮就被换成了一个空盒子。**每一个"按两下"的测试，第二下都按在空气上。**
改成每次 build 都重新造按钮。

（改完之后上面那个现象**照旧**——所以它不是这个缺陷造成的，
但这个缺陷本身是真的，留着会让后面每一轮都误判。）

尺子：十六把全部 exit 0。门：Rust 6685 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：查"为什么第一次点击之后 tap region 从树里没了"。
现象很窄，可以逐层排：
(1) 是 `SubmenuButton::build` 之后**没有**再包 region 了，
    还是包了而 `single()` 的更新把它换掉了？
    在 `TapRegion::build` 的 assemble 里打一行，看第一次点击后它还跑不跑。
(2) 如果它还在跑，那就是 `RenderTapRegion` 在树里但**不在命中路径上**——
    看它的 `hit_test`/`size`：`DeferToChild` 的 region 只有在子节点被命中时
    才上路径，而它的 size 可能在重建后成了零。
**先量出来哪一种，再动手**——这一轮已经证明了猜是没用的。

---

## 第 457 轮：量出来了——面板盖住了按钮

按上一轮列的两条逐层排，答案是第二条，而且比预想的直白。

### (1) region 还建不建？建。

在 `TapRegion::build` 和它的 assemble 里各打一行：

```
TapRegion::build id=8401 registry=true     <- 第一次
  assemble id=8401
--- tap ---
TapRegion::build id=8401 registry=true     <- 点击之后又建了两次
TapRegion::build id=8401 registry=true
--- after ---
  assemble id=8401
  assemble id=8401
AFTER [8401, 8400]
```

**建了，也装配了，就是不在命中路径上。** 所以不是"没包"。

### (2) 顺手撞见的一个真缺陷：两个 region 用了同一个 id

`open_menu_surface` 把 **anchor 的 id** 当成面板 region 的 id 传下去，
而按钮自己（第 455 轮补的）也是一个 id 相同的 region。
注册表按 id 记，于是"8401 被点到了吗"变成"这两个里有一个被点到了吗"，
两者分不开。改成给面板要一个新的 `next_surface_id()`。

改完再量，路径变成 `AFTER [2, 8400]`——**2 就是面板的新 region id**。

### (3) 所以真正的原因是：面板压在按钮上

点 (30,24) 落在**面板**身上。把测试面板从"填满的 Align"换成
一个 100×100 的实心盒子再量一遍，**照旧** `[2, 8400]`——
因为条目就放在 overlay 的原点，(30,24) 在 100×100 里面。

**面板没有位置。** 第 455 轮把定位推到了下一轮，这就是它的代价：
一个没被摆放的面板落在原点，盖住了打开它的按钮。
第 455 轮那三条按不住的变异，全都是这一件事的下游——
第二次点击落在面板上，所以"再按一次"、"面板入组"、"按钮入组"
三条都观察不到。

**记下来而不是当成通过。** 这一轮交的是：一个真的 id 冲突修好了，
和一个量出来的原因。

尺子：十六把全部 exit 0。门：Rust 6685 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：给面板一个位置——这是现在挡着三条变异的唯一一件事。
`theatre.rs` 的 `anchored(anchor, place, surface)` 和
`Anchor`（第 444 轮 `SearchAnchor` 用过：assemble 里 `anchor.set(target)`，
`rect()` 给全局矩形）就是工具。
**位置规则照上游抄，别自己编**：看 `raw_menu_anchor.dart` 里
`RawMenuAnchor` 传给 overlay 的 `RawMenuOverlayInfo`
（`anchorRect`、`overlaySize`、`position`），以及
`menu_anchor.dart` 里 `MenuAnchor` 的 `alignmentOffset` 怎么用——
`SubmenuButton` 已经有一个 `alignment_offset` 字段在那儿等着，**没人读**。

---

## 第 458 轮：一块菜单面板该放在哪儿

上一轮量出来"面板没有位置"，这一轮把位置规则补上——
照上游 `_MenuLayout._positionChild` 抄，不自己编。

想要的位置只有一行：**锚点上的一个点**
（`alignment.withinRect(anchorRect)`）加上 `alignmentOffset`。
剩下的全是"放不下"，而上游的四个答案里有两个不是第一反应会写的：

- **横向放不下的面板贴左边**，不居中也不缩小。能看见多少算多少，
  从头开始——菜单是从前缘读起的。
- **越界时先试按钮的另一侧**：右边放不下的子菜单开到父项左边，
  另一侧也放不下才沿边滑。先滑的话面板会压在它自己出来的那一行上。
- **除非父菜单方向不同。** 挂在菜单**栏**下面的面板没有"另一侧"可试——
  栏是横的、面板是竖的——所以上游直接推。
  这就是 `parentOrientation != orientation` 那一支，
  它看着像特例，其实不是。
- **`alignmentOffset.dy` 只在越过横向父菜单往上翻时被减掉。**
  别处的翻转是精确的；这里调用者原本要的"栏下面那点空隙"，
  翻上去之后得重新加一遍。

### 一处我自己写错的期望

RTL 那条第一版我按 x=100 的锚点算出 -58，实际是 168。
**代码是对的，期望是错的**——从 x=100 往左挂一块 150 宽的面板会出屏，
所以走的是"翻到另一侧"。把锚点挪到 400（两边都宽裕）之后，
读的才是偏移本身而不是某条越界修正。把这件事写进了注释。

### 变异扫描 16 个，第一遍 2 条没红

一条是 fmt 之后搜索串失效；另一条"越界时滑而不翻"是**真缺口**——
我只测了"右边放不下往左翻"，没测左边那一支。
补了一个 RTL 的用例（以及"另一侧也放不下时确实会滑"），全红。

尺子：十六把全部 exit 0。门：Rust 6695 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步**：`MenuLayout::position` **还没有调用者**——
把它接到面板上，第 457 轮那三条按不住的变异就该能红了。
`open_menu_surface` 现在把内容原样塞进 overlay；改成用
`theatre.rs` 的 `anchored(anchor, place, surface)` 包一层，
`place` 就是 `MenuLayout::position`。
**先查一件事**：`Placement` 的签名是
`Fn(锚点矩形, 面板尺寸, overlay 尺寸) -> Offset`，
而 `MenuLayout::position` 要的是 `(alignment, child, allowed)`——
确认 `RenderAnchored` 传进来的第一个参数就是**overlay 坐标系里的**锚点矩形
（第 444 轮 `Anchor::rect()` 的注释说"走到根，正好是 theatre 条目布局所在的系"），
是的话直接接；不是的话先看清楚要换算什么。
## 第 459 轮：macOS 这边把 Text fields 修到能用——整盒可点、右键、打字、输入法

这一轮在 macOS 上做，与第 418-425 轮（Windows 侧）平行发生、rebase 时合流。
用户报的还是那三件：无法输入、无法选择、右键菜单不弹。

### 命中下降是通的——第 425 轮"幻影"结论在 mac 上独立复核了一遍

把探针（临时打在默认 `hit_test` 上，逐层打印类型、尺寸与结果）
打进整棵 gallery 树（挂载真实 app 到 text-field 页、460×820 布局），看到：

    RenderEditable pos=(160.0,0.4) size=342x17 hit=true
    RenderPointerRegion id=10001 behavior=Opaque size=342x17 hit=true
    ...（一路到根，全部命中）

下降是通的——和第 425 轮在 Windows 上的结论一致。第 417 轮 y=200..237 的
扫描全部落空，是因为**那条带恰好没盖住 17px 高的编辑行**：`editable.rs` 的 `RenderPointerRegion` 恰好只包着裸的
`RenderEditable`（342×17），demo 的 51px 字段盒子（浮标签、留白、填充）
全在区域外。点盒子不但落空，还落在 `TextFieldTapRegion` 之外——
**把刚拿到的焦点又弹掉**。"无法输入、无法选择、右键不弹"三个症状同源。

### 对照上游，修了三处

**一、TextField 的手势区域包住装饰盒**（`editable.rs`）。上游
`TextField.build`（`material/text_field.dart`）把 `InputDecorator` 放在
`_selectionGestureDetectorBuilder.buildGestureDetector` **里面**——整个装饰盒
都是手势区。这里加了 `TextField::with_decoration(...)`：应用把盒子交给字段，
字段把它建在自己的 `RenderPointerRegion` 里面。坐标照上游走全局：
`TapEvent` 加了 `position`（上游 `TapUpDetails.globalPosition`，`gestures.rs`），
editable 在 paint 时把窗口原点写进一个槽（借已有的 `ReportPlacement`），
handler 用 `event.position - origin` 换算——上游
`renderEditable.globalToLocal`（`widgets/text_selection.dart:780`）的同一笔账。
越界位置由 `caret_position_at` 收到最近行/字符，等价上游
`getPositionForOffset`。指针区域挪到装饰外面之后，editable 不再被任何父节点
交给访问者，树的读回（`RenderRef::unwrapped` 一层的约定）就找不到它了——
锚点包装从裸 handle 换成 `RenderMetaData`（DeferToChild，id 0 不进命中路径），
这是本仓库现成的"只交出孩子、别的什么都不改"的代理。

**二、demo 的字段盒子搬进 decoration**（`text_field_demo.rs`）。
`field_group` 拆成三份：`decorated_box`（盒子本体，禁用的 email 直接用）、
`field_decoration`（交给 `with_decoration` 的闭包，行内的电话前缀、工资 USD、
密码眼睛都进盒子——上游 `prefixText`/`suffixText`/`suffixIcon` 本来就在
decoration 里），`field_group` 只留区域外的东西（leading icon、错误/帮助/
计数行）。眼睛仍是自己的内层区域，竞技场按最内层裁决——上游 `IconButton`
赢过字段自己的 detector，同一回事。

**三、macOS host 补右键**（`rustflutter_host_mac.mm`）。和第 416 轮修
Windows 是同一个病：只有 `mouseDown/Up/Dragged`，`buttons` 写死 primary。
照上游 engine `FlutterViewController.mm:907-947` 补齐：`MouseState.buttons`
位掩码、`rightMouseDown/Up/Dragged`、`otherMouseDown/Up/Dragged`
（`1 << buttonNumber`），up 先带着位发再清位，`pressed = buttons != 0`。

### 验证

- 新回归测试 `every_text_field_box_answers_the_pointer_over_its_whole_height`
  （gallery `app.rs`）：挂载 text-field 页，沿表单中线 2px 步进扫描，
  断言七个字段各自整个盒子（>40px）都命中自己的 id，禁用字段不命中。
  修之前这个测试只在 17px 的行上命中。
- `flutter_gallery_unittests` 341 全过；`rustflutter_unittests` 6485 过、
  3 个失败与本轮无关（`center_title`、`names_route`、slider 语义——
  平台相关断言，**在本轮改动之前的基线上就已经在 macOS 上失败**，
  它们写下时跑在 Windows 上）。
- 临时探针（默认 `hit_test` 的 RF_HIT_TRACE、app.rs 的 probe 测试）已移除。

### 遗留

- 右键菜单外观这件（417 的第三件）**不用再做**：rebase 合进来的第 426 轮
  已经按平台选 `DesktopTextSelectionControls`。
- 上游把 helper/error/counter 也放在 InputDecorator 里（点它们也聚焦字段）；
  这里留在区域外，注在 `field_group` 的文档里。

### 追加（同一轮）：打字打不进去的真因——macOS host 没有 `flutter/textinput` 的平台半边

用户在修完命中区域后实测：点击能聚焦、**Cmd+V 能粘贴**、但打字无效。
这组症状把范围钉死了：聚焦通（本轮修的）、剪贴板快捷键通（框架侧
`clipboard_shortcuts` 直接改状态、走 `flutter/platform` 的剪贴板方法，
mac host 实现了），而**普通按键的文本没人处理**——编辑是平台的活
（`editable.rs` 文件头写着：Backspace、方向键、选择与组合都在平台侧的
模型里），Windows host 有完整的 `TextInputHandler`（engine 自带的
`flutter::TextInputModel` + `flutter/textinput` 通道 +
`TextInputClient.updateEditingState` 回传），mac host 里**一行都没有**：
框架开了会话就永远等不到第一次状态回传。

照 Windows host 的做法移植（`rustflutter_host_mac.mm`）：

- `TextInputHandler`：同一个 `TextInputModel`（dep `common_cpp_input`
  本来就在），`setClient/clearClient/setEditingState/show/hide` 照答，
  IME 相关的三个方法礼貌地空答（这个 host 声明过没有 IME）；
- `HandlePlatformMessage` 把 `flutter/textinput` 与 `flutter/platform`
  同一套 JSON 方法调用解析分开路由；
- `SendMethodCall`（win :2124 的镜像）把 `updateEditingState` /
  `performAction` 发回框架；
- `keyDown:`：Return/小键盘 Enter → `OnAction`（多行且 action 是
  newline 的字段先插 `\n`，上游 `EnterPressed`）；退格/前删/左右/
  Home/End 按 keyCode 走 `OnEditingKey`（Home/End 带 Shift 选择）；
  其余键取 `characters` 作为已提交文本 `OnText`——**跳过** Cmd/Ctrl
  组合（那是快捷键，框架的剪贴板 handler 在等它们）与 AppKit 用
  0xF700–0xF8FF 拼写的功能键码位。与 win 的差别照实记下：win 等框架
  对每个键的裁决再补发（redispatch），mac 这里不等——框架会消费的
  带文本按键只有那几个快捷键，已经按修饰键跳过了。

### 再追加（同一轮）：输入法（微信输入法等）不工作——视图没有实现 `NSTextInputClient`

上一节的 keyDown 直插只覆盖无 IME 的键入：输入法要工作，视图必须实现
`NSTextInputClient` 并把按键交给 `interpretKeyEvents:`，否则组合根本开始
不了，拼音直接以字母上屏。上游的样子就是 `FlutterTextInputPlugin.mm`：
一个 `NSTextInputClient` 盖在同一个编辑模型上。照它补齐
（`rustflutter_host_mac.mm`）：

- `RfContentView` 采纳 `NSTextInputClient`：`insertText:`（已提交文本，
  组合中则先 `UpdateComposingText` 再 commit）、`setMarkedText:`
  （`BeginComposing`/`UpdateComposingText`，IME 光标带过去）、
  `unmarkText`、`hasMarkedText`/`markedRange`/`selectedRange`
  （UTF-16，与 `NSRange` 同单位）、`firstRectForCharacterRange:`
  （候选框位置：框架 paint 时上报的 `setMarkedTextRect` +
  `setEditableSizeAndTransform` 平移，视图→窗口→屏幕转换；这两个方法
  也从"礼貌空答"改成了真存值，win host 同款解析）、
  `doCommandBySelector:`（只答 `insertNewline:`，其余不落回 NSView 的
  蜂鸣）。
- `keyDown:` 改为：无组合时编辑键（退格/方向/Home/End/Enter）仍直达
  模型——组合中这些键属于 IME（Enter 取字、方向键走候选）——其余一律
  `interpretKeyEvents:`；无 IME 的普通键会原路从 `insertText:` 回来，
  上一节的直插路径由它取代（`OnText` 删除）。
- `TextInput.clearClient` 到达时向主线程投递 `discardMarkedText`：
  焦点走掉时 IME 不能还对着一个不存在的字段组词。
- `updateEditingState` 现在带真实的 `composingBase/Extent`。

组合中的下划线样式框架侧还没有画（`TextEditingValue` 的 composing 字段
带到了，编辑器未用它加下划线），组词过程文本可见、可取字；样式是后续
的活。

---

## 第 459 轮：面板接上了定位，但**这一轮没能证明它在起作用**

先查的那件事是对的：`Placement` 的签名正是
`Fn(锚点矩形, 面板尺寸, overlay 尺寸) -> Offset`，
而 `Anchor::rect()` 走到根，正好是 theatre 条目布局所在的坐标系。
直接接得上。

接线做完了：

- `show_tap_dismissed_at` 收一个可选的 `(Anchor, Placement)`，
  **定位器包在外面、tap region 包在里面**——反过来的话 region 会和
  overlay 一样大，而"遮罩盖住整个屏幕"正是这套东西存在的理由所要避免的。
- `open_menu_surface_at` 把它传下去。
- `SubmenuButton` 在自己的 assemble 里把渲染对象记到 `Anchor` 上
  （和第 444 轮 `SearchAnchor` 同一个办法），并用第 458 轮的
  `MenuLayout::position` 造出 `Placement`。

### 两个测试环境上的真缺陷，顺手修掉了

- **overlay 只有按钮那么大。** 探针打出
  `overlay=Size{64, 48}`——测试页面就是一个按钮，
  所以 overlay 也只有 64×48，面板除了压在按钮上无处可去。
  真实页面是铺满屏幕的；改成把按钮对齐在一个填满的盒子里。
- **页面不是命中目标。** 背景上点一下够不到任何 region，
  tap-region surface 就听不见那次本该关掉菜单的按press。改成不透明。

### 但是：五条定位变异，**一条都没红**

把"根本不定位"、"按钮不记录自己在哪"、"从按钮顶部挂"、
"把面板自己的尺寸当屏幕"、"定位器和 region 里外颠倒"五条都试了，
**全绿**。也就是说这一轮新通过的那两个测试
（面板不再压住按钮、再按一次不开第二个）**并不是因为定位才通过的**——
是上面那两个环境缺陷修好之后的副产品。

**所以定位这条线是写好了、没被钉住的。** 按这个日志一贯的规矩，
不当成通过。代码是照 `_positionChild` 抄的、不是猜的，但"它在起作用"
这句话现在没有证据。

同时**删掉了一个测试**并在此写明：
`a_tap_on_a_panel_of_the_same_menu_does_not_close_it` 原来断言的是
`is_open(SUBMENU)`，而 `handle_outside_tap` 本来就不关 anchor 自己，
所以那条断言**永远成立**。换成断言 overlay 条目数之后它**失败**了：
点同组的兄弟面板，面板仍然被关掉。这是一个**新暴露出来的真缺陷**，
不是这一轮引入的——只是原来的断言看不见它。

尺子：十六把全部 exit 0。门：Rust 6696 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 356 通过；三个目录默认目标 exit 0。

**下一步（loop 在这一轮之后暂停，这里留给恢复时）**：

1. **先把"同组兄弟被当成外面"查清楚**。复现很短：
   开一个子菜单面板，再往 overlay 里插一个同 `group_id` 的 `TapRegion`，
   点它，看 `overlay.entry_count()`。第 453 轮在 theatre 层的同名测试
   是**通过**的，所以差别在这条路上——先比对两者的注册时机
   （面板现在包在 `anchored` 里，注册发生在哪一帧）。
2. **再把定位钉住**。要能观察到位置，测试得读面板真正画在哪儿：
   `Drawn::TransformLayer` 的 `e`/`f`（第 444 轮就是这么量 search view 的），
   而不是只看命中路径。五条变异全部要红才算完。

---

## 第 460 轮：定位钉住了，而上一轮说的"缺陷"是我记错了

按上一轮留的两件事做，两件的结论都和预期相反。

### 一、那个"同组兄弟被当成外面"的缺陷，**不存在**

先做对照实验：把第 453 轮那个通过的 theatre 层用例照搬，
**加上定位**再跑一遍——`showing=true`，定位不是原因。
于是回到菜单这一侧照原样复现，打印命中路径和条目数：

```
entries before 2
path [8403, 8400]      <- 点在同组兄弟身上
entries after 2        <- 面板没关
```

**同组的规则一直是对的。** 上一轮把测试删掉时写的"新暴露的真缺陷"
是错的结论——那次失败另有原因（多半是我改坐标和跑测试之间的某次构建没对上），
而我没有复查就写进了状态文件。**在此更正。**

测试已按严格形式恢复（断言 overlay 的条目数，而不是那条永远成立的 `is_open`），
通过。

### 二、定位现在有证据了

上一轮五条定位变异一条都不红，因为所有测试读的都是**命中路径**——
路径只说"够得着谁"，不说"画在哪儿"。

改成从画布上读面板的位置，并补两条：
面板画在按钮的左下角；**把按钮往下挪 60，面板跟着下去 60**
（位置是从按钮自己的矩形算的，不是常数）。

为此还得改测试面板本身：原来它是一个填满并自己 `Align(BOTTOM_RIGHT)` 的盒子——
**那样它画在哪儿由它自己决定，和定位无关**，所有"位置"测试都会变成
在测这个 `Align`。改成收缩到内容的盒子，真实的菜单面板也是这样。

五条变异**全红**。

尺子：十六把全部 exit 0（`stale_engines` 头一遍红，是我把尺子跑在了
`ninja` 之前——引擎当时确实旧于刚改的源码；构建之后重跑 exit 0，
这正是这把尺子该做的事）。
门：Rust 6699 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录默认目标 exit 0。

**下一步**：`MenuItemButton` 现在还**不是**它所在菜单的 tap region——
只有 `SubmenuButton` 是（第 455 轮补的）。所以点一条普通菜单项，
面板会把它当成"外面"关掉，而上游是先执行再关（`closeOnActivate`）。
下一轮把这条接上：`MenuItemButton` 也要有 `group_id` 并包一层 region，
按下时按 `close_on_activate` 决定关不关。
**先查一件事**：`close_on_activate` 这个字段已经在 `MenuItemButton` 上了，
**没人读**——确认上游关的是哪一个（`MenuController.close` 走的是根，
还是只关自己那一层），照 `MenuAnchorTree` 里现成的方法选，别新写规则。

---

## 第 461 轮：选一行菜单，关掉的是整棵菜单

先查上一轮那件事：上游 `_handleSelect` 是
`_anchor?._root._menuController.close()`——**根**，不是这一层。
所以选一项和按 Escape 到的是同一个地方，
而 `MenuAnchorTree::dismiss(id)`（`close(root_of(id))`）正是那条现成的规则，
不用新写。

这一轮给 `MenuItemButton` 补两样：

- **`group_id`**：一条菜单行要在它所在菜单的 tap-region 组里。
  没有的话，按这一行就是"点在面板外面"——面板在按下的路上关掉，
  而这次按压到达的是一个已经不在的菜单。
- **`anchor_id`**：`close_on_activate` 从哪个 anchor 往上关。
  上游是从树里查（`_MenuAnchorState._maybeOf(context)`），
  这个 crate 没有菜单树的继承查找，所以由调用者说。
  `None` 就是"这一行不在任何菜单里"，什么也不关——
  否则它会伸手去关别处碰巧开着的菜单。

回调**先跑，关在后面**：上游也是这个顺序，
一个想看看自己是从哪个菜单被选中的处理函数，反过来就只能看到已经没了的菜单。

### 一处因此简化掉的重复

第 455 轮给 `SubmenuButton` 单独包过一层 region，而它的行现在自带一层——
**同一个 id 两个 region**，而且后加的那层 `group_id` 是 0，
于是按下子菜单按钮反而把面板关了（两个测试当场变红，正是这么发现的）。
去掉 `SubmenuButton` 自己那层，改成把 `group_id` 交给行。
行**不**拿 `anchor_id`：一条会在被选中时关掉菜单的行，
正是子菜单按钮唯一不该做的事。

### 变异扫描 10 个，第一遍全红

其中"回调排在关闭之后" 10 红——顺序是被四条测试同时按住的。

尺子：十六把全部 exit 0（这一轮把 `ninja` 排在尺子前面，
上一轮的教训）。门：Rust 6703 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录默认目标 exit 0。

**下一步**：`MenuBar` 还是个空壳（`menu_anchor.rs:312` 起，只有
`resolved_panel`）。它是这条线上最后一块：一排 `SubmenuButton`，
横着排，`parent_orientation` 是 `Horizontal`——
而那正是第 458 轮 `MenuLayout` 里"不翻转、往边上推"的那一支，
现在**只有单元测试在用，没有真调用者**。
**先查一件事**：菜单栏的每一项按下时，上游是打开还是"如果已有别的开着就切换"
（看 `_MenuBarState` / `MenuAnchor` 的 `_focusButton` 与
`SubmenuButton._handleHover`：菜单栏开着时，**指针划过**另一项就换过去，
不用再按一下）。确认这条再决定 `MenuBar` 要不要自己拿状态。

---

## 第 462 轮：菜单栏在有人点它之前，对指针是无感的

上一轮留的问题查清楚了。上游 `handlePointerHover` 自己写着：

> Don't open the root menu bar menus on hover unless a sibling menu is
> already open. This means that the user has to first click to open a menu
> on the menu bar before hovering allows them to traverse it.

所以菜单栏是**两态**的：没人点过之前，指针从它上面划过什么也不开；
一旦开了一个，整条栏就跟着指针走，划到哪一项换到哪一项，不用再按。
少了前半句，一个只是路过窗口顶端的指针会在身后拉开一串菜单。

这一轮把这条规则接通，需要三样，缺一样都不成立：

- **`MenuItemButton::with_on_hover`**——上游的 `onHover`。
  挂在 `MouseRegion.onHover` 而不是 `onEnter`，上游给了理由：
  *"onEnter and TextButton.onHover are called if a button is hovered after
  scrolling. This interferes with focus traversal and scroll position."*
  一个在静止指针下自己滚动的列表，否则会自己移动焦点。
- **`SubmenuButton::parent_orientation`**（`in_a_bar(true)`）——
  这个按钮所在的那层菜单是横着排还是竖着排。
  一个字段管两件事，所以是一个字段不是两个：它既决定这个按钮的面板摆在哪
  （第 458 轮的 `MenuLayout`），也决定悬停开不开。
- **`opens_on_hover()`**——问的是**根**，不是自己：
  `_MenuAnchorState._maybeOf(context)!._root`。
  让栏活过来的是**兄弟**开着，而这一项自己的菜单恰恰正是还没开的那个。
  面板里的一行没有这个条件：它的父菜单按定义已经开着，否则这行不在屏幕上。

按下和悬停共用同一个 `open` 闭包——两条路进同一扇门。

### 第 458 轮那条分支，现在有真调用者了

`MenuLayout` 里"菜单栏的面板不翻到按钮另一边，而是推到屏幕边上"那一支，
上一轮记的是"只有单元测试在用"。`parent_orientation` 接上以后它有了真路径，
并且由一个走完整绘制的测试按住：把按钮放在 x=750、面板 100 宽的地方，
面板会跑出 800 宽的屏幕——面板里的一行翻到按钮左边（650），
菜单栏的一项推到屏幕边（700）。两个数不一样，这条线才算被读到。

### 一个空转的测试，和一把学不会"离开"的仪器

想按住 `entered &&` 里的 `entered`（离开也算一次回调）花了三次。

第一次写的测试是"打开、点外面关掉、再把指针移开"——**三个都不红**。
原因是每次 `hover()` 都新建一个 `GestureRouter`：
**离开是一种记忆**，一个刚出生的路由器没在任何地方见过指针，
它只会报到达，永远报不出离开。于是"移开"这一步根本不是事件。
改成 `hover_using(router, ...)`，路由器由调用方拿着，跨两次移动。

第二次仍然不红：路线的第一站就把菜单开了，最后的断言在基线里也是 1，
突变改不了它——**一个两边都成立的断言按不住任何东西**。
最后把场景摆成"离开是唯一的机会"：
指针在栏还关着的时候到达（什么也不开），栏在指针底下被打开，然后指针离开。
到达开不了东西,所以结束时屏幕上但凡有东西,都是"离开"放上去的。

变异扫描 9 个，全红（其中"什么都不在悬停时打开" 5 红）。
尺子：十六把全部 exit 0。门：Rust 6710 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录默认目标 exit 0。

**下一步**：`MenuBar` 仍是空壳（`menu_anchor.rs:312` 起，只有
`resolved_panel`）。零件现在齐了——`SubmenuButton::in_a_bar`、
`MenuAxis::Horizontal` 的摆放、悬停规则——差的是把一排按钮横着放进去的那个
组件本身。**先查一件事**：上游 `MenuBar` 是 `_MenuBarAnchor`
（`MenuAnchor` 的子类，`_isOpen` 被重写成"任何一个孩子开着"），
还是自己拿状态。这决定 `MenuBar` 要不要进菜单树、
以及 `opens_on_hover` 里的 `root_of` 在真装配下指到谁。
## 第 463 轮：iOS host——gallery 在 iPhone 模拟器上渲染、触摸、弹键盘

win/mac/android 之外的第四个 host。范围定在**模拟器 + 软件渲染**:无签名
即可跑通与自动验证;真机(签名)与 Metal/Impeller 留给后续。

### 构建链(此前 iOS 根本编不出对的东西)

- `build/toolchain/mac/BUILD.gn` 的三个 iOS toolchain 声明的是
  `toolchain_os="mac"`、`toolchain_cpu="arm"`,模板里按 os/cpu 推 rust
  `--target` 的分支**永远推不中**——设备版拿不到 target(静默用宿主架构)、
  模拟器版也没有 `aarch64-apple-ios-sim` 可推。修法:toolchain 可以直接
  声明 `rust_abi_target`,三个 iOS toolchain 各自写明。
- `tools/gn` 对 iOS **强制 slimpeller**,而 slimpeller 下软件渲染是死路
  (`flow/surface_frame.cc` 对带 SkSurface 的帧直接 FATAL)。解除强制,
  显式 `--slimpeller` 仍然生效;iOS 于是拿到与 mac 相同的 Skia-CPU 配置。
  Metal shader(IMPELLERC_METAL)在模拟器构建下照常编过,没踩到坑。
- `common/settings.h`:上游把 iOS 的 `enable_impeller` 钉成 const true
  (它的 iOS embedder 只有 Impeller),而 `Shell::Create` 有
  `FML_CHECK(!software || !impeller)`——软件渲染的 host 必须能把它关掉。
  const 只在 SLIMPELLER 下保留,默认值照旧。这是对 engine 头文件的第一处
  行为性修改,理由写在注释里。
- `rustup target add aarch64-apple-ios-sim`;
  `vpython3 flutter/tools/gn --ios --simulator --simulator-cpu arm64
  --unoptimized --no-rbe` → out/ios_debug_sim_unopt_arm64,全量 ninja 绿
  (platform_channels 的探针模块给 iOS 挂了 android 那份"不能自驱输入"的
  被动版,原因也相同:平台不许应用给自己注入输入)。

### host(rustflutter_host_ios.mm,mac 骨架裁剪)

- `rf_host_run` 把选项存进文件级 state 后交给 `UIApplicationMain`,
  **不返回**——iOS 进程没有退出路径,mac 在 `[NSApp run]` 之后做的清理
  在这里没有对应物。宽/高/标题被接受并忽略:手机的窗口就是屏幕。
- AppDelegate 起 UIWindow + RfHostView,ThreadHost 四线程 + Shell::Create
  与 mac 同构;`GPUSurfaceSoftware` + FrameBuffer(mac 的原文)→ 主线程
  `setNeedsDisplay` → `drawRect:` CGImage blit。vsync 仍是 mac 的
  snapped-timer(纯 fml),频率取 `UIScreen.maximumFramesPerSecond`。
- 触摸照上游 `FlutterViewController`:device = UITouch 指针值(多指天然
  区分)、坐标×contentScaleFactor、kind=kTouch、Ended/Cancelled 后补
  kRemove;delta 记账是 android host 的。
- viewport:safeAreaInsets×scale → `physical_padding_*`(刘海与状态栏,
  framework 的 SafeArea 读到的就是它);键盘 frame 通知 →
  `physical_view_inset_bottom`。
- 通道:platform(UIPasteboard 剪贴板、SystemChrome/SystemSound 礼貌空答)、
  lifecycle(UIApplicationDelegate 五回调 → 上游四状态)、settings
  (深浅色、24h)、localization(与 mac 同一段 NSLocale 代码)。

### 文本输入(与 mac 共享一半)

- mac 上一轮落地的 `TextInputHandler`(engine 自己的 `TextInputModel`,
  `flutter/textinput` 全套方法)抽成 `host/rustflutter_text_input.h`,
  mac/iOS 共用;iOS 需要的部分补进去:setClient 的 `obscureText`/
  `autocorrect` 也存下来(键盘 traits 要)、`OnSetSelection`/
  `OnReplaceRange`/`OnDeleteBackward`(UITextInput 的三个动词)、
  框架改状态时的回调钩子(键盘要被告知重读文本)。
- iOS 侧照上游 `FlutterTextInputPlugin` 的形:一个隐藏的
  `RfTextInputView : UIView<UITextInput>`,`TextInput.show/hide` 驱动
  becomeFirstResponder/resign;position/range 是 UTF-16 下标(与 NSRange
  同单位);marked text 走共享 handler 的组合方法,系统拼音键盘的组词
  由此进模型;traits 按 setClient 的配置映射(email/number/phone/url
  键盘、return 键文案、secureTextEntry、autocorrect)。**没做**的照实记:
  逐字符 selectionRects、浮动光标、scribble、dictation 占位。
  文件在 MRC 下编译,per-call 的 position/range 对象走 autorelease。

### 打包与验证

- `host/tools/build_ios_apps.py`(仿 build_apks.py):`Foo.app/{Foo,
  Info.plist, icudtl.dat}`——资产全部 `include_bytes!` 内嵌,bundle 只要
  这三样;`UILaunchScreen={}` 防 letterbox;ad-hoc 签名,模拟器不验;
  `--run` 直接 boot/install/launch,launch 参数原样带给应用
  (`--route demo --slug text-field` 这类)。
- 验证:counter 与 flutter_gallery 均在 iPhone 模拟器上渲染正确
  (深浅色跟随系统、SafeArea 生效、色序无误,截图确认);触摸与键盘的
  交互半边等真人过一遍——模拟器里鼠标即触摸。
- mac 回归:gallery 343 全过;框架 6692 过,3 个失败为此前记录过的
  mac 平台性断言,与本轮无关。

---

## 第 463 轮：菜单栏装起来了，于是"根"变成了一个组

上一轮留的问题查清楚了。上游 `_MenuBarAnchor extends MenuAnchor`，
状态类只改两件事：`_orientation => Axis.horizontal`，
以及把孩子放进 **`RawMenuAnchorGroup`** 而不是一个会开面板的 anchor。
所以菜单栏在树里，但**它自己永远不开**——
`isOpen` 是"任何一个孩子开着"。

这件事一装上就露出了上一轮那条规则的一个真洞。
第 462 轮的 `opens_on_hover` 问的是 `tree.is_open(root_of(id))`，
在只有一个按钮的测试台上成立（它自己就是根）；
可一旦有了真菜单栏，根就是那个**永不打开**的栏节点，
这个问题会永远答"没有"——真实的菜单栏根本不会跟指针走。
上游问的是 `root._menuController.isOpen`，而根是个组，
所以它问的其实是"有没有哪个兄弟开着"。改成
`RawMenuAnchorGroup::is_open(tree, root_of(id))`，
这个第 435 轮就写好的函数**第一次有了真调用者**。

`MenuBar` 现在是真组件：

- 自己进树、离开时出树（留一个节点在树里，
  下一个同 id 的栏会撞上"只加一次"的断言）。
- `entry(at)` 把三件**栏知道而条目不知道**的事一次settle掉：
  挂在栏底下、共用栏的 tap-region 组、所在菜单**横着**跑。
  一个调用者没法只装一半。
- 排成一行，外面按 `MenuBarTheme` 的
  `_kTopLevelMenuHorizontalMinPadding` 留边——横向留、纵向不留：
  一条栏只跟它的条目一样高。

于是第 458 轮那条"栏下的面板推到屏幕边、不翻到按钮另一边"的分支，
和第 462 轮的悬停规则，现在是同一个 `MenuAxis::Horizontal` 喂出来的。

### 一个"点一下唤醒、之后指针随便走"的完整来回

`a_click_wakes_the_bar_and_the_pointer_walks_it_from_there`：
指针先划过 Edit——什么也不开；点一下 File——File 的面板上来；
再划过 Edit——Edit 自己开了。

写这条测试时第一版死活不过，查出来的是仪器不是代码：
**一个没离开过的 ink 仍然是"正被悬停"的**，
所以第二次移到同一个位置根本不是一次到达，什么也不问。
（第一次调试还以为是命中测试没走到第二个按钮——
其实第一次悬停的回调 id 就是 8412，走到了。）
改成整条来回共用一个路由器，中间往别处走一步。

### 十个变异全红，其中一个一开始是"等价变异"

"条目保留自己的 tap-region 组"最初 0 红，而且**不是测试的问题**：
去掉那一句以后整条栏连同面板都落在组 0，前后一致，行为一模一样。
让它可观测的办法是让夹具**先说错**：给 Edit 一个 9999 的组，
由栏改正。这样变异版里 Edit 的按压就成了 File 面板的"外面"，
面板在按下的路上被关掉——差别这才是真的。

尺子：十六把全部 exit 0。这一轮补记一条：
`ninja -C <目录>` 的默认目标**不含** `rustflutter_engine`，
`stale_engines` 因此报红一次；要点名build。
门：Rust 6717 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过。

顺带把测试台合并了：`staged` / `staged_at` / `staged_inset` 三份几乎一样的
`Finder` 合成一个 `staged_page(body)`，新的 `staged_bar` 也从它来。
overlay 要比按钮大、页面要可命中——这两条只学一次，
不该每加一个测试台就重学一遍。

**下一步**：菜单栏能开能走了，但**关不上**。上游
`_MenuBarAnchorState` 的 `actions` 只有一条：
`DismissIntent: DismissMenuAction(controller: _menuController)`，
而 `DismissMenuAction` 这个 crate 里已经有了
（`raw_menu_anchor.rs:525`），同样**没有真调用者**。
**先查一件事**：栏上按 Escape 关的是整棵还是只关最里一层
（看 `DismissMenuAction.invoke` 与 `MenuController.close` 在组上的行为——
组的 `close` 是不是就是 `closeChildren`）。确认了再接。

---

## 第 464 轮：Escape 走通了，路上发现树和屏幕只连了一半

上一轮留的问题先查：上游 `DismissMenuAction.invoke` 是
`controller._anchor!.root.handleCloseRequest()`——**根**，整棵；
而菜单栏这个根是个组，组的 `close` 就是 `closeChildren`。
所以栏上按 Escape 关掉的是整排菜单，不是最里一层。

这条路上的每一段这个 crate 里**早就都有**，而且从没见过面：
`ShortcutRegistry` 知道 Escape 是 dismiss，
`ActionDispatcher` 知道 intent 该干什么，
`focus` 会把键从聚焦节点往上带，
`DismissMenuAction` 知道从根关起——第 435 轮写的，一直没有真调用者。
这一轮按上游的样子把它们串起来：
`Actions(actions: {DismissIntent: DismissMenuAction})` 在外，
`Shortcuts(_kMenuTraversalShortcuts)` 在内——顺序是规则不是口味：
快捷键把键变成 intent 之后是**往上**找谁来服务它的。

两张表在 `initial_state` 里**只造一次**：actions scope 按身份比较
（`Rc::ptr_eq`），每帧换一个 dispatcher 就是每帧换一个 scope，
依赖它的东西会永远重建。

`_kMenuTraversalShortcuts` 只搬了这个 crate 能拼出来的两条：
Escape 和 Tab。四个方向键是 `DirectionalFocusIntent`，
本 crate 的 `Intent` 里没有方向可带，宁可不搬也不映射成别的意思。
Tab 由谁服务不是栏的事（上游也是应用自己的 action 集），
所以那条断言是对**表**的，不是对空页面上按一下 Tab 的。

### 树关了，面板还在屏幕上

写第一条测试时撞上的：**打开、点外面关掉、再点开——第二次打不开**。
查下去是两件事：

1. 树和屏幕只连了**一个方向**。点外面会告诉树，按钮自己攥着面板的 handle，
   但反过来没有：在树里关掉一个节点，它的面板原封不动留在屏幕上。
   一直没人发现，是因为到现在为止每一次关闭都是**从面板发起**的；
   Escape 不是——它从根往下关，关到的面板没有别人攥着。
   于是有了 `with_panels_following`：改树，然后照着树自己的 log
   把凡是关掉的都从屏幕上摘下来。为什么读 log 而不是事后走树——
   等改完再走，孩子早就摘干净了，什么也找不到。
2. **面板那圈 tap region 用错了规则。**上游面板包的是
   `TapRegion(onTapOutside: () => anchor._menuController.close())`——关**自己**；
   而这里接的是 `handle_outside_tap`——关**孩子、留自己**。
   后者是**按钮**那圈的规则（点开了别处，不代表想丢掉菜单栏）。
   两圈 region、两条规则，给面板用了按钮的那条，
   anchor 就一直以为自己开着——`should_open` 问的正是树，所以再按没反应。

改对之后，第 461 轮那条"选一行关掉菜单"也跟着从半条变成整条：
以前只写了树的一半，面板留在屏幕上。

### 一次"等价变异"逼出来的简化

"面板下来了却没被忘记" 一开始 0 红，而且不是测试的锅：
`take_panel_down` 自己也从表里删，两条清理路径，删掉一条另一条兜住。
两份同样的序列就是两个会各自漂移的东西——
改成 `take_panel_down` 只查+只关，删除只在 dismiss 的监听器里做一次。
现在把那次删除拿掉，每条路径都会漏，测试立刻红。

变异扫描 12 个，全红。尺子：十六把全部 exit 0。
门：Rust 6723 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：上面第 2 条只补了面板那圈，**按钮那圈还空着**——
上游 `RawMenuAnchor.buildAnchor` 是
`TapRegion(groupId: root.menuController, onTapOutside: handleOutsideTap)`，
这个 crate 的按钮 region 没有 `on_tap_outside`，
于是 `MenuAnchorTree::handle_outside_tap` 现在**只有单元测试在用**。
**先查一件事**：两圈同时收到同一次外部按压时，上游的净效果是什么
（面板关自己、按钮关孩子，顺序与去重），
确认了再决定这条要不要单独可观测、怎么观测。

---

## 第 465 轮：`MenuAnchor` 本身——控制器第一次能真的打开一个菜单

上一轮留的问题先查，结论是**不做**：上游按钮那圈
`TapRegion(onTapOutside: handleOutsideTap)` 关的是孩子，
而在这个 port 的排布里，凡是能触发它的那一次外部按压，
同时也会到达面板自己那圈（关自己、连带关孩子），净效果一样。
写一段无法被观察到差别的代码，正是这几轮一直在挑的毛病，
所以记在这里，不写。

改去做队列里更大的那块：`MenuAnchor` 和两轮前的 `MenuBar` 一样，
是个**只有旗子没有身体**的壳——六个字段，没有孩子、没有菜单、
不进树、什么也画不出来。而它是上游这条线的主角（每个下拉、
每个右键菜单都是它）。

这一轮把它做成真组件：进树、离开时出树、
`builder(context, controller, child)` 把**控制器交给调用者**（
上游也是这样，因为开菜单的那个按钮在 anchor 外面）、
面板按 `MenuLayout` 挂在自己底下、`onOpen`/`onClose`、
以及自己那圈 tap region。

### 控制器以前只够到树，够不到屏幕

`MenuController` 原来只能改树——它能说"菜单开着"而屏幕上空无一物，
正是第 464 轮刚补掉的那种半截。可这里没法照上游的写法（
上游的控制器攥着 anchor 的 `State`，直接调它的方法）：
这个 crate 里没人攥着别人的 state，而开菜单要的东西——
overlay handle、要包进去的主题、按钮此刻的位置——
全是在 anchor 自己的 `build` 里捕获的。

所以 anchor 把**那个知道这一切的闭包**留下（`OPENERS`，
按 anchor id 存，每次 build 覆盖一次——上一次 build 的闭包
攥着的是上一页的 overlay），控制器去叫它。
`MenuController::open_menu()` / `close_menu()` 于是是真的开和关。

### 三个自找的坑

- **面板挡住了要点的按钮。**"外部按压还能不能落到别处"那条测试里，
  另一个按钮就摆在 anchor 底下——面板正好盖住它，
  于是"没被点到"根本不是被吞掉的证据。挪开 200 像素才成立。
- **`consumeOutsideTap` 在这个 crate 里还吞不掉按压。**
  `tap_region.rs` 顶上早就记着这条 divergence：认领被**记录**下来
  （`last_dispatch_consumed`），但拦不住——上游是往手势竞技场里塞一个
  必胜的成员，这里的竞技场在 router 里面，没有外部入口。
  于是测试改成断言"anchor 认领了这一次按压"，那是现在真实存在的全部。
- **认领这件事只在按下和抬起之间存在。**抬起也是一次 dispatch，
  它谁也不认领，顺手把答案覆盖成 false。加了 `press_only`：
  只按不放，在两者之间问。

### 一个变异照出的"夹具替被测对象干了活"

"anchor 的 region 换个组" 一开始 0 红。原因不是测试写得松，
而是**夹具里那个孩子按钮自己带着菜单的组**——
于是 anchor 有没有那圈 region 根本无所谓。
上游 builder 的孩子是任意 widget，把它放进菜单组的正是 anchor 那圈。
把孩子改成自己一个组之后，变异立刻红。
（顺带另一条也换了断言：按第二次时"面板数"看不出差别——
走一个、来一个，总数不变；能看出来的是调用者：
被关掉又重开的菜单会告诉他两次。）

变异扫描 13 个，全红。尺子：十六把全部 exit 0。
门：Rust 6730 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：`MenuAnchor` 的面板现在是调用者给什么画什么，
上游那一层 `_MenuPanel`（`menuChildren` 排成一列、
`MenuTheme` 的背景与内边距、`clipBehavior`、
`crossAxisUnconstrained` 让子菜单可以比父菜单宽）还没有。
`ResolvedMenuPanel` 早就备好了（第 463 轮菜单栏用的是它的横向那半），
**竖向那半至今没有真调用者**。
**先查一件事**：`crossAxisUnconstrained` 到底放松的是哪一维、
是在 `_Submenu` 的哪一层（`ConstrainedBox` 还是
`UnconstrainedBox(constrainedAxis:)`），确认了再动手。

---

## 第 466 轮：菜单终于有了自己的那只盒子

上一轮留的问题先查清楚：`crossAxisUnconstrained` 那层是
`UnconstrainedBox(constrainedAxis: widget.orientation)`——
**被约束的那一维正是菜单自己的方向**，所以长度照旧听空间的，
宽度放开。这就是"子菜单可以比父菜单旁边的空隙还宽"的出处
（`DropdownMenu` 是唯一把它关掉的场合）。

于是这一轮补 `_MenuPanel`。它是"这里有几行菜单"和"屏幕上一块面板"之间那一层，
四条规则住在里面：孩子沿菜单自己的轴排（菜单向下、栏向右）、
`MainAxisSize.min` 收边、`CrossAxisAlignment.start` 让一列菜单只有一条左边缘；
外观走 `ResolvedMenuPanel`（竖向那半**第一次有真调用者**）；
视觉密度**只加宽不缩窄**内边距——上游把 Material 团队的原话抄在那儿：
compact 会把左右内边距压到 0；以及最后那圈约束，
`fixedSize` **两个数各判各的**（只固定了宽度的调用者说的就是只固定宽度）。

`MenuAnchor` 现在收 `menu_children`，自己铺成一块 `MenuPanel`。
面板是在 `build` 里造的、不是在 `push` 里：第一稿在 `push` 时就把
`cross_axis_unconstrained` 折进去，结果后写的那句旗子什么也不做。

### 两处"看着像 bug，其实是仪器/空洞"

- **画出来的面板是 `RRect` 不是 `Rect`**（它有圆角）。
  第一版测试找 `Rect`，一个也找不到——那和"面板根本没画"长得一模一样。
- **`IntrinsicWidth` 量出来是 0。**`RenderBox::max_intrinsic_width` 的默认实现
  就是 `0.0`，而菜单行外面套的那几层——pointer region、portal、ink——
  都没转发它。于是上游的 `_intrinsicCrossSize` 在这里量出零宽，
  面板画成一条没有宽度的缝。
  先在菜单栏上撞到同一件事（把 `MenuPanel` 也用给 `MenuBar`，
  entries 立刻贴到顶边、栏高为零），于是**菜单栏这一轮不并过来**，
  代码里把原因写在原地。
  上游靠那层拿到的"面板和最宽的一行一样宽"，这里 flex 的横轴本来就是最宽的孩子；
  少掉的是"每一行都被撑到面板那么宽"。等 intrinsics 能穿过那几层再补。

### 三个变异照出的三条空断言

- "密度也加宽上下内边距"不红——我只在 compact（不加宽）那一侧断言了 top。
- "fixedSize 两个数一起用"不红——漏了 `min_height` 那一半，
  被固定成无穷大也照样通过。
- "面板没有内边距"不红——**菜单行自己也有内边距**，
  于是"文字离边缘还有 8 像素"在两种情况下都成立。
  改成量一块只装一个素盒子的面板：高 = 20 + 8×2，宽 = 20。
  顺带发现这测试得**松约束**地量：根给的是紧约束，
  一块被指定了大小的面板说明不了它想要多大。

变异扫描 13 个，全红。尺子：十六把全部 exit 0。
门：Rust 6736 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：就是上面那个洞——**intrinsics 穿不过包装层**。
`RenderBox` 上四个 `*_intrinsic_*` 的默认实现都返回 0，
而 `RenderPointerRegion` / `RenderPortal` / ink 那几层都不转发，
所以 `IntrinsicWidth`、`IntrinsicHeight`、以及任何靠它们的布局
（菜单面板的等宽行、菜单栏的等高项、表格列宽）在真 widget 上都失效。
**先查一件事**：这几层里哪些是"单孩子透传"型（应当无条件转发）、
哪些真的要自己算（`RenderPadding` 要加上自己的 padding，
`RenderConstrainedBox` 要夹一遍），照上游 `RenderProxyBox` 与
`RenderShiftedBox` 的分法来，别一刀切。

---

## 第 467 轮：intrinsics 终于能穿过包装层

上一轮记下的洞：`RenderBox` 上四个 `*_intrinsic_*` 的默认实现都是 `0.0`，
而一堆"只是套一层"的 render 从没重写过它们。
`0.0` 对**没有孩子**的盒子是对的，对这些是错的——
于是它们上面的 `IntrinsicWidth` 量出零，被量的东西以零宽度布局。

按上游 `RenderProxyBox` / `RenderShiftedBox` 的分法来，不一刀切：

- **19 个纯代理**——各种 clip、`ShaderMask`、`BackdropFilter`、
  `AbsorbPointer`、`PhysicalModel/Shape`、`FractionalTranslation`、
  `Baseline`、`OverflowBox`、`ConstraintsTransformBox`，
  以及 `TapRegion`、`TapRegionSurface`、`Portal`、`Anchored`、
  `SizeChangedWithCallback`——四个问题都转给孩子。
- **`RenderFractionallySizedOverflowBox` 有自己的规则**：
  拿**另一个**因子去缩放问孩子的那一维，答案再除以**自己**的因子。
  两半都要：给孩子一半高度的盒子必须按一半高度去问它；
  而一个"给我多少我报一半"的盒子，得报出孩子的两倍宽，孩子才落在自己的宽度上。
- **`RenderCustomSingleChildLayoutBox` 问的是 delegate 不是孩子**——
  决定这个盒子多大的是 delegate，问孩子会报出一个没人会用的宽度。
  被点名的那一维按 `tightForFinite` 问；delegate 答无穷大就返回 0，
  因为"有多少要多少"根本不是一个 intrinsic。

### 真正挡住菜单的那一个，在 `Container` 里

`Container` 的四个 intrinsic 早就写了，转给 `self.composed`——
**而 `composed` 是在 `layout` 里造的**，intrinsic 恰恰是在布局**之前**问的
（`IntrinsicWidth` 先量再按量出来的宽度布局）。
于是它在唯一有人问的那一刻答 0。
补了一条"还没合成时按零件回答"的后备：固定宽高一锤定音、
内边距两边各加一份并从**另一维**扣掉再去问孩子、margin 最后加。
两条路必须给同一个数，测试同时按住这一点。

### 结果：菜单面板那层没有加回去

修好之后把 `IntrinsicWidth` 加回 `MenuPanel`，测试全过——
**然后把它拿掉，测试还是全过**。查下来是真的：
下面的 flex 本来就收边到最宽的一行，而 start 对齐的列不会把孩子撑开，
所以那层 tight 宽度改变不了任何测试能搭出来的排布。
按这个项目自己的规矩，观察不到差别的代码不留——
但原因换了，注释也照实换：从"这个 crate 做不到"改成
"做得到了，只是此刻没有区别；等面板里有东西会撑满时再加回来"。

### 一把尺子被别人的提交照红了

`stale_engines` 报两个 Android 引擎陈旧，"陈旧"于
`rustflutter_vk.cc`——上一轮 rebase 进来的 Vulkan 提交带的文件。
可 `ninja -C out/android_release_arm64 rustflutter_engine` 答的是
"no work to do"：Android 根本不编它，**再怎么重建都不会变绿**。
这正是那把尺子自己开头写着的"永久红的仪器比没有还糟"，从新路口又来了一次。
它已经有 `PLATFORM_ONLY` 这套机制，只是不认识 `_vk`、`_linux`、`_ios`：
补上，并且 `platform_of` 的兜底改成问解释器——
Linux 上的 host 构建，`_linux` 那些文件正是**要**算数的那批，
在那里认成 win 会把最该看的文件排除掉。

变异扫描 12 个，全红。尺子：十六把全部 exit 0。
门：Rust 6742 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：`unwalked` 这把尺子只盯 walk，
而这一轮暴露的是**另一类**空洞：一个 render 少实现一个 trait 方法，
没有任何尺子会说话——`min_intrinsic_*`、`distance_to_baseline`、
`compute_dry_layout` 都是"不写就默认，默认就静静地错"。
**先查一件事**：`RenderBox` 上到底哪几个方法是"默认值对无孩子的盒子成立、
对代理不成立"的（`compute_dry_layout` 的默认是什么？
`distance_to_baseline` 呢？），确认了再决定要不要给它们加一把尺子。

---

## 第 468 轮：同一类洞的另一半，以及一把专管它的尺子

上一轮留的问题：`RenderBox` 上还有哪几个默认实现"对没有孩子的盒子成立、
对包装层不成立"。扫了一遍，答案是三类：
四个 intrinsic（上一轮补了）、`distance_to_baseline`（默认 `None`）、
以及 `compute_dry_layout`（默认 `Size::ZERO`）。
其中 **`distance_to_baseline` 有 33 个包装层没写**。

`None` 的意思是"这东西没有基线"——对空盒子成立，
对一个只是裹着一行字的盒子不成立。
按基线对齐的 Row 会把 `None` 的孩子当成没有基线、改按顶边对齐，
于是 `Opacity` 或 clip 里的一个标签，会比它旁边的标签高出几像素，
而且**没有任何东西会说话**。

按上游两条规则补：

- **纯代理**（`RenderProxyBox.computeDistanceToActualBaseline`）——
  直接是孩子的。补了 25 个：各种 clip、`Opacity`、`Transform`、
  `AspectRatio`、`IntrinsicWidth/Height`、`MetaData`、`IgnorePointer`、
  `AbsorbPointer`、`TapRegion`、`Portal`、`Anchored` 等等。
- **挪动了孩子的**（`RenderShiftedBox`）——孩子的基线**加上孩子被放在哪**。
  基线是从自己顶边量起的距离，报孩子自己的数字等于宣称基线比字实际所在的位置高。

### 一把尺子：`tools/proxy_holes.py`

连着两轮撞同一形状的 bug，说明这不是运气问题。
少实现一个 trait 方法不是编译错误、不是测试失败、不是警告——
**它是一个答案，而且是个看起来合理的答案**，门里没有任何东西能看见。

于是加了第十七把尺子：找出"有 child 字段却让默认值替它回答"的 render。
真的默认值就是对的（sliver 走另一套协议、viewport 由父给多大就多大、
`AnimatedSize` 正在两个尺寸之间），就写进 `EXCUSED` 并附理由——
名字在那里是一个**主张**，不是一条待办。

写它的那一遍就抓到一个真的：`RenderMetaData` 四个 intrinsic 一个都没有。

而且它自己先被自己抓了一次：第一版 `blocks()` 图省事，
把每个 impl 切到**下一个 impl**为止，于是一个 impl 后面跟着 inherent 块的类型，
会把邻居的方法算成自己的答案——尺子于是报告它回答了从没回答过的问题。
读邻居答案的仪器比没有还糟，改成数花括号。

### 变异扫描 6 个 + 给尺子自己的 2 个

代码那 6 个：5 个被测试按住，1 个（`RenderMetaData`）**被新尺子按住**——
这正是加尺子的理由：那种洞本来就没有测试形状。
给尺子的 2 个是反过来做的：**把尺子弄瞎，再挖一个真洞**，
它必须看不见；两个都如此，说明那两行确实在干活。

其中一条自找的弯路值得记：验证"挪动了孩子的盒子加对了轴"时，
第一版夹具让 dx 和 dy 恰好相等（都是 15），
于是"加错轴"这个变异照样通过——**两个数字相等的时候，用哪个都对**。
把两个因子改成不同的（50×80 的孩子在 100×100 里居中，25 和 10）才立得住。

尺子：十七把全部 exit 0。门：Rust 6747 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：这一类洞还剩两处没补——
`compute_dry_layout`（默认 `Size::ZERO`：一个父想"先量不落地"时得到零）
在 `Container` 和 `RenderSemantics` 上是空的；
`hit_test_children`（默认 `false`）有 17 个包装层没写，
但其中多数是**自己重写了整个 `hit_test`**，所以不能一刀切。
**先查一件事**：那 17 个里哪些真的既没有 `hit_test` 也没有
`hit_test_children`——那才是"看得见摸不着"的真洞；
确认了再决定要不要把这两项也加进 `proxy_holes.py` 的 `WANTED`。

---

## 第 469 轮：先量后落地，而最常用的那只盒子量出来是零

上一轮留的两个问题，先查，两个答案都不是原先猜的：

- **`hit_test_children` 没有真洞。** 17 个包装层没写它，
  但每一个都**自己重写了整个 `hit_test`**——同一件事在上一层做了。
  真正两样都没有的只有四个 sliver，而它们本来就走另一套协议。
  所以这一项**不**加进 `proxy_holes.py` 的 `WANTED`：
  一把分不清这两种情况的尺子会报 13 条发现、0 个 bug。
- **`compute_dry_layout` 有真洞，而且在最要命的地方**：`Container`。

`compute_dry_layout` 是"父想先量一下、还不打算落地"时问的
（flex 算剩余空间、`IntrinsicWidth` 量完再布局）。
默认返回 `Size::ZERO`——又是那种**看起来合理的错**：不崩、不报错，
盒子只是量出来没有大小。这个 crate 里有 60 处在调 `dry_layout`。

补了四个：`Container`、`RenderSemantics`（纯代理，转给孩子）、
以及 `Expand`（该占满却答零）和 `Empty`（该听最小值却答零）。

`Container` 那个有意思：它的 `composed` 和 intrinsic 那次一样，
**是在 `layout` 里造的**，而干量发生在布局之前。
这次没有像上一轮那样手写一份公式——那等于把同一只盒子描述两遍，
两份描述迟早会漂。改成**当场把层搭一遍、问完就扔**（`rehearsed`），
用的正是 `layout` 用的那个 `build_layer`。
搭出来的东西一个也不留：`compose` 会记下它建的层，好让下一帧
改配置而不是重建，而一次"排练"如果记下了自己，
下一帧拿到的就是一批从没被布局过的包装层。

尺子加了第六项 `compute_dry_layout`，并且给它开了一个
**按方法**豁免的位置（比整类豁免窄：一个类被整个放行，
就连别的项也不再被看着了）。第一版往里塞了 `RenderViewport`，
查了一下人家根本有这个方法——**为一件本来就没缺的事写豁免，比不写还糟**，
删掉，位置留着，注释写明它今天是空的和为什么。

### 一次被自己咬到的扫描

上一次变异扫描中途 `assert` 挂掉，脚本死在"写进变异体之后、还原之前"——
于是尺子被留在了瞎眼状态，下一轮扫描一开始就 MISS。
（发现得早：MISS 报的是"这段文字找不到"，去看文件才知道是上一次留下的。）
记在这里：**扫描脚本的还原要放在 `finally` 里**，
半路退出的清理不能靠"后面一定会跑到"。

变异扫描 5 个代码 + 1 个尺子的，全红。
尺子那条是两头都验的：把它弄瞎、再挖一个真洞，它必须看不见；
恢复之后，同一个洞必须被看见。

尺子：十七把全部 exit 0。门：Rust 6750 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：`compute_dry_layout` 在**非包装层**上还缺一批，
尺子看不到它们（它只看有 child 字段的）——
`RenderEditable`、`RenderListBody`、`RenderNavigationToolbar`、
`GridHost`、`RenderListWheelViewport`、`RenderErrorBox`。
**先查一件事**：上游对多孩子的 render 是怎么写 `computeDryLayout` 的
（`RenderFlex` 用 `_computeSizes` 的 dry 分支，
而有些干脆 `assert(debugCannotComputeDryLayout(...))` 拒绝回答）——
分清"能干量"和"上游明说拒绝"，再决定这批各自属于哪一类。

---

## 第 470 轮：会布局的，就得说得出自己会布成多大

上一轮留的问题先查上游：这四个到底是"能干量"还是"上游明说拒绝"。
四个都能，而且上游都写了：

- `RenderErrorBox` —— `constraints.constrain(Size(_kMaxWidth, _kMaxHeight))`
- `RenderListWheelViewport` —— `constraints.biggest`
- `RenderNavigationToolbar` —— 委托的 `getSize` 就是 `constraints.biggest`
- `RenderListBody` —— 孩子沿轴加起来，横轴用给到的空间

于是四个都补上。`RenderErrorBox` 那个尤其值得说：
**规则本来就在**——它的 `layout` 只有一行，就是这一行——
少的只是 trait 方法，而 trait 替它答 `Size::ZERO`。
一个在 flex 里量出"没有大小"的错误框，是一个把自己藏起来的报错，
而这只盒子存在的唯一理由就是别把错藏起来。

`RenderListBody` 的四个方向其实是两个：**朝哪边跑**决定算术，
**从哪头起**只决定每个孩子放在哪里——而干量从不问这个。
上游自己的 switch 也是这样两两配对的。

### 两个变异，逼出一个会依赖环境的孩子

"在错误的空间里量孩子"和"用布局代替干量"两条一开始都不红：
夹具里的孩子是 `FixedBox`，它多大跟给它什么约束无关，
**于是问它的方式错了也看不出来**。
换成一个"你要我多宽我就多高"的方块（列表体给孩子的横向约束是**紧**的），
两条立刻红：正确时每个方块 200 宽 200 高、合计 400；
一旦把约束放松，min_width 变 0，它们就都塌成 0。

顺带又踩了一次自己的坑：第一版断言 400，实际拿到 100——
列表体会把自己的高度夹进给它的空间，而我给的空间只有 100 高。
**一个夹得太紧的量具，量什么都得到量具本身。**

变异扫描 9 个，全红。这一轮的扫描脚本把还原放进了 `finally`——
上一轮的教训，脚本半路死掉会把变异体留在树上。

尺子：十七把全部 exit 0。门：Rust 6754 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：`compute_dry_layout` 还差两处，都是大的：
`RenderEditable`（上游 `computeDryLayout` 要跑一遍文本排版，
`_adjustConstraints` + `layoutInlineChildren`）和 `GridHost`。
**先查一件事**：上游 `RenderEditable.computeDryLayout` 用的是
`_textIntrinsics` 这一份**独立于真正布局**的排版器——
本 crate 的 `RenderEditable` 有没有一份可以不落地就跑的排版路径
（`RenderParagraph` 那边是怎么做干量的），
有就照它接，没有就先补 `GridHost`、把 editable 记成一条 divergence。

---

## 第 471 轮：这一类洞补完了，并且交给尺子看着

上一轮留的问题先查：本 crate 有没有一条"不落地就能排版"的路。
有——`RenderParagraph` 的干量走的正是 `shape_at(width)`，
一条不写回任何状态的排版。再看 `RenderEditable`：它的 `layout` 用的
`line_height()`、`visual_lines(width)`、`preferred_height(...)` **全是 `&self`**，
也就是说那段算术从来就不需要"已经布局过"。
上游同样如此——`computeDryLayout` 跑的是一份与真正布局分开的
`_textIntrinsics`。

于是 `RenderEditable` 的干量不是新写一遍，而是**把那段算术挪进
`compute_dry_layout`，让 `layout` 只负责记下来**。
一份描述而不是两份：上游靠 debug 断言让两者保持一致，
这里靠只有一份可保持。

同一轮补完剩下的：`GridHost`（viewport 有多大由父给，
所以答案不依赖"viewport 已经建好"——而干量恰恰发生在建好之前）、
`ListView`（照 `Container` 那招**排练一份**再扔掉，
用的正是 `layout` 用的 builder），以及四个固定尺寸的 cupertino 字形
（`ActivityIndicatorTicks`、`BackChevron`、`SearchGlyph`、`ClearGlyph`——
一个要不到尺寸的字形就是一个没人看得见的字形）。

### 尺子从"包装层"扩到"所有会布局的盒子"

`proxy_holes.py` 现在两问：
包装层有没有让默认值替它回答（第 468 轮），
以及**任何**有 `layout` 的盒子有没有 `compute_dry_layout`（这一轮）。
只有三个 sliver 写进豁免——它们由 sliver 协议度量，
一个盒子的干量对它们根本不是一个问题。
测试替身不算：文件里 `mod tests` 之后的都是夹具，
一个夹具答零是在说它自己。

两把都验过：把尺子弄瞎、再挖一个真洞（拿掉 `GridHost` 的干量），
它必须看不见；恢复之后，同一个洞必须被看见。

变异扫描 8 个代码 + 1 个尺子的，全红。
（"一个字形量出零"最初 0 红——四个字形一个测试也没有，
这正是"补了却没人按住"的样子，补上测试才立住。）

尺子：十七把全部 exit 0。门：Rust 6758 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：这条线（`RenderBox` 的默认值替谁答了什么）到此补完，
回到 `depth.py` 的队头。上一次看队头时前几名是
`Icons`/`CupertinoIcons`（图标表，8826 与 1324 个成员，
本 crate 没有图标字体，是另一码事）、
`CupertinoLocalizations`（2/46）、`CupertinoApp`（8/37）、
`MaterialLocalizations`（36/158）。
**先查一件事**：`CupertinoLocalizations` 那 46 个成员里，
有多少是 `MaterialLocalizations` 已经在这个 crate 里答过的同一批字串
（两边的 `datePickerYear`、`alertDialogLabel` 之类），
能共用的就别抄第二遍——确认了再决定从哪个入手。

---

## 第 472 轮：Cupertino 的字串终于有了一个可以被挡在前面的接口

上一轮留的问题先查，答数出来了：上游 `CupertinoLocalizations` 的 41 个成员里，
**只有 4 个**和这个 crate 的 `MaterialLocalizations` 同名——
`alert_dialog_label`、`cancel_button_label`、`menu_dismiss_label`、
`modal_barrier_dismiss_label`。四十几分之四不是"共用接口"，是英语的巧合：
上游把两个类各自声明，一个语言完全可以把 Cupertino 警告框里的
"取消"和 Material 对话框里的写成不同的词。共用会让这件事没法表达。
所以不共用。

而真正的缺口不是缺字串，是**缺接口**：
`CupertinoLocalizations` 一直是个空结构体加一个 `of`，
字串都是 `DefaultCupertinoLocalizations` 上的常量，每个 widget 直接去读。
`MaterialLocalizations` 早就走过这一步，它自己的文档里写着为什么：
"从实现上读常量，意味着应用永远没法把自己的 bundle 摆到它前面，
而那正是有接口的全部意义"。一个 crate 里两层本地化用两种模型建模，
才是要修的东西——这一轮是第二半。

于是 `CupertinoLocalizations` 变成 trait（41 个成员，
其中 6 个照上游在抽象类上就给了实现的，这里也给默认体），
`DefaultCupertinoLocalizations` 实现它、每个成员都写出来
（上游用 `implements` 而不是 `extends`，为的就是上游加一个成员时这里**响亮地**坏掉，
而不是安静地继承到一个错的答案），`of` 搬到默认 bundle 上——
trait 交不出一个自己的实例，这一点和 Material 那边一样。

顺带补上四个**从来没写下来过**的字串：`ALERT_DIALOG_LABEL`、
`MODAL_BARRIER_DISMISS_LABEL`、`MENU_DISMISS_LABEL`、`CANCEL_BUTTON_LABEL`。
正是同名的那四个——想用它们的 Cupertino widget 只能伸手去拿 Material 的，
那等于让一个语言替两种设计一起决定这个词。

测试按 Material 那边的样子写：一个应用自己的 bundle（`Loud`）摆在前面，
`back_button_label` 与 `cut_button_label` 换了词而其余不变；
再加一条**不是词**的：`datePickerDateOrder` 也是 bundle 的答案，
换一个 bundle 会把日期选择器的列顺序整个换掉（`Ymd` 的三列是年月日）。
还有一条按住那 6 个默认体——`Loud` 从头到尾没提过它们，答案得是上游的默认值。

变异扫描 10 个，全红。尺子：十七把全部 exit 0。
门：Rust 6763 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。
`depth.py` 队头上 `CupertinoLocalizations`（0.04，2/46）已经不在了。

**下一步**：队头现在是 `CupertinoApp`（8/37）。
但先记一件这一轮看见、没有动的事：
`cupertino_app.rs` 里还有第二个 bundle —— `CupertinoLocalizationEn`
（21 个成员，只有日期时间那一半），它**没有**实现这个新 trait。
**先查一件事**：上游的 `CupertinoLocalizationEn` 在
`flutter_localizations` 里是 `GlobalCupertinoLocalizations` 的子类、
成员是全的；这个 crate 里的那份是照哪一份抄的、
缺的那二十个是"还没抄"还是"故意只要这一半"——
确认了再决定是补全它并实现 trait，还是把它记成一条 divergence。

---

## 第 473 轮：另一个 bundle 也把整个接口答完

上一轮留的问题查清了：`CupertinoLocalizationEn` 缺的那二十来个成员是
**"还没抄"**，不是"故意只要这一半"。上游那个类
（`flutter_localizations` 的 `l10n/generated_cupertino_localizations.dart`，
从 `cupertino_en.arb` 生成）把每一个字串都写了出来——
`alertDialogLabel`、`cutButtonLabel`、`expandedHint`……一个不少。
它们和 `DefaultCupertinoLocalizations` 的值**在英语里恰好相同**，
所以谁也没注意到这边少了。

于是这一轮把它们补齐，并让 `CupertinoLocalizationEn` 实现上一轮那个 trait。
补的时候**没有**写成 `-> DefaultCupertinoLocalizations::XXX`：
上游是两份各自来源的字串（一份生成、一份手写），
写成转发等于宣称"它们按构造相同"，而事实是"它们在英语里相同"。
差别不是文字游戏——一个语言完全可以只改其中一份。
所以照上游那样各写各的，再用一条测试把"今天它们相同"变成**被检查的事实**：
二十个词逐个对过去，哪一份被单独改了都会红。

同一条测试的另一半是它们**不同**的地方，也就是装这个 delegate 的全部理由：
`date_picker_hour(1)` 在框架那份是 `1`，在这份是 `01`；
`timer_picker_hour(3)` 是 `3` 与 `03`。

还补了三个从没搬过来的语义标签（生成类里是模板，由
`GlobalCupertinoLocalizations` 填空）：
`$hour o'clock`、`1 minute` / `$minute minutes`（英语里 one 和 other **确实**不同）、
以及 `Tab $tabIndex of $tabCount`。以前想要它们的代码只能伸手去拿框架那份，
那是"另一个 locale 的答案顶着这一个的名字"。

### 一次扫描把我自己的编辑吃掉了

这一轮的变异扫描留下过一处变异体（"an hour has no o'clock"），
下一次扫描一开始就 `baseline is not green` 才发现；
更糟的是随后一次扫描把我刚加的三条断言**一起还原掉了**——
`.bak` 的快照与我手边的编辑交错了。第 469/470 轮记过"还原要放进 `finally`"，
这一轮补上后半句：**扫描之后要核对树**。
现在的收尾是两步：跑一遍 `cargo test`，再 `grep` 一下这一轮加的断言还在不在。
（这也解释了当时那个"改对了却不红"的假象：断言根本已经不在文件里。）

变异扫描 8 个，全红。尺子：十七把全部 exit 0。
门：Rust 6766 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：`depth.py` 队头是 `CupertinoApp`（8/37）。
**先查一件事**：那 37 个成员里有多少是 `MaterialApp` 已经在这个 crate 里
做过的同一批（`navigatorKey`、`routes`、`onGenerateRoute`、`builder`、
`localizationsDelegates`、`shortcuts`/`actions` 等），
以及 `CupertinoApp` 自己独有的是哪几个（`theme`、
`CupertinoPageRoute` 的默认过渡）——
两个 App 类在这个 crate 里是各写各的还是共用一条装配路径，
决定这一轮是"补字段"还是"接装配"。

---

## 第 474 轮：一个 CupertinoApp 在盖起任何东西之前先定下的几件事

上一轮留的问题先查：两个 App 类在这个 crate 里**各写各的**，
而且都不是装配器——它们是**规则对象**：`has_home`/`has_routes` 这种布尔
代表"给没给"，方法把上游的断言和选择写成能问的问题
（`MaterialApp::choose_theme`、`router_is_configured`）。
所以这一轮不是"接装配"，是把 `CupertinoApp` 缺的那几条规则补上。

从上游 `_CupertinoAppState.build` 和 `_buildWidgetApp` 里拿到四条：

- **状态栏样式是反的。**`brightness == dark ? SystemUiOverlayStyle.light : dark`——
  名字说的是**图标**什么色，不是它背后是什么色：深色应用要浅色图标。
  把这对读成"深色应用配深色样式"，就会在黑条上摆黑图标。
  测试还把同一个反转在下一层再说一遍：样式自己的 `statusBarBrightness`
  描述的是它**期待的背景**，所以 light 样式期待一条深色的条——
  一个把这个字段当成"这是哪个样式"来读的测试，会整个反过来而且照样通过。
- **没指定颜色的应用，用主题的主色被认出来**（`widget.color ?? primaryColor`）。
  这是交给操作系统放进任务切换器的那个颜色，所以回退到主题而不是常量。
- **光标是主色，选区是它的五分之一**——一个颜色一个数字，
  主题换了主色两个一起动，上游把它们写成同一个 widget 的两行也是这个道理。
- **本地化 delegate 的顺序**：应用自己的在**前**，框架的**追加在后**。
  上游注释写明了为什么："只有每个类型的第一个 delegate 会被加载，
  所以 localizationsDelegates 参数可以用来覆盖 _CupertinoLocalizationsDelegate"。
  把框架的放前面照样编译、照样加载，只是悄悄让那个参数失效。
  顺带补上 `DefaultCupertinoLocalizationsDelegate` 本身
  （照 `DefaultWidgetsLocalizationsDelegate` 的样子，
  `isSupported` 是 `languageCode == 'en'`——默认 bundle 是美式英语并且直说，
  而不是认领所有 locale 然后一律答英语）。

### 一条写完又删掉的规则

第一版还写了 `CupertinoApp::effective_brightness`——
"主题说了算，没说就问平台"。写完发现
`CupertinoThemeData::brightness_of` **早就是这条**（上游的
`CupertinoTheme.brightnessOf`），我等于用第二个名字抄了第二遍。
删掉，改成在 `overlay_style` 的文档里指过去，
并把那条测试改成"这个问题是主题的，不是应用的"。
同名两份会漂，不同名两份漂了还看不出来。

变异扫描 9 个，全红。扫描后按上一轮的新习惯核对了树
（跑测试 + grep 这一轮的常量还在）。
尺子：十七把全部 exit 0。门：Rust 6772 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：`CupertinoApp` 还差上游那条 `assert`——
`home`/`routes`/`onGenerateRoute` 与 `routerDelegate`/`routerConfig`
不能同时给（`MaterialApp` 这边已经有 `router_is_configured`，
但**没有**那条"两套导航二选一"的断言本身）。
**先查一件事**：上游那条断言在 `WidgetsApp` 的构造函数里
（`assert(routerDelegate != null || routerConfig != null || ...)`）
还是在两个 App 各自的构造函数里——
如果在 `WidgetsApp`，那这一条该补在 `widgets_app.rs`，
两个 App 都受益，而不是抄两遍。

---

## 第 475 轮：一个应用走哪条导航的路，以及那条路禁止什么

上一轮留的问题查清了：那些断言在**`WidgetsApp` 的两个构造函数**里，
不在 `MaterialApp`/`CupertinoApp` 各自的构造函数里。
所以补在 `widgets_app.rs`，两个 App 一起受益，不用抄两遍。

补了两处。

**一、导航那条断言的另一半，这个 port 一直缺着。**
上游那条是：要么有 `home`/`routes`/`onGenerateRoute`/`onUnknownRoute` 之一，
**要么**有 `builder` 且 `navigatorKey`、`initialRoute`、`navigatorObservers`
统统还是初值。这个 crate 里只写了前半句（没路由就必须有 builder），
后半句没有——于是一个"只有 builder，却还带着 navigatorKey"的应用
在这里能通过，在上游会被拒绝。
它描述的是一个自己根本拿不到的 navigator，而后面不会有任何东西提醒他。

**二、`WidgetsApp.router` 的三条断言，一条都没有。**
它们其实是同一件事的三面：**谁来路由，说一次**。
`routerConfig` 是把四样东西打包成一个对象，所以它旁边再给任何一样都是歧义
（不是合并——没有任何地方说过谁赢）；没有它就必须有 `routerDelegate`，
否则没人造得出页面；而给了 `routeInformationProvider` 却不给 parser，
是一条没人读得懂的路由信息流。

顺带把 `_usesRouter` 挪到了这里（`RouterConfiguration::is_configured`）——
`MaterialApp` 和 `CupertinoApp` 各自用自己的名字问同一个问题，
而问题属于 router 那套配置。

测试里有一条特意写成"**只一个方向**"的：parser 没有 provider 是允许的，
上游那条断言只管一个方向。写反了会多拒一批合法应用，
而这种多拒往往没人发现——因为没人会为"本该通过"写测试。

变异扫描 9 个，全红。扫描后核对了树。
尺子：十七把全部 exit 0。门：Rust 6777 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：`MaterialApp`/`CupertinoApp` 现在各自还留着
`has_router_delegate`/`has_router_config` 两个布尔和一个
`router_is_configured`，和这一轮的 `RouterConfiguration` 是同一件事的两份说法。
**先查一件事**：上游的 `MaterialApp.router` 构造函数是**自己**又写了一遍
那三条断言，还是直接把参数转给 `WidgetsApp.router` 让它去断言——
如果是后者，这两个 App 就该改成持有一个 `RouterConfiguration` 而不是两个布尔；
如果上游真的各写各的，那这份重复是照抄上游，得在注释里说清楚它为什么在。

---

## 第 476 轮：两个 App 转发的那五个参数，之前只模型了两个

上一轮留的问题查清了，而且答案是"两边都有一点"：
`MaterialApp.router` 和 `CupertinoApp.router` **各自都写了一条**断言——
`assert(routerDelegate != null || routerConfig != null)`——
然后把五个参数**原样交给** `WidgetsApp.router`，由它断言其余三条。
所以那份重复是上游的，不是这个 port 的；两个 App 的
`router_is_configured` 文档里本来就写着它是"upstream's only constructor
assert"，这一点原先就对。

真正的缺口在别处：两个 App 各拿着 `has_router_delegate` /
`has_router_config` **两个**布尔，而上游拿的是**五个**参数。
少掉的三个正是上一轮那三条断言所**关于**的东西——
于是"provider 给了却没给 parser"这种应用，在这个 port 里连表达都表达不出来，
更谈不上被拒。改成两个 App 各持一个 `RouterConfiguration`，
`router_is_configured` 转给它的 `is_configured`。

两条测试把**分层**按住，因为单看任何一端都像是漏了：
一个 app 可以通过自己构造函数那条断言，随即被 `WidgetsApp.router` 拒掉
（给了 `routerConfig` 又给 `routerDelegate`：配置本来就带着 delegate），
**这不是任何一边的 bug**。反过来，`provider` 没有 `parser` 的应用
"自己那条"也照样通过——弱的那条只管它管得着的。

变异扫描 4 个，全红。扫描后核对了树。
尺子：十七把全部 exit 0。门：Rust 6779 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。
`CupertinoApp`（0.22，8/37）也从 `depth.py` 队头下去了。

**下一步**：队头现在是 `MaterialLocalizations`（36/158）——
上游那 158 个成员里绝大多数是 `DefaultMaterialLocalizations` 的字串与格式化。
**先查一件事**：这个 crate 的 `MaterialLocalizations` trait 有 36 个成员，
而 `DefaultMaterialLocalizations` 上还挂着多少**没有进 trait**的常量
（第 472 轮在 Cupertino 那边遇到的正是这个形状：值都在，接口只有一半）。
数一下"常量有、trait 没有"的那批，就知道这一轮是补接口还是补字串。

---

## 第 477 轮：Material 那半个接口，另一半补上

上一轮留的问题数完了，答案和第 472 轮在 Cupertino 那边遇到的是同一个形状，
而且更大：`DefaultMaterialLocalizations` 上有 **10 个常量**和 **9 个方法**
是上游 `MaterialLocalizations` 的成员，却**一个都不在这个 crate 的 trait 上**。

十个词是文本选择工具条那一排——剪切、复制、粘贴、全选、查找、网页搜索、
分享、扫描文字——加上展开磁贴的两条点击提示。
九个方法是数字分组、小时与分钟、紧凑日期、时间格式、星期缩写、
标签页朗读、表格的"第几行到第几行"和"选中了几项"。

从实现那一侧看，什么也不缺：每个字串都在、都有测试、都被用着。
缺的是**它们够不着**——一个应用没法把自己的措辞或自己的数字分组摆到前面，
而那正是接口存在的唯一理由。

补完之后 trait 从 36 个成员变成 55 个。
`Shouty`（那个测试用的自备 bundle）**当场编译不过**，
这正是上游用 `implements` 而不是 `extends` 想要的效果：
接口多一个成员，实现方要响亮地坏掉，而不是安静地继承到一个错答案。

测试按住两件事：一个应用能换掉词（`cut` → `CUT`），
也能换掉**规则**——`format_decimal(1234567)` 在框架是 `1,234,567`，
在一个按自己方式分组的 bundle 里是 `1234567`。
另一条把"接口答的就是常量"逐条对过去（十个词加九条规则），
包括两个会答"不能"的：这个 bundle 不会说的时间格式，和没人念得出的标签页序号。

变异扫描 9 个，全红。扫描后核对了树。
尺子：十七把全部 exit 0。门：Rust 6781 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。
`MaterialLocalizations`（0.23，36/158）也从 `depth.py` 队头下去了——
队头现在只剩两张图标表，和 `MagnifierController`（2/8）、`Route`（6/24）。

**下一步**：`Route`（`widgets/navigator.dart`，6/24）。
**先查一件事**：上游 `Route` 那 24 个成员里，有多少是这个 crate 已经
在别的名字下做过的（`routes.rs` 里的 `ModalRoute`/`PageRoute` 相关规则，
以及 `theatre.rs` 的 `ModalHandle` 那套 dismiss/上下场），
有多少是 `Route` 自己的生命周期（`install`、`didPush`、`didPop`、
`didComplete`、`willDisposeAfterTransition`、`restorationScopeId`）——
先分清"已在别处"与"确实没有"，再决定这一轮补哪一段。

---

## 第 478 轮：一条路由站在历史里的哪个位置

上一轮留的问题先分清楚。上游 `Route` 那 24 个成员在这个 crate 里散在两处：

- **已经有的**：`install`、`did_push`、`did_pop`、`did_pop_next`、`dispose`、
  `will_handle_pop_internally`、`pop_disposition`——都在 `routes.rs` 的
  `OverlayRoute`/`TransitionRoute`/`ModalRoute` 上。
- **不是同一个东西**：`navigation.rs` 的 `Route` 是"名字 + 参数"，
  对应的其实是上游的 `RouteSettings`，depth 把它当成 `Route` 来数才得出 6/24。
- **确实没有的**：`didAdd`、`didReplace`、`didComplete`、`didChangeNext`、
  `didChangePrevious`、`changedInternalState`、`changedExternalState`，
  以及**位置那四问**——`isCurrent`、`isFirst`、`isActive`、`hasActiveRouteBelow`。

这一轮做位置那四问，连同它们赖以成立的 `_RouteLifecycle`。

### 顺序就是那个类型

`_RouteLifecycle` 的每一个问题都是这串变体上的一个**区间**，
所以变体插错位置会一次悄悄挪动好几个答案。照上游原样搬，连分段注释一起。

而这里有一处值得单独写下来：**`isPresent` 是 `add..=remove`，
它越过了那条写着 `// routes that are not present:` 的注释三个变体**。
一条正在 pop、正在 complete、正在 remove 的路由**仍然是 present**——
它还在屏幕上，还是"哪条是 current"的答案。
那条注释说的是 `willBePresent`（到 `idle` 为止）；
把它当成 `isPresent` 的边界，是这一段最容易犯的错，所以测试专门按住它。

### 四问都是关于**历史**的，不是关于路由的

所以它们是一个 `RoutePosition`（一串 `HistoryEntry`）上的四个方法，
而不是路由身上四个需要各自保持同步的布尔——上游每次被问都现场走一遍
`_navigator!._history`，是同一个道理。

三处细节测试各按一条：
current 是**最后一个 present 的**（不是最后一个——正在 popping 的还在列表里）；
`isActive` 取的是这条路由的**第一个**条目问它 present 不 present
（不是"任意一个条目"——两者只在一条路由在历史里出现两次时不同）；
`hasActiveRouteBelow` 的遍历**停在自己这一条**，这才使它是"下面"而不是"别处"。
还有一条按住"没装上就不在任何地方"：三个方法都以
`if (!_installed) return false` 开头，用 `false` 说"它不在那儿"，
而不是引入第三个值。

变异扫描 10 个，全红。扫描后核对了树。
尺子：十七把全部 exit 0。门：Rust 6787 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：上面列的"确实没有的"里还剩七个回调——
`didAdd`、`didReplace`、`didComplete`、`didChangeNext`、`didChangePrevious`、
`changedInternalState`、`changedExternalState`。
**先查一件事**：它们在上游 `Route` 上大多是空实现（`{}`），
真正有内容的只有 `didComplete`（完成 `_popCompleter`）和
`didAdd`（等一个 `TickerFuture` 再要焦点）——
先确认哪几个在 `ModalRoute`/`TransitionRoute` 的**子类**里才有真实现，
只搬空方法是搬了个签名，那种"补了却什么也没说"的东西这个项目不要。

---

## 第 479 轮：邻居变了，这条路由要做什么

上一轮留的问题先查：那七个回调在 `Route` 上确实大多是 `{}`，
**但真正的内容在子类里**，而且不少：

- `TransitionRoute.didReplace` —— 把旧路由的动画值接过来。
- `TransitionRoute.didChangeNext` —— `_updateSecondaryAnimation(nextRoute)`。
- `ModalRoute.didChangeNext` —— 算 `receivedTransition`。
- `ModalRoute.didChangePrevious` —— 整个方法体就是 `changedInternalState()`。
- `ModalRoute.changedInternalState` / `changedExternalState` —— 各自标脏什么。

所以不是"搬七个空方法"，是搬这五条规则。

**替换的那条最有意思**：一条路由替掉一条开到 0.4 的路由时，
它从 0.4 开始，不是从头。从头会把读者已经看过大半的入场动画重演一遍，
两块屏幕交叉两次。旧路由不是 `TransitionRoute` 时没有值可接，
这条路由保留自己的——这就是那个 `if (oldRoute is TransitionRoute)`。

**`didChangeNext` 的三个条件里，第三个值得停一下**：
上面那条路由如果 delegate 的是**同一个** transition，就什么也不往下传。
照传的话，这块屏幕会把同一个 transition 演两遍——一遍因为它自己有，
一遍因为别人给了它——这种重影从截图里读不回来。

**两个 `changed*State` 的区别，和那个守卫的位置**：
内部变化只标脏 barrier，而且**只在树没锁的时候**（构建期间不许标脏）；
但 `maintainState` 是**照推不误**的，因为那是赋一个值而不是请求一次重建——
所以上游把守卫写在方法**里面**而不是调用处。外部变化则连页面一起重建：
navigator 自己变了（比如上面换了个 `MaterialApp`），
页面依着旧状态建出来的东西已经过期，只标 barrier 会把旧页面留在屏幕上。

变异扫描 10 个，全红——其中两个专门按住上面那两处：
"树锁着也标脏"和"树锁着连 scope 也不告诉"，两个方向都红。
扫描后核对了树。
尺子：十七把全部 exit 0。门：Rust 6792 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：`Route` 那批里还剩 `didAdd`、`didComplete` 两个有内容的。
`didComplete` 是 `_popCompleter.complete(result ?? currentResult)`——
**`??` 那一半是重点**：一条被关掉却没给结果的路由，交出的是它自己的
`currentResult`，而不是空。
**先查一件事**：这个 crate 里 `pop` 的结果走的是哪条路
（`navigation.rs` 的栈？还是 `theatre.rs` 的 `ModalHandle`？），
`currentResult` 在上游 `ModalRoute`/`PopupRoute` 上又是什么——
确认了再决定这条规则挂在哪个类型上，别又造出第二份说法。

---

## 第 480 轮：一条路由走的时候交出什么

上一轮留的问题查清了，两件事：

- **上游 `currentResult` 在框架里没有任何子类重写**（`Route` 上是 `null`，
  `lib/src` 全文只有这一处）。它是留给应用的扩展点：
  一条有"当前选中项"的路由把它设上，之后无论怎么关，答的都是那一项。
- **这个 crate 里根本没有结果这条通道**：`navigation::pop()` 返回 bool，
  结果直接丢掉；`routes.rs` 只模型了 `did_pop` 的"这条路由 pop 了没有"，
  没有"它完成时带出什么值"。

所以这一轮补的是 `popped` / `currentResult` / `didComplete` 三个成员合起来
的那一件事。两条规则值得写下来：

**一、`result ?? currentResult`——`??` 就是全部。**
一条没给结果就被关掉的路由，交出的不是空，而是它自己的 `currentResult`。
这正是"点遮罩关掉的对话框答 null"和"答刚才选中的那一项"之间的差别。

**二、`popped` 只完成一次。**上游的 `Completer` 被完成两次会抛，
而 navigator 有两个调用点——`didPop` 和 `pushReplacement`——
所以"第一次的算数"是一条规则而不是巧合。
这里第二次调用是**谢绝**而不是致命，和 `ModalHandle::dismiss` 的立场一致：
第二次是调用者该在测试里发现的错，不是在读者面前把应用关掉的理由。

类型上一处小决定：`popped()` 返回 `Option<Option<&str>>`，**不摊平**。
外层是"完成了没有"，内层是"交出东西了没有"，两个不同的问题；
摊成一个 `Option` 会让"完成了但没东西可说"和"还没完成"变成同一个答案。
一条测试专门按住这一点。

变异扫描 5 个，全红。扫描后核对了树。
尺子：十七把全部 exit 0。门：Rust 6796 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：`RouteCompletion` 现在是个独立的类型，**还没有接到任何路由上**——
`ModalRoute`/`TransitionRoute` 都不持有它，`navigation::pop()` 还是丢结果。
这正是这几十轮反复挑的那个形状（规则写好了、没有真调用者），所以下一轮接它。
**先查一件事**：上游 `didComplete` 的两个调用点里，
`pushReplacement` 那条是在**被替换的路由**上调的（不是新的），
而且是在 `didReplace` 之前还是之后——顺序决定被替换的路由
交出的是替换前还是替换后的 `currentResult`。

---

## 第 481 轮：把结果接到搬运它的那个条目上

上一轮留的问题查出来的答案，比问题本身更有用：
上游**不是**在替换的地方调 `didComplete`，而是走**条目的生命周期**——
`_RouteEntry.handlePop` 里由 `didPop` 顺带完成，
或者进入 `complete` 状态后由 `handleComplete` 完成。
两条路都紧接着一句 `pendingResult = null`。

也就是说，值**不在路由身上，在条目身上**（`_RouteEntry.pendingResult`）：
一条路由可以被 transition delegate 早早标记为待 pop，
而真正问它 `didPop` 是后来的事，值得在中间等着。
所以这一轮把上一轮那个 `RouteCompletion` 接到 `HistoryEntry` 上，
并搬了 `handlePop` 与 `handleComplete` 两条规则。

`handlePop` 里三处值得慢读：

- **状态先变成 `popping`，然后才问路由**；路由拒绝了再退回 `idle`。
  只在成功时设状态看着等价，其实不是：一条自己消化了 pop 的路由会重建，
  而它在那一刻读到的自己是 `popping`。
- **已经完成的路由被放过**，上游注释点名了那个场合，并且断言此时没有待交的值。
  这一支**什么也不从条目里拿走**，"no further action" 就是这个意思。
- **交出去之后把待交的值清掉**，于是同一个值递不了第二次。
  加上 `RouteCompletion` 谢绝第二次完成，同一扇门上两把锁——上游也是两把。

### 一条不可观察的注释，被改成可观察的

第一版 `handle_pop` 收的是 `route_popped: bool`，
于是"先设状态再问路由"这条**变异掉也不红**——先设还是后设，
外面看到的最终状态一样，而"问的时候是什么状态"根本没人能看见。
改成让它收一个闭包（那正是 `Route.didPop` 的位置），
把状态**交给**那个闭包，规则就有了唯一的观察点，测试也就立得住。

同一轮还有两个变异照出两处：`RouteLifecycle::default()` 没人测过
（默认是 `idle`——"route is being harmless"，而不是 `staging`），
以及"已完成"那一支没有测过它**不动**待交的值。都补上了。

变异扫描 8 个，全红。扫描后核对了树。
尺子：十七把全部 exit 0。门：Rust 6803 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：`HistoryEntry` 现在会搬结果了，但 `navigation.rs` 的那个栈
仍然自己 pop、自己丢结果，两边没有接上——
`navigation::Navigator` 用的是 `Vec<StackEntry>`，
而 `routes.rs` 这套是 `RoutePosition`/`HistoryEntry`。
**先查一件事**：这两个栈在上游是**同一个** `_history`，
还是这个 port 有意分成"给动画看的栈"和"给规则看的历史"两份——
读 `navigation.rs` 顶部的模块注释，它多半已经说过为什么是两份；
是有意的就把接口对上，不是的话就该并成一份。

---

## 第 482 轮：那个真在跑的栈，终于会把答案带回来

上一轮留的问题读完了模块注释，答案是**两份是有意的**：
`navigation.rs` 是一个真在跑的栈（push/pop/replace/转场/返回手势，
gallery 用的就是它），`Route` 在那里是"名字 + 无类型参数"；
`routes.rs` 则是上游那一族 route 类的**规则模型**。
分开没错，但两边一直没接上——而缺的那一块正是上一轮做好的：
`pop()` 返回一个 bool，值就没了。

这一轮把它接上：

- 条目上多了一个 `RouteCompletion`（值在**条目**上，不在 `Route` 上——
  `Route` 是应用交进来又自己留着的东西，而结果是**栈**在送出去的路上交付的）。
- `pop_with_result(result)` 交出值并**返回**它；`push_expecting(...)` 给
  这条路由一个"没选就交这个"的 `currentResult`。
- `replace` 与 `pop_to_root` 也各自完成被拿走的路由——
  被替换、被埋掉的屏幕同样有人在等它的答案，不完成就是一场不会结束的等待。

### 值必须**返回**，因为条目未必活到调用结束

第一版把答案只留在条目上，靠 `result_at(depth)` 去读。
测试当场三条红：一次**没有转场**的 pop 会在同一句里把条目丢掉
（`begin` 遇到 `Transition::None` 立刻清空 outgoing），
于是答案在唯一有人要它的那一刻消失。
改成 `pop_with_result` 直接把值交回来——上游那边是调用者 await 一个 future
拿到值，这里是同一次交接，只是没有 future。
`result_at` 留着，管的是**动画还在跑**的那段：调用者往往在 pop 的同一口气里问，
那正是转场进行中。为此 `outgoing` 从 `Option<Route>` 改成 `Option<Entry>`——
只留路由就等于把答案丢在最有人要的那一刻。

### 一个变异逼出的真缺口

"埋掉的屏幕没被完成"这条变异不红，查下去发现两件事：
一是那些条目被 `truncate` 掉、根本没人完成过（真缺口）；
二是就算补上，答案也**观察不到**，因为条目已经没了。
两件事一起解决：`pop_to_root` 现在返回它拿走的每一块屏幕的答案，
自下而上排列。补完之后"顺序反了"这条变异还差一个观察点——
夹具里只有一块被埋的屏幕，反过来也一样——加到两块才立住。

变异扫描 8 个，全红。扫描后核对了树。
尺子：十七把全部 exit 0。门：Rust 6808 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：`pop_to_root` 现在会完成被埋的屏幕，
但 `replace` 只完成**被它替掉的那一个**——而上游 `pushReplacement`
还有一个 `removeRoute`/`removeRouteBelow` 家族，
它们拿走的是**中间**的路由。
**先查一件事**：这个栈有没有"拿走中间某一条"的操作
（`navigation.rs` 里似乎只有 push/pop/replace/pop_to_root）；
没有的话，这一轮的规则就已经覆盖了它能做的全部动作，
下一轮该回 `depth.py` 队头，而不是给这个栈加一个上游有、这里没人要的操作。

---

## 第 483 轮：正在退场的放大镜，已经不算"显示中"

上一轮留的问题先查：`navigation.rs` 的栈只有
push / push_expecting / pop / pop_with_result / replace / pop_to_root，
**没有"抽掉中间某一条"**。所以上一轮那几条规则已经覆盖了它能做的全部动作，
不该为了对齐上游的 `removeRoute` 家族给它加一个没人要的操作。回队头。

队头（两张图标表之外）是 `MagnifierController`（2/8）。
一看就知道那个 2/8 大半是**映射错位**：这个 crate 把上游控制器的
overlay 生命周期放在 `MagnifierHost` 里，`magnifier.rs` 顶上的注释早写明了。
逐个对下来，`shown`/`show`/`hide`/`removeFromOverlay`/`overlayEntry` 都在，
真正缺的只有一个：**`shown` 的后半句**。

```dart
bool get shown =>
    overlayEntry != null && (animationController?.isForwardOrCompleted ?? true);
```

这个 port 只实现了前半句。补上之后，"不显示"有三种各不相同的方式：
条目被拿走了；平台说这一次要藏起来（条目还挂着）；
以及**正在退场**——还在屏幕上、还在淡出，但已经不算显示中。
`isForwardOrCompleted` 里 `reverse` 和 `dismissed` 都是假，
所以问"我还能用它吗"的调用者从它**开始**离开的那一刻就被告知不能，
而不是等它走完。

`?? true` 那一半单独值得一条测试：没有入场动画的放大镜，一存在就是显示中。
把缺失的控制器读成"不显示"，会把大多数（根本不做动画的）放大镜全藏掉。

`hide` 也跟着改：上游 `hide` 里 await 的是 `animationController?.reverse()`，
所以有动画的 host 在**这里**掉头，而不是在条目被拿走的时候——
`removeFromOverlay: false` 那条路上，第二个时刻根本不会来。
没有动画的 host 则保持没有：无中生有一个状态，会让 `?? true` 那条路永远走不到。

变异扫描 6 个，全红（"没有动画就永不显示"一条打红 5 个测试，
说明这条路径是许多测试的地基）。扫描后核对了树。
尺子：十七把全部 exit 0。门：Rust 6810 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：队头下一个是 `TextSelectionGestureDetectorBuilder`（7/27）。
**先查一件事**：它那 27 个成员里绝大多数是
`onTapDown`/`onSingleTapUp`/`onDragSelectionStart`… 这一串手势回调，
而这个 crate 的 `text_selection.rs` 里已经有一套选择手势的规则——
先数清楚"同名的有几个、名字不同但做同一件事的有几个"，
免得又照着上游的名字造一份和现有规则重复的东西
（第 474 轮那条被删掉的 `effective_brightness` 就是这么来的）。

---

## 第 484 轮：shift 是在手势**开始**时问一次的

上一轮留的问题数完了，结果很干脆：上游
`TextSelectionGestureDetectorBuilder` 的 17 个手势回调，
**和这个 crate 现有的名字零重合**——一个都不叫 `on_*`——
但**规则全都在**，只是叫 `single_tap_up`、`double_tap_down`、`multi_tap_down`、
`long_press_start/move_update/end/finish`、`drag_selection_start/update/end`、
`force_press_start/end`、`secondary_tap`、`shift_tap_down`、`shift_drag_update`。
换句话说，7/27 又是一次**映射错位**（和上一轮的 `MagnifierController` 同类）：
这个 crate 把它们建模成规则函数，而不是一个 builder 上的回调。

逐个对完，真正缺的是一对小回调 `onTapTrackStart` / `onTapTrackReset`，
以及它们维护的那个 `_isShiftPressed`。

而它恰恰是这一整套 shift 规则的**来源**：
`shift_tap_down`、`shift_drag_update`、`shift_is_usable` 全都收一个
`shift_pressed` 而不自己决定它。这一轮补的就是那个决定：
**键盘只在 tap track 开始时被问一次**，之后整个双击、三击序列都用那一次的答案。

两个后果都得写对，因为两边看起来都像 bug：

- 序列**开始之后**才按下 shift 的，不会得到 shift-扩选——他开始的是一次普通序列。
- 中途**松开** shift 的，仍然会——他开始的是一次 shift 序列，
  在第二下和第三下之间改主意，会选中他从没要过的东西。

哪一边都可以争；不能争的是**每一下都去读一次键盘、同一个手势里给出不同答案**。

`onTapTrackReset` 是清成 false 而不是"再问一次"：
再问一次会让一个从没松手的读者把 shift 一直带到下一个无关的点击上去。

变异扫描 4 个，全红。扫描后核对了树。
尺子：十七把全部 exit 0。门：Rust 6813 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：连着两轮队头都是**映射错位**而不是真缺口
（`MagnifierController` 2/8、`TextSelectionGestureDetectorBuilder` 7/27），
两次都靠人工逐个比对才发现。
**先查一件事**：`tools/depth.py` 的映射表是怎么记的
（`coverage.py` 那边有 `mapped`/`mapped-concept` 的分类），
能不能让 depth 也接受一条"这个类的成员在别处，按那边数"的记法——
如果能，这两条就该记进去，下一轮的队头才是真的队头，
而不是再花一轮去发现"其实早就做了"。

---

## 第 485 轮：把读过的记下来，队头才是真的队头

上一轮留的问题，答案是"**这个机制早就有**"：`depth.py` 一直读
`tools/depth_examined.json`，它自己的说明写得很清楚——

> 一行进这个文件的前提是，有人逐个成员比对过，并且能说清楚为什么这个缺口不是缺口。
> `finding` 就是那个理由，而且必须**可核对**：要点名是什么机制回答了那些缺失的成员，
> 不能只写"看过了，没问题"。

所以问题不是"能不能记"，是**我前面几轮读完了却没记**。这一轮把它们补上，五条：

- **`Route`**（0.25，6/24）——映射差异加四轮真活。depth 把上游的 `Route`
  配到了 `navigation::Route`，而后者是"名字 + 参数"，对应的是上游的
  `RouteSettings`；真正被问的那个生命周期对象在 `routes.rs`。
  第 478–482 轮补掉的是位置四问、五个 `did*` 回调、以及 popped/currentResult/didComplete。
- **`MagnifierController`**（0.25，2/8）——overlay 那一半住在 `MagnifierHost`，
  `magnifier.rs` 的模块注释早就写明了；第 483 轮读出并补掉的真缺口是 `shown` 的后半句。
- **`TextSelectionGestureDetectorBuilder`**（0.26，7/27）——十七个回调**零个同名**、
  规则**全都在**，只是建模成规则函数而不是 builder 上的回调；
  第 484 轮补掉的是 `_isShiftPressed` 的采样时机。
- **`Icons`**（0.00，22/8826）与 **`CupertinoIcons`**（0.01，8/1324）——
  生成的码点表。不抄的决定写在 `icons.rs` 的模块注释里，
  数量记成了 `UPSTREAM_ICON_COUNT` 常量；这两条不记下来就会**永远**占着队头，
  而唯一能"关掉"它们的办法是把生成块粘过来——那只会移动比值，别的什么也不会变。

记完之后队头第一次是干净的：`ReorderableList`（0.28，8/29）、
`TextSelectionOverlay`（0.28，7/25）。

### 顺手修掉一处：这把尺子读不出自己最老的记录

`--examined` 跑到 tick 184 那行就崩了——`at` 这个字段是后来才加的，
早期几行没有，而打印器直接 `row["at"]`。
一把在自己最老的条目上崩掉的尺子，就是一把没人会跑的尺子，
而这个打印器是那些"读过了"唯一能被读回来的地方。改成有才打印。

### 这一轮的变异扫描跑的是尺子，不是 cargo

六个变异，全部改变了尺子的输出：把 `at` 的容错去掉 → `--examined` 崩；
把这一轮记下的五行**逐条删掉** → 那个类**当场回到队头**。
最后一条正是这五行的意义所在：它们不是压制列表，是"读过了，理由在这里"。

尺子：十七把全部 exit 0。门：Rust 6813 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；三个目录 default 与
`rustflutter_engine` 都 exit 0。

**下一步**：队头 `ReorderableList`（8/29）。
**先查一件事**：上游那 29 个成员里，`ReorderableList` 与
`ReorderableListState` 是两个类，而 depth 只数前者——
先分清哪些成员在 State 上（`startItemDragReorder`、`cancelReorder`…）、
哪些是构造参数（`itemBuilder`、`onReorder`、`proxyDecorator`…），
再看这个 crate 的 `reorderable_list.rs` 把它们放在了哪里；
前几轮反复出现的教训是：**先分清"在别处"与"确实没有"**，
不然又要花一轮去发现"其实早就做了"。

---

## 第 486 轮：同一条断言，写在上游写它的那一层

队头第一次是真的：`ReorderableList` 8/29。按上一轮说的先分清成员归属，
那 29 个里绝大多数是**构造参数**，而 `startItemDragReorder` / `cancelReorder`
在 `ReorderableListState` 上——那两个这个 crate 早就有，转发给 sliver 的状态，
和上游一模一样。真正没有的，是构造函数里的那两条断言。

上游把它们**逐字写了三遍**：`ReorderableList`、`SliverReorderableList`、
`ReorderableListView` 的构造函数各一份。

```dart
assert(
  (itemExtent == null && prototypeItem == null) ||
      (itemExtent == null && itemExtentBuilder == null) ||
      (prototypeItem == null && itemExtentBuilder == null), ...);
assert(
  (onReorderItem != null && onReorder == null) ||
      (onReorderItem == null && onReorder != null), ...);
```

这个 crate 只在 Material 那一层（`ReorderableListView::validate`）有，
widgets 层的两个类**完全不检查**——而 `reorder_report` 恰恰收一个
`has_on_reorder: bool`，也就是说"两个回调只能有一个"是它的**前提**，
却没有任何地方验证过这个前提。

### 但不能抄第三遍

前面几轮记下过一条：**一串东西抄两份，就是两个会各自漂移的东西**。
所以这一轮不是把断言复制到 sliver 上，而是把它**问一次**：
新的 `ReorderableConfig` 持有五个字段（两个回调、三个 extent 来源），
三个类都拿着它；`ReorderableListView` 原来的五个字段被它换掉，
`ReorderableListViewError` 改名 `ReorderableError`，一个枚举管三个类。

有一处细节决定了它不能只提供一个 `validate()`：
Material 那层在两条断言**中间**还夹了一条 `children.every((w) => w.key != null)`。
所以 `ReorderableConfig` 把两半也单独暴露出来
（`check_extent_sources` / `check_callbacks`），
Material 的 `validate` 按上游顺序把 key 那条夹在中间。
顺序不是摆设：**一个同时犯了两个错的列表，被告知的是哪一个，由它决定**——
两个 extent 来源 + 无 key 的孩子，报的是 extent。

`itemCount >= 0` 那条**没有写**：`usize` 已经回答了它。
（"没有可观察差别的规则不该写下来"，第 466/474/483 轮反复学到的。）

### 变异扫描 7 个，全红，每个都点得出是哪条测试抓住的

`given > 1` → `> 2`（5 红）；两个回调各自容忍（2 红 / 3 红）；
config 的两条断言**换序**（1 红）；Material 把 key 提到最前（1 红）；
外层 `ReorderableList::validate` 不问 sliver 直接 `Ok(())`（1 红）；
新 config 默认两个回调都没有（5 红）。扫描后核对了树，五条新断言都在。

顺手修了扫描脚本自己的一个读数错误：`cargo test` 失败时会打印
`error: test failed`，脚本把它当成了"编译不过"。
**一个变异是没被测到还是没编译过，是两件不同的事**，不能混。

尺子：十七把全部 exit 0。门：Rust 6818 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6818 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：同一个文件里还剩一条真规则——
`ReorderableListState._effectiveScrollCacheExtent`：
`scrollCacheExtent` 优先，否则把**已废弃**的 `cacheExtent`（一个 double）
当作 `ScrollCacheExtent.pixels` 解释，都没有就是 null。
`scrolling.rs` 里 `ScrollCacheExtent` 和它的 `pixels` 构造子已经有了，
所以这条是接得上的；**先查一件事**：这个 crate 里还有谁读 cache extent
（`CustomScrollView` / `Viewport` 那条链），
如果那条链上已经有一处做同样的"两个字段选一个"的解释，
就该让它们共用，而不是在 reorderable 这边再写一遍——和这一轮同样的道理。

---

## 第 487 轮：两种问法要一个缓存范围，谁说了算

接上一轮的"下一步"。先按它说的查了一件事——**上游自己就有三份**：

| 位置 | 弃用的 double 按什么单位读 |
| --- | --- |
| `Viewport` / `ShrinkWrappingViewport`（`widgets/viewport.dart`，两份） | 看该 widget 的 `cacheExtentStyle` |
| `ScrollView.buildViewport` | **写死 pixels** |
| `ReorderableListState`（上一轮那个文件） | **写死 pixels** |

后两处写死 pixels 不是省事，是**它们根本没有 `cacheExtentStyle` 这个参数**：
一个不带单位到达的 double，在那里就是像素。

这个 crate 这边：`ScrollCacheExtent`、`CacheExtentStyle`、`in_pixels`、
`is_legal`、`defaulted` 都在 `scrolling.rs`，
而 `ShrinkWrappingViewport` 只有一个 `cache_extent: Option<f32>`
——**弃用的那半留着，新的那半没有，也没人做这个二选一的解释**。

所以这一轮补的是 `ScrollCacheExtent::effective`，一处，三个调用点各自带上自己的单位：
`ShrinkWrappingViewport`（新增 `scroll_cache_extent` 与 `cache_extent_style`，
读自己的 style）、`ReorderableList`（新增两个字段，写死 Pixel）。

### 最容易写错的是它的返回值

`None` 的意思是 **"调用者什么都没说"**，不是"用默认值"。
widget 层把这份沉默原样传下去，`DEFAULT_CACHE_EXTENT` 是**渲染层**才发生的事
——那是 `defaulted` 回答的另一个问题。两者签名相近、含义相反，
所以测试里把它们并排断言了一次：同样是"没给"，
`effective` 给 `None`，`defaulted` 给 250 像素。

还有一点在测试里钉住了：`scroll_cache_extent` 赢的时候，
**单位跟着赢的那个字段走**，而不是跟着这个 widget 的 style。
一个 `ReorderableList`（写死 pixels）拿到
`ScrollCacheExtent::viewport(0.5)`，在 800 的视口里就是 400 像素。

### 变异 6 个，全红

弃用的 double 反过来压过新字段（2 红）；double 被忽略（2 红）；
`effective` 里丢掉 `style` 一律按 pixels（1 红，正是 viewport 那条）；
viewport 不读自己的 style（1 红）；新建的 viewport 默认按 viewport 计（1 红）；
reorderable 那边把写死的 Pixel 改成 Viewport（1 红）。扫描后核对了树，四条新测试都在。

尺子：十七把全部 exit 0。门：Rust 6822 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6822 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`ReorderableList` 这一路已经走到"配置齐了"，
回到 depth 队头的第二名 `TextSelectionOverlay`（0.28，7/25）。
**先查一件事**（这两轮反复见效的那一步）：上游 `TextSelectionOverlay` 的成员里
有一大半是转发给 `SelectionOverlay` 的
（`showHandles`/`hideHandles`/`showToolbar`/`hideToolbar`/`update`/`dispose`…），
先分清哪些在 `SelectionOverlay` 上、这个 crate 的 `text_selection.rs` 与
`selection_overlay`（如果有）各自放了什么，
再决定是"在别处、记一笔"还是真缺口。

---

## 第 488 轮：这个 overlay 里有**两个**工具条

队头第二名 `TextSelectionOverlay`（0.28，7/25）。按这三轮固定的第一步先分归属，
结果又是一半映射：`showHandles`/`hideHandles`/`update`/`updateForScroll`/
`hide`/`magnifierIsVisible`/`magnifierExists` 都在这个 crate 的
`SelectionOverlay` 上，`_updateTextSelectionOverlayVisibilities` 那三行
（每个手柄看自己那端在不在视口里、工具条看两端有没有一端在）第 4xx 轮就补过，
`_getStartGlyphHeight`/`_getEndGlyphHeight` 是现成的 `glyph_heights`。

但这次分完之后，剩下的**不是零**。上游的 `SelectionOverlay` 里有
`_spellCheckToolbarController`——**第二个、完全独立的工具条**，
而这个 crate 只有一个 `toolbar_visible: bool`。
于是这一轮补的是"拼写建议菜单也是这个 overlay 的住户"，一条链上四处：

- `toolbarIsVisible` 的文档说得很直白："Includes both the text selection
  toolbar and the spell check menu"——它是 `||`，不是选择工具条本身；
  `spellCheckToolbarIsVisible` 才是"只问拼写菜单"。
- `hideToolbar` 两个 `remove()` 都**不加判断**，所以调用者从来不需要知道
  当时立着的是哪一个。
- `showMagnifier` 问的是 `toolbarIsVisible`——**所以在拼错的词上举起放大镜，
  建议菜单也会被收走**，哪怕选择工具条根本没出现过。
- `TextSelectionOverlay.showSpellCheckSuggestionsToolbar` 的最后一行是
  `hideHandles()`：**建议菜单指的是一个拼错的词，不是一段选区**，
  手柄留在那里就是在指一件读者已经不再被问的事。
  注意用的是"销毁"那个动词，`handlesVisible` 一动不动
  ——和第 4xx 轮记下的"两条轴"一致。

### 一处不对称，只有并排放才看得出来

选择工具条被包在 `_SelectionToolbarWrapper(visibility: toolbarVisible, ...)` 里，
建议菜单那一个**没有 `visibility:`**。
所以把选区滚出视口，选择工具条消失，建议菜单**留在原地**——
它本来就不是跟着选区走的。`OverlayVisibilities` 因此多了一个字段，
而且是唯一一个不与视口信号相乘的。

`hide()` 上游那句 `if (_toolbar != null || ...isShown) hideToolbar()` 没有照抄：
那串判断就是 `toolbarIsVisible` 问的同一串，而 `hideToolbar` 对着空气执行等于没执行。
**一个没有可观察差别的判断是一句注释**，就写成注释了。

### 变异 8 个，全红

宽问题各丢掉一半（2 红 / 1 红）；`hideToolbar` 只收一个（3 红）；
没有 context 也开菜单（1 红）；放大镜只问选择工具条（1 红）；
放大镜报告了却没真收（1 红）；给建议菜单也乘上视口信号（1 红）；
建议菜单不收手柄（1 红）。扫描后核对了树，七条新测试都在。

尺子：十七把全部 exit 0。门：Rust 6829 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6829 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`TextSelectionOverlay` 这一遍读下来，只剩两处没落地，先做前者——
`_getHandleDy` 与 `_handleSelectionEndHandleDragUpdate` 里的那段：
拖手柄时 **dy 被吸附到行**（`_getHandleDy` 拿 `dragDy` 和 `handleDy` 比，
让手柄一行一行地跳而不是连续滑），
以及 `_dragStartSelection` 记住起手时的选区、
用它决定拖动中**能不能越过另一个手柄**。
这个 crate 的 `TextSelectionOverlay` 现在只有 `drag_offset` 那一半
（抓点不跳），行吸附与越界规则都没有。
**先查一件事**：`editable.rs` 里是否已经有"某个 dy 属于哪一行"的函数
（`preferredLineHeight`/行盒那一带），有就复用，别在这边再算一遍。

---

## 第 489 轮：拖一个手柄越过另一个，两个平台给出相反的答案

接上一轮。`_handleSelection{Start,End}HandleDragUpdate` 读下来，
里面藏着一条**平台分歧**，而这个 crate 的实时路径（`editable.rs` 的
`drag_handle_to`）只有一句自己写的近似：

```rust
if position != state.value.selection_extent { state.value.selection_base = position; }
```

上游是这样的：

- **Apple 以外**：动的是**当前**选区的那一端，另一端不动，且有一道硬门槛——
  `if (newSelection.baseOffset >= newSelection.extentOffset) return;`
  注释写着 "Don't allow order swapping"。
  把尾手柄拖回首手柄之外，**整次更新被丢掉**——不是折叠、也不是反转，
  选区原地不动。两个手柄不能交叉，读者手里的还是他抓住的那一个。
- **Apple**：锚点是**起手那一刻**选区的远端（`_dragStartSelection`），
  而且**没有那道门槛**——上游的注释是
  "dragging the base handle makes it the extent"。
  拖过头，选区翻个面继续朝另一边长，两个手柄互换，手指握住的还是同一个。

`_dragStartSelection` 为什么非记不可，正在于此：
**越过之后第一帧，当前选区已经是反的**，再拿它当锚点，锚点就会跟着手指往回走。
`??=` 也因此不能写成直接赋值，`_handleAnyDragEnd` 的第一行是把它清掉。

两个平台唯一一致的地方：选区是折叠的（只有一个光标）时没有东西可以拓宽，
拖动就只是**带着光标走**——但一个问 drag-start 选区，另一个问当前选区，
还是同一个分歧。

这一轮把规则移到 `TextSelectionOverlay::drag_selection` 写一次，
**并且让实时路径去问它**，那句近似删掉——不是加第二份。

### 中途把自己造出来的第三个枚举收掉了

写完发现我给"哪个手柄"新造了 `DraggedHandle`，
而同一个文件里早就有 `SelectionHandleEnd`，`selection_host.rs` 里还有第三个 `HandleEnd`。
一个概念三个类型，中间就得有翻译——而**把两端译反的翻译，这个 crate 里没有一条测试能看见**。
所以 `HandleEnd` 改成 `pub use ... SelectionHandleEnd as HandleEnd`，
`drag_handle_to` 直接把手柄传下去，翻译整个消失。
**"按构造消失"比"被覆盖"好。**

### 变异 10 个，9 红 1 活，活的那条据实记下

九条规则变异全部有测试抓住（能不能交叉、门槛是 `>=` 还是 `>`、
Apple 锚在远端还是近端、倒着选的 drag-start、锚点用不用 drag-start、
`??=` 会不会被改写、哪些平台记、drag end 清不清、折叠时的分支）。

活下来的那条是**实时路径**的：把传给规则的手柄写死成 `Start`，
测试全绿——因为这个 crate 里**没有一条测试从挂载好的输入框上真拖一个手柄**。
规则被从各个角度覆盖了，这一次调用没有。
`editable.rs` 里按这个仓库的老规矩把这句真话写在了调用点旁边，
而不是让它看起来像有覆盖。

尺子：十七把全部 exit 0。门：Rust 6836 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6836 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：就把上面那条活的变异杀掉——
`selection_host.rs` 的 `handle_gesture_tests` 已经证明
**一个 `HandleEntry` 是普通组件，不需要 overlay 就能挂起来按**
（`pressed_handle` 就是这么做的）。
所以缺的只是把 `drag_handle_to` 返回的那个闭包接上去：
给它一个真的 `StateHandle<TextFieldState>`、一个 `RenderRef` 锚点和 `LinesSink`，
按下手柄、移动、抬起，断言选区按平台各走各的路。
**先查一件事**：`editable.rs` 的测试里有没有现成的"挂一个输入框并拿到它的
`RenderRef` 与 `lines_sink`"的辅助函数（`a_field_*` 那一批用的东西），
有就复用，没有就先写那一个，别在测试里手搓一遍字段初始化。

---

## 第 490 轮：把上一轮活下来的那条变异杀掉

上一轮唯一活着的变异是实时路径的：把传给规则的手柄写死成 `Start`，全绿——
因为这个 crate 里没有一条测试从**挂载好的输入框**上真拖一个手柄。
这一轮就做这一件事。

按"先查一件事"查了：需要的三样东西都是现成的，
上一轮记的"这条闭包够不到"和第 426 轮那句一样，是**"我没想到怎么做"**：

- **活的 `StateHandle`**——`StateHandle::detached()` 的 `set_state` 什么也改不到，
  但 `TextField::with_state_sink` 就是这个字段自己公开句柄的办法，
  挂一个 `TextField` 三行就够（`a_field_can_be_given_the_text_it_opens_with` 早就这么做）。
- **`RenderRef` 锚点**——`RenderRef::new` 包一个 `RenderConstrainedBox` 即可；
  没有父级时 `global_to_local` 就是恒等，正合用。
- **`LineLayout`**——测试模块在同一个文件里，直接构造一行 `VisualLine` 就行。

于是有了 `mounted_field` / `selection_of` / `field_handle_drag` 三个小辅助，
和两条真走一遍的测试：

- **Windows**：`Hello brave world` 选中 `brave`(6,11)，
  把**首**手柄拖到最左 → (0,11)；把**尾**手柄拖到同一点 → **选区一动不动**，
  因为两端不能交叉，整次更新被丢掉。
- **iOS**：同一个手势 → (6,0)，选区翻面，锚在起手处。

**同一个手势，两个平台，一个不动一个翻面**——这正是上一轮那条规则的意义，
现在它是从输入框这一端被看见的，而不是只在规则函数上。

### 变异 10 个，全红（上一轮是 9 红 1 活）

上一轮那条"写死 Start"现在被两条新测试同时抓住；
另外"能不能交叉""哪些平台记 drag-start""Apple 锚在哪一端"三条，
除了原有的规则测试之外，**也**被走一遍的测试抓住了——
这正是走一遍的价值：规则和调用点一起被钉住。

同时把上一轮写在调用点旁边的那句话改了：它当时是真的，现在是假的。
**一句过期的注释比没有注释更坏**，所以它现在指向那条测试的名字。

尺子：十七把全部 exit 0。门：Rust 6838 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6838 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：回到 `TextSelectionOverlay` 剩下的最后一处——`_getHandleDy`：

```dart
final double distanceDragged = dragDy - handleDy;
final dragDirection = distanceDragged < 0.0 ? -1 : 1;
final int linesDragged = dragDirection * (distanceDragged.abs() / preferredLineHeight).floor();
return handleDy + linesDragged * preferredLineHeight;
```

**手柄的 dy 是一行一行跳的，不是连着滑的**（横向仍然连续），
而且 `preferredLineHeight <= 0` 或任一端非有限时返回 null——**这一次移动整个不算**。
**先查一件事**：这条规则要成立，得有人记住 `_endHandleDragPosition`
（每次移动都被吸附后的值写回去，下一次从它再算），
而这个 crate 的实时路径每次 move 都是从零开始算的、没有这个状态。
所以先确认：这份状态放在 `TextSelectionOverlay`（它已经存着 grab 和 drag-start 选区）
还是放在 `selection_host` 的 geometry 上——
选错地方就会变成第 489 轮那种"一个概念两个住处"。

---

## 第 491 轮：手柄是一行一行跳的

上一轮的"先查一件事"有明确答案：`_startHandleDragPosition` / `_endHandleDragPosition`
**上游就在 `TextSelectionOverlay` 上**，而这个 crate 的
`TextSelectionOverlay` 已经存着 grab 和 drag-start 选区——同一个住处，不必新开。
而且是**两个**字段而不是一个：两根手指可以同时拖两个手柄，
上游的 `isDraggingStartHandle` / `isDraggingEndHandle` 也是两个。

规则本身（`_getHandleDy`）：**横向跟着手指走，纵向要走满一整行才跳一行**。
滑着走的手柄会大半时间指在两行之间，指下的位置会随着一两个像素来回闪。

一处容易抄错的细节：`floor` 取在**绝对值**上、符号最后补回来，
不等于对带符号的商取 `floor`——`(-1.45).floor()` 是 `-2`，
那样向上拖会比同样距离的向下拖**早一行**跳。

### 写回去，才有迟滞

`advance_handle_drag_dy` 把吸附后的值写回记录，下一次从它再量。
这不是可有可无的：从 100 拖到 139 → 落在 120；再回到 101，
**手柄不动**（离 120 还差一行）；到 99 才跟着回到 100。
不写回的话，回到 101 就直接弹回 100 了。测试正是钉这个来回。

### 为了让它成立，按下时多报一个位置

`_getHandleDy` 要有"从哪儿量"，也就是上游在 drag start 记的
`details.globalPosition.dy`；而这个 crate 的 `on_drag_start` 只报了
**手柄内部**的抓点。两个位置的原点不同、用途也不同：
抓点是每次移动都要减掉的修正，全局位置是**这次拖动的起点**。
所以回调签名从 `Fn(HandleEnd, Offset)` 变成 `Fn(HandleEnd, Offset, Offset)`，
`editable.rs` 在按下时一次性把它换算进字段坐标记下来
（上游是在吸附时才换算，因为缩放变换会把行高也缩放——换算一次、两边同坐标更省事且等价）。

没有按下就来的移动（合成事件，或按下还没报上来），
**以它自己的位置为起点**——上游根本到不了这个分支（`late double`），
但这个 crate 到得了，而 `handle_lift` 早就为同一种情况给了同一种答案，两处得一致。

### 变异 9 个，全红，其中两条是走一遍的测试抓的

新的走一遍测试用了一个**偏心的抓点**（抓在手柄下 4 像素）——
握在正中间的手柄，吸不吸附都落在同一行上，**看不出区别**；
偏心之后，拖四分之三行：吸附的话选区一动不动，不吸附就已经跳到下一行了。
"把手指位置直接当吸附结果"这条变异正是被它抓住的。

尺子：十七把全部 exit 0。门：Rust 6846 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6846 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`TextSelectionOverlay` 逐个成员这一遍到此走完了，
把它记进 `tools/depth_examined.json`（连同这四轮补掉的东西：
第二个工具条、拖动的平台分歧、行吸附、走一遍的测试），
然后 `python tools/depth.py` 看新的队头。
**先查一件事**：记之前先确认 `_endHandleDragTarget` / `_startHandleDragTarget`
到底算不算已经落地——它是 `centerOfLineGlobal - details.globalPosition.dy`，
而这个 crate 用 `handle_lift(grab, line_height)` 做同一件事（把点抬到行上）。
两者**取值不同**（上游用端点的行中心，这里用抓点），
所以要么承认它是个真缺口、要么写清楚为什么这里的做法是等价的——
不能含糊过去，那正是"记一笔"这个机制最容易被滥用的地方。

---

## 第 492 轮：拖手柄瞄的是行的中间，不是手柄的尖

上一轮留了一个必须先答清楚的问题：`_endHandleDragTarget` 到底算不算已经落地？
**答案是没有，而且这个 crate 原来的做法在边界上是错的。**

上游 drag start：

```dart
final double centerOfLineLocal =
    _selectionOverlay.selectionEndpoints.last.point.dy - renderObject.preferredLineHeight / 2;
_endHandleDragTarget = centerOfLineGlobal - details.globalPosition.dy;
```

update 里命中测试用的是 `_endHandleDragPosition + _endHandleDragTarget`，
上游还留了注释说明为什么："selection handles typically hang above or below
the line that they point to"。
而这个 crate 用的是 `handle_lift(grab, line_height)`：**按手指在手柄里的位置往上抬**。
上游第 830 行恰好写着相反的话——
"This is NOT the same as details.localPosition. That is relative to the selection
handle" ——**上游根本不用手柄内坐标**。

差别不是风格问题。端点是这一行的**底边**，
底边是两行的交界、归下面那行；按抓点上抬会落在手柄的尖上，
也就是那条交界线——**于是命中的是下一行**。
上游的修正量是"行中心 − 按下位置"，加回去永远落在行的中间。
而且这一个常量同时覆盖了三件事：手柄挂在行下、读者可能抓在任意位置、
命中测试要的是行中间而不是边缘。

所以这一轮：
- `SelectionEndpoint::line_centre()`（`point.dy − line_height/2`）；
- 手柄按下时报的东西从两个 `Offset` 收成一个 `HandlePress`
  （抓点、按下位置、这个手柄所指那行的中心）——三个数原点不同，
  混用其中两个就会把选区放到差一行的地方；
- `TextSelectionOverlay::begin_handle_drag_at` / `handle_drag_point`
  存下并施加那个修正量；
- `editable.rs` 用它命中；`handle_lift` 降为**没有按下记录时**的退路
  （合成移动那一种，`handle_lift` 本来就是为这种情况写的）。

### 变异 10 个：先 6 红 4 活，补完测试后全红

活下来的四条正好指出三处没测到的地方：
`line_centre()` 本身没人测（两条变异都活）；
"两个手柄共用一份修正"活着，因为我的测试只从一头看
（设了 End 去问 Start）——**得两头都问**；
还有一条是**装配**：drag-start 闭包里"三个数哪个给谁"没人能看见。

第三条按这个仓库的老办法解决：把闭包体抽成自由函数 `record_handle_press`，
于是它变成三个参数的普通调用，测试直接给三个**互不相同**的数，
断言抓点、量起点、瞄准点各归其位。
这也是第 490 轮那句话的又一次应验：**"够不到"往往是"我没想到怎么做"**。

尺子：十七把全部 exit 0。门：Rust 6853 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6853 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`TextSelectionOverlay` 这一遍（第 488–492 轮）到此确实走完了，
四处都补了：第二个工具条、拖动的平台分歧、行吸附、瞄准行中心。
把它记进 `tools/depth_examined.json`——
`finding` 要点名**是哪个机制回答了那些缺失成员**
（`SelectionOverlay` 承接了转发的一半，`glyph_heights` 承接了两个字形高度，
其余四条是这四轮补的），`at` 指向 `text_selection.rs` 与这几轮的记录。
记完 `python tools/depth.py` 重看队头。
**先查一件事**：`_buildMagnifier` 那一支（`MagnifierInfo` 的四个字段：
`globalGesturePosition`、`caretRect`、`fieldBounds`、`currentLineBoundaries`）
是不是已经在 `magnifier.rs` 里了——如果在，这一条也写进 finding；
如果不在，那它就是 `TextSelectionOverlay` 剩下的**真缺口**，
这一遍就还没走完，别急着记。

---

## 第 493 轮：放大镜被告知的是哪一行

上一轮说了：记 `depth_examined.json` 之前先查 `_buildMagnifier`。查了——
**`MagnifierInfo` 这个类型在（四个字段齐全、被 `MagnifierHost` 和摆放规则消费），
但没有任何地方按上游的规矩去*造*一个。**
所以这一遍还没走完，这一轮补掉它，下一轮再记。

`_buildMagnifier` 里有三条真规则：

**一、行的两端取相反的 affinity。**

```dart
final positionAtEndOfLine = TextPosition(
  offset: lineAtOffset.extentOffset, affinity: TextAffinity.upstream);
// Default affinity is downstream.
final positionAtBeginningOfLine = TextPosition(offset: lineAtOffset.baseOffset);
```

上游特意为第二行写了注释，就是为了说明第一行不是手滑。
折行处的那个 offset 是屏幕上的**两个**位置；用默认的 downstream 去问一条折行的末尾，
答的是**下一行的开头**——放大镜的边界就会伸到下面一行去，
手指越靠近折行处漂得越远。

**二、行的边界由两个光标矩形拼出来，而且各取不同的角**：
起点光标的 `topCenter` 到终点光标的 `bottomCenter`。
两个都取同一个角，得到的矩形会矮一整行。
x 取的是光标的**中心**而不是边——光标有两三个像素宽，两条边都不是文字的起止处。
`Rect.fromPoints` 会归一化，所以从右往左的行给出同一个矩形而不是反的。

**三、手势位置有退路，另外三个没有**：
`overlay?.globalToLocal(pos) ?? pos`。四样东西最后都在**overlay 的坐标系**里，
只有这一个是从全局出发的，所以只有它需要"没有 overlay 就原样用"。

### 一条差点混过去的测试

`with_no_overlay_...` 的第二个断言我一开始写成
`assert_eq!(f(global, Some(&overlay)), overlay.global_to_local(global, None))`
——**拿被测对象的邻居去算期望值**，而且一个没挂进树里的 render object
`global_to_local` 就是恒等，所以"根本没问 overlay"的变异照样绿。
变异扫描当场把它抓了出来（8 条里唯一活着的一条）。
改法是给 overlay 一个**真的位置**：塞进 `RenderStack` 的 `left: 30, top: 25` 里
并且让根是个 `RenderRef` 走一遍 layout（父子关系是 `layout_child` 建立的），
然后断言一个具体的数 `(90, 15)`。补完 8 条全红。

这一条值得记住：**期望值不能由被测代码的邻居算出来**——
第 47x 轮记过一次"读邻居答案的尺子不如没有"，这次是同一个错误换了个地方出现。

尺子：十七把全部 exit 0。门：Rust 6857 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6857 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：现在 `TextSelectionOverlay` 这一遍（488–493）真的走完了，
把它记进 `tools/depth_examined.json`：`finding` 要点名
**是哪个机制回答了那些缺失成员**——转发的一半在 `SelectionOverlay`、
两个字形高度在 `glyph_heights`、拖动状态与规则在 `TextSelectionOverlay` 自己身上、
放大镜信息在 `magnifier_line_*` 与 `magnifier.rs`——
并列出这六轮补掉的六处真缺口；`at` 指向 `text_selection.rs` 与 488–493 的记录。
记完 `python tools/depth.py` 重看队头。
**先查一件事**：`depth_examined.json` 的既有条目里，
有没有哪条的 `finding` 只写了"在别处"而没点名机制——
这个文件的门槛是"可核对"，如果我自己这条写成一句话，
那这个机制就开始退化成压制列表了。

---

## 第 494 轮：记下这一遍，顺便让这本账自己能被核对

`TextSelectionOverlay` 那一遍（488–493）记进 `tools/depth_examined.json` 了。
`finding` 按门槛写足：哪些成员在 `SelectionOverlay`、在 `OverlayMagnifier` 与
`MagnifierHost`、在 `glyph_heights`、在 `visibilities`，
哪些是构造时传过去而不存下来的（context、renderObject、selectionControls…），
以及这六轮补掉的**五处真缺口**分别是什么（第 490 轮不算缺口，它补的是覆盖）。

### 先查的那件事，答案有点出乎意料

要查的是"有没有条目只写了'在别处'而不点名机制"。逐条读完：**没有**，
每条都点了名。但发现了另一种烂法——**三条最老的行没有 `at` 字段**
（它后来才加），其中两条把 PORTING_STATUS 的轮次写在正文里，
读的人得自己去翻。补齐这三条时又发现 `HapticFeedback` 那条我差点写成
`services/haptic_feedback.rs`——**这个文件不存在**，它在 `services/system.rs` 里。

也就是说，这本账**最容易烂的地方不是理由写得空，而是指针指错**。
文件会改名、模块会拆分，指针失效是常态而不是意外。

所以这一轮给 `depth.py` 加了一条自检：加载时把每条 `at` 里出现的
`src/....rs` 都验一遍，找不到就**打印出来并 exit 1**，
仓库相对和 crate 相对两种写法都接受（两种在不同场合都更好读）。
现有条目里有五处是 crate 相对的简写，正好被这条规则覆盖。

> 一条没人能跟着走到的记录，就是一次压制，不是一次阅读。

### 变异 6 个，全红——其中一条改变了我对自己这条记录的说法

- 删掉 `TextSelectionOverlay` 这一行 → `--examined` 里就没有它了。
  **注意这里我原本写错了**：本来打算断言"删掉它就回到队头"，结果不是——
  这六轮把它从 0.28 做到了 **0.68（17/25）**，早就不在队头附近了。
  所以这条记录**不是**让它隐身的东西，*工作本身*才是；
  记录承担的是"剩下那 8 个成员的读数"。扫描逼我把这句话改对。
- 三条不同的记录各自被改成指向一个不存在的文件 → 尺子拒绝加载（3 红）。
- 去掉自检 → 那个本该被拒的文件顺利通过（1 红）。
- 只接受仓库相对路径 → 现有的 crate 相对写法全部报错（1 红）。

尺子：十七把全部 exit 0。门：Rust 6857 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6857 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：新的队头是 **`ReorderableListView`（0.26，8/31，material）**——
第 486 轮刚动过它（`ReorderableConfig`），所以先别当成新地方。
**先查一件事**：上游那 31 个成员里，有多少是
`ReorderableListView.builder` 这个命名构造子带来的、有多少是
`ScrollView` 一路传下去的滚动参数（`scrollDirection`、`padding`、`physics`、
`primary`、`shrinkWrap`、`anchor`、`clipBehavior`、`restorationId`…）。
如果绝大多数是后者，那这条和第 487 轮的 cache extent 是同一类问题：
**这个 crate 的滚动参数没有一个统一的住处**，
那就不该在 `ReorderableListView` 上再补一遍，而该看
`scrolling.rs` 里有没有（或该不该有）一个"滚动视图的公共配置"。

---

## 第 495 轮：可重排列表是一个滚动视图，现在它这么说了

队头 `ReorderableListView`（0.26，8/31）。按上一轮的"先查一件事"数了那 31 个成员：

- **13 个**是重排本身的（itemBuilder、itemCount、四个回调、proxyDecorator、
  buildDefaultDragHandles、三个 extent 来源、autoScrollerVelocityScalar、dragBoundaryProvider）；
- **14 个**是 `ScrollView` 的参数，原样往下传（scrollDirection、reverse、
  scrollController、primary、physics、shrinkWrap、anchor、cacheExtent、
  scrollCacheExtent、dragStartBehavior、keyboardDismissBehavior、restorationId、
  clipBehavior、padding）；
- **3 个**是这个 Material 类自己的（header、footer、mouseCursor）。

假设成立：**近一半的"缺口"是滚动视图的参数。**

然后第二件事查出了一个更好的结果：**这个 crate 早就有 `scroll_view::ScrollView`**，
上游 `ScrollView` 的四条断言、`physics` 的默认规则、`effectivePrimary`、
`viewport_kind`、keyboard dismiss 全都在里面。
也就是说不需要"造一个公共住处"——**住处早就有，只是这两个可重排列表没住进去**。

所以这一轮不是再声明十四个字段，而是让两个类各持有一个 `ScrollView`：

- `ReorderableListView::validate` 在自己的三条之后再问它一次；
- `ReorderableList`（widgets 层）同样，并且 `with_scroll` 会把轴和方向带下去。

### 顺序是有意义的，不是摆设

Material 那三条断言在**写下这个 widget 的那一刻**触发，
而滚动视图的断言是 `build` 造 `CustomScrollView` 时才触发。
所以两处都错的列表，**先听到的是自己的那条**。这一条单独有测试钉着。

### 一个轴，两个住处，这次是有理由的

上游的 sliver 不带轴——它从约束里读回来。这个 crate 的 sliver 需要它做间隙运算，
所以轴在 `scroll` 和 `sliver` 上各有一份。
处理办法是**只留一个 setter**：`with_axis` 同时写两处，
`with_scroll` 把整个滚动视图的轴带下去。
变异扫描专门试了"只写一处"的三种写法，全部被抓。

### 变异 7 个，全红

两个类各自"不问滚动视图"（2 红）；把滚动视图的断言提到最前（1 红）；
轴只到 scroll、只到 sliver、整个 scroll 换掉却不带轴、reverse 只到 sliver（4 红）。

尺子：十七把全部 exit 0。门：Rust 6861 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6861 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`ReorderableListView` 现在 0.29（9/31）——比值几乎没动，
因为**多出来的十四个成员是 `ScrollView` 的字段，depth 只数这个类身上的**。
这正是 `depth_examined.json` 该记的那种情况，但**先别急着记**：
剩下的三个 Material 成员 `header` / `footer` / `mouseCursor` 还没查过。
**先查一件事**：上游 `ReorderableListView` 的 header/footer 不只是"多两个孩子"——
它们参与 `_itemBuilder` 的**索引换算**（有 header 时孩子的 index 要减一），
而且 `onReorder` 报的索引是**列表的**索引不是孩子的。
先看这个 crate 的 `reorder_report` / `insert_index_for` 有没有这层换算，
没有就是真缺口，有就连同 ScrollView 那十四个一起记。

---

## 第 496 轮：内边距只能花一次

上一轮让我先查 header/footer 有没有**索引换算**。查了：**没有**——
我猜错了。上游把 header 和 footer 做成**各自独立的 sliver**
（`SliverToBoxAdapter` 包在 `SliverPadding` 里），
可重排的那个 sliver 的索引因此完全不受影响，`onReorder` 报的就是它自己的索引。
先查这一步的价值正在于此：不查就会去补一个不存在的换算。

但同一段 `build` 里有另一条真规则，上游自己给它写了注释：

```dart
// If there is a header or footer we can't just apply the padding to the
// list, so we break it up into padding for the header, footer and
// padding for the list.
```

**有了 header，内边距就不能整个交给列表**，否则读者会看到两遍：
header 上方一次，header 与第一行之间又一次。
所以 header 保留自己那一端、交出与列表相邻的那条边，列表反过来。

三个细节都容易写错，也都各有测试：

1. **反向列表整个翻过来**。上游在别的事情之前先 `(start, end) = (end, start)`：
   反向时 header 在屏幕**下方**，于是它保留的是**下边距**。
2. **`(start ?? end) == null` 是一个判断管两件事**——
   这个表达式为 null 当且仅当 header 和 footer 都没有。
   写成"没有 header 就不拆"会让只有 footer 的列表把内边距花两次。
3. **横轴自始至终不动**。三个 sliver 共享的只有滚动方向的两端；
   左右两边属于它们每一个。

### 变异 7 个，全红

反向不翻（1 红）；header/footer 不换回来（3 红）；只有 footer 当成没有（1 红）；
列表保留整份内边距（3 红）；header 保留与列表相邻的那条边（2 红）；
横向列表拆的是上下（1 红）；连横轴也拆（3 红）。

`ReorderableListView` 从 0.26 走到 **0.42（13/31）**，两轮加了五个成员，
每一个都带着规则进来，没有一个是"补个字段让比值好看"。

尺子：十七把全部 exit 0。门：Rust 6866 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6866 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`ReorderableListView` 剩下的成员里，
`mouseCursor` 是最后一个 Material 独有的，其余都是 `ScrollView` 的字段
（depth 只数这个类身上的，所以比值还会停在这附近）。
**先查一件事**：上游的 `mouseCursor` 不是直接挂在行上的——
它进的是 `_ReorderableListViewChildGlobalKey` 那条路还是
`ReorderableDragStartListener` 的 `MouseRegion`？
读清楚**它作用在哪一块区域**（整行、还是只有拖动把手），
再决定是补到 `ReorderableDragStartListener`（那里已经有 `enabled` 了）
还是补到这个类上。如果它作用在把手上，那就该在 widgets 层，
和第 486 轮"断言写在上游写它的那一层"是同一条道理。

---

## 第 497 轮：桌面上给一个把手，手机上整行就是把手

上一轮让我先读清 `mouseCursor` 作用在哪。读了——它的文档写得很直白：
*"The cursor for a mouse pointer when it enters or is hovering over the drag
handle."* **只作用在内建的拖动把手上**。
顺着它读 `_itemBuilder`，发现把手这一整块比一个光标大得多，而且这个 crate
只有一个 `builds_default_drag_handles: bool`：

**上游按平台给两种完全不同的东西**：

- linux / windows / macOS：行上叠一个**看得见的把手**
  （`Stack` + `Positioned.directional`），用 `ReorderableDragStartListener`，
  **一按就拖**；
- iOS / android / fuchsia：**没有把手**，整行套 `ReorderableDelayedDragStartListener`，
  **长按才拖**。

两半出自同一个事实：桌面读者有指针，一次落在行上的按下没有别的用途，
所以给一个看得见的小目标更好；手机读者的按下**已经被滚动占用了**，
所以拖动必须用一个滚动不认领的手势——长按就是那个手势。
（这个 crate 的两个 listener 早就在，正是这一对。）

把手的位置也是规则，不是随手写的数：
**整条横轴 + 贴住主轴的尾端**。竖直列表里是沿右侧的一条通高长条、把手在中间；
水平列表里是沿底边的一条通宽长条。长条是为了好按——读者在行的任意高度按下都能中；
对齐是为了把手本身不乱跑。8 像素的内缩只加在**贴住的那条边**上，
另一侧不加：加了就会从行内容那一侧把长条削窄。

光标那条最后也补上了：**悬停是张开的手，按住是握起的手**；
应用给了自己的光标时，仍然拿 `{dragged}` 去解析它，
所以一个按状态给值的光标在拖动时仍然走它自己的 dragged 分支——
不是被压成一个值。

### 变异 8 个，全红

每个平台都给把手 / macOS 算进手机那半（2 红）；不建把手也照建（1 红）；
竖直把手不是长条（1 红）；把手贴到前缘（1 红）；
水平列表按竖直的方式贴（1 红）；手永远不握起（1 红）；
自带光标不按拖动状态解析（1 红）。

`ReorderableListView` 走到 **0.52（16/31）**，三轮从 0.26 上来，
每一步都是带着规则进来的成员。

尺子：十七把全部 exit 0。门：Rust 6871 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6871 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`ReorderableListView` 这一遍差不多到底了——
剩下的 15 个里绝大多数是 `ScrollView` 的字段（住在 `self.scroll` 上，
depth 数不到）和三个 extent 来源（住在 `self.config` 上）。
**先查一件事**：还有一个真的没查过的——`_proxyDecorator`：
拖起来的那一行被包进 `Material(elevation: lerp(0, 6, Curves.easeInOut(t)))`。
**这是默认的拖动外观**，而 `proxyDecorator` 参数只是让应用换掉它。
先看这个 crate 有没有"被拖起的行长什么样"的东西
（`reorderable_list.rs` 里只有几何，没有外观；`material.rs` 的 elevation 那一带可能有），
没有就补这条默认装饰，然后把 `ReorderableListView` 记进 `depth_examined.json`。

---

## 第 498 轮：被拖起来的那一行，怎么抬起来、怎么飞回去

`_proxyDecorator` 查了：这个 crate 有拖动的**几何**（间隙、插入索引、落点分类），
但**没有任何关于"被拖起的行长什么样、怎么动"的东西**。补掉三条，
它们凑在一起就是"那一行"的全部：

**一、抬起来**：`Material(elevation: lerpDouble(0, 6, Curves.easeInOut(t)))`。
阴影是**跟着同一段动画长出来的**，读者看到的是这一行被拿起来；
一上来就是 6 会读成"另一行冒出来了"。
`easeInOut` 是两端都静止的曲线——拖之前静止，拿住之后也静止。

**二、飞回去**：`_DragItemProxy` 里另一条 lerp，用的是 **`easeOut`**，不是同一条。
手指抬起的那一刻才有落点，动画随即倒着跑回 0，
所以这条 lerp 要**从远端读**：t=1 还在手指下，t=0 正好是行该在的地方。
一端有速度、一端停住——所以是 easeOut。
还有 `dropPosition - overlayOrigin`：落点是列表坐标、proxy 在 overlay 里，
少减这一下，行就会按"列表离页面顶端多远"整个飞偏——
而且只在列表上方还有东西的脚手架上才看得出来。

**三、溢出对齐**：`OverflowBox` 的 alignment，横向 `centerLeft`、纵向 `topCenter`
——**留住的是滚动出发的那条边**。绕中心溢出的话，行一被拿起就会看起来挪了一下。

### 变异 8 个：先 7 红 1 活，测试改完全红

活下来的是"没有落点也照飞"。原因在我的测试：它只在 **t = 1** 上断言，
而那一点上 `lerp(x, position, 1) == position`，变异当然看不出来。
改成在 `t ∈ {0, 0.3, 1}` 上都断言——**没有落点时，曲线和 overlay 原点都不该有机会插手**。
这是第 493 轮那条教训的近亲：**断言挑的那个点，可能正好是所有实现都一致的点。**

### 记账

`ReorderableListView`（495–498）记进 `depth_examined.json` 了：
十四个成员在 `self.scroll`、三个 extent 来源在 `self.config`、
四个回调是两个标志加 `ReorderReport`，
四轮补掉的四处真缺口，以及**确实没有对应物**的两个
（`dragBoundaryProvider` 和自动滚动，因为 `EdgeDraggingAutoScroller` 没移植，
这一条模块注释里早就写着）。
比值 0.52，记录承担的是剩下那一半的读数。

尺子：十七把全部 exit 0。门：Rust 6875 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6875 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：新队头 `CupertinoSegmentedControl`（0.31，4/13）。
**先查一件事**：这个 crate 里 `cupertino_controls.rs` / `cupertino.rs` 有没有
`CupertinoSlidingSegmentedControl`——上游是**两个**类
（老的 `CupertinoSegmentedControl` 与新的 sliding 版），
它们的规则大半不同（老的只有边框和填充，新的有滑块和 thumb 动画）。
如果这个 crate 只做了 sliding 那一个，那 4/13 就是"另一个类没做"，
是真缺口而不是映射；如果两个都在，就要分清 depth 把哪一个配给了它。

---

## 第 499 轮：一个分段控件可以被上色，也可以被关掉

队头 `CupertinoSegmentedControl`（0.31，4/13）。先查了上一轮问的那件事：
上游确实是**两个**类，而这个 crate 只做了老的那个——
`cupertino.rs` 的锚点注释写得很清楚："the iOS-13 `CupertinoSlidingSegmentedControl`
is a different widget and not part of this port"。所以 4/13 数的是老的这个，
而它缺的 9 个里有一大半是**六个颜色参数和禁用集合**：
这个 crate 的 build 直接读了四处主题，**应用根本没法给自己的颜色**。

`_updateColors` 有两处不是"给了就用、没给用主题"那么简单：

**一、`disabledColor` 一个参数、两个默认值，而且不对称**：

```dart
final Color selectedDisabledColor = widget.disabledColor ?? selectedColor.withOpacity(0.5);
final Color unselectedDisabledColor = widget.disabledColor ?? unselectedColor;
```

被选中但禁用的那一段**淡到一半**——它仍然要读作"选中的那个"，同时说明改不了；
未选中的那一段**原样不动**——它本来就是空的，把空的再淡一次什么也没说。
给了这个参数，两边都变成它——那是调用者把这个判断收回去。

**二、`disabledTextColor` 的默认值不是主题的**，是常量
`Color.fromARGB(115, 122, 122, 122)`。整个控件里只有这一个颜色不跟主题走，
道理也说得通："这个按不了"不是一句关于品牌的话。

### 三个判断的顺序就是规则

`getBackgroundColor` / `getTextColor` 的分支顺序：
**禁用压过一切**（所以禁用的段不会显示按下）；
**选中排在按下之前**（所以按已选中的那一段什么也不会变——
这与 `_onTapDown` 从不把已选中的段标记为按下是同一件事）；
文字永远是填充的反色——这里只有一对颜色，谁是墨取决于谁是漆。

顺手补了两条构造断言：至少两段（一段不是选择，是标签）；
选中项必须是其中之一（上游按 key 找，这里就是下标越界）。

### 变异 12 个，全红——其中两条是走一遍的

新加了 `painted_rect_colors`：把控件挂起来、布局、真画一遍，收集画出的矩形颜色。
于是"widget 仍然读主题而不是读给的颜色"和"禁用段照常画"这两条变异
**被画布抓住**，而不是只被规则函数抓住。

`CupertinoSegmentedControl` 0.31 → **0.54（7/13）**。

尺子：十七把全部 exit 0。门：Rust 6884 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6884 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：这个类还剩下的是 `children`（这里是 `labels`，映射差异）、
`onValueChanged`（`wired`）、`groupValue`（`selected`）——都在，
以及**真的没做的**：每段选中时 165ms 的淡入（`_kFadeDuration` 与三条
`ColorTween`，模块注释里写着"not ported"）。
**先查一件事**：这个 crate 有没有"一段颜色随动画在两个值之间走"的现成东西
（`animation.rs` 的 `ColorTween`？`component_themes.rs` 里到处在 lerp 颜色），
有就把 `getBackgroundColor` 的 animating 分支补上——
它正是这三条 tween 的读取点；没有就先补 `ColorTween`，
别在这个控件里手搓一次颜色插值。

---

## 第 500 轮：165 毫秒，从手指底下的那个颜色开始

`ColorTween` 查了：`animation.rs` 里就有，还实现了 `Tween`/`Animatable`。
所以这一轮把上一轮明说"没做"的那部分补上——每段选中时的淡入。

上游三条 tween，**两条背景色的终点相同、起点不同**，差别就是整个设计：

```dart
_forwardBackgroundColorTween = ColorTween(begin: _pressedColor, end: _selectedColor);
_reverseBackgroundColorTween = ColorTween(begin: _unselectedColor, end: _selectedColor);
```

刚被**点中**的那一段，一瞬间之前还在手指底下，所以它从**按下的浅色**淡进填充色——
中间不会闪回空白；正在**失去**选中的那一段没有手指压着，就淡回普通的未选中色。

而且这两条 tween 在两个地方**分配得正好相反**，这不是笔误：

- `_updateAnimationControllers`（初次建立）：选中的那段拿 **reverse** 并停在 1.0
  ——它将来失去选中时正好一路退回空白；此时还没有任何按下，
  "按下色"没有资格成为一个静止段落的任何一端。
- `didUpdateWidget`（选中变了）：新选中的拿 **forward** 往前跑，其余拿 reverse 往回跑。

**中途改主意的那一下也是规则**：`AnimationController.forward()` 从半开的位置出发，
剩下的路只有一半，所以只花一半时间——颜色始终以同一个速度走。
从当前值重新计时整段时长，会让第二次快速点击变得拖沓。

分支顺序也补齐了：**禁用第一、动画第二**、然后才是选中/按下。
动画排在选中之前，因为半途中的段落**介于两个答案之间**，
先问"它属于哪一边"就会把它啪一下吸到某一边。

### 变异 11 个：先 7 红 4 活，全部处理完再全红

四条活的各说明一件事：

1. **"淡入永不结束"**——我的断言只测到 `t = 时长` 那一点，
   而那一点上"有没有夹紧"给出同样的值。补了一条 `3 倍时长`：**它得停在那里，不能冲过头**。
2. **"禁用测试挪到动画之后"**——我第一版写的变异是 `fade.filter(|_| true)`，
   那是个**等价变异**，不是覆盖缺口。换成真的挪顺序，当场被现有断言抓住。
3+4. **widget 那一半没被观察**（不启动淡入、不请求下一帧）。
   补了一条走一遍的测试：挂起来、改 `selected`、再建一帧，然后**真画一遍**——
   刚被选中的那段画出来的是**按下色**（时钟没走，淡入正好在起点），
   而且树还在要下一帧。三条变异被它一起杀掉。

顺手把模块注释里"per-segment selection fade ... is not ported"改掉了——
它现在是假的。**过期的注释比没有注释更坏**，第 490 轮记过一次。

尺子：十七把全部 exit 0。门：Rust 6889 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6889 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`CupertinoSegmentedControl` 这一遍到底了（0.54，7/13，
剩下的是 `children`/`groupValue`/`onValueChanged` 的映射差异与
`_SegmentButton` 的焦点管理），下一轮把它记进 `depth_examined.json`。
**先查一件事**：记之前先确认 `_SegmentButton` 那条——
上游给每段一个 `GlobalKey<_SegmentButtonState>` 并在 `_onTap` 里
`requestFocus()`，也就是**点一下会把焦点移到那一段**。
这个 crate 的 `focus.rs` 有焦点节点，但这个控件一个也没建。
如果确实没有，那它就是一处真缺口，这一遍还没走完——
和第 493 轮 `_buildMagnifier` 的情形一模一样，别急着记。

---

## 第 501 轮：一段就是一个单选钮，禁用的那段不在组里

上一轮说记账前先查 `_SegmentButton`。查了，**是真缺口**，所以这一遍还没走完
（和第 493 轮 `_buildMagnifier` 一模一样，幸好没急着记）。

上游每一段都是一个带 `FocusNode` 的 `RadioClient`——这才是分段控件"是一排单选钮"
的地方：方向键在段之间走，空格选中，而**选中会把键盘带过去**。
这个 crate 的 `radio_group.rs` 早就有 `RadioClient`/`RadioGroup`/`set_registry`，
所以缺的只是把这一段接上去。

三条规则，每条都有它的道理：

**一、禁用的段要*离开*组，而不是留在组里当一个禁用成员。**

```dart
registry = widget.enabled ? RadioGroup.maybeOf<T>(context) : null;
```

差别在方向键上：留在组里的禁用项是一个**要跳过去**的停靠点，
离开了的根本不在那儿。而且上游在 `didUpdateWidget` 里**又写了一遍**同一行，
为的是重新启用时能回到组里——否则一个随表单填写逐步可用的控件会永远够不着。

**二、`dispose` 的第一行是 `registry = null`**，在处置 focus 节点**之前**：
一个还攥着已销毁节点的组，会把键盘交给一个不存在的东西。

**三、`requestFocus` 只在启用时生效**，而 `_onTap` 是**先聚焦、后上报**：
用鼠标选一段之后，键盘就停在那一段上，方向键从读者刚才的位置继续。

`tristate => false` 也顺手钉住了：一个分段控件永远恰好选中一个，
没有第三个状态可循环——这与 `_onTap` 拒绝重复上报已选中段是同一个事实。

### 一处如实记下的没接上

想把 focus 节点真接进 widget 时撞上了：这个控件的 `build` 用 `leaf(...)`
直接造**渲染对象**，而 `Focus` 是个 widget，进不去那里。
于是点击处理里"请求焦点"这一步写着，但**没有节点会应答**。
按这个仓库的老规矩，这句真话写在调用点旁边：规则移植了、测试有了，
**注册没有**，要接上得先把这个 build 改成 widget 树。
不写清楚的话，下一个读者会以为焦点已经能用了。

### 变异 6 个，全红

禁用还留在组里；`enabled` 标志不记；禁用段照样拿键盘；选中不移焦点；
段变成可切换的；销毁时不退组。

尺子：十七把全部 exit 0。门：Rust 6893 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6893 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：把上面那处接上——**先查一件事**：
这个 crate 里有没有别的控件是"`leaf` 造渲染对象 + 需要焦点"的先例
（`components.rs` 的按钮那一带、`cupertino_controls.rs` 的开关），
看它们是怎么解决的：是整个 build 改成 widget 树，
还是有一个能包住渲染对象的焦点组件。**照着已有的那条路走**，
别为这一个控件发明第三种做法——第 489 轮的教训是同一个概念多一个住处就多一处会漂移。

---

## 第 502 轮：把焦点真接上，然后发现自己接了两遍

上一轮如实记下的那处"没接上"，这一轮接上了。按"先查一件事"看了先例：
`ink_well.rs` 是把**一个 widget 孩子**包进 `crate::focus::Focus`，
而 `cupertino.rs` 里到处用的 `many(children, |rendered| ...)`
正是"孩子是 widget、布局仍然自己写"的办法。照这条路走：
每一段变成自己的 widget（`leaf` 里只剩这一段的渲染对象），
外面包一个 `Focus`，再用 `many` 把它们排成一行。

### 然后变异扫描告诉我：焦点被接了两遍

第一版我在点击回调里写了 `crate::focus::focus(node)`，
结果"**把这一句整个删掉**"的变异**全绿**——因为 `Focus` 组件**本来就**
`focus_on_tap: true`，它自己会在被点时取走键盘。
也就是同一条规则有了两个执行者，而其中一个还是我刚加的。

处理办法不是把测试写严，是**只留一个**：删掉显式那句，
改成给节点自己的 `focus_on_tap` 加上**闸门**——
`SegmentButton::request_focus()` 对禁用段答 `None`，
于是 `with_focus_on_tap(takes_focus)` 与 `with_traversable(takes_focus)`
同时关掉"点击取焦"和"Tab 停靠"。
**上游把禁用段移出单选组，方向键和 Tab 必须口径一致。**

顺带抓出一个真 bug：build 里创建 `SegmentButton` 时**从没告诉它自己是不是禁用的**
（`enabled` 默认 true），所以禁用段照样吃掉了键盘。
是"点一下禁用段"的走一遍测试把它逼出来的。

### 变异 5 个（加上一轮的 6 个），全红

节点不再随点击取焦；禁用段照样取焦；button 从不被告知禁用；
禁用段仍是 Tab 停靠点；焦点 id 与指针 id 撞车。
最后那条值得一提：两套 id 本来就该分开，撞车之后
`Pointer` 和 `Focus` 会注册同一个数字——现在有测试盯着。

三条走一遍的测试：点一段 → 键盘落在它上；点禁用段 → 键盘不动；
Tab 走过三段 → **直接跳过中间那个禁用的**。

尺子：十七把全部 exit 0。门：Rust 6896 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6896 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`CupertinoSegmentedControl` 这一遍（499–502）现在真的走完了，
下一轮把它记进 `tools/depth_examined.json`：
`finding` 要点名 `children`→`labels`、`groupValue`→`selected`、
`onValueChanged`→`wired` 这三处映射，六个颜色参数与禁用集合在哪，
`_SegmentButton` 的单选钮身份在 `SegmentButton` 上，
以及**确实没有对应物**的那个——`CupertinoSlidingSegmentedControl` 是**另一个类**，
模块注释里写着不在移植范围（depth 会不会把它也算进这一条，记之前顺手确认）。

---

## 第 503 轮：记账之前那一查，查出一句假话

要记 `CupertinoSegmentedControl` 之前，按上一轮说的先确认 depth 会不会把
sliding 那个类也算进来。一查——

```
python tools/depth.py --name CupertinoSlidingSegmentedControl
  0.82     9   11  CupertinoSlidingSegmentedControl  (cupertino/sliding_segmented_control.dart)
```

**它早就移植了**，在 `cupertino_controls.rs` 里，带着 thumb 半径、插入量和分隔线。
而 `cupertino.rs` 顶上的锚点注释写着"the iOS-13
`CupertinoSlidingSegmentedControl` is a different widget and **not part of this
port**"——写的时候是真的，现在是假的。
如果不查这一下，我就会把这句假话原样抄进 `depth_examined.json`，
那本账正是靠"可核对"活着的。

顺着这条线还发现一件事：两个文件各有一个 **`SegmentedControlError`**，
而且都有 `FewerThanTwoSegments` 这个同名变体——
上游确实在两个控件里都写了 `assert(children.length >= 2)`，
所以这是**同一条规则的两个住处**，正是第 489 轮记过的那种漂移起点。
合成一个：`cupertino_controls.rs` 改成 `pub use crate::cupertino::SegmentedControlError;`。

### 扫描

两条，都不是 cargo 能测的那种：
删掉这一轮记的那行 → `--examined` 里就没有它了；
把共用的错误类型改回两个同名枚举 → **编译不过**
（探针函数把一个当另一个用），也就是"共用"这件事有编译器盯着，不需要测试。

### 记账

`CupertinoSegmentedControl`（499–503）进账了，`finding` 点名：
`children`→`labels`（上游按值作键、这里按下标，所以它的 groupValue 断言在这里是越界检查）、
`groupValue`→`selected`、`onValueChanged`→`wired`；
四轮补掉的四组真缺口；以及**确实没有对应物**的一处——
`RadioGroup.maybeOf(context)` 这个继承式查找，这个 crate 没有那套注册表，
所以 `set_enabled` 是把要加入的组当参数传进去的。

尺子：十七把全部 exit 0。门：Rust 6896 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6896 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：新队头 `CupertinoSwitch`（0.31，8/26）。
**先查一件事**：这一轮的教训直接用得上——先看 `cupertino_controls.rs` 与
`cupertino.rs` 里**各有没有**一个开关（`grep -rn "CupertinoSwitch"`），
以及 `component_themes.rs` 里有没有 `CupertinoSwitchThemeData`（`activeTrackColor`
那一族）。上游 26 个成员里有一大半是颜色与主题参数，
如果主题那半在 `component_themes.rs`，那它和第 495 轮 `ScrollView` 的情形一样：
**住处已有，只是控件没住进去**，别在开关上再声明一遍。

---

## 第 504 轮：关掉的开关，滑块还是那个颜色

队头 `CupertinoSwitch`（0.31，8/26）。先查了主题那一半：
`component_themes.rs` 里的 `active_track_color` 属于 **Material** 的 `Switch`，
Cupertino 这边**没有**主题类——上游的 `CupertinoSwitch` 也确实没有
`CupertinoSwitchThemeData`，它的颜色是**参数**加 `CupertinoTheme` 的两三个值。
所以这不是第 495 轮那种"住处已有"，是真的缺参数：
这个 crate 的开关只有 `active_track_color`，
**关闭态的轨道和滑块颜色都是写死的**。

补掉四个颜色和它们的默认链，其中一条值得单独说：

```dart
final Color effectiveInactiveThumbColor =
    _resolveThumbColor(widget.inactiveThumbColor, inactiveStates) ??
    _widgetThumbColor.resolve(inactiveStates) ??
    effectiveActiveThumbColor;
```

**关闭态的滑块回退到打开态的滑块，而不是白色。**
一个自定义了滑块颜色的开关，关掉时**保持那个颜色**；
只有明确要求了不同的关闭态滑块才会有两个颜色。
回退到白色的话，每个定制过的滑块都会在关掉的那一瞬间闪回白——
而那正是读者盯着它看的时刻。

还有一对"同一个东西两个名字"：`activeColor` 被改名为 `activeTrackColor`，
`trackColor` 改名为 `inactiveTrackColor`，
上游在初始化列表里**先断言不能都给，再把旧名折进新名**。
两件事是一对：只折不断言的话，一个改名改到一半、两个名字给了两个颜色的调用者，
会看到其中一个被悄悄丢掉；断言说明白这是哪一种错——
不是"这个颜色不对"，而是"你把同一件事命名了两次"。

### 变异 8 个：先 7 红 1 活，活的那条是我自己的设计问题

活下来的是"widget 画主题而不是自己的颜色"。原因不在测试，在我写的
`fn at(&self, on, resolved)`——**它根本没读 `self`**，
于是把接收者换成任何一个同类型的值都看不出区别。
一个不读接收者的方法就是一个乔装的自由函数。
改成 `ResolvedSwitchColors::at(on)`（真正读自己那四个字段），
变异就有了落点，重扫全红。

走一遍的那条测试画了一个关着的、自定颜色的开关，
从画布上读回轨道色和滑块色。

`CupertinoSwitch` 0.31 → **0.46（12/26）**。

尺子：十七把全部 exit 0。门：Rust 6901 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6901 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`CupertinoSwitch` 剩下的成员里，最像真规则的是
`onLabelColor` / `offLabelColor` 与 `MediaQuery.onOffSwitchLabelsOf(context)`——
**开关上的那两个小标记（竖线与圆圈）只在系统的辅助功能开关打开时才画**，
而这个 crate 已经有 `SwitchOnOffLabels`（`ON_SIZE`、`OFF_RADIUS`、两个 padding、
两个默认色）。**先查一件事**：`SwitchOnOffLabels` 现在是**谁在读**——
如果只有常量没有画出来，那缺的是"画"；如果画了但没有那个 MediaQuery 开关，
那缺的是"什么时候画"。两者要补的东西完全不同，别猜。

---

## 第 505 轮：竖线和圆圈，画在拇指下面

上一轮问的是"缺的是画，还是什么时候画"。查了：
`SwitchOnOffLabels::resolve`（那个 MediaQuery 闸门加两个默认色）**在**，还有测试；
`ON_SIZE`/`OFF_RADIUS`/两个 padding 也都在。
**缺的是画**——整个 crate 里没有一处读这些常量去画东西。

补上，三条规则跟着一起进来：

**一、两个内缩量不一样**（11 与 12），因为形状不一样：
一根一像素宽的竖线和一个直径十的圆环，在同样的内缩下**看起来不一样深**。
右到左时两个标记换端，别的不变。

**二、每个标记随着拇指压到它身上而淡出**：

```dart
final double leftLabelOpacity = visualPosition * (1.0 - currentReactionValue);
final double rightLabelOpacity = (1.0 - visualPosition) * (1.0 - currentReactionValue);
```

于是没有哪一个会被移动中的拇指切成一半——开着时竖线是实的、圆环在拇指底下已经没了。

**三、第二个因子是"按住"**：拇指被按住时会朝轨道中间变宽，
所以两个标记**一起**淡出，而不是被它裁掉。按住的开关一个标记也不显示。

画的位置也照上游：**在轨道和拇指之间**。这正是第二条规则成立的前提——
如果画在拇指上面，淡出就成了多余的装饰而不是必要的。
圆环是 `PaintingStyle.stroke` 描边而不是填充，
所以顺手补了 `_kOffLabelWidth` 这个常量（`OFF_STROKE`）——
它和 `_kOnLabelWidth` 数值相同、意思不同：一个是线宽，一个是竖线的宽度。

### 变异 7 个，全红

两个标记同一内缩；右到左不换端；两个标记同时实心；按住仍然显示；
透明度不随方向交换；不问 MediaQuery 一律画；干脆不画。
最后两条是走一遍抓的——同一个开关在设置开与关时画出的图形数量不同。

`CupertinoSwitch` 0.46 → **0.54（14/26）**。

尺子：十七把全部 exit 0。门：Rust 6905 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6905 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`CupertinoSwitch` 还剩下的成员里，
`focusNode`/`onFocusChange`/`autofocus`/`focusColor` 是一组——
和第 501–502 轮分段控件走过的**完全同一条路**（那时是 `leaf` 里放不下 `Focus`，
后来用 `many` 解决）。**先查一件事**：这个开关的 build 现在是
`crate::implicit::animated(...)` 包着一个 `leaf`，
`Focus` 能不能包在 `animated` **外面**（它返回的是 AnyWidget 吗）——
如果能，这一组就是把上一次的做法再用一次，不必再动结构；
如果不能，就照分段控件那样把 `leaf` 拆成 widget 树，
别在这里发明第三种写法。

---

## 第 506 轮：焦点环的颜色，只留下色相

上一轮问的是能不能把 `Focus` 直接包在 `implicit::animated` 外面。
查了：`animated` 返回的就是 `AnyWidget`，**可以**——
所以这一组不用像分段控件那样拆结构，一层包裹就够，
这正是"照着已有的那条路走"该有的样子。

补的是 `focusColor` / `onFocusChange` 与那个**焦点环**：

```dart
HSLColor.fromColor(activeColor.withOpacity(kCupertinoFocusColorOpacity))
    .withLightness(kCupertinoFocusColorBrightness)
    .withSaturation(kCupertinoFocusColorSaturation)
    .toColor()
```

**只留下色相。** 蓝开关得到蓝环、绿开关得到绿环，
而两个环的明度和饱和度**完全一样**——
这才让一页上颜色各异的控件的焦点环看起来是同一种东西。
固定一个颜色的环会在某些控件上消失、在另一些上刺眼；
直接用控件自己的颜色又根本不像一个环。
透明度是在 HSL 往返**之前**加的，所以它活下来了——变异把它挪到之后，测试当场发现。

环的几何也是一条：`trackRRect.inflate(1.75)` 描边 3.5——
**膨胀量正好是线宽的一半**，于是描边跨在路径上时，
内侧那一半刚好落在轨道的轮廓上，环整个在轨道之外，轨道本身一点不动。

`Focus` 那一层顺带带来两件事：禁用的开关既不因点击取焦、也不是 Tab 停靠点
（和第 502 轮分段控件同一个闸门）；焦点变化通过 `on_focus_change` 报出去，
并写进状态——**环是画出来的，不是算出来的**，所以必须有人告诉这个 widget 它被聚焦了。

### 一个没接上的参数，删掉而不是留着

上游的 `autofocus` 我一开始加了字段，然后发现这个 crate 的自动聚焦是
**按作用域**授予的（`focus::autofocus_in` + 每帧一次的 `apply_pending_autofocus`），
节点级的"出现即取键盘"需要把顺序想清楚。
于是把字段删了，在 `with_on_focus_change` 的文档里写明为什么没有——
**留一个没人读的字段，比没有这个参数更坏**：尺子会放过它，读者会以为它管用。

### 变异 7 个：先 6 红 1 活，补一条测试后全红

活的是"禁用的开关仍是 Tab 停靠点"。补了两个开关排一列、
按一次 Tab 直接落到能用的那个的测试。

`CupertinoSwitch` 0.54 → **0.62（16/26）**。

尺子：十七把全部 exit 0。门：Rust 6910 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6910 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`CupertinoSwitch` 还剩 `trackOutlineColor` / `trackOutlineWidth`
（`WidgetStateProperty`，画在轨道外的另一圈线，默认 2.0）、
`thumbIcon`、两张 `thumbImage` 和它们的错误回调、`mouseCursor`、`dragStartBehavior`。
**先查一件事**：`trackOutline*` 是 `WidgetStateProperty<Color?>`，
按状态解析——这个 crate 的 `StateProperty` 上一轮刚在分段控件的光标上用过。
先确认 `WidgetStates` 里有没有 `Selected` 这个状态（上游的轨道轮廓正是按
`selected` 分别解析成开/关两个值的），有就照那条路补，
没有就先补状态本身，别在开关里手搓一个"开/关两个颜色"的字段对。

---

## 第 507 轮：轨道里面那圈线，以及"画布记下了、我没去读"

先查了上一轮问的：`WidgetState::Selected` **在**（`widget_state.rs` 里就有），
所以 `trackOutlineColor`/`trackOutlineWidth` 照 `StateProperty` 那条路补，
不用在开关里手搓一对"开/关颜色"字段。

于是有了 `CupertinoSwitch::states(value, focused, pressed, enabled)`：
上游维护一个 `states` 集合，在每次解析前后加减 `selected`，
**同一个属性因此能对开和关给出两个答案**——这正是它是状态属性而不是颜色的原因。
焦点和按下也在同一个集合里，所以一个开关可以在被按住时描不一样的边。

轮廓的几何有两条：
- **`if (trackOutlineColor != null)`**：只给宽度不给颜色，什么也不画。
- **内缩一像素**：描边跨在路径上，画在轨道自身的边上会有一半溢出到
  "开关这个形状"之外；内缩一格之后，两像素宽的描边外沿正好落在边界上。
  和上一轮焦点环的 `inflate(1.75) / stroke 3.5` 是同一道算术反过来用。

### 变异 7 个：先 5 红 2 活，两条活的是同一个毛病

活下来的是"忽略给定的线宽"和"画在轨道边上而不是里面"。
原因不在实现，在**我的测试只从画布上读了颜色**——
而 `Drawn::RRect` 一直记着 `left/top/right/bottom` 和 `stroke`。
**画布记下了，我没去读。**

改成把那两个圆角矩形取出来：填充的是轨道、红色的是轮廓，
断言轮廓的线宽正是属性给的 6.0（不是默认的 2.0），
四条边各比轨道内缩一格。补完全红。

这一条和第 498 轮"断言挑的点恰好是所有实现都一致的点"是同一类：
**测试读到的东西比可读的少，就会有一整类错误看不见。**

`CupertinoSwitch` 0.62 → **0.73（19/26）**。

尺子：十七把全部 exit 0。门：Rust 6915 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6915 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`CupertinoSwitch` 只剩 `thumbIcon`、两张 `thumbImage` 与它们的错误回调、
`mouseCursor`、`dragStartBehavior` 了。
**先查一件事**：`dragStartBehavior` 在这个 crate 里有没有落脚点——
`gestures.rs` 里 `DragStartBehavior`（`start` 对 `down`：拖动是从**手指落下**那一点
算起还是从**判定为拖动**那一刻算起）是否已经有类型和规则。
如果有，开关这边只是把它接上去；如果没有，那它是一条**跨很多控件**的规则，
应该补在 `gestures.rs` 而不是补在开关里——
和第 495 轮"滚动参数该有一个统一住处"是同一个判断。

---

## 第 508 轮：指针的形状，以及一条根本没接线的管道

上一轮问 `dragStartBehavior` 在哪。查了：**在 `tap_and_drag.rs`**，
连"两者何时才有差别"的规则都在（无人竞争时识别器立刻获胜，两个位置相同）。
而这个开关的拖动是**裸指针回调**，不是能接受一个 behaviour 的识别器——
所以这个参数在这个 widget 上**没有可变的东西**。这是个读数，不是活。

接着看 `mouseCursor`，查出一件更值得记的事：
`MouseTrackerAnnotation` 带着 `cursor` 字段躺在 services 里，
而**这个 crate 里没有任何渲染对象挂过一个**——`render.rs` 里连 cursor 这个词都没有。
也就是说，**任何 widget 都还没法把一个指针形状送上屏幕**。

所以这一轮补的是规则本身，并把"没接线"写在文档里：

```dart
if (states.contains(WidgetState.disabled)) return MouseCursor.defer;
return kIsWeb ? SystemMouseCursors.click : MouseCursor.defer;
```

**`None` 是 `MouseCursor.defer`**，不是"没有光标"，而是"由背后的东西决定"。
原生开关在**任何桌面上都不改变指针**；只有在**网页**上它才取那只手，
因为在网页上开关是一页里读者预期要点的东西之一。禁用时连网页上也让开。
调用者给了自己的属性就**整个替换**默认（包括禁用那一支），
不是和默认合并——变异把它改成合并，当场被抓。

按第 506 轮那条教训，这次不加没人读的字段：规则是自由函数，
文档里点名"没有哪条管道能让它上屏"，并说明**那是整个 crate 的活，不是这个 widget 的**。

### 记账

`CupertinoSwitch`（504–508，0.31 → 0.73）记进 `depth_examined.json`，
`finding` 三类分明：这五轮补掉的四组、在别处的两个（`dragStartBehavior`、`focusNode`）、
**确实没有对应物**的三组（`mouseCursor` 缺管道、`thumbIcon` 与两张 thumbImage 缺图标字体与图片加载、
`autofocus` 只在作用域一级）。

变异 6 个全红（含"删掉这条记录，`--examined` 就没有它了"）。

尺子：十七把全部 exit 0。门：Rust 6917 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6917 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：新队头 `WidgetsApp`（0.31，13/42）。
第 475 轮补过它的 `RouteConfiguration`/`RouterConfiguration`，所以这不是新地方。
**先查一件事**：那 42 个成员里有多少是**本地化与语言**那一族
（`localizationsDelegates`、`supportedLocales`、`localeResolutionCallback`、
`localeListResolutionCallback`、`onGenerateTitle`…）——
第 472–477 轮刚给 Cupertino/Material 两个 App 补过 localizations delegate 那条链，
如果 `WidgetsApp` 的这一族已经被那几轮覆盖，就该按"在别处"记；
如果没有，那 locale 解析的**回退顺序**（先问回调、再逐个匹配语言/国家/脚本）
是一条真规则，值得单独一轮。

---

## 第 509 轮：手里握着两个回调的人，从来没被问过

按上一轮说的先查 `WidgetsApp` 那 42 个成员里的本地化一族。
`localizations.rs` 里**全都在**：`Locale`、`basic_locale_list_resolution`
（四张索引、倒序建表让**靠前**的受支持语言胜出）、
`resolve_locales`（列表回调 → 单个回调 → 算法）、`LocalizationsResolver`
（显式 locale 也要过一遍解析、`update` 只在受支持集合变了才重算、
delegates 的追加顺序）。看起来是"在别处，记一笔"就完了。

但把 `LocalizationsResolver` 逐行读下来，发现一处真缺口：
**它两条路都直接调 `basic_locale_list_resolution`，绕过了 `resolve_locales`。**
也就是说这个 resolver **根本不持有那两个回调**——
上游 `_updateResolvedLocale` 和 `locale` getter 都走 `_resolveLocales`，
应用写的 `localeResolutionCallback` 在这里**永远不会被调用一次**。
`resolve_locales` 这个函数移植得很仔细、有测试，然后没有任何人用它。

补上之后有三条规则被钉住：

- **平台那条路**要问回调（现在会了）；
- **显式 locale 那条路也要问**——上游是
  `_resolveLocales(<Locale>[_locale!], supportedLocales)`：
  应用自己的 locale 是一个**只有一项的偏好列表**，走同一套解析。
  所以一个要求了自己不支持的语言的应用，得到的回退和读者要求它时**一模一样**；
- `update` 只在受支持集合变了才重算，这留下一个**看起来像 bug 的不对称**：
  换一个回调**立刻**改变显式 locale 的答案，却**完全不影响**平台那条路，
  直到下一次系统语言到达。因为显式那条每次读都跑回调，平台那条在语言到达时跑过了。
  这一条单独写了测试，免得下一个读者把它当成漏掉的刷新。

### 变异 5 个，全红

两条路各自绕过回调；显式 locale 直接照用不解析；
`update` 丢掉传进来的回调；`update` 无条件重算。

尺子：十七把全部 exit 0。门：Rust 6922 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6922 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`WidgetsApp` 自己身上还差一件与此直接相关的：
它的 `supported_locales` 是 `Vec<String>`，而 `localizations.rs` 有真正的 `Locale`
类型和上面这套解析——**这个 app 拿着字符串，没法把它们交给解析器**。
**先查一件事**：`WidgetsApp::validate` 是否检查了
`assert(supportedLocales.isNotEmpty)`（字段注释提到了这条断言，但要确认代码里真的有），
以及 `Locale` 是否 `Clone + PartialEq`（换字段类型时要用）。
然后把这个字段换成 `Vec<Locale>` 并接上 `LocalizationsResolver`——
这样"应用支持哪些语言"就只有一个住处，而不是一个字符串列表加一个真类型。

---

## 第 510 轮：一个应用支持哪些语言，只该有一个住处

先查了上一轮的两件事：`WidgetsApp::validate` **确实**有
`supportedLocales must not be empty` 这条；`Locale` 是 `Clone + PartialEq`。
于是把 `supported_locales` 从 `Vec<String>` 换成 `Vec<Locale>`，
补上 `locale` 字段，并加了 `WidgetsApp::localizations(...)`——
上游 `_WidgetsAppState` 正是**从 widget 造出** resolver 而不是把它存在 widget 里，
所以这里也返回一个：widget 每帧重建，resolver 活着并保存着已解析的 locale。

### 一个构造顺序，被测试当场抓住

第一版我写成 `LocalizationsResolver::new(...).with_callbacks(...)`，
测试立刻红了：`new` 在**构造时**就解析了一遍平台 locale，
而回调是**之后**才装上去的——第一次解析没有它们。
上游的写法说明了原因：回调在**初始化列表**里赋值，构造函数体才解析，
所以第一次就带着它们。

改法不是"记得先装回调"，而是**把回调变成 `new` 的参数**，
让那个顺序**不可表达**。这和第 489 轮把两个枚举合成一个是同一个动作：
能被写错的顺序，最好让它无法写。

### 变异 4 个，全红

第一次解析绕过回调；app 不把自己的 locale 列表交出去；
app 不传自己的 `locale`；app 把回调丢掉。

`WidgetsApp` 0.31 → **0.36（15/42）**。

尺子：十七把全部 exit 0。门：Rust 6925 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6925 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**顺手记一条给自己的规矩**：这一轮又被 heredoc 咬了一次——
`bash <<'PY'` 里的 Python 字符串里写 `\n`，会被当成真正的换行，literal 就断了。
第三次了。**变异脚本一律用编辑器写，不要用 heredoc。**

**下一步**：`WidgetsApp` 剩下的 27 个里，`shortcuts` / `actions` 是最大的一族——
上游的 `WidgetsApp.defaultShortcuts` 是一张**按平台分的**大表
（`_defaultShortcuts` 加 `_defaultWebShortcuts` 加 macOS 的一套），
`defaultActions` 是与之配对的意图表。
**先查一件事**：`shortcuts.rs` 里有没有这张默认表
（`grep -n "DEFAULT_SHORTCUTS\|default_shortcuts" shortcuts.rs`），
以及它是否按平台分。如果表在而 App 没接，那是"接上去"；
如果表根本没有，那要先看清上游那张表有多大——
它可能值好几轮，而不是一轮里塞进去。

---

## 第 511 轮：默认快捷键的三张表，以及 Tab 从来没有移动过焦点

查 `shortcuts.rs` 有没有默认表：**没有**，只有一个
`default_traversal_registry()`。而它的文档写着"Tab to next, Shift+Tab to
previous"，代码里两条却都是 `Intent::Activate`——
**Tab 什么也没移动，只是按下了当时有键盘的那个东西。**
注释和代码各说各的，这一轮先把它改对。

然后把上游那三张表补齐，它们的差别本身就是规则：

- **桌面（Android/Fuchsia/Linux/Windows）**：方向键**移动焦点**，
  Control+方向键才滚动。桌面上方向键属于当前有焦点的东西（列表、菜单、输入框），
  所以滚页面得用修饰键来要。
- **网页**：裸方向键**滚动**（浏览器里的每一页都这样，Flutter 页面要是拿方向键做遍历就是异类）；
  **空格是两个意图按顺序**——先激活有键盘的东西，不行就翻一页，
  这正是浏览器的行为，也是这张表里唯一一处一个键有两个意思；
  **回车只按按钮**（`ButtonActivateIntent`），因为网页上输入框里的回车是换行或提交。
- **Apple（iOS/macOS）**：和桌面那张一样，只是**Meta 取代 Control**——
  滚动的修饰键跟平台的习惯走，而不是跟键盘上的字母走。

选择器的形状也是规则：**先问是不是网页，再问平台**。
Flutter 在网页上首先是网页、其次才是 macOS——
一个 Mac 上用浏览器的读者应该得到浏览器的方向键和空格，
反过来问会给他 Meta+方向键这种浏览器自己都不做的滚动。

顺带补了 `Intent::DirectionalFocus`（这个 crate 之前没有这个意图，
只有 `NextFocus`/`PreviousFocus`——**"下一个"是阅读顺序，"向下"是屏幕方向**，不是一回事）。

### 变异 11 个：先 9 红 2 活，两条都是我的测试没测到点子上

"Apple 走了 Control 那张表"活着——因为我在测 Apple 时直接调了
`default_apple_shortcuts_table()`，**没有经过选择器**；改成从
`default_shortcuts(MacOS, false)` 问 Meta 和 Control 之后就红了。
另一条是格式化后字符串对不上，重写即可。

### 又被同一个坑咬了

heredoc 里的 `\n` 又被吃掉一次——上一轮我刚给自己写下这条规矩。
这次连补丁脚本也改用编辑器写了。**记下来不算数，做到才算。**

尺子：十七把全部 exit 0。门：Rust 6932 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6932 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：这三张表还差几行没抄——`gameButtonA` 与 `numpadEnter`
（桌面表里的激活键），因为 `keys.rs` 里没找到对应的 `LogicalKey` 常量。
**先查一件事**：`grep -n "GAME_BUTTON\|NUMPAD" keyboard/keys.rs` 看清楚
**逻辑键**那一段有没有它们（物理键那一段是有 `NUMPAD_ENTER` 的）。
如果逻辑常量缺失，那补的是键表而不是快捷键表——
键表是从上游 `logical_key_data` 生成的，得看清是漏了几个还是整段没生成，
再决定这是一轮的活还是一行的活。

---

## 第 512 轮：一个宿主能送上来、却没有名字的键

上一轮问：`numpadEnter` / `gameButtonA` 的**逻辑键常量**在不在。
查下来是一条比"缺两行"大得多的事：

`keys.rs` 是 `gen_key_map.py` 从**Windows** 的 `flutter_key_map.g.cc` 生成的。
Windows 把小键盘的 Enter 和主 Enter 报成**同一个逻辑键**（`enter`），
所以 Windows 表里根本没有 `numpadEnter` 这个逻辑值——名字自然也就没有。

但这个 port 还有 **GTK 宿主**。数了一下：
GTK 的 `gtk_keyval_to_logical_key_map` 有 171 行，
其中 **62 个逻辑值在 `keys.rs` 里没有名字**——
`KP_Enter`、Copy、Paste、Redo、Super、Hyper、altGraph……
**一个 Linux 读者按下的键，框架里没有名字，也就没有任何东西能给它绑快捷键。**

### 补在生成器上，不是补在生成的文件里

C 数组是"Windows 宿主拿 Windows 的键去查的表"，只该是 Windows 的；
而 Rust 常量是"应用拿来绑快捷键的名字"，应用跑在**每一个**宿主上。
所以生成器多收一个参数（GTK 的映射），**只取它的逻辑值**，
名字则从上游 `keyboard_key.g.dart` 按**值**反查——
GTK 注释里是 X11 的名字（`KP_Enter`、`3270_Copy`），不是 Flutter 的；
上游没有名字的值就不给名字，**宁可留一个无名值，也不要一个我编的名字**。

重新生成后：**C++ 那张表逐字节未变**（它本来就该只有 Windows），
Rust 常量从 159 涨到 217。

### 一处漂移，顺手收回生成器

重新生成时发现四个手写的常量被冲掉了：`CONTROL`/`SHIFT`/`ALT`/`META`
——那四个"不分左右"的同义键，没有哪个宿主会送，所以不在任何映射表里，
当初是**手写进这个生成文件**的。`platform_menu_bar.rs` 正在用它们。
手改生成文件就是这样：下一次生成把它抹掉。
现在它们由生成器发出，`SYNONYMS` 那一段写明了为什么它们不在表里。

### 三张快捷键表接上新名字

`numpadEnter` 在桌面与 Apple 表里是 `Activate`、在网页表里是 `ButtonActivate`
（和主 Enter 一致）。

### 变异 4 个，全红

生成器不读 GTK 映射 → 生成的文件里没有 `NUMPAD_ENTER`；
去掉 `SYNONYMS` → 四个同义键消失；
两张表各自去掉那一行 → 测试红。
前两条是**跑生成器**去验的，不是跑 cargo——被测的是工具，就得考工具。

尺子：十七把全部 exit 0（`stale_engines` 在重新生成后、重建之前红了一次，
这正是它该做的：FFI 源比引擎新）。门：Rust 6933 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6933 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`gameButtonA` 仍然没有名字——GTK 也不送手柄按键，
它来自 Android 的 `KeyEvent`。**先查一件事**：
`src/flutter/rust/host` 下有没有 Android 那一侧的键映射
（`ls src/flutter/rust/host | grep -i android`，或 embedding 里的 Java/Kotlin），
以及它是不是也从某个 `.g.` 表生成。
如果有，那就是给生成器再加一个来源，和这一轮同样的做法；
如果 Android 那一侧根本不走这张表，那 `gameButtonA` 就是"这个 port 的宿主都送不出的键"，
该记一笔而不是硬造一个名字。

---

## 第 513 轮：Tab 归表管；以及没有一个宿主在送键

先查 Android 那侧有没有键映射：**没有**。
`RustflutterActivity.onKeyDown` 只做三件事——编辑键送进文本域、
Enter/NumpadEnter 触发 editor action、可打印字符当文本送上去。
没有 keyCode→逻辑键的表，也没有把键送进框架的路。
所以 `gameButtonA` 是"这个 port 的宿主都送不出的键"，该记一笔而不是硬造名字。

> **这一段是错的，第 514 轮更正。** 下面"没有任何宿主调用"一句不成立：
> 调用点在 `runtime/runtime_controller.cc`，桌面宿主一直在送键。
> 当时的 grep 只搜了 `rust/` 一个目录。

**顺着这条线查出一件更大的事**：`rf_app_dispatch_key` 这个 FFI 入口
**没有任何宿主调用**（只有 `ffi_unittests.cc` 调过）。
也就是说框架的整条按键通路——三张快捷键表、`Focus` 的 `on_key`、Tab 遍历——
**只有测试和这个入口能到达**。今天键盘到达应用的路只有两条：IME 的文本，
和文本域的几个编辑键。这句话写进了 `default_shortcuts` 的文档，
免得下一个读者去猜"为什么 Windows 上按 Tab 没反应"。

### 这一轮做的那件事：Tab 不再自己判断

`focus::handle_traversal_key` 原本自己写着 `logical == TAB` 并去问 `keyboard.shift()`
——**同一条规则的第二份**，而且这一份不知道网页表的存在。
改成去问 `default_shortcuts(host(), false)`，按回答的意图行动。

变异扫描逼出了三条测试的不足，全是"两份规则等价所以看不出差别"那一类：

- **Ctrl+Tab**：手写版只看 Tab、不管别的修饰键，所以 Ctrl+Tab 也会遍历；
  `SingleActivator` 要求修饰键**完全一致**，所以它不是那一行。
  这正是两份规则**真正不同**的地方，补了测试。
- **Shift+Tab 真的往回走**：原来的测试只有两个节点，
  一前一后绕回同一处，`previous()` 改成 `next()` 看不出来。换成三个。
- **松开的键**：原来没有测试按下之外的事件。

第四条变异（去掉 `is_down` 判断）活着，因为**它本来就是多余的**：
每个 `ShortcutActivator::accepts` 开头就有同样的判断。
于是把这行删了，并把这条理由写在原地——
**一次没有可观察差别的删除，正是那行不值得留下的定义**。
改成在 activator 那一层做同样的变异，测试立刻红。

尺子：十七把全部 exit 0。门：Rust 6937 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6937 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：把这条通路接上——**从 Windows 宿主开始**，因为它已经在
`rustflutter_host_win.cc` 里把 `WM_KEYDOWN` 拆成了 `event.editing_key`
（第 2483/2888 行那一带），也就是说**键已经在手里了**，只是没往框架送。
**先查一件事**：`RfKeyEvent` 这个结构在 FFI 头里长什么样
（`grep -n "RfKeyEvent" -A12 src/flutter/rust/host/rustflutter_host.h` 或 ffi 头），
以及 Windows 那边现成的 `key.virtual_key` / scan code 能不能直接喂
`LogicalKeyForVirtualKey`（那个函数就在 `rustflutter_key_map_win.cc` 里，
本轮之前就有）。如果能，这一轮的活就是"把已经算好的值送过去"，
而不是重新做一套键映射。

---

## 第 514 轮：上一轮那句话是错的——键一直在到达框架

上一轮写下"没有任何宿主调用 `rf_app_dispatch_key`"，并把这句话写进了
`default_shortcuts` 的文档。**这句话不成立。** 顺着"下一步"去查 `RfKeyEvent`
的结构，查出来的第一件事就是它的调用点：

- `src/flutter/runtime/runtime_controller.cc` 的 `DispatchKeyDataPacket`
  把 `flutter/keydata` 平台消息拆成 `RfKeyEvent`，
  调 `RustApp().dispatch_key(app_, &event)`，再把返回值 `answer(handled)` 回去；
- Windows 宿主 `rustflutter_host_win.cc` 的 `SendKeyEvent` 早就用
  `PhysicalKeyForScanCode` / `LogicalKeyForVirtualKey` 填好了 `KeyData`
  并发出那条消息，框架不要的键再 redispatch 回系统。

也就是说：**桌面上按 Tab 是能走到三张快捷键表和焦点遍历的**，
真正没有键路的只有 Android（`RustflutterActivity.onKeyDown` 仍旧只做编辑键）。
上一轮那条 grep 只看了 `rust/` 一个目录，**漏掉一个目录的搜索，
是关于这次搜索的结论，不是关于代码的结论**。

### 这一轮做的那件事：把这条通路的第一步搬到能被检查的地方

`RfKeyEvent` → `KeyEvent` 的翻译原本躺在 `mod abi` 里，
而 `mod abi` 是 `#[cfg(not(test))]` 的——那些 `#[no_mangle]` 入口在不链接引擎的
测试二进制里会留下未定义符号。于是**这条规则只存在于没有测试的那个 build 里**。
把 `RfKeyEvent` 和新提出来的 `key_event_from` 挪出 `mod abi`：
它是一条规则，不是 ABI；字符的取用（空串与空指针都折成 `None`）也一并收进去，
`rf_app_dispatch_key` 剩下 null 检查和一次调用。

规则本身写在原地：**空字符串就是没打出字**。框架各处拿
`character.is_some()` 当"这一下敲出了东西"用，`Some("")` 会声称打了字却什么也不插。

两条测试：一条走翻译本身（重复键、physical/logical、synthesized、时间戳，
以及空指针和空串两种"没打字"的说法都必须变成 `None`），
一条把翻译出来的 Tab 交给焦点遍历，断言它答 `true`（宿主据此不把键塞回系统队列）
且焦点落到第一个节点。

六个变异全部杀死。其中"不检查空指针"这一条**打崩了整个测试二进制**
（0xc0000005），和隔壁那些 null-app 守卫是同一种形状：
**这类守卫写错不会变红，会变成崩溃**——扫描能看见，正说明它值钱。

上一轮那段错话在 `shortcuts.rs` 里换成了"# How a key gets here"，
写清真实路线并点名 Android 是例外；`PORTING_STATUS.md` 第 513 轮那一段
也当场加了更正框，**一条留在原地的错记录比没有记录更糟**。

尺子：十七把全部 exit 0。门：Rust 6939 通过、`cargo fmt --check` 干净；
C++ 34 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6939 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：Android 的键路——真正的缺口在这里。
`RustflutterActivity.onKeyDown` 现在只认编辑键，没有 keyCode→逻辑键的表。
**先查一件事**：上游 `shell/platform/android/io/flutter/embedding/android/`
下的 `KeyboardMap.g.java` 是怎么生成的（`keyCode`/`scanCode` 两张表），
以及本 port 的 `gen_key_map.py` 能不能顺手多吐一张 Android 表——
如果能，这一轮又是"把已经有的映射送过去"，而不是新做一套。

---

## 第 515 轮：Android 的键映射——上一轮点名的那个真缺口

上一轮查清桌面宿主一直在送键，只有 Android 没有键路。这一轮补上映射那一半。

### 表从哪来

上游 `KeyboardMap.java` 由同一次 `gen_keycodes.dart` 生成，
和 Windows 那张表是同一份数据的另一面，不是第二个真相来源。
于是 `gen_key_map.py` 多收一个参数（第五个），吐出
`rustflutter_key_map_android.cc`：`scanCodeToPhysical` 232 条、
`keyCodeToLogical` 260 条，仍是排序数组加二分查找。
`rustflutter_key_map_win.cc` 生成结果**逐字节不变**。

规则照 `KeyEmbedderResponder.getPhysicalKey/getLogicalKey` 抄，不是猜的：

- **scanCode 为 0** 不是键盘上的键，是根本没经过键盘的事件——
  `adb shell input keyevent` 和模拟器都这样。此时从 keyCode 造物理键。
  理由不是整洁：两个键共用一个物理值，一个的松开会取消另一个的按下，
  框架就会以为有个没人按着的键还按着。
- 表里没有的键，保留自己的号码并移进 Android 平面，
  免得撞上真正的 Unicode 码点。
- 逻辑键只要 keyCode——Android 送达前已经把布局解好了，
  这点和 Windows 不同（Windows 要 scan code 才能分清左右 Shift 和小键盘）。

### 一处险些酿成的重名

Android 的表**给字母和数字起了名**，而生成器末尾又用算术补
`KEY_A`/`DIGIT_0`。第一次生成出来是 `KEY_A` 声明两次（根本不编译）
和 `DIGIT0` 与 `DIGIT_0` 并存（一个值两个名字，正是漂移的起点）。
改法是把算术那段挪到按值合并**之前**，让它先占住这些值；
并在末尾加一句对所有名字的断言：值不许重复、标识符不许重复。
**生成器写错这两样都是静默的**，它会老老实实写出来。
现在 keys.rs 有 157 个物理名、323 个逻辑名（上一轮 217）。

### 接上宿主

- Android 平台视图有了 `SendKey`，形状照 GTK 宿主：一条 `flutter/keydata`
  平台消息，**不等答复**。Windows 等，是因为窗口过程是键与默认处理之间唯一的东西；
  Android 的 `onKeyDown` 必须当场答复，而框架的裁决要晚得多才到平台线程。
  上游用"把事件再投递一次"解决，那是另一套机制，还没有。
- JNI `nativeKey(keyCode, scanCode, down, repeat, character)`。
- Java 那侧 `onKeyDown`/**`onKeyUp`** 都走同一条 `handleKey`：
  先把键送上去，再（仅按下时）做原来的编辑/回车/文字。
  **`onKeyUp` 以前根本没有**——只有按下没有松开，框架会以为键一直按着。

顺手修掉一个真 bug：原来 `getUnicodeChar() != 0` 就当文字送上去，
而 Tab 的 unicode 是 0x09、Esc 是 0x1b——**硬键盘按 Tab 会往输入框里打一个制表符**。
按 GTK 宿主同一条规则收紧：控制字符是"这个键是什么"，不是"它打出了什么"；
按住 Ctrl/Alt 也一样，那是快捷键不是文字。

### 扫描

五个变异全部杀死，其中**两个是第二轮才杀死的**，都是我测试本身的漏洞：

- 回退探针的 scanCode 和 keyCode **用了同一个数**，
  于是"物理键回退用错另一个号码"这个变异看不出差别——
  **在所有实现都一致的地方断言，什么也证明不了**。
- 探针 0x1fff **在表的范围之外**，二分查找直接跑到末尾，
  那个"找到的真是我要的吗"判断根本没被考。
  补了一个**表中间的空洞**（scan 0x54、keyCode 0x4e），
  查找会落在真实的邻居行上，只有那个判断挡着。

**没有接设备**：这一轮 `adb devices` 是空的，所以 Java 与 JNI 那一半
只做到"能编译"（javac 对 android.jar、arm64/x64 都过），
键真正走完全程没有在真机上看过。这句话记在这里，免得下一轮当成已验证。

尺子：十七把全部 exit 0。门：Rust 6939 通过、`cargo fmt --check` 干净；
C++ 38 个 gtest 全过（新增 4 个）；gallery 357 通过；
`rustflutter_unittests` 6939 通过；三个目录 default 与 `rustflutter_engine` 都 exit 0。

**已知重复，写下来而不是半做**：`kValueMask` 这个常数在四个 key map 头文件里各有一份。
新的 Android 头**没有**再加一份（它是唯一到处都编译的，最可能撞上别人的那份），
放进了 .cc。真正合并要动 mac 和 linux 的表，这台机器既不能编也不能跑它们。

**下一步**：接一台设备，把这条路走完。
**先做一件事**：`adb devices` 有没有那台三星（只有它收合成输入）；
有的话用 `make_apk.py` 打一个 gallery 的 APK 装上去，
`adb shell input keyevent 61`（KEYCODE_TAB，scanCode 为 0，正是本轮那条分支）
看焦点框是否移动，`adb shell input keyevent 111`（ESCAPE）看是否**不再**往输入框里打字符。
没有设备就换一条：Android 的**修饰键同步**（`pressingGoals`/`togglingGoals`）——
`metaState` 说 Shift 按着而框架没收到过那次按下时，上游会补一个合成事件；
本轮把 `metaState` 整个丢掉了，那是下一个真缺口。

---

## 第 516 轮：Android 的修饰键同步——框架以为按着的，和真按着的

`adb devices` 仍然是空的，所以走上一轮记下的另一条：`metaState`。
上一轮把它整个丢掉了。

### 这为什么是个真缺口

框架的按下集合完全由收到的事件建立，所以它**只能和事件流一样完整**——
而 Android 的事件流不完整。应用启动时 Shift 已经按着、
松开发生在别的窗口上、修饰键被系统吃掉：
每一种都留下有按下没松开或有松开没按下。
于是 Shift+Tab 往前走，或者松手之后每个快捷键都还带着 Shift。

Android 每个事件都带 `getMetaState()`，说它认为哪些修饰键按着。
所以每个事件都是一次对账的机会：先补上让两边一致的合成按下/松开，
再发真事件。这就是上游 `KeyEmbedderResponder`，
本轮把它连同 `pressingRecords` 一起搬进 `rustflutter_key_sync_android.cc`。

**为什么记录放在宿主而不是问框架**：键是平台消息，在平台线程上处理，
决定要发什么的时候够不到框架那份集合。上游同理，同因。

### 表也是生成的

`pressingGoals`（Ctrl/Shift/Alt 各两个键，**不含 Meta**——上游不做，这里也不做）
一并由 `gen_key_map.py` 从 `KeyboardMap.java` 生成。
掩码名字是 Android 常量，C++ 拼不出来，**值是从平台 jar 里读出来的，不是记出来的**：

    javap -classpath <sdk>/platforms/android-36.1/android.jar \
          -constants android.view.KeyEvent

上游只用无侧别的位（`META_SHIFT_ON`，不用 `META_SHIFT_LEFT_ON`），
因为 ChromeOS 把右修饰键报成 UNSIDED | LEFT_SIDE，侧别位比没有还糟。

### 顺带纠正上一轮做错的三处主事件规则

照 `handleEventImpl` 读下来，上一轮有三处是我自己想的，不是上游的：

- **重复不由 `getRepeatCount()` 一个人说了算**：物理键不在记录里，
  哪怕 repeatCount 大于零也是首次按下。
- **没见过按下的松开要丢掉**，不能往上送——那会让框架从按下集合里
  移走一个不是它放进去的键。
- **同一个物理键连着两次按下**（且不是重复）说明松开丢了，
  要先补一个松开，否则框架记下同一个键按了两次。

还有一条上游明写的：**只有按下才打字**，松开不带字符。

on-screen 键盘（Gboard）**相信而不纠正**：它把 Shift 位置上却从不发 Shift 事件，
补出来的那次按下将永远等不到松开。

### 扫描

九个变异全部杀死。第九个第一次写成 `if (false)`，**根本不编译**——
不编译的变异什么也不能证明，换成把条件反过来（`==` 代替 `!=`）才算数。

一条测试是我自己写错的：第二次按 Tab 时前一次的 Tab 还记着按下，
于是走了"已按着的键先补松开"那条分支，多出一个事件。
**测试没写错的地方，是代码在做正确的事**——改成中间先松开 Tab。

尺子：十七把全部 exit 0。门：Rust 6939 通过、`cargo fmt --check` 干净；
C++ 48 个 gtest 全过（新增 10 个）；gallery 357 通过；
`rustflutter_unittests` 6939 通过；三个目录 default 与 `rustflutter_engine` 都 exit 0。
Java 用 javac 对 android.jar 编译通过。**仍然没有真机**。

**下一步**：`togglingGoals`——CapsLock。本轮只做了按住类修饰键，
锁定类一个没做，`KeyboardMap.getTogglingGoals()` 就在同一个文件里。
**先读一件事**：`synchronizeTogglingKey` 为什么要**连发两个**事件
（按下再松开，或松开再按下），以及它为什么**跳过 CapsLock 自己的事件**——
上游注释说 ChromeOS 上 CapsLock 自己的事件 metaState 是"按住"语义而非"锁定"语义。
这条规则不读注释是抄不对的。
另外框架那边 `Keyboard` 有 `lock` 状态（`keyboard/mod.rs` 第 218 行附近，
注释说它不能从 pressed 推导），要确认合成事件到达时它是否真的会翻转。

---

## 第 517 轮：CapsLock——锁不是修饰键，所以要发一对事件

接上一轮的"下一步"：`togglingGoals`。

### 先读懂那两条规则，再动手

**为什么发两个事件**：锁和修饰键的差别就在"没人碰它的键时它也开着"，
所以框架读不出来——它是**在锁键的每一次按下时翻转**
（`keyboard/mod.rs` 的 `record`，注释写着"A key down, not a key up, and not a
repeat"）。于是单发一个合成事件都不行：发松开什么也不翻，
发按下则翻了之后把 CapsLock 永远记成按着。必须一按一松成对。
先按还是先松，看这个键此刻记录里是不是按着。

**为什么跳过锁键自己的事件**：上游注释点名 ChromeOS——
那里 CapsLock 自己的事件把这个位当**按住**语义用（按下为 1、松开为 0），
而别的事件都把它当**锁定**语义用。拿一个在这一个事件上含义不同的位去对账，
会把锁多翻一次。锁键自己的按下另走一条路翻转，就在决定完事件类型之后。

**这两条不读注释是抄不对的**，这也是上一轮把它单独留给这一轮的原因。

`getTogglingGoals()` 只有 CapsLock 一条。NumLock 和 ScrollLock 不在，
上游说了原因：ChromeOS 上按它们根本不置位，
盯一个永不变化的位的 goal 要么什么都不做，要么永远打架。
这条也一并生成，掩码同样来自 `javap`。

### 顺手记下的一件事

CapsLock 的 scan code 是 0x3a，而 keyCode 0x3a 是 **altRight**。
两个号码空间毫无关系，测试里把这句写在常量旁边——
这正是"一个号码顶另一个用"最容易出事的地方。

### 扫描

六个变异全部杀死。其中"一个事件代替一对"和"先松后按"
分别打中两条测试，说明这对事件的**顺序**和**数量**都真被看着。

尺子：十七把全部 exit 0。门：Rust 6939 通过、`cargo fmt --check` 干净；
C++ 51 个 gtest 全过（新增 3 个）；gallery 357 通过；
`rustflutter_unittests` 6939 通过；三个目录 default 与 `rustflutter_engine` 都 exit 0；
javac 对 android.jar 编译通过。**仍然没有真机**（`adb devices` 空）。

顺带记一次老毛病：本轮又在 bash heredoc 里写 Python 字符串字面量，
`"\n".join` 被吞成真换行，生成器直接语法错误。**第五次**。
这条规则已经写过：**改脚本用 Write/Edit 工具，不要走 heredoc**。

**下一步**：Android 键路还差最后一块——**框架不要的键要还给系统**。
现在 `handleKey` 无论如何都 `return true`，于是硬键盘上的音量键、
返回键之类会被应用吞掉。上游用的是"再投递一次"（redispatch）：
`KeyboardManager` 把框架不要的事件重新塞回 `Activity.dispatchKeyEvent`，
并用一个"这是我自己发回来的"标记避免死循环。
**先读一件事**：`shell/platform/android/io/flutter/embedding/android/KeyboardManager.java`
里那个重投递是怎么标记的（是不是靠一个 `redispatchedEvents` 集合），
以及本 port 的 `SendKey` 现在**不等答复**——要接这条路，
就得像 Windows 宿主那样带上 response 回调，那是这一轮真正的代价，
需要先想清楚 `onKeyDown` 必须当场返回而答复晚到之间怎么办。

---

## 第 518 轮：框架不要的键要还给 Android

Android 键路的最后一块。上一轮之前 `handleKey` 一律 `return true`，
于是**音量键、媒体键这些系统默认处理的键都被应用吞掉了**。

### 难点是时序，不是通路

`onKeyDown` 必须当场返回，而框架的裁决要晚得多才从平台线程回来。
上游的办法是：**先全要下来，等答复说"不要"，再把这个事件从 Activity 顶上投递一遍**，
并用一个"这是我自己发回来的"标记避免死循环。这一轮照抄这套。

- C++ `SendKey` 加上 `sequence_id` 和 response 回调（Windows 宿主早就这么做）。
  答复一个字节；**空答复读作"没处理"**——没人要的键还给系统，
  比凭空消失安全。
- `JavaBridge::KeyResult` 把答复送回 Java 的 `onKeyResult`。
- Java 用 `SparseArray` 按序号存住原事件，`onKeyResult` post 到视图线程
  （答复来自平台线程，投递键事件必须在视图线程上），
  不要的就 `sRedispatched.add(event)` 再 `dispatchKeyEvent`。
  标记集合用 **IdentityHashMap 撑的 Set**：问的就是"是不是我放回去的那个对象"。
  `KeyEvent` 没覆盖 `equals`，今天两者等价——但**把问题写成它本来的样子**，
  哪天它不再等价也不会悄悄改变行为。

### 两个只有写的时候才会想到的坑

**序号只给真事件**。合成的修饰键事件背后没有原始 Android 事件可还，
答复无处可去。判据用 `synthesized` 标志，正是框架读它们的那个标志。

**被丢掉的键也得答复**。`Handle` 返回 false（异常松开、没号码的事件）时
一个事件都不发，没人会答复，而 Java 那边正握着这个键等着。
所以 JNI 里补一句"框架没见过它，那就是没处理"。
注意这条会**在 nativeKey 这一次调用里同步回调进 Java**，
所以 Java 必须先登记再发送——顺序写反就丢答复。

**本地吃掉的键不必绕这一圈**：编辑键/回车/文字这三条在 Java 这侧当场生效，
吃掉就把待答表项删掉，之后的答复找不到人，自然不会重投递。

### 能测的和不能测的

JNI 和 Java 这两半在这台机器上**测不了**（没有真机，测试二进制里也没有 JVM）。
但它们依赖的那条不变量能测，而且值钱：
**一次 Android 事件最多产生一个 `synthesized == 0` 的事件**。
如果哪天能产生两个，一个 Java 待答项就会收到两次答复，
第二次要么找不到人，要么撞上一个复用了同号的新键。
新增一条测试，把加修饰键、加锁、修饰键自己按下几种情况都过一遍数。

三个变异全部杀死（真事件被标成合成、丢弃的键谎报成功、没号码的事件谎报成功）。

尺子：十七把全部 exit 0。门：Rust 6939 通过、`cargo fmt --check` 干净；
C++ 52 个 gtest 全过（新增 1 个）；gallery 357 通过；
`rustflutter_unittests` 6939 通过；三个目录 default 与 `rustflutter_engine` 都 exit 0；
javac 对 android.jar 编译通过。**仍然没有真机**。

**下一步**：Android 键路四轮下来（映射、按住修饰键、锁、还键）
在代码上已经闭环，但**一次都没在真机上跑过**。
先看 `adb devices`：有那台三星就打 APK 装上去，
用 `adb shell input keyevent 61`（Tab，scanCode 为 0 那条分支）看焦点是否移动，
`input keyevent 24`（音量加）看是否**仍然调音量**——那正是本轮修的东西，
也是最容易一试就露馅的一条。
没有设备就换方向：`depth.py` 队头的 `SelectableText`（0.32，11/34），
它和刚做完的选择/键盘那块是连着的。

---

## 第 519 轮：`showCursor`——代码自己招认的那个洞

`adb devices` 还是空的，于是转 `depth.py` 队头的 `SelectableText`。
不用找缺口：它的文档注释里明写着

> **`show_cursor` is not honoured.** ... the missing piece is in the field, not here.

**一个存下来却哪儿也不去的字段**。选中一段文章后，中间会闪一个光标——
那正是"一个马上要被打字的输入框"的样子，而这个控件恰恰不是。

### 上游的默认值不是常量

`EditableText` 写的是 `showCursor = showCursor ?? !readOnly`。
能打字的字段要指出下一个字符落在哪儿；只读的没有下一个字符。
所以字段那侧加的是 `Option<bool>`，`None` 就是上游的 null，
`cursor_shown()` 是那一行本身。

**两处要问同一个问题**：绘制（画不画光标）和眨眼时钟（有没有事可做）。
写成一个方法而不是两处判断，否则"只读但被要求显示光标"这种情况
两边会给出不同答案。

眨眼那一处也照上游做了：`_startCursorBlink` 在 `showCursor` 为假时
**直接返回，不启动定时器**。这里对应的是 `advance` 返回 false——
本 crate 里那就是"不要下一帧"。省的不是翻转，是**每半秒一帧、永远不停**。

`SelectableText` 现在把自己的 `show_cursor` 传下去，而且**必须显式传**：
字段自己的默认已经是 `!read_only`（对只读也是不显示），
但这是两条不同的规则碰巧同值，而上游允许 `showCursor: true` 把光标要回来。

### 测试里改对的一件事

第一版 `carets()` 按颜色数——用了 `Theme::light().primary`，结果一个也没数到：
裸树里的环境主题是 **dark**。改成按 **`CARET_WIDTH` 宽度**数：
颜色要求测试点名一个主题，而哪个主题是默认跟光标毫无关系，
**主题一改这测试就会为了不相干的理由变红**。宽度是绘制本身遵循的那条规则。

`SelectableText` 那条测试**跑真的眨眼时钟**（`focus` 开会话、`advance_frame`
走第一帧）而不是直接把标志设成 true——这样 `advance` 也在路径里，
"没有光标的字段根本不会把标志打开"这件事顺带被看着。

七个变异全部杀死，覆盖：绘制少判一次、时钟少判一次、默认值改成常量真、
改成常量假、忽略显式值、控件不往下传、控件永远要光标。

尺子：十七把全部 exit 0。门：Rust 6943 通过、`cargo fmt --check` 干净；
C++ 52 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6943 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`SelectableText` 还差得远（11/34），下一个真缺口按同样标准挑——
**先看 `selection_area.rs` 里还有没有第二处"存了但不用"的字段**：
`max_lines` 是用了的，`data` 是用的，但 `rich(spans)` 那条路
（`with_span_count` 收下参数后**原样返回 self**）就是同一类东西，
而且比 `show_cursor` 更彻底：它连字段都没有。
**先查一件事**：本 crate 的 `TextSpan`（`painting.rs`）能不能真的喂给
`RenderEditable`——如果字段那侧只认 `String`，那这一轮的活是在字段里，
和这次一样；如果能，那就是把 `rich` 接上去。

---

## 第 520 轮：`SelectableText.rich`——比上一轮那个洞更彻底的一个

上一轮记下的：`rich(spans: usize)` 收一个**计数**，`with_span_count` 原样返回 self。
比 `show_cursor` 更狠——那个至少还有字段，这个连字段都没有，
`SelectableText::rich(3)` 画出来是**一个空字符串**。

### 先查的那件事，答案是好消息

`RenderEditable` 只认一个 `String` 加一个 `TextStyle`。
但 `painting::shape_rich(&[(String, TextStyle)], ...)` **早就有了**，
而且整个字段的文字只走**两个口子**：一个 `measure()`，
每行一次 `draw_paragraph`。所以改动是收敛的，不是散的。

### 一次机械但必要的重构

`measure(&str)` 改成 `measure(Range<usize>)`。
所有调用点本来就是 `measure(&text[a..b])` 这个形状，
但**只有拿到范围才知道该用哪些 run 去量**。
`wrap_lines` / `caret_position_at` 的测量闭包一并改成收范围。

然后 `RenderEditable` 多一份 `runs: Vec<StyleRun>`（`{end, style}` 边界，空=单一样式）：

- `shape_range(range)` —— 有 run 走 `shape_rich`，没有走 `shape`。
  **不是统一走 rich**：那是同一个答案用更慢的方式算，还会打乱 shaping 缓存的键。
- `runs_in(range)` 把 run 切到范围上。**最后一个 run 之后的文字取基础样式**：
  run 是 build 时给的，而文字会在它们脚下变（字段可编辑，render object 不会跟着重建）。
  与其拒绝绘制，不如把没人描述的那段按"没有描述的字段"来画。
  `SelectableText` 从不编辑，永远走不到这条分支。
- `clamp_range` 把范围收到字符边界：切 `str` 切在半个字符上会 panic，
  而调用者里有拿平台报来的偏移量的。

绘制那一处**用同一个 `shape_range`**。注释写在原地：
量一套、画一套是"富文本行在错误的地方换行"的成因。

### 接上去

`TextField::with_runs` **顺带把开场文字也设了**，就从 run 里拼——
富文本没有另一个字符串可给，而两处各说一遍文字是什么就是两处要同步的东西。
`SelectableText::rich(Vec<TextSpan>)` 现在收真的 span，`data` 是它们拼起来的，
`widget()` 按有没有 run 走两扇门进同一个字段。

### 测试能证明什么，不能证明什么

写在测试里而不是含糊过去：**打桩引擎每段落只保留最后一次 push 的样式**，
所以这条测试能证明 run 确实走到了 `shape_rich`（画出来的颜色是**最后一个 run 的**，
而不是基础样式的），**不能**证明每个 run 各自用了自己的样式——那是真 shaper 的事。
**在这里断言"每段各有其样式"就是断言了没看见的东西。**

七个变异全部杀死：绘制绕过 run、有 run 却当没有、边界全归零、
字段不往下传、富文本文字另找来源、控件走平字段那扇门、`rich` 不留 run。

尺子：十七把全部 exit 0。门：Rust 6944 通过、`cargo fmt --check` 干净；
C++ 52 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6944 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

又一次栽在 bash heredoc 上（第六次）：这回是 `"ab\ncd"` 里的 `\n`，
外加一个正则里的转义括号。**改脚本一律用 Write/Edit 工具**——
这条规则已经写了两轮，这轮才算真的照做（`fixmeasure.py`、`richtest.py` 都是写成文件跑的）。

**下一步**：`SelectableText` 仍是 11/34，但下一个真缺口挑法不变——
**`style` / `text_align` / `text_direction` 三个是同一类**：
控件收下却没有任何去处，而字段那侧 `with_style` 是有的。
**先查一件事**：`RenderEditable` 有没有 `text_align`——
上面 `shape_range` 里我写死了 `TextAlign::Left`，
如果字段本来就不支持对齐，那是个**本轮新写下的、写死的假设**，
下一轮该先把它变成真的字段，而不是再往上堆。

---

## 第 521 轮：`textAlign`——先把上一轮自己写下的那个写死值变成真的

上一轮 `shape_range` 里写死了 `TextAlign::Left`。查证：字段里
**`text_align` 三个字一处都没有**，全文只有那一处写死。
也就是说这不只是我留下的债，本来就是个缺口：
`SelectableText.textAlign` / `TextField.textAlign` 都没有落点，
每一行永远从盒子左边开始画。

### 对齐是"每行往右挪多少"

这个 port 里所有横向几何都从 `measure(range)` 加一个 `base` 得来，
所以对齐就是**每行一个位移**：

- `align_shift(line_width, viewport)` 是那条规则本身。
  单独一个函数，因为**要对齐的有两样东西，而它们量法不同**：
  正文按字段自己的 run 量，占位符是另一个字符串、另一套样式。
  一条规则，问两次。
- `resolved_align()` 把 `Start`/`End` 按方向解开。
  **它们不是左右的别名**：RTL 段落里正好反过来，
  这也正是上游默认值是 `Start` 而不是 `Left` 的全部原因。
- `Justify` 是拉伸词间空白，本 crate 的 shaper 不拉伸任何东西，
  所以当 `Left` 处理并写明——**不假装**。
- 位移不为负：比盒子宽的行本来就靠滚动而不是靠左拉，
  再挪就把行首挪到够不着的地方。

绘制里每行的 `base` 加上自己的位移，**高亮、字形、拼写下划线一起动**——
它们描述的是同一批字形。`caret_rect` 也加，因为下游（把光标滚进可视区、
告诉 IME 光标矩形）读的都是内容坐标。

**点击那一半最容易忘**：手指指的是字形，字形挪走了。
位移随 `LineLayout` 一起交给点击处理器，而不是让它重算——
点击发生在绘制的下一帧，重算会用**那一帧**的宽度，
而手指指的是读者看见的那一帧。

`shape_rich` 那里仍传 `Left`，但现在写明了理由：
段落是孤立成形、由 `line_shifts` 摆位的，把对齐再喂给 shaper 会**挪两次**。

### 扫描逼出四个漏洞，全是测试的

九个变异，第一轮活了四个：

- **比盒子宽的行**：根本没有测过没有余量的情况。
- **光标矩形不加位移**：我那条测试用的是**选中区间**，
  而选中时根本不画光标（`show_caret && !has_selection`）——
  **一条看不见光标的测试，证明不了光标的任何事**。拆成独立一条，用折叠选区。
- **字段不往下传** / **控件不往下传**：两跳都没有端到端测过。
  补了一条从 `SelectableText` 一直到画布上那个 x 的测试。

补完之后九个全死。

尺子：十七把全部 exit 0。门：Rust 6950 通过、`cargo fmt --check` 干净；
C++ 52 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6950 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

顺带一条新的机械教训：**注释里带反引号的字符串不要经 bash 传**——
这轮有一处 `python -c` 里的反引号被 shell 当成命令替换，
把 `line_shifts` 三个字整段吃掉，留下 `// , so the shaper...`。
和 heredoc 的 `\n` 是同一类：**shell 会读你的字符串**。规则不变：用 Write/Edit。

**下一步**：`SelectableText` 的 `style`（`TextStyle?`）——
和刚做完的两个是同一类，控件收下没有去处。
**先查一件事**：字段的 `with_style` 已经有了，
但 `SelectableText::widget` 现在**一次都没调过它**——
如果是这样，这一轮就是一跳的活，比对齐小得多；
真正要想清楚的是 `None` 该落到哪里：上游是
`DefaultTextStyle.of(context).merge(style)`，而本 crate 的字段
默认样式是从主题取的，**两者不是一回事**，要么接上 `DefaultTextStyle`，
要么把差别写清楚。

---

## 第 522 轮：`SelectableText.style`——一跳的活，难在 `None` 落到哪里

先查上一轮记下的那件事：字段的 `with_style` **早就有**
（`TextField` 有 `style: Option<TextStyle>`），
而 `SelectableText::widget` 一次都没调过。所以代码是一跳的活。

真正要想清楚的是 `None`。上游写的是：

    effectiveTextStyle = DefaultTextStyle.of(context).style
                             .merge(style ?? textSpan.style)

两件事，本 crate 各有各的答案：

- **`DefaultTextStyle` 这个 crate 没有**（components.rs 第 8522 行的注释
  早就写着"This crate has no such"）。所以 `None` 就照原样传下去，
  由字段回退到主题的 body ——**环境默认本来也是从那儿来的**。
  这里特意**不**在控件里解析一份主题样式再传下去：
  那是把同一个答案写在第二个地方，而且主题一变它不会跟着变。
- **merge 没有照抄**。上游允许只给一个"加粗"然后其余继承；
  这里给了样式就是用这个样式。这跟本 crate `TextSpan` 早就写下的规则是同一条：
  **继承是调用方的活**，因为跑到 shaper 面前时答案总归是一个解析好的样式。
  差别写进了字段文档，不是含糊过去。

### 一条反直觉的、值得单独写测试的事

**富文本passage里，`with_style` 什么也改不了。**
每个 run 自带解析好的样式，基础样式只覆盖最后一个 run 之后的文字，
而 passage 没有那种文字。`with_style` 看上去像是"应该赢"的那个，
实际上一点作用没有——所以这条单独写了测试，而不是留给下一个读者去猜。

四个变异全部杀死：控件不往下传、`with_style` 不存、
字段忽略给定样式只用主题、run 取基础样式而不是自己的。

尺子：十七把全部 exit 0。门：Rust 6952 通过、`cargo fmt --check` 干净；
C++ 52 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6952 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`SelectableText` 还剩的成员里，`min_lines` 和 `autofocus`
是最近三轮同一类里最后两个"控件收下没去处"的
（字段两边都有：`min_lines` 在 `TextField` 里是有的，`autofocus` 要查）。
**先查一件事**：`TextField` 有没有 autofocus——
`crate::focus::focus(id)` 是现成的，但**谁来调、在哪一帧调**才是问题：
上游是 `initState` 里 `SchedulerBinding.addPostFrameCallback`，
本 crate 的等价时机要找准，否则会变成"每帧都抢焦点"，
那比没有还糟。如果一时找不到干净的时机，就照 `show_cursor` 那轮的做法
**明写下来**，别悄悄放着。

---

## 第 523 轮：`autofocus`——上一轮担心的"什么时候"其实早有答案

上一轮记的疑问是"谁来调、在哪一帧调"。查证：**这条路早就修好了**——
`focus::apply_pending_autofocus()` 由 app.rs 在每帧 build 之后调一次，
对应上游 `applyFocusChangesIfNeeded` 的位置。

但它只有**按作用域**的一半（`autofocus_in(trap)`，
"把焦点放进这个模态里的某处"），没有**按节点**的那一半
（上游的 `Focus(autofocus: true)`，`TextField.autofocus` 传下去的那个）。
cupertino.rs 第 1520 行早就把这条缺口记下来了：
"Upstream's `autofocus` is **not** here"。

### 加上按节点的一半

`focus::autofocus_node(id)`，和作用域请求在同一趟里发放，
**节点优先于作用域**——这条顺序是规则不是巧合：
作用域说的是"放进来就行"，节点说的是"放我身上"；
先满足作用域会拿第一个停靠点交差，真正开口的那个反而落空。

要求方（字段）在 `initial_state` 里请求，那就是这个 crate 的 `initState`：
**每次挂载一次，不是每次 build 一次**。
每次 build 都要的字段会把焦点从读者移到别处的地方抢回来——比不要还糟。

### 扫描逼出四个，其中一个的正确处置是删代码

四个活下来的变异里，"节点不存在也照样聚焦"这一条**不该补测试，该删代码**：
`focus()` 开头就已经拒绝未注册的 id。我那句 `is_registered` 是
**同一条规则的第二份**，没有任何可观察差别——
这正是本项目写过的两条规则撞在一起：
"没有可观察差别的规则不该写"、"一个概念两个家就是漂移的起点"。
删掉，并把"为什么这里没有存在性检查"写在原地。

另外三个补了测试，都是真的空白：

- **模态挡在前面**：字段所在的页面被对话框盖住之后才轮到发放，
  给了就等于把键盘放到读者看不见也拿不回来的地方。
- **请求要"花掉"而不是"留着"**：`take` 而不是读。留着的话，
  某个后来的帧焦点一空，一个很久以前提的请求会突然生效。
- **控件那一跳**：`SelectableText` 到字段。

七个变异全部杀死。

尺子：十七把全部 exit 0。门：Rust 6959 通过、`cargo fmt --check` 干净；
C++ 52 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6959 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`cupertino.rs` 第 1520 行那条注释**现在是错的**——
它说这个 crate 没有按节点的 autofocus，而这一轮加上了。
**下一轮第一件事就是去改它**，顺带看 `CupertinoTextField`
能不能直接用上（它那里本来就有 `autofocus` 参数被记为"不支持"）。
**一条留在原地的错记录比没有记录更糟**——这句话第 514 轮写过，
这次是我自己刚制造的一条。

---

## 第 524 轮：把上一轮自己写错的那条注释改掉，顺手把 autofocus 放到该在的地方

上一轮末尾自己记的：cupertino.rs 第 1520 行那条注释
（"这个 crate 没有按节点的 autofocus，所以 `CupertinoSwitch` 不提供"）
在上一轮之后**变成了假话**。这一轮第一件事就是去改它。

但改注释不是终点。真正该问的是：**autofocus 应该住在哪儿**。
上一轮加在了 `TextField` 上，而上游的 `autofocus` 是
`Focus(autofocus:)` 的参数——**任何基于 `Focus` 的控件都该有**。
于是搬家：`focus::Focus::with_autofocus`，
`CupertinoSwitch` 只是把自己的那个传下去。
`TextField` 那条路径保持不动（它不经过 `Focus` 组件，自己注册）。

### "只在第一次 build 时要"这次怎么写的

字段那边用的是 `initial_state`（每次挂载一次）。
`Focus` 没有 state，`build` 每次都跑。
判据用**注册表本身**：这个 id 还没有条目，就说明这是注册它的那次 build。
`prune` 会在元素消失时把条目拿走，所以**重新挂载会重新要一次**——
这正是上游的语义（新的 state 对象，`initState` 再跑一次）。

禁用的开关不要键盘：`self.autofocus && enabled`，
和已有的"点不到、Tab 不到"是同一道闸。
**一个禁用控件抢走键盘，读者按什么都离不开它。**

### 扫描活了一个，而它暴露的是测试在说谎

"每次 build 都要"这条变异活着。原因：我在 focus.rs 写的那条测试
`rebuild_dirty()` 时**没有任何东西是脏的**，所以 `Focus::build` 根本没再跑。
**一条名字承诺"不在其它 build 上要"的测试，从没见过第二次 build。**

补在 cupertino 那边：开关的 `on_focus_change` 会 `set_state`，
所以焦点一变它就是脏的，`rebuild_dirty` 是**真的重建同一个元素**——
这才是"只在第一次"这条规则真正立得住的地方。

focus.rs 那条测试没有删，但**名字和注释都改成它实际看得见的东西**，
并写明它看不见什么、该去哪儿看。
**一条名不副实的测试，比没有测试更容易让人停止检查。**

五个变异全部杀死。

尺子：十七把全部 exit 0。门：Rust 6962 通过、`cargo fmt --check` 干净；
C++ 52 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6962 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`controls.rs` 第 4534 行 `ChipAttributes` 那条注释也点名了
`autofocus`（连同 `focusNode`、`mouseCursor` 等）"没有类型可以回答"。
`autofocus` 这一条**现在有了**。
**先查一件事**：`Chip` 系列是不是也建在 `focus::Focus` 上——
如果是，那就是和这一轮同样的一跳；如果不是（比如它根本没有焦点节点），
那这条注释仍然成立，**只该把 `autofocus` 从那串名单里摘掉**，
而不是硬加一个没有节点可依附的参数。

---

## 第 525 轮：能点的 chip 现在是键盘走得到的地方

接上一轮记的：`ChipAttributes` 那条注释把 `autofocus` 列进
"这个 crate 没有类型可以回答"。先查那件事——**controls.rs 里一个 `Focus` 都没有**。

于是问题不是"缺一个类型"，是**chip 根本没有焦点节点**：
一个能点、也能被读屏读出来的 filter chip，**Tab 从它旁边直接走过去**，
没有指针就完全操作不了。这才是真缺口，注释只是指错了地方。

### 做的那件事

能点的 chip 外面包一层 `focus::Focus`（节点在 semantics 外面，
和上游 `RawChip` 把持有焦点节点的 `InkWell` 放在 `Semantics` 里面同序），
并接上 `autofocus`。

**没有 handler 的 chip 完全不包**。上游 chip 的 `canRequestFocus` 是 `isEnabled`；
一个只当标签用的 chip 同理——**一个什么键都不答的停靠点，
是读者白白落进去、还得再 Tab 出来的地方**。

### 一件明写下来而不是顺手做掉的事

**这个 crate 里没有任何控件响应 Enter/Space。**
`shortcuts.rs` 里 `Intent::Activate` 有表项，但**没有一个 widget 消费它**——
按钮、开关、chip 都一样。所以这一轮**没有**给 chip 单独做键盘激活：
那会让 chip 有一套邻居们都没有的键盘行为。写在 `Chip::build` 的注释里，
点名这是全 crate 的缺口而不是 chip 的。

`ChipAttributes` 那条注释里把 `autofocus` 摘掉了，并写清它为什么
**仍然不在这个 trait 里**：trait 说的是所有 chip 的共同点，
而没有 handler 的 chip 没有节点可以 autofocus。

### 扫描

四个变异，第一轮活了一个："每个 chip 都自动抢焦点"。
原因：我的测试里那个能点的 chip **从没跑过 autofocus 那一趟**——
只验证了"能被 focus 到"，没验证"没要就不给"。
补了一句：**可达不等于自荐**，否则每一页打开都是第一个 filter chip 被选中。
补完四个全死。

尺子：十七把全部 exit 0。门：Rust 6965 通过、`cargo fmt --check` 干净；
C++ 52 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6965 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：就是本轮写下的那条——**`Intent::Activate` 没有任何 widget 消费**。
表里有、焦点有、意图有，就是没人动手，所以**键盘上没有一个控件能被按下**。
**先查两件事**：一是 `actions.rs` 的 `dispatcher.maybe_invoke` 现在由谁调用
（`shortcuts.rs:1434` 那处 `with_on_key` 是唯一的消费点吗）；
二是 `focus::dispatch_key` 走到焦点节点之后，
`Focus::with_on_key` 的返回值怎么表示"我用掉了"——
接激活的正确位置多半就在那里，而不是每个控件各写一遍。

---

## 第 526 轮：键盘上终于能按下一个控件了

上一轮自己写下的那条：`Intent::Activate` 表里有、焦点有、意图有，
**没有一个 widget 消费它**——整个 crate 里没有任何控件能用键盘按下。

### 先查的两件事，答案决定了放在哪儿

一是消费点：`shortcuts.rs` 的 `Shortcuts` widget 确实完整
（键→意图→`Actions::maybe_invoke_key`），但它要求**上面装了 Actions 作用域**。
二是 `Focus::with_on_key` 的返回值 `KeyResult::Handled` 就是"我用掉了"。

于是放在 `Focus` 上：`with_on_activate`。理由写在原地——
上游用 `FocusableActionDetector`（Focus + Shortcuts + Actions 三合一）；
这个 crate **对每个控件共有的另一个键已经做过同样的合并**：
`handle_traversal_key` 直接问 `default_shortcuts` 而不要求每个按钮上面装一层。
激活跟着它走，**否则一个控件在有作用域的 app 里能按、在没有的里面不能按**。
`Shortcuts` 仍然是调用方绑自己快捷键的地方，这里只管每个可操作控件都有的那一个绑定。

**调用方自己的 on_key 先跑**。文本域的 Enter 是提交，
它不能顺带把自己所在的按钮也按了。

`is_activation` 问的是同一张表（`Intent::Activate` 或 `ButtonActivate`），
所以 Enter 和 Space 在这里和在别处含义一致，
**哪天某个平台改了拼法，只有一处要改**。

### chip 接上

上一轮刚给 chip 加了焦点节点却按不动；这一轮它调用**指针会调的同一个 handler**。
合成的 tap 在原点、`pointer_id: -1`：**键没有位置**，
上游 `ActivateAction` 调 `onPressed` 而不是 `onTap` 也是这个道理。
这条写在注释里，免得下一个读者以为原点是个坐标。

六个变异全部杀死，覆盖：处理器没注册、自己的 on_key 顺序反了、
所有键都当激活、只认 Enter 不认 Space、按了却报告没用掉、chip 不接。

尺子：十七把全部 exit 0。门：Rust 6968 通过、`cargo fmt --check` 干净；
C++ 52 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6968 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：机制有了，**用的人只有 chip 一个**。
按钮、开关、单选、复选这些真正天天按的控件都还没接。
**先查一件事**：`ink_well.rs` 的 `InkWell` 已经有 `focus_id` 和
`Focus::new(focus_id, child)`（第 756 行那处），
如果按钮系是建在 `InkWell` 上的，那么**在 `InkWell` 上接一次
就等于给一大片控件同时接上**——先确认按钮走的是不是这条路，
以及 `InkWell` 的 `on_tap` 是不是就是它要调的那个。
接错地方就会变成每个控件各写一遍。

---

## 第 527 轮：按钮

先查上一轮记的那件事：`InkWell` 在自己文件之外只有**三处**调用，
**按钮系不在其中**。所以"在 InkWell 上接一次覆盖一大片"这条路是走不通的，
查清楚才没有接错地方。

真查出来的是：`Button` 和上一轮之前的 `Chip` **一模一样的形状**——
`PointerHandlers`，没有焦点节点。也就是说
**这个 crate 里的 Material 按钮根本不能用键盘操作**。
这是同一个缺口，但爆炸半径大得多。

### 一条规则，一个家

第三次要写同样的包裹（chip、button，还会有更多），所以先提出来：
`focus::operable(id, autofocus, on_tap, child)`——
每个可操作控件都需要的三件事，且没有一个需要不同的做法：
节点、autofocus、绑到**指针会调的同一个 handler** 的激活。

**它收的是指针那个回调，不是第二个**。理由和 semantics 那层一样：
两条路不能对"按下这个是什么意思"给出不同答案。

没有 handler 的控件原样返回。禁用的按钮**连节点都不建**——
上游 `canRequestFocus` 就是 `isEnabled`，
**一个什么键都不答的停靠点，是读者白白落进去、还得再 Tab 出来的死胡同**。

chip 改成调它，代码短了一大截；按钮接上，同时补了 `autofocus`。

### 扫描里两个不编译的变异

五个变异里有两个**根本不编译**——不编译的变异什么也证明不了，
按第 516 轮定下的规矩重写成能编译的形式（一个把激活体换成空操作，
一个用 `std::convert::identity` 把 `operable` 整个绕过去）。
重写之后五个全死。

尺子：十七把全部 exit 0。门：Rust 6971 通过、`cargo fmt --check` 干净；
C++ 52 个 gtest 全过；gallery 357 通过；`rustflutter_unittests` 6971 通过；
三个目录 default 与 `rustflutter_engine` 都 exit 0。

**下一步**：`operable` 现在有两个用户（chip、button），
而**开关、单选、复选**这些天天按的还没接。
**先查一件事**：它们的 tap 回调是不是也在 `PointerHandlers.on_tap` 里——
如果是，那就是三处一样的两行；如果它们用的是别的形状
（比如 `on_changed(bool)` 而不是 `on_tap`），
那 `operable` 收的类型就需要想一想：
**是让它们凑成 `on_tap`，还是 `operable` 再收一个"按下就调这个"的闭包**。
后者更可能是对的——`ActivateAction` 调的本来就是 `onPressed` 而不是 `onTap`。
