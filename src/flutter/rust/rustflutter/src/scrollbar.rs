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
        // Upstream's `RawScrollbar.build` reads `ScrollbarTheme.of(context)`
        // for the thickness, the thumb's colour and the margins before its
        // own defaults. A `Scrollbar` given metrics outright -- which is how
        // `CupertinoScrollbar` is built from this one -- keeps them: the
        // widget's own field is the first step of upstream's chain.
        let scrollbar = crate::component_themes::ResolvedScrollbar::of(
            context,
            crate::widget_state::WidgetStates::NONE,
        );
        // The idle thumb: upstream's `_thumbColor` with no state attached is
        // `onSurface.withOpacity(0.3)` in a dark scheme, and `text_muted` is
        // this port's nearest on-surface.
        let themed_thumb = crate::component_themes::ScrollbarTheme::of(context)
            .thumb_color
            .as_ref()
            .and_then(|property| property.resolve(crate::widget_state::WidgetStates::NONE));
        let color = self.color.or(themed_thumb).unwrap_or(theme.text_muted);
        let opacity = state.opacity();
        let metrics = if self.metrics == ScrollbarMetrics::default() {
            // Nobody overrode them, so the theme has its say.
            ScrollbarMetrics {
                thickness: scrollbar.thickness,
                radius: scrollbar.radius.x,
                min_thumb_length: scrollbar.min_thumb_length,
                cross_axis_margin: scrollbar.cross_axis_margin.max(2.0),
                ..self.metrics
            }
        } else {
            self.metrics
        };
        let thumb = thumb_within(
            state.viewport,
            state.content,
            state.offset,
            metrics.min_thumb_length,
        );
        let axis = self.axis;

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

// -- Upstream's full scrollbar ------------------------------------------------
//
// Everything above is the simplified arithmetic this crate's own `Scrollbar`
// uses. What follows is upstream's `ScrollbarPainter`, `RawScrollbar` and
// `RawScrollbarState`, which differ in three ways that matter: margins at the
// ends of the track, a thumb that is allowed to shrink below its minimum while
// overscrolling, and hit testing that treats a fingertip and a cursor
// differently.

use crate::engine::Rect;
use crate::gestures::PointerKind;
use crate::render::AxisDirection;
use crate::scroll_plumbing::ScrollPlatform;
use crate::scrolling::ScrollMetrics;

/// Upstream `ScrollbarOrientation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollbarOrientation {
    Left,
    Right,
    Top,
    Bottom,
}

impl ScrollbarOrientation {
    pub fn axis(self) -> Axis {
        match self {
            ScrollbarOrientation::Left | ScrollbarOrientation::Right => Axis::Vertical,
            ScrollbarOrientation::Top | ScrollbarOrientation::Bottom => Axis::Horizontal,
        }
    }
}

/// Upstream `ScrollbarPainter`.
///
/// A `ChangeNotifier` that is also a painter: it is handed metrics and works
/// out the thumb's size and position, and it is the object the gesture handlers
/// ask when they need to turn a position on the track into a scroll offset.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollbarPainter {
    /// The smallest the thumb is allowed to get while scrolling normally.
    /// Upstream's default is 18 logical pixels -- below that there is nothing
    /// left to grab.
    pub min_length: f32,
    /// The smallest it may get while **over**scrolling, which is allowed to be
    /// less. Must not exceed `min_length`.
    pub min_overscroll_length: f32,
    pub main_axis_margin: f32,
    pub cross_axis_margin: f32,
    pub thickness: f32,
    pub ignore_pointer: bool,
    /// The fade animation's current value. A scrollbar at zero is not there.
    pub fadeout_opacity: f32,
    metrics: Option<ScrollMetrics>,
    axis_direction: AxisDirection,
    track_extent: f32,
    thumb_extent: f32,
    notifications: usize,
}

impl ScrollbarPainter {
    pub const DEFAULT_MIN_LENGTH: f32 = 18.0;
    /// Upstream `_kMinInteractiveSize`: the smallest thing a finger can be
    /// expected to hit.
    pub const MIN_INTERACTIVE_SIZE: f32 = 48.0;
    /// Upstream's observation, written into the source: iOS's thumb reaches its
    /// smallest at about a fifth of a viewport of overscroll.
    pub const OVERSCROLL_FRACTION_AT_MINIMUM: f32 = 0.2;

