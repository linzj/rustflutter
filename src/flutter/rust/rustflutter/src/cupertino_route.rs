//! The iOS page transition and the routes that use it -- a port of upstream's
//! `cupertino/route.dart`.
//!
//! The transition is a parallax: the arriving page slides a **full** screen
//! width in from the right, while the page it covers slides only **a third**
//! of one to the left. The mismatch is the effect. A page that slid the same
//! distance as its replacement would read as two pages on a conveyor belt
//! rather than one page lying under another.
//!
//! Three of the numbers in here are eyeballed rather than derived, and
//! upstream says so each time -- the 500ms page duration is "a relatively
//! rigorous eyeball estimation", the drop-off curve was "determined through
//! rigorously eyeballing native iOS animations", and the dialog's 1.3 initial
//! scale was "mostly eyeballed from iOS". They are copied here unchanged. A
//! transition that is nearly right is more obviously wrong than one that is
//! plainly different.
//!
//! ## What is not here
//!
//! These are `Route`s and `Widget`s upstream, driven by a `Navigator` and an
//! `AnimationController` this crate does not have -- see [`crate::routes`].
//! What is ported is the geometry, the durations and curves, and the
//! decisions: which way a released back-swipe goes, when a route may animate
//! at all, and what the shadow along the page edge is made of.

use crate::animation::Curve;
use crate::engine::Color;
use crate::physics::{SpringDescription, SpringSimulation, Tolerance};
use crate::render::Offset;
use crate::routes::{ModalRoute, PopupRoute, RawDialogRoute};

/// Upstream's `_kBackGestureWidth`: how wide the strip along the leading edge
/// that starts a back swipe is.
pub const BACK_GESTURE_WIDTH: f32 = 20.0;

/// Upstream's `_kMinFlingVelocity`, in **screen widths per second** rather
/// than pixels -- the whole back gesture works in logical coordinates, where
/// 0 is the new page dismissed and 1 is it on top, so a fling is measured
/// against the screen rather than against the device.
pub const MIN_FLING_VELOCITY: f32 = 1.0;

/// Upstream's `_kDroppedSwipePageAnimationDuration`: how long a released
/// back-swipe takes to finish going wherever it decided to go. Shorter than
/// the 500ms of a pushed page, because most of the distance is already behind
/// it.
pub const DROPPED_SWIPE_ANIMATION_MICROS: i64 = 350_000;

/// Upstream's `_kCupertinoPageTransitionBarrierColor`: barely visible, and
/// meant to be. It darkens the page underneath just enough to separate the two
/// during the slide.
pub const PAGE_TRANSITION_BARRIER_COLOR: Color = Color(0x1800_0000);

/// Upstream's `_kModalPopupTransitionDuration`.
pub const MODAL_POPUP_TRANSITION_MICROS: i64 = 335_000;

/// Upstream's `CupertinoRouteTransitionMixin.kTransitionDuration`, described
/// there as "a relatively rigorous eyeball estimation".
pub const PAGE_TRANSITION_MICROS: i64 = 500_000;

/// Upstream's `_kStandardSpring`, read off `CASpringAnimation` in Xcode.
///
/// The damping is not independent: `2 * sqrt(522.35) = 45.7099...`, which is
/// exactly critical. iOS's sheets do not overshoot, and a spring one notch
/// under critical would put a visible bounce on every action sheet.
pub const STANDARD_SPRING: SpringDescription = SpringDescription {
    mass: 1.0,
    stiffness: 522.35,
    damping: 45.709_955_2,
};

/// Upstream's `_kStandardTolerance`, and worth the note it carries.
///
/// iOS's spring settles in 0.404s, at which point its position is within 1e-3
/// of the end -- so the position tolerance matches the default. Its
/// **velocity** at that moment is still about 0.02, which says iOS is not
/// looking at velocity at all when it decides the animation is over. Upstream
/// widens the velocity tolerance to 0.03 rather than narrowing it, so the
/// animation ends when iOS's would instead of running on past it.
pub const STANDARD_TOLERANCE: Tolerance = Tolerance {
    distance: 1e-3,
    time: 1e-3,
    velocity: 0.03,
};

/// Upstream's `_kRightMiddleTween`: the arriving page, a full screen width.
pub const RIGHT_MIDDLE: (Offset, Offset) =
    (Offset { dx: 1.0, dy: 0.0 }, Offset { dx: 0.0, dy: 0.0 });

/// Upstream's `_kMiddleLeftTween`: the covered page, **one third** of a screen
/// width. The parallax is this fraction and nothing else.
pub const MIDDLE_LEFT: (Offset, Offset) = (
    Offset { dx: 0.0, dy: 0.0 },
    Offset {
        dx: -1.0 / 3.0,
        dy: 0.0,
    },
);

/// Upstream's `_kBottomUpTween`: a fullscreen dialog, straight up from the
/// bottom.
pub const BOTTOM_UP: (Offset, Offset) = (Offset { dx: 0.0, dy: 1.0 }, Offset { dx: 0.0, dy: 0.0 });

fn lerp_offset(from: Offset, to: Offset, t: f32) -> Offset {
    Offset {
        dx: from.dx + (to.dx - from.dx) * t,
        dy: from.dy + (to.dy - from.dy) * t,
    }
}

