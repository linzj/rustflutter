# STATUS(dart2rust)

**2026-09-07 压缩(用户裁定:只保留这一个文件)。** 第 2–134 轮叙事、「运行尺子」
的 run429–run471 表、「第二个 goal」的 run472–ws571 表行移出本文——它们一行未删
地留在 git 历史:`git log --oneline -- tools/dart2rust/STATUS.md`;
取某版:`git show <sha>:tools/dart2rust/STATUS.md`(压缩点 = 本文件入库的那一次提交)。
保留节的正文与原文件逐字节一致;新增内容只在本头部、各「章结/校注」段、
〈数字轨迹〉〈撤回与作废〉〈已知欠账〉三节和活账窗口新行。

**2026-09-09 第二次压缩(用户裁定:直接压缩)。** ws749–ws876 的 106 个每轮小节
(2520 行,占全文 79%)移出本文,同样一行未删地留在 git 历史(压缩前 = `927eab67`,
`git show 927eab67:tools/dart2rust/STATUS.md`)。留下的是:每轮标题那一句规则,
进〈活账〉窗口;撤回过的结论,进〈撤回与作废〉;量过的账,进〈已知欠账〉;
两处「章结」和几处普查、模型改动原文保留(它们当初就没写成每轮小节)。
3215 行 → 本文。

**2026-09-09 第三次压缩(用户裁定:需要简写)。** 压缩点 = `dbc6d961`
(`git show dbc6d961:tools/dart2rust/STATUS.md`,1266 行)。移出本文的六项,
一行未删地在 git 里:

1. 活账窗口 51 行 → **40** 行。
2. 四个 `## wsN` 形态的小节归位:三个去掉轮次(ws880 的验法、ws881 的拆法、
   ws884 的隐式状态,**正文一字未动**),ws894 的 `dart:ffi` 那节整段并进
   〈已知欠账〉。
3. 〈已知欠账〉里两条已解的划掉项删除(分析器前端不再编译、覆盖时收窄类型实参)。
4. 黄金层那一簇(ws886–ws890b)的叙事移出,只留三个根因、7 个缺陷、顺序、
   和那条验收标准(**crate 编过 + 146 个 test 跑绿,不是错误数降低**)。
5. ws864 双重装箱的叙事移出,在〈已知欠账〉留一条(五个候选全部排除,以及
   trace 照出来的那个反向事实)。
6. 两处被后来的读数取代的章结(ws824/run825、ws828 身份普查)移出——它们的
   结论已经分别在〈数字轨迹〉〈撤回与作废〉〈已知欠账〉里;〈下一步〉与
   〈当前队头〉的正文自 2026-09-05 起就对不上(两处校注早就这么写),正文移出,
   校注留下。

1266 行 → 本文。

**维护规约(防再胖):**〈活账〉表只留最近约 40 行;每批提交时把滑出窗口的行直接
删掉(git 有)。「撤回与作废」和「数字轨迹」是考古结论的留置处,只增不删。
**每轮的记录写成〈活账〉的一行,不写成 `## wsN` 小节**——2026-09-07 到 09-09 之间
文件涨回 6 倍,就是因为规约只管住了表,而记录换成了小节。一条规则的「为什么」
值得留下时,它进〈九条要记住的〉〈撤回与作废〉〈已知欠账〉,或另起一个不带轮次的
标题(本文里那几节就是这么来的);其余的,git 有。
**活账的一行就是一行**——ws886 起有的行长到 600 字,是把小节塞进了单元格,
和写成 `## wsN` 是同一件事换了个位置。一行写规则和读数;要展开,去上面那三个
留置处。

## 决定（2026-09-04）：恢复 `Result`，整程序做传播分析

ws60 关掉签名上的 `Result`（`throw` = panic）是因为当时的失败集合是**按类算**的：
`_computeFailing` 只在一个类的方法之间做定点，所以只有 `this` 上的调用看得见
被调方的签名变了；经别的对象、trait 声明、闭包、静态的调用都在调用方报错。
这是分析的范围问题，不是模型的问题——这里是 closed world（AOT dill，TFA 之后，
整个程序都在手里），哪个成员会抛是能算准的。用户据此要求撤回 panic 模型。

**修正（用户，2026-09-04，同日）：不按分析决定，一律 `Result`。** 所有函数、方法、
构造函数、闭包都返回 `Result<T, Rc<dyn Object>>`；函数类型和 `Future` 的输出同样
是 `Result`。ws244–ws245 的失败分析留作以后的优化（哪些成员其实不失败），不再决定
签名。要一起改的：函数类型统一；构造函数 `Result<Self, E>`（计数类 `Result<Rc<Self>, E>`），
const 上下文用 `match` 展开；prelude 接回调的函数自己也返回 `Result` 或暂存错误；
`throw` = `return Err`，`rethrow` = `return Err(e)`，`try` = `match`，`finally` 在分派
前跑；trait 声明、impl 转发器、super fn、async 同源。原语（越界、除零、null check、
`as`）仍先量再做检查版本。

**v2 的形状：**

1. **错误类型统一为 `Rc<dyn Object>`。** Dart 的异常就是对象，`catch (e)` 按 `is`
   分派；单一错误类型让 `?` 在任何调用形态下都不需要转换。
2. **失败集合在 driver 里对整个 component 算定点**（像 `dynamicSlots` 那样传给
   前端）：成员失败 ⇔ 体内有不被 catch-all `try` 包住的 `throw`，或调用了失败的
   成员——静态调用直接看目标；实例调用看接口目标在 closed world 里的**全部实现者**
   （任何一个失败就算失败，trait 方法签名因此对所有实现者一致）；函数值按声明的
   函数类型槽算并集（有失败的闭包/tear-off 流进去，槽就是 `Result`）。
3. **签名与调用点：** 失败成员返回 `Result<T, Rc<dyn Object>>`；失败成员里对失败
   成员的调用一律 `?`（`_qualified`、getter、闭包调用、静态、trait 默认转发器，
   全部形态）；非失败的值进 `Result` 槽包 `Ok`；`try` 体是 `match (|| -> Result {..})()`，
   `finally` 在分派前跑。
4. **原语（越界、除零、null check、失败的 `as`）先不进 `Result`。** 它们在 Dart 里
   也能被 `catch (e)` 抓到，要补的是 prelude 的检查版本；先量它们在 `try` 里出现
   多少次再做。
5. **尺子不变：** 每一步都过 `stubs.py`，Result 造成的错误按 kind 单独记。

## 决定:异常翻译到 `Result<T, E>`

用户定的方向,并明确说明理由:**约定到返回值的异常处理优于 panic**。
我先前提出的传染问题是**要处理的成本,不是反对的理由**,这里记下来。

语料(第 22 轮之后量的):

| | flutter | gallery |
|---|---|---|
| `throw` | **831** | 94 |
| `try/catch` | 171 | 1 |
| `on <Type> catch` | 174 | 1 |
| 带 stack trace 的 catch | 133 | 1 |
| `try/finally` | 73 | 0 |
| `rethrow` | 8 | 0 |

抛的是什么:`FlutterError` **474**、`StateError` 135、`UnsupportedError` 64、
`UnimplementedError` 45、`NoSuchMethodError` 39。

**`Result` 路线要面对的三件事**,都得先量再做:

1. **传染范围。** Dart 的 throw 不出现在签名里,所以"哪些函数返回 `Result`"
   要**算**:从每个 throw 出发,沿调用图向上传播,在有 `try/catch` 的地方停下。
   这和第 12 轮 `&mut self` 的不动点是同一形状的分析,只是跨类。
2. **错误类型。** 474 次 `FlutterError` 说明有一个主导类型。
   可以先做 `Result<T, FlutterError>`,别的 `throw` 暂时拒绝——
   **先量"一个函数能抛几种类型"**,如果多数是一种,枚举就不必要。
3. **`try/finally`(73)。** `Result` 不管清理。Rust 的答案是 `Drop` 守卫,
   或者把清理写在两条路径上。这一条和 `Result` 是**分开的**问题。

**不要**把译不了的 throw 变成能编译的安静错答案——第 19 轮 `??` 的教训。


## 目标改写(2026-09-03):从「翻译得完」改成「跑得起来」

原来的目标是**翻译**:把 gallery 和它依赖的 framework 翻成 Rust,尺子是拒绝数
往下走。第 114 轮结束时那把尺子读作 2743 个类 / 1265 个零拒绝的类 / 3099 次
拒绝,而队头那 1034 条(闭包捕获 `this` 599 + 撕方法 435)第 30 轮就量清楚了:
**它不是翻译问题**。一个闭包活得比造它的那次调用长,而 `this` 是借来的——
Dart 的对象是共享可变的,上游用 GC 提供这一点。翻译器再补一千个补丁也变不出
一个对象模型。

所以目标改成:**跑起来**,宿主是上游 flutter engine 的 **AOT 模式**,而 AOT
那一侧的两半都换掉——代码那半是 dart2rust 的输出,运行时那半是一个 Rust 写的
plain Dart VM。那 1034 条于是变成一个运行时的决定(`Rc<RefCell<T>>`,回边
`Weak`),做一次,不是做一千次。

### 要占的位置有多大(量过的)

engine checkout `0c2d270c5a9`(2026-09-03),`out/host_profile`,linux x64,
`flutter_runtime_mode = "profile"`——profile 就是 AOT。
`python3 bin/embedder_api.py`:

| 方向 | 是什么 | 量 |
|---|---|---|
| engine → VM | `Dart_*` 嵌入 API,五个头文件声明 312 个 | engine 真正调用 **168 个**,945 处调用点 |
| app → engine | `dart:ui` natives(`dart_ui.cc` 的 `FFI_FUNCTION_LIST` 57 + `FFI_METHOD_LIST` 174) | **231 个** |
| engine → app | `PlatformConfiguration` 里的持久句柄(begin frame、pointer packet、window metrics……) | **19 个** |

装载点只有那么几处,全在 shell 和 runtime 里:`Dart_LoadELF` 1 处、
`Dart_Initialize` 2 处、`Dart_CreateIsolateGroup` 5 处、
`Dart_SetFfiNativeResolver` 1 处。

上游的产物就在盘上,可以对着看:
`~/gallery_upstream/.dart_tool/flutter_build/ef21e168…/app.so`,32,965,632 字节,
里面有两个符号——`kDartSnapshotData` 和 `kDartSnapshotText`,正是
`runtime/dart_snapshot.cc:18` 写下的那两个名字。(这一份是 8 月 17 日在 Windows
上 build 的,`file` 说它是 PE32+ DLL;要在这台机器上跑,得重新 build 一份。)

### 两条路,选了哪条

**A(选这条):Rust 的 VM 顶掉 `libdart`。** engine 一个字节都不改,它照常调
`Dart_Initialize` / `Dart_CreateIsolateGroup` / `Dart_Invoke`,答话的是 Rust。
`args.gn` 里 `dart_component_kind = "static_library"`——libdart 本来就是静态链
进去的一个库,所以这是**换一个库,不是改一个 engine**。dart2rust 出的 crate
就是这个 VM 的 “snapshot”:`Dart_LoadELF` 那一步换成「把已经链进来的那份代码
交出来」。代价照直写:handle / scope / isolate 的语义要真做,168 个函数不是桩。

**B(没选,但记着):** 在 embedder 侧加一层薄 ABI,像 master 那条线做的那样
——`src/flutter/runtime/rust_app_api.h`,539 行,把下行绑定和上行回调摊成 C
函数,**而且已经被证明能跑完整个 gallery**。它便宜得多。但它是一个**改过的
engine**,不是上游的 AOT 模式,而后者正是新目标特意指定的那一条。
**A 推不动时可以退到 B,退的时候要说清是退了**——不能让「跑起来了」这句话
悄悄换了主语。

### 运行时要提供什么,接在哪

`lib/prelude.dart` 那 1248 行已经是这件事的第一块:手写的 `dart:core` /
`dart:typed_data` 子集,只是现在作为字符串跟着生成代码一起发出来。运行时 crate
(`tools/dart2rust/runtime/`,**还不存在**)先把它接管过来,再长出翻译代码
本来就假设有人提供的那些能力:

| 能力 | 上游在哪 | 接到 dart2rust 的哪里 |
|---|---|---|
| 对象模型(共享可变、身份、回边) | GC | 后端发 `Rc<RefCell<T>>`;`identical` 比的是地址(第 110 轮);环用 `Weak`(第 113 轮) |
| `dart:core` / `dart:typed_data` | VM 的 patch 文件 | 现在的 `prelude.dart`,搬进 crate |
| 事件循环、`Future`、`async` | isolate 的 message loop | 队头里 47 次 `await` 拒绝 |
| 异常与栈 | VM | 现在 catch 读 stack trace 一律拒绝(32 次) |
| 类型测试 `is` / `as` | VM 的类型层次 | `DartAny`(见 prelude),259 次 `is` 拒绝 |
| `dart:ui` | 231 个 native | VM 转发给 engine,生成代码只看见 Dart 那侧的签名 |

`embedder_api.py` 现在打印 `runtime  no crate at runtime yet`,这个 0 是故意印
出来的:距离就是这把尺子的全部意义。

### 尺子的盲点,先写下来

1. **「168」是上界,不是启动路径。** 数的是调用点,不是**跑到**的调用点——
   service isolate、DevTools、message port 有些编进去了但 headless 跑不到。
   下一轮按启动路径再收一次。
2. **写这把尺子的时候它自己先漏了一次。** 声明本来按行读,而 `Dart_SetField`
   的返回类型在 `DART_EXPORT` 那一行、名字在下一行,于是漏掉 18 个声明,
   第一次跑报的是 144。改成整篇读之后是 168。**数字往上走,是尺子准了**——
   和第 9 轮、第 24 轮同一类(九条第 8 条)。
3. `Dart_Isolate` 和 `Dart_NewFinalizableHandle_DL` 被丢掉是对的:一个是类型,
   一个是 `_DL` 宏,展开到 `Dart_NewFinalizableHandle`。**丢掉的东西看过一眼**,
   这是九条第 9 条要求的。

### 不变的部分

翻译那一半的纪律一个字不改:前端不认识的构造仍然**拒绝**,不猜;census 的队头
仍然是下一件事;fixture 仍然要能分辨「对」和「看着对」。新目标只是换了验收:
**gallery 出现在屏幕上**,而不是拒绝数归零——九条第 1 条早就说过那两件事不是
一回事。

---


## 运行尺子（2026-09-05 起）

编译尺子（`stubs.py` 的 `total stubbed`）量的是全树能否编译，目标是 gallery 跑起来，两者不等价：stub 是 panic 不是编译失败，工作区一直能编译。从 ws429 起换尺子：

- `workspace.py` 在有 `pub async fn main()` 的模块存在时多生成一个二进制 crate `dart_main`（`fn main() { dart_prelude::run_main(gallery_above::main::main()) }`）；prelude 加 `run_main`：轮询 future、跑 microtask 和到期 timer、睡到下一个 timer，没有任何东西能推进时报告并退出（没有引擎，等帧回调就是这个结果）。
- `bin/run_main.sh <log>`：在链跑完留下的**已打桩**工作区上 `cargo build -p dart_main`，运行，记录第一个 panic（stub 或 refusal 的原因）。同样受内存守卫，不和链并行。
- 每轮：修掉启动路径上第一个 panic（通用机制）→ 链（回归尺子，`total stubbed` 不许涨，`reachable crates` 从 139 变 **140**，多的是 `dart_main`）→ 构建运行 → 下一个 panic。
- 第一项已知：`main` 本身被拒（`GoogleFonts.config.allowRuntimeFetching = false`），见 ws429。


