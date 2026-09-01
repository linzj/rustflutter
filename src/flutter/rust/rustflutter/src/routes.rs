//! Routes, their transitions and their local history -- a port of upstream's
//! `widgets/routes.dart`.
//!
//! Two things in here carry most of the judgement.
//!
//! The first is **who answers a back press**. Upstream asks in a chain, and
//! each link overrides the one below it: a `PopScope` that says no wins over
//! everything; failing that, a route's own local history absorbs the press;
//! failing that, the route pops, unless it is the first one, in which case the
//! press goes back to the platform and the application closes.
//!
//! The second is **local history**: a route can carry its own stack of things
//! a back press should undo before the route itself goes -- a bottom sheet, a
//! drawer, a step in a mid-flow form. Each back press peels one off. Without
//! it the reader's first back press would close the whole page.
//!
//! ## What is not here
//!
//! Every one of these is a `Route` upstream, driven by a `Navigator`, which
//! this crate does not have; and their transitions are driven by an
//! `AnimationController` this crate does not hand them. What is ported is the
//! state each route keeps and the decisions it makes from it -- who handles a
//! pop, when the overlay entries come out, when a barrier is dismissible, and
//! what an observer tells its subscribers.

use crate::engine::Color;

/// Upstream `RoutePopDisposition`: what should happen to a pop request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RoutePopDisposition {
    /// Pop the route.
    #[default]
    Pop,
    /// Do not pop -- something in the route asked to stay.
    DoNotPop,
    /// This route does not want the pop, so the platform should have it back.
    /// On the first route that is what closes the application.
    Bubble,
}

/// The base `Route::popDisposition` the rest of this file overrides.
///
/// `is_first` is the whole of it: the bottom of the stack has nowhere to pop
/// to, so it hands the press back rather than swallowing it. `page_can_pop` is
/// the page-based check upstream does before that -- a declarative page can
/// refuse without any of the machinery below.
pub fn route_pop_disposition(is_first: bool, page_can_pop: bool) -> RoutePopDisposition {
    if !page_can_pop {
        return RoutePopDisposition::DoNotPop;
    }
    if is_first {
        RoutePopDisposition::Bubble
    } else {
        RoutePopDisposition::Pop
    }
}

/// Upstream `LocalHistoryEntry`: one thing a back press should undo before the
/// route itself goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalHistoryEntry {
    pub id: u64,
    /// Upstream's `impliesAppBarDismissal`, **true by default**.
    ///
    /// It decides whether the app bar shows a back arrow while this entry
    /// exists. A drawer wants it; a route that added an entry only to intercept
    /// the back gesture does not, because an arrow that undoes something
    /// invisible reads as a broken arrow.
    pub implies_app_bar_dismissal: bool,
    /// Whether the entry is currently owned by a route. Upstream keeps
    /// `_owner`, and asserts on it in both directions.
    owned: bool,
    /// Whether upstream's `onRemove` has run.
    removed: bool,
}

impl LocalHistoryEntry {
    pub fn new(id: u64) -> LocalHistoryEntry {
        LocalHistoryEntry {
            id,
            implies_app_bar_dismissal: true,
            owned: false,
            removed: false,
        }
    }

    pub fn with_implies_app_bar_dismissal(mut self, implies: bool) -> Self {
        self.implies_app_bar_dismissal = implies;
        self
    }

    pub fn is_owned(&self) -> bool {
        self.owned
    }

    pub fn was_removed(&self) -> bool {
        self.removed
    }
}

/// Upstream `LocalHistoryRoute`: a route with a stack of its own.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LocalHistoryRoute {
    entries: Vec<LocalHistoryEntry>,
    /// Upstream's `_entriesImpliesAppBarDismissal`, counted separately from
    /// the entries themselves because not every entry votes.
    entries_implying_dismissal: usize,
    /// How many times upstream would have called `changedInternalState`.
    state_changes: usize,
    /// State changes upstream deferred to a post-frame callback because the
    /// tree was locked. See [`LocalHistoryRoute::remove_local_history_entry`].
    deferred_state_changes: usize,
}

impl LocalHistoryRoute {
    pub fn new() -> LocalHistoryRoute {
        LocalHistoryRoute::default()
    }

    pub fn entries(&self) -> &[LocalHistoryEntry] {
        &self.entries
    }

    pub fn state_changes(&self) -> usize {
        self.state_changes
    }

    pub fn deferred_state_changes(&self) -> usize {
        self.deferred_state_changes
    }

    /// Upstream's `willHandlePopInternally`.
    pub fn will_handle_pop_internally(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Whether the app bar should show a back arrow because of local history.
    pub fn implies_app_bar_dismissal(&self) -> bool {
        self.entries_implying_dismissal > 0
    }

    /// Upstream's `addLocalHistoryEntry`.
    ///
    /// The state change fires on the **edges only**: when the stack goes from
    /// empty to not, or when the dismissal count goes from zero to one. Adding
    /// a second entry changes nothing anyone can see, and rebuilding for it
    /// would be a rebuild per bottom sheet.
    pub fn add_local_history_entry(&mut self, mut entry: LocalHistoryEntry) {
        debug_assert!(!entry.owned, "an entry belongs to one route at a time");
        entry.owned = true;
        let was_empty = self.entries.is_empty();
        let mut dismissal_changed = false;
        if entry.implies_app_bar_dismissal {
            dismissal_changed = self.entries_implying_dismissal == 0;
            self.entries_implying_dismissal += 1;
        }
        self.entries.push(entry);
        if was_empty || dismissal_changed {
            self.state_changes += 1;
        }
    }

    /// Upstream's `removeLocalHistoryEntry`, which removes a **named** entry
    /// rather than the top one -- a drawer closing while a sheet is open takes
    /// its own entry out of the middle.
    ///
    /// `tree_is_locked` is upstream's `SchedulerPhase.persistentCallbacks`
    /// check. An entry removed during a build cannot mark the route dirty,
    /// because the tree is being finalized and nothing may be marked; upstream
    /// defers the notification to a post-frame callback, **guarded by
    /// `isActive`** -- by the time that callback runs the route may already be
    /// gone, and telling a dead route to rebuild is worse than not telling it.
    pub fn remove_local_history_entry(
        &mut self,
        id: u64,
        tree_is_locked: bool,
    ) -> Option<LocalHistoryEntry> {
        let at = self.entries.iter().position(|entry| entry.id == id)?;
        let mut entry = self.entries.remove(at);
        let mut dismissal_changed = false;
        if entry.implies_app_bar_dismissal {
            self.entries_implying_dismissal -= 1;
            dismissal_changed = self.entries_implying_dismissal == 0;
        }
        entry.owned = false;
        entry.removed = true;
        if self.entries.is_empty() || dismissal_changed {
            if tree_is_locked {
                self.deferred_state_changes += 1;
            } else {
                self.state_changes += 1;
            }
        }
        Some(entry)
    }

    /// Runs whatever [`LocalHistoryRoute::remove_local_history_entry`]
    /// deferred, if the route is still active.
    pub fn flush_deferred_state_changes(&mut self, is_active: bool) {
        if is_active {
            self.state_changes += self.deferred_state_changes;
        }
        self.deferred_state_changes = 0;
    }

    /// Upstream's `didPop`.
    ///
    /// **Returns false when it consumed the pop**, which reads backwards until
    /// the caller is named: the answer is "did the *route* pop", and it did
    /// not -- one of its local entries did. So a back press peels one layer at
    /// a time and the page survives until the last of them is gone.
    pub fn did_pop(&mut self) -> bool {
        let Some(mut entry) = self.entries.pop() else {
            return true;
        };
        entry.owned = false;
        entry.removed = true;
        if entry.implies_app_bar_dismissal {
            self.entries_implying_dismissal -= 1;
        }
        self.state_changes += 1;
        false
    }

    /// Upstream's `popDisposition`, which answers `Pop` outright while there
    /// is local history.
    ///
    /// Note what that overrides: a **first** route, whose base disposition is
    /// `Bubble`, answers `Pop` instead while it holds local entries. That is
    /// the point -- a back press on the application's only page closes the
    /// sheet on it rather than the application.
    pub fn pop_disposition(&self, is_first: bool, page_can_pop: bool) -> RoutePopDisposition {
        if self.will_handle_pop_internally() {
            return RoutePopDisposition::Pop;
        }
        route_pop_disposition(is_first, page_can_pop)
    }
}

/// Upstream `PopEntry`: something in the subtree that wants a say in whether
/// the route pops.
///
/// `canPop` is a **standing** answer held in a listenable rather than a
/// callback asked at pop time, and that is not laziness: the platform's
/// predictive back gesture needs to know *before* the reader starts swiping
/// whether the page will leave, so it can animate the page behind it. A
/// callback could only answer once the gesture was already under way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopEntry {
    pub id: u64,
    pub can_pop: bool,
    invocations: Vec<bool>,
}

impl PopEntry {
    pub fn new(id: u64, can_pop: bool) -> PopEntry {
        PopEntry {
            id,
            can_pop,
            invocations: Vec::new(),
        }
    }

    /// Upstream's `onPopInvokedWithResult`, whose `didPop` argument is the
    /// interesting one: an entry is told about a pop **whether or not it went
    /// through**, so a form can show "you have unsaved changes" exactly when
    /// it was the one that refused.
    pub fn pop_invoked(&mut self, did_pop: bool) {
        self.invocations.push(did_pop);
    }

    /// What this entry was told, in order.
    pub fn invocations(&self) -> &[bool] {
        &self.invocations
    }
}

/// Upstream `Route`'s `popped` future, `currentResult` and `didComplete`
/// together: **what a route hands back when it goes**.
///
/// # The `??` is the whole of it
///
/// ```dart
/// void didComplete(T? result) {
///   _popCompleter.complete(result ?? currentResult);
/// }
/// ```
///
/// A route popped without a result does not hand back nothing -- it hands back
/// its own `currentResult`. Upstream documents the pair from both ends
/// (*"When this route is popped, if the result isn't specified or if it's
/// null, this value will be used instead"*), and it is the difference between
/// a dialog dismissed by tapping the barrier answering `null` and answering
/// "whatever was selected when it closed".
///
/// `currentResult` is `null` on `Route` and **nothing in the framework
/// overrides it**: it exists for applications. A route that has a current
/// selection sets it, and then closing the route any which way still answers
/// with that selection.
///
/// # The value, here
///
/// Upstream's result is `T?` for the route's own type parameter. This port
/// carries a `String`, the way [`crate::localizations::LoadedResources`] does:
/// what these rules are about is *which* value travels and *when*, and a
/// string is enough to tell one from another.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteCompletion {
    current_result: Option<String>,
    completed: Option<Option<String>>,
}

impl RouteCompletion {
    pub fn new() -> RouteCompletion {
        RouteCompletion::default()
    }

    /// Upstream's `currentResult`, which an application overrides.
    pub fn with_current_result(mut self, result: impl Into<String>) -> RouteCompletion {
        self.current_result = Some(result.into());
        self
    }

    pub fn current_result(&self) -> Option<&str> {
        self.current_result.as_deref()
    }

    /// Upstream's `didComplete`: complete the `popped` future with `result`,
    /// **or with `currentResult` when there is no result**.
    ///
    /// A second call is declined rather than fatal. Upstream's `Completer`
    /// throws on being completed twice, and the navigator has two callers --
    /// `didPop` and `pushReplacement` -- so "the first one wins" is a rule and
    /// not an accident. Declining follows this crate's usual stance, the same
    /// one [`crate::theatre::ModalHandle::dismiss`] takes: a second attempt is
    /// a caller's mistake to find in a test, not a reason to take the
    /// application down in front of a reader.
    pub fn did_complete(&mut self, result: Option<String>) {
        if self.completed.is_some() {
            return;
        }
        self.completed = Some(result.or_else(|| self.current_result.clone()));
    }

    /// Upstream's `popped` future: `None` while the route is still on the
    /// navigator, and `Some(value)` once it has gone -- where the value may
    /// itself be `None`, for a route that had nothing to say and no
    /// `currentResult` to fall back on.
    ///
    /// The two `None`s are different questions, which is why they are nested
    /// rather than flattened: "has it finished" and "did it hand anything
    /// back".
    pub fn popped(&self) -> Option<Option<&str>> {
        self.completed.as_ref().map(|result| result.as_deref())
    }

    /// Whether the future has been completed at all.
    pub fn is_completed(&self) -> bool {
        self.completed.is_some()
    }
}

