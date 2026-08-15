// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! The framework side of the shell contract.
//!
//! `RuntimeController` (C++, `//flutter/runtime`) owns one `AppInstance` per
//! shell and calls the `rf_app_*` functions below on the UI task runner. Each
//! frame ends with one [`RfAppHost::render`] call per view, handing the shell a
//! layer tree -- the same object `RuntimeDelegate::Render` took from Dart.
//!
//! Upstream the corresponding code is `PlatformDispatcher` plus
//! `RendererBinding`: the former receives the platform's calls, the latter
//! turns a frame request into layout, paint and `window.render()`.

// The C ABI module at the bottom is the only user of most of this file, and it
// is compiled out under cfg(test) -- see the comment there. Without this, the
// test build reports every type it touches as dead.
#![cfg_attr(test, allow(dead_code, unused_imports))]

use std::collections::HashMap;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::sync::OnceLock;

use crate::engine::{self, Canvas, Color, LayerTree};
use crate::framework::{AnyWidget, ElementTree};
use crate::gestures::{GestureRouter, PointerChange, PointerEvent, PointerKind};
use crate::render::{BoxConstraints, PaintContext, RenderBox};
use crate::widgets::{BoxedWidget, Offset, Size};

// -- The platform's view of a view --------------------------------------------

/// Mirrors `flutter::ViewportMetrics`, in physical pixels.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct ViewMetrics {
    pub device_pixel_ratio: f64,
    pub width: f64,
    pub height: f64,
    pub padding_top: f64,
    pub padding_right: f64,
    pub padding_bottom: f64,
    pub padding_left: f64,
    pub view_inset_top: f64,
    pub view_inset_right: f64,
    pub view_inset_bottom: f64,
    pub view_inset_left: f64,
}

impl ViewMetrics {
    /// The view's size in logical pixels -- what widgets lay out against.
    pub fn logical_size(&self) -> Size {
        let dpr = if self.device_pixel_ratio > 0.0 {
            self.device_pixel_ratio
        } else {
            1.0
        };
        Size::new((self.width / dpr) as f32, (self.height / dpr) as f32)
    }

    /// Physical pixel size, which is the layer tree's size.
    pub fn physical_size(&self) -> (i32, i32) {
        (self.width.round() as i32, self.height.round() as i32)
    }
}

// -- What the shell gives us --------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct RfAppHost {
    user_data: *mut c_void,
    render: Option<
        unsafe extern "C" fn(*mut c_void, i64, *mut engine::sys::RfLayerTree, f64),
    >,
    schedule_frame: Option<unsafe extern "C" fn(*mut c_void)>,
}

// -- What an app implements ---------------------------------------------------

/// Lets an application ask for another frame.
///
/// Equivalent to dart:ui's `PlatformDispatcher.scheduleFrame`. Frames are on
/// demand, not free-running: without a request the engine goes idle after the
/// last one, which is why a static UI costs nothing to keep on screen.
#[derive(Clone, Copy)]
pub struct FrameScheduler {
    host: RfAppHost,
}

impl Default for FrameScheduler {
    /// A scheduler with nowhere to send the request. Requesting a frame on it
    /// does nothing, which is what the headless single-frame path wants.
    fn default() -> FrameScheduler {
        FrameScheduler {
            host: RfAppHost {
                user_data: std::ptr::null_mut(),
                render: None,
                schedule_frame: None,
            },
        }
    }
}

impl FrameScheduler {
    /// Requests one more frame. Repeated calls within a frame coalesce.
    pub fn request_frame(&self) {
        if let Some(schedule) = self.host.schedule_frame {
            unsafe { schedule(self.host.user_data) };
        }
    }
}

/// Context handed to [`Application::begin_frame`].
pub struct FrameContext {
    /// Frame number, monotonically increasing from 1.
    pub frame_number: u64,
    /// Time the frame is targeted at, in microseconds since epoch.
    pub frame_time_micros: i64,
    /// Ask for the frame after this one -- how an animation keeps going.
    pub scheduler: FrameScheduler,
}

