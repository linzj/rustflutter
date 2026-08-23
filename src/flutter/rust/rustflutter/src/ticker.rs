// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The tick source, from upstream `scheduler/ticker.dart` and
//! `widgets/ticker_provider.dart`.
//!
//! An animation needs somebody to move its clock forward once a frame, and
//! upstream that somebody is a [`Ticker`]: started, it is called back every
//! frame with how long it has been running, and stopped, it completes the
//! future it handed out. A [`TickerProvider`] is whoever hands tickers out --
//! upstream a `State` mixin, so that a ticker dies with the widget that
//! wanted it and is muted when that widget goes offstage.
//!
//! # Where the frame comes from
//!
//! Upstream a ticker asks `SchedulerBinding` to schedule a frame and is
//! called back from it. This crate's frames are already on demand and every
//! mounted component gets [`advance`](crate::framework::StatefulComponent::advance)
//! once per frame, so a ticker is driven from there instead:
//! [`Tickers::tick`] in the state's `advance`, and its answer -- whether
//! anything is still ticking -- is exactly what `advance` returns.
//!
//! ```ignore
//! struct FadeState { tickers: Tickers, controller: Rc<AnimationController> }
//!
//! fn advance(&self, state: &mut FadeState, frame_time_micros: i64) -> bool {
//!     state.tickers.tick(frame_time_micros)
//! }
//! ```
//!
//! # Recorded divergences
//!
//! * [`TickerFuture`] is a settled-state machine first and a future second.
//!   Upstream's `whenCompleteOrCancel` is a method here too, and `orCancel`'s
//!   distinction between the two outcomes is the flag the callback is given;
//!   `.await` is layered on top of that (see [`TickerFuture::settled`])
//!   rather than
//!   underneath it, because the callback form has to keep working where there
//!   is no executor -- a unit test, a headless render.
//! * `Ticker.forceFrames` and the scheduler phases have nowhere to go: a
//!   frame here is asked for by returning true from `advance`, and there is
//!   no phase to be in when it is not.
//! * `SingleTickerProviderStateMixin` and `TickerProviderStateMixin` are
//!   [`SingleTicker`] and [`Tickers`]. Upstream's split is an allocation and
//!   an assertion; here it is the difference between an `Option` and a `Vec`,
//!   which is the same saving with the assertion built into the type.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};
use std::time::Duration;

/// What a ticker calls every frame: how long it has been running.
///
/// Upstream `TickerCallback`. The duration is measured from the ticker's
/// first tick, not from the last one -- an animation that is behind should
/// jump to where it ought to be rather than accumulate the lag.
pub type TickerCallback = Rc<dyn Fn(Duration)>;

/// Upstream `TickerProvider`: whatever can hand out a [`Ticker`].
pub trait TickerProvider {
    fn create_ticker(&self, on_tick: TickerCallback) -> Ticker;
}

/// Upstream `TickerCanceled`: what a cancelled ticker's future fails with.
///
/// Upstream it is an `Exception`; here nothing is thrown, so it is the value
/// a cancelled [`TickerFuture`] carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickerCanceled {
    /// The ticker's debug label, when it had one -- upstream's message says
    /// which ticker was cancelled and this is that.
    pub ticker_debug_label: Option<String>,
}

/// How a [`TickerFuture`] settled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickerFutureState {
    /// Still ticking.
    Pending,
    /// Stopped on its own terms -- upstream's completed future.
    Complete,
    /// Stopped early -- upstream's future failing with [`TickerCanceled`].
    Canceled,
}

/// Upstream `TickerFuture`: what [`Ticker::start`] hands back, settled when
/// the ticker stops.
pub struct TickerFuture {
    state: Cell<TickerFutureState>,
    /// Told on settling, with whether it completed rather than cancelled --
    /// upstream's `whenCompleteOrCancel`, whose callback runs either way.
    callbacks: RefCell<Vec<Rc<dyn Fn(bool)>>>,
    canceled: RefCell<Option<TickerCanceled>>,
}

impl TickerFuture {
    fn pending() -> Rc<TickerFuture> {
        Rc::new(TickerFuture {
            state: Cell::new(TickerFutureState::Pending),
            callbacks: RefCell::new(Vec::new()),
            canceled: RefCell::new(None),
        })
    }

    /// Upstream `TickerFuture.complete()`: an animation that ran to its end.
    pub fn complete(&self) {
        self.settle(TickerFutureState::Complete, None);
    }

