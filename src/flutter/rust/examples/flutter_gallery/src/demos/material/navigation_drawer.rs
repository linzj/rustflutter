// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/navigation_drawer.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `NavDrawerDemo` is a `Scaffold` whose `AppBar` gets an automatic
//! drawer button, whose body is a centered line of text, and whose `drawer` is
//! a `Drawer` with a `UserAccountsDrawerHeader` and two tappable items. Here
//! the scaffold is the framework's (`components.rs`) and the panel the
//! framework's `Drawer` (`drawer.rs`); opening it is
//! [`rustflutter::show_drawer`], which is upstream's `DrawerController` -- the
//! 246ms slide, the scrim fading up behind it, and a barrier tap that closes.
//!
//! This file used to say that whether the drawer is open is the demo's own
//! state, "the way every overlay's is", and pointed at `drawer.rs`'s note that
//! the `DrawerController` machinery is not ported. The slide is back; what is
//! still missing is the edge drag (desktop-only upstream) and the back-button
//! history entry (there is no Navigator to hold it).
//!
//! Divergences, each also marked at its site:
//!
//! * The framework's `AppBar` has no leading slot, so the bar is built by
//!   hand here with the menu button on the leading side, the way upstream's
//!   `automaticallyImplyLeading` puts it.
//! * The scaffold is height-bounded ([`DEMO_HEIGHT`]): upstream's fills the
//!   demo screen, and the stage asks its content how tall it is.
//! * `UserAccountsDrawerHeader`'s account switcher arrow and the
//!   `otherAccountsPictures` row have no counterpart in what the demo sets,
//!   and are absent here as they are upstream; the header is a primary-colored
//!   panel with the avatar, name and email, sized to its content rather than
//!   to upstream's fixed 160.

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, Center, Pointer, SizedBox};

use rustflutter::{overlay, show_drawer, DrawerControls, DrawerSide, OverlayHandle};

use crate::app::ids;
use crate::data::demos::{self as catalog, icon};
use crate::pages::splash;

/// How tall the demo's scaffold is. Upstream the demo fills the screen; the
/// stage lays its content out unbounded, so the scaffold needs an explicit
/// height to fill.
const DEMO_HEIGHT: f32 = 420.0;

/// The demo body for the `nav_drawer` slug.
pub(super) fn stage() -> AnyWidget {
    stateful(NavDrawerDemo)
}

/// Upstream's `NavDrawerDemo`.
struct NavDrawerDemo;

/// What the demo holds on to: the controls for whatever drawer it put up.
///
/// Not a bool. `open` used to be one, because the drawer was a slot in the
/// scaffold that was either filled or not; a drawer that slides has states in
/// between, and the controls are what has them.
#[derive(Default)]
struct NavDrawerState {
    drawer: Option<DrawerControls>,
}

impl StatefulComponent for NavDrawerDemo {
    type State = NavDrawerState;

    fn build(
        &self,
        state: &NavDrawerState,
        handle: StateHandle<NavDrawerState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let bar = component(DrawerBar {
            drawer: state.drawer.clone(),
            open_handle: handle.clone(),
        });

        // Upstream's body: `Center(Padding(EdgeInsets.all(50.0), Text(...)))`,
        // the text `demoNavigationDrawerText`.
        let body = component(BodyText);

        // The scaffold has no drawer slot filled: the drawer is an overlay
        // entry, and the overlay is the demo's own so the panel slides out of
        // the card's edge and the scrim covers the card -- upstream's drawer
        // is over the demo's screen, not the whole window.
        let scaffold = Scaffold::new(body).with_app_bar(bar);

        let card = single(component(scaffold), |inner| {
            Box::new(Container::new().with_height(DEMO_HEIGHT).with_child(inner))
        });
        overlay(card)
    }
}

/// The app bar. The framework's `AppBar` puts its one action on the trailing
/// side and has no leading slot, so this bar is assembled by hand: the menu
/// button leading, the title (`demoNavigationDrawerTitle`) after it, at the
/// same 56-pixel toolbar height.
struct DrawerBar {
    /// The controls for whatever drawer the demo has up, so a second tap on
    /// the button does not put up a second one.
    drawer: Option<DrawerControls>,
    /// Where the opened drawer's controls go, so the next build has them.
    open_handle: StateHandle<NavDrawerState>,
}

