//! A port of `widgets/stretch_effect.dart`.
//!
//! What [`crate::overscroll_indicator::StretchingOverscrollIndicator`] hands
//! its strength to. The interesting part is that there are **two
//! implementations of the same effect**, chosen at runtime by whether the
//! engine can run a shader filter.

use crate::direction::TextDirection;
use crate::render::Axis;

/// Where the stretch is anchored, in upstream's `AlignmentDirectional` terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StretchAlignment {
    TopCenter,
    BottomCenter,
    CenterStart,
    CenterEnd,
}

/// Which of the two implementations is in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StretchImplementation {
    /// A real non-uniform mesh deformation, which is what Android does: the
    /// content near the pulled edge stretches more than the content far from
    /// it. Needs a shader filter, which today means Impeller.
    Shader,
    /// A plain uniform scale, anchored at the **far** edge so the content grows
    /// away from the one being pulled. Every pixel moves by the same
    /// proportion, which is not what the platform does but is recognisably the
    /// same gesture.
    UniformTransform,
}

/// Upstream `StretchEffect`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StretchEffect {
    /// Between -1 and 1 inclusive. The sign is the direction: on the vertical
    /// axis, positive pulls downward from the top.
    pub stretch_strength: f32,
    pub axis: Axis,
}

impl StretchEffect {
    pub fn new(axis: Axis) -> StretchEffect {
        StretchEffect {
            stretch_strength: 0.0,
            axis,
        }
    }

    pub fn with_strength(mut self, strength: f32) -> Self {
        debug_assert!(
            (-1.0..=1.0).contains(&strength),
            "stretchStrength must be between -1.0 and 1.0"
        );
        self.stretch_strength = strength;
        self
    }

    pub fn is_valid(&self) -> bool {
        (-1.0..=1.0).contains(&self.stretch_strength)
    }

    /// Upstream's `build` picks by `ui.ImageFilter.isShaderFilterSupported`.
    pub fn implementation(shader_filter_supported: bool) -> StretchImplementation {
        if shader_filter_supported {
            StretchImplementation::Shader
        } else {
            StretchImplementation::UniformTransform
        }
    }

    /// Upstream `_getAlignment`.
    ///
    /// The anchor is the edge **opposite** the pull, so the content grows away
    /// from the finger rather than out from under it. The vertical axis has no
    /// reading direction to consult; the horizontal one does, and the two swap
    /// in right-to-left.
    pub fn alignment(&self, direction: TextDirection) -> StretchAlignment {
        let forward = self.stretch_strength > 0.0;
        match self.axis {
            Axis::Vertical => {
                if forward {
                    StretchAlignment::TopCenter
                } else {
                    StretchAlignment::BottomCenter
                }
            }
            Axis::Horizontal => match (direction, forward) {
                (TextDirection::Rtl, true) => StretchAlignment::CenterEnd,
                (TextDirection::Rtl, false) => StretchAlignment::CenterStart,
                (TextDirection::Ltr, true) => StretchAlignment::CenterStart,
                (TextDirection::Ltr, false) => StretchAlignment::CenterEnd,
            },
        }
    }

    /// The fallback's scale, as `(x, y)`. Only the stretched axis grows, and it
    /// grows by the **magnitude** -- the sign already went into the anchor.
    pub fn scale(&self) -> (f32, f32) {
        match self.axis {
            Axis::Horizontal => (1.0 + self.stretch_strength.abs(), 1.0),
            Axis::Vertical => (1.0, 1.0 + self.stretch_strength.abs()),
        }
    }

    /// Whether the fallback asks for a filter quality.
    ///
    /// Upstream passes `null` at a strength of zero, and that is not a
    /// micro-optimisation: a `Transform` with a filter quality set is a raster
    /// operation even when the matrix is the identity, so an unstretched list
    /// would pay for a resample on every frame.
    pub fn uses_filter_quality(&self) -> bool {
        self.stretch_strength != 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_effect_has_two_implementations_chosen_at_runtime() {
        // One is a real non-uniform mesh deformation, the other a uniform
        // scale. They are the same gesture, not the same picture.
        assert_eq!(
            StretchEffect::implementation(true),
            StretchImplementation::Shader
        );
        assert_eq!(
            StretchEffect::implementation(false),
            StretchImplementation::UniformTransform
        );
    }

    #[test]
    fn the_content_grows_away_from_the_edge_being_pulled() {
        // Anchored at the far end, so it does not slide out from under the
        // finger.
        let down = StretchEffect::new(Axis::Vertical).with_strength(0.4);
        assert_eq!(
            down.alignment(TextDirection::Ltr),
            StretchAlignment::TopCenter
        );

        let up = StretchEffect::new(Axis::Vertical).with_strength(-0.4);
        assert_eq!(
            up.alignment(TextDirection::Ltr),
            StretchAlignment::BottomCenter
        );
    }

    #[test]
    fn only_the_horizontal_axis_has_a_reading_direction_to_consult() {
        let right = StretchEffect::new(Axis::Horizontal).with_strength(0.4);
        assert_eq!(
            right.alignment(TextDirection::Ltr),
            StretchAlignment::CenterStart
        );
        assert_eq!(
            right.alignment(TextDirection::Rtl),
            StretchAlignment::CenterEnd
        );

        let vertical = StretchEffect::new(Axis::Vertical).with_strength(0.4);
        assert_eq!(
            vertical.alignment(TextDirection::Ltr),
            vertical.alignment(TextDirection::Rtl),
            "there is no such thing as a bottom-to-top locale"
        );
    }

    #[test]
    fn only_the_stretched_axis_grows_and_it_grows_by_the_magnitude() {
        // The sign already went into the anchor.
        let down = StretchEffect::new(Axis::Vertical).with_strength(0.4);
        let up = StretchEffect::new(Axis::Vertical).with_strength(-0.4);
        assert_eq!(down.scale(), (1.0, 1.4));
        assert_eq!(up.scale(), down.scale());
        assert_ne!(
            down.alignment(TextDirection::Ltr),
            up.alignment(TextDirection::Ltr)
        );

        let sideways = StretchEffect::new(Axis::Horizontal).with_strength(0.4);
        assert_eq!(sideways.scale(), (1.4, 1.0));
    }

    #[test]
    fn an_unstretched_list_does_not_pay_for_a_resample_on_every_frame() {
        // A Transform with a filter quality set is a raster operation even at
        // the identity matrix.
        let still = StretchEffect::new(Axis::Vertical);
        assert_eq!(still.scale(), (1.0, 1.0));
        assert!(!still.uses_filter_quality());

        assert!(
            StretchEffect::new(Axis::Vertical)
                .with_strength(0.01)
                .uses_filter_quality()
        );
    }

    #[test]
    fn the_strength_is_a_fraction_and_the_ends_are_included() {
        for strength in [-1.0, 0.0, 1.0] {
            assert!(
                StretchEffect {
                    stretch_strength: strength,
                    axis: Axis::Vertical
                }
                .is_valid()
            );
        }
        assert!(
            !StretchEffect {
                stretch_strength: 1.5,
                axis: Axis::Vertical
            }
            .is_valid()
        );
    }
}
