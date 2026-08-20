//! The navigator's bookkeeping -- a port of the parts of upstream's
//! `widgets/navigator.dart` that are decisions rather than widget plumbing.
//!
//! The substantial thing in here is the **transition delegate**: when a
//! declarative page list changes, something has to work out which routes are
//! arriving, which are leaving, and -- the part with the judgement in it --
//! which of them get an animation. Upstream's answer is two rules, and both
//! are about not animating things nobody asked to see move:
//!
//! 1. Entering routes go **on top of** exiting routes at the same location.
//! 2. Only the **topmost** route transitions with an animation. Everything
//!    below is added or completed outright.
//!
//! Without the second rule, replacing a three-page stack in one update would
//! play three transitions at once behind each other, which the reader would
//! see as flicker under the page they are actually looking at.
//!
//! ## What is not here
//!
//! `Navigator` is a `StatefulWidget` upstream with an `Overlay`, focus
//! scopes, restoration and hero coordination, none of which this crate has.
//! Routes are identified here by an id rather than by a `Route` object -- see
//! [`crate::routes`] for what a route itself decides. What is ported is the
//! history bookkeeping, the observer contract and its ordering, and the
//! transition delegate in full.

use crate::routes::RoutePopDisposition;
use std::collections::HashMap;

/// Upstream `RouteSettings`: the name and arguments a route was built from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RouteSettings {
    /// `None` means the route is anonymous, which is not the same as unnamed:
    /// a route pushed by object rather than by name never had one.
    pub name: Option<String>,
    /// Upstream's `arguments` is `Object?`; carried here as an opaque string
    /// because this crate has no dynamic value type.
    pub arguments: Option<String>,
}

impl RouteSettings {
    pub fn new() -> RouteSettings {
        RouteSettings::default()
    }

    pub fn named(name: impl Into<String>) -> RouteSettings {
        RouteSettings {
            name: Some(name.into()),
            arguments: None,
        }
    }

    pub fn with_arguments(mut self, arguments: impl Into<String>) -> Self {
        self.arguments = Some(arguments.into());
        self
    }

    pub fn is_anonymous(&self) -> bool {
        self.name.is_none()
    }
}

/// Upstream `Page`: a route described declaratively, as an entry in a list
/// rather than as something pushed.
///
/// The difference from [`RouteSettings`] -- which it extends upstream -- is
/// that a page is *matched against the previous frame's list*. A navigator
/// handed a new page list has to work out which pages are the same ones it had,
/// which are new, and which have gone, and [`Page::can_update`] is that answer.
///
/// # Two stand-ins, both for things this crate has no counterpart for
///
/// Upstream's `canUpdate` is `other.runtimeType == runtimeType && other.key ==
/// key`, and neither half survives literally:
///
/// * **`runtimeType`.** Upstream's `Page` is abstract and applications subclass
///   it, so the type is what says a `HomePage` is not a `SettingsPage`. Here
///   there is one concrete type, so [`Page::kind`] carries that distinction
///   explicitly and the caller sets it.
/// * **`key`.** Upstream's is a `LocalKey`, which this crate does not have;
///   carried as an opaque string, the same way [`RouteSettings::arguments`]
///   carries upstream's `Object?` and for the same stated reason.
///
/// A stand-in that a caller has to fill in is worse than a language feature
/// that fills itself in, and both are written down rather than hidden behind a
/// default that looks like it works.
#[derive(Clone)]
pub struct Page {
    /// The name and arguments, which upstream inherits from `RouteSettings`.
    pub settings: RouteSettings,
    /// Upstream's `key`. **Two pages with no key never match**, which falls out
    /// of `null == null` being true in Dart and is therefore *not* what
    /// upstream does -- see [`Page::can_update`], which spells the case out.
    pub key: Option<String>,
    /// The stand-in for `runtimeType`. Two pages of different kinds never
    /// update into one another however their keys compare.
    pub kind: &'static str,
    /// Upstream's `restorationId`.
    pub restoration_id: Option<String>,
    /// Upstream's `canPop`, which defaults to **true**: a page is poppable
    /// unless it says otherwise.
    pub can_pop: bool,
}

impl Page {
    /// A page of `kind`, poppable, with nothing else said about it.
    pub fn new(kind: &'static str) -> Page {
        Page {
            settings: RouteSettings::new(),
            key: None,
            kind,
            restoration_id: None,
            can_pop: true,
        }
    }

    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.settings.name = Some(name.into());
        self
    }

    pub fn with_arguments(mut self, arguments: impl Into<String>) -> Self {
        self.settings.arguments = Some(arguments.into());
        self
    }

    pub fn with_restoration_id(mut self, id: impl Into<String>) -> Self {
        self.restoration_id = Some(id.into());
        self
    }

    /// Upstream's `canPop: false`, for a page that refuses the back gesture.
    pub fn with_can_pop(mut self, can_pop: bool) -> Self {
        self.can_pop = can_pop;
        self
    }

    /// Upstream's `canUpdate`: whether `other` describes the same page as this
    /// one, so the route already on screen can be reused instead of replaced.
    ///
    /// # Two keyless pages *do* match, and that is upstream's behaviour
    ///
    /// Dart's `null == null` is true, so `other.key == key` holds when neither
    /// has one. That means a list of unkeyed pages of the same kind matches
    /// position for position -- which is why a declarative navigator that
    /// reorders unkeyed pages appears to change their contents rather than
    /// move them, and why keys are what upstream tells you to add when that
    /// happens.
    ///
    /// It is worth stating because the intuition runs the other way: "no key"
    /// reads like "no identity", and it is not.
    pub fn can_update(&self, other: &Page) -> bool {
        self.kind == other.kind && self.key == other.key
    }
}

impl std::fmt::Debug for Page {
    /// Upstream's `toString`: the name, the key and the arguments.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Page(\"{}\", {:?}, {:?})",
            self.settings.name.as_deref().unwrap_or(""),
            self.key,
            self.settings.arguments
        )
    }
}

/// Upstream `NavigatorObserver`: the seven things a navigator announces.
///
/// Every method has an empty default, so an observer implements only what it
/// cares about -- upstream's own `HeroController` overrides four of them and a
/// route-logging observer typically overrides one.
pub trait NavigatorObserver {
    fn did_push(&mut self, _route: u64, _previous_route: Option<u64>) {}
    fn did_pop(&mut self, _route: u64, _previous_route: Option<u64>) {}

    /// Upstream's `didRemove`, whose contract is worth reading twice: when
    /// **several** routes are removed at once, `previousRoute` is the route
    /// below the *bottommost* one being removed -- the same value every time
    /// -- and the callback fires once per removed route, top to bottom.
    fn did_remove(&mut self, _route: u64, _previous_route: Option<u64>) {}

    fn did_replace(&mut self, _new_route: Option<u64>, _old_route: Option<u64>) {}

    /// Upstream's `didChangeTop`, which is not derivable from the other four:
    /// the top can change because something was pushed, popped, removed or
    /// replaced, and an observer that only wants "what is the reader looking
    /// at now" would otherwise have to reconstruct it from all of them.
    fn did_change_top(&mut self, _top_route: u64, _previous_top_route: Option<u64>) {}

