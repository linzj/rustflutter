// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Ported from `lib/demos/material/text_field_demo.dart` (flutter/gallery @
//! d12640d), aligned with upstream.
//!
//! Upstream's `TextFieldDemo` is a `Scaffold` around `TextFormFieldDemo`, the
//! form itself. The scaffold and app bar are the demo page's chrome here
//! (`src/pages/demo.rs`); what remains is the form: eight fields, a submit
//! button and the required-field footnote, with 24 between them (upstream's
//! `sizedBoxSpace`). The form's state -- the `PersonData`, the autovalidate
//! mode, the password's obscurity -- is [`FormState`] on a per-demo
//! `StatefulComponent`, as upstream's `TextFormFieldDemoState` is per widget.
//!
//! The framework's `TextField` is an editable and nothing else: it has no
//! `InputDecoration`, so what upstream's decoration says is said here around
//! the field instead --
//!
//! - `labelText` becomes a caption above the field, `hintText` stays the
//!   field's placeholder, and `helperText`/validation errors become a note
//!   below it (the error replaces the helper, as upstream's does);
//! - `filled: true` and `OutlineInputBorder` become the box the field sits
//!   in, per field, matching which decoration upstream gave which field;
//! - the leading icons (`Icons.person`, `Icons.phone`, `Icons.email`) and the
//!   phone field's `prefixText: '+1 '` have no hook and are dropped;
//! - the salary field's `suffixText: 'USD'` is a trailing label in the box;
//! - `PasswordField`'s visibility `IconButton` becomes a text button whose
//!   label is the icon's semantic label ("Show password"/"Hide password");
//! - `maxLength: 8` on the password fields is not enforceable -- there is no
//!   length hook -- and is dropped; the helper text stays;
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

use rustflutter::framework::BuildContext;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, FlexChild, MainAxisSize, RenderFlex};
use rustflutter::widgets::Center;

use crate::app::ids;

use super::column;

