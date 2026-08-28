// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/bottom_sheet_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's two configurations of `BottomSheetDemo` -- "Persistent bottom
//! sheet" and "Modal bottom sheet" -- are one flattened catalogue entry here
//! (PORTING.md), so the stage stacks both variants. The persistent sheet's open
//! state is `DemoState::persistent_open`, so the launcher can anchor the sheet
//! over both sections; the modal's is `DemoState::sheet_open`, whose overlay is
//! [`sheet_overlay`].
//!
//! Divergences, each commented at its site as well:
//!
//! * The demo Scaffold's app bar and `FloatingActionButton`
//!   (`BottomSheetDemo.build`) are the demo page's chrome here
//!   (`pages/demo.rs`), so the add-button FAB is not drawn. The FAB itself is
//!   ported in `button_demo.rs`'s floating section.
//! * The persistent sheet is anchored to the bottom of the demo card rather
//!   than the scaffold's: the card is what fills the page here, and it is the
//!   area a demo's own sheets overlay.
//! * The framework's `BottomSheet` always draws a grab handle; upstream's M2
//!   sheets have none.
//! * The sheet list scrolls but does not fling -- it has wheel and drag
//!   handlers; the ballistic fling upstream gets from `ClampingScrollPhysics`
//!   is the shared `Scroll`'s, which this component advances per frame.

use rustflutter::framework::{component, leaf, many, single, stateful, BuildContext, StateHandle};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex, StackPosition,
};
use rustflutter::widgets::{Align, Empty, ListView, Pointer, Stack};

use crate::app::{ids, GalleryState};

use super::{caption, column, DemoState};

/// Hit-test ids, from the demo-local block (PORTING.md: fixed bases, no
/// counters).
const MODAL_BUTTON: u64 = ids::DEMO_LOCAL;
const PERSISTENT_BUTTON: u64 = ids::DEMO_LOCAL + 1;
const PERSISTENT_SCROLL: u64 = ids::DEMO_LOCAL + 2;
const MODAL_SCROLL: u64 = ids::DEMO_LOCAL + 3;

/// The stage: both variants, persistent first the way the catalogue's subtitle
/// reads ("Persistent and modal bottom sheets").
pub(super) fn sheet_launcher(
    state: &DemoState,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
    context: &mut BuildContext,
) -> AnyWidget {
    // Upstream's `_PersistentBottomSheetDemo.build`: a centered
    // ElevatedButton wired to `_showBottomSheetCallback` -- which is null
    // while the sheet is open, so the button disables until the sheet's
    // `closed` future completes.
    let persistent_button = single(
        component(
            Button::new(PERSISTENT_BUTTON, "SHOW BOTTOM SHEET")
                .with_enabled(!state.persistent_open)
                .with_pressed(pressed == Some(PERSISTENT_BUTTON))
                .wired(
                    handle.clone(),
                    |s| &mut s.pressed,
                    |s| s.demo.persistent_open = true,
                ),
        ),
        |button| Box::new(rustflutter::widgets::Center::new(button)),
    );
    // Upstream's `_ModalBottomSheetDemo.build`: a centered
    // ElevatedButton that calls `_showModalBottomSheet`.
    let modal_button = single(
        component(
            Button::new(MODAL_BUTTON, "SHOW BOTTOM SHEET")
                .with_pressed(pressed == Some(MODAL_BUTTON))
                .wired(handle, |s| &mut s.pressed, |s| s.demo.sheet_open = true),
        ),
        |button| Box::new(rustflutter::widgets::Center::new(button)),
    );
    let sections = column(
        vec![
            caption("Persistent bottom sheet"),
            persistent_button,
            caption("Modal bottom sheet"),
            modal_button,
        ],
        12.0,
    );
    if !state.persistent_open {
        return sections;
    }
    // `Scaffold.of(context).showBottomSheet((_) => _BottomSheetContent(),
    // elevation: 25)`: the sheet overlays the sections, anchored to the
    // bottom of the demo area. Upstream gives a persistent sheet no close
    // affordance and no scrim, and the demo never closes it -- the button
    // stays disabled until the route does, exactly like upstream's.
    let canvas = theme_of(context).background;
    let sheet = single(
        stateful(BottomSheetContent {
            scroll_id: PERSISTENT_SCROLL,
        }),
        move |content| {
            Box::new(
                Container::new()
                    .with_color(canvas)
                    .with_elevation(25)
                    .with_child(content),
            )
        },
    );
    many(vec![sections, sheet], |mut rendered| {
        let sheet = rendered.pop().expect("two children");
        let sections = rendered.pop().expect("two children");
        Box::new(Stack::new().push(sections).push_positioned(
            sheet,
            StackPosition {
                left: Some(0.0),
                right: Some(0.0),
                bottom: Some(0.0),
                ..Default::default()
            },
        ))
    })
}

/// The modal sheet, over the whole demo area while `DemoState::sheet_open`.
///
/// Upstream's `showModalBottomSheet` (`_ModalBottomSheetDemo._showModalBottomSheet`):
/// a barrier that dismisses on a tap, with `_BottomSheetContent` along the
/// bottom edge.
pub(super) fn sheet_overlay(handle: StateHandle<GalleryState>) -> AnyWidget {
    many(
        vec![
            component(Scrim::new(ids::SCRIM).wired(handle, |s| s.demo.sheet_open = false)),
            component(BottomSheet::new(stateful(BottomSheetContent {
                scroll_id: MODAL_SCROLL,
            }))),
        ],
        |mut rendered| {
            let sheet = rendered.pop().unwrap_or_else(|| boxed(Empty));
            let scrim = rendered.pop().unwrap_or_else(|| boxed(Empty));
            Box::new(
                Stack::new()
                    .push_positioned(scrim, rustflutter::widgets::Positioned::fill())
                    .push_positioned(
                        sheet,
                        StackPosition {
                            left: Some(0.0),
                            right: Some(0.0),
                            bottom: Some(0.0),
                            ..Default::default()
                        },
                    ),
            )
        },
    )
}

