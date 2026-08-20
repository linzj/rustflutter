//! Ports of `widgets/scroll_view.dart` and
//! `widgets/single_child_scroll_view.dart`: `ScrollView`, `CustomScrollView`,
//! `BoxScrollView` and `SingleChildScrollView`.
//!
//! The two files are the two answers to the same question, and the difference
//! is the reason both exist: **a `SingleChildScrollView` lays out all of its
//! content every time; a `CustomScrollView` lays out only what is on screen.**
//! Everything upstream says about not putting a long list in the first one
//! follows from that one line.

use crate::render::{Axis, AxisDirection, EdgeInsets};
use crate::scroll_plumbing::{ScrollAxis, ScrollPlatform};

/// Which viewport a scroll view builds. Upstream picks between two classes in
/// `buildViewport`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportKind {
    /// The ordinary one: it takes the space it is given and shows a window on
    /// the content.
    Fixed,
    /// Upstream's `ShrinkWrappingViewport`: it takes only as much space as its
    /// slivers need. Costly, because it has to lay them all out to find out.
    ShrinkWrapping,
}

/// Upstream `ScrollViewKeyboardDismissBehavior`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScrollViewKeyboardDismissBehavior {
    #[default]
    Manual,
    /// Dragging the list puts the keyboard away. Only a **drag** does it: a
    /// fling that the list is still carrying out is not the reader touching
    /// the screen.
    OnDrag,
}

/// Which physics a scroll view ends up with when none was given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefaultedPhysics {
    /// Upstream's `AlwaysScrollableScrollPhysics`: it moves under a drag even
    /// when the content fits.
    AlwaysScrollable,
    /// Nothing of its own -- whatever the ambient `ScrollBehavior` says, which
    /// for content that fits means not moving at all.
    Inherited,
}

/// Upstream `ScrollView`: the configuration every scroll view shares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollView {
    pub scroll_direction: Axis,
    pub reverse: bool,
    /// Whether a controller was passed explicitly.
    pub has_controller: bool,
    /// `None` means "work it out": upstream defaults it from whether a
    /// controller was given and whether the axis is the primary one.
    pub primary: Option<bool>,
    /// Whether physics were given explicitly.
    pub has_physics: bool,
    pub shrink_wrap: bool,
    /// Whether a `center` sliver key was given.
    pub has_center: bool,
    /// Where in the viewport the zero scroll offset sits, as a fraction.
    pub anchor: f32,
    pub semantic_child_count: Option<i32>,
    pub keyboard_dismiss_behavior: Option<ScrollViewKeyboardDismissBehavior>,
}

impl ScrollView {
    pub fn new() -> ScrollView {
        ScrollView {
            scroll_direction: Axis::Vertical,
            reverse: false,
            has_controller: false,
            primary: None,
            has_physics: false,
            shrink_wrap: false,
            has_center: false,
            anchor: 0.0,
            semantic_child_count: None,
            keyboard_dismiss_behavior: None,
        }
    }

    pub fn horizontal(mut self) -> Self {
        self.scroll_direction = Axis::Horizontal;
        self
    }

    pub fn with_controller(mut self) -> Self {
        self.has_controller = true;
        self
    }

    pub fn with_primary(mut self, primary: bool) -> Self {
        self.primary = Some(primary);
        self
    }

    pub fn shrink_wrapped(mut self) -> Self {
        self.shrink_wrap = true;
        self
    }

