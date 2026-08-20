//! A port of `widgets/automatic_keep_alive.dart`.
//!
//! A lazy list discards the children that scroll out of it -- that is what
//! makes it lazy. This file is the exception mechanism: a widget deep inside a
//! row can say "not me", and keep its state while it is off screen. A focused
//! text field is the example upstream gives; a half-played video is another.
//!
//! Until now this crate simply dropped anything that left the viewport, and
//! `coverage_ledger.json` recorded that as a divergence. It no longer is.

use crate::layout_builder::SchedulerPhaseForRebuild;
use std::collections::BTreeMap;

/// Upstream `KeepAliveHandle`, a `ChangeNotifier` that is only ever triggered
/// once.
///
/// Its whole body upstream is an override of `dispose` that calls
/// `notifyListeners()` *before* `super.dispose()`. Notifying from a dispose is
/// normally exactly the thing not to do -- but here disposal **is** the
/// message. The handle exists to carry one announcement, "I no longer need to
/// be kept alive", and destroying it is how that announcement is made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeepAliveHandle {
    id: u64,
    listeners: Vec<u64>,
    notified: Vec<u64>,
    disposed: bool,
}

impl KeepAliveHandle {
    pub fn new(id: u64) -> KeepAliveHandle {
        KeepAliveHandle {
            id,
            listeners: Vec::new(),
            notified: Vec::new(),
            disposed: false,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn add_listener(&mut self, listener: u64) {
        assert!(
            !self.disposed,
            "a disposed keep-alive handle can no longer be used"
        );
        self.listeners.push(listener);
    }

    pub fn remove_listener(&mut self, listener: u64) {
        if let Some(index) = self.listeners.iter().position(|id| *id == listener) {
            self.listeners.remove(index);
        }
    }

    pub fn listener_count(&self) -> usize {
        self.listeners.len()
    }

    /// Who has been told, in the order they were told.
    pub fn notified(&self) -> &[u64] {
        &self.notified
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed
    }

    /// Upstream's `dispose`: notify, then dispose.
    pub fn dispose(&mut self) {
        assert!(
            !self.disposed,
            "a keep-alive handle is triggered exactly once"
        );
        self.notified.extend(self.listeners.iter().copied());
        self.disposed = true;
    }
}

/// Upstream `KeepAliveNotification`.
///
/// A notification with one field, and the field is a `Listenable` rather than a
/// flag -- because the interesting message is not "keep me" but "you may stop".
/// The notification says the first; the handle it carries is how the second
/// arrives later, from a widget that may by then be gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeepAliveNotification {
    /// The handle whose triggering means the subtree may be released.
    pub handle: u64,
}

impl KeepAliveNotification {
    pub fn new(handle: u64) -> KeepAliveNotification {
        KeepAliveNotification { handle }
    }
}

/// What the host did with a request to stop keeping a subtree alive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseOutcome {
    /// Other clients still want the subtree, so nothing changed.
    StillWanted,
    /// The last client let go and the parent data was updated for this frame.
    ReleasedNow,
    /// The last client let go too late in the frame to act on. Upstream's own
    /// comment calls this "very unfortunate", and names the cost: the subtree
    /// is not collected for another 16ms.
    ReleasedNextFrame,
}

/// What the host did with a new request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RetainOutcome {
    /// The subtree was already being kept alive for somebody else.
    AlreadyKept,
    /// Parent data was applied out of turn, synchronously.
    AppliedOutOfTurn,
    /// This was the subtree's very first build, so the child does not exist to
    /// apply parent data to yet; upstream waits for the end of the frame.
    DeferredToEndOfFrame,
}

/// Upstream `AutomaticKeepAlive`, and the handle bookkeeping of its state.
///
/// It listens for `KeepAliveNotification`s from anywhere below it and, while at
/// least one is outstanding, wraps its child in a `KeepAlive` so the sliver
/// above will not throw the row away.
///
/// The asymmetry between the two halves is the shape of the class. **Starting**
/// to keep a subtree alive is applied out of turn, synchronously, mid-build if
/// need be -- because the alternative is that the row is discarded before the
/// request lands. **Stopping** cannot be: it needs a rebuild, and if the frame
/// has already gone past build there is nothing to do but wait for the next
/// one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AutomaticKeepAlive {
    /// The handles currently outstanding, and whether each has a listener
    /// registered. A `BTreeMap` so the order is the handles' own.
    handles: BTreeMap<u64, bool>,
    keeping_alive: bool,
    child_built: bool,
    mounted: bool,
    deferred_release: bool,
}

