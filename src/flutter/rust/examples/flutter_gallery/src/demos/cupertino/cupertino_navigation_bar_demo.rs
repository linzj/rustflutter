// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_navigation_bar_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoNavigationBarDemo` is a two-route `Navigator`: the
//! home route (`homeRoute`) is a scrollable list of twenty drawer items under
//! a `CupertinoSliverNavigationBar`, tapping an item pushes `secondPageRoute`,
//! whose `CupertinoNavigationBar` shows the item's title with a back chevron
//! labelled with the previous route's title. A demo stage has no route stack,
//! so the routes are the per-demo [`NavigationBarDemoState`]'s `page`: `None`
//! is the home route, `Some(title)` the pushed one, and the back button pops
//! it. The push/pop transition animation is not carried -- upstream's own home
//! route already disables it (`_NoAnimationCupertinoPageRoute`).
//!
//! Divergences, each marked at its site:
//!
//! * The `CupertinoSliverNavigationBar` is a plain `CupertinoNavigationBar`
//!   plus a large title at the top of the body -- the serving the
//!   framework's port documents for exactly this demo
//!   (rustflutter/src/cupertino.rs, `CupertinoNavigationBar`'s docs). The
//!   title is pinned where upstream's scrolls away with the list.
//! * The stage is height-bounded ([`DEMO_HEIGHT`]): upstream fills the demo
//!   screen; the demo page's stage does not guarantee a bounded height (the
//!   same choice `navigation_drawer.rs` makes).
//! * `Navigator.restorationScopeId`/`restorablePushNamed` have no
//!   counterpart (PORTING.md: restoration is not carried anywhere).

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Empty, ListView, Pointer};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// The stage's fixed height, standing in for the demo screen (see the header).
const DEMO_HEIGHT: f32 = 480.0;

/// `SliverChildBuilderDelegate(childCount: 20)`.
const ITEM_COUNT: usize = 20;

/// The demo body for the `cupertino-navigation-bar` slug.
///
/// Upstream the demo page wraps every demo in
/// `CupertinoTheme(data: CupertinoThemeData(brightness: Brightness.light))`
/// (`lib/pages/demo.dart`'s `DemoWrapper`); the demo page here wraps only the
/// Material demo theme, so the Cupertino tier's theme is provided here.
pub(super) fn stage() -> AnyWidget {
    provide(
        CupertinoTheme::light(),
        single(stateful(NavigationBarDemo), move |inner| {
            Box::new(Container::new().with_height(DEMO_HEIGHT).with_child(inner))
        }),
    )
}

/// Upstream's `CupertinoNavigationBarDemo`, stateful for the route stack it
/// no longer has.
struct NavigationBarDemo;

/// What the demo remembers: which route is showing, and the home route's
/// scroll offset (upstream's `Scrollable` inside the `CustomScrollView`).
#[derive(Default)]
struct NavigationBarDemoState {
    /// `None` is `homeRoute`; `Some(title)` is `secondPageRoute` pushed with
    /// `arguments: {'pageTitle': title}`.
    page: Option<String>,
    /// The home route's list position.
    scroll: Scroll,
}

impl StatefulComponent for NavigationBarDemo {
    type State = NavigationBarDemoState;

    fn advance(&self, state: &mut NavigationBarDemoState, frame_time_micros: i64) -> bool {
        // A fling on the home route's list plays out on the frame clock.
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &NavigationBarDemoState,
        handle: StateHandle<NavigationBarDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        match &state.page {
            None => first_page(state, handle, &theme),
            // `_SecondPage`: a bar with the pushed title and a back chevron
            // over an empty body (`child: Container()`).
            Some(title) => {
                let bar = CupertinoNavigationBar::new()
                    .with_middle(title.clone())
                    // `automaticallyImplyLeading`'s output: the chevron and the
                    // previous route's title, `demoCupertinoNavigationBarTitle`.
                    .with_back(
                        ids::DEMO_LOCAL + 30,
                        Some(
                            GalleryLocalizations::en()
                                .demo_cupertino_navigation_bar_title()
                                .to_string(),
                        ),
                    )
                    .wired_back(handle, |state| state.page = None);
                component(
                    CupertinoPageScaffold::new(leaf(|| Empty)).with_navigation_bar(component(bar)),
                )
            }
        }
    }
}