**章结(run429–run471,逐行表见 git):** 无头尺子从「`main` 被拒」修到 run471
**没有 panic**——`main` 跑完 binding 构造和 `GetStorage.init`,以 Dart 异常正常
结束("Bad state: BackgroundIsolateBinaryMessenger … ensureInitialized",来自无引擎
时 `GetRootIsolateToken` 答 0)。途中落地的机制:DartFuture 共享 eager 模型、
经 trait 的 async 调用先 `?`、构造体内联基类构造(`_inheritedBodies`)、late 首次
读取求值、集合相等走 `DartEq`、泛型 trait 方法的擦除孪生 `m__erased`、常量实例
带类型、`identical` 句柄按地址、native 边界 `dart_native` 与无宿主 `NativeAnswer`。

## 第二个 goal：无头引擎（2026-09-06 起）

run471 之后启动路径上剩下的每个停点都是 engine 的 native，所以 goal 换成：**runtime crate 的第一层——一个无头引擎**。它是手写的 Rust（`tools/dart2rust/runtime/src/lib.rs`），`workspace.py` 把它拷进工作区当 `dart_runtime` crate，附一个生成的 `generated.rs`（`pub use <持有 dart:ui 的 crate>::dart_ui;`，crate 名由分区决定），`dart_main` 先 `dart_runtime::install()` 再 `run_main`，跑完 `dart_runtime::report()`（帧数、平台消息）。

它按 engine 的 `PlatformConfiguration` 那样做事，不按 gallery 做事（没有任何 gallery 专用分支）：

- 装一个 `NativeHost`（prelude 的 native 边界，按 `@Native` 符号答）：`GetRootIsolateToken` → 1，`DefaultRouteName` → "/"，`GetPersistentIsolateData` → null，`ScheduleFrame` → 一个 `Timer::run` 到时调 `dart_ui::_begin_frame(us, n)` 和 `_draw_frame()`（计帧），`SendPlatformMessage` → 记名字、下一轮用 null 回调（"没有插件"），`RespondToPlatformMessage`/`SetNeedsReportTimings`/`Render`/`UpdateSemantics` → 无事。别的符号答 `Ok(None)`——宿主的"不是我的"，无引擎的值照旧、照旧记账（`NativeHost` 的返回类型为此改成 `Result<Option<Rc<dyn Object>>, E>`）。
- 装好后先做 engine 在 `main` 之前做的事：`_update_locales(["en","US","",""])`、`_update_user_settings_data(json)`、`_add_view(0, dpr 1.0, 800×600, …)`。

尺子：**gallery 无头地 build、layout、paint 出第一帧**，`run_main` 报出帧数。路径上撞到的 stub 照旧只用通用机制修。之后把这层换成真 engine 的桥（同一个宿主接口）。


**章结(run472–ws571,逐行表见 git):** Goal 2 于 run528 名义达成、run529 拆穿
(只是宿主自踢的预热帧,无 `Render` native)、run552 真正走进合成
(`SceneBuilder::Create` 已调,死在 `_pushTransform` 对 `None` 旧 layer 的 unwrap)、
run572 起帧数为真。途中落地的机制:对象协议(toString/`==`/hashCode 全进
`DartAny` + 注册表)、`dart_boxed` 装箱、私有成员跨库改名(`_memberName`,Dart
私有名按库)、擦除类型参数的类型字面量由对象回答(`_typeArg<Class><T>` getter)、
动态槽分派(`_dynamicSlotCall`)、`is C<dynamic>` 按运行时名(`dart_is_kind`)、
构造链 `chain.reversed`、事件循环吃掉回调异常(Dart isolate 语义)、panic hook
里 report(工作区 `panic = "abort"`:unwind 落点让巨型 widget 函数 codegen 半
小时)。run579–582 起延迟库静态链接,`merged_gallery_scc`,137→约 60 crate。

## Goal 3(2026-09-06 起):渲染树对账

尺子:翻译出的 render 树的**结构化遍历**(每个 render object 一行:`runtimeType`、
laid-out 的 `size`、`BoxParentData` 的 `offset`)与 Flutter 自己的输出 diff → 0。
(原尺子是 `toStringDeep` dump,它在 `assert` 里,release dill 没有——run554 起换。)

- 参考:`~/gallery_upstream/test/dump_render_walk_test.dart` 打出,存
  `~/dart2rust_build/scratch/ref_render_walk.txt`,6 行:`_ReusableRenderView →
  RenderSemanticsAnnotations → RenderSemanticsAnnotations → RenderTapRegionSurface →
  RenderSemanticsAnnotations → RenderConstrainedBox`,后五个带 `size=Size(800.0, 600.0)`。
- 产物:`~/dart2rust_build/scratch/got_render_walk.txt`。
- 第二份参考(2026-09-07):`test/dump_render_walk_settled_test.dart`(pump 8×100ms)→
  `~/dart2rust_build/scratch/ref_render_walk_settled.txt`,708 行,gallery 主页整棵树;
  run658 起前 23 行类型一致,分叉在过渡(`_RenderSnapshotWidget` vs `RenderAnimatedOpacity`)。
- **现状(run599):节点类型序列 6/6 一致(自 run584);还差 `size=`**——dump 时
  没有 layout 数据(2 帧已画,待查是 dump 时机还是 size 读取)。
- 仪器:`DART2RUST_DUMP_RENDER_TREE=1`(render 树)、`DART2RUST_DUMP_APP=1`
  (元素树)、`DART2RUST_RUNTIME_TRACE=名,名`(成员入口)、`DART2RUST_TRACE_HOST=1`
  (native 符号)、`DART2RUST_TRACE_CTOR`、`DART2RUST_TRACE_COVARIANT=1`、
  `DART2RUST_TRACE_MESSAGES`(平台消息字节)、`DART2RUST_RUN_SECONDS`(run_main.sh
  默认 60,预算用完报告还活着的定时器并 dump)、`DART2RUST_TRACE_TIMERS=1`。


**(2026-09-09 校注)** 结构那半**已闭合**:run773 起渲染树的节点类型、顺序、层级
与参考一致,run775/run807/run848/run874 反复量到 **708 行对 708 行、忽略
`size=`/`offset=` 后 0 处差异**(需 `DART2RUST_OS=android`)。仍差的 508 行全部只差
`size=`,根因是 32 个 `RenderParagraph` 量成 `Size(0.0, 0.0)`——无头运行时不答任何
`Paragraph::*` native,且 native 桥给实例 native **不传接收者**,宿主因此无法为
每个 paragraph 建模。**是运行时缺口,不是翻译缺口。**

## 数字轨迹

```
编译链(total stubbed / 拒绝 / crate):
  ws243(panic 模型最好)   7843
  ws246(Result v2 初版)   17068 → ws250 11061
  ws271(开放类落地)       7270
  ws279(this 句柄化)      5247
  ws350(删分支战役)       3284 / 804 / 138+1
  ws458(修尺子自身 bug)   2198(-375 全是误伤)
  ws497(Object?=dynamic)  ~1300
  ws522(协变全有或全无)   1068
  ws552                   794
  ws582(延迟库静态链接)   745 / 261 / 137→约 60
  ws601                   722 / 252 / 63 全可达

运行尺子:
  run430  第一次跑起来(build+run 约 100s)
  run471  无头尺子到头(main 以 Dart 异常正常结束)
  run528  名义首帧(run529 拆穿:宿主自踢的预热帧)
  run552  真正走进合成(SceneBuilder::Create;死在 oldLayer.unwrap)
  run572  第一帧真的 drawn;元素树 23 层
  run584  渲染树节点类型 6/6
  run600  jsonDecode 过(_coreTopLevel 表 + prelude 解析)
  run601  程序挂住(无 panic,超时):locale 名把 StringBuffer 打成
          "Instance of"、flutter/assets 的 en.json 回 none、周期定时器
          让 run_main 永不闲;新仪器 DART2RUST_RUN_SECONDS(默认 60)、
          DART2RUST_TRACE_TIMERS
```

```
2026-09-09 校注(接上,编译链):
  ws601   722 / 252 / 63 全可达
  ws698   463 / 183 / 64
  ws749   433 ——「整体清理」法开始:按 stubs<N>.txt.detail.txt 的
          expected/found 归并,一轮一条规则,量整组
  ws808   221 / 130
  ws847   201 /  60
  ws876   152 /  57
  ws877   152 /  52
  ws878   152 /  49
  ws895   147 /  38
  ws898   147 /  38,可达 64 -> 67(l10n 从 app 的 crate 里拆出去)
  ws900   147 /  38,可达 67 -> 69(use 行改从 Kernel 引用写,文本扫描删光)
  ws901   135 /  38,可达 69(counted 普查改读 mixin 应用里的体;引用带上签名类型)
  ws902   134 /  38,可达 69;运行尺子的渲染树回来了(707 行,类型差异 0)
  ws905   134 /  38,可达 69;`is T` 不再恒真,运行尺子往前走到旧桩上
  ws906   133 /  38,可达 69;`List.from` 非列表要收集
  ws907   133 /  38,可达 69;`(f ??= C()).add(x)` 加进的是副本
  ws908   133 /  38,可达 69;`Iterable` 成了 trait(`dyn DartIterable` 0 -> 660 处 / 126 个文件),`OverlayState.rearrange` 掉了
  ws909   132 /  38,可达 69;窄元素列表的加宽排到装箱之前
  ws910   132 /  38,可达 69;`Map` 的索引按版本而不是按条数判有效——**运行尺子回来了**:432 帧 0 panic、渲染树 707 行、与 ref 类型差异 0
  ws911   130 /  38,可达 69;界是 `Iterable` 的类型参数,槽跟着声明走;装箱不再无条件克隆(274 -> 42 处)
  ws912   129 /  **33**,可达 69;静态 setter 按被调用的名字找得到;`int.parse(s, radix: r)`;`super.where` 走到 `this` 的元素上
  ws915   127 /  33,可达 69;`SynchronousFuture.then` 当场回调(帧数 432 → 290,原因未查)
  ws916   126 /  33,可达 69;链子步骤的形参是**对句柄的引用**,少解了一层
  ws913   127 /  33,可达 69;`<num>[..]` 的 `int` 元素要转;prelude 的标量槽收 `dynamic` 要转

分区与墙钟(ws898,同一台机器,打桩循环的尾巴):
  改前  merged_gallery_scc 1,026,045 行 / 223 模块,依赖图的尾巴
        round 7/8/9 = 37s / 50s / 53s
  改后  scc_gallery   942,252 行(78 个 l10n,排在 app 下面,与别的 crate 并行)
        gallery_above  82,892 行(app 本体,-91.9%)
        round 7/8/9 = 20s / 33s / 33s

分区与墙钟(ws900,同一台机器):
  改前(ws898,文本扫)  64 crate;scc_flutter_widgets 325,924 行 / 240 模块
  改后(ws900,账本)    66 crate;scc_flutter_widgets 402,903 行 / 324 模块
        打桩循环最后三轮 = 37s / 34s(round 10/11,0 error 收尾)
  账本的边更多也更真:文本扫少导的名字过去是拿打桩抵掉的,现在导全了,
  于是 services -> widgets 这类真边把 84 个模块并进了同一个 crate。
  **这是这轮唯一的退步,记在〈已知欠账〉里。**

运行尺子(接上):
  run763  错误盒可读(dart_boxed:抛出的错误能打印了)
  run773  渲染树**结构**与参考一致
  run775  708 行对 708 行,忽略 size=/offset= 后 0 处差异
  run792  尺子从「panic」变成「时钟」:不再崩,改为超时
  run796  Map::from_pairs 解开阻塞;7 帧 46 条平台消息
  run807  预算改成线程本地截止时间(此前「每帧排下一帧」的程序
          永不离开 run_until_idle,预算永不到期,timeout 先杀掉它)
          708/708,0 类型差异;并加仪器 DART2RUST_TRACE_FRAMES=1
  run848  708 行,0 类型差异,0 panic,201 帧
  run874  708 行,0 类型差异,0 panic,196 帧,2881 条平台消息

  仍未收的两条(run804/run807 量的,仪器已在):
    - gallery **永不静止**:一帧排下一帧,上游会停,我们不停
    - 一帧**越来越贵**:frame 6 = 265ms,frame 500 = 493ms,
      斜率 +0.46ms/帧(线性,即总量二次)。有东西按帧累积
```

## 撤回与作废(不要再试)

**(2026-09-09,ws914 试过又撤回;证据不足,不是定论)** 在 `_widenedInto`
的可空闸上加 `param is VoidType`。理由是对的——Kernel 给 `VoidType` 的
nullability 是 `nullable`,照字面读它,每一次往 `void` 槽里存都被包成
`Some(..)`,`WidgetsBinding._handleBackGestureInvocation` 那两个桩就是这么
来的;桩确实掉了两个(127 → 125,新增 0)。撤回是因为改完连跑三次渲染树
都是 2 行,而**这把尺子本来就在抖**:撤回之后同一个二进制连跑两次是
707 行 / 2 行各一次。三次全空对上一个大约三成的空窗率,p 大概 0.2 ——
**suggestive,不是证据**。

所以这条记在这里的意思不是「这个改动错」,而是:**在尺子能稳定读之前,
不要合并这种半径的改动**。那条闸管的是全程序每一个 `void` 槽,两个桩换
不起一次读不出来的回归。

**(2026-09-10)重做完了,是对的**——ws934 治好了抖动之后,ws936 把
`param is VoidType` 放回去:stub 124 → **122**(新增 0,少的正是当初那两个
`_handle_back_gesture_invocation__body`),渲染树连采五次全是 707 行 /
类型差异 0 / 0 panic。当初撤回的三次空树是尺子在抖,不是这个改动。

近期的(细节在活账/git):

- **ws879**:泛型局部函数(`T? effectiveValue<T>(..)`,4 个拒绝)——声明按 bound 擦除、
  调用点也说擦除后的话,这半是**成的**:fixture `genlocalfn` 两端都打 `red/12|black/12`,
  拒绝 49 → **45**。但翻出来的四个成员编不过,stub **152 → 156**;再补一条
  「被同体内另一个闭包调用的局部函数必须自持」(`_CalledInNestedFunction`)之后仍
  **157**(多出 `material_time_picker.rs paint`)。**stub 只许降不许升**,故整轮撤回;
  改动留在 scratchpad 的 `ws879_frontend_kernel.dart`。
  **ws892 收了这一条**,并纠正了它的自我判断:「调用点也说擦除后的话」这半**不成**
  ——它读的是已代入的 `node.functionType`,把一个真值为 `Rc<dyn Object>` 的调用声明
  成了 `Option<Rc<dyn Fn(..)>>`,只是那四个成员先被借用/移动的错停住,类型错没显形。
  借用/移动那一面的根因也不是捕获规则:Kernel 的 `LocalFunctionInvocation` 不带
  `VariableGet`,`_LocalFinder` 因此看不见「闭包调用了兄弟局部函数」这次读。
- **ws899 的两次误判**(都被尺子当场抓住,记着别再犯):
  1. 「进对象槽就装箱」这条规则一落地,`loops` 就**翻译了它声明要拒绝的东西**——
     `identical(Object? a, Object? b)` 的形参就是对象槽,两边一装箱,
     `isTheCopy(Ladder other)` 不再被拒绝,改为比较两个刚装出来的箱子的地址,
     **编得过而且永远答 false**。收法:`identical` 问的是引用不是值,在前端已经
     识别它的那一处把操作数上的 upcast 取回来。
  2. `fixtures.py` 报「agree now, so take them out of BEHIND」**不可尽信**:
     「追上了」算的是 `BEHIND - behind`,而一个以别的方式**失败**的名字(`loops`
     当时正卡在 REFUSES 那条)根本不进 `behind`,于是一次失败被读成了一次追上。
     我照着把 `loops` 划掉了,下一轮它就以老样子重新差异出来。有失败时,先修失败,
     再看追上。