    /// Upstream `TickerFuture._cancel`.
    pub fn cancel(&self, canceled: TickerCanceled) {
        self.settle(TickerFutureState::Canceled, Some(canceled));
    }

    fn settle(&self, state: TickerFutureState, canceled: Option<TickerCanceled>) {
        if self.state.get() != TickerFutureState::Pending {
            // Upstream's future can only complete once, and a ticker that is
            // stopped twice has already given its future away.
            return;
        }
        self.state.set(state);
        *self.canceled.borrow_mut() = canceled;
        let completed = state == TickerFutureState::Complete;
        for callback in self.callbacks.borrow().clone() {
            callback(completed);
        }
    }

    pub fn state(&self) -> TickerFutureState {
        self.state.get()
    }

    pub fn is_complete(&self) -> bool {
        self.state.get() == TickerFutureState::Complete
    }

    pub fn is_canceled(&self) -> bool {
        self.state.get() == TickerFutureState::Canceled
    }

    /// Upstream `TickerCanceled`, when this is how it ended.
    pub fn canceled(&self) -> Option<TickerCanceled> {
        self.canceled.borrow().clone()
    }

    /// The awaitable twin of
    /// [`when_complete_or_cancel`](Self::when_complete_or_cancel):
    /// `controller.forward().settled().await` is upstream's
    /// `await controller.forward()`.
    ///
    /// A method rather than an `IntoFuture` impl because what a caller holds is
    /// an `Rc<TickerFuture>`, and `Rc` is not a fundamental type -- the orphan
    /// rule will not have `impl IntoFuture for Rc<TickerFuture>`. Spelling the
    /// step out is the smaller price.
    ///
    /// The hard half is already done underneath: a ticker that has *already*
    /// settled tells its callback at once, so awaiting a settled future
    /// resolves on the first poll rather than parking forever.
    pub fn settled(&self) -> TickerSettled {
        let (sender, receiver) = crate::task::oneshot();
        // `RefCell<Option<Sender>>` because the callback list is `Fn`, not
        // `FnOnce`, and `Sender::send` consumes. Settling happens once, so the
        // take succeeds once and any later call finds `None`.
        let sender = RefCell::new(Some(sender));
        self.when_complete_or_cancel(Rc::new(move |completed| {
            if let Some(sender) = sender.borrow_mut().take() {
                sender.send(completed);
            }
        }));
        TickerSettled(receiver)
    }

    /// Upstream `whenCompleteOrCancel`: told either way, with which it was.
    ///
    /// A future that has already settled tells the callback at once, which is
    /// what awaiting a settled future does.
    pub fn when_complete_or_cancel(&self, callback: Rc<dyn Fn(bool)>) {
        match self.state.get() {
            TickerFutureState::Pending => self.callbacks.borrow_mut().push(callback),
            settled => callback(settled == TickerFutureState::Complete),
        }
    }
}

/// What awaiting a [`TickerFuture`] gives you: `true` if the ticker ran to its
/// end, `false` if it was cancelled.
///
/// Upstream's `TickerFuture` completes for one and raises `TickerCanceled` for
/// the other, and `orCancel` is the switch between them. A `bool` says the same
/// thing without a second error type, and [`TickerFuture::canceled`] still has
/// the details for a caller that wants them.
pub struct TickerSettled(crate::task::Receiver<bool>);

impl Future for TickerSettled {
    type Output = bool;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<bool> {
        // `Receiver` holds an `Rc`, so it is `Unpin` and can be re-pinned here
        // rather than projected.
        match Pin::new(&mut self.0).poll(context) {
            // The sender is held by the callback list, which lives as long as
            // the `TickerFuture`. It is dropped without sending only if the
            // ticker itself goes away unsettled, and a ticker that will never
            // settle did not complete.
            Poll::Ready(settled) => Poll::Ready(settled.unwrap_or(false)),
            Poll::Pending => Poll::Pending,
        }
    }
}


/// The part of a [`Ticker`] its handles share.
struct TickerInner {
    on_tick: TickerCallback,
    future: RefCell<Option<Rc<TickerFuture>>>,
    muted: Cell<bool>,
    /// The frame this ticker first ticked on, in the frame clock's
    /// microseconds -- upstream's `_startTime`, which is likewise not set
    /// until the first tick arrives.
    start_micros: Cell<Option<i64>>,
    debug_label: Option<String>,
}

