// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! `System.requestAppExit` and `System.exitApplication`, by hand.
//!
//! Asking to leave is a question, not a command. This application refuses to
//! answer yes until you let it, which is the point of the protocol: an
//! application with unsaved work needs somewhere to stand between the reader
//! asking to go and the window going away.
//!
//! What does the asking differs by platform and nothing else does: on a desktop
//! it is the close button, and on Android it is the back gesture.
//!
//! What to try, in this order:
//!
//! 1. **Leave "may close" off and click the X.** The window stays. The counter
//!    goes up, and the type says `cancelable` -- that is the platform asking
//!    rather than telling.
//! 2. **Click "SystemNavigator.pop()".** Same thing. `pop` posts the same
//!    `WM_CLOSE` a close button does, so it is refusable too.
//! 3. **Turn "may close" on, then click the X.** It closes.
//! 4. **Click "exit(required, 3)" with "may close" still off.** It closes
//!    anyway, because a required exit is not a question -- and the process
//!    exits with 3. Check it: `./exit_demo.exe; echo $?`
//!
//! The state lives in plain cells rather than in the element's state, because
//! the exit handler is not a widget: it is installed once and has to answer
//! from wherever it is called.

use std::cell::Cell;
use std::os::raw::{c_char, c_int};

use rustflutter::prelude::*;
use rustflutter::render::{CrossAxisAlignment, MainAxisSize, RenderFlex, RenderPadding};
use rustflutter::services::system;

const WIDTH: i32 = 560;
const HEIGHT: i32 = 560;

thread_local! {
    /// What the exit handler answers. Read from the handler, written by the
    /// switch.
    static MAY_CLOSE: Cell<bool> = const { Cell::new(false) };
    /// How many times the platform has asked.
    static REQUESTS: Cell<u32> = const { Cell::new(0) };
    /// What the last request said it was.
    static LAST_KIND: Cell<Option<AppExitType>> = const { Cell::new(None) };
    /// Whether the handler has been installed. Registering it twice would be
    /// harmless -- the second replaces the first -- but it would also hide a
    /// mistake about when builds happen.
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
}

const SWITCH_MAY_CLOSE: u64 = 1;
const BUTTON_POP: u64 = 2;
const BUTTON_EXIT_0: u64 = 3;
const BUTTON_EXIT_3: u64 = 4;
const BUTTON_CANCELABLE: u64 = 5;

#[derive(Default)]
struct State {
    /// Bumped to force a rebuild when a cell changes. The cells are the truth;
    /// this is only how the tree hears about it.
    version: u32,
    pressed: Option<u64>,
}

struct Page {
    /// Whether two buttons fit side by side. A phone is narrower than the
    /// window this was written for, and a row of two runs off the side.
    wide: bool,
}

impl StatefulComponent for Page {
    type State = State;

    fn build(
        &self,
        state: &State,
        handle: StateHandle<State>,
        _context: &mut rustflutter::framework::BuildContext,
    ) -> AnyWidget {
        install_handler(handle.clone());

        let may_close = MAY_CLOSE.with(Cell::get);
        let requests = REQUESTS.with(Cell::get);
        let last = LAST_KIND.with(Cell::get);

        let children = vec![
            component(Label::title("System.requestAppExit")),
            component(Label::muted(
                "the close button -- or the back gesture -- is a question. \
                 this application can say no.",
            )),
            gap(1.0),
            component(Label::new(format!("the platform has asked {requests} time(s)"))),
            component(Label::new(match last {
                Some(kind) => format!("the last one was {kind:?}"),
                None => String::from("nothing has asked yet"),
            })),
            component(Label::new(format!(
                "this window answers: {}",
                if may_close { "exit" } else { "cancel" }
            ))),
            gap(1.0),
            stack_row(
                vec![
                    component(Label::new("may close")),
                    component(
                        Switch::new(SWITCH_MAY_CLOSE, may_close)
                            .wired(handle.clone(), |state: &mut State| {
                                MAY_CLOSE.with(|flag| flag.set(!flag.get()));
                                state.version += 1;
                            }),
                    ),
                ],
                12.0,
            ),
            gap(1.0),
            self.buttons(
                vec![
                    tap(
                        BUTTON_POP,
                        "SystemNavigator.pop()",
                        ButtonVariant::Outlined,
                        state,
                        handle.clone(),
                        || SystemNavigator::pop(),
                    ),
                    tap(
                        BUTTON_CANCELABLE,
                        "exit(cancelable, 0)",
                        ButtonVariant::Outlined,
                        state,
                        handle.clone(),
                        || system::exit_application(AppExitType::Cancelable, 0, |_| {}),
                    ),
                ],
            ),
            self.buttons(
                vec![
                    tap(
                        BUTTON_EXIT_0,
                        "exit(required, 0)",
                        ButtonVariant::Filled,
                        state,
                        handle.clone(),
                        || system::exit_application(AppExitType::Required, 0, |_| {}),
                    ),
                    tap(
                        BUTTON_EXIT_3,
                        "exit(required, 3)",
                        ButtonVariant::Danger,
                        state,
                        handle.clone(),
                        || system::exit_application(AppExitType::Required, 3, |_| {}),
                    ),
                ],
            ),
            component(Label::muted(
                "a required exit is not a question: it closes even with the \
                 switch off, and its code becomes the process's.",
            )),
        ];

        provide(Theme::dark(), page(children))
    }
}