- **ws721**:「接收者的静态类不是具体类就一律走 trait 访问器」(想修 `notification.metrics`
  的字段访问)——**stub 805**(基线 449),撤回。窄解是只对**重定类型的形参**按声明的类判断。
- **ws704–706**(已作废,ws708 走通了):把「覆盖关系」算成 covariance 的 flow site
  一开始是 +22——擦除边界上标量与句柄进出不成立。补齐四条边界规则后(见 ws707/ws708)
  同一条改动是 **-3**。教训:量到 +N 时先看是不是边界规则缺,不要先撤回结论。
- **ws531/532**:把空心 mixin 的字段从 application 恢复到声明上(+163/+181,撤回)
  → 窄解是 `IrClass.appliedFields`(trait 只多声明 `_cell()`)。
- **ws544**:方法类型参数加 `+ Trait` 约束撞擦除孪生(`__erased` 用 `Rc<dyn Object>`
  实例化 T,句柄不是 trait,+252/332 错,撤回)→ T 上调 bound 成员走对象自己的 cast。
- **ws520**:协变擦除擦掉 `RestorableEnum<T extends Enum>` 的 `Enum`(bound 没有拼写,
  函数外错误,全链断)→ 只在 bound 有句柄拼写时擦;且沿继承链**全有或全无**(ws521)。
- **ws539**:块值先于语句降低(switch 临时量未声明先读,拒绝 357→777)→ 语句先、值后。
- **ws550**:装箱闭包 `as Rc<dyn Fn(..)>` 不把期望类型传进闭包体;typed-let 修正在
  37020756 又改回朴素拼法 + null-aware map 写明返回。
- **ws595**:`Vec<T>: DartAny` 要 `T: Clone` 断了整个链 → trait 声明的类型参数也带
  `Clone`。
- **ws498**:`Object?` 归 `dynamic` 后,更宽的 impl 必须按 **Rust 类型**(`sameRust`)
  去重,按 IR 文本会撞(E0119 函数体外)。
- **ws500**:生成的遍历器与类自己的 `toList` 重名 → 用 Dart 成员取不到的名字
  (`__to_list`)。
- **ws437**:`_ThisEscapes` 太宽(`return this`/值参数都算,enum 也成了 Rc)
  → 只有 `this` 进 handle 槽、进字面量、存进别的对象字段才算;enum 永不计数。
- **ws480**:「trait 声明的非 final 字段一律 cell」太宽 → 只有 trait 自己的体在
  `this` 上赋值的字段才做 cell。
- **ws458**:**尺子自身的 bug**:同轮同文件按行号打桩,高处桩挪了行号误伤健康
  函数,累计虚高 375。任何「+N」先怀疑尺子。
- **ws449/450**:方法级 `where Self: X` 在可派发 trait 方法上是 E0038,走不通;
  mixin 的 supertrait 取每个 application 都放在它下面的东西(`_appliedOver`)。

更早的(叙事在 git,结论留置):

- 第 65 轮:nightly 并行前端对名字解析无效。**不做。**
- 第 40 轮:按 SCC 拆 crate,库图只允许并行两个。**不做。**
- 第 44 轮:翻译 `dart:core` 让错误 6608→16955。**dart: 一律手写(prelude)。**
- 第 57 轮:「直接借」这条近路只值 4%。
- 第 86 轮:那 14 个错误该留着(否定结果)。
- 第 102/103 轮:`Rc<Self>` 的价钱由 fixture 定的形状。


**2026-09-09 补(ws749–ws876 这一程撤回的,细节在 git):**

- **ws759**「一个没有名字的类型参数就是它的界」——读起来通顺,是错的:
  `_asApplied` 后来会把名字填上。314 → 325,撤回。
- **ws765–767** 三次收窄擦除规则,三次都是 ±0——**第一次的补丁根本没落进文件**
  (补丁报告成功,提交里却是另一段)。是 `DART2RUST_ERASURE_OFF=1` 这个二分开关
  (ws768,关掉规则量到 289)才照出来的。教训:量到 ±0 时先验证补丁真的落了盘。
- **`identityHashCode` 两次**、**`identical` 打在 prelude 值上**、
  **`dart:core` 接口当超 trait**、**`first =`/`last =`**、**`x is Function`**、
  **链的类型(两次)**、**泛型局部函数**:一程十四条落地里撤回六条,
  **其中四条能编过而且答案是错的**——这就是每轮先付夹具的账、再付链的账的理由。
- **值类身份的窄解**(`super.==` 走 `_identical`):参数到达时是值不是引用,
  两半是同一堵墙,与其留一条永远说不的路,不如撤回。

## 活账:ws/run 表(窗口 40 行;更老的在 git)

窗口 = 最近 40 轮;更老的在 git。

