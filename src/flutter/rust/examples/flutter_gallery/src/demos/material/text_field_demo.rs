// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/text_field_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `TextFieldDemo` is a `Scaffold` with an app bar (the
//! `demoTextFieldTitle`, no leading) around `TextFormFieldDemo`, the form
//! itself. The app bar is built here, the way `grid_list_demo.rs` builds its
//! own; what remains is the form: eight fields, a submit button and the
//! required-field footnote, with 24 between them (upstream's
//! `sizedBoxSpace`). The form's state -- the `PersonData`, the autovalidate
//! mode, the password's obscurity -- is [`FormState`] on a per-demo
//! `StatefulComponent`, as upstream's `TextFormFieldDemoState` is per widget.
//!
//! The framework's `TextField` is an editable and nothing else: it has no
//! `InputDecoration`, so what upstream's decoration says is said here around
//! the field instead:
//!
//! - `labelText` is a floated label at the top of the field's box. Upstream
//!   rests with the label in the field's middle and floats it when the field
//!   is focused or has content; there is no floating-label animation to port
//!   that with, so the label is always floated -- upstream's focused look.
//!   Its colour follows upstream's: primary while the field is focused
//!   (asked of `focus::has_focus`), red when it shows an error.
//! - `filled: true` is the Material filled field: a fill with the top
//!   corners rounded 4 and a bottom underline, which thickens and goes
//!   primary on focus, red on error. `OutlineInputBorder` is a 1px outline
//!   rounded 4, 2px and primary on focus.
//! - The leading icons (`Icons.person`, `Icons.phone`, `Icons.email`) sit
//!   beside the box as glyph text, and the phone field's `prefixText: '+1 '`
//!   shows when the field is focused or has content, as upstream's
//!   `prefixText` does. Upstream's prefix is inside the field's decoration;
//!   here it is a sibling, so the row holding the two is built whether the
//!   prefix has anything to say or not -- a row that came and went would
//!   re-parent the field, and a re-parented `TextField` loses the editing
//!   session its state holds;
//! - the salary field's `suffixText: 'USD'` is a trailing label in the box;
//! - `PasswordField`'s visibility `IconButton` is the same eye glyph inside
//!   the box, toggling the field's obscurity (its semantic label has no
//!   counterpart here);
//! - `maxLength: 8` on the password fields is not enforceable -- there is no
//!   length hook -- but its counter is, so the demo draws the `n/8` itself;
//! - `textCapitalization`, `keyboardType` and `textInputAction` have no
//!   counterparts. Focus walking is the framework's (Tab between fields);
//!   upstream's `onFieldSubmitted` focus moves and the form-level
//!   `RestorationMixin` are not carried.
//!
//! The phone field's input formatters (`FilteringTextInputFormatter.
//! digitsOnly` and `_UsNumberTextInputFormatter`) cannot rewrite the field's
//! text -- a `TextField`'s value is its own state, with no setter -- so the
//! field shows what was typed, and the ported formatter runs where its result
//! is consumed: validation and the confirmation message. The formatter itself
//! is ported verbatim as [`format_us_number`] and is unit-tested against
//! upstream's walk-through cases.
//!
//! Upstream reports the submit outcome with `ScaffoldMessenger.showSnackBar`.
//! The demo overlay channel (`mod.rs::overlay`) is wired for the `snackbars`
//! slug only, so the message rides at the bottom of the form column instead,
//! still leaving on its own after upstream's four-second
//! `_kSnackBarDisplayDuration`.

use rustflutter::borders::{BorderRadius, Radius};
use rustflutter::components::K_TOOLBAR_HEIGHT;
use rustflutter::framework::BuildContext;
use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::{Align, Center, Pointer};

use crate::app::ids;
use crate::data::demos as catalog;
use crate::l10n::gallery_localizations::GalleryLocalizations;
use crate::themes::material_demo_theme_data::MaterialDemoThemeData;

use super::column;

/// How long the form's message stays up, in frame-clock microseconds.
/// Upstream's default `SnackBar.duration`, `_kSnackBarDisplayDuration`.
const SNACKBAR_DURATION_MICROS: i64 = 4_000_000;

/// The floated label's size, upstream's `bodySmall` the decorator shrinks to.
const LABEL_SIZE: f32 = 12.0;

