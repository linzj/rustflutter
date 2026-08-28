// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The gallery's state, its router, and the page frame every screen sits in.
//!
//! Maps to `lib/main.dart` (the `GalleryApp` wiring): everything the gallery
//! remembers lives in one [`GalleryState`], and every interactive control
//! mutates it through a [`StateHandle`]. That is not a stylistic choice: a
//! widget is rebuilt every frame, so a control cannot hold its own state, and
//! the handle is what lets a tap that happens between frames reach something
//! that outlives them.

use std::time::Duration;

use rustflutter::animation::Controller;
use rustflutter::components::Theme;
use rustflutter::framework::{
    component, leaf, many, provide, AnyWidget, BuildContext, StateHandle,
};
use rustflutter::media_query::{media_query_of, MediaQuery};
use rustflutter::navigation::{Navigator, Route, RouteArgs, Transition};
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex, StackPosition};
// The offset a page is scrolled to, the extent it is measured against, and the
// fling that keeps it moving after the finger lifts -- all three in the
// framework, because the album needs the same thing and neither of them should
// be writing scroll physics.
pub use rustflutter::scrolling::Scroll;
use rustflutter::services::system;
use rustflutter::widgets::{Container, Empty, ListView, Stack};

use rustflutter::platform::Brightness;

use crate::constants;
use crate::data::demos as catalog;
use crate::data::gallery_options::{GalleryOptions, ThemeMode};
use crate::demos::material as demos;
use crate::demos::reference::MotionValue;
use crate::pages::{self, adaptive_layout, backdrop, demo as demo_page, splash};
use crate::studies;
use crate::themes::gallery_theme_data::Scheme;

/// Hit-test identities.
///
/// Handed out from fixed bases rather than a counter, because an id has to be
/// the same across rebuilds -- a control whose id shifted between frames would
/// lose a press that was already in progress.
pub mod ids {
    /// The back button in the app bar.
    pub const BACK: u64 = 1;
    /// The settings button.
    pub const SETTINGS: u64 = 2;
    /// The scrim under a modal.
    pub const SCRIM: u64 = 4;
    /// Home page category headers, one per category.
    pub const CATEGORY: u64 = 100;
    /// The study cards in the home page carousel, one per study.
    pub const STUDY_CARD: u64 = 200;
    /// The scrollables. A scroll region is a hit-test target like any other:
    /// it has to have an identity so a drag that starts on a row can find it.
    pub const PAGE_SCROLL: u64 = 300;
    pub const CAROUSEL_SCROLL: u64 = 301;
    pub const SCREEN_SCROLL: u64 = 302;
    /// The desktop carousel's page buttons, and the home page's splash strip.
    pub const CAROUSEL_PREV: u64 = 303;
    pub const CAROUSEL_NEXT: u64 = 304;
    pub const SPLASH_STRIP: u64 = 305;
    /// The splash page's own layers.
    pub const SPLASH_FRONT: u64 = 306;
    pub const SPLASH_BACK: u64 = 307;
    /// The desktop home page's category column lists, offset by index.
    pub const CATEGORY_COLUMN_SCROLL: u64 = 320;
    /// Home page demo rows, one per demo, offset by index.
    pub const DEMO: u64 = 1_000;
    /// Everything a demo page puts on screen.
    pub const DEMO_LOCAL: u64 = 10_000;
    /// Study screens.
    pub const STUDY_LOCAL: u64 = 20_000;
    /// The settings panel. Reserved so a control added there cannot collide
    /// with a demo's.
    pub const SETTINGS_LOCAL: u64 = 30_000;
    /// The demo page's own chrome: the section icons and the content tap that
    /// dismisses a section.
    pub const DEMO_CHROME: u64 = 40_000;
}

/// Route names. Kept as constants because a typo in a string route is a screen
/// that silently never appears.
pub mod routes {
    pub const HOME: &str = "home";
    pub const DEMO: &str = "demo";
    pub const STUDY: &str = "study";
    pub const SETTINGS: &str = "settings";
}

