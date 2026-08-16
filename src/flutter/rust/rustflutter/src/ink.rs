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

use std::cell::Cell;
use std::rc::Rc;

use crate::animation::Curve;
use crate::components::theme_of;
use crate::engine::Color;
use crate::framework::{
    AnyWidget, BuildContext, StateHandle, StatefulComponent, single, stateful,
};
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
        Ink { id, build_child: Rc::new(build_child), color: None, contained: true }
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
        let Some(splash) = &mut state.splash else { return false };
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
                    stack = stack.push_positioned(
                        crate::render::RenderOpacity::new(opacity, circle),
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
            let watched = crate::render::RenderSizeReporter::new(
                Rc::clone(&size_sink),
                stack,
            );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn splash_at(at: Offset, started: i64, now: i64, released: Option<i64>) -> Splash {
        Splash { at, started_micros: started, released_micros: released, now_micros: now }
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
        assert!(first >= target * 0.3, "a splash that starts at nothing looks like a delay");

        let later = splash_at(Offset::new(10.0, 10.0), 0, 500_000, None).radius(target);
        assert!(later > first);
    }

    #[test]
    fn a_splash_fades_in_quickly_and_out_slowly() {
        let at = Offset::new(10.0, 10.0);
        assert_eq!(splash_at(at, 0, 0, None).opacity(), 0.0, "not there yet");
        assert!(splash_at(at, 0, FADE_IN_MICROS, None).opacity() > 0.99, "arrived");

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
        assert!(held.alive(), "a finger that has not lifted is still pressing");
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
        assert_eq!(size, Size::new(100.0, 44.0), "the ink must not resize the button");
    }

    #[test]
    fn a_splashing_region_is_still_the_size_of_its_child() {
        // The bug this is written for: pressing a button made it vanish. A
        // splash is drawn *over* the child and must not change what the parent
        // measures.
        use crate::framework::{ElementTree, leaf};
        use crate::gestures::{GestureRouter, PointerChange, PointerEvent, PointerKind, SignalKind};
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
        assert!(router.dispatch(&root, &down), "the press should land on the ink");

        tree.advance_frame(16_000);
        tree.rebuild_dirty();
        let mut splashing = tree.build_render_tree().expect("mounted");
        let size = splashing.layout(BoxConstraints::new(0.0, 400.0, 0.0, 400.0));
        assert_eq!(size, Size::new(100.0, 44.0), "the splash resized the button");
    }
}
