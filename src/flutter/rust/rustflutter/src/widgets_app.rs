//! Ports of `widgets/app.dart`, `widgets/banner.dart`,
//! `widgets/performance_overlay.dart` and `widgets/pop_scope.dart`:
//! `WidgetsApp`, `CheckedModeBanner`, `PerformanceOverlay` and `PopScope`.
//!
//! The frame around everything else. `WidgetsApp` is the root that installs the
//! inherited widgets an application needs and turns a route name into a page;
//! the banner and the overlay are the two things it draws **over** the whole
//! app; and `PopScope` is how a page answers the back gesture that arrives
//! through it.
//!
//! Note that `app.rs` in this crate is a different subject entirely -- the shell
//! contract, the FFI the engine calls into. This is the widget.

/// Where a route came from. Upstream's error message spells the order out, and
/// the order is the interesting part: an explicit table is consulted **before**
/// the generator callback, so a named route somebody wrote down wins over one
/// somebody would have computed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteSource {
    /// The `home` widget, for the default route only.
    Home,
    /// A matching entry in the `routes` table.
    RoutesTable,
    /// The `onGenerateRoute` callback.
    Generator,
    /// The `onUnknownRoute` callback, which is the last resort.
    Unknown,
}

/// What a [`WidgetsApp`] was configured with, as far as routing goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouteConfiguration {
    pub has_home: bool,
    /// Whether the `routes` table has an entry for `"/"`.
    pub routes_has_default: bool,
    pub routes_is_empty: bool,
    pub has_on_generate_route: bool,
    pub has_on_unknown_route: bool,
    pub has_on_generate_initial_routes: bool,
    pub has_builder: bool,
    pub has_page_route_builder: bool,
}

/// Upstream `WidgetsApp`.
#[derive(Clone, Debug, PartialEq)]
pub struct WidgetsApp {
    pub routes: RouteConfiguration,
    /// Upstream asserts this is non-empty: an application that supports no
    /// locales has nothing to resolve the system's locale against.
    pub supported_locales: Vec<String>,
    pub show_performance_overlay: bool,
    pub debug_show_checked_mode_banner: bool,
}

impl WidgetsApp {
    /// The default route's name.
    pub const DEFAULT_ROUTE_NAME: &'static str = "/";

    pub fn new() -> WidgetsApp {
        WidgetsApp {
            routes: RouteConfiguration {
                has_home: true,
                has_page_route_builder: true,
                routes_is_empty: true,
                ..RouteConfiguration::default()
            },
            supported_locales: vec!["en".to_string()],
            show_performance_overlay: false,
            debug_show_checked_mode_banner: true,
        }
    }

    /// Upstream's constructor asserts, which between them say one thing: **an
    /// application must have some way to produce a route**, and several of the
    /// ways are redundant with each other rather than wrong.
    pub fn validate(&self) -> Result<(), &'static str> {
        let routes = self.routes;
        if routes.has_on_generate_initial_routes && routes.has_home {
            return Err(
                "If onGenerateInitialRoutes is specified, the home argument will be redundant.",
            );
        }
        if routes.has_home && routes.routes_has_default {
            // Both answer the same question, and nothing says which wins.
            return Err(
                "If the home property is specified, the routes table cannot include an entry for \"/\".",
            );
        }
        let can_route = routes.has_home
            || routes.routes_has_default
            || routes.has_on_generate_route
            || routes.has_on_unknown_route;
        if !can_route && !routes.has_builder {
            return Err(
                "Either home, a \"/\" route, onGenerateRoute, onUnknownRoute, or builder must be provided.",
            );
        }
        if !routes.has_builder && !routes.has_on_generate_route && !routes.has_page_route_builder {
            // Somebody has to say what kind of transition the default handler
            // should build, and only pageRouteBuilder says it.
            return Err(
                "If neither builder nor onGenerateRoute are provided, pageRouteBuilder must be.",
            );
        }
        if self.supported_locales.is_empty() {
            return Err("supportedLocales must not be empty");
        }
        Ok(())
    }

    /// Upstream `_onGenerateRoute`, followed by `_onUnknownRoute`.
    ///
    /// `route_in_table` is whether the `routes` map has an entry for this name.
    pub fn route_source(&self, name: &str, route_in_table: bool) -> RouteSource {
        if name == WidgetsApp::DEFAULT_ROUTE_NAME && self.routes.has_home {
            return RouteSource::Home;
        }
        if route_in_table {
            return RouteSource::RoutesTable;
        }
        if self.routes.has_on_generate_route {
            return RouteSource::Generator;
        }
        RouteSource::Unknown
    }

    /// Upstream's assert inside the default handler: using `home` or `routes`
    /// requires a `pageRouteBuilder`, because the default handler has to build
    /// a page route and nothing else tells it which kind.
    pub fn default_handler_is_usable(&self) -> bool {
        (!self.routes.has_home && self.routes.routes_is_empty) || self.routes.has_page_route_builder
    }
}

