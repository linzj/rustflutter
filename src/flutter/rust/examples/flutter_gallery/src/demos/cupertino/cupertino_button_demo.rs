// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_button_demo.dart` (flutter/
//! gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoButtonDemo` is stateless: a `CupertinoPageScaffold`
//! whose centred column holds a plain and a filled `CupertinoButton` with
//! empty `onPressed` callbacks, then the same pair disabled, with 16/30/16
//! gaps between the four. The port keeps the four as data ([`ROWS`]) and the
//! gap list as [`GAPS`].
//!
//! Divergences, each also marked at its site:
//!
//! - **the pressed fade is the demo's own state.** Upstream's buttons are
//!   stateless -- the widget owns its `_opacityAnimation`. The framework's
//!   [`CupertinoButton`] reads its pressed flag from the caller, so this demo
//!   keeps a one-field [`ButtonDemoState`] (`Option<u64>`, the held button's
//!   id) the way the material demos keep `GalleryState::pressed`.
//! - **the scaffold is a fixed height.** Upstream's `DemoWrapper` gives the
//!   demo the page's content height; the demo page here renders each stage in
//!   a scrolling column at its intrinsic height, so the scaffold gets
//!   [`DEMO_HEIGHT`] to stand in for the screen's remainder.

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::Center;

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// The height the scaffold stands in for; see the module header.
const DEMO_HEIGHT: f32 = 700.0;

/// The four buttons upstream's column holds, in order: enabled plain and
/// filled, then the same two disabled. `(filled, enabled)`.
const ROWS: [(bool, bool); 4] = [(false, true), (true, true), (false, false), (true, false)];

/// The `SizedBox` heights between the four buttons: 16, 30, 16.
const GAPS: [f32; 3] = [16.0, 30.0, 16.0];

/// The demo body for the `cupertino-buttons` slug.
pub(super) fn stage() -> AnyWidget {
    stateful(ButtonDemo)
}

/// Upstream's `CupertinoButtonDemo`; stateful here only for the pressed fade
/// (see the module header).
struct ButtonDemo;

/// Which button is held, if one is.
#[derive(Default)]
struct ButtonDemoState {
    pressed: Option<u64>,
}

impl StatefulComponent for ButtonDemo {
    type State = ButtonDemoState;

    fn build(
        &self,
        state: &ButtonDemoState,
        handle: StateHandle<ButtonDemoState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let l10n = GalleryLocalizations::en();

        let mut children: Vec<AnyWidget> = Vec::new();
        for (index, (filled, enabled)) in ROWS.iter().enumerate() {
            if index > 0 {
                let gap = GAPS[index - 1];
                children.push(leaf(move || Container::new().with_height(gap)));
            }
            let id = ids::DEMO_LOCAL + index as u64;
            // The label pair: `cupertinoButton` for the plain buttons,
            // `cupertinoButtonWithBackground` for the filled ones.
            let label = if *filled {
                l10n.cupertino_button_with_background()
            } else {
                l10n.cupertino_button()
            };
            let button = if *filled {
                CupertinoButton::filled(id, label)
            } else {
                CupertinoButton::new(id, label)
            };
            let button = button
                .with_enabled(*enabled)
                .with_pressed(state.pressed == Some(id));
            // Upstream's `onPressed: () {}` -- the empty action below. A
            // disabled button is not wired, as upstream's `onPressed: null`
            // is not either.
            let button = if *enabled {
                button.wired(handle.clone(), |state| &mut state.pressed, |_| {})
            } else {
                button
            };
            children.push(component(button));
        }

        let body = many(children, move |rendered| {
            // Upstream's `Column(mainAxisAlignment: MainAxisAlignment.center)`
            // inside `Center`: a shrink-wrapped column, centred both ways.
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            for child in rendered {
                column = column.push(child);
            }
            Box::new(Center::new(column))
        });

        let scaffold = component(
            CupertinoPageScaffold::new(body).with_navigation_bar(component(
                CupertinoNavigationBar::new().with_middle(l10n.demo_cupertino_buttons_title()),
            )),
        );
        // Upstream's `DemoWrapper` wraps every demo in a light
        // `CupertinoTheme` (`lib/pages/demo.dart`).
        provide(
            CupertinoTheme::light(),
            single(scaffold, |scaffold| {
                // The fixed height stands in for the content height; see the
                // module header.
                Box::new(
                    Container::new()
                        .with_height(DEMO_HEIGHT)
                        .with_child(scaffold),
                )
            }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::framework::ElementTree;
    use rustflutter::render::{BoxConstraints, RenderBox, Size};

    fn lay_out(widget: AnyWidget, width: f32) -> Size {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::new(0.0, width, 0.0, f32::INFINITY))
    }

    #[test]
    fn the_rows_are_upstreams_four_in_order() {
        // Enabled plain and filled, then both disabled.
        assert_eq!(
            ROWS,
            [(false, true), (true, true), (false, false), (true, false)]
        );
        assert_eq!(GAPS, [16.0, 30.0, 16.0]);
    }

    #[test]
    fn nothing_is_pressed_to_begin_with() {
        assert_eq!(ButtonDemoState::default().pressed, None);
    }

    #[test]
    fn the_stage_is_a_scaffold_at_the_stand_in_height() {
        let size = lay_out(stage(), 428.0);
        assert_eq!(size.height, DEMO_HEIGHT);
        assert_eq!(size.width, 428.0);
    }
}
