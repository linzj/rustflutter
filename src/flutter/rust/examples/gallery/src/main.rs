// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! A gallery for the rendering layer.
//!
//! Hello World proves the pipeline; this proves the layout. Every widget on
//! screen is doing something a static picture could not fake:
//!
//!   * the header is a gradient `Container` with a `Row` that uses `Expanded`
//!     to push its trailing text to the far edge,
//!   * the list is a `RenderViewport` over a `Column` taller than the window,
//!     clipped and scrolled by a real offset,
//!   * each card is a `Row` with a circular avatar, a shrink-wrapped `Column`
//!     of two text runs, and a badge aligned on the cross axis,
//!   * the scroll indicator is a `Stack` child anchored to the right edge, and
//!     its height is a fraction of the scrollable extent,
//!   * the footer sits over the list through `Positioned`, at partial opacity.
//!
//! `--scroll <pixels>` renders the list scrolled, which is how the headless
//! check shows that scrolling moves content and clips it rather than just
//! resizing something.

use std::os::raw::{c_char, c_int};

use rustflutter::prelude::*;
use rustflutter::render::{
    Alignment, Axis, CrossAxisAlignment, MainAxisAlignment, MainAxisSize, RenderFlex,
    StackPosition,
};
use rustflutter::widgets::{
    Align, ClipRRect, Column, Empty, Expanded, ListView, Opacity, Padding, Pointer, Row, SizedBox,
    Stack,
};

const WIDTH: i32 = 480;
const HEIGHT: i32 = 720;

const BACKGROUND: Color = Color::rgb(0x0B, 0x11, 0x1E);
const CARD: Color = Color::rgb(0x16, 0x21, 0x33);
const CARD_EDGE: Color = Color::rgb(0x24, 0x33, 0x4A);
const HEADER_FROM: Color = Color::rgb(0x1E, 0x3A, 0x5F);
const HEADER_TO: Color = Color::rgb(0x0E, 0x7C, 0x86);
const TEXT: Color = Color::rgb(0xE8, 0xEE, 0xF5);
const MUTED: Color = Color::rgb(0x7F, 0x93, 0xAD);
const ACCENT: Color = Color::rgb(0x54, 0xC5, 0xF8);

/// One row of the list. `hue` only exists so the avatars differ.
struct Entry {
    title: &'static str,
    subtitle: &'static str,
    badge: &'static str,
    hue: Color,
}

const ENTRIES: &[Entry] = &[
    Entry {
        title: "Constraints go down",
        subtitle: "A parent hands each child a BoxConstraints",
        badge: "layout",
        hue: Color::rgb(0x54, 0xC5, 0xF8),
    },
    Entry {
        title: "Sizes come up",
        subtitle: "The child picks a size inside them and reports it",
        badge: "layout",
        hue: Color::rgb(0x7B, 0xD3, 0x89),
    },
    Entry {
        title: "The parent positions the child",
        subtitle: "Nothing reads its parent, so a subtree lays out alone",
        badge: "layout",
        hue: Color::rgb(0xF2, 0xB1, 0x4F),
    },
    Entry {
        title: "Flex divides what is left",
        subtitle: "Inflexible children first, then the remainder by factor",
        badge: "flex",
        hue: Color::rgb(0xE0, 0x7A, 0x9B),
    },
    Entry {
        title: "A viewport is a window",
        subtitle: "Child laid out unbounded, shown at an offset, clipped",
        badge: "scroll",
        hue: Color::rgb(0x9B, 0x8C, 0xF0),
    },
    Entry {
        title: "Stacks overlay",
        subtitle: "Anchor a child to an edge, or stretch it across two",
        badge: "stack",
        hue: Color::rgb(0x4F, 0xC8, 0xB0),
    },
    Entry {
        title: "Intrinsics ask ahead",
        subtitle: "What a box would like, before it knows what it gets",
        badge: "layout",
        hue: Color::rgb(0xF0, 0x8A, 0x6E),
    },
    Entry {
        title: "Hit tests walk back",
        subtitle: "Front to back, innermost target first",
        badge: "input",
        hue: Color::rgb(0x63, 0xB3, 0xED),
    },
];

