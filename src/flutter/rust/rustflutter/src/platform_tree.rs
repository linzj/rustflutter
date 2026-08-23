//! Ports of `rendering/sliver_tree.dart`'s `TreeSliverNodeParentData` and
//! `RenderTreeSliver`, and `widgets/platform_view.dart`'s `HtmlElementView`,
//! `PlatformViewCreationParams` and `AndroidViewSurface`.
//!
//! The last of `rendering/` and `widgets/`.

use crate::render::AxisDirection;

/// Upstream `TreeSliverNodeParentData`.
///
/// One field, `depth`, and it lives on the **parent data** rather than the
/// widget because the render object needs it during layout -- parent data is
/// how a child tells the parent laying it out something about itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TreeSliverNodeParentData {
    pub depth: u32,
}

/// Upstream `TreeSliverIndentationType`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeSliverIndentationType {
    value: f32,
}

impl TreeSliverIndentationType {
    /// Upstream's `standard`.
    pub const STANDARD: TreeSliverIndentationType = TreeSliverIndentationType { value: 10.0 };

    /// Upstream's `none`, which is `custom(0.0)` by another name.
    ///
    /// The two are indistinguishable to the render object -- same field, same
    /// number -- and the docs keep them apart by **intent**: `none` is
    /// documented as *"Useful if the indentation is implemented in the
    /// `TreeSliver.treeNodeBuilder` instead"*, while `custom(0.0)` just says
    /// indent by nothing. **Two names for one value, distinguished by what the
    /// caller means rather than by what happens.**
    pub const NONE: TreeSliverIndentationType = TreeSliverIndentationType { value: 0.0 };

    /// Upstream's `custom`, which asserts a non-negative value.
    pub fn custom(value: f32) -> Option<TreeSliverIndentationType> {
        (value >= 0.0).then_some(TreeSliverIndentationType { value })
    }

    pub fn value(&self) -> f32 {
        self.value
    }

    /// Whether the render object handles the indent itself.
    ///
    /// The class doc explains the whole trade in one sentence: when the render
    /// object indents, *"the space allotted to the indentation will **not** be
    /// part of the space made available to the Widget returned by
    /// `TreeSliver.treeNodeBuilder`"*.
    ///
    /// **So the choice is about who owns the indented pixels**, and that decides
    /// whether anything can be painted into them. Handing the job to the builder
    /// is how you get to fill the indent with a decoration or let an ink effect
    /// run under it -- the render object's version leaves that space belonging
    /// to nobody.
    pub fn builder_owns_the_indent(&self) -> bool {
        self.value == 0.0
    }
}

/// Why a tree sliver refused to lay out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeSliverError {
    NotLaidOutDownwards,
}

/// Upstream `RenderTreeSliver`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderTreeSliver {
    pub indentation: TreeSliverIndentationType,
}

impl RenderTreeSliver {
    pub fn new() -> RenderTreeSliver {
        RenderTreeSliver {
            indentation: TreeSliverIndentationType::STANDARD,
        }
    }

    /// Upstream's `performLayout` opens with:
    ///
    /// ```dart
    /// assert(
    ///   constraints.axisDirection == AxisDirection.down,
    ///   'TreeSliver is only supported in Viewports with an AxisDirection.down. '
    ///   'The current axis direction is: ${constraints.axisDirection}.',
    /// );
    /// ```
    ///
    /// **Down, not merely vertical.** A reversed vertical viewport is refused
    /// along with the two horizontal ones, which is stricter than it first
    /// looks and right: a tree's order is inherently top to bottom, so a list
    /// running upwards would put every child above its parent. The indentation
    /// would still work; the meaning would not.
    ///
    /// The message names the direction it actually got, which is the difference
    /// between an assert and a diagnosis.
    pub fn perform_layout(&self, axis_direction: AxisDirection) -> Result<(), TreeSliverError> {
        if axis_direction != AxisDirection::Down {
            return Err(TreeSliverError::NotLaidOutDownwards);
        }
        Ok(())
    }

    /// How far into the cross axis a node at this depth is pushed.
    pub fn cross_axis_offset(&self, depth: u32) -> f32 {
        self.indentation.value() * depth as f32
    }
}

impl Default for RenderTreeSliver {
    fn default() -> Self {
        RenderTreeSliver::new()
    }
}