| 轮 | 规则 / 读数 | 数 |
|---|---|---|
| run877 | the reading, after ws875-ws877 | — |
| ws878 | `identityHashCode` on a handle is the address behind it | stub **152**,拒绝 49,可达 64 |
| ws879 | 泛型局部函数:翻得出来,编不过,整轮撤回(见〈撤回与作废〉) | stub **152**,拒绝 49,可达 64(未动) |
| ws880 | 编译器自身:装回分析器与单测,四份变异名表并作一处,删死码 387 行 | stub **152**,拒绝 49,可达 64;**生成的 Rust 与 `HEAD~1` 逐字节相同** |
| ws881 | 两个 god class 各拆成一个目录的 part(`augment class`),搬运零改字 | stub **152**,拒绝 49,可达 64;**生成的 Rust 与拆前逐字节相同** |
| ws882 | part 文件再切细:按成员边界切进两个 5k 文件,最大 part 1,841 行 | **生成的 Rust 与拆前逐字节相同**(md5 4429c8f1),故 152/49/64 不变——这一轮只跑了翻译,没跑 cargo 九轮 |
| ws883 | `_expressionRaw` 1,717 行一个方法拆成十段 run,`the_class` 再切四份;最大 part 1,452 | **生成的 Rust 仍与拆前逐字节相同**(md5 4429c8f1) |
| ws884 | 拒绝回滚的名单补全(9→22 字段)并加 `bin/statecheck.py` 守住 | stub **152**,拒绝 49,可达 64;**生成的 Rust 一字未动**——是陷阱不是活 bug |
| ws885 | mixin 的 super 函数要 `__Self` 是什么,trait 头上就得先是什么 | stub **150**,拒绝 49,可达 64;渲染树与 run877 逐字节相同(197 帧 0 panic) |
| ws886 | 把分析器重新变成门:前端迁到 analyzer 14.3(47→0 错),`check.sh` 摘掉 `|| true` 并给警告数加上限,双前端预言机重新点着 | 33 个 fixture 两侧全能生成(此前分析器那侧一个都不能——driver 自己编译不过);**生成的 Rust 一字未动**(926 模块 md5 e69150fe,拒绝仍 49);check.sh 干净退出,87 checks OK |
| ws887 | 复审抓到 ws886 过度宣称:重生成的黄金按错误数收下,实为退步——全部退回,并给「黄金怎么验收」装尺子(`regen.py` 先问 `can_propagate()`);顺手清掉恒真式死码 `_computeFailing`/`_errorIn`/`_traitDeclares` | **生成的 Rust 仍一字未动**(926 模块 md5 e69150fe);analyze 86 条不变;check.sh 干净退出 |
| ws888 | 复审指出 ws887 的门是字面绊线:`'fails:' not in ...` 被一行 TODO 注释就能打开。换成行为探针——真跑这一轮要用的每个 driver,输出里那个必失败的调用不带 `?` 就拒绝;`--anyway` 从 `testdata/src` 改写进 gitignore 的 `.agree/anyway/` | 探针 21 秒,两个 driver 都当场重现出 `Ok((self.checked(value) * 2.0))`;补上 TODO 注释后旧门放行、新门照拒;`git status` 零改动;check.sh 干净退出,86 条 / 87 checks |
| ws889 | 两个 fixture driver 现在都发得出 `?`:`_fails` 开头那句 `if (throws == null)` 删掉——它收着一个 `ThrowsAnalysis` 却一行答案都不读,是穿着分析外衣的开关;分析器前端补上 `_callFails`(10 个调用点),两侧共用 `ir.dart` 的 `translatedLibrary`。**顺带撞见这一轮最大的一件事**:删掉那个「什么都不决定」的分析,gallery 输出动了 32,653 行——真正起作用的是它把每个 body 都读了一遍;dill 改成显式 `BinaryBuilder(disableLazyReading: true)` | gallery 仍是 e69150fe(926 模块 / 49 拒绝),eager 读 60 秒 / 1.29 GB;预言机 exit 0,曾经新分叉的 7 个(cascade/failure/freefn/ifnull/mutation/nullcheck/trycatch)重新一致,BEHIND 仍是 16;analyze 86 -> 80,check.sh 上限同步下调 |
| ws890 | 黄金重生成:32 个文件,driver 已被行为探针证明发得出 `?`,预言机绿着。**验收不是数字**——44 个错误一条不落地读完,归成 7 个根因,全部在生成的代码里,没有一条在 `lib.rs` | lib 139 -> 44 错;`lib.rs` 里另有 317 个是「调用现在返回 Result」的机械改造,还没做,146 个 `#[test]` 仍然全黑;预言机 exit 0,BEHIND 仍 16 |
| ws890b | 试了 `lib.rs` 的机械改造并**放弃提交**:编译器自己的 span 驱动,317 -> 102 错、289 行改动,然后停手。理由不是难,是**验不了**——那 289 行的唯一检查就是 146 个 test 跑起来,而它们被生成代码里那 44 个错误挡着 | 抽查已见坏编辑(`.collect.unwrap()()`、给一个 `Map` 加 `.unwrap()`);全部回退,`git status` 干净 |
| ws892 | 泛型局部函数:声明按类型参数的界擦写,调用点改读 `node.localFunction` 的原始签名;闭包调用兄弟局部函数是对那个绑定的一次读;块的值若是本块没绑定的局部,要克隆 | stub 150(未升),拒绝 **49 → 45**,可达 64;夹具 `genlocalfn`/`dynfn`/`ifnullmove` |
| ws893 | 促升过的局部作接收者:`_WalkSelf` 与 `_mutPlace` 现在剥同样的两层(`!` 与「读即克隆」),`let mut` 才跟得上 | stub **150 → 149**,拒绝 45,可达 64;run894 708 行 / 差异 0 / 0 panic;夹具 `promotedmut` |
| ws894 | `dart:ffi` 的 `_abi()`:结构体布局是「按 ABI 一项的常量表 + `_abi()` 下标」,prelude 按目标机的 `OS`/`ARCH` 在 `Abi.values` 里查,查不到就说没有 | 拒绝 **45 → 38**,stub 149(未升),可达 64;run895 708 行 / 差异 0 / 0 panic;夹具 `ffisizeof` |
| ws895 | 窄化过的动态 `num` 调用要记下手里剩的是什么(按 prelude 的 `DartDouble` 签名);`_returned` 原先只认识 `IrNew`/`IrConstInstance` 两种「能变成对象的东西」,一次调用是第三种 | stub **149 → 147**,拒绝 38,可达 64;run896 708 行 / 差异 0 / 0 panic / 196 帧;夹具 `dyncall` |
| ws896 | 模块图的边是从生成文本正则回扫出来的,连字符串和注释一起扫:五个翻译文案里的单词("Header"、"Sac a main")就是五条真边。扫描前剥掉注释与字面量;再加一条配对规则——只有当一个「所有定义处都是 `fn`」的名字在本模块**既被绑定、又从未被调用/走路径**时才不算引用 | 拒绝 38、stub **147**(未升)、可达 64 → 67;`merged_gallery_scc` 1,026,045 行拆成 `scc_gallery` 942,252(l10n)+ `gallery_above` 82,892(app,**-91.9%**);链尾 53s → 33s;先只用前半句时 rustc 当场给了 188 条 `cannot find value`(自由函数当值传),那正是配对规则的由来 |
| ws899 | 黄金 44 个错里 28 个的根因是「夹具 driver 不是同一个编译器」:`dart2rust_kernel.dart` 一个 `TypeEnvironment` 都没建(前端里 34 处读它,全走 null 分支),`const Spacing._(3.0)` 的 `3.0` 因此退化成 `dynamic`,`const` 里发出一次拆箱。另一半:分析器前端一次实参加宽都不做,补上「进对象槽就装箱」(`IrUpcast`,由后端选 `dart_boxed`/`dart_object`/句柄) | testdata **44 → 16** 错;oracle exit 0、BEHIND 16 → 14(`constdirect`/`named_args` 真追上);十个 fx 夹具全 AGREE;gallery 逐字节未动(md5 1aee02ea,拒绝仍 38)——这一轮没碰生产路径 |
| ws900 | `use` 行一直是从**生成出来的文本**用正则回扫猜出来的,而后端发射时明明知道每个引用指向哪个库——`_ReferenceCollector._member` 手里就是答案,却只留下库和类名、把成员名扔了。改成一次遍历(`referencesOf`)同时交出库、类名、成员,`use` 行照账本写;文本侧十二条补丁一次删完(`_code`/`_identifiersIn`/`_calledIn`/`_boundIn`/`_packageOf`/`everyDefinitionIsAFunction`/`visible`/同包兜底/Dart import 列表/`pub use` 再导出)。账本盖不到的只有**编译器自己发明的名字**,各自在发明处记一笔:`superFn` 与它的 trait 界、类头的 supertrait、`implName`、抽象类的静态、宽 impl 与动态槽两次普查、`_genericOnTrait` 选中的体、被应用的 mixin 体 | stub **147**(未升)、拒绝 38、可达 **67 → 69**、`cargo check --workspace` **0 error**;`dart2rust_package.dart` −290 行;widgets crate 325,924 → 402,903 行(**变大,记债**);oracle exit 0 / BEHIND 14;**run900 红**——账本把 `scheduleMicrotask` 从 `dart:ui` 的空实现改绑到 prelude,microtask 第一次真的跑起来,当场撞上 147 里早就有的那个 `_handle_focus_changed`(下一轮修) |
| ws901 | run900 停在 `_handle_focus_changed`:`policy.invalidate_scope_data(..)` 要 `&mut self`,而 `policy` 是 `Rc<dyn FocusTraversalPolicy>`。两本普查读的不是同一批体——后端的 `_mutating` 读**降下来的 IR**(里面有 `_policy_data.remove(k)`),前端决定 `counted` 的 `_writesFieldInMethod` 只走类**声明**里的 procedures,而 CFE 把 mixin 的体拷进了匿名**应用**类。让它读同一批:自己的 + 上方匿名应用的 + (是 mixin 声明时)`applications` 里的。顺手把账本欠的三处收了:`_member` 现在也读被引用成员的签名类型(Rust 调用处没有推断——适配实参发射的是被调方的参数类型) | stub **147 → 135**(focus 那组 9 个 + 三处 import 3 个,一个新的都没加)、拒绝 38、可达 69、`cargo check` 0 error;夹具 `mixinmutmap` 先红后绿;oracle exit 0 / BEHIND 14;run901 仍红,但往前挪到了下一个桩(`Completer<void>.complete()`) |
| ws902 | 两条都在 microtask 刚被叫醒的那条路上。一、`Completer<T?>.complete()` 省略实参是 Dart 的 `complete(null)`,而这里写死成「`Completer<void>`,传 `()`」——改成按接收者的类型实参:`void` 传单元,投影过的 `T?` 用 `from_option`(`IrNullableOf`),其余传 null。二、prelude 的 `_schedule_microtask` 是**同步就地执行**回调的,而 `SCHEDULER.microtasks` 队列和 `run_until_idle` 的排干顺序本来就在——ws900 之前没人调它(名字被解析到 `dart:ui` 那个没有 host 应答的原生上),所以这条一直没被看见。入队后,`FocusManager._markNeedsUpdate` 的回调不再在 build 中途重入 | stub **135 → 134**、拒绝 38、可达 69、0 error;夹具 `completervoid` 先红后绿;**渲染树回来了:707 个节点,与 `ref_render_walk_settled.txt` 类型差异 0**;run903 仍以 panic 收尾——`NotificationListener<T>` 的 `notification is T` 里 `T` 被擦成了界,恒真(下一轮) |
| ws905 | `x is T` 在擦除过的类型参数上**恒真**(夹具 `iserased` 给的是错答案:`Sink<ScrollNote>().accepts(MetricsNote())` rust true / dart false)。`_typeArgumentGetters` 只给抽象/open 类开 `_typeArg<C><T>` getter——具体类没有子类能回答,只有 `new` 那一处知道。改成:被自己的体当类型读、且**界是一个具体翻译类**的参数不擦(界读自声明,不读实例化普查——普查是边降边填的,先降的库和后降的库会给出不同答案,`WidgetStateMapper` 就在一个模块里是泛型、另一个里不是);留住还要**顺着实参传播**——`NotificationListener<T>.createElement()` 造 `_NotificationElement<T>`,而那个签名里没有它,得走构造调用的类型实参 | stub **134**(集合与 ws903 完全相同,零进零出)、拒绝 38、可达 69、0 error;夹具 `iserased` 先红后绿;run905 渲染树 707 行 / 类型差异 0,`editable_text` 那个 panic 没了,现在停在 `_ScrollNotificationObserverState._notify_listeners`——134 个旧桩里的一个 |
| ws906 | `List.from(xs)` 发的是 `xs.clone()`——只有 `xs` 已经是 `Vec` 时才对。`List<_ListenerEntry>.from(_listeners!)` 的 `_listeners` 是 `LinkedList`,于是一个 `LinkedList` 落进 `Vec` 槽。按实参的 Rust 类型分:是列表就克隆,不是就 `to_list()` 收集 | stub **134 → 133**、拒绝 38、可达 69、0 error;夹具 `listfromiterable` 先红后绿;run906 越过了 `_notify_listeners`,停在 `RenderObjectElement.renderObject` 的 `unwrap`——`_LayoutBuilderElement` 在 `didChangeDependencies` 里取渲染对象,而它还没挂上(microtask 醒来后才走到的一条新路) |
| ws907 | `(field ??= C()).add(x)`:`??=` 交回的是它读到的**值**,而一个集合字段读出来是副本,`add` 加进副本就丢了。`Element.dependOnInheritedElement` 写的正是这一句,于是 `_dependencies` **永远是空的**,`_ensureDeactivated` 的循环一次都没跑。IR 上给 `IrIfNull` 记一位 `assignsLeft`(前端按 Kernel 给 `x ??= v` 的形状认:右边存进左边读的那个位置),后端的 `_mutPlace` 就能穿过去,先保证有东西再从 cell 上改 | stub **133**(集合不变)、拒绝 38、可达 69、0 error;夹具 `ifnullfieldadd` 由**错答案**(rust 0 / dart 2)转绿;探针确认修好后卸下时真的看到依赖了(1184 次带 1 条、424 次带 3 条),但 run907 仍停在同一处——摘不掉的是 `_dependents` 那一侧,还在查 |
| ws908 | `Iterable` 早就是一个 trait(`DartIterable`),`Vec` 和 `Set` 都实现了它,而生成的 Rust 里 `dyn DartIterable` 出现 **0 次**:类型下降见到 Dart 的 `Iterable` 就无条件写成 `Vec<T>`,于是「静态类型写着 `Iterable`、手里是 `Set` 或 `LinkedList`」的位置每次都是类型错误。先量两个数再动手——`Iterable` 作为槽出现 **1662** 处(在类型下降处打点,不是文本 grep);一个体向 `Iterable` 要的成员是 **25 个名字 501 处**(`toList` 148、`forEach` 78、`where` 39、`first` 33)。既然要的东西都写在列表上,trait 就只带两件列表做不到的事:`iterator` 和 `dart_to_list`;`Iterable<T>` 落成 `Rc<dyn DartIterable<T>>`,`Vec`/`Set`/`VecDeque`/`LinkedList` 各一个 impl。边界只有两句话:**翻译过的代码说 `Iterable`,prelude 说 `List`**——prelude 成员产出的 `Iterable` 按 `List` 记账(`expression` 的收尾),prelude 形参的 `Iterable` 槽按 `List` 记账(`_overList`),其余全部交给 `coerceInto` 的一进一出两条规则。顺带删掉四条现在说的是假话的旧账:`normalName` 里 `Iterable => List`(把两个不同的 Rust 类型当成同一个,`sameRust` 每处都因此放行),后端 `_returned` 里那条返回时装箱的补丁(返回值本来就走 `_widened`),`_widenedInto` 里那条往翻译过的 `Iterable` 形参装箱的特例(通用 `coerce` 会做,而且它会先按元素类型转换),以及 `dart:` 的几个 Set 类不在 `normalName` 里 | stub **133**(集合一进一出:`OverlayState.rearrange` 掉了——`LinkedHashSet` 一直是以陌生人的身份走过每一条 `Set` 规则的,ws638 起挂到今天;新挂 `hash_sink._finalizeData`,`Uint8List` 的元素加宽被装箱抢在了前面)、拒绝 38、可达 69、0 error;`dyn DartIterable` **0 → 660 处 / 126 个文件**;夹具 `iterableread`、`iterableparam`、`listfromiterable` 三绿;run918 与 run907 **逐字节相同**(同一处 `widgets_framework.rs:5471` 的 `unwrap`,同样 18 帧、同样两行渲染树)——运行尺子没被这一轮动过 |
| ws909 | ws908 换进来的那个桩:`Uint8List` 是 `Vec<u8>`,`Uint8Buffer.addAll(Iterable<int>)` 的槽是 `Rc<dyn DartIterable<i64>>`,中间要一次 `!widen`(`v as i64`)。那条规则在 `_widenedInto` 的**尾巴**上——ws907 时通用 `coerce` 对这两个是恒等,所以尾巴跑得到;现在 `coerce` 在中间就装箱返回了,尾巴再也到不了。把判定抽成 `_widensNarrowElements`,槽是 `Iterable` 时在 `coerce` **之前**先加宽,尾巴上那条留给 `List` 槽并加一位「已经加过」 | stub **133 → 132**(与 ws907 的集合逐条比:新增 0,少了 `OverlayState.rearrange`)、拒绝 38、可达 69、0 error;夹具 `iterableread`/`iterableparam`/`listfromiterable` 三绿;新写的 `iterablebound` **是红的**,它钉住的是下一条:界是 `Iterable<E>` 的类型参数,声明按界拼、实参的槽却按实例化拼(`equality.rs` 三个桩) |
| ws910 | 渲染树从 ws906 起就塌成两行,STATUS 记的线索是「6488 次卸载里 1192 次 `_dependents.remove` 没命中」。根因不在 Element 那一侧,在 prelude 的 `Map`:它维护一个惰性索引,而**索引是否还有效是拿建索引时的条数和现在的条数比出来的**。条数会回来。八条以下查找走线性扫描、索引根本不维护,于是一张跌破八条又长回同样条数的表,带着为**已经不在了的那批条目**建的索引被当成有效的:`contains_key` 对在场的键说不在、`remove` 找不到东西删、`insert` 把一个它看不见的键又追加了一遍。`InheritedElement._dependents` 每一帧都在做这件事——同一个元素被插了两次、只删掉一次,失效的 `_LayoutBuilderElement` 就留在依赖表里被通知,`renderObject` 拿 `None` 去 `unwrap`。改成一个 `version` 计数器:每一次改动 `entries` 都自增,索引记下它建立时的版本,追加是唯一能顺着走而不用重建的改动 | stub **132**、拒绝 38、可达 69、0 error;夹具 `mapindexstale` 由错答案(rust `11/12/4/null/107/null`,dart `11/11/8/103/107/11`)转绿;**run921:0 panic、432 帧(run919 是 18 帧)、渲染树 707 行、与 `ref_render_walk_settled.txt` 的类型差异 0** |
| ws911 | 三件。一、**界是 `Iterable<E>` 的类型参数,声明按界拼、实参的槽却按实例化拼**:`_type` 把 `T extends Iterable<E>` 拼成界(现在是 `Rc<dyn DartIterable<E>>`),而 `_landingSlot` 把接收者的 `T := Set<E>` 代进去,说槽收 `Set`,于是 `coerceInto` 觉得两边同型、原样放行。让 `_landingSlot` 对这种参数交回 null,`_argument` 也按声明走(`_atBound`),读的一侧 `_listReceiver` 认得这种接收者(`_iterableSpelling`)。**只对 `Iterable` 界**——`_spelledAsBound` 还答 `String`/`int`/`double`/`bool`/`List`,那几种实例化和拼写同型,拿声明去换是把对的换错。二、**装箱不再无条件克隆**:`!as_iterable` 原先总补 `.clone()`,理由写的是「接收者常常是借用」,而对着生成的 Rust 数,`&Vec<`/`&Set<` 各 0 处,只有 26 个 `&mut Vec<` 形参是真借用;问一句「`expr` 发出来的东西已经是自己的了吗」(`_ownedWhenSpelled`:调用、字面量、构造、块值是,裸局部不是)。三、**给复制挂上计数器**(`DART2RUST_COUNT_COPIES`),这是第 3 条大改动的前置 | stub **132 → 130**(与 ws921 逐条比:新增 0,少了 `equality.rs` 的 `hash`/`equals`)、拒绝 38、可达 69、0 error;夹具 `iterablebound` 先红后绿;装箱里还带 `.clone()` 的 **274 → 42**(共 280 处装箱);run924 树 707 行、类型差异 0、0 panic |
| ws912 | 三条拒绝,都是「明明翻译了却说没翻译」。一、**静态 setter 按两个名字擦肩而过**:调用点叫它 `set_systemContextMenuClient`(Dart 名前面加 `set_`),类里记的是 `systemContextMenuClient` 加一位 `isSetter`,`m.name == name || _methodName(m) == name` 两边都对不上,于是 `ServicesBinding.systemContextMenuClient` 这个**已经发出来的**函数被判成没翻译(3 处)。二、**`int.parse(s, radix: r)`**:原规则要求没有具名实参,`parseCompactDate` 传了 `radix: 10`。prelude 补两个函数(`from_str_radix` 自己认符号),前端认这一个具名参数,并把名字加进 `_preludeFunctions`——不加的话它转头又被判成「没翻译的顶层函数」(这一步在 ws926 里现场发生过)。三、**`super.where` 进 `dart:core` 的 `Iterable`**:基类不是翻译出来的类,后端只能拒绝;而 `Iterable.where` 本来就是「在 `this` 的元素上做」,所以前端把它降成 `this` 上的普通成员调用,接收者照常物化 | stub 130 → **129**、拒绝 38 → **33**、可达 69、0 error;与 ws921 逐条比新增 0,少了 `equality.rs` 的 `hash`/`equals` 和 `widgets_system_context_menu.rs` 的 `init_state` |
| ws913 | 两条都是「槽的 Rust 类型和送进去的东西对不上,而中间那道转换没人叫」。一、**`<num>[..]` 里的 `int` 元素**:prelude 把 `num` 拼成 `f64`,而 `coerceInto` 里 `slot.name == 'num'` 是**原样放行**——那条是为算术写的(`num.+` 收 `num`,`i + 1` 还是 `i64`),对元素位置就是错的。`TextInput._setSelectionRects` 建的是 `<num>[bounds.left, .., rect.position, rect.direction.index]`,double 和 int 混着,`Vec<f64>` 只收一种。判定放在列表字面量的元素上(`listElement`),两个降列表字面量的地方(`_listLiteral` 和 CFE 的 `_GrowableList._literalN`)共用它。二、**`dynamic` 进 prelude 的标量槽**:`DateTime.fromMillisecondsSinceEpoch(arguments)` 的 `arguments` 是从 `Map<String, Object?>` 里读出来的句柄,而 prelude 收 `i64`;`translated` 那道闸从来不问,因为标量形参不提任何顶类型 | stub 129 → **127**、拒绝 33、可达 69、0 error;与 ws927 逐条比新增 0,少了 `set_selection_rects` 和 `_date_picker_route` |
| ws915 | **`SynchronousFuture.then` 必须当场回调**,而这里它派了个任务。Flutter 自己在 `_RootRestorationScopeState._replaceRootBucket` 里写了断言:`assert(!_isWaitingForRootBucket); // Ensure that load finished synchronously.`——靠的就是 `rootBucket` 在桶已经有效时返回 `SynchronousFuture`,`then` 同步回调。派任务的话,那一帧 `build` 返回 `SizedBox.shrink()`,整棵子树就没了。prelude 的 `DartFuture` 加一位 `synchronous`(只有 `DartFuture::synchronous` 会置),`then` 见到它就当场跑 `on_value` 并交回另一个同步 future;`Future.value(x).then(f)` 不受影响——Dart 那个本来就是微任务。**认哪个类不看名字**:问它自己的 `then` 体里有没有把回调参数当函数调用(`_CallsParameter`)——`SynchronousFuture.then` 里是 `onValue(_value)`,`package:async` 的 `DelegatingFuture.then` 是转交给别的 future,不算。这个程序里 `_futureLike` 只匹配到 `SynchronousFuture` 一个类(`package:async` 不在可达集里),但判定是照体写的,再来一个也答得对 | stub **127**(不变)、拒绝 33、可达 69、0 error;`future_synchronous` 出现在 10 个文件里,包括 `services_restoration.rs`;**渲染树的抖动没治好**:5 次采样 2 满 3 空,和改之前分不出来。**帧数 432 → 290(降 33%),原因没查**——这是这条改动已知的代价,记在这里 |
| ws916 | `xs.iter().map(|child| ..)` 交给体的是 `&Rc<dyn X>`,比句柄多一层引用,而接收者按 `_isHandle` 拼成 `&*child`——少解一层,于是 `FocusNode::to_diagnostics_node(&*child, ..)` 说「`Rc<dyn FocusNode>` 没有实现 `FocusNode`」。空安全绑定(`IrBound`)早就有这条 `&**`,缺的是**谁知道这个局部是按引用绑的**——只有 `_stepClosure` 知道,所以它像记 `_cellLocals` 一样把这些名字记进 `_refLocals`,接收者那一处照着 `IrBound` 的样子多解一层 | stub 127 → **126**、拒绝 33、可达 69、0 error;与 ws932 逐条比新增 0,少了 `focus_node_super_debug_describe_children` |
| ws934 | **trait 对象的 `==` 按地址比,而 Dart 的 `==` 派发到对象**:`WidgetsApp(key: GlobalObjectKey(this))` 的两把钥匙包着同一个 state,Dart 说相等、这里说不等,于是 `canUpdate` 说不能更新,`WidgetsApp` 连同整棵子树每次 rebuild 都重建(60 秒 193 次)——渲染尺子抖了十几轮就是这件事,细节在〈已知欠账〉。改成和 `dyn Object` 一样走对象自己的答案(prelude 的 `dart_any_eq`/`dart_any_hash`:注册表里有就用类的 `==`,没有退回地址)。顺路两条:擦除过的实例化在槽上 cast 回来(类型参数带 `'static`,`TypeId` 问得出),而「结果按界读回来」的调用上接收者不做这次 cast(否则转换两次);`statecheck.py` 点名的 `_refLocals` 补进 `_member` 的存/还 | stub **126 → 124**(逐条比新增 **0**)、拒绝 33、可达 69、0 error;21 个 fixture 全 AGREE;**run935 连采五次全是 707 行 / 类型差异 0 / 0 panic**——尺子第一次不抖 |
| ws936 | 把 ws914 撤回的那条放回去并**量了**:Kernel 给 `VoidType` 的 nullability 是 `nullable`,照字面读,每一次往 `void` 槽里存都被包成 `Some(..)`;`_widened` 的可空闸现在也认 `param is VoidType`。当初撤回的理由是「改完连跑三次渲染树都是 2 行」,而那是尺子在抖(ws934 治好了)。顺路给擦除加了一个诊断:`DART2RUST_TRACE_ERASED=1` 说协变扫描标过的参数最后**擦没擦、被哪一道闸拦下**——协变的 trace 只说标了什么 | stub **124 → 122**(逐条比新增 **0**,少了 `widgets_binding.rs` 的 `_handle_back_gesture_invocation__body` 与它的 super fn)、拒绝 33、可达 69、0 error;run936 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws937 | **`late` 字段的格子里装的是 `Option`,而「赋值当表达式用」那一路没有包 `Some`**。语句那一路早就包了(`IrAssignField` 里那句注释写着「这是唯一发生这件事的地方」——在表达式那一路也需要它之前是真的)。Dart 里 `x = v` 的**值是 `v`**,存进去的才是 `Some(v)`,所以只包存的那一侧,`__set` 照旧是裸的:`_opacityAnimation = CurvedAnimation(parent: _opacityController = AnimationController(..), ..)` 是这个形状 | stub **122 → 119**(逐条比新增 **0**,少了 `material_data_table.rs` 的 `init_state`、`painting_text_painter.rs` 的 `_compute_caret_metrics`、`rendering_animated_size.rs` 的 `perform_layout`)、拒绝 33、可达 69、0 error;run937 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws938/939 | **同一条继承链上参数不一致时,方向应该是「都擦」而不是「都不擦」——但只在两边都担得起的时候**。`_InheritedProviderScopeElement<T> implements InheritedContext<T>`:下面被真实流点标了(`element = this` 落进 `_InheritedProviderScopeElement<T?>` 的槽),上面没标,原规则把两边一起丢掉,于是 provider 六个成员手里是 `X<T>`、声明写的是 `X<T?>`。先试「一律往上抬」(ws938):清掉那 6 个,却因为把 `Animatable.T` 也擦了而**新增 7 个**(`Tween.lerp`、滑块 demo 的 `paint`……),净 +1,**撤回**。加一道闸再来(ws939):**只在这个位置上每一个传自己参数进来的子类都已经标了的时候才抬**——`InheritedContext` 只有一个子类且已标,`Animatable` 有 `TweenSequence`/`_ChainedEvaluation` 没标,于是只抬前者。`DART2RUST_RAISE=0` 退回原来的丢弃 | stub **119 → 113**(逐条比新增 **0**,少的正是 provider 的 `build`/`mount`/`unmount`/`update` 那一族六个)、拒绝 33、可达 69、0 error;run939 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws940 | **被树摇空了的增强枚举,现在照样发出它的变体**。原来的规则是:增强枚举(每个变体带自己的字段)如果那些字段的值从常量里读不回来,就整个发成空枚举——理由写着「不然就会把它当普通枚举发、把成员丢掉」。可是空枚举**把成员和名字一起丢了**,而且是悄悄地:`enum KeyboardLockMode {}`,于是 `KeyboardLockMode::NumLock` 指着一个不存在的变体、`Set<KeyboardLockMode>` 连 `DartEq` 都没有。变体发出来之后,只有真去读那份状态的成员编不过,而编不过就是一个桩——看得见,一个一个数得清。`valueFields` 仍旧是空的,所以那份状态不发 getter | stub **113 → 112**、拒绝 **33 → 32**(`KeyboardLockMode.findLockByLogicalKey` 从「拒绝」变成一个桩,`handle_key_event` 和 `_should_accept_num_lock` 两个桩清掉)、可达 69、0 error;run940 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws891 | 第五轮复审抓到:ws889 落地后 `regen.py` 的 `hints()` 会**永远误报**——它按 `'throws:' not in driver` 字面判定,而正确的修法恰恰是把那个参数删掉,所以那条提示从此指向唯一不该做的修法。换成 `DECIDED_IN`:只指出决定写在哪两个函数里,不对文件的现状下任何断言。顺手分开探针的两种失败(没调用 vs 调用了没 `?`) | `fails:` 在 frontend.dart 已 11 处、`throws:` 在 kernel driver 已 0 处——两条提示一条正确变哑、一条永久说谎,实测属实;两个诊断分支各跑一次验过 |

