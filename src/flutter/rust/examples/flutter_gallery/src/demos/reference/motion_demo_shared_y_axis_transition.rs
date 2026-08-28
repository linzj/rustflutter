// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/reference/motion_demo_shared_y_axis_transition.dart`
//! (flutter/gallery @ d12640d), aligned with upstream.
//!
//! Upstream's `SharedYAxisTransitionDemo` is "268 albums" sorted two ways:
//! the recent list ("Album 1".."Album 10") and the alphabetical list ("Album
//! A".."Album J") traded through a `PageTransitionSwitcher` running a
//! vertical `SharedAxisTransition`, driven by the "Recently played"/"A-Z"
//! sort toggle whose arrow rotates a half-turn per flip on a 300ms
//! controller (`reset` + `animateTo(0.5)`, then `animateTo(1)`). The
//! transition is reproduced here by [`transitions::shared_axis_enter`] and
//! [`transitions::shared_axis_exit`].
//!
//! Divergences, each also marked at its site:
//!
//! * The demo is one of six sections stacked on the single `motion` stage
//!   (see `mod.rs`'s header), so the lists scroll inside a bounded window
//!   ([`LIST_HEIGHT`]) rather than filling a screen.
//! * `_AlbumTile`'s trailing duration is stable here. Upstream rolls
//!   `Random().nextInt(50) + 10` in `build`, so the minutes reshuffle on
//!   every frame; [`minute_for`] deals them once, deterministically.
//! * The sort toggle is a plain tappable row rather than an `InkWell` with a
//!   rounded border: a custom ink shape has no counterpart, and all the
//!   border would do is clip a splash this framework does not draw.

use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, FlexChild, MainAxisAlignment, MainAxisSize, RenderFlex,
};
use rustflutter::widgets::{
    Align, Empty, FullWidth, ImageView, Opacity, Pointer, Stack, Transform,
};

use crate::app::ids;
use crate::data::demos::{icon, MATERIAL_ICONS};
use crate::l10n::gallery_localizations::GalleryLocalizations;

use super::{
    screen_column,
    transitions::{self, SharedAxis},
};

/// The hit-test ids this section's controls take from.
const ID_BASE: u64 = ids::DEMO_LOCAL + 1200;

/// The switcher's default duration and the arrow controller's, both 300ms.
const TRANSITION_MICROS: i64 = 300_000;

/// The height the album lists scroll inside; see the module header.
const LIST_HEIGHT: f32 = 380.0;

/// The album-art grey, upstream's `Colors.grey`.
const ART_GREY: Color = Color(0xFF9E9E9E);

/// The tile art, upstream's `placeholders/placeholder_image.png`.
const PLACEHOLDER_IMAGE: &[u8] =
    include_bytes!("../../../assets/placeholders/placeholder_image.png");
const PLACEHOLDER_CACHE_KEY: &str = "placeholders/placeholder_image.png";

/// Upstream's `_alphabet`.
const ALPHABET: [&str; 10] = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J"];

/// The recent list's titles, upstream's `(i + 1).toString()`.
const RECENT: [&str; 10] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"];

/// The tile at `index`'s minutes, dealt once (see the module header).
/// Upstream's roll is `Random().nextInt(50) + 10`; this is a fixed-seed
/// linear congruential deal into the same range.
fn minute_for(index: usize) -> u32 {
    // Knuth's LCG constants, stepped from a fixed seed per tile.
    let mut next = 0x2C6A_u32.wrapping_add(index as u32);
    next = next.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    next = next.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (next >> 16) % 50 + 10
}

/// The demo's section: upstream's `SharedYAxisTransitionDemo`.
pub(super) fn section() -> AnyWidget {
    stateful(SharedYAxisTransitionDemo)
}

struct SharedYAxisTransitionDemo;

/// Upstream's `_SharedYAxisTransitionDemoState`: the sort order, the arrow's
/// controller and the page switcher's clock. The lists' scroll positions are
/// kept per list, as upstream's keyed `ListView`s do.
struct SharedYAxisDemoState {
    /// Upstream's `_isAlphabetical`.
    alphabetical: bool,
    /// The page transition's progress, 0..1 over [`TRANSITION_MICROS`].
    progress: f32,
    running: bool,
    /// The arrow controller's value: 0 at rest, turning half a revolution
    /// per sort flip toward its target (0.5, then 1.0).
    arrow: f32,
    arrow_target: f32,
    recent_scroll: Scroll,
    alphabetical_scroll: Scroll,
    last_frame_micros: Option<i64>,
}

