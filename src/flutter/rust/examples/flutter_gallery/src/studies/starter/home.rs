// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/starter/home.dart` (flutter/gallery @ d12640d):
//! the starter `HomePage`, its `AdaptiveAppBar` and its `ListDrawer`.
//!
//! This is the module `studies::page` routes the `starter` slug to. Upstream
//! the page adapts on `isDisplayDesktop`: desktop pins the drawer beside the
//! body behind a vertical divider and grows the app bar to 128; mobile puts
//! the drawer in the scaffold with a menu button to open it and swaps the
//! extended floating action button for a round one.
//!
//! Divergences, each also marked at its site:
//!
//! * The drawer's open-and-shut is this page's own state. Upstream's
//!   `ScaffoldState` owns it through the `DrawerController` machinery -- the
//!   slide animation, the edge drag, the back-button history entry -- none of
//!   which is ported (see `rustflutter::drawer`'s module docs). A tap on the
//!   scrim still closes, upstream's `drawerBarrierDismissible`.
//! * The app bars are assembled by hand: the framework's `AppBar` has one
//!   trailing slot and no leading, where upstream's has the menu button and
//!   three actions. The shapes are upstream's (`kToolbarHeight`,
//!   `appBarDesktopHeight`, the 72/22 title inset).
//! * The action icons, the floating action buttons and the drawer items carry
//!   upstream's empty `onPressed: () {}` / select-only `onTap`s. With nothing
//!   to do, they draw enabled and carry no handlers; only the menu button,
//!   the scrim and the drawer's selection are wired.
//! * Tooltips (`starterAppTooltipShare` and friends) are not shown: a tooltip
//!   is a hover overlay and this page has no overlay to show one in.
//! * Text styles are the 2018 type scale's values for the roles upstream
//!   names -- `displaySmall` 48, `titleLarge` 20 w500, `titleMedium` 16,
//!   `bodyLarge` 16, `bodyMedium` 14 -- in the framework's default face
//!   (Roboto does not ship; see `starter/app.rs`). Body ink is the scale's
//!   `bodyColor`, black at 0xDD.
//! * The drawer's `ListView` is a plain column, the way the nav-drawer demo's
//!   is: ten items never outgrow the panel, and a drawer here owns no scroll
//!   state.
//! * `SafeArea` around the body and drawer is a no-op here and is omitted:
//!   the gallery's scaffold has already consumed the status-bar inset from
//!   the body's `MediaQuery`.

use rustflutter::framework::{
    AnyWidget, BuildContext, StateHandle, StatefulComponent, component, leaf, many, stateful,
};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, EdgeInsets, MainAxisSize, RenderFlex, RenderRef, RenderStack,
    StackFit, StackPosition,
};
use rustflutter::widgets::{Align, Pointer, RenderNavigationToolbar, SizedBox};

use crate::app::ids;
use crate::data::demos::{self as catalog, icon};
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::pages::adaptive_layout;
use crate::studies::starter::app as starter_app;

/// Upstream's `appBarDesktopHeight`.
pub(crate) const APP_BAR_DESKTOP_HEIGHT: f32 = 128.0;
/// Upstream's `kToolbarHeight` (`material/constants.dart`).
pub(crate) const TOOLBAR_HEIGHT: f32 = 56.0;

/// Upstream's `_ListDrawerState.numItems`.
const NUM_ITEMS: usize = 9;

/// The 2018 scale's `bodyColor` (`Typography.englishLike2018` applied to the
/// black geometry): the ink of everything upstream does not explicitly
/// colour, black at 0xDD.
const INK: Color = Color(0xDD000000);

/// A drawer item's unselected icon, upstream's M2 `ListTileThemeData`
/// default: `Colors.black45`.
const ICON_INK: Color = Color(0x73000000);

/// Upstream's `Icons.share`. The gallery's icon table (`data/demos.rs`) has
/// no entry for it; the codepoint is the Material font's, the way the table's
/// own aliases are.
const SHARE: &str = "\u{e593}";

/// The body `studies::page` routes the `starter` slug to. Upstream's
/// `StarterApp.defaultRoute` table entry: `_Home`, an `ApplyTextOptions`
/// around `HomePage` -- the text options ride the gallery root's
/// `MediaQuery`, so what is here is the theme over the page.
pub(crate) fn screen() -> AnyWidget {
    starter_app::app(stateful(HomePage))
}

