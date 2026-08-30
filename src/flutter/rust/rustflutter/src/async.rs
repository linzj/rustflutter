//! Async builders, from upstream `widgets/async.dart`: a widget that shows
//! what an asynchronous value is doing.
//!
//! Two ways in, and the difference is who owns the work.
//!
//! [`async_builder`] takes a *poll* -- a closure the framework calls each
//! build, answering the connection state and the latest payload. Whatever
//! drives the value (a worker thread, the engine's task runners, a channel
//! being drained) keeps the ownership, and the widget only reports. This is
//! the older of the two and it is not a workaround: it is the right shape
//! whenever the producer is not a `Future` at all, which covers streams,
//! polled hardware, and anything already advancing on its own clock.
//!
//! [`future_builder`] takes a `Future` and is upstream's `FutureBuilder`
//! spelled literally. It spawns the future on the framework's executor
//! ([`task`](crate::task)), keeps the snapshot, and hands it to the same
//! builder contract. The frame it needs is asked for by the executor, which
//! schedules one whenever a task actually ran.
//!
//! Recorded divergence (see PORTING_STATUS.md): upstream re-builds only the
//! widget holding the future, because the completion is delivered to that
//! widget. Here the completion marks a shared cell and the next build reads
//! it, so what re-runs is whatever the framework would have re-run anyway.
//! `StreamBuilder` remains [`async_builder`]'s shape; a `Stream` needs a
//! type this crate does not have.

use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

use crate::framework::{AnyWidget, BuildContext, StateHandle, component, stateful};

/// Upstream `ConnectionState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not yet connected to anything.
    None,
    /// Waiting for the first (or the next) value.
    Waiting,
    /// Connected and holding the latest value.
    Active,
    /// The stream is closed.
    Done,
}

/// Upstream `AsyncSnapshot<T>`: what the builder is shown -- the connection
/// state, and exactly one of data or error.
#[derive(Clone, Debug, PartialEq)]
pub struct AsyncSnapshot<T> {
    pub connection_state: ConnectionState,
    pub data: Option<T>,
    /// The error message; upstream carries an `Object` and a stack trace,
    /// the port's async sources are strings.
    pub error: Option<String>,
}

impl<T> AsyncSnapshot<T> {
    /// Upstream `AsyncSnapshot.nothing`.
    pub fn nothing() -> AsyncSnapshot<T> {
        AsyncSnapshot {
            connection_state: ConnectionState::None,
            data: None,
            error: None,
        }
    }

    /// Upstream `AsyncSnapshot.waiting`.
    pub fn waiting() -> AsyncSnapshot<T> {
        AsyncSnapshot {
            connection_state: ConnectionState::Waiting,
            data: None,
            error: None,
        }
    }
}

impl<T: Clone> AsyncSnapshot<T> {
    /// Upstream `AsyncSnapshot.withData`.
    pub fn with_data(connection_state: ConnectionState, data: T) -> AsyncSnapshot<T> {
        AsyncSnapshot {
            connection_state,
            data: Some(data),
            error: None,
        }
    }

    /// Upstream `AsyncSnapshot.withError`.
    pub fn with_error(
        connection_state: ConnectionState,
        error: impl Into<String>,
    ) -> AsyncSnapshot<T> {
        AsyncSnapshot {
            connection_state,
            data: None,
            error: Some(error.into()),
        }
    }

    /// Upstream `hasData`.
    pub fn has_data(&self) -> bool {
        self.data.is_some()
    }

    /// Upstream `hasError`.
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }

    /// Upstream `inState`.
    pub fn in_state(&self, connection_state: ConnectionState) -> AsyncSnapshot<T> {
        AsyncSnapshot {
            connection_state,
            data: self.data.clone(),
            error: self.error.clone(),
        }
    }
}

/// The poll both builders share: answer the snapshot as of this build. The
/// state it closes over belongs to the caller -- the future's driver.
pub type AsyncPoll<T> = Rc<dyn Fn() -> AsyncSnapshot<T>>;

/// Upstream `FutureBuilder<T>` / `StreamBuilder<T>`, one widget: the frame
/// polls, the builder shows. `initial` is what the first frame sees,
/// upstream's `initialData`.
pub fn async_builder<T: Clone + 'static>(
    poll: AsyncPoll<T>,
    initial: AsyncSnapshot<T>,
    builder: impl Fn(&BuildContext, AsyncSnapshot<T>) -> AnyWidget + 'static,
) -> AnyWidget {
    stateful(AsyncBuilder {
        poll,
        snapshot: initial,
        builder: Rc::new(builder),
    })
}