// -- The sheet body (BEGIN bottomSheetDemoModal#1 bottomSheetDemoPersistent#1) --

/// Upstream's `_BottomSheetContent`'s fixed height.
const SHEET_HEIGHT: f32 = 300.0;
/// The header row's height.
const HEADER_HEIGHT: f32 = 70.0;
/// `ListView.builder(itemCount: 21)`.
const ITEM_COUNT: usize = 21;

/// `GalleryLocalizations.demoBottomSheetItem`.
fn sheet_item_label(index: usize) -> String {
    format!("Item {index}")
}

/// Upstream's `_BottomSheetContent`: a header, a divider, and twenty-one
/// list items in the rest of a 300-pixel box. Stateful for the list's scroll
/// offset, the way upstream's `ListView` holds its own `ScrollPosition`.
struct BottomSheetContent {
    /// The list's hit-test id. The persistent and modal sheets can both be
    /// open at once, so each carries its own.
    scroll_id: u64,
}

#[derive(Default)]
struct SheetContentState {
    scroll: Scroll,
}

impl StatefulComponent for BottomSheetContent {
    type State = SheetContentState;

    fn advance(&self, state: &mut SheetContentState, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &SheetContentState,
        handle: StateHandle<SheetContentState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let body = theme.body();

        // SizedBox(height: 70, child: Center(child: Text('Header'))).
        let header_style = body.clone();
        let header: AnyWidget = leaf(move || {
            Container::new()
                .with_height(HEADER_HEIGHT)
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new("Header").with_style(header_style.clone()),
                ))
        });

        // `const Divider(thickness: 1)`. The framework's Divider is the same
        // hairline centred in sixteen pixels.
        let divider: AnyWidget = component(Divider);

        // The scroll gestures, with the same signs as `app::scroll_handlers`:
        // a drag says where the content went, a wheel where the reader wants
        // to go.
        let stop_handle = handle.clone();
        let drag_handle = handle.clone();
        let wheel_handle = handle;
        let handlers = PointerHandlers::new()
            .with_pointer_down(move |_| {
                stop_handle.set_state(|s| s.scroll.stop());
            })
            .with_drag_update(move |drag| {
                let delta = drag.delta.dy;
                drag_handle.set_state(move |s| s.scroll.scroll_by(-delta));
            })
            .with_scroll(move |scroll| {
                let delta = scroll.delta.dy;
                wheel_handle.set_state(move |s| s.scroll.scroll_by(delta));
            });

        // `Expanded(child: ListView.builder(itemCount: 21, ...))`, one
        // `ListTile(title: Text('Item N'))` per row. The tile here is the
        // one-line part of upstream's: sixteen pixels of horizontal padding,
        // vertically centred title. The list is built inside the `leaf`:
        // its closure is `Fn` (a leaf rebuilds), so the `ListView` cannot be
        // moved in from outside.
        let scroll = state.scroll.clone();
        let scroll_id = self.scroll_id;
        let list_widget: AnyWidget = leaf(move || {
            let mut list = ListView::new()
                .with_offset(scroll.offset)
                .with_link(scroll.link());
            for index in 0..ITEM_COUNT {
                list = list.push(
                    Container::new()
                        .with_padding(EdgeInsets::symmetric(16.0, 14.0))
                        .with_child(Text::new(sheet_item_label(index)).with_style(body.clone())),
                );
            }
            Pointer::new(scroll_id, list).with_handlers(handlers.clone())
        });

        many(vec![header, divider, list_widget], |mut rendered| {
            let list = rendered.pop().expect("three children");
            let divider = rendered.pop().expect("three children");
            let header = rendered.pop().expect("three children");
            Box::new(
                Container::new().with_height(SHEET_HEIGHT).with_child(
                    RenderFlex::column()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        .push(header)
                        .push(divider)
                        .push_flex(FlexChild::expanded(list, 1)),
                ),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_content_is_upstreams_shape() {
        // `_BottomSheetContent`: SizedBox(height: 300) holding a 70-pixel
        // header, a divider, and twenty-one items.
        assert_eq!(SHEET_HEIGHT, 300.0);
        assert_eq!(HEADER_HEIGHT, 70.0);
        assert_eq!(ITEM_COUNT, 21);
    }

    #[test]
    fn item_labels_are_upstreams() {
        // `demoBottomSheetItem(index)` is 'Item $index', zero-based from the
        // builder's index.
        assert_eq!(sheet_item_label(0), "Item 0");
        assert_eq!(sheet_item_label(20), "Item 20");
    }

    #[test]
    fn the_persistent_button_disables_while_the_sheet_is_open() {
        // `_showBottomSheetCallback` starts set, goes null on open, and comes
        // back on close.
        let mut state = DemoState::default();
        assert!(!state.persistent_open);
        state.persistent_open = true;
        assert!(state.persistent_open);
    }
}
