// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! How far down a list you are.
//!
//! A long list with no scrollbar tells the reader nothing about how much of it
//! they have seen or how much is left, and there is no way to find out except
//! by scrolling to the end. Upstream this is `Scrollbar` over `RawScrollbar`,
//! painted by `ScrollbarPainter`; the arithmetic here is that painter's.
//!
//! It fades: in over 300ms when the list starts moving, and out again 600ms
//! after it stops, both edges on `Curves.fastOutSlowIn`. Upstream's
//! `_kScrollbarTimeToFade`, `_kScrollbarFadeDuration`, and the
//! `_fadeoutOpacityAnimation` that shapes the way in as well as the way out
//! -- and the reason is that a scrollbar is an answer to a question the
//! reader only asks while scrolling.
//!
//! It listens rather than asks. Upstream's `Scrollbar` is a
//! `NotificationListener<ScrollNotification>` wrapping the scrollable: the
//! position arrives by bubbling notification, which is what lets it live
//! above the scrollable in the tree without either one holding the other.
//! Here it is [`notification_listener`] that does the same job, with the
//! offset kept in the state and updated from the notifications that go by.

use std::rc::Rc;

use crate::animation::Curve;
use crate::components::theme_of;
use crate::engine::Color;
use crate::framework::{
    AnyWidget, BuildContext, StateHandle, StatefulComponent, notification_listener, single,
    stateful,
};
use crate::render::{Axis, EdgeInsets, RenderIgnorePointer, RenderStack, StackPosition};
use crate::scrolling::ScrollNotification;

/// How thick the thumb is. Upstream's Material `_kScrollbarThickness`.
pub const THICKNESS: f32 = 8.0;

/// The rounding of the thumb's corners. Upstream Material's `_kScrollbarRadius`.
pub const RADIUS: f32 = 8.0;

/// The shortest a thumb may be drawn, however long the list is. Upstream's
/// `_kScrollbarMinLength`: a thumb that shrinks with the content becomes a
/// dot on a list of ten thousand rows, and a dot cannot be aimed at.
pub const MIN_THUMB_LENGTH: f32 = 48.0;

/// How long after the last scroll the thumb starts to go.
pub const TIME_TO_FADE_MICROS: i64 = 600_000;

/// How long it takes to go.
pub const FADE_MICROS: i64 = 300_000;

/// The measurements a scrollbar is drawn and faded with.
///
/// Material's values are the default -- the constants above; Cupertino's
/// `CupertinoScrollbar` (cupertino/scrollbar.dart) is the same widget with a
/// different set, which is why they are parameters rather than constants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarMetrics {
    /// How thick the thumb is. Material's [`THICKNESS`].
    pub thickness: f32,
    /// The rounding of the thumb's corners. Material's [`RADIUS`].
    pub radius: f32,
    /// The shortest a thumb may be drawn, however long the list is.
    /// Material's [`MIN_THUMB_LENGTH`].
    pub min_thumb_length: f32,
    /// How far the thumb sits in from the edge of the scrollable.
    pub cross_axis_margin: f32,
    /// How long after the last scroll the thumb starts to go.
    /// Material's [`TIME_TO_FADE_MICROS`].
    pub time_to_fade_micros: i64,
    /// How long the fade itself takes. Material's [`FADE_MICROS`].
    pub fade_micros: i64,
}

impl Default for ScrollbarMetrics {
    fn default() -> ScrollbarMetrics {
        ScrollbarMetrics {
            thickness: THICKNESS,
            radius: RADIUS,
            min_thumb_length: MIN_THUMB_LENGTH,
            cross_axis_margin: 2.0,
            time_to_fade_micros: TIME_TO_FADE_MICROS,
            fade_micros: FADE_MICROS,
        }
    }
}

/// Where the thumb goes and how big it is.
///
/// `viewport` is what is visible, `content` is the whole thing, `offset` is how
/// far down. Returns `None` when there is nothing to scroll -- upstream hides
/// the scrollbar in exactly that case rather than drawing a full-length thumb.
pub fn thumb(viewport: f32, content: f32, offset: f32) -> Option<(f32, f32)> {
    thumb_within(viewport, content, offset, MIN_THUMB_LENGTH)
}