/// Upstream `_RouteLifecycle`: where an entry of the navigator's history is in
/// its life, **in order**.
///
/// The order is the whole type. Every question the navigator asks about an
/// entry -- is it present, will it be, should it be announced -- is a range
/// over these variants, so a variant inserted in the wrong place silently
/// moves several answers at once. Upstream writes them as one enum with the
/// section comments below kept verbatim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RouteLifecycle {
    /// *"we will wait for transition delegate to decide what to do with this
    /// route."*
    Staging,
    // routes that are present:
    /// *"a route created by onGenerateInitialRoutes or by the initial
    /// widget.pages"*
    Add,
    /// *"we'll waiting for the future from didPush of top-most route to
    /// complete"*
    Adding,
    // routes that are ready for transition.
    /// *"a route added via push() and friends"*
    Push,
    /// *"a route added via pushReplace() and friends"*
    PushReplace,
    /// *"we're waiting for the future from didPush to complete"*
    Pushing,
    /// *"a route added via replace() and friends"*
    Replace,
    /// *"route is being harmless"*
    Idle,
    // routes that are not present:
    /// *"we'll want to call didPop"*
    Pop,
    /// *"we'll want to call didComplete"*
    Complete,
    /// *"we'll want to run didReplace/didRemove etc"*
    Remove,
    /// *"we're waiting for the route to call finalizeRoute to switch to
    /// dispose"*
    Popping,
    /// *"we are waiting for subsequent routes to be done animating"*
    Removing,
    Dispose,
    /// *"The entry is waiting for its widget subtree to be disposed first."*
    Disposing,
    Disposed,
}

impl RouteLifecycle {
    /// Upstream's `isPresent`: `add` through `remove`, **inclusive**.
    ///
    /// It reaches three variants past the `// routes that are not present:`
    /// comment above `pop` -- a route being popped, completing, or being
    /// removed is still *present* by this question, and only stops being so
    /// once it is `popping`. The comment is about `willBePresent`, which stops
    /// at `idle`; reading it as the boundary for this one is the mistake worth
    /// naming, because a route that has just been popped is still on screen
    /// and still the answer to "which route is current".
    pub fn is_present(self) -> bool {
        self >= RouteLifecycle::Add && self <= RouteLifecycle::Remove
    }

    /// Upstream's `willBePresent`: `add` through `idle` -- what will still be
    /// there once everything settles.
    pub fn will_be_present(self) -> bool {
        self >= RouteLifecycle::Add && self <= RouteLifecycle::Idle
    }

    /// Upstream's `isPresentForRestoration`: everything up to and including
    /// `idle`, **without** a lower bound -- so `staging` counts. A route the
    /// transition delegate has not ruled on yet is still part of the state to
    /// restore.
    pub fn is_present_for_restoration(self) -> bool {
        self <= RouteLifecycle::Idle
    }

    /// Upstream's `suitableForAnnouncement`: `push` through `removing`.
    ///
    /// A narrower window than [`RouteLifecycle::is_present`] at **both** ends:
    /// a route that is only being added is not announced, and one that is
    /// still animating out is.
    pub fn suitable_for_announcement(self) -> bool {
        self >= RouteLifecycle::Push && self <= RouteLifecycle::Removing
    }
}

/// One entry of the navigator's history: upstream's `_RouteEntry`.
///
/// It carries the route, where that route is in its life, and the **pending
/// result** -- the value a pop is on its way to hand back. Upstream keeps the
/// result here rather than on the route for a reason this port inherits: a
/// route can be marked for popping by the transition delegate long before
/// anything calls `didPop`, and the value has to wait somewhere in between.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HistoryEntry {
    pub route: u64,
    pub state: RouteLifecycle,
    /// Upstream's `_RouteEntry.pendingResult`.
    pub pending_result: Option<String>,
    /// The route's `popped` future -- see [`RouteCompletion`].
    pub completion: RouteCompletion,
}

impl HistoryEntry {
    pub fn new(route: u64, state: RouteLifecycle) -> HistoryEntry {
        HistoryEntry {
            route,
            state,
            pending_result: None,
            completion: RouteCompletion::new(),
        }
    }

    /// The value this pop is carrying, which arrives with the request and
    /// waits here until the route is actually asked.
    pub fn with_pending_result(mut self, result: impl Into<String>) -> HistoryEntry {
        self.pending_result = Some(result.into());
        self
    }

    /// The route's own `currentResult`, which is what a pop with no value
    /// falls back to.
    pub fn with_current_result(mut self, result: impl Into<String>) -> HistoryEntry {
        self.completion = self.completion.with_current_result(result);
        self
    }

    /// Upstream's `_RouteEntry.isPresentPredicate`.
    pub fn is_present(&self) -> bool {
        self.state.is_present()
    }

    /// Upstream's `_RouteEntry.handlePop`: *"A route can be marked for pop by
    /// transition delegate or Navigator.pop, this method actually pops the
    /// route by calling Route.didPop."*
    ///
    /// `ask_the_route` is `Route.didPop`: it answers `false` when the route
    /// consumed the pop itself, which is what a route with local history does
    /// (see [`LocalHistoryRoute::did_pop`]). It is handed **the state the
    /// entry is in while it is being asked**, which is the point of the first
    /// rule below.
    ///
    /// Three things worth reading slowly:
    ///
    /// * **The state goes to `popping` before the route is asked**, and comes
    ///   back to `idle` if the route refuses. Setting it only on success would
    ///   look equivalent and is not: a route consuming the pop rebuilds, and
    ///   what it reads about itself while it does is `popping`.
    /// * **An already-completed route is left alone.** Upstream's comment says
    ///   which case that is -- *"a page-based route popped through the
    ///   Navigator.pop. The didPop should have been called"* -- and asserts
    ///   there is no pending result to lose. Nothing is taken from the entry
    ///   in that branch, which is what "no further action" means.
    /// * **The pending result is cleared on the way out**, so the value cannot
    ///   be handed over a second time. Together with
    ///   [`RouteCompletion::did_complete`] declining a second completion, that
    ///   is two locks on the same door, which is upstream's arrangement too.
    pub fn handle_pop(&mut self, ask_the_route: impl FnOnce(RouteLifecycle) -> bool) -> bool {
        self.state = RouteLifecycle::Popping;
        if self.completion.is_completed() {
            return true;
        }
        if !ask_the_route(self.state) {
            self.state = RouteLifecycle::Idle;
            return false;
        }
        self.completion.did_complete(self.pending_result.take());
        true
    }

    /// Upstream's `_RouteEntry.handleComplete`: complete the route's future
    /// with whatever the pop was carrying, drop it, and move on to `remove`.
    ///
    /// The other way a route's future is completed -- `pushReplacement` takes
    /// this road rather than `handlePop`, which is why the value lives on the
    /// entry and not in a pop that never happened.
    pub fn handle_complete(&mut self) {
        self.completion.did_complete(self.pending_result.take());
        self.state = RouteLifecycle::Remove;
    }
}

impl Default for RouteLifecycle {
    /// `idle` -- upstream's *"route is being harmless"*, which is what an
    /// entry is whenever nothing is happening to it.
    fn default() -> RouteLifecycle {
        RouteLifecycle::Idle
    }
}

/// Where a route stands in the navigator's history: upstream `Route`'s
/// `isCurrent`, `isFirst`, `isActive` and `hasActiveRouteBelow`.
///
/// All four are questions about the **history**, not about the route -- which
/// is why they are here as one type over a list of entries rather than four
/// booleans on a route that would have to be kept in step. Upstream computes
/// each one by walking `_navigator!._history` at the moment it is asked, for
/// the same reason.
///
/// # Not installed is not "no"
///
/// Three of the four begin `if (!_installed) return false`, and the fourth
/// answers through a null-aware navigator that gives the same. A route that
/// has never been given to a navigator is not the current route, not the first
/// one and not active -- it is not anywhere, and answering `false` is how
/// upstream says so without a third value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RoutePosition {
    /// The navigator's history, bottom first -- the order upstream's `_history`
    /// is in.
    pub history: Vec<HistoryEntry>,
}

impl RoutePosition {
    pub fn new(history: Vec<HistoryEntry>) -> RoutePosition {
        RoutePosition { history }
    }

    /// Upstream's `isCurrent`: the **last** present entry is this route.
    pub fn is_current(&self, route: u64, installed: bool) -> bool {
        if !installed {
            return false;
        }
        self.history
            .iter()
            .rev()
            .find(|entry| entry.is_present())
            .map(|entry| entry.route == route)
            .unwrap_or(false)
    }

    /// Upstream's `isFirst`: the **first** present entry is this route.
    pub fn is_first(&self, route: u64, installed: bool) -> bool {
        if !installed {
            return false;
        }
        self.history
            .iter()
            .find(|entry| entry.is_present())
            .map(|entry| entry.route == route)
            .unwrap_or(false)
    }

    /// Upstream's `isActive`: this route's **first** entry is present.
    ///
    /// Not "any entry of it is present": upstream takes the first entry for
    /// this route and asks whether that one is present, which is the same
    /// answer for a route in the history once and a different one for a route
    /// that is in it twice.
    pub fn is_active(&self, route: u64) -> bool {
        self.history
            .iter()
            .find(|entry| entry.route == route)
            .map(|entry| entry.is_present())
            .unwrap_or(false)
    }

    /// Upstream's `hasActiveRouteBelow`: walking up from the bottom, is there
    /// a present entry **before** this route's own?
    ///
    /// The walk stops at this route rather than counting everything present,
    /// which is what makes it "below" rather than "elsewhere".
    pub fn has_active_route_below(&self, route: u64, installed: bool) -> bool {
        if !installed {
            return false;
        }
        for entry in &self.history {
            if entry.route == route {
                return false;
            }
            if entry.is_present() {
                return true;
            }
        }
        false
    }
}

/// Upstream `PredictiveBackRoute`: what the platform's back gesture needs to
/// know about a route, and what it does to one.
pub trait PredictiveBackRoute {
    /// Upstream's `popGestureInProgress`.
    fn pop_gesture_in_progress(&self) -> bool;

    /// Upstream's `isCurrent`.
    fn is_current(&self) -> bool;

    /// Upstream's `handleStartBackGesture`.
    fn handle_start_back_gesture(&mut self, progress: f32);

    /// Upstream's `handleUpdateBackGestureProgress`.
    fn handle_update_back_gesture_progress(&mut self, progress: f32);

    /// Upstream's `handleCancelBackGesture`.
    fn handle_cancel_back_gesture(&mut self);

    /// Upstream's `handleCommitBackGesture`.
    fn handle_commit_back_gesture(&mut self);
}

/// Upstream `OverlayRoute`: a route that is a set of overlay entries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OverlayRoute {
    entries: Vec<u64>,
    installed: bool,
    finalized: bool,
}

impl OverlayRoute {
    pub fn new() -> OverlayRoute {
        OverlayRoute::default()
    }

    /// Upstream's `install`, which **asserts the entries are empty** first --
    /// installing twice would leave the first set orphaned in the overlay,
    /// with nothing holding a reference to take them out again.
    pub fn install(&mut self, entries: Vec<u64>) {
        debug_assert!(self.entries.is_empty(), "a route installs once");
        self.entries = entries;
        self.installed = true;
    }

    pub fn entries(&self) -> &[u64] {
        &self.entries
    }

    pub fn is_installed(&self) -> bool {
        self.installed
    }

    pub fn is_finalized(&self) -> bool {
        self.finalized
    }

    /// Upstream's `finishedWhenPopped`, **true** for a plain overlay route:
    /// its entries come out as soon as it is popped.
    pub fn finished_when_popped(&self) -> bool {
        true
    }

    /// Upstream's `didPop`, which finalizes the route -- taking its entries
    /// out of the overlay -- only when `finishedWhenPopped` says so. A route
    /// that answers false is **on the hook for calling `finalizeRoute`
    /// itself**, which is what a transition route does once its animation has
    /// run to the end.
    pub fn did_pop(&mut self, finished_when_popped: bool) -> bool {
        if finished_when_popped {
            self.finalize();
        }
        true
    }

    /// Upstream's `NavigatorState.finalizeRoute`, seen from this side.
    pub fn finalize(&mut self) {
        self.finalized = true;
    }

    /// Upstream's `dispose`, which drops the entries.
    pub fn dispose(&mut self) {
        self.entries.clear();
    }
}

