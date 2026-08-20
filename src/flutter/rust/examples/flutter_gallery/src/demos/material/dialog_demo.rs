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
//! Upstream's `_DialogDemoState` holds three `RestorableRouteFuture`s and the
//! value the popped route handed `_showInSnackBar`. The dialogs go up here
//! through [`rustflutter::show_dialog_with`], the imperative call upstream's
//! `showDialog` is: it puts the content in the application's overlay behind a
//! real `ModalBarrier` and hands back a [`rustflutter::ModalHandle`] to close
//! it with. The popped value stays in this demo's slice of `DemoState` as
//! `counter`, encoded `1 + variant * 4 + option index` (0 = nothing yet).
//!
//! This file used to keep a `dialog_open` flag beside it so that `mod.rs`'s
//! shared `overlay()` slot could dispatch which dialog to stack over the page,
//! and hand-rolled a `Scrim` under the card. Both are gone: the framework has
//! the overlay, so the demo shows a dialog by asking for one, and a press
//! pushes a dialog the way upstream's press pushes a route.
//!
//! Divergences, each marked at its site as well:
//!
//! * **the snackbar is inline** -- upstream's `_showInSnackBar` goes through
//!   `ScaffoldMessenger`. The `SnackBar` is drawn as the stage's last row
//!   instead, which keeps the popped value beside the button that produced it;
//!   `snackbar_demo.rs` is the demo that exercises the messenger.
//! * **barrier dismiss pops no value** -- upstream's barrier tap completes the
//!   route with null, which the demo's non-nullable `onComplete` never sees as
//!   a selection; here the barrier closes the dialog and leaves the previous
//!   result standing, the same observable outcome.
//! * **hand-rolled dialog card** -- the framework's `Dialog` always draws a
//!   title row, and upstream's alert (`_alertDialogDemoRoute`) has none, so
//!   all three modal variants share the card helper below, styled as the
//!   framework's is (surface, radius 28, elevation 6).

use rustflutter::framework::{BuildContext, single};
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, CrossAxisAlignment, MainAxisAlignment, MainAxisSize, RenderBox, RenderFlex,
};
use rustflutter::modal_barrier::ModalBarrier;
use rustflutter::widgets::{Align, Center, Empty, Pointer, Row};
use rustflutter::{DialogCloser, ModalHandle, OverlayHandle, show_dialog_with};

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
    // A component rather than a plain function, because showing a dialog needs
    // the overlay and the overlay is read from a `BuildContext`.
    component(DialogLauncher {
        result: state.counter,
        pressed,
        gallery: handle,
    })
}

struct DialogLauncher {
    /// The popped value, or 0 for nothing yet.
    result: i32,
    pressed: Option<u64>,
    gallery: StateHandle<GalleryState>,
}

impl Component for DialogLauncher {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
    let overlay = OverlayHandle::of(context);
    let pressed = self.pressed;
    let handle = self.gallery.clone();
    let result = self.result;
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
                                let overlay = overlay.clone();
                                move |_| {
                                    // Upstream's press pushes a route; this one
                                    // puts a dialog in the overlay. Two presses
                                    // give two, as two pushes would.
                                    if let Some(overlay) = overlay.clone() {
                                        show_variant(overlay, variant, handle.clone());
                                    }
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
    if let Some(text) = result_text(result) {
        children.push(component(Snackbar::new(
            ids::DEMO_LOCAL + sections.len() as u64,
            l10n.dialog_selected_option(text),
        )));
    }

    column(children, 12.0)
    }
}

/// Puts one variant up. Upstream's four routes, as four calls.
///
/// The fullscreen one gets a barrier that **cannot be tapped away**, because
/// upstream's is a `MaterialPageRoute` and not a `DialogRoute`: a page is left
/// through its own SAVE, not by touching beside it. The three dialogs take
/// `showDialog`'s dismissible default.
fn show_variant(
    overlay: std::rc::Rc<OverlayHandle>,
    variant: i32,
    gallery: StateHandle<GalleryState>,
) -> Option<ModalHandle> {
    let closer = DialogCloser::new();
    let barrier = barrier_for(variant);

    let opened = {
        let closer = closer.clone();
        show_dialog_with(overlay, barrier, move || {
            let gallery = gallery.clone();
            let closer = closer.clone();
            match variant {
                variant::FULLSCREEN => fullscreen_dialog(gallery, closer),
                variant::ALERT => alert_dialog(gallery, closer),
                variant::ALERT_TITLE => alert_dialog_with_title(gallery, closer),
                _ => simple_dialog(gallery, closer),
            }
        })
    };
    if let Some(handle) = opened.clone() {
        // The knot: the buttons inside the dialog need the handle, and the
        // handle does not exist until the dialog is up. See `DialogCloser`.
        closer.arm(handle);
    }
    opened
}

