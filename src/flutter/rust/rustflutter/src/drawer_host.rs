//! The drawer, with the animation it lost.
//!
//! `drawer.rs` ports the panel and says plainly what it left out, with the
//! reason:
//!
//! > **The slide animation** (`_kBaseSettleDuration` 246ms of
//! > `AnimationController`): it is the controller's `value` that positions the
//! > drawer and fades the scrim, and **there is no controller without a
//! > route-like owner for it**. The drawer is simply present or absent.
//!
//! The owner exists now. An overlay entry is a widget the framework keeps
//! alive across frames, and a `StatefulComponent` in one gets `advance` called
//! once a frame -- which is a clock, which is all a controller ever was.
//!
//! # A drawer is modal, and the scrim is the animation
//!
//! The panel slides in from the edge and the scrim fades up behind it, both
//! from the same 0-to-1 value. That is upstream's arrangement exactly: one
//! controller, two things reading it. Getting them from separate sources is how
//! a drawer ends up half-lit.

use std::cell::RefCell;
use std::rc::Rc;

use crate::drawer::BASE_SETTLE_MILLISECONDS;
use crate::engine::Color;
use crate::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent, many};
use crate::render::{RenderFractionalTranslation, RenderStack, Size};
use crate::theatre::{ModalHandle, OverlayHandle, RenderScrim};

/// Which edge a drawer comes in from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DrawerSide {
    #[default]
    Start,
    End,
}

/// Upstream's scrim at full strength: `Colors.black54`.
pub const DRAWER_SCRIM_COLOR: Color = Color::argb(0x8A, 0, 0, 0);

/// How far along the slide is, and which way it is going.
#[derive(Default)]
pub struct DrawerAnimation {
    /// 0 is closed, 1 is open. Upstream's `AnimationController.value`.
    progress: f32,
    /// What it is heading for.
    target: f32,
    /// The frame this last moved on, so a gap between frames advances by the
    /// time that actually passed rather than by one tick.
    last_frame_micros: Option<i64>,
    /// Set when the slide has run all the way out, so the entry can be removed
    /// once it is no longer visible.
    closed: bool,
}

impl DrawerAnimation {
    pub fn progress(&self) -> f32 {
        self.progress
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Moves the value towards its target, and says whether another frame is
    /// wanted. Upstream's `AnimationController.forward` / `reverse` reduced to
    /// the one thing they do.
    ///
    /// The step is taken from the elapsed time rather than assumed, because a
    /// frame that arrived late covered more of the distance than one that did
    /// not -- an animation that counts frames instead of milliseconds runs slow
    /// on a busy machine and fast on an idle one.
    pub fn advance(&mut self, frame_time_micros: i64) -> bool {
        let elapsed = match self.last_frame_micros {
            Some(last) => (frame_time_micros - last).max(0) as f32 / 1000.0,
            None => 0.0,
        };
        self.last_frame_micros = Some(frame_time_micros);

        if (self.progress - self.target).abs() < f32::EPSILON {
            self.closed = self.target == 0.0 && self.progress == 0.0;
            return false;
        }
        let step = elapsed / BASE_SETTLE_MILLISECONDS as f32;
        if self.target > self.progress {
            self.progress = (self.progress + step).min(self.target);
        } else {
            self.progress = (self.progress - step).max(self.target);
        }
        if (self.progress - self.target).abs() < 1e-4 {
            self.progress = self.target;
        }
        self.closed = self.progress == 0.0 && self.target == 0.0;
        true
    }

    pub fn open(&mut self) {
        self.target = 1.0;
        self.closed = false;
    }

    pub fn close(&mut self) {
        self.target = 0.0;
    }

    pub fn is_settled(&self) -> bool {
        (self.progress - self.target).abs() < f32::EPSILON
    }
}

/// The drawer as an overlay entry: a scrim and a panel, both reading one value.
pub struct DrawerHost {
    panel: Rc<dyn Fn() -> AnyWidget>,
    side: DrawerSide,
    scrim_id: u64,
    on_dismiss: Rc<dyn Fn()>,
    /// Handed to the state so an outside caller can start the slide out.
    controls: DrawerControls,
}

/// Opens and closes a hosted drawer.
#[derive(Clone, Default)]
pub struct DrawerControls {
    handle: Rc<RefCell<Option<StateHandle<DrawerAnimation>>>>,
}

impl DrawerControls {
    pub fn new() -> DrawerControls {
        DrawerControls::default()
    }