/// [`thumb`] with the minimum length as a parameter: Cupertino's
/// `_kScrollbarMinLength` is 36, not Material's 48.
pub fn thumb_within(
    viewport: f32,
    content: f32,
    offset: f32,
    min_thumb_length: f32,
) -> Option<(f32, f32)> {
    if viewport <= 0.0 || content <= viewport {
        return None;
    }
    // The proportion of the content that is visible, floored so it can still
    // be grabbed. Upstream's ScrollbarPainter does the same in
    // `_thumbExtent`.
    let proportional = viewport / content * viewport;
    let length = proportional.max(min_thumb_length).min(viewport);
    let max_offset = content - viewport;
    let fraction = (offset / max_offset).clamp(0.0, 1.0);
    // The thumb travels the track *minus its own length*, which is why this is
    // not simply the scroll fraction times the viewport: at the bottom the
    // thumb's far edge has to land on the far edge of the track.
    Some((fraction * (viewport - length), length))
}

/// What a [`Scrollbar`] remembers between frames.
///
/// The geometry arrives by notification, between frames; the fade is driven
/// by the frame clock after it. Upstream keeps the same two halves: the
/// painter updated from the notifications that go by, and
/// `_fadeoutAnimationController`, whose curved value is the thumb's opacity.
#[derive(Default)]
pub struct ScrollbarState {
    /// Where the scroll is and how big the content and the viewport are, as
    /// the last notification reported them. Upstream's scrollbar state reads
    /// the same numbers out of the position each notification carries.
    offset: f32,
    viewport: f32,
    content: f32,
    /// How far through the fade the thumb is, from hidden (0) to shown (1),
    /// before the curve bends it. Upstream's `_fadeoutAnimationController`
    /// value, which starts dismissed -- the thumb starts hidden, and nothing
    /// shows it but a movement.
    fade: f32,
    /// Which way the fade is running: upstream runs the controller `forward`
    /// for a movement and `reverse` when the fade-out timer has fired.
    fading_out: bool,
    /// When the fade-out begins, if the delay is running. Upstream's
    /// `_fadeoutTimer`, the 600ms timer the scroll's end starts. A
    /// notification cannot know the clock, so the frame that observes a
    /// movement is when the countdown starts from.
    fadeout_at_micros: Option<i64>,
    /// Bumped by each notification that moved the thumb -- an update or an
    /// overscroll, and nothing else, which is upstream's rule for showing it
    /// too. [`advance`](Scrollbar::advance) is where the bump is noticed, the
    /// first frame after it.
    moved: u32,
    observed: u32,
    /// The frame clock as of the last advance, so the fade steps by the time
    /// that actually passed rather than by frames counted.
    last_frame_micros: Option<i64>,
}

impl ScrollbarState {
    /// How visible the thumb is right now.
    ///
    /// The fade's progress bent by `Curves.fastOutSlowIn`, which is upstream's
    /// `_fadeoutOpacityAnimation`: a `CurvedAnimation` over the 300ms fade
    /// controller, run forward on the way in and reverse on the way out, so
    /// both edges leave quickly and arrive slowly.
    fn opacity(&self) -> f32 {
        Curve::FAST_OUT_SLOW_IN.transform(self.fade)
    }
}

/// A thumb along the edge of a scrollable, showing how far down it is.
///
/// It wraps the scrollable and paints the thumb over it, so adding one cannot
/// change where anything in the child ends up -- upstream's arrangement too,
/// where the scrollbar is an ancestor, not a layout participant.
///
/// It is told nothing from the outside, at construction or after: everything
/// it knows -- where the scroll is, how much of the content is on screen, how
/// long the content is -- arrives in the [`ScrollNotification`]s that bubble
/// out of the child, which is upstream's wiring exactly. There the
/// `Scrollbar`'s state reads the same numbers out of the position each
/// notification carries; here they are learned the same way, from
/// [`ScrollNotification::metrics`]. Until something scrolls there is nothing
/// to say and the thumb stays hidden, which is also upstream's cold start.
///
/// ```ignore
/// component(Scrollbar::new(
///     || list, // the scrollable, rebuilt when the scrollbar rebuilds
/// ))
/// ```
pub struct Scrollbar {
    axis: Axis,
    color: Option<Color>,
    metrics: ScrollbarMetrics,
    /// The child, as a builder: stateful widgets are rebuilt from the same
    /// widget instance, so a child held by value would be consumed by the
    /// first build and missing from the second.
    build_child: Rc<dyn Fn() -> AnyWidget>,
}