/// Upstream `Ticker`: a callback, once a frame, for as long as it is
/// started.
///
/// A handle, not a value: the state is shared, so the ticker a widget keeps
/// and the ticker its callback closed over are the same ticker.
#[derive(Clone)]
pub struct Ticker {
    inner: Rc<TickerInner>,
}

impl Ticker {
    pub fn new(on_tick: TickerCallback) -> Ticker {
        Ticker {
            inner: Rc::new(TickerInner {
                on_tick,
                future: RefCell::new(None),
                muted: Cell::new(false),
                start_micros: Cell::new(None),
                debug_label: None,
            }),
        }
    }

    /// Upstream `Ticker(this._onTick, {this.debugLabel})`.
    pub fn with_debug_label(on_tick: TickerCallback, label: impl Into<String>) -> Ticker {
        let ticker = Ticker::new(on_tick);
        Ticker {
            inner: Rc::new(TickerInner {
                on_tick: Rc::clone(&ticker.inner.on_tick),
                future: RefCell::new(None),
                muted: Cell::new(false),
                start_micros: Cell::new(None),
                debug_label: Some(label.into()),
            }),
        }
    }

    /// Upstream `isActive`: started and not yet stopped.
    pub fn is_active(&self) -> bool {
        self.inner.future.borrow().is_some()
    }

    /// Upstream `isTicking`: active and not muted.
    ///
    /// Upstream also asks the scheduler whether frames are enabled at all;
    /// here that answer is the caller's, since a tick only happens inside a
    /// frame somebody asked for.
    pub fn is_ticking(&self) -> bool {
        self.is_active() && !self.muted()
    }

    /// Upstream `muted`: a muted ticker stays active and stops being called.
    pub fn muted(&self) -> bool {
        self.inner.muted.get()
    }

    /// Upstream's `set muted`. Unmuting does not rewind: the elapsed time a
    /// muted ticker's callback would have been given goes on running, which
    /// is what upstream's unscheduled-then-rescheduled tick does too.
    pub fn set_muted(&self, muted: bool) {
        self.inner.muted.set(muted);
    }

    /// Upstream `start()`: begins ticking and hands back the future that
    /// settles when it stops.
    pub fn start(&self) -> Rc<TickerFuture> {
        debug_assert!(
            !self.is_active(),
            "a ticker that is already active cannot be started again without first stopping it"
        );
        let future = TickerFuture::pending();
        *self.inner.future.borrow_mut() = Some(Rc::clone(&future));
        self.inner.start_micros.set(None);
        future
    }

    /// Upstream `stop({bool canceled = false})`.
    ///
    /// The future is taken out of the ticker before it is settled, so a
    /// callback that asks whether this is ticking is told no -- upstream
    /// takes the same care, and for the same reason.
    pub fn stop(&self, canceled: bool) {
        let Some(future) = self.inner.future.borrow_mut().take() else {
            return;
        };
        self.inner.start_micros.set(None);
        if canceled {
            future.cancel(TickerCanceled {
                ticker_debug_label: self.inner.debug_label.clone(),
            });
        } else {
            future.complete();
        }
    }

    /// One frame. Returns whether this ticker still wants the next one.
    ///
    /// Upstream's `_tick`: the first tick after a start is what sets the
    /// start time, so the elapsed duration a callback sees begins at zero
    /// however long the frame took to arrive.
    pub fn tick(&self, frame_time_micros: i64) -> bool {
        if !self.is_ticking() {
            return false;
        }
        let start = match self.inner.start_micros.get() {
            Some(start) => start,
            None => {
                self.inner.start_micros.set(Some(frame_time_micros));
                frame_time_micros
            }
        };
        let elapsed = Duration::from_micros((frame_time_micros - start).max(0) as u64);
        (self.inner.on_tick)(elapsed);
        // The callback may have stopped this ticker, or stopped and started
        // it again; either way the answer is read after it has run, as
        // upstream's `shouldScheduleTick` is.
        self.is_ticking()
    }

    /// Upstream `absorbTicker`: this ticker takes over the other's flight,
    /// and the other is stopped without cancelling.
    ///
    /// Upstream's `TickerProviderStateMixin` uses it when a `State` is moved
    /// with a `GlobalKey`, so an animation crossing the tree does not
    /// restart.
    pub fn absorb_ticker(&self, original: &Ticker) {
        debug_assert!(!self.is_active(), "an absorbing ticker must not be active");
        if original.is_active() {
            let future = original.inner.future.borrow_mut().take();
            self.inner
                .start_micros
                .set(original.inner.start_micros.get());
            *self.inner.future.borrow_mut() = future;
            original.inner.start_micros.set(None);
        }
        original.stop(false);
    }
}

