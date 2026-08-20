//! Views, and the zones they divide the widget tree into -- a port of
//! upstream's `widgets/view.dart`.
//!
//! An application usually has one window and one widget tree, and the two
//! coincide so completely that nothing has to say so. These four widgets are
//! what happens when they do not: a tooltip that may extend past the edge of
//! the main window, a second monitor, a platform view hosted alongside.
//!
//! The idea that makes them work is **zones**. Most of the tree is a
//! *rendering* zone: every widget in it eventually produces something a render
//! object paints into one view. A [`ViewAnchor`]'s side slot and a
//! [`ViewCollection`]'s children are *non-rendering* zones -- the widgets
//! there build no render objects of their own, they only carry [`View`]s, and
//! each of those starts a rendering zone again for its own window.
//!
//! The rule that falls out is the one thing worth remembering here: **no
//! render-object widget may sit between an anchor and the next `View`**.
//! Anything that painted in that gap would have no view to paint into.
//! Inherited widgets are fine, and are the point -- the side view sees
//! everything the anchor could.
//!
//! ## What is not here
//!
//! `View` bootstraps a `RenderView` and a `PipelineOwner` and registers them
//! with the binding, none of which this crate models the way upstream does.
//! What is ported is the zone rule and the deprecated-pair constraint that
//! `View` and `RawView` both assert.

/// A window a tree can be drawn into -- upstream's `FlutterView`, identified
/// here by number.
pub type FlutterViewId = u64;

/// Which kind of zone a part of the tree is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeZone {
    /// Widgets here build render objects that paint into the enclosing view.
    Rendering,
    /// Widgets here build no render objects. They carry [`View`]s, and each
    /// one opens a rendering zone of its own.
    NonRendering,
}

/// Upstream `View`: bootstraps a render tree for one window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub view: FlutterViewId,
    /// Upstream's pair of `deprecatedDoNotUseWillBeRemovedWithoutNotice…`
    /// parameters, which exist only to keep two removed binding properties
    /// working.
    deprecated_pipeline_owner: Option<u64>,
    deprecated_render_view: Option<u64>,
}

impl View {
    pub fn new(view: FlutterViewId) -> View {
        View {
            view,
            deprecated_pipeline_owner: None,
            deprecated_render_view: None,
        }
    }

    /// Upstream's two assertions on the deprecated pair, and they are worth
    /// keeping: the two must be supplied **together or not at all**, and the
    /// render view supplied must be the one for this window.
    ///
    /// Half a pair would leave a render tree owned by one pipeline and
    /// registered with another; a mismatched view would draw the application
    /// into somebody else's window.
    pub fn with_deprecated_pair(
        mut self,
        pipeline_owner: Option<u64>,
        render_view: Option<(u64, FlutterViewId)>,
    ) -> Option<View> {
        if pipeline_owner.is_none() != render_view.is_none() {
            return None;
        }
        if let Some((_, its_view)) = render_view {
            if its_view != self.view {
                return None;
            }
        }
        self.deprecated_pipeline_owner = pipeline_owner;
        self.deprecated_render_view = render_view.map(|(id, _)| id);
        Some(self)
    }

    pub fn deprecated_pair(&self) -> (Option<u64>, Option<u64>) {
        (self.deprecated_pipeline_owner, self.deprecated_render_view)
    }

    /// The zone a `View`'s child is in: it opens a rendering zone for its own
    /// window, wherever it was placed.
    pub fn child_zone(&self) -> TreeZone {
        TreeZone::Rendering
    }
}

/// Upstream `RawView`: `View` without the binding-observer state.
///
/// The split exists because `View` is stateful only to watch for the window
/// going away, and a caller that manages that itself -- the binding, during
/// startup -- wants the widget without the observer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawView {
    pub view: FlutterViewId,
    deprecated_pipeline_owner: Option<u64>,
    deprecated_render_view: Option<u64>,
}

impl RawView {
    pub fn new(view: FlutterViewId) -> RawView {
        RawView {
            view,
            deprecated_pipeline_owner: None,
            deprecated_render_view: None,
        }
    }

    /// The same pair, with the same two assertions.
    pub fn with_deprecated_pair(
        mut self,
        pipeline_owner: Option<u64>,
        render_view: Option<(u64, FlutterViewId)>,
    ) -> Option<RawView> {
        if pipeline_owner.is_none() != render_view.is_none() {
            return None;
        }
        if let Some((_, its_view)) = render_view {
            if its_view != self.view {
                return None;
            }
        }
        self.deprecated_pipeline_owner = pipeline_owner;
        self.deprecated_render_view = render_view.map(|(id, _)| id);
        Some(self)
    }

