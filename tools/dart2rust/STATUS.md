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

- **把 `Stream.listen` 的槽改成 Dart 声明的那些类型,再去接 `ByteConversionSink`**——
  **错误挪了两次然后不动了,尺子一格没动,撤回**(ws1067,2026-09-11)。
  四步都做了,链条 1m52–1m56,拒绝 12、stub 37(集合逐条相同)、可达 69,**一格没动**。
  **但前两步立住的事实是真的,下次从这里开始,别重新推**:
  ① **`listen` 的 `onError` 是这份 prelude 自己发明的类型**。Dart 声明的是裸 `Function?`
  (一个实参或两个,运行时按 arity 挑),而 prelude 写成
  `Rc<dyn Fn(Rc<dyn DartAny>, Option<StackTrace>) -> ()>`;
  **arity 那条规则 prelude 里本来就有**(`dart_call_error_handler`),
  `handle_error`、`catchError`、future 那几条路早就用的是 `Rc<dyn DartAny>`,**只有 `listen` 是例外**。
  改成 Dart 的类型之后,错误从 `onError` 挪到了 `onData`。
  ② **`onData`/`onDone` 写的是 `-> ()`,而本编译器发出的每一个闭包都返回 `Result`**
  (量过:生成侧 9,068 个 `dyn Fn` 槽,**无一例外**带 `-> Result`)。
  给它们 `Result`、让 `listen` 往外送(并进 `_preludeFailing`),错误又挪了一次,
  挪到 `ByteConversionSink::new`。
  ③ **`ByteConversionSink` 是 `Rc<dyn DartSink<Vec<i64>>>` 的别名**,
  Rust 不允许给 `Rc` 写 inherent `impl`,所以 `ByteConversionSink::new` **永远解析不到**;
  构造函数在 `_ByteCallbackSink` 上。
  **我两次想把它重定向过去,两次都没生效**——`nullaware.dart` 的无名工厂分支和具名构造分支都加了表,
  发出来的还是一模一样的 `ByteConversionSink::new`。
  **说明发射点不在我找的那两处**。按〈stubdiff 比的是集合〉那条:
  **报错一字不差就是规则没生效,要插桩,不是再猜一次**——我猜了两次,所以停手。
  下一次从 trace 开始:先问清楚 `ByteConversionSink.withCallback` 是从哪儿发出来的。
  顺带:`to_bytes` 和 crypto 那三个桩(`hash_super_convert`、`start_chunked_conversion`、
  `hash_sink_super__iterate`)同属 `DartSink` 一家,值得一起收。

- **给 prelude 的 `Comparable<T>` 加上 `DartAny` 上界、并补一个 `comparable_compare`**——
  **它会让 `_sort` 从「桩」变成「能编译但一定 panic」,所以撤回**(ws1058,2026-09-11)。
  链条读数是「拒绝 17、stub 38、集合逐条相同」,看起来什么都没发生;
  但**按〈stubdiff 比的是集合〉那条,0/0 不等于没变**——去读 `_sort` 的报错,它**挪了**:
  `dart_cast_to` 原本连方法都不是(`trait bounds were not satisfied`),加了上界之后能调了,
  下一个缺的是 `Comparable.compare` 这个 dart:core 的静态(`comparable_compare`,本编译器把抽象类的静态写成自由函数)。
  两个都补上之后**夹具编译通过、运行时 panic**:`called Option::unwrap() on a None value`。
  原因是**擦除后的实例化**:`Comparable.compare` 在 Dart 里声明在 `Comparable<dynamic>` 上,
  所以后端发出的是 `dart_cast_to::<dyn Comparable<Rc<dyn DartAny>>>()`,
  而一个 `implements Comparable<Weight>` 的类只为**它自己那个实例化**作答——标量同理
  (`int` 是 `Comparable<num>`,`String` 是 `Comparable<String>`)。
  **真正要做的是擦除实例化那一半**(prelude trait 的 erased twin:让 `dart_cast` 为擦除实例化作答,并在边界上适配实参),
  上界和 `comparable_compare` 只是它的前置。
  **为什么这条必须撤回**:桩是**被尺子数着**的,运行时 panic 不是——
  这么换会让 stub 从 38 掉到 37,而程序并没有变对。**桩只能诚实地掉。**

- **把 `super.==` / `super.hashCode`(落进 `Object` 的那两个)按身份答出来**——拒绝 22 → 20、桩数不动、
  可达 69,**读数是好的,还是撤回了**(ws1052,2026-09-11)。**夹具把它拆穿了**:
  `dart_self_<trait>()` 对一个**值结构**来说是 `Rc::new(self.clone())`——**每次调用现装一个新的**,
  于是 `dart_identical_any(&这个, &那个)` 永远为假(`w == w` 在 Dart 里是真),
  `dart_identity_hash_code()` 每次是另一个地址(`a.hashCode == a.hashCode` 会是假)。
  **这正是这段代码原来的注释写着的那句话**:「身份是被复制的值类没有的东西」——ws828 那次也是栽在这里,
  只不过那次栽在实参上,这次栽在接收者上。超函数是所有实现者共用的,`Widget` 的实现者是不是 counted
  在那里根本判断不了,所以这条**要等 aliasing/counted 那个决定**,不是能单独做的。
  夹具顺带**又量到一次**〈已知欠账〉里 ws985 那条(继承来的 `==` 没被路由到):
  `a == b` 两个不同对象答成 true,Dart 是 false。**新的一点写在那条账下面**。

- **给 `DartFuture<Object>` 加 `dart_cast_future`、在 `coerce` 里把它转成 `Future<T>`
  (想清掉 `scheduleTask` 那两个桩)——**逐字节相同,规则一次都没触发**,已撤回
  (ws1042,2026-09-11)。**规则没错,是那条边上根本看不见这个差**:`DART2RUST_TRACE_RETURN`
  打出来是 `have=Future<T> slot=Future<T>`——IR 两边**本来就一致**。
  错配只存在于**发出来的 Rust** 里:`_TaskEntry<T>` 被擦成了非泛型,
  `completer` 是 `Completer<Rc<dyn DartAny>>`,而 `entry.completer.future` 的类型是
  `_memberRustType` 按**接收者的 Dart 静态类型**算的——Dart 的静态类型不知道擦除这回事。
  **要动就得动那里**:成员读的类型应当跟着**降下来的接收者的 `rustType`** 走,
  而不是跟着 Dart 静态类型走;那是每一次成员读都经过的路,不是一轮能量完的改动。
  夹具复现不出来:同样形状的 `Entry<T>`(一个 `Completer<T>` 加一个 `Future<T> Function()`)
  在夹具里**没被判成协变**,于是没擦除,两边一致——**试过、不收在 `fx/` 里**:
  它两边都只打印空串,连它要钉的那件事都失败不了,这种夹具比没有更坏。
  真要复现,得先弄清协变扫描为什么在 gallery 里标了 `_TaskEntry<T>` 而在夹具里不标
  (`DART2RUST_TRACE_COVARIANT`)。

- **往 `_preludeInterfaces` 里加 `Sink`(想给 `DigestSink implements Sink<Digest>` 补上
  `impl DartSink`)——**可达 crate 69 → 65**,两趟都没救回来,已撤回(ws1033/1034,2026-09-11)。**
  **往那张表里加一项不是局部动作**:`the_class.dart` 只在「prelude 有这个 trait」时
  才把接口列进抽象类的**父 trait**,所以加进去以后 `Sink` 立刻成了
  `pub trait HashSink: .. + Sink<Vec<i64>>` 的一员——而 `Sink<T>` 是
  `Rc<dyn DartSink<T>>` 的**别名**,不是 trait,报 "expected trait, found type alias"。
  这条错**在函数外面**,打不了桩,于是连crate带下游一起掉(和 ws990 那次 69 → 40 同形)。
  第二趟把父 trait 也按 trait 名拼(加了一张 `_preludeInterfaceTraits`),别名的错没了,
  接着是**实现者不满足**:`_Sha256Sink` 通过 `HashSink` 继承到 `DartSink<Vec<i64>>` 的要求,
  自己却没拿到转发 impl(`_forwardingCall` 找不到,或它并不直接 `implements Sink`)。
  **最要紧的一条**:ws1033 的桩集读数是 **54 → 49、掉 5 个**——看着是本轮最好的一次,
  其实那 5 个是**跟着 4 个 crate 一起消失的**。目标里「可达 crate 不掉」是独立的一条,
  就是为了照出这种事;只看桩数会把回归读成进展。
  真要做,得先让**每个实现者**都拿得到转发 impl,再谈把接口列进父 trait。

- **给「TFA 判死的接收者」上的成员调用加个类型绑定——量出来逐字节相同,已撤回
  (ws1009,2026-09-10)。**
  **更正(同日,提交信息里写错了):这一组是 2 个桩,不是 3 个。**
  我当时按「detail 块里出现过那行 TFA 文本」分组,`_handle_entry_mode_toggle`
  是被这么误收进来的——它体内别处有一行 TFA,但它自己的错是
  `?` 作用在 `Option<DateTime>` 上,和这条无关。真正属于这一组的是
  `_buildDayPicker`(`can't compare () with i64`)和
  `_adjustSelectionIndexBasedOnSelectionGeometry`(`can't compare () with
  SelectionStatus`)。**分组要按每个桩自己的首条 error 分,别按整块文本 grep。**
  两个桩同一个成因:TFA 在证明不可达的地方种一行
  `throw "Attempt to execute code removed by Dart AOT compiler (TFA)"`,前端降成
  `unreachable!(..)`;它上面的成员读被 `calls.dart` 的 `if (_neverReturns(target))
  return expr(target!)` **整个丢掉**,剩一个没有类型的 `!`,Rust 的 never-type
  fallback 填成 `()`,于是 `widget.minimumDate!.month == selectedMonth` 变成
  `{ unreachable!(..) } == i64`,报 "can't compare `()` with `i64`"。
  改法是在那里按调用本该产生的类型绑一下(`{ let __never: T = ..; __never }`,
  `!` 在 `let` 上会 coerce)。**发射了 5 处,桩集逐字节相同 70/gone 0/new 0**——
  那 5 处本来就编得过,而**出错的 3 处根本不是带 `resultType` 的 `_call`**
  (`.month` 那种大概是 getter/字段读那条路)。下次要动先把出错那 3 处**是哪个发射点**
  打出来,别再从 `_call` 猜。
  **另外:这条的形状钉不进夹具。** `fx/tfadead.dart` 写了(手写 TFA 那句 throw、
  表达式位置的 throw、`?:` 里的、以及 `x?.a == b && x!.a > c` 这种空断言),
  **第一次跑就是绿的**——真正的成因是 TFA 在**整个程序**范围内证明 `minimumDate`
  永远为空,单库夹具里 TFA 不会下这个结论,验过确实不会。夹具留着当回归网
  (那三种形状将来必须继续编得过),但这一组的判据只能是链子的桩集差。