/// Context handed to [`Application::build`] each frame.
pub struct BuildContext {
    /// Which view is being built. Single-window apps only ever see the
    /// implicit view, id 0.
    pub view_id: i64,
    /// The view's geometry, in logical pixels.
    pub size: Size,
    /// The raw platform metrics, if the app needs padding or insets.
    pub metrics: ViewMetrics,
    /// Frame number, monotonically increasing from 1.
    pub frame_number: u64,
    /// Time the frame is targeted at, in microseconds since epoch.
    pub frame_time_micros: i64,
    /// Ask for another frame, e.g. because this build started an animation.
    pub scheduler: FrameScheduler,
}

/// The root of a rustflutter application.
///
/// Hand one to [`register_application`] before starting the shell; it is
/// instantiated once and `build` is called every frame.
pub trait Application {
    /// Builds the widget tree for one view.
    fn build(&mut self, context: &BuildContext) -> BoxedWidget;

    /// Colour painted before the widget tree. Defaults to white.
    fn background(&self) -> Color {
        Color::WHITE
    }

    /// Advances animations. Called before `build`, matching dart:ui's
    /// `onBeginFrame` running ahead of `onDrawFrame` -- an animation that
    /// starts here is visible to the build that follows it in the same frame.
    fn begin_frame(&mut self, _context: &FrameContext) {}
}

// -- Widget-based applications ------------------------------------------------

/// An application described in widgets rather than render objects.
///
/// The difference that matters is what happens between frames: a widget
/// application gets an [`ElementTree`], so state survives and a `set_state`
/// rebuilds only its own subtree. An [`Application`] rebuilds its render
/// objects from scratch every frame and has nowhere to keep anything.
pub trait WidgetApplication {
    /// Builds the root widget. Called once when the tree is first mounted, and
    /// again whenever the view's size changes -- see [`WidgetHost`] for why.
    fn build(&mut self, context: &BuildContext) -> AnyWidget;

    fn background(&self) -> Color {
        Color::WHITE
    }

    fn begin_frame(&mut self, _context: &FrameContext) {}
}

/// Runs a [`WidgetApplication`] as an [`Application`].
///
/// Frames come in two shapes. The first frame, and any frame after the view
/// resizes, rebuilds the root widget and reconciles the whole tree. Every other
/// frame only rebuilds the elements that `set_state` marked dirty.
///
/// The resize case exists because the root `build` is handed the view size, so
/// a description that depends on it has to be asked again. Upstream this is
/// what `MediaQuery` and its `InheritedWidget` dependency tracking do for a
/// living; until that arrives, a resize is a full rebuild.
///
/// One thing this does *not* yet skip is layout. Element reuse preserves state
/// and avoids re-running `build`; the render tree is still assembled fresh each
/// frame, so layout and paint run in full. Making render objects persistent
/// across frames is the next thing worth doing here.
pub struct WidgetHost<W: WidgetApplication> {
    app: W,
    tree: ElementTree,
    last_size: Option<Size>,
}

impl<W: WidgetApplication> WidgetHost<W> {
    pub fn new(app: W) -> WidgetHost<W> {
        WidgetHost { app, tree: ElementTree::new(), last_size: None }
    }

    /// The element tree, for tests and diagnostics.
    pub fn tree(&self) -> &ElementTree {
        &self.tree
    }
}

impl<W: WidgetApplication> Application for WidgetHost<W> {
    fn background(&self) -> Color {
        self.app.background()
    }

    fn begin_frame(&mut self, context: &FrameContext) {
        // Advancing happens here rather than in build, and the difference is
        // measurable: Animator posts its vsync wait after an idle, so a request
        // made during the build reaches it a frame or more late. Asking at the
        // start of the frame gives it the whole frame to schedule the next one.
        let animating = self.tree.advance_frame(context.frame_time_micros);
        if animating {
            context.scheduler.request_frame();
        }
        self.app.begin_frame(context);
    }

    fn build(&mut self, context: &BuildContext) -> BoxedWidget {
        // The clock was already moved forward in begin_frame; this only makes
        // sure every build in the frame reads the same value.
        self.tree.set_frame_time(context.frame_time_micros);

        let resized = self.last_size != Some(context.size);
        if self.tree.is_empty() || resized {
            let root = self.app.build(context);
            self.tree.rebuild(root);
            self.last_size = Some(context.size);
        } else {
            self.tree.rebuild_dirty();
        }

        // A set_state that arrived during this build, or one that was queued
        // rather than applied, needs another frame to become visible.
        if self.tree.needs_frame() {
            self.tree.clear_needs_frame();
            context.scheduler.request_frame();
        }

        self.tree
            .build_render_tree()
            .unwrap_or_else(|| Box::new(crate::widgets::Empty))
    }
}

