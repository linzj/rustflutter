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
use crate::widgets::{BoxedWidget, Constraints, Offset, Size};

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
        root.layout(Constraints::tight(context.size.width, context.size.height));

        let mut canvas = Canvas::new(context.size.width, context.size.height);
        canvas.draw_color(background);
        root.paint(&mut canvas, Offset::ZERO);
        let display_list = canvas.build();

        let mut tree = LayerTree::new(physical_width, physical_height);
        tree.add_display_list(&display_list, 0.0, 0.0);

        if let Some(render) = self.host.render {
            // Ownership crosses here: the shell converts the handle into a
            // flow::LayerTree and frees it.
            let raw = tree.into_raw();
            unsafe { render(self.host.user_data, view_id, raw, metrics.device_pixel_ratio) };
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
    /// Render with Impeller instead of the Skia software surface. Not wired to
    /// a GL context yet, so leave it off.
    pub impeller: bool,
}

impl Default for RunOptions {
    fn default() -> RunOptions {
        RunOptions {
            width: 800,
            height: 600,
            title: String::from("rustflutter"),
            impeller: false,
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

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_dispatch_pointer_packet(
        app: *mut RfApp,
        _data: *const u8,
        _length: usize,
    ) {
        // Pointer routing needs a hit-testable render tree, which arrives with M5.
        // Accepting and dropping the packet keeps the shell's contract intact.
        let _ = instance(app);
    }
}