/// Upstream `PlatformViewHitTestBehavior`: how a platform view answers a
/// touch.
///
/// # Not the same three as `HitTestBehavior`
///
/// [`crate::render::HitTestBehavior`] offers `deferToChild`, `opaque` and
/// `translucent`. This one offers `opaque`, `translucent` and **`transparent`**
/// -- and the swap is not a rename. A platform view has no Flutter children to
/// defer to; it is somebody else's surface. So the third option cannot be "ask
/// my children", and is instead "I am not here".
///
/// # Two questions, not three cases
///
/// Upstream's whole implementation is two lines:
///
/// ```dart
/// bool hitTest(result, {position}) {
///   if (behavior == transparent || !size.contains(position!)) return false;
///   result.add(BoxHitTestEntry(this, position));
///   return behavior == opaque;
/// }
/// bool hitTestSelf(position) => behavior != transparent;
/// ```
///
/// Which asks two separate things: **does this view take the event**, and
/// **does it stop the search**. `translucent` is the one that separates them
/// -- it records itself *and* returns false, so it receives the touch and what
/// is behind it receives the touch too.
///
/// The fourth combination -- refusing the event but stopping the search --
/// would be a view that blocks what it will not use, and there is no value for
/// it.
///
/// # Ported without the surface it would drive
///
/// The render objects that read this are `blocked_engine` in this tree: there
/// is no platform-view channel, so `AndroidView` and its family have nowhere
/// to send anything. The decision here is not blocked by that, because it is
/// arithmetic on the enum rather than on a foreign surface -- the same reason
/// `SmartDashesType::resolve` is testable without a keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlatformViewHitTestBehavior {
    /// Takes the event and stops the search.
    #[default]
    Opaque,
    /// Takes the event and lets the search go on behind it.
    Translucent,
    /// Takes nothing and stops nothing.
    Transparent,
}

impl PlatformViewHitTestBehavior {
    pub const ALL: [PlatformViewHitTestBehavior; 3] = [
        PlatformViewHitTestBehavior::Opaque,
        PlatformViewHitTestBehavior::Translucent,
        PlatformViewHitTestBehavior::Transparent,
    ];

    /// Upstream's `hitTestSelf`: `behavior != transparent`.
    pub fn takes_the_event(self) -> bool {
        !matches!(self, PlatformViewHitTestBehavior::Transparent)
    }

    /// Upstream's `hitTest` return: `behavior == opaque`.
    pub fn stops_the_search(self) -> bool {
        matches!(self, PlatformViewHitTestBehavior::Opaque)
    }

    /// Upstream's `hitTest` in full, including the bounds check that comes
    /// before the behaviour is consulted at all.
    ///
    /// Returns `(recorded, stop)`: whether this view is added to the path, and
    /// whether the hit test stops here. A touch outside the bounds is neither,
    /// **whatever the behaviour is** -- `opaque` does not mean "everywhere".
    pub fn hit_test(self, within_bounds: bool) -> (bool, bool) {
        if !within_bounds || !self.takes_the_event() {
            return (false, false);
        }
        (true, self.stops_the_search())
    }
}

/// Upstream `PlatformViewCreationParams`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlatformViewCreationParams {
    /// *"The unique identifier for the new platform view."*
    pub id: i64,
    /// *"This viewType is used to tell the platform which type of view to
    /// associate with the `id`."*
    pub view_type: String,
    pub has_on_platform_view_created: bool,
    pub has_on_focus_changed: bool,
}

impl PlatformViewCreationParams {
    pub fn new(id: i64, view_type: impl Into<String>) -> PlatformViewCreationParams {
        PlatformViewCreationParams {
            id,
            view_type: view_type.into(),
            has_on_platform_view_created: true,
            has_on_focus_changed: true,
        }
    }

    /// The two identifiers do different jobs and it is easy to conflate them:
    /// **`viewType` says what kind of thing to make, `id` says which one this
    /// is.** A screen with three maps has one view type and three ids.
    pub fn identifies_a_kind_and_an_instance(&self) -> bool {
        true
    }
}

/// Upstream `HtmlElementView`.
#[derive(Clone, Debug, PartialEq)]
pub struct HtmlElementView {
    pub view_type: String,
    /// Not about being seen. See [`HtmlElementView::wastes_an_overlay`].
    pub is_visible: bool,
    pub has_creation_params: bool,
    pub has_creation_params_codec: bool,
}

impl HtmlElementView {
    pub fn new(view_type: impl Into<String>) -> HtmlElementView {
        HtmlElementView {
            view_type: view_type.into(),
            is_visible: true,
            has_creation_params: false,
            has_creation_params_codec: false,
        }
    }

    /// Upstream's `isVisible`, and its doc is not about being seen:
    ///
    /// > Correctly defining this value helps the Flutter web rendering engine
    /// > optimize the amount of _overlays_ it'll need [...] in some
    /// > `HtmlElementView`s (like the `pointer_interceptor` or `Link` widget),
    /// > it can be set to `false`, **so the engine doesn't waste an overlay to
    /// > render Flutter content on top of views that don't paint any pixels.**
    ///
    /// So `isVisible: false` does not hide anything. It says **this element
    /// paints nothing** -- it is a hit target or an anchor, not a picture -- and
    /// the engine uses that to skip allocating a compositing overlay above it.
    ///
    /// A flag named for appearance whose actual subject is the compositing
    /// budget. Fourth of a family this sweep has been collecting, after
    /// `indexIsChanging` named for its cause, `ListTileControlAffinity.platform`
    /// named for the wrong axis, and `tapEnabled` named for its lever.
    pub fn wastes_an_overlay(&self) -> bool {
        self.is_visible
    }

