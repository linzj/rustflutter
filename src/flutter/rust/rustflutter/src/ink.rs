// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The splash a tap leaves behind.
//!
//! Upstream this is `InkWell` over `InkRipple`, painted by the `Material` the
//! tap landed on. It is the one piece of Material that is not decoration: it
//! is the acknowledgement. A button that only changes colour tells the reader
//! it is pressed; a splash tells them *where* they pressed and that the press
//! was received, which is the difference between an interface that feels
//! connected to the finger and one that feels like it is deciding whether to
//! believe you.
//!
//! # Timings
//!
//! Upstream's, from `ink_ripple.dart`: the radius grows over a second while the
//! press is unconfirmed, the colour fades in over 75ms, and on release the
//! splash fades out over 375ms. The radius it grows towards is the distance to
//! the furthest corner of the box from the point that was touched -- which is
//! what makes a tap near an edge still fill the whole thing.
//!
//! # One thing to know about the live region
//!
//! That target radius is [`InkSplash`]'s rule, not [`InkRipple`]'s: a ripple
//! grows to *half the box's diagonal*, the same size wherever it is touched.
//! The [`Ink`] region below therefore runs `InkSplash`'s target with
//! `InkRipple`'s curves, a combination upstream does not have. It is left as
//! it is rather than changed here, because the change is a visual one and the
//! two are now written down side by side; the one line is [`Ink`]'s call to
//! [`splash_radius`], which becomes [`InkRipple::target_radius`] if the ripple
//! is what is wanted.

use std::cell::Cell;
use std::rc::Rc;

use crate::animation::Curve;
use crate::components::theme_of;
use crate::engine::Color;
use crate::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent, single, stateful};
use crate::gestures::PointerHandlers;
use crate::render::{Alignment, Offset, Size, StackPosition};

/// How long the radius takes to reach its target while the finger is still
/// down. Upstream's `_kUnconfirmedRippleDuration`.
const GROW_MICROS: i64 = 1_000_000;

/// How long the colour takes to arrive. `_kFadeInDuration`.
const FADE_IN_MICROS: i64 = 75_000;

/// How long the splash takes to disappear after the finger lifts.
/// `_kFadeOutDuration`.
const FADE_OUT_MICROS: i64 = 375_000;

/// The distance from `at` to the furthest corner of a box that size.
///
/// Upstream's `_getSplashRadiusForPositionInSize`: a tap in the corner has to
/// reach the opposite corner, or the splash stops before the edge of the thing
/// it is acknowledging.
pub fn splash_radius(size: Size, at: Offset) -> f32 {
    let corner = |x: f32, y: f32| Offset::new(at.dx - x, at.dy - y).distance();
    corner(0.0, 0.0)
        .max(corner(size.width, 0.0))
        .max(corner(0.0, size.height))
        .max(corner(size.width, size.height))
        .ceil()
}

/// One splash, in flight.
#[derive(Clone, Copy, Debug)]
struct Splash {
    /// Where the finger landed, in the region's coordinates.
    at: Offset,
    /// When it landed.
    started_micros: i64,
    /// When the finger lifted, if it has.
    released_micros: Option<i64>,
    /// The frame clock, so the build can evaluate without being handed it.
    now_micros: i64,
}

impl Splash {
    fn radius(&self, target: f32) -> f32 {
        let elapsed = (self.now_micros - self.started_micros).max(0) as f32;
        let t = (elapsed / GROW_MICROS as f32).clamp(0.0, 1.0);
        // Upstream starts at 30% of the target rather than at nothing: a
        // splash that begins as a point looks like a delay.
        let from = target * 0.30;
        from + (target + 5.0 - from) * Curve::EASE.transform(t)
    }

    fn opacity(&self) -> f32 {
        let elapsed = (self.now_micros - self.started_micros).max(0) as f32;
        let fade_in = (elapsed / FADE_IN_MICROS as f32).clamp(0.0, 1.0);
        let fade_out = match self.released_micros {
            Some(released) => {
                let since = (self.now_micros - released).max(0) as f32;
                1.0 - (since / FADE_OUT_MICROS as f32).clamp(0.0, 1.0)
            }
            None => 1.0,
        };
        fade_in * fade_out
    }

    /// Whether this splash still has anything to draw.
    fn alive(&self) -> bool {
        match self.released_micros {
            Some(released) => self.now_micros - released < FADE_OUT_MICROS,
            None => true,
        }
    }
}

/// What an [`Ink`] region remembers between frames.
#[derive(Default)]
pub struct InkState {
    splash: Option<Splash>,
    /// The region's size, filled in at layout so the next splash knows how far
    /// it has to reach. Upstream asks its `referenceBox`; the same answer, one
    /// frame later, which only matters for the first tap on a region that has
    /// never been laid out.
    size: Rc<Cell<Size>>,
}

