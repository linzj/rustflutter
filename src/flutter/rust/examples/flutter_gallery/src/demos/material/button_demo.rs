// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/button_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's five configurations of `ButtonDemo` -- text, elevated, outlined,
//! toggle and floating -- are one flattened catalogue entry here (PORTING.md),
//! so the stage stacks all five sections under their upstream titles
//! (`demoTextButtonTitle` and friends). Each section keeps upstream's shape:
//! a row of the enabled buttons, then the row of disabled ones.
//!
//! Divergences, each commented at its site as well:
//!
//! * The framework's `Button` has no icon slot, so the `.icon` variants are
//!   drawn by [`IconDemoButton`], a local replica of `Button`'s face with the
//!   glyph added.
//! * `ToggleButtons` has no framework counterpart; [`ToggleButtonsDemo`]
//!   draws the bordered group and its segments directly.
//! * `FloatingActionButton` likewise; [`FabDemo`] draws the 56-pixel circle
//!   and the extended variant. The fill is the demo theme's secondary colour,
//!   read off `material_demo_theme_data::COLOR_SCHEME` because the framework's
//!   `Theme` has no secondary slot.
//! * Upstream's per-variant app bars are the demo page's own bar here
//!   (`pages/demo.rs`).

use std::rc::Rc;

use rustflutter::framework::{component, leaf, single, stateful, BuildContext, StateHandle};
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, MainAxisSize, RenderFlex, StackPosition};
use rustflutter::semantics::{SemanticsAction, SemanticsProperties};
use rustflutter::widgets::{Align, Center, Pointer};

use crate::app::{ids, GalleryState};
use crate::data::demos as catalog;
use crate::themes::material_demo_theme_data::COLOR_SCHEME;

use super::{caption, column, DemoState};

/// Hit-test ids, from the demo-local block (PORTING.md: fixed bases, no
/// counters). Two per style section (plain, icon), three for the toggle
/// segments, two for the FABs and one for the FAB's tooltip trigger.
const TEXT_PLAIN: u64 = ids::DEMO_LOCAL;
const TEXT_ICON: u64 = ids::DEMO_LOCAL + 1;
const ELEVATED_PLAIN: u64 = ids::DEMO_LOCAL + 2;
const ELEVATED_ICON: u64 = ids::DEMO_LOCAL + 3;
const OUTLINED_PLAIN: u64 = ids::DEMO_LOCAL + 4;
const OUTLINED_ICON: u64 = ids::DEMO_LOCAL + 5;
const TOGGLE_BASE: u64 = ids::DEMO_LOCAL + 6;
const FAB: u64 = ids::DEMO_LOCAL + 9;
const FAB_TOOLTIP: u64 = ids::DEMO_LOCAL + 10;
const FAB_EXTENDED: u64 = ids::DEMO_LOCAL + 11;

/// Material Icons codepoints the framework's icon table does not name, in the
/// MATERIAL_ICONS family the app registers (`data/demos.rs`). The shipped
/// font build is newer than the codepoints upstream's `Icons` class names, so
/// these are the font's own `*_baseline` entries, the same convention as
/// `data/demos.rs`'s icon table.
mod glyph {
    /// `Icons.format_bold`.
    pub const FORMAT_BOLD: &str = "\u{e2af}";
    /// `Icons.format_italic`.
    pub const FORMAT_ITALIC: &str = "\u{e2b6}";
    /// `Icons.format_underline`.
    pub const FORMAT_UNDERLINE: &str = "\u{e2c2}";
}

/// Upstream's button label (`buttonText`).
const BUTTON_LABEL: &str = "BUTTON";

/// The stage: the five variants in upstream's `ButtonDemoType` order.
pub(super) fn buttons(
    state: &DemoState,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let _ = state;
    column(
        vec![
            caption("Text Button"),
            component(StandardButtonsDemo {
                style: ButtonVariant::Text,
                plain_id: TEXT_PLAIN,
                icon_id: TEXT_ICON,
                pressed,
                handle: handle.clone(),
            }),
            component(Divider),
            caption("Elevated Button"),
            component(StandardButtonsDemo {
                style: ButtonVariant::Filled,
                plain_id: ELEVATED_PLAIN,
                icon_id: ELEVATED_ICON,
                pressed,
                handle: handle.clone(),
            }),
            component(Divider),
            caption("Outlined Button"),
            component(StandardButtonsDemo {
                style: ButtonVariant::Outlined,
                plain_id: OUTLINED_PLAIN,
                icon_id: OUTLINED_ICON,
                pressed,
                handle: handle.clone(),
            }),
            component(Divider),
            caption("Toggle Buttons"),
            stateful(ToggleButtonsDemo),
            component(Divider),
            caption("Floating Action Button"),
            stateful(FabDemo),
        ],
        12.0,
    )
}

