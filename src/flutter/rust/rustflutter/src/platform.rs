// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! What the platform says about itself: the user's settings and their locales.
//!
//! Upstream this is the part of `PlatformDispatcher` that is *not* a channel.
//! `flutter/settings` and `flutter/localization` do arrive as platform
//! messages, but `Engine` takes them on the way past -- see
//! `Engine::HandleSettingsPlatformMessage` and
//! `HandleLocalizationPlatformMessage` -- and hands the contents to the
//! framework directly. A `MethodChannel` never sees either one, in Flutter or
//! here, which is why this module is next to [`crate::services`] rather than
//! inside it.
//!
//! Three settings and a list of locales, and each of them is something an
//! application is expected to obey rather than override:
//!
//! * **`platform_brightness`** is the reader's light/dark choice. [`Theme`]
//!   already has both, so this is one line in an application:
//!   `if platform::brightness() == Brightness::Dark { Theme::dark() }`.
//! * **`text_scale_factor`** is an accessibility setting. A reader who has
//!   asked for larger text has asked every application for it.
//! * **`always_use_24_hour_format`** is a formatting preference, and the one
//!   that is genuinely regional rather than personal.
//!
//! [`Theme`]: crate::components::Theme
//!
//! # Reading them
//!
//! [`user_settings`] and [`locales`] answer with whatever the platform has said
//! so far, which before the platform has said anything is a documented default
//! rather than an error: a framework that could not lay anything out until the
//! settings arrived would have nothing to show on the first frame.
//!
//! # Being told they changed
//!
//! [`on_settings_changed`] and [`on_locales_changed`]. The shell already asks
//! for a frame when either changes -- `Engine::HandleSettingsPlatformMessage`
//! calls `ScheduleFrame` for exactly this reason -- so a widget that reads the
//! value in `build` needs no callback at all. The callbacks are for the work
//! that is not a rebuild: reloading a translation table, say.

use std::cell::RefCell;

use crate::services::codec::{JsonMessageCodec, MessageCodec, Value};

/// Light or dark, as the reader has asked for it.
///
/// Upstream's `ui.Brightness`. Two values and no "unknown": a platform that
/// cannot say is light, because that is what a platform that has never heard of
/// dark mode is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Brightness {
    Light,
    Dark,
}

impl Brightness {
    /// Upstream's `toString()` on the enum, which is what the text input
    /// channel carries for `keyboardAppearance`. Not the bare `light` that
    /// `flutter/settings` uses -- two channels, two spellings of the same
    /// value, and both are the platform's to insist on.
    pub fn as_name(self) -> &'static str {
        match self {
            Brightness::Light => "Brightness.light",
            Brightness::Dark => "Brightness.dark",
        }
    }

    /// The name `flutter/settings` uses.
    fn from_message(name: &str) -> Option<Brightness> {
        match name {
            "light" => Some(Brightness::Light),
            "dark" => Some(Brightness::Dark),
            _ => None,
        }
    }
}

/// The three things `flutter/settings` carries.
///
/// Upstream these are three separate fields on `PlatformDispatcher` with three
/// separate change callbacks. They are one struct here because they arrive in
/// one message and there is nothing to be gained from pretending otherwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UserSettings {
    /// What every font size should be multiplied by. One means unscaled.
    pub text_scale_factor: f64,
    /// Whether times should be written 13:00 rather than 1:00 PM.
    pub always_use_24_hour_format: bool,
    /// Light or dark.
    pub platform_brightness: Brightness,
}

impl Default for UserSettings {
    /// What the framework runs on until the platform says otherwise.
    ///
    /// Upstream's defaults, and they are the neutral ones on purpose: unscaled
    /// text, 12-hour time, light. A framework that guessed dark and was wrong
    /// would show one frame of white-on-white.
    fn default() -> UserSettings {
        UserSettings {
            text_scale_factor: 1.0,
            always_use_24_hour_format: false,
            platform_brightness: Brightness::Light,
        }
    }
}

/// One of the reader's languages.
///
/// Upstream's `ui.Locale`. Only the language code is required; the other three
/// are empty far more often than not, and an empty one is [`None`] here rather
/// than `""` so that "no country" cannot be confused with a country whose code
/// is the empty string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Locale {
    /// Two or three lowercase letters: `"en"`, `"zh"`.
    pub language_code: String,
    /// Two uppercase letters or three digits: `"US"`, `"CN"`.
    pub country_code: Option<String>,
    /// Four letters, first capitalised: `"Hans"`, `"Latn"`.
    pub script_code: Option<String>,
    /// Anything further the platform wanted to say.
    pub variant_code: Option<String>,
}

