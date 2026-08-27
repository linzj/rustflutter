// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/tooltip_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `TooltipDemo` is a `Scaffold` with an app bar (the
//! `demoTooltipTitle`, no leading) whose body is a `Center` around a column
//! of the instructions and a search `IconButton` wrapped in a `Tooltip`. So
//! is this one: the scaffold is the framework's (`components.rs`), the way
//! `navigation_drawer.rs`'s demo is, and [`rustflutter::Tooltip`] is
//! upstream's whole widget -- the trigger, the bubble, and an `OverlayPortal`
//! that puts the bubble in the application's overlay, positioned against the
//! button's own measured rectangle.
//!
//! This file used to say that composing the trigger and the bubble was the
//! application's, and stacked the bubble under the button as the column's last
//! child, with a comment calling it "the overlay slot without the overlay".
//! It kept a `tooltip_pressed` flag on the shared `DemoState` to do it. All of
//! that is gone: the framework hosts the bubble, and the demo is one widget.
//!
//! Divergences, each also marked at its site:
//!
//! * The scaffold is height-bounded ([`DEMO_HEIGHT`]): upstream's fills the
//!   demo screen, and the stage asks its content how tall it is.
//! * The two upstream behaviours that had no clock still have none, and they
//!   are the framework's gap rather than this demo's -- see `raw_tooltip.rs`,
//!   which has the timings and no owner to run them: a touch-shown tooltip
//!   lingers for `showDuration` (1500ms) and then hides, and `dismissDelay`
//!   (100ms from hover-exit to hide); hover-exit hides at once.
//!
//! What is no longer a gap: **the bubble is placed against the button**.
//! `position_dependent_box` decides above or below and pulls it back from the
//! screen edges, working from where the button actually ended up -- which is
//! what `rustflutter::render::RenderRef::transform_to` answers and nothing
//! could ask before.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, Center};
use rustflutter::{Scaffold, Tooltip, TooltipBubble};

use crate::app::ids;
use crate::data::demos as catalog;
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::themes::material_demo_theme_data::MaterialDemoThemeData;

/// How tall the demo's scaffold is. Upstream the demo fills the screen; the
/// stage lays its content out unbounded, so the scaffold needs an explicit
/// height to fill -- the same answer `navigation_drawer.rs`'s demo gives.
const DEMO_HEIGHT: f32 = 420.0;

/// The demo body for the `tooltip` slug: upstream's `TooltipDemo`.
pub(super) fn tooltips() -> AnyWidget {
    // Upstream's app bar: `AppBar(title: Text(demoTooltipTitle))` on the demo
    // theme's app-bar colors (`MaterialDemoThemeData.appBarTheme`) -- primary
    // fill, on-primary title. With no leading, the title sits 16 from the
    // start edge.
    let (bar_fill, bar_ink) = MaterialDemoThemeData::app_bar_theme();
    let title = GalleryLocalizations::en().demo_tooltip_title();
    let bar = leaf(move || {
        Container::new()
            .with_height(rustflutter::components::K_TOOLBAR_HEIGHT)
            .with_color(bar_fill)
            .with_padding(EdgeInsets::only(16.0, 0.0, 0.0, 0.0))
            .with_child(Align::new(
                Alignment::CENTER_LEFT,
                Text::new(title)
                    .with_size(20.0)
                    .with_weight(500)
                    .with_color(bar_ink),
            ))
    });

    // The instructions, upstream's `demoTooltipInstructions`, centered as
    // upstream's `Text(textAlign: TextAlign.center)` has them.
    let instructions = GalleryLocalizations::en().demo_tooltip_instructions();
    let instructions = leaf(move || Text::new(instructions).with_align(TextAlign::Center));

    // The search icon button: upstream's `IconButton(icon:
    // Icon(Icons.search), color: colorScheme.primary, onPressed: () {})` -- an
    // `InkResponse` with no tap callback, so the ink answers the press and the
    // hover, and the tooltip's gestures ride beside it.
    let button = component(SearchIconButton);
    let tooltip = Tooltip::new(ids::DEMO_LOCAL, button, || {
        component(TooltipBubble::new("Search"))
    })
    .build();

    // Upstream's body: `Center(child: Column(mainAxisAlignment:
    // MainAxisAlignment.center, children: [instructions, SizedBox(height:
    // 16), Tooltip(...)]))`.
    let body = many(vec![instructions, tooltip], |rendered| {
        let mut flex = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(16.0);
        for child in rendered {
            flex = flex.push(child);
        }
        Box::new(Center::new(flex))
    });

    let scaffold = Scaffold::new(body).with_app_bar(bar);
    single(component(scaffold), |inner| {
        Box::new(Container::new().with_height(DEMO_HEIGHT).with_child(inner))
    })
}