/// Upstream's `onPressed: () {}` -- the plain buttons acknowledge a tap but
/// change nothing.
fn noop(_state: &mut GalleryState) {}

// -- Text, elevated and outlined (BEGIN buttonDemoText / Elevated / Outlined) --

/// One of the three style sections: `_TextButtonDemo`, `_ElevatedButtonDemo`
/// and `_OutlinedButtonDemo` are the same column with a different button
/// class, so they are one component here keyed on the style.
struct StandardButtonsDemo {
    style: ButtonVariant,
    plain_id: u64,
    icon_id: u64,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
}

impl Component for StandardButtonsDemo {
    fn build(&self, _context: &mut BuildContext) -> AnyWidget {
        let style = self.style;
        let plain_id = self.plain_id;
        let icon_id = self.icon_id;
        let pressed = self.pressed;

        let enabled_row = super::row(
            vec![
                component(
                    Button::new(plain_id, BUTTON_LABEL)
                        .with_style(style)
                        .with_pressed(pressed == Some(plain_id))
                        .wired(self.handle.clone(), |s| &mut s.pressed, noop),
                ),
                component(
                    IconDemoButton::new(icon_id, BUTTON_LABEL)
                        .with_style(style)
                        .with_pressed(pressed == Some(icon_id))
                        .wired(self.handle.clone(), |s| &mut s.pressed, noop),
                ),
            ],
            // Upstream's `SizedBox(width: 12)` between the two buttons.
            12.0,
        );
        let disabled_row = super::row(
            vec![
                component(
                    Button::new(plain_id, BUTTON_LABEL)
                        .with_style(style)
                        .with_enabled(false),
                ),
                component(
                    IconDemoButton::new(icon_id, BUTTON_LABEL)
                        .with_style(style)
                        .with_enabled(false),
                ),
            ],
            12.0,
        );

        // Upstream centres both rows; the stage's column is start-aligned, so
        // the centring is applied here.
        column(
            vec![
                single(enabled_row, |row| Box::new(Center::new(row))),
                single(disabled_row, |row| Box::new(Center::new(row))),
            ],
            // Upstream's `SizedBox(height: 12)` between the rows.
            12.0,
        )
    }
}

/// Upstream's `TextButton.icon` / `ElevatedButton.icon` / `OutlinedButton.icon`.
///
/// The framework's `Button` (components.rs) takes a label but no icon, so the
/// icon variants replicate its face here -- same height, stadium border,
/// colours per style, disabled wash and splash -- with the glyph ahead of the
/// label. What is not replicated: `ButtonBounds`'s minimum-width layout box is
/// private, so a short label is not held to 64 pixels (with an icon ahead of
/// it, no label here comes under it), and the horizontal padding is the icon
/// constructors' twelve pixels at unit text scale rather than
/// `ButtonVariantButton.scaledPadding`'s curve.
struct IconDemoButton {
    id: u64,
    label: String,
    style: ButtonVariant,
    pressed: bool,
    enabled: bool,
    handlers: PointerHandlers,
}

impl IconDemoButton {
    fn new(id: u64, label: impl Into<String>) -> IconDemoButton {
        IconDemoButton {
            id,
            label: label.into(),
            style: ButtonVariant::default(),
            pressed: false,
            enabled: true,
            handlers: PointerHandlers::new(),
        }
    }

    fn with_style(mut self, style: ButtonVariant) -> Self {
        self.style = style;
        self
    }

    fn with_pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The same wiring as `Button::wired`.
    fn wired<S: 'static>(
        mut self,
        handle: StateHandle<S>,
        pressed_field: fn(&mut S) -> &mut Option<u64>,
        action: fn(&mut S),
    ) -> Self {
        if !self.enabled {
            return self;
        }
        let id = self.id;
        let tap_handle = handle.clone();
        let press_handle = handle;
        self.handlers = PointerHandlers::new()
            .with_tap(move |_| {
                tap_handle.set_state(move |state| action(state));
            })
            .with_press_change(move |down| {
                press_handle.set_state(move |state| {
                    *pressed_field(state) = if down { Some(id) } else { None };
                });
            });
        self
    }
}