    /// The assert all three platform view constructors share, written out three
    /// times: `assert(creationParams == null || creationParamsCodec != null)`.
    ///
    /// An implication -- **arguments require a codec** -- because there is no
    /// way to put values on a platform channel without saying how they are to
    /// be encoded. A codec with no params is fine; params with no codec is
    /// unsendable.
    pub fn is_valid(&self) -> bool {
        !self.has_creation_params || self.has_creation_params_codec
    }
}

/// Upstream `AndroidViewSurface`.
///
/// *"Integrates an Android view with Flutter's compositor, touch, and semantics
/// subsystems. The compositor integration is done by adding a `TextureLayer` to
/// the layer tree."*
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AndroidViewSurface {
    pub platform_view_created: bool,
}

impl AndroidViewSurface {
    pub fn new() -> AndroidViewSurface {
        AndroidViewSurface::default()
    }

    /// *"The parent of this object must provide bounded layout constraints."*
    /// A texture has to be given a size; it has no opinion of its own.
    pub fn requires_bounded_constraints() -> bool {
        true
    }

    /// *"If the associated platform view is not created, the
    /// `AndroidViewSurface` does not paint any contents."*
    ///
    /// Not an error and not a placeholder -- **nothing.** The surface exists
    /// before the thing it shows does, and in the gap it simply draws none of
    /// it.
    pub fn paints(&self) -> bool {
        self.platform_view_created
    }

    /// Upstream's own doc steers you away:
    ///
    /// > When possible, you may want to use `AndroidView` directly, since it
    /// > requires **less boilerplate code** than `AndroidViewSurface`, and
    /// > there's **no difference in performance, or other trade-off(s)**.
    ///
    /// A class kept for the cases that need the control, with the docs saying
    /// plainly that choosing it buys nothing but work. The same shape as
    /// `MaterialButton` pointing at `TextButton` in tick 83 -- except that one
    /// was superseded, and this one never had an advantage to lose.
    pub fn is_the_verbose_option() -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Down, not merely vertical ----------------------------------------------------

