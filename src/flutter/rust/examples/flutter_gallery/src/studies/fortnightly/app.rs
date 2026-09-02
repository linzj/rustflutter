// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/fortnightly/app.dart` (flutter/gallery @ d12640d), upstream's
//! the `FortnightlyApp`.
//!
//! The app root, the mobile home and the desktop home are all here, as they
//! are upstream. `FortnightlyApp.build` picks the home by
//! `isDisplayDesktop(context)`; the mobile home is a stateful widget for the
//! drawer's open/closed, the desktop home needs no state of its own (every
//! scrollable keeps its own, the way upstream's `ListView`s do).
//!
//! Divergences from upstream, beyond the ones `shared.rs` carries:
//!
//! - **The drawer gets a hamburger**: upstream sets
//!   `AppBar.automaticallyImplyLeading: false`, so no menu button is drawn
//!   and the drawer opens by an edge swipe. The framework has no edge-swipe
//!   gesture, so without a button the drawer -- and `NavigationMenu`'s
//!   closeable variant -- would be unreachable. The button is the one
//!   upstream's Scaffold would have drawn had the flag not suppressed it.
//! - **The drawer does not slide**: it appears and dismisses under its scrim
//!   without the 250ms `DrawerController` animation.
//! - **The search button is inert, faithfully**: upstream's `onPressed` is
//!   `() {}`. It draws; a tap does nothing, so it is given no hit-test id.
//! - **Semantics labels are not ported**: upstream wraps the wordmark in
//!   `Semantics(label: 'Fortnightly')`; no example screen here wires the
//!   framework's semantics tree.
//! - **The study still sits in the gallery's frame**: `studies::page` wraps
//!   the body in the gallery scaffold (title bar, back button), so the
//!   study's own app bar renders below it.
//!
//! This is the module `studies::page` routes the `fortnightly` slug to.

use rustflutter::framework::{
    AnyWidget, BuildContext, Component, StateHandle, StatefulComponent, component, leaf, many,
    provide, single, stateful,
};
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, EdgeInsets, FlexChild, MainAxisSize, RenderFlex, RenderPadding,
    StackPosition,
};
use rustflutter::widgets::{Center, Empty, Pointer, SizedBox, Stack};

use crate::data::demos::{self as catalog, icon};
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::pages::adaptive_layout::is_display_desktop;
use crate::studies::fortnightly::shared::{self, study_ids};

/// Upstream's `_fortnightlyTitle`.
const FORTNIGHTLY_TITLE: &str = "Fortnightly";
/// Upstream's `_FortnightlyHomeDesktop` `menuWidth`.
const MENU_WIDTH: f32 = 200.0;
/// Upstream's `_FortnightlyHomeDesktop` `spacer`.
const SPACER: f32 = 20.0;
/// Upstream's AppBar height, `kToolbarHeight`.
const TOOLBAR_HEIGHT: f32 = 56.0;

/// The body `studies::page` wraps in the study scaffold. Registers the
/// study's faces, then provides its theme (upstream's `buildTheme`) for
/// everything below.
pub(crate) fn screen() -> AnyWidget {
    // The gallery shell owns the title bar; the constant documents the
    // MaterialApp title against the route rather than driving anything here.
    let _ = FORTNIGHTLY_TITLE;
    shared::register_fonts();
    provide(shared::theme(), component(FortnightlyApp))
}

/// Upstream's `FortnightlyApp`: the theme and the adaptive home. The
/// `MaterialApp` concerns around it -- routes, localizations, restoration --
/// are the gallery shell's here.
struct FortnightlyApp;

impl Component for FortnightlyApp {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let home = if is_display_desktop(context) {
            desktop_home(context)
        } else {
            stateful(MobileHome)
        };
        // Upstream's `scaffoldBackgroundColor: Colors.white`, painted by the
        // study itself because the window behind it may be the gallery's dark
        // theme.
        single(home, move |inner| {
            Container::new().with_color(Color::WHITE).with_child(inner)
        })
    }
}

// -- Mobile -----------------------------------------------------------------------

/// Upstream's `_FortnightlyHomeMobile`. The state is the drawer: open or
/// closed. Upstream keeps it in the Scaffold's `DrawerController`; the
/// study's `StudyState` slot is shared and has no drawer field, so the flag
/// lives here, the way a per-demo `State` would.
struct MobileHome;

#[derive(Default)]
struct MobileHomeState {
    drawer_open: bool,
}

fn open_drawer(state: &mut MobileHomeState) {
    state.drawer_open = true;
}

fn close_drawer(state: &mut MobileHomeState) {
    state.drawer_open = false;
}

impl StatefulComponent for MobileHome {
    type State = MobileHomeState;