    /// Upstream's constructor asserts.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.has_controller && self.primary == Some(true) {
            // Upstream's message says what the contradiction is: a primary
            // scroll view gets its controller by inheritance, so passing one
            // and asking for the inherited one at once has no answer.
            return Err("You cannot both set primary to true and pass an explicit controller.");
        }
        if self.shrink_wrap && self.has_center {
            // A shrink-wrapping viewport has no fixed size, and centring is a
            // statement about a fixed size.
            return Err("A shrink-wrapping viewport cannot have a center sliver");
        }
        if !(0.0..=1.0).contains(&self.anchor) {
            return Err("anchor must be between zero and one");
        }
        if self.semantic_child_count.is_some_and(|count| count < 0) {
            return Err("semanticChildCount must not be negative");
        }
        Ok(())
    }

    /// Upstream's physics defaulting, which is the most consequential default
    /// in the file.
    ///
    /// A vertical scroll view with no controller of its own gets
    /// `AlwaysScrollableScrollPhysics`: **it bounces even when the content
    /// fits.** That reads as waste until you notice what such a view usually
    /// is -- the page. A page that refuses to move when the reader pulls it
    /// looks broken, whatever it contains. A horizontal carousel, or one with
    /// its own controller, is a component rather than the page, and a short one
    /// simply does not move.
    pub fn defaulted_physics(&self) -> Option<DefaultedPhysics> {
        if self.has_physics {
            return None;
        }
        let always = self.primary == Some(true)
            || (self.primary.is_none()
                && !self.has_controller
                && self.scroll_direction == Axis::Vertical);
        Some(if always {
            DefaultedPhysics::AlwaysScrollable
        } else {
            DefaultedPhysics::Inherited
        })
    }

    /// Upstream's `effectivePrimary`, which asks the ambient
    /// `PrimaryScrollController` rather than deciding alone.
    pub fn effective_primary(&self, platform: ScrollPlatform, axis: ScrollAxis) -> bool {
        self.primary.unwrap_or_else(|| {
            !self.has_controller
                && crate::scroll_plumbing::PrimaryScrollController::new()
                    .should_inherit(platform, axis)
        })
    }

    /// Whether the scroll view blocks the primary controller from anything
    /// below it. Upstream wraps in `PrimaryScrollController.none` when it took
    /// the controller, with the reason on it: a descendant scroll view would
    /// otherwise inherit the same one and two lists would drive it.
    pub fn blocks_primary_controller_below(&self, took_the_primary: bool) -> bool {
        took_the_primary
    }

    pub fn viewport_kind(&self) -> ViewportKind {
        if self.shrink_wrap {
            ViewportKind::ShrinkWrapping
        } else {
            ViewportKind::Fixed
        }
    }

    /// Upstream `getDirection`: the axis plus `reverse` plus, for a vertical
    /// view, the ambient reading direction.
    pub fn axis_direction(&self, reading_left_to_right: bool) -> AxisDirection {
        match (self.scroll_direction, self.reverse) {
            (Axis::Vertical, false) => AxisDirection::Down,
            (Axis::Vertical, true) => AxisDirection::Up,
            (Axis::Horizontal, false) => {
                if reading_left_to_right {
                    AxisDirection::Right
                } else {
                    AxisDirection::Left
                }
            }
            (Axis::Horizontal, true) => {
                if reading_left_to_right {
                    AxisDirection::Left
                } else {
                    AxisDirection::Right
                }
            }
        }
    }

    /// Upstream's three-level fallback: the widget's own setting, then the
    /// scroll behaviour's, then the ambient `ScrollConfiguration`'s.
    pub fn effective_keyboard_dismiss_behavior(
        &self,
        from_behavior: Option<ScrollViewKeyboardDismissBehavior>,
        from_configuration: ScrollViewKeyboardDismissBehavior,
    ) -> ScrollViewKeyboardDismissBehavior {
        self.keyboard_dismiss_behavior
            .or(from_behavior)
            .unwrap_or(from_configuration)
    }

    /// Whether a scroll notification should put the keyboard away. Upstream
    /// checks `dragDetails != null`, so only a finger on the glass counts.
    pub fn dismisses_keyboard(
        &self,
        behavior: ScrollViewKeyboardDismissBehavior,
        is_drag: bool,
    ) -> bool {
        behavior == ScrollViewKeyboardDismissBehavior::OnDrag && is_drag
    }
}

impl Default for ScrollView {
    fn default() -> Self {
        ScrollView::new()
    }
}

/// Upstream `CustomScrollView`.
///
/// The one that does nothing at all to its children: the slivers it is given
/// are the slivers it builds. Everything else in the family exists to spare a
/// caller from writing them out.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomScrollView {
    pub base: ScrollView,
    pub slivers: Vec<u64>,
}