    /// Upstream's `didStartUserGesture`, whose entire purpose is to let the
    /// hero controller stand down: a hero flying between two pages while the
    /// reader drags one of them by hand fights the finger.
    fn did_start_user_gesture(&mut self, _route: u64, _previous_route: Option<u64>) {}

    fn did_stop_user_gesture(&mut self) {}
}

/// What a [`NavigatorState`] announced, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigatorObservation {
    Push {
        route: u64,
        previous: Option<u64>,
    },
    Pop {
        route: u64,
        previous: Option<u64>,
    },
    Remove {
        route: u64,
        previous: Option<u64>,
    },
    Replace {
        new_route: Option<u64>,
        old_route: Option<u64>,
    },
    ChangeTop {
        top: u64,
        previous: Option<u64>,
    },
    StartUserGesture {
        route: u64,
        previous: Option<u64>,
    },
    StopUserGesture,
}

/// Upstream `HeroControllerScope`: hosts a hero controller for the navigators
/// below it.
///
/// The rule that shapes it: **a controller may be subscribed to one navigator
/// at a time**, and the first navigator to pick one up bars every navigator
/// beneath it from seeing it. A hero flight is between two pages of one
/// history; two navigators sharing a controller would be two histories
/// disagreeing about where the hero is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeroControllerScope {
    /// `None` is upstream's `HeroControllerScope.none`, which exists to say
    /// "stop here" -- a subtree that should *not* inherit the controller above
    /// it, such as a nested navigator that runs its own transitions.
    pub controller: Option<u64>,
    claimed_by: Option<u64>,
}

impl HeroControllerScope {
    pub fn new(controller: u64) -> HeroControllerScope {
        HeroControllerScope {
            controller: Some(controller),
            claimed_by: None,
        }
    }

    /// Upstream's `HeroControllerScope.none`.
    pub fn none() -> HeroControllerScope {
        HeroControllerScope {
            controller: None,
            claimed_by: None,
        }
    }

    pub fn claimed_by(&self) -> Option<u64> {
        self.claimed_by
    }

    /// Whether `navigator` gets the controller. The first asker takes it; a
    /// later one is told no rather than sharing.
    pub fn claim(&mut self, navigator: u64) -> Option<u64> {
        let controller = self.controller?;
        match self.claimed_by {
            Some(owner) if owner != navigator => None,
            _ => {
                self.claimed_by = Some(navigator);
                Some(controller)
            }
        }
    }

    pub fn release(&mut self, navigator: u64) {
        if self.claimed_by == Some(navigator) {
            self.claimed_by = None;
        }
    }
}

/// What a [`TransitionDelegate`] decided for one route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionDecision {
    /// Upstream's `markForPush`: enters with an animation.
    Push,
    /// Upstream's `markForAdd`: enters with none.
    Add,
    /// Upstream's `markForPop`: leaves with an animation.
    Pop,
    /// Upstream's `markForComplete`: leaves with none, and its future
    /// completes.
    Complete,
}

impl TransitionDecision {
    /// Whether this decision animates. The two that do are the two the reader
    /// is meant to notice.
    pub fn is_animated(self) -> bool {
        matches!(self, TransitionDecision::Push | TransitionDecision::Pop)
    }
}

/// Upstream `RouteTransitionRecord`: one route, and the decision it is waiting
/// for.
///
/// The two `waiting` flags are separate rather than one enum because a record
/// can be waiting for neither: a route already popped in an earlier update is
/// still in the exiting set while its animation finishes, and asking for a
/// decision about it again would restart the animation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteTransitionRecord {
    pub route: u64,
    waiting_for_entering: bool,
    waiting_for_exiting: bool,
    decision: Option<TransitionDecision>,
}

impl RouteTransitionRecord {
    /// A record waiting to be told how to come on screen.
    pub fn entering(route: u64) -> RouteTransitionRecord {
        RouteTransitionRecord {
            route,
            waiting_for_entering: true,
            waiting_for_exiting: false,
            decision: None,
        }
    }

    /// A record waiting to be told how to leave.
    pub fn exiting(route: u64) -> RouteTransitionRecord {
        RouteTransitionRecord {
            route,
            waiting_for_entering: false,
            waiting_for_exiting: true,
            decision: None,
        }
    }

    /// A record that is settled already -- already popped in an earlier update
    /// and still animating out.
    pub fn settled(route: u64) -> RouteTransitionRecord {
        RouteTransitionRecord {
            route,
            waiting_for_entering: false,
            waiting_for_exiting: false,
            decision: None,
        }
    }

    pub fn is_waiting_for_entering_decision(&self) -> bool {
        self.waiting_for_entering
    }

    pub fn is_waiting_for_exiting_decision(&self) -> bool {
        self.waiting_for_exiting
    }

    pub fn decision(&self) -> Option<TransitionDecision> {
        self.decision
    }

    pub fn mark_for_push(&mut self) {
        self.decide(TransitionDecision::Push);
    }

    pub fn mark_for_add(&mut self) {
        self.decide(TransitionDecision::Add);
    }

    pub fn mark_for_pop(&mut self) {
        self.decide(TransitionDecision::Pop);
    }

    /// Upstream's `markForComplete`, which is also what the deprecated
    /// `markForRemove` now does. The two used to differ in whether the route's
    /// future completed, and removing without completing left whoever awaited
    /// the route waiting forever.
    pub fn mark_for_complete(&mut self) {
        self.decide(TransitionDecision::Complete);
    }

    fn decide(&mut self, decision: TransitionDecision) {
        self.decision = Some(decision);
        self.waiting_for_entering = false;
        self.waiting_for_exiting = false;
    }
}

/// Everything a [`TransitionDelegate`] is given.
#[derive(Clone, Debug, Default)]
pub struct TransitionRequest {
    /// The page-based routes in the order they will be in afterwards.
    pub new_page_route_history: Vec<RouteTransitionRecord>,
    /// Exiting page routes, keyed by the route **directly below** where they
    /// were. `None` is the bottom of the stack. Keying by location rather than
    /// by the route itself is what lets a delegate put a leaving page back
    /// where it was rather than at the top.
    pub location_to_exiting_page_route: HashMap<Option<u64>, RouteTransitionRecord>,
    /// Pageless routes -- dialogs, sheets -- that belong to a page route.
    pub page_route_to_pageless_routes: HashMap<u64, Vec<RouteTransitionRecord>>,
}

impl TransitionRequest {
    pub fn new() -> TransitionRequest {
        TransitionRequest::default()
    }

    pub fn with_history(mut self, history: Vec<RouteTransitionRecord>) -> Self {
        self.new_page_route_history = history;
        self
    }

    pub fn exiting_at(mut self, location: Option<u64>, record: RouteTransitionRecord) -> Self {
        self.location_to_exiting_page_route.insert(location, record);
        self
    }

    pub fn pageless_under(mut self, page: u64, records: Vec<RouteTransitionRecord>) -> Self {
        self.page_route_to_pageless_routes.insert(page, records);
        self
    }
}

