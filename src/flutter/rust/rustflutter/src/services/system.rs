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
//! protocol *is*, not that anything answers it: [`ACCESSIBILITY`] is spelled
//! out here because that is its name wherever it is implemented, and this port
//! has no semantics tree to implement it with. A call on a channel nobody
//! serves comes back as `Ok(None)` -- upstream's `MissingPluginException` --
//! which is a normal outcome rather than a fault. See
//! [`MethodReply`](super::channel::MethodReply).
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
                Ok(Some(value)) => {
                    value.get("text").and_then(Value::as_str).map(str::to_string)
                }
                _ => None,
            });
        });
    }

    /// Puts text on the clipboard.
    pub fn set_data(text: &str) {
        PLATFORM.invoke("Clipboard.setData", Value::map([("text", Value::from(text))]));
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
        PLATFORM.invoke_with_reply("Clipboard.hasStrings", Value::from(TEXT_PLAIN), move |reply| {
            callback(match reply {
                Ok(Some(value)) => value.get("value").and_then(Value::as_bool).unwrap_or(false),
                _ => false,
            });
        });
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
        responder.success(Value::map([("response", Value::from(response.as_message()))]));
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
            Value::map([("device", Value::I64(device)), ("kind", Value::from(self.kind()))]),
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
            vec!["SystemNavigator.pop", "SystemSound.play", "Clipboard.setData"]
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
            assert!(known.contains(&cursor.kind()), "{} is not in the table", cursor.kind());
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
            assert_eq!(AppLifecycleState::from_message(state.as_message()), Some(state));
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
        on_route_message(move |method, _arguments| {
            recorded.borrow_mut().push(method.to_string())
        });

        let call = JsonMethodCodec
            .encode_method_call(&super::super::MethodCall::new("popRoute", Value::Null))
            .unwrap();
        recorder.deliver("flutter/navigation", &call, 2);

        assert_eq!(seen.borrow().as_slice(), &["popRoute".to_string()]);
        let (response_id, reply) = recorder.responses().remove(0);
        assert_eq!(response_id, 2);
        // A success envelope carrying null -- not a bool. See on_route_message.
        assert_eq!(JsonMethodCodec.decode_envelope(&reply.unwrap()), Ok(Some(Value::Null)));
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
        assert_eq!(StandardMethodCodec.decode_method_call(&bytes).unwrap().method, "level");

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
        value.get("response").and_then(Value::as_str).map(str::to_string)
    }

    fn ask_to_exit(
        recorder: &super::super::tests_support::Recorder,
        kind: &str,
    ) -> Option<String> {
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
        assert_eq!(ask_to_exit(&recorder, "cancelable").as_deref(), Some("exit"));
    }

    #[test]
    fn an_application_can_refuse_a_cancelable_exit() {
        let recorder = install();
        on_exit_requested(|kind| {
            assert_eq!(kind, AppExitType::Cancelable);
            AppExitResponse::Cancel
        });
        assert_eq!(ask_to_exit(&recorder, "cancelable").as_deref(), Some("cancel"));
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
        assert_eq!(call.arguments.get("type").and_then(Value::as_str), Some("required"));
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