impl CustomScrollView {
    pub fn new(slivers: Vec<u64>) -> CustomScrollView {
        CustomScrollView {
            base: ScrollView::new(),
            slivers,
        }
    }

    /// Upstream `buildSlivers`, in full.
    pub fn build_slivers(&self) -> Vec<u64> {
        self.slivers.clone()
    }
}

/// How a [`BoxScrollView`] split the ambient padding.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaddingSplit {
    /// Consumed by the scroll view itself, as a `SliverPadding` -- applied once,
    /// at the ends of the scroll.
    pub consumed: EdgeInsets,
    /// Left in the `MediaQuery` for the children, which each need it.
    pub left_for_children: EdgeInsets,
}

/// Upstream `BoxScrollView`: the base of `ListView` and `GridView`, which take
/// boxes rather than slivers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxScrollView {
    pub base: ScrollView,
    /// `None` means "use the ambient padding", which is where the interesting
    /// part is.
    pub padding: Option<EdgeInsets>,
}

impl BoxScrollView {
    pub fn new() -> BoxScrollView {
        BoxScrollView {
            base: ScrollView::new(),
            padding: None,
        }
    }

    pub fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Upstream `buildSlivers`, which does something worth reading twice when
    /// no padding was given.
    ///
    /// It **splits the ambient `MediaQuery` padding along the two axes**. The
    /// main-axis half is consumed by the scroll view as a `SliverPadding`; the
    /// cross-axis half is left in the `MediaQuery` for the children.
    ///
    /// On a phone with a notch and a home indicator, that is exactly right. A
    /// vertical list wants the top and bottom insets **once**, at the ends of
    /// the scroll, so the first row starts below the notch and the last ends
    /// above the indicator -- applying them to every row would leave a gap in
    /// the middle of the list. But the left and right insets have to reach
    /// **every** row, because every row spans the width.
    ///
    /// An explicit padding turns all of this off, because a caller who wrote
    /// one has thought about it.
    pub fn split_padding(&self, ambient: EdgeInsets) -> PaddingSplit {
        if let Some(padding) = self.padding {
            return PaddingSplit {
                consumed: padding,
                left_for_children: ambient,
            };
        }
        let horizontal = EdgeInsets::only(ambient.left, 0.0, ambient.right, 0.0);
        let vertical = EdgeInsets::only(0.0, ambient.top, 0.0, ambient.bottom);
        match self.base.scroll_direction {
            Axis::Vertical => PaddingSplit {
                consumed: vertical,
                left_for_children: horizontal,
            },
            Axis::Horizontal => PaddingSplit {
                consumed: horizontal,
                left_for_children: vertical,
            },
        }
    }
}

impl Default for BoxScrollView {
    fn default() -> Self {
        BoxScrollView::new()
    }
}

/// What laying out a [`SingleChildScrollView`] produced.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SingleChildScrollGeometry {
    /// The viewport's own size.
    pub size: (f32, f32),
    pub min_scroll_extent: f32,
    pub max_scroll_extent: f32,
    /// A correction upstream applies when the offset is now out of range --
    /// a child that shrank while the reader was at the bottom pulls the offset
    /// back rather than leaving a blank.
    pub offset_correction: Option<f32>,
}

/// Upstream `SingleChildScrollView`.
///
/// One box in a window. What makes it different from every other scroll view
/// is not the child count but the **layout**: the child is given no constraint
/// at all along the scroll axis, so it lays out at its natural size, all of it,
/// every frame. That is what makes it right for a form that is usually short
/// and wrong for a list that might be long.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SingleChildScrollView {
    pub scroll_direction: Axis,
    pub reverse: bool,
    pub has_controller: bool,
    pub primary: Option<bool>,
    pub padding: Option<EdgeInsets>,
}

impl SingleChildScrollView {
    pub fn new() -> SingleChildScrollView {
        SingleChildScrollView {
            scroll_direction: Axis::Vertical,
            reverse: false,
            has_controller: false,
            primary: None,
            padding: None,
        }
    }

    pub fn horizontal(mut self) -> Self {
        self.scroll_direction = Axis::Horizontal;
        self
    }