/// Upstream's `HomePage`, a `StatelessWidget` there.
///
/// Stateful here for one reason: whether the drawer is open is application
/// state in this framework (upstream's `ScaffoldState` owns it through the
/// unported `DrawerController`), and `studies::page` hands this study no
/// shared state to keep it in.
struct HomePage;

#[derive(Default)]
struct HomeState {
    /// Whether the drawer is showing. Mobile only: on desktop the drawer is
    /// pinned beside the body and never opens or closes.
    drawer_open: bool,
}

impl StatefulComponent for HomePage {
    type State = HomeState;

    fn build(
        &self,
        state: &HomeState,
        handle: StateHandle<HomeState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let localizations = GalleryLocalizations::en();
        let is_desktop = adaptive_layout::is_display_desktop(context);

        let body = body(is_desktop, &localizations);

        if is_desktop {
            // Upstream: `Row[ListDrawer, VerticalDivider(width: 1),
            // Expanded(Scaffold(appBar: AdaptiveAppBar(isDesktop: true),
            // body: body, floatingActionButton: extended))]`.
            let fab = with_fab(body, extended_fab(&localizations));
            let scaffold = Scaffold::new(fab).with_app_bar(component(AdaptiveAppBar {
                is_desktop: true,
                open_drawer: None,
            }));
            let drawer = component(Drawer::new(stateful(ListDrawer)));
            many(
                vec![drawer, vertical_divider(), component(scaffold)],
                |mut rendered| {
                    let scaffold = rendered.pop().expect("three children");
                    let divider = rendered.pop().expect("three children");
                    let drawer = rendered.pop().expect("three children");
                    RenderRef::new(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .push(drawer)
                            .push(divider)
                            .push_flex(rustflutter::render::FlexChild::expanded(scaffold, 1)),
                    )
                },
            )
        } else {
            // Upstream: `Scaffold(appBar: AdaptiveAppBar(), body: body,
            // drawer: ListDrawer, floatingActionButton: round)`.
            let fab = with_fab(body, round_fab(&localizations));
            let open_handle = handle.clone();
            let bar = component(AdaptiveAppBar {
                is_desktop: false,
                open_drawer: Some(PointerHandlers::new().with_tap(move |_| {
                    // Upstream's `ScaffoldState.openDrawer`, behind the app
                    // bar's automatic drawer button.
                    open_handle.set_state(|state| state.drawer_open = true);
                })),
            });
            let scaffold = Scaffold::new(fab)
                .with_app_bar(bar)
                .with_drawer(component(Drawer::new(stateful(ListDrawer))))
                .with_drawer_open(state.drawer_open)
                // The barrier dismisses, upstream's `drawerBarrierDismissible`.
                .wired_drawer(ids::STUDY_LOCAL, handle, |state| state.drawer_open = false);
            component(scaffold)
        }
    }
}

/// Upstream's `body`: the padded headline / subtitle / body column.
///
/// The text styles are the 2018 scale's values for the roles upstream reads
/// (see the module header): `displaySmall` for the headline in
/// `colorScheme.onSecondary`, `titleMedium` for the subtitle, `bodyLarge`
/// for the body.
fn body(is_desktop: bool, localizations: &GalleryLocalizations) -> AnyWidget {
    // Upstream: desktop `EdgeInsets.symmetric(horizontal: 72, vertical: 48)`,
    // mobile `(horizontal: 16, vertical: 24)`.
    let (horizontal, vertical) = if is_desktop {
        (72.0, 48.0)
    } else {
        (16.0, 24.0)
    };
    let headline = localizations.starter_app_generic_headline();
    let subtitle = localizations.starter_app_generic_subtitle();
    let body = localizations.starter_app_generic_body();
    leaf(move || {
        Container::new()
            .with_padding(EdgeInsets::symmetric(horizontal, vertical))
            .with_child(
                Column::new()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Start)
                    .push(
                        Text::new(headline)
                            .with_size(48.0)
                            .with_color(starter_app::ON_SECONDARY),
                    )
                    // Upstream's `SizedBox(height: 10)`.
                    .push(SizedBox::new(0.0, 10.0))
                    .push(Text::new(subtitle).with_size(16.0).with_color(INK))
                    // Upstream's `SizedBox(height: 48)`.
                    .push(SizedBox::new(0.0, 48.0))
                    .push(Text::new(body).with_size(16.0).with_color(INK)),
            )
    })
}