    /// Upstream's `build`, which wraps the child in two scopes: one naming the
    /// view, one naming the pipeline owner. The second is what lets a nested
    /// `View` find something to attach its own pipeline to -- a side view is a
    /// child of this tree even though it paints somewhere else.
    pub fn scopes(&self) -> (FlutterViewId, TreeZone) {
        (self.view, TreeZone::Rendering)
    }
}

/// What is wrong with a placement, if anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneError {
    /// A render-object widget between an anchor and the next `View`. It would
    /// have no view to paint into.
    RenderObjectInNonRenderingZone,
    /// Something other than a `View` reached the bottom of a non-rendering
    /// zone.
    MissingView,
}

/// One widget on the path from an anchor down to a view, for checking the
/// zone rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneNode {
    /// An inherited widget, a builder, anything that builds no render object.
    Passthrough,
    /// A widget that builds a render object.
    RenderObject,
    /// A `View`, which ends the non-rendering zone.
    View,
}

/// Upstream's rule for the contents of a [`ViewAnchor::view`] slot or a
/// [`ViewCollection`]'s children, as a check.
///
/// Upstream states it in prose and enforces it with an assertion deep in the
/// element; the shape is what matters. **Non-render-object widgets are
/// allowed and expected** -- an `InheritedWidget` between the anchor and the
/// view is how a side view inherits a theme -- and a render object is not,
/// because it would be asked to paint before anything had said where.
pub fn check_non_rendering_zone(path: &[ZoneNode]) -> Result<(), ZoneError> {
    for node in path {
        match node {
            ZoneNode::Passthrough => continue,
            ZoneNode::RenderObject => return Err(ZoneError::RenderObjectInNonRenderingZone),
            ZoneNode::View => return Ok(()),
        }
    }
    Err(ZoneError::MissingView)
}

/// Upstream `ViewCollection`: several views and nothing else.
///
/// It has **no child** -- only views -- which is what distinguishes it from
/// [`ViewAnchor`]. A collection is used where the surrounding tree renders
/// nothing itself and simply hosts windows, which is what the root of a
/// multi-window application looks like.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewCollection {
    pub views: Vec<FlutterViewId>,
}

impl ViewCollection {
    pub fn new(views: Vec<FlutterViewId>) -> ViewCollection {
        ViewCollection { views }
    }

    pub fn views_zone(&self) -> TreeZone {
        TreeZone::NonRendering
    }
}

/// Upstream `ViewAnchor`: a widget in one view with another view attached to
/// its side.
///
/// The example upstream gives is the one that explains the whole file: a
/// tooltip that has to extend past the edge of the main window. It cannot be a
/// child of the button, because a child is clipped by the window; so it
/// becomes its own view, anchored to the button by wrapping the button in one
/// of these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewAnchor {
    /// Upstream's `view`, **optional**: an anchor with nothing attached is a
    /// legitimate state, and is what a tooltip's anchor looks like while the
    /// tooltip is not showing. Making it required would force callers to
    /// build and discard a view on every frame.
    pub view: Option<FlutterViewId>,
}

impl ViewAnchor {
    pub fn new(view: Option<FlutterViewId>) -> ViewAnchor {
        ViewAnchor { view }
    }

    /// The anchor's own child stays in the surrounding view.
    pub fn child_zone(&self) -> TreeZone {
        TreeZone::Rendering
    }

    /// The side slot starts a new non-rendering zone.
    pub fn view_zone(&self) -> TreeZone {
        TreeZone::NonRendering
    }

    /// Upstream wraps the side view in a `LookupBoundary`, and only the side
    /// view.
    ///
    /// That boundary is the point of the widget stated precisely: the side
    /// view may **read** everything above the anchor -- inherited themes,
    /// directionality -- but nothing inside it may **look up** through the
    /// anchor and find the surrounding render tree. It renders elsewhere, and
    /// a descendant that found the main view's render object would be asking
    /// about the wrong window.
    pub fn view_is_lookup_bounded(&self) -> bool {
        self.view.is_some()
    }