/// A region that splashes where it is touched.
///
/// ```ignore
/// component(Ink::new(id, component(Card::new(body))))
/// ```
///
/// The splash is drawn over the child and under nothing: it is ink spreading
/// through the surface, so it goes on top of the colour and below whatever the
/// child draws on it. Upstream draws it into the `Material` beneath for exactly
/// that reason; here the child is what stands in for the material.
pub struct Ink {
    id: u64,
    /// A *builder* rather than a widget, and that is not a style choice: a
    /// stateful component is rebuilt from the same widget instance every time
    /// its own state changes, so a child stored as a widget would be handed
    /// over on the first build and gone on the second. A pressed button
    /// vanished exactly that way.
    build_child: Rc<dyn Fn() -> AnyWidget>,
    color: Option<Color>,
    /// Clipped to the child's box. A splash that escapes its button looks like
    /// a bug, and upstream's `containedInkWell` is the same switch.
    contained: bool,
}

impl Ink {
    pub fn new(id: u64, build_child: impl Fn() -> AnyWidget + 'static) -> Ink {
        Ink {
            id,
            build_child: Rc::new(build_child),
            color: None,
            contained: true,
        }
    }

    /// The splash colour. Defaults to the theme's primary at a tenth, which is
    /// what a Material overlay is: the colour of the thing that was pressed,
    /// not a grey.
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_contained(mut self, contained: bool) -> Self {
        self.contained = contained;
        self
    }
}

impl StatefulComponent for Ink {
    type State = InkState;

    fn key(&self) -> crate::framework::Key {
        Some(self.id)
    }

    fn advance(&self, state: &mut InkState, frame_time_micros: i64) -> bool {
        let Some(splash) = &mut state.splash else {
            return false;
        };
        splash.now_micros = frame_time_micros;
        if !splash.alive() {
            state.splash = None;
            // The frame that clears it still has to be drawn, or the last
            // ring of the splash stays on the screen for ever. See the same
            // rule in `animation::Controller::tick`.
            return true;
        }
        true
    }

    fn build(
        &self,
        state: &InkState,
        handle: StateHandle<InkState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let color = self.color.unwrap_or(theme.primary.with_alpha(0x1f));
        let child = (self.build_child)();

        let down_handle = handle.clone();
        let up_handle = handle.clone();
        let handlers = PointerHandlers::new()
            .with_pointer_down(move |event| {
                let at = event.local_position;
                let now = event.time_stamp_micros;
                down_handle.set_state(move |state| {
                    state.splash = Some(Splash {
                        at,
                        started_micros: now,
                        released_micros: None,
                        now_micros: now,
                    });
                });
            })
            .with_pointer_up({
                let handle = up_handle.clone();
                move |_| release(&handle)
            })
            // A press the platform took away fades out too: nothing was
            // completed, so the splash unwinds rather than finishing.
            .with_pointer_cancel(move |_| release(&up_handle));

        let splash = state.splash;
        let size_sink = Rc::clone(&state.size);
        let id = self.id;
        let contained = self.contained;

        single(child, move |child| {
            let measured = size_sink.get();
            let painted = splash.filter(|_| measured.width > 0.0).map(|splash| {
                let target = splash_radius(measured, splash.at);
                (splash.radius(target), splash.opacity(), splash.at)
            });

            let mut stack = crate::render::RenderStack::new().push_boxed(child);
            if let Some((radius, opacity, at)) = painted {
                if opacity > 0.0 && radius > 0.0 {
                    let circle = crate::widgets::Container::new()
                        .with_size(radius * 2.0, radius * 2.0)
                        .with_color(color)
                        .with_corner_radius(radius);
                    // Invisible to the pointer. Upstream a splash is not in the
                    // tree at all -- `_RenderInkFeatures` paints it over its
                    // child -- so nothing about it can take a press; here it is
                    // a real box stacked on top of the content, and this is
                    // what keeps it from being one.
                    stack = stack.push_positioned(
                        crate::render::RenderIgnorePointer::new(crate::render::RenderOpacity::new(
                            opacity, circle,
                        )),
                        StackPosition {
                            left: Some(at.dx - radius),
                            top: Some(at.dy - radius),
                            ..Default::default()
                        },
                    );
                }
            }
            // Reports the size the region was actually given, for the next
            // splash to measure itself against.
            let watched = crate::render::RenderSizeReporter::new(Rc::clone(&size_sink), stack);
            let region = crate::render::RenderPointerRegion::new(id, watched)
                .with_handlers(handlers.clone());
            if contained {
                crate::render::RenderRef::new(crate::render::RenderClipRect::new(region))
            } else {
                crate::render::RenderRef::new(region)
            }
        })
    }
}

/// Starts a splash on its way out.
///
/// Raw pointer events rather than the press state, and that is what makes an
/// `Ink` composable: press is an affordance that only one region on the path
/// can have, while every listener hears the pointer itself. A splash inside a
/// button whose tap belongs to the button still knows when the finger lifted.
fn release(handle: &StateHandle<InkState>) {
    handle.set_state(|state| {
        if let Some(splash) = &mut state.splash {
            if splash.released_micros.is_none() {
                splash.released_micros = Some(splash.now_micros);
            }
        }
    });
}

/// [`Ink`] as a widget.
pub fn ink(id: u64, build_child: impl Fn() -> AnyWidget + 'static) -> AnyWidget {
    stateful(Ink::new(id, build_child))
}

/// The alignment a splash is centred on when nothing was touched -- used by
/// the tests, which have no pointer.
pub const CENTRE: Alignment = Alignment::CENTER;

