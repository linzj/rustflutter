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

## 活账:ws/run 表(窗口约 40 行,ws698 起;更老的在 git)

| 轮 | 第一个停点 / 读数 | 处理 |
|---|---|---|
| ws698 | 链：stub **463**（-1：`RenderShrinkWrappingViewport::new`，无新增），拒绝 183，可达 64。 | run698 |
| run698 | 过了 viewport。停在 `Switch._getSwitchSize` 的**运行期** unwrap（不是 stub）：`defaults.padding!` 拿到 `None`——`_SwitchDefaultsM3` 用 getter 覆盖了基类 `SwitchThemeData` 的**字段** `padding`（`const EdgeInsets.symmetric(horizontal: 4)`），而 trait 的字段访问器一律读存储，动态分发到不了 getter。修（通用）：本类自己声明的同名 getter 覆盖基类字段，访问器改调它——仅当 getter 的结果就是 trait 声明的那个 Rust 类型（Dart 允许协变收窄，如 `WidgetStateProperty<Color>` 顶 `WidgetStateProperty<Color?>`，那是另一笔欠账，仍读存储）。夹具 getterover SAME（基线复现同一个 `unwrap` on None）。 | 链 ws699 |
| ws699 | 链：stub **463**（无变化），拒绝 183，可达 64。 | run699 |
| run699 | 栈溢出（gdb：`AnimatedWidget::listenable` → `listenable_builder_super_listenable` → `AnimatedBuilder::listenable` → 回到第一个）。`ListenableBuilder.listenable` 和 `AnimatedBuilder.listenable` 上游都是 `=> super.listenable;`（只为挂文档），而基类字段的 trait 访问器**就是** `super.x` 读到的那份存储——把它改调这种 getter 就成环。修（通用）：getter 体里读了 `super.<同名>` 的不改路（它本来就是基类的 `x`；`_WalkSelf` 记下 `superMembers`）。夹具 getterover 加了 restated / restated2 两层 SAME。 | 链 ws700 |
| ws700 | 链：stub **463**（无变化），拒绝 183，可达 64。 | run700 |
| run700 | 过了开关尺寸。停在 `_SwitchPainter::new` 的 stub：「cannot find struct `_UnspecifiedTextScaler`」——三个库各声明一个同名私有类（paragraph/text_painter/media_query），`TextPainter.textScaler` 的默认值 `const _UnspecifiedTextScaler()` 谁也没指。两处原因：① 同名类普查（`collidingClassNames`）把**私有**名字排除了——私有在上游是库内可见，在这边是每库一个模块的 `pub(crate)`，跨模块按裸名解析等于看那个文件 import 了谁；② 常量实例的 `IrType` 没带 `module`（`_type` 带，`IrConstInstance` 不带），后端查类也只按名字。修（通用）：普查收私有名（匿名 mixin 仍不收）；常量实例带 `module`，后端 `library.resolve(t)` 查类。夹具 constmod（三个库同名私有 `_Unspecified`）SAME，基线是 `A:c,B:c,A:c`。 | 链 ws701 |
| ws701 | 链：stub **452**（-11：`_SwitchPainter`/`_CupertinoSwitch`/`Slider`/`RangeSlider`/`TabBar`/两个 Cupertino 对话框/日期选择器的 `new`、`RenderEditable._textIntrinsics`、`TwoPaneDemo.build`、`LineChart._drawXAxisLabels`，无新增），拒绝 183，可达 64。 | run701 |
| run701 | 停在 `_MaterialSwitchState.build`：`defaults.thumbColor!` 拿到 `None`——又是「子类 getter 覆盖基类字段」，但这次 getter 的类型收窄了。量了一遍：全 gallery 206 个这样的访问器，115 个只差可空（`Color` vs `Color?`）、9 个差类（`EdgeInsets` vs `EdgeInsetsGeometry`）、82 个差**类型实参**（`WidgetStateProperty<Color>` vs `WidgetStateProperty<Color?>`，Material 的 defaults 惯用法）。修（通用）：访问器改调 getter 的条件从「类型相同」放宽到「能按同一条 coercion 规则装进去」（`_fitsAccessor`：类型实参必须逐个相同，差类/差可空交给 coerce）——前两类共 124 个从此走 getter，第三类仍读存储。 | 链 ws702 |
| ws702 | 链：stub **454**（+2：-2 `_FileSpan.start/end`，+4 `Checkbox/Switch.overlayColor`（本类 getter 收 `&Rc<Self>`，trait 体里没有句柄）、`_TimePickerDefaultsM3.hourMinuteTextColor/TextStyle`（`_TimePickerDefaults` 与 `TimePickerThemeData` 都声明了它，`self.x()` 有歧义））。修（通用）：只有「这个 impl 块能以 `self.x()` 叫到」的 getter 才改路——排除 `&Rc<Self>` receiver 和「抽象父型把它声明成方法」（那种被发到那个 trait 的 impl 里去了）。 | 链 ws703 |
| ws703 | 链：stub **452**（回到 ws701 的同一组），拒绝 183，可达 64。 | run703 |
| run703 | 仍停在同一处：`thumbColor` 属于那 82 个「类型实参收窄」的。量过、放下的一条路（**不要再原样试**）：把「覆盖关系」也算成 covariance 的 flow site，并让 `_same` 认可空（比较的本来就是类型实参，`Color`/`Color?` 在这边是两个 Rust 类型）——`WidgetStateProperty<T>` 于是被擦除，两边拼法一致，但 **stub 474（+22）**：擦除边界上标量进出不成立。补了三条边界规则后降到 455（+3），剩两处：`buildToggleable` 的 `mouseCursor?.resolve(states)`（null-aware 体没按自己的静态类型转换，改了两版都没打中，说明它走的不是那条 lowering）和 `TweenSequence._evaluateAt` 的 `return`（进的是保留的 `T`）。covariance 那条撤回；三条边界规则留下（本身就对）：① `_asConstructorCall` 只按**保留的**类型参数代入（擦除的槽是 `Rc<dyn Object>`，代 `double` 进去把 `f64` 塞给了收对象的构造器）；② 被调方声明的返回是**擦除的**类型参数时，值按擦到的 bound 到达（`_erasedResult`），落地时由 coerce 读回；③ 条件是 `bool`——`if`/`while`/`?:`/`!`/`&&`/`||` 的操作数都走 `_condition`。夹具 covarnull（真实 flow site 触发擦除）SAME，基线 9 个错。 | 链 ws707 |
| ws704–706 | 上面那条路的三次测量：474 → 458 → 455。记在〈撤回与作废〉。 | |
| ws707 | 链：stub **452**（与 ws703 同一组，三条边界规则对 gallery 是中性的），拒绝 183，可达 64。 | run707 |
| run707 | 追那两处「擦除边界」剩下的：① `mouseCursor?.resolve(states)` 走的不是 null-aware 那条 lowering——TFA 把它换成了 `unsafeCast<MouseCursor?>(#t.resolve(s))`，而 `unsafeCast` 的「非空进可空」分支直接 `IrSome(操作数)`，手里是擦到的 `Rc<dyn Object>`。修（通用）：包 `Some` 之前先按同一条 coercion 规则把值读回来。夹具 covarnull 加 cursorBang SAME。② `TweenSequence._evaluateAt` 那条（保留的 `T` 收擦除结果）在夹具里已经成立（Seq/at/pair SAME），gallery 里那处是 super 自由函数，留 1 个 stub。补完后把 covariance 那条重新打开。 | 链 ws708 |
| ws708 | 链：stub **449**（-3：`_SwitchDefaultsM3` 一族的访问器接上了；唯一新增是 `tween_sequence_super__evaluate_at`），拒绝 183，可达 64。covariance 那条从 +22 变成 -3。 | run708 |
| run708 | **过了整个 Switch**（`_getSwitchSize`、`_MaterialSwitchState.build` 都不再 unwrap 到 `None`）。停在 `material_page_transitions_theme` 的静态初始化式：`.dart_cast_to::<dyn Animatable<Rc<dyn Object>>>().unwrap()` 拿到 `None`——`TweenSequenceItem<T>` 的 `T` 现在被擦除（真实 flow site：`_OpenContainerRoute._getColorTween` 把 `TweenSequenceItem<Color>` 交给 `TweenSequenceItem<Color?>`，可空一算数就露出来了），字段槽成了 `Rc<dyn Animatable<Rc<dyn Object>>>`；`TweenImpl<f64>` 有那个「更宽实例」的 impl（`addWiderImpls` 按**程序里出现过的**实例化生成），`_ChainedEvaluation<f64>` 没有——它是在泛型自由函数 `animatable_super_chain` 里造出来的，程序里从没写过这个类型，实例化普查看不见。下一轮：让「更宽实例」的普查也看见「泛型函数体内构造的类，按该函数被调用的实例化」。 | 下一轮 |
| ws709–712 | 修 run708：实例化普查（`_censusMembers` 的 `walk`）只收 abstract-like 的类，可「更宽 impl」要的是**具体泛型类的实例化**。第一版把 `walk` 一律收进来 → **451**（+2：provider 的 `_DelegateState.element/set_element`）；改成只收「体内构造的」（`_constructedIn` 那一趟，`built` 标记）仍 451——那两处正是从这条路来的：新的 self 实例化让 `_ValueInheritedProviderState<Listenable?>` 多了一个 `impl _DelegateState<Object>`，而字段是 `_InheritedProviderScopeElement<Listenable?>`，两个结构体实例化之间没有转换（已知欠账）。修（通用）：访问器（读与写）在**没有任何规则能架桥**时写 `todo!()`，不写不编译的代码——方法那条路一直是这么说的。链 **449**，与 ws708 同一组。 | run712 |
| run712 | 过了页面转场的静态初始化式。停在 `RenderFlex.performLayout` 的**拒绝**（not yet implemented）。 | 链 ws713 |
| ws713–717 | `RenderFlex.performLayout`/`_computeSizes` 的拒绝是「`is` against `Function`/`Record`」——CFE 把记录解构模式（`final (nextChild, topLeftChild) = ..`）降成「拿每个字段跟它本来就有的类型做 `is`」，函数类型和 Record 都不是 `Any` 问得出来的。修（通用，四条）：① 操作数的静态类型是所问类型的子类型时，Dart 自己的子类型关系就是答案，拼 `true`（与上面字面量那条同源）；扩展类型走**表示类型**（`_AscentDescent` 就是 `(double, double)?`）。② 只差一个 `?`、且所问类型没有运行期测试（记录/函数类型）时，测试就是那个空检查——类的情形留给原来的 `is`，它的收窄这条看不见（`is_none` 打到 `Rc<Border>` 上，ws716）。③ 记录字段读按**记录持有的**类型定型，不按模式提升后的 `Object?`（Rust 元组里仍是 `f64`），可空记录读经 unwrap（Dart 只允许提升后读）。④ `_clonedWhenPassed` 穿过扩展类型（`_AxisSize` 就是 `Size`，不克隆就被第一个实参移走）。夹具 recdestr2 SAME。链 **449**（与 ws713 同一组），拒绝 **183 → 174**。 | run717 |
| run717 | `_computeSizes` 过了。停在 `_AscentDescent operator +` 的拒绝：仍是 `is` against `Record`。 | 链 ws718 |
| ws718 | 修 run717：所问的类型也要擦——扩展类型在运行期没有身份，`x is _AscentDescent` 问的就是表示类型。链 **449**（同一组），拒绝 **173**。 | run718 |
| run718 | 整个 `RenderFlex` 布局过了。停在 `SliverMultiBoxAdaptorElement.didFinishLayout` 的拒绝：`Map.firstKey`（`_childElements` 是 `SplayTreeMap<int, Element?>`，prelude 没有按键排序的首/末键）。 | 链 ws719 |
| ws719 | 修 run718：prelude 的 `Map` 按插入序存（`SplayTreeMap` 就是它的别名），排序映射承诺的首/末键**算出来**——`impl<K: Clone + PartialOrd, V> Map<K, V>` 上的 `first_key`/`last_key`，只有键可比时才有这两个方法；`mapMethodNames` 加两条。夹具 sortedmap SAME。链 **449**（同一组），拒绝 **170**。 | run719 |
| run719 | 过了 sliver 的 `didFinishLayout`。停在 `_PageViewState.build` 的 stub：「multiple applicable items in scope」。 | 链 ws720–722 |
| ws720–722 | `_PageViewState.build` 的两处，同一个根因：`NotificationListener<ScrollNotification>` 的回调形参被擦除的 `T` **重定类型**成 `Notification`，读回时按声明的类下转型（`_localRead`），但两处判断都还在看 Dart 静态类型。① `notification.depth` 的限定名从静态类型（bound）走，那上面什么也没声明，于是 `ScrollNotification` 与 `ViewportNotificationMixin` 都来认领（E0034）→ 重定类型的形参按**声明的类**算限定名。② `notification.metrics` 出成字段访问：TFA 把静态类型收窄成了具体子类 `OverscrollNotification`，`concrete` 判真，可手里的值还是 `dyn ScrollNotification` → 重定类型的形参的「接收者的类」也按声明的那个算。中途试过「接收者不是具体类就一律走访问器」——**stub 805**，远超，撤回（记在〈撤回与作废〉）。夹具 mixdepth SAME。链 **448**（-1），拒绝 170。 | run722 |
| run722 | 过了 PageView。停在 gallery 自己的 `_MobileCarouselState.builder`：`x.clamp(0, 1)`——`clamp` 的形参声明是 `num`，而 `num` 在这边不是类型，整型字面量原样给了 `f64::clamp`。修（通用）：`num` 形参在**数字自己的方法**上就是接收者那个数字（`_numReceiver`）；`int.+(num)` 那条不受影响（接收者是 int）。夹具 numclamp SAME。 | 链 ws723 |
| ws723 | 链：stub **439**（-9：滑块/进度/购物车/雪碧图等一串 `clamp`），拒绝 170，可达 64。 | run723 |
| run723 | **启动路径上不再有 panic**——程序跑满 120s 无输出、无 panic。gdb 抓栈：`gallery/constants.dart` 的 `kTransparentImage` 的 `LazyLock` 自锁——上游写的是 `final kTransparentImage = transparent_image::kTransparentImage;`（另一个库的同名顶层），拼成裸名后读到了自己。修（通用）：顶层**读**也带 module（`IrTopLevel.module`，与 `_topLevelModule` 同一张判断），后端拼 `crate::<module>::NAME`。夹具 topshadow SAME（基线挂住）。 | 链 ws724 |
| ws724 | 链：stub **439**（同一组），拒绝 170。 | run724 |
| run724 | 死锁没了。停在 `ImageProvider.resolve` 的 stub：`None.await`——上游写的是 `await null;`（让微任务队列跑一轮的惯用法）。修（通用）：`await v` 当 `v` 静态类型**不可能是 future** 时（不是 Future/FutureOr/顶类型/类型参数/实现 Future 的类），就是「一个 turn 加这个值」——拼成 `future_ready::<T>(v).await`，turbofish 由静态类型给（裸 `None` 推不出 `T`）。夹具 awaitnonfut SAME。链 **438**（-1）。 | run725 |
| run725 | 停在 `AssetImage.obtainKey` 的拒绝：`FutureExtensions|onError`（dart:async 在 `Future` 上的扩展）。修（通用）：它就是把错误类型折进 test 的 `catchError`——`E` 是 `Object` 时 prelude 的 `catch_error` 就是全部；更窄的 `E` 要把 `is` 写进 test，拒绝而不是丢掉。实参按**扩展自己的类型参数**代入（回调返回 `FutureOr<T>`，按声明降会拼出没人声明的 `T`）。夹具 futonerror SAME。 | 链 ws726 |
| ws726 | 链：stub **439**（+1：`obtain_key` 的 `Ok(FutureOr::value(None))` 推不出 `T`——`catch_error` 的回调按 `Rc<dyn Object>` 存着，什么也不约束）。修（通用）：`_fallsOffValue` 的 `None` 拼出类型 `None::<T>`。链 **438**（ws727），拒绝 169。 | run727 |
| run727 | 图片路径走通了（错误被抛出并开始格式化）。停在 `FlutterError.defaultStackFilter` 的拒绝：`Map.update`。修（通用）：prelude 的 `Map` 加 `update(key, update, {ifAbsent})`（没有 `ifAbsent` 又没有键时抛 `ArgumentError`，与 Dart 同），`mapMethodNames` 与 `_preludeFailing` 各加一条。夹具 mapupdate SAME。 | 链 ws728 |
| ws728 | 链：stub **438**，拒绝 168。 | run728 |
| run728 | 停在 prelude 的 `List.sort() without a comparator on a type with no natural order` —— `DartList<T> for Vec<T>` 对任意 `T`，自然序放不进去。修（通用）：`sort_natural` 单独一个 trait，按 **Dart 的 `Comparable`** 排（标量在 prelude 里已实现，翻译类的 impl 由后端 `_preludeInterfaces` 生成）；没有 `Comparable` 的元素就没有这个方法——停在编译期而不是运行期。省略的比较器是「无比较器」那一支，不是 `sort_by_dart(None)`。中途先用 `PartialOrd` 试过：ws729 **441**（+3，`_SemanticsSortGroup` 这类只实现 `Comparable` 的翻译类不满足），改成 `Comparable` 后 **438**。夹具 sortnat SAME。 | 链 ws730 |
| ws730 | 链：stub **438**（与 ws728 同一组），拒绝 168，可达 64。 | run730 |
| run730–733 | 一串小口子，各修各的（都带夹具，链每轮不涨）：`firstWhere` 给了 `orElse` 时要**裸**传（Dart 的槽是 `E Function()?`，coercion 包了 `Some(Rc::new(..))`，prelude 的形参是 `impl Fn()`）；prelude 加 `String.lastIndexOf`；枚举方法里的 `*self` 作接收者要加括号（`*self.dart_to_string()` 解引用的是那个 `String`）；枚举上**程序员写的** `toString` 不能当隐式成员丢掉（丢了 `dart_to_string` 就回落到 `Kind.material`，Dart 打的是 `MATERIAL`）。链 ws733 **435**，拒绝 168。 | run733 |
| run733–734 | `RenderPhysicalModel` 读到未初始化的 `late _needsCompositing`：`RenderObject()` 的**体**设它，而构造器体上溯到泛型基类就停（基类体里的 `T` 这边拼不出来）——可**无体**的泛型基类什么也没拼，`_RenderPhysicalModelBase<T>` 正是这样夹在中间。修（通用）：无体的泛型基类不挡路。链 ws734 **435**（同组）。 | run734 |
| run734–743 | `ContainerRenderObjectMixin.visitChildren` 把 `SliverMultiBoxAdaptorParentData` 往 `FlexParentData` 上转：mixin 的体是从**某一个应用**里借的（CFE 把参数替换掉了），当 trait 默认体用时要服务所有应用。修（通用）：借来的体里，应用替进去的实参**换回** mixin 自己的类型参数——只对**被擦除**的参数（它的拼法就是 bound，本来就是大家读的那个 trait），`_appliedBack` 一张映射，`_type`、字段读的「接收者的类」、限定名（读与写）、`as` 的目标类型都走它；映射只应用一次（bound 里再提到被映射的类型会打转，ws741 前端栈溢出）。同一趟还补：`__aN` 实参临时量绑定的是**位置**时要克隆（`slot` 被移走后又读）。链 ws743 **435**（与 ws734 同一组），拒绝 168，`rendering_object.rs` 里再无 `FlexParentData`。 | run743 |
| run743 | 停在 `BorderDirectional.paint` 的 stub：`cannot index into a value of type Set<Color>`。 | 下一轮 |

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

