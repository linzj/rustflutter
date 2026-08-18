// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/dialog_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `DialogDemo` takes a `DialogDemoType` and shows one variant per
//! catalogue configuration, each variant a page whose centred
//! `ElevatedButton(child: Text(dialogShow))` presents its dialog. The
//! catalogue here is flattened to one configuration per demo (PORTING.md:
//! "demo options section is unreachable"), so the stage stacks all four
//! variants: a caption with the variant's upstream title and its SHOW DIALOG
//! button.
//!
//! State: upstream's `_DialogDemoState` holds three `RestorableRouteFuture`s
//! and the value the popped route handed `_showInSnackBar`. The overlay a
//! demo shows is dispatched from `mod.rs`'s shared `overlay()`, which reads
//! only `GalleryState`, so this demo's slice of the shared `DemoState`
//! carries it: `dialog_open` is whether a dialog is up, and `counter` is the
//! open dialog's variant index while one is, or the popped value encoded as
//! `1 + variant * 4 + option index` after one closes (0 = nothing yet). The
//! encoding is this file's own; `mod.rs` only ever reads `dialog_open`.
//!
//! Divergences, each marked at its site as well:
//!
//! * **the snackbar is inline** -- upstream's `_showInSnackBar` goes through
//!   `ScaffoldMessenger`, whose overlay here belongs to the dialog itself
//!   (`mod.rs` gives a demo one overlay slot). The `SnackBar` is drawn as the
//!   stage's last row instead, and does not time out.
//! * **barrier dismiss pops no value** -- upstream's barrier tap completes
//!   the route with null, which the demo's non-nullable `onComplete` never
//!   sees as a selection; here the scrim closes the dialog and clears the
//!   result, the same observable outcome.
//! * **hand-rolled dialog card** -- the framework's `Dialog` always draws a
//!   title row, and upstream's alert (`_alertDialogDemoRoute`) has none, so
//!   all three modal variants share the card helper below, styled as the
//!   framework's is (surface, radius 28, elevation 6).

use rustflutter::framework::single;
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, MainAxisAlignment, MainAxisSize, RenderBox, RenderFlex,
};
use rustflutter::widgets::{Align, Center, Empty, Pointer, Positioned, Row, Stack};

use crate::app::{ids, GalleryState};
use crate::data::demos::MATERIAL_ICONS;
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::themes::material_demo_theme_data::COLOR_SCHEME;

use super::{caption, column, DemoState};

/// Upstream's `DialogDemoType`, as the value `counter` holds while
/// `dialog_open` is set.
mod variant {
    pub const ALERT: i32 = 0;
    pub const ALERT_TITLE: i32 = 1;
    pub const SIMPLE: i32 = 2;
    pub const FULLSCREEN: i32 = 3;
}

/// Encodes the value a dialog popped with: `1 + variant * 4 + option`, so 0
/// stays "nothing has been popped" (and `DemoState::default` reads that way).
fn result_code(variant: i32, option: usize) -> i32 {
    1 + variant * 4 + option as i32
}

/// The text upstream's route popped with, back out of the encoding. Upstream
/// the popped value is the `_DialogButton`'s text, or the `_DialogDemoItem`'s.
fn result_text(code: i32) -> Option<String> {
    if code <= 0 {
        return None;
    }
    let l10n = GalleryLocalizations::en();
    let variant = (code - 1) / 4;
    let option = (code - 1) % 4;
    let text = match (variant, option) {
        (variant::ALERT, 0) => l10n.dialog_cancel().to_string(),
        (variant::ALERT, 1) => l10n.dialog_discard().to_string(),
        (variant::ALERT_TITLE, 0) => l10n.dialog_disagree().to_string(),
        (variant::ALERT_TITLE, 1) => l10n.dialog_agree().to_string(),
        (variant::SIMPLE, 0) => "username@gmail.com".to_string(),
        (variant::SIMPLE, 1) => "user02@gmail.com".to_string(),
        (variant::SIMPLE, 2) => l10n.dialog_add_account().to_string(),
        _ => return None,
    };
    Some(text)
}

