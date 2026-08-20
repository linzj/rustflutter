//! A port of `widgets/autofill.dart`.
//!
//! Fields that belong to the same form, so the platform's autofill can fill
//! them together. A password manager offering to fill a username without the
//! password beside it would be no use at all.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use crate::framework::{AnyWidget, BuildContext, Component, component, provide};

/// Upstream `AutofillContextAction`: what to do with what the reader typed when
/// the group goes away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AutofillContextAction {
    /// Tell the platform to save it. What a submitted form wants.
    #[default]
    Commit,
    /// Throw it away. What an abandoned one wants.
    Cancel,
}

/// One field registered with a group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutofillClient {
    pub id: u64,
    /// A field can be registered and still not want filling.
    pub enabled: bool,
}

/// What disposing a group told the platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisposeOutcome {
    /// This group is inside another one, so it says nothing. The autofill
    /// context belongs to the outermost group, and finishing it from an inner
    /// one would save a half-filled form.
    NotTopmost,
    FinishedSaving,
    FinishedDiscarding,
}

/// Upstream `AutofillGroupState`.
#[derive(Clone, Debug, PartialEq)]
pub struct AutofillGroupState {
    pub on_dispose_action: AutofillContextAction,
    clients: BTreeMap<u64, AutofillClient>,
    is_topmost: bool,
}

impl AutofillGroupState {
    pub fn new(on_dispose_action: AutofillContextAction) -> AutofillGroupState {
        AutofillGroupState {
            on_dispose_action,
            clients: BTreeMap::new(),
            is_topmost: false,
        }
    }

    /// Upstream `didChangeDependencies`, which recomputes this every time the
    /// ancestors change -- so a group reparented under another one stops being
    /// topmost without anybody telling it.
    pub fn did_change_dependencies(&mut self, has_ancestor_group: bool) {
        self.is_topmost = !has_ancestor_group;
    }

    pub fn is_topmost(&self) -> bool {
        self.is_topmost
    }

    pub fn get_autofill_client(&self, id: u64) -> Option<AutofillClient> {
        self.clients.get(&id).copied()
    }

    /// Upstream's `autofillClients`, which **filters to the enabled ones**. A
    /// disabled field is still registered -- it is part of the form -- but it is
    /// not offered to the platform.
    pub fn autofill_clients(&self) -> Vec<AutofillClient> {
        self.clients
            .values()
            .copied()
            .filter(|client| client.enabled)
            .collect()
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Upstream `register`, which uses `putIfAbsent`: registering the same id
    /// twice keeps the first. A field that re-registers during a rebuild does
    /// not replace itself with a copy.
    pub fn register(&mut self, client: AutofillClient) {
        self.clients.entry(client.id).or_insert(client);
    }

    /// Upstream `unregister`, which **asserts the id is there**. Removing
    /// something that was never registered means the register and unregister
    /// calls have got out of step, and a silent no-op would hide it.
    pub fn unregister(&mut self, id: u64) -> Result<(), &'static str> {
        if self.clients.remove(&id).is_none() {
            return Err("unregistering an autofill client that was never registered");
        }
        Ok(())
    }

    /// Upstream `dispose`.
    ///
    /// Only the outermost group finishes the platform's autofill context. The
    /// context is one thing for the whole form, and a nested group disposing --
    /// a section of the form being rebuilt, say -- must not decide on the
    /// form's behalf whether what has been typed so far is worth saving.
    pub fn dispose(&self) -> DisposeOutcome {
        if !self.is_topmost {
            return DisposeOutcome::NotTopmost;
        }
        match self.on_dispose_action {
            AutofillContextAction::Commit => DisposeOutcome::FinishedSaving,
            AutofillContextAction::Cancel => DisposeOutcome::FinishedDiscarding,
        }
    }
}

// -- The widget ----------------------------------------------------------------

/// Upstream's private `_AutofillScope`: the inherited widget an [`AutofillGroup`]
/// publishes so the fields below it can find its state.
///
/// Upstream's `updateShouldNotify` is `_scope != old._scope` -- an **identity**
/// comparison of the state object, not a comparison of what it holds. That is
/// the right test and it is kept here as `Rc::ptr_eq`: fields depend on *which*
/// group they belong to, and a group whose registrations changed has not become
/// a different group. Comparing contents would rebuild every field in the form
/// each time one of them registered.
#[derive(Clone, Debug)]
pub struct AutofillScopeHandle(pub Rc<RefCell<AutofillGroupState>>);

