// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The framework's executor: upstream's microtask queue, in the shape Rust
//! needs it.
//!
//! Upstream's `Future` is not a concurrency primitive. `async`/`await` in Dart
//! schedules a continuation on the same isolate's event loop and runs it on the
//! same thread; parallelism is `Isolate`, which `packages/flutter` barely uses.
//! So a Dart future means exactly "call me back later, on this thread" -- which
//! is why every one of them ported cleanly to a callback, and why this module
//! is small: it adds the sequencing sugar those callbacks lack, not a new
//! concurrency model.
//!
//! What `core::future` and `core::task` supply is the vocabulary -- the trait,
//! `Poll`, `Waker`, and the `async fn` state machine, which is a compiler
//! feature needing no library at all. What they deliberately do not supply is
//! an executor, or any answer to what `Waker::wake()` should cause. That answer
//! is [`RfAppHost::post_task`](crate::app), and it is the only thing here that
//! the crate could not have written for itself.
//!
//! # Thread affinity
//!
//! A task resumes on the thread that spawned it. Four invariants say so, two
//! held by the compiler and two by assertions:
//!
//! 1. **A future cannot leave its thread.** [`Task::future`] is
//!    `Pin<Box<dyn Future<Output = ()>>>` with no `+ Send`, and the table
//!    holding it is a `thread_local`. Moving one off is a type error.
//! 2. **Only the waker crosses, and it carries an id.** `Waker::from(Arc<W>)`
//!    wants `W: Send + Sync`, which is the division this needs: the signal may
//!    travel, the state may not.
//! 3. **The poster is bound at [`attach`]** to the task runner of the thread
//!    that attached, and that thread's id is recorded beside it.
//! 4. **[`run_until_stalled`] checks both** -- the owning thread, and that it is
//!    not already running.
//!
//! A thread that never called [`attach`] -- a decode worker, say -- has no
//! executor, so [`spawn`] there answers `None` rather than parking a task
//! nobody will ever poll.
//!
//! # Recorded divergences
//!
//! * Upstream drains microtasks at several points in a turn; here there is one
//!   drain per frame plus one per `post_task`, both landing in
//!   `rf_app_run_tasks`. The position inside the frame is upstream's --
//!   between the animation phase and the build phase.
//! * Upstream futures never cancel; Rust futures cancel on drop. [`detach`]
//!   therefore drops pending tasks rather than completing them, and the
//!   callback-shaped APIs underneath ([`services`](crate::services)) are the
//!   ones that keep upstream's "always called exactly once" guarantee. See the
//!   ordering note on [`detach`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::ThreadId;

/// Names one spawned task. Only ever compared and looked up; the numbering is
/// not meaningful beyond being unique for the life of an executor.
pub type TaskId = u64;

// -- Waking -------------------------------------------------------------------

/// The host's "come back and drain me", and the `user_data` it takes.
///
/// Raw pointers are not `Send` and this one has to be: a decode worker
/// finishing wakes a task and must be able to ask the UI thread for a drain.
/// Sound because the pointer is never dereferenced here -- it is handed back to
/// the host, which owns whatever it points at -- and because [`Shared::poster`]
/// is only read under the lock that [`detach`] takes to clear it, so no call
/// can be in flight once the host has gone.
struct Poster {
    post_task: unsafe extern "C" fn(*mut c_void),
    user_data: *mut c_void,
}

// SAFETY: see above. The pointer is opaque here and its owner outlives every
// call, because the lock orders the last call before the clearing.
unsafe impl Send for Poster {}

/// The part of the executor a [`Waker`] can reach, and therefore the only part
/// that crosses threads. It carries ids; it never carries futures.
struct Shared {
    /// Tasks woken since the last drain. A `Mutex` rather than a `RefCell`
    /// because the waking thread may not be the owning one.
    inbox: Mutex<Vec<TaskId>>,
    /// The thread that owns the futures. Every resume happens on it.
    owner: ThreadId,
    /// `None` before [`attach`] gives one, and again after [`detach`] takes it
    /// away. Holding the lock across the call is what makes the host's
    /// `user_data` safe to use: teardown cannot start until the call returns.
    poster: Mutex<Option<Poster>>,
    /// A drain has been asked for and not yet started. Coalesces: a hundred
    /// wakes between two drains cost one `post_task`.
    posted: AtomicBool,
}