/// The demo body for the `dialog` slug: upstream's `_DialogDemoState.build`,
/// one section per variant rather than one page.
pub(super) fn dialog_launcher(
    state: &DemoState,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let sections = [
        (variant::ALERT, l10n.demo_alert_dialog_title()),
        (variant::ALERT_TITLE, l10n.demo_alert_title_dialog_title()),
        (variant::SIMPLE, l10n.demo_simple_dialog_title()),
        (variant::FULLSCREEN, l10n.demo_fullscreen_dialog_title()),
    ];

    let mut children: Vec<AnyWidget> = Vec::new();
    for (index, (variant, title)) in sections.iter().enumerate() {
        let id = ids::DEMO_LOCAL + index as u64;
        let variant = *variant;
        children.push(caption(*title));
        // Upstream's centred `ElevatedButton`; the stage's column is
        // start-aligned, so the button is centred by hand. Hand-wired rather
        // than `Button::wired`, because the tap has to know its variant and
        // `wired`'s action is a plain `fn`.
        children.push(single(
            component(
                Button::new(id, l10n.dialog_show())
                    .with_pressed(pressed == Some(id))
                    .with_handlers(
                        rustflutter::gestures::PointerHandlers::new()
                            .with_tap({
                                let handle = handle.clone();
                                move |_| {
                                    handle.set_state(move |s| {
                                        s.demo.counter = variant;
                                        s.demo.dialog_open = true;
                                    });
                                }
                            })
                            .with_press_change({
                                let handle = handle.clone();
                                move |down| {
                                    handle.set_state(move |s| {
                                        s.pressed = if down { Some(id) } else { None };
                                    });
                                }
                            }),
                    ),
            ),
            |rendered| Box::new(Center::new(rendered)),
        ));
    }

    // Upstream's `_showInSnackBar`: the popped value, shown after a dialog
    // closes. Inline rather than through `ScaffoldMessenger` (see the module
    // header); the fullscreen route pops with void, so it sets no result.
    if let Some(text) = result_text(state.counter) {
        children.push(component(Snackbar::new(
            ids::DEMO_LOCAL + sections.len() as u64,
            l10n.dialog_selected_option(text),
        )));
    }

    column(children, 12.0)
}

/// The modal over the demo's page: upstream's three `DialogRoute`s and the
/// fullscreen `MaterialPageRoute`, dispatched by the open variant.
pub(super) fn dialog_overlay(state: &GalleryState, handle: StateHandle<GalleryState>) -> AnyWidget {
    let pressed = state.pressed;
    match state.demo.counter {
        variant::FULLSCREEN => fullscreen_dialog(pressed, handle),
        other => {
            let scrim_handle = handle.clone();
            let scrim = component(Scrim::new(ids::SCRIM).wired(scrim_handle, |s| {
                s.demo.dialog_open = false;
                s.demo.counter = 0;
            }));
            let dialog = match other {
                variant::ALERT => alert_dialog(pressed, handle),
                variant::ALERT_TITLE => alert_dialog_with_title(pressed, handle),
                _ => simple_dialog(pressed, handle),
            };
            many(vec![scrim, dialog], |mut rendered| {
                let dialog = rendered.pop().unwrap_or_else(|| boxed(Empty));
                let scrim = rendered.pop().unwrap_or_else(|| boxed(Empty));
                Box::new(
                    Stack::new()
                        .push_positioned(scrim, Positioned::fill())
                        .push(Center::new(dialog)),
                )
            })
        }
    }
}

/// The card the three modal variants share: the framework `Dialog`'s metrics
/// (Material 3's 28-radius corner, elevation 6, 280 minimum width), hand-rolled
/// because that component always draws a title row and upstream's plain alert
/// has none.
fn dialog_card(content: impl RenderBox + 'static) -> impl RenderBox + 'static {
    Container::new()
        .with_width(280.0)
        .with_color(COLOR_SCHEME.surface)
        .with_corner_radius(28.0)
        .with_elevation(6)
        .with_padding(EdgeInsets::all(24.0))
        .with_child(content)
}

/// Upstream's `dialogTextStyle`: `titleMedium` sized, `bodySmall` coloured.
fn dialog_text(text: String) -> impl RenderBox + 'static {
    Text::new(text)
        .with_size(16.0)
        .with_color(COLOR_SCHEME.on_surface.with_alpha(0xB3))
}

/// Upstream's `_DialogButton`: a `TextButton` that pops with its own text.
/// Hand-wired for the same reason the launchers are: `Button::wired`'s action
/// is a plain `fn`, and this tap has to know which option it is.
fn dialog_button(
    id: u64,
    text: String,
    option: usize,
    pressed: Option<u64>,
    handle: StateHandle<GalleryState>,
) -> AnyWidget {
    component(
        Button::new(id, text)
            .with_style(ButtonStyle::Text)
            .with_pressed(pressed == Some(id))
            .with_handlers(
                rustflutter::gestures::PointerHandlers::new()
                    .with_tap({
                        let handle = handle.clone();
                        move |_| {
                            handle.set_state(move |s| {
                                s.demo.counter = result_code(s.demo.counter, option);
                                s.demo.dialog_open = false;
                            });
                        }
                    })
                    .with_press_change(move |down| {
                        handle.set_state(move |s| {
                            s.pressed = if down { Some(id) } else { None };
                        });
                    }),
            ),
    )
}

