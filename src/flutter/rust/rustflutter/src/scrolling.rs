// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Where a list is scrolled to, and what keeps it moving.
//!
//! A scroll offset looks like a number that a drag handler can add to, and for
//! as long as the finger is down that is all it is. The moment it lifts, the
//! offset becomes something else: a value with momentum, moving on its own,
//! for a while, and stopping where physics says rather than where the finger
//! left it. Without that, content stops dead the instant the finger does --
//! which is what a list feels like when nobody has written this file, and is
//! immediately obvious next to any other application on the device.
//!
//! [`Scroll`] is both halves. It holds the offset, it clamps against the
//! content, and it owns the flight: [`Scroll::fling`] starts a ballistic one
//! and [`Scroll::animate_to`] a driven one, and [`Scroll::advance`] moves
//! either along, once per frame, returning whether it wants another.
//!
//! # What upstream splits up
//!
//! Upstream this is three objects. `ScrollPosition` holds the offset and the
//! extents; `ScrollActivity` is what is currently in charge of it -- a
//! `DragScrollActivity` while a finger is down, a `BallisticScrollActivity`
//! after it lifts, a `DrivenScrollActivity` for `animateTo`, an
//! `IdleScrollActivity` when nothing is happening; and `ScrollPhysics` decides
//! which activity comes next and with what simulation. The split earns its
//! keep there because activities are pluggable: page snapping and overscroll
//! bouncing are each another activity. Dragging is a field here as it is
//! there, and everything else that can be in charge of the offset is one
//! enum, [`Motion`], because the day a third activity was wanted was the day
//! it became one.
//!
//! # Which way is positive
//!
//! `offset` grows as the reader goes further into the content, exactly as
//! upstream's `pixels` does, and a fling's velocity is in the same direction.
//! That is *opposite* to the finger: dragging down reveals earlier content, so
//! it decreases the offset. Handlers negate, and it is worth doing in the one
//! place they do it rather than here, because a wheel does not need negating
//! and a scrollbar drag does not either.
//!
//! The same holds for a viewport laid out the other way (an `Up` or `Left`
//! [`AxisDirection`](crate::render::AxisDirection)): the offset is still
//! measured from the start of the content, whichever screen edge that start
//! sits at, and the viewport is the one that turns the offset into a paint
//! translation. Upstream reverses the *finger*, not the offset, and it does
//! so in the drag plumbing -- `ScrollDragController._reversed` -- which is
//! why this file does not: the drag handlers are where that knowledge lives.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::animation::Curve;
use crate::framework::{Notification, NotificationSink};
use crate::physics::{ClampingScrollSimulation, Simulation};

// -- Scroll notifications -----------------------------------------------------

/// What a scrollable reports about itself along with a notification.
///
/// Upstream's `ScrollMetrics`, as far as a [`Scroll`] can know it: the offset,
/// the two ends of the content, and how much of it is on screen. The viewport
/// dimension is upstream's fourth field, `viewportDimension` -- it belongs to
/// the viewport, so it arrives when the extent does, in
/// [`Scroll::set_extent`], told by whoever laid the content out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollMetrics {
    /// How far into the content the view is. Upstream's `pixels`.
    pub pixels: f32,
    /// The least the offset can be. Zero in this port; upstream's
    /// `minScrollExtent` is negative for center-aligned and reversed viewports.
    pub min_scroll_extent: f32,
    /// How far the content can be scrolled. Upstream's `maxScrollExtent`.
    pub max_scroll_extent: f32,
    /// How much of the content is on screen. Upstream's `viewportDimension`;
    /// without it "where" cannot be turned into "how far along", which is what
    /// every listener above a scroll -- a scrollbar is one -- wants to know.
    pub viewport_dimension: f32,
}

impl ScrollMetrics {
    /// The quantity of content above the viewport. Upstream's
    /// `ScrollMetrics.extentBefore`: `max(pixels - minScrollExtent, 0.0)` --
    /// the content above what [`extent_inside`](ScrollMetrics::extent_inside)
    /// describes.
    pub fn extent_before(&self) -> f32 {
        (self.pixels - self.min_scroll_extent).max(0.0)
    }

    /// The quantity of content inside the viewport, empty space included when
    /// there is less content than viewport. Upstream's
    /// `ScrollMetrics.extentInside`: the viewport dimension less the overscroll
    /// at either end, each clamped so the answer stays between zero and the
    /// viewport -- it can be less while overscrolled, and never negative.
    pub fn extent_inside(&self) -> f32 {
        self.viewport_dimension
            - (self.min_scroll_extent - self.pixels).clamp(0.0, self.viewport_dimension)
            - (self.pixels - self.max_scroll_extent).clamp(0.0, self.viewport_dimension)
    }

    /// The quantity of content below the viewport. Upstream's
    /// `ScrollMetrics.extentAfter`: `max(maxScrollExtent - pixels, 0.0)` --
    /// the content below what [`extent_inside`](ScrollMetrics::extent_inside)
    /// describes.
    pub fn extent_after(&self) -> f32 {
        (self.max_scroll_extent - self.pixels).max(0.0)
    }

    /// Whether the offset is outside the content bounds. Upstream's
    /// `ScrollMetrics.outOfRange`:
    /// `pixels < minScrollExtent || pixels > maxScrollExtent`.
    pub fn out_of_range(&self) -> bool {
        self.pixels < self.min_scroll_extent || self.pixels > self.max_scroll_extent
    }

    /// Whether the offset is exactly at either end of the content. Upstream's
    /// `ScrollMetrics.atEdge`:
    /// `pixels == minScrollExtent || pixels == maxScrollExtent`.
    pub fn at_edge(&self) -> bool {
        self.pixels == self.min_scroll_extent || self.pixels == self.max_scroll_extent
    }
}

/// Which way the reader is moving through the content.
///
/// Upstream's `ScrollDirection`, and the same trap: it describes the *reader*,
/// not the offset. `Forward` is towards the start of the content, which is a
/// decreasing offset, and `Reverse` is towards the end, an increasing one --
/// because the finger and the offset point opposite ways (see "Which way is
/// positive" above).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollDirection {
    #[default]
    Idle,
    Forward,
    Reverse,
}

/// A notification from a scrollable, bubbling up through
/// [`notification_listener`](crate::framework::notification_listener)s above
/// it.
///
/// Upstream's `scroll_notification.dart` family -- `ScrollStartNotification`,
/// `ScrollUpdateNotification`, `OverscrollNotification`,
/// `ScrollEndNotification`, `UserScrollNotification` -- as one enum, because a
/// listener is chosen by exact type here and "everything about scrolling,
/// whichever kind" has to be one name to listen for. The variant is where
/// upstream would switch on `runtimeType`; the fields are that class's.
///
/// The lifecycle is upstream's, and the order it arrives in:
///
/// * [`Start`](ScrollNotification::Start) when the scrolling begins -- a
///   drag's first move, a fling, an [`animate_to`](Scroll::animate_to).
/// * [`Update`](ScrollNotification::Update) per change of position, and
///   [`Overscroll`](ScrollNotification::Overscroll) per change the content
///   bounds ate instead.
/// * [`UserScroll`](ScrollNotification::UserScroll) whenever the reader's
///   direction changes, including to [`Idle`](ScrollDirection::Idle).
/// * [`End`](ScrollNotification::End) when the scrolling stops -- a drag let
///   go without a throw, a fling settling, an animation arriving, a touch
///   catching a moving list.
#[derive(Clone, Copy, Debug)]
pub enum ScrollNotification {
    Start {
        metrics: ScrollMetrics,
        /// How many viewports this has bubbled through. Zero from the
        /// scrollable it came from; each enclosing viewport adds one, which is
        /// upstream's `ViewportNotificationMixin.depth` bumped by
        /// `ViewportElementMixin`. There are no viewport elements to bump it
        /// yet, so it is always zero -- the field is the seam it will arrive
        /// through.
        depth: u32,
    },
    Update {
        metrics: ScrollMetrics,
        /// How far the position moved, in logical pixels. Upstream's
        /// `scrollDelta`.
        scroll_delta: f32,
        depth: u32,
    },
    Overscroll {
        metrics: ScrollMetrics,
        /// How much of the requested motion the content bounds kept. Upstream's
        /// `overscroll`.
        overscroll: f32,
        /// How fast the scroll was going when it hit the bound, in logical
        /// pixels per second. Upstream's `velocity`, which a ballistic
        /// activity fills in with its simulation's and a drag leaves at the
        /// default of zero.
        velocity: f32,
        depth: u32,
    },
    End {
        metrics: ScrollMetrics,
        depth: u32,
    },
    UserScroll {
        metrics: ScrollMetrics,
        /// The reader's new direction. Idle means they stopped.
        direction: ScrollDirection,
        depth: u32,
    },
}

impl ScrollNotification {
    /// Where the scrollable was when this was dispatched.
    pub fn metrics(&self) -> ScrollMetrics {
        match *self {
            ScrollNotification::Start { metrics, .. }
            | ScrollNotification::Update { metrics, .. }
            | ScrollNotification::Overscroll { metrics, .. }
            | ScrollNotification::End { metrics, .. }
            | ScrollNotification::UserScroll { metrics, .. } => metrics,
        }
    }
}

impl Notification for ScrollNotification {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A scroll offset, its limit, and any fling in progress.
///
/// The limit lives behind an [`Rc<Cell>`](std::cell::Cell) because it is not
/// known when the offset is set: how far a list can scroll depends on how tall
/// its content turned out to be, which is settled during layout, a frame after
/// whoever holds the offset needed it. [`crate::widgets::ListView::with_extent_sink`]
/// fills it in from the other side.
#[derive(Clone)]
pub struct Scroll {
    /// How far into the content the view is, in logical pixels. Upstream's
    /// `pixels`, which `forcePixels` can leave outside the content bounds
    /// until the next layout corrects it; here a
    /// [`jump_to`](Scroll::jump_to) can do the same, and
    /// [`set_extent`](Scroll::set_extent) is the correction.
    pub offset: f32,
    /// What this scroll and its viewport say to each other: how far the
    /// content can scroll and how much of it is on screen, filled in at
    /// layout, and the reveals the render tree asks for. See [`ScrollLink`].
    link: Rc<ScrollLink>,
    /// What is in flight, if anything: a fling or an animation. Upstream's
    /// current `ScrollActivity`.
    activity: Option<Activity>,
    /// Where this scroll's notifications are dispatched from, if it has been
    /// given anywhere to dispatch them to. Bound with
    /// [`Scroll::set_notification_sink`] during a build; without one the
    /// scrolling works exactly as before and says nothing to anyone.
    sink: RefCell<Option<NotificationSink>>,
    /// Whether something that constitutes scrolling is in charge: a drag, a
    /// fling, an animation. Upstream's `activity.isScrolling`, and the answer
    /// to "would `beginActivity` dispatch a start or an end notification" --
    /// false going to true dispatches the start, true going to false the end.
    scrolling: Cell<bool>,
    /// Which way the reader is last known to be moving. Upstream's
    /// `userScrollDirection`; kept so a [`UserScrollNotification`] goes out
    /// only when it actually changed.
    user_direction: Cell<ScrollDirection>,
}

/// A motion in flight: what is moving the offset, and when it started.
#[derive(Clone, Copy)]
struct Activity {
    motion: Motion,
    /// When it started, in frame-clock microseconds. Not known when the motion
    /// is created -- the finger lifts between frames -- so it is taken from
    /// the first frame that advances it, which is also the first frame that
    /// could draw it. Upstream a `Ticker` does the same thing: its elapsed
    /// duration is measured from its first tick, not from `start`.
    started_micros: Option<i64>,
}

/// What is in charge of the offset once the finger is off it. Upstream these
/// are activities -- `BallisticScrollActivity` for a fling,
/// `DrivenScrollActivity` for `animateTo` -- and they are one enum here
/// because they are driven the same way: ask a simulation where the offset is
/// now, once a frame, until it is done or something replaces them.
#[derive(Clone, Copy)]
enum Motion {
    /// A fling, its physics deciding where it goes and when it stops.
    Ballistic(ClampingScrollSimulation),
    /// An [`Scroll::animate_to`]: a chosen distance in a chosen time.
    Driven(Driven),
}

impl Motion {
    /// What to ask for the offset, per frame. Both variants answer, because
    /// both are simulations upstream too -- the driven one is what an
    /// `AnimationController.animateTo` runs, as `_InterpolationSimulation`.
    fn simulation(&self) -> &dyn Simulation {
        match self {
            Motion::Ballistic(simulation) => simulation,
            Motion::Driven(driven) => driven,
        }
    }
}

/// An [`Scroll::animate_to`] being played out.
///
/// A line from `from` to `to`, bent by `curve` and walked over `duration`
/// seconds. Upstream's `DrivenScrollActivity` holds exactly this in an
/// `AnimationController.unbounded` started at `from` and `animateTo`'d to
/// `to`; this is that controller's simulation, sampled on the same frame
/// clock a fling is.
#[derive(Clone, Copy)]
struct Driven {
    /// Where the animation started.
    from: f32,
    /// Where it is going. Not clamped to the content: the per-frame clamp in
    /// [`Scroll::advance`] stops the animation at the end of it instead, as
    /// upstream's `setPixels` does.
    to: f32,
    /// How long it takes, in seconds.
    duration: f32,
    /// The shape of the travel.
    curve: Curve,
}

impl Simulation for Driven {
    fn x(&self, time: f32) -> f32 {
        if self.duration <= 0.0 {
            return self.to;
        }
        let t = (time / self.duration).clamp(0.0, 1.0);
        self.from + (self.to - self.from) * self.curve.transform(t)
    }

    /// A difference, not a formula: a Bezier's slope has no closed form, so
    /// this is the central difference upstream's `_InterpolationSimulation`
    /// takes over the tolerance's time.
    fn dx(&self, time: f32) -> f32 {
        const EPSILON: f32 = 1e-3;
        (self.x(time + EPSILON) - self.x(time - EPSILON)) / (2.0 * EPSILON)
    }

    fn is_done(&self, time: f32) -> bool {
        // Strictly past the end, not at it: upstream's
        // `_InterpolationSimulation.isDone` is `timeInSeconds >
        // _durationInSeconds`, so the frame that lands exactly on the duration
        // is still going. It is also already at `to`, which `x` clamps, so the
        // arrival is the same either way.
        time > self.duration
    }
}

/// How slow a throw may be and still start a fling, in logical pixels per
/// second. Upstream's tolerance velocity, `ScrollPhysics.toleranceFor`'s
/// `1.0 / (0.050 * devicePixelRatio)`; this port's logical pixels are the
/// device-independent kind at a ratio of one, so the number is 20.
const FLING_TOLERANCE_VELOCITY: f32 = 20.0;

/// How far apart two offsets may be and still count as the same place, when
/// the question is whether an animation to the second is worth starting.
/// Upstream's tolerance distance, `toleranceFor`'s `1.0 / devicePixelRatio`
/// logical pixels.
const SCROLL_TOLERANCE_DISTANCE: f32 = 1.0;

/// What a viewport and the [`Scroll`] that holds its offset say to each other.
///
/// Upstream needs no such thing. A `RenderViewportBase` holds its
/// `ViewportOffset`, and that object *is* the `ScrollPosition`: reading the
/// offset, reporting the extent and moving the offset are three method calls
/// on one thing. Here the offset lives on the application's state, because
/// that is what lets a drag and a fling move it from where the events arrive
/// -- so the two halves are joined by this handle instead.
///
/// It carries both directions, and that is the point of it being one type:
///
///   * **down**, what only layout knows -- how far the content can scroll and
///     how much of it is on screen, settled when the viewport is measured;
///   * **up**, what only the render tree can decide -- a *reveal*: "put the
///     offset here, so that something inside can be seen". A focused text
///     field about to be covered by the keyboard is what asks.
///
/// One handle rather than two sinks because a scrollable that wired the extent
/// and forgot the reveal would look completely normal and silently never
/// scroll a focused field into view. There is nothing to forget: whatever is
/// given a link can do both.
#[derive(Debug, Default)]
pub struct ScrollLink {
    extent: Cell<f32>,
    viewport: Cell<f32>,
    /// A reveal asked for by the render tree and not yet acted on. Taken by
    /// [`Scroll::advance`] on the next frame, which is the first moment there
    /// is a frame clock to animate against.
    reveal: Cell<Option<(f32, crate::render::Reveal)>>,
}

impl ScrollLink {
    /// What the last layout measured. Called by the viewport.
    pub fn set_measurements(&self, extent: f32, viewport: f32) {
        self.extent.set(extent);
        self.viewport.set(viewport);
    }

