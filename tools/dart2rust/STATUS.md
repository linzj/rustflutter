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
  `.build/scratch/ref_render_walk.txt`,6 行:`_ReusableRenderView →
  RenderSemanticsAnnotations → RenderSemanticsAnnotations → RenderTapRegionSurface →
  RenderSemanticsAnnotations → RenderConstrainedBox`,后五个带 `size=Size(800.0, 600.0)`。
- 产物:`.build/scratch/got_render_walk.txt`。
- 第二份参考(2026-09-07):`test/dump_render_walk_settled_test.dart`(pump 8×100ms)→
  `.build/scratch/ref_render_walk_settled.txt`,708 行,gallery 主页整棵树;
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

  这两条(run804/run807 量的)已被 ws934 一并收掉,证据在下:
    - ~~gallery **永不静止**~~ **已销**。run969 连采五次都是 **6 帧、`exit=0`、
      两秒跑完**,日志里没有「预算用尽」那一行——程序自己停了,跟上游一样。
      原因是 ws934:`dyn Trait` 的 `==` 按地址比,`GlobalObjectKey` 每次 rebuild
      都不相等,整棵 `WidgetsApp` 子树被扔掉重建 **193 次/分钟**。「永不静止」
      量的就是这个 churn。
    - 一帧**越来越贵**(frame 6 = 265ms,frame 500 = 493ms,斜率 +0.46ms/帧):
      **大概率同源,但今天量不出来**——只跑 6 帧,500 帧的斜率无从谈起。按〈已知
      欠账〉的出场规则第 2 条,它等一次重量:预算调回 60 秒跑一次,斜率还在就重开
      并写新数,不在就删。**〈已知欠账〉里「运行尺子的下一格该是计数而不是采样」
      那个探针,多半已经不需要了。**