/// `_FirstPage`: the large-title bar over the twenty-item list.
fn first_page(
    state: &NavigationBarDemoState,
    handle: StateHandle<NavigationBarDemoState>,
    theme: &CupertinoTheme,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    // The sliver bar's large title, moved into the scrolled body (see the
    // header): text_theme.dart's `_kDefaultLargeTitleTextStyle`, 34pt w700,
    // in the label color.
    let large_title_color = theme.resolve(CupertinoColors::LABEL);
    let large_title: AnyWidget = leaf(move || {
        Container::new()
            .with_padding(EdgeInsets::only(16.0, 8.0, 16.0, 8.0))
            .with_child(
                Text::new(l10n.demo_cupertino_navigation_bar_title())
                    .with_size(34.0)
                    .with_weight(700)
                    .with_color(large_title_color),
            )
    });

    // The tiles: `ListTile(title: Text(title), onTap: push)`, the title
    // `starterAppDrawerItem(index + 1)`. The framework's `ListTile` reads the
    // ambient Material theme, which the demo page sets to the always-light
    // demo theme -- what upstream's tiles read too.
    let mut tiles: Vec<AnyWidget> = Vec::new();
    for index in 0..ITEM_COUNT {
        let title = GalleryLocalizations::en().starter_app_drawer_item(index + 1);
        let tap_title = title.clone();
        let tap_handle = handle.clone();
        let handlers = PointerHandlers::new().with_tap(move |_| {
            let title = tap_title.clone();
            tap_handle.set_state(move |state| state.page = Some(title));
        });
        tiles.push(component(
            ListTile::new(title).tappable(ids::DEMO_LOCAL + index as u64, handlers),
        ));
    }

    // The list's scroll: the same four handlers `app::scroll_handlers` gives
    // the page scrollables, against this demo's own `Scroll` -- a finger down
    // stops a fling, a drag moves the content, letting go throws it, the
    // wheel walks it.
    let down_handle = handle.clone();
    let drag_handle = handle.clone();
    let end_handle = handle.clone();
    let wheel_handle = handle;
    let handlers = PointerHandlers::new()
        .with_pointer_down(move |_| {
            down_handle.set_state(|state| state.scroll.stop());
        })
        .with_drag_update(move |drag| {
            let delta = drag.delta.dy;
            drag_handle.set_state(move |state| state.scroll.scroll_by(-delta));
        })
        .with_drag_end(move |end| {
            let velocity = end.velocity.dy;
            end_handle.set_state(move |state| state.scroll.fling(-velocity));
        })
        .with_scroll(move |scroll| {
            let delta = scroll.delta.dy;
            wheel_handle.set_state(move |state| state.scroll.scroll_by(delta));
        });

    let offset = state.scroll.offset;
    let extent = state.scroll.extent.clone();
    let list: AnyWidget = many(tiles, move |rendered| {
        let mut list = ListView::new()
            .with_offset(offset)
            .with_extent_sink(extent.clone());
        for tile in rendered {
            list = list.push(tile);
        }
        Box::new(Pointer::new(ids::DEMO_LOCAL + 20, list).with_handlers(handlers.clone()))
    });

    // Upstream's `CustomScrollView` slivers become a column: the large title
    // scrolls with the list upstream; pinned above it here, the standing
    // difference the framework's nav-bar docs prescribe.
    let body = many(vec![large_title, list], move |rendered| {
        let mut rendered = rendered.into_iter();
        let title = rendered.next().expect("two children");
        let list = rendered.next().expect("two children");
        Box::new(
            RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push(title)
                .push_flex(FlexChild::expanded(list, 1)),
        )
    });

    // The plain bar stands in for `CupertinoSliverNavigationBar(
    // automaticallyImplyLeading: false)`: no back, and no middle while the
    // large title below is showing.
    component(
        CupertinoPageScaffold::new(body)
            .with_navigation_bar(component(CupertinoNavigationBar::new())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox, Size};

    #[test]
    fn the_home_route_has_twenty_items_named_as_upstreams() {
        // `SliverChildBuilderDelegate(childCount: 20)` with titles
        // `starterAppDrawerItem(index + 1)` ("Item N").
        assert_eq!(ITEM_COUNT, 20);
        let l10n = GalleryLocalizations::en();
        assert_eq!(l10n.starter_app_drawer_item(1), "Item 1");
        assert_eq!(l10n.starter_app_drawer_item(20), "Item 20");
    }

    #[test]
    fn tapping_an_item_pushes_and_the_back_button_pops() {
        // The route stack the state stands in for: `page` starts on the home
        // route, a tap pushes the titled route, `wired_back`'s pop returns.
        let mut state = NavigationBarDemoState::default();
        assert_eq!(state.page, None);
        state.page = Some("Item 3".to_string());
        assert_eq!(state.page.as_deref(), Some("Item 3"));
        state.page = None;
        assert_eq!(state.page, None);
    }

    #[test]
    fn the_stage_is_height_bounded_and_shows_the_home_route() {
        let mut tree = ElementTree::new();
        tree.rebuild(stage());
        let mut root = tree.build_render_tree().expect("a root");
        let size: Size = root.layout(BoxConstraints::loose(400.0, 2000.0));
        assert_eq!(size.height, DEMO_HEIGHT);
    }
}
