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

## 完全覆盖计划的第一簇(2026-08-17 起,PORTING_PLAN.md 记账)

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

- `TickerFuture` 不是 future(crate 无异步运行时,同 `async.rs` 的账),
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
hasData/hasError)、`async_builder`(poll 形态——crate 无异步运行时,future 的
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

