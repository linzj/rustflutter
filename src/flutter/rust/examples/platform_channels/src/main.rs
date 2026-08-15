// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Platform channels and text input, end to end, against a real shell.
//!
//! Two jobs at once. It is the worked example for `rustflutter::services` and
//! for [`TextField`] -- every shape a channel comes in appears here once -- and
//! it is the only check that crosses the C ABI, because the unit tests stand a
//! recorder in for the shell and never reach the engine at all.
//!
//! The text input half is deliberately written the way an application writes
//! it: a `TextField`, a callback, and nothing else. There is no `TextInput`,
//! no client, no editing state and no IME anywhere in the application's code,
//! because adapting to an IME is the framework's job and is done once.
//!
//! It checks itself and closes itself: each expectation is recorded as it
//! arrives, the results are compared, and the process exits non-zero if
//! anything is missing or wrong. Run it and read the exit code.

use std::cell::RefCell;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicI32, Ordering};

use rustflutter::platform;
use rustflutter::prelude::*;
use rustflutter::services::system;
use rustflutter::services::{MethodChannel, StandardMethodCodec};

// The half of this example that drives the host's own window. What it can do
// is a property of the platform, not of the framework, so there is one module
// per platform and main.rs asks `probe::DRIVES_INPUT` rather than asking which
// platform it is on.
#[cfg_attr(windows, path = "win32.rs")]
#[cfg_attr(target_os = "android", path = "android.rs")]
mod probe;

/// What the process exits with. Set from the UI thread before the window
/// closes; read on the way out of `main`.
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

/// The text this puts on the clipboard and expects to read back.
const MARKER: &str = "rustflutter platform_channels";

/// What the probe types, one `WM_CHAR` at a time.
///
/// Not ASCII on purpose. The character crosses as UTF-16 from Windows, is held
/// as UTF-8 by the platform's model, and comes back with offsets counted in
/// UTF-16 code units -- three encodings, and a wrong conversion in any of them
/// shows up here.
const TYPED: &str = "ab\u{4e2d}";

/// What the probe asks an IME to compose. Pinyin, as a reader would type it
/// before choosing a character.
const COMPOSED: &str = "zhong";

/// A frame budget rather than a schedule: every answer here crosses to the
/// platform thread and back, which is a handful of frames at most.
const FRAME_BUDGET: u32 = 150;

/// Which frame the questions are asked on.
///
/// The second one on a desktop, where a window that exists is a window that can
/// answer. Later on Android, because the clipboard there is only readable and
/// writable by the focused application -- a rule Android 10 introduced -- and
/// an Activity that has drawn its first frame has not necessarily been given
/// focus yet. Asking too early does not fail loudly; it reads back nothing.
const ASK_AT: u32 = if probe::DRIVES_INPUT { 2 } else { 20 };

#[derive(Default)]
struct Results {
    lifecycle: Vec<system::AppLifecycleState>,
    has_strings: Option<bool>,
    clipboard: Option<Option<String>>,
    absent: Option<bool>,
    bad_format: Option<String>,
    /// Every text the field has held, in order, as the application sees it.
    typed: Vec<String>,
    /// Whether the input context accepted a programmatic composition. `None`
    /// until it has been tried; `Some(false)` where there is no IME to drive.
    composition_started: Option<bool>,
    /// The text while composing, and after the composition was committed.
    composing_text: Option<String>,
    committed_text: Option<String>,
    /// What the mouse cursor channel answered: a success, an unserved method,
    /// and a call with the `kind` left out.
    cursor_ok: Option<bool>,
    cursor_unknown_method: Option<bool>,
    cursor_bad_arguments: Option<String>,
    /// Whether the window claimed WM_SETCURSOR, and what the cursor became.
    cursor_claimed: Option<bool>,
    cursor_applied: Option<Option<bool>>,
    /// The first exit request the framework was asked, and whether refusing it
    /// left the window open.
    exit_requested: Option<system::AppExitType>,
    survived_refusal: Option<bool>,
}

impl Results {
    fn typed_arrived(&self) -> bool {
        self.typed.iter().any(|text| text == TYPED)
    }

    fn composition_settled(&self) -> bool {
        match self.composition_started {
            // Nothing to wait for where there is no input context.
            Some(false) => true,
            Some(true) => self.committed_text.is_some(),
            None => false,
        }
    }

