// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The channels the engine and its embedders define.
//!
//! Upstream this is `SystemChannels` plus the handful of typed façades built on
//! it -- `Clipboard`, `SystemSound`, `SystemNavigator`, `SystemChrome`,
//! `MouseCursor`. The channels are the protocol; the façades are the part worth
//! having, because nobody should have to remember that the clipboard's argument
//! is the bare string `"text/plain"` and its answer is a map with one key.
//!
//! # These are names, not implementations
//!
//! A channel constant is a name and a codec. Declaring one says what the
//! protocol *is*, not that anything answers it. A call on a channel nobody
//! serves comes back as `Ok(None)` -- upstream's `MissingPluginException` --
//! which is a normal outcome rather than a fault. See
//! [`MethodReply`](super::channel::MethodReply).
//!
//! [`ACCESSIBILITY`] used to be listed here as a name with nothing behind it,
//! on the grounds that this port had no semantics tree. It has one
//! ([`crate::semantics`]), and [`crate::semantics_event`] is what sends on this
//! channel -- announcements and the platform's tap and long-press feedback.
//!
//! What the Windows host in this repository answers is [`PLATFORM`] -- the
//! clipboard, sound and exit methods of it -- and [`TEXT_INPUT`], which is the
//! IME. What it sends is [`LIFECYCLE`], [`KEY_DATA`] and the editing states
//! that come back on [`TEXT_INPUT`]. Everything else here -- [`MOUSE_CURSOR`]
//! included -- is the name waiting for an implementation on one side or the
//! other.

use super::channel::{BasicMessageChannel, MethodChannel, MethodReply};
use super::codec::{
    JsonMessageCodec, JsonMethodCodec, StandardMessageCodec, StandardMethodCodec, StringCodec,
    Value,
};
use crate::engine::Color;
use crate::platform::Brightness;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

// -- The channels -------------------------------------------------------------

/// Miscellaneous platform services: clipboard, sound, haptics, chrome, exit.
///
/// One channel for a dozen unrelated things, which is historical rather than
/// designed -- but it is the shape every embedder implements, so it is the
/// shape here.
pub const PLATFORM: MethodChannel<JsonMethodCodec> =
    MethodChannel::new("flutter/platform", JsonMethodCodec::new());

/// Routes pushed and popped by the platform: the initial route, the Android
/// back button, a deep link.
pub const NAVIGATION: MethodChannel<JsonMethodCodec> =
    MethodChannel::new("flutter/navigation", JsonMethodCodec::new());

/// The application's lifecycle state, as a bare string.
///
/// The one engine-owned channel that reaches the framework: `Engine` records
/// what it says and deliberately does not consume it.
pub const LIFECYCLE: BasicMessageChannel<StringCodec> =
    BasicMessageChannel::new("flutter/lifecycle", StringCodec::new());

/// Text scale, 24-hour clock, platform brightness, accessibility switches.
///
/// Consumed by `Engine::HandleSettingsPlatformMessage` before the framework
/// sees it; what it carries arrives instead through `SetUserSettingsData`.
/// Named here because it is part of the protocol, not because a handler on it
/// would ever fire.
pub const SETTINGS: BasicMessageChannel<JsonMessageCodec> =
    BasicMessageChannel::new("flutter/settings", JsonMessageCodec::new());

/// Memory pressure, and font changes.
pub const SYSTEM: BasicMessageChannel<JsonMessageCodec> =
    BasicMessageChannel::new("flutter/system", JsonMessageCodec::new());

/// The mouse cursor for a pointer device.
pub const MOUSE_CURSOR: MethodChannel<StandardMethodCodec> =
    MethodChannel::new("flutter/mousecursor", StandardMethodCodec::new());

/// The IME.
///
/// Served on both sides. The framework's end is
/// [`text_input`](super::text_input), which `TextField` drives; the Windows
/// host's end is IMM32. An application should not need this constant.
pub const TEXT_INPUT: MethodChannel<JsonMethodCodec> =
    MethodChannel::new("flutter/textinput", JsonMethodCodec::new());

/// Semantics announcements and gestures.
pub const ACCESSIBILITY: BasicMessageChannel<StandardMessageCodec> =
    BasicMessageChannel::new("flutter/accessibility", StandardMessageCodec::new());

/// Creating, sizing and disposing of platform views.
pub const PLATFORM_VIEWS: MethodChannel<StandardMethodCodec> =
    MethodChannel::new("flutter/platform_views", StandardMethodCodec::new());

/// The raster cache's byte budget.
pub const SKIA: MethodChannel<JsonMethodCodec> =
    MethodChannel::new("flutter/skia", JsonMethodCodec::new());

/// State restoration data, both ways.
pub const RESTORATION: MethodChannel<StandardMethodCodec> =
    MethodChannel::new("flutter/restoration", StandardMethodCodec::new());

/// The legacy JSON key channel, which this port does not use.
///
/// Keys arrive on [`KEY_DATA`] instead. Both exist upstream and an embedder
/// picks one; sending on both would deliver every key twice.
pub const KEY_EVENT: BasicMessageChannel<JsonMessageCodec> =
    BasicMessageChannel::new("flutter/keyevent", JsonMessageCodec::new());

/// Where key events arrive.
///
/// Not a channel in the usual sense and deliberately not declared as one: the
/// payload is a `KeyDataPacket` -- a packed struct, not a codec's output -- and
/// `RuntimeController` unpacks it before the messenger is involved. The name is
/// here so that the one channel this framework can never register a handler for
/// is written down rather than merely absent.
pub const KEY_DATA: &str = "flutter/keydata";

// -- Lifecycle ----------------------------------------------------------------

/// What the application is doing, from the platform's point of view.
///
/// Upstream's `AppLifecycleState`, and the five values are upstream's five. The
/// distinction that matters on a desktop is `Inactive` (visible, not focused)
/// against `Hidden` (not visible): an animation should keep running in the
/// first and stop in the second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppLifecycleState {
    Resumed,
    Inactive,
    Hidden,
    Paused,
    Detached,
}

impl AppLifecycleState {
    /// The wire spelling, which is the enum's Dart `toString`.
    pub fn as_message(self) -> &'static str {
        match self {
            AppLifecycleState::Resumed => "AppLifecycleState.resumed",
            AppLifecycleState::Inactive => "AppLifecycleState.inactive",
            AppLifecycleState::Hidden => "AppLifecycleState.hidden",
            AppLifecycleState::Paused => "AppLifecycleState.paused",
            AppLifecycleState::Detached => "AppLifecycleState.detached",
        }
    }

    pub fn from_message(message: &str) -> Option<AppLifecycleState> {
        match message {
            "AppLifecycleState.resumed" => Some(AppLifecycleState::Resumed),
            "AppLifecycleState.inactive" => Some(AppLifecycleState::Inactive),
            "AppLifecycleState.hidden" => Some(AppLifecycleState::Hidden),
            "AppLifecycleState.paused" => Some(AppLifecycleState::Paused),
            "AppLifecycleState.detached" => Some(AppLifecycleState::Detached),
            _ => None,
        }
    }
}

/// Watches the application's lifecycle state.
///
/// The state the embedder sent before this was called is delivered
/// immediately -- that is what the messenger's buffering is for, and why an
/// application registering this in its first frame does not miss the message
/// that says the window is already up.
pub fn on_lifecycle_changed(mut handler: impl FnMut(AppLifecycleState) + 'static) {
    LIFECYCLE.set_handler(move |message, respond| {
        if let Some(state) = message.as_str().and_then(AppLifecycleState::from_message) {
            handler(state);
        }
        // Nothing to say back; the embedder is not waiting on an answer.
        respond.reply(None);
    });
}

// -- Clipboard ----------------------------------------------------------------

/// The system clipboard.
///
/// Upstream's `Clipboard`. Text only, which is upstream's limit too: the
/// channel's protocol has one MIME type in it.
pub struct Clipboard;

/// The only format the clipboard channel speaks.
const TEXT_PLAIN: &str = "text/plain";

impl Clipboard {
    /// Asks for the clipboard's text.
    ///
    /// `callback` gets `None` when the clipboard is empty, holds something that
    /// is not text, or when no embedder answers at all. Those are three
    /// different facts and one useful answer.
    pub fn get_data(callback: impl FnOnce(Option<String>) + 'static) {
        PLATFORM.invoke_with_reply("Clipboard.getData", Value::from(TEXT_PLAIN), move |reply| {
            callback(match reply {
                Ok(Some(value)) => value
                    .get("text")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                _ => None,
            });
        });
    }

    /// Puts text on the clipboard.
    pub fn set_data(text: &str) {
        PLATFORM.invoke(
            "Clipboard.setData",
            Value::map([("text", Value::from(text))]),
        );
    }

