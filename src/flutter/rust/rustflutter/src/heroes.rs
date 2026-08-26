//! A port of `widgets/heroes.dart`.
//!
//! A hero is a widget that appears to fly from one route to the next, because
//! two routes each contain a widget with the same tag. Nothing actually moves:
//! both heroes are hidden for the duration and a third copy is flown in the
//! navigator's overlay between the two rectangles.
//!
//! What is ported here is the part that decides *whether* a flight happens and
//! *when* it can be measured -- `HeroController._maybeStartHeroTransition`.
//! The flight itself needs an overlay that hosts widgets, which this crate does
//! not have yet; `crate::overlay` keeps the entry list but nothing renders it.

use std::collections::HashMap;

/// Upstream `HeroFlightDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroFlightDirection {
    /// The hero flies from the route being pushed onto the one below it, so it
    /// starts at the old route's rectangle and ends at the new one's.
    Push,
    /// The reverse: it starts where the route being popped had it.
    Pop,
}

/// The status of a route's transition animation, as far as the controller
/// cares. Upstream reads `AnimationStatus` off the route's animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteAnimationStatus {
    Dismissed,
    Forward,
    Reverse,
    Completed,
}

/// Upstream `Hero`.
///
/// It builds nothing of its own -- it is a marker that the machinery finds by
/// walking the route's subtree. Its fields are all instructions to the flight.
#[derive(Clone, Debug, PartialEq)]
pub struct Hero {
    /// The identifier that pairs this hero with one on the other route. Two
    /// heroes in the same route subtree may not share a tag.
    pub tag: String,
    /// Whether this hero flies for transitions the user is driving with a
    /// gesture, as opposed to ones a push or pop started. Defaults to `false`:
    /// a back-swipe that the reader may abandon halfway is not a good moment to
    /// tear a widget out of the page.
    pub transition_on_user_gestures: bool,
    /// Upstream defaults this to `Curves.fastOutSlowIn`.
    pub curve: &'static str,
    /// The subtree the hero wraps, as an id.
    pub child: u64,
}

impl Hero {
    pub fn new(tag: impl Into<String>, child: u64) -> Hero {
        Hero {
            tag: tag.into(),
            transition_on_user_gestures: false,
            curve: "fastOutSlowIn",
            child,
        }
    }

    pub fn with_transition_on_user_gestures(mut self, transition: bool) -> Self {
        self.transition_on_user_gestures = transition;
        self
    }

    /// Upstream `Hero.build` returns the child inside a `_HeroMarker`; from the
    /// outside a hero is its child.
    pub fn build(&self) -> u64 {
        self.child
    }
}

/// Upstream `HeroMode`.
///
/// The whole class is `Widget build(BuildContext context) => child;`. It paints
/// nothing, wraps nothing and changes no layout: it exists so that
/// `_allHeroesFor` can see, while walking the subtree, that heroes below this
/// point are switched off. A widget that is only findable is still a widget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeroMode {
    pub child: u64,
    /// Defaults to `true`.
    pub enabled: bool,
}