impl Locale {
    /// A locale with nothing but a language.
    pub fn new(language_code: &str) -> Locale {
        Locale {
            language_code: language_code.to_string(),
            ..Locale::default()
        }
    }

    /// The BCP 47 tag, which is what a translation table is usually keyed by.
    ///
    /// Upstream's `Locale.toLanguageTag`: the parts that are present, joined
    /// with hyphens, in the order the standard puts them.
    pub fn to_language_tag(&self) -> String {
        let mut tag = self.language_code.clone();
        for part in [&self.script_code, &self.country_code, &self.variant_code] {
            if let Some(part) = part {
                tag.push('-');
                tag.push_str(part);
            }
        }
        tag
    }
}

type SettingsHandler = Box<dyn FnMut(&UserSettings)>;
type FieldHandler = Box<dyn FnMut(&UserSettings)>;
type LocalesHandler = Box<dyn FnMut(&[Locale])>;

/// The platform state, and who is watching it.
///
/// Thread-local for the same reason [`crate::services`] is: this is UI-thread
/// state, and the shell only ever writes it from the UI thread.
#[derive(Default)]
struct Platform {
    settings: UserSettings,
    locales: Vec<Locale>,
    settings_handler: Option<SettingsHandler>,
    text_scale_handler: Option<FieldHandler>,
    brightness_handler: Option<FieldHandler>,
    locales_handler: Option<LocalesHandler>,
}

thread_local! {
    static PLATFORM: RefCell<Platform> = RefCell::new(Platform::default());
}

/// Everything the platform has said about the user's settings.
pub fn user_settings() -> UserSettings {
    PLATFORM.with(|platform| platform.borrow().settings)
}

/// The reader's light or dark choice. Shorthand for the common read.
pub fn brightness() -> Brightness {
    user_settings().platform_brightness
}

/// What every font size should be multiplied by.
pub fn text_scale_factor() -> f64 {
    user_settings().text_scale_factor
}

/// The reader's languages, most preferred first.
///
/// Empty until the platform has said, which on a shell that never says is
/// forever -- so a caller that needs an answer should use [`locale`], which has
/// a documented fallback.
pub fn locales() -> Vec<Locale> {
    PLATFORM.with(|platform| platform.borrow().locales.clone())
}

/// The reader's most preferred language.
///
/// Upstream's `PlatformDispatcher.locale`, including the fallback: a platform
/// that has told us nothing is treated as `en`, because a framework with no
/// locale at all cannot format a date.
pub fn locale() -> Locale {
    PLATFORM.with(|platform| {
        platform
            .borrow()
            .locales
            .first()
            .cloned()
            .unwrap_or_else(|| Locale::new("en"))
    })
}

/// Called when **anything** in the platform's settings changes, and not for a
/// delivery that says what was already there.
///
/// Upstream's `PlatformDispatcher.onPlatformConfigurationChanged`, which is
/// the one that fires for every change; the two below fire for one field
/// each. All three are single setters, upstream and here, because upstream's
/// framework installs each once and re-broadcasts to a list of its own.
///
/// # Which handler hears what
///
/// Upstream's `_updateUserSettingsData` compares field by field and calls only
/// the callbacks whose field moved. This one hears every change; the other two
/// hear only their own. **A change to `always_use_24_hour_format` reaches this
/// one and no other**, because upstream declares no callback for it: it counts
/// towards "something changed" and stops there.
pub fn on_settings_changed(handler: impl FnMut(&UserSettings) + 'static) {
    PLATFORM.with(|platform| {
        platform.borrow_mut().settings_handler = Some(Box::new(handler));
    });
}

/// Called when the reader's text size setting changes, and not when anything
/// else does. Upstream's `PlatformDispatcher.onTextScaleFactorChanged`.
///
/// The narrowness is the point: a listener that reloads a font table on this
/// should not be woken by the theme going dark.
pub fn on_text_scale_factor_changed(handler: impl FnMut(&UserSettings) + 'static) {
    PLATFORM.with(|platform| {
        platform.borrow_mut().text_scale_handler = Some(Box::new(handler));
    });
}

/// Called when the platform's light-or-dark choice changes, and not when
/// anything else does. Upstream's
/// `PlatformDispatcher.onPlatformBrightnessChanged`.
pub fn on_platform_brightness_changed(handler: impl FnMut(&UserSettings) + 'static) {
    PLATFORM.with(|platform| {
        platform.borrow_mut().brightness_handler = Some(Box::new(handler));
    });
}

