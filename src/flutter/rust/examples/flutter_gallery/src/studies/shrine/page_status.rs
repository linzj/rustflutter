// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/shrine/page_status.dart` (flutter/gallery @
//! d12640d): the `PageStatus` inherited widget and the three page-visibility
//! questions the study asks of it.
//!
//! Upstream the widget publishes the two controllers so any descendant can
//! read their status. Here the controllers live in the study's root state
//! (`app.rs`'s `ShrineState`), and the questions are functions of them --
//! the inherited-widget plumbing would only move the same two references
//! through the tree.

use rustflutter::animation::Controller;

/// Upstream's `AnimationStatus.dismissed`: settled at 0.
fn is_dismissed(controller: &Controller) -> bool {
    !controller.is_running() && controller.value() <= 0.0
}

/// Upstream's `AnimationStatus.completed`: settled at 1.
fn is_completed(controller: &Controller) -> bool {
    !controller.is_running() && controller.value() >= 1.0
}

/// Upstream's `productPageIsVisible`.
pub fn product_page_is_visible(cart: &Controller, menu: &Controller, is_desktop: bool) -> bool {
    is_dismissed(cart) && (is_completed(menu) || is_desktop)
}

/// Upstream's `menuPageIsVisible`.
pub fn menu_page_is_visible(cart: &Controller, menu: &Controller, is_desktop: bool) -> bool {
    is_dismissed(cart) && (is_dismissed(menu) || is_desktop)
}

/// Upstream's `cartPageIsVisible`.
pub fn cart_page_is_visible(cart: &Controller) -> bool {
    is_completed(cart)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn settled(value: f32) -> Controller {
        let mut controller = Controller::new(Duration::from_millis(1));
        controller.set_value(value);
        controller
    }

    #[test]
    fn the_product_page_shows_when_the_cart_is_away_and_the_menu_is_closed() {
        let cart = settled(0.0);
        let menu_closed = settled(1.0);
        let menu_open = settled(0.0);
        assert!(product_page_is_visible(&cart, &menu_closed, false));
        assert!(!product_page_is_visible(&cart, &menu_open, false));
        // On desktop the menu and the products are both always there.
        assert!(product_page_is_visible(&cart, &menu_open, true));
    }

    #[test]
    fn the_menu_shows_when_the_cart_is_away_and_it_is_open() {
        let cart = settled(0.0);
        assert!(menu_page_is_visible(&cart, &settled(0.0), false));
        assert!(!menu_page_is_visible(&cart, &settled(1.0), false));
        assert!(menu_page_is_visible(&cart, &settled(1.0), true));
    }

    #[test]
    fn an_open_cart_hides_both_pages() {
        let cart = settled(1.0);
        assert!(cart_page_is_visible(&cart));
        assert!(!product_page_is_visible(&cart, &settled(1.0), false));
        assert!(!menu_page_is_visible(&cart, &settled(0.0), false));
        assert!(!cart_page_is_visible(&settled(0.0)));
    }

    #[test]
    fn mid_animation_neither_end_claims_the_page() {
        let mut cart = Controller::new(Duration::from_millis(500));
        cart.set_value(0.5);
        assert!(!cart_page_is_visible(&cart));
        assert!(!product_page_is_visible(&cart, &settled(1.0), false));
    }
}
