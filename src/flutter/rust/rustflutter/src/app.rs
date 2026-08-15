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
use std::os::raw::{c_char, c_int};
use std::sync::OnceLock;

use crate::engine::{self, Color, LayerTree};
use crate::framework::{AnyWidget, ElementTree};
use crate::gestures::{GestureRouter, PointerChange, PointerEvent, PointerKind};
use crate::keyboard::{KeyEvent, Keyboard};
use crate::platform;
use crate::services;
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
    send_platform_message: Option<
        unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize, i64),
    >,
    respond_to_platform_message:
        Option<unsafe extern "C" fn(*mut c_void, i64, *const u8, usize)>,
    send_channel_update: Option<unsafe extern "C" fn(*mut c_void, *const c_char, bool)>,
}

/// The shell, as the messenger sees it.
///
/// Implements [`services::PlatformSink`] over the host callbacks, which is the
/// whole of the framework's outward half of a platform message: everything
/// above it -- codecs, channels, the named ones in
/// [`services::system`](crate::services::system) -- is ordinary Rust with no
/// knowledge that a C ABI is down here.
struct HostSink {
    host: RfAppHost,
    /// Whether the shell behind `host.user_data` is still there.
    ///
    /// A [`services::Responder`] holds a share of this sink and may outlive the
    /// application -- a handler is allowed to answer from a callback, and
    /// nothing stops that callback running after the shell has gone. The
    /// pointer inside `host` would be dangling by then, so the sink is switched
    /// off before the shell is torn down and every call becomes a no-op.
    alive: std::cell::Cell<bool>,
}

impl HostSink {
    fn new(host: RfAppHost) -> HostSink {
        HostSink { host, alive: std::cell::Cell::new(true) }
    }

    /// Cuts the sink off from the shell. Called before the shell is destroyed.
    fn disconnect(&self) {
        self.alive.set(false);
    }

    /// A channel name as a NUL-terminated C string.
    ///
    /// Allocated per call rather than cached: a channel name crosses once per
    /// message, and a message is already a copy of its bytes plus a thread hop.
    /// A name with an interior NUL cannot be sent at all -- it would arrive
    /// truncated, on a different channel than the one asked for -- so it is
    /// refused instead.
    fn c_channel(channel: &str) -> Option<std::ffi::CString> {
        std::ffi::CString::new(channel).ok()
    }
}

impl services::PlatformSink for HostSink {
    fn send(&self, channel: &str, message: &[u8], response_id: i64) {
        if !self.alive.get() {
            return;
        }
        let Some(send) = self.host.send_platform_message else {
            return;
        };
        let Some(name) = HostSink::c_channel(channel) else {
            return;
        };
        unsafe {
            send(
                self.host.user_data,
                name.as_ptr(),
                message.as_ptr(),
                message.len(),
                response_id,
            );
        }
    }

    fn respond(&self, response_id: i64, reply: services::ReplyData<'_>) {
        if !self.alive.get() {
            return;
        }
        let Some(respond) = self.host.respond_to_platform_message else {
            return;
        };
        // A null pointer is "nothing handled it", which the shell passes on as
        // CompleteEmpty. An empty slice would say something different and mean
        // something different at the far end.
        let (pointer, length) = match reply {
            Some(bytes) => (bytes.as_ptr(), bytes.len()),
            None => (std::ptr::null(), 0),
        };
        unsafe { respond(self.host.user_data, response_id, pointer, length) };
    }

    fn channel_update(&self, channel: &str, listening: bool) {
        if !self.alive.get() {
            return;
        }
        let Some(update) = self.host.send_channel_update else {
            return;
        };
        let Some(name) = HostSink::c_channel(channel) else {
            return;
        };
        unsafe { update(self.host.user_data, name.as_ptr(), listening) };
    }

    fn request_frame(&self) {
        if !self.alive.get() {
            return;
        }
        if let Some(schedule) = self.host.schedule_frame {
            unsafe { schedule(self.host.user_data) };
        }
    }
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
                send_platform_message: None,
                respond_to_platform_message: None,
                send_channel_update: None,
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