impl Shared {
    fn request_drain(&self) {
        if self.posted.swap(true, Ordering::AcqRel) {
            // Already asked. The drain that answers will see this wake too,
            // because it clears the flag before reading the inbox.
            return;
        }
        let poster = self.poster.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(poster) = poster.as_ref() {
            // SAFETY: the lock is held, so `detach` cannot have run and the
            // host is still there. See the note on `Poster`.
            unsafe { (poster.post_task)(poster.user_data) };
        }
    }
}

/// One per task, made at spawn and reused for every poll of it.
struct TaskWaker {
    id: TaskId,
    shared: Arc<Shared>,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.shared
            .inbox
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(self.id);
        self.shared.request_drain();
    }
}

// -- The executor -------------------------------------------------------------

struct Task {
    /// No `+ Send`, deliberately: this is invariant 1. A future here holds the
    /// framework's `Rc`s and `RefCell`s, and moving one to another thread would
    /// make every one of them unsound at once.
    future: Pin<Box<dyn Future<Output = ()>>>,
    waker: Waker,
}

struct Executor {
    tasks: HashMap<TaskId, Task>,
    /// Spawned and not yet polled once. Kept apart from the inbox so a fresh
    /// task runs in the same drain that spawned it, as a Dart microtask does.
    ready: Vec<TaskId>,
    next_id: TaskId,
    shared: Arc<Shared>,
    /// True while [`run_until_stalled`] is on the stack. A wake during a drain
    /// queues; it does not recurse.
    running: bool,
}

thread_local! {
    /// `None` until [`attach`]. A thread that never attached has no executor
    /// rather than an empty one, so [`spawn`] can tell the caller.
    static EXECUTOR: RefCell<Option<Executor>> = const { RefCell::new(None) };

    /// Set while the frame is building, laying out or painting. Draining there
    /// would poll a task into the middle of a tree that is already borrowed.
    static IN_FRAME_PHASE: Cell<bool> = const { Cell::new(false) };
}

/// Marks the part of a frame during which no task may be polled.
///
/// The framework's state is `Rc<RefCell<..>>` throughout, and a build has the
/// element tree checked out. A task resumed there would reach the same cells
/// through a different path and fail the borrow -- so the drain happens between
/// the phases, never inside one, and this is what says so out loud.
pub struct FramePhase(());

impl FramePhase {
    /// Enters the phase. Nested entries are allowed and only the outermost
    /// matters, which is what makes it safe to put one around each of layout
    /// and paint as well as around the build.
    pub fn enter() -> FramePhase {
        IN_FRAME_PHASE.with(|flag| flag.set(true));
        FramePhase(())
    }
}

impl Drop for FramePhase {
    fn drop(&mut self) {
        IN_FRAME_PHASE.with(|flag| flag.set(false));
    }
}

/// Creates this thread's executor and binds it to the host's task runner.
///
/// `post_task` may be `None`: the executor then still works, but nothing asks
/// for a drain, so tasks only advance when the frame loop drains them. That is
/// the state a unit test and a headless render are in, and it is why the tests
/// below drive [`run_until_stalled`] by hand.
pub fn attach(post_task: Option<unsafe extern "C" fn(*mut c_void)>, user_data: *mut c_void) {
    let shared = Arc::new(Shared {
        inbox: Mutex::new(Vec::new()),
        owner: std::thread::current().id(),
        poster: Mutex::new(post_task.map(|post_task| Poster {
            post_task,
            user_data,
        })),
        posted: AtomicBool::new(false),
    });
    EXECUTOR.with(|executor| {
        *executor.borrow_mut() = Some(Executor {
            tasks: HashMap::new(),
            ready: Vec::new(),
            next_id: 1,
            shared,
            running: false,
        });
    });
}

