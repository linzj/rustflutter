// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/reference/two_pane_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream's single-pane fallback.
//!
//! Upstream's `TwoPaneDemo` wraps a `dual_screen` package `TwoPane` in a
//! `SimulateScreen` -- a device-shaped shell that injects a hinge
//! `DisplayFeature` into the `MediaQuery` for the foldable configuration --
//! and offers three configurations: foldable, tablet and small screen. The
//! demo is upstream's `DeferredWidget` showcase, so the catalogue here
//! excludes it (PORTING.md, "deferred loading is synchronous"): this module
//! is wired into `demos/reference` but no slug routes to it.
//!
//! What renders is the small-screen configuration, the single-pane
//! fallback: the list pane until an item is selected, then the details pane
//! with its close button (`TwoPanePriority.start` / `.end` on a screen too
//! small for both). The divergences, each also marked at its site:
//!
//! * The foldable configuration is unreachable: there is no hinge data in
//!   `MediaQueryData`, so `is_display_foldable` never fires (PORTING.md,
//!   "foldable is always false", `src/pages/adaptive_layout.rs`). No hinge
//!   is simulated either -- the `dual_screen` package and its `TwoPane` have
//!   no counterpart in the framework.
//! * The tablet configuration (both panes side by side at
//!   `paneProportion: 0.3`) is likewise not rendered: with one configuration
//!   per demo here (PORTING.md, "demo options section is unreachable"), the
//!   single-pane fallback is the one that shows what `TwoPane` decides.
//! * The restoration machinery (`RestorationMixin`, `RestorableInt
//!   _currentIndex`) has no counterpart, as in every demo port.
//! * `SimulateScreen`'s shell is drawn at a fixed height rather than
//!   centered in the demo screen, because the stage does not guarantee a
//!   bounded height.

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, AspectRatio, ListView, Pointer};

use crate::app::ids;
use crate::data::demos as catalog;
use crate::l10n::gallery_localizations::GalleryLocalizations;

/// Upstream's `SimulateScreen.singleScreenAspectRatio`, the 16x9 candy bar
/// phone.
const SINGLE_SCREEN_ASPECT_RATIO: f32 = 9.0 / 16.0;

/// The simulated screen's height. Upstream the shell is centered in the demo
/// screen and the aspect ratio sets its size; here the stage has no bounded
/// height, so the height is fixed and the aspect ratio sets the width.
const SIMULATED_HEIGHT: f32 = 560.0;

/// The shell's padding, upstream's `padding: const EdgeInsets.all(14)`.
const SHELL_PADDING: f32 = 14.0;

/// A pane's app-bar height, the framework `AppBar`'s.
const BAR_HEIGHT: f32 = 56.0;

/// The details pane's body fill, upstream's `color: const Color(0xfffafafa)`.
const DETAILS_FILL: Color = Color(0xFFFAFAFA);

/// The demo body. Unrouted today -- see the module header.
#[allow(dead_code)]
pub(super) fn stage() -> AnyWidget {
    stateful(TwoPaneDemo)
}

/// Upstream's `TwoPaneDemo` at `TwoPaneDemoType.smallScreen`.
struct TwoPaneDemo;

/// What the demo remembers: `_currentIndex` (-1 is nothing selected) and the
/// list pane's scroll position.
struct TwoPaneDemoState {
    selected: i32,
    scroll: Scroll,
}

impl Default for TwoPaneDemoState {
    fn default() -> TwoPaneDemoState {
        TwoPaneDemoState {
            selected: -1,
            scroll: Scroll::default(),
        }
    }
}

impl StatefulComponent for TwoPaneDemo {
    type State = TwoPaneDemoState;

    fn advance(&self, state: &mut TwoPaneDemoState, frame_time_micros: i64) -> bool {
        state.scroll.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &TwoPaneDemoState,
        handle: StateHandle<TwoPaneDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        // Upstream's `build`: on a small screen the two panes never share it
        // -- the list until an item is selected, the details after.
        let pane = if state.selected == -1 {
            list_pane(state, handle.clone(), context)
        } else {
            details_pane(state.selected, handle, context)
        };

        // Upstream's `SimulateScreen`: the black rounded shell around the
        // simulated screen. Without the hinge injection (see the header) it
        // is only a frame.
        let framed = single(pane, move |inner| {
            Box::new(AspectRatio::new(SINGLE_SCREEN_ASPECT_RATIO, inner))
        });
        single(framed, move |inner| {
            Box::new(
                Container::new()
                    .with_height(SIMULATED_HEIGHT)
                    .with_child(Align::new(
                        Alignment::CENTER,
                        Container::new()
                            .with_color(Color::BLACK)
                            .with_corner_radius(16.0)
                            .with_padding(EdgeInsets::all(SHELL_PADDING))
                            .with_child(inner),
                    )),
            )
        })
    }
}

/// A pane's app bar: primary fill, on-primary title, and -- for the details
/// pane -- the close button upstream shows when `onClose` is not null.
struct PaneBar {
    title: String,
    /// The details pane's `onClose`; the list pane has none.
    on_close: Option<StateHandle<TwoPaneDemoState>>,
}

impl Component for PaneBar {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let fill = theme.primary;
        let ink = theme.on_primary;
        let title = self.title.clone();