/// The scaffold body with the floating action button over it, bottom end.
///
/// Upstream's `floatingActionButton` slot positions the button sixteen points
/// off the bottom and end edges (`_ScaffoldLayout` with
/// `FloatingActionButtonLocation.endFloat`); the framework's `Scaffold` has
/// no slot for it, so the same position is a stack here.
fn with_fab(body: AnyWidget, fab: AnyWidget) -> AnyWidget {
    many(vec![body, fab], |mut rendered| {
        let fab = rendered.pop().expect("two children");
        let body = rendered.pop().expect("two children");
        RenderStack::new()
            .with_fit(StackFit::Expand)
            .push(body)
            .push_positioned(
                fab,
                StackPosition {
                    right: Some(16.0),
                    bottom: Some(16.0),
                    ..Default::default()
                },
            )
    })
}

/// The mobile button, upstream's `FloatingActionButton` with `Icons.add`.
///
/// Upstream's `onPressed` is empty and its tooltip is `starterAppTooltipAdd`
/// ("Add"); the button draws enabled and carries no handler (see the module
/// header). `heroTag` has no counterpart: there is no hero machinery.
fn round_fab(_localizations: &GalleryLocalizations) -> AnyWidget {
    leaf(|| {
        Container::new()
            .with_size(56.0, 56.0)
            .with_corner_radius(28.0)
            .with_color(starter_app::SECONDARY)
            // `_FloatingActionButtonDefaultsM2.elevation`.
            .with_elevation(6)
            .with_child(Align::new(
                Alignment::CENTER,
                Text::new(icon::ADD)
                    .with_font_family(catalog::MATERIAL_ICONS)
                    .with_size(24.0)
                    .with_color(starter_app::ON_SECONDARY),
            ))
    })
}

/// The desktop button, upstream's `FloatingActionButton.extended` with
/// `Icons.add` and the `starterAppGenericButton` label.
///
/// The shape is Material 2's stadium for an extended button -- a 48-high
/// pill -- with the spec's 20-point horizontal padding around icon, gap,
/// label. The label style is the 2018 scale's `labelLarge` (button): 14,
/// w500, in `onSecondary` as upstream writes it.
fn extended_fab(localizations: &GalleryLocalizations) -> AnyWidget {
    let label = localizations.starter_app_generic_button();
    leaf(move || {
        Container::new()
            .with_height(48.0)
            .with_corner_radius(24.0)
            .with_color(starter_app::SECONDARY)
            .with_elevation(6)
            .with_padding(EdgeInsets::symmetric(20.0, 0.0))
            .with_child(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.0)
                    .push(
                        Text::new(icon::ADD)
                            .with_font_family(catalog::MATERIAL_ICONS)
                            .with_size(24.0)
                            .with_color(starter_app::ON_SECONDARY),
                    )
                    .push(
                        Text::new(label)
                            .with_size(14.0)
                            .with_weight(500)
                            .with_color(starter_app::ON_SECONDARY),
                    ),
            )
    })
}

/// Upstream's `AdaptiveAppBar`: a 56-high bar with the menu button and title
/// on mobile, a 128-high one with the title under the actions on desktop.
///
/// The framework's `AppBar` has no leading slot and one trailing widget, so
/// the bar is assembled by hand, the way the nav-drawer demo's is.
struct AdaptiveAppBar {
    is_desktop: bool,
    /// The menu button's tap. `None` on desktop, upstream's
    /// `automaticallyImplyLeading: !isDesktop`.
    open_drawer: Option<PointerHandlers>,
}

impl AdaptiveAppBar {
    /// One action icon: the 48-point button box around a 24-point glyph, in
    /// `onPrimary`. Upstream's `onPressed` is empty and the tooltips are not
    /// shown (see the module header), so the buttons draw and carry no
    /// handlers.
    fn action(glyph: &'static str, ink: Color) -> impl rustflutter::render::RenderBox {
        Container::new()
            .with_size(48.0, 48.0)
            .with_child(Align::new(
                Alignment::CENTER,
                Text::new(glyph)
                    .with_font_family(catalog::MATERIAL_ICONS)
                    .with_size(24.0)
                    .with_color(ink),
            ))
    }

    /// Upstream's `actions`: share, favorite, search.
    fn actions(ink: Color) -> RenderFlex {
        RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .push(Self::action(SHARE, ink))
            .push(Self::action(icon::FAVORITE, ink))
            .push(Self::action(icon::SEARCH, ink))
    }
}