impl Default for SharedYAxisDemoState {
    fn default() -> Self {
        SharedYAxisDemoState {
            alphabetical: false,
            progress: 0.0,
            running: false,
            arrow: 0.0,
            arrow_target: 0.0,
            recent_scroll: Scroll::default(),
            alphabetical_scroll: Scroll::default(),
            last_frame_micros: None,
        }
    }
}

/// The sort toggle's tap: upstream flips the order, restarts the arrow
/// toward 0.5 on the first flip and on to 1.0 on the second, and the
/// switcher trades the lists (its `reverse: _isAlphabetical` mirrors the
/// slide for the alphabetical page).
fn toggle_sort(state: &mut SharedYAxisDemoState) {
    // Upstream: `if (!_isAlphabetical) { reset(); animateTo(0.5); } else
    // { animateTo(1); }` -- the reset drops the arrow back to 0 before the
    // first half-turn.
    if !state.alphabetical {
        state.arrow = 0.0;
        state.arrow_target = 0.5;
    } else {
        state.arrow_target = 1.0;
    }
    state.alphabetical = !state.alphabetical;
    state.progress = 0.0;
    state.running = true;
}

/// One frame's move of the arrow toward its target, at the controller's
/// 300ms-per-turn rate. Returns whether it is still moving.
fn tick_arrow(value: &mut f32, target: f32, elapsed_micros: i64) -> bool {
    let step = elapsed_micros as f32 / TRANSITION_MICROS as f32;
    if *value < target {
        *value = (*value + step).min(target);
    } else if *value > target {
        *value = (*value - step).max(target);
    }
    *value != target
}

impl StatefulComponent for SharedYAxisTransitionDemo {
    type State = SharedYAxisDemoState;

    fn advance(&self, state: &mut SharedYAxisDemoState, frame_time_micros: i64) -> bool {
        let elapsed = match state.last_frame_micros.replace(frame_time_micros) {
            Some(previous) => (frame_time_micros - previous).clamp(0, crate::app::MAX_FRAME_MICROS),
            None => 0,
        };
        let mut active = tick_arrow(&mut state.arrow, state.arrow_target, elapsed);
        if state.running {
            state.progress = (state.progress + elapsed as f32 / TRANSITION_MICROS as f32).min(1.0);
            if state.progress >= 1.0 {
                state.running = false;
            }
            active = true;
        }
        // The lists' flings play out on the same clock.
        active |= state.recent_scroll.advance(frame_time_micros);
        active |= state.alphabetical_scroll.advance(frame_time_micros);
        active
    }

