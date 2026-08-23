# 一个 async 函数在 UI 线程上走完的全程

`task.rs` 的执行器把一次 `.await` 拆成了四段栈：派生、发出、唤醒、恢复。四段之间
没有一根连续的栈，所以看代码时容易接不上——这份文件把它接上。

设计取舍记在 `PORTING_STATUS.md` 顶部的执行器一节；分支当时的推理记在
`ASYNC_PLAN.md`。这里只讲**运行时发生了什么**。

场景是最典型的一种：点击处理器里发一次平台调用。

```rust
// UI 线程，某个 tap handler 里
task::spawn(async move {
    let reply = battery.invoke_awaiting("getLevel", Value::Null).await;
    handle.set_state(move |s| s.level = reply.ok().flatten());
});
```

---

## A. 派生 → 要一次排空

```
UI 线程
│
├─ task::spawn(future)                              task.rs
│    id = next_id++
│    waker = Waker::from(Arc::new(TaskWaker{ id, shared }))
│    tasks.insert(id, Task{ future: Pin<Box<dyn Future>>, waker })
│    ready.push(id)              ← future 未带 +Send，钉死在本线程
│    └─ Shared::request_drain()
│         posted.swap(true) == false → 继续（true 就直接返回，去重）
│         poster.lock()          ← 锁持有到调用返回，detach 要拿同一把
│         └─ (post_task)(user_data) ──────────────┐
│                                                  │
│  ★ 唯一从 Rust 出去的唤醒信号                     │
▼                                                  ▼
                        RuntimeController::OnPostTask   runtime_controller.cc
                          GetUITaskRunner()->PostTask(
                            [weak = weak_for_tasks_]{
                              if (weak && weak->app_)
                                rf_app_run_tasks(weak->app_);
                            })
                                     │
                                     ▼
                          ┌──────────────────────┐
                          │  UI 消息循环队列       │
                          └──────────────────────┘
```

---

## B. 第一次排空 → 调用发出去，任务停住

```
UI 消息循环取出闭包
│
├─ rf_app_run_tasks(app)                            app.rs
│  └─ task::run_until_stalled()                     task.rs
│       debug_assert: owner == current thread   ✓
│       debug_assert: !running                  ✓
│       debug_assert: !IN_FRAME_PHASE           ✓  不在 build/layout/paint 里
│       │
│       loop {
│         fire_expired_timers()                 → 无
│         posted.store(false)      ← 先清标志再读 inbox，本轮到的唤醒
│         batch = take(ready) ++ inbox            要下一轮，不会被吞
│         batch = [1]              ← 只含被唤醒的 id，不是 tasks 全表
│         │
│         └─ tasks.remove(&1)      ← 摘出来再 poll，poll 期间表不被借
│            ran = true
│            future.poll(cx):
│              │
│              ├─ invoke_awaiting("getLevel", Null)          channel.rs
│              │    ★ 这半截是普通 fn，调用即执行
│              │    oneshot() → (sender, receiver)
│              │    invoke_with_reply(.., |r| sender.send(r))
│              │      services::send_with_reply()        services/mod.rs
│              │        with_messenger()
│              │          └─ debug_assert_ui_thread      ✓
│              │        response_id = next_response_id++
│              │        waiting.insert(response_id, callback)
│              │        sink.send(..) ──► host.send_platform_message
│              │                          └─► 引擎 ─► 嵌入器 ─► 平台
│              │
│              └─ receiver.await   ★ 这半截在 async 块里，poll 时才执行
│                   Receiver::poll: 无值、未关闭
│                   slot.waker = Some(cx.waker().clone())
│                   → Poll::Pending
│            未完成 → tasks.insert(1, task)
│         │
│         fire_expired_timers(); batch 空 → running=false; return ran
│       }
│
└─ ran == true → instance.schedule_frame()
      ⚠ 这一帧没东西可画。首次 poll 必然 ran=true——见文末「已知代价」。
```

**`invoke_awaiting` 是普通 `fn` 而不是 `async fn`，这是有意的。** 写成 `async fn`
会把整个函数体装进状态机，平台消息要等到第一次 poll 才发出去。现在这样，「立刻做」
在块外、「等结果」在块内。方法文档里那句 *"The call goes out at once, not when the
future is first polled"* 说的就是这件事——也是为什么丢掉这个 future **取消不了任何
东西**，只是丢掉一个答案。