impl Page {
    /// Two buttons side by side where there is room, and stacked where there
    /// is not.
    fn buttons(&self, buttons: Vec<AnyWidget>) -> AnyWidget {
        if self.wide {
            stack_row(buttons, 10.0)
        } else {
            many(buttons, |rendered| {
                let mut column = RenderFlex::column()
                    .with_main_axis_size(MainAxisSize::Min)
                    .with_cross_axis_alignment(CrossAxisAlignment::Start)
                    .with_spacing(10.0);
                for child in rendered {
                    column = column.push(child);
                }
                Box::new(column)
            })
        }
    }
}

/// Installs the exit handler, once.
///
/// Inside a build because that is the first place a `StateHandle` exists, and
/// the handler needs one: an exit request arrives from the platform, and the
/// counter it bumps is on screen.
fn install_handler(handle: StateHandle<State>) {
    if INSTALLED.with(Cell::get) {
        return;
    }
    INSTALLED.with(|installed| installed.set(true));
    system::on_exit_requested(move |kind| {
        REQUESTS.with(|count| count.set(count.get() + 1));
        LAST_KIND.with(|last| last.set(Some(kind)));
        handle.set_state(|state| state.version += 1);
        if MAY_CLOSE.with(Cell::get) {
            AppExitResponse::Exit
        } else {
            AppExitResponse::Cancel
        }
    });
}

/// A button whose tap runs a closure, which `Button::wired` cannot do.
fn tap(
    id: u64,
    label: &str,
    style: ButtonVariant,
    state: &State,
    handle: StateHandle<State>,
    action: fn(),
) -> AnyWidget {
    use rustflutter::gestures::PointerHandlers;
    let press = handle;
    let handlers = PointerHandlers::new()
        .with_tap(move |_| action())
        .with_press_change(move |down| {
            press.set_state(move |state| {
                state.pressed = if down { Some(id) } else { None };
            });
        });
    component(
        Button::new(id, label)
            .with_style(style)
            .with_pressed(state.pressed == Some(id))
            .with_min_width(200.0)
            .with_handlers(handlers),
    )
}

/// A padded, left-aligned column.
fn page(children: Vec<AnyWidget>) -> AnyWidget {
    many(children, |rendered| {
        let mut column = RenderFlex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(10.0);
        for child in rendered {
            column = column.push(child);
        }
        Box::new(RenderPadding::new(EdgeInsets::all(24.0), column))
    })
}

struct ExitApp;

impl WidgetApplication for ExitApp {
    fn background(&self) -> Color {
        Theme::dark().background
    }

    fn build(&mut self, context: &BuildContext) -> AnyWidget {
        // Two 200-wide buttons, a gap and the page's margins.
        component(SafeArea::new(stateful(Page { wide: context.size.width >= 470.0 })))
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(_argc: c_int, _argv: *const *const c_char) -> c_int {
    register_application(|| Box::new(WidgetHost::new(ExitApp)));
    let options = RunOptions {
        width: WIDTH,
        height: HEIGHT,
        title: String::from("App exit - rustflutter"),
        ..RunOptions::default()
    };
    match run(&options) {
        // The interesting return: an exit code the framework asked for comes
        // back through PostQuitMessage and rf_host_run, not from here.
        Ok(()) => 0,
        Err(code) => code,
    }
}
