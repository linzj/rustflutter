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