impl HeroMode {
    pub fn new(child: u64) -> HeroMode {
        HeroMode {
            child,
            enabled: true,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Upstream's entire `build`.
    pub fn build(&self) -> u64 {
        self.child
    }
}

/// One hero found in a route's subtree.
#[derive(Clone, Debug, PartialEq)]
pub struct HeroEntry {
    pub tag: String,
    pub transition_on_user_gestures: bool,
    /// Upstream skips heroes whose `Navigator.of` is not this controller's
    /// navigator, so a nested navigator's heroes are not stolen by the outer
    /// one.
    pub navigator: u64,
    /// Whether a previous flight left this hero hidden.
    pub hidden: bool,
    /// Whether the enclosing `HeroMode` is enabled.
    pub in_enabled_mode: bool,
}

impl HeroEntry {
    pub fn new(tag: impl Into<String>, navigator: u64) -> HeroEntry {
        HeroEntry {
            tag: tag.into(),
            transition_on_user_gestures: false,
            navigator,
            hidden: false,
            in_enabled_mode: true,
        }
    }

    pub fn with_transition_on_user_gestures(mut self, transition: bool) -> Self {
        self.transition_on_user_gestures = transition;
        self
    }

    pub fn hidden(mut self) -> Self {
        self.hidden = true;
        self
    }

    pub fn with_hero_mode(mut self, enabled: bool) -> Self {
        self.in_enabled_mode = enabled;
        self
    }
}

/// Why a subtree yielded no heroes, or which ones it yielded.
#[derive(Clone, Debug, PartialEq)]
pub struct CollectedHeroes {
    /// Tag to index in the source list, for the heroes that will fly.
    pub flying: HashMap<String, usize>,
    /// The heroes that were passed over and had to be un-hidden. Upstream calls
    /// `endFlight` on each: excluding a hero from a flight is not the same as
    /// leaving it alone, because a previous flight may have hidden it.
    pub ended: Vec<usize>,
}

/// The route state `_maybeStartHeroTransition` reads.
#[derive(Clone, Debug, PartialEq)]
pub struct HeroRoute {
    pub id: u64,
    /// Upstream requires both routes to be `PageRoute`s -- a dialog or a popup
    /// route does not replace the page under it, so there is nothing to fly
    /// between.
    pub is_page_route: bool,
    pub animation_status: RouteAnimationStatus,
    pub animation_value: f32,
    /// Whether the route keeps its subtree alive while covered.
    pub maintain_state: bool,
    /// Whether the route's render box has a size and that size is finite. A
    /// route added straight to the pages stack may never have been laid out.
    pub has_valid_size: bool,
    pub offstage: bool,
    /// Whether the route is still attached to a navigator. Upstream re-checks
    /// this inside the post-frame callback.
    pub attached: bool,
    pub heroes: Vec<HeroEntry>,
}

impl HeroRoute {
    pub fn new(id: u64) -> HeroRoute {
        HeroRoute {
            id,
            is_page_route: true,
            animation_status: RouteAnimationStatus::Dismissed,
            animation_value: 0.0,
            maintain_state: true,
            has_valid_size: true,
            offstage: false,
            attached: true,
            heroes: Vec::new(),
        }
    }

    pub fn with_animation(mut self, status: RouteAnimationStatus, value: f32) -> Self {
        self.animation_status = status;
        self.animation_value = value;
        self
    }

    pub fn with_hero(mut self, hero: HeroEntry) -> Self {
        self.heroes.push(hero);
        self
    }

    pub fn with_maintain_state(mut self, maintain: bool) -> Self {
        self.maintain_state = maintain;
        self
    }

    pub fn with_valid_size(mut self, valid: bool) -> Self {
        self.has_valid_size = valid;
        self
    }
}

/// What `_maybeStartHeroTransition` decided.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TransitionStart {
    /// The two routes were not a pair this controller acts on at all.
    Ignored,
    /// The flight was resolved and then abandoned, because the animation had
    /// already reached the end it was flying towards.
    AlreadyThere,
    /// Measured and started at once, without waiting for a frame.
    Immediate(HeroFlightDirection),
    /// Deferred to the end of the next frame so the "to" route can build and
    /// lay out. `to_offstage` is what the route's `offstage` was set to for the
    /// duration: putting a route offstage drives its animation value to 1.0, so
    /// the heroes can be measured where they are going to end up.
    ///
    /// `flight` is an `Option` because upstream schedules this pass even when
    /// no flight type could be worked out -- it is also how existing flights
    /// get ended.
    NextFrame {
        flight: Option<HeroFlightDirection>,
        to_offstage: bool,
    },
}

/// How a hero's rectangle is interpolated between its two ends -- upstream's
/// `HeroController.createRectTween`, and the one thing
/// `MaterialApp.createMaterialHeroController` and
/// `CupertinoApp.createCupertinoHeroController` disagree about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeroRectTween {
    /// The default `RectTween`: each corner moves independently and linearly,
    /// so the rectangle's **centre travels in a straight line**.
    ///
    /// Upstream writes this as the *absence* of an argument, with the whole
    /// documentation it gets on the same line:
    /// `=> HeroController(); // Linear tweening.`
    Linear,
    /// [`crate::arc::MaterialRectArcTween`]: two opposite corners swing on
    /// circular arcs, so the **centre travels on a curve**.
    Arc,
}

