// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/shrine/model/app_state_model.dart` (flutter/
//! gallery @ d12640d): the `AppStateModel` cart/catalog state.
//!
//! Upstream this is a `scoped_model` `Model`; the `notifyListeners()` calls
//! are what rebuild the listening widgets. Here the model is plain data held
//! by the study's root component (`app.rs`), and the rebuild after a mutation
//! is the `StateHandle::set_state` the mutation arrived through -- so the
//! methods mutate and return, and the notifications are the framework's.
//!
//! `_productsInCart` is a `LinkedHashMap` upstream, and its iteration order
//! -- insertion order -- is load-bearing: the collapsed cart shows the first
//! three *distinct* products as thumbnails and counts the rest as overflow
//! (`expanding_bottom_sheet.dart`'s `ExtraProductsNumber`). The port keeps a
//! `Vec` of (id, quantity) pairs for the same order.

use super::product::Category;
use super::products_repository::{self, Product};

/// Upstream's `_salesTaxRate`.
pub const SALES_TAX_RATE: f64 = 0.06;
/// Upstream's `_shippingCostPerItem`.
pub const SHIPPING_COST_PER_ITEM: f64 = 7.0;

/// The whole of what the study remembers about the shop.
#[derive(Clone, Debug, Default)]
pub struct AppStateModel {
    /// Upstream's `_selectedCategory`; `None` is `categoryAll`.
    selected_category: Option<Category>,
    /// Upstream's `_productsInCart`: (id, quantity) pairs in insertion
    /// order.
    products_in_cart: Vec<(u32, u32)>,
}

impl AppStateModel {
    /// Upstream's `AppStateModel()..loadProducts()`: the catalogue is a
    /// constant here, so there is nothing to load and the constructor is the
    /// loaded state.
    pub fn loaded() -> AppStateModel {
        AppStateModel::default()
    }

    /// Upstream's `productsInCart` getter: a snapshot of the cart, in
    /// insertion order.
    pub fn products_in_cart(&self) -> Vec<(u32, u32)> {
        self.products_in_cart.clone()
    }

    /// Upstream's `totalCartQuantity`.
    pub fn total_cart_quantity(&self) -> u32 {
        self.products_in_cart
            .iter()
            .map(|(_, quantity)| quantity)
            .sum()
    }

    pub fn selected_category(&self) -> Option<Category> {
        self.selected_category
    }

    /// Upstream's `subtotalCost`. Upstream indexes `_availableProducts` with
    /// the id, which holds because the ids are the catalogue's indices (the
    /// test in `product.rs` pins that).
    pub fn subtotal_cost(&self) -> f64 {
        self.products_in_cart
            .iter()
            .map(|(id, quantity)| {
                products_repository::PRODUCTS[*id as usize].price as f64 * *quantity as f64
            })
            .sum()
    }

    /// Upstream's `shippingCost`.
    pub fn shipping_cost(&self) -> f64 {
        SHIPPING_COST_PER_ITEM * self.total_cart_quantity() as f64
    }

    /// Upstream's `tax` getter.
    pub fn tax(&self) -> f64 {
        self.subtotal_cost() * SALES_TAX_RATE
    }

    /// Upstream's `totalCost` getter.
    pub fn total_cost(&self) -> f64 {
        self.subtotal_cost() + self.shipping_cost() + self.tax()
    }