impl PartialEq for AutofillScopeHandle {
    fn eq(&self, other: &AutofillScopeHandle) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Upstream `AutofillGroup`: the widget that makes a group of fields one form.
///
/// The state -- the registrations, the topmost question, what disposing does --
/// is [`AutofillGroupState`]. What the widget adds is the two things a caller
/// gives it, and the way a field inside finds it.
///
/// # Being found
///
/// A field does not name its group; it asks for the nearest one, and *depends*
/// on the answer, so a field reparented under a different group is rebuilt and
/// re-registers. That is [`AutofillScopeHandle`] published with
/// [`provide`](crate::framework::provide) and read with
/// [`AutofillGroup::maybe_of`].
pub struct AutofillGroup {
    child: RefCell<Option<AnyWidget>>,
    state: Rc<RefCell<AutofillGroupState>>,
}

impl AutofillGroup {
    /// Upstream's constructor. `onDisposeAction` defaults to
    /// [`AutofillContextAction::Commit`] -- a form that goes away is far more
    /// often submitted than abandoned, and being offered to save what was typed
    /// costs the reader nothing.
    pub fn new(child: AnyWidget) -> AutofillGroup {
        AutofillGroup {
            child: RefCell::new(Some(child)),
            state: Rc::new(RefCell::new(AutofillGroupState::new(
                AutofillContextAction::Commit,
            ))),
        }
    }

    pub fn with_on_dispose_action(self, action: AutofillContextAction) -> Self {
        self.state.borrow_mut().on_dispose_action = action;
        self
    }

    pub fn on_dispose_action(&self) -> AutofillContextAction {
        self.state.borrow().on_dispose_action
    }

    /// The state this group's fields register into -- upstream's
    /// `createState` result, which callers reach through [`AutofillGroup::of`].
    pub fn state(&self) -> Rc<RefCell<AutofillGroupState>> {
        Rc::clone(&self.state)
    }

    /// Upstream's `AutofillGroup.maybeOf`: the nearest enclosing group, or
    /// nothing.
    pub fn maybe_of(context: &mut BuildContext) -> Option<Rc<RefCell<AutofillGroupState>>> {
        context
            .inherited::<AutofillScopeHandle>()
            .map(|handle| Rc::clone(&handle.0))
    }

    /// Upstream's `AutofillGroup.of`, which asserts when there is none.
    ///
    /// The two spellings are kept apart for upstream's reason: a field that can
    /// work without a group asks [`maybe_of`](AutofillGroup::maybe_of), and one
    /// that cannot has found a bug rather than a configuration.
    pub fn of(context: &mut BuildContext) -> Rc<RefCell<AutofillGroupState>> {
        AutofillGroup::maybe_of(context).expect(
            "AutofillGroup::of() was called with a context that has no AutofillGroup ancestor.              Use AutofillGroup::maybe_of() if the caller can do without one.",
        )
    }

    /// Upstream's `dispose`, which runs on the state and only for the topmost
    /// group -- see [`AutofillGroupState::dispose`].
    pub fn dispose(&self) -> DisposeOutcome {
        self.state.borrow().dispose()
    }
}

impl Component for AutofillGroup {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        // Upstream's `didChangeDependencies`. The question is asked against the
        // *ancestors* of this build, which is what makes a group nested inside
        // another one not topmost -- and what stops a group from being its own
        // ancestor, since it has not published yet.
        let has_ancestor = AutofillGroup::maybe_of(context).is_some();
        self.state
            .borrow_mut()
            .did_change_dependencies(has_ancestor);

        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| crate::framework::leaf(|| crate::widgets::Empty));
        provide(AutofillScopeHandle(Rc::clone(&self.state)), child)
    }
}

