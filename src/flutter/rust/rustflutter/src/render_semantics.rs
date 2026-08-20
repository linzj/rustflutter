//! Ports of the semantics render objects in `rendering/proxy_box.dart` and
//! `rendering/proxy_sliver.dart`.
//!
//! The widget side of these went into `semantics_markers.rs` several ticks ago;
//! these are the render objects underneath them, which the ruler noticed were
//! still missing.

use crate::render::HitTestBehavior;
use crate::semantics::SemanticsAction;

/// Upstream `RenderSemanticsGestureHandler`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderSemanticsGestureHandler {
    pub behavior: HitTestBehavior,
    pub has_on_tap: bool,
    pub has_on_long_press: bool,
    pub has_on_horizontal_drag_update: bool,
    pub has_on_vertical_drag_update: bool,
    /// `None` allows everything the callbacks offer.
    pub valid_actions: Option<Vec<SemanticsAction>>,
}

impl RenderSemanticsGestureHandler {
    pub fn new() -> RenderSemanticsGestureHandler {
        RenderSemanticsGestureHandler::default()
    }

    /// What the callbacks alone would advertise.
    ///
    /// A horizontal drag handler offers **both** directions, because a drag
    /// handler has no idea which way there is anywhere to go.
    pub fn offered_actions(&self) -> Vec<SemanticsAction> {
        let mut actions = Vec::new();
        if self.has_on_tap {
            actions.push(SemanticsAction::Tap);
        }
        if self.has_on_long_press {
            actions.push(SemanticsAction::LongPress);
        }
        if self.has_on_horizontal_drag_update {
            actions.push(SemanticsAction::ScrollLeft);
            actions.push(SemanticsAction::ScrollRight);
        }
        if self.has_on_vertical_drag_update {
            actions.push(SemanticsAction::ScrollUp);
            actions.push(SemanticsAction::ScrollDown);
        }
        actions
    }

    /// Upstream `validActions`, whose doc is worth quoting because it describes
    /// a filter that takes things away:
    ///
    /// > If non-null, the set of actions to allow. **Other actions will be
    /// > omitted, even if their callback is provided.** [...] This is normally
    /// > used to filter the actions made available by `onHorizontalDragUpdate`
    /// > and `onVerticalDragUpdate`. Normally, these make both the right and
    /// > left, or up and down, actions available.
    ///
    /// So there are two separate facts about one action -- **whether it is
    /// wired up, and whether it is possible right now** -- and only the second
    /// changes as you scroll. The callback is the first, `validActions` is the
    /// second, and a list at its left edge uses it to say "you may scroll
    /// right, but not left" without unhooking anything.
    pub fn advertised_actions(&self) -> Vec<SemanticsAction> {
        let mut actions = self.offered_actions();
        if let Some(valid) = &self.valid_actions {
            actions.retain(|action| valid.contains(action));
        }
        actions
    }
}

/// Upstream `RenderSemanticsAnnotations`, which carries the whole of a
/// `Semantics` widget's properties down to the semantics tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderSemanticsAnnotations {
    pub container: bool,
    pub explicit_child_nodes: bool,
    pub exclude_semantics: bool,
    pub label: Option<String>,
    pub value: Option<String>,
}

impl RenderSemanticsAnnotations {
    pub fn new() -> RenderSemanticsAnnotations {
        RenderSemanticsAnnotations::default()
    }

    /// Whether this node becomes a boundary of its own, which `container` asks
    /// for directly.
    pub fn is_semantic_boundary(&self) -> bool {
        self.container
    }

    /// Upstream's `visitChildrenForSemantics` skips the children entirely when
    /// `excludeSemantics` is set, the same way [`RenderExcludeSemantics`] does.
    pub fn visits_children(&self) -> bool {
        !self.exclude_semantics
    }
}

/// Upstream `RenderBlockSemantics`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderBlockSemantics {
    blocking: bool,
    pub needs_semantics_update: bool,
}

impl RenderBlockSemantics {
    pub fn new(blocking: bool) -> RenderBlockSemantics {
        RenderBlockSemantics {
            blocking,
            needs_semantics_update: false,
        }
    }

    /// Upstream's doc, and the operative words are the last four:
    ///
    /// > Whether this render object is blocking semantics of **previously
    /// > painted** `RenderObject`s below a common semantics boundary from the
    /// > semantic tree.
    ///
    /// **Paint order, not tree order.** What it blocks is whatever was drawn
    /// before it, which is exactly what a modal wants: a dialog is painted after
    /// the page it covers, so blocking the previously painted nodes hides the
    /// page and nothing else. Tree order would have hidden the wrong things --
    /// the dialog is not an ancestor of the page.
    pub fn blocking(&self) -> bool {
        self.blocking
    }

    /// Every setter in this family has the same body: return if unchanged,
    /// otherwise store and `markNeedsSemanticsUpdate()`. Five classes, five
    /// copies.
    pub fn set_blocking(&mut self, value: bool) {
        if value == self.blocking {
            return;
        }
        self.blocking = value;
        self.needs_semantics_update = true;
    }
}