```

## 撤回与作废(不要再试)

- **Object 协议:把注册表换成 vtable,第 2 步(拿掉毯式 impl)——渲染树红,已撤回
  (2026-09-10,work.md 那份计划的主体)。** 第 0 步(证明够得着)和第 1 步(翻译 trait 走 vtable)
  都落地了(`ddca1520`、`81752f94`);**第 2 步撤回,读数写在 `/tmp/work.md` 第五节**。
  **渲染树 707/0/0 → 501 行 / 1 panic / 类型差异 229**(连采五次一模一样),而 §6 写着渲染树是**唯一**的正确性判据。
  **那个 panic 就是多出来的那一个桩**(74 → 75):`ImageProvider.obtainKey` 打了桩,
  跑到图片加载那条路上撞上,一下子带走 206 个节点——**所以那一个桩是拦路的,不是可以记成后续的**。
  反过来讲,**vtable 本身没被证伪,只是那个桩还在路上时没法验**。
  值得留下的量:prelude 那半边**单独编过、0 error**,代价是 **117 个 `impl Object for`**(§7.1 那个「最大的未知」在 prelude 侧变成了一张单子);
  后端有**三条**发射路径不是一条(结构体、**枚举**、闭包进 `Object` 槽),漏掉枚举那条 = **可达 crate 69 → 18**;
  以及一个坑:**prelude 新加的自由函数和翻译类的 Dart 名字共用命名空间**,
  `object_runtime_type` 已经被一个翻译出来的同名函数占了,glob import 打不过本文件的定义,**2,319 个 E0061**。
  卡住的点:`catchError` 的裸 `Function` 槽收闭包,以前靠毯式 impl 让闭包自己是 `Object`;
  把 `coercion.dart` 那条排除去掉试过,**74 → 98**,已撤回。

- **`super.==` / `super.hashCode` 进 `Object` 改成同一性——量出来是**把一个对的拒绝换成一个错答案**,已撤回
  (ws985,2026-09-10)。** Flutter 的 `Widget` 写的就是 `operator ==(Object other) => super == other;`
  和 `int get hashCode => super.hashCode;`,这两条是 29 条拒绝里的 2 条。prelude 现成就有非虚的同一性
  (`dart_identical_any` 比两个地址、`dart_identity_hash_code` 就是地址),于是在**类是句柄**时发出去,
  **拒绝 29 → 27,桩 77 逐字节相同**——数字是真的好看。
  **但夹具照出它是错的**:`Thing`(抽象)下面一个 `Leaf`(**值结构体**),两个不同的 `Leaf(1)`
  在 Dart 里 `hashCode` 不同(同一性),这边**相同**——`dart_self_thing()` 对一个按值拼的类
  根本不是一个稳定的身份。改之前那里是 `panic!`(诚实的拒绝),改之后是一个**悄悄的错答案**。
  **超类函数的接收者是 `&dyn Thing`,发射的时候不知道具体类是不是句柄**,所以这条规则没法在这里立住;
  限制成「所有实现者都是句柄」的话,`Widget` 首先就不满足,那 2 条拒绝也就一条都掉不了。
  `super_calls.dart` 里 ws828 留的那句话是对的:**同一性正是一个被复制的值类所没有的东西**。
  **这 2 条属于 cc88ae36 说的「今天就是对的那 ~7 条拒绝」。**
  顺带照出一条**更老的欠账**(见〈已知欠账〉):子类从抽象超类**继承**来的 `operator ==` 根本没被路由到——
  结构体的 `dart_eq` 是按字段 derive 的,`dart_any_eq` 在注册表里找到的就是它,
  所以 `Leaf(1) == Leaf(1)` 这边答 `true`、Dart 答 `false`。

- **`x?.m(..)` 里 `m` 要 `&mut self` 就改走 `as_mut()`——两次都不成,已撤回(ws984,2026-09-10)。**
  两个桩(`cupertino_route._handleDragCancel`、`services_restoration._removeChildData`)是
  「cannot borrow `*it` as mutable, as it is behind a `&` reference」(E0596):`x?.dragEnd(0)`
  降成 `x.as_ref().map(|it| it.drag_end(..))`,而 `drag_end` 要 `&mut self`。
  nullaware 里本来就有一条 `as_mut()` 的路,闸门 `mutating` 只问「这个名字是不是*集合*修改器」;
  改成也问 `_mutating`(就是决定被调方印不印 `&mut self` 的那套),**77 → 134**(+57)。
  57 个新桩全是同一个错:`old_bucket.as_mut()`,而那个绑定没有 `mut`——IR 里那次调用的接收者是
  绑定出来的 `it`,不是 `x`,所以 `receiverLocals` 从来没见过 `x`,`let mut` 的判定也就漏了。
  补上第二半(`_WalkSelf` 记下 `x?.m(..)` 这一对,由后端回答 `&mut self`)之后 **134 → 80**,
  **但两个目标桩一个都没掉**,净 +3。**两次都没碰到要修的东西,按「不许在坏改动上再叠一层」撤回**
  (ws979 就是叠上去的,最后把自己造的错说成了老错)。
  留下的事实:(1)`as_mut()` 这条路要的是**两半**——发射端和 `let mut` 的判定,少一半就是 57 个桩;
  (2)那两个桩的接收者**不是局部变量**,所以 `place` 那三条(cell / 自己的字段 / 局部)都算不出位置来,
  这才是它们没掉的原因——下次要从**位置**那一侧入手,不是从闸门。

**(2026-09-10)这一节的出场规则。** 本节**只进不出**——一条撤回记录的价值就是防重踩
(一程十四条落地撤回六条,其中四条能编过而且答案是错的)。唯一允许的压缩是:**一条
实验后来被重做并结案时,缩成「当初为什么撤、后来为什么对」两句**,过程移出(git 有)。
不要为了短而删掉「为什么错」和「要重做需要什么」——那两样正是这一节存在的理由。

**(ws966 撤回 → ws971 做成;按本节的压缩规则缩成两句)** `null?.x` 就是 `null`。
**当初为什么撤**:按在**前端**、认的是 Kernel 节点(`_isNull`,ws777),而 gallery
里那个 `None` 是 TFA 折出来的,到那儿已经不是它认得的节点——量出来 **stub 93 → 93,
桩的集合一模一样**,一次都没触发。**后来为什么对**:ws971 把同一句话按在**后端发射
处**(`_writtenNull`,穿过 `clone` 往里看),那里看得见接收者就是降下来的 null
字面量;连带「对写下来的 null 做判空的三目,另一支是死的」一起,**stub 89 → 88**,
夹具 `writtennull` 先红(一模一样的 `&_`,E0282)后绿。

**(2026-09-10,ws978 试过又撤回;一次都没触发)** 生成的 `==` 里,**Rust 比不了的字段
应该走 `dart_eq`**。`byIdentity` 只挑函数类型和光秃秃的句柄,于是**装着**翻译类的字段
——`Option<Vec<DropdownMenuItem<T>>>`——落到了 `==` 上,而翻译类除非自己 derive 过,
否则没有 `PartialEq`(E0369,`DropdownButton.==` 与 `_WindowingOwner.==`)。改成用
上面十行处就有的 `_comparableType` 来判,**量出来 82 → 82,桩的集合逐字节相同**。

**为什么没触发**:探针跑的是 `DropdownButton`,而报错的 `eq` 根本不是它的——

    EQ DropdownButton.items type=Option<Vec<DropdownMenuItem<T>>> comparable=true

报错说的是 `Option<DropdownMenuItem<T>>`(没有 `Vec`),是 **`_MenuItem<T>`** 的 `eq`。

**这条记录改过两次,这是第三版,前两版都别信。**

- 第一版:「毛病在 `_comparableType`,它对带界的 derive 是瞎的」——**方向对,措辞错**。
- 第二版:「根本不在 `_comparableType`」——**错**。我是在**探错了类**(探的 
  `DropdownButton`,报错的是 `_MenuItem<T>`)之后靠推理翻的案,而且就在刚写完「规则不
  触发就打印节点、别再推」之后。

**实际是两件事,ws979/ws980 一件,ws981 另一件:**

1. `_MenuItem<T>` 的 `where` 只有 `T: PartialEq`,少了 `<T as DartNullable>::Or:
   PartialEq`——子句按**本类自己**的投影字段拼,而它只是**装着**一个需要它的
   `DropdownMenuItem<T>`。两套界都要发(每个类型参数一份 + 每个投影字段的**基**一份;
   基不一定是类型参数)。**82 → 81。**
2. `_comparableType` **确实**是瞎的,但瞎的是**投影**这件事:带投影字段的类
   derive 不出 `PartialEq`(derive 写不出 `<T as DartNullable>::Or: PartialEq`),
   没有句柄字段的话手写的那份也不发——于是它**一个 `PartialEq` 都没有**,而
   `_comparableType` 说它可比,装着它的类就拿 `==` 去比。夹具 `heldgenericeq` 复现,
   gallery 今天没有这个形状(**81 → 81,集合逐字节相同**)。

**(2026-09-10,ws971 试过两次都撤回)** 同一件事按在 `coerceInto` 里,两次都不对。
第一次按 `slot.projected && have0.name == 'Null'`:**89 → 89,桩的集合一模一样**,
一次都没触发。第二次按 `have0.name == 'Null' && slot.isFunction`,并把字面量的
`rustType` 改写成槽的类型——**stub 89 → 119、拒绝 29 → 34**,多出来的绝大多数是
「cannot find type `T` in this scope」:槽是用**被调方自己的类型参数**拼的,调用点
一个都拼不出来。**要重做需要什么**:别在 `coerceInto` 里找它——那条 `None.as_ref()
.map(..)` 根本不是 `coerceInto` 发的,是后端的空安全发射器发的(`_nullAware`),
探针一跑就看见了(`DART2RUST_TRACE_NONE`)。
下次要动它,先弄清那个 `None` 是从哪条路出来的——不是这条。

**(2026-09-10,ws962 试过又撤回;拒绝 -1、桩 +1,净零)** 把 `dart:io` 的三个
异常(新写的 `HttpException`,以及 prelude 早就有结构体的 `SocketException`
/`FileSystemException`)加进 `_preludeClasses`,让 `is` 能经由 `DartCoreAs`
问到它们。**规则是对的**:那三个结构体本来就在 prelude 里,只有那张表决定
`is` 够不够得着;夹具 `ishttpexception` 先红(`cannot find type HttpException`)
后绿,量出来 **拒绝 29 → 28**——`IOClient.send`(它在同一个 catch 里问
`is HttpException` 和 `is SocketException`)不再被整member 拒绝。
撤回是因为**它换来了一个桩**:`send` 翻得出来之后编不过,`response.statusCode`
/`contentLength`/`isRedirect`/`persistentConnection`/`reasonPhrase`/`headers`
/`handleError` 一个都没有——prelude 的 `HttpClientResponse` 是个空 unit
struct(「签名用得着、还没给体」那一族),`HttpHeaders` 根本不存在,`handleError`
要求它是个 `Stream<List<int>>`。**stub 98 → 99**,而「桩只降不升」是硬约束。
把这些补齐是一件独立的活:给一个**这个程序里没有任何东西造得出来**的 HTTP
响应面写实现。要么先做那件事再开这张表,要么不开。

**(2026-09-09 ws914 撤回 → 2026-09-10 ws936 重做,已结案)** `_widenedInto` 的可空闸
加 `param is VoidType`:Kernel 给 `VoidType` 的 nullability 是 `nullable`,照字面读它,
每次往 `void` 槽里存都被包成 `Some(..)`(`WidgetsBinding._handleBackGestureInvocation`
那两个桩就是这么来的)。**当初撤回**是因为改完连跑三次树都是 2 行;**那是尺子在抖,
不是这个改动**——撤回后同一个二进制连跑两次是 707 行 / 2 行各一次,三次全空对上三成
空窗率 p≈0.2,suggestive 不是证据。ws934 治好抖动后 ws936 把它放回去:stub 124 →
**122**,新增 0,少的正是那两个桩,五次采样全 707 行 / 类型差异 0 / 0 panic。
**教训留着:半径这么大的改动(全程序每一个 `void` 槽),不要在尺子读不稳时合并。**

**(2026-09-10,ws941 试过又撤回;可达 crate 掉了)** 把 `Sink` 加进
`_preludeInterfaces`(外加一层「Dart 接口名 → prelude trait 名」的映射,因为
`Sink<T>` 是句柄别名 `Rc<dyn DartSink<T>>`)。`DigestSink implements Sink<Digest>`
确实拿到了 `impl DartSink<Digest> for DigestSink`,`hash_super_convert` 那个桩也没了,
但 `crypto_below/src/sha256.rs` 长出**两个函数之外的错**——
`_Sha256Sink: DartSink<Vec<i64>>` 不满足:转发体建不起来时 `_member` 跳过了那个 impl,
而 `dart_cast` 的表**照样把这个 trait 列了出来**。函数之外的错停不掉,于是
`unstubbable: 2`,`crypto_below` 连同下游一起掉出可达集:**可达 69 → 65**,
桩数看着从 112 掉到 105 是假的(是 crate 没了)。

要重做,先补上「`dart_cast` 只列**发出来了**的 impl」——`_stubbed` 已经在记
哪些成员被拒了,protocol impl 那一侧读它就行。这条和「一个被拒的成员不能被
后写的 protocol impl 调用」是同一条规矩,只是 `dart_cast` 还没照办。

**(2026-09-10,ws944 试过又撤回;夹具当场抓住)** 给「体里对它调了变异方法的
`for` 循环变量」加 `mut`。三个桩(`SemanticsNode.detach`、`sendSemanticsUpdate`、
`_buildBottomSheet`)都是这一形状,加上 `mut` 之后确实编得过了——**而且是错的**。
夹具 `forinmut` 当场说了话:

```
--- rust: 4,4/abbccc|dddde
--- dart: 2,2/abb+|e+
DISAGREE
```

`for x in xs.iter().cloned()` 交给体的是元素的**副本**,改副本改不到集合里去;
Dart 的循环变量是同一个对象。加 `mut` 只是把「诚实地 panic 的桩」换成
「悄悄算错」,比原样更坏。撤回。

**这是别名问题的第三块证据**(前两块:`merge_sort` 把同一个 list 当两个
`&mut` 参数传;`_mergeSort` 同样)。正解要么是 `iter_mut()` 走可变的那个位置
(`_mutPlace` 已有,但 `Set<Set<..>>` 的元素在 Rust 里不能就地改),要么是
把 `List<T>` 换成 `Rc<RefCell<Vec<T>>>`(work.md 第 3 条)。夹具 `forinmut`
留着并且**预期是红的**,和 `ffistruct` 一样——它是那个改动的验收条件。

**(2026-09-10,ws945/946 试过又撤回;两次都更坏)** 让「落进可空槽的 `null` 字面量
带上那个槽的类型」。动机是真的:后端早就有「知道槽就写 `None::<T>`」那条规则,
而一个被提升成临时变量的 `null` 出来是 `let __t12: Option<Null> = None`,
被调用方要的是 `Option<Rc<dyn ScrollController>>`。

第一版(只要 `_type(param)` 拼得出来就记上):**拒绝 32 → 37**,五个构造函数
翻不出来了。第二版按后端那条规则的闸收窄(可空、非投影、非函数、不是
`Null`/`dynamic`):拒绝回到 32,前四轮的错也少了——**而第 5 轮 2019 个错**
(ws943 同一轮是 16),诊断直接撑爆 512 MB。

原因是这个改动只改了**记录的类型**,没改**发出来的东西**:`None` 还是那个
`None`,而下游每一处 coerce 从此都以为这个值已经在槽的类型上了,该转的不转。
要做对,得让「记类型」和「发 `None::<T>`」是同一件事——也就是在这里就把字面量
换成一个带类型的节点,而不是给旧节点贴个标签。

**(2026-09-10,ws954 试过又撤回;+3 且一个没清)** 把 `_throughReceiver` 从
「声明的类型**就是**拥有者的参数」放宽到「声明的类型**提到**拥有者的参数」,
想让 `Completer<T>.future` 在一个被擦除的接收者上记成 `Future<dynamic>`。
结果:`_TaskEntry` 那两个桩**一个没清**,另外**新增 3 个**
(`Route.popped`、`TransitionRoute.completed`、`SemanticsNode.sendEvent`——
都是在带擦除参数的类上返回 `Future<T>` 的 getter),109 → 112,撤回。

前提就是错的:「发出来的 Rust 交回的是擦除拼法」这件事,取决于**接收者在这一处
到底拼成什么**,而 `_erasedByReceiver` 是拿 `_erasedRead` 去问**声明类**的擦除,
不是问这一处。要做对,得拿到**降下来的接收者**的 `rustType`——而那个东西在
`_memberRustType` / `_throughReceiver` 这一层还没有,重新降一次会多出临时变量。

所以 `_TaskEntry` 这一族(`scheduler_binding` 的两个桩)现在的账是:
协变扫描已经看得见它了(ws953),`_TaskEntry` 也已经没有类型参数了,
剩下的一步是**「调用的类型按接收者降下来之后的实例化算」**——这需要把降好的
接收者传进类型计算里,是一次结构上的改动,不是一条规则。

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
| ws916 | `xs.iter().map(|child| ..)` 交给体的是 `&Rc<dyn X>`,比句柄多一层引用,而接收者按 `_isHandle` 拼成 `&*child`——少解一层,于是 `FocusNode::to_diagnostics_node(&*child, ..)` 说「`Rc<dyn FocusNode>` 没有实现 `FocusNode`」。空安全绑定(`IrBound`)早就有这条 `&**`,缺的是**谁知道这个局部是按引用绑的**——只有 `_stepClosure` 知道,所以它像记 `_cellLocals` 一样把这些名字记进 `_refLocals`,接收者那一处照着 `IrBound` 的样子多解一层 | stub 127 → **126**、拒绝 33、可达 69、0 error;与 ws932 逐条比新增 0,少了 `focus_node_super_debug_describe_children` |
| ws934 | **trait 对象的 `==` 按地址比,而 Dart 的 `==` 派发到对象**:`WidgetsApp(key: GlobalObjectKey(this))` 的两把钥匙包着同一个 state,Dart 说相等、这里说不等,于是 `canUpdate` 说不能更新,`WidgetsApp` 连同整棵子树每次 rebuild 都重建(60 秒 193 次)——渲染尺子抖了十几轮就是这件事,细节在〈已知欠账〉。改成和 `dyn Object` 一样走对象自己的答案(prelude 的 `dart_any_eq`/`dart_any_hash`:注册表里有就用类的 `==`,没有退回地址)。顺路两条:擦除过的实例化在槽上 cast 回来(类型参数带 `'static`,`TypeId` 问得出),而「结果按界读回来」的调用上接收者不做这次 cast(否则转换两次);`statecheck.py` 点名的 `_refLocals` 补进 `_member` 的存/还 | stub **126 → 124**(逐条比新增 **0**)、拒绝 33、可达 69、0 error;21 个 fixture 全 AGREE;**run935 连采五次全是 707 行 / 类型差异 0 / 0 panic**——尺子第一次不抖 |
| ws936 | 把 ws914 撤回的那条放回去并**量了**:Kernel 给 `VoidType` 的 nullability 是 `nullable`,照字面读,每一次往 `void` 槽里存都被包成 `Some(..)`;`_widened` 的可空闸现在也认 `param is VoidType`。当初撤回的理由是「改完连跑三次渲染树都是 2 行」,而那是尺子在抖(ws934 治好了)。顺路给擦除加了一个诊断:`DART2RUST_TRACE_ERASED=1` 说协变扫描标过的参数最后**擦没擦、被哪一道闸拦下**——协变的 trace 只说标了什么 | stub **124 → 122**(逐条比新增 **0**,少了 `widgets_binding.rs` 的 `_handle_back_gesture_invocation__body` 与它的 super fn)、拒绝 33、可达 69、0 error;run936 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws937 | **`late` 字段的格子里装的是 `Option`,而「赋值当表达式用」那一路没有包 `Some`**。语句那一路早就包了(`IrAssignField` 里那句注释写着「这是唯一发生这件事的地方」——在表达式那一路也需要它之前是真的)。Dart 里 `x = v` 的**值是 `v`**,存进去的才是 `Some(v)`,所以只包存的那一侧,`__set` 照旧是裸的:`_opacityAnimation = CurvedAnimation(parent: _opacityController = AnimationController(..), ..)` 是这个形状 | stub **122 → 119**(逐条比新增 **0**,少了 `material_data_table.rs` 的 `init_state`、`painting_text_painter.rs` 的 `_compute_caret_metrics`、`rendering_animated_size.rs` 的 `perform_layout`)、拒绝 33、可达 69、0 error;run937 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws938/939 | **同一条继承链上参数不一致时,方向应该是「都擦」而不是「都不擦」——但只在两边都担得起的时候**。`_InheritedProviderScopeElement<T> implements InheritedContext<T>`:下面被真实流点标了(`element = this` 落进 `_InheritedProviderScopeElement<T?>` 的槽),上面没标,原规则把两边一起丢掉,于是 provider 六个成员手里是 `X<T>`、声明写的是 `X<T?>`。先试「一律往上抬」(ws938):清掉那 6 个,却因为把 `Animatable.T` 也擦了而**新增 7 个**(`Tween.lerp`、滑块 demo 的 `paint`……),净 +1,**撤回**。加一道闸再来(ws939):**只在这个位置上每一个传自己参数进来的子类都已经标了的时候才抬**——`InheritedContext` 只有一个子类且已标,`Animatable` 有 `TweenSequence`/`_ChainedEvaluation` 没标,于是只抬前者。`DART2RUST_RAISE=0` 退回原来的丢弃 | stub **119 → 113**(逐条比新增 **0**,少的正是 provider 的 `build`/`mount`/`unmount`/`update` 那一族六个)、拒绝 33、可达 69、0 error;run939 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws940 | **被树摇空了的增强枚举,现在照样发出它的变体**。原来的规则是:增强枚举(每个变体带自己的字段)如果那些字段的值从常量里读不回来,就整个发成空枚举——理由写着「不然就会把它当普通枚举发、把成员丢掉」。可是空枚举**把成员和名字一起丢了**,而且是悄悄地:`enum KeyboardLockMode {}`,于是 `KeyboardLockMode::NumLock` 指着一个不存在的变体、`Set<KeyboardLockMode>` 连 `DartEq` 都没有。变体发出来之后,只有真去读那份状态的成员编不过,而编不过就是一个桩——看得见,一个一个数得清。`valueFields` 仍旧是空的,所以那份状态不发 getter | stub **113 → 112**、拒绝 **33 → 32**(`KeyboardLockMode.findLockByLogicalKey` 从「拒绝」变成一个桩,`handle_key_event` 和 `_should_accept_num_lock` 两个桩清掉)、可达 69、0 error;run940 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws943 | **`a ?? b` 两边是同一个类、但类型实参不同时,左边的拼法只有在右边真放得进去的时候才算数**。`children ?? buttonItems` 一边是 `List<Widget>`、一边是 `List<ContextMenuButtonItem>`,Dart 说整个是 `List<Object>`;而 `lub` 的判定只比 `classNode`,两边都是 `List` 就取了左边,于是右边逐元素被抬成一个它并不实现的 `Widget`。判定改成「类不同**或者**右边不是左边的子类型」(`typeEnvironment.isSubtypeOf`)。夹具 `ifnulllub` 先红(3 个编译错)后绿 | stub **112 → 110**(逐条比新增 **0**,少了两个 `adaptive_text_selection_toolbar.rs` 的 `build`)、拒绝 32、可达 69、0 error;run943 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws947 | `String.fromCharCodes(codes)` 是前端手写的一条 prelude 调用,而它的实参**一个转换都没走**:`Uint8List` 是 `Vec<u8>`,prelude 收 `Vec<i64>`。手写的 prelude 调用得自己要那次加宽(`_widensNarrowElements` 给别的 `List<int>` 槽做的那次)| stub **110 → 109**(逐条比新增 **0**,少了 `crypto_below/src/digest.rs` 的 `_hex_encode`)、拒绝 32、可达 69、0 error;run947 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws948 | **促升过的类型参数没有拼法**。Kernel 在 `if (x == null) return;` 证明了一个 `T?` 非空之后,把那个位置写成 `IntersectionType(T% & Object)`,而 `_type` 没有这一条,整个成员被拒。促升改的是**知道什么**,不是**手里拿的是什么**:拼法就是这个参数自己的、非空的那个。(拼成促升到的那一侧会说 `Object`,而值是个 `T`。)夹具 `promotedparam` 先红(带着一模一样的拒绝信息 panic)后绿 | stub 109(不变)、拒绝 **32 → 31**(`UndoHistoryState._update` 现在翻得出来**而且编得过**,没有换成一个桩)、可达 69、0 error;run948 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws949/950 | **Dart 的 `continue <case>;`**:控制离开这一个 case 去跑**另一个**,而 Rust 的 `match` 跑完一条臂就完了。`LicenseEntryWithLineBreaks.paragraphs` 整个是用这个写的状态机,一直被整member 拒绝。做法:这个 switch 变成一个带标签的 `loop`,臂**编号**(`IrSwitch.threadLabel`),编号是一个 `let mut`,循环对它 `match`;`continue` 就是「改编号、再转一圈」(`IrContinueSwitch`)。臂号在**降体之前**先编好,因为体里要指名道姓。ws949 只做了 `match` 那一种拼法,而这个 switch 的 case 值是 `String`——不是 Rust 的模式,于是拒绝换成了一个桩;ws950 补上 if 链那一种(普通 switch 早就有两种拼法)。夹具 `continueswitch` 两种拼法都覆盖,先红(带着一模一样的拒绝信息 panic)后绿 | stub 109(不变)、拒绝 **31 → 30**(`paragraphs` 现在翻得出来**而且编得过**)、可达 69、0 error;run950 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws951/952 | **`f<int>` 作为一个值**(`Instantiation`):Dart 把一个泛型函数值实例化到写出来的类型上,而**那些类型正是下面那个 tear-off 缺的东西**。Rust 的闭包没有自己的类型参数,所以 tear-off 变成的那个闭包就带着这些类型去调方法(`IrCall.typeArguments`)。`showDialog` 递的 `Navigator.of(context).pop` 就是这个形状,整个顶层函数为它被拒。ws951 只做了「实例化就是它自己」那半,于是拒绝换成了一个桩——泛型方法的 `T?` 形参在被调方那边拼成投影 `<T as DartNullable>::Or`(实例化之后是个 `Option`),而闭包收的是槽声明的那个(`Object?` 在这里是裸的 `Rc<dyn Object>`);ws952 让参数在传过去的路上进被调方的拼法,和普通调用的实参一样。夹具 `instantiation` 先红(带着一模一样的拒绝信息 panic)后绿,`T?` 可选形参也覆盖了 | stub 109(不变)、拒绝 **30 → 29**(`showDialog` 现在翻得出来**而且编得过**)、可达 69、0 error;run952 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws953 | **协变扫描看不见 mixin 的体**。去重过的 mixin application 的体住在 `dart:mixin_deduplication` 里,而那个库的 uri 没有任何前缀匹配得上——别名普查 run672 就吃过这个亏并修了(`aliasScanned`),协变扫描没有。于是 `SchedulerBinding.scheduleTask` 把一个 `_TaskEntry<T>` 加进 `PriorityQueue<_TaskEntry<dynamic>>` 这个流点**根本没被看过**,`_TaskEntry.T` 也就没被标成协变。改成扫 `aliasScanned`。顺手加了 `DART2RUST_TRACE_FLOW=<类名>` / `=@<成员名>`:这个扫描**看过**的每一处 value→slot,不管标没标——`TRACE_COVARIANT_SITE` 只说标了什么,「这一处为什么没标」原来无从读起 | stub 109、拒绝 29、可达 69、0 error——**四把尺子一格没动**,但生成的东西动了:`_TaskEntry` 现在没有类型参数了,`_task_queue().add(entry)` 那个错没了。那两个桩还在,卡在**下一条**规则上:`entry.completer.future()` 发出来是 `DartFuture<Rc<dyn Object>>`,而**记下来的**类型是 Dart 说的 `Future<T>`(`_memberRustType` 按 `_staticType(receiver)` 算),两边一样于是没人去转换。要清掉它,得让「调用的类型按接收者**记下来的**实例化算」——试过在 `coerceInto` 上加一条 `Future<Object> → Future<T>` 的转换(prelude 里 `CastErased<DartFuture<B>> for DartFuture<A>` 本来就有),**一次都没触发**,已撤回 |
| ws891 | 第五轮复审抓到:ws889 落地后 `regen.py` 的 `hints()` 会**永远误报**——它按 `'throws:' not in driver` 字面判定,而正确的修法恰恰是把那个参数删掉,所以那条提示从此指向唯一不该做的修法。换成 `DECIDED_IN`:只指出决定写在哪两个函数里,不对文件的现状下任何断言。顺手分开探针的两种失败(没调用 vs 调用了没 `?`) | `fails:` 在 frontend.dart 已 11 处、`throws:` 在 kernel driver 已 0 处——两条提示一条正确变哑、一条永久说谎,实测属实;两个诊断分支各跑一次验过 |
| ws955 | **没有东西能告诉 rustc 一个 prelude 静态调用的类型参数是什么**:`Iterable<int>.generate(n)` 省掉了生成器,填进去的 `None` 什么也不说,`count` 是 `i64` 与元素无关;结果又被迭代而不是存进一个带标注的局部,于是上下文那头也没得推(`type annotations needed for &_`,starter study 的 `home.dart`)。规则:被调方是 prelude 的、且调用**实际填了的**每一个形参都不提到类型参数,就把类型实参拼出来。**但工厂的类型参数是它那个类的**——prelude 把类写成泛型时,它们在 impl 上而不在关联函数上(`Completer<T>.sync()` 是 `Completer::sync()`);少了这一分,E0107 多出 6 个,dart:ui 的 `_futurize` 在内。哪些 prelude 类型带自己的参数,是**读 prelude 源码**读出来的(`_genericPreludeTypes`),不是手列的表。夹具 `generateindices` 两半都覆盖,先红(一模一样的 `&_`)后绿 | stub **109 → 108**、拒绝 29、可达 69、0 error;run955 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws956 | **一个浮点字面量做接收者要自己说是 `f64`**——这条规则早就有(HCT 里 21 个 E0689),但它只认**光秃秃的**字面量,而 `math.min(-_kFlingVelocity, ..)` 里那个接收者是一个 `const double` 取负之后折出来的字面量,在 IR 里是 `IrUnary('-', 字面量)`:rustc 眼里它照样是 `{float}`,这边却不认。改成看**值**是不是浮点字面量(穿过取负),并且把后缀写在**字面量身上**——`(-(2.0_f64))`,不是 `(-2.0)_f64`,后缀属于字面量而不属于它外面那层表达式。夹具 `minnegconst` 三种拼法(取负的 const、写在调用里的负字面量、光秃秃的)都覆盖,先红后绿 | stub **108 → 107**(reply 的 `_handleDragEnd`)、拒绝 29、可达 69、0 error;run956 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws957 | **投影的 `T?` 每过一道边界都要换一次拼法**,radio group 那三个桩是同一件事的三种形状。(a)`??`:`<T as DartNullable>::Or` 是关联类型不是 `Option`,`match` 不了——原来只把**被匹配的那一边**摊平,于是两条臂不一样;要**两边都摊**,再把结果放回这个表达式记着的那个投影里(`registry?.groupValue ?? widget.groupValue`)。(b)**tear-off 的被调方槽**:ws952 只认泛型**方法**自己的类型参数,而泛型**类**的 `T?` 同样是投影(`Registry<T>.changed(T? value)` 撕成 `ValueChanged<T?>`,收到的是体里那个 `Option<T>`)。(c)**绑到 `null` 的 `Let` 什么也不绑**:`null` 没有位置、没有身份、没有副作用,body 读哪儿就把字面量放哪儿——原来那条绑定还是**错的**,因为静态类型 `Null` 的变量拼成 `Option<Null>`,而槽要的是它自己的 `Option<T>`(`registry!.onChanged(null)`)。夹具 `projectednullget` 三种形状都覆盖,先红后绿 | stub **107 → 103**(radio 三个,外加 `_detail_page_route` —— (c) 顺手清的)、拒绝 29、可达 69、0 error;run957 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws958 | **Dart 的函数子类型是逆变的,Rust 的不是**:`Set.contains(Object?)` 在 Dart 里可以当 `bool Function(String)` 用,而 tear-off 变成的那个闭包按**方法自己的**形参声明,于是 prelude 的 `first_where` 收到 `Fn(Rc<dyn Object>)` 而它要 `Fn(String)`(E0631)。做法:tear-off 落进的那个函数槽记下来(`_expectedTearOff`,和闭包字面量自己的 `_expectedFunction` 分开,免得改了嵌套闭包的降法),闭包按**槽的**形参声明,每个形参在进调用的路上再放宽回方法自己的槽——集合方法那一路借 `_retyped` (生成的调用读的是被调方自己的变量,读完由实参机制放宽),普通那一路借 `passedOn`。夹具 `widetearoff` 覆盖 `firstWhere`、`firstWhere(orElse:)`、`where` 三处,先红(一模一样的 E0631)后绿 | stub **103 → 102**(`commonDirectionalityOf`)、拒绝 29、可达 69、0 error;run958 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws959 | **rustc 从这个值本身推出来的槽,向上转型就得写出来**。prelude 的泛型和 Dart 的泛型是同一个(`fold<R>(R initial, ..)` 就是 `fold_dart<R>(initial: R, ..)`),所以 `R` 是**从递进去的那个值**读出来的;Rust 在**写明白的**槽上会自己 unsize,而这里没有写明白的槽,于是 `borders.fold<EdgeInsetsGeometry>(EdgeInsets.zero, ..)` 把 `R` 定成了 `Rc<EdgeInsets>`,写在 trait 上的 combine 就对不上了(E0631)。两条:被调方是 `dart:` 的、形参**恰好就是方法自己的类型参数**时,槽取调用写出来的类型实参(`_ownParameterSlots`,与 `_narrowSlots` 合并成 `_preludeSlots`);并且这种槽上的 `IrUpcast` 一律 `explicit`。夹具 `foldwiden` 先红(一模一样的 E0631)后绿 | stub **102 → 100**(`_CompoundBorder.dimensions`,外加顺手清的 `material_tabs.did_update_widget`)、拒绝 29、可达 69、0 error;run959 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws960 | **运算符的形参也要 `mut`**。方法的形参在体里被赋值时会带上 `mut`,运算符的没有:`std::ops` 那个 impl 只是转发,而**装着体的那个固有方法**和转发器共用了同一份形参拼法,于是 `Priority operator +(int offset)`(体里先把 `offset` 夹住再用)报 E0384。转发器保持原样——它用不上的 `mut` 是一个被 deny 的 `unused_mut`;固有方法按 `_assignedIn(体)` 决定。夹具 `operatormut` 覆盖「写形参的」和「不写的」两种运算符,先红(一模一样的 E0384)后绿 | stub **100 → 99**、拒绝 29、可达 69、0 error;run960 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws961 | **裸的集合类型也有元素**。Dart 写 `as List?` 就是 `List<dynamic>`,而降下来的 downcast 目标一个类型实参都没带,`downcast_ref::<Vec>()` 不是任何 Rust 类型(E0107,`PredictiveBackEvent.fromMap` 拿平台消息的字段就是这么转的)。规则:downcast 目标是集合而没写实参时,按它的元数把 `dynamic` 拼出来——`List`/`Set` 一个,`Map` 两个,这正是 Dart 说裸集合装的东西。夹具 `rawlistcast` 覆盖裸的、缺键的、以及写了元素的三种,先红(一模一样的 E0107)后绿 | stub **99 → 98**、拒绝 29、可达 69、0 error;run961 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws963 | **窄化转换的源也得有类型**。`Uint32List.fromList(xs)` 拼成 `xs.iter().map(|v| *v as u32).collect::<Vec<u32>>()`,而这串东西对**它读的是什么**一个字都没说;源是个字面量列表时,Rust 的整数默认把它定成 `i32`,SHA-256 那八个初值里有五个装不下(`literal out of range for i32`,而且是 deny 的)。做法:把源按**它自己记下来的类型**绑一道(`{ let __src: T = ..; __src }`)——不是按名字认 `List`,列表字面量记的是 CFE 给的运行时类名(`_GrowableList<int>`),拼出来同样是 `Vec<i64>`。夹具 `typedfromlist` 覆盖超过 2^31 的整数源和 double 源两种,先红后绿 | stub **98 → 97**(crypto 的 `Sha256Sink`)、拒绝 29、可达 69、0 error;run963 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws964 | **界是可空的类型参数,促升要把界的 `Option` 也摘掉**。`T extends String?` 按界拼(`_spelledAsBound`),所以 `T` 是 `Option<String>`;Kernel 说类型参数自己的可空性是**未定**而不是可空,于是 `if (input == null || input.isEmpty) return input;` 之后那个促升读没有解包,`substring` 落在 `Option<String>` 上。两条:促升解包问的是**拼法**(记下来的 IrType 可空、且不是投影),不是 Kernel 说的可空性;并且 `'..' as T` 在按界拼的参数上**不是 trait 转换**——`dart_cast_to::<dyn String>` 把结构体当成了 trait(E0404)——而是往那个拼法上的普通 coercion。夹具 `promotedor` 覆盖 `||` 守卫的两半和 `as T`,先红后绿 | stub **97 → 96**(intl 的 `toBeginningOfSentenceCase`)、拒绝 29、可达 69、0 error;run964 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws965 | **建在「不返回」的操作数上的表达式,就是那个操作数**。Dart 先算操作数,所以后面那一步根本到不了;Rust 这边在 `!` 上既解析不出方法(E0282「cannot infer type」),也 await 不了(`()` is not a future)。AOT 编译器凡是证明某个值不可能存在,就在那里种一句自己的 throw,前端按**文本**认出来降成 `unreachable!()`。三处:调用的接收者、`await` 的操作数、以及**转换**的操作数——`dart_cast_to` 那条才是 `inputDecorationTheme` 真正走的路(先按调用改,量出来一个没清;看了未打桩的源码才知道是 `IrCastTo`)。另外按**字面量文本**认而不是按 `identical`:flattening 会重建整棵树,到后端的是副本。夹具 `deadoperand` 把 TFA 那句 marker 亲手写出来(夹具那么小,TFA 自己不会种,只会把整段折掉),覆盖裸的和绑定过的两种;它与 Dart 同输出,但**没能复现出块那一种**——那一种是 CFE 的表达式 `Let`,记在这里 | stub **96 → 93**(`DropdownMenuThemeData`/`DatePickerThemeData` 的 `inputDecorationTheme`,`_ContrastEvaluation._evaluate`)、拒绝 29、可达 69、0 error;run965 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws967 | **`Isolate.run(computation)`**:这里只有一个 isolate,所以那段计算就在这个 isolate 上派出去——而这正是 `Future(computation)` 已经在做的事,回调的形状(`FutureOr<R> Function()`)也一模一样,所以降成 `future_new(..)`。**不能**写成 prelude 那个 `Isolate<T>` 上的 `run`:那是给 `static` 用的包装,只是恰好和 `dart:isolate` 同名,`Isolate::run` 那个 `T` 无从推起(「no associated function named `run` found for struct `Isolate<_>`」)。夹具 `isolaterun` 先红(一模一样的 E0599)后绿——同步的夹具驱动不了事件循环,所以它验的是「调得出来、拿回来的是对的 future 类型」 | stub **93 → 92**(foundation 的 `compute`)、拒绝 29、可达 69、0 error;run967 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws968 | **`Object.hashAllUnordered(xs)` prelude 里没有**,调用名了一个没人写的函数(E0425)。补上:每个元素的 hash 用**可交换**的方式折进去(求和、异或、计数,和 SDK 自己那套一样),所以同样的元素换个顺序算出来一样。补完之后那个桩还在,原因换成了 mismatched types——`hashAll`/`hashAllUnordered` 收的是 `Iterable<Object?>`,而 prelude 那两个写的是 `Vec<T>`,`RenderObject.hashCode` 递进去的是个 `Set`;于是两个都改成收**任何能迭代的东西**(`IntoIterator`,`Set<T>` 早就实现了)。夹具 `hashunordered` 覆盖「换序相同」「不同元素不同」「有序的那个确实看顺序」和「收 `Set`」四条,先红后绿 | stub **92 → 91**(`RenderObject.hashCode`)、拒绝 29、可达 69、0 error;run968 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws969 | **字面量之间的算术,做接收者一样是没定住的**。`(1.5 * 0.35).sin()` 和 `1.5.sin()` 一样——里面没有一处说它是哪种浮点(E0689)。ws956 教会了这条规则穿过取负,这是另一种拼法:穿过 `+ - * /`,并且**两边都得是字面量**——只要有一个带类型的操作数,推导本来就有了。后缀写在**最左边那个字面量**上,一处就把整个表达式定住。夹具 `binaryfloatrecv` 覆盖全字面量的两种和带类型操作数的一种,先红(一模一样的 E0689)后绿 | stub **91 → 90**(`InkSparkle._updateFragmentShader`);第三轮的错误数 86 → 80,说明这条规则不止一处在用;拒绝 29、可达 69、0 error;run969 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws970 | **super 函数的 `__Self` 上站着 Dart 链从没提过的 trait**。`super.x()` 在混入里派发到*应用*的前一个类——`on` 子句没提过它——自由函数就把那个 trait 也要进界里(`_superBoundTraits`),而**界是往下继承的**:`on` 那个混入的混入,`__Self` 上同样站着它。于是 Dart 只声明过一次的名字,在 Rust 这边有第二个候选(E0034),而 Dart 那边无从看见。`_boundOnlyTraits` 用同一个 walk 从 IR 里把这些多出来的 trait 读回来,数候选时算上;答案仍是 Dart 的那个**声明 trait**,重写照样到位(具体类对它的 impl 带着重写)。夹具 `boundwidened` 先红(一模一样的 E0034,`Base` 对 `Mid`)后绿。**同一轮把尺子的输入搬回工程目录**:`~/dart2rust_build/` 被清掉,一次带走 dill、41 个夹具的全部源码和参考渲染树,而仓库里没有一行说过怎么再造。dill 与参考各补了脚本(`bin/gallery_dill.py`、`bin/render_ref.py`),夹具**源**入库(`fx/`),扫描与运行尺子也第一次写下来(`bin/allfx.sh`、`bin/render_ruler.py`) | stub **90 → 89**(`RenderAbstractLayoutBuilderMixin.layoutCallback`;`layoutInfo` 的 E0034 也没了,底下露出另一条错);第三轮错误数 80 → 79;拒绝 29、可达 69、0 error;run970 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **39 个,37 AGREE + 2 个故意的红**(从转录里捞回 39 个,`cmpscalar` 和另一个没捞回来) |
| ws971 | **降下来的那个 `null`,是已经知道答案的**。同一句话说两遍,都在后端发射处:`null?.x` 就是 `null`(`?.` 右边 Dart 一步都不算,所以没有东西要 map);`x == null ? a : b` 里的 `x` 是这样的 null 时就是 `a`(另一支是死的,可它照样得过类型,而站在那儿的光秃秃 `None` 什么类型都推不出来)。只折**降低过程自己写下来**的 null——省略的实参、常量字段;仅仅是可空的接收者照旧运行期再问,夹具里 `viaField` 按着这一条。三次才对:前两次按在 `coerceInto` 里,一次一个桩都没动(集合逐字节相同)、一次把 stub 顶到 119、拒绝顶到 34,两条连数带原因都进了〈撤回与作废〉;探针(`DART2RUST_TRACE_NONE`)一跑才看见那条 `None.as_ref().map(..)` 压根不是 `coerceInto` 发的。ws966 撤回的是同一句话按在前端,按本节压缩规则缩成两句 | stub **89 → 88**(`_WidgetStateTextStyle.new`);`ChangeNotifierProvider.value` 的 `&_` 也没了,底下露出另一条(`Some(Rc::new(闭包))` 不 unsize 成 `Rc<dyn Fn>`);拒绝 29、可达 69、0 error;run971 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **40 个,38 AGREE + 2 个故意的红** |
| ws972 | **一个类可以*就是* `Future`**。Dart 里 `class TickerFuture implements Future<void>`,`await controller.forward()` 是再普通不过的写法;prelude 的 future 是**结构体** `DartFuture<T>` 不是 trait,所以这条接口没法变成 supertrait,整个丢了——`Rc<TickerFuture>` 在 Rust 这边根本不是 future(E0277,rustc 自己提示「must implement `IntoFuture`」)。Dart 的 `await` 对任何 future 都是 `then`,所以实现了 `Future<T>` 的类发一个**固有方法** `dart_into_future`(背后就是它自己的 `then`),await 处调它;可空的那种先问再 await。**不是** `impl IntoFuture for Rc<Self>`:`Rc` 不是 `#[fundamental]`,那个 impl 不归本 crate 写。调用参数按类**自己那份 `then`** 拼——具化类型实参是只在类型参数被保留时才有的隐藏尾参(`__ty_<i>`),`TickerFuture` 上是三个、别的类上是两个,写死三个就是 E0061;`then` 要是还有别的必填参数就不发,不替它猜。夹具 `awaitclass` 先红(一模一样的 E0277)后绿,这条 E0061 正是它照出来的——gallery 一个人永远照不出 | stub **88 → 85**(`_MagnifierState.show`/`hide`、`_ShrineAppState._onWillPop`,动画路径上的三个);拒绝 29、可达 69、0 error;run972 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **41 个,39 AGREE + 2 个故意的红** |
| ws973 | **一轮两条,因为头一条单独量不出数**。(1)**trait 体里*借出去*的字段也要走 cell 访问器**——读的时候早就走了,借的时候没走,`_mutRef` 直接拼 `self.<字段>`;在句柄上那个名字是方法不是字段(E0615)。super 函数里的闭包也是 trait 体,gallery 那两个就在那儿。量出来 **85 → 85**,两个 E0615 都没了,底下露出(2)。(2)**可比之物的句柄,比法跟它自己一样**——`implements Comparable<T>` 的 impl 发在**结构体**上,而它的 List 装的是句柄,`sort()` 要的是 `Rc<_ActiveItem>: Comparable<Rc<_ActiveItem>>`,没人答(E0599)。prelude 补一条毯式 impl;界是 `Comparable<Rc<T>>` **不是** `Comparable<T>`——类型是类的形参按句柄借出,类实现的那条实参本来就是句柄,写成后者对任何翻译类都不成立(先写错了一次,量到 85 → 85)。夹具 `lentcell` 覆盖的是 **(2)**:先红(一模一样的 E0599)后绿。**(1) 没有夹具**——这么小的程序里 mixin 的体被摊进两个应用类,闭包的 `this` 就是结构体、`_items` 真是字段;gallery 那个是句柄,因为闭包被动画拿着活过了这次调用。缺口写在夹具注释里,没有糊过去 | stub **85 → 83**(`_SliverAnimatedMultiBoxAdaptorState` 的 `insertItem`/`removeItem`);拒绝 29、可达 69、0 error;run973 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **42 个,40 AGREE + 2 个故意的红** |
| ws974 | **只是**重名**成集合修改器的方法,不是对某个位置的修改**。`remove` 是 `List` 干的事,于是名叫 `remove` 的调用被送去「接收者所在的那个位置、可变借出」;句柄上没有那样一个位置——接收者就是个带方法的对象,改的是它肚子里。照借不误的结果是,闭包捕获的那个绑定被 `&mut` 借了,而它从来没拿到过 `mut`(E0596)。**由接收者的类型决定**;类型没记下来时**照旧走老路**——在没记录的地方猜「不是集合」,会把一次真的修改悄悄挪到克隆上,那正是 ws944 量到的代价。ws509 早就为**字段**学过这一条,把一般情形留在了原地。先把这条按在 `?.` 那条路上试了一下,只翻译一遍就看见它在那儿一次都不触发(那个点是空检查不是 `?.` map),按 ws966 的标准**撤掉**,没留下没证据的规则 | stub **83 → 82**(`ScaffoldState._buildBottomSheet`);拒绝 29、可达 69、0 error;run974 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **43 个,41 AGREE + 2 个故意的红** |
| ws975 | **super 体不再按实现者单态化,改成写一遍、吃 `&dyn Trait`**(`/tmp/work.md` 第一项)。Rust 没有 `super`,基类方法体本来做成对 `__Self` 泛型的自由函数,每个实现者复制一份。trait 加一个**借**的孪生 `dart_as_<trait>(&self) -> &(dyn Trait + 'static)`,每个实现者写成 `{ self }`——不用 `impl .. for Rc<Self>`(`Rc` 不是 `#[fundamental]`),也不复用已有的 `dart_self_<trait>()`(那个还回 `Rc`,每次 super 调用要付一对引用计数)。**`'static` 落在对象上、不在借上**:体内要从 `this_` 造 `Rc<dyn Trait>` 和 `'static` 闭包,写成 `+ '_` 时每个 trait 默认体都是「borrowed data escapes」(E0521;第一次量 **82 → 12463 stub**)。**够不够格由 IR 现算**(`_superTakesDyn`),不是发射时记下来——调用点常在另一个模块,记不住;turbofish 比签名多一个参数,就是 28 个新桩。**五处调用点**,不是 work.md 说的「一处改动」:trait 默认体、super 里的 super、异步包装、erased 孪生、`IrSuperDispatch` | **`.text` 136,627,376 → 108,334,192(−27.0 MB)**,二进制 329.1 → 283.9 MB(−43.1 MB),`_super_` 符号 **31,406 → 4,382**;work.md 估 −21.3 MB,实际更多——少了 27,000 个函数实例,`.eh_frame` 跟着少 2.0 MB、`.gcc_except_table` 少 2.6 MB。**尺子一格没动**:stub 82(集合逐字节相同)、拒绝 29、可达 69、第 0 轮错误 22 → 17;run975 连采五次:707 行 / 类型差异 0 / 0 panic。基线二进制是 09:10 那版(ws971 之后),ws972–974 三条小规则夹在中间,解释不了 27 MB,但这个差不是只归 ws975 一条 |
| ws976 | **release 也 `panic = "abort"`**(`/tmp/work.md` 第三节)。`[profile.dev]` 早就是 abort,release 是唯一的例外;这个程序里没有东西 unwind——Dart 的 `throw` 是 `Err`,不是 panic。代价是 `runtime/src/lib.rs` 那个 `catch_unwind`(帧内 panic 之后还能继续 dump 渲染树),但那把尺子跑的是 **debug** 构建,本来就是 abort、本来就没有它;例外的一直是 release。**顺手回答了 work.md 第八节第 5 条**「`Result` 清理路径在 `.text` 里到底占多少,只有 `.gcc_except_table` 9.4 MB 这个影子」:**影子底下是 21.7 MB**。另:`bin/workspace.py` 是从**没打桩**的 `.crate/src` 重生成 `.crate-ws` 的,单独跑它会把 `stubs.py` 的成果扔掉——改 manifest 和改 prelude 一样,只能走 chain。我先犯了这个错,又把一次**失败**构建后残留的旧二进制的段大小当成读数报了出来:**读数之前先看构建自己的退出码** | `.text` 108,334,192 → **85,581,600**(−21.7 MB),二进制 283.9 → **245.1 MB**(−37.0 MB),`.gcc_except_table` 7,135,632 → 13,252;work.md 估 −9.4 MB(只算了表),实际是它的四倍,因为 landing pad 的**代码**也一起没了。**从 09:10 基线累计:`.text` 136.6 → 85.6 MB(−48.7 MB,−37%),二进制 329.1 → 245.1 MB(−80.1 MB)**。尺子:stub 82(集合与 ws975 逐字节相同)、拒绝 29、可达 69;run976 连采五次:707 行 / 类型差异 0 / 0 panic |
| ws979+ws980 | **只是*装着*一个泛型的类,也得替它把界要出来**。`_MenuItem<T>` 的 `PartialEq` 只写了 `where T: PartialEq`,却比一个 `Option<DropdownMenuItem<T>>` 字段;而 `DropdownMenuItem<T>: PartialEq` 还要 `<T as DartNullable>::Or: PartialEq`——那句子句是按**本类自己**的投影字段拼的,`_MenuItem` 一个投影字段都没有,它只是**装着**一个需要它的东西(E0369)。**两套界都要**:每个类型参数一份,外加每个投影字段的**基**一份——基不一定是类型参数,`IterableProperty<T>._value` 是 `<Rc<dyn DartIterable<T>> as DartNullable>::Or`,要的是**那整个句柄**的 `Or`。**ws979 是我自己弄坏的**:它把投影基那一套**换掉**而不是加上去,于是清掉一个又弄出一个,我还在提交信息里把自己造的错说成「底下露出来的老错」。ws980 两套一起发,才真的掉了一格 | stub **82 → 81**(`_MenuItem.==`);拒绝 29、可达 69、0 error;run980 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 43 个,41 AGREE + 2 个故意的红 |
| ws981 | **`_comparableType` 走字段时看不见*投影字段*。**一个类只要有一个投影字段,derive 出来的 `PartialEq` 就得写 `<T as DartNullable>::Or: PartialEq` 这条 where——写不出来,于是这个类**根本没有 `PartialEq`**;而 `_comparableType` 照旧对它说「可比」,**装着**它的那个类的 `==` 就直接写 `self.x == other.x`(E0369)。两半一起改才成立:`generics.dart` 里 `f.type.projected` 一票否决,`emit_struct.dart` 里判定不可比的字段改走 `dart_eq`——只改前一半是把一个编译错误换成另一个。这条跟 ws979+ws980 是**两个不同的毛病**:那条是「装着泛型的类没替它把界要出来」,这条是「拿不到界的类被当成可比的」。我先把它俩当成一回事,又拿错了类去探针、据此写下一条否认 `_comparableType` 有份的「更正」,账本里那条撤回记录因此改了两次 | **stub 81 → 81,桩集合逐字节相同**——gallery 里没有「有投影字段的类,被一个没有句柄的类装着比」这个组合,这条**只在夹具上触发**。所以说清楚:**这是一条带夹具的正确性修补,数上是中性的,不是朝目标走的一步**;它跟 ws966 不同之处只在于 ws966 哪儿都不触发**且没有夹具**。拒绝 29、可达 69、0 error;run981 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **44 个,42 AGREE + 2 个故意的红** |
| ws982 | **循环变量被就地改动时,循环要走那个*位置*,不是它的克隆**。Dart 的 `final` 管的是绑定不是对象:`for (final childSet in ..) childSet.removeWhere(..)` 改的是循环递出来的那个 set,而这边每一轮都 `.iter().cloned()`,改在克隆上。**只给绑定加 `mut` 是错的**——那样编得过,答案却是空的:`fx/forinmut` 读出 `4,4/abbccc|dddde`,Dart 是 `2,2/abb+|e+`,**把一个桩换成了一个悄悄的错答案**,比桩还坏。改成借位置:prelude 补 `Set::iter_mut` 与 `Map::values_mut`(`values()` 收的是一个全新的克隆 `Vec`,这就是丢失的那一处),后端只在**找得到位置**时才借——调用的结果、形参的读都自带所有权,写不回去,那里照旧走克隆,该是桩还是桩。**判据错了两次,都量过**:先用 `_reassigned`(那是成员级的 `let mut` 答案,**把所有接收者都算进去**——`recipe.clone()`、`fields.clone().len()` 都会把局部标成「被改」),**+7 个桩**;再加两道闸(元素是句柄就跳过、元素类型不明就跳过)回到 81。真正的毛病只有一个:问错了问题。改成**只问这个循环体里、真正就地改动的调用**(`_WalkSelf.inPlaceLocals`,按 `mutatingNames` 的全名单,不动 `mutatingWalkSelfRustNames`——`remove_where` 在那张表里是 `member_names.dart` 明写「记下不补」的缺口,补它是另一轮)。**那两道闸事后各做了一次消融:79,桩集合逐字节相同,一处都不触发,按 ws966 的标准全部删掉**。夹具补了两段它原来睡过去的形状(边迭代边读集合、元素是句柄),并**验证过它真的会红**(把判据换回去,E0502) | stub **81 → 79**(`SemanticsNode.detach`、`sendSemanticsUpdate`);拒绝 29、可达 69、0 error;run982 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **44 个,43 AGREE + 1 个故意的红**(`forinmut` 转正)。**另外修了尺子自己**:`render_ruler.py` 在**工具根目录**读 `<log>.run`,而 `run_main.sh` 先 `cd .crate-ws` 再写——读不到就当成空树,报出来是 `nodes=0 panics=0 typediff=707`,**看着像全面崩盘,实际五个 118 KB 的日志里各有一棵完整的 707 节点树**。缺日志现在是硬错误,不是空树 |
| ws983 | **显式向上转型会*移走*它的操作数,而在闭包里那个操作数是捕获来的**。`x as Rc<dyn T>` 吃掉 `x`;`Scaffold.hitTestableAtOrigin` 在 `result.path.any((entry) => entry.target == renderObject)` 里比,那次转型把捕获的句柄消耗掉了(E0507,两个文件同一形状)。克隆一个 `Rc` 就是一次引用计数,所以**凡是转型的操作数*指着别人持有的那个位置*就先克隆**;表达式自己造出来的值(调用的结果、构造)没有位置要护,不克隆(判据用现成的 `_ownedWhenSpelled`)。**夹具试了六次才复现,每一次失败都是一条实情**:(1)闭包之后再读一次那个局部,后面那次读本来就会克隆,转型就没东西可移了——**第一版就是这样,绿的,什么也没钉住**;(2)把那次读删掉,TFA 就把唯一被读的字段摇掉了,于是每个 `Meta` 都相等,`absent` 出 `true` 而 Dart 是 `false`(和 `heldgenericeq` 踩的是同一个坑);(3)字段的读和「让这个类变成 counted 的那个闭包」都得留着,否则它不是句柄,转型走的是另一条分支;(4)(5)照着 gallery 的控制形状和转型目标改,依旧在上游被克隆;(6)**把被比较的字段改成非空**才复现——可空的槽会把比较包进 `Some(..)`,走的是另一条会插克隆的 coercion。这一条是**拿生成出来的 Rust 和 gallery 的逐字对比**找出来的(`Some(..)` 一直摆在那儿),不是靠猜哪条规则该触发 | stub **79 → 77**(`material_scaffold`、`cupertino_page_scaffold` 的 `hitTestableAtOrigin`);拒绝 29、可达 69、0 error;run983 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **45 个,44 AGREE + 1 个故意的红**。`upcastcapture` **验证过没有这条修改就是红的**(一模一样的 E0507)——第一版夹具是绿的,差点当成钉子交了出去,拦住它的是「把修改撤回去再跑一遍」这一步 |
| ws986 | **按*可空的键*查表,那张表是借来看的,不是搬走的**。`m[k]`(`k` 可能是 null)降成 `{ let __m = m; k.as_ref().and_then(|__k| __m.get(__k).cloned()) }`——绑定**按值**拿走了整张表。`get` 只要一个引用,`cloned()` 交回来的本来就是自有的值,所以那次搬移什么也没买到,却把第二圈循环赔掉了:「use of moved value ... in previous iteration of loop」(E0382)。改成 `let __m = &m`;`let` 绑一个借用会把临时量延到块尾,所以接收者本身是个调用时也活得够久。**一次就中,因为先对着生成出来的 Rust 把发射点逐字找出来了再动手**——ws984 是没做这一步,两趟链子都花在一个根本走不到的分支上 | stub **77 → 75**(`_SlottedRenderObjectElement._updateChildren`、`Table.update`);拒绝 29、可达 69、0 error;run986 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **46 个,45 AGREE + 1 个故意的红**。`mapgetnullkey` **验证过没有这条就是红的**(一模一样的 E0382)——循环是关键,只查一次的话搬走也编得过,这就是它藏到今天的原因 |
| ws987 | **`list.last = v` 要写进那个位置,而且这条路上一共缺三样,少一样就是错的**。(1)prelude 里**根本没有 `set_last`**——`Vec` 的 `last` 是 Rust 自己的切片方法,所以 rustc 说的是「有个名字像的 `last`,参数不一样」;(2)`set_last` 要进 `mutatingRustOnlyNames`;(3)**真正漏掉的一环**:`xs.last = v` 降下来是 `IrSetter` **不是 `IrCall`**,所以它从来没走到调用那条路上「会就地改动的调用要走位置」的判定——(1)(2)对这条路是**不起作用的**。只做前两样的话,链子的数**一模一样是 74**,但发出来的是 `borrow().clone().set_last(..)`:**编得过、写在拷贝上、悄悄什么也没干**,比原来那个桩还坏。夹具读出 `1,2,3/6/6` 而 Dart 是 `1,2,9/12/12`,是它把这一层照出来的——**两次链子的 stub 数完全相同,只有夹具能分辨**。`member_names.dart` 写着放宽这两张表是「对着链子量一轮,不是改一张表」,`test/member_names_test.dart` 钉着的三个数各加一,是照着改的,不是绕过去的 | stub **75 → 74**(`DiagnosticsNode.write` 的 `_wrappableRanges.last = wrapEnd`);拒绝 29、可达 69、0 error;run987 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **47 个,46 AGREE + 1 个故意的红**。`listsetlast` **两半都验过**:没有 prelude 那个方法是编译错,有方法没有位置是答案错 |
| ws988 | **翻译 trait 的 `==`/`hashCode` 改走对象自己的 vtable,不再查注册表**(`/tmp/work.md`「Object 协议:把注册表换成 vtable」第 1 步)。`impl DartEq for dyn <Trait>` 原来发的是 `dart_any_eq(self.as_any(), other.as_any())`——**一次 `TypeId` 哈希 + 一次 thread_local 表探测,绕一圈回到那个类自己的实现**。每个翻译 trait 都以 `DartAny` 为 supertrait(`pub trait Key: DartAny + Debug`),所以 `dyn Key` 的 vtable 里本来就有这四个方法。改成 `self.dart_eq_any(other.dart_any_ref())` / `self.dart_hash_any()`。**先证明再改**:`bin/vtable_probe.py` 把探针接在真 prelude 后面交给 rustc,两条都编过,**而且带对照**(去掉 `DartAny` supertrait 必须编不过——实测 5 个错,4 个 E0599 + 1 个 E0308 正是 upcast)。这一处非要编译证据不可:prelude 里关于它的两条注释**都是过期的**(`:393` 说后端给每个 trait 发 `impl Object for dyn X`,生成代码里 **0 个**;`:339` 和 `:639` 直接矛盾,639 对) | 生成代码里 `dart_any_eq(`/`dart_any_hash(` **各 758 → 0**(只剩 prelude 自己那两个 helper,而它们手上本来就是 `&dyn Object`,只是用 `.as_any()` 把 vtable 扔了);stub **74,集合逐字节相同**、拒绝 29、可达 69、0 error;**run988 连采五次:707 行 / 类型差异 0 / 0 panic——ws934 保住了**(这轮唯一的正确性判据就是它:`GlobalObjectKey` 比错了会让 `WidgetsApp` 每帧重建,树立刻不一样);夹具 47 个,46 AGREE + 1 个故意的红。**这一步不省尺寸**:表还在、4,449 次注册还在、`dart_register` 那 11.34 MB 还在,省的是运行期 |
| ws990 | **一个*字段*可以顶掉接口声明的 getter**。`Iterator<E>` 声明 `E get current`,Dart 允许用字段实现它——`_BoardIterator implements Iterator<BoardPoint?>` 写的是 `BoardPoint? current;`。后端**本来就会**给「自己是个 `Iterator` 的类」发 `impl DartIterator<E>`(`_emitPreludeInterfaces`),但它要求接口的每个成员都落在一个**方法**上;字段顶上的那个 `_forwardingCall` 答 null,于是**整条 impl 都不发**,这个类就压根不是迭代器了。字段按**它被持有的样子**读:counted 类的可变字段在 cell 里,所以是 `borrow().clone()` 不是裸字段(`_inCell`/`_isCopy` 决定),并且加一个标记让 `Result` 模型的 `.unwrap()` **不要**接在一个不是调用的东西后面。**第一版是错的,已撤回**:我另写了一个发射器,结果 `VerticalCaretMovementRun` 拿到**两条**一样的 impl——重复 impl 是 impl 层的错,打不了桩,**可达 crate 69 → 40**,连带 22 个桩「消失」(其实是它们的 crate 没了)。**那次失败自己就是诊断**:一个类有 impl、另一个没有,说明路径早就存在,该找的是它为什么跳过,不是再写一个。和 ws984 是同一个错,一天之内第二次 | stub **74 → 73**(`Board.iterator`);拒绝 29、可达 69、0 error;run990 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **48 个,47 AGREE + 1 个故意的红**。`iteratorfield` **验证过没有这条就是红的**(E0277,`DartIterator` 的界不满足);夹具里那个类是**counted**的,所以 `current` 在 cell 里、走的正是 gallery 要的那条读法——不 counted 的话会走另一条分支,钉不住 |