    /// How far the content can scroll, as last measured.
    pub fn extent(&self) -> f32 {
        self.extent.get()
    }

    /// How much of the content is on screen, as last measured.
    pub fn viewport(&self) -> f32 {
        self.viewport.get()
    }

    /// Asks for the offset to go to `offset`. Called by the viewport, from a
    /// [`crate::render::RenderRef::show_on_screen`] walk.
    ///
    /// The last ask wins: two reveals in one frame mean the second knows
    /// something the first did not, which is upstream's behaviour too -- its
    /// `offset.moveTo` simply replaces whatever activity was running.
    pub fn request_reveal(&self, offset: f32, reveal: crate::render::Reveal) {
        self.reveal.set(Some((offset, reveal)));
    }

    /// Takes the pending reveal, if any.
    pub fn take_reveal(&self) -> Option<(f32, crate::render::Reveal)> {
        self.reveal.take()
    }
}

impl Default for Scroll {
    fn default() -> Scroll {
        Scroll {
            offset: 0.0,
            link: Rc::new(ScrollLink::default()),
            activity: None,
            sink: RefCell::new(None),
            scrolling: Cell::new(false),
            user_direction: Cell::new(ScrollDirection::Idle),
        }
    }
}

impl Scroll {
    pub fn new() -> Scroll {
        Scroll::default()
    }

    /// How far this list can scroll. Zero until something has measured it.
    pub fn max_extent(&self) -> f32 {
        self.link.extent().max(0.0)
    }

    /// The handle a viewport is given, so it can report what it measured and
    /// ask for what it needs to show. See [`ScrollLink`].
    pub fn link(&self) -> Rc<ScrollLink> {
        Rc::clone(&self.link)
    }

    /// Records how far the list can scroll and how much of it is on screen,
    /// for callers that measure the content themselves rather than handing
    /// [`extent`](Scroll::extent) to a [`ListView`](crate::widgets::ListView).
    ///
    /// Both numbers in one call because upstream hands them to its position
    /// together, in `applyNewDimensions`: they are settled by the same layout,
    /// and a listener that got one without the other could not answer "how far
    /// along" -- a [`Scrollbar`](crate::scrollbar::Scrollbar) is exactly that
    /// listener.
    ///
    /// The same call is where an offset put out of range by
    /// [`jump_to`](Scroll::jump_to) -- which stores what it was given, as
    /// upstream's `forcePixels` does -- comes back into range: upstream's
    /// layout corrects the pixels when the dimensions arrive, in
    /// `applyContentDimensions`, and this is that moment here. A jump made
    /// before anything was measured therefore survives to be measured.
    pub fn set_extent(&mut self, extent: f32, viewport_dimension: f32) {
        self.link.set_measurements(extent, viewport_dimension);
        self.offset = self.offset.clamp(0.0, self.max_extent());
    }

    /// Gives this scroll somewhere to dispatch its notifications from.
    ///
    /// ```ignore
    /// fn build(&self, state: &State, handle: StateHandle<State>, context: &mut BuildContext) -> AnyWidget {
    ///     state.scroll.set_notification_sink(context.notification_sink());
    ///     ListView::new().with_offset(state.scroll.offset)
    /// }
    /// ```
    ///
    /// Takes `&self` for the same reason [`Scroll::set_extent`] does: the one
    /// moment the context exists is a build, which sees the state read-only.
    /// The sink names this element, so the listeners that hear the scroll are
    /// the ones above it -- upstream's arrangement, where a `Scrollbar` is an
    /// ancestor of the `Scrollable` and hears its notifications from there.
    pub fn set_notification_sink(&self, sink: NotificationSink) {
        *self.sink.borrow_mut() = Some(sink);
    }

    // -- Notification dispatch ------------------------------------------------
    //
    // Upstream's `ScrollPosition` reports through `didStartScroll` and
    // friends, which hand a notification to the current activity to dispatch
    // through the Scrollable's context. The activities are one enum here, so
    // the reporting is three small helpers the movers call at the moments the
    // activity transitions would have.

    pub fn metrics(&self) -> ScrollMetrics {
        ScrollMetrics {
            pixels: self.offset,
            min_scroll_extent: 0.0,
            max_scroll_extent: self.max_extent(),
            viewport_dimension: self.link.viewport(),
        }
    }

    fn notify(&self, notification: ScrollNotification) {
        if let Some(sink) = self.sink.borrow().as_ref() {
            sink.dispatch(&notification);
        }
    }

    /// Scrolling has begun, unless it already had.
    ///
    /// Upstream's `beginActivity` trading a non-scrolling activity for a
    /// scrolling one: the drag taking over from idle, the ballistic taking
    /// over from the hold. A scrolling activity taking over from another
    /// scrolling one -- a thrown drag becoming a fling -- dispatches nothing,
    /// which is why this is idempotent.
    fn start_scroll(&self) {
        if self.scrolling.replace(true) {
            return;
        }
        self.notify(ScrollNotification::Start {
            metrics: self.metrics(),
            depth: 0,
        });
    }

    /// Scrolling has stopped, if it had not already.
    ///
    /// The end first and the user direction going idle after it, in that
    /// order, because that is the order upstream's `beginActivity` does both
    /// in: `didEndScroll`, then `updateUserScrollDirection(idle)`.
    fn end_scroll(&self) {
        if !self.scrolling.replace(false) {
            return;
        }
        self.notify(ScrollNotification::End {
            metrics: self.metrics(),
            depth: 0,
        });
        self.update_user_direction(ScrollDirection::Idle);
    }

    /// The reader's direction changed, if it did.
    fn update_user_direction(&self, direction: ScrollDirection) {
        if self.user_direction.replace(direction) == direction {
            return;
        }
        self.notify(ScrollNotification::UserScroll {
            metrics: self.metrics(),
            direction,
            depth: 0,
        });
    }

    /// Applies `delta` to the offset and reports what happened, which is
    /// upstream's `setPixels`: an update for the part that moved, an
    /// overscroll for the part the content bounds kept.
    fn move_by(&mut self, delta: f32) {
        let max = self.max_extent();
        let requested = self.offset + delta;
        let applied = requested.clamp(0.0, max);
        let overscroll = requested - applied;

        if applied != self.offset {
            let scroll_delta = applied - self.offset;
            self.offset = applied;
            self.notify(ScrollNotification::Update {
                metrics: self.metrics(),
                scroll_delta,
                depth: 0,
            });
        }
        if overscroll != 0.0 {
            // A drag's overscroll carries no velocity upstream -- only the
            // ballistic activity's does, and this is the drag and wheel path.
            self.notify(ScrollNotification::Overscroll {
                metrics: self.metrics(),
                overscroll,
                velocity: 0.0,
                depth: 0,
            });
        }
    }

    /// Moves by `delta` and stays inside the content, as one whole scroll.
    ///
    /// This is upstream's `pointerScroll`, the wheel notch: a scroll that
    /// begins and ends in a single event. Whatever was in flight ends first --
    /// `goIdle` -- then the reader's new direction is reported, then the move
    /// as a start, its update, and the end; a wheel scroll left open would
    /// leave everything above it thinking a scroll was still underway, which
    /// is why the end is here rather than left to whatever comes next. A
    /// delta of zero ends whatever was in flight and reports nothing, exactly
    /// as upstream's early return.
    ///
    /// Clamping to the content rather than in the viewport is what stops an
    /// overscroll from banking travel: without it, flinging past the end and
    /// dragging back would do nothing until the imaginary distance had been
    /// paid off. The part the bounds keep is an
    /// [`Overscroll`](ScrollNotification::Overscroll), so a scroll pinned at
    /// the edge is still heard -- by a
    /// [`Scrollbar`](crate::scrollbar::Scrollbar), among others.
    pub fn scroll_by(&mut self, delta: f32) {
        self.activity = None;
        self.end_scroll();
        if delta == 0.0 {
            return;
        }
        // The direction is the reader's, so it is negated with the finger: a
        // positive delta here is further into the content, which upstream
        // calls reverse. See "Which way is positive" above.
        if delta < 0.0 {
            self.update_user_direction(ScrollDirection::Forward);
        } else {
            self.update_user_direction(ScrollDirection::Reverse);
        }
        self.start_scroll();
        self.move_by(delta);
        self.end_scroll();
    }

    /// Puts the offset somewhere, without any physics. For jumping to a
    /// position rather than travelling to it.
    ///
    /// Ends whatever was in flight, as upstream's `jumpTo` goes idle before it
    /// moves. The new offset is there the moment this returns; the frame that
    /// shows it is whatever the caller does next, because a jump has nothing
    /// to wait for and never asks for one.
    ///
    /// The target is stored exactly as given, clamped by nobody here --
    /// upstream's `forcePixels`, which `jumpTo` moves with, stores the raw
    /// value and leaves it to the next layout to bring the position back into
    /// range ([`set_extent`](Scroll::set_extent) is that correction here). So
    /// a jump made before the content has been measured is not silently lost
    /// to an extent of zero, and the notifications a jump past the ends
    /// dispatches say where the position was really put. A jump that moves
    /// reports as a whole scroll of its own -- start, one update, end -- and
    /// no overscroll, which is upstream's `jumpTo` exactly: idle, then the
    /// three dispatched by hand between `forcePixels` and `goBallistic`.
    pub fn jump_to(&mut self, offset: f32) {
        self.activity = None;
        self.end_scroll();
        if offset != self.offset {
            let scroll_delta = offset - self.offset;
            self.offset = offset;
            self.start_scroll();
            self.notify(ScrollNotification::Update {
                metrics: self.metrics(),
                scroll_delta,
                depth: 0,
            });
            self.end_scroll();
        }
    }

    /// Stops whatever is in flight where it is. What a finger touching the
    /// content does.
    ///
    /// Upstream's `hold`: a touch trades whatever is in flight for a hold,
    /// which does not constitute scrolling, so a fling or an animation being
    /// caught ends here. A drag in charge is left alone -- a second finger on
    /// a list someone is already dragging does not end their drag -- and ends
    /// when the drag ends, at the [`fling`](Scroll::fling) that follows it.
    pub fn stop(&mut self) {
        if self.activity.take().is_some() {
            self.end_scroll();
        }
    }

    /// Whether anything is in flight: a fling or an
    /// [`animate_to`](Scroll::animate_to).
    ///
    /// Upstream's position always has an activity -- idle is one -- so the
    /// question there is which kind is in charge, and this answers it for
    /// exactly the two that run on a ticker, a `BallisticScrollActivity` and a
    /// `DrivenScrollActivity`: false, and the frame loop can go back to sleep.
    /// Whether a scroll is underway at all, a drag included, is the other
    /// question -- upstream's `activity.isScrolling` -- and the `scrolling`
    /// cell is what tracks it here, because a drag is scrolling that costs no
    /// frames.
    pub fn is_ballistic(&self) -> bool {
        self.activity.is_some()
    }

    /// Starts a fling at `velocity` logical pixels per second, in offset
    /// space -- positive meaning further into the content.
    ///
    /// Does nothing when there is nowhere to go, which is upstream's
    /// `ClampingScrollPhysics.createBallisticSimulation` returning null: a
    /// dead throw -- a velocity under `ScrollPhysics.toleranceFor`'s
    /// velocity -- or already at the end the fling is heading for. Starting
    /// one anyway would cost a run of frames that each clamp to the same
    /// number. That null
    /// simulation is `goBallistic` going idle instead, so these are also the
    /// ways a drag that was not thrown ends: an
    /// [`End`](ScrollNotification::End) notification, and nothing starts.
    ///
    /// When the fling does start, this is upstream's drag handing the offset
    /// to a ballistic activity, scrolling to scrolling: no end, no second
    /// start, and the end arrives when the fling settles. From idle it is a
    /// start, like any scrolling activity taking over.
    pub fn fling(&mut self, velocity: f32) {
        self.activity = None;
        // The dead throw, first of upstream's nulls: below the tolerance
        // velocity there is no simulation, and the release is an end and an
        // idle and nothing more.
        if velocity.abs() < FLING_TOLERANCE_VELOCITY {
            self.end_scroll();
            return;
        }
        if velocity > 0.0 && self.offset >= self.max_extent() {
            self.end_scroll();
            return;
        }
        if velocity < 0.0 && self.offset <= 0.0 {
            self.end_scroll();
            return;
        }
        self.start_scroll();
        self.activity = Some(Activity {
            motion: Motion::Ballistic(ClampingScrollSimulation::new(self.offset, velocity)),
            started_micros: None,
        });
    }

    /// Animates the offset to `target` over `duration_micros` on `curve`.
    ///
    /// Upstream's `ScrollPosition.animateTo`: driven rather than thrown -- a
    /// chosen distance in a chosen time, not a particle left to physics. There
    /// are no defaults upstream, where `duration` and `curve` are required, so
    /// there are none here; a caller wanting the usual ones passes
    /// [`Curve::EASE`] and whatever duration suits the distance. A zero
    /// duration is a [`jump_to`](Scroll::jump_to), as upstream's `moveTo`
    /// treats `Duration.zero`.
    ///
    /// Runs on the same channel a fling does: [`Scroll::advance`] moves it
    /// once a frame, it stops at the end of the content rather than pushing
    /// past it, and whatever else takes over -- a drag, a touch, a
    /// `jump_to`, another `animate_to`, a `fling` -- replaces it, exactly as
    /// upstream's `beginActivity` replaces the current activity.
    ///
    /// A driven activity is a scrolling one, so starting it from idle
    /// dispatches a [`Start`](ScrollNotification::Start) and arriving
    /// dispatches the [`End`](ScrollNotification::End); starting it over a
    /// drag in flight dispatches neither, as no scrolling-to-scrolling
    /// transition does.
    pub fn animate_to(&mut self, target: f32, duration_micros: i64, curve: Curve) {
        // Already there, to within a pixel: upstream skips the animation and
        // jumps -- `nearEqual(to, pixels, tolerance)`, whose tolerance here is
        // the scroll one, a logical pixel, rather than the simulation
        // machinery's thousandth.
        if (target - self.offset).abs() <= SCROLL_TOLERANCE_DISTANCE {
            self.jump_to(target);
            return;
        }
        if duration_micros <= 0 {
            self.jump_to(target);
            return;
        }
        self.start_scroll();
        self.activity = Some(Activity {
            motion: Motion::Driven(Driven {
                from: self.offset,
                to: target,
                duration: duration_micros as f32 / 1_000_000.0,
                curve,
            }),
            started_micros: None,
        });
    }