/// Upstream `TransitionRoute`: a route that animates in and out.
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionRoute {
    pub overlay: OverlayRoute,
    /// Upstream's `transitionDuration`.
    pub transition_duration_micros: i64,
    /// Upstream's `reverseTransitionDuration`, which **defaults to the forward
    /// one** but is a separate getter because leaving is often meant to be
    /// quicker: the reader has already decided.
    pub reverse_transition_duration_micros: i64,
    /// Upstream's `opaque`: whether the routes behind this one can stop being
    /// built once the entrance transition finishes.
    pub opaque: bool,
    /// Upstream's `allowSnapshotting`, true by default.
    pub allow_snapshotting: bool,
    animation: f32,
    pop_finalized: bool,
    gesture_in_progress: bool,
    current: bool,
}

impl Default for TransitionRoute {
    fn default() -> TransitionRoute {
        TransitionRoute::new(300_000)
    }
}

impl TransitionRoute {
    pub fn new(transition_duration_micros: i64) -> TransitionRoute {
        TransitionRoute {
            overlay: OverlayRoute::new(),
            transition_duration_micros,
            reverse_transition_duration_micros: transition_duration_micros,
            opaque: true,
            allow_snapshotting: true,
            animation: 0.0,
            pop_finalized: false,
            gesture_in_progress: false,
            current: false,
        }
    }

    pub fn with_reverse_duration(mut self, micros: i64) -> Self {
        self.reverse_transition_duration_micros = micros;
        self
    }

    pub fn with_opaque(mut self, opaque: bool) -> Self {
        self.opaque = opaque;
        self
    }

    pub fn animation(&self) -> f32 {
        self.animation
    }

    pub fn set_animation(&mut self, value: f32) {
        self.animation = value.clamp(0.0, 1.0);
    }

    /// Upstream's `_controller.isDismissed`.
    pub fn is_dismissed(&self) -> bool {
        self.animation == 0.0
    }

    /// Upstream's `finishedWhenPopped` override, and the one place in this
    /// file where the answer is neither constant nor obvious:
    ///
    /// ```text
    /// bool get finishedWhenPopped => _controller!.isDismissed && !_popFinalized;
    /// ```
    ///
    /// A route normally pops first and animates out afterwards, so at the
    /// moment of the pop the animation is still at one and this is false --
    /// the entries stay, or the reader would watch the page vanish instead of
    /// slide away. But the iOS back-swipe drags a route **all the way to
    /// dismissed while it is still current**, and pops it only then. By that
    /// point there is nothing left to animate, so the entries can come out at
    /// once. Upstream's own note is that without this, such a route would
    /// never be disposed at all.
    ///
    /// The `_popFinalized` half makes it a one-shot: a second pop must not
    /// finalize a route that already was.
    pub fn finished_when_popped(&self) -> bool {
        self.is_dismissed() && !self.pop_finalized
    }

    /// Upstream's `didPop`.
    pub fn did_pop(&mut self) -> bool {
        let finished = self.finished_when_popped();
        if finished {
            self.pop_finalized = true;
        }
        self.overlay.did_pop(finished)
    }

    /// Upstream's `canTransitionTo`/`canTransitionFrom` default: a route
    /// transitions with anything.
    pub fn can_transition_to(&self, _other: &TransitionRoute) -> bool {
        true
    }

    pub fn set_current(&mut self, current: bool) {
        self.current = current;
    }

    /// Upstream's `TransitionRoute.didReplace`:
    ///
    /// ```dart
    /// if (oldRoute is TransitionRoute) {
    ///   _controller!.value = oldRoute._controller!.value;
    /// }
    /// ```
    ///
    /// **The replacement takes over where the old route had got to.** A route
    /// that replaces a half-open one at 0.4 opens from 0.4, not from nothing:
    /// starting again would play an entrance the reader has already watched
    /// most of, and the two screens would cross twice.
    ///
    /// `replacing` is `None` when the old route was not a transition route --
    /// it has no value to take over, and this route keeps its own.
    pub fn did_replace(&mut self, replacing: Option<f32>) {
        if let Some(value) = replacing {
            self.set_animation(value);
        }
    }

    /// Upstream's `TransitionRoute.didChangeNext`, whose body is
    /// `_updateSecondaryAnimation(nextRoute)`: **whether this route can be
    /// pushed past by the one arriving above it**.
    ///
    /// The answer decides whether this route animates out of the way or sits
    /// still while the next one covers it, and it is `canTransitionTo`'s to
    /// give -- see [`TransitionRoute::can_transition_to`]. `None` for a route
    /// with nothing above it, which is the same as "nothing to move for".
    pub fn secondary_animates_for(&self, next: Option<&TransitionRoute>) -> bool {
        next.map(|next| self.can_transition_to(next))
            .unwrap_or(false)
    }
}

impl PredictiveBackRoute for TransitionRoute {
    fn pop_gesture_in_progress(&self) -> bool {
        self.gesture_in_progress
    }

    fn is_current(&self) -> bool {
        self.current
    }

    fn handle_start_back_gesture(&mut self, progress: f32) {
        self.gesture_in_progress = true;
        self.set_animation(1.0 - progress);
    }

    fn handle_update_back_gesture_progress(&mut self, progress: f32) {
        self.set_animation(1.0 - progress);
    }

    /// A cancelled gesture puts the route **back**, not away: the reader
    /// changed their mind mid-swipe, and the page they were leaving is the one
    /// they meant to keep.
    fn handle_cancel_back_gesture(&mut self) {
        self.gesture_in_progress = false;
        self.set_animation(1.0);
    }

    fn handle_commit_back_gesture(&mut self) {
        self.gesture_in_progress = false;
        self.set_animation(0.0);
    }
}

/// Upstream `RouteBarrierDetails`: what a barrier builder is told.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteBarrierDetails {
    pub animation: f32,
    pub barrier_color: Option<Color>,
    /// Upstream's `barrierLabel`, which a screen reader announces. Upstream
    /// requires it whenever the barrier is dismissible, because a barrier the
    /// reader can dismiss is a control, and a control needs a name.
    pub barrier_label: Option<String>,
    pub barrier_dismissible: bool,
}

impl RouteBarrierDetails {
    pub fn new(animation: f32, barrier_dismissible: bool) -> RouteBarrierDetails {
        RouteBarrierDetails {
            animation,
            barrier_color: None,
            barrier_label: None,
            barrier_dismissible,
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.barrier_color = Some(color);
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.barrier_label = Some(label.into());
        self
    }

    /// Upstream's assertion: a dismissible barrier must have a label.
    pub fn is_valid(&self) -> bool {
        !self.barrier_dismissible || self.barrier_label.is_some()
    }
}

/// Upstream `ModalRoute`: a route that blocks what is behind it, with local
/// history of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct ModalRoute {
    pub transition: TransitionRoute,
    pub local_history: LocalHistoryRoute,
    /// Upstream's `receivedTransition`: the transition the route **above**
    /// this one handed down, for this one to play as it is covered.
    ///
    /// `None` is not "no animation" -- it is *this route does its own thing*,
    /// which is what a route gets when the one above it either cannot be
    /// transitioned to or delegates the same transition this one already has.
    pub received_transition: Option<u64>,
    /// This route's own `delegatedTransition`, which is what it hands **down**
    /// to whatever it covers.
    pub delegated_transition: Option<u64>,
    /// How many times the barrier was marked for rebuilding.
    barrier_rebuilds: usize,
    /// How many times the page under the barrier was forced to rebuild.
    page_rebuilds: usize,
    /// What `maintainState` was last pushed into the scope, which is the only
    /// way the scope hears about a change to it.
    scope_maintains_state: bool,
    pub barrier_dismissible: bool,
    pub barrier_color: Option<Color>,
    pub barrier_label: Option<String>,
    /// Upstream's `maintainState`. A modal route's subtree stays alive under a
    /// route pushed on top of it, so scroll positions and half-typed text
    /// survive a round trip.
    pub maintain_state: bool,
    /// Whether this route is the bottom of the stack.
    pub is_first: bool,
    /// The page-based `canPop`, checked before everything else.
    pub page_can_pop: bool,
    pop_entries: Vec<PopEntry>,
}

impl Default for ModalRoute {
    fn default() -> ModalRoute {
        ModalRoute::new()
    }
}

impl ModalRoute {
    pub fn new() -> ModalRoute {
        ModalRoute {
            received_transition: None,
            delegated_transition: None,
            barrier_rebuilds: 0,
            page_rebuilds: 0,
            scope_maintains_state: true,
            transition: TransitionRoute::default(),
            local_history: LocalHistoryRoute::new(),
            barrier_dismissible: false,
            barrier_color: None,
            barrier_label: None,
            maintain_state: true,
            is_first: false,
            page_can_pop: true,
            pop_entries: Vec::new(),
        }
    }

    /// Upstream's `ModalRoute.didChangeNext`:
    ///
    /// ```dart
    /// if (nextRoute is ModalRoute<T> &&
    ///     canTransitionTo(nextRoute) &&
    ///     nextRoute.delegatedTransition != delegatedTransition) {
    ///   receivedTransition = nextRoute.delegatedTransition;
    /// } else {
    ///   receivedTransition = null;
    /// }
    /// ```
    ///
    /// **Three conditions, and the third is the one worth stopping at**: a
    /// route above that delegates the *same* transition this one already has
    /// hands down nothing. Taking it anyway would play the transition twice
    /// over one screen -- once because this route has it and once because it
    /// was given it -- which is the sort of doubling nobody reads back out of
    /// a screenshot.
    ///
    /// It ends with `changedInternalState`, because what a route plays while
    /// being covered is state its subtree can see.
    pub fn did_change_next(&mut self, next: Option<&ModalRoute>, tree_is_locked: bool) {
        self.received_transition = match next {
            Some(next)
                if self.transition.can_transition_to(&next.transition)
                    && next.delegated_transition != self.delegated_transition =>
            {
                next.delegated_transition
            }
            _ => None,
        };
        self.changed_internal_state(tree_is_locked);
    }

    /// Upstream's `ModalRoute.didChangePrevious`, whose whole body is
    /// `changedInternalState()` -- what is *below* this route changes nothing
    /// this route draws, but the barrier's semantics say "dismiss to the thing
    /// behind", so the barrier is rebuilt.
    pub fn did_change_previous(&mut self, tree_is_locked: bool) {
        self.changed_internal_state(tree_is_locked);
    }

    /// Upstream's `ModalRoute.changedInternalState`.
    ///
    /// Two things, and the guard belongs to only one of them: the barrier is
    /// marked for rebuilding **unless the tree is locked** -- nothing may be
    /// marked dirty during a build -- while `maintainState` is pushed into the
    /// scope either way, because it is a value being assigned rather than a
    /// rebuild being requested.
    pub fn changed_internal_state(&mut self, tree_is_locked: bool) {
        if !tree_is_locked {
            self.barrier_rebuilds += 1;
        }
        self.scope_maintains_state = self.maintain_state;
    }

    /// Upstream's `ModalRoute.changedExternalState`: the barrier is rebuilt
    /// **and** the page is forced to rebuild.
    ///
    /// The page too, and that is the difference from the internal one: the
    /// navigator itself changed -- a new `MaterialApp` above it, say -- so
    /// what the page built from that state is out of date, and marking only
    /// the barrier would leave the screen showing the old one.
    pub fn changed_external_state(&mut self) {
        self.barrier_rebuilds += 1;
        self.page_rebuilds += 1;
    }

    pub fn barrier_rebuilds(&self) -> usize {
        self.barrier_rebuilds
    }

    pub fn page_rebuilds(&self) -> usize {
        self.page_rebuilds
    }

    /// What the scope was last told `maintainState` is.
    pub fn scope_maintains_state(&self) -> bool {
        self.scope_maintains_state
    }

    pub fn with_barrier_dismissible(mut self, dismissible: bool) -> Self {
        self.barrier_dismissible = dismissible;
        self
    }

    pub fn with_barrier_label(mut self, label: impl Into<String>) -> Self {
        self.barrier_label = Some(label.into());
        self
    }

    pub fn with_barrier_color(mut self, color: Color) -> Self {
        self.barrier_color = Some(color);
        self
    }

    pub fn with_maintain_state(mut self, maintain: bool) -> Self {
        self.maintain_state = maintain;
        self
    }

    pub fn with_is_first(mut self, is_first: bool) -> Self {
        self.is_first = is_first;
        self
    }

    /// Upstream's `registerPopEntry`.
    pub fn register_pop_entry(&mut self, entry: PopEntry) {
        self.pop_entries.push(entry);
    }