/// What upstream's integrity assertion catches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransitionIntegrityError {
    /// A record came back still waiting for a decision.
    Undecided(u64),
    /// The entering routes came back in a different order than they were
    /// given in. A delegate may put exiting routes anywhere, but it may not
    /// reorder the history it was handed -- that order *is* the new stack.
    ReorderedHistory,
    /// A route that was supposed to leave is not in the result at all, so
    /// nothing would ever take it off screen.
    MissingRoutes,
}

/// Upstream `TransitionDelegate`: decides how a page update animates.
pub trait TransitionDelegate {
    /// Upstream's `resolve`. It must decide every waiting record and return
    /// the merged list of **page** routes.
    ///
    /// The request is taken by `&mut` rather than by value because upstream's
    /// pageless records are decided **in place** and never appear in the
    /// returned list -- the navigator holds the same objects and reads them
    /// afterwards. Without aliasing, handing the request back is how the
    /// caller learns what happened to the dialogs on a leaving page.
    fn resolve(&self, request: &mut TransitionRequest) -> Vec<RouteTransitionRecord>;

    /// Upstream's `_transition`, which calls [`TransitionDelegate::resolve`]
    /// and then checks the result.
    ///
    /// Upstream does this inside an `assert`, so it is a debug-only check
    /// there; it is a returned `Result` here, because the three things it
    /// catches are all silent in release otherwise -- an undecided route never
    /// enters, a dropped exiting route never leaves, and a reordered history
    /// puts the reader on the wrong page.
    fn transition(
        &self,
        request: &mut TransitionRequest,
    ) -> Result<Vec<RouteTransitionRecord>, TransitionIntegrityError> {
        let expected_history: Vec<u64> = request
            .new_page_route_history
            .iter()
            .map(|record| record.route)
            .collect();
        let mut expected_exiting: Vec<u64> = request
            .location_to_exiting_page_route
            .values()
            .map(|record| record.route)
            .collect();
        let results = self.resolve(request);

        // Every pageless route under an exiting page must have been decided
        // too, or the dialog on a page that is going stays on screen.
        for records in request.page_route_to_pageless_routes.values() {
            for record in records {
                if record.is_waiting_for_exiting_decision() {
                    return Err(TransitionIntegrityError::Undecided(record.route));
                }
            }
        }

        let mut next_in_history = 0usize;
        for record in results.iter() {
            if record.is_waiting_for_entering_decision() || record.is_waiting_for_exiting_decision()
            {
                return Err(TransitionIntegrityError::Undecided(record.route));
            }
            if next_in_history < expected_history.len()
                && record.route == expected_history[next_in_history]
            {
                next_in_history += 1;
            } else if let Some(at) = expected_exiting.iter().position(|id| *id == record.route) {
                expected_exiting.remove(at);
            } else {
                // Neither the next route in the history nor a route that was
                // meant to leave: the history has been reordered.
                return Err(TransitionIntegrityError::ReorderedHistory);
            }
        }
        if next_in_history != expected_history.len() || !expected_exiting.is_empty() {
            return Err(TransitionIntegrityError::MissingRoutes);
        }
        Ok(results)
    }
}

/// Upstream `DefaultTransitionDelegate`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DefaultTransitionDelegate;

impl DefaultTransitionDelegate {
    pub const fn new() -> DefaultTransitionDelegate {
        DefaultTransitionDelegate
    }

    /// Upstream's `handleExitingRoute`, and it is recursive for a reason:
    /// several pages can have been removed from the same place at once, each
    /// recorded as sitting above the last, and all of them have to come out.
    fn handle_exiting_route(
        location: Option<u64>,
        is_last: bool,
        exiting: &mut HashMap<Option<u64>, RouteTransitionRecord>,
        pageless: &mut HashMap<u64, Vec<RouteTransitionRecord>>,
        results: &mut Vec<RouteTransitionRecord>,
    ) {
        let Some(mut exiting_page_route) = exiting.remove(&location) else {
            return;
        };
        let route = exiting_page_route.route;
        if exiting_page_route.is_waiting_for_exiting_decision() {
            let has_pageless = pageless.contains_key(&route);
            // The last one out only if nothing else is leaving from above it.
            let is_last_exiting = is_last && !exiting.contains_key(&Some(route));
            if is_last_exiting && !has_pageless {
                exiting_page_route.mark_for_pop();
            } else {
                // A page with a dialog still on it does **not** animate out
                // itself: the dialog is what the reader is looking at, so the
                // dialog gets the animation and the page underneath it simply
                // completes.
                exiting_page_route.mark_for_complete();
            }
            if let Some(pageless_routes) = pageless.get_mut(&route) {
                let last_index = pageless_routes.len().saturating_sub(1);
                for (index, pageless_route) in pageless_routes.iter_mut().enumerate() {
                    // Some may need no decision: the page list can be updated
                    // right after a pop, and that route is already on its way
                    // out.
                    if !pageless_route.is_waiting_for_exiting_decision() {
                        continue;
                    }
                    if is_last_exiting && index == last_index {
                        pageless_route.mark_for_pop();
                    } else {
                        pageless_route.mark_for_complete();
                    }
                }
            }
        }
        results.push(exiting_page_route);
        // There may be another exiting route above this one.
        Self::handle_exiting_route(Some(route), is_last, exiting, pageless, results);
    }
}

impl TransitionDelegate for DefaultTransitionDelegate {
    fn resolve(&self, request: &mut TransitionRequest) -> Vec<RouteTransitionRecord> {
        let new_page_route_history = std::mem::take(&mut request.new_page_route_history);
        let location_to_exiting_page_route = &mut request.location_to_exiting_page_route;
        let page_route_to_pageless_routes = &mut request.page_route_to_pageless_routes;
        let mut results = Vec::new();

        // Anything that was leaving from the bottom of the stack comes first.
        Self::handle_exiting_route(
            None,
            new_page_route_history.is_empty(),
            location_to_exiting_page_route,
            page_route_to_pageless_routes,
            &mut results,
        );

        let last_index = new_page_route_history.len().saturating_sub(1);
        for (index, mut page_route) in new_page_route_history.into_iter().enumerate() {
            let is_last = index == last_index;
            let route = page_route.route;
            if page_route.is_waiting_for_entering_decision() {
                // Animated only if it is the top **and** nothing is leaving
                // from its position. If something is, the arriving page is
                // taking a place that is still occupied, and animating it in
                // would put two pages over each other mid-flight.
                if is_last && !location_to_exiting_page_route.contains_key(&Some(route)) {
                    page_route.mark_for_push();
                } else {
                    page_route.mark_for_add();
                }
            }
            results.push(page_route);
            Self::handle_exiting_route(
                Some(route),
                is_last,
                location_to_exiting_page_route,
                page_route_to_pageless_routes,
                &mut results,
            );
        }
        results
    }
}

/// Upstream `NavigatorState`, reduced to the history it keeps and what it
/// announces about it.
#[derive(Debug, Default)]
pub struct NavigatorState {
    history: Vec<u64>,
    observations: Vec<NavigatorObservation>,
    user_gesture_in_progress: bool,
}

impl NavigatorState {
    pub fn new() -> NavigatorState {
        NavigatorState::default()
    }

    pub fn history(&self) -> &[u64] {
        &self.history
    }