    /// Moves a fling or an animation on by one frame, and says whether another
    /// is wanted.
    ///
    /// Call once per frame from a
    /// [`StatefulComponent::advance`](crate::framework::StatefulComponent::advance).
    /// Returns false when nothing is moving, which is what lets the frame loop
    /// go back to sleep.
    ///
    /// Each frame that moves dispatches an [`Update`](ScrollNotification::Update);
    /// the frame the motion finishes -- or runs into the end of the content --
    /// dispatches the [`End`](ScrollNotification::End) and nothing further.
    pub fn advance(&mut self, frame_time_micros: i64) -> bool {
        // A reveal the render tree asked for during the last frame's paint --
        // a focused field getting out from under the keyboard. Taken before
        // the early return below, because a scroll standing still is exactly
        // the case a reveal has to be able to start from.
        //
        // Upstream does this without a hop: `showInViewport` calls
        // `offset.moveTo` and the position it is holding moves there and then.
        // Here the offset is the application's, and this is the first moment
        // it is in hand -- with a frame clock, which is what an animated
        // reveal needs.
        let revealed = match self.link.take_reveal() {
            Some((target, reveal)) => {
                self.animate_to(target, reveal.duration_micros, reveal.curve);
                true
            }
            None => false,
        };

        // Ask the simulation first and let it go, before anything is notified:
        // the notification is dispatched into the tree, which may rebuild, and
        // nothing there may still be borrowed when it is.
        //
        // `revealed` rather than `false`, and that is not a nicety: a reveal
        // with no duration is a jump, a jump leaves no activity behind, and
        // answering "nothing is moving" to the frame loop would end the frame
        // that was going to draw the offset this just changed. The keyboard's
        // reveal is exactly that jump -- see `Reveal::NOW`.
        let Some(activity) = self.activity.as_mut() else {
            return revealed;
        };
        let started = *activity.started_micros.get_or_insert(frame_time_micros);
        let elapsed = (frame_time_micros - started).max(0) as f32 / 1_000_000.0;
        let simulation = activity.motion.simulation();
        let position = simulation.x(elapsed);
        let velocity = simulation.dx(elapsed);
        let done = simulation.is_done(elapsed);

        let max = self.max_extent();
        let clamped = position.clamp(0.0, max);
        let moved = clamped != self.offset;
        if moved {
            self.move_by(clamped - self.offset);
        }

        // Hitting either end ends the motion, however much of the simulation
        // is left: the content has run out, and continuing would be a run of
        // frames that each clamp to the same number. Upstream stops the same
        // way, a fling and an animation both -- every activity's `applyMoveTo`
        // returns false when the position could not go where the simulation
        // asked, and the activity goes idle.
        if done || clamped != position {
            self.activity = None;
            if clamped != position {
                // The part the content bounds kept, on the way to going idle,
                // carrying how fast it was going: upstream's
                // `BallisticScrollActivity` dispatches its overscroll with
                // `velocity: velocity`, read off the simulation.
                self.notify(ScrollNotification::Overscroll {
                    metrics: self.metrics(),
                    overscroll: position - clamped,
                    velocity,
                    depth: 0,
                });
            }
            self.end_scroll();
            return moved;
        }
        true
    }
}

// -- Lazy lists ---------------------------------------------------------------

/// How far beyond the visible window items are still built.
///
/// Upstream's `RenderAbstractViewport.defaultCacheExtent`, and the same number.
/// Building exactly what is visible means the first row of a fling is built in
/// the frame it appears, which is the frame that has the least time to spare;
/// a screen's worth of margin at each end costs a few items and removes that.
pub const DEFAULT_CACHE_EXTENT: f32 = 250.0;

/// Upstream `CacheExtentStyle` (`rendering/viewport.dart`): what unit a
/// viewport's cache extent is written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CacheExtentStyle {
    /// Logical pixels, taken literally.
    #[default]
    Pixel,
    /// A multiplier of the viewport's own main-axis extent.
    Viewport,
}

/// Upstream `ScrollCacheExtent`: a cache extent together with the unit it is
/// written in.
///
/// The two are kept in one value because **neither half means anything
/// alone**. `250` is a screenful on a phone and a sliver of a desktop window;
/// `0.5` is half a viewport or half a pixel. Upstream models this as a sealed
/// class with a `pixels` and a `viewport` constructor rather than as a pair of
/// loose parameters, which is what makes the mismatched combination
/// unwritable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollCacheExtent {
    pub value: f32,
    pub style: CacheExtentStyle,
}

impl ScrollCacheExtent {
    /// Upstream's `ScrollCacheExtent.pixels`.
    pub fn pixels(value: f32) -> ScrollCacheExtent {
        ScrollCacheExtent {
            value,
            style: CacheExtentStyle::Pixel,
        }
    }

    /// Upstream's `ScrollCacheExtent.viewport`.
    pub fn viewport(value: f32) -> ScrollCacheExtent {
        ScrollCacheExtent {
            value,
            style: CacheExtentStyle::Viewport,
        }
    }

    /// Upstream's `_calculateCacheOffset`: the extent in logical pixels, once
    /// the viewport's size is known.
    ///
    /// The pixel form **ignores the argument entirely** -- that is the
    /// difference between the two, not a special case of it.
    pub fn in_pixels(&self, main_axis_extent: f32) -> f32 {
        match self.style {
            CacheExtentStyle::Pixel => self.value,
            CacheExtentStyle::Viewport => self.value * main_axis_extent,
        }
    }

    /// Upstream's assert:
    ///
    /// ```dart
    /// assert(cacheExtent != null || cacheExtentStyle == CacheExtentStyle.pixel)
    /// ```
    ///
    /// **The default only exists in one of the two units.** Leaving the extent
    /// out is fine in pixels, where [`DEFAULT_CACHE_EXTENT`] means a screenful
    /// or so; as a multiplier the same number would ask for 250 viewports of
    /// cache, so upstream refuses the combination rather than inventing a
    /// second default nobody wrote down.
    pub fn is_legal(value: Option<f32>, style: CacheExtentStyle) -> bool {
        value.is_some() || style == CacheExtentStyle::Pixel
    }

    /// What a viewport ends up with when no extent was given.
    pub fn defaulted(value: Option<f32>, style: CacheExtentStyle) -> Option<ScrollCacheExtent> {
        match (value, style) {
            (Some(value), style) => Some(ScrollCacheExtent { value, style }),
            (None, CacheExtentStyle::Pixel) => {
                Some(ScrollCacheExtent::pixels(DEFAULT_CACHE_EXTENT))
            }
            (None, CacheExtentStyle::Viewport) => None,
        }
    }
}

impl Default for ScrollCacheExtent {
    /// `ScrollCacheExtent.pixels(RenderAbstractViewport.defaultCacheExtent)`.
    fn default() -> ScrollCacheExtent {
        ScrollCacheExtent::pixels(DEFAULT_CACHE_EXTENT)
    }
}

/// Which items a fixed-extent list needs to have built.
///
/// The whole point of a fixed extent: the answer is arithmetic rather than
/// measurement, so a list of a hundred thousand rows costs the same as a list
/// of ten. Upstream this is `SliverFixedExtentList`'s
/// `getMinChildIndexForScrollOffset` / `getMaxChildIndexForScrollOffset`, and
/// the rounding is theirs -- an offset that lands within a rounding error of an
/// item boundary counts as being on it, which stops a row from flickering in
/// and out when a fling leaves the offset a ten-thousandth of a pixel short.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemWindow {
    /// First index to build.
    pub first: usize,
    /// Last index to build, inclusive. Equal to `first` for one item.
    pub last: usize,
}

impl ItemWindow {
    pub fn len(&self) -> usize {
        self.last + 1 - self.first
    }

    /// Always false, and that is a fact about the type rather than a stub.
    ///
    /// A window is a closed range of indices to build, and there is no such
    /// thing as one covering nothing: the builder returns `Option<ItemWindow>`
    /// and says so with `None` -- no items, or an extent that cannot divide a
    /// scroll offset. What it returns instead clamps `last` up to `first`, so
    /// `first <= last` holds by construction.
    ///
    /// Which is also what makes [`ItemWindow::len`] safe to write as
    /// `last + 1 - first` over `usize`: the one arrangement that would
    /// underflow is the one the constructor cannot produce.
    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn contains(&self, index: usize) -> bool {
        index >= self.first && index <= self.last
    }
}

/// Rounding slack, as upstream's `precisionErrorTolerance`.
const PRECISION_ERROR: f32 = 1e-10;

/// Which items to build for a viewport of `viewport` showing `offset`.
///
/// `count` items of `extent` each, plus [`DEFAULT_CACHE_EXTENT`] of margin at
/// both ends. Returns `None` when there is nothing to show at all.
pub fn item_window(
    count: usize,
    extent: f32,
    offset: f32,
    viewport: f32,
    cache_extent: f32,
) -> Option<ItemWindow> {
    if count == 0 || extent <= 0.0 {
        return None;
    }
    let start = (offset - cache_extent).max(0.0);
    let end = offset + viewport + cache_extent;

    let first = {
        let actual = start / extent;
        let round = actual.round();
        if (actual * extent - round * extent).abs() < PRECISION_ERROR {
            round
        } else {
            actual.floor()
        }
    };
    let last = {
        let actual = end / extent - 1.0;
        let round = actual.round();
        let index = if (actual * extent - round * extent).abs() < PRECISION_ERROR {
            round
        } else {
            actual.ceil()
        };
        index.max(0.0)
    };

    let first = (first.max(0.0) as usize).min(count - 1);
    let last = (last.max(0.0) as usize).min(count - 1);
    Some(ItemWindow {
        first,
        last: last.max(first),
    })
}

#[cfg(test)]
mod item_window_emptiness_tests {
    use super::item_window;

    #[test]
    fn no_items_is_no_window_rather_than_an_empty_one() {
        // Emptiness lives in the Option, which is why is_empty can answer a
        // constant false without lying.
        assert_eq!(item_window(0, 50.0, 0.0, 500.0, 0.0), None);
        assert_eq!(item_window(10, 0.0, 0.0, 500.0, 0.0), None);
    }

    #[test]
    fn and_every_window_that_does_exist_holds_at_least_one_item() {
        // Including the degenerate arrangements: a viewport of nothing, and an
        // offset scrolled past the end of a short list.
        for (count, extent, offset, viewport) in [
            (1usize, 50.0f32, 0.0f32, 0.0f32),
            (1, 50.0, 0.0, 500.0),
            (3, 50.0, 10_000.0, 100.0),
            (10, 50.0, 100.0, 0.0),
            (100, 50.0, 0.0, 500.0),
        ] {
            let window = item_window(count, extent, offset, viewport, 0.0)
                .expect("a list with items has a window");
            assert!(window.first <= window.last, "{count} {offset}");
            assert!(window.len() >= 1, "{count} {offset}");
            assert!(!window.is_empty(), "{count} {offset}");
        }
    }
}

/// A list that builds only the items it is showing.
///
/// Upstream this is `ListView.builder` over a `SliverFixedExtentList`: the
/// items are all the same height, so which ones are on screen is arithmetic,
/// and the ones that are not are never built. A list of a hundred thousand rows
/// costs what a screenful costs.
///
/// The items that are not built are still *accounted for*: the list reserves
/// their space with a gap at each end, so the scrollbar, the extent and the
/// offsets are all what they would be if every row existed.
///
/// ```ignore
/// component(
///     LazyList::new(photos.len(), 72.0, move |index| row_for(index))
///         .with_offset(state.scroll.offset)
///         .with_viewport(size.height),
/// )
/// ```
///
/// # What a variable extent would need
///
/// Upstream's plain `SliverList` allows items of different heights, and pays
/// for it: it cannot know where item ten thousand is without having laid out
/// the nine thousand nine hundred and ninety-nine before it, so it keeps a
/// running estimate and corrects it as the reader scrolls. That is a different
/// piece of work and it needs render objects that survive a frame. This is the
/// half that does not.
pub struct LazyList {
    count: usize,
    extent: f32,
    offset: f32,
    viewport: f32,
    cache_extent: f32,
    spacing: f32,
    build_item: Rc<dyn Fn(usize) -> crate::framework::AnyWidget>,
}

impl LazyList {
    pub fn new(
        count: usize,
        extent: f32,
        build_item: impl Fn(usize) -> crate::framework::AnyWidget + 'static,
    ) -> LazyList {
        LazyList {
            count,
            extent,
            offset: 0.0,
            viewport: 0.0,
            cache_extent: DEFAULT_CACHE_EXTENT,
            spacing: 0.0,
            build_item: Rc::new(build_item),
        }
    }

    /// How far the list is scrolled.
    pub fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    /// How much of the list is visible. Without this the list builds one
    /// screenful of cache and nothing else, which is what a viewport of zero
    /// means.
    pub fn with_viewport(mut self, viewport: f32) -> Self {
        self.viewport = viewport;
        self
    }

    pub fn with_cache_extent(mut self, cache_extent: f32) -> Self {
        self.cache_extent = cache_extent.max(0.0);
        self
    }

    /// Space between items. Counted in the extent, as upstream counts a
    /// separator: `item_extent` is one item plus one gap.
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// How tall the whole list is, built or not.
    pub fn content_extent(&self) -> f32 {
        if self.count == 0 {
            0.0
        } else {
            self.count as f32 * (self.extent + self.spacing) - self.spacing
        }
    }

    /// How far this list can scroll, given the viewport it is in.
    pub fn max_scroll_extent(&self) -> f32 {
        (self.content_extent() - self.viewport).max(0.0)
    }

    /// Which items this list would build right now.
    pub fn window(&self) -> Option<ItemWindow> {
        item_window(
            self.count,
            self.extent + self.spacing,
            self.offset,
            self.viewport,
            self.cache_extent,
        )
    }
}

impl crate::framework::Component for LazyList {
    fn build(&self, _context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        let Some(window) = self.window() else {
            return crate::framework::leaf(|| crate::widgets::Empty);
        };
        let step = self.extent + self.spacing;
        // The items that are not built are still taken up: a gap at each end,
        // as tall as the rows it stands in for. Without it every offset below
        // the window would be wrong by however much was skipped.
        //
        // A spacer that stands for nothing is left out rather than given a
        // height of zero, and one that stands for something is a gap shorter
        // than the rows it replaces. Both because the column puts a gap
        // *between* its children: the gap above the first built row is the
        // column's to add, and counting it here as well made a spaced list
        // two gaps taller than its own content.
        let leading = (window.first as f32 * step - self.spacing).max(0.0);
        let after = self.count - 1 - window.last;
        let trailing = (after as f32 * step - self.spacing).max(0.0);

        let mut children = Vec::with_capacity(window.len() + 2);
        if window.first > 0 {
            children.push(crate::framework::leaf(move || {
                crate::render::RenderConstrainedBox::tight(0.0, leading)
            }));
        }
        for index in window.first..=window.last {
            // Keyed by index, so an item keeps its element -- and therefore
            // its state -- when the window moves. Matching by position would
            // hand row eleven's state to row ten as soon as one scrolled off
            // the top.
            children.push(crate::framework::keyed_single(
                index as u64,
                // In a layer of its own, as upstream's
                // `SliverChildBuilderDelegate` does by default
                // (`addRepaintBoundaries`), and for the reason a list is the
                // example everyone gives: the rows that did not change are
                // every row, and scrolling moves them rather than redrawing
                // them.
                crate::widgets::repaint_boundary((self.build_item)(index)),
                // **And a semantic index**, which is the same delegate's
                // `addSemanticIndexes` and equally on by default. It is what
                // lets a reader be told "item 3 of 40" instead of meeting
                // forty indistinguishable rows: the row's *position in the
                // list*, which nothing downstream can work out, because by the
                // time the walk sees these rows the ones scrolled past are
                // already gone.
                //
                // Everything below this had been ported and had no producer --
                // `RenderIndexedSemanticsBox`, the walk's `pending_index`, and
                // `RfSemanticsNode::scroll_index` crossing to the engine. This
                // one line is what they were waiting for.
                move |child| crate::render::RenderIndexedSemanticsBox::new(index as i64, child),
            ));
        }
        if after > 0 {
            children.push(crate::framework::leaf(move || {
                crate::render::RenderConstrainedBox::tight(0.0, trailing)
            }));
        }

        let spacing = self.spacing;
        crate::framework::many(children, move |rendered| {
            let mut column = crate::render::RenderFlex::column()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Stretch)
                .with_spacing(spacing);
            for child in rendered {
                column = column.push(child);
            }
            // The column itself, not a box round it: a second box between the
            // handle and the object puts the object one layer away from the
            // description it is asked to compare itself against, and it would
            // quietly decline every rebuild. See `RenderBox for Box<R>`.
            column
        })
    }
}