    /// Handles a key, before anything else sees it. Return true if it was used.
    ///
    /// This is the application-wide layer, and deliberately only that: it runs
    /// for every key no matter what is on screen, which makes it right for
    /// shortcuts and wrong for anything a particular widget should own. Widgets
    /// need focus to be addressed, and there is no focus tree yet -- see the
    /// [`keyboard`](crate::keyboard) module for what that costs and why it is
    /// separate work.
    ///
    /// Upstream's counterpart is `FocusManager`'s early key event handlers,
    /// which likewise run ahead of the focus walk and likewise see everything.
    ///
    /// Returning true schedules a frame, on the assumption that a handled key
    /// changed something. It does not stop the platform from also acting on the
    /// key; nothing here can suppress Alt+F4.
    fn on_key(&mut self, _event: &KeyEvent, _keyboard: &Keyboard) -> bool {
        false
    }
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

    /// See [`Application::on_key`]. A widget application will usually want to
    /// mark something dirty here rather than change state directly, because the
    /// key arrives between frames and the tree is not being built.
    fn on_key(&mut self, _event: &KeyEvent, _keyboard: &Keyboard) -> bool {
        false
    }
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

    fn on_key(&mut self, event: &KeyEvent, keyboard: &Keyboard) -> bool {
        self.app.on_key(event, keyboard)
    }