/// Everything the gallery remembers.
pub struct GalleryState {
    pub navigator: Navigator,
    /// Upstream's `GalleryOptions`, carried by a `ModelBinding` at the root
    /// there and by this state here: theme mode, text scale, direction,
    /// locale, time dilation, platform override.
    pub options: GalleryOptions,
    /// One controller per home page category: 0 is closed, 1 is open, and the
    /// expansion animates between them (upstream's `_expansionAnimations`,
    /// `lib/pages/home.dart`).
    pub category_expand: Vec<Controller>,
    /// Whether the settings panel is open. The panel is not a route: upstream
    /// slides it over the home page as the backdrop's front layer
    /// (`lib/pages/backdrop.dart`), and these controllers drive that.
    pub settings_open: bool,
    pub backdrop_panel: Controller,
    /// The settings icon's gear-to-close morph.
    pub icon: Controller,
    /// The splash layer's pull-down reveal (`lib/pages/splash.dart`).
    pub splash: Controller,
    /// The home page's entrance stagger. Upstream sets it to 1.0 at initState
    /// when the splash is already finished, which is always, so it never
    /// visibly plays here either.
    pub entrance: Controller,
    /// One per expandable settings row (`pages/settings.rs`).
    pub setting_expand: Vec<Controller>,
    /// Which settings row is open, if any. Upstream's `_expandedSettingIndex`.
    pub expanded_setting: Option<usize>,
    /// Whether the about dialog is showing.
    pub about_open: bool,
    /// Which section the demo page is showing (`pages/demo.rs`).
    pub demo_section: demo_page::DemoSection,
    /// Which of the demo's configurations is on stage, for the demos that
    /// have any. Upstream's `_configIndex` (`lib/pages/demo.dart`).
    pub demo_config: usize,
    /// The state each demo page needs, all in one place so a demo can be a
    /// plain function rather than a component with a life cycle.
    pub demo: demos::DemoState,
    pub study: studies::StudyState,
    /// Which control is currently held, for the pressed look.
    pub pressed: Option<u64>,
    /// When the last frame was, so a transition advances by how long it has
    /// actually been rather than by an assumed frame interval.
    pub last_frame_micros: Option<i64>,
    /// The home page's vertical scroll, and the carousel's horizontal one.
    pub page: Scroll,
    pub carousel: Scroll,
    /// The desktop home page's per-category column scrolls, one per category.
    pub category_columns: Vec<Scroll>,
    /// Whatever screen is on top of the navigator.
    pub screen: Scroll,
}

impl Default for GalleryState {
    fn default() -> GalleryState {
        let mut category_expand: Vec<Controller> = catalog::CATEGORIES
            .iter()
            .map(|_| Controller::new(pages::category_list_item::EXPAND_DURATION))
            .collect();
        // Material is open to begin with, so the first screen shows
        // something to tap rather than three closed headers.
        category_expand[0].set_value(1.0);
        let mut entrance = Controller::new(Duration::from_millis(800));
        // Upstream sets the entrance controller to 1.0 at initState whenever
        // the splash is already finished -- which it always is at d12640d, so
        // the stagger never visibly plays there or here.
        entrance.set_value(1.0);
        GalleryState {
            navigator: Navigator::new(Route::new(routes::HOME))
                .with_duration(Duration::from_millis(280)),
            // Whatever the reader's system says, which is upstream's initial
            // `GalleryOptions.themeMode` -- a phone in day mode opens a light
            // gallery. `Gallery::theme_mode` overrides it where the answer has
            // to be the same on every machine; see [`Gallery::initial_state`].
            options: GalleryOptions::default(),
            category_expand,
            settings_open: false,
            // Upstream reads `isDesktop ? mobile : desktop` for this duration
            // (flutter/gallery @ d12640d, `lib/pages/backdrop.dart`) -- the
            // branches are inverted upstream, and this mirrors them as
            // written. The state is constructed before any MediaQuery exists,
            // so the port cannot switch per breakpoint and uses the desktop
            // constant.
            backdrop_panel: Controller::new(constants::SETTINGS_PANEL_DESKTOP_ANIMATION_DURATION),
            icon: Controller::new(Duration::from_millis(500)),
            splash: Controller::new(constants::SPLASH_PAGE_ANIMATION_DURATION),
            entrance,
            setting_expand: pages::settings::EXPANDABLE_SETTINGS
                .iter()
                .map(|_| Controller::new(pages::settings::EXPAND_DURATION))
                .collect(),
            expanded_setting: None,
            about_open: false,
            demo_section: demo_page::DemoSection::default(),
            demo_config: 0,
            demo: demos::DemoState::default(),
            study: studies::StudyState::default(),
            pressed: None,
            last_frame_micros: None,
            page: Scroll::default(),
            carousel: Scroll::default(),
            category_columns: catalog::CATEGORIES
                .iter()
                .map(|_| Scroll::default())
                .collect(),
            screen: Scroll::default(),
        }
    }
}

impl GalleryState {
    /// Upstream's full `ColorScheme`. The home page needs roles the component
    /// library has no name for, so the scheme is what gets passed around and
    /// the framework's `Theme` is derived from it.
    pub fn scheme(&self) -> Scheme {
        match self.options.resolved_brightness() {
            Brightness::Light => Scheme::light(),
            Brightness::Dark => Scheme::dark(),
        }
    }

    pub fn theme(&self) -> Theme {
        self.scheme().theme()
    }

    /// Upstream's `_shouldOpenList`: a closed, settled list opens; anything
    /// else -- open or mid-animation -- closes.
    pub fn toggle_category(&mut self, category: catalog::Category) {
        let Some(index) = category_index(category) else {
            return;
        };
        let controller = &mut self.category_expand[index];
        if controller.value() == 0.0 && !controller.is_running() {
            controller.forward();
        } else {
            controller.reverse();
        }
    }