- **Object 协议:把注册表换成 vtable,第 2 步(拿掉毯式 impl)——渲染树红,已撤回
  (2026-09-10,work.md 那份计划的主体)。**
  **注**(2026-09-10 晚):作废的是**这条路**——「拿掉毯式 `impl<T: 'static> Object for T`、
  把协议方法搬到 `Object` 上」。目的地本身在 `ws1005`/`ws1006` 到了,走的是另一条:
  **不动毯式,改手柄的类型**(`Rc<dyn Object>` → `Rc<dyn DartAny>`)。
  下面这一整段的代价——117 个 `impl Object for`、255 个枚举错误、2,319 个 E0061——
  **一个都没出现**,因为那条路一个 `impl Object for` 都不加、一个自由函数都不加。 第 0 步(证明够得着)和第 1 步(翻译 trait 走 vtable)
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
| ws1008 | **一个进到列表*元素类型*里的实参必须被 coerce 进去,而以前只有「比较元素」的三个成员做了这件事**。`{remove, indexOf, lastIndexOf}` 是**比**一个元素的;**放**一个进去的 `add`/`insert` 问的是同一个问题,漏了。写成集合表达不了,因为元素不总是第一个实参——`insert(index, element)` 在第二位,所以改成一张「哪个位置的实参是元素」的表 `listElementArgument`。**顺着挖出更深的一层**(是仪表打出来的,不是猜的):coercion 的闸门读 `lowered.rustType`,而**闭包在有槽给它类型之前没有自己的类型**,于是任何闭包到达一个已拼出的槽都**根本没被问过**——槽是函数类型时,那个槽就是答案(`coerceInto` 的 `!rc`)。闸门上方那个 `_untypedCensus` 一直在数这些漏网的。**`fillRange` 进过表又被量出来退掉了**:Dart 声明是 `fillRange(int, int, [E? fill])`,槽是「元素或 null」,prelude 里是 `Option<T>`;按 `E` 去 coerce 就是 `f64` 撞 `Option<f64>`,在 `SliverMasonryGrid.performLayout` 多出一个桩(ws1007 读数 71→71:掉 1 新增 1,不算数,退掉重跑) | stub **71 → 70**(掉的是 `cupertino_date_picker.rs build`,新增 **0**);拒绝 29、可达 69、0 error;run1008 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 **50 个,49 AGREE + 1 个故意的红**。`listinsertfn` **验证过没有这条就是红的**,而且红法和 gallery 逐字节同形(expected `Rc<dyn Fn(i64) -> ..>`, found closure);它同时钉住两种 tear-off:实例方法捕获 `this` 是闭包,顶层函数是裸 `fn` item(`cupertino_route.rs` 那个) |
| ws1010 | **和一行「AOT 判死的代码」比较时,死的那一边按活的那一边的类型拼出来**。TFA 在证明不可达处种一行 throw,前端降成 `unreachable!(..)`;它是个 `!`,而没有东西约束的 `!` 会被 never-type fallback 填成 `()`,于是 `A == B` 去要 `(): PartialEq<i64>`,没人实现——`_buildDayPicker` 里 `widget.minimumDate!.month == selectedMonth`(这个 gallery 从不给 `minimumDate` 赋值,所以那个 `!` 是死的)。绑一下就够了,`!` 在 `let` 上会 coerce。**只管比较,这不是留余地**:`&&`/`||` 两边各自是 `bool`,不需要统一,fallback 无害——全程序另外约 50 处发散操作数今天就是这么编过去的,一处没动;比较要 `A: PartialEq<B>`,不定的 `A` 才是问题本身。**先仪表后改**:一趟 translate 的 trace 直接点名发射点是 `_binary`(不是 ws1009 猜的 `_call`)、正好 **3 处 / 2 个桩**、而且两个操作数**本来就带着类型**(`IrBlockValue/Never` 对 `IrField/int`、`IrStatic/SelectionStatus`),所以改动只有五行,落点和预言一致 | stub **70 → 68**(掉的正是预言的那两个,新增 **0**);拒绝 29、可达 69、0 error;run1010 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 51 个,50 AGREE + 1 个故意的红。**这一组的凭据比本轮其他组弱,写明**:`fx/tfadead.dart` 没有这条也是绿的——TFA 是在**整个程序**范围内得出 `minimumDate` 恒空的,单库夹具逼不出这个结论(ws1009 验过),所以判据只有链子的桩集差 + 渲染树,夹具只当回归网 |
| ws1013 | **函数引用要在*它自己*那一层 unsize,但只有在「目标已知且全是位置参数」时才能钉**。`Rc::new(f)` 对一个函数项是 `Rc<fn item>`;unsize 作用在指针上,Rust **不会伸进 `Option` 里**去做,所以 `Some(Rc::new(_buildCupertinoDialogTransitions))` 是 `Option<Rc<fn item>>`,而槽要的是 `Option<Rc<dyn Fn(..)>>`(`CupertinoDialogRoute` 的 `transitionBuilder ?? _buildCupertinoDialogTransitions`)。用的是代码里现成的办法:`{ let __f: T = ..; __f }`,和闭包那条(`_call` 的 `!rc`)同一个。**两次自己造的回归,过程比结论有用**:(1)一上来无条件钉 → **新增 5 个桩**。原来那个不 unsize 的 `Rc<fn item>` 是**故意**留着的——Rust 里参数顺序是*声明*顺序,而槽的函数类型可能换了顺序,`_namedOrderAdapter` 在后面收拾;提前钉死就把错的签名冻住了(`(alignment, alignmentPolicy)` 撞 `(alignmentPolicy, alignment)`)。(2)改成「目标有具名参数就不钉」→ **还剩 1 个**:构造器 tear-off(`RoundedRectangleBorder::new`)根本不在 `target` 的查找范围里,查出来是 null,而我把「不知道」当成了「可以钉」。(3)反过来才对:**目标已知、且每个参数都是位置参数**才钉——那正好就是「下游不欠任何 adapter」的条件。**教训**:改一个拼法之前先查清楚**谁依赖现在这个拼法**;`_namedOrderAdapter` 就在那儿,该先读它 | stub **68 → 67**(掉的正是起点那个 `cupertino_route.rs new`,新增 **0**);拒绝 29、可达 69、0 error;run1013 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 52 个,51 AGREE + 1 个故意的红。`optfnslot` 钉住三种可调用形状(顶层 `fn` 项 / 实例方法 tear-off 的闭包 / 闭包字面量)、`??` 的两侧、以及经过 `super(..)` 的那条——**这四种单独都是绿的**,真正红的是它们**套在 `Option` 里**那一种,夹具留着当回归网 |
| ws1014 | **声明类型只有在它*真的说明了可空性*时,才有资格盖过记录下来的类型**。`_nullChecked` 会先看操作数的**声明**类型——这对「被流分析收窄过、而 Rust 字段仍是 `Option`」的读是对的。但 `flexes[x]!`(`List<double?>`)的被调者是 `List.[]`,它的声明返回类型是**光杆参数 `E`**,`TypeParameterType(List.E%)`——`%` 就是「可空性未定,随实例化而定」,在这个调用点上什么也没说。被当成非空之后 `!` 就被丢了,于是 `newTotalFlex += flexes[x]!` 发出来是 `f64 + Option<f64>`(`RenderTable._computeColumnWidths`)。**先仪表后改**:trace 一行就把两个我自己的猜测毙了——不是复合赋值的事(`2.0 * flexes[0]!` 一样红),局部变量那条从来没坏过(`VariableGet declared=InterfaceType(int?) → check`,`InstanceInvocation declared=List.E% → DROP`)| stub **67 → 65**(掉的是 `_computeColumnWidths` 和 `_handleEntryModeToggle`,新增 **0**);拒绝 29、可达 69、0 error;run1014 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 53 个,52 AGREE + 1 个故意的红。`compoundbang` **改之前是红的,4 个 error**,钉住 `+=`/`-=`/`*=`、纯表达式、列表元素与可空局部两种来源。**顺带更正 ws1009 的一处分组**:`_handleEntryModeToggle` 当时被我归进 TFA 那组,其实它的 `?` 作用在 `Option<DateTime>` 上,根子就是这条掉了的 `!` |
| ws1015 | **`dart:ffi` 的读那一半通了,拒绝第一次动:29 → 22**。三件事,每件都是 trace 出来的不是猜的:(1)**「没有结构体可摊的 prelude 基类」原来是一个名字判断**(`_isStreamView`),现在把规则写一次、用一张表说它有哪些实例(`dart:async` 的 `StreamView`、`dart:ffi` 的 `Struct`);(2)**要带的字段得沿整条基类链找,不是 `base.fields`**——trace 打出 `chain=[Struct:[], _Compound:[_typedDataBase, _offsetInBytes], Object:[]]`,`Struct` 自己一个字段都不声明,这正是结构体发出来是空的、每个 getter 无处可读的原因;(3)**super 构造器的位置实参按顺序填这些字段**,两个实例一次量到 (`StreamView params=1 args=1 carried=[_stream]`、`Struct params=2 args=2 carried=[两个]`),数目对不上就拒绝,不猜。再加 prelude 里三个读原语(按 `dart:ffi` 规定的小端布局在偏移处读)。**只做读**:`Uint8List` 在这边是值,写会落进一份拷贝、下一次读看不见——那是别名那个决定,不是疏漏。**走错一次**:三个名字先按 Rust 拼法注册,而那个检查跑在 `snake()` **之前**,比的是 Dart 名字,白跑一趟 translate | **拒绝 29 → 22**(掉的是 `_WindowsMessage` 五个 getter 和两个 `#fromTypedDataBase`;剩下的 `onMessage` 要回调蹦床,写那一半按上面的理由留着);stub **65 逐字节相同**(gone 0 / new 0)、可达 69、0 error;run1015 连采五次:707 行 / 类型差异 0 / 0 panic。`ffistructread` **两端都是 `0/0/0/0/24`**,钉住读路径和 `sizeOf`。**`ffistruct` 的红换了种红法**:以前是整个结构体被拒绝、编不过,现在编得过、跑到第一次*写*才 panic——夹具头注和 `allfx.sh` 的说明都跟着改了,那句「translator refuses on purpose」已经不成立 |
| ws1017 | **`x?.m(..)` 里 `m` 要 `&mut self`——ws984 撤回两次的那条,这次掉了一个。**关键是**闸门和位置各自在替对方回答问题**:闸门问「这名字是不是*集合*修改器」(`_mutatesInPlace`),`_cellPlace` 问「格子里装的是不是集合」——**两个都不是真问题**,真问题是**被调方要不要 `&mut self`**。直接问被调方,两个替身一起消失:`_cellPlace` 加一个 `anyHeld` 开关(只在被调方已确定时跳过集合判断),闸门加一条按被调方声明回答的 `_mutatesSelf`。**ws984 记的原因是错的**:它写「接收者不是局部变量所以三条位置规则都算不出来」,trace 打出来是 `recv=IrField/... mutPlace=null`——**是字段,而且字段就在格子里**,卡住的是深一层那句 `if (!_isMutableCollection(held)) return null;`。**判据也选错过一次**:我先用 `_sharedMutation`,不发火——`drag_end` 是固有方法,它的 `&mut self` 来自 `_mutating`;对的写法是照抄 `_receiverOf` 的真实判断(counted → `&self`,否则 `_sharedMutation || _mutating`)。**而且走格子的 `borrow_mut()` 本身就是 `&mut`**,ws984 走局部绑定才需要的「第二半」(给绑定加 `mut`,缺了就是 57 个桩)**这里根本不需要** | stub **65 → 64**(掉的是 `cupertino_route._handle_drag_cancel`,新增 **0**);拒绝 22、可达 69、0 error;run1017 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 54 个,53 AGREE + 1 个故意的红。**两个目标只掉了一个**,另一个 `services_restoration._removeChildData` 的接收者是 `_childrenToAdd[key]?.remove(child)`——**map 的下标读**,位置得是 `get_mut`,不是格子,是另一条机制 |
| ws1018 | **`m[k]?.mutate(..)` 的位置是 `get_mut` 交回来的那个 `Option`,不是它里面的值**——ws984 那一对的另一半。`_heldSlot` 只认**空断言**那一形:`m[k]!.add(v)` 的位置是个*值*,`get_mut(&k).unwrap()`,后面再接 `.as_mut()`;**空感知**那一形要保住「不存在」这件事,所以位置**就是** `get_mut` 已经给出的 `Option<&mut V>`,再接 `.as_mut()` 就多了一层。类型不同,所以是另一个 helper 加另一种发射,不是把原来那条放宽。**trace 一次定位**:`name=remove_value recv=IrCall(!map_get) recvType=List<RestorationBucket>? mutating=true held=null`——闸门本来就过,卡的是 `_heldSlot` 不认光杆 `!map_get`;同一份 trace 还显示全程序其他 `!map_get` 接收者**都是 `mutating=false`**,所以这条只碰这一处。**它不只是编不过,是个正等着发作的错答案**:`_childrenToAdd[key]?.remove(child)` 改的是列表的**拷贝**,bucket 自己那份里 child 还在——和 `_heldSlot` 注释里 `RenderTapRegionSurface` 每个 group 都空掉是同一类 | stub **64 → 63**(掉的正是 `services_restoration._remove_child_data`,新增 **0**);拒绝 22、可达 69、0 error;run1018 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 54 个,53 AGREE + 1 个故意的红。**ws984 撤回两次的那一对,到这里两个都掉了** |
| ws1021 | **`Option<T>` 的 `==` 问的是里面那个值的 `PartialEq`,永远走不到 `DartEq`**。`DynamicLibrary` 早就在 `dart_eq_identity!` 里(有 `DartEq`,按同一性),缺的是 `PartialEq` 的 derive;`_Win32PlatformInterface` 拿 `DynamicLibrary?` 存库、开库前先问 `_library == null`,那句就编不过。**差点补出一个重复 impl**:我顺手也往 `dart_eq!` 里加了一个,而它已经在 `dart_eq_identity!` 里——两个宏都发 `DartEq`,加上去就是重复实现;发现是因为先去查了它到底有没有 `DartEq` | stub **63 → 62**(掉的正是 `widgets_window_win32.rs eq`,新增 **0**);拒绝 22、可达 69、0 error;run1021 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 55 个,54 AGREE + 1 个故意的红。**这一条是把 63 个桩逐个分类之后挑出来的**——分类结论见〈桩尾还剩什么〉:尾巴**不再按成因聚集**了,最集中的一个文件(`widgets_window_win32.rs`,4 个)是**四种不同成因** |
| ws1022 | **闭包体里的局部变量,可变性由**它自己的**体来定,和方法体一样**。`_closure` 本来就算了 `_assignedIn(node.body)`,但只花在**形参**和**捕获**上,body 里 `let` 出来的局部还是拿**外层成员**留下的那个集合判——`Widget dialog = themes?.wrap(..) ?? pageChild;` 后面跟 `dialog = SafeArea(child: dialog)`(`_DialogRoute` 的 `pageBuilder` 闭包)于是发成 `let dialog`,E0384。改法:进 body 前 `_reassigned = {..._reassigned, ...assigned}`,出来还原;**并集不是替换**,因为捕获进来的局部是外面定的。**又一次是 trace 赢了推理**:我已经说服自己「`_WalkSelf` 明明会进闭包、构造器初始化式也走了,所以 `dialog` 一定在集合里」,一行 trace 打出来是 `in=false set=1`——外层集合总共就 1 个元素 | stub **62 → 61**(掉的正是 `material_dialog.rs new`,新增 **0**);拒绝 22、可达 69、0 error;run1022 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 55 个,54 AGREE + 1 个故意的红。**这一条夹具的分量比平时重**:夹具 crate 里 `unused_mut` 是 deny 的,集合放宽过头会在全扫里直接红,而不是悄悄带过 |
| ws1026 | **写一个「静态量里装着 trait 手柄」的字段,是调 setter;而且只有*可变*静态量才有格子可借**。`dyn Trait` 上没有字段,只有它声明的那一对存取器。`_ContextMenuRoute._rectTweenReverse`(`RectTween?`)和 `_sheetScaleTween`(`Tween<double>`)的 `..end = x` 都发成了字段写,报 "method, not a field"。判据要**提到两条分支上面**:同一个函数里两处写分别走「counted 拥有者」和「其他」两条,各自都假设静态量里装的是结构体。**三趟链子,每一趟错误都往前挪了一格**(字段 → 调 setter → 借错格子 → 编过),不是原地不动——这正是和 tear-off 那次(三轮全是逐字节相同)该分开对待的地方。**三个自己的错都是量出来的、不是想出来的**:(1)类的静态量挂在**拥有者**上,不在库的顶层常量表里(`places.dart` 里本来就有写对的写法);(2)只修了一条分支,同一个函数里还有第二处走另一条;(3)不可变静态量根本没有 `RefCell`,`borrow_mut()` 是多余的 | stub **61 → 60**(掉的正是 `cupertino_context_menu.rs _update_tween_rects`,新增 **0**);拒绝 22、可达 69、0 error;run1026 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 55 个,54 AGREE + 1 个故意的红 |
| ws1027 | **字段*写*的接收者,也是一次成员访问的接收者**——擦除过的读要窄化到 Dart 给的类型(`_receiver`),而赋值那条路用的是裸的 `expression(receiver)`(三处)。`InheritedNotifier<T extends Listenable>` 把 `notifier` 声明成 `T`,`_InheritedResetNotifier extends InheritedNotifier<_ResetNotifier>` 里 `inheritedNotifier.notifier!._wasCalled = false` 于是看到的是 `Rc<dyn Listenable>`,上面没有这个字段。**同一个 Dart 函数里紧挨着的两行,读的那行早就是对的**(`final wasCalled = inheritedNotifier.notifier!._wasCalled;` 编得过),写的那行不是。**trace 又一次推翻了我的判断**:我认定是读被擦过头了,打出来 `targetRust=_ResetNotifier`——读没问题。**ws1019 那次撤回在这里回本了**:它是因为量不出来被撤的,不是因为诊断错;「擦除过的读进成员访问要窄化」这条结论留下来了,这次一眼就认出同一个形状 | stub **60 → 59**(掉的正是 `widgets_draggable_scrollable_sheet.rs should_reset`,新增 **0**);拒绝 22、可达 69、0 error;run1027 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 55 个,54 AGREE + 1 个故意的红 |
| ws1028 | **局部函数省略掉的可选实参,要按声明填默认值——和方法调用一样**。局部调用那条路按 **invocation** 的函数类型取实参,而没传的那个参数在 invocation 类型里根本不存在。`void takeMeasurementsInSourceRoute([Duration? _])` 被写成 `takeMeasurementsInSourceRoute()`,发出来就是拿 0 个实参去调一个 1 参闭包(`_OpenContainerRoute._takeMeasurements`)。`_instanceInvocation` 早就为方法长过同一条(它注释里写着 `weigh()` 那次),这里照抄。**只在声明的位置参数比调用给的多时才去读声明**——无条件读会把已经传进来的实参丢掉,夹具里两种调用都写了,正是钉这一点 | stub **59 → 58**;拒绝 22、可达 69、0 error;run1028 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 59 个,58 AGREE + 1 个故意的红。`localoptional` **改之前是红的,3 个 E0057**,形状和 gallery 一样。**两处都修好了,但只掉了一个**:`material_list_tile.rs build` 整个掉了;诊断时看的那个 `open_container._take_measurements` 的 arity 错没了,露出下一层——`hideableKey.currentState!.placeholderSize = ..` 里 `set_placeholder_size` 落在 `Rc<dyn State>` 上,**正是 ws1027 那个「擦除的读要窄化」家族**,只不过这次在 getter 读上 |
| ws1029 | **setter *调用*的接收者也要窄化**——和 ws1027 的字段写是同一句话,只是另一条构造路径(`expression(value.receiver)` → `_receiver(value.receiver)`)。`GlobalKey<T extends State>.currentState` 声明成 `T?`,`hideableKey.currentState!.placeholderSize = ..` 于是看到声明的界 `Rc<dyn State>`,上面没有那个 setter。**是 ws1028 顺出来的线索**:那一轮把 arity 修好之后,`_takeMeasurements` 露出的下一层正是这个 | stub **58 → 56**(掉了两个:`open_container._take_measurements`、`material_time_picker.did_change_dependencies`,新增 **0**)——**本轮单次改动掉得最多的一次**;拒绝 22、可达 69、0 error;run1029 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 59 个,58 AGREE + 1 个故意的红 |
| ws1031 | **`try` 挡住的是抛出,不是调用**。两条传染都顺着「对 `this` 的调用」走:调了要句柄的方法,自己也要句柄(`_computeHandles`);调了写字段的方法,自己也要 `&mut self`(`_mutating`)。两边读的都是 `selfCalls`,而它只在 `try` **外面**才记——那个 `_caught` 闸门是给第三个问题用的:**失败会不会逃出去**,`catch` 确实挡得住。两个不相干的问题共用了一个闸门。`EditableTextState._pasteTextWithReporting` 是 `try { await pasteText(cause); } catch ..`,而 `pasteText` 收句柄:这次调用没被记下,调用方保持 `&self`,`self.paste_text(..)` 就找不到方法。**定点本来就是对的、也是传递的,空的是喂给它的输入**——trace 打出来 `paste_text=true withReporting=false calls={}` | stub **56 → 55**(掉的正是 `_paste_text_with_reporting__body`,新增 **0**)。**这条同时放宽了全程序的两条传染集,而新增桩是 0**——这比什么都更能说明那个闸门只是接错了地方,不是在承重;拒绝 22、可达 69、0 error;run1031 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 60 个,59 AGREE + 1 个故意的红。`handleintry` **改之前是红的**,形状一样(`no method named spawn found for reference &Box`)|
| ws1032 | **`Pointer<T>` 是个地址,所以它是 `Copy`**。`Clone` 本来就是手写的——**手写的理由正是 derive 会加一条指针不需要的 `T: Clone` 界**——但只写了一半,没有 `Copy`。`_onMessage` 在循环里把同一个 `message` 交给每个 handler,第二圈就是「use of moved value」。`PhantomData<T>` 对任何 `T` 都是 `Copy`,Dart 的 `Pointer` 本来就是值语义,所以两半是同一个理由,现在都写出来并把理由记在旁边 | stub **55 → 54**(掉的正是 `widgets_window_win32.rs _on_message`,新增 **0**);拒绝 22、可达 69、0 error;run1032 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 60 个,59 AGREE + 1 个故意的红 |
| ws1037 | **`List.of(x)` 里 `x` 是「翻译出来的、自己就是 `Iterable`」的类时,走它的 `__to_list`,不是 Dart 的 `toList`**。这种类本来就发一个 `__to_list()`(`_emitToList`,Dart 里没这个名字),正是为这种场合准备的;它自己的 `toList` 是另一回事——`Iterable.toList({bool growable = true})` 可以被覆盖,而**具名参数在 Rust 里变成位置参数**,所以 `ObserverList.toList({bool growable = true})` 是 `to_list(&self, growable: bool)`,`to_list()` 就是拿 0 个实参去调 1 参方法。**上一轮撤回时写下的下一步(「先打印那个调用点走哪条发射路径,别按 `to_list` 这名字找」)一次命中**:Dart 根本不是 `_listeners.toList()`,是 `List<ValueChanged<..>>.of(_listeners)`,落在 `nullaware.dart` 的 `List.from/of` 分支——之前那次 trace「接收者全是 List/Set」不是死路,是在指路 | stub **54 → 53**(掉的正是 `widgets_focus_manager.rs notify_listeners`,新增 **0**);拒绝 22、可达 69、0 error;run1037 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 61 个,60 AGREE + 1 个故意的红。`listoflistable` **改之前是红的**,首条错和 gallery 一样(`E0061: this method takes 1 argument but 0 arguments were supplied`);夹具里**特意写了覆盖版的 `toList`**——不写的话那个类只有生成的 `__to_list`,这个 bug 冒不出来 |
| ws1039 | **Dart 的 `Pattern` 是 `String` 或 `RegExp`,Rust 没有这个并集,所以一个成员在这边有两个实现**。`String.replaceAll(Pattern, String)` 里 prelude 的 `DartString::replace_all` 只收 `String`,`_DateFormatQuotedField._patchQuotes` 传的是 `RegExp`,发出来就是 `s.replace_all(re, "'")`——expected `String`, found `RegExp`。**分派的依据是实参的静态类型,而静态类型只有前端知道**:第一版写在 backend 的 `_call` 里按 `args[0].rustType?.name == 'RegExp'` 认,跑出来**逐字节相同**——那个位置上实参是个静态字段读,身上根本没有 Dart 类型。改到 `_instanceInvocation`:`regexpPatternMember` 一张表(`replaceAll` → `replace_all_in`),接收者和第一个实参**对调**,调用直接落在 `RegExp` 自己的方法上,后端一行都不用改。prelude 里的 `replace_all_in` 写在 `all_matches` 上头,**两半因此对「什么算一次匹配」的看法一致**;索引按 UTF-16 code unit,和这边其它地方一样 | stub **53 → 52**(掉的正是 `intl_date_format.rs _patch_quotes`,新增 **0**);拒绝 22、可达 69、0 error;run1039 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 62 个,61 AGREE + 1 个故意的红。`regexpattern` 覆盖重叠候选、两端各一次匹配、替换串更长、完全不匹配、空串、整串都是匹配,外加一条字符串 pattern 走原路——先红(`E0308: expected String, found RegExp`)后绿 |
| ws1040 | **句柄进「类型参数」的槽,先上到 `Object` 再由它自己的 `FromDynamic` 接回来**——这正是它下面那条规则(类型参数的值进 trait 槽走 `dart_cast_to`)的镜像,原来只认**已经是 `Object`** 的值。**被擦成界的泛型类,再从当初许诺更多的那个参数读回去**,就是这个形状:`_InheritedModel<T extends Model>` 里 `final T model` 发出来是 `model: Rc<dyn Model>`,`ScopedModel.of<T>` 的 `return (widget as _InheritedModel<T>).model;` 于是把 `Rc<dyn Model>` 交给一个 `T`;`LayoutInfoType get layoutInfo => constraints as LayoutInfoType`(那个 `as` 落到了唯一的实例化上)一模一样。接回来的是**同一个对象**,只是窄到调用方要的类型,失败就按 Dart 的隐式向下转型失败 | stub **52 → 50**(掉的正是 `scoped_model.rs of` 和 `widgets_layout_builder.rs ..._super_layout_info`,新增 **0**);拒绝 22、可达 69、0 error;run1040 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 63 个,62 AGREE + 1 个故意的红。`typaramback` **改之前是红的,两个 E0308 和 gallery 逐字一样**(`expected type parameter `T`, found `Rc<dyn Model>`),夹具里两个模型是**不同的类、答案也不同**,并且各调一个**界上没有的**成员,所以「拿回界」或「拿错对象」都会在输出里露出来 |
| ws1041 | **枚举也有自己的静态成员,变体带的状态也不一定是字面量**——一处形状的两半,都出自 `KeyboardLockMode`。(一)Dart 枚举可以在变体旁边声明 `static`,而前端把枚举声明的字段**一律丢掉**(注释写着「枚举自己的成员就是变体和 CFE 的簿记」),读的那一侧还把它**当成变体来拼**——`KeyboardLockMode::_knownLockModes`,一个 Rust 枚举从来没有过的名字。变体是「类型就是这个枚举本身的 static const 字段」(`_isVariantOf` 早就写着),除此之外声明的静态就是普通类的常量,照普通类发。(二)变体带的状态只认四种字面量,`numLock._(LogicalKeyboardKey.numLock)` 带的是个对象,于是**整个变体的状态被丢掉**、getter 一个都不发,读状态的成员各挂一个桩(ws939 记的账)。改成把**常量本身**带出来,到有前端的地方再用 `_constant` 降——它对 `LogicalKeyboardKey.numLock` 的答案就是别处一样的 `LogicalKeyboardKey::new(..)`;降不出来的仍旧退回「状态没恢复」,一个不多。**两半必须一起做**:静态的初值读的正是变体带的状态,只做第一半时桩从函数挪到了静态上,数目一个没少 | stub **50 → 49**(掉的正是 `services_hardware_keyboard.rs find_lock_by_logical_key`,新增 **0**;只做第一半时是 gone 1 / new 1);拒绝 22、可达 69、0 error;run1041 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 64 个,63 AGREE + 1 个故意的红。`enumstatic` **改之前是红的,两种形状都在**:`_byId` 被当成变体(E0599),以及 `key`/`bit`/`mark` 三个 getter 全没发——夹具里特意让一个变体字段带对象、另两个带字面量,正是钉「一个不是字面量就全丢」这一条 |
| ws1044 | **覆盖可以给可选具名参数**加**一个**,而 mixin 的方法体仍按 mixin 声明的参数去调**。Dart 允许:`SemanticsNode.toDiagnosticsNode` 在 `DiagnosticableTree` 的 `name`/`style` 之外多一个带默认值的 `childOrder`,而 `DiagnosticableTreeMixin.toString` 里写的是 `toDiagnosticsNode(style: ..)`。mixin 的体是**发进类里**的(`impl SemanticsNode`),于是 `self.to_diagnostics_node(name, style)` 落在类自己那个三参方法上,少一个实参(E0061)。**先在前端试了一版,逐字节相同**:`DART2RUST_TRACE_LANDING` 打出来是 `interface=DiagnosticableTreeMixin(name,style) landed=同一个`——那段体是在 **mixin 的语境**里降的,应用它的类在它**下面**,前端根本看不见。后端看得见:发这段体的时候它知道 `impl` 是谁的,也知道两边的元数。规则落在 `calls.dart` 的限定符那一段(**按个数**判断,元数一致的名字照旧走固有方法),而 trait 那一侧的转发 impl **本来就把默认值填好了** | stub **49 → 48**(掉的正是 `semantics_semantics.rs to_string`,新增 **0**);拒绝 22、可达 69、0 error;run1044 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 65 个,64 AGREE + 1 个故意的红。`widerarity` **改之前是红的,和 gallery 同一句话**(`this method takes 3 arguments but 2 arguments were supplied`);夹具钉的不只是「能编」:Dart 会**分派到覆盖**,所以 `node.render()` 必须是 `node/r/plain/3` 而不是 mixin 自己的答案,另写一个**不覆盖**的类,确认那一路仍然落在 mixin 的体上 |
| ws1045 | **再声明一次同名局部,它的读写就是它自己的**——闭包写的局部在这边住进 cell,而 `_cellLocals` 是**按名字**记的,于是同名的**新**声明之后,读还在发 `action.borrow()`。`Actions.maybeFind` 结尾是 `if (action case final Action<T>? action) { return action; }`,CFE 把这个**模式绑定**提到外层作用域,发出来就是 cell 那个 `let` 之后再来一个普通的 `let mut action`——**Rust 认后面那个**,`Option<T>` 上没有 `borrow`(E0599)。做法:普通声明**注销**同名的 cell 条目——让这张表和 Rust 实际解析的结果一致;同时给带花括号的体加了作用域保存/恢复(`_scoped`),分支里的遮蔽随分支结束(`IrBlock` 不发花括号,Dart 的块作用域本来就被摊平进外层,所以它不算) | stub **48 → 47**(掉的正是 `widgets_actions.rs maybe_find`,新增 **0**);拒绝 22、可达 69、0 error;run1045 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 66 个,65 AGREE + 1 个故意的红。`shadowedcell` **第一版是绿的、复现不了**——我用了两个**不同名**的局部,而 Dart 同一作用域里根本不许重名;改成 gallery 的真形状(模式绑定)之后**改前 8 个错、首条和 gallery 逐字一样**(`no method named `borrow` found for enum `Option<T>`)。夹具最后一格是第二个闭包再写一次 cell:遮蔽没有把它变成普通局部 |
| ws1046 | **tear-off 的接收者也要窄化,而绑定了接收者的 tear-off 是个「块」**——一处两层。`RestorableChangeNotifier<T extends ChangeNotifier>` 继承 `RestorableListenable<T extends Listenable>`,字段是**基类的**,从擦除过的 trait 读回来就是基类的界:`scheduleMicrotask(_value!.dispose)` 的接收者是 `Rc<dyn Listenable>`,上面没有 `dispose`。第一层照 ws1027/ws1029 办(tear-off 的接收者走 `_receiver`),**错误随即往前挪了一格**——`dart_cast_to::<dyn ChangeNotifier>()` 出来了,`dispose` 找得到了,露出第二层:闭包**没进 `Rc`**(expected `Rc<dyn Fn()>`, found closure)。**这里我连猜两次都是逐字节相同**(先猜 `coerce` 的块规则,再猜后端的 owned 位置),第三次用 `DART2RUST_TRACE_SLOT` 打出来才看清:`param=void Function() lowered=IrBlockValue`——`scheduleMicrotask` 是 **prelude** 被调方,`_widenedInto` 的 `translated` 闸门根本不放行,所以那一路压根不做强制转换;装箱是 `_withBorrowing`(「被调方留着这个参数,就装箱」)干的,而它只认**光秃秃的闭包**。绑定了接收者的 tear-off 发出来是 `IrBlockValue(let __t1 = ..; 闭包)`,于是漏掉——同一个 tear-off 只要根在 `this`(不用绑),一直都是装了箱的,这正是**只有这一种形状错**的原因 | stub **47 → 46**(掉的正是 `widgets_restoration_properties.rs restorable_change_notifier_super__dispose_old_value`,新增 **0**);拒绝 22、可达 69、0 error;run1046 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 67 个,66 AGREE + 1 个故意的红。`tearoffbound` 第一版**没复现**(泛型没被擦),按 gallery 的真形状重写——**基类的界更宽、字段在基类**——之后改前是红的,错误和 gallery 同形(`no method named bump found for struct Rc<dyn Listen>`);夹具连**效果**一起钉:撕下来的方法必须落在接收者指的那个对象上,存起来的闭包过后还能再调 |
| ws1047 | **prelude 里「只调一次就丢」的回调槽,静态方法上也要借,不能给句柄**。那种槽是 `impl Fn`,而 `Rc<dyn Fn>` 不是——这条借用规则本来只给「接收者属于 prelude」的实例调用,静态调用那条路(`_staticCall`)没有。`Timeline.timeSync(label, () {..})` 于是收到 `Rc::new(闭包)`。**要紧的不是拼法**:`Rc<dyn Fn>` 必须 `'static`,而读 `&self` 的闭包做不到——`_TaskEntry.run` 报的正是「lifetime may not live long enough」。两半:prelude 的 `time_sync` 改成 `impl Fn`(它确实只调一次就丢),并进 `_preludeLends`;后端在**prelude 类的静态调用**上也做同样的借。**只改 prelude 那一半时错误换了一句话**(expected an `Fn()` closure, found `Rc<{closure}>`),数目没动——调用点还在装箱,这正好指出漏的是哪一条路 | stub **46 → 45**(掉的正是 `scheduler_binding.rs run`,新增 **0**);拒绝 22、可达 69、0 error;run1047 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 68 个,67 AGREE + 1 个故意的红。`lentstatic` **改之前不是红的,夹具头里写明了**:借还是拷贝取决于 `_keeps`,而它读的是**被调方的函数体**——gallery 的 AOT dill 带着 `Timeline.timeSync` 的体,夹具的最小 dill 不带,所以夹具里一直是装箱(且能编)。它钉的是改过的 prelude 方法的**行为**:交给借用槽的闭包,跨多次调用仍然看见它写的那个对象 |
| ws1048 | **界是 prelude 的值类型时,`T` 上什么成员都没有**——Rust 拼不出 `T: DateTime`(它是个结构体,不是 trait),于是 `CalendarDelegate<T extends DateTime>` 里 `dateA.year` 在 `T` 上找不到 `year`。prelude 的值类型个个都有 `FromDynamic`,所以可以**问对象要 Dart 许诺的那个界**:上到 `Object` 再接回来。**四层,一层一层被上一层露出来**:(一)`_receiver` 原来遇到「不是翻译过的抽象类」的界就停(`DART2RUST_TRACE_NARROW` 打出来是 `declined T bound=DateTime translated=false abstractLike=false`,这条 trace 是先跑的,不是先改);(二)coerce 里加「类型参数 → prelude 值类型」;(三)null-aware 绑定是**引用**,`DartAny` 实现在值上,装箱要先 clone;(四)装箱**取走**值,所以局部/形参的**读**一律 clone(`show(T a)` 里 `a` 被第一次装箱移走,后面两次就用不了了)。**第二层第一版把翻译过的结构体和枚举也算上,加了 2 个桩**:`SlottedContainerRenderObjectMixin<SlotType>` 的 `Map<SlotType, ..>` 键槽拼的是唯一那次实例化(`_ChipSlot`),把 `SlotType` 转成它就把实参交错了类型——收窄成只认 prelude 值类型 | stub **45 → 42**(掉的是 `material_date.rs` 的 `is_same_day`、`is_same_month`、`op_eq`,新增 **0**);拒绝 22、可达 69、0 error;run1048 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 69 个,68 AGREE + 1 个故意的红。`valuebound` **改之前是红的,13 个错、首条和 gallery 逐字一样**(`no method named year found for reference &T`);**第四层是夹具自己找出来的**——gallery 那三个没有那个形状;夹具里几个日期在每一格上各差一处,读错哪一格都会露出来 |
| ws1049 | **调用的类型实参,要和它的值拼成同一个样子**。可空的类型参数过调用这条边是**投影**过的(`<T as DartNullable>::Or`,不是 `Option<T>`),所以指着它的类型实参也得这么拼,否则 turbofish 和值对不上:`entry.complete::<Option<T>>(<T as DartNullable>::from_option(result))`(`NavigatorState.removeRoute`,被调方是 `complete<T>(T result)`)。**先跑的是 trace**:`DART2RUST_TRACE_SLOT` 打出 `param=TypeParameterType(..T?) slotIr=T? lowered=IrLocal have=T?`——IR 里两边**本来就一致**,差的只是拼法,这就把问题定在拼法上而不是类型上。**第一版无条件改,净增 0**:`Localizations.of` 多一个桩——`resourcesFor<T>` 那种「类型参数只决定返回」的,投影拼法会让嵌套多一层(`Option<Or>` 不是 `Option<Option<T>>`)。收窄成**只在两种拼法真的碰面时**:被调方有个形参的声明类型**就是**那个类型参数 | stub **42 → 41**(掉的正是 `widgets_navigator.rs remove_route`,新增 **0**);拒绝 22、可达 69、0 error;run1049 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 70 个,69 AGREE + 1 个故意的红。`edgeturbofish` **改之前是红的,和 gallery 逐字一样**;写它试了三版:前两版的第二半各自撞进**别的洞**(方法自己的类型参数上做 `as V`、`List<V?>` 的元素投影),第三版发现**调用方自己得是泛型**——非泛型函数里的 `T?` 局部本来就是 `Option<T>`,根本不问这个问题 |
| ws1050 | **局部的声明类型是类型参数时,提升给的是「交集」不是类型**。`if (v is double)` 落在 `T v` 上,kernel 记的是 `T & double`(`IntersectionType`);而读那一侧的分支是按 `InterfaceType` / `TypeParameterType` / `FunctionType` 写的,**交集一个都不匹配**,于是读原样发成一个 `T`——`debugFormatDouble(v)` 收到类型参数而声明要 `f64`。提升说它**现在是什么**,就在交集的右边;拆开之后,底下那些分支本来就会做。**又是 trace 先跑**:`DART2RUST_TRACE_PROMOTED` 打出 `v declared=TypeParameterType(IterableProperty.T%) promoted=IntersectionType((IterableProperty.T% & double))`——一行就指出漏的是哪一类节点 | stub **41 → 39**(掉的是 `foundation_diagnostics.rs` 的 `value_to_string` 和 `diagnostics_property_super_value_to_string`,新增 **0**);拒绝 22、可达 69、0 error;run1050 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 71 个,70 AGREE + 1 个故意的红。`promotedtyparam` **改之前是红的,4 个错**,每个分支一个,首条和 gallery 同形(`no method named .. found for type parameter T`);夹具里每条分支都按**不同的类型**去读那个值(标量两种、翻译过的类、它的抽象基类),读错哪一边都会在输出里露出来 |
| ws1051 | **两个参数个数不同的函数值,永远不是同一个对象**——Dart 里没有哪个函数值同时有两种元数,所以 `==` 是假、`!=` 是真,压根没有可比的东西;而 `dart_eq` 收 `&Self`,两边是不同的类型(E0308)。gallery 走到这里是因为**上游 Flutter 自己的复制粘贴**:`_TimePickerModel.updateShouldNotifyDependent` 里写的是`onHourMinuteModeChanged != oldWidget.onHourDoubleTapped`,底下两行左边读的都是同一个字段——`packages/flutter/lib/src/material/time_picker.dart` 里就是这么写的,kernel 也是这么带的(`DART2RUST_TRACE_FIELDREAD` 打出来 `wrote=onHourMinuteModeChanged receiver=ThisExpression` 配 `wrote=onHourDoubleTapped receiver=VariableGet`)。**我一开始把它当成翻译错了**,是 trace 和源码把它澄清的:翻译是忠实的。**规则先写在 `operators.dart` 里,逐字节没变**——这处相等是从**调用**那条路发的(`!dart_eq`),不是运算符那条 | stub **39 → 38**(掉的正是 `material_time_picker.rs update_should_notify_dependent`,新增 **0**);拒绝 22、可达 69、0 error;run1051 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 72 个,71 AGREE + 1 个故意的红。`arityeq` **改之前是红的,3 个错,和 gallery 逐字一样**;夹具**不假设答案,去问 Dart**:跨元数两个方向、同元数的 tear-off(相等)和两个不同的闭包(不等)都写上,Dart 给的正是 `true/false/true/true/false/true` |
| ws1053 | **`dart:io` 的两个异常,prelude 里没有**——于是 `is HttpException` 被拒,**整个 `IOClient.send` 跟着被拒**。`SocketException` 本来就是个结构体,只是从没为 `is` 注册过;`HttpException` 是新加的,带 `IOClient.send` 读的那两个成员。**这是本次会话第一条真正掉下来的拒绝**(22 → 21),它的代价要写清楚:`send` 现在能翻译了,**它的函数体因此变成一个桩**(38 → 39)。换句话说:**拒绝换成了桩**——函数从「根本不存在」变成「存在但会 panic,而且被桩尺子量着」。一路上错误挪了四次,每次都是一个签名细节:`status_code` 没有 → `HttpClientResponse` 是个空占位;`handle_error` 没有 → 它在 Dart 里**是**个 `Stream<List<int>>`;`onError` 类型不对 → Dart 的 `handleError` 收的是**裸 `Function`**(所以到手的是函数对象,和 `DartFuture::then` 一样);最后停在**函数对象适配器**里的 `Null`/`Infallible`——那是另一个子系统,留给它自己的一轮。**中途还自己砍了一次可达 crate:0**:给 `HttpClientResponse` 加的 `#[derive(Default)]` 要求 `Stream<T>: Default`,prelude 编不过,整个工作区跟着掉——`Stream` 的 `Clone`/`Debug` 本来就是手写的(避开 `T:` 界),`Default` 也得手写 | 拒绝 **22 → 21**;stub **38 → 39**(新增的正是 `io_client.rs send__body`);可达 69、0 error;run1053 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 73 个,72 AGREE + 1 个故意的红。`ioexceptions` **改之前是红的**(`cannot find type HttpException`,3 个);**夹具还自己抓到一件事**:`HttpException` 只进了 `is` 的表、没进对象协议那张,`dart_boxed` 立刻不满足——两张表都要进 |
| ws1054 | **`runZonedGuarded` 不在 prelude 里,`dart:ui` 的 `_invoke1WithReturn` 就整个被拒**——平台回调是在它下面跑的,所以引擎回调里抛出的错误本该走 zone 的未捕获钩子。这里只有一个 zone,所以「zone」那一半什么都不做(`Zone::run` 本来就是直接调),**剩下的一半是真的**:body 正常返回就交出值,抛了就调 `onError` 再交出 `null`。顺手补上 `Zone::handleUncaughtError`(根 zone 之上没有处理者,所以它就是 prelude 到处都在说的那句「抛了没人接」)。编译器教了两件事,两件都写进了代码:**具名参数是按位置传的**(调用点是四个实参、两个 `None`),以及**顶层调用不会自动加 `?`**,除非名字进了 `_preludeFailingStatics`——`_preludeFunctions` 只管「这个名字算不算翻译过」。同一轮还加了 `_nativeEffect`:它是空操作**不是偷懒,是规格**——SDK 自己写着「调用和它的实参在流图构建时被删掉」。它**没有**让 `_CallocAllocator.new` 的拒绝消失,而是把拒绝**挪到了 `_ffiCall`**,也就是真正的 DLL 调用——`fx/ffistruct.dart` 早就把那条线画在那里了 | 拒绝 **21 → 20**(掉的是 `dart_ui.rs` 的 `_invoke1WithReturn`,而且**没变成桩**);stub **39 → 39**,集合逐条相同(gone 0 / new 0);可达 69、0 error;run1054 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 74 个,73 AGREE + `ffistruct` 那一个故意的红。`zoneguarded` 与 Dart 同字节:`value=42|caught=Marker(boom)|thrown=null|ran|built=ok`。**记下一条边界而不是绕过它**:`R` 必须能从 body 闭包的返回类型**正着**推出来——rustc 不会把 `<R as DartNullable>::Or` 倒着解,而只会抛出的 Dart body 根本没有返回类型;gallery 里每个调用点都有返回路径,所以这条线写在 prelude 的注释里。`_nativeEffect` **没有夹具、也不可能有**:它是 `dart:_internal` 的私有外部函数,没有 Dart 程序能调它 |
| ws1055 | **`dynamic` 上的成员访问,现在按「谁声明了这个名字」来分派**。`_dynamicSlotCall` 回答的是「值来自哪个槽、槽里能装什么」;这一条回答另一半——`demo.slug` 里没有任何东西知道 `demo` 是什么(`LinkedHashMap.fromIterable` 的闭包参数就写着 `dynamic`),但**封闭世界知道谁声明了 `slug`**,而那份名单很短。构造是同一个 `IrDynamicDispatch`,只是候选人从「槽的普查」换成了「成员名的普查」(`dynamicMembersIn`,整个 gallery 只有 15 个名字)。每条臂都把值装箱成 `dynamic` 持有的那个 `Object`——十七个类声明 `toJson`,它们的 Dart 返回类型没有两个一样,唯一都同意的类型就是槽自己的类型。**兜底那一条是要做决定的地方**:`_dispatch` 原本 panic「一个 dynamic 槽装了没料到的类型」,那是关于**本编译器普查**的事实;而「没人声明这个成员」是关于**程序**的事实,Dart 在那里抛 `NoSuchMethodError`,而且 Dart 程序会接住它(`package:get` 就把 `toJson` 包在 `try` 里)——所以这条臂抛的是一个 `catch` 能拿住的值,不是 panic。`NoSuchMethodError` 按 ws1053 教的那四个地方进了 prelude,外加 `dart_error_text`。**中间错了一次,错法值得记**:第一版按 **Dart 的可空性**决定装箱,于是 `dynamic toJson()`——Dart 认为它可空,可它的 Rust 值本来就是那个对象——被套进 `dart_option_object`,`Rc<dyn DartAny>` 被要求当 `Option`,**赔了一个桩**(拒绝换成桩,20 → 18 / 39 → 40)。改成按**降级后的类型**决定,桩就回去了,这一格也进了夹具 | 拒绝 **20 → 18**(`Demos.asSlugToDemoMap` 和 `Rx.toJson` 都是干净掉的,没变成桩);stub **39 → 39**,集合逐条相同(gone 0 / new 0);可达 69、0 error;run1055 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 75 个,74 AGREE + `ffistruct` 那一个故意的红。`dynmember` 与 Dart 同字节:`a|b|null|x/A|page:x|{tag: p}|null|nosuch`——两个类都声明 `slug` 时选对臂、可空字段回来是 Dart 的 `null`、实参真的到了方法里、`dynamic` 返回值原样出去、两个名字都没声明的对象被当作 `NoSuchMethodError` **接住**。**没掉的那一个,普查直接说清了原因**:`GetQueue._check` 的 `completeError` 只有一个声明者,`dart:async` 的 `_Completer`——prelude 的类,本编译器不写它的结构体,所以没法 downcast 过去。让 prelude 的类也能当候选人要把类型实参拼出来,那是另一件事,不是这里能猜的 |
| ws1056 | **mixin 声明是空壳,而每一个问「这个字段是 final 还是共享」的普查都问错了类**。`ListNotifierMixin.addListener` 返回的闭包要从 `_updaters` 里删监听者;CFE 把 mixin 的方法体搬进了每个 application,**闭包跟着搬走了**,而声明里字段的位置上只剩一个抽象访问器——于是普查拿到的是 `Procedure`,不是 `Field`,答案变成「它根本不是字段」,闭包就被判为「捕获了 `this`」。**这一句是 trace 给的,不是猜的**:新加的 `DART2RUST_TRACE_CARRIED` 打出 `refused=_updaters(Procedure)`,一个词就说完了(以前「闭包捕获了 this」从不说是哪个字段逼的)。接着的三处都是修好前两处之后**编译器自己报出来的**:applied 字段从来没标过 `shared`,所以没有 cell 访问器;`_sharedField` 只看声明字段,于是闭包捕获了字段的**值**而不是它的 cell,回头写不进去(`cannot assign to _turns, as it is a captured variable in a Fn closure`——夹具从绿变红的那一刻)。**最后一道限制也是赔了一个桩换来的**:一并解析 `final` 字段会把 `WidgetsBinding.pipelineOwner` 喂给拷贝路径,它是 `late final`,而**它自己的初始化式就在造那些回头读它的回调**——那一刻既没有值可拷,也没有 `self` 可拷(`expected value, found module self`)。所以解析器只认**可变**字段:它存在的理由是找 cell,而 final 字段没有 cell。**中途还自己制造并修掉一次性能回归**:第一版从 `_closureCallsMethod` 里重走 mixin 的每个 application,而那个问题是**按表达式**问的,gallery 的翻译从三分钟涨到四十分钟以上——普查改在 `_closureFields`(按类问)里做,applied 闭包跨库缓存 | 拒绝 **18 → 17**(掉的是 `ListNotifierMixin.addListener`,没变成桩);stub **39 → 39**,集合逐条相同(gone 0 / new 0);可达 69、0 error;run1056 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 76 个,75 AGREE + `ffistruct` 那一个故意的红。`mixincellclosure` 与 Dart 同字节:`a1|a1,b2|a1,b2/2|a1,b2,c3|a1,b2,c3/3`——闭包写进去的,对象下一次读得到;后取的闭包看得见先前那次写:**一个字段,不是两个** |
| ws1057 | **一条规则,两个位置,两处都是编译器自己报出来的**。`a ?? b` 在 `dynamic` 上会走 `dart_nullable(a)`,而 `dart_nullable` 是**把句柄拿进去、再从 `Option` 里还回来**——所以第一个 `??` 就把裸局部量移走了;`_MasterDetailScaffold.build` 在一个表达式里写了两次 `value ?? ..`(一次给 key,一次给它建的页),于是成了桩。**同类问题就在隔壁**:`??` 的右手边落在 `match` 的 `None` 臂里,而**臂会把局部量移出去**——`_ifNull` 的**左边一直在 clone,注释也一直写着为什么**,右边却从来没有。两边现在一样。**夹具还顺手抓出第三个缺口,留了名字没有顺手补**:`dynamic ?? String` 编不过,因为 fallback 没有装箱成另一条臂给出的那个对象——那是另一件事,不该混进来 | stub **39 → 38**,掉的正是诊断点名的那一个(`material_about.rs build`),**没有新增**;拒绝 **17**(这一轮不动它);可达 69、0 error;run1057 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 77 个,76 AGREE + `ffistruct` 那一个故意的红。`dynamicdefaulted` 与 Dart 同字节:`here:here|gone:gone|7:7|h:hh:h` |
| ws1059 | **在 cell 里的字段按下标读,借用活得比 cell 还久**。可变字段放在 cell 里,所以 `_list[i]` 是「借出 cell、取下标、克隆元素」——**克隆才是那个值,借用不该比它活得长**;可是**块尾的临时量会被延长到整条语句结束**,而 cell 句柄本身是个作用域更短的局部量(闭包序言把它携带的每个字段各克隆一份),于是借用活过了被借的东西。`AppStateModel.subtotalCost`(Shrine)就是这个形状:一个 `fold`,闭包在**同一个表达式**里读`_availableProducts[id]` 和 `_productsInCart[id]`——**而挨着它的那个 map 读法一直是绑好的**(`{ let __r = ..borrow().get(&k).cloned(); __r }`),下标读法从来不是。现在两边一样。**夹具专门钉住一件事**:绑住借用**不能**把读变成别处取的快照——bump 之后、push 之后再读,都要看得见 | stub **38 → 37**,掉的正是诊断点名的那一个(`app_state_model.rs subtotal_cost`),**没有新增**;拒绝 **17**(这一轮不动它);可达 69、0 error;run1059 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 78 个,77 AGREE + `ffistruct` 那一个故意的红。`cellindexborrow` 与 Dart 同字节:`33|133|101,2,3,4|22` |
| ws1060 | **被复制的值,现在有身份了**——ws437 起搁置的那个决定,终于落地了一半。**令牌**:一个 `Rc<()>`,一个对象的所有克隆共用它,另起一个的对象没有它。这在本编译器里**不是新想法**:可变字段本来就放在克隆共用的 cell 里,理由一模一样——**克隆不是第二个 Dart 对象,是同一个对象再拿到一次**;令牌只是把这句话补给身份。代价是每个实例一个字。**全 gallery 只有 6 个类拿到令牌**。**范围是量出来的,不是想出来的**,三次都是编译器说的:①第一版 **stub 38 → 80**——`Cell::get` 要 `Copy`,而全 `bool` 字段的 `SemanticsFlags` 因为拿了令牌丢了 `Copy`;于是**把「本编译器当作纯值的类」排除掉**:`Copy` 恰恰是「按位复制出来的是个新值」这个承诺,和令牌说的正好相反。②收窄时先问错了问题——用 `scalarNames`,可那里面有 `String`,而 Rust 的 `String` 不是 `Copy`,**结果每个带字符串字段的类都丢了令牌,包括这条夹具自己的**;要问的是「可 `Copy`」:数字、`bool`、枚举。③剩下最后一个桩:`emit_struct` 和 `_isCopy` 对同一个问题给了**两个答案**,`SelectionPoint.hashCode` 于是按 move 读了一个结构体已经不许 move 的字段(E0507)——两处对齐即可。**顺带修正了一处次序**:令牌判断必须排在「两个局部量比栈地址」那条**前面**——那条答「不同」,对 `Zone`、对拷贝出来的 map 是对的,对两个其实是同一个对象的局部量是错的 | 拒绝 **17 → 16**(掉的是 `_CompositeRenderEditablePainter.shouldRepaint`,**没变成桩**);stub **37 → 37**,集合逐条相同(gone 0 / new 0);可达 69、0 error;run1060 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 79 个,78 AGREE + `ffistruct` 那一个故意的红。`valueidentity` 与 Dart 同字节:`true/false|same/equal/different|true/false/true`——注意 `a` 与 `twin` **逐字段相等**,所以结构化的哈希会把这三问全答成真;只有令牌能答对。**夹具本身写废了三次,每次都值得记**:放进列表字面量、通过别名写字段,都会让类变成 **counted**(它本来就有地址,测的不是这条);`const` 类则暴露了真正的边界。**剩下 7 条身份拒绝里,大部分卡在 const 那一半**:Dart 会把常量规范化,可局部的 `const Frozen(1)` 到后端是一次**构造调用**,每次新令牌就会说两个规范化常量是不同对象——那是它自己的一轮 |
| ws1061 | **`as` 失败是抛出,不是 panic**——`/tmp/work.md` 第 1 步(「任何 panic 都不算过」那条规矩的第一组)。Dart 的 `x as Foo` 在类型不对时抛 `TypeError`,而 `on TypeError catch` 是普通 Dart;本编译器把它降级成 `.unwrap()`,**那是 panic:程序丢了一条它本来还有的路,而且没有尺子看得见,因为进程直接没了**。现在是 `dart_cast_failed`,一个 `Err(TypeError)`,用 `?` 往外送——**四成之外的函数本来就带着错误通道**(量过:生成侧 419,452 个 `fn` 里 343,447 个返回 `Result`,81.9%),所以这不是签名雪崩,是就地换。没有通道的那些仍旧 `.unwrap()`,那是第六节的边界问题,不是这一组的。**计划说这一组是 3,068 处,实际远不止**:`as` 有**四个发射点**,`dart_cast_to`(trait)、`dart_cast_any`(类型参数与 counted 类)、`DartCoreAs`(prelude 异常)、以及最大的那个 `downcast_ref`(具体类)。**夹具当场抓到了这件事**:第一版只改了 `IrCastTo` 两条臂,`(a as Dog)` 照样 panic,因为具体类走的是 `IrDowncast` | **`.unwrap()` 29,439 → 19,680**(一轮掉了 9,759,三分之一);`dart_cast_to::<..>().unwrap()` **3,068 → 127**(剩下的是另外三个发射点,同一条规则,下一轮);`.unwrap_or*` 3,384 不动(它们是全函数,不 panic);`panic!` 合计 7,223 不动(`.unwrap()` 不是 `panic!` 字面量)。拒绝 **16**、stub **37**(集合逐条相同 gone 0 / new 0)、可达 69、0 error;run1061 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 80 个,79 AGREE + `ffistruct`。`castthrows` 与 Dart 同字节:`rex!|not a dog|fido!|caught|caught|alive`——**最后那个 `alive` 才是重点**:panic 之后没有「接着跑」这回事。**一处诚实的差**:helper 说得出要转成什么类型,说不出手上是什么类型——值到这一步已经在 `Option` 里了,为了说出来就得把操作数求值两次。Dart 的原话是`type 'X' is not a subtype of type 'Y' in type cast`,这里只说它知道的那一半 |
| ws1062 | **`x!` 落空是抛出,不是 panic**——`/tmp/work.md` 第 2 步(第 4 组 + 第 1 组,最大的一笔)。Dart 的空断言抛 `TypeError`(「Null check operator used on a null value」),`on TypeError catch` 是普通 Dart。**本编译器的消息一直是 Dart 原样的,行为不是**:`dart_null_check_failed` 直接 `panic!`,而局部量上的 `x!` 降级成 `Option::unwrap`。两条都让程序丢了它本来还有的路。现在 helper 返回 `DartError`,三条臂统一成 `.ok_or_else(dart_null_check_failed)` 再用 `?` 送出去;静态就是 null 的那条用 `None::<()>`——那里没有值可解、也没有类型可解(run696 记过这个坑) | **`.unwrap()` 19,680 → 7,331**;`x!` 实际是 **12,408 处**,**比计划里那个 8,281 的上界还多**(计划自己写着那是上界不是读数,只覆盖 `IrLocal` 那条臂)。两轮合计 **29,439 → 7,331,四分之三没了**。`.unwrap_or*` 3,384 不动;`panic!` 合计 7,223 不动。拒绝 **16**、stub **37**(集合逐条相同 gone 0 / new 0)、可达 69、0 error;run1062 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 81 个,80 AGREE + `ffistruct`。`nullassertthrows` 与 Dart 同字节:`got 5|null|field 7|field null|at0=1|at1=null|alive` |
| ws1063 | **`mixin class` 也是空壳的,可编译器问的是「它是怎么写的」而不是「它被应用过没有」**。`abstract mixin class WidgetsBindingObserver`(Dart 3 的 `mixin class`,既能被 extends 也能被 with)在 Kernel 里是个 `Class`,**`isMixinDeclaration == false`**——可 CFE 照样把声明掏空、把方法体各复制一份进每个应用类。于是 `_WidgetsAppState` 里的 `super.didChangeAppLifecycleState(state)` 被拒:「super call to `WidgetsBindingObserver.didChangeAppLifecycleState`, which was not translated」。**这一条是 trace 直接说出来的,不是猜的**:新的 `DART2RUST_TRACE_MIXINBODY=<名字>` 一行打出`mixinDecl=false abstract=true applications=16 carriers=<13 个>`——**方法体在那儿,躺在十三个应用类里,只是那个 flag 把门关上了**。改法是把问题换掉:`_appliedAnywhere(node)`(CFE 到底有没有把它当 mixin 应用过),**同一个错问题在四处**,全是 ws1056 的地盘:方法体查找、applied 字段、applied 方法、以及「访问器背后的字段」那个解析器 | 拒绝 **16 → 15**(掉的是 `_WidgetsAppState.didChangeAppLifecycleState`,**没变成桩**);stub **37 → 37**,集合逐条相同(gone 0 / new 0);可达 69、0 error;run1063 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 82 个,81 AGREE + `ffistruct`。`mixinclasssuper` 与 Dart 同字节:`base:A,base:B|base:c`——既盖了 override 里走 `super` 的那条,也盖了直接落到默认体的那条 |
| ws1064 | **常量必须留成常量**——身份令牌的另一半,ws1060 记着的那一半。前端拿到的常量**已经求过值**,它会把 `const Alignment(-1,-1)` **重建成 `Alignment::new(-1.0,-1.0)`**,因为「读起来像源码」(`IrConstInstance` 的注释一直这么写着,只在构造函数被摇掉时才退回结构体字面量)。**对带令牌的类,这个重建是错的**:构造函数会铸一个新令牌,于是 Dart 规范化成同一个对象的两个 `const X(1)` 会变成两个对象。改法是把重建关掉——常量走结构体字面量,`__identity: None`,而两个无令牌的值按字段比,**对规范化常量来说那就是 `identical` 的意思**。普查里那条「有 const 构造函数就排除」随之删掉,判据收进 `_carriesIdentityToken`,两处共用。**代价一处,和 ws1060 同一个形状**:`std::ops` 按值取操作数,而带令牌的类永远不是 `Copy`,`BorderRadius * other` 报「cannot move out of `*self`」——运算符体改发 `self.clone()` | 拒绝 **15 → 12**(本会话单轮最大的一跌:`_ScribbleCacheKey.compare`、`_IdentityThemeDataCacheKey operator ==`、`_IdentityThemeDataCacheKey.hashCode`,**都没变成桩**);stub **37 → 37**,集合逐条相同(gone 0 / new 0);可达 69、0 error;run1064 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 82 个,81 AGREE + `ffistruct`。`valueidentity` 六组与 Dart 同字节:`true/false|same/equal/different|true/false/true|true/false/false/false|true/false|x2x`。**夹具第三次写废也记下来**:`Frozen` 的 `String` 字段没人读,TFA 把它摇掉了,只剩一个 `int` 的 `Frozen` 就是本编译器眼里的纯值——按设计不该有令牌,于是又什么都没测到 |
| ws1066 | **ws1064 把翻译拖慢了四十多分钟,而我提交它的时候没量过时间**。ws1064 让带令牌的类的常量**不再重建成构造调用**、改走结构体字面量;对的答案,错的做法——那等于把每个常量的每个字段都摊开写,而 Flutter 的 `ThemeData` 常量大得吓人:**前端从几分钟涨到将近一小时**。三把尺子都量不到编译器自己的速度,我也没看,就提交了。改法:给带令牌的类的每个构造函数发一个 **`_const` 孪生**——调真构造函数,然后把 `__identity` 清成 `None`。常量还是一句紧凑的调用,身份还是对的,文本量回到原样。**中间漏了一处**:重定向构造函数在发射器里提前 `return`,孪生没发出来,而常量正调用它——`AutofillConfiguration::new_const` 不存在,赔了 2 个桩;两条路径都补上就好了 | **链条总时长 2m 0s**(ws1065c 那次同样的形状要四十多分钟);拒绝 **12**、stub **37**(集合逐条相同 gone 0 / new 0)、可达 69、0 error;run1066 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 82 个,81 AGREE + `ffistruct`。`valueidentity` 六组一字不差,走孪生和走字面量的输出完全相同 |
| ws1068 | **ws1067 撤回之后接着做,这次把补丁打在量出来的地方**。三条签名事实照旧(它们是对的,只是单独立不住尺子):① `listen` 的 `onError` 是这份 prelude 自己发明的类型,Dart 声明的是裸 `Function?`,按 arity 挑实参——**这条规则 prelude 里本来就有**(`dart_call_error_handler`),`handleError`/`catchError`/future 早就在用,**只有 `listen` 是例外**;② `onData`/`onDone` 写着 `-> ()`,而生成侧 **9,068 个 `dyn Fn` 槽无一例外带 `-> Result`**——给它们 `Result`、`listen` 往外送(进 `_preludeFailing`);③ `ByteConversionSink` 是 `Rc<dyn DartSink<Vec<i64>>>` 的**别名**,Rust 不许给 `Rc` 写 inherent `impl`,所以 `ByteConversionSink::new` 怎么拼都解析不到,构造函数在 `_ByteCallbackSink` 上。**关键的不同在第四步**:ws1067 里我把重定向加在 `nullaware.dart` 的两条静态调用路径上,两次都读回**一字不差**的报错——按〈stubdiff 比的是集合〉那条,那就是规则没生效。**而这恰恰把发射点框出来了**:两条路都排除,剩下唯一能拼出那串文本的是 `values.dart` 的 `_new`,它是从**类型**而不是 owner 字符串拼名字的。补丁打在那里,一次就中 | stub **37 → 36**,掉的正是诊断点名的那一个(`http_below/src/byte_stream.rs to_bytes`),**没有新增**;拒绝 **12**、可达 69、0 error;链条 **2m 2s**;run1068 连采五次:707 行 / 类型差异 0 / 0 panic;夹具 82 个,81 AGREE + `ffistruct` |
| ws1069 | **撤回。给 prelude 接口表加 `Sink` 是对的一半,但它同时打开了第二处发射,第二处是墙**。`DigestSink implements Sink<Digest>` 没有 `impl DartSink<Digest>`,`hash.rs` 的 `hash_super_convert` 因此是桩;把 `Sink` 写进 `_preludeInterfaces` 正好补上这个 impl。可 `the_class.dart:369` 的**超 trait 列表**读的是同一张表,而且照接口的**名字**拼——`Sink` 在这份 prelude 里是**类型别名** `Rc<dyn DartSink<T>>`,于是 `pub trait HashSink: .. + Sink<Vec<i64>>` 是 E0404「expected trait, found type alias」,**`crypto_below` 连同四个下游 crate 一起掉了**。按 impl 那边的口径给它改名(`_preludeInterfaceTraits`)之后,别名那个错没了,露出来的是真正的那堵墙:`impl HashSink for _Sha256Sink` 现在要求 `_Sha256Sink: DartSink<Vec<i64>>`,而 **Rust 的 impl 不继承**——`_preludeInterfacesOf` 只走*抽象*祖先,crypto 的 `HashSink` 是具体类,子类根本拿不到这个 impl;要让它拿到,得按**继承来的**成员做转发,那比这里押着的一两个桩大得多。**两次测量可达都是 65,所以整轮退回工作区。** 连带撤回的还有 ws1068 那个 `.from`/`.withCallback` 的修正:它单独立不住尺子(两种发射各是一个桩,数不动),按「量不出来的改动不留下」一并退回。 | 第一次:桩 32、unstubbable **1**、可达 **65**;改名后第二次:桩 35、unstubbable **2**、可达 **65**;基线 ws1068 是桩 36 / unstubbable 0 / 可达 69 / 拒绝 12,两次都掉 4 个 crate。工作区退回 `4f80a5cb`,尺子一格没动 |

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

