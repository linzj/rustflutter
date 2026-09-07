# STATUS(dart2rust)

**2026-09-07 压缩(用户裁定:只保留这一个文件)。** 第 2–134 轮叙事、「运行尺子」
的 run429–run471 表、「第二个 goal」的 run472–ws571 表行移出本文——它们一行未删
地留在 git 历史:`git log --oneline -- tools/dart2rust/STATUS.md`;
取某版:`git show <sha>:tools/dart2rust/STATUS.md`(压缩点 = 本文件入库的那一次提交)。
保留节的正文与原文件逐字节一致;新增内容只在本头部、各「章结/校注」段、
〈数字轨迹〉〈撤回与作废〉〈已知欠账〉三节和活账窗口新行。

**维护规约(防再胖):** 〈活账〉表只留最近约 40 行;每批提交时把滑出窗口的行直接
删掉(git 有)。「撤回与作废」和「数字轨迹」是考古结论的留置处,只增不删。

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
- **现状(run599):节点类型序列 6/6 一致(自 run584);还差 `size=`**——dump 时
  没有 layout 数据(2 帧已画,待查是 dump 时机还是 size 读取)。
- 仪器:`DART2RUST_DUMP_RENDER_TREE=1`(render 树)、`DART2RUST_DUMP_APP=1`
  (元素树)、`DART2RUST_RUNTIME_TRACE=名,名`(成员入口)、`DART2RUST_TRACE_HOST=1`
  (native 符号)、`DART2RUST_TRACE_CTOR`、`DART2RUST_TRACE_COVARIANT=1`、
  `DART2RUST_TRACE_MESSAGES`(平台消息字节)、`DART2RUST_RUN_SECONDS`(run_main.sh
  默认 60,预算用完报告还活着的定时器并 dump)、`DART2RUST_TRACE_TIMERS=1`。

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

## 撤回与作废(不要再试)

近期的(细节在活账/git):

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

## 活账:ws/run 表(窗口 run572 起;更老的在 git)

