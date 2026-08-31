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