## ws749 — identical on a nullable value slot (433 → 398)

The 433 blocks of `stubs747.txt.detail.txt`, grouped by normalised
expected/found pairs, put 35 in one group: `&Option<Rc<_>>` wanted where
`&Option<BadgeThemeData>` was given. `dart_identical_opt` takes handles,
and every theme's `static X? lerp(X? a, X? b, double t)` starts with
`identical(a, b)` on two nullable slots of a class the backend spells by
value.

The rule: a nullable slot that is not a handle is answered by the prelude's
`dart_identical_opt_value` -- both absent, or the same storage. That is the
answer two value locals already get here (a value class is copied where
Dart shared one object), so the nullable case now says what the non-nullable
case says.

    ws747 433 stubbed, 162 refusals, 64 crates
    ws749 398 stubbed, 162 refusals, 64 crates   -35, 0 new

Grouping is worth far more than the panic-by-panic walk: one rule, 35 gone,
nothing new.

## ws750 — a chain step's callback is built once, outside (398 → 390)

Second group of the ws747 census: 8 blocks of `expected bool, found
Result<_, Rc<dyn Object>>`, all in `_sortAndFilter{Horizontally,Vertically}`.
Upstream writes `nodes.where(switch (direction) { .. => (node) => .., up ||
down => throw ArgumentError(..) })`. The switch expression is a block, and
`_stepClosure` inlined it into the `filter` closure it writes -- so the
throwing arm's `return Err(..)` returned from a closure typed `-> bool`,
and the block was rebuilt once per element.