fn lerp_channel(from: u8, to: u8, t: f32) -> u8 {
    (from as f32 + (to as f32 - from as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

/// A straight per-channel blend. The crate has no `Color::lerp` yet and this
/// module needs one only for the edge shadow's bands.
fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    Color::argb(
        lerp_channel(from.alpha(), to.alpha(), t),
        lerp_channel(from.red(), to.red(), t),
        lerp_channel(from.green(), to.green(), t),
        lerp_channel(from.blue(), to.blue(), t),
    )
}

/// What [`CupertinoRouteTransitionMixin::can_transition_to`] needs to know
/// about the route arriving on top.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NextRouteFacts {
    pub is_page_route: bool,
    pub fullscreen_dialog: bool,
    pub is_cupertino: bool,
    pub has_delegated_transition: bool,
}

/// Upstream `CupertinoRouteTransitionMixin`: the iOS behaviour any `PageRoute`
/// can mix in.
///
/// The state it needs is one slot -- the previous route's title -- and the
/// nesting is deliberate: the **outer** `Option` is upstream's "not installed
/// on a Navigator yet", which it asserts against; the **inner** one is "the
/// previous route has no title, or is not a Cupertino route at all".
pub trait CupertinoRouteTransitionMixin {
    /// Upstream's `title`, used to fill in a navigation bar's middle when none
    /// was given.
    fn title(&self) -> Option<&str>;

    /// Upstream's `PageRoute.fullscreenDialog`.
    fn fullscreen_dialog(&self) -> bool;

    fn previous_title_slot(&self) -> &Option<Option<String>>;
    fn previous_title_slot_mut(&mut self) -> &mut Option<Option<String>>;

    /// Upstream's `previousTitle`, which **asserts** rather than returning
    /// null when the route has not been installed. Reading it early is a
    /// programming error, not a state a caller should branch on.
    fn previous_title(&self) -> Option<&str> {
        let slot = self
            .previous_title_slot()
            .as_ref()
            .expect("cannot read previousTitle for a route that has not been installed");
        slot.as_deref()
    }

    fn has_previous_title(&self) -> bool {
        self.previous_title_slot().is_some()
    }

    /// Upstream's `didChangePrevious`. The first call creates the notifier;
    /// later ones set its value, so anything listening survives a replacement
    /// of the route behind this one.
    fn did_change_previous(&mut self, previous_title: Option<String>) {
        match self.previous_title_slot_mut() {
            slot @ None => *slot = Some(previous_title),
            Some(value) => *value = previous_title,
        }
    }

    fn transition_duration_micros(&self) -> i64 {
        PAGE_TRANSITION_MICROS
    }

    /// A fullscreen dialog gets **no** barrier tint. It covers the screen
    /// outright, so there is nothing behind it left to separate it from.
    fn barrier_color(&self) -> Option<Color> {
        if self.fullscreen_dialog() {
            None
        } else {
            Some(PAGE_TRANSITION_BARRIER_COLOR)
        }
    }

    /// Upstream returns null: the page barrier is decoration, not a control,
    /// so there is nothing for a screen reader to announce.
    fn barrier_label(&self) -> Option<&str> {
        None
    }

    /// Upstream's `canTransitionTo`: may this route play its outgoing
    /// animation when `next` arrives?
    ///
    /// Two conditions, and both are about staying in step. A fullscreen dialog
    /// comes up from the bottom over everything, so a page sliding left
    /// underneath it would be moving for no reason. And the page only animates
    /// out if the arriving route will animate in on the same schedule --
    /// either because it is Cupertino too, or because it handed back a
    /// delegated transition to sync against.
    fn can_transition_to(&self, next: NextRouteFacts) -> bool {
        let next_is_not_fullscreen = !next.is_page_route || !next.fullscreen_dialog;
        next_is_not_fullscreen && (next.is_cupertino || next.has_delegated_transition)
    }

    /// Upstream's `canTransitionFrom`: may the route **underneath** animate
    /// when this one arrives? Not if this is a fullscreen dialog -- see above,
    /// from the other side.
    fn can_transition_from(&self, previous_is_page_route: bool) -> bool {
        previous_is_page_route && !self.fullscreen_dialog()
    }
}

/// Upstream `CupertinoPageRoute`: a modal route that replaces the screen with
/// an iOS transition.
#[derive(Debug, Clone, PartialEq)]
pub struct CupertinoPageRoute {
    pub modal: ModalRoute,
    pub title: Option<String>,
    pub fullscreen_dialog: bool,
    pub allow_snapshotting: bool,
    previous_title: Option<Option<String>>,
}

impl Default for CupertinoPageRoute {
    fn default() -> CupertinoPageRoute {
        CupertinoPageRoute::new()
    }
}

impl CupertinoPageRoute {
    pub fn new() -> CupertinoPageRoute {
        let mut modal = ModalRoute::new();
        modal.transition.transition_duration_micros = PAGE_TRANSITION_MICROS;
        modal.transition.reverse_transition_duration_micros = PAGE_TRANSITION_MICROS;
        modal.barrier_color = Some(PAGE_TRANSITION_BARRIER_COLOR);
        CupertinoPageRoute {
            modal,
            title: None,
            fullscreen_dialog: false,
            allow_snapshotting: true,
            previous_title: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_fullscreen_dialog(mut self, fullscreen: bool) -> Self {
        self.fullscreen_dialog = fullscreen;
        self.modal.barrier_color = if fullscreen {
            None
        } else {
            Some(PAGE_TRANSITION_BARRIER_COLOR)
        };
        self
    }

    /// Upstream's `install`, which is where `previousTitle` becomes readable.
    pub fn install(&mut self, previous_title: Option<String>) {
        self.did_change_previous(previous_title);
    }
}

impl CupertinoRouteTransitionMixin for CupertinoPageRoute {
    fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    fn fullscreen_dialog(&self) -> bool {
        self.fullscreen_dialog
    }

    fn previous_title_slot(&self) -> &Option<Option<String>> {
        &self.previous_title
    }

    fn previous_title_slot_mut(&mut self) -> &mut Option<Option<String>> {
        &mut self.previous_title
    }
}

/// Upstream `CupertinoPage`: the declarative form of [`CupertinoPageRoute`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CupertinoPage {
    pub title: Option<String>,
    pub maintain_state: bool,
    pub fullscreen_dialog: bool,
    pub allow_snapshotting: bool,
    pub can_pop: bool,
}

impl CupertinoPage {
    pub fn new() -> CupertinoPage {
        CupertinoPage {
            title: None,
            maintain_state: true,
            fullscreen_dialog: false,
            allow_snapshotting: true,
            can_pop: true,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Upstream's `createRoute`.
    pub fn create_route(&self) -> CupertinoPageRoute {
        let mut route = CupertinoPageRoute::new().with_fullscreen_dialog(self.fullscreen_dialog);
        route.title.clone_from(&self.title);
        route.allow_snapshotting = self.allow_snapshotting;
        route.modal.maintain_state = self.maintain_state;
        route.modal.page_can_pop = self.can_pop;
        route
    }
}

/// Upstream `CupertinoPageTransition`: the parallax slide, plus the shadow
/// down the arriving page's leading edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CupertinoPageTransition {
    /// 0 to 1 as this page is pushed.
    pub primary: f32,
    /// 0 to 1 as another page is pushed on top of it.
    pub secondary: f32,
    /// Upstream's `linearTransition`: whether to skip the curves entirely.
    ///
    /// It is true exactly while a back-swipe is in progress, and the reason is
    /// that the page has to sit under the reader's finger. An eased page would
    /// lag its own drag and then catch up, which reads as the page being stuck
    /// to the glass rather than to the finger.
    pub linear: bool,
}

impl CupertinoPageTransition {
    pub fn new(primary: f32, secondary: f32, linear: bool) -> CupertinoPageTransition {
        CupertinoPageTransition {
            primary,
            secondary,
            linear,
        }
    }

    /// The arriving page's curve. Upstream uses the same curve **flipped** for
    /// the reverse, rather than a separate ease-in, so a page dragged back
    /// retraces its own path.
    pub fn primary_curve(&self, reverse: bool) -> Option<Curve> {
        if self.linear {
            return None;
        }
        Some(if reverse {
            Curve::FAST_EASE_IN_TO_SLOW_EASE_OUT.flipped()
        } else {
            Curve::FAST_EASE_IN_TO_SLOW_EASE_OUT
        })
    }

    /// The covered page's curve, which is **not** the same shape as the
    /// arriving one: it eases out going forward and eases in coming back.
    pub fn secondary_curve(&self, reverse: bool) -> Option<Curve> {
        if self.linear {
            return None;
        }
        Some(if reverse {
            Curve::EASE_IN_TO_LINEAR
        } else {
            Curve::LINEAR_TO_EASE_OUT
        })
    }

    /// Where the arriving page sits, as a fraction of the screen width.
    pub fn primary_offset(&self, reverse: bool) -> Offset {
        let t = match self.primary_curve(reverse) {
            Some(curve) => curve.transform(self.primary),
            None => self.primary,
        };
        lerp_offset(RIGHT_MIDDLE.0, RIGHT_MIDDLE.1, t)
    }

    /// Where the covered page sits. A third of a screen, no more.
    pub fn secondary_offset(&self, reverse: bool) -> Offset {
        let t = match self.secondary_curve(reverse) {
            Some(curve) => curve.transform(self.secondary),
            None => self.secondary,
        };
        lerp_offset(MIDDLE_LEFT.0, MIDDLE_LEFT.1, t)
    }

    /// The shadow on the arriving page's leading edge, from nothing to
    /// [`CupertinoEdgeShadowDecoration::END`].
    pub fn shadow(&self) -> CupertinoEdgeShadowDecoration {
        let t = if self.linear {
            self.primary
        } else {
            Curve::LINEAR_TO_EASE_OUT.transform(self.primary)
        };
        CupertinoEdgeShadowDecoration::lerp(
            CupertinoEdgeShadowDecoration::NONE,
            CupertinoEdgeShadowDecoration::END,
            t,
        )
    }

    /// Upstream's `delegatedTransition`: the slide this route hands to the
    /// route **underneath** it, so a non-Cupertino page can still be shifted
    /// correctly while a Cupertino page covers it.
    pub fn delegated_transition(secondary: f32, reverse: bool) -> Offset {
        let curve = if reverse {
            Curve::EASE_IN_TO_LINEAR
        } else {
            Curve::LINEAR_TO_EASE_OUT
        };
        lerp_offset(MIDDLE_LEFT.0, MIDDLE_LEFT.1, curve.transform(secondary))
    }
}

/// Upstream `CupertinoFullscreenDialogTransition`: straight up from the
/// bottom, and no shadow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CupertinoFullscreenDialogTransition {
    pub primary: f32,
    pub secondary: f32,
    pub linear: bool,
}

impl CupertinoFullscreenDialogTransition {
    pub fn new(primary: f32, secondary: f32, linear: bool) -> CupertinoFullscreenDialogTransition {
        CupertinoFullscreenDialogTransition {
            primary,
            secondary,
            linear,
        }
    }

    /// The arriving dialog's curve.
    ///
    /// Note the reverse: upstream uses `linearToEaseOut.flipped()` and says
    /// why -- "the curve must be flipped so that the reverse animation doesn't
    /// play an ease-in curve, which iOS does not use". The flip is not
    /// symmetry for its own sake; it is the only way to get an ease-out on the
    /// way back out of a curve that eases out on the way in.
    ///
    /// And unlike [`CupertinoPageTransition`], the **primary** curve is
    /// applied even during a back swipe: a fullscreen dialog has no back
    /// gesture to track.
    pub fn primary_curve(&self, reverse: bool) -> Curve {
        if reverse {
            Curve::LINEAR_TO_EASE_OUT.flipped()
        } else {
            Curve::LINEAR_TO_EASE_OUT
        }
    }

    pub fn primary_offset(&self, reverse: bool) -> Offset {
        let t = self.primary_curve(reverse).transform(self.primary);
        lerp_offset(BOTTOM_UP.0, BOTTOM_UP.1, t)
    }

    /// The dialog still shifts whatever is under it a third of a screen left,
    /// exactly as a page would -- and this one **does** go linear during a
    /// swipe, because the route underneath may be the one being dragged.
    pub fn secondary_offset(&self, reverse: bool) -> Offset {
        let t = if self.linear {
            self.secondary
        } else if reverse {
            Curve::EASE_IN_TO_LINEAR.transform(self.secondary)
        } else {
            Curve::LINEAR_TO_EASE_OUT.transform(self.secondary)
        };
        lerp_offset(MIDDLE_LEFT.0, MIDDLE_LEFT.1, t)
    }
}

/// Upstream's `_CupertinoEdgeShadowDecoration`: the gradient down the leading
/// edge of an arriving page.
///
/// Private upstream, ported because the page transition is not the page
/// transition without it -- the shadow is what makes the covered page look
/// like it is underneath rather than beside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CupertinoEdgeShadowDecoration {
    /// `None` means no shadow at all, which is what the tween starts from.
    pub colors: Option<[Color; 2]>,
}

impl CupertinoEdgeShadowDecoration {
    /// The tween's start: nothing.
    pub const NONE: CupertinoEdgeShadowDecoration = CupertinoEdgeShadowDecoration { colors: None };

    /// The tween's end. `0x04000000` is a **1.6% black** -- barely a shadow at
    /// all, which is the point: it has to read as depth without reading as a
    /// line.
    pub const END: CupertinoEdgeShadowDecoration = CupertinoEdgeShadowDecoration {
        colors: Some([Color(0x0400_0000), Color(0x0000_0000)]),
    };

    /// The shadow spans 5% of the page width.
    pub const WIDTH_FRACTION: f32 = 0.05;

    /// Upstream's `lerp`. Interpolating from "no shadow" fades the colours in
    /// from fully transparent rather than snapping the gradient on.
    pub fn lerp(
        from: CupertinoEdgeShadowDecoration,
        to: CupertinoEdgeShadowDecoration,
        t: f32,
    ) -> CupertinoEdgeShadowDecoration {
        match (from.colors, to.colors) {
            (None, None) => CupertinoEdgeShadowDecoration::NONE,
            (Some(a), Some(b)) => CupertinoEdgeShadowDecoration {
                colors: Some([lerp_color(a[0], b[0], t), lerp_color(a[1], b[1], t)]),
            },
            (None, Some(b)) => CupertinoEdgeShadowDecoration {
                colors: Some([
                    lerp_color(b[0].with_alpha(0), b[0], t),
                    lerp_color(b[1].with_alpha(0), b[1], t),
                ]),
            },
            (Some(a), None) => CupertinoEdgeShadowDecoration {
                colors: Some([
                    lerp_color(a[0], a[0].with_alpha(0), t),
                    lerp_color(a[1], a[1].with_alpha(0), t),
                ]),
            },
        }
    }

    /// The colour of the 1-pixel band at `dx` pixels in from the edge, given
    /// the page width.
    ///
    /// **This is drawn as a stack of 1px rectangles rather than as a
    /// `LinearGradient`, on purpose.** Upstream measured it on an iPhone XR in
    /// February 2021: compiling the gradient's shader took long enough that
    /// the worst frame of a page transition in a freshly installed app was
    /// ~95ms, and drawing the bands by hand brought it to ~30ms. The slower
    /// -- looking code is the faster one, because the cost was never in the
    /// drawing.
    pub fn band_color(&self, dx: f32, page_width: f32) -> Option<Color> {
        let colors = self.colors?;
        let shadow_width = Self::WIDTH_FRACTION * page_width;
        if dx < 0.0 || dx >= shadow_width {
            return None;
        }
        let band_width = shadow_width / (colors.len() - 1) as f32;
        let index = ((dx / band_width) as usize).min(colors.len() - 2);
        let within = (dx % band_width) / band_width;
        Some(lerp_color(colors[index], colors[index + 1], within))
    }

    /// Which way the bands march from the starting edge: leftwards in LTR,
    /// rightwards in RTL. The shadow is on the *leading* edge, so it swaps
    /// with the reading direction along with everything else.
    pub fn shadow_direction(is_rtl: bool) -> f32 {
        if is_rtl { 1.0 } else { -1.0 }
    }
}

/// Which way a released back-swipe goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackSwipeOutcome {
    /// Back to a fully presented page: the swipe is abandoned.
    Restore,
    /// Off the screen: the route pops.
    Dismiss,
}