## 下一步

**不做**(量过的,仍然算数):nightly 的并行前端(第 65 轮,对名字解析无效);
按 SCC 拆 crate(第 40 轮,库图只允许并行两个);翻译 `dart:core`(第 44 轮,
+10347)。

**2026-09-05 那版「六条」的正文已删**(git 有,压缩点 `dbc6d961`)。六条里四条已解
(`Rc<dyn Fn>` 的 `PartialEq`——ws438 起集合相等一律 `DartEq`;`dynamic` vs `Object?`
——ws497 `Object?` 即 `dynamic`;多实现体的 trait 泛型方法——擦除孪生 `m__erased`,
ws482/ws494;refusal 归并 804 → 29,一直在按类别收)。还活着的两条:

- **Result 记账债**:函数值调用不参与传播、原语不进 Result;事件循环吃掉回调异常
  并报告(2593df32),债变成「报了但没人 catch」。新的一面见〈已知欠账〉:99.05%
  的函数带 `Result` 而只有 6.9% 会失败。
- **运行时 `Dart_*` 仍是 0/168**;`runtime/` crate(无头引擎)从 09-06 起存在。

**(2026-09-10 校注)** 现状:**ws980 81 stub / 29 拒绝 / 69 crate 全可达 / 0 error**,
运行尺子 run980 **707 行、类型差异 0、0 panic,连采五次一模一样**——尺子从 ws934
起不再抖(根因见〈已知欠账〉第一条),所以「读三次取多数」那套读法可以退休了,
一次就算数;为了看住回归,每轮仍连采五次。