// -- The provider states (upstream `widgets/ticker_provider.dart`) ------------

/// Upstream `SingleTickerProviderStateMixin`: a state that hands out exactly
/// one ticker.
///
/// Embed it in a component's `State`, ask it for the ticker in the first
/// build, and drive it from `advance`.
#[derive(Default)]
pub struct SingleTicker {
    ticker: RefCell<Option<Ticker>>,
    muted: Cell<bool>,
}

impl SingleTicker {
    pub fn new() -> SingleTicker {
        SingleTicker::default()
    }

    /// The ticker this state has, if it has been asked for one.
    pub fn ticker(&self) -> Option<Ticker> {
        self.ticker.borrow().clone()
    }

    /// Upstream's `TickerMode` reaching the tickers: an offstage subtree's
    /// tickers are muted, not stopped.
    pub fn set_muted(&self, muted: bool) {
        self.muted.set(muted);
        if let Some(ticker) = self.ticker.borrow().as_ref() {
            ticker.set_muted(muted);
        }
    }

    /// One frame, for the ticker this state holds.
    pub fn tick(&self, frame_time_micros: i64) -> bool {
        self.ticker
            .borrow()
            .as_ref()
            .is_some_and(|ticker| ticker.tick(frame_time_micros))
    }
}

impl TickerProvider for SingleTicker {
    /// Upstream asserts that this mixin is asked exactly once; here asking
    /// twice replaces the ticker, and the assertion says so in debug.
    fn create_ticker(&self, on_tick: TickerCallback) -> Ticker {
        debug_assert!(
            self.ticker.borrow().is_none(),
            "SingleTicker can only be used once; use Tickers for more than one"
        );
        let ticker = Ticker::new(on_tick);
        ticker.set_muted(self.muted.get());
        *self.ticker.borrow_mut() = Some(ticker.clone());
        ticker
    }
}

/// Upstream `TickerProviderStateMixin`: a state that hands out any number of
/// tickers and drives them together.
#[derive(Default)]
pub struct Tickers {
    tickers: RefCell<Vec<Ticker>>,
    muted: Cell<bool>,
}

impl Tickers {
    pub fn new() -> Tickers {
        Tickers::default()
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.set(muted);
        for ticker in self.tickers.borrow().iter() {
            ticker.set_muted(muted);
        }
    }

    pub fn muted(&self) -> bool {
        self.muted.get()
    }

    /// One frame, for every ticker this state holds. Returns whether any of
    /// them still wants the next one -- which is what a state's `advance`
    /// returns.
    pub fn tick(&self, frame_time_micros: i64) -> bool {
        let mut wants_frame = false;
        for ticker in self.tickers.borrow().clone() {
            wants_frame |= ticker.tick(frame_time_micros);
        }
        wants_frame
    }

    /// Upstream's `dispose`, which asserts every ticker was disposed of:
    /// stopping them is what this crate has instead, since a state that is
    /// gone stops being advanced anyway.
    pub fn stop_all(&self) {
        for ticker in self.tickers.borrow().iter() {
            ticker.stop(true);
        }
        self.tickers.borrow_mut().clear();
    }

    pub fn len(&self) -> usize {
        self.tickers.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.tickers.borrow().is_empty()
    }
}

impl TickerProvider for Tickers {
    fn create_ticker(&self, on_tick: TickerCallback) -> Ticker {
        let ticker = Ticker::new(on_tick);
        ticker.set_muted(self.muted.get());
        self.tickers.borrow_mut().push(ticker.clone());
        ticker
    }
}

/// Upstream `TickerModeData`: whether the tickers under a subtree run.
///
/// Upstream this is the value a `ValueListenable<TickerModeData>` carries, so
/// that a widget can hear the mode change without rebuilding; here it is the
/// provided value itself, and hearing it is rebuilding, since a provided
/// value that changed rebuilds its dependants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TickerModeData {
    pub enabled: bool,
}

impl TickerModeData {
    pub const fn new(enabled: bool) -> TickerModeData {
        TickerModeData { enabled }
    }
}

/// Upstream `TickerMode`: turns the tickers in a subtree off without
/// stopping them, which is what an offstage route does to the animations
/// underneath it.
pub struct TickerMode;

impl TickerMode {
    /// The subtree, with its tickers enabled or muted.
    pub fn new(enabled: bool, child: crate::framework::AnyWidget) -> crate::framework::AnyWidget {
        crate::framework::provide(TickerModeData::new(enabled), child)
    }

