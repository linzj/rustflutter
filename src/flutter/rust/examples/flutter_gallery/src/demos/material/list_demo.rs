// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/list_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `ListDemo` has two configurations -- `ListDemoType.oneLine`
//! ("One Line") and `ListDemoType.twoLine` ("Two Lines"), picked through the
//! demo's options section. The catalogue here flattens every demo to one
//! configuration (PORTING.md's "demo options section is unreachable"), so the
//! stage shows both, one labelled section each, in upstream's configuration
//! order.
//!
//! Divergences, each also marked at its site:
//!
//! * `ListDemo.build`'s `Scaffold`/`AppBar` (title "Lists") is the demo
//!   page's own chrome now (`pages/demo.rs`); the stage starts at the body,
//!   the `Scrollbar` around the `ListView`.
//! * Each list is height-bounded ([`LIST_HEIGHT`]) rather than filling the
//!   screen, because two configurations share one stage.
//! * Upstream's `ListView(padding: EdgeInsets.symmetric(vertical: 8))` is
//!   two 8-pixel end caps here: the framework's `widgets::ListView` has no
//!   padding slot (only `SliverListView` does).

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, ListView, Pointer};

use crate::app::ids;

use super::{caption, column};

/// How tall each configuration's list is. Upstream the list fills the demo
/// screen; here two of them share one stage, so each gets a bounded window it
/// scrolls inside.
const LIST_HEIGHT: f32 = 280.0;

/// Upstream's `ListDemoType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ListDemoType {
    OneLine,
    TwoLine,
}

/// The demo body for the `lists` slug.
pub(super) fn lists() -> AnyWidget {
    column(
        vec![
            // Upstream's `demoOneLineListsTitle` / `demoTwoLineListsTitle`,
            // the two configuration titles.
            caption("One Line"),
            list_section(ListDemoType::OneLine),
            caption("Two Lines"),
            list_section(ListDemoType::TwoLine),
        ],
        8.0,
    )
}

/// One configuration: a `Scrollbar` around a bounded, scrollable `ListView`,
/// the body of upstream's `ListDemo.build`.
fn list_section(list_type: ListDemoType) -> AnyWidget {
    let id = match list_type {
        ListDemoType::OneLine => ids::DEMO_LOCAL,
        ListDemoType::TwoLine => ids::DEMO_LOCAL + 1,
    };
    // `Scrollbar`'s child is a builder because the list is rebuilt from it on
    // every frame; the scroll offset lives in the list's own state.
    let scrollable = scrollbar(move || stateful(DemoList { list_type, id }));
    single(scrollable, move |inner| {
        Box::new(Container::new().with_height(LIST_HEIGHT).with_child(inner))
    })
}

/// The scrollable list itself, with its own `Scroll` for a state -- the
/// per-demo `State` upstream's `Scrollable` carries inside its element.
struct DemoList {
    list_type: ListDemoType,
    id: u64,
}

#[derive(Default)]
struct DemoListState {
    scroll: Scroll,
}

impl StatefulComponent for DemoList {
    type State = DemoListState;

    fn advance(&self, state: &mut DemoListState, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &DemoListState,
        handle: StateHandle<DemoListState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        // Dispatched from this element so the `Scrollbar` above hears the
        // list's movements -- upstream's notification bubbling.
        state
            .scroll
            .set_notification_sink(context.notification_sink());
        let offset = state.scroll.offset;
        let extent = state.scroll.link();
        let id = self.id;

        // The same handlers `app::scroll_handlers` gives the page scrollables,
        // against this list's own `Scroll`: a finger down stops a fling, a
        // drag moves the content with the finger, letting go throws it, and
        // the wheel walks it.
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

        // Upstream: `for (int index = 1; index < 21; index++)`.
        let two_line = self.list_type == ListDemoType::TwoLine;
        let tiles: Vec<AnyWidget> = (1..=20)
            .map(|index| component(DemoTile { index, two_line }))
            .collect();

        many(tiles, move |rendered| {
            let mut flex = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
            // Upstream's `padding: const EdgeInsets.symmetric(vertical: 8)`
            // as end caps; `widgets::ListView` has no padding slot.
            flex = flex.push(Container::new().with_size(1.0, 8.0));
            for tile in rendered {
                flex = flex.push(tile);
            }
            flex = flex.push(Container::new().with_size(1.0, 8.0));
            let list = ListView::new()
                .with_offset(offset)
                .with_link(extent.clone())
                .push(flex);
            Box::new(Pointer::new(id, list).with_handlers(handlers.clone()))
        })
    }
}

