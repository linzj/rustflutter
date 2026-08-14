// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Hello World for rustflutter.
//!
//! Built as a staticlib and linked into a small C++ shim, because the engine it
//! draws through is C++. `rustflutter_app_main` is the real entry point.

use std::os::raw::{c_char, c_int};

use rustflutter::prelude::*;

const WIDTH: i32 = 800;
const HEIGHT: i32 = 600;

const BACKGROUND: Color = Color::rgb(0x0E, 0x16, 0x26);
const ACCENT: Color = Color::rgb(0x54, 0xC5, 0xF8);
const CARD: Color = Color::rgb(0x1B, 0x2A, 0x3A);

/// The application. The shell instantiates one and calls `build` every frame.
struct HelloWorld;

impl Application for HelloWorld {
    fn background(&self) -> Color {
        BACKGROUND
    }

    fn build(&mut self, _context: &BuildContext) -> BoxedWidget {
        Box::new(Center::new(
            Container::new()
                .with_color(CARD)
                .with_corner_radius(16.0)
                .with_padding(EdgeInsets::symmetric(48.0, 36.0))
                .with_child(
                    Column::new()
                        .with_spacing(12.0)
                        .push(
                            Text::new("Hello, World!")
                                .with_size(52.0)
                                .with_weight(700)
                                .with_color(Color::WHITE)
                                .centered(),
                        )
                        .push(
                            Text::new("Rendered by Rust on the Flutter engine")
                                .with_size(20.0)
                                .with_color(ACCENT)
                                .centered(),
                        ),
                ),
        ))
    }
}

/// Entry point called by the C++ shim.
///
/// With no arguments this opens a window and hands control to the engine.
/// `--png <path>` renders one frame headlessly instead, without a shell, which
/// is what CI uses.
#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(argc: c_int, argv: *const *const c_char) -> c_int {
    let args = collect_args(argc, argv);

    if let Some(path) = png_output(&args) {
        return render_png(&path);
    }

    register_application(|| Box::new(HelloWorld));
    let options = RunOptions {
        width: WIDTH,
        height: HEIGHT,
        title: String::from("Hello, World! - rustflutter"),
        ..RunOptions::default()
    };
    match run(&options) {
        Ok(()) => 0,
        Err(code) => {
            eprintln!("rustflutter: the host exited with {code}");
            1
        }
    }
}

/// One frame, no shell, no window. Uses the direct build-and-flatten path.
fn render_png(path: &str) -> c_int {
    let app = App::new(WIDTH, HEIGHT).with_background(BACKGROUND);
    let mut root = HelloWorld;
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
            println!("rustflutter: wrote {path} ({WIDTH}x{HEIGHT})");
            0
        }
        Err(err) => {
            eprintln!("rustflutter: render failed: {err}");
            1
        }
    }
}

/// Returns the path for `--png <path>`, defaulting the path if it is omitted.
fn png_output(args: &[String]) -> Option<String> {
    let index = args.iter().position(|a| a == "--png")?;
    Some(
        args.get(index + 1)
            .cloned()
            .unwrap_or_else(|| "hello_world.png".to_string()),
    )
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
