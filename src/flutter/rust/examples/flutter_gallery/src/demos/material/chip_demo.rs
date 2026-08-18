// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/chip_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's four configurations of `ChipDemo` -- action, choice, filter and
//! input -- are one flattened catalogue entry here (PORTING.md), so the stage
//! stacks all four sections under their upstream titles (`demoActionChipTitle`
//! and friends). The choice and filter sections are per-demo stateful
//! components, like upstream's `_ChoiceChipDemoState` and
//! `_FilterChipDemoState`.
//!
//! Divergences, each commented at its site as well:
//!
//! * The framework's `Chip` has no avatar, no delete icon and no disabled
//!   look, so all four sections draw [`DemoChip`], a replica of `Chip`'s
//!   face (controls.rs) with those three things added.
//! * Restoration (`RestorationMixin` on both state classes) is not carried:
//!   nothing here restores.
//! * Upstream's per-variant app bars are the demo page's own bar here
//!   (`pages/demo.rs`).

use rustflutter::framework::{component, leaf, many, single, stateful, BuildContext, StateHandle};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, MainAxisSize, RenderFlex};
use rustflutter::semantics::SemanticsProperties;
use rustflutter::widgets::{Align, Center, Pointer, Wrap};

use crate::app::{ids, GalleryState};
use crate::data::demos as catalog;

use super::{caption, column, DemoState};

/// Hit-test ids, from the demo-local block (PORTING.md: fixed bases, no
/// counters).
const ACTION_CHIP: u64 = ids::DEMO_LOCAL;
const CHOICE_BASE: u64 = ids::DEMO_LOCAL + 1;
const FILTER_BASE: u64 = ids::DEMO_LOCAL + 4;
const INPUT_CHIP: u64 = ids::DEMO_LOCAL + 7;

/// Material Icons codepoints the framework's icon table does not name, in the
/// MATERIAL_ICONS family the app registers (`data/demos.rs`). The shipped
/// font build is newer than the codepoints upstream's `Icons` class names, so
/// these are the font's own `*_baseline` entries, the same convention as
/// `data/demos.rs`'s icon table.
mod glyph {
    /// `Icons.brightness_5`, the action chip's avatar.
    pub const BRIGHTNESS_5: &str = "\u{e109}";
    /// `Icons.directions_bike`, the input chip's avatar.
    pub const DIRECTIONS_BIKE: &str = "\u{e1d2}";
    /// `Icons.cancel`, the input chip's default delete icon.
    pub const CANCEL: &str = "\u{e139}";
}

/// The stage: the four variants in upstream's `ChipDemoType` order.
pub(super) fn chips(state: &DemoState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let _ = state;
    column(
        vec![
            caption("Action Chip"),
            component(ActionChipDemo {
                handle: handle.clone(),
            }),
            component(Divider),
            caption("Choice Chip"),
            stateful(ChoiceChipDemo),
            component(Divider),
            caption("Filter Chip"),
            stateful(FilterChipDemo),
            component(Divider),
            caption("Input Chip"),
            component(InputChipDemo { handle }),
        ],
        12.0,
    )
}

/// Upstream's `onPressed: () {}` -- the static chips acknowledge a tap but
/// change nothing.
fn noop(_state: &mut GalleryState) {}

/// A chip the way upstream draws it: the framework's `Chip` face (height 32,
/// stadium, the same colours per style), plus the avatar, delete icon and
/// disabled look that `Chip` has no slots for.
///
/// The disabled rule is `Button`'s (components.rs): the label drops to 38% of
/// the on-surface colour and a border to 12%. A selected chip keeps its fill
/// when disabled, because upstream's disabled rows mirror the enabled row's
/// `selected` values.
struct DemoChip {
    id: u64,
    label: String,
    style: ChipStyle,
    enabled: bool,
    /// The avatar glyph and its size, upstream's `avatar:`.
    avatar: Option<(&'static str, f32)>,
    /// Upstream's `onDeleted != null`: the delete icon shows.
    delete_icon: bool,
    handlers: PointerHandlers,
}

impl DemoChip {
    fn new(id: u64, label: impl Into<String>) -> DemoChip {
        DemoChip {
            id,
            label: label.into(),
            style: ChipStyle::default(),
            enabled: true,
            avatar: None,
            delete_icon: false,
            handlers: PointerHandlers::new(),
        }
    }

    fn selected(self, selected: bool) -> Self {
        self.with_style(if selected {
            ChipStyle::Selected
        } else {
            ChipStyle::Filter
        })
    }

    fn with_style(mut self, style: ChipStyle) -> Self {
        self.style = style;
        self
    }

    fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    fn with_avatar(mut self, glyph: &'static str, size: f32) -> Self {
        self.avatar = Some((glyph, size));
        self
    }

    fn with_delete_icon(mut self) -> Self {
        self.delete_icon = true;
        self
    }