    /// Starts the slide in.
    pub fn open(&self) -> bool {
        self.with(|state| state.open())
    }

    /// Starts the slide out. The entry stays until the panel has finished
    /// leaving -- removing it now is what made the old drawer pop rather than
    /// close.
    pub fn close(&self) -> bool {
        self.with(|state| state.close())
    }

    fn with(&self, mutate: impl FnOnce(&mut DrawerAnimation) + 'static) -> bool {
        let handle = self.handle.borrow().clone();
        handle.is_some_and(|handle| handle.set_state(mutate))
    }

    fn attach(&self, handle: StateHandle<DrawerAnimation>) {
        *self.handle.borrow_mut() = Some(handle);
    }
}

impl StatefulComponent for DrawerHost {
    type State = DrawerAnimation;

    fn initial_state(&self) -> DrawerAnimation {
        // Opens as soon as it is mounted, which is what putting a drawer up
        // means. Upstream's `DrawerController.open` does the same on the
        // controller it has just made.
        let mut animation = DrawerAnimation::default();
        animation.open();
        animation
    }

    fn advance(&self, state: &mut DrawerAnimation, frame_time_micros: i64) -> bool {
        let moving = state.advance(frame_time_micros);
        if state.is_closed() {
            (self.on_dismiss)();
        }
        moving
    }

