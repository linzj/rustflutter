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

**维护规约(防再胖):**〈活账〉表只留最近约 40 行;每批提交时把滑出窗口的行直接
删掉(git 有)。「撤回与作废」和「数字轨迹」是考古结论的留置处,只增不删。
**每轮的记录写成〈活账〉的一行,不写成 `## wsN` 小节**——2026-09-07 到 09-09 之间
文件涨回 6 倍,就是因为规约只管住了表,而记录换成了小节。一条规则的「为什么」
值得留下时,它进〈九条要记住的〉〈撤回与作废〉〈已知欠账〉,或另起一个不带轮次的
标题(本文里那几节就是这么来的);其余的,git 有。

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
  ws876   153 /  57
  ws877   152 /  52

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

近期的(细节在活账/git):

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
| run818 | the reading still holds | — |
| ws819 | a function value handed to translated code is the handle | stub **202**,拒绝 88,可达 64 |
| ws820 | `x is DateTime`, and one convention for a callback slot | stub **201**,拒绝 85,可达 64 |
| ws821 | `.indexed`, `unawaited`, and what identity cannot answer | stub **202**,拒绝 80,可达 64 |
| ws822 | `Map.addEntries`, and two rules that measured to nothing | stub **202**,拒绝 79,可达 64 |
| ws823 | `x is Function` is a test on a signature, not on being a function | stub **203**,拒绝 78,可达 64 |
| ws826 | an enum the tree shaker emptied is not a refusal | stub **202**,拒绝 71,可达 64 |
| ws828 | N catch clauses are one catch that dispatches on the type | stub **202**,拒绝 70,可达 64 |
| ws830 | `jsonEncode`, by the value's own type | stub **203**,拒绝 69,可达 64 |
| ws831 | the `toEncodable` shape | stub **202**,拒绝 69,可达 64 |
| ws832 | `const Stream()`, `stdout`, and a generic local function that needs its call site too | stub **203**,拒绝 64,可达 64 |
| run834 | the reading, again | — |
| ws836 | a rule that changed nothing, and where the generic local function stops | stub **203**,拒绝 64,可达 64 |
| ws838 | a local function that is only called may borrow | stub **204**,拒绝 60,可达 64 |
| run842 | the reading, after the borrowing rules | — |
| ws847 | a `ByteBuffer` is its bytes, and a narrow number prints as itself | stub **201**,拒绝 60,可达 64 |
| run848 | the reading, after `toString` changed for ten types | — |
| ws850 | `is` against a function type is a downcast, not an arity guess | stub **201**,拒绝 59,可达 64 |
| ws852 | a prelude generic answers `is` by the runtime type it reports | stub **202**,拒绝 58,可达 64 |
| ws853 | a class that *is* the prelude's future has no members of its own | stub **201**,拒绝 57,可达 64 |
| ws854 | a tear-off adapter returns into its slot, like any other value | stub **201**,拒绝 57,可达 64 |
| ws855 | an adapter holds `this` itself, or it borrows whoever does | stub **195**,拒绝 57,可达 64 |
| ws856/857 | a closure into an optional function slot is spelled, by a binding and not a cast | stub **195**,拒绝 57,可达 64 |
| ws858 | a null-aware map spells its return where the body cannot fail | stub **193**,拒绝 57,可达 64 |
| ws859/860 | a trait that implements one of the prelude's has it above it | stub **189**,拒绝 57,可达 64 |
| ws861 | a value widens under the `Option` only when it is in one | stub **186**,拒绝 57,可达 64 |
| ws862 | the prelude's `DateTime` says what Dart's says | stub **184**,拒绝 57,可达 64 |
| ws863 | an `Option` hashes whatever it holds, not only a handle | stub **181**,拒绝 57,可达 64 |
| ws866 | a chain step returns against its own return, as any closure does | stub **178**,拒绝 57,可达 64 |
| ws867 | each arm of a conditional widens, not the conditional | stub **175**,拒绝 57,可达 64 |
| ws868 | an arm that never arrives is one whether or not a `Some` is on it | stub **172**,拒绝 57,可达 64 |
| ws869 | `Uri.tryParse` is a name the prelude did not have | stub **171**,拒绝 57,可达 64 |
| ws870 | a wider impl's field is read in this class's terms | stub **169**,拒绝 57,可达 64 |
| ws871 | a mapped element's body unwraps, as a chain step's does | stub **166**,拒绝 57,可达 64 |
| ws872 | `hashCode` on a handle reaches the class's own, not `Rc`'s blanket | stub **164**,拒绝 57,可达 64 |
| ws873 | identity on a nullable slot asks the spelling, not the class table | stub **159**,拒绝 57,可达 64 |
| ws874 | a method body falls into the null of its own type, once | stub **156**,拒绝 57,可达 64 |
| ws875 | a scalar stands in an interface slot it implements | stub **154**,拒绝 57,可达 64 |
| ws876 | `a ?? b` is an object where Dart says `Object`, not its text | stub **153**,拒绝 57,可达 64 |
| ws877 | a super call reaches an operator, as it reaches any other member | stub **152**,拒绝 52,可达 64 |

