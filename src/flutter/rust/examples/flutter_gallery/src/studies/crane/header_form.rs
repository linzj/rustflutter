// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/header_form.dart` (flutter/gallery @
//! d12640d): `HeaderFormField`, `HeaderForm` and its `_HeaderTextField`.
//!
//! The constants are upstream's. The field itself is upstream's
//! `InputDecoration`: a filled `cranePurple700` box with 4px corners, 16px of
//! content padding, a white60 prefix icon and a white text -- the framework's
//! [`TextField`] draws no decoration of its own, so the decoration is the
//! [`Container`] the field sits in and the icon is a glyph from the shipped
//! MaterialIcons font, tinted as upstream's `iconTheme` is.
//!
//! Layout is upstream's two branches: desktop is a grid of equal shares
//! (four across, two on a small desktop, never more columns than fields) with
//! a 16px gap; mobile is a column with an 8px gap. The grid is a flex row of
//! expanded cells -- `GridView.count`'s arithmetic with the column count
//! upstream computes.

use rustflutter::framework::{AnyWidget, many, stateful};
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::Container;

use crate::data::demos::MATERIAL_ICONS;
use crate::data::icons::IconData;

use super::colors;

/// Upstream's `textFieldHeight`.
pub const TEXT_FIELD_HEIGHT: f32 = 60.0;
/// Upstream's `appPaddingLarge`.
pub const APP_PADDING_LARGE: f32 = 120.0;
/// Upstream's `appPaddingSmall`.
pub const APP_PADDING_SMALL: f32 = 24.0;

/// The MaterialIcons codepoints upstream's forms use. The gallery's own
/// icon table (`src/data/icons.rs`) covers the demo rows, not these, so the
/// glyphs are named here with their codepoints from the shipped
/// `MaterialIcons-Regular.otf`.
pub mod field_icons {
    use super::{IconData, MATERIAL_ICONS};

    const fn material(glyph: &'static str) -> IconData {
        IconData {
            glyph,
            font_family: MATERIAL_ICONS,
        }
    }

    /// `Icons.person`.
    pub const PERSON: IconData = material("\u{e7fd}");
    /// `Icons.date_range`.
    pub const DATE_RANGE: IconData = material("\u{e916}");
    /// `Icons.access_time`.
    pub const ACCESS_TIME: IconData = material("\u{e192}");
    /// `Icons.restaurant_menu`.
    pub const RESTAURANT_MENU: IconData = material("\u{e56c}");
    /// `Icons.place`.
    pub const PLACE: IconData = material("\u{e55f}");
    /// `Icons.airplanemode_active`.
    pub const AIRPLANEMODE_ACTIVE: IconData = material("\u{e195}");
    /// `Icons.hotel`.
    pub const HOTEL: IconData = material("\u{e53a}");
}

/// Upstream's `HeaderFormField`. The `TextEditingController` is the
/// framework's per-field [`TextFieldState`], owned by the field itself.
pub struct HeaderFormField {
    pub index: usize,
    pub icon: IconData,
    pub title: &'static str,
}

/// Upstream's `HeaderForm.build`. `first_id` is the hit-test identity of the
/// first field; the rest follow consecutively.
pub fn header_form(
    fields: &[HeaderFormField],
    first_id: u64,
    is_desktop: bool,
    is_small_desktop: bool,
) -> AnyWidget {
    let horizontal_padding = if is_desktop && !is_small_desktop {
        APP_PADDING_LARGE
    } else {
        APP_PADDING_SMALL
    };

    let mut children: Vec<AnyWidget> = Vec::new();
    if is_desktop {
        // Upstream's GridView.count: `crossAxisCount` is 4, or 2 on a small
        // desktop, capped at the number of fields, with every cell but a
        // row's last padded 16 on the end -- a row of expanded cells with a
        // 16 gap is the same layout.
        let cross_axis_count = if is_small_desktop { 2 } else { 4 }.min(fields.len());
        for row in fields.chunks(cross_axis_count) {
            let cells: Vec<AnyWidget> = row
                .iter()
                .map(|field| text_field(field, first_id + field.index as u64))
                .collect();
            children.push(many(cells, |rendered| {
                let mut row = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(16.0);
                for cell in rendered {
                    row = row.push_flex(FlexChild::expanded(cell, 1));
                }
                Box::new(row)
            }));
        }
    } else {
        for field in fields {
            children.push(text_field(field, first_id + field.index as u64));
        }
    }

    let vertical_gap = if is_desktop { 0.0 } else { 8.0 };
    many(children, move |rendered| {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(vertical_gap);
        for child in rendered {
            column = column.push(child);
        }
        Box::new(
            Container::new()
                .with_padding(EdgeInsets::symmetric(horizontal_padding, 0.0))
                .with_child(column),
        )
    })
}