impl Component for AdaptiveAppBar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        // The bar's fill and ink: `ThemeData`'s M2 app bar takes
        // `colorScheme.primary` for the bar and `onPrimary` over it.
        let primary = theme.primary;
        let on_primary = theme.on_primary;
        let localizations = GalleryLocalizations::en();
        let title = localizations.starter_app_generic_title();
        let is_desktop = self.is_desktop;
        let open_drawer = self.open_drawer.clone();

        leaf(move || {
            // The title in the bar's style: the 2018 scale's `titleLarge`
            // (headline6), 20 w500, in the bar's foreground colour -- both
            // upstream's toolbar text default and the desktop bottom title's
            // explicit `titleLarge.copyWith(color: onPrimary)`.
            let title_text = move || {
                Text::new(title.to_string())
                    .with_size(20.0)
                    .with_weight(500)
                    .with_color(on_primary)
            };

            if is_desktop {
                // Upstream's 128-high bar: the toolbar's actions along the
                // top, then the 26-high bottom `PreferredSize` with the title
                // inset 72 from the start and 22 from its bottom edge. The
                // padding plays the margin's part -- both inset the child the
                // same way when the strip carries no decoration of its own.
                let actions = Container::new()
                    .with_height(TOOLBAR_HEIGHT)
                    .with_padding(EdgeInsets::only(0.0, 0.0, 8.0, 0.0))
                    .with_alignment(Alignment::CENTER_RIGHT)
                    .with_child(Self::actions(on_primary));
                let bottom = Container::new()
                    .with_height(26.0)
                    .with_padding(EdgeInsets::only(72.0, 0.0, 0.0, 22.0))
                    .with_alignment(Alignment::CENTER_LEFT)
                    .with_child(title_text());
                Container::new()
                    .with_height(APP_BAR_DESKTOP_HEIGHT)
                    .with_color(primary)
                    .with_alignment(Alignment::TOP_LEFT)
                    .with_child(
                        Column::new()
                            .with_main_axis_size(MainAxisSize::Min)
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .push(actions)
                            .push(bottom),
                    )
            } else {
                let mut toolbar = RenderNavigationToolbar::new()
                    // `_getEffectiveCenterTitle` answers false off iOS/macOS.
                    .with_center_middle(false);
                if let Some(open) = &open_drawer {
                    toolbar = toolbar.with_leading(
                        Pointer::new(ids::STUDY_LOCAL + 1, Self::action(icon::MENU, on_primary))
                            .with_handlers(open.clone()),
                    );
                }
                toolbar = toolbar.with_middle(title_text()).with_trailing(
                    Container::new()
                        .with_padding(EdgeInsets::only(0.0, 0.0, 8.0, 0.0))
                        .with_child(Self::actions(on_primary)),
                );
                Container::new()
                    .with_height(TOOLBAR_HEIGHT)
                    .with_color(primary)
                    .with_child(toolbar)
            }
        })
    }
}

/// The desktop row's `VerticalDivider(width: 1)`: the starter theme's
/// divider colour at its one-point thickness.
fn vertical_divider() -> AnyWidget {
    leaf(|| {
        Container::new()
            .with_width(1.0)
            .with_color(starter_app::DIVIDER)
    })
}

/// Upstream's `ListDrawer` and `_ListDrawerState`: the header tile, a
/// divider, then nine selectable items.
struct ListDrawer;

#[derive(Default)]
struct ListDrawerState {
    /// Upstream's `selectedItem`.
    selected: usize,
}

impl StatefulComponent for ListDrawer {
    type State = ListDrawerState;

    fn build(
        &self,
        state: &ListDrawerState,
        handle: StateHandle<ListDrawerState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let localizations = GalleryLocalizations::en();

        // Upstream's header `ListTile`: the app title in `titleLarge` over
        // the generic subtitle in `bodyMedium`.
        let header = {
            let title = localizations.starter_app_title();
            let subtitle = localizations.starter_app_generic_subtitle();
            leaf(move || {
                Container::new()
                    .with_padding(EdgeInsets::symmetric(16.0, 14.0))
                    .with_child(
                        Column::new()
                            .with_main_axis_size(MainAxisSize::Min)
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .with_spacing(2.0)
                            .push(
                                Text::new(title)
                                    .with_size(20.0)
                                    .with_weight(500)
                                    .with_color(INK),
                            )
                            .push(Text::new(subtitle).with_size(14.0).with_color(INK)),
                    )
            })
        };

        let mut children = vec![header, component(Divider::new())];
        for index in 0..NUM_ITEMS {
            children.push(Self::item(
                index,
                index == state.selected,
                &localizations,
                &handle,
            ));
        }

        // Upstream's `Drawer(child: SafeArea(child: ListView(...)))`; the
        // SafeArea is a no-op here and the ListView a plain column -- see the
        // module header.
        component(Drawer::new(many(children, |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(column)
        })))
    }
}