    /// Opens or closes the settings panel, upstream's `_toggleSettings`.
    pub fn toggle_settings(&mut self) {
        self.settings_open = !self.settings_open;
        if self.settings_open {
            self.backdrop_panel.forward();
            self.icon.forward();
        } else {
            self.backdrop_panel.reverse();
            self.icon.reverse();
            // The expanded row is reset in `advance` once the panel has
            // fully closed.
        }
    }

    /// Expands a settings row, collapsing whichever one was open (upstream's
    /// `_toggleSetting` in `lib/pages/settings.dart`).
    pub fn toggle_setting(&mut self, index: usize) {
        let opening = self.expanded_setting != Some(index);
        for (i, controller) in self.setting_expand.iter_mut().enumerate() {
            if i == index && opening {
                controller.forward();
            } else if controller.value() > 0.0 || controller.is_running() {
                controller.reverse();
            }
        }
        self.expanded_setting = if opening { Some(index) } else { None };
    }

    /// Opens a demo page.
    pub fn open(&mut self, slug: &'static str) {
        if catalog::find(slug).is_none() {
            return;
        }
        self.push_screen(routes::DEMO, slug);
    }

    /// Opens a study. Separate from [`open`] because studies are a separate
    /// table upstream too -- they have artwork and a whole app behind them
    /// rather than an icon and one screen.
    pub fn open_study(&mut self, slug: &'static str) {
        if catalog::find_study(slug).is_none() {
            return;
        }
        self.push_screen(routes::STUDY, slug);
    }

    fn push_screen(&mut self, route: &'static str, slug: &'static str) {
        // A screen opens at its top, not wherever the last one was left.
        self.screen = Scroll::default();
        // Reset whatever the last visit left behind, so a screen always opens
        // in the state its description describes.
        self.demo = demos::DemoState::default();
        self.demo_section = demo_page::DemoSection::default();
        self.demo_config = 0;
        self.study = studies::StudyState::default();
        self.navigator.push(
            Route::new(route).with_args(RouteArgs::new().with("slug", slug)),
            Transition::SlideFromRight,
        );
    }

    pub fn back(&mut self) {
        self.navigator.pop();
    }

    /// Whether the screen on top has something moving on it.
    ///
    /// Two demos do: the progress spinner and the motion sample, both of which
    /// are functions of the frame clock and both of which run forever while
    /// they are on screen. Everything else is still, and a still screen should
    /// cost nothing.
    pub fn current_screen_animates(&self) -> bool {
        let route = self.navigator.current();
        if route.name != routes::DEMO {
            return false;
        }
        route
            .arg("slug")
            .is_some_and(|slug| ANIMATED_DEMOS.contains(&slug))
    }
}

/// The demos that never stop moving, by upstream's slugs.
///
/// Named here rather than matched inline so that a renamed slug is a failing
/// test rather than a screen that quietly stops animating -- which is what
/// happened when these were brought into line with upstream and this list was
/// not.
pub const ANIMATED_DEMOS: &[&str] = &["progress-indicator", "motion"];

/// Where a category sits in `catalog::CATEGORIES`, and so in the
/// per-category controller and scroll lists. Studies are the carousel, not a
/// list, so they have no index.
fn category_index(category: catalog::Category) -> Option<usize> {
    catalog::CATEGORIES.iter().position(|c| *c == category)
}

/// The longest a single frame is allowed to advance an animation by, roughly
/// three frames at sixty a second. See the clamp in [`Gallery::advance`].
pub const MAX_FRAME_MICROS: i64 = 50_000;

/// How long the indeterminate spinner takes to go round once.
pub const SPINNER_PERIOD_MICROS: i64 = 1_400_000;
/// How long the motion demo takes to run its sample out and back.
pub const MOTION_PERIOD_MICROS: i64 = 3_200_000;

/// The root of the gallery, and the only thing that owns state.
///
/// There is exactly one stateful component in the whole app. That is not an
/// accident: two of them with the same `State` type would each get their own,
/// and the one that navigated would not be the one that drew -- which is a bug
/// that looks like a screen that never changes.
pub struct Gallery {
    /// Which theme to open in, or `None` to follow the reader's system
    /// setting the way upstream's initial options do.
    ///
    /// A running app leaves this `None`; the headless renderer fills it in,
    /// because a screenshot has to be the same picture on every machine and
    /// the platform brightness is the one input that is not.
    pub theme_mode: Option<ThemeMode>,
    /// Where to open. Only the headless path uses anything but home; a running
    /// app starts at the top and navigates.
    pub route: &'static str,
    pub slug: Option<String>,
}