// -- The sliver door -----------------------------------------------------------

/// A lazily built list through the sliver protocol.
///
/// The door onto [`crate::render::RenderSliverList`] -- a sliver whose
/// children outside the window do not exist -- inside a
/// [`crate::render::RenderSliverViewport`], with a
/// [`crate::render::RenderSliverPadding`] in front of it when padding was
/// asked for. Upstream this is `ListView.builder` (a `CustomScrollView` with a
/// `SliverPadding` and a `SliverList`); [`LazyList`] above is this port's
/// fixed-extent shortcut that predates the slivers and stays. The two read
/// alike:
///
/// ```ignore
/// component(
///     SliverListView::new(1000, move |index| row(index))
///         .with_item_extent(40.0)
///         .with_offset(state.scroll.offset),
/// )
/// ```
///
/// The builder answers *render objects* rather than widgets, because the list
/// builds its children while it is being laid out -- where the widget walk has
/// already been and gone. What a builder cannot know -- where row ten thousand
/// is -- the list answers by dead reckoning plus a scroll offset correction,
/// seeded from the item extent or estimate it was given (see
/// [`crate::render::RenderSliverList`], whose doc says which parts are
/// upstream and which one thing is not).
///
/// The viewport, the padding sliver and the list sliver survive every frame,
/// which is the whole reason a sliver list is cheaper than the column it
/// replaces: a rebuild hands the same objects their new configuration, and the
/// materialized window -- every child in it, measured -- moves rather than
/// being made again.
#[derive(Clone)]
pub struct SliverListView {
    axis_direction: crate::render::AxisDirection,
    child_count: usize,
    build_item: Rc<dyn Fn(usize) -> crate::render::RenderRef>,
    item_extent: Option<f32>,
    estimated_extent: Option<f32>,
    padding: Option<crate::render::EdgeInsets>,
    offset: f32,
    cache_extent: f32,
    user_scroll_direction: ScrollDirection,
    link: Option<Rc<ScrollLink>>,
}

impl SliverListView {
    pub fn new(
        child_count: usize,
        build_item: impl Fn(usize) -> crate::render::RenderRef + 'static,
    ) -> SliverListView {
        SliverListView {
            axis_direction: crate::render::AxisDirection::Down,
            child_count,
            build_item: Rc::new(build_item),
            item_extent: None,
            estimated_extent: None,
            padding: None,
            offset: 0.0,
            cache_extent: DEFAULT_CACHE_EXTENT,
            user_scroll_direction: ScrollDirection::Idle,
            link: None,
        }
    }

    /// A horizontal list. Which way it scrolls follows the ambient text
    /// direction where the list was built, the same line upstream's
    /// `ScrollView` builds its viewport with: rightward in an LTR subtree,
    /// leftward in an RTL one.
    pub fn horizontal(
        child_count: usize,
        build_item: impl Fn(usize) -> crate::render::RenderRef + 'static,
    ) -> SliverListView {
        let axis_direction =
            if crate::direction::current_direction() == crate::direction::TextDirection::Rtl {
                crate::render::AxisDirection::Left
            } else {
                crate::render::AxisDirection::Right
            };
        SliverListView {
            axis_direction,
            ..Self::new(child_count, build_item)
        }
    }

    /// The exact extent of every child, when every child has one. Makes the
    /// lazy window arithmetic exact -- upstream would say to use a
    /// `SliverFixedExtentList`, and this is the same arithmetic arrived at
    /// from the same fact.
    pub fn with_item_extent(mut self, item_extent: f32) -> Self {
        self.item_extent = Some(item_extent);
        self
    }

    /// What to estimate a child's extent at when they vary. Only read on a
    /// far jump, and corrected into truth as the reader scrolls.
    pub fn with_estimated_extent(mut self, estimated_extent: f32) -> Self {
        self.estimated_extent = Some(estimated_extent);
        self
    }

    /// Pads the list, as a `SliverPadding` in front of the `SliverList` --
    /// padding that scrolls with the content, upstream's
    /// `ListView(padding: ...)`.
    pub fn with_padding(mut self, padding: crate::render::EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// How far the content is scrolled. Clamped to the scrollable extent once
    /// the content has been measured.
    pub fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    /// The axis direction to lay the viewport out in -- the door a reversed
    /// list (`Up`, `Left`) comes in by.
    pub fn with_axis_direction(mut self, axis_direction: crate::render::AxisDirection) -> Self {
        self.axis_direction = axis_direction;
        self
    }

    /// The band before the leading and after the trailing edge kept warm.
    pub fn with_cache_extent(mut self, cache_extent: f32) -> Self {
        self.cache_extent = cache_extent;
        self
    }

    /// Reports how far this list can scroll, once it has been laid out. The
    /// cell is the way back out; see [`ListView`]'s note on the same trick.
    pub fn with_link(mut self, link: Rc<ScrollLink>) -> Self {
        self.link = Some(link);
        self
    }
}

impl crate::framework::Component for SliverListView {
    fn build(&self, _context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        // The host is mounted as a leaf so that it -- not this description --
        // is what the element tree reconciles against, and the sliver chain
        // under it survives the rebuild.
        let config = self.clone();
        crate::framework::leaf(move || SliverListHost::new(config.clone()))
    }
}

/// The render half of [`SliverListView`]: composes the sliver chain once and
/// keeps it, handing the same objects their new configuration every rebuild.
/// Upstream the elements between the widgets and the render objects are what
/// keeps the chain alive across a rebuild; here the host is.
struct SliverListHost {
    config: SliverListView,
    /// The list sliver, and the padding around it when there is one. The
    /// viewport's child is whichever of the two is the outermost.
    sliver: Option<crate::render::RenderRef>,
    padding_sliver: Option<crate::render::RenderRef>,
    viewport: Option<crate::render::RenderSliverViewport>,
}

impl SliverListHost {
    fn new(config: SliverListView) -> SliverListHost {
        SliverListHost {
            config,
            sliver: None,
            padding_sliver: None,
            viewport: None,
        }
    }

    /// A fresh list sliver describing the current configuration, for
    /// reconfiguring the kept one with.
    fn fresh_sliver(config: &SliverListView) -> crate::render::RenderSliverList {
        // The `Rc` is cloned into a plain closure because `Rc<dyn Fn>` is not
        // itself an `Fn`, and the list asks for the latter.
        let build = Rc::clone(&config.build_item);
        let mut sliver =
            crate::render::RenderSliverList::new(config.child_count, move |index| build(index));
        if let Some(extent) = config.item_extent {
            sliver = sliver.with_item_extent(extent);
        }
        if let Some(extent) = config.estimated_extent {
            sliver = sliver.with_estimated_extent(extent);
        }
        sliver
    }
}

impl crate::render::RenderBox for SliverListHost {
    /// The host's half of the reconciliation: the list sliver is reconfigured
    /// first (which reconfigures every live child with its freshly built
    /// self, upstream's element rebuild visiting them), then the padding, then
    /// the viewport -- staged around the *same* handles, so the viewport's
    /// same-children test passes and the window below survives the rebuild.
    fn update_from(
        &mut self,
        fresh: &mut dyn crate::render::RenderBox,
    ) -> Option<crate::render::UpdateEffect> {
        let fresh = fresh.as_any_mut().downcast_mut::<SliverListHost>()?;
        self.config = fresh.config.clone();
        let Some(sliver) = self.sliver.clone() else {
            // Never composed: the first layout builds out of what was just
            // taken.
            return Some(crate::render::UpdateEffect::Relayout);
        };
        let mut effect = crate::render::UpdateEffect::Nothing;
        if !sliver.reconfigure(crate::render::RenderRef::new(Self::fresh_sliver(
            &self.config,
        ))) {
            return None;
        }
        // The padding sliver comes and goes with the configuration; either
        // way the root of the chain is whatever the viewport is handed.
        let root = match (self.padding_sliver.take(), self.config.padding) {
            (Some(padding), Some(insets)) => {
                let staged = crate::render::RenderSliverPadding::new(insets, sliver.clone());
                if !padding.reconfigure(crate::render::RenderRef::new(staged)) {
                    return None;
                }
                effect = effect.and(crate::render::UpdateEffect::Relayout);
                padding
            }
            (None, Some(insets)) => {
                // Padding added to a list that did not have it: a new sliver
                // in front of the list, which the viewport is restaged with.
                let padding = crate::render::RenderRef::new(
                    crate::render::RenderSliverPadding::new(insets, sliver.clone()),
                );
                self.padding_sliver = Some(padding.clone());
                effect = effect.and(crate::render::UpdateEffect::Relayout);
                padding
            }
            (Some(_), None) => {
                // Padding removed: the list is the root again.
                effect = effect.and(crate::render::UpdateEffect::Relayout);
                sliver.clone()
            }
            (None, None) => sliver.clone(),
        };
        let mut staged = crate::render::RenderSliverViewport::new(self.config.axis_direction)
            .with_sliver(root)
            .with_offset(self.config.offset)
            .with_cache_extent(self.config.cache_extent)
            .with_user_scroll_direction(self.config.user_scroll_direction);
        self.viewport
            .as_mut()
            .expect("built with the slivers")
            .update_from(&mut staged)
            .map(|viewport_effect| effect.and(viewport_effect))
    }

    fn layout(&mut self, constraints: crate::render::BoxConstraints) -> crate::render::Size {
        if self.viewport.is_none() {
            let sliver = crate::render::RenderRef::new(Self::fresh_sliver(&self.config));
            let root = if let Some(insets) = self.config.padding {
                let padding = crate::render::RenderRef::new(
                    crate::render::RenderSliverPadding::new(insets, sliver.clone()),
                );
                self.padding_sliver = Some(padding.clone());
                padding
            } else {
                sliver.clone()
            };
            self.sliver = Some(sliver);
            self.viewport = Some(
                crate::render::RenderSliverViewport::new(self.config.axis_direction)
                    .with_sliver(root)
                    .with_offset(self.config.offset)
                    .with_cache_extent(self.config.cache_extent)
                    .with_user_scroll_direction(self.config.user_scroll_direction),
            );
        }
        let viewport = self.viewport.as_mut().expect("built just above");
        let size = viewport.layout(constraints);
        if let Some(link) = &self.config.link {
            link.set_measurements(viewport.max_scroll_extent(), size.height);
        }
        size
    }

    fn size(&self) -> crate::render::Size {
        self.viewport
            .as_ref()
            .map_or(crate::render::Size::ZERO, |v| v.size())
    }

    fn paint(&self, context: &mut crate::render::PaintContext, offset: crate::render::Offset) {
        if let Some(viewport) = &self.viewport {
            viewport.paint(context, offset);
        }
    }

    fn visit_children(
        &self,
        visit: &mut dyn FnMut(&dyn crate::render::RenderBox, crate::render::Offset),
    ) {
        if let Some(viewport) = &self.viewport {
            visit(viewport, crate::render::Offset::ZERO);
        }
    }

    fn hit_test(
        &self,
        position: crate::render::Offset,
        result: &mut crate::render::HitTestResult,
    ) -> bool {
        self.viewport
            .as_ref()
            .is_some_and(|v| v.hit_test(position, result))
    }
}

// -- Lists whose items are not all the same height ----------------------------

/// What a variable-extent list has learned about how tall its items are.
///
/// Upstream's `RenderSliverList` keeps the same knowledge on the children
/// themselves, as `SliverMultiBoxAdaptorParentData.layoutOffset`, and can
/// afford to because it creates its children inside its own `performLayout`:
/// it lays one out, learns its height, and knows where the next one starts.
///
/// Nothing here can do that. Children are elements, and elements are built one
/// step before anything is measured -- so the window has to be chosen from what
/// is already known. The measurements are therefore kept beside the list rather
/// than inside it, and a frame reads what the frame before it wrote. That is
/// already how this framework answers "how far can this scroll" (see
/// [`Scroll::extent`]) and it is the same one-frame lag, for the same reason.
///
/// A book belongs to whoever holds the scroll offset, and lives as long as it
/// does. Sharing is the point: two frames of the same list are the same list.
#[derive(Clone, Default)]
pub struct ExtentBook(Rc<RefCell<Book>>);

#[derive(Default)]
struct Book {
    /// Measured extents by index. `None` for an item that has never been laid
    /// out -- which at the start is all of them.
    measured: Vec<Option<f32>>,
    /// Sum of the measured ones, and how many there are, so the average is not
    /// a walk.
    total: f32,
    known: usize,
    /// Bumped whenever a measurement arrives that was not already the answer.
    /// A caller that wants the correction applied without waiting for the next
    /// thing to happen watches this and asks for a frame.
    revision: u64,
}

impl ExtentBook {
    pub fn new() -> ExtentBook {
        ExtentBook::default()
    }

    /// Records how tall item `index` turned out. Called from layout.
    pub fn record(&self, index: usize, extent: f32) {
        if !extent.is_finite() || extent < 0.0 {
            return;
        }
        let mut book = self.0.borrow_mut();
        if book.measured.len() <= index {
            book.measured.resize(index + 1, None);
        }
        match book.measured[index] {
            Some(before) if before == extent => return,
            Some(before) => book.total += extent - before,
            None => {
                book.total += extent;
                book.known += 1;
            }
        }
        book.measured[index] = Some(extent);
        book.revision += 1;
    }

    /// What was measured for `index`, if it has ever been on screen.
    pub fn measured(&self, index: usize) -> Option<f32> {
        self.0.borrow().measured.get(index).copied().flatten()
    }

    /// How many items have been measured.
    pub fn known(&self) -> usize {
        self.0.borrow().known
    }

    /// Changes so far. Only useful compared against an earlier reading.
    pub fn revision(&self) -> u64 {
        self.0.borrow().revision
    }

    //--------------------------------------------------------------------------
    /// The height to assume for an item nobody has measured.
    ///
    /// The average of the ones that have been, which is upstream's
    /// `_extrapolateMaxScrollOffset`: it takes the extent of the children it
    /// currently has and spreads it over the ones it does not. The difference
    /// is that this averages over everything ever measured rather than only
    /// what is alive now, which is strictly better informed and converges to
    /// the same number.
    ///
    /// `fallback` is the answer before anything at all has been measured, and
    /// it is the caller's guess -- upstream has no equivalent because its first
    /// layout is a real one.
    pub fn average(&self, fallback: f32) -> f32 {
        let book = self.0.borrow();
        if book.known == 0 {
            fallback.max(0.0)
        } else {
            book.total / book.known as f32
        }
    }

    /// Where item `index` starts, counting from the top of the content.
    fn offset_of(&self, index: usize, fallback: f32, spacing: f32) -> f32 {
        let average = self.average(fallback);
        let book = self.0.borrow();
        let mut at = 0.0;
        for item in 0..index {
            at += book
                .measured
                .get(item)
                .copied()
                .flatten()
                .unwrap_or(average)
                + spacing;
        }
        at
    }