    fn build(
        &self,
        state: &SharedYAxisDemoState,
        handle: StateHandle<SharedYAxisDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let l10n = GalleryLocalizations::en();
        let theme = theme_of(context);
        let canvas = theme.background;
        let text = theme.text;

        let app_bar = component(
            AppBar::new(l10n.demo_shared_y_axis_title())
                .with_subtitle(format!("({})", l10n.demo_shared_y_axis_demo_instructions())),
        );

        // The header row: the album count on the left, the sort toggle on
        // the right with its rotating arrow.
        let sort_label = if state.alphabetical {
            l10n.demo_shared_y_axis_alphabetical_sort_title()
        } else {
            l10n.demo_shared_y_axis_recent_sort_title()
        };
        let turns = state.arrow;
        let toggle = leaf({
            let handle = handle.clone();
            move || {
                let toggle_handle = handle.clone();
                Pointer::new(
                    ID_BASE,
                    Container::new()
                        .with_padding(EdgeInsets::symmetric(7.0, 4.0))
                        .with_child(
                            RenderFlex::row()
                                .with_main_axis_size(MainAxisSize::Min)
                                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                .with_spacing(2.0)
                                .push(Text::new(sort_label).with_size(14.0).with_color(text))
                                // Upstream's `RotationTransition(turns: 0..1)` over
                                // the controller: a full turn across both flips.
                                .push(Transform::rotate(
                                    turns * 360.0,
                                    Text::new(icon::ARROW_DROP_DOWN)
                                        .with_font_family(MATERIAL_ICONS)
                                        .with_size(24.0)
                                        .with_color(text),
                                )),
                        ),
                )
                .with_handlers(PointerHandlers::new().with_tap(move |_| {
                    toggle_handle.set_state(toggle_sort);
                }))
            }
        });
        let header = many(vec![toggle], move |rendered| {
            let toggle = rendered.into_iter().next().unwrap_or_else(|| boxed(Empty));
            Box::new(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .push(
                        Container::new()
                            .with_padding(EdgeInsets::only(15.0, 0.0, 0.0, 0.0))
                            .with_child(
                                Text::new(l10n.demo_shared_y_axis_album_count()).with_size(14.0),
                            ),
                    )
                    .push(toggle),
            )
        });

        // The lists, traded by the vertical shared-axis transition. The
        // switcher's `reverse: _isAlphabetical` mirrors the slide.
        let reverse = state.alphabetical;
        let body = if state.running {
            let enter =
                transitions::shared_axis_enter(state.progress, SharedAxis::Vertical, reverse);
            let exit = transitions::shared_axis_exit(state.progress, SharedAxis::Vertical, reverse);
            let arriving = album_list(state, &handle, state.alphabetical, ID_BASE + 1);
            let leaving = album_list(state, &handle, !state.alphabetical, ID_BASE + 40);
            many(vec![leaving, arriving], move |mut rendered| {
                let arriving = rendered.pop().unwrap_or_else(|| boxed(Empty));
                let leaving = rendered.pop().unwrap_or_else(|| boxed(Empty));
                Box::new(
                    Stack::new()
                        .push(Opacity::new(
                            exit.opacity,
                            Transform::matrix([1.0, 0.0, 0.0, 1.0, 0.0, exit.dy], leaving),
                        ))
                        .push(Opacity::new(
                            enter.opacity,
                            Transform::matrix([1.0, 0.0, 0.0, 1.0, 0.0, enter.dy], arriving),
                        )),
                )
            })
        } else {
            album_list(state, &handle, state.alphabetical, ID_BASE + 1)
        };
        let body = single(body, move |inner| {
            Box::new(
                Container::new()
                    .with_height(LIST_HEIGHT)
                    .with_color(canvas)
                    .with_child(inner),
            )
        });

        screen_column(vec![
            app_bar,
            leaf(|| Container::new().with_height(5.0)),
            header,
            leaf(|| Container::new().with_height(10.0)),
            body,
        ])
    }
}

/// One of the two lists: the recent one or the alphabetical one, scrolling
/// inside the window. Upstream keys them `UniqueKey()` so a switch rebuilds;
/// here the scroll positions are the state's, and a switch starts the other
/// list where it was left.
fn album_list(
    state: &SharedYAxisDemoState,
    handle: &StateHandle<SharedYAxisDemoState>,
    alphabetical: bool,
    id: u64,
) -> AnyWidget {
    let titles: &[&str] = if alphabetical { &ALPHABET } else { &RECENT };
    let (offset, extent) = if alphabetical {
        (
            state.alphabetical_scroll.offset,
            state.alphabetical_scroll.link(),
        )
    } else {
        (state.recent_scroll.offset, state.recent_scroll.link())
    };

    let down_handle = handle.clone();
    let drag_handle = handle.clone();
    let end_handle = handle.clone();
    let wheel_handle = handle.clone();
    let handlers = PointerHandlers::new()
        .with_pointer_down(move |_| {
            down_handle.set_state(move |s| scroll_of(s, alphabetical).stop());
        })
        .with_drag_update(move |drag| {
            let delta = drag.delta.dy;
            drag_handle.set_state(move |s| scroll_of(s, alphabetical).scroll_by(-delta));
        })
        .with_drag_end(move |end| {
            let velocity = end.velocity.dy;
            end_handle.set_state(move |s| scroll_of(s, alphabetical).fling(-velocity));
        })
        .with_scroll(move |scroll| {
            let delta = scroll.delta.dy;
            wheel_handle.set_state(move |s| scroll_of(s, alphabetical).scroll_by(delta));
        });

    let tiles: Vec<AnyWidget> = (0..titles.len())
        .map(|index| album_tile(titles[index], index))
        .collect();
    many(tiles, move |rendered| {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch);
        for tile in rendered {
            column = column.push(tile);
        }
        let list = rustflutter::widgets::ListView::new()
            .with_offset(offset)
            .with_link(extent.clone())
            .push(column);
        Box::new(Pointer::new(id, list).with_handlers(handlers.clone()))
    })
}

