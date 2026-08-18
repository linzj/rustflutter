// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_context_menu_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoContextMenuDemo` is a `CupertinoPageScaffold` whose
//! centred column holds a 100x100 box containing a `CupertinoContextMenu`
//! around a `FlutterLogo(size: 250)`, then the hint text
//! (`demoCupertinoContextMenuActionText`, centred, padded 30, black). A long
//! press on the logo opens the menu: upstream zooms the child out of the
//! layout and puts a `_ContextMenuSheet` of two `CupertinoContextMenuAction`s
//! (`demoCupertinoContextMenuActionOne`/Two) beside it over a barrier.
//!
//! The framework's cupertino tier splits that widget the way its
//! [`CupertinoContextMenu`] documents: the widget is the long-press trigger,
//! and putting the [`CupertinoContextMenuSheet`] over a scrim in a `Stack` is
//! the app's. The stack here is the stage's own, for the same reason the
//! alerts demo keeps its dialogs in its own stack: `mod.rs`'s shared
//! `overlay()` reads only `GalleryState`, and this demo's open flag is its
//! own state (upstream's is the route's).
//!
//! Divergences, each also marked at its site:
//!
//! - **the child is the logo asset, not a drawn `FlutterLogo`.** The widget
//!   has no counterpart here; `pages/splash.rs`'s colour logo at 100x100
//!   stands in, the substitution `material/navigation_drawer.rs` already
//!   makes.
//! - **no zoom, blur or drag-to-dismiss.** Upstream's open animation lifts
//!   the child out of the layout and rounds its corners; the menu here
//!   appears with the frame the long press schedules, child rounded to
//!   [`K_OPEN_BORDER_RADIUS`], sheet centred beneath it.
//! - **the scaffold is a fixed height.** Upstream's `DemoWrapper` gives the
//!   demo the page's content height; the demo page here renders each stage in
//!   a scrolling column at its intrinsic height, so the scaffold gets
//!   [`DEMO_HEIGHT`] to stand in for the screen's remainder.

use rustflutter::cupertino::{CONTEXT_MENU_BARRIER_COLOR, K_OPEN_BORDER_RADIUS};
use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{BoxedRender, CrossAxisAlignment, MainAxisSize, RenderFlex, RenderRef};
use rustflutter::widgets::{Center, ClipRRect, Empty, Pointer, Positioned, SizedBox, Stack};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::pages::splash;

/// The height the scaffold stands in for; see the module header.
const DEMO_HEIGHT: f32 = 700.0;

/// The logo's size: upstream's `SizedBox(width: 100, height: 100)` (its
/// `FlutterLogo(size: 250)` is clipped to it; the asset needs no help).
const LOGO_SIZE: f32 = 100.0;

/// The demo body for the `cupertino-context-menu` slug.
pub(super) fn stage() -> AnyWidget {
    stateful(ContextMenuDemo)
}

/// Upstream's `CupertinoContextMenuDemo`. Stateless upstream; the open flag
/// lives here because the framework's context menu is a trigger, not a route
/// (see the module header).
struct ContextMenuDemo;

/// Whether the menu is open. Upstream this is whether its route is up.
#[derive(Default)]
struct ContextMenuDemoState {
    open: bool,
}

/// Closes the menu: the `Navigator.pop(context)` both of upstream's actions
/// run, and the barrier's dismiss.
fn close(state: &mut ContextMenuDemoState) {
    state.open = false;
}

/// The logo, at `size`. Upstream's `FlutterLogo(size: 250)`; the colour logo
/// asset stands in for it (see the module header).
fn logo(size: f32) -> AnyWidget {
    let image = Image::shared("flutter_logo_color", splash::FLUTTER_LOGO_COLOR);
    leaf(move || {
        let view: BoxedRender = match image.clone() {
            Some(image) => RenderRef::new(rustflutter::widgets::ImageView::with_fit(
                image,
                rustflutter::render::BoxFit::Contain,
            )),
            // Not yet decoded: the box still takes the layout space, and the
            // next frame draws the logo (a headless render waits for it).
            None => RenderRef::new(Empty),
        };
        SizedBox::new(size, size).with_child(view)
    })
}

impl StatefulComponent for ContextMenuDemo {
    type State = ContextMenuDemoState;