| 轮 | 第一个停点 / 读数 | 处理 |
|---|---|---|
| run572 | **第一帧画出来了**（`1 frame(s) drawn (0 panicked)`；平台消息多了 `flutter/assets`、`flutter/restoration`）。渲染树仍只到 `RenderConstrainedBox`（`RootRestorationScope` 等 restoration bucket 期间建的 `SizedBox`）。第一处 panic：字体清单加载失败后走 `then(onError:)`——`onError` 是 `Function?` 槽，闭包裸着传（`Some(Rc::new(|e, st| ..))`），prelude 的 `dart_call_error_handler` 要的是带元数的函数对象 → "a {closure} called as a function"。通用修：prelude 被调方的裸 `Function` 槽和 `dynamic` 一样要 coerce（`_calleeTranslated`），值走 `dart_function_object`。夹具 onerr 编译通过（异步结果由 gallery 运行验证）。 | ws573 量；run573 看下一站。 |
| ws573/574 | 链：**738**（-2），141。（ws573 是无效版本——`_widened` 对 prelude 被调方的闭包字面量整个跳过按类型 coerce，裸 `Function` 槽例外之后才生效；ws573 按 PID 杀掉重跑为 ws574。） | |
| run574 | **2 帧**；restoration 过了，真正的子树开始建（`_ShortcutsState.initState`）。第一处 panic：`mapEquals` 的 `identical(a, b)`——两个提升过的 `Map<T, U>?` 局部（`IrNullCheck`）不是引用，拒绝。修：值类型的提升可空槽视同槽（快路径答"不同"，句柄仍走指针规则）。夹具 identmap 与 dart 一致。 | ws575 量；run575 看下一站。 |
| ws575 | 链：**738**（持平），141。 | |
| run575 | 渲染树长到 `_ReusableRenderView → RenderSemanticsAnnotations`。第一处 panic：`RenderSemanticsAnnotations.describeSemanticsConfiguration`（`SemanticsAnnotationsMixin` 应用进来的体）里 `super.describeSemanticsConfiguration(config)` 拒绝——`_realOwner` 从应用类往上爬时，越过 `RenderProxyBox` 的匿名应用后停在 `RenderBox`（不声明），没继续到 `RenderObject`。修：真类在匿名应用**前后**都按"是否声明具体成员"继续爬。夹具 supermix 与 dart 一致。 | ws576 量。 |
| ws576 | 链：**738**（持平），141。 | |
| run576 | 渲染树到两层 `RenderSemanticsAnnotations`。第一处 panic：`_LayoutCacheStorage.clear` 的 stub——`_cachedDryLayoutSizes?.clear()`：`?.` 的体在 `as_ref().map(|it| it.clear())` 里改不了 `&Map`。修：`?.` 体是就地修改的调用时，接收者按**地方**借可变（cell 的 `borrow_mut().as_mut()`、自家字段、局部）。顺带修两件 supermix 揪出的通用缺口：(1) `final` 的静态/顶层集合被就地修改（`log.add`、`pendingFontFutures.remove`）时读的是克隆——`_StaticFillFinder` 全组件找就地修改的静态字段，标成 cell；后端 `_cellPlace` 认 `IrTopLevel`/`IrStatic` 的 cell（`(**X).borrow_mut()`）；(2) cell 接收者上 `borrow_mut()` 先于实参求值——实参先绑定为 `let __aN`（闭包/字面量除外）。夹具 statmut、nullmut。 | ws577 量；run577 看下一站。 |
| ws577 | 链：**786**（+48）。回归三组：(a) 预绑定实参丢了隐式上转（`_tickers!.remove(ticker)`：`let __a0 = Rc<_WidgetTicker>` 对 `Set<Rc<dyn Ticker>>`）——绑定时 `_explicitUpcast`；(b) `late` 字段的 cell 装的是 `Option`（`ObserverList._set.clear()`）——`_mutPlace` 对 late 字段 `as_mut().unwrap()`；(c) 静态持有的**计数类**句柄上的字段写（`GoogleFonts.config.allowRuntimeFetching = false`）被当作静态被改——只有值 struct 才算。另揪出 `_mutatesInPlace` 只认 Rust 拼法（`addAll` 没进 cell 路径）。夹具 latecell（late 集合、`Set<Ticker>` 存/删计数类、静态句柄字段写）与 dart 一致。 | ws578 量。 |
| ws578 | 链：**734**（-4 vs ws576：`_LayoutCacheStorage.clear`、`SemanticsNode.==`、`Element.activate`、`RenderTable.mount`、`WidgetSpan.extractFromInlineSpan`；+1 `ListNotifierMixin` 的 super fn：闭包里 trait 访问器的 `self` 是句柄 `__me`，`_cellPlace` 直接传了——改用 `_addressOf(this)`），141。 | |
| run578 | **渲染树 4/6**（`_ReusableRenderView → RenderSemanticsAnnotations → RenderSemanticsAnnotations → RenderTapRegionSurface → RenderSemanticsAnnotations`），2 帧。第一处 panic：`_loadAll` 的拒绝"lazy Iterable 没有 collect"——`Future.wait(pendingList.map(..))`。通用改：链作为值一律 `collect`（prelude 的 `Iterable` 本来就是 `Vec`，惰性本就没有）。 | ws579 量；run579 看下一站。 |
| ws579 | 链：stub **756**（+22）但**拒绝 333→286**（-47）：链一律 collect 让 47 个原本整体拒绝的函数译出来了，其中 22 个卡在别的编译错误（`ends_with(String)` 不是 Pattern、`List::unmodifiable`、`Set::remove_all`、`?` 在 `()` 闭包里……）。合计（stub+拒绝）1067→1042。reachable 140 = workspace.py 这轮切成 137 crate（`get_above` 并进去了），全部可达。 | 下面逐个收这些编译错误。 |
| run579 | `_loadAll` 过了；下一站 `lookupGalleryLocalizations`：`deferred as` 导入的 `LoadLibrary`/`CheckLibraryIsLoaded` 拒绝。通用：延迟库在这里静态链接——`loadLibrary()` 是已完成的 null future，检查是 null。夹具 deferred 编译通过。 | ws580 量；run580 看下一站。 |
| ws580 | 链：stub **767**（+11：全是延迟库的 `__load_library_*`——`future_value::<Rc<dyn Object>>(dart_null_object())` 少了 `Some`；改成无参 `future_none`），拒绝 286→**271**。这轮顺手收 ws579 暴露的编译错误：`startsWith/endsWith(String)`→prelude `dart_starts_with/dart_ends_with`（`_stdShadowed` 表）；`List.unmodifiable`/`Map.unmodifiable` 进 `preludeSiblings`（同 `of`），`Set.removeAll/retainAll/containsAll` 的槽是集合自己的元素（同 `addAll`；实例调用也走 sibling，sibling 被 TFA 摇掉时直接把 `Iterable<Object?>` 参数拼成 `Iterable<E>`）；AOT dill 里列表字面量是 `_GrowableList._literalN<dynamic>(..)`，进槽时同 `ListLiteral` 一样按槽的元素类型重新下降；prelude `Set` 补 `remove_all/retain_all/contains_all`。夹具 strpat 与 dart 一致。 | ws581 量；run581 看下一站。 |
| ws581 | 链：stub **745**（-22），拒绝 271，140。 | |
| run581 | 延迟库过了；`lookupGalleryLocalizations` 换了拒绝：嵌套 switch 的 case 里 `break #L1`（跳出外层 switch）——原规则只认 case 体最后一句的 `break`。通用：(1) 尾语句穿过嵌套块找；(2) 真在中间跳出的，match 套进带标签的块，`break 'l`（`_SwitchBreakFinder`）。夹具 switchbrk 与 dart 一致。 | ws582 量；run582 看下一站。 |
| ws582 | 链：stub **745**（持平：-2 `_onEdgeForDirection` / +2），拒绝 271→**261**。reachable 63——不是回归：延迟库译出来后 `gallery_localizations` ↔ 各语言库成了 import 环，workspace.py 把它们并成一个 `merged_gallery_scc`（137→60 crate）。两个新 stub：(a) `lookupGalleryLocalizations`：`then` 回调返回 `FutureOr::value(dart_object(Zu))`，rustc 把 `FutureOr<Rc<Zu>>` 推死了不上转——通用：`FutureOr` 包装改用自由函数 `future_or_value::<T>`/`future_or_future::<T>` 把槽的 `T` 拼出来；(b) `Navigator._flushHistoryUpdates`：带标签块里的裸 `continue`（E0695）——循环体里有提前跳出的 switch 时，循环体按 CFE 的标签块拼，`continue` 就是 `break 'body`。 | ws583 量；run583 看下一站。 |
| ws583 | 链：stub **743**（-2），拒绝 261，60 crate 全可达。 | |
| run583 | `lookupGalleryLocalizations` 译出来了，运行时 `future_none::<Rc<dyn Object>>()` panic："Future.value() without a value on a non-nullable type"——`Rc<T>` 的 `DartNullable::dart_null` 一律 None，而 `dynamic` 的 null 是 `Null` 对象。修：blanket impl 里按 `TypeId` 认出 `Rc<dyn Object>`（Box<dyn Any> downcast，无 unsafe）给出 `Null` 对象。 | ws584 量；run584 看下一站。 |
| ws584 | 链：stub **743**（持平），63 可达。 | |
| run584 | **渲染树六个节点类型全到齐**（尺子 6 行的类型序列一致；差别：还没 layout 所以没有 `size=`，以及开放类实例的 `runtimeType` 打成 `RenderConstrainedBoxImpl`），3 帧。第一处 panic：`_LocalizationsState._textDirection` 的 `_localizations[WidgetsLocalizations]!` 为 None——`LocalizationsDelegate<T>` 的 `T` 是被擦除的类型参数，`Type get type => T` 译成了 `Type::of("Object")`，所有 delegate 共用一个 map 槽。通用：**被擦除的类参数作为类型字面量，由对象回答**——用到它的抽象类得到 getter `_typeArg<Class><T>`（trait 方法），其下每个类按自己祖先链里放进去的实参实现（`hierarchy.getInterfaceTypeAsInstanceOfClass`）；具体类自己的擦除参数没法答（值 struct 不带 T），维持 bound。夹具 erasedtype 与 dart 一致。 | ws585 量；run585 看下一站。 |
| ws585 | 链：stub **743**（持平），拒绝 261。 | |
| run585 | 各 delegate 的 `type` 分开了，于是所有 delegate 都真的 load 了；第一处 panic 挪到 `_MaterialLocalizationsDelegate.load` → intl `initializeDateFormattingCustom` 的 `dateTimeSymbols[locale] = symbols`：`dynamic` 槽上的 `[]=`（`DynamicInvocation`）拒绝。通用：动态槽分派（`_dynamicSlotCall`）加 `[]=`——Map 候选臂在副本上 `insert` 再把副本装回槽；槽的类型来源除 `StaticSet` 外，经**只转存参数的 setter** 的存入也算（`set dateTimeSymbols(v) { _dateTimeSymbols = v; }`）；分派的实参按共享克隆。顺带：开放类实例的 `runtimeType` 报 Dart 类名（`IrClass.dartName`）。夹具 dynslot（含跨库 dynamic 槽、getter/setter 转发）与 dart 一致。 | ws587 量；run587 看下一站。 |
| ws586 | 链：stub **743**（持平）。 | |
| ws587 | 链：stub **743**（持平），拒绝 260。 | |
| run587 | 元素树名字不带 `Impl` 了。第一处 panic：`[]=` 落到 `UninitializedLocaleData` 臂——`initializeDateSymbols` 的 `dateTimeSymbols is UninitializedLocaleData<dynamic>` 一直是 false：`downcast_ref::<UninitializedLocaleData<Rc<dyn Object>>>` 对不上槽里的 `UninitializedLocaleData<DateSymbols>`。通用：对翻译出的泛型 struct 做 `is C<dynamic>`（实参全是 top 类型）按运行时类型名判（`dart_is_kind`，和 prelude 集合一样）。顺带：动态槽 `[]` 的结果是 `dynamic`（`dart_option_object`：值装箱、None 是 `Null` 对象），常量实例带类型实参拼 `C::<T> { .. }`（`PhantomData` 推不出）。夹具 isgeneric 与 dart 一致。 | ws588 量；run588 看下一站。 |
| ws588 | 链：stub **745**（+2）：常量实例拼类型实参过头——`const DeepCollectionEquality()` 是 `DefaultEquality<Never>`，`Never` 是 Dart 的"随便"，该留给槽推断；`ContextMenuButtonItem.hashCode`：`object_hash` 要 `Debug`，闭包没有。两处通用修：`Never` 实参不拼；`Object.hash/hashAll` 改走 Object 协议的 `dart_hash_any`（原来按 Debug 文本 hash，和 `==` 不一致）。 | |
| ws589 | 链：stub **737**（-8：一批 `hashCode`），拒绝 260；+2 `hashCode`：`Object.hash(.., x == null ? null : hashAll(x!), ..)` 第二臂被 TFA 删掉后 `Some(!)` 推不出 `T`——条件式有发散臂时按 IR 类型拼成带类型的 `let`。 | |
| run589 | 第一处 panic：`Locale.toString` 的 `identical(_cachedLocale, this)`——静态 `Locale?` 对 `this`，拒绝。通用：`identical` 一侧是可空句柄时，`match` 出来按两个句柄比（`dart_identical_any`），None 不同；规则排在"槽对槽"前面。夹具 identstatic 与 dart 一致。 | ws590 量；run590 看下一站。 |
| ws590 | 链：stub **739**（+2：`RenderEditablePainter.shouldRepaint`——可空句柄 `identical` 的 match 按值把参数移走了，改成 `match &x`），拒绝 260。 | |
| run590 | `Locale.toString` 过了；下一站 intl `DateFormat.localeExists` 的 stub：动态槽分派里 `UninitializedLocaleData.containsKey` 臂是翻译方法（`Result`），Map 臂是 `bool`——翻译类的臂标 `fails`。夹具 identstatic 加了 `shouldRepaint` 形状，揪出两件：`Painter? old` 提升成 `Caret` 的读没先脱 `Option`；`_nullChecked` 造的 `IrNullCheck` 没带类型，后端不知道它是句柄而问了 `Rc` 自己的 `Any`。 | ws591 量；run591 看下一站。 |
| ws591 | 链：stub **736**（-1 `localeExists`），拒绝 253。 | |
| run591 | 下一站 intl `verifiedLocale` 的 stub：函数值列表 `[canonicalizedLocale, languageRegionOnlyLocale, .., (s) => ..]` 走 push 形式（元素里有 `?`）时 `Vec::new()` 的元素类型被第一个闭包定死。修：push 形式带上元素类型 `let mut __v: Vec<T>`。夹具 fnlist 与 dart 一致。 | ws592 量；run592 看下一站。 |
| ws592 | 链：stub **735**（-1），拒绝 253。 | |
| run592 | `verifiedLocale` 还是 stub，换了错：列表里的闭包 `(locale) => deprecatedLocale(canonicalizedLocale(locale))`——`locale` 被槽重定型成 `String`（`_retype`），但读它时静态类型仍是 `dynamic`，实参 coerce 走了 `dynamic → String?`。修：`_staticType`/局部读认 `_retyped`。夹具 fnlist 加了这形状，与 dart 一致。 | ws593 量；run593 看下一站。 |
| ws593 | 链：stub **732**（-3），拒绝 253。 | |
| run593 | `verifiedLocale` 过了；下一站 `DateFormat._availableSkeletons` 的 stub：动态槽分派里 `UninitializedLocaleData` 自己的 `[]` 臂是翻译方法，`Some(Result<..>)` 没 `?`——同 containsKey 标 `fails`。夹具 isgeneric 加了类自带 `[]`/`containsKey` 的臂，与 dart 一致。 | ws594 量；run594 看下一站。 |
| ws594 | 链：stub **730**（-2），拒绝 253。 | |
| run594 | `_availableSkeletons` 译出来了，运行时 `dart_cast_map::<dyn Object, dyn Object>` 转不了 `Map<String, String>`（只认几种来源形状）→ None。通用：`Map`/`Vec` 的 `DartAny::dart_cast` 能给出**全 dynamic 形式**（每个条目装箱），`dart_cast_map`/`dart_cast_list` 最后从它转（`from_dynamic`），来源形状不用枚举。夹具 dyncastmap 与 dart 一致。 | ws595 量；run595 看下一站。 |
| ws595 | 链断了：stub 60、unstubbable 1、可达 32——`Vec<T>: DartAny` 要 `T: Clone` 后，trait 声明的类型参数没带 `Clone`（`_UnorderedEquality<E>: Equality<Vec<E>>`）整个 crate 编不过。修：trait 的类型参数 bound 也加 `Clone`（struct/impl 早就有）。 | ws596 量。 |
| ws596 | 链：stub **730**（回到 ws594），拒绝 253，63 可达。 | |
| run596 | `_availableSkeletons` 过了；下一站 `DateFormat.addPattern`：`_appendPattern(_availableSkeletons[inputPattern], ..)` 的隐式 `as String` 在 `Option<Rc<dyn Object>>` 上直接问 `as_any` → None。通用：`_asAny` 对可空类型先 `clone().unwrap()`（句柄再 `as_ref`）。夹具 dyncastmap 加 `String pat(k) => skeletons[k]`，与 dart 一致。 | ws597 量；run597 看下一站。 |
| ws597 | 链：stub **732**（+2）：`_asAny` 的 unwrap 打到了投影的 `T?`（不是 `Option`）和提升读（记录类型是声明的可空、值已脱壳）。收窄为**朴素读**（局部/字段/静态/下标/`!map_get`/clone）且非投影。 | ws598 量。 |
| ws598 | 链：stub **731**（+1 `CupertinoTextField.build`：clone 的目标是提升读——clone 的朴素性也看它的目标），拒绝 253。 | |
| run598 | intl 的 DateFormat 整条过了；下一站 `LocaleNamesLocalizationsDelegate.load`（flutter_localized_locales）的 stub：trait 转发器对 **async** 方法跳过了结果整形——`Future<LocaleNames>`（值 struct）对 trait 擦除后的 `Future<Rc<dyn Object>>`。修：async 也走 `coerceInto`（同类型时原样，不同则 `.map(|v| dart_boxed(v))`）。夹具 asyncfwd 编译通过。 | ws599 量；run599 看下一站。 |
| ws599 | 链：stub **727**（-4），拒绝 253。 | |
| run599 | **渲染树六个节点全部到齐、顺序一致**（只差 `size=`——还没 layout）。元素树进到 `Localizations` 下面。第一处 panic 在 future 里：`LocaleNamesLocalizationsDelegate._loadJSON` 的 `jsonDecode`（dart:convert 顶层）拒绝——进 `_coreTopLevel` 表（prelude `json_decode` = `JsonCodec.decode`）。夹具 jsondec 揪出 `as List<dynamic>` 对 map 读的 `Option` 没脱壳（`dart_cast_list(&Option)`）——`_optionRead` 统一给 `_asAny` 和集合 cast 用。 | ws600 量；run600 看下一站。 |
| ws600 | 链：stub **726**（-1），拒绝 252。 | |
| run600 | `jsonDecode` 过了；下一站 `CachingAssetBundle.loadStructuredData` 的 stub：`loadString(key).then<T>(parser)` 的适配器参数拼成了 `T`——`_genericSlotIr` 只替换方法自己的类型参数，`Future<String>` 的类参数 `T` 留着，和调用方自己的 `T` 撞名。修：先按接收者类型把类的参数代入。夹具 thenfwd 编译通过。 | ws601 量；run601 看下一站。 |
| ws601 | 链：stub **722**（-4），拒绝 252。 | |
| run601 | `loadStructuredData` 过了编译，但程序**挂住**（timeout 124，无 panic）：打印 `Locale Instance of 'StringBuffer' is not an available locale. Falling back to 'en'`，请求 `flutter/assets` 的 `packages/flutter_localized_locales/data/en.json` 回 none，之后再无动作。三处：(1) `StringBuffer` 在 `dart_any_named!` 名单里，`toString()` 打成 `Instance of`——给它自己的 `DartAny`，`dart_to_string` 返回文本；(2) `loadStructuredData` 的 `completer` 被 `then` 回调捕获、却在闭包**之后**才赋值——`_CapturedWrites` 只把闭包内写的局部做 cell；扩为"被闭包读到 + 在本函数里声明之后有赋值"也做 cell（夹具 capcompl：`counted()` 从 0 变 5，与 Dart 同）；(3) 周期定时器让 `run_main` 永不空闲，被外面 `timeout` 杀掉就没有报告和渲染树——加 `DART2RUST_RUN_SECONDS`（run_main.sh 默认 60）：预算用完就报告还活着的定时器并 dump；`DART2RUST_TRACE_TIMERS=1` 看每个定时器的装载。 | ws602 量；run602 看下一站。 |
| ws602 | 链：stub **730**（+8），拒绝 252。八个全是同一形：`Widget child; child = ..; builder: (_) => child`——赋值在闭包**之前**，新规则也把它做了 cell，而 `dyn Widget`/`Offset` 没有 `Default` 起手。规则改为按源码顺序：闭包里读到之后、本函数里还有赋值的才做 cell（夹具 capcompl 加 `before()`，`b` 仍是普通局部）。 | ws603 量。 |
| ws603 | 链：stub **722**（=ws601），拒绝 252。 | |
| run603 | Locale 那句没了。但预算 60s 用完时 `main done, 0 timer(s)`——不是周期定时器，是 `run_until_idle` 把"轮询了 pending task"当成有进展，`run_main` 空转（run601 挂住的真因）。改为只有 task 完成才算 worked → 之后 run_main 会报"main is waiting on …"。同时 `flutter/assets` 一直回 none：runtime 加资产服务，`DART2RUST_ASSETS` 指向 `flutter build` 的 `flutter_assets`（run_main.sh 默认 `~/gallery_upstream/build/flutter_assets`），key 即相对路径，回文件字节。 | run604。 |
| run604 | 资产回得来了，路径走到 google_fonts：`_findFamilyWithVariantAssetPath` stub——`['.ttf', '.otf'].where(asset.endsWith)`：`filter` 给闭包 `&&T`，撕下的方法把参数裸传 → `dart_ends_with(&&String)`。`_stepClosure` 对 `filter` 步先把项 clone 出来（`let other = (*__p_other).clone()`），闭包体和 `map` 步看到的一样。夹具 wheretear（撕下 / lambda / 自定义类撕下）SAME。 | ws604 量；run605 看下一站。 |
| ws604 | 链：stub **720**（-2：`_findFamilyWithVariantAssetPath`、`FocusTraversalPolicy._findInitialFocus`），拒绝 252。 | |
| run605 | google_fonts 的 `isTest` 拒绝：`Platform.environment` 是 static **getter**，走 `IrStaticCall`，而 `_staticRead` 的 Platform 规则只接字段读。静态调用处同一条规则（`platform_<name>()`），prelude 加 `platform_environment()`（进程环境 → `Map<String, String>`）。夹具 platenv SAME。 | ws605 量；run606 看下一站。 |
| ws605 | 链：stub **726**（+6），拒绝 **244**（-8）：解禁的 `Platform` getter 露出 prelude 没有的六个（`executable`/`resolvedExecutable`/`script`/`executableArguments`/`packageConfig`/`localeName`，package platform 的 `LocalPlatform`）。prelude 补齐（进程自身路径、空 VM 参数、无 package config、`LC_ALL`/`LANG` 的 locale）。 | ws606 量。 |
| ws606 | 链：stub **720**（=ws604），拒绝 244。 | |
| run606 | 无 panic、退出 0、3 帧，但元素树仍停在 `Localizations → SizedBox`，google_fonts 六个字体都"unable to load"。`DART2RUST_TRACE_MESSAGES` 看到资产 key 是 `/flutter_localized_locales/data/en.json`、`/flutter_gallery_assets/fonts/…`——少了 `packages/` 且多了 `/`：prelude `Uri.path` 没有 authority 时把第一段当 host 吃掉了。修 `path()`（无 `://` 时整个文本，去掉 `scheme:`）。顺带 `Uri.toString()` 打 `Instance of 'Uri'`：`dart_any_named!` 不接 `Display`——加 `dart_any_display!`，`Uri`/`Duration`/错误类等 13 个有 `Display` 的类型迁过去。夹具 uripath SAME。 | run607。 |
| ws607 | 链：stub **720**，拒绝 244（prelude 改动无回归）。 | |
| run607 | 资产找到了；下一站 dart:ui `loadFontFromList` 的 stub：`.then((_) => _sendFontChangeMessage())`，被调是 `FutureOr<void> f() async`——async 函数返回的是 `Future<flatten(R)>`，这里却按声明给了 `FutureOr`（Rust `DartFuture<Option<FutureOr<()>>>` 进 `Option<FutureOr<()>>` 槽）。前端：async 成员声明 `FutureOr<T>`/`Future<T>?` 的调用结果一律 `Future<T>`（`_spawnedFuture`，静态与实例两处）；后端 `_awaited` 也把 `FutureOr` 拍平。夹具 asyncfutor 编译通过。 | ws608 量；run608 看下一站。 |
| ws608 | 链：stub **719**（-1 `loadFontFromList`），拒绝 244。 | |
| run608 | 下一站 dart:ui `_futurize` 的 stub：`completer.completeError(Exception('operation failed'))`——prelude 被调的槽只在提到 `dynamic`/类型参数时按类型 coerce，`Object` 槽走形状规则，把裸 `Exception` 结构体塞进了 `Rc<dyn Object>`。`_calleeTranslated` 的规则扩到 top 类型（`_mentionsTop`：`dynamic` 或 dart:core `Object`，两者 Rust 拼法相同）。连带 prelude `StringBuffer`：`new`/`write`/`writeln` 从 `Display` 改走 `DartAny::dart_to_string`（`Option`/handle/标量/结构体都按 Dart 打印），补 `writeAll`。夹具 complerr SAME。 | ws609 量（这条规则碰所有 prelude `Object` 槽，看涨跌）；run609 看下一站。 |
| ws609 | 链：stub **735**（+16：-13 +29），拒绝 244。涨的全是 `Object?` 槽：prelude 对 `Object?` 是泛型地接（`contains(&T)`、`AssertionError(String)`、`log(error: Option<..>)`），coerce 成 handle 反而不合。规则收窄：只有**非空** `Object`（和 `dynamic`）按类型 coerce，`Object?` 仍走形状。 | ws610 量。 |
| ws610 | 链：stub **711**（-8 vs ws608，无新增），拒绝 244。 | |
| run610 | **Localizations 加载完成了**，元素树往下建：`_AnimatedThemeState.initState → createTicker → TickerMode.getValuesNotifier` 的 stub：`widget?.notifier ?? fallback`——左边 `ValueNotifier<..>?`，右边只实现 `ValueListenable`，整体静态类型是 LUB `ValueListenable`；lowering 一直是"右边进左边的类型"，fallback 没有 `ValueNotifier` 可变。改为两边都进整体的静态类型（左边经 `Option` map 上转）。夹具 ifnulllub SAME。 | ws611 量；run611 看下一站。 |
| ws611 | 链：stub **715**（+4：-2 +6），拒绝 244。六个新的全是 `double? ?? 0`：Dart 里整体是 `num`，按 LUB 走就丢了字面量的 `0.0` 拼法。LUB 规则排除标量结果（`scalarNames`），标量仍按左边。 | ws612 量。 |
| ws612 | 链：stub **709**（-2 vs ws610，无新增），拒绝 244。 | |
| run612 | `getValuesNotifier` 过了；下一站 `ImplicitlyAnimatedWidgetState._constructTweens` 的 stub：`tween.end ??= tween.begin`，CFE 拼成 `let #t = tween in #t.end == null ? #t.end = .. : null`，两臂一个是存值的 `Option<Rc<dyn Object>>`、一个是 `Null` 对象，作为表达式类型不合。语句位置的条件表达式（值被丢弃）改译成 `if`（`_conditionalStatement`，穿过 CFE 的 `Let`；`?.`/`??` 形仍归 `_let`）。夹具 condstmt SAME。 | ws613 量；run613 看下一站。 |
| ws613 | 链：stub **723**（+14），拒绝 244。十三个是 CFE 的模式缓存 `#isSet ? #t : (#isSet = true, #t = ..)` 作语句：`#t` 臂作为语句是 `__t8;`，把临时量 move 掉了；一个是 `_callPopInvoked` 的 TFA 死尾巴（条件是 throw）。改：纯读的臂（读、字面量、`this`）当空臂，空 then 变成取反的 `if`；带 throw 的仍走表达式形。 | ws614 量。 |
| ws614 | 链：stub **709**（=ws612），拒绝 244。`_constructTweens` 过了 `??=`，卡在下一处：`targetValue != (tween.end ?? tween.begin)`——`Tween<dynamic>` 的 `end`/`begin` 是投影的 `T?`（`Option<Rc<dyn Object>>`），整体是 `dynamic`（不可空）：`match` 的 None 臂给了 `Option`，Some 臂给了 `Rc`。整体为 `dynamic` 而右边仍是 `Option` 时，右边经 `dart_option_object` 装成 handle（null 即 `Null` 对象）。夹具 dynifnull SAME。 | ws615 量；run615 看下一站。 |
| ws615 | 链：stub **708**（-1 `_constructTweens`），拒绝 244。 | |
| run615 | 下一站运行时 `todo!`：`ThemeDataTween.end is written through a trait but is not a cell`——trait 里可变字段只在 trait 自己的体里写到 `this` 时才做 cell（ws480 全做过、翻车）；`tween.end ??= ..` 是在 `_constructTweens` 里经 `Tween<dynamic>` 句柄写的，setter 落到每个实现者。加"经句柄的写"：`_WalkSelf` 记下 `IrSetter` 的接收者类与字段（按类对象记忆一次），`_fieldsWrittenBy(trait)` 并入全程序里对该 trait（或其子类）句柄的写。顺带插值里 `int?`/`bool?` 改走 Object 协议（擦除存储的 `Tween<int>.end` 打成了 `Instance of 'int'`）。夹具 traitset SAME。 | ws616 量；run616 看下一站。 |
| ws616 | 链：stub **708**（持平），拒绝 244。 | |
| run616 | 还是同一个 `todo!`：gallery 里那笔写不是 `IrSetter`，是经 CFE 临时量的 `IrAssignField(owner: 'Tween')`（夹具里参数写才是 setter 形）。`_WalkSelf` 对非 `this` 的 `IrAssignField`（按 `owner`）和 `IrSetValue`（按接收者类型）也记入 `setterWrites`。夹具 traitset 加 gallery 同形（闭包参数重赋值后经临时量写），SAME，且生成里无 `todo!`。 | ws617 量；run617 看下一站。 |
| ws617 | **链断了**：stub 464、unstubbable 1、可达 34——`_AnimatedPhysicalModelState._borderRadius` 进了 `Cell<Option<BorderRadiusTween>>`，而 `BorderRadiusTween` 的 `begin`/`end` 刚成了 cell（`Rc`，不 `Copy`）：`_classIsCopy` 只问 shared 和计数类的可变字段，没问 `_inCellOf` 的另外两条（trait 交出的 / 经 trait 写的）。改为按 `_inCellOf`。夹具 traitset 加 holder 形，SAME。 | ws618 量。 |
| ws618 | 链：stub **707**（-1），拒绝 244，63 可达；`ThemeDataTween.end` 的 `todo!` 没了。 | |
| run618 | AnimatedTheme 过了；下一站 `CupertinoThemeData.noDefault` 的 stub：`super.primaryColor`——基类 `NoDefaultCupertinoThemeData` 的**字段**，本类用 getter 覆盖；`super.x` 对字段一直译成 `this.x`（结构体里同一存储），但在 trait 体里 `this.x` 是本 trait 的访问器 = 覆盖的 getter（非空），槽要的是基类的 `Color?`。前端在节点上记字段的类（`owner`），后端在 trait 体里改问基 trait 的访问器 `Base::x(this_)`。夹具 superfield SAME。 | ws619 量；run619 看下一站。 |
| ws619 | 链：stub **696**（-11：CupertinoThemeData 全部 super fn），拒绝 244。 | |
| run619 | `noDefault` 过了；下一站运行时 `unwrap on None`：`CupertinoDynamicColor.maybeResolve(Color? resolvable, ..)` 的 `resolvable is CupertinoDynamicColor`——`_isTest` 的 `_asAny` 把 `Option` 直接 unwrap。对可空操作数（非投影）且目标非空的 `is`：`match &x { Some(__v) => 测试(__v), None => false }`（取反则 true）。夹具 isnull SAME。 | ws620 量；run620 看下一站。 |
| ws620 | 链：stub **698**（+2）：`CupertinoTextField.build` 的 `border is Border` 是提升过的读（记录类型可空、值已脱壳），`_maybeAddKey` 里 `match &key` 借用活不过 `dart_is_kind` 的 `'static`。只对 `_optionRead` 认得的普通 `Option` 读包 `match`，且按值 match（`.clone()`）。夹具 isnull 加提升读与 `Key?` 句柄两形。 | ws621 量。 |
| ws621 | 链：stub **696**（=ws619），拒绝 244。 | |
| run621 | `maybeResolve` 过了；下一站同文件 `resolve(Color resolvable, ..)`（非空）：`is` 说是、提升读却 `unwrap on None`——提升读的 `IrLocal` 没带 rustType，后端 `_asAny` 不知道它是句柄，问了 `Rc` 自己的 `Any`（`resolvable.as_any()` 而非 `.as_ref().as_any()`）。提升读的局部一律带声明类型。夹具 isnull 加非空句柄提升，SAME。 | ws622 量；run622 看下一站。 |
| ws622 | 链：stub **696**（持平），拒绝 244。 | |
| run622 | `resolve` 过了；下一站 `CupertinoDynamicColor.resolveFrom` 的 stub（E0282）：记录模式的 switch，CFE 的缓存临时量 `#0#15 = block{..}`，TFA 把块里的读换成了 throw，块的值成了 `!`；赋值当值用时前端把值先 hold 进 `let mut __t = ..`，没类型，rustc 推不出。hold 的临时量在值类型为 `Never`/未知时按 Dart 静态类型标注。夹具 patcache（记录模式 + 枚举元组 switch）SAME。 | ws623 量；run623 看下一站。 |
| ws623 | 链：stub **681**（-15，全是记录模式 switch 的缓存形），拒绝 244。 | |
| run623 | `resolveFrom` 过了；下一站运行时 `unwrap on None`：`MediaQuery._of` → `InheritedModel.inheritFrom<MediaQuery>` → `_findModels<T>` 里 `context.getElementForInheritedWidgetOfExactType<T>()` 走了 `__erased` 孪生——`T` 成了 `Rc<dyn Object>`，体内 `_inheritedElements[T]` 查的是 `dart_type_of::<Rc<dyn Object>>()`。`_genericOnTrait` 没接手是因为 provider 的 `_InheritedProviderScopeElement` 也有一份体（两份 → 放弃）。通用机制：**被当作类型字面量用的方法类型参数以 `Type` 值随隐藏尾参 `__ty_<i>` 传递**（Dart 运行时本来就这么传类型实参）：按方法族（最顶层声明 + 所有覆盖）算观察到的下标；体内字面量读 `__ty_i`，闭包按局部捕获；调用点补 `_typeLiteral(实参)`；trait 声明/孪生/转发器随 `IrParam` 自然带上。顺带：`_genericBodies` 按 `_translatedClass` 而非 `package:` 前缀；`super.m<T>()` 的返回按投影边类型；`dart_cast_any` 接受结构体答的 `Rc<T>`；值结构体的 `dart_cast` 也答 `Object`（擦除孪生里的 `as T`）。夹具 gentrait（trait 泛型方法 + 结构体覆盖 + 闭包里的 `T`）SAME。 | ws624 量；run624 看下一站。 |
| ws624 | 链：stub **681**（持平），拒绝 244，可达 **64**。 | |
| run624 | **MediaQuery 过了**（`__erased(Type::of("HeroControllerScope"))` 找到了元素）；渲染树 8 个节点。下一站 `NavigatorState.initState` 的 `unwrap on None`：`?.widget as HeroControllerScope?`——可空转可空的 `as` 里 `IrBound()` 没类型，`_asAny` 问的是 `Rc` 自己的 `Any`。bound 按操作数的非空记录类型标注。夹具 asnull SAME。 | ws625 量；run625 看下一站。 |
| ws625 | 链：stub **681**（持平），拒绝 244，可达 64。 | |
| run625 | `initState` 的 `as` 过了；下一站拒绝 `List.+`（`widget.observers + <NavigatorObserver>[..]`，`NavigatorState._updateEffectiveObservers`）。表里加 `'+': 'dart_concat'`，prelude `DartList` 加 `dart_concat`。夹具 listplus SAME。 | ws626 量；run626 看下一站。 |
| ws626 | 链：stub **681**（持平），拒绝 **242**（-2），可达 64。 | |
| run626 | `List.+` 过了；下一站 `NavigatorState.restoreState` 的 stub：`registerForRestoration(_rawNextPagelessRestorationScopeId, 'id')`——`RestorableNum<int>` 进 `RestorableProperty<Object?>` 槽，Dart 泛型协变，Rust 里 `RestorableProperty<i64>` 和 `RestorableProperty<Rc<dyn Object>>` 是两个 trait。已有机制 `extraImpls`（按闭世界普查给更宽实例化各写一份 impl）只做非泛型类（泛型类的宽 impl 会和自己的 `impl<T>` 重叠 E0119）。扩：泛型类按程序里点名的**具体实例化**各写一份（`impl RestorableProperty<Rc<dyn Object>> for RestorableNum<i64>`，不重叠）——普查也记具体泛型类的实例化；`IrClass.extraImplSelf` 记自身实参；后端 `_selfBinding` 让类自己的参数在该 impl 里拼成实参；cell 的种类仍按声明拼法决定（`_heldDecl`）；`dart_cast` 里按 `TypeId` 判定自身实例化后再答。夹具 restoreprop SAME。 | ws627 量；run627 看下一站。 |
| ws627 | 链：stub **715**（+34：-8 +42）。新宽 impl 的连带：(1) 转发体 `ConstantTween::lerp(self, t)` 让 rustc 从 trait 返回类型推出 `T = Option<f64>`——按 `_selfBinding` 拼 `ConstantTween::<f64>::lerp`；(2) 转发时 own 侧的 `T` 没按实例化代入就送进 coerce（裸 `<T as DartNullable>::option`、`dart_from_dynamic::<Orientation>` 丢了 `?`）——`_selfBound` 先代入；(3) 全擦除的泛型类拼成了 `MapEquality<>`——空实参当自身；(4) `MapEquality.equals(Map? e1, ..)` 自己有更宽签名的 inherent 方法，`_wideTraitFor` 却把调用改走 trait 签名——类自身有同名 inherent 方法时不限定；(5) `RestorableProperty::dispose(&*x)` 在多份 impl 下 E0283——按接收者自身实例化限定 `<RestorableEnumN<Orientation> as RestorableProperty<Option<Orientation>>>`。留：`RestorableRouteFuture<TimeOfDay?>` 的宽 impl 因 Dart 可见性（widgets 看不见 material）没写，demo 的 `restoreState` 仍是 stub（不在启动路径）。夹具 restoreprop/gentrait SAME。 | ws628 量。 |
| ws628 | 链：stub **677**（-4 vs ws626：-9 +6），拒绝 242。run628 仍停在 `restoreState`：实参是 trait 句柄 `Rc<dyn RestorableNum<i64>>`，coerce 对 trait→超 trait 一律隐式上转（Rust 只能同实参上转）——带实参的改走 `dart_cast_to`（对象经宽 impl 作答）。连带：开放泛型类自己的结构体（`NumValImpl`）也按 Dart 类名领宽 impl；继承来的方法（`via`）的类型先换成本类的实参（`RestorableValue<T?>` 的 `T`）；泛型接收者按记录类型的实参限定 `<SetEquality<..> as Equality<..>>`。夹具 restoreprop（值/句柄/子类三形）、gentrait SAME。 | ws629 量；run629 看下一站。 |
| ws629 | 链：stub **658**（-19），拒绝 242。`restoreState` 过了编译；run629 停在 `_HistoryProperty` 宽 impl 的 `initWithValue`：转发把 `Rc<dyn Object>` 送进 `Map<String?, List<Object>>?` 只做了 `dart_nullable`——coerce 对 `dynamic → Map/List` 一直放行不转，补 `IrDowncast`（`dart_cast_map`）。新增 8 个 stub 是 `dart_cast_to::<dyn ValueListenable<T>>` 里 `T` 是被调方的类型参数（无 `TypeId`）——带实参的 trait cast 只在实参全是已知类型时走对象，否则仍隐式上转；`DiagnosticsProperty<void>` 的宽 impl 不写。顺带：语句开头的块表达式加括号（`{..}[i].m()` 被 Rust 拆成两句）。**记债**：(a) `ui.TextStyle` 在 painting 模块里拼成本模块的 `TextStyle`（同名类跨模块碰撞，`EditableText.build` 因此 stub，不在启动路径）；(b) 子类把 `T` 绑成可空（`Val<Map?>`）时 `super(v)` 进 `T` 槽少了 `Some`。夹具 restoreprop 加 map 属性经宽句柄 `initWithValue`，SAME。 | ws630 量；run630 看下一站。 |
| ws630 | 链：stub **627**（-31），拒绝 242。 | |
| run630 | `restoreState` 过了；下一站拒绝 `List.removeWhere`（`Navigator.defaultGenerateInitialRoutes`）。表里加 `removeWhere`/`retainWhere`，prelude `DartList` 加 `remove_where`/`retain_where`（`impl Fn`，Result 经 `_preludeFailing` 传出）。**记债**：值结构体局部的集合字段就地变异（`h.routes.addAll(..)`）落在 clone 上（读字段即拷贝），夹具里改经 `this` 的方法避开。夹具 removewhere SAME。 | ws631 量；run631 看下一站。 |
| ws631 | 链：stub **629**（+2，解禁露出：`LinkedHashMap.fromEntries` 缺、`showOnScreen` 回调槽 0 参对 4 参），拒绝 **232**（-10）。 | |
| run631 | `removeWhere` 过了；进到 gallery 自己的 `RouteConfiguration.onGenerateRoute`：`RegExpMatch.groupCount` 缺——而 prelude 的 `RegExp` 根本**没有引擎**（`hasMatch` panic、`firstMatch` 恒 null；设计上不引 crate）。在 prelude 里写了一个回溯正则引擎：字面量/转义（`\d \w \s \b` 及取反）、`.`、字符类（区间/取反）、`^ $`（multiLine）、捕获/非捕获/命名组、`|`、`* + ? {m,n}` 贪婪与懒惰，`caseSensitive`/`dotAll`；`RegExpMatch` 带各组区间：`group`/`groupCount`/`namedGroup`/`[]`（`index_of`）/`start`/`end`；`allMatches(input, [start])`、`matchAsPrefix`、`RegExp.escape`。夹具 regex1（21 例含 gallery 路由模式）与 Dart 全同。 | ws632 量；run632 看下一站。 |
| ws632 | 链：stub **627**（-2），拒绝 232。 | |
| run632 | 路由正则过了；下一站运行时 `erased_cast_failed`：`result.cast<Route>()`（`List<Route?>` → `List<Route>`，`defaultGenerateInitialRoutes`）——prelude 的 `cast_to` 把元素 `Rc::new(v.clone())` 装箱，`Option<Rc<dyn Route>>` 装成了 `Rc<Option<..>>`，再 `from_dynamic::<Rc<dyn Route>>` 认不出。改走 Object 协议 `dart_boxed`：`Option` 的 `dart_cast` 对 `None` 答 `Null` 对象；标量（i64/f64/bool/String）的 `dart_cast` 也答 `Object`（否则 `Option<Rc<dyn Object>>` 里的标量还是装成 `Option`）。夹具 listcast（List/Map/Set 的 cast）SAME。 | ws633 量；run633 看下一站。 |
| ws633 | 链：stub **627**（持平），拒绝 232。 | |
| run633 | `cast<Route>()` 过了；下一站 `RestorableValue.value=` 的 stub：`final T? oldValue = _value; didUpdateValue(oldValue)`——体内 `Option<T>` 进边上的投影 `<T as DartNullable>::Or` 没做 `from_option`：`this` 上的调用没绑定本类的类型参数（`this` 的静态类型不在节点上，且 `didUpdateValue` 是抽象方法、`getDispatchTarget` 无目标）。修：`this` 按当前类定型；抽象目标以接口成员为落点。顺带 `CastErased<Option<T>>` 里 `Some(Null 对象)` 视为 null。夹具 projarg SAME；gentrait/listcast/restoreprop 仍 SAME。 | ws634 量；run634 看下一站。 |
| ws634 | 链：stub **623**（-4），拒绝 232。 | |
| run634 | `value=` 过了，`MaterialPageRoute.didAdd` 起动画；下一站拒绝 `List.generate` 3 参（`growable:`，`HashedObserverList.toList`）——接受第三参。夹具 listgen 顺带揪出三处：`Iterator` 局部被闭包捕获后 `..moveNext()`——prelude 的 `DartIter` 不共享游标也不是 `Iterator<T>` 的拼法，改 `dart_iter` 返回 `xs.iterator` 同款 `Rc<dyn DartIterator<T>>`；`=> _map[v] = ..` 在 void 函数里走了值形（写进 clone）——void `return e` 改按语句译；CFE 把 `this._map` 绑进临时量再 `[]=`——`this.field` 也算可别名的 place。夹具 listgen/latecell/nullmut/statmut SAME。 | ws635 量；run635 看下一站。 |
| ws635 | 链：stub **623**（持平），拒绝 **231**（-1）。 | |
| run635 | `List.generate` 过了；下一站运行时 `dart_from_dynamic::<Rc<dyn Fn(AnimationStatus)>>` panic：`AnimationController.notifyStatusListeners` 从 `ObserverList<T>`（`T` 擦除，存的是 `DartFunction` 对象）取回监听器，`Rc<T>` 的 `FromDynamic` 只认句柄/对象，不认 `Function` 对象。加一条：`DartFunction` 里存着它由之而来的那个类型化闭包（`original`），同型就原样取回（`removeListener` 也因此找得到）。夹具 fnback（泛型观察者列表存/取/删类型化监听器）SAME。 | ws636 量；run636 看下一站。 |
| ws636 | 链：stub **623**（持平），拒绝 231。 | |
| run636 | 状态监听器回得来了；下一站 `_flushRouteAnnouncement` 里 `next?.route != entry.lastAnnouncedNextRoute` `unwrap on None`：`Route?` 对 `_RoutePlaceholder?`（`Route extends _RoutePlaceholder`），`==` 一律把右边 coerce 进左边的类型 → 对占位符对象做 `dart_cast_to::<dyn Route>` 失败。改：类型不同时**低的一边升到高的一边**（前端按 `isSubInterfaceOf`），互不相干的两个 trait 留给后端按对象比（`dart_option_object(..).dart_eq`）。夹具 eqabove SAME；identmap/identstatic 仍 SAME。 | ws637 量；run637 看下一站。 |
| ws637 | 链：stub **622**（-1），拒绝 231。 | |
| run637 | 路由通告过了，**Navigator 把 Overlay 建起来了**（渲染树 10 个节点：多了 `RenderPointerListener`、`RenderAbsorbPointer`）；下一站拒绝 `List.insertAll`（`OverlayState.insertAll`）——表里加 `insertAll`，prelude `DartList` 加 `insert_all`。夹具 insertall SAME。 | ws638 量；run638 看下一站。 |
| ws638 | 链：stub **623**（+1 `OverlayState.rearrange`，解禁露出：`Iterable<T>` 提升成 `List<T>` 被当成 trait cast `dyn List<..>`），拒绝 **229**（-2）。修：prelude 的集合彼此不是 trait，`Iterable`→`List` 就是同一个 `Vec`。 | |
| run638 | `insertAll` 过了，`_RenderTheater` 进树（渲染树 11 节点）；下一站 `_RenderTheater.attach` 的 stub：`Iterator<RenderBox>? iterator = childParentData.paintOrderIterator`，getter 记录的类型是 TFA 收窄的 `Iterator<_RenderDeferredLayoutBox>`，Rust 的 `DartIterator<T>` 不协变。加通用机制：`Iterator<A>` 进 `Iterator<B>` 槽走 prelude 的 `dart_iterator_map`（同一游标，元素按 coerce 规则逐个转），coerce 加规则、IR 复用 `IrMapElements('Iterator')`。夹具 itermap SAME。 | ws639 量；run639 看下一站。 |
| ws639 | 链：stub **622**（-1 `attach`），拒绝 229；`rearrange` 仍在——那个 `dyn List` cast 来自提升读（`newEntries is List ? newEntries : ..`），不是 coerce：提升读的 trait cast 也只对**翻译过的**类做。 | |
| run639 | `_RenderTheater.attach` 过了，**路由的 ModalBarrier 开始 build**；下一站 `ModalBarrier.build` 的 stub：局部函数 `handleDismiss` 传给 `_ModalBarrierGestureDetector(onDismiss: ..)` 时给的是借用 `&*f`——`_keeps` 只看构造函数的 body，`this.onDismiss` 的存储在 initializer 里，没算"留下"。构造函数的 initializers 也算。顺带：dump 时 `visitChildrenOfOverlayEntry` 的 `value!` 在半建的 entry 上 panic，把 panic hook 里的报告一起带走（Dart 里同样会抛；记为 runtime 报告的健壮性债）。夹具 localfn/itermap SAME。 | ws640 量；run640 看下一站。 |
| ws640 | 链：stub **619**（-3），拒绝 229。 | |
| run640 | ModalBarrier 过了，路由的 `_ModalScope` 开始 inflate；下一站 `todo!("_ModalScope has no handle of its own")`——泛型值类在 trait impl 里不给句柄（ws275 时 impl 泛型没带 `Clone`，如今 `_implGenerics` 早带了 `T: Clone`），改为一律 `Rc::new(self.clone())`。夹具 genhandle SAME。 | ws641 量；run641 看下一站。 |
| ws641 | 链：stub **619**（持平），拒绝 229；`rearrange` 仍在——真因是我 ws638 的集合捷径把 `Set` 也当成 `Vec`（`insertAll(index, Set)`），改成只在两边同为 `Vec` 类（`List`/`Iterable`）或同为 `Set` 时放行。 | |
| run641 | `_ModalScope` 有句柄了；下一站**栈溢出**（gdb 跑子进程看到）：`FocusManager == FocusManager` 走字段逐一比较，`FocusManager → rootScope → manager → …` 环形对象图无限递归。Dart 里没有重写 `operator ==` 的类是**恒等**比较：计数类且无 `==` 的，`PartialEq`/`DartEq` 一律 `std::ptr::eq`（`hashCode` 本来就是地址）。夹具 identeq（环形图恒等、有 `==` 的值类仍按值）SAME。 | ws642 量；run642 看下一站。 |
| ws642 | 链：stub **614**（-5：五个 `eq`），拒绝 229；`rearrange` 仍在：`insertAll(index, LinkedHashSet)`——coerce 里 `Set` 进 `List`/`Iterable` 槽没有规则（`to_list()`），补上。 | |
| run642 | 栈溢出没了；下一站 `FocusNode._removeChild` 的 stub：`.forEach(nodeScope._focusedChildren.remove)`——prelude 集合方法的**撕下**直接拼 `IrCall(list, 'remove')`，绕过了 `listMethodNames`（`remove` 是 `remove_value`，不是 `Vec::remove(usize)`）。撕下改为合成 `InstanceInvocation` 走正常调用 lowering（所有集合表都生效）；`for_each` 步的闭包丢弃返回值（`void Function(T)` 槽接 `bool` 撕下）。夹具 tearcol（list.remove / set.add / map.containsKey 撕下）SAME；wheretear 仍 SAME。 | ws643 量；run643 看下一站。 |

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
- **`Layer.find<S>`**:泛型值类(`AnnotationResult<S>`)当出参经擦除孪生对不上
  (run553 记,合成路径上绕道了,机制待做)。
- **泛型 mixin 的 `T?` 字段 getter**:`Or` vs `Option<T>` 的投影差(run555 记)。
- **无 `==` 的值类做键**:Dart 按身份,值 struct 按结构;AOT 还会删未读字段
  (run558 记,不在路径上)。
- **Goal 3 差 `size=`**:渲染树类型序列已 6/6,dump 时拿不到 layout 结果。
- **727 个 stub 的长尾**:最大类是 "mismatched types"(约一半),其余是参数数、
  注解、闭包形状等;随运行尺子推进逐站收。