/// Which list's scroll a handler moves.
fn scroll_of(state: &mut SharedYAxisDemoState, alphabetical: bool) -> &mut Scroll {
    if alphabetical {
        &mut state.alphabetical_scroll
    } else {
        &mut state.recent_scroll
    }
}

/// `_AlbumTile`: the rounded grey art, "Album {title}" / "Artist", the
/// duration trailing, and the divider.
fn album_tile(title: &'static str, index: usize) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let minutes = minute_for(index);
    leaf(move || {
        let mut art = Container::new()
            .with_size(60.0, 60.0)
            .with_color(ART_GREY)
            .with_corner_radius(4.0)
            .with_padding(EdgeInsets::all(6.0));
        if let Some(image) = Image::shared(PLACEHOLDER_CACHE_KEY, PLACEHOLDER_IMAGE) {
            art = art.with_child(ImageView::new(image));
        }
        Column::new()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .push(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(16.0)
                    .push(
                        Container::new()
                            .with_padding(EdgeInsets::only(16.0, 0.0, 0.0, 0.0))
                            .with_child(art),
                    )
                    .push_flex(FlexChild::expanded(
                        Column::new()
                            .with_main_axis_size(MainAxisSize::Min)
                            .with_cross_axis_alignment(CrossAxisAlignment::Start)
                            .with_spacing(2.0)
                            .push(
                                Text::new(format!(
                                    "{} {}",
                                    l10n.demo_shared_y_axis_album_tile_title(),
                                    title
                                ))
                                .with_size(15.0),
                            )
                            .push(
                                Text::new(l10n.demo_shared_y_axis_album_tile_subtitle())
                                    .with_size(12.0),
                            ),
                        1,
                    ))
                    .push(
                        Container::new()
                            .with_padding(EdgeInsets::only(0.0, 0.0, 16.0, 0.0))
                            .with_child(
                                Text::new(format!(
                                    "{} {}",
                                    minutes,
                                    l10n.demo_shared_y_axis_album_tile_duration_unit()
                                ))
                                .with_size(12.0),
                            ),
                    ),
            )
            // Upstream's `Divider(height: 20, thickness: 1)`.
            .push(
                Container::new().with_height(20.0).with_child(Align::new(
                    Alignment::CENTER,
                    FullWidth::new(
                        Container::new()
                            .with_height(1.0)
                            .with_color(Color(0x1F00_0000)),
                    ),
                )),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_minutes_are_upstream_range_and_stable() {
        for index in 0..10 {
            let minutes = minute_for(index);
            assert!(
                (10..60).contains(&minutes),
                "upstream rolls nextInt(50) + 10"
            );
            assert_eq!(minutes, minute_for(index), "dealt once, not per build");
        }
    }

    #[test]
    fn the_first_flip_restarts_the_arrow_the_second_finishes_the_turn() {
        let mut state = SharedYAxisDemoState::default();
        toggle_sort(&mut state);
        assert!(state.alphabetical);
        assert_eq!(state.arrow, 0.0);
        assert_eq!(state.arrow_target, 0.5);
        assert!(state.running);
        toggle_sort(&mut state);
        assert!(!state.alphabetical);
        assert_eq!(state.arrow_target, 1.0, "animateTo(1) on the way back");
    }

    #[test]
    fn the_arrow_moves_to_its_target_and_stops() {
        // The rate is one full range (1.0) per 300ms -- Flutter's
        // `animateTo` scales the duration by the distance traversed.
        let mut value = 0.0;
        assert!(tick_arrow(&mut value, 0.5, TRANSITION_MICROS / 4));
        assert!((value - 0.25).abs() < 1e-5);
        assert!(!tick_arrow(&mut value, 0.5, TRANSITION_MICROS));
        assert_eq!(value, 0.5, "clamped at the target");
    }
}