    pub fn new(track_extent: f32) -> ScrollbarPainter {
        ScrollbarPainter {
            min_length: ScrollbarPainter::DEFAULT_MIN_LENGTH,
            min_overscroll_length: ScrollbarPainter::DEFAULT_MIN_LENGTH,
            main_axis_margin: 0.0,
            cross_axis_margin: 0.0,
            thickness: 6.0,
            ignore_pointer: false,
            fadeout_opacity: 1.0,
            metrics: None,
            axis_direction: AxisDirection::Down,
            track_extent,
            thumb_extent: 0.0,
            notifications: 0,
        }
    }

    pub fn with_min_lengths(mut self, min_length: f32, min_overscroll_length: f32) -> Self {
        debug_assert!(min_length >= 0.0);
        debug_assert!(
            min_overscroll_length <= min_length,
            "the overscroll minimum is the smaller of the two"
        );
        self.min_length = min_length;
        self.min_overscroll_length = min_overscroll_length;
        self
    }

    pub fn notification_count(&self) -> usize {
        self.notifications
    }

    pub fn thumb_extent(&self) -> f32 {
        self.thumb_extent
    }

    /// The distance the thumb may travel: the track, less the margins at each
    /// end.
    pub fn traversable_track_extent(&self) -> f32 {
        (self.track_extent - self.main_axis_margin * 2.0).max(0.0)
    }

    fn is_reversed(&self) -> bool {
        matches!(self.axis_direction, AxisDirection::Up | AxisDirection::Left)
    }

    pub fn is_vertical(&self) -> bool {
        matches!(self.axis_direction, AxisDirection::Up | AxisDirection::Down)
    }

    /// Whether there is anything to scroll. A scrollbar over content that fits
    /// is drawn but not touchable.
    pub fn metrics_are_scrollable(&self) -> bool {
        self.metrics
            .as_ref()
            .is_some_and(|metrics| metrics.min_scroll_extent != metrics.max_scroll_extent)
    }

    /// Upstream `update`, which returns early when nothing that matters moved.
    ///
    /// The comparison is on `extentBefore`, `extentInside`, `extentAfter` and
    /// the axis direction -- **not on `pixels`**. Those three are what the thumb
    /// is drawn from, and a change in pixels that leaves all three alone (an
    /// overscroll that the physics absorbed, say) has nothing to repaint for.
    pub fn update(&mut self, metrics: ScrollMetrics, axis_direction: AxisDirection) -> bool {
        if let Some(last) = &self.metrics {
            if last.extent_before() == metrics.extent_before()
                && last.extent_inside() == metrics.extent_inside()
                && last.extent_after() == metrics.extent_after()
                && self.axis_direction == axis_direction
            {
                return false;
            }
        }
        self.metrics = Some(metrics);
        self.axis_direction = axis_direction;
        self.set_thumb_extent();
        self.notifications += 1;
        true
    }

    /// The total content, viewport included. Upstream's `_totalContentExtent`.
    fn total_content_extent(&self) -> f32 {
        let metrics = self.metrics.as_ref().unwrap();
        metrics.max_scroll_extent - metrics.min_scroll_extent + metrics.viewport_dimension
    }

    /// Upstream `_setThumbExtent`.
    ///
    /// The thumb's size is the fraction of the content that is visible, which
    /// is the one thing a scrollbar is actually for: **its length is the answer
    /// to "how much of this am I looking at".**
    ///
    /// The floor is where it gets interesting. While scrolling normally the
    /// thumb never goes below `min_length`, because below that there is nothing
    /// to grab. While *over*scrolling it may, down to
    /// `min_overscroll_length` -- and upstream cannot interpolate that with the
    /// visible fraction, because the fraction does not move smoothly across the
    /// boundary. So it uses the fraction of the viewport still holding content
    /// instead, and maps `[0.8, 1.0]` onto `[0.0, 1.0]`: the observed iOS
    /// behaviour is that the thumb reaches its smallest at about 20% of
    /// overscroll.
    fn set_thumb_extent(&mut self) {
        let Some(metrics) = self.metrics.clone() else {
            return;
        };
        let traversable = self.traversable_track_extent();
        let offsets = self.main_axis_margin * 2.0;
        let fraction_visible = ((metrics.extent_inside() - offsets)
            / (self.total_content_extent() - offsets))
            .clamp(0.0, 1.0);

        let thumb_extent =
            (traversable * fraction_visible).max(traversable.min(self.min_overscroll_length));

        let fraction_overscrolled = 1.0 - metrics.extent_inside() / metrics.viewport_dimension;
        let safe_min_length = self.min_length.min(traversable);
        let overscrolling = !(metrics.extent_before() > 0.0 && metrics.extent_after() > 0.0);
        let new_min_length = if overscrolling {
            safe_min_length
                * (1.0
                    - fraction_overscrolled
                        .clamp(0.0, ScrollbarPainter::OVERSCROLL_FRACTION_AT_MINIMUM)
                        / ScrollbarPainter::OVERSCROLL_FRACTION_AT_MINIMUM)
        } else {
            safe_min_length
        };

        // Upstream's note: the thumb must not exceed the track, "otherwise the
        // scrollbar may scroll towards the wrong direction" -- a thumb longer
        // than its travel makes the movable extent negative and inverts the
        // mapping below.
        self.thumb_extent = thumb_extent.clamp(new_min_length.min(traversable), traversable);
    }