/// The actions row upstream's `AlertDialog` lays out: end-aligned text
/// buttons, 8 apart. The buttons arrive already rendered, from the variant's
/// `many`.
fn actions_row(rendered: Vec<rustflutter::widgets::BoxedWidget>) -> RenderFlex {
    let mut row = RenderFlex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_main_axis_alignment(MainAxisAlignment::End)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.0);
    for action in rendered {
        row = row.push(action);
    }
    row
}

/// Upstream's `_alertDialogDemoRoute`: content only, no title.
fn alert_dialog(pressed: Option<u64>, handle: StateHandle<GalleryState>) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let cancel = dialog_button(
        ids::DEMO_LOCAL + 10,
        l10n.dialog_cancel().to_string(),
        0,
        pressed,
        handle.clone(),
    );
    let discard = dialog_button(
        ids::DEMO_LOCAL + 11,
        l10n.dialog_discard().to_string(),
        1,
        pressed,
        handle,
    );
    many(vec![cancel, discard], move |rendered| {
        Box::new(dialog_card(
            Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(20.0)
                .push(dialog_text(l10n.dialog_discard_title().to_string()))
                .push(actions_row(rendered)),
        ))
    })
}

/// Upstream's `_alertDialogWithTitleDemoRoute`.
fn alert_dialog_with_title(pressed: Option<u64>, handle: StateHandle<GalleryState>) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let disagree = dialog_button(
        ids::DEMO_LOCAL + 10,
        l10n.dialog_disagree().to_string(),
        0,
        pressed,
        handle.clone(),
    );
    let agree = dialog_button(
        ids::DEMO_LOCAL + 11,
        l10n.dialog_agree().to_string(),
        1,
        pressed,
        handle,
    );
    many(vec![disagree, agree], move |rendered| {
        Box::new(dialog_card(
            Column::new()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Start)
                .with_spacing(16.0)
                // Upstream's `AlertDialog.title`, headline small.
                .push(
                    Text::new(l10n.dialog_location_title())
                        .with_size(20.0)
                        .with_color(COLOR_SCHEME.on_surface),
                )
                .push(dialog_text(l10n.dialog_location_description().to_string()))
                .push(actions_row(rendered)),
        ))
    })
}

/// Upstream's `_simpleDialogDemoRoute`: a title and three
/// `SimpleDialogOption`s.
fn simple_dialog(pressed: Option<u64>, handle: StateHandle<GalleryState>) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let items = [
        // Upstream's `Icons.account_circle` in `colorScheme.primary`.
        (
            "\u{e853}",
            COLOR_SCHEME.primary,
            "username@gmail.com".to_string(),
        ),
        // Upstream's `Icons.account_circle` in `colorScheme.secondary`.
        (
            "\u{e853}",
            COLOR_SCHEME.secondary,
            "user02@gmail.com".to_string(),
        ),
        // Upstream's `Icons.add_circle` in `disabledColor` (on-surface at
        // 38%, `ThemeData.disabledColor`'s default).
        (
            "\u{e147}",
            COLOR_SCHEME.on_surface.with_alpha(0x61),
            l10n.dialog_add_account().to_string(),
        ),
    ];

    let mut options: Vec<AnyWidget> = Vec::new();
    for (index, (icon, color, text)) in items.iter().enumerate() {
        let id = ids::DEMO_LOCAL + 12 + index as u64;
        let option = index;
        let tap_handle = handle.clone();
        options.push(component(DialogDemoItem {
            id,
            icon,
            color: *color,
            text: text.clone(),
            pressed: pressed == Some(id),
            option,
            handle: tap_handle,
        }));
    }

    many(options, move |rendered| {
        let mut list = Column::new()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(8.0)
            // Upstream's `SimpleDialog.title`, padded like its
            // `titlePadding` (top 24, sides 24 -- already the card's).
            .push(
                Text::new(l10n.dialog_set_backup())
                    .with_size(20.0)
                    .with_color(COLOR_SCHEME.on_surface),
            );
        for option in rendered {
            list = list.push(option);
        }
        Box::new(dialog_card(list))
    })
}

/// Upstream's `_DialogDemoItem`: an icon, then the text 16 to its right, the
/// whole row a `SimpleDialogOption` that pops with the text.
struct DialogDemoItem {
    id: u64,
    icon: &'static str,
    color: Color,
    text: String,
    pressed: bool,
    /// Which `SimpleDialogOption` this is; the tap pops with it.
    option: usize,
    handle: StateHandle<GalleryState>,
}

