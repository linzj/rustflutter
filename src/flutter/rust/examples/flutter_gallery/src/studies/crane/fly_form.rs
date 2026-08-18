// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/fly_form.dart` (flutter/gallery @ d12640d):
//! `FlyForm`, the back layer for the FLY tab (index 0).
//!
//! The four `RestorableTextEditingController`s are the framework's per-field
//! `TextFieldState`s, owned by the fields themselves; there is no
//! `RestorationMixin` to port (PORTING.md: the M-D batch dropped it).

use rustflutter::framework::AnyWidget;

use super::backlayer::BackLayerItem;
use super::header_form::{field_icons, header_form, HeaderFormField};

/// Upstream's `FlyForm`.
pub struct FlyForm;

impl BackLayerItem for FlyForm {
    fn index(&self) -> usize {
        0
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
                icon: field_icons::PLACE,
                title: "Choose Origin",
            },
            HeaderFormField {
                index: 2,
                icon: field_icons::AIRPLANEMODE_ACTIVE,
                title: "Choose Destination",
            },
            HeaderFormField {
                index: 3,
                icon: field_icons::DATE_RANGE,
                title: "Select Dates",
            },
        ]
    }
}

/// The form, built. `first_id` is the hit-test identity of the first field.
pub fn form(first_id: u64, is_desktop: bool, is_small_desktop: bool) -> AnyWidget {
    header_form(&FlyForm.fields(), first_id, is_desktop, is_small_desktop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fields_match_upstream() {
        let form = FlyForm;
        assert_eq!(form.index(), 0);
        let titles: Vec<&str> = form.fields().iter().map(|field| field.title).collect();
        assert_eq!(
            titles,
            [
                "Travelers",
                "Choose Origin",
                "Choose Destination",
                "Select Dates"
            ]
        );
        let indexes: Vec<usize> = form.fields().iter().map(|field| field.index).collect();
        assert_eq!(indexes, [0, 1, 2, 3]);
    }
}