/// Wires the system back gesture to the navigator, once.
///
/// `popRoute` is what every platform sends when the reader asks to go back --
/// Android's back gesture, and a desktop window being asked to close. Upstream
/// this is `WidgetsBinding.handlePopRoute`, and the rule it implements is the
/// one below: pop if there is anything to pop, and otherwise tell the platform
/// to leave. An application that only popped would trap the reader on the home
/// screen with a back gesture that did nothing.
fn install_back_handler(handle: StateHandle<GalleryState>) {
    use std::cell::Cell;
    thread_local! {
        static INSTALLED: Cell<bool> = const { Cell::new(false) };
    }
    if INSTALLED.with(Cell::get) {
        return;
    }
    INSTALLED.with(|installed| installed.set(true));

    system::on_route_message(move |method, _arguments| {
        if method != "popRoute" {
            return;
        }
        handle.set_state(|state| {
            if !state.navigator.pop() {
                SystemNavigator::pop();
            }
        });
    });
}

impl StatefulComponent for Gallery {
    type State = GalleryState;

    fn advance(&self, state: &mut GalleryState, frame_time_micros: i64) -> bool {
        // How long it has actually been. The first frame has no previous one to
        // measure from, so it advances nothing and starts the clock.
        let elapsed = match state.last_frame_micros {
            Some(previous) if frame_time_micros > previous => {
                // Clamped, because the gap is not always the frame rate. Frames
                // are on demand: the app draws nothing while a page sits still,
                // so the tap that starts a transition is measured against a
                // frame from however long ago the user was last doing
                // something. Unclamped, a transition entered after a pause
                // would open at a third of the way through rather than at the
                // beginning. The cost of clamping is that a genuinely slow
                // stretch plays out slightly slower than real time, which is
                // the trade every animation loop makes.
                let gap = (frame_time_micros - previous).min(MAX_FRAME_MICROS);
                Duration::from_micros(gap as u64)
            }
            _ => Duration::ZERO,
        };
        state.last_frame_micros = Some(frame_time_micros);

        // Slow motion (`GalleryOptions.time_dilation`): everything the frame
        // advances sees dilated time, which is what the settings toggle is
        // for. Guarded against zero so a zeroed option cannot divide by it.
        let elapsed = elapsed.div_f64(state.options.time_dilation.max(0.01));
        let transitioning = state.navigator.tick(elapsed);

        // The backdrop, splash, home and settings controllers. Written out
        // rather than folded with `||`, which would stop at the first one
        // that says yes and leave the others frozen.
        let mut animating = state.backdrop_panel.tick(elapsed);
        animating |= state.icon.tick(elapsed);
        animating |= state.splash.tick(elapsed);
        animating |= state.entrance.tick(elapsed);
        for controller in &mut state.category_expand {
            animating |= controller.tick(elapsed);
        }
        for controller in &mut state.setting_expand {
            animating |= controller.tick(elapsed);
        }

        // Once the settings panel has fully closed, forget which row was
        // expanded: the next opening starts from all-collapsed, as upstream's
        // does when `_settingsPageController` finishes.
        if !state.settings_open
            && state.backdrop_panel.is_settled()
            && state.backdrop_panel.value() == 0.0
            && state.expanded_setting.is_some()
        {
            state.expanded_setting = None;
            for controller in &mut state.setting_expand {
                controller.set_value(0.0);
            }
        }

        // Every scrollable, not only the one on screen: which of them is
        // visible depends on the route, the flung one may be underneath a page
        // that is still sliding in, and a fling that is not advanced is a
        // fling that never stops. They cost nothing when nothing is moving.
        let mut flinging = state.page.advance(frame_time_micros);
        flinging |= state.carousel.advance(frame_time_micros);
        flinging |= state.screen.advance(frame_time_micros);
        for column in &mut state.category_columns {
            flinging |= column.advance(frame_time_micros);
        }

        // Only ask for another frame when something is actually moving. A
        // gallery that always asked would hold a core at sixty frames a second
        // to draw a page that has not changed since the last one.
        transitioning || animating || flinging || state.current_screen_animates()
    }

    fn initial_state(&self) -> GalleryState {
        let mut state = GalleryState::default();
        // Only when the launch asked for one. Left alone, the mode is
        // `ThemeMode::System` and resolves against `platform::brightness()`,
        // which is what upstream's gallery starts at.
        if let Some(theme_mode) = self.theme_mode {
            state.options.theme_mode = theme_mode;
        }
        match self.route {
            // Settings is the backdrop's front layer, not a route: land on
            // home with the panel already open.
            routes::SETTINGS => {
                state.settings_open = true;
                state.backdrop_panel.set_value(1.0);
                state.icon.set_value(1.0);
            }
            routes::DEMO => {
                if let Some(demo) = self.slug.as_deref().and_then(catalog::find) {
                    state.open(demo.slug);
                }
            }
            routes::STUDY => {
                if let Some(study) = self.slug.as_deref().and_then(catalog::find_study) {
                    state.open_study(study.slug);
                }
            }
            _ => {}
        }
        // Land on the destination rather than partway into its transition: a
        // screen that was opened before the first frame was never anywhere
        // else.
        state.navigator.tick(Duration::from_secs(5));
        state
    }

