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
//! press on the logo grows it, then opens the menu full-screen over the
//! blurred page with the two `CupertinoContextMenuAction`s
//! (`demoCupertinoContextMenuActionOne`/Two) beside it.
//!
//! So is this one, and it is stateless as upstream's is: opening, hiding the
//! child, placing the preview and the sheet, and closing again all belong to
//! [`rustflutter::CupertinoContextMenu`] now. This file used to own an `open`
//! flag, a scrim and a second copy of the logo stacked under the sheet,
//! because the framework's context menu was only the long-press trigger. It
//! is not any more; what is left here is upstream's widget tree.
//!
//! Divergences, each also marked at its site:
//!
//! - **the scaffold is a fixed height.** Upstream's `DemoWrapper` gives the
//!   demo the page's content height; the demo page here renders each stage in
//!   a scrolling column at its intrinsic height, so the scaffold gets
//!   [`DEMO_HEIGHT`] to stand in for the screen's remainder.
//! - **the actions close the menu through the widget's controller**, not
//!   through `Navigator.pop`: the menu is an overlay portal rather than a
//!   route (see [`rustflutter::CupertinoContextMenu`], which also records the
//!   drag-to-dismiss it does not have).

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Center, Empty, SizedBox};
use rustflutter::{Decoration, FlutterLogoDecoration, FlutterLogoStyle};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// The height the scaffold stands in for; see the module header.
const DEMO_HEIGHT: f32 = 700.0;

/// The box the trigger sits in: upstream's `SizedBox(width: 100, height: 100)`.
const TRIGGER_SIZE: f32 = 100.0;

/// Upstream's `FlutterLogo(size: 250)`. The logo is bigger than the box it is
/// closed in on purpose: the box tightens it to 100 while the menu is shut,
/// and the open preview -- which lays its child out unconstrained inside a
/// `FittedBox` -- gets all 250 of it.
const LOGO_SIZE: f32 = 250.0;

/// The demo body for the `cupertino-context-menu` slug.
pub(super) fn stage() -> AnyWidget {
    component(ContextMenuDemo)
}

/// Upstream's `CupertinoContextMenuDemo`.
struct ContextMenuDemo;

/// Upstream's `FlutterLogo(size: 250)`: the mark on its own, which is what
/// `FlutterLogoStyle.markOnly` -- the widget's default -- draws.
///
/// The wordmark asset `pages/splash.rs` carries used to stand in for this,
/// because the logo widget had no counterpart here. It has one:
/// [`FlutterLogoDecoration`] draws the mark from the artwork's own
/// coordinates, so the substitution is gone and the demo shows what upstream
/// shows.
fn logo() -> AnyWidget {
    leaf(|| {
        Container::new()
            .with_size(LOGO_SIZE, LOGO_SIZE)
            .with_decoration(Decoration::FlutterLogo(FlutterLogoDecoration::new(
                FlutterLogoStyle::MarkOnly,
            )))
    })
}

impl Component for ContextMenuDemo {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let l10n = GalleryLocalizations::en();

        // Upstream's `CupertinoContextMenu(actions: [...], child:
        // FlutterLogo(size: 250))`. The controller is taken before the actions
        // are wired to it, the way upstream's actions close over the
        // `BuildContext` they will pop.
        let menu = CupertinoContextMenu::new(ids::DEMO_LOCAL).with_child(logo);
        let dismiss = menu.controller();
        let menu = menu
            .push_action(
                CupertinoContextMenuAction::new(
                    ids::DEMO_LOCAL + 1,
                    l10n.demo_cupertino_context_menu_action_one(),
                )
                .on_pressed({
                    let dismiss = dismiss.clone();
                    move || dismiss.close()
                }),
            )
            .push_action(
                CupertinoContextMenuAction::new(
                    ids::DEMO_LOCAL + 2,
                    l10n.demo_cupertino_context_menu_action_two(),
                )
                .on_pressed({
                    let dismiss = dismiss.clone();
                    move || dismiss.close()
                }),
            );

        // Upstream's `Center(SizedBox(100, 100, CupertinoContextMenu(...)))`.
        let trigger = single(stateful(menu), |menu| {
            Box::new(Center::new(
                SizedBox::new(TRIGGER_SIZE, TRIGGER_SIZE).with_child(menu),
            ))
        });

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

        // Upstream's `DemoWrapper` wraps every demo in a light
        // `CupertinoTheme` (`lib/pages/demo.dart`).
        provide(CupertinoTheme::light(), stage)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::cupertino::{
        context_menu_animation_opens_at, context_menu_location, context_menu_scale_factor,
        ContextMenuLocation, CONTEXT_MENU_OPEN_SCALE, CONTEXT_MENU_PADDING,
    };
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, EdgeInsets as Insets, RenderBox, Size};

    fn lay_out(widget: AnyWidget, width: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(CupertinoTheme::light(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::new(0.0, width, 0.0, f32::INFINITY))
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
    fn the_stage_is_a_scaffold_at_the_stand_in_height() {
        let size = lay_out(stage(), 428.0);
        assert_eq!(size.height, DEMO_HEIGHT);
        assert_eq!(size.width, 428.0);
    }

    /// The demo's trigger sits in the middle of the demo card, which on the
    /// desktop layout is the right-hand half of the window -- so the sheet
    /// goes to the *left* of the preview, which is what upstream shows.
    #[test]
    fn a_trigger_in_the_right_hand_half_puts_the_sheet_first() {
        let child = rustflutter::Rect::xywh(1000.0, 400.0, TRIGGER_SIZE, TRIGGER_SIZE);
        assert_eq!(
            context_menu_location(child, 1536.0),
            ContextMenuLocation::Right
        );
    }

    /// A trigger centred in the window is centred when it opens.
    #[test]
    fn a_trigger_in_the_middle_stays_in_the_middle() {
        let child = rustflutter::Rect::xywh(718.0, 400.0, TRIGGER_SIZE, TRIGGER_SIZE);
        assert_eq!(
            context_menu_location(child, 1536.0),
            ContextMenuLocation::Center
        );
    }

    /// Room to spare means the press grows the child by the full `_kOpenScale`.
    #[test]
    fn a_press_in_open_space_grows_by_the_full_scale() {
        let child = rustflutter::Rect::xywh(718.0, 400.0, TRIGGER_SIZE, TRIGGER_SIZE);
        let scale = context_menu_scale_factor(child, Insets::ZERO, Size::new(1536.0, 826.0));
        assert!((scale - CONTEXT_MENU_OPEN_SCALE).abs() < 1e-6, "{scale}");
    }

    /// The press is most of the combined animation; the menu opens in the
    /// remainder. Upstream's `animationOpensAt` is 800/1135.
    #[test]
    fn the_menu_opens_in_the_last_third_of_the_animation() {
        let opens_at = context_menu_animation_opens_at();
        assert!((opens_at - 800.0 / 1135.0).abs() < 1e-6, "{opens_at}");
    }

    /// The gap the open menu keeps, upstream's `_kPadding`.
    #[test]
    fn the_open_menu_keeps_upstreams_padding() {
        assert_eq!(CONTEXT_MENU_PADDING, 20.0);
    }
}