/// Whether the message shown at `shown_micros` has served its time.
fn should_dismiss(shown_micros: i64, frame_time_micros: i64) -> bool {
    frame_time_micros - shown_micros >= SNACKBAR_DURATION_MICROS
}

/// The demo body: upstream's `TextFormFieldDemo`.
pub(super) fn stage() -> AnyWidget {
    stateful(TextFormFieldDemo)
}

struct TextFormFieldDemo;

/// Upstream's `TextFormFieldDemoState`: the `PersonData`, the validators'
/// answers, and the autovalidate mode.
struct FormState {
    /// `PersonData.name`, tracked live from the field (upstream's `onSaved`).
    name: String,
    /// `PersonData.phoneNumber`, digits only: upstream's
    /// `FilteringTextInputFormatter.digitsOnly` applied where the text
    /// arrives, since the field itself cannot be formatted.
    phone: String,
    /// `PersonData.email`.
    email: String,
    /// `PersonData.password`, tracked live from the password field; upstream
    /// reads it off the field's own state in `_validatePassword`, which is
    /// the same value at the same time.
    password: String,
    /// The re-type field's text, for its validator.
    retype: String,
    /// `PasswordField`'s `_obscureText`.
    obscure_password: bool,
    /// `_autoValidateModeIndex`: false is `AutovalidateMode.disabled`, true is
    /// `always` (the only two states upstream's `_handleSubmitted` uses).
    autovalidate: bool,
    errors: FormErrors,
    /// The submit outcome's message, while it is up.
    snackbar: Option<String>,
    snackbar_shown_micros: Option<i64>,
    /// Which control is held down, for the pressed look. The buttons' `wired`
    /// wants a field to point at.
    pressed: Option<u64>,
}

impl Default for FormState {
    fn default() -> FormState {
        FormState {
            name: String::new(),
            phone: String::new(),
            email: String::new(),
            password: String::new(),
            retype: String::new(),
            obscure_password: true,
            autovalidate: false,
            errors: FormErrors::default(),
            snackbar: None,
            snackbar_shown_micros: None,
            pressed: None,
        }
    }
}

/// The three validated fields' current errors.
#[derive(Default)]
struct FormErrors {
    name: Option<&'static str>,
    phone: Option<&'static str>,
    retype: Option<&'static str>,
}

impl FormErrors {
    fn any(&self) -> bool {
        self.name.is_some() || self.phone.is_some() || self.retype.is_some()
    }
}

impl FormState {
    /// Upstream's `Form.validate()`: every validator runs, and every error is
    /// shown at once.
    fn validate(&mut self) {
        self.errors = FormErrors {
            name: validate_name(&self.name),
            phone: validate_phone(&self.phone),
            retype: validate_retype(&self.password, &self.retype),
        };
    }

    /// Upstream's `_handleSubmitted`.
    fn handle_submitted(&mut self) {
        self.validate();
        if self.errors.any() {
            // "Start validating on every change."
            self.autovalidate = true;
            self.show_snackbar("Please fix the errors in red before submitting.");
        } else {
            // Upstream's `demoTextFieldNameHasPhoneNumber`, with the phone
            // number formatted the way the field would have shown it.
            let message = format!(
                "{} phone number is {}",
                self.name,
                format_us_number(&self.phone)
            );
            self.show_snackbar(message);
        }
    }

    fn show_snackbar(&mut self, message: impl Into<String>) {
        self.snackbar = Some(message.into());
        self.snackbar_shown_micros = None;
    }
}

/// Upstream's `_validateName`: `RegExp(r'^[A-Za-z ]+$')`.
fn validate_name(value: &str) -> Option<&'static str> {
    if value.is_empty() {
        return Some("Name is required.");
    }
    if !value.chars().all(|c| c.is_ascii_alphabetic() || c == ' ') {
        return Some("Please enter only alphabetical characters.");
    }
    None
}