**剩下的 90 个桩已经没有大簇了**,最大的一族是 3 个(`TickerFuture` 从 `Option`
里 await),其余都是一两个;而**拒绝这边还有一族 9 个**——win32 的 `_WindowsMessage`
/ `_WindowingInitRequest` 走 `dart:ffi` 的 `_loadInt32/_loadInt64/_loadPointer` 与
`Struct` 的 `#fromTypedDataBase`,是 29 个拒绝里唯一还成簇的。按性价比排,下一步值得做的:

1. **`List<T>` → `Rc<RefCell<Vec<T>>>`**(work.md 第 3 条)。量过的复制次数说它
   不是热点,所以立论只能靠**语义**——而语义的证据现在有了:`merge_sort` /
   `_merge_sort` 两个桩是同一件事,Dart 把**同一个 list 当两个参数**传进去
   (`_mergeSort(list, .., list, ..)`),`&mut` 借两次编不过。别名写得回去正是
   这个改动要买的东西。半径 6553 处 `Vec<` 拼写,前置三条(48541 个 `__v.push(`
   要留裸 `Vec`、`fn iterator(self: Rc<Self>)` 已验、`Rc<RefCell<..>>` 到
   `Rc<dyn DartIterable<T>>` 是 unsizing)都清了。

   **(2026-09-10 补正:这个拼法不是终点,是通往终点的唯一一步。)** 忠实的终点是
   `List<T>` 落成 **`Rc<dyn DartList<T>>`**——`List` 在 Dart 里是*接口*不是类,
   而今天类型下降见到 `List` 就无条件写 `Vec<T>`,**和 ws908 之前对 `Iterable`
   犯的是同一个错**(那次是见到 `Iterable` 就写 `Vec<T>`,于是「静态类型写着
   `Iterable`、手里是 `Set`」每处都是类型错误)。正被压平的第二、第三个实现在
   生成代码里数得出来:`Float64List` 123、`Uint8List` 72、`Float32List` 57、
   `Int32List` 44、`Int64List` 6、`ListQueue` 2、`UnmodifiableView` 1,**共 305 处**;
   ffi 那 5 个字段读写要的「`_typedDataBase` 共享且可变」是同一件事。

   但**终点今天造不出来**,两道门都只有 `Rc<RefCell<..>>` 开得了:

   - **`dyn DartList<T>` 不满足对象安全。** trait 上八个泛型方法
     (`first_where<F>`、`first_where_or<F, G>`、`remove_where<F>`、`retain_where<F>`、
     `fold_dart<R, F>`、`reduce_dart<F>`、`index_where<F>`、`last_where(_or)<F, G>`),
     一个 `where Self: Sized` 都没有。生成代码里 `dyn DartList` **0 次不是没人用,
     是写不出来**——对照 `dyn DartIterable` **851 次**,那个 trait 只有 `iterator`
     和 `dart_to_list` 两个方法,是 ws908 照普查特意做薄的。
   - **`DartList` 的写方法全是 `&mut self`**(`sort_by_dart`、`set_range`、
     `insert_all`、`remove_range`、`fill_range`…),`Rc<dyn ..>` 后面拿不到 `&mut`。
     要它们能用,只有内部可变一条路。
   - **`DartList<T>` 连超 trait 都没写。** Dart 里 `List` 就是 `Iterable`
     (`List<E> implements EfficientLengthIterable<E>`),Rust 这边该是
     `trait DartList<T>: DartIterable<T>`;而今天 `DartIterable<T>: DartAny` 写了,
     **`DartList<T>` 后面一个超 trait 都没有**——两者在类型系统里毫无关系,只是
     `Vec` 各实现了一份。补上之后 `Rc<dyn DartList<T>>` 才谈得上 upcast 成
     `Rc<dyn DartIterable<T>>`(trait upcasting,Rust 1.86 起稳定,本机 rustc 1.98)。
     **这条按本仓库的规矩要实测,不能假设**——`Rc<Self>` 做 `dyn` 接收者那条就是
     专门写十行 Rust 验过的(10a017dd)。

   **为什么不是直接用现成的 `Rc<dyn DartIterable<T>>`**(问过一次,记下来):
   `DartIterable` 只有 `iterator` + `dart_to_list` 两个方法,**只读**,而
   `dart_to_list()` 交回的是**副本**。拿它当 `List` 的拼法不是慢,是**错**——
   `list[i] = x` / `list.add(x)` 会写进副本再扔掉,**编得过、静静答错**,正是本文
   反复记的那一类。生成代码里它表达不了的操作:`.push(` 49011、索引 1203、
   `.len() as i64` 555、`.insert(` 434、`.remove(` 287、`.clear()` 134、`.sort_by` 17
   ——**约 5.16 万处**,每处都会变成「整表复制 → 在副本上做 → 扔掉」。反方向也丢:
   `DartIterable` 的实现者含 `Set`/`LinkedList`,拿它当 `List` 槽等于让一个 `Set`
   悄悄落进 `List` 槽,正是 ws908 修的那个错的镜像。

   所以层次是:**`Rc<RefCell<Vec<T>>>` 是表示,`Rc<dyn DartList<T>>` 是槽的拼法**,
   后者靠免费的 unsizing 建在前者上(已验)。而且**现在还不该上 trait**:
   `DartList` 的实现者只有 `Vec<T>` 一个,而 `DartIterable` 有四个(`Vec`/`Set`/
   `LinkedList`/+1)——`dyn` 的开销全付、多态一分不赚,要等 typed-data 视图真成为
   第二个实现者,而那件事的前置又是内部可变。次序是锁死的。

   **还欠一个普查。** ws908 动 `Iterable` 之前先量了两个数(`Iterable` 作为槽
   1662 处;一个体向它要的成员 25 个名字 501 处),trait 该多薄就是这两个数定的。
   **`List` 的同一份普查从没做过**——`DartList` 该多胖、哪些成员留在具体 `Vec` 上,
   今天没有依据。仪器现成(在类型下降那一处打点,不是文本 grep)。
   (2026-09-10 复量的半径:`Vec<` 拼写 6593 处、`__v.push(` 47996 处,与上面
   6553/48541 是同一个量在轮次间的漂移。)
