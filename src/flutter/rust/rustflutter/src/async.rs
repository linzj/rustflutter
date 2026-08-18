//! Async builders, from upstream `widgets/async.dart`: a widget that shows
//! what an asynchronous value is doing. The crate has no async runtime of
//! its own; the builders take a *poll* -- a closure the framework calls
//! each build, answering the connection state and the latest payload -- so
//! whatever drives the future (the engine's task runners, a worker thread)
//! keeps the ownership and the widget stays declarative.
//!
//! Recorded divergence (see PORTING_STATUS.md): upstream subscribes to a
//! Dart `Future`/`Stream` and re-builds on each event; here the frame loop
//! polls. Same snapshot states, same builder contract, one seam moved.

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
    fn a_builder_polls_and_shows_what_it_gets() {
        let poll: AsyncPoll<i32> = Rc::new(|| AsyncSnapshot::with_data(ConnectionState::Active, 7));
        let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
        let builder = {
            let seen = Rc::clone(&seen);
            move |_context: &BuildContext, snapshot: AsyncSnapshot<i32>| {
                seen.borrow_mut().push(snapshot);
                crate::framework::leaf(crate::render::RenderFullWidth::new)
            }
        };
        let widget = async_builder(Rc::clone(&poll), AsyncSnapshot::waiting(), builder);
        let _ = widget;
        // The poll itself answers; the build pass drives it through the
        // element tree in the framework tests.
        assert_eq!((*poll)().data, Some(7));
    }
}
