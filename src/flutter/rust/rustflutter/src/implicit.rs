// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Animations nobody started.
//!
//! An explicit animation is a [`Controller`](crate::animation::Controller)
//! somebody calls `forward` on. An *implicit* one has no such call: a widget is
//! given a value, the value is different from last time, and it moves there by
//! itself. Upstream this is the `Animated*` family -- `AnimatedContainer`,
//! `AnimatedOpacity`, `AnimatedAlign` -- all of them built on the same base,
//! `ImplicitlyAnimatedWidget`, and all of them expressible through the one
//! general form, `TweenAnimationBuilder`. That general form is what is here.
//!
//! ```ignore
//! // A panel that slides to whatever height the state says, without anyone
//! // having to drive it.
//! animated(state.height, Duration::from_millis(200), Curve::FAST_OUT_SLOW_IN,
//!          |height| leaf(move || SizedBox::new(200.0, height)))
//! ```
//!
//! # Interruption is the whole design
//!
//! When the target changes mid-flight, the animation restarts *from where it
//! currently is* rather than from the old target. Upstream does the same, in
//! `ImplicitlyAnimatedWidgetState.didUpdateWidget`: the tween's beginning
//! becomes the evaluated current value. Without that rule a value changed
//! twice in quick succession jumps backwards to the first animation's start,
//! which is exactly the case implicit animations exist to make painless.

use std::time::Duration;

use crate::animation::Curve;
use crate::engine::Color;
use crate::framework::{AnyWidget, BuildContext, Key, StateHandle, StatefulComponent, stateful};
use crate::render::{EdgeInsets, Offset, Size};

/// A value an implicit animation can move through.
///
/// Upstream this is a `Tween<T>` per type; here it is one trait, because the
/// only thing a tween does that matters to this file is find the value a
/// fraction of the way between two others.
pub trait Lerp: Copy + PartialEq + 'static {
    /// The value `t` of the way from `from` to `to`, where `t` is 0..1.
    fn lerp(from: Self, to: Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(from: f32, to: f32, t: f32) -> f32 {
        from + (to - from) * t
    }
}

impl Lerp for Offset {
    fn lerp(from: Offset, to: Offset, t: f32) -> Offset {
        Offset::new(f32::lerp(from.dx, to.dx, t), f32::lerp(from.dy, to.dy, t))
    }
}

impl Lerp for Size {
    fn lerp(from: Size, to: Size, t: f32) -> Size {
        Size::new(
            f32::lerp(from.width, to.width, t),
            f32::lerp(from.height, to.height, t),
        )
    }
}

impl Lerp for EdgeInsets {
    fn lerp(from: EdgeInsets, to: EdgeInsets, t: f32) -> EdgeInsets {
        EdgeInsets {
            left: f32::lerp(from.left, to.left, t),
            top: f32::lerp(from.top, to.top, t),
            right: f32::lerp(from.right, to.right, t),
            bottom: f32::lerp(from.bottom, to.bottom, t),
        }
    }
}

impl Lerp for Color {
    fn lerp(from: Color, to: Color, t: f32) -> Color {
        // Channel by channel in premultiplied-free sRGB, which is what
        // `Color.lerp` does upstream. Interpolating the packed integer instead
        // would take the colours through whatever lies between them in memory.
        let channel = |shift: u32| {
            let a = ((from.0 >> shift) & 0xff) as f32;
            let b = ((to.0 >> shift) & 0xff) as f32;
            ((a + (b - a) * t).round().clamp(0.0, 255.0) as u32) << shift
        };
        Color(channel(24) | channel(16) | channel(8) | channel(0))
    }
}

/// Where an implicit animation has got to.
///
/// `from` and `to` are the ends of the current flight; there is no flight at
/// all until the target changes, which is why they start equal.
pub struct AnimatedState<T> {
    from: Option<T>,
    to: Option<T>,
    /// When the current flight began, or None if there is no flight.
    started_micros: Option<i64>,
    /// The frame clock, so `build` can evaluate without being handed the time.
    now_micros: i64,
    /// Set by `did_update_widget` when the target moved, and consumed by the
    /// next `advance`.
    ///
    /// Upstream restarts the flight inside `didUpdateWidget` itself, off its
    /// own controller; the clock here is the advance pass, so the restart is
    /// *decided* when the widget is updated and *carried out* on the next
    /// tick, from wherever the value currently is.
    restart_pending: bool,
}

impl<T> Default for AnimatedState<T> {
    fn default() -> AnimatedState<T> {
        AnimatedState {
            from: None,
            to: None,
            started_micros: None,
            now_micros: 0,
            restart_pending: false,
        }
    }
}

impl<T: Lerp> AnimatedState<T> {
    /// How far through the current flight, 0..1. One when there is none.
    fn progress(&self, duration: Duration) -> f32 {
        let Some(started) = self.started_micros else {
            return 1.0;
        };
        let total = duration.as_micros() as f32;
        if total <= 0.0 {
            return 1.0;
        }
        (((self.now_micros - started).max(0) as f32) / total).clamp(0.0, 1.0)
    }