    /// The same wiring as `Chip::wired`.
    fn wired<S: 'static>(mut self, handle: StateHandle<S>, action: fn(&mut S)) -> Self {
        if !self.enabled {
            return self;
        }
        self.handlers = PointerHandlers::new().with_tap(move |_| {
            handle.set_state(move |state| action(state));
        });
        self
    }

    fn with_handlers(mut self, handlers: PointerHandlers) -> Self {
        self.handlers = handlers;
        self
    }
}

impl Component for DemoChip {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        // The colour table is `Chip::build`'s, verbatim.
        let (fill, mut text_color, mut border) = match self.style {
            ChipStyle::Filter => (Color::TRANSPARENT, theme.text, Some(theme.outline)),
            ChipStyle::Selected => (
                theme.primary.with_alpha(0x33),
                theme.primary,
                Some(theme.primary),
            ),
            ChipStyle::Action => (theme.surface_variant, theme.text, None),
        };
        // Upstream's avatar and delete-icon colour is `Colors.black54`;
        // on-surface at 54%.
        let mut icon_color = theme.text.with_alpha(0x8A);
        if !self.enabled {
            text_color = theme.text.with_alpha(0x61);
            icon_color = theme.text.with_alpha(0x61);
            border = border.map(|_| theme.text.with_alpha(0x1F));
        }
        let size = theme.body_size - 1.0;
        let label = self.label.clone();
        let avatar = self.avatar;
        let delete_icon = self.delete_icon;
        let id = self.id;
        let handlers = self.handlers.clone();

        let face = leaf(move || {
            let mut content = RenderFlex::row()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(8.0);
            if let Some((glyph, glyph_size)) = avatar {
                content = content.push(
                    Text::new(glyph)
                        .with_font_family(catalog::MATERIAL_ICONS)
                        .with_size(glyph_size)
                        .with_color(icon_color),
                );
            }
            content = content.push(
                Text::new(label.clone())
                    .with_size(size)
                    .with_weight(600)
                    .with_color(text_color),
            );
            if delete_icon {
                // Upstream's default delete icon, `Icons.cancel` at 18. Its
                // tap target is upstream's `onDeleted`, a no-op here, so the
                // glyph is drawn but not a region of its own.
                content = content.push(
                    Text::new(glyph::CANCEL)
                        .with_font_family(catalog::MATERIAL_ICONS)
                        .with_size(18.0)
                        .with_color(icon_color),
                );
            }
            // Chips with an avatar or a delete icon pull that side's padding
            // in; a bare label keeps the framework's fourteen.
            let left = if avatar.is_some() { 8.0 } else { 14.0 };
            let right = if delete_icon { 8.0 } else { 14.0 };
            let mut container = Container::new()
                .with_height(32.0)
                .with_color(fill)
                .with_corner_radius(16.0)
                .with_padding(EdgeInsets::only(left, 0.0, right, 0.0))
                .with_child(Align::new(Alignment::CENTER, content));
            if let Some(border) = border {
                container = container.with_border(1.0, border);
            }
            // Like `Chip::build`, a disabled chip keeps its region but has no
            // handlers.
            Pointer::new(id, container).with_handlers(handlers.clone())
        });

        rustflutter::semantics::describe(
            if self.enabled {
                SemanticsProperties::button(&self.label)
            } else {
                SemanticsProperties::disabled_button(&self.label)
            },
            face,
        )
    }
}

// -- Action (BEGIN chipDemoAction) --------------------------------------------

/// Upstream's `_ActionChipDemo`: one centered chip with a brightness avatar
/// and a no-op `onPressed`.
struct ActionChipDemo {
    handle: StateHandle<GalleryState>,
}

impl Component for ActionChipDemo {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        single(
            component(
                DemoChip::new(ACTION_CHIP, "Turn on lights")
                    .with_style(ChipStyle::Action)
                    .with_avatar(glyph::BRIGHTNESS_5, 18.0)
                    .wired(self.handle.clone(), noop),
            ),
            |chip| Box::new(Center::new(chip)),
        )
    }
}

// -- Choice (BEGIN chipDemoChoice) --------------------------------------------

/// Upstream's `_ChoiceChipDemo`.
struct ChoiceChipDemo;

/// Upstream's `_indexSelected`: `RestorableIntN(null)`, so no chip starts
/// selected.
#[derive(Default)]
struct ChoiceChipState {
    selected: Option<usize>,
}

/// Upstream's `onSelected: (value) { _indexSelected = value ? index : -1 }`:
/// tapping the selected chip clears the selection, tapping another moves it.
fn choice_after_tap(current: Option<usize>, tapped: usize) -> Option<usize> {
    if current == Some(tapped) {
        None
    } else {
        Some(tapped)
    }
}

/// The labels, upstream's `chipSmall`/`chipMedium`/`chipLarge`.
const CHOICE_LABELS: [&str; 3] = ["Small", "Medium", "Large"];

