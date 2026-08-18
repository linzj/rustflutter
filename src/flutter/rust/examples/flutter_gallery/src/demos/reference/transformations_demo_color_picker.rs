// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Maps to `lib/demos/reference/transformations_demo_color_picker.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! A generic widget for a list of selectable colors: [`ColorPicker`] is
//! upstream's `ColorPicker`, a centered row of swatches, and `ColorSwatch`
//! below is upstream's `_ColorPickerSwatch` -- a 60x60 `RawMaterialButton`
//! filled with the color, its 2dp horizontal padding on the `Container`
//! around it, a white check on the selected one.
//!
//! Divergences, each marked at its site as well:
//!
//! * **the check is the icon font's glyph** -- upstream draws
//!   `Icons.check`/`Colors.white`; the gallery has the Material Icons font
//!   registered, so the check is the same codepoint the demo page's chrome
//!   draws (`data/demos.rs`'s `icon::CHECK`).
//! * **the swatch corners are square** -- upstream's `RawMaterialButton`
//!   takes the button theme's shape, which the gallery never sets, so it is
//!   the default `RoundedRectangleBorder()` with no radius; the framework's
//!   `Button` is a filled *rounded* Material 3 button, so the swatch is a
//!   colored `Container` with a tap region instead of a `Button`.

use std::rc::Rc;

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, MainAxisAlignment, MainAxisSize, RenderFlex,
};
use rustflutter::widgets::{Align, Pointer};

use crate::data::demos::{self, icon};

/// A generic widget for a list of selectable colors. Upstream's
/// `ColorPicker`.
pub struct ColorPicker {
    /// The hit-test id of the first swatch; the rest count on from it.
    id_base: u64,
    /// Upstream's `colors`. A `Set<Color>` there; the one caller builds it
    /// from a literal, so a `Vec` carries the same colors in the same order.
    colors: Vec<Color>,
    /// Upstream's `selectedColor`.
    selected_color: Color,
    /// Upstream's `onColorSelection`.
    on_color_selection: Rc<dyn Fn(Color)>,
}

impl ColorPicker {
    pub fn new(
        id_base: u64,
        colors: Vec<Color>,
        selected_color: Color,
        on_color_selection: impl Fn(Color) + 'static,
    ) -> ColorPicker {
        ColorPicker {
            id_base,
            colors,
            selected_color,
            on_color_selection: Rc::new(on_color_selection),
        }
    }
}

impl Component for ColorPicker {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let swatches: Vec<AnyWidget> = self
            .colors
            .iter()
            .enumerate()
            .map(|(index, color)| {
                component(ColorSwatch {
                    id: self.id_base + index as u64,
                    color: *color,
                    selected: *color == self.selected_color,
                    on_tap: Rc::clone(&self.on_color_selection),
                })
            })
            .collect();
        many(swatches, move |rendered| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_main_axis_alignment(MainAxisAlignment::Center)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            for swatch in rendered {
                row = row.push(swatch);
            }
            Box::new(row)
        })
    }
}

/// A single selectable color widget in the ColorPicker. Upstream's
/// `_ColorPickerSwatch`.
struct ColorSwatch {
    id: u64,
    color: Color,
    selected: bool,
    on_tap: Rc<dyn Fn(Color)>,
}

impl Component for ColorSwatch {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let id = self.id;
        let color = self.color;
        let selected = self.selected;
        let on_tap = Rc::clone(&self.on_tap);
        leaf(move || {
            // The button itself: `RawMaterialButton(fillColor: color)` with the
            // theme's default shape, which is square (see the module header).
            let mut button = Container::new().with_color(color);
            if selected {
                button = button.with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(icon::CHECK)
                        .with_font_family(demos::MATERIAL_ICONS)
                        .with_size(24.0)
                        .with_color(Color::WHITE),
                ));
            }
            let on_tap = Rc::clone(&on_tap);
            Pointer::new(
                id,
                Container::new()
                    .with_width(60.0)
                    .with_height(60.0)
                    .with_padding(EdgeInsets::only(2.0, 0.0, 2.0, 0.0))
                    .with_child(button),
            )
            .with_handlers(PointerHandlers::new().with_tap(move |_| on_tap(color)))
        })
    }
}
