//! A port of `widgets/adapter.dart`: `RenderObjectToWidgetAdapter` and
//! `RenderObjectToWidgetElement`.
//!
//! The graft. Everywhere else in the framework a widget describes a render
//! object and the framework builds it; here the render object **already
//! exists** -- it is the view the engine gave us -- and a widget tree has to be
//! attached underneath it. This is what `runApp` does, and the inversion is the
//! whole design.

/// Which path [`RenderObjectToWidgetAdapter::attach_to_render_tree`] took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttachPath {
    /// Nothing was there. The element is created, given the owner and mounted,
    /// all inside a locked state and a build scope -- imperatively, right now,
    /// because there is no frame in progress to defer to.
    Mounted,
    /// An element is already attached to this container. Rather than replacing
    /// it, the new widget is **stashed** and a rebuild is scheduled: the
    /// existing element tree is kept and reconciled against the new widget.
    Scheduled,
}

/// Upstream `RenderObjectToWidgetAdapter`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderObjectToWidgetAdapter {
    /// The render object the widget tree is being grafted onto.
    pub container: u64,
    pub child: Option<u64>,
}

impl RenderObjectToWidgetAdapter {
    pub fn new(container: u64, child: Option<u64>) -> RenderObjectToWidgetAdapter {
        RenderObjectToWidgetAdapter { container, child }
    }

    /// Upstream's key is `GlobalObjectKey(container)` -- **the container itself
    /// is the identity**.
    ///
    /// Which is what makes calling `runApp` a second time replace the
    /// application rather than restart it: two adapters over the same view are
    /// the same widget, so the element tree underneath is reconciled instead of
    /// thrown away. Hot reload lives here.
    pub fn key(&self) -> u64 {
        self.container
    }

    /// Upstream `createRenderObject`, which **returns the container it was
    /// given** rather than making anything.
    ///
    /// A render object widget that does not create a render object reads as a
    /// contradiction until you see what it is for: the render tree came first,
    /// and this widget is the framework agreeing to pretend it built it.
    pub fn create_render_object(&self) -> u64 {
        self.container
    }

    /// Upstream `updateRenderObject`, whose body is empty. The container was
    /// never configured by this widget, so there is nothing about it to update.
    pub fn update_render_object(&self) {}

    /// Upstream `attachToRenderTree`.
    pub fn attach_to_render_tree(&self, existing: Option<u64>) -> AttachPath {
        match existing {
            None => AttachPath::Mounted,
            Some(_) => AttachPath::Scheduled,
        }
    }
}

/// What a rebuild of the root element did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootRebuild {
    /// A stashed widget was taken and applied.
    UpdatedToStashedWidget,
    /// There was nothing stashed. Upstream's comment names the case: a
    /// reassemble -- a hot reload -- rebuilds the root without handing it a new
    /// widget.
    NothingStashed,
}

/// Upstream `RenderObjectToWidgetElement`.
///
/// The root of an element tree, and it can only ever be that: `mount` asserts
/// its parent is null.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderObjectToWidgetElement {
    child: Option<u64>,
    stashed_widget: Option<u64>,
    render_child: Option<u64>,
}

impl RenderObjectToWidgetElement {
    /// Upstream's `_rootChildSlot`, a `static const Object()`. There is exactly
    /// one child slot, and its identity is the whole of it.
    pub const ROOT_CHILD_SLOT: u64 = 0;

    pub fn new() -> RenderObjectToWidgetElement {
        RenderObjectToWidgetElement::default()
    }

    pub fn child(&self) -> Option<u64> {
        self.child
    }

    pub fn render_child(&self) -> Option<u64> {
        self.render_child
    }

    pub fn has_stashed_widget(&self) -> bool {
        self.stashed_widget.is_some()
    }

    /// Upstream's `_newWidget`, set by `attachToRenderTree` and taken by
    /// `performRebuild`.
    pub fn stash(&mut self, widget: u64) {
        self.stashed_widget = Some(widget);
    }