/// Upstream `FutureBuilder<T>`, with a real future behind it.
///
/// The future is spawned at once, not when the widget first builds: a
/// `FutureBuilder` upstream is handed a future that is already running, and
/// starting it on first build would make the work depend on whether anything
/// was on screen.
///
/// `Result` rather than a bare `T` because [`AsyncSnapshot`] carries exactly
/// one of data or error, and a future that cannot fail would leave one of the
/// snapshot's own states unreachable. An infallible producer is
/// `async { Ok(value) }`.
///
/// The states a builder will see, in order: [`ConnectionState::Waiting`] with
/// `initial_data` if any, then [`ConnectionState::Done`] with the data or the
/// error. Upstream's `Active` belongs to `StreamBuilder`, which has more than
/// one value to deliver.
///
/// With no executor on this thread the future is dropped and the builder stays
/// at `Waiting` forever -- the same nothing that spawning into no executor
/// means everywhere else. That is a headless render or a unit test that has
/// not called [`task::attach`](crate::task::attach).
pub fn future_builder<T: Clone + 'static>(
    future: impl Future<Output = Result<T, String>> + 'static,
    initial_data: Option<T>,
    builder: impl Fn(&BuildContext, AsyncSnapshot<T>) -> AnyWidget + 'static,
) -> AnyWidget {
    let waiting = match initial_data {
        Some(data) => AsyncSnapshot::with_data(ConnectionState::Waiting, data),
        None => AsyncSnapshot::waiting(),
    };
    // Shared between the task that completes it and the poll that reads it.
    // The task holds one end for as long as it takes; the widget holds the
    // other for as long as it is on screen, and neither outliving the other is
    // required -- a completion nobody reads is simply not read.
    let cell = Rc::new(RefCell::new(waiting.clone()));
    let writing = Rc::clone(&cell);
    crate::task::spawn(async move {
        let settled = match future.await {
            Ok(data) => AsyncSnapshot::with_data(ConnectionState::Done, data),
            Err(error) => AsyncSnapshot::with_error(ConnectionState::Done, error),
        };
        *writing.borrow_mut() = settled;
        // No frame is asked for here. `rf_app_run_tasks` schedules one when
        // anything ran, which this did.
    });
    async_builder(Rc::new(move || cell.borrow().clone()), waiting, builder)
}

/// The `StatefulWidget` half -- upstream's `FutureBuilderState` polling
/// instead of subscribing.
pub struct AsyncBuilder<T: Clone + 'static> {
    poll: AsyncPoll<T>,
    snapshot: AsyncSnapshot<T>,
    builder: Rc<dyn Fn(&BuildContext, AsyncSnapshot<T>) -> AnyWidget>,
}

/// The state: just the last snapshot, upstream's `_summary`.
pub struct AsyncBuilderState<T> {
    snapshot: AsyncSnapshot<T>,
}

impl<T> Default for AsyncBuilderState<T> {
    fn default() -> Self {
        AsyncBuilderState {
            snapshot: AsyncSnapshot::nothing(),
        }
    }
}