    /// How far the thumb's leading edge sits along the track. Upstream's
    /// `_getScrollToTrack`.
    pub fn thumb_offset(&self) -> f32 {
        let Some(metrics) = &self.metrics else {
            return 0.0;
        };
        let scrollable_extent = metrics.max_scroll_extent - metrics.min_scroll_extent;
        let fraction_past = if scrollable_extent > 0.0 {
            ((metrics.pixels - metrics.min_scroll_extent) / scrollable_extent).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let fraction_past = if self.is_reversed() {
            1.0 - fraction_past
        } else {
            fraction_past
        };
        fraction_past * (self.traversable_track_extent() - self.thumb_extent)
    }

    /// Upstream `getTrackToScroll`: the inverse of the above, and the reason
    /// the pair has to be exact.
    ///
    /// The divisor is the **movable** extent -- the track less the thumb's own
    /// length -- because that is how far the thumb can actually go. Dividing by
    /// the whole track instead would make the content stop short of the end
    /// with the thumb already against it.
    pub fn get_track_to_scroll(&self, thumb_offset_local: f32) -> f32 {
        let Some(metrics) = &self.metrics else {
            return 0.0;
        };
        let scrollable_extent = metrics.max_scroll_extent - metrics.min_scroll_extent;
        let movable = self.traversable_track_extent() - self.thumb_extent;
        if movable == 0.0 {
            return 0.0;
        }
        scrollable_extent * thumb_offset_local / movable
    }

    /// The thumb's rectangle along the main axis, as `(start, end)`.
    pub fn thumb_bounds(&self) -> (f32, f32) {
        let start = self.main_axis_margin + self.thumb_offset();
        (start, start + self.thumb_extent)
    }

    /// Upstream `hitTestInteractive`: whether a pointer at `position` along the
    /// main axis lands on the scrollbar at all, track included.
    ///
    /// The exception at the end is the whole reason this is not just a
    /// rectangle test: **a faded-out scrollbar is not hittable, unless a mouse
    /// is hovering.** That case has to work, because bringing it back into view
    /// is exactly what a mouse moving to the edge of the window is asking for.
    pub fn hit_test_interactive(&self, position: f32, kind: PointerKind, for_hover: bool) -> bool {
        if self.metrics.is_none() || self.ignore_pointer || !self.metrics_are_scrollable() {
            return false;
        }
        if self.fadeout_opacity == 0.0 && !(for_hover && kind == PointerKind::Mouse) {
            return false;
        }
        let (thumb_start, thumb_end) = self.thumb_bounds();
        let centre = (thumb_start + thumb_end) / 2.0;
        let padded_start = 0.0f32.min(centre - ScrollbarPainter::MIN_INTERACTIVE_SIZE / 2.0);
        let padded_end = self
            .track_extent
            .max(centre + ScrollbarPainter::MIN_INTERACTIVE_SIZE / 2.0);
        position >= padded_start && position <= padded_end
    }

    /// Upstream `hitTestOnlyThumbInteractive`.
    ///
    /// A finger gets the thumb expanded to the minimum interactive size; a
    /// mouse gets the thumb as drawn. **A cursor is a pixel and a fingertip is
    /// not**, and padding the target for a mouse would make a six-pixel
    /// scrollbar swallow clicks meant for the content beside it.
    pub fn hit_test_only_thumb_interactive(&self, position: f32, kind: PointerKind) -> bool {
        if self.metrics.is_none()
            || self.ignore_pointer
            || self.fadeout_opacity == 0.0
            || !self.metrics_are_scrollable()
        {
            return false;
        }
        let (start, end) = self.thumb_bounds();
        match kind {
            PointerKind::Touch | PointerKind::Trackpad => {
                let centre = (start + end) / 2.0;
                let half = ScrollbarPainter::MIN_INTERACTIVE_SIZE / 2.0;
                let start = start.min(centre - half);
                let end = end.max(centre + half);
                position >= start && position <= end
            }
            _ => position >= start && position <= end,
        }
    }

    /// The rectangle the thumb is drawn in, for a vertical scrollbar on the
    /// right of a viewport `cross_extent` wide.
    pub fn thumb_rect(&self, cross_extent: f32) -> Rect {
        let (start, end) = self.thumb_bounds();
        if self.is_vertical() {
            let x = cross_extent - self.thickness - self.cross_axis_margin;
            Rect::ltrb(x, start, x + self.thickness, end)
        } else {
            let y = cross_extent - self.thickness - self.cross_axis_margin;
            Rect::ltrb(start, y, end, y + self.thickness)
        }
    }
}

/// Upstream `RawScrollbar`.
#[derive(Clone, Debug, PartialEq)]
pub struct RawScrollbar {
    /// `None` means "decide from the platform and the gesture".
    pub thumb_visibility: Option<bool>,
    /// A track is only drawn under a thumb.
    pub track_visibility: Option<bool>,
    pub min_thumb_length: f32,
    pub min_overscroll_length: Option<f32>,
    /// How long the scrollbar stays after the scrolling stops.
    pub time_to_fade_ms: f32,
    /// How long a press must be held before the thumb can be dragged. Zero for
    /// a mouse, non-zero on touch platforms where a press might be a scroll.
    pub press_duration_ms: f32,
    pub interactive: Option<bool>,
    pub scrollbar_orientation: Option<ScrollbarOrientation>,
    pub has_radius: bool,
    pub has_shape: bool,
}

impl RawScrollbar {
    pub const DEFAULT_TIME_TO_FADE_MS: f32 = 600.0;