    /// Upstream `mount`, which may only be called with a null parent.
    pub fn mount(&mut self, parent: Option<u64>, child: Option<u64>) -> Result<(), &'static str> {
        if parent.is_some() {
            return Err("a RenderObjectToWidgetElement can only be the root of a tree");
        }
        self.rebuild(child);
        Ok(())
    }

    /// Upstream `performRebuild`, which takes the stashed widget if there is
    /// one and then rebuilds either way.
    pub fn perform_rebuild(&mut self, child: Option<u64>) -> RootRebuild {
        let outcome = if self.stashed_widget.take().is_some() {
            RootRebuild::UpdatedToStashedWidget
        } else {
            RootRebuild::NothingStashed
        };
        self.rebuild(child);
        outcome
    }

    fn rebuild(&mut self, child: Option<u64>) {
        self.child = child;
    }

    /// Upstream `_rebuild`'s `catch`.
    ///
    /// A build failure at the root does not take the application down; it puts
    /// the error widget up. And note the argument: `updateChild(**null**,
    /// error, slot)` -- the failed subtree is **discarded** rather than
    /// reconciled against the error widget, because reconciling against a tree
    /// that threw while building is asking the same question again.
    pub fn rebuild_failed(&mut self, error_widget: u64) {
        self.child = Some(error_widget);
    }

    /// Upstream `insertRenderObjectChild`, which sets the container's one
    /// child. The slot is asserted rather than matched -- there is only one.
    pub fn insert_render_object_child(
        &mut self,
        child: u64,
        slot: u64,
    ) -> Result<(), &'static str> {
        if slot != RenderObjectToWidgetElement::ROOT_CHILD_SLOT {
            return Err("the root element has exactly one slot");
        }
        self.render_child = Some(child);
        Ok(())
    }

    /// Upstream `removeRenderObjectChild`.
    pub fn remove_render_object_child(&mut self) {
        self.render_child = None;
    }

    /// Upstream `moveRenderObjectChild`, whose body is `assert(false)`.
    ///
    /// Not "unsupported" -- **impossible**. With one slot there is nowhere to
    /// move to, so reaching this is a framework bug rather than a caller doing
    /// something unusual.
    pub fn move_render_object_child_is_reachable() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_container_itself_is_the_identity() {
        // Which is what makes calling runApp a second time replace the
        // application rather than restart it: two adapters over the same view
        // are the same widget, so the element tree underneath is reconciled.
        let first = RenderObjectToWidgetAdapter::new(7, Some(100));
        let second = RenderObjectToWidgetAdapter::new(7, Some(200));
        assert_eq!(first.key(), second.key());

        let other_view = RenderObjectToWidgetAdapter::new(8, Some(100));
        assert_ne!(first.key(), other_view.key());
    }

    #[test]
    fn a_render_object_widget_that_creates_no_render_object() {
        // The render tree came first; this widget is the framework agreeing to
        // pretend it built it.
        let adapter = RenderObjectToWidgetAdapter::new(7, None);
        assert_eq!(adapter.create_render_object(), 7);
        adapter.update_render_object();
    }

    #[test]
    fn the_first_attach_mounts_and_every_later_one_schedules() {
        // There is no frame in progress to defer the first one to, and no
        // reason to throw away an element tree for the rest.
        let adapter = RenderObjectToWidgetAdapter::new(7, Some(100));
        assert_eq!(adapter.attach_to_render_tree(None), AttachPath::Mounted);
        assert_eq!(
            adapter.attach_to_render_tree(Some(42)),
            AttachPath::Scheduled
        );
    }

    #[test]
    fn this_element_can_only_ever_be_a_root() {
        let mut element = RenderObjectToWidgetElement::new();
        assert!(element.mount(Some(1), Some(100)).is_err());
        assert!(element.mount(None, Some(100)).is_ok());
        assert_eq!(element.child(), Some(100));
    }

    #[test]
    fn a_stashed_widget_is_taken_on_the_next_rebuild_and_only_once() {
        let mut element = RenderObjectToWidgetElement::new();
        element.mount(None, Some(100)).unwrap();
        element.stash(200);
        assert!(element.has_stashed_widget());

        assert_eq!(
            element.perform_rebuild(Some(200)),
            RootRebuild::UpdatedToStashedWidget
        );
        assert!(!element.has_stashed_widget());
        assert_eq!(
            element.perform_rebuild(Some(200)),
            RootRebuild::NothingStashed
        );
    }

    #[test]
    fn a_hot_reload_rebuilds_the_root_without_handing_it_a_new_widget() {
        // Upstream names the case in a comment: _newWidget can be null if we
        // were rebuilt due to a reassemble.
        let mut element = RenderObjectToWidgetElement::new();
        element.mount(None, Some(100)).unwrap();
        assert_eq!(
            element.perform_rebuild(Some(100)),
            RootRebuild::NothingStashed
        );
        assert_eq!(element.child(), Some(100));
    }

    #[test]
    fn a_build_failure_at_the_root_puts_the_error_widget_up_rather_than_going_down() {
        // And it discards the failed subtree rather than reconciling against
        // it: reconciling with a tree that threw while building is asking the
        // same question again.
        let mut element = RenderObjectToWidgetElement::new();
        element.mount(None, Some(100)).unwrap();
        element.rebuild_failed(999);
        assert_eq!(element.child(), Some(999));
    }

    #[test]
    fn the_root_has_exactly_one_slot() {
        let mut element = RenderObjectToWidgetElement::new();
        assert!(
            element
                .insert_render_object_child(50, RenderObjectToWidgetElement::ROOT_CHILD_SLOT)
                .is_ok()
        );
        assert_eq!(element.render_child(), Some(50));
        assert!(element.insert_render_object_child(51, 1).is_err());

        element.remove_render_object_child();
        assert_eq!(element.render_child(), None);
    }

    #[test]
    fn moving_the_root_child_is_impossible_rather_than_unsupported() {
        // With one slot there is nowhere to move to, so reaching it is a
        // framework bug and not a caller doing something unusual.
        assert!(!RenderObjectToWidgetElement::move_render_object_child_is_reachable());
    }
}
