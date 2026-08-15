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

use rustflutter::prelude::*;
use rustflutter::services::system;
use rustflutter::services::{MethodChannel, StandardMethodCodec};

mod win32;

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
        !self.lifecycle.is_empty()
            && self.has_strings.is_some()
            && self.clipboard.is_some()
            && self.absent.is_some()
            && self.bad_format.is_some()
            && self.typed_arrived()
            && self.composition_settled()
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
    Checking,
}

struct Probe {
    phase: Phase,
    frames: u32,
    window: win32::Hwnd,
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
            (Phase::Asking, 2) => {
                self.ask();
                self.window = win32::find_window();
                if self.window.is_null() {
                    println!("  FAIL the host window could not be found");
                }
                self.phase = Phase::Focusing;
            }
            // A real click, which is how a field is focused. Nothing else in
            // this file knows that focusing opens a platform connection.
            (Phase::Focusing, 6) => {
                win32::click_centre(self.window);
                self.phase = Phase::Typing;
            }
            (Phase::Typing, 12) => {
                for character in TYPED.chars() {
                    win32::type_char(self.window, character);
                }
                self.phase = Phase::Composing;
            }
            (Phase::Composing, 24) => {
                let started = win32::compose(self.window, COMPOSED);
                record(|results| results.composition_started = Some(started));
                self.phase = Phase::Committing;
            }
            (Phase::Committing, 36) => {
                win32::commit_composition(self.window);
                self.phase = Phase::Checking;
            }
            _ => {}
        }

        let done = RESULTS.with(|results| results.borrow().complete());
        if (done && self.frames > 36) || self.frames >= FRAME_BUDGET {
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
    }
}

/// Compares what arrived against what was asked for, then closes the window.
fn finish() {
    let failures = RESULTS.with(|results| check(&results.borrow()));
    EXIT_CODE.store(failures, Ordering::SeqCst);
    println!(
        "{}",
        if failures == 0 { "platform_channels: PASS" } else { "platform_channels: FAILED" }
    );
    // Outbound with no reply, and the host acts on it: the window closes and
    // the process ends, which is the last thing this checks.
    SystemNavigator::pop();
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