    pub fn observations(&self) -> &[NavigatorObservation] {
        &self.observations
    }

    pub fn user_gesture_in_progress(&self) -> bool {
        self.user_gesture_in_progress
    }

    fn top(&self) -> Option<u64> {
        self.history.last().copied()
    }

    fn below_top(&self) -> Option<u64> {
        if self.history.len() < 2 {
            return None;
        }
        self.history.get(self.history.len() - 2).copied()
    }

    /// Upstream's `push`.
    pub fn push(&mut self, route: u64) {
        let previous = self.top();
        self.history.push(route);
        self.observations
            .push(NavigatorObservation::Push { route, previous });
        self.observations.push(NavigatorObservation::ChangeTop {
            top: route,
            previous,
        });
    }

    /// Upstream's `pop`, once the route has already agreed to go.
    pub fn pop(&mut self) -> Option<u64> {
        let route = self.history.pop()?;
        let previous = self.top();
        self.observations
            .push(NavigatorObservation::Pop { route, previous });
        if let Some(top) = previous {
            self.observations.push(NavigatorObservation::ChangeTop {
                top,
                previous: Some(route),
            });
        }
        Some(route)
    }

    /// Upstream's `canPop`.
    ///
    /// Read the order carefully: it consults the **bottom-most** route's
    /// `willHandlePopInternally`, not the top one's. That is not a mistake --
    /// it is the single-route case being answered. With two or more routes the
    /// answer is yes regardless, and the only stack where the question is open
    /// is one with a single route that might still have local history of its
    /// own to peel.
    pub fn can_pop(&self, bottom_handles_pop_internally: bool) -> bool {
        if self.history.is_empty() {
            return false;
        }
        if bottom_handles_pop_internally {
            return true;
        }
        self.history.len() >= 2
    }

    /// Upstream's `maybePop`: consult the top route, then act.
    ///
    /// The return value is **whether the request was handled**, not whether
    /// anything popped -- and the two differ in exactly one case. A route that
    /// refused still handled the press: something in it said no, and the
    /// caller should stop looking for someone else to ask. Only `Bubble`
    /// returns false, and that is what lets the press reach the platform.
    pub fn maybe_pop(&mut self, disposition: RoutePopDisposition) -> bool {
        if self.history.is_empty() {
            return false;
        }
        match disposition {
            RoutePopDisposition::Bubble => false,
            RoutePopDisposition::Pop => {
                self.pop();
                true
            }
            RoutePopDisposition::DoNotPop => true,
        }
    }

    /// Upstream's `removeRoute`.
    pub fn remove(&mut self, route: u64) -> bool {
        let Some(at) = self.history.iter().position(|held| *held == route) else {
            return false;
        };
        let was_top = at + 1 == self.history.len();
        let previous_top = self.top();
        let previous = if at == 0 {
            None
        } else {
            self.history.get(at - 1).copied()
        };
        self.history.remove(at);
        self.observations
            .push(NavigatorObservation::Remove { route, previous });
        if was_top {
            if let Some(top) = self.top() {
                self.observations.push(NavigatorObservation::ChangeTop {
                    top,
                    previous: previous_top,
                });
            }
        }
        true
    }

    /// Upstream's `removeRouteBelow`, seen through `didRemove`'s contract:
    /// several routes going at once report the **same** `previousRoute` -- the
    /// one below the bottommost of them -- and are announced from the top
    /// down.
    pub fn remove_range(&mut self, routes: &[u64]) {
        let Some(lowest) = routes
            .iter()
            .filter_map(|route| self.history.iter().position(|held| held == route))
            .min()
        else {
            return;
        };
        let previous = if lowest == 0 {
            None
        } else {
            self.history.get(lowest - 1).copied()
        };
        let previous_top = self.top();
        let mut going: Vec<u64> = self
            .history
            .iter()
            .copied()
            .filter(|held| routes.contains(held))
            .collect();
        going.reverse();
        self.history.retain(|held| !routes.contains(held));
        for route in going {
            self.observations
                .push(NavigatorObservation::Remove { route, previous });
        }
        if let Some(top) = self.top() {
            if Some(top) != previous_top {
                self.observations.push(NavigatorObservation::ChangeTop {
                    top,
                    previous: previous_top,
                });
            }
        }
    }

    /// Upstream's `replace`.
    pub fn replace(&mut self, old_route: u64, new_route: u64) -> bool {
        let Some(at) = self.history.iter().position(|held| *held == old_route) else {
            return false;
        };
        let was_top = at + 1 == self.history.len();
        self.history[at] = new_route;
        self.observations.push(NavigatorObservation::Replace {
            new_route: Some(new_route),
            old_route: Some(old_route),
        });
        if was_top {
            self.observations.push(NavigatorObservation::ChangeTop {
                top: new_route,
                previous: Some(old_route),
            });
        }
        true
    }

    /// Upstream's `didStartUserGesture`, which the hero controller listens for
    /// so it can stand down while a finger is driving the transition.
    pub fn did_start_user_gesture(&mut self) {
        self.user_gesture_in_progress = true;
        if let Some(route) = self.top() {
            self.observations
                .push(NavigatorObservation::StartUserGesture {
                    route,
                    previous: self.below_top(),
                });
        }
    }

    pub fn did_stop_user_gesture(&mut self) {
        self.user_gesture_in_progress = false;
        self.observations
            .push(NavigatorObservation::StopUserGesture);
    }

    /// Applies what a [`TransitionDelegate`] decided, in the order it returned
    /// them. Routes that were told to leave without an animation are gone
    /// immediately; the animated ones stay until their animation ends.
    pub fn apply_transition(&mut self, records: &[RouteTransitionRecord]) {
        self.history = records
            .iter()
            .filter(|record| {
                !matches!(
                    record.decision(),
                    Some(TransitionDecision::Complete) | Some(TransitionDecision::Pop)
                )
            })
            .map(|record| record.route)
            .collect();
    }
}

/// Upstream `RestorableRouteFuture`: a route that survives the application
/// being killed and restarted.
///
/// What it actually stores is **an id, not a route** -- the whole point. A
/// restored application has no route objects yet, so the only thing that can
/// be written to disk is the name of one, and the route is rebuilt from it and
/// re-hooked to the same completion callback.
#[derive(Debug, Default)]
pub struct RestorableRouteFuture {
    route_id: Option<String>,
    completed_with: Option<Option<String>>,
}

impl RestorableRouteFuture {
    pub fn new() -> RestorableRouteFuture {
        RestorableRouteFuture::default()
    }

    /// Upstream's `createDefaultValue`: nothing shown.
    pub fn create_default_value(&self) -> Option<String> {
        None
    }

    /// Upstream's `present`, which **asserts it is not already presenting**. A
    /// second route under one future would leave the first with nothing to
    /// report its result to.
    pub fn present(&mut self, route_id: impl Into<String>) {
        debug_assert!(!self.is_present(), "already presenting a route");
        self.route_id = Some(route_id.into());
        self.completed_with = None;
    }

    /// Upstream's `initWithValue`: a restored id hooks straight back onto the
    /// route's future without anyone calling `present` again.
    pub fn init_with_value(&mut self, value: Option<String>) {
        self.route_id = value;
    }