/// One row of the list. Upstream's `ListTile` with a `CircleAvatar` leading
/// showing the index, the title `demoBottomSheetItem(index)` ("Item N") and,
/// for the two-line configuration, the subtitle `demoListsSecondary`
/// ("Secondary text").
struct DemoTile {
    index: i32,
    two_line: bool,
}

impl Component for DemoTile {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let index = self.index;
        let two_line = self.two_line;
        let primary = theme.primary;
        let on_primary = theme.on_primary;
        let body = theme.body();
        let muted = theme.muted();

        leaf(move || {
            // The avatar: upstream's `CircleAvatar(child: Text('$index'))`,
            // a 40-wide circle on the theme's primary color.
            let avatar = Container::new()
                .with_size(40.0, 40.0)
                .with_color(primary)
                .with_corner_radius(20.0)
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(format!("{index}"))
                        .with_size(16.0)
                        .with_color(on_primary),
                ));
            let mut texts = Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(2.0)
                .push(Text::new(format!("Item {index}")).with_style(body.clone()));
            if two_line {
                texts = texts.push(Text::new("Secondary text").with_style(muted.clone()));
            }
            Container::new()
                // Upstream `ListTile`'s default `contentPadding`,
                // `EdgeInsets.symmetric(horizontal: 16)`. Vertically the tile
                // is upstream's fixed heights: 56 one-line, 72 two-line -- the
                // 40-wide avatar plus 8 or 16 of vertical padding.
                .with_padding(EdgeInsets::symmetric(
                    16.0,
                    if two_line { 16.0 } else { 8.0 },
                ))
                .with_child(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(16.0)
                        .push(avatar)
                        .push(texts),
                )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox, Size};

    fn lay_out(widget: AnyWidget, width: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), widget));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::new(0.0, width, 0.0, f32::INFINITY))
    }

    #[test]
    fn a_two_line_tile_is_taller_than_a_one_line_tile() {
        // Upstream's fixed ListTile heights: 56 one-line, 72 two-line.
        let one_line = lay_out(
            component(DemoTile {
                index: 1,
                two_line: false,
            }),
            400.0,
        );
        let two_line = lay_out(
            component(DemoTile {
                index: 1,
                two_line: true,
            }),
            400.0,
        );
        assert_eq!(one_line.height, 56.0);
        assert_eq!(two_line.height, 72.0);
    }

    #[test]
    fn the_list_has_twenty_tiles_between_two_end_caps() {
        // Upstream's loop is `1..21`; the caps stand in for its vertical
        // padding. Laying the stage out is the count check that matters: a
        // missing tile shows up as a shorter window.
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), lists()));
        let mut root = tree.build_render_tree().expect("a root");
        let size = root.layout(BoxConstraints::loose(460.0, 2000.0));
        // Two captions, two 280-tall windows, three gaps of 8.
        assert_eq!(
            size.height,
            280.0 * 2.0 + 8.0 * 3.0 + 2.0 * caption_height(&mut tree)
        );
    }

    /// One caption laid out on its own, for the stage-height arithmetic.
    fn caption_height(tree: &mut ElementTree) -> f32 {
        tree.rebuild(provide(Theme::dark(), caption("One Line")));
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(460.0, 2000.0)).height
    }
}