    fn build(
        &self,
        state: &GalleryState,
        handle: StateHandle<GalleryState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        install_back_handler(handle.clone());

        // Two animations, both of which run forever: a spinner spins and the
        // motion sample runs back and forth. Neither starts or stops, so both
        // are a function of the frame clock rather than of a controller --
        // state that is not needed is state that can be wrong.
        let now = context.frame_time_micros();
        let spinner = cycle(now, SPINNER_PERIOD_MICROS);
        let motion = ping_pong(now, MOTION_PERIOD_MICROS);

        // The breakpoint decides between the mobile and desktop layouts of
        // the home, settings, demo and splash pages.
        let is_desktop = adaptive_layout::is_display_desktop(context);

        // The settings page's text-scaling option is a MediaQuery override
        // republished over the whole tree, as upstream's `GalleryOptions`
        // does in `lib/main.dart`. `false` resolves the "system" sentinel to
        // whatever the platform reports.
        let media =
            media_query_of(context).with_text_scale(state.options.text_scale_factor(false) as f32);

        // The theme is published once at the root; every component below reads
        // it rather than being handed colours.
        provide(
            state.theme(),
            provide(
                demos::SpinnerValue(spinner),
                provide(
                    MotionValue(motion),
                    MediaQuery::new(media, page_stack(state, handle, is_desktop)),
                ),
            ),
        )
    }
}

/// 0 to 1, once per `period`.
pub fn cycle(now_micros: i64, period_micros: i64) -> f32 {
    if period_micros <= 0 {
        return 0.0;
    }
    (now_micros.rem_euclid(period_micros) as f32) / period_micros as f32
}

/// 0 to 1 and back, once per `period`.
pub fn ping_pong(now_micros: i64, period_micros: i64) -> f32 {
    let t = cycle(now_micros, period_micros);
    if t <= 0.5 {
        t * 2.0
    } else {
        (1.0 - t) * 2.0
    }
}

/// Builds the current screen, and the outgoing one if a transition is running.
///
/// The two are stacked with the incoming one on top, which is also the order
/// hit testing walks, so a tap during a transition goes to the screen that is
/// arriving rather than the one that is leaving.
fn page_stack(
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
    is_desktop: bool,
) -> AnyWidget {
    let presentation = state.navigator.presentation();
    let offsets = presentation.offsets();

    let current = screen(presentation.current, state, handle.clone(), is_desktop);
    if !presentation.is_transitioning() {
        return current;
    }

    let previous = presentation
        .previous
        .map(|route| screen(route, state, handle, is_desktop))
        .unwrap_or_else(|| leaf(|| Empty));
    let transition = presentation.transition;

    // Offsets are fractions of the view, and the widget layer works in logical
    // pixels, so the shift is applied as a fraction of whatever width the stack
    // turns out to have. A FractionalTranslation render object would be the
    // tidy way; this is the same arithmetic without one.
    many(vec![previous, current], move |mut rendered| {
        let current = rendered.pop().unwrap_or_else(|| boxed(Empty));
        let previous = rendered.pop().unwrap_or_else(|| boxed(Empty));
        Box::new(TransitionStack {
            previous,
            current,
            previous_offset: (offsets.previous_dx, offsets.previous_dy),
            current_offset: (offsets.current_dx, offsets.current_dy),
            previous_opacity: offsets.previous_opacity,
            current_opacity: offsets.current_opacity,
            // A slide dims the screen it is covering; a fade reveals one
            // through the other. Dimming is a rectangle over an opaque screen,
            // which costs one draw call; revealing needs the two composited,
            // which costs a full-screen offscreen pass every frame.
            dim_only: transition != Transition::Fade,
            size: rustflutter::render::Size::ZERO,
        })
    })
}

/// Lays two screens out at the same size and paints them at fractional offsets.
///
/// A render object rather than a composition because the offsets are fractions
/// of a size that is not known until layout, and a widget cannot see its own
/// size before it is laid out.
struct TransitionStack {
    previous: rustflutter::render::BoxedRender,
    current: rustflutter::render::BoxedRender,
    previous_offset: (f32, f32),
    current_offset: (f32, f32),
    previous_opacity: f32,
    current_opacity: f32,
    /// Whether a partly-transparent screen should be dimmed with a scrim
    /// rather than composited at reduced alpha.
    dim_only: bool,
    size: rustflutter::render::Size,
}

impl rustflutter::render::RenderBox for TransitionStack {
    fn layout(
        &mut self,
        constraints: rustflutter::render::BoxConstraints,
    ) -> rustflutter::render::Size {
        // Both screens are laid out at the full size: a screen sliding in from
        // the right is a whole screen that happens to be off to the right, not
        // a narrow one.
        let tight = rustflutter::render::BoxConstraints::tight_for(constraints.biggest());
        self.previous.layout(tight);
        self.current.layout(tight);
        self.size = constraints.biggest();
        self.size
    }