/// Upstream's `_CupertinoBackGestureController.dragEnd` decision, on its own.
///
/// Three cases, and the **first one is the interesting one**: if the route is
/// no longer current, the answer comes from whether it is still in the stack
/// and nothing else -- not the velocity, not how far the drag got. Upstream
/// cites flutter/flutter#141268 for it: a route being nudged back by a few
/// pixels when a programmatic pop lands should still leave, because it has
/// already been popped. Asking the finger at that point would put a
/// half-dismissed page back on screen that nothing owns any more.
///
/// After that: a fling decides by direction alone, however far the drag got,
/// and only a slow release falls back to "past halfway stays".
pub fn back_swipe_outcome(
    is_current: bool,
    is_active: bool,
    velocity: f32,
    progress: f32,
) -> BackSwipeOutcome {
    let restore = if !is_current {
        is_active
    } else if velocity.abs() >= MIN_FLING_VELOCITY {
        // Positive velocity is a drag towards the trailing edge -- the page
        // being pushed away -- so it is a dismissal, and only a fling back the
        // other way restores.
        velocity <= 0.0
    } else {
        progress > 0.5
    };
    if restore {
        BackSwipeOutcome::Restore
    } else {
        BackSwipeOutcome::Dismiss
    }
}

/// Upstream's `_CupertinoBackGestureController`, which owns the drag once one
/// starts.
///
/// It works entirely in logical coordinates: 0 is the page dismissed, 1 is the
/// page on top, and the detector divides by the screen width and flips the
/// sign in RTL before anything gets here. That is why
/// [`MIN_FLING_VELOCITY`] is 1.0 and means "one screen width per second".
#[derive(Debug, Clone, PartialEq)]
pub struct CupertinoBackGestureController {
    pub progress: f32,
    /// Upstream keeps `userGestureInProgress` true until the settling
    /// animation **finishes**, not until the finger lifts. The transition
    /// reads that flag to decide whether to go linear, so dropping it early
    /// would change the curve halfway through the settle -- a visible kink in
    /// the middle of a page sliding home.
    pub user_gesture_in_progress: bool,
    outcome: Option<BackSwipeOutcome>,
}