    /// Asks whether the clipboard holds any text.
    ///
    /// Separate from [`Clipboard::get_data`] because reading the clipboard is a
    /// privileged act on some platforms -- iOS shows the user a banner -- and a
    /// paste button only needs to know whether to be enabled.
    pub fn has_strings(callback: impl FnOnce(bool) + 'static) {
        // The format goes with this call too, exactly as with `get_data`.
        // Upstream's `Clipboard.hasStrings` passes `kTextPlain`, and the
        // embedders check it -- sending nothing gets an "Unknown clipboard
        // format" error back, which reads here as "no text on the clipboard".
        PLATFORM.invoke_with_reply(
            "Clipboard.hasStrings",
            Value::from(TEXT_PLAIN),
            move |reply| {
                callback(match reply {
                    Ok(Some(value)) => value.get("value").and_then(Value::as_bool).unwrap_or(false),
                    _ => false,
                });
            },
        );
    }
}

// -- Sound and haptics --------------------------------------------------------

/// A sound the platform owns.
///
/// Upstream's three. Whether any of them makes a noise is the platform's
/// business and varies: Windows has no sound for a key click or a scroll tick,
/// so it accepts both and stays silent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemSoundType {
    Click,
    Alert,
    Tick,
}

impl SystemSoundType {
    fn as_argument(self) -> &'static str {
        match self {
            SystemSoundType::Click => "SystemSoundType.click",
            SystemSoundType::Alert => "SystemSoundType.alert",
            SystemSoundType::Tick => "SystemSoundType.tick",
        }
    }
}

/// Plays one of the platform's own sounds.
pub struct SystemSound;

impl SystemSound {
    pub fn play(sound: SystemSoundType) {
        PLATFORM.invoke("SystemSound.play", Value::from(sound.as_argument()));
    }
}

/// How hard the device should buzz.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HapticFeedbackType {
    Standard,
    Light,
    Medium,
    Heavy,
    Selection,
}

/// Vibration. Silent on a platform with no vibrator, which is most desktops.
pub struct HapticFeedback;

impl HapticFeedback {
    pub fn vibrate(kind: HapticFeedbackType) {
        let argument = match kind {
            // The standard buzz takes no argument at all -- upstream calls
            // `HapticFeedback.vibrate` with none, and the four named ones with
            // one. Not a quirk worth smoothing over: the embedders switch on it.
            HapticFeedbackType::Standard => Value::Null,
            HapticFeedbackType::Light => Value::from("HapticFeedbackType.lightImpact"),
            HapticFeedbackType::Medium => Value::from("HapticFeedbackType.mediumImpact"),
            HapticFeedbackType::Heavy => Value::from("HapticFeedbackType.heavyImpact"),
            HapticFeedbackType::Selection => Value::from("HapticFeedbackType.selectionClick"),
        };
        PLATFORM.invoke("HapticFeedback.vibrate", argument);
    }
}

// -- The application's own window ---------------------------------------------

/// Asks the platform to close the application.
///
/// Upstream's `SystemNavigator`. On a desktop this closes the window; on
/// Android it is what the back button does at the root of the stack.
pub struct SystemNavigator;

impl SystemNavigator {
    pub fn pop() {
        PLATFORM.invoke("SystemNavigator.pop", Value::Null);
    }
}

// -- Closing, and being asked whether it is all right to close ----------------

/// Whether an exit may be refused.
///
/// Upstream's `ui.AppExitType`. The distinction is the whole reason the
/// protocol exists: a reader clicking the close button can be asked to save
/// first, and a machine shutting down cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppExitType {
    /// The application is closing. The answer is not consulted.
    Required,
    /// The application has been *asked* to close and may say no.
    Cancelable,
}

impl AppExitType {
    fn as_message(self) -> &'static str {
        match self {
            AppExitType::Required => "required",
            AppExitType::Cancelable => "cancelable",
        }
    }

    fn from_message(name: &str) -> AppExitType {
        // Anything that is not "cancelable" is required, which is how
        // `StringToAppExitType` in the Windows embedder decides it. Erring
        // this way means a request nobody understands still closes the window.
        match name {
            "cancelable" => AppExitType::Cancelable,
            _ => AppExitType::Required,
        }
    }
}

/// What an application says when asked whether it may close.
///
/// Upstream's `ui.AppExitResponse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppExitResponse {
    Exit,
    Cancel,
}

impl AppExitResponse {
    fn as_message(self) -> &'static str {
        match self {
            AppExitResponse::Exit => "exit",
            AppExitResponse::Cancel => "cancel",
        }
    }

    fn from_message(name: &str) -> Option<AppExitResponse> {
        match name {
            "exit" => Some(AppExitResponse::Exit),
            "cancel" => Some(AppExitResponse::Cancel),
            _ => None,
        }
    }
}

/// Answers `System.requestAppExit` for an application that has not said
/// otherwise.
///
/// Upstream's `ServicesBinding.handleRequestAppExit`, whose default is also
/// `exit`: an application with nothing to save must not need to write code to
/// make its close button work.
fn default_exit_response(_kind: AppExitType) -> AppExitResponse {
    AppExitResponse::Exit
}

thread_local! {
    static EXIT_HANDLER: std::cell::RefCell<Option<Box<dyn FnMut(AppExitType) -> AppExitResponse>>> =
        const { std::cell::RefCell::new(None) };
}

/// Installs the `flutter/platform` handler that answers exit requests.
///
/// Called once when the messenger is attached, not when an application asks for
/// it, and that ordering is load-bearing in two ways. Upstream's
/// `ServicesBinding.initInstances` does the same:
///
/// * The close button has to work in an application that has never heard of
///   this protocol. Without a handler the platform would ask and nothing would
///   answer.
/// * Registering the handler is what tells the embedder there is somebody to
///   ask -- it is the channel update on `flutter/platform` that turns the
///   embedder's `WM_CLOSE` handling on. An embedder that never sees it closes
///   the window the ordinary way, which is the right fallback.
pub(crate) fn install_exit_handler() {
    PLATFORM.set_handler(|call, responder| {
        if call.method != "System.requestAppExit" {
            // The framework serves exactly one method on this channel. Anything
            // else on it is the platform's to answer, not ours.
            responder.not_implemented();
            return;
        }
        let kind = call
            .arguments
            .get("type")
            .and_then(Value::as_str)
            .map(AppExitType::from_message)
            .unwrap_or(AppExitType::Required);
        // The handler is moved out before it runs: it may close over state the
        // application also touches, and it is allowed to replace itself.
        let handler = EXIT_HANDLER.with(|slot| slot.borrow_mut().take());
        let response = match handler {
            Some(mut handler) => {
                let response = handler(kind);
                EXIT_HANDLER.with(|slot| {
                    let mut slot = slot.borrow_mut();
                    if slot.is_none() {
                        *slot = Some(handler);
                    }
                });
                response
            }
            None => default_exit_response(kind),
        };
        // A required exit is not a question. Answering `cancel` to one would
        // be answering a question nobody asked, and upstream ignores it too.
        let response = match kind {
            AppExitType::Required => AppExitResponse::Exit,
            AppExitType::Cancelable => response,
        };
        responder.success(Value::map([(
            "response",
            Value::from(response.as_message()),
        )]));
    });
}

/// Removes the exit handler. Called when the messenger is detached, so that the
/// embedder is told there is no longer anybody to ask.
pub(crate) fn remove_exit_handler() {
    EXIT_HANDLER.with(|slot| *slot.borrow_mut() = None);
    PLATFORM.clear_handler();
}

/// Decides what happens when the platform asks whether the application may
/// close.
///
/// Upstream's `AppLifecycleListener.onExitRequested`. One handler, because
/// there is one answer; an application with several things to check does the
/// checking inside it.
///
/// The handler runs on the UI thread and must answer synchronously, which is
/// the one place this differs from upstream: `onExitRequested` returns a
/// `Future`, so a Flutter application can put up a "save your work?" dialog and
/// answer when the reader has clicked. Doing that here means answering
/// [`AppExitResponse::Cancel`] now and calling [`exit_application`] later, which
/// is the same exchange with the waiting moved into the application.
pub fn on_exit_requested(handler: impl FnMut(AppExitType) -> AppExitResponse + 'static) {
    EXIT_HANDLER.with(|slot| *slot.borrow_mut() = Some(Box::new(handler)));
}