## Object 协议第二版:第 0/1 步过了,第 2 步量到红,撤回(2026-09-10)

**第 2 步试过一次,六趟链子,撤回。** 最后一趟:**可达 crate 69 → 23、29 个无法打桩的错误**。
读数轨迹(每一趟的成因都不一样,都是从输出里读出来的,不是猜的):

| 趟 | 桩 | 新增 | 成因 |
|---|---|---|---|
| 1 | 618 | — | prelude 里有 4 处签名把错误位**写死**成 `Result<.., std::rc::Rc<dyn Object>>` 而不是 `DartError`(`dart_native`、`dart_native_as` 那种;行很长,值类型里带 `>`,一开始的正则没匹配到) |
| 2 | 300 | 227 | `flattening.dart` 判断「这个抛出的值要不要装箱」是拿 `_failure` **和字面量字符串比**;错误类型一改,比较就false,`throw AssertionError(..)` 不再装箱 |
| 3 | 300 | 227 | 装箱回来了,但走的是 `dart_boxed`,它**返回 `Rc<dyn Object>`**。加了个兄弟 `dart_thrown -> DartError`(不改 `dart_boxed`,否则每个值槽都要 upcast) |
| 4 | 104 | 31 | 抛出路径里还有一处 `as std::rc::Rc<dyn Object>` 的**显式转换** |
| 5 | 103 | 30 | 改了那处,只掉 1 个——**剩下的来自 `IrUpcast` 的 explicit 分支**,它拼的是 `type(IrType('Object'))` |
| 6 | 121 | 112 | **把类型表整个换成 `Rc<dyn DartAny>`**(计划本来就是这么说的)→ **可达 crate 23**,撤回 |