/// Upstream `RenderMergeSemantics`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderMergeSemantics;

impl RenderMergeSemantics {
    /// Upstream sets **two** flags together:
    ///
    /// ```dart
    /// config
    ///   ..isSemanticBoundary = true
    ///   ..isMergingSemanticsOfDescendants = true;
    /// ```
    ///
    /// And it has to. Merging means everything below collapses **into** this
    /// node, and a node you can collapse into is precisely what a boundary is --
    /// so asking to merge without becoming a boundary would be asking the
    /// descendants to fold into nothing.
    pub fn is_semantic_boundary() -> bool {
        true
    }

    pub fn is_merging_semantics_of_descendants() -> bool {
        true
    }
}

/// Upstream `RenderExcludeSemantics`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderExcludeSemantics {
    excluding: bool,
    pub needs_semantics_update: bool,
}

impl RenderExcludeSemantics {
    pub fn new(excluding: bool) -> RenderExcludeSemantics {
        RenderExcludeSemantics {
            excluding,
            needs_semantics_update: false,
        }
    }

    pub fn excluding(&self) -> bool {
        self.excluding
    }

    pub fn set_excluding(&mut self, value: bool) {
        if value == self.excluding {
            return;
        }
        self.excluding = value;
        self.needs_semantics_update = true;
    }

    /// Upstream's `visitChildrenForSemantics`:
    ///
    /// ```dart
    /// if (excluding) {
    ///   return;
    /// }
    /// super.visitChildrenForSemantics(visitor);
    /// ```
    ///
    /// **Exclusion is not a flag on the subtree, it is a walk that never gets
    /// there.** Nothing below is marked hidden; the traversal simply turns back
    /// at this node, so the descendants are never asked what they would have
    /// said.
    pub fn visits_children(&self) -> bool {
        !self.excluding
    }
}

/// Upstream `RenderIndexedSemantics`.
///
/// The doc says what the index is for: *"the `ScrollView` uses the index of the
/// first visible child semantics node to determine the
/// `SemanticsConfiguration.scrollIndex`."* A screen reader saying "row 12 of
/// 200" needs somebody to have written the 12 down, and the list is the only
/// one who knows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderIndexedSemantics {
    index: i64,
    pub needs_semantics_update: bool,
}

impl RenderIndexedSemantics {
    pub fn new(index: i64) -> RenderIndexedSemantics {
        RenderIndexedSemantics {
            index,
            needs_semantics_update: false,
        }
    }

    pub fn index(&self) -> i64 {
        self.index
    }

    pub fn set_index(&mut self, value: i64) {
        if value == self.index {
            return;
        }
        self.index = value;
        self.needs_semantics_update = true;
    }
}

/// Upstream `RenderAnnotatedRegion`, which puts a value into the layer tree for
/// `Layer.find` to pull back out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderAnnotatedRegion<T> {
    pub value: T,
    /// Upstream: *"If `sized` is true, the layer is provided with the size of
    /// this render object to clip the results of `Layer.find`."*
    ///
    /// So `sized` decides whether the annotation has a shape or covers
    /// everything a search reaches. An unsized region answers for any point;
    /// a sized one answers only for points inside it.
    pub sized: bool,
}

impl<T: Copy + PartialEq> RenderAnnotatedRegion<T> {
    pub fn new(value: T, sized: bool) -> RenderAnnotatedRegion<T> {
        RenderAnnotatedRegion { value, sized }
    }

    /// Whether a search at this point finds the annotation, given the region's
    /// own size.
    pub fn finds_at(&self, point: (f32, f32), size: (f32, f32)) -> bool {
        if !self.sized {
            return true;
        }
        point.0 >= 0.0 && point.0 < size.0 && point.1 >= 0.0 && point.1 < size.1
    }
}

/// Upstream `RenderSliverSemanticsAnnotations`, the sliver twin of
/// [`RenderSemanticsAnnotations`]. Same properties, same
/// `SemanticsAnnotationsMixin`, applied to a sliver instead of a box.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderSliverSemanticsAnnotations {
    pub annotations: RenderSemanticsAnnotations,
}

impl RenderSliverSemanticsAnnotations {
    pub fn new() -> RenderSliverSemanticsAnnotations {
        RenderSliverSemanticsAnnotations::default()
    }

    pub fn is_semantic_boundary(&self) -> bool {
        self.annotations.is_semantic_boundary()
    }

