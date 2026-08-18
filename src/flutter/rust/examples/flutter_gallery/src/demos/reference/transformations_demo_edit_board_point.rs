// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Maps to `lib/demos/reference/transformations_demo_edit_board_point.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! The panel for editing a board point: [`EditBoardPoint`] is upstream's
//! `EditBoardPoint` -- the point's `q, r` in bold, right-aligned, over a
//! [`ColorPicker`] of the five colors upstream's `boardPointColors` set
//! holds. The colors come from the *dark* color scheme even in a light
//! gallery, because upstream reads `GalleryThemeData.darkColorScheme`
//! unconditionally; [`Scheme::dark`] is that table.
//!
//! [`Scheme::dark`]: crate::themes::gallery_theme_data::Scheme::dark
//!
//! No divergences of its own; how the panel is presented (upstream's
//! `showModalBottomSheet`) is the caller's divergence -- see
//! `transformations_demo.rs`'s header.

use std::rc::Rc;

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex};

use crate::themes::gallery_theme_data::Scheme;

use super::transformations_demo_board::BoardPoint;
use super::transformations_demo_color_picker::ColorPicker;

/// Upstream's `backgroundColor`: the board's backdrop, and one of the colors
/// a point can take -- painting a point this color is how the demo "deletes"
/// one, which is why the edit file owns the constant upstream.
pub const BACKGROUND_COLOR: Color = Color(0xFF272727);

/// The panel for editing a board point. Upstream's `EditBoardPoint`.
pub struct EditBoardPoint {
    /// The hit-test id of the picker's first swatch.
    id_base: u64,
    /// Upstream's `boardPoint`.
    board_point: BoardPoint,
    /// Upstream's `onColorSelection`.
    on_color_selection: Rc<dyn Fn(Color)>,
}

impl EditBoardPoint {
    pub fn new(
        id_base: u64,
        board_point: BoardPoint,
        on_color_selection: impl Fn(Color) + 'static,
    ) -> EditBoardPoint {
        EditBoardPoint {
            id_base,
            board_point,
            on_color_selection: Rc::new(on_color_selection),
        }
    }
}

impl Component for EditBoardPoint {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        // Upstream's `boardPointColors`: white, and the dark scheme's primary,
        // primaryContainer and secondary, and the board's background color --
        // the dark scheme whether or not the gallery is dark (see the module
        // header).
        let scheme = Scheme::dark();
        let board_point_colors = vec![
            Color::WHITE,
            scheme.primary,
            scheme.primary_container,
            scheme.secondary,
            BACKGROUND_COLOR,
        ];

        let point = self.board_point;
        // `Text('${boardPoint.q}, ${boardPoint.r}', textAlign:
        // TextAlign.right, style: bold)`.
        let title = leaf(move || {
            Text::new(format!("{}, {}", point.q, point.r))
                .with_style(TextStyle {
                    font_weight: 700,
                    color: Color::WHITE,
                    ..TextStyle::default()
                })
                .with_align(TextAlign::Right)
        });
        let picker = component(ColorPicker::new(
            self.id_base,
            board_point_colors,
            point.color,
            {
                let on_color_selection = Rc::clone(&self.on_color_selection);
                move |color| on_color_selection(color)
            },
        ));

        many(vec![title, picker], move |mut rendered| {
            let picker = rendered.pop().expect("the color picker");
            let title = rendered.pop().expect("the title");
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            column = column.push(title);
            column = column.push(picker);
            Box::new(column)
        })
    }
}
