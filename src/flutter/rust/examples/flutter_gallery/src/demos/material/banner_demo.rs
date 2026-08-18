// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/banner_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream the demo is a `Scaffold` with an app bar whose overflow menu
//! resets the banner or toggles its second action and leading icon
//! (`BannerDemoAction`), over a `ListView.builder` of twenty tiles whose
//! first row is the `MaterialBanner` while it is displayed. The state is
//! upstream's `_BannerDemoState` (`_displayBanner`, `_showMultipleActions`,
//! `_showLeading`), kept here in [`BannerDemoState`] rather than in the
//! shared `DemoState` -- the restoration machinery around it
//! (`RestorationMixin`, the `RestorableBool`s) has no counterpart and is not
//! carried.
//!
//! Divergences from upstream, each also marked at its site:
//!
//! * The overflow menu is anchored to the top trailing corner of the demo
//!   rather than to the button's rectangle: [`popup_menu_offset`] needs the
//!   anchor's `Rect`, which is layout information a build does not have.
//! * The list's viewport is a fixed height rather than the screen's
//!   remainder: the demo renders in a card on the demo page, not on a
//!   display of its own.

use rustflutter::components::K_TOOLBAR_HEIGHT;
use rustflutter::framework::{BuildContext, StatefulComponent};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxConstraints, CrossAxisAlignment, EdgeInsets, MainAxisAlignment, MainAxisSize,
    RenderConstrainedBox, RenderFlex, StackPosition,
};
use rustflutter::widgets::{
    Align, BoxedWidget, Container, ListView, Pointer, RenderNavigationToolbar, Text,
    K_MIDDLE_SPACING,
};

use crate::app::{ids, GalleryState};
use crate::data::demos;
use crate::themes::material_demo_theme_data::MaterialDemoThemeData;

use super::DemoState;

/// The demo body for the `banner` slug.
pub(super) fn banner(_state: &DemoState, _handle: StateHandle<GalleryState>) -> AnyWidget {
    stateful(BannerDemo)
}

/// Upstream's `BannerDemo`.
struct BannerDemo;

/// Upstream's `BannerDemoAction`: the overflow menu's entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BannerDemoAction {
    Reset,
    ShowMultipleActions,
    ShowLeading,
}

/// Upstream's `_BannerDemoState`, minus the restoration wrappers.
///
/// `Scroll` is not `Debug`, so the derive skips it.
#[derive(Clone)]
struct BannerDemoState {
    display_banner: bool,
    show_multiple_actions: bool,
    show_leading: bool,
    /// Whether the overflow menu is showing. Upstream the menu is a route;
    /// here it is the application's state (see `menu.rs`'s module docs).
    menu_open: bool,
    /// The list's scroll position, upstream's `ScrollableState`. Not Debug.
    scroll: Scroll,
}

impl std::fmt::Debug for BannerDemoState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BannerDemoState")
            .field("display_banner", &self.display_banner)
            .field("show_multiple_actions", &self.show_multiple_actions)
            .field("show_leading", &self.show_leading)
            .field("menu_open", &self.menu_open)
            .finish_non_exhaustive()
    }
}

impl Default for BannerDemoState {
    fn default() -> BannerDemoState {
        BannerDemoState {
            display_banner: true,
            show_multiple_actions: true,
            show_leading: true,
            menu_open: false,
            scroll: Scroll::default(),
        }
    }
}

impl BannerDemoState {
    /// Upstream's `_BannerDemoState.handleDemoAction`, plus the menu closing:
    /// upstream a tap on a `PopupMenuItem` pops the route the menu lives on.
    fn handle_demo_action(&mut self, action: BannerDemoAction) {
        match action {
            BannerDemoAction::Reset => {
                self.display_banner = true;
                self.show_multiple_actions = true;
                self.show_leading = true;
            }
            BannerDemoAction::ShowMultipleActions => {
                self.show_multiple_actions = !self.show_multiple_actions;
            }
            BannerDemoAction::ShowLeading => {
                self.show_leading = !self.show_leading;
            }
        }
        self.menu_open = false;
    }
}

/// Upstream's `_itemCount`.
const ITEM_COUNT: usize = 20;

/// The fixed height the list scrolls in. See the module header.
const LIST_VIEWPORT: f32 = 420.0;

/// `Icons.access_alarm` in the icon font the gallery ships (the codepoint is
/// the font's, resolved against `assets/fonts/MaterialIcons-Regular.otf`;
/// `data/demos.rs`'s `icon` module does not carry this one).
const ACCESS_ALARM: &str = "\u{e038}";

/// The banner's content text, upstream's `bannerDemoText`.
const BANNER_TEXT: &str = "Your password was updated on your other device. Please sign in again.";