/// Upstream's `FilteringTextInputFormatter.digitsOnly`.
fn digits_only(value: &str) -> String {
    value.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Formats incoming numeric text to fit the format of `(###) ###-#### ##`.
///
/// A verbatim port of `_UsNumberTextInputFormatter.formatEditUpdate`, minus
/// the selection tracking: this port consumes the formatted text, never its
/// caret. The input is expected to be digits only, which is what the
/// digits-only filter upstream applies first guarantees.
fn format_us_number(digits: &str) -> String {
    let length = digits.len();
    let mut formatted = String::new();
    let mut used = 0;
    if length >= 1 {
        formatted.push('(');
    }
    if length >= 4 {
        formatted.push_str(&digits[0..3]);
        formatted.push_str(") ");
        used = 3;
    }
    if length >= 7 {
        formatted.push_str(&digits[3..6]);
        formatted.push('-');
        used = 6;
    }
    if length >= 11 {
        formatted.push_str(&digits[6..10]);
        formatted.push(' ');
        used = 10;
    }
    // Dump the rest.
    if length >= used {
        formatted.push_str(&digits[used..]);
    }
    formatted
}

/// Whether `formatted` matches `^\(\d\d\d\) \d\d\d\-\d\d\d\d$`, the pattern
/// upstream's `_validatePhoneNumber` matches against.
fn is_us_phone(formatted: &str) -> bool {
    let bytes = formatted.as_bytes();
    if bytes.len() != 14 {
        return false;
    }
    let digit_at = |mut range: std::ops::Range<usize>| range.all(|i| bytes[i].is_ascii_digit());
    bytes[0] == b'('
        && digit_at(1..4)
        && bytes[4] == b')'
        && bytes[5] == b' '
        && digit_at(6..9)
        && bytes[9] == b'-'
        && digit_at(10..14)
}

/// Upstream's `_validatePhoneNumber`, over the digits the field received.
fn validate_phone(digits: &str) -> Option<&'static str> {
    if !is_us_phone(&format_us_number(digits)) {
        return Some("(###) ###-#### - Enter a US phone number.");
    }
    None
}

/// Upstream's `_validatePassword`: the re-type field's validator, reading the
/// password field's value.
fn validate_retype(password: &str, retype: &str) -> Option<&'static str> {
    if password.is_empty() {
        return Some("Please enter a password.");
    }
    if password != retype {
        return Some("The passwords don't match");
    }
    None
}

/// The decoration's palette, resolved once per build.
#[derive(Clone, Copy)]
struct FieldColors {
    fill: Color,
    outline: Color,
    muted: Color,
    primary: Color,
    danger: Color,
}

/// The two `InputDecoration` shapes the form uses: `filled: true`, or
/// `border: OutlineInputBorder()`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoxKind {
    Filled,
    Outlined,
}

/// A Material-icons glyph as text, the demo bar's trick.
fn material_icon(glyph: &'static str, color: Color) -> AnyWidget {
    leaf(move || {
        Text::new(glyph)
            .with_font_family(catalog::MATERIAL_ICONS)
            .with_size(24.0)
            .with_color(color)
    })
}

