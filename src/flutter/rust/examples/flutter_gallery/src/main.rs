// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Flutter Gallery, ported to rustflutter.
//!
//! Upstream is `dev/integration_tests/new_gallery`: a home page of studies and
//! categories, a demo page per component, and a settings panel. This is the
//! same structure over the Rust framework, drawn by the Flutter engine through
//! Impeller.
//!
//! What is ported: the shape, the demo list with upstream's own titles and
//! subtitles, and one screen from each of three studies. What is not: the
//! localisations, the asset-backed carousel, the code viewer, and the demos
//! that need text input. Each absence is stated where it would otherwise look
//! like an oversight -- see `settings.rs` and the catalogue's descriptions.
//!
//! ```text
//!   --png <path>        render one frame headlessly
//!   --route <name>      which screen: home, demo, study, settings
//!   --slug <slug>       which demo or study, for --route demo|study
//!   --light             start in the light theme
//! ```

mod app;
mod catalog;
mod demos;
mod home;
mod settings;
mod studies;

use std::os::raw::{c_char, c_int};

use rustflutter::framework::{AnyWidget, ElementTree, stateful};
use rustflutter::prelude::*;
use rustflutter::render::{BoxConstraints, Offset, PaintContext, RenderBox};

use app::Gallery;

const WIDTH: i32 = 460;
const HEIGHT: i32 = 820;

/// The application the shell runs.
struct GalleryApp {
    light: bool,
    route: &'static str,
    slug: Option<String>,
}

impl WidgetApplication for GalleryApp {
    fn background(&self) -> Color {
        if self.light { Theme::light().background } else { Theme::dark().background }
    }

    fn build(&mut self, _context: &BuildContext) -> AnyWidget {
        stateful(Gallery {
            light: self.light,
            route: self.route,
            slug: self.slug.clone(),
        })
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_app_main(argc: c_int, argv: *const *const c_char) -> c_int {
    let args = collect_args(argc, argv);
    let light = args.iter().any(|a| a == "--light");
    let route = static_route(named(&args, "--route").as_deref().unwrap_or("home"));
    let slug = named(&args, "--slug");

    if let Some(path) = named(&args, "--png") {
        return render_png(&path, route, slug.as_deref(), light);
    }

    register_application(move || {
        Box::new(WidgetHost::new(GalleryApp {
            light,
            route,
            slug: slug.clone(),
        }))
    });
    let options = RunOptions {
        width: WIDTH,
        height: HEIGHT,
        title: String::from("rustflutter Gallery"),
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

/// Route names live for the program's lifetime, and `Gallery` holds one, so a
/// borrowed argument has to become one of these.
fn static_route(name: &str) -> &'static str {
    match name {
        "settings" => app::routes::SETTINGS,
        "demo" => app::routes::DEMO,
        "study" => app::routes::STUDY,
        _ => app::routes::HOME,
    }
}

/// Renders one screen headlessly.
///
/// The screen is chosen by building the tree with a `Gallery` that already
/// knows where it should be, rather than by simulating a tap: the point is to
/// show a screen, and finding a row to tap would make the check depend on the
/// home page's layout as well as on the screen's.
fn render_png(path: &str, route: &'static str, slug: Option<&str>, light: bool) -> c_int {
    rustflutter::engine::initialize();

    let mut tree = ElementTree::new();
    // A fixed clock, so the animated demos render the same frame every time.
    // A wall clock would make the motion demo's screenshot differ per run.
    tree.set_frame_time(app::SPINNER_PERIOD_MICROS / 4);
    tree.rebuild(stateful(Gallery {
        light,
        route,
        slug: slug.map(str::to_string),
    }));

    let mut root = tree.build_render_tree().expect("the tree has a root");
    root.layout(BoxConstraints::tight(WIDTH as f32, HEIGHT as f32));

    let background = if light { Theme::light().background } else { Theme::dark().background };
    let mut canvas = rustflutter::engine::Canvas::new(WIDTH as f32, HEIGHT as f32);
    canvas.draw_color(background);
    {
        let mut context = PaintContext::new(&mut canvas);
        root.paint(&mut context, Offset::ZERO);
    }
    let display_list = canvas.build();
    let mut layer_tree = rustflutter::engine::LayerTree::new(WIDTH, HEIGHT);
    layer_tree.add_display_list(&display_list, 0.0, 0.0);

    match layer_tree.write_png(std::path::Path::new(path)) {
        Ok(()) => {
            let what = match slug {
                Some(slug) => format!("{route} {slug}"),
                None => route.to_string(),
            };
            println!("gallery: wrote {path} ({what})");
            0
        }
        Err(err) => {
            eprintln!("gallery: render failed: {err}");
            1
        }
    }
}

fn named(args: &[String], flag: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cycle_runs_from_zero_to_one() {
        assert_eq!(app::cycle(0, 1000), 0.0);
        assert!((app::cycle(500, 1000) - 0.5).abs() < 1e-5);
        // Wraps rather than running past the end.
        assert_eq!(app::cycle(1000, 1000), 0.0);
        assert!((app::cycle(2500, 1000) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_ping_pong_turns_around_in_the_middle() {
        assert_eq!(app::ping_pong(0, 1000), 0.0);
        assert!((app::ping_pong(500, 1000) - 1.0).abs() < 1e-5);
        assert!((app::ping_pong(750, 1000) - 0.5).abs() < 1e-5);
        assert!(app::ping_pong(999, 1000) < 0.01);
    }

    #[test]
    fn an_unknown_route_falls_back_to_home() {
        assert_eq!(static_route("nonsense"), app::routes::HOME);
        assert_eq!(static_route("settings"), app::routes::SETTINGS);
    }
}