/// A tile in the list, upstream's `ListTile(title:
/// Text(starterAppDrawerItem(index)))`: the tiles are numbered 1 through 20
/// whether or not the banner takes the first slot.
fn list_tile(number: usize) -> AnyWidget {
    let label = format!("Item {number}");
    leaf(move || {
        Container::new()
            .with_height(48.0)
            .with_padding(EdgeInsets::symmetric(16.0, 0.0))
            .with_child(Align::new(Alignment::CENTER_LEFT, Text::new(label.clone())))
    })
}

/// The `MaterialBanner`: content with an optional leading circle avatar, and
/// one or two text buttons, both of which dismiss it. With one action the
/// button sits beside the content; with two the actions go below it --
/// `MaterialBanner`'s rule (`forceActionsBelow` defaults to placing multiple
/// actions under the content).
fn material_banner(
    show_leading: bool,
    show_multiple_actions: bool,
    handle: StateHandle<BannerDemoState>,
    context: &mut BuildContext,
) -> AnyWidget {
    let theme = theme_of(context);
    let primary = theme.primary;
    let on_primary = theme.on_primary;
    let background = theme.background;
    let outline = theme.outline;
    let body = theme.body();

    let content = leaf(move || {
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(16.0);
        if show_leading {
            // Upstream's `CircleAvatar`: primary fill, the alarm glyph in
            // on-primary.
            row = row.push(
                Container::new()
                    .with_size(40.0, 40.0)
                    .with_color(primary)
                    .with_corner_radius(20.0)
                    .with_child(Align::new(
                        Alignment::CENTER,
                        Text::new(ACCESS_ALARM)
                            .with_font_family(demos::MATERIAL_ICONS)
                            .with_size(24.0)
                            .with_color(on_primary),
                    )),
            );
        }
        row.push_flex(rustflutter::render::FlexChild::expanded(
            Align::new(
                Alignment::CENTER_LEFT,
                Text::new(BANNER_TEXT).with_style(body.clone()),
            ),
            1,
        ))
    });

    // A tap dismisses the banner, upstream's `setState(_displayBanner.value
    // = false)` on both buttons. `Button::wired` is not used: it wants a
    // pressed-id field for splash feedback, which this state does not carry.
    let sign_in_handle = handle.clone();
    let sign_in = component(
        Button::new(ids::DEMO_LOCAL + 5, "SIGN IN")
            .with_style(ButtonStyle::Text)
            .with_handlers(PointerHandlers::new().with_tap(move |_| {
                sign_in_handle.set_state(|state| state.display_banner = false);
            })),
    );

    let mut children = vec![content, sign_in];
    if show_multiple_actions {
        children.push(component(
            Button::new(ids::DEMO_LOCAL + 6, "DISMISS")
                .with_style(ButtonStyle::Text)
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    handle.set_state(|state| state.display_banner = false);
                })),
        ));
    }
    let multiple = show_multiple_actions;

    many(children, move |mut rendered| {
        let content = rendered.remove(0);
        let inner: BoxedWidget = if multiple {
            let mut actions_row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::End)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.0);
            for action in rendered {
                actions_row = actions_row.push(action);
            }
            rustflutter::widgets::boxed(
                Container::new()
                    .with_padding(EdgeInsets::only(16.0, 24.0, 8.0, 8.0))
                    .with_child(
                        RenderFlex::column()
                            .with_main_axis_size(MainAxisSize::Min)
                            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                            .with_spacing(8.0)
                            .push(content)
                            .push(actions_row),
                    ),
            )
        } else {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.0)
                .push_flex(rustflutter::render::FlexChild::expanded(content, 1));
            for action in rendered {
                row = row.push(action);
            }
            rustflutter::widgets::boxed(
                Container::new()
                    .with_padding(EdgeInsets::only(16.0, 24.0, 16.0, 24.0))
                    .with_child(row),
            )
        };
        // The divider under the banner is part of upstream's `MaterialBanner`.
        Box::new(
            Container::new().with_color(background).with_child(
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .push(inner)
                    .push(Container::new().with_height(1.0).with_color(outline)),
            ),
        )
    })
}

/// The list's drag wiring, the per-demo counterpart of
/// `app.rs`'s `scroll_handlers` (which is bound to `GalleryState`).
fn scroll_handlers(handle: StateHandle<BannerDemoState>) -> PointerHandlers {
    let down_handle = handle.clone();
    let drag_handle = handle.clone();
    let end_handle = handle;
    PointerHandlers::new()
        .with_pointer_down(move |_| {
            down_handle.set_state(|state| state.scroll.stop());
        })
        .with_drag_update(move |drag| {
            drag_handle.set_state(move |state| state.scroll.scroll_by(-drag.delta.dy));
        })
        .with_drag_end(move |end| {
            end_handle.set_state(move |state| state.scroll.fling(-end.velocity.dy));
        })
}

impl StatefulComponent for BannerDemo {
    type State = BannerDemoState;