/// The barrier a variant goes behind.
fn barrier_for(variant: i32) -> ModalBarrier {
    if variant == variant::FULLSCREEN {
        // Upstream's fullscreen route is a `MaterialPageRoute`: it fills the
        // screen, so there is no page behind it to dim, and it is left through
        // its own SAVE rather than by touching beside it.
        ModalBarrier::new().with_dismissible(false)
    } else {
        ModalBarrier::new().with_color(DIALOG_BARRIER_COLOR)
    }
}

/// The scrim upstream's `showDialog` puts under a dialog: `Colors.black54`.
const DIALOG_BARRIER_COLOR: Color = Color::argb(0x8A, 0, 0, 0);

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
    variant: i32,
    handle: StateHandle<GalleryState>,
    closer: DialogCloser,
) -> AnyWidget {
    stateful(DialogButton {
        id,
        text,
        option,
        variant,
        handle,
        closer,
    })
}

/// A button inside a dialog.
///
/// **Its press highlight is its own**, where the page's buttons put theirs on
/// the shared `GalleryState`. A dialog's content is built once when it goes up
/// and rebuilt only when the overlay entry does, so a highlight read from the
/// page's state would be whatever it was at that moment. Local state is also
/// what it should have been all along: nothing outside the dialog has any
/// business knowing which of its buttons is held.
struct DialogButton {
    id: u64,
    text: String,
    option: usize,
    /// Which dialog this button is in. The popped value is
    /// `result_code(variant, option)`, and the dialog no longer announces its
    /// variant through a shared field for the button to read back.
    variant: i32,
    handle: StateHandle<GalleryState>,
    closer: DialogCloser,
}

impl StatefulComponent for DialogButton {
    type State = bool;