/// Builds the application's root object. Registered before the shell starts.
pub type ApplicationFactory = Box<dyn Fn() -> Box<dyn Application> + Send + Sync>;

static APPLICATION_FACTORY: OnceLock<ApplicationFactory> = OnceLock::new();

/// Tells the framework what to run.
///
/// An application calls this from `main` before starting the shell. The first
/// registration wins; later ones are ignored and return `false`.
///
/// Upstream the equivalent is the Dart isolate looking up `main` by name in the
/// app's kernel snapshot. Resolving it at run time rather than link time is
/// what lets the framework stay a plain library: a binary that links it but
/// never registers an application -- a test, a tool -- still links.
pub fn register_application<F>(factory: F) -> bool
where
    F: Fn() -> Box<dyn Application> + Send + Sync + 'static,
{
    APPLICATION_FACTORY.set(Box::new(factory)).is_ok()
}

// -- The instance the shell holds ---------------------------------------------

struct AppInstance {
    host: RfAppHost,
    application: Option<Box<dyn Application>>,
    views: HashMap<i64, ViewMetrics>,
    // Insertion order, so multi-view frames render deterministically. HashMap
    // iteration order is not stable across runs.
    view_order: Vec<i64>,
    frame_number: u64,
    frame_time_micros: i64,
    /// The render tree from each view's last painted frame, kept so a pointer
    /// that arrives between frames has something to hit-test against. Upstream
    /// the render tree is persistent and this is simply "the tree"; here it is
    /// rebuilt each frame, so the last one has to be held on to deliberately.
    painted: HashMap<i64, BoxedWidget>,
    router: GestureRouter,
}

impl AppInstance {
    fn schedule_frame(&self) {
        if let Some(schedule) = self.host.schedule_frame {
            unsafe { schedule(self.host.user_data) };
        }
    }

    fn draw_view(&mut self, view_id: i64) {
        let Some(metrics) = self.views.get(&view_id).copied() else {
            return;
        };
        let Some(application) = self.application.as_mut() else {
            return;
        };
        let (physical_width, physical_height) = metrics.physical_size();
        if physical_width <= 0 || physical_height <= 0 {
            return;
        }

        let context = BuildContext {
            view_id,
            size: metrics.logical_size(),
            metrics,
            frame_number: self.frame_number,
            frame_time_micros: self.frame_time_micros,
            scheduler: FrameScheduler { host: self.host },
        };
        let background = application.background();
        let mut root = application.build(&context);

        // Constraints down, sizes up -- the RenderBox protocol. The root is
        // forced to the view size, which is what RenderView does upstream.
        root.layout(BoxConstraints::tight(context.size.width, context.size.height));

        let mut canvas = Canvas::new(context.size.width, context.size.height);
        canvas.draw_color(background);
        {
            let mut paint_context = PaintContext::new(&mut canvas);
            root.paint(&mut paint_context, Offset::ZERO);
        }
        let display_list = canvas.build();

        let mut tree = LayerTree::new(physical_width, physical_height);
        tree.add_display_list(&display_list, 0.0, 0.0);

        if let Some(render) = self.host.render {
            // Ownership crosses here: the shell converts the handle into a
            // flow::LayerTree and frees it.
            let raw = tree.into_raw();
            unsafe { render(self.host.user_data, view_id, raw, metrics.device_pixel_ratio) };
        }

        // Keep the laid-out tree for hit testing until the next frame replaces
        // it. `root` has been through layout and paint, so its geometry is the
        // geometry the user is looking at.
        self.painted.insert(view_id, root);
    }

    /// Routes one event to the tree that was painted for its view.
    fn dispatch_pointer(&mut self, event: &PointerEvent) {
        let Some(root) = self.painted.get(&event.view_id) else {
            // Nothing has been painted for this view yet, so there is nothing
            // under the pointer.
            return;
        };
        // The router needs the tree immutably and itself mutably; they are
        // different fields, so this is a split borrow rather than an alias.
        let router = &mut self.router;
        let handled = router.dispatch(root.as_ref(), event);

        // A handler almost certainly called set_state, and a frame is the only
        // way that becomes visible.
        if handled {
            self.schedule_frame();
        }
    }
}

