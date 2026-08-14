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

use std::collections::HashMap;
use std::ffi::c_void;
use std::os::raw::c_int;

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
}

/// The root of a rustflutter application.
///
/// Register an implementation with the [`crate::app!`] macro; the shell
/// instantiates it once and calls `build` every frame.
pub trait Application {
    /// Builds the widget tree for one view.
    fn build(&mut self, context: &BuildContext) -> BoxedWidget;

    /// Colour painted before the widget tree. Defaults to white.
    fn background(&self) -> Color {
        Color::WHITE
    }

    /// Advances animations. Called before `build`, matching dart:ui's
    /// `onBeginFrame` running ahead of `onDrawFrame`.
    fn begin_frame(&mut self, _frame_time_micros: i64, _frame_number: u64) {}
}

// The app crate provides this through the `app!` macro. Declaring it here and
// resolving it at link time is what lets the framework be a library while the
// entry point lives in the application.
unsafe extern "C" {
    fn rustflutter_create_application() -> *mut c_void;
}

// The crate's own test binary has no application to link against, so it
// provides the symbol itself. `app!` would be a duplicate definition here.
#[cfg(test)]
#[unsafe(no_mangle)]
pub extern "C" fn rustflutter_create_application() -> *mut c_void {
    std::ptr::null_mut()
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

// -- The C ABI ----------------------------------------------------------------

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

    let raw = unsafe { rustflutter_create_application() };
    if raw.is_null() {
        return -3;
    }
    instance.application =
        Some(*unsafe { Box::from_raw(raw as *mut Box<dyn Application>) });

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
    if let Some(application) = instance.application.as_mut() {
        application.begin_frame(frame_time_micros, frame_number);
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

/// Declares the root of an application.
///
/// ```ignore
/// struct HelloWorld;
/// impl rustflutter::Application for HelloWorld { /* ... */ }
/// rustflutter::app!(HelloWorld);
/// ```
///
/// Expands to the `rustflutter_create_application` symbol the framework
/// resolves at link time -- the equivalent of Dart's `main()` being found by
/// name in the app's isolate.
#[macro_export]
macro_rules! app {
    ($root:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn rustflutter_create_application() -> *mut ::core::ffi::c_void {
            let application: ::std::boxed::Box<dyn $crate::Application> =
                ::std::boxed::Box::new($root);
            ::std::boxed::Box::into_raw(::std::boxed::Box::new(application))
                as *mut ::core::ffi::c_void
        }
    };
}
