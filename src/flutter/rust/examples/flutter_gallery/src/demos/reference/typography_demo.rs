// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/reference/typography_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `TypographyDemo` is the 2018 Material type scale as a list of
//! thirteen `_TextStyleItem` rows: the style's name in a 72-wide column
//! (`textTheme.bodySmall`) and the style's description ("Light 96sp", ...)
//! set in the style itself.
//!
//! Divergences, each also marked at its site:
//!
//! * The `Scaffold`/`AppBar` (title `demoTypographyTitle`) is the demo
//!   page's own chrome (`src/pages/demo.rs`); the stage starts at the body,
//!   the `Scrollbar` around the `ListView`. The list is height-bounded
//!   ([`LIST_HEIGHT`]) because the demo page's stage does not guarantee a
//!   bounded height.
//! * The styles are the 2018 scale's values as data rather than reads off
//!   the ambient `TextTheme`: the framework's `Theme` carries no typography
//!   (`src/themes/material_demo_theme_data.rs`, whose upstream
//!   `Typography.material2018` is exactly this scale), so the demo states
//!   them. Roboto is not among the fonts the gallery ships; the framework
//!   default stands in, as it does for the demo theme.

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::{ListView, Pointer};

use crate::app::ids;

/// How tall the style list is. Upstream's `ListView` fills the demo screen;
/// here the stage does not guarantee a bounded height, so the list gets a
/// fixed window it scrolls inside.
const LIST_HEIGHT: f32 = 480.0;

/// The width of the name column, upstream's `SizedBox(width: 72)`.
const NAME_WIDTH: f32 = 72.0;

/// One row of the type scale, upstream's `_TextStyleItem`: the style's name,
/// the description set in it, and the style itself as (size, weight, letter
/// spacing) -- the 2018 scale's `englishLike` values.
struct TextStyleItem {
    name: &'static str,
    text: &'static str,
    font_size: f32,
    font_weight: i32,
    letter_spacing: f32,
}

/// Upstream's `styleItems`, in upstream's order: the 2018 scale from
/// `displayLarge` (upstream's `headline1`) to `labelSmall` (`overline`).
const STYLE_ITEMS: [TextStyleItem; 13] = [
    TextStyleItem {
        name: "Headline 1",
        text: "Light 96sp",
        font_size: 96.0,
        font_weight: 300,
        letter_spacing: -1.5,
    },
    TextStyleItem {
        name: "Headline 2",
        text: "Light 60sp",
        font_size: 60.0,
        font_weight: 300,
        letter_spacing: -0.5,
    },
    TextStyleItem {
        name: "Headline 3",
        text: "Regular 48sp",
        font_size: 48.0,
        font_weight: 400,
        letter_spacing: 0.0,
    },
    TextStyleItem {
        name: "Headline 4",
        text: "Regular 34sp",
        font_size: 34.0,
        font_weight: 400,
        letter_spacing: 0.25,
    },
    TextStyleItem {
        name: "Headline 5",
        text: "Regular 24sp",
        font_size: 24.0,
        font_weight: 400,
        letter_spacing: 0.0,
    },
    TextStyleItem {
        name: "Headline 6",
        text: "Medium 20sp",
        font_size: 20.0,
        font_weight: 500,
        letter_spacing: 0.15,
    },
    TextStyleItem {
        name: "Subtitle 1",
        text: "Regular 16sp",
        font_size: 16.0,
        font_weight: 400,
        letter_spacing: 0.15,
    },
    TextStyleItem {
        name: "Subtitle 2",
        text: "Medium 14sp",
        font_size: 14.0,
        font_weight: 500,
        letter_spacing: 0.1,
    },
    TextStyleItem {
        name: "Body Text 1",
        text: "Regular 16sp",
        font_size: 16.0,
        font_weight: 400,
        letter_spacing: 0.5,
    },
    TextStyleItem {
        name: "Body Text 2",
        text: "Regular 14sp",
        font_size: 14.0,
        font_weight: 400,
        letter_spacing: 0.25,
    },
    TextStyleItem {
        name: "Button",
        text: "MEDIUM (ALL CAPS) 14sp",
        font_size: 14.0,
        font_weight: 500,
        letter_spacing: 1.25,
    },
    TextStyleItem {
        name: "Caption",
        text: "Regular 12sp",
        font_size: 12.0,
        font_weight: 400,
        letter_spacing: 0.4,
    },
    TextStyleItem {
        name: "Overline",
        text: "REGULAR (ALL CAPS) 10sp",
        font_size: 10.0,
        font_weight: 400,
        letter_spacing: 1.5,
    },
];