    /// How many children the anchor actually builds: the child always, the
    /// side view only when there is one.
    pub fn view_slot_len(&self) -> usize {
        usize::from(self.view.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_render_object_between_the_anchor_and_the_view_has_nowhere_to_paint() {
        // It would be asked to paint before anything had said where.
        assert_eq!(
            check_non_rendering_zone(&[ZoneNode::RenderObject, ZoneNode::View]),
            Err(ZoneError::RenderObjectInNonRenderingZone)
        );
    }

    #[test]
    fn inherited_widgets_between_the_anchor_and_the_view_are_the_point() {
        // A theme between the anchor and the view is how a side view inherits
        // one.
        assert_eq!(
            check_non_rendering_zone(&[
                ZoneNode::Passthrough,
                ZoneNode::Passthrough,
                ZoneNode::View,
            ]),
            Ok(())
        );
    }

    #[test]
    fn a_non_rendering_zone_that_never_reaches_a_view_is_a_dead_end() {
        assert_eq!(
            check_non_rendering_zone(&[ZoneNode::Passthrough]),
            Err(ZoneError::MissingView)
        );
        assert_eq!(check_non_rendering_zone(&[]), Err(ZoneError::MissingView));
    }

    #[test]
    fn nothing_below_the_view_is_checked_because_the_zone_ended_there() {
        // A View opens a rendering zone of its own, so a render object under
        // it is exactly what is expected.
        assert_eq!(
            check_non_rendering_zone(&[ZoneNode::View, ZoneNode::RenderObject]),
            Ok(())
        );
    }

    #[test]
    fn the_anchors_own_child_stays_in_the_surrounding_view() {
        let anchor = ViewAnchor::new(Some(7));
        assert_eq!(anchor.child_zone(), TreeZone::Rendering);
        assert_eq!(anchor.view_zone(), TreeZone::NonRendering);
        assert_eq!(View::new(7).child_zone(), TreeZone::Rendering);
    }

    #[test]
    fn an_anchor_with_nothing_attached_is_a_legitimate_state() {
        // Which is what a tooltip's anchor looks like while the tooltip is not
        // showing. Requiring the view would force building and discarding one
        // every frame.
        let empty = ViewAnchor::new(None);
        assert_eq!(empty.view_slot_len(), 0);
        assert!(!empty.view_is_lookup_bounded());

        let showing = ViewAnchor::new(Some(7));
        assert_eq!(showing.view_slot_len(), 1);
        assert!(
            showing.view_is_lookup_bounded(),
            "and only the side view is bounded"
        );
    }

    #[test]
    fn a_collection_hosts_views_and_nothing_else() {
        // Which is what distinguishes it from an anchor: no child of its own.
        let collection = ViewCollection::new(vec![1, 2, 3]);
        assert_eq!(collection.views.len(), 3);
        assert_eq!(collection.views_zone(), TreeZone::NonRendering);
        assert!(ViewCollection::default().views.is_empty());
    }

    #[test]
    fn the_deprecated_pair_has_to_be_supplied_together_or_not_at_all() {
        // Half a pair leaves a render tree owned by one pipeline and
        // registered with another.
        assert!(View::new(7).with_deprecated_pair(None, None).is_some());
        assert!(
            View::new(7)
                .with_deprecated_pair(Some(1), Some((2, 7)))
                .is_some()
        );
        assert!(View::new(7).with_deprecated_pair(Some(1), None).is_none());
        assert!(
            View::new(7)
                .with_deprecated_pair(None, Some((2, 7)))
                .is_none()
        );
    }

    #[test]
    fn a_render_view_for_a_different_window_is_refused() {
        // It would draw the application into somebody else's window.
        assert!(
            View::new(7)
                .with_deprecated_pair(Some(1), Some((2, 9)))
                .is_none()
        );
        assert!(
            RawView::new(7)
                .with_deprecated_pair(Some(1), Some((2, 9)))
                .is_none()
        );
        assert!(
            RawView::new(7)
                .with_deprecated_pair(Some(1), Some((2, 7)))
                .is_some()
        );
    }

    #[test]
    fn the_pair_is_carried_through_rather_than_dropped() {
        let view = View::new(7)
            .with_deprecated_pair(Some(11), Some((22, 7)))
            .unwrap();
        assert_eq!(view.deprecated_pair(), (Some(11), Some(22)));
    }

    #[test]
    fn a_raw_view_names_its_window_and_opens_a_rendering_zone() {
        assert_eq!(RawView::new(7).scopes(), (7, TreeZone::Rendering));
    }
}