    #[test]
    fn a_tree_refuses_every_direction_but_down_including_the_reversed_vertical() {
        let tree = RenderTreeSliver::new();
        assert_eq!(tree.perform_layout(AxisDirection::Down), Ok(()));
        for refused in [AxisDirection::Up, AxisDirection::Left, AxisDirection::Right] {
            assert_eq!(
                tree.perform_layout(refused),
                Err(TreeSliverError::NotLaidOutDownwards),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn because_a_tree_running_upwards_would_put_children_above_their_parents() {
        // The indentation would still work; the meaning would not.
        let tree = RenderTreeSliver::new();
        assert_eq!(tree.cross_axis_offset(2), 20.0);
        assert!(tree.perform_layout(AxisDirection::Up).is_err());
    }

    // -- Who owns the indented pixels ---------------------------------------------------

    #[test]
    fn handing_the_indent_to_the_builder_is_how_you_get_to_paint_in_it() {
        assert!(TreeSliverIndentationType::NONE.builder_owns_the_indent());
        assert!(!TreeSliverIndentationType::STANDARD.builder_owns_the_indent());
    }

    #[test]
    fn none_and_a_custom_zero_are_the_same_value_kept_apart_by_intent() {
        assert_eq!(
            TreeSliverIndentationType::NONE.value(),
            TreeSliverIndentationType::custom(0.0).unwrap().value()
        );
        assert_eq!(
            TreeSliverIndentationType::NONE,
            TreeSliverIndentationType::custom(0.0).unwrap(),
            "the render object cannot tell them apart at all"
        );
    }

    #[test]
    fn an_indentation_may_not_be_negative() {
        assert!(TreeSliverIndentationType::custom(24.0).is_some());
        assert!(TreeSliverIndentationType::custom(0.0).is_some());
        assert!(TreeSliverIndentationType::custom(-1.0).is_none());
    }

    #[test]
    fn depth_rides_on_the_parent_data_because_layout_is_where_it_is_needed() {
        let mut parent_data = TreeSliverNodeParentData::default();
        assert_eq!(parent_data.depth, 0);
        parent_data.depth = 3;

        let tree = RenderTreeSliver::new();
        assert_eq!(tree.cross_axis_offset(parent_data.depth), 30.0);
    }

    // -- A flag named for appearance, doing compositing ---------------------------------

    #[test]
    fn is_visible_false_means_paints_nothing_rather_than_is_hidden() {
        // pointer_interceptor and Link are the doc's own examples: hit targets
        // and anchors, not pictures.
        let mut view = HtmlElementView::new("pointer_interceptor");
        assert!(view.wastes_an_overlay(), "the default costs one");

        view.is_visible = false;
        assert!(
            !view.wastes_an_overlay(),
            "and saying so lets the engine skip it"
        );
    }

    #[test]
    fn arguments_require_a_codec_but_a_codec_needs_no_arguments() {
        let mut view = HtmlElementView::new("map");
        assert!(view.is_valid(), "neither is fine");

        view.has_creation_params_codec = true;
        assert!(view.is_valid(), "a codec alone is fine");

        view.has_creation_params = true;
        assert!(view.is_valid());

        view.has_creation_params_codec = false;
        assert!(!view.is_valid(), "there is no way to encode them");
    }

    // -- A kind and an instance ----------------------------------------------------------

    #[test]
    fn view_type_says_what_to_make_and_id_says_which_one_this_is() {
        let first = PlatformViewCreationParams::new(1, "map");
        let second = PlatformViewCreationParams::new(2, "map");
        assert_eq!(first.view_type, second.view_type);
        assert_ne!(first.id, second.id);
        assert!(first.identifies_a_kind_and_an_instance());
    }

    // -- The surface before the thing ------------------------------------------------------

    #[test]
    fn a_surface_whose_platform_view_is_not_made_yet_paints_nothing_at_all() {
        let mut surface = AndroidViewSurface::new();
        assert!(!surface.paints(), "not an error and not a placeholder");
        surface.platform_view_created = true;
        assert!(surface.paints());
    }

    #[test]
    fn a_texture_has_to_be_given_a_size() {
        assert!(AndroidViewSurface::requires_bounded_constraints());
    }

    #[test]
    fn the_docs_say_plainly_that_choosing_this_one_buys_nothing_but_work() {
        assert!(AndroidViewSurface::is_the_verbose_option());
    }
}

#[cfg(test)]
mod platform_view_hit_test_tests {
    use super::PlatformViewHitTestBehavior;

    #[test]
    fn translucent_takes_the_event_and_still_lets_it_through() {
        // The value that separates the two questions: it records itself and
        // returns false, so it receives the touch and so does what is behind.
        let behavior = PlatformViewHitTestBehavior::Translucent;
        assert!(behavior.takes_the_event());
        assert!(!behavior.stops_the_search());
        assert_eq!(behavior.hit_test(true), (true, false));
    }

    #[test]
    fn opaque_takes_it_and_keeps_it() {
        let behavior = PlatformViewHitTestBehavior::Opaque;
        assert!(behavior.takes_the_event());
        assert!(behavior.stops_the_search());
        assert_eq!(behavior.hit_test(true), (true, true));
    }

    #[test]
    fn and_transparent_takes_nothing_and_stops_nothing() {
        let behavior = PlatformViewHitTestBehavior::Transparent;
        assert!(!behavior.takes_the_event());
        assert!(!behavior.stops_the_search());
        assert_eq!(behavior.hit_test(true), (false, false));
    }

    #[test]
    fn the_two_questions_tell_the_three_apart() {
        // Neither question alone separates all three: taking groups opaque
        // with translucent, stopping groups translucent with transparent.
        let mut answers: Vec<(bool, bool)> = PlatformViewHitTestBehavior::ALL
            .iter()
            .map(|b| (b.takes_the_event(), b.stops_the_search()))
            .collect();
        answers.sort();
        answers.dedup();
        assert_eq!(answers.len(), 3);
        // And the missing fourth combination -- refusing the event while
        // stopping the search -- is one no value produces, because a view that
        // blocks what it will not use is not a thing anyone wants.
        assert!(!answers.contains(&(false, true)));
    }

    #[test]
    fn a_touch_outside_the_bounds_is_nothing_whatever_the_behaviour() {
        // The bounds check comes before the behaviour is consulted at all, so
        // opaque does not mean everywhere.
        for behavior in PlatformViewHitTestBehavior::ALL {
            assert_eq!(behavior.hit_test(false), (false, false), "{behavior:?}");
        }
        // Inside the bounds they differ, or the test above would be empty.
        let inside: Vec<(bool, bool)> = PlatformViewHitTestBehavior::ALL
            .iter()
            .map(|b| b.hit_test(true))
            .collect();
        assert_eq!(inside.len(), 3);
        assert!(inside.iter().any(|answer| *answer != (false, false)));
    }

    #[test]
    fn a_platform_view_blocks_what_is_behind_it_unless_told_otherwise() {
        assert_eq!(
            PlatformViewHitTestBehavior::default(),
            PlatformViewHitTestBehavior::Opaque
        );
    }
}