impl Component for IconDemoButton {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let style = self.style;
        let pressed = self.pressed && self.enabled;
        let enabled = self.enabled;
        let label = self.label.clone();
        let handlers = self.handlers.clone();
        let id = self.id;

        // The colour table is `Button::build`'s, verbatim.
        let (mut fill, mut label_color, mut border) = match style {
            ButtonVariant::Filled => (Some(theme.primary), theme.on_primary, None),
            ButtonVariant::Danger => (Some(theme.danger), theme.on_primary, None),
            ButtonVariant::Outlined => (None, theme.primary, Some(theme.outline)),
            ButtonVariant::Text => (None, theme.primary, None),
        };
        if !enabled {
            if fill.is_some() || border.is_some() {
                let wash = theme.text.with_alpha(0x1F);
                if fill.is_some() {
                    fill = Some(wash);
                }
                border = border.map(|_| wash);
            }
            label_color = theme.text.with_alpha(0x61);
        }
        let press_overlay = pressed.then(|| match style {
            ButtonVariant::Filled | ButtonVariant::Danger => theme.on_primary.with_alpha(0x1A),
            _ => theme.primary.with_alpha(0x1A),
        });
        let radius = BUTTON_HEIGHT / 2.0;
        let body_size = theme.body_size;
        let splash_color = match style {
            ButtonVariant::Filled | ButtonVariant::Danger => theme.on_primary.with_alpha(0x30),
            _ => theme.primary.with_alpha(0x24),
        };

        let face = move || {
            let label = label.clone();
            let handlers = handlers.clone();
            leaf(move || {
                // Upstream's `.icon` layout: the 18-pixel icon, an 8-pixel
                // gap, the label.
                let content = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.0)
                    .push(
                        Text::new(catalog::icon::ADD)
                            .with_font_family(catalog::MATERIAL_ICONS)
                            .with_size(18.0)
                            .with_color(label_color),
                    )
                    .push(
                        Text::new(label.clone())
                            .with_size(body_size)
                            .with_weight(500)
                            .with_color(label_color),
                    );
                let mut container = Container::new()
                    .with_height(BUTTON_HEIGHT)
                    .with_corner_radius(radius)
                    .with_padding(EdgeInsets::symmetric(12.0, 0.0))
                    .with_child(Align::new(Alignment::CENTER, content));
                if let Some(color) = fill {
                    container = container.with_color(color);
                }
                if let Some(color) = border {
                    container = container.with_border(1.5, color);
                }
                let body = if let Some(overlay) = press_overlay {
                    rustflutter::render::RenderStack::new()
                        .push(container)
                        .push_positioned(
                            Container::new()
                                .with_color(overlay)
                                .with_corner_radius(radius),
                            StackPosition::fill(),
                        )
                } else {
                    rustflutter::render::RenderStack::new().push(container)
                };
                Pointer::new(id, body).with_handlers(handlers.clone())
            })
        };

        let described = |inner: AnyWidget| {
            let properties = if enabled {
                SemanticsProperties::button(&self.label)
            } else {
                SemanticsProperties::disabled_button(&self.label)
            };
            let tap = self.handlers.on_tap.clone();
            rustflutter::semantics::semantics_with_action(
                rustflutter::semantics::node_id_for(id),
                properties,
                inner,
                move |action| {
                    if action == SemanticsAction::Tap {
                        if let Some(tap) = &tap {
                            tap(rustflutter::gestures::TapEvent {
                                local_position: rustflutter::render::Offset::ZERO,
                                pointer_id: 0,
                            });
                        }
                    }
                },
            )
        };

        if !enabled {
            return described(face());
        }
        // The splash is the button's own colour, inside its own region, as in
        // `Button::build`; the id offset is components.rs's `INK_ID_OFFSET`.
        described(stateful(
            Ink::new(id.wrapping_add(1 << 40), face).with_color(splash_color),
        ))
    }
}

/// `Button`'s height (`components.rs`'s `BUTTON_HEIGHT`), needed here because
/// the constant is private.
const BUTTON_HEIGHT: f32 = 40.0;