/// One field with its decoration: the label floated inside the box, the
/// leading icon beside it, the note (error or helper) and the character
/// counter below. The error wins the note slot, as upstream's `errorText`
/// replaces `helperText`.
fn field_group(
    label: &'static str,
    icon: Option<&'static str>,
    content: AnyWidget,
    kind: BoxKind,
    focused: bool,
    error: Option<String>,
    helper: Option<String>,
    counter: Option<String>,
    colors: FieldColors,
) -> AnyWidget {
    // The label's colour answers focus and error before anything else does,
    // upstream's `InputDecorator`'s labelStyle resolution.
    let label_color = if error.is_some() {
        colors.danger
    } else if focused {
        colors.primary
    } else {
        colors.muted
    };
    let floated = leaf(move || {
        Text::new(label)
            .with_size(LABEL_SIZE)
            .with_color(label_color)
    });
    let inner = column(vec![floated, content], 4.0);

    // The box. Filled is a fill with the top corners rounded and an
    // underline flush beneath (the two are one shape upstream, split here
    // because a `Container` border is all four sides); outlined is a border
    // rounded all round.
    let edge_color = if error.is_some() {
        colors.danger
    } else if focused {
        colors.primary
    } else {
        colors.outline
    };
    let boxed = single(inner, move |inner| {
        let container = Container::new().with_padding(EdgeInsets::symmetric(12.0, 8.0));
        let container = match kind {
            BoxKind::Filled => container
                .with_color(colors.fill)
                .with_border_radius(BorderRadius::vertical(Radius::circular(4.0), Radius::ZERO)),
            BoxKind::Outlined => container
                .with_border(if focused { 2.0 } else { 1.0 }, edge_color)
                .with_border_radius(BorderRadius::circular(4.0)),
        };
        Box::new(container.with_child(inner))
    });
    let decorated = if kind == BoxKind::Filled {
        let underline_color = edge_color;
        let underline = leaf(move || {
            Container::new()
                .with_height(if focused { 2.0 } else { 1.0 })
                .with_color(underline_color)
        });
        column(vec![boxed, underline], 0.0)
    } else {
        boxed
    };

    // The leading icon, centred against the box as upstream's `icon:` is.
    let body = if let Some(glyph) = icon {
        let icon_color = colors.muted;
        many(
            vec![material_icon(glyph, icon_color), decorated],
            |mut rendered| {
                let field = rendered.pop().expect("the field");
                let icon = rendered.pop().expect("the icon");
                Box::new(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(16.0)
                        .push(icon)
                        .push_flex(FlexChild::expanded(field, 1)),
                )
            },
        )
    } else {
        decorated
    };

    // Below the box: the note on the left, the counter on the right --
    // upstream's helper/error and counter share the decorator's subtext row.
    //
    // The row is built whether it has anything to say or not, the way the
    // phone field's prefix row is and for the same reason: a row that came
    // and went would re-parent the field above it, and a re-parented
    // `TextField` loses the editing session its state holds. An error
    // arriving on submit is exactly such a moment -- upstream's
    // `Form.validate()` shows every error without touching the fields' text,
    // so this tree's shape may not answer the errors either.
    let note = error
        .map(|text| (text, true))
        .or(helper.map(|text| (text, false)));
    let note_widget = match note {
        Some((text, is_error)) => {
            let color = if is_error {
                colors.danger
            } else {
                colors.muted
            };
            leaf(move || {
                Text::new(text.clone())
                    .with_size(LABEL_SIZE)
                    .with_color(color)
            })
        }
        None => leaf(Container::new),
    };
    let mut note_row = Vec::new();
    note_row.push(note_widget);
    if let Some(counter) = counter {
        let muted = colors.muted;
        note_row.push(leaf(move || {
            Text::new(counter.clone())
                .with_size(LABEL_SIZE)
                .with_color(muted)
        }));
    }
    let note_row = many(note_row, |mut rendered| {
        let counter = if rendered.len() > 1 {
            Some(rendered.pop().expect("the counter"))
        } else {
            None
        };
        let note = rendered.pop().expect("the note");
        let mut flex = RenderFlex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(8.0)
            .push_flex(FlexChild::expanded(note, 1));
        if let Some(counter) = counter {
            flex = flex.push(counter);
        }
        Box::new(flex)
    });
    column(vec![body, note_row], 4.0)
}

impl StatefulComponent for TextFormFieldDemo {
    type State = FormState;

    fn advance(&self, state: &mut FormState, frame_time_micros: i64) -> bool {
        // The message's four seconds, on the frame clock. Only a live message
        // has a clock to run.
        if state.snackbar.is_none() {
            return false;
        }
        let shown = *state.snackbar_shown_micros.get_or_insert(frame_time_micros);
        if should_dismiss(shown, frame_time_micros) {
            state.snackbar = None;
            state.snackbar_shown_micros = None;
        }
        true
    }

    fn build(
        &self,
        state: &FormState,
        handle: StateHandle<FormState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let base = ids::DEMO_LOCAL;
        let colors = FieldColors {
            fill: theme.surface_variant,
            outline: theme.outline,
            muted: theme.text_muted,
            primary: theme.primary,
            danger: theme.danger,
        };
        let note_style = TextStyle {
            font_size: theme.body_size - 2.0,
            ..theme.muted()
        };
        let text_style = {
            let mut style = theme.body();
            style.color = theme.text;
            style
        };

        // A change handler that also re-validates, once a failed submit has
        // switched the form to `AutovalidateMode.always`.
        let on_changed =
            |handle: StateHandle<FormState>,
             store: fn(&mut FormState, &str),
             refresh: fn(&mut FormErrors) -> &mut Option<&'static str>,
             validate: fn(&FormState) -> Option<&'static str>| {
                move |text: &str| {
                    // `set_state` wants a 'static closure; the text crosses as an
                    // owned copy.
                    let text = text.to_string();
                    handle.set_state(move |s| {
                        store(s, &text);
                        if s.autovalidate {
                            *refresh(&mut s.errors) = validate(s);
                        }
                    });
                }
            };

        // A focus listener that rebuilds the form: the decoration answers
        // focus (the label and underline go primary), and the field's own
        // session machinery marks only the field dirty.
        let rebuild_on_focus = |handle: &StateHandle<FormState>| {
            let handle = handle.clone();
            move |_| {
                handle.set_state(|_| {});
            }
        };