## 下一步(2026-09-05 重铺)

本节和〈当前队头〉原来停在 2026-09-03 目标改写时(`crate.py` 的 416 个错误、
老 census 的类别表),早已对不上,作废重铺。活账是上面的 ws 表,队头以表末
(ws350:**3284 stub / 804 refusal / 782 `todo!`**,138+1 个 crate 全到)为准:

原来的六条(`todo!` 剩员、refusal 归并、Result 记账债、`Rc<dyn Fn>` 的
`PartialEq`、`dynamic` vs `Object?`、运行时 0/168)正文移出本文,git 有;
两处校注就是它们的现状,列在下面。

**loop 已停**(cron `5435ce19` 已删)。下次继续时环境变量见上面那节。

**不做**(量过的,仍然算数):nightly 的并行前端(第 65 轮,对名字解析无效);
按 SCC 拆 crate(第 40 轮,库图只允许并行两个);翻译 `dart:core`(第 44 轮,
+10347)。


**(2026-09-07 校注)** 六条的现状:1(`todo!` 剩员)此后再未量,勿引用 782 为新数。
2(refusal)804 → **252**,一直在按类别收。3(Result 记账债)仍挂:函数值调用不
参与传播、原语不进 Result;事件循环现在吃掉回调异常并报告(2593df32),债变为
「报了但没人 catch」。4 **已解**(ws438 起集合相等一律 `DartEq`)。5 前半**已解**
(ws497:`Object?` 即 `dynamic`);RegExp 仍无引擎(暂未在路径上);多实现体的
trait 泛型方法**已解**(擦除孪生 `m__erased`,ws482/ws494)。6 仍是 **0/168**,
但 `runtime/` crate 从 09-06 起存在(无头引擎层)。

**(2026-09-09 校注)** 本节六条与〈当前队头〉自 09-05 起未动,现状以〈活账〉窗口
末行为准:**ws878 152 stub / 49 拒绝 / 64 crate 全可达**,运行尺子 run877
**708 行对 708 行、0 类型差异、0 panic、197 帧**。六条里 1(`todo!` 剩员)仍未再量;
2(拒绝)804 → **49**;3(Result 记账债)仍挂,并且长出了新的一面——见〈已知欠账〉
里 99.05% 的函数带着 `Result` 而只有 6.9% 会失败;4、5 已解(见 09-07 校注);
6 的 `runtime/` crate 存在且已是无头引擎,`Dart_*` 仍 0/168。
**队头现在不是一张类别表**:剩下的 152 个 stub 是长尾(最大的一个形状只有两个成员),
52 个拒绝里大半是四个「决定」而不是四条规则——见〈已知欠账〉的前两条与〈章结〉。

## 当前队头(2026-09-05)

两半,两把尺子。

**运行时那半(`bin/embedder_api.py`,engine `0c2d270c5a9`)** ——不变:

| 数 | 是什么 |
|---|---|
| 168 / 312 | engine 真正调用的 `Dart_*`,945 处调用点(上界,还没收到启动路径上) |
| 231 | `dart:ui` 的下行 native |
| 19 | `PlatformConfiguration` 的上行句柄 |
| **1 层** | Rust 这边实现了的——`runtime/` crate 从 2026-09-06 起存在：无头引擎，见〈第二个 goal〉；`Dart_*` 仍是 0 |

**翻译那半(dill `0700f1e5`,`gen_kernel --aot --tfa --minimal-kernel` 出的
sig dill,前缀 `package:,dart:ui`,931 个库;尺子 `bin/stubs.py`,峰值 4–19 GB)**

`ws350: 3284 stub / 804 refusal / 782 todo!`,轨迹见〈数字轨迹〉。

三个数各量一样东西:**stub** 是「译出了、编不过」的函数;**refusal** 是
「没译出」的函数;**`todo!`** 是「编得过、一跑就 panic」的转发器体——
ws344 才照到它,一量 26199 个,削到 782。


## 九条要记住的

1. **"零拒绝"(781 / 2743)不等于"翻译好了"**:引用其他库的东西仍靠手写桩。
   2026-09-03 目标改写之后这一条不再只是提醒,它就是新的验收:**跑得起来**。
2. **`testdata` 的桩已经吃力**。真正的答案是 `dart:core` / `dart:ui` 的最小子集
   ——**写出来,不是翻译出来**(第 44 轮量过:翻译 `dart:core` 让错误从 6608
   涨到 16955)。那份东西现在有了名字和位置:`runtime/` crate,即那个 plain 的
   Dart VM。
3. **哪把仪器守哪条守卫。** 断言、rustc、前端对照各有盲区。
   第 25 轮又添一处:`fixtures.py` 比对前剥掉注释,而**拒绝就是注释**,
   于是删掉一条拒绝规则没有任何检查会变红。现在 fixture 用 `// REFUSES:` 自己声明。
   第 27 轮发现那条新检查自己也有洞:它只看 `// NOT TRANSLATED:` 那一行,
   而**理由写在下一行**——加一条新尺子之后要拿一个真会被拒的东西试它。
4. **短路语义最容易静默出错**(`??` 错了十七轮)。
5. **fixture 里的值要能分辨错误配对**——已栽五次。
6. **变异存活时,先查我改的那行和测试断言的是不是同一件事。**
   变异**被杀**时同样要查:第 25 轮有一个"杀了但死在 dill 构建失败上",
   报错取行改准了才看清。
7. **探针能分对"是什么"却分错"长什么样"。**
   第 35 轮的近亲:**一条新分支可能排在够不着的位置上**。analyzer 的
   `expression()` 早就把 `PropertyAccess` 交给了 `_property`,写在它后面的
   检查永远不运行——我猜了三次守卫条件,而问题根本不在条件上。
   和第 24 轮 Kernel 侧 `throw` 那次同一种。**"没匹配上"先查排序,再查条件。**
   第 28 轮的近亲:**一条拒绝的理由可能已经不成立了**。"合成变量"当初拒绝
   是因为没东西能称呼它;上一轮给它起了名字之后,那条拒绝就只是没人回头看。
   加了新能力之后,回头查一遍还有哪些拒绝是靠旧前提立着的。
8. **拒绝数升高可能是好事。** 第 9 轮(私有成员)和第 24 轮(构造函数体)
   都是把静默丢弃换成明确拒绝,数字变差而尺子变准。
   **报进度时要说清是哪一种。**
9. **用正则当尺子时,先看一条它匹配到的东西。** 第 53、54 轮:
   `^// NOT TRANSLATED: <标识符>$` 被当成"整个类被拒",而它也匹配
   `animation_super_to_string` 这种自由函数。真实数字是 0,报出来是 157。
   一条原始输出就能拆穿,而我隔了一整轮才看。

10. **一个变异必须还编得过。** 让编译器自己编不过的变异什么也没证明。
   第 29 轮和第 31 轮各撞见一次:删掉一个分支会留下未用的变量或死代码里的
   类型错误。**改答案,不要改得编不过**。
11. **数字必须带量它的条件。** 第 24 轮记的 `10040` 没写 dill 和前缀,
   第 25 轮想比时发现任何组合都复现不出,那一轮的进度记录就此作废。

## 改编译器自身时,这一轮是怎么验的(方法)

改编译器自身而不改规则时,读数相同不够——152 个 stub 可以来自不同的 152 处。
这一轮的验法记在这里,以后同类的改动照抄:

1. **链子**:152 stub / 49 拒绝 / 64 可达,且 stub 集与 ws878 **逐条**相同
   (`diff <(f stubs878.txt) <(f stubs880.txt)` 无输出)。
2. **逐字节**:在 `HEAD~1` 和 `HEAD` 各跑一次纯翻译(只跑
   `dart2rust_package.dart`,不跑 cargo),两份 `.crate/src` 对比——
   926 个模块、98,915,019 字节、md5 `4429c8f1`,`diff -rq` 0 个文件不同。
   `dart_prelude.rs` 也在这 926 个里。
3. **对比本身可信**:拿一份复制品往 `dart_prelude.rs` 加一行,`diff` 报得出来。
4. **表**:四张变异名表并进 `member_names.dart` 后,用 Dart 打印出来跟
   `git show HEAD:` 的原表逐名对,51/34/22/22 全等。
5. **fixture**:`cmpscalar`、`idhash` 用仓库里的新 `bin/fx.sh` 跑,都 AGREE。

纯搬运的轮次(ws881、ws882)只跑第 2 步就够:`.crate/src` 逐字节相同,
而 `workspace.py`、`stubs.py` 和 cargo 都是它的确定性函数,所以 152/49/64
是推出来的,不是又量了一遍。哪一轮改了规则,就得整条链子。

一个教训:`operatorTraits` 那次改名落在链子的翻译阶段**之后**,所以链子那个
读数当时并不覆盖它;是单独把 `src_ws880` 和 `src_head` 对了一遍才补上的。
翻译阶段(`wrote .crate-ws` 出现)之后再改 `lib/*.dart`,虽然不会打断链子,
但那次读数就不算数了。

## 拆 god class 的结论

**评审把 #1 和 #2 的顺序写反了。** 量出来的:`RustBackend` 的 7 个分节共享
69 个字段里的 42 个,`KernelFrontend` 的 6 个分节共享 77 里的 35。所以在把
隐式可变字段改成显式状态之前,分节**不可能**成为独立协作对象;mixin 也不行,
分节之间是有环的(Expressions ↔ Statements ↔ The class),要用 mixin 就得给
几百个跨节成员补一份抽象声明,等于把「同一份知识两处写」再犯一遍。

能做且已做的是另一半:**文件拆开,类不拆**。`augment class` 把一个类摊到一个
目录的 part 里,按文件本来就有的分节注释切,搬运时一个字符没改。
`lib/backend_rust.dart` 12,936 → 一个目录 18 个文件;
`lib/frontend_kernel.dart` 15,749 → 一个目录 16 个文件。

第一刀按文件本来就有的分节注释切,剩下两个 5,000 行以上的;
第二刀(ws882)切进它们内部,边界自己定,取在成员声明处,
每个 part 顶上写一行说明它装的是什么。**没有一个 part 超过 1,850 行**,
最接近的两个:`frontend_kernel/expression_raw.dart` 1,717 行(就是
`_expressionRaw` 一个方法,再切要动代码)和 `backend_rust/the_class.dart`
1,841 行(还没细看)。