impl CupertinoBackGestureController {
    pub fn new(progress: f32) -> CupertinoBackGestureController {
        CupertinoBackGestureController {
            progress,
            user_gesture_in_progress: true,
            outcome: None,
        }
    }

    /// Upstream's `dragUpdate`, which **subtracts**: dragging towards the
    /// trailing edge takes the page away, so a positive delta lowers the
    /// animation value.
    pub fn drag_update(&mut self, delta: f32) {
        self.progress = (self.progress - delta).clamp(0.0, 1.0);
    }

    /// Upstream's `dragEnd`.
    pub fn drag_end(
        &mut self,
        velocity: f32,
        is_current: bool,
        is_active: bool,
    ) -> BackSwipeOutcome {
        let outcome = back_swipe_outcome(is_current, is_active, velocity, self.progress);
        self.outcome = Some(outcome);
        outcome
    }

    /// Upstream's `_handleDragCancel`, which ends the drag **as if it were
    /// released with no velocity at all**. A cancel is not a separate
    /// decision: wherever the page got to is the answer.
    pub fn drag_cancel(&mut self, is_current: bool, is_active: bool) -> BackSwipeOutcome {
        self.drag_end(0.0, is_current, is_active)
    }

    /// The settle finished; only now does the gesture stop counting as in
    /// progress.
    pub fn settle_completed(&mut self) {
        self.user_gesture_in_progress = false;
    }

    pub fn outcome(&self) -> Option<BackSwipeOutcome> {
        self.outcome
    }

    pub fn settle_duration_micros(&self) -> i64 {
        DROPPED_SWIPE_ANIMATION_MICROS
    }
}

