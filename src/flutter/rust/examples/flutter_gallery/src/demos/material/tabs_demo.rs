// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/tabs_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `TabsDemo` is one widget with two configurations,
//! `_TabsScrollableDemo` (twelve tabs, `isScrollable: true`) and
//! `_TabsNonScrollableDemo` (three, `isScrollable: false`). The catalogue here
//! flattens every demo to one configuration (PORTING.md, "demo options
//! section is unreachable"), so the stage shows both variants stacked, under
//! the configuration titles "Scrolling" and "Non-scrolling". Each variant's
//! `Scaffold`/`AppBar` is the demo page's chrome (`src/pages/demo.rs`).
//!
//! The state both variants keep upstream -- the `TabController`'s index, made
//! restorable as `tabIndex` -- is the per-demo [`TabsState`] here, for the
//! same reason upstream keeps it per widget: leaving and reopening the demo
//! resets it. What is not carried: the restoration machinery itself (there is
//! no `RestorationMixin` counterpart), and `TabBarView`'s swipe-between-pages
//! physics -- the body below each bar is the selected tab's label, centered,
//! which is all upstream puts in it.

use rustflutter::framework::{keyed_leaf, BuildContext};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::Alignment;
use rustflutter::widgets::{Align, ListView, Pointer};

use crate::app::{ids, GalleryState};

use super::{caption, column, DemoState};

/// The six colors both variants cycle, upstream's `colorsRed` ...
/// `colorsPurple` tab labels.
const COLORS: [&str; 6] = ["RED", "ORANGE", "GREEN", "BLUE", "INDIGO", "PURPLE"];

/// `_TabsScrollableDemo`'s twelve tabs: the six colors, twice.
const SCROLLABLE_TAB_COUNT: usize = 12;

/// The scrollable variant's tab labels.
fn scrollable_labels() -> Vec<&'static str> {
    (0..SCROLLABLE_TAB_COUNT)
        .map(|index| COLORS[index % COLORS.len()])
        .collect()
}

/// The non-scrollable variant's tab labels.
fn non_scrollable_labels() -> Vec<&'static str> {
    COLORS[..3].to_vec()
}

