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

// Compiled for this crate's own `#[test]` binary, and for anything built with
// `--cfg rustflutter_stubs` -- which is how a crate that depends on this one
// gets its tests linked without the whole C++ engine behind them.
#[cfg(any(test, rustflutter_stubs))]
mod engine_test_stubs;

pub mod animation;
pub mod app;
pub mod components;
pub mod controls;
pub mod editable;
pub mod engine;
pub mod focus;
pub mod framework;
pub mod gestures;
pub mod implicit;
pub mod ink;
pub mod keyboard;
pub mod media_query;
pub mod navigation;
pub mod painting;
pub mod physics;
pub mod platform;
pub mod render;
pub mod scrollbar;
pub mod scrolling;
pub mod services;
pub mod widgets;

pub use app::{
    Application, ApplicationFactory, BuildContext, FrameContext, FrameScheduler, RunOptions,
    ViewMetrics, WidgetApplication, WidgetHost, register_application,
};
pub use animation::{
    Animations, ColorTween, Controller, Curve, Direction, FloatTween, OffsetTween, Repeat, Tween,
};
pub use controls::{
    Banner, BottomNavigation, BottomSheet, Checkbox, Chip, ChipStyle, DataTable, Destination,
    Dialog, GridList, NavigationRail, Radio, Scrim, Section, Snackbar, Spinner, TabBar, Tooltip,
};
pub use navigation::{
    Motion, Navigator, Presentation, Route, RouteArgs, Transition, TransitionOffsets,
};
pub use components::{
    AppBar, Badge, Button, ButtonGroupState, ButtonStyle, Card, Divider, IdSource, Label,
    LabelStyle, ListTile, ProgressBar, Scaffold, Slider, Switch, Theme, theme_of,
};
pub use editable::{RenderEditable, TextField, TextFieldState};
pub use focus::{Focus, KeyResult, focusable};
pub use implicit::{Animated, Lerp, animated};
pub use ink::{Ink, ink};
pub use keyboard::{KeyChange, KeyEvent, Keyboard, LogicalKey, PhysicalKey};
pub use media_query::{
    MediaQuery, MediaQueryData, SafeArea, current_text_scale, media_query_of, safe_area,
};
pub use services::{
    BasicMessageChannel, EventChannel, EventSink, JsonMessageCodec, JsonMethodCodec, MethodCall,
    MethodChannel, MethodError, MethodResult, StandardMessageCodec, StandardMethodCodec, Value,
};
pub use framework::{
    AnyWidget, Component, ElementTree, Key, RenderWidget, StateHandle, StatefulComponent,
    component, leaf, keyed_leaf, keyed_many, keyed_single, many, provide, single, stateful,
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
pub use scrollbar::{Scrollbar, scrollbar};
pub use scrolling::{ItemWindow, LazyList, Scroll, item_window};
pub use widgets::{
    BoxedWidget, Center, Column, Constraints, Container, EdgeInsets, Offset, Size, Text, TextSpan,
    Widget,
};

/// Everything a typical app needs in one import.
pub mod prelude {
    pub use crate::app::{
        Application, BuildContext, FrameContext, FrameScheduler, RunOptions, WidgetApplication,
        WidgetHost, register_application,
    };
    pub use crate::animation::{Animations, ColorTween, Controller, Curve, FloatTween, Tween};
    pub use crate::controls::{
        Banner, BottomNavigation, BottomSheet, Checkbox, Chip, ChipStyle, DataTable, Destination,
        Dialog, GridList, NavigationRail, Radio, Scrim, Section, Snackbar, Spinner, TabBar,
        Tooltip,
    };
    pub use crate::navigation::{Navigator, Route, RouteArgs, Transition};
    pub use crate::components::{
        AppBar, Badge, Button, ButtonGroupState, ButtonStyle, Card, Divider, IdSource, Label,
        ListTile, ProgressBar, Scaffold, Slider, Switch, Theme, gap, stack_column, stack_row,
        theme_of,
    };
    pub use crate::editable::{TextField, TextFieldState};
    pub use crate::focus::{Focus, KeyResult, focusable};
    pub use crate::implicit::{Animated, animated};
    pub use crate::ink::{Ink, ink};
    pub use crate::keyboard::{KeyChange, KeyEvent, Keyboard, LogicalKey, PhysicalKey};
    pub use crate::media_query::{
        MediaQuery, MediaQueryData, SafeArea, current_text_scale, media_query_of, safe_area,
    };
    pub use crate::services::system::{
        AppExitResponse, AppExitType, AppLifecycleState, Clipboard, HapticFeedback, SystemChrome,
        SystemMouseCursor, SystemNavigator, SystemSound, SystemSoundType,
    };
    pub use crate::services::{MethodCall, MethodChannel, MethodError, Value};
    pub use crate::platform::{Brightness, Locale, UserSettings};
    pub use crate::scrollbar::{Scrollbar, scrollbar};
    pub use crate::scrolling::{LazyList, Scroll};
    pub use crate::framework::{
        AnyWidget, Component, StateHandle, StatefulComponent, component, keyed_leaf, keyed_many,
        keyed_single, leaf, many, provide, single, stateful,
    };
    #[cfg(not(test))]
    pub use crate::app::run;
    pub use crate::engine::{Color, Paint, Rect, Style, TextAlign, TextStyle};
    pub use crate::painting::{
        BlendMode, ClipBehavior, ClipOp, FillType, Gradient, Image, RenderPath, StrokeCap,
        StrokeJoin, TileMode,
    };
    pub use crate::widgets::{
        BoxedWidget, Center, Column, Constraints, Container, EdgeInsets, Offset, Size, Text,
        TextSpan, Widget,
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

        crate::app::compose_frame(
            self.width,
            self.height,
            1.0,
            render::Size::new(width, height),
            self.background,
            |context| root.paint(context, Offset::ZERO),
        )
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
