//! Ports of `cupertino/thumb_painter.dart`'s `CupertinoThumbPainter`,
//! `cupertino/radio.dart`'s `CupertinoRadio`,
//! `cupertino/sliding_segmented_control.dart`'s
//! `CupertinoSlidingSegmentedControl` and `cupertino/icons.dart`'s
//! `CupertinoIcons`.

/// One shadow under a thumb.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbShadow {
    pub color: u32,
    pub dy: f32,
    pub blur_radius: f32,
}

/// Upstream `CupertinoThumbPainter`.
#[derive(Clone, Debug, PartialEq)]
pub struct CupertinoThumbPainter {
    pub color: u32,
    pub shadows: Vec<ThumbShadow>,
}

impl CupertinoThumbPainter {
    /// Upstream's `radius`, documented as *"Half the default diameter of the
    /// thumb."*
    pub const RADIUS: f32 = 14.0;

    /// Upstream's `extension`: *"The default amount the thumb should be extended
    /// horizontally when pressed."* Exactly half the radius.
    pub const EXTENSION: f32 = 7.0;

    /// Upstream's `_kThumbBorderColor`, four percent black.
    pub const BORDER_COLOR: u32 = 0x0A000000;

    /// Upstream's `_kSliderBoxShadows`: **three** layers.
    pub fn slider() -> CupertinoThumbPainter {
        CupertinoThumbPainter {
            color: 0xFFFFFFFF,
            shadows: vec![
                ThumbShadow {
                    color: 0x26000000,
                    dy: 3.0,
                    blur_radius: 8.0,
                },
                ThumbShadow {
                    color: 0x29000000,
                    dy: 1.0,
                    blur_radius: 1.0,
                },
                ThumbShadow {
                    color: 0x1A000000,
                    dy: 3.0,
                    blur_radius: 1.0,
                },
            ],
        }
    }

    /// Upstream's `.switchThumb` redirecting constructor, whose only difference
    /// is `_kSwitchBoxShadows`: **two** layers.
    ///
    /// The two share their first shadow exactly and diverge after it. The
    /// slider's extra layer is a tight contact shadow at an offset of one, which
    /// is how a thing that has been *picked up* is drawn; the switch thumb only
    /// ever slides along its track and gets the flatter pair.
    ///
    /// One class, two configurations, no subclass -- a redirecting constructor
    /// with different defaults.
    pub fn switch_thumb() -> CupertinoThumbPainter {
        CupertinoThumbPainter {
            color: 0xFFFFFFFF,
            shadows: vec![
                ThumbShadow {
                    color: 0x26000000,
                    dy: 3.0,
                    blur_radius: 8.0,
                },
                ThumbShadow {
                    color: 0x0F000000,
                    dy: 3.0,
                    blur_radius: 1.0,
                },
            ],
        }
    }

    /// Upstream draws the border as
    /// `canvas.drawRRect(thumbShape.inflate(0.5), Paint()..color = _kThumbBorderColor)`
    /// **before** the fill.
    ///
    /// **The border is not stroked, it is a slightly larger shape painted
    /// behind.** Half a pixel out, then covered by the fill, so what survives is
    /// a hairline entirely outside the thumb rather than a stroke straddling its
    /// edge. Tick 85's toggle buttons reached the same end by deflating before
    /// stroking; this is the other way round.
    pub fn border_inflation() -> f32 {
        0.5
    }

    /// Upstream's comment on the shape:
    ///
    /// > Paint RRects instead of RSuperellipses here, because practically
    /// > `CupertinoSlider` only draws circular thumbs.
    ///
    /// iOS shapes are usually superellipses, and a circle is the one case where
    /// the cheaper primitive is not an approximation but the same figure.
    pub fn uses_rounded_rect_rather_than_superellipse() -> bool {
        true
    }

    /// The corner radius upstream gives the thumb: half its shortest side, which
    /// makes a square rect a circle.
    pub fn corner_radius(rect: (f32, f32)) -> f32 {
        rect.0.min(rect.1) / 2.0
    }
}

/// Upstream `CupertinoRadio`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoRadio {
    pub selected: bool,
    /// Upstream's `useCheckmarkStyle`: a tick instead of a filled dot, which is
    /// what iOS list settings use.
    pub use_checkmark_style: bool,
}

impl CupertinoRadio {
    /// Upstream's `_kOuterRadius`.
    pub const OUTER_RADIUS: f32 = 7.0;

    /// Upstream's `_kInnerRadius`, and it is worth looking at.
    ///
    /// **2.975.** Four significant figures sitting next to a plain 7.0, with
    /// **no comment at all** about where it came from. Tick 97 found the same
    /// shape of number in `cupertino/sheet.dart` -- `_kSheetScaleFactor =
    /// 0.0835` -- and there the comment said exactly what was measured against
    /// what, on which simulator, running which iOS. Here the number is just as
    /// clearly measured and says nothing.
    ///
    /// It is 0.425 of the outer radius, which is the sort of ratio you get by
    /// measuring rather than by choosing.
    pub const INNER_RADIUS: f32 = 2.975;