/// The selected tab's label, or nothing for an index out of range -- the
/// phase-2 body answered the same way, and an index the bar cannot produce
/// should draw nothing rather than the wrong tab.
fn selected_label(labels: &[&'static str], selected: usize) -> &'static str {
    labels.get(selected).copied().unwrap_or("")
}

/// The demo body: both variants, stacked.
pub(super) fn tabs(_state: &DemoState, _handle: StateHandle<GalleryState>) -> AnyWidget {
    stateful(TabsDemo)
}

/// Upstream's `TabsDemo`: the pair of demos behind the type switch, one
/// state object for the two of them the way upstream has one `State` per
/// variant.
struct TabsDemo;

/// What the two variants remember: each one's selected tab, and the
/// scrollable strip's position.
#[derive(Default)]
struct TabsState {
    /// `_TabsScrollableDemoState.tabIndex`.
    scrollable: usize,
    /// `__TabsNonScrollableDemoState.tabIndex`.
    non_scrollable: usize,
    /// The strip's own scroll position. Upstream this is the scrollable
    /// `TabBar`'s internal `Scrollable`; the framework's `TabBar` has no
    /// scrollable mode, so the strip below is built from a horizontal
    /// `ListView` and keeps its position here.
    scroll: Scroll,
}

impl StatefulComponent for TabsDemo {
    type State = TabsState;

    fn advance(&self, state: &mut TabsState, frame_time_micros: i64) -> bool {
        // A fling on the strip plays out on the frame clock.
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &TabsState,
        handle: StateHandle<TabsState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let scrollable = scrollable_labels();
        let non_scrollable = non_scrollable_labels();

        column(
            vec![
                caption("Scrolling"),
                scrollable_strip(state, handle.clone(), context, &scrollable),
                tab_panel(scrollable_selected(state, &scrollable), state.scrollable),
                caption("Non-scrolling"),
                component(
                    TabBar::new(
                        ids::DEMO_LOCAL + 24,
                        non_scrollable
                            .iter()
                            .map(|label| label.to_string())
                            .collect(),
                        state.non_scrollable,
                    )
                    .wired(handle, |s, index| s.non_scrollable = index),
                ),
                tab_panel(
                    selected_label(&non_scrollable, state.non_scrollable),
                    state.non_scrollable + 100,
                ),
            ],
            12.0,
        )
    }
}

fn scrollable_selected(state: &TabsState, labels: &[&'static str]) -> &'static str {
    selected_label(labels, state.scrollable)
}

/// The selected tab's body: upstream's `TabBarView` page, which is
/// `Center(child: Text(tab))`. Keyed by the tab so the element is replaced
/// rather than reused on a change -- the key is what a cross-fade would
/// animate between, as the shared `FadedPanel` documents.
fn tab_panel(label: &'static str, key: usize) -> AnyWidget {
    keyed_leaf(key as u64 + 1, move || {
        rustflutter::widgets::Container::new()
            .with_height(120.0)
            .with_child(Align::new(
                Alignment::CENTER,
                Text::new(label).with_size(14.0),
            ))
    })
}

/// The scrollable variant's tab strip.
///
/// Upstream this is `TabBar(isScrollable: true)`: tabs sized to their
/// labels, scrolled when they do not fit. The framework's `TabBar` lays every
/// tab out expanded to an equal share of the width -- twelve tabs in a demo
/// card would be thirty pixels apiece and the labels would overlap -- so the
/// strip is a horizontal `ListView` of label-sized tabs drawn the way
/// `TabBar` draws them: 46 high, the label in `primary` when selected and
/// muted otherwise, and a two-pixel indicator under each tab.
fn scrollable_strip(
    state: &TabsState,
    handle: StateHandle<TabsState>,
    context: &mut BuildContext,
    labels: &[&'static str],
) -> AnyWidget {
    let theme = theme_of(context);
    let selected = state.scrollable;
    let offset = state.scroll.offset;
    let extent = state.scroll.extent.clone();
    let primary = theme.primary;
    let muted = theme.text_muted;
    let outline = theme.outline;
    let size = theme.body_size;

    let tabs: Vec<AnyWidget> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let active = index == selected;
            let label = *label;
            let tap = PointerHandlers::new().with_tap({
                let handle = handle.clone();
                move |_| {
                    handle.set_state(move |s| s.scrollable = index);
                }
            });
            leaf(move || {
                // The indicator is a positioned layer at the tab's bottom
                // edge rather than a flex child: a flex inside the
                // horizontal viewport does not measure the bar reliably.
                let indicator = rustflutter::widgets::Container::new()
                    .with_height(2.0)
                    .with_color(if active {
                        primary
                    } else {
                        outline.with_alpha(0x30)
                    });
                Pointer::new(
                    ids::DEMO_LOCAL + index as u64,
                    rustflutter::widgets::Container::new()
                        .with_height(46.0)
                        .with_padding(EdgeInsets::symmetric(16.0, 0.0))
                        .with_child(
                            rustflutter::render::RenderStack::new()
                                .push(Align::new(
                                    Alignment::CENTER,
                                    Text::new(label)
                                        .with_size(size)
                                        .with_weight(if active { 700 } else { 500 })
                                        .with_color(if active { primary } else { muted }),
                                ))
                                .push_positioned(
                                    indicator,
                                    rustflutter::render::StackPosition {
                                        left: Some(0.0),
                                        right: Some(0.0),
                                        bottom: Some(0.0),
                                        ..Default::default()
                                    },
                                ),
                        ),
                )
                .with_handlers(tap.clone())
            })
        })
        .collect();

    let scroll_handlers = strip_scroll_handlers(handle);
    many(tabs, move |rendered| {
        let mut list = ListView::horizontal()
            .with_offset(offset)
            .with_extent_sink(extent.clone());
        for tab in rendered {
            list = list.push(tab);
        }
        Box::new(Pointer::new(ids::DEMO_LOCAL + 16, list).with_handlers(scroll_handlers.clone()))
    })
}

/// The strip's drag and wheel, the horizontal case of the gallery's page
/// scrolling (`app::scroll_handlers`, which is typed to `GalleryState` and so
/// cannot serve a per-demo state).
fn strip_scroll_handlers(handle: StateHandle<TabsState>) -> PointerHandlers {
    let down_handle = handle.clone();
    let drag_handle = handle.clone();
    let end_handle = handle.clone();
    let wheel_handle = handle;
    PointerHandlers::new()
        .with_pointer_down(move |_| {
            down_handle.set_state(|s| s.scroll.stop());
        })
        .with_drag_update(move |drag| {
            let delta = drag.delta.dx;
            drag_handle.set_state(move |s| s.scroll.scroll_by(-delta));
        })
        .with_drag_end(move |end| {
            let velocity = end.velocity.dx;
            end_handle.set_state(move |s| s.scroll.fling(-velocity));
        })
        // A wheel is a vertical delta even over a horizontal strip; upstream's
        // scrollables map it onto the scroll axis, so this does too.
        .with_scroll(move |scroll| {
            let along = scroll.delta.dy;
            wheel_handle.set_state(move |s| s.scroll.scroll_by(along));
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scrollable_variant_has_twelve_tabs_cycling_six_colors() {
        let labels = scrollable_labels();
        assert_eq!(labels.len(), SCROLLABLE_TAB_COUNT);
        assert_eq!(labels[..6], COLORS);
        assert_eq!(labels[6..], COLORS);
    }

    #[test]
    fn the_non_scrollable_variant_has_three_tabs() {
        assert_eq!(non_scrollable_labels(), ["RED", "ORANGE", "GREEN"]);
    }

    #[test]
    fn an_out_of_range_selection_has_no_body() {
        let labels = non_scrollable_labels();
        assert_eq!(selected_label(&labels, 1), "ORANGE");
        assert_eq!(selected_label(&labels, 3), "");
    }
}