/// How wide the strip that starts a back swipe is, on a device whose leading
/// edge has a notch or a rounded corner eating into it.
///
/// Upstream takes the **larger** of the safe-area inset and the flat 20: the
/// gesture has to stay reachable, and on a device where the system already
/// keeps a wide margin on that side, 20 logical pixels would put the whole
/// strip under the hardware.
pub fn back_gesture_area_width(leading_safe_area_inset: f32) -> f32 {
    leading_safe_area_inset.max(BACK_GESTURE_WIDTH)
}

/// Upstream's `kCupertinoModalBarrierColor`, the light one. Upstream declares
/// it as a `CupertinoDynamicColor` with a dark variant; only the light value
/// is carried here, since this crate has no dynamic colour resolution yet.
pub const MODAL_BARRIER_COLOR: Color = Color(0x33_00_00_00);

/// Upstream `CupertinoModalPopupRoute`: the sheet that slides up from the
/// bottom.
#[derive(Debug, Clone, PartialEq)]
pub struct CupertinoModalPopupRoute {
    pub popup: PopupRoute,
    /// Upstream's `semanticsDismissible`, **false** by default where
    /// `barrierDismissible` is true. The barrier can be tapped, but it is left
    /// out of the semantics tree unless asked for: a screen reader user
    /// dismisses the sheet through its own controls, and an extra full-screen
    /// "Dismiss" target in the tree gets in the way of reaching them.
    pub semantics_dismissible: bool,
}

impl Default for CupertinoModalPopupRoute {
    fn default() -> CupertinoModalPopupRoute {
        CupertinoModalPopupRoute::new()
    }
}

impl CupertinoModalPopupRoute {
    pub fn new() -> CupertinoModalPopupRoute {
        let mut popup = PopupRoute::new();
        popup.modal.barrier_dismissible = true;
        popup.modal.barrier_label = Some("Dismiss".to_string());
        popup.modal.barrier_color = Some(MODAL_BARRIER_COLOR);
        popup.modal.transition.transition_duration_micros = MODAL_POPUP_TRANSITION_MICROS;
        popup.modal.transition.reverse_transition_duration_micros = MODAL_POPUP_TRANSITION_MICROS;
        CupertinoModalPopupRoute {
            popup,
            semantics_dismissible: false,
        }
    }

    /// Upstream's `createSimulation`: a spring, not a curve.
    ///
    /// A curve has a fixed duration and a sheet interrupted halfway has to
    /// start a new one; a spring carries the position it was already at, so a
    /// sheet grabbed on the way up keeps moving from where it was.
    pub fn create_simulation(&self, from: f32, forward: bool) -> SpringSimulation {
        SpringSimulation::with_tolerance(
            STANDARD_SPRING,
            from,
            if forward { 1.0 } else { 0.0 },
            0.0,
            STANDARD_TOLERANCE,
        )
    }

    /// Where the sheet sits, as a fraction of its own height.
    pub fn offset(&self, animation: f32) -> Offset {
        lerp_offset(BOTTOM_UP.0, BOTTOM_UP.1, animation)
    }
}

/// Upstream `CupertinoDialogRoute`: an iOS alert.
#[derive(Debug, Clone, PartialEq)]
pub struct CupertinoDialogRoute {
    pub dialog: RawDialogRoute,
}

impl Default for CupertinoDialogRoute {
    fn default() -> CupertinoDialogRoute {
        CupertinoDialogRoute::new()
    }
}

impl CupertinoDialogRoute {
    /// Upstream's default, "eyeballed comparing with iOS": 250ms, between the
    /// 200 of a Material dialog and the 335 of an action sheet.
    pub const TRANSITION_MICROS: i64 = 250_000;

    /// Upstream's `_dialogScaleTween`, from **1.3**: the alert starts larger
    /// than it ends and settles into place, which is what iOS does. Growing
    /// into place from below one would read as something arriving from far
    /// away rather than something already in front of you.
    pub const SCALE_BEGIN: f32 = 1.3;
    pub const SCALE_END: f32 = 1.0;

    pub fn new() -> CupertinoDialogRoute {
        let dialog = RawDialogRoute::new()
            .with_transition_duration(Self::TRANSITION_MICROS)
            .with_barrier_color(Some(MODAL_BARRIER_COLOR));
        CupertinoDialogRoute { dialog }
    }

    /// The same spring as the action sheet.
    pub fn create_simulation(&self, from: f32, forward: bool) -> SpringSimulation {
        SpringSimulation::with_tolerance(
            STANDARD_SPRING,
            from,
            if forward { 1.0 } else { 0.0 },
            0.0,
            STANDARD_TOLERANCE,
        )
    }

    pub fn opacity(&self, animation: f32) -> f32 {
        animation
    }

    /// The scale, and the asymmetry worth naming: **going out, there is
    /// none**. Upstream returns a plain fade when the animation is reversing.
    /// An alert that grew back to 1.3 while fading would look like it was
    /// coming towards the reader as it left.
    pub fn scale(&self, animation: f32, reverse: bool) -> f32 {
        if reverse {
            return Self::SCALE_END;
        }
        Self::SCALE_BEGIN + (Self::SCALE_END - Self::SCALE_BEGIN) * animation
    }
}

/// Upstream `CupertinoPageTransitionsBuilder`: the iOS entry in a
/// `PageTransitionsTheme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CupertinoPageTransitionsBuilder;

impl CupertinoPageTransitionsBuilder {
    pub const fn new() -> CupertinoPageTransitionsBuilder {
        CupertinoPageTransitionsBuilder
    }

    pub fn transition_duration_micros(&self) -> i64 {
        PAGE_TRANSITION_MICROS
    }

    /// Upstream's `buildTransitions`, which dispatches on one thing: a
    /// fullscreen dialog comes up from the bottom, everything else slides in
    /// from the side. And the side one is the only one wrapped in a back
    /// gesture detector, because there is no edge swipe for a dialog.
    pub fn is_fullscreen_dialog_transition(&self, route: &CupertinoPageRoute) -> bool {
        route.fullscreen_dialog
    }