第二刀顺手量出一件事:`// -- Failure in the return value --` 这个标题下面
2,841 行,只有头 295 行是讲失败的,其余 2,546 行是整个类的发射器——
分节注释被它下面长出来的东西甩掉了。现在是 `emit_struct.dart`、
`emit_impl.dart`、`emit_members.dart`。

**代价,写下来而不是等人踩:**

- `augment` 在 `--enable-experiment=augmentations` 后面,dev SDK 上的实验特性。
  每一个跑这个编译器的 `dart` 都要带,`bin/experiments.sh` 是唯一命名它的地方。
  SDK 哪天去掉这个特性,build 就停;退回去是机械的(把 part 拼回一个文件、
  去掉 `augment class` 壳)。
- **`dart format` 读不了 `augment class`**,对这 11 个文件 exit 65。也就是说
  `dart format lib` 会报「Formatted 29 files (0 changed)」然后什么都没做,
  而 pre-commit 会直接挡住任何碰这些文件的提交。`bin/fmt.py` 是入口:
  把关键字从副本上摘掉、用同一个 SDK 的同一个 formatter 跑、再放回去。
  `bin/check.sh` 和 `.githooks/pre-commit` 都走它。dart_style 哪天认了
  `augment`,把 `bin/fmt.py` 删掉换回 `dart format` 就行。

## 隐式状态:量出来的形状

评审说「隐式可变上下文是自招的 bug 农场」,点名 `ws478 一个拒绝把
`_expectedReturn` 留给了下一个成员`。量下来,这个病有个很具体的形状,
而且两半的答案是相反的:

- **前端**的逐成员 `catch`(`declarations.dart`)**什么都不还原**,所以它
  靠的是每一处自己的 `try/finally`——24 处,一处不漏。
- **后端**靠中央回滚 `RustBackend._member`,注释写着「every scrap of state
  a member's emission sets」。**但它是一份手写名单,只列了九个**,而到
  2026-09-09 已经有 13 个字段在别处被加进来没同步:`_asyncBody`、
  `_boundByValue`、`_cellLocals`、`_closureCaptured`、`_fallsOff`、
  `_inFlowClosure`、`_inSuperFn`、`_lateCellLocals`、`_lendingClosure`、
  `_methodTypeParams`、`_returns`、`_selfBinding`、`_spellsReturn`。

补全之后 **生成的 Rust 一个字节没变**——所以这 13 个是**潜在**的,不是正在
生效的:今天 gallery 里没有哪个拒绝落在会让它们被下一个成员读到的位置。
修是对的(它是个陷阱),但不能说它修好了什么。

`bin/statecheck.py` 现在同时管住两条规则,进了 `bin/check.sh`。它管不到的
一件事写在它自己的文档注释里:**在一个成员内部**被接住的拒绝,仍然会跳过
后端某个 scope 自己的还原,而守卫要到成员结束才跑。今天没有这种写法;
真出现了,后端那些 scope 就得**同时**有 `try/finally`,而不是二选一。

## 已知欠账

**(2026-09-09 量出来的三个数,ws911)**

- **一次完整启动里,整表复制发生 86259 次、复制了 440334 个元素**
  (`DART2RUST_COUNT_COPIES=1`,run924):

  ```
  Vec::dart_to_list   calls=71735  elements=426869  longest=119
  Vec::iterator       calls=14524  elements=13465   longest=1
  Set::dart_to_list   calls=0      Set::iterator    calls=0
  ```

  平均一张表 **5.95 个元素**,最长 119。元素几乎都是 `Rc`,复制一个是一次
  引用计数加一。**这是把 `List<T>` 从 `Vec<T>` 换成 `Rc<RefCell<Vec<T>>>`
  之前要拿到的数**:它说这些复制是四十几万次引用计数,不是热点,所以那个
  改动要靠**语义**(别名写得回去、`identical` 对列表不再恒假)来立论,
  不能靠性能。

- **渲染树尺子抖了十几轮,根因找到了,尺子现在不抖了(ws934)。**

  根因是**一条 `==`**:`impl DartEq for dyn Trait` 后端写的是
  `std::ptr::addr_eq`——按地址比。Dart 的 `==` 是**派发到对象**上的,
  `Key` 的每个子类都自己重写了 `==`。`_MaterialAppState._buildWidgetApp`
  建的是 `WidgetsApp(key: GlobalObjectKey(this))`:两次 build 造出两个
  `GlobalObjectKey`,包着同一个 state,Dart 说相等,这里说不等。于是

  ```
  Widget.canUpdate(old, new)  ->  runtimeType 相同,key 不等  ->  false
  Element.updateChild         ->  deactivate + inflate,整棵子树重建
  ```

  在 `element_super_update_child` 的重建分支上挂一个**静默**普查(累加到
  `thread_local`,预算用尽时打一行),一次跑得到:

  ```
  PROBE reinflate total=291
    [WidgetsApp -> WidgetsApp tyeq=true keyeq=false okey=GlobalObjectKey nkey=GlobalObjectKey x193]
    [SizedBox -> Semantics x97]
    [SizedBox -> UnmanagedRestorationScope x1]
  ```

  **193 次**。每重建一次,下面就有一个新的 `_LocalizationsState`,它的
  `_locale` 又是 null,`build` 返回 `SizedBox.shrink()`——树就只剩 6 行,
  直到那一份代理加载完(`LocaleNamesLocalizationsDelegate.load` 在坏的那次
  正挂着,好的那次不挂)。所以树是**来回摆**的,不是「建不起来」:

  ```
  DART2RUST_TREE_EVERY=50   frame 50 707 / 100 6 / 150 6 / 200 707 / 250 6
  ```

  预算到期时 dump 落在哪一相,读数就是哪一个。改完之后连采五次:
  **707 行 / 类型差异 0 / 0 panic,五次一模一样**;而且程序现在会**自己跑完**
  (6 帧后空闲退出,`RUN-DONE exit=0`),不再是每帧重建整棵树撑到 432 帧。

  **这条线索上此前记错的三件,一并更正:**

  1. 「那一次跑的整整 380 帧里,树一直是 6 行,从来没到过 707」——**错**。
     那个探针是每 20 帧走一遍树,开销本身把它压住了;换成每 50 帧就看得见
     707 和 6 交替。
  2. 空树的那个 `SizedBox` 不是 `_RootRestorationScopeState` 的。它的
     blank 分支在好坏两次里都只走 **1** 次(静默计数器量的)。空树的第 5 层
     是 `_LocalizationsState.build` 的 `SizedBox.shrink()`——比它低四层。
  3. 「一次跑里有 197 个不同的 `_RootRestorationScopeState`」——数是真的
     (以 `Rc::as_ptr(&self._root_bucket_valid)` 作身份,约 194 个),但**好坏
     两次一样**(好的那次 194/193),所以它是同一个重建的**后果**,不是
     区分好坏的那一条。

  **量这种东西必须用静默探针**:每次 build 打一行(196 行)会让 7/7 次都变空,
  而同一个二进制不打就是六成满。计数器累加在 prelude 的 `thread_local` 里,
  预算报告时打一行——这条已写进 memory。

  (`DART2RUST_TREE_EVERY` 和预算用尽时的「还挂着哪些 future / 哪些 completer
  没完成」都是这次查出来的工具,留着。)

- ~~**provider 的擦除账还剩 6 个桩**~~ —— ws939 清掉了,根因是继承链一致性
  规则的方向(见活账)。下面这段留着,因为它记的是**怎么找到的**:
  ws934 的「擦除过的实例化在槽上 cast 回来」让
  `widget.owner._delegate()` 这一步过了(`dart_cast_to::<dyn _Delegate<T>>()`,
  对象的 `dart_cast` 本来就答得出这个 `TypeId`),但下一句立刻换了个错:

  ```
  cascaded.set_element(Some(self.dart_self_ref().get()))
  expected Rc<_InheritedProviderScopeElement<<T as DartNullable>::Or>>
  found    Rc<_InheritedProviderScopeElement<T>>
  ```

  也就是**投影(`Or`)和裸参数在同一个类上混着用**。这是另一条规则,不是
  这一条;6 个桩(`value` / `unmount` / `build` / `update` / `reassemble` /
  `mount`)全是同一形状,清起来应该是一次。

- **`erasedread` 夹具还没真正复现那条规则。**两端 AGREE,但生成出来的
  `Owner<T>` **没有被擦除**——协变扫描确实标了它
  (`TRACE_COVARIANT_SITE Owner<T> ... Owner<int> -> Owner<Object>`,
  `TRACE_COVARIANT Owner <T>`),而 `_erasedParameter` 仍答 false,原因没查出来。
  所以这条规则目前的证据是**链上的桩数**(2 个清掉、0 个新增)和
  provider 生成代码里那句 cast,不是夹具。夹具欠着。

- **`Rc<Self>` 做 `dyn` 接收者是合法的,验过了**(work.md 第 3 条动手前的
  那个「别假设」)。十行 Rust,`rustc --edition 2021` 直接过并跑出 `ab/3`:

  ```rust
  trait DartIterable<T> { fn iterator(self: Rc<Self>) -> Rc<dyn DartIterator<T>>; }
  impl<T: Clone + 'static> DartIterable<T> for RefCell<Vec<T>> { .. }
  let handle: Rc<dyn DartIterable<String>> = xs.clone();   // 零拷贝的 unsizing
  ```

  同时验到的三件:`Rc<RefCell<Vec<T>>>` 到 `Rc<dyn DartIterable<T>>` 是
  unsizing,**一次复制都没有**;迭代器 `current` 里「借一个元素、立刻放手」
  写得出来;别名 `borrow_mut().push(..)` 之后另一边 `borrow().len()` 看得见
  ——就是那个语义论点。所以第 3 条的三个前置里,这一个已经清掉。

- **`gallery_above` 那 42 秒是冷缓存,不是 crate 重**:`touch` 它的源码后
  连续两次 `cargo build -p dart_main` 是 **8.1 秒 / 7.8 秒**。所以「给大库
  分子模块」这条不要做,该想的是别让 build 那份增量缓存每轮都作废
  (链子重写了全部源码)。

**(2026-09-09 新增,ws908 自己换出来的)**

- **每一次装进 `Iterable` 槽都克隆一份列表**:`!as_iterable` 发的是
  `Rc::new((x).clone()) as Rc<dyn DartIterable<T>>`。那个 `.clone()` 是为
  借来的值加的——`&Vec<T>` 的形参、`&Set<T>` 的字段读——句柄要拥有它手里
  的东西,而后端在这一处分不出手里是值还是借用。生成代码里有 660 处
  `dyn DartIterable`,其中多数的接收者本来就是一个刚做出来的临时值,这份
  克隆是白花的。要去掉得让后端知道一个表达式是不是借用(`_borrowed`
  这类判定现在没有),不是这一轮的活。**没量过它值多少**。

- **界是 `Iterable<E>` 的类型参数,声明按界拼、调用按实例化拼**(ws909
  量出来,夹具 `iterablebound` 红)。`_type` 见到 `T extends Iterable<E>`
  就拼成界(`types.dart:176`),而界现在是 `Rc<dyn DartIterable<E>>`;可是
  实参的槽是把 `T := Set<E>` 代进去之后的 `Set<E>`,于是 `coerceInto` 觉得
  两边同型、原样放行,发出来的却是「`Set<String>` 塞进 `Rc<dyn
  DartIterable<String>>`」。`collection` 的 `_UnorderedEquality<E, T extends
  Iterable<E>>` 就是这个形状(`equality.rs` 的 `hash`/`equals`/`new` 三个
  桩)。夹具已经写好并且是红的:`SetUnordered`/`ListUnordered` 各一次,
  `Set` 和 `Vec` 都没被装箱。修法是让实参的槽跟着声明走——参数的声明类型
  是一个「按界拼」的类型参数时,槽就是那个界,不是实例化。

**(2026-09-09 新增,ws885 自己换出来的)**

- **两个 trait 同名成员,在 trait 体里调不了**:ws885 把 super 函数要求的
  额外 trait 放到 trait 头上之后,`RenderObjectWithLayoutCallbackMixin`
  同时在 `RenderObject` 和 `RenderBox` 之下,`this_.constraints()` 就成了
  E0034(`widgets_layout_builder.rs` 的
  `render_abstract_layout_builder_mixin_super_layout_callback`,这一轮清掉
  三个换来的一个)。挑限定名的机制**已经有**——`_accessorQualifier` 正是
  「链上有两个声明才返回」的判定,`calls.dart` 里只在闭包句柄
  (`_selfName == _countedSelf`)上用它。要补两处:把这个判定也用在
  trait 体里的 `this`(`_fieldsAreAccessors`),并让 `_supertypesOf` 认得
  ws885 新加的那些父 trait(`_superBoundTraits`),否则链上找不到第二个
  声明,判定返回 null。是下一轮的活,不是这一轮的收尾。

**(2026-09-09 新增,ws880 由分析器查出,三条都还没量)**

- **`super.<async 方法>()` 没有 await**:`_superCall` 算出了 `isAsync`
  ——注释写着「async 的 super 函数是 `async fn`,调用方的 trait 要的是
  盒装 future」——然后**这个值一次都没被读**,函数直接 `return call;`。
  也就是说至今每个 async 的 super 调用都是当同步调的。改它会动到每一个
  async super 调用点的输出,是独立一轮。位置:`lib/backend_rust.dart`
  的 `KNOWN GAP` 注释处。
- **变异名表的三处缺口**:四份表并进 `lib/member_names.dart` 时量出来的
  (原样保留,没有顺手补):`_ThisWriteFinder` 缺 `removeRange`、`updateAll`
  和全部 `ByteData` setter,所以 `this._x.removeRange(..)` 不算对象的写;
  `_inPlace` 缺 `update_all`。补任何一条都会加宽哪些方法拿 `&mut self`,
  要过链子。`test/member_names_test.dart` 把这三个差集钉死了,补的时候
  测试会红,和量数的那次提交一起改。
**(2026-09-09 新增,ws886 量出来的)**

- **内核前端已经走在前面 16 个 fixture**:预言机熄火的那段时间,内核那侧
  接着走了几百轮。重新点着的那一小时量的:33 个里 14 个逐字相同、3 个
  已声明差异(`// DIFFERS:`)、16 个是内核侧领先——构造器的
  `let mut __new` 与 `dart_register`、字段初始化的 `.clone()`、参数上的
  `mut`、闭包拿到类型、调用处的擦除拆包。没有一条是两边对 Dart 的理解
  不同。名单钉在 `bin/fixtures.py` 的 `BEHIND` 里:只允许变短;变长、或者
  名单上的名字自己追上了,这个工具都会失败。补它们是一条队列,不是一轮。
- **`branching` 的 `// REFUSES:` 过期了**:声明「case 中间的 break 一律
  拒绝」,但 CFE 把 switch 包成 `LabeledStatement`,内核前端照 `IrLabeled`
  译成 `'__l0: { match .. break '__l0 .. }`——是对的,读过生成的 Rust。
  熄火期间长出来的能力,没人看见。已改成 `// DIFFERS:` 并写明两侧各自
  怎么办。分析器那侧仍然拒绝。