fn instance<'a>(app: *mut RfApp) -> Option<&'a mut AppInstance> {
    if app.is_null() {
        return None;
    }
    Some(unsafe { &mut *(app as *mut AppInstance) })
}

/// Opaque to C; every `rf_app_*` function casts it back to `AppInstance`.
#[allow(non_camel_case_types)]
pub enum RfApp {}

// -- Starting the shell -------------------------------------------------------

// Gated the same way as the C ABI below: the crate's `#[test]` binary is linked
// by rustc without the C++ engine, so a reference to rf_host_run would leave it
// undefined.
#[cfg(not(test))]
mod host_sys {
    use std::os::raw::{c_char, c_int};

    #[repr(C)]
    pub struct RfHostOptions {
        pub width: c_int,
        pub height: c_int,
        pub title: *const c_char,
        pub icu_data_path: *const c_char,
        pub enable_impeller: c_int,
    }

    unsafe extern "C" {
        pub fn rf_host_run(options: *const RfHostOptions) -> c_int;
    }
}

/// How the window and the shell are configured.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// Client-area size in physical pixels.
    pub width: i32,
    pub height: i32,
    pub title: String,
    /// Render with Impeller (OpenGL ES through ANGLE) instead of the Skia
    /// software surface. Falls back to software, with a logged reason, if the
    /// machine cannot give it a GL context.
    pub impeller: bool,
}

impl Default for RunOptions {
    fn default() -> RunOptions {
        RunOptions {
            width: 800,
            height: 600,
            title: String::from("rustflutter"),
            impeller: true,
        }
    }
}

/// Opens a window, starts the shell, and blocks until the window closes.
///
/// Call [`register_application`] first. From here on the engine is in charge:
/// frames come from vsync, and each one runs
/// `Animator -> Engine -> RuntimeController -> Application::build`.
#[cfg(not(test))]
pub fn run(options: &RunOptions) -> Result<(), i32> {
    let title = std::ffi::CString::new(options.title.as_str()).map_err(|_| -1)?;
    let raw = host_sys::RfHostOptions {
        width: options.width,
        height: options.height,
        title: title.as_ptr(),
        icu_data_path: std::ptr::null(),
        enable_impeller: if options.impeller { 1 } else { 0 },
    };
    let code = unsafe { host_sys::rf_host_run(&raw) };
    if code == 0 { Ok(()) } else { Err(code) }
}

// -- The C ABI ----------------------------------------------------------------