---

## C. 平台答复 → 唤醒

```
平台线程 / worker                     UI 线程
│
├─ 嵌入器答复
└─ RustPlatformMessageResponse::Post        runtime_controller.cc
     ui_task_runner_->PostTask(..) ──────► CompletePlatformMessageReply
                                            │
                                            ├─ rf_app_complete_platform_
                                            │    message_reply()      app.rs
                                            └─ services::complete_reply()
                                                            services/mod.rs
                                                 with_messenger → assert ui ✓
                                                 waiting.remove(response_id)
                                                 │
                                                 ├─ callback(reply)
                                                 │    decode_envelope
                                                 │    sender.send(reply)  task.rs
                                                 │      slot.value = Some(..)
                                                 │      waker = slot.waker.take()
                                                 │      ★ 先放掉 borrow 再 wake
                                                 │      └─ waker.wake()
                                                 │         TaskWaker::wake_by_ref
                                                 │           inbox.lock().push(1)
                                                 │           request_drain()
                                                 │             → post_task ─────┐
                                                 │                              │
                                                 └─ request_frame()             │
                                                    （两次帧请求在 Animator      │
                                                      的 semaphore 里合并）      │
                                                                                ▼
                                                                     UI 消息循环队列
```

---

## D. 第二次排空 → 任务跑完

```
├─ rf_app_run_tasks(app)
│  └─ run_until_stalled()
│       batch = take(ready)[空] ++ inbox[1] = [1]
│       └─ tasks.remove(&1); ran = true
│          future.poll(cx):
│            从 .await 处恢复    ★ 本线程，和派生它的那根一样
│            Receiver::poll → slot.value.take() → Ready(Some(reply))
│            handle.set_state(|s| s.level = ..)          framework.rs
│              try_borrow_mut 成功 → 立即改；失败 → 排队
│              标脏 element，tree.needs_frame = true
│            → Poll::Ready(())
│          完成 → 不放回表  →  pending() == 0
│
└─ ran == true → schedule_frame()
```

---

## E. 帧

```
Animator ── vsync ──► RuntimeController::BeginFrame   runtime_controller.cc
                       │
                       ├─ rf_app_begin_frame()   动画相位：tick 推进
                       │
                       ├─ rf_app_run_tasks()     ★ 排空点
                       │    上游 FlushMicrotasksNow 的位置，而这个位置是承重的：
                       │    在 tick 期间完成的任务必须被紧随其后的 build 看见,
                       │    和「在 onBeginFrame 里起的动画必须被同一帧看见」同理。
                       │
                       └─ rf_app_draw_frame()
                            draw_view()                              app.rs
                            │
                            ├─ let _phase = FramePhase::enter()
                            │    IN_FRAME_PHASE = true
                            │    ← 从这里到 drop，排空被禁止
                            │
                            ├─ application.build()  tree.rebuild_dirty()
                            │                       新的 s.level 被读到
                            ├─ 布局（约束下去、尺寸上来）
                            ├─ 绘制
                            │
                            ├─ drop(_phase)         IN_FRAME_PHASE = false
                            └─ host.render(layer_tree) ──► shell ──► Rasterizer
```

---

## 三个变体

### 跨线程唤醒（解码 worker）

C 段换成这样，其余不变：

```
rf.image.2 worker          │  UI 线程
  Image::decode 完成        │
  waker.wake() ────────────►│  inbox.lock().push(id)   ← 只搬一个 u64
   （Waker 是 Send+Sync,     │  request_drain()
     future 不是）           │    → post_task
                            │      （线程安全，这就是它存在的理由：
                            │        schedule_frame 走到 Animator::
                            │        RequestFrame，那里是 UI 线程亲和的）
                            │      → UI runner → rf_app_run_tasks
                            ▼      → 在 UI 线程 poll ★
```

### `sleep(d)`

B 段的 poll 换成：

```
Sleep::poll:  deadline > now
  *self.waker.borrow_mut() = Some(cx.waker().clone())
      ← 每次 poll 都换新的：契约是唤醒最后一次 poll 它的那个 waker
  if !armed:
     executor.timers.push(Timer{ deadline, waker: slot })
     shared.request_delayed_drain(deadline - now)
       → post_delayed_task(user_data, micros)
         → PostDelayedTask(.., TimeDelta::FromMicroseconds)
  → Pending

到点：UI runner → rf_app_run_tasks → run_until_stalled
      └─ fire_expired_timers()
           每轮循环开头都查，所以没有宿主时钟时也不会漏,
           只会等到下一次因别的原因发生的排空——晚，但不丢。
```

