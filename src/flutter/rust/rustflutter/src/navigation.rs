// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! A navigation stack.
//!
//! A screen that can drill into another screen and come back is a stack, and
//! the reason it needs framework support rather than a boolean in the app's
//! state is the coming back: during a transition *both* screens exist, the
//! outgoing one has to keep the state it will be restored to, and whichever
//! one is on top has to be the one that gets the taps.
//!
//! # What a route is here
//!
//! A [`Route`] is a name plus whatever the screen needs to know -- a
//! [`RouteArgs`] of strings and numbers rather than a typed payload, because a
//! typed one would make the stack generic over every screen's parameters and
//! push that generic into everything that touches it. The app matches on the
//! name in one place and reads what it needs.
//!
//! # What is here and what is not
//!
//! Push, pop, replace, pop-to-root, a slide-and-fade transition, and a back
//! gesture hook. Not here: named route tables with declarative arguments, or
//! nested navigators. Both are worth having and neither changes the shape of
//! this file.

use std::collections::HashMap;
use std::time::Duration;

use crate::animation::{Controller, Curve};

/// The parameters a route was pushed with.
///
/// Deliberately untyped: a typed payload would make [`Route`] generic, and that
/// parameter would then appear in the navigator, the stack, the transition and
/// every signature that touches any of them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RouteArgs {
    values: HashMap<String, String>,
}

impl RouteArgs {
    pub fn new() -> RouteArgs {
        RouteArgs::default()
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.values.insert(key.into(), value.into());
        self
    }

    pub fn with_int(self, key: impl Into<String>, value: i64) -> Self {
        self.with(key, value.to_string())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn get_int(&self, key: &str) -> Option<i64> {
        self.get(key)?.parse().ok()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// One entry on the stack.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub name: String,
    pub args: RouteArgs,
}

impl Route {
    pub fn new(name: impl Into<String>) -> Route {
        Route {
            name: name.into(),
            args: RouteArgs::new(),
        }
    }

    pub fn with_args(mut self, args: RouteArgs) -> Route {
        self.args = args;
        self
    }

    pub fn arg(&self, key: &str) -> Option<&str> {
        self.args.get(key)
    }

    pub fn arg_int(&self, key: &str) -> Option<i64> {
        self.args.get_int(key)
    }
}

/// How the incoming and outgoing screens move past each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Transition {
    /// No animation. The stack changes between one frame and the next.
    None,
    /// The new screen slides in from the right as the old one fades back.
    #[default]
    SlideFromRight,
    /// The new screen rises from the bottom. For something modal.
    SlideFromBottom,
    /// A crossfade, for a change of context rather than depth.
    Fade,
}

/// Which way the stack is moving, which decides which screen is on top and
/// which way the transition runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Pushing,
    Popping,
}

/// What is on screen right now.
///
/// During a transition there are two routes and a progress value; the rest of
/// the time `previous` is None and `progress` is 1.
#[derive(Clone, Debug)]
pub struct Presentation<'a> {
    /// The route that is (or is becoming) the top of the stack.
    pub current: &'a Route,
    /// The one it is moving past, if a transition is running.
    pub previous: Option<&'a Route>,
    /// 0 at the start of the transition, 1 when it is done. Already curved.
    pub progress: f32,
    pub transition: Transition,
    pub motion: Motion,
}

impl Presentation<'_> {
    /// Whether a transition is under way. A build can skip the second screen
    /// entirely when it is not.
    pub fn is_transitioning(&self) -> bool {
        self.previous.is_some() && self.progress < 1.0
    }
}

