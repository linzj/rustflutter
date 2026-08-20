//! A port of `material/page.dart`: `MaterialRouteTransitionMixin`,
//! `MaterialPageRoute` and `MaterialPage`.
//!
//! What a Material page route does at the seams -- which transition it asks the
//! theme for, and whether it animates out at all when something else arrives on
//! top of it.

use crate::page_transitions_theme::{PageTransitionsTheme, ResolvedTransition};
use crate::scroll_plumbing::ScrollPlatform;

/// Upstream `MaterialRouteTransitionMixin`.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialRouteTransitionMixin {
    pub fullscreen_dialog: bool,
}

impl MaterialRouteTransitionMixin {
    /// Upstream's `?? const Duration(microseconds: 300)`.
    ///
    /// Note the unit. Every other duration in this area is milliseconds, and
    /// three hundred **microseconds** is three tenths of one millisecond --
    /// instant. It is a typo, and it is a typo nobody can reach: the fallback
    /// switch below is exhaustive over the platforms, so the builder lookup
    /// never returns null and the `??` never fires.
    ///
    /// Ported as it stands, because a dead branch with the wrong number in it
    /// is exactly what stops being dead when somebody adds a platform.
    pub const UNREACHABLE_FALLBACK_MICROSECONDS: u32 = 300;

    pub fn new() -> MaterialRouteTransitionMixin {
        MaterialRouteTransitionMixin {
            fullscreen_dialog: false,
        }
    }

    pub fn fullscreen_dialog() -> MaterialRouteTransitionMixin {
        MaterialRouteTransitionMixin {
            fullscreen_dialog: true,
        }
    }

    /// Upstream `_getPageTransitionBuilder`: the theme's entry, or a two-way
    /// platform split.
    ///
    /// The fallback is **not** the same as `PageTransitionsTheme`'s own default
    /// table, which gives Android the predictive-back transition. This one is
    /// the simpler split, and it only runs when the theme was given a table
    /// that has no entry for this platform.
    pub fn transition_builder(
        theme: &PageTransitionsTheme,
        platform: ScrollPlatform,
    ) -> ResolvedTransition {
        theme
            .builders()
            .iter()
            .find(|(candidate, _)| *candidate == platform)
            .map(|(_, transition)| *transition)
            .unwrap_or(match platform {
                ScrollPlatform::IOS | ScrollPlatform::MacOS => ResolvedTransition::Cupertino,
                _ => ResolvedTransition::Zoom,
            })
    }

    /// Upstream overrides `didPush` and `didPop` only to push the durations
    /// into the controller by hand, with the reason in a comment: the
    /// `AnimationController` is built once from `transitionDuration`, so a
    /// theme that changes afterwards cannot reach it.
    ///
    /// A workaround carrying its own removal condition -- upstream's TODO says
    /// to delete this "when controller can be updated as the transitionDuration
    /// is changed". Worth keeping the shape, because the shape is the reminder.
    pub fn pushes_duration_into_controller_by_hand() -> bool {
        true
    }

    /// Upstream `barrierColor` and `barrierLabel` are both null: a page route
    /// covers the screen, so a scrim behind it would be invisible and would
    /// cost a layer for nothing.
    pub fn has_barrier() -> bool {
        false
    }

    /// Upstream `canTransitionTo`.
    ///
    /// The question reads as "may I animate out?" and is really **"does the
    /// thing arriving know what to do with me?"** A route only animates out
    /// when the arriving one either shares this mixin -- so the two are already
    /// in step -- or carries a delegated transition, which means it will drive
    /// this route's exit itself. Otherwise this route sits still, which beats
    /// two independent animations fighting over the same screen.
    ///
    /// And nothing animates out under a fullscreen dialog: it covers
    /// everything, so the motion underneath is motion nobody sees.
    pub fn can_transition_to(
        &self,
        next_is_page_route: bool,
        next_is_fullscreen_dialog: bool,
        next_shares_the_mixin: bool,
        next_has_delegated_transition: bool,
    ) -> bool {
        let next_is_not_fullscreen = !next_is_page_route || !next_is_fullscreen_dialog;
        next_is_not_fullscreen && (next_shares_the_mixin || next_has_delegated_transition)
    }

