// Copyright 2019 The Flutter team. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/studies/shrine/supplemental/layout_cache.dart` (flutter/
//! gallery @ d12640d): the `LayoutCache` inherited widget.
//!
//! Upstream publishes a `Map<String, List<List<int>>>` from the study root so
//! the desktop asymmetric view's balanced layout -- a search over column
//! assignments -- is computed once per (column count, products, widths)
//! rather than once per frame. Here the map lives in the study's root state
//! (`app.rs`'s `ShrineState`), behind a `RefCell` because builds read the
//! state by shared reference; `balanced_layout.rs` is the only user.

use std::cell::RefCell;
use std::collections::HashMap;

/// A memo table for balanced layouts. Upstream's keys are the encoded
/// parameters; the values are one list of product indices per column.
#[derive(Debug, Default)]
pub struct LayoutCache {
    layouts: RefCell<HashMap<String, Vec<Vec<usize>>>>,
}

impl LayoutCache {
    /// The cached layout for `key`, if it was computed before.
    pub fn get(&self, key: &str) -> Option<Vec<Vec<usize>>> {
        self.layouts.borrow().get(key).cloned()
    }

    /// Remembers `layout` under `key`.
    pub fn insert(&self, key: String, layout: Vec<Vec<usize>>) {
        self.layouts.borrow_mut().insert(key, layout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_goes_in_comes_back_out() {
        let cache = LayoutCache::default();
        assert!(cache.get("nothing").is_none());
        cache.insert("1;0,1".to_string(), vec![vec![0], vec![1]]);
        assert_eq!(cache.get("1;0,1"), Some(vec![vec![0], vec![1]]));
    }
}