## 下一步(2026-09-05 重铺)

本节和〈当前队头〉原来停在 2026-09-03 目标改写时(`crate.py` 的 416 个错误、
老 census 的类别表),早已对不上,作废重铺。活账是上面的 ws 表,队头以表末
(ws350:**3284 stub / 804 refusal / 782 `todo!`**,138+1 个 crate 全到)为准:

1. **782 个 `todo!` 的剩员**:`debugFillProperties` 197 个是 AOT 树摇掉的成员
   (dill 里没有,release 下是死代码,todo 是实话);ws348 露出的新桩里
   `insert`/`remove`/`_insertIntoChildList`/`_removeFromChildList` 各 12 个 applier
   ws350 已全部编过,剩 `create_ticker` 35 个——先看它编不过在哪。
2. **refusal 804**:ws348 一道清掉 68(链式 setter 赋值当值用);剩下的还没按
   类别重新归并,归并一次是下一轮的事。
3. **Result 的记账债**:函数值调用 1200 处不参与失败传播(闭包里
   `.unwrap_or_else(panic)` 计数,ws244/ws250);原语(越界/除零/null check/`as`)
   不进 Result;microtask/Timer 回调的错误无人可收,`.unwrap()` 记为响的债。
4. **`Rc<dyn Fn>` 的 `PartialEq`** 撞 orphan 规则(`Set<Ticker>.remove` 一族;
   ws299 记了两个候选:prelude 自己的 `dyn DartFn`,或 `Map`/`Set` 改用 `DartEq`
   比键),都没动手。
5. **`dynamic` vs `Object?`** 表示不一致(第 129 轮起挂着);RegExp 无引擎
   (intl 依赖);多实现体的 trait 泛型方法 31 处(`IrDynamicDispatch` 没做,ws291)。
6. **运行时那半仍是 0/168**:`runtime/` crate 不存在。翻译半烧完之前它是最终
   瓶颈;原话仍有效——立 crate 接管 prelude,再把 168 收到启动路径上。

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

**当前 frontier(run601):** 程序挂住(无 panic,超时)——locale 名把
`StringBuffer` 打成 "Instance of"、`flutter/assets` 的 `en.json` 宿主回 none、
周期定时器让 `run_main` 永不闲;新仪器 `DART2RUST_RUN_SECONDS`(默认 60)、
`DART2RUST_TRACE_TIMERS`。之后:Goal 3 的 `size=`。


**(2026-09-09 校注)** 本节六条与〈当前队头〉自 09-05 起未动,现状以〈活账〉窗口
末行为准:**ws877 152 stub / 52 拒绝 / 64 crate 全可达**,运行尺子 run874
**708 行对 708 行、0 类型差异、0 panic、196 帧**。六条里 1(`todo!` 剩员)仍未再量;
2(拒绝)804 → **52**;3(Result 记账债)仍挂,并且长出了新的一面——见〈已知欠账〉
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

