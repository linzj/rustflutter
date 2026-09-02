// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/shrine/model/product.dart` (flutter/gallery @
//! d12640d): the `Product`/`Category` types.
//!
//! The types themselves are defined in `products_repository.rs` next to the
//! catalogue, because that file is generated (`tools/gen_shrine.py`) and the
//! generator folded them in rather than importing them from here; this module
//! re-exports them and carries what the generator did not:
//!
//! * upstream's `categories` list, in upstream's order (ALL, ACCESSORIES,
//!   CLOTHING, HOME). The generated `CATEGORIES` runs ALL, ACCESSORIES, HOME,
//!   CLOTHING -- the order the pre-split aggregate grid used. The category
//!   menu reads this module's.
//! * the `assetName`/`assetPackage` getters, as functions: the photograph is
//!   already bytes here (`Product::photo`), but the names are part of the
//!   data contract.
//! * `categoryAll`: upstream models "all" as a `Category` object; here it is
//!   `None`, so the list is `Option<Category>` with `None` first.
//!
//! `Product.isFeatured` is not carried: upstream declares it and nothing
//! outside `product.dart` reads it, so the generator left it out.

use crate::l10n::gallery_localizations::GalleryLocalizations;

pub use super::products_repository::{Category, PRODUCTS, Product};

/// Upstream's `categories`: `[categoryAll, categoryAccessories,
/// categoryClothing, categoryHome]`.
pub const CATEGORIES: &[Option<Category>] = &[
    None,
    Some(Category::Accessories),
    Some(Category::Clothing),
    Some(Category::Home),
];

/// Upstream's `Category.name(context)`: the localised, upper-case caption.
/// English only, per PORTING.md.
pub fn category_name(category: Option<Category>) -> &'static str {
    let l10n = GalleryLocalizations::en();
    match category {
        None => l10n.shrine_category_name_all(),
        Some(Category::Accessories) => l10n.shrine_category_name_accessories(),
        Some(Category::Clothing) => l10n.shrine_category_name_clothing(),
        Some(Category::Home) => l10n.shrine_category_name_home(),
    }
}

/// Upstream's `Product.assetName`.
pub fn asset_name(product: &Product) -> String {
    format!("{}-0.jpg", product.id)
}

/// Upstream's `Product.assetPackage`.
pub fn asset_package() -> &'static str {
    "shrine_images"
}

/// Upstream's `Product.name(context)`. The generated table carries the
/// English names, and PORTING.md keeps the port English-only.
pub fn product_name(product: &Product) -> &'static str {
    product.name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_categories_run_in_upstreams_order() {
        assert_eq!(
            CATEGORIES,
            &[
                None,
                Some(Category::Accessories),
                Some(Category::Clothing),
                Some(Category::Home),
            ]
        );
    }

    #[test]
    fn the_category_names_are_upstreams_english() {
        assert_eq!(category_name(None), "ALL");
        assert_eq!(category_name(Some(Category::Accessories)), "ACCESSORIES");
        assert_eq!(category_name(Some(Category::Clothing)), "CLOTHING");
        assert_eq!(category_name(Some(Category::Home)), "HOME");
    }

    #[test]
    fn the_asset_naming_is_upstreams() {
        assert_eq!(asset_name(&PRODUCTS[0]), "0-0.jpg");
        assert_eq!(asset_name(&PRODUCTS[37]), "37-0.jpg");
        assert_eq!(asset_package(), "shrine_images");
    }

    #[test]
    fn every_product_id_is_its_index() {
        // `app_state_model.rs` prices the cart by indexing the catalogue
        // with the id, the way upstream's `AppStateModel` does.
        for (index, product) in PRODUCTS.iter().enumerate() {
            assert_eq!(product.id as usize, index);
        }
    }
}