/// Asks the platform to close the application.
///
/// Upstream's `ServicesBinding.exitApplication`. The two kinds differ in what
/// the platform does, not in what this does:
///
/// * [`AppExitType::Required`] closes the window and answers `Exit`.
/// * [`AppExitType::Cancelable`] answers `Cancel` straight away and *then*
///   asks -- including asking this application, through the handler above.
///   That is not a quirk of this port; it is what the reply means. The real
///   answer arrives as the window closing or not closing.
///
/// `exit_code` is what the process exits with.
pub fn exit_application(
    kind: AppExitType,
    exit_code: i32,
    callback: impl FnOnce(Option<AppExitResponse>) + 'static,
) {
    PLATFORM.invoke_with_reply(
        "System.exitApplication",
        Value::map([
            ("type", Value::from(kind.as_message())),
            ("exitCode", Value::I64(exit_code as i64)),
        ]),
        move |reply| {
            callback(match reply {
                Ok(Some(value)) => value
                    .get("response")
                    .and_then(Value::as_str)
                    .and_then(AppExitResponse::from_message),
                // No handler, or an error. Either way the platform did not say
                // it was closing, and reporting nothing is more honest than
                // reporting a cancel the platform never sent.
                _ => None,
            });
        },
    );
}

/// What the platform shows about the application outside its own window.
pub struct SystemChrome;

impl SystemChrome {
    /// Sets the title and colour the platform's task switcher shows.
    ///
    /// `primary_color` is 0xAARRGGBB, the same encoding as everything else here.
    pub fn set_application_switcher_description(label: &str, primary_color: u32) {
        PLATFORM.invoke(
            "SystemChrome.setApplicationSwitcherDescription",
            Value::map([
                ("label", Value::from(label)),
                // Signed because JSON has no unsigned integers and the far end
                // reads it as a Dart int. The bit pattern is what matters.
                ("primaryColor", Value::I64(primary_color as i32 as i64)),
            ]),
        );
    }
}

// -- The mouse cursor ---------------------------------------------------------

/// A cursor the platform provides.
///
/// The kinds are the ones the Windows embedder maps to a real `HCURSOR`
/// (`FlutterWindowsEngine::GetCursorByName`); a kind an embedder does not know
/// falls back to the arrow rather than failing, which is upstream's behaviour
/// and the right one -- a missing cursor should not stop a drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemMouseCursor {
    /// Hides the cursor entirely.
    None,
    Basic,
    Click,
    Forbidden,
    Help,
    Move,
    NoDrop,
    Precise,
    Progress,
    Text,
    Wait,
    AllScroll,
    ResizeLeftRight,
    ResizeUpDown,
    ResizeUpLeftDownRight,
    ResizeUpRightDownLeft,
    ResizeColumn,
    ResizeRow,
}

impl SystemMouseCursor {
    /// The name the embedder looks up. Upstream's `SystemMouseCursor.kind`.
    pub fn kind(self) -> &'static str {
        match self {
            SystemMouseCursor::None => "none",
            SystemMouseCursor::Basic => "basic",
            SystemMouseCursor::Click => "click",
            SystemMouseCursor::Forbidden => "forbidden",
            SystemMouseCursor::Help => "help",
            SystemMouseCursor::Move => "move",
            SystemMouseCursor::NoDrop => "noDrop",
            SystemMouseCursor::Precise => "precise",
            SystemMouseCursor::Progress => "progress",
            SystemMouseCursor::Text => "text",
            SystemMouseCursor::Wait => "wait",
            SystemMouseCursor::AllScroll => "allScroll",
            SystemMouseCursor::ResizeLeftRight => "resizeLeftRight",
            SystemMouseCursor::ResizeUpDown => "resizeUpDown",
            SystemMouseCursor::ResizeUpLeftDownRight => "resizeUpLeftDownRight",
            SystemMouseCursor::ResizeUpRightDownLeft => "resizeUpRightDownLeft",
            SystemMouseCursor::ResizeColumn => "resizeColumn",
            SystemMouseCursor::ResizeRow => "resizeRow",
        }
    }

    /// Sets the cursor for one pointer device.
    ///
    /// The device id is part of the protocol because a machine can have more
    /// than one pointer, and upstream's `MouseTracker` tracks a cursor per
    /// device. A single-mouse application passes 0 and never thinks about it.
    pub fn activate(self, device: i64) {
        MOUSE_CURSOR.invoke(
            "activateSystemCursor",
            Value::map([
                ("device", Value::I64(device)),
                ("kind", Value::from(self.kind())),
            ]),
        );
    }
}

// -- Memory ------------------------------------------------------------------

/// Watches for the platform saying it is short of memory.
///
/// The message is a bare `{"type":"memoryPressure"}` on [`SYSTEM`], and what to
/// do about it is the application's business -- upstream's `ImageCache` clears
/// itself here.
pub fn on_memory_pressure(mut handler: impl FnMut() + 'static) {
    SYSTEM.set_handler(move |message, respond| {
        if message.get("type").and_then(Value::as_str) == Some("memoryPressure") {
            handler();
        }
        respond.reply(None);
    });
}

// -- Routes ------------------------------------------------------------------

/// Watches for routes the platform pushes at the application.
///
/// `pushRoute` is a deep link arriving; `popRoute` is the Android back button
/// or a desktop window being asked to close.
///
/// The answer is an empty success, which is what upstream's
/// `WidgetsBinding._handleNavigationInvocation` produces: `handlePushRoute` and
/// `handlePopRoute` both return `Future<void>`, so nothing on this channel
/// reports back whether the route was taken. An earlier version of this
/// answered with a bool, which would have been a protocol of our own invention.
pub fn on_route_message(mut handler: impl FnMut(&str, &Value) + 'static) {
    NAVIGATION.set_handler(move |call, respond| {
        handler(&call.method, &call.arguments);
        respond.success(Value::Null);
    });
}

// -- A raw call, for anything not covered above -------------------------------

/// Calls a method on a channel named at run time, with the standard codec.
///
/// The escape hatch for a plugin this module knows nothing about, which is most
/// of them. Everything above is a convenience over exactly this.
pub fn invoke_plugin_method(
    channel: &str,
    method: &str,
    arguments: Value,
    callback: impl FnOnce(MethodReply) + 'static,
) {
    MethodChannel::named(channel, StandardMethodCodec::new())
        .invoke_with_reply(method, arguments, callback);
}

#[cfg(test)]
mod tests {
    use super::super::codec::{MethodCall, MethodCodec, MethodError};
    use super::super::tests_support::install;
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn each_haptic_names_the_impact_the_embedders_switch_on() {
        // Five values, five payloads, and nothing in the crate was reading
        // any of them: swapping heavyImpact for lightImpact left the suite
        // green. A lookup table nobody checks is a table that can be wrong in
        // one row forever.
        let expected = [
            (HapticFeedbackType::Standard, Value::Null),
            (
                HapticFeedbackType::Light,
                Value::from("HapticFeedbackType.lightImpact"),
            ),
            (
                HapticFeedbackType::Medium,
                Value::from("HapticFeedbackType.mediumImpact"),
            ),
            (
                HapticFeedbackType::Heavy,
                Value::from("HapticFeedbackType.heavyImpact"),
            ),
            (
                HapticFeedbackType::Selection,
                Value::from("HapticFeedbackType.selectionClick"),
            ),
        ];
        for (kind, argument) in expected {
            let recorder = install();
            HapticFeedback::vibrate(kind);
            let (channel, bytes, _) = recorder.sent().remove(0);
            assert_eq!(channel, "flutter/platform");
            let call = JsonMethodCodec.decode_method_call(&bytes).unwrap();
            assert_eq!(call.method, "HapticFeedback.vibrate", "{kind:?}");
            assert_eq!(call.arguments, argument, "{kind:?}");
        }
    }

    #[test]
    fn and_the_plain_buzz_carries_no_argument_where_the_others_do() {
        // Upstream calls `HapticFeedback.vibrate` with no argument for the
        // standard buzz and with one for the four named impacts. The embedders
        // switch on exactly that, so the absence is part of the protocol.
        let recorder = install();
        HapticFeedback::vibrate(HapticFeedbackType::Standard);
        let (_, bytes, _) = recorder.sent().remove(0);
        assert_eq!(
            JsonMethodCodec
                .decode_method_call(&bytes)
                .unwrap()
                .arguments,
            Value::Null
        );

        let recorder = install();
        HapticFeedback::vibrate(HapticFeedbackType::Light);
        let (_, bytes, _) = recorder.sent().remove(0);
        assert_ne!(
            JsonMethodCodec
                .decode_method_call(&bytes)
                .unwrap()
                .arguments,
            Value::Null
        );
    }