2. **prelude 接口的成员经由对象调用**:`binarySearch<T extends Comparable<Object>>`
   的 `element.compareTo(v)` 落在裸 `T` 上没有方法。`_receiver` 只把类型参数窄化到
   **翻译出来的**抽象界;prelude 有 trait 的那几个(`Comparable`、`DartIterator`)
   同样可以经由 `dart_cast_to` 走一趟。半径很窄。
3. **`Sink<T>` 进 `_preludeInterfaces`**:`DigestSink implements Sink<Digest>` 没有
   `impl DartSink<Digest> for DigestSink`。表已经在那里(`Comparable`/`DartIterator`),
   要多一层「Dart 接口名 → prelude trait 名」的映射,因为 `Sink<T>` 是**句柄别名**
   (`Rc<dyn DartSink<T>>`),槽拼 `Sink`、impl 写 `DartSink`。
4. **增强枚举的每变体状态**:ws940 之后 `KeyboardLockMode.findLockByLogicalKey`
   是个桩,因为 `logicalKey` 是个 const 实例而不是字面量,`_EnumConstantFinder._literal`
   只认 int/double/bool/String。要收下它,普查得把 `Constant` 交给前端去降,
   而不是自己拼 Rust 字面量。

**拒绝还剩 29,逐条数过(ws973,从 `.crate/src` 里的 `NOT TRANSLATED` 数的)**:

