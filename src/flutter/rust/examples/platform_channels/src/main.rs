// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Platform channels, end to end, against a real shell.
//!
//! Two jobs at once. It is the worked example for `rustflutter::services` --
//! every shape a channel comes in appears here once -- and it is the only test
//! that crosses the C ABI, because the unit tests stand a recorder in for the
//! shell and never reach the engine at all.
//!
//! It checks itself and closes itself: each expectation is recorded as it
//! arrives, the results are compared after a few frames, and the process exits
//! non-zero if anything is missing or wrong. Run it and read the exit code.

use std::cell::RefCell;
use std::os::raw::{c_char, c_int};
use std::sync::atomic::{AtomicI32, Ordering};

use rustflutter::prelude::*;
use rustflutter::services::system;
use rustflutter::services::{MethodChannel, StandardMethodCodec};

/// What the process exits with. Set once, from the UI thread, before the
/// window closes; read on the way out of `main`.
static EXIT_CODE: AtomicI32 = AtomicI32::new(0);

/// The text this puts on the clipboard and expects to read back.
const MARKER: &str = "rustflutter platform_channels";

/// How long to wait before deciding an answer is not coming. Every reply here
/// crosses to the platform thread and back, which is a handful of frames at
/// most; a hundred is a timeout, not a schedule.
const FRAME_BUDGET: u32 = 100;

#[derive(Default)]
struct Results {
    lifecycle: Vec<system::AppLifecycleState>,
    has_strings: Option<bool>,
    clipboard: Option<Option<String>>,
    absent: Option<bool>,
    bad_format: Option<String>,
}

impl Results {
    /// The one thing every answer has in common: it has to arrive.
    fn complete(&self) -> bool {
        !self.lifecycle.is_empty()
            && self.has_strings.is_some()
            && self.clipboard.is_some()
            && self.absent.is_some()
            && self.bad_format.is_some()
    }
}

thread_local! {
    static RESULTS: RefCell<Results> = RefCell::new(Results::default());
}

fn record(update: impl FnOnce(&mut Results)) {
    RESULTS.with(|results| update(&mut results.borrow_mut()));
}

struct Probe {
    asked: bool,
    frames: u32,
}

impl Application for Probe {
    fn background(&self) -> Color {
        Color::rgb(0x0E, 0x16, 0x26)
    }

    fn build(&mut self, context: &BuildContext) -> BoxedWidget {
        if !self.asked {
            self.asked = true;
            self.ask();
        }

        self.frames += 1;
        let done = RESULTS.with(|results| results.borrow().complete());
        if done || self.frames >= FRAME_BUDGET {
            self.finish();
        }
        context.scheduler.request_frame();

        Box::new(Center::new(
            Text::new("platform channels").with_size(28.0).with_color(Color::WHITE),
        ))
    }
}

impl Probe {
    /// Everything the probe wants to know, asked at once.
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
        // reply -- and the point is that it arrives at all rather than leaving
        // the caller waiting.
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

    /// Compares what arrived against what was asked for, then closes.
    fn finish(&self) {
        let failures = RESULTS.with(|results| check(&results.borrow()));
        EXIT_CODE.store(failures, Ordering::SeqCst);
        println!(
            "{}",
            if failures == 0 { "platform_channels: PASS" } else { "platform_channels: FAILED" }
        );
        // Outbound with no reply, and the host acts on it: the window closes
        // and the process ends, which is the last thing this checks.
        SystemNavigator::pop();
    }
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

    failures
}

#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(_argc: c_int, _argv: *const *const c_char) -> c_int {
    rustflutter::register_application(|| Box::new(Probe { asked: false, frames: 0 }));
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
