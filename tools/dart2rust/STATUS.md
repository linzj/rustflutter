## 1. 文档与总目标
- 目标已从“翻译得完”改成“**跑得起来**”：把 gallery 翻成 Rust，跑在上游 Flutter engine 的 AOT 模式里。
- 宿主路线选 **A**：Rust VM 顶掉 `libdart`，engine 不改；B 是改 engine 的退路，退时要明说。

## 2. 核心模型
- 异常一律走 `Result`：`throw = Err`，`rethrow = Err(e)`，`try = match`，`finally` 在分派前跑。
- 错误/对象手柄最终统一到 `Rc<dyn DartAny>`，删掉注册表，协议走 vtable。
- 对象模型：`Rc<RefCell<T>>`、回边 `Weak`、身份令牌；值类身份仍是最大欠账之一。

## 3. 尺子与当前读数
- 编译链：`stubs.py` 量 stub / refusal / 可达 crate / error。
- 夹具尺子：`bin/fx.sh` / `bin/allfx.sh`，判据是 **panicgate**（panic 一律不算过，先于退出码查）。2026-09-11 起缓存**按每一步自己的输入分档**（`bin/fx/fingerprint.sh`），不是一把大锁：`fp_prelude` = `lib/prelude.dart`（它只声明一个 `const rustPrelude`，只被逐字写成 `dart_prelude.rs`，所以改它不可能改动别的产物）；`fp_code` = 翻译器其余部分；`fp_sdk` = 夹具源码那一端的输入（SDK、platform dill、`package_config.json`），**dill 与 Dart 那一端只认它**——`gen_kernel --aot --tfa` 是每个夹具 5.85s（占翻译 10.6s 的 55%），而后端动不了它一个字节。缓存戳从前读在翻译**之后**，只省了 cargo 那 3.5s。三档实测（104 个夹具，3 并发）：**什么都没动** 6m51 → **11.6s**（判据逐条相同，底线是故意红的 `ffistruct`，它失败就不写戳，每遍全跑）；**只动 prelude** 每夹具 14s → 5.2s；**动后端** 每夹具 10.25s → **4.67s**，整遍 8m25 → **2m55**。墙钟数要看机器多热才有意义——同样 33 分钟 CPU 的一遍扫描，一次 8m25、一次 22m39（跟在 release 构建后，页缓存冷，并行度 ~4.5 核掉到 ~1.7 核）；**跨机器状态可比的是 CPU 时间：33m00 → 11m35**。
- 运行尺子：`dart_main` + `run_main.sh`，修启动路径第一个 panic。
- 渲染尺子：与 Flutter 参考遍历对账，参考 `ref_render_walk_settled.txt` **707 行**（707 行全部非空；`render_ruler.py` 的 `nodes=` 数的就是它，`typediff` 含长度差，所以 0 差异同时保证行数相同）。**曾写作 708，那是旧数**：重量于 2026-09-11 ws1094，参考文件与五次运行都是 707。
- **panic 尺子 `bin/panic_ruler.py`（ws1094 新增）**：把工作区里**每一种会中止的写法**数全——`panic!`/`unreachable!`/`todo!`/`assert*`/`.expect`/`.unwrap()`——**先出总数再分类**，对不上就报 MISCOUNTED。它取代的是 `grep -c 'panic!("uncaught'`：那是一行的模式，而 `erased_cast_failed` 的字符串写在 `panic!(` 的下一行，所以尺子报 0、东西还在、六处在调它。**量一个字符串只会静默地错，量总数不会。**
  - **`a8055b16` 发现它自己在发牌照**：它把任何认不出的 `dart2rust:` 消息折进一个桶 `dart2rust: .. (other)`，并把那个桶算作合法，于是宣布规则收尾的那天它的最后一行写着 0，真数是 **516**。前缀是这个编译器写的，前缀后面说的话却未必是这个编译器的事实——`panic!("dart2rust: a {} where a `{}` was wanted")` 就是一个挂着本编译器名字的 Dart `TypeError`。桶已删，每条消息各按原文单列；**新出现的一条在有人替它辩护之前一律算不合法**。