/// [`AutofillGroup`] as a widget.
pub fn autofill_group(child: AnyWidget) -> AnyWidget {
    component(AutofillGroup::new(child))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(id: u64, enabled: bool) -> AutofillClient {
        AutofillClient { id, enabled }
    }

    fn topmost() -> AutofillGroupState {
        let mut group = AutofillGroupState::new(AutofillContextAction::Commit);
        group.did_change_dependencies(false);
        group
    }

    #[test]
    fn a_disabled_field_is_part_of_the_form_but_not_offered_to_the_platform() {
        let mut group = topmost();
        group.register(client(1, true));
        group.register(client(2, false));
        assert_eq!(group.client_count(), 2);
        assert_eq!(group.autofill_clients(), [client(1, true)]);
    }

    #[test]
    fn registering_the_same_field_twice_keeps_the_first() {
        // A field that re-registers during a rebuild does not replace itself
        // with a copy.
        let mut group = topmost();
        group.register(client(1, true));
        group.register(client(1, false));
        assert_eq!(group.get_autofill_client(1), Some(client(1, true)));
        assert_eq!(group.client_count(), 1);
    }

    #[test]
    fn unregistering_something_that_was_never_there_is_reported() {
        // It means the register and unregister calls got out of step, and a
        // silent no-op would hide that.
        let mut group = topmost();
        assert!(group.unregister(1).is_err());

        group.register(client(1, true));
        assert!(group.unregister(1).is_ok());
        assert_eq!(group.client_count(), 0);
    }

    #[test]
    fn only_the_outermost_group_finishes_the_autofill_context() {
        // The context is one thing for the whole form. A nested group being
        // rebuilt must not decide on the form's behalf whether what has been
        // typed so far is worth saving.
        let mut nested = AutofillGroupState::new(AutofillContextAction::Commit);
        nested.did_change_dependencies(true);
        assert!(!nested.is_topmost());
        assert_eq!(nested.dispose(), DisposeOutcome::NotTopmost);

        assert_eq!(topmost().dispose(), DisposeOutcome::FinishedSaving);
    }

    #[test]
    fn a_submitted_form_saves_and_an_abandoned_one_does_not() {
        let mut cancelling = AutofillGroupState::new(AutofillContextAction::Cancel);
        cancelling.did_change_dependencies(false);
        assert_eq!(cancelling.dispose(), DisposeOutcome::FinishedDiscarding);
    }

    #[test]
    fn a_group_reparented_under_another_stops_being_topmost_without_being_told() {
        // didChangeDependencies recomputes it every time the ancestors change.
        let mut group = topmost();
        assert!(group.is_topmost());
        group.did_change_dependencies(true);
        assert!(!group.is_topmost());
    }

    #[test]
    fn saving_is_the_default_because_a_form_that_was_filled_in_usually_matters() {
        assert_eq!(
            AutofillContextAction::default(),
            AutofillContextAction::Commit
        );
    }
}

#[cfg(test)]
mod group_widget_tests {
    use super::*;
    use crate::framework::{ElementTree, leaf};
    use crate::widgets::Empty;

    thread_local! {
        /// What each field found when it looked for its group, in build order.
        static FOUND: RefCell<Vec<Option<Rc<RefCell<AutofillGroupState>>>>> =
            const { RefCell::new(Vec::new()) };
    }

    /// A field, which registers with whatever group it finds.
    struct Field(u64);

    impl Component for Field {
        fn build(&self, context: &mut BuildContext) -> AnyWidget {
            let group = AutofillGroup::maybe_of(context);
            if let Some(group) = &group {
                group.borrow_mut().register(AutofillClient {
                    id: self.0,
                    enabled: true,
                });
            }
            FOUND.with(|found| found.borrow_mut().push(group));
            leaf(|| Empty)
        }
    }

    fn found() -> Vec<Option<Rc<RefCell<AutofillGroupState>>>> {
        FOUND.with(|found| found.borrow().clone())
    }

    fn reset() {
        FOUND.with(|found| found.borrow_mut().clear());
    }

    #[test]
    fn a_field_inside_a_group_finds_it() {
        reset();
        let group = AutofillGroup::new(component(Field(7)));
        let state = group.state();
        ElementTree::new().rebuild(component(group));

        assert_eq!(found().len(), 1);
        assert!(
            Rc::ptr_eq(found()[0].as_ref().unwrap(), &state),
            "and it is this group's state, not a copy"
        );
        assert_eq!(state.borrow().client_count(), 1, "so it registered");
    }

    #[test]
    fn a_field_with_no_group_above_it_finds_nothing() {
        // And is not an error: a lone text field still works, it just autofills
        // on its own rather than as part of a form.
        reset();
        ElementTree::new().rebuild(component(Field(7)));
        assert_eq!(found(), vec![None]);
    }