    fn build(
        &self,
        state: &MobileHomeState,
        handle: StateHandle<MobileHomeState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        // The app bar: hamburger, the wordmark, and the (inert) search
        // action, on white with no elevation -- upstream's `appBarTheme`.
        let bar = app_bar(&handle);

        // The body: the hashtag strip over the feed, each article padded by
        // upstream's `EdgeInsets.symmetric(horizontal: 16)`.
        let mut feed_children: Vec<AnyWidget> = vec![stateful(shared::HashtagBar {
            id: study_ids::HASHTAGS,
        })];
        for item in shared::build_article_preview_items() {
            feed_children.push(single(item, |inner| {
                RenderPadding::new(EdgeInsets::symmetric(16.0, 0.0), inner)
            }));
        }
        let feed = stateful(shared::ScrollColumn::new(study_ids::FEED, feed_children));

        let body = many(vec![bar, feed], |mut rendered| {
            let feed = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let bar = rendered.pop().unwrap_or_else(|| boxed(Empty));
            Box::new(
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .push(bar)
                    .push_flex(FlexChild::expanded(feed, 1)),
            )
        });

        if !state.drawer_open {
            return body;
        }

        // The open drawer: upstream's Scaffold drawer is a scrimmed panel over
        // the body, dismissed by the scrim or by the close control in the
        // menu's header row (upstream's `Navigator.pop`).
        let close_handle = handle.clone();
        let close_button = leaf(move || {
            let tap_handle = close_handle.clone();
            icon_button(study_ids::DRAWER_CLOSE, icon::CLOSE).with_handlers(
                rustflutter::gestures::PointerHandlers::new().with_tap(move |_| {
                    tap_handle.set_state(close_drawer);
                }),
            )
        });
        let menu = component(
            shared::NavigationMenu::new(true, study_ids::MENU_DRAWER)
                .with_close_button(close_button),
        );
        let scrim = component(Scrim::new(study_ids::DRAWER_SCRIM).wired(handle, close_drawer));
        let drawer = component(Drawer::new(menu));

        many(vec![body, scrim, drawer], |mut rendered| {
            let drawer = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let scrim = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let body = rendered.pop().unwrap_or_else(|| boxed(Empty));
            Box::new(
                Stack::new()
                    .push_positioned(body, StackPosition::fill())
                    .push_positioned(scrim, StackPosition::fill())
                    .push_positioned(
                        drawer,
                        StackPosition {
                            left: Some(0.0),
                            top: Some(0.0),
                            bottom: Some(0.0),
                            ..Default::default()
                        },
                    ),
            )
        })
    }
}

/// The mobile app bar: 56 high, white, flat. The hamburger is the documented
/// divergence in the module header; the search action is upstream's
/// `IconButton(icon: Icon(Icons.search), onPressed: () {})` -- drawn, and a
/// tap does nothing.
fn app_bar(handle: &StateHandle<MobileHomeState>) -> AnyWidget {
    let open_handle = handle.clone();
    let hamburger = leaf(move || {
        let tap_handle = open_handle.clone();
        icon_button(study_ids::DRAWER_OPEN, icon::MENU).with_handlers(
            rustflutter::gestures::PointerHandlers::new().with_tap(move |_| {
                tap_handle.set_state(open_drawer);
            }),
        )
    });
    // `Semantics(label: _fortnightlyTitle, child: wordmark)`: the label is
    // not ported (see the module header); the wordmark is.
    let wordmark = leaf(|| {
        Container::new()
            .with_alignment(Alignment::CENTER_LEFT)
            .with_child(shared::title_image())
    });
    let search = leaf(|| {
        Center::new(
            Text::new(icon::SEARCH)
                .with_font_family(catalog::MATERIAL_ICONS)
                .with_size(24.0)
                .with_color(Color::BLACK),
        )
    });
    many(
        vec![
            hamburger,
            wordmark,
            // The bar is white and flat: `AppBarTheme(color: Colors.white,
            // elevation: 0)`.
            single(search, |icon| SizedBox::new(48.0, 48.0).with_child(icon)),
        ],
        |mut rendered| {
            let search = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let wordmark = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let hamburger = rendered.pop().unwrap_or_else(|| boxed(Empty));
            Box::new(
                Container::new().with_color(Color::WHITE).with_child(
                    SizedBox::height(TOOLBAR_HEIGHT).with_child(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .push(hamburger)
                            .push_flex(FlexChild::expanded(wordmark, 1))
                            .push(search),
                    ),
                ),
            )
        },
    )
}

/// A 48-by-48 IconButton tap target with a 24-point glyph, the Material
/// default upstream's `IconButton` draws.
fn icon_button(id: u64, glyph: &'static str) -> rustflutter::render::RenderPointerRegion {
    Pointer::new(
        id,
        SizedBox::new(48.0, 48.0).with_child(Center::new(
            Text::new(glyph)
                .with_font_family(catalog::MATERIAL_ICONS)
                .with_size(24.0)
                .with_color(Color::BLACK),
        )),
    )
}

// -- Desktop ----------------------------------------------------------------------

