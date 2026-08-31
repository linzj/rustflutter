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
