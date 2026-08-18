// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/cupertino/cupertino_segmented_control_demo.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `CupertinoSegmentedControlDemo` shows the two segmented
//! controls sharing one value: a `CupertinoSegmentedControl` and a
//! `CupertinoSlidingSegmentedControl`, each over the three labels
//! `colorsIndigo`/`colorsTeal`/`colorsCyan`, with the selected label repeated
//! in a 300-high preview below. The selection is the per-demo
//! [`SegmentedControlDemoState`]'s `current`, upstream's `RestorableInt
//! currentSegment`.
//!
//! Divergences, each marked at its site:
//!
//! * `CupertinoSlidingSegmentedControl` is not part of the framework's
//!   Cupertino tier (rustflutter/src/cupertino.rs, `CupertinoSegmentedControl`
//!   docs), so the second control is drawn locally in its shape -- the
//!   `tertiarySystemFill` well, the white thumb under the selected segment --
//!   without the thumb's slide animation.
//! * The restoration machinery (`RestorationMixin`, restorationId
//!   'cupertino_segmented_control') has no counterpart; PORTING.md's
//!   standing rule.
//! * Upstream's `segmentedControlMaxWidth` (500) has no visible effect
//!   upstream either -- a `ListView`'s tight cross axis overrides the
//!   `SizedBox` -- so the controls simply take the stage's width.
//! * The body is a fixed column in a height-bounded stage
//!   ([`DEMO_HEIGHT`]): upstream's `ListView` never scrolls at 300 pixels of
//!   preview plus two controls, and the demo page's stage does not guarantee
//!   a bounded height.
//! * The first control's labels draw at the framework default size rather
//!   than upstream's 13pt `DefaultTextStyle`; the framework's
//!   `CupertinoSegmentedControl` styles segment text itself
//!   (rustflutter/src/cupertino.rs).

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, Pointer};

use crate::app::ids;
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// The stage's fixed height, standing in for the demo screen (see the header).
const DEMO_HEIGHT: f32 = 460.0;

/// The preview's height, upstream's `Container(height: 300)`.
const PREVIEW_HEIGHT: f32 = 300.0;

/// The body text's size, upstream's `DefaultTextStyle(fontSize: 13)`.
const BODY_SIZE: f32 = 13.0;

/// The demo body for the `cupertino-segmented-control` slug. The Cupertino
/// theme the demo page provides upstream (`DemoWrapper`'s
/// `CupertinoTheme(brightness: light)`) is provided here; see the sibling
/// demos' headers.
pub(super) fn stage() -> AnyWidget {
    provide(
        CupertinoTheme::light(),
        single(stateful(SegmentedControlDemo), move |inner| {
            Box::new(Container::new().with_height(DEMO_HEIGHT).with_child(inner))
        }),
    )
}

/// Upstream's `CupertinoSegmentedControlDemo`.
struct SegmentedControlDemo;

/// What the demo remembers: `currentSegment.value`.
#[derive(Default)]
struct SegmentedControlDemoState {
    current: usize,
}

/// The three segment labels: `colorsIndigo`, `colorsTeal`, `colorsCyan`.
fn labels() -> Vec<String> {
    let l10n = GalleryLocalizations::en();
    vec![
        l10n.colors_indigo().to_string(),
        l10n.colors_teal().to_string(),
        l10n.colors_cyan().to_string(),
    ]
}

impl StatefulComponent for SegmentedControlDemo {
    type State = SegmentedControlDemoState;

