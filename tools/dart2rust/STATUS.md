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

## ws772 — 288, unchanged

    ws770 288 stubbed
    ws772 288 stubbed, 130 refusals, 64 crates   0 gone, 0 new

`BoxBorder.lerp` still wants its `Some`: the promoted value there is not
the `IrCastTo` that was typed but a downcast reaching `_handleOf`, which
hands back a handle with no recorded type -- and `_widenedInto` treats an
unknown type as "may already be an Option" and skips the wrap. That guard
is the pattern behind this whole family; typing the three promoted reads
one at a time is chipping at it.

## run773 — the render tree's *structure* matches the reference

    DART2RUST_OS=android
    walk 708 lines against the reference's 708
    RenderErrorBox 0, _RenderSnapshotWidget 0
    identical prefix: 43 lines
    ignoring `size=` and `offset=`: **2 differing lines**, and they are
    one node: `RenderAnnotatedRegion<SystemUiOverlayStyle>` against our
    `RenderAnnotatedRegion`

So the node type sequence -- 708 of them, the whole gallery home page --
is the reference's, once the run presents the platform the reference was
captured on. That is Goal 3's first half.

What is left is the numbers. 510 lines differ on `size=`, and the root of
it is text: **32 `RenderParagraph`s measure `Size(0.0, 0.0)`** here and
none does in the reference. Their zero sizes propagate into every
ancestor's, which is most of the 510.

Next: why a paragraph measures zero, and `runtimeType` of a generic class
spelling its type arguments.

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

## ws774 — 288, the applied `runtimeType` costs nothing

    ws772 288 stubbed
    ws774 288 stubbed, 130 refusals, 64 crates   0 gone, 0 new

## run775 — the render walk's structure is the reference's, exactly

    DART2RUST_OS=android
    708 lines against 708
    ignoring `size=` and `offset=`: **0 differing lines**
    with them: 508, and the identical prefix is 60 lines

Every node's `runtimeType`, in order, at the same depth, is the
reference's -- the applied `runtimeType` closed the last one. Goal 3's
structural half is done.

The 508 are the sizes, and their root is the 32 paragraphs measuring zero
(see above): a text engine, not a translation. The first size to differ is
`RenderIndexedSemantics size=Size(800.0, 52.0)` against our
`Size(800.0, 26.0)` -- a list row whose height is its text's.

## ws776 — 288, and the run's panic moved one level in

    ws774 288 stubbed
    ws776 288 stubbed, 130 refusals, 64 crates   0 gone, 0 new

The `Future` top-bound rule did what it said: the erased twin's cast now
asks for `DartFuture<Option<Rc<dyn Object>>>`, which is what the twin
returns. The error behind it is one level in -- the *awaited* value was
still recorded a bare `dynamic`, so `dart_nullable` went around something
already in its `Option`. An `await` is now typed by the future its operand
holds.

Also in flight: a `dart:` mixin's bodies are not copied into a class that
is an `Iterable`. `Board extends Iterable<BoardPoint?>` mixes in
`dart:collection`'s `IterableMixin`, whose `skip`, `where` and `cast`
build `SkipIterable`, `WhereIterable` and `CastIterable` -- private
classes nothing translates. The prelude answers those members through
`__to_list` (9 at ws774).

## ws777 — a `dart:` mixin's bodies stay out (288 → 280)

    ws776 288 stubbed
    ws777 280 stubbed, 130 refusals, 64 crates   -8, 0 new

All eight are `Board`'s: `cast`, `elementAt`, `first`, `where`, `skip`,
`toList`, `toSet`, `toString`, copied in from `dart:collection`'s
`IterableMixin` and building private classes nothing translates. The
prelude answers them.

Grouped, from ws747: 433 -> 280 stubbed, 183 -> 130 refusals, 64 crates.

## ws778 — an await is typed by its future (280 → 277)

    ws777 280 stubbed
    ws778 277 stubbed, 130 refusals, 64 crates   -3, 0 new

## ws779 — the null-aware fold and the big literal (277 → 276)

    ws778 277 stubbed
    ws779 276 stubbed, 130 refusals, 64 crates

Grouped, from ws747: 433 -> 276 stubbed, 183 -> 130 refusals, 64 crates
throughout, and the render walk's structure is the reference's.

## run780 — the walk holds, the panic moves again

    DART2RUST_OS=android, 708 lines, structure identical, 32 zero paragraphs
    panic: `ImplicitlyAnimatedWidgetState.didUpdateWidget`

A `?..` cascade hands the binding back, and `as_ref()` binds a `&T` where
the slot takes the `T`. The body's *value* being the binding is now a
clone; a blanket clone would be wrong, because the cascade's steps mutate
through that same reference.

And the `lerpDouble(a, 0, t)` family, which took three tries to place:
`lerpDouble` is declared `num?`, not `double?`; it lives in `dart:ui` but
is *translated*, so its `num` really is an `f64` here -- the guard was
"declared in `dart:`" and is now "not a translated callee"; the literal
arrives as a `ConstantExpression`, not an `IntLiteral`; and `_toF64` has
to reach inside the `Some` the widening already put on, or the cast lands
on the `Option`.

## ws781 — 276, twelve for twelve

    ws779 276 stubbed
    ws781 276 stubbed, 130 refusals, 64 crates   12 gone, 12 new

The twelve that went are the `lerpDouble(a, 0, t)` family and the cascade
binding. The twelve that came are one typo's worth: the backend suffixes a
float literal *receiver* with `_f64`, and a literal the front end had
already suffixed read `0.0_f64_f64`. Suffixed once now.

## ws782 — the suffix, once (276 → 264)

    ws781 276 stubbed
    ws782 264 stubbed, 130 refusals, 64 crates   -12, 0 new

Grouped, from ws747: **433 -> 264** stubbed, 183 -> 130 refusals, 64
crates throughout, and the render walk's structure is the reference's.

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

## ws783 — the abstract Iterable's walk (264 → 259)

    ws782 264 stubbed
    ws783 259 stubbed, 130 refusals, 64 crates   -6, 1 new

The six are the text-field code's `Characters` calls. The one new is
`TypedDataBuffer`, where a supertrait declares `iterator` too and the
default body's bare `self.iterator()` was ambiguous; it goes through this
trait explicitly now.

Grouped, from ws747: **433 -> 259** stubbed, 183 -> 130 refusals.

## ws784 — 258

    ws783 259 stubbed
    ws784 258 stubbed, 130 refusals, 64 crates   -1, 0 new

## run785 — the walk holds at 708/0, and the panic moves to a dead tail

    DART2RUST_OS=android, 708 lines, type-only differing: 0
    panic: `BaseTapAndDragGestureRecognizer._resetDragUpdateThrottle`

Type flow analysis proved the guard and left the rest of the body in
place; emitted, its `else`-less `if` landed in the function's tail
position (E0317) and the function was stubbed for it. Nothing after a
statement that always returns is emitted now -- by `_alwaysReturns`, which
already knew how to say it, so a nested block or a both-arms `if` counts
too.

## ws786 — 256 stubbed (was 258), 130 refusals, 64 crates

The dead-tail rule cleared exactly the two functions it was written for and
brought nothing new:

  - gestures_tap_and_drag.rs  base_tap_and_drag_gesture_recognizer_super__reset_drag_update_throttle
  - rendering_paragraph.rs    _update_selection_registrar_subscription

Both had a TFA-proved `return` with a live tail behind it, and the tail's
`else`-less `if` landed in the function's tail position (E0317).

## ws788/789 — the run ruler's panic was a silent no-op, not a stub

run787 aborted in `RenderTapRegionSurface.unregisterTapRegion`
(`Option::unwrap()` on a `None`) after 6 frames and a walk that still holds
at 708 lines / 0 type-only differences. The cause was two lines earlier and
in a different method: `_groupIdToRegions[region.groupId]!.add(region)`
read the set *out* of the map, added to the copy, and dropped it. The group
in the map stayed empty, the next unregistration read it as empty and
removed the key, and the one after that found no key at all.

Dart's `[]` hands back the object the collection holds; a `Vec` and a `Map`
here hand back a value. So a mutating call on a value read out of a
collection now acts on the collection's own place (`_heldSlot`):
`m[k]!.add(v)` is `map.borrow_mut().get_mut(&k).unwrap().add(v)`, and
`xs[i].push(v)` is `xs.borrow_mut()[i].push(v)` -- the outermost collection,
so `rawCells[y][x]` reaches `rawCells`. `Map::get_mut` is new in the
prelude. The mapslot fixture disagreed with Dart before (`0 0 [] []` against
`2 1 [7, 9] [8]`) and agrees now.

  - ws788: 257 (`RenderTable.assembleSemanticsNode`: `_heldIn` stopped at
    the inner index, so the local was not `let mut`)
  - ws789: **256**, 130 refusals, 64 crates -- the same stub set as ws786.

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

## ws790 — 253 stubbed (was 256), 130 refusals, 64 crates

The projection-edge rule cleared `AsyncSnapshot.nothing/withData/withError`.
The mixin-fill rule cleared the five `&mut Vec<Rc<dyn DiagnosticsNode>>`
errors outright -- those functions are still stubbed, but on a different and
much simpler ground: `Map.fromIterables` was missing from the prelude
(`SlottedContainerRenderObjectMixin.debugDescribeChildren` builds one from
`_slotToChild.values()` and `.keys()`, and four `debugDescribeChildren`
copies of it do too). Added.

