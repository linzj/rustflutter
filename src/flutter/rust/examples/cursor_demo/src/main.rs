// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! `flutter/mousecursor`, by hand.
//!
//! Tap a name and the pointer changes shape. That is the whole program, and it
//! is the one thing about this channel a test cannot check: the cursor is a
//! single shared resource for the whole desktop, handed to whichever window the
//! pointer happens to be over, so reading it back only says what the pointer is
//! over at that instant.
//!
//! What a person can see, and a test cannot:
//!
//! * the shape changes **immediately**, without waiting for the pointer to
//!   move -- that is the posted message half of the host's handling;
//! * the shape **stays** changed as the pointer moves around the window --
//!   that is the `WM_SETCURSOR` half, which has to return TRUE or the window
//!   class's arrow comes straight back;
//! * `none` really does hide it, because "none" is `SetCursor(nullptr)` rather
//!   than a picture of nothing.
//!
//! Press any key to get the arrow back, which matters after tapping `none`.

use std::os::raw::{c_char, c_int};

use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex, RenderPadding};

const WIDTH: i32 = 700;
const HEIGHT: i32 = 620;

/// Every kind the Windows host maps to a real `HCURSOR`: the plain ones first,
/// then the resize arrows, which are only worth comparing next to each other.
const KINDS: &[(SystemMouseCursor, &str)] = &[
    (SystemMouseCursor::Basic, "basic"),
    (SystemMouseCursor::Click, "click"),
    (SystemMouseCursor::Text, "text"),
    (SystemMouseCursor::Forbidden, "forbidden"),
    (SystemMouseCursor::Help, "help"),
    (SystemMouseCursor::Progress, "progress"),
    (SystemMouseCursor::Wait, "wait"),
    (SystemMouseCursor::Precise, "precise"),
    (SystemMouseCursor::Move, "move"),
    (SystemMouseCursor::NoDrop, "noDrop"),
    (SystemMouseCursor::AllScroll, "allScroll"),
    (SystemMouseCursor::None, "none - hides it"),
    (SystemMouseCursor::ResizeLeftRight, "resizeLeftRight"),
    (SystemMouseCursor::ResizeUpDown, "resizeUpDown"),
    (SystemMouseCursor::ResizeUpLeftDownRight, "resizeUpLeftDownRight"),
    (SystemMouseCursor::ResizeUpRightDownLeft, "resizeUpRightDownLeft"),
    (SystemMouseCursor::ResizeColumn, "resizeColumn"),
    (SystemMouseCursor::ResizeRow, "resizeRow"),
];

/// How many buttons fit across the window.
///
/// A phone is narrower than the window this was written for, and a row of
/// three there runs off the side. The width is known at build time, so it is
/// asked rather than assumed.
fn columns_for(width: f32) -> usize {
    const BUTTON: f32 = 200.0;
    const GAP: f32 = 10.0;
    const MARGIN: f32 = 48.0;
    let usable = width - MARGIN;
    let fits = ((usable + GAP) / (BUTTON + GAP)).floor() as usize;
    fits.clamp(1, 3)
}

#[derive(Default)]
struct State {
    /// Which kind was last asked for, by index into `KINDS`.
    active: usize,
    pressed: Option<u64>,
}

struct Page {
    /// How many buttons fit across, worked out from the view's width by the
    /// application root, which is the only place the width is known.
    columns: usize,
    /// Whether this platform has a pointer to change the shape of at all.
    has_pointer: bool,
}

impl StatefulComponent for Page {
    type State = State;

    fn build(
        &self,
        state: &State,
        handle: StateHandle<State>,
        _context: &mut rustflutter::framework::BuildContext,
    ) -> AnyWidget {
        let mut children = vec![
            component(Label::title("flutter/mousecursor")),
            component(Label::muted(format!(
                "asked for: {}", KINDS[state.active].1
            ))),
            component(Label::muted(if self.has_pointer {
                "move the pointer over this window. it should change at once, \
                 and stay changed as you move."
            } else {
                // Said rather than hidden: the channel is served here, the
                // calls below really are sent and really are answered, and
                // there is simply nothing on a touch screen for the answer to
                // change. A demo that quietly did nothing would be worse.
                "this is a touch screen, so there is no pointer to change \
                 the shape of. the calls below are still sent and still \
                 answered -- `asked for` above follows them."
            })),
            component(Label::muted("any key puts the arrow back.")),
            gap(1.0),
        ];

        for row in (0..KINDS.len()).collect::<Vec<_>>().chunks(self.columns) {
            let buttons: Vec<AnyWidget> = row
                .iter()
                .map(|index| button(*index, state, handle.clone()))
                .collect();
            children.push(stack_row(buttons, 10.0));
        }

        // Provided here rather than at the application root, because only what
        // a tap rebuilds is rebuilt and the root is not that.
        provide(Theme::dark(), page(children))
    }
}

/// One button that asks for one cursor.
///
/// Built with explicit handlers rather than `Button::wired`, because `wired`
/// takes a plain `fn` and this has to capture which kind it is.
fn button(index: usize, state: &State, handle: StateHandle<State>) -> AnyWidget {
    let id = index as u64 + 1;
    let (cursor, label) = KINDS[index];
    let tap = handle.clone();
    let press = handle;
    let handlers = PointerHandlers::new()
        .with_tap(move |_| {
            // The whole of the application's side of this channel.
            cursor.activate(0);
            tap.set_state(move |state| state.active = index);
        })
        .with_press_change(move |down| {
            press.set_state(move |state| {
                state.pressed = if down { Some(id) } else { None };
            });
        });

    component(
        Button::new(id, label)
            .with_style(if index == state.active {
                ButtonStyle::Filled
            } else {
                ButtonStyle::Outlined
            })
            .with_pressed(state.pressed == Some(id))
            .with_min_width(200.0)
            .with_handlers(handlers),
    )
}

/// A padded, left-aligned column. `stack_column` stretches its children, which
/// here would make every row as wide as the window.
fn page(children: Vec<AnyWidget>) -> AnyWidget {
    many(children, |rendered| {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(12.0);
        for child in rendered {
            column = column.push(child);
        }
        Box::new(RenderPadding::new(EdgeInsets::all(24.0), column))
    })
}

struct CursorApp;

impl WidgetApplication for CursorApp {
    fn background(&self) -> Color {
        Theme::dark().background
    }

    /// Any key puts the arrow back.
    ///
    /// Not decoration: after tapping `none` there is no pointer to aim with,
    /// and a demo that can paint itself into that corner is a bad demo.
    fn on_key(&mut self, event: &KeyEvent, _keyboard: &Keyboard) -> bool {
        if event.change == KeyChange::Down {
            SystemMouseCursor::Basic.activate(0);
        }
        false
    }

    fn build(&mut self, context: &BuildContext) -> AnyWidget {
        stateful(Page {
            columns: columns_for(context.size.width),
            // A pointer is a desktop thing. There is no channel that reports
            // this, and inventing one would be inventing protocol, so the
            // target is what decides.
            has_pointer: cfg!(not(target_os = "android")),
        })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(_argc: c_int, _argv: *const *const c_char) -> c_int {
    register_application(|| Box::new(WidgetHost::new(CursorApp)));
    let options = RunOptions {
        width: WIDTH,
        height: HEIGHT,
        title: String::from("Mouse cursor - rustflutter"),
        ..RunOptions::default()
    };
    match run(&options) {
        Ok(()) => 0,
        Err(code) => code,
    }
}
