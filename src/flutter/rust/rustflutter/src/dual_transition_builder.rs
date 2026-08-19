// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Two different transitions, one for each direction (upstream
//! `widgets/dual_transition_builder.dart`).
//!
//! A page that slides in from the right and *fades* out is two animations,
//! not one played backwards. `DualTransitionBuilder` takes both and runs
//! whichever the direction calls for.
//!
//! # The interesting part
//!
//! What happens when a transition is interrupted half way. The obvious answer
//! -- switch to the other builder -- is wrong: the forward and reverse
//! transitions look nothing alike, so a page caught mid-slide would jump into
//! a fade from wherever it had got to. Upstream instead keeps playing the
//! *ongoing* animation backwards, and only lets the direction change once the
//! animation has reached one end or the other.
//!
//! [`effective_animation_status`] is that rule, and it is the whole of this
//! file worth reading.

use std::rc::Rc;

use crate::animation::{
    AlwaysStoppedAnimation, Animation, AnimationStatus, ProxyAnimation, ReverseAnimation,
};

/// Upstream's `_calculateEffectiveAnimationStatus`: which direction the
/// builders should behave as though the animation is going.
///
/// Not the animation's own status. The two differ exactly when a transition
/// was interrupted: the animation says it is now going forwards, and this
/// says it is still the reverse transition being played -- backwards.
///
/// * A finished animation, either end, is taken at face value: there is
///   nothing in flight to keep playing.
/// * A change of direction mid-flight is refused, and the last effective
///   direction stands.
pub fn effective_animation_status(
    last_effective: AnimationStatus,
    current: AnimationStatus,
) -> AnimationStatus {
    match current {
        // Both ends are the truth: whatever was in flight has landed.
        AnimationStatus::Dismissed | AnimationStatus::Completed => current,
        AnimationStatus::Forward => match last_effective {
            AnimationStatus::Dismissed | AnimationStatus::Completed | AnimationStatus::Forward => {
                current
            }
            // Interrupted: the reverse transition keeps playing, backwards.
            AnimationStatus::Reverse => last_effective,
        },
        AnimationStatus::Reverse => match last_effective {
            AnimationStatus::Dismissed | AnimationStatus::Completed | AnimationStatus::Reverse => {
                current
            }
            AnimationStatus::Forward => last_effective,
        },
    }
}

/// Which animation each builder should be given, for one effective status.
///
/// Upstream's `_updateAnimations` sets the two proxies; this is the same
/// decision as a pair of values, so that it can be checked without a tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DualTransitionPhase {
    /// The forward builder drives from the real animation, and the reverse
    /// builder is pinned at dismissed -- it is not on screen.
    Forward,
    /// The reverse builder drives from the animation *reversed*, and the
    /// forward builder is pinned at complete: it has finished its job and
    /// must stay finished rather than winding back.
    Reverse,
}

impl DualTransitionPhase {
    /// Upstream's `_updateAnimations`, as an answer rather than an effect.
    pub fn of(effective: AnimationStatus) -> DualTransitionPhase {
        match effective {
            AnimationStatus::Dismissed | AnimationStatus::Forward => DualTransitionPhase::Forward,
            AnimationStatus::Reverse | AnimationStatus::Completed => DualTransitionPhase::Reverse,
        }
    }
}

/// Upstream `DualTransitionBuilder`.
///
/// # Recorded divergences
///
/// * Upstream is a `StatefulWidget` whose state listens to the animation's
///   status and re-points two `ProxyAnimation`s. The state here is the
///   effective status, which [`advance`](DualTransitionBuilder::advance)
///   updates -- this crate drives animation from a per-frame `advance` rather
///   than from listeners, as every other transition in it does.
/// * Upstream's builders take a `child` and are free to ignore it. The same
///   here: the reverse builder wraps the child and the forward builder wraps
///   that, which is upstream's nesting order and the reason a page can slide
///   *and* fade at once.
pub struct DualTransitionBuilder {
    animation: Rc<dyn Animation>,
    effective: AnimationStatus,
    forward_animation: ProxyAnimation,
    reverse_animation: ProxyAnimation,
}

impl DualTransitionBuilder {
    pub fn new(animation: Rc<dyn Animation>) -> DualTransitionBuilder {
        let effective = animation.status();
        let builder = DualTransitionBuilder {
            animation,
            effective,
            forward_animation: ProxyAnimation::new(),
            reverse_animation: ProxyAnimation::new(),
        };
        builder.point_animations();
        builder
    }

    /// The effective status, which is what the builders behave as though the
    /// animation is doing.
    pub fn effective_status(&self) -> AnimationStatus {
        self.effective
    }