    /// Upstream's `unregisterPopEntry`.
    pub fn unregister_pop_entry(&mut self, id: u64) {
        self.pop_entries.retain(|entry| entry.id != id);
    }

    pub fn pop_entries(&self) -> &[PopEntry] {
        &self.pop_entries
    }

    /// Upstream's `popDisposition`, the top of the chain.
    ///
    /// **Any one entry saying no is enough**, and the entries are asked before
    /// local history: a page with two forms on it must not lose either one's
    /// unsaved work because the other was willing to go, and must not lose it
    /// to a back press that was only meant to close a sheet.
    pub fn pop_disposition(&self) -> RoutePopDisposition {
        if self.pop_entries.iter().any(|entry| !entry.can_pop) {
            return RoutePopDisposition::DoNotPop;
        }
        self.local_history
            .pop_disposition(self.is_first, self.page_can_pop)
    }

    /// Upstream's `onPopInvokedWithResult` fan-out, which tells **every**
    /// entry rather than only the one that refused -- a form that did not
    /// object still wants to know the page stayed.
    pub fn pop_invoked(&mut self, did_pop: bool) {
        for entry in self.pop_entries.iter_mut() {
            entry.pop_invoked(did_pop);
        }
    }

    /// The details a barrier builder is given.
    pub fn barrier_details(&self) -> RouteBarrierDetails {
        RouteBarrierDetails {
            animation: self.transition.animation(),
            barrier_color: self.barrier_color,
            barrier_label: self.barrier_label.clone(),
            barrier_dismissible: self.barrier_dismissible,
        }
    }
}

/// Upstream `PopupRoute`: a modal route that never becomes opaque.
///
/// Its whole content is two overrides, and both say the same thing: what is
/// behind a popup stays visible and stays alive. A menu that blanked the page
/// under it would have to rebuild that page on every dismissal, and would look
/// wrong doing it.
#[derive(Debug, Clone, PartialEq)]
pub struct PopupRoute {
    pub modal: ModalRoute,
}

impl Default for PopupRoute {
    fn default() -> PopupRoute {
        PopupRoute::new()
    }
}

impl PopupRoute {
    pub fn new() -> PopupRoute {
        let mut modal = ModalRoute::new();
        modal.transition.opaque = false;
        modal.maintain_state = true;
        PopupRoute { modal }
    }

    /// Upstream's `opaque` override, which is always false.
    pub fn opaque(&self) -> bool {
        false
    }

    /// Upstream's `maintainState` override, which is always true.
    pub fn maintain_state(&self) -> bool {
        true
    }
}

/// Upstream `RawDialogRoute`: a popup route with a dialog in it.
#[derive(Debug, Clone, PartialEq)]
pub struct RawDialogRoute {
    pub popup: PopupRoute,
    pub full_screen_dialog: bool,
}

impl Default for RawDialogRoute {
    fn default() -> RawDialogRoute {
        RawDialogRoute::new()
    }
}

impl RawDialogRoute {
    /// Upstream's default transition for a dialog: 200ms, shorter than a
    /// page's 300 -- a dialog appears over what the reader is already looking
    /// at rather than replacing it, so it has less distance to cover.
    pub const DEFAULT_TRANSITION_MICROS: i64 = 200_000;

    /// Upstream's default barrier: half-transparent black, `0x80000000`. Not
    /// opaque, because the reader should still see the page the dialog is
    /// asking them about.
    pub const DEFAULT_BARRIER_COLOR: Color = Color(0x8000_0000);

    pub fn new() -> RawDialogRoute {
        let mut popup = PopupRoute::new();
        popup.modal.transition.transition_duration_micros = Self::DEFAULT_TRANSITION_MICROS;
        popup.modal.transition.reverse_transition_duration_micros = Self::DEFAULT_TRANSITION_MICROS;
        // A dialog's barrier is dismissible by **default**, where a plain modal
        // route's is not: a dialog is a question the reader is allowed to walk
        // away from, and tapping outside it is how they say so.
        popup.modal.barrier_dismissible = true;
        popup.modal.barrier_color = Some(Self::DEFAULT_BARRIER_COLOR);
        RawDialogRoute {
            popup,
            full_screen_dialog: false,
        }
    }

    pub fn with_barrier_dismissible(mut self, dismissible: bool) -> Self {
        self.popup.modal.barrier_dismissible = dismissible;
        self
    }

    pub fn with_barrier_color(mut self, color: Option<Color>) -> Self {
        self.popup.modal.barrier_color = color;
        self
    }

    pub fn with_transition_duration(mut self, micros: i64) -> Self {
        self.popup.modal.transition.transition_duration_micros = micros;
        self.popup
            .modal
            .transition
            .reverse_transition_duration_micros = micros;
        self
    }

    pub fn transition_duration_micros(&self) -> i64 {
        self.popup.modal.transition.transition_duration_micros
    }
}

/// Upstream `RouteAware`: something that wants to know about the route it is
/// in.
pub trait RouteAware {
    /// Upstream's `didPush`: this route was pushed, or the subscriber joined
    /// while it was already there.
    fn did_push(&mut self) {}

    /// Upstream's `didPop`: this route was popped.
    fn did_pop(&mut self) {}

    /// Upstream's `didPushNext`: something was pushed on top of this route.
    fn did_push_next(&mut self) {}

    /// Upstream's `didPopNext`: what covered this route went away.
    fn did_pop_next(&mut self) {}
}

/// One of the four things a [`RouteObserver`] can say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteAwareEvent {
    DidPush,
    DidPop,
    DidPushNext,
    DidPopNext,
}

/// Upstream `RouteObserver`: routes navigator events to the subscribers of the
/// route they concern.
#[derive(Debug, Default)]
pub struct RouteObserver {
    /// Route to its subscribers, in registration order. Upstream keeps a
    /// `Map<R, Set<RouteAware>>`; the order matters here only so that the
    /// deliveries are reproducible.
    listeners: Vec<(u64, Vec<u64>)>,
    delivered: Vec<(u64, RouteAwareEvent)>,
}

impl RouteObserver {
    pub fn new() -> RouteObserver {
        RouteObserver::default()
    }

    /// What was delivered, in order.
    pub fn delivered(&self) -> &[(u64, RouteAwareEvent)] {
        &self.delivered
    }

    /// Upstream's `debugObservingRoute`.
    pub fn is_observing_route(&self, route: u64) -> bool {
        self.listeners.iter().any(|(held, _)| *held == route)
    }

    /// Upstream's `subscribe`.
    ///
    /// **A genuinely new subscriber is told `didPush` at once**, and that is
    /// the interesting part: a widget that appears part-way through a route's
    /// life still needs to know it is on screen, and it never saw the push
    /// that put it there. Re-subscribing something already there says nothing,
    /// or a rebuild would read as a navigation.
    pub fn subscribe(&mut self, aware: u64, route: u64) {
        let position = self.listeners.iter().position(|(held, _)| *held == route);
        let index = match position {
            Some(index) => index,
            None => {
                self.listeners.push((route, Vec::new()));
                self.listeners.len() - 1
            }
        };
        if self.listeners[index].1.contains(&aware) {
            return;
        }
        self.listeners[index].1.push(aware);
        self.delivered.push((aware, RouteAwareEvent::DidPush));
    }

    /// Upstream's `unsubscribe`, which walks **every** route rather than the
    /// one the subscriber joined -- it may have joined several, and upstream
    /// takes no argument saying which. Routes left with no subscribers are
    /// dropped, so the map does not grow one dead entry per page visited.
    pub fn unsubscribe(&mut self, aware: u64) {
        for (_, subscribers) in self.listeners.iter_mut() {
            subscribers.retain(|held| *held != aware);
        }
        self.listeners
            .retain(|(_, subscribers)| !subscribers.is_empty());
    }

    /// Upstream's `didPop`.
    ///
    /// The **previous** route's subscribers are told first, then the popped
    /// route's. The order is deliberate: whatever is being revealed should
    /// have refreshed itself before the thing that is leaving announces it has
    /// gone.
    pub fn did_pop(&mut self, route: u64, previous_route: Option<u64>) {
        if let Some(previous) = previous_route {
            for aware in self.subscribers_of(previous) {
                self.delivered.push((aware, RouteAwareEvent::DidPopNext));
            }
        }
        for aware in self.subscribers_of(route) {
            self.delivered.push((aware, RouteAwareEvent::DidPop));
        }
    }

    /// Upstream's `didPush`, which tells only the route being **covered**.
    ///
    /// Nothing is said to the pushed route's own subscribers, and nothing can
    /// be: they subscribe from inside a subtree that does not exist yet. That
    /// is exactly the gap [`RouteObserver::subscribe`] closes when they
    /// arrive.
    pub fn did_push(&mut self, _route: u64, previous_route: Option<u64>) {
        let Some(previous) = previous_route else {
            return;
        };
        for aware in self.subscribers_of(previous) {
            self.delivered.push((aware, RouteAwareEvent::DidPushNext));
        }
    }

    fn subscribers_of(&self, route: u64) -> Vec<u64> {
        self.listeners
            .iter()
            .find(|(held, _)| *held == route)
            .map(|(_, subscribers)| subscribers.clone())
            .unwrap_or_default()
    }
}

/// Upstream `WillPopScope`: the deprecated ancestor of `PopScope`.
///
/// It is deprecated for a reason worth stating, because it is the same reason
/// [`PopEntry`] holds a standing answer: **`onWillPop` is asked at pop time
/// and returns a future.** Android's predictive back gesture has to know
/// before the swipe starts whether the page will leave, so it can draw the
/// page behind it -- and there is no way to get an answer out of a future
/// that has not been awaited yet. The replacement swaps the question for a
/// value that is always available.
///
/// The registration dance is the whole implementation: register on
/// `didChangeDependencies`, swap on `didUpdateWidget`, unregister on
/// `dispose`. The first of those is not `initState`, and that matters -- the
/// enclosing route is found through the context, and the context has no
/// ancestors to search until dependencies are being resolved.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WillPopScope {
    /// Whether a callback was supplied. Upstream allows null, meaning the
    /// scope is inert -- a caller can leave one in the tree and turn it off by
    /// passing null rather than removing the widget.
    pub has_callback: bool,
    registered: bool,
    registrations: usize,
    removals: usize,
}

impl WillPopScope {
    pub fn new(has_callback: bool) -> WillPopScope {
        WillPopScope {
            has_callback,
            registered: false,
            registrations: 0,
            removals: 0,
        }
    }

    pub fn is_registered(&self) -> bool {
        self.registered
    }

    pub fn registrations(&self) -> usize {
        self.registrations
    }

    pub fn removals(&self) -> usize {
        self.removals
    }

    /// Upstream's `didChangeDependencies`, which **removes before it adds**
    /// even on the first run. The route may have changed under the widget --
    /// it moved to a different one -- and the callback has to come off the old
    /// route before going onto the new.
    pub fn did_change_dependencies(&mut self, has_route: bool) {
        if self.has_callback && self.registered {
            self.registered = false;
            self.removals += 1;
        }
        if self.has_callback && has_route {
            self.registered = true;
            self.registrations += 1;
        }
    }

    /// Upstream's `didUpdateWidget`, which swaps only when the callback itself
    /// changed. A rebuild that passes the same callback must not churn the
    /// route's list.
    pub fn did_update_widget(&mut self, next_has_callback: bool, callback_changed: bool) {
        if !callback_changed {
            return;
        }
        if self.has_callback && self.registered {
            self.registered = false;
            self.removals += 1;
        }
        self.has_callback = next_has_callback;
        if self.has_callback {
            self.registered = true;
            self.registrations += 1;
        }
    }

    pub fn dispose(&mut self) {
        if self.has_callback && self.registered {
            self.registered = false;
            self.removals += 1;
        }
    }
}

/// Upstream `PageRoute`: a modal route that fills the screen.
///
/// Its two transition rules are a matched pair, and both say the same thing:
/// **a page route only animates alongside another page route.** A dialog
/// appearing over a page must not make the page slide, and a page arriving
/// under a dialog must not make the dialog move. Anything that is not a full
/// screen of content is not part of the same movement.
#[derive(Debug, Clone, PartialEq)]
pub struct PageRoute {
    pub modal: ModalRoute,
    /// Upstream's `fullscreenDialog`, which changes two things: the transition
    /// (up from the bottom rather than in from the side) and whether the back
    /// swipe works at all.
    pub fullscreen_dialog: bool,
    pub allow_snapshotting: bool,
    /// Whether the platform offers a back swipe for this route at all.
    pub platform_pop_gesture_available: bool,
}