struct Gallery {
    scroll: f32,
}

impl Application for Gallery {
    fn background(&self) -> Color {
        BACKGROUND
    }

    fn build(&mut self, context: &BuildContext) -> BoxedWidget {
        // No element tree here, so no `SafeArea` either -- this application is
        // render objects all the way down. The padding is the same padding;
        // only the way to reach it differs.
        let padding = context.metrics.padding();
        let scrollable_height = context.size.height - header_height() - padding.vertical();
        Box::new(rustflutter::render::RenderPadding::new(
            padding,
            Column::expanded()
                .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
                .push(header())
                .push_flex(Expanded::new(self.body(scrollable_height))),
        ))
    }
}

impl Gallery {
    /// The list, the scroll indicator beside it, and the footer over it.
    fn body(&self, height: f32) -> impl Widget + 'static {
        let content_height = ENTRIES.len() as f32 * (card_height() + card_spacing());
        let max_scroll = (content_height - height).max(0.0);
        let progress = if max_scroll > 0.0 {
            (self.scroll / max_scroll).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let mut list = ListView::new().with_spacing(card_spacing()).with_offset(self.scroll);
        for (index, entry) in ENTRIES.iter().enumerate() {
            list = list.push(Pointer::new(index as u64 + 1, card(entry)));
        }

        Stack::new()
            .push(Padding::new(EdgeInsets::symmetric(16.0, 16.0), list))
            .push_positioned(scroll_indicator(height, progress), scroll_indicator_position())
            .push_positioned(footer(), StackPosition {
                left: Some(0.0),
                right: Some(0.0),
                bottom: Some(0.0),
                ..Default::default()
            })
    }
}

fn header_height() -> f32 {
    96.0
}

fn card_height() -> f32 {
    88.0
}

fn card_spacing() -> f32 {
    12.0
}

fn header() -> impl Widget + 'static {
    Container::new()
        .with_height(header_height())
        .with_gradient(
            Alignment::CENTER_LEFT,
            Alignment::CENTER_RIGHT,
            Gradient::new(&[HEADER_FROM, HEADER_TO]),
        )
        .with_padding(EdgeInsets::symmetric(20.0, 0.0))
        .with_child(
            Row::expanded()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .push(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_spacing(4.0)
                        .push(
                            Text::new("Rendering layer")
                                .with_size(24.0)
                                .with_weight(700)
                                .with_color(TEXT),
                        )
                        .push(
                            Text::new("flex, stacks, viewports")
                                .with_size(13.0)
                                .with_color(Color::rgb(0xBF, 0xDD, 0xE8)),
                        ),
                )
                // Expanded with nothing in it is the idiomatic spacer: it takes
                // the free space so the next child lands at the far edge.
                .push_flex(Expanded::new(Empty))
                .push(
                    Container::new()
                        .with_color(Color::argb(0x33, 0xFF, 0xFF, 0xFF))
                        .with_corner_radius(12.0)
                        .with_padding(EdgeInsets::symmetric(10.0, 6.0))
                        .with_child(
                            Text::new(format!("{} items", ENTRIES.len()))
                                .with_size(12.0)
                                .with_weight(700)
                                .with_color(TEXT),
                        ),
                ),
        )
}

fn card(entry: &Entry) -> impl Widget + 'static {
    Container::new()
        .with_height(card_height())
        .with_color(CARD)
        .with_corner_radius(14.0)
        .with_border(1.0, CARD_EDGE)
        .with_padding(EdgeInsets::symmetric(14.0, 12.0))
        .with_child(
            Row::expanded()
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_spacing(14.0)
                .push(avatar(entry.hue))
                .push_flex(Expanded::new(
                    Column::new()
                        .with_cross_axis_alignment(CrossAxisAlignment::Start)
                        .with_main_axis_alignment(MainAxisAlignment::Center)
                        .with_spacing(5.0)
                        .push(
                            Text::new(entry.title)
                                .with_size(16.0)
                                .with_weight(700)
                                .with_color(TEXT),
                        )
                        .push(
                            Text::new(entry.subtitle)
                                .with_size(12.5)
                                .with_color(MUTED),
                        ),
                ))
                .push(badge(entry.badge, entry.hue)),
        )
}