    pub fn is_present(&self) -> bool {
        self.route_id.is_some()
    }

    pub fn route(&self) -> Option<&str> {
        self.route_id.as_deref()
    }

    /// The route completed; the future's value arrives and the slot empties.
    pub fn complete(&mut self, result: Option<String>) {
        self.route_id = None;
        self.completed_with = Some(result);
    }

    pub fn completed_with(&self) -> Option<Option<&str>> {
        self.completed_with.as_ref().map(|result| result.as_deref())
    }
}

/// Upstream `NavigationNotification`: "something below me can handle a pop".
///
/// It travels **up** the tree rather than down, and that direction is the
/// point: a nested navigator, a `PopScope`, or anything else that would absorb
/// a back press announces itself so the thing at the top -- which is what the
/// platform actually asks -- knows not to close the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavigationNotification {
    pub can_handle_pop: bool,
}

impl NavigationNotification {
    pub fn new(can_handle_pop: bool) -> NavigationNotification {
        NavigationNotification { can_handle_pop }
    }
}

/// Upstream `NavigatorPopHandler`: lets a nested navigator handle the back
/// press before the outer one does.
///
/// The whole widget is one inversion, and it reads backwards until it is
/// named:
///
/// ```dart
/// canPop: !widget.enabled || _canPop,
/// // and, from the notification:
/// final bool nextCanPop = !notification.canHandlePop;
/// ```
///
/// **When the subtree says it *can* handle a pop, this scope reports that it
/// *cannot* pop.** Refusing here is what stops the outer navigator taking the
/// press, which leaves the nested one free to take it instead. Saying "yes I
/// can pop" would pop this whole route and take the nested navigator's entire
/// history with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavigatorPopHandler {
    /// Upstream's `enabled`, true by default. When false the handler stands
    /// aside entirely -- `canPop` is true and the outer navigator behaves as
    /// though the handler were not there.
    pub enabled: bool,
    /// Upstream's `_canPop`, derived from the last notification seen.
    can_pop: bool,
    pops: usize,
}

impl Default for NavigatorPopHandler {
    fn default() -> NavigatorPopHandler {
        NavigatorPopHandler::new()
    }
}

