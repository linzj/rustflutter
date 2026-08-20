//! The modal barrier -- a port of upstream's `widgets/modal_barrier.dart`.
//!
//! A barrier is the sheet of nothing between a dialog and the page behind it.
//! It has no appearance worth the name -- a colour, sometimes not even that --
//! and its whole job is to **absorb**: taps, so the page behind cannot be
//! operated; and semantics, so a screen reader does not read out a page the
//! reader cannot reach.
//!
//! The interesting part is that those two are configured **separately**. A
//! barrier can be dismissible by touch and still not offer itself to a screen
//! reader as something to dismiss, and upstream ships exactly that
//! combination as a default in places.

use crate::engine::Color;

/// Upstream `ModalBarrier`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModalBarrier {
    /// `None` paints nothing at all -- a barrier that blocks without dimming.
    /// A menu's barrier is usually one of these: it has to catch the tap that
    /// closes the menu, and darkening the page for a menu would be heavy.
    pub color: Option<Color>,
    /// Upstream's `dismissible`, **true** by default.
    pub dismissible: bool,
    /// Upstream's `semanticsLabel`, announced when the barrier is dismissible.
    /// A control a reader can activate needs a name.
    pub semantics_label: Option<String>,
    /// Upstream's `barrierSemanticsDismissible`, and its doc says the part
    /// that matters: **this field is ignored if `dismissible` is false**.
    ///
    /// So the two are not independent in both directions. A barrier that
    /// cannot be dismissed is never offered to a screen reader as
    /// dismissible; a barrier that can be may still be withheld, which is what
    /// a route does when there is a better way out that the reader should be
    /// steered towards.
    pub barrier_semantics_dismissible: Option<bool>,
    /// Upstream's `semanticsOnTapHint`, which fills in the "double tap to
    /// **dismiss**" part rather than replacing the whole announcement.
    pub semantics_on_tap_hint: Option<String>,
}