        let mut children: Vec<AnyWidget> = Vec::new();
        if let Some(handle) = self.on_close.clone() {
            // Upstream's `leading: IconButton(icon: const Icon(Icons.close))`.
            let tap = PointerHandlers::new().with_tap(move |_| {
                handle.set_state(|state| state.selected = -1);
            });
            children.push(leaf(move || {
                Pointer::new(
                    ids::DEMO_LOCAL + 2,
                    Container::new()
                        .with_size(48.0, BAR_HEIGHT)
                        .with_child(Align::new(
                            Alignment::CENTER,
                            Text::new(catalog::icon::CLOSE)
                                .with_font_family(catalog::MATERIAL_ICONS)
                                .with_size(24.0)
                                .with_color(ink),
                        )),
                )
                .with_handlers(tap.clone())
            }));
        }
        children.push(leaf(move || {
            Container::new()
                .with_padding(EdgeInsets::symmetric(16.0, 0.0))
                .with_child(
                    Text::new(title.clone())
                        .with_size(20.0)
                        .with_weight(500)
                        .with_color(ink),
                )
        }));

        many(children, move |rendered| {
            let mut bar = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center);
            for child in rendered {
                bar = bar.push(child);
            }
            Box::new(
                Container::new()
                    .with_height(BAR_HEIGHT)
                    .with_color(fill)
                    .with_child(bar),
            )
        })
    }
}

/// Upstream's `ListPane`: the "List" bar over the twenty items.
fn list_pane(
    state: &TwoPaneDemoState,
    handle: StateHandle<TwoPaneDemoState>,
    context: &mut BuildContext,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let bar = component(PaneBar {
        title: l10n.demo_two_pane_list().to_string(),
        on_close: None,
    });
    let theme = theme_of(context);
    let primary = theme.primary;
    let on_primary = theme.on_primary;
    let body = theme.body();
    let selected = state.selected;
    let offset = state.scroll.offset;
    let extent = state.scroll.extent.clone();

    // The list's drag wiring, the per-demo counterpart of
    // `app::scroll_handlers`: a finger down stops a fling, a drag moves the
    // content with the finger, letting go throws it, and the wheel walks it.
    let down_handle = handle.clone();
    let drag_handle = handle.clone();
    let end_handle = handle.clone();
    let wheel_handle = handle.clone();
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
    let tiles: Vec<AnyWidget> = (1..=20)
        .map(|index| {
            let is_selected = index == selected;
            let title = l10n.demo_two_pane_item(index);
            let tap = PointerHandlers::new().with_tap({
                let handle = handle.clone();
                move |_| {
                    handle.set_state(move |state| state.selected = index);
                }
            });
            let body = body.clone();
            leaf(move || {
                // The avatar: upstream's `CircleAvatar(child:
                // Text('$index'))`, a 40-wide circle on primary.
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
                Pointer::new(
                    ids::DEMO_LOCAL + 10 + index as u64,
                    Container::new()
                        // Upstream's `ListTile(selected: ...)`: the primary
                        // wash over the selected row.
                        .with_color(if is_selected {
                            primary.with_alpha(0x18)
                        } else {
                            Color::TRANSPARENT
                        })
                        .with_padding(EdgeInsets::symmetric(16.0, 4.0))
                        .with_child(
                            RenderFlex::row()
                                .with_main_axis_size(MainAxisSize::Max)
                                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                .with_spacing(16.0)
                                .push(avatar)
                                .push(Text::new(title.clone()).with_style(body.clone())),
                        ),
                )
                .with_handlers(tap.clone())
            })
        })
        .collect();

    let list = many(tiles, move |rendered| {
        let mut flex = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        // Upstream's `padding: const EdgeInsets.symmetric(vertical: 8)` as
        // end caps; `widgets::ListView` has no padding slot.
        flex = flex.push(Container::new().with_size(1.0, 8.0));
        for tile in rendered {
            flex = flex.push(tile);
        }
        flex = flex.push(Container::new().with_size(1.0, 8.0));
        let list = ListView::new()
            .with_offset(offset)
            .with_extent_sink(extent.clone())
            .push(flex);
        Box::new(Pointer::new(ids::DEMO_LOCAL + 1, list).with_handlers(handlers.clone()))
    });

    // The pane is bar over body; the body takes what the shell leaves it.
    let body_height = SIMULATED_HEIGHT - SHELL_PADDING * 2.0 - BAR_HEIGHT;
    many(vec![bar, list], move |mut rendered| {
        let list = rendered.pop().expect("two children");
        let bar = rendered.pop().expect("two children");
        Box::new(
            RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push(bar)
                .push(Container::new().with_height(body_height).with_child(list)),
        )
    })
}

/// Upstream's `DetailsPane`: the "Details" bar with its close button over
/// the selected item's details, on `0xfffafafa`.
fn details_pane(
    selected: i32,
    handle: StateHandle<TwoPaneDemoState>,
    context: &mut BuildContext,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let bar = component(PaneBar {
        title: l10n.demo_two_pane_details().to_string(),
        on_close: Some(handle),
    });
    let theme = theme_of(context);
    let body = theme.body();
    // Upstream: `selectedIndex == -1 ? demoTwoPaneSelectItem :
    // demoTwoPaneItemDetails(selectedIndex)`. The small-screen fallback only
    // shows this pane with a selection, but the empty case is upstream's
    // text either way.
    let text = if selected == -1 {
        l10n.demo_two_pane_select_item().to_string()
    } else {
        l10n.demo_two_pane_item_details(selected)
    };

    let details = leaf(move || {
        Container::new()
            .with_color(DETAILS_FILL)
            .with_child(Align::new(
                Alignment::CENTER,
                Text::new(text.clone()).with_style(body.clone()),
            ))
    });

    let body_height = SIMULATED_HEIGHT - SHELL_PADDING * 2.0 - BAR_HEIGHT;
    many(vec![bar, details], move |mut rendered| {
        let details = rendered.pop().expect("two children");
        let bar = rendered.pop().expect("two children");
        Box::new(
            RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push(bar)
                .push(
                    Container::new()
                        .with_height(body_height)
                        .with_child(details),
                ),
        )
    })
}