    /// Upstream `canTransitionFrom`: the same judgement from the other side. A
    /// fullscreen dialog suppresses the route it covered.
    pub fn can_transition_from(&self, previous_is_page_route: bool) -> bool {
        previous_is_page_route && !self.fullscreen_dialog
    }
}

impl Default for MaterialRouteTransitionMixin {
    fn default() -> Self {
        MaterialRouteTransitionMixin::new()
    }
}

/// Upstream `MaterialPageRoute`: the imperative one, pushed with
/// `Navigator.push`.
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialPageRoute {
    pub transitions: MaterialRouteTransitionMixin,
    /// Whether the route is kept alive while covered. Upstream defaults it to
    /// true, so a page scrolled halfway down is still halfway down when you
    /// come back to it.
    pub maintain_state: bool,
}

impl MaterialPageRoute {
    pub fn new() -> MaterialPageRoute {
        MaterialPageRoute {
            transitions: MaterialRouteTransitionMixin::new(),
            maintain_state: true,
        }
    }

    pub fn fullscreen_dialog() -> MaterialPageRoute {
        MaterialPageRoute {
            transitions: MaterialRouteTransitionMixin::fullscreen_dialog(),
            maintain_state: true,
        }
    }
}

impl Default for MaterialPageRoute {
    fn default() -> Self {
        MaterialPageRoute::new()
    }
}

/// Upstream `MaterialPage`: the declarative one, put in a `Navigator.pages`
/// list.
///
/// The difference from [`MaterialPageRoute`] is not what it looks like but
/// **who owns the stack**. A page route is pushed and popped by imperative
/// calls; a page is a description, and the navigator works out what to push and
/// pop by comparing the list it was given against the one it had.
///
/// Which is why a page needs a `key` and a route does not: the navigator has to
/// be able to say "this is the same page as before, in a different position"
/// rather than "a page went and another arrived".
#[derive(Clone, Debug, PartialEq)]
pub struct MaterialPage {
    pub key: Option<u64>,
    pub transitions: MaterialRouteTransitionMixin,
    pub maintain_state: bool,
    pub child: u64,
}

impl MaterialPage {
    pub fn new(child: u64) -> MaterialPage {
        MaterialPage {
            key: None,
            transitions: MaterialRouteTransitionMixin::new(),
            maintain_state: true,
            child,
        }
    }

    pub fn with_key(mut self, key: u64) -> Self {
        self.key = Some(key);
        self
    }