        let mut children: Vec<AnyWidget> = Vec::new();

        // Upstream's app bar: `AppBar(automaticallyImplyLeading: false,
        // title: Text(demoTextFieldTitle))`, on the demo theme's app-bar
        // colours -- the same bar `grid_list_demo.rs` builds.
        let (bar_fill, bar_ink) = MaterialDemoThemeData::app_bar_theme();
        let title = GalleryLocalizations::en().demo_text_field_title();
        children.push(leaf(move || {
            Container::new()
                .with_height(K_TOOLBAR_HEIGHT)
                .with_color(bar_fill)
                .with_padding(EdgeInsets::only(16.0, 0.0, 0.0, 0.0))
                .with_child(Align::new(
                    Alignment::CENTER_LEFT,
                    Text::new(title)
                        .with_size(20.0)
                        .with_weight(500)
                        .with_color(bar_ink),
                ))
        }));

        // Name. Upstream's `textCapitalization: words` has no counterpart.
        children.push(field_group(
            "Name*",
            Some(catalog::icon::PERSON),
            stateful(
                TextField::new(base)
                    .with_placeholder("What do people call you?")
                    .with_on_focus_change(rebuild_on_focus(&handle))
                    .with_on_changed(on_changed(
                        handle.clone(),
                        |s, text| s.name = text.to_string(),
                        |e| &mut e.name,
                        |s| validate_name(&s.name),
                    )),
            ),
            BoxKind::Filled,
            rustflutter::focus::has_focus(base),
            state.errors.name.map(str::to_string),
            None,
            None,
            colors,
        ));

        // Phone number. The digits-only filter runs here; the US formatter
        // runs at validation. Upstream's `prefixText: '+1 '` shows with the
        // field focused or filled.
        let phone_field = stateful(
            TextField::new(base + 1)
                .with_placeholder("Where can we reach you?")
                .with_on_focus_change(rebuild_on_focus(&handle))
                .with_on_changed(on_changed(
                    handle.clone(),
                    |s, text| s.phone = digits_only(text),
                    |e| &mut e.phone,
                    |s| validate_phone(&s.phone),
                )),
        );
        let phone_focused = rustflutter::focus::has_focus(base + 1);
        // The prefix comes and goes, but the row it sits in does not: it is
        // the prefix's *text* that empties, not the row that disappears.
        //
        // Upstream's `prefixText` lives inside the field's own
        // `InputDecoration`, so showing it moves nothing. Here the prefix is
        // a sibling, and a sibling that appears re-parents the field -- into
        // this row on focus, out of it again on blur. A re-parented widget is
        // a new element with a new `TextFieldState`, and a `TextField`'s
        // editing session lives in that state: the field would take the
        // keyboard, open a session, and then be rebuilt out from under the
        // session in the very frame the focus asked for. Focus said the field
        // had the keyboard; nothing could be typed into it.
        let prefix_text = if phone_focused || !state.phone.is_empty() {
            "+1 "
        } else {
            ""
        };
        let prefix_style = text_style.clone();
        let phone_content: AnyWidget = many(
            vec![
                leaf(move || Text::new(prefix_text).with_style(prefix_style.clone())),
                phone_field,
            ],
            |mut rendered| {
                let field = rendered.pop().expect("the field");
                let prefix = rendered.pop().expect("the prefix");
                Box::new(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .push(prefix)
                        .push_flex(FlexChild::expanded(field, 1)),
                )
            },
        );
        children.push(field_group(
            "Phone number*",
            Some(catalog::icon::PHONE),
            phone_content,
            BoxKind::Filled,
            phone_focused,
            state.errors.phone.map(str::to_string),
            None,
            None,
            colors,
        ));

        // Email. Upstream validates nothing on it; neither does this.
        children.push(field_group(
            "Email",
            Some(catalog::icon::EMAIL),
            stateful(
                TextField::new(base + 2)
                    .with_placeholder("Your email address")
                    .with_on_focus_change(rebuild_on_focus(&handle))
                    .with_on_changed({
                        let handle = handle.clone();
                        move |text: &str| {
                            let text = text.to_string();
                            handle.set_state(move |s| s.email = text);
                        }
                    }),
            ),
            BoxKind::Filled,
            rustflutter::focus::has_focus(base + 2),
            None,
            None,
            None,
            colors,
        ));