impl ModalBarrier {
    pub fn new() -> ModalBarrier {
        ModalBarrier {
            color: None,
            dismissible: true,
            semantics_label: None,
            barrier_semantics_dismissible: None,
            semantics_on_tap_hint: None,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_dismissible(mut self, dismissible: bool) -> Self {
        self.dismissible = dismissible;
        self
    }

    pub fn with_semantics_label(mut self, label: impl Into<String>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    pub fn with_barrier_semantics_dismissible(mut self, dismissible: bool) -> Self {
        self.barrier_semantics_dismissible = Some(dismissible);
        self
    }

    /// Whether a tap on the barrier dismisses the route.
    pub fn absorbs_taps(&self) -> bool {
        true
    }

    /// Whether the barrier appears in the semantics tree as something to
    /// dismiss.
    ///
    /// The `dismissible` check comes first and is the reason the pair is not
    /// symmetric: there is nothing to offer for a barrier that does nothing.
    pub fn is_semantically_dismissible(&self) -> bool {
        self.dismissible && self.barrier_semantics_dismissible.unwrap_or(true)
    }

    /// Whether the barrier paints anything.
    pub fn paints(&self) -> bool {
        self.color.is_some()
    }
}

/// Upstream `AnimatedModalBarrier`: the same barrier with an animated colour.
///
/// It is a separate class rather than a nullable field on the first because
/// the two take genuinely different arguments -- one a `Color`, the other an
/// `Animation<Color?>` -- and folding them together would leave every caller
/// passing a constant animation.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimatedModalBarrier {
    pub barrier: ModalBarrier,
}

impl AnimatedModalBarrier {
    pub fn new(barrier: ModalBarrier) -> AnimatedModalBarrier {
        AnimatedModalBarrier { barrier }
    }

    /// The colour at a point in the route's animation, which is what makes the
    /// scrim fade in with the dialog rather than appearing under it.
    pub fn color_at(&self, from: Option<Color>, to: Option<Color>, t: f32) -> Option<Color> {
        match (from, to) {
            (None, None) => None,
            (Some(from), Some(to)) => Some(lerp_color(from, to, t)),
            (None, Some(to)) => Some(lerp_color(to.with_alpha(0), to, t)),
            (Some(from), None) => Some(lerp_color(from, from.with_alpha(0), t)),
        }
    }
}

fn lerp_channel(from: u8, to: u8, t: f32) -> u8 {
    (from as f32 + (to as f32 - from as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    Color::argb(
        lerp_channel(from.alpha(), to.alpha(), t),
        lerp_channel(from.red(), to.red(), t),
        lerp_channel(from.green(), to.green(), t),
        lerp_channel(from.blue(), to.blue(), t),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_barrier_absorbs_taps_whether_or_not_it_dismisses() {
        // Blocking the page behind is the job; dismissing is an extra.
        assert!(ModalBarrier::new().absorbs_taps());
        assert!(ModalBarrier::new().with_dismissible(false).absorbs_taps());
    }

    #[test]
    fn a_barrier_that_does_nothing_is_never_offered_to_a_screen_reader() {
        // Which is why the two flags are not symmetric: there is nothing to
        // offer for a barrier that cannot be dismissed.
        let inert = ModalBarrier::new()
            .with_dismissible(false)
            .with_barrier_semantics_dismissible(true);
        assert!(!inert.is_semantically_dismissible());
    }

    #[test]
    fn a_dismissible_barrier_may_still_be_withheld_from_a_screen_reader() {
        // Which a route does when there is a better way out that the reader
        // should be steered towards.
        let touch_only = ModalBarrier::new().with_barrier_semantics_dismissible(false);
        assert!(touch_only.dismissible);
        assert!(!touch_only.is_semantically_dismissible());

        assert!(
            ModalBarrier::new().is_semantically_dismissible(),
            "and unspecified means yes"
        );
    }

    #[test]
    fn a_barrier_with_no_colour_blocks_without_dimming() {
        // A menu's barrier is one of these: it catches the tap that closes the
        // menu, and darkening the page for a menu would be heavy.
        let clear = ModalBarrier::new();
        assert!(!clear.paints());
        assert!(clear.absorbs_taps());

        assert!(ModalBarrier::new().with_color(Color(0x8000_0000)).paints());
    }

    #[test]
    fn the_scrim_fades_in_with_the_dialog_rather_than_appearing_under_it() {
        let animated = AnimatedModalBarrier::new(ModalBarrier::new());
        let target = Color(0x8000_0000);

        let start = animated.color_at(None, Some(target), 0.0).unwrap();
        assert_eq!(start.alpha(), 0, "invisible at the start");

        let half = animated.color_at(None, Some(target), 0.5).unwrap();
        assert!(half.alpha() > 0 && half.alpha() < target.alpha());

        assert_eq!(animated.color_at(None, Some(target), 1.0), Some(target));
    }

    #[test]
    fn a_barrier_that_was_never_coloured_stays_uncoloured() {
        let animated = AnimatedModalBarrier::new(ModalBarrier::new());
        assert_eq!(animated.color_at(None, None, 0.5), None);
    }

    #[test]
    fn a_barrier_leaving_fades_back_out() {
        let animated = AnimatedModalBarrier::new(ModalBarrier::new());
        let from = Color(0x8000_0000);
        assert_eq!(animated.color_at(Some(from), None, 1.0).unwrap().alpha(), 0);
    }

    #[test]
    fn a_named_barrier_carries_its_label_and_hint_separately() {
        // The hint fills in the "double tap to _dismiss_" part rather than
        // replacing the whole announcement.
        let mut barrier = ModalBarrier::new().with_semantics_label("Dismiss");
        barrier.semantics_on_tap_hint = Some("close the menu".to_string());
        assert_eq!(barrier.semantics_label.as_deref(), Some("Dismiss"));
        assert_eq!(
            barrier.semantics_on_tap_hint.as_deref(),
            Some("close the menu")
        );
    }
}
