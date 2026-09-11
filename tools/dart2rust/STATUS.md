## 1. 文档与总目标
- 目标已从“翻译得完”改成“**跑得起来**”：把 gallery 翻成 Rust，跑在上游 Flutter engine 的 AOT 模式里。
- 宿主路线选 **A**：Rust VM 顶掉 `libdart`，engine 不改；B 是改 engine 的退路，退时要明说。

## 2. 核心模型
- 异常一律走 `Result`：`throw = Err`，`rethrow = Err(e)`，`try = match`，`finally` 在分派前跑。
- 错误/对象手柄最终统一到 `Rc<dyn DartAny>`，删掉注册表，协议走 vtable。
- 对象模型：`Rc<RefCell<T>>`、回边 `Weak`、身份令牌；值类身份仍是最大欠账之一。

## 3. 尺子与当前读数
- 编译链：`stubs.py` 量 stub / refusal / 可达 crate / error。
- 运行尺子：`dart_main` + `run_main.sh`，修启动路径第一个 panic。
- 渲染尺子：与 Flutter 参考遍历对账，参考 settled 为 708 行。
- **当前读数**：拒绝 **12**，桩 **36**，unstubbable **0**，可达 **69**，0 error；渲染树连采五次 **707 节点 / 0 panic / 0 类型差异**；夹具 **95 个，94 AGREE + `ffistruct` 故意红**。
- 体积：ws1086 收下 `--icf=all` 后，release `.text` **56,555,618**，stripped **113,003,496**；从 work_size 起点 `.text` 75,158,528 → 56,555,618（−24.8%），stripped 135,833,000 → 113,003,496（−16.8%）；对官方 `libapp.so` 的 `.text` 从 6.11× 降到 **4.60×**。

## 4. 已闭合/已落地
- Goal 2 无头引擎：能 build、layout、paint 出真帧。
- Goal 3 渲染树：节点类型、顺序、层级与参考一致，**708/708，忽略 `size=`/`offset=` 后 0 差异**。
- 剩余 508 行只差 `size=`：32 个 `RenderParagraph` 量成 `Size(0,0)`，因无头运行时不答 `Paragraph::*` native，且 native 桥不传接收者。**这是运行时缺口，不是翻译缺口。**
- `work.md` 六步全部落地：生成侧 `uncaught Dart exception` 16 → 0，prelude 32 → 0；`.unwrap()` 29,439 → **1,165（−96%）**。
- Object 协议第二版最终在 ws1005/ws1006 做成：删 `dart_register`，四张表归零。
- 体积优化落地：常量表、`--icf=all`、`force-unwind-tables=no` 等。

## 5. 最近撤回与主要欠账
- ws1088 D：擦除降体积四刀，桩 655 / 139 / 79 / 57，可达 39 / 67 / 66 / 66，未达基线 36 / 69，撤回。
- ws1089 B：`DartStr = Cow<'static, str>` 改造走了一大半，夹具 60 → 64 → 70 → 71，但 gallery 尾巴未开始，整轮退回。
- ws1087 量清：D 的真实上限 **3.07 MB（5.4%）**；B 的 l10n **19,437,406 字节，占函数符号 28.8%**。
- 最大挡路工程：**值语义 / 别名 / counted**。11 条拒绝同一决定；做它曾量到 **+901 桩**。
- 拒绝 12 中约 8 条今天拒绝得**正确**，所以“拒绝归零”不能字面达成。
- 桩尾已是长尾：最大簇也只有几个。剩下多要子系统：`StreamController`、HTTP 客户端、gzip/JSON、ffi 写路径、`ListBase`、擦除类型参数界、tear-off coercion。

## 6. 下一步优先级
1. **别名/counted + `List<T>` 表示**：`Rc<RefCell<Vec<T>>>` 是表示，`Rc<dyn DartList<T>>` 是槽拼法；先做普查。
2. prelude 接口成员经对象调用：`Comparable`、`DartIterator` 等经 `dart_cast_to`。
3. `Sink<T>` 进 `_preludeInterfaces`，补 `DartSink` 转发 impl。
4. 增强枚举每变体状态。
5. 运行期补：Paragraph native、Stream、HTTP、codec、ffi 回调。

**不做**：nightly 并行前端；按 SCC 拆 crate；翻译 `dart:core`。

## 7. 九条要记住的
零拒绝 ≠ 翻译好；`dart:core`/`dart:ui` 手写 prelude；仪器各有盲区；短路语义易静默错；夹具值要能分辨错配；变异要查断言对象；探针先查排序再查条件；拒绝升高可能是尺子变准；正则先看一条匹配；变异必须还编得过；数字必须带条件。

**一句话**：翻译与渲染结构已接近收尾，数字停在 **12 拒绝 / 36 桩 / 69 可达 / 707 行 0 差异**；再往下主要是运行时子系统与值语义/别名工程。