        // The disabled email field. `TextField` has no `enabled`, so the
        // field is its decoration and its hint, drawn muted, and takes no
        // input at all -- which is what `enabled: false` means upstream.
        let muted_hint = note_style.clone();
        children.push(field_group(
            "Email",
            Some(catalog::icon::EMAIL),
            leaf(move || Text::new("Your email address").with_style(muted_hint.clone())),
            BoxKind::Filled,
            false,
            None,
            None,
            None,
            colors,
        ));

        // Life story: upstream's three-line outlined field.
        children.push(field_group(
            "Life story",
            None,
            stateful(
                TextField::new(base + 4)
                    .with_placeholder(
                        "Tell us about yourself (e.g., write down what you do or what hobbies you have)",
                    )
                    .with_on_focus_change(rebuild_on_focus(&handle))
                    .with_max_lines(3),
            ),
            BoxKind::Outlined,
            rustflutter::focus::has_focus(base + 4),
            None,
            Some("Keep it short, this is just a demo.".to_string()),
            None,
            colors,
        ));

        // Salary: outlined, with the "USD" suffix alongside the field.
        let usd_style = note_style.clone();
        let salary = many(
            vec![
                stateful(TextField::new(base + 5).with_on_focus_change(rebuild_on_focus(&handle))),
                leaf(move || Text::new("USD").with_style(usd_style.clone())),
            ],
            |mut rendered| {
                let suffix = rendered.pop().expect("the suffix");
                let field = rendered.pop().expect("the field");
                Box::new(
                    RenderFlex::row()
                        .with_main_axis_size(MainAxisSize::Max)
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(8.0)
                        .push_flex(FlexChild::expanded(field, 1))
                        .push(suffix),
                )
            },
        );
        children.push(field_group(
            "Salary",
            None,
            salary,
            BoxKind::Outlined,
            rustflutter::focus::has_focus(base + 5),
            None,
            None,
            None,
            colors,
        ));

        // Password. Upstream's `maxLength: 8` is not enforceable here, but
        // its counter is drawn below. The field's text is tracked as it
        // changes: upstream's `_validatePassword` reads the field's value,
        // which is this. The visibility `IconButton` is the eye glyph inside
        // the box.
        let password_field = TextField::new(base + 6)
            .with_on_focus_change(rebuild_on_focus(&handle))
            .with_on_changed({
                let handle = handle.clone();
                move |text: &str| {
                    let text = text.to_string();
                    handle.set_state(move |s| s.password = text);
                }
            });
        let password_field: AnyWidget = if state.obscure_password {
            stateful(password_field.obscured())
        } else {
            stateful(password_field)
        };
        let toggle_glyph = if state.obscure_password {
            catalog::icon::VISIBILITY
        } else {
            catalog::icon::VISIBILITY_OFF
        };
        let toggle_color = colors.muted;
        let toggle = leaf({
            let handle = handle.clone();
            move || {
                Pointer::new(
                    base + 8,
                    Container::new()
                        .with_size(40.0, 40.0)
                        .with_child(Align::new(
                            Alignment::CENTER,
                            Text::new(toggle_glyph)
                                .with_font_family(catalog::MATERIAL_ICONS)
                                .with_size(24.0)
                                .with_color(toggle_color),
                        )),
                )
                .with_handlers(PointerHandlers::new().with_tap({
                    let handle = handle.clone();
                    move |_| {
                        handle.set_state(|s| {
                            s.obscure_password = !s.obscure_password;
                        });
                    }
                }))
            }
        });
        let password_content = many(vec![password_field, toggle], |mut rendered| {
            let toggle = rendered.pop().expect("the toggle");
            let field = rendered.pop().expect("the field");
            Box::new(
                RenderFlex::row()
                    .with_main_axis_size(MainAxisSize::Max)
                    .with_cross_axis_alignment(CrossAxisAlignment::Center)
                    .with_spacing(8.0)
                    .push_flex(FlexChild::expanded(field, 1))
                    .push(toggle),
            )
        });
        let password_counter = format!("{}/8", state.password.chars().count());
        children.push(field_group(
            "Password*",
            None,
            password_content,
            BoxKind::Filled,
            rustflutter::focus::has_focus(base + 6),
            None,
            Some("No more than 8 characters.".to_string()),
            Some(password_counter),
            colors,
        ));