    /// Upstream's `delegatedTransition`, handed to the route underneath.
    pub fn delegated_transition(&self, secondary: f32, reverse: bool) -> Offset {
        CupertinoPageTransition::delegated_transition(secondary, reverse)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::Simulation;

    // -- The parallax ------------------------------------------------------

    #[test]
    fn the_covered_page_moves_a_third_as_far_as_the_one_covering_it() {
        // The mismatch is the whole effect. Two pages sliding the same
        // distance read as a conveyor belt, not as one page under another.
        let transition = CupertinoPageTransition::new(1.0, 1.0, true);
        assert_eq!(transition.primary_offset(false).dx, 0.0, "fully arrived");
        assert!(
            (transition.secondary_offset(false).dx + 1.0 / 3.0).abs() < 1e-6,
            "and the page under it has gone a third of a screen"
        );

        let start = CupertinoPageTransition::new(0.0, 0.0, true);
        assert_eq!(start.primary_offset(false).dx, 1.0, "a full screen away");
    }

    #[test]
    fn a_page_being_dragged_tracks_the_finger_exactly() {
        // An eased page would lag the drag and then catch up, which reads as
        // the page being stuck to the glass rather than to the finger.
        let dragged = CupertinoPageTransition::new(0.25, 0.0, true);
        assert!(dragged.primary_curve(false).is_none());
        assert_eq!(dragged.primary_offset(false).dx, 0.75);

        let free = CupertinoPageTransition::new(0.25, 0.0, false);
        assert!(free.primary_curve(false).is_some());
        assert_ne!(
            free.primary_offset(false).dx,
            0.75,
            "where a page animating on its own is eased"
        );
    }

    #[test]
    fn the_arriving_page_and_the_covered_one_are_not_given_the_same_curve() {
        let transition = CupertinoPageTransition::new(0.5, 0.5, false);
        assert_eq!(
            transition.primary_curve(false),
            Some(Curve::FAST_EASE_IN_TO_SLOW_EASE_OUT)
        );
        assert_eq!(
            transition.secondary_curve(false),
            Some(Curve::LINEAR_TO_EASE_OUT)
        );
        assert_eq!(
            transition.secondary_curve(true),
            Some(Curve::EASE_IN_TO_LINEAR),
            "and coming back it eases in rather than flipping the other curve"
        );
    }

    #[test]
    fn a_fullscreen_dialog_is_eased_even_while_something_is_being_dragged() {
        // Where a page transition goes linear for exactly that case. A
        // fullscreen dialog has no edge swipe of its own, so there is no
        // finger for its arrival to track.
        let dialog = CupertinoFullscreenDialogTransition::new(0.25, 0.25, true);
        assert_ne!(
            dialog.primary_offset(false).dy,
            0.75,
            "the curve is applied regardless"
        );
        assert_eq!(
            dialog.secondary_offset(false).dx,
            CupertinoFullscreenDialogTransition::new(0.25, 0.25, true)
                .secondary_offset(false)
                .dx
        );
        assert_eq!(
            dialog.secondary_offset(false).dx,
            -0.25 / 3.0,
            "but the route underneath it does go linear, because that one may be the drag"
        );
    }

    #[test]
    fn a_fullscreen_dialog_leaves_the_way_it_came_rather_than_easing_in() {
        // Upstream flips the curve and says why: iOS does not use an ease-in
        // for this, so the reverse has to be the same shape run backwards.
        let dialog = CupertinoFullscreenDialogTransition::new(0.5, 0.0, false);
        assert_eq!(dialog.primary_curve(false), Curve::LINEAR_TO_EASE_OUT);
        assert_eq!(
            dialog.primary_curve(true),
            Curve::LINEAR_TO_EASE_OUT.flipped()
        );
        assert_ne!(
            dialog.primary_curve(true),
            Curve::EASE_IN_TO_LINEAR,
            "and it is not the other named curve, close as that looks"
        );
    }

    #[test]
    fn a_dialog_goes_up_and_a_page_goes_sideways() {
        let dialog = CupertinoFullscreenDialogTransition::new(0.0, 0.0, true);
        assert_eq!(dialog.primary_offset(false), Offset { dx: 0.0, dy: 1.0 });

        let page = CupertinoPageTransition::new(0.0, 0.0, true);
        assert_eq!(page.primary_offset(false), Offset { dx: 1.0, dy: 0.0 });
    }

    // -- When a route may animate at all -----------------------------------

    #[test]
    fn nothing_slides_out_from_under_a_fullscreen_dialog() {
        // It covers everything on its way up, so a page moving underneath it
        // would be moving for no reason.
        let route = CupertinoPageRoute::new();
        assert!(!route.can_transition_to(NextRouteFacts {
            is_page_route: true,
            fullscreen_dialog: true,
            is_cupertino: true,
            has_delegated_transition: false,
        }));
    }

    #[test]
    fn a_page_animates_out_only_for_something_it_can_stay_in_step_with() {
        let route = CupertinoPageRoute::new();
        let cupertino = NextRouteFacts {
            is_page_route: true,
            is_cupertino: true,
            ..NextRouteFacts::default()
        };
        assert!(route.can_transition_to(cupertino));

        let delegating = NextRouteFacts {
            is_page_route: true,
            has_delegated_transition: true,
            ..NextRouteFacts::default()
        };
        assert!(
            route.can_transition_to(delegating),
            "a route that handed back a transition to sync against"
        );

        let stranger = NextRouteFacts {
            is_page_route: true,
            ..NextRouteFacts::default()
        };
        assert!(
            !route.can_transition_to(stranger),
            "and one that offered neither is left alone"
        );
    }

    #[test]
    fn a_fullscreen_dialog_suppresses_the_route_beneath_it_from_the_other_side() {
        assert!(CupertinoPageRoute::new().can_transition_from(true));
        assert!(
            !CupertinoPageRoute::new()
                .with_fullscreen_dialog(true)
                .can_transition_from(true)
        );
        assert!(
            !CupertinoPageRoute::new().can_transition_from(false),
            "and nothing that is not a page route transitions either"
        );
    }

    #[test]
    fn a_fullscreen_dialog_has_no_barrier_to_tint() {
        // It covers the screen outright, so there is nothing left behind it to
        // separate it from.
        assert_eq!(
            CupertinoPageRoute::new().barrier_color(),
            Some(Color(0x1800_0000))
        );
        assert_eq!(
            CupertinoPageRoute::new()
                .with_fullscreen_dialog(true)
                .barrier_color(),
            None
        );
        assert_eq!(
            CupertinoPageRoute::new().barrier_label(),
            None,
            "the page barrier is decoration, not a control"
        );
    }

    // -- The previous title ------------------------------------------------

    #[test]
    fn the_previous_title_survives_the_route_behind_it_being_replaced() {
        let mut route = CupertinoPageRoute::new().with_title("Details");
        assert!(!route.has_previous_title());

        route.install(Some("Inbox".to_string()));
        assert_eq!(route.previous_title(), Some("Inbox"));

        route.did_change_previous(None);
        assert_eq!(
            route.previous_title(),
            None,
            "the slot stays; only its value changed"
        );
        assert!(
            route.has_previous_title(),
            "so a listener is still attached"
        );
    }

    #[test]
    #[should_panic(expected = "has not been installed")]
    fn reading_the_previous_title_too_early_is_a_mistake_and_not_a_state() {
        let _ = CupertinoPageRoute::new().previous_title();
    }

    // -- The back swipe ----------------------------------------------------

    #[test]
    fn a_route_already_popped_leaves_however_the_finger_was_moving() {
        // flutter/flutter#141268: a route being nudged back by a few pixels
        // when a programmatic pop lands should still go, because it has
        // already been popped. Asking the finger would put a half-dismissed
        // page back on screen that nothing owns any more.
        assert_eq!(
            back_swipe_outcome(false, false, -50.0, 0.99),
            BackSwipeOutcome::Dismiss,
            "flung hard back in, nearly fully present -- and it still leaves"
        );
        assert_eq!(
            back_swipe_outcome(false, true, 50.0, 0.01),
            BackSwipeOutcome::Restore,
            "and still in the stack means it comes back, however hard it was pushed away"
        );
    }

    #[test]
    fn a_fling_decides_by_direction_alone_however_far_it_got() {
        // Released before mid screen with enough speed, the page still goes.
        assert_eq!(
            back_swipe_outcome(true, true, 2.0, 0.9),
            BackSwipeOutcome::Dismiss,
            "flung away from nine tenths present"
        );
        assert_eq!(
            back_swipe_outcome(true, true, -2.0, 0.1),
            BackSwipeOutcome::Restore,
            "and flung back from one tenth"
        );
    }

    #[test]
    fn a_slow_release_falls_back_to_which_side_of_the_middle_it_is_on() {
        assert_eq!(
            back_swipe_outcome(true, true, 0.5, 0.51),
            BackSwipeOutcome::Restore
        );
        assert_eq!(
            back_swipe_outcome(true, true, 0.5, 0.5),
            BackSwipeOutcome::Dismiss,
            "exactly half is not past half"
        );
        assert_eq!(
            back_swipe_outcome(true, true, 0.999, 0.4),
            BackSwipeOutcome::Dismiss,
            "just under the fling threshold, so position decides"
        );
        assert_eq!(
            back_swipe_outcome(true, true, 1.0, 0.4),
            BackSwipeOutcome::Dismiss,
            "and at the threshold direction decides, which agrees here"
        );
        assert_eq!(
            back_swipe_outcome(true, true, -1.0, 0.4),
            BackSwipeOutcome::Restore,
            "where it disagrees, direction wins"
        );
    }

    #[test]
    fn dragging_towards_the_trailing_edge_takes_the_page_away() {
        let mut controller = CupertinoBackGestureController::new(1.0);
        controller.drag_update(0.3);
        assert!((controller.progress - 0.7).abs() < 1e-6, "subtracted");

        controller.drag_update(-0.1);
        assert!((controller.progress - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_cancelled_drag_is_a_release_with_no_velocity() {
        // Wherever the page got to is the answer; a cancel is not a separate
        // decision.
        let mut past_half = CupertinoBackGestureController::new(0.6);
        assert_eq!(past_half.drag_cancel(true, true), BackSwipeOutcome::Restore);

        let mut short_of_half = CupertinoBackGestureController::new(0.4);
        assert_eq!(
            short_of_half.drag_cancel(true, true),
            BackSwipeOutcome::Dismiss
        );
    }

    #[test]
    fn the_gesture_counts_as_in_progress_until_the_settle_ends() {
        // The transition reads this to decide whether to go linear, so
        // dropping it when the finger lifts would put a kink in the middle of
        // the page sliding home.
        let mut controller = CupertinoBackGestureController::new(0.8);
        controller.drag_end(0.0, true, true);
        assert!(
            controller.user_gesture_in_progress,
            "the finger is gone but the page is still moving"
        );

        controller.settle_completed();
        assert!(!controller.user_gesture_in_progress);
        assert_eq!(controller.outcome(), Some(BackSwipeOutcome::Restore));
    }

    #[test]
    fn the_swipe_strip_widens_to_clear_a_notch() {
        // Twenty logical pixels on a device whose system margin is wider would
        // put the whole strip under the hardware.
        assert_eq!(back_gesture_area_width(0.0), 20.0);
        assert_eq!(back_gesture_area_width(44.0), 44.0);
        assert_eq!(back_gesture_area_width(8.0), 20.0);
    }

    #[test]
    fn a_dropped_swipe_settles_faster_than_a_page_arrives() {
        // Most of the distance is already behind it.
        assert_eq!(
            CupertinoBackGestureController::new(0.5).settle_duration_micros(),
            350_000
        );
        assert_eq!(PAGE_TRANSITION_MICROS, 500_000);
    }

    // -- The edge shadow ---------------------------------------------------

    #[test]
    fn the_shadow_fades_in_from_nothing_rather_than_snapping_on() {
        let none = CupertinoEdgeShadowDecoration::NONE;
        assert!(none.band_color(0.0, 400.0).is_none());

        let half =
            CupertinoEdgeShadowDecoration::lerp(none, CupertinoEdgeShadowDecoration::END, 0.5);
        let full = CupertinoEdgeShadowDecoration::END;
        let half_alpha = half.band_color(0.0, 400.0).unwrap().alpha();
        let full_alpha = full.band_color(0.0, 400.0).unwrap().alpha();
        assert!(
            half_alpha < full_alpha && half_alpha > 0,
            "half of a shadow, not none and not all of it"
        );
    }

    #[test]
    fn the_shadow_spans_a_twentieth_of_the_page_and_stops() {
        let shadow = CupertinoEdgeShadowDecoration::END;
        assert!(shadow.band_color(0.0, 400.0).is_some());
        assert!(shadow.band_color(19.9, 400.0).is_some(), "inside 5% of 400");
        assert!(
            shadow.band_color(20.0, 400.0).is_none(),
            "and nothing beyond it"
        );
        assert!(shadow.band_color(-1.0, 400.0).is_none());
    }

    #[test]
    fn the_shadow_is_darkest_at_the_edge_and_gone_by_the_far_side() {
        // It is a drop shadow, so the page it belongs to is what casts it.
        let shadow = CupertinoEdgeShadowDecoration::END;
        let at_edge = shadow.band_color(0.0, 400.0).unwrap();
        let near_end = shadow.band_color(19.0, 400.0).unwrap();
        assert_eq!(at_edge.alpha(), 4, "0x04: barely a shadow at all");
        assert!(near_end.alpha() < at_edge.alpha());
    }

    #[test]
    fn the_shadow_marches_the_other_way_when_the_reading_does() {
        // It is on the leading edge, which swaps with everything else.
        assert_eq!(CupertinoEdgeShadowDecoration::shadow_direction(false), -1.0);
        assert_eq!(CupertinoEdgeShadowDecoration::shadow_direction(true), 1.0);
    }

    // -- The popup and the dialog -----------------------------------------

    #[test]
    fn a_sheet_grabbed_mid_flight_carries_the_position_it_was_already_at() {
        // Which is why these are springs and not curves: a curve has a fixed
        // duration and would have to start over.
        let route = CupertinoModalPopupRoute::new();
        let interrupted = route.create_simulation(0.62, true);
        assert!((interrupted.x(0.0) - 0.62).abs() < 1e-4);
        assert!(interrupted.x(0.5) > 0.99, "and it still gets there");
        assert!(interrupted.is_done(0.5));
    }

    #[test]
    fn the_spring_is_critically_damped_so_no_sheet_ever_bounces() {
        // damping = 2 * sqrt(stiffness), to the precision upstream carries.
        let critical = 2.0 * STANDARD_SPRING.stiffness.sqrt();
        assert!(
            (STANDARD_SPRING.damping - critical).abs() < 1e-4,
            "{} against {critical}",
            STANDARD_SPRING.damping
        );

        let simulation = CupertinoModalPopupRoute::new().create_simulation(0.0, true);
        for step in 0..60 {
            let t = step as f32 / 100.0;
            assert!(
                simulation.x(t) <= 1.0 + 1e-4,
                "overshot to {} at {t}",
                simulation.x(t)
            );
        }
    }

    #[test]
    fn the_velocity_tolerance_is_widened_rather_than_narrowed() {
        // iOS's own spring is still moving at about 0.02 when it calls itself
        // finished, so a default 1e-3 would keep animating past the point iOS
        // stops.
        assert_eq!(STANDARD_TOLERANCE.velocity, 0.03);
        assert_eq!(STANDARD_TOLERANCE.distance, Tolerance::DEFAULT.distance);
        assert!(STANDARD_TOLERANCE.velocity > Tolerance::DEFAULT.velocity);
    }

    #[test]
    fn an_alert_settles_into_place_rather_than_growing_into_it() {
        let route = CupertinoDialogRoute::new();
        assert_eq!(route.scale(0.0, false), 1.3);
        assert_eq!(route.scale(1.0, false), 1.0);
        assert!(route.scale(0.5, false) > 1.0, "always coming down to size");
    }

    #[test]
    fn an_alert_leaving_only_fades() {
        // Growing back to 1.3 while fading would look like it was coming
        // towards the reader as it left.
        let route = CupertinoDialogRoute::new();
        assert_eq!(route.scale(0.5, true), 1.0);
        assert_eq!(route.scale(0.0, true), 1.0);
        assert_eq!(route.opacity(0.5), 0.5, "the fade is all of it");
    }

    #[test]
    fn each_of_the_three_has_its_own_eyeballed_duration() {
        assert_eq!(CupertinoDialogRoute::TRANSITION_MICROS, 250_000);
        assert_eq!(MODAL_POPUP_TRANSITION_MICROS, 335_000);
        assert_eq!(PAGE_TRANSITION_MICROS, 500_000);
    }

    #[test]
    fn a_sheet_may_be_tapped_away_but_is_not_offered_to_a_screen_reader() {
        // A reader gets out through the sheet's own controls, and a
        // full-screen Dismiss target in the tree sits in front of them.
        let route = CupertinoModalPopupRoute::new();
        assert!(route.popup.modal.barrier_dismissible);
        assert!(!route.semantics_dismissible);
        assert_eq!(route.popup.modal.barrier_label.as_deref(), Some("Dismiss"));
        assert!(
            !route.popup.opaque(),
            "and the page behind it stays visible"
        );
    }

    #[test]
    fn a_sheet_arrives_from_below_and_stops_at_the_bottom_edge() {
        let route = CupertinoModalPopupRoute::new();
        assert_eq!(route.offset(0.0), Offset { dx: 0.0, dy: 1.0 });
        assert_eq!(route.offset(1.0), Offset { dx: 0.0, dy: 0.0 });
    }

    // -- The page and the builder -----------------------------------------

    #[test]
    fn a_page_hands_its_answers_to_the_route_it_creates() {
        let page = CupertinoPage {
            title: Some("Compose".to_string()),
            maintain_state: false,
            fullscreen_dialog: true,
            allow_snapshotting: false,
            can_pop: false,
        };
        let route = page.create_route();
        assert_eq!(route.title.as_deref(), Some("Compose"));
        assert!(route.fullscreen_dialog);
        assert!(!route.modal.maintain_state);
        assert!(!route.modal.page_can_pop);
        assert!(!route.allow_snapshotting);
        assert_eq!(
            route.barrier_color(),
            None,
            "and being a fullscreen dialog followed it across"
        );
    }

    #[test]
    fn the_builder_dispatches_on_the_one_thing_that_changes_the_animation() {
        let builder = CupertinoPageTransitionsBuilder::new();
        assert!(!builder.is_fullscreen_dialog_transition(&CupertinoPageRoute::new()));
        assert!(builder.is_fullscreen_dialog_transition(
            &CupertinoPageRoute::new().with_fullscreen_dialog(true)
        ));
        assert_eq!(builder.transition_duration_micros(), PAGE_TRANSITION_MICROS);
    }

    #[test]
    fn the_delegated_transition_is_the_same_shift_a_cupertino_page_would_get() {
        // Which is the point of handing it over: a Material page covered by a
        // Cupertino one still moves the way iOS expects.
        let builder = CupertinoPageTransitionsBuilder::new();
        let delegated = builder.delegated_transition(0.5, false);
        let native = CupertinoPageTransition::new(0.0, 0.5, false).secondary_offset(false);
        assert_eq!(delegated, native);
    }
}