    fn build(&self, held: &bool, held_handle: StateHandle<bool>, _: &mut BuildContext) -> AnyWidget {
        let id = self.id;
        let option = self.option;
        let variant = self.variant;
        let handle = self.handle.clone();
        let closer = self.closer.clone();
        component(
            Button::new(id, self.text.clone())
                .with_style(ButtonVariant::Text)
                .with_pressed(*held)
                .with_handlers(
                    rustflutter::gestures::PointerHandlers::new()
                        .with_tap(move |_| {
                            // Upstream's `Navigator.pop(context, value)`: the
                            // value first, then the route goes.
                            handle.set_state(move |s| {
                                s.demo.counter = result_code(variant, option);
                            });
                            closer.close();
                        })
                        .with_press_change(move |down| {
                            held_handle.set_state(move |held| *held = down);
                        }),
                ),
        )
    }
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
fn alert_dialog(handle: StateHandle<GalleryState>, closer: DialogCloser) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let cancel = dialog_button(
        ids::DEMO_LOCAL + 10,
        l10n.dialog_cancel().to_string(),
        0,
        variant::ALERT,
        handle.clone(),
        closer.clone(),
    );
    let discard = dialog_button(
        ids::DEMO_LOCAL + 11,
        l10n.dialog_discard().to_string(),
        1,
        variant::ALERT,
        handle,
        closer,
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
fn alert_dialog_with_title(handle: StateHandle<GalleryState>, closer: DialogCloser) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let disagree = dialog_button(
        ids::DEMO_LOCAL + 10,
        l10n.dialog_disagree().to_string(),
        0,
        variant::ALERT_TITLE,
        handle.clone(),
        closer.clone(),
    );
    let agree = dialog_button(
        ids::DEMO_LOCAL + 11,
        l10n.dialog_agree().to_string(),
        1,
        variant::ALERT_TITLE,
        handle,
        closer,
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
fn simple_dialog(handle: StateHandle<GalleryState>, closer: DialogCloser) -> AnyWidget {
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
        options.push(stateful(DialogDemoItem {
            id,
            icon,
            color: *color,
            text: text.clone(),
            option,
            handle: handle.clone(),
            closer: closer.clone(),
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
    /// Which `SimpleDialogOption` this is; the tap pops with it.
    option: usize,
    handle: StateHandle<GalleryState>,
    closer: DialogCloser,
}

impl StatefulComponent for DialogDemoItem {
    /// Its own held-ness, for the reason [`DialogButton`] documents.
    type State = bool;

    fn build(
        &self,
        held: &bool,
        held_handle: StateHandle<bool>,
        _: &mut rustflutter::framework::BuildContext,
    ) -> AnyWidget {
        let id = self.id;
        let icon = self.icon;
        let color = self.color;
        let text = self.text.clone();
        let pressed = *held;
        let option = self.option;
        let tap_handle = self.handle.clone();
        let closer = self.closer.clone();
        let press_handle = held_handle;

        let handlers = rustflutter::gestures::PointerHandlers::new()
            .with_tap(move |_| {
                tap_handle.set_state(move |s| {
                    s.demo.counter = result_code(variant::SIMPLE, option);
                });
                closer.close();
            })
            .with_press_change(move |down| {
                press_handle.set_state(move |held| *held = down);
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
fn fullscreen_dialog(handle: StateHandle<GalleryState>, closer: DialogCloser) -> AnyWidget {
    let l10n = GalleryLocalizations::en();
    let _ = handle;
    // SAVE pops with nothing -- upstream's fullscreen route returns void -- so
    // the button closes and sets no result. `dialog_button` would set one.
    let save = stateful(FullscreenSave { closer });

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

/// The fullscreen page's SAVE. Its own held-ness, as the dialog buttons have.
struct FullscreenSave {
    closer: DialogCloser,
}

impl StatefulComponent for FullscreenSave {
    type State = bool;

    fn build(&self, held: &bool, held_handle: StateHandle<bool>, _: &mut BuildContext) -> AnyWidget {
        let closer = self.closer.clone();
        component(
            Button::new(
                ids::DEMO_LOCAL + 16,
                GalleryLocalizations::en().dialog_fullscreen_save(),
            )
            .with_style(ButtonVariant::Text)
            .with_pressed(*held)
            .with_handlers(
                rustflutter::gestures::PointerHandlers::new()
                    .with_tap(move |_| {
                        closer.close();
                    })
                    .with_press_change(move |down| {
                        held_handle.set_state(move |held| *held = down);
                    }),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fullscreen_page_cannot_be_tapped_away_and_the_dialogs_can() {
        // Upstream's is a `MaterialPageRoute` and the other three are
        // `DialogRoute`s. A page is left through its own SAVE; a dialog is
        // `showDialog`'s barrierDismissible default, which is true.
        for variant in [variant::ALERT, variant::ALERT_TITLE, variant::SIMPLE] {
            assert!(
                barrier_for(variant).dismissible,
                "variant {variant} is a dialog"
            );
        }
        assert!(!barrier_for(variant::FULLSCREEN).dismissible);
    }

    #[test]
    fn a_dialog_dims_what_is_behind_it_and_the_page_does_not() {
        // The scrim is what says "this is over the page"; a full-screen page
        // has no page showing to dim.
        assert_eq!(
            barrier_for(variant::ALERT).color,
            Some(DIALOG_BARRIER_COLOR)
        );
        assert_eq!(barrier_for(variant::FULLSCREEN).color, None);
        assert_eq!(DIALOG_BARRIER_COLOR.alpha(), 0x8A, "Colors.black54");
    }

    #[test]
    fn a_button_pops_with_its_own_dialogs_variant() {
        // The button carries its variant now, where it used to read the open
        // dialog's back off a shared field. Same answers, no shared field.
        assert_eq!(
            result_text(result_code(variant::ALERT_TITLE, 1)).as_deref(),
            Some("AGREE")
        );
        assert_eq!(
            result_text(result_code(variant::ALERT, 1)).as_deref(),
            Some("DISCARD"),
            "the same option index means something different per variant"
        );
    }

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