        // Re-type password: submitting it submits the form, upstream's
        // `onFieldSubmitted: (value) { _handleSubmitted(); }`.
        let retype_counter = format!("{}/8", state.retype.chars().count());
        children.push(field_group(
            "Re-type password*",
            None,
            stateful(
                TextField::new(base + 7)
                    .obscured()
                    .with_on_focus_change(rebuild_on_focus(&handle))
                    .with_on_changed(on_changed(
                        handle.clone(),
                        |s, text| s.retype = text.to_string(),
                        |e| &mut e.retype,
                        |s| validate_retype(&s.password, &s.retype),
                    ))
                    .with_on_submitted({
                        let handle = handle.clone();
                        move |_text: &str| {
                            handle.set_state(|s| s.handle_submitted());
                        }
                    }),
            ),
            BoxKind::Filled,
            rustflutter::focus::has_focus(base + 7),
            state.errors.retype.map(str::to_string),
            None,
            Some(retype_counter),
            colors,
        ));

        // Submit.
        children.push(single(
            component(
                Button::new(base + 9, "SUBMIT")
                    .with_pressed(state.pressed == Some(base + 9))
                    .wired(handle.clone(), |s| &mut s.pressed, |s| s.handle_submitted()),
            ),
            |button| Box::new(Center::new(button)),
        ));

        // The required-field footnote, upstream's `bodySmall`.
        let footnote_style = note_style.clone();
        children.push(single(
            leaf(move || {
                Text::new("* indicates required field").with_style(footnote_style.clone())
            }),
            |text| Box::new(Center::new(text)),
        ));

        // The submit outcome, while it is up.
        if let Some(message) = &state.snackbar {
            children.push(component(Snackbar::new(base + 10, message.clone())));
        }