// -- The three ink features ---------------------------------------------------

/// Upstream's `Material.defaultSplashRadius`: how far a splash spreads when
/// nothing bounds it -- an unconstrained `InkWell`, or a circular highlight
/// with no radius of its own.
pub const DEFAULT_SPLASH_RADIUS: f32 = 35.0;

/// Where an ink feature is in its life, and what the caller has told it.
///
/// Upstream this is two or three `AnimationController`s per feature plus a
/// `confirm`/`cancel` pair called by the `InkWell` when the gesture settles.
/// Here it is one value the caller keeps and the feature reads, because the
/// animation in this crate is per-frame arithmetic rather than a controller
/// with listeners -- see [`crate::implicit`] and the module docs above.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkPhase {
    /// When the finger landed.
    pub started_micros: i64,
    /// When the gesture settled, and how. `None` while the finger is still
    /// down, which is the *unconfirmed* phase all three features start in.
    pub settled: Option<(i64, InkSettlement)>,
    /// The frame clock.
    pub now_micros: i64,
}

/// How a gesture ended. Upstream's `confirm()` and `cancel()`.
///
/// The distinction is not cosmetic: a confirmed tap finishes growing quickly
/// and *then* fades, so the reader sees the thing they hit fill in; a
/// cancelled one -- a scroll that started as a press -- fades at once, so
/// nothing acknowledges a press that was not a press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InkSettlement {
    Confirmed,
    Cancelled,
}

impl InkPhase {
    pub fn new(started_micros: i64, now_micros: i64) -> InkPhase {
        InkPhase {
            started_micros,
            settled: None,
            now_micros,
        }
    }

    pub fn settled_at(mut self, micros: i64, how: InkSettlement) -> InkPhase {
        self.settled = Some((micros, how));
        self
    }

    fn since_start(&self) -> f32 {
        (self.now_micros - self.started_micros).max(0) as f32
    }

    /// How far into its settling the feature is, or `None` while the finger
    /// is still down.
    fn since_settle(&self) -> Option<(f32, InkSettlement)> {
        self.settled
            .map(|(at, how)| ((self.now_micros - at).max(0) as f32, how))
    }
}

/// `t` clamped to 0..1 for a phase of `duration` microseconds.
fn phase_t(elapsed_micros: f32, duration_micros: i64) -> f32 {
    (elapsed_micros / duration_micros as f32).clamp(0.0, 1.0)
}

/// Upstream `InkSplash` (`material/ink_splash.dart`): the original Material
/// splash, a circle growing from nothing.
///
/// Its **target radius is the distance to the furthest corner** from where the
/// finger landed, which is what makes a tap near an edge still fill the whole
/// box. Note that this is *not* [`InkRipple`]'s rule -- see there -- and the
/// difference is the easiest thing to get wrong about the two, because they
/// are otherwise described the same way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkSplash {
    /// Where the finger landed, in the box's coordinates.
    pub position: Offset,
    pub target_radius: f32,
    /// Upstream's `containedInkWell`: whether the splash is clipped to the
    /// box. An uncontained splash is also the one that walks to the centre --
    /// upstream's `_repositionToReferenceBox = !containedInkWell` -- because
    /// a splash with nothing to fill has no reason to stay where it started.
    pub contained: bool,
}

impl InkSplash {
    /// `_kUnconfirmedSplashDuration`: how long the radius takes while the
    /// press is still unconfirmed.
    pub const UNCONFIRMED_MICROS: i64 = 1_000_000;
    /// `_kSplashFadeDuration`.
    pub const FADE_MICROS: i64 = 200_000;
    /// `_kSplashInitialSize`: nothing. A splash begins as a point.
    pub const INITIAL_RADIUS: f32 = 0.0;
    /// `_kSplashConfirmedVelocity`, in logical pixels per millisecond.
    pub const CONFIRMED_VELOCITY: f32 = 1.0;

    /// Upstream's `_getTargetRadius`: the furthest corner for a contained
    /// splash, and the flat default for one with no box to fill.
    pub fn target_radius(size: Size, position: Offset, contained: bool) -> f32 {
        if contained {
            splash_radius(size, position)
        } else {
            DEFAULT_SPLASH_RADIUS
        }
    }

    pub fn new(size: Size, position: Offset, contained: bool) -> InkSplash {
        InkSplash {
            position,
            target_radius: InkSplash::target_radius(size, position, contained),
            contained,
        }
    }

    /// Upstream's `confirm()`: the radius controller's duration becomes
    /// `targetRadius / _kSplashConfirmedVelocity` milliseconds.
    ///
    /// So a confirmed splash finishes at a *speed*, not in a fixed time: a
    /// big box takes longer to fill than a small one, which is what makes the
    /// ink read as ink rather than as a timer.
    pub fn confirmed_micros(&self) -> i64 {
        (self.target_radius / InkSplash::CONFIRMED_VELOCITY).floor() as i64 * 1_000
    }