| 组 | 个 | 要什么 |
|---|---|---|
| `dart:ffi`/win32 的指针读写、`_createNativeCallableIsolateLocal` | 9 | 一个 C 可调用的 trampoline 注册表;最后那个没有它就没有诚实的 Rust 实现 |
| 值类上的 `identical` | 5 | counted 那个决定。**现在拒绝是对的**:按位相等会给出**错的答案**,不是缺答案 |
| `const GZipCodec` ×2、`const JsonEncoder` | 3 | prelude 里真的 gzip 与 JSON 编码器 |
| `super` 进 `Object`(`==`、`hashCode`) | 2 | 故意 |
| `super`/`super(..)` 进不在本文件的类 | 3 | 一例一议 |
| `runZonedGuarded`、`identityHashCode` | 2 | 见下 |
| 其余 | 5 | — |

**桩这一头的形状(ws977 逐条看过的 82 个)**

| 按 crate | 个 |
|---|---|
| `scc_flutter_widgets` | 38 |
| `merged_foundation_leaf` | 7 |
| `flutter_cupertino_above` | 6 |
| 其余十来个 crate | 各 1–4 |

**已经没有大族了。** 最大的一簇是 24 个「mismatched types」,而那 24 个成因各不相同;
剩下的簇全在 2–6 个之间。这和前面十几轮不一样——那时一条规则能一次收掉 2–3 个。

抽查下来,**剩下的桩里有相当一部分要的不是「一条翻译规则」,而是运行时里缺的一块**:

- `LicenseRegistry.licenses` 要 `StreamController`——**prelude 里一个字都没有**,
  要的是整套 Stream(订阅、broadcast)。
- `CalendarDelegate<T extends DateTime>` 的 `T` 丢了界,因为 `DateTime` 是 prelude 的
  **结构体**;Rust 不能拿结构体当类型参数的界,得先给 `DateTime` 一个 trait。
- `_TaskEntry.run` 里逃逸的闭包读 `this` 的**字段**却没让类变成 counted
  ——`_countedClass` 只看方法调用。这一条属于那个 counted/别名工程,ws437 拓宽它时
  一次涨了 **+901** 个桩。
- win32 / `dart:ffi` 那几个,和〈拒绝〉里那 9 条同源。

另一部分确实是翻译规则:`equality.rs new`、`widgets_shortcuts.rs _value` 这些
「mismatched types」都是投影类型(`<E as DartNullable>::Or`)那条边界上的,和 ws957–ws964
收掉的是同一族。

**所以「桩归零」和「拒绝归零」是同一件事的两半**:再往下走,要么在 prelude 里补运行时
(Stream、ffi 回调、`DateTime` 的 trait),要么把 counted/别名那个工程做掉。**一条一条
磨规则的阶段,到 82 这里基本磨完了。**

**结论:这 29 条里没有一条是「谁忘了写个映射」。** 拒绝是因为诚实的答案还没有,
不是因为没人去接线——这正是「宁可拒绝也不猜」那条规则在起作用。两条看着最像顺手
可做的,查了call site 之后都不是:

- `identityHashCode` 的调用方是 `_IdentityThemeDataCacheKey.hashCode`——一个**专门
  拿身份做键**的类。按地址哈希一个值类,错的地方和值类上的 `identical` 一模一样。
- `runZonedGuarded` 还要接住 zone 里**异步**回调抛出的错。写成 try/catch 对同步的
  体是精确的,对异步那半是**静悄悄地吞掉**。

所以「拒绝归零」不是桩那种磨法能磨到的:它前面横着三个正经工程(ffi 回调 trampoline、
gzip + JSON 编码器、值类身份),外加约 7 条今天拒绝得**正确**的。

**(2026-09-09 校注)** 本节六条与〈当前队头〉自 09-05 起未动,现状以〈活账〉窗口
末行为准:**ws878 152 stub / 49 拒绝 / 64 crate 全可达**,运行尺子 run877
**708 行对 708 行、0 类型差异、0 panic、197 帧**。六条里 1(`todo!` 剩员)仍未再量;
2(拒绝)804 → **49**;3(Result 记账债)仍挂,并且长出了新的一面——见〈已知欠账〉
里 99.05% 的函数带着 `Result` 而只有 6.9% 会失败;4、5 已解(见 09-07 校注);
6 的 `runtime/` crate 存在且已是无头引擎,`Dart_*` 仍 0/168。
**队头现在不是一张类别表**:剩下的 152 个 stub 是长尾(最大的一个形状只有两个成员),
52 个拒绝里大半是四个「决定」而不是四条规则——见〈已知欠账〉的前两条与〈章结〉。

## 两半与三把尺子(定义;现状见〈下一步〉末尾的校注)


**运行时那半(`bin/embedder_api.py`,engine `0c2d270c5a9`)** ——不变:

| 数 | 是什么 |
|---|---|
| 168 / 312 | engine 真正调用的 `Dart_*`,945 处调用点(上界,还没收到启动路径上) |
| 231 | `dart:ui` 的下行 native |
| 19 | `PlatformConfiguration` 的上行句柄 |
| **1 层** | Rust 这边实现了的——`runtime/` crate 从 2026-09-06 起存在：无头引擎，见〈第二个 goal〉；`Dart_*` 仍是 0 |

**翻译那半(dill `0700f1e5`,`gen_kernel --aot --tfa --minimal-kernel` 出的
sig dill,前缀 `package:,dart:ui`,931 个库;尺子 `bin/stubs.py`,峰值 4–19 GB)**

尺子的轨迹见〈数字轨迹〉,现状见〈下一步〉末尾的 2026-09-10 校注。

三个数各量一样东西:**stub** 是「译出了、编不过」的函数;**refusal** 是
「没译出」的函数;**`todo!`** 是「编得过、一跑就 panic」的转发器体——
ws344 才照到它,一量 26199 个,削到 782。


**尺子的输入放在哪(2026-09-10 定)**

四把尺子都要吃东西:chain 要 gallery 的 AOT dill,夹具尺子要夹具语料,渲染尺子
要 Flutter 自己那份参考遍历。这些以前在 `~/dart2rust_build/`——**工程目录之外**。
2026-09-10 那个目录被清掉,一次带走了 dill、**41 个夹具的全部源码**和参考渲染树,
而仓库里没有一行说过它们怎么再造出来。

现在的规矩:**产物不出工程目录**,并且**尺子的输入要么在库里,要么有脚本能把它
再造出来**。

| 东西 | 在哪 | 怎么再造 |
|---|---|---|
| gallery 的 AOT dill | `.build/gallery/app_aot_sig.dill`(不入库) | `bin/gallery_dill.py` |
| 夹具**源** | `fx/*.dart`(**入库**) | 无——它们是验收测试,丢了就是丢了 |
| 夹具产物(dill/crate/日志) | `.build/fx/`(不入库) | `bin/fx.sh <name>` |
| 渲染尺子的参考 | `.build/scratch/ref_render_walk_settled.txt`(不入库) | `bin/render_ref.py` |
| 渲染尺子本身 | — | `bin/render_ruler.py <前缀> [次数]` |
| 夹具全扫 | — | `bin/allfx.sh` |

从转录里捞回了 39 个夹具,**`cmpscalar` 和另一个没捞回来**——它们那两次会话的
转录已经不在了。清点也是这时才做的:`bin/allfx.sh`、`bin/render_ruler.py`、
`bin/render_ref.py`、`bin/gallery_dill.py` 四个脚本以前每轮都是手敲的,一行都
没写下来过。


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

## Object 协议第二版:第 0 步过了,第 1 步的前提不成立(2026-09-10)

`/tmp/work.md` 换成了第二版:**不动毯式 impl**,改的是**手柄的类型**——
翻译代码里的 `Rc<dyn Object>` 换成 `Rc<dyn DartAny>`。`DartAny: Object + 'static`,
所以手柄自己就答得出整套协议,注册表跟着没用。第一版(拿掉毯式)已在 `ba3615b0` 撤回。
**`/tmp` 今天被清过两次,所以要紧的记这儿。**

**第 0 步:PASS**(`1903dc54`,`bin/dartany_probe.rs` + `bin/vtable_probe.py`)。四条都编过:
`dyn DartAny` 对象安全;翻译 trait 的手柄能 upcast 成 `Rc<dyn DartAny>`;协议从手柄上答;
**闭包那条通了**——`Rc::new(f)`(`f: Rc<dyn Fn(i64) -> i64>`)直接进 `Rc<dyn DartAny>`,
而这正是第一版死掉的那个点。**控制是真的**:只剪掉 `DartAny` 这个 supertrait,红出来的是
E0308(upcast 失败)和 E0599(协议方法),不是「名字没了」。两个探针的控制都改成了这种剪法。

**第 1 步:前提不成立,别照着做。** 计划说「裸 `Function` 槽把闭包拼成对象本身,改成拼成手柄」。
拿夹具量了三种形状(闭包字面量、局部函数按名字传、局部函数捕获且用不止一次——最后一种
和 gallery 的 `handleError` 一模一样),**三种发出来的都已经在 `Rc<dyn Fn(..)>` 手柄后面了**。
只有 gallery 的 `ImageProvider.resolveStreamForKey` 那一处发的是 `Rc::new({闭包})`,没有手柄,
**为什么不一样还没找到**。**已经找到了:触发它的是 `Future.onError`,不是 `catchError`。**
Dart 的 `catchError` 形参是**裸 `Function`**(没签名),走 `coerce.dart` 的 `slotObject` 分支、
被 `_dynamicFunction` 做成函数对象;`onError<E>` 的形参**是有签名的函数类型**,
`slotObject` 为假,于是既没做成函数对象也没进自己的手柄,直接 `Rc::new(闭包)` 进 `Rc<dyn Object>`——
**只有毯式 impl 撑着**,而第二版正要把那个槽换成 `Rc<dyn DartAny>`。
最小复现:`b.future.onError((Object e, StackTrace s) { return -1; })`,一分钟,不用跑链子。
**第 1 步的判据应该问 prelude 那一侧的槽(`Rc<dyn Object>`),不是 Dart 形参的类型。**
四个猜过的方向(闭包字面量 / 局部函数 tear-off / 捕获且用多次 / `async`)**都不是**,别再试。
另:v1 那次「74 → 98」改的是 `coercion.dart` 的 `_widened` 排除,**不是** `coerce.dart:510-521`,两处别混。

## 拒绝归零要什么(2026-09-10,量出来的,不是估的)

29 条拒绝按**日志里自己写的理由**分组(不是印象):

| 根因 | 条数 | 归零要什么 |
|---|---|---|
| `dart:ffi` / win32 | 11 | 读的那一半能做,**写的那一半做不了**(见下) |
| `identical` 落在不是引用的东西上 | 5 | 值类没有身份 → **别名/counted 那个工程**(ws437 量过 **+901 桩**) |
| 动态派发(`DynamicGet`/`DynamicInvocation`) | 4 | 真的按名字派发。**「名字在闭世界里只有一个声明者」只能解决 33 处里的 1 处**(量过:`slug` 有 2 个声明者,`toJson` 有 18 个) |
| `const GZipCodec` / `const JsonEncoder` | 3 | 真的 gzip 和 JSON 编码器——一个库,不是一条规则 |
| `super` 进 `Object` 的 `==`/`hashCode` | 2 | **不要动。ws985 量过:改了就是把一个对的拒绝换成一个错答案** |
| 零散(`runZonedGuarded`、`identityHashCode`、`is HttpException`、一处 super 进没翻译的类) | 4 | 各自独立 |

**ffi 那 11 条卡在同一个地方,而且和 `merge_sort` 那 2 个桩是同一个地方。**
CFE 把一个 `Struct` 子类摊成:两个字段(`_Compound._typedDataBase`、`_offsetInBytes`)、
一个 `#fromTypedDataBase` 构造器、以及一组 `_loadInt64(this._typedDataBase, X#offsetOf + this._offsetInBytes)` 的存取器。
`#sizeOf` / `#offsetOf` **已经翻得出来**(所以 `_abi()` 是通的),
字段和构造器有现成的一般机制可以照抄(`ByteStream extends StreamView` 那条:
「prelude 的基类没有结构体可摊,子类就自己带上它本该继承的字段,并且没有 Rust 超类」)。
**但 `Uint8List` 在这边是 `Vec<u8>`,一个值。** `_storeInt64(this._typedDataBase, ..)` 改的是一份拷贝,
后面那次 `_loadInt64` 看不见——**读能做,写不能做**。

所以真正的路只有四条,而且**第一条同时挡着三处**:

1. **别名的决定**(`List`/typed data 从值变成共享可变的对象)。挡着:ffi 的写路径、
   `merge_sort` 那 2 个桩(Dart 把同一个 list 同时当源和目标传进去,两个 `&mut` 表达不了)、
   以及 5 条 `identical`。STATUS 早就写过这个改动**要靠语义立论、不能靠性能**(ws911 那三个数)。
2. **ffi 的回调蹦床 + 一个分配器**(`_createNativeCallableIsolateLocal`、`calloc`)。
3. **gzip / JSON 编码器**。
4. **按名字的动态派发**。

**还有一条必须说清楚:「拒绝归零」按字面讲是达不到的。** 已经证明有 2 条(`super` 进 `Object`)
**今天就是对的**——`cc88ae36` 说 29 条里约 7 条属于这一类,ws985 把其中 2 条钉死了。
把它们「清掉」等于让编译器开始说假话。要么这 2 条(以及另外那几条)留着,
要么先做完第 1 条,让它们不再是对的。

## 桩尾还剩什么(2026-09-10,ws988 之后 74 个,逐条看过)

`b09ac222` 说「一条通用规则一轮就掉一格的阶段基本走完了」,这次把剩下的 74 个又过了一遍,确认了,并且把**每一条卡在哪儿**记下来,省得下次重新翻:

- ~~**`implements Iterator<E>` 的类没有 `impl DartIterator<E>`**~~ **已做(ws990)**,掉了 1 个
  (`Board.iterator`)。根因不是「缺一条发射路径」——路径本来就有(`_emitPreludeInterfaces`),
  只是它要求接口的每个成员都落在**方法**上,而 Dart 允许用**字段**顶掉 getter。

- **`Iterable` 的成员在「自己就是 Iterable 的类」上调不动**(剩下 2 个桩:
  `Board.copyWithBoardPointColor` 的 `elementAt`、`transformations_demo.paint` 的 `forEach`)。
  `Board extends Iterable<BoardPoint?>`,从 `dart:core` 继承了 `elementAt`/`forEach`,
  而这边它们是**写在列表上**的。类自己带着 `__to_list`(`_emitToList` 发的),
  **读它的那个东西是 `_listReceiver`**——但 `_listReceiver` 只接在 **for-in** 和几个按名字写死的成员上
  (`cast`、`remove`、`contains`、`char_at` 那几处,都是按 `owner == 'List'/'Set'/'Map'` 分的)。
  `Board` 的 `owner` 是 `Board`,一条都不匹配,于是落进通用调用,发出 `self.element_at(..)`,而结构体上没有这个方法。
  **判据是现成的**:`node.interfaceTarget.enclosingClass` 就是 `dart:core` 的 `Iterable`,
  接收者的静态类型又是一个翻译过的、`_iterableElement` 答得出来的类。
  **试过三次,全撤回了,三次的诊断都不对,记在这儿省得再走一遍:**

  1. **ws991(73 → 77)**:照 `StreamView` 那条的形状,在 `_instanceInvocation` 前面加分支,
     把接收者换成 `_listReceiver(..)`、成员名原样传。发出 `__to_list().r#where(..)`,而 `Vec` 上没有 `r#where`。
     **我当时把这条写成「换接收者就得换成员拼法」——那句话是错的,已改。**
     真相是:`iterStepNames` 早就把 `where` 映成 `filter`,而且**链子的降级本来就在调 `_listReceiver`**
     (`reads_and_calls.dart` 那个 `iterStepNames` 分支)。我的分支坐在它前面,**把它截胡了**,
     `r#where` 是我自己造出来的,不是缺一张表。
  2. **ws992(73 → 73,逐字节相同)**:以为 owner 是 `IterableMixin`,把它加进 `owner == 'List' || 'Iterable'` 那道闸。
     **一处都没触发。**
  3. **ws992b(73 → 73,逐字节相同)**:改成问「declaring 类是不是 `dart:` 下的 `Iterable` 子类型」。
     **还是一处都没触发。**

  **探针给的事实**(`node.interfaceTarget.enclosingClass`):
  `_MixinApplication386&Object&IterableMixin`,库是 **`dart:mixin_deduplication`**——
  和 ws953 踩的是同一个坑。但**光知道这个还不够**:按子类型问也没用,说明**根本没走到那道闸**,
  或者走到了也不改变结果。**下次先在那道闸上打一行 stderr,确认它到底触没触发,再动手改**——
  这三次都是先改后量,量出来 byte-identical 才发现连触发都没触发。