    #[test]
    fn a_field_finds_the_nearest_group_and_not_the_outer_one() {
        reset();
        let inner = AutofillGroup::new(component(Field(7)));
        let inner_state = inner.state();
        let outer = AutofillGroup::new(component(inner));
        let outer_state = outer.state();
        ElementTree::new().rebuild(component(outer));

        let seen = found()[0].clone().unwrap();
        assert!(Rc::ptr_eq(&seen, &inner_state));
        assert_eq!(inner_state.borrow().client_count(), 1);
        assert_eq!(
            outer_state.borrow().client_count(),
            0,
            "the outer group never heard of it"
        );
    }

    #[test]
    fn only_the_outer_group_is_topmost() {
        // Which is the whole reason the widget asks before it publishes: a group
        // that could see itself would find an ancestor and none would be
        // topmost, so nothing would ever finish the autofill context.
        let inner = AutofillGroup::new(leaf(|| Empty));
        let inner_state = inner.state();
        let outer = AutofillGroup::new(component(inner));
        let outer_state = outer.state();
        ElementTree::new().rebuild(component(outer));

        assert!(outer_state.borrow().is_topmost());
        assert!(!inner_state.borrow().is_topmost());
        assert_eq!(
            inner_state.borrow().dispose(),
            DisposeOutcome::NotTopmost,
            "so the inner one saves nothing on its own"
        );
        assert_eq!(
            outer_state.borrow().dispose(),
            DisposeOutcome::FinishedSaving
        );
    }

    #[test]
    fn a_group_with_nothing_above_it_is_topmost() {
        let group = AutofillGroup::new(leaf(|| Empty));
        let state = group.state();
        ElementTree::new().rebuild(component(group));
        assert!(state.borrow().is_topmost());
    }

    #[test]
    fn the_dispose_action_reaches_the_state() {
        // The widget holds no copy of its own -- there is one answer, and it
        // lives where `dispose` reads it.
        let group = AutofillGroup::new(leaf(|| Empty))
            .with_on_dispose_action(AutofillContextAction::Cancel);
        assert_eq!(group.on_dispose_action(), AutofillContextAction::Cancel);
        assert_eq!(
            group.state().borrow().on_dispose_action,
            AutofillContextAction::Cancel
        );

        let state = group.state();
        ElementTree::new().rebuild(component(group));
        assert_eq!(state.borrow().dispose(), DisposeOutcome::FinishedDiscarding);
    }

    #[test]
    fn a_group_is_the_same_group_after_a_field_registers() {
        // Upstream compares the scope by identity, so publishing again after a
        // registration is not a change and the fields below are left alone.
        let state = Rc::new(RefCell::new(AutofillGroupState::new(
            AutofillContextAction::Commit,
        )));
        let before = AutofillScopeHandle(Rc::clone(&state));
        state.borrow_mut().register(AutofillClient {
            id: 1,
            enabled: true,
        });
        let after = AutofillScopeHandle(Rc::clone(&state));
        assert_eq!(before, after, "same group, changed contents");

        // The case that separates identity from contents: a second group
        // holding exactly what the first holds is still a different group, and
        // a field that moved between them must be rebuilt.
        let twin_state = Rc::new(RefCell::new(AutofillGroupState::new(
            AutofillContextAction::Commit,
        )));
        twin_state.borrow_mut().register(AutofillClient {
            id: 1,
            enabled: true,
        });
        assert_eq!(
            *twin_state.borrow(),
            *state.borrow(),
            "the same contents, down to the registrations"
        );
        assert_ne!(
            before,
            AutofillScopeHandle(twin_state),
            "and still not the same group"
        );
    }

    #[test]
    fn two_fields_in_one_group_are_one_form() {
        // Which is what a group is for: a password manager offering a username
        // without the password beside it is no use.
        reset();
        let group = AutofillGroup::new(crate::framework::many(
            vec![component(Field(1)), component(Field(2))],
            |_children| crate::render::RenderFlex::new(crate::render::Axis::Vertical),
        ));
        let state = group.state();
        ElementTree::new().rebuild(component(group));

        assert_eq!(state.borrow().client_count(), 2);
        let seen = found();
        assert!(Rc::ptr_eq(
            seen[0].as_ref().unwrap(),
            seen[1].as_ref().unwrap()
        ));
    }
}