/// Upstream `HeroController`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HeroController {
    /// Whether the navigator is currently running a user gesture. Upstream
    /// reads `navigator!.userGestureInProgress`.
    pub user_gesture_in_progress: bool,
    /// The tags with a flight in the air, in the order they were started.
    flights: Vec<String>,
    /// The post-frame callbacks this controller has queued, by "to" route id.
    pending: Vec<u64>,
}

impl HeroController {
    pub fn new() -> HeroController {
        HeroController::default()
    }

    /// Upstream `MaterialApp.createMaterialHeroController`, which passes a
    /// `createRectTween` that builds a `MaterialRectArcTween`.
    ///
    /// A card expanding into a page sweeps rather than slides, which is the
    /// Material convention.
    pub fn for_material_app() -> HeroRectTween {
        HeroRectTween::Arc
    }

    /// Upstream `CupertinoApp.createCupertinoHeroController`, which passes
    /// **nothing** and so gets the default linear `RectTween`.
    ///
    /// The absence is the decision, and upstream marks it with a comment
    /// rather than an argument: `// Linear tweening.` An iOS push is itself a
    /// straight horizontal slide, and a hero curving inside it would fight the
    /// page it is riding on.
    pub fn for_cupertino_app() -> HeroRectTween {
        HeroRectTween::Linear
    }

    /// Whether the two apps' heroes would be drawn in the same place at
    /// `t`, given a flight between `begin` and `end`.
    ///
    /// **They agree whenever the arc degenerates**, which the arc tween itself
    /// decides: [`crate::arc::MaterialRectArcTween`] swings the diagonal that
    /// leads the motion, and a flight whose ends share a centre, or whose
    /// chosen corners do not move, has nothing to swing. So a hero that only
    /// changes size in place looks identical on both platforms, and a test
    /// built on one cannot tell the two apps apart.
    pub fn the_two_apps_agree(begin: crate::engine::Rect, end: crate::engine::Rect) -> bool {
        let arc = crate::arc::MaterialRectArcTween::new(begin, end);
        // Half way is where a circular arc is furthest from its chord, so it
        // is the frame that separates them if anything does.
        let arced = arc.lerp(0.5);
        let straight = crate::engine::Rect::ltrb(
            begin.left + (end.left - begin.left) * 0.5,
            begin.top + (end.top - begin.top) * 0.5,
            begin.right + (end.right - begin.right) * 0.5,
            begin.bottom + (end.bottom - begin.bottom) * 0.5,
        );
        let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
        close(arced.left, straight.left)
            && close(arced.top, straight.top)
            && close(arced.right, straight.right)
            && close(arced.bottom, straight.bottom)
    }

    /// Upstream `HeroFlightDirection` selection, which is a switch over the
    /// triple `(isUserGestureTransition, oldRoute.status, newRoute.status)`.
    ///
    /// A user gesture is always a pop, whatever the animations say -- the only
    /// gesture that drives a page transition is the back swipe. Otherwise a
    /// reversing *old* route is a pop and a forward-running *new* route is a
    /// push; note that each case looks at a different route.
    pub fn flight_type(
        is_user_gesture: bool,
        old_status: RouteAnimationStatus,
        new_status: RouteAnimationStatus,
    ) -> Option<HeroFlightDirection> {
        match (is_user_gesture, old_status, new_status) {
            (true, _, _) => Some(HeroFlightDirection::Pop),
            (_, RouteAnimationStatus::Reverse, _) => Some(HeroFlightDirection::Pop),
            (_, _, RouteAnimationStatus::Forward) => Some(HeroFlightDirection::Push),
            _ => None,
        }
    }

    /// Upstream `HeroController.didStopUserGesture` and the `didPush` guard.
    ///
    /// While a gesture is in progress no new flight is started, with upstream's
    /// reason written on it: "Don't trigger another flight when a pop is
    /// committed as a user gesture back swipe is snapped." The swipe already
    /// flew the heroes; letting the commit fly them a second time would restart
    /// the flight from wherever the reader let go.
    pub fn accepts_new_flight(&self) -> bool {
        !self.user_gesture_in_progress
    }