- **86 条 analyzer 警告/提示**:抽查三处 dead_code,都不是 bug,是 kernel
  API 收紧可空性后剩下的多余守卫(`node.interfaceTarget` 不再可空、
  `instantiate` 必返 `FunctionType`)。删掉不改变任何输出,但那是生产前端
  里 40 处编辑,自己占一轮。`bin/check.sh` 的 `analyze_ceiling` 钉住这个
  数:只能降,降了要在同一个提交里把它改小。
- **第三把熄了的尺子:`testdata` 那个 cargo crate**。`src/lib.rs` 里
  **146 个 `#[test]`** 是全项目**唯一**断言「运行时的值和 Dart 一样」的一层,
  至今全黑。三个独立原因(ws886 只看见第一个就写成了「根因」,ws887 更正;
  叙事在 git):
  1. `src/lib.rs` 那 1,894 行手写桩加测试停在「方法不返回 `Result`」的年代,
     `Asserts::new(8.0).halved()` 成了在 `Result` 上找方法——196 个 E0599 的大头。
  2. **两个 fixture driver 结构性地生成不出 `?`**(包驱动传 16 个具名参数,
     fixture driver 传 2 个)。**ws889 修了,而且是反着修的**:该删的不是
     driver 的沉默,是 `_fails` 开头那句 `if (throws == null)`——统一模型下
     「会不会 throw」不决定任何事,那个参数里没有信息,只有一个开关。
     复审两轮(它一次,我一次)都读成「driver 必须建那个分析」,不必。
  3. 黄金里另两个形状(const 上下文的运行时 downcast、trait 方法里的裸
     `Vec<Object>`)在 gallery 输出里都是 **0 次**,多半是同一个残缺配置的产物。
  **验收标准写死在这里:crate 编过 + 146 个 test 跑绿,不是错误数降低。**
  ws886 按「139 错 vs 79 错,取较优者」收下 6,300 行黄金,而 79 那批在 `?` 的
  形状上是**退步**——一个「只许降」的棘轮会照样收下它,已全部退回冻结版。
  `bin/regen.py` 开头因此跑一个**行为**探针而不是字面检查:真跑这一轮要用的
  每个 driver,输出里那个必失败的调用不带 `?` 就整体拒绝重生成(21 秒,当场
  重现 `Ok((self.checked(value) * 2.0))`)。ws887 那版查
  `'fails:' not in frontend.dart`,两行 TODO 注释就能把门打开(ws888 实测),
  而那正是修这件事的那一轮第一个会写的东西。`--anyway` 改成写进不在 git 里的
  `.agree/anyway/`。
- **黄金层重新点着了,它第一件事是报出 7 个真缺陷(ws890)**。32 个文件由
  一个「能发 `?`」被行为探针证明过的 driver 生成,预言机同时是绿的,44 个
  错误全部在**生成的代码**里——`lib.rs` 一条都没有。逐条读完,归成 7 个根因:
  1. **const 初值里生成运行时 downcast**(`constinstance`,24 条):
     `3.0.as_any().downcast_ref::<f64>().unwrap().clone()` 写在 `const` 里。
     复审第三轮点名的两个形状之一,**修好 driver 之后仍在**,所以它是真的。
  2. **闭包字面量传进 `Rc<dyn Fn>` 槽没包 `Rc::new`**(`closures`,7 条)。
     也可能反过来:那个槽本该写成 `impl Fn`。同一文件里另一处包了。
  3. **泛型 trait 方法的 erased 变体把槽写成裸 trait 名**(`generic`,4 条):
     `items: Vec<Object>`、`ignored: Object`。复审点名的另一个形状,也是真的。
     gallery 里 0 次,因为包驱动传 `erase: true` 而 fixture driver 什么都不传
     ——**所以它同时是一条配置欠账**,见下。
  4. **`RangeError::new` 拿到 `String`,而签名要 `Rc<dyn Object>`**
     (`trycatch` 2、`failure` 1、`control` 1)。
  5. **setter 返回 `Ok(dart_null_object())`,签名却是 `Result<(), _>`**
     (`setters`,2 条)。
  6. **`Map.get` 的借用**(`lists`,2 条):`sizes.get(name)` 少一个 `&`,
     而且返回 `&i64` 填进要 `i64` 的位置。
  7. **`match` 在表达式位置上默认臂是空的**(`branching`,1 条):
     `_ => {}` 给出 `()`,而那个 match 要 `Result<f64, _>`。
- **顺序:driver → 黄金 → 编译器缺陷 → `lib.rs`**(ws890b 学到的,比 ws887
  写下的多一格)。`lib.rs` 那 317 个错是机械的,编译器 span 驱动的脚本半小时
  能改到 102 个;问题是**改完没有任何东西能检查它**——那 289 行的唯一裁判是
  146 个 `#[test]` 真的跑起来,而它们被上面 7 个缺陷挡着。抽查已见坏编辑
  (`.collect.unwrap()()`、给一个 `Map` 加 `.unwrap()`)。**没法验的批量改动,
  提交进去就是「按计数验收」换了身衣服**,所以整批回退了。脚本留在 scratchpad
  的 `unwrap_fix3.py`/`move_unwrap.py`,证明这段路是通的。
- **fixture driver 的配置仍然不是生产配置**。`KernelFrontend` 的 17 个具名参数
  里,包驱动传 15 个,fixture driver 传 2 个。`?` 只是第一个症状(ws889 修了,
  而且是把那个假开关删掉),`erase` 是第二个(上面第 3 条)。剩下的还有
  `typeEnvironment`、`coerceByType`、`covariantParameters`、`dynamicSlots`、
  `open`、`instantiations`。
  **这里有个要定的事**:让 fixture driver 对齐生产配置,和「两个前端逐字节可比」
  是冲突的——分析器前端没有 erasure、没有 `typeEnvironment`。可能的答案是把两件事
  分开:黄金只从 Kernel 侧的生产配置生成,`fixtures.py` 继续用最小配置只做比较。
  这会改变黄金层的定义,所以先写下来,不顺手做。
- **dill 的 body 什么时候读,决定这个编译器输出什么(ws889 发现,机制未明)**。
  `loadComponentFromBinary` 把函数体留在 `lazyBuilder` 后面。同一个 dill、同一份
  源码,只差「lowering 之前有没有把 body 全读完」,926 个模块里 **32,653 行不同**
  ——32,026 行只差一个 `?`(读完才有),另外 627 行更宽(限定调用与 turbofish 的
  两种拼法)。**为什么会差,还不知道。** ws889 只做了三件事:复现(把
  `ThrowsAnalysis.of` 换成空 `RecursiveVisitor` 走完 130,133 个成员,输出回到
  e69150fe)、改成显式 `BinaryBuilder(disableLazyReading: true)`、写在这里。
  **ws885 以来每一次「逐字节相同」量的都是「先读完」那一版**,而在 ws889 之前,
  替它读的是一个答案没人读的分析。下一步:先定哪一版是对的,再问为什么。
- **值级断言的覆盖已塌到 6 个**:146 个黄金 `#[test]` 全黑之后,唯一
  「对 Dart 真值」的尺子只剩 `bin/fx.sh` 的 6 个 fixture,而且要手跑。
  `fixtures.py` 对「两个 driver 以同样方式配错」结构性失明(它们一致地
  错,所以比较不出来);渲染树 md5 只发现**变化**,发现不了**错误**。
  排 BEHIND 那 16 个的时候顺手长出 fx 候选,并想办法把 fx 进 `check.sh`。

- **RegExp 无引擎**(intl 的某些路径依赖;目前没踩到)。
- **typed_data 共享 buffer 视图**:`_eightBytesAsList` 是拷贝不是视图,
  `putUint16/32/Float64` 写进视图的字节从 buffer 读不到(run509 记)。
- **RefCell 重入**:cell 的借/还规则是按形状打的,重入会 `already borrowed`;
  每修一处都是把读侧的 `Ref` 提前放掉,没有一般性保证。
- **`Dart_*` 0/168**:无头引擎只是第一层,真 engine 的桥还没开始。
- **`Layer.find<S>`**:出参 `AnnotationResult<S>` 经擦除孪生——run646 起用协变分析+隐藏
  `__ty_i` 解掉(见活账 run646);仍欠:方法体把自己的 `S` 作**类型实参**传给带隐藏值的
  方法(`find<S>` 里的 `findAnnotations<S>`)只有走静态派发(单体)时 S 才是真的,经 `find__erased`
  时是 Object——要把「作类型实参」也算值用法并做定点。
- **泛型 mixin 的 `T?` 字段 getter**:`Or` vs `Option<T>` 的投影差(run555 记)。
- **无 `==` 的值类做键**:Dart 按身份,值 struct 按结构;AOT 还会删未读字段
  (run558 记,不在路径上)。
- **Goal 3 差 `size=`**:渲染树类型序列已 6/6,dump 时拿不到 layout 结果。
- ~~**727 个 stub 的长尾**~~(2026-09-09:**149**):最大类是 "mismatched types"(约一半),其余是参数数、
  注解、闭包形状等;随运行尺子推进逐站收。
- **擦除泛型的静态类型失真**:擦除 trait 的方法返回 `Elem<C>` 时 Rust 给的是
  `Elem<Rc<dyn Constraints>>`,而 Dart 侧局部声明为 `Elem<BoxC>`——`let e: Rc<Elem<BoxC>>` 对不上
  (atbounds 夹具第一版踩到,改夹具经抽象基类绕开;gallery 里 `createElement` 返回
  `RenderObjectElement` 所以没踩)。通用解还没有:要么槽也按擦除样子拼,要么擦除处不擦。
- **窄化覆盖的 trait 重声明**:`Builder<L>` 覆盖 `Builder0.makeRender` 把返回窄化成 `Render<L>`
  时 trait 里重声明了一份;gallery 的 `AbstractLayoutBuilder.createRenderObject` 却没有。
  两者差在哪没查(run644 记)。
- **`List.last = v` / `first = v`**:prelude 无 `set_last`/`set_first`;且 `TextTreeRenderer` 里写的是
  字段列表的**克隆**(同「值类字段集合就地改」欠账)。ws652 记,不在启动路径上。
- **`sync*` 是急的**:生成器降成收集好的 `Vec`;无界/惰性依赖副作用顺序的生成器会跑偏。
  `async*` 仍拒绝。(run655 记)
- **provider `_DelegateState<T>.element`**:槽是 `_InheritedProviderScopeElement<T?>`,值是 `<T>`——
  泛型值类的 `T` 与 `T?` 实例化在 Rust 里是两个类型,没有一般转换(ws654 记,`build`/`mount` 同因已 stub)。
- **翻译类实现 prelude 异常接口**（`FlutterError implements AssertionError`）时 `is AssertionError`/
  `on AssertionError catch` 不认：`DartCoreAs` 表只认 prelude 自己的结构体（上游 `is AssertionError` 3 处、
  `on AssertionError catch` 1 处）。子类值按父类读是**转换出来的拷贝**（`RangeError` → `ArgumentError{name: None}`），
  子类独有的字段丢了，`toString` 文本保留。（run683 记）
- **「更宽实例」的 impl 只按程序里出现过的实例化生成**(run708 记):`addWiderImpls` 走
  `instantiations` 普查,泛型自由函数体内构造的类(`animatable_super_chain` 里的
  `_ChainedEvaluation<T>`)从不以具体实例化出现,于是 `_ChainedEvaluation<f64>` 没有
  `impl Animatable<Rc<dyn Object>>`,擦除槽上的 `dart_cast_to` 运行期拿到 `None`。
- **列表/映射按值传递**:`f(log)` 里 `log` 是 `Vec` 的拷贝,被调方(或它返回的闭包)往里 `add`
  调用方看不见(throttle 夹具第一版踩到,改夹具绕开)。counted 类有身份,集合没有——通用解还没有。


**(2026-09-09 新增,ws895 量到)`dynamic` 上的 `num` 方法假定接收者是 double**

那条窄化无条件把接收者下转成 `f64`(`n.as_any().downcast_ref::<f64>()
.unwrap().clone().abs()`),装着 `int` 的 `dynamic` 在这里是 `i64`,downcast
拿不到,`unwrap` 当场炸。夹具第一版就是这么炸的,现已收窄到 double。补个
int 分支不够:Dart 的 `(-7).abs()` 是 **int 7**,印 `7`;当成 double 印
`7.0`。要的是按运行时类型的一次真派发,两条臂各自的结果类型也不同。ws895
只收了装箱那条,int 那条此前是桩 panic、现在是 unwrap panic,run896 没走到。

**(2026-09-09 新增,ws902 量到)`x is T` 在擦除过的类型参数上恒真**

`NotificationListener<T>` 的元素是这样写的:

    if (listener.onNotification != null && notification is T) { .. }

发射出来是

    if { listener.on_notification.clone();
         notification.dart_cast_to::<dyn Notification>().is_some() } { .. }

两处都错。`is T` 里的 `T` 被擦成了它的界 `Notification`,于是**恒真**:
`ScrollMetricsNotification` 走进了只接 `ScrollNotification` 的监听器,
`EditableText.build` 的闭包在里面 `unwrap` 了一个 `None`——run903 就死在这里。
另一处是 `f != null` 变成了一句被丢掉的 `f.clone();`,这里被非空掩着,但它是
独立的缺陷。

夹具 `iserased` 把它钉住了,而且给的是**错误答案**而不是编译错误:

    class Sink<T extends Note> { bool accepts(Note n) => n is T; }
    Sink<ScrollNote>().accepts(MetricsNote())
    rust true   dart false            //  '$T' 也是:rust Note,dart ScrollNote

零件已经有一半:`_TypeLiteralFinder` **已经**认 `is T` / `as T`
(`visitIsExpression`/`visitAsExpression`),`_typeLiteralParams` 也已经把这些
参数筛了出来;prelude 的 `Type` 有 `id: Option<TypeId>` 这个槽,而
`DartAny::dart_cast(TypeId)` 就是 Dart 的 `is`。

缺的那一半比先前记的大。`_typeArgumentGetters` 只给**抽象/open** 类开
`_typeArg<C><T>` getter——由子类回答「祖先给我填了什么」。而
`_NotificationElement<T>` 和 `Sink<T>` 都是**具体**类:没有子类能回答,实参
只有 `new` 的那一处知道。所以要的是:

 1. 具体类上,被当类型字面量读的擦除参数变成一个 `Type` 字段,由构造器带进来;
 2. 每个 `IrNew` 处补上这个实参(`node.arguments.types` 就在手里);
 3. `Type` 字面量在翻译类上要带 `id`(现在 `Type::of("X")` 的 id 是 `None`);
 4. `IrIs(x, 擦除的 T)` 降成按那个 `Type` 的 id 做 `dart_cast`。

**下一轮做这条**,它就是 work.md 第 2 条(擦除边界)那本账的正脸。

**(2026-09-09 新增,ws900 挖出来的)microtask 从来没跑过,现在跑了**

`widgets/focus_manager.dart` 调 `scheduleMicrotask`。文本扫描一直把这个名字
导成 `crate::dart_ui::_schedule_microtask` —— `dart:ui` 那个的体是
`dart_native("DartRuntimeHooks::ScheduleMicrotask")`,而没有 engine host 时
`dart_native` 对无返回值的原生**直接跳过**。也就是说:到 ws899 为止,
**这台机器上每一个 microtask 都被静静丢掉了**,708 行的渲染树是在那个前提下
量出来的。账本按 Kernel 引用解析(目标是 `dart:async` 的 `scheduleMicrotask`,
落在 prelude 上),队列第一次真的跑起来。

醒来的第一条路就撞上 147 里早就有的一个桩:

    scc_flutter_widgets/src/widgets_focus_traversal.rs  _handle_focus_changed
    E0596  policy.clone().invalidate_scope_data(..)
           &mut self 穿过 Rc<dyn FocusTraversalPolicy>

