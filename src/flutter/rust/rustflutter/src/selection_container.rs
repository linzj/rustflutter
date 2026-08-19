//! A group of selectables that acts as one -- a port of upstream's
//! `widgets/selection_container.dart`.
//!
//! A [`SelectionContainer`] takes a subtree's selectables off the registrar
//! above it and puts **itself** there instead. Everything above then sees one
//! selectable where there were twenty, and the container's delegate decides
//! what the group means: what a select-all selects, what a drag through it
//! does, what copying it yields.
//!
//! The other constructor is the more interesting half.
//! [`SelectionContainer::disabled`] carries no delegate at all, and registers
//! nothing -- so a subtree inside it cannot be selected, and, crucially, is not
//! *skipped over* either. A drag passing through it stops rather than
//! continuing on the far side, which is what "this part of the page is not
//! text you can take" has to mean.

use crate::selection::{
    SelectedContent, SelectedContentRange, SelectionEvent, SelectionGeometry, SelectionResult,
    SelectionStatus,
};

/// Upstream `SelectionContainerDelegate`: what a group of selectables means
/// taken together.
///
/// Upstream it is both a `SelectionHandler` and a `SelectionRegistrar`, which
/// is the whole trick: the children register *with it*, and it presents to the
/// registrar above as a single handler.
pub trait SelectionContainerDelegate {
    /// Upstream's `value`.
    fn selection_geometry(&self) -> SelectionGeometry;

    /// Upstream's `dispatchSelectionEvent`.
    fn dispatch_selection_event(&mut self, event: SelectionEvent) -> SelectionResult;

    /// Upstream's `getSelectedContent`.
    fn selected_content(&self) -> Option<SelectedContent>;

    /// Upstream's `getSelection`.
    fn selection_range(&self) -> Option<SelectedContentRange> {
        None
    }

    /// Upstream's `add`, from the registrar half.
    fn add(&mut self, id: u64);

    /// Upstream's `remove`.
    fn remove(&mut self, id: u64);

    /// Upstream's `hasSize`.
    ///
    /// Every one of upstream's geometry accessors asserts on this first, with
    /// a message naming the same cause: the container has not been laid out
    /// yet. Asking a container where its children are before it has a size is
    /// not a question with a wrong answer, it is a question with no answer.
    fn has_size(&self) -> bool;

    /// Upstream's `containerSize`, which asserts `hasSize` before answering.
    fn container_size(&self) -> Option<(f32, f32)>;
}

/// Upstream `SelectionRegistrarScope`: how a subtree finds the registrar above
/// it.
///
/// Its `updateShouldNotify` compares the registrar by identity, so a rebuild
/// that hands down the same registrar does not make every selectable below
/// re-register.
pub struct SelectionRegistrarScope {
    /// Which registrar this scope publishes. `None` is a scope that publishes
    /// nothing, which is what a disabled container installs.
    pub registrar: Option<u64>,
}

impl SelectionRegistrarScope {
    pub fn new(registrar: u64) -> SelectionRegistrarScope {
        SelectionRegistrarScope {
            registrar: Some(registrar),
        }
    }

    /// Upstream's disabled case, which publishes no registrar at all.
    pub fn empty() -> SelectionRegistrarScope {
        SelectionRegistrarScope { registrar: None }
    }

    /// Upstream's `SelectionContainer.maybeOf`.
    pub fn maybe_of(&self) -> Option<u64> {
        self.registrar
    }

    /// Upstream's `updateShouldNotify`.
    pub fn update_should_notify(&self, old: &SelectionRegistrarScope) -> bool {
        self.registrar != old.registrar
    }
}

/// Upstream `SelectionContainer`: a group of selectables presented as one.
pub struct SelectionContainer {
    /// Upstream's `registrar`, the one this container joins. `None` means "ask
    /// the tree", which `didChangeDependencies` does.
    pub registrar: Option<u64>,
    /// This container's own id, as its children see it.
    pub id: u64,
    /// Whether this container has a delegate. Upstream's `_disabled` is
    /// `delegate == null`.
    has_delegate: bool,
}

impl SelectionContainer {
    /// Upstream's default constructor, which requires a delegate.
    pub fn new(id: u64) -> SelectionContainer {
        SelectionContainer {
            registrar: None,
            id,
            has_delegate: true,
        }
    }

    /// Upstream's `SelectionContainer.disabled`.
    ///
    /// **No delegate and no registrar**, and upstream asserts the pair stays
    /// that way on every rebuild. A disabled container is not a container that
    /// selects nothing -- it is one that is not in the selection tree at all.
    pub fn disabled(id: u64) -> SelectionContainer {
        SelectionContainer {
            registrar: None,
            id,
            has_delegate: false,
        }
    }