    /// How tall the whole list is, measured where it can be and estimated
    /// where it cannot.
    fn content_extent(&self, count: usize, fallback: f32, spacing: f32) -> f32 {
        if count == 0 {
            return 0.0;
        }
        let average = self.average(fallback);
        let book = self.0.borrow();
        let mut total = 0.0;
        for index in 0..count {
            total += book
                .measured
                .get(index)
                .copied()
                .flatten()
                .unwrap_or(average);
        }
        total + spacing * (count - 1) as f32
    }

    //--------------------------------------------------------------------------
    /// Which items a viewport at `offset` needs built.
    ///
    /// The fixed-extent answer is arithmetic ([`item_window`]); this one is a
    /// walk, because the only way to know where item ten thousand is, is to
    /// add up the nine thousand nine hundred and ninety-nine before it.
    /// Upstream pays the same price and manages it the same way -- it walks
    /// from the children it already has rather than from zero. Here the walk is
    /// over a vector of floats, which is cheap enough that starting from zero
    /// is not worth avoiding until a list is long enough to prove otherwise.
    fn window(
        &self,
        count: usize,
        offset: f32,
        viewport: f32,
        cache_extent: f32,
        fallback: f32,
        spacing: f32,
    ) -> Option<ItemWindow> {
        if count == 0 {
            return None;
        }
        let average = self.average(fallback);
        if average <= 0.0 && spacing <= 0.0 {
            return None;
        }
        let start = (offset - cache_extent).max(0.0);
        let end = offset + viewport + cache_extent;

        let book = self.0.borrow();
        let mut at = 0.0;
        let mut first = None;
        let mut last = 0;
        for index in 0..count {
            let extent = book
                .measured
                .get(index)
                .copied()
                .flatten()
                .unwrap_or(average);
            let bottom = at + extent;
            // An item that starts at or past the end of the window is below it,
            // and so is everything after it.
            if first.is_some() && at >= end - PRECISION_ERROR {
                break;
            }
            // An item that ends exactly where the window starts is above it,
            // which is the same boundary the fixed-extent arithmetic draws.
            if bottom > start + PRECISION_ERROR {
                if first.is_none() {
                    first = Some(index);
                }
                last = index;
            }
            at = bottom + spacing;
        }
        let first = first.unwrap_or(count - 1);
        Some(ItemWindow {
            first,
            last: last.max(first),
        })
    }
}

/// A list that builds only the items it is showing, without being told how
/// tall they are.
///
/// Upstream this is `ListView.builder` over a plain `SliverList`, as opposed to
/// the `SliverFixedExtentList` that [`LazyList`] is. The difference is the
/// whole of it: a fixed extent makes "which items are on screen" arithmetic,
/// and without one the list has to remember what it measured and estimate the
/// rest. So this needs two things [`LazyList`] does not -- an [`ExtentBook`]
/// that outlives the frame, and render objects that report their height into it
/// as they are laid out.
///
/// ```ignore
/// component(
///     VariableExtentList::new(messages.len(), state.extents.clone(), move |index| {
///         row_for(index)
///     })
///     .with_estimate(72.0)
///     .with_offset(state.scroll.offset)
///     .with_viewport(size.height),
/// )
/// ```
pub struct VariableExtentList {
    count: usize,
    book: ExtentBook,
    /// What to assume before anything has been measured. Only ever used for
    /// the first frame; after that the average of what was measured is a
    /// better guess than any constant.
    estimate: f32,
    offset: f32,
    viewport: f32,
    cache_extent: f32,
    spacing: f32,
    build_item: Rc<dyn Fn(usize) -> crate::framework::AnyWidget>,
}

impl VariableExtentList {
    pub fn new(
        count: usize,
        book: ExtentBook,
        build_item: impl Fn(usize) -> crate::framework::AnyWidget + 'static,
    ) -> VariableExtentList {
        VariableExtentList {
            count,
            book,
            estimate: DEFAULT_ITEM_ESTIMATE,
            offset: 0.0,
            viewport: 0.0,
            cache_extent: DEFAULT_CACHE_EXTENT,
            spacing: 0.0,
            build_item: Rc::new(build_item),
        }
    }

    /// What to assume an unmeasured item is worth on the very first frame.
    pub fn with_estimate(mut self, estimate: f32) -> Self {
        self.estimate = estimate.max(0.0);
        self
    }

    pub fn with_offset(mut self, offset: f32) -> Self {
        self.offset = offset;
        self
    }

    pub fn with_viewport(mut self, viewport: f32) -> Self {
        self.viewport = viewport;
        self
    }

    pub fn with_cache_extent(mut self, cache_extent: f32) -> Self {
        self.cache_extent = cache_extent.max(0.0);
        self
    }

    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// How tall the whole list is, as well as it is currently known.
    pub fn content_extent(&self) -> f32 {
        self.book
            .content_extent(self.count, self.estimate, self.spacing)
    }

    /// How far this list can scroll, given the viewport it is in.
    pub fn max_scroll_extent(&self) -> f32 {
        (self.content_extent() - self.viewport).max(0.0)
    }

    /// Which items this list would build right now.
    pub fn window(&self) -> Option<ItemWindow> {
        self.book.window(
            self.count,
            self.offset,
            self.viewport,
            self.cache_extent,
            self.estimate,
            self.spacing,
        )
    }
}

/// What an unmeasured item is worth before anything better is known.
///
/// A row of one line of text with the padding a list tile has. Upstream has no
/// such number because its first layout measures for real; this is the price of
/// choosing the window a step earlier, and it is paid for exactly one frame.
pub const DEFAULT_ITEM_ESTIMATE: f32 = 56.0;

impl crate::framework::Component for VariableExtentList {
    fn build(&self, _context: &mut crate::framework::BuildContext) -> crate::framework::AnyWidget {
        let Some(window) = self.window() else {
            return crate::framework::leaf(|| crate::widgets::Empty);
        };

        // The items outside the window are still accounted for: a gap at each
        // end as tall as the rows it stands in for, measured where they have
        // been and estimated where they have not. A gap short of the rows it
        // replaces, and left out entirely when there are none, because the
        // column supplies the space between its own children -- see the same
        // arithmetic in `LazyList`.
        let leading = (self
            .book
            .offset_of(window.first, self.estimate, self.spacing)
            - self.spacing)
            .max(0.0);
        let after_last = self
            .book
            .offset_of(window.last + 1, self.estimate, self.spacing);
        let trailing = (self.content_extent() - after_last).max(0.0);
        let more_below = window.last + 1 < self.count;

        let mut children = Vec::with_capacity(window.len() + 2);
        if window.first > 0 {
            children.push(crate::framework::leaf(move || {
                crate::render::RenderConstrainedBox::tight(0.0, leading)
            }));
        }
        for index in window.first..=window.last {
            let book = self.book.clone();
            // Keyed by index, as the fixed-extent list is and for the same
            // reason: an item keeps its element, and therefore its state, when
            // the window moves past it.
            children.push(crate::framework::keyed_single(
                index as u64,
                // In a layer of its own, as in `LazyList` and upstream.
                crate::widgets::repaint_boundary((self.build_item)(index)),
                move |child| RenderMeasuredItem::new(index, book.clone(), child),
            ));
        }
        if more_below {
            children.push(crate::framework::leaf(move || {
                crate::render::RenderConstrainedBox::tight(0.0, trailing)
            }));
        }

        let spacing = self.spacing;
        crate::framework::many(children, move |rendered| {
            let mut column = crate::render::RenderFlex::column()
                .with_main_axis_size(crate::render::MainAxisSize::Min)
                .with_cross_axis_alignment(crate::render::CrossAxisAlignment::Stretch)
                .with_spacing(spacing);
            for child in rendered {
                column = column.push(child);
            }
            // The column itself, not a box round it: a second box between the
            // handle and the object puts the object one layer away from the
            // description it is asked to compare itself against, and it would
            // quietly decline every rebuild. See `RenderBox for Box<R>`.
            column
        })
    }
}

/// Reports how tall its child turned out, into the book its list reads.
///
/// Draws nothing and changes no layout: it is the same box as its child, with
/// one number written down on the way out. Upstream needs no equivalent because
/// the sliver that laid the child out is the thing that wanted the number.
pub struct RenderMeasuredItem {
    index: usize,
    book: ExtentBook,
    child: crate::render::BoxedRender,
    size: crate::render::Size,
}

impl RenderMeasuredItem {
    pub fn new(
        index: usize,
        book: ExtentBook,
        child: impl crate::render::RenderBox + 'static,
    ) -> RenderMeasuredItem {
        RenderMeasuredItem {
            index,
            book,
            child: crate::render::BoxedRender::new(child),
            size: crate::render::Size::ZERO,
        }
    }
}

impl crate::render::RenderBox for RenderMeasuredItem {
    fn layout(&mut self, constraints: crate::render::BoxConstraints) -> crate::render::Size {
        self.size = self.child.layout(constraints);
        self.book.record(self.index, self.size.height);
        self.size
    }

    fn size(&self) -> crate::render::Size {
        self.size
    }

    fn paint(&self, context: &mut crate::render::PaintContext, offset: crate::render::Offset) {
        context.paint_child(&self.child, offset);
    }

    fn hit_test(
        &self,
        position: crate::render::Offset,
        result: &mut crate::render::HitTestResult,
    ) -> bool {
        self.child.hit_test(position, result)
    }

    fn min_intrinsic_width(&self, height: f32) -> f32 {
        self.child.min_intrinsic_width(height)
    }

    fn max_intrinsic_width(&self, height: f32) -> f32 {
        self.child.max_intrinsic_width(height)
    }

    fn min_intrinsic_height(&self, width: f32) -> f32 {
        self.child.min_intrinsic_height(width)
    }

    fn max_intrinsic_height(&self, width: f32) -> f32 {
        self.child.max_intrinsic_height(width)
    }

    fn distance_to_baseline(&self) -> Option<f32> {
        self.child.distance_to_baseline()
    }
}

/// Upstream `ShrinkWrappingViewport`: a viewport that takes only the space its
/// slivers need.
///
/// The difference from an ordinary `Viewport` is one line of intent and a
/// great deal of consequence. An ordinary viewport takes **all** the space
/// offered on the main axis and scrolls its content through it; this one takes
/// as much as its content asked for, up to that maximum.
///
/// The consequence is that its slivers must be laid out before its own size is
/// known, so it cannot be given unbounded constraints -- there would be no
/// maximum to stop at -- and it cannot lay out lazily, because "how big is the
/// content" is exactly the question laziness declines to answer. A shrink-wrap
/// list builds every child. That is why upstream's `ListView` warns against
/// `shrinkWrap: true` on a long list: it is not a hint, it is a promise to
/// build all of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShrinkWrappingViewport {
    pub axis_direction: crate::render::AxisDirection,
    pub cross_axis_direction: Option<crate::render::AxisDirection>,
    pub cache_extent: Option<f32>,
}

impl Default for ShrinkWrappingViewport {
    fn default() -> ShrinkWrappingViewport {
        ShrinkWrappingViewport::new(crate::render::AxisDirection::Down)
    }
}

impl ShrinkWrappingViewport {
    pub fn new(axis_direction: crate::render::AxisDirection) -> ShrinkWrappingViewport {
        ShrinkWrappingViewport {
            axis_direction,
            cross_axis_direction: None,
            cache_extent: None,
        }
    }

    /// The size the viewport takes on its main axis: what the content wanted,
    /// clamped to what was offered.
    pub fn main_axis_extent(&self, content_extent: f32, max_extent: f32) -> f32 {
        content_extent.min(max_extent).max(0.0)
    }