    fn complete(&self) -> bool {
        // The channel half, which every platform runs.
        let channels = !self.lifecycle.is_empty()
            && self.has_strings.is_some()
            && self.clipboard.is_some()
            && self.absent.is_some()
            && self.bad_format.is_some()
            && self.survived_refusal.is_some();
        if !probe::DRIVES_INPUT {
            return channels;
        }
        // The gesture half, which only a platform that lets the probe drive
        // its own window can reach. See android.rs for why that is not a
        // shortcoming to be fixed.
        channels
            && self.typed_arrived()
            && self.composition_settled()
            && self.cursor_ok.is_some()
            && self.cursor_unknown_method.is_some()
            && self.cursor_bad_arguments.is_some()
    }
}

thread_local! {
    static RESULTS: RefCell<Results> = RefCell::new(Results::default());
}

fn record(update: impl FnOnce(&mut Results)) {
    RESULTS.with(|results| update(&mut results.borrow_mut()));
}

/// What the field's text changed to, as the application hears it.
///
/// A plain `&str`. The composing run, the selection, the client id and the
/// channel are all below this and none of them appear here.
fn on_text_changed(text: &str) {
    record(|results| {
        if results.typed.last().map(String::as_str) != Some(text) {
            results.typed.push(text.to_string());
        }
        // A composition in progress shows up as text that is present but not
        // yet chosen. The framework knows which run is composing; an
        // application usually does not need to, and this one only needs to
        // know that it happened.
        if text.ends_with(COMPOSED) {
            results.composing_text = Some(text.to_string());
        } else if results.composing_text.is_some() && !text.is_empty() {
            results.committed_text = Some(text.to_string());
        }
    });
}

// -- The application ----------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Asking,
    Focusing,
    Typing,
    Composing,
    Committing,
    Pointing,
    Refusing,
    Checking,
}

struct Probe {
    phase: Phase,
    frames: u32,
    window: probe::Hwnd,
}

impl WidgetApplication for Probe {
    fn background(&self) -> Color {
        Color::rgb(0x0E, 0x16, 0x26)
    }

    /// The phases, one per few frames. Each step needs the one before it to
    /// have made the round trip to the platform thread and back, which is why
    /// this is a clock rather than a straight line.
    fn begin_frame(&mut self, context: &FrameContext) {
        self.frames += 1;
        match (self.phase, self.frames) {
            (Phase::Asking, frame) if frame == ASK_AT => {
                self.ask();
                self.window = probe::find_window();
                if probe::DRIVES_INPUT && self.window.is_null() {
                    println!("  FAIL the host window could not be found");
                }
                // Where nothing can be clicked or typed, the four phases that
                // need a synthetic gesture are stepped over. The exit
                // handshake is not one of them: asking to leave is a channel
                // call, and it runs on every platform.
                self.phase = if probe::DRIVES_INPUT {
                    Phase::Focusing
                } else {
                    Phase::Refusing
                };
            }
            // A real click, which is how a field is focused. Nothing else in
            // this file knows that focusing opens a platform connection.
            (Phase::Focusing, 6) => {
                probe::click_centre(self.window);
                self.phase = Phase::Typing;
            }
            (Phase::Typing, 12) => {
                for character in TYPED.chars() {
                    probe::type_char(self.window, character);
                }
                self.phase = Phase::Composing;
            }
            (Phase::Composing, 24) => {
                let started = probe::compose(self.window, COMPOSED);
                record(|results| results.composition_started = Some(started));
                self.phase = Phase::Committing;
            }
            (Phase::Committing, 36) => {
                probe::commit_composition(self.window);
                self.phase = Phase::Pointing;
            }
            (Phase::Pointing, 42) => {
                self.point();
                self.phase = Phase::Refusing;
            }
            // The cursor is chosen on the platform thread and applied on the
            // window thread, so it is read a few frames after it was asked for
            // rather than in the same one.
            (Phase::Refusing, 48) if probe::DRIVES_INPUT => {
                self.read_cursor();
                // A close the application refuses. Nothing here closes the
                // window: `on_exit_requested` says no, and the check is that
                // the window is still standing afterwards.
                probe::close(self.window);
                self.phase = Phase::Checking;
            }
            (Phase::Refusing, frame) if !probe::DRIVES_INPUT && frame == ASK_AT + 8 => {
                // The same refusal, asked for from the inside. A cancelable
                // `System.exitApplication` is answered "cancel" and *then* put
                // to the framework as a `System.requestAppExit` -- so this
                // reaches `on_exit_requested` exactly as a close button does,
                // without needing one.
                system::exit_application(system::AppExitType::Cancelable, 0, |_| {});
                self.phase = Phase::Checking;
            }
            (Phase::Checking, 56) if probe::DRIVES_INPUT => {
                record(|results| {
                    results.survived_refusal = Some(probe::is_open(self.window));
                });
            }
            (Phase::Checking, frame) if !probe::DRIVES_INPUT && frame == ASK_AT + 20 => {
                record(|results| {
                    // Still running *is* the answer here: a refusal that had
                    // not been honoured would have taken this process with it
                    // eight frames ago.
                    results.survived_refusal = Some(true);
                });
            }
            _ => {}
        }

        let done = RESULTS.with(|results| results.borrow().complete());
        // The last step's frame number, which differs by platform because the
        // steps do.
        let last = if probe::DRIVES_INPUT { 56 } else { ASK_AT + 20 };
        if (done && self.frames > last) || self.frames >= FRAME_BUDGET {
            finish();
        }
        context.scheduler.request_frame();
    }