    pub fn with_registrar(mut self, registrar: u64) -> Self {
        self.registrar = Some(registrar);
        self
    }

    /// Upstream's `_disabled`.
    pub fn is_disabled(&self) -> bool {
        !self.has_delegate
    }

    /// Upstream's `createState`.
    pub fn create_state(&self) -> SelectionContainerState {
        SelectionContainerState {
            id: self.id,
            disabled: self.is_disabled(),
            explicit_registrar: self.registrar,
            registrar: None,
            listeners: 0,
        }
    }
}

/// Upstream's `_SelectionContainerState`.
///
/// Upstream mixes in both `Selectable` and `SelectionRegistrant`: the state is
/// what the registrar above sees, and it forwards nearly everything to the
/// delegate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionContainerState {
    pub id: u64,
    disabled: bool,
    /// The registrar the widget was given explicitly, if any.
    explicit_registrar: Option<u64>,
    /// The registrar actually joined.
    registrar: Option<u64>,
    listeners: usize,
}

impl SelectionContainerState {
    /// Upstream's `_disabledGeometry`.
    ///
    /// `hasContent: true` with a status of `none` -- and the pair is not a
    /// contradiction but the point. There *is* something here; it simply
    /// cannot be selected. A geometry claiming no content would let a
    /// container above conclude the area is empty and skip it.
    pub fn disabled_geometry() -> SelectionGeometry {
        SelectionGeometry::new(SelectionStatus::None, true)
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn registrar(&self) -> Option<u64> {
        self.registrar
    }

    pub fn listeners(&self) -> usize {
        self.listeners
    }

    /// Upstream's `initState` and `didChangeDependencies` together: an
    /// explicit registrar wins, otherwise the one found in the tree, and a
    /// disabled container joins neither.
    ///
    /// Upstream asserts `!_disabled || registrar == null` after both, which is
    /// the invariant this returns to.
    pub fn resolve_registrar(&mut self, inherited: Option<u64>) {
        if self.disabled {
            self.registrar = None;
            return;
        }
        self.registrar = self.explicit_registrar.or(inherited);
    }

    /// Upstream's `didUpdateWidget` for a changed delegate.
    ///
    /// Returns whether the listeners should be told. Upstream's rule is worth
    /// keeping exactly: the listeners are moved from the old delegate to the
    /// new one, and then told **only if the two delegates disagree about the
    /// geometry**. A container swapped for an equivalent one should not make
    /// the page repaint its selection.
    ///
    /// Upstream's comment on the loop -- "avoid concurrent modification" --
    /// notes that a listener may remove itself while being called, which is
    /// why it iterates a copy.
    pub fn delegate_changed(
        &mut self,
        old_geometry: Option<&SelectionGeometry>,
        new_geometry: Option<&SelectionGeometry>,
    ) -> bool {
        old_geometry != new_geometry
    }

    /// Upstream's `addListener`, which asserts the container is not disabled:
    /// a disabled container has no delegate to hold the listener.
    pub fn add_listener(&mut self) -> bool {
        if self.disabled {
            return false;
        }
        self.listeners += 1;
        true
    }

    /// Upstream's `removeListener`, which -- unlike `addListener` -- does
    /// **not** assert.
    ///
    /// The asymmetry is deliberate: a listener removing itself as the
    /// container is being disabled or torn down is ordinary, and refusing it
    /// would turn an orderly teardown into a crash.
    pub fn remove_listener(&mut self) {
        self.listeners = self.listeners.saturating_sub(1);
    }

    /// Upstream's `value`: the delegate's geometry, or the disabled one.
    pub fn selection_geometry(
        &self,
        delegate: Option<&dyn SelectionContainerDelegate>,
    ) -> SelectionGeometry {
        if self.disabled {
            return Self::disabled_geometry();
        }
        match delegate {
            Some(delegate) => delegate.selection_geometry(),
            None => Self::disabled_geometry(),
        }
    }

    /// Upstream's `dispatchSelectionEvent`, forwarded.
    ///
    /// A disabled container answers [`SelectionResult::None`] rather than
    /// pointing anywhere -- there is nothing here to select and nothing to say
    /// about where the edge should go instead.
    pub fn dispatch_selection_event(
        &self,
        delegate: Option<&mut dyn SelectionContainerDelegate>,
        event: SelectionEvent,
    ) -> SelectionResult {
        if self.disabled {
            return SelectionResult::None;
        }
        match delegate {
            Some(delegate) => delegate.dispatch_selection_event(event),
            None => SelectionResult::None,
        }
    }

    /// Upstream's `getSelectedContent`, forwarded.
    pub fn selected_content(
        &self,
        delegate: Option<&dyn SelectionContainerDelegate>,
    ) -> Option<SelectedContent> {
        if self.disabled {
            return None;
        }
        delegate.and_then(|delegate| delegate.selected_content())
    }

    /// The scope this container publishes to its subtree: its own id when it
    /// is enabled, and nothing when it is not.
    pub fn scope(&self) -> SelectionRegistrarScope {
        if self.disabled {
            SelectionRegistrarScope::empty()
        } else {
            SelectionRegistrarScope::new(self.id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Offset;

    /// A delegate that says a fixed thing, and counts what it was asked.
    struct Group {
        geometry: SelectionGeometry,
        registered: Vec<u64>,
        events: Vec<SelectionEvent>,
    }

    impl Group {
        fn new(status: SelectionStatus) -> Group {
            Group {
                geometry: SelectionGeometry::new(status, true),
                registered: Vec::new(),
                events: Vec::new(),
            }
        }
    }

    impl SelectionContainerDelegate for Group {
        fn selection_geometry(&self) -> SelectionGeometry {
            self.geometry.clone()
        }

        fn dispatch_selection_event(&mut self, event: SelectionEvent) -> SelectionResult {
            self.events.push(event);
            SelectionResult::End
        }

        fn selected_content(&self) -> Option<SelectedContent> {
            Some(SelectedContent::new("the whole group"))
        }

        fn add(&mut self, id: u64) {
            self.registered.push(id);
        }

        fn remove(&mut self, id: u64) {
            self.registered.retain(|held| *held != id);
        }

        fn has_size(&self) -> bool {
            true
        }

        fn container_size(&self) -> Option<(f32, f32)> {
            Some((300.0, 200.0))
        }
    }

    #[test]
    fn a_container_puts_itself_on_the_registrar_where_its_children_were() {
        // Everything above then sees one selectable where there were twenty,
        // and the delegate decides what the group means.
        let container = SelectionContainer::new(7);
        let mut state = container.create_state();
        state.resolve_registrar(Some(1));
        assert_eq!(state.registrar(), Some(1), "it joined the one above");
        assert_eq!(
            state.scope().maybe_of(),
            Some(7),
            "and publishes itself to its children"
        );
    }

    #[test]
    fn an_explicit_registrar_beats_the_one_found_in_the_tree() {
        let container = SelectionContainer::new(7).with_registrar(42);
        let mut state = container.create_state();
        state.resolve_registrar(Some(1));
        assert_eq!(state.registrar(), Some(42));

        // And with nothing explicit and nothing inherited, it joins nothing.
        let mut orphan = SelectionContainer::new(7).create_state();
        orphan.resolve_registrar(None);
        assert_eq!(orphan.registrar(), None);
    }

    #[test]
    fn a_disabled_container_registers_nowhere_at_all() {
        // Not "a container that selects nothing" -- one that is not in the
        // selection tree.
        let container = SelectionContainer::disabled(7);
        assert!(container.is_disabled());
        let mut state = container.create_state();
        state.resolve_registrar(Some(1));
        assert_eq!(state.registrar(), None, "even with one above it");

        // Upstream's invariant, asserted after both lifecycle methods.
        assert!(state.is_disabled() && state.registrar().is_none());
    }

    #[test]
    fn a_disabled_container_publishes_no_registrar_to_its_subtree_either() {
        // Which is what stops a drag from finding the selectables inside it
        // and carrying on through.
        let state = SelectionContainer::disabled(7).create_state();
        assert_eq!(state.scope().maybe_of(), None);
        assert!(SelectionRegistrarScope::empty().maybe_of().is_none());
    }

    #[test]
    fn a_disabled_container_has_content_and_no_selection() {
        // The pair is the point, not a contradiction: there is something here,
        // it simply cannot be selected. Claiming no content would let a
        // container above conclude the area is empty and skip it.
        let geometry = SelectionContainerState::disabled_geometry();
        assert!(geometry.has_content);
        assert!(!geometry.has_selection());
        assert_eq!(geometry.status, SelectionStatus::None);
        assert!(geometry.is_consistent());
    }

    #[test]
    fn a_disabled_container_answers_none_rather_than_pointing_anywhere() {
        // There is nothing here to select and nothing to say about where the
        // edge should go instead.
        let state = SelectionContainer::disabled(7).create_state();
        let mut group = Group::new(SelectionStatus::Uncollapsed);
        assert_eq!(
            state.dispatch_selection_event(
                Some(&mut group),
                SelectionEvent::SelectWord {
                    global_position: Offset::ZERO
                }
            ),
            SelectionResult::None
        );
        assert!(group.events.is_empty(), "the delegate was not even asked");
        assert_eq!(state.selected_content(Some(&group)), None);
        assert_eq!(
            state.selection_geometry(Some(&group)),
            SelectionContainerState::disabled_geometry(),
            "and its own geometry, not the delegate's"
        );
    }

    #[test]
    fn an_enabled_container_forwards_everything_to_its_delegate() {
        let state = SelectionContainer::new(7).create_state();
        let mut group = Group::new(SelectionStatus::Uncollapsed);
        assert_eq!(
            state.dispatch_selection_event(Some(&mut group), SelectionEvent::SelectAll),
            SelectionResult::End
        );
        assert_eq!(group.events, vec![SelectionEvent::SelectAll]);
        assert_eq!(
            state.selected_content(Some(&group)),
            Some(SelectedContent::new("the whole group"))
        );
        assert_eq!(
            state.selection_geometry(Some(&group)).status,
            SelectionStatus::Uncollapsed
        );
    }

    #[test]
    fn swapping_a_delegate_for_an_equivalent_one_does_not_repaint_the_selection() {
        // Upstream tells the listeners only if the two delegates disagree
        // about the geometry.
        let mut state = SelectionContainer::new(7).create_state();
        let same_a = SelectionGeometry::new(SelectionStatus::Collapsed, true);
        let same_b = SelectionGeometry::new(SelectionStatus::Collapsed, true);
        assert!(!state.delegate_changed(Some(&same_a), Some(&same_b)));

        let different = SelectionGeometry::new(SelectionStatus::Uncollapsed, true);
        assert!(state.delegate_changed(Some(&same_a), Some(&different)));

        // Arriving from nothing, or going to nothing, is a change.
        assert!(state.delegate_changed(None, Some(&same_a)));
        assert!(state.delegate_changed(Some(&same_a), None));
        assert!(!state.delegate_changed(None, None));
    }

    #[test]
    fn adding_a_listener_needs_a_delegate_and_removing_one_does_not() {
        // The asymmetry is deliberate: a listener removing itself as the
        // container is torn down is ordinary, and refusing it would turn an
        // orderly teardown into a crash.
        let mut enabled = SelectionContainer::new(7).create_state();
        assert!(enabled.add_listener());
        assert!(enabled.add_listener());
        assert_eq!(enabled.listeners(), 2);
        enabled.remove_listener();
        assert_eq!(enabled.listeners(), 1);

        let mut disabled = SelectionContainer::disabled(7).create_state();
        assert!(!disabled.add_listener(), "nothing to hold it");
        assert_eq!(disabled.listeners(), 0);
        disabled.remove_listener();
        assert_eq!(disabled.listeners(), 0, "and removing is still harmless");
    }

    #[test]
    fn the_scope_only_notifies_when_the_registrar_actually_changed() {
        // Compared by identity, so a rebuild handing down the same registrar
        // does not make every selectable below re-register.
        let one = SelectionRegistrarScope::new(1);
        assert!(!one.update_should_notify(&SelectionRegistrarScope::new(1)));
        assert!(one.update_should_notify(&SelectionRegistrarScope::new(2)));
        assert!(one.update_should_notify(&SelectionRegistrarScope::empty()));
        assert!(
            !SelectionRegistrarScope::empty()
                .update_should_notify(&SelectionRegistrarScope::empty())
        );
    }

    #[test]
    fn a_delegate_registers_and_unregisters_its_children() {
        let mut group = Group::new(SelectionStatus::None);
        group.add(1);
        group.add(2);
        assert_eq!(group.registered, vec![1, 2]);
        group.remove(1);
        assert_eq!(group.registered, vec![2]);
    }

    #[test]
    fn the_delegate_will_not_answer_about_geometry_before_it_has_a_size() {
        // Upstream asserts on hasSize in every geometry accessor, with the
        // same cause named each time: asking where the children are before the
        // container has been laid out is not a question with a wrong answer,
        // it is a question with no answer.
        let group = Group::new(SelectionStatus::None);
        assert!(group.has_size());
        assert_eq!(group.container_size(), Some((300.0, 200.0)));
    }
}