    #[test]
    fn the_clipboard_speaks_the_protocol_the_embedders_implement() {
        // The bare string "text/plain" as the argument and a map with one key
        // as the answer. Neither is ours to choose: every embedder is already
        // written against it.
        let recorder = install();
        let text = Rc::new(RefCell::new(None));
        let recorded = text.clone();
        Clipboard::get_data(move |value| *recorded.borrow_mut() = value);

        let (channel, bytes, response_id) = recorder.sent().remove(0);
        assert_eq!(channel, "flutter/platform");
        let call = JsonMethodCodec.decode_method_call(&bytes).unwrap();
        assert_eq!(call.method, "Clipboard.getData");
        assert_eq!(call.arguments, Value::from("text/plain"));

        let answer = JsonMethodCodec
            .encode_success_envelope(&Value::map([("text", Value::from("copied"))]))
            .unwrap();
        recorder.reply(response_id, Some(&answer));
        assert_eq!(*text.borrow(), Some("copied".to_string()));
    }

    #[test]
    fn a_clipboard_nobody_implements_reads_as_empty_rather_than_hanging() {
        let recorder = install();
        let answered = Rc::new(RefCell::new(false));
        let recorded = answered.clone();
        Clipboard::get_data(move |value| {
            assert_eq!(value, None);
            *recorded.borrow_mut() = true;
        });
        let (_, _, response_id) = recorder.sent().remove(0);
        recorder.reply(response_id, None);
        assert!(*answered.borrow());
    }

    #[test]
    fn every_clipboard_call_carries_the_format_the_embedders_check() {
        // Leaving it off `hasStrings` gets an "Unknown clipboard format" error
        // from a conforming embedder, which a caller reading a bool sees as
        // "no text" -- a wrong answer rather than a visible failure.
        let recorder = install();
        Clipboard::get_data(|_text| {});
        Clipboard::has_strings(|_has| {});

        for (_, bytes, _) in recorder.sent() {
            let call = JsonMethodCodec.decode_method_call(&bytes).unwrap();
            assert_eq!(
                call.arguments,
                Value::from("text/plain"),
                "{} sent {:?}",
                call.method,
                call.arguments
            );
        }
    }

    #[test]
    fn the_fire_and_forget_calls_ask_for_no_reply() {
        // A response handle costs the shell a map entry and a thread hop. These
        // calls have nothing to come back, so they ask for nothing.
        let recorder = install();
        SystemNavigator::pop();
        SystemSound::play(SystemSoundType::Click);
        Clipboard::set_data("x");

        let sent = recorder.sent();
        assert_eq!(sent.len(), 3);
        for (channel, _, response_id) in &sent {
            assert_eq!(channel, "flutter/platform");
            assert_eq!(*response_id, 0, "no reply was asked for");
        }
        let methods: Vec<String> = sent
            .iter()
            .map(|(_, bytes, _)| JsonMethodCodec.decode_method_call(bytes).unwrap().method)
            .collect();
        assert_eq!(
            methods,
            vec![
                "SystemNavigator.pop",
                "SystemSound.play",
                "Clipboard.setData"
            ]
        );
    }

    #[test]
    fn the_cursor_goes_out_on_its_own_channel_with_the_standard_codec() {
        // Not flutter/platform, and not JSON. The mouse cursor is one of the
        // few engine channels that speaks the binary codec.
        let recorder = install();
        SystemMouseCursor::Click.activate(0);

        let (channel, bytes, _) = recorder.sent().remove(0);
        assert_eq!(channel, "flutter/mousecursor");
        let call = StandardMethodCodec.decode_method_call(&bytes).unwrap();
        assert_eq!(call.method, "activateSystemCursor");
        assert_eq!(call.argument("kind"), Some(&Value::from("click")));
        assert_eq!(call.argument("device").and_then(Value::as_i64), Some(0));
    }

    #[test]
    fn every_cursor_kind_is_one_the_windows_embedder_knows() {
        // The names are looked up in a table in FlutterWindowsEngine; a
        // misspelling would silently become an arrow, which is exactly the kind
        // of bug that survives review.
        let known = [
            "none",
            "basic",
            "click",
            "forbidden",
            "help",
            "move",
            "noDrop",
            "precise",
            "progress",
            "text",
            "wait",
            "allScroll",
            "resizeLeftRight",
            "resizeUpDown",
            "resizeUpLeftDownRight",
            "resizeUpRightDownLeft",
            "resizeColumn",
            "resizeRow",
        ];
        let all = [
            SystemMouseCursor::None,
            SystemMouseCursor::Basic,
            SystemMouseCursor::Click,
            SystemMouseCursor::Forbidden,
            SystemMouseCursor::Help,
            SystemMouseCursor::Move,
            SystemMouseCursor::NoDrop,
            SystemMouseCursor::Precise,
            SystemMouseCursor::Progress,
            SystemMouseCursor::Text,
            SystemMouseCursor::Wait,
            SystemMouseCursor::AllScroll,
            SystemMouseCursor::ResizeLeftRight,
            SystemMouseCursor::ResizeUpDown,
            SystemMouseCursor::ResizeUpLeftDownRight,
            SystemMouseCursor::ResizeUpRightDownLeft,
            SystemMouseCursor::ResizeColumn,
            SystemMouseCursor::ResizeRow,
        ];
        for cursor in all {
            assert!(
                known.contains(&cursor.kind()),
                "{} is not in the table",
                cursor.kind()
            );
        }
    }

    #[test]
    fn the_lifecycle_state_arrives_as_the_bare_string_it_is_sent_as() {
        let recorder = install();
        let states = Rc::new(RefCell::new(Vec::new()));
        let recorded = states.clone();
        on_lifecycle_changed(move |state| recorded.borrow_mut().push(state));

        recorder.deliver("flutter/lifecycle", b"AppLifecycleState.inactive", 0);
        recorder.deliver("flutter/lifecycle", b"AppLifecycleState.hidden", 0);
        // Not a state anybody defines. Ignored rather than guessed at.
        recorder.deliver("flutter/lifecycle", b"AppLifecycleState.dreaming", 0);

        assert_eq!(
            states.borrow().as_slice(),
            &[AppLifecycleState::Inactive, AppLifecycleState::Hidden]
        );
    }

    #[test]
    fn a_lifecycle_message_sent_before_anybody_listens_still_arrives() {
        // The embedder sends the first one at startup, which is before any
        // application code has run.
        let recorder = install();
        recorder.deliver("flutter/lifecycle", b"AppLifecycleState.resumed", 0);

        let states = Rc::new(RefCell::new(Vec::new()));
        let recorded = states.clone();
        on_lifecycle_changed(move |state| recorded.borrow_mut().push(state));
        assert_eq!(states.borrow().as_slice(), &[AppLifecycleState::Resumed]);
    }

    #[test]
    fn every_lifecycle_state_round_trips_through_its_wire_name() {
        for state in [
            AppLifecycleState::Resumed,
            AppLifecycleState::Inactive,
            AppLifecycleState::Hidden,
            AppLifecycleState::Paused,
            AppLifecycleState::Detached,
        ] {
            assert_eq!(
                AppLifecycleState::from_message(state.as_message()),
                Some(state)
            );
        }
    }