    /// Upstream `Hero._allHeroesFor`.
    ///
    /// The `else` branch is the interesting one: a hero that is not allowed to
    /// fly still gets `endFlight` called on it, because a previous flight may
    /// have hidden it and nothing else would put it back.
    pub fn collect_heroes(
        route: &HeroRoute,
        navigator: u64,
        is_user_gesture: bool,
    ) -> CollectedHeroes {
        let mut flying = HashMap::new();
        let mut ended = Vec::new();
        for (index, hero) in route.heroes.iter().enumerate() {
            if hero.navigator != navigator || !hero.in_enabled_mode {
                continue;
            }
            if !is_user_gesture || hero.transition_on_user_gestures {
                flying.insert(hero.tag.clone(), index);
            } else {
                ended.push(index);
            }
        }
        CollectedHeroes { flying, ended }
    }

    /// Upstream `HeroController._maybeStartHeroTransition`.
    pub fn maybe_start_hero_transition(
        &mut self,
        from: Option<&HeroRoute>,
        to: Option<&mut HeroRoute>,
        is_user_gesture: bool,
    ) -> TransitionStart {
        let (Some(from), Some(to)) = (from, to) else {
            return TransitionStart::Ignored;
        };
        if from.id == to.id || !from.is_page_route || !to.is_page_route {
            return TransitionStart::Ignored;
        }

        let flight = HeroController::flight_type(
            is_user_gesture,
            from.animation_status,
            to.animation_status,
        );

        // A user gesture may have already completed the pop, or we might be the
        // initial route. Each half looks at the route the flight is leaving
        // from or arriving at, and asks whether it is already there.
        match flight {
            Some(HeroFlightDirection::Pop) if from.animation_value == 0.0 => {
                return TransitionStart::AlreadyThere;
            }
            Some(HeroFlightDirection::Push) if to.animation_value == 1.0 => {
                return TransitionStart::AlreadyThere;
            }
            _ => {}
        }

        // For a pop the reader is driving, the "to" page is the one underneath,
        // and if it kept its state its layout is still valid -- so the final
        // rectangles can be measured now rather than a frame later.
        if is_user_gesture
            && flight == Some(HeroFlightDirection::Pop)
            && to.maintain_state
            && to.has_valid_size
        {
            self.start_hero_transition(from, to, flight);
            return TransitionStart::Immediate(HeroFlightDirection::Pop);
        }

        // Otherwise wait a frame, with the "to" route offstage so its animation
        // reads 1.0 and the heroes can be measured at their destinations.
        let to_offstage = to.animation_value == 0.0;
        to.offstage = to_offstage;
        self.pending.push(to.id);
        TransitionStart::NextFrame {
            flight,
            to_offstage,
        }
    }

    /// The post-frame half. Upstream re-checks that both routes still have a
    /// navigator, because a frame passed and either could have been disposed.
    pub fn run_pending_transition(
        &mut self,
        from: &HeroRoute,
        to: &mut HeroRoute,
        flight: Option<HeroFlightDirection>,
    ) -> bool {
        self.pending.retain(|id| *id != to.id);
        if !from.attached || !to.attached {
            return false;
        }
        self.start_hero_transition(from, to, flight);
        true
    }

    /// Upstream `_startHeroTransition`, as far as the bookkeeping goes.
    fn start_hero_transition(
        &mut self,
        from: &HeroRoute,
        to: &mut HeroRoute,
        flight: Option<HeroFlightDirection>,
    ) {
        // Restoring the animation value to what it was before the route was
        // "moved" offstage.
        to.offstage = false;
        let Some(_) = flight else {
            // No flight type: nothing new takes off, but the pass still ran,
            // which is what ends the flights already in the air.
            self.flights.clear();
            return;
        };
        let from_tags: Vec<&HeroEntry> = from.heroes.iter().collect();
        for hero in from_tags {
            if to.heroes.iter().any(|other| other.tag == hero.tag)
                && !self.flights.contains(&hero.tag)
            {
                self.flights.push(hero.tag.clone());
            }
        }
    }

    pub fn flights_in_the_air(&self) -> &[String] {
        &self.flights
    }