impl NavigatorPopHandler {
    pub fn new() -> NavigatorPopHandler {
        NavigatorPopHandler {
            enabled: true,
            can_pop: true,
            pops: 0,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// How many times upstream's `onPop` would have fired.
    pub fn pops(&self) -> usize {
        self.pops
    }

    /// What the enclosing `PopScope` reports.
    pub fn can_pop(&self) -> bool {
        !self.enabled || self.can_pop
    }

    /// Upstream's `NotificationListener<NavigationNotification>` callback.
    ///
    /// Note it returns **false** -- the notification keeps bubbling. A handler
    /// further out needs to hear the same thing, since it has the same
    /// decision to make about the navigator between them.
    pub fn on_navigation_notification(&mut self, notification: NavigationNotification) -> bool {
        self.can_pop = !notification.can_handle_pop;
        false
    }

    /// Upstream's `onPopInvokedWithResult`, which **returns early when the pop
    /// went through**.
    ///
    /// `onPop` is for the case where the pop was refused: that refusal is this
    /// widget saying "the nested navigator will take it", and the callback is
    /// where the caller makes it do so.
    pub fn on_pop_invoked(&mut self, did_pop: bool) {
        if did_pop {
            return;
        }
        self.pops += 1;
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_subtree_that_can_handle_a_pop_makes_this_scope_refuse_to_pop() {
        // The inversion: refusing here is what stops the outer navigator
        // taking the press, leaving the nested one free to take it.
        let mut handler = NavigatorPopHandler::new();
        assert!(handler.can_pop(), "nothing nested has spoken yet");

        handler.on_navigation_notification(NavigationNotification::new(true));
        assert!(
            !handler.can_pop(),
            "the nested navigator can handle it, so we say we cannot"
        );

        handler.on_navigation_notification(NavigationNotification::new(false));
        assert!(handler.can_pop(), "and now the outer one should have it");
    }

    #[test]
    fn a_disabled_handler_stands_aside_entirely() {
        let mut handler = NavigatorPopHandler::new().with_enabled(false);
        handler.on_navigation_notification(NavigationNotification::new(true));
        assert!(handler.can_pop(), "as though it were not there");
    }

    #[test]
    fn the_notification_keeps_bubbling_past_this_handler() {
        // A handler further out has the same decision to make about the
        // navigator between them.
        let mut handler = NavigatorPopHandler::new();
        assert!(!handler.on_navigation_notification(NavigationNotification::new(true)));
    }

    #[test]
    fn on_pop_fires_only_when_the_pop_was_refused() {
        // The refusal is this widget saying "the nested navigator will take
        // it", and the callback is where the caller makes it do so.
        let mut handler = NavigatorPopHandler::new();
        handler.on_pop_invoked(true);
        assert_eq!(handler.pops(), 0, "it popped; nothing to arrange");

        handler.on_pop_invoked(false);
        assert_eq!(handler.pops(), 1);
    }

    use super::*;

    fn decisions(records: &[RouteTransitionRecord]) -> Vec<(u64, Option<TransitionDecision>)> {
        records
            .iter()
            .map(|record| (record.route, record.decision()))
            .collect()
    }

    // -- The transition delegate ------------------------------------------

    #[test]
    fn only_the_topmost_arriving_page_gets_an_animation() {
        // Three transitions at once would be flicker behind the page the
        // reader is actually looking at.
        let mut request = TransitionRequest::new().with_history(vec![
            RouteTransitionRecord::entering(1),
            RouteTransitionRecord::entering(2),
            RouteTransitionRecord::entering(3),
        ]);
        let results = DefaultTransitionDelegate::new().resolve(&mut request);
        assert_eq!(
            decisions(&results),
            vec![
                (1, Some(TransitionDecision::Add)),
                (2, Some(TransitionDecision::Add)),
                (3, Some(TransitionDecision::Push)),
            ]
        );
    }

    #[test]
    fn an_arriving_page_taking_an_occupied_place_is_added_rather_than_pushed() {
        // Something is still leaving from where it is going. Animating it in
        // would put two pages over each other in mid-flight.
        let mut request = TransitionRequest::new()
            .with_history(vec![RouteTransitionRecord::entering(1)])
            .exiting_at(Some(1), RouteTransitionRecord::exiting(9));
        let results = DefaultTransitionDelegate::new().resolve(&mut request);
        assert_eq!(
            decisions(&results),
            vec![
                (1, Some(TransitionDecision::Add)),
                (9, Some(TransitionDecision::Pop)),
            ],
            "and the one leaving is the one the reader watches"
        );
    }

    #[test]
    fn an_entering_page_goes_on_top_of_what_is_leaving_from_the_same_place() {
        // Upstream's first rule. The order of the result is the new stack, and
        // a page that is on its way out belongs underneath the one replacing
        // it.
        let mut request = TransitionRequest::new()
            .with_history(vec![
                RouteTransitionRecord::entering(1),
                RouteTransitionRecord::entering(2),
            ])
            .exiting_at(Some(1), RouteTransitionRecord::exiting(9));
        let results = DefaultTransitionDelegate::new().resolve(&mut request);
        assert_eq!(
            results.iter().map(|r| r.route).collect::<Vec<_>>(),
            vec![1, 9, 2],
            "the exiting route sits at the location it was at"
        );
    }

    #[test]
    fn a_stack_of_pages_leaving_at_once_only_animates_the_last_one_out() {
        // handleExitingRoute recurses because several pages can have been
        // removed from the same place, each recorded above the last.
        let mut request = TransitionRequest::new()
            .exiting_at(None, RouteTransitionRecord::exiting(7))
            .exiting_at(Some(7), RouteTransitionRecord::exiting(8))
            .exiting_at(Some(8), RouteTransitionRecord::exiting(9));
        let results = DefaultTransitionDelegate::new().resolve(&mut request);
        assert_eq!(
            decisions(&results),
            vec![
                (7, Some(TransitionDecision::Complete)),
                (8, Some(TransitionDecision::Complete)),
                (9, Some(TransitionDecision::Pop)),
            ],
            "the top one animates, the ones beneath it just go"
        );
    }

    #[test]
    fn a_page_with_a_dialog_on_it_lets_the_dialog_do_the_animating() {
        // The dialog is what the reader is looking at, so the page underneath
        // completes rather than sliding out from behind it.
        let mut request = TransitionRequest::new()
            .exiting_at(None, RouteTransitionRecord::exiting(7))
            .pageless_under(7, vec![RouteTransitionRecord::exiting(70)]);
        let results = DefaultTransitionDelegate::new().resolve(&mut request);
        assert_eq!(
            results[0].decision(),
            Some(TransitionDecision::Complete),
            "the page, despite being the last thing leaving"
        );
    }

    #[test]
    fn only_the_last_dialog_on_the_last_leaving_page_animates_out() {
        // The reader is looking at the topmost dialog, so that is the one that
        // gets to slide away; everything under it just goes.
        let mut request = TransitionRequest::new()
            .exiting_at(None, RouteTransitionRecord::exiting(7))
            .pageless_under(
                7,
                vec![
                    RouteTransitionRecord::exiting(70),
                    RouteTransitionRecord::exiting(71),
                    RouteTransitionRecord::exiting(72),
                ],
            );
        DefaultTransitionDelegate::new().resolve(&mut request);

        let marks: Vec<Option<TransitionDecision>> = request.page_route_to_pageless_routes[&7]
            .iter()
            .map(|record| record.decision())
            .collect();
        assert_eq!(
            marks,
            vec![
                Some(TransitionDecision::Complete),
                Some(TransitionDecision::Complete),
                Some(TransitionDecision::Pop),
            ]
        );
    }

    #[test]
    fn a_dialog_that_is_already_leaving_is_not_asked_again() {
        // The page list can be updated right after a pop, and re-deciding
        // would restart an animation that is already running.
        let mut request = TransitionRequest::new()
            .exiting_at(None, RouteTransitionRecord::exiting(7))
            .pageless_under(7, vec![RouteTransitionRecord::settled(70)]);
        DefaultTransitionDelegate::new().resolve(&mut request);
        assert_eq!(
            request.page_route_to_pageless_routes[&7][0].decision(),
            None,
            "left exactly as it was"
        );
    }

    #[test]
    fn a_settled_record_needs_no_decision_and_keeps_none() {
        // Exiting routes still animating from an earlier update come through
        // the delegate untouched.
        let mut request = TransitionRequest::new()
            .with_history(vec![RouteTransitionRecord::entering(1)])
            .exiting_at(None, RouteTransitionRecord::settled(9));
        let results = DefaultTransitionDelegate::new().resolve(&mut request);
        assert_eq!(
            decisions(&results),
            vec![(9, None), (1, Some(TransitionDecision::Push)),],
            "and it does not stop the arriving page from animating"
        );
    }

    #[test]
    fn an_empty_page_list_still_animates_the_last_thing_out() {
        // Every page removed at once: the reader should watch the top one go
        // rather than have the screen empty in one frame.
        let mut request =
            TransitionRequest::new().exiting_at(None, RouteTransitionRecord::exiting(9));
        let results = DefaultTransitionDelegate::new().resolve(&mut request);
        assert_eq!(results[0].decision(), Some(TransitionDecision::Pop));
    }

    #[test]
    fn only_push_and_pop_are_things_the_reader_sees() {
        assert!(TransitionDecision::Push.is_animated());
        assert!(TransitionDecision::Pop.is_animated());
        assert!(!TransitionDecision::Add.is_animated());
        assert!(!TransitionDecision::Complete.is_animated());
    }

    // -- The integrity check ----------------------------------------------

    struct Forgetful;
    impl TransitionDelegate for Forgetful {
        fn resolve(&self, request: &mut TransitionRequest) -> Vec<RouteTransitionRecord> {
            std::mem::take(&mut request.new_page_route_history)
        }
    }

    struct Reordering;
    impl TransitionDelegate for Reordering {
        fn resolve(&self, request: &mut TransitionRequest) -> Vec<RouteTransitionRecord> {
            let mut records = std::mem::take(&mut request.new_page_route_history);
            for record in records.iter_mut() {
                record.mark_for_add();
            }
            records.reverse();
            records
        }
    }

    struct Undeciding;
    impl TransitionDelegate for Undeciding {
        fn resolve(&self, request: &mut TransitionRequest) -> Vec<RouteTransitionRecord> {
            std::mem::take(&mut request.new_page_route_history)
        }
    }

    #[test]
    fn a_delegate_that_drops_an_exiting_route_is_caught() {
        // Nothing would ever take it off the screen.
        let mut request = TransitionRequest::new()
            .with_history(vec![{
                let mut record = RouteTransitionRecord::entering(1);
                record.mark_for_add();
                record
            }])
            .exiting_at(None, RouteTransitionRecord::settled(9));
        assert_eq!(
            Forgetful.transition(&mut request),
            Err(TransitionIntegrityError::MissingRoutes)
        );
    }

    #[test]
    fn a_delegate_that_reorders_the_history_is_caught() {
        // The order it was handed is the new stack; reordering it puts the
        // reader on the wrong page.
        let mut request = TransitionRequest::new().with_history(vec![
            RouteTransitionRecord::entering(1),
            RouteTransitionRecord::entering(2),
        ]);
        assert_eq!(
            Reordering.transition(&mut request),
            Err(TransitionIntegrityError::ReorderedHistory)
        );
    }

    #[test]
    fn a_route_that_was_never_decided_is_caught_by_name() {
        let mut request =
            TransitionRequest::new().with_history(vec![RouteTransitionRecord::entering(42)]);
        assert_eq!(
            Undeciding.transition(&mut request),
            Err(TransitionIntegrityError::Undecided(42))
        );
    }

    #[test]
    fn a_dialog_left_undecided_on_a_leaving_page_is_caught_too() {
        // It never appears in the returned list, so nothing downstream would
        // notice -- and the dialog would stay on screen after its page left.
        struct PageOnly;
        impl TransitionDelegate for PageOnly {
            fn resolve(&self, request: &mut TransitionRequest) -> Vec<RouteTransitionRecord> {
                request
                    .location_to_exiting_page_route
                    .values_mut()
                    .for_each(RouteTransitionRecord::mark_for_complete);
                std::mem::take(&mut request.location_to_exiting_page_route)
                    .into_values()
                    .collect()
            }
        }
        let mut request = TransitionRequest::new()
            .exiting_at(None, RouteTransitionRecord::exiting(7))
            .pageless_under(7, vec![RouteTransitionRecord::exiting(70)]);
        assert_eq!(
            PageOnly.transition(&mut request),
            Err(TransitionIntegrityError::Undecided(70))
        );
    }

    #[test]
    fn the_default_delegate_passes_its_own_check() {
        let mut request = TransitionRequest::new()
            .with_history(vec![
                RouteTransitionRecord::entering(1),
                RouteTransitionRecord::entering(2),
            ])
            .exiting_at(Some(1), RouteTransitionRecord::exiting(9))
            .exiting_at(None, RouteTransitionRecord::exiting(8));
        let results = DefaultTransitionDelegate::new()
            .transition(&mut request)
            .unwrap();
        assert_eq!(
            results.iter().map(|r| r.route).collect::<Vec<_>>(),
            vec![8, 1, 9, 2]
        );
    }

    #[test]
    fn an_exiting_route_may_be_put_anywhere_in_the_result() {
        // Upstream says so outright: results = [D, A, B, C, E] is as valid as
        // [A, B, C, D, E]. Only the entering order is fixed.
        struct FrontLoading;
        impl TransitionDelegate for FrontLoading {
            fn resolve(&self, request: &mut TransitionRequest) -> Vec<RouteTransitionRecord> {
                let mut results: Vec<RouteTransitionRecord> =
                    std::mem::take(&mut request.location_to_exiting_page_route)
                        .into_values()
                        .map(|mut record| {
                            record.mark_for_complete();
                            record
                        })
                        .collect();
                results.sort_by_key(|record| record.route);
                for mut record in std::mem::take(&mut request.new_page_route_history) {
                    record.mark_for_add();
                    results.push(record);
                }
                results
            }
        }
        let mut request = TransitionRequest::new()
            .with_history(vec![
                RouteTransitionRecord::entering(1),
                RouteTransitionRecord::entering(2),
            ])
            .exiting_at(None, RouteTransitionRecord::exiting(8));
        assert!(FrontLoading.transition(&mut request).is_ok());
    }

    // -- The history and its observers -------------------------------------

    #[test]
    fn the_top_changing_is_announced_separately_from_why_it_changed() {
        // An observer that only wants "what is the reader looking at" would
        // otherwise have to reconstruct it from four other callbacks.
        let mut navigator = NavigatorState::new();
        navigator.push(1);
        assert_eq!(
            navigator.observations(),
            &[
                NavigatorObservation::Push {
                    route: 1,
                    previous: None
                },
                NavigatorObservation::ChangeTop {
                    top: 1,
                    previous: None
                },
            ]
        );

        navigator.push(2);
        assert_eq!(
            navigator.observations().last(),
            Some(&NavigatorObservation::ChangeTop {
                top: 2,
                previous: Some(1)
            })
        );
    }

    #[test]
    fn popping_the_last_route_changes_no_top_because_there_is_none() {
        let mut navigator = NavigatorState::new();
        navigator.push(1);
        let before = navigator.observations().len();
        navigator.pop();
        assert_eq!(
            &navigator.observations()[before..],
            &[NavigatorObservation::Pop {
                route: 1,
                previous: None
            }]
        );
        assert!(navigator.history().is_empty());
    }

    #[test]
    fn a_stack_with_one_route_can_only_pop_if_that_route_can_absorb_it() {
        // Which is why canPop consults the bottom-most route rather than the
        // top: with two or more the answer is yes regardless, so the single
        // route case is the only open question.
        let mut navigator = NavigatorState::new();
        assert!(!navigator.can_pop(false), "nothing at all to pop");
        assert!(!navigator.can_pop(true), "and no route to absorb it either");

        navigator.push(1);
        assert!(!navigator.can_pop(false));
        assert!(
            navigator.can_pop(true),
            "one route, but it has local history of its own"
        );

        navigator.push(2);
        assert!(navigator.can_pop(false), "two routes is enough on its own");
    }

    #[test]
    fn a_refusal_still_counts_as_having_handled_the_press() {
        // Something in the route said no; the caller should stop looking for
        // someone else to ask. Only Bubble sends the press onwards.
        let mut navigator = NavigatorState::new();
        navigator.push(1);
        navigator.push(2);

        assert!(navigator.maybe_pop(RoutePopDisposition::DoNotPop));
        assert_eq!(navigator.history(), &[1, 2], "and nothing moved");

        assert!(navigator.maybe_pop(RoutePopDisposition::Pop));
        assert_eq!(navigator.history(), &[1]);

        assert!(
            !navigator.maybe_pop(RoutePopDisposition::Bubble),
            "which is what reaches the platform and closes the application"
        );
        assert_eq!(navigator.history(), &[1], "and it did not pop on the way");
    }

    #[test]
    fn several_routes_removed_at_once_all_report_the_same_route_below_them() {
        // Upstream's contract: previousRoute is the one below the *bottommost*
        // route being removed, and the callbacks run top to bottom.
        let mut navigator = NavigatorState::new();
        for route in 1..=5 {
            navigator.push(route);
        }
        let before = navigator.observations().len();
        navigator.remove_range(&[2, 3, 4]);

        assert_eq!(navigator.history(), &[1, 5]);
        assert_eq!(
            &navigator.observations()[before..],
            &[
                NavigatorObservation::Remove {
                    route: 4,
                    previous: Some(1)
                },
                NavigatorObservation::Remove {
                    route: 3,
                    previous: Some(1)
                },
                NavigatorObservation::Remove {
                    route: 2,
                    previous: Some(1)
                },
            ],
            "top down, and all three name route 1"
        );
    }

    #[test]
    fn removing_something_out_of_the_middle_does_not_change_the_top() {
        let mut navigator = NavigatorState::new();
        navigator.push(1);
        navigator.push(2);
        navigator.push(3);
        let before = navigator.observations().len();

        assert!(navigator.remove(2));
        assert_eq!(navigator.history(), &[1, 3]);
        assert_eq!(
            &navigator.observations()[before..],
            &[NavigatorObservation::Remove {
                route: 2,
                previous: Some(1)
            }],
            "no top change, because the reader is still on 3"
        );

        assert!(!navigator.remove(99));
    }

    #[test]
    fn replacing_the_top_route_is_a_top_change_and_replacing_below_is_not() {
        let mut navigator = NavigatorState::new();
        navigator.push(1);
        navigator.push(2);

        let before = navigator.observations().len();
        navigator.replace(1, 7);
        assert_eq!(navigator.history(), &[7, 2]);
        assert_eq!(
            &navigator.observations()[before..],
            &[NavigatorObservation::Replace {
                new_route: Some(7),
                old_route: Some(1)
            }]
        );

        let before = navigator.observations().len();
        navigator.replace(2, 8);
        assert_eq!(
            &navigator.observations()[before..],
            &[
                NavigatorObservation::Replace {
                    new_route: Some(8),
                    old_route: Some(2)
                },
                NavigatorObservation::ChangeTop {
                    top: 8,
                    previous: Some(2)
                },
            ]
        );
    }

    #[test]
    fn a_user_gesture_is_announced_so_the_hero_controller_can_stand_down() {
        // A hero flying between two pages while a finger drags one of them
        // fights the finger.
        let mut navigator = NavigatorState::new();
        navigator.push(1);
        navigator.push(2);
        let before = navigator.observations().len();

        navigator.did_start_user_gesture();
        assert!(navigator.user_gesture_in_progress());
        assert_eq!(
            &navigator.observations()[before..],
            &[NavigatorObservation::StartUserGesture {
                route: 2,
                previous: Some(1)
            }]
        );

        navigator.did_stop_user_gesture();
        assert!(!navigator.user_gesture_in_progress());
        assert_eq!(
            navigator.observations().last(),
            Some(&NavigatorObservation::StopUserGesture)
        );
    }

    #[test]
    fn applying_a_transition_keeps_only_what_is_staying() {
        let mut navigator = NavigatorState::new();
        let mut request = TransitionRequest::new()
            .with_history(vec![
                RouteTransitionRecord::entering(1),
                RouteTransitionRecord::entering(2),
            ])
            .exiting_at(None, RouteTransitionRecord::exiting(8));
        let results = DefaultTransitionDelegate::new().resolve(&mut request);
        navigator.apply_transition(&results);
        assert_eq!(
            navigator.history(),
            &[1, 2],
            "the completed route is gone from the stack at once"
        );
    }

    // -- The rest ----------------------------------------------------------

    #[test]
    fn a_hero_controller_is_taken_by_the_first_navigator_and_not_shared() {
        // Two navigators sharing one would be two histories disagreeing about
        // where the hero is.
        let mut scope = HeroControllerScope::new(5);
        assert_eq!(scope.claim(1), Some(5));
        assert_eq!(scope.claimed_by(), Some(1));
        assert_eq!(scope.claim(2), None, "barred");
        assert_eq!(scope.claim(1), Some(5), "the owner may ask again");

        scope.release(1);
        assert_eq!(scope.claim(2), Some(5));
    }

    #[test]
    fn a_scope_with_no_controller_is_a_deliberate_full_stop() {
        // HeroControllerScope.none exists to say "do not inherit the one
        // above", which a nested navigator running its own transitions needs.
        let mut none = HeroControllerScope::none();
        assert_eq!(none.claim(1), None);
        assert_eq!(none.claimed_by(), None);
    }

    #[test]
    fn a_restorable_future_stores_an_id_because_a_route_cannot_be_written_down() {
        // A restored application has no route objects yet.
        let mut future = RestorableRouteFuture::new();
        assert_eq!(future.create_default_value(), None);
        assert!(!future.is_present());

        future.present("dialog-1");
        assert!(future.is_present());
        assert_eq!(future.route(), Some("dialog-1"));

        future.complete(Some("ok".to_string()));
        assert!(!future.is_present());
        assert_eq!(future.completed_with(), Some(Some("ok")));
    }

    #[test]
    fn a_restored_id_hooks_back_on_without_presenting_again() {
        let mut future = RestorableRouteFuture::new();
        future.init_with_value(Some("dialog-1".to_string()));
        assert!(
            future.is_present(),
            "the route is already on screen; nobody pushed it this run"
        );
    }

    #[test]
    fn the_notification_travels_up_so_the_top_knows_not_to_close_the_app() {
        assert!(NavigationNotification::new(true).can_handle_pop);
        assert!(!NavigationNotification::new(false).can_handle_pop);
    }

    #[test]
    fn an_anonymous_route_is_one_that_never_had_a_name() {
        assert!(RouteSettings::new().is_anonymous());
        assert!(!RouteSettings::named("/settings").is_anonymous());
        assert_eq!(
            RouteSettings::named("/book")
                .with_arguments("42")
                .arguments
                .as_deref(),
            Some("42")
        );
    }

    // -- Page ---------------------------------------------------------------------

    #[test]
    fn two_pages_of_the_same_kind_and_key_are_the_same_page() {
        let before = Page::new("Home").with_key("home").with_name("/");
        let after = Page::new("Home").with_key("home").with_name("/");
        assert!(before.can_update(&after));
    }

    #[test]
    fn a_different_key_is_a_different_page() {
        let a = Page::new("Detail").with_key("item-1");
        let b = Page::new("Detail").with_key("item-2");
        assert!(!a.can_update(&b));
    }

    #[test]
    fn a_different_kind_never_matches_however_the_keys_compare() {
        // Upstream's `runtimeType` half. A HomePage does not update into a
        // SettingsPage even if somebody gave them the same key.
        let home = Page::new("Home").with_key("same");
        let settings = Page::new("Settings").with_key("same");
        assert!(!home.can_update(&settings));

        let unkeyed_home = Page::new("Home");
        let unkeyed_settings = Page::new("Settings");
        assert!(!unkeyed_home.can_update(&unkeyed_settings));
    }

    #[test]
    fn two_keyless_pages_of_one_kind_do_match() {
        // The case the intuition gets backwards. "No key" reads like "no
        // identity", and in Dart `null == null` is true -- so a list of unkeyed
        // pages matches position for position, which is why reordering them
        // looks like their contents changed and why upstream tells you to add
        // keys when it does.
        let a = Page::new("Item").with_name("first");
        let b = Page::new("Item").with_name("second");
        assert!(
            a.can_update(&b),
            "same kind, both keyless: the navigator reuses the route"
        );
    }

    #[test]
    fn the_name_and_arguments_do_not_decide_identity() {
        // Only the kind and the key. A page whose arguments changed is still
        // the same page, which is the point -- the route is updated rather than
        // replaced.
        let a = Page::new("Detail").with_key("k").with_arguments("id=1");
        let b = Page::new("Detail").with_key("k").with_arguments("id=2");
        assert!(a.can_update(&b));
    }

    #[test]
    fn a_page_is_poppable_unless_it_says_otherwise() {
        assert!(Page::new("Home").can_pop);
        assert!(!Page::new("Home").with_can_pop(false).can_pop);
    }

    #[test]
    fn a_page_carries_the_settings_a_route_was_built_from() {
        let page = Page::new("Detail")
            .with_name("/detail")
            .with_arguments("id=7")
            .with_restoration_id("detail-7");
        assert_eq!(page.settings.name.as_deref(), Some("/detail"));
        assert_eq!(page.settings.arguments.as_deref(), Some("id=7"));
        assert_eq!(page.restoration_id.as_deref(), Some("detail-7"));
        assert!(!page.settings.is_anonymous());
    }

    #[test]
    fn a_pages_debug_form_is_upstreams_to_string() {
        let page = Page::new("Home").with_name("/").with_key("home");
        let shown = format!("{page:?}");
        assert!(shown.starts_with("Page(\"/\""), "{shown}");
        assert!(shown.contains("home"), "{shown}");
    }
}
