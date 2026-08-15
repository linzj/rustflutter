// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! A counter, which is the smallest program that needs everything above the
//! render layer.
//!
//! Nothing here is possible with widgets alone. The count has to live somewhere
//! that survives the rebuild that displays it, the tap has to find the button
//! through geometry that was decided last frame, and pressing the button must
//! not rebuild the rest of the page. That is the element tree, hit testing and
//! `set_state` respectively.
//!
//! The loop, in full:
//!
//! ```text
//!   build      captures a StateHandle in the tap handler
//!   paint      leaves a laid-out render tree behind
//!   tap        hit test finds RenderPointerRegion, calls the handler
//!   set_state  mutates the state, marks the element dirty, asks for a frame
//!   vsync      rebuild_dirty rebuilds that element and nothing else
//! ```
//!
//! The header counts its own builds and shows the number. Tapping a button
//! leaves it unchanged, which is the visible proof that the rebuild was
//! partial.

use std::cell::Cell;
use std::os::raw::{c_char, c_int};

use rustflutter::gestures::PointerHandlers;
use rustflutter::prelude::*;
use rustflutter::render::{Alignment, CrossAxisAlignment, MainAxisAlignment, RenderFlex};
use rustflutter::widgets::{Align, Center, Column, Empty, Pointer, Row, SizedBox};

const WIDTH: i32 = 420;
const HEIGHT: i32 = 460;

const BACKGROUND: Color = Color::rgb(0x0B, 0x11, 0x1E);
const PANEL: Color = Color::rgb(0x16, 0x21, 0x33);
const PANEL_EDGE: Color = Color::rgb(0x24, 0x33, 0x4A);
const TEXT: Color = Color::rgb(0xE8, 0xEE, 0xF5);
const MUTED: Color = Color::rgb(0x7F, 0x93, 0xAD);
const ACCENT: Color = Color::rgb(0x54, 0xC5, 0xF8);
const DANGER: Color = Color::rgb(0xE0, 0x7A, 0x9B);

// Counts how many times the header has been built. A plain Cell is enough: the
// framework is single-threaded by construction, since every callback runs on
// the UI task runner.
thread_local! {
    static HEADER_BUILDS: Cell<u32> = const { Cell::new(0) };
}

// -- The page -----------------------------------------------------------------

struct CounterPage;

impl Component for CounterPage {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        many(
            vec![component(Header), stateful(Counter), component(Footer)],
            |children| {
                let mut flex = RenderFlex::column()
                    .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                    .with_main_axis_alignment(MainAxisAlignment::Start);
                for child in children {
                    flex = flex.push(child);
                }
                Box::new(flex)
            },
        )
    }
}

/// Stateless, and deliberately expensive to notice: it records every build.
struct Header;

impl Component for Header {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        let builds = HEADER_BUILDS.with(|b| {
            b.set(b.get() + 1);
            b.get()
        });
        leaf(move || {
            Container::new()
                .with_height(84.0)
                .with_color(PANEL)
                .with_padding(EdgeInsets::symmetric(20.0, 14.0))
                .with_child(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(4.0)
                        .push(
                            Text::new("Counter")
                                .with_size(22.0)
                                .with_weight(700)
                                .with_color(TEXT),
                        )
                        .push(
                            Text::new(format!("this header has been built {builds}x"))
                                .with_size(12.0)
                                .with_color(MUTED),
                        ),
                )
        })
    }
}

struct Footer;

impl Component for Footer {
    fn build(&self, _context: &mut rustflutter::framework::BuildContext) -> AnyWidget {
        leaf(|| {
            Container::new()
                .with_height(44.0)
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new("tap a button -- the header count stays put")
                        .with_size(12.0)
                        .with_color(MUTED),
                ))
        })
    }
}

// -- The stateful bit ---------------------------------------------------------

#[derive(Default)]
struct CounterState {
    count: i32,
    /// Which button is currently held, so it can be drawn pressed.
    pressed: Option<u64>,
}

struct Counter;

const BUTTON_DECREMENT: u64 = 1;
const BUTTON_INCREMENT: u64 = 2;
const BUTTON_RESET: u64 = 3;

impl StatefulComponent for Counter {
    type State = CounterState;