/// A route and the transition it arrived with.
///
/// The transition belongs to the entry rather than to the navigator: a screen
/// pushed with a fade should leave with a fade, whatever some later push chose,
/// and a single field on the navigator gets that wrong the moment two routes on
/// the stack arrived differently.
#[derive(Clone, Debug)]
struct Entry {
    route: Route,
    transition: Transition,
    /// What this route hands back when it goes -- upstream's `Route.popped`
    /// and `currentResult`, ported as
    /// [`crate::routes::RouteCompletion`].
    ///
    /// It lives on the entry rather than on the [`Route`] for the reason
    /// upstream keeps `pendingResult` on `_RouteEntry`: a `Route` here is a
    /// name and some arguments, a value the application hands in and keeps,
    /// and a result is something the *stack* delivers on its way out.
    completion: crate::routes::RouteCompletion,
}

/// The route stack.
///
/// Owned by the application's state, ticked from `begin_frame`, and read during
/// `build`. It holds no widgets: what a route looks like is the app's business,
/// and keeping the two apart is what lets the stack be tested without a screen.
#[derive(Clone, Debug)]
pub struct Navigator {
    stack: Vec<Entry>,
    /// The **entry** being animated away, kept alive until the transition
    /// ends.
    ///
    /// The whole entry rather than its route: what a leaving route handed back
    /// is on the entry, and a caller usually asks for it in the same breath as
    /// the pop -- while the animation is still running. Keeping only the route
    /// dropped the answer at exactly the moment anyone wanted it.
    outgoing: Option<Entry>,
    controller: Controller,
    /// The transition currently running, taken from whichever entry moved.
    transition: Transition,
    motion: Motion,
    duration: Duration,
}