    #[test]
    fn memory_pressure_reaches_a_listener() {
        let recorder = install();
        let pressed = Rc::new(RefCell::new(0));
        let recorded = pressed.clone();
        on_memory_pressure(move || *recorded.borrow_mut() += 1);

        recorder.deliver("flutter/system", br#"{"type":"memoryPressure"}"#, 0);
        recorder.deliver("flutter/system", br#"{"type":"fontsChange"}"#, 0);
        assert_eq!(*pressed.borrow(), 1);
    }

    #[test]
    fn a_pushed_route_reaches_the_application_and_is_answered_emptily() {
        let recorder = install();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let recorded = seen.clone();
        on_route_message(move |method, _arguments| recorded.borrow_mut().push(method.to_string()));

        let call = JsonMethodCodec
            .encode_method_call(&super::super::MethodCall::new("popRoute", Value::Null))
            .unwrap();
        recorder.deliver("flutter/navigation", &call, 2);

        assert_eq!(seen.borrow().as_slice(), &["popRoute".to_string()]);
        let (response_id, reply) = recorder.responses().remove(0);
        assert_eq!(response_id, 2);
        // A success envelope carrying null -- not a bool. See on_route_message.
        assert_eq!(
            JsonMethodCodec.decode_envelope(&reply.unwrap()),
            Ok(Some(Value::Null))
        );
    }

    #[test]
    fn an_unknown_plugin_can_be_called_without_a_façade() {
        let recorder = install();
        let answer = Rc::new(RefCell::new(None));
        let recorded = answer.clone();
        invoke_plugin_method("com.example/battery", "level", Value::Null, move |reply| {
            *recorded.borrow_mut() = Some(reply)
        });

        let (channel, bytes, response_id) = recorder.sent().remove(0);
        assert_eq!(channel, "com.example/battery");
        assert_eq!(
            StandardMethodCodec
                .decode_method_call(&bytes)
                .unwrap()
                .method,
            "level"
        );

        let error = MethodError::new("UNAVAILABLE", None);
        let envelope = StandardMethodCodec.encode_error_envelope(&error).unwrap();
        recorder.reply(response_id, Some(&envelope));
        assert_eq!(*answer.borrow(), Some(Err(error)));
    }

    // -- Closing --------------------------------------------------------------

    /// Puts a method call on `flutter/platform` the way an embedder does, and
    /// reads back the `response` the framework answered with.
    ///
    /// `None` means the framework answered empty, which on this channel means
    /// the method was not the framework's to answer.
    fn call_platform(
        recorder: &super::super::tests_support::Recorder,
        method: &str,
        arguments: Value,
    ) -> Option<String> {
        let call = JsonMethodCodec
            .encode_method_call(&MethodCall::new(method, arguments))
            .unwrap();
        let response_id = recorder.responses().len() as i64 + 1;
        recorder.deliver("flutter/platform", &call, response_id);
        let (_, reply) = recorder
            .responses()
            .into_iter()
            .find(|(id, _)| *id == response_id)
            .expect("the handler answered");
        let value = JsonMethodCodec.decode_envelope(&reply?).ok()??;
        value
            .get("response")
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    fn ask_to_exit(recorder: &super::super::tests_support::Recorder, kind: &str) -> Option<String> {
        call_platform(
            recorder,
            "System.requestAppExit",
            Value::map([("type", Value::from(kind))]),
        )
    }

    #[test]
    fn an_application_that_says_nothing_still_closes() {
        // The default matters more than it looks: with no handler at all the
        // embedder would ask and nothing would answer, and the close button
        // would do nothing at all.
        let recorder = install();
        assert_eq!(
            ask_to_exit(&recorder, "cancelable").as_deref(),
            Some("exit")
        );
    }

    #[test]
    fn an_application_can_refuse_a_cancelable_exit() {
        let recorder = install();
        on_exit_requested(|kind| {
            assert_eq!(kind, AppExitType::Cancelable);
            AppExitResponse::Cancel
        });
        assert_eq!(
            ask_to_exit(&recorder, "cancelable").as_deref(),
            Some("cancel")
        );
    }

    #[test]
    fn a_required_exit_is_not_a_question() {
        let recorder = install();
        on_exit_requested(|_| AppExitResponse::Cancel);
        // The handler still hears about it -- that is where an application
        // writes down whatever it can before it goes -- but the answer is not
        // consulted, because the machine is already shutting down.
        assert_eq!(ask_to_exit(&recorder, "required").as_deref(), Some("exit"));
    }

    #[test]
    fn a_request_with_no_type_is_treated_as_required() {
        let recorder = install();
        let seen = Rc::new(RefCell::new(None));
        let recorded = seen.clone();
        on_exit_requested(move |kind| {
            *recorded.borrow_mut() = Some(kind);
            AppExitResponse::Cancel
        });
        let response = call_platform(&recorder, "System.requestAppExit", Value::Null);
        assert_eq!(*seen.borrow(), Some(AppExitType::Required));
        assert_eq!(response.as_deref(), Some("exit"));
    }

    #[test]
    fn the_framework_does_not_answer_the_platforms_own_methods() {
        // `flutter/platform` runs in both directions, and the handler installed
        // here must not swallow `Clipboard.getData` on the way to the embedder.
        // An empty reply is what says "not ours".
        let recorder = install();
        assert_eq!(
            call_platform(&recorder, "Clipboard.getData", Value::from("text/plain")),
            None,
            "not implemented is an empty reply"
        );
    }

    #[test]
    fn asking_to_exit_sends_what_the_embedder_reads() {
        let recorder = install();
        let answer = Rc::new(RefCell::new(None));
        let recorded = answer.clone();
        exit_application(AppExitType::Required, 3, move |response| {
            *recorded.borrow_mut() = Some(response)
        });

        let (channel, bytes, response_id) = recorder.sent().remove(0);
        assert_eq!(channel, "flutter/platform");
        let call = JsonMethodCodec.decode_method_call(&bytes).unwrap();
        assert_eq!(call.method, "System.exitApplication");
        assert_eq!(
            call.arguments.get("type").and_then(Value::as_str),
            Some("required")
        );
        // An int, not a double: the embedder reads it with `IsInt` and rejects
        // the whole request if it is anything else.
        assert_eq!(call.arguments.get("exitCode"), Some(&Value::I64(3)));

        let envelope = JsonMethodCodec
            .encode_success_envelope(&Value::map([("response", Value::from("exit"))]))
            .unwrap();
        recorder.reply(response_id, Some(&envelope));
        assert_eq!(*answer.borrow(), Some(Some(AppExitResponse::Exit)));
    }

    #[test]
    fn an_embedder_that_does_not_serve_the_exit_method_reports_nothing() {
        let recorder = install();
        let answer = Rc::new(RefCell::new(None));
        let recorded = answer.clone();
        exit_application(AppExitType::Cancelable, 0, move |response| {
            *recorded.borrow_mut() = Some(response)
        });
        let (_, _, response_id) = recorder.sent().remove(0);
        recorder.reply(response_id, None);
        // Not a cancel. The platform never said it, and inventing one would
        // tell the application its exit was refused when it was never heard.
        assert_eq!(*answer.borrow(), Some(None));
    }
}

/// Upstream `ApplicationSwitcherDescription`: what the platform's task
/// switcher shows for this application.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplicationSwitcherDescription {
    pub label: Option<String>,
    /// Upstream's is a bare `int` and not a `Color`, because it is what the
    /// platform wants: 0xAARRGGBB, the encoding everything here uses.
    pub primary_color: Option<u32>,
}

impl ApplicationSwitcherDescription {
    pub fn new() -> ApplicationSwitcherDescription {
        ApplicationSwitcherDescription::default()
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_primary_color(mut self, primary_color: u32) -> Self {
        self.primary_color = Some(primary_color);
        self
    }

    pub(crate) fn to_value(&self) -> Value {
        Value::map([
            (
                "label",
                match &self.label {
                    Some(label) => Value::from(label.as_str()),
                    None => Value::Null,
                },
            ),
            // Signed because JSON has no unsigned integers and the far end
            // reads it as a Dart int. The bit pattern is what matters.
            (
                "primaryColor",
                match self.primary_color {
                    Some(color) => Value::I64(color as i32 as i64),
                    None => Value::Null,
                },
            ),
        ])
    }
}

/// Upstream `SystemUiOverlayStyle`: how the status bar and the navigation bar
/// should be painted over this application.
///
/// Every field is optional and an unset one means "leave it as it is" -- the
/// bars belong to the platform, and an application only says what it needs
/// changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemUiOverlayStyle {
    pub system_navigation_bar_color: Option<Color>,
    pub system_navigation_bar_divider_color: Option<Color>,
    /// The brightness of the *icons* on the navigation bar, which is the
    /// opposite of the bar's own: light icons go on a dark bar.
    pub system_navigation_bar_icon_brightness: Option<Brightness>,
    pub system_navigation_bar_contrast_enforced: Option<bool>,
    pub status_bar_color: Option<Color>,
    /// iOS only, and the odd one out: this is the brightness of what is
    /// *behind* the status bar, which is why the two constants below set it
    /// to the opposite of the icon brightness beside it.
    pub status_bar_brightness: Option<Brightness>,
    pub status_bar_icon_brightness: Option<Brightness>,
    pub system_status_bar_contrast_enforced: Option<bool>,
}

impl SystemUiOverlayStyle {
    /// Upstream `SystemUiOverlayStyle.light`: light icons, for a dark
    /// application. The name is the *icons*, not the application.
    pub const LIGHT: SystemUiOverlayStyle = SystemUiOverlayStyle {
        system_navigation_bar_color: Some(Color(0xFF00_0000)),
        system_navigation_bar_divider_color: None,
        system_navigation_bar_icon_brightness: Some(Brightness::Light),
        system_navigation_bar_contrast_enforced: None,
        status_bar_color: None,
        status_bar_brightness: Some(Brightness::Dark),
        status_bar_icon_brightness: Some(Brightness::Light),
        system_status_bar_contrast_enforced: None,
    };

    /// Upstream `SystemUiOverlayStyle.dark`: dark icons, for a light
    /// application.
    pub const DARK: SystemUiOverlayStyle = SystemUiOverlayStyle {
        system_navigation_bar_color: Some(Color(0xFF00_0000)),
        system_navigation_bar_divider_color: None,
        system_navigation_bar_icon_brightness: Some(Brightness::Light),
        system_navigation_bar_contrast_enforced: None,
        status_bar_color: None,
        status_bar_brightness: Some(Brightness::Light),
        status_bar_icon_brightness: Some(Brightness::Dark),
        system_status_bar_contrast_enforced: None,
    };

    /// Upstream `copyWith`: the argument's fields where it has them, this
    /// one's where it does not.
    ///
    /// The direction is the opposite of a `merge` and it is worth saying so:
    /// `copyWith` is *this style, amended*, so the amendment wins. Every field
    /// is `other.x.or(self.x)`, and a test that sets a field on one side only
    /// cannot tell the two apart.
    pub fn copy_with(&self, other: &SystemUiOverlayStyle) -> SystemUiOverlayStyle {
        SystemUiOverlayStyle {
            system_navigation_bar_color: other
                .system_navigation_bar_color
                .or(self.system_navigation_bar_color),
            system_navigation_bar_divider_color: other
                .system_navigation_bar_divider_color
                .or(self.system_navigation_bar_divider_color),
            system_navigation_bar_icon_brightness: other
                .system_navigation_bar_icon_brightness
                .or(self.system_navigation_bar_icon_brightness),
            system_navigation_bar_contrast_enforced: other
                .system_navigation_bar_contrast_enforced
                .or(self.system_navigation_bar_contrast_enforced),
            status_bar_color: other.status_bar_color.or(self.status_bar_color),
            status_bar_brightness: other.status_bar_brightness.or(self.status_bar_brightness),
            status_bar_icon_brightness: other
                .status_bar_icon_brightness
                .or(self.status_bar_icon_brightness),
            system_status_bar_contrast_enforced: other
                .system_status_bar_contrast_enforced
                .or(self.system_status_bar_contrast_enforced),
        }
    }

    /// Upstream's `_toMap`. The brightnesses go over as the strings Dart's
    /// `toString` makes of them, which is what the embedders parse.
    pub(crate) fn to_value(&self) -> Value {
        let color = |color: Option<Color>| match color {
            Some(color) => Value::I64(color.0 as i32 as i64),
            None => Value::Null,
        };
        let brightness = |brightness: Option<Brightness>| match brightness {
            Some(Brightness::Light) => Value::from("Brightness.light"),
            Some(Brightness::Dark) => Value::from("Brightness.dark"),
            None => Value::Null,
        };
        let flag = |flag: Option<bool>| match flag {
            Some(flag) => Value::Bool(flag),
            None => Value::Null,
        };
        Value::map([
            (
                "systemNavigationBarColor",
                color(self.system_navigation_bar_color),
            ),
            (
                "systemNavigationBarDividerColor",
                color(self.system_navigation_bar_divider_color),
            ),
            (
                "systemStatusBarContrastEnforced",
                flag(self.system_status_bar_contrast_enforced),
            ),
            ("statusBarColor", color(self.status_bar_color)),
            (
                "statusBarBrightness",
                brightness(self.status_bar_brightness),
            ),
            (
                "statusBarIconBrightness",
                brightness(self.status_bar_icon_brightness),
            ),
            (
                "systemNavigationBarIconBrightness",
                brightness(self.system_navigation_bar_icon_brightness),
            ),
            (
                "systemNavigationBarContrastEnforced",
                flag(self.system_navigation_bar_contrast_enforced),
            ),
        ])
    }
}

// -- Undo and redo from the platform (upstream `services/undo_manager.dart`) --

/// Upstream `UndoDirection`: which way the platform asked to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoDirection {
    Undo,
    Redo,
}

/// Upstream `UndoManagerClient`: what a field has to be able to do for the
/// platform's undo to reach it.
///
/// The platform owns the undo gesture -- a three-finger swipe on iOS, a shake,
/// a menu item -- and the field owns the history. This is the join.
pub trait UndoManagerClient {
    /// Upstream `handlePlatformUndo`: the platform asked.
    fn handle_platform_undo(&self, direction: UndoDirection);
    fn undo(&self);
    fn redo(&self);
    fn can_undo(&self) -> bool;
    fn can_redo(&self) -> bool;
}

/// Upstream `UndoManager`: the one channel the platform's undo arrives on.
pub struct UndoManager;

impl UndoManager {
    pub const HANDLE_UNDO_METHOD: &'static str = "UndoManagerClient.handleUndo";
    pub const SET_UNDO_STATE_METHOD: &'static str = "UndoManager.setUndoState";