    fn build(
        &self,
        state: &CounterState,
        handle: StateHandle<CounterState>,
        _context: &mut rustflutter::framework::BuildContext,
    ) -> AnyWidget {
        let count = state.count;
        let pressed = state.pressed;

        // Each button gets its own clone of the handle. They are cheap -- an
        // index, a generation and a weak reference -- and a handle that
        // outlives its element refuses writes rather than corrupting one.
        let decrement = button(
            BUTTON_DECREMENT,
            "-",
            DANGER,
            pressed == Some(BUTTON_DECREMENT),
            handle.clone(),
            |s| s.count -= 1,
        );
        let increment = button(
            BUTTON_INCREMENT,
            "+",
            ACCENT,
            pressed == Some(BUTTON_INCREMENT),
            handle.clone(),
            |s| s.count += 1,
        );
        let reset = button(
            BUTTON_RESET,
            "reset",
            MUTED,
            pressed == Some(BUTTON_RESET),
            handle.clone(),
            |s| s.count = 0,
        );

        leaf(move || {
            Center::new(
                Column::new()
                    .with_spacing(26.0)
                    .push(
                        Text::new(format!("{count}"))
                            .with_size(76.0)
                            .with_weight(700)
                            .with_color(TEXT),
                    )
                    .push(
                        Row::new()
                            .with_spacing(14.0)
                            .push(decrement.render())
                            .push(increment.render())
                            .push(reset.render()),
                    ),
            )
        })
    }
}

/// A tappable button, built fresh each frame.
///
/// It is a value rather than a widget because the whole counter is one `leaf`:
/// the buttons have no state of their own, so there is nothing for an element
/// to remember, and building them directly is one layer less to read.
#[derive(Clone)]
struct Button {
    id: u64,
    label: &'static str,
    color: Color,
    pressed: bool,
    handlers: PointerHandlers,
}

impl Button {
    fn render(&self) -> impl Widget + 'static {
        let pressed = self.pressed;
        let color = self.color;
        let label = self.label;
        Pointer::new(
            self.id,
            Container::new()
                .with_height(52.0)
                .with_width(if label.len() > 1 { 108.0 } else { 64.0 })
                // The pressed look is just a different fill: state in, pixels
                // out, no animation machinery involved.
                .with_color(if pressed { color.with_alpha(0x44) } else { PANEL })
                .with_corner_radius(14.0)
                .with_border(1.5, if pressed { color } else { PANEL_EDGE })
                .with_child(Align::new(
                    Alignment::CENTER,
                    Text::new(label)
                        .with_size(if label.len() > 1 { 15.0 } else { 26.0 })
                        .with_weight(700)
                        .with_color(color),
                )),
        )
        .with_handlers(self.handlers.clone())
    }
}

fn button(
    id: u64,
    label: &'static str,
    color: Color,
    pressed: bool,
    handle: StateHandle<CounterState>,
    action: fn(&mut CounterState),
) -> Button {
    let tap_handle = handle.clone();
    let press_handle = handle;
    let handlers = PointerHandlers::new()
        .with_tap(move |_| {
            tap_handle.set_state(move |state| action(state));
        })
        .with_press_change(move |down| {
            press_handle.set_state(move |state| {
                state.pressed = if down { Some(id) } else { None };
            });
        });
    Button { id, label, color, pressed, handlers }
}

// -- Entry point --------------------------------------------------------------

struct CounterApp;

impl WidgetApplication for CounterApp {
    fn background(&self) -> Color {
        BACKGROUND
    }