/// The demo body for the `typography` slug.
pub(super) fn stage() -> AnyWidget {
    // The `Scrollbar`'s child is a builder because the list is rebuilt from
    // it on every frame; the scroll offset lives in the list's own state.
    let scrollable = scrollbar(move || stateful(TypographyList));
    single(scrollable, move |inner| {
        Box::new(Container::new().with_height(LIST_HEIGHT).with_child(inner))
    })
}

/// The scrollable list itself, with its own `Scroll` for a state.
struct TypographyList;

#[derive(Default)]
struct TypographyListState {
    scroll: Scroll,
}

impl StatefulComponent for TypographyList {
    type State = TypographyListState;

    fn advance(&self, state: &mut TypographyListState, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &TypographyListState,
        handle: StateHandle<TypographyListState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        // Dispatched from this element so the `Scrollbar` above hears the
        // list's movements -- upstream's notification bubbling.
        state
            .scroll
            .set_notification_sink(context.notification_sink());
        let offset = state.scroll.offset;
        let extent = state.scroll.link();
        let ink = theme_of(context).text;

        // The same handlers the list demo gives its lists, against this
        // list's own `Scroll`.
        let down_handle = handle.clone();
        let drag_handle = handle.clone();
        let end_handle = handle.clone();
        let wheel_handle = handle;
        let handlers = PointerHandlers::new()
            .with_pointer_down(move |_| {
                down_handle.set_state(|state| state.scroll.stop());
            })
            .with_drag_update(move |drag| {
                let delta = drag.delta.dy;
                drag_handle.set_state(move |state| state.scroll.scroll_by(-delta));
            })
            .with_drag_end(move |end| {
                let velocity = end.velocity.dy;
                end_handle.set_state(move |state| state.scroll.fling(-velocity));
            })
            .with_scroll(move |scroll| {
                let delta = scroll.delta.dy;
                wheel_handle.set_state(move |state| state.scroll.scroll_by(delta));
            });

        let items: Vec<AnyWidget> = STYLE_ITEMS.iter().map(|item| row(item, ink)).collect();

        many(items, move |rendered| {
            let mut flex = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for item in rendered {
                flex = flex.push(item);
            }
            let list = ListView::new()
                .with_offset(offset)
                .with_link(extent.clone())
                .push(flex);
            Box::new(Pointer::new(ids::DEMO_LOCAL, list).with_handlers(handlers.clone()))
        })
    }
}

/// One row, upstream's `_TextStyleItem.build`: the name in a 72-wide column
/// in `bodySmall`, the description set in the style itself.
fn row(item: &TextStyleItem, ink: Color) -> AnyWidget {
    let name = item.name;
    let text = item.text;
    let sample = TextStyle {
        font_size: item.font_size,
        font_weight: item.font_weight,
        letter_spacing: Some(item.letter_spacing),
        color: ink,
        ..TextStyle::default()
    };
    leaf(move || {
        Container::new()
            // Upstream's `padding: const EdgeInsets.symmetric(horizontal: 8,
            // vertical: 16)`.
            .with_padding(EdgeInsets::symmetric(8.0, 16.0))
            .with_child(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .push(Container::new().with_width(NAME_WIDTH).with_child(
                        // Upstream's `Theme.of(context).textTheme.bodySmall`.
                        Text::new(name).with_size(12.0).with_color(ink),
                    ))
                    .push_flex(FlexChild::expanded(
                        Text::new(text).with_style(sample.clone()),
                        1,
                    )),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scale_is_upstreams_thirteen_styles() {
        assert_eq!(STYLE_ITEMS.len(), 13);
        assert_eq!(STYLE_ITEMS[0].name, "Headline 1");
        assert_eq!(STYLE_ITEMS[0].font_size, 96.0);
        assert_eq!(STYLE_ITEMS[0].font_weight, 300);
        assert_eq!(STYLE_ITEMS[12].name, "Overline");
        assert_eq!(STYLE_ITEMS[12].font_size, 10.0);
    }
}