    /// Upstream's `UndoManager.client` setter. There is one, because there is
    /// one platform gesture and it goes to whatever has the keyboard.
    pub fn set_client(client: Option<Rc<dyn UndoManagerClient>>) {
        UNDO_CLIENT.with(|slot| *slot.borrow_mut() = client);
        UndoManager::install_handler();
    }

    pub fn client() -> Option<Rc<dyn UndoManagerClient>> {
        UNDO_CLIENT.with(|slot| slot.borrow().clone())
    }

    /// Upstream `setUndoState`: tells the platform whether its undo and redo
    /// affordances should be enabled.
    ///
    /// Upstream reports an error rather than throwing when the send fails,
    /// with the note that this is an event and nobody is waiting on it. Here
    /// the send is fire-and-forget for the same reason.
    pub fn set_undo_state(can_undo: bool, can_redo: bool) {
        undo_channel().invoke(
            UndoManager::SET_UNDO_STATE_METHOD,
            Value::map([
                ("canUndo", Value::Bool(can_undo)),
                ("canRedo", Value::Bool(can_redo)),
            ]),
        );
    }

    /// Upstream's `_toUndoDirection`, which throws for anything else. Here it
    /// is nothing, and the call is dropped: a direction this framework does
    /// not know is a platform saying something new, not a bug to crash on.
    pub fn direction_from(name: &str) -> Option<UndoDirection> {
        match name {
            "undo" => Some(UndoDirection::Undo),
            "redo" => Some(UndoDirection::Redo),
            _ => None,
        }
    }

    fn install_handler() {
        UNDO_HANDLER_INSTALLED.with(|installed| {
            if installed.get() {
                return;
            }
            installed.set(true);
            undo_channel().set_handler(move |call, respond| {
                if call.method == UndoManager::HANDLE_UNDO_METHOD {
                    if let Value::List(arguments) = &call.arguments {
                        if let Some(Value::String(name)) = arguments.first() {
                            if let (Some(direction), Some(client)) =
                                (UndoManager::direction_from(name), UndoManager::client())
                            {
                                client.handle_platform_undo(direction);
                            }
                        }
                    }
                    respond.success(Value::Null);
                    return;
                }
                // Upstream throws `MissingPluginException` for anything else,
                // which on the wire is an empty reply.
                respond.not_implemented();
            });
        });
    }
}

fn undo_channel() -> MethodChannel<JsonMethodCodec> {
    MethodChannel::named(
        crate::services::system_channels::SystemChannels::UNDO_MANAGER,
        JsonMethodCodec,
    )
}

thread_local! {
    static UNDO_CLIENT: RefCell<Option<Rc<dyn UndoManagerClient>>> =
        const { RefCell::new(None) };
    static UNDO_HANDLER_INSTALLED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

// -- Mouse tracking (upstream `services/mouse_tracking.dart`) ------------------

/// Upstream `MouseTrackerAnnotation`: what a region wants from a mouse.
///
/// Upstream's is the thing a `RenderMouseRegion` hands to the tracker. This
/// crate routes hover through [`PointerHandlers`](crate::gestures::PointerHandlers)
/// instead, so this is the data class rather than the plumbing -- what a
/// region asks for, in one place, which is what a caller building one needs.
#[derive(Clone)]
pub struct MouseTrackerAnnotation {
    pub on_enter: Option<Rc<dyn Fn()>>,
    pub on_exit: Option<Rc<dyn Fn()>>,
    /// Upstream's default is `MouseCursor.defer`, which means "whatever the
    /// region behind me says". Absent is that here.
    pub cursor: Option<SystemMouseCursor>,
    /// Upstream's `validForMouseTracker`: false while the region is in a
    /// state where it should be ignored, which is how a hidden region stops
    /// answering without being taken out of the tree.
    pub valid_for_mouse_tracker: bool,
}

impl Default for MouseTrackerAnnotation {
    fn default() -> MouseTrackerAnnotation {
        MouseTrackerAnnotation {
            on_enter: None,
            on_exit: None,
            cursor: None,
            valid_for_mouse_tracker: true,
        }
    }
}

impl MouseTrackerAnnotation {
    pub fn new() -> MouseTrackerAnnotation {
        MouseTrackerAnnotation::default()
    }

    pub fn with_on_enter(mut self, on_enter: impl Fn() + 'static) -> Self {
        self.on_enter = Some(Rc::new(on_enter));
        self
    }

    pub fn with_on_exit(mut self, on_exit: impl Fn() + 'static) -> Self {
        self.on_exit = Some(Rc::new(on_exit));
        self
    }

    pub fn with_cursor(mut self, cursor: SystemMouseCursor) -> Self {
        self.cursor = Some(cursor);
        self
    }

    pub fn with_valid_for_mouse_tracker(mut self, valid: bool) -> Self {
        self.valid_for_mouse_tracker = valid;
        self
    }
}

impl std::fmt::Debug for MouseTrackerAnnotation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MouseTrackerAnnotation")
            .field("enter", &self.on_enter.is_some())
            .field("exit", &self.on_exit.is_some())
            .field("cursor", &self.cursor)
            .field("validForMouseTracker", &self.valid_for_mouse_tracker)
            .finish()
    }
}