`sleep` 是给应用代码的。框架自己的每一条 deadline——长按判定、tooltip 淡出、
snackbar 到期、双击等第二下——都留在帧时钟上：一个在两帧之间到期的 tooltip 反正要
等下一帧才画得出来。

### 没有执行器的线程

`EXECUTOR` 是 `thread_local`，worker 上是 `None`，`spawn()` 返回 `None`，future
当场丢弃。**不是漏，是设计**——一个没人 poll 的任务是伪装成正常的泄漏。

---

## 为什么是 post_task，而不是 wake 直接 resume

这是读这份图最常冒出来的问题。三条理由，从硬到软：

**1. 跨线程时物理上不可能。** future 在 UI 线程的 `thread_local` 里，worker 看不见。
必须有一跳。

**2. 重入。** `wake()` 合法地可以在 `poll()` 内部被调用——`yield_now` 的标准写法就是
`cx.waker().wake_by_ref(); Poll::Pending`。同步 resume 就是无限递归。我们自己代码里
也有一处：`fire_expired_timers()` 在 `run_until_stalled` 的循环里调 `waker.wake()`,
同步排空会当场撞上 `debug_assert!(!running)`。

**3. `RefCell` / `FramePhase`。** `wake()` 会从任意位置被调用。同步 resume 意味着任务
可能在 `draw_view` 中途醒过来，那时元素树正被借着。而 `FramePhase` 这个守卫**只在
排空点唯一时才成立**。

Dart 是同一个设计,而且写进了语义:`Future.then` 的回调永远在后一个微任务轮次跑,
哪怕 future 已经完成。**`post_task` + 排空就是那个微任务队列。**

`Waker` 本身也摸不到 future——它只有一个 `TaskId` 和 `Arc<Shared>`,而且**不能有更多**:
`Waker::from(Arc<W>)` 要求 `W: Send + Sync`,future 是 `!Send`。**waker 里放不下
future,这正是它能跨线程的原因。**

### 一条没走的快路径

同线程唤醒、且当前既不在 poll 里也不在帧相位里时,`request_drain` 本可以直接同步排空,
省掉一次 task runner 跳转。条件是可判定的。

没做,因为它把「恢复点」从一个变成不确定多个。具体到 C 段:任务会在
`services::complete_reply` 的栈上跑完,而那个函数在回调之后还有事要做
（`request_frame()`)。**换来一跳,赔掉「应用代码只在一个地方运行」这条性质。**
要动它得先有数字。

---

## poll 不遍历所有任务

排空的输入是**被唤醒的集合**,不是 `tasks`:

```rust
let mut ids = std::mem::take(&mut executor.ready);   // 新派生的
ids.append(&mut executor.shared.inbox.lock()...);    // 被 wake 的
```

`tasks: HashMap<TaskId, Task>` 在整个排空里只被按 id 索引(`remove` / `insert`),
**从头到尾没有一次 `iter()`**。单次排空是 O(被唤醒数),不是 O(挂起数)。

这正是 `Waker` 这个抽象存在的目的:把「哪个任务可以推进了」从扫描变成信号。
回归行 `a_drain_polls_only_what_was_woken` 盯着它——一百个挂起的任务,排空一次零次 poll。

---

## 已知代价

* **一次 spawn 多一帧。** 首次 poll 必然 `ran = true`,哪怕任务立刻停住,于是
  `rf_app_run_tasks` 会要一帧。要消掉得区分「跑了」和「跑出了变化」,而
  `run_until_stalled` 看不见这个差别。测出来有代价再动。
* **定时器是全扫的。** `fire_expired_timers` 每轮 `timers.retain(..)`,O(timers)。
  前提是这个 `Vec` 很短——`sleep` 只给应用代码。真到了几百个并发 `sleep`,这里要换成
  按 deadline 排的二叉堆。
* **每次 poll 两次哈希操作。** `remove` + `insert`,即使任务只是又停住了。换来的是
  future 运行期间 `tasks` 表不被借,所以被 poll 的代码可以反过来 `spawn`、可以
  `detach`、可以在自己的 channel 上发消息。是买来的性质,不是疏忽。