    /// Whether this viewport can be laid out at all under the given
    /// constraint. An unbounded main axis has no maximum to shrink-wrap
    /// against.
    pub fn accepts_unbounded_main_axis(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Reveals: what a viewport asks its scroll for ------------------------

    #[test]
    fn a_link_carries_a_reveal_once() {
        let link = ScrollLink::default();
        assert_eq!(link.take_reveal(), None, "nothing asked for yet");
        link.request_reveal(120.0, crate::render::Reveal::NOW);
        assert_eq!(
            link.take_reveal(),
            Some((120.0, crate::render::Reveal::NOW))
        );
        assert_eq!(link.take_reveal(), None, "and it is spent");
    }

    #[test]
    fn the_last_reveal_in_a_frame_is_the_one_that_happens() {
        // Two asks before anything consumed either: the second knows whatever
        // the first did. Upstream's `offset.moveTo` replaces the running
        // activity rather than queueing behind it.
        let link = ScrollLink::default();
        link.request_reveal(120.0, crate::render::Reveal::NOW);
        link.request_reveal(300.0, crate::render::Reveal::NOW);
        assert_eq!(link.take_reveal().map(|(offset, _)| offset), Some(300.0));
    }

    #[test]
    fn a_scroll_takes_a_reveal_on_the_next_frame() {
        let mut scroll = Scroll::new();
        scroll.set_extent(500.0, 200.0);
        scroll
            .link()
            .request_reveal(120.0, crate::render::Reveal::NOW);

        let moving = scroll.advance(0);
        assert_eq!(scroll.offset, 120.0, "the offset is where the reveal asked");
        assert!(
            moving,
            "and the frame loop is told something moved -- a reveal with no              duration leaves no activity behind, and answering `false` would              end the frame that was going to draw it"
        );
    }

    #[test]
    fn a_scroll_standing_still_still_takes_a_reveal() {
        // The early return for "no activity" is below the reveal, not above
        // it: a still scroll is exactly the case a reveal starts from.
        let mut scroll = Scroll::new();
        scroll.set_extent(500.0, 200.0);
        assert!(!scroll.advance(0), "nothing is moving to begin with");
        scroll
            .link()
            .request_reveal(80.0, crate::render::Reveal::NOW);
        assert!(scroll.advance(16_000));
        assert_eq!(scroll.offset, 80.0);
    }

    #[test]
    fn an_animated_reveal_travels_rather_than_jumping() {
        let mut scroll = Scroll::new();
        scroll.set_extent(500.0, 200.0);
        scroll.link().request_reveal(
            200.0,
            crate::render::Reveal::animated(100_000, crate::animation::Curve::FAST_OUT_SLOW_IN),
        );
        scroll.advance(0);
        assert!(
            scroll.offset < 200.0,
            "still on its way at the first frame, not already there"
        );
        // Well past the hundred milliseconds it was given.
        scroll.advance(500_000);
        assert_eq!(scroll.offset, 200.0);
    }

    #[test]
    fn a_reveal_out_of_range_waits_to_be_corrected_like_any_other_jump() {
        // The clamping belongs to the viewport, which knows what it measured
        // and does it before asking -- see `RenderViewport::show_in_viewport`.
        // A `Scroll` handed a number past the end keeps it, exactly as
        // [`Scroll::jump_to`] does and for the reason upstream's `forcePixels`
        // does: the correction is the next layout, not the assignment.
        let mut scroll = Scroll::new();
        scroll.set_extent(50.0, 200.0);
        scroll
            .link()
            .request_reveal(400.0, crate::render::Reveal::NOW);
        scroll.advance(0);
        assert_eq!(scroll.offset, 400.0, "stored as given");
        scroll.set_extent(50.0, 200.0);
        assert_eq!(scroll.offset, 50.0, "and corrected when it is measured");
    }

    #[test]
    fn a_shrink_wrapping_viewport_takes_what_its_content_asked_for() {
        let viewport = ShrinkWrappingViewport::default();
        assert_eq!(viewport.main_axis_extent(120.0, 400.0), 120.0);
    }

    #[test]
    fn but_never_more_than_it_was_offered() {
        // Which is what still makes it a viewport rather than a column.
        let viewport = ShrinkWrappingViewport::default();
        assert_eq!(viewport.main_axis_extent(900.0, 400.0), 400.0);
        assert_eq!(viewport.main_axis_extent(-5.0, 400.0), 0.0);
    }

    #[test]
    fn there_is_no_maximum_to_shrink_wrap_against_on_an_unbounded_axis() {
        // And no laziness either: "how big is the content" is exactly the
        // question laziness declines to answer, so a shrink-wrap list builds
        // every child.
        assert!(!ShrinkWrappingViewport::default().accepts_unbounded_main_axis());
    }

    use super::*;

    /// A scroll with room to move, for the tests below. The viewport is the
    /// conventional 500 logical pixels; the physics below never reads it.
    fn scroll(extent: f32) -> Scroll {
        let mut scroll = Scroll::new();
        scroll.set_extent(extent, 500.0);
        scroll
    }

    /// Runs frames at 60Hz until nothing is moving, and returns how many.
    fn settle(scroll: &mut Scroll) -> u32 {
        let mut frames = 0;
        let mut now = 1_000_000;
        while scroll.advance(now) {
            now += 16_667;
            frames += 1;
            assert!(frames < 600, "a fling should not last ten seconds");
        }
        frames
    }

    #[test]
    fn the_metrics_derive_what_upstream_derives_from_them() {
        // extentBefore/Inside/After and outOfRange/atEdge, as
        // scroll_metrics.dart defines them: before is what is above, inside is
        // the viewport less any overscroll at either end, after is what is
        // below, and the edges are exact.
        let metrics = ScrollMetrics {
            pixels: 100.0,
            min_scroll_extent: 0.0,
            max_scroll_extent: 400.0,
            viewport_dimension: 250.0,
        };
        assert_eq!(metrics.extent_before(), 100.0);
        assert_eq!(metrics.extent_inside(), 250.0);
        assert_eq!(metrics.extent_after(), 300.0);
        assert!(!metrics.out_of_range());
        assert!(!metrics.at_edge());

        // At the end, exactly: at edge, and nothing after.
        let bottom = ScrollMetrics {
            pixels: 400.0,
            ..metrics
        };
        assert!(bottom.at_edge());
        assert_eq!(bottom.extent_after(), 0.0);

        // Past it: out of range, the inside shrinks by the overscroll, and
        // the after stays at zero rather than going negative.
        let past = ScrollMetrics {
            pixels: 450.0,
            ..metrics
        };
        assert!(past.out_of_range());
        assert_eq!(past.extent_inside(), 200.0);
        assert_eq!(past.extent_after(), 0.0);
        assert_eq!(past.extent_before(), 450.0);

        // Less content than viewport: the inside is the whole viewport, empty
        // space and all.
        let fits = ScrollMetrics {
            pixels: 0.0,
            min_scroll_extent: 0.0,
            max_scroll_extent: 0.0,
            viewport_dimension: 500.0,
        };
        assert_eq!(fits.extent_inside(), 500.0);
        assert!(fits.at_edge());
    }

    #[test]
    fn dragging_moves_by_the_delta_and_stops_at_the_ends() {
        let mut scroll = scroll(500.0);
        scroll.scroll_by(120.0);
        assert_eq!(scroll.offset, 120.0);
        scroll.scroll_by(-400.0);
        assert_eq!(scroll.offset, 0.0);
        scroll.scroll_by(9000.0);
        assert_eq!(scroll.offset, 500.0);
    }

    #[test]
    fn a_fling_keeps_going_after_the_finger_stops() {
        let mut scroll = scroll(5000.0);
        scroll.fling(2000.0);
        assert!(scroll.is_ballistic());

        // The first frame only starts the clock, exactly as a ticker does.
        assert!(scroll.advance(1_000_000));
        assert_eq!(scroll.offset, 0.0);

        assert!(scroll.advance(1_100_000));
        let after_a_tenth = scroll.offset;
        assert!(
            after_a_tenth > 100.0,
            "a tenth of a second in: {after_a_tenth}"
        );

        settle(&mut scroll);
        assert!(!scroll.is_ballistic());
        // The whole 2000 px/s fling, which the simulation puts at ~647px.
        assert!(
            (scroll.offset - 647.0).abs() < 10.0,
            "should have travelled the simulation's distance, not {}",
            scroll.offset
        );
    }

    #[test]
    fn a_fling_takes_more_than_a_frame_or_two() {
        // The bug this file was written for: a swipe that moved the content
        // and then stopped dead. Whatever else changes, a fling has to be
        // something a person can watch.
        let mut scroll = scroll(5000.0);
        scroll.fling(2000.0);
        let frames = settle(&mut scroll);
        assert!(frames > 30, "a fling should last some frames, not {frames}");
    }

    #[test]
    fn a_fling_stops_at_the_end_of_the_content() {
        let mut scroll = scroll(200.0);
        scroll.fling(4000.0);
        settle(&mut scroll);
        assert_eq!(scroll.offset, 200.0);
        assert!(
            !scroll.is_ballistic(),
            "and does not keep asking for frames"
        );
    }

    #[test]
    fn a_fling_from_the_end_does_not_start() {
        let mut scroll = scroll(200.0);
        scroll.jump_to(200.0);
        scroll.fling(3000.0);
        assert!(!scroll.is_ballistic());

        // But back the other way it does.
        scroll.fling(-3000.0);
        assert!(scroll.is_ballistic());
    }

    #[test]
    fn touching_the_content_stops_the_fling() {
        let mut scroll = scroll(5000.0);
        scroll.fling(3000.0);
        scroll.advance(1_000_000);
        scroll.advance(1_050_000);
        let caught = scroll.offset;
        assert!(caught > 0.0);

        scroll.stop();
        assert!(!scroll.is_ballistic());
        assert!(
            !scroll.advance(1_100_000),
            "a stopped fling asks for nothing"
        );
        assert_eq!(scroll.offset, caught, "and leaves the offset where it was");
    }

    #[test]
    fn dragging_during_a_fling_takes_over() {
        let mut scroll = scroll(5000.0);
        scroll.fling(3000.0);
        scroll.advance(1_000_000);
        scroll.advance(1_050_000);
        let caught = scroll.offset;

        scroll.scroll_by(-20.0);
        assert!(!scroll.is_ballistic());
        assert_eq!(scroll.offset, caught - 20.0);
    }

    #[test]
    fn a_fling_with_no_velocity_does_nothing() {
        let mut scroll = scroll(500.0);
        scroll.fling(0.0);
        assert!(!scroll.is_ballistic());
        assert!(!scroll.advance(1_000_000));
    }

    #[test]
    fn a_fling_survives_a_late_frame() {
        // Frames are on demand and the device is not always fast. A gap does
        // not break the fling; it advances by the time that actually passed.
        let mut scroll = scroll(5000.0);
        scroll.fling(2000.0);
        scroll.advance(1_000_000);
        assert!(scroll.advance(1_300_000));
        let late = scroll.offset;

        let mut steady = self::scroll(5000.0);
        steady.fling(2000.0);
        steady.advance(1_000_000);
        for step in 1..=18 {
            steady.advance(1_000_000 + step * 16_667);
        }
        assert!(
            (late - steady.offset).abs() < 20.0,
            "a late frame should land where the steady ones did: {late} against {}",
            steady.offset
        );
    }

    // -- Programmatic scrolling ------------------------------------------------

    #[test]
    fn animate_to_travels_to_the_target() {
        let mut scroll = scroll(5000.0);
        scroll.animate_to(400.0, 300_000, Curve::EASE);

        // The first frame only starts the clock, exactly as a fling's does.
        assert!(scroll.advance(1_000_000));
        assert_eq!(scroll.offset, 0.0);

        // From there, monotonically to the target: an ease never backs up and
        // never overshoots.
        let mut last = 0.0;
        let mut now = 1_000_000;
        while scroll.advance(now) {
            now += 16_667;
            assert!(scroll.offset >= last, "backed up at {}", scroll.offset);
            assert!(scroll.offset <= 400.0, "overshot at {}", scroll.offset);
            last = scroll.offset;
        }
        assert_eq!(scroll.offset, 400.0, "arrives exactly, on the last frame");
    }

    #[test]
    fn animate_to_stops_at_the_end_of_the_content() {
        // The target is not clamped when the animation starts -- upstream's
        // animateTo does not clamp it either -- but the animation stops the
        // frame the content runs out.
        let mut scroll = scroll(300.0);
        scroll.animate_to(10_000.0, 300_000, Curve::EASE);
        settle(&mut scroll);
        assert_eq!(scroll.offset, 300.0);
        assert!(
            !scroll.is_ballistic(),
            "and does not keep asking for frames"
        );
    }

    #[test]
    fn a_drag_interrupts_an_animate_to() {
        let mut scroll = scroll(5000.0);
        scroll.animate_to(1000.0, 300_000, Curve::EASE);
        scroll.advance(1_000_000);
        scroll.advance(1_050_000);
        let caught = scroll.offset;
        assert!(caught > 0.0, "the animation had got going: {caught}");

        // The reader's finger. Upstream the drag activity replaces the driven
        // one; here that is one field.
        scroll.scroll_by(-20.0);
        assert!(!scroll.is_ballistic());
        assert!(
            !scroll.advance(1_100_000),
            "an interrupted animation asks for nothing"
        );
        assert_eq!(
            scroll.offset,
            caught - 20.0,
            "and stays where the finger left it"
        );
    }

    #[test]
    fn touching_the_content_stops_an_animate_to() {
        let mut scroll = scroll(5000.0);
        scroll.animate_to(1000.0, 300_000, Curve::EASE);
        scroll.advance(1_000_000);
        scroll.advance(1_050_000);
        let caught = scroll.offset;

        scroll.stop();
        assert!(!scroll.advance(1_100_000));
        assert_eq!(scroll.offset, caught, "and leaves the offset where it was");
    }

    #[test]
    fn animate_to_and_a_fling_replace_each_other() {
        // Both are activities; whichever started last is in charge.
        let mut scroll = scroll(5000.0);
        scroll.fling(2000.0);
        scroll.animate_to(100.0, 200_000, Curve::EASE);
        settle(&mut scroll);
        assert_eq!(
            scroll.offset, 100.0,
            "the animation won, wherever the fling was going"
        );

        scroll.fling(2000.0);
        assert!(scroll.is_ballistic());
        settle(&mut scroll);
        // The fling's own distance, from where the animation left it.
        assert!(
            (scroll.offset - 747.0).abs() < 10.0,
            "should have travelled the simulation's distance, not {}",
            scroll.offset
        );
    }

    #[test]
    fn animate_to_where_it_already_is_jumps() {
        // Upstream skips the animation when nearEqual(to, pixels, tolerance)
        // and goes straight to the position -- and the tolerance is the scroll
        // one, a logical pixel, not the simulation machinery's thousandth.
        let mut scroll = scroll(5000.0);
        scroll.jump_to(250.0);
        scroll.animate_to(250.6, 300_000, Curve::EASE);
        assert!(
            !scroll.is_ballistic(),
            "less than a pixel away is nothing to animate"
        );
        assert_eq!(scroll.offset, 250.6);

        // A pixel and a half away is: there is ground to cover.
        scroll.animate_to(252.1, 300_000, Curve::EASE);
        assert!(scroll.is_ballistic());
    }

    #[test]
    fn animate_to_with_no_duration_jumps() {
        // What upstream's moveTo does with Duration.zero.
        let mut scroll = scroll(5000.0);
        scroll.animate_to(250.0, 0, Curve::EASE);
        assert_eq!(scroll.offset, 250.0);
        assert!(!scroll.advance(1_000_000), "nothing is in flight");
    }

    #[test]
    fn jump_to_takes_effect_immediately() {
        let mut scroll = scroll(5000.0);
        scroll.animate_to(1000.0, 300_000, Curve::EASE);
        scroll.advance(1_000_000);
        scroll.advance(1_050_000);
        assert!(
            scroll.offset > 0.0 && scroll.offset < 1000.0,
            "part way there"
        );

        // No frame needed to see it, and it cancels the animation.
        scroll.jump_to(300.0);
        assert_eq!(scroll.offset, 300.0);
        assert!(!scroll.is_ballistic());
        assert!(!scroll.advance(1_100_000), "and asks for no more frames");

        // Out of range both ways: stored exactly as given, as forcePixels
        // stores it, for the next dimensions to correct.
        scroll.jump_to(-50.0);
        assert_eq!(scroll.offset, -50.0);
        scroll.jump_to(90_000.0);
        assert_eq!(scroll.offset, 90_000.0);
        scroll.set_extent(5000.0, 500.0);
        assert_eq!(scroll.offset, 5000.0, "the correction the dimensions make");
    }

    #[test]
    fn a_jump_before_measurement_survives_it() {
        // The bug this is not: a jump clamped against an extent nobody had
        // measured yet, and so lost. forcePixels stores what it is given.
        let mut scroll = Scroll::new();
        scroll.jump_to(300.0);
        assert_eq!(scroll.offset, 300.0);
        scroll.set_extent(5000.0, 500.0);
        assert_eq!(
            scroll.offset, 300.0,
            "in range, so the correction corrects nothing"
        );
    }

    #[test]
    fn an_out_of_range_jump_reports_where_it_was_put() {
        // jumpTo's notifications carry the raw target -- the metrics are a
        // copyWith of a position whose pixels really are past the end -- and
        // no overscroll, because none of the move was applied against bounds.
        let (_tree, handles, log) = listened_scroll(5000.0);
        let handle = handles.borrow().clone().expect("built");

        handle.set_state(|state| state.scroll.jump_to(9000.0));
        assert_eq!(labels(&log.borrow()), vec!["start", "update", "end"]);
        match log
            .borrow()
            .iter()
            .find(|n| matches!(n, ScrollNotification::Update { .. }))
        {
            Some(ScrollNotification::Update {
                metrics,
                scroll_delta,
                ..
            }) => {
                assert_eq!(
                    metrics.pixels, 9000.0,
                    "the notification carries the raw target"
                );
                assert_eq!(*scroll_delta, 9000.0);
                assert!(
                    metrics.out_of_range(),
                    "and says so, as the metrics would upstream"
                );
            }
            _ => panic!("the jump reported its move"),
        }
        assert!(
            !log.borrow()
                .iter()
                .any(|n| matches!(n, ScrollNotification::Overscroll { .. })),
            "a jump dispatches no overscroll"
        );
    }

    #[test]
    fn a_fling_below_the_tolerance_velocity_is_only_a_release() {
        // Upstream's createBallisticSimulation returns null below the
        // tolerance velocity -- 20 logical px/s here -- so the release ends
        // whatever was underway and starts nothing.
        let (tree, handles, log) = listened_scroll(5000.0);
        let handle = handles.borrow().clone().expect("built");

        handle.set_state(|state| state.scroll.fling(19.9));
        assert_eq!(
            tree.state::<ScrollerState, _>(handle.element(), |s| s.scroll.is_ballistic()),
            Some(false),
            "a dead throw is not a fling"
        );
        assert!(
            log.borrow().is_empty(),
            "nothing started, so nothing was reported"
        );

        // The tolerance itself still throws.
        handle.set_state(|state| state.scroll.fling(20.0));
        assert_eq!(labels(&log.borrow()), vec!["start"]);
    }

    // -- Scroll notifications -----------------------------------------------

    use crate::framework::{
        AnyWidget, BuildContext, ElementTree, StateHandle, StatefulComponent,
        notification_listener, stateful,
    };

    /// What kind each dispatched notification was, in the order they arrived.
    fn labels(log: &[ScrollNotification]) -> Vec<&'static str> {
        log.iter()
            .map(|notification| match notification {
                ScrollNotification::Start { .. } => "start",
                ScrollNotification::Update { .. } => "update",
                ScrollNotification::Overscroll { .. } => "overscroll",
                ScrollNotification::End { .. } => "end",
                ScrollNotification::UserScroll { .. } => "user",
            })
            .collect()
    }

    /// A scroll, in a widget, with somewhere for its notifications to go.
    #[derive(Default)]
    struct ScrollerState {
        scroll: Scroll,
    }