    /// The same assert as [`ScrollView`], for the same reason.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.has_controller && self.primary == Some(true) {
            return Err("You cannot both set primary to true and pass an explicit controller.");
        }
        Ok(())
    }

    /// Upstream `_getInnerConstraints`: the constraint on the scroll axis is
    /// **dropped entirely**, and only the cross axis is passed down.
    ///
    /// Returns `(min, max)` along the scroll axis and the cross axis in turn.
    pub fn inner_constraints(&self, cross_min: f32, cross_max: f32) -> ((f32, f32), (f32, f32)) {
        ((0.0, f32::INFINITY), (cross_min, cross_max))
    }

    /// Upstream `performLayout` on `_RenderSingleChildViewport`.
    pub fn layout(
        &self,
        constraint_max: (f32, f32),
        child_size: Option<(f32, f32)>,
        current_pixels: Option<f32>,
    ) -> SingleChildScrollGeometry {
        let Some(child_size) = child_size else {
            return SingleChildScrollGeometry {
                size: (0.0, 0.0),
                min_scroll_extent: 0.0,
                max_scroll_extent: 0.0,
                offset_correction: None,
            };
        };
        // The viewport takes the child's size, constrained -- so a short child
        // makes a short scroll view. It shrink-wraps without being asked to.
        let size = (
            child_size.0.min(constraint_max.0),
            child_size.1.min(constraint_max.1),
        );
        let (child_extent, viewport_extent) = match self.scroll_direction {
            Axis::Vertical => (child_size.1, size.1),
            Axis::Horizontal => (child_size.0, size.0),
        };
        // A child that fits gives nothing to scroll.
        let max_scroll_extent = (child_extent - viewport_extent).max(0.0);
        let offset_correction = current_pixels.and_then(|pixels| {
            if pixels > max_scroll_extent {
                Some(max_scroll_extent - pixels)
            } else if pixels < 0.0 {
                Some(-pixels)
            } else {
                None
            }
        });
        SingleChildScrollGeometry {
            size,
            min_scroll_extent: 0.0,
            max_scroll_extent,
            offset_correction,
        }
    }
}