**最要紧的结构性发现,也是「一次翻,不拆」真正的理由:**
**IR 里错误槽和值槽拼法完全一样,都是 `Object`。** 所以**错误位根本没法单独搬**——
抛出路径上的 `IrUpcast` 和普通值的 upcast 是同一条路。计划 §8 那句「一次翻,不拆」
说的就是这个,我当成了「整洁些比较好」的建议,花了六趟链子才把它读懂。

**`Rc<dyn Object>` 被当字面量写死的地方(六处,下次直接照着改):**
`prelude.dart` 的 `DartError` typedef;prelude 里 4 处 `Result<..., Rc<dyn Object>>` 签名;
`dart_error_text`/`complete_error`(而 `dart_message` **不能**跟着改,它收的是普通对象,改了会断 4 个调用点);
`dart_boxed` 的返回类型;`backend_rust/failure.dart` 的 `_error`;
`flattening.dart` 里两处(装箱判据的字面量比较、抛出的显式 `as`);
`backend_rust.dart:539` 的类型表。
另外 `dyn DartAny` 需要自己的 `impl fmt::Debug`。

**还需要什么(第 6 趟露出来的):** 值槽换成 `Rc<dyn DartAny>` 之后,
prelude 里收 `Rc<dyn Object>` 值的那些函数全都对不上了(`listen_below` 那一片 E0308/E0277/E0631)。
**那一半没做**——要么给它们也换,要么在边界上 upcast。下次从这里起,别再从错误位起。

