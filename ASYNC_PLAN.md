# 给框架接一个执行器

分支 `async-executor`，从 `2ee69d7` 分出。

**八个阶段已全部落地**。结果记在 `PORTING_STATUS.md` 顶部的执行器一节，运行时的
全程画在 `docs/ASYNC_FLOW.md`；这份文件留作当时的推理记录。

| 阶段 | 提交 |
|---|---|
| 1–3 唤醒通道、`task.rs`、帧循环 | `6fcb71c`、`beb7be9` |
| 4 thread_local 守卫 | `6cd7bfe` |
| 5 future 门面 | `2179f26` |
| 6 `future_builder` | `5b34086` |
| 7 `post_delayed_task` + `sleep` | `e2a7c0d` |
| 8 台账 | 本次 |

**唯一没验的一格：GN / ninja。** 这个环境里 `src/flutter/buildtools` 与
`prebuilts` 是 gclient 管的，主 checkout 和 worktree 都没有，也没有 `out/`，
所以 `runtime_controller.cc` 的改动只到 `clang -fsyntax-only` 级别（头文件干净），
**没有真正编译过**。合并前要跑一次 `rustflutter_unittests` 与
`rust_ffi_unittests`。

上游 Flutter 的 `Future` 是同一根 UI 线程上的续延，不是并发；这个港口把它们译成了回调、
帧轮询、帧时钟 deadline 和拆开的同步/异步缝，四种形状都记在 `PORTING_STATUS.md` 里，
而且都是对的。缺的不是语义，是**组合**——三段以上的异步序列写成嵌套回调会难看，错误
得一层层手动往回传。

这条分支补上执行器，让 `.await` 在应用代码和框架边缘可用。

`runtime_controller.cc:358-362` 已经把入口标好了：

> Upstream drains the microtask queue between the two (`FlushMicrotasksNow`), and that
> position is load-bearing… **There is no async runtime here yet, so nothing is queued and
> nothing is drained -- but this is where it would go.**

## 边界

三条约束，每个阶段都受它管。

1. **async 只在框架边缘和应用代码里可用。** `build` / `performLayout` / `paint` /
   `hitTest` / 手势仲裁永远同步——上游也是这样，而且它们必须在一帧内结束。
2. **future 是 `!Send` 的。** `Pin<Box<dyn Future<Output = ()>>>`，不带 `+ Send`。这是
   "本线程发起、本线程 resume"的编译期保证，也是不给整棵 `Rc` 树引入 `Send` 约束的
   前提。
3. **不引依赖。** `core::future` / `core::task` 在 std 里，`std::task::Wake` 稳定且
   跨线程可用，执行器手写。`Cargo.toml` 的 `[dependencies]` 保持空的。

### 明确不做

* 不动手势 / tooltip / snackbar / multitap 的帧时钟 deadline。它们跟帧对齐是对的——
  一个在两帧之间到期的 tooltip 反正要等下一帧才画得出来。
* 不删任何现有回调 API。框架内部还在用，而且没有执行器的上下文（单测、headless）也要
  能用。所有 future 门面都是**加**。
* 不引 tokio / futures。
* 不给 future 加 `Send`。

## 阶段

### Stage 1 — `post_task`：唤醒通道

执行器唯一自己办不到的一件事：`Waker::wake()` 之后凭什么会有人再来 poll。

| 文件 | 改动 |
|---|---|
| `runtime/rust_app_api.h` | `RfAppHost` **末尾**加 `void (*post_task)(void*)`；声明 `void rf_app_run_tasks(RfApp*)` |
| `runtime/runtime_controller.cc` | `host.post_task = &RuntimeController::OnPostTask;` + 实现 |
| `rust/rustflutter/src/app.rs` | `RfAppHost` 镜像加同字段；导出 `rf_app_run_tasks` |

`OnPostTask` 照抄同文件 `runtime_controller.cc:48-76` 的 `RustPlatformMessageResponse`
——那里已经是"weak + `ui_task_runner_->PostTask`"的现成模式。

