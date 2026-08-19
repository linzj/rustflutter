// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/shrine/home.dart` (flutter/gallery @ d12640d).
//!
//! This is the current aggregate Shrine implementation, re-homed from
//! `src/studies/mod.rs` in the M-G split; per-file alignment with upstream is
//! in flight. Upstream's home is the backdrop's front layer over the
//! category menu; what is here is one representative screen -- a filter row,
//! a product grid and a cart count -- as the gallery's one-screen-per-study
//! scope decision allows (PORTING.md). The grid sizes its tiles uniformly
//! (PORTING.md: "shrine uniform grid").

use rustflutter::components::theme_of;
use rustflutter::framework::{component, leaf, many, AnyWidget, BuildContext, StateHandle};
use rustflutter::painting::Image;
use rustflutter::prelude::*;
use rustflutter::render::{BoxFit, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::{BoxedWidget, ClipRRect, Container, ImageView};

use crate::app::{self, ids, GalleryState};
use crate::studies::shrine::model::products_repository as shrine_data;

/// The body `studies::page` wraps in the study scaffold.
pub(crate) fn screen(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let scroll_handle = handle.clone();
    let filter = state.study.filter.min(shrine_data::CATEGORIES.len() - 1);
    let selected = shrine_data::CATEGORIES[filter];
    let cart = state.study.cart;

    let mut chips: Vec<AnyWidget> = Vec::new();
    for (index, category) in shrine_data::CATEGORIES.iter().enumerate() {
        let label = category.map_or("ALL", |c| c.title());
        let chip = Chip::new(ids::STUDY_LOCAL + index as u64, label).with_selected(filter == index);
        // A fn pointer cannot capture the index, so each arm names its own.
        chips.push(component(match index {
            0 => chip.wired(handle.clone(), |s| s.study.filter = 0),
            1 => chip.wired(handle.clone(), |s| s.study.filter = 1),
            2 => chip.wired(handle.clone(), |s| s.study.filter = 2),
            _ => chip.wired(handle.clone(), |s| s.study.filter = 3),
        }));
    }

    let shown: Vec<&'static shrine_data::Product> = shrine_data::in_category(selected).collect();

    // Two columns rather than upstream's staggered pair of columns: the tiles
    // here are a fixed aspect, and a stagger needs each tile's own height fed
    // back into the column it lands in.
    let mut grid = GridList::new(2).with_spacing(16.0).with_aspect_ratio(0.72);
    for product in &shown {
        grid = grid.push(component(ProductTile { product }));
    }

    let add_handle = handle;
    let body = vec![
        component(ShrineHeader {
            cart,
            shown: shown.len(),
        }),
        many(chips, |rendered| {
            let mut row = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.0);
            for child in rendered {
                row = row.push(child);
            }
            Box::new(row)
        }),
        component(grid),
        component(Button::new(ids::STUDY_LOCAL + 20, "Add to cart").wired(
            add_handle,
            |s| &mut s.pressed,
            |s| s.study.cart += 1,
        )),
    ];

    app::scrolling_body(body, 14.0, 16.0, state, scroll_handle)
}

struct ShrineHeader {
    cart: u32,
    shown: usize,
}

impl Component for ShrineHeader {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let cart = self.cart;
        let shown = self.shown;
        let title = theme.title();
        let muted = theme.muted();
        let accent = theme.primary;

        leaf(move || {
            RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .push_flex(FlexChild::expanded(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(2.0)
                        .push(Text::new("Shrine").with_style(title.clone()))
                        .push(Text::new(format!("{shown} items")).with_style(muted.clone())),
                    1,
                ))
                .push(
                    Container::new()
                        .with_color(accent.with_alpha(0x2A))
                        .with_corner_radius(14.0)
                        .with_padding(EdgeInsets::symmetric(12.0, 7.0))
                        .with_child(
                            Text::new(format!("Cart {cart}"))
                                .with_size(12.0)
                                .with_weight(700)
                                .with_color(accent),
                        ),
                )
        })
    }
}

struct ProductTile {
    product: &'static shrine_data::Product,
}

impl Component for ProductTile {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let product = self.product;
        let surface = theme.surface_variant;
        let radius = theme.radius;
        let text = theme.text;
        let muted = theme.text_muted;
        let accent = theme.primary;
        // Keyed by id, so the thirty-eight photographs are decoded once for the
        // life of the process rather than once per frame.
        let photo = Image::shared(&format!("shrine:{}", product.id), product.photo);

        leaf(move || {
            let picture: BoxedWidget = match photo.clone() {
                // Contain rather than Cover: upstream shows the whole product,
                // which is why it carries an aspect ratio per photograph at all.
                Some(photo) => boxed(ImageView::with_fit(photo, BoxFit::Contain)),
                None => boxed(Container::new().with_color(surface)),
            };

            Column::expanded()
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(6.0)
                // The photograph takes whatever height is left after the two
                // text lines, which is what makes the tiles line up whatever
                // their names do.
                .push_flex(FlexChild::expanded(
                    Container::new()
                        .with_color(surface)
                        .with_corner_radius(radius)
                        .with_padding(EdgeInsets::all(8.0))
                        .with_child(ClipRRect::new(radius, picture)),
                    1,
                ))
                .push(
                    Text::new(product.name)
                        .with_size(12.0)
                        .with_weight(700)
                        .with_color(text),
                )
                .push(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .push_flex(FlexChild::expanded(
                            Text::new(product.category.title())
                                .with_size(10.0)
                                .with_color(muted),
                            1,
                        ))
                        .push(
                            Text::new(format!("${}", product.price))
                                .with_size(12.0)
                                .with_weight(700)
                                .with_color(accent),
                        ),
                )
        })
    }
}