    /// The value to draw this frame.
    pub fn value(&self, duration: Duration, curve: Curve) -> Option<T> {
        let from = self.from?;
        let to = self.to?;
        Some(T::lerp(from, to, curve.transform(self.progress(duration))))
    }
}

/// A value that animates to wherever it is put.
///
/// Upstream's `TweenAnimationBuilder`: hand it a target and a way to build
/// something from a value, and it builds from a value that walks to the
/// target whenever the target moves.
pub struct Animated<T: Lerp, F> {
    target: T,
    duration: Duration,
    curve: Curve,
    build: F,
    key: Key,
}

impl<T: Lerp, F> Animated<T, F>
where
    F: Fn(T) -> AnyWidget + 'static,
{
    pub fn new(target: T, duration: Duration, build: F) -> Animated<T, F> {
        Animated {
            target,
            duration,
            curve: Curve::EASE_IN_OUT,
            build,
            key: None,
        }
    }

    pub fn with_curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }

    /// Keeps this animation's state apart from a sibling's. Without one, two
    /// animated values in the same position share an element and therefore
    /// each other's flight.
    pub fn with_key(mut self, key: u64) -> Self {
        self.key = Some(key);
        self
    }
}

impl<T: Lerp, F> StatefulComponent for Animated<T, F>
where
    F: Fn(T) -> AnyWidget + 'static,
{
    type State = AnimatedState<T>;

    fn key(&self) -> Key {
        self.key
    }

    fn did_update_widget(&self, _old: &Self, state: &mut Self::State) {
        // Upstream's `ImplicitlyAnimatedWidgetState.didUpdateWidget`: the
        // tweens are rebuilt only when the configuration actually moved
        // (`_constructTweens`), and the flight restarts from the value on
        // screen. Curve and duration changes need no flag of their own here
        // because both are read live when the value is evaluated, which is the
        // same effect upstream gets from re-arming its controller.
        state.restart_pending = state.to.is_some_and(|to| to != self.target);
    }

    fn advance(&self, state: &mut Self::State, frame_time_micros: i64) -> bool {
        // First frame: no flight, sitting on the target. An implicit animation
        // does not animate its way in from nowhere.
        if state.to.is_none() {
            state.from = Some(self.target);
            state.to = Some(self.target);
            state.now_micros = frame_time_micros;
            state.restart_pending = false;
            return false;
        }

        if state.restart_pending {
            // The target moved while the widget was updated. Start from
            // wherever this is *now*, not from where the last flight was
            // heading -- see the module comment.
            state.restart_pending = false;
            let current = state
                .value(self.duration, self.curve)
                .unwrap_or(self.target);
            state.from = Some(current);
            state.to = Some(self.target);
            state.started_micros = Some(frame_time_micros);
        }

        state.now_micros = frame_time_micros;
        let was_flying = state.started_micros.is_some();
        let flying = was_flying && state.progress(self.duration) < 1.0;
        if was_flying && !flying {
            // Landed on this frame. Dropping the flight stops the asking, but
            // the answer is still yes: this return is also what marks the
            // widget for rebuilding, and the frame that lands is the frame
            // that shows the target. Saying no here would leave every implicit
            // animation stopped one frame short of where it was sent.
            state.started_micros = None;
            return true;
        }
        flying
    }

    fn build(
        &self,
        state: &Self::State,
        _handle: StateHandle<Self::State>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        (self.build)(
            state
                .value(self.duration, self.curve)
                .unwrap_or(self.target),
        )
    }
}

