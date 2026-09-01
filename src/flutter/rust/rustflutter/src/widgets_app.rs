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
use crate::platform::Locale;
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
    /// Upstream's `navigatorKey`, `initialRoute` and `navigatorObservers`.
    ///
    /// They are here to be **forbidden**, not used: an application that routes
    /// with nothing but a `builder` has no navigator for them to configure, so
    /// upstream asserts they are still at their initial values. See
    /// [`WidgetsApp::validate`].
    pub has_navigator_key: bool,
    pub has_initial_route: bool,
    pub has_navigator_observers: bool,
}

/// How an application configured with `WidgetsApp.router` is wired: upstream's
/// `routeInformationProvider`, `routeInformationParser`, `routerDelegate`,
/// `backButtonDispatcher` and `routerConfig`.
///
/// A separate type from [`RouteConfiguration`] because upstream keeps them in
/// separate **constructors**: an application takes one road or the other, and
/// the asserts on each are about that road only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouterConfiguration {
    pub has_route_information_provider: bool,
    pub has_route_information_parser: bool,
    pub has_router_delegate: bool,
    pub has_back_button_dispatcher: bool,
    /// Upstream's `routerConfig`, which carries all four of the others at
    /// once -- which is why giving it *and* any of them is an error rather
    /// than a merge.
    pub has_router_config: bool,
}