The rule: a chain step whose callback is a *value* rather than a written
closure is bound before the chain, and the closure calls the binding. That
is also what Dart does -- `where`'s argument is evaluated once.

    ws749 398 stubbed, 162 refusals, 64 crates
    ws750 390 stubbed, 162 refusals, 64 crates   -8, 0 new

(The rule rode along in ws749's commit; the measurement is this one.)

## ws751 — an operator's right operand is its parameter, not its `Self` (390 → 380)

Third group: `expected Rc<X>, found X`, 17 blocks across `Vector3`,
`_Vector`, `OffsetPair`, `AttributedString`. `impl std::ops::Mul for
Struct` fixes the *left* operand as the struct value, and ws511 wrote
"each handle operand is the value it holds". The right is not that: the
impl's `Rhs` is the operator as it was declared, and a counted class
named in a parameter is its handle -- `Mul<Rc<Matrix3>> for Matrix3`,
`Mul<Rc<dyn Object>>` where the parameter is `dynamic`. Dereferencing it
handed `Vector3` to an `Rc<Vector3>` slot.

    ws750 390 stubbed, 162 refusals, 64 crates
    ws751 380 stubbed, 162 refusals, 64 crates   -10, 0 new

Also in this commit, measured next: an operator's `self` is `std::ops`'s,
by value, so the `_handles` contagion -- "a method that calls a
`&Rc<Self>` method takes the handle too" -- has nowhere to put the
handle. Inside an operator, `this` for such a call is the object's own
`dart_self_ref().get()` (`Vector4::op_mul` calling `clone`, 12 at ws747).

## ws752 — `this` as a handle inside an operator (380 → 363)

The rule committed with ws751, measured: `_handles` is a contagion --
a method that calls a `&Rc<Self>` method on `this` takes the handle too --
and an operator cannot join it, because `std::ops` fixes the receiver by
value. Inside one, `this` for such a call is `dart_self_ref().get()`.

    ws751 380 stubbed, 162 refusals, 64 crates
    ws752 363 stubbed, 162 refusals, 64 crates   -17, 0 new

17, not the 12 the census showed: the `clone_` blocks were the visible
part of a bigger set (`op_add`, `op_sub`, `op_neg` on Vector2/3/4 and
Matrix2/3/4 all clone `this` first).

## ws753 — a closure's trait return type is spelled (363 → 353)

`_closureReturnSpelled` gave `_` for everything but a nullable, so a
closure returning a trait had no coercion site: `List<Widget>.generate(n,
(i) => _VisibilityScope(..))` collected a `Vec<Rc<_VisibilityScope>>`
where `Vec<Rc<dyn Widget>>` went, and the unsizing had nowhere to be
written. Spelled, the `Ok(..)` around the body is that site.

    ws752 363 stubbed, 162 refusals, 64 crates
    ws753 353 stubbed, 162 refusals, 64 crates   -10, 0 new

Four more rules go in with this commit, measured next:

* an indexed assignment crosses into the element as the *list* spells it
  (a `List<E?>` field of a generic class holds `<E as DartNullable>::Or`,
  3 at ws751);
* a value that already is a handle is not wrapped in another (`dart_object`
  around an `Rc<dyn Widget>` made an `Rc<Rc<dyn Widget>>`, 10 at ws751);
* `Object.noSuchMethod` is the prelude's, so the CFE's forwarders on a
  concrete class that implements an interface without implementing it
  resolve (`_DefaultSnapshotPainter`, 15 at ws751);
* a record with named fields is a tuple: the named part after the
  positional, in the type's sorted order, which is Dart's canonical order
  (`SelectionOverlay._handles`, 6 stubs and 5 refusals at ws751).

## ws754 — four rules from the census (353 → 333, refusals 162 → 130)

    ws753 353 stubbed, 162 refusals, 64 crates
    ws754 333 stubbed, 130 refusals, 64 crates   24 gone, 4 new

The four: the indexed assignment's projection crossing, no second handle
around a handle, `Object.noSuchMethod` in the prelude, named record
fields as the sorted tail of the tuple. The refusal count is the record
rule: 32 refusals were `a record type with named fields`, `a record with
named fields` and `RecordNameGet`. The 4 new stubs are that same rule's
bill -- `SelectionOverlay.showMagnifier`, `showToolbar`,
`_classifyRegions`, `getGlyphHeights` were refused before and now compile
far enough to be counted.

## ws755 — an enum's mixins (333 → 329)

    ws754 333 stubbed, 130 refusals, 64 crates
    ws755 329 stubbed, 130 refusals, 64 crates   -4, 0 new

Three rules go in with this commit, measured next -- two of them the bill
for ws754's record rule:

* a closure body's `return` is wrapped against the *closure's* declared
  type, not the enclosing method's. The backend saved `_rustReturns`
  around a closure but not `_returns`, which is what `_returned` reads, so
  `getIcon: (context) => Icons.menu` inside a `Widget build` became
  `dart_object(IconData::new(..)) as Rc<dyn Widget>` (`_ActionIcon`, 4 at
  ws751);
* a record into a record slot converts field by field, as a list's
  elements do: a literal is typed by what was written in it, and the slot
  may spell a field wider (`_classifyRegions` returning a `Set` where the
  typedef says `Iterable`);
* a record field read clones unless the record is a literal built right
  there: a closure's parameter is a reference, and reading a field out of
  one moves (`SelectionOverlay.showToolbar`, E0507).

## ws756 — the closure return and the two record rules (329 → 315)

    ws755 329 stubbed, 130 refusals, 64 crates
    ws756 315 stubbed, 130 refusals, 64 crates   -14, 0 new

Grouped, eight rounds in: 433 -> 315 stubbed, 183 -> 130 refusals, 64
crates throughout, and not one round went up.

## ws757 — the prelude's slot (315 → 314)

    ws756 315 stubbed, 130 refusals, 64 crates
    ws757 314 stubbed, 130 refusals, 64 crates   -1, 0 new

One, not the group's fifteen. `stubs.py` reports the *first* error of a
stubbed function, so a census groups first errors, not functions: the
projection crossings were right to fix and the functions behind them fail
on something else next. From here a rule's number is what it *uncovers*
as much as what it clears -- `intl_plural_logic`'s next error is
`substring` on an `Option<String>`.

## ws759 — the out-of-scope parameter rule was wrong (314 → 325)

    ws758 no report: `T extends Comparable<T>` spelled itself forever and
          the translation was a stack overflow
    ws759 325 stubbed, 130 refusals, 64 crates   2 gone, 13 new

"A type parameter no name here stands for is its bound" reads well and is
wrong: the parameter *is* meaningful, and what puts a name to it is the
instantiation the substitution machinery fills in later (`_asApplied`).
Spelling it at its bound intercepted that -- `MaterialPointArcTween`'s
inherited `Tween<T>.begin=` became `Option<Rc<dyn Object>>` where the
`Option<Offset>` field is, and `FocusNode._focusedChildren` a
`Vec<Rc<dyn Object>>`. It also fixed none of the ten `cannot find type`
it was written for. Reverted; the `return this` rule beside it goes on to
ws760 with the rest.

## ws760 — four rules, and the scheduler's own bill (325 → 299)

    ws759 325 stubbed (the reverted rule), 130 refusals
    ws760 299 stubbed, 130 refusals, 64 crates   16 gone, 1 new

The four: a trait's accessor converts the method it reaches, an operator
forwarder binds its `Output` instead of mapping a `Result` it never had,
`future_none::<T>` is spelled, and `Timer.periodic` hands its callback the
timer as Dart's does. The one new stub is the prelude's own
`run_until_idle`: the `due` vector still spelled the old 0-argument
callback type. Fixed with the rethrow rules below.

Under 300 for the first time; 433 -> 299 grouped, refusals 183 -> 130.

## ws761 — rethrow carries its type (299 → 297)

    ws760 299 stubbed, 130 refusals, 64 crates
    ws761 297 stubbed, 130 refusals, 64 crates   -2, 0 new

Two, and both of them matter more than the number: the prelude's own
`run_until_idle` (the scheduler, and with it every timer at runtime) and
`AssetBundleImageProvider._loadAsync` -- the gallery's whole image path,
which the render-tree reconciliation reads as `RenderImage` 16 -> 0.

## ws762 — the same 297, and the run ruler moved

    ws761 297 stubbed, 130 refusals, 64 crates
    ws762 297 stubbed, 130 refusals, 64 crates   0 gone, 0 new

`dart_boxed` changes no count; it changes what the run can say.

Run 762 (the ws761 workspace): 4 frames, 0 panicked, 14 platform messages
(was 12). The startup panic moved off the image path --
`AssetBundleImageProvider._loadAsync` compiles now -- and onto
`DefaultProcessTextService.queryTextActions`, a stub whose panic the app
survives.

The render walk is 708 lines against the reference's 708, and the first
23 lines are identical. What the counts say, per node type:

    RenderImage              16 -> 0     no image renders at all
    RenderAnimatedOpacity     4 -> 0     the page transition differs:
    RenderFractionalTranslation 4 -> 0     ours builds SnapshotWidget
    RenderClipRect           21 -> 11
    RenderTransform          20 -> 12
    RenderStack              18 -> 10
    RenderRepaintBoundary    29 -> 51
    RenderPointerListener    52 -> 74
    RenderErrorBox            0 -> 6     six subtrees threw while building

The six error boxes are the thing to chase: each is a `build` that threw,
and until ws762 every one of them printed `Instance of 'StateError'`.

## run763 — the error boxes are legible, and they are one bug

`dart_boxed` did its work: the gallery's first uncaught exception now
reads

    Bad state: TweenSequence.evaluate() could not find an interval for 0.0

instead of `Instance of 'StateError'`. The six `RenderErrorBox`es are that
one error, reported six times.

`TweenSequenceImpl::new` was a struct literal and nothing else: the
constructor's whole body -- `_items.addAll(items)` and the loop that fills
`_intervals` -- had been dropped. Two rules behind it, each with a fixture
that now agrees with Dart:

* a constructor body's mutation of the class's own collection acts on the
  field, not on a clone. `_ownCollectionField` asked for `_selfName ==
  'self'`, and a constructor body's `this` is `__new` (fixture ctorbody2:
  `00012` -> `12312`);