- **当前读数（ws1101）**：拒绝 **12**，桩 **34**（ws1098 的 36 起算，gone 2 / new 0，`stubdiff.sh` 逐字节），unstubbable **0**，可达 **69**，0 error；渲染树连采五次 **707 节点 / 0 panic / 0 类型差异**；夹具 **107 个，106 AGREE + `ffistruct` 故意红**；panic 尺子 **「程序还能走、这边走不了」的中止点 = 0**。
  - 510 → 0 的去向，按处置分：**真删掉 504 处**——494 处懒 `late` 尾读（`protocols.dart` 改成 `match`，返回刚算出的值就不必读回来）、5 处 `four()`/`eight()` 把数组 `try_into` 成它自己、1 处 Mutex 换成全函数的 `into_inner`、2 处 `Future.value()`/`Future.delayed` 对非空 T 取 null 改 `Err`、1 处 `FutureOr::Value` 空着（和十行下的 `then` 给同一答案）、1 处 `Sink::close` 补上错误通道；**1 处改成本来的措辞**（`dart:ffi` 没有该平台的 Abi 行是一条拒绝）；**5 处逐条辩上名单**，每条 1 个站点，理由写在尺子里。
  - 名单的规矩写进了 `bin/panic_ruler.py`：**一条消息覆盖很多站点就不该上名单**。494 处懒格子是一个发射点，答案是别再发射中止点，不是给它取个名字。
- 体积：ws1086 收下 `--icf=all` 后，release `.text` **56,617,250**，stripped **112,917,488**；从 work_size 起点 `.text` 75,158,528 → 56,617,250（−24.7%），stripped 135,833,000 → 112,917,488（−16.9%）；对官方 `libapp.so` 的 `.text` 从 6.11× 降到 **4.61×**。ws1094 把 670 处 `.unwrap()` 换成 `?`/`Err` 之后量：`.text` +61,632（**+0.11%**），stripped **−86,008**——work.md 第十一节那条“没量”的账，现在量了：**`?` 的传播代码没有想象中贵，`Err` 构造还替掉了一批 panic 的格式化字符串。**

## 4. 已闭合/已落地
- Goal 2 无头引擎：能 build、layout、paint 出真帧。
- Goal 3 渲染树：节点类型、顺序、层级与参考一致，**707/707，忽略 `size=`/`offset=` 后 0 差异**。
- 剩余 508 行只差 `size=`：32 个 `RenderParagraph` 量成 `Size(0,0)`，因无头运行时不答 `Paragraph::*` native，且 native 桥不传接收者。**这是运行时缺口，不是翻译缺口。**
- `work.md`（2026-09-11 12:0x 版）七步全部落地：生成侧 `.unwrap()` 29,439 → 1,165 → **0**；最后留下的 495 处是懒初始化刚写完那一格的读，已改写成 `.expect("dart2rust: ..")`，把理由写进产物本身。prelude 的中止点 41 → 25 → **19**。手段是七条发射点上的通则（详见 ws1094 的提交说明），不是一处处包 `try`。
- **ws1094 那句“没有一处再说‘那个程序’的事”是错的**，错在尺子而不在手段：见上面 panic 尺子那条。真数是 516；ws1096 把 `dart_from_dynamic`（128 处调用）、`dart_function_of` 的三个提问者、`json_write` 的兜底和三处 Completer 改成值之后是 **510**，ws1098 把剩下的处置完是 **0**。顺手补上一个自己挖的坑：`DartFuture::map` 的回调槽是 `Fn(T) -> U`，于是 `dart_from_dynamic` 在它体内没有 `Result` 可走，冒出 4 个**匿名** `.unwrap()`——比有名字的 panic 更坏。改在槽上：回调可失败，它的错误就是这个 future 的错误，正是 Dart 对会抛的 `then` 的做法。
- Object 协议第二版最终在 ws1005/ws1006 做成：删 `dart_register`，四张表归零。
- 体积优化落地：常量表、`--icf=all`、`force-unwind-tables=no` 等。