    fn build(&mut self, context: &BuildContext) -> BoxedWidget {
        // The clock was already moved forward in begin_frame; this only makes
        // sure every build in the frame reads the same value.
        self.tree.set_frame_time(context.frame_time_micros);

        // A photograph that finished decoding since the last frame is not on
        // screen until whoever asked for it is built again, and nothing records
        // who that was -- so everyone is, exactly as for a resize. See
        // `painting::take_images_arrived` for what the narrow version needs.
        let images_arrived = crate::painting::take_images_arrived();
        let resized = self.last_size != Some(context.size);
        let mounted = if self.tree.is_empty() || resized || images_arrived {
            let root = self.app.build(context);
            self.tree.rebuild(root);
            self.last_size = Some(context.size);
            true
        } else {
            self.tree.rebuild_dirty() > 0
        };

        // Anything built this frame has never been asked whether it wants to
        // move, because advancing happens before building. One more frame gives
        // it the chance; if it says no, that is where the loop stops. Without
        // this a screen whose animation starts on mount -- a spinner, a page
        // that fades itself in -- draws its first frame and freezes, because
        // the only thing that would have asked for a second one is the advance
        // it was never part of.
        if mounted {
            context.scheduler.request_frame();
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

/// Where a frame's time goes on the UI thread, when anyone is asking.
///
/// Set RUSTFLUTTER_FRAME_STATS to any value. The report is a median over sixty
/// frames rather than a mean, because a first paint or a newly shaped run of
/// text is a one-off and averaging hides which of the three phases is actually
/// the expensive one.
///
/// It measures the UI thread only. Rasterising happens on another thread and is
/// reported by the host, next to the frame interval.
struct FrameTimings;

impl FrameTimings {
    #[inline]
    fn now() -> std::time::Instant {
        std::time::Instant::now()
    }

    fn enabled() -> bool {
        static ENABLED: OnceLock<bool> = OnceLock::new();
        *ENABLED.get_or_init(|| std::env::var_os("RUSTFLUTTER_FRAME_STATS").is_some())
    }

    fn record(
        advance_ms: f64,
        started: std::time::Instant,
        built: std::time::Instant,
        laid_out: std::time::Instant,
        painted: std::time::Instant,
    ) {
        if !Self::enabled() {
            return;
        }
        thread_local! {
            static SAMPLES: std::cell::RefCell<Vec<(f64, f64, f64, f64)>> =
                const { std::cell::RefCell::new(Vec::new()) };
        }
        let ms = |a: std::time::Instant, b: std::time::Instant| {
            b.duration_since(a).as_secs_f64() * 1000.0
        };
        SAMPLES.with(|samples| {
            let mut samples = samples.borrow_mut();
            samples.push((
                advance_ms,
                ms(started, built),
                ms(built, laid_out),
                ms(laid_out, painted),
            ));
            if samples.len() < 60 {
                return;
            }
            let median = |pick: fn(&(f64, f64, f64, f64)) -> f64| {
                let mut values: Vec<f64> = samples.iter().map(pick).collect();
                values.sort_by(|a, b| a.partial_cmp(b).expect("no NaN durations"));
                values[values.len() / 2]
            };
            let advance = median(|s| s.0);
            let build = median(|s| s.1);
            let layout = median(|s| s.2);
            let paint = median(|s| s.3);
            eprintln!(
                "ui thread: advance {advance:.2} ms, build {build:.2} ms, \
                 layout {layout:.2} ms, record {paint:.2} ms, total {:.2} ms \
                 (median of {})",
                advance + build + layout + paint,
                samples.len()
            );
            samples.clear();
        });
    }
}

/// Composes one frame's picture into the layer tree the shell rasterizes.
///
/// Layout and paint work in logical pixels; the layer tree is measured in
/// physical ones, and on a display that is not at 100% those differ. Upstream
/// `RenderView` bridges the gap with a transform layer at the root of every
/// scene, and this is that layer.
///
/// A layer rather than a `Canvas::scale` recorded inside the picture, for the
/// same reason upstream chose one: a scale the compositor can see is one it can
/// take into account when it decides what resolution to rasterize a cached
/// subtree at. A scale hidden inside a display list would have it cache at the
/// wrong size and then stretch.
///
/// It lives here, rather than at each call site, so that the windowed path and
/// the headless one cannot come to disagree about what a frame is -- which
/// would make every screenshot a picture of something the user never sees.
pub fn compose_frame(
    physical_width: i32,
    physical_height: i32,
    device_pixel_ratio: f64,
    logical: Size,
    background: Color,
    paint: impl FnOnce(&mut PaintContext),
) -> LayerTree {
    let mut tree = LayerTree::new(physical_width, physical_height);
    let dpr = device_pixel_ratio as f32;
    // Exactly one means the two coordinate systems coincide, and pushing an
    // identity would cost a layer to say nothing.
    let scaled = dpr > 0.0 && dpr != 1.0;
    if scaled {
        tree.push_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0);
    }
    {
        let mut context = PaintContext::new(&mut tree, logical);
        context.canvas().draw_color(background);
        paint(&mut context);
        // Dropping the context hands over the last picture.
    }
    if scaled {
        tree.pop();
    }
    tree
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
    /// Which keys are held. Lives here rather than in the application because
    /// it has to survive an application that does nothing with keys at all --
    /// it is the shell's record of the platform's state, not the app's.
    keyboard: Keyboard,
    /// How long this frame's `begin_frame` took, in milliseconds. It is
    /// measured there and reported here because the shell calls the two halves
    /// of a frame separately, and a report that started at `build` would leave
    /// the tickers out of the total it calls "ui thread".
    advance_ms: f64,
    /// The messenger's way out to the shell, kept so that teardown can switch
    /// it off. See [`HostSink::alive`].
    sink: std::rc::Rc<HostSink>,
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
        let advance_ms = self.advance_ms;
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

        let started = FrameTimings::now();
        let mut root = application.build(&context);
        let built = FrameTimings::now();

        // Something asked for an image that a worker is still decoding, and
        // drew a placeholder instead. Ask for another frame: the one the
        // picture lands in is the one that shows it. Upstream reaches the same
        // place from the other direction -- the decoder completes a future,
        // which marks the image widget dirty.
        if crate::painting::images_pending() {
            context.scheduler.request_frame();
        }

        // Constraints down, sizes up -- the RenderBox protocol. The root is
        // forced to the view size, which is what RenderView does upstream.
        root.layout(BoxConstraints::tight(context.size.width, context.size.height));
        let laid_out = FrameTimings::now();

        let tree = compose_frame(
            physical_width,
            physical_height,
            metrics.device_pixel_ratio,
            context.size,
            background,
            |paint_context| root.paint(paint_context, Offset::ZERO),
        );
        // All the shaping this frame needed has happened by now -- layout asked
        // for it and paint drew from what layout kept. Ageing the cache here
        // retires anything that stopped being drawn a frame ago.
        crate::painting::end_text_frame();
        let painted = FrameTimings::now();
        FrameTimings::record(advance_ms, started, built, laid_out, painted);

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

    /// Routes one key to the application.
    ///
    /// Unlike a pointer this does not consult the painted tree, because there
    /// is nothing to consult it *with*: a key has no position, and without a
    /// focus tree there is nobody it is addressed to. So it goes straight to
    /// the application-wide handler. See the [`keyboard`](crate::keyboard)
    /// module.
    fn dispatch_key(&mut self, event: &mut KeyEvent) -> bool {
        // The pressed set is updated first, and whether the app handles the key
        // or not. It is the platform's state; an app that ignores Shift still
        // has to see it held when the next key it does care about arrives.
        self.keyboard.record(event);

        let Some(application) = self.application.as_mut() else {
            return false;
        };
        let handled = application.on_key(event, &self.keyboard);
        if handled {
            self.schedule_frame();
        }
        handled
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
    /// Client-area size in logical pixels. The host scales it by the display's
    /// DPI, so a window asked for at 1000x700 looks the same size on a 200%
    /// display as on a 100% one.
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
///
/// `Err(code)` is what the process should exit with, and it covers two things
/// that are the same thing from the caller's side: the host could not start
/// (negative), or the application asked to exit with a code of its own through
/// [`services::system::exit_application`] (positive).
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
        let sink = std::rc::Rc::new(HostSink::new(host));
        let instance = Box::new(AppInstance {
            sink: sink.clone(),
            host,
            application: None,
            views: HashMap::new(),
            view_order: Vec::new(),
            frame_number: 0,
            frame_time_micros: 0,
            painted: HashMap::new(),
            router: GestureRouter::new(),
            keyboard: Keyboard::new(),
            advance_ms: 0.0,
        });

        // The messenger is wired up before the application exists, because the
        // shell may deliver a platform message before `rf_app_launch` -- the
        // Windows embedder sends the lifecycle state as soon as the window is
        // up. Buffering catches those, but only once there is somewhere to
        // buffer them.
        services::attach(sink);

        Box::into_raw(instance) as *mut RfApp
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_destroy(app: *mut RfApp) {
        if app.is_null() {
            return;
        }
        let instance = unsafe { Box::from_raw(app as *mut AppInstance) };
        // The sink is switched off rather than merely dropped: a responder the
        // application kept holds its own share of it, and calling that after
        // the shell is gone would reach through a dangling pointer. After this
        // it reaches nothing.
        instance.sink.disconnect();
        // Before the application is dropped, so that a handler dropped here
        // cannot be asked to run against a half-torn-down instance, and so that
        // anything still waiting on a reply is failed rather than left waiting.
        services::detach();
        // Thread-local as well, and for the same reason: a second app on this
        // thread must not start out believing the first one's platform state.
        platform::reset();
        drop(instance);
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

    /// The `flutter/settings` payload, which `Engine` took on the way past.
    ///
    /// Not a platform message by the time it reaches here, and upstream does
    /// the same thing for the same reason: settings are state the framework
    /// keeps, not a call it answers.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_set_user_settings(
        app: *mut RfApp,
        json: *const c_char,
        length: usize,
    ) {
        if instance(app).is_none() || json.is_null() {
            return;
        }
        let bytes = unsafe { std::slice::from_raw_parts(json as *const u8, length) };
        // A settings payload that is not UTF-8 is a broken embedder, and the
        // right thing is to keep the settings we have rather than guess.
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        platform::set_user_settings(text);
    }

    /// The preferred locales, four strings each: language, country, script,
    /// variant. See `rf_app_set_locales` in rust_app_api.h for the layout.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_set_locales(
        app: *mut RfApp,
        locales: *const *const c_char,
        count: usize,
    ) {
        if instance(app).is_none() || locales.is_null() {
            return;
        }
        let flat = unsafe { std::slice::from_raw_parts(locales, count * 4) };
        let mut parsed = Vec::with_capacity(count);
        for group in flat.chunks_exact(4) {
            // An empty string is how the message says "this part is absent",
            // which is not the same as a part whose value is empty.
            let read = |pointer: *const c_char| -> Option<String> {
                if pointer.is_null() {
                    return None;
                }
                let text = unsafe { std::ffi::CStr::from_ptr(pointer) }
                    .to_string_lossy()
                    .into_owned();
                if text.is_empty() { None } else { Some(text) }
            };
            // The language code is the one part that is required. A locale
            // without one says nothing at all, so it is dropped rather than
            // stored as a locale that no lookup could ever match.
            let Some(language_code) = read(group[0]) else {
                continue;
            };
            parsed.push(platform::Locale {
                language_code,
                country_code: read(group[1]),
                script_code: read(group[2]),
                variant_code: read(group[3]),
            });
        }
        platform::set_locales(parsed);
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
        let started = FrameTimings::now();
        if let Some(application) = instance.application.as_mut() {
            application.begin_frame(&FrameContext {
                frame_number,
                frame_time_micros,
                scheduler,
            });
        }
        instance.advance_ms =
            FrameTimings::now().duration_since(started).as_secs_f64() * 1000.0;
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
                signal_kind: crate::gestures::SignalKind::from_raw(raw.signal_kind),
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

    /// Mirrors `RfKeyEvent` in runtime/rust_app_api.h.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct RfKeyEvent {
        pub time_stamp_micros: i64,
        pub change: i32,
        pub physical: u64,
        pub logical: u64,
        pub synthesized: bool,
        pub character: *const std::os::raw::c_char,
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_dispatch_key(
        app: *mut RfApp,
        raw: *const RfKeyEvent,
    ) -> bool {
        let Some(instance) = instance(app) else { return false };
        if raw.is_null() {
            return false;
        }
        let raw = unsafe { &*raw };

        // The character is the shell's, borrowed for the length of this call.
        // It is copied rather than borrowed on because a handler may keep it,
        // and a lossy conversion is right here: the shell produced it from
        // UTF-16, so invalid UTF-8 means a bug rather than a user's input.
        let character = if raw.character.is_null() {
            None
        } else {
            let text = unsafe { std::ffi::CStr::from_ptr(raw.character) };
            let text = text.to_string_lossy();
            if text.is_empty() { None } else { Some(text.into_owned()) }
        };

        let mut event = KeyEvent {
            change: crate::keyboard::KeyChange::from_code(raw.change),
            physical: crate::keyboard::PhysicalKey(raw.physical),
            logical: crate::keyboard::LogicalKey(raw.logical),
            character,
            synthesized: raw.synthesized,
            time_stamp_micros: raw.time_stamp_micros,
        };
        instance.dispatch_key(&mut event)
    }

    /// Delivers a platform message from the embedder.
    ///
    /// Everything but `flutter/keydata` arrives here; that one channel is
    /// unpacked in `RuntimeController` because its payload is a packed struct
    /// rather than a codec's output, which is upstream's arrangement too.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_dispatch_platform_message(
        app: *mut RfApp,
        channel: *const c_char,
        message: *const u8,
        length: usize,
        response_id: i64,
    ) {
        let Some(instance) = instance(app) else { return };
        if channel.is_null() {
            return;
        }
        // Borrowed for the length of this call, and copied on the way into the
        // messenger, which may hold it until a handler appears.
        let name = unsafe { std::ffi::CStr::from_ptr(channel) };
        let Ok(name) = name.to_str() else {
            return;
        };
        let bytes = if message.is_null() || length == 0 {
            &[][..]
        } else {
            unsafe { std::slice::from_raw_parts(message, length) }
        };

        // The frame this probably needs is asked for by the messenger, which
        // is the only place that knows whether a handler actually ran -- a
        // message may be buffered here and delivered minutes later.
        let _ = instance;
        services::handle_platform_message(name, bytes, response_id);
    }

    /// Hands the framework the reply to a message it sent.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_complete_platform_message_reply(
        app: *mut RfApp,
        response_id: i64,
        reply: *const u8,
        length: usize,
    ) {
        let Some(instance) = instance(app) else { return };
        // Null is "nothing handled it", which the caller tells apart from an
        // empty reply all the way up to MethodChannel::invoke_with_reply.
        let bytes = if reply.is_null() {
            None
        } else {
            Some(unsafe { std::slice::from_raw_parts(reply, length) })
        };
        let _ = instance;
        services::complete_reply(response_id, bytes);
    }
}