- **类型参数丢了 Dart 那边的界**(3 个桩:`CalendarDelegate<T extends DateTime>` 的 `year` ×2、
  `binarySearch` 的 `compare_to`)。**别再直接把界搬过去**——ws544 量过,**+252 个桩**:
  擦除孪生把参数实例化成 `Rc<dyn Object>`,而句柄不是那个 trait。`DateTime` 还多一层:它是 prelude 的
  **结构体**,Rust 没法拿结构体当界,得先让 `DateTime` 变成 trait。

- **别名工程挡着的**:`merge_sort` 那 2 个(Dart 把同一个 list 同时当源和目标传)、ffi 的写路径、5 条 `identical` 拒绝。见〈拒绝归零要什么〉。

- **要运行期的**:`Stream`/`StreamController`(prelude 一行都没有)、ffi 回调蹦床、真的 gzip/JSON 编码器。

- **下游的**:`Uint8Buffer.removeRange` 这种,是 `typed_buffer.rs` 那 3 个桩的下游,不是 prelude 缺方法。

## 已知欠账

**(2026-09-10)这一节的出场规则。** 三次压缩都只压尺寸、没定出场条件,于是 1266 行
在一天内涨回 1499——涨的 221 行几乎全在本节。**一条账在下面三种情况下从本节删除**
(git 留底,不写墓碑):

1. **做掉了**——落地的那一轮在〈活账〉里有行,这里不必再留一份。
2. **重量归零**——原来那个数今天量不出来了(仪器还在、条件变了)。销账时把新读数
   写进〈数字轨迹〉一行,本节删除。
3. **被取代**——结论被后来的读数推翻。**这一种要改写原文,不是加校注**:ffi 那条
   曾经同一件事三处三个数(16/11/9)、两个相反结论,就是靠加校注攒出来的。

校注只用于「原文没错但要补一层」;凡是「原文错了」,直接改原文。

- **两条从夹具语料重建时照出来的错(2026-09-10,ws970)**。两个夹具的转录版本比
  当时通过的版本多一截,那一截各自照出一个真错;把多出来的那截摘掉才绿,摘掉的
  东西写在这里,不是丢掉。两条都还没量过在 gallery 里值多少。
  - **map 里存的 `null`,取出来再 `as List?`,会 unwrap 一个 `None`**。
    `{'missing': null}` 的值是 `dart_null_object()`,`map['missing']` 因此是
    `Some(dart_null_object())` 而不是 `None`,`as List?` 的 downcast 就在
    `Some` 里 unwrap,panic。Dart 那边是 `null`。见 `fx/rawlistcast.dart` 的注释。
  - **界是可空的泛型函数,不能在不可空的实例化上调用**。`T extends String?` 的
    参数在签名里一律写成 `Option<String>`,`sentence<String>('hello')` 因此是
    「expected `Option<String>`, found `String`」(E0308),返回值也一样,进
    `format!` 又是一个 E0277。见 `fx/promotedor.dart` 的注释。

- **继承来的 `operator ==` 没被路由到**(2026-09-10,ws985 的夹具照出来的,还没量在 gallery 里值多少)。
  一个类从抽象超类继承 `operator ==`(不自己写),生成的结构体 `dart_eq` 用的是**按字段 derive** 的那个,
  `dart_any_eq` 从注册表里拿到的也是它,于是那个继承来的 `==` 一次都没被调用:
  `Leaf(1) == Leaf(1)` 这边 `true`、Dart `false`。Flutter 的 `Widget` 就是这个形状
  (它写 `==` 就是为了让子类别再定义相等),所以 gallery 里 widget 的相等**很可能是按字段比的,不是按同一性**。
  ws935 已经为 `Key` 修过同一件事的一个特例(`dart_any_eq` 去问注册表要类自己的 `==`),
  这一条是它的一般情形:**注册表里放的是 derive 出来的那个,而不是继承链上真正该赢的那个**。

- **`todo!` 的剩员从没再量过**。三把尺子里这一把量的是「编得过、一跑就 panic」的
  转发器体:ws344 第一次照到它,一量 **26199 个**,削到 **782**——**此后再没量过**,
  **勿引用 782 为新数**。桩扫到 0 的那天,决定「跑不跑得起来」的正是这个未知数。
  仪器就是当初那个,重跑一次的成本很低。

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

- **渲染树尺子抖了十几轮,根因找到了,从 ws934 起不抖**(叙事在活账 ws935)。
  根因是**一条 `==`**:`impl DartEq for dyn Trait` 写的是 `std::ptr::addr_eq`,按地址比;
  而 Dart 的 `==` 派发到对象,`Key` 的每个子类都重写了它。`_MaterialAppState` 建的是
  `WidgetsApp(key: GlobalObjectKey(this), ..)`,每次 rebuild 造一个新 key,两边比不等
  → `Widget.canUpdate` 说不能更新 → `Element.updateChild` 把整棵子树扔掉重建,
  **193 次/分钟**;重建出的 `_LocalizationsState` 在 delegate 装好前 build 出
  `SizedBox.shrink()`——那就是半数读数里的 6 节点树。修法:`dart_any_eq`/`dart_any_hash`
  向注册表要这个类自己的 `==`/`hashCode`,两个都没声明才退回同一性(Dart 的规则)。
  **留下的两件工具**:`DART2RUST_TREE_EVERY=n` 每 n 帧打一次树的大小(它扰动被测对象,
  只看形状、不看真值);预算用尽时报「还挂着哪些 future / 哪些 completer 没完成」。
- ~~**provider 的擦除账还剩 6 个桩**~~ **已清**(ws939,根因是继承链一致性规则的
  方向);后续那条「投影 `Or` 与裸参数在同一个类上混用」也已由 ws957 收掉。叙事在活账。
- **`erasedread` 夹具还没真正复现那条规则。**两端 AGREE,但生成出来的
  `Owner<T>` **没有被擦除**——协变扫描确实标了它
  (`TRACE_COVARIANT_SITE Owner<T> ... Owner<int> -> Owner<Object>`,
  `TRACE_COVARIANT Owner <T>`),而 `_erasedParameter` 仍答 false,原因没查出来。
  所以这条规则目前的证据是**链上的桩数**(2 个清掉、0 个新增)和
  provider 生成代码里那句 cast,不是夹具。夹具欠着。

- **`gallery_above` 那 42 秒是冷缓存,不是 crate 重**:`touch` 它的源码后
  连续两次 `cargo build -p dart_main` 是 **8.1 秒 / 7.8 秒**。所以「给大库
  分子模块」这条不要做,该想的是别让 build 那份增量缓存每轮都作废
  (链子重写了全部源码)。

**(2026-09-09 新增,ws908 自己换出来的)**

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
- **stub 的长尾**(数随轮次动,现状看〈下一步〉末尾的校注):最大类是
  "mismatched types",随运行尺子推进逐站收。
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
后面 4 个的内存来自一个 Windows DLL(`_winCoTaskMemAlloc`),在这台机器上
本来就该死在那个调用处,不该在这里假装有内存。

**所以这一族的结论是**:「给 `Pointer` 一个内存模型」这个大决定**不用做**——
`Rc<RefCell<Vec<T>>>` 落地时 5+2 个顺手掉,剩 4 个应该一直拒绝下去。ws969 的
29 个拒绝里 ffi 这族是 9 个,是唯一还成簇的。(本条 2026-09-10 由一条写在
〈已知欠账〉后段、结论相反的旧文并入——那条写于 ws894 之前,说的是 16 个拒绝、
「这是一个决定」,已删。同一件事一度三处三个数、两个结论,就是加校注攒出来的。)

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
- **`dart:ffi` 的内存模型**:结论在上面〈`dart:ffi` 结构体剩下的一半〉(ws894 量过),
  这里不另记一份。**要点:那个「给 `Pointer` 一个内存模型」的大决定不用做。**
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
  产物在 `.build/scratch/`(`target-release/`、`libapp_x64.so`、
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

## 常量聚合的普查(2026-09-10)

「常量数据被写成了代码」这件事,此前只从二进制符号反推过(`15.3 MB / ~67 个函数`)。
`lib/aggregates.dart` 从 IR 数了一遍,`bin/aggregate_census.dart` 是它的普查。
**不改代码生成**——链子里没有任何文件 import 它们,所以尺子一格没动,也没有重跑。

**数据构造器**:只把参数、常量默认值、别的数据构造器写进字段。**2666 / 4029(66.2%)**,
5 轮收敛。有两件事不对就一个也数不出来:

- **不动点要跑遍整个 component,不是被翻译的那些库。** super 链的尽头是 `dart:core`
  的 `Object`;只问被翻译的库,3214 个构造器**全部**出局,2594 个的理由是
  「看不见的 super」。什么会被翻译、数据构造器能够到什么,是两个问题。
- **在两个数据之间挑一个,仍然是搬数据。** `TextSpan.mouseCursor` 是
  `let #0 = mouseCursor in recognizer == null ? const _DeferringMouseCursor{} : const SystemMouseCursor{kind:"click"}`,
  就这一个字段,让 `TextSpan`——全程序 45,732 次构造——不合格。`EqualsNull` 是空检查
  不是 `operator ==`,所以 `EqualsCall` 不在放行之列。

**对上了的:** `code_segments.dart` **63 棵树、45,732 次构造**,和反汇编数出来的
`TextSpan::new` 次数**一模一样**。63 棵**共用一个形状**
`TextSpan(children:[TextSpan(style:I,text:L)])`——「63 个函数共用 1 个 builder」由 IR 证实;
每棵 **6–8 个**互不相同的不变量读,正好是那个 `u8` 列。

**改掉的成本模型:按构造次数算钱,最多错 8 倍。** `raw_keyboard_android` 是
1003 次构造、35 KB;`dateSymbols` 是 98 次构造、1.16 MB。对着 `nm -S` 拟合出来是
**215 B/构造 + 85 B/字符串叶 + 2 B/标量叶**——一个字符串叶就是一次 `to_string()`,
一次分配加一次存。一个 span 是 1 构造 + 1 字符串 + 1 标量 = 302 B,对上了指令直方图
数出来的 304 B。**所以阈值要落在字符串叶上,不是构造次数上**:N=200 时
**62 棵树**占全部字符串叶的 **85.8%**,投影 15.26 MB,其中 **15.13 MB 是量出来的、
不是外推的**。拟合在能对账的地方站得住(三个大模块 1.00 / 1.04 / 1.15),
在 60 KB 以下的模块**高估 2–4 倍**(rustc 把小树折成了静态量),
所以全程序那个 **20.93 MB 是上界,只有它的头部算证据**。

**普查自己的一个洞(用户找出来的,当天补上)。** `_disqualifies` 只看了构造器的 body 和
`c.initializers`,而**类自己的实例字段初始化式在 Kernel 里留在 `Field.initializer`**,
不在那张表里。于是 `_AppBarDefaultsM2._theme = Theme.of(this.context)` 这种**在字段声明里
打电话**的类,构造器照样被判成「只搬数据」——生成的 Rust 里那次调用就在 `new` 中间。
**77 个构造器是脏的,但报出来的数一个也没受影响**:那 77 个里只有 23 个出现在持有聚合的成员上,
2812 行 TSV 里形状提到脏类的是 **0 行**——脏构造器在成员里,不在被计数的那棵树里。
补法:把类里没被 `FieldInitializer` 覆盖的实例字段也过一遍 `_pureData`。
补完 **2666 → 2546**(直接判掉 96,不动点又连坐 24),**聚合那一侧逐字节相同**
(2812 棵树 / 68,084 次构造 / 71,747 个字符串叶,阈值表一行没动)。
**这个洞不影响读数,但定义是错的**:第一层一旦拿它驱动代码生成,
`_AppBarDefaultsM2(context)` 会被摊成一张静态表,`Theme.of` 那次调用直接消失。
顺带查干净的两处:**super 没有洞**(放行的构造器里「没有 Super/Redirecting initializer
但父类不是 `Object`」的有 0 个,这个 dill 里 super 全是显式的);
**工厂构造器不会误入**(它走 `StaticInvocation`,`ConstructorInvocation.target` 天然够不着)。

**另外两条:** `dateSymbols` 一棵树 **0 个不变量**——纯常量,不需要对 getter 做任何判断,
而它一个成员就是 1.16 MB。出局理由的长尾是「看不见的 super」545 条、
「字段初始化式在算」523 条;普查同时说了,头部之外的收益不大,所以现在不值得再放宽。

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
         **(2026-09-10 correction)** Superseded. ws894 cut this group to 11
         and counted what was left: none of it turns on a memory model.
         See "`dart:ffi` 结构体剩下的一半" under 已知欠账.
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
