// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/shrine/supplemental/product_card.dart` (flutter/
//! gallery @ d12640d): `MobileProductCard`, `DesktopProductCard` and the
//! shared `_buildProductCard`.
//!
//! A card is the product's photograph (cover-fit), its name and price
//! centred under it, an `add_shopping_cart` icon at the top start corner,
//! and a tap anywhere on the card adds the product to the cart -- upstream's
//! `onTap: () => model.addProductToCart(product.id)`.
//!
//! The mobile card is sized by an aspect ratio (the column gives it a width
//! and the image's height follows); the desktop card by an explicit image
//! width. Upstream's `FadeInImagePlaceholder` fades the photograph in over a
//! grey placeholder; the framework's shared image cache hands the decoded
//! image back a frame after it was asked for, which is the same trade
//! without the fade, and the placeholder is the same grey.

use rustflutter::framework::{AnyWidget, StateHandle, leaf, single};
use rustflutter::gestures::PointerHandlers;
use rustflutter::painting::Image;
use rustflutter::prelude::*;
use rustflutter::render::{BoxFit, CrossAxisAlignment, MainAxisAlignment};
use rustflutter::widgets::{AspectRatio, Container, ImageView, Padding, Pointer, SizedBox, Stack};

use crate::data::demos::MATERIAL_ICONS;

use super::super::app::ShrineState;
use super::super::colors::SHRINE_BROWN_900;
use super::super::ids;
use super::super::model::product::Product;
use super::super::theme;

/// Upstream's `MobileProductCard.defaultTextBoxHeight`.
pub const DEFAULT_TEXT_BOX_HEIGHT: f32 = 65.0;

/// Upstream's `MobileProductCard`'s default `imageAspectRatio`, 33 / 49.
pub const DEFAULT_IMAGE_ASPECT_RATIO: f32 = 33.0 / 49.0;

/// `Icons.add_shopping_cart`, drawn from the Material icon font.
const ADD_SHOPPING_CART: &str = "\u{e05a}";

/// Upstream's `NumberFormat.simpleCurrency(decimalDigits: 0)` for the card's
/// price. The catalogue's prices are whole dollars, so this is `$` plus the
/// number.
pub fn format_price(price: u32) -> String {
    format!("${price}")
}

/// The photograph, cover-fit into `width` x `height`.
fn photo(
    product: &'static Product,
    width: f32,
    height: f32,
) -> impl rustflutter::render::RenderBox {
    // Keyed by id, so the thirty-eight photographs are decoded once for the
    // life of the process rather than once per frame.
    let image = match Image::shared(&format!("shrine:{}", product.id), product.photo) {
        Some(image) => boxed(ImageView::with_fit(image, BoxFit::Cover)),
        // Upstream's placeholder: `Colors.black.withOpacity(0.1)`.
        None => boxed(Container::new().with_color(Color::BLACK.with_alpha(0x19))),
    };
    SizedBox::new(width, height).with_child(image)
}

/// Upstream's `_buildProductCard`.
fn build_product_card(
    product: &'static Product,
    image_width: Option<f32>,
    image_aspect_ratio: Option<f32>,
    handle: &StateHandle<ShrineState>,
) -> AnyWidget {
    let name_style = theme::label_large();
    let price_style = theme::body_small();

    let image: AnyWidget = match (image_width, image_aspect_ratio) {
        // Desktop: the width is explicit and the height follows the
        // photograph's own aspect ratio, upstream's
        // `height: imageWidth / product.assetAspectRatio`.
        (Some(width), _) => leaf(move || photo(product, width, width / product.ratio)),
        // Mobile: the column sets the width, the aspect ratio sets the
        // height -- upstream's `AspectRatio(aspectRatio: imageAspectRatio)`.
        (None, Some(ratio)) => leaf(move || {
            let height = width_of_card(ratio);
            AspectRatio::new(ratio, photo(product, width_of_card(ratio), height))
        }),
        _ => unreachable!("a card is built with a width or an aspect ratio"),
    };

    let handle_tap = handle.clone();
    let product_id = product.id;
    let tap = PointerHandlers::new().with_tap(move |_| {
        handle_tap.set_state(move |state| state.model.add_product_to_cart(product_id));
    });

    single(image, move |image| {
        let text_column = Column::new()
            .with_main_axis_alignment(MainAxisAlignment::Start)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .push(SizedBox::height(23.0))
            .push(
                Text::new(product.name)
                    .with_style(name_style.clone())
                    .centered(),
            )
            .push(SizedBox::height(4.0))
            .push(Text::new(format_price(product.price)).with_style(price_style.clone()));

        let column = Column::new()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        let column = match image_width {
            // The text box is exactly the image's width on desktop (upstream's
            // `SizedBox(width: imageWidth, child: Text(...))` keeps the name
            // from out-widening the photograph).
            Some(width) => column
                .push(image)
                .push(Container::new().with_width(width).with_child(text_column)),
            None => column.push(image).push(text_column),
        };

        let card = Stack::new().push(column).push(Padding::all(
            16.0,
            Text::new(ADD_SHOPPING_CART)
                .with_font_family(MATERIAL_ICONS)
                .with_size(24.0)
                .with_color(SHRINE_BROWN_900),
        ));
        Pointer::new(ids::PRODUCT_CARD + product_id as u64, card).with_handlers(tap.clone())
    })
}

/// The card's laid-out width is the aspect ratio's input on mobile; the
/// render side never reads it (the `AspectRatio` does the sizing), so this
/// is only the image's fallback height math -- see the `leaf` above.
fn width_of_card(ratio: f32) -> f32 {
    let _ = ratio;
    0.0
}

/// Upstream's `MobileProductCard`.
pub fn mobile_product_card(
    product: &'static Product,
    image_aspect_ratio: f32,
    handle: &StateHandle<ShrineState>,
) -> AnyWidget {
    assert!(image_aspect_ratio > 0.0);
    build_product_card(product, None, Some(image_aspect_ratio), handle)
}

/// Upstream's `DesktopProductCard`.
pub fn desktop_product_card(
    product: &'static Product,
    image_width: f32,
    handle: &StateHandle<ShrineState>,
) -> AnyWidget {
    build_product_card(product, Some(image_width), None, handle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prices_format_as_whole_dollars() {
        assert_eq!(format_price(120), "$120");
        assert_eq!(format_price(7), "$7");
    }

    #[test]
    fn the_default_metrics_are_upstreams() {
        assert_eq!(DEFAULT_TEXT_BOX_HEIGHT, 65.0);
        assert!((DEFAULT_IMAGE_ASPECT_RATIO - 33.0 / 49.0).abs() < 1e-6);
    }
}