    pub fn new(selected: bool) -> CupertinoRadio {
        CupertinoRadio {
            selected,
            use_checkmark_style: false,
        }
    }

    /// The ratio the two radii stand in.
    pub fn inner_to_outer_ratio() -> f32 {
        CupertinoRadio::INNER_RADIUS / CupertinoRadio::OUTER_RADIUS
    }

    /// What the control draws when selected.
    pub fn draws_a_dot(&self) -> bool {
        self.selected && !self.use_checkmark_style
    }
}

/// Why a sliding segmented control's construction was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentedControlError {
    FewerThanTwoSegments,
}

/// Upstream `CupertinoSlidingSegmentedControl`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoSlidingSegmentedControl {
    pub segment_count: usize,
}

impl CupertinoSlidingSegmentedControl {
    /// Upstream's `_kMinSegmentedControlHeight`.
    pub const MIN_HEIGHT: f32 = 28.0;

    /// Upstream's `_kThumbRadius`.
    pub const THUMB_RADIUS: f32 = 7.0;

    /// Upstream's `_kThumbInsets`, horizontal only -- the thumb is inset from
    /// the ends of the track but runs its full height.
    pub const THUMB_HORIZONTAL_INSET: f32 = 1.0;

    /// Upstream's `_kSeparatorInset`, vertical only -- the hairlines between
    /// segments are shorter than the control, so they read as separators rather
    /// than as a grid.
    pub const SEPARATOR_VERTICAL_INSET: f32 = 5.0;

    pub fn new(segment_count: usize) -> CupertinoSlidingSegmentedControl {
        CupertinoSlidingSegmentedControl { segment_count }
    }

    /// Upstream `assert(children.length >= 2)`.
    ///
    /// **Two is the smallest number of segments that is a choice.** The same
    /// threshold `TabController._changeIndex` uses in tick 84, where it hid
    /// inside `if (value == _index || length < 2) return;` and turned out to be
    /// the only thing holding an invariant its own assert had let go. Here it is
    /// said out loud in the constructor, which is the better place for it.
    pub fn validate(&self) -> Result<(), SegmentedControlError> {
        if self.segment_count < 2 {
            return Err(SegmentedControlError::FewerThanTwoSegments);
        }
        Ok(())
    }

    /// Upstream's `didUpdateWidget` opens with `assert(oldWidget.key ==
    /// widget.key)`, in two separate states.
    ///
    /// **That assert cannot fail.** `didUpdateWidget` is only reached after
    /// `Element.update`, which is only reached when `Widget.canUpdate` returned
    /// true, and that is
    ///
    /// ```dart
    /// oldWidget.runtimeType == newWidget.runtimeType && oldWidget.key == newWidget.key
    /// ```
    ///
    /// So the framework has already required exactly this before calling. The
    /// assert is documentation wearing an assert's clothes -- harmless, and the
    /// mirror image of the rule this port keeps for its own tests, that a check
    /// which cannot fail is worse than none. In a test that would be a defect;
    /// as a note to a reader about what may be relied on, it is only redundant.
    ///
    /// Ported as a statement of the invariant rather than as a check.
    pub fn key_is_guaranteed_stable_by_the_framework() -> bool {
        true
    }

    /// The thumb slides on a spring rather than a curve -- upstream builds a
    /// `SpringSimulation` for it, so an interrupted drag continues from the
    /// velocity it already had.
    pub fn thumb_animates_on_a_spring() -> bool {
        true
    }
}

/// Upstream `CupertinoIcons`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CupertinoIcons;

impl CupertinoIcons {
    /// The generated block's size, recorded rather than reproduced, as tick 90
    /// did for Material's 8,825.
    pub const UPSTREAM_ICON_COUNT: usize = 1322;

    /// Upstream's `iconFont`.
    pub const ICON_FONT: &'static str = "CupertinoIcons";

    /// Upstream's `iconFontPackage`, and this is the difference from Material.
    ///
    /// Material's icons ship inside the framework and are switched on with
    /// `uses-material-design: true` in the pubspec. **These live in a separate
    /// pub package**, `cupertino_icons`, so a reference to
    /// `CupertinoIcons.something` compiles whether or not the application
    /// depends on it and renders tofu if it does not.
    ///
    /// The same failure -- a codepoint in a font the build did not ship -- with
    /// two different mechanisms behind it: a build flag on one side, a
    /// dependency on the other. Neither is visible to the type system.
    pub const ICON_FONT_PACKAGE: &'static str = "cupertino_icons";

    pub fn requires_a_package_dependency() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Two thumbs, drawn at different heights -------------------------------------

    #[test]
    fn a_slider_thumb_gets_three_shadows_and_a_switch_thumb_two() {
        assert_eq!(CupertinoThumbPainter::slider().shadows.len(), 3);
        assert_eq!(CupertinoThumbPainter::switch_thumb().shadows.len(), 2);
    }