**第二次尝试(整体换,不再只动错误位)——也撤回了。** 读数:

| 趟 | 桩 | 新增 | 可达 crate | 成因 |
|---|---|---|---|---|
| 7 | 788 | 783 | **4** | 后端**自己发出来的 trait impl** 里把 `Rc<dyn Object>` 写死了(`from_answer`、`from_dynamic`),和换过的 trait 声明对不上(446 个 E0053) |
| 8 | 1494 | 1421 | 69 | 把后端那 25 处**发射出去的**字面量也换掉(7 处是**比较**,没动)。crate 回来了,桩爆了 |

**所以第 2 步一共试了八趟,两次撤回。** 但 prelude 那一侧的结论**更强了**:
**整体换**(`Rc<dyn Object>` → `Rc<dyn DartAny>`,只留 `TypeId::of` / `for dyn Object` / `impl Object` 那几行)
prelude 单独编**只要 5 处改动、0 error**:
`impl fmt::Debug for dyn DartAny`、`impl PartialEq for dyn DartAny`、`impl DartEq for dyn DartAny`、
`dart_boxed` 改成 `dart_cast_to::<dyn DartAny>`、`JsonCodec::encode` 和 `JsonUtf8Encoder::convert` 补 `DartAny` 界。
(对比第一版拿掉毯式:**161 个错、117 个类型**。)