三条要点：

* **`post_task` 必须文档化为"任意线程可调"。** `fml::TaskRunner::PostTask` 本来就是
  线程安全的，而这正是选它而不是复用 `schedule_frame` 的理由：`schedule_frame` 最终
  走到 `Animator::RequestFrame`（`shell/common/animator.cc:239`），那里直接读写
  `regenerate_layer_trees_` 这个裸 bool，是 UI 线程亲和的。
* **字段加在结构体末尾** + `RfAppHost host = {}` 零初始化，Rust 侧是
  `Option<extern fn>`。宿主没更新时是 `None`，执行器退化到 `schedule_frame` 唤醒
  （同线程可用），不崩。
* **闭包留在 Rust 侧。** ABI 上只过一个"稍后回来叫我"，不传函数指针和生命周期。

验证：`rust_ffi_unittests` 绿；`RfAppHost` 布局断言测试。

### Stage 2 — `task.rs`：执行器

新文件 `rust/rustflutter/src/task.rs`，约 250 行。原型已在
`scratchpad/task_proto.rs` 跑通（std only，`rustc --edition 2024` 直接过）。

```rust
pub fn attach(post_task: Option<...>, user_data: *mut c_void);
pub fn detach();
pub fn spawn(future: impl Future<Output = ()> + 'static) -> Option<TaskId>;
pub fn run_until_stalled() -> bool;      // = FlushMicrotasksNow
pub fn pending() -> usize;
pub fn oneshot<T>() -> (Sender<T>, Receiver<T>);
```

线程亲和的四条不变式，两条编译器管、两条断言管：

1. **future 离不开本线程**（编译器）。`Pin<Box<dyn Future>>` 不带 `+ Send`；任务表在
   `thread_local` 里。
2. **只有 waker 跨线程，且只搬 `TaskId`**（编译器）。`Waker::from(Arc<TaskWaker>)`
   要求载荷 `Send + Sync`，正是想要的分工。
3. **`post_task` 在 `attach` 时绑定到 owner 线程的 task runner**，`owner: ThreadId`
   一并记下，不靠约定。
4. **`run_until_stalled` 断言 `owner == current().id()`**，以及 `!running`。

两个结构上的注意：

* **`Poster` 必须独立于 `PlatformSink`。** `HostSink` 带 `alive: Cell<bool>`
  （`app.rs:256`），`Cell` 不是 `Sync`，进不了 `Arc<Shared>`。`Poster` 只装函数指针 +
  `user_data`，`unsafe impl Send`，论证照 `painting.rs` 的 `Handoff` 写。
* **排空循环"摘出来再 poll"。** 被 poll 的代码可能反过来调进本模块（spawn、在自己的
  channel 上发消息），所以先 `tasks.remove(&id)` 再跑，跑完放回去——`services/mod.rs:556`
  的 `deliver` 对 handler 就是这么做的，`framework.rs:1315` 的 `set_state` 用
  `try_borrow_mut` 解同一个问题。

验证：`cargo test --lib`，执行器单测不需要引擎（`run_until_stalled()` 本身就是手动泵）。

### Stage 3 — 挂进帧循环

```cpp
  rf_app_begin_frame(app_, frame_micros, frame_number);
  rf_app_run_tasks(app_);        // <- runtime_controller.cc:363
  rf_app_draw_frame(app_);
```

Rust 侧 `rf_app_run_tasks` = `task::run_until_stalled()`，**只有返回 `true` 才
`schedule_frame()`**。挂着等平台答复的 task 不该让应用一直画——这是空闲耗电的唯一
防线，要有回归行盯。

生命周期接在 services 旁边：`rf_app_create` 里 `services::attach` 之后
`task::attach`；`rf_app_destroy` 里 **`services::detach()` 之后**才
`task::detach()`——前者会把所有未答复的 `ReplyCallback` 用 `None` 调一遍
（`services/mod.rs:329`："The failures are the point"），那会 `send` 掉 oneshot 的
`Sender`，让等着的 task 拿到 `None` 而不是被静默丢弃。

