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
