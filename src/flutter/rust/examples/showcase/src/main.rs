// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The component library, and the theme mechanism under it.
//!
//! Every control on screen reads its colours from a [`Theme`] published at the
//! root with `provide`. The switch in the title bar swaps that theme, which is
//! the point of the example: one `set_state` at the top changes a value, and
//! every component below it repaints in the new palette without any of them
//! knowing about each other.
//!
//! `--png <path>` renders headlessly; `--light` starts in the light theme.

use std::os::raw::{c_char, c_int};

use rustflutter::components::{gap, stack_column, stack_row};
use rustflutter::framework::{ElementTree, provide};
use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, BoxConstraints, CrossAxisAlignment, MainAxisSize, Offset, RenderBox,
    RenderFlex,
};
use rustflutter::widgets::{Align, Container, Empty, ListView};

const WIDTH: i32 = 460;
const HEIGHT: i32 = 700;

// Hit-test identities. Handed out by hand rather than by an IdSource so that
// the same control keeps the same id across rebuilds -- an id that shifted
// would break a press that is in progress.
const ID_THEME_SWITCH: u64 = 1;
const ID_PRIMARY: u64 = 2;
const ID_OUTLINED: u64 = 3;
const ID_TEXT: u64 = 4;
const ID_DANGER: u64 = 5;
const ID_DISABLED: u64 = 6;
const ID_SLIDER: u64 = 7;
const ID_NOTIFICATIONS: u64 = 8;
const ID_TILE: u64 = 9;

#[derive(Default)]
struct ShowcaseState {
    light: bool,
    pressed: Option<u64>,
    volume: f32,
    notifications: bool,
    taps: u32,
}

struct Showcase {
    start_light: bool,
}

impl StatefulComponent for Showcase {
    type State = ShowcaseState;

    fn build(
        &self,
        state: &ShowcaseState,
        handle: StateHandle<ShowcaseState>,
        _context: &mut rustflutter::framework::BuildContext,
    ) -> AnyWidget {
        // The very first build has default state, which is dark; honour the
        // command line by flipping it once.
        let light = state.light || (self.start_light && state.taps == 0 && !state.light);
        let theme = if light { Theme::light() } else { Theme::dark() };

        provide(
            theme,
            component(Page {
                light,
                pressed: state.pressed,
                volume: if state.volume == 0.0 { 0.4 } else { state.volume },
                notifications: state.notifications,
                taps: state.taps,
                handle,
            }),
        )
    }
}

struct Page {
    light: bool,
    pressed: Option<u64>,
    volume: f32,
    notifications: bool,
    taps: u32,
    handle: StateHandle<ShowcaseState>,
}

impl Component for Page {
    fn build(&self, context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let spacing = theme.spacing;
        let handle = self.handle.clone();

        let app_bar = component(
            AppBar::new("Components")
                .with_subtitle(if self.light { "light theme" } else { "dark theme" })
                .with_trailing(component(
                    Switch::new(ID_THEME_SWITCH, self.light)
                        .wired(handle.clone(), |state| state.light = !state.light),
                )),
        );

        let body = component(Body {
            pressed: self.pressed,
            volume: self.volume,
            notifications: self.notifications,
            taps: self.taps,
            handle,
            spacing,
        });

        component(Scaffold::new(body).with_app_bar(app_bar))
    }
}

struct Body {
    pressed: Option<u64>,
    volume: f32,
    notifications: bool,
    taps: u32,
    handle: StateHandle<ShowcaseState>,
    spacing: f32,
}

impl Component for Body {
    fn build(&self, context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let theme = theme_of(context);
        let spacing = self.spacing;
        let handle = &self.handle;

        let buttons = component(Card::new(stack_column(
            vec![
                component(Label::title("Buttons")),
                component(Label::muted(format!("{} taps so far", self.taps))),
                gap(1.0),
                stack_row(
                    vec![
                        component(
                            Button::new(ID_PRIMARY, "Primary")
                                .with_pressed(self.pressed == Some(ID_PRIMARY))
                                .wired(handle.clone(), |s| &mut s.pressed, |s| s.taps += 1),
                        ),
                        component(
                            Button::new(ID_OUTLINED, "Outlined")
                                .with_style(ButtonStyle::Outlined)
                                .with_pressed(self.pressed == Some(ID_OUTLINED))
                                .wired(handle.clone(), |s| &mut s.pressed, |s| s.taps += 1),
                        ),
                    ],
                    spacing,
                ),
                stack_row(
                    vec![
                        component(
                            Button::new(ID_TEXT, "Text")
                                .with_style(ButtonStyle::Text)
                                .with_pressed(self.pressed == Some(ID_TEXT))
                                .wired(handle.clone(), |s| &mut s.pressed, |s| s.taps += 1),
                        ),
                        component(
                            Button::new(ID_DANGER, "Danger")
                                .with_style(ButtonStyle::Danger)
                                .with_pressed(self.pressed == Some(ID_DANGER))
                                .wired(handle.clone(), |s| &mut s.pressed, |s| s.taps = 0),
                        ),
                        component(
                            Button::new(ID_DISABLED, "Disabled")
                                .with_style(ButtonStyle::Outlined)
                                .with_enabled(false),
                        ),
                    ],
                    spacing,
                ),
            ],
            spacing,
        )));

        let controls = component(Card::new(stack_column(
            vec![
                component(Label::title("Controls")),
                gap(0.5),
                component(
                    ListTile::new("Notifications")
                        .with_subtitle("delivered while the app is closed")
                        .with_accent(theme.primary)
                        .with_trailing(component(
                            Switch::new(ID_NOTIFICATIONS, self.notifications).wired(
                                handle.clone(),
                                |state| state.notifications = !state.notifications,
                            ),
                        )),
                ),
                component(Divider),
                gap(0.5),
                component(Label::muted(format!(
                    "Volume {}%",
                    (self.volume * 100.0).round() as i32
                ))),
                component(
                    SliderRow {
                        value: self.volume,
                        handle: handle.clone(),
                    },
                ),
                gap(0.5),
                component(Label::muted("Progress follows the slider")),
                component(ProgressRow { value: self.volume }),
            ],
            spacing,
        )));

        let badges = component(Card::new(stack_column(
            vec![
                component(Label::title("Badges")),
                gap(0.5),
                stack_row(
                    vec![
                        component(Badge::new("layout")),
                        component(Badge::new("input").with_color(theme.danger)),
                        component(Badge::new("theme").with_color(theme.text_muted)),
                    ],
                    spacing,
                ),
            ],
            spacing,
        )));

        let tile = component(Card::new(stack_column(
            vec![
                component(Label::title("Tappable tile")),
                gap(0.5),
                component(
                    ListTile::new("Tap anywhere on this row")
                        .with_subtitle("the whole tile is one hit-test region")
                        .with_accent(theme.primary)
                        .tappable(
                            ID_TILE,
                            rustflutter::gestures::PointerHandlers::new().with_tap({
                                let handle = handle.clone();
                                move |_| {
                                    handle.set_state(|s| s.taps += 1);
                                }
                            }),
                        ),
                ),
            ],
            spacing,
        )));

        // A ListView so the page scrolls if the window is short.
        let cards = vec![buttons, controls, badges, tile];
        many(cards, move |rendered| {
            let mut column = RenderFlex::column()
                .with_main_axis_size(MainAxisSize::Min)
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .with_spacing(spacing * 1.5);
            for child in rendered {
                column = column.push(child);
            }
            let mut list = ListView::new();
            list = list.push(column);
            Box::new(
                Container::new()
                    .with_padding(rustflutter::render::EdgeInsets::all(spacing * 1.5))
                    .with_child(list),
            )
        })
    }
}