    fn build(
        &self,
        state: &SegmentedControlDemoState,
        handle: StateHandle<SegmentedControlDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = cupertino_theme_of(context);
        let l10n = GalleryLocalizations::en();
        let labels = labels();

        // The first control, upstream's `CupertinoSegmentedControl<int>`.
        let segmented = stateful(
            CupertinoSegmentedControl::new(ids::DEMO_LOCAL + 80, labels.clone(), state.current)
                .wired(handle.clone(), |state, index| state.current = index),
        );

        // The second, upstream's `CupertinoSlidingSegmentedControl<int>`,
        // drawn locally (see the header).
        let sliding =
            sliding_segmented_control(ids::DEMO_LOCAL + 90, &labels, state.current, &theme, handle);

        // The preview: `Container(padding: all(16), height: 300,
        // alignment: Alignment.center, child: children[currentSegment])`.
        let preview_text = labels[state.current.min(labels.len() - 1)].clone();
        let label_color = theme.resolve(CupertinoColors::LABEL);
        let preview: AnyWidget = leaf(move || {
            Container::new()
                .with_padding(EdgeInsets::all(16.0))
                .with_height(PREVIEW_HEIGHT)
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(preview_text.clone())
                        .with_size(BODY_SIZE)
                        .with_color(label_color),
                ))
        });

        // Upstream's `ListView`: `SizedBox(height: 16)`, the control, the
        // sliding control padded 16, the preview (see the header for the
        // fixed column).
        let body = many(vec![segmented, sliding, preview], move |rendered| {
            let mut rendered = rendered.into_iter();
            let segmented = rendered.next().expect("three children");
            let sliding = rendered.next().expect("three children");
            let preview = rendered.next().expect("three children");
            Box::new(
                RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .push(Container::new().with_size(1.0, 16.0))
                    .push(segmented)
                    .push(
                        Container::new()
                            .with_padding(EdgeInsets::all(16.0))
                            .with_child(sliding),
                    )
                    .push(preview),
            )
        });

        component(
            CupertinoPageScaffold::new(body).with_navigation_bar(component(
                // `automaticallyImplyLeading: false`: no back button.
                CupertinoNavigationBar::new()
                    .with_middle(l10n.demo_cupertino_segmented_control_title()),
            )),
        )
    }
}

/// The local stand-in for `CupertinoSlidingSegmentedControl`
/// (cupertino/sliding_segmented_control.dart, not in the framework tier): a
/// `tertiarySystemFill` well rounded 8, the segments in a row, the selected
/// one on a white thumb. The thumb's slide animation and its shadow's blur
/// are not carried.
fn sliding_segmented_control(
    first_id: u64,
    labels: &[String],
    selected: usize,
    theme: &CupertinoTheme,
    handle: StateHandle<SegmentedControlDemoState>,
) -> AnyWidget {
    let well = theme.resolve(CupertinoColors::TERTIARY_SYSTEM_FILL);
    let thumb = theme.resolve(CupertinoColors::WHITE.into());
    let label_color = theme.resolve(CupertinoColors::LABEL);
    let labels = labels.to_vec();

    leaf(move || {
        let mut row = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center);
        for (index, label) in labels.iter().enumerate() {
            let mut segment = Container::new().with_height(28.0).with_child(Center::new(
                Text::new(label.clone())
                    .with_size(BODY_SIZE)
                    .with_color(label_color)
                    .with_soft_wrap(false)
                    .with_max_lines(1),
            ));
            if index == selected {
                // The thumb: white, corners rounded just inside the well's.
                segment = segment.with_color(thumb).with_corner_radius(7.0);
            }
            // Upstream's `onValueChanged` does not fire for the
            // already-selected segment.
            let handlers = PointerHandlers::new().with_tap({
                let handle = handle.clone();
                move |_| {
                    if index != selected {
                        handle.set_state(move |state| state.current = index);
                    }
                }
            });
            row = row.push_flex(FlexChild::expanded(
                Pointer::new(first_id + index as u64, segment).with_handlers(handlers),
                1,
            ));
        }
        Container::new()
            .with_color(well)
            .with_corner_radius(8.0)
            .with_padding(EdgeInsets::all(2.0))
            .with_child(row)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_segments_are_upstreams_three_color_labels() {
        // `children: {0: Text(colorsIndigo), 1: Text(colorsTeal),
        // 2: Text(colorsCyan)}`.
        assert_eq!(labels(), vec!["INDIGO", "TEAL", "CYAN"]);
    }

    #[test]
    fn the_selection_defaults_to_the_first_segment() {
        // `RestorableInt currentSegment = RestorableInt(0)`.
        let state = SegmentedControlDemoState::default();
        assert_eq!(state.current, 0);
    }

    #[test]
    fn the_preview_reads_the_selected_label() {
        let labels = labels();
        let state = SegmentedControlDemoState { current: 2 };
        assert_eq!(labels[state.current.min(labels.len() - 1)], "CYAN");
        // An out-of-range value previews nothing new rather than panicking.
        let state = SegmentedControlDemoState { current: 7 };
        assert_eq!(labels[state.current.min(labels.len() - 1)], "CYAN");
    }
}