/// The button, as its own component so the glyph can take the demo theme's
/// primary colour -- a leaf has no `BuildContext`, and the colour is a theme
/// lookup.
///
/// Upstream's `IconButton` is an `InkResponse` under the icon: a circular
/// hover highlight behind the glyph and a splash on tap. The empty
/// `onPressed` means no tap callback, so there is no `on_tap` here either --
/// the ink is the button's only answer to a press.
struct SearchIconButton;

impl Component for SearchIconButton {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let primary = theme_of(context).primary;
        rustflutter::framework::stateful(
            rustflutter::ink_well::InkResponse::new(ids::DEMO_LOCAL + 1, move || {
                leaf(move || {
                    rustflutter::widgets::Container::new()
                        .with_size(48.0, 48.0)
                        .with_child(Align::new(
                            Alignment::CENTER,
                            Text::new(catalog::icon::SEARCH)
                                .with_font_family(catalog::MATERIAL_ICONS)
                                .with_size(24.0)
                                .with_color(primary),
                        ))
                })
            })
            // Upstream's overlay colours for this button (`icon_button.dart`,
            // Material 3, `useMaterial3: true`): `build` routes through
            // `styleFrom(foregroundColor: color)`, whose
            // `_IconButtonDefaultOverlay` resolves the hover to
            // `foregroundColor.withOpacity(0.08)` and the pressed state to
            // `foregroundColor.withOpacity(0.10)` -- the foreground here is
            // the scheme's primary, so both are tinted purple, not grey. The
            // pressed *highlight* is the gallery theme's `highlightColor:
            // Colors.transparent`; the splash carries the pressed tint.
            .with_hover_color(primary.with_alpha(0x14))
            .with_splash_color(primary.with_alpha(0x1A))
            .with_highlight_color(Color(0x00000000)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox};

    #[test]
    fn the_search_glyph_is_the_material_icons_search_codepoint() {
        // The icon the demo shows, pinned so a drive-by edit is loud.
        assert_eq!(catalog::icon::SEARCH, "\u{e567}");
    }

    #[test]
    fn the_stage_is_the_demo_height() {
        // The scaffold is height-bounded, as upstream's fills the screen and
        // the stage asks its content how tall it is.
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), tooltips()));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::loose(460.0, 820.0));
        assert_eq!(size.height, DEMO_HEIGHT);
    }

    #[test]
    fn the_bubble_is_not_in_the_page_it_is_in_the_overlay() {
        // What the rewiring bought. The demo builds one subtree; the bubble is
        // not a sibling of the button in it, and does not appear at all until
        // the pointer rests on the target.
        let showing = rustflutter::theatre::PortalController::new();
        assert!(!showing.is_showing(), "nothing is up before a hover");
    }

    #[test]
    fn the_bubble_is_offset_from_the_targets_centre_and_not_its_edge() {
        // Upstream's `Tooltip` defaults, pinned because the demo takes them and
        // would not notice if they changed: `preferBelow: true` and a 24
        // vertical offset.
        //
        // The offset is measured from the target's **centre** --
        // `positionDependentBox` is handed `target` as a point, not a rect --
        // so a 48-tall button with the 24 default puts the bubble exactly flush
        // with its bottom edge. A target taller than twice the offset would
        // have the bubble overlapping it, which is upstream's arithmetic and
        // not a bug here.
        let place = rustflutter::tooltip::tooltip_placement(
            rustflutter::tooltip::DEFAULT_VERTICAL_OFFSET,
            true,
        );
        let target = rustflutter::engine::Rect::xywh(200.0, 200.0, 48.0, 48.0);
        let at = place(
            target,
            rustflutter::render::Size::new(80.0, 32.0),
            rustflutter::render::Size::new(800.0, 600.0),
        );
        let centre_y = (target.top + target.bottom) / 2.0;
        assert_eq!(
            at.dy,
            centre_y + rustflutter::tooltip::DEFAULT_VERTICAL_OFFSET,
            "below the centre by the offset"
        );
        assert_eq!(
            at.dy, target.bottom,
            "which for this button is exactly flush"
        );

        // And above, when asked: the same distance the other way, less the
        // bubble's own height, since it is placed by its top-left.
        let above = rustflutter::tooltip::tooltip_placement(
            rustflutter::tooltip::DEFAULT_VERTICAL_OFFSET,
            false,
        );
        let up = above(
            target,
            rustflutter::render::Size::new(80.0, 32.0),
            rustflutter::render::Size::new(800.0, 600.0),
        );
        assert!(up.dy < at.dy, "the other side of the button");
    }
}