    fn build(&mut self, _context: &BuildContext) -> AnyWidget {
        // The whole of the application's text-input code: one widget and one
        // callback. No connection, no client, no editing state, no IME.
        //
        // It is the root, so it is given the window's constraints and fills
        // them -- which is what lets the probe click the middle and know it
        // landed on the field.
        stateful(
            TextField::new(1)
                .with_placeholder("type here")
                .with_on_changed(on_text_changed),
        )
    }
}

impl Probe {
    /// Everything on the other channels, asked at once.
    fn ask(&self) {
        // Inbound, and the interesting part is that the embedder sent this
        // before any of this code ran. The buffered message arrives the moment
        // a handler exists.
        system::on_lifecycle_changed(|state| {
            record(|results| results.lifecycle.push(state));
        });

        // Outbound with a reply, through the host's Win32 clipboard.
        Clipboard::set_data(MARKER);
        Clipboard::has_strings(|has| record(|results| results.has_strings = Some(has)));
        Clipboard::get_data(|text| record(|results| results.clipboard = Some(text)));

        // A channel nobody serves. The answer is "not implemented" -- an empty
        // reply -- and the point is that it arrives rather than leaving the
        // caller waiting.
        MethodChannel::new("com.example/absent", StandardMethodCodec::new())
            .invoke_with_reply("anything", Value::Null, |reply| {
                record(|results| results.absent = Some(reply == Ok(None)));
            });

        // An error envelope, built by the host and unpacked here.
        system::PLATFORM.invoke_with_reply(
            "Clipboard.getData",
            Value::from("image/png"),
            |reply| {
                let code = match reply {
                    Err(error) => error.code,
                    other => format!("expected an error, got {other:?}"),
                };
                record(|results| results.bad_format = Some(code));
            },
        );

        // Silent on Windows, which is upstream's behaviour rather than a
        // failure: there is no system sound for a key click.
        SystemSound::play(SystemSoundType::Click);

        // Refuses the first close. An application with unsaved work does this
        // and then puts up a dialog; this one just refuses once, so that the
        // window still being open afterwards proves the refusal was heard.
        system::on_exit_requested(|kind| {
            record(|results| results.exit_requested = Some(kind));
            AppExitResponse::Cancel
        });
    }

    /// The mouse cursor, which is the one channel here that is binary.
    ///
    /// Three calls: the one an application makes, one the host does not serve,
    /// and one with the argument left out. The last two are the interesting
    /// ones -- they are what say the host decoded the standard codec rather
    /// than happening to answer.
    fn point(&self) {
        SystemMouseCursor::Text.activate(0);

        system::MOUSE_CURSOR.invoke_with_reply(
            "createCustomCursor/windows",
            Value::map([("name", Value::from("unused"))]),
            |reply| {
                record(|results| results.cursor_unknown_method = Some(reply == Ok(None)));
            },
        );

        system::MOUSE_CURSOR.invoke_with_reply(
            "activateSystemCursor",
            // No `kind`, which upstream's cursor_handler.cc reports as an
            // argument error rather than falling back to the arrow.
            Value::map([("device", Value::I64(0))]),
            |reply| {
                let code = match reply {
                    Err(error) => error.code,
                    other => format!("expected an error, got {other:?}"),
                };
                record(|results| results.cursor_bad_arguments = Some(code));
            },
        );

        // Sent last so that its reply, which is the one that says the cursor
        // was actually set, cannot be confused with the others.
        system::MOUSE_CURSOR.invoke_with_reply(
            "activateSystemCursor",
            Value::map([
                ("device", Value::I64(0)),
                ("kind", Value::from(SystemMouseCursor::Text.kind())),
            ]),
            |reply| {
                record(|results| results.cursor_ok = Some(reply == Ok(Some(Value::Null))));
            },
        );
    }