    pub fn new() -> RawScrollbar {
        RawScrollbar {
            thumb_visibility: None,
            track_visibility: None,
            min_thumb_length: ScrollbarPainter::DEFAULT_MIN_LENGTH,
            min_overscroll_length: None,
            time_to_fade_ms: RawScrollbar::DEFAULT_TIME_TO_FADE_MS,
            press_duration_ms: 0.0,
            interactive: None,
            scrollbar_orientation: None,
            has_radius: false,
            has_shape: false,
        }
    }

    /// Upstream's constructor asserts, each of which rules out a configuration
    /// that would draw something meaningless.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.thumb_visibility == Some(false) && self.track_visibility == Some(true) {
            // Upstream's message: a groove with nothing in it says nothing.
            return Err("A scrollbar track cannot be drawn without a scrollbar thumb.");
        }
        if self.min_thumb_length < 0.0 {
            return Err("minThumbLength must not be negative");
        }
        if let Some(overscroll) = self.min_overscroll_length {
            if overscroll > self.min_thumb_length {
                return Err("minOverscrollLength must not exceed minThumbLength");
            }
            if overscroll < 0.0 {
                return Err("minOverscrollLength must not be negative");
            }
        }
        if self.has_radius && self.has_shape {
            // Two ways of saying the same thing, and no rule for which wins.
            return Err("radius and shape cannot both be given");
        }
        Ok(())
    }

    /// Upstream's `minOverscrollLength ?? minThumbLength`.
    pub fn effective_min_overscroll_length(&self) -> f32 {
        self.min_overscroll_length.unwrap_or(self.min_thumb_length)
    }
}

impl Default for RawScrollbar {
    fn default() -> Self {
        RawScrollbar::new()
    }
}

/// What a tap on the track did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TrackTapOutcome {
    /// The scrollable refused the offset -- a locked position, say.
    Refused,
    /// One page towards the tap, over 100ms.
    Paged { direction: AxisDirection, by: f32 },
}

/// Upstream `RawScrollbarState`.
#[derive(Clone, Debug, PartialEq)]
pub struct RawScrollbarState {
    pub widget: RawScrollbar,
    pub painter: ScrollbarPainter,
    /// Whether the scrollbar is being shown regardless of the fade.
    show_scrollbar: bool,
    fadeout_timer_ms: Option<f32>,
    now_ms: f32,
}

impl RawScrollbarState {
    /// Upstream's page animation for a track tap.
    pub const TRACK_TAP_DURATION_MS: f32 = 100.0;

    pub fn new(widget: RawScrollbar, painter: ScrollbarPainter) -> RawScrollbarState {
        RawScrollbarState {
            show_scrollbar: widget.thumb_visibility.unwrap_or(false),
            widget,
            painter,
            fadeout_timer_ms: None,
            now_ms: 0.0,
        }
    }

    /// Upstream's `_showTrack`: a track is only drawn when the scrollbar is
    /// showing **and** the track was asked for.
    pub fn shows_track(&self) -> bool {
        self.show_scrollbar && self.widget.track_visibility.unwrap_or(false)
    }