**`Rc<dyn Object>` 被当字面量写死的地方,七层,每一层都要等上一层修好才露出来:**
(1) prelude 的 4 处 `Result<.., Rc<dyn Object>>` 签名;(2) `flattening.dart` 装箱判据的字面量比较;
(3) `dart_boxed` 的返回类型;(4) 抛出路径里显式的 `as Rc<dyn Object>`;
(5) `IrUpcast` 的 explicit 分支(**错误槽和值槽在这里分不开**);(6) `backend_rust.dart:539` 的类型表;
(7) **后端发射的 trait impl**(`from_answer`/`from_dynamic` 那些)。
后端里还有 **7 处是拿这个拼法做*比较*的**(`_failure == '..'`、`rendered == '..'`),
**那几处不能跟着换**,换了就是把判断改坏。

**第三次尝试(整体换 + 保住 cast 键)——编译这一侧全通了,渲染树没过,还是撤回。**
这次走得最远,读数轨迹(每一步的成因都是从输出里读出来的):

| 趟 | 桩(新增) | 可达 | 成因 |
|---|---|---|---|
| 9 | 1494(+1421) | 69 | 起点:整体换,保住 `TypeId::of` 那些行 |
| 10 | 82(+9) | 69 | **Dart 的 `Object` 走的是「抽象类型」那条分支,不是 `dynamic` 那条**——947 个错都是这个 |
| 11 | 79(+6) | 69 | `runtime/src/lib.rs`(**手写的**,不是生成的)里的原生回调签名 |
| 12 | **73(+0,逐字节相同)** | 69 | 四处「拿拼法当哨兵」的判断要认两种拼法(装箱判据、抛出的 `as`、两处 null 对象) |