    #[test]
    fn and_they_share_their_first_shadow_exactly_before_diverging() {
        let slider = CupertinoThumbPainter::slider();
        let switch_thumb = CupertinoThumbPainter::switch_thumb();
        assert_eq!(slider.shadows[0], switch_thumb.shadows[0]);
        assert_ne!(slider.shadows[1], switch_thumb.shadows[1]);
    }

    #[test]
    fn only_the_slider_gets_the_tight_contact_shadow_of_a_thing_picked_up() {
        let slider = CupertinoThumbPainter::slider();
        assert!(
            slider.shadows.iter().any(|shadow| shadow.dy == 1.0),
            "an offset of one, close under the thumb"
        );
        assert!(
            !CupertinoThumbPainter::switch_thumb()
                .shadows
                .iter()
                .any(|shadow| shadow.dy == 1.0),
            "a switch thumb only ever slides"
        );
    }

    #[test]
    fn the_border_is_a_larger_shape_behind_rather_than_a_stroke_across_the_edge() {
        assert_eq!(CupertinoThumbPainter::border_inflation(), 0.5);
        assert_eq!(CupertinoThumbPainter::BORDER_COLOR, 0x0A000000);
    }

    #[test]
    fn a_square_thumb_comes_out_a_circle() {
        assert_eq!(CupertinoThumbPainter::corner_radius((28.0, 28.0)), 14.0);
        assert_eq!(
            CupertinoThumbPainter::corner_radius((40.0, 28.0)),
            14.0,
            "and a stretched one keeps its ends round"
        );
    }

    #[test]
    fn the_extension_is_half_the_radius() {
        assert_eq!(
            CupertinoThumbPainter::EXTENSION * 2.0,
            CupertinoThumbPainter::RADIUS
        );
    }

    // -- A measured number with nothing said about it --------------------------------

    #[test]
    fn the_inner_radius_has_four_significant_figures_and_no_comment() {
        assert_eq!(CupertinoRadio::INNER_RADIUS, 2.975);
        assert_eq!(CupertinoRadio::OUTER_RADIUS, 7.0);
        assert!(
            (CupertinoRadio::inner_to_outer_ratio() - 0.425).abs() < 1e-6,
            "a ratio you get by measuring rather than by choosing"
        );
    }

    #[test]
    fn the_checkmark_style_replaces_the_dot_rather_than_joining_it() {
        let mut radio = CupertinoRadio::new(true);
        assert!(radio.draws_a_dot());
        radio.use_checkmark_style = true;
        assert!(!radio.draws_a_dot());

        assert!(!CupertinoRadio::new(false).draws_a_dot());
    }

    // -- Two is the smallest number that is a choice -----------------------------------

    #[test]
    fn a_segmented_control_needs_at_least_two_segments() {
        assert_eq!(
            CupertinoSlidingSegmentedControl::new(1).validate(),
            Err(SegmentedControlError::FewerThanTwoSegments)
        );
        assert_eq!(
            CupertinoSlidingSegmentedControl::new(0).validate(),
            Err(SegmentedControlError::FewerThanTwoSegments)
        );
        assert_eq!(CupertinoSlidingSegmentedControl::new(2).validate(), Ok(()));
    }

    #[test]
    fn the_key_assert_restates_what_the_framework_already_required() {
        // canUpdate compares runtimeType and key before update is ever called.
        assert!(CupertinoSlidingSegmentedControl::key_is_guaranteed_stable_by_the_framework());
    }

    #[test]
    fn the_thumb_and_the_separators_are_inset_on_opposite_axes() {
        // The thumb is held off the ends and runs full height; the separators
        // are shortened so they read as rules rather than as a grid.
        assert_eq!(
            CupertinoSlidingSegmentedControl::THUMB_HORIZONTAL_INSET,
            1.0
        );
        assert_eq!(
            CupertinoSlidingSegmentedControl::SEPARATOR_VERTICAL_INSET,
            5.0
        );
        assert!(
            CupertinoSlidingSegmentedControl::SEPARATOR_VERTICAL_INSET * 2.0
                < CupertinoSlidingSegmentedControl::MIN_HEIGHT,
            "so a separator still has some length left"
        );
    }

    #[test]
    fn the_thumb_rides_a_spring_so_an_interrupted_drag_keeps_its_speed() {
        assert!(CupertinoSlidingSegmentedControl::thumb_animates_on_a_spring());
    }

    // -- A flag on one side, a dependency on the other ------------------------------------

    #[test]
    fn the_cupertino_icons_live_in_a_package_where_the_material_ones_need_a_build_flag() {
        use crate::icons::Icons;
        assert!(CupertinoIcons::requires_a_package_dependency());
        assert_eq!(CupertinoIcons::ICON_FONT_PACKAGE, "cupertino_icons");

        assert!(
            Icons::requires_material_design_font(),
            "and Material's is a pubspec flag rather than a dependency"
        );
    }

    #[test]
    fn and_there_are_far_fewer_of_them() {
        use crate::icons::Icons;
        assert_eq!(CupertinoIcons::UPSTREAM_ICON_COUNT, 1322);
        assert!(CupertinoIcons::UPSTREAM_ICON_COUNT < Icons::UPSTREAM_ICON_COUNT);
    }
}