    pub fn is_faded_out(&self) -> bool {
        self.painter.fadeout_opacity == 0.0
    }

    pub fn fadeout_pending(&self) -> bool {
        self.fadeout_timer_ms.is_some()
    }

    /// Upstream `_maybeStartFadeoutTimer`, whose guard is the whole method: a
    /// scrollbar that was asked to stay never starts one.
    pub fn maybe_start_fadeout_timer(&mut self) {
        if self.show_scrollbar {
            return;
        }
        self.fadeout_timer_ms = Some(self.now_ms + self.widget.time_to_fade_ms);
    }

    pub fn advance_ms(&mut self, delta: f32) {
        self.now_ms += delta;
        if let Some(at) = self.fadeout_timer_ms {
            if at <= self.now_ms {
                self.fadeout_timer_ms = None;
                self.painter.fadeout_opacity = 0.0;
            }
        }
    }

    /// A new scroll: the scrollbar comes back and the timer restarts.
    pub fn handle_scroll(&mut self) {
        self.painter.fadeout_opacity = 1.0;
        self.fadeout_timer_ms = None;
    }

    /// Upstream `handleTrackTapDown`.
    ///
    /// Tapping the track does **not** jump to the tapped position: it pages
    /// towards it, one page at a time, over 100ms. A jump would move the
    /// content further than the reader can follow, and the page is the unit
    /// they already know from the keyboard.
    pub fn handle_track_tap_down(
        &self,
        local_position: f32,
        page_extent: f32,
        accepts_user_offset: bool,
    ) -> TrackTapOutcome {
        if !accepts_user_offset {
            return TrackTapOutcome::Refused;
        }
        let past_thumb = local_position > self.painter.thumb_offset();
        let direction = match (self.painter.is_vertical(), past_thumb) {
            (true, true) => AxisDirection::Down,
            (true, false) => AxisDirection::Up,
            (false, true) => AxisDirection::Right,
            (false, false) => AxisDirection::Left,
        };
        let by = if past_thumb {
            page_extent
        } else {
            -page_extent
        };
        TrackTapOutcome::Paged { direction, by }
    }