* a *generic* base's constructor body runs when what this class puts in
  for the base's parameters is those same names -- the struct beside an
  open class's trait carries them unchanged (`SeqImpl<T>` over `Seq<T>`).
  The rule was written for a base whose `T` the subclass cannot name;
  this one can (fixture ctorbody: `0/0/0/0` -> `3/3/1/1`).

Both are silent-wrong-answer bugs, not compile errors: neither shows in
the stub count.

## ws763 — the constructor rules (297 → 298)

    ws762 297 stubbed, 130 refusals, 64 crates
    ws763 298 stubbed, 130 refusals, 64 crates   4 gone, 5 new

+1, and the five new ones are code that had never run before: a generic
base's constructor body now runs, so `HeapPriorityQueue._grow`,
`IterableEquality.hash` and `_SettingsListItem.initState` are reached and
meet the projection and collection edges that were always there. The four
that went are the ones the same rule fixed.

The number is not the point of this round. `TweenSequence` builds its
intervals again.

## run764 — the error boxes are gone, and a panic took the tree with it

The two constructor rules did what they were for: `TweenSequence` builds
its intervals, the `Bad state:` line is gone and `RenderErrorBox` is 0.

But the walk is 138 lines against 708: `_SettingsListItemState.initState`
panics, and the settings list -- most of the home page -- never builds.
That stub is one of ws763's five new ones, and it is the genargproj rule
overreaching: a generic *method*'s parameter is instantiated by what its
turbofish spells (the plain `Option<T>`), but a *class*'s instantiation is
not -- the struct is named `SettingsListItem<<T as DartNullable>::Or>` and
its fields keep the projection. Narrowed to the turbofish case; the
`Map::from(.. from_option(k) ..)` the state built over its widget's
`optionsMap` is gone.

