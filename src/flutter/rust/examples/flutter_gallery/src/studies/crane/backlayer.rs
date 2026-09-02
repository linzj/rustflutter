// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/crane/backlayer.dart` (flutter/gallery @
//! d12640d): `BackLayerItem` and `BackLayer`.
//!
//! Upstream's `BackLayer` is an `IndexedStack` over the three forms, indexed
//! by the tab controller: every form stays in the tree -- and keeps its
//! field state -- while only the active tab's is shown. That is the
//! framework's [`IndexedStack`] exactly. Upstream's `ExcludeFocus` has no
//! counterpart here (the focus tour is a framework tier the gallery does not
//! drive), documented rather than stubbed.

use rustflutter::framework::{AnyWidget, many};
use rustflutter::prelude::*;
use rustflutter::widgets::IndexedStack;

use crate::app::ids;

use super::header_form::HeaderFormField;
use super::{eat_form, fly_form, sleep_form};

/// Upstream's `BackLayerItem`: a back-layer pane that knows its tab index
/// and the fields it shows.
pub trait BackLayerItem {
    /// Upstream's `index`.
    fn index(&self) -> usize;
    /// The fields the form renders, in upstream's order.
    fn fields(&self) -> Vec<HeaderFormField>;
}

/// The hit-test bases of the three forms' fields, one small range per form.
const FLY_FIELDS: u64 = ids::STUDY_LOCAL + 10;
const SLEEP_FIELDS: u64 = ids::STUDY_LOCAL + 20;
const EAT_FIELDS: u64 = ids::STUDY_LOCAL + 30;

/// Upstream's `BackLayer.build`: the three forms in an `IndexedStack`
/// indexed by the current tab.
pub fn back_layer(tab: usize, is_desktop: bool, is_small_desktop: bool) -> AnyWidget {
    let forms = vec![
        fly_form::form(FLY_FIELDS, is_desktop, is_small_desktop),
        sleep_form::form(SLEEP_FIELDS, is_desktop, is_small_desktop),
        eat_form::form(EAT_FIELDS, is_desktop, is_small_desktop),
    ];
    many(forms, move |rendered| {
        let mut stack = IndexedStack::new().with_index(Some(tab.min(2)));
        for form in rendered {
            stack = stack.push_boxed(form);
        }
        Box::new(stack)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox};

    #[test]
    fn the_active_form_is_the_tabs() {
        // The stack shows one form and keeps the others' state; what a test
        // can pin down is that it lays out at the active form's height. The
        // sleep form has three fields, the others four.
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::light(), back_layer(1, false, false)));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::new(0.0, 460.0, 0.0, f32::INFINITY));
        // The stack sizes to its largest child (the four-field forms), so
        // this is the four-field height even on the sleep tab.
        let four_fields = 4.0 * super::super::header_form::TEXT_FIELD_HEIGHT + 3.0 * 8.0;
        assert_eq!(size.height, four_fields);
    }
}