impl Component for DrawerBar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let surface = theme.surface;
        let outline = theme.outline;
        let ink = theme.text;
        let title = theme.title();

        // The demo's own overlay, which the card is wrapped in -- looked up
        // here, below the overlay's `provide`, rather than in the demo's
        // build, which runs above it and would find the window's.
        let overlay = OverlayHandle::of(context);
        let drawer = self.drawer.clone();
        let open_handle = self.open_handle.clone();
        let handlers = PointerHandlers::new().with_tap(move |_| {
            // Upstream's `ScaffoldState.openDrawer`, behind the app bar's
            // automatic drawer button. Asked now, not at build time: the
            // drawer can have slid away and been removed since this handler
            // was built, and a guard read then would refuse every tap after
            // the first one.
            let Some(overlay) = overlay.clone() else {
                return;
            };
            let already_open = drawer
                .as_ref()
                .is_some_and(|controls| controls.is_attached());
            if already_open {
                return;
            }
            let panel_handle = open_handle.clone();
            let opened = show_drawer(overlay, DrawerSide::Start, move || {
                component(Drawer::new(component(DrawerItems {
                    handle: panel_handle.clone(),
                })))
            });
            if let Some((_, controls)) = opened {
                open_handle.set_state(move |state| state.drawer = Some(controls));
            }
        });

        leaf(move || {
            let button = Pointer::new(
                ids::DEMO_LOCAL,
                Container::new()
                    .with_size(48.0, 48.0)
                    .with_child(Align::new(
                        Alignment::CENTER,
                        Text::new(icon::MENU)
                            .with_font_family(catalog::MATERIAL_ICONS)
                            .with_size(24.0)
                            .with_color(ink),
                    )),
            )
            .with_handlers(handlers.clone());
            Container::new()
                .with_height(56.0)
                .with_color(surface)
                .with_border(1.0, outline)
                .with_child(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(16.0)
                        .push(button)
                        .push(Text::new("Navigation Drawer").with_style(title.clone())),
                )
        })
    }
}

/// The body's centered line. Upstream's `demoNavigationDrawerText`.
struct BodyText;

impl Component for BodyText {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let body = theme_of(context).body();
        leaf(move || {
            Center::new(
                Container::new()
                    .with_padding(EdgeInsets::all(50.0))
                    .with_child(
                        Text::new(
                            "Swipe from the edge or tap the upper-left icon to see the drawer",
                        )
                        .with_style(body.clone()),
                    ),
            )
        })
    }
}

/// The drawer's content, upstream's `drawerItems` `ListView`: the
/// `UserAccountsDrawerHeader`, then "Item One" (`Icons.favorite`) and
/// "Item Two" (`Icons.comment`), each closing the drawer on tap -- upstream's
/// `onTap: Navigator.pop`.
struct DrawerItems {
    handle: StateHandle<NavDrawerState>,
}

impl Component for DrawerItems {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let ink = theme.text;
        let body = theme.body();
        let on_primary = theme.on_primary;
        let primary = theme.primary;

        let item = |index: u64, glyph: &'static str, label: &'static str| {
            let handle = self.handle.clone();
            let handlers = PointerHandlers::new().with_tap(move |_| {
                // Upstream's `Navigator.pop` on the drawer's route: the item
                // closes it. Through the controls rather than a flag, so the
                // panel slides out instead of vanishing.
                handle.set_state(|state| {
                    if let Some(controls) = &state.drawer {
                        controls.close();
                    }
                });
            });
            let ink = ink;
            let body = body.clone();
            leaf(move || {
                Pointer::new(
                    ids::DEMO_LOCAL + 1 + index,
                    Container::new()
                        .with_padding(EdgeInsets::symmetric(16.0, 12.0))
                        .with_child(
                            RenderFlex::row()
                                .with_main_axis_size(MainAxisSize::Max)
                                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                .with_spacing(16.0)
                                .push(Align::new(
                                    Alignment::CENTER,
                                    Text::new(glyph)
                                        .with_font_family(catalog::MATERIAL_ICONS)
                                        .with_size(24.0)
                                        .with_color(ink),
                                ))
                                .push(Text::new(label).with_style(body.clone())),
                        ),
                )
                .with_handlers(handlers.clone())
            })
        };