// Excluded from `cfg(test)`. These are `#[no_mangle]`, so the linker keeps them
// alive whether or not anything calls them, and each one reaches the engine FFI
// in rust/ffi. The crate's own `#[test]` binary is built by rustc directly and
// does not link the C++ engine, so retaining them would leave every rf_* symbol
// undefined. The functions are exercised end to end by rust_ffi_unittests
// instead, which does link it.
#[cfg(not(test))]
mod abi {
    use super::*;


    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_create(host: *const c_void) -> *mut RfApp {
        if host.is_null() {
            return std::ptr::null_mut();
        }
        // ICU has to be up before the first Paragraph is shaped, and the shell may
        // build one during the very first frame.
        engine::initialize();

        let host = unsafe { *(host as *const RfAppHost) };
        let instance = Box::new(AppInstance {
            host,
            application: None,
            views: HashMap::new(),
            view_order: Vec::new(),
            frame_number: 0,
            frame_time_micros: 0,
            painted: HashMap::new(),
            router: GestureRouter::new(),
        });
        Box::into_raw(instance) as *mut RfApp
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_destroy(app: *mut RfApp) {
        if app.is_null() {
            return;
        }
        drop(unsafe { Box::from_raw(app as *mut AppInstance) });
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_launch(app: *mut RfApp) -> c_int {
        let Some(instance) = instance(app) else {
            return -1;
        };
        if instance.application.is_some() {
            return -2;
        }

        let Some(factory) = APPLICATION_FACTORY.get() else {
            // Nothing called register_application. The shell has no framework to
            // drive, which is a programming error rather than a runtime condition.
            return -3;
        };
        instance.application = Some(factory());

        // Nothing is on screen until the first vsync, so ask for one.
        instance.schedule_frame();
        0
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_add_view(
        app: *mut RfApp,
        view_id: i64,
        metrics: *const ViewMetrics,
    ) {
        let Some(instance) = instance(app) else { return };
        if metrics.is_null() {
            return;
        }
        if instance.views.insert(view_id, unsafe { *metrics }).is_none() {
            instance.view_order.push(view_id);
        }
        instance.schedule_frame();
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_remove_view(app: *mut RfApp, view_id: i64) {
        let Some(instance) = instance(app) else { return };
        instance.views.remove(&view_id);
        instance.view_order.retain(|id| *id != view_id);
        instance.painted.remove(&view_id);
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_set_view_metrics(
        app: *mut RfApp,
        view_id: i64,
        metrics: *const ViewMetrics,
    ) {
        let Some(instance) = instance(app) else { return };
        if metrics.is_null() {
            return;
        }
        if let Some(slot) = instance.views.get_mut(&view_id) {
            *slot = unsafe { *metrics };
            instance.schedule_frame();
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_begin_frame(
        app: *mut RfApp,
        frame_time_micros: i64,
        frame_number: u64,
    ) {
        let Some(instance) = instance(app) else { return };
        instance.frame_time_micros = frame_time_micros;
        instance.frame_number = frame_number;
        // Copy the host out first: `application` borrows `instance` mutably,
        // and the scheduler needs the host at the same time.
        let scheduler = FrameScheduler { host: instance.host };
        if let Some(application) = instance.application.as_mut() {
            application.begin_frame(&FrameContext {
                frame_number,
                frame_time_micros,
                scheduler,
            });
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_draw_frame(app: *mut RfApp) {
        let Some(instance) = instance(app) else { return };
        let views = instance.view_order.clone();
        for view_id in views {
            instance.draw_view(view_id);
        }
    }

    /// Mirrors `RfPointerEvent` in runtime/rust_app_api.h. The shell narrows
    /// flutter::PointerData to this before crossing, so the layout lives in one
    /// language rather than in two that have to agree.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct RfPointerEvent {
        pub view_id: i64,
        pub device: i64,
        pub pointer_id: i64,
        pub change: i32,
        pub kind: i32,
        pub signal_kind: i32,
        pub buttons: i32,
        pub time_stamp_micros: i64,
        pub physical_x: f64,
        pub physical_y: f64,
        pub delta_x: f64,
        pub delta_y: f64,
        pub scroll_delta_x: f64,
        pub scroll_delta_y: f64,
        pub pressure: f64,
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_dispatch_pointers(
        app: *mut RfApp,
        events: *const RfPointerEvent,
        count: usize,
    ) {
        let Some(instance) = instance(app) else { return };
        if events.is_null() || count == 0 {
            return;
        }
        let events = unsafe { std::slice::from_raw_parts(events, count) };
        for raw in events {
            // Everything above this line is in physical pixels; everything
            // below is in logical ones, because that is what layout used.
            let dpr = instance
                .views
                .get(&raw.view_id)
                .map(|m| m.device_pixel_ratio)
                .filter(|dpr| *dpr > 0.0)
                .unwrap_or(1.0);
            let scale = 1.0 / dpr;
            let event = PointerEvent {
                view_id: raw.view_id,
                device: raw.device,
                pointer_id: raw.pointer_id,
                change: PointerChange::from_code(raw.change),
                kind: PointerKind::from_code(raw.kind),
                buttons: raw.buttons,
                time_stamp_micros: raw.time_stamp_micros,
                position: Offset::new(
                    (raw.physical_x * scale) as f32,
                    (raw.physical_y * scale) as f32,
                ),
                delta: Offset::new((raw.delta_x * scale) as f32, (raw.delta_y * scale) as f32),
                scroll_delta: Offset::new(
                    (raw.scroll_delta_x * scale) as f32,
                    (raw.scroll_delta_y * scale) as f32,
                ),
                pressure: raw.pressure,
                local_position: Offset::ZERO,
            };
            instance.dispatch_pointer(&event);
        }
    }
}