impl AutomaticKeepAlive {
    pub fn new() -> AutomaticKeepAlive {
        AutomaticKeepAlive {
            handles: BTreeMap::new(),
            keeping_alive: false,
            child_built: false,
            mounted: true,
            deferred_release: false,
        }
    }

    /// Whether the child subtree has finished its first build. Until it has,
    /// there is no element to apply parent data to.
    pub fn with_built_child(mut self) -> Self {
        self.child_built = true;
        self
    }

    pub fn is_keeping_alive(&self) -> bool {
        self.keeping_alive
    }

    pub fn client_count(&self) -> usize {
        self.handles.len()
    }

    /// Upstream `_addClient`.
    pub fn add_client(&mut self, notification: KeepAliveNotification) -> RetainOutcome {
        assert!(
            !self.handles.contains_key(&notification.handle),
            "the same handle was used for two keep-alive notifications without \
             being triggered in between"
        );
        self.handles.insert(notification.handle, true);
        if self.keeping_alive {
            return RetainOutcome::AlreadyKept;
        }
        self.keeping_alive = true;
        if self.child_built {
            RetainOutcome::AppliedOutOfTurn
        } else {
            RetainOutcome::DeferredToEndOfFrame
        }
    }

    /// The end-of-frame callback for the deferred case. Upstream's first act is
    /// to check it is still mounted: a frame passed, and the row may have been
    /// scrolled away in it.
    pub fn apply_deferred_parent_data(&mut self) -> bool {
        if !self.mounted {
            return false;
        }
        self.child_built = true;
        true
    }

    /// Upstream's per-handle listener, fired when the client triggers it.
    pub fn release_client(
        &mut self,
        handle: u64,
        phase: SchedulerPhaseForRebuild,
    ) -> ReleaseOutcome {
        assert!(
            self.mounted,
            "a keep-alive handle was triggered after the AutomaticKeepAlive was disposed; \
             widgets must trigger their handle when they are deactivated"
        );
        self.handles.remove(&handle);
        if !self.handles.is_empty() {
            return ReleaseOutcome::StillWanted;
        }
        // Upstream compares against `SchedulerPhase.persistentCallbacks`: build
        // and layout have not started, so a setState still lands this frame.
        if (phase as u8) < (SchedulerPhaseForRebuild::PersistentCallbacks as u8) {
            self.keeping_alive = false;
            self.deferred_release = false;
            ReleaseOutcome::ReleasedNow
        } else {
            self.deferred_release = true;
            ReleaseOutcome::ReleasedNextFrame
        }
    }

    /// Runs the rebuild a deferred release asked for.
    pub fn flush_deferred_release(&mut self) -> bool {
        if !self.deferred_release {
            return false;
        }
        self.deferred_release = false;
        if self.handles.is_empty() {
            self.keeping_alive = false;
        }
        true
    }

    pub fn has_deferred_release(&self) -> bool {
        self.deferred_release
    }

    /// Upstream `dispose`, which unhooks every listener it registered.
    pub fn dispose(&mut self) {
        self.handles.clear();
        self.mounted = false;
    }

    pub fn is_mounted(&self) -> bool {
        self.mounted
    }
}

/// The widget upstream's mixin returns from `build`.
///
/// Its `build` throws. That is the whole class: the mixin's `build` must be
/// *called* and its result *ignored*, and there is no way to check the second
/// half except by making the value poisonous.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NullWidget;

impl NullWidget {
    pub fn build(&self) -> ! {
        panic!(
            "Widgets that mix AutomaticKeepAliveClientMixin into their State must \
             call super.build() but must ignore the return value of the superclass."
        )
    }
}

/// Upstream `AutomaticKeepAliveClientMixin`.
///
/// The client's side of the protocol, and it is smaller than it looks: hold a
/// handle while you want to be kept, drop it when you do not, and dispatch a
/// fresh notification whenever you find yourself without one.
#[derive(Clone, Debug, PartialEq)]
pub struct AutomaticKeepAliveClientMixin {
    /// Upstream's abstract `wantKeepAlive` getter.
    pub want_keep_alive: bool,
    keep_alive_handle: Option<KeepAliveHandle>,
    next_handle_id: u64,
    dispatched: Vec<KeepAliveNotification>,
}

impl AutomaticKeepAliveClientMixin {
    pub fn new(want_keep_alive: bool) -> AutomaticKeepAliveClientMixin {
        AutomaticKeepAliveClientMixin {
            want_keep_alive,
            keep_alive_handle: None,
            next_handle_id: 1,
            dispatched: Vec::new(),
        }
    }