impl Default for WidgetsApp {
    fn default() -> Self {
        WidgetsApp::new()
    }
}

/// Where a [`CheckedModeBanner`] puts its banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerLocation {
    TopStart,
    TopEnd,
    BottomStart,
    BottomEnd,
}

/// Upstream `CheckedModeBanner`.
///
/// The entire class is a `build` whose body sits inside `assert(() { ... return
/// true; }())`, which is Dart's way of writing code the release compiler
/// removes. In a release build this widget **is** its child; there is nothing
/// left of it to cost anything.
///
/// The banner's text direction is hard-coded to left-to-right even though its
/// location is `topEnd`, so the DEBUG label is always in the top right whatever
/// the application's locale. That is on purpose: the banner is for the person
/// building the app, and moving it about by locale would only make it harder to
/// find.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CheckedModeBanner;

impl CheckedModeBanner {
    pub const MESSAGE: &'static str = "DEBUG";
    pub const LOCATION: BannerLocation = BannerLocation::TopEnd;

    pub fn new() -> CheckedModeBanner {
        CheckedModeBanner
    }

    /// Whether the banner is drawn at all. `debug` stands for upstream's
    /// `assert(...)` running.
    pub fn shows_banner(debug: bool) -> bool {
        debug
    }

    /// Upstream's `debugFillProperties`, which reports "disabled" in release.
    pub fn describe(debug: bool) -> &'static str {
        if debug { "\"DEBUG\"" } else { "disabled" }
    }
}

/// Upstream `PerformanceOverlayOption`.
///
/// The variants carry a comment worth keeping: *"these must be in the order
/// needed for their index values to match the constants in
/// performance_overlay_layer.h"*. **The declaration order is an ABI.**
/// Reordering them would not fail to compile; it would quietly make the overlay
/// show the wrong thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerformanceOverlayOption {
    /// The rasterizer's frame time and FPS: how long turning the layer tree
    /// into draw calls took.
    DisplayRasterizerStatistics,
    /// The same, as a graph over time, red when a frame was lost.
    VisualizeRasterizerStatistics,
    /// The UI thread's frame time: how long **building** the layer tree took.
    DisplayEngineStatistics,
    VisualizeEngineStatistics,
}

impl PerformanceOverlayOption {
    pub fn index(self) -> u32 {
        match self {
            PerformanceOverlayOption::DisplayRasterizerStatistics => 0,
            PerformanceOverlayOption::VisualizeRasterizerStatistics => 1,
            PerformanceOverlayOption::DisplayEngineStatistics => 2,
            PerformanceOverlayOption::VisualizeEngineStatistics => 3,
        }
    }

    pub fn bit(self) -> u32 {
        1 << self.index()
    }
}

/// Upstream `PerformanceOverlay`.
///
/// It shows two sets of numbers because there are **two threads and two ways to
/// lose a frame**: the UI thread can take too long building the layer tree, or
/// the rasterizer can take too long drawing it. Knowing which one went over
/// budget is the whole diagnostic, and one number could not say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PerformanceOverlay {
    /// A bit per [`PerformanceOverlayOption`], by its index.
    pub options_mask: u32,
}

