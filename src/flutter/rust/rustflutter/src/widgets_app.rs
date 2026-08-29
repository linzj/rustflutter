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

use crate::engine::Color;
use crate::presence::Title;

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

/// What upstream wraps the application in to name it, at the end of
/// `_WidgetsAppState.build`.
///
/// The second variant is not "an empty name" -- it is **no [`Title`] widget at
/// all**, which is a different thing, and the difference is the whole reason
/// the variant exists. See [`WidgetsApp::app_title`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppTitle {
    /// `Title(title: ..., color: ...)`, wrapped around the app.
    Names(Title),
    /// Nothing wrapped around the app, so nothing is ever sent to the host.
    Unnamed,
}

/// Upstream `WidgetsApp`.
#[derive(Clone, Debug, PartialEq)]
pub struct WidgetsApp {
    pub routes: RouteConfiguration,
    /// Upstream asserts this is non-empty: an application that supports no
    /// locales has nothing to resolve the system's locale against.
    pub supported_locales: Vec<String>,
    /// Upstream's `title`, which is nullable -- and whose null is *not* the
    /// same as `""`. See [`WidgetsApp::app_title`].
    pub title: Option<String>,
    /// Upstream's `color`: required, and **not** required to be opaque, which
    /// is why the title path forces it. Documented as "the primary color to
    /// use for the application in the operating system interface".
    pub color: Color,
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
            title: None,
            color: Color::BLACK,
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

    /// Upstream's three-branch title choice, at the end of
    /// `_WidgetsAppState.build`:
    ///
    /// ```dart
    /// final Widget? title;
    /// if (widget.onGenerateTitle != null) {
    ///   title = Builder(
    ///     // This Builder exists to provide a context below the Localizations widget.
    ///     builder: (BuildContext context) {
    ///       final String title = widget.onGenerateTitle!(context);
    ///       return Title(title: title, color: widget.color.withOpacity(1.0), child: result);
    ///     },
    ///   );
    /// } else if (widget.title == null && kIsWeb) {
    ///   title = null;
    /// } else {
    ///   title = Title(title: widget.title ?? '', color: widget.color.withOpacity(1.0), child: result);
    /// }
    /// ```
    ///
    /// `generated` is what `onGenerateTitle` returned, or `None` when there is
    /// no callback -- the two are the same question here, because a callback
    /// "must not return null".
    ///
    /// Three decisions, none of them obvious:
    ///
    /// **The callback wins outright.** An application that sets both `title`
    /// and `onGenerateTitle` never has its `title` read. There is no fallback
    /// between them and no assert against giving both, so the static one sits
    /// there looking configured and doing nothing.
    ///
    /// **The `Builder` in the first branch is load-bearing.** It is not a
    /// wrapper someone left behind: it exists to give the callback a context
    /// *below* the `Localizations` widget this same build installs. Called
    /// with the state's own context instead, the callback would look up a
    /// `Localizations` that is above it -- the ambient one, or none -- and a
    /// localized title is the entire point of the callback.
    ///
    /// **A null title off the web still names the application** -- it names it
    /// the empty string, because `widget.title ?? ''` builds a real [`Title`]
    /// and [`crate::presence::TitleState`] sends whatever it holds. Android's
    /// embedder passes
    /// that straight into `setTaskDescription(TaskDescription(label, ...))`,
    /// so the recents card ends up labelled with nothing rather than falling
    /// back to the manifest.
    ///
    /// On the web the same default would be worse, and that is what the second
    /// branch is for. The web engine's handler is an unconditional assignment:
    ///
    /// ```dart
    /// final String label = arguments['label'] as String? ?? '';
    /// domDocument.title = label;
    /// ```
    ///
    /// -- so a Flutter view embedded in somebody else's page would blank that
    /// page's `<title>` merely by starting up. The title belongs to the host
    /// there, so an application that did not ask for one gets no [`Title`]
    /// widget and the message is never sent. Note the condition is on
    /// `title == null`, not on emptiness: `title: ''` on the web *does* blank
    /// the tab, because that is then something the application asked for.
    pub fn app_title(&self, generated: Option<&str>, is_web: bool) -> AppTitle {
        // Forced, not checked -- see `Title::opaqued`.
        if let Some(generated) = generated {
            return AppTitle::Names(Title::opaqued(generated, self.color));
        }
        if self.title.is_none() && is_web {
            return AppTitle::Unnamed;
        }
        AppTitle::Names(Title::opaqued(
            self.title.as_deref().unwrap_or(""),
            self.color,
        ))
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

    // -- Naming the application ------------------------------------------------

    fn named(app: &WidgetsApp, generated: Option<&str>, is_web: bool) -> Option<String> {
        match app.app_title(generated, is_web) {
            AppTitle::Names(title) => Some(title.title),
            AppTitle::Unnamed => None,
        }
    }

    #[test]
    fn a_generated_title_wins_outright_and_the_static_one_is_never_read() {
        // There is no fallback between the two and no assert against giving
        // both, so a `title` set alongside an `onGenerateTitle` sits there
        // looking configured and doing nothing.
        let mut app = app();
        app.title = Some("Set In The Constructor".to_string());
        assert_eq!(
            named(&app, Some("Generated"), false).as_deref(),
            Some("Generated")
        );
        assert_eq!(
            named(&app, None, false).as_deref(),
            Some("Set In The Constructor")
        );
    }

    #[test]
    fn the_web_gets_no_title_widget_at_all_rather_than_an_empty_one() {
        // The web engine's handler is `domDocument.title = label`, an
        // unconditional assignment -- an embedded view would blank its host
        // page's tab merely by starting up. So the message is never sent.
        let app = app();
        assert_eq!(named(&app, None, true), None);
    }

    #[test]
    fn off_the_web_the_same_application_names_itself_the_empty_string() {
        // Not "says nothing" -- `widget.title ?? ''` builds a real Title, and
        // Android's embedder passes the empty label into setTaskDescription,
        // so the recents card is labelled with nothing rather than falling
        // back to the manifest. The two branches disagree on purpose.
        let app = app();
        assert_eq!(named(&app, None, false).as_deref(), Some(""));
    }

    #[test]
    fn an_explicitly_empty_title_does_blank_the_web_tab() {
        // The condition is on `title == null`, not on emptiness. An
        // application that asked for an empty title on the web gets one.
        let mut app = app();
        app.title = Some(String::new());
        assert_eq!(named(&app, None, true).as_deref(), Some(""));
    }

    #[test]
    fn the_callback_branch_is_reached_before_the_web_branch() {
        // A web application with no `title` but with an `onGenerateTitle` is
        // named: the first branch never consults `kIsWeb`.
        let app = app();
        assert!(app.title.is_none());
        assert_eq!(
            named(&app, Some("Generated"), true).as_deref(),
            Some("Generated")
        );
    }

    #[test]
    fn a_translucent_application_colour_reaches_the_title_opaque() {
        // `WidgetsApp.color` is required but not required to be opaque, and
        // `Title` asserts opacity. `color.withOpacity(1.0)` is what stops an
        // application from crashing its own root widget.
        let mut app = app();
        app.color = Color(0x2200_7ACC);
        for (generated, is_web) in [
            (Some("Generated"), false),
            (Some("Generated"), true),
            (None, false),
        ] {
            let AppTitle::Names(title) = app.app_title(generated, is_web) else {
                panic!("expected a Title");
            };
            assert_eq!(title.color, Color(0xFF00_7ACC));
        }
    }
}