    /// The notifications this client has sent, oldest first.
    pub fn dispatched(&self) -> &[KeepAliveNotification] {
        &self.dispatched
    }

    pub fn handle(&self) -> Option<&KeepAliveHandle> {
        self.keep_alive_handle.as_ref()
    }

    pub fn is_holding_a_handle(&self) -> bool {
        self.keep_alive_handle.is_some()
    }

    fn ensure_keep_alive(&mut self) {
        assert!(
            self.keep_alive_handle.is_none(),
            "a client holds at most one handle at a time"
        );
        let handle = KeepAliveHandle::new(self.next_handle_id);
        self.next_handle_id += 1;
        self.dispatched
            .push(KeepAliveNotification::new(handle.id()));
        self.keep_alive_handle = Some(handle);
    }

    fn release_keep_alive(&mut self) -> Option<KeepAliveHandle> {
        let mut handle = self.keep_alive_handle.take()?;
        handle.dispose();
        Some(handle)
    }

    /// Upstream `initState`.
    pub fn init_state(&mut self) {
        if self.want_keep_alive {
            self.ensure_keep_alive();
        }
    }

    /// Upstream `updateKeepAlive`: the only way to *stop* being kept alive.
    /// A build will start a keep-alive but never end one, so a subclass whose
    /// `wantKeepAlive` went false without calling this stays kept.
    pub fn update_keep_alive(&mut self) -> Option<KeepAliveHandle> {
        if self.want_keep_alive {
            if self.keep_alive_handle.is_none() {
                self.ensure_keep_alive();
            }
            None
        } else {
            self.release_keep_alive()
        }
    }

    /// Upstream `deactivate`. The handle is released on the way out **even
    /// though the widget may still want to be kept alive**, and `build` puts it
    /// back on the way in. The invariant is re-asserted every build rather than
    /// carried across one, which is what keeps a host from holding a handle
    /// belonging to a subtree that has moved somewhere else.
    pub fn deactivate(&mut self) -> Option<KeepAliveHandle> {
        self.release_keep_alive()
    }

    /// Upstream `build`, which subclasses must call and must ignore.
    pub fn build(&mut self) -> NullWidget {
        if self.want_keep_alive && self.keep_alive_handle.is_none() {
            self.ensure_keep_alive();
        }
        NullWidget
    }

