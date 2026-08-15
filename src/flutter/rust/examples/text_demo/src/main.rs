// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! `flutter/textinput`, by hand.
//!
//! The fourth of the manual windows, and the one that needs a person most: a
//! keyboard is the piece of the platform that cannot be driven from inside the
//! process it is typing into. `platform_channels` drives one on Windows by
//! posting `WM_CHAR` at its own window, which Android will not let an
//! application do to itself -- see that example's `android.rs`. So this is
//! where typing gets checked on a touch screen: by typing.
//!
//! What to try:
//!
//! 1. **Tap the field.** The soft keyboard comes up, or on a desktop the caret
//!    starts blinking. That is `TextInput.setClient` followed by
//!    `TextInput.show`, and nothing in this file sends either.
//! 2. **Type.** Every keystroke crosses to the platform, edits the engine's own
//!    `TextInputModel` there, and comes back as `updateEditingState`. The
//!    counter below says how many times that round trip has happened.
//! 3. **Backspace, and move the caret.** Deleting is not "the text minus a
//!    character": it is `deleteSurroundingText` on Android and `VK_BACK` on
//!    Windows, and both land on the same model.
//! 4. **Type a character that is not ASCII** -- an emoji, or 中. It crosses as
//!    UTF-16, is held as UTF-8, and comes back with offsets counted in UTF-16
//!    code units. Three encodings, and the selection numbers below show whether
//!    the conversions agree.
//! 5. **Press done / enter.** `TextInputClient.performAction` arrives, and the
//!    submitted line is added to the list.
//! 6. **Tap the second field.** The first one's client is cleared and the
//!    second one's is set; the platform is told, which is what stops the
//!    keyboard editing a field that no longer has focus.
//!
//! There is no `TextInput`, no client id, no editing state and no IME anywhere
//! below. Adapting to those is the framework's job, and it is done once.

use std::cell::RefCell;
use std::os::raw::{c_char, c_int};

use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex, RenderPadding};

const WIDTH: i32 = 560;
const HEIGHT: i32 = 620;

thread_local! {
    /// What each field holds, as the application hears it. Plain strings: the
    /// composing run, the selection and the client id are all below this.
    static TEXT: RefCell<[String; 2]> = RefCell::new([String::new(), String::new()]);
    /// How many times a field has reported a change. On screen because a round
    /// trip that produced the same text is still a round trip, and that is the
    /// half a person cannot otherwise tell from the half that is drawing.
    static CHANGES: RefCell<u32> = const { RefCell::new(0) };
    /// Everything that has been submitted, newest last.
    static SUBMITTED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

#[derive(Default)]
struct State {
    /// Bumped from the field callbacks. The cells above are the truth; this is
    /// only how the element tree hears about it.
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
        let text = TEXT.with(|text| text.borrow().clone());
        let changes = CHANGES.with(|count| *count.borrow());
        let submitted = SUBMITTED.with(|list| list.borrow().clone());

        let mut children = vec![
            component(Label::title("flutter/textinput")),
            component(Label::muted(
                "tap a field and type. every keystroke goes to the platform and \
                 comes back.",
            )),
            gap(1.0),
            field(0, "your name", handle.clone()),
            component(Label::muted(format!(
                "   {} characters, {} UTF-16 units",
                text[0].chars().count(),
                text[0].encode_utf16().count()
            ))),
            gap(0.5),
            field(1, "and something with 中 or an emoji", handle),
            component(Label::muted(format!(
                "   {} characters, {} UTF-16 units",
                text[1].chars().count(),
                text[1].encode_utf16().count()
            ))),
            gap(1.0),
            component(Label::new(format!("changes reported: {changes}"))),
        ];

        if submitted.is_empty() {
            children.push(component(Label::muted(
                "nothing submitted yet -- press done or enter",
            )));
        } else {
            children.push(component(Label::new(format!(
                "submitted {} time(s):",
                submitted.len()
            ))));
            for line in submitted.iter().rev().take(4) {
                children.push(component(Label::muted(format!("   {line:?}"))));
            }
        }

        children.push(gap(1.0));
        children.push(component(Label::muted(
            "the character counts are the check worth watching: the text crosses \
             as UTF-16 and is held as UTF-8, so a wrong conversion shows up here \
             and nowhere else.",
        )));

        provide(Theme::dark(), page(children))
    }
}

/// One field. The whole of the application's side of this channel.
fn field(index: usize, placeholder: &str, handle: StateHandle<State>) -> AnyWidget {
    let changed = handle.clone();
    let submitted = handle;
    stateful(
        TextField::new(index as u64 + 1)
            .with_placeholder(placeholder)
            .with_on_changed(move |text| {
                let text = text.to_string();
                TEXT.with(|held| {
                    let mut held = held.borrow_mut();
                    if held[index] == text {
                        return;
                    }
                    held[index] = text;
                    CHANGES.with(|count| *count.borrow_mut() += 1);
                });
                changed.set_state(|state| state.version += 1);
            })
            .with_on_submitted(move |text| {
                SUBMITTED.with(|list| list.borrow_mut().push(text.to_string()));
                submitted.set_state(|state| state.version += 1);
            }),
    )
}

/// A padded, left-aligned column.
fn page(children: Vec<AnyWidget>) -> AnyWidget {
    many(children, |rendered| {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(8.0);
        for child in rendered {
            column = column.push(child);
        }
        Box::new(RenderPadding::new(EdgeInsets::all(24.0), column))
    })
}

struct TextApp;

impl WidgetApplication for TextApp {
    fn background(&self) -> Color {
        Theme::dark().background
    }

    fn build(&mut self, _context: &BuildContext) -> AnyWidget {
        component(SafeArea::new(stateful(Page)))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(_argc: c_int, _argv: *const *const c_char) -> c_int {
    register_application(|| Box::new(WidgetHost::new(TextApp)));
    let options = RunOptions {
        width: WIDTH,
        height: HEIGHT,
        title: String::from("Text input - rustflutter"),
        ..RunOptions::default()
    };
    match run(&options) {
        Ok(()) => 0,
        Err(code) => code,
    }
}