/// [`Animated`] as a widget.
pub fn animated<T: Lerp, F>(target: T, duration: Duration, curve: Curve, build: F) -> AnyWidget
where
    F: Fn(T) -> AnyWidget + 'static,
{
    stateful(Animated::new(target, duration, build).with_curve(curve))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::{ElementTree, leaf};
    use crate::widgets::SizedBox;

    /// Mounts an animated width and returns the tree, so a test can drive
    /// frames through it the way the host does.
    fn tree_of(target: f32, cell: std::rc::Rc<std::cell::Cell<f32>>) -> AnyWidget {
        stateful(
            Animated::new(target, Duration::from_millis(100), move |width: f32| {
                cell.set(width);
                leaf(move || SizedBox::new(width, 10.0))
            })
            .with_curve(Curve::Linear),
        )
    }

    #[test]
    fn a_value_that_never_changes_never_animates() {
        let seen = std::rc::Rc::new(std::cell::Cell::new(0.0));
        let mut tree = ElementTree::new();
        tree.rebuild(tree_of(50.0, seen.clone()));
        assert!(!tree.advance_frame(1_000_000), "nothing to animate");
        tree.rebuild(tree_of(50.0, seen.clone()));
        assert!(!tree.advance_frame(1_016_000));
        assert_eq!(seen.get(), 50.0);
    }

    #[test]
    fn a_changed_target_is_walked_to_rather_than_jumped_to() {
        let seen = std::rc::Rc::new(std::cell::Cell::new(0.0));
        let mut tree = ElementTree::new();
        tree.rebuild(tree_of(0.0, seen.clone()));
        tree.advance_frame(1_000_000);
        tree.rebuild(tree_of(0.0, seen.clone()));

        // The target moves to 100 over 100ms.
        tree.rebuild(tree_of(100.0, seen.clone()));
        assert!(
            tree.advance_frame(2_000_000),
            "a moved target should want frames"
        );
        tree.rebuild_dirty();
        assert_eq!(seen.get(), 0.0, "starts where it was");

        assert!(tree.advance_frame(2_050_000));
        tree.rebuild_dirty();
        assert!(
            (seen.get() - 50.0).abs() < 1.0,
            "halfway through should be halfway there, not {}",
            seen.get()
        );

        // The frame that lands still wants a rebuild -- it is the one that
        // shows the target -- and only the frame after it is idle.
        assert!(
            tree.advance_frame(2_100_000),
            "the landing frame has to be drawn"
        );
        tree.rebuild_dirty();
        assert_eq!(seen.get(), 100.0);
        assert!(!tree.advance_frame(2_116_000), "and then it stops asking");
    }

    #[test]
    fn a_target_that_moves_again_carries_on_from_where_it_is() {
        let seen = std::rc::Rc::new(std::cell::Cell::new(0.0));
        let mut tree = ElementTree::new();
        tree.rebuild(tree_of(0.0, seen.clone()));
        tree.advance_frame(1_000_000);

        tree.rebuild(tree_of(100.0, seen.clone()));
        tree.advance_frame(2_000_000);
        tree.advance_frame(2_050_000);
        tree.rebuild_dirty();
        let halfway = seen.get();

        // Interrupted at the halfway point and sent somewhere else. The bug
        // this rules out: starting the new flight from the old *beginning*,
        // which makes the value jump backwards before setting off again.
        tree.rebuild(tree_of(0.0, seen.clone()));
        tree.advance_frame(2_060_000);
        tree.rebuild_dirty();
        assert!(
            (seen.get() - halfway).abs() < 6.0,
            "jumped from {halfway} to {} instead of carrying on",
            seen.get()
        );
    }

    #[test]
    fn colours_are_interpolated_channel_by_channel() {
        let black = Color(0xff000000);
        let white = Color(0xffffffff);
        let grey = Color::lerp(black, white, 0.5);
        assert_eq!(grey.0 & 0xff, 0x80);
        assert_eq!(
            (grey.0 >> 24) & 0xff,
            0xff,
            "the alpha is not a colour channel to skip"
        );
    }

    #[test]
    fn an_instant_animation_is_allowed() {
        // A zero duration would be a division; it has to land immediately
        // instead of producing an infinity.
        let state = AnimatedState {
            from: Some(0.0f32),
            to: Some(10.0f32),
            started_micros: Some(0),
            now_micros: 5,
            restart_pending: false,
        };
        assert_eq!(state.value(Duration::ZERO, Curve::Linear), Some(10.0));
    }

    // -- Which end a `Lerp` runs from ---------------------------------------
    //
    // Every one of these is a field-by-field walk, and each field is its own
    // line that can be written backwards on its own. A quarter of the way is
    // used rather than a half, because a lerp is symmetric at the midpoint
    // and a test there cannot tell `lerp(a, b, t)` from `lerp(b, a, t)`.

    #[test]
    fn an_offset_lerps_each_axis_from_the_first_towards_the_second() {
        let from = Offset::new(4.0, 20.0);
        let to = Offset::new(20.0, 4.0);
        // The two axes move in opposite directions on purpose: a line that
        // read the wrong field would land on the other axis's answer.
        assert_eq!(Offset::lerp(from, to, 0.25), Offset::new(8.0, 16.0));
        assert_eq!(Offset::lerp(to, from, 0.25), Offset::new(16.0, 8.0));
    }

    #[test]
    fn a_size_lerps_each_dimension_from_the_first_towards_the_second() {
        let from = Size::new(4.0, 20.0);
        let to = Size::new(20.0, 4.0);
        assert_eq!(Size::lerp(from, to, 0.25), Size::new(8.0, 16.0));
        assert_eq!(Size::lerp(to, from, 0.25), Size::new(16.0, 8.0));
    }

    #[test]
    fn edge_insets_lerp_all_four_edges_from_the_first_towards_the_second() {
        // Four different pairs, so a line reading the wrong edge fails as
        // well as a line reading the wrong direction.
        let from = EdgeInsets {
            left: 4.0,
            top: 8.0,
            right: 12.0,
            bottom: 16.0,
        };
        let to = EdgeInsets {
            left: 20.0,
            top: 24.0,
            right: 28.0,
            bottom: 32.0,
        };
        let quarter = EdgeInsets::lerp(from, to, 0.25);
        assert_eq!(
            (quarter.left, quarter.top, quarter.right, quarter.bottom),
            (8.0, 12.0, 16.0, 20.0)
        );
        let back = EdgeInsets::lerp(to, from, 0.25);
        assert_eq!(
            (back.left, back.top, back.right, back.bottom),
            (16.0, 20.0, 24.0, 28.0)
        );
    }

}