    /// Sets `wantKeepAlive` without telling anyone -- which is upstream's
    /// situation whenever a subclass changes it and forgets `updateKeepAlive`.
    pub fn set_want_keep_alive_quietly(&mut self, want: bool) {
        self.want_keep_alive = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use SchedulerPhaseForRebuild::{
        Idle, MidFrameMicrotasks, PersistentCallbacks, PostFrameCallbacks, TransientCallbacks,
    };

    // -- The handle -----------------------------------------------------------

    #[test]
    fn destroying_the_handle_is_how_the_message_is_sent() {
        // Notifying from a dispose is normally the thing not to do. Here
        // disposal *is* the announcement.
        let mut handle = KeepAliveHandle::new(1);
        handle.add_listener(7);
        assert!(handle.notified().is_empty());

        handle.dispose();
        assert_eq!(handle.notified(), [7], "told before it went");
        assert!(handle.is_disposed());
    }

    #[test]
    #[should_panic(expected = "triggered exactly once")]
    fn a_handle_carries_exactly_one_announcement() {
        let mut handle = KeepAliveHandle::new(1);
        handle.dispose();
        handle.dispose();
    }

    #[test]
    #[should_panic(expected = "a disposed keep-alive handle")]
    fn a_spent_handle_says_so_rather_than_listening_quietly() {
        let mut handle = KeepAliveHandle::new(1);
        handle.dispose();
        handle.add_listener(7);
    }

    #[test]
    fn a_removed_listener_is_not_told() {
        let mut handle = KeepAliveHandle::new(1);
        handle.add_listener(7);
        handle.add_listener(8);
        handle.remove_listener(7);
        handle.dispose();
        assert_eq!(handle.notified(), [8]);
    }

    // -- The client -----------------------------------------------------------

    #[test]
    fn a_client_that_wants_nothing_announces_nothing() {
        let mut client = AutomaticKeepAliveClientMixin::new(false);
        client.init_state();
        assert!(!client.is_holding_a_handle());
        assert!(client.dispatched().is_empty());
    }

    #[test]
    fn a_client_that_wants_keeping_says_so_before_its_first_build() {
        // Because the row it is in may be discarded before that build happens.
        let mut client = AutomaticKeepAliveClientMixin::new(true);
        client.init_state();
        assert!(client.is_holding_a_handle());
        assert_eq!(client.dispatched(), [KeepAliveNotification::new(1)]);
    }

    #[test]
    fn a_build_re_establishes_a_keep_alive_but_never_ends_one() {
        // Which is why upstream tells subclasses to call updateKeepAlive: a
        // wantKeepAlive that quietly went false stays kept until somebody says
        // so out loud.
        let mut client = AutomaticKeepAliveClientMixin::new(true);
        client.init_state();

        client.set_want_keep_alive_quietly(false);
        client.build();
        assert!(
            client.is_holding_a_handle(),
            "the build did not notice, and could not"
        );

        assert!(client.update_keep_alive().is_some(), "this is what notices");
        assert!(!client.is_holding_a_handle());
    }

    #[test]
    fn a_build_picks_up_a_keep_alive_that_appeared_since_the_last_one() {
        let mut client = AutomaticKeepAliveClientMixin::new(false);
        client.init_state();
        client.set_want_keep_alive_quietly(true);
        client.build();
        assert!(client.is_holding_a_handle());
        assert_eq!(client.dispatched().len(), 1);
    }

    #[test]
    fn a_build_while_already_kept_sends_nothing_new() {
        let mut client = AutomaticKeepAliveClientMixin::new(true);
        client.init_state();
        client.build();
        client.build();
        assert_eq!(client.dispatched().len(), 1);
    }

    #[test]
    fn leaving_the_tree_releases_the_handle_even_while_still_wanted() {
        // And the next build puts it back. The invariant is re-asserted every
        // build rather than carried across one -- otherwise a host would keep
        // holding a handle for a subtree that has moved somewhere else.
        let mut client = AutomaticKeepAliveClientMixin::new(true);
        client.init_state();

        let released = client.deactivate().expect("released on the way out");
        assert!(released.is_disposed());
        assert!(!client.is_holding_a_handle());
        assert!(client.want_keep_alive, "it still wants to be kept");

        client.build();
        assert!(client.is_holding_a_handle(), "and says so again");
    }

    #[test]
    fn each_re_registration_carries_a_fresh_handle() {
        // A spent handle cannot be reused, so the second notification must name
        // a different one.
        let mut client = AutomaticKeepAliveClientMixin::new(true);
        client.init_state();
        client.deactivate();
        client.build();
        assert_eq!(
            client.dispatched(),
            [KeepAliveNotification::new(1), KeepAliveNotification::new(2)]
        );
    }

    #[test]
    fn deactivating_twice_releases_once() {
        let mut client = AutomaticKeepAliveClientMixin::new(true);
        client.init_state();
        assert!(client.deactivate().is_some());
        assert!(client.deactivate().is_none());
    }

    #[test]
    #[should_panic(expected = "must ignore the return value")]
    fn the_value_of_super_build_is_poison_on_purpose() {
        // There is no way to check "you called it and ignored it" except by
        // making what it returns useless.
        let mut client = AutomaticKeepAliveClientMixin::new(false);
        client.build().build();
    }

    // -- The host --------------------------------------------------------------

    #[test]
    fn starting_to_keep_a_subtree_is_applied_out_of_turn() {
        // Synchronously, mid-build if need be, because the alternative is that
        // the row is discarded before the request lands.
        let mut host = AutomaticKeepAlive::new().with_built_child();
        assert_eq!(
            host.add_client(KeepAliveNotification::new(1)),
            RetainOutcome::AppliedOutOfTurn
        );
        assert!(host.is_keeping_alive());
    }

    #[test]
    fn a_request_during_the_subtrees_very_first_build_waits_for_the_frame_to_end() {
        // There is no child element to apply parent data to yet.
        let mut host = AutomaticKeepAlive::new();
        assert_eq!(
            host.add_client(KeepAliveNotification::new(1)),
            RetainOutcome::DeferredToEndOfFrame
        );
        assert!(host.is_keeping_alive(), "recorded immediately regardless");
        assert!(host.apply_deferred_parent_data());
    }

    #[test]
    fn a_deferred_application_checks_the_host_is_still_there() {
        // A frame passed, and the row may have been scrolled away in it.
        let mut host = AutomaticKeepAlive::new();
        host.add_client(KeepAliveNotification::new(1));
        host.dispose();
        assert!(!host.apply_deferred_parent_data());
    }

    #[test]
    fn a_second_client_changes_nothing() {
        let mut host = AutomaticKeepAlive::new().with_built_child();
        host.add_client(KeepAliveNotification::new(1));
        assert_eq!(
            host.add_client(KeepAliveNotification::new(2)),
            RetainOutcome::AlreadyKept
        );
        assert_eq!(host.client_count(), 2);
    }

    #[test]
    #[should_panic(expected = "two keep-alive notifications")]
    fn the_same_handle_used_twice_is_refused() {
        let mut host = AutomaticKeepAlive::new().with_built_child();
        host.add_client(KeepAliveNotification::new(1));
        host.add_client(KeepAliveNotification::new(1));
    }

    #[test]
    fn the_subtree_stays_while_any_client_still_wants_it() {
        let mut host = AutomaticKeepAlive::new().with_built_child();
        host.add_client(KeepAliveNotification::new(1));
        host.add_client(KeepAliveNotification::new(2));
        assert_eq!(host.release_client(1, Idle), ReleaseOutcome::StillWanted);
        assert!(host.is_keeping_alive());
        assert_eq!(host.release_client(2, Idle), ReleaseOutcome::ReleasedNow);
        assert!(!host.is_keeping_alive());
    }

    #[test]
    fn letting_go_before_build_lands_this_frame_and_after_it_costs_one() {
        // The asymmetry is the shape of the class: retaining can be applied out
        // of turn, releasing needs a rebuild. Upstream calls the late case
        // "very unfortunate" and names the price -- another 16ms.
        for phase in [Idle, TransientCallbacks, MidFrameMicrotasks] {
            let mut host = AutomaticKeepAlive::new().with_built_child();
            host.add_client(KeepAliveNotification::new(1));
            assert_eq!(
                host.release_client(1, phase),
                ReleaseOutcome::ReleasedNow,
                "{phase:?}"
            );
            assert!(!host.is_keeping_alive());
        }

        for phase in [PersistentCallbacks, PostFrameCallbacks] {
            let mut host = AutomaticKeepAlive::new().with_built_child();
            host.add_client(KeepAliveNotification::new(1));
            assert_eq!(
                host.release_client(1, phase),
                ReleaseOutcome::ReleasedNextFrame,
                "{phase:?}"
            );
            assert!(
                host.is_keeping_alive(),
                "still held for one more frame, {phase:?}"
            );
            assert!(host.flush_deferred_release());
            assert!(!host.is_keeping_alive());
        }
    }

    #[test]
    fn a_client_arriving_during_the_deferred_frame_keeps_the_subtree() {
        // The cleanup rebuild only lets go if nobody asked again in between.
        let mut host = AutomaticKeepAlive::new().with_built_child();
        host.add_client(KeepAliveNotification::new(1));
        host.release_client(1, PersistentCallbacks);
        host.add_client(KeepAliveNotification::new(2));

        assert!(host.flush_deferred_release());
        assert!(host.is_keeping_alive(), "somebody still wants it");
    }

    #[test]
    fn there_is_nothing_to_flush_without_a_deferred_release() {
        let mut host = AutomaticKeepAlive::new().with_built_child();
        host.add_client(KeepAliveNotification::new(1));
        host.release_client(1, Idle);
        assert!(!host.has_deferred_release());
        assert!(!host.flush_deferred_release());
    }

    #[test]
    #[should_panic(expected = "after the AutomaticKeepAlive was disposed")]
    fn a_handle_triggered_after_the_host_is_gone_says_so_loudly() {
        // Which means a widget somewhere forgot to trigger its handle on the
        // way out, and the message names that as the cause.
        let mut host = AutomaticKeepAlive::new().with_built_child();
        host.add_client(KeepAliveNotification::new(1));
        host.dispose();
        host.release_client(1, Idle);
    }

    // -- The two halves together -------------------------------------------------

    #[test]
    fn a_client_and_its_host_agree_across_a_scroll_and_back() {
        let mut host = AutomaticKeepAlive::new().with_built_child();
        let mut client = AutomaticKeepAliveClientMixin::new(true);

        client.init_state();
        let first = client.dispatched()[0];
        host.add_client(first);
        assert!(host.is_keeping_alive());

        // Scrolled away: the client is deactivated and triggers its handle.
        let released = client.deactivate().unwrap();
        assert_eq!(
            host.release_client(released.id(), Idle),
            ReleaseOutcome::ReleasedNow
        );
        assert!(!host.is_keeping_alive());

        // Scrolled back: a fresh handle, and the host takes it without
        // complaining about the old one.
        client.build();
        let second = *client.dispatched().last().unwrap();
        assert_ne!(second, first);
        assert_eq!(host.add_client(second), RetainOutcome::AppliedOutOfTurn);
        assert!(host.is_keeping_alive());
    }
}