impl PerformanceOverlay {
    pub fn new() -> PerformanceOverlay {
        PerformanceOverlay { options_mask: 0 }
    }

    /// Upstream `PerformanceOverlay.allEnabled`.
    pub fn all_enabled() -> PerformanceOverlay {
        PerformanceOverlay {
            options_mask: PerformanceOverlayOption::DisplayRasterizerStatistics.bit()
                | PerformanceOverlayOption::VisualizeRasterizerStatistics.bit()
                | PerformanceOverlayOption::DisplayEngineStatistics.bit()
                | PerformanceOverlayOption::VisualizeEngineStatistics.bit(),
        }
    }

    pub fn with_options(options: &[PerformanceOverlayOption]) -> PerformanceOverlay {
        PerformanceOverlay {
            options_mask: options.iter().fold(0, |mask, option| mask | option.bit()),
        }
    }

    pub fn shows(&self, option: PerformanceOverlayOption) -> bool {
        self.options_mask & option.bit() != 0
    }
}

/// What happened to a pop the reader asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PopInvocation {
    /// Whether the navigation actually happened. `false` means the pop was
    /// cancelled, which is what a `can_pop` of false produces.
    pub did_pop: bool,
}

/// Upstream `PopScope`.
///
/// The class most often misread, and the misreading is that
/// `onPopInvokedWithResult` can stop a pop. It cannot: upstream's own
/// documentation says *"the pop has already happened"* by the time it runs.
///
/// **The veto is a field, not a callback.** `can_pop` has to be set in advance,
/// because the decision is needed at the moment the gesture arrives and a
/// callback that could answer late would have to hold the whole navigation
/// still while it thought about it.
///
/// And the callback runs **even when the pop was cancelled** -- `did_pop` says
/// which happened. That is what makes it useful at all: a page that refuses the
/// back gesture is exactly the page that wants to put a "discard your changes?"
/// dialog up, and it needs to hear about the attempt it just blocked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PopScope {
    /// Defaults to true: a scope that says nothing does not interfere.
    pub can_pop: bool,
    pub has_on_pop_invoked: bool,
}

impl PopScope {
    pub fn new() -> PopScope {
        PopScope {
            can_pop: true,
            has_on_pop_invoked: false,
        }
    }

    pub fn blocking() -> PopScope {
        PopScope {
            can_pop: false,
            has_on_pop_invoked: true,
        }
    }

    /// What a back gesture through this scope does.
    pub fn handle_pop_attempt(&self) -> PopInvocation {
        PopInvocation {
            did_pop: self.can_pop,
        }
    }

    /// Whether the callback runs. It does either way -- which is the point.
    pub fn notifies(&self) -> bool {
        self.has_on_pop_invoked
    }
}