impl Navigator {
    /// Starts with `root` on the stack. A navigator is never empty; popping the
    /// last route is refused rather than leaving nothing to draw.
    pub fn new(root: Route) -> Navigator {
        Navigator {
            // The root arrived from nowhere, so it has no transition of its
            // own; it can never be popped, so none is ever needed.
            stack: vec![Entry {
                route: root,
                transition: Transition::None,
                completion: crate::routes::RouteCompletion::new(),
            }],
            outgoing: None,
            controller: Controller::new(Duration::from_millis(300)).with_curve(Curve::EaseInOut),
            transition: Transition::default(),
            motion: Motion::Pushing,
            duration: Duration::from_millis(300),
        }
    }

    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self.controller = Controller::new(duration).with_curve(Curve::EaseInOut);
        self
    }

    /// How deep the stack is. One means the root.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    pub fn can_pop(&self) -> bool {
        self.stack.len() > 1
    }

    /// The route on top.
    pub fn current(&self) -> &Route {
        // The stack is never empty, so this cannot fail; the expect documents
        // that rather than hiding it behind an Option every caller unwraps.
        &self
            .stack
            .last()
            .expect("the navigator always has a root")
            .route
    }

    /// Every route on the stack, root first.
    pub fn routes(&self) -> Vec<&Route> {
        self.stack.iter().map(|entry| &entry.route).collect()
    }

    /// Pushes a route, animating with `transition`.
    ///
    /// A push while a transition is running replaces the outgoing route rather
    /// than queueing: the user asked for the newest destination, and animating
    /// through an intermediate screen they never see is worse than skipping it.
    pub fn push(&mut self, route: Route, transition: Transition) {
        self.outgoing = self.stack.last().cloned();
        self.stack.push(Entry {
            route,
            transition,
            completion: crate::routes::RouteCompletion::new(),
        });
        self.begin(transition, Motion::Pushing);
    }

    /// [`Navigator::push`] for a screen that will hand something back, and the
    /// value it hands back if it is dismissed without choosing.
    ///
    /// Upstream's `currentResult`: a picker closed by the back gesture answers
    /// with whatever was selected at the time, not with nothing. See
    /// [`crate::routes::RouteCompletion`].
    pub fn push_expecting(
        &mut self,
        route: Route,
        transition: Transition,
        if_dismissed: impl Into<String>,
    ) {
        self.push(route, transition);
        let entry = self.stack.last_mut().expect("just pushed");
        entry.completion = entry.completion.clone().with_current_result(if_dismissed);
    }

    /// What the route at `depth` handed back, counting from the bottom.
    ///
    /// `None` while it is still on the stack, `Some(None)` once it has gone
    /// with nothing to say -- the two are different questions, which is why
    /// they are not flattened. See [`crate::routes::RouteCompletion::popped`].
    ///
    /// The answer survives the pop: the entry is kept until the transition
    /// ends, and a caller that asks during the animation gets the same answer
    /// it will get after.
    pub fn result_at(&self, depth: usize) -> Option<Option<&str>> {
        if let Some(entry) = self.stack.get(depth) {
            return entry.completion.popped();
        }
        self.outgoing
            .as_ref()
            .filter(|_| depth == self.stack.len())
            .and_then(|entry| entry.completion.popped())
    }

    /// Pops the top route. Returns false if only the root is left.
    pub fn pop(&mut self) -> bool {
        self.pop_with_result(None).is_some()
    }

    /// [`Navigator::pop`] with the value the leaving screen hands back --
    /// upstream's `Navigator.pop(context, result)`.
    ///
    /// A `None` here is not "nothing": the route's own `currentResult` is used
    /// instead, if it named one when it was pushed. See
    /// [`crate::routes::RouteCompletion::did_complete`].
    /// Answers what the leaving screen handed back -- or `None` when the pop
    /// was refused, which is the only reason nothing happened.
    ///
    /// The value is **returned** rather than only left on the entry, because
    /// the entry does not always outlive the call: a pop with no transition
    /// drops it in the same breath. Upstream's caller awaits a future and is
    /// handed the value; this is the same handover without one.
    pub fn pop_with_result(&mut self, result: Option<String>) -> Option<Option<String>> {
        if !self.can_pop() {
            return None;
        }
        let mut popped = self.stack.pop().expect("checked by can_pop");
        // A route leaves the way it arrived.
        let transition = popped.transition;
        popped.completion.did_complete(result);
        let handed_back = popped
            .completion
            .popped()
            .expect("just completed")
            .map(|value| value.to_string());
        self.outgoing = Some(popped);
        self.begin(transition, Motion::Popping);
        Some(handed_back)
    }

    /// Replaces the top route without changing the depth.
    pub fn replace(&mut self, route: Route, transition: Transition) {
        let mut previous = self.stack.pop();
        self.stack.push(Entry {
            route,
            transition,
            completion: crate::routes::RouteCompletion::new(),
        });
        // Upstream reaches `didComplete` through the entry's lifecycle here
        // too -- `pushReplacement` completes the route it replaced, with
        // nothing, which is how a screen that is replaced rather than popped
        // still finishes rather than hanging on a future nobody will complete.
        if let Some(previous) = previous.as_mut() {
            previous.completion.did_complete(None);
        }
        self.outgoing = previous;
        self.begin(transition, Motion::Pushing);
    }

    /// Pops everything above the root, answering what each screen it took away
    /// handed back -- bottom first, the top one last.
    ///
    /// `None` when there was nothing to pop. Every removed screen finishes,
    /// not only the one on top, and the values are **returned** because the
    /// buried entries do not survive the call: a screen nobody will see again
    /// still has an answer somebody may be waiting for.
    pub fn pop_to_root(&mut self) -> Option<Vec<Option<String>>> {
        if !self.can_pop() {
            return None;
        }
        let mut popped = self.stack.pop().expect("checked by can_pop");
        // **Everything** taken away finishes, not only the one on top. The
        // screens in the middle are never seen again either, and a future
        // nobody completes is a wait that never ends -- upstream reaches
        // `didComplete` for each removed entry through its lifecycle.
        let mut answers = Vec::new();
        for mut buried in self.stack.drain(1..) {
            buried.completion.did_complete(None);
            answers.push(
                buried
                    .completion
                    .popped()
                    .expect("just completed")
                    .map(|value| value.to_string()),
            );
        }
        let transition = popped.transition;
        popped.completion.did_complete(None);
        answers.push(
            popped
                .completion
                .popped()
                .expect("just completed")
                .map(|value| value.to_string()),
        );
        self.outgoing = Some(popped);
        self.begin(transition, Motion::Popping);
        Some(answers)
    }

    fn begin(&mut self, transition: Transition, motion: Motion) {
        self.transition = transition;
        self.motion = motion;
        if transition == Transition::None {
            self.outgoing = None;
            self.controller.set_value(1.0);
            self.controller.stop();
            return;
        }
        self.controller = Controller::new(self.duration).with_curve(Curve::EaseInOut);
        self.controller.forward();
    }

    /// Advances the transition. Returns whether one is still running, which the
    /// caller uses to decide whether to ask for another frame.
    pub fn tick(&mut self, elapsed: Duration) -> bool {
        if self.outgoing.is_none() {
            return false;
        }
        let advanced = self.controller.tick(elapsed);
        if !self.controller.is_running() {
            // The outgoing route is only dropped here, so its state survives
            // for as long as it is on screen. Asked of the controller rather
            // than of the tick's answer: a tick that arrives at the end
            // reports that it did something -- that is what gets the last
            // frame drawn -- and the transition is over all the same.
            self.outgoing = None;
        }
        advanced
    }

    pub fn is_transitioning(&self) -> bool {
        self.outgoing.is_some()
    }

    /// What to draw this frame.
    pub fn presentation(&self) -> Presentation<'_> {
        Presentation {
            current: self.current(),
            previous: self.outgoing.as_ref().map(|entry| &entry.route),
            progress: if self.outgoing.is_some() {
                self.controller.curved()
            } else {
                1.0
            },
            transition: self.transition,
            motion: self.motion,
        }
    }
}