## ws764 — the narrowing paid for itself (298 → 292)

    ws763 298 stubbed, 130 refusals, 64 crates
    ws764 292 stubbed, 130 refusals, 64 crates   -6, 0 new

`cannot find type T` went from ten to six: the instantiation a generic
class's constructor carries reaches the closure written for its
function-typed parameter, and four gallery `build`s came back.

Grouped, from ws747: 433 -> 292 stubbed, 183 -> 130 refusals, 64 crates
throughout.

## run765 — 557 lines, no error boxes, and the next stop is the erasure boundary

    walk 557 lines (was 138 at run764, 708 in the reference)
    RenderErrorBox 0

The narrowing brought the settings list back. The panic is now
`TweenSequence._evaluateAt`, and it is the erasure boundary:
`TweenSequenceItem<T>` loses its `T`, so `item.tween` is an
`Rc<dyn Animatable<Rc<dyn Object>>>` -- while Kernel's substitution says
`Animatable<TweenSequence.T>`, which is what the caller wrote and not what
is there. Two rules:

* a read whose declared type *mentions* a parameter the declaring class
  erased is recorded the way the struct spells it (`_erasedRead`);
* a call whose declared return is the declaring class's own parameter,
  through a receiver that puts a `dynamic` there, returns that `dynamic`
  (`_throughReceiver`) -- recorded even over the type the lowering already
  put on, because that type is the lie.