    pub fn has_pending_transition(&self) -> bool {
        !self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Which tween each app gives its heroes, tick 287 ---------------------

    #[test]
    fn the_two_apps_hand_their_heroes_different_tweens() {
        // Upstream writes the Material one as an argument and the Cupertino
        // one as its absence, with `// Linear tweening.` on the same line.
        assert_eq!(HeroController::for_material_app(), HeroRectTween::Arc);
        assert_eq!(HeroController::for_cupertino_app(), HeroRectTween::Linear);
        assert_ne!(
            HeroController::for_material_app(),
            HeroController::for_cupertino_app()
        );
    }

    #[test]
    fn a_hero_moving_across_the_screen_looks_different_on_the_two_apps() {
        // The arc's whole point: at the half-way frame, where a circular arc
        // is furthest from its chord, the two are not in the same place.
        let begin = crate::engine::Rect::ltrb(0.0, 0.0, 40.0, 40.0);
        let end = crate::engine::Rect::ltrb(200.0, 300.0, 260.0, 360.0);
        assert!(!HeroController::the_two_apps_agree(begin, end));
    }

    #[test]
    fn a_hero_moving_along_one_axis_looks_the_same_on_both() {
        // `MaterialPointArcTween` leaves a near-axial move straight -- the
        // `delta_x <= ON_AXIS_DELTA || delta_y <= ON_AXIS_DELTA` arm -- so a
        // hero sliding horizontally is drawn identically by the two apps. A
        // test built on a flight like this cannot tell them apart, which is
        // why the one above moves diagonally.
        let begin = crate::engine::Rect::ltrb(0.0, 0.0, 40.0, 40.0);
        let across = crate::engine::Rect::ltrb(200.0, 0.0, 240.0, 40.0);
        assert!(HeroController::the_two_apps_agree(begin, across));

        let down = crate::engine::Rect::ltrb(0.0, 300.0, 40.0, 340.0);
        assert!(HeroController::the_two_apps_agree(begin, down));
    }

    #[test]
    fn a_hero_that_only_changes_size_in_place_still_differs() {
        // The counterintuitive one, and the reason this had to be checked
        // rather than assumed: `MaterialRectArcTween` swings two *corners*,
        // not the centre. Ends that share a centre still have corners that
        // move diagonally, so the arc has something to swing and the two apps
        // part company even though the hero has not gone anywhere.
        let begin = crate::engine::Rect::ltrb(100.0, 100.0, 140.0, 140.0);
        let end = crate::engine::Rect::ltrb(80.0, 80.0, 160.0, 160.0);
        assert!(!HeroController::the_two_apps_agree(begin, end));
    }

    #[test]
    fn a_flight_can_disagree_on_one_edge_and_not_another() {
        // Moving up-left and a little down: the arc's chosen diagonal leaves
        // `left` exactly on the chord while `top` is nearly five pixels off
        // it. So "the two apps agree" has to look at every edge -- a check on
        // one corner alone reports agreement here, and the hero is visibly in
        // the wrong place.
        let begin = crate::engine::Rect::ltrb(0.0, 0.0, 40.0, 40.0);
        let end = crate::engine::Rect::ltrb(-200.0, 20.0, -180.0, 40.0);
        let arc = crate::arc::MaterialRectArcTween::new(begin, end).lerp(0.5);
        let midway = |a: f32, b: f32| a + (b - a) * 0.5;
        assert!(
            (arc.left - midway(begin.left, end.left)).abs() < 1e-3,
            "left is on the chord: {arc:?}"
        );
        assert!(
            (arc.top - midway(begin.top, end.top)).abs() > 1.0,
            "and top is not: {arc:?}"
        );
        assert!(!HeroController::the_two_apps_agree(begin, end));
    }

    #[test]
    fn a_flight_that_goes_nowhere_agrees_trivially() {
        let square = crate::engine::Rect::ltrb(10.0, 10.0, 50.0, 50.0);
        assert!(HeroController::the_two_apps_agree(square, square));
    }

    #[test]
    fn the_arc_is_the_one_that_leaves_the_straight_line() {
        // Which of the two is which: the Material path is the one that is not
        // the linear interpolation, and the Cupertino path *is* it.
        let begin = crate::engine::Rect::ltrb(0.0, 0.0, 40.0, 40.0);
        let end = crate::engine::Rect::ltrb(200.0, 300.0, 260.0, 360.0);
        let arc = crate::arc::MaterialRectArcTween::new(begin, end).lerp(0.5);
        let straight_top = begin.top + (end.top - begin.top) * 0.5;
        assert_ne!(
            arc.top, straight_top,
            "the arc leaves the chord at the half-way frame"
        );

        // And at the ends they must meet, or the hero would jump on arrival.
        let start = crate::arc::MaterialRectArcTween::new(begin, end).lerp(0.0);
        let finish = crate::arc::MaterialRectArcTween::new(begin, end).lerp(1.0);
        assert!((start.left - begin.left).abs() < 1e-3, "{start:?}");
        assert!((finish.left - end.left).abs() < 1e-3, "{finish:?}");
    }

    use RouteAnimationStatus::{Completed, Dismissed, Forward, Reverse};

    const NAV: u64 = 1;

    fn from_route() -> HeroRoute {
        HeroRoute::new(10).with_animation(Reverse, 0.5)
    }

    fn to_route() -> HeroRoute {
        HeroRoute::new(20).with_animation(Forward, 0.5)
    }

    // -- HeroMode ------------------------------------------------------------

    #[test]
    fn hero_mode_builds_its_child_and_does_nothing_else() {
        // The entire class is `build(context) => child`. It exists to be found
        // during the subtree walk, not to change what is drawn.
        let mode = HeroMode::new(7);
        assert_eq!(mode.build(), 7);
        assert!(mode.enabled, "a hero mode is on unless it is turned off");
        assert_eq!(mode.with_enabled(false).build(), 7, "still just the child");
    }

    #[test]
    fn a_hero_under_a_disabled_mode_is_passed_over() {
        let route = HeroRoute::new(10)
            .with_hero(HeroEntry::new("a", NAV))
            .with_hero(HeroEntry::new("b", NAV).with_hero_mode(false));
        let collected = HeroController::collect_heroes(&route, NAV, false);
        assert!(collected.flying.contains_key("a"));
        assert!(!collected.flying.contains_key("b"));
    }

    // -- Which flight, if any ------------------------------------------------

    #[test]
    fn a_gesture_driven_transition_is_a_pop_whatever_the_animations_say() {
        // The only gesture that drives a page transition is the back swipe.
        assert_eq!(
            HeroController::flight_type(true, Forward, Forward),
            Some(HeroFlightDirection::Pop)
        );
        assert_eq!(
            HeroController::flight_type(true, Completed, Dismissed),
            Some(HeroFlightDirection::Pop)
        );
    }

    #[test]
    fn a_pop_is_read_off_the_old_route_and_a_push_off_the_new_one() {
        // Each arm of the switch looks at a different route, which is why both
        // are needed rather than one status.
        assert_eq!(
            HeroController::flight_type(false, Reverse, Completed),
            Some(HeroFlightDirection::Pop)
        );
        assert_eq!(
            HeroController::flight_type(false, Completed, Forward),
            Some(HeroFlightDirection::Push)
        );
    }

    #[test]
    fn a_reversing_old_route_wins_over_a_forward_new_one() {
        // The pop arm comes first in the switch, so a pair that matches both
        // reads as a pop.
        assert_eq!(
            HeroController::flight_type(false, Reverse, Forward),
            Some(HeroFlightDirection::Pop)
        );
    }

    #[test]
    fn two_settled_routes_are_not_a_flight_at_all() {
        assert_eq!(
            HeroController::flight_type(false, Completed, Completed),
            None
        );
        assert_eq!(
            HeroController::flight_type(false, Dismissed, Dismissed),
            None
        );
    }

    // -- The two early returns -----------------------------------------------

    #[test]
    fn a_pop_from_a_route_already_gone_is_abandoned() {
        // A user gesture may have already completed the pop.
        let mut controller = HeroController::new();
        let from = from_route().with_animation(Reverse, 0.0);
        let mut to = to_route();
        assert_eq!(
            controller.maybe_start_hero_transition(Some(&from), Some(&mut to), false),
            TransitionStart::AlreadyThere
        );

        // While one still on its way flies normally.
        let from = from_route().with_animation(Reverse, 0.01);
        assert!(matches!(
            controller.maybe_start_hero_transition(Some(&from), Some(&mut to), false),
            TransitionStart::NextFrame { .. }
        ));
    }

    #[test]
    fn a_push_onto_a_route_already_arrived_is_abandoned() {
        // Which is what the initial route looks like.
        let mut controller = HeroController::new();
        let from = from_route().with_animation(Completed, 1.0);
        let mut to = to_route().with_animation(Forward, 1.0);
        assert_eq!(
            controller.maybe_start_hero_transition(Some(&from), Some(&mut to), false),
            TransitionStart::AlreadyThere
        );
    }

    #[test]
    fn a_dialog_over_a_page_is_not_a_hero_transition() {
        // A popup route does not replace the page under it, so there is nothing
        // to fly between.
        let mut controller = HeroController::new();
        let from = from_route();
        let mut to = to_route();
        to.is_page_route = false;
        assert_eq!(
            controller.maybe_start_hero_transition(Some(&from), Some(&mut to), false),
            TransitionStart::Ignored
        );
    }

    #[test]
    fn a_route_replacing_itself_is_ignored() {
        let mut controller = HeroController::new();
        let from = HeroRoute::new(10).with_animation(Reverse, 0.5);
        let mut to = HeroRoute::new(10).with_animation(Forward, 0.5);
        assert_eq!(
            controller.maybe_start_hero_transition(Some(&from), Some(&mut to), false),
            TransitionStart::Ignored
        );
    }

    // -- When it can be measured ---------------------------------------------

    #[test]
    fn a_gesture_pop_onto_a_page_that_kept_its_state_is_measured_at_once() {
        // Its layout is still valid, so there is nothing to wait a frame for.
        let mut controller = HeroController::new();
        let from = from_route();
        let mut to = to_route().with_maintain_state(true).with_valid_size(true);
        assert_eq!(
            controller.maybe_start_hero_transition(Some(&from), Some(&mut to), true),
            TransitionStart::Immediate(HeroFlightDirection::Pop)
        );
        assert!(!controller.has_pending_transition());
    }

    #[test]
    fn each_of_the_three_conditions_alone_sends_it_to_the_next_frame() {
        // A page that did not keep its state has to be rebuilt; one that was
        // never laid out has no size to read; and a push has no page underneath
        // that is already correct.
        let from = from_route();

        let mut controller = HeroController::new();
        let mut to = to_route().with_maintain_state(false);
        assert!(matches!(
            controller.maybe_start_hero_transition(Some(&from), Some(&mut to), true),
            TransitionStart::NextFrame { .. }
        ));

        let mut controller = HeroController::new();
        let mut to = to_route().with_valid_size(false);
        assert!(matches!(
            controller.maybe_start_hero_transition(Some(&from), Some(&mut to), true),
            TransitionStart::NextFrame { .. }
        ));

        let mut controller = HeroController::new();
        let mut to = to_route();
        assert!(matches!(
            controller.maybe_start_hero_transition(Some(&from), Some(&mut to), false),
            TransitionStart::NextFrame { .. }
        ));
    }

    #[test]
    fn the_to_route_goes_offstage_only_when_it_has_not_started_arriving() {
        // Putting a route offstage drives its animation to 1.0, which is how
        // the heroes get measured where they are going to end up. A route part
        // way there is already showing, and hiding it would flicker.
        let mut controller = HeroController::new();
        // A settled old route, so this reads as a push rather than a pop.
        let from = HeroRoute::new(10).with_animation(Completed, 1.0);

        let mut to = to_route().with_animation(Forward, 0.0);
        assert_eq!(
            controller.maybe_start_hero_transition(Some(&from), Some(&mut to), false),
            TransitionStart::NextFrame {
                flight: Some(HeroFlightDirection::Push),
                to_offstage: true,
            }
        );
        assert!(to.offstage);

        let mut to = to_route().with_animation(Forward, 0.4);
        assert_eq!(
            controller.maybe_start_hero_transition(Some(&from), Some(&mut to), false),
            TransitionStart::NextFrame {
                flight: Some(HeroFlightDirection::Push),
                to_offstage: false,
            }
        );
        assert!(!to.offstage);
    }

    #[test]
    fn no_flight_type_still_runs_the_end_of_frame_pass() {
        // Which is not a formality: it is how the flights already in the air
        // get ended.
        let mut controller = HeroController::new();
        let from = HeroRoute::new(10)
            .with_animation(Completed, 1.0)
            .with_hero(HeroEntry::new("photo", NAV));
        let mut to = HeroRoute::new(20)
            .with_animation(Completed, 1.0)
            .with_hero(HeroEntry::new("photo", NAV));

        let start = controller.maybe_start_hero_transition(Some(&from), Some(&mut to), false);
        assert_eq!(
            start,
            TransitionStart::NextFrame {
                flight: None,
                to_offstage: false,
            }
        );

        controller.flights.push("photo".to_string());
        assert!(controller.run_pending_transition(&from, &mut to, None));
        assert!(
            controller.flights_in_the_air().is_empty(),
            "the pass ended the flight rather than starting one"
        );
    }

    #[test]
    fn a_route_disposed_during_the_waiting_frame_starts_nothing() {
        let mut controller = HeroController::new();
        let mut from = from_route().with_hero(HeroEntry::new("photo", NAV));
        let mut to = to_route().with_hero(HeroEntry::new("photo", NAV));
        controller.maybe_start_hero_transition(Some(&from), Some(&mut to), false);

        from.attached = false;
        assert!(!controller.run_pending_transition(
            &from,
            &mut to,
            Some(HeroFlightDirection::Push)
        ));
        assert!(controller.flights_in_the_air().is_empty());
    }

    #[test]
    fn only_a_tag_present_on_both_routes_flies() {
        let mut controller = HeroController::new();
        let from = from_route()
            .with_hero(HeroEntry::new("photo", NAV))
            .with_hero(HeroEntry::new("only-here", NAV));
        let mut to = to_route().with_hero(HeroEntry::new("photo", NAV));
        controller.maybe_start_hero_transition(Some(&from), Some(&mut to), false);
        controller.run_pending_transition(&from, &mut to, Some(HeroFlightDirection::Push));
        assert_eq!(controller.flights_in_the_air(), ["photo"]);
    }

    // -- Which heroes are invited --------------------------------------------

    #[test]
    fn a_hero_barred_from_a_gesture_transition_is_still_put_back() {
        // Excluding a hero from a flight is not the same as leaving it alone: a
        // previous flight may have hidden it, and nothing else would restore it.
        let route = HeroRoute::new(10)
            .with_hero(HeroEntry::new("stays", NAV).hidden())
            .with_hero(HeroEntry::new("flies", NAV).with_transition_on_user_gestures(true));

        let collected = HeroController::collect_heroes(&route, NAV, true);
        assert_eq!(collected.flying.len(), 1);
        assert!(collected.flying.contains_key("flies"));
        assert_eq!(collected.ended, [0], "the barred hero was un-hidden");
    }

    #[test]
    fn a_hero_flies_on_a_push_without_asking_for_permission() {
        // transitionOnUserGestures is only consulted for gestures.
        let route = HeroRoute::new(10).with_hero(HeroEntry::new("photo", NAV));
        let collected = HeroController::collect_heroes(&route, NAV, false);
        assert!(collected.flying.contains_key("photo"));
        assert!(collected.ended.is_empty());
    }

    #[test]
    fn a_nested_navigators_heroes_are_left_to_it() {
        let route = HeroRoute::new(10)
            .with_hero(HeroEntry::new("mine", NAV))
            .with_hero(HeroEntry::new("theirs", 99));
        let collected = HeroController::collect_heroes(&route, NAV, false);
        assert_eq!(collected.flying.len(), 1);
        assert!(collected.flying.contains_key("mine"));
        assert!(
            collected.ended.is_empty(),
            "not this controller's to un-hide either"
        );
    }

    // -- The gesture guard ----------------------------------------------------

    #[test]
    fn a_commit_after_a_snapped_back_swipe_does_not_fly_a_second_time() {
        // The swipe already flew the heroes; flying them again would restart
        // the flight from wherever the reader let go.
        let mut controller = HeroController::new();
        assert!(controller.accepts_new_flight());
        controller.user_gesture_in_progress = true;
        assert!(!controller.accepts_new_flight());
    }

    #[test]
    fn a_hero_defaults_to_sitting_out_gestures() {
        // A back swipe the reader may abandon halfway is not a good moment to
        // tear a widget out of the page.
        let hero = Hero::new("photo", 3);
        assert!(!hero.transition_on_user_gestures);
        assert_eq!(hero.curve, "fastOutSlowIn");
        assert_eq!(hero.build(), 3, "from the outside a hero is its child");
        assert!(
            hero.with_transition_on_user_gestures(true)
                .transition_on_user_gestures
        );
    }
}