    struct Scroller {
        handles: Rc<RefCell<Option<StateHandle<ScrollerState>>>>,
        extent: f32,
    }

    impl StatefulComponent for Scroller {
        type State = ScrollerState;

        fn initial_state(&self) -> ScrollerState {
            let mut state = ScrollerState::default();
            state.scroll.set_extent(self.extent, 500.0);
            state
        }

        fn advance(&self, state: &mut ScrollerState, frame_time_micros: i64) -> bool {
            state.scroll.advance(frame_time_micros)
        }

        fn build(
            &self,
            state: &ScrollerState,
            handle: StateHandle<ScrollerState>,
            context: &mut BuildContext,
        ) -> AnyWidget {
            // The binding every scrolling screen wants: the scroll gets the
            // sink this element's build was given, once per build, and keeps
            // dispatching through it long after.
            state
                .scroll
                .set_notification_sink(context.notification_sink());
            *self.handles.borrow_mut() = Some(handle);
            crate::framework::leaf(|| crate::widgets::Empty)
        }
    }

    /// A listening tree with a scroll under it, and a way to drive the scroll.
    fn listened_scroll(
        extent: f32,
    ) -> (
        ElementTree,
        Rc<RefCell<Option<StateHandle<ScrollerState>>>>,
        Rc<RefCell<Vec<ScrollNotification>>>,
    ) {
        let handles = Rc::new(RefCell::new(None));
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut tree = ElementTree::new();
        let recorder = log.clone();
        tree.rebuild(notification_listener(
            move |notification: &ScrollNotification| {
                recorder.borrow_mut().push(*notification);
                false
            },
            stateful(Scroller {
                handles: handles.clone(),
                extent,
            }),
        ));
        (tree, handles, log)
    }

    /// Runs frames at 60Hz until the scroll goes idle, as a host would.
    fn settle_tree(tree: &mut ElementTree) {
        for step in 0..600 {
            let wants_more = tree.advance_frame(1_000_000 + step * 16_667);
            tree.rebuild_dirty();
            if !wants_more {
                return;
            }
        }
        panic!("a fling should not last ten seconds");
    }

    #[test]
    fn a_wheel_scroll_is_a_whole_scroll_from_start_to_end() {
        let (tree, handles, log) = listened_scroll(5000.0);
        let handle = handles.borrow().clone().expect("built");

        // Two notches of the wheel, each a scroll that begins and ends in one
        // event -- upstream's pointerScroll, which reports the reader's
        // direction, then a start, the update, and the end, and idles the
        // reader. The second notch begins by ending nothing, so the two runs
        // sit end to end, two idles deep.
        handle.set_state(|state| state.scroll.scroll_by(50.0));
        handle.set_state(|state| state.scroll.scroll_by(30.0));

        assert_eq!(
            labels(&log.borrow()),
            vec![
                "user", "start", "update", "end", "user", //
                "user", "start", "update", "end", "user",
            ],
            "each notch: the direction, a start, the move, the end, idle"
        );
        // The last update says where the wheel had got to and by how much.
        let updates: Vec<ScrollNotification> = log
            .borrow()
            .iter()
            .filter(|n| matches!(n, ScrollNotification::Update { .. }))
            .copied()
            .collect();
        match updates.last() {
            Some(ScrollNotification::Update {
                metrics,
                scroll_delta,
                ..
            }) => {
                assert_eq!(metrics.pixels, 80.0);
                assert_eq!(*scroll_delta, 30.0);
                assert_eq!(metrics.max_scroll_extent, 5000.0);
            }
            _ => panic!("the wheel reported its moves"),
        }
        assert_eq!(
            tree.state::<ScrollerState, _>(handle.element(), |s| s.scroll.offset),
            Some(80.0)
        );
    }

    #[test]
    fn a_fling_after_a_wheel_scroll_is_a_scroll_of_its_own() {
        // The wheel's scroll ended with it; the throw that follows is a new
        // one. Upstream's drag-to-fling handover is the seamless one --
        // scrolling to scrolling, no end between -- and this is not that: the
        // wheel was finished, so its end had already gone out.
        let (mut tree, handles, log) = listened_scroll(5000.0);
        let handle = handles.borrow().clone().expect("built");

        handle.set_state(|state| state.scroll.scroll_by(40.0));
        assert_eq!(
            labels(&log.borrow()),
            vec!["user", "start", "update", "end", "user"],
            "the notch came and went whole"
        );

        handle.set_state(|state| state.scroll.fling(2000.0));
        settle_tree(&mut tree);
        let all = labels(&log.borrow());
        assert_eq!(&all[..5], &["user", "start", "update", "end", "user"]);
        assert_eq!(all[5], "start", "the throw started a new scroll");
        assert_eq!(all.last(), Some(&"end"), "the throw ended at the settle");
        // and no idle after it: a ballistic activity does not touch the
        // reader's direction upstream -- only a drag's applyUserOffset and a
        // wheel's pointerScroll do -- so it had been idle since the wheel's
        // end, and the settle's idle update is suppressed, as
        // updateUserScrollDirection's unchanged-direction check does there.
        assert!(
            all.iter().filter(|l| **l == "update").count() > 2,
            "the fling's frames each reported a move"
        );
        assert_eq!(
            all.iter().filter(|l| **l == "end").count(),
            2,
            "each scroll ended exactly once"
        );
    }

    #[test]
    fn a_touch_catches_a_fling_and_ends_the_scroll_where_it_stands() {
        let (mut tree, handles, log) = listened_scroll(5000.0);
        let handle = handles.borrow().clone().expect("built");

        // Thrown from rest: a start, the fling's updates.
        handle.set_state(|state| state.scroll.fling(3000.0));
        tree.advance_frame(1_000_000);
        tree.advance_frame(1_050_000);
        let caught = tree.state::<ScrollerState, _>(handle.element(), |s| s.scroll.offset);
        assert!(caught.unwrap_or(0.0) > 0.0, "the fling had got going");

        // The reader's finger. The hold does not constitute scrolling, so this
        // is the end of it. No UserScroll follows: the reader's direction was
        // never anything but idle, and a direction that did not change is not
        // reported -- upstream's updateUserScrollDirection returns early too.
        handle.set_state(|state| state.scroll.stop());
        let all = labels(&log.borrow());
        assert_eq!(all.last(), Some(&"end"));
        assert!(!tree.advance_frame(1_100_000), "nothing more is in flight");
    }

    #[test]
    fn a_jump_is_a_whole_scroll_of_its_own() {
        // Upstream's jumpTo: idle, then a start, one update, an end, by hand.
        let (_tree, handles, log) = listened_scroll(5000.0);
        let handle = handles.borrow().clone().expect("built");

        handle.set_state(|state| state.scroll.jump_to(300.0));
        assert_eq!(labels(&log.borrow()), vec!["start", "update", "end"]);

        // And a jump that interrupts a wheel scroll ends it first, as goIdle
        // does, then reports itself.
        log.borrow_mut().clear();
        handle.set_state(|state| state.scroll.scroll_by(10.0));
        handle.set_state(|state| state.scroll.jump_to(400.0));
        assert_eq!(
            labels(&log.borrow()),
            vec![
                "user", "start", "update", "end", "user", //
                "start", "update", "end",
            ]
        );
    }

    #[test]
    fn what_the_content_bounds_keep_is_an_overscroll() {
        let (_tree, handles, log) = listened_scroll(500.0);
        let handle = handles.borrow().clone().expect("built");

        handle.set_state(|state| state.scroll.scroll_by(9000.0));
        let overscrolls: Vec<ScrollNotification> = log
            .borrow()
            .iter()
            .filter(|n| matches!(n, ScrollNotification::Overscroll { .. }))
            .copied()
            .collect();
        match overscrolls.last() {
            Some(ScrollNotification::Overscroll {
                metrics,
                overscroll,
                velocity,
                ..
            }) => {
                assert_eq!(metrics.pixels, 500.0, "it moved as far as it could");
                assert_eq!(*overscroll, 8500.0, "and reported the rest as overscroll");
                assert_eq!(*velocity, 0.0, "a wheel carries no velocity into the bound");
            }
            _ => panic!("the clamped-off travel should have been reported"),
        }
    }

    #[test]
    fn an_overscroll_from_a_fling_carries_the_fling_s_velocity() {
        // Upstream's ballistic activity dispatches its overscroll with the
        // velocity it was still moving at when it hit the bound.
        let (mut tree, handles, log) = listened_scroll(200.0);
        let handle = handles.borrow().clone().expect("built");

        handle.set_state(|state| state.scroll.fling(4000.0));
        settle_tree(&mut tree);
        let overscrolls: Vec<ScrollNotification> = log
            .borrow()
            .iter()
            .filter(|n| matches!(n, ScrollNotification::Overscroll { .. }))
            .copied()
            .collect();
        match overscrolls.last() {
            Some(ScrollNotification::Overscroll { velocity, .. }) => {
                assert!(
                    *velocity > 0.0,
                    "still moving into the bound when it hit: {velocity}"
                );
            }
            _ => panic!("the bound kept some of the fling"),
        }
    }

    #[test]
    fn an_animate_to_reports_like_any_scrolling_activity() {
        let (mut tree, handles, log) = listened_scroll(5000.0);
        let handle = handles.borrow().clone().expect("built");

        handle.set_state(|state| state.scroll.animate_to(400.0, 300_000, Curve::EASE));
        assert_eq!(
            labels(&log.borrow()),
            vec!["start"],
            "a driven activity is a scrolling one"
        );

        settle_tree(&mut tree);
        let all = labels(&log.borrow());
        assert_eq!(all.first(), Some(&"start"));
        assert_eq!(
            all.last(),
            Some(&"end"),
            "arrived, reported, and no direction ever changed"
        );
        assert!(all.iter().any(|l| *l == "update"));
    }

    #[test]
    fn a_lazy_list_builds_a_screenful_and_not_a_hundred_thousand() {
        let window = item_window(100_000, 50.0, 0.0, 800.0, DEFAULT_CACHE_EXTENT)
            .expect("a list with items in it");
        assert_eq!(window.first, 0);
        // 800 of viewport plus 250 of cache, in 50-pixel rows.
        assert_eq!(window.last, 20);
        assert_eq!(window.len(), 21);
    }

    #[test]
    fn scrolling_moves_the_window_rather_than_growing_it() {
        let top = item_window(1000, 50.0, 0.0, 500.0, 0.0).expect("items");
        let down = item_window(1000, 50.0, 5000.0, 500.0, 0.0).expect("items");
        assert_eq!(top.first, 0);
        assert_eq!(down.first, 100);
        assert_eq!(
            down.len(),
            top.len(),
            "the same number of rows, further down"
        );
    }

    #[test]
    fn the_cache_reaches_behind_as_well_as_ahead() {
        let plain = item_window(1000, 50.0, 5000.0, 500.0, 0.0).expect("items");
        let cached = item_window(1000, 50.0, 5000.0, 500.0, 250.0).expect("items");
        assert!(cached.first < plain.first, "nothing built above the fold");
        assert!(cached.last > plain.last, "nothing built below it either");
    }

    #[test]
    fn the_window_stops_at_the_ends_of_the_list() {
        let window = item_window(5, 50.0, 0.0, 800.0, 250.0).expect("items");
        assert_eq!(window.first, 0);
        assert_eq!(window.last, 4, "there is no item five to build");

        let past_the_end = item_window(5, 50.0, 10_000.0, 800.0, 250.0).expect("items");
        assert_eq!(past_the_end.last, 4);
        assert!(past_the_end.first <= 4);
    }

    #[test]
    fn an_empty_list_has_no_window() {
        assert!(item_window(0, 50.0, 0.0, 800.0, 250.0).is_none());
        assert!(
            item_window(10, 0.0, 0.0, 800.0, 250.0).is_none(),
            "no extent, no arithmetic"
        );
    }

    #[test]
    fn an_offset_a_hair_off_a_boundary_does_not_flicker() {
        // What upstream's precisionErrorTolerance is for: a fling leaves the
        // offset a rounding error away from an item boundary, and the window
        // must not gain and lose a row between one frame and the next.
        let exact = item_window(100, 50.0, 500.0, 500.0, 0.0).expect("items");
        let hair = item_window(100, 50.0, 500.000_01, 500.0, 0.0).expect("items");
        assert_eq!(exact, hair);
    }