/// The slider needs the theme's width, which only its own build knows.
struct SliderRow {
    value: f32,
    handle: StateHandle<ShowcaseState>,
}

impl Component for SliderRow {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        component(
            Slider::new(ID_SLIDER, self.value)
                .with_width(300.0)
                .wired(self.handle.clone(), |state, value| state.volume = value),
        )
    }
}

struct ProgressRow {
    value: f32,
}

impl Component for ProgressRow {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        component(ProgressBar::new(self.value).with_width(300.0))
    }
}

// -- Entry point --------------------------------------------------------------

struct ShowcaseApp {
    light: bool,
}

impl WidgetApplication for ShowcaseApp {
    fn background(&self) -> Color {
        if self.light { Theme::light().background } else { Theme::dark().background }
    }

    fn build(&mut self, _context: &BuildContext) -> AnyWidget {
        stateful(Showcase { start_light: self.light })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(argc: c_int, argv: *const *const c_char) -> c_int {
    let args = collect_args(argc, argv);
    let light = args.iter().any(|a| a == "--light");

    if let Some(path) = named_string(&args, "--png") {
        return render_png(&path, light);
    }

    register_application(move || Box::new(WidgetHost::new(ShowcaseApp { light })));
    let options = RunOptions {
        width: WIDTH,
        height: HEIGHT,
        title: String::from("Components - rustflutter"),
        ..RunOptions::default()
    };
    match run(&options) {
        Ok(()) => 0,
        Err(code) => {
            eprintln!("showcase: the host exited with {code}");
            1
        }
    }
}

fn render_png(path: &str, light: bool) -> c_int {
    rustflutter::engine::initialize();

    let mut tree = ElementTree::new();
    tree.rebuild(stateful(Showcase { start_light: light }));
    // The first pass builds against default state; a second lets the
    // command-line theme settle in.
    tree.rebuild_dirty();

    let mut root = tree.build_render_tree().expect("the tree has a root");
    root.layout(BoxConstraints::tight(WIDTH as f32, HEIGHT as f32));

    let background = if light { Theme::light().background } else { Theme::dark().background };
    let mut layer_tree = rustflutter::app::compose_frame(
        WIDTH,
        HEIGHT,
        1.0,
        Size::new(WIDTH as f32, HEIGHT as f32),
        background,
        |context| root.paint(context, Offset::ZERO),
    );

    match layer_tree.write_png(std::path::Path::new(path)) {
        Ok(()) => {
            println!("showcase: wrote {path} ({})", if light { "light" } else { "dark" });
            0
        }
        Err(err) => {
            eprintln!("showcase: render failed: {err}");
            1
        }
    }
}

fn named_string(args: &[String], flag: &str) -> Option<String> {
    let index = args.iter().position(|a| a == flag)?;
    args.get(index + 1).cloned()
}

fn collect_args(argc: c_int, argv: *const *const c_char) -> Vec<String> {
    if argv.is_null() || argc <= 1 {
        return Vec::new();
    }
    // SAFETY: the C++ shim passes main()'s argc/argv unchanged, so argv holds
    // at least `argc` NUL-terminated pointers.
    (1..argc as usize)
        .filter_map(|i| unsafe {
            let ptr = *argv.add(i);
            if ptr.is_null() {
                None
            } else {
                std::ffi::CStr::from_ptr(ptr).to_str().ok().map(str::to_string)
            }
        })
        .collect()
}

#[allow(dead_code)]
fn unused_surface() -> impl Widget {
    Align::new(Alignment::CENTER, Empty)
}