再加一个 `IN_FRAME_PHASE` thread_local 标志，由 `app.rs` 的 build/layout/paint 段置位，
`run_until_stalled` 在置位时断言。这是 `RefCell` 不炸的主要手段。

### Stage 4 — thread_local 守卫

跟 async 无关，但一旦有了 task 就变成承重墙：`services::MESSENGER` 和
`painting::IMAGES` 都是 thread_local。worker 线程上调 `send_with_reply` 命中的是一个
`sink: None` 的新 `Messenger`，于是 `services/mod.rs:370` 立刻 `callback(None)`
——**不崩，只是永远拿到"平台没人应答"**。`painting::IMAGES` 更糟：`ImageCache::new()`
会再 spawn 一到四个解码线程（`painting.rs:1002` 的注释已经写下这条前提：
"One pool per thread that builds, which in practice means one: the UI thread"）。

这两个模块的入口加线程断言，把静默的 `None` 变成一声响。

### Stage 5 — future 门面（加，不是改）

框架里 23 处 `*_with_reply` 调用点，一处都不删。

| 现有 | 新增 |
|---|---|
| `MethodChannel::invoke_with_reply` (`services/channel.rs:211`) | `invoke(..) -> impl Future<Output = MethodReply>` |
| `PlatformAssetBundle::prefetch` (`services/asset_bundle.rs:124`) | `prefetch_async(key) -> impl Future<Output = bool>` |
| `TickerFuture::when_complete_or_cancel` (`ticker.rs:149`) | `impl IntoFuture for Rc<TickerFuture>` |
| `RefreshIndicator::refresh_complete` (`progress_indicator.rs:412`) | 收一个 future，完成时自动推进 |

`Sender` 的 `Drop` 保证"永远不来的答复"也会唤醒，对应 `ReplyCallback` 文档里那句
"Always called exactly once"（`services/mod.rs:114`）。

`EventChannel` 的 `Stream` 单列，不在这一轮。

### Stage 6 — `future_builder`

`async.rs` 现有的 `AsyncPoll`（`async.rs:104`）原样保留——那个形态是对的，驱动方持有
所有权。旁边加 `future_builder(fut, initial, builder)`：spawn 那个 future，结果落进
`Rc<RefCell<AsyncSnapshot<T>>>`，`AsyncPoll` 读它，完成时 `request_frame()`。

上游的 `FutureBuilder` 到这里才终于是字面意义上的 FutureBuilder。

### Stage 7 — 时钟

`RfAppHost` 加 `post_delayed_task(void*, int64_t delay_micros)` → `PostDelayedTask`。
`task.rs` 加 `sleep(Duration) -> impl Future`。

**这是给应用代码的，不是给框架内部的**——见"明确不做"第一条。

### Stage 8 — 台账

* `async.rs:2` 和 `ticker.rs:34` 的 "the crate has no async runtime" 要改。
* `runtime_controller.cc:361` 的 "yet" 要改。
* `PORTING_STATUS.md` 记一笔：这条分歧关掉了一半——poll 形态仍在，因为它是对的；
  现在多了一条 future 路。

## 风险

1. **`RefCell` 跨 await**（最大）。缓解：`IN_FRAME_PHASE` 断言 + 排空点唯一。
   code review 红线。
2. **ABI 两份手写镜像漂移**。`rust_app_api.h` 和 `app.rs:132` 已经这么维护六个字段，
   加字段时同步加布局断言测试。
3. **空闲耗电**。Stage 3 那条"只在 `ran` 为真时 `schedule_frame`"是唯一防线，要有回归行：
   一个挂起等平台答复的 task **不得**产生帧。
4. **跨线程 waker 的 `Send` 包装**。`user_data` 是裸指针，`unsafe impl Send` 要论证。

## 基线

覆盖率 1888/1888，测试 3685（`PORTING_STATUS.md:173`）。每个阶段结束时 `cargo test --lib`
与 GN `rustflutter_unittests` 都要绿。