**编译这一侧到此为止是干净的:73 桩逐字节相同、可达 69、拒绝 29、0 error。**
**但渲染树是红的**,而 §7 说渲染树是唯一的正确性判据:

| 渲染树 | 成因 |
|---|---|
| 5 行 / 1 panic | `dart_boxed` 改问 `dart_cast_to::<dyn DartAny>()`,而生成的 `dart_cast` **只答 `dyn Object`**(3521 处答旧的、0 处答新的)→ 每次 cast 落空 → `dart_boxed` 现造一个新 `Rc` → **对象身份没了**(就是 ws934 那个坑) |
| 5 行 / 1 panic | 给每个 `dart_cast` 加了答新手柄的孪生分支(3516 处),**还是 5 行** |
| **361 行** / 1 panic | **prelude 里 `TypeId::of` 那些行其实是两种东西**:`TypeId::of::<dyn Object>` 是**裸手柄的键**(要留,并且要两种都答);`TypeId::of::<Map<Rc<dyn Object>, Rc<dyn Object>>>` / `Vec<...>` 是**被人问的目标**(问的人已经改口了,所以必须跟着换)。改完 5 → 361 |
| 361 行 / 1 panic | 下一层:`material_switch` 里 `resolve(states)` 出来的 `dynamic` `dart_cast_to::<dyn Color>()` 落空。**两个夹具都复现不出来**(简单的 dynamic→接口、可空的 resolve→接口,都是绿的),所以停在这里 |

**最要紧的一条,也是三次尝试都栽在上面的:**
**`TypeId::of` 那一行有两种语义,不能当一类处理。**
「裸手柄的键」和「被问的容器目标」长得一模一样,但一个必须留、一个必须换。
我前两次分别是「全换」和「全留」,都错。

**下次从这里起:** `material_switch.rs:829` 那个 `dart_cast_to::<dyn Color>()`。
编译侧的配方是好的(上表第 12 行),照着做能到 73 逐字节相同;剩下的全是**运行期的 cast 落空**,
建议先做一个「把 `dart_cast` 的每一次落空打到 stderr」的调试开关,一次跑出所有落空的目标类型,
别再一层一层试——这次一层就是一趟链子加一次构建,十五分钟。

**照着自己写的下一步,先把「位置的种类」数了一遍(不用跑链子):**
生成代码里 `Rc<dyn Object>` 共 **20,736 处**,其中

| 种类 | 处数 |
|---|---|
| 发射出来的函数**形参** | 11,504 |
| **`TypeId::of::<dyn Object>` 那一行上的** | **7,121(34.3%)** |
| 容器类型里(`Map<Rc<dyn Object>, ..>` 之类) | 142 |
| 局部/字段标注 | 95 |

**第 8 趟那 1421 个新桩,根子多半就在那 7,121 处。** 那些是 `dart_cast` 体里
**cast 注册表的键**——`if __t == TypeId::of::<dyn Object>() { .. }`——它们必须**保持 `Object`**:
调用方拿 `dyn Object` 的 id 来问,键换成 `dyn DartAny` 就永远对不上。
我那次「整体换」的脚本只排除了 prelude 里的 `TypeId::of`,**没排除后端发射出去的那些**。

所以下次的单子是:**换手柄(13,615 处),别碰 cast 键(7,121 处)**,
而这两种在后端是**同一个字符串常量**拼出来的,得先把它们在发射端分开。

(注:这个计数是在第 8 趟留下的 `.crate/src` 上做的,错误位那一列因此偏低;
种类的比例是可信的,绝对数下次重量一遍。)

**下次要做,先解决「1421 个新桩」那一层**——第 8 趟 crate 是好的、`cargo check` 0 error,
说明类型这一层通了,炸的是**生成代码里那些拿 `Rc<dyn Object>` 当值传来传去的地方**。
建议:别再一层层试;先写个探针,把生成代码里 `Rc<dyn Object>` 出现的**位置种类**数一遍
(值槽 / 错误槽 / trait impl 签名 / 显式 cast),照单子一次改完。

**好消息:prelude 那一侧很浅。** 只把 `DartError` 换成 `Rc<dyn DartAny>`,prelude 单独编,
一开始 6 个错,补完就 **0 error**——对比第一版(拿掉毯式)是 **161 个错、117 个类型**。
第 0 步(`1903dc54`)和第 1 步(`20ddbb50`)都还站着。

### 原来的记录(第 0/1 步)

#### Object 协议第二版:第 0 步过了,第 1 步的前提不成立(2026-09-10)

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
**改在哪一层也查过了**:前端**不知道** prelude 的 Rust 槽——`_ownParameterSlots` 只管
「形参是类型参数」那种,`coerce.dart` 靠 `_bareFunctionType` 当代理,而 `onError` 正是代理失效处。
**后端知道**(它发这个调用,手上就是 prelude 签名),所以按**真实的 Rust 槽**判最直接。
真要放前端,照 ws955 的 `_genericPreludeTypes`——**读 `lib/prelude.dart` 源码**得出来,
**不许手列一张 `{'onError'}` 的表**。
另:v1 那次「74 → 98」改的是 `coercion.dart` 的 `_widened` 排除,**不是** `coerce.dart:510-521`,两处别混。

## 注册机制:删掉了(2026-09-10,ws1005 + ws1006)

**做掉的那一半(`30d0d4ad`):** `dart_register::<T>()` 是每个非 `const` 构造器发一次的,
于是它内联的东西按 T 单态化——四个 `LocalKey::with`,每个约 524 字节。
把四次写表提成一个**非泛型**的 `dart_register_fns`,四个闭包还是按 T 单态化
(它们**就是**表里的值,一个类型一个,躲不掉),塌掉的是外面那层壳。

**量出来的(release,先看了构建退出码再读段表):**

| | 之前 | 之后 | 差 |
|---|---|---|---|
| `.text` | 85,715,504 | **77,147,472** | **−8.57 MB** |
| 二进制 | 245,743,416 | **230,489,072** | **−15.25 MB** |
| `LocalKey::with` 实例 | 17,068 | **187** | −16,881 |
| `dart_register` 一族 | 36,046 个 / 11.37 MB | 24,536 个 / **2.76 MB** | −8.61 MB |

work.md §9 估 −8.5 MB,实测 **−8.57 MB**。二进制比 `.text` 掉得多,是因为少了 11,510 个符号,
`.symtab`/`.strtab` 跟着瘦。**语义零变化**:同一张表、同一个键、同一个值、同样的 `or_insert`。
尺子:stub 73 逐字节相同、拒绝 29、可达 69、run1000 连采五次 707 行 / 0 panic / 类型差异 0。

**另一半也做掉了(`ws1005` 换手柄 + `ws1006` 删表)。**
上面那段写的时候还卡着:`DART_CASTS` 要等手柄变成 `Rc<dyn DartAny>`,而第三次尝试
(`ff941317`)编译侧到了 73 逐字节相同、渲染树只到 361/707,停在
`material_switch.rs:829` 的一次 `dart_cast_to::<dyn Color>()` 落空。

第四次过了,**没有回撤**。分界点是同一行:`TypeId::of` 有两种语义。
`TypeId::of::<dyn Object>()` 是**裸手柄的键**(留,并且要答);
`TypeId::of::<Vec<Rc<dyn Object>>>()` 是**被问的目标**(问的人改口了,必须跟着换)。
这次不在文本上分,而是**在发射端**分成两个常量(`dartHandle` / `objectKey`),
每条键臂加一条 `dyn DartAny` 的孪生臂——两条不能合并,盒子里装的东西
要和提问方将要 downcast 的类型一致。

`dart_register` 在整个工作区从 1,835 个调用点变成 **0 个**;四张表、四个 `fn` 类型、
`dart_register_fns` 全删。六处读表各自落到 vtable 或原有的核心值分支上。
两轮的尺子都是:桩 71 逐字节相同、拒绝 29、可达 69、0 error、渲染树连采五次 707/0/0。

**顺带装上的工具**:`DART2RUST_TRACE_CAST=1` 把每次 `dart_cast_to` 落空按
(问的什么、实际是什么)计数,和渲染树一起报——这轮没用上,但下一次运行期
落空可以一趟跑出全表,不必一层一趟链子加一次构建。

## 拒绝归零要什么(2026-09-10,量出来的,不是估的)

29 条拒绝按**日志里自己写的理由**分组(不是印象)。**ws1015 之后是 22 条**,下表是当时的 29 条,ffi 那一行已按新读数标注:

| 根因 | 条数 | 归零要什么 |
|---|---|---|
| `dart:ffi` / win32 | 11 → **4**(ws1015) | **读的那一半做掉了**:带字段 + 三个读原语,拒绝 29 → 22。剩下的是写那一半、回调蹦床和分配器 |
| `identical` 落在不是引用的东西上 | 5 | 值类没有身份 → **别名/counted 那个工程**(ws437 量过 **+901 桩**) |
| 动态派发(`DynamicGet`/`DynamicInvocation`) | 4 | 真的按名字派发。**「名字在闭世界里只有一个声明者」只能解决 33 处里的 1 处**(量过:`slug` 有 2 个声明者,`toJson` 有 18 个) |
| `const GZipCodec` / `const JsonEncoder` | 3 | 真的 gzip 和 JSON 编码器——一个库,不是一条规则 |
| `super` 进 `Object` 的 `==`/`hashCode` | 2 | **不要动。ws985 量过:改了就是把一个对的拒绝换成一个错答案** |
| `identityHashCode` 落在值类上 | 1 | **不要动,和上面那 5 条是同一件事**(2026-09-10 查证:`_IdentityThemeDataCacheKey.hashCode` 传的 `baseTheme` 是 `ThemeData`,生成出来是 `pub struct ThemeData`,按值持有)|
| 零散(`runZonedGuarded`、`is HttpException`、一处 super 进没翻译的类) | 3 | 各自独立 |

### 22 条里,有 11 条是**同一个决定**挡着的(2026-09-10 逐条查证)

分完组才看清楚:剩下的**一半**根子是同一件事——**值语义**。没有稳定身份、
拿不到一个能活过调用的句柄,于是编译器只能拒绝。这一条决定做完,它们一起掉;
不做,单独去修任何一条都是拿一个对的拒绝换一个错答案(ws985 已经演示过一次)。

| 挡在别名/counted 后面的 | 条数 | 查证 |
|---|---|---|
| `identical` 落在按值来的操作数上 | 5 | 操作数逐个看过:两个 `Map` 值、`ThemeData` 字段、`_lineMetrics`、`oldDelegate`、`other` |
| `super.==` / `super.hashCode` 进 `Object` | 2 | 超类函数收 `&dyn Widget`,发射时不知道具体类;**870 个 `impl Widget for` 里 844 个不是 counted** |
| `identityHashCode` 落在值类上 | 1 | `ThemeData` 发出来是 `pub struct`,按值持有 |
| 混入体里闭包捕获 `this` | 1 | 混入的方法变成收 `this_: &(dyn ListNotifierMixin + 'static)` 的自由函数,**借来的接收者变不出能活过调用的句柄** |
| `dart:ffi` 的写 | 2 | `Uint8List` 是值,写落进拷贝(ws1015 把读那一半做掉了,写这一半留着) |

**`is HttpException` 试过了,是**一比一换**,不算进展(ws1016,已撤回)。**
`is X` 要两样东西,光声明类型不够:`DartCoreAs` 的 impl(prelude 的异常结构体之间
和 Rust 没有继承关系,得自己给一条)、外加 `_preludeClasses` 里有这个名字。
两样补上以后 `IOClient.send` 翻得出来了——**拒绝 22 → 21,桩 65 → 66**,
`21+66` 和 `22+65` 一样,目标一步没动,而且「桩只许掉」这条规矩挡着。
后面那个桩要的是**一个真的 HTTP 客户端**:`HttpClientResponse` 在 prelude 里
只是「为了让签名编得过」的空类型,`send__body` 要它的 `statusCode`、`contentLength`、
`headers`。所以这一条真正的前置是 HTTP 客户端,不是 `is`。
**配方留着**:补 `dart_core_as!(HttpException, SocketException, FileSystemException, OSError)`
四个,加进 `_preludeClasses`,`is` 那一侧就通了——注意只补一个的话,拒绝会原地挪到下一个名字上。

**另外 11 条各自独立**,和上面那个决定无关:动态派发 3、`GZipCodec`/`JsonEncoder` 3、
`runZonedGuarded` 1、`is HttpException` 1、一处 super 进没翻译的观察者方法 1、
ffi 回调蹦床 1、`_ffiCall`(真的调 DLL)1。

**所以「拒绝归零」这个目标条件,现在可以说得很具体**:
先做别名/counted → 22 掉到 11(其中 8 条今天就是对的,那 8 条正是靠这个决定才变成可做的);
再把另外 11 条一条条做掉 → 才到 0。ws437 量过别名那一步是 **+901 桩**,
所以它不是「顺手做掉」的东西,是一个要单独立项的决定。

### 有多少条是**对的**:8 条(2026-09-10 逐条查证;**先写成 6 条,漏了 super 那 2 条**)

**这一节的结论直接决定「拒绝归零」这个目标条件能不能字面达成:不能。**
29 条里有 **8 条是编译器正确地拒绝给出一个错答案**——5 条 `identical`、1 条 `identityHashCode`、
以及 2 条 `super.==`/`super.hashCode` 进 `Object`,全都是同一个理由:**操作数是按值来的,没有地址**。
(第一版写成 6 条,把 super 那 2 条漏在外面了;它们在上表里一直标着「不要动」。)
那 2 条的判据**量过**:`Widget` 是个 trait,所以 `Rc<dyn Widget>` 有身份——听上去可以放行,
但超类函数的接收者是 `&dyn Widget`,发射时不知道具体类是不是句柄,而 **870 个 `impl Widget for`
里有 844 个不是 counted**(值结构体)。「所有实现者都是句柄」这条限制在 `Widget` 上差得最远。五条的操作数逐个看过:
两个 `Map` 值、`ThemeData` 字段 `baseTheme`、`_lineMetrics`、参数 `oldDelegate`、参数 `other`。
`_identical` 自己的注释把道理写全了:翻译出来的值类是 `Copy`,**一份拷贝的地址什么也不说明**,
`identical(this, other)` 在那里会编得过而且**永远为 false**。
ws985 已经用夹具证过一次:把这类拒绝"修好"就是拿一个对的拒绝换一个错答案。