    pub fn radius(&self, phase: InkPhase) -> f32 {
        // Upstream drives one controller whose duration is swapped on
        // confirm, and `forward()` continues from where it was: the elapsed
        // time is the whole press, and only the duration it is measured
        // against changes.
        let duration = match phase.settled {
            Some((_, InkSettlement::Confirmed)) => self.confirmed_micros(),
            _ => InkSplash::UNCONFIRMED_MICROS,
        };
        let t = phase_t(phase.since_start(), duration);
        // No curve: upstream's `Tween` on the raw controller value.
        InkSplash::INITIAL_RADIUS + (self.target_radius - InkSplash::INITIAL_RADIUS) * t
    }

    /// The alpha to paint at, as a fraction of the colour's own.
    ///
    /// Nothing fades until the gesture settles -- upstream's alpha controller
    /// is not started in the constructor, only in `confirm` and `cancel`.
    /// **Both** start it, which is upstream saying that a splash disappears
    /// either way; what differs is how fast the circle got there.
    pub fn opacity(&self, phase: InkPhase) -> f32 {
        match phase.since_settle() {
            None => 1.0,
            Some((since, _)) => 1.0 - phase_t(since, InkSplash::FADE_MICROS),
        }
    }

    /// Where the circle's centre is.
    ///
    /// A contained splash stays where it was touched. An uncontained one
    /// walks to the box's centre as it grows: with no box to fill, a circle
    /// stuck at the corner reads as a mistake.
    pub fn center(&self, size: Size, phase: InkPhase) -> Offset {
        if self.contained {
            return self.position;
        }
        let duration = match phase.settled {
            Some((_, InkSettlement::Confirmed)) => self.confirmed_micros(),
            _ => InkSplash::UNCONFIRMED_MICROS,
        };
        let t = phase_t(phase.since_start(), duration);
        let centre = Offset::new(size.width / 2.0, size.height / 2.0);
        Offset::new(
            self.position.dx + (centre.dx - self.position.dx) * t,
            self.position.dy + (centre.dy - self.position.dy) * t,
        )
    }

    pub fn alive(&self, phase: InkPhase) -> bool {
        match phase.since_settle() {
            None => true,
            Some((since, _)) => since < InkSplash::FADE_MICROS as f32,
        }
    }
}

/// Upstream `InkRipple` (`material/ink_ripple.dart`): the Material 2 splash,
/// which grows from most of the way out rather than from nothing.
///
/// Two things separate it from [`InkSplash`], and both are worth stating:
///
/// * **The target radius is half the box's diagonal**, not the distance to
///   the furthest corner -- so a ripple is the same size wherever it is
///   touched, where a splash is not. Upstream writes it as
///   `max(|bottomRight|, |topRight - bottomLeft|) / 2`, whose two arguments
///   are the same number: both diagonals of a rectangle are
///   `sqrt(w² + h²)`. The `max` is decoration.
/// * **It starts at 30% of the target and overshoots by 5.** A ripple that
///   began as a point would look like a delay, and the 5 is upstream's
///   comment: "final diameter is 10dps larger than the target diameter".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkRipple {
    pub position: Offset,
    pub target_radius: f32,
}

impl InkRipple {
    /// `_kUnconfirmedRippleDuration`.
    pub const UNCONFIRMED_MICROS: i64 = 1_000_000;
    /// `_kFadeInDuration`.
    pub const FADE_IN_MICROS: i64 = 75_000;
    /// `_kRadiusDuration`: the duration the radius finishes in once the tap
    /// is confirmed. Unlike [`InkSplash`], a ripple finishes in a fixed time
    /// however big the box is.
    pub const RADIUS_MICROS: i64 = 225_000;
    /// `_kFadeOutDuration`.
    pub const FADE_OUT_MICROS: i64 = 375_000;
    /// `_kCancelDuration`.
    pub const CANCEL_MICROS: i64 = 75_000;
    /// `_kFadeOutIntervalStart`, with upstream's comment attached: "the fade
    /// out begins 225ms after the `_fadeOutController` starts". Which is the
    /// radius duration -- the ripple finishes filling the box *before* it
    /// begins to leave, so the reader sees what they hit.
    pub const FADE_OUT_INTERVAL_START: f32 = 225.0 / 375.0;

    /// Upstream's `_getTargetRadius`: half the diagonal. See the type docs
    /// for why the `max` of two identical distances.
    pub fn target_radius(size: Size) -> f32 {
        Offset::new(size.width, size.height)
            .distance()
            .max(Offset::new(size.width, -size.height).distance())
            / 2.0
    }

    pub fn new(size: Size, position: Offset) -> InkRipple {
        InkRipple {
            position,
            target_radius: InkRipple::target_radius(size),
        }
    }

    /// How far the radius animation has run, 0..1, before its curve.
    fn radius_t(&self, phase: InkPhase) -> f32 {
        let duration = match phase.settled {
            Some((_, InkSettlement::Confirmed)) => InkRipple::RADIUS_MICROS,
            _ => InkRipple::UNCONFIRMED_MICROS,
        };
        phase_t(phase.since_start(), duration)
    }

    pub fn radius(&self, phase: InkPhase) -> f32 {
        let from = self.target_radius * 0.30;
        let to = self.target_radius + 5.0;
        from + (to - from) * Curve::EASE.transform(self.radius_t(phase))
    }

