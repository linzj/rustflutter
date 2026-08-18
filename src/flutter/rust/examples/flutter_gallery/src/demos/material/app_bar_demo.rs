// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/app_bar_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream the demo is a `Scaffold` whose `AppBar` carries a leading menu
//! `IconButton`, the title and three actions -- favorite, search and an
//! overflow `PopupMenuButton` with three value-less items -- over a centred
//! "Home" body. Every button's callback is empty upstream (`onPressed:
//! () {}`), so the only stateful surface is the overflow menu.
//!
//! Divergences from upstream, each also marked at its site:
//!
//! * The icon buttons' tooltips (`openAppDrawerTooltip`, "Favorite",
//!   "Search") are not shown: a tooltip bubble is a `Stack` layer positioned
//!   at the button, and the button's position is layout information a build
//!   does not have.
//! * The overflow menu is anchored to the top trailing corner of the demo
//!   rather than to the button's rectangle: [`popup_menu_offset`] needs the
//!   anchor's `Rect`, the same layout information.
//! * The icon buttons are drawn but not wired. Upstream's callbacks are
//!   empty, so all an unwired tap cannot do is splash.

use rustflutter::components::K_TOOLBAR_HEIGHT;
use rustflutter::framework::{BuildContext, StatefulComponent};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxConstraints, CrossAxisAlignment, MainAxisSize, RenderConstrainedBox, RenderFlex,
    StackPosition,
};
use rustflutter::widgets::{
    Align, Center, Container, Pointer, RenderNavigationToolbar, Text, K_MIDDLE_SPACING,
};

use crate::app::{ids, GalleryState};
use crate::data::demos::{self, icon};
use crate::themes::material_demo_theme_data::MaterialDemoThemeData;

use super::DemoState;

/// The demo body for the `app-bar` slug. The aggregate `DemoState` has
/// nothing this demo remembers -- upstream's `AppBarDemo` is stateless apart
/// from the popup route -- so the demo's one bit of state (the menu being
/// open) lives in [`AppBarDemoState`].
pub(super) fn app_bar(
    _state: &DemoState,
    _pressed: Option<u64>,
    _handle: StateHandle<GalleryState>,
) -> AnyWidget {
    stateful(AppBarDemo)
}

/// Upstream's `AppBarDemo`.
struct AppBarDemo;

/// What the demo remembers. Upstream this is the `_PopupMenuRoute` the
/// button pushes; here a menu over the page is the application's state (see
/// `menu.rs`'s module docs).
#[derive(Clone, Copy, Debug, Default)]
struct AppBarDemoState {
    menu_open: bool,
}

/// The overflow menu's entries, upstream's three value-less
/// `PopupMenuItem<Text>`s in `AppBarDemo.build`
/// (`demoNavigationRailFirst`/`Second`/`Third`).
const MENU_LABELS: [&str; 3] = ["First", "Second", "Third"];

/// A 48-by-48 icon button, upstream's `IconButton`: the glyph in Material's
/// icon font centred in the minimum interactive target. Drawn but not wired
/// -- see the module header.
fn icon_button(glyph: &str, color: Color) -> AnyWidget {
    let glyph = glyph.to_string();
    leaf(move || {
        Container::new()
            .with_size(48.0, 48.0)
            .with_child(Align::new(
                Alignment::CENTER,
                Text::new(glyph.clone())
                    .with_font_family(demos::MATERIAL_ICONS)
                    .with_size(24.0)
                    .with_color(color),
            ))
    })
}

/// A tap-catcher behind the open menu. Upstream's popup route barrier
/// dismisses without dimming (`barrierColor` is null on `_PopupMenuRoute`),
/// so this is transparent rather than a [`Scrim`].
fn menu_barrier(handle: StateHandle<AppBarDemoState>) -> AnyWidget {
    let handlers = PointerHandlers::new().with_tap(move |_| {
        handle.set_state(|state| state.menu_open = false);
    });
    leaf(move || {
        Pointer::new(
            ids::DEMO_LOCAL + 4,
            Container::new().with_color(Color::TRANSPARENT),
        )
        .with_handlers(handlers.clone())
    })
}

impl StatefulComponent for AppBarDemo {
    type State = AppBarDemoState;