    /// Reads back what the window does with the cursor it was given.
    fn read_cursor(&self) {
        let claimed = probe::ask_to_set_cursor(self.window);
        // `None` where the shape cannot be compared: the cursor belongs to
        // whichever window the pointer is actually over, so this only answers
        // when that is this window. See `probe::current_cursor`.
        let applied =
            probe::current_cursor(self.window).map(|cursor| cursor == probe::text_cursor());
        record(|results| {
            results.cursor_claimed = Some(claimed);
            results.cursor_applied = Some(applied);
        });
    }
}

/// Compares what arrived against what was asked for, then closes the window.
fn finish() {
    // Once. Asking to exit does not stop the frames arriving -- on a desktop
    // the window is destroyed a few messages later, and on Android the
    // Activity finishes on its own schedule -- so without this the report is
    // printed on every frame between the request and the process going away.
    use std::sync::atomic::AtomicBool;
    static REPORTED: AtomicBool = AtomicBool::new(false);
    if REPORTED.swap(true, Ordering::SeqCst) {
        return;
    }

    let failures = RESULTS.with(|results| check(&results.borrow()));
    EXIT_CODE.store(failures, Ordering::SeqCst);
    println!(
        "{}",
        if failures == 0 { "platform_channels: PASS" } else { "platform_channels: FAILED" }
    );
    // The last thing this checks. A required exit is not a question, so the
    // host closes the window without asking -- which is why this gets past the
    // handler installed above that refuses everything. The exit code travels
    // with it and comes back as the process's, through PostQuitMessage and
    // rf_host_run rather than through EXIT_CODE.
    system::exit_application(system::AppExitType::Required, failures, |_| {});
}