    pub fn visits_children(&self) -> bool {
        self.annotations.visits_children()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Wired up and possible are two different facts ------------------------------

    #[test]
    fn a_horizontal_drag_handler_offers_both_directions_because_it_knows_neither() {
        let mut handler = RenderSemanticsGestureHandler::new();
        handler.has_on_horizontal_drag_update = true;
        assert_eq!(
            handler.offered_actions(),
            vec![SemanticsAction::ScrollLeft, SemanticsAction::ScrollRight]
        );
    }

    #[test]
    fn and_a_list_at_its_left_edge_takes_one_back_without_unhooking_anything() {
        let mut handler = RenderSemanticsGestureHandler::new();
        handler.has_on_horizontal_drag_update = true;
        handler.valid_actions = Some(vec![SemanticsAction::ScrollRight]);

        assert_eq!(
            handler.advertised_actions(),
            vec![SemanticsAction::ScrollRight]
        );
        assert_eq!(
            handler.offered_actions().len(),
            2,
            "the callback is still there"
        );
    }

    #[test]
    fn the_filter_can_take_away_a_tap_the_callback_offered() {
        // The doc's own example.
        let mut handler = RenderSemanticsGestureHandler::new();
        handler.has_on_tap = true;
        assert_eq!(handler.advertised_actions(), vec![SemanticsAction::Tap]);

        handler.valid_actions = Some(vec![SemanticsAction::LongPress]);
        assert!(
            handler.advertised_actions().is_empty(),
            "it will not claim to support taps"
        );
    }

    #[test]
    fn no_filter_at_all_allows_everything_the_callbacks_offer() {
        let mut handler = RenderSemanticsGestureHandler::new();
        handler.has_on_tap = true;
        handler.has_on_vertical_drag_update = true;
        assert_eq!(handler.valid_actions, None);
        assert_eq!(handler.advertised_actions(), handler.offered_actions());
        assert_eq!(handler.advertised_actions().len(), 3);
    }

    // -- Merging requires being something to merge into ------------------------------

    #[test]
    fn merging_makes_this_node_a_boundary_as_well() {
        // Everything below folds into this node, and a node you can fold into is
        // what a boundary is.
        assert!(RenderMergeSemantics::is_semantic_boundary());
        assert!(RenderMergeSemantics::is_merging_semantics_of_descendants());
    }

    // -- Paint order, not tree order ---------------------------------------------------

    #[test]
    fn blocking_hides_what_was_painted_before_it() {
        // Which is what a dialog needs: it is painted after the page, and it is
        // not the page's ancestor.
        let blocker = RenderBlockSemantics::new(true);
        assert!(blocker.blocking());
        assert!(!RenderBlockSemantics::new(false).blocking());
    }

    #[test]
    fn every_setter_in_the_family_wakes_the_semantics_only_on_a_real_change() {
        let mut blocker = RenderBlockSemantics::new(true);
        blocker.set_blocking(true);
        assert!(!blocker.needs_semantics_update, "unchanged, so silent");

        blocker.set_blocking(false);
        assert!(blocker.needs_semantics_update);

        let mut excluder = RenderExcludeSemantics::new(false);
        excluder.set_excluding(false);
        assert!(!excluder.needs_semantics_update);
        excluder.set_excluding(true);
        assert!(excluder.needs_semantics_update);

        let mut indexed = RenderIndexedSemantics::new(3);
        indexed.set_index(3);
        assert!(!indexed.needs_semantics_update);
        indexed.set_index(4);
        assert!(indexed.needs_semantics_update);
        assert_eq!(indexed.index(), 4);
    }

    // -- A walk that never gets there ---------------------------------------------------

    #[test]
    fn exclusion_is_a_traversal_turning_back_rather_than_a_flag_on_the_subtree() {
        let excluding = RenderExcludeSemantics::new(true);
        assert!(
            !excluding.visits_children(),
            "the descendants are never asked what they would have said"
        );
        assert!(RenderExcludeSemantics::new(false).visits_children());
    }

    #[test]
    fn the_annotations_render_object_stops_the_walk_the_same_way() {
        let mut annotations = RenderSemanticsAnnotations::new();
        assert!(annotations.visits_children());
        annotations.exclude_semantics = true;
        assert!(!annotations.visits_children());
    }

    #[test]
    fn and_the_sliver_twin_behaves_identically() {
        let mut sliver = RenderSliverSemanticsAnnotations::new();
        assert!(sliver.visits_children());
        assert!(!sliver.is_semantic_boundary());

        sliver.annotations.exclude_semantics = true;
        sliver.annotations.container = true;
        assert!(!sliver.visits_children());
        assert!(sliver.is_semantic_boundary());
    }

    // -- Sized decides whether the annotation has a shape ---------------------------------

    #[test]
    fn an_unsized_region_answers_for_any_point_a_search_reaches() {
        let region = RenderAnnotatedRegion::new(7u32, false);
        assert!(region.finds_at((5.0, 5.0), (10.0, 10.0)));
        assert!(region.finds_at((500.0, 500.0), (10.0, 10.0)));
    }

    #[test]
    fn and_a_sized_one_only_for_points_inside_it() {
        let region = RenderAnnotatedRegion::new(7u32, true);
        assert!(region.finds_at((5.0, 5.0), (10.0, 10.0)));
        assert!(!region.finds_at((11.0, 5.0), (10.0, 10.0)));
        assert!(!region.finds_at((-1.0, 5.0), (10.0, 10.0)));
    }

    #[test]
    fn a_container_asks_to_be_a_boundary_directly() {
        let mut annotations = RenderSemanticsAnnotations::new();
        assert!(!annotations.is_semantic_boundary());
        annotations.container = true;
        assert!(annotations.is_semantic_boundary());
    }
}