    fn build(
        &self,
        state: &ContextMenuDemoState,
        handle: StateHandle<ContextMenuDemoState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let l10n = GalleryLocalizations::en();

        // Upstream's `Center(SizedBox(100, 100, CupertinoContextMenu(...)))`:
        // the trigger carries the logo, and a long press opens the menu.
        let trigger = component(
            CupertinoContextMenu::new(ids::DEMO_LOCAL)
                .with_child(logo(LOGO_SIZE))
                .wired(handle.clone(), |state, open| state.open = open),
        );

        // The hint text: centred, padded 30, black as upstream's explicit
        // `TextStyle(color: Colors.black)` -- the demo always renders light
        // (upstream's `DemoWrapper`), so black reads as it does upstream.
        let hint = {
            let text = l10n.demo_cupertino_context_menu_action_text();
            leaf(move || {
                Container::new()
                    .with_padding(EdgeInsets::all(30.0))
                    .with_child(
                        Text::new(text)
                            .with_color(CupertinoColors::BLACK)
                            .with_align(TextAlign::Center),
                    )
            })
        };

        let body = many(vec![trigger, hint], move |mut rendered| {
            let hint = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let trigger = rendered.pop().unwrap_or_else(|| boxed(Empty));
            // Upstream's `Column(mainAxisAlignment: MainAxisAlignment.center)`
            // with the `SizedBox(height: 20)` between the two.
            Box::new(Center::new(
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .push(trigger)
                    .push(Container::new().with_height(20.0))
                    .push(hint),
            ))
        });

        let scaffold = component(
            CupertinoPageScaffold::new(body).with_navigation_bar(component(
                CupertinoNavigationBar::new().with_middle(l10n.demo_cupertino_context_menu_title()),
            )),
        );

        // The fixed height stands in for the content height; see the module
        // header.
        let stage = single(scaffold, |scaffold| {
            Box::new(
                Container::new()
                    .with_height(DEMO_HEIGHT)
                    .with_child(scaffold),
            )
        });

        // The open menu over the stage: the barrier, then the child and the
        // sheet beside it, centred. See the module header for why this is not
        // `mod.rs`'s `overlay()`, and for the zoom/blur that are not ported.
        let content = if state.open {
            let barrier_handle = handle.clone();
            let barrier = leaf(move || {
                // context_menu.dart's `_kModalBarrierColor`.
                Pointer::new(
                    ids::SCRIM,
                    Container::new().with_color(CONTEXT_MENU_BARRIER_COLOR),
                )
                .with_handlers(
                    rustflutter::gestures::PointerHandlers::new().with_tap({
                        let handle = barrier_handle.clone();
                        move |_| {
                            handle.set_state(close);
                        }
                    }),
                )
            });
            let sheet = component(
                CupertinoContextMenuSheet::new()
                    .push(
                        CupertinoContextMenuAction::new(
                            ids::DEMO_LOCAL + 1,
                            l10n.demo_cupertino_context_menu_action_one(),
                        )
                        .wired(handle.clone(), close),
                    )
                    .push(
                        CupertinoContextMenuAction::new(
                            ids::DEMO_LOCAL + 2,
                            l10n.demo_cupertino_context_menu_action_two(),
                        )
                        .wired(handle.clone(), close),
                    ),
            );
            // The child floats over the sheet, corners rounded to
            // `kOpenBorderRadius` as while open upstream; the zoom is the
            // part that is not ported.
            let lifted = logo(LOGO_SIZE);
            many(vec![stage, barrier, sheet, lifted], move |mut rendered| {
                let logo = rendered.pop().unwrap_or_else(|| boxed(Empty));
                let sheet = rendered.pop().unwrap_or_else(|| boxed(Empty));
                let barrier = rendered.pop().unwrap_or_else(|| boxed(Empty));
                let stage = rendered.pop().unwrap_or_else(|| boxed(Empty));
                let lifted = ClipRRect::new(K_OPEN_BORDER_RADIUS, logo);
                Box::new(
                    Stack::new()
                        .push(stage)
                        .push_positioned(barrier, Positioned::fill())
                        .push(Center::new(
                            RenderFlex::column()
                                .with_main_axis_size(MainAxisSize::Min)
                                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                .push(Container::new().with_child(lifted))
                                .push(Container::new().with_height(20.0))
                                .push(sheet),
                        )),
                )
            })
        } else {
            stage
        };

        // Upstream's `DemoWrapper` wraps every demo in a light
        // `CupertinoTheme` (`lib/pages/demo.dart`).
        provide(CupertinoTheme::light(), content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox, Size};

    fn lay_out(widget: AnyWidget, width: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(CupertinoTheme::light(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::new(0.0, width, 0.0, f32::INFINITY))
    }

    #[test]
    fn the_menu_starts_closed() {
        assert!(!ContextMenuDemoState::default().open);
    }

    #[test]
    fn both_actions_close_the_menu() {
        // Upstream's `onPressed: () { Navigator.pop(context); }` on both.
        let mut state = ContextMenuDemoState { open: true };
        close(&mut state);
        assert!(!state.open);
    }

    #[test]
    fn the_hint_and_actions_are_upstreams_strings() {
        let l10n = GalleryLocalizations::en();
        assert_eq!(l10n.demo_cupertino_context_menu_action_one(), "Action one");
        assert_eq!(l10n.demo_cupertino_context_menu_action_two(), "Action two");
        assert_eq!(
            l10n.demo_cupertino_context_menu_action_text(),
            "Tap and hold the Flutter logo to see the context menu."
        );
    }

    #[test]
    fn the_open_sheet_lays_out_under_the_logo() {
        let sheet = component(
            CupertinoContextMenuSheet::new()
                .push(CupertinoContextMenuAction::new(1, "Action one"))
                .push(CupertinoContextMenuAction::new(2, "Action two")),
        );
        let size = lay_out(sheet, 428.0);
        assert_eq!(size.width, rustflutter::cupertino::CONTEXT_MENU_SHEET_WIDTH);
    }

    #[test]
    fn the_stage_is_a_scaffold_at_the_stand_in_height() {
        let size = lay_out(stage(), 428.0);
        assert_eq!(size.height, DEMO_HEIGHT);
        assert_eq!(size.width, 428.0);
    }
}