// -- Toggle buttons (BEGIN buttonDemoToggle) -----------------------------------

/// Upstream's `_ToggleButtonsDemo`: three segments, independently toggleable,
/// then the same group disabled. Restoration is not carried (nothing here
/// restores); the initial selection is upstream's: only the middle segment.
struct ToggleButtonsDemo;

struct ToggleButtonsState {
    /// Upstream's `isSelected`.
    selected: [bool; 3],
}

impl Default for ToggleButtonsState {
    fn default() -> ToggleButtonsState {
        ToggleButtonsState {
            selected: [false, true, false],
        }
    }
}

/// Upstream's `onPressed`: `isSelected[index] = !isSelected[index]`.
fn toggle_after_tap(selected: [bool; 3], index: usize) -> [bool; 3] {
    let mut next = selected;
    next[index] = !next[index];
    next
}

impl StatefulComponent for ToggleButtonsDemo {
    type State = ToggleButtonsState;

    fn build(
        &self,
        state: &ToggleButtonsState,
        handle: StateHandle<ToggleButtonsState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let selected = state.selected;

        let mut groups: Vec<AnyWidget> = Vec::new();
        for enabled in [true, false] {
            // The tap handlers are built here and cloned inside the `leaf`,
            // because a `leaf` rebuilds: its closure is `Fn`, so nothing it
            // captured may move out.
            let taps: Vec<PointerHandlers> = (0..3)
                .map(|index| {
                    let tap_handle = handle.clone();
                    PointerHandlers::new().with_tap(move |_| {
                        tap_handle.set_state(move |s| {
                            s.selected = toggle_after_tap(s.selected, index);
                        });
                    })
                })
                .collect();
            let theme = Rc::clone(&theme);
            groups.push(leaf(move || {
                let mut segments = RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center);
                for (index, glyph) in [
                    glyph::FORMAT_BOLD,
                    glyph::FORMAT_ITALIC,
                    glyph::FORMAT_UNDERLINE,
                ]
                .iter()
                .enumerate()
                {
                    if index > 0 {
                        // The divider between segments: upstream draws each
                        // child's trailing border, which is the same line.
                        segments = segments.push(
                            Container::new()
                                .with_size(1.0, TOGGLE_HEIGHT)
                                .with_color(theme.outline),
                        );
                    }
                    let (fill, icon_color) =
                        toggle_segment_colors(&theme, selected[index], enabled);
                    let mut segment = Container::new()
                        .with_size(TOGGLE_HEIGHT, TOGGLE_HEIGHT)
                        .with_child(Align::new(
                            Alignment::CENTER,
                            Text::new(*glyph)
                                .with_font_family(catalog::MATERIAL_ICONS)
                                .with_size(24.0)
                                .with_color(icon_color),
                        ));
                    if let Some(fill) = fill {
                        segment = segment.with_color(fill);
                    }
                    if enabled {
                        segments = segments.push(
                            Pointer::new(TOGGLE_BASE + index as u64, segment)
                                .with_handlers(taps[index].clone()),
                        );
                    } else {
                        segments = segments.push(segment);
                    }
                }
                // The group's own border. Upstream's default `ToggleButtons`
                // shape: one outline around the row, square corners.
                Container::new()
                    .with_border(1.0, theme.outline)
                    .with_child(segments)
            }));
        }

        column(
            groups
                .into_iter()
                .map(|group| single(group, |row| Box::new(Center::new(row))))
                .collect(),
            // Upstream's `SizedBox(height: 12)` between the groups.
            12.0,
        )
    }
}

/// A toggle segment is a 48-pixel square: upstream's default
/// `constraints: BoxConstraints(minWidth: 48, minHeight: 48)`.
const TOGGLE_HEIGHT: f32 = 48.0;

/// The segment's fill and icon colour. Upstream's defaults: a selected child
/// fills with `colorScheme.primary` at 12% and draws in `primary`; an
/// unselected one draws in on-surface at 87%; a disabled one draws in
/// on-surface at 38% and does not fill.
fn toggle_segment_colors(theme: &Theme, selected: bool, enabled: bool) -> (Option<Color>, Color) {
    if !enabled {
        return (None, theme.text.with_alpha(0x61));
    }
    if selected {
        (Some(theme.primary.with_alpha(0x1F)), theme.primary)
    } else {
        (None, theme.text.with_alpha(0xDE))
    }
}