    fn build(
        &self,
        state: &AppBarDemoState,
        handle: StateHandle<AppBarDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        // Upstream's demo `appBarTheme`: primary fill, on-primary icons and
        // title (`MaterialDemoThemeData.appBarTheme`).
        let (bar_fill, bar_ink) = MaterialDemoThemeData::app_bar_theme();
        let background = theme.background;

        // The bar, upstream's `AppBar`: a leading menu button, the title
        // (`demoAppBarTitle`), then the actions -- favorite and search icon
        // buttons and the overflow menu button.
        let leading = icon_button(icon::MENU, bar_ink);
        let title = leaf(move || {
            Text::new("App bar")
                .with_size(20.0)
                .with_weight(500)
                .with_color(bar_ink)
        });
        let overflow = component(
            PopupMenuButton::new(ids::DEMO_LOCAL)
                .wired(handle.clone(), |state| state.menu_open = true),
        );
        let actions = many(
            vec![
                icon_button(icon::FAVORITE, bar_ink),
                icon_button(icon::SEARCH, bar_ink),
                overflow,
            ],
            move |rendered| {
                // `MainAxisSize.min` for the reason the framework's `AppBar`
                // gives: `_ToolbarLayout` hands the trailing a bounded width
                // and only a content-sized row leaves the title its room.
                let mut row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center);
                for action in rendered {
                    row = row.push(action);
                }
                Box::new(row)
            },
        );
        let bar = many(vec![leading, title, actions], move |mut rendered| {
            let trailing = rendered.pop().expect("the actions");
            let middle = rendered.pop().expect("the title");
            let leading = rendered.pop().expect("the leading button");
            let toolbar = RenderNavigationToolbar::new()
                // `_getEffectiveCenterTitle`: false everywhere but iOS/macOS.
                .with_center_middle(false)
                .with_middle_spacing(K_MIDDLE_SPACING)
                .with_leading(leading)
                .with_middle(middle)
                .with_trailing(trailing);
            Box::new(
                Container::new().with_color(bar_fill).with_child(
                    // `_ToolbarContainerLayout`: the bar is exactly
                    // `kToolbarHeight` tall.
                    RenderConstrainedBox::new(BoxConstraints::new(
                        0.0,
                        f32::INFINITY,
                        K_TOOLBAR_HEIGHT,
                        K_TOOLBAR_HEIGHT,
                    ))
                    .with_child(toolbar),
                ),
            )
        });

        // The body, upstream's `Center(child: Text(cupertinoTabBarHomeTab))`.
        // The height is the stage's rather than a screen's: the demo renders
        // in a card on the demo page, not on a display of its own.
        let body_style = theme.body();
        let body = leaf(move || {
            Container::new().with_height(200.0).with_child(Center::new(
                Text::new("Home").with_style(body_style.clone()),
            ))
        });

        let page = many(vec![bar, body], move |mut rendered| {
            let body = rendered.pop().expect("the body");
            let bar = rendered.pop().expect("the bar");
            Box::new(
                Container::new().with_color(background).with_child(
                    RenderFlex::column()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push(bar)
                        .push(body),
                ),
            )
        });

        if !state.menu_open {
            return page;
        }

        // The open menu: the barrier, then the menu itself under the bar's
        // trailing corner. See the module header for why the anchor is the
        // corner and not the button's rectangle.
        let mut menu = PopupMenu::new();
        for (index, label) in MENU_LABELS.iter().enumerate() {
            menu = menu.push(
                PopupMenuItem::new(ids::DEMO_LOCAL + 1 + index as u64, *label, *label)
                    // The entries carry no value upstream, so choosing one
                    // only pops the route -- here, closes the menu.
                    .wired(handle.clone(), |state, _label| state.menu_open = false),
            );
        }
        many(
            vec![page, menu_barrier(handle), component(menu)],
            |mut rendered| {
                let menu = rendered.pop().expect("the menu");
                let barrier = rendered.pop().expect("the barrier");
                let page = rendered.pop().expect("the page");
                Box::new(
                    rustflutter::widgets::Stack::new()
                        .push(page)
                        .push_positioned(barrier, StackPosition::fill())
                        .push_positioned(
                            menu,
                            StackPosition {
                                top: Some(K_TOOLBAR_HEIGHT),
                                right: Some(8.0),
                                ..Default::default()
                            },
                        ),
                )
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_starts_closed() {
        assert!(!AppBarDemoState::default().menu_open);
    }

    #[test]
    fn the_overflow_menu_is_upstreams_three_entries() {
        assert_eq!(MENU_LABELS, ["First", "Second", "Third"]);
    }
}
