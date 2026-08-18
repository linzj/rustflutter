// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/tooltip_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `TooltipDemo` is a `Scaffold` (the demo page's chrome here,
//! `src/pages/demo.rs`) whose body centers the instructions and a search
//! `IconButton` wrapped in a `Tooltip`. The framework splits a tooltip the
//! same way upstream's implementation does: [`TooltipTrigger`] is the trigger
//! half (hover shows and hides, a long press shows) and [`Tooltip`] is the
//! bubble; composing the two is the application's, as with every overlay
//! here. Whether the bubble is up is `DemoState::tooltip_pressed`.
//!
//! Two upstream behaviors have no clock to run on (the framework's
//! `TooltipTrigger` documents the same gap):
//!
//! - A touch-shown tooltip lingers for `showDuration` (1500ms) and then
//!   hides. The trigger reports show and hide through one callback, so a
//!   hover-show cannot be told from a long-press-show, and timing only the
//!   long-press case is not possible from here. The bubble stays until a
//!   hover ends it.
//! - Upstream's `dismissDelay` (100ms from hover-exit to hide) is likewise
//!   unportable; hover-exit hides at once.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, Center};

use crate::app::{ids, GalleryState};
use crate::data::demos as catalog;

use super::DemoState;

pub(super) fn tooltips(state: &DemoState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let showing = state.tooltip_pressed;

    // The instructions, upstream's `demoTooltipInstructions`, centered as
    // upstream's `Text(textAlign: TextAlign.center)` has them.
    let instructions = leaf(|| {
        Text::new("Long press or hover to display the tooltip.").with_align(TextAlign::Center)
    });
    let instructions = single(instructions, |text| Box::new(Center::new(text)));

    // The search icon button: upstream's `IconButton(icon:
    // Icon(Icons.search), color: colorScheme.primary, onPressed: () {})`. The
    // empty onPressed means no tap handler at all; the trigger's gestures are
    // the button's only behavior.
    let button = component(SearchIconButton);
    let trigger = component(
        TooltipTrigger::new(ids::DEMO_LOCAL, button)
            .wired(handle, |s, visible| s.demo.tooltip_pressed = visible),
    );
    let trigger = single(trigger, |inner| Box::new(Center::new(inner)));

    // The bubble goes under the button, where upstream's default
    // `preferBelow: true` puts it. It is the column's last child, so its
    // coming and going moves nothing -- the overlay slot without the overlay.
    let mut children = vec![instructions, trigger];
    if showing {
        children.push(single(component(Tooltip::new("Search")), |bubble| {
            Box::new(Center::new(bubble))
        }));
    }

    many(children, |rendered| {
        let mut flex = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(16.0);
        for child in rendered {
            flex = flex.push(child);
        }
        Box::new(flex)
    })
}

/// The button, as its own component so the glyph can take the demo theme's
/// primary colour -- a leaf has no `BuildContext`, and the colour is a theme
/// lookup.
struct SearchIconButton;

impl Component for SearchIconButton {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let primary = theme_of(context).primary;
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_search_glyph_is_the_material_icons_search_codepoint() {
        // The icon the demo shows, pinned so a drive-by edit is loud.
        assert_eq!(catalog::icon::SEARCH, "\u{e567}");
    }
}