## 5. 最近撤回与主要欠账
- ws1088 D：擦除降体积四刀，桩 655 / 139 / 79 / 57，可达 39 / 67 / 66 / 66，未达基线 36 / 69，撤回。
- ws1089 B：`DartStr = Cow<'static, str>` 改造走了一大半，夹具 60 → 64 → 70 → 71，但 gallery 尾巴未开始，整轮退回。
- ws1087 量清：D 的真实上限 **3.07 MB（5.4%）**；B 的 l10n **19,437,406 字节，占函数符号 28.8%**。
- 最大挡路工程：**值语义 / 别名 / counted**。11 条拒绝同一决定；做它曾量到 **+901 桩**。
- **12 条拒绝,逐条查过(ws1101)**。它们是 5 个机制,不是 12 件事;只有 4 条有一句关于「那个程序」的理由,其余 8 条是本编译器的欠账。
  - **dart:ffi / win32 windowing,4 条**(`_Win32PlatformInterface.initializeWindowing`、`_WindowingInitRequest.onMessage`、`_CallocAllocator.new`、`_CallocAllocator._fillMemory`)。四条都落在 dart:ffi 的 `external` 上:`_ffiCall` 是 AOT 编译器生成的本地跳板,`_createNativeCallableIsolateLocal` 带 `vm:external-name "Ffi_createNativeCallableIsolateLocal"`,`Uint8Pointer.operator[]=` 走到 `_storeUint8`——**kernel 里没有 Dart 体可译**,写一个体不是翻译这个程序,是在 prelude 里重做一遍 dart:ffi 运行时。外面那层也不是这个平台的:`_CallocAllocator()` 第一句 `DynamicLibrary.open('ole32.dll')`,`WindowingOwnerWin32()` 第二句 `if (!Platform.isWindows) throw UnsupportedError('Only available on the Win32 platform')`。`DART2RUST_OS=android` 下 Dart 自己也走不过去。这 4 条**留下**。
  - 跨文件 `const` 实例,**3 条**(`GZipCodec` ×2、`JsonEncoder`)。本编译器的欠账:一个 `const X(..)` 的类不在本文件时没被降下来。
  - `identical` 作用在非引用上,**2 条**(`VerticalCaretMovementRun.isValid`、`PageTransitionsTheme operator ==`)。欠账,并且和「值语义 / 别名 / counted」是同一件事:`Map`/`List` 在这边是值,没有引用同一性可比。
  - `super.==` / `super.hashCode` 落到 `Object`,**2 条**(`widgets_framework`)。欠账:prelude 有 `Object` 的这两个成员,是路由没接上。
  - 动态槽上的 `completeError`,**1 条**(`get_below/queue_get_queue`)。欠账:`DynamicInvocation` 这一族本身没翻译。
- 桩尾已是长尾：最大簇也只有几个。剩下多要子系统：`StreamController`、HTTP 客户端、gzip/JSON、ffi 写路径、`ListBase`、擦除类型参数界、tear-off coercion。

## 6. 下一步优先级
1. **别名/counted + `List<T>` 表示**：`Rc<RefCell<Vec<T>>>` 是表示，`Rc<dyn DartList<T>>` 是槽拼法；先做普查。
2. prelude 接口成员经对象调用：`Comparable`、`DartIterator` 等经 `dart_cast_to`。
   - **ws1102 量过一次,是个陷阱**。把 `Comparable<T>` 声明成 `DartAny` 的子 trait,`dart_cast_to` 就有了(`DartCastExt` 是对 `DartAny` 的 blanket impl),`DataTableDemo._sort` 也编译得过——但**运行时抛** `TypeError: type is not a subtype of type 'Comparable' in type cast`。原因:`class Score implements Comparable<Score>` 的 cast 表只登记了 `Comparable<Score>`,而代码要的是 `Comparable<Object?>`;Dart 靠协变直接过。
   - 试过的形状(Dart 答案 `-3|3|-1|1|s1,s2,s3`):`int compareCells(Object? a, Object? b) => (a! as Comparable<Object?>).compareTo(b);`,分别喂自家类、`int`、`String`,再用它做一次 `rows.sort`。
   - 所以这一条**不是**加个 supertrait 能了的:要让对象为**更宽的实例化**答话——为每个实现 prelude 泛型接口的类再发一个擦除实例化的 impl 并登记进 `dart_cast`。单加 supertrait 是把一个看得见的桩换成一个 Dart 不会抛的运行时错,按「任何 panic 都不算过」更糟,已撤回。
3. `Sink<T>` 进 `_preludeInterfaces`，补 `DartSink` 转发 impl。
4. 增强枚举每变体状态。
5. 运行期补：Paragraph native、Stream、HTTP、codec、ffi 回调。

**不做**：nightly 并行前端；按 SCC 拆 crate；翻译 `dart:core`。

## 7. 九条要记住的
零拒绝 ≠ 翻译好；`dart:core`/`dart:ui` 手写 prelude；仪器各有盲区；短路语义易静默错；夹具值要能分辨错配；变异要查断言对象；探针先查排序再查条件；拒绝升高可能是尺子变准；正则先看一条匹配；变异必须还编得过；数字必须带条件。

**一句话**：翻译与渲染结构已接近收尾，数字停在 **12 拒绝 / 34 桩 / 69 可达 / 707 行 0 差异 / 0 个"程序还能走"的中止点**；12 条拒绝已逐条查过，4 条留下并写明理由，8 条是欠账；再往下主要是运行时子系统与值语义/别名工程。