/// Clears the poster, then drops every task that has not finished.
///
/// **Call this after [`services::detach`](crate::services::detach), not
/// before.** That one answers every outstanding reply with `None`, which is
/// what settles the [`oneshot`] a waiting task is parked on; a task dropped
/// first would take its `Receiver` with it and the answer would arrive nowhere.
/// The order is the difference between a task that learns the platform is gone
/// and one that simply vanishes.
///
/// Clearing the poster first, and under the lock, is what makes
/// [`Poster::user_data`] sound: after this returns, no thread is inside
/// `post_task` and none can enter, so the host is free to go.
pub fn detach() {
    let shared = EXECUTOR.with(|executor| {
        executor
            .borrow()
            .as_ref()
            .map(|executor| Arc::clone(&executor.shared))
    });
    if let Some(shared) = shared {
        // Wakers held by worker threads outlive this, and keep `Shared` alive
        // with them. They may still push into the inbox; with no poster and no
        // executor, that is a write nobody reads.
        *shared.poster.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
    let dropped = EXECUTOR.with(|executor| executor.borrow_mut().take());
    drop(dropped);
}

/// Whether this thread has an executor.
pub fn is_attached() -> bool {
    EXECUTOR.with(|executor| executor.borrow().is_some())
}

/// Spawns a future onto *this* thread's executor.
///
/// Answers `None` when there is no executor here, which is the case on every
/// thread but the one the shell created the application on. The future is
/// dropped in that case rather than kept: a task nobody will poll is a leak
/// wearing a disguise.
pub fn spawn(future: impl Future<Output = ()> + 'static) -> Option<TaskId> {
    EXECUTOR.with(|executor| {
        let mut slot = executor.borrow_mut();
        let executor = slot.as_mut()?;
        let id = executor.next_id;
        executor.next_id += 1;
        let waker = Waker::from(Arc::new(TaskWaker {
            id,
            shared: Arc::clone(&executor.shared),
        }));
        executor.tasks.insert(
            id,
            Task {
                future: Box::pin(future),
                waker,
            },
        );
        executor.ready.push(id);
        // A task spawned between frames would otherwise wait for whatever else
        // asks for one. Spawning is a reason on its own.
        executor.shared.request_drain();
        Some(id)
    })
}

/// How many tasks have been spawned and not yet finished.
pub fn pending() -> usize {
    EXECUTOR.with(|executor| {
        executor
            .borrow()
            .as_ref()
            .map_or(0, |executor| executor.tasks.len())
    })
}

/// Polls every ready task until none is left ready. Upstream's
/// `FlushMicrotasksNow`.
///
/// Answers whether anything ran, which is what the frame loop uses to decide
/// whether to ask for another frame. A task parked on a platform reply makes
/// this `false`, and that is what keeps an idle application idle: frames are on
/// demand, and waiting is not a reason to draw.
pub fn run_until_stalled() -> bool {
    EXECUTOR.with(|executor| {
        if let Some(executor) = executor.borrow().as_ref() {
            debug_assert_eq!(
                executor.shared.owner,
                std::thread::current().id(),
                "a task resumes on the thread that spawned it"
            );
            debug_assert!(
                !executor.running,
                "run_until_stalled is not re-entrant; a wake during a drain queues"
            );
        }
    });
    debug_assert!(
        !IN_FRAME_PHASE.with(Cell::get),
        "tasks are drained between a frame's phases, never inside one"
    );

    let mut ran = false;
    loop {
        // Collect one batch, then let the borrow go: polling a future runs
        // application code, and that code may spawn, wake, or send on a
        // channel of its own.
        let batch = EXECUTOR.with(|executor| {
            let mut slot = executor.borrow_mut();
            let Some(executor) = slot.as_mut() else {
                return Vec::new();
            };
            // Cleared before the inbox is read, so a wake that lands during
            // this drain asks for the next one rather than being swallowed.
            executor.shared.posted.store(false, Ordering::Release);
            let mut ids = std::mem::take(&mut executor.ready);
            ids.append(
                &mut executor
                    .shared
                    .inbox
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()),
            );
            // A task woken twice before a drain is one task to poll.
            ids.sort_unstable();
            ids.dedup();
            executor.running = true;
            ids
        });

        if batch.is_empty() {
            EXECUTOR.with(|executor| {
                if let Some(executor) = executor.borrow_mut().as_mut() {
                    executor.running = false;
                }
            });
            return ran;
        }

        for id in batch {
            // Checked out of the table rather than borrowed in place, so the
            // table is free while the future runs. `services::deliver` moves a
            // handler out for the same reason, and `StateHandle::set_state`
            // solves the same problem with `try_borrow_mut`.
            let checked_out = EXECUTOR.with(|executor| {
                let mut slot = executor.borrow_mut();
                slot.as_mut().and_then(|executor| executor.tasks.remove(&id))
            });
            let Some(mut task) = checked_out else {
                // Finished, or dropped by a `detach` from inside another task.
                continue;
            };
            ran = true;
            let mut context = Context::from_waker(&task.waker);
            let finished = task.future.as_mut().poll(&mut context).is_ready();
            if !finished {
                EXECUTOR.with(|executor| {
                    if let Some(executor) = executor.borrow_mut().as_mut() {
                        executor.tasks.insert(id, task);
                    }
                });
            }
        }
    }
}