    /// Whether the thumb can be dragged at all, given the platform. Upstream
    /// defaults `interactive` from the platform: a scrollbar drawn over touch
    /// content is an indicator, not a control.
    pub fn is_interactive(&self, platform: ScrollPlatform) -> bool {
        self.widget.interactive.unwrap_or(!matches!(
            platform,
            ScrollPlatform::Android | ScrollPlatform::IOS | ScrollPlatform::Fuchsia
        ))
    }
}

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
    // -- Upstream's ScrollbarPainter ------------------------------------------

    fn metrics(pixels: f32) -> ScrollMetrics {
        ScrollMetrics {
            pixels,
            min_scroll_extent: 0.0,
            max_scroll_extent: 1600.0,
            viewport_dimension: 400.0,
        }
    }

    fn painter() -> ScrollbarPainter {
        let mut painter = ScrollbarPainter::new(400.0);
        painter.update(metrics(0.0), AxisDirection::Down);
        painter
    }

    #[test]
    fn the_thumbs_length_answers_how_much_of_this_am_i_looking_at() {
        // 400 of 2000 is a fifth, so a fifth of the track.
        let painter = painter();
        assert!((painter.thumb_extent() - 80.0).abs() < 0.01);

        let mut half = ScrollbarPainter::new(400.0);
        half.update(
            ScrollMetrics {
                pixels: 0.0,
                min_scroll_extent: 0.0,
                max_scroll_extent: 400.0,
                viewport_dimension: 400.0,
            },
            AxisDirection::Down,
        );
        assert!((half.thumb_extent() - 200.0).abs() < 0.01);
    }

    #[test]
    fn dragging_the_thumb_and_reading_it_back_are_exact_inverses() {
        // Or the thumb slips out from under the finger holding it.
        let painter = painter();
        let movable = painter.traversable_track_extent() - painter.thumb_extent();
        assert_eq!(painter.get_track_to_scroll(0.0), 0.0);
        assert!(
            (painter.get_track_to_scroll(movable) - 1600.0).abs() < 0.01,
            "the far end of the thumb's travel is the far end of the content"
        );

        let mut at_end = ScrollbarPainter::new(400.0);
        at_end.update(metrics(1600.0), AxisDirection::Down);
        assert!(
            (at_end.thumb_offset() - movable).abs() < 0.01,
            "and the thumb is against the end when the content is"
        );
    }

    #[test]
    fn the_thumb_travels_the_track_less_its_own_length() {
        // Dividing by the whole track instead would leave the content short of
        // the end with the thumb already against it.
        let painter = painter();
        let track = painter.traversable_track_extent();
        assert_eq!(track, 400.0);
        assert!(painter.thumb_extent() > 0.0);
        let naive = 1600.0 * track / track;
        let actual = painter.get_track_to_scroll(track);
        assert!(actual > naive, "{actual} vs {naive}");
    }

    #[test]
    fn a_reversed_scrollbar_puts_the_thumb_at_the_other_end() {
        let mut down = ScrollbarPainter::new(400.0);
        down.update(metrics(0.0), AxisDirection::Down);
        let mut up = ScrollbarPainter::new(400.0);
        up.update(metrics(0.0), AxisDirection::Up);

        assert_eq!(down.thumb_offset(), 0.0);
        assert!(up.thumb_offset() > 0.0);
        assert!((up.thumb_offset() + down.thumb_extent() - 400.0).abs() < 0.01);
    }

    #[test]
    fn a_tiny_thumb_is_floored_so_there_is_something_to_grab() {
        let mut painter = ScrollbarPainter::new(400.0);
        painter.update(
            ScrollMetrics {
                pixels: 100.0,
                min_scroll_extent: 0.0,
                max_scroll_extent: 100_000.0,
                viewport_dimension: 400.0,
            },
            AxisDirection::Down,
        );
        assert_eq!(painter.thumb_extent(), ScrollbarPainter::DEFAULT_MIN_LENGTH);
    }

    #[test]
    fn the_floor_gives_way_while_overscrolling_and_not_before() {
        // Upstream's observation, written into the source: iOS's thumb reaches
        // its smallest at about a fifth of a viewport of overscroll. The
        // interpolation uses the fraction of the viewport still holding
        // content, because the visible fraction does not move smoothly across
        // that boundary.
        // The floor only bites on a list long enough that the proportional
        // thumb is smaller than it -- on a short list the thumb is bigger than
        // either minimum and neither number is consulted.
        let long = |pixels: f32| ScrollMetrics {
            pixels,
            min_scroll_extent: 0.0,
            max_scroll_extent: 100_000.0,
            viewport_dimension: 400.0,
        };
        let mut painter = ScrollbarPainter::new(400.0).with_min_lengths(18.0, 8.0);

        painter.update(long(50_000.0), AxisDirection::Down);
        assert_eq!(painter.thumb_extent(), 18.0, "scrolling normally");

        // Twenty per cent of the viewport past the end.
        painter.update(long(100_080.0), AxisDirection::Down);
        let fully_overscrolled = painter.thumb_extent();
        assert!(
            fully_overscrolled < 18.0,
            "below the ordinary minimum: {fully_overscrolled}"
        );
        assert!(fully_overscrolled >= 8.0, "and never below the other one");

        painter.update(long(100_020.0), AxisDirection::Down);
        let partly = painter.thumb_extent();
        assert!(
            partly > fully_overscrolled && partly < 18.0,
            "and part way there is part way down: {partly}"
        );
    }

    #[test]
    fn resting_at_the_top_is_not_overscrolling() {
        // extentBefore is zero there, which takes the same branch -- and the
        // formula has to degenerate to the ordinary minimum or the thumb would
        // shrink for no reason.
        let mut painter = ScrollbarPainter::new(400.0).with_min_lengths(18.0, 8.0);
        painter.update(
            ScrollMetrics {
                pixels: 0.0,
                min_scroll_extent: 0.0,
                max_scroll_extent: 100_000.0,
                viewport_dimension: 400.0,
            },
            AxisDirection::Down,
        );
        assert_eq!(painter.thumb_extent(), 18.0);
    }

    #[test]
    fn an_update_that_changes_nothing_visible_repaints_nothing() {
        // The comparison is on the three extents and the direction, not on
        // pixels: a change the physics absorbed has nothing to redraw.
        let mut painter = painter();
        let before = painter.notification_count();
        assert!(!painter.update(metrics(0.0), AxisDirection::Down));
        assert_eq!(painter.notification_count(), before);

        assert!(painter.update(metrics(40.0), AxisDirection::Down));
        assert_eq!(painter.notification_count(), before + 1);
    }

    // -- Hit testing ------------------------------------------------------------

    #[test]
    fn a_faded_out_scrollbar_is_not_there_unless_a_mouse_is_looking_for_it() {
        // Bringing it back into view is exactly what a mouse moving to the edge
        // of the window is asking for.
        let mut painter = painter();
        painter.fadeout_opacity = 0.0;

        assert!(!painter.hit_test_interactive(200.0, PointerKind::Touch, false));
        assert!(!painter.hit_test_interactive(200.0, PointerKind::Mouse, false));
        assert!(
            painter.hit_test_interactive(200.0, PointerKind::Mouse, true),
            "hovering finds it"
        );
        assert!(
            !painter.hit_test_interactive(200.0, PointerKind::Touch, true),
            "and a finger cannot hover"
        );
    }

    #[test]
    fn a_fingertip_gets_a_bigger_thumb_than_a_cursor_does() {
        // A cursor is a pixel and a fingertip is not; padding the target for a
        // mouse would make a thin scrollbar swallow clicks meant for the
        // content beside it.
        // Which only matters when the thumb is smaller than a fingertip: an
        // eighty-pixel thumb is already bigger than the minimum and gets no
        // padding at all.
        let mut painter = ScrollbarPainter::new(400.0);
        painter.update(
            ScrollMetrics {
                pixels: 0.0,
                min_scroll_extent: 0.0,
                max_scroll_extent: 100_000.0,
                viewport_dimension: 400.0,
            },
            AxisDirection::Down,
        );
        assert_eq!(painter.thumb_extent(), ScrollbarPainter::DEFAULT_MIN_LENGTH);
        let (_, thumb_end) = painter.thumb_bounds();
        let just_past = thumb_end + 10.0;

        assert!(painter.hit_test_only_thumb_interactive(just_past, PointerKind::Touch));
        assert!(!painter.hit_test_only_thumb_interactive(just_past, PointerKind::Mouse));
        assert!(
            painter.hit_test_only_thumb_interactive(thumb_end - 1.0, PointerKind::Mouse),
            "the mouse still gets the thumb as drawn"
        );
    }

    #[test]
    fn a_scrollbar_over_content_that_fits_is_drawn_but_not_touchable() {
        let mut painter = ScrollbarPainter::new(400.0);
        painter.update(
            ScrollMetrics {
                pixels: 0.0,
                min_scroll_extent: 0.0,
                max_scroll_extent: 0.0,
                viewport_dimension: 400.0,
            },
            AxisDirection::Down,
        );
        assert!(!painter.metrics_are_scrollable());
        assert!(!painter.hit_test_interactive(200.0, PointerKind::Mouse, false));
        assert!(!painter.hit_test_only_thumb_interactive(0.0, PointerKind::Mouse));
    }

    #[test]
    fn ignoring_the_pointer_beats_everything_else() {
        let mut painter = painter();
        painter.ignore_pointer = true;
        assert!(!painter.hit_test_interactive(200.0, PointerKind::Mouse, true));
        assert!(!painter.hit_test_only_thumb_interactive(20.0, PointerKind::Touch));
    }

    #[test]
    fn the_thumb_is_drawn_against_the_far_edge_at_its_own_thickness() {
        let painter = painter();
        let rect = painter.thumb_rect(400.0);
        assert_eq!(rect.right, 400.0);
        assert_eq!(rect.left, 400.0 - painter.thickness);
        assert_eq!(rect.top, 0.0);
        assert!((rect.bottom - painter.thumb_extent()).abs() < 0.01);
    }

    // -- The widget's refusals ---------------------------------------------------

    #[test]
    fn a_track_cannot_be_drawn_without_a_thumb() {
        // A groove with nothing in it says nothing.
        let mut widget = RawScrollbar::new();
        assert_eq!(widget.validate(), Ok(()));

        widget.thumb_visibility = Some(false);
        widget.track_visibility = Some(true);
        assert_eq!(
            widget.validate(),
            Err("A scrollbar track cannot be drawn without a scrollbar thumb.")
        );
    }

    #[test]
    fn the_overscroll_minimum_is_the_smaller_of_the_two() {
        let mut widget = RawScrollbar::new();
        widget.min_overscroll_length = Some(40.0);
        widget.min_thumb_length = 18.0;
        assert!(widget.validate().is_err());

        widget.min_overscroll_length = Some(8.0);
        assert_eq!(widget.validate(), Ok(()));
    }

    #[test]
    fn a_radius_and_a_shape_are_two_ways_to_say_the_same_thing() {
        // And there is no rule for which would win.
        let mut widget = RawScrollbar::new();
        widget.has_radius = true;
        assert_eq!(widget.validate(), Ok(()));
        widget.has_shape = true;
        assert!(widget.validate().is_err());
    }

    #[test]
    fn an_absent_overscroll_minimum_is_the_ordinary_one() {
        let widget = RawScrollbar::new();
        assert_eq!(
            widget.effective_min_overscroll_length(),
            widget.min_thumb_length
        );
    }

    // -- The state -----------------------------------------------------------------

    fn scrollbar_state() -> RawScrollbarState {
        RawScrollbarState::new(RawScrollbar::new(), painter())
    }

    #[test]
    fn a_scrollbar_asked_to_stay_never_starts_a_fadeout() {
        // The guard is the whole method.
        let mut transient = scrollbar_state();
        transient.maybe_start_fadeout_timer();
        assert!(transient.fadeout_pending());

        let mut permanent = RawScrollbarState::new(
            RawScrollbar {
                thumb_visibility: Some(true),
                ..RawScrollbar::new()
            },
            painter(),
        );
        permanent.maybe_start_fadeout_timer();
        assert!(!permanent.fadeout_pending());
    }

    #[test]
    fn the_scrollbar_goes_six_hundred_milliseconds_after_the_list_stops() {
        let mut state = scrollbar_state();
        state.maybe_start_fadeout_timer();
        state.advance_ms(599.0);
        assert!(!state.is_faded_out());

        state.advance_ms(1.0);
        assert!(state.is_faded_out());
    }

    #[test]
    fn scrolling_again_brings_it_back_and_restarts_the_clock() {
        let mut state = scrollbar_state();
        state.maybe_start_fadeout_timer();
        state.advance_ms(500.0);

        state.handle_scroll();
        assert!(!state.fadeout_pending());
        state.advance_ms(500.0);
        assert!(!state.is_faded_out(), "the old timer did not survive");
    }

    #[test]
    fn a_track_is_only_drawn_when_the_scrollbar_is_showing_and_was_asked_for() {
        let showing = RawScrollbarState::new(
            RawScrollbar {
                thumb_visibility: Some(true),
                track_visibility: Some(true),
                ..RawScrollbar::new()
            },
            painter(),
        );
        assert!(showing.shows_track());

        let transient = RawScrollbarState::new(
            RawScrollbar {
                track_visibility: Some(true),
                ..RawScrollbar::new()
            },
            painter(),
        );
        assert!(!transient.shows_track());
    }

    #[test]
    fn tapping_the_track_pages_towards_the_tap_rather_than_jumping_to_it() {
        // A jump would move the content further than the reader can follow;
        // the page is the unit they already know from the keyboard.
        let state = scrollbar_state();
        assert_eq!(
            state.handle_track_tap_down(300.0, 400.0, true),
            TrackTapOutcome::Paged {
                direction: AxisDirection::Down,
                by: 400.0
            }
        );

        let mut at_the_end = scrollbar_state();
        at_the_end
            .painter
            .update(metrics(1600.0), AxisDirection::Down);
        assert_eq!(
            at_the_end.handle_track_tap_down(10.0, 400.0, true),
            TrackTapOutcome::Paged {
                direction: AxisDirection::Up,
                by: -400.0
            }
        );
    }

    #[test]
    fn tapping_the_track_of_a_locked_scrollable_does_nothing() {
        let state = scrollbar_state();
        assert_eq!(
            state.handle_track_tap_down(300.0, 400.0, false),
            TrackTapOutcome::Refused
        );
    }

    #[test]
    fn a_scrollbar_over_touch_content_is_an_indicator_and_not_a_control() {
        let state = scrollbar_state();
        for platform in [
            ScrollPlatform::Android,
            ScrollPlatform::IOS,
            ScrollPlatform::Fuchsia,
        ] {
            assert!(!state.is_interactive(platform), "{platform:?}");
        }
        for platform in [
            ScrollPlatform::Linux,
            ScrollPlatform::MacOS,
            ScrollPlatform::Windows,
        ] {
            assert!(state.is_interactive(platform), "{platform:?}");
        }
    }

    #[test]
    fn saying_so_explicitly_beats_the_platform() {
        let state = RawScrollbarState::new(
            RawScrollbar {
                interactive: Some(true),
                ..RawScrollbar::new()
            },
            painter(),
        );
        assert!(state.is_interactive(ScrollPlatform::IOS));
    }

    #[test]
    fn an_orientation_names_the_axis_it_runs_along() {
        assert_eq!(ScrollbarOrientation::Left.axis(), Axis::Vertical);
        assert_eq!(ScrollbarOrientation::Right.axis(), Axis::Vertical);
        assert_eq!(ScrollbarOrientation::Top.axis(), Axis::Horizontal);
        assert_eq!(ScrollbarOrientation::Bottom.axis(), Axis::Horizontal);
    }
}