这是「共享可变 / counted 对象」那本账,不是一行能收的。**下一轮做这条。**
在它绿之前,run_main 是红的(112 行,exit 134),而这不是回退——回退等于把
「每个 microtask 都丢掉」装回去。

**(2026-09-09 新增,ws900 换来的)账本让 widgets crate 变大**

`use` 行从 Kernel 引用写之后,边更真也更多:文本扫少导的名字过去是拿打桩
抵掉的(147 个里有一部分就是),现在导全了,`services -> widgets` 这类真边
把 84 个模块并进同一个 crate——`scc_flutter_widgets` 325,924 → 402,903 行。
它是这台机器上跑得最久的那个 rustc,所以这是一笔要还的账,不是白赚的。
往回缩的办法有两条,都还没量:把 `_climb` 对**别的库**的祖先只读签名不读体
(ws900e 试过,墙钟没动、打桩反而多了 48 个,已撤回);或者把那几条真边
指向的成员从 trait 里拆出去。

(ws901 补记:那三处漏掉的名字已经收了——`_member` 现在顺手读一遍被引用成员
的**签名类型**。Rust 在调用处没有推断:适配一个实参时发射的是被调方的参数
类型(`Rc<dyn Fn(Rc<dyn PointerUpEvent>) -> _>`),命名一个局部时发射的是它的
返回类型,而 Dart 写的是 `var`。代价只有 514 行:widgets crate 402,903 →
403,435,打桩 138 → 135。)

**(2026-09-09 新增,ws894 量过)`dart:ffi` 结构体剩下的一半**

`_abi()` 之后,`widgets_window_win32.rs` 里还剩 11 个拒绝,分两类,**都不是
一个内存模型决定能过的**:

    5  字段读写   _loadInt64/_loadInt32/_loadPointer/_storePointer
    2  #fromTypedDataBase   往 prelude 基类 `Struct` 调超构造
    4  _CallocAllocator / initializeWindowing

从 dill 里读出来的体是:

    viewId => _loadInt64(this.{_Compound._typedDataBase},
                         viewId#offsetOf + this.{_Compound._offsetInBytes})

所以字段读写要的是「`_typedDataBase` 那块字节**共享且可变**」。这个 prelude
里 `Uint8List` 就是 `Vec<u8>`(一个值),装箱成 `Rc<Vec<u8>>` 之后写不进去
——它压在「Dart 的 list 是引用、这里是 `Vec`」那条老账上,不是 ffi 自己的
问题。`Struct.create<T>()` 那条路的基是程序自己新造的 `Uint8List`,把它拷进
一个 cell 里能对;但**只对这一条路对**,别名共享同一块 `TypedData` 的那条
路会静静地答错,而按形状去分辨哪条是哪条就是硬编码。夹具 `ffistruct` 已经
写好并且**是红的**(`_from_typed_data_base` 没有),留着钉住这一半。

后面 4 个的内存来自一个 Windows DLL(`_winCoTaskMemAlloc`),在这台机器上
本来就该死在那个调用处,不该在这里假装有内存。

- **`map(..).toList()` 的双重装箱(ws864,五个候选全部排除,叙事在 git)**:
  `CupertinoDatePicker.build` 发出的链把每个元素装两次——`map` 闭包的体按它
  落进的槽降低(`_withExpectedReturn`),已经交出句柄;而链是按 Dart 的静态类型
  `Iterable<Expanded>` **记录**的,于是并进 `List<Widget>` 时又宽了一次,
  `dart_object` 套在 `Rc<dyn Widget>` 上造出 `Rc<Rc<dyn Widget>>`,其 pointee
  什么都不实现。三个 `build` 停在这里。写过五个候选修法(traithandle 两形、
  mapwiden、链自己的 `rustType`、元素类型经 `toList()`/`toSet()` 带下去),
  **五个都在 HEAD 上就同意**,或者根本不改变发射——`expression()` 结尾那句
  `if (erasedThrough != null) lowered.rustType = erasedThrough` 会覆盖访问者
  记下的一切。两个临时 trace(`DART2RUST_TRACE_ELEMENTMAP`、`_TRACE_CHAIN`)
  照出的事实与假设相反:那个闭包声明返回 `Expanded`,Dart 的静态类型也是
  `Expanded`,所以**元素合并是对的**,该去掉的是闭包**体内**那一次。
  下一次要问的是:闭包自己的返回说 `Expanded` 时,是谁把那次上转放进去的。
  夹具做不出来(见〈What the fixture harness can and cannot reach〉)。

**2026-09-09 补:**

- **值类的身份令牌**:`identical`/`identityHashCode` 的操作数类**普查过 239 个**
  (Object 215、Zone 40、Endian 26、List 25、Color 11、Uint32List 10,长尾是整个
  ThemeData 家族、TextStyle、BorderSide、RenderObject、SemanticsNode…);调 `super`
  进 `Object` 的类 2 个(DiagnosticsNode、Widget)。忠实的解法是一枚构造时赋值、
  **穿过 `clone`** 的隐藏字段——因为这个模型里 clone 不是新对象,是同一个对象在传递。
  代价:239 个类都要加字段、构造函数要赋值、const 实例要 Dart 的规范化值、
  派生的 `PartialEq`/`Hash` 全部改成手写并跳过它。为的是十一个拒绝。
  **量过、写下来了、没动手。**
- **`dart:ffi` 的内存模型**:16 个拒绝在 win32 窗口层背后,prelude 故意只给名字
  不给行为(「拒绝发生在调用点」)。清掉它等于给 `Pointer` 一个内存模型——
  那是「这个编译器承诺什么」的决定,不是它的缺口。
- **夹具的规模**:四组改动做到了要写夹具那一步,四个都无法在夹具里复现。
  `fx.sh` 走 AOT 管线(`FX_AOT=1`,TFA 会跑)且能跨库,所以缺的不是构造方式,
  是**闭世界的规模**——擦除普查和实例化清单由 924 个库共同决定,四个类的夹具
  做不出同样的决定。夹具得更大,不是换个建法。
- **帧时间的斜率**:+0.46ms/帧,线性。运行尺子的下一格该是**计数**而不是采样:
  第 10 帧和第 100 帧各打一次候选集合的条数(element、`_dependents`、各处 listener、
  `_inactiveElements`、活动 timer),线性增长的那个自己会跳出来;
  0.46ms ÷ 每帧增量 = 单条代价,直接分开「走一遍」和「每条深拷贝」。
- **release 体积对照 `libapp.so`**(2026-09-08 量的,尚未进队列,是一个独立决定):
  同一个 `app_aot.dill` 经 SDK 自己的 `gen_snapshot` 出的 x86-64 `libapp.so`
  strip 后 **23.3 MB**(.text 11.1 / .rodata 12.2);我们 release+strip 后
  **143.2 MB**(.text 90.7 / .rodata 3.8),`.text` 8.2 倍,形状是反的。
  已量到的三笔:`.text` 的 20% 是「常量数据当代码写」(O3 之下不减);
  53% 是泛型实例化(O3 之下不变);**124,364 个 `pub fn` 里 123,181 个返回 `Result`
  (99.05%),而前端自己的 `throws` 普查说只有 8991/130133(6.9%)会失败**。
  产物在 `~/dart2rust_build/scratch/`(`target-release/`、`libapp_x64.so`、
  `rel_syms.txt`、`dbg_syms.txt`)。

## The generic class's `runtimeType`

Dart's `runtimeType` names the arguments a class was instantiated at, and
this output printed the bare name -- the one node of 708 the render walk
still spelled differently. `dart_type_applied(base, args)` builds and
interns the spelling (`Type` holds a `&'static str` and the instantiation
is only known at run time), and a generic class's `dart_runtime_type` calls
it with `dart_type_of::<T>()` for each parameter.

It also makes `runtimeType` comparisons what Dart's are: two
instantiations of one class are different types, and the `operator ==`
fast path now says so.

## What the remaining `size=` difference is, and is not

The reference's paragraphs are `Size(47.7, 17.0)`, `Size(97.3, 24.0)`,
`Size(74.6, 21.0)` -- fractional widths, so **real font metrics**, not
`flutter test`'s square-glyph font. Reproducing them needs a text engine
that shapes Roboto; the headless runtime answers no `Paragraph::*` native
at all, so every paragraph measures `Size(0.0, 0.0)` and its zero
propagates into every ancestor's size.

That is the whole of the remaining 510 lines. It is a *runtime* gap, not a
translation one: the node types, their order and their nesting are the
reference's, 708 of 708.

Two further facts for whoever picks this up:

* the native bridge passes an instance native no receiver
  (`dart_native("Paragraph::layout", [width])`), so a host could not model
  per-paragraph state even with metrics to hand -- that is the first thing
  to change;
* `ref_render_walk.txt` (6 lines) is a *first-frame* capture, not a prefix
  of the settled one: our settled walk agrees with it for 5 of its 6 lines
  and then has the subtree the first frame did not have yet.

## The run's own panic: an erased twin of an `async` method

`DefaultProcessTextService.queryTextActions` is the run's first panic (the
app survives it). `MethodChannel.invokeMethod<T>` returns `Future<T?>`;
the erased twin returns `DartFuture<<Rc<dyn Object> as DartNullable>::Or>`
and the cast at the call site asks for `DartFuture<Rc<dyn Object>>` -- the
`Option` is gone, because the call's *recorded* type is `Future<dynamic>`:
the top-bound rule that reads a `T?` of a top-bounded parameter as
`Option<Rc<dyn Object>>` only looks at a bare return, not at one inside a
`Future`. The cast target should come from the method's own return
instantiated, not from the recorded type. Left for the next round.

## An abstract class that is an `Iterable` declares its iterator

A `Rc<dyn Characters>` had neither `iterator` nor `__to_list` on it: the
trait carried only what the class itself declared, and `Iterable`'s
members are the prelude's. So every `Iterable` member on such a handle was
a call to nothing (6 at ws782, all of them the text-field code).

The trait now declares `iterator` (the front end, from
`IrClass.iterableElement`) and carries `__to_list` as a default beside it
(the backend, the same walk `_emitToList` writes for a struct). Every
implementor has an `iterator` of its own, so the impl forwards it like any
other trait method.

## The census on stubs786.txt.detail.txt

256 blocks, grouped by normalised expected/found: 108 E0308, 50 E0599,
30 E0277, 8 E0282, 8 E0593. The largest identifiable groups and what they
turned out to be:

  - 7 `expected <T as DartNullable>::Or, found Option<T>` -- a translated
    callee's `T?` is the projection, and `_widenedInto` was building its
    slot with `_type`, which spells the plain `Option<T>` a *body* works
    with; the coercion then made `Some(..)` and returned before the tail's
    projection rule could run. `AsyncSnapshot.withData` reaches its
    redirecting `this._(state, data, null, null)` this way.
  - 5 `&mut Vec<Rc<dyn DiagnosticsNode>> <= Vec<..>` -- `_fillsParameter`
    resolved an abstract callee through `getDispatchTarget`, which inside a
    mixin *declaration* answers the abstract member itself. The body the
    CFE moved into an application of the mixin is the one that fills, and
    the declaration's parameter was already emitted `&mut` from it.
  - 8 `C<T> <= C<Rc<dyn Object>>` (provider's `_DelegateState<T>`, the
    scheduler's `_TaskEntry<T>`) -- erasure at a *nested* type argument;
    still open.

## `Map` is no longer an association list to look in

`Map<K, V>` keeps its `Vec<(K, V)>` -- the order Dart promises -- and gains
a lazily built index beside it, by `DartEq::dart_hash_code`, valid exactly
while it covers `entries.len()`. An append extends it; every other mutation
changes the length and the next lookup rebuilds. Under eight entries the
scan is still cheaper and is what runs. A key whose `==` looks in the same
map meets `try_borrow_mut` and falls back to the scan.

`InheritedElement._dependents` is the case that named this: one entry per
element depending on a `Theme` or a `Localizations`, scanned on every
`dependOnInheritedElement`.

## Where this stretch stands

    from  ws808:  221 stubbed, 130 refusals, 64 reachable crates
    to    ws847:  201 stubbed,  60 refusals, 64 reachable crates
    run848:       708 lines, 0 type-only differences, 0 panicked, 201 frames

351 unfinished members are 261, reachable crates never dropped (once, and
it was caught and reverted in the same round), and the run ruler's reading
was taken four times across the stretch without moving.

Twenty-one rules landed, each with a fixture that agrees with Dart and its
own reading of the chain. Ten were reverted after being measured, and six
of those would have compiled and been **wrong** -- which is why a round
pays for a fixture before it pays for a chain.

The stub side is now a long tail: the largest shape inside the 61
"mismatched types" is *two* members. The refusal side is 60, and 51 of
them are in four groups that are each one decision, not one rule:

     16  `dart:ffi`'s internals behind the win32 windowing layer. The
         prelude gives them names and no behaviour on purpose ("the refusal
         is at the call"); clearing them means giving `Pointer` a memory
         model, which is a decision about what this compiler promises, not
         a gap in it.
     15  identity on a value class -- `identical` (5), `identityHashCode`
         (4), and the `super.==`/`super.hashCode` calls that stand on them
         (6). Counted: 239 classes have their identity observed, and the
         faithful fix is an identity token carried through `clone`, on all
         239, with hand-written equality to keep it out. Measured, written
         up, and not attempted.
      4  a generic local function, which needs the declaration, the call
         site and the two spellings of `dynamic` settled in one round; the
         second attempt got the first two and is written up.
      4  `is` against a type this compiler does not have (`Future`,
         `HttpException`), and `is Function`, which needs the `is` lowering
         to carry the function *type* rather than the name `Function`.

The rest is ones and twos, each with its shape recorded above.


## What the fixture harness can and cannot reach

**Corrected below.** The claim first written here -- that the harness
builds a single library and that this is what blocks the remaining groups
-- is wrong, and the correction is the useful part.

`fx.sh` builds the fixture through the *AOT* pipeline (`FX_AOT=1`, so TFA
runs) and then emits the whole `file:` package with
`dart2rust_package.dart`, exactly as the chain does for the gallery. A
fixture may therefore span several files: `staticset.dart` importing
`staticset_binding.dart` compiles, translates and runs both. Cross-library
shapes are reachable, and so is anything TFA decides within the fixture.

What the four fixtures below still failed to reproduce is therefore not
"one library" but something else -- most likely the *scale* of the closed
world, since the erasure census and the instantiation list are decided by
what the whole 924-library program names, and a four-class fixture names
too little to make the same decisions.

Four groups were taken to the point of a fixture this session and none of
the four could be reproduced in one file:

    ws858  the erased accessor        mapfnslot   agrees at HEAD
    ws864  the double box             traithandle agrees at HEAD (2 shapes)
                                      mapwiden    agrees at HEAD
    ws865  a `None` with no element    nullaware   agrees at HEAD

Each is produced by an analysis that runs over the *whole* program -- the
erasure census, the closed-world instantiation list, TFA's narrowing of a
field to `Null` -- and `fx.sh` builds a single library with none of them.
The shapes are real (the chain names the members, and the emission shows
the text), but the harness cannot make them fail.

So a fixture for these groups has to be *bigger*, not differently built:
enough classes and enough instantiations that the census and the erasure
make the same decisions the gallery makes. That is worth trying before
concluding anything about the method -- the harness already gives multiple
files, the AOT pipeline and a whole-package emission, which is more than
the four attempts above used.
