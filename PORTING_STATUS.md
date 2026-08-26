# 与上游 Flutter 的对齐差距 —— widget 层到 paint 层

范围只有 widget 层到 paint 层:`framework.rs`(元素树)、`render.rs`(全部 25 个
生产用 `RenderBox`)、`painting.rs`、以及 `app.rs` 的帧序。引擎侧、平台通道、
无障碍、Android 宿主**不在这个范围内**——它们的移植记录、验证基线与已知缺口在
`git show fc265dc:PORTING_STATUS.md`。

2026-08-17 起的第二轮是**逐 API** 审计:上一轮修复自己新增的面
(`scrolling.rs`/`scrollbar.rs`/`widgets.rs`/`components.rs`/`gestures.rs`/
`physics.rs`/`controls.rs`/`editable.rs`/`focus.rs`/`keyboard/`/`services/`)
与上游逐符号比对(签名与实现),修复项见文末"已补上"各条,下面两节是余账。

比对的上游是 `K:\flutter`(`flutter/flutter @ cf97bfbcb9f`)。

**每一条都照上游的实现写,不要照这份描述写。** 描述可能有偏差,
`packages/flutter/lib/src/rendering/*.dart` 没有。锚点给的是符号名而不是行号。

改完 **Windows 和 Android 都要验**。

---

## 一、要做的

(原第 1 条"语义节点没有被祖先的裁剪切掉"已完成:`RenderBox` 有了
`describe_approximate_paint_clip` / `describe_semantics_clip`,语义走树带裁剪矩形
下传、节点矩形按上游 `computeChildGeometry` 的公式相交、整块出窗的节点剔除。
与上游的一处取舍:上游对 paint 裁剪交空的节点是留树打 `hidden` 标记,本桥的
`SemanticsNode`/C ABI 没有 hidden 概念,按剔除处理——要 hidden 得先扩语义桥。)

第二轮逐 API 审计排出的队,按簇列出;每条都以锚点为准:

**手势(`gestures.rs`)**
- **tap 的 `kPressTimeout` 死线。** 上游 `TapGestureRecognizer` 构造即挂 100ms
  死线(`tap.dart`),超时未决就失去 tap 候选资格;这里 tap 只等竞技场。要在
  `tick` 里走一条长按那样的定时路径。
- **tap 的预接收 slop 共用了 pan slop。** 这里的拖拽是自由 pan,slop 分级后 tap
  拒绝也跟着用 36px;上游 tap 识别器自持 18px(`kTouchSlop`)。补上一条时一并
  把 tap 的 slop 拆开。
- **`on_drag_cancel`。** 上游 `DragGestureRecognizer` 区分 `onDragCancel`(竞技场
  判负/取消)与 `onDragEnd`;这里取消只发零速 `on_drag_end`。
- **scale 降级。** 上游剩一指时降为 pan 继续走(`scale.dart` 的 `_ScaleState`
  机),且按 `computeScaleSlop`/`computePanSlop` 起手;这里第二指落下即起手、
  抬起任一指即结束。
- **`SignalKind::ScrollInertiaCancel` 已解析未分发。** 上游把它路由给拖拽识别器
  以停在触控板惯性滚动;这里落到 `on_hover` 兜底。

**滚动(`scrolling.rs`/`scrollbar.rs`)**
- **thumb 的 hover/drag 态。** 上游 `_thumbColor` 分 idle 0.3 / hover 0.75 /
  drag 0.65;这里只有 idle(0x4D),因为 thumb 不接指针、不能拖。要做交互式
  滚动条时一并做(`RawScrollbar` 的 thumb 拖动整套)。
- **容差常量按 dpr=1 写死。** `FLING_TOLERANCE_VELOCITY`(20.0)与距离容差
  (1.0)上游都从 `devicePixelRatio` 推(`toleranceFor`);`Scroll` 门面不知道
  dpr。等它知道的那天改成随 dpr 推导。

**控件/外观(`widgets.rs`/`components.rs`/`controls.rs`)**
- **`Container` 的 `constraints` 参数**(上游 `Container.constraints`);顺带
  `foregroundDecoration`/`transform`/`clipBehavior`。
- ~~**`ClipRRect` 的逐角圆角**(上游 `BorderRadius`);现为统一 `f32`。~~ 已完成:
  `borders.rs` 落地 `BorderRadius`/`BorderRadiusDirectional` 后,
  `RenderDecoratedBox::with_border_radius`(逐角绘制+命中走 rrect)、
  `RenderClipRect::with_border_radius`(逐角走路径裁剪)、`Container::with_border_radius`、
  `ClipRRect::rounded`/`directional` 全部接上;统一 `f32` 保留为简写。
- **`AppBar` actionsPadding**:上游有效缺省 `EdgeInsets.zero`,这里起头内缩
  16——代码里已自陈为故意,若按上游归零需过一遍相册。

---

## 二、明确不做的(连理由一起,免得下次被当成待办)

- **`BoxConstraints::biggest()` 在无界轴上返回 `min`。** 上游返回 `infinity`。
  这是**故意的**安全化:它同时是 `RenderDecoratedBox` 无子节点时的尺寸来源,真按
  上游改成无限大,示例和相册会一起变成无限大的盒子。要动的话得先给
  `RenderDecoratedBox` 一个 `computeSizeForNoChild` 的对等物。
- **`RenderOpacity` 全透明时不参与命中。** 上游**不**画这条线——它让看不见的子树
  照样可命中,要挡就在上面压一个 `IgnorePointer`。这里比上游严,不是译错;放开
  会把点击交给正在淡出的东西。代码里已自陈。
- ~~**`TextOverflow::Fade`。**~~ 已实现:`BlendMode` / `save_layer` / 渐变都已
  在,`fade` 照上游 `RenderParagraph.paint` 的 saveLayer+modulate 渐变落地。
  (原先"绘制层没有混合模式"的缺席理由已过期。)
- **`ListTile` 尾部预留宽度的那个 32 下限。** 上游是
  `math.max(trailingSize.width + gap, 32.0)`,flex 只能预留
  `trailing + spacing`。只有尾部比 gap(16)还窄时两者才不同,而这里的尾部——开关、
  金额、按钮——没有一个窄于 16。
- **摩擦模拟的 `constantDeceleration` 项与 `BoundedFrictionSimulation` /
  `FrictionSimulation.through` / `timeAtX` / `Simulation.snapToEnd` /
  `ScrollSpringSimulation` / `SpringDescription.withDurationAndBounce` 一族。**
  全部系于未移植的 `BouncingScrollPhysics`/`ClampedSimulation`(桌面滚动过冲
  物理);这里只有钳制滚动,参数无处可去。弹跳物理立项时一并补。
- **`ClampingScrollSimulation` 的 tolerance 注入面。** 上游该参数只进 debug
  assert,无行为差;`FrictionSimulation`/`SpringSimulation` 已各自带 `Tolerance`。
- **模拟一律 `f32`(上游 `f64`)。** 只在极端拖拽暴露(如 drag 0.995 的
  `_finalTime` 丢第五位有效数字);逻辑像素下无感。要较真到 0.01px 再说。
- **`scroll_by` 在顶边发 `Overscroll`。** 上游 `pointerScroll` 钳到边时静默
  (钳后目标等于 pixels 就不发);这里故意发,让顶边拖动也能亮起滚动条。
  `scroll_by` 文档自陈。
- **滚动条渐隐的计时是帧粒度。** "最后移动"由 `advance()` 里第一个帧盖上,
  上游是通知回调里起 600ms `Timer`——这里的通知监听没有时钟。
- **越界的弹簧模拟未移植。** 上游 `createBallisticSimulation` 对出界量走
  `ScrollSpringSimulation`;这里 `set_extent` 在布局时钳正。与上面弹跳物理
  一族同账。

下面这些**得先有 pipeline owner 或者等价物**才谈得上,和"标记一路走到根"是同一笔
账,不适合顺手做:

| 缺的 | 上游在哪 | 后果 |
| --- | --- | --- |

这张表原先的其余各项都已补上:元素层 top/bottom sync(`update_children` 六步)、
GlobalKey/reparent(失活表+认领+帧末清扫,`claim_global_key`/`finalize_inactive`)、
Wrap 的 runAlignment、dry layout(`compute_dry_layout`+缓存,Flex/Wrap 固有尺寸改走
dry 路径)、`markNeedsPaint` 停在最近 boundary、paint 脏列表(boundary 粒度)、留存层就地重录
(引擎 `RemoveAllChildren`+`push_retained`)、relayout boundary/`parentUsesSize`/布局脏列表
(`flush_layout` 从边界起帧)、`needsCompositing`/`flushCompositingBits` 的替代形态(本引擎
clip/transform 恒真 layer,由留存层复用吃掉大头)、**sliver 协议**(`SliverConstraints`/
`SliverGeometry`/`RenderSliverToBoxAdapter`/`RenderSliverViewport`/`RenderSliverList`(懒布局+
GC+offset 校正)/`RenderSliverPadding`/`SliverListView` 门面——与旧 `RenderViewport`/
`LazyList` 并存,相册尚未迁移)、**`ScrollMetrics` 的第四个字段**(`viewportDimension`,
由 `set_extent` 与 extent 成对告知,对齐上游 `applyNewDimensions` 的一并回填)与
**只收 child 的 `Scrollbar`**(上游 `Scrollbar({child})`:位置、视口、内容全长全部从
`ScrollNotification.metrics` 学得,构造期零几何参数;`maxScrollExtent+viewportDimension`
即内容全长)。

第二轮逐 API 审计已落地的修复(按簇,锚点在各文件):

- **`widgets.rs`/`render.rs`/`engine.rs`(widget 簇)**:`SizedBox` 只定宽时高度
  约束改 loose(上游 `tightFor` 只夹有值轴);`Container` 有边框时装饰矩形内缩
  边框宽(`BoxDecoration` 的 `clipInput` 语义);无子 `Container` 取 biggest;
  `NavigationToolbar` 补 `centerMiddle`(上游 `_ToolbarLayout` 中间钉条心);
  `StackFit`(Loose/Expand/Passthrough)+ `clip_behavior` 缺省 HardEdge +
  `StackPosition`/flex 的 debug 断言(上游 `assert` 翻译);图片缺省 fit 改
  `ScaleDown`;RTL 下渐隐遮罩镜像;`TextAlign` 缺省 Start(此前误 Center)。
- **`gestures.rs`/`physics.rs`(手势簇)**:slop 按指针类型分级
  (`compute_hit_slop`/`compute_pan_slop`:mouse 1/2、touch 18、pan 36——
  `kDoubleTapTouchSlop` 一族);双击窗口从 down/up 时刻计(此前混用);
  `FrictionSimulation::final_time` 改牛顿解,不靠步进撞容差。
- **`scrolling.rs`/`scrollbar.rs`(滚动簇)**:容差常量落定(速度 20、距离 1);
  `jump_to` 原样存 pixels 不过弹道;`scroll_by` 按 `pointerScroll` 的
  start→update→end+idle 序发通知;`Overscroll` 带速度;`ScrollMetrics` 补
  `viewport_dimension` 并派生 `extent_before/inside/after`、`out_of_range`、
  `at_edge`(`ScrollMetrics` mixin 同名);`Scrollbar` 改只收 child。
- **`components.rs`/`controls.rs`(控件簇)**:`Button` 对齐 Material 3 默认
  (高 40、最小宽 64、Stadium 边、字重 500、`with_enabled` 禁用态);光标闪烁
  半周期 500ms;文本输入补 14 个枚举变体;`TabBar` 指示条 2.0;
  `BottomNavigation` 高 56(`kBottomNavigationBarHeight`);`NavigationRail`
  256/80(展开/收起);`Scrim` black54;`Dialog` 宽 280、圆角 28;
  `BottomSheet` 手柄 32×4、圆角 28;`Snackbar` 高 48、圆角 4;
  `Checkbox` 18×18 圆角 2(`_kEdgeSize`);`Radio` 补 `with_enabled` 禁用态;
  `Divider` 高 16 中心发丝线(上游 thickness 0)。

sliver 侧仍不做的:center/anchor 反向增长(视口锚 0 单向)与 keepAlive(离窗即弃)——
`render.rs` 相应处有自陈。

- **`LayoutBuilder` / `BuildScope`。** 上游在**布局中**把约束递回 widget 层现场构建子树;
本工程的 render 树每帧从 element 树整体重装配,没有"布局中回头 build"的通路。等价物是
`RenderSizeReporter`:量出约束、下一帧据此构建。首帧尺寸不保证与上游同帧语义一致,
这是记录在案的形态差异,不是漏译。
- **forcePress 手势。** ABI 已带 `pressure`,但宿主(Windows)硬编码 pressure=1.0 且
pressureMin=0,照上游阈值(≥0.5 起始)实现会让每次普通点击都触发力按压。引擎侧先给
真实压力数据再谈。
- **`computeDryBaseline`。** dry 路径用缓存的湿基线顶替(`render.rs` 自陈),常见场景
比上游的 dry 重算更准;真要 dry 重算得给 dry 协议加基线位。

---

## 执行器(2026-08-23,ASYNC_PLAN.md 记账)

### 那条留了很久的缝,现在接上了 —— `task.rs`

`runtime_controller.cc` 里那句注释本来就写着入口在哪:

> Upstream drains the microtask queue between the two (FlushMicrotasksNow), and that position is
> load-bearing… **There is no async runtime here yet, so nothing is queued and nothing is drained
> -- but this is where it would go.**

**这条分歧关掉了一半,另一半是故意留着的。**

先说清楚原来为什么没有,因为那个理由到今天仍然成立:**上游的 `Future` 不是并发原语**。Dart 的
`async`/`await` 把续延排进同一个 isolate 的事件循环、在同一根线程上跑;真并行是 `Isolate`,而
`packages/flutter` 几乎不用。所以上游一个 future 的全部语义是「稍后在这同一根线程上叫我」——
**那正是回调的定义**。当初把二十来处 `Future` 译成回调,丢掉的从来不是语义,是**组合**:三段以上
的异步序列写成嵌套回调,错误要一层层手动往回传。

**缺的那一件,`core::future` 给不了。** 标准库给的是词汇——`Future` trait、`Poll`、`Waker`、
`RawWakerVTable`,以及 `async fn` 这个连库都不需要的编译器特性。它**刻意**不给的是执行器,以及
「`Waker::wake()` 之后该发生什么」的答案。后者只能由宿主提供,而 `RfAppHost` 那六个回调里没有一个
能从 UI 线程之外调:`schedule_frame` 最终走到 `Animator::RequestFrame`,那里直接读写
`regenerate_layer_trees_` 这个裸 bool。

于是加了 `post_task`,**七个回调里唯一允许任意线程调用的那个**,底下是本来就线程安全的
`fml::TaskRunner::PostTask`。

**线程亲和是编译期的,不是约定。** 任务在派生它的那根线程上恢复,四条不变式说明这件事——两条编译器
管:future 是 `Pin<Box<dyn Future>>`、不带 `+ Send`,任务表是 thread_local,把它挪走是类型错误;
`Waker::from(Arc<W>)` 要求载荷 `Send + Sync`,这正是要的分工——**信号可以跨线程,状态不行,而跨过去
的只有一个 `TaskId`**。另两条是断言:owner 线程,和排空不可重入。

**两处顺序是承重的,都写在代码里。**

* 排空点在动画相位和构建相位之间,即上游放 `FlushMicrotasksNow` 的位置。一个在 tick 期间完成的任务
  必须被紧随其后的那次 build 看见——和「在 `onBeginFrame` 里起的动画必须被同一帧看见」是同一条理由。
* `task::detach` 在 `services::detach` **之后**。后者会把每一个未答复的回调用 `None` 调一遍,而那正是
  结算等待中任务所挂的 oneshot 的动作;先丢任务会把 `Receiver` 一起带走,答案就落在没人的地方。

`detach` 还会在每次 `post_task` 都持有的那把锁下清掉 poster,所以它返回之后没有线程在宿主里、也没有
线程能进去。**这才是那个裸 `user_data` 真正安全,而不只是碰巧没被观察到。**

**只有真跑了东西才要帧。** 一个挂着等平台答复的任务不该让引擎一直画——帧在这里是按需的,而「在等」不是
画的理由。有回归行盯着。

### 顺手补上的两处静默失败,和 async 无关

`services::MESSENGER` 与 `painting::IMAGES` 都是 thread_local,所以从解码 worker 够它们**不会 race
——会够到另一份空的**。这比 race 更糟,因为它不像失败:worker 上 `send_with_reply` 遇到一个没有 sink 的
messenger,当场答 `None`,读起来和「这个平台没装插件」一模一样。图片缓存更糟:`ImageCache::new` 会
spawn worker,所以在别处碰一下就凭空多出一个解码池,长在一根永远不画的线程上。

两条**本来就是承重假设**——`ImageCache` 自己的注释写着 "one pool per thread that builds, which in
practice means one"——而**两条都没有任何东西在守**。现在都走单一入口,一行断言。断言在没有应用运行时是
空操作,那是每个单测和每次 headless 渲染,它们本来就有权从自己那根线程驱动框架。

### 记录在案的分歧

* **`async_builder` 的 poll 形态留着,而且不是出于客气。** 当生产者根本不是 `Future` 时,poll 才是对的
  形状——流、被轮询的硬件、任何已经按自己的时钟前进的东西,而这仍是这个 crate 里的多数。
  `future_builder` 是新增的那条,即上游 `FutureBuilder` 的字面译法。
* **`future_builder` 收 `Result<T, String>` 而不是裸 `T`。** `AsyncSnapshot` 带的是 data 与 error 中
  恰好一个,一个不会失败的 future 会让快照自己的一个状态永远到不了。`async { Ok(v) }` 是无误 future 要
  付的税,比一个进不去的状态便宜。
* **`StreamBuilder` 仍是 `async_builder` 的形状。** `Stream` 需要一个这个 crate 没有的类型。
* **`TickerFuture::settled` 是方法而不是 `IntoFuture` 实现。** 调用方手里是 `Rc<TickerFuture>`,而
  `Rc` 不是 fundamental 类型,孤儿规则不接受 `impl IntoFuture for Rc<TickerFuture>`。
* **`RefreshIndicator` 没有 future 门面。** 上游 `onRefresh` 返回 `Future<void>` 由框架盯着;这里状态机
  是持有方拥有的一个值,所以接 future 就是调用点上的
  `spawn(async { fetch().await; set_state(|s| s.indicator.refresh_complete()) })`。给它包个 API,是给
  一行代码包个 API。
* **`sleep` 是给应用代码的。** 框架自己的每一条 deadline——长按判定、tooltip 淡出、snackbar 到期、双击
  等第二下——都留在帧时钟上。一个在两帧之间到期的 tooltip 反正要等下一帧才画得出来,第二个时钟只会买来
  一次无事可做的唤醒。
* **上游 future 不可取消,Rust future drop 即取消。** `detach` 因此是丢弃未完成任务而不是完成它们;
  「恰好调用一次」的保证由底下那层回调 API 继续持有。
* **`RfAppHost` 的线程断言没有单测。** 认领是进程级的,而测试框架在自己的线程上跑几千个测试,一个哪怕
  只短暂认领过的测试都会让当时恰好碰到 messenger 的那个失败。判定被抽成一个值,在那里测。

运行时的全程——派生、发出、唤醒、恢复、成帧,以及跨线程和 `sleep` 两个变体——
画在 `docs/ASYNC_FLOW.md`。

测试 4557。`[dependencies]` 仍然是空的。

---

## 完全覆盖计划的第一簇(2026-08-17 起,PORTING_PLAN.md 记账)

### 少掉的那一项,点的是少掉的那个按钮 —— 1888/1888,MISSING 归零(2026-08-20)

新模块 `cupertino_refresh.rs`,收掉最后四个类:`CupertinoSliverRefreshControl`
(`cupertino/refresh.dart`)、`CupertinoExpansionTile`(`cupertino/expansion_tile.dart`)、
`CupertinoSpellCheckSuggestionsToolbar`、`CupertinoAdaptiveTextSelectionToolbar`。

**覆盖率 1888 / 1888,MISSING 0。十层全部归零。** 测试 3685。

---

**最后这一轮的头一件,是一条 assert 里少掉的一项。**

第 91 轮记过 Material 那条:`assert(buttonItems.length <= _kMaxSuggestions + 1)`——写成 `3 + 1` 而不是
`4`,是在说**第四个不是建议,是底下那个删除按钮**。

而 Cupertino 这条是:

```dart
assert(buttonItems.length <= _kMaxSuggestions);
```

**同一个常量名,同一个 3,没有那个 `+ 1`——因为这边没有删除按钮。** 一项之差,把两个工具栏能做的事的全部区
别点了出来。和第 96 轮 `noMaxLength` 是同一个形状:**少掉的那一项不是疏漏,是一个不存在的功能,而 assert
正是你能看见它的地方。**

配套那条 `assert(!readOnly && !obscureText)` 两个拒绝理由不一样:**改不了的文字没什么可纠正的,而给密码框
提拼写建议会把密码漏进一个菜单里**——一个是没意义,一个是不安全,一条 assert 管了两件。

---

**`refreshTriggerPullDistance >= refreshIndicatorExtent`,消息自己解释了自己:**

> The refresh indicator cannot take more space in its final state than the amount initially created by
> overscrolling.

**指示器歇下来时的高度,必须装得进当初把它拉出来的那一段。** 高过了,松手那一刻列表就需要比手势撑开的更多
的地方——**内容会在本该回弹归位的那一刻往下一顿。**

而那五个状态里 `armed` 最有意思:「dragged far enough that the onRefresh callback **will** run」。
**承诺发生在松手之前。** 手指还按着,答案就已经定了——**这正是指示器能在你手指底下变样的原因;一个到松手才
告诉你的界面,没法预先让你看见松手会怎样。**

---

**而这一轮的最后一条,恰好适合给整个扫尾收个口。**

```dart
if (widget.transitionMode == ExpansionTileTransitionMode.scroll) {
  return child;
}
assert(widget.transitionMode == ExpansionTileTransitionMode.fade);
```

**一条写成 assert 的穷尽性检查。** 两个分支,一个用提前返回吃掉,另一个靠断言兜住——**assert 站在了本该由
`switch` 让编译器站的位置上。** 加进第三个模式,它会在运行时、debug 构建里、恰好走到那条路径的那台机器上
才响。

**这一条在这边是白给的:**`ExpansionTileTransitionMode` 是穷尽匹配的,多一个变体根本编译不过。**在一趟从
Dart 往 Rust 搬了一千八百八十八个类的活儿收尾时,这是少有的一处「目标语言直接回答了源语言只能提问的东
西」。**

顺带:两个过渡模式动的东西不一样——**fade 让孩子保持全尺寸只改不透明度,scroll 让孩子保持不透明只改几
何。** 于是前者得把一个比塌缩后的瓦片还大的孩子画到框外去(`OverlayPortal`),后者一个裁剪就够了。
**更贵的机器给了那个「画面撑出盒子」的。**

验证:`cargo test --lib` 3685 绿,GN `rustflutter_unittests` 3685 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。**覆盖率 1888 accounted / 0 MISSING(100%)。**

### 2.975 旁边一句话都没有(2026-08-20)

新模块 `cupertino_controls.rs`,收掉 `cupertino/thumb_painter.dart` 的 `CupertinoThumbPainter`、
`cupertino/radio.dart` 的 `CupertinoRadio`、`cupertino/sliding_segmented_control.dart` 的
`CupertinoSlidingSegmentedControl` 与 `cupertino/icons.dart` 的 `CupertinoIcons`。覆盖率 1884/1888
(99.8%)。**只剩 4 类。**

```dart
const double _kOuterRadius = 7.0;
const double _kInnerRadius = 2.975;
```

**四位有效数字,挨着一个平平的 7.0,上头一句注释都没有。** 第 97 轮在 `cupertino/sheet.dart` 里见过同样形状
的数字——`_kSheetScaleFactor = 0.0835`——**那一个把「量的什么、跟什么比、在哪台模拟器上、哪个 iOS 版本」全
写清楚了。这一个同样明显是量出来的,却什么也没说。**(它是外圈的 0.425 倍——这是量出来的比例,不是选出来
的。)

**同一个团队,同一层,同一种数字,两种交代方式。** 第 97 轮说「数字的精度会告诉你它是怎么来的」,这一轮补
上后半句:**精度告诉你它是量的,但只有注释能告诉你量的是什么。**

---

**两个 thumb,画在不同的高度上。**

滑块的 thumb 有**三层**阴影,开关的有**两层**,而**第一层一模一样**,之后分岔。滑块多出来的那层是
`offset (0, 1)`、blur 1 的贴身接触阴影——**那是「一个被拈起来的东西」的画法**;开关的 thumb 只在轨道里滑,
拿的是更扁的那一对。

而边框那一笔:`canvas.drawRRect(thumbShape.inflate(0.5), ...)`,**画在填充之前**。**它不是描边,是一个稍大
一点的形状垫在后面**——外扩半像素,再被填充盖住,剩下的那根发丝完全在 thumb 之外,而不是骑在边上。第 85 轮
toggle buttons 是先内缩再描边,**同一个目的,两头走。**

还有一句注释值得留:

> Paint RRects instead of RSuperellipses here, because practically `CupertinoSlider` only draws
> circular thumbs.

iOS 的形状通常是超椭圆,**而圆是唯一一种「用便宜的那个图元不是近似、就是同一个图形」的情况。**

---

其余两条:

* `assert(children.length >= 2)`——**二是「还算个选择」的最小数目。** 和第 84 轮 `TabController` 里那个
  `length < 2` 是同一个门槛,只不过那边藏在 `if (value == _index || length < 2) return;` 里,**还阴差阳错
  成了唯一守住那条不变量的东西**;这边是在构造器里明说的,那是更该待的地方。
* 而 `didUpdateWidget` 开头那条 `assert(oldWidget.key == widget.key)`——**它不可能失败。**
  `didUpdateWidget` 只在 `Element.update` 之后才到得了,而那只在 `Widget.canUpdate` 为真时才到得了,
  那就是 `oldWidget.runtimeType == newWidget.runtimeType && oldWidget.key == newWidget.key`。**框架在调用
  之前已经要求过一模一样的事。** 这是一条穿着 assert 外衣的文档——无害,而且恰好是这个移植对自己测试那条规
  矩的镜像:**一条不可能失败的检查,在测试里是缺陷,在这里只是冗余。** 按「陈述不变量」而不是「检查」移植
  了。
* `CupertinoIcons` 同时给出 `iconFont` 和 **`iconFontPackage = 'cupertino_icons'`**。Material 的图标随框架
  一起发,靠 pubspec 里 `uses-material-design: true` 打开;**这些住在一个独立的 pub 包里。** 同一种失败——
  一个字体里的码位,而构建没把那个字体带上——**一边是构建开关,一边是依赖,两边类型系统都看不见。**(1,322
  个,对 Material 的 8,825 个。)

验证:`cargo test --lib` 3674 绿,GN `rustflutter_unittests` 3674 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1884 accounted / 4 MISSING(99.8%)。

### 是「向下」,不是「竖直」(2026-08-20)

新模块 `platform_tree.rs`,收掉 `rendering/sliver_tree.dart` 的 `TreeSliverNodeParentData`、
`RenderTreeSliver` 与 `widgets/platform_view.dart` 的 `HtmlElementView`、
`PlatformViewCreationParams`、`AndroidViewSurface`。覆盖率 1880/1888(99.6%)。测试 3660。

**十层里九层归零,只剩 cupertino 的 8 类——而这一次是逐层核过的,不是外推的。**(上一轮刚更正过一次外推,
所以这次特意把每层的 MISSING 数了一遍再说。)

---

```dart
assert(
  constraints.axisDirection == AxisDirection.down,
  'TreeSliver is only supported in Viewports with an AxisDirection.down. '
  'The current axis direction is: ${constraints.axisDirection}.',
);
```

**是 down,不是 vertical。** 一个反向的竖直视口会和两个横向的一起被拒——**这比第一眼看上去严格,而且是对
的:树的次序天生是从上往下的,列表要是朝上跑,每个孩子都会画在它父节点上面。** 缩进照样能算,意思没了。

而那条消息把**实际拿到的方向**报了出来——这是「一条断言」和「一次诊断」之间的差别。

---

**缩进那件事,文档用一句话把取舍说完了:**

> the space allotted to the indentation will **not** be part of the space made available to the Widget
> returned by `TreeSliver.treeNodeBuilder`

**所以选的不是「缩多少」,是「那块缩进的像素归谁」**——而这决定了那儿能不能画东西。把活交给 builder
(`TreeSliverIndentationType.none`),你才能在缩进里铺装饰、让水波纹漫过去;**render object 那个版本里,
那块空间不属于任何人。**

顺带:**`none` 和 `custom(0.0)` 是同一个值。** render object 完全分不出这两者——它们是靠**意图**区分的:
`none` 的文档写着「你打算自己在 builder 里做缩进」,`custom(0.0)` 只是「不缩」。**一个值,两个名字,分野在
说话人那边而不在行为那边。**

---

**而 `HtmlElementView.isVisible` 说的根本不是「看不看得见」:**

> so the engine doesn't _waste_ an overlay to render Flutter content on top of views that **don't paint
> any pixels**.

**`isVisible: false` 不隐藏任何东西。它说的是「这个元素一个像素都不画」**——它是个点击靶子或者一个链接锚
点,不是一幅画——**引擎据此省下一层合成 overlay。** 文档举的例子正是 `pointer_interceptor` 和 `Link`。

**一个按外观命名、实际管的是合成预算的标志。** 这是这一轮收集到的第四个同类:第 84 轮 `indexIsChanging`
(按「因为什么」命名)、第 82 轮 `ListTileControlAffinity.platform`(按错轴命名)、第 87 轮 `tapEnabled`
(按杠杆命名),现在是这个。

其余两条:

* 三个平台视图构造器里都写着 `assert(creationParams == null || creationParamsCodec != null)`——**又一条蕴
  含:有参数就必须有编解码器。** 平台通道上没法在不说明怎么编码的情况下塞值进去。**有 codec 没参数可以,
  有参数没 codec 发不出去。**
* `AndroidViewSurface` 的文档把你劝去别处:「you may want to use `AndroidView` directly, since it requires
  **less boilerplate code** [...] and there's **no difference in performance, or other trade-off(s)**」。
  **和第 83 轮 `MaterialButton` 指向 `TextButton` 是同一个形状——只不过那个是被取代了,这个从一开始就没有
  什么优势可失去。**

验证:`cargo test --lib` 3660 绿,GN `rustflutter_unittests` 3660 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1880 accounted / 8 MISSING(99.6%),**九层归零。**

### 是减速带,不是墙——而且它自己说了(2026-08-20)

新模块 `scheduler_priority.rs`,收掉 `scheduler/priority.dart` 的 `Priority`、`scheduler/binding.dart`
的 `PerformanceModeRequestHandle` 与 `SchedulerBinding`,以及 `gestures/binding.dart` 的
`FlutterErrorDetailsForPointerEventDispatcher`。覆盖率 1875/1888(99.3%)。测试 3648。

**三个具名优先级隔着 100,000,而 `kMaxOffset` 是 10,000。** 也就是说**相邻两档之间的距离,是单次最大偏移
的十倍**——一次相对偏移永远抬不动一个任务从 idle 跨进 animation。这个间距就是为了让它跨不过去而选的。

而文档没有装作那是一堵墙:

> It is still possible to have priorities that are offset by more than this amount **by repeatedly
> taking relative offsets**, but that is generally discouraged.

**是减速带,不是墙。** 夹的是**偏移量,不是结果**——所以连着走十次最大偏移,就正好落在 animation 上。回归行
把这两面都钉住了:一次跨不过去,十次正好到;而把夹取改成夹结果(那样才是真的墙),第二条当场红。

顺带一处漂亮的小事:`operator -` 的实现是 `this + (-offset)`。**一份实现加一个别名,而不是把夹取抄两
遍**——**这一轮扫下来见了太多抄两遍的块,这里是没抄的那个。**

---

**`SchedulerBinding.requestPerformanceMode` 的返回类型就已经把设计说完了:它返回可空的句柄。**

```dart
// conflicting requests are not allowed.
if (_performanceMode != null && _performanceMode != mode) {
  return null;
}
```

三种结果:没人占着,你拿走;有人占着**同一个**模式,计数加一,一直撑到最后一个句柄松手;**有人占着别的模
式,你拿到 null。** 不是异常,不是覆盖,不是排队。**先来的赢,后来的不同意见直接被拒。**

而且没有办法强行拿到——这正是重点:**应用里两处要求引擎做相反的事,不可能都对,而悄悄让后来的赢会让结果
取决于启动顺序。**

`PerformanceModeRequestHandle` 则是「句柄即请求」:**它唯一的方法是 dispose,而文档写着「This method must
only be called once per object」。** 拿着它就是这个模式还需要,松手就是撤回。和早先那个 `KeepAliveHandle`
是同一个形状——**一个没有内容的对象,它的全部含义就是「还没被 dispose」。**

---

最后一条:`FlutterErrorDetailsForPointerEventDispatcher` 在 `FlutterErrorDetails` 上加了两个字段,而两个
都在回答同一个问题:**是哪一个?** 一条指针路由上可能挂着十几个 handler,**「handleEvent 里抛了个异常」在
你知道是哪个事件、哪个目标之前,还算不上一份报告。**

而 `hitTestEntry` 可空的理由写在文档里:hover、added、removed 这三类**根本不经过命中测试**。**这个 null
标的是一整类事件,不是一次失败。**

验证:`cargo test --lib` 3648 绿,GN `rustflutter_unittests` 3648 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1875 accounted / 13 MISSING(99.3%)。

### 更正:第 92 轮那句「九层已全覆盖」是我外推出来的,不对(2026-08-20)

新模块 `render_semantics.rs`,收掉 `rendering/proxy_box.dart` 的七个语义 render object
(`RenderSemanticsGestureHandler`、`RenderSemanticsAnnotations`、`RenderBlockSemantics`、
`RenderMergeSemantics`、`RenderExcludeSemantics`、`RenderIndexedSemantics`、`RenderAnnotatedRegion`)
与 `rendering/proxy_sliver.dart` 的 `RenderSliverSemanticsAnnotations`。覆盖率 1871/1888(99.1%)。测试
3636。

**先更正一处我自己写错的东西。** 第 92 轮我在 `PORTING_PLAN.md` 里写下「painting/animation/foundation/
services/widgets/rendering/gestures/scheduler/material 九层已全覆盖,只剩 cupertino」——**那是从
`material/` 归零外推的,没有逐层核对。** 这一轮开头照例列 MISSING,才看见 rendering、widgets、scheduler、
gestures 都还有。真实分布是:

```
rendering 10   widgets  3
cupertino  8   scheduler 3
               gestures  1
```

已经把计划里那句改成真实的五层,并把外推那件事记在原处。**尺子一直是对的,是我没去问它。**

---

而 rendering 那十个里的八个凑成一族:**几轮之前 `semantics_markers.rs` 移的是这些 widget,底下的 render
object 一直没动**——尺子按名字数,widget 有了、render object 没有,它就一直数着。

**`RenderSemanticsGestureHandler.validActions` 的文档描述的是一个只会做减法的过滤器:**

> If non-null, the set of actions to allow. **Other actions will be omitted, even if their callback is
> provided.** [...] Normally, these make both the right and left, or up and down, actions available.

一个横向拖动的处理器**天然同时提供 scrollLeft 和 scrollRight**,因为拖动处理器根本不知道哪边还有地方可
去。**于是同一个动作上有两件互不相同的事:它有没有接上,和它此刻可不可行**——而只有后者会随着你滚动而变。
回调管前一件,`validActions` 管后一件:**一个滚到最左边的列表用它说「你可以往右滚,不能往左」,而不用把任
何回调摘下来。**

---

其余几条:

* **`RenderMergeSemantics` 同时设两个标志**:`isSemanticBoundary = true` 和
  `isMergingSemanticsOfDescendants = true`。**它必须这样。** 合并的意思是底下所有东西都塌进这个节点里,
  **而「一个能被塌进去的节点」正是边界的定义**——只说合并不说边界,等于让子孙折进一个不存在的东西。
* `RenderBlockSemantics` 挡住的是「**previously painted**」的节点——**是绘制顺序,不是树的顺序。** 而这正
  是模态需要的:对话框画在页面之后,挡住「之前画的」恰好挡住页面。**按树的顺序反而会挡错东西——对话框并不
  是页面的祖先。**
* `RenderExcludeSemantics.visitChildrenForSemantics` 在排除时直接 `return`。**排除不是给子树打个隐藏标
  记,是那趟遍历根本走不到那儿**——底下什么都没被标记,只是遍历在这个节点掉头了,子孙压根没被问过它们本来
  会说什么。
* `RenderAnnotatedRegion` 的 `sized` 决定这块标注**有没有形状**:不带尺寸的,搜索够得着的任何一点都算它
  的;带尺寸的,只答自己里头的点。

验证:`cargo test --lib` 3636 绿,GN `rustflutter_unittests` 3636 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1871 accounted / 17 MISSING(99.1%)。

### 滚轮没有两端,所以它的步长必须把圈合上(2026-08-20)

新模块 `cupertino_pickers.rs`,收掉 `cupertino/date_picker.dart` 的 `CupertinoDatePicker`、
`CupertinoTimerPicker` 与 `cupertino/menu_anchor.dart` 的 `CupertinoMenuEntry`、
`CupertinoMenuAnchor`、`CupertinoMenuDivider`、`CupertinoMenuItem`。覆盖率 1863/1888(98.7%)。测试
3623。

```dart
assert(
  minuteInterval > 0 && 60 % minuteInterval == 0,
  'minute interval is not a positive integer factor of 60',
),
```

**分钟间隔必须**整除**六十,不是「是个正数」就行。因为这个轮子是绕圈的。** 间隔取 7,停位是 0、7、……、
56,然后过四分钟就又见到 0——**唯一一个不等于间隔的间隙,恰好是你看不见它要来的那个。**

而紧接着还有一条:`assert(initialDateTime.minute % minuteInterval == 0)`。两条合起来说的是:**你没法把一
个没有中间态的轮子停在两档中间。**

**十二条 assert 里有六条是按模式分的,写法都是 `mode != X || <真正的检查>`** ——每个模式一条蕴含,内联写
着。而第 88 轮那个 `PaginatedDataTable` 把同一个意思写成了 `assert(() { if (...) { assert(...); } return
true; }())` 的闭包套 assert。**同一个「这条规矩只在这种情形下管用」,两个文件两种写法。**

顺带:`assert(oldWidget.mode == widget.mode, "The $runtimeType's mode cannot change once it's built.")`
——**又一个「构造参数是 widget 身份的一部分」**,这是这一轮扫过的第二个,上一个是第 85 轮 stepper 的 step
列表;道理也一样:每个模式搭的是另一组轮子,底下那些 scroll controller 只造一次。

---

**而 `CupertinoMenuEntry` 是个只有两个成员的 `abstract interface class`——两个成员讲的都是邻居,不是自
己。**

```dart
/// If [hasLeading] returns true, siblings of this menu item that are missing
/// a leading widget will have leading space added to align the leading edges
/// of all menu items.
bool hasLeading(BuildContext context);

/// When true, a divider will not be drawn above or below this menu item.
bool get isDivider;
```

**一个条目带了图标,其余每个条目都要跟着缩进。** 对齐是这一组的属性,而这组是靠挨个问成员问出来的——于是
「这个条目有没有前置组件」这个答案,**除了它自己,所有人都要读。**

而 `isDivider` 说的不是「我是一条线」,是**「别在我旁边画线」**——正是这条让你自己加的那道分隔线,不会跟菜
单自动画的那两道挤在一起。回归行把这个反过来的读法钉住了:三个普通条目之间画两道线,中间插一道显式分隔
线,两个缝**一道都不画**;而把 `isDivider` 改成天真的那个意思,三条红。

---

其余两条:

* `assert(enableSwipe || !enableLongPressToOpen, 'enableLongPressToOpen cannot be true if enableSwipe
  is false')`——又一条蕴含,而且是物理意义上的:**长按打开菜单之后,你的手指已经按在上面了**,这个手势自
  然要接着滑过去选一项。**关掉滑动却留着长按打开,等于把那根打开它的手指晾在那儿。**
* `CupertinoTimerPicker` 的 `assert(initialTimerDuration < const Duration(days: 1))`——**严格小于一天**:
  这个选择器只有时、分、秒三列,**二十四小时没有地方显示,只会读成零。**

验证:`cargo test --lib` 3623 绿,GN `rustflutter_unittests` 3623 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1863 accounted / 25 MISSING(98.7%)。

### 九十八处「眼睛量的」,七十四处在同一层里(2026-08-20)

新模块 `cupertino_sheet.rs`,收掉 `cupertino/nav_bar.dart` 的 `CupertinoSliverNavigationBar` 与
`cupertino/sheet.dart` 的 `CupertinoSheetTransition`、`CupertinoSheetRoute`。覆盖率 1857/1888
(98.4%)。测试 3609。

这两个文件里满是「有人对着手机看出来的」数字,而且都写明了。顺手把整棵树数了一遍:

```
cupertino  74      rendering   1
material   15      painting    0
widgets     8      gestures    0
                   services    0
                   animation   0
                   scheduler   0
                   foundation  0
```

**九十八处,四分之三在同一层里。而这不是谁马虎——是两套设计语言的东西不一样。**

**Material 是一份能读的公开规范;iOS 是一件只能量的成品。** 没有任何文档写着 iOS 导航栏的背景是在十个逻
辑像素里淡入的,**所以有人在模拟器上打开设置,盯着看。**

而尾巴那一半同样说明问题:`painting`、`gestures`、`services`、`animation`、`scheduler`、`foundation`
**一处都没有**。因为它们算的是**为真**的东西,不是**看着对**的东西。贝塞尔曲线就是贝塞尔曲线。
**这个数量正好随着「从外观往算术走」一路掉下去。**

---

**而同样是「眼睛量的」,写法差得很远。**

```dart
/// Eyeballed on the native Settings app on an iPhone 15 simulator running iOS 17.4.
const double _kNavBarScrollUnderAnimationExtent = 10.0;
```

**它说了看的哪个 app、哪台设备、哪个系统版本——于是它是一句别人能去复核的断言,而不只是一个数。** 对照
`cupertino/button.dart` 的「Eyeballed values. Feel free to tweak.」**两句都诚实,只有一句可复现。**

而 sheet 里那个更进一步:

```dart
// Amount the sheet in the background scales down. Found by measuring the width
// of the sheet in the background and comparing against the screen width on the
// iOS simulator showing an iPhone 16 pro running iOS 18.0.
const double _kSheetScaleFactor = 0.0835;
```

**这不是眼睛量的,是尺子量的**,而且方法写出来了(比两个宽度)。**所以它有四位有效数字,而旁边那些眼睛量
的是 2.0、300、10.0。数字的精度会告诉你它是怎么来的**——这条回归行专门钉住了这个对照。

顺带一个小巧合:`_kNavBarShowLargeTitleThreshold` 和 `_kNavBarScrollUnderAnimationExtent` **都是 10.0,
干的却是两件事**(一个是标题往上收的距离,一个是背景淡入的距离),**而只有其中一个说了自己从哪儿来。**

---

其余几条:

* `assert(topGap == null || (topGap >= 0.0 && topGap <= 0.9))`——**上界是 0.9,不是 1.0。** 一张 sheet 必
  须给屏幕留下至少十分之一:**盖满了的就不像一张 sheet 了**——后面那条露出来的边,正是「这后头还有东西可
  以退回去」的那句话。
* 导航栏那条 assert 的消息**把两条出路都说了**:「Either provide a largeTitle or set
  automaticallyImplyTitle to true.」**对一条双边规则来说,这是有用的形状**——撞上它的人本来两个改法都不
  想要,他想要的是一个标题;只把规则念一遍,他还得自己猜该动哪边。
* 而 `assert(!widget._searchable || widget.bottom == null)`:**能搜索的导航栏不能再有 bottom,因为那个搜
  索框就是 bottom。**

验证:`cargo test --lib` 3609 绿,GN `rustflutter_unittests` 3609 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1857 accounted / 31 MISSING(98.4%)。

### 一边把值重载了,另一边加了个模式(2026-08-20)

新模块 `cupertino_text_field.rs`,收掉 `cupertino/text_field.dart` 的 `CupertinoTextField` 与
`cupertino/text_form_field_row.dart` 的 `CupertinoTextFormFieldRow`。覆盖率 1854/1888(98.2%)。测试
3593。

**第 92 轮记下过 Material `TextFormField` 的 `maxLength` 有三种状态:**

```dart
assert(maxLength == null || maxLength == TextField.noMaxLength || maxLength > 0),
```

**而 Cupertino 这边的同一行是:**

```dart
assert(maxLength == null || maxLength > 0),
```

**这个文件里根本没有 `noMaxLength`**,字段文档也说得干脆:「This value must be either null or greater than
zero.」

**而这不是漏掉了。** 第 92 轮把 Material 那第三种状态读成「显示计数器,一直往上数,永不拦你」——**而
`CupertinoTextField` 压根没有计数器**,没有 `buildCounter`,没有任何显示计数的东西。**没有计数器,「只数
不拦」就没有意思可表达**,那个哨兵在这儿没有活干。**反过来,这也把第 92 轮对那个 -1 的解读坐实了。**

而 Cupertino 确实需要「别真的拦住」的时候,它是用**一个单独的枚举**说的——`MaxLengthEnforcement.none`——
而不是往那个数字里塞一个魔法值。

**同一个需求的两种设计:一边把值重载了,另一边加了个模式。** 而加模式的那个,是**你还能同时给一个真数字**
的那个:`maxLength: 10` 配上 `enforcement: none`,「数到 10,但不拦」;Material 那边一旦用了 -1,那个数字
就没了。回归行把这一点专门钉住了。

---

其余几条:

* 那二十行 assert 在这个类里**出现了两遍**——普通构造器一遍,`.borderless` 一遍,一个字节都不差。**和第
  94 轮那三份 `buildScrollbar` 对照着看很有意思:** 那三份每份上头都挂着「改这儿记得改别处」的注释,而它
  们已经不一样了;这两份一句注释也没有,却是一模一样的。**把拷贝拴在一起的不是那句注释**,是距离——那三份
  在三个文件里,这两份隔着十二行。
* `CupertinoTextFormFieldRow` 的 `padding` 文档:「If the padding parameter is null, `CupertinoFormRow`
  constructs its own default padding [...] **If no edge insets are intended, explicitly pass
  `EdgeInsets.zero`.**」**null 不是零,是「用标准的那个」。** 这是这一轮扫过的第三处同样的区分了,前两处
  是 icon button 的 `splashRadius`(null 是默认,`Some(0.0)` 被拒)和这个文件自己的 `maxLength`。
  **每一次,"没设" 和 "设成零" 都是两件事;每一次,API 都只能用散文说出来,因为类型说不出来。**
* 而 `prefix` 的文档说「iOS guidelines encourage passing a `Text` widget to `prefix` to detail the nature
  of the input」——**标签在字段旁边,不像 Material 那样浮在字段里面。**

验证:`cargo test --lib` 3593 绿,GN `rustflutter_unittests` 3593 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1854 accounted / 34 MISSING(98.2%)。

### 同一句话说了三遍,三处都没法检查它(2026-08-20)

新模块 `cupertino_tabs.rs`,收掉 `cupertino/tab_scaffold.dart` 的 `CupertinoTabController`、
`RestorableCupertinoTabController` 与 `cupertino/tab_view.dart` 的 `CupertinoTabView`。覆盖率
1852/1888(98.1%)。测试 3579。

**`CupertinoTabController` 的文档写着:**

> The value must be greater than or equal to 0, **and less than the total number of tabs.**

**而 assert 只有 `assert(initialIndex >= 0)`。** 没有上界——**因为这个类根本不知道上界是多少。** 它不像
Material 的 `TabController` 那样收一个 `length`;标签在那个将要拿着它的 `CupertinoTabScaffold` 里。

这句话在三个地方出现:构造器的文档、`index` setter 的文档,以及 `RestorableCupertinoTabController` 的构造
器文档。**三处声明这条契约,三处都检查不了它。**

**和第 84 轮那条正好构成一对。** 那边 `TabController` 的文档和 assert 也对不上,**但那边是 assert 上有个
洞(`length == 0 ||` 把范围检查关掉了);这边是这个类没有那个信息。** 同一种形状的分歧,成因正相反——而
这一边是诚实的:它检查了它看得见的那一半。

**真正检查上界的地方在 scaffold 里:**

```dart
assert(
  _controller.index >= 0 && _controller.index < widget.tabBar.items.length,
  "The $runtimeType's current index ${_controller.index} is "
  'out of bounds for the tab bar with ${widget.tabBar.items.length} tabs',
);
```

**契约被劈成两半放在两个类里**——controller 管它看得见的那半,scaffold 管它看得见的那半——**而错误消息是
从 scaffold 这一侧写的,把两个数都点了出来,因为这是唯一同时知道这两个数的地方。**

---

**而 `didUpdateWidget` 里那个分支的形状要紧:**

```dart
if (widget.controller != oldWidget.controller) {
  _updateTabController(oldWidget.controller);
} else if (_controller.index >= widget.tabBar.items.length) {
  // If a new [tabBar] with less than (_controller.index + 1) items is provided,
  // clamp the current index.
  _controller.index = widget.tabBar.items.length - 1;
}
```

**是 `else if`,不是第二个 `if`。** 变短了的标签栏会把选中项拽回范围内——**但只在 controller 自己没同时
换掉的时候。** 两个一起换,这个夹取就被跳过,越界的下标会一直活到下一次变更时撞上上面那条 assert。回归行
把两种情形分别钉住了。

---

其余几条:

* `_updateTabController` 里那句 `if (oldWidgetController?._isDisposed == false)`——**那个 `== false` 干
  的是判空的活,不是布尔比较。** `?.` 遇到 null 得到 null,而 `null == false` 是 false,所以这一个判断的
  意思是**「存在,并且没被销毁」**。一个已销毁的 `ChangeNotifier` 碰它的监听器就会抛,而**被留下的那个
  controller,恰恰就是调用者可能已经销毁掉的那个。** 注意只有摘监听器这一侧有守卫,挂上去那一侧没有。
* `RestorableCupertinoTabController.toPrimitives()` 就是 `value.index`——**一个光秃秃的整数。** 和第 91
  轮那个 `RestorableTimeOfDay` 摆在一起看:那个存的是 `[minute, hour]`,专等着有人把顺序「理顺」成坏的。
  **只有一个值,就没有顺序可弄错。**
* `CupertinoTabView` 的 `_navigatorKey`:**只在没被给的时候才造一个**,然后一直用那个。这是「只清理自己
  造的那个」那条规矩的另一半——**只造没人给你的那个。**
* 而 `_onUnknownRoute` 抛的 `FlutterError` 把四个路由来源**按尝试顺序**列了出来(`builder` 管 "/",然后
  `routes`,然后 `onGenerateRoute`,最后 `onUnknownRoute`)。**一条在失败的当口顺便把查找顺序教给你的错误
  消息。**

验证:`cargo test --lib` 3579 绿,GN `rustflutter_unittests` 3579 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1852 accounted / 36 MISSING(98.1%)。

### 三份同一个函数,每份上面都写着「改这儿记得改另外那份」,而它们已经不一样了(2026-08-20)

新模块 `cupertino_app.rs`,收掉 `cupertino/app.dart` 的 `CupertinoApp`、`CupertinoScrollBehavior` 与
`cupertino/localizations.dart` 的 `CupertinoLocalizations`、`DefaultCupertinoLocalizations`。覆盖率
1849/1888(97.9%)。测试 3565。

**第 89 轮移的是 Material 那四个,这一轮是它们的对号。把两边并排读,基本就是这一轮的全部内容。**

`buildScrollbar` 一共有三份实现——基类 `ScrollBehavior`、`MaterialScrollBehavior`、
`CupertinoScrollBehavior`——每一份上面都挂着同一句话:

```dart
// When modifying this function, consider modifying the implementation in
// the base class as well.
```

**而 Material 那份开头多了一层 axis 判断,先把横向的原样返回,再去问平台;基类和 Cupertino 这两份都没
有。** 于是**同一个横向列表,在 macOS 上的 Cupertino 应用里有滚动条,在 Material 应用里没有。**

**一句要求三份拷贝保持同步的注释,做的是一个共用函数该做的事,而它没做住。** 回归行把这条差异钉成了断言:
横向上两边不同、纵向上两边逐平台一致。

---

**第二件:第 89 轮的那个猜测,这一轮被上游自己的注释确认了,而且范围更大。**

当时读 Material 的 `buildOverscrollIndicator` 只装饰 Android,我写下的推断是「拿到指示器的,恰好是那些
物理本身不会告诉你边界在哪的平台」。而 Cupertino 这份的函数体全文是:

```dart
// No overscroll indicator.
return child;
```

**因为 Cupertino 应用在所有六个平台上都用回弹物理,所以哪儿都不需要。** 这条规矩从来不是关于 iOS 的,是
关于回弹的。**Android 就是那个把话说清楚的例子:同一个平台,在 Material 应用里被装饰,在 Cupertino 应用
里光着**——差别只在于哪一个给了它回弹。回归行拿这两个并排断言了一次。

顺带:`getScrollPhysics` 里**只有 macOS** 拿到 `ScrollDecelerationRate.fast`。那是触控板——拇指一甩是要
滑出去的,两指在玻璃上一划是个更小、更可复现的手势,滑一样远就全冲过头了。

---

**第三件,和第 89 轮那条注释错误正好对上。**

Cupertino 这边的 `_shortWeekdays` 和 Material 那边**一模一样的七个字符串,一模一样的
`weekDay - DateTime.monday` 索引法**——**而这边一句注释都没有。** 那边解释了两遍,两遍都把
`DateTime.sunday` 写成 6(它是 7)。

**两个文件做着同一件正确的事;只有一个解释了自己,而那个正是能把读者带沟里的那个。**

顺带一条小的:`datePickerDayOfMonth` 返回的是 `' ${...} $dayIndex '`——**首尾各带一个空格,烙在本地化字符
串里面**,给它将要落进去的那个滚轮用。**一段藏在翻译里的排版。**

而 `CupertinoApp` 没有 `debugShowMaterialGrid` 的对号:**没有 Cupertino 网格叠层,因为 iOS 的设计语言不
像 Material 那样是照着一个 8dp 网格写出来的。**

验证:`cargo test --lib` 3565 绿,GN `rustflutter_unittests` 3565 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1849 accounted / 39 MISSING(97.9%)。

### 连默认值本身都没有一个默认的 brightness(2026-08-20)

新模块 `cupertino_theme.rs`,收掉 `cupertino/theme.dart` 的 `CupertinoThemeData`、
`InheritedCupertinoTheme` 与 `cupertino/text_theme.dart` 的 `CupertinoTextThemeData`。覆盖率
1845/1888(97.7%)。测试 3548。**cupertino 是十层里最后一层,还剩 43 类。**

**上一轮那个 `MaterialBasedCupertinoThemeData` 靠着 `.noDefault()` 才能把窟窿留给 Material 去填,当时只
能从外面推断这套机制。这一轮把它翻出来了。**

```dart
class CupertinoThemeData extends NoDefaultCupertinoThemeData with Diagnosticable
```

**继承的方向值得留意:带默认值的那个,是不带默认值那个的子类。** 这是对的——把默认值填上是加行为,不是减
行为——但它带来一个后果:在这个类里面,`super.primaryColor` 是「人家跟我说的」,`this.primaryColor` 是
「我答出来的」,而每个 getter 都写成后者由前者定义:

```dart
Color get primaryColor => super.primaryColor ?? _defaults.primaryColor;
```

于是 `noDefault()` 的写法就顺理成章:**每一个字段都走 `super.` 读,刻意绕开这个类自己的补默认 getter。**
它答的是「我被告知了什么」,不是「我会说什么」。**而这个区分,正是上一轮那个 Material 适配器能落到
Material 上的全部原因。** 基类自己的 `noDefault()` 是 `=> this`——它本来就没有什么可剥的。

回归行里专门写了一条,把这一轮的 `no_default()` 直接喂给上一轮那个桥,断言 Material 的颜色真的透了过来,
并把「若不剥默认值,iOS 蓝会赢」也一并钉住。

---

**而私有的 `_CupertinoThemeDefaults` 里有一个字段和别的都不一样:**

```dart
final Brightness? brightness;   // 其余 Color / bool 字段全是非空的
```

**连「负责把没人说的填上」的那一层,都没有一个默认的 brightness。** 因为真正的默认值不是个常量:

```dart
return inheritedTheme?.theme.data.brightness ?? MediaQuery.platformBrightnessOf(context);
```

**一个没写 brightness 的主题,意思是「跟着设备走」**——而这个「没写」必须一路活到 `MediaQuery` 那里,包括
穿过那一层专门用来消灭「没写」的东西。所以这一个字段一直开着口子到底。

---

其余几条:

* **`resolveFrom` 在同一个方法里逐字段地混用 `super.` 和普通读法**:颜色走 `super.` 再 resolve,而
  `brightness` 和 `applyThemeToAll` 直接读。**这不是不一致。** 颜色是靠拿到 context 才解析出来的,所以必
  须保住「调用者到底说没说」——在这儿把默认值解析进去,它就被烤死了,之后 `noDefault()` 会交出一个没人要
  求过的值。而后两个没有什么可解析的,取补过默认的那个读数反而正是有用的那个。
* 文本主题的默认值是**有条件**解析的(`resolveTextTheme ? ... : ...`):调用者自己给的文本主题已经自己解
  析过了,这个标志是为了别做第二遍。
* 类文档说:「if a `primaryColor` is specified, it would cascade down to affect some fonts in
  `textTheme` if `textTheme` is not specified」——**一个只填了一半的主题,不是「你给的那些 + 其余照抄默认
  值」;你给的那些会改变其余的默认值。**
* `InheritedCupertinoTheme.updateShouldNotify` 比的是 `theme.data != oldWidget.theme.data`——**比的是数
  据,不是 widget。** 一个重建出来、但携带相等 `CupertinoThemeData` 的 `CupertinoTheme`,谁也吵不醒。

验证:`cargo test --lib` 3548 绿,GN `rustflutter_unittests` 3548 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1845 accounted / 43 MISSING(97.7%)。

### 一边是活的视图,另一边是拍下来的照片(2026-08-20)

新模块 `theme_bridge.rs`,收掉 material 层最后四个类:`MaterialBasedCupertinoThemeData`、
`CupertinoBasedMaterialThemeData`(`theme_data.dart`)、`TextFormField`、
`UserAccountsDrawerHeader`。覆盖率 1842/1888(97.6%)。测试 3532。

**`material/` 这一层到此 MISSING 归零。十层里已经完成九层,只剩 `cupertino/` 的 46 个类。**

---

**两个跨设计语言的适配器,名字对仗,东西根本不对仗。**

`MaterialBasedCupertinoThemeData` **继承 `CupertinoThemeData`**,把每个 getter 都改写成「先问 override,
再问 Material」:

```dart
Color get primaryColor => _cupertinoOverrideTheme.primaryColor ?? _materialTheme.colorScheme.primary;
```

**每次访问都现问一遍。这是一个活的视图。**

而 `CupertinoBasedMaterialThemeData` **什么都不继承**。它是一个只有一个字段的盒子,构造的时候用
`ColorScheme.fromSeed` 算一次就完事:

```dart
CupertinoBasedMaterialThemeData({required CupertinoThemeData themeData})
  : materialTheme = ThemeData(colorScheme: ColorScheme.fromSeed(seedColor: themeData.primaryColor, ...));
```

**一边是视图,一边是快照。** 而这个不对称是数据本身逼出来的:**Material 主题回答得了任何 Cupertino 的问
题**——Cupertino 问的就是那么几个它手上现成的颜色;**反过来,四个 Cupertino 颜色回答不了任意一个 Material
问题**,所以它不「转发」,它拿主色当种子把整套 `ColorScheme` **生成**出来交给你。

顺带:种子给了主色,`primary` 又显式传了一遍同一个颜色——**生成的那套负责填上没人说过的,不负责改写说过
的。**

**而让整条 `??` 链能够落到 Material 上的,是那个 `.noDefault()`。** `_cupertinoOverrideTheme` 的类型是
`NoDefaultCupertinoThemeData`——一个把默认值全剥掉的主题。**「没设」必须和「设成了 iOS 默认值」区分得
开**,否则每个 `??` 都会在一个 iOS 默认颜色上短路,Material 那一半永远轮不到。回归行专门把这条钉住了:
把 override 改成带着已解析的 iOS 默认值,五条红。

`copyWith` 的文档也异常直白:「**No derived attributes from iOS defaults or from cascaded Material theme
attributes are copied**」「This copyWith cannot change the base Material ThemeData」——**复制这个主题,
复制的是它「被告知」的东西,不是它「回答」的东西。**

---

**其余几条:**

* `TextFormField` 有九条构造器 assert,其中 `maxLength == null || maxLength == TextField.noMaxLength ||
  maxLength > 0` **把一个哨兵值从正数检查里挖了出来**——而 `noMaxLength` 是 **-1**。**一个负数表示「没有
  上限」。** 而 null 已经表示「根本不显示计数器」了,所以这第二种「没上限」是另一件事:**显示计数器,一直
  往上数,永不拦你。** 三种状态,两种都叫「没有最大长度」。
* `assert(!obscureText || maxLines == 1, 'Obscured fields cannot be multiline.')`——又一条单向蕴含,而且
  `maxLines: null`(不限行)也算多行,一样被拒。
* `UserAccountsDrawerHeader.build` 开头三条 assert,**其中两条一模一样**:
  ```dart
  assert(debugCheckHasDirectionality(context));
  assert(debugCheckHasMaterialLocalizations(context));
  assert(debugCheckHasMaterialLocalizations(context));
  ```
  无害——这个检查没有副作用、永远返回 true——写在这里,只因为**它正是那种会让读者盯着找区别的东西。没有
  区别。**

验证:`cargo test --lib` 3532 绿,GN `rustflutter_unittests` 3532 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1842 accounted / 46 MISSING(97.6%),**material 层 MISSING 归零**。

### 上一轮是两个文件切在不同处,这一轮是一个类里的两个方法(2026-08-20)

新模块 `text_toolbars.rs`(`AdaptiveTextSelectionToolbar`、`SpellCheckSuggestionsToolbar`、
`SpellCheckSuggestionsToolbarLayoutDelegate`),并把 `RestorableTimeOfDay` 补进 `pickers.rs`。覆盖率
1837/1888(97.3%)。测试 3514。

`AdaptiveTextSelectionToolbar` 里有两个平台 switch,**Fuchsia 在这两个里站在不同的队里**:

```dart
// getAdaptiveButtons
case TargetPlatform.fuchsia:
case TargetPlatform.android:      // → TextSelectionToolbarTextButton(Material 的按钮)

// build
case TargetPlatform.fuchsia:
case TargetPlatform.linux:
case TargetPlatform.windows:      // → DesktopTextSelectionToolbar
```

**于是 Fuchsia 上的上下文菜单,是 Android 的按钮装在桌面的工具栏壳子里。** 六个平台里只有它挪了位置,另外
五个在两个 switch 里都对得上——回归行专门把这一点钉住了,**因为「只有它一个不一致」正是它值得被指出来、而
不是耸耸肩的原因。**

上一轮记的是这件事发生在**两个文件**之间(图标按苹果/其余切,滚动行为按桌面/触摸切),当时的结论是「每个
问题按自己需要的地方下刀」。**这一轮是同一个类的两个方法。** 按钮样式和外壳样式确实是两个问题,Fuchsia 想
要 Material 的按钮配桌面的框也说得通——**但这里没有任何一句话是这么说的。** 照它的行为移植,把这处分歧钉
住而不是抹平。

---

**第二件:拼写建议的工具栏是滑的,不是翻的。**

```dart
anchor.dy + childSize.height > size.height ? size.height - childSize.height : anchor.dy
```

框架里别的选区工具栏都有「上锚点」和「下锚点」,放不下就翻到另一边。**这一个只往下放**——往上就会盖住你正
在改的那个词——**放不下的时候只往上挪恰好够的那么多,一点不多。**

而且结果没有下界:工具栏比可用空间还高时,`size.height - childSize.height` 是负的,它就挂在屏幕上边外头。
**上游在这儿什么都不夹**,道理是:溢出比盖住那个词好。

顺带两条数字:`_kMaxSuggestions = 3`,而构造器的 assert 写的是 `<= _kMaxSuggestions + 1`——**写成 3+1 而
不是 4,是在说第四个不是建议**(是底下那个删除按钮)。高度则是
`_kDefaultToolbarHeight - (48.0 * (4 - buttonItems.length))`,而 `_kDefaultToolbarHeight` 是 **193 = 4×48
+ 1**:**这条栏在每一种尺寸上都比它那几行高出一个像素**,因为它是从一个四行的常量往下减的。又一个没人解释
过的数,原样留着——矮一个像素是没人要求过的可见改动。

---

**第三件,一个只有把它改得「看起来对」才会出错的顺序。**

```dart
Object? toPrimitives() => <int>[value.minute, value.hour];

TimeOfDay fromPrimitives(Object? data) {
  final timeData = data! as List<Object?>;
  return TimeOfDay(minute: timeData[0]! as int, hour: timeData[1]! as int);
}
```

**分钟在前,小时在后**——和人写时间的顺序相反,也和 `TimeOfDay` 自己构造器的参数声明顺序相反(为了对上下
标,这里的具名参数是倒着写的)。

两头是一致的,所以它 round-trip 是对的。**值得写下来的是它招来的那种错法:两个字段都是小整数,只改一头
不改另一头,什么都不会崩。** 一个顺手把 `[minute, hour]` 「理顺」成 `[hour, minute]`、又漏了读那一侧的人,
会把四点半存成半点四分。回归行从两个方向都测了,而把写入侧单独「理顺」跑一遍,round-trip 那条当场红。

验证:`cargo test --lib` 3514 绿,GN `rustflutter_unittests` 3514 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1837 accounted / 51 MISSING(97.3%)。

### 开着读屏器的时候,那个淡出整个不发生(2026-08-20)

新模块 `app_bar.rs`(`SliverAppBar`)与 `icons.rs`(`Icons`、`PlatformAdaptiveIcons`)。覆盖率
1833/1888(97.1%)。测试 3494。

```dart
final double toolbarOpacity = !accessibleNavigation && (!pinned || isPinnedWithOpacityFade)
    ? clampDouble(visibleToolbarHeight / (toolbarHeight ?? kToolbarHeight), 0.0, 1.0)
    : 1.0;
```

**`accessibleNavigation` 把这段淡出整个关掉,不是调暗,是不发生。** 读屏器开着的时候,工具栏无论滚出去多
少都保持全不透明——**因为一个淡了一半的工具栏照样能被聚焦、照样会被念出来,淡掉它只会留下一个「读屏器够
得着、人眼看不见」的控件。**

**无障碍那条路不是普通那条路把某个数字拧小,它是另一个答案。** 和第 138 轮那条给 VoiceOver 让路的整整一
秒延迟是同一件事的两个样子。

---

**一处需要更正我自己的预期。** 我先写的回归行断言「收起到八成时工具栏已经在淡了」——红了,而且实现是对
的。

算一遍:expandedHeight 200、toolbar 56、没有 bottom,于是 minExtent 就是 56。整个收起过程里
`visibleToolbarHeight` 从 144 降到 56,除以 56 之后一路被 clamp 在 1.0。**收起的时候工具栏根本不淡——因为
栏在丢掉的是它展开出来的那部分空间,而工具栏正是这部分底下剩下的那个东西。** 淡出属于收满之后继续往上走
的那一段:不钉住的栏会接着滚,直到离开视口顶部。

改成照实测的四个点(0 处满、收满时仍满、再走一段开始淡、走完为 0),并把这句话写进了测试名。

---

**其余在 `SliverAppBar` 里的:**

* `assert(floating || !snap, 'The "snap" argument only makes sense for floating app bars.')`——又一条**单向
  蕴含**,而且消息把理由说成了人话:**snap 是浮动栏在你半途松手时做的事,不会浮动的栏没有什么可 snap 的。**
  三个构造器(small / medium / large)里这三条 assert 一字不差地各写了一遍。
* `maxExtent` 外面套的那个 `math.max(..., minExtent)` **是防止表头翻过来的**:一个比收起高度还小的
  `expandedHeight` 会让「展开」比「收起」还矮,底下每一条收缩计算都会倒着跑。
* `_isPinnedWithOpacityFade` 用**四个条件**点出一种布局:钉住、且浮动、且有 bottom、且没有额外的工具栏高
  度。**这是钉住的栏唯一被允许淡出的场合**——工具栏滑走,把 bottom 留在那儿。而 `bottomOpacity` 那边
  `pinned ? 1.0 : ...` 没有对应的例外:**在那种布局里,bottom 恰恰就是留下来的那一半。**

---

**另一件,两个相邻文件把同样六个平台切在了不同的地方。**

`PlatformAdaptiveIcons._isCupertino()` 画的线是**苹果对其余**:macOS 跟 iOS 一边,Linux 和 Windows 跟
Android、Fuchsia 一边。而上一轮 `MaterialScrollBehavior` 画的线是**桌面对触摸**:三个桌面一边。

**iOS 和 macOS 在这两条线上分处两侧。** 哪一条都不是「那个平台划分」——**每个文件按自己那个问题需要的地
方下刀**:图标问的是用户期待哪套视觉语言,滚动条问的是有没有一个光标能去抓它。

还有一处形状上的事:`Icons` 全是 `static const`,而 `PlatformAdaptiveIcons` 的每一个成员都是**实例
getter**。**一个 const 没法问自己跑在什么平台上。** 所以自适应那套不能是编译期折叠掉的常量命名空间,只能
是一个每次访问都求值的对象——这正是 `adaptive` 返回实例而不是又一个命名空间的全部理由,也是为什么你写
`Icons.arrow_back` 却要写 `Icons.adaptive.arrow_back`。

(顺带:`final class PlatformAdaptiveIcons implements Icons` 里那个 `implements` **什么也没承诺**——
`Icons` 只有静态成员,而 Dart 不继承静态成员。它是个名字。)

上游 `icons.dart` 是 29,454 行、8,825 条 `static const IconData`,整块夹在 `// BEGIN GENERATED ICONS`
里,上面写着 `// Generated code: do not hand-edit.`。**把九千个码位搬过来,读者从字体里能得到的东西一点没
多**,所以这边只留了有代表性的一小把和外面那层机制——有话可说的是那一层。

验证:`cargo test --lib` 3494 绿,GN `rustflutter_unittests` 3494 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1833 accounted / 55 MISSING(97.1%)。

### `DateTime.sunday` 是 7,而那条注释写着 6——写了两遍(2026-08-20)

新模块 `material_app.rs`,收掉 `app.dart`(`MaterialApp`、`MaterialScrollBehavior`)与
`material_localizations.dart`(`MaterialLocalizations`、`DefaultMaterialLocalizations`)。覆盖率
1830/1888(96.9%)。测试 3472。

```dart
// Ordered to match DateTime.monday=1, DateTime.sunday=6
static const List<String> _shortWeekdays = <String>['Mon', ..., 'Sun'];

// Ordered to match DateTime.monday=1, DateTime.sunday=6
static const List<String> _weekdays = <String>['Monday', ..., 'Sunday'];
```

**Dart 的 `DateTime.sunday` 是 7。** 这条注释在同一个类里出现两遍,两遍都把**下标**写在了**常量**的位置
上:表是七项,按 `date.weekday - DateTime.monday` 取,周日落在下标 6——注释想说的是这个,写出来的却是一句
关于 Dart 常量的假话。

**代码是对的,错的只有注释。而这种错法很费读者的时间:照它的字面算,`sunday - monday = 6 - 1 = 5`,而下标
5 是周六。** 回归行把两边都钉住了——七个工作日各自映射到自己的名字、下标 5 确实是 Sat、以及 `SUNDAY` 就是
7;把常量按注释改成 6,那条会红。

---

**第二件:被装饰的滚动条和不被装饰的,连起来看是一条规律。**

`buildScrollbar` 里,**横向滚动在任何平台上都拿不到滚动条**——`Axis.horizontal` 那一支在问平台之前就把
child 原样返回了。纵向的里头,只有三个桌面平台有。触摸平台上你是拽着列表本身滚的,一根用来抓的条子是没
人用的家具。

而那句 `assert(details.controller != null)` **只待在桌面那一支里**:滚动条需要有个东西可以挂上去,不造滚
动条的平台自然没这个要求。回归行专门断言了「要求 controller 的场合」和「造滚动条的场合」是同一批。

`buildOverscrollIndicator` 那边**只装饰 Android**。乍看奇怪,和物理对上就顺了:**iOS 的滚动会回弹,而那
一下回弹本身就是「到头了」的反馈;Android 是钳住的,于是必须画点什么出来说这件事。** 桌面在光标底下滚,
也什么都不给。

**所以拿到 overscroll 指示的那些平台,恰好就是那些物理本身不会告诉你边界在哪的平台。**

---

其余几条:

* **`tabLabel` 断言 `tabIndex >= 1`——它是一基的,而第 84 轮那个 `TabController.index` 是零基的。**
  读屏器念出来的号码,不是代码用来数数的号码。把 controller 的 index 直接递进去,第一个标签就会念成
  「Tab 0 of 3」并当场踩中断言。**这条 assert 站的正是两套计数法交界的地方。**
* `MaterialApp` 的 `debugShowMaterialGrid` 那个 `GridPaper` **是包在 `assert(() { ... }())` 里的**——
  release 下不是「默认关着」,是**那段画它的代码根本不在**。
* `DefaultMaterialLocalizations` 的文档自己说了它是什么:「for **US English (only)**」,而
  `formatCompactDate` 上面还压着一句 `// Assumes US mm/dd/yyyy format`。**这个「默认」大方承认自己是一种
  locale,不是中立的那个。** 年份还走了另一条路(`padLeft(4, '0')`),因为它不是个两位数。
* 而 `_formatTwoDigitZeroPad` 带着范围 assert,`formatMinute` 却把同样的补零逻辑**又内联写了一遍**——同一
  条规则在同一个类里存在两份,一份有范围检查,一份没有。
* `MaterialApp.router` 的 `assert(routerDelegate != null || routerConfig != null)` 是**「至少给一个」**,
  在这一周的排他、单向蕴含之后又添一种形状。

验证:`cargo test --lib` 3472 绿,GN `rustflutter_unittests` 3472 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1830 accounted / 58 MISSING(96.9%)。

### 下拉框得能显示出它自己现在的值(2026-08-20)

新模块 `paginated_data_table.rs`(`PaginatedDataTable`、`PaginatedDataTableState`),并把
`PopupMenuItemState`、`PopupMenuButtonState` 补进已有的 `menu.rs`。覆盖率 1826/1888(96.7%)。测试
3456。

**`PaginatedDataTable` 一个构造器里九条 assert,凑齐了参数检查的好几种形状——其中三种值得分开说。**

**一、蕴含,不是互斥。**

```dart
assert(actions == null || (header != null)),
```

**actions 需要 header,header 不需要 actions。** 这既不是「至多给一个」,也不是上一轮那条「要么都给要么
都不给」——**是单向的**。原因很实在:actions 是画在 header 那一行里的,没有 header 就没地方放。

**二、有条件的 assert。**

```dart
assert(() {
  if (onRowsPerPageChanged != null) {
    assert(availableRowsPerPage.contains(rowsPerPage));
  }
  return true;
}()),
```

**一个改不了的每页行数,可以是任何数;一个能改的,就必须是候选列表里有的那个——因为那个下拉框得能显示出
它自己现在的值。** 回归行把两边都钉住了:`rowsPerPage: 7` 而候选里没有 7,不给 `onRowsPerPageChanged` 时
通过,给了就被拒。把这条改成无条件的,那一条会红。

**三、把退化情形一次排除掉,后面的检查就能是干净的检查。**

```dart
assert(columns.isNotEmpty),
assert(sortColumnIndex == null || (sortColumnIndex >= 0 && sortColumnIndex < columns.length)),
```

**和第 84 轮的 `TabController` 正好是两条路。** 那个允许 length 为 0,于是每一条后续 assert 都得挂一个
`length == 0 ||` 的活口——而那个活口最后成了「文档说 index 必须是 0,assert 却放 47 过去」的来源。这里
**开头一句 `columns.isNotEmpty` 把退化情形排掉,下面那条范围检查就真的只是一条范围检查。**

---

**还有一处,是被废弃参数的体面退场:**

```dart
assert(
  dataRowHeight == null || (dataRowMinHeight == null && dataRowMaxHeight == null),
  'dataRowHeight ($dataRowHeight) must not be set if dataRowMinHeight ... are set.',
),
dataRowMinHeight = dataRowHeight ?? dataRowMinHeight,
dataRowMaxHeight = dataRowHeight ?? dataRowMaxHeight,
```

**assert 不许你把旧的单值和取代它的区间混着说,紧接着的初始化就把那个单值塌进区间里。** 一个固定行高活
下来的样子,是一个宽度为零的区间(min == max),而下游只认那一对。

顺带一条:`pageTo` 的名字在悄悄做事——`(rowIndex ~/ rowsPerPage) * rowsPerPage`。**它收的是行号,给你的
是那一行所在的那一页**,想停在半页中间是停不住的。而 `onPageChanged` 只在**取整之后**的下标变了才发,所以
在同一页里换个行号,是无声的。

---

**另一件:菜单项先把自己关掉,再把控制权交出去。**

```dart
// Need to pop the navigator first in case onTap may push new route onto navigator.
Navigator.pop<T>(context, widget.value);
widget.onTap?.call();
```

**不是为了整洁。** 一个会 push 新路由的回调,若是先跑,那么本该关掉菜单的那次 pop 就会把它刚推上去的路
由弹掉。**所以菜单趁自己还认得哪条路由是自己的时候先退场,然后才随调用者去动 navigator。** 和第 83 轮那
条 elevation 链是同一个形状:**次序是承重的,而且作者写了注释说它承重。** 回归行把两行对调跑了一遍,红。

而 `PopupMenuButtonState` 里那两个缓存(`_cachedButtonRenderBox` / `_cachedOverlayRenderBox`)带着注释和
一个 issue 链接:**这是一份为「活多久」而不是为「快多少」留的缓存。** 定位函数在菜单路由做动画的时候会跑
——包括退场那一程,而那时按钮自己的 render object 可能已经没了。**提前把盒子取下来攥着,菜单才能对着一个
已经离场的按钮把自己摆完。**

验证:`cargo test --lib` 3456 绿,GN `rustflutter_unittests` 3456 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1826 accounted / 62 MISSING(96.7%)。

### 这一周的多参数 assert 全是「至多给一个」,这条是反过来的(2026-08-20)

新模块 `chip.rs`(`RawChip`),并把 `Autocomplete` 补进已有的 `autocomplete.rs`。覆盖率 1822/1888
(96.5%)。测试 3436。

**`RawAutocomplete` 的三条 assert 里,中间那条的形状这一周还没见过:**

```dart
assert((focusNode == null) == (textEditingController == null)),
```

**两个判空之间的一个等号。** 不是「至多给一个」,也不是「至少给一个」——**要么都给,要么都不给。**

这一周记下的多参数 assert 全是前一种:stepper 的三个 extent 来源、chip 的两个回调、reorderable list 的两
个回调。**这是头一条反过来的。** 而它反过来是有道理的:焦点和文字是同一个输入框的两半,**你可以让 widget
全权拥有它,也可以自己全权拥有它,但没有一半的版本**——自己攥着 focus node、让 widget 去造 controller,
那是把一个东西的两半交给了两个主人。

而第一条 assert 正是 `Autocomplete` 不必操心这一切的原因:**它总会给一个 `fieldViewBuilder`**,于是那条
「否则你得同时给出 key、focusNode 和 controller」的分支,从 Material 这一层根本够不着。

---

**第二件:`tapEnabled` 这个名字说的是杠杆,它的文档说的是拉杠杆的理由。**

> If set, this indicates that the chip should be **disabled if all of the tap callbacks are null**. For
> example, the `Chip` class sets this to false because it **can't be disabled**, even if no callbacks
> are set on it, since it is used for displaying information only.

**一个普通的 `Chip` 身上一个回调也没有,而它绝不该看起来是灰的。** 于是它把 `tapEnabled` 关掉——**关掉的
不是「能不能点」,是「要不要从『没有回调』推出『它被禁用了』」。** 点还是点不动,但它看上去是个标签,不是
一个死掉的按钮。

**这是这一周第三个这种名字**:第 84 轮 `indexIsChanging` 按「因为什么」命名,第 82 轮
`ListTileControlAffinity.platform` 按错了的那条轴命名,现在是一个名字描述机制、文档描述意图的。回归行把
两者的差别钉死了:同一个没有回调的 chip,`tapEnabled` 开着是「看起来禁用」,关着是「看起来正常」;把
`looks_disabled` 改成不看 `tapEnabled` 的那个天真版本,那条会红。

---

**第三件,一处需要更正我自己的初读。** 我先是以为「`onPressed` 和 `onSelected` 不能同时给」这条只写在文
档里、没有 assert——**不对,assert 是有的,只是不在构造器里,在 `initState` 里。**

而这个位置本身有后果:**构造器的 assert 在 widget 被造出来时就响;`initState` 的 assert 只在元素被挂上树
时才响。** 一个造出来放进列表、始终没插进树的 chip,永远碰不到它。

再往下看那个点击处理:

```dart
widget.onSelected?.call(!widget.selected);
widget.onPressed?.call();
```

**两行,谁也不挡谁。两个都给了,两个都会调**——而这正是 release 构建里会发生的事,因为那里 assert 已经
没了。**底下这段代码完全容得下 assert 所禁止的那种状态。** 回归行把这件事照实钉住了。

验证:`cargo test --lib` 3436 绿,GN `rustflutter_unittests` 3436 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1822 accounted / 66 MISSING(96.5%)。

### 两个滑块叠在一处的时候,它不猜(2026-08-20)

新模块 `range_slider.rs`(`RangeSlider`),并把 `ReorderableListView`、`TooltipVisibility`、
`TooltipState` 补进已有的 `reorderable_list.rs`。覆盖率 1820/1888(96.4%)。测试 3422。

**先说一件差点重复劳动的事:动手前照例翻了一遍已有的模块,发现 `reorderable_list.rs` 里已经有
`reorder_report`,而且把 `onReorder` / `onReorderItem` 那条差异记得比我这轮草稿更准。** 于是这一轮只补了
Material 那层的 `ReorderableListView`,让它去调已有的那份算术,没有再写一遍。**尺子说 MISSING,不等于那
个文件是空的**——这条第 59 轮就写下来过,这次是它头一回省下事而不是闯出祸。

---

**`_defaultRangeThumbSelector` 的重点,在它会返回 null。**

区间滑块有两个滑块头。**两个头叠在同一处,是读者完全会做的事**(把区间收成一个点),而这时候一次触摸同时
落在两个触摸区里——**手指底下的位置没法说清你要的是哪一个。**

```dart
if (inStartTouchTarget && inEndTouchTarget) {
  final (bool towardsStart, bool towardsEnd) = switch (textDirection) {
    TextDirection.ltr => (dx < 0, dx > 0),
    TextDirection.rtl => (dx > 0, dx < 0),
  };
  if (towardsStart) return Thumb.start;
  if (towardsEnd)   return Thumb.end;
}
...
return null;
```

**它不猜。它谁也不选,然后等着。** 而 `dx` 是拖动至今的位移,**按定义,第一次按下的时候它是 0**。

解开这个结的,是第一次非零的位移:**你往哪边开始动,就说明你抓的是哪一个。** 往左是 start,往右是 end。
这与其说是启发式,不如说是**唯一能落地的读法**——你手里真攥着的那个头,只可能是从另一个头旁边被拉走的
那个。

而且看的是**屏幕上的方向,不是数值上的**:RTL 下 start 画在右边,两边就对调。回归行把这一整串钉住了:
静止时是 None、最小的一点位移就够、RTL 下同样的手势抓到的是另一个,以及**这块含混区间的宽度随滑块头变宽
而变宽**(10px 的头在 2000px 轨道上隔着 0.1 绰绰有余,300px 的头就够着了)。随后把那个 None 改成硬猜
Start 跑了一遍,四条红。

---

其余几条:

* **同一条「每个孩子都得有 key」的规则,被用两种机制执行了两遍**:列表构造器上是
  `assert(children.every((w) => w.key != null))`,而 builder 那条路是 `_itemBuilder` 里逐个抛
  `FlutterError`。**这不是双保险——builder 的条目在有人滚到它之前根本不存在,唯一能查的时刻就是它出现的
  那一刻。两个构造器拿到的是同一条规则,和各自唯一能有的那种执行方式。**
* 那条 extent 的 assert 写成三个「两两都为空」的或:**每一项点的是缺席的那两个**,合起来读是「三个里有
  两个没给」。等价于「至多给一个」,但要愣一下才看得出来。
* **`TooltipVisibility.of` 在完全没有祖先的时候返回 `true`。** 一个「不存在即是同意」的 inherited
  widget——也只能这样:tooltip 本来就不需要谁来开启,这个 widget 是用来给一棵子树**关掉**它的。
* `TooltipState.ensureTooltipVisible` 在 raw state 还没建出来时返回 `false`。**它答的不是「现在可见吗」,
  是「有没有东西可问」**——还没建出来的 tooltip 没法显示,如实说比抛出去有用。

验证:`cargo test --lib` 3422 绿,GN `rustflutter_unittests` 3422 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1820 accounted / 68 MISSING(96.4%)。

### 那条 assert 放过去的那一种,恰好长成它要拦的样子(2026-08-20)

新模块 `stepper.rs`,一次收掉 `stepper.dart`(`ControlsDetails`、`Step`、`Stepper`、`StepStyle`)与
`toggle_buttons.dart`(`ToggleButtons`)。覆盖率 1816/1888(96.2%)。测试 3401。

**上一轮刚记下「文档和它旁边那条 assert 说的不是一回事」,这一轮又撞上一次——而且这次能一路推到底。**

```dart
assert(
  stepIconHeight == null || stepIconWidth == null || stepIconHeight == stepIconWidth,
  'If either stepIconHeight or stepIconWidth is specified, both must be specified and '
  'the values must be equal.',
);
```

**消息说「只给一个是错的」。条件根本没查这件事**——只给 height、width 留 null,第二项直接把检查短路掉,
assert 过。

而往下三百行,画的时候是这么取的:

```dart
width: _stepIconWidth ?? _kStepSize,
height: _stepIconHeight ?? _kStepSize,
```

**于是 `stepIconHeight: 40` 单给一个,画出来是 24 × 40。这条 assert 存在的全部理由就是「别弄成不方的」,
而它放过去的那一种,恰好就是不方的。** 把同一件事明说出来(`height: 40, width: 24`)反而会被拦下。

回归行把这三步分别钉住:半给通过、算出来 24×40、明说被拒。随后把 assert「修」成消息声称的样子跑了一遍,
确认那条会红。

顺带:范围消息写的是 *"must be greater than 24.0"*,代码是 `>= _kStepSize`——**24.0 正好是许可的**,也钉了
一条。

---

**第二件:`ToggleButtons` 的 `hitTest` 只把一条轴压平,另一条原样留着。**

```dart
// Only adjust one axis to ensure the correct button is tapped.
final Offset center = switch (direction) {
  Axis.horizontal => Offset(position.dx, child!.size.height / 2),
  Axis.vertical   => Offset(child!.size.width / 2, position.dy),
};
```

**这是这几天第三个「按得着的比画出来的大」**(第 59 轮滚动条、第 83 轮 icon button),但只有这一个是
**故意只扩一条轴**。它不得不这样:按钮为了凑够 tap target 在纵向补了白,所以点在按钮上方的空白里也得算数
——**可这些按钮在横向是肩并肩排着的,横轴也压平的话,每一次点击都会落到「先被问到的那个邻居」身上。**

**两条轴一起扩,对一个孤零零的控件是对的,对一排控件恰好是错的。** 滚动条和 icon button 可以用省事的那
个版本,这个不行。

---

其余几条:

* **`assert(widget.steps.length == oldWidget.steps.length)`,而 `steps` 的文档写着「The length of steps
  must not change」——列表的长度是这个 widget 身份的一部分。** 这在 Flutter 里不常见。原因就在它下一行:
  state 按下标把每个 step 的旧状态存起来,好让圆圈从 `indexed` 动画到 `complete`。**中间插一个,后面每个
  圆圈都会从别人的过去开始动。**
* **`ControlsDetails` 带两个下标**:`currentStep` 和 `stepIndex`。**因为切换的过程中两个 step 同时在屏幕
  上**,builder 两个都会跑一遍,得能分清这次跑的是哪个。**和上一轮 tab 那条是同一个形状:过渡期间有两个
  东西,代码就得能把两个都叫出名字。**
* 而同一个文件里有两个 `isActive`,意思不一样:`Step.isActive` 的文档是「**The flag only influences
  styling**」,调用者自己设;`ControlsDetails.isActive` 是 `currentStep == stepIndex` 算出来的。
* `Stepper.build` 里有一条 debug assert 直接抛 `FlutterError`:「**Steppers must not be nested**」,并附上
  material 规范的链接。**这里不会坏任何东西——它不是布局约束,也不是代码依赖的不变量,它是一条设计规范,
  被框架当成错误来执行。**
* `ToggleButtons` 那条 build 期 assert 的消息里无保护地写了 `focusNodes!.length`。Dart 只在 assert 失败时
  才构造消息,而条件恰好保证那时它非 null——**正确,但只差一根头发。**

验证:`cargo test --lib` 3401 绿,GN `rustflutter_unittests` 3401 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1816 accounted / 72 MISSING(96.2%)。

### 那个标志按「因为什么」命名,不按「是不是」命名(2026-08-20)

新模块 `tabs.rs`,一次收掉 `tab_controller.dart`(`TabController`、`DefaultTabController`)、
`tabs.dart`(`Tab`、`TabBarScrollController`、`TabBarView`、`TabPageSelectorIndicator`、
`TabPageSelector`)与 `tab_indicator.dart`(`UnderlineTabIndicator`)。覆盖率 1811/1888(95.9%)。
测试 3382。

**`indexIsChanging` 的文档念起来是自相矛盾的:**

> True while we're animating from `previousIndex` to `index` **as a consequence of calling
> `animateTo`**. [...] It is **false** when `offset` is changing as a consequence of the user
> dragging (and "flinging") the `TabBarView`.

拖动的时候选中项**确实在变**,而这个叫「index 正在变」的东西是 false。**因为它问的不是「在不在变」,是
「在按哪一种方式变」——而这两种方式的算法根本不是同一个。**

`TabPageSelector._buildTabIndicator` 把这件事摊开了:一排小圆点,两条分支,两套完全不同的算术。

* **点选那一支**:进度从 previousIndex 走到 index,**中间被跨过去的那些标签一个都不亮**。从第 0 页点到
  第 4 页,1、2、3 全程是暗的。
* **拖动那一支**:能亮的只有当前那个和它紧挨着的一个,**因为一次拖动本来就到不了更远**。当前的按
  `1 - |offset|` 暗下去,邻居按 `|offset|` 亮起来,两个加起来恒为 1。

**这两条谁也推不出谁**:点选那一支需要 previousIndex,拖动那一支需要 offset,而各自在对方的处境里都没有
意义。所以这个标志必须存在,而且必须按「因为什么」命名。

配套还有一条:`offset` 的 setter 里写着 `assert(!indexIsChanging)`。**两种方式不只是被区分开,它们是互斥
的——点选的动画还在跑的时候,你没法拖。**

---

**第二件事:一句文档和紧挨着它的那条 assert 说的不是一回事,而输的是文档。**

```dart
/// The `initialIndex` must be valid given [length]. If [length] is zero, then
/// `initialIndex` must be 0 (the default).
...
assert(initialIndex >= 0 && (length == 0 || initialIndex < length)),
```

**`length == 0` 这一项把范围检查整个关掉了。** 于是 `TabController(length: 0, initialIndex: 47)` 是能造出
来的,而且**后面没有任何一步会去修它**。写这条移植的时候我按文档写了测试,结果红了——红的是测试,不是实现。

同一个洞在 `_changeIndex` 里又出现一次,而那里真正堵住它的是另一行:

```dart
if (value == _index || length < 2) return;
```

**`length < 2` 读起来像个抠性能的短路,它其实是那条不变量唯一的看守。** 没有它,一个零标签的 controller
连 `index = 47` 都会照收。回归行把这两条分别钉住了:零标签的赋值被拒,**而那个 47 仍然留在那儿**——它只是
不能再动,不是被修好了。

---

其余几条:

* **`_indexIsChangingCount` 是个计数器,不是布尔。** 因为动画会叠:第一个还没跑完就点了第二个,这时有两次
  变更在飞。**用布尔的话,先完成的那个会把还在跑的那个也报成「停了」。**
* 而不带动画的那一支写成 `+= 1; 赋值; -= 1;`——**计数事后永远看不出非零,但被那次赋值叫醒的监听者看得
  见。那对增减不是为动画的时长准备的,是为那一次通知准备的。**
* `UnderlineTabIndicator` 的 `lerpFrom` / `lerpTo` **都没把 `borderRadius` 传给新建的那个**,于是它退回
  null:**两个圆角下划线之间做动画,整个过程是方的,落地才弹回圆角。** 按上游的样子移植并记下——在这儿单方
  面修好,只会让两边在动画中途对不上。
* `TabBarScrollController` 整个类的存在理由写在它自己的注释里:**「只是为了处理可滚动 TabBar 带非零
  initialIndex 的情况」**。因为**要滚到哪儿才能看见第五个标签,得先知道 bar 有多宽,而那要等布局跑完。**
  所以位置先不带 pixels 建出来,等尺寸到了再自己纠正。而它下面还压着一条:**视口在真尺寸算出来之前会短暂
  地是 0**,少了那个 guard,第一次布局就会从一个不该信的位置弹射出去——**而且只在 release 里看得见。**
* `Tab` 的 `assert(text == null || child == null)`:**text 不是 child 的简写,是它的替代品**,两个都给是
  错误,不是优先级问题。而 72 那个高度是**给「上下摞两样东西」准备的,不是给「有图标」准备的**——只有图标
  的标签仍然是 46。

验证:`cargo test --lib` 3382 绿,GN `rustflutter_unittests` 3382 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1811 accounted / 77 MISSING(95.9%)。

### 那句「小心别重排」保护的四步里,有一步在默认值下根本看不出来(2026-08-20)

新模块 `buttons.rs`,一次收掉 `RawMaterialButton`(`button.dart`)、`MaterialButton`
(`material_button.dart`)、`IconButton`(`icon_button.dart`)与 `FloatingActionButton`
(`floating_action_button.dart`)。覆盖率 1803/1888(95.5%)。测试 3351。

**`_effectiveElevation` 上面挂着一句作者自己写的警告:**

```dart
// These conditionals are in order of precedence, so be careful about reorganizing them.
if (widget.onPressed == null) return widget.disabledElevation;
if (_pressed)  return widget.highlightElevation;
if (_hovered)  return widget.hoverElevation;
if (_focused)  return widget.focusElevation;
return widget.elevation;
```

**这四个状态可以同时为真——被禁用的按钮照样能被悬停,而用鼠标的话,按下就必然同时悬停着。** 所以必须有个
先后,而这条 if 链就是那份先后。禁用在最前,因为**被悬停的禁用按钮仍然是禁用的**;按下压过悬停,因为
**不这么排的话,鼠标用户永远看不到 highlightElevation** ——它会被 hover 那一条永远挡住。

**但把默认值填进去,这条链有一步是量不出来的:`focusElevation` 和 `hoverElevation` 都是 4.0。** 一个照默
认值配出来的按钮,把 focus 和 hover 两条对调,谁也看不出区别——用户看不出,测试也测不出。**真正在承重的
是「按下压过悬停」那一步,因为 8.0 ≠ 4.0。**

于是回归行不用默认值写,用五个互不相同的数,让每一步都可观测;并且专门留了一条把「默认值区分不了
focus 和 hover」这件事本身钉住——**不绕开它,而是把它记下来:先后仍然要紧,因为主题可以让这两个数不
一样。** 随后把 pressed / hovered 两条对调跑了一遍,确认那条回归行确实会红。

---

**`IconButton` 那条,是同一类事的另一种说法:**

> The hit region of an icon button will, if possible, be at least `kMinInteractiveDimension` pixels in
> size, **regardless of the actual `iconSize`**.

**画出来的东西和按得着的东西是两个尺寸**,按钮只把后者撑大,不动前者。默认情况下这个比例正好是二比一
——**画 24,按 48**,而 `alignment` 决定图标落在这块靶子的哪儿。48 是地板不是尺寸:上游文档自己举的
72 号图标,靶子就是 72。和第 59 轮滚动条的触摸扩张是同一件事:**手指不是光标,按得着的从来不等于看得见的。**

其余两条:

* `assert(splashRadius == null || splashRadius > 0)`——**半径为零的水波是「有反应但没墨」,这跟「不要反应」
  不是一回事**,所以 `None`(用默认)可以,`Some(0.0)` 不行。
* `FloatingActionButton` 的五个 elevation **全是可空的**,断言也跟着变形成
  `elevation == null || elevation >= 0.0`:**它默认谁也不覆盖,只改被交代过的那几个。**
* 而 `MaterialButton` 的文档把调用者指向别处(`TextButton` / `ElevatedButton` / `OutlinedButton`)。
  **它留着是因为已经有代码写着它,不是因为还该有人用它。**

验证:`cargo test --lib` 3351 绿,GN `rustflutter_unittests` 3351 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1803 accounted / 85 MISSING(95.5%)。

### 那个枚举值按错了的那条轴命名(2026-08-20)

新模块 `list_tiles.rs`,一次收掉 `CheckboxListTile`、`RadioListTile`、`SwitchListTile`。覆盖率
1799/1888(95.3%)。测试 3336。

**三个一起读,读出了一件它们各自都不会说的事。**

`ListTileControlAffinity` 有三个值:leading、trailing、**platform**。而 platform 的文档写着「position
the control relative to the text in the fashion that is typical for the **current platform**」。

**没有任何一个实现看过平台。** 它们看的是自己包着哪种控件:

```dart
// checkbox_list_tile.dart 和 switch_list_tile.dart
ListTileControlAffinity.trailing || ListTileControlAffinity.platform => (secondary, control),
// radio_list_tile.dart
ListTileControlAffinity.leading  || ListTileControlAffinity.platform => (control, secondary),
```

**所以这个值是有意义的——它的意思是「这类控件习惯待的位置」:单选惯常在前,复选和开关惯常在后。它只是按错
了的那条轴命名:它随控件变,不随平台变。**

按它实际的行为移植了,名字保留上游给的。回归行把三种控件在 `Platform` 下的落位分别钉住,并断言另外两个显
式值**完全不随控件变**——那才是 platform 那一条的对照。

---

其余几条:

* **`assert(tristate || value != null)`**——和第 61 轮那个 toggleable 是同一条规则:**只有三态控件可以是
  null。**
* **`assert(isThreeLine != true || subtitle != null)`**——**第三行总得从哪儿来。**
* **整个瓦片被包在 `MergeSemantics` 里。** 这既是「对读屏器来说它是**一个**东西,而不是一个复选框旁边有段
  无关的文字」,**也正是点标签能起作用的原因:标签不是第二个控件,它是这一个的一部分。**
* 而 affinity 的回退链是三层:widget → ListTileTheme → `platform`。**一个主题能一次把一整列的控件挪到另
  一边。**

验证:`cargo test --lib` 3336 绿,GN `rustflutter_unittests` 3336 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1799 accounted / 89 MISSING(95.3%)。

### 等一秒,是因为不等的话读者什么都听不见(2026-08-20)

新模块 `expansion.rs`,收掉 `ExpansionTile`(`expansion_tile.dart`)与 `ExpandIcon`
(`expand_icon.dart`)。覆盖率 1796/1888(95.1%)。测试 3327。

**这个文件里最不像话、也最值得记的,是一个整整一秒的延迟——而且只在 iOS 上。**

```dart
// TODO(tahatesser): This is a workaround for VoiceOver interrupting
// semantic announcements on iOS. https://github.com/flutter/flutter/issues/122101.
_timer = Timer(const Duration(seconds: 1), () { ... sendAnnouncement ... });
```

**VoiceOver 在瓦片开完的时候还在念这次点击,而在那当口发出的播报会被打断。** 于是这条播报被推迟一整秒。
**在别的所有地方,等待都比不等待糟;唯独在这里,它是「被听见」和「没被听见」的区别。**

而那一秒**是展开动画(200ms)的五倍**——回归行专门断言了这一点,**因为这正是「它等的不是动画」的证据。**

配套还有一条:**新播报排上之前会先取消掉挂着的那条**,于是一个被快速开合的瓦片说的是后一句,而不是两句
都说。

---

**`ExpandIcon` 的文档用一句话把受控控件的约定说清了:**

> Rebuilding the widget with a different `isExpanded` value will trigger the animation, but will not
> trigger the `onPressed` callback.

**动画跟着值走,回调报告的是那次按下。** 从外面改值会把箭头转过去,而**不会假装有人按过**——这正是「一个
能被驱动的控件」和「一个跟驱动它的人吵架的控件」之间的区别。

---

**`ExpansionTile.maintainState` 默认是 false:收起时孩子被从树里拿掉,展开时重建。** 于是一列五十个收起
的瓦片,代价是五十个标题,不是五十个页面。**想留住的,是那些重建代价高或者根本恢复不了的东西**——一张填了
一半的表、一段播到中间的视频。

而 controller 那条规则又出现了一次:**只销毁自己造的那个**,和二维滚动、搜索锚点一样。

验证:`cargo test --lib` 3327 绿,GN `rustflutter_unittests` 3327 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1796 accounted / 92 MISSING(95.1%)。

### 一个匆匆离开的打断,读起来像是弄错了(2026-08-20)

`ModalBottomSheetRoute`(`bottom_sheet.dart`)与 `DialogRoute`(`dialog.dart`)补进 `routes.rs`——它们正
是第 47 轮那个 `PopupRoute` 的两个 Material 子类。覆盖率 1794/1888(95.0%)。测试 3318。**首次过九成
五。**

**底部弹层的进出时长是刻意不对称的:进 250、出 200。到达该显得从容,离开该让开路。** 和抽屉的 settle、
tooltip 的淡出是同一个形状。**而对话框比弹层更短:150。一个对话框是打断,一个弹层是到来。**

**`reverseTransitionDuration` 一路回退四层,而第二层才是要点:**

```dart
transitionAnimationController?.reverseDuration ?? transitionAnimationController?.duration ?? ...
```

**一个只说了正向时长的 controller,被当成两个方向都是那个数。** 直接跳到默认值,会递给调用方一个他从没
要过的不对称——他的 400 进,框架的 200 出。

---

**`DialogRoute` 里那句「Prevent clicks inside the dialog from passing through to the barrier」,加的是
`Semantics(hitTestBehavior: opaque)`——是**语义**命中测试,不是指针的。**

指针点击本来就停在对话框自己的 Material 上;**这里挡的是一次辅助技术的激活落在对话框内部、却够到了后面那
层可点掉的遮罩——那会关掉读者正想用的东西。** 精确地说出这一点,比笼统地说「防止点击穿透」有用。

**而它的曲线两个方向都是 `easeOut`。** 这个框架里多数转场会在返回时把曲线翻过来,对话框不翻:**进来减
速,出去也减速,哪个方向都不会从读者面前加速逃走。它是一次打断,而一个匆匆离开的打断,读起来像是弄错
了。**

---

**一处自查:** 我写了「弹层的甩动阈值是抽屉的两倍」并按 `> 2 × 365 - 1` 断言,红了。**700 不是 730,是
365 的 1.92 倍。** 改成「将近两倍」并按 1.9 断言,同时在文档里补了一句:**这两个数显然是各自挑的,不是从
对方推出来的。**

**这正是一条真断言该做的事**——把一句听上去顺口、其实只是差不多的话拦下来。

其余两条:`barrierDismissible` 就是 `isDismissible`(**一个概念在两层各有一个名字**);而
scroll-controlled 的弹层**根本没有高度比例**,不是换了个更大的比例。

验证:`cargo test --lib` 3318 绿,GN `rustflutter_unittests` 3318 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1794 accounted / 94 MISSING(95.0%)。

### 布局变了,是因为地方不够了,不是因为谁做了选择(2026-08-20)

新模块 `bottom_bars.rs`,一次收掉底部那三条:`BottomNavigationBar`(M2)、`NavigationBar`(M3)、
`BottomAppBar`。覆盖率 1792/1888(94.9%)。测试 3306。

**`_effectiveType` 的默认值是按**数量**定的:三个及以下用 fixed,四个及以上用 shifting。**

**布局变了,是因为地方不够了,不是因为谁做了选择。** 一部手机的宽度放不下四条标签,于是只写选中的那一条,
其余的让位。回归行把 2–3 和 4–6 两段分别跑了一遍。

**而这里有一条断言值得单独说:「Every item must have a non-null label」——在 shifting 模式下标签根本不画,
它照样是必须的。** **把一个标签藏起来,不等于把它去掉**:它仍然是读屏器要念的东西,**一个没有标签的项,就
是一个没有名字的按钮。**

---

**Material 3 的 `NavigationBar` 保留了同样的两条断言,却完全没有 type。** 它不 shifting,于是那条按数量分
的默认值整个消失了:**不管几个目的地,每一个都保留自己的标签——设计一次把话说死,而不是随着数量去将就。**

---

**`BottomAppBar` 不是导航,是一条放动作的栏,而它之所以是单独一个类,是那个缺口。** 一个悬停在它上面的浮
动按钮,要在栏上挖一个洞;**而这个洞是栏的活,因为只有栏知道自己的轮廓。** `shape` 为 null 时是一个没有缺
口的矩形——**上面没有浮动按钮的栏,没有什么要绕开的。**

`notchMargin` 则是按钮和洞边之间留的那点空,好让两者不贴在一起。

最后:**横屏那三种布局里,「居中」存在的理由是「铺开」的问题**——五个项铺满一台横过来的手机,会离得离谱地
远,**一个够得到第一个的拇指够不到最后一个。**

验证:`cargo test --lib` 3306 绿,GN `rustflutter_unittests` 3306 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1792 accounted / 96 MISSING(94.9%)。

### 抓手要从系统留白里面开始(2026-08-20)

`DrawerController` 与 `DrawerControllerState` 补进既有的 `drawer.rs`,接上第 69 轮那个
`DrawerButton`。覆盖率 1789/1888(94.8%),**MISSING 落到两位数:99**。测试 3293。

**`_settle` 是三条路,而顺序就是规则:速度压过位置,但只在速度真的算一次甩动时。**

一个已经开了九成的抽屉,被一下弹回去,它会关;**在同一个位置轻轻松手,它会开。** 低于 365 px/s 那个门槛
时,那个动作里读不出意图,于是由中点决定。回归行把这一对放在同一个位置上对照。

**而 `visualVelocity = xVelocity / _width * _directionFactor` 里那个除法值得点名:controller 根本不知道
像素,它工作在 0..1 上。** 换算是抽屉的活,而**抽屉的宽度就是那个汇率**——同样的手指速度,越宽的抽屉在动
画里走得越慢,**这正是一个宽抽屉手感更沉的原因。**

---

**`dragAreaWidth` 是 20 加上「这个抽屉真正所在那一侧」的系统留白。**

一台边缘是曲面的手机,或者横过来时有刘海的,会在那条边上留下一片用不了的像素;**一条整个落在那片像素里
的抓手,会让这个抽屉根本拖不开。** 上游那个 2×2 的 switch 看着啰嗦,其实是一句话写开:**start 对齐读前
导那一侧,end 对齐读尾随那一侧,而哪边是哪边取决于阅读方向。** 四个分支各钉了一行。

**桌面上边缘拖动被整个关掉。** 桌面上没有「边缘划入」这件事——**从窗口边缘开始的拖动是缩放窗口或者框
选**,抢过来会同时弄坏这两件事,而且什么也换不回来:桌面上的抽屉是用按钮开的。

最后一条小的:**外对齐和内对齐是相反的。** 外是抽屉贴住的那条屏幕边,内是它滑动时内容被推向的那条边。

验证:`cargo test --lib` 3293 绿,GN `rustflutter_unittests` 3293 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1789 accounted / 99 MISSING(94.8%)。

### 「我能不能演出去」其实是「来的那个知不知道拿我怎么办」(2026-08-20)

新模块 `material_page.rs`,收掉 `material/page.dart` 三个类:`MaterialRouteTransitionMixin`、
`MaterialPageRoute`、`MaterialPage`。覆盖率 1787/1888(94.7%)。测试 3283。

**`canTransitionTo` 问的是「我可以演出去吗」,而它真正在问的是「来的那个知不知道拿我怎么办」。**

一条路由只在两种情况下演出场动画:**新来的那条用的是同一个 mixin**(于是两边本来就同步),**或者新来的那
条带着一个 delegated transition**(于是它会亲自驱动这条的退场)。**两者都没有,这条路由就干脆不动**——那比
两个各干各的动画在同一块屏幕上打架要好。

**而全屏对话框底下什么都不演:** 它盖住一切,**底下的运动是没有人看得见的运动。** `canTransitionFrom` 是
同一个判断的另一面:一个全屏对话框会压掉它盖住的那条路由的转场。

---

**还有一处值得小心地记下来:那句 `?? const Duration(microseconds: 300)`。**

**注意单位。** 附近每一个时长都是毫秒,而三百**微秒**是零点三毫秒——等于瞬间。**这是一个笔误。**

**而它是一个谁也碰不到的笔误:** 它上面那个按平台分支的 `switch` 对 `TargetPlatform` 是穷尽的,于是查找永
远不返回 null,`??` 永远不触发。

**我照原样移过来了,并且把这件事写进文档和回归行**——理由是:**一个里面装着错数字的死分支,恰恰是在有人加
一个新平台那天不再死的东西。** 这和第 74、75 两轮那两条不可达守卫是同一类处理:**说出来,而不是绕过去,也
不是悄悄修好。**

顺带,这条 fallback **和 `PageTransitionsTheme` 自己的默认表不是一回事**:主题给 Android 的是预测式返回,
而这条更简单的二分给它 zoom;它只在主题那张表里没有这个平台时才跑。回归行把两者摆在一起对照。

---

**`didPush` 和 `didPop` 被覆写,只为了手动把时长塞进 controller**,注释给了理由:`AnimationController` 只
在建的时候读一次 `transitionDuration`,**于是后来变了的主题够不着它。** 上游的 TODO 里写着它自己的删除条
件——**一个把「什么时候可以删掉我」写在旁边的变通做法。**

而 `barrierColor` 和 `barrierLabel` 都是 null:**一条页面路由盖住整个屏幕,后面的遮罩既看不见,又要多花一
层。**

**`MaterialPage` 和 `MaterialPageRoute` 的区别不在长相,在于谁拥有那个栈。** 路由是被 push/pop 命令出来
的;页是一份描述,由 navigator 拿新列表和旧列表比对,自己算出该 push 什么、pop 什么。**这也正是页需要
`key` 而路由不需要的原因:navigator 得能说「这是刚才那一页,换了个位置」,而不是「走了一页又来了一页」。**

验证:`cargo test --lib` 3283 绿,GN `rustflutter_unittests` 3283 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1787 accounted / 101 MISSING(94.7%)。

### 关掉那条线,轮廓还在(2026-08-20)

新模块 `input_decorator.rs`,收掉 `InputDecorator`、`InputDecoration`(`input_decorator.dart`)与
`ShapedInputBorder`(`input_border.dart`)。覆盖率 1784/1888(94.5%)。测试 3271。

**`ShapedInputBorder` 的文档里有一句最值得留:边框的 `borderSide` 设成 `none` 时不画线,「However, it
will still define a shape (which you can see if `InputDecoration.filled` is true)」。**

**一条边框是两样东西:一条线和一个轮廓。把线关掉,轮廓还在。** 填充色被裁到那个轮廓上,所以你看得见它。

**而上游顺手警告的那个坑,是同一件事的另一面:** 这种情况下浮动标签应该设成 `never`,否则「the label will
extend beyond the container as if the border were still being drawn」——**标签的缺口是从形状上切的,不是从
线上切的,于是没有线可切的时候它照样切。** 回归行把这两条分别钉住了。

还有一条相关的:**调用方明确写了 `BorderSide.none` 时,decorator 不会拿主题里的那份去替换它。** 一个被去
掉的边,是一个决定,不是一处缺席。

---

**`FloatingLabelBehavior.auto` 的定义是「聚焦时**或者有内容时**浮起」,而后半句才是要紧的那半:一个已经
填了字的字段必须把标签浮上去,否则标签会压在读者刚打的字上面。**

---

**helper 和 error 共用字段下面那一行,而 error 赢。** 上游写得直白:「the helper text is displayed in the
same location as errorText. If a non-null errorText value is specified then the helper text is not
shown.」

**这是对的:两者都是「字段底下那一行」,而一个字段没法同时在解释自己和在抱怨。抱怨更急。**

其余两条:

* **三处「only one of X and XText」**(helper、prefix、suffix):**widget 形式和字符串形式是二选一**——只有
  一个槽,而同时给两个没有任何规则说该用哪个。
* **`InputDecoration.collapsed` 同时设了 `isCollapsed` 和零内容内边距。** 「collapsed」是**两个设置的一
  捆,不是一个开关**;只有标志、却还留着原来内边距的字段,仍然占着它本想省下的地方。

验证:`cargo test --lib` 3271 绿,GN `rustflutter_unittests` 3271 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1784 accounted / 104 MISSING(94.5%)。

### 一条在这个浮点宽度上打不着的守卫(2026-08-20)

新模块 `carousel.rs`,收掉 `CarouselView`、`CarouselScrollPhysics`、`CarouselController`。覆盖率
1781/1888(94.3%)。测试 3259。

轮播的吸附物理基本就是第 40 轮那个 `PageScrollPhysics`,而**多出来的正是这一轮最值得记的东西——以及我在
它上面绕的一圈。**

上游在算「现在停在第几项」时多了一行:

```dart
if ((actual - round).abs() < precisionErrorTolerance) { item = round; }
```

**「离项边界只差一根头发,就算在上面。」** 没有它,`2.9999999` 配一次向前的轻扫会变成 `3.4999999`、进位到
3;而 `3.0000001` 会变成 `3.5000001`、进位到 **4**。**一像素的累计误差会跳过一整项,而往哪边跳取决于谁也
看不见的算术。**

**我先给它写了一条回归行:取 `300.0 - 1e-9`、`300.0`、`300.0 + 1e-9` 三个位置,断言结果相同。它绿了——然
后我去检查它凭什么绿。**

`f32` 在 300 附近的间距大约是 `3e-5`,**于是那三个字面量在 f32 里根本是同一个数**。那条断言什么都没证明。

我先改成内部用 `f64` 算,想让守卫有意义;再一想,输入本身是 `f32`,精度在函数看见它之前就已经丢了,守卫
照样打不着。**而真正的结论比两次修补都干净:**

**上游的 `precisionErrorTolerance` 是 `1e-10`,对一个 double 在 3 附近的间距(约 `4e-16`)有意义,对一个
single 的(约 `2e-7`)没有意义。本 crate 的滚动偏移是 `f32`——一个和精确边界不同的偏移,至少差出一千倍的
容差;而一个不差的偏移,本来就是精确的。这条守卫在这里打不着。**

于是我把这条分歧**写出来**,而不是绕着它测:代码里保留那一行(读者拿两份文件对照时应该找得到它),文档
里说明它在这个宽度上是惰性的,回归行则**证明「打不着」这件事本身**——同第 74 轮那条不可达守卫一样的做
法。**为一条自己不成立的断言编一个场景,比没有断言更糟。**

---

其余几条:

* **`item_fraction` 在不等宽的轮播里用的是「第一个权重除以权重和」**——**「一项」的意思是「领头那一项有多
  宽」**,于是一个大项带一串小项的轮播,按大项吸附。
* **每个 flex 权重都必须为正:** 宽度为零的项不是一项。而给了 extent 又给权重、或者两个都不给,都没有答
  案。
* **`CarouselController.leadingItem` 有两条各自带话的断言**,而第二条有意思:**同一个 controller 挂了两个
  轮播时,「领头那一项」没有答案——于是它拒绝,而不是挑一个。**

验证:`cargo test --lib` 3259 绿,GN `rustflutter_unittests` 3259 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1781 accounted / 107 MISSING(94.3%)。

### 它开始时的那个状态,得是它自己能走到的状态(2026-08-20)

两个新模块:`segmented_button.rs`(`ButtonSegment`、`SegmentedButton`、`SegmentedButtonState`)与
`selection_area.rs`(`SelectionArea`、`SelectionAreaState`、`SelectableText`)。覆盖率
1778/1888(94.2%)。测试 3248。

**`SegmentedButton` 的三条构造断言,合起来说的是同一件事:它开始时的那个状态,得是它自己能走到的状态。**
一个「不允许为空却从空开始」、或者「不允许多选却选了两个」的按钮,处在一个它自己的按下逻辑永远产生不出来
的位置上——**而下面每一条规则,就会变成在推理一件不可能的事。**

**`_handleOnPressed` 里四个子句每一个都在挣自己的位置,而最要紧的是 `validChange`:按下那个「唯一被选中
的」段,什么都不会发生,除非这个按钮允许空选。** **单选模式下没法靠再按一次取消选择**——和第 56 轮单选组
的空格键、第 61 轮 toggleable 的点击循环是同一个判断:**一个能被误触清空的控件,比一个不能的更糟。**

而 `toggle` 那一行说的是:**单选确实会切换,而且只在一种情况下**——最后一个被选中的段,并且允许清空时。

---

**还有一处值得单独说,而它是一个「不写」的决定。**

上游最后有一句 `if (!setEquals(updatedSelection, widget.selected))` 才发回调。我本想为它写一条回归行,然
后发现:**顺着前面的子句走,这个分支到不了。** 不切换时,按下把选中集换成正好那一个段,而它只可能在「按
下的就是唯一选中的那个」时和原来相等——但那种情况 `validChange` 已经拦掉了(除非允许清空,而那时 `toggle`
又是 true)。切换时,加一个或减一个总是变。

**所以我没有为它编一个场景,而是写了一条穷举所有合法配置、断言「Unchanged 到不了」的回归行**,并在注释
里把推理写下来。**那个守卫防的是上面几条子句将来被改动,不是防某个输入;如实移植就意味着把一个到不了的
分支也移过来。**

---

**`SelectionArea` 和 `SelectableText` 都薄得刻意:机器在 `SelectableRegion` 和 `EditableText` 里,这两个
加的是「平台对三个问题的自己的答案」**——手柄长什么样、右键菜单给什么、有没有放大镜。

**而放大镜那条默认值的第三种情况最有意思:iOS 给 Cupertino 的,Android 给 Material 的,其余平台什么都不
建。** 放大镜存在,是因为**指尖盖住了它正在选的那段文字**;而鼠标指针什么都不盖,**于是在桌面上它不是「缺
了」,是「不适用」。**

`SelectableText` 则是「一个把编辑关掉的 `EditableText`」——选择、手柄、工具条、放大镜本来就是为编辑造的。
而 `showCursor` 默认关,上游在字段文档里写了代价:**它在移动端是长按、在别处是双击,会和周围的手势抢。**

验证:`cargo test --lib` 3248 绿,GN `rustflutter_unittests` 3248 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1778 accounted / 110 MISSING(94.2%)。

### 附着在别的东西上的那个,得在那东西移动时走开(2026-08-20)

新模块 `search_anchor.rs`,收掉 `search_anchor.dart` 的 `SearchAnchor`、`SearchController`、
`SearchBar`,以及 `search.dart` 的 `SearchDelegate`。覆盖率 1772/1888(93.9%)。测试 3229。

**搜索视图是一条路由。** `_openView` 推一条 `_SearchViewRoute`,`_closeView` pop 掉 navigator。**于是系统
返回手势关掉搜索,不需要任何人去写这件事——因为「搜索」本来就是一个你去到的地方。**

**而上游文档里那条不对称,理由值得留:**

> The search view route will be popped if the window size is changed and the search view route is not
> in full-screen mode. However, if the search view route is in full-screen mode, changing the window
> size, such as rotating a mobile device, will not close the search view.

**附着在别的东西上的那个,得在那东西移动时走开;没有附着的那个,不用。** 一个 docked 的视图是相对一个锚点
widget 摆的,而那个 widget 在屏幕上的位置刚刚变了,它已经没有地方可待;**一个全屏视图从来就没锚在什么上
面,转屏只是把它重新布局一遍。**

**`SearchController` 继承自 `TextEditingController`**——一个对象同时握着查询文本和视图的开合状态。它们为
什么该在一起,在 `closeView` 里看得见:**先设文本,再 pop**,顺序如此,于是视图淡出时锚点上的那个条已经
读的是被选中的文本了。**两个对象会给你一帧的旧查询。**

而 `_detach` 上的守卫是熟悉的那种形状:**一个已经交给新锚点的 controller,不该被旧锚点的销毁摘掉。**

---

**`SearchDelegate` 是更老的那套,而它的全部就是两页:打字时是建议,提交后是结果。** 有意思的是**两页之间
的移动是显式的**——上游对 `buildSuggestions` 的指引写着:点中一条建议时,应该把 `query` 设成它,**然后**
调 `showResults`。

**选一条建议不是一次搜索;那是把框填好,然后再搜索。** 回归行把这两步分开钉住了:只改 `query` 不会翻页。

还有一条小的:**锚点的 builder 如果返回的是一个 Icon 或者别的点不动的东西,调用方就不必显式调
`openView`**——锚点会替它接上点击。

验证:`cargo test --lib` 3229 绿,GN `rustflutter_unittests` 3229 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1772 accounted / 116 MISSING(93.9%)。

### 一个写成乘法的时长(2026-08-20)

新模块 `progress_indicator.rs`,收掉 `progress_indicator.dart` 四个类(`ProgressIndicator`、
`LinearProgressIndicator`、`CircularProgressIndicator`、`RefreshProgressIndicator`)与
`refresh_indicator.dart` 两个类(`RefreshIndicator`、`RefreshIndicatorState`)。覆盖率
1768/1888(93.6%)。测试 3217。

**圆形不定态的周期是 `1333 * 2222`——一个写成乘法的常数,大约四十九分钟。**

那当然不是给人看的一段时长。**它是一个公倍数:旋转和描边扫过的周期不一样,而这个数让它们只有在很久之后
才重新对上相位——于是这个动画看上去永远不重复。** 而这个数和线性那个 1800ms 一样,是从 Android 自己的
`progress_indeterminate_material.xml` 里抠出来的,**注释里给了源码 URL。两个常数都是引文,而且注明了出
处。**

其余几条:

* **`value` 和 `controller` 不能同时给**,而报错信息说清了矛盾在哪:`value` 是给知道进度的确定态用的,
  `controller` 是用来驱动不定态动画的。**同时给,是一次要了两种指示器。**
* **`_kTrackGapRampDownThreshold = 0.01`:** 进度低于百分之一时,轨道上那道缝按比例缩掉。**进度为零时根本
  没有条,那道「条和轨道之间的缝」就成了一个悬在空处的缺口**;把它斜坡降下去,比给空状态写一个特例便宜。
* **刷新那个圆圈比普通的画得粗:** 它浮在内容上方的一张卡片上,得在任何背景上都读得出来。

---

**拉动刷新那边,要拖的距离是「容器的四分之一」,不是一个固定像素数。** 于是一个长列表和一个短列表,要的
都是「你能看见的那部分的四分之一」,长的那个不会显得又硬又远。回归行拿 400 和 800 两个视口各跑一遍。

**而 armed 之后有一行只在 armed 时跑:`newValue = max(newValue, 1 / _kDragSizeFactorLimit)`。**
**一旦进入 armed,指示器就不会再缩回它的静止尺寸以下**,不管读者往回拖多远。**靠往回拖来「解除武装」,会
让这个控件恰好在读者正在做决定的那一刻变得神经质。**

**六个状态里,最后两个值得点名:`done` 和 `canceled` 都是淡出,区别只在于「为什么」。** 它们被分开,一是
因为长得不一样(刷新完成的会缩掉,放弃的只是收回去),二是因为**「成了」和「你改主意了」不是同一句话。**

还有一条守卫:通知必须 `depth == 0` 且 `leading`。**里面嵌的列表越界不是一次下拉刷新,而滚到底也不是。**

验证:`cargo test --lib` 3217 绿,GN `rustflutter_unittests` 3217 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1768 accounted / 120 MISSING(93.6%)。

### 快照是一张照片,里面动着的东西就不动了(2026-08-20)

新模块 `page_transitions_theme.rs`,补齐页面转场这一族:`FadeForwardsPageTransitionsBuilder`、
`ZoomPageTransitionsBuilder`、`PageTransitionsTheme`(`page_transitions_theme.dart`),以及
`PredictiveBackPageTransitionsBuilder`、`PredictiveBackFullscreenPageTransitionsBuilder`。**第 55 轮从抽
象基类和 Android 两个老答案开的头,这一轮把它收完。** 覆盖率 1762/1888(93.3%)。测试 3203。

**`FadeForwards` 的时长带着这个移植至今找到的第三处「我们是肉眼调的」,而且是最直白的一处:**

> Eyeballed on a physical Pixel 9 running Android 16. This does not match the actual value used by
> native Android, which is 800ms, because native Android is using Material 3 Expressive springs that
> are not currently supported by Flutter. So for now at least, this is an approximation.

**它说出了自己不是的那个数(800ms)、说出了为什么不能是那个数、还说了这个差距是暂时的。** 对比第 54 轮那
两处越界的常量:那里承认了不解,但说不出正确答案本该是什么。回归行把 450 和 800 都钉住,并断言两者不
等——**这条断言的用处就是,哪天 Flutter 支持了那种弹簧,它会红。**

而 `backgroundColor` 存在的理由很朴素:**一页在淡出、另一页在淡入,中间有一刻两页都不是不透明的**,背后
没有颜色的话,读者看穿过去是空的。

---

**`ZoomPageTransitionsBuilder` 的 `allowSnapshotting`,上游把代价和好处写得一样直白:** 打开时,「进出路
由上正在跑的动画,可能看上去是冻住的——除非它是 hero 动画或者画在另一个 overlay 上的东西」。

**这就是快照:一张照片,而里面动着的东西就不动了。** 交换的是一次光栅化,对上一整棵子树在转场的每一帧重
绘;对多数页面,照片赢。

而 `allowEnterRouteSnapshotting` 是**单独一个开关**,理由说得通:**一个读者正要去操作的页面被冻住,比一
个他正在离开的页面被冻住更糟。**

---

**`PageTransitionsTheme` 默认表里有一件事值得看:它根本没有 Fuchsia 这一项**,而查表时是**回退**而不是报
错——回退到 zoom。**一个新平台、或者一个应用故意没配的平台,拿到的是一个合理的东西而不是什么都没有;而
zoom 是那个中立的选择,它不属于任何一个平台。**

**而 `operator ==` 那里的 `_all` 有一句解释自己的注释**:把 builders 映射成「每个平台一个」的列表再比。
**直接比两个 map 比的是它们的键集;真正要紧的是两个主题对每个平台的回答是否一样。** 一个列了四个平台的
主题和一个列了六个的,完全可能是同一个主题——回归行正是这么构造的。

---

**预测式返回那两个类,唯一值得读的决定是:它只在真的有手势时才跑。** 按钮按下或者程序调用 pop,退回到
`FadeForwards`。

**而这不是保守,是对的:「预测式」的意思就是这个动画在跟着一次还没结束的拖动;按钮按下时没有东西可
跟。** 照跑不误的话,那是一个没有输入在驱动的形状。

**而全屏那个和它的区别只有一处:回退到的是 zoom 而不是 fade-forwards。** 一个没有手势、直接来的全屏路
由,正是 zoom 本来就是为它写的那种情况。

验证:`cargo test --lib` 3203 绿,GN `rustflutter_unittests` 3203 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1762 accounted / 126 MISSING(93.3%)。

### 被选中的那一项落在按钮上(2026-08-20)

新模块 `dropdown.rs`,一次收掉三个文件七个类:`DropdownMenuItem`、
`DropdownButtonHideUnderline`、`DropdownButton`、`DropdownButtonFormField`(`dropdown.dart`),
`DropdownMenuEntry`、`DropdownMenu`(`dropdown_menu.dart`),`DropdownMenuFormField`。覆盖率
1757/1888(93.1%)。测试 3189。

**`getMenuLimits` 是这个 widget 之所以「像系统自带控件」的全部原因:菜单被摆成让当前选中的那一项正好落在
按钮上。** 按下「Medium」,「Medium」就在你手指底下,于是再选一次是一次小移动,不是一次寻找。回归行按
「选中项的顶边 == 按钮的顶边」直接钉住了这一条,并且验证换一个选中项时**移动的是整个菜单,不是那一项**。

那段算术后面跟着**三次修正,而且有顺序**:太高就贴上限、太低就贴下限,**而第三次是在补前两次造成的伤**
——如果贴边把选中项的中心推到了按钮中心上面,就再拉回来。

**边距是一个会给按钮让路的偏好:** `topLimit = min(_kMenuItemHeight, buttonTop)`。通常留一个菜单项高度
的边,但一个离屏幕顶只有十像素的按钮,得到的是离顶十像素的菜单。

**而菜单永远不会占满屏幕**——上限是视口高度减去**两个**菜单项高度,上游引了 Material 规范说明理由:「This
ensures a tappable area outside of the simple menu with which to dismiss the menu.」**一个铺满屏幕的菜
单,没有地方可以点出去。**

**装不下时改用滚动把选中项对上去,而上游对这条老实地写了两个限制:只在菜单第一次显示时做**(之后读者自
己的滚动位置不再被动)、**而且只对定高的菜单项准确**(那是默认值,不是保证)。

还有一句上游的坦白值得留:那个「菜单在屏幕内」的断言**只在按钮完整在屏幕上时才检查**,注释写着「If the
button was a bit off-screen, then, oh well.」——**一个只在能讲清楚的情况下成立的不变式,被说出来了,而不
是被默默假设。**

---

**`DropdownButtonHideUnderline` 是一个完全没有数据的 InheritedWidget:它的存在本身就是消息。**
`at(context)` 是对查找结果的一次判空,而 `updateShouldNotify` 返回 false——**因为没有任何东西可能变过**:
出现和消失是树形状的变化,框架本来就管。

---

**而 `DropdownButton` 和 `DropdownMenu` 之间真正的区别,不是长相。** 上游自己的迁移说明先说视觉「差一点
点」,然后给出那条要紧的:**`DropdownButton` 让应用持有当前值,`DropdownMenu` 自己持有。** 一个是受控
的,一个不是,其余都是装饰。回归行把这一对摆在一起:受控那个的 `value` 得由应用给;不受控那个 `select`
之后就自己变了,而 `initial_selection`(它**从哪儿开始**)还是原来那个。

**由此还带出一条:`DropdownMenuItem` 带的是一个 widget,`DropdownMenuEntry` 带的是一个 label。** 这正是
`DropdownMenu` 能在读者打字时过滤的原因——**你没法搜索一个 widget。**

**而过滤和搜索是两件事:过滤用 `contains` 会删掉不匹配的,搜索用 `starts_with` 只是指过去。** 我第一版回
归行写了「只有 Medium 含 m」,红了——**"Small" 中间也有一个 m。** 改完之后这条正好成了这两者区别的最好例
子:`filtered("m")` 是两个,`search("m")` 只指向 Medium。

验证:`cargo test --lib` 3189 绿,GN `rustflutter_unittests` 3189 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1757 accounted / 131 MISSING(93.1%)。

### 图标跟主题走,标签跟操作系统走(2026-08-20)

转入 material 层。新模块 `action_buttons.rs`,一次收掉 `material/action_buttons.dart` 全部八个类:
`BackButton`/`BackButtonIcon`、`CloseButton`/`CloseButtonIcon`、`DrawerButton`/`DrawerButtonIcon`、
`EndDrawerButton`/`EndDrawerButtonIcon`。覆盖率 1750/1888(92.7%)。测试 3171。

**这个文件值得读的是一处反转和一处区分。**

**反转:`onPressed` 为 null 不会禁用这些按钮。** 在一个普通的 `IconButton` 上,null 回调会把它变灰;而
这里的基类把处理器包了一层,null 会落到那件显而易见的事上去。**一个没有回调的返回按钮,仍然是一个返回
按钮**——调用方什么都不给,说的不是「什么也别做」,而是「做返回按钮该做的事」。

**区分:图标按 `Theme.of(context).platform` 选,而语义标签按 `defaultTargetPlatform` 选。** 上游把理由
写在注释里:「This can't use the platform from Theme because it is the Android OS that expects the
duplicated tooltip and label.」

**一次主题覆写改变的是应用长什么样;它不该改变操作系统的无障碍服务被告知了什么。** 一个打扮成 iOS 的应
用跑在 Android 上,读它的仍然是 TalkBack。回归行专门构造了这一对会「不一致」的情形:iOS 的尖角箭头,加
Android 的标签。

其余几条:

* **`BackButton` 和 `CloseButton` 做的是完全同一件事**(`Navigator.maybePop`),只在图标和 tooltip 上不
  同——**同一个动作,被给了两个意思**:「回到你刚才在的地方」和「把这个关掉」。而 **tooltip 是唯一把它们
  分开的东西。**
* **是 `maybePop` 不是 `pop`:** 一条拒绝返回的路由(比如第 62 轮那个 `canPop: false` 的 `PopScope`)会被
  尊重,而不是被压过去。
* **Web 上永远是那支朴素的箭头**,不看平台。**Mac 上的一个网页仍然是一个网页**,而浏览器自己有一个返回
  的东西,不该跟它撞脸。
* **图标被拆成独立的 widget,理由只有一个:`ActionIconTheme` 可以一次把四个全换掉。**

---

**一处工具上的发现,值得记进规矩:尺子不展开宏。**

我先用一个 `macro_rules!` 把这八个类生成了出来——**代码能编译、测试能过,而覆盖率纹丝不动,还是
MISSING:8。** 之前记过尺子「不展开宏体」,这一轮是第一次真的撞上。

改成把八个显式写出来之后立刻变成 covered:8。**而这样其实更好:八个能被搜到的名字,比一个宏加四行调用更
容易读**——这也正是上游把它们写成八个类而不是一个带参数的类的理由:**调用点上的那个名字,就是重点。**

验证:`cargo test --lib` 3171 绿,GN `rustflutter_unittests` 3171 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1750 accounted / 138 MISSING(92.7%)。

### widgets 层只剩一个文件了(2026-08-20)

一次收掉 widgets 层最后五个非引擎类:`Placeholder`、`RawKeyboardListener`、
`DefaultTextHeightBehavior`(都进 `small_widgets.rs`)、`AutofillGroupState`(新模块 `autofill.rs`)、
`TextSelectionToolbarLayoutDelegate`(进 `text_selection.rs`)。覆盖率 1742/1888(92.3%)。测试 3160。

**至此 `widgets/` 层只剩 `platform_view.dart` 一个文件**(3 个 MISSING,同文件另有 5 个已记为
blocked-engine)。**其余每一个 widgets 文件都已覆盖。**

---

**`TextSelectionToolbarLayoutDelegate` 拿到的是两个锚点,不是一个**——「在选区上方时坐哪」和「在下方时坐
哪」——**由布局阶段来挑。** 因为在工具条被量出来之前,没有人知道它放得下哪一边,而到那时调用方早就走了。

它的 `centerOn` 三个分支,上游都在注释里点了名:左边溢出就贴左、右边溢出就贴右、否则正中。**一条半挂在
屏幕外的工具条,比一条没有正对着选区的工具条更糟**——和 tooltip 的 `positionDependentBox` 是同一个判断。

**而 `fitsAbove` 是一个可选的覆写,上游给的理由很具体:** Material 的工具条在它的溢出菜单展开时会强制这
个值,因为**展开的菜单比收起的高,否则工具条会在读者正用着它的时候翻到另一边去**。**这个覆写存在,是为
了让一个 widget 在动画期间把一个决定摁住。**

**`AutofillGroupState` 里最要紧的一条:只有最外层的那个组会去结束平台的 autofill 上下文。** 那个上下文对
整张表单是一个东西,**而一个嵌套的组被重建时,不该替整张表单决定「到现在为止填的东西值不值得存」。**
`_isTopmostAutofillGroup` 在 `didChangeDependencies` 里重算,于是一个被挪到别人下面的组,**不用谁告诉它
就不再是最外层了。**

另外两条:`autofillClients` **只过滤出启用的那些**——一个禁用的字段仍然属于这张表单,只是不递给平台;而
`unregister` **断言那个 id 在**——注销一个从没注册过的东西,意味着注册/注销已经失步,**悄悄什么都不做会
把这件事藏起来。**

---

其余三个,各是一条被写下来的判断:

* **`Placeholder` 在无约束的盒子里退回 400×400 而不是报错。** 失败在这里恰恰是反效果:**这个 widget 的用
  途就是替一个还没写的东西站着**,它在还没写的地方崩掉就毫无意义。而那个「打叉的框」是一次
  `Path`——矩形加两条开放折线——所以整个记号是**一次描边**。
* **`RawKeyboardListener` 是上游已经废弃的那个,而废弃本身才是要点:** 它听的是**原始**按键事件,也就是平
  台自己的编码原样传过来。`KeyboardListener` 取代了它,因为**同一个物理按键在不同平台上的编码不一样**,
  于是照着它们写的东西,是照着一个操作系统写的。
* **`DefaultTextHeightBehavior` 是 `InheritedTheme` 而不是普通的 InheritedWidget,而这一处区别只在一个地
  方要紧:** 一个被推上来的路由是在树里完全另一个地方构建的,**而只有 theme 会被捕获并带过那道缝。** 没
  有它,一个对话框里的文字会悄悄地和打开它的那个页面不一致。

验证:`cargo test --lib` 3160 绿,GN `rustflutter_unittests` 3160 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1742 accounted / 146 MISSING(92.3%)。

### 一个不创建渲染对象的渲染对象 widget(2026-08-20)

新模块 `adapter.rs`(`RenderObjectToWidgetAdapter`、`RenderObjectToWidgetElement`),以及补进
`sliver_fill.rs` 的 `SliverPrototypeExtentList`。覆盖率 1737/1888(92.0%)。测试 3138。

**`adapter.dart` 是那道嫁接口。** 框架里别处都是 widget 描述一个渲染对象、由框架去造;**而这里渲染对象
早就在了**——它就是引擎给的那个视图——需要做的是把一棵 widget 树接到它下面。`runApp` 做的就是这件事,而
这个倒转就是整个设计。

于是 `createRenderObject` **返回它收到的那个 container**,一个也不造。**一个不创建渲染对象的渲染对象
widget 读起来像自相矛盾,直到你看见它是干什么的:渲染树先存在,而这个 widget 是框架同意假装是自己造
的。** `updateRenderObject` 的函数体是空的——container 从来就不是被这个 widget 配置的,没有什么可更新。

**而它的 key 是 `GlobalObjectKey(container)`——container 自己就是身份。** 这正是**第二次调用 `runApp` 会
「换掉」应用而不是「重启」它**的原因:同一个视图上的两个 adapter 是同一个 widget,于是下面那棵 element
树是被协调,不是被扔掉。**热重载就活在这一行上。**

`attachToRenderTree` 的两条路也是两种语气:**第一次是命令式的**——锁住状态、创建 element、指派 owner、
在 build scope 里 mount,当场做完,因为没有一帧正在进行可以推迟给它;**之后是声明式的**——把新 widget
**存起来**,标记需要重建。

其余几条:

* **`_rebuild` 的 `catch` 里,根部的构建失败不会把应用带走,而是把错误 widget 立起来。** 而注意它传的参
  数是 `updateChild(**null**, error, slot)`——**失败的那棵子树是被丢掉的,不是拿去和错误 widget 协调**:
  跟一棵在构建时抛了异常的树做协调,等于把同一个问题再问一遍。
* **`moveRenderObjectChild` 的函数体是 `assert(false)`。** 不是「不支持」,是**不可能**:只有一个槽,没有
  地方可移。走到这里是框架的 bug,不是调用方在做什么出格的事。
* **`performRebuild` 里 `_newWidget` 可能为 null**,上游把场景写在注释里:**一次 reassemble——热重载——会
  在不递新 widget 的情况下重建根。**
* **`mount` 断言 parent 为 null**:这个 element 只可能是根。

---

**`SliverPrototypeExtentList` 是「范围从哪来」的第三个答案。** `SliverFillViewport` 从视口取,
`SliverFillRemaining` 从剩下的地方取,**而这一个从一个你交给它、它从不给你看的 widget 那里取。**

那个原型是渲染对象的孩子,却**不是列表的孩子**:它待在一个自己的槽里、在下标空间之外,**每一趟都被布局,
一趟都不被绘制**。于是范围是由一个真 widget 的真布局量出来的,而完全没有把它放上屏幕的代价。

**`performLayout` 那两行的顺序是有理由的:先布局原型,然后才跑下面那个定长列表的布局**——因为正是那趟布
局在问 `itemExtent`,而在原型有尺寸之前,这个答案根本不存在。而 `itemExtent` 会断言原型已经被布局过:
**提前读是一个错误,不是一个缺席**;这里返回 `None` 而不是一个 0——**一个 0 会安静地让每个孩子都没有高
度。**

**而这正是上一轮 `SliverEnsureSemantics` 点名要的那几个 sliver 之一**,理由也是那一条:它的滚动范围在孩
子被构建之前就是已知的,于是按那个范围导航的辅助技术,会到达它本来要去的地方。

**另外,这两个类在同一处地方形状一样:`RenderObjectToWidgetElement` 和这个原型槽,都只有一个槽、
`moveRenderObjectChild` 都是 `assert(false)`——「只有一个,没地方去」。**

**一处工具上的自查:** 注册模块时用 `sed ... || sed ...` 想做回退,但 **`sed -i` 在没有匹配时也返回
0**,于是回退分支从不执行,模块没被写进 `lib.rs`。是紧接着的 `grep` 打印为空才发现的。**在 shell 里拿退
出码当「有没有改到」用,是错的;要么验证结果,要么别写回退。**

验证:`cargo test --lib` 3138 绿,GN `rustflutter_unittests` 3138 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1737 accounted / 151 MISSING(92.0%)。

### 一个语义动作没有位置(2026-08-20)

新模块 `semantics_debugger.rs`(`SemanticsDebugger`),以及补进 `semantics_markers.rs` 的
`SemanticsGestureDelegate`(`gesture_detector.dart`)与 `SliverEnsureSemantics`(`sliver.dart`)。三个类,
一个主题:**语义树被告知了什么,以及你怎么看见它。** 覆盖率 1734/1888(91.8%)。测试 3124。
**`widgets/` 层只剩 8 个类。**

**`SemanticsGestureDelegate` 的全部难处在一句话上:一个语义动作没有位置。** 读屏器说的是「激活这个」,不
说「在哪儿」。于是这个委托**编一个出来——widget 的中心**,再从那儿把整个手势合成出来。

**而且不是只合成最后一个回调。** 一次语义点击变成 `onTapDown`、`onTapUp`、`onTap` 三个,按顺序发。因为
识别器的每个回调都可能有意义:**一个「按下时高亮、抬起时触发」的按钮,否则对读屏器用户永远不会高亮。**
指针种类填 `unknown`,那是诚实的答案。

**一次语义拖动更甚:down、start、update、end——一整个手势在一次调用里合成完。** 读屏器只给一个 delta,
开头和结尾都得围着它编出来。而**结束速度是 0**:读屏器的划动没有速度,**编一个出来会把列表甩飞。**

**而委托只为「确实存在的识别器」装处理器:** 没有 `TapGestureRecognizer`,就不宣告语义 tap。**语义树对外
提供的,恰好是这个 widget 真的能做的那些。** 而 pan 会同时答应两条轴(pan 本来就管两个方向),并且当 pan
和轴向识别器同时在场时,上游两个都调,不做取舍。

上游还在默认委托上写了一句不常见的话:**「For readers who come here to learn how to write custom
semantics delegates: this is not a proper sample code.」**——它伸手进了检测器的私有状态,而那是为了保住比
这个接口更早的既有行为。真正的委托应该把回调存成属性。

---

**`SemanticsDebugger` 之所以有用而不只是装饰,是因为它接管手势之后派发的是语义动作,不是触摸。** 一个模
拟点击的调试器只会告诉你「这个应用能用」;**这一个对语义树做命中测试并执行找到的那个动作——正是读屏器做
的事**,于是你看见的是语义到底行不行。

它的 `_handlePanEnd` 有两处值得写下来:

* **正好对角的一甩什么都不做。** 上游在两个速度分量绝对值相等时直接 return,而不是挑一个:**没有正确答
  案,而猜会让调试器的行为取决于浮点噪声。**
* **横向甩发两个动作,纵向甩发一个。** 向左在滑块上是 `decrease`,在列表上是 `scrollLeft`,**而调试器根本
  不知道手指下面那个节点是哪种——于是两个都发,让节点挑自己有的那个。** 纵向没有 increase/decrease 的约
  定,所以只发 scroll。

---

**`SliverEnsureSemantics` 是两行:一个代理 sliver,渲染对象把 `ensureSemantics` 覆写成 true**,让孩子即
使滚出了视口**并且**滚出了缓存区,也留在语义树里。读屏器于是够得到一个谁都看不见的头部。

**而它的文档里带着一句其实是坦白的警告,那才是有意思的地方:这东西只对「事先知道自己范围」的 sliver 真的
管用。** 一个懒惰的 `SliverList` 会低估滚动范围,而**按那个范围导航的辅助技术,会滚不到这个 widget 刚刚
让它够得到的那份内容。** 上游的建议是改用 `SliverFixedExtentList`、`SliverVariedExtentList` 或
`SliverPrototypeExtentList`。

**让一样东西「够得到」,和让它「找得到」,不是一回事。**

验证:`cargo test --lib` 3124 绿,GN `rustflutter_unittests` 3124 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1734 accounted / 154 MISSING(91.8%)。

### 屏幕录制没法只录一半(2026-08-20)

两个新模块 `sensitive_content.rs`(`SensitiveContentHost`、`SensitiveContent`)与
`scroll_aware_image_provider.rs`(`ScrollAwareImageProvider`)。主题是**「知道什么时候不做」**:别把这块
内容录进屏幕录像;别在列表飞速滑过时去解码这张图。覆盖率 1731/1888(91.7%)。测试 3111。

**`_ContentSensitivitySetting` 的聚合规则是一个严格优先级,不是投票:** 树里任何一个 `sensitive` 的
widget,就让整个窗口 sensitive,不管屏幕上还有什么别的。

**而这是这个问题唯一可能正确的规则,因为屏幕录制对一个窗口是全有或全无的——没有办法只录一半。** 于是任
何一个说「这块不行」的 widget,必须压过每一个说「这块没事」的。回归行拿二十个 `notSensitive` 加一个
`sensitive`,确认它以一敌二十一。

**计数存在,是因为 widget 会来会走;而答案是一个最大值,永远不是一个和。**

其余几条:

* **「没人在问」和「所有人都说不用」是两回事:** 计数全为零时返回 `null`,而 `null` 会让 host 把平台**放
  回它原来的样子**——那个 fallback 是**第一个 widget 注册时**抓下来的,是嵌入层或开发者本来设的值(在
  Android API 35 上,没人说过话的话就是 auto-sensitive)。**是恢复,不是重置成一个默认值。** 而它只抓一
  次,后来的注册不会覆盖它。
* **平台支不支持这件事,只问一次并记住;而问的时候抛了 `PlatformException`,记成「不支持」**并报错。**没
  能查清楚平台能不能做某件事,不是假定它能做的理由**——往那个方向猜错,代价是读者以为藏起来了的内容出现
  在录像里。
* **等级没变就什么都不说。** 每次 widget build 都发一条 channel 消息,就是每帧一条。
* **计数变成负数时,上游报的是一个带「Please file an issue」的错误而不是断言**——那是框架自己把注册和注销
  弄丢了步,不是调用方的错。

**还有一件事值得记下来:上游根本没有导出这个文件。** 类是 `@visibleForTesting`,上面挂着一条 TODO:
「This is not ready for production」,以及一个「内容在 media projection 期间仍会泄露」的 issue。**这一
版是把已经想清楚的那部分安排移过来,平台那一侧还没有。** 文件头写明了这一点。

---

**`ScrollAwareImageProvider` 整个类是四个检查,而顺序就是设计。**

**缓存检查排在滚动检查之前,也排在「context 还在不在」之前,而上游把两个理由都写了:**

* 告诉被包住的 provider「这张图已经在缓存里」,会更新缓存的 LRU 信息——**「Even though we never showed
  the image, it was still touched more recently.」**
* 而排在滚动检查之前,是因为**字节已经在那儿了,就不管列表飞得多快都直接画出来**:纹理内存没有额外要分配
  的,**等下去省不到任何东西。**

**这正是这个类的要点:推迟的是「工作」,不是「显示」。**

其余三步:

* **context 离开了树,就整个结束,而且流永远不会被标记完成**——监听者不会被通知,因为已经没有人在等了。
* **滚得太快,不是放弃也不是照做,而是排到下一帧帧末重来。**
* **而「重来」是从第一个检查重来,不是从滚动检查接着走。** 因为下一帧时这张图可能已经从别处进了缓存,或
  者 context 已经离开了树——**从中间接着走会把这两种情况都漏掉。** 回归行把这两条分别构造了一遍。

验证:`cargo test --lib` 3111 绿,GN `rustflutter_unittests` 3111 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1731 accounted / 157 MISSING(91.7%)。

### 竖直在外、水平在内,而这个顺序不是随手定的(2026-08-20)

新模块 `scrollable.rs`,补上 `scrollable.dart` 的三个状态类:`ScrollableState`、
`TwoDimensionalScrollable`、`TwoDimensionalScrollableState`。**这兑现了第 60 轮欠下的那句话**——当时说
`Scrollable` 已经在账本里映射到 `scrolling::Scroll`,那一族得先读清楚再动。覆盖率 1728/1888(91.5%)。
测试 3091。

**`deltaToScrollOrigin` 的符号值得单独写下来:滚动偏移永远沿轴向增长,但屏幕偏移只在 `down` 和
`right` 上和它一致。** 一个向上生长的列表,把内容在 y 上搬的是**负的**同样多的像素。四个方向都钉住了。

**`ensureVisible` 不在最近的那个滚动视图停下。** 它一路往外走,穿过它上面的每一个——**一个列表里的行,列
表在页面里,三层都得动,才有人看得见它。**

而其中的微妙处是 `targetRenderObject`:**它只被记录一次,然后在整趟走里保持不变。** 上游把理由连同 issue
链接写在注释里:**最内层的目标被尽可能地露出来,只有当它已经露出来了,外层才轮到去最大化「内层那个滚动
视图」的可见度。** 没有这条,每一层都会重新瞄准一个更大的东西,而调用方真正问的那个可能反而跑到屏幕外
去。

**这里我自己先写错了一次:** 第一版把「这一步瞄的是不是原目标」写成了 `index == 0 || true`——一个两臂都为
真的重言式,和之前那次 tooltip 的错法一模一样。**在写成断言之前它看着像在表达一条规则。** 改成如实的两个
字段(最内层瞄调用方给的对象、且此时还没有记录下任何目标;之后每一层瞄下面那个滚动视图、并带着已记录的
目标),回归行才真的分得开这两种情况。

---

**`TwoDimensionalScrollableState` 就是两个普通的 `Scrollable`,一个套在另一个的视口里——而顺序不是随手
定的:竖直在外,水平在内。** 上游连键的名字都这么写:`_verticalOuterScrollableKey` 和
`_horizontalInnerScrollableKey`。**嵌套顺序决定了哪条轴先看见手势。**

**而两个各自被交给了对方的键,上游给这行加的注释是「for gesture forwarding」。** 一次斜着来的拖动到了其
中一个手里,得能去动另一个;**一个只能动自己的滚动视图,会让对角拖动根本不可能。**

**回退 controller 的那套账在两个方向上都做了:** 调用方给了 controller,就没有自己要拥有的;没给,就必须
自己造一个。`didUpdateWidget` 两边都处理——**而上游对两半都写了断言,不是只断言那个会崩的一半。** 回归行
把「后来给了 controller」和「后来把 controller 收走了」两种转换分别钉住,`dispose` 也只销毁自己造的那些。

**还有两条小的:**

* **那两个 `assert` 拦的是「细节报错了自己的轴」**——一个指向左边的 `verticalDetails`,否则会造出一个和旁
  边那个悄悄不一致的滚动视图。而**反向的轴仍然是同一条轴**(`up` 还是竖直的),回归行把这一点也分开了。
* **`_TwoDimensionalScrollableScope.updateShouldNotify` 返回 false**,理由写在类上面的注释里:`build`
  每次都会重建这个 scope,**于是依赖它的东西是通过那次重建被重建的,再返回 true 就是把同一件事做两遍。**

**工具上也栽了一下:** 这一轮的测试用 `cat <<'EOF'` 写时被 shell 的引号解析顶回来了(报 unexpected
EOF)。这正是之前记下过的那条规矩——**内容一旦同时穿过 shell、Python 和 Rust 三层引号,就写文件、用编辑工
具**。换成 Write 工具一次过。

验证:`cargo test --lib` 3091 绿,GN `rustflutter_unittests` 3091 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1728 accounted / 160 MISSING(91.5%)。

### 摩擦和弹簧接在一个算出来的瞬间上(2026-08-20)

两个新模块 `scroll_simulation.rs`(`BouncingScrollSimulation`)与
`stretch_effect.rs`(`StretchEffect`),外加 `SliverSafeArea` 补进 `media_query.rs`。这一轮把第 54 轮的
越界指示器那条线接上了:**这里是 iOS 的越界从哪来,和 Android 的拉伸最后怎么画。** 覆盖率
1725/1888(91.4%)。测试 3073。

**`BouncingScrollSimulation` 整个类是两个模拟,粘在一个算出来的瞬间上:** 内容还在范围内时是摩擦,出界之
后是弹簧。**而让它感觉像一次运动而不是两次的,正是那个接缝的处理——弹簧拿到的是摩擦在越过边界那一刻的确
切速度。** 回归行在接缝前后各取一个采样点,确认位置没有跳。

**摩擦常数 0.135 的来历上游写在注释里:** `UIScrollView.decelerationRate` 的 `.normal` 是
**0.998,每毫秒**;而 `0.998^1000 ≈ 0.135`,就是同一个速率**每秒**。**这不是谁调出来的数,是苹果的数换
了个时间单位。** 回归行直接算了这个幂来钉住这句话。

**而 `_springTime` 的两个特殊值替掉了两个分支:** **负无穷**表示弹簧从第一刻就接管(内容本来就在界外),
**正无穷**表示它根本不会跑(这一甩在到边之前就停了)。`x(time)` 里那句 `time > _springTime` 于是同时处理
了三种情况。

**还有一处上限值得单独说:`maxSpringTransferVelocity = 5000`。** 一次快到足以冲出边界的甩动,**并不会把
全部动能交给回弹**——一次按比例的、每秒两万像素的回弹,会把内容甩到屏幕外面再甩回来。**封顶意味着「非常
快」和「只是快」弹回来的幅度是一样的**,而这正是平台的行为和读者的预期。回归行拿 20000 和 60000 两个初速
度各跑一遍,确认峰值一样。

为此给 `FrictionSimulation` 补了 `time_at_x`(上游的 `timeAtX`)——**正是它让接缝落在「越过边界的那一
刻」而不是「下一个帧边界」上,这才是 iOS 的回弹看不出接缝的原因。** 它的两处拒绝都是实的:不动的模拟哪
儿也去不了,而被问到自己身后或停止点之外的位置时,答案是永远到不了。顺手也改掉了那句已经过期的注释
(「iOS-style bouncing, which is not ported yet」)。

---

**`StretchEffect` 有意思的地方是:同一个效果有两套实现,在运行时按引擎能不能跑 shader filter 来选。**

* **能跑**(今天意味着 Impeller):一次真正的**非均匀网格形变**——这才是 Android 做的事,靠近被拉的那条边
  的内容拉伸得多,远的拉伸得少。
* **不能跑**:一个朴素的**均匀缩放**,**锚在对面那条边上**,于是内容朝远离手指的方向长,而不是从手指底下
  滑走。每个像素按同一个比例移动,不是平台的画面,但认得出是同一个手势。

而回退路径里 `filterQuality: stretchStrength == 0 ? null : FilterQuality.medium` 不是微优化:**一个设了
filter quality 的 `Transform`,哪怕矩阵是单位阵也是一次光栅操作**——没有被拉伸的列表会为一次重采样每帧付
钱。

`_getAlignment` 只在横轴上问阅读方向,竖轴不问——**没有哪个语言是从下往上读的**。回归行把这一条也写成了
断言。

---

**`SliverSafeArea` 是 `SafeArea` 的 sliver 双生子,而它那两条规则都不是显然的那条:**

* **`minimum` 是一个下限,不是一个加项。** 每条边取「最小值」和「系统内边距」中**大的那个**,于是
  `minimum: all(16)` 是「至少十六,刘海那边更多」,而不是「十六加刘海」。
* **它吃掉的那几条边会在传下去的 `MediaQuery` 里被清零**,于是安全区套安全区不会缩进两次。**而这正好和上
  一批的 `BoxScrollView` 形成对照:那个按轴把内边距劈成两半,因为列表的孩子需要留下交叉轴那一半;这个不
  需要,整份拿走。**

验证:`cargo test --lib` 3073 绿,GN `rustflutter_unittests` 3073 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1725 accounted / 163 MISSING(91.4%)。

### 否决是一个字段,不是一个回调(2026-08-20)

新模块 `widgets_app.rs`,收掉包住一切的那一圈:`WidgetsApp`(`widgets/app.dart`)、
`CheckedModeBanner`(`banner.dart`)、`PerformanceOverlay`(`performance_overlay.dart`)、
`PopScope`(`pop_scope.dart`);另外把 `SystemTextScaler` 加进了 `media_query.rs`。覆盖率
1722/1888(91.2%)。测试 3053。

**(先按上一轮的规矩查了文件名:`app.rs` 已经存在——但它是完全另一回事,是 shell 契约、引擎调进来的那套
FFI。所以新模块叫 `widgets_app.rs`,并在文件头写明了这个区别。)**

---

**`PopScope` 是最容易被读反的一个类,而读反的地方是以为 `onPopInvokedWithResult` 能拦住一次返回。它不
能**——上游自己的文档就写着:这个回调跑的时候,**「the pop has already happened」**。

**否决是一个字段:`canPop`,而且必须提前设好。** 因为这个决定在手势到达的那一刻就要用,而一个能晚点回答
的回调,得让整个导航停在那儿等它想清楚。

**而这个回调在返回被取消时照样会跑**,由 `didPop` 说明发生了哪一种。**这才是它有用的原因:一个拒绝返回
手势的页面,恰恰就是那个想弹「要放弃修改吗?」的页面,而它必须听见自己刚挡下的那次尝试。**

---

**`CheckedModeBanner` 整个类是一个 `build`,而它的函数体整个坐在 `assert(() { ... return true; }())`
里面**——那是 Dart 写「release 编译器会删掉的代码」的方式。**在 release 构建里,这个 widget 就是它的孩
子,它身上没有任何东西还要花钱。**

而横幅的位置是 `topEnd`、文字方向却被**硬写成从左到右**,于是那个 DEBUG 标签不管应用的语言是什么,永远
在右上角。这是有意的:**横幅是给做这个应用的人看的,让它随语言换位置只会让它更难找。**

---

**`PerformanceOverlayOption` 的变体上有一句注释值得留:** 「these must be in the order needed for their
index values to match the constants in `performance_overlay_layer.h`」。**声明顺序是一份 ABI。** 调换它
们不会编译不过,**只会让这个覆盖层安静地显示错的东西。** 回归行把四个下标逐个钉住了。

而它显示两套数字,是因为**有两个线程、两种丢帧的方式**:UI 线程可能来不及**搭**图层树,光栅化器可能来
不及**画**它。**知道是哪一个超了预算,才是这个覆盖层的全部诊断价值**,一个数字说不了这件事。

---

**`WidgetsApp` 那一串断言之间说的是同一件事:一个应用必须有某种办法产出一条路由。** `home`、
`routes['/']`、`onGenerateRoute`、`onUnknownRoute`、`builder`——至少要有一个;而其中几种组合是**冗余**
而不是错的(比如同时给 `home` 和 `routes['/']`,**两个在回答同一个问题,而没有东西说谁赢**)。

**而查找顺序本身有意思:`home` 和 routes 表在 `onGenerateRoute` 之前被查。** **一条有人写下来的路由,赢
过一条有人本来会算出来的。**

还有一条:**用了 `home` 或 `routes`,就必须给 `pageRouteBuilder`**——默认处理器要造一个 page route,而没
有别的东西告诉它造哪一种。

---

**`SystemTextScaler` 正好补上了本 crate `TextScaler` 文档里预留的那句话**(「non-linear platform
scalers would arrive as a new variant」)。

**它之所以不能是一个数,是因为新版 Android 的缩放不是线性的**:在很大的无障碍设置下,**小字被放大很多,
本来就大的字放大得少得多**,好让版面不至于散架。**没有哪一个乘数能描述这件事**,所以缩放必须是一次对平
台的调用。

而 `textScaleFactor` 仍然留着,上游写明了它是干什么的:**用来比较两个 scaler,不用来做算术。** 两个因子
相同的系统 scaler 对同一个输入给出同一个输出,所以这个因子是一个可靠的身份;**但拿字号去乘它,是在发明
一个平台从没同意过的线性模型。** 回归行按一条真的非线性曲线把这一条钉住了。

它的 `==` 还有一处:**因子恰好是 1.0 的系统 scaler,等于 `TextScaler.noScaling`**——两者在外延上是同一个
函数,于是一个拿「没有缩放」去判断能不能走捷径的 widget,不必知道 scaler 从哪来就能得到正确答案。

验证:`cargo test --lib` 3053 绿,GN `rustflutter_unittests` 3053 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1722 accounted / 166 MISSING(91.2%)。

### 不确定态是走到的,不是路过的(2026-08-20)

两个新模块:`toggleable.rs`(`ToggleableStateMixin`、`ToggleablePainter`)与
`sliver_fill.rs`(`SliverFillViewport`、`SliverFillRemaining`)。覆盖率 1717/1888(91.0%)。测试 3031。

`toggleable.dart` 是复选框、开关、单选按钮共用的那套机器——**上一轮 `RawRadio` 混入的正是它**。而它们共
有的不是形状,是几个**问题**:一次点击接下来该做什么、控件怎么从一个视觉状态走到下一个、以及墨水反应怎
么同时听三件事的话。

**`_handleTap` 的循环就是设计本身:**

```dart
case false: onChanged!(true);
case true:  onChanged!(tristate ? null : false);
case null:  onChanged!(false);
```

**于是三态控件是「关 → 开 → 不确定」,而不是「关 → 不确定 → 开」。** **不确定是一个读者走到的状态,不是
他在打开某样东西的路上必须路过的一站。**

**而 `animateToValue` 里有一处该是 `else` 的地方写成了并列语句,那不是笔误:**

```dart
if (value == null) { _positionController.value = 0.0; }
if (value ?? true) { _positionController.forward(); } else { ... }
```

**对 `null`,两句都跑:位置被拍回 0,然后重新向 1 跑一遍。** 于是**一个转到不确定态的三态复选框,不是停
在空和满之间的某处——它先清空,再重新填。** 这才让「不确定」看上去像一个**故意的状态**,而不是一个**没做
完的动画**。

而**不是三态的控件把 `null` 读成 false**,直接清空——同一个值,两个相反的动作。回归行把这一对放在一起
钉住了。

**墨水那边,`paintRadialReaction` 的三层 `Color.lerp` 嵌套是一个优先级:**

```
lerp( lerp( lerp(inactive, reaction, position), hover, hoverFade ), focus, focusFade )
```

**最外层那一层说了算:focus 盖过 hover,hover 盖过控件自己的值。** 一个持有焦点的控件,不管别的怎样,看
上去就是持有焦点的。

**而半径的规则不一样:focus 或 hover 时直接就是整个 splashRadius,不做动画;只有点击的墨水才从零长
起。** 区别在于**点击有一个可以长出来的点,而另外两个没有**——控件上没有哪个位置是「获得焦点」发生的地
方。回归行按 `reaction_origin` 把这一条也钉了:有按下点就从按下点长,没有就从中心。

---

**`sliver_fill.dart` 那两个是从视口而不是从内容取尺寸的 sliver。**

**`SliverFillViewport` 的端部内边距是
`padEnds ? clamp(1 - viewportFraction, 0, 1) / 2 : 0`。** 0.8 的分数给出每端 0.1 的内边距,正好把第一张
卡片停在正中。**而那个 clamp 做的,正是上游用文字说的那件事:分数大于 1 时 `padEnds` 没有效果**——每个孩
子都已经比视口宽了,没有什么可以居中的,而 `1 - fraction` 变负数就是这件事自己掉出来的方式,不用另写一
个 if。

**`SliverFillRemaining` 有两个布尔,却只有三个渲染对象。** `fillOverscroll` 只在 `hasScrollBody` 为
false 时被问,而上游把这句写在那个字段自己的文档里:**一个会滚动的孩子没有固定尺寸可以拉伸,第四种组合
没有意义。**

它的**默认值是更让人意外的那个:`hasScrollBody = true`,孩子会伸到视口外面去滚**——那正是
`NestedScrollView` 的 body 要的。**把它设成 false,才让这个 sliver 变成「把这页剩下的地方填满」。**

而填满是**一份好意,不是一个保证**:上游文档写明,当前面的滚动范围或孩子自己的尺寸超过视口时,**这个
sliver 会让位给孩子的尺寸而不是覆盖它。** **把一个装不下的孩子压扁,比让它溢出更糟。**

验证:`cargo test --lib` 3031 绿,GN `rustflutter_unittests` 3031 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1717 accounted / 171 MISSING(91.0%)。

### 刘海应用一次,而两侧的内边距要发给每一行(2026-08-20)

新模块 `scroll_view.rs`,收掉 `scroll_view.dart` 的 `ScrollView`、`CustomScrollView`、`BoxScrollView`,
以及 `single_child_scroll_view.dart` 的 `SingleChildScrollView`。**`widgets/scroll_view.dart` 全覆盖。**
覆盖率 1713/1888(90.7%)。测试 3006。

**这两个文件是同一个问题的两个答案,而它们为什么都存在,只有一行:**
**`SingleChildScrollView` 每一帧都把全部内容布局一遍;`CustomScrollView` 只布局屏幕上那些。** 上游关于
「别把长列表放进前者」的所有说法,都是这一行的推论。

而这一点不在孩子的**数量**上,在**布局**上:`_getInnerConstraints` **把滚动轴上的约束整个丢掉**,只把
交叉轴传下去。于是孩子按自己的自然尺寸展开,全部展开。而 `size = constraints.constrain(child.size)`
——**一个矮孩子会得到一个矮的滚动视图:它不用被要求就在 shrink-wrap。**

---

**`BoxScrollView.buildSlivers` 在没给 padding 时做的事,值得读两遍:它把环境里的 `MediaQuery` padding
沿两条轴劈成两半。主轴那一半被滚动视图自己吃掉(变成一个 `SliverPadding`),交叉轴那一半留在
`MediaQuery` 里给孩子们。**

在一台有刘海和 home indicator 的手机上,这恰好是对的:**竖直列表要的是把上下内边距用一次,用在滚动的两
端**——第一行从刘海下面开始,最后一行在指示条上面结束。**把它加到每一行上,会在列表中间留下一道道空
隙。** 而**左右内边距必须发给每一行**,因为每一行都横跨整个宽度。

**而只要显式给了 padding,这一整套就关掉了**——写了 padding 的调用方是想过的。

---

**`ScrollView` 里最有后果的默认值是物理:**

```dart
physics = physics ?? ((primary ?? false) || (primary == null && controller == null && scrollDirection == Axis.vertical)
    ? const AlwaysScrollableScrollPhysics() : null);
```

**一个没有自己 controller 的竖直滚动视图,拿到的是「永远可滚动」——它在内容装得下时也会回弹。** 这看着
像浪费,直到你注意到这样一个视图通常是什么:**它是页面。而一个被拉动却不动的页面,不管里面装了什么都像
是坏了。** 而一个横向轮播、或者一个自带 controller 的视图,是一个组件而不是页面,**短的就干脆不动。**

注意条件的第一支**根本不看轴**:一个显式要 primary 的横向视图,照样拿到「永远可滚动」。回归行把这一支
单独钉住了。

其余几条:

* **拿走 primary controller 的视图,同时把它对下面挡住**(`PrimaryScrollController.none`)。上游把理由写
  在注释里:**否则里面嵌的滚动视图会继承同一个,两个列表驱动一个 controller。**
* **`shrinkWrap` 和 `center` 互斥**:一个 shrink-wrap 的视口没有固定尺寸,**而「居中」是一句关于固定尺寸
  的话。**
* **「primary 且带 controller」的报错信息说清了矛盾在哪:** primary 视图是**靠继承**拿 controller 的,同
  时又递一个进来,这个问题没有答案。
* **键盘收起有三级回退**(widget → scrollBehavior → `ScrollConfiguration`),而 `onDrag` 检查的是
  `dragDetails != null`——**只有手指还在玻璃上才算。一次列表还在替读者惯性滑行的甩动,不收键盘。**
* **`performLayout` 会在偏移越界时修正它:** 一个在读者滚到底时缩短了的孩子,会把偏移拉回来,**而不是留
  给读者一片空白。**

**另外,这一轮开头先执行了上一轮定下的新规矩:落盘之前 `ls` 了一眼 `scroll_view.rs`。** 它不存在,可以
写;而 `scrollable.rs` 那三个类下一轮再说——`Scrollable` 在账本里已经映射到 `scrolling::Scroll`,那一族
得先读清楚再动。

验证:`cargo test --lib` 3006 绿,GN `rustflutter_unittests` 3006 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1713 accounted / 175 MISSING(90.7%)。

### 差点把一个已经存在的模块整个覆盖掉(2026-08-20)

`scrollbar.dart` 的三个类补齐:`ScrollbarPainter`、`RawScrollbar`、`RawScrollbarState`。覆盖率
1709/1888(90.5%)。

**先记这一轮最该记的事:我差点把 `src/scrollbar.rs` 整个写没了。** 我按惯例写好新模块、`cp` 过去、注册
——然后编译器报了一串 `Scrollbar` 找不到。**这个 crate 早就有一个 `scrollbar.rs`**,里面有 `Scrollbar`
widget、`thumb_within` 的简化算术和淡入淡出。`cp` 把它整个盖掉了。

`git checkout` 救回来了(而这正是「先提交、再动手」的价值),但**这条规则得写下来:新模块落盘之前,先
`ls` 一眼那个文件名。** 覆盖率表说某个类 MISSING,不等于那个**文件**不存在——尺子是按**类名**匹配的,
一个已经移植了一半的模块,在表上看起来和一片空白一模一样。

最后这三个类是**追加**到既有模块里的,并在接缝处写明了关系:上面是本 crate 自己的简化算术,下面是上游
完整的那套,而两者的差别恰好是三处——**轨道两端的边距、越界时可以低于最小值的滑块、以及把指尖和光标区
别对待的命中测试。**

---

**一个滚动条要在两个相反的方向上回答同一件事:** 内容在哪 → 滑块多大、在哪;滑块被拖到哪 → 内容该去
哪。**这一对必须严格互逆,否则滑块会从按着它的那根手指下面滑走。**

而其中的微妙处是:**滑块能走的距离是轨道减去它自己的长度**,而这段更短的距离才映射到整个可滚动范围。
拿整条轨道去除,会让内容还没到底、滑块已经贴住了。回归行两头都钉了。

**滑块的长度就是「我在看这东西的多少」的答案。** 而下限那里才有意思:

* **正常滚动时不低于 `minLength`(默认 18)**——再小就没有可抓的了。
* **越界时可以更小,低到 `minOverscrollLength`。** 而上游没法用「可见比例」在这个边界上插值,因为那个
  比例在跨过边界时不连续;**于是它改用「视口里还剩多少内容」这个比例,并把 `[0.8, 1.0]` 映到
  `[0.0, 1.0]`**。理由写在源码里,而且是一句观察:**「iOS behavior appears to have the thumb reach its
  minimum size with ~20% of overscroll」。**
* **而这条下限只在列表足够长时才起作用。** 我第一版回归行用了一个短列表,红了——比例算出来的滑块本来就
  比两个最小值都大,那两个数根本没被问到。改成十万像素的列表才真正落到下限上。

还有一句上游的注释值得留:**滑块不能比轨道长,「otherwise the scrollbar may scroll towards the wrong
direction」**——比可走距离还长的滑块会让那个除数变成负的,把映射整个翻过来。

**命中测试有两条,而两条都不是简单的矩形包含:**

* **淡出到零的滚动条是碰不到的——除非有鼠标在悬停。** 而这一条必须成立,因为**鼠标移到窗口边缘,要的正
  是把它叫回来。** 手指没法悬停,所以这个例外只给鼠标。
* **指尖拿到被撑到最小可交互尺寸(48)的滑块,光标拿到画出来的那个。** **光标是一个像素,指尖不是**;给
  鼠标也加上这圈padding,会让一条细滚动条吃掉旁边内容的点击。而**这一条同样只在滑块比 48 小时才有区
  别**——我的第一版回归行用了一个 80 像素的滑块,撑了等于没撑,也红了。

**`update` 的提前返回比的是 `extentBefore`/`extentInside`/`extentAfter` 和轴向,不是 `pixels`。** 那三
个数才是滑块画出来的东西,而一次被物理吸收掉、三个数都没动的 pixels 变化,没有任何东西要重画。

**构造函数的四条断言各自排除了一种「画出来没有意义」的配置:**
「A scrollbar track cannot be drawn without a scrollbar thumb」(空槽什么也没说)、
`minOverscrollLength <= minThumbLength`(越界下限是更小的那个)、两者非负、
以及 `radius` 和 `shape` 不能同时给——**两种说法说同一件事,而没有规则说谁赢。**

**点轨道是「翻一页」而不是「跳到那儿」**,用 100ms 的 `easeInOut`。跳过去会把内容移到读者跟不上的地
方;**而「页」是他们从键盘上已经认识的单位。** 而在一个不接受用户偏移的滚动位置上,点轨道什么都不做。

最后:**`_maybeStartFadeoutTimer` 的守卫就是这个方法的全部**——一个被要求常驻的滚动条,从不开始淡出。

验证:`cargo test --lib` 2976 绿,GN `rustflutter_unittests` 2976 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1709 accounted / 179 MISSING(90.5%)。

### 「占多少地方」和「看得见多少」是两个数(2026-08-20)

新模块 `sliver_headers.rs`,一次收掉四个文件五个类:`SliverPersistentHeaderDelegate`、
`SliverPersistentHeader`、`PinnedHeaderSliver`、`SliverResizingHeader`、`SliverFloatingHeader`。覆盖率
1706/1888(90.4%)。

**这四个是「一个留下来的头部」的四种答案,而把它们分开的是 sliver 协议做出、别处都没有的一个区分:**

* **`layoutExtent` 是这个 sliver 从后面的内容那里占走多少地方。**
* **`paintExtent` 是你能看见它多少。**

**对普通内容,这两个是同一个数。而这里每一个头部,都是这两个数的一组不同答案。**

**钉住这条的回归行是最直接的那个:一个 pinned 头部滚到很远处时,`layoutExtent` 是 0、`paintExtent` 仍
然是 56。** 它不再占地方,而仍然被看见——**这就是「钉住」。**

**四个答案:**

**`SliverPersistentHeader` 是最老、最通用的那个,而它是唯一一个要求调用方「预先说出尺寸」的。**
`minExtent` 和 `maxExtent` 在任何东西被布局之前就被问了,而上游的文档坚持它们**在委托的一生中不能
变**——必须完全来自构造参数,想给别的答案就得通过 `shouldRebuild` 说。**另外三个头部之所以存在,很大程
度上就是因为「对一个你还没量过的 widget 做出这种承诺」很难。**

它的 `pinned` 和 `floating` **是两个独立的问题**,所以是四个渲染对象而不是一条光谱:**`pinned` 问的是
「读者滚过去时会怎样」,`floating` 问的是「读者回头时会怎样」。** 而 snap 配置**只在 floating 时才被理
会**——snap 是回来路上的事,一个不会自己回来的头部,没有一条可以 snap 的回来路。

**`PinnedHeaderSliver` 是那个窄用例,而它更好正是因为窄:没有委托,也不必预测尺寸,因为它只有一个尺
寸。** 它量自己的孩子然后报上去。它的 `maxScrollObstructionExtent` 是孩子的整个高度——**这是它告诉视口
「这么多以后再也滚不到了」的方式。** 而 `paintOrigin` 取 `constraints.overlap`,**这让它叠在前一个钉住
的东西「下面」而不是「底下」。**

**`SliverResizingHeader` 的区别是:两个尺寸是 widget 而不是数字。** 给它一个一行版和一个三行版,它自己
去量;不给最大原型,它就对孩子做一次 **dry layout**——量而不落地。而**孩子是被约束着缩小的,不是被裁
的**,这才让标题移动、副标题消失,而不是被从中间切断。它的 `scrollExtent` 是**全高**、
`maxScrollObstructionExtent` 是**缩到最小后的那点**——两个数说的是不同的事,而两件事都要紧。

**`SliverFloatingHeader` 的诀窍是它根本不按 `constraints.scrollOffset` 布局。** 它自己维护一个
`effectiveScrollOffset`,按读者滚动的**增量**移动。**于是往回滚五十像素,就带回来五十像素的头部——不管
读者在列表的多深处。** 而读者刚一回头时,如果这个偏移已经超过了孩子的高度,就被**停到「刚好在视口上
沿」**,好让它从那里滑进来,而不是从它当初消失的地方。

上游还留了一条噪声守卫,并且写明了:**增量为正(头部在长大)而方向不是往回滚,是矛盾的——当成噪声,归
零。**

**而 `snapMode` 是「有没有东西让路」的两个答案:** `overlay` 的 `layoutExtent` 按真实滚动偏移算(头部
盖在内容上,内容不动),`scroll` 的等于 `paintExtent`(内容被推下去)。回归行把同一个回来的头部按两种模
式各跑一遍,确认**看见的一样多、占的地方不一样。**

**还有一条四个都一样的:`hasVisualOverflow: true`,并且注释说明了是有意保守的——"Conservatively say we
do have overflow to avoid complexity."** 这个方向错了,代价是一层多余的裁剪;**反过来错了,代价是一个
画到视口外面去的头部。**

最后,`PinnedHeaderSliver` 在盖住内容时会给孩子打上 `excludeFromScrolling` 标签:**一个试图滚动到钉住
头部的读屏器,会永远滚下去。**

验证:`cargo test --lib` 2950 绿,GN `rustflutter_unittests` 2950 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1706 accounted / 182 MISSING(90.4%)。

### 从一个提示移到下一个,不再等待(2026-08-20)

新模块 `raw_tooltip.rs`,收掉 `TooltipPositionContext`、`RawTooltip`、`RawTooltipState`。覆盖率
1701/1888(90.1%)——**九成。**

**一个 tooltip 基本上是一个钟。** 这个文件里几乎没有一行是关于「画什么」的,全部是关于**什么时候**:指
针要停多久才出现、出现后待多久、什么会取消它,以及**连着的第二个应该和第一个有什么不同。**

**而最后那一条是这个文件最值得读的一行:**

```dart
_scheduleShowTooltip(withDelay: tooltipsToDismiss.isNotEmpty ? Duration.zero : widget.hoverDelay);
```

**从一个 tooltip 移到下一个,延迟被整个跳过。** 延迟存在的理由是「不要在指针只是路过时弹东西出来」;而
**一个刚读完一条提示的读者,已经表明了他在读提示**——再让他等一次,是在回答一个他已经不再问的问题。回归
行先单独证明了第一个 tooltip 确实要等(否则这条什么都没证明),再证明第二个不等。**并且把这条守卫强行
改成 false 跑了一遍,确认测试会红。**

同一段里还有两处:被解散的只有**没有任何鼠标还悬停着的**那些;而上游说明了在这里做这个检查为什么安全
——鼠标追踪器**总是先派发完所有 `onExit` 再派发任何 `onEnter`**,所以刚离开别处的那个设备,已经从别处的
集合里被移走了。

**默认值本身就是设计:** hover 延迟 **0**、touch 延迟 **1500ms**、dismiss 延迟 **100ms**。
**一个已经停在某个东西上的鼠标,已经等过了**;而**一根抬起来的手指没有办法说「我还在看」**,所以它的
tooltip 必须自己结束。那 100ms 的宽限,刚好够越过按钮和它弹出的提示之间那一像素的缝。

**`_activeHoveringPointerDevices` 是一个设备 id 的集合而不是一个布尔量**,因为可以有两个鼠标。于是:

* **最后一个鼠标离开时才走,不是第一个。**
* **手指抬起来时,如果还有鼠标悬停,`_handlePressUp` 什么都不做**——松开一根手指,不该关掉别人正按着的
  提示。
* **一次 tap 在有鼠标悬停时不设自毁定时器**(`touchDelay: null`)——鼠标会说什么时候结束。
* **而点击别处时,悬停集合也被清空**;否则 tooltip 会先被解散,再立刻被一个根本没动过的鼠标续上。

其余几条:

* **`_scheduleShowTooltip` 的 else 分支:已经在进场或已经在场的 tooltip,延迟被跳过、立即显示。** 一旦
  它已经在那儿了,就没有什么可等的了。
* **`_scheduleDismissTooltip` 读的是 `_backingController` 而不是那个懒加载的 getter**,上游注释写明了理
  由:**问一个 tooltip 在不在,不该顺手把它的动画机器造出来。** 这一条单独钉了一行。
* **状态变化是在「边」上处理的:** 上游 switch 的是 `(原来是 dismissed, 现在是 dismissed)` 这一对,四种
  情况里两种是有意的空。于是**动画走完不是第二次到达**,而**正在退场的 tooltip 仍然算「在显示」**——它确
  实还在屏幕上。
* **`ensureTooltipVisible` 取消定时器且不设新的**,所以这样显示出来的提示会**一直待着**,直到有东西来关
  它。
* **全局指针路由的理由被写下来了:** 全局路由在其他路由**之后**派发,于是 tooltip 能知道别处被点了,**而
  没有把那次点击从别人那里抢走。**
* **`_ExclusiveMouseRegion` 用两个静态可变标志跨越一整趟命中测试**,让嵌套的 tooltip 只有最内层那个收到
  进出事件(一个带删除图标的 chip,不该同时弹两条)。而**最外层那个在回程上重置标志**——这正是静态在这里
  安全的原因:这趟测试是同步单线程的,而且总是从同一个地方开始和结束。

**一处自查:** 我最初把延迟到期后的 `show` 写成了一个两臂都返回 `None` 的重言式,丢掉了调度时带着的
touch 延迟——那会让一个点出来的 tooltip 永远不消失。上游的 `show` 是个**捕获了 `touchDelay` 的闭包**,
所以那个值必须跟着定时器一起走。改成 `TooltipTimer::Show { at_ms, touch_delay_ms }`,并补了回归行。

验证:`cargo test --lib` 2928 绿,GN `rustflutter_unittests` 2928 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1701 accounted / 187 MISSING(90.1%)。

### 单选按钮是唯一一个没法单独做的控件(2026-08-20)

新模块 `radio_group.rs`,一次收掉 `radio_group.dart` 与 `raw_radio.dart` 全部四个类:`RadioGroup`、
`RadioGroupRegistry`、`RadioClient`、`RawRadio`。覆盖率 1698/1888(89.9%)。

**一个单选按钮显示成什么样,取决于它的兄弟在做什么;而按下它会改变它们。** 于是这两个文件就是这套安
排:**组持有值,而按钮向组注册、自己不存任何状态。**

`RawRadio` 的 `value` getter 是一行:`widget.value == registry?.groupValue`。**由此得到一个不那么显然
的结论:一个没有组的单选按钮不是「未选中」,而是「不可交互」**——没有 registry 就没有东西可比,也没有东
西可通知,所以 `onChanged` 是 null,而 null 的 `onChanged` 正是 toggleable 那套机制读作「关掉」的信号。

**而这个文件真正的分量在键盘上,那两件事都不是从 widget 树里长出来的,是平台约定,各自需要一个类去安
排:**

**其一:一个单选组是「一个」Tab 停靠点,不是每个选项一个。** `_SkipUnselectedRadioPolicy` 在读序排序里
把**未选中的同伴全部剔掉**,只留下选中的那个。没有它,走过一个五选一的组要按五次 Tab。

而这个策略的两个退路都想到了:

* **一个还没人回答过的组,拿读序里第一个单选按钮顶上**——否则 Tab 进一个空组会落在什么都没有上。
* **当前获得焦点的那个节点永远不被剔除**,上游写明了原因:它是排序的 `currentNode`,没法把它从结果里
  拿掉。

**其二:方向键在组内移动,而且移动的同时就选中。** `_selectRadioInDirection` 一次做两件事——
`onChanged(...)` 加 `requestFocus()`。**这就是为什么方向键不能交给普通焦点遍历:遍历只会移动,而单选组
的约定是「走到哪儿就选到哪儿」。** 走到头会绕回开头,而**禁用的按钮不是一个「会被跳过的停靠点」,它压根
不在环上**。

**`_RadioGroupShortcutManager` 只做一件事,而那件事是一个否定:没有任何单选按钮持有焦点时,按键被
`ignored` 而不是 `handled`。** 上游把理由写下来了——不要吞掉本该给当前持有焦点的非单选控件的事件。**一
个把子树里所有按键都吃掉的快捷键管理器,会让这个组变得没法住人**:组里放一个文本框,方向键就再也移不动
光标了。

**空格键那条规则值得单独说:按在已选中的按钮上什么都不做,除非它是 tristate。** 一个能被误触空格清空的
单选组,是一个比不能清空的更糟的控件。

其余几处:

* **`RadioClient.registry` 的 setter 照搬了上游的不对称:注销是有条件的(`_registry != newRegistry`),
  注册是无条件的。** 于是把同一个 registry 赋两次,会在没注销的情况下再注册一次——**只因为 registry 存
  的是一个 `Set`,这才无害。** 两半都钉住了。
* **`initState` 里 `registry = widget.groupRegistry` 写在 `super.initState()` 之前**,上游注释说明了原
  因:`ToggleableStateMixin` 初始化时会读 `value`,而 `value` 是一个问向 registry 的问题。**注册顺序是
  承重的。**
* **`_handleChanged(false)` 什么都不做。** 单选按钮没法靠「被按一下」把自己取消——只有组、或者一个
  toggleable 按钮的第二次按下,能清掉这个值。
* **那条 debug 检查比的是 `< 2`**:允许一个**什么都没选**的组(未选中是一个合法状态),只拒绝同时选中两
  个——这个策略根本描述不了一个允许多选的组。
* **语义按平台分岔,而分岔的理由被写下来了:** iOS/macOS 设 `selected` 属性,并且**只给未选中的按钮一条
  hint**——因为选中状态 iOS 已经从 `selected` 念过一遍了,再给一条 hint 就是说了两次。其余平台只用
  `checked`。

验证:`cargo test --lib` 2901 绿,GN `rustflutter_unittests` 2901 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1698 accounted / 190 MISSING(89.9%)。

### 「挡住」和「排除」听着像一回事,方向正好相反(2026-08-20)

两个新模块:`semantics_markers.rs` 收掉 `basic.dart` 剩下的语义五件套(`SliverSemantics`、
`MergeSemantics`、`BlockSemantics`、`ExcludeSemantics`、`IndexedSemantics`),
`page_transitions_builder.rs` 收掉 `PageTransitionsBuilder`、
`FadeUpwardsPageTransitionsBuilder`、`OpenUpwardsPageTransitionsBuilder`。**`widgets/basic.dart` 至此
全覆盖**(65 covered + 4 mapped + 3 blocked)。覆盖率 1694/1888(89.7%)。

**语义那四个各自只有三行,而为什么是四个而不是一个开关,答案是它们删掉的东西不一样。最值得写下来的是
`BlockSemantics` 和 `ExcludeSemantics`——名字听着像一回事,看的方向正好相反:**

* **`ExcludeSemantics` 往里看**:丢掉**自己这棵子树**。
* **`BlockSemantics` 往外、往后看**:丢掉在同一个语义容器里**比它先画**的一切。**不是它的后代,是它的
  兄弟——而且specifically 是屏幕上压在它下面的那些。**

**而「先画」这件事只在屏幕上有意义,树里没有。** 这就是为什么这个类必须按绘制顺序来解析,回归行也是按
绘制顺序构造的:画在 block 之后的东西完全不受影响。

上游给的场景把这个区别说透了:**一个弹窗、一个拉开的抽屉——它们要藏起来的东西不在自己下面,而在自己身
后**,而且那些东西还部分可见。**一个仍然够得到它们的读屏器,会让读者去操作一个已经不在眼前的页面。**

其余三条:

* **`blocking` 和 `excluding` 是字段而不是「这个 widget 在不在」**,于是一个 widget 可以停止挡住,而不
  必从树里消失。
* **`MergeSemantics` 合并是有代价的,上游写得很小心:** 标签用换行拼起来,而**如果被合并的子树里不止一
  个节点能处理手势,树序里第一个拿走回调**——其余的不是被合并,是被丢掉。
* **`IndexedSemantics` 的存在理由,上游用一个例子给了:** 列表里夹着 `Spacer`,自动索引会把空白也数进
  去,于是读屏器宣布「可见四项」而实际只有两项。

---

**另一半是两个 Android 页面转场,而把它们摆在一起才有意思:同一个问题,两种相反的习惯。**

**`FadeUpwards`(Android O)用两条曲线做两件事:** 位置走 `fastOutSlowIn`,不透明度走 `easeIn`。**于是
页面已经基本到位了,却还基本是透明的——它看上去不是滑进来的,是落定进来的。** 回归行按「位置进度比不透
明度至少多 0.2」钉住了这个差。

**`OpenUpwards`(Android P)反过来:一条曲线管全部,而动画有两个,因为旧页面也在动。**

* **新页面根本没有淡入——它是被「露出来」的。** 上游把页面按**全高**放进一个 `OverflowBox`,再让外面的
  裁剪框从底部长上来。**于是内容从不被压扁,变的只是你能看见多少。**
* **旧页面跟着新页面走同一个方向,只走一半远**(5% 对 2.5%)。**不是被推开,是跟过去**,外加一层压到四
  分之一黑的遮罩。
* **那条曲线 `Cubic(0.20, 0.00, 0.00, 1.00)` 的第二个控制点 x 是 0**,把运动整个拽到了前面:**时间过去
  五分之一时,动画已经走完一半以上。** 页面「弹开」,然后慢慢把最后一点走完。
* **反向用的是 `curve.flipped` 而不是把同一条曲线倒着跑。** 一条前重的曲线倒着跑,出去时还是前重的——那
  看上去像页面「啪」地合上。回归行把这两件事分开钉住了:正向在 t=0.2 已过 0.45,镜像在同一点还不到
  0.05。

**`PageTransitionsBuilder` 本身有一处默认值值得点名:`reverseTransitionDuration` 默认等于
`transitionDuration`,而不是等于它自己的一个常数。** 于是一个只想把入场改长的子类,**顺手就得到了对称
的出场**,只有真想让两者不同时才需要开口。回归行用一个只覆写了入场时长的子类验了这一条。

验证:`cargo test --lib` 2878 绿,GN `rustflutter_unittests` 2878 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1694 accounted / 194 MISSING(89.7%)。

### 同一个文件里承认了两次「不知道为什么」(2026-08-20)

新模块 `overscroll_indicator.rs`,收掉 `GlowingOverscrollIndicator`、
`StretchingOverscrollIndicator`、`OverscrollIndicatorNotification`。覆盖率 1686/1888(89.3%)。

**这两个指示器不是一个设计的两种皮肤,而是对同一个问题的两个答案:辉光在内容「上面」画一个东西,拉伸让
内容自己变形。** 而 `ScrollBehavior` 把辉光恰好发给那些物理本身不拉伸的平台——上一轮已经记过,这一轮
是它的另一半。

**而这个文件最值得写下来的,是它承认了两次「不知道为什么」。**

**第一次在辉光的 `pull` 里:**

```dart
_pullDistance += overscroll / 200.0; // This factor is magic. Not clear why we need it to match Android.
```

**第二次在拉伸的常量里,而这一次上游写了整整一段:** `kNaturalFrequency` 和 `kDampingRatio` 是从 Android
的 `EdgeEffect.java` 直接搬来的,可是照搬之后动画「noticeably faster than the native Android
behavior」,而「**The underlying reason for this discrepancy is unknown**」。于是有了
`kTimeCorrectionFactor = 0.8`——一个**靠肉眼比对(“eyeballing”)得出的**数。

**这一段还顺手写下了一个等价关系,而代码用的正是它:** 缩放时间 `t`,等价于把固有频率和初速度乘以同一
个系数。于是实现里 `stiffness` 乘了 **0.8²**、初速度乘了 **0.8**——**一个物理上的恒等式,被用来把「改
时间」翻译成「改弹簧」。**

---

**辉光的两个入口是两件不同的事,而不是一件事的两种参数:** `absorbImpact` 是一次**飞掷撞到了边**,亮度
由**速度**给;`pull` 是一根**手指拖过了边**,亮度由**距离/视口**给。滚动视图告诉它的本来就是两回事。

**而这里我自己的测试先错了一次,错法值得记下来:** 我写了「快的比慢的亮」,红了。因为**不透明度是拿它
自己的 `begin` 当下界去 clamp 的**:

```dart
_glowOpacityTween.end = clampDouble(velocity * _velocityGlowFactor, _glowOpacityTween.begin!, _maxOpacity);
```

空闲的辉光 `begin` 固定是 0.3,而 `0.3 / 0.00006 = 5000`——**于是任何低于 5000 px/s 的撞击,看上去完全
一样。速度要到 5000 以上才开始说话。** 回归行现在把这条边界两侧都钉住了(4900 与 6000)。

其余几条:

* **一次拖动中辉光只会变大,不会变小**——`math.max(..., _glowSize.value)` 是规则不是保险丝,只有 recede
  会把它带下去。
* **手指不必松开。** 停下不动 167ms,就会启动一段 **2000ms** 的慢衰减;而真的松开,给的是 **600ms**
  ——**「我停下了」和「我放手了」是两件事,淡出的时间差了三倍多。**
* **`_pullDistance` 一直留到辉光彻底消失才清零**,于是一个松手又在淡出途中重新抓住的读者,是接着来的,
  不是从头来的。
* **有一个分支专门为「拖动比它自己的动画活得久」而存在:** 167ms 的 pull 动画跑完了而手指还在动,这时没
  有任何动画会去安排下一帧,于是它自己 `notifyListeners()`。
* **辉光横向追手指,半衰期恰好是 60Hz 的一帧**——每帧走完剩余距离的一半:够快,看着是黏在手指上的;够
  慢,不是瞬移。
* **那道弧其实是一个半径为视口宽度 1.5 倍的圆**,在 Y 上按 glowSize 压扁,再裁进一条只有宽度约五分之一
  高的带子里。**一个宽这么多的圆,露出来的只有它很平的顶部**——那正是 Android 辉光的形状。

---

**拉伸那边的强度是「线性 + 指数」,而两半是轮流工作的:** 起手时指数项主导(它在 0 处的斜率是线性项的
`EXPONENTIAL_SCALAR` 倍,约九倍),然后**指数项饱和成一个常数,只剩下慢慢长的线性项**。**一次已经拉得
很远的拖动,再拖几乎不动了——这就是「阻力」的手感。**(我最初把这句写成「约两倍」,是错的,回归行按解
析斜率重算并钉住。)

* **`_interruptedOverscroll`:** 一次 pull 打断正在跑的回弹时,**先把动画当下的值抓下来,再加到 pull 算
  出的量上**。否则一根从没抬起来的手指,会看着边缘先弹回零再重新拉开。
* **`scrollEnd` 只在没有 controller 时才起跑**——已经在回家路上的弹簧,不该被再踢一脚。
* **`whenComplete` 里那个 `if (_controller == controller)` 守卫**,上游注释写明了理由:后来的 pull 可能
  已经换掉并销毁了它,**一个过期的完成回调不能去碰共享状态**(也不能二次销毁)。
* **只有「有拉伸」并且「视口比屏幕小」时才裁剪。** 上游把理由写下来了:视口占满屏幕时,溢出根本没地方
  被看见——**而一个 clip 是一层,是要钱的。**

**`OverscrollIndicatorNotification` 三个字段里有两个是可变的**,这对一个通知很不寻常,而这正是这个类的
用途:**它往上冒,并且在路上被写进去。** 祖先可以直接否决(`disallowIndicator`),也可以只挪一挪画的位
置(`paintOffset`,顶部栏用它让辉光出现在栏下面而不是栏底下)。**而否决是单向的——没有对应的
`allow`,于是一个祖先的反对不会被另一个祖先推翻。**

**另一处自查:** 我先按想当然写了 `main_axis_scale` / `grows_from_leading_edge` 两个方法,读了
`build` 才发现上游根本不是这么做的——它交给 `StretchEffect` 的是 `-overscroll`(反向轴再取一次负),裁剪
的判断也另有其事。两个方法整个换成了上游真有的 `stretch_strength` 与 `clips`。**在读到实现之前写下的
API,是猜的。**

验证:`cargo test --lib` 2857 绿,GN `rustflutter_unittests` 2857 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1686 accounted / 202 MISSING(89.3%)。

### 销毁这个句柄,就是那句话本身(2026-08-20)

`automatic_keep_alive.dart` 整个文件收口(`KeepAliveNotification`、`KeepAliveHandle`、
`AutomaticKeepAliveClientMixin`,外加 **`AutomaticKeepAlive` 本身**),以及 `scroll_delegate.dart` 的二
维三件套(`TwoDimensionalChildDelegate`、`TwoDimensionalChildBuilderDelegate`、
`TwoDimensionalChildListDelegate`)。覆盖率 1683/1888(89.1%)。

**这一轮同时退掉了一条记录在案的分歧。** 账本里 `AutomaticKeepAlive` 一直记着「离窗即弃(记录在案分
歧)」——本 crate 就是把滚出视口的孩子丢掉。现在它是真的移植过来了,那条 `equivalent` 条目已删除。**一
条诚实的分歧记录,过期之后就变成一句恭维话。**

**`KeepAliveHandle` 上游的全部代码,是一个 `dispose` 覆写,里面先 `notifyListeners()` 再
`super.dispose()`。** 从 dispose 里发通知,通常恰恰是不该做的事。**但在这里,销毁就是那句话本身**——这
个句柄存在只为传一条通知:「我不再需要被留下了」,而毁掉它就是说出这句话的方式。

**而 `KeepAliveNotification` 只有一个字段,并且那个字段是 `Listenable` 而不是一个布尔量。** 因为有意思
的消息不是「留住我」,而是「你可以放手了」。**通知说的是前一句;它捎带的那个句柄,是后一句稍后到达的通
道——而那时发信的 widget 可能已经不在了。**

**客户端那一侧最值得写下来的是 `deactivate`:句柄在离开树时被释放,哪怕这个 widget 仍然想被留下**,然
后由 `build` 在回来时重新建立。**这个不变式是每次 build 重新申明的,而不是跨 build 携带的**——否则宿主
会攥着一个属于「已经搬到别处去的子树」的句柄。

由此又生出一条:**`build` 只会重新建立 keep-alive,从不结束一个。** 一个悄悄变成 false 的
`wantKeepAlive`,会一直被留着,直到有人明确调用 `updateKeepAlive` 说出来。回归行把这条钉死了。

**而 `_NullWidget` 整个类就是一个会抛异常的 `build`。** 混入方的 `build` 必须**被调用**、其返回值必须
**被忽略**;而「你忽略了吗」这件事没有任何办法检查——**除非把返回的那个值做成毒药。**

**宿主 `AutomaticKeepAlive` 的形状,就是它两半的不对称:**

* **开始留住一棵子树,是 out of turn 同步应用的**,必要时在 build 中途——因为另一种可能是,这一行在请
  求落地之前就已经被丢掉了。
* **停止留住,做不到这一点**:它需要一次 rebuild。上游拿 `schedulerPhase` 和 `persistentCallbacks` 比,
  build/layout 还没开始就 `setState`;已经过了,就只能等下一帧。上游自己在注释里管这叫「very
  unfortunate」,并且把代价写了出来:**这些资源要再过 16ms 才会被回收。**

另外还有两处小的:第一次 build 时孩子还不存在,于是应用父数据被推到帧末,而那个回调**第一件事是检查自
己还挂着没有**——中间过了一帧,这一行可能已经被滚走了。而句柄在宿主销毁之后被触发,报错信息直接点名原
因:**某个 widget 在离开时忘了触发它的句柄。**

---

**另一半是二维委托,而它和一维委托的真正区别只有一个:`TwoDimensionalChildDelegate extends
ChangeNotifier`。** `SliverChildDelegate` 不是可监听的,于是告诉 sliver「孩子变了」的唯一办法,是交给它
**一个新的委托**并让 `shouldRebuild` 返回 true。**二维的这个,可以直接说一声。**

* **上界断言的是 `>= -1` 而不是 `>= 0`,而这不是宽容:它们是「最大下标」**,于是 0 已经表示「有一个孩
  子」,**不往下走一格就没法表达「一个都没有」。**
* **上界可以是 null**,因为委托未必知道:散点图上面的孩子比下面多,**那条轴上根本没有一个数**。
* **setter 只在真的变了时才通知**——视口把从这个委托缓存的一切在每次通知时全部作废,一次多余的通知就是
  一次孩子的全量重建。
* **builder 委托的 `shouldRebuild` 恒返回 true,而它只能这样:闭包是不透明的,没有东西可比。** 而在便宜
  的那个方向上猜错,留下的是屏幕上的陈旧孩子。
* **list 委托比的是列表的「身份」而不是内容**,这正是上游文档反复强调「改了就必须换一个新的 list 对
  象」的原因——**一个被就地改过的列表还是同一个对象,委托会说什么都没变。** 这里用 `Rc::ptr_eq` 如实照
  搬,回归行两半都钉住了。
* **list 委托按 `children[yIndex][xIndex]` 取,y 在前**——和 `ChildVicinity` 命名它们的顺序正好相反。而
  **每行的长度是从那一行读的**,于是不齐的数组也能用。
* **包装顺序是 `AutomaticKeepAlive(_SelectionKeepAlive(RepaintBoundary(child)))`,keep-alive 在重绘边
  界外面。** 一行正被留住时并没有在画,外面的边界没有东西可隔离;而里面那层在这一行回来时照样管用。
* **builder 抛异常时给出的是一个错误 widget**:表格里一个坏掉的格子是一个坏掉的格子,不是一整块白屏。

验证:`cargo test --lib` 2826 绿,GN `rustflutter_unittests` 2826 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1683 accounted / 205 MISSING(89.1%)。

### 「往右」不是一个有唯一答案的问题(2026-08-20)

两个新模块,主题都是**在两个地方之间移动**:`heroes.rs`(`Hero`、`HeroMode`、`HeroController`)与
`directional_traversal.rs`(`DirectionalFocusTraversalPolicyMixin`、`DirectionalFocusIntent`、
`DirectionalFocusAction`)。覆盖率 1677/1888(88.8%)。

**Hero 的飞行其实不是「移动」。** 两侧的 hero 都被藏起来,飞的是 navigator overlay 里的第三份。所以这
个类的难点从来不在动画,而在**什么时候能量到终点的那个矩形**。

**`flightType` 那个三元 switch 的两条 case 读的是不同的路由,这不是笔误:** 手势永远是 pop(唯一驱动页
面转场的手势就是返回滑动);否则**正在倒放的是「旧」路由**说明是 pop,**正在正放的是「新」路由**说明是
push。而 pop 那条排在前面,于是同时满足两条的一对读作 pop。

**紧接着的两个提前返回各自看的也是不同的路由**,而上游把理由写在一行注释里:「A user gesture may have
already completed the pop, or we might be the initial route.」——pop 时若**旧**路由已经到 0,push 时若
**新**路由已经到 1,那要飞的那段路已经走完了。

**而 `flightType` 为 null 时代码并不 return。** 它照样往下走,照样排那趟帧末的活。**这不是形式:那一趟
正是既有飞行被结束的方式。** 回归行专门钉住了「没有新飞行起飞,但在飞的那个落了地」。

**「立刻量」和「等一帧」的分界是三个条件的合取**,而三个各有各的道理:必须是**手势驱动的 pop**(因为
下面那页本来就在)、下面那页 `maintainState`(否则它得重建)、而且它**真的被布局过**(直接塞进 pages
栈的路由可能从没量过尺寸)。回归行把三个条件各自单独打掉,确认每一个都真的能把它推到下一帧。

否则就是 `toRoute.offstage = toRoute.animation!.value == 0.0;` 加一个帧末回调,而**这一行的机制值得写
下来:把一个路由放到 offstage 会把它的动画值变成 1.0**。这才是「量终点尺寸」能成立的原因——不是等它动
完,而是**先把它挪到终点、量完再挪回来**。而只有还没开始现身的路由才这么办,已经露出一半的藏起来会闪。

**`_allHeroesFor` 的 else 分支是这个函数里最容易漏掉的一句:** 一个不被允许飞的 hero,仍然要收到
`endFlight`。**把一个 hero 排除在飞行之外,和不去管它,不是一回事**——之前那趟飞行可能已经把它藏起来
了,而没有别的东西会把它放回来。

**`HeroMode` 整个类就是 `Widget build(BuildContext context) => child;`。** 不画、不包、不改布局。它存
在只是为了在遍历子树时**被看见**。一个只能被找到的 widget,仍然是一个 widget。

---

**另一半是方向键。** next/previous 有唯一正确答案,因为 widget 是有顺序的;**「这个右边的那个」没有——
那是一道几何题,而答案得被挑出来。**

上游挑的规则是**带(band)**:一条无限长、宽度(或高度)等于当前焦点 widget 的条带,朝行进方向铺开。
碰到这条带的都是候选,其中最近的胜出。**于是「下面」的意思是「在这个 widget 下面」,而不只是「比它更靠
屏幕下方」**——回归行让一个正下方 200 像素的目标,赢过一个斜下方 25 像素的目标。

这一族里有四处是很容易写错的:

* **相交是严格的。** 恰好贴着带边的 widget 在带外;差一个像素的重叠就在带内。两半都钉住了。
* **过滤拿候选的「中心」比焦点的「边」。** 于是一个和焦点重叠的 widget,只要中心过了那条边就算在前
  方;而一个明明伸到焦点下方、中心却还在上面的,不算。
* **`node.rect != target` 是按几何排除,不是按身份。** **两个矩形完全重合的 widget,彼此永远够不到。**
* **带外的回退按「最近的那条边」排序,不是按中心。** 一个伸向带的宽 widget,赢过一个中心更近的窄
  widget——这才是「哪个更靠近这条带」的意思。

**而 scrollable 是偏好不是围栏:** 同一个滚动视图内的候选优先,**但只在这个筛选还剩下东西的时候**。否
则列表的最后一行就成了死路。

**这个 mixin 唯一持有的状态是一摞历史,而它存在只为一件事:迟滞。** 带搜索**不对称**——从一个窄输入框
往下落到一个宽按钮上,再往上时,那个按钮的带盖住的可能是另一个输入框。**没有记忆,右-再-左会把读者留
在他从没去过的地方。** 回归行先单独证明了这个不对称(不带历史时,回去的确落在别处),否则后面那些测试
什么都没证明。

**而弹栈那段有两处刻意的不对齐:守卫看的是 `history.first`(这条路是朝哪个方向起的头),弹出的却是
`history.last`。** 整摞历史属于同一段行程,所以要问的是:这一步是**接着走**、**往回退**、还是**拐上另
一条轴**。而**往回退和拐弯不是一回事**:往回退是沿路退一步,拐弯是把整条路扔掉——**一条朝下走出来的
路,说不出左边在哪。**

**被卸载的节点会让整摞历史作废**,而上游在注释里承认了这个代价:「This has the side effect that
hysteresis might not be avoided when items that go off screen get unmounted.」——**恰好落在最需要迟滞
的那个场景(长列表)上。**

**退回去时的 alignment 是按「你正从哪边来」定的**:往上/往左退,把目标顶到**起始**边;往下/往右退,顶
到**结束**边。这样读者退回去的那个节点,出现在他视线正来的那一侧。

最后,**`DirectionalFocusIntent.ignoreTextFields` 和 `DirectionalFocusAction.forTextField()` 是一处刻
意的分工:意图说「在输入框里可以不理我」,而 action 知道自己在不在输入框里。** 两边单独都不够——一个按
键绑定不知道自己会在哪里被调用,而一个 action 不知道这个绑定是什么意思。默认 `true`:输入框里的方向
键,移动的是光标而不是焦点。

验证:`cargo test --lib` 2789 绿,GN `rustflutter_unittests` 2789 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1677 accounted / 211 MISSING(88.8%)。

### 顶部栏并不在页面上方,所以冒泡永远到不了它(2026-08-20)

新模块 `scroll_plumbing.rs`,一次收掉六个文件八个类:`ScrollBehavior`、`ScrollConfiguration`、
`TrackingScrollController`、`ViewportElementMixin`、`ScrollNotificationObserver`、
`ScrollNotificationObserverState`、`ScrollMetricsNotification`、`PrimaryScrollController`。覆盖率
1671/1888(88.5%)。

**贯穿这一组的是两个问题。**

**「这是哪一种滚动?」** 由 `ScrollBehavior` 回答——一捆平台判断(物理、滚动条、辉光、哪些输入设备可
以拖)顺着树传下去,好让整个应用滚起来是一致的,而不必挨个告诉每个滚动视图。

**「谁在听?」** 被刻意回答了两次:通知沿树往上冒,任何祖先都能接住;而 `ScrollNotificationObserver`
持有一份**扁平的**监听器名单,那些监听器根本不必是祖先。**一个在页面滚动时抬起阴影的顶部栏,并不在
那个页面的上方——冒泡永远到不了它。**

**`ScrollBehavior` 的每个方法都是一次平台 switch,而每个方法都带着同一句注释**:「when modifying this
function, consider modifying the implementation in the Material and Cupertino subclasses as well」。这
句重复的话本身就是对形状的说明:**基类是一个完整的答案,而设计库是覆写而不是扩展**,于是每一份都得靠
人手保持同步。

* **滚动条只在桌面上建。** 触摸平台的滚动条是滚动时短暂画出来的东西,不是控件;在手机上常驻会为一个
  谁都抓不住的东西占掉一条屏幕。而它在那些平台上**断言 controller 存在**——滚动条要读一个位置,没有位
  置就没有可画的东西。
* **辉光正好去了拉伸没去的那些平台。** 不给辉光的,恰是物理本身已经用拉伸表现越界的那些。**两个一起
  做等于把同一件事说两遍。**
* **苹果平台按最外侧手指取平均,其余平台跟最新那根。** 区别在滚动途中第二根手指落下时显出来:iOS 上
  内容继续平滑移动,别处会跳到新手指那里。
* **三种物理都包在 `RangeMaintainingScrollPhysics` 外面**,而那是调用方从不主动要、却总是想要的一
  层:**内容变尺寸时保住读者的位置,不是平台偏好,而是到处都对的事。**
* **`shouldNotify` 默认 false。** `ScrollConfiguration` 会随上面任何东西一起重建,而它的 behavior 通
  常是每次都一模一样的 const 对象;默认通知会让整个应用的每个滚动视图,在任何触及祖先的帧上都重建一
  次。

**`PrimaryScrollController.shouldInherit` 那两个条件都重要:**

* **平台检查**是桌面列表不会悄悄挂上去的原因:桌面滚动视图有滚动条,而滚动条需要一个自己的
  controller——一个被隐式共享的,会去驱动最后挂上的那个。
* **轴向检查**是竖直页面里的横向轮播不会抢走页面 controller 的原因。**一个 primary controller 是给一
  个方向的,而轮播不往那个方向去。**

**`TrackingScrollController` 的 detach 里那两处清理是不一样的,而这正是它存在的意义:** **位置**一
detach 就忘掉(一个指向已经没了的位置的引用,比没有引用更糟);而**偏移量留到最后一个位置也 detach
才丢**。这恰好就是 tab 视图那个场景:列表滚走了,而下一个到来的列表仍然从它原来的位置开始——**这是
「切换标签页」和「丢失阅读位置」之间的区别。**

**`ScrollMetricsNotification` 和 `ScrollUpdateNotification` 的区别就是这个类的全部:** 前者在**内容或
视口尺寸**变了、而偏移没动时触发(列表变长了、键盘弹出来了)。画滚动条的监听者两个都要;数「读者滚了
多远」的只要后者。

而 **depth 是监听者分辨「我自己的滚动」和「我里面某个嵌套滚动」的手段**——嵌套滚动视图的内层列表会穿
过外层冒上来,对每一条都动作的监听者会动作两次。`ViewportElementMixin` 的 `onNotification` 返回
**false**:通知被修改后继续传,从不被消费——**视口是途经点,不是终点。**

**观察者派发时那条「还在不在名单上」的检查值得单独说:光迭代一份拷贝是不够的。** 一个在派发过程中把
另一个监听器摘掉的监听器,否则会让那个已经被告知「你已经没了」的监听器仍然被调用一次。回归行专门构造
了这个情形。

验证:`cargo test --lib` 2743 绿,GN `rustflutter_unittests` 2743 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1671 accounted / 217 MISSING(88.5%)。

### 一个类一个文件,通常意味着一个被写下来的判断(2026-08-20)

新模块 `small_widgets.rs`,一次收掉六个单类文件:`ImageFiltered`、`GridPaper`、
`KeyboardListener`、`NavigationToolbar`、`SharedAppData`、`SpellCheckConfiguration`。覆盖率
1663/1888(88.1%)。

它们没有共同的主题。**它们共同的是「每个文件一个类」——而那通常意味着这个类就是一个被写下来的判
断。** 六个各有一条值得点名:

**`ImageFiltered.enabled` 存在的理由,上游写在文档里:「prefer setting enabled to `false` instead of
creating a no-op filter」。** 一个空操作的滤镜不是免费的——孩子照样被光栅化进一个图层、照样过一遍滤
镜,而**半径为零的模糊,代价几乎等于半径为十的模糊**。`enabled: false` 整个跳过图层,这正是一个要把
滤镜动画**进来**的调用方,在动画开始前每一帧都想要的。

**`GridPaper` 对两个计数都断言大于零**,而报错信息各自说明了理由:「if there were no divisions, the
grid paper would not paint anything」。**零被拒绝,而不是被当成「只画主线」**——写了零的调用方是有意图
的,而回给他一张空白覆盖层,不会告诉他两个参数里错的是哪个。而**两个计数是相乘不是相加**,这才让默认
值(100 / 2 / 5)给出十像素的最细网格而不是十四像素。默认颜色是半透明的,因为**网格是用来对着量的,
一张透不过去的网格是在量它自己**。

**`KeyboardListener` 是 `Focus` 的朴素版,而区别在于它**不**做什么:** 没有遍历、没有快捷键、没有
action。想要「我这棵子树有焦点时按键就跑这个」而不要别的调用方,不必再去关掉一套自己没要过的遍历策
略。而**没有回调时它是透明的而不是一个按键掉进去的洞**;`autofocus` 默认关(构建时抢焦点会从读者正在
用的东西那里抢走);`includeSemantics` 默认开(**能接按键的东西,就是键盘用户够得到的东西**)。

**`NavigationToolbar` 存在是因为 `Row` 会把这件事做错。** `Row` 里居中的标题是**在剩下的空间里**居
中,于是它会随着前导或尾部 widget 宽度变化而移动——**一个返回箭头出现就把标题挪走了**。这个类把中间那
块对着**整条工具栏**布局,只在放不下时才让步。回归行钉住了「前导出现,标题不动」和「真挤不下时,它只
挪它必须挪的那么多」。

**`SharedAppData` 的两个静态方法是一处刻意的对比:`getValue` 建立依赖,`setValue` 不建立。** 上游文档
直说「unlike `SharedAppData.getValue`, this method does _not_ create a dependency」。**一个写值的
widget 不该被自己的写触发重建——它已经知道了。** 而依赖是**按 key** 建立的(走
`InheritedModel.inheritFrom` 把 key 当 aspect),于是读 `foo` 的 widget 不会因为 `bar` 变了而重建。

它的文档还异常小心地说明了它**不是**什么:「not intended to be a substitute for Provider or any of the
other general purpose application state systems」。它存在,是为了让一个包能发布共享一两个值的
widget,而**不必要求开发者往应用里加一个这个包专用的伞状 widget**——`WidgetsApp` 自动创建一个,所以它
总是在那儿。

**`SpellCheckConfiguration.copyWith` 的第一行最值得写下来:一个被禁用的配置,拷不出一个启用的。**

```dart
if (!_spellCheckEnabled) {
  return const SpellCheckConfiguration.disabled();
}
```

调用方传的每个字段都被丢掉。这比看上去更严格,而它是对的:**一个主题交给字段一份禁用配置,说的是「这
里拼写检查是关的」,而一个想加个拼写错误样式的调用方,不该顺手把它打开。** 而 `disabled` 是一个**独立
构造函数**而不是一个 `enabled: false` 参数——正是这条 `copyWith` 规则说明了为什么:**禁用不是调用方能
翻的一个字段,它是一种配置。**

验证:`cargo test --lib` 2722 绿,GN `rustflutter_unittests` 2722 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1663 accounted / 225 MISSING(88.1%)。

### 两个 Text 不带 key,什么动画都不会发生(2026-08-20)

新模块 `crossfade.rs`,一次收掉五个文件五个类:`AnimatedCrossFade`、`AnimatedSwitcher`、
`FadeInImage`、`Icon`、`ImageIcon`(外加 `CrossFadeState`)。覆盖率 1657/1888(87.8%)。

**前三个做的是同一件事,区别在于它们知道多少。** 交叉淡入被告知**两个**孩子和该显示哪一个;切换器只
被告知**一个**孩子,得自己判断这是不是一个新的;而淡入图片是一个切换器,它的两个孩子是占位图和真图,
而它知道第二个什么时候到。

**反复出现的判断是「正在离场的孩子会怎样」,而三者的答案一致:它继续被画出来,而不再是别的任何东
西。** 不接点击、不被屏幕阅读器念、通常也停止动画。**一个正在看淡出的读者,不该点得到一个已经消失了
一半的按钮,更不该听见它和替换它的那个被一起念出来。**

**`AnimatedSwitcher` 那个坑值得单独写:两个不带 key 的 `Text`,什么动画都不会发生。** 切换只在新孩子
**无法更新**旧孩子时才发生——运行时类型不同,或者 key 不同。两个只有字符串不同的 `Text` 可以互相更
新,于是文字直接变了,没有过渡。**这件事每个人都会被坑一次**,上游文档为此写了很长一段。解法是加一个
key,而这正是外面几乎每个例子都带 key 的原因。回归行把四种情形都钉住了。

**`AnimatedCrossFade` 的上下两层是刻意不对称的:**

* **底下那层永远忽略指针、永远排除语义。** 上游注释直说「always exclude the semantics of the widget
  that's fading out」——屏幕阅读器绝不会同时念出同一个东西的两个版本。
* **底下那层的 ticker 只在淡入淡出**进行中**才开。** 一个已经稳定下来的交叉淡入,不该为一棵看不见的
  子树的动画一直付钱。
* **上面那层则全开**:ticker 无条件开着,语义无条件发布——正在到来的那个,正是读者马上要去操作的那
  个。
* **焦点是调用方唯一能控制的那一项**,而默认是排除的。

而**它总是把结果包进一个 `AnimatedSize`**,这是它存在的大半理由:两个尺寸不同的孩子直接交叉淡入而不
动画尺寸,会让周围的布局在第一帧就跳一下。

**`FadeInImage` 的两个时长是刻意不相等的,而这个比例就是要点:占位图 300ms 走,真图 700ms 来。** 一
次对称的交叉淡入会让两者在中间都是半强度,而一张照片压在灰色方块上的半强度读起来是一片糊。**让占位
图先走、图片慢慢来,读者看到的是图片显影,而不是两张画面重叠。** 回归行钉住了「两段淡入淡出是先后而
不是同时」。

而 `FadeInImage.memoryNetwork` / `.assetNetwork` 存在的理由只有一条:**占位图本身绝不能是网络图
片**,否则这个 widget 要等两次下载才能给读者看到任何东西。

**图标那一对:** `Icon` 是字体字形而不是图片,所以它有 `size` 和 `color` 而没有 `fit`——**它像文字一样
缩放,因为它就是文字**,字体在任何尺寸下都不糊。而 `ImageIcon` 存在是为了字体装不下的图标(多色
logo、头像),它**走同一套主题解析**,好跟旁边的字体图标对齐。

* **不给尺寸就从 `IconTheme` 继承**,这是一整条工具栏的图标能靠上面一行改变的原因。
* **`fill` 和 `weight` 是有真实范围的可变字体轴而不是自由数字**,上游为此断言。
* **图标默认没有语义标签**,这是对的:大多数图标旁边就有一个已经说明了它是什么的标签,两个都念会说两
  遍。

验证:`cargo test --lib` 2704 绿,GN `rustflutter_unittests` 2704 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1657 accounted / 231 MISSING(87.8%)。

### 「隐藏」不是一个状态,而是一架阶梯(2026-08-20)

新模块 `presence.rs`,一次收掉五个文件九个类:`Visibility`、`SliverVisibility`、
`ExpansibleController`、`Expansible`、`OrientationBuilder`、`DeviceOrientationBuilder`、`Title`、
`StatusTransitionWidget`(外加 `Orientation`)。覆盖率 1652/1888(87.5%),widgets 层 81%。

**把它们串起来的问题是:一个 widget 被隐藏之后,还剩下多少。** 「不可见」不是一个状态——被隐藏的东西
可以保留、也可以不保留它的 `State`,可以继续、也可以不继续动画,可以占、也可以不占位置,可以被、也
可以不被屏幕阅读器念出来,可以接、也可以不接点击和焦点。`Visibility` 让这六件事**分别可选**,然后用
五条断言把它们约束成一架阶梯:

```
maintainState  <--  maintainAnimation  <--  maintainSize  <--  maintainSemantics
      ^                                            ^
      |                                            +-------  maintainInteractivity
 maintainFocusability
```

箭头读作「需要」。**保不住一个没在动画的东西的尺寸**——保尺寸意味着保持布局,而一棵 ticker 关掉的已布
局子树,是一棵冻在动画中间的子树。**念不出一个不占空间的东西**——屏幕阅读器是按几何导航的。**点不到一
个没有面积的东西。** 而**可聚焦性挂在 state 上而不是走尺寸那条链**:一个可聚焦的东西需要存在,不需要
占地方。

**默认值是最会让人意外的那个:一个普通的 `Visibility` 什么都不保。** 翻转 `visible` 会销毁并重建整棵
子树,里面的滚动位置、打了一半的输入框都没了。上游提供 `Visibility.maintain` 这个「全开」构造函数,
正是因为整架阶梯才是「还在,只是没画出来」的常见需求——而每次手写六个布尔值,恰好是踩中那五条断言的
方式。

**`Visibility.of` 会走**每一个**祖先作用域,而不是停在最近那个。** 一个孩子只有在它上面每一个
`Visibility` 都说可见时才可见,这是唯一正确的答案:一个不可见的祖先会盖住它下面的一切,不管别人多么
声称自己可见。

**两个方向构建器回答的是不同的问题,而混淆它们是真实的 bug:** `OrientationBuilder` 读的是**传进来的
约束**(宽大于高就是横向),`DeviceOrientationBuilder` 读的是**设备**。一个占了横屏平板三分之一的侧
边栏,约束是竖向的、设备是横向的——问「我这个盒子宽不宽」的调用方要前者,问「该不该显示平板布局」的
要后者。而**正方形算竖向**,因为比较是严格大于:总得有人打破平局,而一个在正方形里假设横向的布局,会
没地方放它那一行。

**`Title` 断言颜色必须完全不透明。** 它不是 Flutter 画的颜色——它交给操作系统去画任务切换器的卡片,而
系统会拿它跟任意背景合成。半透明的那个出来会是一个谁都没选过的颜色。而**默认标题是空串而不是应用
名**:框架不知道这个应用叫什么,编一个出来等于在切换器里放了个错名字。

**`StatusTransitionWidget` 听的是动画的**状态**而不是它的**值**,而这就是这个类的全部。** 值监听器一
秒钟触发六十次;状态监听器在一次动画的一生里触发四次——dismissed、forward、completed、reverse。**一个
只需要知道「有没有在跑」而不需要知道「跑到哪了」的 widget,付第二种代价而不是第一种。**

**其余两条:**

* **`ExpansibleController.expand` 是终态而不是切换**——已经展开时调用它没有效果,也不通知任何人。而
  `Expansible` 本身是**刻意没有样式的**:上游把它从 `ExpansionTile` 里抽出来,好让 Material 和
  Cupertino 共用这套机制而不共用外观。
* **`SliverVisibility` 是单独一个类而不是一个标志**,因为替代物的种类不同:隐藏的盒子塌成零尺寸的
  盒子,隐藏的 sliver 塌成零延展的 sliver,而没有哪个 widget 同时是这两样。

验证:`cargo test --lib` 2683 绿,GN `rustflutter_unittests` 2683 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1652 accounted / 236 MISSING(87.5%)。

### 子树能接返回时,这层反而要说自己不能弹(2026-08-20)

一次收掉四个文件六个类:`ModalBarrier`、`AnimatedModalBarrier`(新模块 `modal_barrier.rs`)、
`PageRoute`、`PageRouteBuilder`(进 `routes.rs`)、`NavigatorPopHandler`(进 `navigator.rs`)、
`ContextMenuController`(进 `overlay.rs`)。覆盖率 1644/1888(87.1%)。

**`NavigatorPopHandler` 整个 widget 就是一次取反,而它读起来是反的,直到把它点出来:**

```dart
canPop: !widget.enabled || _canPop,
// 而通知那边:
final bool nextCanPop = !notification.canHandlePop;
```

**当子树说它**能**处理一次弹出时,这层作用域报告自己**不能**弹。** 在这里拒绝,正是阻止外层导航器接
走这次按下的办法——从而让嵌套的那个有机会接。说「能弹」会把整条路由弹掉,把嵌套导航器的整段历史一起
带走。

而它的 `onPop` **只在弹出被拒绝时才触发**:那次拒绝就是这个 widget 在说「嵌套导航器会接」,而回调正
是调用方让它去接的地方。通知监听器**返回 false**——通知继续往上冒,因为更外面的处理器对它们之间那个
导航器有同样的判断要做。

**遮罩那两条配置是分开的,而且不对称:** 挡住点击是它的本职,能不能点掉是额外的;而
`barrierSemanticsDismissible` 在 `dismissible` 为假时**被忽略**。所以一个不能被点掉的遮罩永远不会被
当作「可关闭」提供给屏幕阅读器;而一个能被点掉的仍然可以被扣下——路由在「有更好的出口、应该把读者引
过去」时就这么做。

**而没有颜色的遮罩是常态而不是退化情形:** 菜单的遮罩就是这样——它要接住那次关闭菜单的点击,而为一个
菜单把整页压暗太重了。

**`PageRoute` 那两条转场规则是一对,说的是同一件事:一个页面路由只和另一个页面路由一起动。** 对话框
出现在页面之上,不该让页面滑动;页面到达在对话框之下,不该让对话框移动。**不是一整屏内容的东西,不
属于同一次运动。**

而**全屏对话框不能被返回滑动关掉**,上游注释直说。它从底部上来,没有一条起手的边缘是有意义的——而对
话框通常是一个想要答案的问题,不是一页可以随手退出去的内容。

**`PageRouteBuilder` 的默认值才是它有意思的地方,因为那是调用方什么都不说时拿到的答案:** 默认转场
**原样返回孩子**——不淡入、不滑动,就是出现。对一个专为一次性路由而生的类,这是刻意的:想要转场的调
用方会说清楚要哪种。而它的遮罩默认**不可点掉**,和对话框相反:**一个页面铺满屏幕,没有「外面」可
点。**

**`ContextMenuController` 的「同时只能有一个」是用静态状态强制的**,上游注释说得很直白:「only one
context menu can be displayed at one time」。两个就是对一次右键给了两个答案。而它进的是**根**
overlay 而不是最近的那个,于是从对话框里唤起的菜单不会被对话框裁掉。

它的两个移除方法都存在,理由很实在:**`show` 一个已经显示的菜单会就地重建而不是拆了重放**(这才让粘
贴按钮能在剪贴板回答时出现,而菜单不会闪一下);而**实例的 `remove` 在别人的菜单当前显示时什么都不
做**——一个正在销毁的 widget 应该带走**自己**的菜单,而且只带走自己的。

**过程记录:** 这轮的批量插入脚本被 shell 的引号规则坑了两次(heredoc 里的三引号,以及后续修补时的
CRLF 与转义)。第二次之后改用 Edit 工具直接改那一行,一次就过。**当一段脚本要被 shell、Python 和
Rust 三层引用规则同时穿过时,把它写成文件再用编辑工具改,比继续叠转义可靠。**

验证:`cargo test --lib` 2662 绿,GN `rustflutter_unittests` 2662 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1644 accounted / 244 MISSING(87.1%)。

### 作用域记的是一摞,不是最后一个(2026-08-20)

新模块 `focus_node.rs`,`widgets/focus_manager.dart` 收口:`FocusNode`、`FocusScopeNode`
(外加 `UnfocusDisposition`)。覆盖率 1638/1888(86.8%),widgets 层 79%。

**焦点是一棵树,而树里恰好有一个节点持有主焦点。** 那个节点的每一个祖先也「有焦点」——这就是整个文
件的核心区分:**`hasFocus` 是「在这条链上的某处」,`hasPrimaryFocus` 是「在链的末端」。** 一个文本
框持有主焦点;包着它的表单、再包着的页面都有焦点,而两者都想知道。

**另一半是作用域,而它记的是一摞曾经持有焦点的孩子,不只是最后一个。** 这样当被聚焦的孩子被移除时,
焦点回到它之前那个,而不是回到虚无。`unfocus` 里几乎每一行看起来别扭的代码,都是在对抗这摞东西变
陈旧的各种方式。

**两种 unfocus 的区别正是这摞东西的用法:**

* **`scope`(默认)会把历史清空。** 上游注释说得很清楚:清空能防止「刚 unfocus 完就按 next,又把刚
  才那个重新聚焦回来」,也防止「unfocus 被调用两次时选中倒数第二个」。清空之后再按 next,会由遍历策
  略挑它认为该排第一的那个——这对「我用完这个字段了」是对的,对「暂时把焦点拿走」是错的,而后者正是
  另一种 disposition 的用途。
* **`previouslyFocusedChild` 会往上走到最近的可聚焦作用域,再往下沿着每层的 focusedChild 走到叶子。**
  而**往上走的路上,每个不可聚焦的作用域也会被从它父作用域的历史里删掉**——否则焦点会回到一个根本接
  不住它的作用域。

**`canRequestFocus` 是一条合取:自己说行,而且**每一个**祖先都允许后代被聚焦。** 上面任何一处
`descendantsAreFocusable: false` 就关掉了整棵子树,这正是一个禁用面板不必碰里面任何一个控件就能禁用
它们的原因。

**而那两个 setter 的顺序是上游自己注释过的:先设标志,再 unfocus。** 因为 unfocus 在剔除「不可聚焦
的、曾被聚焦过的孩子」时会去读这个标志;反过来写,会让这个节点在那次本该跳过它的遍历里看起来仍然是
可聚焦的。**而重新打开时不会把焦点抢回来**——一个自我禁用又启用的面板不该偷回焦点。

**几处「不是想当然」的地方:**

* **不可聚焦的作用域提供的遍历孩子是空的**,不是「孩子里去掉不可聚焦的那些」。一扇关着的门应该被绕
  过,而不是走进去。
* **`skipTraversal` 的节点仍然可聚焦**,只是不被 tab 到——点得到,tab 不到。
* **陈旧的 focusedChild 只在作用域下次被要求聚焦时才清理**,这是唯一发生清理的地方:一个变得不可聚焦
  的孩子会一直挂在名单上,直到有人来问。
* **`autofocus` 只在作用域还没有 focusedChild 时才动手。** 要点是不打架:两个都 autofocus 的 widget
  应该让第一个拿到焦点,而谁都不该从已经做出选择的读者手里抢走。
* **`setFirstFocus` 分两种情况**:作用域**有**焦点时立刻聚焦那个孩子;**没有**焦点时只记下来。给一个
  没人在的作用域设首焦点,不该把焦点拉进去。
* **键盘令牌区分的是「默认被聚焦」和「被用户明确选中」。** 节点请求焦点时拿到一个令牌,而管理文本输
  入的 widget 只在能**消费掉**令牌时才弹键盘——于是一个 autofocus 第一个字段的表单,不会在读者还没表
  示要打字之前就把键盘糊上半个屏幕。
* **一个还不在树里的节点,会把焦点请求推迟到下次被接进树时**,一次。这正是 widget 能在 `initState`
  里调 `requestFocus` 的原因。

**自查:尺子又抓到一个命名问题,和上一轮同样的性质。** 我把 `FocusScopeNode` 建模成了
`FocusNode::scope()`——一个布尔字段,而不是一个名字。尺子按名字匹配,读者也按名字搜。补上一个
`FocusScopeNode` 类型句柄,并写明为什么它是句柄而不是独立对象:**作用域**就是**一个 `FocusNode`,上
游是继承。句柄买到的是「作用域专有的操作会说自己是专有的」——`focusedChild`、`setFirstFocus`、
`autofocus` 对一个普通节点毫无意义,而「去要一个句柄」正是检查这件事的地方。

验证:`cargo test --lib` 2637 绿,GN `rustflutter_unittests` 2637 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1638 accounted / 250 MISSING(86.8%)。

### 表单是一份登记表,而它花力气最多的问题是「什么时候校验」(2026-08-20)

两个新模块:`form.rs`(`FormState`、`FormFieldState`)和 `layout_builder.rs`
(`ConstrainedLayoutBuilder`、`RenderAbstractLayoutBuilderMixin`)。覆盖率 1636/1888(86.7%)。

**表单是一份登记表,不是一个容器。** 字段通过 context 找到它、把自己登记上去;表单从不去遍历自己的
子树找字段。这就是一个字段能藏在任意深的布局后面、仍然跟其余字段一起被保存和校验的原因。

**这个文件花力气最多的问题是「什么时候校验」。** 每次按键都校验,等于告诉读者他打了一半的邮箱地址是
错的;只在提交时校验,又让他一次性面对五个错误。五个 `AutovalidateMode` 是五个答案,而最有用的那两
个——`onUserInteraction` 和 `onUserInteractionIfError`——都说「**在他做了什么之前不要**」,区别只在于
一个已经出错的字段,是不是在他修的过程中继续被检查。

**上游那条按字段的守卫是一条恒真式,原样按它计算的意思移植,并写下来:**

```dart
if (!validateOnFocusChange || !hasFocus || (validateOnFocusChange && hasFocus))
```

读成 `!A || !B || (A && B)`——只有 `A && B && !(A && B)` 时才为假,而那不可能。**每个字段都会被校
验,不管它有没有焦点。** 照抄原文会让读者以为这里有一条焦点规则,而并没有。回归行直接钉住「有焦点的
那个也在失败列表里」。

**其余几条判断:**

* **只有第一条错误会被念给屏幕阅读器**,上游注释直说。一次念四条失败,比一条能立刻处理的失败告诉读者
  的更少。
* **`validate()` 会先把 `_hasInteractedByUser` 置为 true**,这对一次程序化调用读起来很怪——没人交互
  过。但它买到的东西是:一次显式校验之后,`onUserInteraction` 会在读者修改的过程中继续检查,而那时这
  才是有帮助的。
* **`forceErrorText` 会完全短路校验器。** 服务器说「这个用户名已被占用」,不是客户端校验器能检查或推
  翻的事,所以它根本不会被问到。回归行用一个「被调用就 panic」的校验器把这条钉死。
* **`isValid` 是被动的**:上游文档强调它「不会设置 errorText 或 hasError,也不会更新错误显示」——给那
  种想在读者还在打字时就点亮提交按钮、又不想把表单弄红的调用方用。
* **`_fieldDidChange` 里那个「交互过」标志是从字段重新算出来的,而不是拴住的。** 这正是 `reset` 能工
  作的原因:字段全被重置的表单会退回「没交互过」,而拴住的标志做不到这一点。
* **`setValue` 不算「读者做了什么」**:widget 在构建期间自己算出来的值,不该让表单以为有人动过。
* **保存不是提交**:`save` 把值交给 `onSaved`,什么都不改变。表单是在告诉调用方它手里有什么,而不是
  把它放到哪儿去。

**布局构建器那半边,所有别扭之处都来自一次倒置:孩子是在父级的 `performLayout` **里面**构建的。** 在
布局期间构建意味着在布局期间把东西标脏,而框架平时禁止这件事;于是重建走的是布局回调而不是普通的
build scope,而**安排一次重建必须小心它发生在什么时候**。

* **在 idle 和 postFrameCallbacks 期间,请求会被推迟到下一帧开始。** 上游的理由是:「the render tree
  should typically be kept clean during the postFrameCallbacks and the idle phase, so the layout
  data can be safely read」——帧与帧之间把渲染树弄脏,会让任何正在读它的人(测试、正在算命中的手势竞技
  场、检查器)读到一棵改到一半的树。而**已经推迟过就直接返回**:帧间的一串请求只该花下一帧开头的一次
  布局,不是每个一次。
* **回调没变就不重新安排布局。** 这个 widget 的父级每次重建它都会跟着重建,为一个一模一样的回调重新
  安排,会让每次祖先重建都付一次布局的代价。
* **`updateShouldRebuild` 默认是 true**,因为没有办法比较两个闭包。知道得更清楚的子类去覆写它——那也
  是唯一能阻止布局构建器在父级每次重建时都重建整棵子树的办法。

验证:`cargo test --lib` 2605 绿,GN `rustflutter_unittests` 2605 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1636 accounted / 252 MISSING(86.7%)。

### 空集合的意思是「全部」,而问过全部之后就收不回来了(2026-08-20)

新模块 `inherited.rs`,一次收掉五个文件的七个类:`InheritedModel`、`InheritedModelElement`、
`InheritedNotifier`、`InheritedTheme`、`CapturedThemes`、`LookupBoundary`、
`LayoutChangedNotification`。覆盖率 1632/1888(86.4%)。

**一个普通的 `InheritedWidget` 提供的是一笔交易:依赖我,我变的时候重建你。** 这几个 widget 各改这笔
交易的一项:

* **`InheritedModel` 收窄「什么算变化」**——问了某个字段的 widget,不会因为另一个字段动了而重建。
* **`InheritedNotifier` 放宽「什么时候发生」**——widget 持有的那个值发出通知时就重建,而不只是 widget
  本身被替换时。
* **`InheritedTheme` 把答案拷一份走**——一棵渲染在别处的子树,仍然看见它被构建时所处的那些主题。
* **`LookupBoundary` 让查找停下**——widget 不能越过它作者画的那道框往外够。

**model 那套两个方法的分工是它的全部设计:** `updateShouldNotify` 回答「有没有东西变了」,问一次;
`updateShouldNotifyDependent` **对每个依赖者各问一次**,带着那个依赖者点过名的 aspect 集合,回答「它
问过的东西变了没有」。十个字段一百个依赖者的 model,于是只重建那些字段真的动了的。

**而 `updateDependencies` 的第一行是最容易看漏的一条:**

```dart
if (dependencies != null && dependencies.isEmpty) return;
```

**空集合的意思是「全部」,而一个依赖者一旦问过全部,再点名一个 aspect 也收不回来了。** 一个先不带
aspect 调 `inheritFrom`、后来又带 aspect 调的 widget,仍然是在要求听到每一次变化;第二次调用不该悄悄
把这个拿走。回归行把这个方向和反方向(先点名、后不点名 → 放宽回全部)分别钉住。

**还有一处相差一个字符的区分:「没有记录」的依赖者根本不通知,「记录为空集」的依赖者永远通知。**

**`_findModels` 会为那些答不上来的 model 也建立依赖**,而不只是最后那个能答的。理由是:它们中的任何
一个都可能在后续构建里**开始**支持这个 aspect,而一个只对远处那个注册过的依赖者永远不会知道。

**`isSupportedAspect` 是「只遮蔽一部分」的手段:** 一个对某个 aspect 答 false 的 model,会让查找就那
一个 aspect 继续往上走,同时仍然为其余的作答。一个只覆盖颜色、不覆盖字体的主题正是这样。

**notifier 那边有两处克制:** 监听器**只在 notifier 真的换了时才搬家**(每次重建都摘了再挂,是每帧
为零变化付的功夫);而通知是**在 build 里发的,不是通知到达的那一刻**——于是一帧之内的一串通知合并
成对依赖者的一次重建。**null 的 notifier 什么都不发**,因为一个 null 对象没法自己发通知。

**主题捕获有两条规则:** `from == to` 捕获**空**(它们之间没有跨度,空才是诚实的答案);而**每种类型
只留第一个**——上游的注释是「inherited themes completely shadow ancestors of the same type」,留两个
会把孩子包两层,外面那层永远看不到。而 `CapturedThemes` 是**冻住的**:原来那些主题后来怎么变,包住
的孩子都看不见,除非重新捕获一次。一条从主题化子树里推出去的路由正需要这个——它渲染在 overlay 里,
离它被创建的地方很远。

**`LookupBoundary.dependOnInheritedWidgetOfExactType` 做的第一件事,没人会猜到:它无条件先依赖那个
boundary 自己,哪怕这次查找什么都没找到。** 上游的注释写明了为什么——否则一个没找到东西的 widget 会
**一个依赖都没有**,而把它挪到一个答案确实存在的地方,永远不会有人告诉它。

而 `findAncestorWidgetOfExactType` 的访问者返回的是 `runtimeType != LookupBoundary`,于是
**boundary 是最后一个被访问的元素,而不是第一个被跳过的**——查找 boundary 类型本身是找得到的。

**`debugIsHidingAncestorWidgetOfExactType` 只在 debug 下存在,而它是为报错信息活着的:** 「没找到
Material」远不如「有一个,但被一个 LookupBoundary 挡住了」有用,而后面那句只有在有人去看的时候才说
得出来。

**`LayoutChangedNotification` 什么都不带,这是设计:** 听众被告知的是「下面有东西改了尺寸」,不是改
了什么、改了多少。更具体的内容会是发送方给不出的承诺——这条通知会冒泡穿过一堆对那次布局一无所知的
widget。

验证:`cargo test --lib` 2574 绿,GN `rustflutter_unittests` 2574 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1632 accounted / 256 MISSING(86.4%)。

### 单元格被摊平之后,行就不存在了(2026-08-20)

两个新模块:`table.rs`(`TableRow`、`Table`、`TableCell`)和 `snapshot_widget.rs`
(`SnapshotController`、`SnapshotWidget`、`SnapshotPainter`)。覆盖率 1625/1888(86.1%)。

**表格是按行写的,按「一整条扁平的单元格列表」排布的。** 它所有的规则都出自这次转换:构造函数里五
条断言,加上一套行匹配算法——后者要在摊平已经把行丢掉之后,重新拼回「哪个单元格属于哪一行」。

**值得读两遍的是最后那条断言:单元格的 key 必须在整张表里唯一,而不只是在一行里。** 因为等到匹配
key 的时候,行已经不存在了——**两个在不同行里带着同一个 key 的单元格,从匹配器站的位置看,就是同一
个单元格出现了两次。**

**行匹配的设计是「带 key 的和不带 key 的从两个不同的池子里配」:** 带 key 的行按 key 去找,不管它挪
到哪儿;不带 key 的行**只和旧的不带 key 的行按位置对齐**。于是**在顶上插入一个带 key 的行,不会把每
个不带 key 的行的身份都挪一位**。回归行专门钉了这一条。

**其余几条表格上的判断:**

* **`TableRow` 不是 widget**——没有 build,没有 element,从不进入树。它只是一种写下一串单元格的方
  式,表格立刻就把它拆开。**唯一活下来的是行的 key**,而那正是 element 下次构建时能把行拼回去的唯一
  依据。
* **只有真的有行带装饰时,才会建那份装饰列表。** 大多数表格一个都没有,而「每行一个 null」的列表,是
  绘制方每帧都要走一遍、却什么都学不到的东西。
* **行装饰会填满整行的横竖两个方向,单元格装饰不会。** 一个单元格可能比它所在的行矮;一行的装饰不会。
* **基线对齐是唯一需要「问是哪条基线」的对齐**:字母基线和表意基线高度不同,跨书写系统没有合理的默
  认值。
* **`TableCell.verticalAlignment` 可空,意思是「用表格的默认」**,所以它没有默认成 `Top`。而
  `applyParentData` 标记的是**父节点**去重新布局:单元格的对齐是相对它那一行的高度决定的,要重新量的
  是那一行。

**快照那半边,整个 widget 就是一笔交易:有些效果**每帧很贵、每像素很便宜**——缩放、扭曲、模糊。作用
在一棵复杂子树上,它们每帧都要完整重绘;作用在**那棵子树的一张照片**上,它们只要一次栅格化,之后就
不要钱了。

而它**刻意只适合短动画**,理由就是同一笔交易反过来读:**快照是冻住的,所以孩子内部任何在动的东西都
会停下来。** 上游举的例子是 Android Q 的缩放页面转场——几百毫秒,长到能省下真功夫,短到没人注意到孩
子其实是张照片。

**三种模式回答的是同一个问题:子树里有平台视图(原生地图、网页视图)时怎么办。** 它由引擎合成、不在
那张图里,所以子树的快照根本不包含它。

* **默认那个会抛错**,这对一个性能优化来说初看很凶,直到考虑另一种做法:**默默不做快照,意味着昂贵
  的那条路照跑,而没人会发现**——那正是这个 widget 被加进来要防的 bug。
* **permissive 退回画活的孩子,而效果仍然生效**(走 `paint` 而不是 `paintSnapshot`),读者看到的是一
  帧正确但更贵的画面。
* **forced 照做快照,让平台视图掉出画面**——调用方知道它在别的东西后面、或不在被动画的那部分里时有
  用。

**控制器上那处不对称是要点:`allowSnapshotting` 是一个**值**,变了才通知;`clear()` 是一个**事件**,
无条件通知。** 把快照打开两次应该什么都不发生;要两次新快照,就该出两张新快照,因为孩子可能两次都
变了。

**`paintSnapshot` 同时收 `size` 和 `sourceSize`,而这是上游花了一整段解释的陷阱:** 图像是按**物理
像素**栅格化的,所以它的宽度是 widget 宽度乘以像素比——但 **`image.width` 是那个数四舍五入成的整
数**,拿它当源矩形会采样到捕获范围之外去。`sourceSize` 才是没被取整的真值。

**`autoresize` 默认 false 也是有意的:** 快照期间尺寸变化会把旧图拉伸,而不是重新栅格化。**这通常是
对的,因为常见情形正是缩放动画——拉伸那张图就是效果本身**,每帧重新栅格化等于把这个 widget 本来要省
的开销原样还回去。

验证:`cargo test --lib` 2546 绿,GN `rustflutter_unittests` 2546 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1625 accounted / 263 MISSING(86.1%)。

### 正在收起的节点,孩子得留到动画结束(2026-08-20)

新模块 `sliver_tree.rs`,`widgets/sliver_tree.dart` 四个全到:`TreeSliverNode`、
`TreeSliverStateMixin`、`TreeSliverController`、`TreeSliver`(外加
`TreeSliverIndentationType`)。覆盖率 1619/1888(85.8%)。

**sliver 排的是一个扁平的行列表,而树不是。** 于是整个文件只有一个想法:**把树按调用方写的样子留
着,在旁边维护一份「当前有行的节点」的扁平列表**。那就是上游的 `_activeNodes`,而 active 的意思是
**能经由展开的父级到达**——这和「可见」不是一回事,上游在每个可能被搞混的方法上都特意说明了这一点。

**值得抵达的那条判断是:一个节点正在收起时会怎样。** 它的孩子必须留在扁平列表里,直到动画结束——否
则就没有东西可以动画出去了。所以 unpack 的规则不是「这个节点展开了吗」,而是**「它展开了,或者它正
在动画中(哪个方向都算)」**。

**自查:我把 expand/collapse all 的顺序说反了,而且回归行也跟着错。** 我写的是「延迟列表倒着走,于
是最深的先切换」。实际是:**walk 是后序的,所以最深的先进列表;而列表倒着走,于是切换是从最浅的开
始。**

而这个顺序之所以成立,恰好就是上面那条 unpack 规则:**一个正在收起的节点会把后代留在 active 列表
里,直到它的动画结束**,于是先收起浅的那个,并不会在深的那个轮到之前把它从列表里拿走。上游
`toggleNode` 里那句 `assert(_activeNodes.contains(node))` 靠的正是这一点。**两处看起来无关的代码,
其实是同一条规则的两面**——而我把顺序写反的时候,并没有看出这一点。回归行改成钉住「[1, 2]」,并顺手
断言两个节点确实是一起在动画中出去的。

**其余几条:**

* **没有孩子的节点不能是展开的**,无论怎么要求:`_expanded = (children?.isNotEmpty ?? false) &&
  expanded`。否则每个信任这个标志的调用方,都还得再检查一遍孩子。
* **深度和父节点是 walk 算出来的,不是调用方存的。** 于是一个被挪到树中别处的节点不需要任何修补——
  下一次 unpack 会告诉它现在在哪。
* **动画中的行区间每次构建都重算,而不是记录下来。** 一个正在动画的节点,它孩子的行号会随着上面任
  何东西的展开收起而变——**稳定的是节点,不是它孩子的下标。**
* **零时长不是「很短的动画」,而是「没有动画」,必须按后者处理。** 上游的注释直说:按动画处理会**冻
  住应用**,因为树会在「节点的孩子已经不再 active」的情况下被更新。
* **expand/collapse all 里,隐藏的节点直接改标志,active 的节点才进延迟列表。** 切换一个隐藏节点,
  会启动一段没人看得见的动画;在一棵大树上,那是每个节点一个控制器,换来一个对读者来说本就是瞬间的
  变化。
* **收起一个父节点时,子节点会记着自己曾经是展开的**,好让父节点再打开时恢复原样。
* **`expandNode` 是一个终态而不是一次切换**:已经展开时调用它没有效果。想要切换的调用方有
  `toggleNode`,而说「展开」的调用方要的是结果,不是过程。
* **一个控制器只能绑一棵树**,上游是断言:共用一个会让 `expandNode` 说不清它指的是哪棵。
* **负的缩进会把更深的行走回到更浅的行外面**,所以 `custom` 拒绝它。而 `none` 是一个真正的选择而不
  是退化情形——缩进可以做进行构建器里,变成一条引导线或一个展开三角,而不是一段空白。

验证:`cargo test --lib` 2517 绿,GN `rustflutter_unittests` 2517 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1619 accounted / 269 MISSING(85.8%)。

### 前导图标不是「第 0 个孩子」,它是前导那个(2026-08-20)

新模块 `slotted.rs`,`widgets/slotted_render_object_widget.dart` 四个全到:
`SlottedMultiChildRenderObjectWidget`、`SlottedMultiChildRenderObjectWidgetMixin`、
`SlottedContainerRenderObjectMixin`、`SlottedRenderObjectElement`。覆盖率 1614/1888(85.5%)。

**大多数带多个孩子的渲染对象把孩子放在一个列表里,而孩子的身份就是它在列表里的位置。** 对于孩子各
有含义的 widget,这是错的:**一个列表项的前导图标和尾部箭头,不是「第 0 个孩子」和「第 2 个孩子」,
它们是前导那个和尾部那个**,而任何一个缺席时,另一个都不该跟着挪位置。

于是孩子住在一张**槽位到孩子的映射**里,而有意思的部分是这张表被重建时会发生什么。

**一个带 key 的孩子在槽位之间移动时会保住它的状态。** 匹配器**先在所有槽位里找 key 匹配**,再看当
前正在填的这个槽位——于是把一个带 key 的 widget 从 leading 挪到 trailing,它的 `State` 会跟着走,和
在列表里挪动一个带 key 的孩子完全一样。**列表给了这个承诺,槽位没有理由打破它。**

三路选择的完整规则:

1. **任何位置的 key 匹配优先**,元素从它原来那个槽位里被取出来。
2. **否则复用同槽位的旧孩子,但只在它没有 key 时。** 一个没匹配上的带 key 旧孩子是**别人的**,复用
   它就是把它的状态交给错的 widget。
3. **否则什么都不复用**,新建一个元素。

回归行把三条都钉了,还钉了「两个带 key 的孩子互换槽位,两个都活下来」。

**渲染对象那一侧有两处顺序上的小心:**

* **`_setChild` 先 drop 旧孩子再 adopt 新的。** 顺序要紧:一个还挂在别处就被收养的渲染对象,会在一
  条语句的时间里有两个父节点,树的深度不变式会短暂地为假。
* **`_moveChild` 只在旧槽位「仍然握着这个孩子」时才清它。** 等这次移动跑起来的时候,别的东西可能已
  经占了那个槽位——**那时清掉它,会把一个刚到的孩子丢掉。**

**两条不变式都是断言:槽位列表不能变,槽位不能重复。** 上游对前者的措辞异常坚决——「The list of slots
must be static and must never change for a given class」。一个槽位会变的类,会让孩子因为没人要求过的
理由出现和消失。这里两条都做成了可返回的错误,好让它们能被钉住。

**自查:尺子指出我少了一个名字,而它是对的。** 我把 `SlottedContainerRenderObjectMixin` 命名成了
`SlottedContainerRenderObject`——去掉了 `Mixin` 后缀,因为它在这里是一个结构体而不是 trait。但**尺
子按名字匹配,读者也按名字搜**;上游叫什么,这里就该叫什么。改回带 `Mixin` 的名字,并在文档里说明
它为什么是结构体:**不像上游多数 mixin,这一个是带状态的**——槽位映射本身——而它做的每件事都是在这张
映射上的方法。

**其余两条:**

* **`children` 的基类实现「对返回顺序不作保证」,而上游明确告诉子类:顺序要紧时请覆写——尤其是命中测
  试。** 一个按映射顺序访问孩子的命中测试,会让错的那个赢。这个移植保留声明的槽位顺序,这比上游的基
  类更强,并把这件事写出来而不是假装映射是有序的。
* **那个 mixin 版本是被废弃的**,理由是「to simplify the process of creating slotted widgets」——**同
  一件事有两种说法,就比需要的多了一种**,而继承那种不需要解释。

验证:`cargo test --lib` 2490 绿,GN `rustflutter_unittests` 2490 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1614 accounted / 274 MISSING(85.5%)。

### 点外面往下关,按 Esc 从根上关(2026-08-20)

新模块 `raw_menu_anchor.rs`,`widgets/raw_menu_anchor.dart` 五个全到:`RawMenuOverlayInfo`、
`RawMenuAnchor`、`RawMenuAnchorGroup`、`MenuController`、`DismissMenuAction`。覆盖率 1610/1888
(85.3%)。

**菜单系统是一棵锚点树,不是一串打开的菜单。** 每个锚点知道自己的父与子,而这里几乎每一条判断,说
的都是**一个请求沿树往哪个方向走**:

* **点击外面往下关**:子菜单没了,它的父级还在。**一个从子菜单上点开的读者,并没有要求丢掉那条菜单
  栏。**
* **Esc 从根上关**:`DismissMenuAction.invoke` 伸手去拿的是 `_anchor.root`,而不是它被触发的那个锚
  点——**Esc 的意思是「这个菜单我用完了」,不是「往上退一级」。**
* **打开状态的变化先往上走**:一个在后代打开时画法不同的祖先,要在任何人重建之前就知道。

**关闭有两种速度,而这个区别是承重的。** `closeChildren` 立刻关掉孩子;`requestChildrenClose` 启动
孩子的**关闭序列**——而**一个会淡出的菜单,正是在这段序列里淡出的**。上游在两个方法的文档里互相交叉
引用,这本身就说明这个差别不是措辞问题。而 `inDispose` 那条路走的是立刻关的那个,也必须如此:**一
个正在被卸载的菜单,已经没有帧可以拿来做动画了。**

**自查:我第一版把这两条写成了同一件事。** `CloseKind` 的两个分支都调 `close`,注释说它们不同、代
码说它们一样——这正是我一直在抓的那种「摆设」。改成 `Requested` 走 `handle_close_request`(记录一次
关闭请求再关),并补了一个 `handle_close_request_deferred` 表示「序列开始了但还没结束」。回归行现在
能看出差别:立刻关那条 `close_requests` 是 0,请求关那条是 1。

**两个自动关闭各有各的克制:**

* **只有根锚点在祖先滚动时关闭。** 上游的注释把理由写死了:「Don't just close it on *any* scroll,
  since we want to be able to scroll menus themselves if they're too big for the view.」——**一个长
  到需要滚动的菜单,否则会在读者刚滚它的那一刻把自己关掉。**
* **视口尺寸变了就关**,因为菜单是相对一个刚刚变过的视口定位的,它的位置已经过时;**在下一次布局之
  前,没有办法知道锚点挪到哪儿去了**,关掉是诚实的答案。而**第一次观察到尺寸只做记录不做关闭**——否
  则每个菜单都会在它打开的那一帧关掉自己。

**`maybeOf` 和 `maybeIsOpenOf` 这一对值得单说:前者刻意**不**建立依赖**,于是一个只是握着控制器好
调用 `close()` 的菜单项,不会在任何菜单开合时都重建一次;后者**建立**依赖,因为它的答案正是那个变
了的东西。

**其余几条:**

* **`MenuController.open` 断言已附着,而 `close` 不断言。** 关掉一个已经没了的菜单,正是 dispose
  路径会做的事,它应该被允许无害地这么说;而打开一个没人构建的菜单是编程错误。
* **`_detach` 只在「就是那个锚点」时才摘**:一个在控制器已经转移之后才被销毁的锚点,不该把控制器从
  它的新锚点上扯下来。
* **`RawMenuAnchorGroup` 自己永远不「打开」**——它的 `isOpen` 是「**任何一个孩子**开着」。这就是菜单
  栏能托管子菜单、而自己不是一个可被 dismiss 的菜单的原因。
* **`consumeOutsideTaps` 默认 false**:关掉菜单的那一下点击**仍然会到达它落在的东西上**,而这通常正
  是读者点那儿的用意。
* **`showOverlay` 在处置之后调用是空操作,而且不会触发 `onOpen`**:一个延迟打开的菜单,如果在等待期
  间已经没了,不该宣告一次不可能发生的打开。
* **`DismissMenuAction.isEnabled` 看控制器有没有附着**:没有菜单开着时的一次 Esc,应该到达别的想要
  它的东西那里——通常是一个对话框。

验证:`cargo test --lib` 2470 绿,GN `rustflutter_unittests` 2470 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1610 accounted / 278 MISSING(85.3%)。

### 返回键问到有人认领就停,退出请求问完所有人才停(2026-08-20)

新模块 `binding.rs`,`widgets/binding.dart` 五个全到:`WidgetsBindingObserver`、
`WidgetsBinding`、`RootWidget`、`RootElement`、`WidgetsFlutterBinding`。覆盖率 1605/1888
(85.0%),**整数关口 85%**。

**这个文件里最值得写下来的,是两条彼此矛盾却都对的分发规则:**

* **一次返回按下,问到第一个认领的就停。** 最里面那个处理掉,后面的人根本不会听说这次按下——**一次
  返回关掉两个东西,正是这条规则要防的 bug**。而没人认领时应用退出,这是对的默认:没人想要的一次按
  下,就该离开。
* **一次退出请求,不在第一个说「取消」的地方停。** 上游的注释把理由写死了:

  > Don't early return. For the case where someone is just using the observer
  > to know when exit happens, we want to call all the observers, even if we
  > already know we're going to cancel.

  一个要在退出路上保存草稿的观察者,即便别人已经拒绝了这次退出,也必须被告知。**一个取消足以取消;
  但不足以停止询问。**

回归行把两条都按「问了几个」钉住:返回键那条断言第三个观察者**从没被问过**,退出那条断言三个**全被
问了**,尽管第一个就已经取消。

**两个循环都遍历观察者列表的一份拷贝**,于是一个在被通知时把自己摘掉的观察者,不会把它正身处其中的
那次遍历弄坏。

**`didRequestAppExit` 的默认答案是「退出」**,这一条也值得单说:**一个不关心的观察者,绝不该成为应用
关不掉的原因。**

**`didPushRouteInformation` 的默认实现会把 URI 规范化再转给 `didPushRoute`**,而规范化不是装饰:空
路径变 `/`,空的查询和片段被丢掉而不是留下一个光秃秃的 `?` 或 `#`。否则一个靠字符串匹配路由的观察
者,要应付同一个地址的四种拼法。

**根那半边,`attach` 的分支正是热重载能保住状态的原因。** 没有 element 时新建并挂载;**有 element
时,把新 widget 存起来、把 element 标脏**,而不是重新挂载一遍——热重载的第二次 `runApp` 就地更新这
棵树,底下每一个 `State` 都活下来。回归行钉住了「标脏之后、build 阶段之前,孩子还是旧的那个」。

**而 `performRebuild` 里 `_newWidget` 可以为空,上游也说了为什么**:「if, for instance, we were
rebuilt due to a reassemble」。一次没有新 widget 的重建,就是对同一个 widget 的重建,而这正是
reassemble 想要的。

**`_rebuild` 的 catch 带着整个文件里最锋利的一句注释:「No error widget possible here since it
wouldn't have a view to render into.」** 框架里其他任何地方,构建失败都会被换成那个红色错误 widget;
这里没有东西可以换上去——**失败的那个,正是本该提供视图的那个**。于是错误被上报、孩子留空,读者得到
的是一块白屏加一条真的错误日志,而不是一次崩溃。

**其余几条:**

* **`RootWidget.child` 是可选的**:一个底下什么都没有的根,正是 `ensureInitialized` 与第一次
  `runApp` 之间存在的那个状态。
* **`debugShortDescription` 替换掉一个所有应用共享的类名**:没有它,每份错误转储的开头都是同一行没
  有信息量的字。
* **`RootElement.mount` 断言父节点为空**:它就是根,挂到别人底下会让这棵树有两个根。
* **`ensureInitialized` 实现的模式值得点名:第一个调用者决定这个 binding 是什么。** 测试框架在应用之
  前先调它自己的那一版,于是等 `runApp` 来问时已经有一个测试 binding 在那儿了,不会被顶掉。无条件构
  造就会把它换走。
* **预测性返回手势在没人注册时会退回成一次普通返回按下**——平台把手势发给一个从没登记过的应用,不该
  把这次按下弄丢。

验证:`cargo test --lib` 2437 绿,GN `rustflutter_unittests` 2437 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1605 accounted / 283 MISSING(85.0%)。

### 「大概是中文」比「读者的第二选择,但完全对上」更差(2026-08-20)

新模块 `localizations.rs`,`widgets/localizations.dart` 五个全到:`LocalizationsDelegate`、
`WidgetsLocalizations`、`DefaultWidgetsLocalizations`、`Localizations`、
`LocalizationsResolver`,外加 `widgets/app.dart` 里喂给它的 `basicLocaleListResolution`。覆盖率
1600/1888(84.7%),**整数关口 1600**。

这里决定两件事,而它们是分开的:**用哪个语言**,和**用哪份资源**。

**语言匹配的精妙之处在于它对「只有语言对上」这种匹配有多**不情愿**——以及这份不情愿在哪儿不适用。**

从**第二个**偏好开始,只有语言对上的命中会被**记住而不是返回**:「大概是中文,字体不知道」比读者自
己列表里再往下一位的精确匹配更差,而循环必须先看到那一位才能知道。

**而第一个偏好完全不不情愿,当场就返回。** 上游的理由是:第一个语言通常是强烈偏好的,所以在那里的
语言级匹配很可能就是读者想要的,哪怕再往下有更精确的。唯一还要等的情形是**下一个偏好是同一种语
言**——那时多等一轮,最坏一样、最好更好。

**自查:我第一版把这条规则写反了。** 回归行写的是「preferred = [zh_Hant_TW, en_US],supported =
[zh, en_US] 应该给 en_US」,理由是「精确匹配更好」。跑出来是 `zh`——**因为 zh_Hant_TW 是第一个偏
好,当场返回那条规则先生效了。** 实现是对的,是我的规则叙述漏了「第一个偏好例外」这一半。改成用一
个匹配不上任何东西的第一偏好(`sv`)把不情愿显出来,再单独钉一条「第一个偏好一点都不不情愿」。**文
档里那段解释也一起改了——把规则说漏一半的注释,比不写注释更容易误导。**

**匹配的完整阶梯:** 语言+字体+国家全中 → 语言+字体 → 语言+国家 → (上一轮记下的语言级命中) → 语言
级 → 最后才是**只按国家**匹配(上游的理由:读者很可能熟悉他所列国家的某种语言),再不行就是应用支持
列表里的第一个。

**而索引是倒着建的**,好让**最先**列出的受支持语言赢下每个键——应用自己的排序是一种偏好,正着遍历会
把它悄悄倒过来。回归行专门钉了这条。

**资源那半边,让 delegate 可组合的只有一句话:每个类型只加载第一个 delegate。** `MaterialApp` 把应
用自己的 delegate 排在框架的前面,于是应用提供的 `MaterialLocalizations` 会**遮蔽**内置的,而不需要
移除任何东西。

**而两个条件的顺序值得读两遍:类型只在这个 delegate**同时也支持这个语言**时才被占住。** 一个本来会
遮蔽框架的 delegate,如果处理不了当前语言,就会**让开**,而不是把读者留在什么都没有的状态。

**还有那条同步快路径:所有 delegate 都同步返回时,整件事在同一帧内解决。** 上游为此花了真功夫——特
意不对已经完成的 future 调 `Future.wait`。目的就是**第一帧就带着字符串**,而不是先空一帧、文字再冒
出来。

**其余几条:**

* **框架自带的 delegate 对**所有**语言都声称支持**,而它只有美式英语一份。这初看是错的,直到考虑另
  一种做法:一个拒绝未知语言的框架,会让读者面对一个「应用自己的文字都翻译好了、而『粘贴』按钮完全
  没有标签」的界面。**英文是个差答案,没答案是更差的那个。**
* **文字方向放在 `WidgetsLocalizations` 里而不是挨着 locale**:它是语言的属性,而一个放在
  localizations 之上的 `Directionality` 得靠人手动保持同步。
* **`shouldReload` 默认是 false**:delegate 通常是 const 对象、每帧重建出一模一样的一个,每帧重载一
  次字符串表荒唐得很。
* **`Localizations.of` 返回可空值而不是断言**:好几个 widget 没有某份本地化也能工作(退回一个没有标
  签的控件),它们应该能问。
* **`LocalizationsResolver.update` 只在受支持集合变了时才重新解析**,这是有意的:另外四个字段由
  `locale` getter 按需读取,改它们这里不必做事;而受支持集合正是那份缓存的平台解析所依据的东西。
* **应用设了 `locale` 就只对着它自己解析**,不再看平台列表——一个说「就用法语」的应用是认真的。

验证:`cargo test --lib` 2414 绿,GN `rustflutter_unittests` 2414 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1600 accounted / 288 MISSING(84.7%)。

### 甩一下必须前进一页,哪怕内容几乎没动(2026-08-20)

两个新模块:`scroll_physics.rs`(六个:`ScrollPhysics`、`ClampingScrollPhysics`、
`RangeMaintainingScrollPhysics`、`BouncingScrollPhysics`、`AlwaysScrollableScrollPhysics`、
`NeverScrollableScrollPhysics`)和 `page_view.rs`(四个:`PageController`、`PageMetrics`、
`PageScrollPhysics`、`PageView`)。覆盖率 1595/1888(84.5%)。

**台账自查(第四次)。** 台账里记着 `ScrollPhysics ≈ physics.rs 的 Simulation 家族 + Scroll 的边界
夹取`、`ClampingScrollPhysics ≈ ClampingScrollSimulation`。前者是硬撑:**`ScrollPhysics` 是一条可
组合的链**——`applyTo`/`applyPhysicsToUserOffset`/`applyBoundaryConditions`/
`createBallisticSimulation`,而那套组合此前一点都不存在。这一轮把它做成真的 trait,两条台账删掉,
`scroll_physics.dart` 从 `mapped:2 MISSING:4` 变成 `covered:6`。

**物理量回答四个问题,而它们分开,是因为各平台在这四件事上各自不同:** 能不能拖?拖 n 像素内容动多
少?越界了怎么办?松手飞到哪里?**Android 对第三个的答案是「停死」,iOS 的是「拉伸再弹回」**——两者
不是彼此的微调,而是各自平台滚动时的全部性格。

**`ClampingScrollPhysics` 的四个分支不是一次夹取,因为答案取决于「位置原本在哪」。** 原本就在边上或
外面,整个提议全被拒;原本在里面而提议跨了出去,**只有越界那一段被拒**——剩下的拖动照常兑现,于是
一次停在边界上的滑动仍然走完它该走的距离。

**`BouncingScrollPhysics` 的摩擦是「越界距离的二次函数」**,阻力比距离长得快——这就是 iOS 的越界像
在拉橡皮筋而不是拉一个恒重物的原因:头五十像素几乎免费,后五十像素不是。而**往外拉比往回放阻力
大**:easing 时摩擦按「拖动将要结束的位置」算,而不是起点。快速减速档上,**往回的拖动完全不收摩擦**
——内容精确跟着手指回家。

**符号约定值得单独写一句,因为我的第一版回归行正栽在它上面:** offset 与 pixels **方向相反**(上游
是 `setPixels(pixels - offset)`)。于是**越过末端时,正的 offset 才是「拖回来」**。我第一版把两者写
反了,测试如期变红——实现是对的,测试的方向感是错的。现在这条约定写进了 trait 的文档,回归行里也留
了一行注释。

**`RangeMaintainingScrollPhysics` 是那种「不出问题就没人注意到」的物理量:** 懒加载列表在读者读到一
半时变长,不该挪动他正在看的那几行。它的判断是**四条各自独立的「别插手」的理由**:位置正在动画中
(上游:「the jumping around would be distracting」)、范围没变、位置已经被别人改过、旧位置本来就
在范围外。

而**「保持越界量」只在范围**缩小**时做,那条限制才是有意思的一半:内容**增加**时保持同样的越界量,
会直接跳过刚到的全部内容、蹦到新的最大值——**读者永远看不到新来的东西。**

第三条还有一条要读两遍的附加条件:**只有四个端点全是有限值时才放松边界**。无限端点意味着一个还不
知道自己多长的懒加载列表;上游的推理是逆否命题——**边界本来就是有限的而位置还是变了,那就是有人有
意为之。**

**分页那半边,最有判断力的是「松手落在哪一页」。** 不是「最近的一页」:一次几乎没让内容动的甩动会
直接四舍五入回原地,而**甩了一下的读者会被告知他的手势毫无意义**。上游在四舍五入**之前**先按甩动
方向挪半页,于是任何超过速度容差的松手都恰好前进一页,而只有慢速松手才退回取最近。

**而这里有一处真实的不对称,原样移植并写下来:四舍五入是「远离零」的。** 从恰好在第 n 页的位置,向
前甩到 `n+0.5` 进位成 `n+1`,**向后甩到 `n-0.5` 却又被舍回 `n`**——一次在精确边界上释放的向后甩动
哪儿也不去。实际上几乎不会咬到人,因为向后拖动在松手时早就把内容挪离了边界:在 `n-0.001` 处同样的
甩动会到 `n-0.501` 并正确舍下去。**但它是真的**,回归行把两半都钉住了——包括「一个像素的拖动就足以
让舍入转向」。

**其余几条:**

* **`_initialPageOffset` 的 `max(0, …)` 就是它的全部**:分数小于 1 时为零(页面比视口窄,从前沿开
  始,邻页在边上探头);**大于 1 时每一页都比视口宽**,首页两边都挂在外面,得往回拉半个超出量才居
  中。
* **像素↔页的往返要吸附回整数**:不吸附会留下 2.9999999999999996,而拿它去 `nextPage` 会差一页。
* **`nextPage` 先四舍五入再加一**,所以停在 2.4 的视图去的是 3 而不是 3.4——否则它会永远停在页边界
  之外。
* **两个 PageView 共用一个 controller 时,`page` 抛错而不是取平均**:它们可能在不同页上,没有答案可
  给。而在视口还没尺寸时 `jumpToPage` 会**记下来而不是失败**——在 `initState` 里跳转是合理的事。
* **`PageView.allowImplicitScrolling` 默认关闭,理由是无障碍而不是物理**:开着的话屏幕阅读器能把焦
  点移进隔壁页,而这只在那一页确实属于同一阅读顺序时才对。
* **`AlwaysScrollableScrollPhysics` 只有一条覆写,而它有用**:基类在没东西可滚时拒绝拖动(对页面里
  的列表是对的),但对下拉刷新是错的——**手势本身正是那个会让列表变得需要滚动的东西。**
* **`NeverScrollableScrollPhysics` 有两条覆写,人们常忘第二条**:关掉用户滚动后,框架仍可自己滚(为
  了露出获得焦点的输入框、或为屏幕阅读器)。一个「故意不可滚、由父级代滚」的列表于是还是会滚——在错
  的轴上、错的时刻。所以隐式滚动也一并关掉。

验证:`cargo test --lib` 2388 绿,GN `rustflutter_unittests` 2388 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1595 accounted / 293 MISSING(84.5%)。

### 不透明省下的是「构建」,不是「绘制」(2026-08-20)

新模块 `overlay.rs`,`widgets/overlay.dart` 五个全到:`OverlayEntry`、`Overlay`、
`OverlayState`、`OverlayPortalController`、`OverlayPortal`。覆盖率 1587/1888(84.1%)。

**overlay 是一个任何地方的代码都能往上压东西的栈。** 路由住在里面,对话框、提示框、拖拽代理、选择
手柄也都是。它比 `Stack` 多出来的东西是:**条目由代码插入,而不是由父节点声明**——于是想要一个提示
框的那个东西,不必同时是负责排布整个屏幕的那个东西。

**第一条要点:不透明是构建期的优化,不是绘制期的。** 一个声称自己不透明的条目,会让 overlay **停止
构建**它下面的一切——而不是把它们建出来再盖住。所以 `maintainState` 才要作为一个单独的退出开关存
在:一条在后台的路由需要保住状态,尽管没人看得见它,因为它承诺过的 future 得能完成。

而**保住状态并不等于保住动画**:offstage 的条目以 `tickerEnabled: false` 构建。**一个看不见的东西
在动,是没有观众的功夫**,而十层路由的栈就是十倍的功夫。

**「已插入」和「已挂载」是两个问题。** 一个躲在不透明条目后面的条目是已插入而未挂载的,而条目会**为
此发通知**——一个要把东西对齐到某条目 widget 上的调用方,需要知道那个 widget 什么时候存在,而「插入
条目」不是那个时刻。

**插入位置那对不对称不是随意的:** `below` 返回锚点自己的下标,`above` 返回它后面一个。考虑到「列表
里更靠后 = 栈里更靠上」,两者都把新条目放在了名字所说的那一侧。

**`rearrange` 的契约有一处容易漏掉:没有被新列表点名、但已经在 overlay 里的条目会被保留**,并按给
定位置重新插入。它是对被点名者的**重排**,不是对整个列表的**替换**。而新旧顺序相同时它提前返回,于
是重算出同样顺序的调用方不必付一次重建。

**`OverlayPortal` 在原地构建它的 overlay 孩子,然后把它显示在别处。** 孩子是在 portal 所在的树位置
上构建的,于是它继承 portal 能看到的主题、方向和一切;它只是被**渲染**在 overlay 里。**反过来在
overlay 里构建,会让提示框拿到 overlay 的继承上下文,而不是按钮的。**

**z 序是一个单调递增的计数器,而不是一个栈**,这就是「最后一个 `show()` 的 portal 在最上面」不需要
任何人维护顺序就成立的原因。于是 **`show()` 在已经显示时会把孩子提到最上面**而不是什么都不做——重新
取一次序号,而新序号按构造就是最大的。上游从 `-2^63` 起步(web 上是 `-2^53`),这样任何序号都胜过
「未设置」;而 **web 上那个起点正是 double 还能精确计数的地方**——过了 2^53,自增就不再改变数值,两
个 portal 会打平。回归行把这条也钉住了,连同它的 double 往返。

**顺手修了四处过时的注释。** `autocomplete.rs`、`reorderable_list.rs`、`text_selection.rs`、
`magnifier.rs` 里都写着「this crate has no overlay」——现在这句话只对了一半。改成实话:
`crate::overlay` 有了条目表和它的顺序规则,**但还没有任何东西承载那些 widget**。一句半真的注释比没
有注释更容易误导下一个读者。

**未移植的部分写在模块头里**:`_Theater` 渲染对象、让某个条目决定 overlay 尺寸的那套布局、以及把
portal 的孩子搬进另一棵子树的 element 级管线,都不在。移植的是**条目表与它的排序规则、onstage/
offstage 的判断,以及 portal 的 z 序**。

验证:`cargo test --lib` 2339 绿,GN `rustflutter_unittests` 2339 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1587 accounted / 301 MISSING(84.1%)。

### 一个能画出窗口边界的提示框,只能是它自己的窗口(2026-08-20)

一轮扫掉五个小文件的八个类:新模块 `view.rs`(`View`、`RawView`、`ViewCollection`、
`ViewAnchor`),外加 `UniqueWidget`→`framework.rs`、`WillPopScope`→`routes.rs`、
`WidgetSpan`→`painting.rs`、`ShrinkWrappingViewport`→`scrolling.rs`。覆盖率 1582/1888
(83.8%)。

**`view.dart` 的核心概念是「分区」。** 一个应用通常只有一个窗口和一棵 widget 树,两者重合到根本不
用说明。这四个 widget 就是它们不重合的时候:一个要伸出主窗口边界的提示框、第二块显示器、并排托管
的平台视图。

树的大部分是**渲染区**——里面的每个 widget 最终都会产出某个渲染对象、画进同一个视图。而
`ViewAnchor` 的侧槽和 `ViewCollection` 的孩子是**非渲染区**:那里的 widget 自己不建渲染对象,只
承载 `View`,而每个 `View` 又为它自己的窗口重新开一个渲染区。

**由此落出来的规则只有一条,也是这个文件唯一需要记住的:锚点和下一个 `View` 之间不允许出现任何渲
染对象 widget。** 在那段空隙里画画的东西,没有视图可画。而**继承类 widget 是允许的,而且正是要
点**——侧视图能读到锚点上方的一切主题和方向。

**上游给的例子解释了整个文件:一个提示框要伸出主窗口的边界。** 它不能是按钮的孩子,因为孩子会被窗
口裁掉;于是它成为自己的视图,靠把按钮包进一个锚点来跟按钮挂钩。

**其余几条判断:**

* **`ViewAnchor.view` 是可选的**,而一个没挂东西的锚点是完全正常的状态——提示框没显示时就长这样。
  要求必填会逼调用方每帧建一个又扔掉。
* **只有侧视图被包进 `LookupBoundary`**,而这正是这个 widget 的意图的精确表述:侧视图可以**读**锚
  点上方的一切,但它里面的任何东西都不能**向上查找**穿过锚点、找到周围那棵渲染树。它画在别处,一
  个找到主视图渲染对象的后代,问的是错的窗口。
* **那对废弃参数的两条断言值得留着**:必须同时给或同时不给(给一半会让一棵渲染树被一个 pipeline 拥
  有、却注册给另一个),而且给的 render view 必须属于这个窗口(不然应用会画进别人的窗口里)。
* **`ViewCollection` 没有 child,只有 views**,这就是它和锚点的区别:周围那层自己什么都不渲染,只
  托管窗口——多窗口应用的根就长这样。

**另外四个各归其位:**

* **`UniqueWidget` 的 key 是必填的,而这就是它的全部设计。** 一个普通 widget 可以出现在任何地方、
  任意多次,「这个 widget 的 state」于是不是一个有唯一答案的问题;全局 key 让它变成一个,而
  `currentState` 就是那个答案——树外的调用方可以直接够到 state,而不必把回调一路传下去。而它返回的
  是可空值并且保持可空:**没挂载的唯一 widget 是常态,不是错误。**
* **`WillPopScope` 被废弃的原因,和 `PopEntry` 持有常驻答案是同一个原因:`onWillPop` 是在弹出时才
  问的,而且返回一个 future。** Android 的预测性返回手势必须在滑动**开始之前**就知道这页会不会走,
  才能把后面那页画出来——而一个还没被 await 的 future 里取不出答案。替代品把「提问」换成了「一个随
  时可读的值」。它的实现就是那套注册舞蹈,而**注册发生在 `didChangeDependencies` 而不是
  `initState`**:所属路由是通过 context 找的,而 context 在依赖解析之前没有祖先可查。而它**每次都
  先摘再挂**,包括第一次——widget 可能被移到了另一条路由下。
* **`WidgetSpan` 是 span 树的叶子**:它托的 widget 根本不是文字的一部分,段落只为它留一个盒子。所
  以这个类带的是对齐方式和基线,而不是任何文本——那是排版器对一个它量不了的东西唯一能回答的问题。
  而**三种相对基线的对齐必须点名是哪条基线**:不说明是哪条的基线对齐,不是一个更宽松的请求,而是一
  个**无法回答**的请求,上游拒绝而不是替你挑一条。缩放因子问的是**这个 span 处生效的字号**,于是
  标题里的内联 widget 会按标题的幅度一起变大;字号为零时直接给零,而不是去除以它。
* **`ShrinkWrappingViewport` 和普通视口只差一句意图,后果却很大。** 普通视口占满主轴上给它的全部空
  间、让内容从中滚过;这一个占内容要多少就多少,上限是给它的那么多。后果是:**它的 sliver 必须在
  它自己的尺寸确定之前就被布局**,于是它不能接受无界约束(没有上限可收),也不能懒加载——「内容有多
  大」正是懒加载拒绝回答的那个问题。**一个 shrinkWrap 的列表会把每个孩子都建出来。** 上游对
  `ListView.shrinkWrap` 的警告不是提示,是一句承诺。

验证:`cargo test --lib` 2309 绿,GN `rustflutter_unittests` 2309 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1582 accounted / 306 MISSING(83.8%)。

### 密码要短暂地不是密码,而剪切和复制的规则并不对称(2026-08-20)

新模块 `editable_text.rs`,`widgets/editable_text.dart` 五个全到:`TextEditingController`、
`ToolbarOptions`、`ContentInsertionConfiguration`、`EditableText`、`EditableTextState`。覆盖率
1574/1888(83.4%)。

**一个文本框并不拥有它自己的值**,`TextEditingController` 才拥有,文本框只是在听。这就是表单不必
伸手进 widget 里就能读到文字的原因,也是控制器那几个 setter 各自带着规矩的原因。

**控制器上的三条规矩:**

* **新建的控制器没有选区,而不是「光标在 0」**(`TextSelection.collapsed(offset: -1)`)。文本框在拿
  到焦点时把光标放到**文字末尾**来修正它——所以用一个字符串建出来的控制器,不会把读者丢在开头。而
  `clear()` 之后的选区**是**收拢在 0 的:一个被清空的字段已经被聚焦过了,一个刚建出来的还没有。
* **`text=` 会把光标扔掉**(选区回到 -1、composing 清空)。上游文档说它基本只该在测试里用,原因就
  在这:读者正在打字时设它,会把人丢回一个不确定的地方。生产代码该设 `value`,并带上自己选好的选
  区。
* **选区越界是抛错而不是夹取**——夹一下会让调用方以为自己拿到了一个它其实没拿到的选区。而**新选区
  离开 composing 范围就把它清掉**:读者已经走出了 IME 正在拼的那个词,那个词就算写完了,不管他是
  不是有意的。

**`buildTextSpan` 里那条「composing 范围越界就整个忽略」是发布版的安全网**,上游注释直说:在发布版
抛错会让这个字段带着一棵坏掉的子树被构建出来,而**少一条下划线,比少一个输入框,是小得多的失败**。

**自查:这条安全网差点被我测成不可达。** 我第一版的回归行想通过 `from_value` 造一个越界值——结果
`debug_assert` 直接把它拦下了,测试挂在断言上。这不是测试写错了参数,而是**这条网在 debug 下根本
够不着**:所有构造路径都先断言了一遍。发布版里断言不存在,于是它是唯一挡在畸形范围和「构建不出来
的字段」之间的东西。所以把它拆成一个接受任意 `TextEditingValue` 的静态方法,回归行直接喂一个没走
过构造函数的值——**这才是它在发布版里真正会遇到的输入。**

**密码要短暂地不是密码。** 手机上的密码框会把刚打的那个字符显示三次光标闪烁再藏起来——**没人能在软
键盘上盲打密码**,而「一个字符、短暂地、只在移动端」是各家最后都落到的那个折中。这里面有四条判断:

* **只有「长度恰好 +1」才算**:粘贴、删除、IME 一次提交三个字符,全都不算——它们都不是需要读者确认
  的那一次按键。
* **计的是光标闪烁次数(3)而不是时长**,于是这段显示的长短跟着读者对这个字段自身节奏的感觉走,而
  不是某人挑的一个毫秒数。
* **移动端才显示,而这个检查放在「构建文本」的时候而不是「起计数」的时候**——于是一个跨平台移动的
  字段会立刻停止显示,而不是等它自己数完。
* **系统设置中途关掉,当场归零**而不是让剩下的次数走完,把读者的密码继续挂在屏幕上。

**工具栏那张表是这个文件里最有意思的部分,而它并不对称:**

* **只读字段可以复制,不能剪切**——读和复制正是只读字段的用途。
* **密码框既不能剪切也不能复制**:两者都会把明文放进剪贴板,让别的应用随便读。
* **但密码框可以粘贴**——`pasteEnabled` **不检查 `obscureText`**:秘密是往里进,不是往外出。
* **`Unknown` 的剪贴板不算可粘贴**:在答案回来之前给出粘贴按钮,就是给一个可能按了没反应的按钮。
* **全选最看平台**:macOS **从不提供**;iOS 只在还没有任何选区时提供;其余平台只要不是「已经全选
  了」就提供。后两者是同一个想法的不同严格程度——**一个按下去什么都不会变的菜单项,不该在菜单
  里。**
* **查词/网页搜索是 iOS 独有,分享是 iOS 加 Android**;而**选中的如果全是空白就都不提供**——对着一
  串空格查词会打开一个什么都没有的词典。
* **Live Text 要的是收拢的选区,和上面所有人相反**:它是**插入**,所以它需要一个插入点,而不是一段
  要处理的范围。

**其余两条:**

* **`_shouldCreateInputConnection` 在只读时仍然为真的两个例外(web 和 macOS)都是关于「选区归谁
  管」的**:那两个平台上选区归平台,断掉连接就等于断掉读者选中文字的能力——而**选中只读文字来复
  制,正是只读字段存在的意义**。
* **批量编辑是可嵌套的计数**:一个在另一个里面开批次的格式化器,不该因为内层关闭就把一个半成品的
  值发给平台。而字段销毁时批次未平衡是断言——里面的值将永远发不出去。

验证:`cargo test --lib` 2283 绿,GN `rustflutter_unittests` 2283 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1574 accounted / 314 MISSING(83.4%)。

### 撤销难的不是栈,是「什么时候入栈」(2026-08-20)

新模块 `undo_history.rs`,`widgets/undo_history.dart` 四个全到:`UndoHistory`、
`UndoHistoryState`、`UndoHistoryValue`、`UndoHistoryController`(外加上游私有的 `_UndoStack` 与
`_throttle`)。覆盖率 1569/1888(83.1%)。

**一个文本框的撤销栈,难点从来不在栈上,而在「什么时候入栈」。** 读者打出 "hello" 会产生五次值变
化,而「每次撤销删掉一个字母」的五步撤销,不是任何人所说的撤销。于是入栈被节流:**每 500 毫秒最
多一次,而真正落下的那一次带的是最新的值**,不是开窗时那个。

那个节流窗口正是后面所有微妙之处的来源——**它开出了一段「屏幕上的值还没进栈」的时间**。有两条判断
只为覆盖这段时间而存在:

* **入栈待定时按撤销,取消这次入栈、恢复上一个已提交的值**,而不是在栈里退一步。读者要撤销的正是
  他刚打的东西,而他刚打的东西从来没被记录过;退一步会跳过一个他看得见的状态。
* **在第一次入栈落地之前收到的撤销,什么也不做**,而不是取消那次入栈。丢掉它会让这个字段一个可回
  去的历史都没有。

**节流的形状值得单独命名**,因为「throttle」这个词被用来指好几种东西:这一个是**前沿开窗、尾值触
发**。第一次调用开窗,窗内的每次调用替换参数并返回**同一个** timer,窗关时用最新的参数跑一次。对
打字来说这恰好对:窗在第一次按键就开,撤销步很快就存在;窗关时带走的是整串,而不是它的第一个字
母。

**栈本身也有两处「不是想当然」的地方:**

* **`undo()` 在栈底不返回 null,而是原地不动、把已经在那儿的值还回来。** 撤销不会掉出边界——它停
  下,读者拿到他能回到的最老状态。redo 在另一头同理。
* **撤销之后再打字,会把被撤销掉的那一段整个丢掉。** 读者走了另一条分支,旧的那条从这里不可达。

**自查,这轮最值得写的一条:`_duringTrigger` 那条守卫,我第一版的回归行是空的。**

第一版我把它写成「撤销产生的值不会被立刻推回去」,断言栈没变——但栈本来就不会变,因为没有任何东西
会在触发期间调 `push`。**这个标志在我的移植里是死代码,而那条回归行什么都没测。** 加了
`echo_during_trigger`(模拟 widget 的监听器在 `onTriggered` 里同步回声)之后再测,**它仍然是空
的**:把守卫改成恒假,测试照样绿。原因是 `_lastValue` 那条检查先一步拦住了——`_update` 在触发前就
把 `_lastValue` 设成了要触发的那个值,而一个**精确采纳**它的 widget 回声回来的就是同一个东西。

**这个标志真正防的是「近似采纳」的 widget**——比如一个把选区规范化进恢复文本里的文本框。那种回声不
等于 `_lastValue`,第一条守卫放行,没有这个标志它就会变成第三个撤销步,于是读者按撤销**看起来什么
都没发生**,因为刚恢复的状态立刻被记成了最新的。改成这个场景之后,把守卫改恒假,测试如期变红。

**「一条恒真的断言比没有断言更糟」的姐妹版:一条恒真的回归行,比没有回归行更糟——它看起来在测。** 这
一轮验证的办法是**把被测的守卫临时改成恒假,看测试会不会红**,以后遇到这种「守卫看着没人触发」的
情况都该这么做一次。

**其余几条:**

* **modifier 在「与 `_lastValue` 比对」之**前**跑**,而这正是它能用的原因:两个只有选区不同的值,
  经 modifier 后是同一段文本,第二个于是被正确认成「没有新东西」。顺序反过来就不成立。
* **控制器把「栈的状态」和「两个动词」分成三个通知源**:按钮听状态决定要不要可用,历史听动词决定
  什么时候动手,一个通知源分不开这两件事。而 `undo()` **先查 `canUndo` 再广播**,于是监听方不必反
  过来问「这次请求是真的吗」。
* **500 毫秒这个数字上游注明是妥协**:「a best fit for the behavior of Mac, Linux, and Windows
  undo/redo state save durations, but it is not perfect for any of them」。三个平台各不相同,而
  一个数得服务三家。
* **被跟踪的值换了对象就清空整个栈**:换了文档就换了历史,留着旧的会让一次撤销把 A 文档的文字换成
  B 文档的。
* **同一个值到两次不算两步**:`_push` 在 init 和「字段拿到焦点」时各跑一次,这是真会发生的。

验证:`cargo test --lib` 2246 绿,GN `rustflutter_unittests` 2246 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1569 accounted / 319 MISSING(83.1%)。

### 竖着滚就按行走,上游自己说这看起来是反的(2026-08-20)

新模块 `two_dimensional.rs`,`widgets/two_dimensional_viewport.dart` 五个全到
(`TwoDimensionalViewport`、`TwoDimensionalViewportParentData`、
`RenderTwoDimensionalViewport`、`TwoDimensionalChildManager`、`ChildVicinity`),外加
`widgets/two_dimensional_scroll_view.dart` 的 `TwoDimensionalScrollView`。覆盖率
1565/1888(82.9%)。

一维视口可以用一个数字给孩子命名,二维的不行,于是每个孩子由一个 `ChildVicinity`——一对
`(x, y)`——命名。**用的词是「邻近」而不是「位置」,是有讲究的:** 孩子可以被摆在任何地方,这对索引
只说明谁是谁的邻居。一张有合并单元格的表,一个孩子覆盖好几个 vicinity,布局**跳过被它吞掉的那
些**,而这不是错误。

**主轴决定绘制顺序,而映射是反的:竖直主轴给出的是行优先。** 上游自己的注释就写着「this seems
backwards」,紧接着给了理由——**竖直是 Flutter 默认的滚动轴,而行优先是矩阵的默认**,要让两个默认
彼此对上,中间的映射就得反过来。所以 `_sortByYIndex`(y 后 x)对应竖直,而 `compareTo` 本身
(x 后 y)对应水平。回归行把两种顺序分别钉住,并额外钉住「两种排序键真的不同」——`(0,5)` 和
`(1,0)` 在两种顺序下的大小关系正好相反。

**可见性是布局之后算的,不是布局当中。** 孩子说它想在哪儿,视口算出这里面有多少落在了屏幕上。完
全在外面的孩子拿到零绘制范围、绘制时被跳过——一万个单元格的表,代价就是屏幕上那四十个。

**`computeChildPaintExtent` 的第一行值得读两遍:宽或高为零的孩子直接不可见,不管它在哪儿。** 否
则裁剪会给一个零面积的孩子算出一个落在视口内的范围,让它看起来是可见的。

**而 `isVisible` 里有一处冗余,原样移植并写下来:**

```dart
return _paintExtent != Size.zero || _paintExtent!.height != 0.0 || _paintExtent!.width != 0.0;
```

**后两个子句永远不会生效。** 第一个为假就意味着范围**就是** `Size.zero`,那么两个维度也都是零。整
个表达式等于「绘制范围不恰好为零」。照抄原文会让读者猜测「是不是在特殊照顾零宽但有高的孩子」——并
没有,那种孩子早在第一个子句就已经算可见了。所以这里写成它实际的意思,再把原文和这段说明留在文
档里。

**`layoutOffset` 和 `paintOffset` 是两个东西,而它们只在 down/right 时相等。** 布局偏移从滚动的前
沿量起,绘制偏移从视口左上角量起;`up` 和 `left` 时要翻回视口坐标。上游文档里那句「覆写 paint 时
请用 paintOffset 而不是 layoutOffset」,正是这一对存在的理由——让这个错误可被发现。回归行把
down/right 相等、up/left 翻转、而布局偏移**始终没动**这三件事一起钉住。

**`reuseChild` 和 `buildChild` 分开,是为了不在每一滚动帧里丢掉孩子的状态。** 而条件里
`needsDelegateRebuild` 那一半才是要点:**delegate 变了会作废每一个孩子,连原地没动的也不例外**
——复用它们会把旧 delegate 的内容显示在新布局里。

**保活桶的进出也是两条判断:** 出桶算 **reuse 而不是 build**(这就是桶的全部意义:滚走了又滚回
来,状态还在);而进桶靠的是「本轮布局**没有**要过它」这个集合差——没要过就是滚出范围了,想留就留
下,不想留就交给 child manager 处置。

**`visitChildren` 会走保活桶,`visitChildrenForSemantics` 不会。** 屏幕阅读器念出读者已经滚过去的
那些行,等于在念一张没人在看的表。

**滚动视图那半边有一个真正的约束而不是偏好:** `primary` 与主轴自带 controller **不能同时成立**,上
游是断言。两个 controller 驱动同一根轴,会各自以为滚动位置归自己所有。

**未移植的部分写在模块头里**:`RenderBox` 那套管线(`performLayout`、命中测试、持有 child manager
的 element)属于本 crate 自己的渲染树,不重复。移植的是 vicinity 与它的两种顺序、parent data 与它
的可见性规则、绘制范围裁剪、build/reuse 判断、保活桶,以及滚动视图的配置校验。

验证:`cargo test --lib` 2224 绿,GN `rustflutter_unittests` 2224 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1565 accounted / 323 MISSING(82.9%)。

### 套接字另一头拿不住一个对象,只能拿住一个字符串(2026-08-20)

新模块 `widget_inspector.rs`,`widgets/widget_inspector.dart` 十个全到:
`InspectorReferenceData`、`WidgetInspectorService`、`WidgetInspector`、
`EnableWidgetInspectorScope`、`DisableWidgetInspectorScope`、`InspectorButton`、
`InspectorSelection`、`DevToolsDeepLinkProperty`、`InspectorSerializationDelegate`、
`WeakMap`。覆盖率 1559/1888(82.6%)。

**这个 service 存在的理由只有一条:套接字另一头的调试器拿不住一个 Dart 对象,只能拿住一个字符
串。** 于是它发 id、自己维护一张表。整个文件的形状都是这一条约束推出来的:

* **id 是分组的**,工具一次调用就能丢掉它看过的全部东西,而不是每检查一个 widget 就漏一份引用。
* **引用要计数**,因为同一个 widget 可以同时在两个组里,谁也不许把它从对方脚下释放掉。
* **表弱持有对象**,检查一个 widget 不该把这个 widget 留住。

**计数的粒度是「组成员身份」,不是「请求次数」**,这一条最容易写错:同一个组里问两次同一个
widget,拿到同一个 id,而计数**不动**。工具序列化一棵在两个位置提到同一个 widget 的树是常态,把
它算成两次,会留下一个 `disposeGroup` 永远归不了零的引用。真正让计数动起来的是**第二个组**来
问——那恰好就是「丢掉一个组不能释放对象」的那种情形。回归行把两边都钉住了。

**两种查找失败是分开的:** id 根本不在表里(工具握着一个过期引用)和 id 在表里但属于别的组(工具
自己有 bug)。上游抛的是两条不同的 `FlutterError`,这里是两个不同的错误值。

**「值为 null」在这里是一个有意义的答案**,不是失败——它表示一个弱持有的对象已经被回收,而 id 还
在表里。工具看到的就是「那个 widget 没了」。所以 `toObject` 对未知 id 抛错而不是返回 null:两者
必须能区分。

**而弱持有有一处不对称,值得写下来:** `WeakReference` **拒绝**字符串、数字和布尔,所以这三类是
**强持有**的。后果是:表里的一个数字被 inspector 留住了,而一个 widget 不会。`WeakMap` 里那个
「原始类型和对象分两张表」也是同一个原因——不是优化,是 `Expando` 根本不收这些键。

**「这是谁的 widget」在没配根目录时是一个猜测,而上游明说是猜的**(TODO 指向
flutter/flutter#32660):判据是「路径里没有 `packages/flutter/`」。这是对的猜法——读者要的是「除了
框架以外的一切」,而要说清楚**哪些是他的**,得有构建系统来回答。配了根目录之后就变成纯前缀比较,
框架那条猜测**完全不再适用**。回归行把这个切换钉住了,顺带钉住「改根目录必须清缓存」——缓存记的正
是一个刚刚改了答案的问题。

**`_shouldShowInSummaryTree` 除最后一条外每条分支都是「显示它」**,这个默认方向是要点:摘要树是个
过滤器,而**一个猜错了的过滤器不该藏起任何东西**。错误节点总显示;不是 diagnosable 的总显示;编译
器没记录创建位置时,**全部**显示——因为根本没法判断这是谁的 widget。

**细节树的深度会「憋着不花」,直到遇见一个也在摘要树里的节点。** 效果是:展开一个节点,会把它底下
那一长串框架 widget 一次展开到下一个读者自己写的东西为止。按层花深度的话,读者要点穿六个
`RenderObjectWidget` 才能看到下一个属于自己的节点。

**其余几条:**

* **`clearCandidates` 不是 `clear`**:它只丢掉命中测试候选,保留选中项。这是为「选择来自 DevTools
  而不是设备上的点击」准备的——陈旧候选是读者上次在屏幕上碰到的东西,把它们画在另一个窗口里选中的
  widget 周围,就是高亮错了对象。
* **选中项的 render object 一旦脱离树,它就不再是一个选中项**(`current` getter 走 `active`),尽
  管字段里还留着它。
* **选择是在手指抬起时提交的,不是按下时**:读者可以在屏幕上移动、看着高亮跟着走,然后再决定。
* **`toObjectForSourceLocation` 把 Element 换成配置它的 Widget**:读者问的是「这东西哪来的」,而
  widget 的类才是他要的答案;element 的类是他没写过的框架细节。
* **`DevToolsDeepLinkProperty` 的名字是空串**,只靠 description 渲染——错误转储里 URL 前面挂个标
  签,是读者必须读过去才能拿到链接的噪音。
* **只有 toggle 变体才有「开没开」这个答案**:上游的具名构造函数对另外两种留 null 而不是给默认
  值,于是一个 filled 按钮不会被读成「关着的」。
* **自己写的 widget 比框架的露出更多**:本地节点按 `fine` 过滤属性,其余按 `info`。读者在调自己的
  代码,不是在调 `RenderFlex`。

**未移植的部分写在模块头里**:VM service 扩展注册、JSON 编码、画选中框的 overlay 都不在——这个
crate 没有服务协议,也没有上游那个形状的 `Element` 树。移植的是**引用表和它的计数、选择状态机、
本地项目判定连同它的缓存,以及摘要树过滤器**。

验证:`cargo test --lib` 2198 绿,GN `rustflutter_unittests` 2198 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1559 accounted / 329 MISSING(82.6%)。

### 一次更新只让最上面那个动,其余的直接就位(2026-08-20)

新模块 `navigator.rs`,`widgets/navigator.dart` 九个补齐(全文件 12 个):`RouteSettings`、
`NavigatorObserver`、`HeroControllerScope`、`RouteTransitionRecord`、`TransitionDelegate`、
`DefaultTransitionDelegate`、`NavigatorState`、`RestorableRouteFuture`、
`NavigationNotification`。覆盖率 1549/1888(82.0%)。

**分量最重的是转场委托:** 声明式页面列表一变,得有人算出谁在进、谁在出,以及——真正需要判断的那
部分——**谁配得上一次动画**。上游的答案是两条规则,而两条都是「不要让没人想看的东西动起来」:

1. **进来的页面压在同位置离场的页面之上。**
2. **只有最顶上那个带动画**,底下的一律直接加入或直接完成。

没有第二条,一次替换掉三页的更新会在读者正在看的那页背后同时播三段转场,看起来就是闪。

**`handleExitingRoute` 是递归的,而它必须递归:** 同一个位置可能一次removed 掉好几页,每一页都记
在上一页之上,全都得出来。回归行钉住了「三页一起走,只有最上面那页带动画」。

**「最后一个」的定义里有个容易漏的与:** `isLastExitingPageRoute = isLast && 它上面没有别的在离
场`。而更值得写下来的是它后面那一条:**一个身上还挂着对话框的页面,自己反而不做动画**——读者看的
是对话框,所以动画归对话框,页面在它底下直接完成。同理,**只有最后一页上的最后一个对话框**才
pop,其余全部 complete。

**上游用「就地改」而我没有别名,于是 `resolve` 收 `&mut`。** 第一版我按值传入,结果
**pageless 记录的决定被决定完就扔掉了**——调用方根本拿不到「那页上的对话框怎么处理」。上游靠的是
navigator 和 delegate 持有同一批对象;Rust 里对应的写法就是把 request 借出去、让调用方回头自己
读。顺手给完整性检查补了一条:**pageless 里还有没决定的,同样算错**——它不出现在返回列表里,下游
谁都不会发现,而结果是页面走了、对话框留在屏幕上。

**完整性检查从 debug assert 变成了返回值。** 上游那三条规则包在 `assert(() {...}())` 里,release
下全部消失,而它们捕获的三件事在 release 下都是静默的:**没决定的路由永远进不来、被漏掉的离场路
由永远出不去、被重排的历史把读者留在错的页面上**。所以这里写成 `Result`,三种错各有名字,各有一
条回归行。同时也钉住了上游明说的自由度:**离场路由可以插在结果的任何位置**(`[D,A,B,C,E]` 和
`[A,B,C,D,E]` 一样合法),只有进场顺序是定死的。

**`canPop` 问的是最底下那个路由,不是最上面那个。** 第一眼像写反了,其实是在回答**只有一个路由**
那种情形:两个及以上时答案横竖是 yes,唯一还开放的问题是「就一页,但它自己还有局部历史可以剥
吗」。

**`maybePop` 的返回值是「这次请求有没有被处理」,不是「有没有弹出」**,而两者只在一处分开:**一
个拒绝了的路由也算处理过了**——路由里有人说了不,调用方就该停止另找他人。只有 `Bubble` 返回
false,那才是让按下抵达平台的那条路。

**`didRemove` 的契约值得读两遍:** 一次移除多个路由时,`previousRoute` 是**最底下那个被移除者之
下**的那个路由——每次都是同一个值——而回调**从上往下**逐个触发。回归行按这个顺序钉死。

**其余几条:**

* **`didChangeTop` 不能由另外四个推导出来**,所以它单独存在:顶部会因为 push/pop/remove/replace
  中的任何一个而改变,只想知道「读者现在在看什么」的观察者,否则得从四个回调里拼。
* **hero 控制器只归第一个拿到它的 navigator**,后来的被挡掉而不是共享——两个 navigator 共用一个,
  就是两段历史对英雄在哪里各执一词。而 `HeroControllerScope.none` 是一个**刻意的句号**:告诉子树
  别继承上面那个。
* **`RestorableRouteFuture` 存的是 id 而不是路由**,这就是它的全部要点:重启后的应用还没有任何路
  由对象,能写进磁盘的只有一个名字。
* **`NavigationNotification` 是往上冒的**,方向就是意义:嵌套导航器或 `PopScope` 向上声明「我这儿
  能接返回」,好让真正被平台询问的那个顶层知道别关应用。
* **`markForRemove` 现在就是 `markForComplete`**:两者曾经差在路由的 future 会不会完成,而「移除但
  不完成」会把等着这条路由的人永远晾在那里。

验证:`cargo test --lib` 2163 绿,GN `rustflutter_unittests` 2163 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1549 accounted / 339 MISSING(82.0%)。

### 视差就是那三分之一,而尺子上那条"约等于"在替我说谎(2026-08-20)

新模块 `cupertino_route.rs`,`cupertino/route.dart` 八个全到:
`CupertinoRouteTransitionMixin`、`CupertinoPageRoute`、`CupertinoPage`、
`CupertinoPageTransition`、`CupertinoFullscreenDialogTransition`、
`CupertinoModalPopupRoute`、`CupertinoDialogRoute`、`CupertinoPageTransitionsBuilder`。
覆盖率 1540/1888(81.6%)。

**iOS 的页面转场,全部内容就是一个不匹配:** 进来的页面滑**一整个**屏宽,被盖住的那页只滑**三分
之一**。差值就是视差。两页滑同样的距离,读起来是传送带上的两页,而不是一页压在另一页下面。

**台账自查,这轮最该写下来的一条:** `fastEaseInToSlowEaseOut` 是 `ThreePointCubic`——两段三次贝
塞尔在一个中点接起来。而尺子的台账里记着 `ThreePointCubic ≈ Curve::Cubic(近似)`。**那条"约等
于"在替我说谎:** 一条穿过 (0,0) 和 (1,1) 的单段三次曲线,做不到在 t=0.198 处就走完 54% 的距离
——那正是这条曲线的全部性格,而接点就是做到它的手段。于是补上真的 `ThreePointCubic`(带
`transform` 与精确的 `flipped`),把台账那条删掉,`animation/curves.dart` 从 covered:1 变成
covered:2。**当尺子夸我的时候去查它**——这是第三次了,前两次是 `IOSScrollViewFlingVelocityTracker`
和宏体盲区。

**手指在的时候,曲线要让路。** `linearTransition` 恰好在返回滑动进行时为真:页面必须待在手指底
下。加了缓动的页面会先落后于自己的拖动、再追上来,读起来像是页面粘在玻璃上而不是粘在手指上。但
**全屏对话框不是这样**——它的主曲线**照用不误**,因为全屏对话框根本没有边缘返回手势,没有手指要
跟。回归行把这个不对称钉住了。

**松手之后往哪走,是三段判断,而第一段最要紧:**

1. **路由已经不是当前的了,就只看它还在不在栈里**——不看速度,不看拖了多远。上游为此引了
   flutter/flutter#141268:一个刚被拖回来几个像素、同时收到程序化 pop 的路由,**仍然应该离开**,
   因为它已经被弹掉了。这时候还去问手指,会把一个半消失的、已经没人拥有的页面放回屏幕上。
2. **甩了一下就只看方向**,不管拖到了哪里。
3. **慢慢松手才回到"过半就留下"。**

还有 **`userGestureInProgress` 要留到归位动画结束**,而不是手指离开时。转场读这个标志决定要不要
走线性,提前放掉会让曲线在归位途中换掉——页面滑回家的半路上出现一个看得见的折点。

**上游那些"目测"出来的数,原样抄:** 页面 500ms(上游写的是「a relatively rigorous eyeball
estimation」)、掉落回弹曲线「rigorously eyeballing native iOS animations」、对话框初始缩放
1.3「mostly eyeballed from iOS」。**一个"差不多对"的转场,比一个明显不同的转场更容易被看出错。**

**边缘阴影是这轮最好的一条性能注解:** 它不是 `LinearGradient`,而是一叠 1 像素宽的矩形,一条一条
lerp 出来。上游 2021-02-08 在 iPhone XR 上量过:编译那个渐变着色器,让一个刚装好的应用做一次页面
转场的最坏帧从 **~95ms** 掉到 **~30ms**。**看起来更笨的代码就是更快的那份,因为开销从来不在绘制
上。** 阴影本身是 `0x04000000`——1.6% 的黑,几乎不算阴影,正是要点:要读出深度,又不能读成一条
线。

**其余判断:**

* **弹簧是临界阻尼的**:`damping = 2*sqrt(522.35) = 45.7099552`。iOS 的表单不会回弹,差一点点就
  会让每张操作表都带上一次看得见的弹跳。回归行同时钉了阻尼比和「六十步内不越过 1.0」。
* **速度容差是被放宽而不是收紧的**(0.03,默认是 1e-3)。iOS 自己的弹簧在宣布结束时速度还有约
  0.02——**说明 iOS 判断结束时根本没看速度**,用默认值反而会一直动到 iOS 早就停下的位置之后。
* **用弹簧而不是曲线**:曲线有固定时长,半路被抓住的表单只能重新开始;弹簧带着它已经在的位置走。
* **告警对话框是"落回原位"而不是"长大到位"**(1.3→1.0);而**退场时完全没有缩放**,只有淡出——一
  边淡出一边涨回 1.3,看起来像它一边离开一边朝读者走来。
* **全屏对话框没有遮罩底色**:它整个盖住屏幕,身后已经没有需要区分的东西了。
* **表单可以点外面关掉,但默认不进语义树**(`semanticsDismissible` 为 false,而
  `barrierDismissible` 为 true):读者用表单自己的控件离开,一个全屏的「Dismiss」目标只会挡在它
  们前面。
* **手势条取安全区内边距和 20 的较大者**:在系统本来就留了宽边距的设备上,20 逻辑像素会把整条手势
  区推到硬件底下。
* `canTransitionTo` 的两条都是「能不能同步」:全屏对话框底下的页面动了也是白动;而只有对方也是
  Cupertino、或者交回了一个可对齐的 delegated transition,这页才值得播退场。

**顺手修掉一个"摆设字段":** `draggable_sheet.rs` 的 `SnappingSimulation::tolerance` 存了却从不读
(编译器一直在提醒)。上游 `Simulation` 基类确实带着它,由驱动方读——所以补一个 `tolerance()`
读取器,而不是把字段删掉。

验证:`cargo test --lib` 2132 绿,GN `rustflutter_unittests` 2132 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1540 accounted / 348 MISSING(81.6%)。

### 一次返回先撕掉一层,页面留到最后一层没了才走(2026-08-20)

新模块 `routes.rs`,`widgets/routes.dart` 十二个全到:`OverlayRoute`、`TransitionRoute`、
`PredictiveBackRoute`、`LocalHistoryEntry`、`LocalHistoryRoute`、`ModalRoute`、`PopupRoute`、
`RouteObserver`、`RouteAware`、`RawDialogRoute`、`RouteBarrierDetails`、`PopEntry`。覆盖率
1532/1888(81.1%)。

**这个文件里最值得写下来的是「谁来接这次返回」,而上游是一条链,每一层都盖住下面那层:**

1. **有 `PopScope` 说不,就不弹**——而且是**先问它、再问局部历史**。一个页面上两个表单,其中一个
   有未保存的内容:另一个愿意走,不该替它把答案花掉;一次只想关掉底部面板的返回,更不该。
2. **其次是路由自己的局部历史**:撕掉一层,页面留着。
3. **最后才是路由本身**,除非它是栈底——栈底把这次按下**还给平台**,应用就此关闭。

第三条和第二条撞在一起时很有意思:**一个栈底路由,只要还有局部历史条目,回答的就是 `Pop` 而不是
`Bubble`**。这正是要点——应用只有一个页面时,返回键该关掉页面上的那个面板,而不是关掉应用。

**局部历史那半边的其余判断:**

* **`didPop` 消化掉这次返回时返回的是 `false`**。读起来是反的,直到把主语点出来:它回答的是「**路
  由**弹了吗」,而路由没弹——弹的是它的一个局部条目。
* **按名字删,而不是从顶上删**:底部面板开着时抽屉关闭,拿走的是它自己那条,从中间。
* **`impliesAppBarDismissal` 和条目数分开计数**:一个只为拦截返回手势而加的条目,没有任何可见的东
  西可撤销,**一个撤销不了任何东西的返回箭头读起来就是个坏箭头**。
* **状态变更只在边界上发**(空↔非空、箭头 0↔1)。第二个面板不改变任何人看得见的东西,为它重建就
  是「每个面板一次重建」。
* **树被锁住时删除会推迟到帧后回调,而那个回调由 `isActive` 把着**:回调跑的时候路由可能已经没
  了,**告诉一个死掉的路由去重建,比不告诉它更糟。**

**`finishedWhenPopped` 是全文件唯一一处答案既不恒定也不显然的地方:**

```dart
bool get finishedWhenPopped => _controller!.isDismissed && !_popFinalized;
```

路由通常先弹再退场,所以弹的那一刻动画还在 1,这里是 false——条目留着,否则读者看到的是页面**凭空
消失**而不是滑走。**但 iOS 的返回滑动会把路由一路拖到 dismissed、且此时它还是 current**,弹是之
后的事;到那时已经没有东西可动画了,条目可以当场撤走。上游自己的注释说得很直白:没有这一条,这
种路由**永远不会被处置**。而 `_popFinalized` 那半边让它只生效一次。回归行把三种情形分开钉住:在
屏幕上的、被滑走的、已经 finalize 过的。

**其余几条:**

* **对话框的遮罩默认可点掉,普通模态路由的默认不可**——对话框是一个问题,**点外面就是读者表示不
  答**。遮罩默认色 `0x80000000`:半透明黑,身后那页仍然读得到。
* **对话框 200ms,页面 300ms**:它出现在读者**已经在看的东西上面**,而不是取代它,要走的距离更
  短。
* **可点掉的遮罩必须有名字**:能点的遮罩就是一个控件,而没有名字的控件,屏幕阅读器无话可说。
* **`canPop` 是一个常驻的答案而不是弹出时才问的回调**,这不是偷懒:平台的预测性返回手势必须在读者
  **开始滑之前**就知道这页会不会走,好把后面那页画出来;回调只能在手势已经开始之后作答。
* **每个 `PopEntry` 都会被告知这次弹出,包括那个说不的**,而且带着 `didPop` 参数——一个表单正是靠
  它在「是我留住了这页」时才弹出「有未保存的更改」。
* **观察者里 `subscribe` 会当场补一次 `didPush`**:一个中途才出现的组件从没见过把它放上来的那次
  push,但它照样需要知道自己在屏幕上;而重复订阅什么都不说,否则一次重建会被读成一次导航。
* **`didPop` 先通知**前一个路由的订阅者(`didPopNext`)、**再**通知被弹路由的(`didPop`)。顺序是
  有意的:正在露出来的那个,应该在离开的那个宣布走掉之前就已经刷新过自己。
* **`unsubscribe` 要走遍所有路由**,因为上游根本没有参数说是哪一个——一个订阅者可能订了好几条;而
  没人监听的路由会被丢掉,免得每访问一页就多留一条空记录。

**未移植的部分写在模块头里**:这些全都是 `Route`,由这个 crate 没有的 `Navigator` 驱动,过渡也由
不是这里交给它们的 `AnimationController` 推动。移植的是**每个路由持有的状态、以及它据此作出的判
断**。

验证:`cargo test --lib` 2093 绿,GN `rustflutter_unittests` 2093 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1532 accounted / 356 MISSING(81.1%)。

### 返回键先问最里面那个,父亲只在没人接手时才管(2026-08-20)

新模块 `router.rs`,`widgets/router.dart` 十二个全到。**覆盖率越过 80%**(1520/1888)。

**内容最多的是返回键分发器,而它那条规则值得单独说:** 一次返回按下,**先递给最近一次被
`deferTo` 的孩子**,再往外走,**父亲只在谁都没接手时才处理**。这才让「壳里的分页里的嵌套导航
器」关掉它自己那个路由,而不是关掉整个应用。

* **孩子集合是有序的**,而且必须有序:被 defer 的先后顺序就是提问的顺序,换成普通集合会让嵌套
  导航器的优先级取决于哈希。
* **`deferTo` 是先删再加**:一个被重复 defer 的孩子会变成最新的,而不是待在原地——这正是「读者
  切回来的那个分页,从他离开的那个手里把优先权拿回去」的实现。
* **`takePriority` 直接清空孩子**:现在是我在答,我之前 defer 过的都不是了。
* **孩子的 `takePriority` 先告诉父亲、再清自己的孩子**。顺序是要点:一个要拿优先权的孩子,必须
  先让父亲 defer 到它,否则从根到它的那条链中间会缺一环,按下去会半路停住。
* **父亲自己没有回调,但只要有孩子 defer 上来,它就「有回调」**;根分发器也据此决定要不要向绑
  定注册——没人能接手的根,没理由去听返回键。
* 没人接手时告诉平台**「应用不要这次按下」**(默认 false),于是应用关闭——这是对的默认:一次没
  有任何人认领的返回,就该离开。

**地址那半边的要点是「什么算同一个地址」:**

* **查询参数是无序比较的**。浏览器有权重排查询参数,把重排过的 URL 当成新地址,会让读者每次回
  到某页都多一条历史记录。
* **state 不参与比较**:state 是应用自己的滚动位置或表单内容,**一个 state 变了的页面并没有变成
  另一个页面。**
* 于是「再次上报同一页」是**替换**而不是压栈,而 `Neglect`/`Navigate` 是调用方直接明说、`None`
  才交给这个比较。比较的对象是**平台当前认为的地址**,不是路由器想要的那个——所以 provider 把两
  者分开存着。
* 空路径就是 `/`:**一个什么都没有的地址是根,不是空白。**

**两个 delegate 的分工也是一条判断:** 解析器管**地址**、delegate 管**页面**。URL 方案可以变而
页面不变,这个切分让两者能各自移动。而 `setInitialRoutePath` 默认转给 `setNewRoutePath` 却单独
存在,是为了让应用能区分「**从这个 URL 打开**」和「**导航到这个 URL**」——一个深链到流程中段的
链接可能要把整条返回栈建出来,而同样的 URL 由导航到达时早就有栈了。回归行把这两种情形分开钉
住。

**自查:** `defer_to` 里我先写了一条 `debug_assert!(... || true)` 想模仿上游的
`assert(hasCallbacks)`——那个条件恒真,什么都不检查。删掉,改成把上游那条规则写进文档。**一条恒
真的断言比没有断言更糟,它看起来像在检查。**

验证:`cargo test --lib` 2068 绿,GN `rustflutter_unittests` 2068 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1520 accounted / 368 MISSING(80.5%)。

### 工具栏必须现在就建出来,而答案要晚一点才到(2026-08-20)

新模块 `text_selection.rs`,`widgets/text_selection.dart` 八个补齐(全文件 11 个):
`ToolbarItemsParentData`、`TextSelectionOverlay`、`SelectionOverlay`、
`TextSelectionGestureDetectorBuilderDelegate`、`TextSelectionGestureDetectorBuilder`、
`TextSelectionGestureDetector`、`ClipboardStatusNotifier`、`LiveTextInputStatusNotifier`。

**那两个 notifier 是有意思的一半,而它们有意思的原因是:它们要回答一个框架没法同步回答的问
题。** 剪贴板里有没有可粘贴的东西、平台提不提供 Live Text,两者都是到宿主的一次往返。**工具栏
必须现在就建出来,而答案要晚一点才到。**

它们做的每件事都由此而来:

* **`Unknown` 是一个真的第三态,不是「还没答案」。** 在答案未知时显示粘贴按钮,可能按下去什么
  都不发生;不显示,又会在一帧之后凭空冒出来。上游把这个状态留着,让调用方自己选。
* **第一个监听者到场时才发问**——没人在听的 notifier 没有理由去跟宿主说话;而最后一个离开时停止
  观察。
* **每次从后台回到前台都重新问一遍**:读者可能在别的应用里复制了东西,别的什么都不会告诉我们。
* **一次失败的询问退回 `Unknown` 而不是留着旧答案**(上游注释:好让它稍后再试)。一个过期的
  `Pasteable` 会留下一个按了没反应的粘贴按钮。
* **在处置之后才到的答案会被丢掉**:上游在 await 前后各查一次 `_disposed`,而**后面那次才是关
  键**——一个在宿主作答期间被处置的 notifier,不能把值写进一个已经死掉的对象。

**Live Text 那个刻意和剪贴板那个不一样:** 它的成功路径和失败路径**都会在「值没变」时提前返
回**。Live Text 可用性是设备的属性、几乎从不变化,一条「它还是原来那样」的通知会让每个工具栏
白白重建一次。回归行把两条路径都钉了。

**回归行盯的其余地方:**

* **工具栏量的按钮比它画的多**:布局和绘制是两个问题,所以一个按钮可以被量过却不画出来——溢出
  菜单正是这么知道自己装了些什么的。
* **force press 是「输入框」的属性而不是平台的**:压感屏上的输入框也可以不要它,而想要它的输入
  框在没有压感的屏上根本不会遇到。
* **滚动之后松手不该把工具栏弹出来**——正好弹在读者刚滚到的那段文字上。
* 一次什么都没改变的点击,**只有被要求时才上报**(`onUserTapAlwaysCalled`)——一个要滚到聚焦字
  段的表单需要这个。
* **手柄和工具栏一起藏**:没有手柄的工具栏,操作的是一个读者已经看不到边界的选区。
* 手柄**按它托着的那一行的高度来定大小**,而两端可以在字号不同的两行上。
* **被拖动的手柄记住它被抓住的那个点**——和拖拽锚点同一个道理:会跳到手指底下的手柄,读起来像
  是换了一个手柄。

验证:`cargo test --lib` 2046 绿,GN `rustflutter_unittests` 2046 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1508 accounted / 380 MISSING。

### 剪切就是复制,外加把选区收起来(2026-08-20)

新模块 `text_editing_intents.rs`,`widgets/text_editing_intents.dart` 二十七个全到——**全树最
大的单文件剩余块**。**accounted 越过 1500。**

**一个文本框不处理按键。** 它声明*意图*——「往回删一个字符」「把选区扩到下一个词」——由一张快捷
键表把击键映射过去。**正是这层间接,让同一份代码在 macOS 上表现得像 macOS 的输入框、在 Windows
上像 Windows 的**:意图一模一样,只有那张表不同。

**两个形状撑起了几乎全部:** `DirectionalTextEditingIntent`(任何带「向前/向后」意味的意图——而
这样的比别的多得多,因为键盘大体上就是一对方向),以及 `DirectionalCaretMovementIntent`(再加
上三个标志,说明光标移动时既有的选区会怎样)。

**那三个标志正是「普通方向键」「按住 Shift 的方向键」和「macOS 与 Windows 各执一词的那些」之
间的全部差别:**

* `collapseSelection`——不按 Shift 的方向键,把选区扔掉、只留一个光标;
* `collapseAtReversal`——按住 Shift 反向时收到光标原处。上游断言它**永不与 `collapseSelection`
  同时为真**,而这两条确实互相矛盾:一个说「压根没有选区」,另一个说「读者掉头时把这个选区收
  起来」;
* `continuesAtWrap`——走过折行末尾时是接到下一视觉行还是停住。两种惯例在这里不同,而**哪一种都
  不是随手定的**。

**剪切就是复制、外加把选区收起来**,上游用「一个类 + 一个私有构造函数」把这句话说了出来。换成
一个单独的 `CutSelectionTextIntent`,处理器就得先把复制那段抄一遍再去删。而 `copy` 是个**常
量**(cause 固定为 keyboard)、`cut` 却要 cause——因为**复制什么都不改**,所以只需要一个;剪切
会改文本,监听者有权知道读者用的是工具栏还是键盘。

**回归行盯的其余地方:**

* 「什么都不做」和「不绑定」是两回事:前者**吃掉**这次击键,上面的人也看不到了。
* `Expand*` 永不收起选区——**另一端不动**,不管读者在拖哪一端。
* 「滚到文档边界」压根不是光标移动:在那些「Ctrl+Home 是滚动而不是移动光标」的平台上,**选区一
  点没变**,所以它是个朴素的方向意图。
* `ExtendSelectionToLineBreakIntent` 是唯一四个参数全收的——Home/End 在四个参数上**每一个**都因
  平台而异。
* 每个无方向的意图都**说明自己为什么发生**:一个「长按出现、按键不出现」的工具栏需要这个答案。
* 一次编辑**描述一个状态而不是假定一个**:意图带着它构建时的那份 value,免得处理器读到一个读者
  早已改过的值。
* 两个 tap-outside 意图**不能互换**:有的平台在按下时收走焦点、有的在抬起时,只听说其中一个的
  输入框没法跟随它所在的平台。

**自查:又踩了一次宏的坑,而且是同一个。** 我把四个删除类/滚动类和三个 cause 类意图**连类型声
明一起**放进了宏里,尺子于是看不见那七个名字(`covered:20 MISSING:7`)。有意思的是**另外十一个
被看见了**——因为我在宏外面另写了 `impl <名字>` 块,而尺子把 `impl` 目标也算作声明。这说明覆盖
是**碰巧**得到的,不是设计出来的。七个类型补成显式声明,宏只留着生成方法,注记写进代码。

验证:`cargo test --lib` 2029 绿,GN `rustflutter_unittests` 2029 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1500 accounted / 388 MISSING。

### 有人扔了一段散文,把它叫成错误就是给它抬身价(2026-08-20)

新模块 `assertions.rs`,`foundation/assertions.dart` 剩下的七个全到:`ErrorDescription`、
`ErrorSummary`、`ErrorHint`、`ErrorSpacer`、`FlutterErrorDetails`、`FlutterError`、
`DiagnosticsStackTrace`。**这一波(诊断)到此收口**:`stack_frame` → `diagnostics` →
`assertions`,三个文件、三十九个类。

**那四个 error 诊断存在的理由,是让一条错误消息是个「有结构的东西」而不是一个字符串。** 这才让
同一个对象既能在控制台里完整展开、又能在 IDE 悬浮里只显示一行、还能在检查器里变成一张卡片。而
**做区分的是等级**:一条错误里恰好有一个 `ErrorSummary`,一行渲染显示的就是它。

**上游那三个 error 行只差一个等级**,其余完全相同(没名字、没分隔符、flat 样式)——所以上游把
它们写成三个一行的子类。`ErrorHint` 排在 `Info` 之上、`Summary` 之下,正是它的地位:**一条提示
比叙述重要,比「到底出了什么事」次要。**

**上游造那句话时区分了四种情形,而这些区分都是真的:**

* **扔出来的字符串叫「message」而不是「error」**——有人扔了一段散文,把它叫成错误就是给它抬身
  价。
* 不是 `Error`/`Exception` 的东西叫「一个 Foo **object**」——「一个 Duration 被抛出了」会读成好
  像 `Duration` 是个错误类型。
* 断言有自己的词,数字有自己的整句(而且**没有第二行**)。

**回归行盯的其余地方:**

* 插值消息**保留各个片段**(上游存成 `List<Object>` 是为了让检查器能把每个插进来的对象做成可点
  的),显示时**中间什么都不加地拼起来**——它们是一句话,不是一个列表。
* **spacer 是一个属性而不是文本里的换行符**:描述里的换行会跟着周围文本一起被缩进和加前缀,根
  本读不出是个空行。
* 没有 summary 级别的部分时,退回到异常的**第一行**并左裁——**退回整段的摘要就不是摘要了。**
* context 是**当作动词短语塞进句子**的(「while laying out the widget tree」)。
* 一条错误**说明是哪个库报的**,免得包里的错被当成框架的。
* 上报走**一个可替换的函数**:测试把它换成收集、应用把它换成上报,框架两边都只调这一个。
* **一次运行里第一条错误完整打印、之后的不**;而 `resetErrorCount` 存在的意义(上游文档原话)
  是让测试框架能让**每个测试**的第一条错误都完整打印。回归行验了「重置的是计数,不是记录」。
* 栈跟踪是**一个带若干行的小节,而不是一个很长的属性值**:属性会被当成一整段折行缩进,就不再
  像个栈了。

**自查:** 「重置计数」那条我把断言写成了 `reported().len() == 2`,可那个测试只上报过一条——是我
的算术错,不是实现错。改成 1。

验证:`cargo test --lib` 2014 绿,GN `rustflutter_unittests` 2014 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1473 accounted / 415 MISSING。

### 那一堆制表符不是琐碎:它是读者判断一棵树在哪儿结束的唯一凭据(2026-08-20)

`foundation/diagnostics.dart` **二十二个全到**。本轮补上剩下的九个:
`TextTreeConfiguration`、`TextTreeRenderer`、`DiagnosticableNode`、
`DiagnosticableTreeNode`、`Diagnosticable`、`DiagnosticableTree`、
`DiagnosticableTreeMixin`、`DiagnosticsBlock`、`DiagnosticsSerializationDelegate`。

`TextTreeConfiguration` 每个字段都是一小段贴在某处的字面文本,整个类型读起来像一张琐事表。**它
不是。** 一份转储的形状,是唯一告诉读者「一个对象在哪儿结束、下一个从哪儿开始」的东西;前缀画
错的树,没人跟得下来。十一种配置的区别**只在这些字符串里**。

**回归行盯的地方:**

* **最后一个孩子收口而不是延续**(`└─` 对 `├─`)——这正是那个凭据。
* **`childLinkSpace` 和它替代的那根竖线一样宽**,于是没有竖线的那一行仍然对得齐;十一种配置逐
  一验过。
* offstage 就是普通的树**换成虚线**,其余一模一样——虚线本身就是全部信息:这棵子树在,但没在显
  示。
* 单行样式**没有换行符可用**(`lineBreak` 是空串)——**这才是它成为一行的原因**,不是某处查了个
  标志,而是压根没东西可断。
* **错误的框无论里面有没有东西都要合上**(唯一带 `mandatoryFooter` 的样式):一个没合上的框会
  撞进控制台接下来打的任何东西。其余样式都没有。
* shallow 就是 whitespace **去掉孩子**;dense 把属性挤进一对括号(这才让大树的密集转储能放进
  一屏);transition 用的是完全另一套分隔符。

**`Diagnosticable` 的默认实现才是有意思的那一半:** 一个对象**不用自己写描述**就能得到一份有
用的描述,因为它已经为检查器列出的那些属性,正是一个字符串需要的那些属性。回归行验了「处在默
认值上的属性不会出现在普通描述里」——上一轮那套等级机制在这里兑现。

**`DiagnosticableNode` 是惰性的,而惰性就是要点:** 为了打印一个对象就把整棵树上每个对象的属性
都建出来,在调试器里慢得会被察觉。

**序列化深度是一级一级花掉的**:检查器是通过一个 socket 跟调试器说话的,而一个真实应用的
widget 树塞不进去。到零时孩子只被命名、不再展开,而且不会花成负数。

**一处坦白的等同:** `DiagnosticableTreeMixin` 在上游和 `DiagnosticableTree` 的唯一区别,是前者
是 mixin、后者是抽象类,好让已经有父类的类型也能拿到这套行为。**在 Rust 里每个 trait 都是
mixin**,所以这里它就是一个别名——**这么说比硬造一个区别更诚实**,而这句话写在代码里。

至此 `foundation/assertions.dart` 剩下的七个(四个 error 诊断、`FlutterErrorDetails`、
`FlutterError`、`DiagnosticsStackTrace`)所依赖的底座已经齐了。

验证:`cargo test --lib` 1997 绿,GN `rustflutter_unittests` 1997 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净,警告数与基线持平。覆盖率 1466 accounted / 422 MISSING。

### `{:.1}` 和 `toStringAsFixed(1)` 的舍入方向不一样(2026-08-20)

新模块 `diagnostics.rs`,`foundation/diagnostics.dart` 二十二个里的十三个:两个枚举、
`DiagnosticsNode`、`DiagnosticsProperty`,以及 `MessageProperty`、`StringProperty`、
`DoubleProperty`、`IntProperty`、`PercentProperty`、`FlagProperty`、`ObjectFlagProperty`、
`EnumProperty`、`IterableProperty`、`FlagsSummary`、`DiagnosticPropertiesBuilder`。

**两个想法贯穿整个文件:**

**一、一个处在默认值上的属性,不是被丢掉,而是被降级。** 它掉到 `Fine`,普通打印器藏起来,而
一个要求「全都给我」的调用方仍然看得见。**一个会悄悄省略东西的转储不可信,而一个列出一百个默
认值的转储没法读**;等级让这两件事同时成立。这也是为什么 `DiagnosticLevel` 的**顺序**就是它的
全部含义:`Off` 排在 `Error` 之上,所以没有任何东西能越过它;`Hidden` 排在 `Fine` 之下,所以
一个属性可以在场却永远不会被误打出来。

**二、一个对自己当前状态无话可说的标志,改为显示自己的名字。** `FlagProperty` 和
`ObjectFlagProperty` 都这么干,而且**两者在同一种情形下同时降为 hidden**——于是那个名字是留给
「我要看隐藏属性」的调用方的后备,而不是普通转储里会出现的东西。否则那一行会是一个光秃秃的
`true`,读者根本不知道它在说什么。

**自查:抓到一处我自己写进去的、静悄悄的偏离。** `format_double` 我第一版直接写了
`format!("{value:.1}")`——可上游是 `toStringAsFixed(1)`,**两者的舍入方向不同**:Dart 是「逢半
远离零」,`0.25` → `0.3`;Rust 的 `{:.1}` 是「逢半取偶」,`0.25` → `0.2`。而我最初那条测试还把
错的行为**钉住了**。改成先用 `f64::round`(它就是逢半远离零)缩放再格式化,`PercentProperty`
也走同一条路径;测试改成钉正确答案,并在实现文档里点名这个陷阱。**写那个显而易见的 `format!`
会让一整类转储和上游差最后一位,而且没人会想到去查。**

**回归行盯的其余地方:**

* 「默认值是 null」和「没有默认值」是两回事,而 null 说不了两件事——所以哨兵是一个独立的变体。
* 等级规则**按上游的顺序**试:`Hidden` 的默认直接胜出(说了「永远别显示」就是永远,连错误也不
  例外);然后是异常(算不出来的属性是这一行上最重要的事);然后是「声明了缺失」的 null;最后
  才轮到「在默认值上」把它降级。
* 双精度**永远一位小数**;`DoubleProperty` 的单位**不加空格**(`16.0px`),而 `PercentProperty`
  的**加空格**——百分号后面直接跟单位会读成一个词。
* 百分比**先夹再乘**:一个略微过冲的动画应当读作 100%——读者被告知的是「走了多远」,而没有比走
  完还远这回事。
* **加了引号之后,空字符串看起来就不空了**(上游原话),所以 `ifEmpty` 是在**引号分支里面**判
  的:`""` 读作一个值,`<none>` 读作一处缺席。
* 父节点要把所有属性挤成一行时,换行符**转义**而不是打出来,否则那一行会变成好几行。
* **空列表不是缺失的列表**,两者给不同的文本;空列表默认不有趣,**除非**调用方给了 `ifEmpty`
  ——那是调用方在说「空本身值得报告」。
* 回调读作 **`has onTap`** 而不是 `true`,而**不存在的回调不值一行**。

**剩下的九个**(`TextTreeConfiguration`、`TextTreeRenderer`、四个 `Diagnosticable`、
`DiagnosticsBlock`、`DiagnosticsSerializationDelegate`)是画树的表格和挂在真实对象上的那些
mixin,留到下一轮。

验证:`cargo test --lib` 1977 绿,GN `rustflutter_unittests` 1977 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1457 accounted / 431 MISSING。

### 过滤器不删帧,它在旁边写一条理由(2026-08-20)

新模块 `stack_frame.rs`:`foundation/stack_frame.dart` 的 `StackFrame` 全到,加上
`foundation/assertions.dart` 里管栈过滤的三个——`PartialStackFrame`、`StackFilter`、
`RepetitiveStackFrameFilter`。**这是诊断那一波的第一步。**

运行时打出来的栈,是一整墙文字,其中大半是关于框架的,而不是关于调用方那个错的。这里做的是
把那墙文字拆成帧,再把没人想读的那些扔掉:异步管道、定时器内部,以及一次构建错误留下的那种
长长的重复段。

**过滤器不删帧。** 它在每个认出来的帧**旁边写一条理由**,由打印器把一串相同的理由折成一行、
说明省掉了什么。**读者仍然会被告知藏起来了多少、以及为什么。**

**回归行盯的地方:**

* 一个普通帧拆成号码、类、方法、包、路径、行、列,而**原样的那一行也留着**,于是一个帧永远能
  按它到达时的样子再显示一遍。
* **方法里的匿名闭包就算作那个方法**——对读者来说它就是,闭包自己没有名字可查。
* 以 `new` 开头的是**构造函数**;类名里带点表示是**具名构造函数**,点后面那截是名字。
* 缺失的行/列是 **-1 而不是 0**,和那两个合成帧用的是同一个「没有位置」,并且区别于「第 1 行第
  0 列」。
* **只有 `dart:` 和 `package:` 两种 scheme 才拆得出包名**;一个 `file:` 帧保留整条路径、包名记
  作 `<unknown>`,因为文件路径没有包可命名。
* **一行解析不了,只赔上它自己**:上游注释说 web 上非 debug 构建会把异常消息打在栈上面,而一行
  读不懂不该让读者失去其余每一帧。
* 部分帧匹配的是**「哪段代码」而不是行号**——行号每改一次那段代码就变。而**包名是模式(子串)、
  类和方法必须相等**:这不对称才让一条过滤器既能指名整个库,又能指准库里的某一个方法。
* 交付这个错误的那套机器(`dart:async`、`package:stack_trace`、`_Timer` 等八项)是**整个删掉**
  而不是折叠的:一个在找自己错处的读者,不该先滚过那个跑了回调的定时器。

**照原样搬的一处上游怪相:** `RepetitiveStackFrameFilter.filter` 的循环边界是
`index < length - numFrames`,**不含最后一个可能的窗口**——一段正好结束在最后一帧的重复,永远
不会被检测到、也就永远不会被折叠;要够到它得写 `<=`。按写的搬,并用回归行钉住:**两个移植版
打出不同的栈,比一个跟着上游怪相的更糟。**

**自查:** 我在「包名是模式」那条里写了 `assert!(!x == false, ...)` 这样的双重否定,读起来是反
的。改成正着写,并在消息里说清那条性质:上游的 `package` 是一个 `Pattern`、用 `allMatches` 匹
配,所以 `flutter` 也匹配 `package:flutter_test/...`。

**这个文件剩下的七个**(`ErrorDescription`、`ErrorSummary`、`ErrorHint`、`ErrorSpacer`、
`FlutterErrorDetails`、`FlutterError`、`DiagnosticsStackTrace`)全都建在
`DiagnosticsProperty` / `DiagnosticsNode` 之上,那是诊断树本身,还没搬——留到那一波。

验证:`cargo test --lib` 1952 绿,GN `rustflutter_unittests` 1952 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1444 accounted / 444 MISSING。

### 那几个位是平台的,不是我们的(2026-08-20)

新模块 `platform_menu_bar.rs`,`widgets/platform_menu_bar.dart` 八个全到:
`ShortcutSerialization`、`PlatformMenuDelegate`、`DefaultPlatformMenuDelegate`、
`PlatformMenuBar`、`PlatformMenu`、`PlatformMenuItemGroup`、`PlatformMenuItem`、
`PlatformProvidedMenuItem`。

**这是 Flutter 唯一不自己画的那个菜单栏。** 在 macOS 上菜单栏属于系统,所以框架的活儿是**把菜
单描述过去**,然后等着被告知读者选了哪一项。这个文件全部是关于那份描述的。

**那几个修饰键的位是平台的,不是我们的**,而且顺序完全不是能猜出来的:meta 是第 0 位、shift
第 1 位、alt 第 2 位、control 第 3 位。框架自己的任何排序都没有透出来。把它们重新编号的移植,
发出去的会是平台读成另一套的快捷键。

**字符型快捷键根本没有 shift 这个标志**,而这是有道理的:`$` 和 `4` 是不同的字符,字符本身已
经说了 shift 有没有按;旁边再放一个 shift 位,就是对同一个问题的第二个、还可能不一致的答案。
上游的 getter 在字符型上返回 null 而不是 false——**「这个问题在这儿不适用」和「shift 没按」是
两个不同的答案。**

**回归行盯的其余地方:**

* 一条快捷键要么送字符、要么送触发键,**绝不两个都送**:平台自有它匹配的办法,给它两个就是让
  这两个有机会打架。
* **修饰键不能当触发键**:「control」这条快捷键会在「control 被按住」和「control 被按下」之间
  歧义,上游的消息直接说改用那几个布尔参数。
* **`enabled` 是算出来的,不是存的**:一项启用,当且仅当应用给了它事情做。**没有办法送出一个
  看着能按、其实按不动的菜单项**——对一个别人来画的菜单来说,这是对的限制。
* 没有 tooltip 就**整个键不送**,而不是送一个空值:平台自己的默认 tooltip,和「没有 tooltip」
  不是一回事。
* 一个组**两边各加一条分隔线**;而**菜单的开头和结尾永远不会横着一条线**——上游是过滤**结果**
  而不是去做类型判断,注释给了原因:组可能和非组交错,而非组自己也可能加分隔线。
* **每一次序列化都取一个新 id,包括一个组的两条分隔线**,两条都映射回那个组;这才让平台能告诉
  框架它碰的是哪一条,哪怕读者根本碰不到分隔线。
* **重设菜单会忘掉旧 id**:它们只对当前在屏幕上的那批菜单有意义,一个陈旧 id 被路由到已经拆掉
  的回调上,正是这一条防住的 bug。
* **清空是送一个空菜单栏,而不是什么都不说**——否则平台会继续画它上一次拿到的那批。
* **一个 delegate 只配一个菜单栏**:两个共用会互相覆盖,读者看到的是后建的那个,而且没有任何
  线索。解锁则宽松些:解一个没锁的没关系,换个人来解不行。
* **平台自带项没有回调,因为平台自己知道**;框架贡献的只是它们摆在菜单的哪个位置。而**只有
  macOS 提供它们**——这不是一个待填的坑,这些就是 macOS 应用菜单的条目,别处没有那个菜单。
* 空的子菜单**送出去是禁用的**:一个里面什么都没有却还能打开的菜单,白费读者一次点击。

**顺带补了四个键常量:** `keyboard/keys.rs` 里原本只有分左右的 `controlLeft` 等,缺了上游那四
个不分左右的同义键(`control`/`shift`/`alt`/`meta`)。补上,并写明上游为什么要把它们分开:键盘
报的是哪一侧,而一条**快捷键**通常不在乎是哪一侧。

验证:`cargo test --lib` 1935 绿,GN `rustflutter_unittests` 1935 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1440 accounted / 448 MISSING。

### 那个下标指的是剥完之后的字符串(2026-08-20)

新模块 `menu_anchor.rs`,`material/menu_anchor.dart` 八个全到:`MenuAnchor`、`MenuBar`、
`MenuItemButton`、`CheckboxMenuButton`、`RadioMenuButton`、`SubmenuButton`、
`MenuAcceleratorCallbackBinding`、`MenuAcceleratorLabel`。

**这个文件里判断最密的一块,恰好是最小的那块:** `stripAcceleratorMarkers` 把 `"&Save As..."`
变成 `"Save As..."` 并说 S 是加速键。它要回答好几个第一版根本想不到要问的问题——`&&` 算什么、
`& ` 算什么、结尾一个孤零零的 `&` 算什么,以及**剥掉标记之后那个下标指的是谁**。

**回归行盯的地方:**

* `&&` 是一个字面的 `&`,**不**标记加速键——`"Search && Replace"` 屏幕上显示一个 `&`,没有下划
  线字母。
* `&` 后面跟空白也不标记:那儿没有字母可以画下划线。
* **只有第一个合格的标记算数**:第二个 `&字母` 照样被剥掉,但不会移动那个下标。一个标签要么有
  一个加速键,要么没有。
* **那个下标指的是剥完之后的字符串**,所以要减去它前面出现过的被引用的 `&` 的个数。搞错了会给
  错的字母画下划线,而且**只在同时含有字面 `&` 的标签里才出错**。
* 标记落在多字节字母上时仍按**字符**而不是字节计下标——上游为此专门用了 `characters`,注释说是
  为了不切开代理对。

**照原样搬的一处上游怪相:** 结尾那个孤零零的 `&`。上游注释说它「就当成一个被引用的 &」,可代
码是直接 `break` 出循环、**没有把它写进去**——于是它消失了,而不是显示出来。按代码搬,并用回归
行钉住,免得日后被当成移植失误。

**自查:一条恒真的测试,和一处有意的偏离。** 我原本写了一条「正则的答案要和循环的答案一致」的
测试——可我把 `has_accelerator` 直接实现成了「剥一遍看有没有下标」,于是那条测试恒真、什么都没
检查。换成**逐种标签形状钉答案本身**。同时把这处偏离写进实现文档:上游有一个正则和一个循环,
两份实现要对同一条规则达成一致,而它们**差一点就不一致**(正则匹配任意位置的 `&x`,循环只为
第一个合格标记设下标);对每个标签它们确实一致,但只留一份实现就没这个问题了。

其余搬过来的是那几个 widget 的配置,以及各自默认值背后的道理:`MenuBar` 默认**不裁剪**而
`MenuAnchor` 默认裁剪(菜单本来就该垂在栏下面);子菜单**允许比父菜单旁边的空间更宽**(菜单
项折成两行比探出去更糟);菜单项按下**默认关掉菜单**;组里的单选**默认不能靠再按一次取消**
(这一组本来就该有个答案)。

**没搬的部分写在模块头里:** `MenuAnchor` 和 `SubmenuButton` 把菜单放进 `OverlayPortal`、用一个
懂路由的控制器驱动,本 crate 两样都没有。

验证:`cargo test --lib` 1918 绿,GN `rustflutter_unittests` 1918 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1432 accounted / 456 MISSING。

### 两个孩子互相指向对方,说明那个点落在它们中间的缝里(2026-08-20)

新模块 `selectable_region.rs`,`widgets/selectable_region.dart` 八个全到。**至此选择这一整套
三层都齐了**:`selection.rs` 是词汇,`selection_container.rs` 把可选区成组,这一层决定**一条被
拖出来的边落在哪个可选区里**,以及拖拽过程中来来去去的孩子怎么办。

**两个问题值得占这些篇幅:**

**一、怎么找到那条边。** 拖拽位置是页面上的一个点,而答案是可选区列表里的一个下标,没有谁掌
握这个映射。做法是问一个孩子、朝它指的方向走——**而当两个相邻的孩子互相指向对方时就停**:这
说明那个点落在它们中间的缝里,谁都不包含它。没有这一条,这个走法会来回踱步、永不终止。

**二、拖到一半才出现的孩子。** 一个在选择过程中被滚动的列表,会构建出完全错过了这次拖拽的可
选区。它们会**各自恰好一次**地收到一条为「最后已知位置」合成的边更新事件,于是加入一个已经在
进行中的选择,而不是空在那儿。

**回归行盯的地方:**

* **一个选择只能被 finalize 一次。** finalize 两次会告诉每个监听者「读者刚刚松手」,而其实早
  就松了;一个据此弹上下文菜单的监听者会弹两次。而**开始一次新拖拽在任何状态下都允许**——读者
  想什么时候重新拉一段就什么时候。
* **从一个没挂上的 notifier 读选择是错误,不是空答案。** 一个空选择看起来和一个恰好为空的真选
  择一模一样,接线接错的调用方永远发现不了。
* 一个 notifier 只归一个 listener;**摘下时连选择一起忘掉**——否则重新挂到新 listener 上的
  notifier,会在新 listener 说话之前先交出旧的那份选择。
* **边界事件覆盖两条边之间的每一个可选区,不管它俩谁大谁小**:一次反向拖出来的选择,起始下标
  比结束下标大,而中间的选词照样要够到全部。
* 落在缝里的边会**走到一个真有话可说的可选区**上;而**跨两个可选区的选择,按构造就是
  uncollapsed**——它至少盖住了它们之间那道缝,不管两边各自怎么说自己。只有两个点来自同一个可
  选区时,它自己的状态才作数,这才让一个段落内部的光标仍是光标。
* 选择中间的可选区可能**没有自己的手柄**(整段被选中时就没有),所以点要**走着找**,而不是直
  接读;走到另一头就停,免得跑出列表。
* **一个还答不上来的孩子(pending)就地停住搜索**——继续往下会落在一个只是因为它还没量过自己
  才显得对的邻居上。
* 合成事件**先发结束边、再发起始边**,这是上游的顺序,而它有讲究:向前拖时正在动的是结束边。
* **离开又回来的孩子算新的**——剪枝让这句话成立,而它必须成立:回来的那个是一个全新的、里面什
  么都没有的可选区。

**自查两处。**

其一,我先写了一个只做恒等返回的 `ResultDefault` 帮助 trait,纯属多余,删掉了。

其二,「一个 notifier 只归一个 listener」那条我一开始写成断言第二次 `register()` 返回
false——可上游在那里是 `assert`,于是 debug 构建下测试自己被断言打死。改成钉**调用方真正能检查
的东西**:`registered` getter。上游给了这个 getter,正是为了这个用途。

**顺带清掉一条我自己留下的警告:** 几轮前把轮子几何从 `cupertino.rs` 搬到 `list_wheel.rs` 之
后,那边的 `max_visible_radian` 只剩测试在用、`project_scale_y` 一个用处都没有了。前者挪进测
试模块的导入,后者删掉。

验证:`cargo test --lib` 1901 绿,GN `rustflutter_unittests` 1901 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净,警告数回到基线。覆盖率 1424 accounted / 464 MISSING。

### 停用的容器不是「选不出东西」,是「根本不在选择树里」(2026-08-20)

新模块 `selection_container.rs`,`widgets/selection_container.dart` 三个全到:
`SelectionContainer`、`SelectionRegistrarScope`、`SelectionContainerDelegate`。接着上一轮搬
好的 `selection.rs` 词汇层。

**一个 `SelectionContainer` 把子树里的可选区从上面的注册表上摘下来,把自己放上去。** 上面于
是只看见一个可选区,而原本有二十个;这一组合起来是什么意思,由容器的 delegate 说了算:全选选
中什么、拖过去是什么效果、复制得到什么。

**另一个构造函数才是更有意思的那半边。** `SelectionContainer.disabled` 没有 delegate,也**什
么都不注册**——于是里面的子树选不出东西,而且**关键在于它也不会被「跳过去」**。一次拖过它的
选择会**停在那里**,而不是从另一头接着往下走;「这一块不是你能拿走的文字」只能是这个意思。

**它的几何是「有内容 + 没有选择」,这一对不是自相矛盾,而正是要点:** 这儿确实有东西,只是选
不了。要是声称「没有内容」,上面的容器会据此认为这块是空的、直接略过去。

**回归行盯的地方:**

* 容器把自己注册到上面那个注册表上,同时把自己发布给子树。
* **显式给的注册表压过树里找到的那个**;两样都没有就谁也不加入。
* **停用的容器哪儿都不注册**,即使上面就有一个;它也**不向子树发布任何注册表**。上游在两个生
  命周期方法后各断言了一次这条不变量。
* 停用的容器对事件答 `None`,**连 delegate 都不问**,几何也用自己那份而不是 delegate 的。
* **换一个等价的 delegate 不会让选择重绘**——上游只在两个 delegate 对几何的说法**不一致**时才
  通知监听者。
* **加监听者需要 delegate,移除监听者不需要**——这个不对称是有意的:一个在容器被停用或拆除时
  把自己摘掉的监听者是再正常不过的事,拒绝它会把一次有序拆除变成一次崩溃。
* scope 按**身份**比较注册表,所以一次把同一个注册表原样传下去的重建,不会让底下每个可选区都
  重新注册一遍。
* delegate 的每个几何访问器上游都先断言 `hasSize`,而且每次都点名同一个原因:**在容器布好局之
  前问它孩子们在哪,不是一个会答错的问题,是一个没有答案的问题。**

验证:`cargo test --lib` 1879 绿,GN `rustflutter_unittests` 1879 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1416 accounted / 472 MISSING。

### 一个回答不上来的可选区,不猜,而是指个方向(2026-08-20)

新模块 `selection.rs`,`rendering/selection.dart` 十七个全到——**这是整棵树里单个文件最大的一
块 MISSING**。

**这个文件是跨 widget 选择文本的那套词汇。** 在一页上拖着选,会同时选中好几个 widget 里的文
字,而它们互相都不认识。把它们串起来的就是这里:一个所有可选区都去注册的**注册表**、一个注
册表往下发的**事件**、以及每个可选区回上来的一个**结果**——这条边归我、归我前面的、还是归我
后面的。

**第三种回答就是整套机制。** 一个被问到自己并不包含的点的可选区**不猜**,它说
「previous」或「next」,容器就朝那个方向去问下一个。于是一次拖出段落的选择,能在**没有任何
人计算全局布局**的情况下找到下一段。

**回归行盯的地方:**

* **竖直位置先问,而且大多数情况一问就定。** 文字是一行行走的,所以一个在上面两行的点就是更
  早,不管它在多右边。先问水平会把一次「往右上方」的拖拽判成往后。只有在矩形**旁边**(同几
  行上)时,水平位置才说了算。
* 矩形外面的平面只切成**两块**,不是四块也不是八块:上方的、以及在它自己那几行上偏左的,都
  算「之前」,吸到前角;其余算「之后」,吸到后角。**这正是文字阅读的样子**,也是为什么
  right-to-left 下换的是那两个角,而不是别的镜像方式。
* **范围记得读者是往哪个方向拖的。** 一个把两个 offset 排序了的调用方,会丢掉下一次按键该往
  哪边扩展。
* **带手柄位置的几何不能同时声称没有选择**——手柄就画在选择点上,两者并存会在屏幕上放一个不
  存在的选择的手柄。
* **「有内容」和「有选择」是两个问题**:空段落有内容没选择,一个一个可选区都没有的容器两样都
  没有。
* **竖直方向的扩展带着起手那一列**(上游的 `dx`)。没有它,光标在一串短行里往下走会一路左
  漂、再也回不来——每个读者都遇到过这么干的编辑器。
* **换注册表要先退出旧的、再加入新的**,顺序反了会让同一个可选区一瞬间同时在两个注册表里;而
  **重复设置同一个注册表什么都不做**,不是「退出再加入」——那会把它从注册表的顺序里摘出来再放
  到末尾。
* `pending` 和 `none` 不是一回事:前者是「等我布好局再问我」,后者是 clear / select-all 的答
  案,没有哪个可选区能「在」里面。

**尺子那一课又用上了一次,而这次的正解不同。** 上游那七个 `SelectionEvent` 子类,我先只写成
了一个枚举——枚举确实是这个封闭集合在 Rust 里的说法,上游自己那个 `type` 字段也承认它是封闭
的——但尺子看不见只作为枚举变体存在的名字,于是那个文件报了 `covered:10 MISSING:7`。补上七个
具名类型、各自 `From` 到对应变体:**枚举是接收方 switch 的东西,子类是发送方给自己要的东西起
的名字**,两者都该在。

**上游一处克制,照搬:** `SelectedContent` 只带纯文本,富文本在上游是个 TODO。**一份声称带着
自己复现不了的格式的选择,比一份老老实实说自己有什么的更糟。**

验证:`cargo test --lib` 1867 绿,GN `rustflutter_unittests` 1867 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1413 accounted / 475 MISSING。

### 一份许可证覆盖四十个包,只存一份(2026-08-20)

新模块 `about.rs`,`material/about.dart` 三个全到:`AboutListTile`、`AboutDialog`、
`LicensePage`,连同上游私有的 `_LicenseData`。

**这个文件里唯一真正有难度的,是包和许可证的关系两头都是多对多**:一份许可证可以覆盖好几个
包,一个包也可以被好几份许可证覆盖。`_LicenseData` 把它存成「一个许可证列表 + 每个包用到哪
几条的下标」,于是**一份随四十个包一起分发的 BSD 文本只存一份**,却能列在四十个包名下。

**下标是在许可证被压入之前记下的,上游管这叫一份「契约」。** 反过来做——先压入再绑定——会让每
个包指向它后面那一条,而这会表现成整个许可证页面列出的全是别人的正文,并且只有在有第二条许
可证之后才显形。回归行拿两条许可证走了一遍,直接比对文本。

**应用自己那个包永远排第一,不管它叫什么名字。** 上游注释说了理由:注册表返回的第一个包就是
本应用的许可证,而一个打开许可证页面的读者,是先找**这个**应用、再找它用到的东西;把它排进
字母序里等于把它埋了。其余的按**不区分大小写**排——否则 `Xml` 会排在 `archive` 前面,读起来像
是根本没排。

**回归行盯的其余地方:**

* 同一个包被提到两次仍然只是一个包,而它的绑定按到达顺序累积。
* 空注册表得到一个空页面,而不是一个错误。
* **应用名兜底到可执行文件名**(两种路径分隔符都认);上游注释还写明了它**故意不做**的事:运
  行中变化的标题不跟踪,需要那个的调用方应该直接传 `applicationName`。
* **版本号不知道就留空,而不是编一个**——上游那里是个 `TODO(ianh)`,说版本该由 embedder 提供而
  现在没法问。**一个 About 框上编造的版本号,比没有版本号更糟。**
* 页面在 **720** 逻辑像素处变成双栏(阈值含等号),而 gutter 也在同一个宽度上变宽,不是另一
  个宽度。
* tile 存在的意义就是打开 dialog,并把拿到的每个字段原样递过去;什么都没被告知的 tile 去问应
  用,它打开的 dialog 也一样。

**没搬的部分写在模块头里:** 页面自身的布局——那个在宽屏上变成双栏的 master/detail flow——是上
游的 `_MasterDetailFlow`,几百行导航器活儿,本 crate 没有路由机制。**但它切换的那个断点搬过来
了,因为那个数字才是那个决定。**

验证:`cargo test --lib` 1849 绿,GN `rustflutter_unittests` 1849 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1396 accounted / 492 MISSING。

### 四秒够读完一句话,不够决定要不要撤销(2026-08-20)

新模块 `snack_bar.rs`,`material/snack_bar.dart` 两个全到:`SnackBar` 和 `SnackBarAction`。
接着上一轮的 `scaffold_messenger`——那边管「这条提示归谁、什么时候关」,这边管条本身。

**一条带按钮的提示条不会自己消失。** 上游的默认是 `persist ?? action != null`,而理由就在时
间里:**四秒够读完一句话,不够决定要不要撤销一件事**,所以一条在问「要不要」的提示条,会等一
个答案。调用方两个方向都能改写这个默认,回归行两边都钉了。

**按钮只响一次,而且守卫在回调之前设。** 一个在提示条正在退场时点了两下「撤销」的读者,否则
会撤销两次,而第二次是他从没要求过的。守卫放在回调前面,于是**连一个会重建、甚至会再点一次
的回调,也放不进第二次**。

**回归行盯的其余地方:**

* **`width` 和 `margin` 是同一件事的两种说法**——都在说「离两边多远」——一条同时给了两个的提示
  条只能忽略掉一个,所以上游直接拒绝。另外两条构造断言(elevation 非负、溢出阈值在 0 到 1 之
  间且两端都含)也一起钉了。
* **一个 `WidgetStateColor` 的背景色本身已经回答了 disabled 那一档**,所以旁边再给一个
  disabled 背景色,是对同一个问题的第二个答案。
* **按钮换行是按「占条宽的几分之几」判的,不是按绝对宽度。** 同一段文案在平板上宽敞、在手机
  上就挤了;阈值的取用顺序是「条自己的 → 主题的 → 默认的」,回归行三级都走了一遍。
* 按下按钮是关闭这条提示的原因,而**关闭理由是可区分的**——这才让一个撤销提示能分清「读者撤
  销了」和「读者由它去了」。
* 四秒、250 毫秒、14 像素这几个数,以及 Material 3 只换了高度曲线这一件事。

**一处写进文档而没有代码的上游细节:** `withAnimation` 的 `fallbackKey` 比看上去重要——两条恰
好构造得一样的提示条,否则会被当成同一个 widget,于是**在第一条按钮上按出的墨水波纹,会继续
在第二条上扩散**。每条一个新 key 才把它们分开。动画本身是 messenger 的而不是提示条的:一个
控制器跑整个队列,这也是提示条被「递」一个动画而不是自己造一个的原因。

验证:`cargo test --lib` 1836 绿,GN `rustflutter_unittests` 1836 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净,警告数与本轮开始时持平。覆盖率 1393 accounted / 495 MISSING。

### 一个提示条不属于它出现在的那个脚手架(2026-08-20)

新模块 `scaffold_messenger.rs`,`material/scaffold.dart` 剩下的六个全到:
`ScaffoldMessenger`/`State`、`ScaffoldGeometry`、`ScaffoldState`、
`ScaffoldFeatureController`、`PersistentBottomSheetController`,连同
`SnackBarClosedReason`。**MISSING 首次降到 500 以下。**

**一个提示条不属于它出现在的那个脚手架**,而属于整个导航器之上的那个 messenger——这才让它能
活过一次翻页:读者还在读的那条,不会因为触发它的代码 push 了一个路由就消失。

**两条规则干了大部分活:**

**一、提示条是队列不是栈。** 第一条还在时再要一条,不会打断它,而是排队。**读者永远不会看到
一条消息的结尾接着另一条的开头。**

**二、嵌套的一组脚手架里,只有根那个显示东西。** 脚手架是会嵌套的——一个页面在一个分页里、又
在一个外壳里——没有这条,一次 `showSnackBar` 会把同一条提示同时放上屏幕三次。而「根」的判定不
是「没有父」,而是「**没有这个 messenger 认得的父**」:父属于另一个 messenger 的脚手架,在这
个 messenger 看来就是根——这正是嵌套 messenger 能拥有自己那批提示条的原因。

**回归行盯的其余地方:**

* **清空保留正在读的那一条,扔掉后面的。** 把当前这条拦腰截断会给读者留半句话;调用方清队列
  要的是「不要再出现新的了」。
* **开了读屏时,关闭动画整个跳过。** 用听的人从那段动画里得不到任何东西,而下一条播报不该被
  它拖着。
* remove 是立刻,hide 是客气地退场;两者在队列空时都不是错误。
* **一条提示条关闭的理由,以第一个给出的为准。** 上游对 completer 加了守卫,因为重复完成会抛
  异常;一条在计时器到点那一刻被划走的提示条,关闭理由是「被划走」——读理由来决定要不要撤销的
  调用方,不能被告知成别的。
* **被缩没的悬浮按钮是「没有区域」,而不是「区域为零」。** 这个区别对那些读几何来避开按钮的
  东西是有意义的:没有区域它就不避了,而一个位于按钮中心的零尺寸矩形会让它继续把那个点当成
  被占着。缩到一半时按钮**朝自己的中心收**,而不是朝原点——这才让缩小读起来像按钮在退后,而
  不是在滑走。
* `copyWith` 里的 `??` 意味着传 `None` 是「保持原样」,不是「清空」。
* 持久底部面板控制器上游只多一个字段,而那个字段是有意思的那个:**这张面板有没有往路由的本
  地历史里放一条**——放了的,系统返回手势会关掉它;没放的,要等触发它的代码来关。
* 脚手架的两个抽屉各算各的。

**记下一条上游的读取时机规则:** `Scaffold.geometryOf` 拒绝在绘制阶段之外被读,错误信息给了
原因——几何是在动画和布局阶段算出来的,在绘制**之前**,所以更早去问会拿到上一帧的答案而毫无
察觉。这条写进了 `ScaffoldGeometry` 的文档。

**自查:** 我在「横幅有自己的队列」那条里写了一行
`assert!(x == false || true)`——一个恒真、什么都不检查的断言,而且它调用的方法在空队列上会触发
debug 断言,反而把测试跑挂了。删掉换成一条真的检查(横幅动完之后提示条仍在)。**一条不可能失
败的断言不值得留着。**

验证:`cargo test --lib` 1824 绿,GN `rustflutter_unittests` 1824 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1391 accounted / 497 MISSING。

### 两个滚动视图之间的那次交接,只经过一个把手(2026-08-20)

新模块 `nested_scroll_view.rs`,`widgets/nested_scroll_view.dart` 九个全到:
`NestedScrollView`/`State`、`SliverOverlapAbsorberHandle`、`SliverOverlapAbsorber` 与它的
渲染对象、`SliverOverlapInjector` 与它的渲染对象、`NestedScrollViewViewport` 与它的渲染对象。

一个「顶栏随着分页列表滚动而收起」的页面,其实是**两个**滚动视图:外面那个放头部 sliver,里
面每个分页一个。这个文件里能完整搬过来、也是本模块主要内容的那部分,是**重叠交接**——外层那
个钉住的头部,怎么告诉内层它遮住了屏幕顶部多少。

* **吸收器**包住钉住的头部,把它的遮挡量**从外层自己的滚动范围里减掉**,记在把手上。
* **注入器**坐在内层顶上,把那么多空白**放回去**,于是内层列表的第一项从头部下面开始,而不是
  被压在头部底下。

**两者从不见面,只共用一个把手。** 这正是「头部住在一个滚动视图里,而它需要的空间出现在另一
个里」得以成立的原因。

**吸收器那两次减法就是全部要点:** 遮挡量从**滚动**范围里减掉,于是外层不再以为自己有那么多
内容可滚;也从**布局**范围里减掉,于是它后面那个 sliver 从「头部不再遮住的地方」开始,而不是
从「头部不再绘制的地方」开始。**它画的东西一点没动**——头部照样盖着屏幕顶部,只是这笔账不记在
这儿了,因为马上要记到内层去。

**注入器那两个夹取相差一个 scroll offset,而这个差就是整个「头部收起」的效果**:paint 范围不
动,layout 范围缩小,于是内层列表是往上滑到头部**底下**去,而不是把头部顶走。

**记下一处上游前后不一致。** 注入器的 `performLayout` 把 `_currentMaxExtent` 从把手的
`layoutExtent` 赋值,而 `attach` 和 handle setter 里却拿它和把手的 `scrollExtent` 比。吸收器
往两个字段写的是同一个数,所以这处永远不会显形——但**一个两个 extent 不相等的把手,会让注入器
永远处于脏状态,每帧重新布局却永远安顿不下来**。按写的搬,并且专门写了一条回归行**把这个后果
变成看得见的**,而不是留一句评论了事。

**回归行盯的其余地方:**

* 空的吸收器什么都不吸;而一个**不遮挡任何东西的普通 sliver 原样穿过**——这才让整段头部列表可
  以放心地被包起来。
* 视口太矮时两个 extent 都被夹住,**但滚动范围仍是整个空隙**。
* 把手能认出**同一个把手被交给了两个吸收器**——这个错误否则会表现成头部在两个高度之间闪,却
  找不到原因;`toString` 那三种情况里有两种其实是诊断(没有主人 / 主人不止一个),都留着了。
* **是「标记把手」把消息带过了那道缝**:吸收器在外层布局时写把手,可没有任何东西会让另一个滚
  动视图里的注入器自己重新布局,视口的 `markNeedsLayout` 去标记把手才是那根线。
* 换把手时**通知新的那个而不是旧的**:正在听新把手的人,才是布局刚刚变得不对的那个。
* 浮动头部要等**内层被滚动过**才肯回来——内层还在顶上时,它没有可浮在上面的东西。

**没搬的部分写在模块头里:** 上游的 `_NestedScrollCoordinator`——决定一次拖拽该动两个 position
里的哪一个、以及一次甩动怎么在两者之间越过去的那几百行——是搭在 `ScrollActivity`、
`ScrollActivityDelegate` 和 `ScrollHoldController` 上的,这三样本 crate 都没有。

验证:`cargo test --lib` 1809 绿,GN `rustflutter_unittests` 1809 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1385 accounted / 503 MISSING。

### 往上一甩的读者,意思就是往上(2026-08-20)

新模块 `draggable_sheet.rs`,`widgets/draggable_scrollable_sheet.dart` 四个全到:
`DraggableScrollableController`、`DraggableScrollableSheet`、
`DraggableScrollableNotification`、`DraggableScrollableActuator`,连同上游私有的
`_DraggableSheetExtent` 和 `_SnappingSimulation`。

**这张纸的尺寸是屏幕的一个分数,不是若干像素**,后面所有事都是从这个决定长出来的:同一张纸
在手机上是半屏、在平板上也是半屏,而一个说「开到 40%」的调用方,两边的高度都不必知道。回归
行用同一次 80 像素的拖拽在 800 和 400 两块屏上给出不同的分数把它钉住。

**两块地方带着判断:**

**一、`_SnappingSimulation` 决定松手后飞向哪个吸附尺寸,而那不是「最近的那个」。** 只要有真
正的速度,纸就去速度方向上的那个,哪怕另一个更近——**往上一甩的读者,意思就是往上**;因为他
起手时离下面更近就把他摁回去,读起来像是纸在跟他较劲。只有几乎没有速度的松手才取最近的。

**二、`hasChanged` 决定一次重建保住谁的位置。** 纸只要动过,重建就保住**读者**的位置(夹进新
边界);没动过,才取调用方新的初始尺寸。于是改 `initialChildSize` 会挪动一张没人碰过的纸,而
不动一张碰过的。

**自查:两条测试期望写错,实现是对的。**

* 「慢速松手取最近」那条我用了 1.0 px/s,以为算「没速度」。可 `Tolerance::DEFAULT` 的速度容差
  是千分之一,1.0 早就是一次真甩了。顺着这条我把**容差从哪儿来**写进了实现文档:上游是用
  `physics.toleranceFor(position)` 构造这个模拟的,在 1 倍屏上约每秒二十像素,**不是**
  `Tolerance` 默认的千分之一——手指没有静到每秒千分之一像素,用默认值那条分支基本永远走不到。
* 「不过冲」那条我从 100 起手,而 100 正好是一个吸附尺寸,于是目标就是 100 本身——这正是上一
  条回归行钉的规则。改成从 110 起手。

**回归行盯的其余地方:**

* **第一次布局之前的拖拽被丢掉**,而不是去除以零:上游在 `availablePixels == 0` 时直接返回。
* 夹住之后落回原处的移动**什么都不说**。
* 通知同时带着**起始尺寸**和当前尺寸——这才让监听者能区分「被拖回了起点」和「什么都没发生」。
* **两头永远是吸附尺寸**,不管调用方有没有列出来(否则读者拖到顶会被甩回列出来的最高那一
  档),而调用方列了的话也不会重复。
* **已经停在吸附尺寸上的纸就待着**,甩得再狠也一样。
* **速度的方向取自目标而不是那一甩**:上游注释直说,很慢的一甩可能吸到与运动方向相反的一边。
  回归行用「微弱上甩却向下吸」把这条钉住。
* 给了 `snapAnimationDuration` 就**完全替掉速度**:纸在那段时间里走完这段距离,最低速度不再
  适用——调用方已经说了该用多久。
* 吸附**到点即停**,不过冲。
* 控制器的移动是一次 change 但**不是一次 drag**:这对标志的意思是「动过,但不是读者动的」,
  于是吸附保持关闭,一张被调用方放在 43% 的纸就待在 43%。
* `reset` 把纸送回去,并且**把「动过」这件事也忘掉**;actuator 的复位标志是**取走即清**的,
  之后再重建多少次也不会再复位一遍。

验证:`cargo test --lib` 1796 绿,GN `rustflutter_unittests` 1796 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1376 accounted / 512 MISSING。

### 慢到的那个回答,常常是先问的那个(2026-08-20)

新模块 `autocomplete.rs`,`widgets/autocomplete.dart` 八个全到:`RawAutocomplete`、六个
intent、`AutocompleteHighlightedOption`,连同上游私有的 `_RawAutocompleteState`。

**这个文件里两件事比它们看上去更要紧:**

**一、选项构造器是异步的,于是可能同时有两个答案在飞,而先问的那个不能赢。** 上游给每次调用
编号,任何不是最新那次的回复都丢掉。读者打字快的时候,**慢到的那个回答常常就是先问的那个**
——没有这个编号,输入框会停在一个读者早就打完的前缀的选项上。回归行把「新的先回、旧的后回」
这一顺序整个走了一遍。

**二、高亮是夹住的,不是绕回去的。** 在最后一项上按向下,还停在最后一项。这是个决定而不是遗
漏:会绕的列表,会把一个按住方向键的读者悄悄送回顶部。

**回归行盯的其余地方:**

* **移动光标不构成重建选项的理由**——上游只在**文本**真的变了时才取新编号,否则一次选区变化
  就会把读者刚移好的高亮扔掉。
* **把选中项写进输入框,不能看起来像打字**:上游用 `_selecting` 挡住,因为 `_select` 会把选
  项的文本写进控制器,而那次写入本会为读者没打过的字再跑一遍构造器。
* **重复选中同一项什么都不做**——连输入框都不重写,因为重写会挪动光标。
* 选中之后再编辑,**选中项就不再是选中项**了。
* 一页是**四项**,一个固定数字而不是一屏的量:调用方的选项视图可以是任意高度,没有「一屏」可
  问。到两头照样夹住。
* **没东西可显示时,方向键不被吞掉**——六个处理器全都以 `_canShowOptionsView` 为门,于是没有
  匹配项的输入框会把方向键让给树里其他人。
* 列表要**同时有焦点和有内容**才显示;失去焦点总是关闭它;选项变空也关闭它,并且高亮归零。
* 语音播报只在**空与非空之间跨越时**才发,而不是每敲一个键。
* 回车只在列表开着时取走高亮项;Esc **关掉开着的列表,否则把这次 dismiss 让给别人**——所以输
  入框后面的对话框在第二次 Esc 时仍会关。
* 「第一项/最后一项」的按键随平台在 ⌘ 和 Ctrl 之间换,而四个裸键(上下箭头、翻页)哪儿都一样。

**一处形状上的交代:** 上游那六个 intent 是 `Intent` 的六个子类,action map 按类型索引。本
crate 的 `Intent` 是一个封闭枚举,所以这六个作为各自的类型放在它旁边,另配一个
`AutocompleteIntent` 枚举做分发——「这是一个固定的集合」这句话,换成 Rust 的说法。

**没搬的部分写在模块头里:** 选项视图在上游是 `OverlayPortal`,按输入框的绘制变换定位;本
crate 没有 overlay。输入框和列表本来就是调用方要建的,上游的 `fieldViewBuilder` 和
`optionsViewBuilder` 也是这么规定的。

验证:`cargo test --lib` 1778 绿,GN `rustflutter_unittests` 1778 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1372 accounted / 516 MISSING。

### 一个还在离开的行,仍然占着一个位置(2026-08-20)

新模块 `animated_scroll_view.rs`,`widgets/animated_scroll_view.dart` 八个全到:
`AnimatedList`/`State`、`AnimatedGrid`/`State`、`SliverAnimatedList`/`State`、
`SliverAnimatedGrid`/`State`,以及上游私有的 `_ActiveItem` 和
`_SliverAnimatedMultiBoxAdaptorState`。

**这个文件的全部难处,是两套对不上的下标**,而上游自己的注释说了为什么会有两套:

> `insertItem()` 和 `removeItem()` 的 index 参数,是按「`removeItem()` 立刻把对应条目移走」
> 来定义的。条目其实要等移除动画放完,才真的从 ListView/GridView 里消失。

于是「这是第几行」有两个答案:**index** 是调用方的意思(列表将来的样子,移走的行已经不在
了),**itemIndex** 是 sliver 看到的(列表现在的样子,正在缩下去的行还占着位置)。

**两个方向的比较符不一样,而且是有意的**:正向用 `<=`,反向用 `<`。反向只会被问到一个**不
在离开**的行——上游为此专门写了断言——所以「同一位置上有个正在离开的行」在那边不可能出现;而
在正向,那正是最常见的情形。这条不对,一个调用方连着说两次「删掉第 3 行」就会删同一行两次。

**回归行盯的地方:**

* 一个还在离开的行**仍然占着一个位置**:sliver 数到 5,调用方数到 4。
* **连着两次「删掉第 3 行」删的是两行**——这正是调用方那套下标存在的理由:一个一行行清空列表
  的调用方,不该需要知道有多少行还在做动画。
* **一次插入会推动所有活动中的行,包括正在离开的**——正在出去的行和正在进来的行一样占位置。
* **一个还没进完就被删掉的行,从它当时的位置往回退**,而不是拿一个满值的新动画:否则一次被
  取消的插入会先「弹」到全尺寸再缩回去。
* 正在离开的位置由**移除时给的那个 builder** 来建,因为调用方自己的 builder 已经不知道那个
  下标了;而安顿好的行拿到的是一个**已完成的动画而不是没有动画**(上游给的是
  `kAlwaysCompleteAnimation`,一个真对象而不是 null),这样 builder 永远不用判空。
* **位置要等动画放完才消失**,消失时它后面的每个活动行下标减一——那一刻这一行的两套下标才重新
  合到一起。
* `insertAllItems` 往**上**走(每次插入把后面的推开,所以 `index + i` 每次都是同一个地方),
  `removeAllItems` 往**下**走(每次删除都用调用方的下标表达,先删第 0 行会把后面全重新编号);
  而且它是从**可见**行数开始数的,所以已经在离开的行不会被删第二次。

**自查:尺子看不见我写的类,而尺子是对的。** 头一版我把四对 widget/state **连类型声明一起**
放进了宏里,`coverage.py` 于是一个都看不到——它读的是声明,而它不去展开宏调用是对的。改成把
八个类型明明白白写出来、宏只生成方法。**一个把自己类名藏起来的移植,是尺子查不了的移植**;这
条注记留在了代码里。这也是本轮 accounted 从 1356 一步到 1364 的原因:先前那一版跑出来是 0。

**顺带一处自查:** `ActiveItem::removed_item_builder` 头一版是个永远没人填的
`Option<Rc<dyn Fn(f32) -> ()>>`——一个装样子的字段。改成真的 `-> AnyWidget`,由 `remove_item`
接进来、由 `build_leaving` 用出去,这才是把上游那个字段搬过来而不是比划一下。

验证:`cargo test --lib` 1761 绿,GN `rustflutter_unittests` 1761 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1364 accounted / 524 MISSING。

### 拖拽时算出的下标,和放下时该用的下标,不是同一个(2026-08-20)

新模块 `reorderable_list.rs`,`widgets/reorderable_list.dart` 六个全到:
`ReorderableList`、`ReorderableListState`、`SliverReorderableList`、
`SliverReorderableListState`、`ReorderableDragStartListener`、
`ReorderableDelayedDragStartListener`。

**这个文件的难处几乎全在一个每帧都要回答的问题上:被拖的那一行现在在这儿,它该落到第几位?**
上游在 `_dragUpdateItems` 里回答,而答案**不是「最近的那一行」**——是一组关于**移动行的两条边
落在每个静止行的哪一半**上的规则。这被搬成 `insert_index_for`,一个纯几何函数,可以单独推敲。

**第二难的是:这个答案有两个意思。** 拖拽进行时,插入下标是**把被拖的行仍算在列表里**算出来
的;真正放下时,那一行先被移走,于是它后面的每个下标都往前挪一位。`reordered_index` 就是这
个修正,**而它正是可重排列表的经典错法**:漏掉它,行会比读者放的位置多走一格,而且只在一个
方向上错,所以随手一试根本试不出来。

**自查:我头一版的三条测试期望都写错了,实现是对的。**

* 「往下拖第 0 行」我以为答案会变成 1,实际是 **2**。因为把被拖行也算在内时,「排在第 1 行前
  面」就是第 0 行现在待的地方——**从 0 出发,下标 1 是到不了的**。这看着像个差一错误,直到把上
  面那个修正算进来:落在 2 → 重排成 1,往下挪一格,正是读者做的事。测试改成把这两步一起钉住。
* 「把行拖走再拖回来」我用第 0 行试,可第 0 行要回到自己那一格,得把代理拖到起点**上方**去。
  换成第 3 行:拖到 5,再拖回 3。
* 横向那条我原本用「同一批数据竖着读等于没动」当对照,可那批行的竖直跨度完全重合,竖着读会
  被第一个非拖拽行认领。换成一个**阶梯**(横向间隔 40,竖向间隔 100),两条轴才真的给出不同
  答案:横着是 2,竖着是 0。

**回归行盯的其余地方:**

* **走的顺序不能改变答案。** 上游那两个 `newIndex <` / `newIndex >` 守卫看着多余,其实不是:
  行是按 map 顺序走的,后面的一轮不能推翻前面已经得出的结论。回归行把同一批行正着走和倒着走
  的结果对比,守卫没了就过不去。
* 被拖的行**能认领回自己的下标**——上游专门给它留了一个分支,因为其余每个分支说的都是「别的
  行」。
* **间隙只在被拖行和它当前落点之间打开**,区间外的行完全不动;反向列表两个方向都反过来。
* **上游的两个重排回调拿到的不是同一组数字**:`onReorder` 拿原始的一对、只要不同就调用;
  `onReorderItem` 拿修正后的一对,而修正后相等时**不调用**——那种情形是一行绕了一圈又落回原
  处。
* 「从下方回到原位」读出来是 `index + 1` 而不是 `index`,上游注释给了原因(插入下标是把被拖
  行算在内算的),没有这条分支,放下的动画会瞄偏整整一行。
* 第二根手指**不会继承第一根没做完的拖拽**;**列表项数一变就取消进行中的拖拽**(上游文档说
  得直白:任何大的列表改动之前都该先取消,免得拖拽被脚下移动的行搞糊涂);**不在屏幕上的行
  不能起拖**(上游在这里抛异常,还留了个 TODO 问能不能改成滚过去)。
* 两个监听器**只差在用哪个手势起拖**:立即的那个给拖拽手柄,延迟的那个给整行——在滚动区域
  里,立即拖拽和滚动是同一个手势,只有时间能把它们分开。这一条正好接上前几轮搬的
  `DelayedMultiDragGestureRecognizer`。

**没搬的部分写在模块头里:** 拖拽代理在上游是 `OverlayEntry`,边缘自动滚动是
`EdgeDraggingAutoScroller`,两者本 crate 都没有。它们都是关于**怎么显示**这次拖拽,而不是关于
**它意味着什么**,而后者才是这个模块带着的东西。

验证:`cargo test --lib` 1746 绿,GN `rustflutter_unittests` 1746 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1356 accounted / 532 MISSING。

### 台账说它无关,可它是另一个有关的东西的基类(2026-08-20)

新模块 `recognizers.rs`,加上 `gestures.rs` 里的两个平台甩动速度追踪器。**gestures 层到此只
剩一个类没搬**:`FlutterErrorDetailsForPointerEventDispatcher`,它等的是 `FlutterErrorDetails`
(诊断那一波),跟手势无关。

搬进来的六个,彼此没什么共同点,除了都很小,每个就是一个想法:

* `EagerGestureRecognizer`——**故意什么都不识别,进哪个仲裁场就赢哪个**。上游把它交给嵌入的
  平台视图:那个视图对「一次触摸是什么」有自己的看法,仲裁场在这里的活儿就是尽快让开。
* `GestureArenaTeam`——让几个识别器**共用一个座位**,于是它们跟外面的世界竞争,而不是互相
  竞争。
* `ForcePressGestureRecognizer`——看屏幕被按得多重。
* `VerticalDragGestureRecognizer` / `HorizontalDragGestureRecognizer`——拖拽识别器的轴向策
  略:什么算甩动,以及允许它多快。

**台账审到一处自己写错的判定。** `IOSScrollViewFlingVelocityTracker` 早先被我以「iOS 专属,
宿主集合为 Windows/Android/macOS」为由记进了 `out_of_scope_classes`。**可 macOS 就在宿主集合
里**,而 `MacOSScrollViewFlingVelocityTracker` 正是它的子类。一个被台账判为无关的类,原来是
一个有关的类的基类。两个都搬了,那条 out-of-scope 记录删了。这不改 accounted 的数字(out 本
来就算进去),但它是一句假话,删掉才对。

**两个平台追踪器的形状本身就是内容。** 普通的 `VelocityTracker` 是对位置做最小二乘二次拟
合——那是「这根指针有多快」的正确答案。这两个回答的是另一个问题:「平台自己的滚动视图会甩
多快」,做法是取三段相邻的两点速度加权平均。**两个都把最新的那一段贴近扔掉**(iOS 给二十分
之一,macOS 给五分之一),因为那一段是手指在抬起,不是读者在甩;而它们对该信哪一段并不一
致——iOS 最重的权在**最旧**的一段(0.6),macOS 在**中间**那段(0.65)。谁也不比谁更对:各自
对齐各自平台的滚动视图,而这就是它们唯一的用处。回归行用「最后一刻停死」把 950 和 800 两个
数钉住,权重写错就过不去。

**回归行盯的其余地方:**

* 上游为四个采样点保留二十个,注释说了原因:**要让 `VelocityEstimate.offset` 大到能过甩动判
  定的「距离」那一半**。速度和位移是**故意**在不同跨度上量的。
* 一个队伍只占一个座位;**先接受的成员赢**,其余被告知输了;**队长替全队赢**,不管是谁先看
  见的;一个成员退出**不交出座位**,直到最后一个也走了——否则一个成员改主意就把手势送给队伍
  外面的东西了。没人说话时**加入顺序就是决胜规则**,所以同样一批成员换个顺序组队,行为并不
  一样,这值得知道而不是撞上。
* 甩动要**又快又真的走过一段**。距离那一半最容易漏,漏掉代价也最大:手指在松开那一刻原地抖
  一下,瞬时速度可以很高而位移是零,没有距离判定,这一抖就把列表甩出去了。
* 力度按压:**测不了力的屏幕不参赛**(上游判 `pressureMax <= 1.0` 后直接判负,而不是去争一个
  它永远检测不到的手势);**独自赢下仲裁场还不算按下去**——上游为此专门留了 `accepted` 这个
  状态;峰值只报一次而更新继续。
* 插值对越界压力做钳位、却让 NaN 原样穿过,上游给的理由是:设备把压力报到自己声明的范围之外
  时,识别器仍该正常工作。

**照原样搬的一处上游怪相:** 力度识别器里 `event.delta.distanceSquared > computeHitSlop(...)`
——**平方的距离对没开平方的 slop**。这让它比名字暗示的更早放弃(slop 18 时,约 4.24 逻辑像素
就够)。按写的搬,并在代码里点名。

验证:`cargo test --lib` 1727 绿,GN `rustflutter_unittests` 1727 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1350 accounted / 538 MISSING。

### 一个手势识别器同时是点击和拖拽,因为它们分不开(2026-08-20)

新模块 `tap_and_drag.rs`,`gestures/tap_and_drag.dart` 九个全到:
`BaseTapAndDragGestureRecognizer`、`TapAndHorizontalDragGestureRecognizer`、
`TapAndPanGestureRecognizer`、`TapAndDragGestureRecognizer`(已废弃的那个),连同上游的
`_TapStatusTrackerMixin`。

**文本框没法把点击识别器和拖拽识别器并排放着**——两个会为同一根手指打架,而仲裁场必须在任何
一方弄清楚发生了什么之前先选一个。所以上游把它们焊在一起:一个识别器既报点击、又报**这是
连着的第几次**,而手指要是继续走就变成拖拽。那个计数就是全部意义所在:一下放光标、两下选
词、三下选段,而从任何一次拖出去,都按那一级的粒度扩展选区。

**这里最值得记的一条是它和 `MultiTapGestureRecognizer` 做了相反的选择,而理由也是相反的。**
MultiTap 在按下时就发 `onTapDown`,不等仲裁场——琴键得在有人判定之前就亮。这个识别器等到赢
下仲裁场才发——光标不该被一根其实在滚动的手指挪走。两条回归行摆在一起。

**回归行盯的地方:**

* 连着三下数成 1、2、3;隔太久、换地方、换键,都从头数;`maxConsecutiveTap` 到顶时**开新的
  一串,而不是报一个没人处理的第四下**。
* **超时是懒判的。** 上游的计时器回调是**故意空的**,注释说清了原因:计时器可能在 tap
  down/up 还没发出去时就到点,那时重置会扔掉回调还要用的状态。所以过期在下一次按下时才被注
  意到。回归行专门验了「计时器到点本身什么都不重置」。
* **一次拖拽会打断这一串。** 计数本身没清,但让下一次点击能并进来的那两样东西清了,于是它
  并不进来——点一下、再点住拖一下的读者,并没有三连击。
* 按住不动过了 `kPressTimeout` 就报 tap down(手指只是搁着,光标也该落下);而**第二下按住不
  放会直接宣布拿下手势**,理由是上游自己写的:否则一个双击选词、然后停顿一下再拖的读者,会
  把选区输给长按识别器。第一下按住则不会——它还可能真是长按。
* 横向识别器**在赢下之前不数竖向位移**(这才让文本框能待在竖向滚动区里);**赢下之后,任何
  轴上的拖拽它都接**——那时没别人在竞争了,手势总得是点什么,而只剩拖拽。
* 两条轴用的阈值不同:横向对 hit slop,平移对 pan slop(两倍远)——不受约束的拖拽,要比已经
  被限制在一条线上的更果断才算数。
* 更新同时报「刚移了多少」和「一共走了多远」;没赢下就走太远的指针被 cancel 而不是被当成点
  击;而**tap down 没发出去过时,cancel 什么也不说**——没人被告知过的东西,取消它是噪声。
* 节流的拖拽**在结束时仍会把最后一个位置送出去**:上游是取消定时器并立刻跑它,而不是丢掉,
  因为一次拖拽的最后位置正是决定选区终点的那个。
* 关掉 `eagerVictoryOnDrag` 后,识别器发现了拖拽也不吭声,等着被交过来。

**自查抓到一个真实现错误。** 「第二下按住宣布拿下」那条第一次跑出来是 `Rejected`。原因是我
让 `stopTrackingIfPointerNoLongerDown` 去调 `_giveUpPointer`,于是一次 tap up 走了两遍放
弃流程,第二遍时指针已不在已接受列表里,便排了一条 Rejected。上游不是这样:
`stopTrackingPointer` 只摘路由,而它**带守卫**——最后一个指针只能停止跟踪一次。补上
`tracked_pointers` 和那道守卫之后就对了。**实现改了,测试没改。**

**顺带修掉一处仓库里早有的重复:** `BUILD.gn` 的源文件列表里 `tap_region.rs` 出现了两次。

验证:`cargo test --lib` 1699 绿,GN `rustflutter_unittests` 1699 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1344 accounted / 544 MISSING。

### 文档说「立刻」,代码说「永不」(2026-08-20)

新模块 `multitap.rs`,补上 `gestures/multitap.dart` 剩下的两个:
`MultiTapGestureRecognizer` 和 `SerialTapGestureRecognizer`,连同上游私有的 `_TapTracker`
和 `_TapGesture`。

**两个都在数点击,而它们对「第二根手指是什么意思」的回答正相反**,这就是它们的全部区别:

* `MultiTapGestureRecognizer` **每根手指各算一次点击**。上游自己的例子最清楚:down-1、
  down-2、up-1、up-2 产生**两次**点击,一次在 up-1、一次在 up-2。钢琴键盘要的是这个。
* `SerialTapGestureRecognizer` 数的是**一串**里的第几次,而中途来的第二根手指不是延长这一
  串,是终结它。文本框「单击选词、双击选段」要的是这个。

**上游的文档和上游的代码在这里对不上。** `longTapDelay` 的文档写着「默认是
`Duration.zero`,意思是 `onLongTapDown` 在 `onTapDown` 之后立刻被调用」;而构造函数里那行
是 `if (longTapDelay > Duration.zero) { _timer = Timer(...) }`——**默认值下根本不会建这个
timer,`_dispatchLongTap` 永远到不了**。按代码搬,用一条回归行钉住;两者之中,代码才是所
有现有调用方一直跑着的那个。

**回归行盯的地方(MultiTap):**

* 上游那个例子,一字不差。
* **`onTapDown` 在按下时就发,不等仲裁场**——等仲裁场意味着琴键在手指已经离开之后才亮。
* 一次点击要**赢下仲裁场**和**手指抬起**两件事,谁先谁后都测了:赢了不够(手指还按着),
  抬起也不够(仲裁场没说话前,这一下仍可能属于某个拖拽)。
* 一根走远的手指**只取消自己那一次**,别人的照样成立。
* **长按报的是手指现在的位置,不是按下时的位置**;而且长按**不结束这次手势**,抬手时那次
  普通点击照样发生。

**回归行盯的地方(SerialTap):**

* 连着三下数成 1、2、3。
* **没有任何回调的识别器不参赛**(`isPointerAllowed`)——否则它会进每一个仲裁场,还可能赢
  下来,把手势从真会处理它的那个识别器手里夺走。
* 换个地方点、换个键点,都是新的一串。
* **两下挨得太近是同一根手指在闪**:上游 `hasElapsedMinTime` 的注释说触摸屏常常断续地检测到
  触摸,所以 40ms 以内的第二次按下是硬件噪声,不是读者点了两下。
* **第二根手指终结这一串而不是延长它**——两根手指不是一次双击。
* 一串会自己超时,然后从 1 重新数。
* **被取消的第三下报的是「第三」。** 上游在 `_rejectPendingTap` 里专门标了语句顺序:取消要
  在通知仲裁场**之前**发,因为通知仲裁场可能重入 `reset`,而那会把取消要报的那个计数清掉。
* 一个被判负的指针**连带整串一起结束**,不只是它自己那一次。

**顺带清掉的两条警告:** 上一轮搬家留下的 `RenderListWheel` 比 `ListWheelViewport::render`
更私有(提为 `pub`——它本来就是一个搬过来的渲染对象),以及 `cupertino.rs` 里只剩测试在用的
`HitTestResult` 导入(挪进测试模块)。

验证:`cargo test --lib` 1682 绿,GN `rustflutter_unittests` 1682 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1340 accounted / 548 MISSING。

### 没有数量的轮子,也没有下界(2026-08-20)

`ListWheelScrollView`、`ListWheelElement`、`ListWheelViewport` 三个补齐,
`widgets/list_wheel_scroll_view.dart` 十个全到。

**先做了一次搬家。** 轮子的柱面几何(五个投影函数和 `RenderListWheel`)原先私藏在
`cupertino.rs` 里、只归 `CupertinoPicker` 用;上游它在 `rendering/list_wheel_viewport.dart`
和 `painting/matrix_utils.dart`,比用它的 widget 低一层,而**上游的 `CupertinoPicker` 本身
就是搭在 `ListWheelScrollView` 上的**。现在两个 widget 都要用,它就该在下面那层。搬完
`cargo test --lib` 1652 原样绿。

搬家顺带修掉一处:`RenderListWheel` 原来把 `PICKER_PERSPECTIVE` 写死在里面。上游的
`RenderListWheelViewport` 一直把 `perspective` 当参数,只是这个渲染对象过去只有一个调用方
所以看不出来;共享之后它必须是参数。picker 传的仍是同一个值(它的常量本来就钉在
`RenderListWheelViewport.defaultPerspective` 上)。

**`ListWheelElement` 的实质是那份缓存的纪律。** 上游它是个 Element 兼 `ListWheelChildManager`,
有规矩的是后半边:**一个下标只建一次并记住,一次 rebuild 忘光,而「这里有没有孩子」是靠建
它来回答的**——无界 builder 的尽头就是这样在布局中被发现的,而不是事先声明的。本 crate 的
`AnyWidget` 不能 clone,所以缓存拆成两半:**「是不是有」**要能被渲染对象反复问而不重建,
**「它是什么」**只能交出去一次。

**`_minEstimatedScrollExtent` 是我自己写错又被上游改回来的那一处。** 我第一版给循环轮子的
metrics 配了 `min = 0.0`,只把上界放成无穷。跑出来 -40 的偏移被夹回 0,报第 0 项而不是第 11
项。翻上游:**没有数量的轮子,负无穷和正无穷两头都是无界的**。给它配个 0 的地板,读者往上
一转就被顶住了——而这恰恰是循环轮子存在的理由。实现改了,测试没改。

**回归行盯的地方:**

* 同一个下标问三遍「有没有」,delegate 只被叫一次。
* 无界 builder 的尽头**靠问出来**;循环 delegate **根本没有数量**。
* 一次 rebuild 先清缓存,再**沿着活着的那一段从头走到尾**,把已经不存在的尾巴松开——上游
  走的是「区间」而不是「集合」,一个数量缩水的 builder 正是这个情形。
* viewport 拒绝上游拒绝的每一组参数,**并且带着上游那句解释**:直径为零的柱面没有面可画;
  透视高过百分之一会在 z 上被裁掉;而**「画到视口外面」和「裁到视口」是各自都合理、放一起
  自相矛盾**的一对——裁剪扔掉的正是另一个要的东西。
* 挤压(squeeze)让同样高度里活着的项更多。
* **新开的轮子不会宣布自己开在哪一项上**:上游从 controller 的 `initialItem` 播种
  `_lastReportedItemIndex`,否则一个开在第 7 项的轮子建好就报第 7 项,而听「变化」的调用方
  会去处理一件读者根本没做的事。
* **循环轮子报的是项,不是它转了多远**:走 delegate 的 `trueIndexOf`,不然读者转过头会被告
  知选中了「十二项列表里的第 137 项」。反方向那条同时又把 Dart 取余那件事钉了一遍。
* 报告时机由 `changeReportingBehavior` 决定,且上游的默认是话多的那个;同一项不会说两遍。
* 甩动走的是上一轮刚搬来的 `FixedExtentScrollPhysics`,**不是** picker 至今仍用的 ease-out。

验证:`cargo test --lib` 1664 绿,GN `rustflutter_unittests` 1664 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1338 accounted / 550 MISSING。

### Dart 的取余和 Rust 的取余,差的是一整个「上一格」(2026-08-20)

新模块 `list_wheel.rs`,上游 `widgets/list_wheel_scroll_view.dart` 十个里的七个:四个
delegate、`FixedExtentMetrics`、`FixedExtentScrollController`、`FixedExtentScrollPhysics`。
另加 `physics.rs` 里的 `FrictionSimulation::through`。剩下三个(`ListWheelScrollView`、
`ListWheelElement`、`ListWheelViewport`)是轮子本身的柱面投影,下轮再说。

**这里其实是两件可以和轮子几何分开的事:** 孩子从哪来(四个 delegate 回答,其中循环那个让
轮子没有头尾),以及**一次甩动允许停在哪**(`FixedExtentScrollPhysics` 回答:永远不停在两
项之间)。

**Dart 的 `%` 和 Rust 的 `%` 不是同一个运算。** Dart 对负的被除数返回非负余数,Rust 保留被
除数的符号——`-1 % 5` 在那边是 4,在这边是 -1。而一个从第一项往上拖的循环轮子,问的正是这些
负下标。**两个运算符的差别,就是「显示最后一项」和「下标越界」的差别。** `rem_euclid` 才是
Dart 的 `%`,回归行把 -1、-5、-6 三个都钉住了。

**回归行盯的地方:**

* 循环 delegate **没有数量**(上游用「未知」表示无穷),普通列表有——轮子据此知道自己没有
  两头。
* 空的循环列表什么都不给,而不是去除以零。
* 无 count 的 builder **靠 builder 说「没有了」来收尾**;有 count 的**根本不会问范围外的
  下标**——一个 builder 有权很贵。
* **`_getItemFromOffset` 是先夹再除。** 一个被拖过头还按着的轮子报的是最后一项,不是它后面
  那一项:读者眼睛盯着的就是最后一项。
* **容差是按设备像素算的,不是逻辑像素。**「近到可以停了」应该意味着「近到读者看不出来」,
  而读者看得出来的是物理像素,所以屏幕越密停得越准。
* `jumpToItem` **不检查范围**,上游文档明说;这不是疏忽——轮子的可滚动范围要到布局才定下来,
  布局才是把它拉回来的那一刻,回归行把这个先后顺序走了一遍。
* 五个场景各一条:站着不动的给 `None`(否则就是每帧算一次「别动」);**太弱的甩动用弹簧滚
  回本项**(得用弹簧,因为运动要反向,而摩擦只会顺着原来的方向跑);真正的甩动**恰好落在一
  项上**;两头则**边界压过网格**——列表到头时,把读者放回列表里比对齐格子重要。

**「恰好落在一项上」那条的阈值是量过的。** 同样这一下甩动,若用普通摩擦会停在 161.83,离第
4 项差 1.83;把断言收到 0.5,这条线才真的在分辨「调过阻力的」和「没调的」,而不是走个形式。

**补上了一处记在案的缺口的一半。** `CupertinoPicker` 的文档一直记着
`FixedExtentScrollPhysics.createBallisticSimulation` 的场景 5(`FrictionSimulation.through`)
没搬,拿一小段 ease-out 顶着。`through` 现在在 `physics.rs` 里了:普通摩擦是给定阻力问它停在
哪,`through` 是给定停在哪反解阻力,同一个方程反过来解。**但 picker 本身还没改接过去**——它
仍用 ease-out 走到同一个目标,落在同一项上,只是路径不同。文档已按实情改写,改接是这件事剩
下的那一半。

验证:`cargo test --lib` 1652 绿,GN `rustflutter_unittests` 1652 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1335 accounted / 553 MISSING。

### 同一把尺子,三个说「是」,一个说「不是」(2026-08-20)

新模块 `multidrag.rs`,上游 `gestures/multidrag.dart` 六个全到:`MultiDragPointerState`、
`MultiDragGestureRecognizer`,以及 `Immediate`/`Horizontal`/`Vertical`/`Delayed` 四个识别器。

**这一族存在的理由,是每根手指各算各的。** 普通的拖拽识别器盯的是「那个」指针——先到的那根
拥有整个手势,第二根要么被忽略、要么被并成缩放。这里每个指针各自判定、各自开始、各自结
束,于是棋盘上两根手指可以同时拖两个子。

**四个识别器只在两件事上不一样**,其余全是共用的一台小状态机:

* **多大的移动算数**:任意方向够远、横向够远、竖向够远,或者(延迟那个)在延迟走完之前**没
  怎么动**;
* **拖拽什么时候真正开始**:立刻,还是停在那里等延迟走完。

**同一把尺子,三个说「是」,一个说「不是」。** 前三个读到的是「够远了,这是拖拽」;延迟那个
读到的是「太远了,这已经不是按住不动了」——同一个 slop 比较,相反的判决。这正是可重排列表
能在滚动区域里用延迟识别器的原因:在那儿,提前移动说明读者在滚动,识别器该让开。

**回归行盯的地方:**

* 两根手指同时拖两样东西,各自收到各自的 update。
* **被识破之前的移动,会一整块交给客户端。** 用两次都不到 slop、加起来才过线的移动钉住
  (12 和 14 对 18):要是不补这一块,东西会先不动、然后在下一次普通移动时凭空跳这么远。
* 横向识别器**不数竖向的位移**,反之亦然——这才是它能待在竖向列表里的原因。
* **延迟与仲裁场的接受,谁先谁后是不定的。** 手指还按着时场就把手势交过来,和按住先走完,
  两种顺序都测了;**后到的那一个才真正启动拖拽**,而客户端两种顺序下都只被告知一次。
* `on_start` 返回 `None` 表示「这个不要」,指针随即丢掉,而不是白盯着。
* **接受晚于抬手不是错误**——上游自己的注释就说这条,一次快速点击就会走到。
* 只有主键、且**只有主键**能拖:上游的默认过滤是相等而不是掩码,左右键一起按也不算。

**自查:** 头一版的那条累加测试我算错了距离(12 与 9 合成 15,根本不到 18),实现是对的,
测试是错的。改成 12 与 14 之后它反而更硬——两次单独都不过线,合起来才过线,累加真的被逼着
干活了。

**一处顺带的可见性改动:** `gestures.rs` 里的 `Disposition` 由 `pub(crate)` 提为 `pub`。这个
模块要说出它准备告诉仲裁场什么,而那正是这个枚举的意思;另造一个同义的枚举只会让两处判决
的词汇对不上。

**没搬过来的那部分,写在模块头里:** 上游每个识别器自己进仲裁场;本 crate 的仲裁场长在
`GestureRouter` 里、按路由器自己的识别器种类编号,外面写的识别器没有座位可坐。于是状态把
「本该告诉场的判决」记下来,由调用方 `take_resolution` 取走。上游那个文件的实质是那台状态
机,而状态机是整搬的。

验证:`cargo test --lib` 1635 绿,GN `rustflutter_unittests` 1635 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1328 accounted / 560 MISSING。

### 一次斜着的甩动,多半是在滚动(2026-08-20)

`Dismissible` 和 `DismissUpdateDetails`(上游 `widgets/dismissible.dart`),连同
`DismissDirection` 和上游那个私有的 `_FlingGestureKind`,进 `drag_target.rs`——挨着同一族
的拖拽放着。

**甩动要过两道条件,而第二道才是在列表里真正管用的那道:**

* 够快(每秒 700 逻辑像素以上),并且
* **沿着轴的分量要比另一个方向多出 400**。

一次快到能过第一道的斜甩,多半是一个读者在滚动、手指顺带偏了一点;要求轴向分量把另一个方
向甩开 400,正是让他的滚动不至于删掉一行的那条。

**从静止开始的甩动不算甩动。** 上游的注释值得整段留着:在正中间松手时,「我们认为用户是想
把它甩回中间,而不是想先把它拖到一边、再甩过中间、从另一边出去」。

**回归行盯的地方:**

* 上面两条,以及**慢的直甩**也不算。
* **朝反方向甩会把项送回去**,不管它已经被拖出多远——**读者的最后一句话,压过他手指碰巧停在
  哪儿**。这条用「拖出九成、反手一甩,照样回来」钉住。
* 没有甩动时才由**位置**说了算,两个方向都算。
* **阈值是 0.4 而不是一半。** 项在还没走到一半时就提交,因为到了一半读者已经看不清自己在删
  什么了;四成足够表明是有意的,又还近到看得见那个东西。
* 竖向的 dismissible 读的是另一个速度分量,而**横着甩不掉一个竖向的项**。
* **更新详情同时带着「现在过没过线」和「上一次过没过线」。** 上游文档原话是这对值在那儿是
  为了「抓住那一刻」。一个想在提交时震一下手机的调用方要的是**边沿**而不是电平,而从一串电
  平里自己算边沿,等于让调用方去存一份 widget 本来就有的状态。

验证:`cargo test --lib` 1621 绿,GN `rustflutter_unittests` 1621 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1322 accounted / 566 MISSING。

### 把东西拿起来的时候,它在手指的哪儿(2026-08-20)

新模块 `drag_target.rs`,上游 `widgets/drag_target.dart` 五个全到:`Draggable`、
`LongPressDraggable`、`DraggableDetails`、`DragTargetDetails`、`DragTarget`。

**这个文件里值得知道的,大多是关于「东西在空中时」的**:它相对手指坐在哪儿、原地还留不留
一份、以及什么样的手势才算「拿起来」。正是这几个决定,让一次拖拽感觉像在搬一个物体,而不
像一个 widget 在光标底下冒出来。

**两种锚点策略,而它们的分别是「拿起」和「贴上」:**

* `childDragAnchorStrategy`(默认):反馈保持手指在**它内部**的位置和原来在 child 里的一
  样。**抓住卡片右下角就是从右下角把它拎起来**——它不会先跳到手指底下,而「先跳一下」正是
  让一次拖拽读成「换了个东西」而不是「移动了这个东西」的原因。
* `pointerDragAnchorStrategy`:反馈的左上角贴到指针上。给那些**不是 child 副本**的反馈用
  (比如一个「你正拿着什么」的小徽章)——child 上没有对应的点可以保,徽章就挂在指尖上。

**回归行盯的地方:**

* 上面两条,包括子锚点下反馈**恰好还盖在 child 原位上**。
* **`maxSimultaneousDrags` 的 0 是区别于「无限制」的第三种状态**:调用方用它禁用拖拽,而不
  必把 widget 换成另一个。
* **锁了轴的拖拽会把偏离那条线的位移丢掉**,于是可重排列表里的项是沿着列表滑的,不会跟着手
  指乱走。
* **反馈不参与命中测试**——不然空中那个东西就成了手指底下那个东西,每一个放置目标都会被正
  要放上去的那一项挡住。
* **`affinity` 和「长按」是从两头解同一个问题的。** 在可滚动区域里面,**一次立即开始的拖拽
  和一次滚动是同一个手势**。affinity 按**方向**把它们分开;长按按**时间**分开。可重排列表
  需要后者,因为它的项移动的方向正是列表滚动的方向,方向分不出来。
* **落在空处的拖拽照样报告它结束了**——一个要把被拒绝的项动画送回原位的应用,不管有没有人
  接住,都需要这个结束事件。
* **目标同时留着「会接的」和「会拒的」两份**:第二份才让一个目标能说「这个不行」,而不只是
  「没亮起来」。
* **递给目标的位置是**指针**的,不是反馈的**:一个要在两行之间插入的列表得知道手指在哪个缝
  上,而反馈的角可能在任何地方——子锚点下它就在这一项当初被抓住的那个位置。

验证:`cargo test --lib` 1613 绿,GN `rustflutter_unittests` 1613 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1320 accounted / 568 MISSING。

### 一个跟着手指,一个躲开手指(2026-08-20)

`TextMagnifier`(`material/magnifier.dart`)和 `CupertinoTextMagnifier`
(`cupertino/magnifier.dart`),进 `magnifier.rs`。三个 magnifier 文件全部收口。

**上游这两个 `StatefulWidget` 的全部工作就是一个方法**
(`_determineMagnifierPositionAndFocalPoint`)。这里把那个方法做成一个「手势 + 屏幕 → 位
置」的纯函数;上游 state 里剩下的是一个监听器、一个计时器和一个 `AnimatedPositioned`,它们
什么都不决定。

**Material 那一套:**

* **水平跟着手指,但绝不离开这一行。** 拖过行尾的话,读者会在自己正要放光标的文字旁边看到
  一片空白。
* **垂直跟的是光标不是手指**:手指可能在下面任何地方,而放大镜属于那一行。
* **只有垂直移动才做动画。** 沿着一行滑动应该感觉是黏在手指上的,所以 x 直接跟;跳到另一行
  应该读作一次跳跃,所以 y 走缓动。而**第一次出现从不做动画**——否则它会从上一枚碰巧在的地
  方滑过整个屏幕。
* **被顶出屏幕上边时,焦点同样挪那么多。** 不然一枚贴着上边缘的放大镜会开始显示错误的那一
  行:为了留在屏内做的位移,必须在它「看哪儿」上抵消掉。
* **比放大镜自己视野还窄的字段,焦点就定死在中间。** 它无论如何都会看到字段外面的东西,于
  是上游不再尝试——**一个固定的错,比一个来回滑的错好读。**

**iOS 那一套是另外三个决定,每一个都是 iOS 在做 iOS:**

* **它是躲开,不是跟随。** 往下拖过 48 像素,放大镜整个消失——拖到那么下面的读者已经不在瞄
  文字了。
* **它抵抗向下拖拽。** 镜片绝不升到行中线以上,而向下时以手指十分之一的速度走,于是它落在
  后面、保持可读,而不是追着跑。
* **它不为屏幕做垂直重定位。** 上游把边界在垂直方向上撑开一整个镜片高度,好让
  `shiftWithinBounds` 只约束 x——注释里直说了。**iOS 的放大镜可以跑出屏幕上边,Material 的
  不行**,这条两边并排断言了一次。
* 而**镜片落后多少,焦点就补回多少**——这正是它能落在手指后面、却仍然显示光标所在那一行的原
  因。

验证:`cargo test --lib` 1604 绿,GN `rustflutter_unittests` 1604 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1315 accounted / 573 MISSING。

### 手指盖住的正是最该看见的地方(2026-08-20)

新模块 `magnifier.rs`:`widgets/magnifier.dart` 五个全到(`MagnifierInfo`、
`TextMagnifierConfiguration`、`MagnifierController`、`MagnifierDecoration`、
`RawMagnifier`),外加两个平台的 `CupertinoMagnifier` 和 `Magnifier`。

**放大镜存在,是因为手指盖住了它正指着的东西。** 触摸屏上读者最需要看见的那一处——光标会落
在哪儿——恰恰就是指尖压着的那一处,所以系统把那块抠一份出来,举到手指上方给他看。

**所以这个文件里几乎所有东西说的都是「位置」而不是「放大」。** 倍率就一个数;其余全是让这
枚放大镜留在屏幕上、盖在对的那一行上、并且别挡着自己的算术。

**回归行盯的地方:**

* **贴边时是滑过去,不是缩小。** 一枚靠近边缘就变小的放大镜,会在读者正好在边距上干活时改
  变它展示的文字量。
* **两个轴各自独立判断**:同时越过左边和下边的放大镜是**一步斜着挪回去**的,而不是被先测到
  的那个轴钳住。
* 本来就在里面的一点都不挪。
* **「关掉」是没有 builder,而不是一个开关。** 上游的 `disabled` **就是**默认构造出来的那
  一个,而它的 `magnifierBuilder` 回落到一个返回 null 的函数。所以一个没有放大镜的平台**什
  么都不提供**,而它上面每一个输入框都是对的——一个条件判断都不用写。
* **手柄默认画在放大后的图像里面。** 听着像是杂乱,其实不是:读者正在拖的就是那个手柄,一
  枚把被拖的东西藏起来的放大镜,会把文字给他看、却不告诉他自己拖到哪儿了。
* **倍率小于 1 是缩小,而上游允许**——断言只针对 0。想要一个字段的广角视图的调用方并没有做
  错什么。而 0 保留成断言而不是钳位:0 会把源上每一个点塌到同一个点,画不出有意义的像,也
  不存在一个「他其实想要的」邻近值;钳位只会把 bug 藏在一枚看着有点怪的放大镜后面。
* **两个平台的放大倍率是同一个 1.25**,尺寸和形状却不同——这条值得知道:**放大镜不是用来把
  字变大的,是用来把字从手指底下抬出来的。**
* **iOS 的是个圆头槽、Material 的不是**;而**两边都把镜片举到手指上方**(iOS 用一个负的
  offset,Material 用一个正的焦点位移——同一个动作的两种写法)。
* `MagnifierInfo::EMPTY` 存在,是因为一个 notifier 在第一次触摸之前也得拿着一个:一旦有东
  西必须持有它,就不存在「没有 info」这回事,零值就是那个替身。

平台层那两个 `TextMagnifier`(把 `MagnifierInfo` 流变成实际位置的那两个有状态 widget)留到
下一轮。

验证:`cargo test --lib` 1595 绿,GN `rustflutter_unittests` 1595 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1313 accounted / 575 MISSING。

### 一个 alpha 决定整页怎么排(2026-08-20)

`ObstructingPreferredSizeWidget`、`CupertinoPageScaffoldBackgroundColor`
(`page_scaffold.dart`,该文件 3/3 收口)、`CupertinoNavigationBarBackButton`
(`nav_bar.dart`)、`CupertinoCheckbox`(`checkbox.dart` 收口)。

**为什么「知道自己多高」不够,还要多问一句。** `CupertinoPageScaffold` 要决定一件事:正文
是从这条 bar **下面**开始,还是从它**底下穿过去**?两种都对,而哪种对取决于只有这条 bar 知
道的事——它透不透。**不透的 bar** 会挡住从它后面过去的东西,正文必须从它下面起,否则第一行
永远看不见;**半透的 bar** 恰恰是要被穿过去的:内容在它后面移动时的那层模糊就是效果本身,
而从它下面起的正文会在那儿留下一条该有效果的空白。

而 `CupertinoNavigationBar` 给的答案就一句:**背景完全不透明才算遮挡**。**一个 alpha 检查
决定整页怎么排**——一个把 bar 调了哪怕一点透明度的调用方,已经不言而喻地要求内容从它底下穿
过去了。

**回归行盯的地方:**

* 上面那条,包括**差一档的 0xFE 仍然算透**。
* **三态复选框先走完两个确定答案再到不确定那个**:上游的循环是 false → true → null,而顺序
  就是要点——null 排在 true **之后**而不是夹在中间,所以连点的读者会先经过两个确定答案,才
  到那个意思是「维持原样」的。
* **普通复选框永远到不了不确定态**:没有 `tristate` 时,null 不是读者能走到的值、只是应用能
  设的值,所以循环停在两个上。上游那条构造断言(`tristate || value != null`)也在。
* **按下去的叠加色随明暗反过来**:同样的 15%,浅底上是黑、深底上是白,这样一次按压在两种底
  上都读得出来是按压。
* **`_assemble` 出来的返回按钮不带自己的颜色和回调**:那是上游在转场途中用的两片形态——尖角
  和标签**分开建、分开带 key**,因为(上游注释原话)它们在页面转场时**各自动画**:去页的标
  题要变成来页的返回标签,两样东西干一件事,一个 widget 干不了。

`CupertinoPageScaffoldBackgroundColor` 存在,是因为**孩子有时得自己把页面的颜色画出来**
——bar 的模糊要知道自己在模糊什么,一行想看着像页面上的一个洞就得用页面的填充色去填。问主题
拿到的是**主题的**背景,而 scaffold 一旦被指定了自己的颜色,那就不是同一个东西了。它只有
`maybeOf` 没有 `of`:不在任何 scaffold 里的 widget 有一个挺好的答案,就是「去问主题」。

验证:`cargo test --lib` 1585 绿,GN `rustflutter_unittests` 1585 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1306 accounted / 582 MISSING。

### 三段高亮要看着像一段(2026-08-20)

五个小类,五个 cupertino 文件:`CupertinoUserInterfaceLevel`(+它的枚举)、
`CupertinoIconThemeData`、`CupertinoFocusHalo`、
`CupertinoPickerDefaultSelectionOverlay`、`CupertinoLinearActivityIndicator`。

**`CupertinoPickerDefaultSelectionOverlay` 那两个 cap 是这一批里最好的一条。** 日期选择器
是好几个 picker 并排,而中间那条高亮带必须读成**一条**。所以中间的列**两端都不封口**,第一
列只封起始端,最后一列只封结束端——三块分开的 overlay 才看着是一条。每一列都两端封口的话,
画出来是三颗中间带缝的药丸。而且**边距只跟着封口走**:没封口的那一边一点边距都不留,因为那
正是这一列的带子和下一列接头的地方——在那儿留边距,恰好就是封口安排本来要避免的那道缝。

**回归行盯的地方:**

* 上面那条(第一列 / 中间列 / 末列 / 独一列四种),以及边距和圆角**只出现在封了口的那一
  边**。
* **没有变化的图标主题答的是它自己。** 上游写的是
  `resolvedColor == color ? this : copyWith(...)`——这不是为省而省:每次 resolve 都答一个新
  对象的话,它和上一帧那个就比不相等,于是它底下每一个图标每一帧都被标记重绘。
* **焦点光环是往外长的,所以控件不会动。** 往里长的环会在控件拿到焦点的那一刻把它的内容挤
  一下。
* **两个界面层级是不同的,而 base 是默认。** 它存在是因为 **iOS 的系统颜色是两个颜色而不是
  一个**:同一个 `systemBackground` 在页面上是一种灰、在盖上去的 sheet 上是更浅的一种,而
  两个 widget 谁都不知道自己是哪一种——挑的就是这个层级。
* **线性指示器按比例填充、不会超出**。它和同文件那个转圈的分别在于各自**知道什么**:转圈是
  因为这场等待没有可度量的终点,而它收一个 `progress` 是因为有。给一场量不出终点的等待画一
  根进度条,是在向读者许一个应用给不出的结局。
* 高度默认 4.5,除非另说。

`CupertinoUserInterfaceLevel::of` 在上游是**抛异常**的,`maybeOf` 才返回空。这里两个都保
留,并且照这个 crate 一贯的做法:debug 断言 + release 回落到 base——发布出去的应用宁可画一
种略微不对的灰,也不该停下来。

验证:`cargo test --lib` 1580 绿,GN `rustflutter_unittests` 1580 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1302 accounted / 586 MISSING。

### 一行的高度是它那个图标加上呼吸(2026-08-20)

`CupertinoListTile`、`CupertinoListTileChevron`、`CupertinoListSection`、
`CupertinoFormRow`、`CupertinoFormSection`(上游 cupertino 那四个文件),进
`cupertino.rs`。四个文件收口。

**这一族里每一个常量都成对出现,一对一个变体**——这是设计而不是巧合:base 分组是通到边、方
角的;inset grouped 是一张从两侧收进来的圆角卡片。两者差得够远,共用任何一个数都会在某处
错,所以上游一个都不共用。行也一样:`CupertinoListTile` 和它的 `.notched` 构造器在**每一个
尺寸上**都不同,所以这里是个 style 枚举而不是一个 bool。

**回归行盯的地方:**

* **一行的最小高度是「那个前导图标 + 两倍留白」**,上游四个最小高度全写成这个算术而不是一
  个数。算术本身就是那句话:图标的尺寸定了行的尺寸,剩下能选的是留白。写死一个 44 既说不出
  它为什么是 44,图标改了它也不会跟着走。
* **有副标题时留白会**变大**(8 → 10),所以两行的行比「多一行」还要高一点。** 这是有意
  的:两行按一行的边距塞进去,即使什么都没重叠,读起来也是挤的。
* **notched 而没有前导图标的那一行会收紧,并且自己带上下内边距**——没有图标来定行高时,得由
  内边距来定。这也是四种里唯一一个内边距有垂直分量的。
* **图标变大时,它和标题之间那道缝会变小**:缝和图标合起来才是眼睛读到的那个缩进,所以一个
  让位给另一个。
* **一行的两端内边距不一样**(左 20 右 14):左边是眼睛顺着整张列表对齐的那条文字边,右边坐
  着一个本来就自带视觉留白的尖角。
* **尖角的大小是读者的字号,不是一个常量**——所以它随字长大,一直像是这一行上的标点,而不是
  一个在大字旁边显得很小的固定装饰。
* **inset 分组的表头浮在它那张卡片上方**(顶部 16),而 base 分组的表头是同一条流水的一部
  分(顶部 0)。
* **有表头时,行的上边距让给表头**:两个都留着会在标签和它所标的那张卡片之间开一个洞。而另
  外三边不动——只有起争议的那一边动。
* **base 分组没有侧边距、inset 有**,这正是两种形状的全部分别。
* **表单行的标签边距比字段边距宽三倍以上**:标签靠着眼睛顺着列往下读的那条边,而字段一直伸
  到接近屏幕边,那一头没什么要预留的。
* **表单分组的行是齐平起头的,列表分组的不是**:表单永远是 inset 形状、而且上面永远有东西,
  所以它的行不需要自己的上边距。

**一处自查:** 我先按「应该是这样」写了两个内边距——`_kPaddingWithSubtitle` 我以为带垂直分
量、`_kNotchedPaddingWithoutLeading` 我以为不带——**两个都反了**。回去读了源码才改对。顺带
发现 `_kPadding` 和 `_kPaddingWithSubtitle` 上游其实是**同两个数**;这里仍然分成两条 match
臂而不是合并,因为上游把它们留成两个常量,合并会让改动其中一个悄悄带走另一个——而那正是把同
一个值写两遍的唯一理由。

验证:`cargo test --lib` 1573 绿,GN `rustflutter_unittests` 1573 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1297 accounted / 591 MISSING。

### 字大了就把对话框加宽——和 Material 恰好相反(2026-08-20)

`CupertinoPopupSurface`、`CupertinoActionSheet`、`CupertinoActionSheetAction`、
`CupertinoDialogAction`(上游 `cupertino/dialog.dart`),进 `cupertino.rs`。
`cupertino/dialog.dart` 5/5 收口。

**iOS 没有「无障碍模式」这个东西可查,于是文字缩放替它当了这个开关。** 上游的
`_isInAccessibilityMode` 是「14 点的默认字号被放大到超过 14 × 1.4」——读者把字调到这个程度
时,对话框该**换个形状**,而不只是重新排一遍。这里照上游那样写成「缩放后的字号 > 默认 ×
系数」,而不是直接比较缩放值,因为那才是在一个非简单倍数的 text scaler 下还成立的写法。

**而它换形状的方式,和 Material 恰好相反:** Material 保持宽度、把留白让出来(见上一批的
`scale_dialog_padding`);Cupertino **把对话框加宽**(270 → 310)。两个设计对同一个问题给
出相反的答案,而两边都按各自平台原样移植。这条回归行把两者并排断言了一次。

**回归行盯的地方:**

* 无障碍模式的判定,以及**正好在 1.4 上不算越过**。
* 上面那条相反的对照。
* **action sheet 比对话框铺得更宽**(边距 8 对 20):sheet 是要几乎顶满屏幕宽的,而对话框是
  浮在屏幕中间的。
* **sheet 的行随文字长高,但基座不会缩没**:上游 `base + fontSize * factor`,是在模拟器上
  试出来的。不长高的话,大字号下词会贴到行的边上;而那个**不随字号缩放的 base**,保证再小
  的字也留着一丝呼吸。
* **按钮标签缩到 10 点就不再缩**——再往下这个词就不是一个可点目标而是装饰了。
* **popup surface 可以只模糊、不给自己上色**:这正是 action sheet 那个取消按钮要的——它自
  带填充,否则会被染两次。
* **模糊照旧配着饱和度提升**(和上一轮 desktop 工具栏同一条理由)。
* **取消按钮是块独立的板,前面有道缝**:那道缝就是在说「这一个不是那些选项之一」;贴着
  sheet 的取消会读成它上面的最后一项。
* **「默认动作」和「破坏性动作」是两回事**:一个是「你多半想要的那个」,一个是「撤不回来
  的那个」,一个按钮至多是其中之一。

那几个不圆整的数(67.8、57.17)照抄不动,上游注释写着是「在 iOS 17 模拟器上比出来的」——凑
整会让它和它要配的那个系统差一点点,而那一点点正是这类数存在的理由。

验证:`cargo test --lib` 1562 绿,GN `rustflutter_unittests` 1562 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1292 accounted / 596 MISSING。

### 棒棒糖,不是方块(2026-08-20)

Cupertino 这一侧的同一道接缝:`CupertinoTextSelectionControls`、
`CupertinoTextSelectionHandleControls`、`CupertinoDesktopTextSelectionControls`、
`CupertinoTextSelectionToolbar`、`CupertinoTextSelectionToolbarButton`、
`CupertinoDesktopTextSelectionToolbar`、`CupertinoDesktopTextSelectionToolbarButton`。
六个 cupertino 文件一次收口。

**iOS 的手柄是个棒棒糖,不是方块。** Material 画的是一个固定 22 像素的方块,坐在行下面;
iOS 画的是**一根跑满这一行文字高度的杆子**加一个头——所以它的尺寸**取决于它正在选的文
字**,一个挨着大标题的手柄比挨着正文的高。

**杆子和头是重叠的**(1.5),所以高度是「和**减去**重叠」而不是一个干净的和:不重叠的话两
个形状恰好相接,而各自的抗锯齿都够不到对方,会留下一道发丝缝。

**回归行盯的地方:**

* **iOS 手柄随文字长高、Material 的不长**;而长的只是杆子,宽度不变。
* 高度**确实是那个减法**。
* **左手柄的锚点在最底下**(头在上、杆子沿着文字躺着),**右手柄的锚点靠近它的头**(它是翻
  过来画的),而**塌缩手柄居中**——它标的是一个光标,没有哪一侧可偏。
* **两个 desktop controls 都不画手柄,但仍然是两个类**:不同的是**工具栏**,不是手柄——所以
  这里两者答得一模一样,而它们是两个类的理由是各自弹出的那个菜单。
* **iOS 工具栏上下两个锚点挪的距离一样**,不像 Material 那个下距要让开拖拽手柄:iOS 的手柄
  是**沿着**行的一根杆子,而不是行下面的一个头,所以选区底下没有东西要让。
* **箭头离屏幕边比工具栏本身远得多**(26 对 8):工具栏可以几乎顶到边,箭头不行——否则它会
  画在工具栏自己的圆角上,把尖给弄没了。而箭头**宽大于高**,这样它读起来是个指针而不是一根
  刺。
* **iOS 菜单按钮高大于宽,desktop 的反过来**:iOS 的选择菜单是一排词、中间是分隔线、没有图
  标,所以是高度给了每个词一个目标;macOS 那一行更密,靠宽度。
* **两个 desktop 工具栏的宽度必须相等**(都是从同一个 macOS 菜单量的),不等就说明有一个飘
  了。
* **模糊要配着饱和度提升一起来**:模糊会把颜色平均掉、洗淡,所以饱和度要推回去,才让透出来
  的东西还认得出。只有其中一个会看着不对。

验证:`cargo test --lib` 1553 绿,GN `rustflutter_unittests` 1553 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1288 accounted / 600 MISSING。

### 选择工具栏摆在哪儿(2026-08-20)

接着上一轮:`TextSelectionToolbarAnchors`
(`widgets/text_selection_toolbar_anchors.dart`)、`TextSelectionToolbar` 和
`TextSelectionToolbarTextButton`(material 那两个文件),都进 `text_selection_controls.rs`。
又三个文件收口。

**两个锚点而不是一个,因为这个工具栏有两个家。** 有地方就摆在选区**上面**,没地方就摆**下
面**——而究竟哪一种是布局时才定的,所以锚点必须把两种可能都带着,而不是带一个已经做完的决
定。

**回归行盯的地方:**

* 两个锚点都**水平居中在选区上**,一个在它顶、一个在它底。
* **两个锚点都被钳进字段区域里。** 一个滚出去一半的选区,否则会把工具栏摆到字段并不在的地
  方、指着读者看不见的文字。钳住之后它贴着字段的边、并且仍然指进字段里面。
* **空选区没有「上面」也没有「下面」**:上游的提前返回——没有矩形可指,也就没有第二个锚
  点。
* **下面那道间隙要让开拖拽手柄,上面那道不用。** 上游把下距写成 `kHandleSize - 2` 而不是
  20:选区下面挡着一个手柄,而上面只有文字,8 就够。写成减法,手柄尺寸一改就跟着走。
* **靠近屏幕顶部的选区会把工具栏推到下面去**——这正是要两个锚点的全部理由。
* **状态栏会吃掉上面的空间**:同一个选区在没刘海的屏上放得下、在刘海高的屏上放不下,而工
  具栏得在挑锚点**之前**就知道。
* **孤零零一个按钮同时是第一个也是最后一个**,两侧都取端边距。
* **两个按钮之间那道缝是分摊的,而端上那道不是。** 各出一半才凑出读起来对的间距;而端上那
  道是一个按钮独自承担,所以得整个 14.5 自己出。
* **按钮的可点区域彼此相接、中间没有死带**——正是因为中间那道缝是**均分**的:前一个的右内
  边距和后一个的左内边距是同一个数,没有哪一像素属于「两个都不是」。

验证:`cargo test --lib` 1543 绿,GN `rustflutter_unittests` 1543 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1281 accounted / 607 MISSING。

### 谁来画选择手柄,以及谁一个都不画(2026-08-20)

新模块 `text_selection_controls.rs`,八个类跨五个文件:`TextSelectionControls`、
`EmptyTextSelectionControls`、`TextSelectionHandleControls`(`widgets/text_selection.dart`),
`MaterialTextSelectionControls`、`MaterialTextSelectionHandleControls`
(`material/text_selection.dart`),`DesktopTextSelectionControls`、
`DesktopTextSelectionToolbar`、`DesktopTextSelectionToolbarButton`(desktop 那三个文
件)。四个文件收口。

**有意思的是那些「一个手柄都不画」的实现——而它们是三个类不是一个,因为理由各不相同:**

* **desktop 不画,是因为桌面有鼠标。** 选区是用指针直接拖出来的,手柄是个没人需要的触摸供
  给物,还多一样东西挡着挨误点。
* **empty 不画,是因为那个**字段**不想要**(比如一个只读标签)。但它**每个问题照样答**——所
  以字段可以拿着它而不用在每个调用点判空。**「没有手柄」和「没有 controls」是两回事。**
* **handle controls 不画的是**工具栏**,手柄照留。** 这是上游的迁移接缝:工具栏搬去了
  `contextMenuBuilder`,所以它对每个工具栏问题都答 false,而手柄原样交给它包着的那个类。
  **这里的 false 意思是「别问我剪切,去问上下文菜单」,不是「没有东西可剪」。**

**回归行盯的地方:**

* **选区塌成一个光标时剪不了也拷不了**——没有东西可移除,也没有东西可放进剪贴板。
* **粘贴只问那个标志位。** 粘贴会替换掉选中的东西,包括「什么都没选」——所以一个光标和一段
  范围一样是合法目标。
* **全选只在还什么都没选中的时候才给。** 这条最出人意料:上游要求选区**是**塌着的。读者已
  经有一段范围之后再给全选,要么什么都不做(范围本来就是全部),要么把他们自己选的东西扔
  掉——**一个塞满了会撤销读者自己劳动的命令的工具栏,比一个短工具栏更糟。**
* **触摸手柄的锚点是不对称的**:左手柄锚在它的**右**边、右手柄锚在**左**边,好让每个手柄的
  方角贴着文字、圆身子挂在选区**外面**,而不是压在正被选中的字上。塌缩手柄标的是一个点而不
  是一条边,所以居中。
* **handle controls 保留手柄、拒答全部命令**,并且锚点和它包着的那个类**逐一相同**。
* **desktop 工具栏把锚点搬进那层内边距的坐标系**——锚点来的是屏幕坐标,而工具栏是在
  padding 里面布局的;不搬的话它会朝每个方向偏 8 像素。
* **desktop 工具栏是定宽的**(222,上游注释说是从 macOS 上 TextEdit 的截图量的):这样菜单
  不会随着命令随选区来去而变形——而它们确实会来去,剪切和拷贝要有范围才出现。
* **工具栏按钮的内边距是往下偏的**(底 3、顶 0):视觉居中,因为文字因升部而坐得比盒子中线
  高;而字距是**负的**,因为宽度是定死的。

**没有照搬的:** `buildHandle` 和 `buildToolbar` 要从 `TextSelectionDelegate`、
`ClipboardStatusNotifier` 和一串 `TextSelectionPoint` 里造 widget——那是
`widgets/text_selection.dart` 的浮层机器,这个 crate 没有。这里在的是决定**要不要**和**在
哪儿**的那一半:手柄尺寸、锚点,和那四条 `can*` 规则。那是其余部分建在上面的答案,也是不
需要浮层就能测的那一半。

验证:`cargo test --lib` 1534 绿,GN `rustflutter_unittests` 1534 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1278 accounted / 610 MISSING。

### 四种 chip,和一个什么都不画的溅开(2026-08-20)

`ActionChip`、`ChoiceChip`、`FilterChip`、`InputChip` 各自那个文件,加上
`no_splash.dart` 的 `NoSplash`。五个文件一次收口。

**这一轮是三轮前那六个属性接口开始还账。** 上游这四个 chip 各自 `implements` 一组不同的
接口,而**它们的差别就在那组组合里**,不在字段表里(四个文件的字段表几乎一模一样,因为
Dart 里实现接口就得把成员全声明一遍)。所以这里把**共有的存储**收进一个 `ChipParts`,让四
个变体只在**实现了什么**上不同——这才让它们读起来是四个**组合**,而不是四份近似的复制。

**回归行盯的地方(用编译期约束钉,而不只是断言):**

* **action chip 没有状态可处。** 它实现 `TappableChipAttributes` 而**不**实现
  `SelectableChipAttributes`——按下去启动一件事,之后长得还是那样;一个亮着的 action chip
  是在宣称一个它并没有的状态。用一个「只收可选 chip」的泛型函数钉住:另外三个进得去,它进
  不去。
* **input chip 是唯一六个都实现的**,用一个要求六个 trait 全在的泛型函数钉住。另外三个各
  是它的一个子集——这正是这套分类值得存在、而不是做成一个「什么字段都有的 chip」的理由。
* **choice chip 默认不显示勾,filter chip 把这事留给主题。** 分别不在 widget 而在那个
  **集合**:一组里只选一个,颜色本身已经把它区分开了,再打个勾是把同一件事说两遍;而多个
  filter 可以同时开着,那一组必须一眼读得出来,光靠颜色不够。
* **choice / filter / action 三个从「有没有事可做」推出 `isEnabled`,而 input chip 是自己
  带着的**——它展示的是读者自己敲进去的东西,不管还能不能对它做什么,**「在场但不可操作」
  对它是一个真实状态**。
* **只有那两个「集合型」的能删。** 一个 choice 是固定集合里的一个,没有什么可移除的——换一
  个才是「不再选它」的方式。
* 四个各有一个 `.elevated` 形态,那是个**形状**而不是一个数:抬起的 chip 坐在自己的面上,
  而不是页面上的一圈轮廓。

**`NoSplash` 是「什么都不画」而不是「没有 feature」。** 这个分别是实打实的:一个主题要求
不要溅开的控件,**照样按下、照样高亮、照样触发回调**——只是不画。上游的 `paintFeature` 是
个空方法体,而它的 `confirm`/`cancel` 直接 `dispose()`;这里就是这个变体报「没有东西要
画」,并且在手势落定的那一刻就死掉(而不是淡出)。工厂里也多了第三个选择——主题一次换掉全
应用的溅开,而「不要」是它能换到的东西之一。

验证:`cargo test --lib` 1524 绿,GN `rustflutter_unittests` 1524 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1270 accounted / 618 MISSING。

### 三处放导航目的地的地方(2026-08-20)

新模块 `navigation_destinations.rs`:`NavigationDestination`、`NavigationIndicator`
(`navigation_bar.dart`)、`NavigationDrawer`、`NavigationDrawerDestination`
(`navigation_drawer.dart`)、`NavigationRailDestination`(`navigation_rail.dart`)。
`navigation_drawer.dart` 和 `navigation_rail.dart` 都收口了。

**上游把这三个「目的地」摊在三个文件里,各自挨着装它的那个面。这里放在一起,是因为关于它
们值得知道的正是它们**怎么不一样**,而这只有并排摆着才看得见:**

* **bar** 的 label 是 `String`,另外两个收 widget——bar 的标签永远是图标底下一个短词,上游
  就按这个类型写。
* **rail** 的 `selectedIcon` 默认回落到 `icon`,另外两个留空。rail 又窄又一直在屏幕上,所
  以被选中的目的地**总得画点什么**;bar 和 drawer 有地方让标签和药丸来担这件事。
* **drawer** 的目的地是整行、带自己的背景色——drawer 的目的地是一个列表,而列表的一行可以
  被染色;bar 的不行。

**最要紧的一条规则:只有目的地被编号。** `selectedIndex` 数的是**目的地**,不是子项;上游
遍历子项时只在遇到 `NavigationDrawerDestination` 时才 `destinationIndex++`,别的原样穿
过。所以夹在两个目的地之间的分割线或小标题**不会挪动高亮的是哪一个**——这才让调用方可以把
目的地归到几个小标题底下而不用重新编号。

**回归行盯的地方:**

* 上面那条(`[None, Some(0), Some(1), None, Some(2)]`),以及选中项要**越过小标题**去找它
  真正的子项。
* **选中值超出末尾时找不到东西,而不是回落到最后一个。** 钳位会悄悄高亮一个调用方没点名的
  目的地,那比不高亮更糟:它看起来像是抽屉工作正常。
* **抽屉默认高亮第一个目的地**(上游默认 `selectedIndex` 是 0 而不是 null)——一个什么都没
  选中的导航面不说明读者在哪儿;而调用方仍然可以显式说「一个都不选」。
* **只有 rail 默认给目的地一个选中图标**,上游只在 rail 的构造器里写了 `selectedIcon ??
  icon`。
* **bar 的 tooltip 从一个可空字符串里读出三种状态**:没设 = 用标签,空串 = 完全不要
  tooltip,别的 = 它自己。移植时用一个光秃秃的 `Option` 会把第三种弄丢。
* **rail 那个标志位是反着拼的**:上游在 rail 上叫 `disabled`,另外两个叫 `enabled`。照上
  游拼写保留,好让对着两个文件读的人不用猜哪个是哪个——并且钉住,因为一个悄悄「统一」了它
  们的移植会把其中一个的默认值弄反。
* **指示药丸默认是个 stadium**:64×32、半径 16——半径**正好是高的一半**,所以两头是半圆。
  上游写的是 16 而不是点名 stadium,两者只靠手工保持相等;所以给了一条 `is_stadium`,一个
  改了高度却忘了半径的调用方会看到它变 false。

验证:`cargo test --lib` 1516 绿,GN `rustflutter_unittests` 1516 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1265 accounted / 623 MISSING。

### 一个数就够了:那条会塌下去的头(2026-08-20)

新模块 `flexible_space_bar.rs`,上游 `material/flexible_space_bar.dart` 两个全到:
`FlexibleSpaceBar` 和 `FlexibleSpaceBarSettings`(连同 `CollapseMode`、`StretchMode`)。

**这个 widget 里所有东西都是一个数 `t` 的函数**:0 是完全展开,1 是塌成工具栏。背景的不透
明度、标题的缩放、背景摆在哪儿,全部由它推出来。**这条 bar 自己在帧与帧之间什么都不
记**——正因如此它才能被一个说跳就跳的滚动位置驱动。

**几处非显然的规则,各钉一条:**

* **`t` 要钳位**:把列表往下拽过头时 current extent 会大于 max,不钳就变成负数,而每一个
  读它的地方都会读反。
* **塌不下去的 bar 读作「完全展开、完全不透明」。** min == max 是一次除以零。上游是在**每
  一个**用到它的地方各特判一次 `maxExtent == minExtent`;这里在 `t` 里**一次**说清,而
  `background_opacity` 也照上游那句原话("the app bar cannot collapse and the content
  should be visible")直接答 1。
* **没说的 extent 意思是「这条 bar 不在塌」而不是零。** 上游 `createSettings` 把 min/max
  默认成**当前**的那个值——这才让一条上面没有 sliver 的 bar 表现得像条普通 bar,而不是像条
  已经完全塌了的。
* **视差是四分之一。** 这个数正是让背景读起来「比页面更远」的东西:在你身后的东西看起来比
  在你身边的东西动得少。pin 则**正好抵消**这次塌陷(背景一动不动),none 让它按 bar 自己
  的速度走。
* **背景先撑住、再在最后一个工具栏高度里淡掉**,不是整段塌陷都在淡:一张读者一滚就开始淡
  的照片看着像 bug,而一张先撑住、到工具栏合拢时才走的,读起来是「工具栏来了」。塌陷本身
  比一个工具栏还浅时,`fadeStart` 落到零,整段都在淡。
* **标题的前导内边距只在真有返回键时才让位**;没说时按「有」算(标题压在按钮底下,比标题
  白白缩进要糟);而**居中的标题不让**——它坐的那一侧没有按钮要躲。
* **拉伸只在列表被拽过顶时发生**,那是唯一有多余空间的时候。三个 stretch mode 是**能叠加
  的三个独立决定**(缩放背景、模糊背景、淡出标题),各管一个部位——所以是个列表而不是一个
  选择;只开模糊时背景就不缩放。
* **标题跟着工具栏自己的不透明度淡**,和背景那份是分开的——因为背景淡出的同时工具栏正在淡
  入。

验证:`cargo test --lib` 1508 绿,GN `rustflutter_unittests` 1508 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1260 accounted / 628 MISSING。

### 字大了,留白反而要收(2026-08-20)

`SimpleDialogOption`、`SimpleDialog`、`AlertDialog`(上游 `material/dialog.dart`)进
`controls.rs`,连同上游的 `_scalePadding`。`dialog.dart` 4/5。

**这一轮最值得写下来的是那条内边距规则:读者的字号越大,对话框的留白越少**——到 2× 时收
到自己的三分之一,再大也不收了。这看着是反的,直到把另一条路想一遍:对话框是个**固定的小
盒子**,字翻倍而留白不动,内容就被挤出底边了。地方总得从某处来,而**留白正是一个要求「把
字放大」的读者没有在要的那部分**。

两端都钳住:1× 以下不放大(要**更小**字的读者不该拿到一个比设计更宽松的对话框),2× 以上
不再收(三分之一已经是文字碰到边缘之前能到的最紧)。

**回归行盯的地方:**

* 上面那条规则本身,以及**只减不增**。
* **两端的钳位**各钉一次。
* **simple dialog 的标题会保住它到第一个选项的那道缝。** 这是上游在这儿唯一的不对称:那
  个底部 inset 是「标题和第一个选项之间的间隔」,而**一道随字号一起缩的缝,会正好在文字最
  需要被分开的时候合上**。所以它不缩——但只在下面真有东西要分开时才这样;什么都没有时它和
  别处一样缩。
* **simple dialog 的选项是通到边的**:它的 content padding 两侧是 0,而选项自己带 24——所
  以溅开能铺到对话框边缘。一个从两侧缩进的选项读起来是「列表里的一个按钮」,而不是「列表
  的一行」。
* **是「有图标」把 alert dialog 的标题居中的**:居中图标底下压一个左对齐的标题读起来像出
  错,所以上游把这两件事绑在一起,而不是把对齐单独开出来。
* **alert 的三处内边距用同一条规则缩**,所以大字号下对话框不会变得一边宽一边窄;而 1× 时
  一动不动。
* `SimpleDialogOption` **收的是 builder 而不是 widget**,并且用挂载证明孩子确实是从
  builder 里出来的——ink well 每次自己状态变了(溅开就是状态)都会用同一个 widget 实例重
  建,交出去一次的孩子第二次就没了。

**`DialogRoute` 留着不动,也不记账。** 它是 `RawDialogRoute` 的子类,带着屏障颜色、屏障
标签和 M3 的对话框转场;这个 crate 没有模态路由这套东西——它的 `Route` 是「名字 + 参数」的
路由**描述**,而不是上游那个带屏障和结果的 `Route` 对象。这和 `Overlay` 是同一个架构缺口。
**账本是给「换了个形状但东西在」的情形用的**,这里是真的没有,所以它该继续报 MISSING。

一处过程记录:`SimpleDialogOption::build` 我第一版写岔了——把 `well` 赋了两次、中间那次是
个占位。原因是我一开始把它当成「收一个 widget」,而 ink well 收的是 builder。改成 builder
之后就直了。另外一条测试我先写成「建一棵无关的树、然后断言计数是 0」——那什么都没证明,已
换成真的挂载它。

验证:`cargo test --lib` 1494 绿,GN `rustflutter_unittests` 1494 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1258 accounted / 630 MISSING。

### 溅在整行上而不是那一格上(2026-08-20)

`DataColumn`、`DataCell`、`TableRowInkWell`(上游 `material/data_table.dart`)进
`components.rs`。`data_table.dart` 5/5 收口。

**`TableRowInkWell` 存在的理由就是一件事:按在一格里,溅开要铺满整行。** 上游重写
`getRectCallback`,往上走到外层 `RenderTable` 拿这一格所在行的 `getRowBox`。没有这一步,
按在一格里的溅开会在那一格里铺开、停在格子边上——那是在说**这一格**被按了,而读者按的是这
一行。

为了做这件事,这一轮开了两个口子,两个都是上游本来就有的东西:

* **`RenderTable::row_box(row)`**(上游 `getRowBox`):布局时把每行的顶和高记下来。
  **单靠格子的偏移答不出这个问题**——一行的高是它**最高**那一格的高,而一个矮格子的偏移对
  此什么都没说。行框是**整张表宽**的,这正是让一格里的溅开覆盖整行的东西。
* **`InkResponse` 上的 `rect` 钩子**(上游 `getRectCallback`):墨迹据以丈量的矩形,替掉
  区域自己的边界。落指点也一并搬进那个矩形的坐标系——不然圆会从错的地方长起来。

和 `RenderAbstractViewport` 那轮一样,行矩形是**传进来**的:这个 crate 没有从 render
object 往祖先走的路子,而正在填这一行的调用方本来就知道它是哪一行。

**回归行盯的地方:**

* **一列「能不能交互」恰好等于「能不能按它排序」**(上游 `_debugInteractive` 就是
  `onSort != null`):没有回调的列不显示排序箭头,而不是显示一个按不动的。
* **`numeric` 是个对齐决定,不是格式决定**:表把数字列右对齐,因为数字是靠同一位上的数码
  比较来读的,左对齐会让个位在每一行落在不同的地方。
* **空格子是「在场且空白」而不是「不在场」**:一张表每行格子数相同,某列没内容的行仍然需
  要一个格子占位,否则它后面每一列都会左移。
* 一个格子只要**任意一个**手势接了线就算可交互。
* **placeholder 说的是「我是占位」,多淡由表决定**——所以它是个标志位而不是一个颜色。
* **行的溅开比格子的远得多**(600×40 的行对 80×40 的格子,半径差三倍以上)。
* `TableRowInkWell` 是**裹住 + 矩形高亮**的,两个配对出现:行高亮铺满行,所以必须裁到行
  上。
* 行框**比一格宽**、**和最高那格一样高**、**行与行紧挨着没有缝**,而**不存在的行答「没
  有」**。

顺带把 `InkResponse` 的 `contained_ink_well` 和 `highlight_shape` 改成 `pub`——上游这两个
本来就是公开的 final 字段,而这一轮的测试要从别的模块读它们。

验证:`cargo test --lib` 1487 绿,GN `rustflutter_unittests` 1487 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1255 accounted / 633 MISSING。

### 从那张卡片里撕出来的面板(2026-08-20)

新模块 `expansion_panel.rs`,上游 `material/expansion_panel.dart` 三个全到:
`ExpansionPanel`、`ExpansionPanelRadio`、`ExpansionPanelList`。它建在上一轮刚落地的
`mergeable_material` 上,而**这正是它的整个设计**:关着的列表是**一张**卡片,展开一个面
板是把它**撕开**——上面的面板仍合并成一块,展开的那个分离出来,下面的合并成另一块。这就
是告诉读者「你打开的东西是从这张列表里出来的」,而不是「盖在它上面的新东西」。

**缝隙规则那两个条件不是随手写的。** 上游在面板 `i` **之前**插缝隙的条件是「i 展开、且
不是第一个、且它前面那个**没有**展开」;在 `i` **之后**插的条件是「i 展开、且不是最后一
个」。两条合起来恰好就是维持 `MergeableMaterial` 那两条不变量的东西:

* 连着两个展开的面板,本来会在第一个之后、第二个之前各得一个缝隙——那就是**连着两个缝
  隙**,`MergeableMaterial` 不允许。`!expanded(i - 1)` 就是挡住第二个的那一句。
* 在两端展开的面板本来会在那一端放一个缝隙——也是不允许的。`i != 0` 和 `i != last` 就是挡
  住那两个的。

**这条用一条穷举的回归行钉住:** 对 0..6 个面板的**每一种**开合组合(2^n 全部)断言
`gaps_are_valid`。理由写在测试里:「我想到的那些情形下不变量成立」是个比这段代码实际做出
的更弱的主张。

**另外几条回归行:**

* 中间那个展开的面板**两侧各被撕开一道**;连着两个展开的面板**中间只共用一道缝隙**;两端
  的展开面板**不会把缝隙放到端外**,而单独一个展开的面板一道缝隙都没有——它本来就是整张卡
  片。
* radio 列表**跟踪的是「哪一个开着」而不是每个一个标志位**;`value` 存在正是因为它要在列
  表带着不同顺序重建时活下来,而下标活不下来。
* **初值指向一个不存在的面板时,什么都不开**,而不是退而开第一个:点名了一个不在那儿的面
  板的调用方是有 bug 的,猜一个会把它藏起来。
* **打开一个 radio 面板时,先报告正在关上的那一个。** 自己记账的调用方否则会看到乱序,最
  后记成两个都开着。
* **报出去的值是「它要去哪儿」而不是「它现在在哪儿」**(上游的 `!isExpanded`,它自己注释
  说了原因:按下的那一刻面板还没翻)。
* **按已经开着的 radio 面板会把它关上**,而不是什么都不做——所以 radio 列表可以全部关闭。
* **两个 value 相同的 radio 面板被判非法**:对列表来说它们是同一个面板,开一个就是开两
  个,而且没有任何一次按压能把它们分开。
* 展开态 header 的内边距按上游那样**写成算术**(`64 - kMinInteractiveDimension`)而不是
  16:两个手工保持一致的数会走散。
* 缝隙和切片的 key 取自**两条不会碰撞的序列**(奇数是缝隙、偶数是切片,上游的
  `index * 2 ± 1`)——碰撞会让 mergeable material 在重建时把一个缝隙和一个切片当成同一项。

**一处自查:** `expansion_callback` 和 `can_tap_on_header` 一开始是**够不着的**——没有任何
东西会触发它们,因为 `ExpandIcon` 没移植。一个永远不会触发的回调是装饰。已经把 header 的
点击接上了(`can_tap_on_header` 打开时套一层点击区域),并加了一条**端到端**的回归行:可
点的 header 接住按压,不可点的放它过去。hit-test id 由调用方经 `with_header_tap_ids` 给
出——在这个 crate 里 id 是调用方分配的,和其它每个可交互控件收 `id` 是同一回事。

**没有照搬的三处:** `ExpandIcon`(会转的那个尖角,它自己是上游一个类,等图标系统);
`AnimatedCrossFade`(上游用它在前 60% 淡出旧尺寸、后 60% 淡入新内容,中间重叠——这正是让面
板在展开途中不显得空的东西);以及图标上那条要 `MaterialLocalizations` 的 `Semantics` 提
示。

验证:`cargo test --lib` 1479 绿,GN `rustflutter_unittests` 1479 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1252 accounted / 636 MISSING。

### 一张会被撕开的卡片(2026-08-20)

新模块 `mergeable_material.rs`,上游 `material/mergeable_material.dart` 四个全到:
`MergeableMaterialItem`、`MaterialSlice`、`MaterialGap`、`MergeableMaterial`。

**想法是一个「相邻项就是同一块表面」的列表。** 两片中间什么都没有,就共用一层阴影和一圈
轮廓,而且**相接处的角是方的**——于是它们看着是一张中间划了道线的卡片,不是两张摞起来的
卡片。在中间放一个 `MaterialGap`,两半就把新露出来的边圆起来、分开。

**这就是全部要点,也是 expansion panel list 建在它上面的原因:展开一个面板不是多出一张
卡片,而是把它原本所属的那张卡片撕开。** 撑住这个观感的正是圆角——一个随着缝隙张开、从方
变圆的角读起来是「撕」,而两张卡片淡入读起来是「换」。

**两条不变量,上游在 `_debugGapsAreValid` 里断言,两条都是结构性的而非装饰性的:**

* **不能有连着的两个缝隙。** 两个挨着的缝隙就是一个更宽的缝隙,没有东西分得出来,所以第
  二个是到不了的状态——而写下它的调用方本来想说的是别的事。
* **两头都不能是缝隙。** 缝隙的活是把两片分开;放在末尾的缝隙是把一片和「没有」分开,那
  是穿着缝隙外衣的内边距。

**回归行盯的地方:**

* 上面两条各一条,外加「空的 run 是合法的」——它没有缝隙可以出错。
* **相接的两片在相接处是方角**,这正是让两片读成一张卡片的东西;而 **run 自己的两头永远
  是圆的**,不管别处怎样——那是卡片的外面。
* 一片**朝着缝隙的那个角才圆**,前后两片各圆各的那一侧。
* **半开的缝隙给半圆的角**:关着时是方的,开一半时半径是一半。这条钉的就是「撕」那个观
  感的来源。
* **轴向决定这个半径落在哪一对角上**:竖排的第一片圆的是**上面**两个角、下面两个是方的
  (下面才是它相接的方向),横排的第一片圆的是**左边**两个。两个答案必须不同——先前那版
  测试拿的是两侧都圆的位置,即使完全忽略轴向也会通过,已经换成能区分的。
* 卡片圆角必须**就是卡片用的那个数**(`MaterialType::Card.border_radius()`,2.0),不是
  它的第二份拷贝。
* 一片可以**带自己的颜色而不破坏这个 run**——这正是 `MaterialSlice.color` 的意义:合并
  run 里的某一片可以被高亮,而这个 run 还是一个 run。

**没有照搬的:** 上游按缝隙的 `LocalKey` 给每个缝隙留一个 `AnimationController`,好让插进
来的缝隙**从零长出来**、被移除的缩回去,片的圆角在同一个时钟上从方走到圆。那需要「按列表
里携带的 key 跨重建存活的动画」,而这个 crate 还没有这个设施——`implicit` 那套是按**在树
里的位置**存状态的,不是按列表项的 key。所以这里的缝隙一次到位就是全尺寸。**但被那个动画
驱动的算术是留了口子的**:`border_radius` 收一个「缝隙张开了几分」的分数,将来接上时钟是
一个调用点的改动。

另一处渲染器限制照旧记着:这个 crate 的渲染器四个角只有一个半径(`BottomSheet` 记的同一
条),所以一片混合角取两者中较大的那个——在「只有一端贴着缝隙」这个常见情形下,这保住了撕
开的那条边是圆的、相接的那条边是方的。

验证:`cargo test --lib` 1466 绿,GN `rustflutter_unittests` 1466 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1249 accounted / 639 MISSING。

### 把一样东西滚进视野(2026-08-20)

`RevealedOffset` 和 `RenderAbstractViewport`(上游 `rendering/viewport.dart`)进
`render.rs`,`DecorationImagePainter` 记账。这两笔收掉了尺子修复露出来的最后两个,
`viewport.dart` 现在 3 covered / 3 mapped。

**`RevealedOffset` 带两个字段而不是一个**,因为调用方通常两个都要:要滚到的偏移,以及目
标到了那儿以后**会占住哪块矩形**——一个要给高亮做动画、或者要判断目标到时还放不放得下的
调用方,得在滚动发生**之前**知道后者。

**`clamp_offset` 那个 `None` 是这个方法的要点。** 给出「把目标顶到前边缘」和「顶到后边
缘」两个偏移,处在两者之间的任何位置都已经完整显示了目标——所以**一个已经看得见的目标,一
下都不该滚**。一个热心地把读者本来就看得见的东西「居中」一下的视口,是在无缘无故地把页面
从他们脚下拽走。

**两个边缘偏移是排序后用的,不是假定的。** 哪个更大取决于轴方向:向上或向左的视口里,前
边缘是**更大**的那个偏移。上游把这种情况叫 `inverted` 并排序;一个假定了顺序的移植会在一
半的方向上滚反。

**回归行盯的地方:**

* alignment 0 顶前边缘、1 顶后边缘、0.5 居中,三个都连同「目标到时占哪块矩形」一起钉。
* **0..1 之外不钳位**,上游也不钳:越过端点是把目标滚到边缘**之外**,而这正是调用方留边距
  的办法。
* **比窗口还大的目标仍然答得出有用的东西**:富余量变成负的,公式照样成立——此时 alignment
  选的是「露出目标的哪一部分」,而不是「把它摆在哪儿」。0 露顶,1 露底。
* **向上的视口从远端量它的偏移**:`up` 视口的偏移从内容底部起算,所以**目标之外还剩多少**
  才是它被滚了多远,不是目标自己的顶。搞反了的话,向上列表里的一次 reveal 会去到它想去的
  地方的镜像位置。
* 横向视口横着量。
* 已经看得见就**不滚**(含正好压在两个边界上);看不见就**只滚到较近的那条边**。
* 两条边**换个顺序传进去答案一样**——这条盯的就是排序而不是假定。
* 接口上那个 `DEFAULT_CACHE_EXTENT` 必须**就是视口真正在读的那个数**(250)。

**这个移植收的是矩形而不是 `RenderObject`。** 上游的 `getOffsetToReveal(target, ...)` 开
头要沿着变换从 `target` 走到视口、算出目标**在哪儿**;这个 crate 没有暴露这样的走法,所以
矩形是传进来的——用的是被滚动的那个 child 的坐标系,而一个知道自己那一项在哪儿的调用方本
来就有这个。剩下的就是算术,而算术是这个方法真正在决定的全部。上游那两个静态的
`maybeOf`/`of` 同理不在:它们走的是 `RenderObject.parent`,而在这里,拿到视口是调用方**持
有**它。

**`DecorationImagePainter` 记账。** 上游它是 `createPainter(onChanged)` 造出来的一个对
象,存在的理由是给「一条已解析的 image stream」一个生命周期落脚点——所以它有 `dispose`。
这个 crate 的 `DecorationImage::paint` 每次绘制自己 resolve,没有要持有的流、也就没有要
dispose 的东西;那两个方法里真正做事的那个就在它身上,文档里早就点了名。**差别是解析时
机,不是这个类少了。**

验证:`cargo test --lib` 1456 绿,GN `rustflutter_unittests` 1456 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1245 accounted / 643 MISSING。

### 一枚 chip 能被问些什么(2026-08-20)

上游 `material/chip.dart` 那六个属性接口,加上 `ChipAnimationStyle`,进 `controls.rs`。
`chip.dart` 从 1/9 到 8/9(只剩 `RawChip`)。这七个里有六个是上一轮修尺子才露出来的。

**上游有六个 chip widget,这个 crate 有一个带 `ChipStyle` 的 `Chip`。** 那六个接口存在,
是因为上游那六个 widget 互相重叠:每个声明自己实现哪几个组合,而一个字段的文档写在接口上
一次,而不是在六个 widget 上写六次。所以这里按它们在这个 crate 里真正是什么来移植:**一
套「一枚 chip 能被问什么」的词汇**,由每种 style 自己回答。一种 style 多出一项能力,是多
实现一个 trait,而不是多长一个没人找得到的字段。

每个方法都是一个**带默认值的问题**,因为上游每个字段都可空,而空的意思是「主题来定」。

**回归行盯的地方:**

* chip 只答自己的 `label`,别的留给主题——`label` 是唯一没有默认值的那个,因为**没有标签
  的不是 chip**。
* **是 style 在回答「选没选中」**:上游把这件事摊在六个 widget 上,这里一个带 style 的
  chip 就答了。
* **没事可做的 chip 就是停用的,不管它的 style 怎么说。** 上游的 `isEnabled` 是从「有没
  有给回调」推出来的,不是一个标志位——所以一枚没接线的 chip 即使长得像 action 也是灰的。
* **没有回调时,删除是「不存在」而不是「被禁用」。** 一个按了没反应的 ✕ 会招来一次没反应
  的按压,所以上游干脆不显示那个 affordance。
* **勾选没设时是「主题来定」,不是「不要」。** `Some(false)` 是 chip 说「别显示」,`None`
  是 chip 什么都没说;把两者揉成一个,每一枚 filter chip 的勾都主题化不了了。
* **选中是带着「它变成了什么」上报的。** 一枚可选 chip **不拥有**自己的选中状态——拿着筛
  选条件的那一方才拥有——所以回调必须说清是往哪个方向去了,而不只是「发生了点什么」。
* **四个 chip 动画是四个不同的速度**(选中 195ms、抽屉开 150ms 关 100ms、停用 75ms),所
  以上游给了四个旋钮而不是一个:一个旋钮必然对其中三个是错的。**抽屉关得比开得快**——来
  的东西值得看,走的东西挡路。而且四个各自可覆盖,只关心选中动画的调用方不必把另外三个
  重说一遍。

**顺手改了一处命名冲突,值得记。** crate 原来的构建器叫 `Chip::selected(bool)`,而上游
`SelectableChipAttributes` 的 getter 也叫 `selected`——一个构建器和一个 getter 不能同名。
构建器改成 `with_selected`,这**本来就是这个 crate 里其它所有构建器的形状**(`with_style`、
`with_color`……),`selected(bool)` 才是那个例外。

**一处过程记录:** 改名时我顺手把 gallery 里两处 `.selected(...)` 也改了,而那两处是
gallery **自己的** `DemoChip`,不是 crate 的 `Chip`——GN 那一关立刻红了。已改回。这也说明
一件事:`cargo test --lib` 绿不等于 gallery 编得过,那两道闸门量的不是同一件事,而当时
`flutter_gallery_unittests.exe` 报的 322 绿是**上一次构建留下的旧二进制**。改回之后三个
GN 目标都在一次干净构建里绿了。

验证:`cargo test --lib` 1445 绿,GN `rustflutter_unittests` 1445 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1242 accounted / 646 MISSING。

### 一次命中测试里的三个角色(2026-08-20)

`HitTestable`、`HitTestDispatcher`、`HitTestTarget`(上游 `gestures/hit_test.dart`)进
`gestures.rs`——正是上一轮修好尺子之后露出来的那三个。`hit_test.dart` 现在 5 covered /
1 blocked。

这三个接口把一次命中测试拆成三个角色:**谁能被测**、**谁负责把事件送下去**、**谁能收**。
上游每个 render object 和每个手势识别器都是 `HitTestTarget`;这里命中项带的是一份
`PointerHandlers` 而不是一个目标对象,因为这个 crate 的区域本来就是处理函数而不是对象,
所以实现这个 trait 的是 `PointerHandlers`。

**回归行盯的地方:**

* **只有三种原始变化会走到 `handle_event`**(down/move/up,加上 cancel)。区域能被告知的
  其它一切——点击、拖拽、长按——都是**被识别出来的**手势,由 `GestureRouter` 从一串原始事
  件里判出来、也由它送达。这个分工是上游的:`handleEvent` 是原始通道,识别器坐在它上
  面。hover 和 add 也不走这里,那是 router 追踪鼠标在哪些区域里面的事。
* **cancel 不是 up。** 什么都没完成,所以正在显示进度的东西要**倒回去**而不是**收尾**;
  两者是分开的回调,只听其中一个的目标听不到另一个。
* **一次派发会到达路径上的每一个目标,不只是第一个。** 卡片里的按钮同时在两者里面,而一
  个只在按压**没打中**按钮时才听得到的卡片监听器,是个数不清按压次数的监听器。停在第一
  个是**手势竞技场**要做的事,那是另一套机制。
* **问另一个 view 会得到「什么都没有」而不是错的那棵树。** 这个 crate 只有一个 view;把
  主 view 的内容答给一个关于第二个 view 的提问是**错的答案**,而空路径只是**没用的答
  案**。
* **那个被弃用的 `hit_test` 就是「关于唯一那个 view 的同一个问题」。** 上游正在从
  `hitTest(result, position)` 迁到 `hitTestInView(result, position, viewId)`,因为一个应
  用现在可以有多个 view、而事件带着自己来自哪一个。两个都在,**弃用的那个是用另一个供出
  来的**——照的是这次迁移的形状,而不是它此刻的状态。这条回归行盯的就是它真的路由到主
  view,而不是一份会走散的第二实现。

同文件的 `NativeHitTestTarget` 早先就记在引擎受阻里(不是这一轮加的):它是个空 mixin,
存在只为了标记「这个 render object 是一块平台视图,命中测试要交给原生那边」,而这个
crate 没有平台视图。

验证:`cargo test --lib` 1437 绿,GN `rustflutter_unittests` 1437 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1235 accounted / 653 MISSING。

### 尺子看不见的那 15 个类(2026-08-20)

第三次审计这把尺子,这次找到的是**盲点而不是虚报**——比虚报更糟的那一种。

`tools/coverage.py` 认 Dart 的类修饰符时列了 `abstract`/`base`/`final`/`sealed`/
`mixin`,**独独漏了 `interface`**。于是每一个 `abstract interface class` 都从它眼前消失
了:不是被算成已覆盖,是根本没被数进去。**一个尺子看不见的类,永远不会被报成
MISSING。** 前两次审计找的是「算作已覆盖但其实什么都没有」,那让数字虚高;这一次是让数
字**看起来触手可及**——照原样下去,`0 MISSING` 是能在还有类没移植的情况下达成的。

修完之后,上游的公开类从 1873 变成 **1888**,多出来的 15 个是:

* `gestures/hit_test.dart` 的 `HitTestable`、`HitTestDispatcher`、`HitTestTarget`
* `material/chip.dart` 的六个属性接口(`ChipAttributes`、`DeletableChipAttributes`、
  `CheckmarkableChipAttributes`、`SelectableChipAttributes`、`DisabledChipAttributes`、
  `TappableChipAttributes`)
* `material/material_state.dart` 的 `WidgetStateInputBorder`
* `painting/decoration_image.dart` 的 `DecorationImagePainter`
* `rendering/viewport.dart` 的 `RenderAbstractViewport`
* `cupertino/menu_anchor.dart` 的 `CupertinoMenuEntry`

其中 `RenderAbstractViewport` 和 `DecorationImagePainter` 在 crate 里**只出现在注释
里**——确实是缺的,尺子现在报得对。

**顺带把这一批弃用重命名记了账**(四条,都有先例):

* `WidgetStateInputBorder` → `WidgetStateOutlinedBorder`。账本里 `InputBorder` 早就映射
  到了 `ShapeBorder` 的 `Underline`/`Outline` 两个变体——**这个 crate 里「输入框边框」就
  是一个 `ShapeBorder`**。那么「按状态解析出来的输入框边框」就是「按状态解析出来的
  `ShapeBorder`」;另立一个逐字段相同的类型只是给同一件事换个名字。
* `MaterialStateOutlineInputBorder` / `MaterialStateUnderlineInputBorder` → 同上。上游自
  己写着 `@Deprecated("Use WidgetStateInputBorder instead. Renamed to match other
  WidgetStateProperty objects.")`——移植一个上游正在删的重命名壳没有意义(和
  `ButtonBar` → `OverflowBar` 同一判断)。
* `MaterialStateMixin` → `WidgetStatesController`。那个 mixin 是给 `State` 子类混入一套
  状态增删查的,而它整份文档现在指向 `WidgetStatesController`,crate 有后者。Rust 没有把
  状态混进别人 `State` 的办法,而**持有一个 controller 正是上游现在推荐的写法**。

净结果:总数 1873 → 1888,accounted 1227 → 1232,MISSING 646 → 656。**数字往难看的方向
走了十个**,这正是审计该有的方向:每一个现在报出来的缺口,都是真的缺口。

验证:`cargo test --lib` 1432 绿(这一轮没动 Rust 代码),`cargo fmt` 干净。
覆盖率 1232 accounted / 656 MISSING(65%)。

### 让日历可以换一本(2026-08-20)

`CalendarDelegate` 和 `GregorianCalendarDelegate`(上游 `material/date.dart`)进
`pickers.rs`;`DateUtils` 记账。`date.dart` 4/4。

**这个 trait 是一道接缝。** 日期选择器关于日期的每一个问题都从这里走——这个月有几天、网
格前面空几格、一个月之后是哪天——好让一本非公历的日历(回历、日本年号)给出不同的答案,
而选择器不必知道答案不止一种。上游发的那一本 `GregorianCalendarDelegate` 就是默认那本,
而它的实现体是**转发**:每个方法转给对应的 `DateUtils` 静态函数,这里则转给同模块里的自
由函数。这个形状值得保留——**那些算术不需要 delegate 也能用,delegate 存在是为了让它可以
被替换,不是为了拥有它。**

**`date_only` 在这里是恒等,而它仍然留在 trait 上。** 上游要把时间剥掉,因为它的日期带时
间,而差几个小时的两个 `DateTime` 在日历上是同一格。这个 crate 的 `Date` **本身就是**那
个只有日期的类型,没有东西可剥。留着,是因为一本日期带时间的日历需要它,也因为写
`delegate.date_only(d)` 的调用方不该去关心自己手上是哪一种。

**回归行盯的地方:**

* delegate 答出来的和自由函数一样(两者不能走散),包括 1900 不是闰年、2000 是。
* **两个「没有日期」算同一天。** 上游是在 optional 上逐字段比,所以「没有」等于「没
  有」——一个在问自己选中项有没有变的选择器就靠这条。
* **给月份日期加月份会落在 1 号。** 上游写的是 `DateTime(year, month + n)`,**一个 day
  都不给**,Dart 读作 1 号。没有这条,一个从 1 月 31 日往后翻页的选择器会落到 3 月 3
  日。往回翻也一样。
* **网格前面空几格跟着 locale 的一周起始日走。** 那是上游从 `MaterialLocalizations` 上读
  的唯一一个数,这里直接传进来。2024 年 6 月 1 日是周六:周日起始空 6 格,周一起始空 5
  格,周六起始不空。
* **一个自定 delegate 真的能答出不同的东西**——用一本十三个月、每月二十八天的日历钉住这
  条;而且它**白拿那些 provided 方法**,这正是做成 trait 而不是九个回调值得的地方。

**没有照搬的一半,点名而不是打桩。** 上游另外十二个方法是格式化和解析
(`formatMonthYear`、`formatYear`、`formatMediumDate`、`formatShortMonthDay`、
`formatShortDate`、`formatFullDate`、`formatCompactDate`、`parseCompactDate`、
`dateHelpText`),每一个都收 `MaterialLocalizations`,而这个 crate 还没有它(本地化那
波)。在 trait 文档里逐个点名,这样将来补上是**新增**而不是**修正**。

**`DateUtils` 记账而不是另写。** 上游那是一个 `abstract final class`——Dart 里的「一袋静
态函数,不许实例化」;Rust 的对应物就是一个模块里的自由函数,而 `pickers.rs` 正是那个模
块,每个函数的文档早就点了它对应的哪个 `DateUtils` 静态方法。和 `MatrixUtils` 同一处理,
账本里有先例。

验证:`cargo test --lib` 1432 绿,GN `rustflutter_unittests` 1432 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1227 accounted / 646 MISSING。

### 走弧线的点和矩形,以及那张动效表(2026-08-20)

两个新模块:`arc.rs`(上游 `material/arc.dart`,3 个全到)和 `motion.rs`
(`material/motion.dart`,2 个全到)。两份文件都是纯算术。

**为什么要弧线:** Material 的动效规范说,一个同时在两个方向上移动的东西应该走弧线。直
着斜穿读起来是机械的——现实里没有东西沿着一条弦起步和停下——所以一张飞向新位置的卡片是
荡过去的。

**弧线的挑法**是让圆心落在两个轴里**较短**的那一边上,于是弧朝长轴鼓出去。有意思的是两
边都不大的时候:**几乎沿着一个轴的移动根本不走弧**。上游 `_kOnAxisDelta` 是两个逻辑像
素,越不过就退回直线插值——两个点在一个轴上只差一像素时,弧要的半径大得离谱,看着就是抖
一下。

**回归行盯的地方:**

* 沿轴的移动**留成直的**,而且阈值是 `<=`:正好两像素仍算沿轴,2.1 才开始走弧。
* 斜着的移动**真的荡出去**——中点离那条弦有距离。
* **两端是直接给出的,不是算出来的。** 弧的端点来自三角函数,只准到一个舍入误差,而
  `t == 1` 上的舍入误差就是一个东西停在离目的地一像素的地方。
* **弧一直待在它自己那个圆上**:每一步到圆心的距离都是半径——这条盯的是角度、半径、圆心
  三者互相对得上。
* **矩形挑的是指向它去处的那条对角线。** 上游取和两个中心连线点积最大的那条。固定挑一对
  角的话,一个往右下走的矩形会因为一个角跑在另一个前面而被拉伸挤压;挑领头的那条对角
  线,两条弧大致平行于运动方向,矩形形状就保住了。
* **打平的时候每次都朝同一边破。** 上游的 `_maxBy` 只在**严格大于**时替换,所以并列时留
  下第一个。一个直着往下走的矩形有两条对角线得分相同,而一个每帧破法不同的平局会让弧线
  在飞行途中翻面。
* **矩形弧会把角摆正**:选中的对角线可能是反着的,所以两条弧的落点是排序后用的——右边在
  左边左边的矩形不是矩形。
* **中心弧荡的是中心、尺寸是直插的**,这就是它和角弧的全部分别:一个动的是什么,不是怎
  么动。
* 两个矩形 tween 的两端也都是直接给出的。

**记一处故意和上游不一样的地方,这一轮唯一一处。** 上游 `MaterialPointArcTween.endAngle`
这个 getter 返回的是 `_beginAngle`——旁边就摆着 `_endAngle` 字段,而 `lerp` 读的是对的那
一个,所以**动画是对的、只有这个访问器在说谎**,而它除了 `toString` 没人用。这里按名字承
诺的那样移植,而不是按写下来的那样:另一条路是留一个名字和答案不符的公开访问器,而且没有
任何东西依赖那个错答案。这处偏离是**故意的**,也用一条回归行钉住(断言两个角不相等),谁
把上游的写法「还原」回来都会红。

`motion.rs` 那两张表是上游从 Material token 数据库生成的,两个都是 `abstract final
class`——Dart 里的「这是一袋常量,不许实例化」。这里用带关联常量的单元结构体说同一句话。
十六个时长看着像犹豫,其实相反:**一个给自己的时长起了名字的设计系统,改一个数就能改掉整
个应用的快慢**,而一个去拿 `Durations::MEDIUM2` 而不是写 `300` 的控件,是会跟着改的那种
控件。回归行钉了:这条尺度只增不减;**每条缓动都从零开始、到一结束**(否则控件会在动画的
一端跳一下);**accelerate 慢慢离开、decelerate 快快到达**(离开的东西不该要眼睛跟出去,
到达的东西该在它停下的地方被接住);上游那条写成 cubic 的 `linear` 确实就是直线;
emphasized 比 standard 拉得更开。

这一轮有两条测试先写错了、实现是忠实的:一条是浮点相等(矩形是从中心和尺寸重建的,边上带
着中心的舍入),一条是我把测试里那个矩形的高算错了。都改成说清规则的写法。

验证:`cargo test --lib` 1425 绿,GN `rustflutter_unittests` 1425 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1224 accounted / 649 MISSING。

### 那颗按钮站在哪儿(2026-08-20)

新模块 `fab_location.rs`,上游 `material/floating_action_button_location.dart`
11 个全到,外加它的输入 `ScaffoldPrelayoutGeometry`(来自 `scaffold.dart`)。这一轮
+12。

**整份文件就是对一个输入做算术**:scaffold 把别的东西都摆完、还没摆这颗按钮时交出来的那
份几何。这个顺序正是这个类存在的理由——按钮是**最后**摆的,所以它能被告知内容到哪儿结
束、snack bar 多高、键盘顶进来多少。

**十九个具名位置不是十九个类的行为**,而是三条水平规则 × 四条垂直规则 ×(要不要 mini
微调)。上游用 mixin 表达,每条规则一个,由十九个类各自组合。这里每条规则是一个装着自
己那段公式的单元结构体——**一个没有状态的 mixin 就是一个函数的命名空间**——而
`FloatingActionButtonLocation` 是那两个选择加一个标志,十九个是它的常量。再多一种组合的
代价是一个常量,不是一个类,而这正是那些 mixin 想要的。

`StandardFabLocation` 做成 **trait 带一个默认方法**:上游它是一个抽象子类,把一个方法拆
成三个(`getOffsetX`/`getOffsetY`/`isMini`)、再用它们供出 `getOffset`。Rust 的 trait
带 provided method 就是同一个安排。

**回归行盯的地方:**

* start/end **随阅读方向对调,center 不动**。
* **mini 微调在两端符号相反**:mini 按钮更小,要往边上再推一点,好让**视觉上的**边距和
  全尺寸的一样;往哪边推取决于是哪条边。
* **居中的按钮忽略 mini 微调**——上游也忽略:那个微调是为了让边距看着对,而居中的按钮没
  有边距要守。
* **top 的按钮骑在 app bar 的下边缘上**(一半在上一半在下);**而没有 bar 可骑时它坐进
  安全区里面**——骑在那条边上就等于半个在刘海底下。
* float 按标准边距离开底边;**snack bar 把它顶上去,bottom sheet 顶得更多**(sheet 是让
  按钮的**中心**落在它的上边缘)。
* **盖住同一块地方的两样东西算一个遮挡**:每一条都是 `min` 而不是减法,所以 snack bar 叠
  在 sheet 上不会把按钮顶两次。
* **docked 的按钮永远不会挂在 scaffold 外面。** 「docked」是中心落在栏的上边缘,这里算出
  来是 772;但上游最后有一句 `min(maxFabY, fabY)`,而内容一直铺到最底时根本没有栏可
  docked,于是这个钳位赢了,按钮落在 744、整个在屏内。没有这条,一个没有底栏的 scaffold
  会画出半个挂在边缘外的按钮。
* **键盘把 docked 的按钮推开**(上游那三分支边距的第三支:半个按钮高 + 标准边距);而
  **内容底下本来就有地方时不加任何边距**(第一支)。
* **contained 是居中在栏**里面而不是骑在栏边上,这就是它和 docked 的全部分别。
* **比栏还高的 contained 按钮仍然留在屏内**:上游让那个间隙**可以是负的**(装不下时宁可
  从栏顶探出去,也不要居中后两头都被切),然后 `min(maxFabY, fabY)` 再把整颗按钮拉回屏
  内。
* **scaling 动画是跳的不是滑的**:按钮在旧位置缩到无、在新位置长回来,在中点跳过去。
  **缩放正好在偏移跳变的那一刻归零**——这才让那一跳看不见,也是那两个阈值是同一个数的原
  因。
* **被打断的移动从它当时那个大小重启**(`min(1 - previous, previous)`):在「开始」阶段
  被打断就接着走,在「结束」阶段被打断就当作从同一点反向开始。两种都不会让按钮的大小跳
  一下。

**记下一处上游注释和代码不一致的地方。** `getRotationAnimation` 旁边写着「this rotation
will turn on the way in, but not on the way out」,而代码把 `Tween(0.75 → 1.0)` 放在
**前半段**——按钮正在缩走的那半段——后半段是一个常量零。**照代码移植,不照注释**;一个跟
着注释写的移植会和它所移植的框架动得不一样。这条也用回归行钉住,免得有人照着那句话把数
字「改对」。

这一轮有三条测试先写错了,实现是忠实的:两条是我没算上 `maxFabY` 那个钳位(改完反而成了
上面两条更有用的回归行),一条是我自己把 scale 的曲线写反了——那一条是**实现**的错,已经
按上游改回:两半读的是**同一条** `Interval(0.5, 1.0, ease)`,第二半在 `progress`、第一
半在 `1 - progress`,所以缩就是长倒着放,两半不会走散。

验证:`cargo test --lib` 1410 绿,GN `rustflutter_unittests` 1410 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1219 accounted / 654 MISSING。

### 墨迹落在的那张面(2026-08-20)

新模块 `material.rs`,上游 `material/material.dart` 四个全到:`MaterialType`、
`InkFeature`、`MaterialInkController`、`Material`、`ShapeBorderTween`。
`material.dart` 4/4。

**这个文件存在的理由是第三件事。** 一张 `Material` 同时是:一个带颜色的形状;一个抬升
(浅色里是阴影,暗色里是把面本身提亮);以及**墨迹被画在的地方**。前两件寻常,第三件不
是:整个应用里每一次溅开、涟漪、高亮,都是由控件**之上最近的那张 Material** 画的,不是
控件自己画的。一个自己画溅开的控件,会把它画在**自己里面**——被自己的边界裁掉、压在自己
画的东西下面、并且在自己重建的那一刻消失。把墨交给下面那张面,它才落在面的底色之上、内
容之下,并且比开启它的那次交互活得久——而这正是墨水的行为。

**回归行盯的地方:**

* **五种类型里只有两种圆角**(card 和 button,各 2)。circle 的 `None` 不是「半径 0」:
  圆的形状是它自己的,上游对同时给了半径的调用方是断言报错的。
* **透明的 material 让按压穿过去,其余四种不让。** 画出来的面是个东西,透明的是个洞:点
  在卡片上两个按钮之间的空档,不该落到卡片背后去。
* **只有 canvas 和 card 有自己的颜色**,另外三种答「没有」——上游随后断言一个非透明、又
  没颜色的 material 是错的,也就是说 button 型必须被告知颜色。而外部直接给的颜色压过这
  一切。
* **抬起的 material 会被染色而不是就那么平着**——暗色里阴影看不见,染色才是它「抬起来」
  的样子。
* 自己给的圆角压过类型给的。
* **加墨会把 material 标记为需要重绘**:新 feature 在屏幕上还什么都没有,所以加它的那一
  帧是变了的一帧。而「需要重绘」这个问题**一帧只问一次**。
* **feature 按加入顺序绘制**——这就是为什么一次按压抬起的高亮压在同一次按压开启的溅开之
  上:它是后加的。
* 淡完的溅开会被丢掉,**而这一丢本身也是一次重绘**(最后那圈得擦掉);然后就真的没有了。
* **装饰不是动画,永远不结束**:它随放下它的 widget 一起走,不随时钟走。上游的
  `InkDecoration` 从不自己 dispose。
* **改变尺寸会重绘墨迹但不会结束它**(上游 `_didChangeLayout`):一个按着改了尺寸的盒子
  量出来的溅开是位置错了,不是画完了。而**空的 material 不在乎自己改了尺寸**。
* 控制器**带着一个它并不绘制的颜色**——上游注释原话:「这个颜色的实际绘制是 build 里的
  一个 Container 做的」。它在这儿是为了让 feature 能问自己落在什么上面。

**`ShapeBorderTween` 上有两处看着像 bug 而不是 bug,各钉一条:**

* **null 的那一端是「被缩到无」的形状,不是「没有形状」。** 上游会走到
  `a.lerpTo(null, t)`,那是 `a.scale(1 - t)`——所以把一个圆插值到 null,终点是一个宽度为
  零的圆形边框,而且**每一个 t(包括 1)结果都是 `Some`**。等着在终点拿到 `None` 的调用
  方会踩到。
* **两端拿到的不是当初给的那两个类。** 圆插值到 stadium,**每一个 t**(0 和 1 也在内)答
  的都是过渡形状,只是参数化成它当下该有的样子。保证的是几何,不是类型——一个按变体
  `match` 的调用方必须知道这条。

这两条都是我先把测试写错、实现是忠实的:改的是测试,并且把它变成了说清上游规则的回归
行。

**没有照搬的三处:** `Material.of(context)`(上游是祖先里找 render object,这个 crate 的
`BuildContext` 没有祖先游走——和 `LookupBoundary` 等的是同一个口子,所以控制器是被传下去
而不是被找到的);`_MaterialInterior` 那套隐式动画(`Lerp: Copy`,`ShapeBorder` 不
是——`ShapeBorderTween` 在,算术有了,缺的是驱动);`AnimatedDefaultTextStyle`(这个
crate 没有环境文本样式)。

验证:`cargo test --lib` 1390 绿,GN `rustflutter_unittests` 1390 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1207 accounted / 666 MISSING。

### 答复触摸的那块地方(2026-08-19)

新模块 `ink_well.rs`,上游 `material/ink_well.dart` 剩下的两个:`InkResponse` 和
`InkWell`。墨迹本身(溅开、涟漪、高亮)在 `ink.rs`;这里是决定**什么时候**做一个、
**做哪一个**的那部分,而这就是这个类的全部。

**先把溅开和高亮的分别写下来,因为数据结构是照着它长的。** 溅开是**事件**:它标记接触
的那一刻,会走,然后不管接下来发生什么它都会消失。高亮是**状态**:指针在上面、按着、或
者控件有焦点时它就亮着,直到那件事不再成立。所以**当前**溅开至多一个,而在飞的溅开可以
有一堆——第二次按下会开一个新的、并把上一个**取消掉而不是删掉**;高亮则是每种恰好一个,
是被**重新激活**而不是换掉。

**回归行盯的地方:**

* **第二次按下取消第一个溅开而不是删掉它。** 发生了两件事,表面上就有两道痕;第一道在原
  地继续淡,第二道同时长起来。
* **点击只确认当前那一个。** 早先那个已经被取消了、也必须保持取消:一次点击不会追认一次
  读者已经放弃的按压。
* **确认两次不可能发生**,因为上游在确认的同一口气里把 `_currentSplash` 置空了。没有这
  条,平台真的会送来的第二个 up 会把淡出的起点往后挪,让溅开活得比它那次点击还久。
* **向一个高亮要它已经处于的状态,什么都不改、也什么都不报。** 这条早返回要紧的原因很
  具体:一只停着不动的鼠标,否则会在每一个重新读指针的帧上调一次 `onHover(true)`。
* **淡出途中回来的高亮还是同一个高亮**:上游对现有的那个调 `activate()` 而不是新建,所
  以 alpha 从它当时的位置继续,而不是从零重来。这条从满值走到一半、再回来,盯的是「从一
  半亮起来」。
* **按压高亮淡得比 hover 和 focus 慢**(200ms 对 50ms)。按压是读者做出的动作、看得见;
  hover 和 focus 跟的是指针或 Tab 键,而那两样在 200ms 的淡出走完之前早就到别处去了。
  `hoverDuration` 覆盖的是后两者,不是按压。
* **按住不放、按过了它自己的动画,当前溅开也不会被丢掉**:它还没落定,所以它没结束,而
  且手指最终抬起来时它还得在。
* 落定并淡完的溅开会被丢掉,高亮也一样;而**什么都不剩时,一帧就没有话要说**。
* **停用的 response 照样把高亮做出来,只是 alpha 是 0。** 上游写的是
  `enabled ? resolved : resolved.withAlpha(0)`,看着像浪费,直到发现理由是生命周期:高
  亮必须存在,这样重新变成可用时才是一次**颜色变化**,而不是一个高亮在 hover 进行到一半
  时凭空冒出来。
* **`InkWell` 就是「裹住 + 矩形高亮」的 `InkResponse`。** 这两个设置是配对的:矩形高亮
  会铺满它的盒子,所以一个不裹住自己的 response 会把高亮画到自己边界外面。上游它是个子
  类,这里是个门面(和 `Wrap`、`OverflowBar` 同一种)。

**没有照搬的四处,各有上游自己的理由:**

* **嵌套 response 的登记表**(`_ParentInkResponseProvider`/
  `markChildInkResponsePressed`)。它是为了让套在另一个里面的 `InkWell` 不要两个都溅
  ——内层告诉外层「我按下了」,外层就不做。它需要一个**后代能写**的 inherited 值,而这个
  crate 的 `provide`/`inherited` 不干这个。
* **`wantKeepAlive`**。上游会在一个滚出窗口的 response 还有墨迹在飞时留住它,免得动画中
  途消失。这个 crate 根本没有 keep-alive(和 `grid.rs` 记的是同一个口子)。
* **焦点高亮模式**。上游在 `FocusManager` 处于触摸模式时干脆不显示焦点高亮——触摸屏上的
  焦点环没有意义。这里焦点高亮和另外两个一样由 `update_highlight` 驱动,谁拥有焦点谁
  调。
* **`statesController` / `overlayColor`**。一个解析出来的 overlay 颜色会一次替掉
  `highlightColor`/`hoverColor`/`focusColor` 三个;三个都在,而那个覆盖它们的属性不在,
  等 `widget_state` 铺到这个控件。

顺带给 `InteractiveInkFeature` 补了 `activate`/`active`——上游 `InkHighlight` 有,而这轮
的状态机真的要用:重新激活时淡入要**从它当时走到的地方往前**走,不是从零。

验证:`cargo test --lib` 1376 绿,GN `rustflutter_unittests` 1376 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1203 accounted / 670 MISSING。`ink_well.dart` 4/4 全覆盖。

### 三种墨迹共用的那个底座,挑墨迹的工厂,以及画进材料里的装饰(2026-08-19)

接着上一轮:`InteractiveInkFeature`、`InteractiveInkFeatureFactory`
(`ink_well.dart`)和 `InkDecoration`(`ink_decoration.dart`),都进 `ink.rs`。

**底座按这个 crate 的老办法做:抽象基类 + 三个子类 → 一个结构体拿共有的东西,加一个
`InkFeatureKind` 枚举包住各自的结构体**——和 `ShapeBorder` 同一个形状,理由也同一个:这
个集合是封闭的,而一个必须交代每个变体的 `match`,正是防着第四个变体被加了一半。

**阶段(phase)放在底座上而不是变体里**,因为 `confirm` 和 `cancel` 写的就是它,而它也
是三者都读的那一份状态。上游把它放在每个 feature 各自的 `AnimationController` 里;分别
还是那条——这里的动画是逐帧算术,所以一个 feature 是一个由持有者推进的值。

**回归行盯的地方:**

* **工厂就是那条半径规则的选择器**:同一个盒子、同一次触摸,两者答出 500 和 255。这正是
  一个主题一次换掉全应用溅开的意义。
* 默认工厂是**涟漪**(上游 M2 主题的默认)。
* **已经落定的 feature 不会再落定一次。** 上游那时控制器已经在跑,再 `forward()` 一次什
  么都不做;这里若不挡,就会把淡出的起点往后挪——溅开会活得比它那次点击还久。
* **绘制颜色是按 feature 自己的 alpha 缩放的**,不是一个固定 alpha:本来就半透明的覆盖
  色,在「全不透明」时仍是那么半透明,再从那里淡下去。
* **停用的高亮从它当时走到的地方淡回去**——这正是底座要在落定那一刻先把 alpha 存下来的
  原因:半途被打断的高亮不能先跳到满值再淡,而「从头读淡出」就会那样。
* **矩形高亮没有圆可画。** 上游用的是 `drawRect`/`drawRRect`,和 `paintInkCircle` 不是同
  一个调用,所以「没有」才是老实的答案,而不是给一个和盒子一样大的圆。
* **看不见的 `InkDecoration` 留着它的装饰、什么都不画。** 上游的 `isVisible` 是一个单独
  的开关而不是把装饰清掉:widget 还在、尺寸还是那个,回来时也不用重建 painter。

**`InkDecoration` 为什么不是一个 `Container`。** 一个夹在材料和内容之间的盒子画出来的装
饰,会被它自己的孩子盖住、却盖不住任何东西——而溅开是材料画在它所有内容**之上**的,于是
装饰会被溅开盖掉。把它当作一个 ink feature 来画,就把它放进了材料自己那张表里:在溅开之
下、在材料底色之上,这正是一张可点表面上的背景图该在的地方。

`InkSparkle` 没有进工厂枚举,因为它没有进这个 crate——整个效果是一支片元着色器,渲染 ABI
没有着色器通路(见 `coverage_ledger.json`)。

`ink_well.dart` 还剩 `InkResponse` 和 `InkWell` 两个,下一轮单独做:它们是手势 + 高亮状
态机,而不是几何。

验证:`cargo test --lib` 1366 绿,GN `rustflutter_unittests` 1366 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1201 accounted / 672 MISSING。

### 三种墨迹,以及它们之间那点最容易搞错的分别(2026-08-19)

`ink.rs` 里添上上游的三个 ink feature:`InkSplash`(`ink_splash.dart`)、
`InkRipple`(`ink_ripple.dart`)、`InkHighlight`(`ink_highlight.dart`);
`InkSparkle` 记为引擎受阻。

**最容易搞错的一处,先说:两者的目标半径不是一回事。** `InkSplash` 长向**离落指点最远
的那个角**——所以贴边点一下也能铺满整块;`InkRipple` 长向**盒子对角线的一半**——所以涟
漪不管点在哪儿都一样大。上游把后者写成
`max(|bottomRight|, |topRight - bottomLeft|) / 2`,而这两个距离**是同一个数**(矩形的两
条对角线都是 `sqrt(w²+h²)`),那个 `max` 纯属装饰。这条也钉了回归行,免得有人把其中一
个「修」成另一个不是的东西。

**记账的边界:** 上游每个 feature 是两三个 `AnimationController` 加一对由 `InkWell` 在
手势落定时调用的 `confirm`/`cancel`。这里是调用方持有、feature 读取的一个
`InkPhase`——因为这个 crate 的动画是逐帧算术而不是带监听器的控制器。`confirm` 和
`cancel` 的分别不是装饰:确认过的点击**先快速长完再淡出**,让读者看见自己按中的是什么;
取消掉的(一次从按压开始的滚动)**立刻淡走**,不留任何东西去确认一次并不是按压的按压。

**回归行盯的地方:**

* 溅开找最远的角,涟漪找半条对角线;而**没有盒子可填的溅开取那个 35 的平值**,盒子多大
  都不说明什么。
* 涟漪**从 30% 起、并且超出 5**;溅开**从零起、线性长**,半径上根本没有曲线。
* **确认过的溅开是按速度收尾的**(`targetRadius / 1px每毫秒`),大盒子填得久——这才让墨
  水读起来像墨水而不像计时器;**确认过的涟漪是按固定时长收尾的**,盒子再大也 225ms。两
  条摆在一起,分别在大表面上最明显。
* **涟漪先把盒子填满,再开始离开。** 上游给淡出加了 `Interval(225/375, 1.0)`,注释说淡
  出在 225ms 之后才开始——正好是半径的时长。
* **取消不等。** 从颜色当时走到的地方淡走,而不是从满值。
* **淡入还在跑时由淡入说了算**(`_fadeInController.isAnimating ? _fadeIn : _fadeOut`):
  75ms 内确认的快速点击仍然继续**淡入**,而不是立刻开始离开。
* 只有**没被裹住的**溅开才往中心走——裹住的有东西可填、没理由挪;没裹住的卡在角上像出了
  错。
* **手势落定前不淡。** 上游的 alpha 控制器不在构造里启动,按住不放的手指颜色一直满;而
  `confirm` 和 `cancel` **两个都**启动它——溅开两种情形都会消失,分别只在圆长得多快。
* 高亮**根本没有半径动画**:溅开是事件,高亮是状态,所以一个有会走的形状,另一个只有在
  不在。
* **半途被打断的高亮从半途淡回去**,不是从满值——从满值意味着往外走的路上先变亮一下。
* **淡到零但又被重新激活的高亮要活下来**:上游的条件是 `isDismissed && !_active`,不是
  「到零就丢」。指针回来时那还是同一个高亮。

**顺带记下一条这个 crate 自己的实情。** 活着的 `Ink` 区域用的是 `InkSplash` 的目标半径
配 `InkRipple` 的曲线——上游没有这个组合。这轮没有改它:改动是视觉上的,而这一轮的闸门
没法用眼睛复验;两者现在并排写着,要改就是 `Ink` 里那一行 `splash_radius`,换成
`InkRipple::target_radius`。模块文档里点了名。

**`InkSparkle` 记为引擎受阻。** 它整个效果就是
`ui.FragmentProgram.fromAsset("shaders/ink_sparkle.frag")` 那支片元着色器,而渲染 ABI 没
有着色器通路。

验证:`cargo test --lib` 1358 绿,GN `rustflutter_unittests` 1358 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1198 accounted / 675 MISSING。

### 一条横幅,以及被上游自己弃用掉的那条按钮栏(2026-08-19)

`MaterialBanner`(进 `components.rs`)和它的三步回退
`ResolvedMaterialBanner`(进 `component_themes.rs`),用的正是上一轮刚落地的
`OverflowBar`;另外把 `ButtonBar` 记到账上,对应物就是 `OverflowBar`。

**单行规则值得写下来:动作只有一个、且没有被强制下移**,动作就排在**内容那一行**上;
别的情况都自己占一行。上游的条件是 `actions.length == 1 && !forceActionsBelow`——是
**个数**,不是「放不放得下」,两个短动作照样自己占一行。两种情形的 padding 不同也有理
由:同一行时,那条 52 高的动作栏本身撑开了横幅,内容几乎不需要自己的上内边距;而堆下去
以后没有别的东西把文字顶离上边缘,横幅只好自己来(上 24 下 4)。

**记下一处看着像疏漏而不是疏漏的地方:** 上游算高度的表达式是
`widget.elevation ?? bannerTheme.elevation ?? 0.0`——它**根本没走到** `defaults`,而
`_BannerDefaultsM3.elevation` 是 1.0。所以一条没有主题的横幅是**平贴在页面上**的,不是
抬起一级。照抄了,并且用一条回归行钉住——一个「顺手修好」的移植会给出和它所移植的框架
不同的答案。

**回归行盯的地方:**

* 只有一个动作才和内容共用一行;两个不行,零个不行,一个但被强制下移也不行。
* 动作**自己占一行以后横幅更高**——动作栏挪到内容下面,同时内容的上内边距从 2 长到
  24。
* 没有主题时抬升是 **0 而不是 1**(上面那处)。
* **抬起的横幅在自己下面留出 10 给自己的阴影**,平的不留——没有阴影就没有要留的地方。
* 内容内边距**跟着动作去了哪儿变**,而且两种都从阅读边起 16、都会镜像。
* **主题给的 padding 压过两个默认值**,不是只压过其中一个——否则一个设了 padding 的主
  题会在动作堆起来时把单行默认值又拿回来。

**没有照搬的:** 上游的 banner 是 `StatefulWidget`,是因为 `ScaffoldMessenger` 递给它的
那个 `animation`——滑入用的高度因子、动画走完时触发的 `onVisible`、它作为 `Hero` 飞的那
段、以及专供 `showMaterialBanner` 调用的
`withAnimation`/`createAnimationController`。这里没有 `ScaffoldMessenger`,所以移植的是
上游自己那条 animation 为空的路径——它在源码里的原话是「this provides a static
banner」。谁把横幅摆上去,谁就拥有它在不在屏幕上,和这个 crate 里每一个浮层一样。

另一处:上游写的是 `Divider(height: 0)`,不占空间、把发丝线画在横幅自己的下边缘上;这
个渲染器是把线画进给它的盒子里的,所以那条规则占掉了它那一个像素。

**`ButtonBar` 记账而不是另写。** 上游自己给它标了
`@Deprecated("Use OverflowBar instead")`,而它的 build 就是把子项交给一个会溢出成列的
行。对应物就是 `OverflowBar` 本身——再写一个已经被弃用的壳,是在移植一个上游正在删的东
西。

验证:`cargo test --lib` 1343 绿,GN `rustflutter_unittests` 1343 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1194 accounted / 679 MISSING。

### 一行放不下就整列站好,以及三个小外框(2026-08-19)

`overflow_bar.rs`(新),上游 `widgets/overflow_bar.dart` 的 `OverflowBar` 和它的
`_RenderOverflowBar`;外加三个 material 小类,各自进它同类所在的文件:`GridTile` 和
`GridTileBar` 进 `grid.rs`,`DrawerHeader` 进 `drawer.rs`。

**`OverflowBar` 要解决的是对话框的那排按钮。** 一排读起来最好,通常也放得下;字号放大
了、或者换成标签更长的语言,就放不下。放不下的 `Row` 是溢出——画到自己边界外面还报
警。这个在放得下时排成一行、放不下时**整列**站好,如此而已:不部分换行、不省略、不压
缩。

**这也正是它和 `RenderWrap` 的分别。** wrap 是每行能塞几个塞几个、塞不下开新行;这个
是**全有或全无**。对按钮来说这才对——三个按钮分成两行看着像出了错,三个按钮排成一列看
着是有意为之。

**记账的边界:** 上游的 `alignment` 是**可空**的,而那个 null 不是 `start` 的同义词:
没有 alignment 时这条 bar 只取子项需要的宽度,有 alignment 时取给它的全部宽度——因为
alignment 回答的是「多出来的空档怎么办」,而收紧到内容宽的 bar 根本没有空档。所以这里
留成 `Option`。

**回归行盯的地方:**

* 放得下就是一行,而且**收紧到内容宽**:没有 alignment 时它不占满给它的宽度。
* 一行里的子项**按最高的那个居中**——这才让高矮不一的按钮读成一条 bar 而不是一条毛边。
* 放不下就**全部**站成一列,不是只把放不下的那几个挪下去。
* **是 spacing 把一行压成一列的。** 两个 50 宽的子项在 100 里正好放得下——直到把它们之
  间的那道缝也算进去,而算它正是要点。
* 站成列时,子项是贴着 **bar 自己的边**放的(用的是约束的宽度,不是最宽子项的宽度),
  所以 `End` 能够到右边缘。
* **往上堆时最后一个子项在最上面。** 对话框的按钮一旦堆起来正是要这样:写在最后的确认
  动作,落到离拇指最近的地方。
* RTL 下一行**从右边缘排起**,而推进 x 时**减的是下一个子项的宽度**而不是当前这个的。
  这不是怪癖:偏移量是左边缘,而 RTL 下一个子项的左边缘取决于那个子项有多宽,当前这个
  说不出来。
* RTL 把站列时的 start/end 对调,**而 center 不动**。
* **两个高度的内在尺寸都拿子项的最小宽度去判断会不会站成列**,连问最大高度时也是。写
  下来是因为它看着像笔误而不是:要回答的问题是「这会不会不得不站成列」,而只有连子项
  最窄时都放不下才不得不。
* 空 bar 取最小尺寸,而 spacing 没有东西可夹,所以贡献零而不是一道负的缝。
* dry layout 和真 layout 在三个宽度上逐一对上——它们共用同一个 `place`,这条盯的是这一
  点没有走散。

**`GridTile` 那条提前返回值得留着。** 上游在既没有 header 也没有 footer 时直接返回
child,不是套一个只装一个孩子的 stack。照片墙每个格子建一个,而一个什么都不装的 stack
是每格一趟布局、一层绘制,换不来任何画面上的分别。

**`GridTileBar` 永远是暗色的。** 上游把内容裹在 `Theme(data: ThemeData.dark())` 和一层
白色 `IconTheme` 里,不管外面的主题是什么。理由是这条 bar 底下是什么:一张照片,颜色没
人选过。暗色文字压在未知图片上是读不出来的,白色文字不会。另外**哪一端有东西,哪一端
的内边距就收窄**(16 变 8)——图标自己的框里已经带了视觉留白,再给满 16 会读成一道
缝。

**`DrawerHeader` 关于状态栏的两件事:** 高度是状态栏高度**加**一个固定的 161,不是固定
161(那样内容会被刘海挤下去、丢掉底部),也不是忽略状态栏(那样会画到时钟底下);**按
刘海高度长高,画出来的区域才不变**。然后它把同一个 inset 加进自己的 padding、又从 child
身上**摘掉**(上游的 `MediaQuery.removePadding(removeTop: true)`)——header 已经用长高
的方式消化了这个 inset,child 再让一次就是让了两遍。那个 161 是 160 加上它自己画的那
道发丝线,所以线以上的内容是整 160。

**没有照搬的一处:** 上游的主体是 `AnimatedContainer`,decoration 变了会走过去。这个
crate 的隐式动画助手 `implicit::animated` 是基于 `Lerp` 的,而 `Lerp: Copy`,
`Decoration` 不是 `Copy`;所以 decoration 是直接套上去的,变了就是跳变。`duration` 和
`curve` 两个字段**没有**留在 API 上而不是留着不用——一个什么都不做的字段比一个不存在的
字段更糟,而将来真有了 `AnimatedContainer`,补回去是同一笔改动。

顺带两个小口子,都是这轮真的要用才开的:`RenderStack::push_positioned_boxed`(和
`push_boxed` 之于 `push` 一样),以及上游那个静态的
`Divider::create_border_side(context)`——调用方想要的是「一道分隔线的边」而不是一条分隔
线时读它,主题挪动分隔线,借了它的每一道边也跟着挪。

验证:`cargo test --lib` 1337 绿,GN `rustflutter_unittests` 1337 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1192 accounted / 681 MISSING。

### 竖分隔线、表格数据源、圆头像(2026-08-19)

三个上游类,各来自 material 的一个文件,都进 `components.rs`(它们各自的同类已经在那
里):`VerticalDivider`、`DataTableSource`、`CircleAvatar`。

**`VerticalDivider` 上游是独立的类而不是 `Divider` 上的一个轴向参数**,这里照做。两者
读的是**同一批**主题字段,而 `space` 在这里指宽度、在那里指高度;做成一个带轴向的
widget,就得在每个调用点解释这个反转。

**`CircleAvatar` 那三个半径的规则值得写下来。** `radius` 把尺寸钉死;`minRadius` 和
`maxRadius` 给出范围、让父级的约束在其中挑。让它读得通的是上游这条:**三个全不设**才
是「就用默认半径」,而只要设了**任何一个**,默认值对两端就都不再适用。没有这条,一个只
给了 `maxRadius` 的调用方会悄悄地仍然拿到默认值当下限——正好和他要的相反。这条两个方
向都钉了。

**回归行盯的地方:**

* 三个半径全不设时就是默认尺寸,上下限相等。
* **只给上限时下限落到零**,只给下限时上限是无穷——都不是默认值。
* 固定半径把两端都钉住(这正是「固定」的意思:父级的约束没有可挑的余地),而且**固定
  半径压过范围**:三个都给时,固定的那个就是两端,范围被忽略。
* 数据源**会说出自己的行数只是个估计**。读流的数据源在读到末尾之前不知道有多少行,而
  一个把估计当真的表格会画出一根说谎的滚动条。默认是「知道」,那是常见情形。
* **源还产不出的那一行返回「没有」而不是空行。** 正在加载的一页答「没有」,表格把它画
  成占位;返回一个空行的话,它和一个真的没有单元格的行就分不出来了。

验证:`cargo test --lib` 1311 绿,GN `rustflutter_unittests` 1311 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1188 accounted / 685 MISSING。


### 让抬起的表面在暗色里看起来是抬起的(2026-08-19)

`elevation_overlay.rs`(新),上游 `material/elevation_overlay.dart` 的
`ElevationOverlay`,连同它要用而这个 crate 此前没有的两个颜色运算:
`alpha_blend`(上游 `Color.alphaBlend`)和 `with_opacity`(上游
`Color.withOpacity`)。

阴影是「抬起」在浅色背景上的读法:底下的表面变暗。暗色里这招不成立——没有更暗可去
——所以 Material 改成把抬起的表面**自己**提亮,越高越亮。M2 用一层白色叠加,M3 用种子
色的一层淡染;两条都在,因为上游两条都留着、由主题挑。

**记账的边界:** 上游的 `applyOverlay`/`overlayColor` 收 `BuildContext` 再从里面读主
题。这里收的是零件——两个颜色和两个开关——因为值得拿的是那套算术,而 context 并不能
给出调用方手上没有的东西。

**回归行盯的地方:**

* 不透明前景就是它自己、全透明前景原样留下背景。这两种情形通用公式做不了(它要除以
  结果的 alpha),所以单独列出来。
* 半白叠在黑上是灰——source-over,这个文件里所有叠加的地基。
* **淡染表在它那六个等级上要一格不差。** 那张表就是 M3 的六级抬升;这里写错一个数,
  应用里每一张卡片的染色都是错的。
* **两级之间要插值**,这才让六级读起来是连续的而不是六个台阶——不插值的话,一个带动画
  的抬升会肉眼可见地跳。
* **表外要钳位而不是外推。** 抬升 100 不是「一百倍的染色」,而是「到表的尽头为止」;
  外推会把不透明度推过 1、把表面整个洗白。
* 透明的淡染色等同于没有淡染:一个把淡染设成透明来清掉它的主题,应该原样拿回自己的颜
  色,而不是拿到一次「和空气混合」的结果。
* **叠加随抬升增长,但越来越慢**——是对数不是直线,第一毫米的抬起远比第二十毫米值钱,
  而阴影本来也是这么读的。这条用「第一步的增量 > 后来一步的增量」钉住。
* **四个条件缺一不可**,所以四个各拿掉一次:抬升要大于零(平的不算抬起);主题要选进
  (M3 改用淡染);要是暗色(浅色里阴影已经说明了);颜色得**真的是主题的 surface**
  ——叠加是针对那一个颜色定义的,把它套到任意颜色上,是在发明规范里没写的行为。
* **匹配 surface 时忽略它自己的 alpha。** 上游拿两者的全不透明版本比较,所以一张半透
  明但颜色对的表面照样得到叠加——否则一张正在淡入的卡片会在淡入途中丢掉自己的抬起。

验证:`cargo test --lib` 1305 绿,GN `rustflutter_unittests` 1305 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1184 accounted / 689 MISSING。


### 拖拽能走多远(2026-08-19)

`drag_boundary.rs`(新),上游 `widgets/drag_boundary.dart` 两个类:
`DragBoundaryDelegate`、`DragBoundary`。一个能被拖出屏幕的可拖拽对话框,就是一个被读
者弄丢的对话框。`DragBoundary` 圈出一块区域,在它里面拖拽的东西可以问两个问题:这个
位置还在里面吗;如果不在,最近的那个在里面的位置是哪。

**记账的边界:** 上游是个 `InheritedWidget`,`forRectOf` 找到元素、问它的渲染对象要
尺寸、再换算成全局坐标。这里 provide 下去的**就是那个矩形**——渲染对象的全局矩形不是
这个 crate 的 `BuildContext` 问得到的东西,所以由调用方说出矩形,而不是框架推导。上游
的 `useGlobalPosition` 是在全局矩形和原点矩形之间二选一;这个选择在这里落在调用方手
上,体现为它 provide 的是哪一个矩形。

另外上游的 `nearestPositionWithinBoundary` 在被拖对象比边界还大时抛异常。这里没有可
抛的地方,也没有合理的返回值,所以答「没有」。

**回归行盯的地方:**

* 紧贴边缘仍然算在里面——把这样的东西从边上推开一点,肉眼看得出来是错的。
* **整个矩形都要留在里面,不只是它的左上角。** 钳位区间的远端是「边界减去对象自己的
  尺寸」;只钳左上角的话,一个宽的东西会从右边挂出去——那正是这个减法要防的错误。
* 每根轴各自钳位:一个从左边出去、纵向还在里面的东西只横向移动。
* **比边界还大的东西没有最近位置。** 上游在这里抛异常;边界小到装不下它时里面根本没
  有位置,而随手编一个——比如钉到某个角——会把它放在调用方从没要求过、也发现不了的地
  方。恰好等于边界尺寸的仍然放得下,而且只有一个位置。
* **没有边界意味着无界而不是禁止。** 这是上面什么都没设时 `for_rect_of` 的回退值:没
  有边界的拖拽应该哪儿都能去,所以答案是宽容的——什么都在里面,什么都不用挪。
* 拖拽会找到它上面的边界,上面没有时回退到无界:拖拽的代码想知道的是「我能去哪」,不
  是「有没有人设过边界」。

验证:`cargo test --lib` 1296 绿,GN `rustflutter_unittests` 1296 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1183 accounted / 690 MISSING。


### 绕开折痕和铰链(2026-08-19)

`display_feature.rs`(新),上游 `widgets/display_feature_sub_screen.dart` 的
`DisplayFeatureSubScreen`,连同它需要的 `DisplayFeature` 与两个枚举。

折叠屏手机的屏幕上横着一样东西——折痕、铰链、摄像头开孔——而一个按整屏居中的对话框
会正好骑在上面。这个类把屏幕切成那样东西**留下的完整矩形**,挑出离锚点最近的一个,
把孩子放进去。

**这里补上了一处早先自己留的账。** `menu.rs` 的 `popup_menu_offset` 文档里写着:
「上游先按显示特征切分屏幕再把菜单塞进最近的子屏;这里的引擎绑定不报告任何显示特
征,所以唯一的子屏就是整个 overlay——那正是那个函数在这种情况下退化成的样子。」现在
那个函数真的在了,而且回归行钉住了「没有特征时就是整屏」这条退化。

**记账的边界:** `DisplayFeature` 和两个枚举是 `dart:ui` 的类——在尺子扫的
`packages/flutter/lib/src` 之外——写在这里是因为这个 crate 此前没有任何地方需要它
们。上游的 widget 从 context 里断言出文字方向,这里方向是参数,和这个 crate 其他地
方一致。

**回归行盯的地方:**

* 横贯屏幕的铰链把屏幕切成两半。
* **只穿过一部分的特征什么都不切。** 摄像头开孔是一块屏幕上的洞,不是分隔物;它两侧
  都没有完整的矩形,所以屏幕原样保留、调用方在洞上面布局——这是对的:一个为了躲开挖
  孔而跳开的对话框,看不出任何理由。
* **平展状态下零宽度的折痕不避让,半开状态下的要避让。** 平展时那是连续屏幕上的一条
  线,什么都没挡住;半开是例外,因为那时两半朝着不同方向,横跨折痕的内容读不了,尽
  管折痕本身不占地方。这条两面都钉了。
* **两个特征切两次**:切分是对特征做折叠,所以一横一竖会切出四块,而不是第二个特征被
  忽略。
* 点到矩形的距离:在里面是 0,在旁边是**直着量**,斜着在角外是**真实距离**(用一个
  3-4-5 三角形钉住)——这就是为什么那是八个区域而不是四个。
* **从左到右的读者拿到左半,从右到左的拿到右半。** 没人指定锚点时,决定对话框落在哪
  一半的正是读者语言的起始角。而**屏幕外的锚点会被拉到边上**——上游的右到左回退值是
  「最右边」,正是这道钳位把它变成屏幕上的一个真实点。
* 显式给的锚点压过读写方向。
* **平局时保留第一个子屏。** 上游用的是 `<` 而不是 `<=`,所以锚点到两半等距时,答案
  不取决于特征恰好是按什么顺序到达的。

验证:`cargo test --lib` 1288 绿,GN `rustflutter_unittests` 1288 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1181 accounted / 692 MISSING。


### 一进一出两套转场,和被打断时该怎么办(2026-08-19)

`dual_transition_builder.rs`(新),上游 `widgets/dual_transition_builder.dart` 的
`DualTransitionBuilder`。一个从右边滑进来、却是**淡出**的页面,是两套动画而不是一套
倒着放;这个 builder 收两套,按方向跑对应的那一套。

**整个文件值得读的就是「被打断时怎么办」这一条。** 显而易见的答案——切到另一套
builder——是错的:进场和出场的转场长得完全不一样,一个滑到一半的页面会从它当时所在
的位置直接跳进一段淡出。上游的做法是**继续放正在放的那一套,只是倒着放**,并且只在
动画真的到达某一端之后才允许换方向。`effective_animation_status` 就是这条规则,也是
这个文件的全部要点。

**记账的边界:** 上游是一个 `StatefulWidget`,它的 state 监听动画状态并重新指向两个
`ProxyAnimation`。这里的 state 是那个「有效状态」,由 `advance` 更新——这个 crate 的
动画一律由每帧的 `advance` 驱动而不是监听器,和其中所有别的转场同形。

**回归行盯的地方:**

* **已经落地的动画按字面采信**:两端都会覆盖之前的状态,因为没有任何东西还在飞。
* **被打断的转场继续放原来那一套。** 反向转场被正向打断,有效状态仍是反向——只是正着
  跑。这是这个文件存在的理由。
* 从静止开始的转场就是它开始的那个方向,包括从「不对的那一端」开始:一个已经在
  dismissed 的动画被要求 reverse,那就是一段反向转场,尽管听着别扭。
* **动画到达某一端之后方向才能变。** 打断只在有东西在飞时被拒绝;落地清掉它,下一个
  方向就被接受。这条用「先拒绝、再落地、再接受」三步钉住。
* **completed 对应的是 reverse 阶段,dismissed 对应 forward 阶段。** 这读起来别扭,
  直到想清楚为什么:停在远端时,屏幕上那个东西正是**反向** builder 将要把它动画走的
  那一个,所以必须是那个 builder 活着并托着它。
* **没在跑的那个 builder 是被钉住的,不是被放着不管。** 钉在它已经到达的那一端:反向
  在跑时正向钉在 complete,正向在跑时反向钉在 dismissed。任何一个放任自由,都会在正
  在跑的那套底下自己倒着走一遍。
* `advance` **只在方向真的变了时**才报告变化,没变的那一帧不重指代理。
* **换进来的新动画会立刻被问状态**,而不是等它下次变化——否则一个换进来时就已经在跑
  的动画,会被当成还停在旧动画的状态上。

验证:`cargo test --lib` 1278 绿,GN `rustflutter_unittests` 1278 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1180 accounted / 693 MISSING。


### 三样「被告知的东西」(2026-08-19)

`icon_data.rs`(新),三个上游类各来自一个文件:`IconData`
(`widgets/icon_data.dart`)、`ContextMenuButtonItem`(与 `ContextMenuButtonType`)、
`BottomNavigationBarItem`。三个都不是 widget——每一个都是 widget **被告知**的东西:
画哪一个字形、菜单那一项做什么、栏里的一个目的地是什么。各自单开一个文件的话每个只
有三行,所以合在一起。

**`IconData` 对这个 crate 有实际用处。** widgets.rs 里早就写着「图标就是私用区码位
上的一个字形,所以一个图标是那个字体里的单字符字符串」——`IconData` 正是那句话缺的数
据类,`to_glyph()` 就是从它到屏幕的全部路径。

**记账的边界:** `IconDataProperty` 是 `DiagnosticsProperty<IconData>` 的子类,属于
诊断树(P10),已记进台账。上游那些图标 tree-shaker 注解(`@RecordUse`、
`@mustBeConst`)是 Dart 构建用来找出应用实际用到哪些图标、把字体其余部分丢掉的;那是
对 Dart 源码的构建期分析,这里没有可以挂靠的东西。

**回归行盯的地方:**

* **图标就是字体里的一个字符**,`to_glyph()` 出来是**单个**字符。
* 图标码位通常落在**私用区**(0xE000–0xF8FF):Unicode 里没有「设置」这个字符,所以
  图标字体把字形放在没人认领的地方。而**根本不是字符的码位**(孤立代理、超出范围)
  返回 None 而不是在去排版的路上 panic。
* **镜像是图标的属性,不是布局的规则。** 箭头在从右到左的布局里要翻,齿轮不翻;只有
  图标自己知道它是哪一种,所以标志在这里而不是布局对所有东西一视同仁地套。
* **字体和码位一样重要**:同一个数字在两个字体里是两张不同的图。相等要求全部字段都
  相等。
* **没有回调的菜单项就是禁用的菜单项。** 上游用可空回调而不是另加一个 `enabled`
  标志,这个形状是对的:没事可做和被关掉是同一件事。
* 标准项在传递时传的是**类型不是标签**:标签是翻译过的,平台还可能想给自己的——iOS
  的「查询」不是这个 crate 该去发明的字符串。
* **`copy_with` 保留没给的、并且清不掉字段。** 上游的 `copyWith` 收可空参数、遇到
  null 就保留旧值,所以它无法清空一个字段,上游接受了这一点。这里照做,好让两边行为
  一致,而不是这一边悄悄地「更好但不一样」。
* 屏幕阅读器念的和标签**是两样东西**:标签常常是一个只有配着图标才说得通的词。

验证:`cargo test --lib` 1270 绿,GN `rustflutter_unittests` 1270 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1179 accounted / 694 MISSING。


### 页面走了,滚动位置留下(2026-08-19)

`page_storage.rs`(新),上游 `widgets/page_storage.dart` 三个类:
`PageStorageKey`、`PageStorageBucket`、`PageStorage`。

一个滚到一半的标签页,切走再切回来,应该还在一半的位置。widget 树里没有任何东西记
得住这件事——标签页的 state 跟着标签页一起没了。bucket 就是那个活得更久的东西:一
张坐在页面之上的表,每个页面在离开时把自己的滚动偏移写进去。

**一条记录的身份是什么。** 不是 widget(它已经没了),也不是它的位置(那会动)。上
游从读取处的 context 往上走,把沿途每一个 `PageStorageKey` 收起来、到 `PageStorage`
为止,这串 key 就是身份——所以两个标签页里的两个列表是**两条**记录(尽管两个都是「那
个列表」),而同一个列表跨过一次重建仍是**一条**(尽管 widget 是新的)。

**记账的边界:** 上游是**往上走**收集 key 的,这个 crate 的 `BuildContext` 走不了祖
先。于是这里把 key 链**往下递**:`PageStorage::scope` 把「自己这一格接上去之后的
链」provide 下去,读取方问最近的那一条。同一个身份,只是从另一头到达的——代价是这里
必须显式写一个 scope,而上游从任意 widget 上的一个 key 就能推出来。另外上游的值是
`dynamic`,这里是 `f64`:所有真正在用 bucket 的东西存的都是一个滚动偏移;做成
`Box<dyn Any>` 是通用版,而没有谁会用到那份通用。

**回归行盯的地方:**

* 一个页面写进去的,同一个页面(在一棵**全新的树**里,但用同一个 bucket)读得回来
  ——这正是全部要点:widget 走了,bucket 没走。
* **两个 key 不同的页面是两条记录。** 这是这个文件要防的那个失败:两个标签页里的
  列表都是「那个列表」,它们绝不能共用一个滚动偏移。
* **上面没有 scope 的读取方没有身份,于是什么都不存。** 上游那道 `isNotEmpty` 判
  据。没有它,每一个没带 key 的读取方共用一条记录,同一页上的两个列表会去抢同一个
  偏移量。
* **嵌套 scope 得到更长的链**,而身份是**整条路径**而不是最内层那一格:同一个列表放
  在两个不同的标签页里是两条记录,因为标签页的 key 在它上面。这条用「两个标签页里
  用同一个内层 key」钉住。
* **同一身份写两次是替换而不是追加。** 页面每滚一下就写一次,会追加的 bucket 会无限
  长大,而且永远读回第一个值。
* **两个 bucket 只有在是同一张表时才相等。** bucket 是身份不是值:内容相同的两个仍
  然是两个可以存进去的地方,把它们当作相等会让一个无关页面的存储替这一个回答。

验证:`cargo test --lib` 1261 绿,GN `rustflutter_unittests` 1261 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1175 accounted / 698 MISSING。


### 两个只说自己尺寸的 widget(2026-08-19)

`preferred_size.rs`(新),四个上游类:`PreferredSizeWidget`、`PreferredSize`
(`widgets/preferred_size.dart`),`SizeChangedLayoutNotification`、
`SizeChangedLayoutNotifier`(`widgets/size_changed_layout_notifier.dart`)。两个都
不画任何东西:一个在被量之前回答自己想要多大,另一个在被量之后说尺寸变了。

**为什么会有「被量之前回答」这种事。** scaffold 要先给 app bar 定尺寸,而那时 bar
还没布局——那一刻谁也量不了它,所以只能问它。这也是答案是一个 `Size` 而不是两个数
的原因:任一维可以是无穷,读作「这根轴上没有意见」,app bar 就是有高度诉求、对宽度
没意见。

**记账的边界:** 上游的 `PreferredSizeWidget` 是一个 widget 类实现的接口,调用方拿
到的是「一个恰好也是它的 Widget」。这个 crate 的 widget 全部擦除成 `AnyWidget`,没
有位置再挂第二个接口,所以 `PreferredSize` 是一对东西——尺寸和 widget——而
`PreferredSizeWidget` 是「能给出这么一对」的那个 trait。

**回归行盯的地方:**

* **首选尺寸不约束孩子。** 上游明说它不强制:孩子拿到的是实际传下来的约束,首选尺
  寸只是父级在决定留多少地方时被告知的那个数。孩子随后要得更多就会溢出,和没有这个
  widget 时一模一样。这条用「50×50 的 PreferredSize 里放一个 200×30 的孩子,量出来
  是 200×30」钉住。
* **第一次布局不算变化。** 上游自己的注释说了后果:那样就是「SizeObserver 重演」
  ——树里每一个 notifier 都在第一帧触发,而那时什么都还没变。
* **同一尺寸的第二次布局也不算变化。** 什么都没改的重布局不该叫醒任何人;这个通知
  是给「跟着某个尺寸重绘」用的,白重绘一次正是这道判据省下的成本。
* 尺寸真的变了才回调一次,停在那儿不再回调,**变回去又算一次**。

`SizeChangedLayoutNotification` 对应上游的 `LayoutChangedNotification` 家族,而那
一家的要点写在文档里:它在**布局过程中**到达,所以监听方自己不能再触发布局——响应它
去改尺寸会死循环。它是给重绘用的。

验证:`cargo test --lib` 1254 绿,GN `rustflutter_unittests` 1254 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1172 accounted / 701 MISSING。


### 识别器递给回调的那九样东西(2026-08-19)

`gesture_details.rs`(新),九个数据类加一个接口:`TapDrag{Down,Up,Start,Update,End}Details`
(`tap_and_drag.dart`)、`SerialTap{Down,Cancel,Up}Details`(`multitap.dart`)、
`ForcePressDetails`(`force_press.dart`),以及它们共同实现的
`PositionedGestureDetails`。gestures 层从 32 降到 19。

**为什么点击和拖拽合成一族。** 文本框两样都要,而且要从**同一个**识别器拿。两个分
开的识别器会在竞技场里打架、点击输了——所以「双击选中一个词、再拖着扩展选区」这件
事,是拼不出来的:它不能由一个点击识别器加一个拖拽识别器组成。上游的答案是
`TapAndDrag`,而让它能用的那个字段是 `consecutive_tap_count`:每一个回调都带着「这
是连续第几次点击」,处理方于是分得清「一次点击之后的拖拽」和「两次点击之后的拖
拽」。

**回归行盯的地方:**

* **局部位置的初值是全局位置**,不是原点。这是上游的规则不是偷懒:在任何变换发生之
  前,这两个就是同一个点;默认成原点会把每个未变换的手势都放到左上角。
* 一次 update **同时**带着「这一步」和「总共」。这是两个不同的问题,处理方两个都要:
  delta 说这一帧滚多少,offset_from_origin 说选区现在够到哪。由其中一个推另一个,意
  味着处理方自己要维护一个累计值——而这正是上游替它省掉的事。
* **pan 没有 primary_delta,单轴拖拽有。** primary 是「单轴识别器盯着的那一根轴上的
  位移」;pan 没有那样一根轴,所以上游留空而不是随便挑一根。
* **没有位置就结束的拖拽报告原点**,而这是有含义的:一个把指针丢了的识别器(被取
  消、或者输给了竞技场)没有位置可报,原点就是它的回答。
* start 可以说出**平台认为它发生的时刻**,这样从排队事件里起手的拖拽,按事件计时而
  不是按读到它的那一帧。
* **连续点击从 1 数起。** 上游用 assert 挡住:没有第 0 次点击,从 0 开始数会让第一
  次点击看起来像是在取消一个不存在的东西。
* **九个里只有 cancel 没有位置。** 上游是故意把它排除在那个接口之外的:cancel 说的
  是「有一次点击要被收回」,它本来会在哪儿并不是谁会去用的信息。这一条是靠「把其余
  八个都装进 `dyn PositionedGestureDetails`」来断言的。
* **up 一定知道是什么碰了屏幕,down 未必。** 上游在 `TapDragUpDetails` 上要求
  `kind`、在另外四个上留可选:一次点击都完成了,平台当然已经说过那是什么。

验证:`cargo test --lib` 1248 绿,GN `rustflutter_unittests` 1248 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1168 accounted / 705 MISSING;gestures 层 71/90。


### 把手指的轨迹对到帧上(2026-08-19)

`resampler.rs`(新),四个上游类:`PointerEventResampler`(`gestures/resampler.dart`)、
`SamplingClock`(`gestures/binding.dart`)、`PointerSignalResolver`、`Drag`。

触摸屏按自己的时钟采样,那不是显示器的时钟。放任不管,位置就会成簇地到:一帧里两
个、下一帧一个都没有,于是一根匀速移动的手指看起来在抖。重采样器把时间固定在帧
上,问「那一刻手指在哪」,在前后两个真实采样之间插值。

**两处我先写错了测试预期、实现是对的地方——两次都因为上游的行为比想当然的更细:**

1. **一帧里被投递出去的那个事件,带的是插值后的位置,而不是它自己的。** 按下发生在
   0、帧在 5ms,框架收到的是「手指在帧那一刻的位置」,时间戳也换成帧的。而且**没有**
   另外一条 move 采样——按下已经把位置带过去了,再报一次就是一次零位移的移动。我原
   本以为这两件事是分开的。
2. **只有抬起和移除会把窗口往后延,按下不会。** 这看着像疏漏,其实不是:重采样是在
   **过去**取样的,所以 12ms 的按下在标记为 10ms 的那一帧里还没发生,投递它等于报告
   一次尚未发生的触摸。抬起正相反——指针已经走了,没有任何东西可以再插值过去,压着不
   发会让手势多挂一帧。

**其余回归行盯的地方:**

* 帧落在两个采样之间时取中间的点(这就是全部目的)。
* 帧落在最后一个采样之后时取最后那个采样,**不外推**——猜手指停止上报之后去了哪,
  正是甩动会冲过头的原因。
* **静止的手指一条采样都不产生。** 位置没变就没有东西可报;照报不误会让每一帧都叫
  醒所有拖拽识别器,而手指根本没动。
* delta 是对着**上一次交出去的位置**量的,不是上一个真实事件——拖拽识别器把它们累
  加,对着别的东西量会漂。
* 两个时间戳相同的采样不会除以零。这是 `_positionAt` 里的第二道判据;触摸屏在同一
  微秒里报两个位置不是假想,没有这道判据,间隔为零、位置成 NaN,然后这个 NaN 会穿过
  每一个手势识别器。
* move 和 hover **被吞掉**,因为它们正是插值要替代的东西;既放过去又插值,等于把抖
  动和平滑一起投递。
* `stop` 把还排着的全部原样发出(带各自的时间戳而不是某一帧的)并忘掉这个指针。为一
  个永远不会到来的帧压着事件,正是手势卡住的成因。
* 信号解析:**第一个登记的拿走滚动,其余被忽略**。命中路径是由内向外的,所以「先到
  先得」就是「最内层赢」——页面里的列表吃掉滚轮,页面自己不跟着滚。没人要的信号会**明
  说没人要**,那是给平台去做它自己默认动作的许可。

验证:`cargo test --lib` 1239 绿,GN `rustflutter_unittests` 1239 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1159 accounted / 714 MISSING。


### 许可证、计时、和那些看不见的方向控制符(2026-08-19)

`licenses.rs`(新),九个上游类,来自 foundation 的三个文件:`licenses.dart`(4)、
`timeline.dart`(4)、`unicode.dart`(1)。foundation 剩下的 33 个几乎全是诊断树
(`diagnostics.dart` 22 + `assertions.dart` 10 + `stack_frame.dart` 1),那是 P10,
计划里明写着押后。

**`LicenseEntryWithLineBreaks` 的段落解析是这一簇的全部难点。** 许可证文件是硬换行
的散文:段内的换行不是分段,空行才是;而缩进只能从行首的空格里读回来,因为没有别的
地方记着它。上游自己的注释把那套缩进规则叫做「一个狂野的启发式」,并说明它是照着常
见的 BSD 和 LGPL 文本凑出来的——这里照原样移植,因为一份排版和上游不一样的许可证就
是一份显示错了的许可证。

**一处我先写错了测试预期、实现是对的地方:** CRLF 只在**段落之间**被合并成一个断
点。段内的状态只盯 `\n` 和 `\f`,所以一个**结束换行行**(而不是结束段落)的 CRLF,
它的 `\r` 属于那一行、会留在拼接出来的段落文本里。上游就是这样,于是这条被写成了断
言而不是被顺手"修掉"——顺手修掉的移植会和它所移植的框架排出不一样的版面。

**其余回归行盯的地方:**

* 硬换行的行以**单个空格**拼成一段——这正是许可证能按显示宽度重排的原因。
* 连续空行是**一个**断点,不是若干个空段落。
* **一级缩进是三列**;超过十列的行被当作**居中的标题**(所以答案是一个标记而不是更
  大的数字——它根本不是缩进)。
* **制表符算八列。** 算一列的话,用制表符缩进的段落会落在缩进 0,和完全没缩进一样。
* **比上一行缩进更多的行开启新段落**,否则许可证里那些缩进的子条款会和它上面的句子
  连成一句。
* **左方括号算作缩进。** 这是上游点名给 LGPL 2.1 的 hack:它开头那段用方括号括起
  来、续行比首行多缩进一格;不把方括号算进去,那两行会变成两段。
* 换页符在**任何位置**都结束段落(许可证有时是分页的,页断落在行中间也一样)。
* 输入结束时的两种情形都要收尾——段内还有没取走的最后一行,段间还有攒着的行;丢掉任
  何一种都会丢掉许可证的结尾。
* 计时**按名字累加、并保持首次出现的顺序**。用会重排的映射,同一帧跑两次会打印出不
  同的表,而这正是一张计时表唯一不能做的事。
* 从没被计时过的名字返回一个**零块**,这样打印表格的调用方不必给「没跑过的那一行」
  开特例。
* 嵌套的块**先关最内层**:build 里面的 layout 先于 build 结束,配反了会让每个内层块
  都拿到外层的时长。
* 那十二个方向控制符是**看不见的**——写错一个既看不出来,又会改变它之后所有文字的排
  版方向。

验证:`cargo test --lib` 1225 绿,GN `rustflutter_unittests` 1225 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1155 accounted / 718 MISSING。


### services 层收尾:141/141(2026-08-19)

十一个上游类,`services` 层归零。这是继 painting 和 animation 之后**第三个完全覆
盖的层**。

`text_input.dart` 余下的八个,加上 `binding.dart` 的 `SystemContextMenuClient` 和
`mouse_cursor.dart` 的两个:

* `RawFloatingCursorPoint`(与 `FloatingCursorDragState`)、`SelectionRect`、
  `TextInputStyle`、`TextSelectionDelegate`(与 `SelectionChangedCause`)、
  `DeltaTextInputClient`、`ScribbleClient`、`TextInputControl`、
  `SystemContextMenuController` + `SystemContextMenuClient`——都进
  `services/text_input.rs`。
* `MouseCursorSession`、`MouseCursorManager`——进 `services/system.rs`,
  `SystemMouseCursor` 已经在那里。

**回归行盯的地方:**

* 浮动光标的 `Update` **必须**说自己移到了哪里(上游用 assert 挡)。三个状态在线上
  是一个枚举,所以这个要求做不成「不可表达」,只能在造点的时候检查。而 `End` 不需
  要偏移量:真光标会吸附到浮动光标最后停的地方。
* 工具条默认提供除 Live Text 以外的一切。这个不对称正是重点:Live Text 需要相机,
  所以字段是**选进**而不是选出。
* **一个什么都不覆盖的 `TextInputControl` 也能编译、而且什么都不做。** 上游给每个
  方法一个空实现是有意的:只关心键盘何时该弹出的控件,覆盖 `show` 一个,其余五个不
  管。
* **在菜单已经在的位置再 show 一次,什么都不发。** 字段每次选区变化都会问,而多数
  变化并不移动菜单;没有这个提前返回,每一次光标闪烁都是一个来回。
* **系统把菜单收走时不回发一个 hide。** 菜单已经没了,再说一遍正是会让它循环的那
  一步;控制器只是不再认为它在,并通知问过的人。
* 鼠标光标:**候选列表里第一个不「让位」的赢**。`None` 是上游的
  `MouseCursor.defer`——一个区域说「听我背后的」。直接取第一项的移植,会让每个位于
  让位区域之下的区域都变成箭头。一路让到底仍然不是答案,那时用 fallback。
* **平台只被告知变化。** 这是这个 manager 存在的全部理由;没有那个提前返回,鼠标
  在一个区域上每移动一次都是一条消息。
* **每个设备各留各的光标。** 同一块屏上的手写笔和鼠标是两个设备,笔悬停在链接上不
  该改变鼠标正在显示的东西。设备移除时会话是**丢掉**而不是替换,所以重新插上的设备
  从头开始,不会继承拔掉之前的光标。

验证:`cargo test --lib` 1208 绿,GN `rustflutter_unittests` 1208 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1146 accounted / 727 MISSING;**services 层
141/141,0 MISSING**。


### 状态栏、平台的撤销、鼠标标注、以及一份手写的空白字符表(2026-08-19)

六个上游类,分别落在已有的模块里而不是新开四个文件——它们各自都只有一两个类,而且
各自属于某个已经在那里的东西:

* `services/system.rs`(已有 `SystemChrome`):`ApplicationSwitcherDescription`、
  `SystemUiOverlayStyle`,加上 `services/undo_manager.dart` 的 `UndoManager`/
  `UndoManagerClient`(与 `UndoDirection`),以及
  `services/mouse_tracking.dart` 的 `MouseTrackerAnnotation`。
* `services/text_boundary.rs`(已有 `LineBoundary`):`TextLayoutMetrics`——
  `LineBoundary` 本来就是问它要行范围的,这是它该在的地方。

**回归行盯的地方:**

* **`SystemUiOverlayStyle.light` 说的是图标是浅色的,不是应用是浅色的。** 把名字
  当成应用自己的明暗,挑到的正是让状态栏看不清的那一个。而 `statusBarBrightness`
  是里面的异类——iOS 的,描述的是状态栏**背后**的东西,所以在两个常量里它都和旁边的
  图标明暗相反。
* **没设的字段发出去是 null,意思是「别动它」。** 那些栏归平台管;发一个默认值过
  去,等于接管了一条应用从没问过的栏。
* 明暗值在线上是 `Brightness.light` 这样的**字符串**——Dart 的 `toString` 就是这么
  写的,各家 embedder 也正是这么匹配的。发 `light` 或 `0`,对面认不出来。
* 颜色在线上是**有符号整数**。JSON 没有无符号整数,对面读的是 Dart int,所以不透明
  的黑在线上是负数。按无符号发,对面解析会溢出。
* 平台给了一个本框架没听说过的撤销方向,就**丢掉**。上游那里抛 `FlutterError`;平
  台说了句新话,不是崩掉应用的理由。
* **没有 cursor 的鼠标标注是「让给底下的」而不是箭头。** 上游默认的
  `MouseCursor.defer` 不是一种光标,是拒绝指定一种;这里用「不存在」表示。默认成
  `Basic` 会让每个区域都变箭头,把底下输入框的文本光标盖掉。
* **上游那份空白字符表不是 Rust 的 `char::is_whitespace`。** 上游自己注释说它是在
  替还没暴露的 ICU 信息占位、是手写的,两者确实不一样:上游算上 `0x1C`-`0x1F` 四个
  ASCII 分隔符(Rust 不算),Rust 算上 `0x0085` 和 `0x2028`-`0x2029`(上游不算——它
  把那三个当行终止符另行处理)。照搬 Rust 的会改变 ctrl-左停在哪里,所以两边的差
  异都写成了断言。另外零宽空格**不是**空白:它是一个可断行的位置而不是一个空隙,
  当成空白会让按词选择停在词中间。

验证:`cargo test --lib` 1194 绿,GN `rustflutter_unittests` 1194 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1135 accounted / 738 MISSING;**services 层 130/141,
只剩 11**,其中 8 个是 `text_input.dart` 的余项。


### 输入框说不的地方(2026-08-19)

`services/text_formatter.rs`(新),上游 `services/text_formatter.dart` 三个类全覆
盖:`TextInputFormatter`、`FilteringTextInputFormatter`、
`LengthLimitingTextInputFormatter`,加上 `MaxLengthEnforcement`。

格式化器坐在键盘和输入框之间,每一次编辑落地之前都能重写它:把数字框里的字母丢
掉、到一百个字符停下、让一行保持一行。这是输入框唯一能对**平台已经做过的事**说不
的地方。

**整套算术跑在 UTF-16 码元上。** `TextEditingValue` 的偏移量就是码元,上游的 Dart
字符串下标也是。这里把文本一次解成 `Vec<u16>`、最后一次编回去,而不是在每个下标上
做转换——这样偏移量和文本不可能对不上,而对不上正是这套算术最容易犯的错。

**记账的边界:** 上游的过滤器收 Dart `Pattern`(`String` 或 `RegExp`)。这个 crate
没有正则引擎,所以 `TextPattern` 是能被求值的那个封闭集合:一个字面量,或一个对单
字符的判定。上游自己用的两个——`digitsOnly` 是 `RegExp(r'[0-9]')`、
`singleLineFormatter` 是字面 `'\n'`——都在里面。另外上游默认的 enforcement 取决于
`TargetPlatform`(这个 crate 没有),这里定为 `Enforced`,那正是上游给 Android 和
Windows 的答案,也正是这个 crate 验收的两个平台。

**回归行盯的地方:**

* **光标跟着它底下的文本走。** 这是那个累加器存在的全部理由:一个重写了文本却把
  光标留在原处的过滤器,每敲一下都会把光标放到别处。
* 而**位于被删除区段之前的光标不动**。这是 `adjustIndex` 的另一半;动了它,就会在
  行末每被拒绝一个字符时把光标往回拖一格。
* `allow` 决定的是「匹配段」和「间隙」哪一个是禁区,除此之外什么都不变——同一个
  pattern,相反的答案。
* 替换串顶掉的是**一整段**,不是一个字符。字面量 pattern 一次匹配整段,所以
  `a123b` 出来是 `a#b`;字符类 pattern 每个数字各是一次匹配,出来是 `a###b`。
* **折叠的组词区被丢掉而不是带着走。** 上游只在组词区有效**且非折叠**时保留它;折
  叠意味着没人在组词,带着它走完这套算术,最后会得到一个盖在没人组词的文本上的组
  词区。
* **往已满的框里打字是被拒绝而不是被截断。** 差别对读者是实打实的:截断新值会让光
  标在每一次被拒的按键上跳到框尾;保留旧值则一切原封不动。但**选中了内容时改为截
  断**——读者要求的是替换,拒绝会让选区原样留着、看起来像什么都没发生。
* 组词中的输入被放行,直到组词结束。半个日文词不是词;中途截断丢的是整个词而不是
  最后一个字符——这条 enforcement 存在的理由就是它。
* **上限数的是字符不是字节也不是码元。** 三个 emoji 是 12 字节、6 码元;上限为 3
  必须意味着三个 emoji。数另外两个里的任何一个,框会拒绝它的第一个字符。
* `None` 和 `-1` 都是上游的「无上限」,必须表现一致——一个接受 -1 又照着执行的
  API,会把每个框截成空的。

验证:`cargo test --lib` 1183 绿,GN `rustflutter_unittests` 1183 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1129 accounted / 744 MISSING;services 层只剩 17。


### 引擎在听的那些通道,和三个只是一次调用的服务(2026-08-19)

`services/system_channels.rs`(新),四个上游类:`SystemChannels`、`Scribe`、
`SensitiveContentService`、`DeferredComponent`。后三个各自只是在某条通道上打一个
方法调用,所以和通道名表放在一起。

**24 个通道名从上游解析生成**(`tools/gen_system_channels.py`)。理由和上一簇的自
动填充提示一样:名字打错不会报错,那条通道**根本没人在听**,调用永远不到达。这类
「靠沉默失败」的东西不手抄。

**一处结构上的分岔:** 上游的 `SystemChannels` 存的是造好的通道对象,每个自带
codec;这里存的是**名字**。在这个 crate 里造一条通道很便宜,而 codec 是在发起调用
的地方选的——一张预造通道表,拿到手也得先知道每条各带什么 codec 才能用。真正必须
分毫不差的是名字,所以生成的就是名字。

**记账的边界:** `SensitiveContentService::is_supported` 上游在非 Android 上不问任
何人直接答 false。这个 crate 里没有 `TargetPlatform` 可问,所以每次都去问平台——两
种情形下平台的回答都是权威的那个,上游只是把它短路掉,并没有替换它。没有该方法处
理器的平台什么也不答,在这里就是 false。

**回归行盯的地方:**

* 每条通道名都在引擎的前缀下,且**两两不同**——两个名字相同意味着其中一个抄错了。
* **先前写的三个服务用的名字要和这张表对得上。** spell_check 和 process_text 是在
  表存在之前写的;哪天两者不一致,就说明有一个错了,而这是它会露出来的地方。
* `ContentSensitivity` 的**变体顺序是协议的一部分**,不是口味问题:平台收到的是一
  个整数、按它自己那个枚举的下标去读。在这里重排一下,会悄没声地把一屏密码标成
  「可以录屏」。
* 本框架没听说过的模式返回**空**而不是猜一个最近的。上游那里是 `_unknown` 加一个
  `UnsupportedError`;猜是唯一的错误答案——它可能把敏感屏标成不敏感。
* 延迟组件的 `loadingUnitId` **永远是 -1**。上游自己的注释说了原因:Dart 侧看不到
  loading unit id,所以改用组件名;字段留在消息里,是为了将来 API 能带上一个而不必
  改协议。

验证:`cargo test --lib` 1168 绿,GN `rustflutter_unittests` 1168 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1126 accounted / 747 MISSING;services 层 121/141,
只剩 20。


### 资源包、字体、以及构建盖的那几个章(2026-08-19)

`services/asset_bundle.rs`(新),五个上游类:`CachingAssetBundle`、
`PlatformAssetBundle`、`FontLoader`、`FlutterVersion`,外加把
`NetworkAssetBundle` 记进台账的裁剪。三个上游文件合成一个,因为后两个各自只有一个
类。

**同步与异步的那道缝,在这里是明写出来的。** 上游每一次加载都是 `Future`;这个
crate 的 `AssetBundle::load` 是同步的,因为 `AssetImage` 是在 build 里读它、没法
等。所以 `PlatformAssetBundle` 把上游用 Future 焊在一起的两半拆开:`prefetch` 去问
平台并把答案填进缓存,`load` 从缓存里答。**没被 prefetch 过的 key 是 miss,不是
等待**——这条写成了回归行,免得它读起来像个 bug。

**`NetworkAssetBundle` 记为超出范围。** 它就是上游的 `dart:io HttpClient` 套在
`AssetBundle` 接口后面;这个 crate 没有 HTTP 客户端,而写一个不属于「移植框架」这
件事。同一个文件里另外两个包都覆盖了,所以这不是绕过困难,是这一个类恰好在范围外。

**回归行盯的地方:**

* 缓存包对底下那个只问一次。
* **miss 也进缓存。** 上游缓存的是 future,而一个以错误完成的 future 仍然是完成
  了的——所以找不到的 key 也不会被再问一遍。只缓存命中的移植,会让每个缺失的资源在
  每一帧都去查一次。
* `evict` 之后下一次加载会重新问。上游给的理由是热重载:磁盘上改过的资源还在缓存
  里,而除此之外没有任何东西会再去问它。
* **资源路径发出去之前要 percent 编码。** 带空格的路径是真的会发生、而且会静默失
  败的那种:引擎会去找一个名字里真带空格的文件,找不到。路径本身的分隔符保持原样
  ——把斜杠也编码掉,就成了在要一个名字里带斜杠的文件。
* 字体加载器只能加载一次,加载之后再 `add_font` 是错误。两者上游都抛 `StateError`;
  悄悄忽略任何一个,得到的是一个永远不出现的字体和无从解释的原因。
* 单元测试跑的引擎桩**拒绝一切字体**,所以这两条测的是:拒绝被**报上来**而不是被
  吞掉;以及无论如何 loader 都算已加载——这是上游的顺序,`_loaded = true` 在交出第
  一个字面之前。
* `FlutterVersion` 的常量在构建没定义时是**不存在**而不是空串。上游用
  `bool.hasEnvironment` 问,这里是 `option_env!`,同一个问题在编译期问一遍。

验证:`cargo test --lib` 1162 绿,GN `rustflutter_unittests` 1162 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1122 accounted / 751 MISSING;services 层 117/141,
只剩 24。


### 告诉系统这个输入框是干什么用的(2026-08-19)

`services/autofill.rs`(新),上游 `services/autofill.dart` 五个类全覆盖:
`AutofillHints`、`AutofillConfiguration`、`AutofillClient`、`AutofillScope`、
`AutofillScopeMixin`。

一个记住了地址的操作系统,只能把它递给**说了自己想要地址**的那个输入框。自动填充
的全部就是这句话:一个框说出自己装的是什么,给自己一个跨重启稳定的标识符,剩下的
交给平台。

**67 个提示常量是从上游解析生成的,不是抄的。** 每一个都是平台拿去匹配的字符串,
拼错一个字母不会报错——那个框只是**再也不会被填**,offer 根本不出现。这类东西手抄
的期望错误率不是零,所以走生成器(和 `colors.rs`、typography 同一个理由)。

**顺带把 `TextInputConfiguration` 补上了 `autofill_configuration` 字段**——自动填
充正是经由它抵达平台的。这让该结构体从 `Copy` 降为 `Clone`(`AutofillConfiguration`
带 `String` 和 `Vec`),`editable.rs` 里那个 `Fn` 闭包因此要显式 clone 一次。

**结构上的分岔:** 上游 `AutofillScopeMixin.attach` 把触发字段的配置包进一个私有的
`_AutofillScopeTextInputConfiguration`(它给 JSON 加一个 `fields` 列表)。那个私有
类不是上游的公开类,所以这里是 `AutofillScope::configuration_with_fields`,同样的
JSON、另一条路。Dart 里「mixin 覆盖 interface 的一部分」在 Rust 里就是 blanket
impl,`AutofillScopeMixin` 因此对每个 `AutofillScope` 自动成立——和上游每个 mix 进
去的类拿到它,是同一件事。

**回归行盯的地方:**

* 禁用的配置在消息里是**整个不存在**,不是 `enabled: false`。上游 `toJson` 返回
  null,字段随之从配置里消失。发一个「关掉了」的字段,等于在跟平台介绍一个不想被
  介绍的框。而这正是默认值——没说过话的框什么也不说。
* 没有 hintText 时那个键**不存在**,不是 null。上游 `'hintText': ?hintText` 丢掉整
  个键;发 null 和不发是两条不同的消息,平台读法不同。
* **scope 会把每一个字段都发出去,不只是被点的那个。** 这才是 scope 之所以是
  scope:只发被点的那个,结果是那个框被填上、表单其余部分空着——而这正是整个类存在
  要防的事。`fields` 是**加上去**的,触发字段自己的配置照旧在。
* 表单里有一个字段关掉了自动填充,整个 scope 就不对了。上游在 `attach` 里用
  assert 挡;它挡的事情是真的:平台保存的账号会填上其余的、跳过那一个,看起来像是
  表单的 bug 而不是那个字段的。
* 常量表里没有重复值——两个常量值相同,意味着其中一个抄错了。

验证:`cargo test --lib` 1154 绿,GN `rustflutter_unittests` 1154 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1117 accounted / 756 MISSING;services 层只剩 29。


### 选中文字能拿去做什么,和资源清单(2026-08-19)

两个新文件,五个上游类。

**`services/process_text.rs`**——`ProcessTextAction`、`ProcessTextService`、
`DefaultProcessTextService`。Android 允许任何应用注册一件「能对选中文字做的事」
——翻译、查词、加进笔记——并把它们摆进选择工具条。框架不知道那些是什么,所以它问:
有哪些动作,然后,拿这个动作跑这段字符串。

**`services/asset_manifest.rs`**——`AssetManifest`、`AssetMetadata`。一个把同一张
图按三种密度打包的应用会打进三个文件,构建过程把哪个是哪个记下来。清单就是那份记
录,也正是 `AssetImage` 能在 2x 屏上挑中 2x 文件、而调用方不必点名的原因。

**两处结构上的分岔:**

* 上游的 `AssetManifest` 是抽象类加一个私有实现 `_AssetManifestBin`;这里它**就是**
  那个实现,和 `TapRegionRegistry` 同一个理由——只有一个实现、也看不到第二个的接口
  买不到任何东西。
* 上游的 `_AssetManifestBin` **按 key 惰性解码**,免得大清单在第一次加载资源时卡一
  下。这里一次把整张表解开:解码在字节变成 `Value` 的那一刻就已经发生了,上游想要
  惰性的那一步是**类型转换**,而 Rust 在进来的路上就做完了。

上游的 `setChannel`(只为测试换通道、且用 assert 藏起来不进发布版)在这里是构造时
就收一个通道,做的是同一件事,不需要那层遮掩。

**回归行盯的地方:**

* 动作的 id 和 label 是**两个**字段不是一个:label 是翻译过的,id 不是。把 label
  发回去,读者一换语言就坏。
* 平台发来一条畸形的键值对,不该把整条工具条清空——坏的那条丢掉,其余照常。
* 回复不是 map(包括 null)时给空列表而不是拒绝:对于横竖都要画出来的工具条,「平
  台没什么可给」和「平台没答上来」是同一件事。
* 清单里 **key 自己那个变体就是 main**。这就是上游 `main` 规则的全部,也正是 1x 文
  件和它旁边 2x 文件的区别所在:key 是调用方写的那个路径,恰好一个变体与之相同。
* **清单没提到的 key 和「提到了但变体为空」不是一回事**:前者是没打包,后者是打包
  了但没什么可挑。
* 没有 `dpr` 的资源照样是资源(字体就没有)。
* 资源列表列的是 **key 不是文件**:三个文件,一个资源。
* 清单要真的能从标准编解码器的字节里解出来——磁盘上那份就是构建用这个编解码器写
  的,除此之外没有别的读法。

验证:`cargo test --lib` 1147 绿,GN `rustflutter_unittests` 1147 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1112 accounted / 761 MISSING;services 层 107/141,
只剩 34。


### 拼写检查,与平台塞进输入框的两样东西(2026-08-19)

两个新文件,六个上游类:

**`services/spell_check.rs`**——`SuggestionSpan`、`SpellCheckResults`、
`SpellCheckService`、`DefaultSpellCheckService`。框架不会拼写;每个平台都自带词
典和检查器,这是通往它的那条通道:递一个 locale 和一个字符串,拿回看着不对的那些
范围和替换建议。上游的 `Future` 在这里是回调,和这个 crate 里其他所有
`MethodChannel` 调用同形。

**`services/keyboard_inserted_content.rs`**——`KeyboardInsertedContent` 与
`PredictiveBackEvent`(含 `SwipeEdge`)。两样各自太小、放不满一个文件的东西:
Android 键盘往输入框里塞的 GIF 或贴纸,和一次已经开始、还没松手的返回手势。

**照抄了上游一处名实不符的地方,并且写下来。** `DefaultSpellCheckService` 合并新
旧结果的条件,上游写作 `spansHaveChanged = listEquals(...)`——名字说「变了」,值算
的是「相等」,而合并跑在**相等**那条分支上。两个相等的列表合并出来还是原来那个,
所以照这么写,这次合并根本不可能改变任何东西。这里照原样移植,并在
`reconcile` 的文档里说明原委:一个悄悄改掉它的移植,会和它所移植的框架给出不同的
答案。回归行把两条分支都钉住了,哪天上游修了它,这里会失败而不是无声漂移。

**回归行盯的其余地方:**

* 合并按 span 起点顺序走两个列表;两边起点相同时**保留旧的那条建议**。这是上游的
  选择而不是巧合:读者可能已经看到过旧的那一组,菜单开着的时候把列表换掉,会改变
  点下去的后果。
* 一端走完之后,另一端剩下的全都跟上。
* 结果只对**它被问的那个字符串**有效——一次按键之后那些偏移量指的就是别的东西了,
  这也是为什么 `SpellCheckResults` 要把文本一起带着。
* 有偏移量没有建议的 span 是合法的:平台不喜欢这个词,但没有更好的可给。
* `hasData` 上「不存在」和「空」是同一个答案——零长度的附件怎么说都没东西可插。
* 返回**按钮**会当作一次没动过的手势传来,而且有两种传法:完全没有触点是一种;
  触点在原点、进度为零也是一种——Android 文档说按钮按下时坐标是 NaN,实机上传的
  是零。文档和设备不一致,所以检查按设备来。
* 平台不可能发出的 `swipeEdge` 被拒绝而不是 panic(上游是拿它去索引枚举值)。

验证:`cargo test --lib` 1136 绿,GN `rustflutter_unittests` 1136 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1107 accounted / 766 MISSING;services 层 102/141,
只剩 39。


### 键盘改了什么,而不是文本现在是什么(2026-08-19)

`services/text_editing_delta.rs`(新),上游 `services/text_editing_delta.dart`
五个类全覆盖:`TextEditingDelta` 与它的四个变体
(`Insertion`/`Deletion`/`Replacement`/`NonTextUpdate`)。

普通的文本输入协议每次按键都把整个新值发过来;想知道**到底发生了什么**的那些东西
——拒绝某个字符的格式化器、撤销栈、拼写检查——只能拿两个字符串去 diff 再猜。delta
直接说出来:这里插入了什么、哪一段被删了、哪一段被替换了,或者文本根本没变、只是
选区动了。四个是封闭集合,所以是枚举加每个变体一个 struct。

**偏移量是 UTF-16 码元。** 和上一簇的 `text_boundary` 正好相反——那个数字节,因为
它作用在 crate 自己的字符串上;delta 是**平台说的话**,就留在平台的单位里。转换发
生在 `apply` 里,那正是一条 delta 从消息变成字符串的地方。

**分类才是这个文件的全部难度。** 平台不说这是哪一种,它只说「这段目标范围变成了这
段源文本」。上游那一整套判断是在读这句话的意思,而它最要紧的场合是输入法组词时:
原生 IME 每敲一下都把整个组词区替换掉,而不是逐字符汇报。敲 `world` 的 `d` 传过来
是「(0,4) 变成了 world」,判断它是插入的唯一办法,是发现**旧区域内的文字没变**——
`worl` 还是 `worl`。删除同理,`world` 变 `worl`、区域文字没变,那是删了一个字符。
区域文字**变了**(`worl` 变成 `hell`),两种读法都不成立,那就是替换。

**一处我自己先写错了测试预期、被实现纠正的地方:** 「删掉不止一个字符」并不都是删
除。上游只有**单个**字符的缩短读作删除;`abcde` 在整段上变成 `ab` 掉了三个字符,上
游把它算作**替换**——没有哪一个字符可以让删除范围指过去。而用空串替换(整段选中后
删掉)确实是删除,而且是**未经收窄的**整段范围,因为里面每个字符都真的没了。两条
都写成了回归行,连同那条分界线本身。

**其余回归行盯的地方:** 单字符退格上游会把汇报范围收窄到真正消失的那一个字符(照
搬汇报范围会从词首开始删);两端都是 -1 是平台在说「文本没变」;**范围一样但文本
相同也算 NonTextUpdate**——上游比的是文本不是范围;偏移量是码元不是字节(基本平面
外的字符是两个码元四个字节,按字节读会把光标插进字符中间);落在字符中间的偏移量
被拒绝而不是 panic——只有畸形的平台消息会走到这里,丢一次按键胜过打字打到一半崩
溃。

验证:`cargo test --lib` 1122 绿,GN `rustflutter_unittests` 1122 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1101 accounted / 772 MISSING。


### 一段文字在哪里断开(2026-08-19)

`services/text_boundary.rs`(新),上游 `services/text_boundary.dart` 五个类全覆
盖:`TextBoundary`、`CharacterBoundary`、`LineBoundary`、`ParagraphBoundary`、
`DocumentBoundary`,外加它们要用的 `TextRange`(上游在 `dart:ui`,这里是第一个需
要它的地方)。

Ctrl-左、Shift-下、双击、读屏器一段一段地读——都是同一个问题换了个单位在问,这个
trait 就是那个问法。一个边界回答关于某个位置的两件事:它所在的那个单位从哪开始,
到哪结束。其余的一切都是从这两件事推出来的。上游那三个默认实现互相递归,正是因为
实现方**覆盖哪一半都够**:知道整段的覆盖 `getTextBoundaryAt`,从位置往外走的覆盖
两端——也因此,三个都不覆盖会死循环,这一点写在 trait 的文档里。

**偏移量是字节。** 上游数 UTF-16 码元,因为 Dart 字符串是 UTF-16;这里数 UTF-8
字节,因为 Rust 字符串是,而且这个 crate 内部本来就用字节——平台通道在边界上转换
(`text_input` 里的 `utf16_to_byte`)。两套约定在 ASCII 上一致、在别处都不一致,
边界类不是留第二套约定的地方。

**记账的边界:** `CharacterBoundary` 走的是 Unicode 标量值,不是扩展字素簇。上游
用 `characters` 包,这个 crate 和 `std` 都没有字素分段器。代价是组合记号、和用零
宽连接符拼起来的 emoji——那些这里会切开而上游不会。对于「一个码点就是一个字符」的
情况(光标通常遇到的)单位是对的。

**两处第一版写错、被回归行抓住的地方:**

* `CharacterBoundary` 的后向边界。上游问的是「`position + 1` 处的那个范围的末
  端」,也就是**大于等于 `position + 1` 的第一个边界**;我第一版写成了「`position`
  所在字符的下一个字符」。两者在正常位置上一样,在 `position = -1` 上不一样:上游
  钳到 0,而 0 处的范围是空的,所以答案是 0 而不是 1。起点之前没有字符可以跨过。
* `ParagraphBoundary` 的前向边界跨 CRLF。上游是 `index -= 2`,落在**这对字符之前
  的那个字符**上;我第一版落在了 CR 自己身上,于是紧接着的循环立刻在 CR 上认出一个
  终止符,把段首报成了 LF 的位置。回归行问的是 "one\r\ntwo" 里位置 4 的段首,应该
  是 0。

**其余回归行盯的地方:** 字节偏移下永远不停在字符中间(否则下一个退格会把字符劈
一半);`-1` 是「那个方向没有边界」,是光标走不出文本的原因;**CRLF 是一个终止符不
是两个**(否则每份来自 Windows 的文件里都会多出一个幽灵空段);上游的
`isLineTerminator` 列了七个字符,只认 `\n` 的话会直接穿过换页符和段落分隔符;软换
行是行边界而**不是**段边界——这正是两个边界各自存在的理由。

验证:`cargo test --lib` 1109 绿,GN `rustflutter_unittests` 1109 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1096 accounted / 777 MISSING。


### 滚动的三样零件(2026-08-19)

`scrollable_helpers.rs`(新),上游 `widgets/scrollable_helpers.dart` 五个类全覆
盖。上游把三件不相干的东西放在一个文件里,理由是各自都太小,这里同理:
`ScrollableDetails`、`EdgeDraggingAutoScroller`、以及键盘滚动的
`ScrollIntent`/`ScrollAction`/`ScrollIncrementDetails`。

**三处结构上的分岔,各有各的理由:**

* `ScrollableDetails` 上游有 `controller` 和 `physics` 两个字段;这里一个都没
  有。台账里 `ScrollController`、`ScrollPosition`、`ScrollPhysics`、`Scrollable`
  **四个上游类映射的都是同一个 `scrolling::Scroll`**——一个 details 对象拿两个句柄
  指着同一个东西,那是在把这个 crate 的形状描述成上游的形状。留下的是真正各自独
  立变化的两样:方向,和装饰裁剪。
* `EdgeDraggingAutoScroller` 上游用 `async` 循环:等一次动画结束,再看一眼。这里
  没有执行器,于是把循环翻过来——`step` 是它的一轮,由持有拖拽的那一方每帧调一
  次,和这个 crate 里其他所有 `advance` 一个形状。
* `ScrollAction` 上游是 `ContextAction`,从 context 找 scrollable。这里的
  `Action` 是个没有 context 的回调,所以 scrollable 在**造 action 的时候**就点
  名——和 `Slider::wired` 同一套接法。上游那条「退回 `PrimaryScrollController`」
  的分支因此没有可退的地方。

`Intent` 枚举加了 `Scroll { direction, increment_type }` 变体;`ScrollIntent` 是
它的构造器而不是独立类型,和 `RequestFocusAction` 那一组同形。

**回归行盯的地方:**

* 反向滚动改的是**方向本身**不是一个 flag——反向的竖直列表是 `up`,不是「向下,
  但倒着」。下游全都读方向;旁边再挂一个 reverse 标志,就得每个下游各自应用一
  次,总有一个会忘。
* 行增量固定 50、不随视口缩放;页增量是视口的**四分之三多一点(0.8)而不是全
  部**,好留一条已读的内容对照。
* 另一根轴上的按键滚动量是 0。竖直列表里的左箭头不是「一点点向上滚」,它是别人的
  按键——返回 0 才让下一个处理器拿到它。
* 反向列表里同一个按键往反方向滚:增量是对着**列表自己的方向**量的,不是屏幕的。
* 拖拽越界的单步有上限(`overDragMax` 20),否则手指停在屏幕外会越滚越快;刚越过
  一点点时,步长是那点越界量本身而不是上限。
* 已经到头的列表不再往前滚,在顶端的也不往回滚。
* **不足一个像素的一步不算一步**——上游最后那道闸。没有它,循环会一直要求一个它已
  经到达的滚动位置,永不停止。

验证:`cargo test --lib` 1099 绿,GN `rustflutter_unittests` 1099 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1091 accounted / 782 MISSING。


### 「点在别处」的那六个类(2026-08-19)

`tap_region.rs`(新),上游 `widgets/tap_region.dart` 六个类全覆盖:
`TapRegionRegistry`、`TapRegionSurface`、`RenderTapRegionSurface`、
`TapRegion`、`RenderTapRegion`、`TextFieldTapRegion`。

菜单、浮层、文本框要问的不是「我被点了吗」——命中测试就能答——而是「**别处**被点
了吗」,而这个问题命中测试自己答不了:没被命中的区域根本不在路径上,不在场的东
西没法被通知。上游的办法是把问题倒过来:树高处的 `TapRegionSurface` 存着所有已
注册区域的名单,一次按下落地时把名单劈成两半——按下穿过的那些,和其余全部。前者
听 `on_tap_inside`,后者听 `on_tap_outside`。**注册就是让没被命中的区域仍然够得
着**,整个文件就是这一个念头。

落到这个 crate 上:

* `TapRegionRegistry` 上游是个抽象类、由 `RenderTapRegionSurface` 实现;这里它
  **就是**那份共享状态,由 surface `provide` 下去。上游那层间接(注册表可能由别
  的东西实现)在只有一个实现的地方买不到任何东西。
* 命中路径的缓存:上游用 `Expando` 挂在命中条目上,这里放在注册表里——因为处理器
  闭包够得着的正是它。surface 的 `hit_test` 写,它自己的 `on_pointer_down` 读。
* 区域在 **layout 时注册**(上游也是那一刻),`Drop` 时注销。禁用的区域是**根本
  不在名单里**,不是在名单里被跳过——所以它连「有人点了别处」都听不到。

**回归行盯的地方:**

* 从没被命中过的区域照样收到通知。这是注册表存在的全部理由,也是命中测试单独做
  不到的事。
* 「里」和「外」是一个划分:点在区域上时它只进「里」,不会两边都算。
* **一个组里任一成员被命中,整组都算被命中。** 文本框和它的选择工具条是两个区域
  一件事;没有组,点工具条就会被读成「点了别处」,把它自己所属的东西关掉。测试
  里把组去掉,同一次命中读出相反的结果。
* **先通知「外」再通知「里」**,这是上游的顺序而且有意义:菜单因为点了别处而关
  闭、和那一点底下的按钮被按下,应该按这个次序发生,否则按钮是在一棵马上要变的
  树里动作。
* 抬起走 `*_up_*` 回调,不走按下的那一对。

**记账的边界:** `consumeOutsideTaps` 存下来也报出来,但**不真的拦住**那次点击。
上游是往手势竞技场里塞一个傀儡成员并宣布它获胜;这个 crate 的竞技场在 router 内
部,没有从外面提出主张的入口。`TapRegionRegistry::last_dispatch_consumed` 是替代
的读法,竞技场钩子是这件事将来收尾的地方——这条也写成了回归行,免得它哪天悄悄变
成别的样子。另外上游的 surface 还监听语义通道(读屏器发出的点击也要参与分类),
那是 `E3`。

验证:`cargo test --lib` 1088 绿,GN `rustflutter_unittests` 1088 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1086 accounted / 787 MISSING。


### 把尺子上的假阳性一次查干净(2026-08-19)

上一簇顺手抓到 `mod` 会冒充类之后,把同一个问题**系统查了一遍**:拿上游每个公开
类去问「crate 里认领它的那个标识符,到底是什么」。结果是尺子还在替十一个类说谎,
全都是名字撞上了方法或私有辅助函数:

| 类 | 认领它的东西 |
| --- | --- |
| `Element` | `BuildContext::element()` |
| `ClipRect` | `Canvas::clip_rect()` |
| `Flex` | `Table::flex()` |
| `AnimatedSize` | `render.rs` 里一个私有 `fn animated_size` |
| `PreferredSize` | 满地的 `preferred_size()`——上一簇我自己刚加了十几个 |
| `Scrollable` | `SemanticsProperties::scrollable()` |
| `Viewport` | 一个测试辅助函数 |
| `Title` | `Theme::title()` |
| `Placeholder` | `InlineSpanSemanticsInformation::placeholder()` |
| `Step` | 焦点遍历里的私有 `fn step` |
| `Cubic` | 三次贝塞尔的私有求值函数 |

**为什么 `fn` 本来要算。** 这个 crate 有一部分 widget 门面就是函数——
`pub fn spacer() -> AnyWidget` 确实是上游 `Spacer` 的移植,`repaint_boundary`、
`keyed_subtree`、`directionality`、`notification_listener` 都是。所以不能一刀切
掉 `fn`。分界线是**自由的、公开的**:方法缩在 `impl` 里,辅助函数不 `pub`。
`tools/coverage.py` 现在对 `fn`/`const`/`static` 只认行首的 `pub`,类型和 `impl`
目标照旧不限位置。上面十一个一个不剩地掉了出来,五个真门面一个没误伤。

**掉出来之后的处置。** 逐个看了,分三类:

* 真有实现、只是换了名字——补进台账:`MatrixUtils`→`painting::matrix_utils`
  (同名模块,三个函数逐个对应)、`TextInput`→`services::text_input` 的四个入口、
  `Element`→`ElementTree`+`ElementId`(元素在这里是树上的一行,不是一个对象)、
  `Cubic`→`Curve::Cubic` 变体、`Scrollable`→`Scroll`。
* 渲染对象一直都在、缺的只是前面那层 widget——补了四个门面:`clip_rect`、
  `flex`、`animated_size`、`viewport`,都是 `repaint_boundary` 那个形状。
* 剩下的 `PreferredSize`、`Placeholder`、`Title`、`Step` 确实没写过,继续记
  MISSING。

`flex` 的回归行盯的是一个容易写错的默认值:上游 `Flex` 的 `mainAxisSize` 默认是
`max`,所以一行**不是**贴着孩子收窄,而是把允许的宽度占满。高度才是「孩子确实进
去了」的证据。

验证:`cargo test --lib` 1080 绿,GN `rustflutter_unittests` 1080 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1080 accounted / 793 MISSING——数字跟上一簇几乎一样,
但这次每一个都站得住。


### 区间滑块的部件,与尺子上的三个假阳性(2026-08-19)

`range_slider_parts.rs`(新),上游 `range_slider_parts.dart` 的 15 个类全覆盖:
`RangeSliderThumbShape`/`RangeSliderTickMarkShape`/`RangeSliderTrackShape`/
`BaseRangeSliderTrackShape` 四个基类,`RoundRangeSliderThumbShape`、
`HandleRangeSliderThumbShape`、`RoundRangeSliderTickMarkShape`、
`RectangularRangeSliderTrackShape`、`RoundedRectRangeSliderTrackShape`、
`GappedRangeSliderTrackShape` 六个具体形状,
`RoundedRectRangeSliderValueIndicatorShape`、
`DropRangeSliderValueIndicatorShape` 两个气泡,以及 `RangeValues`、
`RangeLabels`。`SliderThemeData` 的四个 `range*Shape` 字段与 `thumbSelector`
一并补齐——36/36。

区间版**多数**只是单值版加了第二个滑块头,真正分岔的地方写成了回归行:

* `BaseRangeSliderTrackShape` 在主题带 `padding` 时**仍留半个滑块头**,单值版
  一点都不留。区间滑块的两个头都要走到行程尽头,外侧那半个永远省不掉。没有
  `padding` 时两者算出来一模一样——分岔只在那一个分支里。
* 区间轨道的两段颜色**不随文字方向对调**。单值轨道要对调,是因为「滑块头之前」
  换了一边;区间轨道的活动段在两个头之间,怎么摆都是那一段,变的只是哪个头在
  左边。
* 刻度问的是「在不在两个头之间」,不是「在不在那一个头之前」。这两个问题对第二
  个头之后的每一个刻度都给出不同答案。
* 轨道**有间隙时**才丢掉正压在滑块头下面的刻度——那个位置本该是空的。没有间隙时
  照画,滑块头盖住就是了。两种情况都丢,M2 的每条滑块都会少一个刻度。
* 两个滑块头共用一条 activation 动画。单值版无条件读它,区间版只在自己被按住时
  读,否则拖一个头两个都会抬起来。
* `RangeThumbSelector` 上游是个 typedef,这里是具名类型:裸 `dyn Fn` 既没有
  `Debug` 也没有 `PartialEq`,而带着它的主题两个都要。相等按闭包身份算,和
  `StateProperty` 同规则同理由。

上游在这个文件里又抄了一份 `_RoundedRectSliderValueIndicatorPathPainter` 和
`_DropSliderValueIndicatorPathPainter`,常数一位不差,所以这里的两个区间气泡直接
调 `slider_theme` 里的画笔,没有抄第二份。

**尺子上的三个假阳性。** 新模块本来叫 `range_slider.rs`,而尺子把声明的标识符按
蛇形折成驼峰再比对,于是 `mod range_slider` 冒充了上游的 `RangeSlider` 控件。改名
`range_slider_parts.rs`(也正是上游的文件名)去掉了它。顺手把根因也修了:
`tools/coverage.py` 的声明正则里去掉了 `mod`——模块是文件,不是类型。这又还回来两
个:`mod matrix_utils` 一直在冒充 `MatrixUtils`,`mod text_input` 一直在冒充
`TextInput`,两个类型都没人写过。三个都是尺子在替我们说谎,现在不说了。

验证:`cargo test --lib` 1079 绿,GN `rustflutter_unittests` 1079 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1082 accounted / 791 MISSING——比上一簇多 12,是
15 个新类减去还回来的 3 个假阳性。滑块一族到此只剩 `Slider` 与 `RangeSlider`
两个控件本体。


### 滑块的数值气泡(2026-08-19)

`slider_theme.rs`。滑块拖动时浮在滑块头上方的那个气泡,上游有四种画法,加上
range 版的两个,共 7 个公开类落地:

* `RectangularSliderValueIndicatorShape`——圆角矩形加一个小三角。
* `RoundedRectSliderValueIndicatorShape`——M3 的,一个胶囊,没有尾巴。
* `DropSliderValueIndicatorShape`——尾巴更宽的三角,整体读作一滴水。
* `PaddleSliderValueIndicatorShape`——M2 的桨形:两个圆用一段收腰的颈连起来,
  标签变宽时上圆横向摊开、颈上的弧往下走以保持相切。这一个是三角函数,不是
  查表。
* `RangeSliderValueIndicatorShape`(枚举,抽象基类在上游
  `range_slider_parts.dart`)+ `RectangularRangeSliderValueIndicatorShape`、
  `PaddleRangeSliderValueIndicatorShape`。

上游那四个私有 `_...PathPainter` 在这里是四个私有 struct。它们改成收
`label_width`/`label_height` 两个数而不是收 `TextPainter`——单元测试里没有引擎去
排版文字,`rf_paragraph_width` 桩固定返回 0,收数字才检验得了那套算术。公开的
`preferred_size` 仍然收 `TextPainter`,和上游一样。

**`RenderPath::arc_to`(`painting.rs`,新)。** 桨形要往路径上接弧,绑定层没有
`arcTo`。这里用三次贝塞尔逼近:扫角切成不超过四分之一圈的段,每段的控制点取
切线方向的 `4/3 * tan(Δ/4)`。两端精确,中间的误差在滑块画的任何半径下都不到
一个像素。切出来的段由 `arc_cubics` 返回,单独可测——回归行检查每段中点确实落
在椭圆上,因为切线符号写反的时候两端仍然对,只有中间会往反方向鼓出去。

**几处容易读错、已经写成回归行的地方:**

* 气泡靠边时往里推,但**推不动的时候上游不再居中**:气泡比整条滑块还宽时,它
  改为把溢出更多的那一边钉在 8px 边距上。以为第一个分支到处适用,气泡两头都会
  露出去。
* drop 形状的圆角看着是从 4px 插值到全圆,但 `rectness` 常数是 0,所以永远是全
  圆那一端。上游把旋钮留在了那里。
* 桨形的高度是常数 66(两心距 40 + 上圆 16 + 下圆 10),标签只让它变宽,不让它
  变高。
* 桨形横移的上限是标签本身撑开的那点余量——滑块贴到框边时要得更多,拿到的是
  夹断后的值。
* `scale` 为 0 时直接返回。上游写明了原因:再往下算会把 NaN 送进引擎。
* range 版的两个形状都不读传进来的 `Thumb`。上游照传,这里也照传,但指望起点和
  终点的气泡长得不一样是读了不存在的东西。
* 装成数值气泡的形状用 `paint_indicator`,两参数的 `preferred_size` 对气泡答不
  出来——上游那个重载是靠可选具名参数加上标签的,这里是
  `preferred_size_for_label`。

`SliderThemeData::from_primary_colors` 补上了 `value_indicator_shape`
(`PaddleSliderValueIndicatorShape`),上一簇因为类型还没有而空着。

验证:`cargo test --lib` 1069 绿,GN `rustflutter_unittests` 1069 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1070 accounted / 803 MISSING。`slider_parts.dart`、
`slider_theme.dart`、`slider_value_indicator_shape.dart` 三个文件已全覆盖;
`range_slider_parts.dart` 余 14 个,是下一簇。


### 滑块的部件与主题(2026-08-19)

`slider_theme.rs`(新)。上游把滑块拆成五个可换的部件——轨道、刻度、滑块头、
按压光晕、数值气泡——每个部件是一个抽象类带若干具体子类。这里沿用
`ShapeBorder` 的做法:具体类各自是 `struct`,抽象基类是把它们收进去的 `enum`。
集合是封闭的,画的时候要的是一个 `match`。

本簇落地 13 个上游类:

* `SliderComponentShape`(枚举)+ `RoundSliderThumbShape`、`HandleThumbShape`、
  `RoundSliderOverlayShape`。三者上游同属一个类型,因为主题上那三个字段是可以
  互换的——写成滑块头的形状可以装成光晕——这里保留了这一点,没有拆成三个枚举。
* `SliderTickMarkShape`(枚举)+ `RoundSliderTickMarkShape`。
* `SliderTrackShape`(枚举)+ `BaseSliderTrackShape`、
  `RectangularSliderTrackShape`、`RoundedRectSliderTrackShape`、
  `GappedSliderTrackShape`。上游的 `BaseSliderTrackShape` 是 mixin,这里是自由
  函数:一段三个形状共用、谁都不覆盖的计算。
* `SliderThemeData`(36 个字段里的 32 个)与 `SliderTheme`。

顺带补上 `Size::from_radius`/`shortest_side` 与 `Rect::from_center`/
`from_circle`/`center`/`shortest_side`——都是上游 `dart:ui` 上的真方法,滑块的
几何是照着它们写的。`component_themes.rs` 的三个 `lerp_*` 助手改为
`pub(crate)`。

**控件接线。** `components.rs` 的 `Slider` 原先是硬写的 8px 轨道加 18px 圆头,
颜色取自 `components::Theme`。现在走 `ResolvedSlider::of`,轨道高度、两段颜色、
滑块头的颜色与尺寸都从主题下来。视觉上这是 M3 的样子:轨道 16px,滑块头是
4×44 的竖条。gallery 的 `sliders_demo` 已经在用 `Slider`,因此这一簇的新东西
是被现成的 demo 带着的——**Windows 与 Android 需要手验一眼**,因为轨道和滑块头
的形状变了。

**几处容易读错、已经写成回归行的地方:**

* `getPreferredRect` 在两端留的是「滑块头与光晕里较宽的那个」的一半,主题带了
  `padding` 时则一点都不留——上游那个 null 判断是二选一,不是相加。
* 两段轨道颜色都透明时,只有高度塌成 0,宽度不变。上游的注释说了原因:让一条
  看不见的轨道不至于把旁边的东西挪位置。
* 父盒子比滑块窄时,右边会跑到左边的左侧;上游是把两个数对调,不是断言。
* `HandleThumbShape::preferred_size` 是常数 4×44,但真正画出来的尺寸来自
  `SliderThemeData::thumb_size`。两个数不一样,也不该一样——M3 的滑块头在拖动
  时会变窄,所以那是个 state property。
* 光晕的 preferred size 不随状态变。按下才画,但位置一直占着——否则轨道会在手
  指底下跳。
* `year_2023` 选的是一整张默认值表,不是单个字段。以为它只改轨道高度,形状就
  全错了,连带滑块头也错。
* 主题的 lerp 里颜色和数字插值,形状和枚举取近端:两个形状中间的形状不是形状。

**记账的边界:**

* 四个 `range*Shape` 字段与 `thumbSelector` 等 `range_slider_parts.dart`——它们
  要指的类型还没有。
* 数值气泡的六个形状(`RectangularSliderValueIndicatorShape`、
  `PaddleSliderValueIndicatorShape` 及其 range 对应物、
  `DropSliderValueIndicatorShape`、`RoundedRectSliderValueIndicatorShape`)是下
  一簇。
* 上游滑块头的高度用 `Canvas.drawShadow` 画。引擎绑定没有这个调用,这里改用
  `elevation_shadows` 的那几个圆——`drawShadow` 查的就是同一张表。

验证:`cargo test --lib` 1058 绿,GN `rustflutter_unittests` 1058 绿、
`flutter_gallery_unittests` 322 绿,`flutter_gallery.exe` 链接通过,
`cargo fmt` 干净。覆盖率 1063 accounted / 810 MISSING(57%),material 层
167 accounted / 219 MISSING(43%)。


**P8-M1 收官:button bar 与 theme extension(2026-08-19)。**
`ButtonBarTheme(Data)`(9/9)、`ThemeExtension` + `ThemeExtensions`。

**`ThemeExtension` 是应用自带主题数据的口子**:上游按运行期类型作键,
`Theme.of(context).extension<T>()` 取回;此侧键是 `TypeId`,取回是
`ThemeData::extension::<T>()`。trait 特意保持对象安全(主题存一列它们、并不知道
类型),所以 `lerp` 收发的是 trait 对象而非 `Self`——实现里 downcast,遇上不同类
的就保留自己,这正是上游 `covariant` 参数在运行期的意思。**一端没有的扩展在插值
里保留**:新主题没提到它不等于把它删了。

`ThemeExtensions` 的相等按身份(Rc::ptr_eq),与 `StateProperty` 同一条理由。

**`ActionIconThemeData` 入账为"未做",理由写清**:它四个字段全是
`WidgetBuilder`,整个类由"主题里放 widget 构建器"组成——与 `ButtonStyle` 的两个
builder、`MenuThemeData.submenuIcon`、`SegmentedButtonThemeData.selectedIcon`
是**同一条边界**,不是各自的疏漏。

**P8-M1:排版(2026-08-19)。** `TextTheme`(15/15)+`Typography`(3 个几何),
表由脚本解析上游 `_M3Typography` 生成。`ThemeData` 补上 `textTheme` 与
`primaryTextTheme`——后者是"在 `primaryColor` 上读得清"的那一套(暗色主题的 bar
取 surface,所以它用 `onSurface` 而不是 `onPrimary`)。

**测试抓到一件事,而且它是对的**:我原本写了一条"三个几何应当彼此不同"的回归
线,它挂了。查上游才知道——**M3 的三套几何数字完全相同**,只差
`textBaseline`(dense 是 ideographic,`englishLike` 与 `tall` 逐字段相同)。
M2 那三套确实差字号与行高,M3 不再。此侧 `TextStyle` 不带 baseline(引擎文本
ABI 没这一项),于是三个函数目前答同一张表。**三个函数仍保留**:这个区分是
上游的,baseline 能带的那天就回来了,合并成一个等于把限制焊进 API。回归线改成
断言"三者数字相同"并写明缘由,免得下一个人以为是漏抄。

**P8-M1:navigation bar / drawer / carousel 三对(2026-08-19)。**
`NavigationBarTheme(Data)`(12/12,连带 `NavigationDestinationLabelBehavior`)、
`NavigationDrawerTheme(Data)`(10/10)、`CarouselViewTheme(Data)`(5/6,缺
`itemClipBehavior`)。

**底部有两个 bar,两套主题**:M2 的 `BottomNavigationBar` 与 M3 的
`NavigationBar` 是**不同的 widget**,上游各给一套主题,连图标主题的形态都不同
——M2 是选中/未选中两个字段,M3 是一个按状态解析的属性。此侧照搬,回归线点明
"设了 M3 那套,M2 那套仍是空的"。

`NavigationDrawerThemeData.indicator_size` 是这几对里唯一真插值的字段
(两端都有 Size 时取中间),其余按近端切换。

**P8-M1:popup / dropdown / bottom app bar 三对(2026-08-19)。**
`PopupMenuTheme(Data)`(13/13,连带 `PopupMenuPosition`)、
`DropdownMenuTheme(Data)`(4/4)、`BottomAppBarTheme(Data)`(7/7)。

`DropdownMenuThemeData` 的三个零件各自复用**已有的类型**:字段的装饰是
`InputDecorationThemeData`、掉下来的菜单是 `MenuStyle`——上游没有为下拉框另造
第三种,此侧照做。`BottomAppBarThemeData` 是唯一带 `NotchedShape` 的组件主题
(浮动按钮坐进去的那个缺口)。

`PopupMenuThemeData` 同时有 `textStyle` 与 `labelTextStyle`:后者按状态、且在
两者都设时压过前者——上游为兼容旧代码留着前者。

**P8-M1:icon theme 与 text selection(2026-08-19)。**
`IconTheme(Data)`(9/9)、`TextSelectionTheme(Data)`(3/3)。

**`IconThemeData` 早就该做了**:前面四个组件主题(app bar 两个、chip、
navigation rail 两个、bottom navigation 两个)一共**七个字段**因为"框架无图标
体系(E5)"被记账推掉——但 `IconThemeData` 只是个数据类,画图标才需要 `Icon`
widget。这轮把它补上,那七个字段一并填回,记账里对应的话也删了。**"依赖某物"
和"依赖某物的某一部分"是两件事,当初记账时没分清。**

`opacity` 照上游的做法:存进去时不动,取出来时钳到 0..1——所以 merge 与 lerp
看到的是原值,画笔看到的是钳过的。上游 `Shadow` 此侧用 `BoxShadow`(同样的三个
字段多一个 spread,留零)。

**P8-M1:input decoration(2026-08-19)。** `InputDecorationTheme(Data)`
(37/37)+`FloatingLabelBehavior`/`FloatingLabelAlignment`。这是继 date picker
之后第二长的一个,长的道理也一样:一个带装饰的输入框是一叠零件——会浮的标签、
它底下的提示、下方的帮助与错误、两侧的前后缀、末尾的计数器,再加**五种状态各
一个边框**——每个零件各自设主题。

**五个边框是五个字段而不是一个状态属性**,这是上游的形态且有原因:
`InputBorder` 是 `ShapeBorder`,没法从状态集解析出来,所以"状态"体现为**读哪个
字段**。`resolve_border` 照抄上游 `_getFallbackBorder` 的顺序:disabled →
focused+error → error → focused → enabled,都没设才落到 `border`。回归线专门
验了两处会猜错的:**focused 且 error 但 focusedErrorBorder 未设时,落到 `border`
而不是落到 focused 或 error 那两个**;以及 disabled 压过 error。

上游此类**没有 `lerp`**(边框是形状、开关是开关,输入框不在两者之间做动画),
`ThemeData::lerp` 按近端取并写明原因。

**P8-M1:输入边框(2026-08-19)。** `UnderlineInputBorder`/
`OutlineInputBorder` 落进 `borders.rs`,并作为 `ShapeBorder` 的两个新变体接进
全部九处 match(dimensions/side/outer_path/inner_path/scale/hit/paint/两处
lerp)。`InputBorder` 抽象基类入账为"这两个变体"。

**`OutlineInputBorder` 的缺口是它的全部要点**:上边框要为浮起的标签让开一段,
而且是随标签升起**逐渐张开**的(`gapPercentage`)。`gap_path` 照抄上游
`_gapBorderPath` 的走法——从缺口右端起,顺时针绕完三边,回到缺口左端,是一条
**开放**路径。回归线在 percentage 的 0、0.5、1 三点都画一遍:带百分比的几何
就得在区间两端各验一次。

`UnderlineInputBorder` 画圆角规则线时,上游先把两个下角半径**钳到高度的一半**
("防止抗锯齿的舍入让颜色漏出来"),此侧照做。

余 `ShapedInputBorder`(上游较新的一件,用任意 ShapeBorder 作输入边框)未做。

**P8-M1:date picker(2026-08-19)。** `DatePickerTheme(Data)`(42/44),
组件主题里最长的一个,长得有道理:一个日期选择器是**四个面**——对话框本身、
里面的日期网格、后面的年份网格,以及**范围选择器**。上游给范围选择器**另存
一份对话框的全部字段**(`rangePicker*`),因为范围选择是整页而不是对话框,
不想要对话框那套颜料。回归线点明"设了对话框的头部色不会波及范围选择器的"。
缺 `inputDecorationTheme`(随文本框簇)与 `locale`(随本地化 E4)。

**P8-M1:time picker(2026-08-19)。** `TimePickerTheme(Data)`(23/25)。
上游这个类的字段名脱开语境很难读,照抄不改:一个时间选择器有**表盘**(带指针)、
上方的**时分**一对字段、以及 AM/PM 的**日段**开关,三样各自设主题——它们是三个
不同的东西,只是恰好同处一个对话框。缺 `inputDecorationTheme`
(`InputDecorationThemeData`,随文本框簇来)。

**P8-M1:search 一对(2026-08-19)。** `SearchBarTheme(Data)`(12/12,连带补
`TextCapitalization`)、`SearchViewTheme(Data)`(13/13)。

**这一对的形状差别值得记**:bar 的字段全是状态属性、view 的全是裸值。上游
如此,道理也直白——bar 是指针会碰的控件,view 是它打开出来的面板,面板要么开
着要么不在,没有 hovered 一说。回归线两半都验了。

**P8-M1:FAB 与 toggle buttons(2026-08-19)。**
`FloatingActionButtonTheme(Data)`(21/21)、`ToggleButtonsTheme(Data)`(15/15)。

**FAB 的五个高度是五个字段而不是一个状态属性**,这是上游的形态:它早于
`WidgetStateProperty` 且没有迁过来。此侧照抄——"现代化"它等于替上游回答一个
它没回答的问题。`ResolvedFloatingActionButton` 按上游的顺序挑一个:
disabled → pressed → hovered → focused → 静止,**挑一个而不是混一个**;未设的
hover/focus 高度回落到静止高度(而不是某种插值),回归线点明了这一点。

`ToggleButtonsThemeData` 的三个标签色(常态/选中/禁用)同样各自独立。

**P8-M1:scrollbar 与菜单一族(2026-08-19)。**
`ScrollbarTheme(Data)`(11/11)、`MenuStyle`(13/13)、`MenuTheme(Data)`(1/2)、
`MenuBarTheme(Data)`、`MenuButtonTheme(Data)`、`SegmentedButtonTheme(Data)`
(1/2)。**覆盖过千:1008/1873。**

`MenuStyle` 与 `ButtonStyle` 同形不同字段——一个是面板能被告知的事,一个是
标签能被告知的事。`MenuBarThemeData` 上游是 `MenuThemeData` 的无字段子类
(为了让菜单栏与挂在它上面的菜单分开设主题),此侧是两个同形的类型,回归线
点明"装了一个不等于装了另一个"。

**`Scrollbar` 接上了**:thickness/thumb color/radius/margins/minThumbLength
走 `ScrollbarTheme.of`。这里有个**已有形态要保住**——`ScrollbarMetrics` 是
参数而不是常量,正因为 `CupertinoScrollbar` 就是这个 widget 换一套度量。所以
判据是"调用方没有覆写过 metrics 时才让主题说话",这与上游的链首(widget
自己的字段)是同一件事。thumb 的默认按 states 分:拖动时是 outline 实色,
闲置时 0x4d,回归线两条。

**记录在案的分歧**:`MenuThemeData.submenuIcon` 与
`SegmentedButtonThemeData.selectedIcon` 都是"主题里放 widget"
(`WidgetStateProperty<Widget?>` / `Widget?`),与 `ButtonStyle` 的两个 builder
同一类,此侧还没有这个形态的位置。

**P8-M1:banner / expansion tile / M2 按钮主题(2026-08-19)。**
`MaterialBannerTheme(Data)`(8/8)、`ExpansionTileTheme(Data)`(12/13,缺
`clipBehavior`)、`ButtonTheme(ButtonThemeData)`(15/15,连带补
`ButtonTextTheme`/`ButtonBarLayoutBehavior`)。

**`ButtonThemeData` 与前一轮那五对不是一回事**:它是上游 M2 那套按钮的主题
——问的是"最小宽度、高度、文字主题",而不是"每个状态一个属性"。上游两套并存,
此侧也并存,`ThemeData` 上两个字段各占各的。它的 `padding` 回退按 `textTheme`
分叉(primary 24、其余 16),这条单独上了回归线。上游 `ButtonThemeData` **没有
`lerp`**——字段是尺寸不是颜料,按钮条不会在两个宽度之间做动画——所以
`ThemeData::lerp` 里它按近端取,这一点在代码里写明了。

**`Banner` 接上了**:底色、下方那条规则线、内距都走
`MaterialBannerTheme.of`。回归线用"改内距后高度的变化量"验,避开绝对值。

**`ExpansionTileThemeData` 的两态是两个字段而不是一次插值**——展开与收起的
底色/文字色各自独立,回归线点明了这一点。

**P8-M1:按钮一族,以及两处尺子的假阳性(2026-08-19)。**

**先是个真错:crate 的 `ButtonStyle` 不是上游的 `ButtonStyle`。** 此侧那个是
Filled/Outlined/Text/Danger 四变体的枚举,上游那个是二十五个状态属性的口袋
——**尺子按名字比对,于是把一个没移植的类记成了 covered**。改名为
`ButtonVariant`(它本来就是这个:上游 `FilledButton`/`OutlinedButton`/
`TextButton`/`ElevatedButton` 四个 widget 的差别只在默认 `ButtonStyle`,
一个带 variant 的 Button 是同一套东西的另一种说法),全仓 66 处一并改。

**然后是上游的 `ButtonStyle` 真身**:22/25 字段,其余三件记账
(`splashFactory` 同前;`backgroundBuilder`/`foregroundBuilder` 是把子件包进
任意 widget 的构建器,主题里放 widget 的形态此侧还没有)。`merge` 是上游
"调用方的 style → 主题的 style → 控件自己的默认"三次合并里的那一次。

**五对按钮主题**(elevated/filled/text/outlined/icon),各自只有一个 `style`
字段。**第一版用 macro_rules 生成五对——那是第二处假阳性**:尺子从源码文本里
读声明,而宏体里的 `pub struct $data` 不是它能看见的声明,于是十个真类它一个
也数不到;更糟的是宏名 `button_theme` 让上游的 `ButtonTheme` 被记成 covered。
改为五对写全。**尺子是验收门,写法要让尺子能数。**

**`Button` 接上了**:按 variant 读对应的那个主题(filled 读 filled、outlined
读 outlined),states 由 enabled/pressed 组出,背景/前景/边/内距/最小尺寸各自
解析,控件原有的三式默认作为最后一档。回归线四条,含"filled 的主题不该影响
outlined 的按钮"。

上游四个按钮 widget 与 `ButtonStyleButton` 基类入账为 `Button` + variant。

**P8-M1:导航三对(2026-08-19)。**
`NavigationRailTheme(Data)`(11/13,缺两个 `IconThemeData`——E5,连带补
`NavigationRailLabelType`)、`BottomNavigationBarTheme(Data)`(12/14,同缺两个
图标主题,连带补 `BottomNavigationBarType`/`BottomNavigationBarLandscapeLayout`)、
`DrawerTheme(Data)`(8/9,缺 `clipBehavior`)。

**`Drawer` 接上了,并且顺手修了一处语义**:`Drawer.width` 此前在构造器里就写
死成 `DRAWER_WIDTH`,于是"没人指定"和"指定成 304"没法区分——主题永远插不进
来。改成 `Option<f32>`(上游的 null),取值顺序回到上游的
**widget 自己的字段 → `DrawerTheme.of` → `_kWidth`**,三档各一条回归线。
背景色也从 `theme.surface` 改为 `_DrawerDefaultsM3.backgroundColor`
(`colorScheme.surfaceContainerLow`),这是 `ColorScheme` 落地后才拿得到的角色。

**P8-M1:chip / tab bar / data table 三对(2026-08-19)。**
`ChipTheme(Data)`(22/23,缺 `iconTheme`——E5)、`TabBarTheme(Data)`(15/16,
缺 `splashFactory`,连带补 `TabBarIndicatorSize`/`TabAlignment`/
`TabIndicatorAnimation` 三个上游枚举)、`DataTableTheme(Data)`(15/15)。

**`Chip` 接上了**,并且这一簇把三步回退的**最后一步**写对了:上游的顺序是
M3 的 `color` 状态属性 → 按标志的 `selectedColor`/`disabledColor` →
`backgroundColor` → **控件自己的默认**。第一版把最后一步写成"主题整体等于空
就用旧默认",那是个对着整份数据比相等的将就;改成
`ResolvedChip::of(context, states, default_fill)` ——控件把自己的默认传进来,
正是上游 `?? 控件默认` 那一环。crate 的 Filter/Selected/Action 三式就是这个
默认,所以没有主题时外观一如既往。四步顺序四条回归线。

**记录在案的分歧**:`TabBarThemeData.splashFactory`
(`InteractiveInkFeatureFactory`)不做——此侧的墨水是画它的那个控件的属性
(ink.rs),不是主题往下传的工厂。

**P8-M1:list tile 与 dialog 两对(2026-08-19)。**
`ListTileTheme(Data)`(22/22,连带补上 `ListTileStyle`/`ListTileControlAffinity`/
`ListTileTitleAlignment` 三个上游枚举)、`DialogTheme(Data)`(13/14,缺
`clipBehavior`,同前)。

**`ListTile` 接上了**:content padding、标题与尾件之间的间距、**最小高度**、
以及选中/未选中两套底色与文字色,全走 `ListTileTheme.of` 再落到上游默认
(56,dense 时 48;间距 16;`minLeadingWidth` 40)。此前 padding 是
`spacing*1.5` 的两倍、间距是常量 `HORIZONTAL_TITLE_GAP`、根本没有最小高度。
最小高度用 `RenderConstrainedBox` 包一层实现——`Container` 没有约束面。

**选中态是取另一套值而不是混色**,与上游一致:`selectedTileColor` 与
`selectedColor` 各自独立,`ResolvedListTile::of(context, selected)` 按标志挑,
回归线两条都验了。

**测试破千**:1000 通过 / 0 失败。

**P8-M1:结构件三对组件主题(2026-08-19)。**
`AppBarTheme(Data)`(14/17)、`BottomSheetTheme(Data)`(12/13)、
`SnackBarTheme(Data)`(14/15,含 `SnackBarBehavior` 两式)。

**`AppBar` 接上了**:背景取 `AppBarTheme.of` 否则 `scheme.surface`,前景取
否则 `onSurface`,高度取否则 `kToolbarHeight`(56)。此前是 `theme.surface`
与两个写死的常量。**主题给的高度压过"有副标题就用高的那个"**,回归线按两个
不同主题高度的差值验证(bar 自身还画一条分隔线,直接比绝对值会把那条线算进去)。

**未做的字段,逐条记账**:`AppBarThemeData` 的 `iconTheme`/`actionsIconTheme`
(`IconThemeData`,框架无图标体系——E5)与 `systemOverlayStyle`
(`SystemUiOverlayStyle`,services 侧未到);`BottomSheetThemeData` 与
`SnackBarThemeData` 的 `clipBehavior`(同前:dart:ui 的 `Clip` 不建模);
`SnackBarThemeData.dismissDirection`(`DismissDirection` 随 P6 的
`Dismissible` 一起来)。

**P8-M1:选择控件三对组件主题(2026-08-19)。**
`CheckboxTheme(Data)`/`RadioTheme(Data)`/`SwitchTheme(Data)` 落地,字段按上游
列全(9/9/9)。这三对与前五对的区别是它们的字段大半是
`WidgetStateProperty<Color?>`,于是先补了两件地基:

- **`StateProperty<T>`**(widget_state.rs)——主题里放 `WidgetStateProperty`
  的槽。上游用 `==` 比两份主题,而 `WidgetStatePropertyAll` 按值比、
  `resolveWith` 的回调按身份比;此侧属性一律在 `Rc` 后面,所以**相等即身份**:
  同一个属性对象才算同一个属性,重建出来的解析器算"变了"。这是安全的那一边
  ——解析器可以闭包住任何东西。`lerp_state_property` 是上游
  `WidgetStateProperty.lerp(a, b, t, Color.lerp)`:两端各自按同一组 states
  解析后再插值。
- **`MaterialTapTargetSize`**——Padded/ShrinkWrap 两式 +
  `kMinInteractiveDimension`(48)。

**`Checkbox` 接上了**:按 checked/enabled 组出 `WidgetStates`,
`ResolvedCheckbox::of` 走三步回退——fill 取主题属性、否则上游默认
(选中取 primary、未选中透明、禁用时 onSurface 的 0x61 alpha),side 取主题、
否则 `onSurfaceVariant` 两像素。此前是 `theme.primary`/`theme.outline` 的
三行条件。

**记录在案的分歧**:`SwitchThemeData.thumbIcon` 未做——它是
`WidgetStateProperty<Icon?>`,框架侧还没有图标体系(E5)。

**P8-M1:组件主题的三步回退接通了(2026-08-19)。** `component_themes.rs`,
头五对:`DividerTheme(Data)`/`CardTheme(Data)`/`BadgeTheme(Data)`/
`TooltipTheme(Data)`/`ProgressIndicatorTheme(Data)`——**每一对的字段都按上游
逐条列全**(divider 六个、card 六个、badge 八个、tooltip 十五个、progress
十四个),各带 `lerp`。

**机制是三步回退**,与上游一字不差:`XTheme::of(context)` 先找最近装上的
`XThemeData`,没有就取 `ThemeData` 上的同名字段,字段仍未设则落到控件自己的
默认(通常是 `ColorScheme` 上的某个角色)。`ResolvedDivider::of` 把这三步写
出来一次,因为每个控件做的都是同一件事。**未设的字段不是偷懒**——"未设"的
意思就是"听主题的",只有设了的才覆盖。

`ThemeData` 因此收了五个组件主题字段,并从 `Copy` 变成 `Clone`(tooltip 与
badge 带 `TextStyle`/`Decoration`,不是 `Copy`)。`AnimatedTheme` 随之不再走
`implicit::animated`(它要 `Copy`),改成自带状态的隐式动画——中途换目标从
**当前值**重新起步的那条规则原样写出。

**两个控件接上了**:`Divider` 现在按 space/thickness/color/indent 画
(此前是写死的 16 高、1 像素、`theme.outline`);`Card` 的底色与高度取
`CardTheme.of`。回归线:没有主题时 divider 高 16(上游默认),装上
`DividerTheme(space: 40)` 变 40,只在 `ThemeData` 上设则是 24——三步都走到了
控件的几何上。

**记录在案的分歧**:`clipBehavior` 不建模(dart:ui 的 `Clip`;此侧裁剪是
做裁剪的那个渲染对象的属性,不是主题往下传的值)。

**P8-M1:`ThemeData` 立起来了(2026-08-19)。** `theme.rs`:

- **`ColorScheme::light_m3()`/`dark_m3()`**——上游 `_colorSchemeLightM3`/
  `_colorSchemeDarkM3` 两张常量表,同样由脚本从 theme_data.dart 解析生成。
  这是 `ThemeData()` 在没人指定方案时用的两套,也就是一个 M3 应用真正跑的
  配色——**不需要 fromSeed 也能拿到正经的 M3 默认值**。
- **`ThemeData`(通用半边)**——`from_color_scheme` 就是上游构造器的推导表:
  `primaryColor`=primarySurfaceColor(暗色取 surface、亮色取 primary)、
  canvas/scaffold/card=surface、divider=outline,其余按亮暗取常量
  (highlight `0x66BCBCBC`/`0x40CCCCCC`、unselected black54/white70、
  focus 12%、hover 4% ……),`applyElevationOverlayColor` 恰为"是否暗色"。
  `light()`/`dark()`/`lerp`/`of(context)` 齐。
- **`VisualDensity`**——标准/舒适/紧凑三式、`baseSizeAdjustment`(每单位四像素)、
  `effectiveConstraints`(只动 minima,且不越过 maxima、不低于零)、
  `adaptivePlatformDensity`。
- **`MaterialTheme`/`ThemeDataTween`/`AnimatedTheme`**——theme.dart 三件齐。

**两个主题类型并存的接法(不是上游有的东西,是这一步的桥)**:
`ThemeData::to_component_theme()` 逐角色派生出 crate 既有的
`components::Theme`,`MaterialTheme::new` 一次 provide 两者。这样 controls
可以一簇一簇地迁过来,而不是一笔提交动全部——`components::Theme` 十四个字段
被 components/controls/cupertino/pickers 和整个相册读着。

**记录在案的分歧**:`primarySwatch`/`ColorScheme.fromSwatch` 不做(M2 的入口,
上游正在弃用,见 flutter#91772);`useMaterial3` 不设字段(上游用它在迁移期
切两套默认值,此侧只有 M3 一套);`Typography`/`TextTheme`/`IconThemeData`
随文本与图标簇(E5);**组件主题四十五件随各自的控件簇落地**——没有控件可配的
组件主题是没人读的数据类。

**widgets 层入账轮:restoration 与手势探测器(2026-08-19)。**

- **restoration 全家 25 类挂引擎账**——PORTING_PLAN 门控表早已判过
  ("引擎与框架两侧皆零"),但那 25 个类一直躺在工作队列里冒充待办:
  `RestorationScope`/`RestorationMixin`/`RestorableProperty` 与十八个
  `Restorable*` 值类型,加 services 侧的 `RestorationManager`/`RestorationBucket`。
  按门控表归位。
- **gesture_detector.dart 5 类**——`GestureDetector`/`RawGestureDetector`/
  `RawGestureDetectorState`≙`RenderPointerRegion`+`PointerHandlers`
  (此侧手势探测器就是"带 handlers 的命中区域");
  `GestureRecognizerFactory`(及 WithHandlers)≙`PointerHandlers` 的 `with_*`
  构造链——识别器不是对象,装配也就不需要工厂。余 `SemanticsGestureDelegate` 属 E3。
- **system_context_menu.dart 整文件出范围**——`SystemContextMenu.isSupported`
  就是 `defaultTargetPlatform == iOS`,十一个类全是 iOS 系统菜单项
  (services 侧的对应十件上一轮已出范围)。

**全量过半:934/1873(50%)。**

**P8-M1 起步:颜色地基(2026-08-19)。** material 层的第一块:

- **`color_scheme.rs`**——`ColorScheme` 全角色。只有九个角色是"直接给的"
  (brightness 加 primary/secondary/error/surface 四对及其 on 色),其余四十
  个都各有回退,回退表逐条照抄上游的 getter:`tertiary`→`secondary`、
  每个 surface container→`surface`、`outline`→`onBackground`(而它自己
  →`onSurface`)、`shadow`/`scrim`→黑、`surfaceTint`→`primary`。
  **回退角色存 `Option` 而不是一次算死**,因为上游 `copyWith` 把未设的透传:
  改了 primary 而从没设过 primaryContainer 的方案,容器要跟着新的 primary 走。
  `light()`/`dark()` 是上游的 M2 基线两式,`lerp` 先解析两端再逐角色插值
  (与上游一致)。
- **`colors.rs`**——`MaterialColor`/`MaterialAccentColor`/`Colors`,
  35 个色板 + 17 个纯色常量,**由脚本解析上游 colors.dart 生成**而非手抄
  (两千行常量手抄必有一位数字是错的)。`grey` 有 12 档(多出 350 与 850,
  上游注明是"浅色主题下 raised button 按下时"用的),其余十档,accent 四档。

**记录在案的分歧**:

- `ColorScheme.fromSeed`/`fromImageProvider` 未做——两者都要跑 M3 的色调
  调色板算法(HCT/CAM16,上游 vendored 的 `material_color_utilities`),
  自成一摊工作;在它落地前,配色方案逐角色写出来,即 `light()`/`dark()` 的做法。
- `background`/`onBackground`/`surfaceVariant` 上游已弃用(让位给 `surface`/
  `onSurface`/`surfaceContainerHighest`),此侧保留——上游也保留,且 `outline`
  的回退仍要穿过 `onBackground`。
- 上游色板是 `ColorSwatch<int>` 且可 `[]` 索引,此侧 `shade(weight)` 收权重
  答 `Option`,把"没这一档"放进类型里。

**services 层入账轮(2026-08-19)。** 24/141 → 85/141,逐条对着 `services/`
五个文件核实:

- **通道与编解码**——`BinaryMessenger`≙`services::{send, send_with_reply,
  set_handler, handle_platform_message}`(上游 messenger 是对象,此侧是模块
  函数,两端只经它)、`ServicesBinding`≙services/mod.rs + app.rs 的平台消息
  入口、`PlatformException`≙`MethodError`、`MissingPluginException`≙
  `MethodResult::NotImplemented`(上游抛异常,此侧是结果的一个变体)、
  `JSONMessageCodec`/`JSONMethodCodec`≙`JsonMessageCodec`/`JsonMethodCodec`
  (**仅大小写不同**,尺子按名字比对认不出)、`OptionalMethodChannel`≙
  `MethodChannel`(此侧"插件不在"本就是一个结果而不是异常,不需要第二种通道)。
- **键盘**——`KeyDownEvent`/`KeyUpEvent`/`KeyRepeatEvent`≙`KeyEvent`+
  `KeyChange` 三变体(同指针事件的做法)、`HardwareKeyboard`≙`Keyboard`、
  `KeyEventManager`≙`Keyboard`+`focus::dispatch_key`、
  `KeyboardKey`/`LogicalKeyboardKey`/`PhysicalKeyboardKey`≙`LogicalKey`/
  `PhysicalKey`。
- **光标/剪贴板/选区**——`MouseCursor`/`SystemMouseCursors`≙
  `SystemMouseCursor`、`ClipboardData`≙`Clipboard` 的 String 收发、
  `TextSelection`≙`TextEditingValue` 的 selection_base/extent(此侧选区不是
  独立类型,是编辑值里的两个 UTF-16 下标)。
- **整族出范围**——`raw_keyboard*.dart` 八个文件(上游 v3.18 起整族
  `@Deprecated`,被 HardwareKeyboard/KeyEvent 取代,此侧只移植新的一路)、
  iOS 系统菜单十件与 `LiveText`(iOS 专属)、`BrowserContextMenu`(web-only)。
- **platform_views 14 类**挂引擎账(同 `PlatformViewSurface` 既有账)。

**余 56 类**多是各自成立的面:autofill 5、asset 5(含 `FontLoader`,属 E4)、
spell_check 4、text_editing_delta 5、text_formatter 3、text_boundary 5、
process_text 3、undo_manager 2、restoration 2(门控表挂账候选)、
`MouseCursorManager`/`MouseCursorSession`(**光标应答的框架侧——通道已在
`SystemMouseCursor::activate`,缺的是"指针下是谁的光标"的追踪层**,同门控表)。

**P4:focus 遍历完整化(2026-08-19)。** focus_traversal.dart 16/19:
`FocusOrder`(Numeric/Lexical 两式)、`NumericFocusOrder`/`LexicalFocusOrder`、
`FocusTraversalOrder`(经 provide 发布,下面的 `Focus` 在自己的 build 里取——
与上游 `InheritedNotifier` 同一机制)、`OrderedTraversalPolicy`(有序者在前、
其余保持注册序;稳定排序使"其余"恰是 `WidgetOrderTraversalPolicy`,即它的
secondary)、`FocusTraversalGroup`;actions.rs 补 `Intent::RequestFocus/
NextFocus/PreviousFocus` 与 `RequestFocusAction`/`NextFocusAction`/
`PreviousFocusAction`(后两者 `consumesKey` 为假——无处可去时该把键让给宿主,
同上游)。

**遍历序改成分组递归**,即上游 `_sortAllDescendants` 在此侧注册表上的形态:
每个组内部各自排序,组节点在父组的名单里**代表它的整个子树**站位。这是关键
——第一版按"组成员在注册表里的位置"分桶,而组件是惰性构建的,深处的组成员
注册在浅处的兄弟之后,于是分桶把组甩到了最后。改成递归展开后,
`1 / [组:20,21] / 2` 走出来就是 1、20、21、2,与上游一致。

`ExcludeFocusTraversal`≙`Focus::with_traversable(false)` 入账(子树不再是 Tab
落点,仍可点击聚焦——同上游)。

**余 3 类是方向遍历一族**(`DirectionalFocusTraversalPolicyMixin`/
`DirectionalFocusIntent`/`DirectionalFocusAction`),**缺的是几何**:上游按
`node.rect` 在方向上找最近的候选,而此侧焦点登记表只有 id、祖先链与顺序,
不含布局后的矩形。要它得让 `FocusEntry` 持有渲染对象并在布局后读取——单独
立项,不在本簇里猜。

**scroll 家族入账轮(2026-08-19)。** 同样是纯判定,逐条对着 `scrolling.rs` /
`physics.rs` 核实:

- **活动族九件**——上游的 `ScrollActivity` 对象在此侧是 `Scroll` 自己的一个
  字段:`Option<Activity>` 加 `Motion` 两式。`IdleScrollActivity`≙`None`、
  `BallisticScrollActivity`≙`Motion::Ballistic`、`DrivenScrollActivity`≙
  `Motion::Driven`(即上游 `_InterpolationSimulation`)、
  `ScrollActivityDelegate`≙`Scroll` 自身(活动直接改它的 offset)、
  `ScrollHoldController`/`HoldScrollActivity`≙`Scroll::stop()`(按下即停飞行;
  停下这件事本身就是 hold,所以它报 end 而不是新活动——既有回归线里写着)、
  `ScrollDragController`/`DragScrollActivity`≙手势的 `on_drag_update` 直连
  `scroll_by`。
- **controller/position 四件**——此侧一个 `Scroll` 即上游
  controller+position+ 单 context 合一;`FixedScrollMetrics`≙`ScrollMetrics`
  (本就是快照值类型)。
- **通知六件**——五个具体通知≙`ScrollNotification` 五变体,
  `ViewportNotificationMixin`≙各变体的 `depth` 字段。`ViewportElementMixin`
  仍缺——它是给 depth **加一**的那一侧,此侧没有视口元素来加,字段现在恒零
  (代码里已自陈是"待接的缝")。
- **物理两件**——`ScrollPhysics`≙physics.rs 的 Simulation 族 + `Scroll` 的
  边界钳制,`ClampingScrollPhysics`≙`ClampingScrollSimulation`;弹跳一族
  (`BouncingScrollPhysics`/`BouncingScrollSimulation`/`RangeMaintaining`/
  `AlwaysScrollable`/`NeverScrollable`)属 **E7**,未入账。
- **委托三件**——`SliverChildDelegate`/`ChildBuilderDelegate`/`ChildListDelegate`
  ≙`SliverList` 的 builder 闭包与 `SliverList::list`(上游委托对象的全部职责
  就是"按索引造孩子")。

**层进度(本轮末)**:painting 100%、animation 100%、rendering 88%、
gestures 64%、scheduler 57%、widgets 41%、cupertino 21%、services 17%、
foundation 11%、material 9%;**全量 795/1868(42%)**。

**gestures 层入账轮(2026-08-19)。** 纯判定,不动码——逐类核实后
5/90 → 58/90:

| 块 | 判定 |
| --- | --- |
| **events.dart 16 类** | 上游的 `PointerEvent` 类层级在此侧是一个结构体 + 两个 code 枚举:`PointerChange` 十一变体与上游子类一一对应(Add/Remove/Hover/Down/Move/Up/Cancel/PanZoomStart/Update/End),`SignalKind` 四态对 Signal/Scroll/ScrollInertiaCancel/Scale。Enter/Exit 此侧不产独立事件——区域进出由 `GestureRouter::hovered` 比对上一帧算出,回调是 `on_hover_change`(已核) |
| **arena.dart 3 类** | `GestureArenaMember`≙`Member`(识别器+区域索引)、`GestureArenaEntry`≙成员值本身(resolve 按值查)、`GestureArenaManager`≙`GestureArena`(open/close/hold/sweep 与 eager winner 逐条对齐) |
| **details 族 11 类** | drag 四件≙`DragEvent`/`DragEndEvent`、long press 四件≙`TapEvent`、scale 三件≙`ScaleEvent`、tap 三件≙`TapEvent`/`PointerEvent` |
| **recognizer 族** | `GestureRecognizer`/`OneSequence`/`PrimaryPointer`≙`Recognizer` 枚举 + 路由器每类状态(此侧识别器不是对象而是状态机的一支)、`OffsetPair`≙`position`+`local_position`;Tap/SecondaryTap/DoubleTap/LongPress/Scale/Drag 六件按成员对上 |
| **binding/converter** | `GestureBinding`≙`GestureRouter`+app.rs 指针入口;`PointerEventConverter`≙`rf_dispatch_pointer` 的转换块(按 view 的 DPR 缩放 + from_code) |
| **lsq_solver 2 类** | `LeastSquaresSolver`≙`fit_quadratic`(Gram-Schmidt 解 solve(2);上游的 per-sample 权重其唯一调用方恒传 1,故省)、`PolynomialFit`≙其三系数 |
| **其余** | `PointerRouter`≙`ActivePointer::listeners`(按下时记路径,之后按路径分发)、`Velocity`≙`VelocityEstimate`、`DeviceGestureSettings`≙slop 常量(上游由 view 上报平台 touchSlop,此侧用上游默认值——记录在案)、`NativeHitTestTarget` 挂引擎账、iOS fling tracker 出范围 |

**余 32 类全是 P6 立项内容,不是漏译**:multidrag 6、tap_and_drag 9、multitap 5
(SerialTap 族)、monodrag 的 Vertical/Horizontal 2(此侧 `Drag` 即无轴约束的
pan,按轴竞争未建模——**这是记录在案的简化**,反向嵌套滚动待 P6 补)、
force_press 2(门控表:框架侧可实现,宿主压力数据另立)、eager 1、resampler 2、
team 1、pointer_signal_resolver 1、`Drag` 接口 1、macOS fling tracker 1、
指针分发的错误诊断 1(随 P10 诊断树终裁)。

**P4:sliver widget 层(2026-08-19)。** `sliver.rs`,widgets/sliver.dart 15/16
——P3 落的那批 sliver 渲染对象的 widget 侧名字:`SliverList`(含
`SliverList.list`)/`SliverFixedExtentList`/`SliverGrid`(含 `.count`/`.extent`)/
`SliverOpacity`/`SliverIgnorePointer`/`SliverOffstage`/
`SliverConstrainedCrossAxis`/`SliverCrossAxisExpanded`/`SliverCrossAxisGroup`/
`SliverMainAxisGroup`。

**抓到一个真错:`RenderSliverOffstage` 此前不归零 geometry。** P3 那轮把
Offstage 记成"布局照旧、静默绘制与命中",但上游 `performLayout` 是
`child.layout(...)` 之后 `geometry = SliverGeometry.zero`——offstage 的
sliver **不占滚动距离**,它后面的 slivers 要顶上来。原样透传子几何的话,
一个看不见的 sliver 仍把兄弟往下推,正是盒版 `RenderOffstage` 会犯而没犯的
那个错。已改,render.rs 的对应测试同步改期望。

**入账判定**:`SliverWithKeepAliveWidget`/`KeepAlive`≙"离窗即弃"(同
`AutomaticKeepAlive`/`KeepAliveParentDataMixin` 既有账,E6 立项时一并落);
`SliverMultiBoxAdaptorWidget`/`SliverMultiBoxAdaptorElement`≙渲染对象直接收
builder——上游这两件的全部内容是 widget↔element↔render 的 child-manager 关系,
此侧管理器就是渲染对象本身;`SliverVariedExtentList`≙`SliverList` 的逐子量测
路径(上游那件是"不量测也知道 extent"的优化,此侧窗口内量测、窗口外估算,
与 `RenderSliverList` 既有的估算分歧同源)。余 `SliverEnsureSemantics` 属 E3。

**P4:widget_state 体系(2026-08-19)。** `widget_state.rs`,widget_state.dart
10/10:`WidgetState` 八态、`WidgetStates`(上游 `Set<WidgetState>`,此侧一个
u8 位集——`Copy` 且可比,控件"上帧状态 vs 本帧状态"的重绘判据要的正是这个)、
`WidgetStatesConstraint`(上游是 mixin 加 `&`/`|`/`~` 三个运算符,此侧是那
三个运算符构出的封闭形状,叶子是 `WidgetState`,`ANY` 即 `_AnyWidgetStates`)、
`WidgetStateProperty` trait + `WidgetStatePropertyWith`(resolveWith)/
`WidgetStatePropertyAll`/`WidgetStateMapper`(首个满足的臂胜出——这正是上游
每份主题都把 disabled 臂写在 hover 臂之上的原因)、`lerp_properties`
(`_LerpProperties`)、五个类型化属性(`WidgetStateColor`/`WidgetStateMouseCursor`/
`WidgetStateBorderSide`/`WidgetStateOutlinedBorder`/`WidgetStateTextStyle`)、
`WidgetStatesController`。

**这是 P8 material 的地基**:上游每个 Material 控件的颜色/边/文字样式/光标
都从 `WidgetStateProperty` 里取,主题里存的也是它而不是裸值。

**记录在案的分歧**:

- 上游的类型化属性*既是*属性*也是*值类型(`WidgetStateColor extends Color`),
  所以未解析的值能塞进任何收裸 `Color` 的地方,事后由 `resolveAs` 解析。
  Rust 无继承,各自成型,由调用方在用处解析;`MaybeStateful<T>` + `resolve_as`
  是"裸值或属性"那一档的显式写法。
- `WidgetStateMapper` 无匹配臂时,上游对可空类型答 null、对非空类型抛;此侧
  mapper 一律答 `Option<T>`,类型化属性各自带一个 fallback——上游在运行时报
  的那个错,这里是类型系统提前问的一个问题。
- `WidgetStateMouseCursor` 解析出 `SystemMouseCursor`(services/system.rs 既有);
  上游的 `MouseCursor` 抽象基类无对应物,宿主侧应答仍按门控表挂账。

**P5:tick 源(2026-08-19)。** `ticker.rs`,上游 `scheduler/ticker.dart`
4 类全落地 + `widgets/ticker_provider.dart` 4 类(两个 mixin 记 mapped):
`Ticker`(start/stop/muted/isActive/isTicking/absorbTicker;`_tick` 的
"首帧才定 `_startTime`",所以回调看到的 elapsed 从零起,不管这帧隔了多久
才来)、`TickerFuture`+`TickerCanceled`、`TickerProvider` trait、
`TickerMode`+`TickerModeData`、`SingleTicker`≙`SingleTickerProviderStateMixin`、
`Tickers`≙`TickerProviderStateMixin`。

**这补上了 `AnimationController` 的上一条账**:此前控制器要靠持有方手调
`tick(delta)`。`with_vsync(provider)` 即上游 `AnimationController(vsync:)`
——控制器自己造 ticker、`forward`/`reverse`/`restart` 起它、`stop` 停它,
落地那帧回调里自停(`Controller::tick` 在落地帧返回 true——那正是要画终值
的一帧——所以判据是 `is_running()` 而不是 tick 的返回值)。回调持弱引用回
控制器,所以"控制器拥有 ticker、ticker 回调控制器"不成环。上游回调给的是
自起始以来的累计时长,`Controller::tick` 收的是逐帧步长,差分在桥里做。

**记录在案的分歧**:

- `TickerFuture` 不是 future(当时 crate 无执行器,同 `async.rs` 的账;
  2026-08-23 起有了,见本文件顶部的执行器一节——`settled()` 是后加的门面,
  下面这套三态加回调仍是它的底),
  是它会落到的三态加回调;`whenCompleteOrCancel` 同名同义,`orCancel` 的
  两种结局之分即回调收到的那个 bool。
- `Ticker.forceFrames` 与 scheduler 的 phase 无处可去:此侧"要一帧"就是
  `advance` 返回 true,不在帧里时也没有 phase 可言。scheduler 层的
  `SchedulerBinding`/`Priority`/`PerformanceModeRequestHandle` 仍未入账
  (帧序在 app.rs,终裁随 P10)。
- 上游两个 mixin 的分工是"省一次分配 + 一条断言",此侧是 `Option<Ticker>`
  与 `Vec<Ticker>` 的差别——同样的省法,断言长在类型里。

**P4:basic.dart 余量(2026-08-19)。** widgets 层最大的单个缺口:72 类里 40
未记账,大半是 P2 已经落地的渲染对象没有 widget 侧名字。补上 29 个门面
(widgets.rs,上游顺序):`ShaderMask`/`BackdropFilter`/`CustomPaint`/
`ClipOval`/`ClipRSuperellipse`/`PhysicalModel`/`PhysicalShape`/
`FractionalTranslation`/`RotatedBox`/`CustomSingleChildLayout`/`LayoutId`/
`CustomMultiChildLayout`/`ConstrainedBox`/`ConstraintsTransformBox`/
`UnconstrainedBox`/`Offstage`/`SliverToBoxAdapter`/`SliverPadding`/
`ListBody`/`Flow`/`RichText`/`RawImage`/`Listener`/`MouseRegion`/
`AbsorbPointer`/`MetaData`/`ColoredBox`,以及 framework.rs 的
`KeyedSubtree`(`keyed_subtree` + `ensure_unique_keys_for_list`)与
`StatefulBuilder`(`stateful_builder`)。basic.dart 67/72。

**抓到一个真错:`ConstraintsTransform` 的三个变体不是上游的东西。**
既有实现是 `WidthCapture`(子的 max 宽变成 min)/`HeightCapture`/
`Unconstrained`(只清 minima);上游 `ConstraintsTransformBox` 的静态变换有
七个,没有一个是"max 变 min",而 `unconstrained` 是 `const BoxConstraints()`
——**minima 与 maxima 全清**。只清 minima 的话 `UnconstrainedBox` 恰好不
unconstrain,而那是它唯一的职责。改为上游七式:`unmodified`/`unconstrained`/
`widthUnconstrained`(=`heightConstraints()`)/`heightUnconstrained`/
`maxWidthUnconstrained`/`maxHeightUnconstrained`/`maxUnconstrained`,
`UnconstrainedBox::along(axis)` 即上游 `constrainedAxis` 的 `_axisToTransform`
三分支。原 `width_capture_tightens_the_child` 测试随之改写为两条:
unconstrained 让子溢出父(且溢出不等于可命中——`hit_test` 从自身边界起,
上游同理)、widthUnconstrained 保住高度的上下界。

`ListBody` 的轴按上游 `_getDirection`
(`getAxisDirectionFromAxisReverseAndDirectionality`)算:竖直向下、水平按
阅读序,`reverse` 再翻——rtl 的水平 body 从右端排起。

**入账判定(basic.dart 余下 11 类)**:`IgnoreBaseline`≙代理盒默认基线路径
(同 `RenderIgnoreBaseline` 既有账)、`DefaultAssetBundle`≙`image::root_bundle()`
(此侧 bundle 是全局根,按子树换 bundle 待 E4)、`WidgetToRenderBoxAdapter`≙
`boxed()`/`leaf()`(此侧 widget 本就产出 render object,适配器即恒等);
`BackdropGroup`(引擎无 `BackdropKey`)、`CompositedTransformTarget/Follower`
(引擎无 `LayerLink`,同 Leader/FollowerLayer 既有挂账)记 blocked-engine。

**余 5 类不入账,是波次边界不是漏译**:`MergeSemantics`/`BlockSemantics`/
`ExcludeSemantics`/`IndexedSemantics`/`SliverSemantics` 属 E3 语义 ABI 波
(rendering 侧的语义七件同样在等它),那一波把 `yields_to_a_label` 的常见
情形展开成上游的合并/屏蔽/排除/索引四种标注。

**P4:transitions 全家(2026-08-19)。** `transitions.rs` 落地,
transitions.dart 16/16 入账(唯 `DefaultTextStyleTransition` 仍记 mapped
——框架侧无 `DefaultTextStyle` 组件,随 P7 文本波接线):

- `AnimatedWidget`——上游是 `initState` 里 `addListener(_handleChange)` +
  `setState`;此侧是 `advance` 每帧比对"动画现值/状态"与"上次画的那份",
  不同即请求重建,运行中恒请求下一帧(上游控制器每 tick 都通知,同义)。
- `ListenableBuilder`——`ChangeNotifier` 不是时钟,不能逐帧轮询,所以走真
  订阅:首建时经 `StateHandle` 注册回调,回调即 `set_state(|_| {})`(标脏
  +排帧),订阅句柄存在 state 里随元素消亡。`AnimatedBuilder` 取 `Animation`
  一路(上游两者都收 `Listenable`,只有文档不同)。
- 八个具体过渡:`SlideTransition`(rtl 翻 dx、`transformHitTests`)、
  `MatrixTransition`+`ScaleTransition`(`diagonal3Values`)+`RotationTransition`
  (`rotationZ(v·2π)`)、`SizeTransition`(`math.max(factor, 0)` 下限、
  轴/`alignment`/废弃 `axisAlignment` 的 build 分支、`fixedCrossAxisSizeFactor`)、
  `FadeTransition`/`SliverFadeTransition`、`DecoratedBoxTransition`、
  `AlignTransition`。
- 三个 tween:`RelativeRectTween`、`AlignmentTween`、`AlignmentGeometryTween`
  (后两件是 rendering/tweens.dart 的,顺带清零该文件);`DecorationTween`
  由 mapped 转真码。
- 配套补上的地基:`RelativeRect`(rendering/stack.dart 全套——fromSize/
  fromRect/fromDirectional/shift/inflate/intersect/toRect/toSize/lerp,
  外加 `to_stack_position()` 即上游 `Positioned.fromRelativeRect`)、
  `DecorationPosition`(RenderDecoratedBox 的前景/背景绘制序)、
  `RenderFractionalTranslation.transformHitTests`(此前恒真)、
  `Matrix4::diagonal3_values`、`AnimationController`。

**`AnimationController`:对象图此前没有驱动源。** P5 立的 `Animation` 对象图
里,`CurvedAnimation`/`ProxyAnimation`/`AnimationMean` 都收 `Rc<dyn Animation>`
父,而能当父的只有 `AlwaysStoppedAnimation` 和测试桩——`Controller` 是纯值
时钟,没有 listener 面也不能共享。`AnimationController` 是把它放进共享可变
+ 挂上 listener 的那一层:`value()` 取 curved 值、`status()` 由
running/direction/value 现算(上游存 `_status`,由 `_checkStatusChanged` 写)、
`is_animating()` 覆写为 ticker 活否(上游同样覆写,所以中途 stop 的控制器
status 仍说 forward 而 isAnimating 为假)。tick 仍由持有方在自己的 `advance`
里调——`Ticker`/`TickerProvider` 未移植(widgets/ticker_provider.dart,4 类)。

**记录在案的分歧(随本簇新增)**:

- **child 是闭包不是实例**。上游 `AnimatedWidget` 把拿到的 `child` 原样交回
  每次重建(这正是"把 child 传给 builder 而不是在里面 build"的理由);
  `AnyWidget` 不 `Clone`(内含闭包),所以此侧每帧重建 child 由元素树协调
  ——效果同,省下的那笔省不到。
- **`PositionedTransition`/`RelativePositionedTransition` 是位置,不是 widget**。
  上游它们是 `Positioned`,而 `Positioned` 是外层 `Stack` 读取的
  `ParentDataWidget`;此侧栈直接收孩子的位置(`RenderStack::push_positioned`),
  子 widget 无从标注,所以过渡本身就是那个位置,由外层每次 build 问它要。
  外层本就在逐帧重建(它才是 tick 控制器的那个),这侧不需要 listener。
- `filterQuality` 丢弃(引擎 transform 层无 image filter quality);
  `alwaysIncludeSemantics` 丢弃(`RenderOpacity` 恰在停画时退出语义,无标志可留);
  `MatrixTransition` 的矩阵按 2D 仿射压平(z 行与透视列丢弃,同
  `RenderTransform` 既有的限制——上游那个 Y 轴透视 dartpad 例子此侧画不出)。
- `RelativeRect.lerp` 的上游 `b == null` 分支写的是 `b!.left * k`,唯一能走到
  它的输入上必抛;此侧按对称的本意实现为 `a.left * k`,并在文档里点名。

**P4:widgets 基座台账(2026-08-18)。** ~40 类判定入账:builder 族
(component/animated 即本体)、DecoratedBox/Banner/Feedback/FlutterLogo 等门面
(渲染对象与系统服务已在)、focus 遍历与 intent(focus.rs 既有)、
DefaultTextStyle(Provider 环境)、LayoutBuilder 家族(RenderSizeReporter
记录在案替身)、DisposableBuildContext(整树重装配无挂卸)、
AutomaticKeepAlive(离窗即弃记录分歧)。widgets 层 198/722。

**P4:async + implicit_animations(2026-08-18)。** `async.rs`:`ConnectionState`
四态、`AsyncSnapshot<T>`(nothing/waiting/withData/withError/inState/
hasData/hasError)、`async_builder`(poll 形态——当时 crate 无执行器,future 的
驱动方持有所有权,帧轮询;快照与 builder 契约同上游,分歧记录)。async.dart
4/4。implicit_animations.dart 24/24 入账:基类三件≙`Animated<T>`/
`AnimatedState<T>` 门面,tween 十件≙各类型的 Lerp/算术(BoxBorder::lerp、
BorderRadius ±×、EdgeInsetsGeometry add/scale 均既有),具体十三件≙
`animated()` 门面(相册渐隐已是此路)。

**P4:shortcuts 体系(2026-08-18)。** `shortcuts.rs` 落地:`LogicalKeySet`
(无序集)、`ShortcutActivator` 枚举三式(KeySet——事件键在集内+全集按住+无
他键;Single——一键+四修饰键精确;Character——字符+control+单修饰)、
`ShortcutRegistry`(registry+manager 并一,插入序首中;`dispatch` 直连
ActionDispatcher)、`CallbackShortcuts`(最小拼写)。测试抓到并修正一处真
错:KeySet 匹配缺"事件自身的键在集内"半边——未注册键按住已注册集时会误
中。shortcuts.dart 12/12 入账。`Keyboard::record` 提为 pub(测试按真实
路径按 key)。

**P4 开工:intent/action 体系(2026-08-18)。** `actions.rs` 落地:`Intent`
封闭枚举(上游具体 intent 即变体,含 Prioritized{intents})、`Action`
(on_invoke/is_enabled/consumes_key 三位,toKeyEventResult 的 consumesKey 分
支)、`ActionDispatcher`(invoke_action/maybe_invoke;first_enabled 对
Prioritized 逐个试到启用;禁用动作跳过、无启用则键继续传播)。
actions.dart 22 类全入账(ActionListener/FocusableActionDetector 记 P4 余量
——action 生命周期监听与 focus+hover 合成检测器,首个消费者出现时落码)。
element 族(framework.dart 22 类)同轮判定入账:三 widget 基类≙三 trait、
element 子类≙ElementTree 的 WidgetKind 分支与装配路径(MultiChild 的六步
即既有回归线)、BuildOwner/BuildScope≙整树重装配形态、ErrorWidget≙
ErrorPlaceholder。

**animation 层清零(47/47,2026-08-18)。** curves 族收尾:命名曲线≙`Curve`
枚举变体(Ease=Eubic、Elastic*、Bounce*、Decelerate),`FlippedCurve`≙
`flipped()`,`ParametricCurve`≙`transform`,`Interval`≙权重段的局部 t 映射,
`Threshold`≙StepTween 两步,`SawTooth`≙Repeat 取余;CatmullRom/2D/Split 五件
无消费者,记 P5 余量(首个使用者出现时落码)。

**P5 动画对象图(2026-08-18)。** `Animation` trait(value/status/listener 四
面)+`AnimationStatus` 四态与 isDismissed/isCompleted/isAnimating、
`AnimationListeners` 共用簿记(懒/急/本地值/本地状态四个 mixin 并一)、
`ProxyAnimation`(set_parent 跨换公告状态)、`ReverseAnimation`(1-t+状态对翻)、
`CurvedAnimation`(端点 clamp+reverseCurve)、`AnimationMean/Max/Min`、
`AlwaysStoppedAnimation`;`Animatable` trait+`ChainedAnimatable::evaluate`
(inner→outer);`CurveTween/ReverseTween/StepTween/IntTween/SizeTween/
RectTween/ConstantTween`;`TweenSequence`(权重分段+局部 t)/
`FlippedTweenSequence`/`TweenSequenceItem`(tween/gap);`AnimationStyle`
(at_most 字段回退)。`Controller` 即 AnimationController 的既有形态。
TrainHoppingAnimation 留待首个消费者(曲线可经 ProxyAnimation 直换)。
**persistent header 的 snap 与渐变 lerp 两个记录分歧自此有对象图可关**——
改挂账为待接线。

**rendering 层收账(2026-08-18):211/243 入账。** 本轮判定入账的大块:
`object.dart` 协议族 12 类(RenderObject≙RenderBox trait、ParentData 三件≙容器
持有、PaintingContext≙PaintContext、PipelineOwner/Manifold≙帧序+脏列表——
即此前"这张表原先各项都已补上"的等价物)、`layer.dart` 24 类(引擎 layer
push ABI+offset/clip/transform/opacity/blur 逐类映射,留存层 Rc≙LayerHandle;
Texture/PlatformView/Follower 三件挂引擎账;PerformanceOverlay 同前)、
`binding.dart` 二件≙app.rs 帧序、`box.dart` 命中双件+ParentData 混入、
`flex/stack/wrap` 的 ParentData 三件、`view.dart` 二件、`mouse_tracker` ≙
gestures 逐设备 hover、`tweens` 三件(painting 的 Alignment lerp 已在)、
`decorated_sliver` ≙ adapter 包装饰。**余 32 类全是计划内波次**:selection 17
(P7)+语义七件(E3)+list_wheel 三件(P6)+editable 三件(P7)+tree 三件+debug
二件(P10)+RevealedOffset(P4)。

**animated_size(2026-08-18)。** `RenderAnimatedSize` 落地:四态状态机
(Start→Stable;Stable 中子变尺寸→Changed 重启动画;Changed 中再变→Unstable
追踪;Unstable 稳住→stop 回 Stable——逐符号照抄)、tight 即快照并停、
tween=lerp(begin,end,controller.value)、溢出时硬边裁剪。Controller 无
default 构造(显式 200ms 缺省),tick 由外部喂——同 crate 所有动画的驱动
形态。

**custom_layout 三类(2026-08-18)。** `MultiChildLayoutDelegate` trait
(getSize 缺省+performLayout(Size, &mut Context)+shouldRelayout+kind_id)、
`MultiChildLayoutContext`(hasChild/layoutChild/positionChild 按 id;一次性纪律
以数据承载——上游是 debug 断言,缺失 id 答 None 而非抛)、
`RenderCustomMultiChildLayoutBox`(逐符号,命中从后声明优先)。

**shifted_box 六类(2026-08-18)。** `SingleChildLayoutDelegate` trait(getSize/
getConstraintsForChild/getPositionForChild 三问缺省+shouldRelayout+kind_id)+
`RenderCustomSingleChildLayoutBox`(逐符号:尺寸取 delegate 收紧、子位随
getPositionForChild、基线加子位移);`RenderConstraintsTransformBox`
(widthCapture/heightCapture/unconstrained 三式枚举+对齐位移);
`RenderFractionallySizedOverflowBox`(因子收紧 `_getInnerConstraints`、对齐、
允许溢出)。基类二件记 mapped(无基类形态)。

**viewport 收缩包裹(2026-08-18)。** `RenderSliverViewport::with_shrink_wrap`
即上游 `RenderShrinkWrappingViewport`:同一 attempt/correct 循环,尺寸取
`_shrinkWrapExtent`(slivers 绘制实际到达处)而非约束;先给临时 biggest 尺寸
再收缩(slivers 的 main_axis_extent 读它)。顺带补了三处 geometry 生产端的
`layoutExtent = paintExtent` 缺省(FillRemaining/FillViewport/Grid——上游构造
器的缺省语义;persistent header 显式设值不受影响)。`RenderViewportBase`/
`ViewportOffset`/`ScrollCacheExtent` 记 mapped。

**P3 sliver:除 sliver_tree 外全部入账(2026-08-18)。** 协议族
(`sliver.dart` 的 RenderSliver/HitTest 双件/ParentData 双件/Helpers、
`sliver_padding` 的 directional 变体、`RenderSliverSingleBoxAdapter`)记
mapped——本 crate 以 RenderBox 的 trait 方法承载 sliver 协议,基类职责并入
具体形,是记录在案的形态而非漏译。`sliver_tree`(3 类,树形懒列表)待立项。

**P3 sliver 开工(2026-08-18)。** `sliver_persistent_header.dart` 八类已入账:
`OverScrollHeaderStretchConfiguration`/`PersistentHeaderShowOnScreenConfiguration`/
`FloatingHeaderSnapConfiguration`(数据)、基类 `layoutChild`(shrink=
min(scroll,max),子约束 max(maxExtent−shrink, minExtent),顶部 overlap 拉伸)+
四行为(Scrolling:updateGeometry 的 paintExtent=maxExtent−scroll 与子位
min(0,paint−child);Pinned:paintOrigin=overlap、paint=min(child,
remaining−overlap)、maxScrollObstructionExtent=minExtent;Floating:
effectiveScrollOffset 的 reveal 状态机逐字照抄——forward 方向允许展开、
reverse 将 delta 归零、结果 clamp [0, scrollOffset];FloatingPinned 再钉
子位于 0)。分歧:snap 动画只落决策与目标,补间待 P5 的 Animation<T>。

 `sliver_grid.dart` 八类已入账:
`SliverGridGeometry`(trailing/getBoxConstraints)、`SliverGridRegularTileLayout`
(min/maxChildIndexForScrollOffset 的整除算式、reverseCrossAxis 的镜像、
computeMaxScrollOffset 去尾行 spacing)、`SliverGridDelegate` 枚举两式
(FixedCrossAxisCount 的 usable/count 均分与 MaxCrossAxisExtent 的
ceil 取列,childAspectRatio 或显式 mainAxisExtent)、`RenderSliverGrid`
(窗口=layout 的两 index 查询,子按 tile 的 tight cross/main 约束,跨帧保身份)
+2 mapped(抽象基类与 parentData)。

 `RenderSliverFillViewport` 已落地(PageView 的
引擎):itemExtent=viewport×fraction 的纯算术窗口(ceil 边界——恰在窗口末端
起头的下一页不物化),子按 tight extent 布置、存活窗口跨帧保身份由
update_from 重配置,scroll_extent=count×extent,paint=min(total−scrollOffset,
remaining)。`sliver_fill.dart` 至此四类全入账。

 `sliver_group.dart` 两类已落地:
`RenderSliverCrossAxisGroup`(两遍式:定宽子先按自答扣减 cross 余量,flex 子
按份分余,逐子横移 paint offset,组取最长子;子答一律取 sliver_layout 返回
值留存——sliver_geometry 未被普遍覆写)与 `RenderSliverMainAxisGroup`
(逐子 scroll_offset=组偏移+前行占量、余量按切入差钳;收尾
paint=min(scroll_extent−scrollOffset, remaining) 的上游闭式)。

 `proxy_sliver.dart` 六类已落地
(`RenderProxySliver{behavior}`:PassThrough 透传几何/绘制/命中;
Opacity 的 save-layer 组子与零透明不绘不命中;IgnorePointer/Offstage 的
"布局照旧、静默命中/绘制";ConstrainedCrossAxis 的子取
min(maxExtent, incoming) 且 geometry 同报)。`RenderSliverSemanticsAnnotations`
随语义波。

 `sliver_fill.dart` 的 FillRemaining 三式并
作 `RenderSliverFillRemaining{mode}`(Scrollable:子得余量可滚、sliver 占整视口;
Fill:余量定尺寸,滚过后退回子自身固有量;AndOverscroll:先松后紧拉伸进过冲)
——逐 variant 对齐 `performLayout`;`sliver_fixed_extent_list`/`multi_box_adaptor`
族记 mapped(定长窗口算术已在 `RenderSliverList::with_item_extent`,管理器即
count+builder,keepAlive 维持离窗即弃的记录分歧)。`RenderSliverFillViewport`
(PageView 的引擎)待补。

**P2 rendering 盒族开工(2026-08-18)。** TableBorder 已落地(`borders.rs`):
六边(isUniform 含内外全部六边、dimensions 取外四边)、`_paintTableBorder`
三式(均匀圆角走双 rrect、单色多宽圆角走逐边 inset/outset 双 rrect、其余
paintBorder 四梯形)、`paint` 的内网格先行(行线/列线各自 stroke)、scale/
lerp 逐边。DataTable 的地基齐了。

 Flow 已落地:`FlowDelegate` trait
(getSize/getConstraintsForChild 的缺省、paintChildren、shouldRelayout/
shouldRepaint、kind_id+as_any 顶 runtimeType)+`FlowPaintingContext`
(容器尺寸/逐子尺寸/paintChild 的单次约束与变换+不透明度——分数不透明度走
save-layer)+`RenderFlow`(容器由 delegate 定尺寸而子由逐子约束定、
update_from 即上游 setter 的 relayout-else-repaint 判断、命中按绘制序倒走
并以逆变换映射;变换经 thread-local 交接——paint 借用期进不了 self)。

 Table 已落地:`TableColumnWidth` 枚举
(Intrinsic/Fixed/Fraction/Flex/Max/Min 的 min/maxIntrinsic 与 flex 逐条照抄)+
`RenderTable`(`_computeColumnWidths` 全算法:逐列 min/max、flex 列按余量分配到
目标宽、无 flex 时的等分补差、超宽时先收缩 flex 列至其最小值再全员均摊的
双循环;行两遍:量高/收基线(无基线格子按内容高兜底——上游在 debug 断言处较
真,此处记宽和)、再摆位,fill 格子按行高重排;RTL 列序镜像;命中从后声明
优先)。`TableCellParentData` 记 mapped(容器持有 offsets)。`TableBorder` 待
borders 后续补(六边绘制)。

 proxy_box 视觉族已落地:
`RenderClipRRect`/`RenderClipOval`(不透明裁剪命中按 rrect/椭圆)、
`RenderCustomClipPath`+`CustomClipper` trait+`ShapeBorderClipper`(
shouldReclip 同形状判)、`RenderClipRSuperellipse`(连续角路径,同
superellipse 分歧)、`RenderShaderMask`(shader 回调产 Paint,save-layer
合成)、`RenderBackdropFilter`(引擎 backdrop 位只有 blur——分歧)、
`RenderOffstage`(布局不占位/不绘/不命中,固有量与基线全归零)、
`RenderAbsorbPointer`(吸收态整框命中且不留 entry)、
`RenderFractionalTranslation`(绘制位移与命中同移)、
`RenderPhysicalModel`/`RenderPhysicalShape`(elevation 阴影表+形裁剪+命中
按形)、`RenderMetaData`(payload 随 hit entry 走)。RenderProxyBox 基类族/
AnimatedOpacity 记 mapped;Leader/FollowerLayer 挂引擎账(LayerLink);
semantics 五件与 AnnotatedRegion 留给语义波。 已落地:`CustomPainter` trait
(paint/shouldRepaint/shouldRebuildSemantics 默认走 shouldRepaint/hitTest 的
Option<bool> 三态/repaint listenable 位;`as_any` 顶 runtimeType 比较)+
`RenderCustomPaint`(painter→child→foregroundPainer 绘制序、preferredSize
无子定尺寸、`_didUpdatePainter` 的换画判断——None/None 非变更;
`CustomPainterSemantics` 待语义波)。`RenderRotatedBox`(奇数转轴互换、
constraints.flipped、绘制变换与命中逆变换)。`RenderListBody`(四方向端到端
排布、cross 轴 tightFor、up/left 的从尾回填)。

**foundation 响应式地基已落地(E8):`foundation.rs`。**
`Listenable`/`ListenableMerge`(merge 的监听转发全子)/`ChangeNotifier`
(notify 的重入保持、通知中增删监听的次序稳定)/`ValueNotifier`(同值不
告知)/键族构造器(`keys::value/object/unique`——crate 的 `Key` 即
`Option<u64>`,三种语义各给一段数;`LocalKey`/`ValueListenable`/
`LabeledGlobalKey`/`GlobalObjectKey` 记 mapped)。E8 至此除 DiagnosticsNode
族(P10 终裁)外完成。

**painting 层清零(87/87 入账,2026-08-18)。** 收尾簇:`TextPainter`
(`shape_rich` 的对象形态:layout/width/height/longest_line/paint;
minIntrinsicWidth 引擎只有 longest_line 可答——分歧)、`StrutStyle`(携带
配置,引擎 paragraph 无 strut 位)、`PlaceholderDimensions`/
`PlaceholderAlignment`/`TextBaseline`/`WordBoundary`/`Accumulator`/
`InlineSpanSemanticsInformation`、`DecorationImage`(fit+alignment 绘制,
fit 缺省 ScaleDown 同上游)、`Matrix4`+`matrix_utils`(translation/
scale 提取、transformPoint/transformRect 的快慢双路、inverseTransformRect、
圆柱投影、forceToPoint)、`FlutterLogoDecoration`(三式样,mark 的 SVG
坐标照抄,45° 方块与缩放烘焙进点坐标;label 用段落宽度替代
getBoxesForSelection——引擎不回读字形框)。`ShaderWarmUp` 挂引擎账。

**图片管线簇已落地:`image.rs`。** `image_provider.dart`/
`image_stream.dart`/`image_cache.dart`/`image_resolution.dart`:
`ImageProvider` 枚举(Memory/Asset/File/Network/Resize,上游子类族并为
一枚举)、`ImageStream`/`ImageStreamCompleter`(先听后告知、后听即得;
error 路径)、`ImageInfo`/`ImageConfiguration`/`ImageStreamListener`/
`ImageChunkEvent`、`ImageCache::evict/statusForKey` 的 `image_cache_evict`/
`image_cache_status`、`AssetBundle` trait + root bundle(E4 的种子)、
`NetworkImageLoadException`。resolve 走 worker 池、resolve_now 同步解码
(无头渲染/金测试路径)。分歧:引擎只出一帧(单/多帧 completer 合并,动画
图持首帧);decode ABI 无 resize 位(`ResizeImage` 携带目标不生效);
`NetworkImage` 的 bytes 由调用方 fetch 回调供(crate 无 HTTP 栈)。

**渐变簇已落地:`painting.rs`。** `gradient.dart` 的
`LinearGradient`/`RadialGradient`/`SweepGradient`(implied stops、scale 的 alpha
缩放、lerp 的 stops 并集+双坡采样 `_interpolateColorsAndStops` 全套)与
`GradientTransform`/`GradientRotation`(枚举并一个;`Affine` 2D 仿射,旋转平移式照
抄)。shader 变换烘进几何:线性映射端点(仿射下精确)、径向平移圆心+缩放半径、扫角
加角度。**此前"gradient lerp 只在半程切换"的分歧就此关闭**——新 `ShaderGradient`
家族逐 stop 插值;旧 `Fill` 路径维持原样待 material 波次统一。分歧余账:
`RadialGradient.focal/focalRadius` 引擎无对应图元,携带未画。

**对齐簇已落地:`render.rs`。** `alignment.dart` 补齐:`Alignment` 的
`alongOffset`/`alongSize`/`withinRect`/`lerp`、`AlignmentGeometry`(枚举
Absolute/Directional/Mixed,add 同类保类、lerp 缺侧从 centre、resolve 的
ltr 加/rtl 减)、`TextAlignVertical`(top/center/bottom)。

**色彩/文本缩放簇已落地:`painting.rs`。** `colors.dart` 的 `HSVColor`/`HSLColor`
(`fromColor`/`toColor` 的 `_getHue`/`_colorFromHue` 全套、lerp 的逐通道+hue 取模——
Dart 的 `%` 恒非负,Rust 侧用 `rem_euclid` 对齐)与 `ColorSwatch`(泛型色表,
`MaterialColor` 族的地基);`text_scaler.dart` 的 `TextScaler`(linear/noScaling,
平台非线性缩放器将来作新变体)。`fractional_offset.dart` 的 `FractionalOffset` 记
mapped——上游它就是 `Alignment` 的别名,`render::Alignment` 即本体。

**decoration 簇已落地:`decoration.rs`(`BoxDecoration`+`Decoration` 枚举)。** 上游
`decoration.dart`/`box_decoration.dart` 逐符号对齐:padding(边框宽度作内缩)/
isComplex/getClipPath(circle→内切椭圆、radius→rrect)/scale(颜色 alpha 缩放、
边框/radius/阴影从零生长)/lerp(shape 半程切换,逐字段插值)/hitTest(circle 用
距离平方比较)/绘制序(shadows→background→border,`_BoxDecorationPainter` 同序);
`BoxShadow` 补 `scale`/`lerp`/`lerpList`(`painting.rs`);`ShapeDecoration` 补
`fromBoxDecoration`(circle→`CircleBorder`、radius→`RoundedRectangleBorder`);
`Decoration.lerp` 的四方尝试与"经 null 半程"缺省。`RenderDecoratedBox::with_decoration`
与 `Container::with_decoration` 接上(装饰在场时绘制与命中走它,padding 走
`decoration.padding()`)。上游开放基类在此侧是封闭枚举
`Decoration{Box,Shape}`;`BoxPainter` 无对应物——直接绘制,记 mapped。

**borders 簇已落地:`borders.rs`(约 5,300 行,56 个测试)。** 上游
`border_radius.dart`、`borders.dart`、`box_border.dart`、`circle_border.dart`、
`oval_border.dart`、`stadium_border.dart`、`rounded_rectangle_border.dart`、
`beveled_rectangle_border.dart`、`continuous_rectangle_border.dart`、
`shape_decoration.dart`、`notched_shapes.dart`、`linear_border.dart`、
`star_border.dart` 十三个文件,逐符号对齐:
`Radius`/`RRect`(含 Skia 比例收缩)/`BorderRadius`±`Directional`+`Mixed`(resolve
的 ltr/rtl 镜像与逐角相加)/`BorderSide`(strokeInset/outset/offset 三式、lerp 的
宽度变负→none 与样式错配走零透明度色)/`Border`+`BorderDirectional`+`BoxBorder`
(含跨型 lerp 的 t<0.5 侧边交接)/`paintBorder` 四梯形/七个具体形状与三个私有过渡形
(`_StadiumToCircleBorder` 等,lerp 的 circularity/rectilinearity 算式照抄)/
`_CompoundBorder` 的 add(边缘合并)与 lerp(逐槽、来者在去者前)/`ShapeDecoration`
(shadows→interior→border 的绘制序)/`LinearBorder`+`LinearBorderEdge`(零到四条
线,alignment/size 的几何与 lerp 的缺失侧取在场对齐)/`StarBorder`(星形/多边形,
`_StarGenerator` 的逐点生成、valley/point rounding、fractional points 的收尾短臂,
以及 lerp 的 circle 双分支与 `_twoPhaseLerp` 的 stadium/circle 三段走位)。上游类层级
在此侧是封闭枚举 `ShapeBorder`,私有过渡类是枚举变体——形态不同,算式逐条对上。

**记录在案的分歧(随本簇新增)**:

- `RoundedSuperellipseBorder` 用 `ContinuousRectangleBorder` 的三次曲线画角——
  引擎没有 `RSuperellipse` 图元。
- `AutomaticNotchedShape` 需要 `Path.combine(PathOperation.difference)`;引擎 ABI
  没有路径布尔运算,先画 host 形,引擎补上再改。`CircularNotchedRectangle`
  (BottomAppBar 常用那个)完整:`arcToPoint` 用 kappa 三次逼近。
- `ShapeDecoration` 无 `image` 字段(`DecorationImage` 是后面的波次);lerp 的
  gradient↔gradient 只在半程切换,不像上游逐 stop 插值。
- `StarBorder` 的两处引擎位差:conic(`path.conicTo`)用 w/3 三次逼近——这里权重
  落在 0..=1,误差一根头发丝;squash/rotation 矩阵不再变换成型的 path,而是烘进
  生成点(仿射移动贝塞尔控制点是精确的)。命中测试用未取整的星形多边形近似
  `Path.contains`(引擎不从 path 读回点)。
- 逐角 `BorderRadius` 已接进 `RenderDecoratedBox`(绘制与命中)、`RenderClipRect`
  (路径裁剪)、`Container` 与 `ClipRRect::rounded/directional`(见上"已完成"条)。

---

### 弹层线 L4 收尾：文本选择手柄/工具条、Magnifier、DragTarget feedback

`PORTING_PLAN_OVERLAY.md` §7 表里最后三个消费者。三个都是「逻辑早已 port、缺一个
活的宿主」，补的东西一模一样：**一个 overlay entry，加一次坐标换算**。

- `selection_host.rs` —— 上游 `SelectionOverlay`。**三个 entry 而不是一个**：
  `hideToolbar` 留着手柄、collapsed 选区只有一个手柄，两件事各自来去，一人一个
  entry 才表达得了。手柄放 overlay 的理由是上游自己的：手柄要画到字段边界**外面**
  ——选区贴着字段左边时，左手柄的盒子越过字段左沿，画在字段里就被裁掉，而裁掉的
  正好是读者要抓的那块。
- `magnifier_host.rs` —— 上游 `MagnifierController` 的 entry 生命周期。位置、
  隐藏阈值、焦点内收全在 `magnifier.rs` 里没动。**放大本身做不了**：放大是带缩放
  的 backdrop 采样，paint bridge 只有一个 backdrop 算子而且是模糊。镜身（尺寸、
  圆角、描边、阴影）画得出来，透过它看到的东西没被放大。
- `drag_feedback.rs` —— 上游 `_DragAvatar`。feedback 跟着指针，目标靠命中测试找。

**三个模块共同的那条线：全局进，overlay 局部出。** 上游 `updateDrag` 同时留两个
偏移，这是最值得写下来的一处：

```dart
_lastOffset = globalPosition - dragStartPoint;              // 全局，回调听这个
_overlayOffset = box.globalToLocal(globalPosition) - dragStartPoint;  // overlay 局部，画这个
```

两者只在 overlay 铺满窗口时相等。用错了，满屏 overlay 上看不出来，任何别的
overlay 上就是一个固定错位——「只在别人机器上复现」的那种。为此补了 L0 缺的那
一半：`RenderRef::global_to_local` 与 `invert_affine`，`local_to_global` 的严格
逆。差分表 34 个容器全都顺带跑了逆向那一程。

**顺手挖出来的两个真缺陷**：

1. `RenderMetaData` 没有上游 `MetaData.behavior`。payload 只能搭命中记录出来，
   记录只在这个盒子被命中时才有——`deferToChild` 之下，包着非命中目标（一个普通
   容器、一层装饰）的 `MetaData` 携带的注解**永远读不到**。上游 `DragTarget` 传
   `translucent` 正是为此。补上字段与 translucent 分支。
2. `invert_affine` 的平移必须**穿过**逆线性部分回推，不是取负。表里 34 个容器全
   是纯平移（这正是 `visit_children` 与 `hit_test_children` 的约定），两种写法在
   平移下同解——换掉仍然全绿，验证过。所以另写了带缩放的直接单测。

**一处不可证伪的防御，明说**：`MagnifierHost` 只记住「显示中」的位置。今天没有
任何测试能因为去掉这个 guard 而变红——唯一会隐藏的是 Cupertino，而 Cupertino 的
placement 永远 `animate: false`。留着，因为那是今天这两个平台的巧合，不是规则。

**`EntryRefresh`（`theatre.rs`）**：宿主把几何写进共享 cell 并不会让屏幕变化——
entry 的组件不脏、不重建、读不到新值。是看着 magnifier 报出正确坐标却什么都不画
才发现的。它还要**每个 entry 存一个 handle**：选择层是三个 entry 一起动，单槽位
写法看着对，实际只唤醒最后 build 的那一个。

**台账**：§11 列的 11 个上游类（`Overlay`…`ScaffoldMessengerState`）本来就是按名
计入的，`coverage_ledger.json` 里从来没有它们的例外条目——所以「改判」这一步是空
的，不是漏了。`applyPaintTransform` / `getTransformTo` / `localToGlobal` 是方法不
是类，coverage.py 从不追踪。

---

### 弹层线 S6：相册六个 demo 页改接框架宿主

`PORTING_PLAN_OVERLAY.md` S6 的出口判据是「每件一个相册 demo 页」。上游相册有
页面的六件全部改接完毕；没有页面的四件为什么不造页面，见计划文件 §9 新增的那
一段。改接过程本身挖出的东西比预期多。

**每个 demo 头部原本都写着自己缺什么，现在那些话逐条删掉了。** 这是 gate 7 的
精神落到相册侧：

| demo | 原话（节选） | 现在 |
| --- | --- | --- |
| `tooltip_demo` | 气泡叠在列的最后一项，注释叫它「the overlay slot without the overlay」 | 一个 `Tooltip`，气泡在 overlay 里，按按钮实测位置摆 |
| `snackbar_demo` | 「the launcher cannot reach that state without a new field on the shared DemoState」 | `Messenger` 持有队列与生命周期，字段删了 |
| `dialog_demo` | `dialog_open` + `mod.rs` 的共享 overlay 槽 + 手搭 `Scrim` | `show_dialog_with` 三样全删 |
| `picker_demo` | 「the stage grows to OVERLAY_HOST_HEIGHT while a picker is open」 | 常量删除，stage 开合都是自己内容那么高 |
| `menu_demo` | 「no way to read a sibling's rect at build time」+ 0/56/112/168 偏移表 | 偏移表删除，`popup_menu_offset` 拿到真锚点 |
| `navigation_drawer` | 指向 `drawer.rs` 的「没有 owner 所以没有动画」 | 246ms 滑入回来了 |

**挖出的三个真问题**（都不是接线错误，是端口本来就缺的东西）：

1. **`SnackBar.persist`**。上游构造函数末尾 `persist = persist ?? action != null`
   ——**带 action 的 bar 不会自动消失**，字段文档写了理由：读者正被要求「做一件
   事」，bar 走了就把那件事带走了。相册 demo 原本四秒关掉两个 bar，包括带 ACTION
   的那个，正好反了。计时器补进 `Messenger`（上游也放在 `ScaffoldMessengerState`
   而不是 widget 上），六个测试，三种改错都能变红。上游的写法是**照样起计时器、
   由回调拒绝**，这里照抄，免得一个后来被改成非 persist 的 bar 白拿四秒。
2. **全屏 dialog 的 barrier 不该能点掉**。上游那个是 `MaterialPageRoute`，另外
   三个才是 `DialogRoute`：一整页没有背景可暗，也不该靠点旁边离开。旧写法一个
   `Scrim` 服务所有变体，表达不了这个区别。
3. **dialog 里的按钮按压高亮该是它自己的**。原本记在共享 `GalleryState` 上——
   dialog 的内容只在弹出时构建、在 entry 重建时才重建，从页面状态读到的高亮会
   冻在那一帧。是个真 bug，不只是整洁问题。

**三处「`fn` 收不下」的成对 API**：`Snackbar::on_action`、
`menu::PopupMenuButton::on_press`，加上原有的 `Switch::with_handlers`。规律一样：
翻转一个具名字段用 `fn` 版最短，要抓 `Messenger`/`PopupMenuOpener`/overlay 的必须
用闭包。上游对应物（`SnackBarAction.onPressed` 等）本来就是任意回调。

**命名归位**：`pub use` 这一步（`PORTING_PLAN.md` 三步接线的第一步）之前十个模块
一个都没做，做了以后撞出三处重名，一律按「名字给对得上上游类的那个」收：
`controls::Tooltip`→`TooltipBubble`、`pickers::show_*_picker`→`*_surface`、
`menu::PopupMenuButton` 不再是导出的那个。每个被改名的东西都在自己文档里写了
原名和改名理由。

---

### 尺子修正与第二次归零（2026-08-21）

上一轮写下「十层全部归零、逐类对齐完成」时，那句话按当时那把尺子是真的。但那把
尺子看不见三个层目录和一个子目录——**它藏了 34 个没 port 的类**。审计尺子本身
才发现的，不是靠再跑一遍它。

**尺子的四个盲区**，每一个都是藏工作而不是美化数字（后者更好发现）：

| 盲区 | 后果 |
| --- | --- |
| `LAYERS` 只列十个，上游 `src/` 下有十三个目录 | physics 9、semantics 24、widget_previews 6 个公共类从未被数 |
| `os.listdir` 不递归 | `material/animated_icons/` 3 个类从未被数 |
| 数 `#[cfg(test)]` 里的声明 | 上游 `Page` 被我自己写的一个测试 struct 顶掉 |
| 类型声明不要求 `pub`（而函数与常量要求） | 私有 helper 替上游公共的 `FocusManager`/`FocusScope`/`State`/`RenderObjectWidget` 顶包 |

`impl` 计分**保留**，而且必须保留：这个 crate 用宏生成整族类型（11 个
`caret_movement_intent!`），尺子不展开宏，手写 `impl` 是唯一证据——实测去掉就全红。
但只认行首的，因为 `root: impl Widget` 是参数位，它一直在替上游的 `Widget` 答到。
四条规则每条都单独量过影响再采纳。

**分母 1888 → 1930，MISSING 0 → 39，再归零。**

补齐的东西（按轮次）：physics 3 → semantics 值类型 6 + `StringAttribute` →
`SemanticsEvent` 族 7 → `SemanticsLabelBuilder` → widget_previews 6 →
`SemanticsBinding`/`SemanticsHandle` → `SemanticsConfiguration` →
`ChildSemanticsConfigurations*` + `Page` → `FocusScope` → `SemanticsData` →
`SemanticsOwner` → animated_icons 3。台账新增 `FocusManager`、`Widget`、`State`
与 5 个 `SemanticsEvent` 子类的改判条目。

---

### 这一程里被测试或 mutation 抓出来的自己的错

按发生顺序，因为它们的共同点比各自的内容更值得记：**每一条都是"讲得通的读法"，
而讲得通不等于上游那么写**。

1. **`AttributedString.concat` 的空串早返回**我写成"承重的、不是优化"。删掉一个，
   测试全绿——空串带不了属性，通用路径算出同样的答案。它就是优化;让它*安全*的是
   那条不变量。改了文档，并把测试从"测那个删掉也不红的捷径"换成"测它依赖的不变量"。
2. **`SemanticsLabelBuilder` 有两行上游代码改变不了结果**（`?? textDirection` 的
   兜底、单段早返回）。保留照抄——会整理源码的 port 没法拿去 diff——但标注它们不
   做事，读者不该自己去推。
3. **`hasConflictingFlags` 我理解反了**。我写的是"两个不同的种类 flag 冲突——一个
   节点不能既是按钮又是文本框"。上游的规则是**同一个 flag 两边都置位**;按钮和文本
   框合并得好好的，拦住不该合并的是另一条 `_hasExplicitRole`，而 `isButton` 有意
   不在那个名单里。是测试红了才去查的源码。
4. **`apply_enabled` 里我多加的一道判断**永远不会错——调用点已经在计数跨 0 时才调。
   这次是删掉而不是加注释：那是我的代码不是上游的。
5. **一个测试没在测它声称的东西**（`removing_one_listener_leaves_the_others...`）：
   移除两个 listener 里的第一个再检查第二个还在，无论移除是留洞还是补位都成立。
   改成三个、按 token 移第三个，移位实现会因 token 越界而做错。
6. **一个 mutation 看起来幸存，其实是工具打偏了**：锚点匹配到另一个函数里一模一样
   的行。此后 mutation 脚本会拒绝不唯一的锚点。
7. **`FocusScope` 的回退测试前提不成立**：`drop(tree)` 不清空焦点注册表，`prune`
   才清。节点还注册着、还持有焦点时，`focus_scope` 正确地无事可做，回退根本没跑。
8. **`PreviewThemeData` 我按印象写成空的 const 类**，实际是 `abstract base class`
   带一个 `apply`；`PreviewLocalizationsData` 还漏了 `localizationsDelegates`。
9. **`SemanticsOwner` 文档里写了一条这边没有对应物的规则**（合并全部后代的节点终止
   遍历）。这边合并发生在收集时，被合并的后代根本不会成为节点——改成写明"没有对应物
   也不需要"。

---

### 值得单独记的上游细节

- **两个 `TextDirection` 枚举顺序相反。** `dart:ui` 先声明 `rtl`（线上 rtl=0），
  这个 crate 先声明 `Ltr`。直接 cast 会让每条无障碍播报方向都反，两边都不报错——
  读屏只会把阿拉伯语从左往右念。查 `sky_engine/lib/ui/text.dart` 证实的。
- **同一个上游文件里两个"带方向标记拼接文字"的函数规则不同。**
  `SemanticsLabelBuilder` 与 `_concatAttributedString` 在分隔符（`" "` vs `"\n"`）、
  何时包裹（要求两边方向已知 vs 只要对方已知且不同）、单独一段（不包 vs 包）三处
  都不一样。拿错一个得到一个被微妙误读的 label。
- **`SemanticsTag` 按对象身份、`CustomSemanticsAction` 按值发 id**，方向相反且都
  对：tag 是给某个祖先标记某个节点的，读起来一样的两个不能撞车;custom action 是
  读屏菜单里的一条，两个节点给出同样 label 就是同一个动作。
- **`AnimatedIcon` 的关键帧是"列表里的位置"而不是"首尾之间的比例"**：三帧在
  progress 0.5 精确给出中间那帧。这才是图标能*穿过*中间形状而不是直线滑过去的原因。
- **`AnimatedIcon` 的镜像是转 π 再平移，不是水平翻转**——两个轴都翻。上游那十四个
  图标上下对称，所以看起来一样;换个不对称的图标就不一样了。
- **`SemanticsData` 的构造函数几乎全是断言**：六个字段说同一件事——有文字就必须有
  方向。没有方向可交给读屏时它按字符猜，全一种文字时猜得对，混排时静默地错。
- **`ChildSemanticsConfigurationsResultBuilder` 的判重比对象不比内容**：两个说得
  一模一样的孩子仍是两个孩子。
- **`Page` 的两个无 key 页面是匹配的**（Dart 里 `null == null` 为真）。这正是声明式
  navigator 重排无 key 页面时看起来像"内容变了"的原因。

---

### 有意留下的边界

- **animated_icons 的十四份美术数据没有 port**：上游是 34,000 行 `.g.dart` 生成
  产物，由不在本仓库的工具从矢量图生成。逐行誊抄等于用眼睛复制构建产物，且无从
  校验。机器（插值、路径命令、镜像、缩放、透明度合成）完整可用，调用方自己造的
  `AnimatedIconData` 今天就画得对；`AnimatedIcons::data` 返回 `None` 而不是一个
  空图标——空图标什么都不画却自称是图标，调用方要到渲染的另一头才发现。
- **`Preview` 的注解那一半没有对应物**：Dart 的 `@Preview()` 是当元数据用的 const
  实例，Rust 的对应物是属性宏——那是个 proc-macro crate 而不是类型，等到有东西消费
  它时再写才值得。
- **`SemanticsConfiguration`/`SemanticsData` 的字段取本 crate 已建模的那套**，上游
  约四十个字段里的 platform view id、link URL、validation result、role、input type、
  traversal identifier 在此侧没有对应物。合并规则与断言是真正 port 的部分。
- **`SemanticsOwner` 不做脏节点增量**：上游的节点是原地变更的长寿对象，这边是扁平
  列表里的值，重走加 diff 到达同一个「只告诉平台变了什么」。

---

## 三、不要弄坏的(这些已经逐条比过,是对的)

改任何一条时,这些是回归线:

- **盒协议本身。** `BoxConstraints` 的 `enforce` / `deflate` / `loosen` /
  `constrain` / `tight*` 与 `box.dart` 逐字同义,包括 `enforce` 那四个 `clamp`
  的写法。
- **命中测试的协议。** `hit_test` 是模板:框里 **且**(孩子命中 **或**
  `hit_test_self`)才加自己、才返回 `true`。容器只覆盖 `hit_test_children`;
  覆盖 `hit_test` 的只有 `RenderIgnorePointer` 与 `RenderPointerRegion`,和上游
  覆盖 `hitTest` 的是同两个。
- **`RenderRef::layout` 的早退**(`!needs_layout && constraints 相同`)、
  `mark_needs_layout` 的早退、`update_from` ≙ `updateRenderObject` + 比较型
  setter、repaint boundary 与留存层。
- **具体算法照抄住的**:`RenderAspectRatio`(六个 if 的**顺序**都一样)、
  `RenderPadding`、`RenderAlign`(shrinkWrap 判据一致)、`RenderConstrainedBox`
  的布局、`RenderWrap` 的分行(`break_into_runs` 是布局和固有尺寸**共用**的那一
  份,别再抄第二份)、`RenderFlex` 的三趟(非弹性 → 分配余量 → 定位)与
  "不可 flex 时退化成无界"。
- **`Container` 的层序**:`Align → Padding → Decoration → Constraints → Margin`
  (`widgets.rs` 的 `shape()`)与 `container.dart` 的 `build` **一模一样**,
  包括 padding 在装饰**里面**,以及只有真有装饰时才加 `Decoration` 那一层——
  `RenderDecoratedBox::hit_test_self` 靠的就是后面这条。
- **标题栏先量两头,再给中间。** `RenderNavigationToolbar`(`widgets.rs`)是
  `navigation_toolbar.dart` 的 `_ToolbarLayout.performLayout` 逐行照抄:leading 用
  `minHeight: size.height` 量、钉在 x=0;trailing 用 `loose(size)` 量、钉在
  `size.width - trailingSize.width`;**然后**才把
  `size.width - leadingWidth - trailingWidth - middleSpacing * 2` 交给 middle。
  顺序是全部——反过来用一个 flex 排 [标题][弹性空隙][尾部],标题一旦比条宽,空隙
  归零,尾部就被顶到条外面,画都画不出来。这是回归线,四个测试钉着
  (`widgets.rs` 的 `a_title_too_long_for_the_bar_does_not_push_the_action_off_it`
  等)。`ListTile` 是同一条账,走的是 flex(标题 `expanded`、尾部不可 flex),对应
  `_RenderListTile._computeSizes` 的
  `tighten(width: tileWidth - titleStart - adjustedTrailingWidth)`。
- **动作要用 `MainAxisSize::Min` 的行包一层。** 上游
  `actions = Padding(child: Row(mainAxisSize: min, ...))`,这一层不是装饰:
  `_ToolbarLayout` 给 trailing 的是 `loose(size)`,**宽度有界**,而 `Align` 在有界
  约束下会撑满(上游 `RenderPositionedBox` 只在无界时收缩),这个框架的 `Button`
  正是用 `Align` 居中标签的。少了这层,尾部会吃掉整条,标题分到 0。
- **条是定高的。** `K_TOOLBAR_HEIGHT` = 上游 `kToolbarHeight` = 56,带副标题时用
  `TOOLBAR_HEIGHT_WITH_SUBTITLE`(上游 `AppBar` 没有副标题,`toolbarHeight` 是它
  的参数,这是本项目对它的取值)。外面套 `RenderClipRect`,里面的标题
  `soft_wrap = false` + `TextOverflow::Ellipsis` + `max_lines = 1`,文字缩放按上游
  `_kMaxTitleTextScaleFactor` = 1.34 夹住——**高度不跟着读者的字号长,所以字号得夹**。
  这三样是一套,拆掉任意一样另外两样就说不通了。
- **`PaintContext` ≙ `PaintingContext`** 的双重身份(画布 + 切层),懒起 picture。
- **语义的三道闸门**:没人听、没人标、走完发现没变,任意一道都让这一帧一个字都
  不发。见 `semantics.rs` 的模块文档。

---


## 文本选择那簇六个，以及一个 InheritedWidget 该怎么落地（2026-08-21）

`TextSelectionPoint`、`DesktopTextSelectionToolbarLayoutDelegate`、
`DefaultSelectionStyle`、`RenderEditablePainter`、`VerticalCaretMovementRun`
入 `text_selection.rs`，`AutofillGroup` 入 `autofill.rs`。12 + 8 条测试，
11 条变异全红。MISSING 19 → 13。

几处值得记：

* **`VerticalCaretMovementRun` 的黏着列**：从长行末尾按下到短行，光标落在短行
  末尾；**再**按下到另一条长行，回到原来那一列而不是短行末尾。上游靠的是
  `origin_x` 整段生命期不变、每条新行都拿它去问，而不是逐行搬运位置。这里
  `origin_x` 私有且没有任何写它的代码——「从不更新」是类型的性质而非某个方法的
  行为，所以我删掉了那条空测试：它断言的是编译期已经保证的事。
* **`.max(0.0)` 是我自己加的**：`line_extent` 不为负、光标 x 也不为负，这个夹
  值没有任何输入能让它生效。上游没有它（上游是问段落「离这个偏移最近的位置」）。
  按这条线的老规矩——自己的不可证伪代码删掉，不是加注释——删了。
* **变异 M1 第一次写错了**：`a.min(b)` 换成 `b.min(a)` 不是变异，是同一个函数。
  换成「完全不夹进行内」才真的红。**一条存活的变异要先怀疑变异本身。**
* **`AutofillGroup` 的第一版用了 thread-local 栈**：push 状态、build 子树、pop。
  在这个框架里是错的——`build` 返回子 widget 而不是就地构建它，等子树里的字段
  真正 build 时栈早空了。改用框架自己的 `provide` / `context.inherited`，问题
  自然消失，而且上游 `_AutofillScope.updateShouldNotify` 比的是 `_scope` 的
  **身份**（`!=` 比对象），正好落在 `provide` 要求的 `PartialEq` 上：用
  `Rc::ptr_eq`。比内容会让表单里任何一个字段注册一次就重建全部字段。
* **区分身份与内容的测试第一次没做到**：我拿同一个 `Rc` 包出的两个 handle 去比，
  内容比较给的是同样的答案，变异因此存活。真正能分开的用例是**两个不同的组持有
  完全相同的内容**——加上之后 M3 红了。
* **先问祖先、后发布**，顺序是承重的：先发布的话每个组都能看见自己，于是没有一
  个组是 topmost，autofill context 永远不会被结束。变异掉这个顺序，三条测试红。

剩余 13：list wheel 3、debug/诊断 6、语义 2、`TrainHoppingAnimation` 1、
`MenuSerializableShortcut` 1。下一轮做 list wheel 那簇。

## 列表轮那簇三个：改名不算 port（2026-08-21）

`ListWheelChildManager`、`ListWheelParentData`、`RenderListWheelViewport` 入
`list_wheel.rs`。22 条测试，10 条变异全红。MISSING 13 → 10。

**这三个的账本来可以用「已等价，只是叫 `RenderListWheel`」搪塞过去。** 但那正是
上一轮审计 `mapped` 桶清出去的东西，所以照做了真的：

* **`ListWheelChildManager` 是个 trait，`ListWheelElement` 实现它**——上游的
  element 也正是 `implements ListWheelChildManager`。四个成员元素本来就都有，
  缺的只是那句「它是这个」，而这句话编译器能检查。`child_count` 返回 `None`
  **不是「没有孩子」而是「没有已知上界」**，循环轮就是这么没有两头的；这是最容易
  读反的一个成员，专门写了测试。
* **`ListWheelParentData` 的 `index` render object 推不出来**：活跃区间从轮子
  滚到哪儿开始就从哪儿开始，不从 0 开始。`transform` 是**画的时候记下来**的，
  因为 `applyPaintTransform` 是之后才问的，而投影不是任何一边能凭几何重算的。
  上游说 `transform` 为空「正常不会发生」，在这里会：圆柱背面的孩子布局了但不
  画——不是错误。
* **`RenderListWheelViewport` 原来漏了五个属性**，其中三个 widget 层
  （`ListWheelViewport`）已经声明了却从来没传给 render object：
  `over_and_under_center_opacity`、`render_children_outside_viewport`、`clip`。
  声明了不接线，比没声明更难发现。

三处把上游读对了才没做错的地方：

* **`offAxisFraction` 的 0 是中间，不是 0.5。** 我第一版文档写的就是 0.5。
  上游 `_centerOriginTransform` 平移 `width/2 * (-f*2+1)`：f=0 → 中间，
  f=0.5 → 左边缘，f=-0.5 → 右边缘。
* **`overAndUnderCenterOpacity` 不是渐变。** 我第一版写了个按距中心距离的
  ramp。上游是**一律的一个值**，而且把所有偏心孩子画进**同一个** opacity 层，
  再把中心那些按全不透明画一遍——`center` 参数就是为这个存在的。每个孩子一层
  会有多少行就有多少层，而且重叠处会露缝。改成两遍。
* **`use_magnifier` 与 `magnification` 是两件事。** 变异 M9 一开始活了下来：
  没有测试区分「放大镜关着但带着放大倍数」。上游分支看的是 flag 不是比值，
  补了测试之后红。

一个 API 让步：`PaintContext::in_layer` 从模块私有放宽到 `pub(crate)`。给**一组**
孩子加裁剪或不透明层的 render object 没有东西可以喂给 `child: &dyn RenderBox` 的
接口，而 `RenderBox: AsAny` 蕴含 `'static`，借用式的包装类型做不出来。render.rs
里的辅助函数本来就走这个口子。

剩余 10：debug/诊断 6、语义 2、`TrainHoppingAnimation` 1、
`MenuSerializableShortcut` 1。下一轮做 debug/诊断那簇。

## debug/诊断那簇六个，以及一个会让进程整个中止的 bug（2026-08-21）

`DebugCreator`、`DiagnosticsDebugCreator`（入 `diagnostics.rs`）、
`ShortcutMapProperty`（入 `shortcuts.rs`）、`RenderErrorBox`、
`DebugOverflowIndicatorMixin`（新建 `debug_rendering.rs`）、
`AccessibilityInspector`（入 `semantics.rs`）。40 条测试，24 条变异全红。
MISSING 10 → 4。

**变异逼出来的一个真 bug。** 变异 M14 跑出来不是「红」而是
`fatal runtime error: thread local panicked on drop, aborting`——整个测试进程
0xC0000409 崩掉。查下去是我自己写的代码：`AccessibilityInspector` 是个
thread-local，里面握着的 `SemanticsHandle` 在**线程销毁时**被 drop，
`dispose()` 走到 `apply_enabled(false)`，那里读 `COLLECTOR`，而 `COLLECTOR`
这时可能已经被销毁了。`with` 在那种情况下 panic，而**drop 里的 panic 在
teardown 期间会 abort 整个进程**，不是让一个测试失败。真实 app 里一个握着
semantics 的 inspector 在退出时就会这样。

改成 `try_with` 之后又走了一轮：**不是每个 thread-local 都会消失**。
`const` 初始化的 `Cell` 没有东西要 drop，根本不注册析构，整个线程生命期都可达；
`NEEDS_UPDATE` 和 `HANDLES` 是这种，我给它们加的 `try_with` **一个变异都杀不掉**
——按老规矩，自己的不可证伪代码删掉，改回 `with`。`COLLECTOR` 和
`ENABLED_LISTENERS` 装着 `Vec`，会注册、会消失，两个 `try_with` 都留下。

`ENABLED_LISTENERS` 那个第一次也活了下来，因为 `COLLECTOR` 的守卫先返回了。
要真正打到它，需要一个**初始化时机夹在两者之间**的 dropper：thread-local 按
初始化的逆序销毁，所以先摸 `COLLECTOR`、再建 dropper、最后建 `ENABLED_LISTENERS`，
drop 跑的时候前者还活着、后者已经没了。构造出来之后变异红了。

几处读上游读出来的：

* **`_formatPixels` 三段的边界都是 `>` 不是 `>=`**，而且下面那段是
  `toStringAsPrecision(3)`——**三位有效数字，尾零保留**：`0.5` 是 `"0.500"`
  不是 `"0.5"`。我第一版写成三位小数还顺手把尾零 trim 掉了，两处都错。
* **Dart 的 `toStringAsFixed` 半数向零外舍入，Rust 的 `{:.0}` 向偶舍入**：
  `10.5` 上游是 `11`，我第一版是 `10`。`diagnostics.rs` 里 `format_double`
  早就记过同一个坑，这次是同一个坑的第二次。
* **`DiagnosticsDebugCreator` 是 hidden 级，这就是它的全部内容**：它挂在 render
  object 报的每一条错误上给 inspector 用，级别低到任何普通 dump 都不会打印，
  所以从不出现在读者要看的 console 输出里。「存在但永不显示」是件奇怪到需要
  单独一个类的事。
* **`AccessibilityInspector` 的两遍子节点遍历是空的**：上游先按 traversal
  order 压一遍、再按 inverse hit-test order 压一遍。两遍是同一批孩子，有
  visited 集合在，第二遍不可能到达第一遍没到的节点；它只改出栈顺序，而结果按
  id 建索引，所以那也不改。这里只压一遍，**记下来而不是照抄**——照抄会让它看起来
  在干活。
* **`ShortcutMapProperty` 存在只为一个方法**，而修饰键顺序上游自己就不统一：
  `SingleActivator` 是 Control/Alt/Meta/Shift，`CharacterActivator` 是
  Alt/Control/Meta 且根本没有 shift。原样保留，因为这些字符串会出现在人们
  拿去和上游对比的错误信息里。
* **`SemanticsHandle` 有 `Drop`**，所以上游 `_enableSemantics` 里那个 `??=`
  防的泄漏在这里由 RAII 已经防住了，变异杀不掉。那是**上游的**代码不是我的，
  按老规矩留下并标注，测试改成压那条让它多余的不变量（handle 一 drop 就释放）。

剩余 4：`TrainHoppingAnimation`、`CustomPainterSemantics`、
`PlaceholderSpanIndexSemanticsTag`、`MenuSerializableShortcut`。下一轮清完，
然后审计 `out_of_scope`（50 条）。

## 最后四个，以及第四次审计：out_of_scope 桶（2026-08-21）

`TrainHoppingAnimation`、`PlaceholderSpanIndexSemanticsTag`、
`CustomPainterSemantics`、`MenuSerializableShortcut` 落地，MISSING 4 → 0。
然后审计了**唯一没审过的桶** `out_of_scope`（59 条），9 条理由不成立，
改判回 MISSING 并当轮补齐，再次归零。共 41 条测试、32 条变异全红。

### 最后四个里值得记的

* **`TrainHoppingAnimation` 的两个顺序都承重**：值处理器里先跳车再读值——反过来
  会把旧车最后一个值再报一次；`onSwitchedTrain` 在值通知**之后**才发——反过来
  听到「换车了」的人看到的还是旧车的值。两条各有一条变异。
* **`PlaceholderSpanIndexSemanticsTag` 是唯一按值比较的 tag**。`SemanticsTag`
  按身份比，就是为了让两个碰巧取同名的子系统互不干扰；这个故意反过来，因为段落
  每次布局都重造一批，这一帧的节点必须被认成上一帧那个。这里写成**由 index 推出
  的 id**，落在计数器够不到的区间，两套方案不会撞。
* **`MenuSerializableShortcut` 上游是 mixin 而不是 `ShortcutActivator` 的成员**，
  因为「匹配按键事件」和「出现在平台自己的菜单栏里」是两套机器，而且不是每个
  activator 都能做后者：`LogicalKeySet` 没有「一个触发键 + 修饰位」这种平台能画
  的形状，上游干脆不给它 mix。

### 第四次审计的收获

| 条目 | 记的理由 | 实情 |
|---|---|---|
| `ColorProperty` `IconDataProperty` `TransformProperty` | 「诊断树未移植」 | **假的**，`diagnostics.rs` 早就在了，本会话还用它做了两个类 |
| `ClipContext` | 「仅 debug 断言用」 | **假的**，上游 `PaintingContext extends ClipContext` |
| `ImageSizeInfo` + 内存分配四件套 | 「debug-only」 | 上一轮刚整簇移植 debug 类，这不再是理由 |

补齐时读出来的：

* **`ClipContext` 的四种行为是四条不同的调用序列，不是四个开关。**
  `antiAliasWithSaveLayer` 出去时要 **restore 两次**，因为它多开了一层。抗锯齿
  裁剪会把边缘像素和画布上已有的东西混合——背景不透明时没问题，被裁的内容自己
  也要合成时就会混两次，边缘看得出来。save layer 给它一块自己的缓冲，代价是一趟
  离屏，所以不能当默认。
* **`ImageSizeInfo.isOversized` 按「每个方向都超过两倍」而不是按面积。** 一张远
  比框宽、却不比框高的图不是浪费，是加信箱边——开发者自己选的。变异 M6 一开始
  活了下来，因为我的边界测试两个轴同时正好卡在两倍，`&&` 把它挡住了；补了「一个
  轴正好两倍、另一个轴远超」才红。
* **`FlutterMemoryAllocations` 在 dispatch 期间移除监听者是写洞而不是删元素。**
  按下标走的循环里删一个会让后面的整体前移、漏掉一个。用的是**活跃循环计数**而
  不是布尔，因为可以嵌套。长度也只在开头读一次，所以本轮加进来的监听者不会被
  本轮叫到。
* **创建事件带库名和类名、销毁事件不带**，这个不对称就是设计：追踪器按对象身份
  配对，跑完还没配上的就是泄漏，而且名字已经在手上了。

四个桶全部审计过一遍了。**四次审计，四次都找出被判断藏起来的真实工作。**

## 第五次审计：`covered` 桶不等于「已移植」（2026-08-21）

四个台账桶审完之后，剩下的唯一未经检验的判断是 **`covered` 桶本身**——1400 多个
类，判据只是「有个同名符号」。口径三则第 2 条从一开始就写着「名字命中只是入门」，
而本会话已经撞见过一次反例：`RenderListWheel` 名字对得上、少五个属性，其中三个
widget 层已经声明却从没往下传。

所以写了第二把尺子 `tools/depth.py`：对每个 covered 类，数上游声明的公开成员、
数本侧对应类型暴露的成员、报比值。**它是启发式的，而且撒谎的方向是固定的**——
getter/setter 对、`==`/`hashCode`/`toString` 变成 derive、一个 Rust enum 顶掉
一族 Dart 子类、成员答在别处（自由函数、别的类型），每一种都让比值**低报**。
对一把「产出待读清单」的尺子来说这是对的方向：宁可多报，不藏。

**这把尺子自己也错了两次，两次都是往「看起来更糟」的方向错：**

1. `RUST_MEMBER` 只认 `pub`。trait 的方法和 enum 的变体**没有 `pub`**——它们随
   类型公开。结果全 crate 的 trait 和 enum 都读成 0 个成员。
2. Dart 侧只按缩进匹配，把每个方法体里的局部变量和构造调用都算成成员。
   `ListTile` 因此报了 87 个成员，里面有 `break`、`false`、`InkWell`——比值实际
   在度量「上游的方法有多长」。改成跟踪花括号深度，只数类自己那一层：87 → 40。

修好之后，682 个 6+ 成员的 covered 类里，最浅的那一批**约三成是真的**：

| 抽样 | 报的 | 实情 |
|---|---|---|
| `MediaQuery` 6/65 | 浅 | **假阳性**，65 个几乎全是 `xOf`/`maybeXOf` 便利静态方法，这里由 `context.inherited::<MediaQueryData>()` answer |
| `SemanticsNode` 9/69 | 浅 | **假阳性**，成员在 `SemanticsProperties` 上 |
| `ListTile` 6/40 | 浅 | **真的**——见下 |
| `Text`/`Flex`/`ClipRect` 0/n | 浅 | **假阳性**，移植成自由函数（`pub fn flex(...)`），尺子看不见 |

### `ListTile`：主题解析了控件从不读的东西

跟 ListWheel 一模一样的形状。`ResolvedListTile::of(context, selected)` 早就完整
处理 `selected`（`selected_color`、`selected_tile_color`），也早就解析出
`min_leading_width`、`dense`、`text_color`——**而控件把 `selected` 恒传 `false`，
另外三个一个都不读**。而且没有 `leading`：那是 ListTile 最常用的一个位置。

补上 `leading`、`selected`、`enabled`、`dense`、`is_three_line`、
`content_padding`、`min_leading_width`。10 条测试，10 条变异，两条一开始活着：

* **`dense` 那条是我自己想错了。** 我写的是「解析完再按 dense 取 min」，上游是
  `minTileHeight ?? (dense ? 48 : 56)`——**主题显式设了高度就赢到底，dense 什么
  都不改**。事后调整高度分不出这两种情况。改成把 widget 的 `dense` 传进
  `ResolvedListTile::of` 里去参与解析，两条变异都红了。
* **`min_leading_width` 的几何后果在这个测试架子里不可观测**：它影响的是**标题**
  从哪儿开始，而标题是 tile 内部建的、没有标记，intrinsics 这条链上也没人实现。
  只断言了「控件的值压过主题的」，并在测试里写明为什么不断言几何——**写一条不可能
  失败的测试比不写更糟**。

`covered` 桶的重读是一条长队列（682 个候选，约三成为真）。下一轮继续。

## `covered` 桶重读之二：三个控件列表项**根本没有 widget**（2026-08-21）

`list_tiles.rs` 里 `CheckboxListTile`/`RadioListTile`/`SwitchListTile` 早就在，
`ListTileControlAffinity::resolve` 那条发现（`platform` 按控件而不是按平台变）也
写得很清楚——**但整个文件里没有一个 `Component`、没有一个 `build`、没有一个
`AnyWidget`**。三个类型是纯描述，没有任何东西能把它们画出来。

跟 ListTile 那次是不同形状的漏：不是「选项被丢掉」，是**控件从来没被建出来**。
上游这三个的 `build` 全都以一个 `ListTile` 收尾——而 `ListTile` 上一轮刚补深，
所以现在正好建得出来。加了 `ControlTile`：把控件放进 affinity 说的那个槽、
secondary 放另一个、subtitle/dense/selected/enabled 往下传给 `ListTile`。

11 条变异，5 条第一轮活着，每一条都指出一个真问题：

* **M2**（没有 secondary 时用控件填另一个槽）：变异打在 `control_is_leading`
  为真的分支上，而我的测试用的是 switch（控件在 trailing）——**变异根本没碰到
  被测的那条路**。补了 radio 那一侧才红。
* **M3**（禁用时控件仍带 handlers）：我的测试传的本来就是空 handlers，去不去闸
  一样。改成传真 handlers、点下去数回调次数。
* **M4 / M10 互相遮蔽**：`ControlTile` 有一道 `if self.enabled` 才 `tappable`，
  `ListTile::build` 里还有一道 `enabled.then_some(id)`。**两道闸各自单独去掉都不
  会红**，因为另一道还拦着。两道都是上游的（`enabled: onChanged != null` 与
  `onTap: onChanged == null ? null : ...`），所以都留下并标注；再跑一条**同时去掉
  两道**的变异证明这对是承重的（M11 红）。
* **M9 让我发现一个真 bug**：`ControlTile` 一开始**没把整行做成点击目标**。上游三
  个都把 `onTap` 传给 tile，那不是便利——**标签是控件的一部分**。一行只有那个
  20 像素的方框能点、旁边 400 像素写着它是干什么的却不能点，正是上游那个 `onTap`
  存在的理由。补上之后 M9 红。
* **M7**（subtitle 内容不往下传）活着，而且**在这个测试架子里不可观测**：stub 引擎
  的文字量为零，控件自身 28 高又决定了整行高度，有没有副标题一样高。只断言了
  描述记住了它（`validate` 和三行规则读的就是这个），并写明为什么不断言别的。

顺带给 `Checkbox`/`Radio` 补了 `with_handlers`（`Switch` 早就有）。

**尺子的比值没动**（SwitchListTile 仍是 3/46）——成员在 `ControlTile` 和
`ControlListTile` 上，不在同名类型上，这正是 `depth.py` 文档里写明的盲点之一。
比值是待读清单，不是判决。

## `covered` 桶重读之三：`Switch` 不读自己的主题（2026-08-21）

先给 `depth.py` 加了「同伴类型」：类 `X` 的成员也可能答在 `XData`、`XState`、
`ResolvedX`、`RenderX` 上。加上之后 `MediaQuery`、`Flex`、`AppBar`、`TabBar`、
`InputDecorationTheme` 这一批假阳性直接掉出榜单——不是因为尺子变松了，是因为它
本来就在报同一种盲点报了十几次。

然后 `Switch`（4/30）：**`SwitchThemeData` 完整移植着**——`thumb_color`、
`track_color`、`track_outline_color`、`track_outline_width`、`padding`、
`splash_radius`、`material_tap_target_size` 全在，`SwitchTheme::of` 也在——
**而控件一个都不读**，颜色全是从 app theme 硬取的 `theme.primary` / `theme.outline`。
ListWheel、ListTile 之后同一形状第三次。

补上：读 `SwitchTheme`、`enabled`（上游的 `onChanged == null`）、四个单控件颜色
覆盖（`activeColor`/`activeTrackColor`/`inactiveThumbColor`/`inactiveTrackColor`）。
比值 4/30 → 11/30。

**颜色在这个测试架子里本来是看不见的**：stub 引擎只记「开了几个 layer」，不记画
了什么颜色。所以把解析从 `build` 里提出来成 `Switch::resolved` —— `build` 调它，
测试也调它，**测的是真路径而不是一个 getter**。9 条测试，8 条变异。

**M6 活了下来，而它指向的是我自己的多余代码**：我在 `build` 里写了
`if outline_width > 0.0` 才画边框，而 `RenderDecoratedBox` 里**本来就有一道
`if self.border_width > 0.0`**。按老规矩，自己的不可证伪代码删掉。

但删之前先问了一句：**那道我准备去依赖的闸，自己有测试吗？** 变异掉它——
全绿。没有。整个 crate 里「零宽边框不画」这条没有任何东西在测，而且**谁也测不
了**：stub 的 `rf_canvas_draw_rect` 是个空函数。

所以给 stub 加了一个 `rects` 计数器。`LayerCalls` 原来只说一帧是怎么**搭**的，
现在也说往里**画**了多少。加完之后那条变异红了，而且这解锁的不止这一条——
任何「这个东西画了/没画」的断言现在都有东西可对。

同 `in_layer` 放宽到 `pub(crate)` 那次一样：**为了让一条断言可证伪而动一下工具，
比留一条不可证伪的断言划算。**

## 第三把尺子：38 个主题没有任何人读（2026-08-21）

同一个形状撞了三次——`RenderListWheelViewport`、`ListTile`、`Switch`，每次都是
**主题数据完整移植、`Theme::of` 解析器就位、控件一个字段都不读**——所以这轮不再
一个个撞，直接问这个问题：`tools/unwired.py`。

判据放得很松：只要 `XTheme::of` 或 `ResolvedX::of` 在定义它的文件之外被调用过一次，
整个主题就算「有人读」。所以上榜的是**一个读者都没有的**，不是「读得少的」——
读得少是 `depth.py` 的活。

**49 个主题包装器，38 个没有任何人读。**

一个没人读的主题比缺一个主题更糟：它看起来是做完的、类型也对，而调用者设了它之后
什么都不会发生。

这轮先把 `Radio` 接上（`Checkbox` 是现成的模板，它是这 49 个里少数几个真读了的）。
`RadioThemeData` 里 `fill_color`、`side`、`inner_radius`、`background_color` 全在，
控件一路硬编码 `theme.primary` / `theme.outline`。38 → 37。

读上游读出来的两处：

* **两个半径，只有里面那个是状态属性。** `_kOuterRadius` 8、`_kInnerRadius` 4.5，
  上游只让内圈可按状态覆盖。那个不对称就是动画：单选按钮是靠点从无到有长出来的，
  所以点有每个状态的尺寸、圈不动。**未选中的内半径是 0，那不是特例**——是同一个
  属性对另一个状态的回答，所以画的时候只有一条路径而不是两条。
* **圈和点是同一个颜色**：上游用同一个 `_defaultFillColor` 画轮廓和画点，所以选中
  一个单选按钮是两者一起变色，不可能不一致。

9 条测试、8 条变异。**两条一开始活着，都是「解析对了但没测控件有没有正确地问」**：
M7（widget 传空状态）和 M8（widget 不看 enabled）。解析函数本身测得再细，也证明
不了控件把自己的状态交对了。补的办法是上一轮刚加的 `rects` 计数器——给主题一个
「只有选中时才有背景色」的属性，选中的那个就比没选中的多画一个矩形。**上一轮为了
一条断言加的工具，这一轮直接又还了两条。**

剩下 37 个是一条明确的队列。

## 未接线队列之二：`Badge`，以及一个「像素动了、布局没动」的错法（2026-08-21）

先给 37 个分了类：**25 个控件已存在、只是不读主题**，12 个连同名控件都没有
（`TextTheme`、`ElevatedButtonTheme` 那类，控件在这边叫别的名字或是纯样式）。

这轮做 `Badge`。除了不读主题，它还漏了三件上游的正事：

* **没标签的徽章是一个点，不是空的胶囊。** 这边的 `label: String` 根本不允许没有。
  两者说的不是一件事：数字说「有多少在等」，点只说「有事」。上游为此有两个尺寸
  常量（`smallSize` 6 / `largeSize` 16）而不是一个缩放——点不是小胶囊，它里面
  没有字要留位置。
* **徽章要坐在别的东西的角上。** 这边的 `Badge` 是独立的，没有 `child`。
* **背景是 `colorScheme.error` 而不是 primary。** 徽章是「有事要处理」的计数，
  配色里本来就有一个正好表示这个的颜色；用 primary 会让它看起来像装饰。

顺手记了一条上游自己招认的东西：默认 offset 是 `(4,-4)`，**再加一个 `(0,8)`**，
而上游注释直说那 8 是为了「定位算法改了之后不动到现有用户」加的兼容常量。照抄了
——一个比上游低 8 像素的徽章，理由再正当也是错的。

**一个我自己写错、被 `visit_children` 抓住的错法：** 位移我一开始是用
`RenderTransform` 做的。测试读位置时发现 dy 是 0——因为**变换是绘制时的，布局
不知道**。那不只是测不到：命中测试和布局都会和画出来的东西对不上。上游的私有
`_Badge` 是在**布局里**对齐并推开的，所以改成给 `RenderAlign` 加一个 `nudge`，
写进 `child_offset`——那个字段 paint、hit_test、`visit_children` 三处共用，
于是三者不可能再各说各话。

10 条测试、10 条变异。三条一开始活着，都是「看不见就测不到」：

* 隐藏标签（`isLabelVisible`）——用 `rects` 计数器解决。
* 点不加位移——用 `visit_children` 走一遍渲染树读真实偏移解决。**这是这条线第
  一次读回几何**，比命中测试能说的多得多。
* 标签徽章高度封顶——那是我自己加的，在内容自撑的盒子里 `min_height` 已经决定
  了答案；留着并标注了它对应上游 `_IntrinsicHorizontalStadium` 的哪一句。

另外：`Center` 会**撑满父级**，而上游 Container 的 `alignment: Alignment.center`
在内容自撑的盒子里什么都不动。我一开始照着「居中」的字面意思写了 `Center`，
结果一个宽松约束下的徽章有 400 像素宽。

主题 37 → 36。

## 未接线队列之三：`Tooltip`，以及 `ThemeData` 少一个 `platform`（2026-08-21）

`Tooltip` 不读主题，而且它的构造只收一个 `bubble` 闭包——**调用者自己把气泡整个
搭出来**，于是主题里的 decoration、padding、margin、height、text_style 一个都
进不来。补了上游的 `Tooltip(message:)`：给一段文字，气泡由主题搭。

接上去的时候发现更前面缺一块：**上游读的是 `Theme.of(context).platform`，而这边
的 `ThemeData` 根本没有 `platform` 字段。** 补上了，默认取宿主
（`TargetPlatform::host()`，编译期决定——Rust 二进制只为一个宿主构建，问不出别的
答案），而且**可被覆盖**，因为上游正是靠这个让开发者预览别的平台。

### 这个 tooltip 的一半默认值取决于平台

桌面 24 高、水平 padding 8；手机 32 高、padding 16。**这不是随手定的**：桌面提示
是鼠标精确停在某处唤出来的、一尺远读；触摸提示是长按唤出、出现在手底下、一臂远读。
大的那个不是更大方，是同一个提示放在它实际会被读到的距离上。**只有水平那一半随
平台变**——高度已经给了触摸提示需要的空间，再加竖直 padding 会和它打架。

12 条测试、11 条变异。**三条一开始活着，两条是我写了不可能失败的测试**：

* `the_widgets_own_offset_beats_the_themes` 只断言了「树建起来了」。气泡落在
  overlay 里、这个架子够不着，所以按 `Switch::resolved` 的老办法把那一步提成
  `Tooltip::placement_from`——`build` 调它、测试也调它。
* `ThemeData::lerp` 丢掉 platform 没人管。补了：平台不是数、插不了值，只能从近的
  那一端取——取一个固定值会让每次主题过渡都短暂声称自己跑在别处，所有自适应的
  东西都会闪一下。
* **`TargetPlatform::host()` 写死成 `Windows` 杀不掉**——在 Windows 上跑，两者
  就是同一个值。一台机器分不出 `host()` 和这台机器的名字。测试改成明说这一点：
  钉住的是「默认来自 `host()`」这条链，不是那个值；`host()` 把哪个 target 映射到
  哪个平台，得换台机器才验得了。

主题 36 → 35。`Tooltip` 的深度比 26/26 还高，因为解析器和放置函数都算它的。

## 未接线队列之四：进度指示器的两半从来没接上（2026-08-21）

`progress_indicator.rs` 里 `ProgressIndicator`/`LinearProgressIndicator`/
`CircularProgressIndicator` 都在，周期常量、`validate`、`track_gap_scale` 写得很
细——**一个 `Component` 都没有**；而 `components.rs` 里另有一个 `ProgressBar`，
颜色硬编码，和那三个毫无关系。跟三个控件列表项一模一样的两半没接上。

补了 `ResolvedProgressIndicator`，并让线性和圆形两个真的能画。

* **轨道是自己的颜色，不是调暗的填充色。** 上游默认 `secondaryContainer`。
  调暗的填充读起来像「这一段完成得少一点」；自己的颜色读起来是「填充正在经过的
  空间」，而那才是它。
* **圆形的轨道默认是透明的**（上游 `circularTrackColor` 为 null），而线性的默认
  有轨道。差别是故意的：转圈后面带一圈，看起来像一个能拖的控件而不是「正在发生
  的事」。
* **填充写成 flex 权重而不是宽度**：条的宽度要到布局才知道，把它的一个比例写成
  宽度就是在猜。而写成权重踩到另一件事——**权重 0 不是「不占地方的孩子」而是
  矛盾**，flex 直接 panic（"the flex factor must be positive"）。所以 0 权重的
  孩子根本不压进去。

8 条测试、7 条变异，**一条一开始活着，而它揭穿的是我的测量方式**：其余几条测试
找填充的办法是「比条窄的盒子」，而 **value 1.0 的填充和轨道一样宽**，这个办法看
不见它。于是「不确定时不画填充」那条变异（改成画满）活了下来。换成数矩形——
不确定时一个（轨道），有值时两个（轨道 + 填充），满的时候还是两个而不是一个。

主题 35 → 34。

## 未接线队列之五：`SnackBar` 的错误信息要说清是谁定的（2026-08-21）

`SnackBar` 里 `behavior`/`elevation`/`margin`/`width`/`show_close_icon`/
`action_overflow_threshold` 全是 `Option`，三步链的第一步一应俱全——**第二三步
没有**：主题从不被读，默认值也没有。补了 `ResolvedSnackBar`，并让 `SnackBar`
自己去问它。

三处上游读出来的：

* **`width` 和 `margin` 只在浮动时有意义。** 固定的 snack bar 贴在屏幕底边、横
  跨整宽，那里没有宽度和外边距的位置，上游为此断言而不是默默忽略。
* **而且断言里要说清 fixed 是谁定的。** 上游的消息分三种：构造函数里写的 / 继承
  的主题写的 / 默认。只被告知「宽度只能配浮动」的开发者，如果**从没写过
  `behavior:`**，是没有地方可查的。为此专门做了个 `SnackBarBehaviorSource`——
  一个枚举，只为了让错误信息能指出方向。
* **动作换行看的是比例不是宽度**（默认 0.25）。条的宽度就是屏幕的宽度，同一个
  动作在平板上宽松、在手机上就挤。而且上游的判据是 `>` 不是 `>=`。
* 浮动的条水平内边距 16、固定的 24——浮动那个的 inset padding 已经把它从边上
  推开了,自己就不用那么多。

11 条测试、10 条变异，**第一轮全红**。其中 M9 值得记：`check` 如果读
`self.behavior` 而不是**解析后的** behavior，那么「设了宽度、没设 behavior、
而主题说 fixed」这种情况就漏了——而那恰恰是开发者最难自己看出来的一种。

主题 34 → 33。

## `unwired.py` 自己错了：之前报的数是**多报**的（2026-08-21）

这轮本来要接按钮那一族，一看发现 `ResolvedButton::of` **早就被 `components.rs`
调着**，读的正是 `FilledButtonTheme`/`OutlinedButtonTheme`/`TextButtonTheme`——
而工具把这三个都算成「没人读」。

原因是工具只认两种名字：`XTheme::of` 和 `Resolved<X去掉Theme>::of`。**解析器可以
叫任何名字，也可以一个解析器读好几个主题。** `ResolvedButton` 两条都不符合。

这跟 `depth.py` 文档里写明的盲点是同一个，只是这次咬到了另一把尺子。改成两级
可达：先在主题文件里找出所有「`of` 里调了 `XTheme::of`」的类型，再看这些类型有没有
被外面调。

**所以之前几轮报的 38/37/36/35/34/33 都是多报的**——按接线的先后，真实数应是
36/35/34/33/32/31 左右。每一轮做的接线都是真的（那些主题确实没人读），少的是本来
就已经接上的那几个。修正后的数：**49 个主题，31 个没人读**。

### 顺手做掉的：`ElevatedButtonTheme`

`ResolvedButton` 覆盖 filled/outlined/text，**没有 elevated**——因为
`ButtonVariant` 里根本没有这个变体。补上了。

**它不是「带阴影的实心按钮」**：背景是低层 surface container、文字是 primary，
和实心按钮正好反过来。Material 3 是故意把它降级的——**它靠高度突出，所以颜色
就不必再喊一遍**。

补的时候 gallery 编译不过了：`button_demo.rs` 里有一份**逐字抄过来的颜色表**
（注释就写着 "verbatim"），加一个变体就漏了一个分支。修法不是补分支，是把表提到
`ButtonVariant::default_colors` 上，两边都问它——**两个地方必须保持一致的表，
不属于其中任何一个**。这也顺带让那条「elevated 用了 filled 的配色」的变异变红：
之前测试是自己传默认值进解析器的，压根没经过那张表。

主题 31 → 30。

## 未接线队列之六：`Icon` 的两个默认尺寸不是同一个数（2026-08-21）

`Icon` 之前只有 `size`/`fill`/`weight` 三个字段，**连 `color` 都没有**，而
`resolved_size(theme_size)` 是把主题尺寸当**参数**接进来的——也就是说它从不自己
读 `IconTheme`。补了 `ResolvedIcon`，并把 `color`、`grade`、`optical_size`、
`shadows`、`apply_text_scaling` 一并补上。

两处读上游读出来的：

* **有主题时是 24，完全没有主题时是 14。** `IconThemeData.fallback()` 说 24，
  而 `Icon.build` 自己的最后一招是 `kDefaultFontSize`，**14**。这不是疏忽：
  一个周围没有任何东西可归属的图标，就是一行字里的一个字形，而 14 是字形的尺寸；
  24 是 Material 的图标尺寸，那是**主题**才知道的事。
* **`applyTextScaling` 默认是关的。** 句子里的图标应该跟着读者的字号变大；
  作为按钮的图标不应该——按钮是个固定大小的目标，字形一涨就把它撑破了。
  上游让调用者说清是哪一种，而不是猜。

12 条测试、8 条变异。**一条一开始活着，原因是测试环境里文字缩放恒为 1.0**：
「缩放用在解析后而不是试探尺寸上」这条变异，乘以 1 之后两边一样。用
`with_text_scale(2.0, ...)` 包起来才红——**默认值恰好是恒等元的时候，任何关于
它的断言都是空的**。

主题 30 → 29。

## 未接线队列之七：出错的光标不由任何人做主（2026-08-21）

`TextSelectionThemeData` 的三个颜色一个都没人读，而这条线本会话早些时候刚移植的
`DefaultSelectionStyle` 只有一个「都回落到那个半透明灰」的 `resolved()`——
读不到主题、也读不到配色。补了 `ResolvedTextSelection`。

三处读上游读出来的：

* **出错时的光标颜色在链之外，不是链的第一步。** 上游写的是
  `cursorColor = _hasError ? errorColor : (widget.cursorColor ?? style ?? default)`。
  设过光标颜色的调用者，在字段正拒绝输入的时候**不能留着它**：状态比样式重要，
  而一个「错的时候和对的时候长得一样」的输入框，比一个难看的输入框糟得多。
* **选区颜色不随错误改变。** 选区是读者自己选的，给它换个颜色等于把错误算到
  他们头上。
* **选区是光标颜色的 40%，不是另一个颜色。** 选区必须能「透过去看」——底下的字
  得还能读——所以它是同一个色相小声说一遍，而不是第二个颜色跟它抢。
* **而 handle 回落到 primary 而不是选区色**：handle 是要用手指抓的东西，必须实
  色；回落到选区色会让最需要看清的那一处最看不清。

7 条测试、6 条变异，第一轮全红。

主题 29 → 28。

## 未接线队列之八：一条**四步**的链，和一个与出来的把手（2026-08-21）

`BottomSheetThemeData` 里有成对的字段——`background_color` 配
`modal_background_color`、`elevation` 配 `modal_elevation`——一个都没人读，
而 `BottomSheet` 控件连 `enable_drag`/`show_drag_handle` 都没有。

两处上游的规则，都不是三步链能表达的：

* **模态 sheet 的链有四步**：`widget ?? theme.modalX ?? theme.X ?? defaults.modalX`。
  模态专用字段在前，**共享字段在后**，再才是默认。这样一个主题可以先说一句
  「这里的 sheet 长这样」管住两种，再单独说「模态的不一样」。三步链只能表达其中
  一句。持久式的 sheet 根本不看模态字段——它是页面的一部分，不是压在页面上的东西。
* **主题要的拖拽把手要和「能不能拖」相与**：
  `showDragHandle ?? (enableDrag && (theme.showDragHandle ?? false))`。
  一个要把手的主题，不会给一个拖不动的 sheet 装把手——那是控件在承诺它做不到的
  事。只有 sheet 自己写明的 `showDragHandle` 能压过这条，因为**明写的人已经把
  责任接过去了**。

9 条测试、6 条变异。**一条一开始活着，因为没有一个测试同时设了两个 elevation**
——只设一个的时候，谁先谁后看不出来。补上「两个都设」那一例才红。

主题 28 → 27。

## 未接线队列之九：键盘的内嵌是**加**上去的，不是取较大值（2026-08-21）

`DialogThemeData` 十几个字段一个没人读。补了 `ResolvedDialog`。

**一处上游写法值得记**：
`effectivePadding = MediaQuery.viewInsetsOf(context) + (insetPadding ?? theme ?? default)`
——键盘弹起来时，对话框被推到它上面，**而且保留自己那圈边距**。取两者的较大值
按算术是对的、看起来是错的：那圈边距不是用来避开屏幕边缘的，是用来让对话框
不贴着它下面的任何东西。

另外两条小的：默认边距是 `symmetric(horizontal: 40, vertical: 24)`——**两侧比
上下宽**，因为对话框是一列文字，横跨整个手机宽度的文字不好读，而竖着没有这个
理由；`BoxConstraints(minWidth: 280)` 是**下限不是尺寸**，比这还窄的对话框看着
像一条走丢的 tooltip。

9 条测试、7 条变异，第一轮全红（其中一条锚点不唯一，换了个位置重跑）。

主题 27 → 26。

## 未接线队列之十：一个连自己文档都不符的插值函数（2026-08-21）

做 `ExpansionTileTheme` 时撞见的。

**上游那五对 `X`/`collapsed_X` 不是二选一，是补间的两端。** `collapsedTextColor`
是 `begin`、`textColor` 是 `end`，展开动画驱动它们之间的 `ColorTween`。按状态
挑一个，两端看着都对、**中间整段动画都是错的**——而中间那段才是人真正在看的。
所以这里也是两端一起解析、一起存，`lerp(t)` 给出当下那一帧。

顺带：**背景色两端都没有默认值**（上游的 `_updateBackgroundColor` 根本没有第三
步），因为 tile 坐在列表里、列表自己有背景；而文字色和图标色有默认值，因为
「颜色」不像「背景」那样可以没有。

### 真正的发现：`lerp_color` 和它自己的文档说的不是一回事

文档写着「和上游每个 `*ThemeData.lerp` 一样，走 `Color.lerp`」，紧接着描述的却是
**半程跳变**：`t < 0.5` 给 a，否则给 b。而 Dart 的 `Color.lerp` 在一端为 null 时
是 `_scaleAlpha(另一端, t)`——**淡入淡出**，不是跳变。两者只在两个端点上一致。

后果是可见的：一次主题过渡里，如果一边设了某个颜色、另一边没设，那个颜色会在
半途**啪地出现**，而不是渐渐浮上来。

改对之后 **4368 条测试全绿**——也就是说，这条错误的规则从来没有任何测试碰过。
补了测试，两条变异（跳回半程、淡入方向反）都红。

**还有一条变异揭穿了我自己的测试**：「lerp 不夹 t」那条活了下来，因为
`ColorTween` 夹的是**通道值**不是 `t`，而我挑的两个颜色（纯红、纯绿）外推之后
各自饱和回原值，两种规则**碰巧给出同一个答案**。换成 100 和 150 这种有余量的
颜色才红。

主题 26 → 25。

## 同一个错法的第二个：`lerp_f32`，而且这次是被**测试钉住的**（2026-08-21）

上一轮改对 `lerp_color` 之后顺手查了它的两个兄弟。

**`lerp_f32` 错法一样**：一端为 null 时半程跳变。上游 `lerpDouble` 是
`a ??= 0.0; b ??= 0.0; a*(1-t) + b*t`——**空端就是零**，所以一个主题设了高度、
另一个没设，分隔线是**平滑合上**而不是走到一半啪地闭合。

**和上一轮不同的是：这一条有测试**，而且那条测试断言的正是错的行为，注释还写着
「这就是 `Color.lerp(null, colour, t)` 的结果」——**同一个误解被写进了测试里**。
`Color.lerp` 不是那样（它缩 alpha），而且数不是颜色，`lerpDouble` 有自己的规则。
上一轮说「这条规则从来没有任何测试碰过」，那句话对 `lerp_color` 成立，对
`lerp_f32` 不成立：它被钉住了。改测试的时候把这段来龙去脉写进注释里了。

**`lerp_nearer` 是对的**，保留：形状和枚举没有中点，唯一诚实的答案就是两端之一。

一条变异逼出了第三件事。`if a == b { return a }` 我原以为只是 None-None 的快
路径，变异掉它全绿。但**两个相等的数在浮点里过一遍 `a*(1-t) + a*t` 不等于 a**——
`123.456` 在 t=0.001 上会漂到 `123.45600891`。上游那行不只是快路径，是**防漂**
（外加它自己那半句 NaN）。补了测试之后变异红。

主题仍是 25——这轮做的是三个插值规则，不是接线。

## 未接线队列之十一：`TabBar`，以及一次差点写进注释的错话（2026-08-21）

先做了个顺手的扫查：全 crate 里 `t < 0.5` 的地方还有二十来处，逐个看过——**都
是对的**。渐变、形状、枚举、亮度这些本来就没有中点，上游 `ShapeBorder.lerp` 自己
也是这么退化的；那两条错的只有 `lerp_color` 和 `lerp_f32`，都已改。

然后 `TabBarTheme`。这次不是「没有解析器」——**`ResolvedTabBar` 早就在，六个字段
写得好好的，只是没有任何人调用它**。补的是链上缺的几步和缺的三个字段，再把
`TabBar::resolved` 接上。

**五步链，不是三步**：`labelColor ?? theme.labelColor ?? labelStyle?.color ??
theme.labelStyle?.color ?? default`。文字样式里的颜色算数，但排在两个显式
`labelColor` **之后**。上游注释解释了为什么排这么后：提前会是个没有迁移方案的
破坏性改动——所以**不那么具体的那个位置反而优先级更高**。

**一句差点写错的话。** 我起初打算写「指示器跟随选中标签的颜色，因为下划线和它上面
那个词是一个记号」——听着很有道理。查了上游：`_TabsPrimaryDefaultsM3.indicatorColor`
就是 primary 本身，和标签颜色无关，重新配色标签**不会**动下划线。查证省下了一句
编得很像回事的假话。

还有一条上游明说、这里表达不了的：如果 `labelColor` 是个 `WidgetStateColor`，
上游会用它同时解析出选中和未选中两个颜色，并且**完全忽略** `unselectedLabelColor`。
Dart 的 `WidgetStateColor` **是** `Color` 的子类，`Color?` 字段能装下它；Rust
没有这种子类型关系。记下来，不绕开——把这里的字段改成 `StateProperty` 会是另一套
API，不是这一套。

7 条测试、7 条变异，第一轮全红。主题 25 → 24。

## 未接线队列之十二：按下的按钮不回落到静止高度（2026-08-21）

上一轮发现 `ResolvedTabBar` 早就写好只是没人调，所以先查了一遍：**21 个解析器里
只有 1 个是这种情况**——`ResolvedFloatingActionButton`。其余没接线的主题是真的
连解析器都没有。接上了这一个。

FAB 的五个高度里有一条上游写法值得记：**`hoverElevation ?? elevation`、
`focusElevation ?? elevation`、`disabledElevation ?? elevation` 都回落到静止高度，
而 `highlightElevation` 不回落。** 按下是唯一一个「相对手指更低」的状态——
借用静止高度会把整个按压效果抹平。

顺序也是承重的：disabled 先于 held 先于 hovered 先于 focused。一个按钮可以同时
处在好几个状态里，而高度只有一个。

8 条测试、6 条变异。**一条一开始活着**：把 widget 那侧 hover 和 focus 的顺序对调，
没有测试能看见——因为我只单独设过其中一个。补了「两个高度都设、两个状态都在」
那一例才红。**只设一个字段的测试看不见字段之间的顺序**，这轮和上轮（两个 elevation
都设）是同一个教训的第二次。

主题 24 → 23。

## 第四把尺子：把「顺序」这件事机械地问一遍（2026-08-21）

连着两轮栽在同一个形状上：解析器按某个顺序挑值，而每条测试只设一个字段，于是
**字段之间的顺序对所有测试都是不可见的**。每条测试单看都很完整——这正是读测试
读不出来的原因。

所以写了 `tools/order_sweep.py`，把这个问题机械地问一遍。它只改顺序、不改别的：

* `if states.contains(A) { … } else if states.contains(B)`——**只对调两个条件、
  保留两段函数体**，那正好就是一次重排；
* `x.field.or(y.field)`——对调回退链的两端。

**第一次跑：7 个分支对、39 条 or 链，28 个swap 没有任何测试察觉。** 分支那 7 个
全被抓住（上一轮刚补的），28 个全在 or 链上——绝大多数是三步链和 merge 函数的
方向：`self.x.or(other.x)` 这种，只要有一边没设，方向就看不见。

这轮补了 9 处的测试（`IconThemeData::merge` 八个字段、`Icon` 六条轴、
FAB 三条状态高度回落、SnackBar 的 width、`DefaultSelectionStyle::merge` 的
selection_color），28 → 16。

**工具自己也报了三个假阳性**：它把**我写的注释**里引用的表达式也当成代码改了——
比如 `// \`bar.width.or(data.width)\` -- 只设一边看不见方向`，改完当然什么都不
变，于是报「存活」。三条里有三条是我在解释这条规则时顺手引用它自己造成的。加了
跳过注释的判断。

剩下 16 条是队列，各在 `cupertino_refresh`、`paginated_data_table`、`render`、
`semantics`、`services/system` 里，都是同一形状：merge/回退链的方向没人测。

## 顺序扫查清零，以及一条**因为什么都没发生而通过**的测试（2026-08-21）

把上一轮那 16 条清完了：`AnimationStyle::at_most` 四个字段、
`CupertinoAdaptiveTextSelectionToolbar::is_empty` 的 `children ?? buttonItems`、
`PaginatedDataTable` 的行高两条、`RenderSliverList::seed_extent`、
`SemanticsConfiguration::absorb` 五个滚动字段、`SystemUiOverlayStyle::copy_with`。
**`python tools/order_sweep.py` 现在报 0。**

写这批测试的时候自己踩了一个更深的坑。`SemanticsConfiguration::absorb` 开头有
`if !child.has_been_annotated { return; }`，而我构造 child 时没设那个标志——于是
**absorb 什么都没做**，而我那条测试断言的是 parent 自己的值，所以它**照样通过**。
`absorb` 就算是个空函数它也通过。是姊妹测试（「只有 child 有的字段要浮上来」）
炸了才把它揪出来。

这跟工具的立意是同一件事：一条测试单看很完整，而它成立的理由可能和被测的东西
毫无关系。**测「什么被保留了」的测试，在什么都没发生时最容易通过。**

顺带记下工具的一个盲点：`copy_with` 里八个字段只有两个被扫到——另外六个的接收者
换了行，而 `x.field.or(y.field)` 这个模式只匹配同一行上的接收者和字段。补的测试
把八个都覆盖了，但**工具只看得见其中两个**。

## 上一轮那个「0」是虚的：两边都设不等于两边不同（2026-08-21）

上一轮结尾我自己记下了工具的一个盲点——`x.field.or(y.field)` 只匹配同一行上的
接收者和字段，而 rustfmt 会把长链断行，于是 `copy_with` 八个字段里只有两个被扫到。
**报出「0 swaps nothing noticed」的同时留着一个已知的洞，正是这条线一直在别人的
判断里挑出来的毛病。** 所以先把它补上：模式改成允许换行，可见的 or 链 39 → 56。

**三条新的存活，两条是我自己造成的**：`copy_with` 的两个 contrast 标志，我在
测试里给两边都写了 `Some(true)`——**两边都设了，但两边一样**，于是对调之后值不变，
swap 看不见。`Icon` 的 `apply_text_scaling` 同样。

这是「两边都设」这条规矩的下一层：**两边都设是必要的，不是充分的——它们还得
不一样。** 一个布尔字段最容易犯这个，因为「设上」的默认想法就是 `Some(true)`。

改成两边取值相反之后，56 条全部被抓，工具在更宽的网下报 0。

## 未接线队列之十三：三个默认成同一个颜色的字段（2026-08-21）

`ToggleButtonsTheme` 十几个字段没人读，`ToggleButtons` 控件里几何、命中测试、
两条 assert 都写得很细，就是不碰主题。补了 `ResolvedToggleButton` 并接上。

**三个边框颜色字段默认成同一个颜色**（`onSurface` 的 12%），这不是冗余：字段在那儿
是为了让主题**能够**区分三种状态，而默认不区分——一排 toggle button 默认是一整块
带边框的东西，它的分隔线不随选择移动。默认值若各不相同，读者沿着一排点过去时
整排会闪。

**标签颜色默认是区分的**，因为那才是信号：选中用 primary、未选中 `onSurface` 87%、
禁用 38%。

**`renderBorder: false` 在解析宽度之前就短路了**：设了 `borderWidth` 又把边框关掉的
调用者，得到的是「没有边框」而不是「零宽边框」——这两者在开销上不是一回事。

还有一条分支顺序：上游是 `onPressed != null && isSelected` 在最前，也就是
**选中只在这一排还能用的时候才算数**——一个被禁用的选中项按禁用画，不按选中画。

8 条测试、7 条变异全红。主题 23 → 22。

## 未接线队列之十四：伸展开的导轨不能再要标签（2026-08-23）

`NavigationRailThemeData` 十几个字段没人读。补了 `ResolvedNavigationRail` 并接上。

**上游构造函数里的那条 assert 才是内容**：
`!extended || (labelType == null || labelType == none)`。伸展开的导轨**按定义**
就是每个标签都在图标旁边，再叠一个「只显示选中的」不是偏好而是矛盾——没有一种
排布能同时满足两者。

而且这条检查要对**解析后**的 labelType 跑，不是对控件自己的：一个设了 extended、
没动 labelType 的导轨，在主题要求显示标签时**仍然是错的**，而那正是调用者自己看
不出来的一种。这和 SnackBar 那条 floating-only 断言是同一个形状。

两处默认值值得记：**`groupAlignment` 默认 -1，是顶部不是中间**——导轨是一列从
第一项往下读的东西，居中会让第一项在每种屏幕高度上落在不同位置；**标签默认关**，
因为指示器已经说明了哪个是当前项，标签是给没有指示器时用的。

两个宽度（80 / 256）是**两个独立的数而不是一个缩放**：80 是一个图标加周围的空间，
256 是一列文字。

7 条测试、7 条变异全红。主题 22 → 21。

## 未接线队列之十五：两个默认成同一个数的边界（2026-08-23）

`DataTableThemeData` 十几个字段没人读。补了 `ResolvedDataTable` 并接上。

**`dataRowMinHeight` 和 `dataRowMaxHeight` 都回落到 `kMinInteractiveDimension`
（48）**——所以一张没配置过的表，行高**恰好固定**在 48。这两个字段存在是为了让
行**能伸缩**，而在其中一个被动之前，没有伸缩可言。

由此出来一条从外面很容易搞错的事：**只抬高最小值会让两者交叉**。上游断言
`dataRowMinHeight <= dataRowMaxHeight`，而两者默认是同一个数——于是「只设一个
字段」就足以写出一个矛盾。这条检查要对**解析后**的两个值跑（和 SnackBar、
NavigationRail 那两条是同一形状：断言必须看解析结果，因为出错的那种情况恰恰
是「只写了一半」）。

另外两条：**表头行 56、数据行 48**——表头读一次、数据行读很多次，那 8 点是让
表头不至于读成第一条记录的东西；**复选框的边距回落到表格自己的水平边距**而不是
某个常量：复选框和别的东西在同一条边槽里，除非专门给它一个。

9 条测试、7 条变异全红。主题 21 → 20。

## 未接线队列之十六：一个默认值由另一个**解析后**的值算出来（2026-08-23）

先顺着前三轮那条线查了一遍：「断言要看解析后的值」这条规矩，**只在主题也能提供
被断言的那个字段时才成立**。`BottomNavigationBar`/`NavigationBar`/`Carousel`/`Chip`
那几个 `validate` 断的都是构造参数（项数、当前下标、elevation ≥ 0），主题碰不到，
所以对 widget 字段断言是对的。这条规矩现在有了准确的边界，不是「所有断言都要改」。

然后接 `BottomNavigationBarTheme`。**`showSelectedLabels` 默认恒为 true，而
`showUnselectedLabels` 的默认由 `_defaultShowUnselected` 算出来——shifting 时 false、
fixed 时 true**，也就是说它的默认取决于**解析后的类型**，而类型又取决于项数。

这不对称就是设计：选中的标签是告诉读者「你在哪」的东西，永远不藏；未选中的标签
恰恰在没地方放的时候才藏，而「没地方放」正是 shifting 的意思。两个给同一个默认，
要么让四项的 bar 挤成一团，要么让三项的 bar 没有标签。

后果之一：**主题只设了 type，就已经动了标签**——因为默认是从解析后的类型算的。

**又撞见一个「同一个东西移植了两遍」**：`BottomNavigationBarType` 和
`BottomNavigationBarLandscapeLayout` 在 `component_themes.rs` 和 `bottom_bars.rs`
各有一份，同名、同变体、同一个上游原型。它是在主题的解析想把自己那份交给
`BottomNavigationBar::effective_type` 时才暴露的——编译器拒绝得对。删掉主题里那份，
改成 re-export。**两个模块必须保持一致的类型不属于其中任何一个**，和
`ButtonVariant::default_colors` 那次是同一条规矩。

8 条测试、7 条变异全红。**外加顺序扫查在提交前抓到一条新写的**：
`bar.show_selected_labels.or(data.show_selected_labels)` 两边只设过一边。补上
「两边都设且相反」才清零——工具在这一轮已经开始在提交前挡下东西，而不是事后。

主题 20 → 19。

## 第五次机械提问：同一个类型移植了两遍，一共 15 处（2026-08-23）

连着两轮撞见「同一个类型有两份」（`ButtonVariant` 的颜色表、
`BottomNavigationBarType`），所以这轮直接数了一遍：**1917 个公开类型里，
15 个在两个模块各声明了一份。**

其中 **11 个是真重复**——同名、同变体、同一个上游原型，只是文档一份厚一份薄。
删掉薄的那份、改成 `pub use` 从概念所属的模块 re-export：
`ListTileControlAffinity`、`FloatingLabelBehavior`、`PopupMenuPosition`、
`TooltipTriggerMode`、`MultitouchDragStrategy`、`ScrollDecelerationRate`、
`ScrollAxis`、`ScrollPlatform`、`Orientation`、`TextSelectionHandleType`、
`ContentSensitivity`。**测试数一条没变**——也就是说，从来没有任何测试碰过「这两份
是否一致」，这正是它们能各自待着的原因。

### 其中一份两边**不一致**，而且不一致的正是协议

`ContentSensitivity` 在 `services/system_channels.rs` 里的那份，注释写明
`index()` 就是上游的枚举序号、**顺序即协议**（AutoSensitive=0、Sensitive=1、
NotSensitive=2）；而 `sensitive_content.rs` 那份是 `Sensitive` 打头。查上游：
`autoSensitive, sensitive, notSensitive`——**services 那份是对的，widget 那份错了**。

今天没有出事，因为**没有任何代码在两者之间转换**——而那恰恰是它们能不一致这么久
的原因。统一到上游的顺序，并补了一条把序号钉死在协议上的测试。

### 剩下 4 个不是重复，是重名

`AutofillClient`（这边是个注册记录的 struct，services 那边是上游的 trait）、
`BuildContext`（app 的是按 view 的构建上下文，framework 的才是真的那个）、
`PopupMenuButton`（两个不同的控件）、`TreeSliverIndentationType`。这些是**不同的
东西同名**，改名是另一码事，记下来不动。

主题仍是 19——这轮做的是去重，不是接线。

## M3 导航栏：动画时长是主题唯一插不上话的字段（2026-08-23）

`NavigationBarTheme` 接上读者，与上一轮的 `BottomNavigationBarTheme` 成对。
`ResolvedNavigationBar` 里每个字段都是 `widget ?? theme ?? default` 三步——
**只有一个不是**。

上游读的是 `animationDuration ?? const Duration(milliseconds: 500)`，
而 `NavigationBarThemeData` **根本没有时长字段**：中间那一步在两边都不存在。
本移植的 `NavigationBar::animation_duration_ms` 原本写着「`None` 用主题的，
主题的默认是 500ms」——**给一个不存在的步骤写了默认值**。文档改掉了：
两步，主题被跳过，不是链条漏了一环，而是没有可链的东西。

另一处对照：`label_behavior` 的默认是**常量** `AlwaysShow`，而
`BottomNavigationBar` 的 `show_unselected_labels` 默认要**算**出来
（shifting 时 false）。M3 的栏不 shift，就没有「标签放不下了」的那个数量，
所以这里的默认没有可算的东西——两个栏的差别正在于此。

默认高度 32 = `_kIndicatorHeight`，与指示器共用同一个常量：栏高若独立选定，
指示器就会浮在里面或被切掉。

6 个变异，6 个全红（时长走主题、时长默认改 300、高度默认改 80、
主题高度不生效、标签默认改 OnlyShowSelected、给未设的背景编个值）。
无读者主题 19 → 18。

## 导航抽屉：表面那条链在别的 widget 的主题里收尾（2026-08-23）

`NavigationDrawerTheme` 接上读者。这一轮的发现不在数值上，在**链条的长度**。

`NavigationDrawer.build` 写的是 `backgroundColor ?? theme.backgroundColor`，
然后把结果——**包括 null**——交给一个普通的 `Drawer`。阴影、表面着色、
elevation 都一样。**第三步不是被跳过了，是发生在别处**：在 `DrawerThemeData`
和 `_DrawerDefaultsM3` 里。

后果是一条不对称：包在 `NavigationDrawer` 外面的 `DrawerTheme` 能改它的背景，
而包在普通 `Drawer` 外面的 `NavigationDrawerTheme` 不能。

**而这条不对称从数值上完全看不出来**：`_NavigationDrawerDefaultsM3`
**也声明了**这四个字段，数值与 `_DrawerDefaultsM3` 一模一样——elevation 1、
`surfaceContainerLow`、transparent、transparent。`navigation_drawer.dart` 里
`defaults.` 出现 11 次，**没有一次是这四个**。它们是 `gen_defaults` 生成的
死副本，而且与活的那份一致——所以从错误的那份取值，永远不会有人发现。
真正会露出来的只有那条不对称。

另外三条：指示器的颜色和形状从 **drawer** 起步（`info.indicatorColor`），
destination 自己没有这个字段可给；`tilePadding` **一步都不走主题**
（`NavigationDrawerThemeData` 没有这个字段）；`disabledState` 是
`{disabled}` **单独一个**，选中被丢掉而不是加上去——所以一个禁用的
destination 的选中图标和未选中图标解析成同一个颜色，那个交叉淡入没有东西可淡。

11 个变异，11 个全红。**顺序扫描在提交前又抓到一条**：
`drawer.background_color.or(data.background_color)` 两边从没被同时设过
（那条「widget 胜出」的测试设的是 *DrawerTheme*，不是这条链的第二步），
交换无人察觉。补了一条两边都设且相反的。

顺手改掉两个测试脚手架里 `provide(components::Theme::dark(), ...)`——
`components::Theme` 不是 `ThemeData`，那个 provide 什么也没做，
却让人以为主题参与了答案。

无读者主题 18 → 17。

## 底部动作栏：高度参与颜色，而两条默认的着色都白算了（2026-08-23）

`BottomAppBarTheme` 接上读者，顺带修掉本移植的一处真错。

### elevation 是颜色的输入，不只是阴影的

上游把 `color` 走完三步之后**根本不画它**：
`effectiveColor = isMaterial3 ? applySurfaceTint(color, tint, elevation)
: applyOverlay(context, color, elevation)`。两条分支都吃 elevation。
所以抬高一个栏的 elevation 就是在改它的颜色，分别设了颜色和 elevation 的调用者
其实把颜色设了两遍。

### 两条分支的默认着色都不着色，理由正好相反

M3 走**用** tint 的那条分支，而把 tint 默认成**透明**——`applySurfaceTint`
在透明上短路。M2 把 tint 默认成一个真颜色 `colorScheme.surfaceTint`，
却走**不看** tint 的那条分支。两边都不着色；这个字段只有在
调用者显式设了它**并且**处在 M3 时才起作用。

值得写下来是因为两边看着都活：M2 每次 build 重算一个配色方案里的颜色然后丢掉，
M3 留着一个它确实会读的字段、递给它一个意思是「别读」的值。

### 顺手修的真错：M3 的栏自带一个没人要的缺口

上游 `widget.shape ?? babTheme.shape ?? defaults.shape`，而
`_BottomAppBarDefaultsM3.shape` 是 `AutomaticNotchedShape(RoundedRectangleBorder())`
——**非空**。本移植把 widget 的 shape 建模成一个 `has_notched_shape: bool`，
默认 false，`cuts_a_notch()` 就返回它。**两处都错**：说默认 M3 栏永不开缺口
（上游的总是带着形状），而且完全没找那个悬浮按钮（上游
`notchedShape != null && hasFab`，没有按钮就不挖）。

字段改成 `shape: Option<NotchedShape>`（与上游同型），`cuts_a_notch` 移到
resolved 上并要一个 `has_floating_action_button`。原来那条测试的**名字**
写的是对的规则——「a bar with no floating button over it has nothing to cut
around」——**body 检查的却是一个从没听说过按钮的 flag**。名字和身体不一致的测试。

另两条：M2 不定高度（`SizedBox(height: null)`，多高看孩子），M3 定死 80；
padding 的默认**不在 defaults 类里**，写在使用点的三目里，两个 defaults 类
都没有 padding 字段——链一样长，只是最后一步写在别处。

13 个变异，**一个活下来**：删掉 M2 的深色底色分支，全绿。
没有任何测试在深色 `ThemeData` 下构建过，那条臂根本到不了。补了一条
`resolve_under(ThemeData::dark(), ...)`，重跑变红。

无读者主题 17 → 16。

## 弹出菜单：两个样式字段从不见面（2026-08-23）

`PopupMenuTheme` 接上读者，并改掉本移植的一句错话。

### `text_style` 与 `label_text_style` 不是一对后备

主题里两个字段都有，很容易读成一个盖过另一个。它们不竞争：
**`useMaterial3` 决定哪条链根本会跑**。

* M3：`widget.labelTextStyle ?? theme.labelTextStyle ?? defaults.labelTextStyle`，
  三步都按状态解析。`_PopupMenuDefaultsM3` 填 `labelTextStyle`，`textStyle` 留空。
* M2：`widget.textStyle ?? theme.textStyle ?? defaults.textStyle`，三步都是平的。
  `_PopupMenuDefaultsM2` 填 `textStyle`，`labelTextStyle` 留空。

所以只设了 `textStyle` 的主题在 M3 下**什么都不做**，反过来也一样——
不是「被覆盖」，是**根本没被读**。本移植的 `PopupMenuThemeData` 原本写着
`label_text_style` 在两者都设时「supersedes」`text_style`，描述了一场
不会发生的竞争。

### 禁用色在两个不同的地方处理，而这个差别看得见

M3 没有单独的一步：禁用色从状态解析里出来，所以**调用者自己的 resolver
是最后一句话**。M2 没有状态属性可解析，上游在链跑完**之后**打一发
`style.copyWith(color: theme.disabledColor)`——盖在赢的那个上面，
包括调用者自己的 `textStyle`。

可观察的后果：M2 下调用者**没法**给禁用项上色（覆写在他们下游），
M3 下可以（覆写就在他们提供的那一步里面）。

### 两个听起来像的 padding，只有一个可主题化

`menuPadding` 是菜单自己的，`symmetric(vertical: 8)`，是主题字段。
item 的是 `widget.padding ?? (m3 ? 12 : 16)` 横向，读的是 defaults 类上的
一个 **static**——根本没有对应的主题字段。两者**互相垂直**，
所以是叠加而不是打架。

### `use_material3` 搬回主题上

上一轮为 `BottomAppBar` 记过一条分歧：上游在 `ThemeData.useMaterial3` 上分支，
本移植没有这个字段，而把 `material3` 挂在 widget 上；当时的理由是
「只有一个 widget 读它，加一个没人读的主题字段等于宣传一个不切换任何东西的开关」。
弹出菜单是第二个。**于是它搬到了主题上，也就是上游一直放它的地方**，
`BottomAppBar::material3` 删掉（上游的 `BottomAppBar` 本来就没这个字段）。

12 个变异，**一个活下来**：`ThemeData::lerp` 里把 `use_material3` 钉死成 true，
全绿——没有任何测试插值过两个在这上面不一致的主题。补了一条，重跑变红。
（另有三条因 `cargo fmt` 重排/常量重名而锚点失效，改对锚点后重跑，也都是红的。）

无读者主题 16 → 15。

## 第五把尺子：`unvaried.py`——解析读了、而测试从不改的字段（2026-08-23）

连着两轮，唯一活下来的变异是同一个物种。M2 底部动作栏的颜色分支读
`theme.brightness`，而没有任何测试在深色主题下构建过——那条臂到不了，删掉看不见。
`ThemeData::lerp` 从近端带走 `use_material3`，而没有任何测试插值过两个
在这上面不一致的主题——钉死成 true 看不见。

同一个形状：**解析读了一个字段，而整个测试集从不给它两个不同的值。**
只设了决策一边的测试看不见那个决策；两边都没设的测试连字段都看不见。
手工撞见两次，第三次该变成工具。

### 尺子自己先错了两次

**第一次**：只数测试里写过的字面值。首跑就把它自己的两个立案案例
全报成「never varied」——`use_material3` 是靠
`ThemeData { use_material3: false, ..ThemeData::fallback() }` 变的，
只**点名一次**，另一个值是默认，而默认不写在任何正则看得见的地方；
`brightness` 是靠 `ThemeData::dark()` 对 `ThemeData::light()` 变的，
**一次都没点名**。一把把自己要抓的东西全抓错的尺子，
以后每轮都要付一次假警报，不如没有。改成：默认算一档，
测试里出现 `dark()`/`light()` 算 `brightness` 的两档。

**第二次**：改完以后跑出 0，看着对了。**但它并没有对**——
把测试里所有 M2 改成 M3 再跑，还是 0。原因是默认那一档存成了
`"default true"`，和测试写的 `true` 永远不相等，于是「设成恰好等于默认」
读作两档。**一把没通过自己检查的尺子，先看起来是通过了的。**
去掉前缀之后重跑：测试里没有 M2 时报 1，恢复后报 0。

### 它现在找到的

`color_scheme` 和 `scaffold_background_color`「从没被测试设过」，
`disabled_color`「只设过一个值」。三条都不是真问题：前两个是通过
`ThemeData::dark()`/`light()` 这类 helper 变的（工具明说看不见）；
`scaffold_background_color` 其实是 `CupertinoThemeData` 的同名字段，
读模式按名字而不按类型匹配——这条限制记进了工具的文档。
`disabled_color` 当场验了一把：换成 `hint_color` 立刻红，确实有人守着。

**会做决定的字段里，0 个是测试集不变的。** 这是这把尺子的归零。

## 菜单三件套：主题的名字都没在指 widget（2026-08-23）

`MenuTheme`、`MenuBarTheme`、`MenuButtonTheme` 一次接上三个，
因为**有意思的正是它们之间的关系**。

### 挑主题的是轴，不是 widget

上游写的是
`switch (widget.orientation) { horizontal => MenuBarTheme.of, vertical => MenuTheme.of }`。
两种情况下是**同一个 `_MenuPanel`**。让 `MenuBarTheme` 生效的不是「它是个
`MenuBar`」，而是「它是横的」——`MenuBar` 恰好是横的。

所以这两个主题并不分属两个 widget，而是**一个 widget 的两种朝向**。

### 两份默认只差两个字段，而这两个差别都是那根轴

`_MenuBarDefaultsM3` 和 `_MenuDefaultsM3` 在 elevation 3、四角半径、
`surfaceContainer`、scheme 的 shadow、透明着色、主题的 visual density 上
全都一致。只差两处：

* **alignment**：bar 是 `bottomStart`，menu 是 `topEnd`。bar 的子菜单往下掉，
  menu 的往旁边飞——两者都没有别的地方可去。
* **padding**：bar 是 4 **横向**，menu 是 8 **纵向**。一行的两端和一列的两端
  不在同一根轴上。

两个差别都是同一件事的后果。**一行菜单和一列菜单之间没有别的区别。**

### 菜单行：两个 widget 共用一个主题——正好反过来

`MenuItemButton` 和 `SubmenuButton` 都返回 `_MenuButtonDefaultsM3`，
都读 `MenuButtonTheme`。面板是**一个 widget 读两个主题**，
菜单行是**两个 widget 读一个主题**。两边都是「主题的名字看起来在指一个
widget，其实没有」。

### 面板永远按「什么都没发生」解析

`_MenuPanelState.build` 的 `resolve` 无条件传 `<WidgetState>{}`。
面板是个**表面**，不是控件：被悬停的是它的行，不是它。所以一个
「悬停时换背景」的 `MenuStyle` 只会给出它的无状态答案。

### `elevation ?? 0` 够不到

那是三步之后的第四步。链的最后一步是 defaults 类，两份都给
`WidgetStatePropertyAll(3.0)`；而一个 elevation 解析成 null 的 style
会**落到那份默认上**，不是落出链外。那个 0 只在某份 defaults 不再给
elevation 时才有意义。

### 一个反例：不可观察的顺序不是测试的错

`foregroundColor` 有四条臂——pressed、hovered、focused 和兜底——
**四条全返回 `onSurface`**；`iconColor` 同样四条全返回 `onSurfaceVariant`。
只有 disabled 不同。所以把 pressed/hovered/focused 的顺序打乱**不可观察**，
不是因为没人检查，而是因为**没有东西可检查**——值相等。
`order_sweep.py` 找的是相反的情况（顺序要紧、却没人钉住）。

真正的反馈全在 `overlayColor` 上：pressed 0.1、hovered 0.08、focused 0.1、
其余透明。**一行菜单靠它背后画的东西告诉读者指针在它上面，从不靠改字的颜色。**
而且三者里 pressed 和 focused 相等，只有 hovered 更淡——
指针停在一行上，比按下它或键盘选中它，是更弱的一句话。

13 个变异，13 个全红。**提交前顺序扫描抓到两条新写的**
（`Pressed <-> Hovered`、`Hovered <-> Focused` 无人察觉），补了两条测试。

无读者主题 15 → 12。

## 搜索栏与搜索视图：同一件事，上游用两种方式说（2026-08-23）

`SearchBarTheme` 与 `SearchViewTheme` 一对接上。

### 「栏是控件、视图是表面」写在主题的类型里

`SearchBarThemeData` 的**每个字段都是 `WidgetStateProperty`**，
`SearchViewThemeData` 的**每个字段都是普通可空值**。栏可以被按下、被悬停；
视图是结果坐在上面的那个东西，不能。

这正是上一轮 [`ResolvedMenuPanel`] 记的那件事，而**上游用了两种写法**：
在那边，面板的字段*是*状态属性，而 build 把每一个都按空集解析；
在这边，根本没有可解析的东西，因为主题压根没拿到状态属性。
两种写法都在说「表面没有状态」，但**只有一种可能因为传错状态集而出错**。

### 搜索栏对「获得焦点」毫无反应，菜单行有

`overlayColor` 是 pressed 0.1、hovered 0.08、**focused 透明**——
和兜底一模一样，却仍然写了出来。而上一轮的 `_MenuButtonDefaultsM3`
给 focused 的分量**和 pressed 一样重**。

差别在于屏幕上还有什么在说这件事。获得焦点的搜索栏里有一个闪烁的光标、
有一个对准它的键盘；获得焦点的菜单行除了高亮什么都没有。
所以一个需要遮罩来指出键盘在哪，另一个会把同一句话说两遍。

### 视图的 `barPadding` 就是栏的 `padding`

都是 `symmetric(horizontal: 8)`。视图的 header **就是一个搜索栏**——
如果它给自己的输入框用另一种内边距，读起来就像原地冒出了另一个无关的框。
两者还共享 `surfaceContainerHigh`、elevation 6、透明着色，
以及 `bodyLarge` + `onSurface`（正文）/ `onSurfaceVariant`（提示）。
**视图就是长大的栏。**

### 只除了：栏有上限，视图没有

栏是 `minWidth 360, maxWidth 800, minHeight 56`；
视图是 `minWidth 360, minHeight 240`，**根本没有最大宽度**。
一行文字过宽就不好读，所以栏封顶；一片放结果的区域不会，所以不封。
56 对 240 是同一句话的另一种说法：一个是一行，一个是一块地方。

### 全屏把圆角去掉

停靠时 28 圆角，全屏时是个平直矩形——全屏视图没有角可以圆，
圆了就成了一张浮在并不存在的背景上的卡片。这是**唯一一个默认值
依赖既非主题也非 widget 的东西**：`isFullScreen` 是 defaults 类自己的
构造参数，和 `context` 一样。（顺带钉住：全屏**只**改形状，别的都不改。）

14 个变异，14 个全红。顺序扫描又在提交前抓到新写的那条遮罩阶梯
（`Pressed <-> Hovered`），已补测试。

无读者主题 12 → 10。

## 一个主题不该有读者，另一个只为一个布尔值活着（2026-08-23）

这一轮本来打算把 `ButtonTheme` 和 `ButtonBarTheme` 一起接上，
结果是**其中一个接不得**。

### `ButtonBarTheme`：不接才是对的

写完 `ResolvedButtonBar` 之后去核账本里那句「`ButtonBar` 已 `@Deprecated`，
对应到 `OverflowBar`」——**账本是对的**：上游
`@Deprecated('Use OverflowBar instead')` 同时标在 `ButtonBar` 和
`ButtonBarThemeData` 上。而唯一读这个主题的就是 `ButtonBar`。

所以给它造一个读者，就是**给一个本移植故意不要的 widget 写解析器**——
正是我一直在挑上游毛病的那种死代码。已写的 `ResolvedButtonBar` 删掉
（6179 字符），它记的东西挪进 `ButtonBarThemeData` 自己的文档里：
类型还在，就由它来说明为什么没人读它。

顺带记下它当年决定的事，因为这是真的：
`ButtonBar.build` 拿 `ButtonTheme.of` 做**底子去 copyWith**，
而不是把它当链上的一步——六个字段无条件盖掉，
活下来的是 `ButtonBarThemeData` 没有字段的那些。**一个 bar 重新量它的按钮，
但不重新给它们上色。**
而间距全出自 `paddingUnit = padding.horizontal / 4`：上游注释说那个 4 是
「左右内边距平均值的一半」——`horizontal` 是左加右，一次除法求平均，
另一次取一半。取一半才让算式合上：每个孩子裹 `symmetric(horizontal: unit)`，
**两个相邻孩子之间是两个一半，也就是整个 8**；两端 bar 再补自己的 unit，
加上端头孩子已有的 unit，也是 8；纵向是 `2 * unit`，还是 8。
**一个数出现在四个地方，而除数里的 4 就是把它放到那儿的东西。**

### `ButtonTheme`：整个主题为一个布尔值活着

> **这一节是错的，2026-08-23 更正见文末「MaterialButton」那一轮。**
> `ButtonTheme.of` 有**第四个**调用点 `material_button.dart:394`，
> 而那才是这个主题真正服务的 widget。下面这句话建立在一次被 `head -12`
> 截断的 grep 上。`alignedDropdown` 那些发现本身站得住，
> 「整个主题为一个布尔值活着」这句不站得住。

`ButtonTheme.of` 上游只被读三次：一次在 `ButtonBar`，另两次在
**`DropdownButton`**，且两次都只为 `alignedDropdown`。
一个完整的主题为一个布尔值留着，而读它的 widget 不是按钮。

那个布尔值做的事，比「把内边距从菜单挪到按钮」要精确一点：
aligned 时按钮 `start 16, end 4`、菜单 margin 为零；
unaligned 时按钮为零、菜单 `start 16, end 24`。
**start 那 16 确实在换手**（永远只有一边扛着），
**end 完全不换**——4 对 24。因为两个 end 在干不同的事：
aligned 按钮那 4 是箭头旁边的余地，unaligned 菜单那 24 是与没对齐的东西之间的
净空。按「整体搬家」去理解，start 会对，end 会错二十个像素。

还有一层不对称：菜单 margin 只看 `alignedDropdown`，
按钮 padding 看 `alignedDropdown && _inputDecoration == null`。
**这面旗子只生效一半，而是哪一半，取决于旗子从没听说过的东西。**

9 个变异，9 个全红。

### 尺子第三次修：`unwired.py`

**一、调用点模式太窄。** 原来找 `Resolver::of(`；`DropdownAlignment`
从 `from_theme` 读主题、`of` 只收纯参数，于是一个**正在被调用**的解析器
看不见。放宽成 `Resolver::<任意>(`。这和它当初漏掉 `ResolvedButton`
是同一类盲点。

**二、不是每个没人读的主题都是缺口。** 加了「上游已废弃」一档，
单独列出、不计入队列。而且判定要**跟着 data 类走**：上游把
`ButtonBarThemeData` 和 `ButtonBar` 都标了废弃，**中间那个
`ButtonBarTheme` 这个 `InheritedWidget` 却没标**——只 grep 包装类
会把它当活的。

**三、又一次「找不到」和「没得找」长得一模一样。** 新扫描一开始返回空集，
报告只是安静地多列了一个主题。原因是 heredoc 把正则里的 `` 变成了一个
真的退格符（这个坑我知道，还是踩了；教训仍是正则密集的 Python 用 Write/Edit 写）。
修好之后加了一句守卫：上游目录在、却一个废弃主题都没找到，就明说扫描坏了。
把正则再弄坏一次验过，它会喊。

无读者主题 10 → 8（另有 1 个上游已废弃，不计）。

## 图标按钮：同一个主题，两个动词，两种优先级（2026-08-23）

### `IconButtonTheme` 被四个 widget 读，用了两个不同的动词

`IconButton.themeStyleOf`：
`IconButtonTheme.of(context).style?.merge(iconThemeStyle) ?? iconThemeStyle`。
`ListTile.build` 和 `AppBar.build`：
`IconButtonTheme.of(context).style?.copyWith(foregroundColor: ...)`。

`merge` 保留自己的字段、只在自己为空处取对方的，所以**主题赢，
环境 `IconTheme` 补空**。上游自己的文档就是这么写的：
*「if any of the properties exist in both [IconButtonTheme] and [IconTheme],
[IconTheme] will be overridden.」*
`copyWith` 则是直接替换点名的字段，所以**读者赢**。

同一个主题，相反的答案，而调用者拿到哪个取决于他把按钮放进了什么。
这不是不一致：**一个裸的 `IconButton` 对身后是什么没有意见，理应让位；
而 `ListTile` 或 `AppBar` 自己画了背景，必须压上一个在那背景上读得出来的颜色。**
一个能把 app bar 的动作图标染到看不见的主题，就是一个能弄坏 app bar 的主题。

### merge 是逐字段的，`??` 阶梯不是

这个区别在这里真的起作用：只设了前景色的主题，**仍然让 `IconTheme` 的尺寸通过**，
因为 merge 一个字段一个字段地问。写成 `themeStyle ?? iconThemeStyle`，
第一个非空的 style 会把整块拿走，设一个字段就会**悄悄丢掉另一个**。

### 环境图标主题在进门前先被过滤

`iconThemeStyle` 是 `isDefaultColor ? null : iconTheme.color`（尺寸同理），
所以 `IconTheme` **只贡献被人特意设过的东西**。这是把它 merge 在主题下面
仍然安全的原因：它没法把主题正想替换掉的那个兜底again 顶回来。

### 一处本移植跟不了上游的地方

`isDefaultColor` 用的是
`identical(iconTheme.color, kDefaultIconDarkColor)`——**对象同一性，不是相等**。
Dart 里同值的 `const` 颜色会被规范化因而 identical，而同值的非 const 颜色不会，
会被当成「特意设过」。也就是说行为取决于调用者那个颜色是不是 const。

Rust 的 `Color` 是 `Copy`，没有同一性可比，所以本移植按值比。
const 那种（也是有文档的、常见的那种）与上游一致；
非 const 而恰好等于默认值的那种不一致——上游当它设过，这里当它没设。
**记下来而不是糊过去**：这是真差别，而另一条路是给 Rust 发明一个它没有的同一性。

10 个变异，10 个全红。

### 尺子第四次修：`unwired.py` 的 `wrappers()`

`TextTheme` 一直挂在无读者队列里。上游 `class TextTheme with Diagnosticable`
是个**数据类，根本没有 `.of`**，靠 `Theme.of(context).textTheme` 取用——
它被读得到处都是，而报告要求的那种读者它永远不可能有。

原因是 `wrappers()` 按**名字形状**判定：「叫 `XTheme` 且不叫 `XThemeData`」。
改成按行为判定：**impl 里有 `fn of(` 才是包装类**。

这和 `resolvers()` 当初需要的修正是同一件事。把这把尺子四次出错串起来看，
**全部都是名字形状的启发式在冒充行为形状的判定**：
找 `Resolved<主题名去掉 Theme>`、找 `Resolver::of(`、
grep 包装类而不看它承载的 data 类、以及这次的 `wrappers()`。

主题总数 49 → 48（`TextTheme` 本就不该在名单上），
无读者 8 → 6（另 1 个上游已废弃，不计）。

## 输入框的边：形状和边线来自两个不同的地方（2026-08-23）

`InputDecorationTheme` 接上读者，也是队列里最大的一个。

### 形状是调用者的，边线是主题的

`_getDefaultBorder` 拿 border 只为它的**形状**（下划线、外框、随便什么），
然后 `copyWith(borderSide: ...)` 用一个来自别处的边线把它换掉。
**调用者说这个框长什么样，主题说它用什么颜色多粗画出来。**

### 报错压过禁用

五选一写的是
`!enabled ? (error ? errorBorder : disabledBorder) : focused ? (...) : (...)`。
`errorBorder` 出现在三条臂中的**两条**，其中一条就是禁用那条：
**一个你改不了的字段，仍然会告诉你它是错的。**
改不了不是停止被告知的理由。

六个格子五个名字，唯一共用一个名字的就是那两个 error 格；
而 focused+error 反倒有自己的 `focusedErrorBorder`。

### 两种「保留你给的边」

* border 是个 `WidgetStateProperty<InputBorder>`——调用者已经在按状态回答了，
  下面那套（也按状态解析）再来一遍就是在替他改主意。
* border 的边是 `BorderSide.none`——换掉它就是给一个说了不要线的边框
  把线放回去。而**没有边的 border 仍然定义一个形状**，所以那是个决定，不是缺失。

### filled 与否读不同的字段，而只有一个读主题

filled 走
`InputDecorationTheme.of(context).activeIndicatorBorder ?? defaults.activeIndicatorBorder`；
不 filled 走 `defaults.outlineBorder`**一个**。

`defaults` 是 `_InputDecoratorDefaultsM3(context)`，不是环境主题；
而上面那句把主题折进 decoration 的 `applyDefaults` **两个字段都不带**
（两者都没有 `InputDecoration` 里的对应项）。所以
**`InputDecorationThemeData.outlineBorder` 是一个公开、有文档、而 decorator
从不读其值的字段**——它在文件里另一次出现是在一条只问「有没有任何字段非空」
的 `??` 链里。

filled 那条分支**又去 `InputDecorationTheme.of(context)` 拿了一次**
（明明主题已经折进 `decoration` 了）就是那个破绽：有人需要一个
`applyDefaults` 不带的主题字段，就直接去取了。他为 active indicator 取了，
没为 outline 取。

### 两条默认梯子是同一条梯子，差两个值

`activeIndicatorBorder` 和 `outlineBorder` 六条臂顺序完全相同，只差两处：

* **禁用**：indicator 是 `onSurface@0.38`，outline 是 **`@0.12`**。
  **围得越多的形状，死掉时越淡三倍。** 文字下面一条横线得还看得出是条线；
  绕着一个死字段画一整圈还那么浓，会读成活的。
* **静息**：indicator 是 `onSurfaceVariant`（近乎内容，取文字角色），
  outline 是 `outline`（容器边，取给它起的那个角色）。

其余全共享。而 error 臂里 focused 仍然决定宽度：
**颜色说哪里错了，宽度说你在哪。**

M2 那条宽度梯子把三个不同的理由折成一个零：collapsed、border 为 none、
禁用——画出来一样，只是理由不同。

14 个变异，14 个全红。无读者主题 6 → 5（另 1 个上游已废弃，不计）。

## 分段按钮：八条臂两个答案，和一次我自己造成的污染（2026-08-23）

### 只有两个状态要紧，而上游照样写了八条臂

`foregroundColor` 给选中和未选中各写了四条臂（pressed/hovered/focused/兜底），
**每组四条全返回同一个颜色**：选中 `onSecondaryContainer`，未选中 `onSurface`。
只有 `selected` 和 `disabled` 会改答案。

标签不对触碰起反应，和 [`ResolvedMenuButton`] 一样，理由也一样——反馈在遮罩里。
这里不同的是这条阶梯是**成倍的**：八条写出来的臂塌缩成两个值，
因为生成器不管 token 是否不同，都会把整个叉积写出来。

### 禁用的分段没有容器，选中与否都一样

`backgroundColor` **先**查 disabled 并为它返回 null，未选中也返回 null。
所以只有「选中**且**启用」才有底：把一个选中的分段禁用，是把那颗药丸**拿走**，
不是把它调淡。

乍看是反的，直到你看还剩什么：勾还在、外框还在、标签还在 38%——
三种方式说「就是这个，而你不能用」。再来一个调淡的容器就是第四种，
而那一路还得继续为旁边那些分段工作。

### 两种「没有」的写法，在同一个文件里

defaults 类的 `overlayColor` 兜底返回 **null**；几行之下 `resolveStateColor`
的映射兜底返回 **`Colors.transparent`**。两者都表示没有遮罩、画出来一样，
所以没有任何东西逼它们一致——于是它们就不一致。

### 禁用外框是 0.12，和输入框边用的是同一个数

`ResolvedInputBorder::DISABLED_OUTLINE_OPACITY` 也是 `onSurface@0.12`。
两个毫不相干的组件，同一个角色——**一条描着某个死东西边缘的线**——同一个数。
而禁用的**文字**两边都是 0.38，因为那是文字。

14 个变异，14 个全红。

### 一次我自己造成的污染，以及它差点混过哪些关

我把 `cargo fmt` + `order_sweep.py` 放后台跑，**同时**在同一个文件上手动跑变异批次。
两个写者。`cargo fmt` 在变异 M9 生效时读了文件，在恢复之后把它写了回去，
**把 M9 复活了**。

后果值得记下来，因为它演示了几件事：

* 手动变异批次结束时的全量 `cargo test --lib` 报 **4551 passed 0 failed**——
  那次跑在 fmt 写回之前，所以是干净的。
* 随后的 GN 关卡报了 **1 个失败**。两个跑同一份代码、结果不同，
  这本身就是信号，不是抖动。
* 那条测试**单独跑必挂、在全量里却过**——它在全量里能过，
  是因为别的测试先跑过。这正是「测试可能因为完全不相干的原因而通过」那一类。
* 那次与批次重叠的顺序扫描报了 **0**——它量的是一棵被污染的树。
  重新串行跑之后报 **1**，是 `state_color` 里那条和 `overlay_for` 一模一样、
  却没人测过的阶梯（`Hovered <-> Focused` 无人察觉）。补了测试。

**教训**：变异工具假设自己是唯一的写者。`order_sweep.py` 自己也改写并恢复同一个文件——
两个都在改同一份源码的东西不能并行跑。修完之后，
每条新测试都**单独**跑了一遍，而不只是在群里跑。

无读者主题 5 → 4（另 1 个上游已废弃，不计）。

## 下拉菜单：一个几乎全由别人的主题组成的主题（2026-08-23）

### 这个主题本身就是别的组件的主题

`DropdownMenuThemeData` 四个字段里有三个是别人的类型：一个 `TextStyle`、
一个 `MenuStyle`、一个 `InputDecorationThemeData`。这个文件里其他每个主题都在
描述**它自己那个 widget** 长什么样；这一个说的是
**「当这些组件出现在一个下拉菜单里时，它们改成什么」**。

### 于是包在下拉菜单外面的 `MenuTheme` 够不到它的菜单

菜单样式是以 `MenuAnchor(style: effectiveMenuStyle)` 送进去的，
那是 [`ResolvedMenuPanel`] 链条上的 **widget 那一步**——在 `MenuTheme` 之上。
而 `effectiveMenuStyle` 永远非空（defaults 类会给一个）。

### 每个子主题都是**整块**取的，而这正是那个区分的另一端

`widget.menuStyle ?? theme.menuStyle ?? defaults.menuStyle`，
文字样式和输入装饰主题同样如此。**第一个非空的对象整块赢。**

[`ResolvedIconButton`] 记的是另一端：那里上游写 `merge`，逐字段合并。
这里是 `??`，所以一个为了改一件事而设了 `DropdownMenuTheme.menuStyle` 的调用者，
**连 `_kMinimumWidth`、最大尺寸和视觉密度一起悄悄丢掉了**。

### 菜单至少和开它的那个输入框一样宽

活下来的那个 `minimumSize` 随后被 `min(anchorWidth, maximumWidth)` 覆盖掉
（给了 `width` 就用给的那个）。比自己的输入框还窄的下拉菜单，
看起来像是开出了另一个控件。

于是 defaults 的菜单样式带的三个字段里：`minimumSize` 只要 anchor 量过就被换掉，
`maximumSize` 只要给了 `menuHeight` 就被换掉，**只有 `visualDensity`
是原样到达面板的**。

### 那个下限的夹取，读的是最后定下来的上限

上游把下限写成一个闭包，闭包捕获的是**变量** `effectiveMenuStyle`，
而这个变量在之后又被 `menuHeight` 重新赋值。Dart 的闭包捕获变量而不是值，
所以闭包真正运行时读到的是被重新赋值之后的上限。

后果：给一个 `menuHeight`——它把上限整个换成 `Size(infinity, height)`——
**同时也把下限本来要夹的那个宽度上限抹掉了**，于是下限变成不设夹的 anchor 宽度。
**一个高度悄悄把菜单变宽了。** 本移植按「读发生的顺序」而不是「写发生的顺序」
写出来，就是 `minimum_width`。

### 下拉框的输入框默认是外框，普通输入框不是

`defaults.inputDecorationTheme` 是 `border: OutlineInputBorder()`，
而 `_getDefaultBorder` 的兜底是 `const UnderlineInputBorder()`。
下拉框是一个「按」的成分不亚于「打字」的字段，而一个框比一条线更像可按的。

14 个变异，14 个全红。每条新测试都单独跑过一遍，不只是在群里跑。

无读者主题 4 → 3（另 1 个上游已废弃，不计）。

## 时间选择器：透明和「同一个颜色」不是一回事（2026-08-23）

### entry mode 是 defaults 的第三个输入，而它只动一个字段

`_TimePickerDefaultsM3(context, {entryMode})` 接住那个模式，
方式和搜索视图的 defaults 接住 `isFullScreen` 一样——**既不是主题也不是 widget
的一个参数**。而且和那边一样，它**只改一个默认值**：
`hourMinuteTextStyle` 在表盘上是 `displayLarge`，在输入框里是 `displayMedium`。

**可编辑的时候小一号。** 表盘上的小时是个你点的目标；输入框里的是你打的字，
里面有个光标，`displayLarge` 会给它一个拇指那么高的光标。

### 输入态的框是表盘态的框减八，只减高

`hourMinuteInputSize` 是 `Size(width, height - 8)`，上游还附了一句：
规范里写的是八，但**还没有对应的 token**。宽度不动——一个输入框仍然得放下两位数字，
减掉的只是数字周围的余地。

### 24 小时的框更宽，高度一样

114 对 96。那个模式下旁边没有 AM/PM 选择器了，它原本占的宽度归了数字，
而不是归了边距。两处调整**互不相干、可以叠加**：一个动宽，一个动高。

### 未选中的 AM/PM 是透明，而不是对话框的颜色

上游在注释里给了理由，而这句话读起来像是多此一举，直到你看见原因：
*「Making it transparent enables that without being redundant and allows the
optional elevation overlay for dark mode to be visible.」*

在 `surfaceContainerHigh` 上再画一层 `surfaceContainerHigh`，**和什么都不画不是一回事**，
因为深色模式下对话框的 elevation overlay 就夹在这两层中间。
**透明与同色，恰恰在下面还垫着东西的地方分道扬镳。**

### 时分文字又是八条臂两个答案

选中的四条臂全返回 `onPrimaryContainer`，未选中的四条全返回 `onSurface`，
和 [`ResolvedSegmentedButton`] 一样。交互在 `hourMinuteColor` 里——
一层遮罩混在 `surfaceContainerHighest` 上，还是那三个浓度
（按下 0.1、悬停 0.08、聚焦 0.1）。

14 个变异，14 个全红。**其中六条第一次锚点没对上**——
`component_themes.rs` 已经大到 `ELEVATION`、`HOVERED_OVERLAY` 这些常量名
在多个 `Resolved*` 里重名（`ELEVATION` 出现 6 次）。
把锚点限定在 `impl ResolvedTimePicker` 的范围内重跑，六条也都是红的。
**变异工具的「锚点必须唯一」这条断言又一次挡住了一次假通过。**

每条新测试都单独跑过一遍。无读者主题 3 → 2（另 1 个上游已废弃，不计）。

## 日期选择器：唯一一个「选中压过禁用」的组件（2026-08-23）

### 这一个反过来了

`dayForegroundColor` **先**查 `selected`，**后**查 `disabled`。
本文件里其他每一条阶梯都是反着来的——[`ResolvedSegmentedButton`]、
[`ResolvedMenuButton`]、[`ResolvedInputBorder`]、[`ResolvedNavigationDrawer`]
全是禁用赢，只有这一个不是。

所以一个既被选中又被禁用的日子，画出来**仍然是选中的样子**：有底、`onPrimary`，
而不是变淡。这里这样才对，因为**选择器的选中项就是它当前持有的答案**。
一个日子被禁用，恰恰是它落在可选范围之外的时候——那正是调用者最需要看见
选择器里装着什么的时候：把它变淡，就是把值藏起来，而选择器还拿着它。

一个禁用的分段或菜单行只是「用不了」，调暗它不损失什么。
而一个禁用的**选中项**是一个需要有人去解决的状态。

### 日子是圆的，年份是胶囊

`dayShape` 是 `CircleBorder`，`yearShape` 是 `StadiumBorder`。
一两位数字塞得进圆，四位塞不进——胶囊就是一个放弃了当圆的圆。
**形状跟着内容有多宽走**，不是偏好。

### 今天没有自己的底

`todayBackgroundColor` **就是** `dayBackgroundColor`——上游返回的是另一个 getter，
不是它的副本。今天是靠**边框和文字颜色**标出来的，所以没有一个自己的底
去和选中的底打架，选中今天时直接复用日子的底就行。

两条前景阶梯从另一面说了同一件事：今天是 `primary` 而普通日是 `onSurface`，
禁用时是 `primary@0.38` 而普通日是 `onSurface@0.38`——
**但选中时两者都是 `onPrimary`**，因为那时两者都坐在同一个主色圆上。
**两条阶梯恰好在背景变了的那一条臂上汇合。**

### 范围选择器：搜索视图写成分支的那条规则

对话框是 elevation 6、28 圆角；`rangePickerElevation` 是 **0**，
`rangePickerShape` 是个平直矩形。范围选择器是全屏的，
而全屏的表面没有角可以圆，也没有东西可以浮在上面。

[`ResolvedSearchView`] 是在**一个默认值内部**对 `isFullScreen` 分支得出同一个结论；
这里换成了**另一整套字段**。一条规则，两种编码——
而后者正是这个主题为什么有二十来个 `rangePicker` 前缀字段。

### 一个默认值读另一个默认值

`toggleButtonTextStyle` 是 `titleSmall.apply(color: subHeaderForegroundColor)`，
而 `subHeaderForegroundColor` 是同一个类上的另一个 getter。
副标题的颜色一动，切换按钮的文字跟着动——这是两个默认值之间的依赖，
而不是默认值与主题之间的。副标题那档浓度是 **0.60**，
和禁用的 0.38、死外框的 0.12 并列，是这个家族里的第三个数：
它不是被禁用，是**从属**，那是另一句话。

14 个变异，14 个全红。每条新测试都单独跑过。

### 顺带发现：这个仓库有另一条工作线在并行提交

`master` 上我的上一轮提交 `9380c1f` 后面已经压了 **10 个我没写的提交**
（一条 async 线：唤醒通道、`FutureBuilder`、第二个时钟、park 住的任务、
`docs/ASYNC_FLOW.md`），还有一个 `async-gn-verify` 分支。

这解释了几件之前对不上的事：`cargo test --lib` 在同一轮里从 4588 跳到 4619；
工作区里有一处我没做过的 `BUILD.gn` 改动（给 `debug_rendering.rs` 和 `task.rs`
补 `sources` 条目）。

**查过了：我这 6 个提交里没有一个夹带了别人的文件**，每个都只含
`component_themes.rs` + 对应 widget 模块 + 我自己改的 tools/文档。
这一轮改成只 `git add` 我自己的两个文件，那处 `BUILD.gn` 留给它的作者。
以往提交信息里的测试总数，是当时那棵树上的真实数字。

无读者主题 2 → 1（另 1 个上游已废弃，不计）。

## 轮播：唯一一个没有 defaults 类的组件，`unwired.py` 归零（2026-08-23）

### 它根本没有 defaults 类

这个文件里其他每一个主题，链条最后都落在一个生成出来的 `_XDefaultsM3` 上。
轮播没有：六个默认值全是**手写在 build 方法里的 `??` 常量**。

这个「没有」正是本轮系列里那些 defaults 类的发现**都不适用于它**的原因：
没有可以继承下来的死默认值（底部动作栏有四个），
没有一个默认值读另一个（日期选择器的切换按钮读它的副标题），
也没有第三个构造参数（搜索视图和时间选择器各有一个）。
**没有东西可供 `gen_defaults` 重新生成，也就没有东西会被它落下。**

而且这条链跑在 `_buildCarouselItem(index)` 里边，**每个可见项解析一次**，
不是每帧一次。这里没有任何东西依赖 index，所以答案一致——
但那是每项六条阶梯，不是每帧六条。

### 一个轮播项是平的，而这文件里别的东西都是抬起来的

elevation 默认 **0**。对话框、菜单、搜索栏、时间选择器都是 3 或 6；
轮播里的项**已经被 padding 和邻居分开了**，再抬起来就是把同一句话说两遍。

### 它的 padding 四边都有，而这里其他每一个都只在一个轴上

`EdgeInsets.all(4)`，对比菜单的 8 纵向、菜单栏的 4 横向、
弹出项的 12 横向、搜索栏的 8 横向。**轮播只往一个方向滚，
但它的项两个方向上都得让开容器的边。**

### 遮罩算了出来，却不一定有人用

`effectiveOverlayColor` 是在「用 `InkWell` 还是用光秃秃的 `GestureDetector`」
这个分支**之前**算好的，而 `GestureDetector` 没地方放它。
所以 `enableSplash: false` 给出的是一个**能按、但不出声**的项——
点击照样送到，只是什么都不画来说这件事。

它的兜底返回 `null` 而不是透明——[`ResolvedSegmentedButton`] 同时带着
「没有」的两种写法，这里是 null 那种。

12 个变异，12 个全红。每条新测试都单独跑过。

### `unwired.py` 归零

**48 个主题，0 个无读者**（另 1 个上游已废弃，不计）。
这把尺子是在 tick 26 因为「主题数据齐了、解析器齐了、widget 一个字段都不读」
这件事连着撞见三次才造出来的；它当时报 38 个无读者，中途因为四个
**名字形状冒充行为判定**的毛病被修过四次，现在到底了。

## 三个控件磁贴：磁贴才是控件，里面那个不是（2026-08-23）

主题队列清空后转到 `depth.py` 的队列。头三个是三兄弟：
`SwitchListTile`（3/46）、`RadioListTile`（3/43）、`CheckboxListTile`（5/42）。

### 先核对，再动手：那个比值是本工具自己承认的高报

本移植把三者抽成了一个共享的 `ControlListTile`，`depth.py` 数的是
`SwitchListTile` 自己身上的成员，所以报「3 of 46」——**正是它文档里写明的那种
高报**。而且我第一件想找的东西已经在了：

`ListTileControlAffinity::Platform` 在 checkbox 和 switch 上是 **trailing**，
在 radio 上是 **leading**（上游三个文件的 `switch` 表达式确实不一样：
radio 那份是 `platform => (control, secondary)`）。**同一个枚举值、同一个默认、
相反的边。** 本移植的 `resolve` 已经写对了，还带着一个
`consults_the_platform()` 明说它从不问平台。

### 真正缺的三样，说的是同一件事

1. **`ExcludeFocus`** 包着控件——三个磁贴、每个的两条分支里都有。
   **磁贴才是焦点停靠点，里面那个控件不是。** 否则 Tab 会在一行上停两次，
   而第二次停下来做的事和第一次一样。
2. **`materialTapTargetSize ?? shrinkWrap`**——一个裸的控件默认是 `Padded`，
   会把自己撑到 48 的最小值；在磁贴里这个默认被**推翻**，因为磁贴才是点击目标，
   在一个 48 高的行里再套一个 48 高的目标什么也没买到，只是把行变高。
   **磁贴在这里不是顺从控件，是在和它唱反调。**
3. **`MergeSemantics`**（早已移植）——对读屏器来说是**一个**东西。

三种机制，一句话：**一个控件放进列表磁贴，就在读者能够到它的每一条路上
——键盘、手指、读屏器——不再是一个独立的控件，而只剩下「把状态画出来」这一件事。**

顺带记下 `ExcludeFocus` 关的是**四扇门**，而只有一扇归那个 flag 管：
`canRequestFocus: false`、`skipTraversal: true`、`includeSemantics: false`
是常量，只有 `descendantsAreFocusable: !excluding` 跟着走。
上游还特意说明它**不**改后代自己的 `canRequestFocus`，
而且被它挡掉焦点的东西在它关掉之后**不会被重新聚焦**——排除不是暂停。

### 而 `.adaptive` 正是这些磁贴唯一问平台的地方

`_SwitchThemeAdaptation.adapt` 读的是 **`ThemeData.platform`，不是设备**，
而它在 iOS/macOS 上返回的是 `const SwitchThemeData()`——**一个空主题**。
所以这里的「adaptive」不是「在苹果平台上换一套主题」，
而是**「把你拿到的那套忘掉」**。

对照留在文档里：`ListTileControlAffinity::Platform` **名字里带 platform，
却从不问**；`.adaptive` **名字里没有，却总是问**。

13 个变异，13 个全红（其中三条因 `cargo fmt` 重排锚点失效，改对后重跑也是红的）。
每条新测试都单独跑过。

## `CupertinoDynamicColor`：那条分歧的理由只覆盖了它两条里的一条（2026-08-23）

### 一处被移植的类型，为一个不存在的东西而存在

模块文档里写着：「`CupertinoDynamicColor` 的高对比度变体已丢弃——
平台桥接带了亮度但没有对比度设置」。这条理由**同时覆盖了被丢掉的两组列**，
而它只对其中一组成立：

**高度不来自平台。** `CupertinoUserInterfaceLevel` 是树里的一个 widget
（一张 sheet 把它盖住的东西抬高一级），而**本移植一直就有它**，
文档还明写着它存在的意义是「在 dynamic color 的 base 与 elevated 之间挑一个」
——**而那时候根本没有 elevated 可挑**。

八列里的四列（非高对比度那一半）现在带上了。高对比度仍然缺，
但缺的理由是真正关于它自己的。

### 高度从不动亮色值

上游表里声明了 elevated 变体的十八个颜色，**`color == elevatedColor` 无一例外**。
只有深色那半会动。这跟着「抬高」的含义走：**深色里靠变亮来抬，
而亮色里没有比白更亮的地方可去。**

### 而且动的只有表面，不是画在表面上的东西

`label`、几个 fill、`link` 都不随高度变；六个背景变。
**一个表面升起来的时候，画在它上面的内容不变，变的是表面。**
`separator` 变而 `opaqueSeparator` 不变，是同一条规则从另一头看：
半透明的分隔线透出它背后的东西，就得跟着；不透明的没有可跟的。

### 六个背景抬一级，恰好等于往下走一级角色

`systemBackground` 抬起来 = `secondarySystemBackground` 的深色值；
secondary 抬起来 = tertiary 的；tertiary 抬起来再往下一步，
落在三个有名字的角色之外。分组那三个用同样的灰重复一遍。

**iOS 说「这个盖在那个上面」的两种方式——编号角色和高度特性——落在同一个灰上。**

`separator` 是唯一一个非背景的例外，而且它跑得更远：
84,84,88 到 210,210,210（同样的 alpha 153）——它得跑过它下面那个变亮的表面
才能继续是一条线。

### 那两个依赖标志不是优化

上游的 `_is*Dependent`：一个亮暗两半相同的颜色**不去查**亮度。
重点不在省下的那次比较——**不查就是不依赖**，于是用这个颜色画的 widget
在外观切换时不会被重建。**是个穿着优化外衣的依赖决定。**

13 个变异，13 个全红。**其中一条活了三次才被钉住**：
把梯子检查改成 `.take(1)`（只查第一对）全绿。原因是那个断言函数读的是一个
常量，而**没有任何测试能递给它一个坏的梯子**——读常量的谓词证明不了自己会说「不」。
改成把梯子当参数传进去，测试才能喂它一个坏的；而且那对坏的**必须放在第二位**，
否则只查第一对的版本照样会答「不」，测试通过却什么也没证明。

### GN 关卡又一次抓到 `cargo test --lib` 看不见的东西

`resolve` 从一参变两参，gallery 里两处调用点没跟上——**lib 测试全绿，
gallery 编译不过**。这正是那道关卡存在的理由，和当初加 `ButtonVariant::Elevated`
时是同一类。顺手把 `CupertinoUserInterfaceLevel(Data)` 加进了 crate 的再导出。

## `SemanticsNode`：三种「不在屏幕上」，而它们不是同一回事（2026-08-23）

两个裁剪矩形那半**早就做好了**——`applied_to` 已经把语义裁剪和绘制裁剪
分两步做，两者之间那圈正是上游 `hidden` 的位置，而「本桥接没有 hidden 标志」
这条分歧也在模块文档里带着理由。核了一遍：`SemanticsFlags` 里确实没有那一位，
理由仍然成立——**所以没有去加一个桥接送不出去的标志**
（那正好会造出 `unwired.py` 当初为之而生的那种东西）。

真正缺的是三条节点级的规则：

### `isInvisible` 是一张「可以丢掉」的许可证，所以那个 merged 守卫才在那儿

上游：`!isMergedIntoParent && (rect.isEmpty || transform.isZero())`，
并且写明了它是干什么的——「一个隐形节点可以安全地从语义树里丢掉，
而不损失任何描述当前屏幕内容的语义信息」。

**一个被合并进父节点的节点没有自己的几何可供评判**——它的标签正作为
父节点标签的一部分被读出来——所以空矩形关于「它在不在屏幕上」什么也没说，
丢掉它会丢掉那些字。**那个守卫不是优化，它是「没有尺寸」和「没有意义」之间的区别。**

（本移植不带 zero-transform 那一支：这里的几何是矩形不是矩阵，
一个塌成零的变换到这儿已经是空矩形，被第一个子句抓住了。）

### `indexInParent` 数的是不在这儿的那些孩子

上游自己的例子：一个 scrollable 有五个孩子、前两个不可见，
于是只有三个节点，而最后那个的索引**仍然是 4**。
索引是「读者被告知的那个列表」里的位置，不是「裁剪后活下来的数组」里的位置——
**这才让读屏器能说「第 5 项，共 5 项」，而节点只有三个。**
按 `children.len()` 去算会说「第 3 项，共 3 项」，悄悄告诉读者列表比实际短。

### 而构建树的那次遍历，恰恰是唯一算不出这个索引的地方

上游在 **render 层**设它（`RenderIndexedSemantics`、`RenderTable` 把它放到
`SemanticsConfiguration` 上，节点再从 config 读回来），因为等一个节点走到
遍历这一步时，**被丢掉的兄弟已经不在了，它们的位置也一起没了**。
所以这里 `index_in_parent` 保持 `None`，并写明原因——
在这里用 `children.len()` 填一个值，正是那条规则存在所要禁止的重新编号。

9 个变异，**一个活了两轮**：让遍历自己编个 `Some(0)` 全绿。
因为那条测试**是自己手搓了一个节点**去断言的，从没跑过那次遍历——
它检查的是我测试辅助函数的默认值。挪进 `mod tests` 用 `describe_tree`
真跑一次采集之后（还得换成一个真会产出语义节点的 widget，
`SizedBox` 什么也不产），重跑变红。

**「测试没有真的执行被测的东西」——本轮和上一轮的存活变异是同一个物种。**

## `MaterialButton`：一次被 `head -12` 截断的 grep，和一条真正的阶梯（2026-08-23）

### 先更正：`ButtonTheme` 不是为一个布尔值活着的

第 55 轮记过「`ButtonTheme.of` 上游只被读三次——`ButtonBar` 一次、
`DropdownButton` 两次，都只为 `alignedDropdown`」，并由此断言
**一整个主题为一个布尔值留着**。**这句是错的。**

还有第四个：`material_button.dart:394`，而那才是这个主题真正服务的 widget——
`MaterialButton.build` 从它身上读 `getFillColor`、`getTextColor`、
`getFocusColor`、`getHoverColor`、四个 elevation getter、`getPadding`、
`getConstraints`。那才是 `ButtonThemeData` 存在的理由。

当时那条 grep 管道尾巴是 `head -12`，而 `material_button.dart`
排在 `dropdown_menu.dart` 后面，**正好被截掉**。
`alignedDropdown` 的那些发现（16 换手、end 不换手、装饰器只挡一半）
本身独立成立，不受影响。

### `getTextColor`：禁用先查，但它改变的比看上去少

`getTextColor` 开头是 `if (!button.enabled) return getDisabledTextColor(button)`，
读起来像「禁用压过调用者自己的 `textColor`」。**并不是**：
`getDisabledTextColor` 是 `textColor ?? disabledTextColor ?? onSurface@0.38`，
所以 `textColor` 两条路都赢。**禁用那一支唯一决定的，是没给 `textColor` 时怎么办**
——而那正是 `disabledTextColor` 唯一有机会被读到的地方。

（差点又写成「禁用压过一切」。读了 `getDisabledTextColor` 才没写错。）

### primary 的标签是对着自己的填色选的，不是对着页面

`normal` 和 `accent` 问的是环境亮度和配色方案；`primary`
**估的是填充色的明暗**，只有在没有填色时才退回环境亮度。
一个画在深色上的按钮放在浅色页面上，得到白字——问页面会答错。

### 而且那两个黑不是同一个黑

`normal` 返回 `black87`，`primary` 返回 `black`。
页面上的文字是正文，取 Material 2 的正文黑；坐在色块上的标签需要整个黑。
**那百分之八的差别，是「读一个标签」和「读一段正文」的差别。**

### `getFillColor` 里有一个运行时类型判等

`if (button.runtimeType == MaterialButton) return null;`——不是虚调用，是
**精确类型比较**。一个纯粹的 `MaterialButton` 从主题拿不到任何填色，
只有它的子类拿得到。「是一个 `MaterialButton`」和「**恰好是**一个
`MaterialButton`」在这里是两个答案。Rust 没有对应物，
所以它以一个按含义命名的 flag 到达。
而且那道闸门是**第二**条子句：被明确告知颜色的按钮，什么类型都算数。

### 顺带把 `Color::compute_luminance` 和 `estimateBrightnessForColor` 移植了

阈值不是 WCAG 的 0.0525 而是 **0.15**，上游注释写明理由：
「Material Design 似乎比 WCAG20 建议的更偏向使用浅色文字」，
而 0.15「看起来接近 Material Design 规范给它的调色板所展示的效果」——
**一个看着图定出来的数，就这么写下来了。**
阈值抬高的效果是：更多颜色被算作**浅色**，于是更多颜色配深色文字。

14 个变异，**一个活下来**：把 gamma 展开整个删掉（直接线性返回）全绿。
因为我测的颜色**全在曲线的两端**——0 和 255——那里曲线和直线本来就重合。
补了一个中灰：50% 灰只有约 21.6% 的光，不是 50%；
又补了拐点以下（0.03928 以下除以 12.92 的那一段）。重跑变红。

**「两边都设了但相等」的又一件新衣服：只在函数不变的地方取样。**

## `TextInputClient`：平台可以问什么，以及哪些必须有答案（2026-08-23）

`depth.py` 报 2/15。这次是真的——本移植的 trait 只有
`update_editing_value` 和 `perform_action`。

### 哪些必须答、哪些可以不理，不是按重要性分的

上游九个方法没有方法体、六个有 `{}`。**必须答的那些，是平台随时会问、
而且需要一个答案的**：当前的值、autofill 作用域、一次按键、一个动作、
连接关掉了。可以不理的那些，是关于某个客户端可能压根没有的功能的通知：
Android 的内容插入、Scribble 的占位、macOS 的 selector、换掉的输入控件。

忽略一个可选的，客户端照常工作；忽略一个必须的，
**平台就举着一个问题没人接**——所以上游不给它们方法体可继承。

### `currentAutofillScope` 返回 null 是两件不同的事

上游原话：「如果这个 `TextInputClient` 不需要 autofill 支持，它应该返回 null。
对于一个**支持** autofill 的 `TextInputClient`，返回 null 会让它**独自**参与
autofill。」

所以 `None` 既是「我不做 autofill」，又是「我做，但不跟任何人一组」——
**而这个值本身分不出是哪一种，因为区别在客户端身上，不在答案里。**
（这也是本移植给它套了个 `AutofillScopeId` newtype 的原因：
`None` 已经背了两个意思，不能再让「第 0 组」挤进来。）

### 平台送来的值是用户输入，应用设的值不是

`updateEditingValue` 的文档：新值「被当作用户输入，因此可能要过输入格式化」。
**同一段文字走两扇门，是两样东西。**

### `connectionClosed`：是「了结」，不是「忘掉」

上游写明欠什么：客户端「应该清理它的连接并**了结**编辑」。
了结，不是忘掉——一个正在进行中的输入法组合不会再收到后续消息了，
所以它是要被有意地提交或丢弃的。

这和主动 detach 是**相反方向**：detach 的客户端是自己决定停；
被告知连接关闭的客户端是**事后**被通知的，半打完的字成了它的问题。

移植时把 `TextInputClient.onConnectionClosed` 接进了 dispatch，
而且**先清槽、再通知**：一个在 `connection_closed` 里就地重连的客户端
（表单跳到下一个字段就是这样），不能被这一次的清理把新连接掐掉。

### `onFocusReceived` 默认 false，而且理由写在上游注释里

「通知客户端平台把焦点移回了这个输入框。这是为了支持某些浏览器
（例如 iOS Safari）上的 autofill——它们会先让文本框失焦、再重新聚焦，然后填充。」
返回值表示客户端是否拿到了焦点，**默认 false 就是「那不是我」**，
对于任何没听说过 Safari 这套失焦-重聚焦舞步的客户端，这是安全答案。

7 个变异，7 个全红。每条新测试都单独跑过。

## 触控板手势：解码了，却没有人能订阅（2026-08-23）

从 `Listener`（`depth.py` 报 2/13）查起。本移植的 `PointerHandlers`
其实比上游的 `Listener` **更宽**——它把 `MouseRegion` 和 `GestureDetector`
一并合了进来（点击、拖拽、长按、双击、缩放）。所以那个比值又是高报。

**但有一处是真的缺**：`PanZoomStart/Update/End` 三种事件从线上**解码出来了**
（`7 => PanZoomStart` 等），却没有任何字段可以订阅它们——
而且有一条测试正**断言它们哪儿也到不了**。

那条测试的注释把它们和 hover、add 归成一类：「那些是 router 的」。
**两件事被混成了一句话。** hover 和 add 确实是 router 的（它跟踪鼠标在哪些区域里，
上游也一样）；而 pan-zoom 在上游**是 `Listener` 的回调**，在这里却落在地上。

### 触控板的平移是一条流，滚轮不是

上游 `_handlePointerEventImmediately` 对 signal、hover、down **和**
pan-zoom start 都做命中测试，但**只为 down 和 pan-zoom start 存下结果**：

```
if (event is PointerDownEvent || event is PointerPanZoomStartEvent) {
  _hitTests[event.pointer] = hitTestResult;
}
```

**所以滚轮每一个事件都重新命中测试，而触控板手势被开始时它下面的那个东西捕获住。**
两个从外面看很像的手势，一个按「按压」路由，一个按「一次性」路由——而且是对的：
两根手指滑出了它正在驱动的控件，应该继续驱动那个控件，就像拖拽一样；
而滚轮滑到别的控件上，那就是别的控件的滚动。

### update 同时带总量和步长

上游的 `PointerPanZoomUpdateEvent` 既带 `pan`（自手势开始的总量）
又带 `panDelta`（自上一次的步长），**所以监听者永远不必去积分或求导**：
做平移的要步长，显示「已移动 40 点」的要总量，
而从其中一个算另一个，一边丢精度，一边丢历史。

### 而 scale 的静息值是 1，不是 0

三个构造器里 `scale = 1.0`、`rotation = 0.0`。所以 `PanZoomEvent` 的
`Default` 是**手写的**而不是 derive 的——derive 会给出 `scale: 0.0`，
那是一个塌成一点的视图。

（另：三个 pan-zoom 事件的 `kind` 都被上游写死成 `trackpad`，
不是参数——这种手势不可能来自鼠标或手指。）

9 个变异，9 个全红。顺带把那条把 pan-zoom 和 hover 归成一类的注释改了：
现在分开写清楚两个不同的理由。

## 一个段落有两个字符串，不是一个（2026-08-23）

`TextSpan`（`depth.py` 报 4/22）。本移植的 `TextSpan` 只有 `{ text, style }`。

### 画出来的和读出来的是两个串

上游用 `computeToPlainText` 造画出来的那个，用 `computeSemanticsInformation`
造读出来的那个，**从同一棵树**。两者在任何一个 span 带了 `semanticsLabel`
的地方分岔——上游自己的例子是
`TextSpan(text: r'$$', semanticsLabel: 'Double dollars')`：
**`$$` 是画出来的，"Double dollars" 是听到的。**

本移植原来一个 `content` 兼两职，这在没有 span 想被读成别的样子之前都对。
第二个串**只在有人要求时才造**，所以从不要求的段落一点额外开销也没有。

### 「label 让 span 独立成节点」是错的

上游在构造器里算：

```
requiresOwnNode = isPlaceholder || recognizer != null || semanticsIdentifier != null;
```

三条路，**`semanticsLabel` 不在其中**。label 改的是这一段话说什么，
它不把这段话切开。切开它的是**能被单独够到**：
placeholder 是文字里的一个 widget，读者得能落到它上面；
带 recognizer 的 span 能被激活，读者得能激活它；
identifier 是测试或工具要单独找到的东西。

**给一句话里的一截改个名字，它还是一句话；把那一截变成可点的，就不是了。**

### placeholder 恰好一个字符，而且自己什么也不说

```
assert(!isPlaceholder || (text == '￼' && semanticsLabel == null && recognizer == null));
```

它的文本就是那个对象替换符，不多不少；而且既不能带 label 也不能带
recognizer——因为占着那个位置的 widget **自带语义**，
再盖一层 label 就成了文字层在谈论它看不见的东西。

### 又是同一个物种，第四次

12 个变异，**一个活下来**：把 `describe_semantics` 改回读画出来的那个串，全绿。
因为那两条关于「两个串」的测试**直接查 `spoken()`，从没跑过语义遍历**。

这已经是连续第四轮同一类存活变异了——**测试没有真的执行被测的那条路径**。
补了一条走 `describe_tree` 的测试（断言读者听见 "Costs Double dollars"，
且**永远听不到 `$$`**），重跑变红。

四次都是变异测试抓到的，每次代价是多跑一轮。

## 文本选择手势：一台 iPad 上的鼠标像桌面（2026-08-23）

`TextSelectionGestureDetector`（`depth.py` 报 4/24）。真正的内容在
`TextSelectionGestureDetectorBuilder` 里那几个 `switch (defaultTargetPlatform)`。

### 工具栏是给手指和触控笔的，不是给鼠标的

`_shouldShowSelectionToolbar = kind == null || kind == touch || kind == stylus`。
鼠标有右键菜单和键盘；指尖两样都没有，所以控件必须画在屏幕上。
**unknown 算作手指**——这是安全的那一边：多给一个没人要的工具栏是麻烦，
不给一个没有别的办法复制的人是死路。

（顺带钉住：**反向触控笔不在上游那份名单里**。这条看着像疏漏，也可能真是——
上游自己在这条规则上方留了个未决问题：一台带触摸屏的 Windows 设备怎么办
（flutter/flutter#106586）。这里钉的是它今天的行为。）

### 桌面在按下时移光标，手机等到抬手

上游两处注释互相指着对方：`onTapDown` 里写「on mobile platforms the
selection is set on tap up」，`onSingleTapUp` 里写「on desktop platforms the
selection is set on tap down」。

桌面上一次按下是一次可能的拖选的开始，**光标必须已经在它的一端**；
而一根手指落下可能只是滚动的开头，每次滚动都移一次光标会让一列文字没法读。

### shift-tap 一个未聚焦的字段，从 0 开始展开——两个苹果平台都是

上游写了两遍：一遍在 macOS 分支，一遍在 iOS 分支，别处没有。
一个从没被聚焦过的字段，它的「上一次选区」不属于任何人，
从那里展开等于延伸一个读者从没做过的范围；从头展开至少是一个他们能看全的范围。

### 而光标落在哪，iOS 要问**你是用什么碰的**

上游 macOS 分支的注释自己把对比画了出来：「在 macOS 上，点击把选区放在一个
精确位置。这和 iOS/iPadOS 不同——在那里如果手势是触摸做出的，
选区会移到最近的词边界，而不是精确位置。」

而 iOS 自己那条分支**按设备种类再分一次**：鼠标、触控板、触控笔拿精确位置，
触摸和 unknown 拿词边界。**所以一台 iPad 上的鼠标表现得像桌面**——
要词边界的理由是那根指尖，不是那个操作系统。

11 个变异，11 个全红。

### 顺带：把第六条验收门写进了 PORTING_PLAN

连着四轮（64、65、66、69）的存活变异是同一个物种——**测试没有真的执行
被测的那条路径**：一个读常量的谓词、一条手搓节点不跑遍历的测试、
一条只在曲线两端取样的测试、一条直接查 `spoken()` 不跑语义遍历的测试。

四次都是变异测试抓到的，每次多花一轮。规矩因此写下来：
**这一轮如果把什么接进了某条既有流水线，就必须有一条测试从那条流水线的
公开入口走一遍**，否则「接线」那一半根本没被测过。

## 九宫格：唯一让中心切片起作用的 fit，正是你不指定时得到的那个（2026-08-23）

这一轮先查后做，省了两次重复劳动：

* **`FocusTraversalGroup`（2/11）是高报**——方向遍历早已整个移植在
  `src/directional_traversal.rs` 里（924 行、25 条测试、自己的提交
  「"To the right" is not a question with one right answer」）。
  差点又写一遍。
* **`Listener`（上一轮）同理**。

`RawImage`/`RenderImage`（2/20）是真的缺，而且**本移植的文档里已经写着
一条关于它没有的字段的规则**：`RenderImage::new` 的注释解释默认 fit 时引用了
`fit ??= centerSlice == null ? BoxFit.scaleDown : BoxFit.fill`——
而 `RenderImage` 根本没有 `center_slice`。
（和 `CupertinoUserInterfaceLevel` 那次同一个物种：文档描述了一个不存在的东西。）

### fit 只作用在会拉伸的那部分上

上游三行就是九宫格的全部想法：

```
sliceBorder = inputSize / scale - centerSlice.size;
outputSize  = outputSize - sliceBorder;
inputSize   = inputSize - sliceBorder * scale;
```

切片盖不到的部分——四角和四边——在 fit 跑之前从**两边**都扣掉，跑完再加回来。
**所以 fit 从来看不见四角，四角也从来看不见 fit。**

而且 border 是**两个数不是四个**：左右加在一起、上下加在一起，
因为相对的两条边从不各自缩放。

### 两个 fit 被禁，其余的 fit 全都白搭

`assert(centerSlice == null || (fit != BoxFit.none && fit != BoxFit.cover))`
挡掉两个。挡掉其余的那条更安静，上游写在散文里而不是断言里：
「不会让目标尺寸变形的那些 `BoxFit` 值会导致 `centerSlice` 毫无效果
（因为九个区域会用同一个缩放渲染，就像没指定过一样）。」

保比例的 fit 让九个区域同一个缩放，而**九个同缩放的区域就是一个区域**。
所以唯一让中心切片起作用的 fit 是 `Fill`——
**而那正是你不指定 fit 时得到的那个**（`fit ??= ... : BoxFit.fill`）。
**这个特性的设计用法就是别去动它。**

### 「被禁」和「白搭」不是同一个答案

裁剪型的 fit 被拒，理由不是审美：
「我们没有能力在做九宫格拉伸的同时只画图片的一个子集。」
——**是画布没有这个能力，直说了**。而保比例的 fit 是允许的，只是什么也没做成。

10 个变异，10 个全红（其中一个第一次写成了不编译的形状，补齐遗漏臂后重跑也是红的）。

## 一句差点写出去的错话，钓出了一个真 bug（2026-08-23）

### 先说没做的事：没有去改 `depth.py`

连着三次高报（三个列表磁贴、`Listener`、`FocusTraversalGroup`），
原因都一样：**本移植把成员分解进了名字不同的类型，而 `depth.py` 按上游的名字数。**
它的 `companion_body` 找的是 `X`/`XData`/`XState`/`ResolvedX`/`RenderX`——
**又一个名字形状的启发式**，正是把 `unwired.py` 坑了四次的那一类。

想过改成按行为判定：数那些文档里写着 `Upstream \`X\`` 的 Rust 类型。
**先去查了一下这个约定靠不靠谱——不靠谱**：`PointerHandlers` 的文档里
一次都没提 `Listener`，`ControlListTile` 的文档也没点三个磁贴的名字。
一把三次里对两次的尺子比一把老实的高报尺子更糟，**因为我会开始信它**。
所以没改。真正管用的是这三轮做的事：**动手前先 grep 一下**——一条命令，可靠。

### `CupertinoMenuItem`：禁用压过危险

`_resolveDefaultTextStyle` 的问法是
`onPressed == null → systemGrey`，`else isDestructiveAction → systemRed`，
`else 默认色`。所以**一个被禁用的危险项是灰的，不是红的**。
**能力没了，警告也跟着撤掉**：一个按不下去的按钮没什么可警告的，
而一个什么也不做的红按钮只会无缘无故地吓人。

### 按下先关菜单，再调回调

`_handleSelect` 先关后调，所以回调运行时菜单已经在关了——
一个 push 路由的回调不会和菜单自己的消失抢同一帧。
而 `requestCloseOnActivate: false` 只停掉关闭那一步，**不停掉回调**：
两件事是分开的，只有第一件可选。

### 副标题的颜色是个混合模式，而且是个近似

上游 `blendMode = isDark ? BlendMode.plus : BlendMode.hardLight`，
自己注明 iOS 用的是 `linearDodge` 和 `plusDarker`，这两个是它们的近似；
整套样式还标着「approximated from the iOS and iPadOS 18.5 simulators」。
**记下来，是因为照抄这两个模式等于在复刻一个近似而不是平台本身**——
在有人对着截图去调它们之前，值得先知道这件事。

### 而这一轮真正的收获，是一句差点写出去的错话

我在文档里写了「`requestFocusOnHover` 默认 true——这和 Material 不一样，
`MenuItemButton.requestFocusOnHover` 是 false」。**去核了一下上游：错的。**
Material 那边默认也是 true。

但核这一下**钓出了一个真 bug**：本移植的 `MenuItemButton::new()`
把它设成了 `false`。后果是：**鼠标划过菜单、再按回车，动作会落在键盘
之前留下的那一项上**，而不是指针底下这一项。已改成 true。

**我以为的平台差异，其实是我自己的移植错误。**

10 个变异，10 个全红。

## 千分位、十二点，和一个没加的枚举（2026-08-23）

### 先说没加的：`TextWidthBasis`

`RichText`（2/16）缺 `TextWidthBasis`。上游的规则很利落——两个分支
**夹的是同一个范围，只差夹的是哪个测量值**：

```
longestLine => clamp(longestLine, minWidth, maxWidth)
parent      => clamp(maxIntrinsicLineExtent, minWidth, maxWidth)
```

`longestLine` 是排完版之后最长那一行的**墨迹宽度**（最左字形左边到最右字形右边）；
`maxIntrinsicLineExtent` 是「再加宽也不会更矮」的那个宽度，而且**算上行尾空格**。

**但本移植加不了这个枚举**：`TextPainter::max_intrinsic_width` 现在返回的就是
`longest_line()`——引擎桥只报一个宽度。两个分支会算出同一个数，
**那就是一个不切换任何东西的开关**，正是这一整轮我一直在挑的那种东西
（tick 51 的 `use_material3`、底部动作栏的死默认、没有东西可挑的界面层级）。

所以没加，把理由和解锁条件记在这里：等引擎的 intrinsic-width ABI 落地，
`max_intrinsic_width` 能独立回答之后，这个枚举才有意义。
（另：`min_intrinsic_width` 现在也返回 `longest_line()`——即**最大值**，
那条已有文档记着；后果是一个段落声称自己不能比最长的一行更窄，
于是在按 intrinsic 宽度布局时永远不会换行。）

### `formatDecimal`：一千以下的都从前门走

`if (number > -1000 && number < 1000) return number.toString();`
这条早退不只是快——**一千以下根本没有组可分**，走通用路径只是绕远路写同一个串。

分组**锚在个位**：上游从左往右走，但判据是 `(maxDigitIndex - i) % 3 == 0`，
量的是**从右边数**。所以 1234 是 `1,234` 不是 `123,4`——
一共几位数，永远不影响逗号落在哪。

负号在看任何数字之前就写好了，之后全部来自 `abs()`，
所以**减号永远不参与分组**，`-1000` 不会变成 `-,1000`。
而那个 `abs()` 正是 Dart 和 Rust 分手的地方：Dart 的是不陷阱的 64 位整数，
Rust 的在 `i64::MIN` 上 debug 会 panic。这里用 `unsigned_abs` 显式处理了——
**一个永远不会有人去格式化的数，仍然不构成中止的理由。**

### `formatHour`：十二小时制没有零

`hourOfPeriod == 0 ? 12 : hourOfPeriod`。**午夜和正午都写作 12**——
那个本该是零的钟点，正是表盘顶上的那一个。

而 `DefaultMaterialLocalizations` **只支持六种 `TimeOfDayFormat` 里的两种**，
其余四种直接抛 `AssertionError('$runtimeType does not support $format')`。
它是英语那一份，不是通用的那一份：`frenchCanadian`、`HH_dot_mm` 之类
属于说那些语言的 localization。这里返回 `Err` 而不是抛，好让这个拒绝能被测。

### 而这些为什么不走日期格式化器

上游在本该调用 `DateFormat` 的地方写明了两条理由，第二条有意思：
`DateFormat`「操作的是 DateTime，而它对纪元和时区敏感；
可这里我们要的是一天之内的时和分，**不管这一天落在哪个日期上**」。

**一个 time of day 不是一个时刻。** 把它塞进日期类型，
会拖进一个它没有的日期，和一个它从来不在的时区。

11 个变异，11 个全红。

## `editing` 问的是字段里有什么，不是字段正在发生什么（2026-08-23）

`CupertinoTextField`（`depth.py` 报 15/77）。

### 一个名字读起来像状态、其实是内容的枚举

`OverlayVisibilityMode.editing` 读起来像「读者正在打字的时候」，
而上游自己的文档说的是另一回事：它出现在「当前文本条目不为空」时，
并且「**包括用户没有手动输入的预填文本**」。

所以一个打开时就带着值的字段，在任何人碰它之前就已经是 *editing*；
而一个读者聚焦着、但一个字都没打的空字段不是。
**这个模式问的是字段里有什么，不是字段正在发生什么。**

而且**占位符不算文本**——两个内容模式都明说了。这是自洽的：
占位符是字段在告诉你它是空的，把它算成内容，「空」就永远到不了了。

### 三个附件不共享默认值

前缀和后缀是 `always`，清除按钮是 `never`。
**唯一能删掉读者文字的那个，是默认关着的那个。**

### 清除按钮在出现之前就算装饰

`_hasDecoration` 判的是 `clearButtonMode != never`，不是「按钮正在显示」。
一个只在有文字时才出现按钮的字段，**在还空着的时候就算被装饰了**——
否则读者敲下第一个字符的那一刻字段就会改变对齐，他正在看的文字会跳一下。

### 而对齐正是这条规则的用处

上游注释：「CupertinoTextField 默认顶对齐，除非它有前缀后缀之类的装饰，
那时改为居中。」光秃秃的字段从顶上开始排字，
于是一个会长高的多行字段是从第一行已经在的地方往下长；
有装饰的字段居中，于是文字和旁边的图标在同一条线上。

### 清除按钮那一下算「用户发起的」

`_onClearButtonTapped` 只在**原本有文字**时才调 `onChanged`——
清空一个空字段什么也没变，上报一个没发生的变化，
是对任何在数击键次数的东西撒谎。

而上游写明了这个上报的含义：「点清除按钮也被视为一次**用户发起的**变化
（而不是程序性的）」。这正是 tick 67 在 `updateEditingValue` 上记的那条线的另一端
——**读者造成的值要过格式化器和回调，应用设进去的不用**——
而清除按钮虽然是字段自己的 widget，站在读者那一边。

10 个变异，10 个全红。

## 三种字形几何，如今是一种（2026-08-23）

`Typography`（6/28）。本移植有 `english_like`/`dense`/`tall` 三个函数，
但没有挑它们的那个开关，也没有颜色那一半。

### 先量了再说：三者的字号完全一样

写规则之前先对了一遍数——**本移植的三个几何，十五个字号一模一样**。
上游 `ScriptCategory` 的文档却说「tall 和 dense 的字号，
对比 title 更小的那些文本样式，要**大一个单位**」。

去核上游：**2021 的三个（`_M3Typography` 的 englishLike/dense/tall）
字号和行高完全相同**。所以本移植是对的，那条文档说的是 2014。

再核 2014：确实不同，而且**比文档说的更宽**——

* **title 三个自己也在动**（`titleLarge` 20→21），
  所以「比 title 更小的」这个说法是少说了。
* **`labelMedium` 一个不动**，而它两边的 `labelLarge` 和 `labelSmall` 都动。
  它读起来像 2014 那张表里的一处疏漏，而不是一个决定——
  这里钉的是上游写了什么，不是规则会推出什么。

**Material 3 把三个书写体类别并成了一个。** 于是
`geometryThemeFor` 是一个三条臂返回同一个东西的开关。

### 而这个开关值得建，`TextWidthBasis` 那个不值得

上一轮拒绝了 `TextWidthBasis`，因为本移植的两个测量值会算出同一个数。
这一个今天也分不出来，**差别在于「一样」是从哪来的**：
那边是**引擎桥的限制**，这边是**上游自己的设计**——
而 `material2014` 证明它是可以被改回去的。

**因为取值恰好相等而暂时平凡的开关，仍然是一个开关；
因为我们看不见取值而平凡的开关，是一种假装。**

### 一个文本主题是两个主题，由两件不相干的事挑

上游：主题「由一个**颜色**文本主题（浅色用 black、深色用 white）
和一个**几何**文本主题（englishLike/dense/tall 之一，取决于 locale）
合并而成」。**明暗挑墨色，语言挑度量**，两者互不知情。

### 越南语归 tall，不归 englishLike

上游特意点出来：越南语「用的是拉丁书写系统的一种本地化形式」——
按字母表分类会把它和英语放一起——「但它的带声调字形可能比西欧语言的高得多」。
**类别跟的是墨迹，不是字母表。**

7 个变异，**两个第一次活了下来**：把 `geometry_for` 三条臂全指向 english、
把差异判定只查一半，都全绿。因为三个常量当下相同，**这两个函数对着常量
根本无法被证明能分辨它们**。

用的是 tick 64 的同一个补救：把三个主题**作为参数传进去**
（`select` 和 `any_geometry_differs`），测试就能喂它三个真的不同的主题。
重跑，两个都红了。

**对着一组恰好相等的常量写的函数，证明不了自己会把它们分开。**

## 开关旁边那两个记号不是字母，是电源符号（2026-08-23）

`CupertinoSwitch`（6/26）。拖拽阈值、按压、禁用透明度早就在了；
缺的是 iOS 的**开关标签**那套无障碍功能。

### 是 I 和 O，画出来的

上游把「开」那个记号画成一个 `_kOnLabelWidth = 1` × `_kOnLabelHeight = 10`
的矩形，把「关」那个用 `drawCircle` 画成半径 `_kOffLabelRadius = 5` 的圆。
**一根一乘十的竖条和一个圆环——国际电源符号的 I 和 O**，
用图元画出来，不是排版出来的文字。

这也正是它们**不需要字体、不需要本地化**的原因：
一根竖条和一个圈在任何文字体系里都是同一个意思，
而一个写着 "on" 的开关得翻译，还会放不下。

顺带钉住：**两个内边距不一样**（11 和 12）。
一个半径 5 的圆和一根宽 1 的竖条，用同一个内边距看起来不会一样深。
而圆的直径正好等于竖条的高度，所以这一对看起来是一个尺寸。

### 而且只在读者要了的时候才出现

`MediaQuery.onOffSwitchLabelsOf(context)` 把整对都关掉：
上游构造的是 `(onColor, offColor)` **或者 null**——
设置关着时是**没有东西可画**，不是画一个透明的东西。

### 一个平台报不上来、但应用可以设的设置

`MediaQueryData` 上加了 `on_off_switch_labels`（默认 false）。
桥接**报不上来**——`Binding::did_change_accessibility_features` 这个钩子在，
后面什么都没有。仍然带上它，是因为 **`MediaQuery` 是一个 widget**：
知道这个设置的应用、或者想要这两个记号的测试，
往树里放一个，下面全都看得见。

**这和「一个没人能拨动的开关」是两回事**——上一轮拒绝 `TextWidthBasis`
是因为那两条臂算出同一个数，这里两条路真的会走向不同的结果。

### 而 off 的默认色，本移植表达不出来

上游的 `_kOffLabelColor` 是 `withBrightnessAndContrast`：
普通对比度下是 `(179,179,179)`，**高对比度下是白色**。
tick 64 记过：`CupertinoDynamicColor` 的高对比度两列是唯一还缺的一对，
理由是平台桥不带对比度设置。**于是那个缺口在这里长出了一个具体后果**——
一个要了高对比度的读者，看到的仍是那个普通的灰。

9 个变异，**一个活下来**：让 `from_view` 把这个设置报成 true，全绿。
因为那条测试查的是 `MediaQueryData::default()`，**不是 `from_view`**。

这正是**上一轮刚写进 PORTING_PLAN 的第六条验收门**
（「接进流水线的东西，至少有一条测试从流水线入口进去」），
而我这一轮自己没照做。改成走 `from_view` 之后重跑，红了。


---

## 标签栏报的高度，和它画的盒子，本来就不是一个数（2026-08-24）

`CupertinoTabBar` 上游 `preferredSize` 是 `Size.fromHeight(height)`——
**只有 50，里面没有安全区**。而画出来的盒子是 `height + bottomPadding`。

这不是不一致。那个 inset 是**加进盒子、又原样还给内容**的：

```dart
SizedBox(
  height: height + bottomPadding,
  child: ... Padding(padding: EdgeInsets.only(bottom: bottomPadding)),
)
```

于是标签仍然待在它们的 50 里，多出来的是 home indicator 上方的**空地**。
读 `preferredSize` 的人问的是「标签需要多少地方」，
排版的人则要连 inset 一起给——**报成两者之和，脚手架就会把 inset 留两遍**。

移植早就把盒子算对了（`TAB_BAR_HEIGHT + bottom`），
这一轮补的是**把这两个数分别钉住**：`preferred_height()` 和 `box_height(inset)`。

### `opaque` 是对颜色的提问，不是一个标志位

```dart
return CupertinoDynamicColor.resolve(backgroundColor, context).alpha == 0xFF;
```

没人拨这个开关：一条栏不透明，是**因为它被画成了什么样**。
而它决定的是那层模糊——上游只在不透明的栏后面加模糊，
因为**在看不透的东西底下做模糊，是没人会看的活**。

先 resolve 再问，是这句话有意义的前提：
一个 `CupertinoDynamicColor` 可以在一种外观下不透明、另一种下透明。
顺带确认了默认的 `bar_background_color` 两种外观都是 `0xF0`——
**模糊是常态，不是例外**。

### 居中替得了底对齐，只因为图标槽是定死的

上游那一行是 `CrossAxisAlignment.end`，注释写「Align bottom since we want
the labels to be aligned」——图标矮一点的那格，标签本会比邻居高。

本移植是居中，画面一样，**因为每个图标槽都是 30×30 的定死方块**：
每列一样高时，居中和底对齐是同一个位置。
但这个等价**靠的是那个定死的槽，不是「对齐不重要」**——
所以 `alignment_is_equivalent(icon_heights)` 把高度当参数收进来，
让「谁能自己定图标大小」这件事一发生，测试就红。

### 七个变异，第六个活了下来

把 build 里那句 `EdgeInsets::only(0,0,0,bottom)` 换成 `ZERO`——**全绿**。

因为那条测试**名字叫 padding，查的却只是高度**。
去掉 padding 的栏一样高，
只是会**把一个标签放到 home indicator 底下**——
从屏幕底边上滑的手势会正好按到它。

这是**连着第五轮**同一个物种：*一条不走它所说的那条路的测试*。
改成**用点的**：inset 是 34 时，25 那里点得到标签，
76 那里（indicator 上方那条）点不到任何标签；
而 inset 是 0 时，42 那里又点得到——
**那片空地是 inset 带来的，不是栏自己一直留的边**。

改完之后 M6 红，再补一个「把 padding 加到上边而不是下边」的 M7，也红。

---

## 第六把尺子：注释说这个数出自上游，那上游还这么说吗（2026-08-24）

移植的注释里到处是这种断言：

```rust
/// Upstream's `_kTabBarHeight`.
pub const TAB_BAR_HEIGHT: f32 = 50.0;
```

**从来没有任何东西核对过这两半。**一个 2026 抄对、2027 上游改掉的数，
留下的注释指着一个已经不那么说的常量——而这份对应关系
**只写在注释里**，所以别的东西也不会发现。

`tools/constants.py` 把这句话读出来，两边比。

### 头一次跑，它自己错了两次

七条报告，审下来**两条是尺子的错**：

- `THUMB_RADIUS = 7.0` 报和 `_kThumbRadius = 14.0` 不符。
  上游有**两个**同名常量：`switch.dart` 的 `const double = 14.0`，
  和 `sliding_segmented_control.dart` 的 `const Radius = Radius.circular(7)`。
  移植的 7 是后者，是对的。尺子只看得见标量那个。
  **改法不是「挑看起来更近的那个文件」**——那又是拿名字当行为的判断，
  `unwired.py` 已经在这上面栽过四次。诚实的答案是：
  一个名字有多种声明形式时，这把尺子判不了，记作 unchecked。

- `_kDefaultBorderRadius` 报「上游已不再定义」。它在
  `search_field.dart` 里，写作 `final BorderRadius _kDefaultBorderRadius = ...`
  ——是**字段**，不是 `const`。加上 `final` 就认得了。

而修第一条时又写坏一次：把「结构化」按名字集合算，
于是每个标量也都进了结构化集合，**158 条全判成 unchecked、一条没查**,
却照样打印出让人放心的 `0 disagreeing`。
按声明起始位置分辨才对——标量声明在同一个位置被两个模式同时匹配。

### 剩下五条是真的：数对，出处是编的

| 处 | 引的名字 | 上游实际 |
|---|---|---|
| `ResolvedListTile::MIN_TILE_HEIGHT` | `_kMinTileHeight`-ish | 没有常量；`ListTile.minTileHeight` 的**文档注释**里写着 56/72/88，dense 48/64/76 |
| `ResolvedDrawer::SCRIM` | `_kScrimColor` | 没有；`Drawer.scrimColor` 文档说 fallback 是 `Colors.black54` |
| `ResolvedFloatingActionButton::SIZE` | `_kSizeConstraints` | 没有；是个局部变量 `BoxConstraints sizeConstraints`，56 出自类文档 |
| `GridTileBar::ONE_LINE_HEIGHT` | `_kOneLineHeight` | 没有；上游把 68 和 48 就地写着，两个都没命名 |
| `VisualDensity::PIXELS_PER_UNIT` | `_kDensityAmountPerUnit` | 没有；是 `baseSizeAdjustment` 里的**局部** `const interval = 4.0` |

五条是同一个形状：**上游用散文、文档注释或局部变量说出来的数，
被安上了一个像模像样的 `_kFoo` 私有常量名。**数全是对的，出处全是不存在的。

这比数抄错更难发现：将来有人问「56 是哪来的」，
去上游搜 `_kSizeConstraints`，搜不到，
**而他分不清是数过时了还是名字过时了**。

五条都改成上游实际的说法。尺子归零：153 条引用，0 不符，0 失效
（另 54 条是结构化常量，声明为不比）。

反向验过两头：把 `TAB_BAR_HEIGHT` 改成 51 → 报 MISMATCH；
把一条引用改成 `_kInventedName` → 报 MISSING。

---

## 「没有 leading」不等于「不加那一份」（2026-08-24）

`CupertinoListSection` 的分隔线起点是两个数相加：
`dividerMargin + additionalDividerMargin`。后者由 `hasLeading` 决定，
而**诱人的模型是错的**：

| | dividerMargin | 有 leading | **没有 leading** |
|---|---|---|---|
| base | 20.0 | 44.0 | **0.0** |
| inset grouped | 14.0 | 42.0 | **14.0** |

「有图标就加、没图标就不加」在 base 上对，**在 inset 上错**——
inset 没有 leading 时仍然加 14。
按布尔开关来实现的移植，会把每一条 inset 分隔线画偏 14。

而且**这四个数之间推不出彼此**。上游的注释各自写着是从
**不同的已发布 app 量出来的**：base 那一对来自 Settings，
inset 有 leading 的来自 Reminders，inset 没有的来自 Notes。
是对 iOS 行为的测量，不是一个体系。所以四个都得带着。

### `hasLeading` 的默认是 true，`derive` 会写成 false

上游两个构造函数都写 `bool hasLeading = true`：带图标的行是寻常的 iOS 列表，
不带的那种才是要声明的例外。于是 `Default` 得手写——
和 `PanZoomEvent` 的 `scale` 是同一个坑。

### 而这一轮真正的收获，是改掉了一句自己写错的注释

移植原来这么写 `CupertinoFormSection::INSET_GROUPED_ROWS_MARGIN`：

> 一个**零上边距**，不像 list section 的 20，因为表单的行跟在一个
> 已经给它们让开了距离的头部下面——**表单永远是 inset-grouped 那个形状，
> 也永远有东西在上面。**

**两半都是错的**，而且当初就能查：

1. `CupertinoFormSection` **有 base 构造函数**，`margin = EdgeInsets.zero`。
   表单不是永远 inset-grouped。
2. 零上边距的原因不是「头部已经让开了」——**表单可以没有头部**。
   真正的原因平淡得多：`CupertinoListSection.insetGrouped` 按有没有头部
   挑边距（没有时上边 20，有时 0），而 `CupertinoFormSection` **总是把
   自己的边距传下去**，那个选择于是从不发生。
   一个**没有头部的表单拿到零上边**，而同样没有头部的 list section 拿到 20
   ——恰好和那句注释预测的相反。

上游也根本没推导这个数，注释写的是「determined from SwiftUI's Forms in
iOS 14.2 SDK」。

**这条是靠着「去查一句听起来很有把握的断言」找出来的**，
和 tick 55 那次 `MenuItemButton` 是同一个办法。
上一轮刚加的 `constants.py` 查的是数，这条查的是理由——
**编出来的因果和编出来的常量名是同一个物种**。

### 另外两处真实差异

- `clipBehavior`：`CupertinoListSection.insetGrouped` 默认 `Clip.hardEdge`，
  而 `CupertinoFormSection` 两个构造函数都默认 `Clip.none` 并往下传。
  **inset 的表单不裁到圆角卡片，inset 的 list section 裁。**
- assert 不一样：list section 收 `children.length > 0 || header != null`
  ——**光有一个头部、底下什么都没有，是合法的 list section**；
  表单要求有行。分组可以最后一行都被筛掉；表单是拿来填的，没得填是错误。

九个变异全红。

### 一句该说清楚的话

移植里的 `CupertinoListSection` / `CupertinoFormSection` **是台账类型，
不是搭出来的 widget**——没有哪个 widget 读 `divider_start()` 去画线。
这一轮对齐的是**记下来的规则**，不是画面。真正把它搭出来是后面的事。

---

## 错误标签的红色，两种外观下是同一个红（2026-08-24）

`CupertinoFormRow` 的 `build` 里，相隔三行：

```dart
final TextStyle textStyle = theme.textTheme.textStyle.copyWith(
  color: CupertinoDynamicColor.maybeResolve(theme.textTheme.textStyle.color, context),
);
...
if (helper != null) ... DefaultTextStyle(style: textStyle, child: helper!),
if (error  != null) ... DefaultTextStyle(
      style: const TextStyle(
        color: CupertinoColors.destructiveRed,
        fontWeight: FontWeight.w500,
      ),
```

`helper` 拿到的是**解析过颜色**的主题样式。
`error` 拿到的是一个 `const` 样式——而 `const` 就是全部原因：
解析一个 `CupertinoDynamicColor` 需要运行时的 `BuildContext`，
**编译期的样式只能把它原样带着**。

没解析的 dynamic color 画出来就是它的基准值，
而且下游**修不了**：widgets 层对 cupertino **没有代码依赖**
（`basic.dart` 里只有一行 `@docImport`），
`Text` 和 `DefaultTextStyle` 看见的就是一个普通 `Color`，照画。

`destructiveRed` 就是 `systemRed`：亮 (255,59,48)，暗 (255,69,58)。
所以**暗色页面上的错误标签，画的是亮色的红**，
就在一个刚被仔细解析过的 helper 标签下面。

### 照上游的样子移，不悄悄修

一个**故意照抄的不一致，是看得见、也对得上的**；
本地修一下，移植就和上游不一样了，
而这个不一样的理由，读哪一边的人都找不到。

于是 `error_color()` 返回基准值，`error_color_if_resolved(brightness)`
返回「要是像 helper 那样解析会是什么」，
`error_color_agrees(brightness)` 是两者是否相同——
**亮色下相同（基准值恰好就是亮色值，是巧合不是解析），暗色下不同。**

### 一条防「两条臂算出同一个数」的测试

单有上面那条还不够：如果两个红本来就一样，
「不一致」就是没人看得见的不一致——就是上一次拒绝 `TextWidthBasis` 的理由。
所以专门有一条测试断言两个红**确实是两个红**，并把两个值都钉住。

五个变异全红。

### 这一轮没做的

`CupertinoFormRow` 的排布也读了：prefix 在起点，child 是
`Flexible` + `centerEnd`（`spaceBetween`），helper 和 error 在下面
`centerStart` 并把行撑高。**没有为它们编谓词**——
这一轮能验的是颜色，排布只记在注释里。

---

## 一个上游的数，在移植里写了两遍，而且不一样（2026-08-24）

`CupertinoTextFormFieldRow` 大半早就移好了：
`padding: Option<f32>` 带着「null 不是零」的规矩，
`validate()` 把上游八条 assert 全覆盖了，**包括最刁的那条**——
`maxLines` 默认是 `1` 而不是 null，所以 `expands: true` 单独写就已经违规。
移植的默认也是 `Some(1)`，这条本来就对。

真正的缺口只有三个，而**头一个是被上一轮照出来的**。

### `DEFAULT_PADDING = 6.0`，而 `CupertinoFormRow::PADDING = (20, 6, 6, 6)`

这两个是**同一个上游的数**——表单字段行没给 padding 时，
兜底用的就是表单行自己的那个。移植把它写了两遍，
而且在**起始内边距**上不一致：20 对 6。

偏偏起始那一侧是有用的那一侧：**20 对 6 才让一列标签对齐**，
也是**标量唯一带不动的那一半**。

上一轮之前，两处各自看都说得通，没有任何东西能把它们放到一起比。
上一轮把表单行的 padding 移成了 `EdgeInsets`，两者才第一次碰面。
现在 `DEFAULT_PADDING` **就是** `CupertinoFormRow::PADDING` 本身，
测试断言的是「同一个」，不是「两个恰好相等」。

### 表单行里的字段是 borderless，而且不是可选项

上游的 builder 不把这个当参数——直接写死
`CupertinoTextField.borderless`。**行本身就是那个框**：
它画分隔线、坐在 section 的卡片里，
字段再带一个边框，就是框里套框。移植的 `new()` 建的是带边框的。

### 一个不可能为假的谓词，写在 API 里

```rust
pub fn labels_beside_rather_than_inside() -> bool { true }
```

不收参数，没有任何输入能让它答别的。**它在陈述一件事，却长得像在检查。**
配套那条测试更空：

```rust
assert!(CupertinoTextFormFieldRow::labels_beside_rather_than_inside());
let mut row = ...; row.has_prefix = true; assert!(row.has_prefix);
```

前半断言一个常量真，后半赋一个值再断言它等于自己。

**删掉，没有换一个新的**。这件事是事实，事实该待在注释里。
再编一条测试去替一条假测试，只是把同一件事又做一遍。
这个物种连着五轮都是在测试里抓到的，这一次它长在 API 上。

四个变异全红。

### 顺带修掉三处「张冠李戴」的引用

上一轮那把 `constants.py` 只管 `_kFoo`。这次手工把
**2292 个自称出自上游的反引号符号**扫了一遍，18 个在上游找不到——
绝大多数是移植自己的 Rust 类型名出现在带 "upstream" 的句子里
（`RefCell`、`BTreeMap`、`RenderRef`）。**这把尺子不值得自动化**：
0.8% 的命中率，且要逐条判断，正是尺子给不了的东西。
和当初拒绝修 `depth.py` 是同一个判断。

但手工审那 18 条是值的，捞出三条真的：

| 移植写的 | 上游实际 |
|---|---|
| `InheritedModelElementMixin.updateDependencies` | 类叫 `InheritedModelElement`，`Mixin` 是编的 |
| `PlatformDispatcher.sendChannelUpdate` | 声明在 **`ChannelBuffers`** 上，不在 dispatcher 上 |
| `ChannelBuffers._defaultBufferSize` | 是 `kDefaultBufferSize`，公开的，不是私有 |

还有一条教了我这个探针自己的毛病：`Shadow.convertRadiusToSigma`
被报成不存在，只因为我扫的是 `packages/`，而它在
`bin/cache/pkg/sky_engine/lib/ui/painting.dart` 里——**引用是对的，我的扫描范围是错的**。
和 `constants.py` 头两次栽的是同一个跟头：**工具错了，报成移植错了。**

---

## 「0 MISSING」是尺子看不见 enum（2026-08-24）

这一轮本来是去看 `CupertinoLocalizations`（depth 2/46）够不够格移。
翻的时候发现上游 `localizations.dart` 里有两个公共枚举
——`DatePickerDateOrder`、`DatePickerDateTimeOrder`——移植里一个都没有。
而 `coverage.py` 对这个文件的报告是 **2 个类，0 MISSING**。

因为 `CLASS_RE` 只认 `class` 和 `mixin`。**`enum` 从来没被数过。**

```
(?:class|mixin)\s+([A-Za-z0-9_]+)
```

这个正则上面本来就有一段注释，记着**同一个毛病的上一次**：
`interface` 曾经漏在修饰符里，害尺子看不见每一个
`abstract interface class`，注释里写着「**尺子看不见的类，永远不可能被报成
MISSING**」。写下那句话的时候，`enum` 就在同一行的旁边漏着。

### 真实的数字

| | 原报 | 实际 |
|---|---|---|
| 公共类型 | 1930 | **2102** |
| MISSING | **0** | **49**（三轮台账分类后 37） |

漏掉的 172 个里，49 个移植里根本没有：`ThemeMode`、`ImageRepeat`、
`DeviceOrientation`、`SchedulerPhase`、`TimeOfDayFormat`、`WrapCrossAlignment`、
`SmartDashesType`……**全是 API 用来分支的那种类型**。
枚举在这里不是「次一等的类」，恰恰是这本台账最该数的东西。

反向验过：把 `enum` 从正则里拿掉，输出**一字不差地回到 `1930 / 0 MISSING`**。

### 头一次跑，尺子又错了一次

新正则把 `extension type const BaselineOffset(double? offset)` 的名字
读成了 `const`——`const` 在 `extension type` 和名字**之间**。
而 `const` 没有前导下划线，于是溜过了私有过滤，
被报成 MISSING 四次，每个私有 extension type 一次。

**一把尺子头一次跑的结果，只值它那次跑的审计。**这一条已经是这个会话里的第三次了。

### 而更大的一件事：`rust:` 字段从来没有被读过

台账的 `equivalent` 是这样的：

```json
"GestureDisposition": {"rust": "gestures::Disposition", "note": "..."}
```

`classify()` 里写的是 `if c in eq: yield 'mapped'`——**只看键在不在**。
那个 `rust:` 从来没有和代码比对过。
448 条对应关系，占全部类型的两成，**靠的是一个 JSON 文件里的断言**。

这和这个会话里反复撞见的是同一个物种——**一句没人核对的出处**——
只是规模最大的一次。

加上核对之后，**81 条指着 crate 里不存在的东西**。已经查实的两条：

| 台账写的 | 实际 |
|---|---|
| `SingleChildRenderObjectWidget` → `SingleWidget` | 那个名字早没了，现在是 `framework::single` |
| `MultiChildRenderObjectWidget` → `ManyWidget` | 同上，`framework::many` |

**改名之后没有任何东西发现台账过时了**，因为没有任何东西在读它。

### 这个核对本身也被变异过两次

第一版按 `::` 切，报 225 条不存在——因为 `animation::Animation trait`
切出来的 `Animation trait` 不是标识符，而 `animation` 是模块，
`rust_identifiers` **故意**不收 `mod`。改成抓标识符 token，降到 91。

第二版让模块名也能坐实一条映射（`painting::matrix_utils` 是正当的写法：
上游那个类本来就是一袋静态方法）。这一条要分清楚：
**排除 `mod` 是防「`mod actions` 碰巧盖住 `Actions`」这种意外，
而台账里写 `painting::matrix_utils` 是一个人在说端口在哪——两个问题，两套规则。**

第三版是变异逼出来的：把 `GestureDisposition` 指向
`gestures::NoSuchTypeAnywhere`，**居然还是绿的**——因为 `gestures` 是模块名，
光这一个就满足了检查。而台账里**每一条**都以层名开头，
于是这个检查对所有条目都会通过，无论后面写什么。
改成**开头那一段是上下文、不是主张**（有多段时丢掉第一段），
再变异，81 → 82。

### 这一轮没做的

37 个 MISSING 和 81 条空映射都留着，没有硬塞进这一轮。
一次补 49 个枚举，和当初那个 0 是一类错误——**看起来完成了**。
`WrapCrossAlignment` 尤其要单独一轮：它只有 start/end/center 三个变体，
而移植的 `RenderWrap` 用的是 `CrossAxisAlignment`（五个，多出 stretch 和 baseline），
**移植能表达上游表达不了的状态**，这不是改名，是变宽，不能当映射记。

---

## 三个变体的枚举，被当成五个变体的那个用了（2026-08-24）

上一轮把 `WrapCrossAlignment` 留了下来，理由是它**不是漏移，是变宽**。
这一轮补上。

上游 `WrapCrossAlignment` 只有 start / end / center。
`CrossAxisAlignment` 有五个——多出 `stretch` 和 `baseline`。
少的这两个不是上游忘了填：**把孩子拉满一行会和「行高由最高的那个孩子定」打架**，
而基线要拿一行里每个孩子的文字互相量，wrap 不做这件事。

移植原来在 `RenderWrap` 上直接用 `CrossAxisAlignment`，
并且把多出来的两个**折到 `Start` 上**——注释里写得明明白白。
于是调用方可以向 wrap 要 `Stretch`，安静地拿到 `Start`：
**一个上游的类型根本不让你写出来的状态。**

修法是把类型收窄，不是把折叠写得更清楚：
那两个要不到的东西，应该是**问不出口**，而不是问了给个别的答案。
收窄之后 4786 条测试一条没动——这个宽度从来没被用过，
是个潜伏的口子，不是在用的功能。

### 顺手把六条分支换成上游的一个数

上游把整条规则放在一个分数里：

```dart
double get _alignment => switch (this) { start => 0, end => 1, center => 0.5 };
WrapCrossAlignment get _flipped => switch (this) { start => end, end => start, center => center };
```

摆一个孩子就是一次乘法，翻转在乘之前发生。
移植原来是六条 match 分支，每条里面各自判一次 `flip_cross`。
**六条分支可以互相矛盾，一个数不能。**

### 变异又抓到同一个物种

七个变异，第一轮**活了一个**：把 layout 里的翻转整段删掉，全绿。
因为我所有测试用的都是横向、垂直方向朝下的 wrap，`flip_cross` 一直是 false——
`flipped()` 被单独测过，**却从来没有从调用它的那条路上走过一遍**。

补测试时又撞到一层：`VerticalDirection::Up` 不只翻转孩子在行内的位置，
**也把行本身挪到了另一头**，所以孩子的绝对 `dy` 里混着行原点的移动。
改成量「孩子相对于同行那个最高的孩子」——最高的那个填满整行，
无论哪头朝上都贴着行的边——才是 `WrapCrossAlignment` 真正决定的那个量。

再加一条测试防它白过：start 和 end 翻转前后**必须落在不同的地方**，
而 center 必须落在同一个地方。

补完之后七个变异全红。

coverage 2102 / 1985 记账 / **36 MISSING**（少一个）/ 81 条空映射。

---

## 六个格式塌成三个小时，而默认本地化只认得其中两个（2026-08-24）

`TimeOfDayFormat`（6 个）和 `HourFormat`（3 个）都在上一轮新暴露的
MISSING 里。它们不是两张名字表，中间有一个真正的推导：

```dart
HourFormat hourFormat({required TimeOfDayFormat of}) => switch (of) {
  h_colon_mm_space_a || a_space_h_colon_mm => HourFormat.h,
  H_colon_mm                               => HourFormat.H,
  HH_dot_mm || HH_colon_mm || frenchCanadian => HourFormat.HH,
};
```

**两个十二小时的格式只差「上午下午标记在哪一边」，
三个补零的只差「小时和分钟之间放什么」——两件事都不是关于小时的。**

顺带一个命名上的不一致：六个里五个按自己的样子命名
（`HH_colon_mm` 就是 ICU `HH:mm`），**只有 `frenchCanadian` 按用它的人命名**，
按前五个的规矩它该叫 `HH_h_mm`。名字照抄上游——
两边搜 `frenchCanadian` 都该搜得到。

### 而 `formatHour` 只在六个里的两个上是全函数

```dart
case TimeOfDayFormat.a_space_h_colon_mm:
case TimeOfDayFormat.frenchCanadian:
case TimeOfDayFormat.H_colon_mm:
case TimeOfDayFormat.HH_dot_mm:
  throw AssertionError('$runtimeType does not support $format.');
```

这不是需要被抹平的疏漏。子类的 `timeOfDayFormat` 可以返回六个里的任何一个，
而 `DefaultMaterialLocalizations` 只写得出两个；
**拒绝就是它说这件事的方式**，好过挑个最接近的、然后在四个语言里安静地错。

### 移植原来把两步压成了一个布尔

`format_hour(hour, always_use_24_hour_format)` 直接走，
**从来不构造 `TimeOfDayFormat`**。答案是对的——
因为它能到达的两个格式，恰好就是能写出来的那两个。

**于是那条拒绝没有地方可以存在。**

把格式变成参数（`format_hour_in(format, hour)`），
另外四个才重新回到调用方和测试够得着的范围里。
`format_hour` 保留成两步的复合，并有一条测试钉住它答得和从前一模一样。

### 一条防「两条臂算出同一个数」的测试

`hour_format` 把 `HH_colon_mm` 和 `HH_dot_mm` 映到同一个 `HourFormat`。
要是这两个格式本身也没差别，这个「塌缩」就什么都没塌。
所以专门断言：**同一个 hour format，分隔符必须不同。**

六个变异全红。coverage 2102 / 1987 记账 / **34 MISSING**。

---

## 第三条臂不问天黑没黑（2026-08-24）

`ThemeMode` 也在上一轮暴露出来的 MISSING 里。三个变体是小事，
挂在它上面的 `_themeBuilder` 不是：

```dart
if (useDarkTheme && highContrast && widget.highContrastDarkTheme != null) {
  theme = widget.highContrastDarkTheme;
} else if (useDarkTheme && widget.darkTheme != null) {
  theme = widget.darkTheme;
} else if (highContrast && widget.highContrastTheme != null) {   // <-- 没有 useDarkTheme
  theme = widget.highContrastTheme;
}
theme ??= widget.theme ?? ThemeData();
```

**「先定亮暗、再在里面挑对比度」这种显然的重写是错的。**
第三条臂根本不问 `useDarkTheme`。于是一个应用：暗色模式、开着高对比度、
给了 `highContrastTheme` 但没给 `highContrastDarkTheme` 也没给 `darkTheme`
——**拿到的是亮色的高对比度主题**。

这几条臂是**按顺序试、先合适的赢**，不是一棵关于（亮暗，对比度）的决策树。
这是不是有意为之是另一个问题；代码就是这么跑的，
而一个只给了四个主题里一部分的应用看得见。

### 而 `_isDarkTheme` 和 `_themeBuilder` 把同一件事拼写了两遍

```dart
// _themeBuilder
final ThemeMode mode = widget.themeMode ?? ThemeMode.system;
// _isDarkTheme
return widget.themeMode == ThemeMode.dark ||
    widget.themeMode == ThemeMode.system && ...;
```

**后者没有 `?? ThemeMode.system`。**而 `themeMode` 的类型是 `ThemeMode?`
（构造函数默认给的是 `system`，但显式传 null 是合法的）。

于是：显式传 null、平台是暗色时，
`_themeBuilder` 把 null 读成 system 从而选了暗色主题，
`_isDarkTheme` 返回 **false**——
**一个暗色的应用，把「你在亮色主题上」告诉了文本选择控件。**

照上游的样子移，不改。理由和 `CupertinoFormRow::error_color` 那次一样：
**故意照抄的差异是看得见、对得上的。**

另配一条测试防它变成噪音：除了「显式 null + 暗色平台」这一个格子，
其余每个（mode，platform）组合两种拼写都必须一致。

六个变异全红。coverage 2102 / 1988 记账 / **33 MISSING**。

---

## 同一份墨水，两个桶（2026-08-24）

`RenderComparison` 也在新暴露的 MISSING 里。四个变体，**而顺序就是它的全部**：
上游比的是 `.index`，不是值——

```dart
if (comparison.index >= RenderComparison.layout.index) {  markNeedsLayout(); }
else if (comparison.index >= RenderComparison.paint.index) { /* 只重画 */ }
```

`TextSpan.compareTo` 沿着 style 和每个孩子走，**保留一个跑动的最大值**，
并且**一到 layout 就提前返回**：

```dart
if (candidate.index > result.index) { result = candidate; }
if (result == RenderComparison.layout) { return result; }
```

这个提前返回**不是近似，是精确的**——因为 layout 是梯子的顶，
后面的孩子不可能把答案抬得更高。测试直接钉这一条：
`Layout.worse_of(任何一个) == Layout`。

### 而 `TextStyle.compareTo` 的那条分界线不在看上去的地方

- **`color` 算 paint。`foreground` 算 layout。**

同一份墨水的两种写法，落在两个桶里。因为 `Paint` 可以带描边宽度，
而**描边会把字形撑宽**；上游看不进一个 `Paint` 里面，就按最坏的算。
一个纯颜色动不了任何东西，而且这一点是known的。

`shadows` 也在 layout 那边，同样的保守：阴影不改度量，
但上游把它归到「我不打算推理的字段」那一堆里。

本移植的 `TextStyle` 既没有 `foreground` 也没有 `shadows`，
所以这条规则是**记下来的，不是跑出来的**；它有的那些字段按上游的分法分。

`align` 是本移植自己加在结构体上的（上游把对齐放在 painter 上而不是 style 上），
两张表里都没有它。这里算 layout——对齐会挪字形，上游要放也会放那边。

### Metadata 这一级本移植还产生不出来

上游从 `TextSpan.compareTo` 里 `recognizer` 不同的时候得到它——
**一个既不挪也不重上色的差别**。本移植的 span 不带 recognizer。

那一级**照样留着**，因为梯子的编号是上游的：拿掉它，
`paint` 在这边就成了第二级、在那边还是第三级。

六个变异全红。coverage 2102 / 1989 记账 / **32 MISSING**。

---

## 平铺的两端，不是从同一条边量的（2026-08-24）

`ImageRepeat` 四个变体，而上游的铺砖器**从不按四种情况分支**——
它分别问两个轴：

```dart
if (repeat == ImageRepeat.repeat || repeat == ImageRepeat.repeatX) { ...x... }
if (repeat == ImageRepeat.repeat || repeat == ImageRepeat.repeatY) { ...y... }
```

`Repeat` 只是那个两边都答「是」的值。

### 起点和终点量的是不同的边

```dart
startX = ((outputRect.left  - fundamentalRect.left ) / strideX).floor();
stopX  = ((outputRect.right - fundamentalRect.right) / strideX).ceil();
```

**起点用 left−left，终点用 right−right**，各自对着砖的同侧边。
两端都从砖的近边量（也就是写成 `outputRect.width / stride`）
会在砖有悬出的时候**多出一块**。

500 宽的盒子、50 宽的砖：`(500−50)/50 = 9`，于是 0..=9 十块，正好 500。
按错的写法是 `ceil(500/50) = 10`，0..=10 十一块。
测试把这两个数**都写出来**，断言它们不同——
不然「对」只是碰巧和另一种算法一致。

### 区间是闭的，所以不平铺也还是一块

不重复的轴返回 `(0, 0)`，是**一块，不是零块**。
`noRepeat` 是「画一次」，不是「不画」。

### 而正好填满时，重复会被提前关掉

```dart
if (repeat != ImageRepeat.noRepeat && destinationSize == outputSize) {
  repeat = ImageRepeat.noRepeat;
}
```

图片已经把盒子填满，铺格子什么也得不到。这个降级**是看得见的**：
它是「生成一块」和「先把一整个网格算出来再生成一块」的区别。

### 一次我自己算错

`a_tile_starting_before_the_box_gets_a_negative_index` 头一次是红的，
**错的是我写在测试里的期望值，不是代码**：
`floor((0 − (−25)) / 50) = 0`，我写了 1。
把三块砖的位置（−25、25、75）写进注释之后改对。

七个变异全红（其中一个先写成了删 `#[default]`，那是编译错误不是变异，
改成把默认挪到 `Repeat` 才算数）。

coverage 2102 / 1990 记账 / **31 MISSING**。

---

## 那个默认值只在两种单位里的一种下存在（2026-08-24）

`CacheExtentStyle` 两个变体，`pixel` 和 `viewport`。
差别不是「一个是另一个的特例」——**像素那一支根本不看视口**：

```dart
// _PixelScrollCacheExtent
double _calculateCacheOffset(double mainAxisExtent) => pixels;
// _ViewportScrollCacheExtent
double _calculateCacheOffset(double mainAxisExtent) => value * mainAxisExtent;
```

上游把数值和单位**绑在一个值里**（一个 sealed class，两个构造函数），
因为**两半单独拿出来都没有意义**：250 在手机上是一屏，在桌面窗口上是一条边；
0.5 是半个视口还是半个像素。绑在一起，那个错配的组合就写不出来。

### 而缺省只有一种单位说得通

```dart
assert(cacheExtent != null || cacheExtentStyle == CacheExtentStyle.pixel)
```

不给数值，在像素下没问题——`defaultCacheExtent = 250` 差不多是一屏。
**同一个 250 当乘数就是「缓存 250 个视口」**，
所以上游**拒绝这个组合**，而不是另编一个没人写下来的第二默认值。

移植里 `ScrollCacheExtent::defaulted` 因此返回 `Option`：
像素少了数值有救，视口少了数值没救。

### 一条防「两条臂碰巧相等」的测试

乘数是 1 的时候，两种单位**在恰好一个视口尺寸上相等**，而那个尺寸就是数值本身。
所以测试同时断言：600 时相等，601 时不等。

六个变异全红。coverage 2102 / 1991 记账 / **30 MISSING**。

---

## 月年选择器不是四种情况，是把「日」划掉（2026-08-24）

`DatePickerDateOrder`（dmy/mdy/ymd/ydm）和 `DatePickerDateTimeOrder`
——就是最初那一轮去看 `CupertinoLocalizations` 时发现移植里没有、
而 `coverage.py` 报 0 MISSING 的那两个枚举。

上游在 `monthYear` 模式下另写一个 switch，把四种配成两对：

```dart
case DatePickerDateOrder.mdy:
case DatePickerDateOrder.dmy:
  pickerBuilders = [_buildMonthPicker, _buildYearPicker];
case DatePickerDateOrder.ymd:
case DatePickerDateOrder.ydm:
  pickerBuilders = [_buildYearPicker, _buildMonthPicker];
```

文档里也直说：「both `dmy` and `mdy` will result in the month|year order」。

**但这不是四种情况，是同一个顺序把「日」划掉。**
`Mdy` 去掉 Day 剩 month, year；`Dmy` 去掉 Day 也剩 month, year；
`Ymd` 和 `Ydm` 都剩 year, month。

移植里 `month_year_columns()` 就是这么算出来的，不是又抄一遍表。
**推导出来的配对是个后果；抄两张表出来的配对只是两张表碰巧一样。**
测试直接断言这条推导对四个值都成立。

### 而 date-time 那四个，重点在没写出来的二十个

二十四种排列，上游只命名四种。漏掉的那些就是规则：

- **小时永远紧挨在分钟前面**——钟点是从左往右读的；
- **日期永远在一头或另一头**，从不夹在中间——
  不该为了够到分钟而先趟过一个日期。

两条都写成了对全部四个值的断言，而不是逐个列出来。
再加一条防空转：两头都真的有人用（`DateTimeDayPeriod` 在前，
`TimeDayPeriodDate` 在后），否则「永远在一头」可以是空话。

六个变异全红。coverage 2102 / 1993 记账 / **28 MISSING**。

---

## 两个都答「没处理」，只有一个松开了焦点（2026-08-24）

`TraversalEdgeBehavior` 四个值，落到边界上时：

| | 松开焦点 | 报告「我移动了」 |
|---|---|---|
| `closedLoop` | 否 | 是（绕到另一头） |
| `leaveFlutterView` | **是** | 否 |
| `parentScope`（有父作用域） | 是 | 交给父的结果 |
| `parentScope`（没有） | 否 | 是——**回落到 closedLoop** |
| `stop` | **否** | 否 |

### `leaveFlutterView` 和 `stop` 的全部区别，就是那一列

两个都 `return false`。**只有前者先 `focusedChild.unfocus()`。**
把它们并成一个，就会出现「浏览器接走了下一个 tab，
而一个 widget 上还留着焦点框」。

### 而 `parentScope` 走投无路时是绕，不是停

上游注释写着：「No valid parent scope. Fallback to closed loop behavior.」
而且「有效」**不包括根作用域**——所以顶层的 `parentScope` 没处可去。

它这时的行为是**绕回另一头**。一个本打算把焦点交出去的作用域，
发现外面没人接，于是变成一个环，**不是一堵墙**。
测试同时断言它此时等于 `closedLoop`、且不等于 `stop`。

### 滚动对齐跟的是走的方向，不是落脚点

```dart
alignmentPolicy: forward
    ? ScrollPositionAlignmentPolicy.keepVisibleAtEnd
    : ScrollPositionAlignmentPolicy.keepVisibleAtStart,
```

普通一步和绕回去那一步**传的是同一个表达式**。
于是往前绕、落在**第一个**节点上时，要的仍然是 `keepVisibleAtEnd`——
「落在第一个，所以对齐到开头」这种直觉读法，正是上游没有做的事。
这个策略问的是**用户在往哪边走**，不是**停在了哪里**。

### 这一轮没做的

本 crate 的 `focus::next()` 目前就是硬写死的 `ClosedLoop`——
它没有作用域树，没有父作用域可交。所以这一轮加的是**那个判断**
（把「有没有有效父作用域」当参数收进来），并把现状写在注释里。
真正接进 `step()` 要等作用域树，那是另一轮。

六个变异全红。coverage 2102 / 1994 记账 / **27 MISSING**。

---

## 状态机是全的，走过它的测试只有一半（2026-08-24）

`RenderAnimatedSizeState` 在 MISSING 名单里，去看的时候发现
**移植早就把整台状态机写好了**，只是叫 `AnimatedSizeState`。
四个 `layout_*` 函数和上游 `_layoutStart/_layoutStable/_layoutChanged/_layoutUnstable`
**逐臂一致**。所以这是一条映射，不是缺口。

但映射只值它背后那次核对。核对的时候顺手问了一句：
**这台机器有多少是被测试走过的？**

两个探针，都活着：

- 把 `layout_changed` 里 `-> Unstable` 改成 `-> Stable`：**全绿**。
- 把 `unstable -> stable` 的 `controller.stop()` 改成 `forward()`：**全绿**。

`Unstable` 那一半**一次都没有被走过**。

### 而原来那条测试根本没驱动任何东西

```rust
child_cell.set(Size::new(100.0, 100.0));
animated.layout(BoxConstraints::loose(200.0, 200.0));
assert!(animated.size().width < 100.0);
```

`layout_child` 在「孩子不脏且约束没变」时**直接返回缓存的尺寸**。
测试换了 cell 里的值却没有 `mark_needs_layout`，
于是孩子从来没有重新布局，盒子也从来没看见变化——
**那条断言成立，是因为什么都没发生。**

这已经是这个会话里第七轮同一个物种了：*一条不走它所说的那条路的测试*。

### 补上之后，两条微妙的规则才真的被钉住

**一、`stable -> changed` 是追，`changed -> unstable` 是贴。**

第一次变化：`begin = 当前尺寸`、`end = 孩子尺寸`——从眼睛看到的地方动过去。
第二次变化：`begin = end = 孩子尺寸`——**两端相同，等于不动画，直接贴住**。
上游的文档写得明白：「Instead of chasing the child's size in this state,
the render object tightly tracks the child's size until it stabilizes.」
追一个每帧都在跑的目标，比跟着它还难看。

**二、`unstable -> stable` 停表，`changed -> stable` 续跑。**

同一个目的地，相反的动作。理由就是上一条：
`changed` 一直在动画，还有一段没放完；
`unstable` 一直只是在贴，没有任何东西在跑。

六个变异全红（含最早那两个探针）。
coverage 2102 / 1995 记账 / **26 MISSING**。

---

## 一张没人查的对照表，可以错一行错到底（2026-08-24）

上一轮发现 `Unstable` 那半台状态机从没被走过。**那就该问：还有多少？**

`tools/unwalked.py` 数了一下：**1332 个公共枚举变体，252 个从没在任何测试里被写出来。**

### 但这是筛子，不是尺子

写出来 ≠ 走到过，没写出来 ≠ 没走到——
一个变体可以经由 `Default`、经由对 `ALL` 数组的遍历、
或者被两个模块以外的调用方碰到。所以**每一条都要用变异确认过才算数**，
和当初那个引用探针一样。

它**不做验收门**。252 条是一个和 MISSING 并排的待办队列，
不是一个要赶紧清零的数字；而且里面一定有一些查下去其实是走到了的——
那正是筛子被允许给出的答案。

它自己头一次跑也是错的：**只拿每个文件和它自己的测试模块比**，
于是 `TextAlign` 的变体被报成没测——它们是在 `painting.rs` 的测试里被用的，
不在 `engine.rs` 的。**探针错了，不是移植错了**，
和当初只扫 `packages/` 而漏掉 `dart:ui` 是同一个形状。

### 抽查两条，两条都是真的

- 把 `ConstraintsTransform::MaxWidthUnconstrained` 改成什么都不做：**全绿**。
- 把 `HapticFeedbackType::Heavy` 的载荷换成 `lightImpact`：**全绿**。

第二条尤其难看：那是一张发给宿主的字符串对照表，
**错一行没有任何东西会发现**。

### 补上之后，两处真正的区别才被钉住

**`WidthUnconstrained` 连下限一起扔，`MaxWidthUnconstrained` 只扔上限。**

上游 `widthUnconstrained` 是 `constraints.heightConstraints()`——
整个宽度都不要了；`maxWidthUnconstrained` 是
`copyWith(maxWidth: infinity)`——**下限还在**。
一个把两者并起来的移植，会让本该至少 10 宽的孩子可以缩到 0。

另配一条测试：七个 transform 两两作用在同一组约束上，
**结果必须互不相同**，防止上面每一条都在「两条臂碰巧相等」的情况下通过。

**而「标准震动不带参数」也是协议的一部分**：
上游对普通那一下调 `HapticFeedback.vibrate` 不给参数，
四个具名的给一个——宿主就是照这个分支的。

五个变异全红。

---

## 名字形状的筛子，换成行为形状的扫（2026-08-24）

上一轮那把 `unwalked.py` 问的是「这个变体在测试里写出来过吗」。
抽查的**第一个枚举就整个是假阳性**：`WidgetStatesConstraint` 五条臂全被标成没测，
逐条变异下来**五条全都被抓**——测试是用 `.and()` / `.or()` / `.not()` 造的值，
从来不写 `WidgetStatesConstraint::And` 这个字面路径。

**这是这个项目里第四次拿名字形状的启发式去顶行为形状的问题**
（`unwired.py` 自己就占了四次）。所以换成 `tools/variant_sweep.py`：
照 `order_sweep.py` 的老办法，**把一条 match 臂改成返回上一条臂的值**，
跑全套测试，看有没有人吭声。两个状态变得分不出来，是个真的改动。

### 两次扫，两处真东西

**`WidgetState::bit`**：八个状态各占一位，**其中三条臂可以和上一条撞位而无人发现**。
两个状态共用一位，集合就分不出它们——问其中一个会因为另一个在场而答「是」。
补的测试是**对每一对做的性质断言**，不是八条各写一遍：
以后加第九个状态，不用谁记得来改这里。

**`SystemMouseCursor::kind`**：十八行里**十七行**可以顶替上一行的字符串，全绿。
这些字符串是**协议**——宿主照着查——这边没有任何东西读它们，
所以错一行在这边是**结构性地看不见**。

而对着上游一比，又发现：**移植只带了上游三十六个光标里的十八个**。
带的那十八个拼写全对，另外十八个（`contextMenu`、`grab`、`grabbing`、
八个单向 resize、`zoomIn`/`zoomOut` 等）根本不在。
**`coverage.py` 看不见这种缺**——它数的是类型，而类型是在的。

补全三十六个，并把整张表钉成一个 `ALL` 数组：
拼写逐行对上游、三十六个 kind 互不相同、每个变体在表里恰好出现一次。
最后一条是防以后加了变体却忘了加行——那正是这次的起点。

再扫一遍：**19 条存活降到 3 条**，光标那张表全被抓住。
（剩下 3 条没能指认：那一次运行还没加 stdout 刷新，输出被缓冲截断了。
刷新已经补上，指认留给下一次扫。）

### 顺带记一笔：这类缺口有多普遍

写了个只读的对照，把上游枚举和移植的变体数逐个比。
**130 个两边都有的枚举，只有 4 个少了值。** 光标那种是特例，不是普遍现象。

而头一次跑报的是 6 个，**两个是我自己工具的错**：
一个要求变体首字母大写，于是把 `TimeOfDayFormat::h_colon_mm_space_a`
报成缺失（那是我自己两轮前加的）；
另一个没过滤上游的私有值，把 `ContentSensitivity._unknown` 当成了缺口。
**三分之一的「发现」一查就没了**——先审后动的理由。

---

## 一个上游已经改掉的名字，和两个「问题」而不是「答案」的值（2026-08-24）

上一轮那个只读对照报了四个真缺口，这一轮补完，剩一个。

### `SelectionChangedCause` 停在了退休的名字上

上游把 `scribble` 改名成了 `stylusHandwriting`，
旧名只留成一个 deprecated 别名：

```dart
static const SelectionChangedCause scribble = stylusHandwriting;
```

移植还叫 `ScribbleUpdate`。改成 `StylusHandwriting`。
**别名不带**——它在上游存在是为了让改名前写的代码还能编译，这边没有那种代码。

### `OptionsViewOpenDirection::MostSpace` 是个问题，不是一个方向

```dart
final bool opensUp = switch (direction) {
  up => true,
  down => false,
  mostSpace => spaceAbove > spaceBelow,
};
```

`Up` 和 `Down` 是答案，**这个是提问**：哪边地方大往哪边开。
而且那两个固定方向**根本不看两个参数**——
一个被要求朝上开的字段，上面没地方也照样朝上开，上游由它。

比较是**严格的**，所以两边一样大时朝下开。
这不是随手偏一边：`Down` 是默认值，而平手正是「没学到任何该离开默认的理由」。

本来以为这个枚举在移植里没有消费者（只被存着），
结果**编译器指出 `autocomplete_view.rs` 的摆放函数就在 match 它**——
于是 `MostSpace` 接进了真正的摆放，而不是挂一个空变体。
测试也从那个闭包走：字段贴近顶部时它和 `Down` 一致，
贴近底部时和 `Up` 一致，而这两个答案本身必须不同
（否则上面两条对任何实现都成立）。

### `MultitouchDragStrategy::SumAllPointers`，和两个问题

上游用两处代码把三种策略分开，**而单独任何一处都分不开三种**：

- `_shouldTrackMoveEvent`：sum 和 average 都跟所有手指，latest 只跟当前那根；
- `_recordMoveDeltaForMultitouch`：开头就 return，**除非**是 average。

**求平均要知道每根手指每帧动了多少**（才找得出最外侧那一对），
求和不在乎是谁贡献的。于是两个问题、三种组合。
测试直接断言这三组答案两两不同。

### 剩下那一个，是记下来的减法

`ScrollPositionAlignmentPolicy` 少一个 `explicit`。
上游那个值的意思是「用 `ensureVisible` 的 `alignment` 参数决定」——
它不是第三种滚法，**是把决定权让给一个本移植根本不带的数**。
这边每个调用方都是遍历要求把下一个节点带进视野，正好就是那两个 keep-visible。

把理由从「as much of it as the pop needs」改写成了上面这段：
**加第三个变体就会是一个背后什么都没有的名字**，和当初拒绝 `TextWidthBasis` 是同一条理由。

七个变异全红。130 个共有枚举里少值的从 4 降到 1。

---

## 同一个意图，两套看不出关系的算术（2026-08-24）

`AnimationBehavior` 两个值，`_enableAnimations` 是
`normal → !disableAnimations`、`preserve → true`。
有意思的是**它喂给谁**：

| 机制 | 关掉动画时 |
|---|---|
| 有时长的动画 | 时长 × **0.05** |
| 弹簧（fling） | 速度 × **200** |

**不是零，是百分之五。**上游的注释给了理由，而且不是审美：
「零时长的动画框架理应能处理；但**永远重复的动画**要是不至少拖一帧，
可能会转成死循环。于是按正常时长的 5% 跑，把多数动画限制在一帧里。」

而弹簧**没有时长可除**——它跑到停为止——
所以让它立刻到位的办法是**扔狠两百倍**。
同一个意图，两套算术，唯一的共同点是都由那一个布尔决定。
测试专门断言**一个缩一个涨**，免得它们变成「同一个数的两种写法」。

### 两个构造函数的默认值不一样

`AnimationController` 默认 `normal`；
`AnimationController.unbounded` 默认 **`preserve`**——那是重复动画的那个，
上游说理由是「防止它们在屏幕上快速闪烁，如果 widget 自己没考虑这个标志的话」。

**于是无障碍设置默认是被尊重的，除了尊重它会更难看的地方。**

### 标志是参数，不是全局

这个 crate 没有把 `disableAnimations` 桥接进来。
和之前 `on_off_switch_labels` 一样的处理：**把标志当参数收进来**。
一个只可能答一种的开关不是开关，是个名字比较长的常量。

而且这一轮把这个比例**接进了 `Controller::tick`**，不是留一个没人调的谓词：
测试从时钟走一遍——一个被告知「读者不要动画」的控制器，
在一个 50 毫秒的帧里走完了本该二十帧的路。
把比例从 tick 里删掉那条变异是红的，所以这根线是真的。

六个变异全红。coverage 2102 / 1996 记账 / **25 MISSING**。

---

## 画的顺序和摸的顺序，永远互为倒序（2026-08-24）

`SliverPaintOrder` 两个值。上游把那条不变量**在两个 getter 上各写了一遍**：
`childrenInPaintOrder`「应当是 `childrenInHitTestOrder` 的倒序」，反过来也一样。

**这不是巧合，是要求。**画是从后往前，所以最后画的在最上面；
命中测试是从前往后，所以最先问的赢。两者一旦不再互为倒序，
手指就会点到读者看不见的东西上。

于是 `hit_test_walk` 是**从 `walk` 推出来的**，不是另抄一遍，两者不可能各自漂移。

### 而接进 viewport 的那部分，写了又撤了

`RenderSliverViewport` 仍然无条件按 `firstIsTop` 画和命中。
接线代码写完了，**又拿掉了，因为没有测试能把两种顺序区分开**：

viewport 里的 sliver 是一个接一个排的，两个普通 sliver 永远不覆盖同一个像素。
唯一会重叠的是 pinned header——而那条测试在给它的时间里没做出来。
两个变异（画忽略顺序、命中忽略顺序）**都活着**。

一个 setter 喂两个循环、而没有任何测试能分辨它们，
**就是「一个背后什么都没有的名字」**——正是上一轮把
`ScrollPositionAlignmentPolicy::explicit` 挡在外面的那条理由。
**这条规矩也得管我刚写的代码。**

要改变这一点，需要一条 viewport 测试：pinned header 和它下面的内容同时被命中，
由顺序决定谁来应答。

### 这一轮我错了两次，都是自己查出来的

**一、说 pinned header 的命中测试「缺失」，是错的。**
我去看 `hit_test_children`（box 那一层）没找到重写，就下了结论。
sliver 走的是 `sliver_hit_test`，而 `RenderSliverPersistentHeader`
**早就正确实现了它**——加第二个的时候编译器报了重复定义才发现。
移植没有这个 bug。

**二、又一次在后台扫的时候动了树。**
`TaskStop` 杀掉的是外层 shell，python 还活着，
于是我"恢复"完文件之后它又写了一次，测试红在一个我以为已经修好的地方。
`variant_sweep.py` 现在开跑前会检查残留的 `.sweep` 备份并先恢复——
`finally` 在进程被杀时是不跑的，而扫描正是那种会被人中途叫停的长活。

---

## 孩子溢出，盒子不溢出（2026-08-24）

`OverflowBoxFit` 两个值。孩子**两种情况下都是按溢出约束量的**——
那正是这个盒子叫 overflow box 的原因，也是孩子可以比装它的盒子大的原因。
这个枚举决定的是**盒子自己**的尺寸。

- `Max`：父允许多大就多大，**完全不提孩子**。
  正因为不提，上游才能把它标成 `sizedByParent`——
  尺寸里没有孩子，那么给孩子布局就不可能改变它。
- `DeferToChild`：`constraints.constrain(child.size)`。

第二条那个 `constrain` 是关键。孩子按溢出约束量出来可能是 300，
而盒子只允许 100——**于是孩子溢出，盒子不溢出**。
要是直接取孩子的尺寸，这个 widget 就没意义了：
它存在的全部理由就是让孩子超出自己而不超出父亲。

上游明确写「`deferToChild` ... 因此不能是 sizedByParent」。
移植把它当成事实带着（`sized_by_parent()`），
而不是当成一个钩子——这个 crate 的 `RenderBox` 没有把 resize 和 layout 分开。
真正依赖它的是 `compute_dry_layout` 要不要问孩子。

### 和上一轮的对比

上一轮 `SliverPaintOrder` 的接线被撤掉了，因为没有测试能分辨两个值。
这一轮不一样：`Max` 和 `DeferToChild` 在一个 20×20 的孩子上就给出不同的盒子尺寸，
干布局和实际布局都测了，五个变异全红。**同样的标准，不同的结果。**

coverage 2102 / 1998 记账 / **23 MISSING**。

---

## `manual` 不是第五种模式，是另一个方法（2026-08-24）

`DeviceOrientation`、`SystemUiMode`、`SystemUiOverlay` 三个都在 MISSING 里，
都出自 `system_chrome.dart`，一起补。

`setEnabledSystemUIMode` **看着像一个调用带五个选项，其实是两个调用**：

```dart
if (mode != SystemUiMode.manual) {
  invokeMethod('SystemChrome.setEnabledSystemUIMode', mode.toString());
} else {
  assert(mode == SystemUiMode.manual && overlays != null);
  invokeMethod('SystemChrome.setEnabledSystemUIOverlays', _stringify(overlays!));
}
```

另外四个是**交给平台去解释的模式**；`manual` 是「别解释了，就显示这几条」。
把它当成第五个模式字符串发过去，宿主那边根本没有对应的分支。

而且只有它需要 `overlays`——那条 assert 是这个配对的另一半。

### 线上的名字是 Dart 枚举的 `toString()`

上游发的是 `mode.toString()`，所以宿主读到的是 `"SystemUiMode.leanBack"`，
不是 `"leanBack"`。**去掉前缀在这边完全看不出来，在通道那头是错的。**
和光标那张表同一个道理：这边没有任何东西读它。

### 四个朝向的声明顺序是一圈，不是一种分类

`portraitUp, landscapeLeft, portraitDown, landscapeRight`——
既不是字母序，也不是「先竖后横」，而是**每次转四分之一圈**。
于是下标就是角度，任何一个的对面都是「往后数两个」。
测试断言的就是这条：隔两个的那个，必须和它同为竖屏或同为横屏。

六个变异全红。coverage 2102 / 2001 记账 / **20 MISSING**。

---

## 锁不是修饰键：一个是按住，一个是拨过去（2026-08-24）

`KeyboardLockMode` 三个值，每个**自己带着开关它的那把键**——
上游写成 `numLock._(LogicalKeyboardKey.numLock)`。

上游的说明就是要点：「每次按下对应的逻辑键，这个模式的状态翻一次」。
所以锁既不是键，也不是修饰键：**修饰键是按住的，锁是拨过去就留在那儿的**，
手上什么都不用按着。测试专门拿 Shift 做对照——
它和 Caps Lock 一样会改变字母，而它不是锁。

### 查找是那个配对的逆，不是第二张表

上游有个 `_knownLockModes` 映射给 `findLockByLogicalKey` 用。
移植**从配对推出来**，不另存一张表：
两张表可以对「哪把键开哪把锁」各执一词，一个推导不能。
测试断言的就是这条闭环：任何模式说出的那把键，必须再找回这个模式。

三把键还必须互不相同——共用一把键会让查找有歧义，
而且其中一把锁**永远到不了**。

四个变异全红。coverage 2102 / 2002 记账 / **19 MISSING**。

---

## `Ignored` 不是 `Unlocked`（2026-08-24）

`LockState` 三个值答的是一个是非问题，而**第三个值正是这个类型存在的理由**：
不在乎的快捷键两种情况都触发；写 `Unlocked` 的那个，锁开着时**拒绝触发**。
把它建模成 `bool` 的移植，必须替「没写」这种情况在两者里挑一个，
然后对一半的快捷键是错的。

### 而这需要键盘上的第二份状态

`lockModesEnabled` **推不出来**：锁开着的时候没有人按着它的键。
那正是锁和修饰键的全部区别，所以它需要另一份状态，
而不是对 `pressed` 再问一个问题。

上一轮那个「键↔模式」的配对在这里派上用场：
按下时用 `find_by_logical_key` 认出锁键，翻转它。
**按下才翻，抬起不翻**——一次按放是一次翻转，不是两次，
否则锁看上去永远不会变。

### 只有 num lock 有 LockState，而这不是随意的

`SingleActivator` 只给 num lock 这一把锁配了 `LockState`。
看着随意，直到去问每把锁到底做什么：
**num lock 改变的是小键盘上那些键「是什么键」**——是 `1` 还是 `End`——
所以绑在那些键上的快捷键必须说清楚要哪一种。
Caps lock 和 scroll lock 改变的是一个键**产出什么**，不是它**是哪个键**，
而快捷键绑的是键。

### 又一个存活的变异，同一个物种

让**每一次**按键都翻转 num lock——全绿。
因为测试里唯一被 `record` 过的键就是锁键本身：
`accepts` 是**读**键盘的，不是**喂**它的，
所以通过 activator 按下的那个 A **从来没有到过 `record`**。

补了两条：普通键按三次不翻任何锁；锁键的**抬起**也不翻。
补完之后六个变异全红。

coverage 2102 / 2003 记账 / **18 MISSING**。

---

## `Defunct` 是这一次尝试结束了，不是这个识别器结束了（2026-08-24）

`GestureRecognizerState` 三个值，转移只有三条，而**两条上都有守卫**：

- `addAllowedPointer` 整个函数体在 `if (state == ready)` 里面。
  **第二根手指按下来不会变成 primary**，也不会重启这次尝试。
  没有这条守卫，识别器就会跟着最后按下的那根手指走，
  双指触摸会**悄悄换掉**这个手势的目标。
- `rejectGesture` 同时守 pointer 和 state：
  `if (pointer == primaryPointer && state == possible)`。
  别的指针在竞技场里输了，和这个手势没关系；
  已经 `defunct` 的识别器也没有什么可以再放弃的。
- `didStopTrackingLastPointer` 开头是 `assert(state != ready)`——
  **没开始过就没法停止跟踪**。而它从 `defunct` 和从 `possible` 都回到 `ready`，
  这正是被拒绝过的识别器还能接着用的原因。

所以 `Defunct` 的意思是「我等的那个手势没有发生」，
**不是「这个对象完了」**。测试直接钉这一条：拒绝之后停止跟踪，
再按一根新手指，它照样被收为 primary。

五个变异全红。

### 顺带清掉一个退休的枚举，和一个我记错的前提

`KeyDataTransitMode` 上游自己标了 `@Deprecated`：
「Transit mode is always key data only.」它区分的是 raw key data 和 key data
两条投递路径，而 raw 那一族文件（`raw_keyboard.dart` 加七个平台变体）
**本台账早就记成 out_of_scope_files 了**。只剩一个取值有意义的枚举，
移过来就是个死名字。记账。

而我这一轮本来要移 `ModifierKey`，理由是「上游标了 deprecated，
所以要记账而不是移植」——查下来**它根本不在 MISSING 里**：
`raw_keyboard.dart` 整族早就出了范围，`ModifierKey` 从来没被报缺过。
**是我记错了前提，不是台账漏了。**

coverage 2102 / 2005 记账 / **16 MISSING**。

---

## 密码框里不能有智能替换（2026-08-24）

`SmartDashesType` 和 `SmartQuotesType` 各两个值，
而**默认值由 `obscureText` 推出来**——同一行写法在三处出现
（`EditableText`、`TextField`、`CupertinoTextField`）：

```dart
smartDashesType ?? (obscureText ? SmartDashesType.disabled : SmartDashesType.enabled)
```

这不是装饰性的：**智能替换会改写已经键入的内容**。
两个连字符变成破折号，直引号变成弯引号——在散文里无害，
**在密码框里就是悄悄改掉了读者以为自己输入的字符。**

### 而「没设」不等于两个值里的任何一个

参数在上游是可空的，第三种状态正是默认值得以存在的原因：
没设 = 「从 `obscureText` 决定」。
所以一个**确实想在密码框里要智能破折号**的字段，
仍然可以明写 `Enabled` 并拿到它。移植保留 `Option`。

这已经是这个会话里第四次遇到同一个形状了：
`LockState::Ignored`、`padding` 的 null 不是零、
`cacheExtent` 的没给不是零、这一个。
**每次都是「未设」和某个具体值长得像，而含义不同。**

### 两个参数是两个参数

上游分开写，移植也分开写：一个字段可以要破折号、不要弯引号。
一个「智能替换」总开关表达不了这件事。

四个变异全红。coverage 2102 / 2007 记账 / **14 MISSING**。

---

## 同一个小类型里，两个运算对「不存在」的看法相反（2026-08-24）

`BaselineOffset` 整个 `extension type` 只有两个运算，
而它们**对 `null` 的处理正好相反**：

- `+` 是**吸收的**：`BaselineOffset(null) + 10` 还是不存在。
  一个没有基线的盒子，不会因为被挪了一下就长出一条。挪动「无」得到的还是「无」。
- `minOf` 是**中性的**：真基线和不存在的那个合并，得到真的那个。
  一个没有基线的孩子，不该让它的兄弟们也失去基线。

**第二条错起来又容易又安静。**Rust 自己的 `Option` 排序把 `None` 排在
每个 `Some` 之下——顺手取个最小值，「不存在」就会赢下每一次比较，
一整行孩子里没有任何基线活得下来。
这条规则不是「最小的那个」，是**「存在的那些里最小的那个」**。

移植原来在 `RenderFlex` 里用 `filter_map` + `fold` 手写对了这件事，
但这两条规则没有名字、也没有地方。现在有了，
并且那处 fold 改成走这个类型——把 `shifted_by` 从 fold 里拿掉的变异是红的,
所以这根线是真接上的，不是摆着看的。

顺带钉了一条顺序：**先平移再比较**。
一个位置靠下、基线数值小的孩子，平移之后可能反而在更下面；
比较在平移之前做，答案会反过来。

五个变异全红。coverage 2102 / 2008 记账 / **13 MISSING**。

---

## 三种模式不是三张单子，是「谁来陪着分钟」（2026-08-24）

`CupertinoTimerPickerMode` 三个值。上游的 `initState` 把话说清楚了：

```dart
selectedMinute = widget.initialTimerDuration.inMinutes % 60;
if (widget.mode != CupertinoTimerPickerMode.ms) { selectedHour = ...; }
if (widget.mode != CupertinoTimerPickerMode.hm) { selectedSecond = ...; }
```

**分钟是无条件的**，只有另外两个带守卫。
所以这三种模式不是三张单位清单，而是**小时和秒各自要不要来陪分钟**。

### 于是分钟所在的列号会变

上游在每个调用点各自内联算一次：
分钟的 off-axis fraction 是 `mode == ms ? 0 : 1`，秒的是 `mode == ms ? 1 : 2`。
**两个都是列下标。**移植把它们从那张列表里推出来，
将来加一种模式，两处不会各说各话。

列数也一样：上游写 `mode == hms ? 3 : 2`，这里是 `columns().len()`。

### 一条不会有别的东西替我抓住的不变量

无论哪种模式漏掉谁，剩下的**必须保持 时·分·秒 的顺序**。
一个读作「43 sec | 14 min」的计时器是荒唐的，
而这件事**没有任何别的测试会发现**——列数对、单位齐、下标自洽，全都对得上。
所以单写了一条。

六个变异全红（其中三条第一次跑时锚点没对上：`cargo fmt`
把 match 臂折成了单行，这是老毛病了）。

coverage 2102 / 2009 记账 / **12 MISSING**。

---

## 第三个值不是改名，是因为平台视图没有孩子（2026-08-24）

`HitTestBehavior` 是 `deferToChild / opaque / translucent`；
`PlatformViewHitTestBehavior` 是 `opaque / translucent / **transparent**`。

**换掉的那个不是改名。**平台视图没有 Flutter 的孩子可以「交给」——
它是别人的一块画面。所以第三个选项不可能是「问我的孩子」，
只能是「我不在这儿」。

### 两个问题，不是三种情况

上游整个实现就两行：

```dart
if (behavior == transparent || !size.contains(position!)) return false;
result.add(BoxHitTestEntry(this, position));
return behavior == opaque;
```

问的是两件独立的事：**这个视图收不收这个事件**，和**它拦不拦后面的搜索**。
`translucent` 正是把两者分开的那个值——它**既把自己记进路径，又返回 false**，
于是它收到这次触摸，它背后的东西也收到。

第四种组合——不收事件却拦住搜索——**没有任何值产生**，
因为「挡住一个自己都不用的东西」不是谁想要的。测试把这条也断言了。

而边界检查在**读取 behavior 之前**：`opaque` 不等于「到处都是」。

### 移了判断，没有它要驱动的那块画面

读这个值的那些 render object 在本树里是 `blocked_engine`——没有平台视图通道，
`AndroidView` 那一族没地方发东西。但**这个判断不受此阻**：
它是对枚举做的算术，不是对一块外来画面做的，
和 `SmartDashesType::resolve` 不需要键盘就能测是同一个道理。

（这一点要和第 96 轮分清楚：那次是**接线**没法被测试分辨，所以撤掉了；
这次没有声称接了任何线，谓词本身就是全部内容。）

五个变异全红。coverage 2102 / 2010 记账 / **11 MISSING**。

---

## Fuchsia 从来走不到里面那层 switch（2026-08-24）

`AndroidOverscrollIndicator` 两个值。选哪个**由 `useMaterial3` 决定**，
不是由平台决定——但**平台可以让这个决定作废**：

```dart
final indicator = Theme.of(context).useMaterial3 ? stretch : glow;
switch (getPlatform(context)) {
  case iOS: case linux: case macOS: case windows:
    return child;                       // 一个都不画
  case android:
    switch (indicator) {
      case stretch: return StretchingOverscrollIndicator(...);
      case glow: break;
    }
  case fuchsia:
    break;                              // ← 根本没进里面那层
}
return GlowingOverscrollIndicator(...);
```

**Fuchsia 直接 break 到底下的 glow。**所以它在 Material 3 下也是发光，
那个决定种类的标志**只在 Android 上被读**。

于是这个枚举叫 `AndroidOverscrollIndicator` 是**精确的，不是随口**：
Android 是唯一一个它两个取值都可能成为答案的平台。

### 顺带修一句过时的注释

移植原来在 `builds_overscroll_indicator` 上写「It is the glow」——
在 M3 的 stretch 出现之前是对的，现在 Android 上它是拉伸。
改成只说「有没有」，把「是哪个」交给新的那个方法。

四个变异全红。coverage 2102 / 2011 记账 / **10 MISSING**。

---

## 两种模式收得一样多，差别在最后剩下什么（2026-08-24）

`NavigationBarBottomMode` 两个值。`maxExtent` **根本不提这个模式**——
完全展开时，两种模式下这条栏一样高。
差的是 `minExtent`：

```dart
double get minExtent =>
    persistentHeight + (bottomMode == always ? bottomHeight : 0.0);
```

`always` 把底部留在收缩行程的终点，`automatic` 不留。
于是**先被吃掉的东西也就不同**：
`automatic` 先吃底部再让大标题滑进去，`always` 底部钉住、大标题先滑。

### 而「能滚掉多少」是另一处单独写的

```dart
final bottomScrollOffset = widget.bottomMode == always ? 0.0 : _bottomHeight;
```

它和 `minExtent` 留下的那部分**是互补的**，而且**上游分两处写**——
所以它们可以被改到互相矛盾。测试直接钉这条不变量：
留下的 + 能滚掉的 = 底部高度，两种模式都成立。
矛盾的后果不是抽象的：要么这条栏把自己的一部分吃了两次，要么留下一道缝。

五个变异全红。coverage 2102 / 2012 记账 / **9 MISSING**。

---

## 三个值，两套机制（2026-08-24）

`DropdownMenuCloseBehavior` 三个值，而上游**不是在一处 switch 三次**，
是在**两个关闭不同东西的地方各读一次**：

```dart
closeOnActivate: widget.closeBehavior == DropdownMenuCloseBehavior.all,
...
if (widget.closeBehavior == DropdownMenuCloseBehavior.self) {
  _controller.close();
}
```

第一处把活交给菜单系统，它会往上走、把找到的都关掉。
第二处关的是**这一个**菜单的 controller，到此为止。

所以两个问题是「菜单系统关不关全部」和「这个菜单关不关自己」，
三个值是这两个布尔的四种组合里的三种。
**第四种——两个都做——不是一个值**，也不该是：
让菜单系统关掉一切、然后再关自己一次，是在要求一件已经发生的事。

`all` 和 `self` 从**这个**菜单看是一样的结果，
区别只有**站在外层菜单上**才看得见。测试把这一点单列了一条。

这已经是这个会话里第三次遇到「两个问题、三个答案、第四种组合无人使用」了
（`PlatformViewHitTestBehavior`、`MultitouchDragStrategy`、这个）。
每次那个空缺的组合都不是疏漏，而是**不想要的那件事**。

五个变异全红。coverage 2102 / 2013 记账 / **8 MISSING**。

---

## 又一次「画的和摸的互为倒序」，这回从语义树那头来（2026-08-24）

`DebugSemanticsDumpOrder` 两个值，各自对着一个不同的问题：

- `traversalOrder`：读者用「下一个 / 上一个」走过界面的顺序。上游到处的默认值。
- `inverseHitTest`：孩子**被问「你要不要这次触摸」**的顺序——最后一个先问。
  因为后画的盖在先画的上面，最后那个在最上面，就得先问它。

**两者互为倒序，是因为画和摸互为倒序**——
和 `SliverPaintOrder` 带着的是同一条规则，只是这次从语义树那一头撞上。
移植里 `children` 本来就按遍历序存着，所以另一个是它的**反转**，
不是第二张表——加一个孩子时两者不会各走各的。

四个变异全红。

### 顺带记账一个映射

`RepeatMode`（restart / reverse）对上本移植的 `animation::Repeat`。
逐条核过：上游 `_controller.repeat(reverse: mode == reverse)`，
于是 restart ↔ `Loop`（`rem_euclid(1.0)`，绕回 0），
reverse ↔ `PingPong`（折到 [0,2) 再镜像，并翻转方向）。
`Repeat::Once` 没有对应值——**只跑一次的 RepeatingAnimationBuilder 就不叫 repeating 了**，
那个值属于通用的 Controller，不属于这个 builder。

coverage 2102 / 2015 记账 / **6 MISSING**。

---

## 挡住一个节点，不等于挡住它的孩子（2026-08-24）

`AccessibilityFocusBlockType` 三个值。上游的 `_merge` 是三个 if，
说的其实是一句话：**两个节点合并时取更强的那个。**
`blockSubtree` 压过一切，然后 `blockNode`，都不是就是 `none`。

于是这又是一条梯子，和 `RenderComparison` 同一个形状：
全序、合并取最大、`none` 是幺元、顶是吸收元。
移植写成一次比较而不是三个 if——**三个条件可以被改到互相矛盾，一个序不能。**

### 中间那一级是这个类型有三个值的理由

「不让停在这个节点上，但里面的东西照样能到」是一件真实的需求：
一个容器本身不值得被读出来，它的内容值得。
**两个值的版本说不出这件事。**

测试按代数写而不是把九种组合列一遍：可交换、幂等、幺元、吸收元。
「谁先被问」这一条尤其要钉——合并的结果要是依赖顺序，
就变成了「取决于谁碰巧是父节点」。

五个变异全红。coverage 2102 / 2016 记账 / **5 MISSING**。

---

## 四个按钮里三个有标准键（2026-08-24）

移 `StandardComponentType` 的时候，撞见移植里这个：

```rust
pub fn has_standard_key(&self) -> bool {
    true
}
```

不收参数、没有任何输入能让它答别的——**又是那个物种**。
而且这次它**还是错的**。

上游四个 action button，只有三个往父类传 `standardComponent:`：

```dart
BackButton   : super(icon: ..., standardComponent: StandardComponentType.backButton);
CloseButton  : super(icon: ..., standardComponent: StandardComponentType.closeButton);
DrawerButton : super(icon: ..., standardComponent: StandardComponentType.drawerButton);
EndDrawerButton : super(icon: const EndDrawerButtonIcon());   // ← 没有
```

而且**根本没有 `StandardComponentType.endDrawerButton` 可传**。
于是测试找得到抽屉按钮，找不到右抽屉按钮。

原来那条测试只断言了「后退按钮有标准键」——是真的，所以它一直绿着。

### 两个枚举不是彼此的改名

三个重合，然后各有一个对方没有的：
`ActionButtonKind` 多 `EndDrawer`（两个抽屉各一个按钮），
`StandardComponentType` 多 `MoreButton`（弹出菜单的溢出按钮，根本不是 action button）。

它们问的是不同的问题：一个问「这是四个 action button 里的哪个」，
另一个问「测试要找的是哪个众所周知的部件」——
而后者点名的部件来自三个不同的家族（action button、popup menu、文本选择工具栏）。

### 键是从值本身造出来的

上游 `ValueKey<StandardComponentType>(this)`：
**两个部件不可能撞键，除非它们是同一个部件**，而一个键读回来就指名它的来处。
手写一张键名表两头都能漂：两个部件共用一个名字，或者一个名字谁也匹配不上。
测试把「四个名字互不相同」单列了一条。

五个变异全红。

### 顺带两个记账

- `WebHtmlElementStrategy`：web-only。**不是「类型只在 web 上有」**——
  `_network_image_io.dart` 里也有这个字段，但**只出现在构造函数、字段声明和 toString 三处，
  从不分支**。非 web 下它按构造就是惰性的；上游带着它是为了两套实现 API 一致，本移植只有一套。
- `UiKitViewGestureBlockingPolicy`：iOS 专属，且双重够不着——
  它管 UIKit 平台视图上 UIGestureRecognizer 的拦截时机，
  而 platform view 一族在本台账里已是 blocked_engine，UIGestureRecognizer 这边根本不存在。

coverage 2102 / 2019 记账 / **2 MISSING**。

---

## 三个数，两个坐标系，只有一样东西把它们连起来（2026-08-24）

`OverlayChildLayoutInfo` 是个三元组
`(childSize, childPaintTransform, overlaySize)`，
交给 `OverlayPortal.overlayChildLayoutBuilder`。
上游的文档对**每一个在哪个坐标系里**都写得很小心，而三者并不一致：

- `childSize` 在**孩子自己的**坐标里；
- `overlaySize` 在**overlay 自己的**坐标里;
- `childPaintTransform` 在 **overlay 的**坐标里。

于是三个里有两个各量各的、根本没法比，**第三个是唯一把它们关联起来的东西**。
一个要决定自己坐哪的 overlay child 三样都需要：
锚点多大、锚点落在 overlay 的什么位置、overlay 有多大地方。

而它不是一个 offset：**中间可能有东西把锚点缩放或旋转过**，offset 说不出这件事。
所以 `child_rect_in_overlay()` 在变换不是纯平移时返回 `None`——
那时候四个角构不成一个轴对齐矩形，硬给一个出去，
就是在错误的位置摆东西**而且一声不吭**。

四个变异全红。coverage 2102 / 2020 记账 / **1 MISSING**。

---

## MISSING 归零，而这不等于完成（2026-08-24）

最后一个是 `DynamicSchemeVariant`。它是 `ColorScheme.fromSeed` 的参数：
九个取值各选一个 `Scheme*` 类，而**那九个类在 `material_color_utilities` 包里**，
不在 `packages/flutter/lib/src` 下——所以 `coverage.py` 根本没数过它们，
`DynamicSchemeVariant` 才会孤零零地剩在最后。

本移植没有 `fromSeed`。`color_scheme.rs` 的模块注释早写下了理由：
HCT/CAM16 那套色调板算法是独立的一大块活。
**没有 fromSeed，这个选择器没有东西可选**——移过来就是又一个背后什么都没有的名字。

### 于是加了一个新的记账类别，而不是塞进旧的

塞进 `out_of_scope` 会在台账里写下一句**假话**。
「out of scope」说的是「这个移植永远不会要它」——web-only、iOS-only、
一个根本没有对应物的调试通道。
而这个是「想要，只是还没把它踩着的东西建起来」。

新类别 `blocked_unported_dependency`（报告里显示为 `blocked-work`）
单占一行，**不和 out-of-scope 混在一起**：
把「还没做」悄悄归进「不做」，就是把活退休掉。

反向验过：把这条记账删掉，MISSING 立刻回到 1。

### 而真正剩下的队列是 81 条空映射

`coverage.py` 现在报 **0 MISSING**，但同一份报告里还有
**81 条 `mapping-unresolved`**——台账里 `rust:` 指着 crate 里不存在的东西。
第 81 轮加的那个检查就是为它们准备的。

**「0 MISSING」不是「完成」。**它是「每一个上游公共类型都有一条记录」，
而其中 81 条记录还没被证明指向真东西。下一段活在那里。

---

## 那 81 条几乎全是记账格式问题，不是缺陷（2026-08-24）

上一轮我说「真正剩下的队列是 81 条空映射」。**说重了。**
分开数之后：**2 条是符号形状的，81 条是散文。**

```
BoxPainter          -> Decoration::paint          ← 符号，且找不到
LeastSquaresSolver  -> gestures::fit_quadratic    ← 符号，私有
其余 81 条           -> 「同上」「直接在里面（render.rs）」「各个容器自己存」…
```

`rust:` 这个字段一直在干**两份活**：
「这个上游类型对应哪个符号」和「这个概念在这边是怎么安置的」。
检查只验得了第一份。两者混在一起数，那个数字就什么也不说明。

### 于是把两种行分开声明，而不是给散文编个符号名

没有单一对应物的行**直接不写 `rust:`**，解释留在 `note` 里，
报告里单列成 `mapped-concept`。
**不并进 `mapped`**——否则「删掉 rust: 字段」就成了绕过检查的办法。
现在它是报告里单独的一行，只会看得更清楚，不会更隐蔽。
81 条转换过去，其中 47 条的散文是 `note` 里没有的，前置保留、没有丢。

### 剩下的两条

- `BoxPainter` 原来指着 `Decoration::paint`,**那个类型在本 crate 里根本没有**。
  真正对应上游 `_BoxDecorationPainter.paint` 的是 `BoxDecoration::paint`。
  **这条真错，而它一直埋在 81 条噪声里。**
- `LeastSquaresSolver → gestures::fit_quadratic` 是**对的但验不了**：
  那个函数是私有的，而 `rust_identifiers` 故意不收私有自由函数。

### 想把第二条也验掉，试了一次，更糟

把私有声明并进检查用的名字集合——**未解析从 1 涨到 25**。
因为「开头那一段是上下文」的规则会丢掉第一个 token，
而一旦**每个声明过的名字都算上下文**，`Decoration::paint` 这样的真主张
就被丢掉了承载主张的那一半。

**改回去了**，并把这次教训写进那个函数的文档：
留着这一条看得见的未解析，比让二十五条看不见强。

coverage 2102 / 2101 记账 / 0 MISSING / **1 条未解析** / 79 条 concept。

---

## 抽屉在 RTL 下所有手势都是反的（2026-08-24）

MISSING 归零之后，队列回到 `depth.py`。跑它，**它是坏的**——
第 81 轮我改了 `coverage.classify` 的签名，`depth.py` 是调用方，
而我此后三十来轮没再跑过它。**改了共享函数、留了个坏掉的调用方，三十轮没人发现。**
先修好。

修好之后看第一屏，大部分是它自己那个已经记录在案的高报模式
（`Icons` 8826 个图标常量、三个 list tile、`RichText`/`RawImage`）。
但 `DrawerControllerState` 0/6 是真的。

### `_directionFactor` 是一个 2×2

```dart
return switch ((Directionality.of(context), widget.alignment)) {
  (TextDirection.rtl, DrawerAlignment.start) => -1,
  (TextDirection.rtl, DrawerAlignment.end)   => 1,
  (TextDirection.ltr, DrawerAlignment.start) => 1,
  (TextDirection.ltr, DrawerAlignment.end)   => -1,
};
```

移植**只读了 alignment**：

```rust
match self.alignment {
    DrawerAlignment::Start => 1.0,
    DrawerAlignment::End => -1.0,
}
```

在从左往右的阅读方向下是对的，**在从右往左的阅读方向下两个抽屉全反**。
start 抽屉在那边是长在右边的，开它的那一划方向就反过来——
于是一个想关抽屉的滑动会把它打开。

两个参数**只在一起才有意义**：翻转任何一个都翻转答案，两个都翻转则不变。
这是一个异或，不是两条规则；只读一个参数的实现，四种情形里最多对一半。
测试直接钉这条结构，而不是把四个值列一遍。

四个变异全红——**其中 M1 就是原来那个 bug**。

coverage 2102 / 2101 记账 / 0 MISSING / 1 未解析 / 79 concept。

---

## 有钩子，没有挂在钩子上的那条规则（2026-08-24）

`depth.py` 那份 0.20 以下的名单一共二十条，多数是它自己记录在案的高报。
这一轮先把两条查清楚：

**`RefreshIndicatorState` 0/6 是高报。**移植的状态机比上游的六态还多一个 `Idle`，
只是方法名不同（`begin_drag`/`drag_by`/`release`/…）。
**没有靠读，靠变异确认**：把「未武装就放手也去刷新」「拿掉武装后的下限」
「on-edge 模式接受任何位置的拖动」「尾边溢出也算」四条各改一次——**四条全被抓**。

**`TextSelectionGestureDetector` 4/24 有真东西。**

### shift 是在手势开始时采一次，不是随时读

```dart
void onTapTrackStart() {
  _isShiftPressed = HardwareKeyboard.instance.logicalKeysPressed
      .intersection({shiftLeft, shiftRight}).isNotEmpty;
}
void onTapTrackReset() { _isShiftPressed = false; }
```

答案在**这一串点按开始时**取定，整串期间不变。
手指按下之后再按 shift，不会把一次点击追认成一次扩选；
拖到一半松开 shift，也仍然在扩选。
**每个事件都去读键盘是显然的实现，也是错的**——
那会在读者手底下改变这个手势的含义。

### 而移植有那两个钩子，没有挂在上面的东西

`TapStatusTracker` 一直在 `track_tap` 和 `reset` 里如实地触发这两个回调，
但整个 crate 里没有 `_isShiftPressed` 的对应物——
因为**这里的选择模型还没有「从锚点扩选」**，shift 点击扩选本身没实现。

所以这一轮补的是**那条规则本身**：它属于手势，不属于选择模型，
而承载它的机制已经在了。选择模型那一半仍然缺，如实记在类型的文档里。

三个变异全红，其中一个是「tracker 不再触发 reset 钩子」——
所以这条规则挂着的那个钩子是真的会响。

---

## 五个数在，挑哪一个的规则不在（2026-08-24）

`MaterialButton` 5/34。移植有 `ButtonElevations` 的五个值
（resting/focus/hover/highlight/disabled）和「都得非负」的断言，
**但没有「什么时候用哪个」**。

上游那段自己带着警告：

```dart
// These conditionals are in order of precedence, so be careful about
// reorganizing them.
if (isDisabled) return widget.disabledElevation;
if (isPressed)  return widget.highlightElevation;
if (isHovered)  return widget.hoverElevation;
if (isFocused)  return widget.focusElevation;
return widget.elevation;
```

**这些状态几乎总是重叠的**——按下的按钮几乎必然也悬停着，
禁用的按钮两者都可能——所以这不是五种情况，是**一条次序**，
而且每一对都在说事：

- **禁用压过按下**：一个不会响应的按钮，不该为了迎接这次点击而抬起来。
- **按下压过悬停**：用鼠标按下时两者同时为真，悬停要是先赢，
  highlight 那一级用指针根本到不了。
- **悬停压过聚焦**：焦点可以在指针不在的地方，而指针是更即时的那个。

四个变异——**相邻两级每一次对调**——全红。

### 一条测试是防另一条空过的

上游默认里 hover 和 focus **都是 4**。
所以拿默认值去测「悬停压过聚焦」，把这两臂对调也照样绿。
钉次序的那条测试因此用了五个互不相同的值，
另有一条把「默认下这两个相等」单独写出来——**那是巧合，不是规则**。

### 顺带删掉第三个空谓词

```rust
pub fn is_superseded() -> bool { true }
```

这次它**不是错的**（`MaterialButton` 确实被取代了），只是空的——
而且类型自己的文档注释上一行就写着同一句话。删掉，
那条测试改成断言它真正在验的东西：它的五个抬升就是 raw button 的五个。

---

## 把「空谓词」当成一个可搜的形状（2026-08-24）

这个会话里已经撞见三个了——`labels_beside_rather_than_inside`、
`has_standard_key`、`is_superseded`，其中一个还是**错的**。
这个形状可以搜：**参数里没有能变的东西，函数体是一个字面量。**

`tools/hollow.py` 扫出 **76 个**。

### 但 76 不是缺陷数——抽查三个，三个都对

- `FabMiniOffsetAdjustment::is_mini() -> true`：上游那个 mixin 的整个函数体
  就是 `isMini() => true`。**忠实移植。**
- `ActionButton::is_enabled() -> true`：文档解释了那个反转——
  这里 `onPressed` 为空**不会**禁用按钮，和普通 IconButton 相反。
- `CupertinoFormSection::clips_rows() -> false`：第 78 轮写的，有理由。

**把 76 当成缺陷数报出去，就是重犯上一次「81 条未解析」的错。**

### 真正的信号窄得多：**76 里 70 个有文档**

常量答案配一段解释，说明它被检查过。什么都没写的那 6 个才是没被看过的。
另外 6 个是更坏的一种：**收了参数却不看**——签名承诺了一个问题。

### 而那 6 个里第一个就是真错的

`NavigationBar::shows_label(&self, _index) -> true`，
注释写「Always——没有 shifting 模式可以藏它」。
**那个理由属于 Material 2 的 `BottomNavigationBarType.shifting`。
这是 Material 3 的 bar**，它有 `labelBehavior`，三个取值里
`alwaysHide` 藏掉全部、`onlyShowSelected` 只留选中的那个。

于是这个答案在三分之二的情况下是错的，
而**被忽略的那个参数就是线索**：`onlyShowSelected` 正是
「问的是哪个目的地」决定答案的那种情形。

枚举在 crate 里（`component_themes.rs`），主题也带着它，只有 bar 自己没读。
四个变异全红，其中 M1 就是原来那个 bug。

## 第 119-120 轮：空洞谓词队列清完，剩下的两个真错

### 收了参数却不看：6 个里 2 个是真错的

上一轮修掉 `shows_label` 之后还剩五个。四个是诚实的：

* `TransitionRoute::can_transition_to` 就是上游基类的默认值
  （`=> true`），而**三个收窄它的子类全都已经移植了**——
  Material 的 mixin、Cupertino 的、还有 `PageRoute` 的
  `nextRoute is PageRoute`（在这里叫 `can_transition_with_page_route`）。
* `license_count_is_valid` 是个 `assert(licenseCount >= 0)`，
  在 Dart 里是检查，在这里是类型。
* Cupertino 那两个（`overscroll_indicator`/`bounces`）上游本来就是无条件的。

第五个不是。

### `ScrollBehavior::should_notify` 收了 old 却不看

上游的基类也答 `false`——但它**担得起**，因为上游把这里合起来的东西拆开了：
`ScrollBehavior` 本身没有任何可配置状态，所以常量 `false` 是诚实的；
而 `copyWith` 返回一个私有的 `_WrappedScrollBehavior` 拿着那些旋钮，
**那个**是逐字段比较的。

这个移植把旋钮折进了同一个结构体，于是同时继承了
包装类的数据和基类的答案。后果：一个关掉滚动条的 `ScrollConfiguration`
什么也不通知，下面的滚动视图继续画滚动条，直到别的东西碰巧重建它们。

原有两个测试都是拿一个配置和它自己比——这恰好是无论 `shouldNotify`
怎么写都保持沉默的那一种。所以这个谓词有两个测试，两个都看不见它。

上游的中间那一项没有照抄，并且注释里写了为什么：
`behavior != oldWidget.behavior` 在 Dart 里是**同一性**比较
（两个类都没有重载 `==`），用来廉价地跳过「同一个对象又传了一遍」；
这里的相等是结构性的、覆盖同样那些字段，所以那个守卫永远改变不了结果。
**一行永远改变不了结果的代码，比它不存在更糟。**

### 最后一个：常量是对的，但它撑着一个没人测的不变量

`ItemWindow::is_empty` 恒为 `false`。这是真的——空的情况写作 `None`，
而不是一个盖住零个元素的 window。但这个事实哪儿也没写，
而它正是 `len` 能写成 `last + 1 - first`（usize）的前提。

两个构造函数都用 `last: last.max(first)` 守着，**两个 clamp 都没有测试**。
拆掉哪个都没人吭声。需要它们的是退化情形：
视口高度为零时 `last` 算作 `end / extent - 1`，落在 `first` 下面一格；
以及偏移滚过列表末尾时，走查一个元素也没找到，
`first` 回退到 `count - 1` 而 `last` 还停在 0——
`0 + 1 - 9` 在 usize 上是下溢，不只是读起来奇怪。

### 顺带：让 hollow.py 别再报 dispatch 的叶子

`is_rounded` 在四个圆角轨道形状上都是 `true`，没有第五个实现答 `false`，
因为那个 `false` 是上面枚举里的一条 match 分支。问题是真的，答案按类型分——
这是一门没有继承可以挂的语言里，「类型级事实」的样子。

第一版规则只按函数名分组，一下子吞掉十三条，**包括真正要看的那条**：
`is_empty` 在十几个不相干的类型上都有实现。改成按 `(文件, 名字)` 分组，
因为负责 dispatch 的枚举就住在它的叶子旁边。76 → 65，未解释的 6 → 3。

## 第 123-124 轮：尺子跟随别名，图像找回自己的尺寸

### `depth.py` 现在会跟着 `pub type` 走

`Text` 报 0/18——一个完整移植的类型被报成完全不存在。crate 里写的是
`pub type Text = RenderParagraph`。**别名是恒等**，所以把目标类型的成员算给它是精确的。
45 个别名里有 21 个点名了上游的类。`Text` 现在 40/18。

facade（单元结构体的 `new` 返回真正干活的 render object）**没有**照此办理，
而且是有意的：`Row` 和 `Column` 都构造 `RenderFlex`，
给它们各自记上全部成员会盖住「声明了属性却从不往下传」——
而这正是这把尺子存在的理由。

### 一次并发写入的教训，形状是新的

`git add -A` 在 order_sweep 跑的时候执行，把一个 12113 行的 `.sweep` 备份
和**一行正在被测试的错误变异**一起提交了（`ResolvedSnackBar::elevation`
的优先级被调换）。工作区本身是对的——sweep 后来恢复了文件——所以修正提交
提交的是「恢复」而不是新的修法，并用 `git diff aaa764e` 为空来验证。

前两次并发写入的教训都是「别改树」。这次我一个 `src/` 下的文件都没改。
**`git add -A` 不是读操作。**`*.sweep` 现在进了 `.gitignore`。

### `RawImage` 缺四个，先补上能补的那个

`RenderImage` 有 image/fit/alignment/opacity/size/centre_slice，
上游还声明了 `scale`、`color`、`colorBlendMode`、`filterQuality`，
ledger 里没有任何条目为它们开脱。

`scale` 补上了：它把像素数**除**成逻辑尺寸，@2x 的图因此和 1x 的图一样大。
方向弄反不是细微错误——scale=2 会让高密度图占四倍面积。

`color`/`colorBlendMode` **暂时补不了，而且我先前说错了**。我在上一条提交信息里
写「画布已经能表达染色，因为 `draw_image_rect` 收 `Paint` 而 `Paint` 有
`set_blend_mode`」。不对：paint 上的 blend mode 决定**图像与背景**如何合成，
不会给图像本身染色。染色是 `ColorFilter.mode(color, srcIn)`，
而 FFI 里没有 `rf_paint_set_color_filter`。
FFI 是我们自己的（`src/flutter/rust/ffi/`）且底下是 `DlPaint`，它有 `setColorFilter`——
所以这是一件要写 C++ 的活，不是被引擎挡住。

### 而找 scale 的测试时，撞见一个更大的洞

测试怎么写都过不了：布局出来是 0×0。原因是
`engine_test_stubs.rs` 里的 `rf_image_width`/`rf_image_height` **恒返回 0**，
把 `from_pixels` 收到的宽高丢掉了。

于是**整条图像几何路径都是不可测的**——`natural`、box fit、destination rect、
intrinsics 全都对着一个永远为零的尺寸测量，因此它们与**任何**实现都相容。
原有那个图像测试只检查 `centre_slice`，正是这个缘故。

stub 现在记住宽高。改完之后没有任何既有测试变红，这本身是条信息。
把 stub 改回返回 0 作为变异：三红。


## 第 151 轮 — SemanticsConfiguration：两条上游没有的规则，和一条没人调用的规则

`depth.py` 上 `SemanticsConfiguration` 32/104。上游那一百多个成员大半是
setter/getter 成对展开，类文档早就写明"字段集是本 crate 的，**merge rules**
才是值得移植的部分"。逐条对下来，问题恰好出在它自己声称要保留的那部分。

### `is_compatible_with` 里有两条上游没有的规则

上游只问六个槽：`platformViewId`、`maxValueLength`、`currentValueLength`、
`attributedValue`、`minValue`、`maxValue`——**每一个都是渲染对象在描述自己**，
两个声称者才真的是两个对象在互相矛盾。

port 另外加了 `hintOverrides` 和 `indexInParent` 两条，理由写成"一个节点只有
一个位置，两个就多了一个"。但这两样东西不是对象自己说的，是**祖先安排的**：
一个索引来自给列表编号的那次遍历，一个 hint override 来自外面的 `Semantics`。
一条合并链上出现两个，那是那次安排，不是分歧，所以上游让 `absorb` 按
first-wins 留下外面那个。

拒绝它的后果是**本该一个节点的地方裂成两个**，读屏用户听见一次多余的停顿。
两条都删掉。原来那条 `two_positions_in_one_parent_cannot_merge` 断言的正是
上游否认的事，改写成断言上游的实际行为（允许合并，外层取胜），
并给 `hintOverrides` 补了独立的一条——两条规则是一起删的，一个测试会让其中
一条悄悄回来。

### `AccessibilityFocusBlockType::merge` 写好了、测过了，但没有字段带着它

这是这个类里**唯一一条不是 first-wins 的规则**：最强者胜。
`blockSubtree > blockNode > none`，因为 blocking 说的是"读屏不能到达什么"，
而合并后的节点，凡是能到达任一半的路径都能到达它——所以子节点的承诺只有
取更强的那个才守得住。

crate 里这个枚举和它的 `merge` 早就在，还有穷举两两组合的测试，
但 `SemanticsConfiguration` 没有这个字段，于是这条规则**永远不可能被调用**。
补上字段，`absorb` 里接上。新测试里有一条专门打 first-wins 会答错的那个方向
（父弱子强）。

### 更大的那件事：这两个方法全 crate 没有生产调用者

`absorb` 和 `is_compatible_with` 只有测试碰过。走树用的是
`SemanticsAnnotation`，它做的事简单得多——文本节点让位给外层 label，就这些。

这正是那两条错规则能安然活下来的原因：**除了测试没有任何东西能发现它们**。
已经写进类型文档，因为这是"测试是唯一的支撑"，那是把测试看得更紧的理由，
不是放松的理由。

### 还没做的（都是规则，不是字段）

- `controlsNodes` 是**并集**，不是 first-wins
- `validationResult` 里 `invalid` 永远优先
- `hitTestBehavior` 的兼容检查是"**任一**边不是 defer 就拒绝"，不是"两边都设了"
- `role` / `inputType` 用 `none` 当未设，`headingLevel` 用 `0`，`identifier` 用 `''`
  ——上游一共有四种"未设"的拼法，port 只建模了 null 和 `''` 两种

变异三条，全红且各自只打中对应的一条：合并改成 first-wins（blocking 那条红）、
把 `index_in_parent` 规则放回去（位置那条红）、把 `hint_overrides` 规则放回去
（hint 那条红）。

5137 测试通过，完整 GN 门过。`depth.py` 上 32/104 → 33/104，并记进
`depth_examined.json`。

## 第 152 轮 — 平台一直在报的那个设置，没人能看见

先从上一轮的疑问出发：`coverage.py` 是不是看不见上游的 `enum`？**不是。**
它早就数枚举了（文件头写着这条曾经的盲区）。上游 171 个枚举里 15 个 port
从没提过，其中 12 个在 ledger 里（6 个 `*ServiceExtensions`、
`WebHtmlElementStrategy`、`KeyDataTransitMode`、
`UiKitViewGestureBlockingPolicy` 等），`KeyboardSide` / `ModifierKey` 属于
`raw_keyboard.dart`（整族 out_of_scope，上游 v3.18 起 `@Deprecated`），
`WindowPositionerAnchor` 在下划线私有文件里。尺子没问题，这条线到此为止。

### `MediaQueryData` 少一个字段，而平台早就在报它

`always_use_24_hour_format` 从 `flutter/settings` 通道进来，
`platform::UserSettings` 从那个通道写好的那天起就在存它。
`MediaQueryData::from_view` 就在旁边读 `text_scale_factor` 和
`platform_brightness`——**唯独不读它**。

后果是这一跳断了。`TimePickerDialog` 的类型文档自己把这件事写了下来：

> Upstream reads the 24-hour preference from `MediaQuery.alwaysUse24HourFormatOf`,
> which `MediaQueryData` does not carry, so it is an explicit setting here.

于是 `build` 里是 `unwrap_or(false)`——**一个编译进去的 12 小时制**。
`DefaultMaterialLocalizations::time_of_day_format(bool)` 和
`ResolvedTimePicker::hour_minute_size(bool, bool)` 同样把它当参数收，
而这三个参数的调用方**全是测试**：生产代码里没有一处能供给它，因为无处可取。
一个 24 小时制国家的用户会看到 AM / PM。

补上字段、aspect 和 `always_use_24_hour_format_of`，
对话框那行改成 `unwrap_or_else(|| always_use_24_hour_format_of(context))`——
和它上面一行 `self.orientation.unwrap_or_else(|| orientation_of(context))`
形状一致。显式设置保留为**覆盖**而非默认，并有测试盯着它仍然优先。

### `orientation` 是上游的派生 getter，port 里被各写了一遍

`pickers.rs` 里有个私有的 `orientation_of(context)`，从
`media_query_of(context).size` 现算。上游是 `MediaQueryData.orientation` 加
`MediaQuery.orientationOf`。搬到 `MediaQueryData` 上，顺带得到一件本来没有的
事：**它成了一个 aspect**。从 `size` 现算的话每次改变尺寸都是新闻；
按 aspect 问，只有跨过正方形那一次才是。

`presence.rs` 里那个同名函数留着——它问的是**布局约束**给的那个盒子，
是上游的 `OrientationBuilder`，不是同一个问题。

正方形归纵向，因为比较是严格的。这不只是个边界口味：一个被拖过正方形的窗口
会有那么一帧两个数相等，`>=` 会让答案在那一帧翻面。

五条变异全红且各自只打中对应的一条：`from_view` 写死 false、
`>` 改 `>=`、orientation aspect 改成比 `size`、对话框改回 `unwrap_or(false)`、
24 小时 aspect 改成恒 false。（`>=` 那条另外撞红三个 picker 布局测试——
它们依赖默认 `MediaQueryData` 的 0×0 被判为纵向，这与改动前的行为一致。）

5143 测试通过，完整 GN 门过（gallery 333 也过）。
`depth.py` 上 `MediaQuery` 20/65 → 22/65。

## 第 153 轮 — 改了系统字号，画面不动

顺着上一轮 `MediaQueryData` 的线往上走，问一个更基本的问题：
**设置变了之后，谁请求下一帧？**

答案是没有人。`rf_app_set_user_settings` 只调 `platform::set_user_settings`，
C++ 那边的 `RuntimeController::SetUserSettingsData` 也不排帧。而设置真正抵达
控件树是在**帧内**——`MediaQueryData::from_view` 加 `tree.publish(data)`，
`app.rs` 里那段注释自己点了名：

> Everything else about the view -- the keyboard arriving, the status bar
> changing height, **the reader turning the text size up** -- goes to the
> widgets that asked about it

传播路径通着，缺的是唤醒。所以一个读者在应用静止时把系统字号调大，
**要等到他碰一下屏幕才看得见**。上游是靠
`_MediaQueryFromViewState.didChangeTextScaleFactor` 里的 `setState` 走到这一步的，
而本 port 还没有那条链（`WidgetsBinding` 不是单例，平台回调够不着它）。

`set_user_settings` 改成返回"有没有变"，ABI 据此排一帧。只在真的变了时排：
壳层每次任一项变化都会重发整个设置对象，为一条什么都没说的消息渲染一帧，
正是那个既有的相等判断存在的理由。

### 顺带发现：一个 handler 应付所有变化

上游 `PlatformDispatcher._updateUserSettingsData` 是**逐字段**比较、逐字段分发：

- `onPlatformConfigurationChanged` —— 任何变化
- `onTextScaleFactorChanged` —— 只在文字缩放变了时
- `onPlatformBrightnessChanged` —— 只在明暗变了时
- `alwaysUse24HourFormat` 变化 **不触发任何专属回调**，它只计入"有变化"

port 只有一个 `on_settings_changed`，任何变化都叫它。后果是一个为了文字缩放
去重载字体表的监听者，会被主题变暗吵醒，而且**分辨不出发生的是哪一件**。

补上 `on_text_scale_factor_changed` 和 `on_platform_brightness_changed`，
按上游的行序分发：先总的，再文字缩放，再明暗。
24 小时制那条**故意**没有专属槽位——那是上游的形状而不是这里的遗漏，
所以它有一条自己的测试。

三条变异全红：三个 handler 都无条件叫（两条测试红）、把专属的排到总的前面
（顺序那条红）、没变也返回 true（帧那条红）。

ABI 那半行没有测试挡着，因为 `mod abi` 整个是 `#[cfg(not(test))]`——
crate 自己的测试二进制不链接 C++ 引擎。判定本身放在了
`platform::set_user_settings` 的返回值里，那是测得到的一半。

5147 测试通过，完整 GN 门过（gallery 333 也过）。

## 第 154 轮 — 模块文档描述了一份没有做的拷贝，防的是做不到的事

接着上一轮的链往上：`WidgetsBinding` 只在自己的测试里被构造过，整个类是有测试
的模型、没有生产实例。`flutter/navigation` 也一样——`system::on_route_message`
是个单槽，而 `WidgetsBinding::handle_pop_route`（"第一个认领的停住"、兜底退出、
返回手势名单）是一份平行的、够不着的实现。把两层接起来需要单例，那不是一轮的事。

这一轮做的是路上一件独立且必须先做的：**`remove_observer` 不存在**。

### 文档说的和代码做的不是一回事

模块头写着：

> Both loops walk a **copy** of the observer list, so an observer that removes
> itself while being notified does not corrupt the walk it is in.

两个循环都不是走拷贝，是按下标走。而且**没有 `remove_observer`**，
所以"an observer that removes itself"这件事在这个 port 里根本发生不了。
这段文档描述的是上游的保证，写得像是这里的。

上游确实要拷贝：它按对象辨认观察者，`removeObserver` 从列表里删掉，
删了之后后面全部前移。这里按 `add_observer` 发的**令牌**辨认，
所以正确答案不是拷贝，是**墓碑**——移除留下空槽，不合拢。

这不是上游保证的缩水版，是另一条保证，而且是这个 port 需要的那条：
`register_back_gesture_observer` 存的是令牌，一次会重新编号的移除
会**悄悄把返回手势指向别人**。

### `removeObserver` 的第一行最容易漏

上游：

```dart
bool removeObserver(WidgetsBindingObserver observer) {
  _backGestureObservers.remove(observer);
  return _observers.remove(observer);
}
```

先摘返回手势，再摘主列表。一个从主列表下来、却还留在手势列表上的观察者，
在它已经要求不再被驱动之后，仍然会被预测返回手势驱动。

`observers` 改成 `Vec<Option<Box<dyn ...>>>`，六个分发循环全部跳过空槽，
`asked` 只数真被问到的，`observer_count` 只数活着的，令牌不复用。

四条变异全红：移除改成合拢（令牌那条 + 手势那条红）、漏掉手势那一行
（手势那条红）、`observer_count` 改数槽位（令牌那条 + 二次移除那条红）、
`asked` 在跳过空槽之前就加一（兜底退出那条 + 令牌那条红）。

写测试时抓到自己一个真空断言：广播那条本来写的是"每一行都以 b: 开头"，
而一行都没有时它同样通过——**而一行都没有正是"广播跳过了所有人"的样子**。
改成逐行断言完整日志。同时给测试里的 `Spy` 补上 `did_change_metrics`、
`did_change_locales`、`handle_commit_back_gesture` 三个它本来没实现的钩子，
否则那两条测试测的是"什么都没记"。

5152 测试通过，完整 GN 门过。`depth.py` 上 `WidgetsBinding` 15/38 → 16/38。

## 第 155 轮 — 读者的语言，树里问不到

本来打算给 `WidgetsBinding` 加单例，把上一轮那条链接完。先按纪律找真实调用方，
结果发现管子的另一头也是空的：`Localizations::did_change_locales` 同样没有生产
驱动，`WidgetsApp` 那半只是一组断言，**框架里没有任何东西读 `platform::locale()`**。

给一条没有听众的通路加"请求一帧"，等于给不存在的人装闹钟。真正的缺口在更前面：

### `Localizations` 从来没被发布进树

上游的 `Localizations` 是 `InheritedWidget`，配 `Localizations.localeOf(context)`。
这里它是个普通结构体，有 `of(&self, resource_type)`——**没有进树的路**。
于是 `flutter/localization` 通道进来的语言到 `platform::locale()` 就停住了。

补上三个：

- `provide_localizations(bundle, child)` —— 上游 `Localizations` 的 widget 那一半
- `maybe_locale_of(context)` / `locale_of(context)` —— 上游的 `maybeLocaleOf` / `localeOf`
- `resource_of(context, type)` —— 上游的 `Localizations.of<T>`，返回 null 而不是断言

`locale_of` 在头上什么都没有时回落到 `platform::locale()`，那个本身还有一层
有文档的回落（平台什么都没说就当 `en`，因为没有 locale 的框架格式化不了日期）。
上游在这里是断言；这里根总会发布一个，所以回落只有"单独挂载一个 widget 的测试"
够得着——和 `media_query_of` 同样的安排、同样的理由。

根上按帧读 `platform::locale()` 发布，和 `MediaQueryData::from_view` 并排；
换语言走 `publish` 而不是重挂载，所以**只有问过的人被重建**。

`rf_app_set_locales` 现在会请求一帧了——而这一次是有听众的。

四条新测试加一条 `set_locales` 的返回值测试；三条变异全红：
回落改成空 locale、`maybe_locale_of` 恒 None、列表没变也返回 true。

### 沿途记下的

`localizations.rs` 里 `maybe_locale` 的文档原来写着"Upstream's `maybeLocaleOf`"，
但上游那个收 context、是查树的。改成说清楚：那是新加的 `maybe_locale_of`，
这个是它读的东西。

5157 测试通过，完整 GN 门过。`depth.py` 上 `Localizations` 3/8 → 6/8。

## 第 156 轮 — 一条画错颜色的描边，对所有测试都是隐形的

binding 那条链每次都通向同一件事：`MaterialApp` / `WidgetsApp` 在这个 port 里也是
校验模型而不是 widget，再往前就是造 widget 了。换队列，回到 `unpainted.py`——
`cupertino.rs` 上 8 处绘制调用没有任何观察者，是那张表上最大的一条。

### 先开盲点：`Drawn::Line` 不记颜色

`Rect`、`RRect`、`Oval`、`Circle`、`Path` 每一个都记 `argb`，只有 `Line` 不记。
后果是**一条描边画错了颜色，对所有测试都是隐形的**。

这不是假设。`cupertino.rs` 的清除标记（上游是
`CupertinoIcons.xmark_circle_fill`）是一个实心圆，叉用**字段背景色**抠出来的。
把它改成条目色，得到的是一个里面什么都看不见的实心圆，而没有任何东西会说话。

给 `Drawn::Line` 加上 `argb`。加完之后**没有一条既有测试变红**——
这本身是条信息：在此之前没有任何一条测试在看描边颜色。

### 三个手绘 glyph，此前一笔都没被检查过

`cupertino.rs` 里没有图标字体（模块文档说了原因），所以返回箭头、搜索放大镜、
清除标记都是手画的——屏幕上的东西**就是**这些 draw 调用。五条测试：

- **返回箭头是两笔，且两笔要相接**。停在半路的两笔读起来是折断的箭头而不是尖角。
- **RTL 下它是关于盒子中线的反射**，不是"另一组恰好朝反方向的数字"。
  差别会显示成一个偏心的箭头。`(11,3)` ↔ `(1,9)`，盒宽 12，两两相加都等于 12。
- **放大镜的手柄在过圆心的对角线上**，且从镜缘向外。否则就是一个圆旁边支了根棍。
  手柄起点 11.6 对半径 5 的圆，实际落在 5.09 处——在 1.6 的描边宽度内相接，
  测试写了这个容差。
- **清除标记的叉是用背景色抠出来的**，两臂关于圆心对称、方向相反。
  这条正是上面那个盲点挡着的。
- **每个 glyph 都画在它被放置的位置**。忽略 offset 的 glyph 会画到屏幕角上，
  而那种错误能活过所有"只数调用次数"的测试。

五条变异全红且各自只打中对应的一条：镜像改成非反射、叉改用条目色、
手柄离开对角线、`x`/`y` 闭包丢掉 offset、第二笔起点上移一格。

`unpainted.py`：19 → 11 处无人观察的绘制调用，13 个文件里有观察者的从 6 增至 7。

5162 测试通过，完整 GN 门过。

## 第 157 轮 — 画弧的那个 stub 函数体是空的

上一轮末尾留的线索：`controls.rs` 有 `draw_arc` 和 `draw_oval` 两处，
而 `Drawn` 里没有 `Arc` 变体。查下去比预想的更彻底——
`rf_canvas_draw_arc` 的**函数体整个是空的**，什么都不记。
弧线对测试根本不存在。

加上 `Drawn::Arc { left, top, right, bottom, start_degrees, sweep_degrees,
use_center, argb }`。加完之后没有一条既有测试变红——和上一轮的描边颜色一样，
这本身是条信息。

`translated` 里角度**不跟着平移**：角度不是位置。

### 圆形进度条，七条此前不存在的断言

`ArcSpinner` 是一个椭圆加一条弧，它要说的话全在角度里：

- **起点是十二点（-90°）而不是三点（0°）**。画布上的零是三点钟方向，
  从那里起步的 spinner 看上去没动就已经转过四分之一圈，
  而唯一说明这件事的就是那个数字。
- **扫过角是 `360 × value`**，0.25 就是九十度。
- **`value == 0` 时一笔弧都不画**，而不是画一条长度为零的弧——
  弧有圆头笔帽，长度为零画出来是十二点钟方向的一个点，
  等于告诉用户"已经开始了"。
- **弧和轨道共用同一个 bounds**。两者笔宽相同，共用 bounds 才是同一个圆环；
  分开就成了轨道旁边另开一道槽。
- **bounds 内缩半个笔宽**（不是一个笔宽）。描边以路径为中心，
  画在盒子边上会被裁掉外面那一半。
- **`use_center` 为假**：为真是把两端穿过圆心连起来，那是个填充扇形，
  和圆环段长得完全不一样，而这个标志只差一个词。
- 平移时盒子跟着走，角度不动。

六条变异全红且各自只打中对应的一条：起点改 0°、`use_center` 改真、
`sweep > 0.0` 改 `>= 0.0`、内缩改成整个笔宽、给弧单开一道槽、
扫过角改成 `180 × value`。

`unpainted.py`：11 → 9 处无人观察的绘制调用，13 个文件里有观察者的从 7 增至 8。

5169 测试通过，完整 GN 门过。

## 第 158 轮 — 24 小时表盘上，被选中的那个小时从来没有被高亮过

`rf_canvas_draw_paragraph` 和上一轮的 `draw_arc` 一样，函数体是空的。
文字从来没被记录过，代价不只是"查不了写了什么"——**是任何"这个要画在字的下面"
的规则都问不出口**，而 `editable.rs` 里有两条测试在自己的注释里写着这件事，
一边断言着它们够得着的那个较弱的说法。

加上 `Drawn::Paragraph { text, x, y }`。`Drawn` 因此从 `Copy` 降为 `Clone`
（`String` 不是 `Copy`），`render.rs` 里五处 `.copied()` 跟着改。
加完之后照例没有一条既有测试变红。

### 打开之后，三件事

**1. 一条测试的名字是假的。**
`an_empty_field_showing_a_hint_still_draws_its_caret` 从来没设过占位文字——
`field` 辅助函数不调 `with_placeholder`。它建的是一个空空如也的字段，
而在 paragraph 不被记录之前，没有任何东西能说破。现在它真的设一个，
并断言插入符画在占位文字**之上**：藏在灰色提示后面的插入符，
会让一个正握着键盘的字段看起来没有焦点。

**2. 一条测试终于能问它本来的问题。**
`the_highlight_is_the_first_mark_of_the_frame` 的注释写着"that half cannot be
checked here, because paragraphs are not recorded and there is no glyph in the
list to be before"。现在有了。改名为
`the_highlight_goes_down_before_the_glyphs_it_highlights`，
断言真正的规则：填充矩形画在字之后会盖住它本要高亮的字。

**3. 一个真缺陷。**

`TimeDial::paint_labels` 里 `enumerate()` 在 `filter` **之前**，
所以 `index` 是标签在 `self.labels` 里的下标；而调用方传的 `only_index` 是
**它在那一圈里的位置**（从角度和 `ring_len` 算出来的，0..len-1）。

外圈从 0 开始，两种编号恰好相同，所以一直看不出来。
**内圈是 labels 12..23，`only_index` 的 0..11 一个都匹配不上**——
24 小时表盘上，被选中的小时永远不会用选择器的颜色重绘，
它就以普通标签色躺在选择器圆点下面。而这一切没有任何测试能看见，
因为 paragraph 不被记录。

`enumerate()` 移到 `filter` 之后。

### 表盘标签的七条断言

12 小时圈是 12 个数字、从 12 开始；第一个在正上方且顺时针推进
（起点错会整体旋转，符号错会镜像，两种坏法不同）；分钟圈每五分钟一个；
24 小时是双圈且内圈确实更靠内；被选中的那个数字最后再画一遍、落在原位。

居中那条我第一版写错了：记录的是段落的**左上角**，所以对置两个标签的 y
中点并不落在圆心上——它整体上移半个行高，而这**正是**判别式：
挂在角上画的话中点会正好落在圆心。改成断言"每一对对置标签的中点一致，
且在圆心上方"。

四条变异全红：把 `enumerate` 挪回 `filter` 之前（24 小时那两条红）、
标签挂角画、圈子逆时针、以及在高亮之前先画一遍字。

5176 测试通过，完整 GN 门过。

## 第 159 轮 — BoxDecoration 的三笔，之前一笔都看不见

`unpainted.py` 上剩下最大的一条：`decoration.rs` 三处 `draw_path`。
路径在这个 stub 里只留包围盒和颜色（这是当初刻意的：记下"画过一条路径"
而不记它的形状，会招来一条证明得比它看起来少的测试）。
但包围盒加颜色，恰好够问 `BoxDecoration::paint` 会出错的那几件事。

六条断言：

- **阴影在填充下面，填充在边框下面**（上游 `_BoxDecorationPainter.paint`
  的次序）。两种交换坏法不同：阴影盖在填充上是一道糊痕，
  边框压在填充下则是根本看不见。
- **阴影是本体按 offset 平移过去的**——这正是"光从某处来"和"一圈光晕"的区别。
- **再按 spread 四边等量外扩**（是 inflate 不是 scale，
  所以细高的盒子不会得到一个更细更高的影子）。
- **列表里每一个阴影都画，后面的压在前面上**。
  只画第一个就丢掉了 Material 每一级 elevation 都由"环境光 + 主光"两道组成的那对。
- **圆形时按最短边取正圆并居中**，而不是撑满盒子的椭圆。
- **什么都没有的 decoration 一笔不画**——画一个透明矩形代替的话，
  树里每个空容器都要花掉一次绘制调用。

五条变异全红：阴影忽略 offset、spread 只往两边长、只画第一个阴影、
圆形按最长边取半径、边框挪到填充之前。

`unpainted.py`：9 → 6 处无人观察的绘制调用，13 个文件里有观察者的从 8 增至 9。

5182 测试通过，完整 GN 门过。

## 第 160 轮 — 解码器把每张图都报成 0×0，于是整条 provider 路径不可测

本来只是要覆盖 `painting.rs` 剩下的两处绘制调用，结果撞上一个比它们大的洞：
`rf_image_decode` 恒返回 0×0。stub 自己的注释承认了这件事，
但把它说成"这一个句柄还是零"——**实际上是整条 `ImageProvider` 路径都不可测**。
任何"解析一个 provider 再把结果画出来"的代码，都在对着一个永远为零的尺寸做
`apply_box_fit`，因而与自己的任何实现都相容。

给 stub 加一种**明确标记为夹具**的编码：以 `RFIM` 开头的字节后面跟两个
小端 `u16` 宽高，`encoded_image(w, h)` 生成。其余一律照旧返回 0×0——
所以任何递进真 PNG 的既有测试，答案一个字都没变。

### `DecorationImage::paint` 的六条断言

- **source 是整张图、以它自己的像素为单位**。这正是这个 recorder 当初要抓的
  单位混用：按逻辑像素取的话，比盒子宽的图会被裁而不是被缩，
  而只看 destination 的测试全都会通过。
- **没给 fit 时是 `scaleDown` 而不是 `fill`**（上游对 `null` 的文档规定）。
  换成 fill，每个图标都会被拉成盒子的形状。
- **而 `scaleDown` 对过大的图确实会缩**——只试小图的测试分不出这两者。
- **按 alignment 的点居中**，默认是盒子中心；换个 alignment 就得动。
- **加载失败时一笔不画且返回 false**——上层用返回值决定要不要画占位图，
  所以"没画"和"返回 false"必须是同一件事（`painted` 每次调用都断言这一点）。
- **但解码成 0×0 的图仍然是一张图**：未识别的载荷解码为零尺寸而不是失败，
  所以照画不误。单独写出来，因为拿这个用例去断言"没图就不画"是在对错误的
  路径断言错误的事。

### 自己的两处

第一版把 `ImageRect` 的两个矩形读成了 `(x, y, w, h)`，实际是
`(left, top, right, bottom)`，三条测试带着**对的数字在错的槽位**上红了——
这是两种发现方式里更有用的那种。已在模块文档里写明。

`a_provider_with_no_picture_draws_nothing_and_says_so` 名不副实：
它测的是未识别载荷，而那条路径照样会画。拆成上面那两条。

五条变异全红：source 按逻辑像素取、缺省 fit 改成 fill、
按 alignment 点挂角而非居中、加载失败也画一个空矩形、夹具解码器忘掉高度。

`unpainted.py`：6 → 4 处无人观察的绘制调用，13 个文件里有观察者的从 9 增至 10。

5188 测试通过，完整 GN 门过。

## 第 161 轮 — unpainted 归零

清掉队列最后四处，`unpainted.py` 从 19（第 156 轮开始时）到 **0**：
13 个有绘制调用的文件，现在全都有一条读回画布被告知了什么的测试。

### 第五个空 stub

`rf_canvas_draw_color` 的函数体也是空的（这一轮里的第五个：
`draw_line` 不记颜色、`draw_arc`、`draw_paragraph`、`rf_image_decode`
恒返回 0×0，再加这个）。加上 `Drawn::Color { argb }`。

### 错误框与溢出条纹

两者都只在事情已经出错时才被看见——这恰恰是没人盯着它们的原因：
一个什么都不画的错误框，看起来只像一个悄悄失败的应用；
而溢出条纹是唯一告诉开发者"这一行太宽了"的东西。

- 错误框**先铺底再写字**（顺序反了底色会盖住字）、
  铺满自己的尺寸、写的是**给它的那句话**
  （在 paragraph 不被记录之前这不是废话：一个画自己类名或者什么都不画的
  错误框，从外面看是一样的）、没有话说时仍然画框
  （无消息的失败仍然是失败，两样都不画等于说应用没事）、按 offset 落位。
- 溢出条纹在孩子装得下时**一条都不画**（这是运行正常的应用的每一帧，
  给合身的孩子打标记会让整个界面糊上黄色）、按越界的**那条边**打标记
  （右边溢出和下边溢出是不同的 bug，两边都标等于都没标）、
  两个方向都超就标两处、并且加上 offset
  （漏掉的话，滚动页面里的每次溢出都会标在屏幕顶上而不是控件上）。

### 屏障与帧背景

- 有颜色的 scrim **铺满它被给的尺寸**；没颜色的**一笔不画**——
  画一个透明矩形眼睛看不出区别，却要在这类路由的每一帧上花掉一次绘制调用。
- 帧背景**在应用作画之前落下**（这是 `draw_color` 唯一的一条规则，
  而在此之前它不是任何测试能提出的主张）；且**无论 dpr 是多少都只填一次**——
  dpr 恰为 1 时会跳过 transform 图层（"pushing an identity would cost a layer
  to say nothing"），背景不能跟着一起被跳过。

六条变异全红且各自只打中对应的一条：错误框先写字后铺底、铺底忽略 offset、
条纹忽略 offset、无色 scrim 改为透明填充、背景挪到应用之后、
背景只在缩放时填。

5201 测试通过，完整 GN 门过。

## 第 162 轮 — 两个没人问的公式，和一个读了邻居主题也没人发现的按钮

`unpainted` 清空后转向另外两条队列。

### hollow：两个常量谓词，说明写在了别处

`hollow.py` 找的是"看起来像检查、实际是陈述事实"的谓词，
而它要求那段事实写在**函数自己的**文档注释上。两个漏网的：

- `CupertinoIcons::requires_a_package_dependency` —— 说明在它上面那个常量的
  注释里。Material 的图标随框架发布，这些不是，本 crate 怎么摆布都改不了。
- `FabMiniOffsetAdjustment::is_mini` —— 说明在 struct 上。
  上游那个 mixin 的全部内容就是 `isMini() => true`，它存在的意义是覆盖
  `StandardFabLocation::is_mini` 的 `false`，**两个答案相反就是这个类型的全部内容**。

查下来还有一件更实的：这个 struct 的两个成员**都没有任何读者**。
真正的机制是 location 上的 `mini` 标志加模块级常量。而文件自己的模块文档说
"each rule is a unit struct holding its formula"——五个兄弟规则结构都被
`get_offset_x` / `offset_y` 读着，只有这一个的公式没人问。
按文件自己声明的设计接上（`get_offset` 现在走
`FabMiniOffsetAdjustment::ADJUSTMENT`），并补一条测试钉住"它翻转默认值而不是
重复默认值"。

hollow 的未说明项：2 → 0。

### variant_sweep：component_themes.rs 13 处臂，3 处存活

- **`ButtonVariant::Outlined` 可以去读 `ElevatedButtonTheme`（上一条臂），
  整个测试套件全绿。** 原因是每一条走到 `ResolvedButton::of` 的测试都只往树里
  放一个按钮主题。四个一起放进去，谁读了谁就写在答案里了。
  一个给自己 outlined 按钮设了主题却什么都没变的应用，没有别的症状。
  顺带给 `Danger` 读 filled 主题这件事单写了一条——共用一条臂是决定而不是遗漏，
  所以它需要一条自己的测试，而不是被留着看起来像遗漏。
- **`TextCapitalization::as_name` 四行里有两行可以换成邻居。**
  这正是 sweep 文档点名的第一类：发往 `flutter/textinput` 的表，
  这边没人读，读它的是 embedder，所以错一行会换掉读者拿到的键盘而这里毫无反应。
  补 `ALL` 和两条测试：逐行断言（上游发的是 Dart 枚举的 `toString()`，
  形如 `TextCapitalization.words`），以及两两互不相同——
  后者才是"换成邻居"这件事可被察觉的前提。

重跑 sweep：13 处臂全部被抓住，**0 存活**。
`unwalked.py`：268 → 264 个从未被点名的变体。

5205 测试通过，完整 GN 门过。

## 第 163 轮 — 在 RTL 语言里，每个刻度都染在错的一侧

`variant_sweep` 扫 `slider_theme.rs`：8 处臂，2 处存活。

### 刻度的 RTL 那一支没有任何人在看

`RoundSliderTickMarkShape::paint` 判断"这个刻度在拇指的哪一侧"：

```rust
let inactive = match direction {
    TextDirection::Ltr => offset > 0.0,
    TextDirection::Rtl => offset < 0.0,
};
```

RTL 那一行可以改成和 LTR 一样，**整个测试套件全绿**。
后果是在 RTL 语言里，每一个滑块上的每一个刻度都染在错的一侧——
而任何 LTR 的测试都看不见这件事。

补的测试写成**上面那条的镜像**而不是又两个数字：同一个几何，
两个方向必须给出不同的答案，否则其中一个没被读。
另加边界那条：offset 恰为零时两边都判为 active（两个比较都是严格的），
这是拇指压着的那个刻度在越过时不会闪的原因。

### `Round` 刻度的尺寸可以变成零

`SliderTickMarkShape::preferred_size` 的 `Round` 臂可以答成 `Size::ZERO`
（也就是 `Empty` 的答案），全绿。而 `paint` 里有 `if radius > 0.0` 的守卫——
所以那等于**一个刻度都不画**。一个悄悄不再显示分度的滑块，
看起来就像一个本来就没有分度的滑块。

顺带钉住上游的默认：半径没给时是轨道高度的四分之一（这就是那个字段是
`Option<f32>` 而不是一个带默认值的数的原因——轨道更高的滑块自动得到更大的分度）。

重跑 sweep：8 处臂全部被抓住，**0 存活**。
`unwalked.py`：264 → 262。

5211 测试通过，完整 GN 门过。

## 第 164 轮 — 两张只有引擎读的号码表，和一个转错轴的滚轮

`variant_sweep` 扫 `painting.rs`：12 处臂，**7 处存活**，是目前单文件最多的一次。
其中六处是同一件事的两半。

### `TileMode::code` 和 `ClipBehavior::code`

两张发往 C++ 的号码表。除第一行外，**每一行都能拿走邻居的号码而整套测试全绿**。
原因是结构性的而不是疏忽：这边没人读这些数字，
读它们的是 `rustflutter_ffi_draw.cc` 里的 `ToTileMode` 和 `ToClipBehavior`，
两边是一个 ABI 的两份手写镜像。号码错了，渐变会以错误的方式平铺、
裁剪会用错抗锯齿策略，而这边毫无反应。

补 `ALL` 和三条测试：逐行断言（注释里点名了 C++ 那一半的文件和它的 `switch`），
以及两表各自两两互不相同——后者才是"换成邻居"可被察觉的前提。

### 竖直滚轮绕的是平轴

`matrix_utils::create_cylindrical_projection_transform` 是
`ListWheelScrollView` 那个把平面卷成圆柱的矩阵。上游：
`horizontal => rotationY`、`vertical => rotationX`。
**`Vertical` 那一支可以改成绕 y 轴而全绿**——一个上下滚动的滚轮会变成横着转，
而这个矩阵既有的每一条测试用的都是默认朝向。

判别式第一版我选错了：模型先把每个点平移到 `z = radius` 再转，
所以两种旋转都会改变点的 x，测试红了。真正分开它们的是**哪个坐标保持为零**——
绕竖轴转不会把地平线上的点抬起来，绕平轴转不会把中线上的点推出去。
已写进注释。

重跑 sweep：12 处臂全部被抓住，**0 存活**。

`unwalked.py` 仍是 262：那些变体名出现在 `ALL` 常量里而不是测试模块里，
而 `unwalked` 问的是名字形状的问题。这正是 `variant_sweep` 存在的理由——
行为形状的那个答案是 0，它比名字那个强。

5215 测试通过，完整 GN 门过。

## 第 165 轮 — 一次写文件失败会报成编码失败

三个文件扫下来，存活的 12 处里有 8 处属于同一类：**只有引擎或 embedder 读的表**。
这一轮直接朝这一类去，扫 `engine.rs`：7 处臂，4 处存活，又是两组。

### `TextAlign::code`

发往 `MakeParagraphStyle`（`rustflutter_ffi.cc`）的号码表。
`Right` 和 `Center` 两行能拿走邻居的号码而全绿。
一段要求右对齐的文字会被居中，而这边没有任何东西读这些数字。

注意那个 `switch` 的默认臂是 `left`——所以 `Left` 的号码是**这一侧选的**，
不是那一侧点名的。测试注释里写明了这点。

### `RenderError` 的两条消息可以互换

`WriteFailed`（"could not open the output file"）和
`EncodeFailed`（"PNG encoding failed"）能拿走对方的文本而全绿。
一次磁盘写不进去的无头渲染，会报告成编码故障——
而这段文本是调用方唯一能拿到的东西。

补的测试分两层：**两两不同**（读起来一样的两条消息就是一个诊断），
以及**每条都点出是哪一步失败的**
（rasterize / snapshot / open / encoding / path / presenter——
这些词才是把 `rf_layer_tree_write_png` 的三种失败分开的东西）。

顺带把 `from_code` 那张表也钉住（C++ 那侧：-1 无路径、-2 光栅化、-3 快照、
-4 打开或写入、-5 编码，-100 来自窗口呈现器而不是绘制侧），
并断言未认领的码**带着数字**原样传出去而不是被猜成最近的已知错误。

写的时候抓到自己一处重言式：`assert_eq!(RenderError::ALL.map(|_| ()).len(), 6)`
——`ALL` 的长度按构造就是 6。换成断言 `ALL` 与带码表逐项相等，
这样往枚举里加第七个而不给它码，会编译不过而不是悄悄变成 `Unknown`。

重跑 sweep：7 处臂全部被抓住，**0 存活**。
`unwalked.py`：262 → 255。

5221 测试通过，完整 GN 门过。

## 第 166 轮 — 一个本可被拒绝的退出，会被当成不容拒绝的

`variant_sweep` 扫 `services/system.rs`：**56 处臂，3 处存活**。
（这个文件的鼠标指针表十八行、设备朝向四行、系统 UI 模式五行——
全部被抓住，因为它们在第 130 轮左右已经被同样的方式扫过一遍了。）

三处存活又都是通道字符串。

### `SystemSoundType::as_argument`

`Alert` 和 `Tick` 能拿走邻居的字符串。要求发出提示音的地方会发出点击声，
而这边没有任何东西读这些字符串——读它的是 embedder。

### `AppExitType::as_message`：这一条不是外观差异

`Cancelable` 能拿走 `Required` 的字符串而全绿。后果是
**一个应用想要有机会拒绝的退出，会被当成它无法拒绝的那种发出去**，
embedder 不会再问就把窗口关掉——读者没有被问过要不要保存。

两个方向都补上了，而它们**故意不对称**，方向也相反：

- `from_message`：不是 `"cancelable"` 的一律当 `Required`。
  这是 Windows embedder 里 `StringToAppExitType` 的做法。
  反过来错的话，一台正在关机的机器会卡在一个自以为可以拒绝的应用上。
- `AppExitResponse::from_message`：认不出来就是 `None`，**不猜**。
  这里猜 `exit` 会把一个 embedder 回了本 crate 没听过的话的应用直接关掉；
  拿 `None` 怎么办由调用方决定。

两条测试各自把这个方向写清楚，因为它们看起来像同一段代码的两半，
而实际上是两个相反的决定。

重跑 sweep：56 处臂全部被抓住，**0 存活**。

5227 测试通过，完整 GN 门过。

## 第 167 轮 — 声明顺序就是协议，而 sweep 看不见它

沿着"只有引擎/embedder 读的表"这条线扫剩下两个文件，
`variant_sweep` 说：**两个文件都没有单行 match 臂**。

它说的是实话，而这正是问题所在。

### `PlatformProvidedMenuItemType`：没有一条臂，却是一张最脆的表

`serialize_node` 送出去的是 `provided.menu_type as i32`——**判别式**。
上游发的是 `item.type.index`，而 Dart 枚举的 index 就是它被写下的位置。
所以这十二个变体的**声明顺序是协议，不是一个列表**：
往中间插一个，后面十一项全部改号，embedder 会在应用要求 Quit 的地方
画出一个服务子菜单。

这张表没有任何逐变体的东西可以被变异——没有字符串、没有方法——
所以 `variant_sweep` 结构上看不见它。**找到它的是 `unwalked.py`**，
那个名字形状的、一般来说更差的问题，在这里恰好是对的那个。

补 `ALL` 和三条测试：逐位断言（注释里抄了上游的顺序）、
把 Quit 和 ServicesSubmenu 这对挨着的单独点名（差一位的代价就是它们）、
以及两两号码不同（给某一个加显式判别式会静默撞车）。

手工变异：往枚举中间插一个 `Preferences`，两条测试红。

这个盲区写进了 `variant_sweep.py` 自己的文档：它只重写单行臂，
所以拼成 `enum as i32` 的表没有臂可看。两条队列要并排留着，
而不是用行为形状的那条取代名字形状的那条。

### `ContextMenuButtonType` 是个标签

`icon_data.rs` 里这个枚举在本 port 没有逐变体的行为——上游用它在
`AdaptiveTextSelectionToolbar` 里按平台选标签，而那一层还没有。
不是遗漏，是还没到；没有为它发明一个消费者。

`unwalked.py`：255 → 254。5230 测试通过，完整 GN 门过。

## 第 168 轮 — 二十九个混合模式，两把尺子都看不见

上一轮的形状——**判别式直接当线上格式**——是可以系统搜的：
在 FFI 调用处对枚举做 `as c_int` 的地方。搜出来五张表，
一张都没有 match 臂，所以 `variant_sweep` 全部够不着。

- `StrokeCap`（1 round、2 square、默认 butt）
- `StrokeJoin`（1 round、2 bevel、默认 miter）
- `FillType`（1 odd、否则 non-zero——自交路径的内部归属，
  一颗五角星在一种规则下是实心的、另一种下中间是空的）
- `ClipOp`（1 difference、否则 intersect——弄反了会显示出整个屏幕，
  唯独不显示本该可见的那块）
- `BlendMode`，直接 `static_cast` 到 `flutter::DlBlendMode`

每一条都对着读它的那段 C++ 写了断言，并点名文件和 switch。

### 而 `BlendMode` 少了四个

文档写着"判别式与 `DlBlendMode` 一致，后者又与 `dart:ui` 的 `BlendMode` 一致"。
判别式确实一致——**少的是尾巴**：port 停在 `Multiply = 24`，
而上游有 29 个。`Multiply` 是上游标注的"最后一个可分离模式"，
后面四个（`Hue`、`Saturation`、`Color`、`Luminosity`）拿整个颜色而不是逐通道，
所以它们排在最后，也所以一个 port 会不经意地停在那里。补齐了。

`rf_paint_set_blend_mode` 的守卫会把超出 `kLastMode` 的号码**静默丢弃**，
所以这边多发明一个模式是一次无操作而不是一次崩溃——这也写进了文档。

### 两把尺子的盲区，各修一处

- `variant_sweep` 只重写单行 match 臂，这五张表一条臂都没有。
  文档里已经记了（上一轮）。
- **`unwalked.py` 的变体正则要求名字后面跟 `,`、`{` 或 `(`**，
  所以 `Clear = 0,` 不匹配——**每一个带显式判别式的枚举对它都是隐形的**。
  而那恰恰是"数字就是线上格式"的那一类，风险最高。
  正则修好后总数 1428 → 1457（正好是 `BlendMode` 的 29 个），
  未点名数仍是 244，因为这一轮的测试把那 29 个全点了。

三条手工变异全红：交换 `StrokeCap` 的前两个、交换 `ClipOp` 的两个、
把 `Hue` 和 `Saturation` 的号码对调。

5236 测试通过，完整 GN 门过。

## 第 169 轮 — 画布的存还栈，此前一次都没被看见

继续照形状搜。最富矿的那个形状是**空函数体的 stub**——这一串里已抓到五个。
系统查一遍：还有 **29 个**。其中最要紧的一组是画布状态栈：
`save`、`save_layer`、`restore`、`restore_to_count`、`translate`、`clip_rect`
全部什么都不记。

一个存了不还的 `paint`，会把它设的裁剪和变换留给**之后画的每一个兄弟**。
症状是屏幕上别处的一个控件画错位置或干脆消失，而原因在一个没人看的文件里。
这件事在本 crate 里此前完全不可观察。

加上六个变体和一个 `save_depth` 辅助（返回结束时的深度，中途转负则 `None`——
还多了一次和留了一个没还，是两种不同的坏法）。

### 写测试时被纠正了两次

**第一次**：我拿 `RenderClipRect` 加宽松约束去测裁剪，结果一个调用都没有。
不是测试写错了机制——**是规则**：孩子装得下时，save、clip、restore 三个调用
全部跳过。这是每一帧、每一个裁剪上的事，写成了一条测试。

**第二次，更值得记**：我给 fade 那条写的说法是"两存两还"，
然后按纪律去变异——删掉内层那个 `canvas.restore()`——**套件全绿**。

查下来 `Canvas::saved` 是这样的：

```rust
let count = self.save_count();
self.save();
let result = body(self);
self.restore_to_count(count);
```

**按记下的深度整体弹回**，不是逐个还。所以闭包里漏一个 `restore()` 是被兜住的，
那一句是冗余的——我的前提本身就是错的。改成断言真正成立的事：
层被打开了、整体弹回把它关上了。并单加一条直接问那个性质的测试：
一个体内存三次不还的闭包，出来时画布仍在原处。

换成会咬的变异（把 `saved` 的 `restore_to_count` 改成 `restore()`）：三条红。
另外两条变异——裁剪按原点取而不是按 offset、没溢出也裁剪——各红一条。

`push_clip_rect` 用的也是整体弹回，理由相同：包住一个**它无权信任其 paint 的
孩子**时，逐个还会被孩子少还一次带偏，整体弹回不会。

5244 测试通过，完整 GN 门过。

## 第 170 轮 — 描边和填充，此前没有任何测试分得出

`StubPaint` 从头到尾只带一个 `argb`。所以**一条边框被画成填充的矩形、
或者两点宽的轮廓被画成八点宽，都会通过每一条检查它颜色和外框的测试**——
而那是大多数测试。

给 `StubPaint` 加 `stroke: Option<f32>`（`None` 是填充，`Some(w)` 是那个宽度的
描边），七个形状变体各加一个字段，二十一处字面构造跟着改。

### 加完之后，编译器和红测试把两件事教了出来

**一、边框的两套机制画的不是一回事。**

- `borders::paint_border`：每条边是一个**填充的四边形**（外沿到内沿四个角），
  这是上游 `paintBorder` 的形状。我一开始把期望值写成描边，三条测试红了——
  是我错，不是代码错。
- 而**零宽度的边是一条宽度为零的描边**（上游的发丝线，设备能画的最细的线）。
  这个 `if side.width == 0.0` 分支此前完全不可见。
  两者的外框一模一样（`bottom == top`），**只有 paint 能把
  "最细的一条线"和"什么都没有"分开**——这句话单写了一条断言。
- `RenderDecoratedBox::with_border` 走的是另一套：**描边**，内缩半个宽度。
  内缩和宽度是同一件事的两半，而此前只有内缩那一半能被检查。

**二、`a_fill_and_a_border_are_two_rectangles_in_that_order` 这条测试的名字，
现在才真的成立。** 先画的那个是填充、后画的那个是描边——
这正是次序存在的理由：描边先画，会被填充盖掉一半。

三条变异全红：让所有边都填充（发丝线那条红）、让所有边都按自身宽度描边
（三条带的红）、把 `RenderDecoratedBox` 的描边去掉（三条红）。

`engine_test_stubs.rs` 里还剩 26 个空函数体的 stub。

5244 测试通过，完整 GN 门过。

## 第 171 轮 — 叠在一起的一圈刻度，和转成一圈的一圈刻度，此前长得一模一样

继续清空 stub。`rf_canvas_scale` 和 `rf_canvas_rotate` 都是空的。

`cupertino.rs` 的活动指示器（那个 iOS 转圈）是**同一个刻度形状画八次，
每次之间转 `360/8` 度**。转错角度、或者根本不转，画出来的调用序列
在此前的记录器眼里完全一样——把它们分开的唯一东西是一个没人看得见的画布调用。

六条测试：一圈是"一个 alpha 一个刻度"、之间的旋转加起来正好一整圈
（否则要么挤成一段弧、要么绕过头）；`progress` 只揭示它该揭示的那些；
还没开始时一个都不画；平移先走，所以转的是**指示器的中心**而不是屏幕角落
（在那儿一个半径十的环只能看见四分之一）；以及那八个 alpha。

### 又被纠正了两次

**一、我按对上游的印象写了"最亮的是第一个"，红了。**
`TICK_ALPHAS` 是 `[47, 47, 47, 47, 72, 97, 122, 147]`——**最亮的是最后一个**，
领头的那条臂在列表末尾。顺带补了一条"从不先亮后暗"：尾部是平的四条同值，
这是它读起来像转圈而不是像一颗拖着长尾的彗星的原因。

**二、我在注释里写了"十二"**，而数量是 `TICK_ALPHAS.len()`，是八。
改成不写数字——上游的 `_kAlphaValues` 才决定这个转圈有几条臂，
测试里写死一个数就是在断言它自己的算术。

四条变异全红：步长减半、根本不转、平移到角落、以及不看 `progress` 全画出来。

`engine_test_stubs.rs` 里的空 stub：29 → **20**。

5250 测试通过，完整 GN 门过。

## 第 172 轮 — 追着透明度问了三处，三处都不在画布上

`rf_paint_set_opacity` 是空的。给 `StubPaint` 加上它，
并挂到 `Drawn::SaveLayer` 上（不是挂到每个形状上——这个 crate 里带
opacity 的 paint 只有交给 `save_layer` 的那些）。

然后去找它的读者，连着找错了三次，而每一次的答案都是同一件事。

- **`RenderOpacity`** 用 `push_opacity`——合成器**图层**，画布状态栈毫不知情。
- **`RenderClipRect`** 的裁剪也是图层（第 169 轮已记）。
- **`RenderListWheelViewport`** 的调暗同样是 `push_opacity`。

所以这个 crate 里**所有分数不透明度走的都是图层**，
只有段落 fade 和 `RenderFlow` 用画布 `save_layer`。这不是缺陷，是分工：
画布组是这条线程每帧要填的一块和子树等大的缓冲，
图层是合成器的事、还能在没动的帧上复用。两者在 `paint_children` 里只差一个调用。

### 顺带把第 169 轮自己写的一条弱测试改掉

`an_opacity_group_is_closed_too` 断言的是深度回零——而**一个根本不开层的
paint 同样满足它**，`RenderOpacity` 正是如此。它当时通过，
不是因为对，是因为问得太松。

改名为 `a_faded_subtree_is_an_opacity_layer_and_not_a_canvas_group`，
断言真正成立且有失败模式的事：没有画布组、孩子照画。
外加"全透明时一笔不画"（图层填了再扔掉是最糟的一种）。
list wheel 那边同样加了一条。

三条变异全红：去掉"opacity ≥ 1 时单遍"、去掉"opacity ≤ 0 时不画"、
把 list wheel 的图层改成画布组。

`engine_test_stubs.rs` 里的空 stub：20 → 19。

5253 测试通过，完整 GN 门过。

## 第 173 轮 — 一把新尺子：只断言"什么都没发生"的测试

最近四轮里出现了三条同样毛病的测试，而每一条都是**用同一种方式**被发现的：
把测试指向错误的机制，然后看着它照样通过。

- `an_opacity_group_is_closed_too` 断言存还深度为零——**一个根本不开层的
  paint 也满足**，而 `RenderOpacity` 正是如此。
- `the_highlight_is_the_first_mark_of_the_frame` 断言高亮在第 0 位——
  后面有没有字都成立。
- `an_empty_field_showing_a_hint_still_draws_its_caret` 数了一个矩形，
  而它名字里的那个 hint 从来没被设过。

这个形状可以搜，所以有了 `tools/vacuous.py`：**一条测试的所有断言都是
"这里没有这样东西"**，那它在"被测代码什么都没做"的运行里同样通过——
包括测试建错了东西、或那个特性根本没被走到。

这类测试不自动是错的（"空 decoration 一笔不画"正是这个形状且值得有），
它缺的是**同一条测试里一句"这条路确实跑过了"**。

### 尺子本身收窄了三次

- 第一版把 `assert_ne!`、`assert!(!x)`、`assert_eq!(x, 0)` 也算进去，
  报了 5253 里的 368 条——那不是队列，是情绪。
- 第二版仍然匹配断言里任何位置的 `, None`，于是把
  `resolve(true, None, None)` 这种**参数位**上的 None 也算成缺席，
  报出了两条明明带着肯定断言的测试（其中一条的下一行就是 `.is_some()`）。
- 第三版漏掉了取反：`assert!(!sheet.is_empty())` 是"确实有东西"的主张。

368 → 145 → 47 → **12**。一把点名了正在做对事情的测试的尺子，
读它的人付出的比省下的多。

### 修了三条

- `a_field_told_not_to_show_a_caret_shows_none`：补上"同一个字段把插入符打开
  就画一个"。
- `a_descendant_finds_the_overlay_above_it`：名字说的是"找到了上面的 overlay"，
  而唯一的断言是计数为零——那是**一个指向空处的句柄**也会给的答案。
  补上一次插入。
- `a_wheel_standing_where_it_means_to_stand_is_given_nothing_to_do`：
  补上"被甩过的轮子有事可做"。第一版我写成"稍微偏离但速度为零"，
  踩了 `FrictionSimulation` 的 `debug_assert`——滚回去是一段运动，
  而没有速度的运动这套物理造不出来。已写进注释。

三条变异全红（把插入符关死、让句柄的计数恒为零、让弹道模拟恒返回 None），
每一条都打中了对应的那条测试。

5253 测试通过，完整 GN 门过。

## 第 174 轮 — 把新尺子的队列读完

`vacuous.py` 报的 12 条，逐条读。四条是真缺口，八条不是。

### 四条补了伴随断言

- **`tidying_leaves_a_menu_of_nothing_but_dividers_empty`**：断言三条分隔线
  收拾完是空的。一个对什么都答空的 `tidy_dividers` 同样满足——
  而那会把每一个菜单从菜单栏上抹掉。补上"有内容的菜单留住内容"。
- **`a_painter_says_nothing_unless_it_chooses_to`**：默认的语义构建器返回空表。
  补一个"确实要说话"的画家，说明那个空表是这个画家的决定，
  而不是这个 trait 唯一能给的答案。
- **`and_an_offstage_one_cannot`**：断言探针不在命中路径上。
  **手指因为别的原因没碰到时，它同样成立**——探针尺寸为零、坐标落在盒外，
  这条测试就既不关于 offstage 也不关于命中测试了。
  补上"同一棵树在台上、同一个点能碰到"。
- **`nothing_is_collected_until_something_asks`**：关掉时不收集语义树。
  一个从不构建任何东西的 `flush` 同样满足——那是一台对每个应用都听见沉默的读屏器。
  补上"有人问的时候就构建"。

三条变异各自打红对应的测试（`tidy_dividers` 恒空、`RenderOffstage` 的命中测试
恒假、`flush` 恒 None）。

### 八条读过后原样留下

`merging_two_empty_lists_is_empty`、
`a_negative_index_is_out_whatever_the_bounds_say`、
`a_scrim_with_no_colour_paints_nothing_at_all` 等——
**缺席本身就是全部主张**，而同模块里有做正面主张的兄弟测试。
把这个数字驱到零意味着给它们塞上没人需要的断言，那正好和这把尺子的用途相反。
这句话写进了 `vacuous.py` 的文档：**这个计数不是目标**。

5253 测试通过，完整 GN 门过。

## 第 175 轮 — 选中的那一行，该用谁的颜色

回到 `depth` 队列。`SwitchListTile` 3/46 是比例缺口最大的一条，
但那个 3 大半是**类形状造成的假警报**：这个 port 把三个控件 tile 合并成
一个 `ControlListTile` 加一个 `ControlTile`（两边都有文档），
所以 `depth.py` 数在 `SwitchListTile` 上的成员其实在别处。

逐条读上游那 55 个成员：除了六个，其余全是转发给 `Switch` 或 `ListTile` 的
构造参数。六个带规则的里，`effectiveControlAffinity` 和 `.adaptive` 已经在了。
两个不在。

### 一、`selectedColor: effectiveActiveColor`

上游三个 tile 都把**自己控件的颜色**交给 `ListTile` 当选中色。
所以选中的那一行是用"让它变成选中的那个东西"的颜色画的。

这个 port 只从 list tile 主题解析选中色——于是一页绿色开关的设置里，
**选中那行的标题会用主题的强调色**，一行上出现两种强调色。

补上 `ListTile::with_selected_color` 和
`ResolvedListTile::of_with_selected_color`，优先级是
widget 的 → 主题的 → 配色的（上游的顺序）。

### 二、控件外面的 `ExcludeFocus` —— 没做，并且说明为什么

上游用它把控件排除在焦点遍历之外，可聚焦的是整条 tile，
所以在设置页上按 Tab 每行停一次而不是两次。

**没有加**：这个 port 里控件和 `ListTile` 都不注册焦点节点，
包上去就是给不存在的机制建模。

### 一处坦白：接线那一行没有测试

删掉 `ControlTile::build` 里 `.with_selected_color(...)` 那一行，套件全绿。
`ListTile` 解析出来的文字颜色**从外面观察不到**——标题作为 paragraph 发出去，
而 `Drawn::Paragraph` 记文本和位置、不记颜色。

所以测的是每个控件各自答什么颜色（三条，变异红），
而那一行的缺口写在了它上面的文档里。
**把段落样式的颜色记进 stub 就能补上这条，而且能一并补上这个 crate 里
其他所有关于文字颜色的主张**——那是下一个明确的动作。

三条变异红（widget 的颜色被忽略、选中判断恒真、switch 不读自己的主题）；
第四条（删掉接线）绿，已如实记录。

`depth_examined.json` 记了这次阅读。5259 测试通过，完整 GN 门过。

## 第 176 轮 — 文字是什么颜色，此前问不出口

执行上一轮点名的下一步。`rf_paragraph_builder_push_style` 和一次成型的那个
构造函数**本来就收 `argb`**，stub 直接丢掉。后果是
**这个 crate 里没有一条测试能说出任何文字是什么颜色画的**——
禁用的标签用启用的颜色、提示文字和旁边的正文分不出、选中行的标题用错强调色，
每一样都通过了所有写得出来的测试。

`StubParagraph` 记下颜色，`Drawn::Paragraph` 带上它。加完照例全绿。

### 三条此前问不出的话，现在问了

**一、上一轮欠着的那条接线。** 删掉 `ControlTile::build` 里
`.with_selected_color(...)`，上一轮全绿。现在红了——
一条真正画出来的标题，颜色就是它开关的颜色。
再加一条"没被选中的行不用那个颜色"。

**二、第 158 轮表盘标签那条重绘。** 当时只能断言"一个数字落在另一个数字上"，
因为颜色看不见。现在断言真正的理由：**第二遍是用选择器的颜色画的**，
而第一遍是标签色——不同色才是画两遍的意义所在。

**三、rich 段落的样式颜色。** 第一版我用了单段文本，
而**单段的 rich 不算 rich**（走廉价的一次成型路径），
去掉 `push_style` 里颜色记录的那条变异因此全绿。改成两段才碰到 builder。
顺带写明 stub 的限制：它每个段落只留**最后一个**推入的样式的颜色，
逐段的颜色是这个模型答不出的。

四条变异全红：把重绘改用标签色、删掉接线、去掉 `push_style` 的颜色记录
（两段版本下红了）、以及上一轮那三条。

`engine_test_stubs.rs` 里的空 stub：19。

5262 测试通过，完整 GN 门过。

## 第 177 轮 — 过期的自白

上一串的规律是"打开盲点会兑现早先写着'这一半问不出口'的旧测试"。
那些自白是可以搜的，所以搜了：`cannot be checked` / `not observable` /
`nothing could see` 之类。三十多处，大半是历史注记或真实的架构事实。
**两处已经过期**。

### 一、"stub 只数矩形，不说它们是什么颜色"

`the_badges_own_colour_is_recorded_for_the_widget_step` 当年只断言字段，
理由写在注释里。那句话对矩形来说早就不成立了，对文字则是上一轮才不成立。
注释活得比它描述的限制更久。

改成断言**答案**而不是输入：药丸就是给它的那个颜色、没给的时候不是；
上面的数字也一样。两条变异红（背景忽略 widget 的、文字颜色忽略 widget 的）。

### 二、"测几何的测试会是一条不可能失败的测试"

`the_tile_overrides_the_themes_leading_width` 写着这个 harness
"既看不见标题（它在 tile 内部构建）也看不见固有宽度"。
**前半句在 stub 开始记录段落和落点时就不成立了。**

`minLeadingWidth` 保留的是标题之前的空间，所以它的全部效果就是那个数字。
新测试断言**精确差值**：更宽的保留把标题推开的距离，正好等于两个保留之差。
变异红（tile 忽略自己的覆盖值）。

顺带扫了一类相关形状——只断言"刚设进去的字段读回来还是它"的测试，
八处，其余六处都不是重言式（`with_max_lines(3)` 同时改了输入类型和回车键，
`with_max_lines(1)` 映射到 `Single` 而不是 `Bounded(1)`）。

5263 测试通过，完整 GN 门过。

## 第 178 轮 — 说"永不浮动"的标签，被画在了用户打的字上面

回到 `depth`。`InputDecoration` 20/61。它那六十一个成员里绝大多数是转发给
`InputDecorationTheme` 的样式字段（这个 crate 已经在解析），
真正由它承担的是标签的那几条规则：`_labelShouldWithdraw`、
`labelShouldWithdraw`、`_floatingLabelEnabled`、`_hasInlineLabel`、
`_shouldShowLabel`。逐条对下来，**两条是错的**。

### 一、`never` 加上有内容，标签应当藏起来

上游 `_shouldShowLabel` 是 `_hasInlineLabel || _floatingLabelEnabled`，
只有一种情况为假：行为是 `never` 而标签已经"退位"。
它**无处可去**——`never` 禁掉了浮动的位置，而行内的位置现在是读者打的字。

这个 port 没有这个状态，答的是 `Inline`——**标签会画在输入的文字上面**。
`ShapedInputBorder` 的文档从另一侧警告过 `never`（缺口照旧被切出来），
这是它的近侧。加了 `LabelPlacement::Hidden`。

### 二、`isFocused && decoration.enabled` 里的 `enabled` 项漏了

一个被禁用的字段拿到焦点会浮起标签——那等于说它正在被编辑，而它不能被编辑。

顺手把 `FloatingLabelBehavior` 拆成 `withdraws()` 和 `allows_floating()`，
这是上游自己的拆法，也是这两个问题不是同一个问题的理由：
**标签退位是因为字段里有东西，浮起来则要行为允许**。

三条变异全红：去掉 `enabled` 项、`Hidden` 改回 `Inline`、`Always` 不再压过一切。

`depth_examined.json` 记了这次阅读。5266 测试通过，完整 GN 门过。

## 第 179 轮 — 注释说"上游的三条断言"，上游有六条

先试了一把新尺子：**会分支、却仍有一格状态到不了的函数**——
第 178 轮那个 bug 的成因正是如此。收紧后只剩 13 条候选，
挨个查前三条，**全是假阳性**：`maybe_invoke` 的 `Handled` 由
`to_key_result` 给出、`direction` 的 `Up` 由 `flip_axis_direction` 给出、
`in_direction` 的 `Retraced` 由 `pop_policy_data_if_needed` 给出。

委托是常态，所以这个形状不跟数据流就搜不出来。**没做成工具**——
这是名字形状的启发式在这个代码库上第五、六次失效，
`variant_sweep.py` 当初就是为此而生。结论记在这里。

### 然后是这一轮真的东西

`InputDecoration::validate` 的文档写着"Upstream's three 'only one of' asserts"。
**上游有六条**：label、hint、helper、prefix、suffix、error。
少的三对，连同它们所说的那三个 widget 形式，都不在这里。

后果不只是少了断言：

- 上游 `_hasError` 是 `errorText != null || error != null`，
  而字段下面的一切都转在这一个答案上——副标题那一行归谁、边框什么颜色、
  标签什么颜色。**只建模了字符串形式，于是一个给了 error widget 的字段
  报告"没有错误"**：它的 helper 行原地不动，边框还是启用时的颜色。
- 以字段另一端的同一处不对称：**以 widget 形式给的标签是 `Absent`**——
  从不浮起、从不退位，那个字段看起来像没有名字。

补 `has_error()`、`has_label()`、`hint_text` 和缺的三条断言。
三条变异全红。

`depth_examined.json` 上 `InputDecoration` 那条补了这一段。
5269 测试通过，完整 GN 门过。

## 第 180 轮 — 一个只在 debug 门里存在的符号

### 先处理一件事故

下游 `rustflutter_album` 链接失败：

```
lld-link: error: undefined symbol: rf_paint_set_color_filter
lld-link: error: undefined symbol: rf_paint_clear_color_filter
```

这两个符号是早先一轮我为 `RenderImage` 的染色加进 `rustflutter_ffi.cc` 的
（blend mode 合成的是图像与背景，染色要的是 `ColorFilter.mode(colour, srcIn)`，
两者不是一回事）。源码里有，`out/host_release/obj/flutter/rust/rustflutter_engine.lib`
里没有——**那份静态库停在 8 月 17 日**。

原因是我的门只构建 `out/host_debug_unopt`。改了 FFI 而只在 debug 侧验证，
release 侧的静态库就悄悄落后，而落后的表现是在**别人的**构建里报未定义符号。

重建后两个符号都在（`External`）。**门从这一轮起加上
`ninja -C out/host_release rustflutter_engine`**，FFI 一改两边同时构建。

### 然后是这一轮的移植

先试了"注释里写下的数字是可以对着上游数的断言"这个思路
（第 179 轮的 `three` 对 `six` 就是这么找到的）。查了三条：
`SliverAppBar` 的三条断言、`CupertinoDatePicker` 的十二条——都对。
命中率约三分之一，方法记下但不做成工具。

**`AppBar` 的标题居中规则整个不在。** 上游：

```dart
return centerTitle ?? appbarTheme.centerTitle ?? platformCenter();
```

`ResolvedAppBar::of` 只有中间那一级，然后 `unwrap_or(false)`——
**于是在 iOS 和 macOS 上标题从不居中**，而那是那个平台导航栏的全部惯例。

`platformCenter()` 里还有一句容易漏掉的：Apple 平台**只在动作少于两个时**居中。
居中的标题两侧都有按钮，就得短到能塞进中间；长出第二个动作的栏宁可放弃居中，
也不截断标题。

三条变异全红：Apple 分支恒真、macOS 掉队、动作数量泄漏到别的平台。

5272 测试通过，完整 GN 门过，release 引擎库重建。

## 第 181 轮 — 五份引擎库，五份都旧了

上一轮那个事故只修了 `host_release`。这一轮先问：**还有几处**。

```
host_debug_unopt         2026-08-17    ← debug 门自己的，也旧了
host_release             2026-08-25    ← 上一轮刚修
host_release_unopt       2026-08-15
android_release_arm64    2026-08-16
android_release_x64      2026-08-16
FFI 源码                  2026-08-24
```

**五份里四份比源码旧**，包括 `host_debug_unopt` 自己的那份——
debug 门跑的单元测试链接的是 **stub** 引擎，从不链接真的那一份，
所以门每轮都绿而那份库一直在腐烂。

五份全部重建，`llvm-nm` 逐个确认两个符号都在。

### 而"以后记得重建"是一句关于我记性的话

所以有了 `tools/stale_engines.py`：拿每个输出目录里的引擎库和
本项目自己写的那几个 C++ 文件（`rust/ffi`、`rust/host`、
`runtime_controller.cc`、`rust_app_api.h`）比时间戳，旧了就非零退出。

碰一下 `rustflutter_ffi.cc` 验证它会咬：五份全报 stale，并把
每一条重建命令打出来。重建后归零。

### 然后是移植

`WidgetsApp` 10/42。上游 `_resolveLocales` 有两级覆盖再落到基础算法，
**这个 port 只有最后一级**——应用根本无法干预语言解析。

移植了那三步，并写清楚为什么是这个顺序：列表回调先，
因为它是唯一看得见读者**按顺序**要了什么的那个（`[fr_CA, fr_FR, en]`
说的事不是单个 locale 能说的）；单个回调只拿第一个首选；
而返回 `None` 是"继续往下"而不是"没有语言"——否则每个只想偶尔介入的回调
都会把它服务的应用整个变白。

写完跑变异，**交换两个回调次序那条存活**——因为我没有一条测试让两个回调
同时开口。补上判别式后三条全红。

5278 测试通过，完整 GN 门过，五份引擎库全部重建。

## 第 182 轮 — 同一处缺失，在中间是绕过去，在末尾是全部作废

`Navigator.defaultGenerateInitialRoutes` 整个不在。一个从 `/a/b/c` 启动的应用
拿到的不是一条路由而是**一叠**：`/`、`/a`、`/a/b`、`/a/b/c`。
理由是深链接指向的是应用**内部**的一个位置，到了那里底下什么都没有，
读者就只剩系统的返回键可用；铺上祖先，返回键才走得出去，
而且走的是他手动导航过来的那条路。

### 那条不对称

一段前缀不存在，会被**按它所在的位置**用两种方式对待，
而上游只为其中一种写了注释：

- 缺在**中间**的被跳过，两侧因此相邻。上游自己的例子是
  `routes = ['A', 'A/B/C']` 而 `A/B` 不存在，答案是 `['A', 'A/B/C']`——
  应用的路由表允许有洞。
- 缺在**末尾**的让整叠作废。那是读者真正要求的那一个；
  没有它，其余的只是一堆"通往他到不了的地方"的祖先，
  所以上游报错并只开 `/`。

**同一处缺失，是绕过去的洞还是放弃的理由，只取决于它在第几位。**
这条写成了一条专门的测试：同一个 `/a/b/c`，一次缺 `/a/b`、一次缺 `/a/b/c`，
两次的答案分属两类。

另外三条也钉住了：不以 `/` 开头的名字只试一次、没有祖先可拆；
`/` 两个分支都不进，靠最后那个空栈兜底；
而上游在最后那一次尝试**去掉了 `allowNull`**——
一个连 `/` 都没有的应用是要报告的错误，不是可以留空的栈。

四条变异全红：末尾缺失改成普通的洞、任何洞都致命、
只开完整那一条而不铺祖先、去掉空栈兜底。

5286 测试通过，完整 GN 门过。

### 第 183 次：一个子树可以被排除在 Tab 之外，同时仍然可以被聚焦

`FocusTraversalGroup`（depth 2/11）补上上游的 `descendantsAreFocusable` 与
`descendantsAreTraversable`。这两个标志此前一个都没有，也就是说
`ExcludeFocus` 和 `FocusTraversalGroup(descendantsAreTraversable: false)`
在本移植里都无从表达。

- `FocusEntry` 与 `Focus` 各增两个字段，配 `with_descendants_focusable`
  / `with_descendants_traversable` 两个构造器，另加
  `FocusTraversalGroup::unfocusable` / `::untraversable` 两个快捷构造。
- `can_take_focus` 是**对所有祖先的与**（上游 `canRequestFocus`）：
  任何一个祖先说不，无论隔多远都够了。
- `skips_traversal` 是**对所有祖先的或**（上游 `skipTraversal`）。
  两者形状相反是有意为之，各自照上游的写法写，而不是互相定义。
- `traversal_order` 的过滤与压栈、以及 `focus(id)` 都改走这两个判定。

写测试时发现的真差别：上游 `traversalDescendants` 过滤的是
`!skipTraversal && canRequestFocus`，所以**蕴含只朝一个方向走**——不可聚焦
必然不可遍历，反之不成立。`skips_traversal` 因此要先问 `can_take_focus`。
`the_implication_runs_one_way_only` 一条测试把两半写在一起，因为任何一半
单独都能被"让两个标志做同一件事"满足。

变异：四条（focus 不再走链、不可聚焦不再蕴含不可遍历、"所有祖先"退化为
"最近祖先"、不可遍历反过来蕴含不可聚焦）全部转红，且两个方向各自被不同的
测试打中。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，stale_engines 全部不落后。
门：5291 + 333 通过；五个输出目录的 rustflutter_engine 全部重建通过。

### 第 184 次：ledger 把 `Feedback` 交给了它用的材料，而不是它本身

`Feedback`（widgets/feedback.dart）在 coverage ledger 里是一条 `equivalent`
行，写着交给 "services::system 的震动/音效函数"。这条读反了：声音和震动是
**材料**，`Feedback` 是唯一说出"哪个手势在哪个平台上值得哪一种"的地方，而这个
判断本移植里根本不存在。绕过它的应用会在没有振子的桌面上震动，又在想要同时
听到点击声的 iPhone 上保持安静。

新建 `feedback.rs`，四个入口照上游写：

- `for_tap` / `for_long_press` **先无条件发语义事件**，再问平台。这个顺序比
  看上去重要：Linux/macOS/Windows 上平台那半边什么都不做，所以把语义事件折进
  switch 的第一条分支，在 Android 上看不出来，在 Windows 上则是读屏用户唯一
  能得到的东西全没了。
- Android/Fuchsia 的长按是**无参数的 `vibrate`**，不是 heavy impact；heavy
  impact 是 iOS 的。两条分支很容易写反，因为各自单独看都说得通。
- iOS 对一个手势做**两件事**（点击声 + heavy impact），而对轻点什么都不做。
  所以 iOS 不是"安静的那个平台"。
- `wrap_for_tap` / `wrap_for_long_press`：反馈在回调**之前**发出；回调为
  `None` 时返回 `None`，而不是一个只发声不做事的闭包。

顺带补上 `HapticFeedback` 的三种 notification 触感（success / warning /
error）——此前完全无法表达，而 iOS 上这三种走的是另一个发生器
（`UINotificationFeedbackGenerator`），不是"同一件事换个力度"。
`TargetPlatform::ALL` 新增，因为按平台分派的代码几乎都有一条"其余什么都不做"
的分支，抽查两个平台等于四个平台从没被看过。

`depth.py` 对 `HapticFeedback` 会永远报 2/8：上游八个静态方法在线上是**同一个**
方法名，只有参数不同，所以这里是一个函数加一个八值枚举。这是拼写差异，不是
缺口，已记入 `depth_examined.json`。ledger 里那条 `equivalent` 行删除——
`Feedback` 现在是真符号。

变异：五条（语义事件折进响的分支、Android 长按改成 heavy impact、iOS 丢掉
点击声、Fuchsia 归到桌面一侧、包装器改成先回调后反馈）全部转红，各由不同的
测试打中。`no_callback_is_no_wrapper` 一开始被 `vacuous.py` 抓住——整条测试
只声称"什么都没发生"，补上"有回调就有包装器"那一半后回到 8。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5298 + 333 通过；五个输出目录的 rustflutter_engine 全部重建。

### 第 185 次：藏起来和拿掉是两种状态，而这里只有一种

`MagnifierController`（depth 2/8）。本移植的 `MagnifierHost::hide` 顶着一句
"Upstream's `MagnifierController.hide`, which keeps the entry" 的注释——这句
**说错了上游**。上游是 `hide({bool removeFromOverlay = true})`，默认是拿掉；
本移植实现的是 `hide(removeFromOverlay: false)`，而那个拼法上游只为一个调用者
存在：Cupertino 放大镜在手指拖过阈值时自己藏起来，需要 entry 活着，好让手指
回来时找到同一个。上游自己的指引正好反过来："in general, `removeFromOverlay`
should be true"。

- `entry: u64` 变 `Option<u64>`。
- 新增 `exists()`（上游 `overlayEntry != null`，也就是
  `SelectionOverlay.magnifierExists`）。这是**上游两个守卫实际问的问题**：
  `showMagnifier` 在已存在时提前返回，`hideMagnifier` 在不存在时提前返回，
  后者还带一句注释说明为什么不能问 `shown`——"放大镜可能还在 overlay 里，
  只是没显示"。本移植此前根本没有这个问题的答案，只有 `is_shown()`；用它当
  守卫的调用者每次手指越过阈值再回来都会插入第二个放大镜。
- `hide(remove_from_overlay: bool)` 按上游取默认语义；`dismiss(self)` 改名
  `remove_from_overlay(&mut self)`，对齐上游那个 `@visibleForTesting`、
  "abrupt removals" 的同名方法，并在第二次调用时返回 false 而不是静默。
- `is_shown()` 加上 `exists()` 项；`update` 在 entry 已消失时不再计算位置。

两条变异先是存活，查下来两处都是**冗余**：`remove_from_overlay` 自己把
`shown` 置了 false，于是 `is_shown` 里的 entry 项和 `hide` 里的
`if !exists() return` 守卫都观察不到。上游的 `removeFromOverlay` 只有三行，
一行都不碰 animation controller。改成照上游写之后，entry 项变成可证伪的
（变异 3 转红），而 `hide` 的守卫在这里确实买不到任何东西——没有要 await 的
动画——所以删掉并把原因写在文档里，而不是留一行读起来像规则的死代码。

变异：六条中五条转红（默认不再拿掉、默认总是拿掉、`shown` 丢掉 entry 项、
`exists` 改答另一个问题、`update` 继续搬已消失的 entry）。第六条是把那行冗余
赋值加回去，本就是 no-op，绿是正确结果。

**未完**：`MagnifierController` 仍留在 depth 队列上，是 7/8 而不是 8/8。缺的
是 `animationController`——本移植的放大镜没有进出动画，瞬间出现和消失。这是
真缺口不是拼写差异，所以**没有**记进 `depth_examined.json` 把它从队列里藏掉。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5301 + 333 通过；五个输出目录的 rustflutter_engine 全部重建。

### 第 186 次：一行注释宣称的三件事，三件都不对

`ControlTile::control_active_color`——第 175 次补上的那条"选中行用自己控件的
颜色"的链——顶着一句注释："Both resolve through a state property keyed on
`selected`, and both fall back to the scheme's primary."。逐条对上游查下来，
这句话描述的三件事没有一件成立。

1. **开关那支读的是 track，上游读的是 thumb。** 上游的链是
   `activeThumbColor ?? activeColor ?? switchTheme.thumbColor ?? ...`。在
   常见的双色开关上（有色轨道、浅色滑块）这是两个不同的颜色，所以凡是同时
   设了两者的主题，选中行的标题都画错了。
2. **状态集问的是控件的值，上游问的是行的 `selected`。** 上游是
   `final states = <WidgetState>{if (selected) WidgetState.selected}`，
   `selected` 是 tile 的属性。两者是不同的属性，日常就会分开——设置页会把
   读者刚到达的那一行标为选中，与它的开关开没开无关。此前用值去问，两个
   方向都答错。
3. **checkbox 和 radio 那支根本没问过任何主题**，直接返回 accent。上游是
   `checkboxTheme.fillColor?.resolve(states)` / `radioThemeData.fillColor`。
   一个把复选框改了色的页面，选中行的标题会留在默认色上。

第三件里还有一半是**形状差异而不是缺口**，也写清楚了：上游三条链都止于
`theme.colorScheme.secondary`，而本 crate 的 `components::Theme` 是一套更小
的调色板，根本没有 secondary 这个角色，所以最后一步落在它的 `primary`。这是
一次替换，写成替换，而不是继续声称上游止于 primary。

第 175 次的两条测试是照着实现写的（设 `track_color` 再断言拿到它），所以改成
读 thumb 后立刻转红。新测试把 thumb 和 track 设成**不同**颜色，两个方向各断言
一次；单色的测试看不出这个区别。

变异：六条（改回读 track、状态集改回问控件的值、状态集恒空、checkbox 不读
主题、radio 不读主题、checkbox 改读开关主题）全部转红。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5307 + 333 通过；五个输出目录的 rustflutter_engine 全部重建。

### 第 187 次：一扇写着"关"的门一直开着

先写了个探针，把 port 文档里 71 段 ```dart 引用逐行拿去上游比对。噪声很大
（29 段有对不上的行，绝大多数是带 `...` 的转述而非逐字引用），**不值得立成
尺子**——一把大部分时候是对的尺子比一把有明说盲区的更糟，因为它错的那些正是
你会照着动手的。但它当一次性筛子是有用的，交出了几个候选。

顺着 `focus_node.rs` 那条候选（引用本身是对的，假阳性）读到上游同一个文件的
第 1376 行：

```dart
// FocusScopeNode
bool get descendantsAreFocusable => _canRequestFocus && super.descendantsAreFocusable;
```

这是**只有 scope 才有的覆写**，两种节点对自己的 `canRequestFocus: false` 处理
完全不同：

- 普通节点不能取焦点，就只是**一个**节点被键盘跳过，子节点毫发无损——这正是
  `Focus` 包装器可以关掉而不禁用里面那个控件的原因；
- **scope** 不能取焦点，里面的一切都取不到。`FocusScope(canRequestFocus: false)`
  就是靠这个把模态下面整页禁用掉，而不用碰页上任何一个字段。

本移植对所有祖先一律读那个存储字段，所以**一扇写着"关"的门一直开着**：模态
下面那一页的每个字段仍然可以被点、被 Tab 走到。

新增 `FocusTree::descendants_are_focusable(id)` 作为 getter，`can_request_focus`
改走它。另记一条上游的约束：`FocusScopeNode` 的构造函数传
`super(descendantsAreFocusable: true)` 且不提供这个参数，所以 scope 的存储字段
永远是 true，覆写是它关门的**唯一**途径——测试因此要构造一棵上游造不出来的树
才能区分"读 canRequestFocus"和"读两者的与"。

变异：五条中四条转红（去掉覆写、覆写扩到所有节点、scope 忘掉存储字段、
`can_request_focus` 改回读原字段）。第五条——`traversal_children` 改回读原
字段——存活，查证后确认在任何树上都不可区分：它前面那个
`!can_request_focus(id)` 子句已经先返回了。按第 185 次的做法撤掉这处不可证伪
的改动，把原因写在原地，而不是留一行读起来像规则却什么都不决定的代码。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5311 + 333 通过；五个输出目录的 rustflutter_engine 全部重建。

### 第 188 次：`Priority` 移植了，它存在的理由没有

`SchedulerBinding`（depth 5/33）。本移植有 `Priority` 三档、`MAX_OFFSET` 的
夹取规则，以及性能模式那一半——但 `scheduleTask` 的优先队列和调度策略一行
都没有。也就是说 `Priority` 是一个**没有消费者的抽象**：三档优先级之间为什么
隔 100,000、为什么单次偏移夹在 10,000，这些写在文档里的理由在代码里无处落地。

补上另一半：

- `schedule_task(priority, task)`。上游只在**第一个任务**且未上锁时踢事件
  循环。两半都是"不要问两次"：队列里已经有活就已经有回调在路上，上锁的绑定
  由 `unlocked()` 来踢。每次都踢会让一轮把整个队列跑完，而那正是队列要避免的。
- `set_locked` 返回"解锁这一下是否就是那个踢"——上锁期间到达的活没有自己的
  踢，只能从这里来。
- `default_scheduling_strategy`：只要还有帧回调注册着（也就是有东西在动），
  就只有 `Priority::ANIMATION` 及以上能跑。三档的间距就是为这条线服务的——
  idle 的活恰恰是不该和运行中的动画抢帧预算的活。
- `handle_event_loop_callback`。**返回值是微妙的那一半**，上游文档写得很明白：
  "Returns true otherwise, **including when no task is executed due to
  priority being too low**."。调用方靠 true 给自己重新上弦，所以"有活但还
  不到时候"和"我跑了一个"必须给同一个答案；被饿着的任务若答 false，循环就停了，
  等动画结束时它还在那儿，而已经没有东西会来叫醒它。false 只留给上游点名的
  两种情况：空队列和上锁。

变异：九条中八条转红。第九条——把"只问队首"改成"扫描队列找第一个能跑的"——
存活，查证后确认**在任何队列上都不可区分**：队列是降序的，队首被拒就是全部
被拒。"只问队首"因此不是独立规则而是排序的推论；那条测试改名为它实际成立的
说法（队首被拒时整队停住且一个不丢），并把这一点写在测试里。

另外把那条测试里等优先级的先入先出断言去掉了：文档刚说过上游的堆不保证这个
顺序，测试就不该靠它——三个优先级现在互不相同。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5320 + 333 通过；五个输出目录的 rustflutter_engine 全部重建。

### 第 189 次：第 185 次造了 `exists()`，它的调用者在这里

`SelectionOverlay`（depth 9/43）。上游的这个类除了两个手柄和工具条，还拥有
`_magnifierController`，并对外给出四个入口——`magnifierExists`、
`magnifierIsVisible`、`showMagnifier`、`hideMagnifier`（加上
`updateMagnifier`）。本移植的 `SelectionHost` 一条都没有：放大镜和选区覆盖层
在这里是两个互不相识的东西。

补上之后，`showMagnifier` 里三件事都是承重的，而且顺序就是理由：

1. **守卫问的是 exists 不是 shown。** 中途自己藏起来的放大镜仍在 overlay 里，
   按可见性守卫会在手指每次越线回来时留下一个旧的、再插一个新的。这正是第
   185 次那个 `exists()` 存在的原因，而它此前没有调用者。
2. **工具条先下去。** 选区工具条和放大镜是同一块文字上争同一块地方的两样
   东西，而跟着手指走的是放大镜。反过来不成立：`hideMagnifier` **不**把工具条
   放回来——它是靠再次做出唤起它的手势回来的。
3. **先问 builder 再插入。** 上游在插入任何东西之前先构建，builder 返回 null
   就直接返回；这就是 `showMagnifier` "safe to call on platforms not mobile"
   的全部含义。先插后问会在每个桌面长按上留下一个空 entry。而工具条是在问
   builder **之前**就下去的——把守卫挪到之后会让桌面的工具条留着，读起来像
   对的，但不是上游。

变异：九条中七条转红。两条存活的都是**冗余守卫**——`hide_magnifier` 和
`update_magnifier` 各自照上游写了 `if (overlayEntry == null) return`，而下一层
的 `MagnifierHost::remove_from_overlay` 和 `MagnifierHost::update` 已经各自
挡住了。这是第三次遇到同一个形状，处理照旧：撤掉，把上游的形状留在文档的引用
里，而不是留一行读起来像规则却从不决定任何事的代码。一条规则配两道守卫，
意味着其中一道永远不是任何事发生的原因。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5327 + 333 通过；五个输出目录的 rustflutter_engine 全部重建。

### 第 190 次：把手工做了五次的那件事交给机器

同一个形状在五次里出现了四次：第 185 次 `hide` 的守卫、第 187 次
`traversal_children` 的、第 188 次 `handleEventLoopCallback` 的队首检查、
第 189 次放大镜的两条。每一条都是因为我**恰好选中它**写成变异才发现的。
没选中的那些还在原地。

`tools/idle_guards.py` 把选择权交出去：对一个文件里每一条

    if <条件> {
        return <表达式>;
    }

形状的守卫，删掉这三行、跑单元测试、还原。删掉后仍然全绿的，是**候选，不是
缺陷**——工具文档把三种情况都写明了，只有第一种该照着改：

1. 守卫真的冗余（下一层或后面已经挡住了）；
2. 守卫承重但**没有任何测试走到它**——这是缺测试，删守卫恰恰是最错的做法；
3. 类型系统在所有可达状态下已经排除了它。

只有读代码能分辨，所以工具打印候选就停。一把替我做决定的工具会有三分之一
的时候是错的，而错的那三分之一正是我会照着动手的。

端到端验证：把第 189 次删掉的那条守卫临时加回去，工具自己找到了它。

第一次正经出手就在 `navigator.rs` 捞出**两条第二类**：

- `below_top` 的 `if self.history.len() < 2`。下一行对 `usize` 减二，栈里只有
  一条路由时会下溢 panic。而这个 case 是可达的：调用者是
  `did_start_user_gesture`，**根路由上的回扫手势就是恰好一条路由的栈**。
- `maybe_pop` 的 `if self.history.is_empty()`。上游 `maybePop` 在看任何
  disposition 之前先返回 false，所以空导航器对三种 disposition 一律答 false
  ——包括 `DoNotPop`，那个在有路由时意味着"处理了，但什么都没动"。

两条都补了测试，`navigator.rs` 的候选数从 2 降到 0。

`focus.rs` 报出的三条属于第三类：`hover` / `focus` / `set_enabled` 的相等检查
在上游同样冗余（`update` 本就只在派生答案变化时才触发回调）。守卫保留——它们
是上游的形状且不花钱——但把注释改准了：原文说"根本不进回调机制"，这对控制流
成立，读起来却像是在讲结果。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5329 + 333 通过；五个输出目录的 rustflutter_engine 全部重建。

### 第 191 次：按下去长出来的那个矩形

下游应用（`rustflutter_album`）报告：按钮按下去是一个矩形高亮框逐渐扩张，
而正确的 Flutter 看不见任何矩形。

先排除了框架自己画错形状的可能——两个探针都确认涟漪发出的是
`RRect 80×80 radius 40`，也就是一个正圆。问题在**裁剪的形状**。上游的
`paintInkCircle` 有三条分支：

```dart
if (customBorder != null) canvas.clipPath(customBorder.getOuterPath(rect, ...));
else if (borderRadius != BorderRadius.zero) canvas.clipRRect(...);
else canvas.clipRect(rect);
```

本移植只有第三条。Material 按钮是胶囊形，所以涟漪一旦宽到越过两端的圆角，
就把**外接矩形的四个角**填满——胶囊外面四块方形色块，跟着涟漪一起长大。
读者看到的不是一个圆在按钮里铺开，而是一个矩形从按钮里长出来，而那个形状
上游根本没有。

`Ink::with_corner_radius`（上游 `InkWell.borderRadius`）补上，按钮把自己那个
"和它一样高的一半"的半径交下去。裁剪加在容纳层而不是圆上，因为本移植的容纳
层就在那里，效果相同：容纳层里唯一够得着角落的就是涟漪。

**打开的盲区**：层裁剪此前只被 `LayerCalls` 计数，没有任何东西记录它的形状，
所以"裁成矩形"和"裁成胶囊"是同一个观察。新增 `Drawn::ClipRectLayer` 和
`Drawn::ClipRRectLayer`，与画布上的 `Drawn::ClipRect` 分开——两者是不同的事件，
一条断言其中之一的测试不该被另一个满足，而此前写的每一条 `ClipRect` 断言指的
都是画布那个。

盲区一打开就有一条既有测试转红：`and_a_child_that_fits_is_not_clipped_at_all`
断言"装得下就完全没有裁剪"。这句话只在**画布调用**可见时成立——裁剪层一直
都在推，只是没人看得见。上游的 `RenderClipRect.paint` 只看 `clipBehavior`，
从不问孩子装不装得下。测试改名为 `..._costs_the_canvas_nothing`，并补上一条
正面断言：层照推不误。

变异：四条（容纳层丢掉半径、`with_corner_radius` 变成空操作、容纳开关被半径
顶替、按钮不再交出形状）全部转红，其中两条同时打中按钮层面那条新测试——
用户真正看到的就是那一层。`vacuous.py` 抓住了新写的"未容纳就不裁剪"只声称
缺席，补上"容纳了就裁剪"那一半后回到 8。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5333 + 333 通过；五个输出目录的 rustflutter_engine 全部重建
（下游链接的就是它们）。

### 第 192 次：走向哪一端，就在哪一端拒绝

读第 190 次那把筛子在 `services/text_boundary.rs` 上留下的五条候选。其中三条
是**真缺口**（第二类：守卫承重，但没有任何测试走到它），而且它们讲的是同一条
规则：

```dart
// getLeadingTextBoundaryAt          // getTrailingTextBoundaryAt
if (position < 0 || _text.isEmpty)   if (position >= _text.length || _text.isEmpty)
  return null;                         return null;
if (position >= _text.length)        if (position < 0)
  return _text.length;                 return 0;
```

**每个方向都在自己走向的那一端拒绝，在身后那一端夹取。** leading 向 0 走，
所以负位置身前无物、答 `None`，而越过文末的位置身后仍有整篇文本、答 `len`；
trailing 是它的镜像。于是两者永远不会对同一个越界位置都答 `None`——把光标拖
出任何一端的读者，仍能在有边界的那个方向拿到边界。

同一条规则在 trait 的默认实现里也在，而且此前同样没有测试：默认的
`leading_boundary_at` 拒绝负位置，默认的 `trailing_boundary_at` 先做
`math.max(0, position)`。`LineBoundary` 是唯一两个默认都用的实现，而它的
`text_boundary_at` 自己会夹取——所以**没有那道守卫，向后走的调用者会拿到
`Some(0)` 而不是 `None`，永远学不到自己已经走出了文本**。

剩下两条候选是第三类：`position == 0` 和循环里的 `index >= len` 都是上游的
快捷路径，删掉后答案一样（走一遍循环也到同一处）。保留——上游的形状，且不
花钱——但在原地写明它们省的是一次迭代而不是一个结果。

变异：六条（默认实现改成向后夹取、默认实现改成向前拒绝、段落边界向后夹取、
越过文末改成拒绝、向前改成夹取、空文档不再拒绝）全部转红。
`text_boundary.rs` 的候选数从 5 降到 3，剩下的三条都已注明。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5336 + 333 通过；五个输出目录的 rustflutter_engine 全部重建。

### 第 193 次：一条以机制命名的测试，实际是被另一个机制满足的

`undo_history.rs` 的四条候选。最有意思的一条是：文件里**已经有**一条测试叫
`the_modifier_runs_before_the_duplicate_check_and_that_is_what_makes_it_work`，
而筛子说删掉那道检查它照样绿。

查下来它断言的 `stack().len() == 1` 是被 **`UndoStack::push` 自己的去重**
满足的（第 118 行：等于当前值就不入栈），和它命名的机制毫无关系。

`push` 里那道检查唯一的可观察后果，是**它会不会去碰节流窗口**：拿着没有变化
的值走到 `Throttled::call`，会把时钟开起来，于是**下一次真实编辑的窗口是从
一次光标移动开始计时的，而不是从它自己**。撤销步骤会被切在错误的地方，而栈
看上去完全正常。新测试就断言这个。

`push` 里的两次 `last_value` 比较不是彼此的快捷路径：第一次问"**控件报上来的**
变了没有"，第二次问"**栈要存的**变了没有"。写第一条测试时我选错了场景——
`last_value` 存的是**修改后**的值，所以报回 "hello" 永远不等于 "hello!"，那次
比较根本没被走到。改成报回被记录的那个值（文本框的控制器被撤销栈赋值时正是
如此），才真正打中。

`update` 那条也补上了：栈底的撤销**会把已经在那里的值再交回来**——它是停住
而不是掉下去——所以读者按住快捷键时，第一次之后每一次都会走到这里。上游的
`onTriggered` 会去设置字段的值，拿字段已有的内容再设一次不是无害的：它会移动
光标，还会和读者接下来做的事打架。

`current_value` 的空表检查保留并注明：上游 `_list[_index]` 越界会抛，而
`Vec::get` 返回 `None`，所以这里那道检查确实不决定任何事——是语言差异，不是
我们的冗余。

`undo_history.rs` 的候选数从 4 降到 1，剩下那条已注明。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5339 + 333 通过；五个输出目录的 rustflutter_engine 全部重建。

### 第 194 次：按钮旁边那第二个更暗的胶囊

下游应用（`rustflutter_album`）截图：蓝色胶囊按钮右边紧贴着一块更暗的、同样
高、右端也是圆的形状，两者合起来像一个宽胶囊。

探针把它复现得一清二楚。按钮盒子 200 宽，画出来的胶囊只有 0..60，标签停在
x=16：

    root size 200x40
    ClipRRectLayer 0..200  radius 20
    RRect          0..60   radius 20   <- 画出来的面
    Paragraph "next" x=16

右边 140 像素是**没有被画过的盒子**，而 ink 的圆角裁剪是按 200 宽做的，于是
背后的东西透出来，形状恰好是一个右端带圆角的胶囊。算术全对，只是没人去画那块
背景。

最小宽度由 `ButtonBounds` 设定，然后被它和胶囊之间那个 `RenderStack` 在往下
传时**放松掉了**——`Stack` 默认 `StackFit::Loose`。上游这条路径上根本没有
stack：state layer 是画在孩子之上的 ink feature，不是并排的另一个盒子，所以
没有东西会去放松任何约束。改成 `StackFit::Passthrough`。

`Ink` 自己那个 stack 同样改了，理由相同：上游的 ink feature 由
`_RenderInkFeatures`（一个 `RenderProxyBox`）绘制，约束原样穿过。

默认按钮此前也有 4 像素的未绘制条（盒子 64、胶囊 60），只是宽按钮上才看得出来。

变异：四条全部转红。第一轮时第三条（按下分支那个 stack）存活——测试只建了
未按下的按钮，而读者看着的正是按下那一支。测试改成四种组合（两种宽度 × 按下
与否）后打中。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5341 + 333 通过；五个输出目录的 rustflutter_engine 全部重建。

### 第 195 次：上一轮修错了地方——真正丢掉尺寸的是 `ButtonBounds` 自己

上一轮的修复经肉眼验证**没有解决问题**。读了 `rustflutter_album` 的
`ui.rs`：按钮是被 `push_positioned` 放进一个带显式 `width`/`height` 的槽位
（104×40），也就是**紧约束**。而 `ButtonBounds::layout` 把孩子用最小值去布局、
完全不看传进来的约束：

```rust
let inner = BoxConstraints::new(self.min_width, INF, self.min_height, INF);
self.size = constraints.constrain(child.layout_child(inner, true));
```

盒子被 `constrain` 成 104，孩子却早按 64 布局完了。上一轮改的两个 stack fit
在这条路径上根本轮不到——损失发生在它们**上面**。

改成把传进来的约束**抬到最小值**再传下去。这一改立刻让一条既有测试转红，
而那条红暴露了根因：上游按钮里是 `Align(widthFactor: 1.0, heightFactor: 1.0)`
（收缩包裹），本移植的 `Align` 没有 factor、会撑满可用宽度——**旧代码传
`INFINITY` 正是在绕开这一点**，代价是丢掉每一个紧约束。补上 factor 之后
两件事同时成立。

变异：五条中三条转红。存活的两条各说明一件事——把上限也抬到最小值既不可
证伪也不如上游忠实（上游从不在这里加宽上限），去掉；高度 factor 在这里被
容器的固定高度决定，保留并注明。

顺带把 `InkResponse`（`InkWell` 真正建在其上的那个类）的同三处补齐：上游的
`borderRadius` 与 `customBorder` 此前完全没有，裁剪一律是直角；stack 也是
loose 的。`customBorder` 优先于 `borderRadius`，这是上游 `paintInkCircle`
的顺序。均匀圆角走 rrect 裁剪而不是 path——上游中间那支是 `clipRRect`。

**又开了一个盲区**：`rf_layer_tree_push_clip_path` 此前只被计数。新增
`Drawn::ClipPathLayer`，用路径的**外接矩形**描述它，并写明这比
`ClipRRectLayer` 说得少：它能把胶囊和不同大小的圆分开，不能把胶囊和同样大小
的圆分开。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5345 + 333 通过；五个输出目录的 rustflutter_engine 与
rust_lib 全部重建。

### 第 196 次：同一个形状去别处找，八处里有一处是真的

按钮那个缺陷的根子是 `Align` 没有 factor——上游是
`Align(widthFactor: 1.0, heightFactor: 1.0)`，而无 factor 的 `Align` 会撑满
被给到的宽度。这个形状既然出现过，就该去别处找。

写了个筛子：`Align` 位于一个**没有设宽度**的 `Container` 里、且十几行内没有
`with_factors`。八处。

七处读下来是**对的**，而且理由各不相同：分隔线（`Divider`、
`PopupMenuDivider`）就该横贯整行；对话框动作条要靠右对齐，靠右就必须先撑满；
表格单元要填满它那一列；`PopupMenuItem` 要和菜单一样宽——上游那个也确实没有
widthFactor；标签页在 `CrossAxisAlignment::Stretch` 的行里等分。**这个形状本身
不是缺陷**，要紧的是这个控件该不该按内容定尺寸。

一处是真的：**`Chip`**。探针给出

    给 400 宽 -> 400 宽
    给 120 宽 -> 120 宽

而 chip 通常放在 `Wrap` 里，按offer定宽意味着**每个 chip 都占满一行**。补上
factor 后它按标签收缩，长标签仍会把它撑宽。

变异：两条（去掉两个 factor、只去掉宽度 factor）都转红。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5346 + 333 通过；五个输出目录的 rustflutter_engine 与
rust_lib 全部重建。

### 第 197 次：那个写下去看着无害的值

结清第 190 次筛子留下的最后三条候选。

`ClipboardStatusNotifier::fail_update` 的 `if self.disposed { return; }`。
`fail_update` **故意**写 `Unknown`——好让它稍后再试一次——这恰恰使得"往一个
已销毁的对象里写"看上去无害：它写进去的值，正是一个新建的通知器本来就有的值。

并不无害。通知器是 `ValueNotifier`，写它就是在告诉它的监听者；一个已销毁的
通知器已经说过自己结束了。**没有任何东西走到过这道守卫，而原因恰恰是那个值
看上去没变。**补了测试：死掉的通知器保住它最后说过的话，而活着的照收失败。

`focus_node` 里 `set_can_request_focus` 和 `set_descendants_are_focusable`
的两条 `if !changed`。上游那个 `if (value != _canRequestFocus)` 真正在保护的
是最后一行 `_manager?._markPropertiesChanged(this)`，一个通知；而底下的
unfocus 本来就以 `had_focus && !value` 为条件。**本移植还没有
`_markPropertiesChanged`，所以这道守卫已经没有可保护的东西了**——保留作为
上游的形状、也作为将来那条通知要落的位置，并把这一点写在原地，而不是留给
下一次重新发现。

`text_selection.rs` 的候选数 1→0；另两条已注明。第 190 次那批候选到此全部
结清：真缺口都补了测试，其余各自写明了它们为什么不决定任何事。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5347 + 333 通过；五个输出目录的 rustflutter_engine 与
rust_lib 全部重建。

### 第 198 次：一个偏移量是两个地方

`TextSelectionGestureDetectorBuilder`（depth 6/27）。上游那条最长的判断，
连注释一起横跨十几行，讲的是**在已聚焦的字段上轻点一下，是切换工具条还是把
光标挪到词边**：

```dart
} else if (((_positionWasOnSelectionExclusive(textPosition) && !previousSelection.isCollapsed)
         || (_positionWasOnSelectionInclusive(textPosition) && previousSelection.isCollapsed
             && isAffinityTheSame && !renderEditable.readOnly))
        && renderEditable.hasFocus) {
```

每一项都有它的理由，而且理由各不相同：

- **范围用 exclusive，光标用 inclusive。** 高亮区的两端是**手柄所在**，点在
  那里是冲着手柄去的、或是要把光标挪出选区。而光标没有"内部"——对一个零宽
  选区做 exclusive 判断对任何偏移量都是假，那一支永远不会触发，而它恰恰是这
  条规则存在的理由。
- **affinity 必须相同。** 换行处**一个偏移量是两个地方**。若读者点的是另一个，
  光标应当移到下一行，而不是在他们没点的位置弹出工具条。
- **非只读。** 只读字段里的光标不是读者放的，点它是"我想选"而不是"再点一次
  我自己的光标"。
- **已聚焦**管住整个条件，而不是其中一支：第一次点是为了取得焦点。
- **拼写错误在这一切之前判断**：点在拼错的词上就是要看建议，选区是什么、有没有
  焦点都不重要。

`else` 那支同样有内容：选完词边之后，**只有当什么都没动时**才升起工具条——那
说明读者是在同一处点了第二次；点动了光标则收起工具条，因为工具条属于当初为它
升起的那个选区。

顺带补上 `TextAffinity`（`dart:ui`）。本移植从做文本输入起就一直在往线上发
`TextAffinity.downstream` 这个字面量，却从来没有这个类型——没有任何东西能说出
一个位置指的是两者中的哪一个。

变异：九条全部转红。测试里那句 `assert_eq!(ALL.len(), 2)` 是同义反复（这是第
二次犯，第一次在第 165 次），改成逐项比较。

尺子：coverage 2102/0，constants 158/0/0，unwired 48/0，unvaried 0，
unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，stale_engines
全部不落后。门：5356 + 333 通过；五个输出目录的 rustflutter_engine 与
rust_lib 全部重建。

### 第 199 次：错两次的那个断言，和它指出的一整类东西

第 198 次我写了 `assert_eq!(ALL.len(), 2)`——同义反复，第 165 次已经犯过一次。
去数了数这个形状还有多少：三处。

要紧的不是"同义反复"本身。`ALL.len() == N` 是对**数组字面量**的绊线，能抓住
有人加减了一项；它抓不住的是**换掉一项**——个数不变。三处里
`TimeOfDayFormat` 那条的注释还写着"每一个都有着落，没有模式被漏掉"，而断言
根本没说这件事。改成"这六个都在 `ALL` 里 + `ALL` 里只有六个"：两句配在一起
才等于"恰好是这六个"，所以那个计数留在原地、并且现在真的在做事。没有写成
`assert_eq!(ALL, [...])`，因为那还会主张一个顺序，而这里没有东西依赖顺序。
变异验证：把 `ALL` 里的一项换成另一项的重复（个数不变），旧写法全绿，新写法
转红。

另外两处（`AutofillHints` 67 个、`SystemChannels` 24 个）逐项写出来就是把表
抄第二遍。它们要的是**另一种检查**，于是有了第十把尺子。

`tools/wire_strings.py`：本移植声明的**协议字符串**是否还存在于上游。

    pub const TEXT_INPUT: &str = "flutter/textinput";
    pub const ONE_TIME_CODE: &str = "oneTimeCode";

这些值全都在另一端按名字匹配——嵌入层、操作系统的自动填充服务、引擎的通道表。
错一个是**静默失败**：不报错，通道上就是没人听，自动填充的提示就是不出现。
数字错了通常在屏幕上看得出来，这些不会。

122 个协议字符串，0 个对不上。盲区写明了：它只检查字符串仍存在于上游，**不
检查它是不是这个常量该有的那一个**——手抄错成另一个上游字符串会通过。把每个
常量和它上游的声明配起来需要一份逐行的账本，而没人维护的账本比一句写明的盲区
更糟。验证：临时把 `flutter/textinput` 改成 `flutter/textinputt`，尺子转红。

尺子：coverage 2102/0，constants 158/0/0，**wire_strings 122/0**，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，
stale_engines 全部不落后。门：5356 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 200 次：一个字段写着"这里还没有消费者"

`CupertinoSwitch`（depth 6/26）。文件里那句注释——
"`selectionHandleColor`/`applyThemeToAll` have no consumers here yet"——
是自陈的债，而这类注释在第 187 次就证明过是值得追的。

上游打开的开关，轨道颜色是**三级**链：

```dart
widget.activeTrackColor
  ?? ((widget.applyTheme ?? theme.applyThemeToAll) ? theme.primaryColor : null)
  ?? CupertinoColors.systemGreen
```

本移植只有第一级和第三级。**中间那级整个不在**：一个写了
`applyTheme: true` 的开关被忽略，一个写了 `applyThemeToAll: true` 的主题也被
忽略，这个 crate 里的每个开关都是 iOS 绿，不管它自己或它的主题要什么。

而 `applyTheme` 是 `bool?` 而不是 `bool`，那个 null 是**第三个答案**而不是缺
一个答案：它让单个开关能在**两个方向上**与主题相左——`Some(true)` 在说不的
主题下取主色，`Some(false)` 在说是的主题下保持绿色。

默认值本身也值得留一句：`applyThemeToAll` 默认为假，因为 iOS 开关是绿的是因为
iOS 开关就是绿的，不是因为应用是绿的。主题的主色属于应用自己选的东西——按钮、
链接——把系统控件也接管过去是要显式要求的。

变异：五条（中间那级消失、控件的答案不再压过主题、主题不再是回落、一次性
颜色不再优先、默认值翻转）全部转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，
stale_engines 全部不落后。门：5361 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 201 次：同一件事说两遍，是因为两个读屏器读的不是同一处

`RawRadio.build` 里那段按平台分岔的无障碍代码。三件事，中间那件是意外的：

```dart
switch (defaultTargetPlatform) {
  case android || fuchsia || linux || windows:
    accessibilitySelected = null;  semanticsHint = null;
  case iOS || macOS:
    accessibilitySelected = value;
    if (!(value ?? false)) semanticsHint = localizations.radioButtonUnselectedLabel;
}
Semantics(inMutuallyExclusiveGroup: true, checked: value,
          selected: accessibilitySelected, hint: semanticsHint, ...)
```

- **`checked` 在所有平台都带着答案，`inMutuallyExclusiveGroup` 也在所有平台
  设上。**这两条加起来才是"单选"。本移植没有后一个 flag——而它正是把 radio 和
  checkbox 分开的那一个：两者都可勾选、都会播报开关，只有它说出"打开这一个会
  关掉另一个"，也就是"其中七个同时开着"是可能还是荒谬。少了它，一列单选被读成
  一列复选。
- **`selected` 也设，但只在苹果平台。**同一件事在两个属性里说两遍，因为两个
  读屏器读的不是同一处。到处都设并非中性：TalkBack 会把一个单选播报成
  selected **且** checked。
- **提示只给没被选中的那个**，上游自己写了理由：iOS 已经通过 `selected` 播报
  选中状态，给选中的再加提示等于说两遍。需要被告知的是**没被选中**的那个——
  那里的沉默和"这个控件什么都不做"分辨不出来。

变异：七条（去掉 group flag、`selected` 到处都设、`selected` 不看选中状态、
提示到处都给、提示不看选中状态、merge 改成交集、苹果平台漏掉 macOS）全部转红。

线索仍是自陈的债：第 187、200 次各中一次，这次扫了全库 51 条这类注释，绝大多数
是有理由的架构取舍（没有动画系统、没有模糊层），读完没有过期的。

**未做并记下**：上游 `RawRadioState` 里 `tristate => widget.toggleable` 这个
等式——单选的"可以再点一下取消"就是 toggleable mixin 的三态——本移植的
`RadioMenuButton::next_group_value` 有这条规则，而 `controls.rs` 的 `Radio`
没有组的概念，加在那里是硬凑。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，
stale_engines 全部不落后。门：5366 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 202 次：右键不是左键，四条分岔没有一条是装饰

接着第 198 次把 `TextSelectionGestureDetectorBuilder` 的 `onSecondaryTap`
移过来。整条按平台一分为二，而两边的四处差别各有各的道理：

- **苹果选中一个词，其余只放光标。** 在 macOS 上右键点一个词会选中它，好让
  菜单里的 `Copy` 和 `Look Up` 有东西可作用。在 Windows 和 Linux 上右键是为了
  开菜单，移动光标已经是它做的全部。
- **苹果会保住点在其中的那个选区。** 这就是 `_lastSecondaryTapWasOnSelection`
  的用处：在高亮区内右键不动它，于是 `Copy` 复制的是读者选的那一段，而不是
  指针底下那一个词。
- **其余平台只在字段未聚焦时才动选区。** 已聚焦的字段保留它原有的选择，点在
  哪里都一样。
- **苹果是"收起再展开"，其余是切换。** 前者意味着在别处再右键一次会把菜单
  **移过去**，后者意味着再右键一次把它收掉。两者都是有意的，一个到处都用的
  移植不是在四个平台上错，就是在两个平台上错。

`shouldShowSelectionToolbar` 只管苹果那一支。其余无条件切换——这是上游的形状
而不是疏漏：被这个标志压住的切换会留下一个收不掉的菜单。

还记下一处两个判断的区别：`_lastSecondaryTapWasOnSelection` 用的是 **inclusive**
判断，也就是**右键点在选区边缘算在里面**，而第 198 次那个左键判断不算。两者
问的不是同一个问题——左键点边缘是冲着手柄去的，而右键底下没有手柄。

变异：九条中八条一次转红。第九条（把苹果那支的条件从
`!on_selection || !has_focus` 削成 `!has_focus`）存活，因为我漏了那个组合：
**已聚焦的字段上、右键点在选区之外**——而那正是"选中该词"的主场景。补上后转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，
stale_engines 全部不落后。门：5373 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 203 次：`expand` 和 `extend` 只在一种情况下不同，而那种情况是它们名字的由来

同一个类，Shift 点击那一半。上游有两个函数，读起来像同义词：

```dart
// _extendSelection
selection.copyWith(extentOffset: tappedPosition.offset);

// _expandSelection
final bool baseIsCloser =
    (tappedPosition.offset - selection.baseOffset).abs()
  < (tappedPosition.offset - selection.extentOffset).abs();
selection.copyWith(
  baseOffset: baseIsCloser ? selection.extentOffset : selection.baseOffset,
  extentOffset: tappedPosition.offset,
);
```

沿着原来的方向往前点，两者给出同一个答案。**越过锚点往回点**才分家，而这正是
名字要说的事。选区是 4..9、Shift 点在 1：

- extend 把松的那端拖到 1，得到 `4..1`——4 到 9 那一段没了，读者刚才在往上加
  的东西丢了；
- expand 发现 1 离 base 更近，于是改以 **extent** 为锚，得到 `9..1`——原来那段
  仍在里面。

所以 expand **从不丢掉远端的边界**。这就是全部差别，也是苹果平台用它的理由：
Shift 往回点是在那边**扩大**选区，而不是重新开一个。

`baseIsCloser` 这个拼法把结论藏起来了，所以文档里直接写出来：**留下的那一端
是离点击更远的那一端，不管它是哪一个。**

还有一条只属于 macOS：

> "On macOS, a shift-tapped unfocused field expands from 0, not from the
> previous selection."

Shift 点进一个没人在的字段，从文本开头选到点击处——那是这个平台每个文本视图
的做法。Linux/Windows 用 extend；Android/Fuchsia/iOS 在按下时什么都不做，
它们在抬起时决定。

`isShiftPressedValid` 那一半也照上游写了，它自己的注释说明了理由："It is
impossible to extend the selection when the shift key is pressed, if the
renderEditable.selection is invalid." ——没有可扩的东西。

变异：九条全部转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 0，vacuous 8，
stale_engines 全部不落后。门：5381 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 204 次：一次点击的两端，各带一份规则

`onSingleTapUp`，也就是第 203 次那一半的镜像：桌面在按下时决定的，手机在抬起
时决定，两份名单恰好互补，合起来每个平台被回答一次。三件读了代码才知道的事：

- **iOS 也有 macOS 那条"未聚焦时从 0 展开"的规则**，连注释都各抄了一份——但
  在**抬起**时，因为 iOS 在那里决定。两个苹果平台在**行为**上一致，在**时机**
  上不一致。
- **Android 在普通点击后给拼写建议，Fuchsia 不给。** 两支除此之外是同样的五
  行，这是它们**唯一**的区别：把它们合并会在 Android 上丢掉拼写检查，或者在
  Fuchsia 上凭空造出一个。
- **iOS 上精确设备收起工具条，手指则切换它。** 第 198 次那条长规则只在手指
  下才走到。iPad 上的鼠标能瞄准，不需要再点一次来说明它指的是哪儿，所以它拿到
  精确光标、菜单收起。

还有一条：`requestKeyboard()` 在 switch **之后**，连"不允许选择"那一支也是从
它下面返回的。**一个选不了的字段被点一下，键盘照样起来**——读者点的是文本框，
打字是他们可能想做的另一件事。

变异：九条中八条一次转红。第九条（shift 不再要求"有可扩的东西"）存活——
第 203 次给 tap-down 那一半写了这条测试，孪生的 tap-up 没写。**一次点击的两端
各带一份这条规则，所以两处都要说。**补上后转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 66/0，vacuous 8，
stale_engines 全部不落后。门：5388 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 205 次：三个处理器对同两个问题给出三种答案

同一个类里较重的三个手势。它们看起来该是同一段代码带参数，其实不是：

|                    | 选中 | 弹工具条 | 抬起 `_shouldShowSelectionToolbar` |
|--------------------|------|----------|------------------------------------|
| `onDoubleTapDown`  | 看字段能否选 | **问标志** | 不动 |
| `onForcePressStart`| 看字段能否选 | **不问，直接弹** | **置 true** |
| `onForcePressEnd`  | **总是选** | **问标志** | 不动 |

两处顺序是有意的：

- **标志在"字段能否选"这个检查之前置位。**赋值在上游那个提前返回的上面，所以
  一个不允许选择的字段仍然把它留成 true。用力按下按定义就是深思熟虑的手势
  ——没人会不小心用力按——而那正是这个标志往下带的主张，与这个字段拿它做什么
  无关。
- **start 不问就弹，其余处理器都先问。**它刚刚自己把标志置了位；再问就是把
  自己的问题问回给自己。

而 end 问标志这件事，平时答案总是"是"（start 刚置过）。**中间唯一会把这道门
关上的是拖动**：读者把用力按变成了滚动，标志被清掉，于是松手时不会在他正滚向
的文字上弹出工具条。

还有一处：end **没有** `selectionEnabled` 检查，只有"force press 已启用"的
断言——上游根本只在 delegate 要了那个识别器时才走到这个处理器。

变异：六条全部转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 66/0，vacuous 8，
stale_engines 全部不落后。门：5393 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 206 次：长按在两个地方是两件事，而触感不是均匀给的

`onSingleLongTapStart`。苹果平台三分支，其余平台一个答案：

- **未聚焦**——选中该词，记住"这次按下时还没有焦点"，**不给触感**。字段马上就
  要取得焦点并把键盘推上屏幕，那已经是反馈了。
- **已聚焦且只读**——选中该词，**并且给触感**。这是苹果那三支里唯一会震的一支。
- **已聚焦且可编辑**——**根本不选词**。把光标放到手指处，并启动**浮动光标**，
  那是这个平台精确放置光标的手势。不给触感：浮动光标自带反馈，而震动会宣告
  一次并没有发生的选择。

其余平台只有一句：选中该词并震动。

**所以长按在 Android 上是"选中这个词"，在活着的 iOS 字段上是"让我把光标放这
儿"**——把两者一视同仁的移植，必定错掉其中一个。

`_longPressStartedWithoutFocus` 为什么要记：随后的拖动会读它。在苹果平台上，
长按之后拖动是**按词扩展**（当按下时没有焦点、或字段只读），否则是移动光标。
而到了拖动的时候字段已经有焦点了——正是它需要的那个事实已经没了。

`_onSingleLongTapEndOrCancel` 里有一处上游的**不对称**，照写不改：起手在
`iOS || macOS` 上启动浮动光标，收尾却只在 `defaultTargetPlatform == iOS` 时
结束它。猜着把它对称化就是在一个"用鼠标长按"本就少见的平台上凭空发明行为。

变异：九条全部转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 66/0，vacuous 8，
stale_engines 全部不落后。门：5402 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 207 次：锚点是按下时记的一个全局点，而文字在它底下动了

`onSingleLongTapMoveUpdate`，也就是第 206 次那个标志的调用者。

苹果那一支**不问字段现在有没有焦点**——到这时它一定有，因为按下时就取到了。
它问的是 `_longPressStartedWithoutFocus || readOnly`，也就是**按下时回答过的
那个问题被保存了下来**。这就是那个标志存在的全部理由：拖动必须继续做它由之
长出来的那次按下正在做的事，而能告诉它的那个事实，到它运行时已经没了。

另一半是那段锚点修正的算术，值得单独说：

```dart
final editableOffset = renderEditable.maxLines == 1
    ? Offset(renderEditable.offset.pixels - _dragStartViewportOffset, 0.0)
    : Offset(0.0, renderEditable.offset.pixels - _dragStartViewportOffset);
final Offset scrollableOffset = switch (axisDirectionToAxis(_scrollDirection ?? AxisDirection.left)) {
  Axis.horizontal => Offset(_scrollPosition - _dragStartScrollOffset, 0.0),
  Axis.vertical => Offset(0.0, _scrollPosition - _dragStartScrollOffset),
};
```

锚点是按下那一刻记下的一个**全局**点，选区从它连到手指。从那以后有两样东西
可能动过它：字段把自己的文字滚了，以及字段所在的页面滚了。两者都不会移动手指
——所以少了这段修正，一个会自动滚动的字段里的拖动，锚点会落在读者从没按过的
地方，选区从错误的一端长起来。

**两半问的不是同一个轴。**单行字段的文字左右滚，修正在 x 上；多行的上下滚，
修正在 y 上。外层的可滚动区有它自己的轴、单独回答——一个单行字段放在垂直滚动
的页面里，一个修正在 x、另一个在 y。

上游在没有外层可滚动区时回落到 `AxisDirection.left`（水平）。这没有区别：
那时两个像素读数都是 0，落在哪个轴上都是零。写了一条测试而不是耸耸肩，因为
将来会有人纳闷为什么回落的不是大多数页面在滚的那个垂直轴。

变异：九条全部转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 66/0，vacuous 8，
stale_engines 全部不落后。门：5410 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 208 次：上游的注释点了三个平台的名，而它管的那一支只服务两个

`onDragSelectionStart`，这个类里最后一大块。

**一次变成拖动的双击，保住它选中的那个词。**上游那个
`consecutiveTapCount > 1` 就直接返回，注释写着 "Do not set the selection on a
consecutive tap and drag."——第二下已经选中了一个词，随后的拖动是按词长的；
在这里放一个光标，等于在读者刚开始拖的那一刻把它扔掉。注意那个 return 的位置：
**在记录标志和拖动起始状态之后**，因为随后的 update 无论如何都要用它们。

按平台三分：

- **桌面**（Linux/macOS/Windows）无论什么指针都放光标；桌面上的拖动没有别的
  意思。
- **Android 与 Fuchsia** 对鼠标和触控板放光标，对手指**只在字段已经有焦点时**
  才放——而这是整个方法里**唯一**会升起放大镜的一条路。
- **iOS** 对鼠标和触控板放光标，对手指**什么都不做**。上游在 Android 那一支
  上的注释写着 "For Android, Fuchsia, and iOS platforms, a touch drag does not
  initiate unless the editable has focus"，可 iOS 自己那一支的 touch 分支是
  **空的**：那里没有焦点判断，因为那里根本没有路径。**注释点了三个平台的名，
  而它管的那一支只服务两个。**照代码写，把差异记下来。

触控笔在任何平台上都不启动任何东西，但一路过去仍然把 overlay 的两个标志设上。

一处本移植说不出来的事也写明了：上游的 `details.kind` 可空，而 `null` 是
`PointerDeviceKind.unknown` 之外的**第三个**答案——Android/Fuchsia 上 `unknown`
走那条受焦点管的触摸路径，`null` 一条都不走。本 crate 的 `PointerKind` 没有
null，所以 `Unknown` 对应上游的 `unknown`，而 null 那一档无处安放。

变异：九条中八条一次转红。第九条（把"精确设备"从 `Mouse | Trackpad` 缩成
`Mouse`）存活——我一处都没用过 `Trackpad`。补上后转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 66/0，vacuous 8，
stale_engines 全部不落后。门：5418 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 209 次：把一个样式设过去再设回来，一条消息都不该发

`SystemChrome.setSystemUIOverlayStyle`（depth 2/8）。样式是从 `build` 里设的，
而重建又便宜又频繁——嵌在几层里的 `AnnotatedRegion` 每帧都会设同一个样式。所以
上游把发送推到一个 microtask 上，等它跑的时候发这一轮里最后一个调用者要的那个。

三条出口，加上里面第四道判断：

```dart
if (_pendingStyle != null) { _pendingStyle = style; return; }
if (style == _latestStyle) { return; }          // 上游称之为 trivial success
_pendingStyle = style;
scheduleMicrotask(() {
  if (_pendingStyle != _latestStyle) { ...发送...; _latestStyle = _pendingStyle; }
  _pendingStyle = null;
});
```

**第四道是微妙的那一道**：pending 在排队之后还可能被换掉，甚至换回成当前已经
生效的那个。**把一个样式设过去再设回来，一条消息都不发**——而只有 microtask
里那第二次比较看得见这件事。

还有一处顺序：`_pendingStyle = null` 在那个 `if` **外面**。一次什么都没发的
flush 也必须把槽位清空，否则下一次 `set` 会走"已经排过队了"那条出口，而那个
microtask 早就跑完了——从此再也不发。

`handleAppLifecycleStateChanged` 只在 **detached** 时把记录清掉，理由是宿主
在应用离开时把样式丢了，所以"已经发过"这个记录也得丢——否则回来时用同一个值
调 `set` 会走"已经生效"那条出口，状态栏就带着错的样子回来。而它也是在
**microtask 上**清而不是当场清：这一轮里已经排好的发送先跑（比的是旧记录），
清除排在它后面。当场清会让那个待发的比较对上一个空值，然后朝一个正在离开的
应用发出去。

本移植没有自己的 microtask 队列，所以两半是两次调用：`set` 回答"要不要安排一次
flush"，`flush` 是那个 microtask 的函数体。

变异：八条全部转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 66/0，vacuous 8，
stale_engines 全部不落后。门：5425 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 210 次：注册会通知宿主，注销不会

补齐 `SystemChrome` 剩下的三个成员，这个类从 2/8 到 **8/8**。

**`setSystemUIChangeCallback`** 只做一个决定：**要不要告诉宿主这里有个监听者**。
上游注释写着 "Skip setting up the listener if there is no callback." ——注册时
发一条消息，清掉时**一条都不发**。这不是要抹平的疏漏：那条消息是在**请求一项
功能**，而平台那一侧没有关闭开关。被告知过一次的宿主会一直上报，框架把自己
已经没有回调可交的东西丢掉。

它什么时候才会响也值得记：只有在覆盖层会自己来去的那几种模式下——`leanBack`、
`immersive`、`immersiveSticky`。`edgeToEdge` 下覆盖层始终可见，永远不响；
`manual` 下**只在每一个覆盖层都被禁用时**才响，上游注明那让这种情况的行为和
`leanBack` 一样。

**`restoreSystemUIOverlays`** 不带参数，因为状态在宿主那边——它恢复的是
`setEnabledSystemUIMode` 上次要的东西。它存在是因为平台可以推翻应用：上游举的
例子是 Android 键盘，它弹起时强制打开状态栏和导航栏，收起时**不会通知应用**。
上游还记了一条限制值得带过来：**Android 上系统 UI 在上次改动后一秒内不能再改**，
而理由不是性能——是为了让恶意软件没法靠"重新藏得比人反应还快"来永久藏掉导航键。

**`setPreferredOrientations`** 的空列表**不是**"没有表达偏好"，而是"偏好为空"，
上游把如何解读留给平台。照原样发送；替应用做决定是嵌入层的事，不是这里的。

变异：六条全部转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 66/0，vacuous 8，
stale_engines 全部不落后。门：5429 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 211 次：`clone` 和 `dispose` 是编译器的活，`isCloneOf` 不是

`ImageInfo`，2/7 到 **7/7**。

上游这个类有 `clone()` 和 `dispose()`，还有一整段关于谁该调哪个的约定。那是
因为 `dart:ui` 的 image 是一个**句柄**：`clone()` 造出第二个句柄，每一个都要
被 dispose，最后一个走了缓冲区才走。

`Rc<Image>` 就是那套计数，由编译器做。所以这两个方法在这里无事可做，也就没有
写。**但 `isCloneOf` 不是白来的**——它回答的是监听者每一帧都要回答的问题：
*这是新的像素，还是同样的像素又来了一次？*答案决定要不要重新布局和重绘。

两边分家的地方也写清楚了：在 Dart 里 `clone()` 造第二个句柄，所以克隆出来的
`ImageInfo` 是 `isCloneOf` 而**不是** `==`（`==` 比的是句柄）。这里 `Rc` 的
克隆就是同一个指针，两个问题答案相同，于是它是 `Rc::ptr_eq`。上游需要的那个
区分是**手工计数句柄的后果**，不是关于图像的事实，合并它是对的；而**把这个
方法整个略掉就不对**——"这是不是同样的像素"仍然是一个有错误答案可选的问题
（比尺寸，或者什么都不比、每帧都重绘）。

`size_bytes` 是**解码后**的大小，不是文件的：不管源格式是什么，每像素四字节。
一张 200 KB 的照片在 4000×3000 下是 48 MB，缓存要数的是这个数——这也是上游的
`ImageCache` 按字节而不是按张数计的原因。

变异：六条中四条一次转红。另两条（去掉那个 4、把 scale 乘进去）存活——我第一版
是在那张一像素的 fixture 上量的，而它的宽高正是桩对不认识的字节返回的值，
**每一种算错都给出同一个答案**。改成用桩自己的 40×30 fixture 之后两条都转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 66/0，vacuous 8，
stale_engines 全部不落后。门：5433 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 212 次：两个形状之间不是把轮廓插值出来的

`StadiumBorder`（3/11 到 9/11）。上游的 `lerpFrom`/`lerpTo` 不去插值两条轮廓
——**它造出第三个形状**，带一个"走到哪儿了"的参数，由那个形状自己知道中途该
怎么画：`_StadiumToCircleBorder` 和 `_StadiumToRoundedRectangleBorder`。逐点
插值两条路径要求它们有同样多的点、且顺序一致，而胶囊和圆并没有。

参数的方向是这里最容易改坏的地方：

```dart
// lerpFrom：circularity: 1.0 - t     // lerpTo：circularity: t
```

这不是要顺手抹平的符号错误。那个参数的含义**始终是"有多圆"**，所以它必须从
圆所在的那一端开始数：**从**圆过来时它从 1 往下走，**往**圆去时它从 0 往上走。
变的只有 `t`。而 `eccentricity` 取自两个操作数里**是圆的那一个**——胶囊自己
没有偏心率可以参与插值；`border_radius` 同理。

不认识的形状返回 `NotSpecial`（上游的 `super.lerpFrom`，淡出淡入而不是变形）。
把这件事说出来是有意义的：一个在这里悄悄返回胶囊的移植，会把上游用交叉淡化
处理的形状变化做成一段形变动画。

变异：八条中六条一次转红，一条不编译（等同于红）。存活的两条都是测试的问题：

- `lerp_from` 的 eccentricity 从没被测到——两个用例一个用零偏心率、一个只测
  `lerp_to`。
- 边框宽度那条用了 **t = 0.5**，而**插值在中点是对称的**：那里分不出
  `lerp(a, b, t)` 和 `lerp(b, a, t)`，把两端调换仍然全绿。改成四分之一处。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5440 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 213 次：把 lerp 的两端对调，107 处里 101 处没人察觉

第 212 次那个"t = 0.5 看不出方向"的错值得机械地扫一遍。`tools/swap_lerps.py`
把 `borders.rs` 里每一处 `lerp(a, b, t)` 的两端对调、跑全量测试、再还原。

**107 处里 101 处对调后毫无察觉。**这个文件整个 lerp 家族的方向都没有被断言过。

补测试的时候撞出两处真差别：

**一、圆变成椭圆时会跳，而不是张开。**上游不需要为这一对写分支——
`OvalBorder extends CircleBorder`，所以圆自己的 lerp 就接住了它，并插值两个
偏心率（0 到 1），**那正是圆张开成椭圆的过程**。本 crate 把两者做成两个变体，
这一对就掉进了末尾的交叉淡化。而文件里那几个工具函数早就写着"the oval is the
circle with the eccentricity pinned"——**该用它们的那条分支缺了**。补在
`lerp_from` 和 `lerp_to` 两处，理由和上游需要两个方向的方法一样。

**二、`TableBorder::lerp` 的圆角。**这里原本的注释说"上游也不插值——边框带着
`t` 越过的那一侧"。这句**说错了上游**：上游构造 `TableBorder(...)` 时**根本
没传 `borderRadius:`**，于是落到默认的零——一个圆角表格边框动到另一个圆角边框，
整段动画期间没有圆角，结束时才回来。那读起来像是上游的疏漏，仍然照写，理由和
第 206 次那处浮动光标一样：把上游的省略猜成"它看上去该有的样子"，就是在发明
行为。而本移植那个更体贴的版本**同样不是上游做的事**，且没有东西能发现——
因为从来没有人问过一次 lerp 之后半径是多少。

补上的测试按类型走而不是按点抽样：形状两两组合、`TableBorder` 的六条边加圆角、
`LinearBorder` 的四条边各自的 size 与 alignment。断言的形状也订正过一次：
一开始要求每一对都给出 3.0（变形），但上游的 beveled 只和 beveled 变形，
**那是在要求得比上游多**。改成"四分之一处答案必须落在 `a` 那一侧"——变形给 3、
交叉淡化给 2，两者都是方向主张，而对调后都会失败。

候选数 101 → 94，剩下的是其他类型 lerp 里逐字段的方向，**如实记为待办**。
筛子留在 `tools/swap_lerps.py`。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5446 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 214 次：一条对三种情况都成立的断言

接着收第 213 次那 94 条。按类型补方向测试——`Border`、`BorderDirectional`、
`BorderRadiusDirectional`、两种输入框边框、以及带圆角的那几个形状分支——每个
字段给不同的一对数，好让"读错字段"和"读反方向"都失败。

候选数 94 → **65**。

然后把形状表从五个变体扩到全部（含 `StadiumToCircle` 一类只在别的 lerp 中途
存在的形状——一段被打断又改向的动画正落在那里，它们的分支和别的一样可达），
这一扩就撞上了一条：`stadium -> star` 在四分之一处给 2.5。

星形是**分段** lerp：胶囊先变成圆，再变成星，边框在每一段里各走一次，所以
"整体四分之一"是"第一段的一半"。我原来那条断言（要么 3、要么 2）对它不成立
——**不是代码错了，是断言写得比事实窄**。

改成一条对三种情况都成立的说法：**四分之一处的答案必须落在 `a` 那一侧的
中点以内**。变形给 3，交叉淡化给 2，分段给 2.5，三者都在 `a` 这边，且三者
对调之后都会失败。至于普通那几对的确切数值，由下面几条专门的测试钉住——一条
统一的断言负责覆盖面，几条专门的负责精度。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5451 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 215 次：`lerp` 先问 `lerp_from`，所以另一半从来没被走到过

继续收。**65 → 47 → 43 → 25。**

先把形状表的两端在**圆角**上也拉开（4 对 12）——半数分支在插值边框之外还
单独插值一条半径，而只读边框的测试看不见那一行被调转。断言的形状连改两次，
两次都是我要求得比上游多：

- `circle -> rounded` 的圆角出来是 12。**上游正是这样**：圆没有半径可插值，
  所以结果直接带上圆角矩形自己的那个，形变由 `circularity` 承担。
- `rounded -> stadium-to-rounded` 也是 12，上游那一支写的就是
  `borderRadius: borderRadius`——它自己的。

于是换成一条对两种分支都成立的说法：**两个方向的结果若不同，正向必须比反向
小**。原样带过去的分支两边给同一个数、什么都不主张；插值的分支给 6 和 10，
调转之后给 10 和 6。

然后是结构上的一件事：`ShapeBorder::lerp` **先问 `b.lerp_from(a)`**，只有它
拒绝才落到 `a.lerp_to(b)`。所以**凡是 `lerp_from` 接得住的一对，`lerp_to` 里
那条镜像分支经由 `lerp` 永远走不到**——43 条里有 24 条正在那一半。它们不是
死代码：两个方法都是公开的，上游的 `lerp` 也是同样的两步。补了一条直接调用
两者的测试——同一段旅程，用调用者可以用的两种问法各问一次。

剩下 25 条如实记为待办，重测的脚本留在 `tools/recheck_lerps.py`。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5452 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 216 次：左右和首尾不是同一对边，所以只能分两半来

**25 → 15。**

两处。`StadiumBorder` 与圆、与圆角矩形的那两支各自还插值一条边框，而我在第
212 次写的测试只问了 `circularity` 和 `rectilinearity`——**那一行一直是自由的**。
把两端的边框宽度也拉开再读一次。

另一处是这个文件里最绕的一段：`BoxBorder::lerp` 里"统一边框变成方向边框"。

`left`/`right` 和 `start`/`end` **不是同一对边**——一个是固定的，一个要看阅读
方向。它们之间没有可插值的东西，所以上游做了唯一诚实的处理：把真正对应的
`top` 和 `bottom` 直接对插，然后**前半段把左右淡出、后半段把首尾淡入**，
结果在中点换类型。左右用 `t * 2.0`、首尾用 `(t - 0.5) * 2.0`，各自在自己那半
段里走完整程。

上游还有一条捷径在两段式代码之前：目标的首尾都为空时没有东西可淡入，于是整段
动画就是左右按**普通速率**淡出——走两段式的边框会有一半时间站着不动。

反方向靠交换两端并取 `1.0 - t` 复用同一段算术，而那只有在**交换把 `t` 一起带
走**时才对：少了那个 `1.0 - t`，读者要求倒放而动画正放。

写这几条时我自己把 12 到 32 的四分之一算成了 16（是 17），三条测试同时转红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5455 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 217 次：抽一个点看两段式的动画，就是只看了它两条规则里的一条

**15 → 10。**（起点 101。）

四类，各有各的道理：

**同类的两个盒子边框。** `BoxBorder::lerp` 在那段复杂逻辑之前有两条容易的出口
——统一对统一、方向对方向，各交给自己的 `lerp`。第 216 次测的全是混合的一对，
这两条从没被走过。

**形状表缺 `Linear`。** 补上。

**星形第二段。** 一个胶囊变星形要经过一个圆，而那条分支顶上算出来的边框是用来
**造那个中间的圆**的。第一段里答案由胶囊那端主导，四分之一处几乎说不出它；
真正读它的是那个圆当起点的那一段。**抽一个点看两段式的动画，就是只看了它两条
规则里的一条**——所以两段各取一点，并断言两点的先后。

**阴影列表。** 两串阴影按下标走：两边都有就插值，只有一边有就淡——属于 `a`
的**淡出**、属于 `b` 的**淡入**，而这正是一次调转会让两者分不出来的地方。
两串故意取不同长度，因为等长的两串根本走不到那两条淡化分支。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5458 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 218 次：`borders.rs` 的 107 处 lerp，方向全部有人看着了

**10 → 0。**从 101 起。

最后四类，每一类都说明了"抽样"在哪里不够：

**两个未走到的捷径。** `BoxBorder::lerp` 里"目标没有首尾"和"起点没有左右"是
一对镜像的捷径，第 216 次只测了前一个。后者是：没有左右可淡出，于是首尾按
**普通速率**淡入，结果一开始就是方向边框。

**范围断言不够窄。** 星形那两条的中间圆是用**整体 `t`** 算边框的，所以把两端
调转之后答案会移动，却移不过任何显眼的界标——四分之一处正向给 3、调转给 5，
而"小于中点"两个都收。改成确切数值（前后两段各取一点：3 和 9，反向 9 和 3）。

**比较式断言在镜像分支上是空的。** `(Rounded, RoundedToCircle)` 一支插值半径，
而它的镜像走的是**另一条分支**并把半径原样带过去——两个方向答案相同，于是
"正向小于反向"什么都没说。改成直接断言那个数。

**守卫挡住的分支要专门去走。** `(Superellipse, RoundedToCircle)` 只在
`smooth_corners` 为真时才走，而表里那个是假的。

`tools/swap_lerps.py` 现在对这个文件读零：**107 处 lerp，任何一处把两端调转，
都会有测试转红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5460 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 219 次：其余七个文件的 27 处 lerp，方向也全部有人看着了；筛子自己有两个洞

**26 → 0。**`borders.rs` 之外还剩七个文件，第 218 次筛出 30 处能被静默调换。

逐个补上：`implicit.rs` 的 `Offset` / `Size` / `EdgeInsets`（两轴故意反向走，
读错字段和读反方向一样红）、`transitions.rs` 的三个 tween（`begin` 与 `end`
不可互换）、`widget_state.rs` 的两个属性 lerp 包装与 `WidgetStateBorderSide`
**两端都在**的那一支（淡入淡出的两支本来就不对称，只有这一支在中点看不出
顺序）、`decoration.rs` 的 `lerp_from` 与 `lerp_to`（后者的 box/box 分支从
`Decoration::lerp` 走不到——它先找到 `lerp_from`——只能直接调用）、`theme.rs`
的 `visual_density`，以及 `component_themes.rs` 的十一处：三个可空包装
（icon theme / button style / menu style）、五处 `VisualDensity` 分支、三处
`BorderSide` 分支。全部取四分之一处，因为 lerp 在中点对称。

**筛子把注释也当成了代码。** `component_themes.rs` 那两处"发现"在
`/// Color.lerp(null, y, t)` 这样的散文里——调换它什么都不会红。这不是发现，
是噪音。筛子现在跳过 `//` 开头的行。

**筛子被杀就会把文件留在变异态。** `implicit.rs` 那一轮撞上两分钟超时，
`finally` 没机会跑；下一轮于是把**坏掉的文件**当作基线，反过来把"改回正确"
报成了绿色发现。差点当真。现在 `swap_lerps.py` 与 `idle_guards.py` 在动手前
都先把原文写到 sidecar，启动时若发现 sidecar 就先复原——`finally` 挡不住
被杀，落盘的副本可以。

八个文件、134 处 lerp，`tools/swap_lerps.py` 现在全部读零。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5475 通过；五个输出目录的 rustflutter_engine
与 rust_lib 全部重建。

### 第 220 次：三个字段在中点跳变，因为它们需要的混合函数根本不存在

`swap_lerps.py` 的正则只认 `lerp(a, b, t)` 这一种写法。这个代码库里真正的
大族是主题的逐字段行走——`x: lerp_color(a.x, b.x, t)`，一个方法四十行——
而 rustfmt 会把长的折成四行，正则一行都看不见。**387 处从来没有被筛过。**

新筛子 `tools/unlerped_fields.py` 换了个更钝的问题：把整个混合式换成**第一
端**，让这个字段完全不动。绿，就说明没有任何测试从"混合"这条路上读过它——
不是方向，不是数值，连"它会动"都没人看着。这一族的典型缺陷不是两端调转，
而是**复制粘贴时那一行还写着上一行的字段名**，而那种错在这里同样看不见。

第一次运行就是三个真缺陷。`slider_theme.rs` 的 22 行里 17 行读绿，其中：

**`padding` / `thumb_size` / `value_indicator_text_style` 全都在跳变。**
上游分别走 `EdgeInsetsGeometry.lerp`、
`WidgetStateProperty.lerp<Size?>(..., Size.lerp)` 和 `TextStyle.lerp`，端口
三个都用了"取较近的一端"。跳变的 padding 会让整条轨道在主题过渡的中点横移
一帧；跳变的 thumb_size 会让滑块跳一下。

**原因是那三个混合函数在这个端口里压根不存在。**于是这次补上：

- `EdgeInsetsGeometry::scale` 与 `::lerp`。空端不是"另一端不动"——上游是
  `b * t` 和 `a * (1 - t)`，出现的内边距从无长出来。`Zero` 不是那个空：它是
  `EdgeInsets.zero`，一个绝对侧的实值，所以 `Zero`/`Directional` 走混合支。
  混合支这里是近似：本端口的枚举没有 `_MixedEdgeInsets` 这一支，只能先按
  从左到右解析——`add` 早就是这个近似。两端仍然对，RTL 下只有中间帧是镜像。
- `TextStyle::lerp`。上游最容易写错的一条是
  `lerpDouble(a.x ?? b.x, b.x ?? a.x, t)`：某一端没有值时，**用另一端的值
  填两端**，于是这个字段根本不动。把缺的那端当零会让字距先缩到零再回来，
  而 null 的意思是"照字体的"，不是"没有"。
- `lerp_size`，`Size.lerp` 的空端。

**16 个颜色字段一条断言全部变成承重的。**两个主题，十六个字段各给一个**互不
相同**的数，于是 `lerp(numbered(0), numbered(80), 0.25) == numbered(20)` 这
一条比较，只有在十六行各读各的字段时才成立。

**冻结在中点之前是看不见的。**剩下 8 处全是"取较近一端"的字段，而先前的测试
都取四分之一处——那里答案本来就是 `a`，冻结成 `a` 什么也不会变。补了一条在
0.499 与 0.5 各看一次的测试之后，`slider_theme.rs` 读零。

七条承重断言逐条强制改错，七条全红，各由预期的那一条测试抓住。

**下一个队列：`component_themes.rs` 里同一族的约五十处。**上游对 padding、
textStyle、elevation 一律插值（`badge_theme.dart:107-108`、
`tooltip_theme.dart:193,199`、`chip_theme.dart:539,542,545`），端口一律
`lerp_nearer`。12 处 padding、约 20 处 text style，外加若干 size / radius /
elevation。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5490 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 221 次：整套排版在主题过渡的中点跳一次字号

第 220 次留下的队列。按上游逐行对读 `component_themes.rs` 的 235 处
`lerp_nearer`，其中**一大批上游是插值的**。

**`TextTheme` 的 15 行全在跳变。**上游 `TextTheme.lerp` 十五个字号一律走
`TextStyle.lerp`。端口十五行全是"取较近的一端"——主题过渡走到一半时，应用
里**每一段文字**会在一帧里换一次字号，而不是长过去。这是这批里最显眼的一个。

一共改了这些族（每一处都先在上游确认过，不是照猜）：

- **59 处文字样式** → `lerp_text_style` / `lerp_state_text_style`
- **13 处内边距** → `EdgeInsetsGeometry::lerp` / `lerp_edge_insets` /
  `lerp_state_insets`。12 处 `padding` 分属三种类型，靠名字改会错三分之一，
  是按"所在结构体字面量 → 该结构体的字段声明"逐处定的。
- **7 处 `WidgetStateProperty.lerp<double?>`** → `lerp_state_f32`
  （elevation ×3、thickness、icon_size、inner_radius、track_outline_width）
- **2 处 `<IconThemeData?>`** 与 **2 处 `<OutlinedBorder?>`**

**有两处看着像缺陷，其实不是。**`thumb_visibility` 与 `track_visibility` 上游
包在 `WidgetStateProperty.lerp<bool?>` 里，但传进去的 `_lerpBool` 就是
`t < 0.5 ? a : b`。逐状态跳、每个状态用同一个 `t`，和整体跳是同一个答案。
`lerp_nearer` 在这里不是抄近路，它就是那个函数。注释写在原处。

**一处说不出口的差别写进了测试。**上游 `TextStyle.lerp` 的单空端不跳变：颜色
从透明淡入，其余字段在中点换。本端口的 `TextStyle` 没有可空字段——`color`、
`font_size` 是值不是选项——"字号还是 null 的样式"无处安放，所以只有一端有的
样式整体跳变。`a_style_only_one_end_names_still_steps` 把这条差别钉住，而不是
留着以后被发现。

十三族逐族强制改回 `lerp_nearer`：**十三族全红**，各由预期的那条测试抓住。

**一次自己造成的破坏。**回滚一个插错的测试块时，我按"从标记处截断再补一个
右花括号"来做——而 `component_themes.rs` 有**六个**测试模块，`mod tests` 后面
还有五个。那一刀把它们全切了，套件从 5495 掉到 5480 还是绿的。从 HEAD 取回，
并按 struct / enum / lerp 三个计数与 HEAD 对齐确认没有别的损失。教训和第 219
次那次一样：**按行号或标记截断文件，前提是文件的结构和我以为的一样。**

**下一个队列**：同一族里剩下的 Size / BoxConstraints / BorderRadius /
Alignment / ShapeBorder / Decoration 各族，约七十处。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5498 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 222 次：约束、尺寸、圆角、对齐、边和装饰——同一族的最后 37 处

第 221 次留下的队列。仍然是逐处对着上游读，不照猜。

补上四个端口里没有的混合函数：

- **`BoxConstraints::lerp`**。上游把无穷保持为无穷，而不是把它插进去：
  `a.maxWidth.isFinite ? lerpDouble(...) : double.infinity`。**无界是一种
  "类"，不是一个尺寸**——视口是故意把某条轴放开的，"离无界四分之一路"的
  约束没人能拿来布局。
- **`Radius::lerp_optional`**、**`BorderRadius::lerp_optional`**、
  **`BorderRadiusGeometry::lerp_optional`**。上游对空端有**两种写法**：前两个
  是 `b * t`，几何那个是 `a ??= BorderRadius.zero` 再线性插值。两种写法此刻
  同解——从零插到 `b` 就是 `b * t`——但只要哪一端不再是线性的，它们就会分家。
  测试把两种拼写都钉住，并说明了它们为什么此刻一致。
- **`lerp_state_size`** 与 **`lerp_state_side`**。后者是上游 `_LerpSides`：
  缺的那端取另一端的颜色、零透明度、零宽度，于是出现的边框是淡入的。

改动的 37 处（`AlignmentGeometry::lerp` 端口里本来就有，只是没人用）：
8 处 `constraints`、6 处状态尺寸、5 处 `alignment`、3 处状态边、3 处装饰、
2+2 处圆角、3 处 FAB 约束、2 处 Chip 约束，以及被 rustfmt 折行因而漏掉的
8 处——其中 `unselected_label_style` 与 `time_selector_separator_text_style`
是第 221 次那一批的漏网。

**三处确认是对的，没有动。**`ListTileStyle`、`heading_row_alignment`、
`input_decoration_theme`、`material_tap_target_size`、
`expansion_animation_style` 与 `ButtonBarThemeData::alignment` 上游本来就是
`t < 0.5 ? a : b`。

**筛出来的比我列出来的多。**改完之后按"混合函数的形状"而不是按我的清单去
逐族强制改回 `lerp_nearer`：30 个字段/混合对里有 **10 个读绿**——
actions_padding、children_padding、content_padding、extended_padding、
inset_padding、label_padding、leading_padding、tile_padding、
expanded_alignment、size_constraints。这十个是第 221 次批量改对了、但从来
没有测试看着的。补了一条测试，十个字段各给一个**全测试内唯一**的数。再筛，
49 处、30 对，**一个绿的都没有**。

新写的六条承重规则逐条强制改错：六条全红（其中 `lerp_state_size` 的空端第一
轮读绿，补测试后转红）。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0，
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5513 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 223 次：形状从来不变形，而三个筛子有半个文件没看

**二十九处形状在中点被整个换掉。**上游对 `shape` 一律走
`ShapeBorder.lerp`（少数几处是 `WidgetStateProperty.lerp<OutlinedBorder?>`），
端口二十三个主题全用 `lerp_nearer`——卡片、对话框、chip、抽屉、菜单、日期
和时间选择器的轮廓，都是在主题过渡走到一半时**一帧换掉**，而不是变形过去。

讽刺的是第 218 次整整一个 tick 都在把 `ShapeBorder::lerp` 的每一条分支修对，
**而主题里没有一处在调它**。

**一处确认是对的，专门写了测试说明为什么。**`BottomAppBarThemeData::shape`
是 `NotchedShape`，上游真的是 `t < 0.5 ? a : b`。它夹在二十多个都变形的轮廓
中间，所以"它为什么不变形"值得写在下一次扫这里的人会读到的地方：凹口是
算出来的挖空，不是有宽度的边框，圆形凹口和平边之间没有"一半"可言。

二十九处逐处强制改回 `lerp_nearer`：**二十九处全红**。

---

**三个筛子共用了一个已经过期的假设。**`idle_guards.py` 里写着：

> Every test module in this crate is the tail of its file, so the first
> `#[cfg(test)]` is enough.

写下时是真的，现在不是。`swap_lerps.py` 和 `unlerped_fields.py` 抄了同一个
`body_limit`。于是它们全都**在第一个 `#[cfg(test)]` 处停下**，而这个 crate 里
测试模块和产品代码是交错的：

| 文件 | 第一个测试模块之后仍是产品代码 |
| --- | --- |
| `render.rs` | 171,898 字符 |
| `component_themes.rs` | 96,072 字符 |
| `scrolling.rs` | 37,469 字符 |
| `animation.rs` | 32,865 字符 |

**是手工数出来的。**我按行号逐处变异那 27 处形状，筛子只报了 16 处——差的
11 处都在第一个测试模块之后。

新的 `tools/rust_source.py` 真的去数括号，给出每个 `#[cfg(test)] mod` 的字符
区间；三个筛子都改用它。边界修正之后，第 219 次那句"134 处 lerp，全部读零"
**不再成立**：真实数字是 **189**（`component_themes.rs` 11 → 60，
`borders.rs` 107 → 109，`slider_theme.rs` 0 → 3，`engine.rs` 0 → 1）。

那条注释点名了自己的假设，这是这个洞还能被发现的唯一原因——但它仍然让三个
筛子安静地只读了半个文件读了好几个 tick。

边界修好之后重跑：`component_themes.rs` 60 处里 **23 处调换无人察觉**——全在
以前看不到的那半边。补了四条方向测试（内边距 11 处、约束 6 处、对齐 3 处、
`BorderSide` 分支 3 处），每一处的两端都取到"反过来是另一个数"。再跑：
`component_themes.rs` 60 处读零，`borders.rs` 109 处读零，其余六个文件也读零
——**189 处，一处调换都逃不掉**。

**下一个队列**：`unlerped_fields.py` 的边界也修了，它现在看得见 475 处
（`component_themes.rs` 283 → 371，`theme.rs` 29 处从来没筛过）；
`idle_guards.py` 同理，`render.rs` 17 万字符、`scrolling.rs` 3.7 万、
`animation.rs` 3.3 万的产品代码从来没被它看过。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5522 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 224 次：49 个角色里只有 3 个被看着，而且是在对称的中点

`ColorScheme::lerp` 是 49 行 `mix(a.x(), b.x())`，`ThemeData::lerp` 是 16 行
`mix(a.x, b.x)`。这两处**没有任何筛子看得见**：`unlerped_fields.py` 找的是
调用里的 `t`，而 `mix` 把 `t` 闭包进去了。

原有的那条测试只看三个角色，而且取 `t = 0.5`——lerp 在中点对称，那里连两端
写反都看不出来。

**先确认没有漏掉的角色。**第一次数的时候我的正则说有五个角色没被插值
（`on_primary_fixed_variant`、三个 `*_fixed_variant`、
`surface_container_lowest`/`highest`），对着上游一看确实都是插值的——差点当成
五个真缺陷。再看代码：那五个是**折行**写的 `Some(mix(\n a.x(),\n b.x(),\n))`，
正则没匹配上。改成容忍折行之后：**49 个字段，49 个都插值，一个不缺。**

**测试是生成的，不是手打的。**49 个角色名手写一遍，就是 49 次把名字打成隔壁
那个的机会——而那正是这条测试要抓的缺陷，打错一次就变成"因为错误的理由而
通过"。所以从结构体声明生成：每个角色一个**全表唯一**的数，四分之一处读回，
再反过来读一次。`ThemeData` 的 16 个颜色同样处理。

**逐行让它读隔壁的字段。**55 处一行式的 `mix` 全部逐行改成读上一处的字段：
**55 处全红**。另外 9 处折行的，正则没覆盖，手工验了一处
（`surface_container_lowest` 改读 `surface_container_low`）——也红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5525 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 225 次：整个 `ThemeData` 可以停止混合它的 24 个组件主题，没人会发现

第 223 次修好边界之后，`unlerped_fields.py` 第一次看得见 `theme.rs`。

**但它先是几乎瞎的。**第一轮 29 处里 **27 处"编译不过"**——冻结变异写的是
`x: a.x`，而这些混合是按 `&a.x` 取参、字段又不是 `Copy` 的，移不出引用。
筛子于是只报了一处。现在它在放弃之前会再试一次 `a.x.clone()`：**29 处全部
编译得过**，读绿的从 1 处变成 **24 处**。

那 24 处是 `ThemeData::lerp` 交给每一个组件主题的那一行。每个组件主题都有
自己的测试，但**没有任何测试看着 `ThemeData::lerp` 去调它们**——整个主题可以
把第一端的组件主题原样交回去，套件照样绿。

测试是生成的：24 个名字手写一遍就是 24 次把名字打成隔壁那个的机会。每个主题
探一个它自己会插值的字段（颜色优先，其次数字，再次内边距），每个探针给一个
**全表唯一**的数。

生成器自己有三个洞，都写在原处：

- **一次跨结构体误配。**探针在整个 crate 里找 `pub opacity`，命中了别的结构
  体的同名公开字段，而 `IconThemeData::opacity` 是私有的。改成只在该主题自己
  的结构体里找。
- **私有字段挡住了 `..Type::default()`。**`IconThemeData` 因此要走构造器。
- **折行的签名找不到。**`MaterialBannerThemeData::lerp` 的参数是折行写的，
  生成器按 `pub fn lerp(a: &Type` 去找就漏了它。这四处手写。

**还栈溢出了一次。**二十组 `ThemeData` 放在同一个测试函数里，栈直接爆了——
`ThemeData` 是个很大的值。改成每个主题一条测试。

再跑：`theme.rs` 29 处**读零**。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5550 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 226 次：371 处混合里 284 处没人看着，40 条测试盖掉 164 处

第 223 次修好筛子的边界之后，`unlerped_fields.py` 第一次看得见
`component_themes.rs` 的全部 371 处。它报出 **284 处读绿**——把整个混合式冻结
成"取第一端"，套件照样绿。

**284 条独立测试是荒谬的**，筛子自己的文档就写着"六十个颜色字段的主题不需要
六十条测试"。但这个形状**真正**的缺陷是"复制粘贴的那行还写着上一行的字段
名"，而一条断言就能覆盖一整个主题：给每个字段一个该主题内唯一的数，混合两
端，再和"按预期数字构造的主题"整体比较——只有每一行都读自己那个字段，才可能
相等。其余字段两端相同，对这条比较没有贡献。

两个生成器：

- **31 个主题、180 个字段**走整体比较。
- **9 个主题、44 个字段**走状态属性。那里不能整体比较——`StateProperty` 的
  `PartialEq` 是 `Rc::ptr_eq`，两个分别构造的属性即使解析出同一个值也永不
  相等——所以逐字段解析后比较。

都是生成的，理由和第 224 次一样：**字段名本身就是被测对象**，手打一遍就是把
要抓的 bug 写进测试里。

再跑筛子：**284 → 120**。

剩下的 120 处不是同一回事，值得说清楚：绝大多数是 `lerp_nearer` 的跳变字段，
而冻结变异在中点之前是看不见的——那里"第一端"本来就是答案。这是第 220 次在
`slider_theme.rs` 上学到的同一条：**要看着一个跳变字段，测试得取 `t >= 0.5`。**
另有一批是生成器跳过的文字样式与嵌套 style。两者都是下一个 tick 的队列，而
不是这一个的失败。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5590 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 227 次：一个上游表达不出来的状态，和一处被粗糙匹配器藏起来的缺陷

三件事，都是在等第 226 次的筛子跑完时用**只读对照**找出来的。

**一、`IconThemeData.shadows` 在中点整张换掉。**上游走
`Shadow.lerpList`——多出来的那一层阴影是**淡入**的，不是在中点凭空出现。端口
是 `lerp_nearer`。

这一处第 222 次那个粗糙匹配器漏掉了，因为它只在 `material/` 目录里找单行
右值，而 `IconThemeData` 住在 `widgets/`。新的对照按"这个主题**自己**的
`lerp` 体在哪、里面对这个字段做了什么"来找，不再按目录和行形状猜。

**二、上游自己丢掉的六个字段。**`TooltipThemeData.lerp` 只赋十个字段，
`waitDuration`、`showDuration`、`exitDuration`、`triggerMode`、`enableFeedback`
一个都不赋——混合出来的主题这五个是 null，每个 tooltip 回落到自己的默认值。
`SnackBarThemeData.lerp` 同样丢掉 `showCloseIcon`。

端口用"取较近一端"把它们带过去了。这看着更像上游的疏漏而不是决定，但**端口
和上游不一样这件事本身应该是被写下来、被测试盯住的选择**。现在两处 `lerp`
的文档都点名了上游丢哪几个，两条测试把携带行为钉住。

**三、`ThemeData.brightness` 从字段改成了方法。**上游是

```dart
Brightness get brightness => colorScheme.brightness;
```

端口存成了 `pub brightness: Brightness`，紧挨着 `color_scheme`。于是端口能
表达一个上游**表达不出来**的状态：主题声称自己是亮色，而它携带的配色方案是
暗色。没有代码路径会产生它——`from_color_scheme` 是唯一设置它的地方——但
`ThemeData { color_scheme: dark, ..ThemeData::light() }` 就差一行，而
`ResolvedBottomAppBar::of` **在同一个函数里**既读 `theme.color_scheme` 又读
`theme.brightness`。改成派生就让这个不一致无法表达，这正是上游那样写的理由。

四条承重断言逐条强制改错：**四条全红。**其中 brightness 那条最初是被
`bottom_bars` 里一条既有测试抓住的，说明改动确实承重，但没有测试盯着"派生
关系"本身——补了一条，它连"两端调转"和"混合中途两者是否一致"一起钉住。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后。门：5594 + 333 通过；五个输出目录的
rustflutter_engine 与 rust_lib 全部重建。

### 第 228 次：一把新尺子，和它自己改了三次的口径

`unwired.py` 问的是"每个**主题**有没有读者"，读零已经很久。但一个四十字段的
主题，读者可以只碰三十九个——第四十个就是一个被声明、被文档、被 `copy_with`
带过、被 `lerp` 插值、被测试盯着，而**没有任何东西回答**的字段。

手工先找到一个：**`DividerThemeData::radius` 没有任何 widget 读它。**上游
`Divider.build` 读 `radius ?? dividerTheme.radius ?? defaults.radius`，而端口
的 `ResolvedDivider` 干脆把它丢了——`Divider` 和 `VerticalDivider` 永远圆不了
角，设了这个主题字段的调用方得到沉默。

于是有了 `tools/unread_theme_fields.py`。

**它的口径改了三次，每次都是尺子的错，不是端口的：**

| 版本 | 规则 | 报出 | 错在哪 |
| --- | --- | --- | --- |
| v1 | 四个主题文件之外的才算读者 | 224 | 30 个 `ThemeData` 字段 `component_themes.rs` 读得好好的 |
| v2 | 声明该结构体的那个文件之外的才算 | 152 | `ResolvedSlider::of` 就在 `slider_theme.rs` 里读 `data.track_shape` |
| v3 | 除了该字段自己的"文书"（结构体声明、`lerp`、`copy_with`、构造器、测试模块）之外都算 | **137** | |

第一个数字虚高了 63%。这一点写进了尺子的文档里——**一把没人该比读它更信任
的尺子，纠正的方向应该总是指向它自己。**

它的口径仍然是保守的：按裸字段名在全 crate 里找，所以 `color` 这种常见名会
被巧合命中而不报。它**只会少报**。

**137 条里 135 条上游自己的 widget 会读**（静态查得），另 17 条上游也不读，
属于"仅仅被携带"。前者是 135 根该接而没接的线。

### 接上第一根：`SliderThemeData::allowed_interaction`

上游在 `_SliderState._startInteraction` 和 `_handleDragUpdate` 里按它分支：

| 模式 | 点 | 拖 |
| --- | --- | --- |
| `TapAndSlide` | 跳过去 | 滑动 |
| `TapOnly` | 跳过去 | 忽略 |
| `SlideOnly` | 忽略 | 滑动 |
| `SlideThumb` | 忽略 | 只有从拇指上开始的才滑 |

端口每一个点和每一个拖都收——主题要 `SlideOnly`（应用不想让误触改动数值）
被完全忽略。

**原因是手势建在 `wired` 里，而 `wired` 跑的时候还没有可以解析主题的
context。**所以 `wired` 现在只记住"拿到一个值该做什么"，`build` 才决定"哪些
手势有资格产生一个值"。这样主题是自动被遵守的，不需要调用方多做一步。

**一处和上游的差别写在了原处**：`SlideThumb` 上游问 `_isPointerOnOverlay`，
而 overlay 比拇指宽。这里问的是拇指本身，因为这个 slider 只有拇指这一个矩形
——它把拇指画成行里的一个盒子，没有 overlay。紧挨着 Material 3 拇指外侧开始
的拖动，这里拒绝，上游接受。

五条承重规则逐条强制改错：**五条全红**。其中"解析器读错主题字段"那条第一轮
**读绿**——四条手势测试都是手工构造 `ResolvedSlider` 的，它们看着 widget 拿到
答案之后做什么，却没有看着答案是从主题来的。补了一条走真实控件树的测试。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后，**unread_theme_fields 137（新，队列）**。
门：5600 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 229 次：分隔线终于能圆角了，而找到它的不是那把尺子

第 228 次手工找到的那根线：**`DividerThemeData::radius` 没有任何 widget 读
它**。`ResolvedDivider` 干脆没有这个字段，所以无论主题怎么设，端口里的
`Divider` 和 `VerticalDivider` 都圆不了角。上游两处 `build` 都读

```dart
borderRadius: radius ?? dividerTheme.radius ?? defaults.radius
```

而 `_DividerDefaultsM2` 与 `_DividerDefaultsM3` 都不设 `radius`——所以解析就是
"主题给什么用什么，不给就是方角"。现在两条规都按方向解析后交给 `Container`。

**一处和上游的差别写在了原处**：上游把线画成一个圆角盒子的**底边框**，这里
是填满一个线厚的盒子。半径为零时两者是同一张图；有半径时两端不同，而这是
这个 `Container` 画得出来的形状。

**测试拆成了两半，因为一半说不准。**圆角填充到引擎是一条 `Path`，而
`Drawn::Path` 只记录包围盒——这是引擎桩自己声明的盲区。所以：

- `components.rs` 那条说**圆角到达了画笔**：有半径画路径，没半径画矩形，
  且没半径时不会凭空多出一条路径。
- `component_themes.rs` 那条说**到达画笔的数值是主题的**：解析器交出的正是
  设进去的那个 `BorderRadius`，未设时仍是未设。

两条各自都不是"这根线接上了"，合起来才是。测试里写明了这一点。

三段逐段强制改错：**三段全红。**

**尺子没找到它。**`unread_theme_fields.py` 按裸字段名在全 crate 里搜，而
`.radius` 到处都是——所以 `DividerThemeData::radius` 从来没进过那份名单，读数
仍是 137。尺子找到的是这**一类**缺陷；这个具体的实例是对着上游读出来的。
这正是那把尺子文档里说的"它只会少报"的样子。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后，unread_theme_fields 137（队列）。
门：5602 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 230 次：更正一个数字，然后接上 snack bar 的五条颜色链

**先更正。**第 228 次的条目里写"137 条里 135 条上游自己的 widget 会读"——那是
把 v3 尺子的**总数**和 v1 尺子的**排序**拼在了一起。用 v3 重新排过之后是
**110 条上游会读，27 条上游也不读**。当时那份工单是用旧尺子生成的，这一轮
才发现：拿它去查 `AppBarThemeData::title_spacing`，结果那个字段
`ResolvedAppBar::of_with_center_title` 明明在读。

（那个字段在 v3 下并不上榜。上榜的 `AppBarThemeData` 变成 4 条。）

### 接上的五条链

`unread_theme_fields.py` 报出 `SnackBarThemeData` 四个颜色没人读。对着上游读
下去，发现第五个问题：`action_text_color` 虽然被 `ResolvedSnackBar` 带着，
**却只是半根线**——它只取主题的值，动作自己的颜色和上游的默认都到不了。而
`SnackBar` 连 `close_icon_color` 这个字段都没有，那条链的第一级根本无法表达。

上游的五条链（`_SnackBarState.build` 与 `_SnackbarDefaultsM3`）：

| 字段 | 链 |
| --- | --- |
| `actionTextColor` | 动作 → 主题 → `colorScheme.inversePrimary` |
| `disabledActionTextColor` | 动作 → 主题 → `colorScheme.inversePrimary` |
| `actionBackgroundColor` | 动作 → 主题 → 透明 |
| `disabledActionBackgroundColor` | 动作 → 主题 → 透明 |
| `closeIconColor` | bar → 主题 → `colorScheme.onInverseSurface` |

四个背景/标签色现在从 `Option<Color>` 变成了 `Color`——上游对这五个都有确定的
默认，所以"没有值"不是一个可能的答案。`SnackBar` 补上了 `close_icon_color`
字段与构造器。

测试把**每一级**都查了，而且每一级用一个全测试内唯一的数——所以一行读错了来源
（拿主题当动作）或读错了字段（拿启用色当禁用色）都会答出一个不属于它的值。
另有一条盯着"没有动作的 bar 也照样解析动作颜色"：上游无论有没有东西可画都
解析它们，而一个伸手去取不存在的动作的解析器会 panic 而不是回答。

四条承重规则逐条强制改错：**四条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后，**unread_theme_fields 137 → 133**。
门：5606 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 231 次：一个当时记下的决定，把回落留在了不存在的地方

**先修工单。**排序脚本没有跳过注释。`ThemeData::canvas_color` 被归到
`bottom_navigation_bar.dart`，而那一处是句文档注释——真正读它的是
`material/dart` 里的 `MaterialType.canvas => theme.canvasColor`。这和第 219 次
在 `swap_lerps.py` 里找到的是同一个盲区。修好之后**总数没变（106），变的是
归因**：每个字段本来就至少有一处真读，错的是"在哪里读"。

### `ThemeData::unselected_widget_color` 到不了任何地方

`ResolvedBottomNavigationBar` 把主题的两个 item 颜色照抄成 `Option<Color>`
就结束了，没有最后一级。于是三条回落全部落空：固定条未选端的
`unselected_widget_color`、选中端的 `themeColor`、以及 shifting 条两端的
`colorScheme.surface`。

**widget 也帮不上忙**：它带的是 `has_selected_item_color` 和 `has_fixed_color`
两个**布尔**，够上游那句"不能同时给"的断言，此外什么都不够——调用方说出一个
颜色，没有地方可放。现在它带颜色本身，断言从颜色推出来。

上游的两条链（`_BottomNavigationBarState.build`）：

| 类型 | 未选中 | 选中 |
| --- | --- | --- |
| 固定 | widget → 主题 → `unselectedWidgetColor` | widget → 主题 → `fixedColor` → `themeColor` |
| shifting | widget → 主题 → `colorScheme.surface` | 同左 |

`themeColor` 是亮色主题的 `primary`、暗色主题的 `secondary`。**这个对调是有
理由的**：暗色主题的 primary 是给大面积用的浅色调，而一个小小的选中图标需要
的是强调色。

**一条既有测试的前提站不住了。**原来那条叫
`nothing_is_invented_for_the_colours_upstream_leaves_null`，注释写着"widget
会回落到 primary 和 caption 色，在这里编一个颜色它就分不清了"。**这个端口里
没有那一步 widget 代码**——解析器就是上游在 `build` 里做的事。所以那个当时
记下的决定，把回落留在了不存在的地方，`unselected_widget_color` 正是这样
落空的。测试改写了，理由写在原处；背景色仍然留 null，因为上游确实留 null，
由 `Material` 自己补。

四条承重规则逐条强制改错——把 `fixedColor` 提到主题前面、把未选端接到选中端
的角色上、让 shifting 条走固定条的回落、让 `themeColor` 不随明暗对调——
**四条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后，**unread_theme_fields 133 → 132**。
门：5612 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 232 次：五种对齐里四种到不了，因为那条规则是写死的

`ListTileThemeData::title_alignment` 到不了任何地方。列表项把上游 `threeLine`
那条规则**写死**了——三行块贴顶、否则居中——从不问主题。于是：

- 主题指定另外四种对齐中的任何一种，都被忽略；
- 上游 Material 2 的默认 `titleHeight` 也无从生效；
- 那个枚举里五个变体有四个是不可达的。

上游的链是
`titleAlignment ?? tileTheme.titleAlignment ?? (useMaterial3 ? threeLine : titleHeight)`，
现在照这个解析，列表项按解析结果选交叉轴对齐。

**五种里两种是近似的，测试里写明了。**`ThreeLine` 和 `Center` 是精确的。
`Top` 与 `Bottom` 精确到上游还会额外插入的 `minVerticalPadding`——这个行在
交叉轴上不加那一份。

`TitleHeight` **不精确**：上游在整块高于 72 时把 leading 放在**标题**顶部下方
十六像素处，否则对着两行标题居中、而 leading 取"离标题顶部更近的那一个"。
那条规则需要标题自己的盒子，而这个行只对着整条交叉轴对齐，没有那个盒子。
居中是它给得出的两个答案里更近的一个，而且短的列表项在上游也是居中——
**高的 Material 2 列表项才是两者分道扬镳的地方**。这一条有自己的测试。

三条承重规则逐条强制改错——把规则改回写死、把 `Top` 与 `Bottom` 对调、让
解析器一律给 `Top`——**三条全红**。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8，
stale_engines 全部不落后，**unread_theme_fields 132 → 131**。
门：5615 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 233 次：一个 FAB 有四种大小，而这个端口只会问其中一种

`ResolvedFloatingActionButton::of` 无论按钮是哪一种，都只读
`size_constraints`。于是 `smallSizeConstraints`、`largeSizeConstraints`、
`extendedSizeConstraints` 三个字段到不了任何地方，扩展形态的
`extendedIconLabelSpacing`、`extendedPadding`、`extendedTextStyle` 也一样。

**widget 也说不出来**：它带的是 `is_extended: bool`，这个类型**根本表达不了
"小"和"大"**。上游的 `_FloatingActionButtonType` 有四个成员，`build` 按它分支
——所以要带的是"种类"而不是一个布尔。现在有了
`FloatingActionButtonKind`，以及 `small()` / `large()` 两个构造器。

**扩展形态只固定高度。**上游的 `extendedSizeConstraints` 是
`BoxConstraints.tightFor(height: 56.0)`——**宽度不定**，因为那正是这个形态的
意义：固定宽度会截断或撑开标签。

**内边距取决于有没有图标。**上游 M3 的默认是
`EdgeInsetsDirectional.only(start: hasChild ? 16 : 20, end: 20)`：旁边没有
图标的标签两端要一样的余量，有图标的不用。所以 widget 也补上了 `has_icon`。

测试给四种大小四个**互不相同**的数——一行读了邻居的字段，就会答出一个不属于
它的尺寸。四条承重规则逐条强制改错：**四条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 131 → 125**。
门：5620 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 234 次：chip 的七个字段，其中两个是为一个 widget 存在的

`ResolvedChip` 带的是填充、边、内边距三样。`checkmarkColor`、
`deleteIconColor`、`selectedShadowColor`、`avatarBoxConstraints`、
`deleteIconBoxConstraints` 五个到不了任何地方。

**另外两个是另一回事。**`secondarySelectedColor` 与 `secondaryLabelStyle` 是
上游为 **`ChoiceChip` 一个 widget** 准备的字段——

```dart
labelStyle: labelStyle ?? (selected ? chipTheme.secondaryLabelStyle : null),
selectedColor: selectedColor ?? chipTheme.secondarySelectedColor,
```

它们到不了，是因为**那个 widget 没有自己的解析步骤**。现在有了
`ResolvedChip::of_choice`。这两个字段和 `selectedColor` 分开，是因为筛选型
chip 的选中是一个开关，而选择型 chip 的选中是一次挑选——两者想长得不一样。

**两处 `None` 是答案，不是缺口，测试专门说了。**上游 M3 的 `checkmarkColor`
默认就是 null，意思是"跟着标签的颜色走"而不是它自己有一个颜色；两个
`BoxConstraints` 上游也没有默认——布局回落到"内容自身大小的正方形"，那是一条
布局规则而不是一个主题值。

**删除图标的链要穿过图标主题。**上游是
`widget ?? chipTheme ?? theme.chipTheme ?? widget.iconTheme?.color ??
chipTheme.iconTheme?.color ?? defaults`——所以只设了图标颜色的主题，删除叉也
会跟着变色。

五条承重规则逐条强制改错：**五条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 125 → 118**。
门：5625 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 235 次：有一族**不该接**，尺子学会了不报它

`ButtonBarThemeData` 的六个字段一直挂在队列里，像是六个缺陷。它们不是：上游把
`ButtonBar` 和它的主题都标了
`@Deprecated("Use OverflowBar instead")`，这个端口按那句话把 `ButtonBar` 映射
到 `OverflowBar`，而 `OverflowBar` 从不查主题。给它们接线，等于为一个不存在的
widget 造一个解析器——这一点该类型自己的文档早就写明了。

`unwired.py` 早有这条规则，而且是**从上游的 `@Deprecated` 读出来的，不是从
谁写过的一张名单**。这把尺子现在也这样做，并且把跳过的那一族单独列出来，
而不是悄悄从分母里减掉。**118 → 112**，减掉的是不该接的。

### 接上 `AppBarThemeData` 的四条

`ResolvedAppBar` 带的是背景、前景、高度、居中规则、标题间距。
`scrolledUnderElevation`、`actionsIconTheme`、`leadingWidth`、
`toolbarTextStyle` 四个到不了。

三处上游的细节值得记：

- **"被滚过"是另一个数**。Material 3 会把栏从它遮住的内容上抬起来，而不是
  一直平贴，所以静止和被滚过是两个高度（M3 静止是 0，被滚过是 3）。
- **动作图标先回落到栏自己的图标主题**。上游的链是
  `actionsIconTheme ?? theme.actionsIconTheme ?? iconTheme ?? theme.iconTheme
  ?? defaults`——只设了 `iconTheme` 的主题，尾部图标也会跟着变。
- **前导槽默认是方的**。上游 `_kLeadingWidth = kToolbarHeight`，注释写着
  "So the leading button is square"。测试查的是这个**关系**而不是字面的 56。

**一处第一轮读绿。**`toolbar_text_style` 的回落被整个去掉，套件没反应——我的
测试只问了"主题给的值解析成什么"，没问"什么都不给时回落到哪"。补了一条。

四条承重规则最终逐条强制改错：**四条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 118 → 107**（其中 6 条是
剔除废弃族，5 条是真接上的）。
门：5630 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 236 次：尺子第一次**多报**，以及七根小线

前三次修尺子改的都是它**少看**的地方。这次是反过来的一次：
`SearchBarThemeData::hint_style` 明明被读了，读它的是

```rust
hint_style: pick!(hint_style)
```

——一个宏。展开之后才有那次读取，源码里从来没写过那个点号。尺子加上了
`!(field)` 这一种形状，并把这类写进文档：**前两次错都是少报，这次是多报。**
全队列里只有这一处。**107 → 106。**

### 七根线

| 主题 | 字段 | 上游的最后一级 |
| --- | --- | --- |
| `ListTileThemeData` | 三个文字样式 | `bodyLarge` / `bodyMedium` / `labelSmall` |
| `ProgressIndicatorThemeData` | `strokeCap`、`circularTrackPadding` | 无 |
| `ScrollbarThemeData` | `trackBorderColor` | 随明暗而变 |
| `TooltipThemeData` | `exitDuration` | 100ms |

**三个文字样式是三个不同的角色**——`ResolvedListTile` 一个样式也没带。测试除了
查每个字段都到位，还查了**三个默认互不相同**：三个样式变成一个样式，是不会被
任何单个数字发现的。（其中 `title_text_style` 尺子没报，因为这个名字 crate
里别处正好也出现——又一次它只会少报的例子。）

**`stroke_cap` 的 `None` 是答案。**上游自己的默认不是一个值：spinner 和线性
轨道是 round，而 Material 3 那条带缺口的线性条是 butt。所以"没有值"的意思是
"各画各的"，不是"方头"。

**轨道边线随明暗而变**：亮色主题取墨色的十分之一透明度，暗色取四分之一——
在白底上读作"淡"的那个透明度，在黑底上会直接消失。

**tooltip 离开得比停留快**：`_defaultExitDuration` 是 100ms，而 `showDuration`
是 1500。指针滑开和读者读完不是同一件事，不给同样的宽限。

四条承重规则逐条强制改错：**四条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 107 → 99**。
门：5634 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 237 次：同一个洞，第四次踩到

第 223 次发现三个筛子共用了一句已经过期的假设——"测试模块都在文件末尾，所以
第一个 `#[cfg(test)]` 就够了"。那次修了三个，并写了 `tools/rust_source.py`
去数括号。

**这把尺子是那之后写的，又把同一个假设写了一遍。**`component_themes.rs` 第一个
测试模块在 8377 行，而它后面还有九万多字符的产品代码——所有行号大于它的
`XTheme::of` 全部看不见。后果是十二个 `ThemeData` 的组件主题槽被报成"没人
读"，而它们正被上游要求的那次回落读着（`SearchBarTheme::of` 找不到继承的
数据时，就落到 `ThemeData::of(context).search_bar_theme`）。

先用一个只读的普查确认过：**48 个包装器，48 个都有回落，一个不缺。**队列错了
不是端口错了。改用 `rust_source::test_spans` 之后：**99 → 83**。

一句已经被改正过的假设，在一个新工具里重新出现——这说明修正应该落在**共用的
那个函数**里，而不只是落在当时那三个调用点上。`rust_source.py` 本来就是为此
写的；这次是它没有被用上。

### 顺手接上 `ThemeData::card_color`

卡片停在了组件主题自己的 surface 上，所以上游的最后一级缺了——而那一级
**不是一个颜色**：`_CardDefaultsM3` 答 `surfaceContainerLow`，
`_CardDefaultsM2` 答 `Theme.of(context).cardColor`。Material 2 的应用因此
根本没法给卡片上色。

测试读的是**卡片自己那层填充**：卡片先画阴影（同样大小的圆角矩形，半透明
黑），再画自己的表面盖上去——所以取"最后一个非描边的填充"而不是"最大的那个"，
按面积取会撞上平局。两条断言互为对照（M2 取到 77，M3 取不到 88），单独一条
都不足以说明那一级在按版本分支。

两条承重规则强制改错：**两条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 99 → 82**（16 条是尺子看见了
本来就接着的线，1 条是真接上的）。
门：5636 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 238 次：两个默认取决于解析器看不到的东西

`ResolvedTabBar` 带的是颜色、分隔线、指示器尺寸、标签内边距和两个标签样式。
`indicator`、`tabAlignment`、`textScaler`、`indicatorAnimation` 四个到不了。

**其中两个的默认取决于"栏自己知道、主题不知道"的东西**，所以解析器必须被
告知——这是这一批和前几批不同的地方：

| 字段 | 上游的默认 |
| --- | --- |
| `tabAlignment` | `isScrollable ? (M3 ? startOffset : start) : fill` |
| `indicatorAnimation` | `indicatorSize == label ? elastic : linear` |

第一条是**一个字段三个答案**：不滚动的栏铺满；滚动的栏靠前，而 Material 3
比 Material 2 多一个起始偏移。测试把四种组合都查了。

第二条的理由值得写下来：**指示器和标签一样宽时是"弹性"，和整个 tab 一样宽时
是"线性"**——弹性的手感读起来像下划线在够向下一个词，而那只在它是词形的时候
才说得通。

**另外两个没有默认可编。**`indicator` 为 null 的意思是"用颜色和粗细画那条
下划线"，`textScaler` 为 null 的意思是"别动环境里的那个"——上游把它直接传进
`MediaQuery.copyWith` 就是这个意思。两处 `None` 都有测试。

四条承重规则逐条强制改错：**四条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 82 → 78**。
门：5639 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 239 次：尺子第五、六次改口径，一次太宽一次太窄

`ColorScheme::on_secondary` 被报成"没人读"。它其实是三个角色的最后一级——
`on_secondary_container()`、`on_secondary_fixed()`、`on_tertiary()` 都落到它
——而那三个访问器住在 `impl ColorScheme` 里，被尺子**整块**排除了。排除的本意
是构造器（`with_x` 会写 `self.x = x`，否则算成自读），却把访问器一起带走了。

**改窄之后又太窄了。**只排除 `lerp`、`copy_with`、`with_*` 的话，
`TextTheme::merge` 和 `TextTheme::apply_color` 会把十五个角色名全提一遍——
那是搬运字段，不是回答。读数从 78 掉到 64，掉得没有道理。**我核对了一条才
发现**，没有直接采信。

最后的口径不按"这个方法住在哪一块"，按**它做什么**：

> 拿走这个类型、交回同一个类型的方法，是在搬运字段；交回一个值的方法，是在
> 用它回答。

所以排除 `lerp` / `copy_with` / `merge` / `apply*` / `with_*`，访问器留下。
**78 → 76。**

### 七根线，外加一根顺手发现的

| 主题 | 字段 |
| --- | --- |
| `DataTableThemeData` | 两个行色、两个光标（都按状态解析） |
| `DrawerThemeData` | `endShape` —— 以及 `shape`，**它也从来没被解析过** |
| `BottomSheetThemeData` | `dragHandleSize` |
| `ExpansionTileThemeData` | `expansionAnimationStyle` |

**抽屉那两个是两个形状，不是一个镜像。**上游把圆角放在**朝向页面的那一侧**，
对起始抽屉是尾侧、对结尾抽屉是首侧，`Drawer.build` 按 `isDrawerStart` 挑。
写测试时才发现 `ResolvedDrawer` 连前导那个 `shape` 都没有——报表里只有
`end_shape`，因为 `shape` 这个名字 crate 里别处也说。两个一起补了。

**动画样式整个带过去**，不拆开：上游把三部分分开问，各有各的回落，而
`reverseCurve` **一个回落都没有**——所以只写了时长的样式仍然保留默认曲线。

五条承重规则逐条强制改错：**五条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 78 → 69**。
门：5643 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 240 次：一个写死的 `true`，和一条因此才通过的测试

`ThemeData::apply_elevation_overlay_color` 到不了任何地方。原因很具体：

```rust
crate::elevation_overlay::ElevationOverlay::apply_overlay(
    self.color,
    self.elevation,
    true,          // <- 这个参数就是 applyElevationOverlayColor
    is_dark,
    ...
```

第三个参数被写死成 `true`。上游 `applyOverlay` 查三件事——高度大于零、**这个
标志**、以及暗色——而这里第二件永远成立。于是想让暗色表面保持平坦的
Material 2 应用被完全忽略。

**改对之后，一条既有测试红了。**它是这样写的：解析一个**亮色** M2 主题下的
栏，然后分别用 `is_dark: false` 和 `is_dark: true` 去问，断言两者不同。

这条测试此前能通过，**正是因为那个 `true` 是写死的**。上游把两者绑在一起：
`theme.applyElevationOverlayColor && theme.brightness == dark`，而标志的默认
就是主题自己的明暗（`apply_elevation_overlay_color: is_dark`）。**一个亮色
主题自称在暗处，是上游到不了的状态。**

所以改的不是断言的数值，而是它的前提：暗色那一问现在在**暗色主题下**解析。
理由写在测试里。

新加一条测试直接盯住标志本身：同一个暗色 M2 主题，标志开着叠加、关掉不叠加。
两条承重规则强制改错——把 `true` 写回去、让解析器不看主题——**两条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 69 → 68**。
门：5644 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 241 次：两个颜色互相推导，缺哪个就从另一个的明暗推

`CircleAvatar` 从组件 `Theme` 取一个 `surface_variant` 就结束了，从不问
`ThemeData`。于是三个字段到不了：`primaryColorLight`、`primaryColorDark`、
`primaryTextTheme`。

上游 `CircleAvatar.build` 的链有意思的地方是**互推**：

```
foreground = 自己的 ?? (M3 ? onPrimaryContainer : 无)
background = 自己的 ?? (M3 ? primaryContainer  : 无)

若 background 仍然没有：
    background = 前景是暗的 ? primaryColorLight : primaryColorDark
否则若 foreground 仍然没有：
    foreground = 背景是暗的 ? primaryColorLight : primaryColorDark
```

所以两者**总是互相读得清**。**只有一支会触发**，这是它不会绕圈的原因——这一点
写在文档里，也各有一条测试。

Material 3 两个都从容器对里取，一个 primary 都不问；单独一条测试盯住这点，
不然"M3 也去摸 primary"这种错不会有人发现。

**排版也分版本**：M3 取普通排版的 `titleMedium`，M2 取**主排版**的——那是给
"读在主色表面上"准备的字号，而头像正是那种表面。

**没有把前景色留成半根线。**端口没有 `DefaultTextStyle`，前景色推不到任意子
节点。所以给头像加了 `label`：上游最常见的写法就是 `CircleAvatar(child:
Text('AB'))`，这就是那个情形，按解析出的前景色和排版画出来——而不是算出一个
颜色然后没人用。

四条承重规则逐条强制改错：**四条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 68 → 65**。
门：5648 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 242 次：默认那一种材质用的颜色，从来没被读过

上游 `Material.build` 的开头是一个 switch：

```dart
backgroundColor = widget.color ?? switch (widget.type) {
  MaterialType.canvas => theme.canvasColor,
  MaterialType.card   => theme.cardColor,
  button | circle | transparency => null,
};
```

这个端口把 `Canvas` 映到组件 `Theme` 的 `background`、`Card` 映到它的
`surface`，两个 `ThemeData` 字段一个都不问。`canvas_color` 因此到不了——
**而 `Canvas` 是默认的材质类型**，所以这不是一个角落，是绝大多数材质本该取的
那个颜色。

测试给两个字段两个**互不相同**的数：一个把两支都写成同一个字段的 switch，会
答出一个不属于它的颜色。另外单独查了"什么都不说的材质取画布色"，因为默认值
本身也是一条会被写错的规则。

两条承重规则强制改错：**两条全红。**

顺带把三条既有测试的前提改了——它们拿组件 `Theme` 去问 `effective_color`，
而那个方法现在要 `ThemeData`。改的是它们问谁，断言的意思没有变。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 65 → 64**。
门：5649 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

### 第 243 次：最后一级不属于主题，而属于按钮

`ButtonStyle::icon_alignment` 与 `animation_duration` 到不了任何地方。
`ResolvedButton` 带的是背景、前景、边、内边距、最小尺寸五样，而上游
`ButtonStyleButton.build` 用**同一个** `effectiveValue` 走法解析这两个——
widget 的样式、主题的样式、默认——再交给它构建的按钮。

**这两个的最后一级和前面几批不一样：它属于按钮而不是主题。**
`IconAlignment.start` 和 `kThemeChangeDuration` 都是 widget 自己的默认，所以
它们和其余字段一样通过 `defaults` 参数进来，而不是在解析器里写一个常量。
两条测试分开查了这两级——主题给了就用主题的，主题不给就留按钮的。

`ANIMATION_DURATION` 那个常量单独断言了一次等于 200ms。上游的
`kThemeChangeDuration` 是个具体数字，而一个"差不多的时长"不会被别的断言发现。

两条承重规则强制改错：**两条全红。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 64 → 62**。
门：5651 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**剩下的 62 条全部集中在四块**：`DatePickerThemeData` 26、
`TimePickerThemeData` 15、`TextTheme` 10、`SliderThemeData` 9。它们不是"少接
一根线"——各自要等对应的绘制器或 widget 先长出来（日期/时间选择器的绘制路径、
用排版角色的那些 widget、基于形状的滑块绘制器）。那是工程，不是接线。

### 第 244 次：滑块只会从 0 走到 1，所以刻度没有东西可标

上一轮结尾说剩下四块要等 widget 长出来。这是其中一块的第一步。

端口的 `Slider` 只有一个值和一个宽度：没有 `min`、没有 `max`、没有
`divisions`。两个后果，而尺子只看得见第二个。

**第一个是明面上的**：有真实范围的调用方（音量 0 到 11、年份 1900 到 2000）
得在两头手工换算，而 `Slider::new` 会把值裁剪进 0..1——**把它悄悄毁掉**。

**第二个才是 `SliderThemeData::tick_mark_shape` 到不了的原因**：刻度标记标的
是**分度**，而这里一个分度都没有，主题那个字段无从回答。

上游的算术（`_SliderState`）：

```
_unlerp(value)  = (value - min) / (max - min)
_lerp(f)        = min + f * (max - min)
_discretize(f)  = (f * divisions).round() / divisions
_convert(value) = divisions == null ? _unlerp : _discretize(_unlerp)
```

**裁剪换成了断言。**上游不裁剪构造参数，它断言 `value >= min && value <= max`。
裁剪是三种做法里最差的一种：往 0..1 的滑块里传 5 会拿回 1 而且毫无怨言，
而一旦有了范围，0..10 的滑块会把这个完全合法的 5 毁在入口。改成这个 crate
自己的 `validate()` 惯例。

**分度在离开轨道之前生效**——`_discretize` 作用在比例上而不是值上，所以吸附
到的是分度，不是调用方单位里的整数。测试用 0..10、四分度：0.3 吸到 2.5，
0.4 吸到 5.0。

**四个分度是五个标记**，两端都算。

六条承重规则逐条强制改错。其中"轨道按值画而不是按比例画"**第一轮读绿**——
补了一条读回填充宽度的测试：0..10 的滑块在 5 处填 100 像素，而按值画的版本
会去要一千像素、被裁剪成整条轨道。之后六条全红。

**下一步**：`label` 与数值指示器（一个浮层），那会让
`value_indicator_shape`、`show_value_indicator`、`value_indicator_text_style`
三个字段有东西可答。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 62（不变——刻度形状还要等绘制
那一步）。
门：5658 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

## 第 245 轮：刻度形状终于被人调用了

`RoundSliderTickMarkShape::paint` 移植得很早，也有自己的测试——它知道拇指
之后的标记是暗的、之前的是亮的，也知道"之后"是哪一侧要看阅读方向。缺的从来
不是它，而是**没有任何 widget 请它画过**。上一轮给了滑块分度，于是有了标记
可画。

`SliderTickMarks` 是一个 `CustomPainter`，按 `tick_fractions()` 逐个请形状
落笔，画在轨道之上、拇指之下——上游的顺序是轨道、刻度、涂层、拇指，所以被
拇指压住的标记是被盖掉，而不是画在拇指上面。

**第一次画出来是空的。** 形状从 `SliderThemeData` 上直接读四个刻度颜色，而
`SliderTheme::of` 答出来的原始主题里这四个全是 `None`，形状就一笔不画地返回
了。上游的 `_SliderState.build` 从不把原始数据交给形状：它先把整张默认表
`copyWith` 进去，再把**已解析**的 `SliderThemeData` 往下传。这一步端口没有，
而在此之前也没人发现，因为在此之前没有任何形状被调用过。

于是 `ResolvedSlider` 多了一个 `shape_theme`：主题说了什么，加上这个端口能
答的默认值。四个刻度颜色都是 `_SliderDefaultsM3` 的同一个 38%——标记是提示，
不是要读的东西——但压在**四种不同的墨色**上，所以一个标记取哪一种，说明的是
它在哪一侧、以及滑块还活着没有。

`ResolvedSlider` 因此不再是 `Copy`。

五条承重规则逐条强制改错：绘制器不挂上去、每个标记都画在最左端、拇指中心钉死
在零、把原始主题而不是解析后的交给形状、默认值不折进去——五条全红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 62 → 61（`tick_mark_shape`
离队；四个刻度颜色本来就不在队里，形状自己读它们）。
门：5660 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

## 第 246 轮：拇指上的气泡

四个数值指示器形状——矩形、圆角矩形、水滴、桨形——早就移植好了，连路径绘制
器和各自的测试都在，`SliderComponentShape::paint_indicator` 也是专门为了让它
们能被调用而写的。**没有任何东西调用过它。** 三个主题字段
（`value_indicator_shape`、`show_value_indicator`、`value_indicator_text_style`）
在自己的文书之外一个字都没被提过，因为滑块没有 `label`、没有"何时该显示"的
规则、`ResolvedSlider` 也一个字段都没有。

**上游把这个判断拆成两条，而且它们不一致。** `_buildValueIndicator` 决定指示
器是不是被建出来（否则是一个缩成零的盒子），`shouldShowValueIndicatorWhenDragged`
决定建出来的那个在拖动时是否可见。`AlwaysVisible` 正是证明这是两条规则的那个
变体：它对第一条答是，对第二条答**否**——因为它本来就显示着。把两条折成一条，
`AlwaysVisible` 和 `OnDrag` 就变成同一个变体了。测试因此是一张表：六个变体 ×
（连续/分度）×（静止/拖动），二十四格。

拖动状态是调用方的，跟的是 `Button::with_pressed` 而不是上游——上游放在
`_SliderState._dragging` 里，而这个端口的每个 widget 都把瞬时状态交给持有它的
人。这个差别就是 `with_dragging` 存在的全部理由，注释里直说。

**没有文字就没有气泡**，这一条上游没有：它会给 null 的 label 排版并画出一个空
气泡。空气泡比没有更糟，所以这里先要词。

`with_tick_marks` 变成了 `over`：两个绘制器挂上去的方式一模一样，而且都不是
总在——刻度要分度，气泡要标签加拖动。一个函数，于是"怎么挂"的修正是对两者的
修正。测试里三处 `ResolvedSlider` 字面量收成了 `plain_slider()`：光这一轮解析
器就多了三个字段，三份字面量就是三次编辑和三次写出不同滑块的机会。

十条承重规则逐条强制改错，全红。其中两次是**我自己错了而不是代码错了**：
第一版的气泡测试问的是一个**连续**滑块要不要显示，而默认就是
`OnlyForDiscrete`——测试踩了它自己那一轮刚写下的规则；另一次把"气泡填充不设"
写成了等价改写（`None.or(x).or(Some(y))`），读绿，重写后才红。顺手把气泡填充
本身也纳入观察：`inverseSurface` 的底配 `onInverseSurface` 的字，两者是一起
动的，只盯字色看不见一个已经不再被画出来的气泡。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 61 → 57。
门：5663 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**下一步**：`SliderThemeData` 队列里剩下的五个（`range_track_shape`、
`range_tick_mark_shape`、`range_value_indicator_shape`、`min_thumb_separation`、
`thumb_selector`）全部在等 `RangeSlider`，那个 widget 这个端口还没有。

## 第 247 轮：一个可以被拖着穿过同伴的滑块

`RangeSlider::values_with` 换掉被选中的那一侧，然后在自己的文档注释里说：
"上游在下游 assert `newValues.start <= newValues.end`，所以一个会穿过同伴的
拇指是调用方的错，而不是这里要修的东西。"

**这句话是错的**，而且这个错是承重的。上游在 `_handleDragUpdate` 里就修了：

    Thumb.start => RangeValues(
      math.min(currentDragValue, currentValues.end - _minThumbSeparationValue),
      currentValues.end),
    Thumb.end => RangeValues(
      currentValues.start,
      math.max(currentDragValue, currentValues.start + _minThumbSeparationValue)),

而这两个 `math.min` / `math.max` 是 `SliderThemeData.minThumbSeparation`
**唯一的读者**——这正是那个字段在本端口里除了自己的文书之外一个字都没被提过的
原因。端口里还有一条名叫 `crossing_is_not_repaired_here` 的测试，把这个缺陷
断言成了规则：`assert!(start > end)`。测试改名为
`a_thumb_dragged_past_its_partner_stops_at_it`，并把旧名字和旧理由写进注释，
因为下一个人会想知道它为什么翻面。

`_minThumbSeparationValue` 本身有两处值得记：主题的字段是**像素**而值是**比例**，
所以要除以轨道宽度（同样的八像素，在四十像素的轨道上是五分之一，在八百像素的
轨道上是四十分之一）；而在**分度滑块上它是零**，无论主题说什么——分度已经把两个
拇指隔开了，再加一段间隔会让拇指够不到它本可以占据的位置。

`minThumbSeparation` 的默认值来自上游**专门为区间滑块另开的一对表**
（`_RangeSliderDefaultsM2` / `_RangeSliderDefaultsM3`），而这两张表恰好就在这一
个字段上不一致：Material 2 是八像素，Material 3 是零——M3 允许两个拇指贴在一起。

`thumbSelector` 是另一半：主题对 `_defaultRangeThumbSelector` 的替换。类型和
默认实现两边都早就移植好了，**没有任何调用方在两者之间做过选择**。

七条承重规则逐条强制改错，全红。第六条第一次又被我写成了空操作
（`Some(_selector) if false => unreachable!()`，一个永不匹配的守卫），读绿，
改成"主题的选择器永不被查询"之后才红。这已经是连续两轮同一种自欺了。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 57 → 55。
门：5668 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**下一步**：`SliderThemeData` 队列只剩三个 `range_*` 形状字段，它们要等一个会
画的区间滑块 widget——`RangeSlider` 目前是一座纯逻辑的孤岛，没有 `build`，
没有任何东西构造它。

## 第 248 轮：一座逻辑孤岛终于画出了第一个像素

`RangeSlider` 知道一次触摸指的是哪个拇指、一次拖拽落在哪里、构造函数该拒绝
什么——**而没有任何东西构造过它**。它没有 `build`，从未往画布上放过一个像素。
这就是 `SliderThemeData` 未读队列里最后三条（`range_track_shape`、
`range_tick_mark_shape`、`range_value_indicator_shape`）的全部原因。

三个形状连同绘制器都早就在，而且各自知道单值滑块不需要知道的事：
`RangeSliderTrackShape::paint` 知道区间轨道的活动段在**两个拇指之间**，无论
哪个在左；`RangeSliderTickMarkShape::paint` 拿到的是**两个**拇指中心，所以
一个落在区间内的刻度可以和区间外的取不同颜色；`RangeSliderThumbShape::paint`
知道两个拇指重合时要画一圈环，免得一个折叠的区间看起来像一个单拇指。

`ResolvedRangeSlider` 是独立的一个类型，因为**上游为区间滑块另开了一对默认表**，
而两对表互不相同：单值滑块 M3 的轨道是 `GappedSliderTrackShape`，区间滑块的是
`GappedRangeSliderTrackShape`——另一个类，活动段的**两端各留一个缺口**而不是
一个。

**这一轮的教训全在测试里，而且是一条一条撞出来的：**

- 按半径数圆，一个有两个拇指的滑块数出**八个**——M2 圆拇指把三层高程阴影也画
  成了圆，同心、半径几乎一样。
- 改成按填充色数，M3 又数出两个拇指——而 M3 根本不画圆拇指（handle 是圆角
  矩形）。多出来的是**gapped 轨道自己在两端画的停止指示点**，小圆，同样的
  `primary`。两个条件缺一不可，注释里两条都写了。
- 最要紧的一条：`assert_ne!(两张表画出来的东西)` 是**空的**。把两张表的轨道
  形状改成同一个，这条断言照样绿——因为拇指形状还不一样，够它满足了。而两种
  轨道都画三条路径，只有数字不同，那更不是测试。改成对**解析结果本身**逐个
  形状断言，四条各自可证伪。

十条承重规则逐条强制改错，全红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 55 → 52——**`SliderThemeData`
的队列清空了**。

门：5674 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**下一步**：这个 widget 还只是画。它没有手势（`RangeSlider::select_thumb_under`
和 `values_with` 都还没有调用方）、没有标签、没有数值指示器——
`range_value_indicator_shape` 现在被解析了但还没被画。队列里下一批最大的是
`DatePickerThemeData` 26 条与 `TimePickerThemeData` 15 条。

## 第 249 轮：一个装饰好的输入框，字没有颜色

`InputDecoration` 把整个结构都建好了——哪个槽放什么、标签什么时候让位、五个
边框里哪一个在用——`ResolvedInputBorder` 也答完了那些**线**。没有任何东西答
那些**字**。`InputDecorationThemeData` 上的五个样式字段（`hint_style`、
`label_style`、`floating_label_style`、`helper_style`、`error_style`）全是裸
的 `Option<TextStyle>`，没设的那个就一路落空到什么都没有。

上游的两张默认表把五个都给成 `WidgetStateTextStyle`，而它们**是整个 material
库里 `ThemeData.hintColor` 仅有的读者**（加上 `dropdown.dart` 用它做同一件
事）。这就是 `hint_color` 在本端口除自己的文书外一个字都没被提过的原因，
`TextTheme::body_small` 同理——helper 与 error 两行正是 `bodySmall` 的用武之地。

**这一轮记下四件上游做了而端口原本无处表达的事：**

1. **Material 2 下，禁用字段的 helper 与 error 变成透明**，不是"很淡"。透明是
   在**不改变布局**的前提下把那一行藏起来——字段禁用时高度不变。Material 3 改
   为淡到 38% 并允许被读。
2. **Material 3 的 error 没有 disabled 分支**，是两张表里唯一没有的。一个不能
   被编辑的字段上，抱怨依然是红的：读者仍然需要知道它为什么被拒。
3. **Material 2 下只有*浮动*标签跟随焦点与错误**，行内标签无论字段在做什么都是
   `hintColor`。这正是 label 与 floating_label 必须是两个槽而不是一个的原因——
   Material 3 的两张表逐字相同，折在一起看起来毫无损失。测试把"M3 下两者在它们
   分支的每一个状态上都相等"也断言了下来，因为那才是折叠看起来无害的理由。
4. **悬停中的错误标签会变软而不是更响**：上游 error 分支三条臂里两条是
   `error`，只有 hovered 那条是 `onErrorContainer`——悬停是"这个字段可以被修好"
   的承诺。而 focused 排在 hovered 前面。

八条承重规则逐条强制改错，全红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 52 → 50（`hint_color` 与
`body_small` 离队）。
门：5680 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**下一步**：`TextTheme` 还剩八个排版角色没有读者，每一个都要等一个用它的
widget（`title_large` 是 M3 标题栏的角色，`headline_small` 是对话框的，
`display_*` 是日期选择器头部的）。`ThemeData::secondary_header_color` 只有一个
读者，上游的 `PaginatedDataTable`，这个端口还没有。

## 第 250 轮：一个自己编标题样式的标题栏

`ResolvedAppBar` 解析了工具栏的文字样式，却**没有标题的那一项**——于是 `AppBar`
用 `theme.title()` 画标题，一个手工拼出来的 `TextStyle`，里面硬写着字重 700。
上游的 `titleLarge` 是 400。一个把标题加粗的标题栏，是在说一件字号表没有说的事。

三处缺口，同在上游那两行里：

1. **`AppBarThemeData::title_text_style` 无处落地。** 主题带着这个字段，没有
   任何解析器读它，调用方设了等于没设。
2. **`TextTheme::title_large` 在整个端口没有读者。** 它是标题栏标题在上游两张
   默认表里的角色（`_AppBarDefaultsM2` 读 `_theme.textTheme.titleLarge`，
   `_AppBarDefaultsM3` 读 `_textTheme.titleLarge`），而这个 widget 本该是去
   要它的那个。
3. **两个默认样式都要过 `copyWith(color: foregroundColor)`。** 角色带来字号、
   字重和字族；**墨色由栏本身带来**，因为前景色是栏的属性而不是字号表的。端口
   解析 `toolbar_text_style` 时漏了这一步，所以一个设了前景色的栏，非标题文字
   画错了颜色。而这一步只加在**默认值**上：主题明写的样式已经定了自己的墨色。

**把 widget 从手工样式换到解析样式，一条测试都没红——把它换回去也一样。** 这是
这一轮的另一半发现：解析器有自己的测试，但**没有任何东西看着 widget 有没有去问**。
补了一条画面级测试，用前景色让它可见（手工样式的墨色来自更老的那个 `Theme`，
所以一个被要求用某种颜色的栏画成了另一种）。之后第五条改错才红。

五条承重规则逐条强制改错，全红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 50 → 49（`title_large` 离队）。
门：5684 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**下一步**：`TextTheme` 还剩七个角色没有读者，都在等用它们的 widget——
`headline_small` 是对话框标题的角色，`title_small` 是列表小标题的，
`display_*` 是日期选择器头部的。对话框是其中最近的一个。

## 第 251 轮：一条断言"上游也什么都不给"的测试

`ResolvedDialog` 把五个字段原样透传——`title_text_style`、
`content_text_style`、`icon_color`、`shadow_color`、`surface_tint_color`——
而一条名叫 `nothing_is_invented_for_the_fields_upstream_leaves_null` 的测试
断言其中三个保持 `None`，理由写着"在这里编一个颜色出来，widget 就分不清它和
真答案了"。

**上游只把这四个里的一个留空。** `_DialogDefaultsM3` 给了其余三个，而
`_DialogDefaultsM2` 里有两个答得不一样：

    M3                              M2
    shadowColor      transparent    theme.shadowColor
    surfaceTintColor transparent    （没有这个概念）
    titleTextStyle   headlineSmall  titleLarge
    contentTextStyle bodyMedium     titleMedium
    iconColor        secondary      iconTheme.color

真正留空的是 `barrierColor`，而且理由值得留着：**遮罩不属于对话框**，它属于
`showDialog`——把对话框放上屏幕的那个东西——默认值也在那里。测试改名为
`only_the_barrier_is_left_for_someone_else_to_answer`，旧名字和旧理由写进注释。

**两个 transparent 是这里最有意思的一对。** M3 的对话框是 elevation 6，
**既不投影也不上色调**：它离页面多高，完全由 `surfaceContainerHigh` 这一种容器
颜色来说。留成 `None`，下游任何人读到的都是"没人说过"，于是画出上游特意关掉的
那道阴影。

`iconColor` 两张表**连答案从哪来都不一样**：M3 直接点名一个配色角色，M2 取
周围图标主题正在用的颜色——所以 M2 对话框的图标跟身边的图标一致，而不是自成
一格。

`headlineSmall` 此前在整个端口没有读者。

这一轮把上一轮的教训**用在了改错之前而不是之后**：解析器有自己的测试，只有
画面级测试才看得住 widget 有没有去问。`Dialog::build` 原本用
`theme.title()` / `theme.muted()`——这个 crate 自己编的样式——根本没调用过
`ResolvedDialog::of`。先补画面级测试，再跑七条改错，第七条（widget 退回自编
样式）当场就红。

七条承重规则逐条强制改错，全红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 49 → 48（`headline_small` 离队）。
门：5688 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**下一步**：`TextTheme` 还剩六个角色没有读者。`headline_medium` 与
`headline_large`、三个 `display_*` 都是日期选择器头部的，`title_small` 是列表
小标题的——`DatePickerThemeData` 那 26 条也在同一个方向上。

## 第 252 轮：标签页的字，字号来自不知何处

`ResolvedTabBar` 算出五种颜色、分隔线高度、内边距、指示器尺寸、对齐方式和
动画——**两个标签样式却原样透传，一个默认值都没有**。上游三张表全都给了答案：

    _TabsDefaultsM2            primaryTextTheme.bodyLarge，两个都是
    _TabsPrimaryDefaultsM3     titleSmall，两个都是
    _TabsSecondaryDefaultsM3   titleSmall，两个都是

三张表一致地把**选中与未选中的标签给成同一个角色**——一个选中的标签是靠颜色和
下划线被认出来的，不是靠换个字号。它们分歧的是**哪个角色**。

`primaryTextTheme` 是画在**主色表面上**的文字用的那套字号表，而 Material 2 的
标签栏正是这种情况：它嵌在应用栏里。Material 3 的不是，所以读普通那套。这也是
`ThemeData::primary_text_theme` 在本端口一直没有读者的原因——上游如今还在用它
的地方本就不多。

而 widget 一个都没问。`TabBar::build` 用 `theme.body_size` 定字号、手写 700 或
500 的字重、从更老的那个 `Theme` 取两种颜色——所以主题指定的标签样式毫无作用，
`ResolvedTabBar` 用**五步**小心算出来的那两种颜色，也不是画上去的那两种。
（那五步值得一提：颜色可以写在 `labelColor` 里，也可以写在**样式内部**，而后者
的优先级更低——上游注释说，把它提上来会是一个没有迁移路径的破坏性改动。）

五条承重规则逐条强制改错，全红。第五条（widget 退回自编样式）第一次读绿——
但这次不是测试的问题：我的改错脚本第一个替换没匹配上，安静地失败了，于是跑的
是没改过的代码。按正确方式重跑后当场红。**"改错读绿"有两种可能，另一种是改错
本身没生效，得先确认文件真的变了。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 48 → 47（`title_small` 离队）。
门：5691 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**下一步**：`TextTheme` 只剩五个角色，全部是日期选择器头部的
（`headline_large`/`headline_medium` 与三个 `display_*`）——和队列里
`DatePickerThemeData` 的 26 条是同一件事。

## 第 253 轮：一份日历读起来是什么样

`ResolvedDatePicker` 答了九个字段——表头两种颜色、副表头一种、今日边框、对话框
与区间选择器各自的高度和圆角——**八个文字样式一路落空到什么都没有**。上游两张
表在其中六条上不一致，而且分歧不是装饰性的：**Material 3 的日历整整比
Material 2 大一级**。

    字段                            M2                       M3
    headerHeadlineStyle            headlineSmall            headlineLarge
    headerHelpStyle                labelSmall               labelLarge
    weekdayStyle                   bodySmall @ onSurface60  bodyLarge @ onSurface
    dayStyle                       bodySmall                bodyLarge
    yearStyle                      bodyLarge                bodyLarge
    toggleButtonTextStyle          titleSmall @ subHeader   titleSmall @ subHeader
    rangePickerHeaderHeadlineStyle headlineSmall            titleLarge
    rangePickerHeaderHelpStyle     labelSmall               titleSmall

其中两条是**一个角色加一层不属于它的颜色**。`weekdayStyle` 的 M2 六成透明度，
是日历上方那行字母比下面的日期安静；M3 去掉这层淡化、改成把日期放大——同一句话
的另一种说法。`toggleButtonTextStyle` 两张表都取副表头的颜色：按钮和它旁边的
字是同一个控件，重新着色副表头会连按钮一起带走。

两条"一致"也各有各的一致法：`yearStyle` 是**取值相同**（一个年份在哪一级都是
年份），`toggleButtonTextStyle` 是**构造方式相同**而取值随副表头浮动。八条里
七条分支、一条不分支，所以那一条也写了断言——否则看起来像漏了。

`TextTheme::headline_large` 此前在整个端口没有读者。

顺手核了一遍剩下的角色在上游有没有读者：`displaySmall` **在整个 framework 里
一个读者都没有**——它是给应用用的，不是给控件用的。这是答案而不是缺口。
`displayLarge`/`displayMedium` 是时间选择器的时分数字，`headlineMedium` 是
中号应用栏的标题和横屏日期选择器的表头。

八条承重规则逐条强制改错，全红。改错脚本这一轮加了一行：**匹配数不为一就报告
"改错未生效"并退出**，而不是安静地失败——上一轮正是那样读出一个假绿。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 47 → 38（一轮九条）。
门：5697 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**下一步**：`DatePickerThemeData` 还剩十八条，分三组——日期与年份的状态颜色
（八条 `WidgetStateProperty`）、三个形状、以及区间选择器自己的那一层表面
（五条）加上区间选中的两条。

## 第 254 轮：日历格子的八种状态颜色

三个槽——普通日期、今天、年份列表里的年份——每个三种颜色，减去今天的浮层
（上游不给它单独一份）。**八个属性一个解析器都没有**：本端口里的一个日期格子
没有选中色、没有禁用色、没有波纹。

解析器做成**逐格子**而不是逐选择器，因为问题本来就是这个形状：一份日历为屏幕上
每个日期各问一次，每次带着不同的状态集。

`background` 与 `overlay` 是 `Option`，因为上游的属性在普通情形下答 null，而
**null 不是一种颜色**：一个未选中的日期**没有**背景，这跟"有一个透明背景"不是
一回事——它身后的东西会透上来，包括画在格子底下的区间选中色，而一层透明矩形会
把那层盖掉。

**两张表真正分歧的只有一处：`todayForegroundColor` 的禁用臂。** M2 淡向普通
墨色（`onSurface` 38%），M3 保持一个淡化的 `primary`。所以一个超出范围的
"今天"在 M3 下**仍然说自己是今天**，在 M2 下就不说了。今天那一列的其余台阶两张
表一致——这才使得那一条是分歧而不是另一套规则，所以一致的部分也写了断言。

浮层的数字是另一处差别：M3 两个分支都用 0.1 / 0.08 / 0.1；M2 给**按下的选中
日期** 0.38——**是两张表里第二大数字的三倍**。一个选中的日期本就顶着 primary，
普通强度的波纹在上面看不见；M3 用另一种办法解决同一个问题：波纹改画
`onPrimary`，数字不动。

年份那三条属性**在 `_DatePickerDefaultsM2` 里根本不存在**——不是给了 null，是
没有覆写。M2 的年份列表拿不到任何东西，这是上游的答案而不是遗漏。

八条承重规则逐条强制改错。第四条读绿，**这次是测试太弱**：我断言的是"M2 压得
比 M3 重"，而 M2 的每一个浮层都比 M3 的重，所以一条丢了 0.38、退回 0.12 的臂
照样满足它。改成断言 **M2 自己的选中分支比它自己的未选中分支重一倍以上**——
这才是那 0.38 独有的事——之后才红。**"比另一张表大"通常不是一个claim，因为整张
表都比它大。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 38 → 30（一轮八条）。
门：5704 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**下一步**：`DatePickerThemeData` 还剩十条——两个格子形状、区间选择器自己那层
表面的六条、区间选中的两条。

## 第 255 轮：格子的形状，和区间选择器

`DatePickerThemeData` 最后十一个字段。

    字段                              M2                        M3
    dayShape                          CircleBorder              同左
    yearShape                         StadiumBorder             同左
    rangePickerShape                  RoundedRectangleBorder()  同左
    rangePickerBackgroundColor        surface                   **不覆写**
    rangePickerShadowColor            transparent               transparent
    rangePickerSurfaceTintColor       transparent               transparent
    rangePickerHeaderBackgroundColor  dark ? surface : primary  transparent
    rangePickerHeaderForegroundColor  dark ? onSurface:onPrimary onSurfaceVariant
    rangeSelectionBackgroundColor     primary @ 12%             secondaryContainer
    rangeSelectionOverlayColor        两个分支                   **一个**

三个形状是在两张默认表的**构造函数**里传进去的，而不是作为 getter 覆写——所以
在上游 grep `get dayShape` 一个都找不到。两张表传的是同一组：日期是圆，年份是
胶囊，全屏的区间选择器是**完全不圆角**的矩形（普通对话框是 28）——一块屏幕没有
角可以圆。

今天**没有自己的形状**：上游是给日期形状加一条 *side* 来画今天的，所以自定义
`dayShape` 会把今天的那圈环一起带走。这一条写成了断言。

四处值得记：

**`rangePickerHeaderBackgroundColor` 是日期选择器里最后一处按*明暗*而不是按
配色角色取色的地方**：暗色主题得 `surface`，亮色得 `primary`，前景跟着走。M3
把表头改成透明、让身后的对话框透上来——和它对普通表头的处理一样。

**`rangePickerBackgroundColor` 在 M3 表里根本没有覆写。** M3 的区间选择器铺满
屏幕，用对话框自己的背景；M2 的是一张卡片，需要自己的一份。

**区间条从"淡化的选中色"变成了"一块自己的表面"**：M2 是 `primary` 12%，M3 是
`secondaryContainer` 满强度。

**`rangeSelectionOverlayColor` 在 M3 下没有选中分支** ——区间里每个格子**本来就
是**选中的，对它分支等于什么都没说。整条区间用一种方式起波纹：`onPrimaryContainer`
压在它自己那层 `secondaryContainer` 上。

十三条承重规则逐条强制改错。第十三条读绿——主题自己指定的浮层没人看着，补了
断言之后才红。**这一轮里凡是"主题说了算"的那一条，都要单独有测试；它和默认表
那条是两个不同的规则。**

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 30 → 20——**`DatePickerThemeData`
的队列清空了**。
门：5711 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**下一步**：队列只剩 `TimePickerThemeData` 十四条、`TextTheme` 四个角色、
`ThemeData::secondary_header_color` 一条。时间选择器那十四条里就有
`displayLarge`/`displayMedium`（时分数字）——两件事是同一件。

## 第 256 轮：时间选择器顶上那个大数字

六个字段，一个解析器都没有。

    字段                            M2                       M3
    hourMinuteColor                selected: primary @       状态浮层 alphaBlend
                                   dark?0.24:0.12            到容器色**之上**
                                   else onSurface @ 0.12
    hourMinuteShape                半径 4                    半径 8
    hourMinuteTextColor            selected ? primary        onPrimaryContainer
                                   : onSurface               / onSurface
    hourMinuteTextStyle            displayMedium，平的        表盘上 displayLarge，
                                                             输入框里 displayMedium
    timeSelectorSeparatorColor     **不存在**                 onSurface
    timeSelectorSeparatorTextStyle **不存在**                 displayLarge

三处值得记：

**M3 的 `hourMinuteColor` 是混合而不是挑选。** 状态浮层压在容器色**之上**而不是
替换它，而按下那一臂的浮层是**满不透明度的墨色**——所以按住小时框，它会直接从
`primaryContainer` 变成 `onPrimaryContainer`，这是所有这些默认表里最强的一次
状态变化。悬停和聚焦是压在同一底色上的普通 0.08 / 0.1 混合。混合真正买到的是
**每种状态下的结果都是不透明的**：把浮层原样交出去会留下一层半透明，身后的对话框
会从你正在选的那个小时里透出来。

**`hourMinuteTextStyle` 是整个 framework 里唯一按*录入方式*分支的样式。** 表盘
上小时是整块屏幕的主角，用 `displayLarge`；同一个选择器切到打字模式，小时进了
一个要留出边框和标签的输入框，降一级到 `displayMedium`。M2 两种都用
`displayMedium`。

**分隔冒号是 Material 3 才有的概念。** 它那两个字段在 `_TimePickerDefaultsM2`
里根本不存在——M2 的选择器把冒号按 hour/minute 样式画，和旁边的数字一样；M3 给
冒号单独的颜色和样式，好让它比它夹着的数字安静。

M2 的 `hourMinuteColor` 也是最后几处**按明暗取色**的地方之一：暗色主题下选中的
框透明度是亮色的两倍，因为同样 12% 的 `primary` 压在暗色表面上看不出来。

十一条承重规则逐条强制改错。第二条读绿——**我没有断言"混合"到底买到了什么**。
补上"每种状态下 alpha 都是 0xFF"之后才红。另外 `hour_minute_shape` 第一遍没有
离队：我解析的是一个**半径**，而上游那个字段是一整个 `ShapeBorder`——主题可以
把框做成胶囊，而不只是换个圆角。改成读形状本身，并断言了胶囊那种情形。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 20 → 12。
门：5718 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**下一步**：`TimePickerThemeData` 还剩九条——上下午那四条、表盘那四条、
`help_text_style` 一条。`displayLarge`/`displayMedium` 已经离队，剩下的
`TextTheme` 两个角色里，`displaySmall` 上游**根本没有控件读它**（已在第 253 轮
记下），`headlineMedium` 要等中号应用栏。

## 第 257 轮：上下午的切换、表盘，和它们上面那行字

九个字段，一个解析器都没有。

    字段                  M2                          M3
    dayPeriodColor       选中: primary @              选中: tertiaryContainer
                         dark?0.24:0.12              未选中: **透明**
                         未选中: **透明**
    dayPeriodBorderSide  alphaBlend(onSurface38,     outline
                         surface)
    dayPeriodShape       半径 4                       半径 8
    dayPeriodTextColor   选中 ? primary               选中 ? onTertiaryContainer
                         : onSurface @ 0.60          : onSurfaceVariant
    dayPeriodTextStyle   titleMedium + 该颜色         titleMedium + 该颜色
    dialBackgroundColor  onSurface @ dark?0.12:0.08  surfaceContainerHighest
    dialHandColor        primary                     primary
    dialTextColor        选中 ? surface               选中 ? onPrimary
                         : onSurface                 : onSurface
    dialTextStyle        bodyLarge                   bodyLarge
    helpTextStyle        labelSmall，平的              labelMedium @ onSurfaceVariant

**未选中的那一半在两张表里都是透明的**，而上游在每张表里都重复了同一句理由：
未选中的一半应当与它身后的对话框一致，透明做到了这一点"而不必重复一遍，也让
暗色模式下可选的高度叠色仍然可见"。从对话框抄一份颜色过来，会多出一个要一起改
的地方，而且会压在高度叠色**之上**而不是之下。

**形状和边是两个总被合起来用的字段。** 上游是
`(theme.dayPeriodShape ?? defaults).copyWith(side: theme.dayPeriodBorderSide ?? defaults)`
——主题可以只指定圆角或只指定描边，赢的那个形状会带上赢的那条边。M3 的表在自己的
getter 里又写了一遍 `copyWith`，与之重复且无害。

**M2 的描边是混到表面上的，不是留成半透明的。** 切换按钮压在对话框上，一条透光
的描边会把高度叠色放在它身后的东西吸上来。M3 直接点名 `outline`，不需要混合。

**`dialTextColor` 选中时最耐人寻味**：M2 是 `surface`，M3 是 `onPrimary`。指针
是 `primary`，两者都在说"压在指针上的墨色"——M2 只是还没有 `onPrimary` 这个习惯。

`dialHandColor` 与 `dialTextStyle` 两张表完全相同，也写了断言——这张表大部分都
在分支，不写就像是漏了。

顺手改了一处不在队列里的缺陷：`entryModeIconColor` 原本解析成平的 `onSurface`，
那是 M3 的答案。M2 是暗色下满强度、**亮色下六成**——又一处这张表满是的明暗分支。

十三条承重规则逐条强制改错。第十条读绿——**"主题说了算"那条我又没写测试，这是
连续第二轮**。补了一条把两个解析器的每个字段都指定一遍的测试（而且指定在"表答
透明"的未选中状态上，这样只有主题生效才可能返回不透明），之后第十到十三条全红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，**unread_theme_fields 12 → 3**——
`TimePickerThemeData` 队列清空。
门：5728 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**剩下的三条各有各的答案**：`display_small` 上游**整个 framework 没有控件读它**
（第 253 轮已核实，它是给应用用的角色）；`headline_medium` 要等中号应用栏；
`ThemeData::secondary_header_color` 只有 `PaginatedDataTable` 一个读者，这个端口
还没有那个 widget。

## 第 258 轮：一个知道自己是哪一种、却什么都不做的 sliver 应用栏

`SliverAppVariant` 早就移植了，`SliverAppBar` 也带着一个——**而没有任何东西读
过它**。可这个变体正是上游三个构造函数之间的全部差别：
`_SliverAppBarState.build` 就是按它来选收起高度、展开高度和弹性空间的，
`_ScrollUnderFlexibleConfig` 是背后那张按变体分的表。

    字段                    medium                     large
    collapsedHeight        64                         64
    expandedHeight         112                        152
    collapsedTextStyle     titleLarge @ onSurface     titleLarge @ onSurface
    expandedTextStyle      headlineSmall @ onSurface  headlineMedium @ onSurface
    expandedTitlePadding   16, 0, 16, 20              16, 0, 16, 28

**两种变体收起时高度相同、样式也相同。** 让大号栏成为大号的，只是它能展开多远，
以及展开后标题下面多出多少地方。四个数字里有两个相等，不写出来就像是复制粘贴，
所以写了断言。收起时的 `titleLarge` 也正是普通应用栏标题的角色——一个被滚上去的
大号栏，和一个本来就普通的栏是分不出来的。

`TextTheme::headline_medium` 此前在整个端口没有读者，而它的位置就在这里：大号
应用栏展开时的标题。

两条来自 `_ScrollUnderFlexibleSpace.build` 的规则值得带过来：

**栏下面挂了东西，标题底下那圈内边距就没了。** 下面的标签条或搜索框自带地方，
再留二十像素会把标题顶出它所属的那条栏。只有底边归零，左右不动。

**栏自己的前景色盖过表里的墨色。** 链条是
`titleTextStyle ?? appBarTheme.titleTextStyle ?? config.expandedTextStyle?.copyWith(color: foregroundColor ?? ...)`，
所以每张 config 里写的 `apply(color: onSurface)` 是一个下一行就会覆盖掉的值：
在 Material 3 的默认下两者相同，而一旦某条栏指定了前景色就不同了——那正是它要紧
的时候。

还有两处算术值得记：**底部高度加进*两个*范围**（挂了标签条的栏展开时高这么多、
收起时也高这么多，标签条才会一路留在屏幕上——把它放进栏里而不是栏下面就是为了
这个）；而**状态栏的地方只加进收起高度**（它已经通过 `top_padding` 进了
`min_extent`，再加到展开高度上就算了两遍）。

九条承重规则逐条强制改错，全红。

尺子：coverage 2102/0，constants 158/0/0，wire_strings 122/0，unwired 48/0,
unvaried 0，unread_strings 36+16/0，unpainted 0，hollow 67/0，vacuous 8,
stale_engines 全部不落后，unread_theme_fields 3 → 2。
门：5736 + 333 通过；五个输出目录的 rustflutter_engine 与 rust_lib 全部重建。

**`unread_theme_fields` 到此实际见底**：剩下两条各有答案——`display_small` 上游
整个 framework 没有控件读它（第 253 轮核实过，它是给应用用的角色），
`ThemeData::secondary_header_color` 只有 `PaginatedDataTable` 一个读者，那个
widget 这个端口还没有。

**下一步转向别的尺子**：`unwalked` 说 1528 个公开枚举变体里有 219 个从未在任何
测试里被点过名；`depth` 说 672 个类里有一批成员覆盖率极低（`CupertinoLocalizations`
2/46、`RadioListTile` 3/43、`MaterialButton` 4/34、`CheckboxListTile` 5/42）。