    /// Whether the navigator can recognise this page across a rebuild.
    pub fn is_identifiable(&self) -> bool {
        self.key.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Which transition ------------------------------------------------------

    #[test]
    fn the_route_asks_the_theme_first_and_falls_back_to_a_two_way_split() {
        let empty = PageTransitionsTheme::with_builders(vec![]);
        assert_eq!(
            MaterialRouteTransitionMixin::transition_builder(&empty, ScrollPlatform::IOS),
            ResolvedTransition::Cupertino
        );
        assert_eq!(
            MaterialRouteTransitionMixin::transition_builder(&empty, ScrollPlatform::MacOS),
            ResolvedTransition::Cupertino
        );
        for platform in [
            ScrollPlatform::Android,
            ScrollPlatform::Fuchsia,
            ScrollPlatform::Windows,
            ScrollPlatform::Linux,
        ] {
            assert_eq!(
                MaterialRouteTransitionMixin::transition_builder(&empty, platform),
                ResolvedTransition::Zoom,
                "{platform:?}"
            );
        }
    }

    #[test]
    fn this_fallback_is_not_the_themes_own_default_table() {
        // The theme gives Android the predictive-back transition; this simpler
        // split gives it zoom, and only runs when the theme has no entry.
        let empty = PageTransitionsTheme::with_builders(vec![]);
        assert_eq!(
            MaterialRouteTransitionMixin::transition_builder(&empty, ScrollPlatform::Android),
            ResolvedTransition::Zoom
        );
        assert_eq!(
            PageTransitionsTheme::new().resolve(ScrollPlatform::Android),
            ResolvedTransition::PredictiveBack
        );
    }

    #[test]
    fn a_theme_entry_wins_over_the_fallback() {
        let themed = PageTransitionsTheme::with_builders(vec![(
            ScrollPlatform::IOS,
            ResolvedTransition::FadeForwards,
        )]);
        assert_eq!(
            MaterialRouteTransitionMixin::transition_builder(&themed, ScrollPlatform::IOS),
            ResolvedTransition::FadeForwards
        );
    }

    #[test]
    fn the_unreachable_fallback_duration_is_in_the_wrong_unit() {
        // Three hundred MICROseconds where everything nearby is milliseconds.
        // A typo nobody can reach, because the platform switch above it is
        // exhaustive -- and exactly the kind of dead branch that stops being
        // dead when a platform is added.
        assert_eq!(
            MaterialRouteTransitionMixin::UNREACHABLE_FALLBACK_MICROSECONDS,
            300
        );
        assert!(
            MaterialRouteTransitionMixin::UNREACHABLE_FALLBACK_MICROSECONDS < 1000,
            "less than a single millisecond"
        );
    }

    // -- Whether to animate out at all -------------------------------------------

    #[test]
    fn a_route_animates_out_only_if_the_arriving_one_knows_what_to_do_with_it() {
        // Otherwise two independent animations would fight over the screen.
        let route = MaterialRouteTransitionMixin::new();
        assert!(
            route.can_transition_to(true, false, true, false),
            "the next route shares the mixin, so they are already in step"
        );
        assert!(
            route.can_transition_to(true, false, false, true),
            "or it will drive this exit itself"
        );
        assert!(
            !route.can_transition_to(true, false, false, false),
            "and with neither, this route sits still"
        );
    }

    #[test]
    fn nothing_animates_out_under_a_fullscreen_dialog() {
        // It covers everything, so the motion underneath is motion nobody sees.
        let route = MaterialRouteTransitionMixin::new();
        assert!(!route.can_transition_to(true, true, true, true));
        assert!(
            route.can_transition_to(false, true, true, false),
            "and something that is not a page route is not a fullscreen dialog"
        );
    }

    #[test]
    fn a_fullscreen_dialog_suppresses_the_route_it_covered() {
        // The same judgement from the other side.
        assert!(MaterialRouteTransitionMixin::new().can_transition_from(true));
        assert!(!MaterialRouteTransitionMixin::fullscreen_dialog().can_transition_from(true));
        assert!(
            !MaterialRouteTransitionMixin::new().can_transition_from(false),
            "and there is nothing to transition from if it was not a page"
        );
    }

    #[test]
    fn a_page_route_has_no_scrim_because_it_covers_the_screen() {
        assert!(!MaterialRouteTransitionMixin::has_barrier());
    }

    #[test]
    fn the_durations_are_pushed_into_the_controller_by_hand() {
        // Because the controller is built once and a theme that changes
        // afterwards cannot reach it. Upstream's TODO carries its own removal
        // condition.
        assert!(MaterialRouteTransitionMixin::pushes_duration_into_controller_by_hand());
    }

    // -- Route against page ---------------------------------------------------------

    #[test]
    fn a_page_needs_a_key_and_a_route_does_not() {
        // The navigator has to be able to say "the same page, in a different
        // position" rather than "one went and another arrived".
        let anonymous = MaterialPage::new(1);
        assert!(!anonymous.is_identifiable());
        assert!(MaterialPage::new(1).with_key(7).is_identifiable());
    }

    #[test]
    fn a_covered_page_keeps_its_state_so_you_come_back_where_you_were() {
        assert!(MaterialPageRoute::new().maintain_state);
        assert!(MaterialPage::new(1).maintain_state);
    }

    #[test]
    fn a_fullscreen_dialog_route_carries_the_flag_through_to_its_transitions() {
        let dialog = MaterialPageRoute::fullscreen_dialog();
        assert!(dialog.transitions.fullscreen_dialog);
        assert!(!MaterialPageRoute::new().transitions.fullscreen_dialog);
    }
}