fn check(results: &Results) -> i32 {
    let mut failures = 0;
    let mut fail = |what: &str| {
        println!("  FAIL {what}");
        failures += 1;
    };

    match results.lifecycle.first() {
        // Sent before a handler existed, so this also proves the buffering.
        Some(system::AppLifecycleState::Resumed) => {}
        Some(other) => fail(&format!("the first lifecycle state was {other:?}, not Resumed")),
        None => fail("no lifecycle state arrived"),
    }

    match results.has_strings {
        Some(true) => {}
        Some(false) => fail("hasStrings said no after setData said yes"),
        None => fail("hasStrings never answered"),
    }

    match &results.clipboard {
        Some(Some(text)) if text == MARKER => {}
        Some(Some(text)) => fail(&format!("the clipboard read back {text:?}")),
        Some(None) => fail("the clipboard read back nothing"),
        None => fail("getData never answered"),
    }

    match results.absent {
        Some(true) => {}
        Some(false) => fail("an unserved channel did not answer with Ok(None)"),
        None => fail("an unserved channel never answered at all"),
    }

    match results.bad_format.as_deref() {
        // The host's code, copied from upstream's platform_handler.cc. An
        // application branches on it, so it is part of the protocol.
        Some("Clipboard error") => {}
        Some(other) => fail(&format!("the error code was {other:?}")),
        None => fail("the bad format never answered"),
    }

    // Typing, through a field the application declared and never wired to the
    // keyboard. If this works, text input works.
    //
    // Everything from here to the settings needs a synthetic gesture, and a
    // platform that will not let an application inject one into itself cannot
    // have it. See android.rs for why that is the platform's decision rather
    // than something missing here.
    if !probe::DRIVES_INPUT {
        println!(
            "  SKIP typing, composition and the cursor: this platform does not let an \
             application inject input into its own window"
        );
    } else {
        if !results.typed_arrived() {
            fail(&format!(
                "the field never held {TYPED:?}; it saw {:?}",
                results.typed
            ));
        }
        if results.typed.len() < TYPED.chars().count() {
            fail(&format!(
                "{} characters produced only {} changes, so the field would not have\n       \
                 redrawn as it was typed into",
                TYPED.chars().count(),
                results.typed.len()
            ));
        }

        // The IME. Driven through the input context rather than by hand, which
        // exercises the same WM_IME_* messages a reader typing pinyin would.
        match results.composition_started {
            Some(false) => {
                println!(
                    "  SKIP composition: this machine has no input context to compose in"
                );
            }
            Some(true) => {
                match results.composing_text.as_deref() {
                    Some(text) if text.ends_with(COMPOSED) => {}
                    Some(text) => fail(&format!("the composing text was {text:?}")),
                    None => fail("a composition started but never reached the field"),
                }
                match results.committed_text.as_deref() {
                    Some(text) if !text.ends_with(COMPOSED) => {}
                    Some(text) => fail(&format!("committing left the text at {text:?}")),
                    None => fail("the composition was never committed"),
                }
            }
            None => fail("the composition was never attempted"),
        }

        // The mouse cursor, which is the only channel here that speaks the binary
        // standard codec rather than JSON.
        match results.cursor_ok {
            Some(true) => {}
            Some(false) => fail("activateSystemCursor did not come back a plain success"),
            None => fail("activateSystemCursor never answered"),
        }
        match results.cursor_unknown_method {
            Some(true) => {}
            Some(false) => fail("a mousecursor method nobody serves did not answer Ok(None)"),
            None => fail("a mousecursor method nobody serves never answered"),
        }
        match results.cursor_bad_arguments.as_deref() {
            // Upstream's cursor_handler.cc code, which an application branches on.
            Some("Argument error") => {}
            Some(other) => fail(&format!("the missing-kind error code was {other:?}")),
            None => fail("a call with no kind never answered"),
        }
        match results.cursor_claimed {
            Some(true) => {}
            // Without this the class cursor comes back on the next mouse move.
            _ => fail("the window did not claim WM_SETCURSOR, so the choice would not stick"),
        }
        match results.cursor_applied {
            Some(Some(true)) => {}
            Some(Some(false)) => fail("the cursor was set to something other than the I-beam"),
            Some(None) => println!(
                "  SKIP cursor shape: the pointer is not over this window, so the cursor\n       \
                 belongs to whichever window it is over"
            ),
            None => fail("the cursor was never read back"),
        }
    }

    // The user's settings, which arrive without ever being a channel message
    // the framework sees: `Engine` takes them and hands them over directly.
    let settings = platform::user_settings();
    if settings.text_scale_factor <= 0.0 {
        fail(&format!(
            "the text scale factor is {}, which would lay every glyph out at no width",
            settings.text_scale_factor
        ));
    }
    match probe::prefers_dark_theme() {
        Some(dark) => {
            let expected = if dark {
                platform::Brightness::Dark
            } else {
                platform::Brightness::Light
            };
            if settings.platform_brightness != expected {
                fail(&format!(
                    "the platform says {:?} but the system says {expected:?}",
                    settings.platform_brightness
                ));
            }
        }
        None => println!(
            "  SKIP the brightness cross-check: this platform has only one place to
                    read it from, and the host already read it there"
        ),
    }

    // The languages. Checked for shape rather than content: what they are is
    // the machine's business, but a language code is never empty and a tag
    // never starts with a hyphen.
    let locales = platform::locales();
    if locales.is_empty() {
        fail("no locales arrived");
    }
    for locale in &locales {
        if locale.language_code.is_empty() {
            fail("a locale arrived with no language code");
        }
        if locale.to_language_tag().starts_with('-') {
            fail(&format!("the tag {:?} is missing its language", locale.to_language_tag()));
        }
    }

    // Closing, refused. Still being here afterwards is the whole point of the
    // protocol: without it an application could not stop to save.
    match results.exit_requested {
        Some(system::AppExitType::Cancelable) => {}
        Some(other) => fail(&format!("the close was reported as {other:?}, not cancelable")),
        None => fail("the close button never reached the framework"),
    }
    match results.survived_refusal {
        Some(true) => {}
        Some(false) => fail("refusing the close did not keep the application up"),
        None => fail("nothing checked whether the application survived"),
    }

    failures
}

#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(_argc: c_int, _argv: *const *const c_char) -> c_int {
    rustflutter::register_application(|| {
        Box::new(WidgetHost::new(Probe {
            phase: Phase::Asking,
            frames: 0,
            window: std::ptr::null_mut(),
        }))
    });
    let result = rustflutter::app::run(&RunOptions {
        width: 480,
        height: 320,
        title: "platform channels".to_string(),
        impeller: false,
    });
    match result {
        Ok(()) => EXIT_CODE.load(Ordering::SeqCst),
        Err(code) => code,
    }
}