`_evaluateAt` now reads `dart_from_dynamic::<T>(element.tween.transform(t)?)`.

## ws765 — the erasure rule overreached (292 → 324)

    ws764 292 stubbed, 130 refusals, 64 crates
    ws765 324 stubbed, 130 refusals, 64 crates   1 gone, 33 new

`_throughReceiver` typed the result by what the receiver puts in for the
declaring class's parameter, and dropped the declared nullability with it:
`Map<K, V>.[]` returns `V?`, which the top-bound rule already spells
`Option<Rc<dyn Object>>`, and a bare `dynamic` there took the `Option` off
every null-aware read around it. Narrowed to a non-nullable parameter,
which is the case it was written for (`Animatable<T>.transform`).

## ws766 — the nullability was not the overreach (324)

    ws765 324 stubbed
    ws766 324 stubbed, 130 refusals, 64 crates   (vs ws764: 1 gone, 33 new)

Narrowing `_throughReceiver` to a non-nullable parameter changed nothing,
so the cost is `_erasedRead`: it fired for *any* read whose declared type
mentions a parameter the declaring class erased, and most such classes
keep some of their parameters, so reading at the bound there is simply
wrong. Narrowed again to a class that kept **none** of them -- the struct
then has no type parameters at all, which is what makes the spelling
unambiguous, and is exactly `TweenSequenceItem`.

