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

## 活账:ws/run 表(窗口约 40 行,ws657 起;更老的在 git)

| 轮 | 第一个停点 / 读数 | 处理 |
|---|---|---|
| ws657 | 链：stub **551**（持平），拒绝 197。 | |
| run657 | **layout 跑起来了**（`RenderView.performLayout` → 子树 `layout`），停在 `RenderProxyBoxMixin.performLayout` 的 stub：`(child?..layout(..))?.size ?? ..`——`?..` 级联的值 `=>#t3` 是绑定值经 `dart_cast_to::<dyn RenderBox>`（子访问器是擦除的 `RenderObject?`，mixin 的 `T extends RenderBox`）——这个 `IrCastTo` 没带 rustType，`expression()` 用 Kernel 的 `RenderBox?` 补了个可空 → 外层 `.flatten()` 套在非 Option 上。修：绑定值的转型按自身（非空）定型。夹具 cascade（界更窄的 `T`）SAME。此刻的 render 树正好是参考尺子的 6 行（restoration `SizedBox` 时刻）。另外补了一份 **settle 后的参考**：`~/gallery_upstream/test/dump_render_walk_settled_test.dart`（pump 8×100ms）→ `~/dart2rust_build/scratch/ref_render_walk_settled.txt`，708 行。 | 链 ws658 |
| ws658 | 链：stub **549**（-2：两个 `performLayout`），拒绝 197。 | |
| run658 | **`size=` 出现了**：前 5 行与参考完全一致（`RenderSemanticsAnnotations size=Size(800.0, 600.0)`…），树 34 节点。第一个 panic：`_AnimatedPhysicalModelState.forEachTween` 里 `(dynamic v) => Tween<double>(begin: v as double)` 作为 `TweenConstructor<dynamic>` 交出去时 `dart_cast_to::<dyn Tween<Rc<dyn Object>>>()` 得 None——`TweenImpl<f64>`（open 类 `Tween` 自己的 struct）没有 `Tween<Object>` 的宽 impl：`addWiderImpls` 把「base 就是自己」一律跳过，open 类自己的 trait 也是它 struct 的一个 base。修：open 类不跳。夹具 opentween SAME（`TwImpl<f64>: Tw<Object>`、`Anim<Object>`）。 | 链 ws659 |
| ws659 | 链：stub **550**（+1：`ValueKeyImpl<i64>: ValueKey<Option<i64>>` 的字段访问器转发 `Ok(self.value)` 少了 `Some`——转发时 `__v` 按声明 `T` 定型，规则看不出 `i64`→`Option<i64>`），拒绝 197。修：转发的字段按 impl 的实例化定型（`_selfBound`）。夹具 opentween 加 `Tw<int?>` 读 `start`。 | 链 ws660 |
| ws660 | 链：stub **549**（持平，无新增），拒绝 197。 | |
| run660 | `forEachTween` 过了；4 帧，render 树 41 节点、18 个带 `size=`，前 5 行与参考尺子一致（第 6 行起是 gallery 主页而非 restoration `SizedBox`，时刻不同）。第一个 panic：gallery 自己的 `_SettingsPageState.build` stub：`SettingsListItem<ThemeMode?>(selectedOption: options.themeMode)`——构造子参数 `T selectedOption` 在 `T := ThemeMode?` 下是 `Option<ThemeMode>`，槽却拼成裸 `T`（`_genericSlotIr` 只认方法级泛型调用，不认构造实例化）。修：`_constructedSlotIr`——构造调用的参数槽按类实例化（保留参数代入，擦除的按界）拼，三条槽链都接上。夹具 ctornull SAME（含命名参数 + `LinkedHashMap.of`）。 | 链 ws661 |
| ws661 | 链：stub **541**（-8：构造槽按实例化后 `segmented_control`/`radio`/`restoration_properties` 一批过了），拒绝 197。 | |
| run661 | 同一函数 `_SettingsPageState.build` 的下一处：`SettingsListItem<Locale?>(optionsMap: _getLocaleOptions())`——`LinkedHashMap<Locale, X>` 进 `Map<Locale?, X>` 槽：coerce 的「逐键逐值」map 规则只认名字 `Map`，`LinkedHashMap` 被 `if (have == 'Map' || slot == 'Map') return value` 放行了。修：`dart:` 的各 map 类名（`mapNames`）视同 prelude 的一个 `Map`。夹具 mapwiden SAME（map 键 `Locale`→`Locale?`、`List<int>`→`List<int?>`）。 | 链 ws662 |
| ws662 | 链：stub **540**（-1），拒绝 197。 | |
| run662 | `_SettingsPageState.build` 再下一处：`LinkedHashMap.fromEntries(displayLocales)`——prelude 没有 `Map::from_entries`；夹具还揪出 `MapEntry(k, v)` 解析到 dart:core 的私有 `MapEntry._`（公共的是重定向工厂），拼成 `new_`。修（prelude 表）：`Map::from_entries(Vec<MapEntry>)`、`MapEntry::new_`。夹具 fromentries SAME。 | 链 ws663 |
| ws663 | 链：stub **538**（-2），拒绝 197。 | |
| run663 | `_getLocaleOptions` 再下一处：`Map<Locale, DisplayOption>.fromIterable(supportedLocales, value: (dynamic l) => ..)`——prelude 无 `Map::from_iterable`。修（prelude 表）：`from_iterable<E>(Vec<E>, key: Option<Rc<dyn Fn(Object)>>, value: ..)`，元素按调用方持有的列表类型收、装箱交回调，缺省 `as K`/`as V` 走 `FromDynamic`。夹具 fromiterable SAME（typed list 进 `Iterable<dynamic>`、只 value、key+value、都缺省）。 | 链 ws664 |
| ws664 | 链：stub **538**（持平），拒绝 197；settings 的 stub 挪到 `_getLocaleOptions`：`(dynamic locale) => .. locale as Locale?`——`dynamic` 上的 `as T?` 走 `<Locale as FromDynamic>`，可 `Locale` 是 counted 类，持有形式是 `Rc<Locale>`（拷了 struct，身份也丢）。修：`!as_opt`/`IrDowncast` 对 counted 类按句柄（`<Rc<T> as FromDynamic>` / `dart_cast_any::<Rc<T>>`）；顺带：所有局部读都带声明类型；持句柄的局部（`Rc<T>`/`Rc<dyn X>`）有「地址」= 指向物（`identical(b, loc)` 两个局部原按槽地址比，永远不等）。夹具 dyncast SAME。 | 链 ws665 |
| ws665 | 链：stub **537**（-1），拒绝 197。 | |
| run665 | settings 页建完；下一站 `RestorableValue<double?>.value`：`_value as T`（AOT 写成 `let #t = _value in #t == null ? #t as T : #t`，即 `x!` 的形）被当成 null 检查 `unwrap`——`T = double?` 时 null 是合法的 `T`。修（通用）：`x as T` 在参数自己的 `T?` 上（两种写法）走 prelude `dart_as_own::<T>`：里面的 `T`，或 `T` 自己的 null（`dart_null()`），都没有则是 Dart 的 TypeError（新 `TypeError` 错误类，可 catch）。夹具 asown SAME（`double?` 的 null、`int` 的抛错可 catch）。 | 链 ws666 |
| ws666 | 链：stub **536**（-1），拒绝 197。 | |
| run666 | `RestorableValue.value` 过了；下一站 `RawScrollbarState.initState` 的 stub：`scrollbarPainter = ScrollbarPainter(..)`——`late final ScrollbarPainter scrollbarPainter;`（无初始化器）在 open 类上，trait 只给了 getter 没给 setter（`!isFinal` 当「可写」）。修（通用）：无初始化器的 `late final` 是「赋一次」的可写字段（`_writable`），trait 声明 setter、impl 交 setter、经 trait 写的字段进 cell。夹具 latefinal SAME。同文件还有三处 stub 待踩：`_fadeoutTimer` 的 cell、`_getPrimaryDelta` 的 `Option<f64> + f64`、`_repaint` 的 `noSuchMethod`。 | 链 ws667 |
| ws667 | 链：stub **534**（-2：`late final` 可写后 `FormFieldState.initState`、`RawScrollbarState.initState`），拒绝 197。跳过 run667，先把同文件的另两处一起修：① `_fadeoutTimer = null` 在 trait 体的闭包里——闭包捕获字段的 cell（`_copyOf` 要 `_cell()`），trait 却只为集合字段交 cell；修：`shared`（被闭包写的）字段也交 cell，且 Copy 类型的 cell 是 `Cell`（`_cellType`）。② `late double primaryDeltaFromDragStart` 在 switch 里赋值再读——`late` 局部持 `Option` 读时没 unwrap；修：`late` 非空局部读 unwrap（同 try 写入的局部）。夹具 closurefield、latelocal SAME。 | 链 ws668 |
| ws668 | 链：stub **523**（-11，无新增），拒绝 197。 | |
| run668 | 滚动条 `initState` 过了，`build` → `_gestures` → `ScrollController.position` refusal：`_positions.single`——列表表里没有 `single`。修（表 + prelude）：`single` 进 `listMethodNames`，`DartList::single`/`Set::single`（`StateError` 语义 panic，同 `first`）。夹具 listsingle SAME。 | 链 ws669 |
| ws669 | 链：stub **523**（持平），拒绝 **192**（-5：`single`）。 | |
| run669 | `ScrollController.position` 过了，render 树 59 节点；下一站 `SliverList.createElement` → `SliverMultiBoxAdaptorElement` 构造子 stub：字段初始化 `SplayTreeMap<int, Element?>()` 拼成 `Map::new(None, None)`——`SplayTreeMap([compare, isValidKey])` 两个省略的可选参数被当实参填了 `None`，而 `SplayTreeMap` 不在后端「空集合构造」表里。修：`SplayTreeMap`/`SplayTreeSet` 进表（prelude 里就是 `Map`/`Set` 的别名，顺序已知丢失），省略的可选参数（拼出来是 `None`）不算实参。夹具 splaymap SAME。 | 链 ws670 |
| ws670 | 链：stub **521**（-4 +2：`SliverMultiBoxAdaptorElement.createChild/removeChild` 里 `_childElements.remove(index)` 在闭包中拼成 `&*__me._child_elements_cell()?.borrow_mut().remove(..)`——`&*` 本是给 `Trait::x_cell(&*__me)` 这种**实参**位置的，作方法接收者时套在整条链上把 `remove` 的结果解引用了），拒绝 192。修：cell 访问器作接收者时用裸句柄。夹具 closurefield 加 map 字段在闭包里改。提交 c5da374b。 | 链 ws671 |
| ws671 | 链：stub **517**（-4，无新增），拒绝 192。 | |
| run671 | render 树 60 节点；下一站 `MultiChildRenderObjectElement.children`：`_children.where((c) => !_forgottenChildren.contains(c))`——迭代器步闭包里捕获字段的绑定（`this_._forgotten_children_cell()?`）带 `?`，而步闭包不返回 Result；且捕获的 cell 没按 cell 读。修：步闭包的捕获绑定也在「unwrap」语境下拼，捕获的共享字段登记为 cell 局部（同 boxed 闭包）。夹具 stepcapture SAME。 | 链 ws672 |
| ws672 | 链：stub **505**（-12：`where` 步闭包捕获修好后 reply/shrine 的一批 getter 过了），拒绝 192。 | |
| run672 | `children` 过了，render 树 61 节点；下一站 `ScaffoldState.didChangeDependencies` → `_maybeBuildPersistentBottomSheet` 的 stub：`ModalRoute.of(context)!.addLocalHistoryEntry(entry)` 拼成 `ModalRoute::add_local_history_entry(&*route, ..)`——两个 trait 都声明该方法所以限定了，可 `ModalRoute<T>` 是**泛型** trait，裸 `Trait::m(..)` 是 E0782。修：非 `this` 句柄上的泛型 trait 限定走 `<dyn Trait<args> as Trait<args>>::m(&*h)`（实参从句柄类型/经类的实例化取，`_dynQualified`）。夹具 qualgeneric SAME。 | 链 ws673 |
| ws673 | 链：stub **509**（+4：`_dynQualified` 把 `RestorableEnum<X>` 经 `_argumentsThrough` 拿到的 `RestorableProperty<T>` 没代入句柄自己的实参——修：按句柄实参代入），拒绝 192。scaffold 的 `ModalRoute::add_local_history_entry` 其实另有根因：`LocalHistoryRoute.addLocalHistoryEntry` **根本没翻**（refusal「assignment to a field of another object (param, value)」：`entry._owner = this` 经参数写 `LocalHistoryEntry` 的字段，该类不是 counted）。修（通用）：别名变异普查（`alias_mutation.dart`）把「经非 this、非本地持有者的引用写字段」也算别名变异 → 类 counted；`_declaringTrait`/`_abstractAncestors` 跨模块查（`elsewhere`）。夹具 aliasparam SAME。小欠：Dart 方法叫 `drop` 会撞 Rust 的析构名。 | 链 ws674 |
| ws674 | 链：stub **505**（持平），拒绝 192（**没降**）：别名普查只扫 `inPackage` 的库，`entry._owner = this` 所在的应用类在 `dart:mixin_deduplication` 里，根本没被扫到。修：普查连去重库一起扫（写在应用类里、目标是 package 类的字段）。本地翻译验证：拒绝 192→188，`LocalHistoryEntry._owner` 进了 cell。 | 链 ws675 |
| ws675 | 链：stub **502**（-3），拒绝 **188**（-4：`addLocalHistoryEntry` 一族）。 | |
| run675 | Scaffold 过了，render 树 64 节点；下一站 gallery 自己的 `_AnimatedHomePageState.build`（两处 stub 同文件）：① `Function` 槽里的 void 闭包：适配器把 `Null` 类型的调用**整个换成** `dart_null_object()`（两处 `Null→Object` 规则都丢了副作用）——修：非字面量的 `Null` 值先求值再给 null 对象。② `_AnimatedCarousel.build` 的 `LayoutBuilder` builder 里的嵌套 builder 又从 `self.` 拷字段，把 `&self` 借进了 `'static` 闭包——修：外层闭包已拷贝的字段，内层从外层的局部拷。夹具还揪出 `flag.value = true` 在闭包里写**值类**局部的字段改到的是副本：别名普查把「经捕获的局部写字段」也算别名变异（`_ownLocal` 看声明函数）。夹具 dynfall、nestcapture SAME。 | 链 ws676 |
| ws676 | 链：stub **495**（-7），拒绝 188。提交 f0cdc53f。 | |
| run676 | 主页建到 render 树 **90 节点**；下一站老欠账 `AnimationMin<T extends num>.value`：`math.min(first.value, next.value)` 在 `T` 上没有 `min`。修（通用）：Dart 界是 `num`/`int`/`double` 的类型参数记为 `IrClass.numericParameters`，泛型界拼上 prelude 的 `DartNum`（`dart_min`/`dart_max`，实现在 i64/f64；名字不与 `Ord::min` 撞），`math.min/max` 的第一个实参是这种 `T` 时走它。夹具 nummin SAME。 | 链 ws677 |
| ws677 | 链：stub **494**（-1），拒绝 188。 | |
| run677 | `AnimationMin` 过了；下一站 `UndoHistoryState<T>.initState` 同文件两处 stub：① `UndoHistoryValue.hashCode` 的 `Object.hash(canUndo, ..)` 在 `bool` 字段上拼 `.hash_code()`——修：非翻译类的值（标量/prelude 类型/句柄）上的 `hashCode` 走 `DartEq::dart_hash_code`；② 顶层泛型 `_throttle<T>` 里被闭包写的 `T? arg` 局部拼成 `Cell<Option<T>>` 又 `.get()`——`_isCopy` 只把**类**的类型参数当非 Copy，方法/函数自己的没算。修：方法级类型参数同样非 Copy。夹具还揪出 `arg as T` 在 `Option<T>` 局部上（不是投影的 `Or`）：`dart_as_own` 改收 `Option<T>`，投影操作数先经 `option()`。夹具 hashfield、throttle SAME。 | 链 ws678 |
| ws678 | 链：stub **490**（-4），拒绝 188。 | |
| run678 | `UndoHistoryState.initState` 下一处 refusal：`UndoManager.client = this`——类的**静态 setter** 赋值只做了顶层 setter 的形。修：类静态 setter → `Owner::set_x(v)` 静态调用；顺带 free-static 类（只有静态成员的类）的静态 setter 与同名 getter 拼成同一个函数名（E0428）→ setter 保留 `set_` 前缀，静态调用存在性检查按 Rust 名。夹具 staticset SAME。 | 链 ws679 |
| ws679 | 链：stub **490**（持平），拒绝 **186**（-2）。 | |
| run679 | **走到文字排版**：`RenderEditable.performLayout` → `TextPainter.layout` → `TextStyle.getParagraphStyle` 的 stub——`ui.StrutStyle(..)` 9 个实参对上了 painting 的 `StrutStyle::new`（11 个）：dart:ui 与 painting 同名类（`StrutStyle`/`TextStyle`/`Gradient`/`Image`）在同一 crate 里按简单名引用，谁被 `use` 谁赢（老欠账「TextStyle 名字冲突」）。下一步：跨库同名类的引用按模块限定（`crate::dart_ui::StrutStyle`），同 `IrStaticCall.module` 对顶层函数的做法。 | |

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
- **`Layer.find<S>`**:出参 `AnnotationResult<S>` 经擦除孪生——run646 起用协变分析+隐藏
  `__ty_i` 解掉(见活账 run646);仍欠:方法体把自己的 `S` 作**类型实参**传给带隐藏值的
  方法(`find<S>` 里的 `findAnnotations<S>`)只有走静态派发(单体)时 S 才是真的,经 `find__erased`
  时是 Object——要把「作类型实参」也算值用法并做定点。
- **泛型 mixin 的 `T?` 字段 getter**:`Or` vs `Option<T>` 的投影差(run555 记)。
- **无 `==` 的值类做键**:Dart 按身份,值 struct 按结构;AOT 还会删未读字段
  (run558 记,不在路径上)。
- **Goal 3 差 `size=`**:渲染树类型序列已 6/6,dump 时拿不到 layout 结果。
- **727 个 stub 的长尾**:最大类是 "mismatched types"(约一半),其余是参数数、
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
- **列表/映射按值传递**:`f(log)` 里 `log` 是 `Vec` 的拷贝,被调方(或它返回的闭包)往里 `add`
  调用方看不见(throttle 夹具第一版踩到,改夹具绕开)。counted 类有身份,集合没有——通用解还没有。