impl Default for PageRoute {
    fn default() -> PageRoute {
        PageRoute::new()
    }
}

impl PageRoute {
    pub fn new() -> PageRoute {
        PageRoute {
            modal: ModalRoute::new(),
            fullscreen_dialog: false,
            allow_snapshotting: true,
            platform_pop_gesture_available: true,
        }
    }

    pub fn with_fullscreen_dialog(mut self, fullscreen: bool) -> Self {
        self.fullscreen_dialog = fullscreen;
        self
    }

    /// Upstream's `opaque`, which is **always true** for a page route: it
    /// fills the screen, so nothing behind it needs building.
    pub fn opaque(&self) -> bool {
        true
    }

    /// Upstream's `canTransitionTo` and `canTransitionFrom`, which are the
    /// same test in both directions.
    pub fn can_transition_with_page_route(&self, other_is_page_route: bool) -> bool {
        other_is_page_route
    }

    /// Upstream's `popGestureEnabled` override, with its comment attached:
    /// "Fullscreen dialogs aren't dismissible by back swipe."
    ///
    /// They come up from the bottom, so there is no edge a swipe would start
    /// from that means anything -- and a dialog is usually a question that
    /// wants an answer rather than a page to wander back out of.
    pub fn pop_gesture_enabled(&self) -> bool {
        !self.fullscreen_dialog && self.platform_pop_gesture_available
    }
}

/// Upstream `PageRouteBuilder`: a page route defined by callbacks instead of a
/// subclass.
///
/// Its defaults are the interesting part, because they are the answers a
/// caller gets for saying nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct PageRouteBuilder {
    pub page: PageRoute,
    pub transition_micros: i64,
    pub reverse_transition_micros: i64,
    pub opaque: bool,
    pub barrier_dismissible: bool,
    pub maintain_state: bool,
    /// Whether a `transitionsBuilder` was supplied.
    pub has_transitions_builder: bool,
}

impl Default for PageRouteBuilder {
    fn default() -> PageRouteBuilder {
        PageRouteBuilder::new()
    }
}

impl PageRouteBuilder {
    /// Upstream's default transition, in both directions: 300ms.
    pub const DEFAULT_TRANSITION_MICROS: i64 = 300_000;

    pub fn new() -> PageRouteBuilder {
        PageRouteBuilder {
            page: PageRoute::new(),
            transition_micros: Self::DEFAULT_TRANSITION_MICROS,
            reverse_transition_micros: Self::DEFAULT_TRANSITION_MICROS,
            opaque: true,
            // Upstream's default is **false**, unlike a dialog's: a page fills
            // the screen, so there is no outside to tap.
            barrier_dismissible: false,
            maintain_state: true,
            has_transitions_builder: false,
        }
    }

    /// Upstream's `_defaultTransitionsBuilder`, which returns the child
    /// **unchanged**.
    ///
    /// So a `PageRouteBuilder` with no transitions builder does not fade or
    /// slide -- it appears. That is a deliberate default for a class whose
    /// point is one-off routes: a caller who wanted a transition would have
    /// said which.
    pub fn default_transition(child: u64) -> u64 {
        child
    }

    pub fn with_transitions_builder(mut self) -> Self {
        self.has_transitions_builder = true;
        self
    }
}

// -- Material's two modal routes ----------------------------------------------
//
// `ModalBottomSheetRoute` and `DialogRoute` are Material's subclasses of the
// popup route above. Both dim the page and sit on top of it; what separates
// them is how long they take and how hard you have to push to get rid of them.

/// Upstream's `_kBottomSheetEnterDuration` and `_kBottomSheetExitDuration`.
///
/// **Asymmetric on purpose: 250 in, 200 out.** Arriving should feel deliberate;
/// leaving should get out of the way. The same shape as the drawer's settle and
/// the tooltip's fade.
pub const BOTTOM_SHEET_ENTER_MS: u32 = 250;
pub const BOTTOM_SHEET_EXIT_MS: u32 = 200;

/// Upstream's `_kMinFlingVelocity` for a sheet -- **nearly twice the drawer's
/// 365**, though the two numbers were plainly picked apart rather than derived
/// from each other.
///
/// A bottom sheet usually holds something scrollable, and a vertical flick
/// inside one is far more often a scroll than a dismissal. Making the sheet
/// harder to fling away is what keeps the two gestures apart.
pub const BOTTOM_SHEET_MIN_FLING_VELOCITY: f32 = 700.0;

/// Upstream's `_kCloseProgressThreshold`.
pub const BOTTOM_SHEET_CLOSE_THRESHOLD: f32 = 0.5;

/// Upstream's dialog transition, and it is **shorter than the sheet's**: 150
/// against 250. A dialog interrupts; a sheet arrives.
pub const DIALOG_TRANSITION_MS: u32 = 150;

/// Both routes use `Colors.black54` behind them, so a dialog and a sheet dim
/// the page by exactly the same amount.
pub const MODAL_BARRIER_ALPHA: u8 = 0x8A;

/// Upstream `ModalBottomSheetRoute`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModalBottomSheetRoute {
    /// Whether a tap on the scrim closes it. Upstream's `barrierDismissible`
    /// is literally this field under another name -- one concept named twice,
    /// once for the sheet and once for the route it lives in.
    pub is_dismissible: bool,
    /// Whether the sheet is expected to hold a scrollable and may be dragged
    /// past the usual height.
    pub is_scroll_controlled: bool,
    /// The cap when it is not scroll controlled. Nine sixteenths: enough to be
    /// a sheet, not so much that the page behind it disappears.
    pub scroll_control_disabled_max_height_ratio: f32,
    /// Durations a caller's own controller supplies, if any.
    pub controller_duration_ms: Option<u32>,
    pub controller_reverse_duration_ms: Option<u32>,
}

impl ModalBottomSheetRoute {
    pub const DEFAULT_MAX_HEIGHT_RATIO: f32 = 9.0 / 16.0;

    pub fn new() -> ModalBottomSheetRoute {
        ModalBottomSheetRoute {
            is_dismissible: true,
            is_scroll_controlled: false,
            scroll_control_disabled_max_height_ratio:
                ModalBottomSheetRoute::DEFAULT_MAX_HEIGHT_RATIO,
            controller_duration_ms: None,
            controller_reverse_duration_ms: None,
        }
    }

    pub fn barrier_dismissible(&self) -> bool {
        self.is_dismissible
    }

    pub fn transition_duration_ms(&self) -> u32 {
        self.controller_duration_ms.unwrap_or(BOTTOM_SHEET_ENTER_MS)
    }

    /// Upstream falls back through four levels here, and the second is the one
    /// worth reading: `transitionAnimationController?.reverseDuration ??
    /// transitionAnimationController?.duration ?? ...`.
    ///
    /// **A controller that names only a forward duration is taken to mean it
    /// for both directions.** Skipping to the default instead would hand the
    /// caller an asymmetry they never asked for -- their 400ms in and the
    /// framework's 200ms out.
    pub fn reverse_transition_duration_ms(&self) -> u32 {
        self.controller_reverse_duration_ms
            .or(self.controller_duration_ms)
            .unwrap_or(BOTTOM_SHEET_EXIT_MS)
    }

    /// The height cap, or `None` when the sheet controls its own scrolling and
    /// no ratio applies at all.
    pub fn max_height(&self, available: f32) -> Option<f32> {
        if self.is_scroll_controlled {
            None
        } else {
            Some(available * self.scroll_control_disabled_max_height_ratio)
        }
    }

    /// Whether a downward flick at this speed dismisses the sheet.
    pub fn flick_dismisses(velocity: f32) -> bool {
        velocity >= BOTTOM_SHEET_MIN_FLING_VELOCITY
    }

    /// Below the fling speed, position decides -- the same three-way shape as
    /// the drawer.
    pub fn drag_dismisses(progress: f32) -> bool {
        progress < BOTTOM_SHEET_CLOSE_THRESHOLD
    }
}

impl Default for ModalBottomSheetRoute {
    fn default() -> Self {
        ModalBottomSheetRoute::new()
    }
}

/// Upstream `DialogRoute`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DialogRoute {
    pub barrier_dismissible: bool,
    pub use_safe_area: bool,
    pub transition_ms: u32,
}

impl DialogRoute {
    pub fn new() -> DialogRoute {
        DialogRoute {
            barrier_dismissible: true,
            use_safe_area: true,
            transition_ms: DIALOG_TRANSITION_MS,
        }
    }

    /// Upstream wraps the dialog in `Semantics(hitTestBehavior: opaque)` with
    /// the comment *"Prevent clicks inside the dialog from passing through to
    /// the barrier"*.
    ///
    /// Note what it is: the **semantics** hit test, not the pointer one. A
    /// pointer tap already stops at the dialog's own material. What this stops
    /// is an assistive-technology activation landing inside the dialog and
    /// reaching the dismissable barrier behind it -- which would close the very
    /// thing the reader was trying to use.
    pub fn semantics_hit_test_is_opaque() -> bool {
        true
    }

    /// Upstream's curves: `easeOut` **both ways**.
    ///
    /// Most transitions in this framework flip the curve on the way back. A
    /// dialog does not: it decelerates in and decelerates out, so neither
    /// direction accelerates away from the reader. It is an interruption, and
    /// an interruption that leaves in a hurry reads as a mistake.
    pub fn curve_is_symmetric() -> bool {
        true
    }

    /// Upstream's `_setAnimation` rebuilds the curved animation only when the
    /// parent actually changed, disposing the old one. The same "not for the
    /// same answer" guard that turns up all over this framework.
    pub fn rebuilds_curve(old_parent: Option<u64>, new_parent: u64) -> bool {
        old_parent != Some(new_parent)
    }
}