    #[test]
    fn a_sliver_list_view_builds_only_the_window_it_is_asked_for() {
        use crate::framework::{ElementTree, component};
        use crate::render::{BoxConstraints, RenderBox};

        let built = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let counter = built.clone();
        let mut tree = ElementTree::new();
        tree.rebuild(component(
            SliverListView::new(1000, move |_| {
                counter.set(counter.get() + 1);
                crate::render::RenderRef::new(crate::render::RenderConstrainedBox::tight(
                    100.0, 50.0,
                ))
            })
            .with_item_extent(50.0)
            .with_offset(0.0),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        let size = root.layout(BoxConstraints::new(0.0, 300.0, 0.0, 500.0));
        // The viewport is the window it was given, not the list behind it:
        // upstream's `ListView` sizes to its viewport, and the fifty thousand
        // pixels of content live in the scroll extent, not in the box.
        assert_eq!(size.height, 500.0);
        // A 500-pixel window plus the default 250 of cache, in 50-pixel rows.
        assert!(
            built.get() <= 15,
            "a thousand rows were offered, {} were built",
            built.get()
        );
        assert!(built.get() >= 10, "the visible window was not even filled");
    }

    #[test]
    fn a_lazy_list_reserves_the_space_of_what_it_did_not_build() {
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        let mut tree = ElementTree::new();
        tree.rebuild(component(
            LazyList::new(1000, 50.0, |_| leaf(|| SizedBox::new(100.0, 50.0)))
                .with_offset(0.0)
                .with_viewport(500.0),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        let size = root.layout(BoxConstraints::new(0.0, 300.0, 0.0, f32::INFINITY));
        // Fifty thousand pixels of list, whatever was actually built.
        assert_eq!(size.height, 50_000.0);
    }

    #[test]
    fn a_spaced_list_is_as_tall_as_its_own_content() {
        // The gap between two rows belongs to the column that puts it there.
        // Counting it in the spacers as well made a list two gaps taller than
        // the sum of what is in it, which is invisible until the scrollbar is
        // asked where the bottom is.
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        let mut tree = ElementTree::new();
        tree.rebuild(component(
            LazyList::new(100, 40.0, |_| leaf(|| SizedBox::new(100.0, 40.0)))
                .with_spacing(8.0)
                .with_offset(1000.0)
                .with_viewport(400.0),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        let size = root.layout(BoxConstraints::new(0.0, 300.0, 0.0, f32::INFINITY));
        // A hundred rows of forty, with ninety-nine gaps of eight between them.
        assert_eq!(size.height, 100.0 * 40.0 + 99.0 * 8.0);
    }

    // -- Items that are not all the same height ----------------------------

    #[test]
    fn a_book_with_nothing_in_it_falls_back_to_the_guess() {
        let book = ExtentBook::new();
        assert_eq!(book.known(), 0);
        assert_eq!(book.average(72.0), 72.0);
        assert_eq!(book.content_extent(10, 72.0, 0.0), 720.0);
    }

    #[test]
    fn an_unmeasured_item_is_worth_the_average_of_the_measured_ones() {
        // Upstream's `_extrapolateMaxScrollOffset`: take what the children you
        // have are worth and spread it over the ones you do not.
        let book = ExtentBook::new();
        book.record(0, 100.0);
        book.record(1, 50.0);
        assert_eq!(book.average(999.0), 75.0, "the guess stops mattering");
        // Two measured, eight at the average.
        assert_eq!(
            book.content_extent(10, 999.0, 0.0),
            100.0 + 50.0 + 8.0 * 75.0
        );
    }

    #[test]
    fn a_measurement_that_says_the_same_thing_is_not_a_change() {
        let book = ExtentBook::new();
        book.record(3, 40.0);
        let after_first = book.revision();
        book.record(3, 40.0);
        assert_eq!(book.revision(), after_first, "nothing was learned");
        book.record(3, 41.0);
        assert!(book.revision() > after_first, "and something was");
        assert_eq!(book.known(), 1, "the same item, remeasured");
        assert_eq!(book.average(0.0), 41.0, "and the total followed it");
    }

    #[test]
    fn the_window_walks_past_the_tall_ones() {
        // The whole difference from a fixed extent: which items are on screen
        // is not arithmetic, because item zero being tall pushes item one down.
        let book = ExtentBook::new();
        book.record(0, 300.0);
        book.record(1, 20.0);
        book.record(2, 20.0);
        let window = book.window(10, 0.0, 100.0, 0.0, 50.0, 0.0).expect("items");
        assert_eq!(window.first, 0);
        assert_eq!(window.last, 0, "one very tall row fills the viewport");

        // Below the tall one, the short ones fit several to a screen.
        let window = book
            .window(10, 300.0, 100.0, 0.0, 50.0, 0.0)
            .expect("items");
        assert_eq!(window.first, 1);
        assert!(
            window.last >= 3,
            "several short rows fit where one tall one did"
        );
    }

    #[test]
    fn an_empty_variable_list_has_no_window() {
        let book = ExtentBook::new();
        assert!(book.window(0, 0.0, 800.0, 250.0, 50.0, 0.0).is_none());
    }

    #[test]
    fn a_walked_window_holds_at_least_one_item_however_far_past_the_end_it_starts() {
        // The same invariant the fixed-extent window keeps, and the walk
        // reaches it by a different road: when no item ends after the window
        // starts, `first` falls back to the last index while `last` is still
        // sitting at zero. Unclamped that is a window running backwards, and
        // `len` is `last + 1 - first` over usize -- so the arrangement does not
        // merely read oddly, it underflows.
        let book = ExtentBook::new();
        for (offset, what) in [
            (10_000.0, "scrolled far past the end"),
            (500.0, "scrolled to exactly the end"),
            (0.0, "at the top with no viewport"),
        ] {
            let window = book
                .window(10, offset, 0.0, 0.0, 50.0, 0.0)
                .expect("a list with items has a window");
            assert!(window.first <= window.last, "{what}");
            assert!(window.len() >= 1, "{what}");
            assert!(!window.is_empty(), "{what}");
        }
    }

    #[test]
    fn a_lazy_lists_rows_say_which_one_they_are() {
        // Upstream's `SliverChildBuilderDelegate` wraps every child in an
        // `IndexedSemantics` by default (`addSemanticIndexes`), beside the
        // repaint boundary it is better known for. This port had the whole
        // chain below that line -- `RenderIndexedSemanticsBox`, the walk's
        // `pending_index`, `RfSemanticsNode::scroll_index` crossing to the
        // engine -- and **nobody putting an index on a row**, so a reader met
        // forty indistinguishable rows with no sense of where in the list they
        // were.
        //
        // The position cannot be recovered further down: by the time the walk
        // runs, the rows scrolled off the top are gone, so counting the ones
        // that survived would number the first *visible* row zero.
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox, Size};

        // **Scrolled**, which is the whole point: at the top the row's place
        // in the list and its place among the rows that survived are the same
        // number, so a list examined only at rest cannot tell an absolute
        // index from a relative one.
        crate::semantics::set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(component(
            LazyList::new(40, 50.0, |index| {
                component(crate::components::Label::new(format!("Row {index}")))
            })
            .with_offset(500.0),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        RenderBox::layout(&mut root, BoxConstraints::tight(200.0, 150.0));
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(Size::new(200.0, 150.0), &root).unwrap_or_default();
        crate::semantics::set_enabled(false);

        let numbered: Vec<(String, Option<i32>)> = nodes
            .iter()
            .filter(|node| node.properties.label.starts_with("Row "))
            .map(|node| (node.properties.label.clone(), node.index_in_parent))
            .collect();
        assert!(!numbered.is_empty(), "no rows reached the walk");
        // **And nothing above them says it is a list yet.** No node in this
        // tree carries `scroll_index` or `scroll_child_count`, because
        // `RenderSliverViewport` -- which is what a `LazyList` scrolls in --
        // has no `describe_semantics` at all; only the box `RenderViewport`
        // does. So the numbers these rows now carry have no scrollable node to
        // be gathered onto, and the platform still hears no "3 of 40".
        //
        // Asserted rather than left implied, so the day that annotation lands
        // this test says which half moved.
        assert!(
            nodes
                .iter()
                .all(|node| node.properties.scroll_index.is_none()
                    && node.properties.scroll_child_count.is_none()),
            "a sliver viewport started describing itself -- see the note above"
        );
        for (label, index) in &numbered {
            let expected: i32 = label.trim_start_matches("Row ").parse().expect("a number");
            assert_eq!(
                *index,
                Some(expected),
                "{label} did not say which row it is: {numbered:?}"
            );
        }
    }

    #[test]
    fn a_list_that_scrolled_redraws_none_of_the_rows_it_kept() {
        // What the repaint boundary around every row is for, and it could not
        // be shown until a rebuilt element stopped remaking its render object:
        // scrolling rebuilds the whole list, so before that every row was a new
        // object with nothing drawn yet, and a boundary over a new object has
        // nothing to hand back. Upstream's `SliverChildBuilderDelegate` puts one
        // around every item by default for exactly this frame.
        use crate::engine::LayerTree;
        use crate::engine_test_stubs::{layer_calls, reset_layer_calls};
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, Offset, PaintContext, RenderBox, Size};
        use crate::widgets::SizedBox;

        let list = |offset: f32| {
            component(
                LazyList::new(100, 50.0, |_| leaf(|| SizedBox::new(100.0, 50.0)))
                    .with_offset(offset)
                    .with_viewport(200.0)
                    .with_cache_extent(0.0),
            )
        };

        let frame = |tree: &mut ElementTree| {
            let mut root = tree.build_render_tree().expect("a mounted root");
            root.layout(BoxConstraints::new(0.0, 100.0, 0.0, 200.0));
            reset_layer_calls();
            let mut layers = LayerTree::new(100, 200);
            {
                let mut context = PaintContext::new(&mut layers, Size::new(100.0, 200.0));
                root.paint(&mut context, Offset::ZERO);
            }
            layer_calls()
        };

        let mut tree = ElementTree::new();
        tree.rebuild(list(0.0));
        let first = frame(&mut tree);
        assert_eq!(
            first.retainable, 4,
            "four rows fit, and all four had to be drawn"
        );
        assert_eq!(first.retained, 0, "nothing had been drawn before this");

        // One row further down. Every row still on screen is the same row, in
        // the same place relative to the list, holding the same drawing; only
        // the list moves under the window.
        tree.rebuild(list(50.0));
        let second = frame(&mut tree);
        assert_eq!(
            second.retained, 3,
            "the three rows that stayed did not keep their drawing"
        );
        assert_eq!(
            second.retainable, 1,
            "more than the newly revealed row was drawn"
        );
    }

    #[test]
    fn a_variable_list_measures_what_it_built() {
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        // Every third row is twice as tall.
        let height = |index: usize| if index % 3 == 0 { 80.0 } else { 40.0 };
        let book = ExtentBook::new();
        let mut tree = ElementTree::new();
        tree.rebuild(component(
            VariableExtentList::new(60, book.clone(), move |index| {
                leaf(move || SizedBox::new(100.0, height(index)))
            })
            .with_estimate(50.0)
            .with_offset(0.0)
            .with_viewport(200.0)
            .with_cache_extent(0.0),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        root.layout(BoxConstraints::new(0.0, 300.0, 0.0, f32::INFINITY));

        assert!(book.known() > 0, "nothing was measured");
        assert!(
            book.known() < 60,
            "everything was built, so it was not lazy"
        );
        assert_eq!(book.measured(0), Some(80.0));
        assert_eq!(book.measured(1), Some(40.0));
    }

    #[test]
    fn a_variable_list_learns_the_whole_list_by_being_scrolled_through_it() {
        // The list starts with one guess for every row and ends holding the
        // measurements. Nothing corrects it inside a single frame -- upstream
        // corrects mid-layout with `scrollOffsetCorrection`, and this cannot,
        // because the window is chosen before anything is measured. What it
        // does instead is be right by the next frame, which is the lag this
        // framework already has for `Scroll::extent`.
        //
        // The estimate is not monotonic on the way there, and neither is
        // upstream's: a sample that happens to be all short rows says the list
        // is shorter than it is, and the next tall row it meets corrects it
        // upwards. What is monotonic is what has been measured.
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        let height = |index: usize| if index % 3 == 0 { 80.0 } else { 40.0 };
        let truth: f32 = (0..60).map(height).sum();

        let book = ExtentBook::new();
        let list = |offset: f32, book: ExtentBook| {
            component(
                VariableExtentList::new(60, book, move |index| {
                    leaf(move || SizedBox::new(100.0, height(index)))
                })
                .with_estimate(50.0)
                .with_offset(offset)
                .with_viewport(200.0)
                .with_cache_extent(0.0),
            )
        };

        let mut tree = ElementTree::new();
        let mut known = 0;
        let mut offset = 0.0;
        for _ in 0..40 {
            tree.rebuild(list(offset, book.clone()));
            let mut root = tree.build_render_tree().expect("a mounted root");
            root.layout(BoxConstraints::new(0.0, 300.0, 0.0, f32::INFINITY));
            assert!(book.known() >= known, "a measurement was forgotten");
            known = book.known();
            // Follow the list down as it is currently understood, which is what
            // a reader dragging the scrollbar to the bottom would do.
            offset = (offset + 120.0).min(book.content_extent(60, 50.0, 0.0) - 200.0);
        }

        assert_eq!(book.known(), 60, "the whole list was scrolled past");
        assert_eq!(
            book.content_extent(60, 50.0, 0.0),
            truth,
            "with nothing left to estimate the answer is not an estimate"
        );
    }

    #[test]
    fn a_variable_list_reserves_the_space_of_what_it_did_not_build() {
        use crate::framework::{ElementTree, component, leaf};
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        // All the same height, so the answer is one nobody has to estimate.
        let book = ExtentBook::new();
        let mut tree = ElementTree::new();
        tree.rebuild(component(
            VariableExtentList::new(500, book.clone(), |_| leaf(|| SizedBox::new(100.0, 40.0)))
                .with_estimate(40.0)
                .with_offset(0.0)
                .with_viewport(400.0),
        ));
        let mut root = tree.build_render_tree().expect("a mounted root");
        let size = root.layout(BoxConstraints::new(0.0, 300.0, 0.0, f32::INFINITY));
        assert_eq!(size.height, 500.0 * 40.0);
    }

    #[test]
    fn an_item_keeps_its_state_when_the_window_moves() {
        use crate::framework::{ElementTree, component, leaf};
        use crate::widgets::SizedBox;

        let build = |offset: f32| {
            component(
                LazyList::new(100, 50.0, |_| leaf(|| SizedBox::new(10.0, 50.0)))
                    .with_offset(offset)
                    .with_viewport(200.0)
                    .with_cache_extent(0.0),
            )
        };
        let mut tree = ElementTree::new();
        // Below the first row, so that both windows have a spacer standing in
        // for what is above them -- at the very top there is nothing to stand
        // in for and the list does not build one.
        tree.rebuild(build(50.0));
        let before = tree.len();

        // One row further down: three of the four rows are the same rows.
        tree.rebuild(build(100.0));
        assert_eq!(
            tree.len(),
            before,
            "the window is the same size, so is the tree"
        );
    }
}

#[cfg(test)]
mod cache_extent_tests {
    use super::{CacheExtentStyle, DEFAULT_CACHE_EXTENT, ScrollCacheExtent};

    #[test]
    fn pixels_ignore_the_viewport_and_a_multiplier_does_not() {
        let fixed = ScrollCacheExtent::pixels(250.0);
        // The same answer on a phone and on a desktop window: that is what
        // "pixel" means, not a special case of the other.
        assert_eq!(fixed.in_pixels(600.0), 250.0);
        assert_eq!(fixed.in_pixels(2000.0), 250.0);

        let relative = ScrollCacheExtent::viewport(0.5);
        assert_eq!(relative.in_pixels(600.0), 300.0);
        assert_eq!(relative.in_pixels(2000.0), 1000.0);
        assert_ne!(relative.in_pixels(600.0), relative.in_pixels(2000.0));
    }

    #[test]
    fn and_the_same_number_means_two_different_things() {
        // Which is why the value and the unit travel together. One is a
        // screenful; the other is 250 screenfuls.
        let extent = 250.0;
        let as_pixels = ScrollCacheExtent::pixels(extent).in_pixels(800.0);
        let as_viewports = ScrollCacheExtent::viewport(extent).in_pixels(800.0);
        assert_eq!(as_pixels, 250.0);
        assert_eq!(as_viewports, 200_000.0);
        assert_ne!(as_pixels, as_viewports);
    }

    #[test]
    fn a_viewport_multiple_must_be_given_and_a_pixel_count_need_not() {
        // Upstream: assert(cacheExtent != null || style == pixel).
        assert!(ScrollCacheExtent::is_legal(None, CacheExtentStyle::Pixel));
        assert!(!ScrollCacheExtent::is_legal(
            None,
            CacheExtentStyle::Viewport
        ));
        for style in [CacheExtentStyle::Pixel, CacheExtentStyle::Viewport] {
            assert!(ScrollCacheExtent::is_legal(Some(1.0), style), "{style:?}");
        }
    }

    #[test]
    fn and_the_default_only_exists_in_pixels() {
        assert_eq!(
            ScrollCacheExtent::defaulted(None, CacheExtentStyle::Pixel),
            Some(ScrollCacheExtent::pixels(DEFAULT_CACHE_EXTENT))
        );
        assert_eq!(
            ScrollCacheExtent::defaulted(None, CacheExtentStyle::Viewport),
            None,
            "250 viewports of cache is not a default anybody wrote down"
        );
        // A value given is kept whichever unit it is in.
        assert_eq!(
            ScrollCacheExtent::defaulted(Some(2.0), CacheExtentStyle::Viewport),
            Some(ScrollCacheExtent::viewport(2.0))
        );
        assert_eq!(
            ScrollCacheExtent::defaulted(Some(2.0), CacheExtentStyle::Pixel),
            Some(ScrollCacheExtent::pixels(2.0))
        );
    }

    #[test]
    fn a_viewport_caches_a_screenful_or_so_unless_told_otherwise() {
        let default = ScrollCacheExtent::default();
        assert_eq!(default, ScrollCacheExtent::pixels(DEFAULT_CACHE_EXTENT));
        assert_eq!(default.style, CacheExtentStyle::Pixel);
        assert_eq!(DEFAULT_CACHE_EXTENT, 250.0);
        assert_eq!(CacheExtentStyle::default(), CacheExtentStyle::Pixel);
    }

    #[test]
    fn the_two_styles_are_not_interchangeable_at_any_viewport_size() {
        // Guards the tests above from passing because the arms happen to
        // agree: for a multiplier of 1 they agree at exactly one size, and
        // that size is the value itself.
        let relative = ScrollCacheExtent::viewport(1.0);
        let fixed = ScrollCacheExtent::pixels(600.0);
        assert_eq!(relative.in_pixels(600.0), fixed.in_pixels(600.0));
        assert_ne!(relative.in_pixels(601.0), fixed.in_pixels(601.0));
    }
}