```
ws350:  3284 stub / 804 refusal / 782 todo!,138+1 个 crate 全到
轨迹:   ws243(panic 模型最好) 7843
        ws246(Result v2 初版) 17068 → ws250 11061
        ws271(开放类落地)     7270
        ws279(this 句柄化)    5247
        ws350                 3284
```

三个数各量一样东西:**stub** 是「译出了、编不过」的函数;**refusal** 是
「没译出」的函数;**`todo!`** 是「编得过、一跑就 panic」的转发器体——
ws344 才照到它,一量 26199 个,削到 782。

老的那份类别表(闭包捕获 `this` 599、撕方法 435 那份)来自更老的输入,
已删——它量的事(ws119–122 的所有权三样)大半已经做完;新输入的 804 个
refusal 还没按类别归并,归并之前不引用旧数字。


**(2026-09-07 校注)** 翻译那半的队头以活账表末为准:

```
ws601:  722 stub / 252 拒绝 / 63 crate 全可达(138 分区,延迟库合并后)
轨迹:   ws350  3284 / 804 / 138+1
        ws522  1068(协变全有或全无)
        ws552  794
        ws601  722 / 252
```

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

## 已知欠账

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
- ~~**727 个 stub 的长尾**~~(2026-09-09:**152**):最大类是 "mismatched types"(约一半),其余是参数数、
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
- ~~**覆盖时收窄类型实参**~~(run703 量的,ws708 已解:覆盖关系算 flow site,那 82 处的
  `WidgetStateProperty<T>` 等被擦除,两边拼法一致):子类 getter 覆盖基类字段并把类型实参收窄——
  `_SwitchDefaultsM3.thumbColor` 是 `WidgetStateProperty<Color>`,基类字段是
  `WidgetStateProperty<Color?>?`。Dart 的协变允许,Rust 的 `dyn WSP<Rc<dyn Color>>` 与
  `dyn WSP<Option<Rc<dyn Color>>>` 无关。全 gallery **82** 处,全是 Material 的 defaults 惯用法;
  这些 trait 访问器仍读存储(基类字段的 `None`),所以 `Switch._getSwitchSize` 之后的
  `defaults.thumbColor!` 在运行期 unwrap 到 `None`。两条路都试过:适配器对象会换身份
  (covariance.dart 开头写过为什么不走);擦除那条量了 +22(见〈撤回与作废〉ws704)。


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