// -- oneshot ------------------------------------------------------------------

/// The shared cell a [`Sender`] fills and a [`Receiver`] awaits.
struct Slot<T> {
    value: Option<T>,
    waker: Option<Waker>,
    /// The sender went away without sending. Distinct from "not yet".
    closed: bool,
}

/// Fills a [`Receiver`] exactly once.
///
/// Dropping one without sending settles the receiver with `None`, which is what
/// keeps [`ReplyCallback`](crate::services::ReplyCallback)'s "always called
/// exactly once" guarantee true on this side of the wrapper: a reply that never
/// comes must not leave a task parked forever.
pub struct Sender<T>(Rc<RefCell<Slot<T>>>);

/// Resolves to `Some` when the [`Sender`] sends, `None` if it is dropped first.
pub struct Receiver<T>(Rc<RefCell<Slot<T>>>);

/// A one-shot channel, single-threaded: the half that turns a reply callback
/// into something awaitable.
///
/// `Rc`, not `Arc`, because both halves live on the owning thread -- the
/// callback that sends is the one the shell hands the reply to, and the shell
/// hands it over on the UI thread. Work that finishes on another thread reaches
/// a task through its [`Waker`] instead.
pub fn oneshot<T>() -> (Sender<T>, Receiver<T>) {
    let slot = Rc::new(RefCell::new(Slot {
        value: None,
        waker: None,
        closed: false,
    }));
    (Sender(Rc::clone(&slot)), Receiver(slot))
}