**所以要么这 6 条留着(数字停在 6),要么先把值类改成 counted 让它们真有身份**——
后者是别名/counted 那个工程,ws437 量过 **+901 桩**。
在那之前,报告里不该把 29 说成"还差 29 步":能动的是 23 条。

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

### `RestorableChangeNotifier._disposeOldValue` 是**两层**,第一层修好了也不动尺子(ws1019,已撤回)

`scheduleMicrotask(_value!.dispose)`。`_value` 声明在 `RestorableListenable<T extends
Listenable>` 上,而这句在 `RestorableChangeNotifier<T extends ChangeNotifier>` 里,
所以读出来是 `Rc<dyn Listenable>`,`dispose` 不在上面。

**第一层的改法是对的、也确实生效了**:撕下来的方法(tear-off)的接收者**就是**一次成员访问的
接收者,该走 `_receiver`(把擦除过的读窄化到 Dart 给的类型),而那条路上写的是裸的
`expression(receiver)`(`raw_casts.dart` 里绑定 `bindReceiver` 那两处)。
改完 `dispose` 就找得到了——**但桩没掉**,错误换成了第二层:

    expected `Rc<dyn Fn() -> Result<(), Rc<...>>>`, found closure

也就是 tear-off 造出来的闭包没有进它的函数手柄(ws1008/ws1013 那一族)。
第二层一直都在,只是被第一层挡着看不见。

**两层一起做也试过了(ws1020),还是逐字节相同,一并撤回。** 第二层的位置找对了:
接收者被绑定时,tear-off 交回来的是一个 **`IrBlockValue`** 包着闭包
(`{ let __t1 = ..; move || .. }`),而 `coerceInto` 里那条「闭包要进函数手柄」的规则
只认光杆 `IrClosure`。把块看穿之后 `!rc` **仍然没发**,所以还有第三个未知数。

**第三次量过了,`!rc` 为什么不发已经清楚了(ws1021 的 trace,代码没留)**:
那条规则包在 `if (sameRust(have0, slot) && ..)` 里,而这一处 **`sameRust` 是 false**——
tear-off 记下来的函数类型和槽的函数类型不一致(名字都印成 `Function`,差别在参数/返回上),
于是它走了**适配器**那条路,而适配器**不把结果放进手柄**。所以第二层的位置对、判据不对。
顺带量到的两个数:全程序 **3,244 处**走到那条分支时 `boxed=true`(闭包本来就在手柄后面,不用管),
**只有 11 处**是「未装箱的闭包进一个非空函数槽」,这一处是其中之一。
**下一步是第四层**:让适配器那条路也把结果放进手柄,或者让 tear-off 记的类型和槽对上。

**原来那条(已作废,留着防止重走)**:调用方是 `_schedule_microtask`,**是翻译出来的**
(不是 prelude 的 `schedule_microtask`),所以 `translated` 为真、coercion 那条闸门应该跑得到。
要打的是 `coerceInto` 对这个实参的 trace:`have0`、`slot`、`sameRust(have0, slot)`
以及 `closure.boxed` 各是什么——`!rc` 那条包在
`if (sameRust && !projectionDiffers && !arityDiffers)` 里面,四个条件里任何一个不成立都会静默跳过。

第一层是**验证过有效的**:`dispose` 改完就找得到了,只是被第二层挡着量不出来。

### 62 个桩逐个分类了(2026-09-10,ws1021 之后)

**尾巴不再按成因聚集。** 最集中的一个文件是 `widgets_window_win32.rs`,4 个桩,
而那 4 个是**四种不同成因**(ffi 分配器的 `Pointer<T>`、`Option<DynamicLibrary>` 的 `==`、
一次 moved value、一个被拒绝的静态)。按错误码分也没有大组:E0308 有 22 个,
但 `expected/found` 各不相同(ws1010 那次量过)。

剩下的大致落进四类,**没有一类是「一条规则一轮」能做的**:

| 类 | 例子 | 要什么 |
|---|---|---|
| **子系统** | `StreamController`(prelude 里**一个都没有**)、HTTP 客户端(`HttpClientResponse` 是空类型)、`ListBase` 的可变成员(`typed_buffer` 的 `removeRange` 在 Dart 里根本没声明,来自 `dart:collection` 的 `ListMixin`)、gzip/JSON、ffi 的写 | 各自是一个库,不是一条规则 |
| **擦除的类型参数界** | `material_date` 的 `T extends DateTime`——`DateTime` 在这边是**结构体**,`T: DateTime` 在 Rust 里根本写不出来 | ws544 量过:带上 Dart 的界是 **+252 桩** |
| **别名** | `foundation_collections` 那两个 E0499(`merge_sort`) | 和 11 条拒绝是**同一个决定** |
| **多层的 coercion 追查** | tear-off 那一处(ws1019/1020/1021 三轮) | 每一层都要一趟链子;第四层的位置已经写在上面了 |

**所以「桩归零」和「拒绝归零」现在指向同一件事**:先做别名/counted 那个决定
(它一次解开 11 条拒绝 + 2 个桩),再逐个把子系统补上(stream、HTTP、codec、ffi 写、ListBase)。
在那之前,能靠通用规则捡的已经不多了——本轮 73 → 62 里,最后几个都是一次一个。

### 「成员访问的接收者要窄化」——把这一族的路径数清楚了(2026-09-10)

擦除过的读进到一次成员访问,要窄化到 Dart 给的类型(`_receiver`)。
问题是**每条路径都是各自写的**,于是一条一条地以桩的形式冒出来:

| 路径 | 状态 |
|---|---|
| 调用的接收者 | 本来就对 |
| 字段**读** | 本来就对(ws1027 的 trace 证过:`targetRust=_ResetNotifier`) |
| 字段**写**(`locals.dart` 三处) | ws1027 修好,掉 1 个桩 |
| tear-off 的接收者 | ws1019 诊断对、**因为量不出来撤回**;结论留着,ws1027 靠它一眼认出同形 |
| setter **调用** | ws1029 修好,**掉 2 个桩** |

**审计过一遍**(`grep` 全部 18 处裸 `expression(receiver)`),剩下**三处**是同一个问题:
`locals.dart:761`(这条路上**第四个** `IrAssignField`,ws1027 只够到上面三个)、
`raw_writes.dart:256/262`(复合赋值下的字段写和 setter 调用)。
**改了、量了、逐字节相同,已撤回**——这个 gallery 里没有走到它们的地方。
按「量不出来就不留」的规矩撤,但**位置记在这里**:下次这一族再冒出一个桩,
先看这三处,是一行的事。

其余 15 处不是成员访问(动态派发、数值 downcast、字符串转换等),不在这一族里。

### `navigator` 那个 E0631 量过了:机制没坏,是 `dynamic?` 两边塌缩不一样(2026-09-11)

`poppedRoute._disposeCompleter.future.then((dynamic result) async {..})`,
报 expected `fn(Option<Rc<_>>)`,found `fn(Rc<_>)`。

**先怀疑「闭包形参没按槽的类型拼」,量过之后不是**:`_closureParamType` 第 521 行
`if (declared is DynamicType) return wanted;` 本来就覆盖这一形,而且 trace 打出来
**全程序只有 2 处** `expected == null` 且形参是 `dynamic` 的闭包,两处都是 `Sync`——
这一处是 `async`,所以机制**发火了**。

真正的差别在别处:`Completer<T?>`,`T` 是 `dynamic`。Dart 把 `dynamic?` 塌成 `dynamic`,
Rust 这边不塌,仍是 `Option<Rc<dyn DartAny>>`。闭包形参按 Dart 的类型拼出来是
`Rc<dyn DartAny>`,槽是 `Option<..>`,于是对不上。
**这是投影/可空那一族**(ws957 记的「投影的 `T?` 每过一道边界都要换一次拼法」),
不是闭包形参那一族——下次别再从 `_expectedFunction` 那边查。

### `provider.rs update` 的 `T` 不在作用域里:同一个文件里两种拼法并存(2026-09-11 查证)

`dart_cast_to::<dyn _Delegate<T>>()` 落在 `impl _InheritedProviderScopeElement` 里,
而那个 impl **没有 `<T>`**——`pub struct _InheritedProviderScopeElement {` 也没有,
所以这个类的 `T` 是**擦掉了的**。

**先怀疑「`_type` 不认擦除的参数」,不是**:`types.dart:163` 写着
「An erased parameter is its bound」,而且同一个文件里 `_Delegate<std::rc::Rc<dyn DartAny>>`
出现 **8 次**——那条路是通的。可 `_Delegate<T>` 同时出现 **17 次**。

**两种拼法并存说明那 17 处带的是另一个 `TypeParameter` 对象**:
`_InheritedProviderScopeElement` 是 `_InheritedProviderScope<T>` 的 Element,
Dart 里它自己也是 `<T>` 的;擦掉的是**元素类自己**的 `T`,而这些类型里名字相同的
是**作用域类**的 `T`,`_erasedParameter` 对它答 false。

**所以下一步是查「哪个 `TypeParameter` 对象」,不是查 `_type`。**
这一族是擦除孪生(ws544 量过:把 Dart 的界带上是 **+252 桩**),
`material_date` 的 `T extends DateTime` 也在里面——**不是一轮能做的**。

### (已解决,ws1037)`FocusManager.notifyListeners` 的 `to_list()` 少一个实参

`self._listeners.toList()`,`_listeners` 是 `ObserverList<VoidCallback>`,
它自己声明了 `toList({bool growable = true})`,翻出来是 `to_list(&self, growable: bool)`。
调用发出来是 0 个实参。

**排掉了两个想当然的猜测**:
(1)**不是 ws1004 的路由**——`declaring` 是 `ObserverList`(package 类,不是 `dart:`),
`_dartIterableCall` 第一条就返回 false;
(2)**不是没填默认值**——`parameters.dart` 的 `_omitted` 会填具名默认值,前端填了。

**也不是后端 `calls.dart` 那条 `to_list` 分支**(那条注释写着「`growable` 两边都丢掉」)。
给它加了「接收者自己声明了 `toList` 就别丢」的判断,**逐字节相同**;
trace 打出来全程序走到那条分支的接收者**全是 `List<..>` / `Set<..>`**,
`ObserverList` 那处根本不经过它。已撤回。

**照着这句做,一次就找到了**(ws1037):Dart 根本不是 `_listeners.toList()`,是
**`List<ValueChanged<..>>.of(_listeners)`**——落在 `nullaware.dart` 的 `List.from/of` 分支,
和 `calls.dart` 那条 `to_list` 分支毫无关系。之前那次 trace「接收者全是 List/Set」不是死路,
是在指路,我读成了死路。
**改法用的是现成机制**:翻译出来的、自己是 `Iterable` 的类本来就有 `__to_list()`
(`_emitToList` 发的,Dart 里没有这个名字),正是为这种场合准备的。

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

- **体积:量了,涨 2.8%**(2026-09-11 08:19,ws1066 之后;现建的 release,7m57s)。
  **release 对 release**:`.text` 73,071,360 → **75,112,624**(**+2.8%**);
  文件 214,887,120 → **218,898,768**(**+1.9%**)。
  跨 ws1053–ws1066 十四轮,其中 ws1061/ws1062 把 **22,108 处 `.unwrap()` 换成了
  `?` 传播加 `Err` 构造**,再加身份令牌(每个带令牌的实例一个字、一条 vtable 方法)、
  动态成员分派(每个候选一条 downcast 臂)——**合起来 2.8%**。
  `/tmp/work.md` 第五节第 3 条担心的涨幅,量出来是小的。
  **先前那条 +48.8% / +64.2% 是错的,已作废**:我拿 debug 的二进制比了 release 的基准,
  而盘上那个 release 还是 09-10 17:55 建的,早于本会话每一轮。
  **两个 profile 的基准要分开记**:debug(render_ruler 和 run_main 跑的那个)
  09-11 08:00 是 `.text` 108,803,442 / 文件 353,012,488;release 见上。

- **「任何 panic 都不算过」那件事的三类边界:量过了,两类不存在,一类合法**
  (2026-09-11,ws1062 之后)。`/tmp/work.md` 第六节把「76,005 个不返回 `Result`
  的函数」当成真正的成本,并列出三类要先量后改的边界。量下来:
  **①闭包槽不是边界**——生成侧 **9,068 个 `dyn Fn` 槽,每一个都带 `-> Result<..>`,
  一个例外都没有**(两种数法互相印证:按 `-> Result<` 数 8,934,按「`dyn Fn` 后 200
  字符内没有 `-> Result<`」数 **0**;差额是嵌套括号让第一个正则漏掉的)。
  prelude 那 68 处里没带 `Result` 的基本是泛型的 `-> R`/`-> B`(宏与类型别名)
  和注释里的例子——`R` 本来就可以实例化成 `Result`。
  **②`Drop` 不是边界**——生成侧和 prelude **各 0 个 `impl Drop`**。
  **③native 边界是合法的那一类**——**没有任何 `extern "C"`**;
  7,162 处 `panic!("native ..")` 说的是「宿主没答」,按第三节的判据本来就该炸;
  `catch_unwind` 全程序只有 1 处,就是 `run_callback` 那处,计划自己写着不碰。
  **所以第六节剩下的不是三个决定,是零个**:那 76,005 个函数不是「接不住」,
  只是「还没接」。第 3 组(prelude 32 处)会动签名,但它动的是**函数**,不是边界。

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
  **(2026-09-11 再量,ws1052 的夹具)** 形状原样复现(`a == b` 两个不同对象 → true)。
  新看到的一点是**同一个 impl 里两半不一致**:
  `impl DartEq for Leaf { fn dart_eq(&self, other) { self == other } `——按字段;
  `fn dart_hash_code(&self) { Node::hash_code(self).unwrap_or(0) } }`——**已经**转发到继承来的声明。
  哈希那半怎么找到继承链上那个声明的,相等这半照着做就是了。

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