/// A circle, made by clipping a square to half its side.
fn avatar(hue: Color) -> impl Widget + 'static {
    ClipRRect::new(
        22.0,
        Container::new().with_size(44.0, 44.0).with_gradient(
            Alignment::TOP_LEFT,
            Alignment::BOTTOM_RIGHT,
            Gradient::new(&[hue, Color::argb(0x88, hue.red(), hue.green(), hue.blue())]),
        ),
    )
}

fn badge(label: &str, hue: Color) -> impl Widget + 'static {
    Container::new()
        .with_color(Color::argb(0x22, hue.red(), hue.green(), hue.blue()))
        .with_corner_radius(9.0)
        .with_padding(EdgeInsets::symmetric(9.0, 5.0))
        .with_child(
            Text::new(label.to_string())
                .with_size(11.0)
                .with_weight(700)
                .with_color(hue),
        )
}

/// A track with a thumb whose position follows `progress`.
fn scroll_indicator(height: f32, progress: f32) -> impl Widget + 'static {
    let track = (height - 32.0).max(0.0);
    let thumb = (track * 0.28).max(24.0);
    let travel = (track - thumb).max(0.0);
    Container::new().with_size(4.0, track).with_child(
        Stack::new().push_positioned(
            Container::new()
                .with_size(4.0, thumb)
                .with_color(ACCENT)
                .with_corner_radius(2.0),
            StackPosition {
                top: Some(travel * progress),
                left: Some(0.0),
                ..Default::default()
            },
        ),
    )
}

fn scroll_indicator_position() -> StackPosition {
    StackPosition { right: Some(6.0), top: Some(16.0), ..Default::default() }
}

/// Sits over the list, so the content scrolls behind it.
fn footer() -> impl Widget + 'static {
    Opacity::new(
        0.92,
        Container::new()
            .with_height(44.0)
            .with_color(Color::argb(0xEE, 0x0B, 0x11, 0x1E))
            .with_child(Align::new(
                Alignment::CENTER,
                Text::new("Positioned over the viewport, at 92% opacity")
                    .with_size(12.0)
                    .with_color(MUTED),
            )),
    )
}

/// Entry point, called by the C++ shim in main.cc.
#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(argc: c_int, argv: *const *const c_char) -> c_int {
    let args = collect_args(argc, argv);
    let scroll = named_value(&args, "--scroll").unwrap_or(0.0);

    if let Some(path) = named_string(&args, "--png") {
        return render_png(&path, scroll);
    }

    register_application(move || Box::new(Gallery { scroll }));
    let options = RunOptions {
        width: WIDTH,
        height: HEIGHT,
        title: String::from("Rendering layer - rustflutter"),
        ..RunOptions::default()
    };
    match run(&options) {
        Ok(()) => 0,
        Err(code) => {
            eprintln!("gallery: the host exited with {code}");
            1
        }
    }
}

fn render_png(path: &str, scroll: f32) -> c_int {
    let app = App::new(WIDTH, HEIGHT).with_background(BACKGROUND);
    let mut root = Gallery { scroll };
    let context = BuildContext {
        view_id: 0,
        size: Size::new(WIDTH as f32, HEIGHT as f32),
        metrics: Default::default(),
        frame_number: 1,
        frame_time_micros: 0,
        scheduler: Default::default(),
    };
    match app.render_to_png(root.build(&context), path) {
        Ok(()) => {
            println!("gallery: wrote {path} ({WIDTH}x{HEIGHT}, scroll {scroll})");
            0
        }
        Err(err) => {
            eprintln!("gallery: render failed: {err}");
            1
        }
    }
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

/// Unused, but keeps the flex helpers referenced so the example documents the
/// whole surface rather than only what it happens to draw.
#[allow(dead_code)]
fn unused_surface() -> RenderFlex {
    RenderFlex::new(Axis::Horizontal)
        .with_main_axis_size(MainAxisSize::Min)
        .with_main_axis_alignment(MainAxisAlignment::SpaceEvenly)
        .push(SizedBox::new(1.0, 1.0))
}