## ws824 / run825 -- where this stretch stands

    bin/run_chain.sh:  202 stubbed, 79 refusals, 64 reachable crates
                       (the stub set is ws822's, member for member)

    DART2RUST_OS=android DART2RUST_DUMP_RENDER_TREE=1 bin/run_main.sh:
                       exit 0, budget spent with main pending
                       201 frame(s) drawn, 0 panicked
                       render tree 708 lines, RenderErrorBox 0

    diff ref_render_walk_settled.txt walk, ignoring size=/offset=:    0
    diff ref_render_walk_settled.txt walk, as printed:              508

From ws808's 221 stubbed and 130 refusals: 351 unfinished members down to
281, reachable crates 64 the whole way, and the run ruler's reading
unchanged -- 708 lines, no type differing, nothing panicking.

Fourteen rules landed with a fixture each, and six were reverted after
being measured: the chain's type (twice), `identical` on a prelude value,
`identityHashCode`, a `dart:core` interface as a supertrait, `first =`/
`last =`, and `x is Function`. Every one of the six is written up above
with what it answered and what it should have answered; four of them
would have compiled and been *wrong*, which is the reason each round pays
for a fixture before it pays for a chain.

What is left is not a table gap. The 79 refusals are, by count:

    24  a top-level function with no translation -- 16 of them `dart:ffi`'s
        internals (`_abi`, `_loadInt64`, `_storePointer`) behind the win32
        windowing layer, which the prelude deliberately gives names and no
        behaviour. Clearing them means a memory model for `Pointer`.
     8  an enum the tree shaker emptied of its constants
     6  `super.==` / `super.hashCode` into `Object`, and 3 more `super`
        calls into classes not in the file
     5  `identical` on something that is not a reference
     5  a `const` instance of a prelude class
     5  a closure capturing `this` in a mixin application
     4  a generic local function
    19  a tail of ones and twos

Three of those groups -- identity on a value class, `dart:ffi`'s memory,
and a local function that captures `this` and cannot escape -- are the
same question asked three ways: **what does this compiler let an object
be?** A Dart list is a reference and a `Vec` here; a `Widget` is an object
with an address and a struct here. Each of the three is a model change
with its own round of fixtures, not a rule that can be added to a table.

## ws828 census -- what identity would cost, counted rather than guessed

Two groups of refusals are the same question -- `identical` on something
that is not a reference (5), and `super.==` / `super.hashCode` into
`Object` (2 classes, whose refusal drags 6 more `super.==` calls into
bases whose own `==` was refused for the same reason). Before deciding
whether to change the model, both halves were counted over
`app_aot_sig.dill`:

    identical()/identityHashCode() operand classes:  239
      215 Object, 40 Zone, 26 Endian, 25 List, 11 Color, 10 Uint32List,
      then a long tail: every ThemeData family member, TextStyle,
      BorderSide, RenderObject, SemanticsNode, MenuStyle, ButtonStyle, ..
    classes whose member calls `super` into `Object`:  2
      DiagnosticsNode, Widget

The faithful fix for a value class is an identity *token*: a hidden field
assigned at construction and carried through `clone`, because a clone in
this model is not a new Dart object -- it is the same object being passed
around, which is the model's whole premise. That is exactly Dart's
identity, and it is why the address of a copy is not.

It is also 239 classes. Every one of them would need the field, its
constructors would need to assign it, its `const` instances would need the
canonical value Dart's const canonicalisation implies, and the derived
`PartialEq`/`Hash` would have to be replaced by hand-written impls that
skip it -- on the whole `ThemeData` family, which is where const instances
and structural equality both live. For eleven refusals. Not this stretch,
and not without its own run of fixtures.

The narrow half was tried: `super.==` into `Object` routed through the
`_identical` machinery, so that it answers where that machinery can and
refuses where it cannot. It refuses -- at the argument, which arrives as a
value (`IrCall (Object)`) rather than a reference. The two halves are one
wall, and the rule is reverted rather than left as a path that only ever
says no. `super.hashCode` did translate on its own, and took a call
resolution with it (`hash_code()?` on an `i64`), so it goes back too.

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


## ws864 -- two rules tried, neither shipped, and why

The double box is real and now read from the source rather than guessed at.
`CupertinoDatePicker.build` emits

    columns.iter().map(|child| (dart_object(Expanded::new(..)) as Rc<dyn Widget>))
        .collect::<Vec<_>>()
        .into_iter().map(|v| (dart_object(v) as Rc<dyn Widget>)).collect::<Vec<_>>()

-- the `map` closure's body is lowered against the slot it lands in
(`_withExpectedReturn`) and already hands back the handle, while the chain
is *recorded* by Dart's static type, `Iterable<Expanded>`, so the coercion
into `List<Widget>` widens every element a second time. `dart_object`
around an `Rc<dyn Widget>` makes an `Rc<Rc<dyn Widget>>`, whose pointee
implements nothing: the same shape ws751 fixed on `!rc`. Three `build`s
stop there.

Two fixes were written and both reverted, because **neither could be made
to fail at HEAD**:

  * a trait-typed value is already a handle, so `IrUpcast` should take
    `_handleOf` and not `dart_object` (traithandle, in two shapes);
  * the chain's type follows its `map`'s closure, not the static type
    (mapwiden).

Four fixtures across the two, all agreeing at HEAD as well as with the
rule. The shape needs the *expected return* to reach inside the closure,
and that depends on whole-program context -- the erasure census, TFA, the
closed world -- which a one-file fixture does not recreate. This is the
same wall ws858 hit and worked around by reading the emission.

Three more attempts followed, each read back from a fresh translate of the
gallery rather than from a fixture, and each eliminating a candidate:

  * the chain's own `rustType`, set from its `map` closure's recorded
    return -- which *is* the coerced one (`_returnsType` takes
    `_expectedReturn` when there is one), so the closure knows. The
    emission did not change: `expression()` ends with
    `if (erasedThrough != null) lowered.rustType = erasedThrough`, which
    overwrites whatever the visitor recorded.
  * that overwrite guarded for a chain. Still no change.
  * the element type carried through `toList()`/`toSet()` as well, since
    `map(..).toList()` is two nodes. Still no change -- this attempt had
    dropped the guard above, so the clobber was back.

The three together were then tried as well, and the emission still did not
change -- so the model behind them was wrong, and two traces were added to
say what is actually there (both reverted; the switches are
`DART2RUST_TRACE_ELEMENTMAP` and `DART2RUST_TRACE_CHAIN` in the commit
message of this note if they are wanted again):

  * `TRACE_ELEMENTMAP` -- the second box is `coerce.dart`'s
    element-by-element rule, and every one of its `have`s is a
    `List<ConcreteWidget>` against a `List<Widget>` slot
    (`_TextStyleItem`, `LayoutId`, `DropdownMenuItem<int>`,
    `_OverlayEntryWidget`, ..). That coercion is *correct* on its own: a
    list of values does become a list of handles.
  * `TRACE_CHAIN` -- the `map` closure of the `Expanded` site records
    `returns=Expanded` (one such closure in the gallery; twelve others
    record `Widget`), and its `toList()` receiver is `Iterable<Expanded>`.

So the two halves disagree in the *other* direction from what was assumed:
the closure is declared to return `Expanded` and Dart's static type agrees,
which makes the element coercion right -- and the closure's **body** has
already been upcast to `Rc<dyn Widget>` anyway. The box to remove is the
one inside the closure, not the one after it, and the question for the next
attempt is what put it there when the closure's own return says `Expanded`.
Nothing was shipped: five candidate fixes, all reverted, and the shape is
now pinned to a single closure whose body and declared return disagree.

That is the honest limit here: what is left in the tail is mostly shapes
whose cause is only visible with the whole gallery in hand, and the
fixture-first rule this session has kept means they wait for a way to
reproduce them rather than for a plausible patch.

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

run877 holds the ruler after ws875-ws877: 708 walk lines, 0 type-only
differences, 508 as printed, no `RenderErrorBox`, 197 frames drawn and 0
panicked.

## ws878 -- `identityHashCode` on a handle is the address behind it

    bin/run_chain.sh:  152 stubbed (unchanged), 49 refusals (was 52), 64 crates

`identityHashCode(x)` was refused wholesale as a top-level function nobody
wrote. It is the hash of the object's *identity*, and identity here is the
address behind the handle -- the same one `_identical` compares and the one
`Expando` keys by. So a handle has an answer, and the prelude gives it one:

    pub trait DartIdentityHash { fn dart_identity_hash_code(&self) -> i64; }
    impl<O: Object + ?Sized + 'static> DartIdentityHash for std::rc::Rc<O>

A value class does not: it is copied into each slot, and two copies of one
Dart object would answer differently. Those stay refused, which is the same
answer `identical` gives on one (`_identical`'s census, ws828).

Dart's null is one value with one hash, and an `Object`/`dynamic` slot
holds it as a fresh `Null` each time `dart_null_object` writes one, so the
impl answers `2011` for it -- the hash `RcHashCode` already gives `None`.

`idhash` asks the three things the gallery's callers ask: that one object
hashes the same twice, that two objects hash differently, and that an
`Object? value` field holding null hashes the same twice. At HEAD it is the
gallery's refusal verbatim, twice; with the rule both ends print
`true false true false true`.