/// Called when the platform's locales change.
pub fn on_locales_changed(handler: impl FnMut(&[Locale]) + 'static) {
    PLATFORM.with(|platform| {
        platform.borrow_mut().locales_handler = Some(Box::new(handler));
    });
}

/// Reads the `flutter/settings` payload.
///
/// Every member is optional and a missing one keeps its previous value rather
/// than reverting to the default. That is upstream's behaviour and it matters:
/// an embedder that sends only `platformBrightness` when the theme changes --
/// which is a reasonable thing to do -- must not silently reset the reader's
/// text scale to 1.
fn parse_settings(json: &str, previous: UserSettings) -> UserSettings {
    let Ok(value) = JsonMessageCodec::new().decode(json.as_bytes()) else {
        return previous;
    };
    let mut settings = previous;
    if let Some(factor) = value.get("textScaleFactor").and_then(Value::as_f64) {
        // A zero or negative scale would lay every glyph out at no width; a
        // platform that reports one is wrong, and obeying it would look like a
        // framework bug rather than a platform one.
        if factor > 0.0 {
            settings.text_scale_factor = factor;
        }
    }
    if let Some(flag) = value.get("alwaysUse24HourFormat").and_then(Value::as_bool) {
        settings.always_use_24_hour_format = flag;
    }
    if let Some(name) = value.get("platformBrightness").and_then(Value::as_str) {
        if let Some(brightness) = Brightness::from_message(name) {
            settings.platform_brightness = brightness;
        }
    }
    settings
}

/// Records new settings and tells the handler, if anything changed.
///
/// Called by the ABI. The equality check is not an optimisation: the shell
/// re-sends the whole settings object whenever any part of it changes, and a
/// handler that reloaded a translation table on every theme change would do it
/// for nothing.
pub(crate) fn set_user_settings(json: &str) -> bool {
    let previous = PLATFORM.with(|platform| platform.borrow().settings);
    let settings = parse_settings(json, previous);
    if settings == previous {
        return false;
    }
    PLATFORM.with(|platform| platform.borrow_mut().settings = settings);

    // Upstream's order, from `_updateUserSettingsData`: the configuration
    // callback first, then the field ones. A listener on both should see the
    // general news before the particular.
    call(Slot::Settings, &settings);
    if settings.text_scale_factor != previous.text_scale_factor {
        call(Slot::TextScale, &settings);
    }
    if settings.platform_brightness != previous.platform_brightness {
        call(Slot::Brightness, &settings);
    }
    // `always_use_24_hour_format` has no slot of its own, which is upstream's
    // shape and not an omission here: it counts towards the change and is
    // heard by `on_settings_changed` alone.
    true
}

/// Which of the three settings handlers.
#[derive(Clone, Copy)]
enum Slot {
    Settings,
    TextScale,
    Brightness,
}

/// Runs one handler with the borrow released.
///
/// The handler may read the settings back -- that is most of what a handler is
/// for -- and it cannot do that while the borrow is alive, so it is moved out
/// and put back. It is **not** put back if it registered a different one,
/// which is how a handler unregisters or replaces itself from inside itself.
fn call(slot: Slot, settings: &UserSettings) {
    let taken = PLATFORM.with(|platform| {
        let mut platform = platform.borrow_mut();
        match slot {
            Slot::Settings => platform.settings_handler.take(),
            Slot::TextScale => platform.text_scale_handler.take(),
            Slot::Brightness => platform.brightness_handler.take(),
        }
    });
    let Some(mut handler) = taken else { return };
    handler(settings);
    PLATFORM.with(|platform| {
        let mut platform = platform.borrow_mut();
        let seat = match slot {
            Slot::Settings => &mut platform.settings_handler,
            Slot::TextScale => &mut platform.text_scale_handler,
            Slot::Brightness => &mut platform.brightness_handler,
        };
        if seat.is_none() {
            *seat = Some(handler);
        }
    });
}

/// Records new locales and tells the handler, if anything changed.
pub(crate) fn set_locales(locales: Vec<Locale>) -> bool {
    let unchanged = PLATFORM.with(|platform| platform.borrow().locales == locales);
    if unchanged {
        return false;
    }
    let handler = PLATFORM.with(|platform| {
        let mut platform = platform.borrow_mut();
        platform.locales = locales;
        platform.locales_handler.take()
    });
    let Some(mut handler) = handler else {
        return true;
    };
    let current = self::locales();
    handler(&current);
    PLATFORM.with(|platform| {
        let mut platform = platform.borrow_mut();
        if platform.locales_handler.is_none() {
            platform.locales_handler = Some(handler);
        }
    });
    true
}