    pub fn phase(&self) -> DualTransitionPhase {
        DualTransitionPhase::of(self.effective)
    }

    /// What the forward builder should be given.
    pub fn forward_animation(&self) -> &ProxyAnimation {
        &self.forward_animation
    }

    /// What the reverse builder should be given.
    pub fn reverse_animation(&self) -> &ProxyAnimation {
        &self.reverse_animation
    }

    /// Upstream's `_animationListener`: takes the animation's current status
    /// and works out whether the effective one moved.
    ///
    /// Answers whether it did, which is upstream's `if (oldEffective !=
    /// _effectiveAnimationStatus)` -- the animations are re-pointed only on a
    /// change, so a frame that did not change direction costs nothing.
    pub fn advance(&mut self) -> bool {
        let old = self.effective;
        self.effective = effective_animation_status(self.effective, self.animation.status());
        if old != self.effective {
            self.point_animations();
            return true;
        }
        false
    }

    /// Upstream `didUpdateWidget`'s animation branch: a new animation is
    /// asked its status straight away, rather than waiting for it to change.
    pub fn set_animation(&mut self, animation: Rc<dyn Animation>) {
        self.animation = animation;
        self.effective = effective_animation_status(self.effective, self.animation.status());
        self.point_animations();
    }

    /// Upstream `_updateAnimations`.
    fn point_animations(&self) {
        match self.phase() {
            DualTransitionPhase::Forward => {
                self.forward_animation
                    .set_parent(Some(Rc::clone(&self.animation)));
                // Upstream's `kAlwaysDismissedAnimation`: the reverse
                // transition is not on screen, and pinning it at zero is what
                // says so.
                self.reverse_animation
                    .set_parent(Some(Rc::new(AlwaysStoppedAnimation { value: 0.0 })));
            }
            DualTransitionPhase::Reverse => {
                // Upstream's `kAlwaysCompleteAnimation`: the forward
                // transition finished, and it must stay finished rather than
                // winding back while the reverse one plays.
                self.forward_animation
                    .set_parent(Some(Rc::new(AlwaysStoppedAnimation { value: 1.0 })));
                self.reverse_animation
                    .set_parent(Some(Rc::new(ReverseAnimation::new(Rc::clone(
                        &self.animation,
                    )))));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// An animation whose status the test moves by hand.
    struct Driven {
        status: Cell<AnimationStatus>,
        value: Cell<f32>,
    }

    impl Driven {
        fn new(status: AnimationStatus, value: f32) -> Rc<Driven> {
            Rc::new(Driven {
                status: Cell::new(status),
                value: Cell::new(value),
            })
        }
    }

    impl Animation for Driven {
        fn value(&self) -> f32 {
            self.value.get()
        }

        fn status(&self) -> AnimationStatus {
            self.status.get()
        }

        fn add_listener(&self, _listener: crate::animation::AnimationListener) {}
        fn remove_listener(&self, _listener: &crate::animation::AnimationListener) {}
    }

    #[test]
    fn an_animation_that_has_landed_is_taken_at_face_value() {
        // Nothing is in flight, so there is nothing to keep playing: both
        // ends override whatever came before.
        for last in [
            AnimationStatus::Dismissed,
            AnimationStatus::Forward,
            AnimationStatus::Reverse,
            AnimationStatus::Completed,
        ] {
            assert_eq!(
                effective_animation_status(last, AnimationStatus::Completed),
                AnimationStatus::Completed
            );
            assert_eq!(
                effective_animation_status(last, AnimationStatus::Dismissed),
                AnimationStatus::Dismissed
            );
        }
    }

    #[test]
    fn an_interrupted_transition_keeps_playing_the_one_that_was_running() {
        // The whole point of the file. A reverse transition interrupted by a
        // forward one keeps being the reverse transition -- played backwards
        // -- because the two look nothing alike and switching mid-flight
        // would jump the page from a slide into a fade.
        assert_eq!(
            effective_animation_status(AnimationStatus::Reverse, AnimationStatus::Forward),
            AnimationStatus::Reverse,
            "still the reverse transition, now running forwards"
        );
        assert_eq!(
            effective_animation_status(AnimationStatus::Forward, AnimationStatus::Reverse),
            AnimationStatus::Forward
        );
    }

    #[test]
    fn a_transition_started_from_rest_is_the_direction_it_started_in() {
        // Nothing was interrupted, so the animation's own direction is the
        // effective one.
        assert_eq!(
            effective_animation_status(AnimationStatus::Dismissed, AnimationStatus::Forward),
            AnimationStatus::Forward
        );
        assert_eq!(
            effective_animation_status(AnimationStatus::Completed, AnimationStatus::Reverse),
            AnimationStatus::Reverse
        );
        // And from the "wrong" end, too: a dismissed animation asked to
        // reverse is a reverse transition, odd as that is.
        assert_eq!(
            effective_animation_status(AnimationStatus::Dismissed, AnimationStatus::Reverse),
            AnimationStatus::Reverse
        );
    }

    #[test]
    fn the_direction_can_change_once_the_animation_reaches_an_end() {
        // The interruption is only refused while something is in flight.
        // Landing at an end clears it, and the next direction is taken.
        let mut effective = AnimationStatus::Forward;
        effective = effective_animation_status(effective, AnimationStatus::Reverse);
        assert_eq!(effective, AnimationStatus::Forward, "refused mid-flight");
        effective = effective_animation_status(effective, AnimationStatus::Completed);
        assert_eq!(effective, AnimationStatus::Completed);
        effective = effective_animation_status(effective, AnimationStatus::Reverse);
        assert_eq!(effective, AnimationStatus::Reverse, "accepted now");
    }

    #[test]
    fn a_completed_animation_is_the_reverse_phase_and_a_dismissed_one_forward() {
        // Which reads oddly until you see why: at rest at the far end, the
        // thing on screen is what the *reverse* builder will animate away, so
        // that is the builder that has to be live and holding it.
        assert_eq!(
            DualTransitionPhase::of(AnimationStatus::Dismissed),
            DualTransitionPhase::Forward
        );
        assert_eq!(
            DualTransitionPhase::of(AnimationStatus::Forward),
            DualTransitionPhase::Forward
        );
        assert_eq!(
            DualTransitionPhase::of(AnimationStatus::Reverse),
            DualTransitionPhase::Reverse
        );
        assert_eq!(
            DualTransitionPhase::of(AnimationStatus::Completed),
            DualTransitionPhase::Reverse
        );
    }

    #[test]
    fn the_builder_that_is_not_running_is_pinned_rather_than_left_alone() {
        // Pinned at the end it has already reached: the forward one at
        // complete while the reverse plays, the reverse one at dismissed
        // while the forward plays. Leaving either free would have it wind
        // back through its own animation under the one that is running.
        let animation = Driven::new(AnimationStatus::Forward, 0.4);
        let builder = DualTransitionBuilder::new(animation.clone());
        assert_eq!(builder.phase(), DualTransitionPhase::Forward);
        assert_eq!(builder.forward_animation().value(), 0.4);
        assert_eq!(builder.reverse_animation().value(), 0.0);

        let animation = Driven::new(AnimationStatus::Reverse, 0.4);
        let builder = DualTransitionBuilder::new(animation.clone());
        assert_eq!(builder.phase(), DualTransitionPhase::Reverse);
        assert_eq!(builder.forward_animation().value(), 1.0);
        assert_eq!(
            builder.reverse_animation().value(),
            0.6,
            "the reverse builder sees the animation reversed"
        );
    }

    #[test]
    fn advancing_reports_only_a_real_change_of_direction() {
        // Upstream re-points the proxies only when the effective status
        // moved, so a frame that changed nothing costs nothing.
        let animation = Driven::new(AnimationStatus::Forward, 0.2);
        let mut builder = DualTransitionBuilder::new(animation.clone());
        assert!(!builder.advance(), "the same status is not a change");

        // Interrupted: the animation turns round, and the effective status
        // does not.
        animation.status.set(AnimationStatus::Reverse);
        assert!(!builder.advance());
        assert_eq!(builder.effective_status(), AnimationStatus::Forward);

        // It lands, and now the effective status does move.
        animation.status.set(AnimationStatus::Dismissed);
        assert!(builder.advance());
        assert_eq!(builder.effective_status(), AnimationStatus::Dismissed);
    }

    #[test]
    fn a_replacement_animation_is_asked_its_status_at_once() {
        // Upstream's `didUpdateWidget` calls the listener with the new
        // animation's status rather than waiting for it to change -- without
        // that, a swapped-in animation that is already running would be
        // treated as though it were still at the old one's status.
        let first = Driven::new(AnimationStatus::Dismissed, 0.0);
        let mut builder = DualTransitionBuilder::new(first);
        assert_eq!(builder.phase(), DualTransitionPhase::Forward);

        let second = Driven::new(AnimationStatus::Completed, 1.0);
        builder.set_animation(second);
        assert_eq!(builder.effective_status(), AnimationStatus::Completed);
        assert_eq!(builder.phase(), DualTransitionPhase::Reverse);
    }
}
