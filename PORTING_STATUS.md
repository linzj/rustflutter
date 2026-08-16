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
- **`ClipRRect` 的逐角圆角**(上游 `BorderRadius`);现为统一 `f32`。
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

