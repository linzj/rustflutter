//! A port of `widgets/autofill.dart`'s `AutofillGroupState`.
//!
//! Fields that belong to the same form, so the platform's autofill can fill
//! them together. A password manager offering to fill a username without the
//! password beside it would be no use at all.

use std::collections::BTreeMap;

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