Also fixed after the ws789 census: a `?.` on a projected `T?` spelled
`.as_ref()` on the associated type. `_nullAware` unprojected its receiver in
one of its four branches only -- not in the `Result` one, which is the one a
failing body takes -- and a flattened body handed back an
`Option<<T as DartNullable>::Or>`, which is one `Option` layer, not two.
Both go through `_plain` now. The projread fixture (`T? get value` read
through `?.`, and a `final T? held` field) agrees with Dart.

## ws791 — 247 stubbed (was 253), 130 refusals, 64 crates

  - `Map.fromIterables`: 4 `debug_describe_children` (cupertino_text_field,
    material_chip, material_input_decorator, material_list_tile).
  - the null-aware projection: `foundation_diagnostics.value_to_string`,
    `widgets_restoration_properties.to_primitives`.

433 -> 247 over the grouped method; refusals 183 -> 130; reachable crates 64
throughout.

## run792 / run794 — the ruler stopped being a panic and became a clock

With the tap-region no-op fixed, the run no longer aborts: it times out with
*no output at all* (the program dumps at the end of its budget, so a run that
never reaches the end says nothing). Two stack samples under gdb, taken at
the timeout, named two different quadratic reads.

**1. An indexed read cloned the whole collection.** run792 sat in
`ChangeNotifier.removeListener`, dropping a `Vec<Option<Rc<dyn Fn()>>>`:
`_listeners[i]` was `this_._listeners()?[i]`, and the accessor hands out a
*clone of the list*. Once per iteration in `removeListener`, once per element
in `addListener`'s growth loop -- O(n^2) with an allocation and n reference
counts in the inner step. An indexed read now borrows the cell the collection
is kept in (`_readPlace`, `_mutPlace`'s other half): `xs.borrow()[i].clone()`,
with the index bound first so its own reads happen before the borrow.
ws793 = 247, the same stub set as ws791.

**2. A map literal was built by scanning its own association list.** run794
then sat in `flutter_localized_locales`'s `nativeLocaleNames`, a 700-entry
const map that the *getter* rebuilds on every call -- and `Map::from_pairs`
called `insert` per entry, each `insert` scanning every entry already there:
245k `dart_eq`s per build, and the settings page builds one per locale.
`from_pairs` now buckets by `dart_hash_code`, so a key that hashes for real
costs one bucket and a key that does not degrades to the old scan.

Still open from this: our `Map` is an association list, so every lookup is
O(n); and a Dart `const` collection is one canonical object, which this
output rebuilds at each mention. Either would be a bigger win than the
literal's construction.

Four more rules came out of the ws793 census while the run was measured:
`~x` on an int (`DartInt::bit_not`, 3); `x is T?` admits null and its
promotion keeps the absence (nested's `SingleChildWidgetElementMixin.mount`,
4); a tear-off whose extra parameters are all optional adapts to the slot
that takes none -- instance tear-offs included, in the *type's* named order,
holding the receiver (`Timer(delay, _controller.reverse)`, 8).

## ws795 — 234 stubbed (was 247), 130 refusals, 64 crates

`x is T?` was worth 13 on its own, and not only where it was found:

  - nested.rs: 4 `mount` (`if (parent is _NestedHookElement?)`)
  - painting_shape_decoration, painting_flutter_logo, painting_box_border:
    5 `lerp_from`/`lerp_to`/`box_border_lerp` -- upstream writes these as a
    `switch (a) { ShapeDecoration? _ => .. }`, whose nullable *pattern* is
    the same test, and the missing `Some` those had at ws786 was the
    promotion, not the argument edge
  - rendering_object.update_children
  - `~x`: services_raw_keyboard_linux (2), crypto's sha256

433 -> 234 over the grouped method.

## run796 — the run finishes again: 7 frames, 46 platform messages

`Map::from_pairs` unblocked it. The run now aborts on a real panic instead
of the clock:

    scc_flutter_widgets/src/animation_tween.rs:952
    tween_super_lerp::<TweenImpl<f64>, f64>  ->  Option::unwrap() on a None

`Tween.lerp` is `(begin as dynamic) + ((end as dynamic) - (begin as
dynamic)) * t`, and `begin` is a projected `T?`. The cast to `dynamic` was
*dropped* -- the lowering's last resort is the operand itself -- so the
value kept its `<T as DartNullable>::Or` spelling while its recorded type
said `dynamic`, and the `dynamic` operator rules asked that associated type
for its `Any`. A cast to `dynamic`/`Object` now goes through `coerce` like
any other crossing; a value already behind the handle coerces to itself.
The tweenlerp fixture (`transform(0)/transform(0.5)/transform(1)`) agrees
with Dart.

The walk at run796 is 450 lines against the settled reference's 708 -- the
process aborts in `_MaterialInteriorState`'s implicit animation before the
tree settles, so this is the panic's shadow, not a structural regression.
run787 (the last run that reached the end) was 708/0.

## ws797 — 230 stubbed (was 234), 130 refusals, 64 crates

  - the function-holding field's `dart_eq`: widgets_app, material_app,
    cupertino_tab_view `eq`
  - the scalar step parameter: material_paginated_data_table.build,
    studies_shrine_shopping_cart, cupertino_picker_demo, material_about (2)

Four regressions came with the step rule and are fixed on top:
`|__p_r#box|` is a prefixed identifier, which Rust 2021 reserves (the
temporary is named from the identifier now), and a source that already
hands values out -- `iter().cloned()`, which `for_each`, `any` and `all`
take -- has nothing to deref.

## ws798 — 226 stubbed (was 230), 130 refusals, 64 crates

The four the step rule had broken, and nothing else moved.
433 -> 226 over the grouped method; refusals 183 -> 130; reachable crates 64
throughout.

## run799 — the Tween panic is gone; the clock moved twice

The `as dynamic` fix cleared `Tween.lerp`, and the run went back to timing
out with no output. Two gdb samples, one per attempt:

  1. `InheritedElement.setDependencies` -> `Map::insert` -> `Map::at`, the
     association-list scan. `_dependents` has one entry per element
     depending on a `Theme` or a `Localizations`, and every
     `dependOnInheritedElement` scans it. `dart_eq` on an `Rc<dyn Element>`
     is identity (`std::ptr::addr_eq`), so this is the scan itself.
  2. `ImageStreamCompleter.removeListener` -> `Vec<ImageStreamListener>::
     clone`, from the accessor: `_listeners.length` in the loop condition
     cloned the whole listener list once per iteration.

(2) is fixed: a read that does not need the collection -- `length`,
`isEmpty`, `keys`, `values`, `m[k]` -- goes through the cell's `borrow()`
inside a block, so the `Ref` drops with the `let` that made it. That is the
read counterpart of `_mutPlace`, and the same shape the existing
`({ let __r = ..borrow().clone(); __r })` uses to keep a borrow short.

(1) is still open, and is the same question as before: our `Map` is an
association list, so every lookup is O(n). A hash index over
`DartEq::dart_hash_code` (identity for a counted class, a real hash for a
`String`) would fix it, keeping the insertion order a `Vec` gives.

## ws800 — 223 stubbed (was 226), 130 refusals, 64 crates

The projection family went: `Provider.of`, `RawRadio.value`, and the three
`_getOverrideAction`. The last of those was a *signature/prologue*
disagreement -- a mixin copy takes its parameter types from the declaration
it was copied from, and those name the declaration's own type parameters,
which `_projectedSlot` does not recognise as this one's; the signature came
out `Option<U>` and the prologue unprojected it anyway. The prologue reads
the same `_declaredParamTypes` the signature does now.

Two regressions came with the borrowed read and are fixed on top: binding
the key of `m[k]` to a local *moved* it, where the ordinary emission takes
it by reference (`SlottedContainerRenderObjectMixin._setChild`).

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

## ws801 — 221 stubbed (was 223), 130 refusals, 64 crates

The two the borrowed read had broken, and nothing else moved.
433 -> 221 over the grouped method.

## run802 — the Map index found a refusal that had never been called

4 frames, then

    material_theme_data.rs:1200
    _IdentityThemeDataCacheKey::hash_code
      -> dart2rust: not translated: unsupported call to top-level
         `identityHashCode`

`_IdentityThemeDataCacheKey.hashCode` is `identityHashCode(baseTheme) ^
identityHashCode(localTextGeometry)`, which this compiler refuses; the stub
it left had simply never been called, because nothing asked a key to hash
until `Map` started indexing. Two things follow, and both are the rule
rather than the case:

  - `DartEq::dart_hash_code` must not panic -- its default (`0`, or a
    counted handle's address) is consistent with any equality, which is
    what the protocol promises. A `hashCode` this class could not emit is
    no longer wired into it (`_stubbed`).
  - a map small enough never to have built an index must never ask its keys
    to hash at all: `insert` asks *inside* the index, so the five-entry
    `_FifoCache` behaves exactly as it did before.

## ws803 — 221 stubbed, the same set as ws801

The map index and the two rules that came with it are compile-neutral.

## run804 — the theme-cache panic is gone; the clock is a build now

No output inside the budget again, and this time the sample is not a hot
loop: `_MaterialState.build` -> `ColorScheme.shadow`, an ordinary build.
`ColorScheme` is a value struct of about a hundred fields here, and
`Theme.of(context).colorScheme` copies all of them at every mention -- which
is what a Dart *reference* to an immutable object costs nothing for. That is
the model question (a non-counted class has neither identity nor sharing),
not a rule the census can name, and it is the next thing worth measuring:
how much of the frame budget is spent copying themes.

Where the two rulers stand at the end of this stretch:

  - compile: 433 -> **221** stubbed, 130 refusals (from 183), 64 reachable
    crates throughout
  - run: the gallery gets past every panic the ruler has named so far --
    the tap-region no-op, the dead tail, `Tween.lerp`'s dropped `as
    dynamic`, the theme cache's untranslatable `hashCode` -- and the walk
    was last measured whole at run787: 708 lines against the reference's
    708, with **0** type-only differences.

## run807 — the walk is back, and it matches: 708 / 708, 0 type-only

    dart2rust: run budget spent with main pending; 1 timer(s) still active
    192 frame(s) drawn (0 panicked); 2821 platform message(s)
    render tree: 708 lines

    diff <settled reference> <ours>, ignoring size= and offset=:  0
    diff <settled reference> <ours>, as printed:                508

The runs since run796 produced *nothing* -- no tree, no frame count, no
report -- and the reason was the ruler, not the translation. `run_main`
checks its `DART2RUST_RUN_SECONDS` budget at the top of its loop, and it
only gets the loop back from `run_until_idle`; a program whose every frame
schedules the next never leaves `run_until_idle`, so the budget was never
reached and `timeout` killed the host before it could report. The budget is
a thread-local deadline now and the scheduler stops at it between tasks.

`runtime/src/lib.rs` also grew `DART2RUST_TRACE_FRAMES=1`: one line per
frame, begin and end, with the wall clock. That is what found this --

    frame  1 end after  71.703µs
    frame  6 end after 264.587336ms
    frame 380 end after 436.684488ms

-- and it says two more things worth having:

  - the gallery **never settles**: 381 frames and counting, one scheduling
    the next for ever. Upstream's does settle, so something keeps marking
    needs-build or needs-paint.
  - a frame gets **steadily more expensive**: 265ms at frame 6, 440ms at
    frame 380. Something accumulates per frame -- a listener list, an
    element's dependents, the inactive elements -- and that is the next
    thing the run ruler should name.

Against `ref_render_walk.txt` (a *first-frame* capture, 6 lines) ours agrees
on 5 of 6; the sixth is where a settled tree and a first frame part company,
as before.

## ws808 / run809 — the reading, through the documented commands

    bin/run_chain.sh:  221 stubbed, 130 refusals, 64 reachable crates
                       (the same stub set as ws803: the runtime and prelude
                        changes are compile-neutral)

    DART2RUST_OS=android DART2RUST_DUMP_RENDER_TREE=1 bin/run_main.sh:
                       exit 0, budget spent with main pending
                       191 frame(s) drawn, 0 panicked
                       2806 platform messages
                       render tree 708 lines, RenderErrorBox 0

    diff ref_render_walk_settled.txt walk, ignoring size=/offset=:    0
    diff ref_render_walk_settled.txt walk, as printed:              508

Every one of the 508 is a `size=`/`offset=` on a node whose *type* and
place in the tree are right, and they all trace back to the 32
`RenderParagraph`s that measure `Size(0.0, 0.0)`: the headless runtime
answers no `Paragraph::*` native, and the native bridge hands an instance
native no receiver, so a host cannot model per-paragraph state. Those two
are the text gap, unchanged and still the only thing between this walk and
the reference.

## ws810 -- a capture and a parameter cannot share a name, and a step of a chain is not a coercion site

The goal changed with this round: clear every stub *and* every refusal,
and prove each cleared group with a fixture that agrees with Dart while
the render-tree reading holds. So the census now has two halves. Stubs
come from `stubsNNN.txt.detail.txt`; refusals never appear there -- they
are `// NOT TRANSLATED:` lines in `.crate/src/*.rs`, and this is the
first round that counted them:

    130 refusals, by reason:
      32  a `List`/`Map` method with no prelude name (fold 8, reduce 5,
          removeRange 4, Map.map 3, indexWhere 3, followedBy 3,
          Map.removeWhere 2, skipWhile/lastWhere/fillRange/asMap 1 each)
       6  a constant `InstantiationConstant`
       6  a local function with no name
       5  a super call to something not translated
       4  a const instance of a class in another file
       4  a generic local function
       5  a closure capturing `this` in a mixin application
       3  `Map.map`, refused for insertion order
      ..  and a tail of ones

Two rules this round, each with its own fixture.

**A closure's captured field and its own parameter cannot share a name.**
A closure that only reads `final` fields copies them in as locals named
after the field (`IrClosure.captures`), and a parameter of the same name
shadows the copy -- so a read of the *field* found the parameter instead.
Dart has no such collision: there the field is `this.child`. The gallery's
`FadeInImagePlaceholder.build` writes `this.child ?? child`, and the copy
came out `let child = self.child.clone()` under a `move |child: Rc<dyn
Widget>|`. A parameter whose written name is one of the copies now takes a
temporary's name instead (`_paramName`); the reads follow, because they
find the name through `_temporaries` by identity. The capshadow fixture:
Rust printed nothing, Dart printed `param|field`.

**A step of an iterator chain is not a coercion site.** `xs.map<Shape>((x)
=> Sq(x))` renders `.map(|x| ..).collect::<Vec<_>>()`, and Rust reads the
closure's return type off the body -- so the implicit upcast the front end
put there (`dart_object(..)`, an `IrUpcast` left unspelled) never happened
and the chain collected `Vec<Rc<Sq>>` where `Vec<Rc<dyn Shape>>` was
declared. `IrUpcast.explicit` already says this ("inside a closure body
nothing expects a type"); it just was not asked at a step closure's
`return`. `_spellsReturn` now says so, and `_returned` spells the upcast.
The mapret fixture, in both its expression-bodied and block-bodied forms.

    bin/run_chain.sh:  211 stubbed (was 221), 130 refusals, 64 crates
    the 10 cleared: cupertino/material `AdaptiveTextSelectionToolbar
      .getAdaptiveButtons`, `TabBarView._updateChildren`,
      `FadeInImagePlaceholder.build`, `PointerEventConverter.expand`,
      `Element.describeElements`, `_MaterialGridListDemo.build`,
      `UserAccountsDrawerHeader.build`, `FlutterErrorDetails.new`,
      `_SpellCheckSuggestionsToolbar._buildToolbarButtons`
    no new stubs

## ws811 -- the `List` and `Map` methods the prelude never had

The refusal census said the largest single group was a table gap: 32 of
the 130 refusals were a `List` or `Map` method with no prelude name. This
round wrote them: `fold`, `reduce`, `removeRange`, `fillRange`,
`indexWhere`, `followedBy`, `skipWhile`, `takeWhile`, `asMap`, `toList`
(on something already a `Vec`) in `DartList`, `removeWhere` on `Map`, and
`lastWhere`/`lastWhere(orElse:)` through the same two-method shape
`firstWhere` already had. The listgaps fixture runs all twelve and agrees
with Dart.

    bin/run_chain.sh:  219 stubbed (was 211), 102 refusals (was 130), 64 crates

Refusals down 28; stubbed up 8, all of them members that now translate
and then fail to compile for reasons of their own -- the cost of turning
a refusal into a compile error, and the eight are the next census. Net
unfinished members: 341 -> 321.

## ws812 -- the prelude's own gaps, and a callback slot that only calls

The eight stubs ws811 added were the price of turning refusals into
compile errors; this round paid most of it back and took the "no method
named X" census with it. Two rules.

**The prelude was missing names Dart has.** `truncateToDouble`,
`toStringAsPrecision`, `String.runes`, `String.replaceAllMapped`,
`Pattern.allMatches` (the literal case as well as the regexp one),
`DateTime.isAtSameMomentAs`, `toList` on a `Vec`, and `toSet`/`firstWhere`
on a `Set` -- an `Iterable` method the set had no answer for. The
preludegaps fixture runs all of them against Dart, `toStringAsPrecision`
in its three regimes (exponential low, decimal, trailing zeros kept).

Two things were wrong beside the names. `_preludeFailing` is keyed by the
*Rust* name, and a method the front end maps by table arrives spelled
`first_where` while one the backend only snake-cases arrives spelled
`replaceAllMapped` -- the second kind never matched, so its `?` was never
appended. It is asked both ways now.

**A prelude callback slot that only calls what it is given is `impl Fn`,
and an `Rc<dyn Fn>` is not one.** A closure written at the call site is
already the closure; a tear-off `coerce` put behind an `Rc`, or a
function-typed local or parameter -- always a handle here -- is not.
`_preludeLends` names those slots, and the argument becomes the function
itself (`Rc::new(f)` -> `f`) or a loan of the handle (`&*h`, since
`&dyn Fn(..)` implements `Fn(..)`). The lentfn fixture.

    bin/run_chain.sh:  209 stubbed (was 219), 102 refusals, 64 crates
    11 cleared, 1 added

The one added was the rule reading a name it did not own: `_History`
declares its own `indexWhere`, whose Rust name is `index_where`, and its
signature takes the handle. `_preludeLends` now asks only on a receiver
whose class the library does not know.

## ws813 -- the names the CFE invents, a generic function torn off, and `Map.map`

The refusal census, taken properly this time (a marker's reason is on the
line under it when the member's own line does not carry one), named three
groups this round could answer.

**A local function the CFE invented has no name a human wrote.** `late
final x = ..` inside a body becomes a `#x#initializer()` local function
beside the cell and its flag, and `#` is not a character the backend can
carry. It gets the same `__tN` a temporary gets, by identity, and both
`LocalFunctionInvocation` and `VariableGet` find it again the same way --
which is exactly what `_declare` already did for the CFE's variables. The
lateinit fixture: `late final` read twice, computed once.

**A generic function torn off at a type is the function.** `sizes.reduce(
math.max)` arrives as an `InstantiationConstant` wrapping the tear-off,
and Rust's function items are named rather than instantiated at a value --
the slot's own type says which instantiation this is. `math.max`/`min`
needed one thing more: a *call* to them is an inherent method of the
receiver (`f64::max`, `Ord::max`), and a method is no name to hand on, so
the prelude grew `dart_max_of`/`dart_min_of` for the value form. The
mathvalue fixture, over both `double` and `int`.

**`Map.map` was refused for insertion order it no longer loses.** The
prelude's `Map` is a `Vec` of pairs, so the entries the transform returns
keep the order it returns them in, a later key replacing an earlier one.
`orderedMapMembers` is empty now. The mapmap fixture.

    bin/run_chain.sh:  208 stubbed (was 209), 88 refusals (was 102), 64 crates

Nothing new: all seventeen members that stopped being refused compiled.
The one stub cleared was ws812's regression, `_preludeLends` reading a
name it did not own.

## ws814 -- two expressions that now say what they are, and nothing moved

Two rules, both about an expression carrying its own Rust type so that a
slot can coerce it.

**`a ?? b` says what it is where its arms agree.** Untyped, nothing could
coerce it: `final Object o = a ?? b` with two `String` arms left the
handle off, because `coerce` asks the value's `rustType` first and there
was none. The ifnullobj fixture fails without the rule (no `dart_boxed`
around the `match`) and agrees with it.

**A chain says what it produces.** `xs.map(f)` was untyped, so a slot
coerced it against Dart's declared element and upcast a handle that was
already the trait (`dart_object(v) as Rc<dyn Widget>` on a `v` that was
one). Typing the chain by the closure's own return should have settled the
three `Rc<dyn Widget>: Widget` stubs.

    bin/run_chain.sh:  208 stubbed (unchanged), 88 refusals, 64 crates
    the stub set is identical to ws813's, member for member

It did not: the three are still there, and the chain rule changed nothing
anywhere. The coercion those three go through does not read the chain's
type, so typing it was inert -- and an inert rule is a rule with no
evidence, so it is reverted. The `??` rule stays: its fixture fails
without it, which is evidence of its own even though the gallery's stubs
do not happen to be that shape.

Recorded for the next census: `xs.map(f).where(g)` does not compile
(`filter` after `map` gets the item by value and the chain's trailing
`.cloned()` has nothing to clone) -- found by a fixture written for
something else, and not yet in any stub because no gallery member writes
it.

## ws815 -- a `dart:core` interface on a trait, and a null test with one answer

**A `dyn CharacterRange` could not be asked `current()`.** `CharacterRange
implements Iterator<String>`, and the prelude's `DartIterator` was only
ever an impl on the concrete classes -- so the trait the abstract class
becomes did not carry it, and every read through a handle was "no method
named `current`". It is a supertrait now, except where its arguments name
the class itself (`SourceSpan implements Comparable<SourceSpan>` inside
its own bound is a cycle). The iterface fixture.

**A null test on a literal null has one answer.** Type flow analysis folds
a value it proved always null into the literal, and both arms were lowered
anyway -- the dead one with nothing to infer its types from (`None
.as_ref().map(|it| ..)`). The branch that runs is the whole conditional
now. The nullfold fixture.

    bin/run_chain.sh:  211 stubbed (was 208), 88 refusals, 64 crates
    3 cleared (the `current` group), 6 added

The six were a second half of the interface rule that went too far:
forwarding impls were emitted for interfaces an *ancestor* listed, and a
forwarding impl calls an **inherent** method. Reached only through a trait
`self.compare_to(other)` names two candidates and neither wins; with
another arity (`moveNext(int count)` against the prelude's `move_next()`)
it is the wrong method outright. The impl is now emitted only where every
call it would make lands on a method this class declares.

Also tried and reverted: `identical` on a nullable *prelude* value (an
`Option<Vec<E>>`, four stubs in `package:collection`'s equalities) picking
the prelude's value form. It compiles and it is **wrong**: the value form
compares the addresses of the two bindings, and `identical(xs, xs)` on a
list came out `false` where Dart says `true`. A Dart list is a reference
and this compiler makes it a value; identity on one is not something the
model can answer, so those four stay refused rather than answered wrongly.

## ws816/ws817 -- a supertrait is an obligation on every implementer

ws815 left six new stubs from forwarding impls emitted for interfaces an
*ancestor* listed, and the fix looked obvious: emit the impl only where
every call it makes lands on a method this class declares. It was the
wrong fix, and the ruler said so in the loudest way it has:

    ws816:  27 stubbed, 88 refusals, **33 reachable crates**, 1 unstubbable

`CharacterRange: DartIterator<String>` as a supertrait is an obligation on
every implementer, and `StringCharacterRange` declares `moveNext([int
count = 1])` -- one argument, not the prelude's `move_next()`. With the
forwarding impl now (correctly) withheld, the obligation could not be met,
and the error landed in a `dart_cast` body outside any function. `stubs.py`
stubs *functions*; an error outside one is unstubbable, so `characters_-
below` failed to build and took every crate above it out of the workspace.
Half the program stopped being measured.

The supertrait is reverted. The widening and its `_canForward` guard stay:
they emit a forwarding impl for an interface an ancestor listed, where the
class has the method to forward to.

    ws817:  208 stubbed, 88 refusals, 64 reachable crates

The same count as ws814, with one member swapped inside it (`Board.current`
for `Board.iterator`). The `current` group is a stub again, and the lesson
is recorded above it: a rule that adds an *obligation* has to be measured
against the thing that must meet it, not only against the thing that asked.

## run818 -- the reading still holds

Eight rounds of translator changes since run809, so the other half of the
goal was measured before going further. The same commands, on ws817's
workspace:

    DART2RUST_OS=android DART2RUST_DUMP_RENDER_TREE=1 bin/run_main.sh:
                       exit 0, budget spent with main pending
                       198 frame(s) drawn, 0 panicked
                       2911 platform messages
                       render tree 708 lines, RenderErrorBox 0

    diff ref_render_walk_settled.txt walk, ignoring size=/offset=:    0
    diff ref_render_walk_settled.txt walk, as printed:              508

Unchanged from run809 except the frame count (191 -> 198, the run is
timed and the machine was less busy). Nothing the compile ruler cleared
has cost the run ruler anything.

## ws819 -- a function value handed to translated code is the handle

The backend spells *every* function-typed parameter of translated code
`Rc<dyn Fn(..)>` -- "one spelling, both sides", decided long before this
round -- so the front end's rule that lent the closure behind the handle
at a slot the callee "only calls" had nothing left to be right about. It
fired on a function-typed local and produced `&*layout_child.clone()`
where `RenderFlex._computeSizes` declares the handle. The rule is gone.

The loan belongs to the prelude, whose collection callbacks really are
`impl Fn`, and `_preludeLends` is where it lives: the list grew
`remove_where`, `retain_where`, `for_each` and `put_if_absent`, and a
closure literal that arrived boxed is unboxed at one of those slots.

The fnhandle fixture: a `Sizer` local passed to two methods that only call
it, beside a closure literal at the same slot.
    bin/run_chain.sh:  202 stubbed (was 208), 88 refusals, 64 crates
    7 cleared, 1 added

The one added was two prelude methods of the same name disagreeing:
`Set.removeWhere` declared `Rc<dyn Fn>` where every other callback slot
declares `impl Fn`, so unboxing the closure at it was the wrong shape. The
set's is `impl Fn` now, and a caller no longer has to know which
collection it holds.

Also this round, from the refusal census: `x is DateTime` and
`x is ByteData` answer through `DartCoreAs` like the prelude's exception
classes (3 refusals). The isprelude fixture.

## ws820 -- `x is DateTime`, and one convention for a callback slot

    bin/run_chain.sh:  201 stubbed (was 202), 85 refusals (was 88), 64 crates

Two small ones. `x is DateTime` and `x is ByteData` answer through
`DartCoreAs`, as the prelude's exception classes already did -- the
`is` lowering knew how to ask a prelude class and simply had not been
told about these two (3 refusals; the isprelude fixture, which also
checks that a `String` and an `int` still say no).

And `Set.removeWhere` declared `Rc<dyn Fn>` where every other prelude
callback slot declares `impl Fn`. Two conventions under one name meant a
caller had to know which collection it held; the set's is `impl Fn` now,
which is what ws819's one added stub was about.

Staged for the next reading: `package:collection`'s
`IterableExtensions.indexed` and `dart:async`'s `unawaited` (5 refusals,
and `.indexed` is on the build path -- it is what
`KeyedSubtree.ensureUniqueKeysForList` needs). The indexedext fixture
runs both. `unawaited` takes an `Option<DartFuture<T>>`: Dart declares
`Future<void>?`, and the argument arrives wrapped.

Also fixed here: the fixture harness ran the Dart side without a package
config, so a fixture importing `package:collection` "agreed" with Dart by
both sides printing nothing. It runs against the gallery's config now.

## ws821 -- `.indexed`, `unawaited`, and what identity cannot answer

    bin/run_chain.sh:  202 stubbed (was 201), 80 refusals (was 85), 64 crates

Five refusals cleared: `package:collection`'s
`IterableExtensions.indexed` and `dart:async`'s `unawaited`, both as
prelude functions under the names the CFE lowers their extension and
top-level forms to. The indexedext fixture runs both. `unawaited` takes an
`Option<DartFuture<T>>` -- Dart declares `Future<void>?` -- and is a no-op
that consumes the handle: `DartFuture` is a handle on a task the scheduler
holds, not the task, so dropping it does not cancel anything.

The one stub added is `_CupertinoDatePickerDateTimeState.build`, which
stopped being refused (it needed `.indexed`) and landed in the
`Rc<dyn Widget>: Widget` group -- the double upcast ws814 tried and failed
to settle. It is tried again this round from the other end: the chain
carries its element type *and* `toList()` on a chain carries the chain's,
which is the type a slot actually reads.

**`identityHashCode` was tried and reverted, for the second time and now
with the reason written down.** It looks answerable for a handle -- the
address is the identity -- and it is not, because a value class is boxed
into its trait object *at the call*: `Key(a)` and `Key(a)` box the same
`Named` twice and get two `Rc`s. The identhash2 fixture said
`k1.code == k2.code` was `false` where Dart says `true`. Identity in this
model belongs to counted classes only, and the four `hashCode`s that ask
for it (`ObjectKey`, `GlobalObjectKey`, `_HeroTag`,
`_IdentityThemeDataCacheKey`) hold values. They stay refused: this is the
same wall `identical` on a list stops at, and the same answer.

`Map.addEntries` did land (the addentries fixture): each entry inserted in
order, a later key replacing an earlier one.

## ws822 -- `Map.addEntries`, and two rules that measured to nothing

    bin/run_chain.sh:  202 stubbed (unchanged), 79 refusals (was 80), 64 crates
    the stub set is identical to ws821's, member for member

`Map.addEntries` landed (one refusal, the addentries fixture). The other
two things tried this round both measured to nothing, and both are worth
writing down because each looked certain.

**The chain's type, second attempt.** ws814 typed the chain and nothing
moved; the diagnosis then was that the coercion reads the *`toList()`*
expression, not the chain. So this round typed both -- and nothing moved
again, member for member. Whatever puts the second upcast on
`_CupertinoDatePickerDateTimeState.build`'s children does not read either
type. Reverted a second time. What is now known about the group: the inner
step closure already spells `as Rc<dyn Widget>` (so `IrClosure.returns` is
the trait), and something still coerces the collected `Vec` element by
element into the same trait. The next attempt should find that coercion
rather than guess at its input; `_receiver`'s coercion of a receiver to its
Dart static type is the untested candidate.

**`xs.last = v` on a field, and why it stayed refused.** The prelude has
no `first =`/`last =`, so `DiagnosticsNode.write` is a stub. Adding them
compiles -- and the listends fixture then read `[1, 2, 3]` where Dart
reads `[70, 2, 7]`: a setter's receiver does not go through `_mutPlace`,
so the write lands in a *copy* read out of the cell. A compile error
became a silent wrong answer, which is the one trade this compiler does
not make, so the methods are reverted and the stub stands. The fixture is
the evidence for whoever routes `IrSetter` through the mutating-call path:
a local (`local.last = 9`) is already right, only a field is not.

## ws823 -- `x is Function` is a test on a signature, not on being a function

    bin/run_chain.sh:  203 stubbed (was 202), 78 refusals (was 79), 64 crates

One refusal cleared and one stub added, and then both reverted, because
the rule was answering a different question from the one asked.

`x is Function` looked like the easy end of the `is` census: the prelude
makes function objects, so asking whether an object is one is a downcast
to `DartFunction`. The fixture agreed (`fn fn text other`). But the `is`
lowering is handed a *name*, and every function type -- `void Function()`,
`String Function(String)`, `_ListStringArgFunction` -- arrives under the
name `Function`. So the rule answered "is it a function at all" where Dart
asked "does it have this signature", and `dart:ui`'s `_runMain`, whose
whole body is

    if (userMainFunction is _ListStringArgFunction) { userMainFunction(args); }
    else { userMainFunction(); }

took the one-argument branch for a zero-argument `main`. The fnpromote
fixture caught it: `run(() => 1, 'hi')` returned `one:` where Dart returns
`none`, and the adapter then threw a `StateError` on the arity.

Arity alone would discriminate both gallery sites, and arity alone is a
guess -- Dart checks the parameter and return types too. So `is Function`
is reverted with the promotion rule it needed (a function-typed promotion
has to build the closure that calls the object dynamically; correct in
itself, and unreachable without the test that motivated it). The refusal
stands, and what it needs is written down: the `is` lowering has to carry
the function *type*, not the name `Function`.

The tree is back at ws822's translator, whose reading is 202 stubbed,
79 refusals, 64 reachable crates.

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

## ws826 -- an enum the tree shaker emptied is not a refusal

    bin/run_chain.sh:  202 stubbed (unchanged), 71 refusals (was 79), 64 crates
    the stub set is identical to ws824's, member for member

The marker on an empty enum said "either an enhanced enum this compiler
refused, or one the tree shaker emptied" -- two different situations under
one word. Kernel can tell them apart: an enum's elements are `Field`s with
`isEnumElement`, and a class the shaker emptied has none. Checked before
the rule was written, on `app_aot_sig.dill`, for all eight:

    _StateLifecycle  isEnum=true fields=0 enumElements=0
    KeyboardLockMode isEnum=true fields=2 enumElements=0
    SelectionResult, SelectionEventType, SelectionExtendDirection,
    TextGranularity, SmartManagement, PathOperation -- all fields=0

Not one is an enhanced enum this compiler refused. Every one of them was
emptied by the shaker, and the *program that was shaken* cannot make one
of these either -- an uninhabited Rust enum is exact, which is what the
old marker's own comment already said ("no value of it is ever made, and
any code that tries does not compile"). `IrClass.enumElementsDeclared`
carries the distinction now: elements still declared and no values
recovered is a refusal and says so; elements gone is a fact about the
input, and the emitted enum says that instead.

Nothing about the output changed -- the same uninhabited enum, byte for
byte apart from the comment. What changed is that the census no longer
counts eight of the compiler's own diagnostics as work left to do.
`KeyboardLockMode.findLockByLogicalKey` is still refused, and correctly:
that member *is* in the program and cannot be translated without values.

## ws828 -- N catch clauses are one catch that dispatches on the type

    bin/run_chain.sh:  202 stubbed (unchanged), 70 refusals (was 71), 64 crates
    the stub set is identical to ws826's, member for member

Two rules, each with a fixture.

**`try { } on A catch (e) { } on B catch (e) { }` is one catch whose
handler dispatches.** Written as the nesting rather than as a new IR node,
because that is what Dart's clauses *are*: the clauses are tried in order,
the first matching guard handles it, an unguarded one catches everything,
and none matching is the error going back out -- which is `rethrow`, and is
what the chain's base is. Each clause's own variable is bound inside its
branch, narrowed the way a typed catch's binding already is (a trait by the
cast, a struct by `Any`): `coerce` leaves a prelude class alone, and `let
e: StateError = __caught` did not type. The multicatch fixture runs four
paths through a three-clause try and one error that no clause takes.

**A `try` whose body only throws had a `()` where its value goes.** The
`Ok(())` arm of the match is unreachable then, and the handler's every path
returns, so the match is the method's tail and `{}` is a unit. Proved
pre-existing first, with singlecatch_tail: a *single* typed catch on a body
that only throws fails exactly the same way. The arm is `unreachable!` now,
which is `!` and coerces to whatever the tail wants -- the same thing the
`flows` arm two lines up has always said for returns.

Refusals moved 71 -> 70 rather than 68: both multi-clause `try`s translate
now, and one of the bodies they let through (`IOClient.send`) reaches an
`is` against a class this compiler does not have, which is the group next
door.

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

## ws830 -- `jsonEncode`, by the value's own type

    bin/run_chain.sh:  203 stubbed (was 202), 69 refusals (was 70), 64 crates

`dart:convert`'s `jsonEncode` was the last of the top-level functions that
needed nothing new to be *possible*: `JsonCodec.encode` was already there,
reached through `json.encoder`. What it needed was to be asked the right
way. The codec's own path boxes the value and walks a table of shapes,
downcasting one by one -- and a `Vec<i64>` behind a fresh handle is not
found by it (the fixture panicked, "JsonCodec.encode of a Vec"). A
top-level `jsonEncode(x)` has `x` with its Rust type in hand, so the
prelude's `json_encode` takes `V: JsonPiece` and lets the compiler pick
the shape. The jsonenc fixture: a map of mixed values, a list, a string,
a map of strings.

The stub it added is the member that stopped being refused
(`NavigatorState._afterNavigation`) meeting the `toEncodable` slot: Dart
declares `Object? Function(Object?)`, and an `Object?` here is the handle
whose null is the `Null` object, not an `Option`. Fixed in the same round
by spelling the parameter the way the caller writes it.

## ws831 -- the `toEncodable` shape

    bin/run_chain.sh:  202 stubbed (was 203), 69 refusals, 64 crates

ws830's one added stub, cleared by spelling `toEncodable` the way the
caller writes it: `Object? Function(Object?)`, and an `Object?` here is the
handle whose null is the `Null` object rather than an `Option`.

## ws832 -- `const Stream()`, `stdout`, and a generic local function that needs its call site too

    bin/run_chain.sh:  203 stubbed (was 202), 64 refusals (was 69), 64 crates

Five refusals cleared by two small rules, both tables of the kind the
prelude already keeps.

**`const Stream()`** is `dart:async`'s abstract base constructor, which
carries nothing, and a stream with no events is exactly what the prelude's
ready stream is when nothing filled it. `_preludeConstInstances` maps it,
and only for a constant with *no fields*: one that carries some is a
different object and the shapes would have to agree.

**`stdout` / `stdin`** are getters the CFE lowers to calls, and
`supportsAnsiEscapes` is all the gallery asks of them. A program with no
terminal answers no, and so does the prelude; the stdioansi fixture agrees
with Dart under a pipe, which is the same answer for the same reason.

The stub added is `LicenseRegistry.licenses`, which stopped being refused
and now wants `StreamController` -- a real one, with listeners and a queue.
Recorded, not attempted.

**A generic local function was tried and reverted.** A Rust closure cannot
be generic and a nested `fn` cannot see the enclosing locals one reads, so
the shape that fits is the one covariant class parameters already use:
erase the parameter to its bound and convert at the call. Erasing it puts
the right signature on the declaration -- `effective` took `Rc<dyn
Fn(Option<Style>) -> Result<Option<Rc<dyn Object>>, _>>` -- and left the
call site unadapted: the argument closure still returned `Option<f64>`,
and the result still came back as `Option<Rc<dyn Object>>` into an
`Option<f64>` binding. `_argument` with no callee does not set the expected
return, which is where the adaptation would have come from. The refusal
stands with that written next to it.

## run834 -- the reading, again

    DART2RUST_OS=android DART2RUST_DUMP_RENDER_TREE=1 bin/run_main.sh:
                       exit 0, budget spent with main pending
                       203 frame(s) drawn, 0 panicked
                       render tree 708 lines, RenderErrorBox 0

    diff ref_render_walk_settled.txt walk, ignoring size=/offset=:    0
    diff ref_render_walk_settled.txt walk, as printed:              508

Twelve more rounds since run818 and the reading has not moved: 708 lines,
no type differing, nothing panicking. The compile ruler in the same tree
reads 203 stubbed, 64 refusals, 64 reachable crates.

From the start of this stretch: 221 stubbed and 130 refusals, so 351
unfinished members down to 267.

## ws836 -- a rule that changed nothing, and where the generic local function stops

    bin/run_chain.sh:  203 stubbed, 64 refusals, 64 crates
    the stub set is identical to ws832's, member for member

The generic local function was tried a second time and got further. The
first attempt failed because a generic function *type* carries **structural**
parameters, not the `TypeParameter`s the declaration erased -- asking the
erased set about them is always no, so the call site never took the erased
path at all. With that fixed the declaration and the argument closure
agree (`Fn(Option<Style>) -> Result<Rc<dyn Object>, _>`), and what is left
is the older question of which `dynamic` an erased `T?` is: the
declaration's slot spells `Option<Rc<dyn Object>>` and the body's returns
spell `Rc<dyn Object>`, because a `dynamic` carries its own null.

That produced a rule worth having on its own -- a `dynamic` that is still
an `Option` meeting one that is not, which the front end does by hand in
three places (`??`'s arm, `?.`'s receiver, and this) -- so it was measured
alone. It changed nothing: the same 203 stubs, member for member. Inert,
so it is reverted with the rest, and both are written down here for the
round that does the generic local function properly, which needs the
declaration, the call site and the two spellings of `dynamic` settled
together.

## ws838 -- a local function that is only called may borrow

    bin/run_chain.sh:  204 stubbed (was 203), 60 refusals (was 64), 64 crates
    3 stubs cleared, 4 added; refusals down 4

A closure that reaches `this` has been refused unless it could copy the
`final` fields it reads, because a function value here is an `Rc<dyn Fn>`
and a `'static` closure cannot borrow. A *local function* whose binding is
only ever **called** is not that: it never outlives the body it is written
in, so it is a plain `let f = |..| ..` and may borrow what it reads --
including `this`. The member's whole body decides: a read of the binding
anywhere in it (passed on, stored, torn off) puts it back behind the
handle. `IrLocalFunction.lends`, and the lendlocal fixture -- a local
function inside a mixin that calls a method on `this` and changes its
fields, agreeing with Dart on the list, the total and the log.

That cleared five refusals of `popOrInvalidate` inside
`DirectionalFocusTraversalPolicyMixin._popPolicyDataIfNeeded` and the three
stubs whose callers could not find the method. Four stubs took their place,
and they are a smaller question than the one they replaced: the method is
`&mut self`, so the `move` closure moves the `&mut` rather than reborrowing
it, and the super function's `this_: &__Self` cannot reach a `&mut self`
trait method. The refusal was "this cannot be expressed"; what is left is
"this needs a reborrow, and super functions need to know when a method
mutates".

## ws840/ws841 -- the reborrow, and the binding that holds it

    ws840:  204 stubbed, 60 refusals   (the reborrow landed, `let mut` did not)
    ws841:  202 stubbed, 60 refusals, 64 crates

Two steps to finish what ws838 started. A lending local function's closure
is a `move` one -- it owns the locals it copied in -- and a `&mut Self` is
not `Copy`, so it *moved* `self` instead of borrowing it. The closure now
moves a reborrow bound just before it (`let __mut_me = &mut *self;`), which
lasts exactly as long as the closure does; ws840 proved that half by the
error changing from "borrow of moved value" to the next one. That next one
was the binding: a closure that borrows anything mutably is an `FnMut`, and
calling one wants `let mut`. Both fixtures (a mixin's local function, and
one inside a method that writes a field) agree with Dart.

    from ws836:  203 stubbed and 64 refusals -> 202 and 60

What is left of the group is the two *super functions* of the same method:
a super function's receiver is `this_: &__Self` by design, and
`invalidateScopeData` is `&mut self` because an implementer writes a field
in it. That is the same question the group started from, one level up: a
super function needs to know when the method it holds mutates.

## run842 -- the reading, after the borrowing rules

    DART2RUST_OS=android DART2RUST_DUMP_RENDER_TREE=1 bin/run_main.sh:
                       exit 0, budget spent with main pending
                       201 frame(s) drawn, 0 panicked
                       render tree 708 lines, RenderErrorBox 0

    diff ref_render_walk_settled.txt walk, ignoring size=/offset=:    0
    diff ref_render_walk_settled.txt walk, as printed:              508

The compile ruler in the same tree: 202 stubbed, 60 refusals, 64 reachable
crates. The borrowing rules changed how a local function reaches `this`
across the whole program and the walk is unchanged, node for node.

From the start of this stretch -- 221 stubbed and 130 refusals -- 351
unfinished members are 262.

## ws843/ws845 -- three ways of asking the same null question, none of them the right one

    ws843:  202 stubbed, 60 refusals, 64 crates   (unchanged, member for member)
    ws845:  202 stubbed, 60 refusals, 64 crates   (unchanged, member for member)

`WidgetStateTextStyle`'s constructor emits

    font_family: if None.is_none() { None.clone() }
                 else { Some(format!("packages/{}/{}", None.clone().unwrap(), ..)) }

-- a test on something the AOT compiler already folded to null, with a dead
`else` whose `None.as_ref().map(|it| ..)` has no element type to infer.
Three of the eight "type annotations needed" stubs are this.

ws815 folded a `ConditionalExpression` whose condition is `EqualsNull` over
a *literal* null. ws843 widened that to the operand's static type being
`Null`. ws845 asked instead what the condition *lowered to*, an `IrIsNull`
over an `IrLiteral('null')`, lowering it once so nothing is evaluated
twice. All three are inert here: the stub set did not move by one member.

So this `if` is not reaching the `ConditionalExpression` branch at all, and
the next attempt should start by finding which lowering emits it rather
than by widening the test again. Recorded with the shape above, which is
the thing to search the output for.

## ws847 -- a `ByteBuffer` is its bytes, and a narrow number prints as itself

    bin/run_chain.sh:  201 stubbed (was 202), 60 refusals, 64 crates

`asByteData` and the rest of that family are declared on `DartByteBuffer`,
which the `Vec<u8>` a typed list is here implements; a `ByteBuffer` holds
those bytes and now forwards to them. One stub (`HashSink._finalizeData`).

The fixture written for it found something else, which is the better half
of the round: `'$bytes'` on a `Uint8List` printed
`[Instance of 'int', Instance of 'int', Instance of 'int']`. The narrow
numbers -- `u8`, `i8`, `u16`, `i16`, `u32`, `i32`, `u64`, `usize`, `isize`,
`f32`, and `char` -- were declared through `dart_any_named!`, which gives a
runtime type and no `dart_to_string`, so every one of them fell through to
the blanket `Object`'s "Instance of". They go through `dart_any_display!`
now, like the `i64` and `f64` beside them. Compile-neutral, and a silent
wrong answer fewer: nothing in the stub set moved for it.

## run848 -- the reading, after `toString` changed for ten types

    DART2RUST_OS=android DART2RUST_DUMP_RENDER_TREE=1 bin/run_main.sh:
                       exit 0, 201 frame(s) drawn, 0 panicked
                       render tree 708 lines, RenderErrorBox 0
    type-only differences: 0     as printed: 508

Worth its own reading because `dart_to_string` changed for ten types this
round, and a `toString` is the kind of thing a diagnostics walk prints.
It did not move: 708 lines, no type differing, nothing panicking.

    the compile ruler in the same tree:  201 stubbed, 60 refusals, 64 crates

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

## ws850 -- `is` against a function type is a downcast, not an arity guess

    bin/run_chain.sh:  201 stubbed (unchanged), 59 refusals (was 60), 64 crates

ws823 refused `x is Function` because the `is` lowering is handed a *name*,
and every function type -- `void Function()`, `String Function(String)`,
`_ListStringArgFunction` -- arrives under the name `Function`; answering
"is it a function at all" made `dart:ui`'s `_runMain` hand a zero-argument
`main` the argument list. The IR carried the type all along, in
`IrType.parameters`; what was missing was something exact to ask.

The prelude has it. A function object keeps the handle it was made from
(`original`, there so that `removeListener(f)` finds what `addListener(f)`
stored), and that handle's Rust type *is* the Dart signature translated. So
`x is R Function(A)` is `dart_is_function_of::<dyn Fn(A) -> Result<R, E>>`,
a downcast, and a bare `Function` is `dart_is_function`.

Making it exact needed the boxing site to spell the same type: the binding
inside `_dynamicFunction` is declared with the function's own type now, so
a tear-off is an `Rc<dyn Fn(..)>` rather than the `Rc<{fn item}>` it used
to infer, and a closure that arrived boxed is unboxed first rather than
becoming an `Rc<Rc<..>>`. `dart_function_same` asks the same question and
gets the same answer, so it is more often right too.

With the test exact, the promotion ws823 reverted with it comes back: after
`f is OneArg`, `f(arg)` is the function value. The isfnsig fixture -- a
one-argument tear-off, a zero-argument one, and a closure of neither
signature -- and sixteen existing function-value fixtures beside it,
because this changed how every function reaches an `Object` slot.

## ws852 -- a prelude generic answers `is` by the runtime type it reports

    bin/run_chain.sh:  202 stubbed (was 201), 58 refusals (was 59), 64 crates

`x is Future` cannot be a downcast: a `DartFuture<T>` is boxed as the one
instantiation it happened to be, and the question is about all of them. The
blanket `runtime_type` on the prelude's struct answers it -- `DartFuture` --
and the isfuture fixture agreed with Dart on a handle holding a future, a
string and an int.

It bought one refusal and cost one stub, and the trade was worse than the
numbers say. The member it unblocked, `SynchronousFuture.whenComplete`,
asks `result is Future` of a `FutureOr<dynamic>`, and the prelude spells
Dart's sum as an *enum* whose blanket `runtime_type` reports `FutureOr`:
the test would have answered `false` for a future. It never got that far --
the member stopped compiling on the next line, `return this`, a class
returning itself where the prelude's future struct goes -- so what shipped
was a stub and not a wrong answer. ws853 has both halves.

## ws853 -- a class that *is* the prelude's future has no members of its own

    bin/run_chain.sh:  201 stubbed (was 202), 57 refusals (was 58), 64 crates

Since ws482 a class that implements `dart:async`'s `Future` and carries its
value *is* the prelude's future here: `_type` spells every value of one
`Future<T>`, and constructing it is `future_ready`. Both halves were in
place; the conclusion was not. Nothing in the program ever holds the
struct, so nothing can reach its members -- `f.then(..)` on such a value is
`DartFuture::then`, the prelude's. The front end was translating them all
the same, and the two that did not compile were being counted:
`SynchronousFuture.then`, refused for the `is Future<R>` its body asks, and
`whenComplete`, stubbed at ws852. They are not gaps. The class is skipped.

The second half is the `is` that dead member asked, which is a real rule
for the code that stays: `x is Future` where `x` is a `FutureOr<T>` asks
which case the sum holds, and the enum answers exactly --
`matches!(&x, FutureOr::Future(_))`. The runtime-type rule keeps the cases
it is right for, a handle.

The futureclass fixture is a `Now<T> implements Future<T>` in full --
`then`, `catchError`, `whenComplete`, `asStream`, `timeout` -- asked three
questions: whether a value of it is a `Future` (yes), whether a `FutureOr`
holding a future is (yes), and whether one holding an `int` is (no). Both
ends say `future future value`.

What this does *not* cover is the other class in the tree that implements
`Future`: `TickerFuture`, which is a `Future<void>` with no type parameter
and no value to be constructed with, so `_futureLike` does not hold and it
stays a class of its own. Five stubs wait on it -- three `await
controller.forward()`, two tear-offs of `reverse` into a `void Function()`
-- and clearing them means giving the prelude's future a delegate: a class
that implements `Future` becomes one through the only thing the interface
promises, its own `then`, called at the first poll rather than at the wrap
(`SynchronousFuture.then` hands back another one of them, and starting
eagerly would not bottom out).

run853 says nothing moved: 708 walk lines, 0 type-only differences, 508 as
printed, no `RenderErrorBox`, 200 frames drawn and 0 panicked. Removing a
class the program never held is invisible from the outside, which is the
point.

## ws854 -- a tear-off adapter returns into its slot, like any other value

    bin/run_chain.sh:  201 stubbed (unchanged), 57 refusals (unchanged), 64 crates

`Timer(touchDelay, _controller.reverse)`: the method takes a named
`{double? from}` and the slot takes none, so the front end writes an
adapter -- a closure of the slot's shape calling the method with the
default. It was declared to return what the *slot* returns and given a
body returning what the *method* returns, with nothing in between, and
`AnimationController.reverse` hands back a `TickerFuture` where the
`void Function()` has `()`. Two members of `RawTooltip` were stubbed on it.

Everything needed was already there: `coerce` drops a value into a `void`
slot (ws538), and the adapter is the one place that was not asking. It asks
now, and a return the rule cannot spell leaves the adapter as it was.

The numbers did not move, and the error did: both members now fail on the
*next* thing, a lifetime. The tear-off is made inside `show()`, a local
function that already holds `this` as `__me`, and the handle the adapter
takes (`__me.dart_self_ref().get()`) borrows that capture inside an
`Rc<dyn Fn>` the `Timer` keeps. That is the round after this one. Kept
rather than reverted because the fixture, not the chain, is the evidence
here: voidtearoff4 does not compile without it.

The voidtearoff4 fixture is that call in miniature -- a method with a named
optional returning a class, torn off into the prelude's `Timer` -- and it
fails to compile at HEAD and agrees with Dart with the rule. The twelve
tear-off fixtures beside it (tearopt, tearnamed, teardef, instear, ctortear,
supertear, tearcol, wheretear, voidslot, fnadapt, voidtearoff) still agree.

One of them does not, and did not before this round either: **erasedtear
loops forever**, at HEAD and here alike. Its `childAfter` asks
`children.indexOf(child)` on a `List<Sliver>` whose elements are boxed
values, `indexOf` does not find the child it was just handed, and the
`while (child != null)` walk never advances past the first. That is the
identity-on-a-value-class group (15 refusals, counted at ws849) showing up
as a *run* rather than a refusal, and it is the sharpest evidence yet for
what that group costs. Recorded, not fixed.

## ws855 -- an adapter holds `this` itself, or it borrows whoever does

    bin/run_chain.sh:  195 stubbed (was 201), 57 refusals (unchanged), 64 crates

Where ws854 left the two `RawTooltip` members: the adapter around
`_controller.reverse` compiled its body and would not live long enough. It
is made inside `show()`, a *local function*, which holds `this` as a
capture; the adapter read that capture to take its own handle, and a
closure that borrows what encloses it cannot be the `'static`
`Rc<dyn Fn>` a `Timer` keeps.

The frontend already gave the adapter the tear-off's free *locals* for
exactly this reason (ws793's note: "or it borrows the local the receiver
came from"). It gives it the handle on `this` now as well, which is the
other half of the same sentence -- and the backend, which makes a closure
a `move` closure when it owns anything, then binds `__me` at the adapter
rather than inside it.

Six stubs went, none came: the two tooltip members the two rounds were
aimed at, and four more of the same shape that neither round went looking
for -- `RenderObject._buildSemanticsSubtree`, `._createSemanticsNode`,
`._mergeSiblingGroup` and `RenderScrollable.assembleSemanticsNode`, each
tearing off a method with optional named parameters inside a closure that
holds `this`. ws854 and ws855 are one rule in two halves; the numbers only
moved when both were in.

The tearinfn fixture is that shape: a class holding a `Timer`, a method
with a named optional torn off inside a local function that also touches
`this`. It fails to compile at HEAD with the same "lifetime may not live
long enough" the gallery had, and agrees with Dart with the rule. Twelve
tear-off fixtures beside it still agree.

run855 holds the ruler: 708 walk lines, 0 type-only differences, 508 as
printed, no `RenderErrorBox`, 199 frames drawn and 0 panicked. Four of the
six members are semantics (`_buildSemanticsSubtree` and its neighbours), so
they now run where they used to panic, and the walk did not move.

## ws856/857 -- a closure into an optional function slot is spelled, by a binding and not a cast

    bin/run_chain.sh:  195 stubbed (unchanged), 57 refusals (unchanged), 64 crates

A closure going into a `T Function(..)?` slot is emitted `Some(Rc::new(..))`,
and Rust does not unsize a closure through the `Some` on the way into a
struct literal's field: the `Rc<{closure}>` stays one. The optfnslot fixture
is the shape (a `Field<T>` whose validator slot is erased, a `StringField`
handing it a closure) and it does not compile at HEAD.

ws856 spelled the type with a cast -- `Some(x as Rc<dyn Fn..>)` -- and the
chain answered 196 stubbed: one member added, none cleared. The cast is not
the same as a slot. `RenderObject._colorsWithinRect` hands a closure whose
body is `Ok(1)`, and where the slot had made that `1` an `i64`, the cast
left it the `i32` a bare literal defaults to. This is ws551's lesson again:
spelling a type takes inference away.

ws857 spells it as a *binding* instead -- `Some({ let __f: Rc<dyn Fn..> = x;
__f })` -- which unsizes the same way and still hands the literal its type.
The optfnint fixture is that guard: a `Holder(count: () => 1)` through an
`int Function()?`, which the cast broke and the binding does not.

The chain says 195 and the stub set is *identical* to ws855's: the rule is
inert on the gallery. The two members that look like this shape --
`TextFormField`'s `validator` and `onSaved` -- fail at the other site, a
null-aware `.map` whose closure the same reasoning would spell, and that is
a separate place to teach. Kept rather than reverted because the fixture,
not the chain, is the evidence: the gap is real and the cure is measured
not to cost anything.

### Where the next rounds are, at 195/57

Two shapes are characterised and not yet done:

  * **The other spelling site.** `TextFormField.validator` and `.onSaved`
    stop at `Option<Rc<{closure}>>` where `Option<Rc<dyn Fn..>>` goes, the
    same gap ws857 closed for `Some(..)` -- but at a null-aware `.map`,
    whose closure return `_nullAware` already knows how to spell
    (`spelled`) and did not here. Find why the body's type came back
    unspellable rather than widening the test.

  * **A covariant parameter keeps the base's Rust type.** Seven stubs in
    `collection_below/src/equality.rs`: `SetEquality.equals(Set<E>? e1,..)`
    overrides `UnorderedIterableEquality.equals(Iterable<E>? e1,..)`, the
    CFE writes `e1 as Set<E>` at the top of the body, and the downcast
    lands in a parameter the Rust signature spells `Option<Vec<..>>` --
    `Set` is its own struct here and an `Iterable` is a `Vec`. The Set into
    a List slot is a rule `coerce` already has (`to_list`, ws642); what is
    missing is that the covariant cast go through it.

And one that is a *run*, not a compile: **erasedtear loops forever**, at
HEAD and before this session's rounds alike -- `children.indexOf(child)` on
a list of boxed values does not find the child it was handed, so the walk
never advances. That is the identity-on-a-value-class group showing what it
costs at runtime rather than as a refusal.

## ws858 -- a null-aware map spells its return where the body cannot fail

    bin/run_chain.sh:  193 stubbed (was 195), 57 refusals (unchanged), 64 crates

The first of the two rounds the note above asked for. `_nullAware` already
worked out what the body's type is and whether it is worth spelling
(`spelled`, ws486), and then used it in one branch only: the one where the
body can fail, which has a `Result` to hang the annotation on. A body that
cannot fail got `.map(|it| ..)` with nothing said about what comes out, and
an adapter closure made in there unsized against nothing.

`DART2RUST_TRACE_NULLAWARE=1` -- a trace that has been in the file since
ws486 -- said the type was there all along: nine of these bodies are
closures with a spelled function type. The closure's own return says it
now, `.map(|it| -> Rc<dyn Fn(..)> { .. })`, which is the same mechanism
ws857 used and for the same reason: a declared type unsizes and still lets
inference through.

`TextFormField.validator` and `.onSaved` are the two members, both gone,
and nothing came back in their place.

No fixture reaches this one. The site is an *erased* accessor -- the field
is stored under `<Rc<dyn Object> as DartNullable>::Or` and read back at
`String?` -- and that erasure is a closed-world decision the whole gallery
makes; the mapfnslot fixture builds the same class shape by hand, agrees
with Dart, and does not produce the adapter at all. What stands in for a
fixture here is the emission read before and after (`material_text_form_
field.rs:264`, `.map(|it| ..)` becoming `.map(|it| -> Rc<dyn Fn(..)> ..)`)
and the chain's own answer: exactly the two members, nothing added.

## ws859/860 -- a trait that implements one of the prelude's has it above it

    bin/run_chain.sh:  189 stubbed (was 193), 57 refusals (unchanged), 64 crates

`CharacterRange` implements `Iterator<String>` and does not redeclare
`current`, so on `dyn CharacterRange` there was no such method and four
members that walk graphemes -- `_transposeCharacters`,
`_updateSelectionRects`, `truncate`, `getTrailingTextBoundaryAt` -- did
not compile. ws816 tried the obvious fix, the supertrait, and lost 33
crates; the reason was recorded at the time and is what this round starts
from.

Two halves, because the supertrait alone is what ws816 was:

  * **The forwarding impl fills the defaults.** `CharacterRange.moveNext(
    [int count = 1])` widens `Iterator.moveNext()`, and `_canForward`
    withheld the whole `impl DartIterator<String>` over the arity -- so a
    supertrait nothing satisfied took `characters_below` out, and with it
    everything above. It forwards with the declared default now
    (`self.move_next(1)`), which is what the interface means by the call
    and what `IrParam.defaultValue` has been carrying for translated
    bases since ws793.

  * **The prelude's interface is a supertrait, unless it names this trait.**
    The cycle the old comment warns about is `Comparable<Self>`;
    `Iterator<String>` is not one, and only `CharacterRange` gained a
    supertrait in the whole gallery.

The iterwide fixture -- an `abstract class Range implements Iterator<String>`
that redeclares `moveNext([int count = 1])`, walked through the interface --
does not compile at HEAD with the same "no method named `current`" the
gallery had. It also found what the chain could not: `r.moveNext()` through
the object names both the trait's and the supertrait's, with no inherent
method to win (E0034). So a call that *widened* the supertrait's is
qualified with the receiver's own trait, the way a name two translated
traits declare already is (ws462). A name merely inherited is left alone --
it is not ambiguous, and naming the subtrait for it would not resolve.
Both ends of the fixture say `a,b,c 3`.

run860 holds the ruler: 708 walk lines, 0 type-only differences, 508 as
printed, no `RenderErrorBox`, 196 frames drawn and 0 panicked. Four members
that walk graphemes now run where they used to panic, and the walk did not
move.

## ws861 -- a value widens under the `Option` only when it is in one

    bin/run_chain.sh:  186 stubbed (was 189), 57 refusals (unchanged), 64 crates

`Object.hash(.., fallback == null ? null : Object.hashAll(fallback), ..)`:
in the second arm Dart has promoted `fallback` to a `List<String>` and the
value in hand is a `Vec`, but `_widened` asked the *declared* type, saw
`List<String>?`, and widened the elements "under the `Option`" -- a
null-aware map over something that is not one. What came out was
`as_ref()` on a `Vec`, which names two `AsRef` impls and types nothing
(E0282), and the three members that hash a nullable list stopped there:
`TextStyle.hashCode` and two in `dart:ui`.

The rule now asks what is actually in hand (`lowered.rustType`) as well as
what was declared, which is the same distinction `_isTest` draws for a
promoted read (ws620): recorded nullable, already unwrapped.

The hashallnull fixture is that expression -- a class hashing a
`List<String>?` field through `Object.hash` and `Object.hashAll` under a
null check, compared for two equal values, two nulls, and one of each. It
does not compile at HEAD and both ends say `true true false`.

## ws862 -- the prelude's `DateTime` says what Dart's says

    bin/run_chain.sh:  184 stubbed (was 186), 57 refusals (unchanged), 64 crates

`DateTime.fromMillisecondsSinceEpoch(int ms, {bool isUtc = false})`, and the
prelude's took the milliseconds alone -- so `RestorableDateTime.
fromPrimitives`, which restores with the flag spelled, handed it one
argument too many. The class has an `is_utc` field and every other
constructor sets it; these two now take it, as Dart declares them.
`microsecondsSinceEpoch` was the same kind of gap next door: the field was
there and the getter was not, and a `dart:` class's field is read as a call.

The dtepoch fixture takes both constructors with the flag and without, and
reads `millisecondsSinceEpoch`, `microsecondsSinceEpoch` and `isUtc` back.
It does not compile at HEAD -- the same "takes 1 argument but 2 arguments
were supplied" the gallery had -- and both ends say
`1700000000000 false true 5 true false`.

## ws863 -- an `Option` hashes whatever it holds, not only a handle

    bin/run_chain.sh:  181 stubbed (was 184), 57 refusals (unchanged), 64 crates

`title.hashCode` where `title` is a `String?` is ordinary Dart -- `null`
has a `hashCode`, and the prelude has had the number for it since the
`Option` impl was written. What it did not have is a way to hash the value
inside: the impl asked for `RcHashCode`, which only a handle has, and
`Option<String>` found none. Three `hashCode`s stopped there, two of them
in the `IOSSystemContextMenuItem` family and one in `CupertinoRoute`.

Every value in this compiler answers `dart_hash_any` -- it is what `DartAny`
is for -- and a handle's own `hash_code` was already forwarding to exactly
that. The bound is `DartAny` now and the body asks it directly, so the
handle case is unchanged and the scalars work.

The nullhash fixture hashes a class with a `String?` and an `int?` through
`Object.hash`, comparing two equal values, one with nulls, and two nulls.
It does not compile at HEAD and both ends say `true false true`.

A neighbour it does *not* cover: `null.hashCode` written on the literal has
no type to infer (`None.hash_code()`, E0282). Nothing in the gallery writes
it and the fixture drops it.

run863 holds the ruler after ws861-863: 708 walk lines, 0 type-only
differences, 508 as printed, no `RenderErrorBox`, 198 frames drawn and 0
panicked.