## ws767 — three narrowings, the same 324

    ws765 324, ws766 324, ws767 324, all with round 3 at 336 against
    ws764's 249.

Narrowing `_throughReceiver` to a non-nullable parameter and `_erasedRead`
to a class that kept none of its parameters changed nothing at all, which
says the cost is neither. The third change in that commit was
`_fillsParameter` resolving an abstract dispatch target -- a change to
*signatures*, program-wide. `DART2RUST_ERASURE_OFF=1` turns the two
erasure rules off so the next round says which it is.

## ws768 — the bisect (289 with the erasure rules off)

    ws767 324 stubbed (rules on),  round 3: 336
    ws768 289 stubbed (rules off), round 3: 246

So the +35 was theirs after all, and the three narrowings did nothing
because the *first* of them never landed in the file: the patch reported
success and the commit carries a different hunk. The guard is in now, and
strict: `Map<K, V>.[]` returns `V?`, which Kernel writes `V%` --
*undetermined*, not `nullable`, because `V`'s bound is nullable -- so
testing for `nullable` let every `Map`-of-dynamic read through and took
its `Option` off.

289 is also three better than ws764: the lending fix, `DartEnum` on an
`Enum`-bounded parameter, and the promoted downcast's type.

## ws769 — 289 with the rules on, and the guard was one notch too tight

    ws768 289 (rules off)
    ws769 289 (rules on, strict non-null guard)   0 gone, 0 new