/// Upstream's `_FortnightlyHomeDesktop`: the header row (wordmark, hashtags,
/// search), then the three columns -- menu, feed, stocks + videos -- in a 16
/// margin.
fn desktop_home(context: &mut BuildContext) -> AnyWidget {
    // `headerHeight = 40 * reducedTextScale(context)`.
    let header_height = 40.0 * shared::reduced_text_scale(context);

    let header = desktop_header(header_height);

    let menu = single(
        component(shared::NavigationMenu::new(false, study_ids::MENU_DESKTOP)),
        |menu| SizedBox::width(MENU_WIDTH).with_child(menu),
    );
    let feed = stateful(shared::ScrollColumn::new(
        study_ids::FEED,
        shared::build_article_preview_items(),
    ));
    let mut sidebar_children = shared::build_stock_items();
    sidebar_children.push(leaf(|| SizedBox::height(32.0)));
    sidebar_children.extend(shared::build_video_preview_items());
    let sidebar = stateful(shared::ScrollColumn::new(
        study_ids::SIDEBAR,
        sidebar_children,
    ));

    let body = many(
        vec![
            menu,
            leaf(|| SizedBox::width(SPACER)),
            feed,
            leaf(|| SizedBox::width(SPACER)),
            sidebar,
        ],
        |mut rendered| {
            let sidebar = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let spacer2 = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let feed = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let spacer1 = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let menu = rendered.pop().unwrap_or_else(|| boxed(Empty));
            Box::new(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .push(menu)
                    .push(spacer1)
                    // Upstream's Flexibles are all `FlexFit.tight`.
                    .push_flex(FlexChild::expanded(feed, 2))
                    .push(spacer2)
                    .push_flex(FlexChild::expanded(sidebar, 1)),
            )
        },
    );

    many(vec![header, body], |mut rendered| {
        let body = rendered.pop().unwrap_or_else(|| boxed(Empty));
        let header = rendered.pop().unwrap_or_else(|| boxed(Empty));
        Box::new(
            Container::new()
                .with_padding(EdgeInsets::all(16.0))
                .with_child(
                    RenderFlex::column()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push(header)
                        .push_flex(FlexChild::expanded(body, 1)),
                ),
        )
    })
}

/// The desktop header: the wordmark in a menu-width slot, the hashtag bar at
/// flex 2, and the search action at flex 1, right-aligned.
fn desktop_header(header_height: f32) -> AnyWidget {
    let wordmark = leaf(|| {
        Container::new()
            .with_width(MENU_WIDTH)
            // `margin: const EdgeInsets.only(left: 12)`.
            .with_margin(EdgeInsets {
                left: 12.0,
                ..EdgeInsets::ZERO
            })
            .with_alignment(Alignment::CENTER_LEFT)
            .with_child(shared::title_image())
    });
    let hashtags = stateful(shared::HashtagBar {
        id: study_ids::HASHTAGS,
    });
    // The inert search action, right-aligned in its flexible slot.
    let search = leaf(|| {
        Container::new()
            .with_alignment(Alignment::CENTER_RIGHT)
            .with_child(Center::new(
                Text::new(icon::SEARCH)
                    .with_font_family(catalog::MATERIAL_ICONS)
                    .with_size(24.0)
                    .with_color(Color::BLACK),
            ))
    });

    many(
        vec![
            wordmark,
            leaf(|| SizedBox::width(SPACER)),
            hashtags,
            leaf(|| SizedBox::width(SPACER)),
            search,
        ],
        move |mut rendered| {
            let search = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let spacer2 = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let hashtags = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let spacer1 = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let wordmark = rendered.pop().unwrap_or_else(|| boxed(Empty));
            Box::new(
                SizedBox::height(header_height).with_child(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .push(wordmark)
                        .push(spacer1)
                        .push_flex(FlexChild::expanded(hashtags, 2))
                        .push(spacer2)
                        .push_flex(FlexChild::expanded(search, 1)),
                ),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studies::fortnightly::routes;

    #[test]
    fn the_app_names_itself() {
        // `_fortnightlyTitle` and the route the app registers its home under.
        assert_eq!(FORTNIGHTLY_TITLE, "Fortnightly");
        assert_eq!(routes::DEFAULT_ROUTE, "/fortnightly");
    }

    #[test]
    fn the_drawer_opens_and_closes() {
        let mut state = MobileHomeState::default();
        assert!(!state.drawer_open);
        open_drawer(&mut state);
        assert!(state.drawer_open);
        close_drawer(&mut state);
        assert!(!state.drawer_open);
    }

    #[test]
    fn desktop_metrics_are_upstreams() {
        assert_eq!(MENU_WIDTH, 200.0);
        assert_eq!(SPACER, 20.0);
        assert_eq!(TOOLBAR_HEIGHT, 56.0);
        // `GalleryLocalizations.of(context).shrineTooltipSearch` -- the search
        // tooltip really is Shrine's string upstream.
        assert_eq!(GalleryLocalizations::en().shrine_tooltip_search(), "Search");
    }
}
