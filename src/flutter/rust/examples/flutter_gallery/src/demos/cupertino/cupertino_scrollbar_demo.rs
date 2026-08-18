// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_scrollbar_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoScrollbarDemo` is a `CupertinoPageScaffold` whose
//! body is a `CupertinoScrollbar` around a 120-item `ListView.builder` of
//! centered `Text('item $index')` rows. The same shape here: the framework's
//! `CupertinoScrollbar` around a scrollable list whose offset lives in the
//! per-demo [`ScrollbarDemoState`], the way upstream's `Scrollable` holds its
//! own position. The list dispatches its scroll notifications so the
//! scrollbar hears them -- upstream's notification bubbling.
//!
//! Divergences, each marked at its site:
//!
//! * Upstream customizes the thumb (`thickness: 6.0`,
//!   `thicknessWhileDragging: 10.0`, `radius: 34.0`, `radiusWhileDragging:
//!   Radius.zero`); the framework's `CupertinoScrollbar` takes only
//!   `thumbColor`, and the dragging pair has no counterpart at all
//!   (rustflutter/src/cupertino.rs's `CUPERTINO_SCROLLBAR_METRICS` docs), so
//!   the bar draws with the Cupertino defaults (3 / 1.5).
//! * The stage is height-bounded ([`DEMO_HEIGHT`]): upstream fills the demo
//!   screen; the demo page's stage does not guarantee a bounded height (the
//!   same choice `navigation_drawer.rs` makes).

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::widgets::{ListView, Pointer};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// The stage's fixed height, standing in for the demo screen (see the header).
const DEMO_HEIGHT: f32 = 420.0;

/// `ListView.builder(itemCount: 120)`.
const ITEM_COUNT: usize = 120;

/// The demo body for the `cupertino-scrollbar` slug. The Cupertino theme the
/// demo page provides upstream (`DemoWrapper`'s `CupertinoTheme(brightness:
/// light)`) is provided here; see the sibling demos' headers.
pub(super) fn stage() -> AnyWidget {
    provide(
        CupertinoTheme::light(),
        single(stateful(ScrollbarDemo), move |inner| {
            Box::new(Container::new().with_height(DEMO_HEIGHT).with_child(inner))
        }),
    )
}

/// Upstream's `CupertinoScrollbarDemo`, stateful for the list's scroll
/// position.
struct ScrollbarDemo;

#[derive(Default)]
struct ScrollbarDemoState {
    scroll: Scroll,
}

impl StatefulComponent for ScrollbarDemo {
    type State = ScrollbarDemoState;

    fn advance(&self, state: &mut ScrollbarDemoState, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &ScrollbarDemoState,
        handle: StateHandle<ScrollbarDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        // Dispatched from this element so the `CupertinoScrollbar` above
        // hears the list's movements -- upstream's notification bubbling.
        state
            .scroll
            .set_notification_sink(context.notification_sink());
        let theme = cupertino_theme_of(context);
        // The rows read `CupertinoTheme.of(context).textTheme.textStyle`
        // upstream: 17pt, -0.41 tracking, in the label color.
        let text_style = theme.text_style();

        // The same four handlers `app::scroll_handlers` gives the page
        // scrollables, against this list's own `Scroll`.
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

        // `CupertinoScrollbar`'s child is a builder because the list is
        // rebuilt from it on every frame; the scroll offset lives in the
        // demo's state.
        let scroll = state.scroll.clone();
        let build_list = move || {
            let handlers = handlers.clone();
            let scroll = scroll.clone();
            let text_style = text_style.clone();
            leaf(move || {
                let mut list = ListView::new()
                    .with_offset(scroll.offset)
                    .with_extent_sink(scroll.extent.clone());
                for index in 0..ITEM_COUNT {
                    // `Center(child: Text('item $index'))`, zero-based as the
                    // builder's index is.
                    list = list.push(Center::new(
                        Text::new(format!("item {index}")).with_style(text_style.clone()),
                    ));
                }
                Pointer::new(ids::DEMO_LOCAL + 60, list).with_handlers(handlers.clone())
            })
        };

        component(
            CupertinoPageScaffold::new(component(CupertinoScrollbar::new(build_list)))
                .with_navigation_bar(component(
                    // `automaticallyImplyLeading: false`: no back button.
                    CupertinoNavigationBar::new()
                        .with_middle(GalleryLocalizations::en().demo_cupertino_scrollbar_title()),
                )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox, Size};

    #[test]
    fn the_list_has_upstreams_120_items() {
        assert_eq!(ITEM_COUNT, 120);
        // The labels are the builder's `'item $index'`, zero-based.
        assert_eq!(format!("item {}", 0), "item 0");
        assert_eq!(format!("item {}", ITEM_COUNT - 1), "item 119");
    }

    #[test]
    fn the_stage_is_height_bounded() {
        let mut tree = ElementTree::new();
        tree.rebuild(stage());
        let mut root = tree.build_render_tree().expect("a root");
        let size: Size = root.layout(BoxConstraints::loose(400.0, 2000.0));
        assert_eq!(size.height, DEMO_HEIGHT);
    }
}