impl rustflutter::framework::Component for DialogDemoItem {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let id = self.id;
        let icon = self.icon;
        let color = self.color;
        let text = self.text.clone();
        let pressed = self.pressed;
        let option = self.option;
        let tap_handle = self.handle.clone();
        let press_handle = self.handle.clone();

        let handlers = rustflutter::gestures::PointerHandlers::new()
            .with_tap(move |_| {
                tap_handle.set_state(move |s| {
                    s.demo.counter = result_code(variant::SIMPLE, option);
                    s.demo.dialog_open = false;
                });
            })
            .with_press_change(move |down| {
                press_handle.set_state(move |s| {
                    s.pressed = if down { Some(id) } else { None };
                });
            });

        rustflutter::framework::leaf(move || {
            Pointer::new(
                id,
                Container::new()
                    // The row's highlight while held, upstream's ink well on
                    // the option.
                    .with_color(if pressed {
                        COLOR_SCHEME.on_surface.with_alpha(0x0A)
                    } else {
                        Color::TRANSPARENT
                    })
                    .with_padding(EdgeInsets::symmetric(8.0, 8.0))
                    .with_child(
                        Row::new()
                            .with_cross_axis_alignment(CrossAxisAlignment::Center)
                            .with_spacing(16.0)
                            // Upstream's `Icon(icon, size: 36, color: color)`.
                            .push(
                                Text::new(icon)
                                    .with_font_family(MATERIAL_ICONS)
                                    .with_size(36.0)
                                    .with_color(color),
                            )
                            .push(
                                Text::new(text.clone())
                                    .with_size(16.0)
                                    .with_color(COLOR_SCHEME.on_surface),
                            ),
                    ),
            )
            .with_handlers(handlers.clone())
        })
    }
}

/// Upstream's `_FullScreenDialogDemo`: a `Scaffold` whose app bar carries the
/// SAVE action, over a centred line of text. It fills the demo's area, as
/// upstream's fullscreen route fills the demo's navigator.
fn fullscreen_dialog(pressed: Option<u64>, handle: StateHandle<GalleryState>) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let save = component(
        Button::new(ids::DEMO_LOCAL + 16, l10n.dialog_fullscreen_save())
            .with_style(ButtonStyle::Text)
            .with_pressed(pressed == Some(ids::DEMO_LOCAL + 16))
            .wired(
                handle,
                |s| &mut s.pressed,
                |s| {
                    s.demo.dialog_open = false;
                },
            ),
    );

    many(vec![save], move |mut rendered| {
        let save = rendered.pop().unwrap_or_else(|| boxed(Empty));
        Box::new(
            Container::new()
                .with_color(COLOR_SCHEME.background)
                .with_child(
                    Column::expanded()
                        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                        // Upstream's AppBar under the demo theme's
                        // `appBarTheme`: primary fill, on-primary content.
                        .push(
                            Container::new()
                                .with_height(56.0)
                                .with_color(COLOR_SCHEME.primary)
                                .with_padding(EdgeInsets::symmetric(16.0, 0.0))
                                .with_child(
                                    Row::new()
                                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                                        .push_flex(rustflutter::render::FlexChild::expanded(
                                            Align::new(
                                                Alignment::CENTER_LEFT,
                                                Text::new(l10n.dialog_fullscreen_title())
                                                    .with_size(20.0)
                                                    .with_color(COLOR_SCHEME.on_primary),
                                            ),
                                            1,
                                        ))
                                        .push(save),
                                ),
                        )
                        .push_flex(rustflutter::render::FlexChild::expanded(
                            Center::new(
                                Text::new(l10n.dialog_fullscreen_description())
                                    .with_size(14.0)
                                    .with_color(COLOR_SCHEME.on_surface),
                            ),
                            1,
                        )),
                ),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_result_encoding_round_trips() {
        assert_eq!(result_text(0), None);
        for (variant, option, text) in [
            (variant::ALERT, 0, "CANCEL"),
            (variant::ALERT, 1, "DISCARD"),
            (variant::ALERT_TITLE, 0, "DISAGREE"),
            (variant::ALERT_TITLE, 1, "AGREE"),
            (variant::SIMPLE, 0, "username@gmail.com"),
            (variant::SIMPLE, 1, "user02@gmail.com"),
            (variant::SIMPLE, 2, "Add account"),
        ] {
            assert_eq!(
                result_text(result_code(variant, option)).as_deref(),
                Some(text)
            );
        }
        // Out of range is no result, never a panic.
        assert_eq!(result_text(result_code(variant::FULLSCREEN, 0)), None);
        assert_eq!(result_text(result_code(variant::SIMPLE, 3)), None);
    }
}