    fn build(
        &self,
        state: &DrawerAnimation,
        handle: StateHandle<DrawerAnimation>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        self.controls.attach(handle);
        let progress = state.progress().clamp(0.0, 1.0);

        // Both from the same value: one controller, two things reading it.
        let scrim_alpha = (DRAWER_SCRIM_COLOR.alpha() as f32 * progress) as u8;
        let scrim_color = Color::argb(scrim_alpha, 0, 0, 0);
        let dismiss = Rc::clone(&self.on_dismiss);
        let scrim_id = self.scrim_id;
        let scrim = crate::framework::leaf(move || {
            let dismiss = Rc::clone(&dismiss);
            crate::render::RenderPointerRegion::new(scrim_id, RenderScrim::new(Some(scrim_color)))
                .with_handlers(crate::gestures::PointerHandlers::new().with_tap(move |_| dismiss()))
                .with_behavior(crate::render::HitTestBehavior::Opaque)
        });

        // Fully out at 0, fully in at 1. Negative for a drawer on the start
        // edge, positive for one on the end.
        let offscreen = match self.side {
            DrawerSide::Start => -(1.0 - progress),
            DrawerSide::End => 1.0 - progress,
        };
        let panel = (self.panel)();
        let panel = many(vec![panel], move |mut rendered| {
            RenderFractionalTranslation::new(
                (offscreen, 0.0),
                rendered.pop().expect("the drawer panel"),
            )
        });

        let alignment = match self.side {
            DrawerSide::Start => crate::render::Alignment::new(-1.0, 0.0),
            DrawerSide::End => crate::render::Alignment::new(1.0, 0.0),
        };
        many(vec![scrim, panel], move |mut rendered| {
            let panel = rendered.pop().expect("the panel");
            let scrim = rendered.pop().expect("the scrim");
            RenderStack::new()
                .push_boxed(scrim)
                .push_boxed(crate::render::RenderRef::new(
                    crate::render::RenderAlign::new(alignment, panel),
                ))
        })
    }
}

/// Puts a drawer up over `overlay`, sliding in from `side`.
///
/// The handle takes it down; the drawer also takes itself down when its scrim
/// is tapped, which is upstream's behaviour and the reason the panel is behind
/// a barrier at all.
pub fn show_drawer(
    overlay: Rc<OverlayHandle>,
    side: DrawerSide,
    panel: impl Fn() -> AnyWidget + 'static,
) -> Option<(ModalHandle, DrawerControls)> {
    let controls = DrawerControls::new();
    let panel: Rc<dyn Fn() -> AnyWidget> = Rc::new(panel);
    let closing = controls.clone();
    let scrim_id = crate::theatre::next_surface_id();

    let entry = {
        let panel = Rc::clone(&panel);
        let controls = controls.clone();
        overlay.insert_entry(crate::overlay::OverlayEntry::new(0), move || {
            let closing = closing.clone();
            crate::framework::stateful(DrawerHost {
                panel: Rc::clone(&panel),
                side,
                scrim_id,
                on_dismiss: Rc::new(move || {
                    closing.close();
                }),
                controls: controls.clone(),
            })
        })?
    };

    let handle = crate::theatre::modal_from_entry(overlay, entry);
    Some((handle, controls))
}

/// The size a drawer panel takes. Upstream's `_kWidth`.
pub fn drawer_width() -> f32 {
    crate::drawer::DRAWER_WIDTH
}

/// How wide a drawer is against the screen it is over.
pub fn panel_size(overlay: Size) -> Size {
    Size::new(drawer_width().min(overlay.width), overlay.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A quarter of the settle duration, in microseconds.
    const QUARTER: i64 = (BASE_SETTLE_MILLISECONDS as i64 * 1000) / 4;

    fn opening() -> DrawerAnimation {
        let mut animation = DrawerAnimation::default();
        animation.open();
        // The first frame establishes the clock; nothing has elapsed yet.
        animation.advance(0);
        animation
    }

    #[test]
    fn a_drawer_slides_rather_than_appearing() {
        // The whole of what drawer.rs said it could not have: the panel is
        // partway in, not present or absent.
        let mut animation = opening();
        assert_eq!(animation.progress(), 0.0, "starts closed");

        animation.advance(QUARTER);
        let quarter = animation.progress();
        assert!(
            quarter > 0.0 && quarter < 1.0,
            "partway in after a quarter of the settle: {quarter}"
        );
        assert!(
            (quarter - 0.25).abs() < 0.01,
            "and about a quarter: {quarter}"
        );
    }

    #[test]
    fn it_takes_the_settle_duration_to_arrive() {
        let mut animation = opening();
        animation.advance(BASE_SETTLE_MILLISECONDS as i64 * 1000);
        assert_eq!(animation.progress(), 1.0);
        assert!(animation.is_settled());
    }

    #[test]
    fn and_stops_asking_for_frames_once_it_has() {
        // Frames are on demand, so an animation that stops asking stops
        // running. The call that *arrives* at the target still moved, so it
        // still asks; the one after it has nowhere to go.
        let mut animation = opening();
        let settle = BASE_SETTLE_MILLISECONDS as i64 * 1000;
        assert!(
            animation.advance(QUARTER),
            "moving, so another frame please"
        );
        assert!(
            animation.advance(settle),
            "this one arrived, and moved to do it"
        );
        assert_eq!(animation.progress(), 1.0);
        assert!(
            !animation.advance(settle + QUARTER),
            "and now there is nothing left to draw"
        );
    }

    #[test]
    fn the_step_comes_from_the_time_that_passed_not_from_the_frame_count() {
        // An animation that counts frames runs slow on a busy machine and fast
        // on an idle one. Two frames covering half the duration between them
        // arrive at the same place as four.
        let mut coarse = opening();
        coarse.advance(QUARTER * 2);
        let after_two = coarse.progress();

        let mut fine = opening();
        fine.advance(QUARTER / 2);
        fine.advance(QUARTER);
        fine.advance(QUARTER + QUARTER / 2);
        fine.advance(QUARTER * 2);
        let after_four = fine.progress();

        assert!(
            (after_two - after_four).abs() < 1e-3,
            "{after_two} vs {after_four}"
        );
    }

    #[test]
    fn closing_runs_the_same_value_backwards() {
        let mut animation = opening();
        animation.advance(BASE_SETTLE_MILLISECONDS as i64 * 1000);
        assert_eq!(animation.progress(), 1.0);

        animation.close();
        animation.advance(BASE_SETTLE_MILLISECONDS as i64 * 1000 + QUARTER);
        let after = animation.progress();
        assert!(after > 0.0 && after < 1.0, "partway out: {after}");
    }

    #[test]
    fn a_drawer_is_only_closed_once_it_has_finished_leaving() {
        // Removing the entry at the moment `close` was called is what made the
        // old drawer pop rather than close.
        let mut animation = opening();
        animation.advance(BASE_SETTLE_MILLISECONDS as i64 * 1000);
        animation.close();
        assert!(!animation.is_closed(), "still on screen, on its way out");

        animation.advance(BASE_SETTLE_MILLISECONDS as i64 * 2000);
        assert_eq!(animation.progress(), 0.0);
        assert!(animation.is_closed());
    }

    #[test]
    fn reopening_mid_close_turns_around_from_where_it_is() {
        // Not from the start: a drawer that jumped back to closed before
        // reopening would flicker.
        let mut animation = opening();
        animation.advance(BASE_SETTLE_MILLISECONDS as i64 * 1000);
        animation.close();
        animation.advance(BASE_SETTLE_MILLISECONDS as i64 * 1000 + QUARTER);
        let mid = animation.progress();

        animation.open();
        assert_eq!(animation.progress(), mid, "it did not jump");
        animation.advance(BASE_SETTLE_MILLISECONDS as i64 * 1000 + QUARTER * 2);
        assert!(animation.progress() > mid, "and went back the other way");
    }

    // -- The two things reading one value ------------------------------------------

    #[test]
    fn the_panel_and_the_scrim_read_the_same_number() {
        // Getting them from separate sources is how a drawer ends up half-lit.
        for progress in [0.0f32, 0.25, 0.5, 1.0] {
            let scrim_alpha = (DRAWER_SCRIM_COLOR.alpha() as f32 * progress) as u8;
            let offscreen = -(1.0 - progress);
            assert_eq!(
                scrim_alpha == 0,
                offscreen <= -1.0,
                "fully out means fully clear, at {progress}"
            );
        }
    }

    #[test]
    fn a_drawer_on_the_end_edge_comes_in_from_the_other_side() {
        let start = match DrawerSide::Start {
            DrawerSide::Start => -(1.0 - 0.5),
            DrawerSide::End => 1.0 - 0.5,
        };
        let end = match DrawerSide::End {
            DrawerSide::Start => -(1.0 - 0.5),
            DrawerSide::End => 1.0 - 0.5,
        };
        assert_eq!(start, -0.5);
        assert_eq!(end, 0.5);
        assert_eq!(start, -end, "mirrored, not offset");
    }

    #[test]
    fn the_scrim_is_upstreams_black54_at_full_strength() {
        assert_eq!(DRAWER_SCRIM_COLOR.alpha(), 0x8A);
    }

    #[test]
    fn a_panel_is_the_upstream_width_unless_the_screen_is_narrower() {
        assert_eq!(drawer_width(), 304.0);
        assert_eq!(panel_size(Size::new(800.0, 600.0)).width, 304.0);
        assert_eq!(
            panel_size(Size::new(200.0, 600.0)).width,
            200.0,
            "a drawer wider than the screen is not a drawer"
        );
    }

    #[test]
    fn controls_with_no_drawer_attached_do_nothing() {
        let controls = DrawerControls::new();
        assert!(!controls.open());
        assert!(!controls.close());
    }
}