    fn build(&mut self, _context: &BuildContext) -> AnyWidget {
        component(CounterPage)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(argc: c_int, argv: *const *const c_char) -> c_int {
    let args = collect_args(argc, argv);

    if let Some(path) = named_string(&args, "--png") {
        let taps = named_value(&args, "--taps").unwrap_or(0.0) as u32;
        return render_png(&path, taps);
    }

    register_application(|| Box::new(WidgetHost::new(CounterApp)));
    let options = RunOptions {
        width: WIDTH,
        height: HEIGHT,
        title: String::from("Counter - rustflutter"),
        ..RunOptions::default()
    };
    match run(&options) {
        Ok(()) => 0,
        Err(code) => {
            eprintln!("counter: the host exited with {code}");
            1
        }
    }
}

/// Renders one frame headlessly, optionally after simulating `taps` presses of
/// the increment button.
///
/// This is the milestone's check in a form CI can run: it drives the same
/// element tree, the same hit test and the same `set_state` as a real tap, and
/// then shows what the user would see.
fn render_png(path: &str, taps: u32) -> c_int {
    use rustflutter::framework::ElementTree;
    use rustflutter::gestures::{GestureRouter, PointerChange, PointerEvent, PointerKind};
    use rustflutter::render::{BoxConstraints, Offset, PaintContext, RenderBox};

    // Text is shaped during the very first layout below, and skparagraph
    // cannot break a line without ICU data.
    rustflutter::engine::initialize();

    let size = Size::new(WIDTH as f32, HEIGHT as f32);
    let mut tree = ElementTree::new();
    tree.rebuild(component(CounterPage));

    let mut router = GestureRouter::new();
    let mut root: Option<rustflutter::render::BoxedRender> = None;

    // One pass per tap, plus one to lay the first frame out and one to show the
    // final state.
    for tap in 0..=taps {
        let mut built = tree.build_render_tree().expect("the tree has a root");
        built.layout(BoxConstraints::tight(size.width, size.height));

        if tap < taps {
            // Find the increment button and press it where it actually is.
            let Some(target) = find_target(built.as_ref(), size, BUTTON_INCREMENT) else {
                eprintln!("counter: could not find the increment button");
                return 1;
            };
            for change in [PointerChange::Down, PointerChange::Up] {
                router.dispatch(
                    built.as_ref(),
                    &PointerEvent {
                        view_id: 0,
                        device: 0,
                        pointer_id: 1,
                        change,
                        kind: PointerKind::Mouse,
                        signal_kind: rustflutter::gestures::SignalKind::None,
                        buttons: 1,
                        time_stamp_micros: 0,
                        position: target,
                        delta: Offset::ZERO,
                        scroll_delta: Offset::ZERO,
                        pressure: 1.0,
                        local_position: target,
                    },
                );
            }
            tree.rebuild_dirty();
            // The press-change handler fires on down and again on up, so a
            // second pass settles the pressed state back to none.
            tree.rebuild_dirty();
        }
        root = Some(built);
    }

    let mut root = root.expect("at least one pass ran");
    root.layout(BoxConstraints::tight(size.width, size.height));

    // Paint through the same one-shot path the other examples use.
    let mut canvas = rustflutter::engine::Canvas::new(size.width, size.height);
    canvas.draw_color(BACKGROUND);
    {
        let mut context = PaintContext::new(&mut canvas);
        root.paint(&mut context, Offset::ZERO);
    }
    let display_list = canvas.build();
    let mut layer_tree = rustflutter::engine::LayerTree::new(WIDTH, HEIGHT);
    layer_tree.add_display_list(&display_list, 0.0, 0.0);

    match layer_tree.write_png(std::path::Path::new(path)) {
        Ok(()) => {
            println!("counter: wrote {path} after {taps} taps");
            0
        }
        Err(err) => {
            eprintln!("counter: render failed: {err}");
            1
        }
    }
}

/// Scans for the centre of the region with `id` by hit-testing a grid.
///
/// Crude, and deliberately so: it uses only the public hit-test API, so it
/// proves the same path a real pointer takes rather than reaching into the
/// tree's internals.
fn find_target(
    root: &dyn rustflutter::render::RenderBox,
    size: Size,
    id: u64,
) -> Option<rustflutter::render::Offset> {
    use rustflutter::render::{HitTestResult, Offset};
    let step = 4.0;
    let mut y = 0.0;
    while y < size.height {
        let mut x = 0.0;
        while x < size.width {
            let point = Offset::new(x, y);
            let mut result = HitTestResult::new();
            root.hit_test(point, &mut result);
            if result.innermost().map(|entry| entry.target) == Some(id) {
                return Some(point);
            }
            x += step;
        }
        y += step;
    }
    None
}

fn named_string(args: &[String], flag: &str) -> Option<String> {
    let index = args.iter().position(|a| a == flag)?;
    args.get(index + 1).cloned()
}

fn named_value(args: &[String], flag: &str) -> Option<f32> {
    named_string(args, flag)?.parse().ok()
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

// Keeps the helpers the example mentions but does not draw referenced, so the
// import list documents the whole surface.
#[allow(dead_code)]
fn unused_surface() -> impl Widget {
    SizedBox::new(1.0, 1.0).with_child(Empty)
}