impl Default for SingleChildScrollView {
    fn default() -> Self {
        SingleChildScrollView::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- The refusals ----------------------------------------------------------

    #[test]
    fn asking_for_the_inherited_controller_and_passing_one_has_no_answer() {
        let mut view = ScrollView::new().with_controller();
        assert_eq!(view.validate(), Ok(()), "a controller alone is fine");

        view.primary = Some(true);
        assert!(view.validate().is_err());

        view.primary = Some(false);
        assert_eq!(view.validate(), Ok(()), "and saying so explicitly is fine");
    }

    #[test]
    fn a_shrink_wrapping_viewport_has_no_size_to_centre_within() {
        let mut view = ScrollView::new().shrink_wrapped();
        assert_eq!(view.validate(), Ok(()));
        view.has_center = true;
        assert!(view.validate().is_err());

        let centred = ScrollView {
            has_center: true,
            ..ScrollView::new()
        };
        assert_eq!(
            centred.validate(),
            Ok(()),
            "without the shrink wrap it is fine"
        );
    }

    #[test]
    fn the_anchor_is_a_fraction_of_the_viewport() {
        for anchor in [0.0, 0.5, 1.0] {
            assert_eq!(
                ScrollView {
                    anchor,
                    ..ScrollView::new()
                }
                .validate(),
                Ok(())
            );
        }
        for anchor in [-0.1, 1.1] {
            assert!(
                ScrollView {
                    anchor,
                    ..ScrollView::new()
                }
                .validate()
                .is_err()
            );
        }
    }

    #[test]
    fn a_negative_number_of_semantic_children_is_refused() {
        assert!(
            ScrollView {
                semantic_child_count: Some(-1),
                ..ScrollView::new()
            }
            .validate()
            .is_err()
        );
        assert_eq!(
            ScrollView {
                semantic_child_count: Some(0),
                ..ScrollView::new()
            }
            .validate(),
            Ok(())
        );
    }

    // -- The physics default ------------------------------------------------------

    #[test]
    fn a_page_bounces_even_when_it_has_nothing_to_scroll() {
        // Which reads as waste until you notice what such a view usually is: a
        // page that refuses to move when pulled looks broken whatever it holds.
        assert_eq!(
            ScrollView::new().defaulted_physics(),
            Some(DefaultedPhysics::AlwaysScrollable)
        );
        assert_eq!(
            ScrollView::new().with_primary(true).defaulted_physics(),
            Some(DefaultedPhysics::AlwaysScrollable),
            "and asking for it outright says the same"
        );
    }

    #[test]
    fn a_carousel_or_a_view_with_its_own_controller_is_a_component_not_the_page() {
        // A short one simply does not move.
        assert_eq!(
            ScrollView::new().horizontal().defaulted_physics(),
            Some(DefaultedPhysics::Inherited)
        );
        assert_eq!(
            ScrollView::new().with_controller().defaulted_physics(),
            Some(DefaultedPhysics::Inherited)
        );
        assert_eq!(
            ScrollView::new().with_primary(false).defaulted_physics(),
            Some(DefaultedPhysics::Inherited)
        );
    }

    #[test]
    fn a_horizontal_view_that_asked_to_be_primary_still_gets_it() {
        // The first arm of the condition does not look at the axis at all.
        assert_eq!(
            ScrollView::new()
                .horizontal()
                .with_primary(true)
                .defaulted_physics(),
            Some(DefaultedPhysics::AlwaysScrollable)
        );
    }

    #[test]
    fn given_physics_are_left_alone() {
        let explicit = ScrollView {
            has_physics: true,
            ..ScrollView::new()
        };
        assert_eq!(explicit.defaulted_physics(), None);
    }

    // -- The primary controller ------------------------------------------------------

    #[test]
    fn a_vertical_phone_list_takes_the_primary_controller_and_a_desktop_one_does_not() {
        let view = ScrollView::new();
        assert!(view.effective_primary(ScrollPlatform::IOS, ScrollAxis::Vertical));
        assert!(!view.effective_primary(ScrollPlatform::Windows, ScrollAxis::Vertical));
        assert!(!view.effective_primary(ScrollPlatform::IOS, ScrollAxis::Horizontal));
    }

    #[test]
    fn a_view_with_its_own_controller_never_takes_the_primary_one() {
        let view = ScrollView::new().with_controller();
        assert!(!view.effective_primary(ScrollPlatform::IOS, ScrollAxis::Vertical));
    }

    #[test]
    fn saying_primary_outright_overrules_the_platform() {
        assert!(
            ScrollView::new()
                .with_primary(true)
                .effective_primary(ScrollPlatform::Windows, ScrollAxis::Horizontal)
        );
        assert!(
            !ScrollView::new()
                .with_primary(false)
                .effective_primary(ScrollPlatform::IOS, ScrollAxis::Vertical)
        );
    }

    #[test]
    fn taking_the_primary_controller_also_blocks_it_from_below() {
        // Or a nested list would inherit the same one and two lists would drive
        // it.
        let view = ScrollView::new();
        assert!(view.blocks_primary_controller_below(true));
        assert!(!view.blocks_primary_controller_below(false));
    }

    // -- Direction and viewport ---------------------------------------------------------

    #[test]
    fn only_a_horizontal_scroll_view_asks_which_way_the_reader_reads() {
        let vertical = ScrollView::new();
        assert_eq!(vertical.axis_direction(true), AxisDirection::Down);
        assert_eq!(vertical.axis_direction(false), AxisDirection::Down);

        let horizontal = ScrollView::new().horizontal();
        assert_eq!(horizontal.axis_direction(true), AxisDirection::Right);
        assert_eq!(horizontal.axis_direction(false), AxisDirection::Left);
    }

    #[test]
    fn reversing_flips_whichever_direction_was_settled_on() {
        let up = ScrollView {
            reverse: true,
            ..ScrollView::new()
        };
        assert_eq!(up.axis_direction(true), AxisDirection::Up);

        let reversed_rtl = ScrollView {
            reverse: true,
            ..ScrollView::new().horizontal()
        };
        assert_eq!(reversed_rtl.axis_direction(false), AxisDirection::Right);
    }

    #[test]
    fn shrink_wrapping_picks_the_other_viewport() {
        assert_eq!(ScrollView::new().viewport_kind(), ViewportKind::Fixed);
        assert_eq!(
            ScrollView::new().shrink_wrapped().viewport_kind(),
            ViewportKind::ShrinkWrapping
        );
    }

    // -- The keyboard --------------------------------------------------------------------

    #[test]
    fn the_keyboard_setting_falls_back_through_three_levels() {
        let view = ScrollView::new();
        assert_eq!(
            view.effective_keyboard_dismiss_behavior(
                None,
                ScrollViewKeyboardDismissBehavior::OnDrag
            ),
            ScrollViewKeyboardDismissBehavior::OnDrag,
            "the configuration has the last word"
        );
        assert_eq!(
            view.effective_keyboard_dismiss_behavior(
                Some(ScrollViewKeyboardDismissBehavior::Manual),
                ScrollViewKeyboardDismissBehavior::OnDrag
            ),
            ScrollViewKeyboardDismissBehavior::Manual,
            "the behaviour overrules it"
        );

        let explicit = ScrollView {
            keyboard_dismiss_behavior: Some(ScrollViewKeyboardDismissBehavior::OnDrag),
            ..ScrollView::new()
        };
        assert_eq!(
            explicit.effective_keyboard_dismiss_behavior(
                Some(ScrollViewKeyboardDismissBehavior::Manual),
                ScrollViewKeyboardDismissBehavior::Manual
            ),
            ScrollViewKeyboardDismissBehavior::OnDrag,
            "and the widget overrules both"
        );
    }

    #[test]
    fn a_fling_the_list_is_still_carrying_out_does_not_put_the_keyboard_away() {
        // Only a finger on the glass counts -- upstream checks dragDetails.
        let view = ScrollView::new();
        let on_drag = ScrollViewKeyboardDismissBehavior::OnDrag;
        assert!(view.dismisses_keyboard(on_drag, true));
        assert!(!view.dismisses_keyboard(on_drag, false));
        assert!(!view.dismisses_keyboard(ScrollViewKeyboardDismissBehavior::Manual, true));
    }

    // -- CustomScrollView ---------------------------------------------------------------

    #[test]
    fn a_custom_scroll_view_does_nothing_at_all_to_its_slivers() {
        // Everything else in the family exists to spare a caller from writing
        // them out.
        let view = CustomScrollView::new(vec![1, 2, 3]);
        assert_eq!(view.build_slivers(), [1, 2, 3]);
        assert_eq!(
            CustomScrollView::new(vec![]).build_slivers(),
            Vec::<u64>::new()
        );
    }

    // -- The padding split ----------------------------------------------------------------

    #[test]
    fn a_list_applies_the_notch_once_and_the_side_insets_to_every_row() {
        // The top and bottom belong at the ends of the scroll -- applying them
        // per row would leave a gap in the middle of the list. The left and
        // right have to reach every row, because every row spans the width.
        let list = BoxScrollView::new();
        let ambient = EdgeInsets::only(16.0, 44.0, 16.0, 34.0);
        let split = list.split_padding(ambient);

        assert_eq!(split.consumed, EdgeInsets::only(0.0, 44.0, 0.0, 34.0));
        assert_eq!(
            split.left_for_children,
            EdgeInsets::only(16.0, 0.0, 16.0, 0.0)
        );
    }

    #[test]
    fn a_horizontal_list_splits_the_same_padding_the_other_way() {
        let row = BoxScrollView {
            base: ScrollView::new().horizontal(),
            padding: None,
        };
        let split = row.split_padding(EdgeInsets::only(16.0, 44.0, 16.0, 34.0));
        assert_eq!(split.consumed, EdgeInsets::only(16.0, 0.0, 16.0, 0.0));
        assert_eq!(
            split.left_for_children,
            EdgeInsets::only(0.0, 44.0, 0.0, 34.0)
        );
    }

    #[test]
    fn an_explicit_padding_turns_the_whole_arrangement_off() {
        // A caller who wrote one has thought about it.
        let list = BoxScrollView::new().with_padding(EdgeInsets::all(8.0));
        let ambient = EdgeInsets::only(16.0, 44.0, 16.0, 34.0);
        let split = list.split_padding(ambient);
        assert_eq!(split.consumed, EdgeInsets::all(8.0));
        assert_eq!(
            split.left_for_children, ambient,
            "and the ambient padding is passed through untouched"
        );
    }

    #[test]
    fn nothing_to_split_splits_to_nothing() {
        let split = BoxScrollView::new().split_padding(EdgeInsets::ZERO);
        assert_eq!(split.consumed, EdgeInsets::ZERO);
        assert_eq!(split.left_for_children, EdgeInsets::ZERO);
    }

    // -- SingleChildScrollView -------------------------------------------------------------

    #[test]
    fn the_child_is_given_no_constraint_at_all_along_the_scroll_axis() {
        // Which is the whole difference: it lays out at its natural size, all
        // of it, every frame.
        let view = SingleChildScrollView::new();
        let (main, cross) = view.inner_constraints(0.0, 400.0);
        assert_eq!(main, (0.0, f32::INFINITY));
        assert_eq!(cross, (0.0, 400.0));
    }

    #[test]
    fn a_short_child_makes_a_short_scroll_view_without_being_asked() {
        let view = SingleChildScrollView::new();
        let geometry = view.layout((400.0, 800.0), Some((400.0, 200.0)), Some(0.0));
        assert_eq!(geometry.size, (400.0, 200.0));
        assert_eq!(
            geometry.max_scroll_extent, 0.0,
            "and a child that fits gives nothing to scroll"
        );
    }

    #[test]
    fn a_tall_child_is_clamped_to_the_viewport_and_the_rest_is_the_scroll() {
        let view = SingleChildScrollView::new();
        let geometry = view.layout((400.0, 800.0), Some((400.0, 2000.0)), Some(0.0));
        assert_eq!(geometry.size, (400.0, 800.0));
        assert_eq!(geometry.max_scroll_extent, 1200.0);
        assert_eq!(geometry.min_scroll_extent, 0.0);
    }

    #[test]
    fn a_child_that_shrank_under_the_reader_pulls_the_offset_back() {
        // Rather than leaving them looking at a blank.
        let view = SingleChildScrollView::new();
        let scrolled_to_the_bottom =
            view.layout((400.0, 800.0), Some((400.0, 2000.0)), Some(1200.0));
        assert_eq!(scrolled_to_the_bottom.offset_correction, None);

        let shrank = view.layout((400.0, 800.0), Some((400.0, 1000.0)), Some(1200.0));
        assert_eq!(shrank.max_scroll_extent, 200.0);
        assert_eq!(shrank.offset_correction, Some(-1000.0));
    }

    #[test]
    fn an_offset_dragged_above_the_start_is_pulled_back_too() {
        let view = SingleChildScrollView::new();
        let geometry = view.layout((400.0, 800.0), Some((400.0, 2000.0)), Some(-50.0));
        assert_eq!(geometry.offset_correction, Some(50.0));
    }

    #[test]
    fn a_horizontal_single_child_view_measures_the_other_axis() {
        let view = SingleChildScrollView::new().horizontal();
        let geometry = view.layout((400.0, 800.0), Some((2000.0, 800.0)), Some(0.0));
        assert_eq!(geometry.max_scroll_extent, 1600.0);
    }

    #[test]
    fn a_view_with_no_child_has_nothing_to_report() {
        let view = SingleChildScrollView::new();
        let geometry = view.layout((400.0, 800.0), None, Some(0.0));
        assert_eq!(geometry.size, (0.0, 0.0));
        assert_eq!(geometry.max_scroll_extent, 0.0);
        assert_eq!(geometry.offset_correction, None);
    }

    #[test]
    fn the_single_child_view_refuses_the_same_pair_for_the_same_reason() {
        let mut view = SingleChildScrollView::new();
        view.has_controller = true;
        view.primary = Some(true);
        assert!(view.validate().is_err());
    }
}