/// Where the two screens sit during a transition, as fractions of the view.
///
/// Returned rather than applied so the caller decides whether to spend a
/// transform layer, an opacity layer, or neither -- a fully settled transition
/// costs nothing if the caller checks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransitionOffsets {
    /// Horizontal offset of the incoming screen, in fractions of the width.
    pub current_dx: f32,
    pub current_dy: f32,
    pub current_opacity: f32,
    pub previous_dx: f32,
    pub previous_dy: f32,
    pub previous_opacity: f32,
}

impl TransitionOffsets {
    /// Everything in place, nothing faded. What a settled stack looks like.
    pub const SETTLED: TransitionOffsets = TransitionOffsets {
        current_dx: 0.0,
        current_dy: 0.0,
        current_opacity: 1.0,
        previous_dx: 0.0,
        previous_dy: 0.0,
        previous_opacity: 1.0,
    };
}

impl Presentation<'_> {
    /// Resolves the transition into offsets and opacities.
    ///
    /// There is only one animation here, described once and read from both
    /// ends. `t` runs 0..1 from "the deeper screen is entirely off to the
    /// right" to "it has arrived"; a pop is that same animation played
    /// backwards, so it runs `t` the other way and swaps which of `current`
    /// and `previous` is the deeper one.
    pub fn offsets(&self) -> TransitionOffsets {
        if !self.is_transitioning() {
            return TransitionOffsets::SETTLED;
        }
        let t = match self.motion {
            Motion::Pushing => self.progress,
            Motion::Popping => 1.0 - self.progress,
        };

        // Where the two screens are at `t`, by depth rather than by role.
        let (deep, shallow) = match self.transition {
            Transition::None => ((0.0, 0.0, 1.0), (0.0, 0.0, 1.0)),
            Transition::SlideFromRight => (
                // The deeper screen slides in from the right...
                (1.0 - t, 0.0, 1.0),
                // ...while the shallower one drifts a little to the left and
                // dims, which is what gives the stack its sense of depth.
                (-0.25 * t, 0.0, 1.0 - 0.35 * t),
            ),
            Transition::SlideFromBottom => ((0.0, 1.0 - t, 1.0), (0.0, 0.0, 1.0 - 0.2 * t)),
            Transition::Fade => ((0.0, 0.0, t), (0.0, 0.0, 1.0 - t)),
        };

        // On a push the top of the stack is the deeper screen; on a pop it is
        // the shallower one being revealed.
        let (current, previous) = match self.motion {
            Motion::Pushing => (deep, shallow),
            Motion::Popping => (shallow, deep),
        };

        TransitionOffsets {
            current_dx: current.0,
            current_dy: current.1,
            current_opacity: current.2,
            previous_dx: previous.0,
            previous_dy: previous.1,
            previous_opacity: previous.2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn navigator() -> Navigator {
        Navigator::new(Route::new("home")).with_duration(Duration::from_millis(100))
    }

    #[test]
    fn a_new_navigator_is_at_its_root() {
        let nav = navigator();
        assert_eq!(nav.depth(), 1);
        assert_eq!(nav.current().name, "home");
        assert!(!nav.can_pop());
        assert!(!nav.is_transitioning());
    }

    #[test]
    fn pushing_deepens_the_stack_and_starts_a_transition() {
        let mut nav = navigator();
        nav.push(Route::new("detail"), Transition::SlideFromRight);
        assert_eq!(nav.depth(), 2);
        assert_eq!(nav.current().name, "detail");
        assert!(nav.is_transitioning());

        let presentation = nav.presentation();
        assert_eq!(presentation.previous.map(|r| r.name.as_str()), Some("home"));
        assert!(presentation.progress < 1.0);
    }

    #[test]
    fn a_transition_ends_and_drops_the_outgoing_route() {
        let mut nav = navigator();
        nav.push(Route::new("detail"), Transition::SlideFromRight);
        assert!(nav.tick(Duration::from_millis(50)));
        assert!(nav.is_transitioning());
        // The tick that arrives still counts as work -- it is the frame that
        // puts the page in its final position -- and the transition is over by
        // the end of it.
        assert!(nav.tick(Duration::from_millis(60)));
        assert!(!nav.is_transitioning());
        assert_eq!(nav.presentation().progress, 1.0);
        assert!(!nav.tick(Duration::from_millis(16)), "and then it is idle");
    }

    #[test]
    fn popping_the_root_is_refused() {
        let mut nav = navigator();
        assert!(!nav.pop());
        assert_eq!(nav.depth(), 1);
    }

    #[test]
    fn popping_returns_to_the_previous_route() {
        let mut nav = navigator();
        nav.push(Route::new("detail"), Transition::SlideFromRight);
        nav.tick(Duration::from_millis(200));

        assert!(nav.pop());
        assert_eq!(nav.current().name, "home");
        // The popped route is still on screen while it animates away.
        assert_eq!(
            nav.presentation().previous.map(|r| r.name.as_str()),
            Some("detail")
        );
        nav.tick(Duration::from_millis(200));
        assert!(!nav.is_transitioning());
    }

    #[test]
    fn a_route_leaves_the_way_it_arrived() {
        let mut nav = navigator();
        nav.push(Route::new("faded"), Transition::Fade);
        nav.tick(Duration::from_millis(200));
        nav.push(Route::new("slid"), Transition::SlideFromRight);
        nav.tick(Duration::from_millis(200));

        // Popping the slid route uses a slide...
        nav.pop();
        assert_eq!(nav.presentation().transition, Transition::SlideFromRight);
        nav.tick(Duration::from_millis(200));

        // ...and popping the faded one underneath still uses a fade, even
        // though a slide happened in between.
        nav.pop();
        assert_eq!(nav.presentation().transition, Transition::Fade);
    }

    #[test]
    fn pop_to_root_collapses_everything() {
        let mut nav = navigator();
        nav.push(Route::new("a"), Transition::None);
        nav.push(Route::new("b"), Transition::None);
        nav.push(Route::new("c"), Transition::None);
        assert_eq!(nav.depth(), 4);
        assert!(nav.pop_to_root().is_some());
        assert_eq!(nav.depth(), 1);
        assert_eq!(nav.current().name, "home");
    }

    #[test]
    fn replace_keeps_the_depth() {
        let mut nav = navigator();
        nav.push(Route::new("a"), Transition::None);
        nav.replace(Route::new("b"), Transition::Fade);
        assert_eq!(nav.depth(), 2);
        assert_eq!(nav.current().name, "b");
    }

    #[test]
    fn a_route_with_no_transition_settles_immediately() {
        let mut nav = navigator();
        nav.push(Route::new("detail"), Transition::None);
        assert!(!nav.is_transitioning());
        assert_eq!(nav.presentation().progress, 1.0);
        assert_eq!(nav.presentation().offsets(), TransitionOffsets::SETTLED);
    }

    #[test]
    fn arguments_survive_the_round_trip() {
        let mut nav = navigator();
        nav.push(
            Route::new("demo").with_args(
                RouteArgs::new()
                    .with("slug", "buttons")
                    .with_int("index", 3),
            ),
            Transition::None,
        );
        assert_eq!(nav.current().arg("slug"), Some("buttons"));
        assert_eq!(nav.current().arg_int("index"), Some(3));
        assert_eq!(nav.current().arg("missing"), None);
    }

    #[test]
    fn a_slide_starts_the_new_screen_off_to_the_right() {
        let mut nav = navigator();
        nav.push(Route::new("detail"), Transition::SlideFromRight);
        let offsets = nav.presentation().offsets();
        // Barely started: the incoming screen is still almost a full width out.
        assert!(offsets.current_dx > 0.9);
        assert!(offsets.previous_dx <= 0.0);

        nav.tick(Duration::from_millis(200));
        assert_eq!(nav.presentation().offsets(), TransitionOffsets::SETTLED);
    }

    #[test]
    fn a_pop_runs_the_slide_backwards() {
        let mut nav = navigator();
        nav.push(Route::new("detail"), Transition::SlideFromRight);
        nav.tick(Duration::from_millis(200));
        nav.pop();
        let presentation = nav.presentation();
        assert_eq!(presentation.motion, Motion::Popping);
        let offsets = presentation.offsets();
        // The screen being revealed starts pushed to the left, and the one
        // leaving starts in place before sliding right.
        assert!(offsets.current_dx < 0.0);
        assert!(offsets.previous_dx < 0.1);
    }

    #[test]
    fn a_push_during_a_transition_takes_the_newest_destination() {
        let mut nav = navigator();
        nav.push(Route::new("a"), Transition::SlideFromRight);
        nav.tick(Duration::from_millis(20));
        nav.push(Route::new("b"), Transition::SlideFromRight);

        assert_eq!(nav.current().name, "b");
        // The screen it animates past is the one that was on top, not the root.
        assert_eq!(
            nav.presentation().previous.map(|r| r.name.as_str()),
            Some("a")
        );
        assert_eq!(nav.depth(), 3);
    }

    #[test]
    fn ticking_a_settled_navigator_asks_for_nothing() {
        let mut nav = navigator();
        assert!(!nav.tick(Duration::from_millis(16)));
    }

    // -- What a screen hands back --------------------------------------------

    #[test]
    fn a_popped_screen_hands_its_answer_back() {
        // Upstream's `Navigator.pop(context, result)` reaching the route's
        // `popped` future. The stack had no channel for this at all: `pop`
        // answered a bool and the value went nowhere.
        let mut navigator = Navigator::new(Route::new("home"));
        navigator.push(Route::new("picker"), Transition::None);
        assert_eq!(navigator.result_at(1), None, "still open");

        assert_eq!(
            navigator.pop_with_result(Some("the second one".to_string())),
            Some(Some("the second one".to_string()))
        );

        // And a pop that is refused answers `None` -- the outer one, which is
        // "nothing happened" rather than "nothing was handed back". The two
        // are told apart by the nesting, which is the whole reason for it.
        assert_eq!(
            navigator.pop_with_result(None),
            None,
            "only the root is left, so nothing was popped at all"
        );
    }

    #[test]
    fn a_screen_dismissed_without_choosing_hands_back_what_it_was_showing() {
        // `result ?? currentResult`. A picker closed by the back gesture
        // answers with whatever was selected at the time -- which is the
        // difference between a caller seeing "nothing" and seeing the
        // selection the reader was looking at.
        let mut navigator = Navigator::new(Route::new("home"));
        navigator.push_expecting(Route::new("picker"), Transition::None, "what was shown");
        assert_eq!(
            navigator.pop_with_result(None),
            Some(Some("what was shown".to_string()))
        );

        // And a screen that named no fallback answers nothing -- while still
        // having finished, which is the other question.
        let mut plain = Navigator::new(Route::new("home"));
        plain.push(Route::new("page"), Transition::None);
        assert_eq!(
            plain.pop_with_result(None),
            Some(None),
            "it finished, with nothing to say"
        );
    }

    #[test]
    fn the_answer_is_there_while_the_screen_is_still_sliding_away() {
        // The entry is kept until the transition ends -- and a caller asks for
        // the result in the same breath as the pop, which is *during* the
        // animation. Keeping only the route dropped the answer at exactly the
        // moment anyone wanted it, which is why `outgoing` holds the entry.
        let mut navigator = Navigator::new(Route::new("home"));
        navigator.push(Route::new("picker"), Transition::SlideFromRight);
        assert_eq!(
            navigator.pop_with_result(Some("chosen".to_string())),
            Some(Some("chosen".to_string()))
        );
        assert!(navigator.is_transitioning(), "still on screen");
        assert_eq!(
            navigator.result_at(1),
            Some(Some("chosen")),
            "and already answered"
        );
    }

    #[test]
    fn popping_to_the_root_finishes_everything_it_takes_away() {
        // The same rule as replacing, by the other road: a screen removed
        // without being popped one at a time still has a future somebody may
        // be waiting on. Leaving it uncompleted is a wait that never ends.
        let mut navigator = Navigator::new(Route::new("home"));
        navigator.push(Route::new("first"), Transition::None);
        navigator.push(Route::new("second"), Transition::SlideFromRight);
        // Two screens were taken away, and both finished -- the one in the
        // middle nobody will ever see again included. Its answer has to come
        // back here, because its entry does not survive the call.
        let mut expecting = Navigator::new(Route::new("home"));
        expecting.push_expecting(Route::new("first"), Transition::None, "the lower one");
        expecting.push_expecting(Route::new("second"), Transition::None, "the upper one");
        expecting.push(Route::new("third"), Transition::SlideFromRight);
        assert_eq!(
            expecting.pop_to_root(),
            Some(vec![
                Some("the lower one".to_string()),
                Some("the upper one".to_string()),
                None
            ]),
            "bottom first, the top one last -- two buried screens, in the              order they were stacked"
        );

        assert_eq!(
            navigator.pop_to_root(),
            Some(vec![None, None]),
            "and screens with nothing to say still finish"
        );
        assert_eq!(
            navigator.result_at(1),
            Some(None),
            "the one on top is still on screen, and already answered"
        );
    }

    #[test]
    fn a_replaced_screen_finishes_rather_than_hanging() {
        // `pushReplacement` completes the route it replaced, with nothing.
        // A screen that is replaced rather than popped still has a future
        // somebody may be waiting on, and leaving it uncompleted is a wait
        // that never ends.
        let mut navigator = Navigator::new(Route::new("home"));
        navigator.push(Route::new("first"), Transition::None);
        navigator.replace(Route::new("second"), Transition::SlideFromRight);
        assert_eq!(
            navigator.result_at(1),
            None,
            "the entry at that depth is the new one, still open"
        );
        assert_eq!(
            navigator.result_at(2),
            Some(None),
            "and the one it replaced has finished, with nothing to say"
        );
    }
}