    /// Upstream's `getProducts()`: the catalogue, filtered by the selected
    /// category.
    pub fn products(&self) -> Vec<&'static Product> {
        products_repository::in_category(self.selected_category).collect()
    }

    /// Upstream's `addProductToCart`.
    pub fn add_product_to_cart(&mut self, product_id: u32) {
        match self
            .products_in_cart
            .iter_mut()
            .find(|(id, _)| *id == product_id)
        {
            Some((_, quantity)) => *quantity += 1,
            None => self.products_in_cart.push((product_id, 1)),
        }
    }

    /// Upstream's `addMultipleProductsToCart`. `quantity` must be positive.
    pub fn add_multiple_products_to_cart(&mut self, product_id: u32, quantity: u32) {
        assert!(quantity > 0);
        match self
            .products_in_cart
            .iter_mut()
            .find(|(id, _)| *id == product_id)
        {
            Some((_, existing)) => *existing += quantity,
            None => self.products_in_cart.push((product_id, quantity)),
        }
    }

    /// Upstream's `removeItemFromCart`: one less of the product, and no entry
    /// at all when the last one goes.
    pub fn remove_item_from_cart(&mut self, product_id: u32) {
        if let Some(index) = self
            .products_in_cart
            .iter()
            .position(|(id, _)| *id == product_id)
        {
            if self.products_in_cart[index].1 == 1 {
                self.products_in_cart.remove(index);
            } else {
                self.products_in_cart[index].1 -= 1;
            }
        }
    }

    /// Upstream's `getProductById`.
    pub fn product_by_id(&self, id: u32) -> &'static Product {
        products_repository::PRODUCTS
            .iter()
            .find(|product| product.id == id)
            .expect("the catalogue has every id the cart can hold")
    }

    /// Upstream's `clearCart`.
    pub fn clear_cart(&mut self) {
        self.products_in_cart.clear();
    }

    /// Upstream's `setCategory`.
    pub fn set_category(&mut self, category: Option<Category>) {
        self.selected_category = category;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_accumulates_quantity_in_insertion_order() {
        let mut model = AppStateModel::loaded();
        assert!(model.products_in_cart().is_empty());

        model.add_product_to_cart(5);
        model.add_product_to_cart(2);
        model.add_product_to_cart(5);
        // Insertion order, not id order: 5 was added first.
        assert_eq!(model.products_in_cart(), vec![(5, 2), (2, 1)]);
        assert_eq!(model.total_cart_quantity(), 3);
    }

    #[test]
    fn removing_one_at_a_time_until_the_entry_goes() {
        let mut model = AppStateModel::loaded();
        model.add_product_to_cart(5);
        model.add_product_to_cart(5);
        model.remove_item_from_cart(5);
        assert_eq!(model.products_in_cart(), vec![(5, 1)]);
        model.remove_item_from_cart(5);
        assert!(model.products_in_cart().is_empty());
        // Removing what is not there is a no-op, as upstream's containsKey
        // guard makes it.
        model.remove_item_from_cart(5);
        assert!(model.products_in_cart().is_empty());
    }

    #[test]
    fn adding_many_at_once_matches_adding_one_at_a_time() {
        let mut one = AppStateModel::loaded();
        let mut many = AppStateModel::loaded();
        for _ in 0..3 {
            one.add_product_to_cart(9);
        }
        many.add_multiple_products_to_cart(9, 3);
        assert_eq!(one.products_in_cart(), many.products_in_cart());
    }

    #[test]
    fn the_costs_follow_upstreams_formulas() {
        let mut model = AppStateModel::loaded();
        // Vagabond sack, $120, twice; Weave keyring, $16, once.
        model.add_multiple_products_to_cart(0, 2);
        model.add_product_to_cart(6);
        let subtotal = 2.0 * 120.0 + 16.0;
        assert_eq!(model.subtotal_cost(), subtotal);
        assert_eq!(model.shipping_cost(), 3.0 * SHIPPING_COST_PER_ITEM);
        assert_eq!(model.tax(), subtotal * SALES_TAX_RATE);
        assert_eq!(
            model.total_cost(),
            subtotal + 3.0 * SHIPPING_COST_PER_ITEM + subtotal * SALES_TAX_RATE
        );
    }

    #[test]
    fn the_category_filter_selects() {
        let mut model = AppStateModel::loaded();
        assert_eq!(model.products().len(), 38);
        model.set_category(Some(Category::Clothing));
        let clothing = model.products();
        assert_eq!(clothing.len(), 19);
        assert!(clothing.iter().all(|p| p.category == Category::Clothing));
        model.set_category(None);
        assert_eq!(model.products().len(), 38);
    }

    #[test]
    fn clearing_the_cart_empties_it() {
        let mut model = AppStateModel::loaded();
        model.add_product_to_cart(1);
        model.add_product_to_cart(2);
        model.clear_cart();
        assert!(model.products_in_cart().is_empty());
        assert_eq!(model.total_cart_quantity(), 0);
        assert_eq!(model.total_cost(), 0.0);
    }

    #[test]
    fn products_come_back_by_id() {
        let model = AppStateModel::loaded();
        assert_eq!(model.product_by_id(0).name, "Vagabond sack");
        assert_eq!(model.product_by_id(37).name, "Fine lines tee");
    }
}