Identical, and `_evaluateAt` still stubbed: `Animatable<T>.transform`
returns a bare `T`, which Kernel also writes *undetermined* when `T`'s
bound is nullable, so "strictly non-null" rejected the very case the rule
was for. The distinction that matters is the one Kernel does draw:
`T?` is `nullable`, a bare `T` is not. Rejecting only `nullable` keeps
`Map<K, V>.[]` out and lets `transform` in -- checked in the translation,
both ways, before measuring this time.

## ws770 — 288, and the erasure rule earns its keep

    ws769 289 stubbed
    ws770 288 stubbed, 130 refusals, 64 crates   -1, 0 new

The one that went is `TweenSequence._evaluateAt` -- the run ruler's
current stop. Grouped, from ws747: 433 -> 288 stubbed, 183 -> 130
refusals, 64 crates throughout.

## run771 — the walk is 770 lines and the difference is one widget

    walk 770 lines against the reference's 708, 752 of them laid out
    RenderErrorBox 0

`_evaluateAt` compiling put the whole home page back. Counted per node
type, these now *match* the reference exactly: `RenderImage` (16, and it
was 0 four runs ago), `RenderPadding`, `RenderSemanticsAnnotations`,
`RenderParagraph`, `RenderFlex`, `RenderStack`, `RenderPhysicalShape`,
`RenderConstrainedBox`, `RenderPositionedBox`, `_RenderInkFeatures`,
`RenderIndexedSemantics`, `RenderWrap`, `RenderOpacity`.

What is left is one divergence, at line 24, and everything else follows
from it:

    ref : RenderAnimatedOpacity + RenderFractionalTranslation  (4 each)
    ours: _RenderSnapshotWidget                                (4)

    RenderPointerListener      52 -> 76      RenderClipRect  21 -> 13
    RenderRepaintBoundary      29 -> 53      RenderTransform 20 -> 12
    RenderMouseRegion          23 -> 35
    RenderSemanticsGestureHandler 11 -> 23
    RenderCustomPaint          17 -> 29

That is the page transition. The reference is a `flutter test` capture and
the test binding presents `TargetPlatform.android`, whose default builder
is `PredictiveBack` -> `FadeForwards` (fade and slide, no snapshot); this
run is on Linux and `PageTransitionsTheme` gave it
`ZoomPageTransitionsBuilder`, which is a `SnapshotWidget`. So the trees
are both right and the *platforms* differ.

`DART2RUST_OS` makes the run present a platform, as `DART2RUST_DUMP_*`
make it dump: a host knob, not a translation rule. The next run sets it to
`android` and the walk is compared again.