    fn size(&self) -> rustflutter::render::Size {
        self.size
    }

    fn paint(
        &self,
        context: &mut rustflutter::render::PaintContext,
        offset: rustflutter::render::Offset,
    ) {
        paint_layer(
            context,
            &self.previous,
            offset,
            self.size,
            self.previous_offset,
            self.previous_opacity,
            self.dim_only,
        );
        paint_layer(
            context,
            &self.current,
            offset,
            self.size,
            self.current_offset,
            self.current_opacity,
            self.dim_only,
        );
    }

    fn hit_test(
        &self,
        position: rustflutter::render::Offset,
        result: &mut rustflutter::render::HitTestResult,
    ) -> bool {
        if !self.size.contains(position) {
            return false;
        }
        // Front to back: the arriving screen is on top, so it gets the tap.
        let shifted = rustflutter::render::Offset::new(
            position.dx - self.current_offset.0 * self.size.width,
            position.dy - self.current_offset.1 * self.size.height,
        );
        self.current.hit_test(shifted, result);
        true
    }
}

fn paint_layer(
    context: &mut rustflutter::render::PaintContext,
    child: &dyn rustflutter::render::RenderBox,
    offset: rustflutter::render::Offset,
    size: rustflutter::render::Size,
    fraction: (f32, f32),
    opacity: f32,
    dim_only: bool,
) {
    if opacity <= 0.01 && !dim_only {
        return;
    }
    let placed = rustflutter::render::Offset::new(
        offset.dx + fraction.0 * size.width,
        offset.dy + fraction.1 * size.height,
    );
    let bounds = Rect::xywh(placed.dx, placed.dy, size.width, size.height);

    if opacity >= 0.99 {
        child.paint(context, placed);
        return;
    }

    if dim_only {
        // The screen underneath a slide is opaque, so "less visible" and
        // "darker" look the same. A rectangle over it is one draw call; the
        // alternative composites the whole screen through an offscreen buffer
        // every frame, which on an unoptimised build is the difference between
        // a sixteen millisecond frame and a hundred millisecond one.
        child.paint(context, placed);
        let scrim = Paint::new(Color::argb(
            (((1.0 - opacity) * 255.0).round() as u8).min(255),
            0,
            0,
            0,
        ));
        context.canvas().draw_rect(bounds, &scrim);
        return;
    }

    // A real opacity layer rather than a save layer inside the picture: the
    // compositor can distribute the alpha into the child's own draws when the
    // child allows it, and only falls back to an offscreen pass when it cannot.
    let alpha = (opacity * 255.0).round().clamp(0.0, 255.0) as u8;
    context.push_opacity(alpha, placed, child);
}

/// Dispatches a route to the screen that draws it.
fn screen(
    route: &Route,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
    is_desktop: bool,
) -> AnyWidget {
    match route.name.as_str() {
        routes::DEMO => match route.arg("slug").and_then(catalog::find) {
            Some(demo) => demo_page::page(demo, state, handle, is_desktop),
            None => missing(route),
        },
        routes::STUDY => match route.arg("slug").and_then(catalog::find_study) {
            Some(study) => studies::page(study, state, handle),
            None => missing(route),
        },
        // Home and settings are one screen: the settings panel is the
        // backdrop's front layer, and the splash's pull-down layer covers
        // both.
        _ => splash::page(
            state,
            handle.clone(),
            is_desktop,
            backdrop::page(state, handle, is_desktop),
        ),
    }
}

/// What an unroutable route draws.
///
/// A blank screen would be indistinguishable from a layout bug, so this says
/// what it could not find.
fn missing(route: &Route) -> AnyWidget {
    let name = route.name.clone();
    let slug = route.arg("slug").unwrap_or("(none)").to_string();
    component(Scaffold::new(leaf(move || {
        Center::new(
            // Min, so the `Center` has a content-sized column to centre;
            // the upstream default (max) would fill the screen and leave
            // the message at the top.
            Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_spacing(8.0)
                .push(Text::new("No such screen").with_size(20.0).with_weight(700))
                .push(Text::new(format!("route {name}, slug {slug}")).with_size(13.0)),
        )
    })))
}

// -- The frame every screen sits in -------------------------------------------

/// A screen with a title bar, a back button when there is somewhere to go back
/// to, and a body.
///
/// Every screen goes through this rather than building its own bar, so the back
/// button is in the same place on all of them and there is one definition of
/// what "back" means.
pub fn scaffold(
    title: &str,
    subtitle: Option<&str>,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
    body: AnyWidget,
) -> AnyWidget {
    let can_pop = state.navigator.can_pop();

    let mut bar = AppBar::new(title.to_string());
    if let Some(subtitle) = subtitle {
        bar = bar.with_subtitle(subtitle.to_string());
    }

    // Deeper screens get a back control. The home screen no longer goes
    // through here: its settings control is the backdrop's settings icon.
    if can_pop {
        bar = bar.with_trailing(component(
            Button::new(ids::BACK, "Back")
                .with_style(ButtonVariant::Outlined)
                .with_pressed(state.pressed == Some(ids::BACK))
                .wired(handle, |s| &mut s.pressed, |s| s.back()),
        ));
    }

    component(Scaffold::new(body).with_app_bar(component(bar)))
}

