// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/eat_form.dart` (flutter/gallery @ d12640d):
//! `EatForm`, the back layer for the EAT tab (index 2).
//!
//! The controllers are the framework's per-field `TextFieldState`s; see
//! `fly_form.rs`.

use rustflutter::framework::AnyWidget;

use super::backlayer::BackLayerItem;
use super::header_form::{field_icons, header_form, HeaderFormField};

/// Upstream's `EatForm`.
pub struct EatForm;

impl BackLayerItem for EatForm {
    fn index(&self) -> usize {
        2
    }

    fn fields(&self) -> Vec<HeaderFormField> {
        vec![
            HeaderFormField {
                index: 0,
                icon: field_icons::PERSON,
                title: "Diners",
            },
            HeaderFormField {
                index: 1,
                icon: field_icons::DATE_RANGE,
                title: "Select Date",
            },
            HeaderFormField {
                index: 2,
                icon: field_icons::ACCESS_TIME,
                title: "Select Time",
            },
            HeaderFormField {
                index: 3,
                icon: field_icons::RESTAURANT_MENU,
                title: "Select Location",
            },
        ]
    }
}

/// The form, built. `first_id` is the hit-test identity of the first field.
pub fn form(first_id: u64, is_desktop: bool, is_small_desktop: bool) -> AnyWidget {
    header_form(&EatForm.fields(), first_id, is_desktop, is_small_desktop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fields_match_upstream() {
        let form = EatForm;
        assert_eq!(form.index(), 2);
        let titles: Vec<&str> = form.fields().iter().map(|field| field.title).collect();
        assert_eq!(
            titles,
            ["Diners", "Select Date", "Select Time", "Select Location"]
        );
    }
}