impl Default for DialogRoute {
    fn default() -> Self {
        DialogRoute::new()
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_page_route_only_animates_alongside_another_page_route() {
        // A dialog over a page must not make the page slide, and a page under
        // a dialog must not make the dialog move.
        let route = PageRoute::new();
        assert!(route.can_transition_with_page_route(true));
        assert!(!route.can_transition_with_page_route(false));
    }

    #[test]
    fn a_page_route_is_always_opaque_because_it_fills_the_screen() {
        assert!(PageRoute::new().opaque());
        assert!(PageRoute::new().with_fullscreen_dialog(true).opaque());
    }

    #[test]
    fn a_fullscreen_dialog_is_not_dismissible_by_back_swipe() {
        // It comes up from the bottom, so there is no edge a swipe would start
        // from that means anything.
        assert!(PageRoute::new().pop_gesture_enabled());
        assert!(
            !PageRoute::new()
                .with_fullscreen_dialog(true)
                .pop_gesture_enabled()
        );
    }

    #[test]
    fn a_platform_with_no_back_swipe_disables_it_for_every_page() {
        let mut route = PageRoute::new();
        route.platform_pop_gesture_available = false;
        assert!(!route.pop_gesture_enabled());
    }

    #[test]
    fn a_route_builder_with_no_transition_makes_the_page_appear() {
        // A deliberate default for a class whose point is one-off routes: a
        // caller who wanted a transition would have said which.
        let builder = PageRouteBuilder::new();
        assert!(!builder.has_transitions_builder);
        assert_eq!(PageRouteBuilder::default_transition(7), 7, "unchanged");
    }

    #[test]
    fn a_page_has_no_outside_to_tap_so_its_barrier_is_not_dismissible() {
        // Unlike a dialog's, which is.
        let builder = PageRouteBuilder::new();
        assert!(!builder.barrier_dismissible);
        assert!(RawDialogRoute::new().popup.modal.barrier_dismissible);
    }

    #[test]
    fn the_route_builder_uses_the_same_duration_in_both_directions() {
        let builder = PageRouteBuilder::new();
        assert_eq!(builder.transition_micros, 300_000);
        assert_eq!(builder.reverse_transition_micros, 300_000);
        assert!(builder.maintain_state && builder.opaque);
    }

    #[test]
    fn a_will_pop_scope_registers_when_dependencies_resolve_and_not_before() {
        // The enclosing route is found through the context, and the context
        // has no ancestors to search until dependencies are being resolved.
        let mut scope = WillPopScope::new(true);
        assert!(!scope.is_registered());

        scope.did_change_dependencies(true);
        assert!(scope.is_registered());
        assert_eq!(scope.registrations(), 1);
        assert_eq!(scope.removals(), 0);
    }

    #[test]
    fn moving_to_a_different_route_takes_the_callback_off_the_old_one_first() {
        let mut scope = WillPopScope::new(true);
        scope.did_change_dependencies(true);
        scope.did_change_dependencies(true);
        assert_eq!(scope.removals(), 1, "off the old route");
        assert_eq!(scope.registrations(), 2, "and onto the new");
    }

    #[test]
    fn a_scope_with_no_callback_is_inert_rather_than_broken() {
        // A caller can leave one in the tree and turn it off by passing null.
        let mut scope = WillPopScope::new(false);
        scope.did_change_dependencies(true);
        assert!(!scope.is_registered());
        assert_eq!(scope.registrations(), 0);
    }

    #[test]
    fn a_rebuild_with_the_same_callback_does_not_churn_the_routes_list() {
        let mut scope = WillPopScope::new(true);
        scope.did_change_dependencies(true);
        let before = (scope.registrations(), scope.removals());

        scope.did_update_widget(true, false);
        assert_eq!((scope.registrations(), scope.removals()), before);

        scope.did_update_widget(true, true);
        assert_eq!(scope.registrations(), before.0 + 1);
        assert_eq!(scope.removals(), before.1 + 1);
    }

    #[test]
    fn turning_the_callback_off_removes_it_without_adding_another() {
        let mut scope = WillPopScope::new(true);
        scope.did_change_dependencies(true);
        scope.did_update_widget(false, true);
        assert!(!scope.is_registered());
        assert_eq!(scope.removals(), 1);
        assert_eq!(scope.registrations(), 1, "no second registration");
    }

    #[test]
    fn disposing_takes_the_callback_off_the_route() {
        // Or the route would call into a widget that no longer exists.
        let mut scope = WillPopScope::new(true);
        scope.did_change_dependencies(true);
        scope.dispose();
        assert!(!scope.is_registered());
        assert_eq!(scope.removals(), 1);
    }

    use super::*;

    // -- Local history ----------------------------------------------------

    #[test]
    fn a_back_press_peels_one_local_entry_and_the_route_stays() {
        // Which is what lets a page put a bottom sheet and a drawer on itself
        // without the first back press closing all three.
        let mut route = LocalHistoryRoute::new();
        route.add_local_history_entry(LocalHistoryEntry::new(1));
        route.add_local_history_entry(LocalHistoryEntry::new(2));

        assert!(!route.did_pop(), "the route did not pop, an entry did");
        assert_eq!(route.entries().len(), 1);
        assert!(!route.did_pop());
        assert_eq!(route.entries().len(), 0);
        assert!(route.did_pop(), "and now the route itself goes");
    }

    #[test]
    fn an_entry_that_does_not_imply_dismissal_leaves_the_arrow_alone() {
        // A route that added an entry only to intercept the back gesture has
        // nothing for an arrow to undo, and an arrow that undoes something
        // invisible reads as a broken arrow.
        let mut route = LocalHistoryRoute::new();
        route.add_local_history_entry(
            LocalHistoryEntry::new(1).with_implies_app_bar_dismissal(false),
        );
        assert!(
            route.will_handle_pop_internally(),
            "the press is still ours"
        );
        assert!(!route.implies_app_bar_dismissal(), "but no arrow");

        route.add_local_history_entry(LocalHistoryEntry::new(2));
        assert!(route.implies_app_bar_dismissal());
    }

    #[test]
    fn the_state_change_fires_on_the_edges_and_not_on_every_entry() {
        // Or a page would rebuild once per sheet for a change nothing can see.
        let mut route = LocalHistoryRoute::new();
        route.add_local_history_entry(LocalHistoryEntry::new(1));
        assert_eq!(route.state_changes(), 1, "empty to not empty");

        route.add_local_history_entry(LocalHistoryEntry::new(2));
        route.add_local_history_entry(LocalHistoryEntry::new(3));
        assert_eq!(route.state_changes(), 1, "and nothing since");

        route.remove_local_history_entry(2, false);
        assert_eq!(route.state_changes(), 1, "still not empty, still an arrow");
        route.remove_local_history_entry(3, false);
        route.remove_local_history_entry(1, false);
        assert_eq!(route.state_changes(), 2, "back to empty");
    }

    #[test]
    fn an_entry_is_removed_by_name_and_not_off_the_top() {
        // A drawer closing while a sheet is open takes its own entry out of
        // the middle; taking the top one would close the sheet instead.
        let mut route = LocalHistoryRoute::new();
        route.add_local_history_entry(LocalHistoryEntry::new(1));
        route.add_local_history_entry(LocalHistoryEntry::new(2));
        route.add_local_history_entry(LocalHistoryEntry::new(3));

        let removed = route.remove_local_history_entry(2, false).unwrap();
        assert!(removed.was_removed() && !removed.is_owned());
        let ids: Vec<u64> = route.entries().iter().map(|entry| entry.id).collect();
        assert_eq!(ids, vec![1, 3]);

        assert!(
            route.remove_local_history_entry(9, false).is_none(),
            "and an entry that is not there is not an entry"
        );
    }

    #[test]
    fn a_removal_during_a_locked_tree_waits_for_the_frame_to_end() {
        let mut route = LocalHistoryRoute::new();
        route.add_local_history_entry(LocalHistoryEntry::new(1));
        route.remove_local_history_entry(1, true);
        assert_eq!(route.state_changes(), 1, "the add, and nothing since");
        assert_eq!(route.deferred_state_changes(), 1);

        route.flush_deferred_state_changes(true);
        assert_eq!(route.state_changes(), 2);
        assert_eq!(route.deferred_state_changes(), 0);
    }

    #[test]
    fn a_route_that_died_before_the_frame_ended_is_never_told() {
        // Telling a dead route to rebuild is worse than not telling it, which
        // is why upstream's post-frame callback is guarded by isActive.
        let mut route = LocalHistoryRoute::new();
        route.add_local_history_entry(LocalHistoryEntry::new(1));
        route.remove_local_history_entry(1, true);
        route.flush_deferred_state_changes(false);
        assert_eq!(route.state_changes(), 1, "only the add");
        assert_eq!(route.deferred_state_changes(), 0, "and it was dropped");
    }

    // -- Who answers the back press ---------------------------------------

    #[test]
    fn the_bottom_of_the_stack_hands_the_press_back_to_the_platform() {
        // Which is what closes the application: a route with nowhere to pop to
        // should not swallow the press.
        assert_eq!(
            route_pop_disposition(true, true),
            RoutePopDisposition::Bubble
        );
        assert_eq!(route_pop_disposition(false, true), RoutePopDisposition::Pop);
        assert_eq!(
            route_pop_disposition(true, false),
            RoutePopDisposition::DoNotPop,
            "a page that says no is asked before anything else"
        );
    }

    #[test]
    fn local_history_stops_the_first_route_from_closing_the_application() {
        // A back press on the application's only page should close the sheet
        // on it, not the application.
        let mut route = LocalHistoryRoute::new();
        assert_eq!(
            route.pop_disposition(true, true),
            RoutePopDisposition::Bubble
        );

        route.add_local_history_entry(LocalHistoryEntry::new(1));
        assert_eq!(
            route.pop_disposition(true, true),
            RoutePopDisposition::Pop,
            "there is something to undo now"
        );
    }

    #[test]
    fn a_single_veto_beats_everything_below_it() {
        // Two forms on a page: one with unsaved work, one without. The one
        // without must not spend the other's answer.
        let mut route = ModalRoute::new();
        route
            .local_history
            .add_local_history_entry(LocalHistoryEntry::new(1));
        assert_eq!(route.pop_disposition(), RoutePopDisposition::Pop);

        route.register_pop_entry(PopEntry::new(10, true));
        route.register_pop_entry(PopEntry::new(11, false));
        assert_eq!(
            route.pop_disposition(),
            RoutePopDisposition::DoNotPop,
            "asked before local history, and one no is enough"
        );

        route.unregister_pop_entry(11);
        assert_eq!(route.pop_disposition(), RoutePopDisposition::Pop);
    }

    #[test]
    fn every_entry_hears_about_the_pop_including_the_one_that_refused() {
        // The didPop argument is the point: a form shows "you have unsaved
        // changes" exactly when it was the one that kept the page.
        let mut route = ModalRoute::new();
        route.register_pop_entry(PopEntry::new(10, true));
        route.register_pop_entry(PopEntry::new(11, false));

        route.pop_invoked(false);
        for entry in route.pop_entries() {
            assert_eq!(entry.invocations(), &[false]);
        }

        route.pop_invoked(true);
        assert_eq!(route.pop_entries()[0].invocations(), &[false, true]);
    }

    // -- Overlay entries and the transition -------------------------------

    #[test]
    fn a_plain_overlay_route_takes_its_entries_out_the_moment_it_pops() {
        let mut route = OverlayRoute::new();
        route.install(vec![1, 2]);
        assert!(route.is_installed() && !route.is_finalized());

        let finished = route.finished_when_popped();
        assert!(route.did_pop(finished));
        assert!(route.is_finalized());
    }

    #[test]
    fn a_route_still_on_screen_keeps_its_entries_through_the_pop() {
        // Or the reader would watch the page vanish instead of slide away.
        let mut route = TransitionRoute::default();
        route.overlay.install(vec![1]);
        route.set_animation(1.0);

        assert!(!route.finished_when_popped());
        route.did_pop();
        assert!(
            !route.overlay.is_finalized(),
            "the animation still has to run"
        );
    }

    #[test]
    fn a_route_swiped_all_the_way_off_is_finished_the_moment_it_pops() {
        // The iOS back-swipe drags a route to dismissed while it is still
        // current and pops it only afterwards. By then there is nothing left
        // to animate, and upstream's note is that without this the route would
        // never be disposed at all.
        let mut route = TransitionRoute::default();
        route.overlay.install(vec![1]);
        route.set_animation(1.0);
        route.handle_start_back_gesture(0.5);
        route.handle_update_back_gesture_progress(1.0);
        route.handle_commit_back_gesture();

        assert!(route.is_dismissed() && !route.pop_gesture_in_progress());
        assert!(route.finished_when_popped());
        route.did_pop();
        assert!(route.overlay.is_finalized());
    }

    #[test]
    fn a_second_pop_does_not_finalize_a_route_that_already_was() {
        // Which is the whole of upstream's _popFinalized.
        let mut route = TransitionRoute::default();
        route.overlay.install(vec![1]);
        route.did_pop();
        assert!(route.overlay.is_finalized());
        assert!(
            !route.finished_when_popped(),
            "dismissed, but it has been finalized once already"
        );
    }

    #[test]
    fn a_cancelled_back_gesture_puts_the_route_back() {
        // The reader changed their mind mid-swipe, and the page they were
        // leaving is the one they meant to keep.
        let mut route = TransitionRoute::default();
        route.set_animation(1.0);
        route.handle_start_back_gesture(0.3);
        assert!(route.pop_gesture_in_progress());
        assert!((route.animation() - 0.7).abs() < 1e-6);

        route.handle_cancel_back_gesture();
        assert_eq!(route.animation(), 1.0);
        assert!(!route.pop_gesture_in_progress());
    }

    #[test]
    fn leaving_can_be_quicker_than_arriving() {
        // The reader has already decided, so the exit has less to say.
        let route = TransitionRoute::new(300_000);
        assert_eq!(route.reverse_transition_duration_micros, 300_000);

        let quicker = TransitionRoute::new(300_000).with_reverse_duration(150_000);
        assert_eq!(quicker.transition_duration_micros, 300_000);
        assert_eq!(quicker.reverse_transition_duration_micros, 150_000);
    }

    // -- Barriers and the modal family ------------------------------------

    #[test]
    fn a_barrier_the_reader_can_dismiss_has_to_have_a_name() {
        // Because a barrier that can be tapped is a control, and a screen
        // reader has nothing to announce for a control with no name.
        assert!(RouteBarrierDetails::new(1.0, false).is_valid());
        assert!(!RouteBarrierDetails::new(1.0, true).is_valid());
        assert!(
            RouteBarrierDetails::new(1.0, true)
                .with_label("Dismiss")
                .is_valid()
        );
    }

    #[test]
    fn a_popup_never_blanks_what_is_behind_it() {
        // A menu that dropped the page under it would have to rebuild that
        // page on every dismissal, and would look wrong doing it.
        let popup = PopupRoute::new();
        assert!(!popup.opaque());
        assert!(!popup.modal.transition.opaque);
        assert!(popup.maintain_state());

        assert!(
            ModalRoute::new().transition.opaque,
            "where a plain modal route does obscure what it covers"
        );
    }

    #[test]
    fn a_dialog_may_be_walked_away_from_and_a_modal_route_may_not() {
        // A dialog is a question; tapping outside it is how the reader
        // declines to answer.
        let dialog = RawDialogRoute::new();
        assert!(dialog.popup.modal.barrier_dismissible);
        assert!(!ModalRoute::new().barrier_dismissible);

        assert_eq!(
            dialog.popup.modal.barrier_color,
            Some(Color(0x8000_0000)),
            "half-transparent black: the page behind stays readable"
        );
    }

    #[test]
    fn a_dialog_has_less_distance_to_cover_than_a_page() {
        // It appears over what the reader is already looking at rather than
        // replacing it.
        assert_eq!(RawDialogRoute::new().transition_duration_micros(), 200_000);
        assert_eq!(
            TransitionRoute::default().transition_duration_micros,
            300_000
        );
    }

    #[test]
    fn the_barrier_details_carry_the_animation_that_is_driving_them() {
        let mut route = ModalRoute::new()
            .with_barrier_dismissible(true)
            .with_barrier_label("Close")
            .with_barrier_color(Color(0x8000_0000));
        route.transition.set_animation(0.4);

        let details = route.barrier_details();
        assert_eq!(
            details.animation, 0.4,
            "so the scrim fades in with the route"
        );
        assert!(details.is_valid());
    }

    // -- The observer ------------------------------------------------------

    #[test]
    fn a_subscriber_arriving_late_is_told_it_is_on_screen() {
        // It never saw the push that put it there, and it still needs to know.
        let mut observer = RouteObserver::new();
        observer.subscribe(1, 100);
        assert_eq!(observer.delivered(), &[(1, RouteAwareEvent::DidPush)]);

        observer.subscribe(1, 100);
        assert_eq!(
            observer.delivered().len(),
            1,
            "a rebuild is not a navigation"
        );
    }

    #[test]
    fn what_is_revealed_refreshes_before_what_is_leaving_says_goodbye() {
        let mut observer = RouteObserver::new();
        observer.subscribe(1, 100);
        observer.subscribe(2, 200);
        let pushes = observer.delivered().len();

        observer.did_pop(200, Some(100));
        assert_eq!(
            &observer.delivered()[pushes..],
            &[
                (1, RouteAwareEvent::DidPopNext),
                (2, RouteAwareEvent::DidPop),
            ]
        );
    }

    #[test]
    fn a_push_is_announced_only_to_the_route_being_covered() {
        // The pushed route's own subscribers live in a subtree that does not
        // exist yet, which is the gap subscribe closes when they arrive.
        let mut observer = RouteObserver::new();
        observer.subscribe(1, 100);
        observer.subscribe(2, 200);
        let pushes = observer.delivered().len();

        observer.did_push(200, Some(100));
        assert_eq!(
            &observer.delivered()[pushes..],
            &[(1, RouteAwareEvent::DidPushNext)]
        );

        observer.did_push(100, None);
        assert_eq!(
            observer.delivered().len(),
            pushes + 1,
            "the first route covers nothing"
        );
    }

    #[test]
    fn unsubscribing_walks_every_route_because_nobody_said_which() {
        let mut observer = RouteObserver::new();
        observer.subscribe(1, 100);
        observer.subscribe(1, 200);
        observer.subscribe(2, 200);
        assert!(observer.is_observing_route(100) && observer.is_observing_route(200));

        observer.unsubscribe(1);
        assert!(
            !observer.is_observing_route(100),
            "a route with nobody listening is dropped rather than kept empty"
        );
        assert!(
            observer.is_observing_route(200),
            "where this one still has 2"
        );

        let delivered = observer.delivered().len();
        observer.did_pop(200, None);
        assert_eq!(
            &observer.delivered()[delivered..],
            &[(2, RouteAwareEvent::DidPop)],
            "and 1 hears nothing"
        );
    }
    // -- Material's two modal routes -------------------------------------------

    #[test]
    fn arriving_is_deliberate_and_leaving_gets_out_of_the_way() {
        assert_eq!(BOTTOM_SHEET_ENTER_MS, 250);
        assert_eq!(BOTTOM_SHEET_EXIT_MS, 200);
        assert!(BOTTOM_SHEET_EXIT_MS < BOTTOM_SHEET_ENTER_MS);
    }

    #[test]
    fn a_dialog_interrupts_and_a_sheet_arrives() {
        assert!(DIALOG_TRANSITION_MS < BOTTOM_SHEET_ENTER_MS);
        assert_eq!(DIALOG_TRANSITION_MS, 150);
    }

    #[test]
    fn a_sheet_takes_nearly_twice_the_speed_of_a_drawer_to_fling_away() {
        // A bottom sheet usually holds something scrollable, and a vertical
        // flick inside one is far more often a scroll than a dismissal.
        assert_eq!(BOTTOM_SHEET_MIN_FLING_VELOCITY, 700.0);
        assert!(
            BOTTOM_SHEET_MIN_FLING_VELOCITY > 365.0 * 1.9,
            "nearly twice the drawer's 365, though not exactly: 700 is 1.92 of it"
        );

        assert!(ModalBottomSheetRoute::flick_dismisses(701.0));
        assert!(!ModalBottomSheetRoute::flick_dismisses(699.0));
    }

    #[test]
    fn below_the_fling_speed_the_position_decides_as_it_does_for_a_drawer() {
        assert!(ModalBottomSheetRoute::drag_dismisses(0.49));
        assert!(!ModalBottomSheetRoute::drag_dismisses(0.5));
    }

    #[test]
    fn a_controller_naming_only_one_duration_is_taken_to_mean_it_both_ways() {
        // Skipping to the default instead would hand the caller an asymmetry
        // they never asked for: their 400 in and the framework's 200 out.
        let mut route = ModalBottomSheetRoute::new();
        route.controller_duration_ms = Some(400);
        assert_eq!(route.transition_duration_ms(), 400);
        assert_eq!(route.reverse_transition_duration_ms(), 400);

        route.controller_reverse_duration_ms = Some(120);
        assert_eq!(
            route.reverse_transition_duration_ms(),
            120,
            "and a stated reverse wins over the borrowed one"
        );
    }

    #[test]
    fn with_no_controller_the_frameworks_own_pair_is_used() {
        let route = ModalBottomSheetRoute::new();
        assert_eq!(route.transition_duration_ms(), BOTTOM_SHEET_ENTER_MS);
        assert_eq!(route.reverse_transition_duration_ms(), BOTTOM_SHEET_EXIT_MS);
    }

    #[test]
    fn a_scroll_controlled_sheet_has_no_height_ratio_at_all() {
        let plain = ModalBottomSheetRoute::new();
        assert_eq!(plain.max_height(1600.0), Some(900.0), "nine sixteenths");

        let mut scrollable = ModalBottomSheetRoute::new();
        scrollable.is_scroll_controlled = true;
        assert_eq!(
            scrollable.max_height(1600.0),
            None,
            "no ratio applies, not a larger one"
        );
    }

    #[test]
    fn one_concept_named_twice_for_the_sheet_and_for_its_route() {
        let mut route = ModalBottomSheetRoute::new();
        assert!(route.barrier_dismissible());
        route.is_dismissible = false;
        assert!(!route.barrier_dismissible());
    }

    #[test]
    fn the_opaque_hit_test_is_the_semantics_one_and_not_the_pointer_one() {
        // A pointer tap already stops at the dialog's own material. What this
        // stops is an assistive-technology activation inside the dialog
        // reaching the dismissable barrier behind it -- closing the very thing
        // the reader was trying to use.
        assert!(DialogRoute::semantics_hit_test_is_opaque());
    }

    #[test]
    fn a_dialog_decelerates_in_and_decelerates_out() {
        // Most transitions flip the curve on the way back. An interruption that
        // leaves in a hurry reads as a mistake.
        assert!(DialogRoute::curve_is_symmetric());
    }

    #[test]
    fn the_curve_is_rebuilt_only_when_its_parent_actually_changed() {
        assert!(DialogRoute::rebuilds_curve(None, 1));
        assert!(DialogRoute::rebuilds_curve(Some(1), 2));
        assert!(!DialogRoute::rebuilds_curve(Some(1), 1));
    }

    #[test]
    fn a_dialog_and_a_sheet_dim_the_page_by_the_same_amount() {
        assert_eq!(MODAL_BARRIER_ALPHA, 0x8A);
    }

    // -- Where a route stands in the history ---------------------------------

    use super::{HistoryEntry, RouteLifecycle, RoutePosition};

    /// Bottom first, as `_history` is.
    fn history(entries: &[(u64, RouteLifecycle)]) -> RoutePosition {
        RoutePosition::new(
            entries
                .iter()
                .map(|(route, state)| HistoryEntry::new(*route, *state))
                .collect(),
        )
    }

    #[test]
    fn present_reaches_three_variants_past_the_comment_that_says_it_does_not() {
        // `isPresent` is `add..=remove`, and the `// routes that are not
        // present:` comment sits above `pop` -- three variants inside the
        // range. A route being popped, completing, or being removed is still
        // present, and that is not a slip: it is still on screen and still the
        // answer to "which route is current".
        for state in [
            RouteLifecycle::Add,
            RouteLifecycle::Adding,
            RouteLifecycle::Push,
            RouteLifecycle::PushReplace,
            RouteLifecycle::Pushing,
            RouteLifecycle::Replace,
            RouteLifecycle::Idle,
            RouteLifecycle::Pop,
            RouteLifecycle::Complete,
            RouteLifecycle::Remove,
        ] {
            assert!(state.is_present(), "{state:?} is present");
        }
        for state in [
            RouteLifecycle::Staging,
            RouteLifecycle::Popping,
            RouteLifecycle::Removing,
            RouteLifecycle::Dispose,
            RouteLifecycle::Disposing,
            RouteLifecycle::Disposed,
        ] {
            assert!(!state.is_present(), "{state:?} is not");
        }

        // The three questions that are *not* the same range, each differing at
        // one end -- which is the reason they are three questions.
        assert!(
            RouteLifecycle::Pop.is_present() && !RouteLifecycle::Pop.will_be_present(),
            "a popped route is present now and will not be"
        );
        assert!(
            RouteLifecycle::Staging.is_present_for_restoration()
                && !RouteLifecycle::Staging.is_present(),
            "a staged route is not present, and is still state to restore"
        );
        assert!(
            !RouteLifecycle::Add.suitable_for_announcement(),
            "a route only being added is not announced"
        );
        assert!(
            RouteLifecycle::Removing.suitable_for_announcement(),
            "and one still animating out is"
        );
    }

    #[test]
    fn the_current_route_is_the_last_one_that_is_present() {
        // Not the last one in the list: a route that is `popping` is still in
        // the history and is no longer present, so the route below it is
        // current while the animation runs.
        let stack = history(&[
            (1, RouteLifecycle::Idle),
            (2, RouteLifecycle::Idle),
            (3, RouteLifecycle::Popping),
        ]);
        assert!(stack.is_current(2, true), "the one below the leaving route");
        assert!(!stack.is_current(3, true));
        assert!(!stack.is_current(1, true));
    }

    #[test]
    fn the_first_route_is_the_bottom_one_that_is_present() {
        let stack = history(&[
            (1, RouteLifecycle::Removing),
            (2, RouteLifecycle::Idle),
            (3, RouteLifecycle::Idle),
        ]);
        assert!(stack.is_first(2, true), "the bottom-most present entry");
        assert!(!stack.is_first(1, true), "a route on its way out is not");

        // With one route on the navigator, it is both first and current --
        // upstream says so in `isFirst`'s own documentation.
        let alone = history(&[(9, RouteLifecycle::Idle)]);
        assert!(alone.is_first(9, true) && alone.is_current(9, true));
    }

    #[test]
    fn nothing_is_anywhere_until_it_is_installed() {
        // Three of the four begin `if (!_installed) return false`. A route
        // that has never been given to a navigator is not the current route,
        // not the first, and has nothing below it -- it is not anywhere, and
        // `false` is how upstream says that without a third value.
        let stack = history(&[(1, RouteLifecycle::Idle)]);
        assert!(!stack.is_current(1, false));
        assert!(!stack.is_first(1, false));
        assert!(!stack.has_active_route_below(1, false));
    }

    #[test]
    fn a_route_is_active_by_its_first_entry_and_no_other() {
        // `isActive` takes the **first** entry for this route and asks whether
        // that one is present -- not "any entry of it". The two differ exactly
        // when a route is in the history twice.
        let twice = history(&[
            (7, RouteLifecycle::Removing),
            (8, RouteLifecycle::Idle),
            (7, RouteLifecycle::Idle),
        ]);
        assert!(
            !twice.is_active(7),
            "the first entry for it is on its way out, so it is not active"
        );
        assert!(twice.is_active(8));
        assert!(!twice.is_active(99), "a route that is not there at all");
    }

    #[test]
    fn what_is_below_is_below_this_route_and_not_merely_elsewhere() {
        // The walk stops at this route's own entry, which is what makes the
        // question "below" rather than "anywhere in the history".
        let stack = history(&[
            (1, RouteLifecycle::Idle),
            (2, RouteLifecycle::Idle),
            (3, RouteLifecycle::Idle),
        ]);
        assert!(
            !stack.has_active_route_below(1, true),
            "nothing under the bottom"
        );
        assert!(stack.has_active_route_below(2, true));
        assert!(stack.has_active_route_below(3, true));

        // And only *present* entries count as being below.
        let leaving = history(&[(1, RouteLifecycle::Popping), (2, RouteLifecycle::Idle)]);
        assert!(
            !leaving.has_active_route_below(2, true),
            "the only thing under it is on its way out"
        );
    }

    // -- When the stack around a route changes -------------------------------

    #[test]
    fn a_replacement_opens_from_where_the_old_route_had_got_to() {
        // `TransitionRoute.didReplace`: `_controller!.value = oldRoute
        // ._controller!.value`. A route replacing a half-open one at 0.4 opens
        // from 0.4 -- starting again would replay an entrance the reader has
        // already watched most of, and the two screens would cross twice.
        let mut arriving = TransitionRoute::new(300_000);
        assert_eq!(arriving.animation(), 0.0);
        arriving.did_replace(Some(0.4));
        assert_eq!(arriving.animation(), 0.4);

        // And a route replacing something that was not a transition route has
        // nothing to take over, so it keeps its own.
        let mut fresh = TransitionRoute::new(300_000);
        fresh.set_animation(0.25);
        fresh.did_replace(None);
        assert_eq!(fresh.animation(), 0.25);
    }

    #[test]
    fn a_route_moves_out_of_the_way_only_for_one_it_can_transition_to() {
        // `didChangeNext` is `_updateSecondaryAnimation(nextRoute)`, and what
        // it settles is whether this route animates as the next one covers it.
        let covered = TransitionRoute::new(300_000);
        let arriving = TransitionRoute::new(300_000);
        assert!(covered.secondary_animates_for(Some(&arriving)));
        assert!(
            !covered.secondary_animates_for(None),
            "with nothing above it there is nothing to move for"
        );
    }

    #[test]
    fn a_route_above_that_delegates_the_same_transition_hands_down_nothing() {
        // The third of `ModalRoute.didChangeNext`'s three conditions:
        // `nextRoute.delegatedTransition != delegatedTransition`. A route that
        // already has this transition would otherwise play it twice over one
        // screen -- once because it has it, once because it was given it.
        let mut below = ModalRoute::new();
        below.delegated_transition = Some(7);

        let mut above = ModalRoute::new();
        above.delegated_transition = Some(7);
        below.did_change_next(Some(&above), false);
        assert_eq!(
            below.received_transition, None,
            "the same transition, so nothing is handed down"
        );

        above.delegated_transition = Some(9);
        below.did_change_next(Some(&above), false);
        assert_eq!(below.received_transition, Some(9), "a different one is");

        // And nothing above at all clears it again -- upstream's `else` arm.
        below.did_change_next(None, false);
        assert_eq!(below.received_transition, None);
    }

    #[test]
    fn the_neighbours_changing_rebuilds_the_barrier_unless_the_tree_is_locked() {
        // `changedInternalState` marks the barrier dirty, and upstream guards
        // exactly that with the scheduler phase: nothing may be marked during
        // a build. `maintainState` is pushed into the scope either way,
        // because it is a value being assigned rather than a rebuild asked
        // for -- which is why the guard sits inside the method and not around
        // the call.
        let mut route = ModalRoute::new();
        route.maintain_state = false;

        route.changed_internal_state(true);
        assert_eq!(route.barrier_rebuilds(), 0, "the tree was locked");
        assert!(
            !route.scope_maintains_state(),
            "and the scope was told anyway"
        );

        route.changed_internal_state(false);
        assert_eq!(route.barrier_rebuilds(), 1);

        // `didChangePrevious` is that method and nothing else: what is below
        // changes nothing this route draws, but the barrier says "dismiss to
        // the thing behind".
        route.did_change_previous(false);
        assert_eq!(route.barrier_rebuilds(), 2);
    }

    #[test]
    fn an_external_change_rebuilds_the_page_as_well() {
        // The difference between the two: `changedExternalState` also forces
        // the page to rebuild, because the navigator itself changed -- a new
        // `MaterialApp` above it, say -- so what the page built from that
        // state is out of date. Marking only the barrier would leave the old
        // page on screen.
        let mut route = ModalRoute::new();
        route.changed_internal_state(false);
        assert_eq!(
            (route.barrier_rebuilds(), route.page_rebuilds()),
            (1, 0),
            "an internal change leaves the page alone"
        );

        route.changed_external_state();
        assert_eq!((route.barrier_rebuilds(), route.page_rebuilds()), (2, 1));
    }

    // -- What a route hands back when it goes --------------------------------

    use super::RouteCompletion;

    #[test]
    fn a_route_popped_with_no_result_hands_back_its_own() {
        // `_popCompleter.complete(result ?? currentResult)`, and the `??` is
        // the whole of it: a dialog dismissed by tapping the barrier answers
        // with whatever was selected when it closed, not with nothing.
        let mut chooser = RouteCompletion::new().with_current_result("the third one");
        assert_eq!(chooser.popped(), None, "still on the navigator");

        chooser.did_complete(None);
        assert_eq!(chooser.popped(), Some(Some("the third one")));
    }

    #[test]
    fn a_result_that_was_given_wins_over_the_fallback() {
        let mut chooser = RouteCompletion::new().with_current_result("the third one");
        chooser.did_complete(Some("the second one".to_string()));
        assert_eq!(chooser.popped(), Some(Some("the second one")));
    }

    #[test]
    fn a_route_with_neither_hands_back_nothing_and_still_finishes() {
        // The two `None`s are different questions, which is why they are
        // nested: "has it finished" and "did it hand anything back". A route
        // that answered nothing has still answered.
        let mut plain = RouteCompletion::new();
        assert!(!plain.is_completed());
        plain.did_complete(None);
        assert!(plain.is_completed());
        assert_eq!(plain.popped(), Some(None), "finished, with nothing to say");
    }

    #[test]
    fn the_first_completion_is_the_one_that_counts() {
        // Upstream's `Completer` throws on being completed twice, and the
        // navigator has two callers -- `didPop` and `pushReplacement` -- so
        // "the first one wins" is a rule rather than an accident. A second
        // call is declined here rather than fatal, the same stance
        // `ModalHandle::dismiss` takes.
        let mut route = RouteCompletion::new().with_current_result("fallback");
        route.did_complete(Some("first".to_string()));
        route.did_complete(Some("second".to_string()));
        assert_eq!(route.popped(), Some(Some("first")));

        // Including when the first one was the fallback, and the second one
        // brought a real value: it is still too late.
        let mut late = RouteCompletion::new().with_current_result("fallback");
        late.did_complete(None);
        late.did_complete(Some("too late".to_string()));
        assert_eq!(late.popped(), Some(Some("fallback")));
    }

    // -- The entry that carries the pop -------------------------------------

    #[test]
    fn a_pop_hands_over_what_it_was_carrying() {
        // `handlePop` calls `didPop(pendingResult)` and then clears it. The
        // value arrives with the request and waits on the entry, because a
        // route can be marked for popping by the transition delegate long
        // before anything calls `didPop`.
        let mut entry = HistoryEntry::new(1, RouteLifecycle::Idle).with_pending_result("chosen");
        assert!(entry.handle_pop(|_| true));
        assert_eq!(entry.completion.popped(), Some(Some("chosen")));
        assert_eq!(
            entry.pending_result, None,
            "and it cannot be handed over twice"
        );
        assert_eq!(entry.state, RouteLifecycle::Popping);
    }

    #[test]
    fn a_pop_with_nothing_to_carry_falls_back_to_the_route_s_own() {
        // The `??` from the other side: the entry has no pending result, so
        // what the future completes with is the route's `currentResult`.
        let mut entry = HistoryEntry::new(1, RouteLifecycle::Idle).with_current_result("selected");
        assert!(entry.handle_pop(|_| true));
        assert_eq!(entry.completion.popped(), Some(Some("selected")));
    }

    #[test]
    fn the_route_is_asked_while_the_entry_is_already_popping() {
        // The state goes to `popping` **before** the route is asked, not after
        // it agrees. A route consuming the pop rebuilds while it does so, and
        // what it reads about itself in that moment is `popping` -- so the
        // order is observable from inside `didPop` and nowhere else, which is
        // why the route is asked through a closure here.
        let mut entry = HistoryEntry::new(1, RouteLifecycle::Idle);
        let mut seen = None;
        entry.handle_pop(|state| {
            seen = Some(state);
            true
        });
        assert_eq!(seen, Some(RouteLifecycle::Popping));
    }

    #[test]
    fn an_entry_with_nothing_happening_to_it_is_idle() {
        // `_RouteLifecycle.idle` is upstream's *"route is being harmless"*,
        // and it is what an entry is by default -- not `staging`, which means
        // the transition delegate has not ruled on it yet and is a state an
        // entry is put into rather than born in.
        assert_eq!(RouteLifecycle::default(), RouteLifecycle::Idle);
        assert_eq!(HistoryEntry::default().state, RouteLifecycle::Idle);
    }

    #[test]
    fn a_route_that_consumed_the_pop_puts_the_state_back() {
        // The state goes to `popping` **before** the route is asked and comes
        // back to `idle` when the route refuses -- a route with local history
        // pops its own entry instead, and what it reads about itself while it
        // rebuilds is `popping`. Setting the state only on success would look
        // equivalent and would not be.
        let mut entry = HistoryEntry::new(1, RouteLifecycle::Idle).with_pending_result("chosen");
        assert!(!entry.handle_pop(|_| false), "the entry did not pop");
        assert_eq!(entry.state, RouteLifecycle::Idle, "and it is idle again");
        assert_eq!(
            entry.pending_result.as_deref(),
            Some("chosen"),
            "the value is still waiting for the pop that does happen"
        );
        assert!(!entry.completion.is_completed());

        // And when the route does pop, later, the value that waited is the
        // one handed over.
        assert!(entry.handle_pop(|_| true));
        assert_eq!(entry.completion.popped(), Some(Some("chosen")));
    }

    #[test]
    fn an_already_completed_route_is_left_alone() {
        // Upstream's short circuit, whose comment names the case: *"This is a
        // page-based route popped through the Navigator.pop. The didPop should
        // have been called. No further action is needed."*
        let mut entry =
            HistoryEntry::new(1, RouteLifecycle::Idle).with_pending_result("never delivered");
        entry.completion.did_complete(Some("already".to_string()));
        let mut asked = false;
        assert!(entry.handle_pop(|_| {
            asked = true;
            true
        }));
        assert!(!asked, "the route is not asked a second time");
        assert_eq!(
            entry.completion.popped(),
            Some(Some("already")),
            "nothing was completed a second time"
        );
        assert_eq!(
            entry.pending_result.as_deref(),
            Some("never delivered"),
            "and nothing was taken from the entry -- \"no further action\" is              what it says"
        );
        assert_eq!(entry.state, RouteLifecycle::Popping);
    }

    #[test]
    fn the_other_road_to_completion_is_the_lifecycle_itself() {
        // `handleComplete` is what `pushReplacement` reaches -- a route whose
        // future has to finish although no pop ever happened. It completes,
        // drops the value, and moves the entry on to `remove`.
        let mut entry =
            HistoryEntry::new(1, RouteLifecycle::Complete).with_pending_result("replaced away");
        entry.handle_complete();
        assert_eq!(entry.completion.popped(), Some(Some("replaced away")));
        assert_eq!(entry.pending_result, None);
        assert_eq!(entry.state, RouteLifecycle::Remove);
        assert!(
            entry.state.is_present(),
            "and `remove` is still present -- the route is on screen until it is `popping`"
        );
    }
}