impl RouterConfiguration {
    /// Upstream's `WidgetsApp.router` constructor asserts, all three.
    ///
    /// They are one idea in three parts: **say who routes, once**. A
    /// `routerConfig` is the whole arrangement in one object, so nothing else
    /// may be given alongside it; without one there has to be a
    /// `routerDelegate`, because something must build the pages; and a
    /// provider with no parser is a stream of route information nothing can
    /// read.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.has_router_config {
            if self.has_route_information_provider
                || self.has_route_information_parser
                || self.has_router_delegate
                || self.has_back_button_dispatcher
            {
                return Err(
                    "If the routerConfig is provided, all the other router delegates must not be provided",
                );
            }
            return Ok(());
        }
        if !self.has_router_delegate {
            return Err("Either one of routerDelegate or routerConfig must be provided");
        }
        if self.has_route_information_provider && !self.has_route_information_parser {
            return Err(
                "If routeInformationProvider is provided, routeInformationParser must also be provided",
            );
        }
        Ok(())
    }

    /// Whether this application routes with a `Router` at all -- upstream's
    /// `_usesRouter`, which both `MaterialApp` and `CupertinoApp` ask under
    /// their own names.
    pub fn is_configured(&self) -> bool {
        self.has_router_delegate || self.has_router_config
    }
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
    ///
    /// Real [`Locale`]s rather than the strings this used to hold: the
    /// resolution algorithm matches on language, script and country
    /// separately -- see [`crate::localizations::basic_locale_list_resolution`]
    /// -- and a list of strings cannot be handed to it. Two homes for "which
    /// languages does this application speak" is one too many.
    pub supported_locales: Vec<Locale>,
    /// Upstream's `locale`: the application's own choice, which **overrides
    /// the platform's** and is still resolved against the supported list. See
    /// [`WidgetsApp::localizations`].
    pub locale: Option<Locale>,
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
            supported_locales: vec![Locale::new("en")],
            locale: None,
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
        // The **other half** of that assert, which this port had missing: a
        // `builder` is allowed to be the only way to a route, but then the
        // navigator's own settings have to be untouched. Upstream spells out
        // which and what they must be -- *"namely navigatorKey, initialRoute,
        // and navigatorObservers, must have their initial values (null, null,
        // and the empty list, respectively)"* -- because an application that
        // set one of them is describing a navigator it is not going to get,
        // and nothing later would say so.
        if !can_route
            && (routes.has_navigator_key
                || routes.has_initial_route
                || routes.has_navigator_observers)
        {
            return Err(
                "If no route is provided using home, routes, onGenerateRoute, or onUnknownRoute, \
                 navigatorKey, initialRoute and navigatorObservers must have their initial values.",
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

    /// The resolver upstream's `_WidgetsAppState` builds from these fields:
    ///
    /// ```dart
    /// late final LocalizationsResolver _localizationsResolver = LocalizationsResolver(
    ///   locale: widget.locale,
    ///   localeListResolutionCallback: widget.localeListResolutionCallback,
    ///   localeResolutionCallback: widget.localeResolutionCallback,
    ///   localizationsDelegates: widget.localizationsDelegates,
    ///   supportedLocales: widget.supportedLocales,
    /// );
    /// ```
    ///
    /// It is built **from** the widget rather than kept in it, which is why
    /// this returns one rather than storing one: the widget is rebuilt with
    /// new fields and the resolver survives, holding the resolved locale
    /// across those rebuilds -- upstream's `_updateLocalizations` hands it the
    /// new fields instead of making another.
    pub fn localizations(
        &self,
        platform_locales: &[Locale],
        list_callback: Option<crate::localizations::LocaleListResolution>,
        single_callback: Option<crate::localizations::LocaleResolution>,
    ) -> crate::localizations::LocalizationsResolver {
        let mut resolver = crate::localizations::LocalizationsResolver::new(
            self.supported_locales.clone(),
            platform_locales,
            list_callback,
            single_callback,
        );
        resolver.locale = self.locale.clone();
        resolver
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

    /// The banner over `child`, or `child` untouched when this is not a debug
    /// build.
    ///
    /// Upstream's whole `build` is inside `assert(() { ... }())`, so in
    /// release the widget **is** its child and costs nothing. `debug` stands
    /// in for that assert running, the same way
    /// [`CheckedModeBanner::shows_banner`] already did -- what was missing was
    /// anything for it to return.
    pub fn widget(debug: bool, child: crate::framework::AnyWidget) -> crate::framework::AnyWidget {
        if !CheckedModeBanner::shows_banner(debug) {
            return child;
        }
        crate::framework::single(child, |child| {
            RenderBanner::new(
                // Upstream hard-codes both directions to left-to-right here,
                // not just the text: `Banner(... textDirection: TextDirection.ltr,
                // layoutDirection: TextDirection.ltr ...)`. So the label stays
                // in the top right in an Arabic locale too, because it is for
                // whoever is building the app rather than for whoever is using
                // it.
                BannerPainter::new(CheckedModeBanner::MESSAGE, CheckedModeBanner::LOCATION)
                    .with_directions(
                        crate::direction::TextDirection::Ltr,
                        crate::direction::TextDirection::Ltr,
                    ),
                child,
            )
        })
    }
}

// -- The banner itself (upstream `widgets/banner.dart`) -----------------------

/// Upstream `BannerPainter`, which is where every number in a banner lives.
///
/// The class had no port at all: [`BannerLocation`] and [`CheckedModeBanner`]
/// described *where* a banner goes and *what it says*, and nothing in the
/// crate could draw one. So an app carrying
/// `debug_show_checked_mode_banner: true` -- which all three of
/// [`WidgetsApp`], `MaterialApp` and `CupertinoApp` do by default -- showed
/// nothing, because there was nothing to show.
///
/// The shape is a ribbon across a corner at 45 degrees. Upstream draws it in a
/// **translated and rotated** canvas, so all four corners are one rectangle
/// and one text run with different transforms, and the arithmetic below is
/// that transform rather than four sets of coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct BannerPainter {
    pub message: String,
    /// Which way the *message* reads, for a bidirectional string.
    pub text_direction: crate::direction::TextDirection,
    pub location: BannerLocation,
    /// Which way `location`'s start and end are read. Upstream keeps this
    /// apart from `text_direction` on purpose: a banner can sit in the corner
    /// the layout calls "end" while its text still reads left to right.
    pub layout_direction: crate::direction::TextDirection,
    pub color: crate::engine::Color,
    pub text_style: crate::engine::TextStyle,
    pub shadow: crate::painting::BoxShadow,
}

impl BannerPainter {
    /// Distance from the corner to the bottom of the banner, measured along
    /// the edge -- upstream's `_kOffset`.
    pub const OFFSET: f32 = 40.0;
    /// The ribbon's thickness, upstream's `_kHeight`.
    pub const HEIGHT: f32 = 12.0;
    /// Upstream's `_kColor`: a dark red, and **not** opaque -- the top byte is
    /// `A0`, so whatever is underneath shows through the ribbon.
    pub const COLOR: crate::engine::Color = crate::engine::Color(0xA0B7_1C1C);

    /// Upstream's `_kBottomOffset`: the offset plus the ribbon's thickness
    /// measured across the 45-degree diagonal, which is where the `sqrt(1/2)`
    /// comes from. Only the bottom corners need it, because only they are
    /// positioned from the far edge.
    pub fn bottom_offset() -> f32 {
        BannerPainter::OFFSET + std::f32::consts::FRAC_1_SQRT_2 * BannerPainter::HEIGHT
    }

    /// Upstream's `_kRect`, in the rotated frame: the ribbon is twice the
    /// offset wide and centred on the corner, which is why its left edge is
    /// negative.
    pub fn rect() -> crate::engine::Rect {
        crate::engine::Rect::xywh(
            -BannerPainter::OFFSET,
            BannerPainter::OFFSET - BannerPainter::HEIGHT,
            BannerPainter::OFFSET * 2.0,
            BannerPainter::HEIGHT,
        )
    }

    /// Upstream's `_kTextStyle`: white, heavy, and `height: 1.0` so the line
    /// box is the EM square and the text sits in the middle of a 12-pixel
    /// ribbon rather than wherever the font's own metrics would put it.
    pub fn default_text_style() -> crate::engine::TextStyle {
        crate::engine::TextStyle {
            font_size: BannerPainter::HEIGHT * 0.85,
            font_weight: 900,
            height: Some(1.0),
            color: crate::engine::Color(0xFFFF_FFFF),
            align: crate::engine::TextAlign::Center,
            ..Default::default()
        }
    }

    /// Upstream's `_kShadow`, which is a blur and no offset: the ribbon is
    /// meant to look lifted off the corner rather than lit from a direction.
    pub fn default_shadow() -> crate::painting::BoxShadow {
        crate::painting::BoxShadow::new(crate::engine::Color(0x7F00_0000), 0.0, 0.0, 6.0, 0.0)
    }

    pub fn new(message: impl Into<String>, location: BannerLocation) -> BannerPainter {
        BannerPainter {
            message: message.into(),
            text_direction: crate::direction::TextDirection::Ltr,
            location,
            layout_direction: crate::direction::TextDirection::Ltr,
            color: BannerPainter::COLOR,
            text_style: BannerPainter::default_text_style(),
            shadow: BannerPainter::default_shadow(),
        }
    }

    pub fn with_directions(
        mut self,
        text_direction: crate::direction::TextDirection,
        layout_direction: crate::direction::TextDirection,
    ) -> Self {
        self.text_direction = text_direction;
        self.layout_direction = layout_direction;
        self
    }

    pub fn with_color(mut self, color: crate::engine::Color) -> Self {
        self.color = color;
        self
    }

    /// Upstream's `_translationX`.
    ///
    /// The two top corners sit **on** the corner, and the two bottom ones are
    /// pulled in by [`BannerPainter::bottom_offset`] -- the ribbon hangs off
    /// the top corners and has to be brought back inside at the bottom.
    pub fn translation_x(&self, width: f32) -> f32 {
        use crate::direction::TextDirection::{Ltr, Rtl};
        match (self.layout_direction, self.location) {
            (Rtl, BannerLocation::TopStart) => width,
            (Ltr, BannerLocation::TopStart) => 0.0,
            (Rtl, BannerLocation::TopEnd) => 0.0,
            (Ltr, BannerLocation::TopEnd) => width,
            (Rtl, BannerLocation::BottomStart) => width - BannerPainter::bottom_offset(),
            (Ltr, BannerLocation::BottomStart) => BannerPainter::bottom_offset(),
            (Rtl, BannerLocation::BottomEnd) => BannerPainter::bottom_offset(),
            (Ltr, BannerLocation::BottomEnd) => width - BannerPainter::bottom_offset(),
        }
    }

    /// Upstream's `_translationY`, which does not depend on the direction: up
    /// and down are the same in both.
    pub fn translation_y(&self, height: f32) -> f32 {
        match self.location {
            BannerLocation::BottomStart | BannerLocation::BottomEnd => {
                height - BannerPainter::bottom_offset()
            }
            BannerLocation::TopStart | BannerLocation::TopEnd => 0.0,
        }
    }

    /// Upstream's `_rotation`: a quarter turn, and the sign is what puts the
    /// ribbon across the corner it was sent to rather than off the screen.
    pub fn rotation(&self) -> f32 {
        use crate::direction::TextDirection::{Ltr, Rtl};
        let sign = match (self.layout_direction, self.location) {
            (Rtl, BannerLocation::TopStart | BannerLocation::BottomEnd) => 1.0,
            (Ltr, BannerLocation::TopStart | BannerLocation::BottomEnd) => -1.0,
            (Rtl, BannerLocation::BottomStart | BannerLocation::TopEnd) => -1.0,
            (Ltr, BannerLocation::BottomStart | BannerLocation::TopEnd) => 1.0,
        };
        std::f32::consts::FRAC_PI_4 * sign
    }

    /// The ribbon on its own, in the rotated frame: the two rectangles and the
    /// text, ready to be handed to a transform.
    fn ribbon(&self) -> BannerRibbon {
        BannerRibbon {
            color: self.color,
            shadow: self.shadow,
            text: crate::render::RenderRef::new(
                crate::render::RenderParagraph::new(self.message.clone())
                    .with_style(self.text_style.clone())
                    .with_text_direction(self.text_direction),
            ),
            size: crate::render::Size::ZERO,
        }
    }
}

/// The ribbon in its own coordinates, which is what upstream draws once the
/// canvas has been translated and rotated for it.
///
/// It exists as a render object rather than as three canvas calls because a
/// transform here takes a **child**, where upstream's canvas takes a matrix
/// and keeps drawing. Same three draws, one level further in.
struct BannerRibbon {
    color: crate::engine::Color,
    shadow: crate::painting::BoxShadow,
    text: crate::render::BoxedRender,
    size: crate::render::Size,
}

impl crate::render::RenderBox for BannerRibbon {
    fn layout(&mut self, constraints: crate::render::BoxConstraints) -> crate::render::Size {
        // The text is laid out at the ribbon's full width so that centring it
        // means something -- upstream lays its painter out with `minWidth` and
        // `maxWidth` both `_kOffset * 2`.
        let width = BannerPainter::OFFSET * 2.0;
        self.text.layout_child(
            crate::render::BoxConstraints::new(width, width, 0.0, f32::INFINITY),
            true,
        );
        self.size = constraints.constrain(crate::render::Size::new(width, BannerPainter::HEIGHT));
        self.size
    }

    fn size(&self) -> crate::render::Size {
        self.size
    }

    fn paint(&self, context: &mut crate::render::PaintContext, offset: crate::render::Offset) {
        let rect = BannerPainter::rect();
        let placed = crate::engine::Rect::ltrb(
            rect.left + offset.dx,
            rect.top + offset.dy,
            rect.right + offset.dx,
            rect.bottom + offset.dy,
        );
        let shadow = self.shadow.to_paint();
        let banner = crate::engine::Paint::new(self.color);
        {
            let canvas = context.canvas();
            canvas.draw_rect(placed, &shadow);
            canvas.draw_rect(placed, &banner);
        }
        // Upstream centres the text vertically in the ribbon by hand rather
        // than aligning it, because the painter has a rect and not a box.
        let text_height = crate::render::RenderBox::size(&self.text).height;
        context.paint_child(
            &self.text,
            crate::render::Offset::new(
                placed.left,
                placed.top + (BannerPainter::HEIGHT - text_height) / 2.0,
            ),
        );
    }

    fn visit_children(
        &self,
        visit: &mut dyn FnMut(&dyn crate::render::RenderBox, crate::render::Offset),
    ) {
        visit(&self.text, crate::render::Offset::ZERO);
    }

    /// Upstream's `BannerPainter.hitTest` returns false: a banner is drawn
    /// over the app and takes nothing from it.
    fn hit_test_children(
        &self,
        _position: crate::render::Offset,
        _result: &mut crate::render::HitTestResult,
    ) -> bool {
        false
    }
}

/// Upstream's `Banner`, which is a `CustomPaint` with the painter in
/// **front** of the child.
///
/// There is no `Banner` widget struct here because the name is taken:
/// [`crate::controls::Banner`] is upstream's `MaterialBanner`. What upstream's
/// `Banner` adds over its painter is one thing -- painting the ribbon after
/// the child instead of before it -- and that is this render object.
pub struct RenderBanner {
    child: crate::render::BoxedRender,
    painter: BannerPainter,
    ribbon: crate::render::BoxedRender,
    size: crate::render::Size,
}

impl RenderBanner {
    pub fn new(
        painter: BannerPainter,
        child: impl crate::render::RenderBox + 'static,
    ) -> RenderBanner {
        let ribbon = crate::render::RenderRef::new(painter.ribbon());
        RenderBanner {
            child: crate::render::RenderRef::new(child),
            painter,
            ribbon,
            size: crate::render::Size::ZERO,
        }
    }

    pub fn painter(&self) -> &BannerPainter {
        &self.painter
    }
}

impl crate::render::RenderBox for RenderBanner {
    fn layout(&mut self, constraints: crate::render::BoxConstraints) -> crate::render::Size {
        self.size = self.child.layout_child(constraints, true);
        self.ribbon.layout_child(
            crate::render::BoxConstraints::loose(f32::INFINITY, f32::INFINITY),
            true,
        );
        self.size
    }

    fn size(&self) -> crate::render::Size {
        self.size
    }

    fn paint(&self, context: &mut crate::render::PaintContext, offset: crate::render::Offset) {
        context.paint_child(&self.child, offset);
        let rotation = self.painter.rotation();
        let (sin, cos) = rotation.sin_cos();
        // Upstream: `canvas..translate(tx, ty)..rotate(r)`. The translation
        // rides in the offset and the matrix is the rotation, which is the
        // same composition in the other order this context takes it.
        context.push_transform(
            [cos, sin, -sin, cos, 0.0, 0.0],
            crate::render::Offset::ZERO,
            crate::render::Offset::new(
                offset.dx + self.painter.translation_x(self.size.width),
                offset.dy + self.painter.translation_y(self.size.height),
            ),
            &self.ribbon,
        );
    }

    fn visit_children(
        &self,
        visit: &mut dyn FnMut(&dyn crate::render::RenderBox, crate::render::Offset),
    ) {
        visit(&self.child, crate::render::Offset::ZERO);
    }

    /// The banner is not offered to the semantics walk or the hit test: only
    /// the child is. A "DEBUG" ribbon read out to a screen-reader user, or
    /// swallowing a tap in the corner of every screen, would both be faults.
    fn hit_test_children(
        &self,
        position: crate::render::Offset,
        result: &mut crate::render::HitTestResult,
    ) -> bool {
        self.child.hit_test(position, result)
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
    fn the_app_hands_its_locales_to_the_resolver_that_can_use_them() {
        // The field held strings, which the resolution algorithm cannot match
        // on -- it compares language, script and country separately.
        use crate::platform::Locale;
        let app = WidgetsApp {
            supported_locales: vec![Locale::new("en"), Locale::new("fr")],
            ..app()
        };
        let resolver = app.localizations(&[Locale::new("fr")], None, None);
        assert_eq!(resolver.resolved(), Some(Locale::new("fr")));
    }

    #[test]
    fn an_apps_own_locale_beats_the_platforms_and_is_still_resolved() {
        use crate::platform::Locale;
        let app = WidgetsApp {
            supported_locales: vec![Locale::new("en"), Locale::new("fr")],
            locale: Some(Locale::new("fr")),
            ..app()
        };
        assert_eq!(
            app.localizations(&[Locale::new("en")], None, None)
                .resolved(),
            Some(Locale::new("fr")),
            "the application said French, so French it is"
        );

        // And a locale the application does not support falls back the same
        // way a reader's would.
        let asking_for_german = WidgetsApp {
            locale: Some(Locale::new("de")),
            ..app
        };
        assert_eq!(
            asking_for_german
                .localizations(&[Locale::new("en")], None, None)
                .resolved(),
            Some(Locale::new("en"))
        );
    }

    #[test]
    fn the_apps_callbacks_reach_the_resolver() {
        use crate::platform::Locale;
        fn swedish(_preferred: &[Locale], _supported: &[Locale]) -> Option<Locale> {
            Some(Locale::new("sv"))
        }
        let app = WidgetsApp {
            supported_locales: vec![Locale::new("en"), Locale::new("fr")],
            ..app()
        };
        assert_eq!(
            app.localizations(&[Locale::new("fr")], Some(swedish), None)
                .resolved(),
            Some(Locale::new("sv")),
            "an application that wrote a callback is asked"
        );
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

    // -- The banner ----------------------------------------------------------

    /// What a banner over a 200x100 child left on the canvas, in order.
    fn banner_drawn(debug: bool) -> Vec<crate::engine_test_stubs::Drawn> {
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(CheckedModeBanner::widget(
            debug,
            crate::framework::leaf(|| crate::widgets::Container::new().with_size(200.0, 100.0)),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 100.0),
        );
        let mut layers = crate::engine::LayerTree::new(400, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(400.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, crate::render::Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    #[test]
    fn a_release_build_is_its_child_and_nothing_else() {
        // Upstream's whole build sits inside an `assert(() {...}())`, which
        // the release compiler removes. Nothing of the banner is left to cost
        // anything -- not a transform, not a rectangle.
        let calls = banner_drawn(false);
        assert!(
            !calls
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::TransformLayer { .. })),
            "a release build should not even push the banner's transform: {calls:?}"
        );
        assert!(
            !calls
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::Paragraph { .. })),
            "nor draw its text: {calls:?}"
        );
    }

    #[test]
    fn a_debug_build_puts_the_word_debug_across_the_corner() {
        let calls = banner_drawn(true);
        assert!(
            calls.iter().any(|call| matches!(
                call,
                crate::engine_test_stubs::Drawn::Paragraph { text, .. } if text == "DEBUG"
            )),
            "{calls:?}"
        );
        // Two rectangles, the shadow under the ribbon.
        let rects: Vec<u32> = calls
            .iter()
            .filter_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect { argb, .. } => Some(*argb),
                _ => None,
            })
            .collect();
        assert!(
            rects.contains(&BannerPainter::COLOR.0),
            "the ribbon itself: {rects:?}"
        );
        assert!(
            rects
                .iter()
                .position(|argb| *argb == BannerPainter::default_shadow().color.0)
                < rects
                    .iter()
                    .position(|argb| *argb == BannerPainter::COLOR.0),
            "and its shadow underneath it, not over it: {rects:?}"
        );
    }

    #[test]
    fn the_banner_is_rotated_a_quarter_turn_and_not_merely_moved() {
        // The whole shape is the rotation. Reading only the counts -- which is
        // all a test could do before the matrix was recorded -- cannot tell a
        // ribbon across the corner from a bar along the top.
        let calls = banner_drawn(true);
        let transform = calls
            .iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::TransformLayer { a, b, c, d, e, f } => {
                    Some((*a, *b, *c, *d, *e, *f))
                }
                _ => None,
            })
            .expect("the banner pushes a transform");
        let (sin, cos) = (std::f32::consts::FRAC_PI_4).sin_cos();
        assert!(
            (transform.0 - cos).abs() < 1e-4 && (transform.3 - cos).abs() < 1e-4,
            "a quarter turn: {transform:?}"
        );
        assert!(
            (transform.1 - sin).abs() < 1e-4,
            "turned towards the top right corner, not away from it: {transform:?}"
        );
        // `topEnd` in a left-to-right layout is the top right, so the ribbon
        // is translated the full width across and not down at all.
        assert!(
            (transform.4 - 200.0).abs() < 1e-4 && transform.5.abs() < 1e-4,
            "put at the top right corner: {transform:?}"
        );
    }

    #[test]
    fn each_corner_gets_its_own_translation_and_its_own_sign() {
        // Upstream's `_translationX`, `_translationY` and `_rotation`, which
        // are three switches over the same pair and disagree with each other
        // in ways no single rule would produce.
        let ltr = crate::direction::TextDirection::Ltr;
        let at = |location| BannerPainter::new("DEBUG", location).with_directions(ltr, ltr);
        let quarter = std::f32::consts::FRAC_PI_4;

        // The two top corners sit on the corner itself.
        assert_eq!(at(BannerLocation::TopStart).translation_x(200.0), 0.0);
        assert_eq!(at(BannerLocation::TopEnd).translation_x(200.0), 200.0);
        assert_eq!(at(BannerLocation::TopStart).translation_y(100.0), 0.0);

        // The two bottom ones are pulled back inside by the diagonal.
        let inset = BannerPainter::bottom_offset();
        assert!((inset - (40.0 + std::f32::consts::FRAC_1_SQRT_2 * 12.0)).abs() < 1e-4);
        assert_eq!(at(BannerLocation::BottomStart).translation_x(200.0), inset);
        assert_eq!(
            at(BannerLocation::BottomEnd).translation_x(200.0),
            200.0 - inset
        );
        assert_eq!(
            at(BannerLocation::BottomStart).translation_y(100.0),
            100.0 - inset
        );

        // And the sign flips along both diagonals, so opposite corners agree
        // and adjacent ones do not.
        assert!((at(BannerLocation::TopStart).rotation() + quarter).abs() < 1e-6);
        assert!((at(BannerLocation::BottomEnd).rotation() + quarter).abs() < 1e-6);
        assert!((at(BannerLocation::TopEnd).rotation() - quarter).abs() < 1e-6);
        assert!((at(BannerLocation::BottomStart).rotation() - quarter).abs() < 1e-6);
    }

    #[test]
    fn a_right_to_left_layout_swaps_the_corners_and_the_signs_together() {
        // `layoutDirection` is what reads `location`, and it is deliberately
        // not the same field as the one that reads the message.
        let ltr = crate::direction::TextDirection::Ltr;
        let rtl = crate::direction::TextDirection::Rtl;
        let end = |layout| {
            BannerPainter::new("DEBUG", BannerLocation::TopEnd).with_directions(ltr, layout)
        };
        assert_eq!(end(ltr).translation_x(200.0), 200.0, "top right");
        assert_eq!(end(rtl).translation_x(200.0), 0.0, "top left");
        assert!(
            end(ltr).rotation() * end(rtl).rotation() < 0.0,
            "and the ribbon turns the other way with it"
        );
    }

    #[test]
    fn the_banner_takes_no_taps_and_leaves_the_child_reachable() {
        // Upstream's `BannerPainter.hitTest` returns false. A ribbon that
        // swallowed the top-right corner of every screen would be a fault
        // shipped in every debug build.
        const CHILD: u64 = 4101;
        let mut tree = crate::framework::ElementTree::new();
        tree.rebuild(CheckedModeBanner::widget(
            true,
            crate::framework::leaf(|| {
                crate::widgets::Pointer::new(
                    CHILD,
                    crate::widgets::Container::new().with_size(200.0, 100.0),
                )
            }),
        ));
        let mut root = tree.build_render_tree().expect("mounted");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(200.0, 100.0),
        );
        let mut result = crate::render::HitTestResult::default();
        // Right in the middle of where the ribbon is drawn.
        crate::render::RenderBox::hit_test(
            &root,
            crate::render::Offset::new(180.0, 20.0),
            &mut result,
        );
        assert!(
            result.path.iter().any(|entry| entry.target == CHILD),
            "the child under the banner still answers"
        );
    }

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

    #[test]
    fn a_builder_only_app_may_not_also_describe_a_navigator() {
        // The half of upstream's assert this port had missing. A `builder` is
        // allowed to be the only way to a route -- and then `navigatorKey`,
        // `initialRoute` and `navigatorObservers` must still be at their
        // initial values, *"(null, null, and the empty list, respectively)"*.
        //
        // An application that set one of them is describing a navigator it is
        // not going to get, and nothing later in the frame would say so.
        let builder_only = WidgetsApp {
            routes: RouteConfiguration {
                has_home: false,
                routes_is_empty: true,
                has_builder: true,
                has_page_route_builder: true,
                ..RouteConfiguration::default()
            },
            ..WidgetsApp::new()
        };
        assert_eq!(builder_only.validate(), Ok(()), "a builder alone is fine");

        for (what, routes) in [
            (
                "a navigator key",
                RouteConfiguration {
                    has_navigator_key: true,
                    ..builder_only.routes
                },
            ),
            (
                "an initial route",
                RouteConfiguration {
                    has_initial_route: true,
                    ..builder_only.routes
                },
            ),
            (
                "an observer",
                RouteConfiguration {
                    has_navigator_observers: true,
                    ..builder_only.routes
                },
            ),
        ] {
            let app = WidgetsApp {
                routes,
                ..WidgetsApp::new()
            };
            assert!(
                app.validate().is_err(),
                "a builder-only app that also brought {what} is refused"
            );
        }

        // And an app that *does* have a route source may say all three: there
        // is a navigator for them to configure.
        let with_home = WidgetsApp {
            routes: RouteConfiguration {
                has_navigator_key: true,
                has_initial_route: true,
                has_navigator_observers: true,
                ..WidgetsApp::new().routes
            },
            ..WidgetsApp::new()
        };
        assert_eq!(with_home.validate(), Ok(()));
    }

    #[test]
    fn a_router_config_is_the_whole_arrangement_or_none_of_it() {
        // `WidgetsApp.router`'s first assert: *"If the routerConfig is
        // provided, all the other router delegates must not be provided"*. It
        // carries all four of the others at once, so giving both is an
        // ambiguity rather than a merge -- nothing says which would win.
        let config_alone = RouterConfiguration {
            has_router_config: true,
            ..RouterConfiguration::default()
        };
        assert_eq!(config_alone.validate(), Ok(()));

        for also in [
            RouterConfiguration {
                has_route_information_provider: true,
                ..config_alone
            },
            RouterConfiguration {
                has_route_information_parser: true,
                ..config_alone
            },
            RouterConfiguration {
                has_router_delegate: true,
                ..config_alone
            },
            RouterConfiguration {
                has_back_button_dispatcher: true,
                ..config_alone
            },
        ] {
            assert!(
                also.validate().is_err(),
                "a routerConfig alongside anything else is refused: {also:?}"
            );
        }
    }

    #[test]
    fn without_a_config_something_still_has_to_build_the_pages() {
        // The second assert: *"Either one of routerDelegate or routerConfig
        // must be provided"*. A router with neither has nowhere to get a page
        // from.
        assert!(RouterConfiguration::default().validate().is_err());
        assert_eq!(
            RouterConfiguration {
                has_router_delegate: true,
                ..RouterConfiguration::default()
            }
            .validate(),
            Ok(())
        );
    }

    #[test]
    fn route_information_needs_something_that_can_read_it() {
        // The third: *"If routeInformationProvider is provided,
        // routeInformationParser must also be provided"* -- a stream of route
        // information with no parser is a stream nothing can read.
        let provider_only = RouterConfiguration {
            has_router_delegate: true,
            has_route_information_provider: true,
            ..RouterConfiguration::default()
        };
        assert!(provider_only.validate().is_err());
        assert_eq!(
            RouterConfiguration {
                has_route_information_parser: true,
                ..provider_only
            }
            .validate(),
            Ok(())
        );

        // A parser without a provider is allowed: upstream's assert is one
        // way round, and an application may hand a parser to a `routerConfig`
        // it builds elsewhere.
        assert_eq!(
            RouterConfiguration {
                has_router_delegate: true,
                has_route_information_parser: true,
                ..RouterConfiguration::default()
            }
            .validate(),
            Ok(())
        );
    }

    #[test]
    fn an_app_routes_with_a_router_when_either_piece_is_there() {
        // Upstream's `_usesRouter`, which `MaterialApp` and `CupertinoApp` each
        // ask under their own name -- and which is the same question, so it
        // lives here with the rest of the router's rules.
        assert!(!RouterConfiguration::default().is_configured());
        assert!(
            RouterConfiguration {
                has_router_delegate: true,
                ..RouterConfiguration::default()
            }
            .is_configured()
        );
        assert!(
            RouterConfiguration {
                has_router_config: true,
                ..RouterConfiguration::default()
            }
            .is_configured()
        );
    }
}