    /// The alpha to paint at, as a fraction of the colour's own.
    ///
    /// Upstream picks between two controllers -- `_fadeInController
    /// .isAnimating ? _fadeIn.value : _fadeOut.value` -- so the fade-in wins
    /// while it is still running even after a confirm.
    pub fn opacity(&self, phase: InkPhase) -> f32 {
        let fade_in = phase_t(phase.since_start(), InkRipple::FADE_IN_MICROS);
        match phase.since_settle() {
            // Still down, or the fade-in has not finished: the fade-in owns
            // the alpha.
            _ if fade_in < 1.0 => fade_in,
            None => 1.0,
            Some((since, InkSettlement::Confirmed)) => {
                // The interval: the first 225ms of the 375ms fade-out do
                // nothing, because the radius is still filling the box.
                let t = phase_t(since, InkRipple::FADE_OUT_MICROS);
                1.0 - interval(t, InkRipple::FADE_OUT_INTERVAL_START, 1.0)
            }
            // A cancel does not wait: the press was not a press, so nothing
            // should be left acknowledging it. Upstream sets the fade-out
            // controller's value to `1 - fadeIn.value` and runs the rest over
            // `_kCancelDuration`, which for a press shorter than the fade-in
            // is a fade from wherever the colour had got to.
            Some((since, InkSettlement::Cancelled)) => {
                let from = fade_in;
                from * (1.0 - phase_t(since, InkRipple::CANCEL_MICROS))
            }
        }
    }

    /// Where the circle's centre is: it walks from where it was touched to
    /// the box's centre as it grows, on the same eased clock the radius uses.
    pub fn center(&self, size: Size, phase: InkPhase) -> Offset {
        let t = Curve::EASE.transform(self.radius_t(phase));
        let centre = Offset::new(size.width / 2.0, size.height / 2.0);
        Offset::new(
            self.position.dx + (centre.dx - self.position.dx) * t,
            self.position.dy + (centre.dy - self.position.dy) * t,
        )
    }

    pub fn alive(&self, phase: InkPhase) -> bool {
        match phase.since_settle() {
            None => true,
            Some((since, InkSettlement::Confirmed)) => since < InkRipple::FADE_OUT_MICROS as f32,
            Some((since, InkSettlement::Cancelled)) => since < InkRipple::CANCEL_MICROS as f32,
        }
    }
}

/// Upstream's `Interval` curve, as the plain arithmetic: `t` remapped so that
/// nothing happens before `begin` and everything is over by `end`.
fn interval(t: f32, begin: f32, end: f32) -> f32 {
    ((t - begin) / (end - begin)).clamp(0.0, 1.0)
}

/// Upstream `InkHighlight` (`material/ink_highlight.dart`): the wash a
/// pointer leaves while it is *hovering or held*, as distinct from the splash
/// that marks the moment of contact.
///
/// It has no radius animation at all -- only an alpha, fading in over 200ms
/// and back out over the same when it is deactivated. That is the whole
/// difference: a splash is an event and a highlight is a state, so one has a
/// shape that travels and the other has only a presence.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InkHighlight {
    pub shape: InkHighlightShape,
    /// Used only by a circular highlight; a rectangular one fills its box.
    pub radius: Option<f32>,
    pub fade_micros: i64,
}

/// What an [`InkHighlight`] paints. Upstream's `BoxShape`, narrowed to the two
/// values a highlight uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum InkHighlightShape {
    #[default]
    Rectangle,
    Circle,
}

impl InkHighlight {
    /// `_kDefaultHighlightFadeDuration`.
    pub const FADE_MICROS: i64 = 200_000;

    pub fn new() -> InkHighlight {
        InkHighlight {
            shape: InkHighlightShape::Rectangle,
            radius: None,
            fade_micros: InkHighlight::FADE_MICROS,
        }
    }

    pub fn circular(radius: Option<f32>) -> InkHighlight {
        InkHighlight {
            shape: InkHighlightShape::Circle,
            radius,
            fade_micros: InkHighlight::FADE_MICROS,
        }
    }

    /// Upstream's `fadeDuration`, which a caller may shorten -- a highlight
    /// that follows the mouse wants to keep up with it.
    pub fn with_fade_micros(mut self, micros: i64) -> Self {
        self.fade_micros = micros;
        self
    }

    /// The radius a circular highlight paints at. A rectangular one has none.
    pub fn circle_radius(&self) -> Option<f32> {
        match self.shape {
            InkHighlightShape::Circle => Some(self.radius.unwrap_or(DEFAULT_SPLASH_RADIUS)),
            InkHighlightShape::Rectangle => None,
        }
    }

    /// The alpha to paint at, as a fraction of the colour's own.
    ///
    /// `active` is upstream's `_active`, set by `activate`/`deactivate`. The
    /// fade runs *backwards* from wherever it had got to when it was
    /// deactivated -- which is why this takes the fraction reached rather
    /// than a start time: a highlight that was interrupted half way in fades
    /// from half, not from full.
    pub fn opacity(&self, elapsed_micros: i64, active: bool, from: f32) -> f32 {
        let t = phase_t(elapsed_micros.max(0) as f32, self.fade_micros);
        if active {
            (from + (1.0 - from) * t).clamp(0.0, 1.0)
        } else {
            (from - from * t).clamp(0.0, 1.0)
        }
    }

