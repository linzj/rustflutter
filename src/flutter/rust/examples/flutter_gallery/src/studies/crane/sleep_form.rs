// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/sleep_form.dart` (flutter/gallery @
//! d12640d): `SleepForm`, the back layer for the SLEEP tab (index 1).
//!
//! The controllers are the framework's per-field `TextFieldState`s; see
//! `fly_form.rs`.

use rustflutter::framework::AnyWidget;

use super::backlayer::BackLayerItem;
use super::header_form::{HeaderFormField, field_icons, header_form};

/// Upstream's `SleepForm`.
pub struct SleepForm;

impl BackLayerItem for SleepForm {
    fn index(&self) -> usize {
        1
    }

    fn fields(&self) -> Vec<HeaderFormField> {
        vec![
            HeaderFormField {
                index: 0,
                icon: field_icons::PERSON,
                title: "Travelers",
            },
            HeaderFormField {
                index: 1,
                icon: field_icons::DATE_RANGE,
                title: "Select Dates",
            },
            HeaderFormField {
                index: 2,
                icon: field_icons::HOTEL,
                title: "Select Location",
            },
        ]
    }
}

/// The form, built. `first_id` is the hit-test identity of the first field.
pub fn form(first_id: u64, is_desktop: bool, is_small_desktop: bool) -> AnyWidget {
    header_form(&SleepForm.fields(), first_id, is_desktop, is_small_desktop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fields_match_upstream() {
        let form = SleepForm;
        assert_eq!(form.index(), 1);
        let titles: Vec<&str> = form.fields().iter().map(|field| field.title).collect();
        assert_eq!(titles, ["Travelers", "Select Dates", "Select Location"]);
    }
}