/// Forgets everything, so that a second app on this thread does not inherit the
/// first one's platform state. Called from `rf_app_destroy`.
pub(crate) fn reset() {
    PLATFORM.with(|platform| *platform.borrow_mut() = Platform::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clear() {
        reset();
    }

    #[test]
    fn the_defaults_are_the_neutral_ones() {
        clear();
        let settings = user_settings();
        assert_eq!(settings.text_scale_factor, 1.0);
        assert!(!settings.always_use_24_hour_format);
        assert_eq!(settings.platform_brightness, Brightness::Light);
        // A framework with no locale cannot format a date, so there is one.
        assert_eq!(locale().language_code, "en");
        assert!(locales().is_empty());
    }

    #[test]
    fn a_settings_message_is_read_the_way_the_embedder_writes_it() {
        clear();
        set_user_settings(
            r#"{"textScaleFactor":1.5,"alwaysUse24HourFormat":true,"platformBrightness":"dark"}"#,
        );
        let settings = user_settings();
        assert_eq!(settings.text_scale_factor, 1.5);
        assert!(settings.always_use_24_hour_format);
        assert_eq!(settings.platform_brightness, Brightness::Dark);
        assert_eq!(brightness(), Brightness::Dark);
        assert_eq!(text_scale_factor(), 1.5);
    }

    #[test]
    fn a_partial_message_leaves_the_rest_alone() {
        clear();
        set_user_settings(r#"{"textScaleFactor":2.0,"platformBrightness":"dark"}"#);
        // Only the brightness this time. The scale must survive it: an embedder
        // that reports a theme change and nothing else is not asking for the
        // reader's accessibility setting to be thrown away.
        set_user_settings(r#"{"platformBrightness":"light"}"#);
        assert_eq!(text_scale_factor(), 2.0);
        assert_eq!(brightness(), Brightness::Light);
    }

    #[test]
    fn a_nonsense_message_changes_nothing() {
        clear();
        set_user_settings(r#"{"textScaleFactor":1.25}"#);
        set_user_settings("not json at all");
        set_user_settings(r#"{"textScaleFactor":0.0}"#);
        set_user_settings(r#"{"textScaleFactor":-3.0}"#);
        set_user_settings(r#"{"platformBrightness":"chartreuse"}"#);
        // A scale of zero would lay every glyph out at no width.
        assert_eq!(text_scale_factor(), 1.25);
        assert_eq!(brightness(), Brightness::Light);
    }

    #[test]
    fn the_handler_hears_a_change_and_not_a_repeat() {
        clear();
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = count.clone();
        on_settings_changed(move |_| seen.set(seen.get() + 1));

        set_user_settings(r#"{"platformBrightness":"dark"}"#);
        assert_eq!(count.get(), 1);
        // The shell re-sends the whole object whenever any part of it changes,
        // so the same object arriving twice is ordinary and is not a change.
        set_user_settings(r#"{"platformBrightness":"dark"}"#);
        assert_eq!(count.get(), 1);
        set_user_settings(r#"{"platformBrightness":"light"}"#);
        assert_eq!(count.get(), 2);
    }

    #[test]
    fn a_handler_reads_the_new_value_rather_than_the_old_one() {
        clear();
        let seen = std::rc::Rc::new(std::cell::Cell::new(Brightness::Light));
        let recorded = seen.clone();
        // Reads it back through the public accessor rather than the argument,
        // which is what a widget rebuilding from a callback would do -- and
        // which is only possible if the borrow was released first.
        on_settings_changed(move |_| recorded.set(brightness()));
        set_user_settings(r#"{"platformBrightness":"dark"}"#);
        assert_eq!(seen.get(), Brightness::Dark);
    }

    #[test]
    fn a_handler_can_replace_itself() {
        clear();
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let outer = count.clone();
        on_settings_changed(move |_| {
            let inner = outer.clone();
            outer.set(outer.get() + 1);
            on_settings_changed(move |_| inner.set(inner.get() + 10));
        });
        set_user_settings(r#"{"platformBrightness":"dark"}"#);
        assert_eq!(count.get(), 1);
        set_user_settings(r#"{"platformBrightness":"light"}"#);
        assert_eq!(count.get(), 11);
    }

    /// Registers all three handlers, each appending its own name to one log,
    /// so both *which* fired and *in what order* are readable.
    fn watch_all() -> std::rc::Rc<std::cell::RefCell<Vec<&'static str>>> {
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let (any, scale, bright) = (log.clone(), log.clone(), log.clone());
        on_settings_changed(move |_| any.borrow_mut().push("any"));
        on_text_scale_factor_changed(move |_| scale.borrow_mut().push("scale"));
        on_platform_brightness_changed(move |_| bright.borrow_mut().push("bright"));
        log
    }

    #[test]
    fn each_handler_hears_its_own_field_and_not_the_others() {
        // Upstream's `_updateUserSettingsData` compares field by field and
        // calls only the callbacks whose field moved. One handler for every
        // change means a listener that reloads a font table on the text scale
        // is woken by the theme going dark, and cannot tell which happened.
        clear();
        let log = watch_all();

        set_user_settings(r#"{"textScaleFactor":1.5}"#);
        assert_eq!(*log.borrow(), vec!["any", "scale"]);

        log.borrow_mut().clear();
        set_user_settings(r#"{"platformBrightness":"dark"}"#);
        assert_eq!(*log.borrow(), vec!["any", "bright"]);
    }

    #[test]
    fn the_clock_wakes_the_general_handler_and_no_other() {
        // Upstream declares no callback for `alwaysUse24HourFormat`. It counts
        // towards "something changed" -- `onPlatformConfigurationChanged`
        // fires -- and stops there. Absent here on purpose rather than by
        // omission, which is why it has a test.
        clear();
        let log = watch_all();
        set_user_settings(r#"{"alwaysUse24HourFormat":true}"#);
        assert_eq!(*log.borrow(), vec!["any"]);
    }

    #[test]
    fn one_message_that_moves_two_fields_wakes_both_in_upstreams_order() {
        // The general news before the particular, which is upstream's line
        // order: `onPlatformConfigurationChanged`, then the text scale, then
        // the brightness.
        clear();
        let log = watch_all();
        set_user_settings(
            r#"{"textScaleFactor":1.5,"platformBrightness":"dark","alwaysUse24HourFormat":true}"#,
        );
        assert_eq!(*log.borrow(), vec!["any", "scale", "bright"]);
    }

    #[test]
    fn a_message_that_says_nothing_new_wakes_nobody_and_asks_for_no_frame() {
        // The return value is what the ABI schedules a frame on. The shell
        // re-sends the whole settings object whenever any part of it changes,
        // so a payload that repeats what is already there is ordinary -- and
        // rendering a frame for it is the cost this guard exists to avoid.
        clear();
        let log = watch_all();
        assert!(set_user_settings(r#"{"textScaleFactor":1.5}"#));
        assert_eq!(*log.borrow(), vec!["any", "scale"]);

        log.borrow_mut().clear();
        assert!(
            !set_user_settings(r#"{"textScaleFactor":1.5}"#),
            "the same message twice is not a change"
        );
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn the_same_locale_list_twice_is_not_a_change() {
        // The return value is what the ABI schedules a frame on, and the shell
        // re-sends the whole list whenever any of it moves. Same shape as the
        // settings payload, and same reason.
        clear();
        assert!(set_locales(vec![Locale::new("fr")]));
        assert!(
            !set_locales(vec![Locale::new("fr")]),
            "the same list again says nothing new"
        );
        assert!(set_locales(vec![Locale::new("ja")]));
    }

    #[test]
    fn locales_arrive_in_order_and_the_first_one_is_the_locale() {
        clear();
        set_locales(vec![
            Locale {
                language_code: "zh".to_string(),
                country_code: Some("CN".to_string()),
                script_code: Some("Hans".to_string()),
                variant_code: None,
            },
            Locale::new("en"),
        ]);
        assert_eq!(locales().len(), 2);
        assert_eq!(locale().language_code, "zh");
        // Script before region, which is the order BCP 47 puts them in.
        assert_eq!(locale().to_language_tag(), "zh-Hans-CN");
        assert_eq!(locales()[1].to_language_tag(), "en");
    }

    #[test]
    fn the_locale_handler_hears_a_change_and_not_a_repeat() {
        clear();
        let count = std::rc::Rc::new(std::cell::Cell::new(0));
        let seen = count.clone();
        on_locales_changed(move |_| seen.set(seen.get() + 1));
        set_locales(vec![Locale::new("fr")]);
        assert_eq!(count.get(), 1);
        set_locales(vec![Locale::new("fr")]);
        assert_eq!(count.get(), 1);
        set_locales(vec![Locale::new("fr"), Locale::new("en")]);
        assert_eq!(count.get(), 2);
    }
}
