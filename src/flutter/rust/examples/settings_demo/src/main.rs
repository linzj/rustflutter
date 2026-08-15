// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! `flutter/settings` and `flutter/localization`, by hand.
//!
//! Everything on screen comes from the platform, and none of it can be changed
//! from inside this window -- which is the point. Change it in the system
//! settings and watch this window change with it, without restarting.
//!
//! The four settings are the same four everywhere; only where the reader
//! changes them differs, which is why the hints below are per platform.
//!
//! On Windows:
//!
//! 1. **Settings → Personalisation → Colours → Choose your mode.**
//! 2. **Settings → Accessibility → Text size.**
//! 3. **Settings → Time & language → Language & region.**
//! 4. **Settings → Time & language → Date & time → time format.**
//!
//! On Android:
//!
//! 1. **Settings → Display → Dark theme.**
//! 2. **Settings → Display → Font size**, or Accessibility → Font size.
//! 3. **Settings → System → Languages.**
//! 4. **Settings → System → Date & time → Use 24-hour format.**
//!
//! Neither channel is ever seen as a channel: `Engine` takes both on the way
//! past and hands the contents to the framework, exactly as upstream does. So
//! this reads `platform::` rather than `services::`.

use std::cell::Cell;
use std::os::raw::{c_char, c_int};

use rustflutter::platform::{self, Brightness};
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex, RenderPadding};

const WIDTH: i32 = 620;
const HEIGHT: i32 = 660;

/// What to call the thing the settings came from.
#[cfg(target_os = "android")]
const SYSTEM: &str = "Android";
#[cfg(not(target_os = "android"))]
const SYSTEM: &str = "Windows";

/// Where the reader changes each of the four, which is the only part of this
/// screen that is not the same on every platform.
struct Hints {
    brightness: &'static str,
    text_size: &'static str,
    hour_format: &'static str,
    languages: &'static str,
}

#[cfg(target_os = "android")]
const HINTS: Hints = Hints {
    brightness: "Settings > Display > Dark theme",
    text_size: "Settings > Display > Font size",
    hour_format: "Settings > System > Date & time > Use 24-hour format",
    languages: "Settings > System > Languages",
};

#[cfg(not(target_os = "android"))]
const HINTS: Hints = Hints {
    brightness: "Settings > Personalisation > Colours > Choose your mode",
    text_size: "Settings > Accessibility > Text size",
    hour_format: "Settings > Time & language > Date & time",
    languages: "Settings > Time & language > Language & region",
};

thread_local! {
    /// Whether the change callbacks have been installed.
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
    /// How many times the platform has reported a change. On screen because a
    /// change that arrives and looks the same is still worth seeing: it says
    /// the message got through, which is the half a person cannot otherwise
    /// tell from the half that is drawing.
    static CHANGES: Cell<u32> = const { Cell::new(0) };
    static LOCALE_CHANGES: Cell<u32> = const { Cell::new(0) };
}

/// The theme the platform's brightness asks for.
fn platform_theme() -> Theme {
    match platform::brightness() {
        Brightness::Dark => Theme::dark(),
        Brightness::Light => Theme::light(),
    }
}

#[derive(Default)]
struct State {
    /// Bumped from the platform callbacks. The values are read live from
    /// `platform::`; this only tells the element tree to look again.
    version: u32,
}

struct Page;

impl StatefulComponent for Page {
    type State = State;

    fn build(
        &self,
        _state: &State,
        handle: StateHandle<State>,
        _context: &mut rustflutter::framework::BuildContext,
    ) -> AnyWidget {
        install_watchers(handle);

        let settings = platform::user_settings();
        let locales = platform::locales();

        let mut children = vec![
            component(Label::title("flutter/settings")),
            component(Label::muted(format!(
                "all of this comes from {SYSTEM}. change it there and watch."
            ))),
            gap(1.0),
            component(Label::new(format!(
                "platformBrightness    {:?}",
                settings.platform_brightness
            ))),
            component(Label::muted(format!("   {}", HINTS.brightness))),
            component(Label::new(format!(
                "textScaleFactor       {:.2}",
                settings.text_scale_factor
            ))),
            component(Label::muted(format!("   {}", HINTS.text_size))),
            component(Label::new(format!(
                "alwaysUse24HourFormat {}",
                settings.always_use_24_hour_format
            ))),
            component(Label::muted(format!("   {}", HINTS.hour_format))),
            gap(1.0),
            component(Label::title("flutter/localization")),
            component(Label::muted(format!("   {}", HINTS.languages))),
        ];

        if locales.is_empty() {
            children.push(component(Label::new("no locales arrived")));
        } else {
            for (rank, locale) in locales.iter().enumerate() {
                children.push(component(Label::new(format!(
                    "{}. {}",
                    rank + 1,
                    locale.to_language_tag()
                ))));
            }
        }

        children.push(gap(1.0));
        children.push(component(Label::new(format!(
            "changes reported: {} settings, {} locales",
            CHANGES.with(Cell::get),
            LOCALE_CHANGES.with(Cell::get),
        ))));
        children.push(component(Label::muted(
            "a change that arrives and looks the same still counts here, which \
             is how you tell the message got through.",
        )));
        children.push(component(Label::muted(
            "nothing in this window multiplies a font size. the scale is \
             applied where text is shaped.",
        )));

        provide(platform_theme(), page(children))
    }
}

/// Asks to be told when the platform changes its mind.
///
/// The shell already schedules a frame when either arrives -- that is what
/// `Engine::HandleSettingsPlatformMessage` calls `ScheduleFrame` for -- but a
/// frame is not a rebuild: only dirty elements are built again. This is what
/// makes this element one of them.
fn install_watchers(handle: StateHandle<State>) {
    if INSTALLED.with(Cell::get) {
        return;
    }
    INSTALLED.with(|installed| installed.set(true));

    let for_settings = handle.clone();
    platform::on_settings_changed(move |_| {
        CHANGES.with(|count| count.set(count.get() + 1));
        for_settings.set_state(|state| state.version += 1);
    });
    platform::on_locales_changed(move |_| {
        LOCALE_CHANGES.with(|count| count.set(count.get() + 1));
        handle.set_state(|state| state.version += 1);
    });
}

/// A padded, left-aligned column.
fn page(children: Vec<AnyWidget>) -> AnyWidget {
    many(children, |rendered| {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(6.0);
        for child in rendered {
            column = column.push(child);
        }
        Box::new(RenderPadding::new(EdgeInsets::all(24.0), column))
    })
}

struct SettingsApp;

impl WidgetApplication for SettingsApp {
    /// Read every frame, so this follows the platform without a rebuild.
    fn background(&self) -> Color {
        platform_theme().background
    }

    fn build(&mut self, _context: &BuildContext) -> AnyWidget {
        component(SafeArea::new(stateful(Page)))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(_argc: c_int, _argv: *const *const c_char) -> c_int {
    register_application(|| Box::new(WidgetHost::new(SettingsApp)));
    let options = RunOptions {
        width: WIDTH,
        height: HEIGHT,
        title: String::from("Platform settings - rustflutter"),
        ..RunOptions::default()
    };
    match run(&options) {
        Ok(()) => 0,
        Err(code) => code,
    }
}