impl<T> Sender<T> {
    /// Hands the value over and wakes whoever is waiting.
    pub fn send(self, value: T) {
        // The waker is taken and the borrow released before it is called:
        // waking may poll the receiver, and that borrows the same cell.
        let waker = {
            let mut slot = self.0.borrow_mut();
            slot.value = Some(value);
            slot.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let waker = {
            let mut slot = self.0.borrow_mut();
            if slot.value.is_some() {
                return;
            }
            slot.closed = true;
            slot.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

impl<T> Future for Receiver<T> {
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<T>> {
        let mut slot = self.0.borrow_mut();
        if let Some(value) = slot.value.take() {
            return Poll::Ready(Some(value));
        }
        if slot.closed {
            return Poll::Ready(None);
        }
        // Replaced rather than kept: a future may be polled by a different
        // waker than last time, and the newest is the one that will be heard.
        slot.waker = Some(context.waker().clone());
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every test attaches for itself: `EXECUTOR` is a thread_local and the
    /// test harness runs tests on threads of its own.
    fn attached() {
        attach(None, std::ptr::null_mut());
    }

    #[test]
    fn a_task_runs_when_it_is_drained_and_not_before() {
        attached();
        let ran = Rc::new(Cell::new(false));
        let flag = Rc::clone(&ran);
        spawn(async move { flag.set(true) });
        assert!(!ran.get(), "spawning does not run it");
        assert!(run_until_stalled());
        assert!(ran.get());
        assert_eq!(pending(), 0, "a finished task leaves the table");
        detach();
    }

    #[test]
    fn a_task_parked_on_a_oneshot_resumes_when_it_is_sent() {
        attached();
        let (sender, receiver) = oneshot::<i32>();
        let seen = Rc::new(Cell::new(None));
        let out = Rc::clone(&seen);
        spawn(async move { out.set(receiver.await) });

        run_until_stalled();
        assert_eq!(pending(), 1, "still waiting");
        assert_eq!(seen.get(), None);

        sender.send(7);
        assert!(run_until_stalled());
        assert_eq!(seen.get(), Some(7));
        assert_eq!(pending(), 0);
        detach();
    }

    #[test]
    fn a_sender_dropped_without_sending_settles_the_receiver() {
        // The guarantee `ReplyCallback` documents, kept through the wrapper: a
        // platform that never answers must not park a task for the life of the
        // process.
        attached();
        let (sender, receiver) = oneshot::<i32>();
        let seen = Rc::new(Cell::new(Some(0)));
        let out = Rc::clone(&seen);
        spawn(async move { out.set(receiver.await) });
        run_until_stalled();

        drop(sender);
        run_until_stalled();
        assert_eq!(seen.get(), None, "settled with None, not left pending");
        assert_eq!(pending(), 0);
        detach();
    }

    #[test]
    fn a_drain_with_nothing_ready_says_so() {
        // This is what keeps an idle application idle: the frame loop asks for
        // another frame only when something ran.
        attached();
        assert!(!run_until_stalled(), "nothing spawned");
        let (sender, receiver) = oneshot::<i32>();
        spawn(async move {
            let _ = receiver.await;
        });
        assert!(run_until_stalled(), "the first poll counts as running");
        assert!(!run_until_stalled(), "parked, so nothing ran");
        drop(sender);
        detach();
    }

    #[test]
    fn a_task_spawned_from_inside_a_task_runs_in_the_same_drain() {
        // Upstream a microtask queued by a microtask runs before the queue is
        // considered empty, and the loop here has the same shape.
        attached();
        let order = Rc::new(RefCell::new(Vec::new()));
        let outer = Rc::clone(&order);
        spawn(async move {
            outer.borrow_mut().push("outer");
            let inner = Rc::clone(&outer);
            spawn(async move { inner.borrow_mut().push("inner") });
        });
        assert!(run_until_stalled());
        assert_eq!(*order.borrow(), vec!["outer", "inner"]);
        detach();
    }

    #[test]
    fn waking_twice_before_a_drain_polls_once() {
        attached();
        let polls = Rc::new(Cell::new(0));
        let counted = Rc::clone(&polls);
        let (sender, receiver) = oneshot::<i32>();
        spawn(async move {
            counted.set(counted.get() + 1);
            let _ = receiver.await;
            counted.set(counted.get() + 1);
        });
        run_until_stalled();
        assert_eq!(polls.get(), 1);

        // Two wakes, one drain.
        let waker = EXECUTOR.with(|executor| {
            let slot = executor.borrow();
            slot.as_ref().unwrap().tasks.values().next().unwrap().waker.clone()
        });
        waker.wake_by_ref();
        waker.wake();
        run_until_stalled();
        assert_eq!(polls.get(), 1, "the poll found it still parked, once");

        sender.send(1);
        run_until_stalled();
        assert_eq!(polls.get(), 2);
        detach();
    }

    #[test]
    fn a_waker_woken_from_another_thread_resumes_here() {
        // The decode-worker case: the signal travels, the future does not.
        attached();
        let owner = std::thread::current().id();
        let resumed_on = Rc::new(Cell::new(None));
        let out = Rc::clone(&resumed_on);
        let (sender, receiver) = oneshot::<()>();
        spawn(async move {
            let _ = receiver.await;
            out.set(Some(std::thread::current().id()));
        });
        run_until_stalled();

        let waker = EXECUTOR.with(|executor| {
            let slot = executor.borrow();
            slot.as_ref().unwrap().tasks.values().next().unwrap().waker.clone()
        });
        std::thread::spawn(move || waker.wake()).join().unwrap();

        sender.send(());
        run_until_stalled();
        assert_eq!(
            resumed_on.get(),
            Some(owner),
            "resumed on the thread that spawned it"
        );
        detach();
    }

    #[test]
    fn a_thread_without_an_executor_refuses_to_spawn() {
        // Not an error to reach here -- a decode worker has no executor by
        // design -- but the task must not be silently accepted and never run.
        let ran = std::thread::spawn(|| {
            assert!(!is_attached());
            let id = spawn(async {
                unreachable!("nothing polls this");
            });
            id
        })
        .join()
        .unwrap();
        assert_eq!(ran, None);
    }

    #[test]
    fn detach_drops_what_is_still_pending() {
        attached();
        let alive = Rc::new(Cell::new(true));
        let flag = Rc::clone(&alive);
        struct Marker(Rc<Cell<bool>>);
        impl Drop for Marker {
            fn drop(&mut self) {
                self.0.set(false);
            }
        }
        let (sender, receiver) = oneshot::<i32>();
        spawn(async move {
            let _marker = Marker(flag);
            let _ = receiver.await;
        });
        run_until_stalled();
        assert!(alive.get());

        detach();
        assert!(!alive.get(), "the task was dropped, and its captures with it");
        assert!(!is_attached());
        drop(sender);
    }

    #[test]
    fn detaching_clears_the_poster_before_the_host_goes() {
        // The ordering the C ABI relies on: after detach, no thread can enter
        // post_task, so the shell is free to tear down.
        static POSTED: AtomicBool = AtomicBool::new(false);
        unsafe extern "C" fn post(_user_data: *mut c_void) {
            POSTED.store(true, Ordering::Release);
        }
        POSTED.store(false, Ordering::Release);

        attach(Some(post), std::ptr::null_mut());
        let (sender, receiver) = oneshot::<i32>();
        spawn(async move {
            let _ = receiver.await;
        });
        assert!(POSTED.load(Ordering::Acquire), "spawning asks for a drain");
        run_until_stalled();

        let waker = EXECUTOR.with(|executor| {
            let slot = executor.borrow();
            slot.as_ref().unwrap().tasks.values().next().unwrap().waker.clone()
        });
        detach();

        POSTED.store(false, Ordering::Release);
        waker.wake();
        assert!(
            !POSTED.load(Ordering::Acquire),
            "a wake after detach reaches no host"
        );
        drop(sender);
    }

    #[test]
    fn one_post_task_covers_many_wakes() {
        static COUNT: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);
        unsafe extern "C" fn post(_user_data: *mut c_void) {
            COUNT.fetch_add(1, Ordering::AcqRel);
        }
        COUNT.store(0, Ordering::Release);

        attach(Some(post), std::ptr::null_mut());
        let (sender, receiver) = oneshot::<i32>();
        spawn(async move {
            let _ = receiver.await;
        });
        assert_eq!(COUNT.load(Ordering::Acquire), 1, "spawn asked once");
        run_until_stalled();

        let waker = EXECUTOR.with(|executor| {
            let slot = executor.borrow();
            slot.as_ref().unwrap().tasks.values().next().unwrap().waker.clone()
        });
        COUNT.store(0, Ordering::Release);
        for _ in 0..10 {
            waker.wake_by_ref();
        }
        assert_eq!(
            COUNT.load(Ordering::Acquire),
            1,
            "ten wakes between drains cost one post_task"
        );
        run_until_stalled();
        for _ in 0..3 {
            waker.wake_by_ref();
        }
        assert_eq!(
            COUNT.load(Ordering::Acquire),
            2,
            "and the next drain re-arms it"
        );
        drop(sender);
        detach();
    }

    #[test]
    fn a_frame_phase_forbids_draining() {
        attached();
        let phase = FramePhase::enter();
        assert!(IN_FRAME_PHASE.with(Cell::get));
        drop(phase);
        assert!(!IN_FRAME_PHASE.with(Cell::get));
        // And the drain is fine once the phase is over.
        assert!(!run_until_stalled());
        detach();
    }
}
