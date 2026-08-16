# 与上游 Flutter 的对齐差距 —— widget 层到 paint 层

范围只有 widget 层到 paint 层:`framework.rs`(元素树)、`render.rs`(全部 25 个
生产用 `RenderBox`)、`painting.rs`、以及 `app.rs` 的帧序。引擎侧、平台通道、
无障碍、Android 宿主**不在这个范围内**——它们的移植记录、验证基线与已知缺口在
`git show fc265dc:PORTING_STATUS.md`。

比对的上游是 `K:\flutter`(`flutter/flutter @ cf97bfbcb9f`)。

**每一条都照上游的实现写,不要照这份描述写。** 描述可能有偏差,
`packages/flutter/lib/src/rendering/*.dart` 没有。锚点给的是符号名而不是行号。

改完 **Windows 和 Android 都要验**。

---

## 一、要做的

### 1. 语义节点没有被祖先的裁剪切掉

滚出视口的子节点照样进语义树,矩形还是它在内容里的绝对位置——没有谁拿视口的裁剪
去截它。查 flutter_gallery 首页可见:窗口客户区 690 宽,轮播里的 `Rally` 报在
x=1318,列表底部的 `Tooltips` 报在 y=3651。Windows 的 UIA 桥原样报出去,成了窗口
外的坐标;Android 那边平台自己把它夹成 `[0,0][0,0]`,于是变成一个能聚焦的零尺寸
节点。两边都不对,只是烂法不同。

上游锚点:`rendering/object.dart` 的 `_SemanticsGeometry.computeChildGeometry`。
它沿着到父节点的那条链累积两个矩形——`describeApproximatePaintClip` 与
`describeSemanticsClip`(视口给的那个按 cacheExtent 放大),然后

- `rect = semanticsClipRect?.intersect(semanticBounds) ?? semanticBounds`,
  即矩形先被语义裁剪切一刀;
- 再与 `paintClipRect` 相交,交出来空、原来的又非空,就是 `hidden`——留在树里但
  打上隐藏标记,读屏跳过;否则 `rect` 换成相交后的那个。

这里缺的是整条链:`RenderBox` 没有 `describe_*_clip` 的对等物,
`semantics.rs` 的走树也没有把裁剪矩形带下去(它只带偏移)。

**做到什么算做完:** 语义走树带一个裁剪矩形往下,`RenderViewport` 与
`RenderClipRect` 各自贡献自己的那一份;节点矩形按它相交;整块落在视口外的节点不
再出现在树里。验收就用上面那两个数——首页 dump 里不该再有超出客户区的矩形,
Android 那边不该再有 `[0,0][0,0]` 的节点。

---

## 二、明确不做的(连理由一起,免得下次被当成待办)

- **`BoxConstraints::biggest()` 在无界轴上返回 `min`。** 上游返回 `infinity`。
  这是**故意的**安全化:它同时是 `RenderDecoratedBox` 无子节点时的尺寸来源,真按
  上游改成无限大,示例和相册会一起变成无限大的盒子。要动的话得先给
  `RenderDecoratedBox` 一个 `computeSizeForNoChild` 的对等物。
- **`RenderOpacity` 全透明时不参与命中。** 上游**不**画这条线——它让看不见的子树
  照样可命中,要挡就在上面压一个 `IgnorePointer`。这里比上游严,不是译错;放开
  会把点击交给正在淡出的东西。代码里已自陈。
- **`TextOverflow::Fade`。** `TextOverflow` 的另外三个(`Clip` / `Ellipsis` /
  `Visible`)都在。`fade` 根本不是文字功能而是绘制功能:上游把文字画进一个
  `saveLayer`,再拿一条透明到不透明的渐变用 `BlendMode.modulate` 乘上去。这个绘制
  层没有混合模式,而框架里没有一处要它。
- **`ListTile` 尾部预留宽度的那个 32 下限。** 上游是
  `math.max(trailingSize.width + gap, 32.0)`,flex 只能预留
  `trailing + spacing`。只有尾部比 gap(16)还窄时两者才不同,而这里的尾部——开关、
  金额、按钮——没有一个窄于 16。
- **`StackFit`**:是补功能,不是对齐,与这个范围无关。

下面这些**得先有 pipeline owner 或者等价物**才谈得上,和"标记一路走到根"是同一笔
账,不适合顺手做:

| 缺的 | 上游在哪 | 后果 |
| --- | --- | --- |
| **dry layout**(`computeDryLayout` / `getDryLayout`) | `box.dart` | Flex 与 Wrap 的交叉轴固有尺寸只能靠子节点的固有值估——见 `RenderWrap::packed` 与 `RenderFlex::intrinsic_cross` 的自陈 |
| **`needsCompositing` / `flushCompositingBits`** | `object.dart`、`binding.dart` | 每个 clip、每个 transform 都是真 layer;上游不需要合成时把裁剪记进 display list。帧序上这里是 `build → layout → paint → semantics`,少的正是 `flushCompositingBits` 这一步 |
| **relayout boundary / pipeline owner / parentUsesSize** | `object.dart` | 标记一路走到根,没有谁存着脏边界、也没有谁能从半路开始一帧 |
| **sliver 协议** | `sliver*.dart` | `RenderViewport` 其实是 `_RenderSingleChildViewport` |
| **`RenderTransform` 逆变换** | `proxy_box.dart` 的 `addWithPaintTransform` | 命中测试与语义矩形都用未变换几何(代码里自陈) |
| **元素层的 top / bottom sync** | `widgets/framework.dart` 的 `updateChildren` | 未加键的列表中段插入一个**不同类型**的子节点,它后面的全体失配、状态丢光;上游靠尾部同步保住 |
| **`GlobalKey` / deactivate-activate** | 同上 | 跨父节点搬迁保不住状态 |
| **`didChangeDependencies` / `dispose` 钩子** | 同上 | 状态靠 Rust 的 `Drop` 收尾(其实够用),但没有"还挂着的时候被通知"这一步 |

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