        column(children, 24.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustflutter::editable::TextFieldState;
    use rustflutter::framework::{ElementId, ElementTree};

    /// Every element holding a `TextFieldState`, in tree order: the seven
    /// editable fields, name first.
    fn field_elements(tree: &ElementTree) -> Vec<ElementId> {
        let mut found = Vec::new();
        let mut stack: Vec<ElementId> = tree.root().into_iter().collect();
        while let Some(id) = stack.pop() {
            if tree.state::<TextFieldState, _>(id, |_| ()).is_some() {
                found.push(id);
            }
            let mut children = tree.children_of(id);
            children.reverse();
            stack.extend(children);
        }
        found
    }

    /// Mounts the demo and settles it: one build, then the render tree, which
    /// is what registers the focus nodes.
    fn mounted() -> ElementTree {
        let mut tree = ElementTree::new();
        tree.rebuild(provide(Theme::dark(), stage()));
        let _ = tree.build_render_tree();
        tree
    }

    #[test]
    fn focusing_the_phone_field_keeps_the_field_that_was_focused() {
        // The bug this guards: the phone field grows a `+1 ` prefix when it
        // takes the keyboard, and the prefix used to arrive by *wrapping the
        // field in a row*. That moves the field to a new parent, so its
        // element is dropped and rebuilt -- and a rebuilt `TextField` has a
        // fresh `TextFieldState`, with no editing session in it. Focus said
        // the field had the keyboard; nothing could be typed into it.
        let mut tree = mounted();
        let before = field_elements(&tree);
        assert_eq!(before.len(), 7, "seven editable fields");

        let phone = ids::DEMO_LOCAL + 1;
        assert!(rustflutter::focus::focus(phone), "the phone field focuses");
        tree.rebuild_dirty();
        let _ = tree.build_render_tree();

        let after = field_elements(&tree);
        assert_eq!(
            after.get(1),
            before.get(1),
            "the focused phone field is the same element it was"
        );
        assert_eq!(
            rustflutter::focus::focused(),
            Some(phone),
            "and it still holds the keyboard"
        );
    }

    #[test]
    fn digits_only_drops_everything_but_digits() {
        assert_eq!(digits_only("(123) 456-7890"), "1234567890");
        assert_eq!(digits_only(""), "");
        assert_eq!(digits_only("abc"), "");
    }

    #[test]
    fn the_us_number_formatter_builds_up_as_digits_arrive() {
        // The walk-through cases of upstream's
        // `_UsNumberTextInputFormatter.formatEditUpdate`.
        assert_eq!(format_us_number(""), "");
        assert_eq!(format_us_number("1"), "(1");
        assert_eq!(format_us_number("123"), "(123");
        assert_eq!(format_us_number("1234"), "(123) 4");
        assert_eq!(format_us_number("123456"), "(123) 456");
        assert_eq!(format_us_number("1234567"), "(123) 456-7");
        assert_eq!(format_us_number("1234567890"), "(123) 456-7890");
        // Past ten digits the rest is dumped after a space.
        assert_eq!(format_us_number("12345678901"), "(123) 456-7890 1");
    }

    #[test]
    fn a_full_us_phone_number_matches_the_pattern() {
        assert!(is_us_phone("(123) 456-7890"));
        assert!(!is_us_phone("(123) 456-789"));
        assert!(!is_us_phone("(123) 456-78901"));
        assert!(!is_us_phone("123-456-7890"));
        assert!(!is_us_phone("(abc) def-ghij"));
    }

    #[test]
    fn the_name_validator_is_upstream_regexp() {
        assert_eq!(validate_name(""), Some("Name is required."));
        assert_eq!(
            validate_name("Agent 47"),
            Some("Please enter only alphabetical characters.")
        );
        assert_eq!(validate_name("Jane Doe"), None);
    }

    #[test]
    fn the_phone_validator_runs_the_formatter_first() {
        assert_eq!(validate_phone("1234567890"), None);
        assert_eq!(
            validate_phone("123"),
            Some("(###) ###-#### - Enter a US phone number.")
        );
        assert_eq!(
            validate_phone(""),
            Some("(###) ###-#### - Enter a US phone number.")
        );
    }

    #[test]
    fn the_retype_validator_reads_the_password() {
        assert_eq!(validate_retype("", ""), Some("Please enter a password."));
        assert_eq!(
            validate_retype("hunter2", "hunter3"),
            Some("The passwords don't match")
        );
        assert_eq!(validate_retype("hunter2", "hunter2"), None);
    }

    #[test]
    fn a_failed_submit_switches_autovalidate_on_and_a_passing_one_saves() {
        let mut form = FormState::default();
        form.handle_submitted();
        assert!(form.autovalidate);
        assert!(form.errors.any());
        assert_eq!(
            form.snackbar.as_deref(),
            Some("Please fix the errors in red before submitting.")
        );

        let mut form = FormState::default();
        form.name = "Jane Doe".to_string();
        form.phone = "1234567890".to_string();
        form.password = "hunter2".to_string();
        form.retype = "hunter2".to_string();
        form.handle_submitted();
        assert!(!form.autovalidate);
        assert!(!form.errors.any());
        assert_eq!(
            form.snackbar.as_deref(),
            Some("Jane Doe phone number is (123) 456-7890")
        );
    }

    #[test]
    fn the_message_dismisses_after_four_seconds() {
        assert!(!should_dismiss(
            1_000_000,
            1_000_000 + SNACKBAR_DURATION_MICROS - 1
        ));
        assert!(should_dismiss(
            1_000_000,
            1_000_000 + SNACKBAR_DURATION_MICROS
        ));
    }

    #[test]
    fn an_error_arriving_does_not_remount_the_field_it_describes() {
        // The bug this guards: a failed submit shows the name and phone
        // fields' errors, and the note row used to *arrive* with the error --
        // a new widget in the slot the field's group held, so the element
        // tree dropped the subtree and remounted it, and the remounted
        // `TextField` had a fresh `TextFieldState`: what the reader had typed
        // was gone. Upstream's `Form.validate()` shows every error without
        // touching the fields' text, so the row is always there and the
        // tree's shape never answers the errors.
        let theme = Theme::dark();
        let colors = FieldColors {
            fill: theme.surface_variant,
            outline: theme.outline,
            muted: theme.text_muted,
            primary: theme.primary,
            danger: theme.danger,
        };
        let build = |error: Option<String>| {
            field_group(
                "Name*",
                None,
                stateful(TextField::new(ids::DEMO_LOCAL)),
                BoxKind::Filled,
                false,
                error,
                None,
                None,
                colors,
            )
        };

        let mut tree = ElementTree::new();
        tree.rebuild(build(None));
        let before = field_elements(&tree);
        tree.rebuild(build(Some("Name is required.".to_string())));
        let after = field_elements(&tree);
        assert_eq!(before.len(), 1, "one field");
        assert_eq!(
            before, after,
            "the error arriving remounted nothing, so the field keeps its text"
        );
    }
}