    /// Upstream `TickerMode.of(context)`, which is true where nobody said
    /// otherwise -- the tree's default is to animate.
    pub fn of(context: &mut crate::framework::BuildContext) -> bool {
        context
            .inherited::<TickerModeData>()
            .map_or(true, |data| data.enabled)
    }

    /// Upstream `TickerMode.getNotifier(context)`, whose value is what
    /// [`TickerMode::of`] answers.
    pub fn data_of(context: &mut crate::framework::BuildContext) -> TickerModeData {
        context
            .inherited::<TickerModeData>()
            .map_or(TickerModeData::new(true), |data| *data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ticker that records the elapsed duration it was told about.
    fn recording() -> (Ticker, Rc<RefCell<Vec<Duration>>>) {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&seen);
        let ticker = Ticker::new(Rc::new(move |elapsed| sink.borrow_mut().push(elapsed)));
        (ticker, seen)
    }

    #[test]
    fn an_awaited_ticker_future_resolves_with_how_it_settled() {
        crate::task::attach(None, std::ptr::null_mut());
        let (ticker, _seen) = recording();
        let settled = Rc::new(Cell::new(None));

        let future = ticker.start();
        let out = Rc::clone(&settled);
        let awaited = future.settled();
        crate::task::spawn(async move { out.set(Some(awaited.await)) });
        crate::task::run_until_stalled();
        assert_eq!(settled.get(), None, "still ticking");

        future.complete();
        crate::task::run_until_stalled();
        assert_eq!(settled.get(), Some(true), "ran to its end");
        crate::task::detach();
    }

    #[test]
    fn an_awaited_ticker_future_that_was_cancelled_says_so() {
        crate::task::attach(None, std::ptr::null_mut());
        let (ticker, _seen) = recording();
        let settled = Rc::new(Cell::new(None));
        let future = ticker.start();
        let out = Rc::clone(&settled);
        let awaited = future.settled();
        crate::task::spawn(async move { out.set(Some(awaited.await)) });
        crate::task::run_until_stalled();

        ticker.stop(true);
        crate::task::run_until_stalled();
        assert_eq!(settled.get(), Some(false));
        crate::task::detach();
    }

    #[test]
    fn awaiting_a_future_that_has_already_settled_resolves_at_once() {
        // The property `when_complete_or_cancel` already had, carried through
        // the wrapper: awaiting something finished must not park.
        crate::task::attach(None, std::ptr::null_mut());
        let (ticker, _seen) = recording();
        let future = ticker.start();
        future.complete();

        let settled = Rc::new(Cell::new(None));
        let out = Rc::clone(&settled);
        let awaited = future.settled();
        crate::task::spawn(async move { out.set(Some(awaited.await)) });
        crate::task::run_until_stalled();
        assert_eq!(settled.get(), Some(true), "resolved on the first poll");
        assert_eq!(crate::task::pending(), 0);
        crate::task::detach();
    }

    #[test]
    fn a_ticker_measures_from_its_first_tick_and_not_from_its_start() {
        let (ticker, seen) = recording();
        assert!(!ticker.is_active());
        ticker.start();
        assert!(ticker.is_active() && ticker.is_ticking());

        // The frame clock is already well past zero when the ticker starts;
        // the first tick is elapsed zero all the same.
        assert!(ticker.tick(1_000_000));
        assert!(ticker.tick(1_016_000));
        assert_eq!(
            *seen.borrow(),
            vec![Duration::ZERO, Duration::from_micros(16_000)]
        );
    }

    #[test]
    fn a_muted_ticker_stays_active_and_stops_being_called() {
        let (ticker, seen) = recording();
        ticker.start();
        ticker.tick(0);
        ticker.set_muted(true);
        assert!(ticker.is_active(), "muted is not stopped");
        assert!(!ticker.is_ticking());
        assert!(!ticker.tick(16_000));
        assert_eq!(seen.borrow().len(), 1);

        // Unmuting does not rewind: the clock ran while nobody was told.
        ticker.set_muted(false);
        assert!(ticker.tick(32_000));
        assert_eq!(
            *seen.borrow().last().expect("ticked"),
            Duration::from_micros(32_000)
        );
    }

    #[test]
    fn stopping_completes_the_future_and_cancelling_fails_it() {
        let (ticker, _) = recording();
        let future = ticker.start();
        let told = Rc::new(Cell::new(None));
        let sink = Rc::clone(&told);
        future.when_complete_or_cancel(Rc::new(move |completed| sink.set(Some(completed))));
        ticker.stop(false);
        assert_eq!(future.state(), TickerFutureState::Complete);
        assert_eq!(told.get(), Some(true));
        assert!(!ticker.is_active());

        let (ticker, _) = recording();
        let future = ticker.start();
        ticker.stop(true);
        assert_eq!(future.state(), TickerFutureState::Canceled);
        assert!(future.canceled().is_some());

        // A callback added after the fact is told at once.
        let late = Rc::new(Cell::new(None));
        let sink = Rc::clone(&late);
        future.when_complete_or_cancel(Rc::new(move |completed| sink.set(Some(completed))));
        assert_eq!(late.get(), Some(false));
    }

    #[test]
    fn a_stopped_ticker_that_starts_again_measures_from_scratch() {
        let (ticker, seen) = recording();
        ticker.start();
        ticker.tick(1_000_000);
        ticker.tick(1_050_000);
        ticker.stop(false);

        ticker.start();
        ticker.tick(2_000_000);
        assert_eq!(
            *seen.borrow(),
            vec![
                Duration::ZERO,
                Duration::from_micros(50_000),
                Duration::ZERO
            ]
        );
    }

    #[test]
    fn an_absorbed_ticker_keeps_the_flight_it_was_on() {
        let (original, seen) = recording();
        let future = original.start();
        original.tick(1_000_000);

        let (replacement, replacement_seen) = recording();
        replacement.absorb_ticker(&original);
        assert!(!original.is_active(), "the original gave its flight away");
        assert!(replacement.is_active());
        assert_eq!(
            future.state(),
            TickerFutureState::Pending,
            "the flight did not end, it changed hands"
        );

        // And the elapsed clock carried across rather than restarting.
        replacement.tick(1_050_000);
        assert_eq!(
            *replacement_seen.borrow(),
            vec![Duration::from_micros(50_000)]
        );
        assert_eq!(seen.borrow().len(), 1);
    }

    #[test]
    fn a_provider_drives_every_ticker_it_handed_out() {
        let tickers = Tickers::new();
        let count = Rc::new(Cell::new(0));
        for _ in 0..3 {
            let sink = Rc::clone(&count);
            let ticker = tickers.create_ticker(Rc::new(move |_| sink.set(sink.get() + 1)));
            ticker.start();
        }
        assert_eq!(tickers.len(), 3);
        assert!(tickers.tick(0));
        assert_eq!(count.get(), 3);

        // Muted, they stay handed out and stop being called -- and nothing
        // asks for another frame.
        tickers.set_muted(true);
        assert!(!tickers.tick(16_000));
        assert_eq!(count.get(), 3);

        tickers.stop_all();
        assert!(tickers.is_empty());
        assert!(!tickers.tick(32_000));
    }

    #[test]
    fn a_single_ticker_provider_hands_out_one_and_mutes_it_with_the_state() {
        let single = SingleTicker::new();
        let ticks = Rc::new(Cell::new(0));
        let sink = Rc::clone(&ticks);
        let ticker = single.create_ticker(Rc::new(move |_| sink.set(sink.get() + 1)));
        ticker.start();
        assert!(single.tick(0));
        assert_eq!(ticks.get(), 1);

        single.set_muted(true);
        assert!(!single.tick(16_000));
        assert_eq!(ticks.get(), 1);
        assert!(single.ticker().expect("handed out").is_active());
    }

    #[test]
    fn a_ticker_mode_mutes_the_subtree_under_it() {
        use crate::framework::{ElementTree, component, leaf};
        use crate::widgets::SizedBox;

        struct Reader(Rc<Cell<Option<bool>>>);

        impl crate::framework::Component for Reader {
            fn build(
                &self,
                context: &mut crate::framework::BuildContext,
            ) -> crate::framework::AnyWidget {
                self.0.set(Some(TickerMode::of(context)));
                leaf(|| SizedBox::new(1.0, 1.0))
            }
        }

        // Nothing said otherwise: the tree animates.
        let seen = Rc::new(Cell::new(None));
        let mut tree = ElementTree::new();
        tree.rebuild(component(Reader(Rc::clone(&seen))));
        assert_eq!(seen.get(), Some(true));

        let mut tree = ElementTree::new();
        tree.rebuild(TickerMode::new(false, component(Reader(Rc::clone(&seen)))));
        assert_eq!(seen.get(), Some(false));
    }
}