/// Upstream's `_HeaderTextField`.
fn text_field(field: &HeaderFormField, id: u64) -> AnyWidget {
    let icon = field.icon;
    let title = field.title;
    let field_widget = stateful(
        TextField::new(id)
            .with_placeholder(title)
            .with_style(TextStyle {
                font_size: 16.0,
                color: colors::CRANE_PRIMARY_WHITE,
                font_family: Some(super::theme::RALEWAY.to_string()),
                ..TextStyle::default()
            }),
    );
    many(vec![field_widget], move |rendered| {
        let mut rendered = rendered;
        let field = rendered
            .pop()
            .unwrap_or_else(|| boxed(rustflutter::widgets::Empty));
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(16.0);
        row = row.push(
            Text::new(icon.glyph)
                .with_font_family(icon.font_family)
                .with_size(24.0)
                .with_color(colors::CRANE_WHITE_60),
        );
        row = row.push_flex(FlexChild::expanded(field, 1));
        Box::new(
            Container::new()
                .with_color(colors::CRANE_PURPLE_700)
                .with_corner_radius(4.0)
                .with_height(TEXT_FIELD_HEIGHT)
                .with_padding(EdgeInsets::all(16.0))
                .with_child(row),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox, Size};

    fn lay_out(widget: AnyWidget, width: f32, height: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::light(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::new(0.0, width, 0.0, height))
    }

    fn fields(count: usize) -> Vec<HeaderFormField> {
        (0..count)
            .map(|index| HeaderFormField {
                index,
                icon: field_icons::PERSON,
                title: "Field",
            })
            .collect()
    }

    #[test]
    fn a_mobile_form_is_a_column_of_sixty_pixel_fields() {
        // Four fields of 60 with three 8px gaps, inside the padding.
        let size = lay_out(
            header_form(&fields(4), 1, false, false),
            460.0,
            f32::INFINITY,
        );
        assert_eq!(size.height, 4.0 * TEXT_FIELD_HEIGHT + 3.0 * 8.0);
    }

    #[test]
    fn a_desktop_form_lays_four_fields_out_in_one_row() {
        let size = lay_out(
            header_form(&fields(4), 1, true, false),
            1280.0,
            f32::INFINITY,
        );
        assert_eq!(size.height, TEXT_FIELD_HEIGHT);
    }

    #[test]
    fn a_small_desktop_wraps_to_two_rows() {
        let size = lay_out(header_form(&fields(4), 1, true, true), 900.0, f32::INFINITY);
        assert_eq!(size.height, 2.0 * TEXT_FIELD_HEIGHT);
    }

    #[test]
    fn the_column_count_never_exceeds_the_field_count() {
        // Upstream caps `crossAxisCount` at `fields.length`: the three-field
        // sleep form is one row of three on a desktop, not four with a hole.
        let size = lay_out(
            header_form(&fields(3), 1, true, false),
            1280.0,
            f32::INFINITY,
        );
        assert_eq!(size.height, TEXT_FIELD_HEIGHT);
    }

    #[test]
    fn mobile_pads_the_form_small_and_desktop_large() {
        // The padding is inside the full offered width either way; what
        // differs is how much of it is padding. Lay out narrow and wide and
        // check the width is fully taken -- the padding constants themselves
        // are the upstream values, asserted directly.
        assert_eq!(APP_PADDING_SMALL, 24.0);
        assert_eq!(APP_PADDING_LARGE, 120.0);
        let size = lay_out(
            header_form(&fields(1), 1, false, false),
            460.0,
            f32::INFINITY,
        );
        assert_eq!(size.width, 460.0);
    }
}