impl ListDrawer {
    /// One item: the favorite glyph, then `starterAppDrawerItem(i + 1)`.
    ///
    /// Upstream's `onTap` only selects -- it does not close the drawer --
    /// and neither does this. The selected item draws its icon and title in
    /// `colorScheme.primary`, upstream's M2 `ListTile` selected colour; the
    /// selected tile's background is transparent there and here.
    fn item(
        index: usize,
        selected: bool,
        localizations: &GalleryLocalizations,
        handle: &StateHandle<ListDrawerState>,
    ) -> AnyWidget {
        let tap_handle = handle.clone();
        let handlers = PointerHandlers::new().with_tap(move |_| {
            // Upstream's `setState(() => selectedItem = i)`.
            tap_handle.set_state(move |state| state.selected = index);
        });
        let label = localizations.starter_app_drawer_item(index + 1);
        let ink = if selected { starter_app::PRIMARY } else { INK };
        let icon_ink = if selected {
            starter_app::PRIMARY
        } else {
            ICON_INK
        };
        let id = ids::STUDY_LOCAL + 16 + index as u64;
        leaf(move || {
            Pointer::new(
                id,
                Container::new()
                    .with_padding(EdgeInsets::symmetric(16.0, 12.0))
                    .with_child(
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Max)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            // Upstream's leading icon starts the title at 72:
                            // 16 of content padding, a 24-point glyph, and the
                            // 32 that remain.
                            .with_spacing(32.0)
                            .push(
                                Text::new(icon::FAVORITE)
                                    .with_font_family(catalog::MATERIAL_ICONS)
                                    .with_size(24.0)
                                    .with_color(icon_ink),
                            )
                            .push(Text::new(label.clone()).with_size(16.0).with_color(ink)),
                    ),
            )
            .with_handlers(handlers.clone())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox};

    fn lay_out(widget: AnyWidget, width: f32, height: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(width, height))
    }

    #[test]
    fn the_drawer_starts_closed_and_the_first_item_selected() {
        // Upstream: the `DrawerController` starts dismissed, and
        // `_ListDrawerState` starts at `selectedItem = 0`.
        assert!(!HomeState::default().drawer_open);
        assert_eq!(ListDrawerState::default().selected, 0);
    }

    #[test]
    fn the_item_count_is_upstream() {
        // `_ListDrawerState.numItems`.
        assert_eq!(NUM_ITEMS, 9);
    }

    #[test]
    fn the_bar_heights_are_upstream() {
        // `appBarDesktopHeight` and `kToolbarHeight`.
        assert_eq!(APP_BAR_DESKTOP_HEIGHT, 128.0);
        assert_eq!(TOOLBAR_HEIGHT, 56.0);
    }

    #[test]
    fn the_drawer_item_labels_are_upstream() {
        // `starterAppDrawerItem(1)` through `(9)`.
        let localizations = GalleryLocalizations::en();
        assert_eq!(localizations.starter_app_drawer_item(1), "Item 1");
        assert_eq!(localizations.starter_app_drawer_item(9), "Item 9");
    }

    #[test]
    fn the_mobile_screen_fills_what_it_is_offered() {
        let size = lay_out(screen(), 460.0, 820.0);
        assert_eq!(size, Size::new(460.0, 820.0));
    }

    #[test]
    fn the_desktop_screen_fills_what_it_is_offered() {
        // Over the 1024 medium breakpoint the drawer pins beside the body.
        let size = lay_out(screen(), 1280.0, 800.0);
        assert_eq!(size, Size::new(1280.0, 800.0));
    }

    #[test]
    fn the_desktop_bar_is_128_high() {
        let bar = component(AdaptiveAppBar {
            is_desktop: true,
            open_drawer: None,
        });
        let size = lay_out(starter_app::app(bar), 1280.0, 800.0);
        assert_eq!(size.height, APP_BAR_DESKTOP_HEIGHT);
    }
}
