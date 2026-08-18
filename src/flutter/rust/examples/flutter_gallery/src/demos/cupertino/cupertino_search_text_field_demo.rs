// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_search_text_field_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoSearchTextFieldDemo` is a `CupertinoSearchTextField`
//! over a filtered list of six platform names: the field's controller
//! listener keeps `_searchPlatform`, and `_buildPlatformList` filters
//! case-insensitively (`contains` on both sides lowered). Here the query is
//! the per-demo [`SearchTextFieldDemoState`]'s `query`, written from the
//! field's `onChanged` (which the clear button also drives, the way
//! upstream's `controller.clear()` re-fires the listener), and the list is
//! filtered at build.
//!
//! Divergences, each marked at its site:
//!
//! * The field's `decoration` (a bottom hairline in `inactiveGray`) and
//!   `padding` (`EdgeInsets.symmetric(horizontal: 6, vertical: 12)`) are not
//!   `CupertinoSearchTextField` parameters in the framework tier, so the
//!   padding wraps the field and the hairline is a one-pixel container below
//!   it.
//! * `restorationId: 'search_text_field'` has no counterpart (PORTING.md:
//!   restoration is not carried anywhere).
//! * The list is a fixed column: upstream's `ListView.builder(shrinkWrap:
//!   true)` over six rows never scrolls.
//! * The stage is height-bounded ([`DEMO_HEIGHT`]); upstream's `SafeArea`
//!   fills the demo screen.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// The stage's fixed height, standing in for the demo screen (see the header).
const DEMO_HEIGHT: f32 = 420.0;

/// Upstream's `platforms` list.
const PLATFORMS: [&str; 6] = ["Android", "iOS", "Windows", "Linux", "MacOS", "Web"];

/// The demo body for the `cupertino-search-text-field` slug. The Cupertino
/// theme the demo page provides upstream (`DemoWrapper`'s
/// `CupertinoTheme(brightness: light)`) is provided here; see the sibling
/// demos' headers.
pub(super) fn stage() -> AnyWidget {
    provide(
        CupertinoTheme::light(),
        single(stateful(SearchTextFieldDemo), move |inner| {
            Box::new(Container::new().with_height(DEMO_HEIGHT).with_child(inner))
        }),
    )
}

/// Upstream's `CupertinoSearchTextFieldDemo`.
struct SearchTextFieldDemo;

/// What the demo remembers: `_searchPlatform`. Upstream also keeps
/// `filteredPlatforms` as a field, but only ever as the filter's output
/// derived in `_buildPlatformList`, so here it is derived at build.
#[derive(Default)]
struct SearchTextFieldDemoState {
    query: String,
}

/// `_buildPlatformList`'s filter: case-insensitive containment, the full list
/// for an empty query.
fn filtered_platforms(query: &str) -> Vec<&'static str> {
    if query.is_empty() {
        return PLATFORMS.to_vec();
    }
    let needle = query.to_lowercase();
    PLATFORMS
        .iter()
        .copied()
        .filter(|platform| platform.to_lowercase().contains(&needle))
        .collect()
}

impl StatefulComponent for SearchTextFieldDemo {
    type State = SearchTextFieldDemoState;

    fn build(
        &self,
        state: &SearchTextFieldDemoState,
        handle: StateHandle<SearchTextFieldDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let l10n = GalleryLocalizations::en();

        // The field, with upstream's placeholder
        // (`demoCupertinoSearchTextFieldPlaceholder`). Upstream listens to its
        // controller; here `onChanged` is the same signal -- it fires for the
        // clear button too.
        let field = stateful(
            CupertinoSearchTextField::new(ids::DEMO_LOCAL + 70)
                .with_placeholder(l10n.demo_cupertino_search_text_field_placeholder())
                .wired(
                    handle,
                    |state, text| state.query = text.to_string(),
                    |state, text| state.query = text.to_string(),
                ),
        );

        // Upstream's `padding` and `decoration` (a bottom hairline), wrapped
        // around the field rather than passed to it (see the header).
        let hairline_color = theme.resolve(CupertinoColors::INACTIVE_GRAY);
        let field_block: AnyWidget = single(field, move |inner| {
            Box::new(
                Container::new()
                    .with_padding(EdgeInsets::symmetric(6.0, 12.0))
                    .with_child(inner),
            )
        });
        let hairline: AnyWidget =
            leaf(move || Container::new().with_height(1.0).with_color(hairline_color));

        // `_buildPlatformList`: one `ListTile(title:)` per surviving platform.
        // The framework's `ListTile` reads the ambient Material theme, which
        // the demo page sets to the always-light demo theme -- what upstream's
        // tiles read too.
        let tiles: Vec<AnyWidget> = filtered_platforms(&state.query)
            .into_iter()
            .map(|platform| component(ListTile::new(platform)))
            .collect();

        let mut children: Vec<AnyWidget> = vec![field_block, hairline];
        children.extend(tiles);
        let body = many(children, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(column)
        });

        component(
            CupertinoPageScaffold::new(body).with_navigation_bar(component(
                // `automaticallyImplyLeading: false`: no back button.
                CupertinoNavigationBar::new()
                    .with_middle(l10n.demo_cupertino_search_text_field_title()),
            )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_platforms_are_upstreams_list() {
        assert_eq!(
            PLATFORMS,
            ["Android", "iOS", "Windows", "Linux", "MacOS", "Web"]
        );
    }

    #[test]
    fn an_empty_query_lists_everything() {
        // The controller listener's empty-text branch: `filteredPlatforms =
        // platforms`.
        assert_eq!(filtered_platforms(""), PLATFORMS.to_vec());
    }

    #[test]
    fn the_filter_is_case_insensitive_containment() {
        // `_buildPlatformList`'s `toLowerCase().contains(...)` on both sides.
        assert_eq!(filtered_platforms("os"), vec!["iOS", "MacOS"]);
        assert_eq!(filtered_platforms("OS"), vec!["iOS", "MacOS"]);
        assert_eq!(
            filtered_platforms("i"),
            vec!["Android", "iOS", "Windows", "Linux"]
        );
        assert_eq!(filtered_platforms("n"), vec!["Android", "Windows", "Linux"]);
        assert_eq!(filtered_platforms("xyz"), Vec::<&str>::new());
    }

    #[test]
    fn the_field_writes_the_query_back() {
        // What the wired `onChanged` does to the state.
        let mut state = SearchTextFieldDemoState::default();
        state.query = "web".to_string();
        assert_eq!(filtered_platforms(&state.query), vec!["Web"]);
        state.query.clear();
        assert_eq!(filtered_platforms(&state.query).len(), PLATFORMS.len());
    }
}