impl Default for PopScope {
    fn default() -> Self {
        PopScope::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> WidgetsApp {
        WidgetsApp::new()
    }

    // -- An application must be able to produce a route ------------------------

    #[test]
    fn every_way_of_producing_a_route_is_enough_on_its_own() {
        for routes in [
            RouteConfiguration {
                has_home: true,
                has_page_route_builder: true,
                routes_is_empty: true,
                ..RouteConfiguration::default()
            },
            RouteConfiguration {
                routes_has_default: true,
                has_page_route_builder: true,
                ..RouteConfiguration::default()
            },
            RouteConfiguration {
                has_on_generate_route: true,
                routes_is_empty: true,
                ..RouteConfiguration::default()
            },
            RouteConfiguration {
                has_on_unknown_route: true,
                has_page_route_builder: true,
                routes_is_empty: true,
                ..RouteConfiguration::default()
            },
        ] {
            let app = WidgetsApp { routes, ..app() };
            assert_eq!(app.validate(), Ok(()), "{routes:?}");
        }
    }

    #[test]
    fn an_application_with_no_way_to_produce_a_route_is_refused() {
        let app = WidgetsApp {
            routes: RouteConfiguration {
                routes_is_empty: true,
                has_page_route_builder: true,
                ..RouteConfiguration::default()
            },
            ..app()
        };
        assert!(app.validate().is_err());
    }

    #[test]
    fn a_builder_alone_is_enough_because_it_replaces_the_navigator() {
        let app = WidgetsApp {
            routes: RouteConfiguration {
                has_builder: true,
                routes_is_empty: true,
                ..RouteConfiguration::default()
            },
            ..app()
        };
        assert_eq!(app.validate(), Ok(()));
    }

    #[test]
    fn two_ways_of_answering_the_same_question_are_refused_as_redundant() {
        // Not wrong -- redundant, and nothing would say which wins.
        let both = WidgetsApp {
            routes: RouteConfiguration {
                has_home: true,
                routes_has_default: true,
                has_page_route_builder: true,
                ..RouteConfiguration::default()
            },
            ..app()
        };
        assert!(both.validate().is_err());

        let initial_routes = WidgetsApp {
            routes: RouteConfiguration {
                has_home: true,
                has_on_generate_initial_routes: true,
                has_page_route_builder: true,
                routes_is_empty: true,
                ..RouteConfiguration::default()
            },
            ..app()
        };
        assert!(initial_routes.validate().is_err());
    }

    #[test]
    fn somebody_has_to_say_what_kind_of_transition_the_default_handler_builds() {
        let app = WidgetsApp {
            routes: RouteConfiguration {
                has_home: true,
                has_page_route_builder: false,
                routes_is_empty: true,
                ..RouteConfiguration::default()
            },
            ..app()
        };
        assert!(app.validate().is_err());
        assert!(!app.default_handler_is_usable());
    }

    #[test]
    fn a_generator_needs_no_page_route_builder_because_it_builds_the_route() {
        let app = WidgetsApp {
            routes: RouteConfiguration {
                has_on_generate_route: true,
                has_page_route_builder: false,
                routes_is_empty: true,
                ..RouteConfiguration::default()
            },
            ..app()
        };
        assert_eq!(app.validate(), Ok(()));
        assert!(app.default_handler_is_usable());
    }

    #[test]
    fn an_application_that_supports_no_locales_has_nothing_to_resolve_against() {
        let app = WidgetsApp {
            supported_locales: Vec::new(),
            ..app()
        };
        assert!(app.validate().is_err());
    }

    // -- Where a route comes from -----------------------------------------------

    #[test]
    fn a_route_somebody_wrote_down_wins_over_one_somebody_would_compute() {
        // The table is consulted before the generator.
        let app = WidgetsApp {
            routes: RouteConfiguration {
                has_home: true,
                has_on_generate_route: true,
                has_page_route_builder: true,
                ..RouteConfiguration::default()
            },
            ..app()
        };
        assert_eq!(app.route_source("/", false), RouteSource::Home);
        assert_eq!(app.route_source("/details", true), RouteSource::RoutesTable);
        assert_eq!(app.route_source("/details", false), RouteSource::Generator);
    }

    #[test]
    fn home_answers_only_the_default_route() {
        let app = app();
        assert_eq!(app.route_source("/", false), RouteSource::Home);
        assert_eq!(app.route_source("/settings", false), RouteSource::Unknown);
    }

    #[test]
    fn with_nothing_matching_it_falls_through_to_the_unknown_handler() {
        let app = WidgetsApp {
            routes: RouteConfiguration {
                has_on_unknown_route: true,
                has_page_route_builder: true,
                routes_is_empty: true,
                ..RouteConfiguration::default()
            },
            ..app()
        };
        assert_eq!(app.route_source("/nowhere", false), RouteSource::Unknown);
    }

    // -- The debug banner ---------------------------------------------------------

    #[test]
    fn in_a_release_build_the_banner_widget_is_its_child() {
        // The whole body sits inside an assert, which the release compiler
        // removes. There is nothing left of it to cost anything.
        assert!(CheckedModeBanner::shows_banner(true));
        assert!(!CheckedModeBanner::shows_banner(false));
        assert_eq!(CheckedModeBanner::describe(true), "\"DEBUG\"");
        assert_eq!(CheckedModeBanner::describe(false), "disabled");
    }

    #[test]
    fn the_debug_banner_is_always_in_the_same_corner() {
        // Its location is topEnd but its direction is hard-coded left to right,
        // so it does not move about by locale. It is for the person building
        // the app.
        assert_eq!(CheckedModeBanner::LOCATION, BannerLocation::TopEnd);
        assert_eq!(CheckedModeBanner::MESSAGE, "DEBUG");
    }

    // -- The performance overlay -----------------------------------------------------

    #[test]
    fn the_declaration_order_of_the_options_is_an_abi() {
        // Upstream's comment says these must match constants in the engine's
        // header. Reordering them would not fail to compile; it would quietly
        // show the wrong thing.
        assert_eq!(
            PerformanceOverlayOption::DisplayRasterizerStatistics.index(),
            0
        );
        assert_eq!(
            PerformanceOverlayOption::VisualizeRasterizerStatistics.index(),
            1
        );
        assert_eq!(PerformanceOverlayOption::DisplayEngineStatistics.index(), 2);
        assert_eq!(
            PerformanceOverlayOption::VisualizeEngineStatistics.index(),
            3
        );
    }

    #[test]
    fn the_overlay_shows_both_threads_because_either_can_lose_a_frame() {
        // Knowing which one went over budget is the whole diagnostic.
        let all = PerformanceOverlay::all_enabled();
        assert_eq!(all.options_mask, 0b1111);
        for option in [
            PerformanceOverlayOption::DisplayRasterizerStatistics,
            PerformanceOverlayOption::VisualizeRasterizerStatistics,
            PerformanceOverlayOption::DisplayEngineStatistics,
            PerformanceOverlayOption::VisualizeEngineStatistics,
        ] {
            assert!(all.shows(option), "{option:?}");
        }
    }

    #[test]
    fn an_empty_mask_shows_nothing() {
        let none = PerformanceOverlay::new();
        assert_eq!(none.options_mask, 0);
        assert!(!none.shows(PerformanceOverlayOption::DisplayEngineStatistics));
    }

    #[test]
    fn only_the_rasterizer_can_be_asked_for() {
        let overlay = PerformanceOverlay::with_options(&[
            PerformanceOverlayOption::DisplayRasterizerStatistics,
            PerformanceOverlayOption::VisualizeRasterizerStatistics,
        ]);
        assert_eq!(overlay.options_mask, 0b0011);
        assert!(!overlay.shows(PerformanceOverlayOption::DisplayEngineStatistics));
    }

    // -- PopScope --------------------------------------------------------------------

    #[test]
    fn the_veto_is_a_field_and_not_a_callback() {
        // By the time the callback runs the pop has already happened, which
        // upstream's own documentation says outright. The decision is needed
        // when the gesture arrives.
        let open = PopScope::new();
        assert!(open.can_pop);
        assert_eq!(open.handle_pop_attempt(), PopInvocation { did_pop: true });

        let guarded = PopScope::blocking();
        assert_eq!(
            guarded.handle_pop_attempt(),
            PopInvocation { did_pop: false }
        );
    }

    #[test]
    fn the_callback_runs_even_when_the_pop_was_refused() {
        // Which is what makes it useful: a page that refuses the back gesture
        // is exactly the page that wants to ask "discard your changes?", and it
        // has to hear about the attempt it just blocked.
        let guarded = PopScope::blocking();
        assert!(guarded.notifies());
        assert!(!guarded.handle_pop_attempt().did_pop);
    }

    #[test]
    fn a_scope_that_says_nothing_does_not_interfere() {
        let default = PopScope::default();
        assert!(default.can_pop);
        assert!(!default.notifies());
    }
}