/// How long the form's message stays up, in frame-clock microseconds.
/// Upstream's default `SnackBar.duration`, `_kSnackBarDisplayDuration`.
const SNACKBAR_DURATION_MICROS: i64 = 4_000_000;

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
        let fill = theme.surface_variant;
        let outline = theme.outline;
        let muted_color = theme.text_muted;
        let danger = theme.danger;
        let label_style = TextStyle {
            font_weight: 600,
            ..theme.body()
        };
        let note_style = TextStyle {
            font_size: theme.body_size - 2.0,
            ..theme.muted()
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

        let mut children: Vec<AnyWidget> = Vec::new();

        // Name. Upstream's `textCapitalization: words` has no counterpart.
        children.push(field_group(
            "Name*",
            decoration_box(
                stateful(
                    TextField::new(base)
                        .with_placeholder("What do people call you?")
                        .with_on_changed(on_changed(
                            handle.clone(),
                            |s, text| s.name = text.to_string(),
                            |e| &mut e.name,
                            |s| validate_name(&s.name),
                        )),
                ),
                true,
                fill,
                outline,
            ),
            state.errors.name.map(str::to_string),
            None,
            danger,
            label_style.clone(),
            note_style.clone(),
        ));

        // Phone number. The digits-only filter runs here; the US formatter
        // runs at validation. Upstream's `prefixText: '+1 '` is dropped.
        children.push(field_group(
            "Phone number*",
            decoration_box(
                stateful(
                    TextField::new(base + 1)
                        .with_placeholder("Where can we reach you?")
                        .with_on_changed(on_changed(
                            handle.clone(),
                            |s, text| s.phone = digits_only(text),
                            |e| &mut e.phone,
                            |s| validate_phone(&s.phone),
                        )),
                ),
                true,
                fill,
                outline,
            ),
            state.errors.phone.map(str::to_string),
            None,
            danger,
            label_style.clone(),
            note_style.clone(),
        ));

        // Email. Upstream validates nothing on it; neither does this.
        children.push(field_group(
            "Email",
            decoration_box(
                stateful(
                    TextField::new(base + 2)
                        .with_placeholder("Your email address")
                        .with_on_changed({
                            let handle = handle.clone();
                            move |text: &str| {
                                let text = text.to_string();
                                handle.set_state(move |s| s.email = text);
                            }
                        }),
                ),
                true,
                fill,
                outline,
            ),
            None,
            None,
            danger,
            label_style.clone(),
            note_style.clone(),
        ));

        // The disabled email field. `TextField` has no `enabled`, so the
        // field is its decoration and its hint, drawn muted, and takes no
        // input at all -- which is what `enabled: false` means upstream.
        children.push(field_group(
            "Email",
            decoration_box(
                leaf(move || {
                    Text::new("Your email address").with_style(TextStyle {
                        color: muted_color,
                        ..TextStyle::default()
                    })
                }),
                true,
                fill,
                outline,
            ),
            None,
            None,
            danger,
            label_style.clone(),
            note_style.clone(),
        ));

        // Life story: upstream's three-line outlined field.
        children.push(field_group(
            "Life story",
            decoration_box(
                stateful(
                    TextField::new(base + 4)
                        .with_placeholder(
                            "Tell us about yourself (e.g., write down what you do or what hobbies you have)",
                        )
                        .with_max_lines(3),
                ),
                false,
                fill,
                outline,
            ),
            None,
            Some("Keep it short, this is just a demo.".to_string()),
            danger,
            label_style.clone(),
            note_style.clone(),
        ));

        // Salary: outlined, with the "USD" suffix alongside the field.
        let usd_style = note_style.clone();
        let salary = many(
            vec![
                stateful(TextField::new(base + 5)),
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
            decoration_box(salary, false, fill, outline),
            None,
            None,
            danger,
            label_style.clone(),
            note_style.clone(),
        ));

        // Password. Upstream's `maxLength: 8` is not enforceable here. The
        // field's text is tracked as it changes: upstream's
        // `_validatePassword` reads the field's value, which is this.
        let toggle_label = if state.obscure_password {
            "Show password"
        } else {
            "Hide password"
        };
        let password_field = TextField::new(base + 6).with_on_changed({
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
        let toggle = component(
            Button::new(base + 8, toggle_label)
                .with_style(ButtonStyle::Text)
                .with_pressed(state.pressed == Some(base + 8))
                .wired(
                    handle.clone(),
                    |s| &mut s.pressed,
                    |s| {
                        s.obscure_password = !s.obscure_password;
                    },
                ),
        );
        let password_row = many(vec![password_field, toggle], |mut rendered| {
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
        children.push(field_group(
            "Password*",
            decoration_box(password_row, true, fill, outline),
            None,
            Some("No more than 8 characters.".to_string()),
            danger,
            label_style.clone(),
            note_style.clone(),
        ));

        // Re-type password: submitting it submits the form, upstream's
        // `onFieldSubmitted: (value) { _handleSubmitted(); }`.
        children.push(field_group(
            "Re-type password*",
            decoration_box(
                stateful(
                    TextField::new(base + 7)
                        .obscured()
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
                true,
                fill,
                outline,
            ),
            state.errors.retype.map(str::to_string),
            None,
            danger,
            label_style.clone(),
            note_style.clone(),
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

/// One field with its decoration: label above, note (error or helper) below.
/// Upstream's `InputDecoration` spread around the field, per the module
/// header. The error wins the note slot, as upstream's `errorText` replaces
/// `helperText`.
fn field_group(
    label: &'static str,
    field: AnyWidget,
    error: Option<String>,
    helper: Option<String>,
    danger: Color,
    label_style: TextStyle,
    note_style: TextStyle,
) -> AnyWidget {
    let note = error
        .map(|text| (text, true))
        .or(helper.map(|text| (text, false)));
    let mut rows = vec![
        leaf(move || Text::new(label).with_style(label_style.clone())),
        field,
    ];
    if let Some((text, is_error)) = note {
        let mut style = note_style.clone();
        if is_error {
            style.color = danger;
        }
        rows.push(leaf(move || {
            Text::new(text.clone()).with_style(style.clone())
        }));
    }
    many(rows, |rendered| {
        let mut flex = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(6.0);
        for row in rendered {
            flex = flex.push(row);
        }
        Box::new(flex)
    })
}

/// The box a field sits in: filled, or outlined, per the field's upstream
/// decoration.
fn decoration_box(field: AnyWidget, filled: bool, fill: Color, outline: Color) -> AnyWidget {
    single(field, move |inner| {
        let mut container = rustflutter::widgets::Container::new()
            .with_corner_radius(8.0)
            .with_padding(EdgeInsets::symmetric(12.0, 10.0));
        container = if filled {
            container.with_color(fill)
        } else {
            container.with_border(1.0, outline)
        };
        Box::new(container.with_child(inner))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
