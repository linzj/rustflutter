// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/divider_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `DividerDemo` takes a `DividerDemoType` and shows one variant
//! per catalogue configuration; the catalogue here is flattened to one
//! configuration per demo (PORTING.md: "demo options section is unreachable"),
//! so the stage shows both variants stacked, the way the other multi-variant
//! demos show their variants: upstream's `_HorizontalDividerDemo` above its
//! `_VerticalDividerDemo`, each under a caption with its upstream title.
//!
//! Divergences, each marked at its site as well:
//!
//! * **fixed box heights** -- upstream's coloured boxes are `Expanded` inside
//!   a body that fills the screen; the stage here is a shrink-wrapping column
//!   inside the page's own scroll view, where an expanded child would be zero
//!   tall, so the boxes take a fixed height instead.
//! * **hand-drawn rules** -- the framework's `Divider` is a fixed 16-pixel
//!   hairline in the theme's outline colour; upstream's demo asks for a grey
//!   rule 20 tall with a 20 indent (and its vertical twin), so both rules here
//!   are drawn from `Container`s with exactly those metrics.

use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, FlexChild, MainAxisSize, RenderBox, RenderFlex,
};
use rustflutter::widgets::Align;

use crate::l10n::gallery_localizations::GalleryLocalizations;

use super::{caption, column};

/// Upstream's `Colors.deepPurpleAccent`, the leading box's fill.
const DEEP_PURPLE_ACCENT: Color = Color(0xFF7C4DFF);
/// Upstream's `Colors.deepOrangeAccent`, the trailing box's fill.
const DEEP_ORANGE_ACCENT: Color = Color(0xFFFF6E40);
/// Upstream's `Colors.grey`, the rule's colour in both variants.
const GREY: Color = Color(0xFF9E9E9E);

/// The height each coloured box stands in for an upstream `Expanded` with.
const BOX_HEIGHT: f32 = 120.0;

/// The demo body for the `divider` slug: both upstream variants.
pub(super) fn dividers() -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    column(
        vec![
            caption(l10n.demo_divider_title()),
            horizontal_divider_demo(),
            caption(l10n.demo_vertical_divider_title()),
            vertical_divider_demo(),
        ],
        12.0,
    )
}

/// One of upstream's rounded colour blocks.
fn color_block(color: Color) -> impl RenderBox + 'static {
    Container::new()
        .with_height(BOX_HEIGHT)
        // Upstream's `BorderRadius.circular(10)`.
        .with_corner_radius(10.0)
        .with_color(color)
}

/// Upstream's `_HorizontalDividerDemo`.
///
/// The `Expanded` children are fixed-height here (see the module header), and
/// the divider is drawn by hand because the framework's is not parameterizable:
/// upstream's is `Divider(color: Colors.grey, height: 20, thickness: 1,
/// indent: 20, endIndent: 0)`.
fn horizontal_divider_demo() -> AnyWidget {
    leaf(|| {
        RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .push(color_block(DEEP_PURPLE_ACCENT))
            .push(
                // height: 20 with a thickness-1 rule centred in it, inset 20
                // from the leading edge.
                Container::new()
                    .with_height(20.0)
                    .with_padding(EdgeInsets::only(20.0, 0.0, 0.0, 0.0))
                    .with_child(Align::new(
                        Alignment::CENTER,
                        Container::new().with_height(1.0).with_color(GREY),
                    )),
            )
            .push(color_block(DEEP_ORANGE_ACCENT))
    })
}

/// Upstream's `_VerticalDividerDemo`, the same constraints turned sideways:
/// `VerticalDivider(color: Colors.grey, thickness: 1, indent: 20, endIndent: 0,
/// width: 20)`.
fn vertical_divider_demo() -> AnyWidget {
    leaf(|| {
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Start);
        row = row.push_flex(FlexChild::expanded(color_block(DEEP_PURPLE_ACCENT), 1));
        row = row.push(
            // width: 20 with the thickness-1 rule centred across it, inset 20
            // from the top.
            Container::new()
                .with_width(20.0)
                .with_height(BOX_HEIGHT)
                .with_padding(EdgeInsets::only(0.0, 20.0, 0.0, 0.0))
                .with_child(Align::new(
                    Alignment::CENTER,
                    Container::new().with_width(1.0).with_color(GREY),
                )),
        );
        row.push_flex(FlexChild::expanded(color_block(DEEP_ORANGE_ACCENT), 1))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_colors_are_upstreams() {
        // `Colors.deepPurpleAccent`, `Colors.deepOrangeAccent`, `Colors.grey`.
        assert_eq!(DEEP_PURPLE_ACCENT, Color::rgb(0x7C, 0x4D, 0xFF));
        assert_eq!(DEEP_ORANGE_ACCENT, Color::rgb(0xFF, 0x6E, 0x40));
        assert_eq!(GREY, Color::rgb(0x9E, 0x9E, 0x9E));
    }

    #[test]
    fn both_variants_build() {
        // A smoke test against the render side: each variant lays out inside a
        // phone-width column without asking for more than it was offered.
        for variant in [horizontal_divider_demo(), vertical_divider_demo()] {
            let mut root = rustflutter::framework::ElementTree::new();
            root.rebuild(variant);
            let mut render = root.build_render_tree().expect("a mounted root");
            let size = render.layout(rustflutter::render::BoxConstraints::new(
                0.0,
                400.0,
                0.0,
                f32::INFINITY,
            ));
            assert!(size.width <= 400.0 && size.height.is_finite(), "{size:?}");
        }
    }
}