/// A page with no bar: just the background and the body.
///
/// This is the home page's frame. Upstream's home has no app bar either -- the
/// header is part of the scrolling list, so that it scrolls away, and the
/// settings control is the backdrop's settings icon rather than a bar item.
pub fn bare_page(
    _state: &GalleryState,
    _handle: StateHandle<GalleryState>,
    body: AnyWidget,
) -> AnyWidget {
    component(Scaffold::new(body))
}

/// The handlers a scrollable needs: touch it, drag it, throw it, or turn the
/// wheel over it.
///
/// They all end up in the same place. The signs differ because they describe
/// different things: a drag says where the content went, a wheel says where the
/// reader wants to go.
pub fn scroll_handlers(
    handle: StateHandle<GalleryState>,
    pick: impl Fn(&mut GalleryState) -> &mut Scroll + Copy + 'static,
    axis: rustflutter::render::Axis,
) -> rustflutter::gestures::PointerHandlers {
    use rustflutter::render::Axis;
    let down_handle = handle.clone();
    let drag_handle = handle.clone();
    let end_handle = handle.clone();
    rustflutter::gestures::PointerHandlers::new()
        // A finger on a moving list stops it, before it has travelled far
        // enough to be a drag. Catching a fling is half of what makes a long
        // list usable, and it has to happen on contact.
        .with_pointer_down(move |_| {
            down_handle.set_state(move |state| pick(state).stop());
        })
        .with_drag_update(move |drag| {
            let along = match axis {
                Axis::Vertical => drag.delta.dy,
                Axis::Horizontal => drag.delta.dx,
            };
            drag_handle.set_state(move |state| pick(state).scroll_by(-along));
        })
        // And letting go throws it. The velocity is negated for the reason the
        // drag delta is: it describes the finger, and the offset describes how
        // far into the content the reader has gone.
        .with_drag_end(move |end| {
            let along = match axis {
                Axis::Vertical => end.velocity.dy,
                Axis::Horizontal => end.velocity.dx,
            };
            end_handle.set_state(move |state| pick(state).fling(-along));
        })
        .with_scroll(move |scroll| {
            let along = match axis {
                Axis::Vertical => scroll.delta.dy,
                Axis::Horizontal => scroll.delta.dx,
            };
            handle.set_state(move |state| pick(state).scroll_by(along));
        })
}

/// A page body that scrolls, with the gallery's standard padding.
pub fn scrolling_body(
    children: Vec<AnyWidget>,
    spacing: f32,
    padding: f32,
    state: &GalleryState,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let offset = state.screen.offset;
    let extent = state.screen.extent.clone();
    let handlers = scroll_handlers(
        handle,
        |s| &mut s.screen,
        rustflutter::render::Axis::Vertical,
    );

    many(children, move |rendered| {
        // Stretch, so every card on a page is the same width. Safe here
        // because a card has no width of its own; the columns inside a card use
        // Start, because their children do.
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(spacing);
        for child in rendered {
            column = column.push(child);
        }
        let list = ListView::new()
            .with_offset(offset)
            .with_extent_sink(extent.clone())
            .push(column);
        Box::new(
            rustflutter::widgets::Pointer::new(
                ids::SCREEN_SCROLL,
                Container::new()
                    .with_padding(EdgeInsets::all(padding))
                    .with_child(list),
            )
            .with_handlers(handlers.clone()),
        )
    })
}