impl<T: Clone + 'static> crate::framework::StatefulComponent for AsyncBuilder<T> {
    type State = AsyncBuilderState<T>;

    fn initial_state(&self) -> Self::State {
        AsyncBuilderState {
            snapshot: self.snapshot.clone(),
        }
    }

    fn build(
        &self,
        _state: &Self::State,
        _handle: StateHandle<Self::State>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        // Poll: whatever drives the future may have moved the answer since
        // the last frame.
        let snapshot = (self.poll)();
        (self.builder)(context, snapshot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_carry_exactly_one_of_data_or_error() {
        let nothing = AsyncSnapshot::<i32>::nothing();
        assert_eq!(nothing.connection_state, ConnectionState::None);
        assert!(!nothing.has_data() && !nothing.has_error());

        let with_data = AsyncSnapshot::with_data(ConnectionState::Active, 42);
        assert!(with_data.has_data() && !with_data.has_error());

        let with_error = AsyncSnapshot::<i32>::with_error(ConnectionState::Done, "boom");
        assert!(!with_error.has_data() && with_error.has_error());

        // inState keeps the payload, moves the state.
        let moved = with_data.in_state(ConnectionState::Done);
        assert_eq!(moved.connection_state, ConnectionState::Done);
        assert_eq!(moved.data, Some(42));
    }

    #[test]
    fn a_future_builder_waits_then_shows_what_the_future_gave() {
        crate::task::attach(None, None, std::ptr::null_mut());
        let (sender, receiver) = crate::task::oneshot::<Result<i32, String>>();
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let recorded = Rc::clone(&seen);
        let widget = future_builder(
            async move { receiver.await.unwrap_or(Err("gone".into())) },
            None,
            move |_context: &BuildContext, snapshot: AsyncSnapshot<i32>| {
                recorded.borrow_mut().push(snapshot);
                crate::framework::leaf(crate::render::RenderFullWidth::new)
            },
        );
        // Mounted, and rebuilt after the future lands, because the claim is
        // that a snapshot moves from waiting to done and a reader only ever
        // sees that through a build. The comment here used to say exactly that
        // and the assertions did not check it: the widget was dropped with
        // `let _ = widget` and the only claim made was that no task was left
        // pending, which is true of a future that resolved to anything at all.
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(widget.clone());
        tree.build_render_tree().expect("it mounted");
        assert_eq!(
            seen.borrow()
                .last()
                .map(|snapshot| snapshot.connection_state),
            Some(ConnectionState::Waiting),
            "nothing has arrived yet"
        );

        crate::task::run_until_stalled();
        sender.send(Ok(7));
        crate::task::run_until_stalled();
        assert_eq!(crate::task::pending(), 0, "the future finished");

        // A resolved future schedules no build of its own -- `future_builder`
        // says so where it spawns: "No frame is asked for here.
        // `rf_app_run_tasks` schedules one when anything ran." The poll is
        // read *during* a build, which is what makes this a poll rather than a
        // subscription. So the next frame has to be driven, and a frame in
        // this port is the whole tree rebuilt from the same widget.
        tree.rebuild(widget);
        tree.build_render_tree().expect("still mounted");

        let last = seen.borrow().last().cloned().expect("a snapshot");
        assert_eq!(last.data, Some(7), "and the value reached the builder");
        assert_eq!(last.connection_state, ConnectionState::Done);
        crate::task::detach();
    }

    #[test]
    fn a_future_builder_starts_its_future_without_waiting_to_be_built() {
        // Upstream is handed a future that is already running. Starting it on
        // first build would make the work depend on being on screen.
        crate::task::attach(None, None, std::ptr::null_mut());
        let started = Rc::new(std::cell::Cell::new(false));
        let flag = Rc::clone(&started);
        let _widget = future_builder(
            async move {
                flag.set(true);
                Ok::<i32, String>(1)
            },
            None,
            |_context: &BuildContext, _snapshot: AsyncSnapshot<i32>| {
                crate::framework::leaf(crate::render::RenderFullWidth::new)
            },
        );
        crate::task::run_until_stalled();
        assert!(started.get(), "ran without anything having been built");
        crate::task::detach();
    }

    #[test]
    fn a_future_that_fails_reaches_the_error_arm() {
        crate::task::attach(None, None, std::ptr::null_mut());
        let landed = Rc::new(std::cell::RefCell::new(AsyncSnapshot::<i32>::nothing()));
        let cell = Rc::clone(&landed);
        // The same shape `future_builder` builds internally, so the assertion
        // is about the mapping rather than about the widget.
        crate::task::spawn(async move {
            let settled: Result<i32, String> = Err("boom".into());
            *cell.borrow_mut() = match settled {
                Ok(data) => AsyncSnapshot::with_data(ConnectionState::Done, data),
                Err(error) => AsyncSnapshot::with_error(ConnectionState::Done, error),
            };
        });
        crate::task::run_until_stalled();
        let snapshot = landed.borrow().clone();
        assert_eq!(snapshot.connection_state, ConnectionState::Done);
        assert!(snapshot.has_error() && !snapshot.has_data());
        crate::task::detach();
    }

    #[test]
    fn a_builder_polls_and_shows_what_it_gets() {
        // Mounted, because the claim is about what the *builder* is handed and
        // that only happens during a build. This test used to construct the
        // widget, throw it away with `let _ = widget`, and assert
        // `(*poll)().data == Some(7)` -- which calls the closure the test
        // itself wrote and touches `async_builder` not at all. It passed
        // whatever the widget did.
        let poll: AsyncPoll<i32> = Rc::new(|| AsyncSnapshot::with_data(ConnectionState::Active, 7));
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let builder = {
            let seen = Rc::clone(&seen);
            move |_context: &BuildContext, snapshot: AsyncSnapshot<i32>| {
                seen.borrow_mut().push(snapshot);
                crate::framework::leaf(crate::render::RenderFullWidth::new)
            }
        };
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(async_builder(
            Rc::clone(&poll),
            AsyncSnapshot::waiting(),
            builder,
        ));
        tree.build_render_tree().expect("it mounted");

        let seen = seen.borrow();
        assert!(!seen.is_empty(), "the builder was never called");
        let last = seen.last().expect("a snapshot reached the builder");
        assert_eq!(
            last.data,
            Some(7),
            "the builder is handed what the poll answered, not the initial              snapshot it was given"
        );
        assert_eq!(last.connection_state, ConnectionState::Active);
    }
}