#[cfg(test)]
mod chrome_tests {
    use super::*;

    fn key<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
        let Value::Map(pairs) = value else {
            return None;
        };
        pairs
            .iter()
            .find(|(field, _)| matches!(field, Value::String(field) if field == name))
            .map(|(_, value)| value)
    }

    #[test]
    fn the_two_overlay_styles_are_named_for_their_icons_not_their_apps() {
        // `light` means light *icons*, which go on a dark application. A
        // reader who takes the name for the app's own brightness picks the
        // one that makes the status bar unreadable.
        assert_eq!(
            SystemUiOverlayStyle::LIGHT.status_bar_icon_brightness,
            Some(Brightness::Light)
        );
        assert_eq!(
            SystemUiOverlayStyle::DARK.status_bar_icon_brightness,
            Some(Brightness::Dark)
        );
        // And `statusBarBrightness` is the odd one out -- iOS's, describing
        // what is *behind* the bar, so it is the opposite of the icons in
        // both constants.
        assert_eq!(
            SystemUiOverlayStyle::LIGHT.status_bar_brightness,
            Some(Brightness::Dark)
        );
        assert_eq!(
            SystemUiOverlayStyle::DARK.status_bar_brightness,
            Some(Brightness::Light)
        );
    }

    #[test]
    fn an_unset_field_goes_over_as_null_and_means_leave_it_alone() {
        // The bars belong to the platform; an application says only what it
        // needs changed. Sending a default instead of a null would take over
        // a bar the application never asked about.
        let value = SystemUiOverlayStyle::default().to_value();
        assert_eq!(key(&value, "statusBarColor"), Some(&Value::Null));
        assert_eq!(key(&value, "statusBarBrightness"), Some(&Value::Null));
        assert_eq!(
            key(&value, "systemNavigationBarContrastEnforced"),
            Some(&Value::Null)
        );
    }

    #[test]
    fn a_brightness_goes_over_as_the_string_the_embedders_parse() {
        // Dart sends `Brightness.light` because that is what `toString` makes
        // of it, and the embedders match on exactly that. Sending `light` or
        // `0` reaches an embedder that does not recognise it.
        let value = SystemUiOverlayStyle::LIGHT.to_value();
        assert_eq!(
            key(&value, "statusBarIconBrightness"),
            Some(&Value::from("Brightness.light"))
        );
        assert_eq!(
            key(&value, "statusBarBrightness"),
            Some(&Value::from("Brightness.dark"))
        );
    }

    #[test]
    fn a_colour_goes_over_as_a_signed_integer() {
        // JSON has no unsigned integers and the far end reads a Dart int, so
        // an opaque black is negative on the wire. The bit pattern is what
        // matters, and sending it unsigned overflows the far end's parse.
        let style = SystemUiOverlayStyle {
            status_bar_color: Some(Color(0xFF00_0000)),
            ..SystemUiOverlayStyle::default()
        };
        assert_eq!(
            key(&style.to_value(), "statusBarColor"),
            Some(&Value::I64(0xFF00_0000u32 as i32 as i64))
        );
    }

    #[test]
    fn copy_with_takes_what_the_other_said_and_keeps_the_rest() {
        let based = SystemUiOverlayStyle::LIGHT.copy_with(&SystemUiOverlayStyle {
            status_bar_color: Some(Color(0xFF00_00FF)),
            ..SystemUiOverlayStyle::default()
        });
        assert_eq!(based.status_bar_color, Some(Color(0xFF00_00FF)));
        assert_eq!(
            based.status_bar_icon_brightness,
            SystemUiOverlayStyle::LIGHT.status_bar_icon_brightness
        );
    }

    #[test]
    fn a_switcher_description_may_say_only_one_of_its_two_things() {
        let labelled = ApplicationSwitcherDescription::new().with_label("Inbox");
        let value = labelled.to_value();
        assert_eq!(key(&value, "label"), Some(&Value::from("Inbox")));
        assert_eq!(key(&value, "primaryColor"), Some(&Value::Null));
    }

    #[test]
    fn an_undo_direction_the_framework_has_never_heard_of_is_dropped() {
        // Upstream throws a `FlutterError` for an unknown direction. A
        // platform saying something new is not a reason to crash the
        // application, so it is nothing here and the call is ignored.
        assert_eq!(
            UndoManager::direction_from("undo"),
            Some(UndoDirection::Undo)
        );
        assert_eq!(
            UndoManager::direction_from("redo"),
            Some(UndoDirection::Redo)
        );
        assert_eq!(UndoManager::direction_from("Undo"), None);
        assert_eq!(UndoManager::direction_from(""), None);
    }

    #[test]
    fn the_undo_methods_are_the_ones_the_platform_dispatches_on() {
        assert_eq!(
            UndoManager::HANDLE_UNDO_METHOD,
            "UndoManagerClient.handleUndo"
        );
        assert_eq!(
            UndoManager::SET_UNDO_STATE_METHOD,
            "UndoManager.setUndoState"
        );
    }

    #[test]
    fn an_annotation_with_no_cursor_defers_to_whatever_is_behind_it() {
        // Upstream's default is `MouseCursor.defer`, which is not a cursor
        // but a refusal to name one. Absent is that here; a default of
        // `Basic` would make every region an arrow and hide the text cursor
        // of the field underneath.
        let annotation = MouseTrackerAnnotation::new();
        assert_eq!(annotation.cursor, None);
        assert!(annotation.valid_for_mouse_tracker);
        assert_eq!(
            MouseTrackerAnnotation::new()
                .with_cursor(SystemMouseCursor::Click)
                .cursor,
            Some(SystemMouseCursor::Click)
        );
    }
}

// -- Keeping the cursor right (upstream `services/mouse_cursor.dart`) ---------

/// Upstream `MouseCursorSession`: one cursor, active on one pointing device.
///
/// A session rather than a call because a cursor may need holding: upstream's
/// own only sends a message, but the type exists so that a cursor backed by
/// something with state -- an animation, a custom bitmap -- has somewhere to
/// keep it, and a defined moment to let go.
pub struct MouseCursorSession {
    pub cursor: SystemMouseCursor,
    pub device: i64,
    activated: Cell<bool>,
    disposed: Cell<bool>,
}

impl MouseCursorSession {
    pub fn new(cursor: SystemMouseCursor, device: i64) -> MouseCursorSession {
        MouseCursorSession {
            cursor,
            device,
            activated: Cell::new(false),
            disposed: Cell::new(false),
        }
    }

    /// Upstream `activate`: tell the platform this is the cursor now.
    pub fn activate(&self) {
        self.activated.set(true);
        self.cursor.activate(self.device);
    }

    /// Upstream `dispose`. The platform is not told anything: the session
    /// that replaces this one has already told it, and a message here would
    /// undo that.
    pub fn dispose(&self) {
        self.disposed.set(true);
    }

    pub fn is_activated(&self) -> bool {
        self.activated.get()
    }

    pub fn is_disposed(&self) -> bool {
        self.disposed.get()
    }
}

/// Upstream `MouseCursorManager`: which cursor each device is showing.
///
/// The rule it exists for: several regions under the pointer each name a
/// cursor, the innermost one that names a real cursor wins, and the platform
/// hears about it only when the answer changed. Without the last part every
/// mouse move is a message.
pub struct MouseCursorManager {
    /// Upstream's `fallbackMouseCursor`, used when nothing under the pointer
    /// named one. Upstream asserts it is not `defer`, since deferring to
    /// nothing is not an answer.
    pub fallback_mouse_cursor: SystemMouseCursor,
    last_session: RefCell<Vec<(i64, MouseCursorSession)>>,
}