    fn advance(&self, state: &mut BannerDemoState, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &BannerDemoState,
        handle: StateHandle<BannerDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let (bar_fill, bar_ink) = MaterialDemoThemeData::app_bar_theme();
        let background = theme.background;

        // The app bar, upstream's `AppBar(automaticallyImplyLeading: false,
        // title: demoBannerTitle, actions: [PopupMenuButton])`.
        let title = leaf(move || {
            Text::new("Banner")
                .with_size(20.0)
                .with_weight(500)
                .with_color(bar_ink)
        });
        let overflow = component(
            PopupMenuButton::new(ids::DEMO_LOCAL)
                .wired(handle.clone(), |state| state.menu_open = true),
        );
        let bar = many(vec![title, overflow], move |mut rendered| {
            let trailing = rendered.pop().expect("the overflow button");
            let middle = rendered.pop().expect("the title");
            let toolbar = RenderNavigationToolbar::new()
                .with_center_middle(false)
                .with_middle_spacing(K_MIDDLE_SPACING)
                .with_middle(middle)
                .with_trailing(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Min)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .push(trailing),
                );
            Box::new(
                Container::new().with_color(bar_fill).with_child(
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

        // The body, upstream's `ListView.builder`: the banner first while it
        // is displayed, then twenty tiles.
        let mut rows: Vec<AnyWidget> = Vec::new();
        if state.display_banner {
            rows.push(material_banner(
                state.show_leading,
                state.show_multiple_actions,
                handle.clone(),
                context,
            ));
        }
        for number in 1..=ITEM_COUNT {
            rows.push(list_tile(number));
        }
        let offset = state.scroll.offset;
        let extent = state.scroll.extent.clone();
        let handlers = scroll_handlers(handle.clone());
        let body = many(rows, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for row in rendered {
                column = column.push(row);
            }
            Box::new(
                Pointer::new(
                    ids::DEMO_LOCAL + 7,
                    Container::new().with_height(LIST_VIEWPORT).with_child(
                        ListView::new()
                            .with_offset(offset)
                            .with_extent_sink(extent.clone())
                            .push(column),
                    ),
                )
                .with_handlers(handlers.clone()),
            )
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

        // The open menu, upstream's `PopupMenuButton<BannerDemoAction>`:
        // reset, a divider, then the two checked entries. The barrier is
        // transparent -- a popup route does not dim what is under it.
        let close_handle = handle.clone();
        let menu = component(
            PopupMenu::new()
                .push(
                    PopupMenuItem::new(
                        ids::DEMO_LOCAL + 1,
                        "Reset the banner",
                        BannerDemoAction::Reset,
                    )
                    .wired(handle.clone(), |state, action| {
                        state.handle_demo_action(action)
                    }),
                )
                .push(PopupMenuDivider::new())
                .push(
                    CheckedPopupMenuItem::new(
                        ids::DEMO_LOCAL + 2,
                        "Multiple actions",
                        BannerDemoAction::ShowMultipleActions,
                        state.show_multiple_actions,
                    )
                    .wired(handle.clone(), |state, action| {
                        state.handle_demo_action(action)
                    }),
                )
                .push(
                    CheckedPopupMenuItem::new(
                        ids::DEMO_LOCAL + 3,
                        "Leading Icon",
                        BannerDemoAction::ShowLeading,
                        state.show_leading,
                    )
                    .wired(handle, |state, action| state.handle_demo_action(action)),
                ),
        );
        many(
            vec![page, menu_barrier(close_handle), menu],
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

/// A tap-catcher behind the open menu; transparent because a popup route's
/// barrier does not dim. See app_bar_demo.rs for the same shape.
fn menu_barrier(handle: StateHandle<BannerDemoState>) -> AnyWidget {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_state_is_upstreams_initial_state() {
        let state = BannerDemoState::default();
        // Upstream's `RestorableBool(true)` three times over.
        assert!(state.display_banner);
        assert!(state.show_multiple_actions);
        assert!(state.show_leading);
        assert!(!state.menu_open);
    }

    #[test]
    fn reset_restores_everything_and_closes_the_menu() {
        let mut state = BannerDemoState {
            display_banner: false,
            show_multiple_actions: false,
            show_leading: false,
            menu_open: true,
            scroll: Scroll::default(),
        };
        state.handle_demo_action(BannerDemoAction::Reset);
        assert!(state.display_banner);
        assert!(state.show_multiple_actions);
        assert!(state.show_leading);
        assert!(!state.menu_open, "choosing an entry pops the menu's route");
    }

    #[test]
    fn the_toggles_flip_and_leave_the_banner_alone() {
        let mut state = BannerDemoState::default();
        state.handle_demo_action(BannerDemoAction::ShowMultipleActions);
        assert!(!state.show_multiple_actions);
        assert!(state.display_banner);
        state.handle_demo_action(BannerDemoAction::ShowLeading);
        assert!(!state.show_leading);
        state.handle_demo_action(BannerDemoAction::ShowMultipleActions);
        assert!(state.show_multiple_actions);
    }

    #[test]
    fn the_list_is_twenty_tiles() {
        assert_eq!(ITEM_COUNT, 20);
    }
}