/// A `Wrap(spacing: 8)` of chips, centred the way upstream's `Center` does.
fn chip_wrap(chips: Vec<AnyWidget>) -> AnyWidget {
    many(chips, |rendered| {
        let mut wrap = Wrap::new().with_spacing(8.0);
        for chip in rendered {
            wrap = wrap.push(chip);
        }
        Box::new(Center::new(wrap))
    })
}

impl StatefulComponent for ChoiceChipDemo {
    type State = ChoiceChipState;

    fn build(
        &self,
        state: &ChoiceChipState,
        handle: StateHandle<ChoiceChipState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let selected = state.selected;
        // Two rows of the same three chips: enabled, then disabled
        // (upstream's "Disabled chips" row mirrors the selection).
        let rows: Vec<AnyWidget> = [true, false]
            .into_iter()
            .map(|enabled| {
                let chips = CHOICE_LABELS
                    .iter()
                    .enumerate()
                    .map(|(index, label)| {
                        let chip = DemoChip::new(CHOICE_BASE + index as u64, *label)
                            .selected(selected == Some(index))
                            .with_enabled(enabled);
                        let chip = if enabled {
                            let tap_handle = handle.clone();
                            chip.with_handlers(PointerHandlers::new().with_tap(move |_| {
                                tap_handle.set_state(move |s| {
                                    s.selected = choice_after_tap(s.selected, index);
                                });
                            }))
                        } else {
                            chip
                        };
                        component(chip)
                    })
                    .collect();
                chip_wrap(chips)
            })
            .collect();
        column(rows, 12.0)
    }
}

// -- Filter (BEGIN chipDemoFilter) --------------------------------------------

/// Upstream's `_FilterChipDemo`.
struct FilterChipDemo;

/// Upstream's three `RestorableBool(false)`s.
#[derive(Default)]
struct FilterChipState {
    selected: [bool; 3],
}

/// The labels, upstream's `chipElevator`/`chipWasher`/`chipFireplace`.
const FILTER_LABELS: [&str; 3] = ["Elevator", "Washer", "Fireplace"];

/// Upstream's `onSelected`: the chip's own bool flips.
fn filter_after_tap(selected: [bool; 3], index: usize) -> [bool; 3] {
    let mut next = selected;
    next[index] = !next[index];
    next
}

impl StatefulComponent for FilterChipDemo {
    type State = FilterChipState;

    fn build(
        &self,
        state: &FilterChipState,
        handle: StateHandle<FilterChipState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let selected = state.selected;
        let rows: Vec<AnyWidget> = [true, false]
            .into_iter()
            .map(|enabled| {
                let chips = FILTER_LABELS
                    .iter()
                    .enumerate()
                    .map(|(index, label)| {
                        let chip = DemoChip::new(FILTER_BASE + index as u64, *label)
                            .selected(selected[index])
                            .with_enabled(enabled);
                        let chip = if enabled {
                            let tap_handle = handle.clone();
                            chip.with_handlers(PointerHandlers::new().with_tap(move |_| {
                                tap_handle.set_state(move |s| {
                                    s.selected = filter_after_tap(s.selected, index);
                                });
                            }))
                        } else {
                            chip
                        };
                        component(chip)
                    })
                    .collect();
                chip_wrap(chips)
            })
            .collect();
        column(rows, 12.0)
    }
}

// -- Input (BEGIN chipDemoInput) ----------------------------------------------

/// Upstream's `_InputChipDemo`: the enabled chip, then the disabled one, both
/// with the bike avatar and a delete icon.
struct InputChipDemo {
    handle: StateHandle<GalleryState>,
}

impl Component for InputChipDemo {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        column(
            [true, false]
                .into_iter()
                .map(|enabled| {
                    let chip = DemoChip::new(INPUT_CHIP, "Biking")
                        .with_avatar(glyph::DIRECTIONS_BIKE, 20.0)
                        .with_delete_icon()
                        .with_enabled(enabled);
                    let chip = if enabled {
                        chip.wired(self.handle.clone(), noop)
                    } else {
                        chip
                    };
                    single(component(chip), |chip| Box::new(Center::new(chip)))
                })
                .collect(),
            12.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_choice_chip_starts_selected() {
        // Upstream's `RestorableIntN(null)`.
        assert_eq!(ChoiceChipState::default().selected, None);
    }

    #[test]
    fn tapping_the_selected_choice_clears_it() {
        // `value ? index : -1`: a selected chip reports false, clearing the
        // selection; any other chip reports true, taking it.
        assert_eq!(choice_after_tap(None, 1), Some(1));
        assert_eq!(choice_after_tap(Some(1), 1), None);
        assert_eq!(choice_after_tap(Some(1), 2), Some(2));
    }

    #[test]
    fn filter_chips_toggle_independently() {
        let start = FilterChipState::default().selected;
        assert_eq!(start, [false, false, false]);
        assert_eq!(filter_after_tap(start, 0), [true, false, false]);
        assert_eq!(
            filter_after_tap([true, false, true], 2),
            [true, false, false]
        );
    }
}
