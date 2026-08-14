// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! rustflutter -- a Rust UI framework on the Flutter engine.
//!
//! The engine's rendering, compositing, text-layout and threading stack is used
//! unmodified; the Dart VM, `dart:ui` and `packages/flutter` are replaced by
//! this crate. See `PORTING_STATUS.md` at the repository root.
//!
//! ```no_run
//! use rustflutter::prelude::*;
//!
//! let ui = Center::new(Text::new("Hello, World!").with_size(48.0));
//! App::new(800, 600).with_background(Color::WHITE).render_to_png(ui, "hello.png").unwrap();
//! ```

#![deny(unsafe_op_in_unsafe_fn)]

pub mod app;
pub mod engine;
pub mod painting;
pub mod widgets;

pub use app::{
    Application, ApplicationFactory, BuildContext, FrameContext, FrameScheduler, RunOptions,
    ViewMetrics, register_application,
};
#[cfg(not(test))]
pub use app::run;
pub use engine::{
    Canvas, Color, DisplayList, LayerTree, Paint, Paragraph, Rect, RenderError, Style, TextAlign,
    TextStyle,
};
pub use painting::{
    BlendMode, ClipBehavior, ClipOp, FillType, Gradient, Image, RenderPath, StrokeCap, StrokeJoin,
    TileMode,
};
pub use widgets::{
    BoxedWidget, Center, Column, Constraints, Container, EdgeInsets, Offset, Size, Text, Widget,
};

/// Everything a typical app needs in one import.
pub mod prelude {
    pub use crate::app::{
        Application, BuildContext, FrameContext, FrameScheduler, RunOptions, register_application,
    };
    #[cfg(not(test))]
    pub use crate::app::run;
    pub use crate::engine::{Color, Paint, Rect, Style, TextAlign, TextStyle};
    pub use crate::painting::{
        BlendMode, ClipBehavior, ClipOp, FillType, Gradient, Image, RenderPath, StrokeCap,
        StrokeJoin, TileMode,
    };
    pub use crate::widgets::{
        BoxedWidget, Center, Column, Constraints, Container, EdgeInsets, Offset, Size, Text, Widget,
    };
    pub use crate::App;
}

/// Drives one frame: lays the widget tree out against the window size, paints
/// it into a `DisplayList`, wraps that in a `LayerTree` and hands it to the
/// engine.
///
/// Upstream this is what `RenderView` + `SchedulerBinding.drawFrame` do before
/// calling `PlatformDispatcher.render()`; the handoff object is the same
/// `flow::LayerTree`.
pub struct App {
    width: i32,
    height: i32,
    background: Color,
}

impl App {
    pub fn new(width: i32, height: i32) -> App {
        // The text stack needs ICU data before the first Paragraph is built.
        // Idempotent, so doing it here keeps app entry points boilerplate-free.
        engine::initialize();
        App {
            width,
            height,
            background: Color::WHITE,
        }
    }

    pub fn with_background(mut self, color: Color) -> App {
        self.background = color;
        self
    }

    pub fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }

    /// Runs layout + paint and returns the layer tree the engine rasterizes.
    pub fn build_frame(&self, mut root: impl Widget) -> LayerTree {
        let width = self.width as f32;
        let height = self.height as f32;

        root.layout(Constraints::tight(width, height));

        let mut canvas = Canvas::new(width, height);
        canvas.draw_color(self.background);
        root.paint(&mut canvas, Offset::ZERO);
        let display_list = canvas.build();

        let mut tree = LayerTree::new(self.width, self.height);
        tree.add_display_list(&display_list, 0.0, 0.0);
        tree
    }

    /// Renders one frame to a PNG. Headless -- no window or GPU context.
    pub fn render_to_png(
        &self,
        root: impl Widget,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), RenderError> {
        self.build_frame(root).write_png(path.as_ref())
    }

    /// Renders one frame into a BGRA8888 buffer, for blitting into a window.
    pub fn render_to_bgra(&self, root: impl Widget) -> Result<Vec<u8>, RenderError> {
        self.build_frame(root).rasterize_bgra()
    }

    /// Renders one frame and shows it in a window, blocking until it closes.
    pub fn show(&self, root: impl Widget, title: &str) -> Result<(), RenderError> {
        self.build_frame(root).show(title)
    }
}