impl MouseCursorManager {
    pub fn new(fallback_mouse_cursor: SystemMouseCursor) -> MouseCursorManager {
        MouseCursorManager {
            fallback_mouse_cursor,
            last_session: RefCell::new(Vec::new()),
        }
    }

    /// Upstream `debugDeviceActiveCursor`: what this device is showing.
    pub fn device_active_cursor(&self, device: i64) -> Option<SystemMouseCursor> {
        self.last_session
            .borrow()
            .iter()
            .find(|(at, _)| *at == device)
            .map(|(_, session)| session.cursor)
    }

    /// Upstream `handleDeviceCursorUpdate`.
    ///
    /// `candidates` is the cursors named by what is under the pointer,
    /// innermost first; `None` in the list is upstream's `MouseCursor.defer`,
    /// which is a region saying "whatever is behind me". `removed` is
    /// upstream's `PointerRemovedEvent` -- the device is gone, so the session
    /// goes with it rather than being replaced.
    pub fn handle_device_cursor_update(
        &self,
        device: i64,
        removed: bool,
        candidates: &[Option<SystemMouseCursor>],
    ) {
        if removed {
            self.last_session
                .borrow_mut()
                .retain(|(at, _)| *at != device);
            return;
        }
        // The first candidate that is not deferring; nothing at all falls to
        // the fallback.
        let next = candidates
            .iter()
            .find_map(|candidate| *candidate)
            .unwrap_or(self.fallback_mouse_cursor);
        if self.device_active_cursor(device) == Some(next) {
            // Upstream's early return, and the reason the manager exists: the
            // platform hears only about changes.
            return;
        }
        let session = MouseCursorSession::new(next, device);
        let mut sessions = self.last_session.borrow_mut();
        // Upstream replaces the entry first, then disposes the old session
        // and activates the new one -- in that order, so that a dispose that
        // asked what the current session is sees the new one.
        let previous = match sessions.iter().position(|(at, _)| *at == device) {
            Some(index) => Some(std::mem::replace(&mut sessions[index], (device, session))),
            None => {
                sessions.push((device, session));
                None
            }
        };
        drop(sessions);
        if let Some((_, previous)) = previous {
            previous.dispose();
        }
        let sessions = self.last_session.borrow();
        if let Some((_, session)) = sessions.iter().find(|(at, _)| *at == device) {
            session.activate();
        }
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::*;

    #[test]
    fn the_innermost_region_that_names_a_cursor_wins() {
        // `None` in the list is upstream's `MouseCursor.defer`: a region
        // saying "whatever is behind me". A port that took the first entry
        // outright would give every region under a deferring one an arrow.
        let manager = MouseCursorManager::new(SystemMouseCursor::Basic);
        manager.handle_device_cursor_update(
            1,
            false,
            &[
                None,
                Some(SystemMouseCursor::Text),
                Some(SystemMouseCursor::Click),
            ],
        );
        assert_eq!(
            manager.device_active_cursor(1),
            Some(SystemMouseCursor::Text)
        );
    }

    #[test]
    fn nothing_under_the_pointer_falls_back() {
        // Deferring all the way down is still not an answer, so the fallback
        // is what the platform is told.
        let manager = MouseCursorManager::new(SystemMouseCursor::Basic);
        manager.handle_device_cursor_update(1, false, &[None, None]);
        assert_eq!(
            manager.device_active_cursor(1),
            Some(SystemMouseCursor::Basic)
        );
        // And an empty list is the same case.
        manager.handle_device_cursor_update(2, false, &[]);
        assert_eq!(
            manager.device_active_cursor(2),
            Some(SystemMouseCursor::Basic)
        );
    }

    #[test]
    fn the_platform_hears_only_about_changes() {
        // The reason the manager exists. Without the early return, every
        // mouse move over a region is a message to the platform.
        let manager = MouseCursorManager::new(SystemMouseCursor::Basic);
        manager.handle_device_cursor_update(1, false, &[Some(SystemMouseCursor::Text)]);
        let first = manager.device_active_cursor(1);
        manager.handle_device_cursor_update(1, false, &[Some(SystemMouseCursor::Text)]);
        assert_eq!(manager.device_active_cursor(1), first);
    }

    #[test]
    fn each_device_keeps_its_own_cursor() {
        // A tablet and a mouse on the same screen are two devices, and the
        // pen hovering a link should not change what the mouse is showing.
        let manager = MouseCursorManager::new(SystemMouseCursor::Basic);
        manager.handle_device_cursor_update(1, false, &[Some(SystemMouseCursor::Text)]);
        manager.handle_device_cursor_update(2, false, &[Some(SystemMouseCursor::Click)]);
        assert_eq!(
            manager.device_active_cursor(1),
            Some(SystemMouseCursor::Text)
        );
        assert_eq!(
            manager.device_active_cursor(2),
            Some(SystemMouseCursor::Click)
        );
    }

    #[test]
    fn a_device_that_went_away_takes_its_session_with_it() {
        // Upstream's `PointerRemovedEvent` branch: the session is dropped
        // rather than replaced, so a device that comes back starts fresh
        // instead of inheriting a cursor from before it was unplugged.
        let manager = MouseCursorManager::new(SystemMouseCursor::Basic);
        manager.handle_device_cursor_update(1, false, &[Some(SystemMouseCursor::Text)]);
        manager.handle_device_cursor_update(1, true, &[]);
        assert_eq!(manager.device_active_cursor(1), None);
    }

    #[test]
    fn a_session_is_activated_when_it_takes_over_and_the_old_one_disposed() {
        let session = MouseCursorSession::new(SystemMouseCursor::Text, 1);
        assert!(!session.is_activated());
        assert!(!session.is_disposed());
        session.activate();
        assert!(session.is_activated());
        session.dispose();
        assert!(session.is_disposed());
    }
}

#[cfg(test)]
mod copy_with_direction_tests {
    use super::*;

    /// Every field set, and set *differently* on the two sides.
    ///
    /// The first draft gave both styles `Some(true)` for the two contrast
    /// flags, so swapping their sides produced the same value and the swap was
    /// invisible -- both sides set is not enough, they have to disagree.
    fn all(nav: u32, bright: Brightness, contrast: bool) -> SystemUiOverlayStyle {
        SystemUiOverlayStyle {
            system_navigation_bar_color: Some(Color(nav)),
            system_navigation_bar_divider_color: Some(Color(nav + 1)),
            system_navigation_bar_icon_brightness: Some(bright),
            system_navigation_bar_contrast_enforced: Some(contrast),
            status_bar_color: Some(Color(nav + 2)),
            status_bar_brightness: Some(bright),
            status_bar_icon_brightness: Some(bright),
            system_status_bar_contrast_enforced: Some(!contrast),
        }
    }

    #[test]
    fn the_amendment_wins_every_field_and_not_just_the_first() {
        // `copyWith` is *this style, amended*, so the argument wins -- the
        // opposite direction from a `merge`, which is why it is worth a test
        // of its own. `tools/order_sweep.py` found two of these eight; the
        // other six it could not see, because their receivers span lines and
        // its pattern only matches a receiver and its field on one line.
        let base = all(0x1000_0000, Brightness::Dark, false);
        let amended = all(0x2000_0000, Brightness::Light, true);
        let result = base.copy_with(&amended);

        assert_eq!(result.system_navigation_bar_color, Some(Color(0x2000_0000)));
        assert_eq!(
            result.system_navigation_bar_divider_color,
            Some(Color(0x2000_0001))
        );
        assert_eq!(
            result.system_navigation_bar_icon_brightness,
            Some(Brightness::Light)
        );
        assert_eq!(result.status_bar_color, Some(Color(0x2000_0002)));
        assert_eq!(result.status_bar_brightness, Some(Brightness::Light));
        assert_eq!(result.status_bar_icon_brightness, Some(Brightness::Light));
        assert_eq!(
            result.system_navigation_bar_contrast_enforced,
            Some(true),
            "and the flags too, which need the two sides to disagree"
        );
        assert_eq!(result.system_status_bar_contrast_enforced, Some(false));
    }

    #[test]
    fn a_field_the_amendment_leaves_alone_keeps_what_it_had() {
        let base = all(0x1000_0000, Brightness::Dark, false);
        let sparse = SystemUiOverlayStyle {
            status_bar_color: Some(Color(0x3000_0000)),
            ..SystemUiOverlayStyle::default()
        };
        let result = base.copy_with(&sparse);
        assert_eq!(result.status_bar_color, Some(Color(0x3000_0000)), "amended");
        assert_eq!(
            result.system_navigation_bar_color,
            Some(Color(0x1000_0000)),
            "and the rest is as it was"
        );
    }
}