        // Upstream's `UserAccountsDrawerHeader`: the Flutter logo in a circle
        // over the account name (`demoNavigationDrawerUserName`) and email
        // (`demoNavigationDrawerUserEmail`), on the theme's primary color.
        let logo = Image::shared("flutter_logo", splash::FLUTTER_LOGO);
        let header = leaf(move || {
            let avatar = Container::new()
                .with_size(72.0, 72.0)
                .with_color(on_primary)
                .with_corner_radius(36.0)
                .with_child(match logo.clone() {
                    // Upstream's `CircleAvatar(child: FlutterLogo(size: 42.0))`.
                    Some(image) => Align::new(
                        Alignment::CENTER,
                        SizedBox::new(42.0, 42.0).with_child(
                            rustflutter::widgets::ImageView::with_fit(
                                image,
                                rustflutter::render::BoxFit::Contain,
                            ),
                        ),
                    ),
                    None => Align::new(Alignment::CENTER, rustflutter::widgets::Empty),
                });
            Container::new()
                .with_color(primary)
                .with_padding(EdgeInsets::only(16.0, 16.0, 16.0, 8.0))
                .with_child(
                    Column::new()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(8.0)
                        .push(avatar)
                        .push(
                            Text::new("User Name")
                                .with_color(on_primary)
                                .with_weight(700),
                        )
                        .push(Text::new("user.name@example.com").with_color(on_primary)),
                )
        });

        column_items(vec![
            header,
            // `demoNavigationDrawerToPageOne`, leading `Icons.favorite`.
            item(0, icon::FAVORITE, "Item One"),
            // `demoNavigationDrawerToPageTwo`, leading `Icons.comment`; the
            // catalog's icon table has no comment glyph, so the feedback
            // bubble -- the same speech-bubble shape -- stands in.
            item(1, icon::FEEDBACK, "Item Two"),
        ])
    }
}

/// A plain vertical stack, for the drawer's content.
fn column_items(children: Vec<AnyWidget>) -> AnyWidget {
    many(children, |rendered| {
        let mut flex = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for child in rendered {
            flex = flex.push(child);
        }
        Box::new(flex)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox};

    #[test]
    fn there_is_no_drawer_until_the_button_is_pressed() {
        // Upstream's `Scaffold` builds its drawer lazily behind the automatic
        // button; there is nothing on screen before the press.
        assert!(NavDrawerState::default().drawer.is_none());
    }

    #[test]
    fn the_panel_slides_rather_than_appearing() {
        // What the rewiring bought, and the thing `drawer.rs` said it could not
        // have without a route-like owner: the panel is partway in, not present
        // or absent. The owner is an overlay entry.
        let mut animation = rustflutter::drawer_host::DrawerAnimation::default();
        animation.open();
        animation.advance(0);
        assert_eq!(animation.progress(), 0.0);

        let quarter = (rustflutter::drawer::BASE_SETTLE_MILLISECONDS as i64 * 1000) / 4;
        animation.advance(quarter);
        let at = animation.progress();
        assert!(at > 0.0 && at < 1.0, "partway in: {at}");

        animation.advance(rustflutter::drawer::BASE_SETTLE_MILLISECONDS as i64 * 1000);
        assert_eq!(animation.progress(), 1.0, "and arrives in 246ms");
    }

    #[test]
    fn a_closing_drawer_stays_on_screen_until_it_has_left() {
        // Removing the entry when `close` was called is what made the old
        // drawer pop rather than close.
        let mut animation = rustflutter::drawer_host::DrawerAnimation::default();
        animation.open();
        animation.advance(0);
        animation.advance(rustflutter::drawer::BASE_SETTLE_MILLISECONDS as i64 * 1000);
        animation.close();
        assert!(!animation.is_closed(), "on its way out, still visible");

        animation.advance(rustflutter::drawer::BASE_SETTLE_MILLISECONDS as i64 * 3000);
        assert!(animation.is_closed());
    }

    #[test]
    fn the_stage_is_the_demo_height() {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), stage()));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::loose(460.0, 820.0));
        assert_eq!(size.height, DEMO_HEIGHT);
    }

    #[test]
    fn the_drawers_items_fit_its_width() {
        // The drawer's content laid out at the framework's drawer width:
        // upstream's `_kWidth`, 304.
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), stateful(DrawerItemsProbe)));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::new(0.0, 304.0, 0.0, f32::INFINITY));
        assert!(size.width <= 304.0, "{size:?}");
    }

    /// Builds `DrawerItems` with a handle to itself, the way the demo does.
    struct DrawerItemsProbe;

    impl StatefulComponent for DrawerItemsProbe {
        type State = NavDrawerState;

        fn build(
            &self,
            _state: &NavDrawerState,
            handle: StateHandle<NavDrawerState>,
            _context: &mut BuildContext,
        ) -> AnyWidget {
            component(DrawerItems { handle })
        }
    }
}