impl Scrollbar {
    pub fn new(build_child: impl Fn() -> AnyWidget + 'static) -> Scrollbar {
        Scrollbar {
            axis: Axis::Vertical,
            color: None,
            metrics: ScrollbarMetrics::default(),
            build_child: Rc::new(build_child),
        }
    }

    pub fn horizontal(mut self) -> Self {
        self.axis = Axis::Horizontal;
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Measurements other than Material's defaults -- Cupertino's
    /// `CupertinoScrollbar` is this widget with its own set.
    pub fn with_metrics(mut self, metrics: ScrollbarMetrics) -> Self {
        self.metrics = metrics;
        self
    }
}

impl StatefulComponent for Scrollbar {
    type State = ScrollbarState;

    fn advance(&self, state: &mut ScrollbarState, frame_time_micros: i64) -> bool {
        let was = state.opacity();
        if state.observed != state.moved {
            // A movement: the fade-out delay restarts, and a fade in progress
            // turns around from wherever it got to -- upstream cancels the
            // timer and runs the controller forward.
            state.observed = state.moved;
            state.fading_out = false;
            state.fadeout_at_micros = Some(frame_time_micros + self.metrics.time_to_fade_micros);
        }
        let step = state.last_frame_micros.map_or(0.0, |previous| {
            (frame_time_micros - previous).max(0) as f32 / self.metrics.fade_micros as f32
        });
        state.last_frame_micros = Some(frame_time_micros);
        if state.fadeout_at_micros.is_some() {
            if state.fading_out {
                state.fade = (state.fade - step).max(0.0);
            } else {
                state.fade = (state.fade + step).min(1.0);
            }
        }
        // The delay is up: the fade turns around, from full shown, on the
        // frame that passes the deadline.
        if !state.fading_out
            && state
                .fadeout_at_micros
                .is_some_and(|at| frame_time_micros >= at)
        {
            state.fading_out = true;
        }
        let now = state.opacity();
        // Keep asking for frames while the thumb is on its way anywhere or
        // has only this frame reached nothing: the frame that reaches zero
        // still has to be drawn, or the thumb stays on screen.
        now > 0.0 || was > 0.0 || (!state.fading_out && state.fadeout_at_micros.is_some())
    }