    /// Upstream's `_handleAlphaStatusChanged`: a highlight is disposed when
    /// the fade reaches zero *and* it is still deactivated -- not merely when
    /// it reaches zero, because a highlight reactivated mid-fade is the same
    /// highlight and has to survive.
    pub fn alive(&self, opacity: f32, active: bool) -> bool {
        active || opacity > 0.0
    }
}

impl Default for InkHighlight {
    fn default() -> InkHighlight {
        InkHighlight::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn splash_at(at: Offset, started: i64, now: i64, released: Option<i64>) -> Splash {
        Splash {
            at,
            started_micros: started,
            released_micros: released,
            now_micros: now,
        }
    }

    #[test]
    fn a_splash_reaches_the_furthest_corner() {
        // A tap in the top-left of a 100x100 box has to reach the opposite
        // corner, which is 141 away.
        let radius = splash_radius(Size::new(100.0, 100.0), Offset::new(0.0, 0.0));
        assert!((radius - 142.0).abs() <= 1.0, "{radius}");

        // A tap in the middle only has to reach 71.
        let middle = splash_radius(Size::new(100.0, 100.0), Offset::new(50.0, 50.0));
        assert!((middle - 71.0).abs() <= 1.0, "{middle}");
    }

    #[test]
    fn a_splash_starts_visible_and_grows() {
        let target = 100.0;
        let splash = splash_at(Offset::new(10.0, 10.0), 0, 0, None);
        let first = splash.radius(target);
        assert!(
            first >= target * 0.3,
            "a splash that starts at nothing looks like a delay"
        );

        let later = splash_at(Offset::new(10.0, 10.0), 0, 500_000, None).radius(target);
        assert!(later > first);
    }

    #[test]
    fn a_splash_fades_in_quickly_and_out_slowly() {
        let at = Offset::new(10.0, 10.0);
        assert_eq!(splash_at(at, 0, 0, None).opacity(), 0.0, "not there yet");
        assert!(
            splash_at(at, 0, FADE_IN_MICROS, None).opacity() > 0.99,
            "arrived"
        );

        // Released at 200ms, and 375ms later it is gone.
        let leaving = splash_at(at, 0, 200_000 + FADE_OUT_MICROS / 2, Some(200_000));
        assert!(leaving.opacity() > 0.0 && leaving.opacity() < 1.0);
        let gone = splash_at(at, 0, 200_000 + FADE_OUT_MICROS, Some(200_000));
        assert_eq!(gone.opacity(), 0.0);
        assert!(!gone.alive());
    }

    #[test]
    fn a_press_that_is_still_held_does_not_fade() {
        let held = splash_at(Offset::new(1.0, 1.0), 0, 5_000_000, None);
        assert!(
            held.alive(),
            "a finger that has not lifted is still pressing"
        );
        assert!(held.opacity() > 0.99);
    }

    #[test]
    fn an_ink_region_is_the_size_of_its_child() {
        use crate::framework::{ElementTree, leaf};
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        let mut tree = ElementTree::new();
        tree.rebuild(super::ink(1, || leaf(|| SizedBox::new(100.0, 44.0))));
        let mut root = tree.build_render_tree().expect("mounted");
        let size = root.layout(BoxConstraints::new(0.0, 400.0, 0.0, 400.0));
        assert_eq!(
            size,
            Size::new(100.0, 44.0),
            "the ink must not resize the button"
        );
    }

    #[test]
    fn a_splashing_region_is_still_the_size_of_its_child() {
        // The bug this is written for: pressing a button made it vanish. A
        // splash is drawn *over* the child and must not change what the parent
        // measures.
        use crate::framework::{ElementTree, leaf};
        use crate::gestures::{
            GestureRouter, PointerChange, PointerEvent, PointerKind, SignalKind,
        };
        use crate::render::{BoxConstraints, RenderBox};
        use crate::widgets::SizedBox;

        let mut tree = ElementTree::new();
        tree.rebuild(super::ink(1, || leaf(|| SizedBox::new(100.0, 44.0))));
        let mut root = tree.build_render_tree().expect("mounted");
        root.layout(BoxConstraints::new(0.0, 400.0, 0.0, 400.0));

        let mut router = GestureRouter::new();
        let down = PointerEvent {
            view_id: 0,
            device: 0,
            pointer_id: 1,
            change: PointerChange::Down,
            kind: PointerKind::Touch,
            signal_kind: SignalKind::None,
            buttons: 1,
            time_stamp_micros: 0,
            position: Offset::new(50.0, 22.0),
            delta: Offset::ZERO,
            scroll_delta: Offset::ZERO,
            pressure: 1.0,
            local_position: Offset::new(50.0, 22.0),
        };
        assert!(
            router.dispatch(&root, &down),
            "the press should land on the ink"
        );

        tree.advance_frame(16_000);
        tree.rebuild_dirty();
        let mut splashing = tree.build_render_tree().expect("mounted");
        let size = splashing.layout(BoxConstraints::new(0.0, 400.0, 0.0, 400.0));
        assert_eq!(
            size,
            Size::new(100.0, 44.0),
            "the splash resized the button"
        );
    }

    const MS: i64 = 1_000;

    fn at(micros: i64) -> InkPhase {
        InkPhase::new(0, micros)
    }

    #[test]
    fn a_splash_reaches_the_furthest_corner_and_a_ripple_reaches_half_the_diagonal() {
        // The easiest thing to get wrong about the two, because they are
        // otherwise described the same way. A splash's size depends on where
        // it was touched; a ripple's does not.
        let size = Size::new(300.0, 400.0);
        assert_eq!(
            InkSplash::target_radius(size, Offset::new(0.0, 0.0), true),
            500.0,
            "the opposite corner of a 3-4-5 box"
        );
        assert_eq!(
            InkSplash::target_radius(size, Offset::new(150.0, 200.0), true),
            250.0,
            "half of it, from the middle"
        );
        // Half the diagonal, wherever the finger landed.
        assert_eq!(InkRipple::target_radius(size), 250.0);
    }

    #[test]
    fn an_uncontained_splash_takes_the_flat_default_rather_than_the_boxs_corner() {
        // With nothing to fill, the box's size says nothing about how far the
        // ink should spread.
        assert_eq!(
            InkSplash::target_radius(Size::new(1000.0, 1000.0), Offset::ZERO, false),
            DEFAULT_SPLASH_RADIUS
        );
    }

    #[test]
    fn upstreams_max_of_two_diagonals_is_a_max_of_one_number() {
        // Upstream writes `max(|bottomRight|, |topRight - bottomLeft|) / 2`.
        // Both diagonals of a rectangle are sqrt(w^2 + h^2), so the max is
        // decoration -- worth pinning so nobody "fixes" one of the two into
        // something that is not the other.
        for (w, h) in [(10.0, 90.0), (90.0, 10.0), (7.0, 7.0)] {
            let size = Size::new(w, h);
            assert_eq!(InkRipple::target_radius(size), (w * w + h * h).sqrt() / 2.0);
        }
    }

    #[test]
    fn a_ripple_starts_most_of_the_way_out_and_overshoots() {
        // A ripple that began as a point would look like a delay; the
        // overshoot is upstream's "final diameter is 10dps larger".
        let ripple = InkRipple::new(Size::new(200.0, 0.0), Offset::new(10.0, 0.0));
        assert_eq!(ripple.target_radius, 100.0);
        // 30% of the target at t=0. Compared with a tolerance because the
        // ease curve is solved rather than evaluated, and a cubic solver
        // lands a few billionths off its own endpoints.
        assert!((ripple.radius(at(0)) - 30.0).abs() < 0.001);
        assert!((ripple.radius(at(InkRipple::UNCONFIRMED_MICROS)) - 105.0).abs() < 0.01);
    }

    #[test]
    fn a_splash_begins_as_a_point() {
        // Where the ripple starts at 30%, the older splash starts at nothing
        // and grows linearly -- no curve at all on its radius.
        let splash = InkSplash::new(Size::new(200.0, 0.0), Offset::new(100.0, 0.0), true);
        assert_eq!(splash.radius(at(0)), 0.0);
        let half = splash.radius(at(InkSplash::UNCONFIRMED_MICROS / 2));
        assert!(
            (half - splash.target_radius / 2.0).abs() < 0.01,
            "half the time is half the radius: {half}"
        );
    }

    #[test]
    fn a_confirmed_splash_finishes_at_a_speed_rather_than_in_a_time() {
        // Upstream's `targetRadius / _kSplashConfirmedVelocity`: a big box
        // takes longer to fill than a small one, which is what makes the ink
        // read as ink rather than as a timer.
        let small = InkSplash::new(Size::new(40.0, 0.0), Offset::new(20.0, 0.0), true);
        let large = InkSplash::new(Size::new(400.0, 0.0), Offset::new(200.0, 0.0), true);
        assert!(large.confirmed_micros() > small.confirmed_micros());
        assert_eq!(
            small.confirmed_micros(),
            20 * MS,
            "20 logical pixels at 1/ms"
        );
    }

    #[test]
    fn a_confirmed_ripple_finishes_in_a_fixed_time_however_big_the_box() {
        // The other half of the pair above: where a splash has a velocity, a
        // ripple has a duration, so the two diverge most on a large surface.
        let big = InkRipple::new(Size::new(2000.0, 2000.0), Offset::ZERO);
        let confirmed = at(InkRipple::RADIUS_MICROS).settled_at(0, InkSettlement::Confirmed);
        assert!((big.radius(confirmed) - (big.target_radius + 5.0)).abs() < 0.01);
    }

    #[test]
    fn a_ripple_finishes_filling_the_box_before_it_begins_to_leave() {
        // Upstream's `Interval(225/375, 1.0)` on the fade-out, whose comment
        // says the fade begins 225ms in -- which is exactly the radius
        // duration. The reader sees what they hit.
        let ripple = InkRipple::new(Size::new(100.0, 100.0), Offset::ZERO);
        let settled = |micros: i64| {
            InkPhase::new(0, InkRipple::FADE_IN_MICROS + micros)
                .settled_at(InkRipple::FADE_IN_MICROS, InkSettlement::Confirmed)
        };
        assert_eq!(ripple.opacity(settled(0)), 1.0);
        assert_eq!(
            ripple.opacity(settled(200 * MS)),
            1.0,
            "still nothing at 200ms, because the radius is still growing"
        );
        assert!(ripple.opacity(settled(300 * MS)) < 1.0);
        assert_eq!(ripple.opacity(settled(InkRipple::FADE_OUT_MICROS)), 0.0);
    }

    #[test]
    fn a_cancelled_ripple_does_not_wait() {
        // A scroll that started as a press: nothing should be left
        // acknowledging a press that was not one, so the cancel fades from
        // wherever the colour had got to rather than from full.
        let ripple = InkRipple::new(Size::new(100.0, 100.0), Offset::ZERO);
        let pressed_for = 40 * MS; // shorter than the 75ms fade-in
        let at_cancel =
            InkPhase::new(0, pressed_for).settled_at(pressed_for, InkSettlement::Cancelled);
        let partial = ripple.opacity(at_cancel);
        assert!(
            partial < 1.0 && partial > 0.0,
            "faded in only part way: {partial}"
        );
        let done = InkPhase::new(0, pressed_for + InkRipple::CANCEL_MICROS)
            .settled_at(pressed_for, InkSettlement::Cancelled);
        assert_eq!(ripple.opacity(done), 0.0);
        assert!(!ripple.alive(done));
    }

    #[test]
    fn the_fade_in_owns_the_alpha_while_it_is_still_running() {
        // Upstream's `_fadeInController.isAnimating ? _fadeIn : _fadeOut`: a
        // tap confirmed inside the first 75ms keeps fading *in*, so a quick
        // tap still shows its colour rather than starting to leave at once.
        let ripple = InkRipple::new(Size::new(100.0, 100.0), Offset::ZERO);
        let quick = InkPhase::new(0, 30 * MS).settled_at(10 * MS, InkSettlement::Confirmed);
        assert!((ripple.opacity(quick) - 0.4).abs() < 0.01, "30 of 75ms in");
    }

    #[test]
    fn only_an_uncontained_splash_walks_to_the_centre() {
        // A contained splash is clipped to the box, so it has something to
        // fill and no reason to move; an uncontained one stuck at the corner
        // reads as a mistake.
        let size = Size::new(200.0, 100.0);
        let corner = Offset::new(0.0, 0.0);
        let contained = InkSplash::new(size, corner, true);
        let free = InkSplash::new(size, corner, false);
        let late = at(InkSplash::UNCONFIRMED_MICROS);
        assert_eq!(contained.center(size, late), corner);
        assert_eq!(free.center(size, late), Offset::new(100.0, 50.0));
    }

    #[test]
    fn a_splash_does_not_fade_until_the_gesture_settles() {
        // Upstream does not start the alpha controller in the constructor:
        // a finger held down keeps its splash at full colour however long it
        // is held.
        let splash = InkSplash::new(Size::new(100.0, 100.0), Offset::ZERO, true);
        assert_eq!(splash.opacity(at(10_000 * MS)), 1.0);
        // And both endings start it -- a splash disappears either way; what
        // differed was how fast the circle got there.
        for how in [InkSettlement::Confirmed, InkSettlement::Cancelled] {
            let done = InkPhase::new(0, InkSplash::FADE_MICROS).settled_at(0, how);
            assert_eq!(splash.opacity(done), 0.0);
            assert!(!splash.alive(done));
        }
    }

    #[test]
    fn a_highlight_has_no_radius_animation_at_all() {
        // A splash is an event and a highlight is a state, so one has a shape
        // that travels and the other has only a presence.
        let highlight = InkHighlight::new();
        assert_eq!(highlight.circle_radius(), None, "a rectangle fills its box");
        assert_eq!(
            InkHighlight::circular(None).circle_radius(),
            Some(DEFAULT_SPLASH_RADIUS)
        );
        assert_eq!(InkHighlight::circular(Some(8.0)).circle_radius(), Some(8.0));
    }

    #[test]
    fn a_highlight_interrupted_half_way_in_fades_from_half() {
        // Upstream reverses the same controller, so the fade runs backwards
        // from wherever it had got to -- not from full, which would be a jump
        // to brighter on the way out.
        let highlight = InkHighlight::new();
        let half = highlight.opacity(InkHighlight::FADE_MICROS / 2, true, 0.0);
        assert!((half - 0.5).abs() < 0.01);
        let out = highlight.opacity(InkHighlight::FADE_MICROS / 2, false, half);
        assert!((out - 0.25).abs() < 0.01, "half of a half: {out}");
    }

    #[test]
    fn a_highlight_reactivated_mid_fade_survives() {
        // Upstream disposes on `isDismissed && !_active`, not on reaching
        // zero: a highlight the pointer came back to is the same highlight.
        let highlight = InkHighlight::new();
        assert!(highlight.alive(0.0, true), "still wanted");
        assert!(!highlight.alive(0.0, false), "gone and not wanted");
        assert!(highlight.alive(0.1, false), "still fading out");
    }
}