// -- Floating action buttons (BEGIN buttonDemoFloating) -------------------------

/// Upstream's `_FloatingActionButtonDemo`: a round FAB with a tooltip and an
/// extended FAB, both with `onPressed: () {}` -- they splash but change
/// nothing.
struct FabDemo;

#[derive(Default)]
struct FabDemoState {
    /// Whether the round FAB's tooltip is showing.
    tooltip: bool,
}

impl StatefulComponent for FabDemo {
    type State = FabDemoState;

    fn build(
        &self,
        state: &FabDemoState,
        handle: StateHandle<FabDemoState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        // Upstream's FAB fill in this theme is `colorScheme.secondary`; the
        // framework's Theme has no secondary slot, so it is read off the
        // ported scheme.
        let fill = COLOR_SCHEME.secondary;
        let on_fill = COLOR_SCHEME.on_secondary;
        let splash = on_fill.with_alpha(0x30);
        let label_size = theme.body_size;

        // FloatingActionButton(onPressed: () {}, tooltip: 'Create',
        // child: Icon(Icons.add)). 56 pixels, elevation 6: upstream's
        // defaults.
        let fab_face = move || {
            leaf(move || {
                Container::new()
                    .with_size(56.0, 56.0)
                    .with_color(fill)
                    .with_corner_radius(28.0)
                    .with_elevation(6)
                    .with_child(Align::new(
                        Alignment::CENTER,
                        Text::new(catalog::icon::ADD)
                            .with_font_family(catalog::MATERIAL_ICONS)
                            .with_size(24.0)
                            .with_color(on_fill),
                    ))
            })
        };
        let fab = stateful(Ink::new(FAB, fab_face).with_color(splash));
        let fab = component(
            TooltipTrigger::new(FAB_TOOLTIP, fab).wired(handle, |s, visible| s.tooltip = visible),
        );
        let fab = rustflutter::semantics::describe(SemanticsProperties::button("Create"), fab);

        // FloatingActionButton.extended(icon: Icon(Icons.add),
        // label: Text('Create'), onPressed: () {}). Height 48 with horizontal
        // padding 20: upstream's extended defaults.
        let extended_face = move || {
            leaf(move || {
                Container::new()
                    .with_height(48.0)
                    .with_color(fill)
                    .with_corner_radius(24.0)
                    .with_elevation(6)
                    .with_padding(EdgeInsets::symmetric(20.0, 0.0))
                    .with_child(Align::new(
                        Alignment::CENTER,
                        RenderFlex::row()
                            .with_main_axis_size(MainAxisSize::Min)
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(8.0)
                            .push(
                                Text::new(catalog::icon::ADD)
                                    .with_font_family(catalog::MATERIAL_ICONS)
                                    .with_size(24.0)
                                    .with_color(on_fill),
                            )
                            .push(
                                Text::new("Create")
                                    .with_size(label_size)
                                    .with_weight(500)
                                    .with_color(on_fill),
                            ),
                    ))
            })
        };
        let extended = stateful(Ink::new(FAB_EXTENDED, extended_face).with_color(splash));
        let extended =
            rustflutter::semantics::describe(SemanticsProperties::button("Create"), extended);

        let mut children: Vec<AnyWidget> =
            vec![single(super::row(vec![fab, extended], 12.0), |row| {
                Box::new(Center::new(row))
            })];
        if state.tooltip {
            children.push(single(component(TooltipBubble::new("Create")), |bubble| {
                Box::new(Center::new(bubble))
            }));
        }
        column(children, 12.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toggle_group_starts_with_only_the_middle_selected() {
        // Upstream's `isSelected`: [false, true, false].
        assert_eq!(ToggleButtonsState::default().selected, [false, true, false]);
    }

    #[test]
    fn a_tap_toggles_its_own_segment_only() {
        let start = ToggleButtonsState::default().selected;
        assert_eq!(toggle_after_tap(start, 0), [true, true, false]);
        assert_eq!(toggle_after_tap(start, 1), [false, false, false]);
        assert_eq!(toggle_after_tap(start, 2), [false, true, true]);
    }

    #[test]
    fn the_button_label_is_upstreams() {
        // `GalleryLocalizations.buttonText`.
        assert_eq!(BUTTON_LABEL, "BUTTON");
    }
}