    fn build(
        &self,
        state: &ScrollbarState,
        handle: StateHandle<ScrollbarState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        // The idle thumb: upstream's `_thumbColor` with no state attached is
        // `onSurface.withOpacity(0.3)` in a dark scheme, and `text_muted` is
        // this port's nearest on-surface.
        let color = self.color.unwrap_or(theme.text_muted);
        let opacity = state.opacity();
        let thumb = thumb_within(
            state.viewport,
            state.content,
            state.offset,
            self.metrics.min_thumb_length,
        );
        let axis = self.axis;
        let metrics = self.metrics;

        // The listener upstream puts between the scrollable and everything
        // above it. Every kind of scroll notification carries where the
        // scrollable was and how big it is; the scrollbar takes it, says
        // nothing, and lets the notification go on -- an ancestor may want it
        // too.
        notification_listener(
            move |notification: &ScrollNotification| {
                match notification {
                    // The reader changing direction does not move the thumb.
                    ScrollNotification::UserScroll { .. } => {}
                    _ => {
                        // What shows it: an update or an overscroll, and only
                        // those -- upstream's `_handleScrollNotification` runs
                        // its fade forward for exactly the two that say the
                        // position changed, so a start on its own shows
                        // nothing, and a drag pinned at the edge shows on its
                        // overscrolls alone.
                        let moved = matches!(
                            notification,
                            ScrollNotification::Update { .. }
                                | ScrollNotification::Overscroll { .. }
                        );
                        let metrics = notification.metrics();
                        handle.set_state(move |state| {
                            state.offset = metrics.pixels;
                            state.viewport = metrics.viewport_dimension;
                            // The extent the metrics carry is how far the
                            // content can *scroll*; the whole of it is that
                            // plus a viewport's worth, which is upstream's
                            // `maxScrollExtent + viewportDimension` too.
                            state.content = metrics.max_scroll_extent + metrics.viewport_dimension;
                            if moved {
                                state.moved = state.moved.wrapping_add(1);
                            }
                        });
                    }
                }
                false
            },
            single((self.build_child)(), move |child| {
                let mut stack = RenderStack::new().push(child);
                if let (Some((start, length)), true) = (thumb, opacity > 0.0) {
                    let bar = crate::widgets::Container::new()
                        .with_color(color.with_alpha((0x4D as f32 * opacity) as u8))
                        .with_corner_radius(metrics.radius);
                    let (bar, position) = match axis {
                        Axis::Vertical => (
                            bar.with_size(metrics.thickness, length),
                            StackPosition {
                                top: Some(start),
                                right: Some(metrics.cross_axis_margin),
                                ..Default::default()
                            },
                        ),
                        Axis::Horizontal => (
                            bar.with_size(length, metrics.thickness),
                            StackPosition {
                                left: Some(start),
                                bottom: Some(metrics.cross_axis_margin),
                                ..Default::default()
                            },
                        ),
                    };
                    // Invisible to the list, not the other way round: a bar
                    // that took the taps meant for the rows underneath would
                    // be worse than no bar.
                    stack = stack.push_positioned(RenderIgnorePointer::new(bar), position);
                }
                stack
            }),
        )
    }
}

/// [`Scrollbar`] as a widget.
pub fn scrollbar(build_child: impl Fn() -> AnyWidget + 'static) -> AnyWidget {
    stateful(Scrollbar::new(build_child))
}

/// Padding a list should leave for a scrollbar that is drawn over it.
pub const GUTTER: EdgeInsets = EdgeInsets::only(0.0, 0.0, THICKNESS + 4.0, 0.0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_scroll_is_nothing_to_draw() {
        assert!(thumb(500.0, 400.0, 0.0).is_none(), "the content fits");
        assert!(thumb(500.0, 500.0, 0.0).is_none(), "exactly fits");
        assert!(thumb(0.0, 500.0, 0.0).is_none(), "no viewport at all");
    }

    #[test]
    fn the_thumb_is_the_visible_proportion() {
        // A quarter of the content is visible, so the thumb is a quarter of
        // the track.
        let (start, length) = thumb(500.0, 2000.0, 0.0).expect("scrollable");
        assert_eq!(length, 125.0);
        assert_eq!(start, 0.0);
    }

    #[test]
    fn the_thumb_reaches_the_bottom_at_the_bottom() {
        let viewport = 500.0;
        let content = 2000.0;
        let (start, length) = thumb(viewport, content, content - viewport).expect("scrollable");
        // The thumb's far edge lands on the track's far edge, which is what
        // makes "am I at the end" answerable at a glance.
        assert!((start + length - viewport).abs() < 0.01);
    }

    #[test]
    fn a_very_long_list_still_has_a_thumb_you_can_see() {
        let (_, length) = thumb(500.0, 500_000.0, 0.0).expect("scrollable");
        assert_eq!(length, MIN_THUMB_LENGTH, "a dot cannot be aimed at");
    }

    #[test]
    fn the_thumb_starts_hidden_and_fades_both_ways_on_the_curve() {
        let mut state = ScrollbarState::default();
        let bar = Scrollbar::new(|| crate::framework::leaf(|| crate::widgets::Empty));

        // Nothing has moved: hidden, and asking for no frames. Upstream's
        // cold start, the controller sitting dismissed.
        assert!(!bar.advance(&mut state, 1_000_000));
        assert_eq!(state.opacity(), 0.0);

        // A movement. The way in is 300ms on fastOutSlowIn: quick to leave,
        // so halfway through the fade the thumb is past half visible.
        state.moved += 1;
        assert!(bar.advance(&mut state, 1_000_000), "the fade wants frames");
        assert!(bar.advance(&mut state, 1_150_000));
        let half_in = state.opacity();
        assert!(
            (half_in - Curve::FAST_OUT_SLOW_IN.transform(0.5)).abs() < 1e-4,
            "on the curve, not the line: {half_in}"
        );
        assert!(half_in > 0.5, "fast out of the gate: {half_in}");
        assert!(bar.advance(&mut state, 1_300_000));
        assert_eq!(state.opacity(), 1.0, "fully in at 300ms");

        // 600ms after the movement, out over 300ms on the same curve.
        assert!(bar.advance(&mut state, 1_600_000), "the delay ends here");
        assert_eq!(state.opacity(), 1.0, "and the turn happens after it");
        assert!(bar.advance(&mut state, 1_750_000));
        let half_out = state.opacity();
        assert!(
            (half_out - Curve::FAST_OUT_SLOW_IN.transform(0.5)).abs() < 1e-4,
            "the way out is the same curve: {half_out}"
        );
        assert!(
            bar.advance(&mut state, 1_900_000),
            "the frame that reaches zero still draws"
        );
        assert_eq!(state.opacity(), 0.0);
        assert!(!bar.advance(&mut state, 1_916_667), "and then it is idle");
    }

    #[test]
    fn scrolling_again_brings_it_back() {
        let mut state = ScrollbarState::default();
        let still = Scrollbar::new(|| crate::framework::leaf(|| crate::widgets::Empty));

        // Shown once, and long enough idle to have gone entirely.
        state.moved += 1;
        still.advance(&mut state, 1_000_000);
        still.advance(&mut state, 1_300_000);
        still.advance(&mut state, 1_600_000);
        still.advance(&mut state, 1_900_000);
        assert_eq!(state.opacity(), 0.0);

        // Another movement brings it back, from nothing and on the same
        // curve. (The counter is bumped here directly: the path it arrives by
        // is the tree tests below.)
        state.moved += 1;
        still.advance(&mut state, 1_916_667);
        assert!(still.advance(&mut state, 2_050_000));
        let coming_back = state.opacity();
        assert!(
            coming_back > 0.0 && coming_back < 1.0,
            "halfway back in: {coming_back}"
        );
        assert_eq!(
            coming_back,
            Curve::FAST_OUT_SLOW_IN.transform(0.5),
            "from nothing, on the way in's curve"
        );
    }

    // -- The notifications driving it --

    use std::cell::RefCell;

    use crate::framework::{ElementTree, leaf, stateful};
    use crate::scrolling::Scroll;

    /// What the scrollbar tests need below it: a scrollable that reports.
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
            state
                .scroll
                .set_notification_sink(context.notification_sink());
            *self.handles.borrow_mut() = Some(handle);
            leaf(|| crate::widgets::Empty)
        }
    }

    #[test]
    fn the_thumb_follows_the_scroll_notifications() {
        let handles = Rc::new(RefCell::new(None));
        let builder_handles = handles.clone();
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(Scrollbar::new(move || {
            stateful(Scroller {
                handles: builder_handles.clone(),
                extent: 2000.0,
            })
        })));
        let bar = tree.root().expect("mounted");
        let handle = handles.borrow().clone().expect("built");

        // Nothing has happened; no thumb anywhere.
        assert_eq!(
            tree.state::<ScrollbarState, _>(bar, |state| state.offset),
            Some(0.0)
        );

        // A wheel notch of 80px: the scrollbar hears the update through the
        // tree, without the caller telling it anything -- and the notch ends
        // its own scroll, so there is nothing to release.
        handle.set_state(|state| state.scroll.scroll_by(80.0));
        tree.rebuild_dirty();
        assert_eq!(
            tree.state::<ScrollbarState, _>(bar, |state| state.offset),
            Some(80.0)
        );

        // The geometry came the same way: the metrics carried a 500-viewport
        // and 2000 of scroll, so the content the bar sizes its thumb against
        // is a viewport's worth more than the scroll.
        assert_eq!(
            tree.state::<ScrollbarState, _>(bar, |state| state.viewport),
            Some(500.0)
        );
        assert_eq!(
            tree.state::<ScrollbarState, _>(bar, |state| state.content),
            Some(2500.0)
        );

        // And the movement brought the thumb up -- fading in, so the frame
        // that notices the movement is nothing yet and 300ms later it is all
        // the way in.
        let wants_more = tree.advance_frame(1_000_000);
        tree.rebuild_dirty();
        assert!(wants_more, "the fade wants frames");
        assert_eq!(
            tree.state::<ScrollbarState, _>(bar, |state| state.opacity()),
            Some(0.0)
        );
        tree.advance_frame(1_300_000);
        tree.rebuild_dirty();
        assert_eq!(
            tree.state::<ScrollbarState, _>(bar, |state| state.opacity()),
            Some(1.0)
        );
    }

    #[test]
    fn a_start_alone_shows_nothing_and_an_overscroll_at_the_edge_shows() {
        // Upstream shows the thumb for updates and overscrolls, and for
        // nothing else: a start on its own moves nothing, and a scroll pinned
        // at the edge moves nothing but the bound's refusal -- which is an
        // overscroll, and heard.
        let handles = Rc::new(RefCell::new(None));
        let builder_handles = handles.clone();
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(Scrollbar::new(move || {
            stateful(Scroller {
                handles: builder_handles.clone(),
                extent: 500.0,
            })
        })));
        let bar = tree.root().expect("mounted");
        let handle = handles.borrow().clone().expect("built");

        // To the bottom edge, and let the jump's showing fade all the way out
        // again.
        handle.set_state(|state| state.scroll.jump_to(500.0));
        tree.rebuild_dirty();
        let mut now = 1_000_000;
        while tree.advance_frame(now) {
            tree.rebuild_dirty();
            now += 100_000;
        }
        assert_eq!(
            tree.state::<ScrollbarState, _>(bar, |state| state.opacity()),
            Some(0.0)
        );

        // A fling started and caught before it moved anything: its start (and
        // the catch's end) show nothing.
        handle.set_state(|state| state.scroll.fling(-3000.0));
        handle.set_state(|state| state.scroll.stop());
        tree.rebuild_dirty();
        tree.advance_frame(now);
        tree.rebuild_dirty();
        assert_eq!(
            tree.state::<ScrollbarState, _>(bar, |state| state.opacity()),
            Some(0.0),
            "a start and an end moved nothing, so nothing showed"
        );

        // A wheel notch pinned at the edge: no update, only an overscroll --
        // and that shows.
        handle.set_state(|state| state.scroll.scroll_by(50.0));
        tree.rebuild_dirty();
        tree.advance_frame(now);
        tree.rebuild_dirty();
        tree.advance_frame(now + 300_000);
        tree.rebuild_dirty();
        assert_eq!(
            tree.state::<ScrollbarState, _>(bar, |state| state.opacity()),
            Some(1.0),
            "the overscroll alone brought the thumb up"
        );
    }

    #[test]
    fn a_scroll_under_the_scrollbar_keeps_bubbling_past_it() {
        // The scrollbar's listener says false, so a listener *outside* it
        // still hears the scroll -- a scrollbar must not swallow what it
        // overhears.
        let handles = Rc::new(RefCell::new(None));
        let builder_handles = handles.clone();
        let heard = Rc::new(RefCell::new(0));
        let count = heard.clone();
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::notification_listener(
            move |_: &ScrollNotification| {
                *count.borrow_mut() += 1;
                true
            },
            stateful(Scrollbar::new(move || {
                stateful(Scroller {
                    handles: builder_handles.clone(),
                    extent: 2000.0,
                })
            })),
        ));
        let bar = tree.children_of(tree.root().expect("mounted"))[0];
        let handle = handles.borrow().clone().expect("built");

        handle.set_state(|state| state.scroll.scroll_by(50.0));
        tree.rebuild_dirty();

        assert_eq!(
            tree.state::<ScrollbarState, _>(bar, |state| state.offset),
            Some(50.0),
            "the scrollbar heard it",
        );
        assert!(*heard.borrow() >= 1, "and so did the listener outside it");
    }
}