/// Puts `overlay` over `page`, filling the view. What a dialog or a sheet
/// needs, and the reason there is no overlay manager in the framework.
pub fn with_overlay(page: AnyWidget, overlay: Option<AnyWidget>) -> AnyWidget {
    match overlay {
        None => page,
        Some(overlay) => many(vec![page, overlay], |mut rendered| {
            let overlay = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let page = rendered.pop().unwrap_or_else(|| boxed(Empty));
            Box::new(Stack::new().push(page).push_positioned(
                overlay,
                StackPosition {
                    left: Some(0.0),
                    top: Some(0.0),
                    right: Some(0.0),
                    bottom: Some(0.0),
                    ..Default::default()
                },
            ))
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gallery() -> Gallery {
        Gallery {
            // Explicit, so a test's colours do not depend on the machine's
            // light/dark setting.
            theme_mode: Some(ThemeMode::Dark),
            route: routes::HOME,
            slug: None,
        }
    }

    #[test]
    fn a_launch_that_says_nothing_follows_the_system() {
        // Upstream's initial `GalleryOptions.themeMode` is `ThemeMode.system`,
        // and a phone in day mode opening a dark gallery was the complaint
        // this answers. Only the headless renderer pins a mode; see
        // `Gallery::theme_mode`.
        assert_eq!(
            GalleryState::default().options.theme_mode,
            ThemeMode::System
        );
        let following = Gallery {
            theme_mode: None,
            route: routes::HOME,
            slug: None,
        };
        assert_eq!(
            following.initial_state().options.theme_mode,
            ThemeMode::System
        );
        assert_eq!(
            gallery().initial_state().options.theme_mode,
            ThemeMode::Dark,
            "and a launch that does say is obeyed"
        );
    }

    #[test]
    fn every_animated_demo_is_in_the_catalogue() {
        // The list is matched against route arguments, so an entry that no
        // longer names a demo is a screen that silently stops moving.
        for slug in ANIMATED_DEMOS {
            assert!(catalog::find(slug).is_some(), "{slug} is not a demo");
        }
    }

    #[test]
    fn an_animated_demo_keeps_asking_for_frames() {
        let mut state = GalleryState::default();
        assert!(!state.current_screen_animates(), "home is still");
        state.open(ANIMATED_DEMOS[0]);
        assert!(
            state.current_screen_animates(),
            "the spinner has to keep spinning"
        );
        state.back();
        state.navigator.tick(Duration::from_secs(5));
        assert!(!state.current_screen_animates(), "and stop when it is left");
    }

    #[test]
    fn a_scroll_stays_inside_the_content() {
        let mut scroll = Scroll::default();
        scroll.extent.set(500.0);

        scroll.scroll_by(200.0);
        assert_eq!(scroll.offset, 200.0);
        // Past the end stops at the end rather than banking the travel: without
        // that, dragging back would do nothing until the imaginary distance had
        // been paid off.
        scroll.scroll_by(1000.0);
        assert_eq!(scroll.offset, 500.0);
        scroll.scroll_by(-10.0);
        assert_eq!(scroll.offset, 490.0);
        scroll.scroll_by(-10_000.0);
        assert_eq!(scroll.offset, 0.0);
    }

    #[test]
    fn a_list_that_fits_does_not_scroll() {
        // The extent starts at zero and stays there until something measures
        // it, so a scroll before the first layout must not move anything.
        let mut scroll = Scroll::default();
        scroll.scroll_by(120.0);
        assert_eq!(scroll.offset, 0.0);
    }

    #[test]
    fn opening_a_screen_puts_it_at_the_top() {
        let mut state = GalleryState::default();
        state.screen.extent.set(900.0);
        state.screen.scroll_by(400.0);
        state.open(catalog::DEMOS[0].slug);
        assert_eq!(state.screen.offset, 0.0, "a screen opens at its top");
    }

    /// A transition entered after the app has been sitting idle should start at
    /// the beginning, not partway through. Frames are on demand, so the gap
    /// since the last one says how long the user was reading the page -- not
    /// how long the animation should jump.
    #[test]
    fn a_long_gap_does_not_skip_a_transition_forward() {
        let gallery = gallery();
        let mut state = GalleryState::default();

        // One frame to start the clock, then a navigation.
        gallery.advance(&mut state, 0);
        state.open(catalog::DEMOS[0].slug);

        // Two seconds later the user taps and a frame arrives.
        let animating = gallery.advance(&mut state, 2_000_000);

        assert!(animating, "the transition should still be running");
        let progress = state.navigator.presentation().progress;
        assert!(
            progress < 0.3,
            "a transition should open near its start, got {progress}"
        );
    }

    /// The clamp slows a stalled animation down; it must not stop one.
    #[test]
    fn a_transition_still_finishes_over_enough_frames() {
        let gallery = gallery();
        let mut state = GalleryState::default();
        gallery.advance(&mut state, 0);
        state.open(catalog::DEMOS[0].slug);

        let mut now = 0;
        for _ in 0..40 {
            now += MAX_FRAME_MICROS;
            if !gallery.advance(&mut state, now) {
                break;
            }
        }
        assert_eq!(state.navigator.presentation().progress, 1.0);
        assert!(
            !gallery.advance(&mut state, now + MAX_FRAME_MICROS),
            "a settled gallery should stop asking for frames"
        );
    }

    /// Ordinary frame spacing is passed through untouched.
    #[test]
    fn a_normal_frame_is_not_clamped() {
        let gallery = gallery();
        let mut state = GalleryState::default();
        gallery.advance(&mut state, 0);
        state.open(catalog::DEMOS[0].slug);

        gallery.advance(&mut state, 16_667);
        let one = state.navigator.presentation().progress;
        gallery.advance(&mut state, 33_334);
        let two = state.navigator.presentation().progress;

        assert!(
            one > 0.0 && two > one,
            "progress should climb: {one} then {two}"
        );
        // Two 16.7ms frames of a transition, not two clamped 50ms ones.
        assert!(
            two < 0.5,
            "16ms frames should not advance like 50ms ones: {two}"
        );
    }
}
