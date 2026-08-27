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
use crate::render::{BoxConstraints, EdgeInsets, PaintContext, RenderBox};
use crate::services;
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

    /// What the system draws over, in logical pixels: the status bar, a notch,
    /// the gesture bar. Unaffected by the keyboard.
    ///
    /// Upstream's `FlutterView.viewPadding`. The name of the field it comes
    /// from is `padding`, not `viewPadding`, because that is the slot the
    /// embedders fill: `FlutterRenderer` on Android sends `viewPaddingTop` into
    /// `physicalPaddingTop`, and `dart:ui` derives the other one.
    pub fn view_padding(&self) -> EdgeInsets {
        self.logical_insets(
            self.padding_left,
            self.padding_top,
            self.padding_right,
            self.padding_bottom,
        )
    }

    /// What is covering the view and pushing content out of the way, in
    /// logical pixels -- the software keyboard, and essentially only it.
    pub fn view_insets(&self) -> EdgeInsets {
        self.logical_insets(
            self.view_inset_left,
            self.view_inset_top,
            self.view_inset_right,
            self.view_inset_bottom,
        )
    }

    /// What the system still covers once the keyboard has taken its share:
    /// `max(0, view_padding - view_insets)` per side.
    ///
    /// This is `FlutterView.padding`, and upstream computes it in `window.dart`
    /// for the same reason it is computed here rather than in the widget layer
    /// -- it is a property of the view, not a decision a widget makes.
    pub fn padding(&self) -> EdgeInsets {
        let view_padding = self.view_padding();
        let insets = self.view_insets();
        EdgeInsets {
            left: (view_padding.left - insets.left).max(0.0),
            top: (view_padding.top - insets.top).max(0.0),
            right: (view_padding.right - insets.right).max(0.0),
            bottom: (view_padding.bottom - insets.bottom).max(0.0),
        }
    }

    fn logical_insets(&self, left: f64, top: f64, right: f64, bottom: f64) -> EdgeInsets {
        let dpr = if self.device_pixel_ratio > 0.0 {
            self.device_pixel_ratio
        } else {
            1.0
        };
        EdgeInsets {
            left: (left / dpr) as f32,
            top: (top / dpr) as f32,
            right: (right / dpr) as f32,
            bottom: (bottom / dpr) as f32,
        }
    }
}

// -- What the shell gives us --------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
struct RfAppHost {
    user_data: *mut c_void,
    render: Option<unsafe extern "C" fn(*mut c_void, i64, *mut engine::sys::RfLayerTree, f64)>,
    schedule_frame: Option<unsafe extern "C" fn(*mut c_void)>,
    send_platform_message:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const u8, usize, i64)>,
    respond_to_platform_message: Option<unsafe extern "C" fn(*mut c_void, i64, *const u8, usize)>,
    send_channel_update: Option<unsafe extern "C" fn(*mut c_void, *const c_char, bool)>,
    update_semantics: Option<unsafe extern "C" fn(*mut c_void, i64, *const RfSemanticsNode, usize)>,
    /// Asks for `rf_app_run_tasks` on the UI thread. The one callback here that
    /// may be invoked from any thread, and the only reason `task` can wake a
    /// future from a decode worker: `schedule_frame` cannot be called from off
    /// the UI thread, and `fml::TaskRunner::PostTask` can.
    ///
    /// `None` from an embedder that predates the field -- `RfAppHost` is zero
    /// initialised on the C++ side and this was added at the end, so an
    /// unaware host degrades to draining tasks once per frame rather than
    /// crashing.
    post_task: Option<unsafe extern "C" fn(*mut c_void)>,
    /// The delayed twin, and the framework's only clock other than the frame's.
    /// See [`task::sleep`](crate::task::sleep) for why the framework's own
    /// deadlines do not use it.
    post_delayed_task: Option<unsafe extern "C" fn(*mut c_void, i64)>,
}

/// This struct and `RfAppHost` in `runtime/rust_app_api.h` are two hand-written
/// mirrors of one ABI, and nothing but a reader keeps them in step. The count
/// is the cheap half of that: one `user_data` and eight callbacks, all pointer
/// sized. `runtime_controller.cc` carries the matching `static_assert`, so a
/// field added to one side and not the other fails to build rather than
/// reading the next field's bytes.
const _: () = assert!(
    size_of::<RfAppHost>() == size_of::<*mut c_void>() * 9,
    "RfAppHost has drifted from rust_app_api.h"
);

/// One semantics node, as the C ABI carries it. Mirrors `RfSemanticsNode` in
/// `rust_app_api.h`; the two have to agree field for field.
#[repr(C)]
pub struct RfSemanticsNode {
    pub id: i32,
    pub flags: i32,
    pub actions: i32,
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub label: *const c_char,
    pub value: *const c_char,
    pub hint: *const c_char,
    pub increased_value: *const c_char,
    pub decreased_value: *const c_char,
    pub scroll_position: f64,
    pub scroll_extent_min: f64,
    pub scroll_extent_max: f64,
    pub children: *const i32,
    pub child_count: usize,
    /// The reading direction of the label and its kin: 0 = unknown, 1 = rtl,
    /// 2 = ltr -- the embedder's `FlutterTextDirection`, which is what the
    /// engine's `SemanticsNode::textDirection` holds. A node with nothing to
    /// read crosses as 0.
    pub text_direction: i32,
}

/// Bit positions of `RfSemanticsNode::flags`, matching `rust_app_api.h`.
mod semantics_bits {
    pub const IS_BUTTON: i32 = 1 << 0;
    pub const IS_TEXT_FIELD: i32 = 1 << 1;
    pub const IS_HEADER: i32 = 1 << 2;
    pub const IS_IMAGE: i32 = 1 << 3;
    pub const IS_LINK: i32 = 1 << 4;
    pub const IS_SLIDER: i32 = 1 << 5;
    pub const IS_OBSCURED: i32 = 1 << 6;
    pub const IS_READ_ONLY: i32 = 1 << 7;
    pub const IS_LIVE_REGION: i32 = 1 << 8;
    pub const HAS_CHECKED_STATE: i32 = 1 << 9;
    pub const IS_CHECKED: i32 = 1 << 10;
    pub const HAS_ENABLED_STATE: i32 = 1 << 11;
    pub const IS_ENABLED: i32 = 1 << 12;
    pub const IS_SELECTED: i32 = 1 << 13;
    pub const IS_FOCUSED: i32 = 1 << 14;
    /// The fourth check state, which the two bits above cannot carry between
    /// them. Read only when `HAS_CHECKED_STATE` is set.
    pub const IS_CHECK_STATE_MIXED: i32 = 1 << 15;
    pub const HAS_TOGGLED_STATE: i32 = 1 << 16;
    pub const IS_TOGGLED: i32 = 1 << 17;
    pub const HAS_EXPANDED_STATE: i32 = 1 << 18;
    pub const IS_EXPANDED: i32 = 1 << 19;
    pub const HAS_REQUIRED_STATE: i32 = 1 << 20;
    pub const IS_REQUIRED: i32 = 1 << 21;
    pub const HAS_SELECTED_STATE: i32 = 1 << 22;
    pub const HAS_FOCUSED_STATE: i32 = 1 << 23;
    pub const NAMES_ROUTE: i32 = 1 << 24;
    pub const IS_HIDDEN: i32 = 1 << 25;
}

/// Packs the framework's flags into the ABI's bit set.
pub fn pack_semantics_flags(flags: &crate::semantics::SemanticsFlags) -> i32 {
    use semantics_bits::*;
    let mut bits = 0;
    let set = |bits: &mut i32, on: bool, bit: i32| {
        if on {
            *bits |= bit;
        }
    };
    set(&mut bits, flags.is_button, IS_BUTTON);
    set(&mut bits, flags.is_text_field, IS_TEXT_FIELD);
    set(&mut bits, flags.is_header, IS_HEADER);
    set(&mut bits, flags.names_route, NAMES_ROUTE);
    set(&mut bits, flags.is_hidden, IS_HIDDEN);
    set(&mut bits, flags.is_image, IS_IMAGE);
    set(&mut bits, flags.is_link, IS_LINK);
    set(&mut bits, flags.is_slider, IS_SLIDER);
    set(&mut bits, flags.is_obscured, IS_OBSCURED);
    set(&mut bits, flags.is_read_only, IS_READ_ONLY);
    set(&mut bits, flags.is_live_region, IS_LIVE_REGION);
    // Three bits for four states, which is the cheapest honest encoding:
    // "checkable" gates the other two, and "mixed" outranks "checked".
    use crate::semantics::SemanticsCheckState;
    set(&mut bits, flags.checked.is_checkable(), HAS_CHECKED_STATE);
    set(
        &mut bits,
        flags.checked == SemanticsCheckState::Checked,
        IS_CHECKED,
    );
    set(
        &mut bits,
        flags.checked == SemanticsCheckState::Mixed,
        IS_CHECK_STATE_MIXED,
    );
    set(&mut bits, flags.has_enabled_state, HAS_ENABLED_STATE);
    set(&mut bits, flags.is_enabled, IS_ENABLED);

    // The three tristates, each a "has it" bit gating an "is it" one -- the
    // same encoding the checked pair uses, and for the same reason: "no
    // opinion" is a third thing and one bit says two.
    use crate::semantics::SemanticsTristate;
    for (state, has, is) in [
        (flags.toggled, HAS_TOGGLED_STATE, IS_TOGGLED),
        (flags.expanded, HAS_EXPANDED_STATE, IS_EXPANDED),
        (flags.required, HAS_REQUIRED_STATE, IS_REQUIRED),
        (flags.selected, HAS_SELECTED_STATE, IS_SELECTED),
        (flags.focused, HAS_FOCUSED_STATE, IS_FOCUSED),
    ] {
        set(&mut bits, state.is_set(), has);
        set(&mut bits, state == SemanticsTristate::True, is);
    }
    bits
}

/// Packs the framework's reading direction into the ABI's.
///
/// `None` is upstream's null `textDirection` -- nothing to read, so nothing
/// to say which way it runs -- and crosses as 0, the embedder's
/// `kFlutterTextDirectionUnknown`, exactly as a null does one layer up in
/// `SemanticsUpdateBuilder.updateNode`.
pub fn pack_text_direction(direction: Option<crate::direction::TextDirection>) -> i32 {
    use crate::direction::TextDirection;
    match direction {
        Some(TextDirection::Rtl) => 1,
        Some(TextDirection::Ltr) => 2,
        None => 0,
    }
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
        HostSink {
            host,
            alive: std::cell::Cell::new(true),
        }
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
                update_semantics: None,
                post_task: None,
                post_delayed_task: None,
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
        WidgetHost {
            app,
            tree: ElementTree::new(),
            last_size: None,
        }
    }

    /// The element tree, for tests and diagnostics.
    pub fn tree(&self) -> &ElementTree {
        &self.tree
    }
}

/// The one tap-region surface, above the overlay. An id rather than a name
/// because that is what a region is keyed by; nothing else in a tree reaches
/// for it, so it only has to be a number no application would pick.
const TAP_REGION_SURFACE_ID: u64 = 0x7A9_0000;

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
        // Escape takes down the topmost dismissible modal, before the
        // application sees it. Upstream this is a `DismissIntent` travelling
        // the actions system to the `ModalRoute`; `actions.rs` and
        // `shortcuts.rs` are still pure-logic ports, so the key is routed
        // directly here and this becomes a `DismissIntent` when they are live.
        //
        // Before the application, because a modal is *modal*: while a dialog is
        // up, the page behind it should not be acting on keys any more than it
        // should be acting on presses.
        if event.is_down()
            && event.logical == crate::keyboard::LogicalKey::ESCAPE
            && crate::theatre::dismiss_topmost_modal()
        {
            return true;
        }
        self.app.on_key(event, keyboard)
    }

    fn build(&mut self, context: &BuildContext) -> BoxedWidget {
        // The clock was already moved forward in begin_frame; this only makes
        // sure every build in the frame reads the same value.
        self.tree.set_frame_time(context.frame_time_micros);
        // Focus nodes outlive the frames their widgets do not rebuild in, so
        // this drops only the ones whose elements have actually gone.
        let tree = &self.tree;
        crate::focus::prune(|element| tree.is_live(element));

        // A photograph that finished decoding since the last frame is not on
        // screen until whoever asked for it is built again, and nothing records
        // who that was -- so everyone is, exactly as for a resize. See
        // `painting::take_images_arrived` for what the narrow version needs.
        let images_arrived = crate::painting::take_images_arrived();
        let resized = self.last_size != Some(context.size);
        let data = crate::media_query::MediaQueryData::from_view(&context.metrics);
        // The reader's language, from the same place the text scale and the
        // brightness come from. Upstream's `WidgetsApp` builds a
        // `Localizations` around what you gave it; this is the same position
        // and the same reason -- everything below can ask what language it is
        // in without being handed one.
        let localizations = crate::localizations::Localizations::new(crate::platform::locale());
        let mounted = if self.tree.is_empty() || resized || images_arrived {
            // Published above the application's own root, which is where
            // upstream puts it too: `WidgetsApp` wraps what you gave it in a
            // `MediaQuery.fromView`. Everything below can then ask how big the
            // view is and what is covering it without being handed either.
            //
            // A resize goes through here rather than through `publish` because
            // the application's own `build` is handed the size: whatever it
            // decides from that has to be decided again.
            // The overlay goes *inside* the MediaQuery and *outside* the
            // application, which is where upstream's `WidgetsApp` puts its
            // `Navigator`'s `Overlay`: an entry needs to know how big the view
            // is as much as the page does, and a dialog put up by the
            // application has to land above everything the application built.
            let root = crate::localizations::provide_localizations(
                localizations,
                crate::media_query::MediaQuery::new(
                    data,
                    // Outside the overlay, because the regions it has to
                    // classify are on both sides of it: a text field down in
                    // the application, and the selection toolbar that belongs
                    // to it up in an overlay entry. A surface under the
                    // overlay would read every tap on the toolbar as a tap
                    // somewhere else and take the keyboard away.
                    crate::tap_region::TapRegionSurface::new(
                        TAP_REGION_SURFACE_ID,
                        crate::theatre::overlay(self.app.build(context)),
                    ),
                ),
            );
            self.tree.rebuild(root);
            self.last_size = Some(context.size);
            true
        } else {
            // Everything else about the view -- the keyboard arriving, the
            // status bar changing height, the reader turning the text size up
            // -- goes to the widgets that asked about it and to nothing else.
            // This is what the dependency tracking is for, and the case that
            // shows it is a keyboard opening: the insets change on every frame
            // of that animation, and the answer should not be rebuilding the
            // page thirty times.
            let republished = self.tree.publish(data);
            // The language goes the same way, and for the same reason: a
            // reader who changes their system language should not cost a
            // remount of everything, only a rebuild of whoever asked.
            let relocalised = self.tree.publish(localizations);
            self.tree.rebuild_dirty() > 0 || republished || relocalised
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
            .unwrap_or_else(|| crate::render::RenderRef::new(crate::widgets::Empty))
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

        // Build, layout and paint all reach the element tree through the same
        // `RefCell`s, and hold them out on loan while they run. A task resumed
        // in the middle would arrive at cells that are already borrowed, so the
        // drain is forbidden for the length of this and happens between the
        // phases instead -- see `task::run_until_stalled` and the
        // `rf_app_run_tasks` between begin_frame and draw_frame.
        let _phase = crate::task::FramePhase::enter();

        let started = FrameTimings::now();
        let root = application.build(&context);
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
        //
        // The whole of a frame's layout is the flush, as it is upstream:
        // `RendererBinding.drawFrame` opens with `flushLayout` and has no walk
        // from the root at all. It lays out the relayout boundaries
        // `mark_needs_layout` enqueued, each from itself against the
        // constraints it remembers, so a change under a boundary costs that
        // subtree and not the whole screen.
        //
        // There is no walk to fall back on because a walk could not do the
        // job: a mark stops at a boundary and leaves every ancestor clean, so
        // a descent from the root early returns at the first clean object it
        // meets and never arrives. Everything a walk was here for is a mark
        // instead -- upstream says a resized view by calling `markNeedsLayout`
        // from `RenderView`'s `configuration` setter, and says a root that has
        // never been laid out by `scheduleInitialLayout`. Both are
        // `schedule_root_layout`, and both land in the queue the flush drains.
        let view_constraints = BoxConstraints::tight(context.size.width, context.size.height);
        crate::render::schedule_root_layout(&root, view_constraints);
        crate::render::flush_layout();
        let laid_out = FrameTimings::now();

        let tree = compose_frame(
            physical_width,
            physical_height,
            metrics.device_pixel_ratio,
            context.size,
            background,
            |paint_context| {
                // The dirty boundaries first, into the layers they kept --
                // upstream's `flushPaint`, before the frame walk it feeds. A
                // boundary that is dirty under a clean one is re-recorded here
                // or not at all: the walk never enters a subtree a clean
                // boundary hands back as a kept layer.
                crate::render::flush_paint(paint_context);
                root.paint(paint_context, Offset::ZERO)
            },
        );

        // A walk of its own over the laid-out tree, the way upstream's
        // `flushSemantics` is its own walk. It runs after the paint only
        // because that is the tidier place to read it; it needs the layout, not
        // the drawing. Most frames it does nothing: nobody is reading, or
        // nothing marked itself, or the walk came out the same as the tree the
        // platform is already holding. See the three gates in
        // [`crate::semantics`].
        if let Some(nodes) = crate::semantics::flush(context.size, &root) {
            self.send_semantics(view_id, &nodes);
        }
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
            unsafe {
                render(
                    self.host.user_data,
                    view_id,
                    raw,
                    metrics.device_pixel_ratio,
                )
            };
        }

        // Keep the laid-out tree for hit testing until the next frame replaces
        // it. `root` has been through layout and paint, so its geometry is the
        // geometry the user is looking at.
        self.painted.insert(view_id, root);
    }

    /// Hands one frame's semantics tree to the shell.
    ///
    /// Everything crosses in one call and borrowed: the C side copies what it
    /// wants before returning, which is what upstream's `SemanticsUpdate` does
    /// too. The `CString`s are held in a vector for exactly as long as the
    /// pointers into them are on the other side of the call.
    fn send_semantics(&self, view_id: i64, nodes: &[crate::semantics::SemanticsNode]) {
        let Some(update) = self.host.update_semantics else {
            return;
        };

        // A string the framework knows cannot contain a NUL, since it came
        // from Rust `String`s -- but an interior NUL would truncate a label
        // rather than crash, so it is replaced instead of unwrapped.
        let owned =
            |text: &str| std::ffi::CString::new(text.replace('\0', " ")).unwrap_or_default();

        let mut strings: Vec<std::ffi::CString> = Vec::with_capacity(nodes.len() * 5);
        let mut children: Vec<Vec<i32>> = Vec::with_capacity(nodes.len());
        for node in nodes {
            strings.push(owned(&node.properties.label));
            strings.push(owned(&node.properties.value));
            strings.push(owned(&node.properties.hint));
            strings.push(owned(&node.properties.increased_value));
            strings.push(owned(&node.properties.decreased_value));
            children.push(node.children.clone());
        }

        let raw: Vec<RfSemanticsNode> = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let base = index * 5;
                RfSemanticsNode {
                    id: node.id,
                    flags: pack_semantics_flags(&node.properties.flags),
                    actions: node.properties.actions,
                    left: node.left,
                    top: node.top,
                    right: node.right,
                    bottom: node.bottom,
                    label: strings[base].as_ptr(),
                    value: strings[base + 1].as_ptr(),
                    hint: strings[base + 2].as_ptr(),
                    increased_value: strings[base + 3].as_ptr(),
                    decreased_value: strings[base + 4].as_ptr(),
                    scroll_position: node.properties.scroll_position as f64,
                    scroll_extent_min: node.properties.scroll_extent_min as f64,
                    scroll_extent_max: node.properties.scroll_extent_max as f64,
                    children: children[index].as_ptr(),
                    child_count: children[index].len(),
                    text_direction: pack_text_direction(node.properties.text_direction),
                }
            })
            .collect();

        unsafe { update(self.host.user_data, view_id, raw.as_ptr(), raw.len()) };
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
        let handled = router.dispatch(root, event);
        // A press that could still become a long press, or a tap that could
        // still become a double one, is waiting for the clock rather than for
        // the finger. Nothing else would ask for the frame that moves it, and
        // a gesture that only fires when something else happens to redraw is
        // not a gesture.
        let waiting = router.awaits_deadline(event.time_stamp_micros);

        // A handler almost certainly called set_state, and a frame is the only
        // way that becomes visible.
        if handled || waiting {
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

        // The application first, then the focused widget, then Tab.
        //
        // Upstream's order, from `WidgetsBinding.handleKeyMessage`: the
        // application-wide handlers run before the focus walk, and the
        // traversal shortcuts are the last thing to see a key -- they are a
        // default rather than a rule, so an application that wants Tab for
        // something else takes it here and the focus tree never sees it.
        let handled = match self.application.as_mut() {
            Some(application) => application.on_key(event, &self.keyboard),
            None => false,
        };
        let handled = handled
            || crate::focus::dispatch_key(event)
            || crate::focus::handle_traversal_key(event, &self.keyboard);
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
    // Before the shell exists, which is the whole requirement: everything it
    // will call is in the table this installs.
    register_app_interface();

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

// -- What we give the shell ---------------------------------------------------

// Both live in `abi` below, beside the functions the table is made of and the
// two event structs its signatures name. They are the module's public face all
// the same, and `lib.rs` re-exports them from here.
#[cfg(not(test))]
pub use abi::{RfAppInterface, register_app_interface};

/// Telling the Android host where the application's entry point is.
///
/// Everywhere else the application owns `main` and calls `rustflutter_app_main`
/// itself. On Android the Activity owns the process and the host calls it, when
/// there is a Surface -- so the host has to be able to reach it, and reaching
/// it by name is a call out of the engine and up into whoever loaded it. See
/// `rf_set_app_main` in `rustflutter_host.h`.
///
/// The registration has to happen before anything asks, and there is no moment
/// between "loaded" and "asked" that the framework controls, so it goes in
/// `.init_array`: the ELF loader runs it while `System.loadLibrary` is still
/// returning. `#[used]` is what keeps it, since nothing references it.
///
/// `rustflutter_app_main` is the application's, not the framework's, and this
/// is where the framework depends on the application defining it. That is only
/// true on Android, where every application is a library the host starts; a
/// desktop binary that never defined one links fine because none of this is
/// compiled.
#[cfg(all(target_os = "android", not(test)))]
mod android_entry {
    use std::os::raw::{c_char, c_int};

    unsafe extern "C" {
        /// Defined by the application.
        fn rustflutter_app_main(argc: c_int, argv: *const *const c_char) -> c_int;

        /// Defined by the host, in `rustflutter_app_main.cc`.
        fn rf_set_app_main(app_main: unsafe extern "C" fn(c_int, *const *const c_char) -> c_int);
    }

    unsafe extern "C" fn register() {
        unsafe { rf_set_app_main(rustflutter_app_main) };
    }

    #[used]
    #[unsafe(link_section = ".init_array")]
    static REGISTER: unsafe extern "C" fn() = register;
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
        // Claimed before anything reaches the messenger or the image cache, so
        // that the very first stray call from another thread is the one that
        // fails rather than the tenth.
        crate::task::adopt_ui_thread();
        services::attach(sink);
        // On this thread, which is the UI thread and the only one that will
        // ever hold futures. `host` is copied into the poster, so a worker
        // waking a task later reaches the shell through the same pointer the
        // rest of this file uses.
        crate::task::attach(host.post_task, host.post_delayed_task, host.user_data);

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
        // After the messenger, never before: `services::detach` answers every
        // outstanding reply with `None`, and that is what settles the oneshot a
        // waiting task is parked on. Dropping the tasks first would take their
        // receivers with them and the answers would arrive nowhere.
        //
        // It also clears the poster under its lock, so that once this returns
        // no thread is inside `post_task` and none can enter -- which is what
        // makes the shell safe to tear down behind it.
        crate::task::detach();
        // Given up last: everything above is entitled to check it, and the next
        // application may well come up on a different thread.
        crate::task::release_ui_thread();
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
        let Some(instance) = instance(app) else {
            return;
        };
        if metrics.is_null() {
            return;
        }
        if instance
            .views
            .insert(view_id, unsafe { *metrics })
            .is_none()
        {
            instance.view_order.push(view_id);
        }
        instance.schedule_frame();
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_remove_view(app: *mut RfApp, view_id: i64) {
        let Some(instance) = instance(app) else {
            return;
        };
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
        let Some(instance) = instance(app) else {
            return;
        };
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
        let Some(instance) = instance(app) else {
            return;
        };
        if json.is_null() {
            return;
        }
        let bytes = unsafe { std::slice::from_raw_parts(json as *const u8, length) };
        // A settings payload that is not UTF-8 is a broken embedder, and the
        // right thing is to keep the settings we have rather than guess.
        let Ok(text) = std::str::from_utf8(bytes) else {
            return;
        };
        // A frame, because nothing else asks for one. The settings themselves
        // reach the tree through `MediaQueryData::from_view` and `publish`,
        // which happen *during* a frame -- so a reader who turns the system
        // font size up while the application is sitting still saw nothing
        // change until they touched the screen. `RuntimeController::
        // SetUserSettingsData` does not schedule one either; upstream reaches
        // it through `_MediaQueryFromViewState.didChangeTextScaleFactor`
        // calling `setState`, which this port has no counterpart for yet.
        //
        // Only on an actual change, which is what the return value is for: the
        // shell re-sends the whole settings object whenever any part of it
        // changes, and re-rendering for a payload that says nothing new is the
        // cost this guard exists to avoid.
        if platform::set_user_settings(text) {
            instance.schedule_frame();
        }
    }

    /// The preferred locales, four strings each: language, country, script,
    /// variant. See `rf_app_set_locales` in rust_app_api.h for the layout.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_set_locales(
        app: *mut RfApp,
        locales: *const *const c_char,
        count: usize,
    ) {
        let Some(instance) = instance(app) else {
            return;
        };
        if locales.is_null() {
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
        // A frame, for the same reason `rf_app_set_user_settings` asks for
        // one: the language reaches the tree through the root's
        // `Localizations` during a frame, and nothing else was going to ask
        // for one. Only on a change -- the shell re-sends the whole list.
        if platform::set_locales(parsed) {
            instance.schedule_frame();
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_begin_frame(
        app: *mut RfApp,
        frame_time_micros: i64,
        frame_number: u64,
    ) {
        let Some(instance) = instance(app) else {
            return;
        };
        instance.frame_time_micros = frame_time_micros;
        instance.frame_number = frame_number;
        // Copy the host out first: `application` borrows `instance` mutably,
        // and the scheduler needs the host at the same time.
        let scheduler = FrameScheduler {
            host: instance.host,
        };
        let started = FrameTimings::now();
        // Gestures that are decided by time rather than by movement -- a long
        // press, and a single tap waiting to find out whether it is the first
        // half of a double one -- are settled here. Upstream they are `Timer`s
        // on the platform thread; frames are on demand here, so a gesture with
        // a deadline keeps asking for the next frame until its deadline is
        // reached. A press that is not waiting for anything asks for nothing.
        if instance.router.tick(frame_time_micros) {
            scheduler.request_frame();
        }
        if let Some(application) = instance.application.as_mut() {
            application.begin_frame(&FrameContext {
                frame_number,
                frame_time_micros,
                scheduler,
            });
        }
        instance.advance_ms = FrameTimings::now().duration_since(started).as_secs_f64() * 1000.0;
    }

    /// Drains the framework's task queue. Upstream's `FlushMicrotasksNow`.
    ///
    /// Called from two places, both on the UI thread: once inside every frame,
    /// between the animation phase and the build phase, and once for each
    /// `RfAppHost::post_task` the framework asked for.
    ///
    /// A frame is asked for only when something actually ran. A task parked on
    /// a platform reply must not keep the engine drawing -- frames are on
    /// demand here, and waiting is not a reason to draw.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_run_tasks(app: *mut RfApp) {
        let Some(instance) = instance(app) else {
            return;
        };
        if crate::task::run_until_stalled() {
            instance.schedule_frame();
        }
    }

    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_draw_frame(app: *mut RfApp) {
        let Some(instance) = instance(app) else {
            return;
        };
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
        let Some(instance) = instance(app) else {
            return;
        };
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
    pub unsafe extern "C" fn rf_app_dispatch_key(app: *mut RfApp, raw: *const RfKeyEvent) -> bool {
        let Some(instance) = instance(app) else {
            return false;
        };
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
            if text.is_empty() {
                None
            } else {
                Some(text.into_owned())
            }
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

    /// Turns the semantics tree on or off.
    ///
    /// Upstream's `PlatformView::SetSemanticsEnabled`. Nothing is built while
    /// it is off: a tree nobody reads is a tree that would quietly rot.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_set_semantics_enabled(app: *mut RfApp, enabled: bool) {
        let Some(instance) = instance(app) else {
            return;
        };
        crate::semantics::set_enabled(enabled);
        // The tree only exists as a by-product of a frame, so turning it on
        // has to ask for one -- otherwise a reader gets nothing until
        // something else happens to change.
        instance.schedule_frame();
    }

    /// Delivers an action a screen reader asked for.
    #[unsafe(no_mangle)]
    pub unsafe extern "C" fn rf_app_dispatch_semantics_action(
        app: *mut RfApp,
        node_id: i32,
        action: i32,
    ) -> bool {
        let Some(instance) = instance(app) else {
            return false;
        };
        let Some(action) = crate::semantics::SemanticsAction::from_bits(action) else {
            return false;
        };
        // Asked of the tree that was painted, which is the one the reader is
        // looking at. The node id says which control but not which view, so
        // every painted tree is offered it -- ids are unique across all of
        // them, so at most one answers.
        let handled = instance
            .painted
            .values()
            .any(|root| crate::semantics::perform_action(root, node_id, action));
        if handled {
            // Whatever the handler did, the reader is owed the frame that
            // shows it -- and the fresh semantics tree that goes with it.
            instance.schedule_frame();
        }
        handled
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
        let Some(instance) = instance(app) else {
            return;
        };
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
        let Some(instance) = instance(app) else {
            return;
        };
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

    // -- Handing them over ----------------------------------------------------

    /// The mirror of `RfAppInterface` in `runtime/rust_app_api.h`: every
    /// `rf_app_*` function above, as a table.
    ///
    /// The shell does not call them by name. It could when the two halves are
    /// one executable, and that is how this started, but the call points the
    /// wrong way the moment they are not: out of the C++ engine and up into
    /// whatever module the framework was linked into. Nothing spells that
    /// portably -- Windows wants the executable to export and the DLL to import
    /// back, ELF wants `--export-dynamic` on whoever links last -- so the
    /// framework hands its functions down instead, exactly as the shell hands
    /// [`RfAppHost`] up, and both link modes then run the same code.
    ///
    /// The fields are in the order the header declares them, and the order is
    /// the ABI: a field inserted in the middle on one side only would call its
    /// neighbour with the wrong signature. The `size_of` check below and the
    /// matching `static_assert` in `runtime_controller.cc` catch a count that
    /// has drifted; nothing but a reader catches a swap.
    #[repr(C)]
    pub struct RfAppInterface {
        create: unsafe extern "C" fn(*const c_void) -> *mut RfApp,
        destroy: unsafe extern "C" fn(*mut RfApp),
        launch: unsafe extern "C" fn(*mut RfApp) -> c_int,
        add_view: unsafe extern "C" fn(*mut RfApp, i64, *const ViewMetrics),
        remove_view: unsafe extern "C" fn(*mut RfApp, i64),
        set_view_metrics: unsafe extern "C" fn(*mut RfApp, i64, *const ViewMetrics),
        set_user_settings: unsafe extern "C" fn(*mut RfApp, *const c_char, usize),
        set_locales: unsafe extern "C" fn(*mut RfApp, *const *const c_char, usize),
        begin_frame: unsafe extern "C" fn(*mut RfApp, i64, u64),
        draw_frame: unsafe extern "C" fn(*mut RfApp),
        run_tasks: unsafe extern "C" fn(*mut RfApp),
        dispatch_pointers: unsafe extern "C" fn(*mut RfApp, *const RfPointerEvent, usize),
        dispatch_key: unsafe extern "C" fn(*mut RfApp, *const RfKeyEvent) -> bool,
        dispatch_platform_message:
            unsafe extern "C" fn(*mut RfApp, *const c_char, *const u8, usize, i64),
        complete_platform_message_reply: unsafe extern "C" fn(*mut RfApp, i64, *const u8, usize),
        set_semantics_enabled: unsafe extern "C" fn(*mut RfApp, bool),
        dispatch_semantics_action: unsafe extern "C" fn(*mut RfApp, i32, i32) -> bool,
    }

    const _: () = assert!(
        size_of::<RfAppInterface>() == size_of::<*mut c_void>() * 17,
        "RfAppInterface has drifted from rust_app_api.h"
    );

    unsafe extern "C" {
        /// Takes the table, in the engine. Not copied: `INTERFACE` is `static`,
        /// so it outlives every shell that will read it.
        fn rf_set_app_interface(app_interface: *const RfAppInterface);
    }

    /// Installs the table, so the shell can reach the framework.
    ///
    /// [`run`](super::run) does this on the way to the host and nothing else
    /// has to. It is public for the embedder that starts a shell some other way
    /// -- the Android host reaches `rustflutter_app_main` and the application
    /// calls `run` from there, but an embedder that owns its own shell would
    /// not -- and it is idempotent, because it writes one pointer.
    ///
    /// Must be called before the shell starts. After that the first thing the
    /// shell does is ask for the table, and one that finds none stops there.
    pub fn register_app_interface() {
        unsafe { rf_set_app_interface(&INTERFACE) };
    }

    /// The seventeen functions above, in the order `rust_app_api.h` declares
    /// them. Read the two together; a line out of place here is a call landing
    /// on the wrong signature with nothing to catch it.
    static INTERFACE: RfAppInterface = RfAppInterface {
        create: rf_app_create,
        destroy: rf_app_destroy,
        launch: rf_app_launch,
        add_view: rf_app_add_view,
        remove_view: rf_app_remove_view,
        set_view_metrics: rf_app_set_view_metrics,
        set_user_settings: rf_app_set_user_settings,
        set_locales: rf_app_set_locales,
        begin_frame: rf_app_begin_frame,
        draw_frame: rf_app_draw_frame,
        run_tasks: rf_app_run_tasks,
        dispatch_pointers: rf_app_dispatch_pointers,
        dispatch_key: rf_app_dispatch_key,
        dispatch_platform_message: rf_app_dispatch_platform_message,
        complete_platform_message_reply: rf_app_complete_platform_message_reply,
        set_semantics_enabled: rf_app_set_semantics_enabled,
        dispatch_semantics_action: rf_app_dispatch_semantics_action,
    };
}

#[cfg(test)]
mod tests {
    use super::{compose_frame, pack_text_direction};
    use crate::direction::TextDirection;
    use crate::engine::Color;
    use crate::engine_test_stubs::{Drawn, drawn, reset_drawn};
    use crate::render::Size;

    const BACKGROUND: Color = Color(0xff101418);
    const MARK: Color = Color(0xffcc0000);

    /// Composes a frame whose application paints one rectangle, and hands back
    /// what the canvas was told.
    fn frame(device_pixel_ratio: f64) -> Vec<Drawn> {
        reset_drawn();
        let _tree = compose_frame(
            800,
            600,
            device_pixel_ratio,
            Size::new(400.0, 300.0),
            BACKGROUND,
            |context| {
                context.canvas().draw_rect(
                    crate::engine::Rect::xywh(0.0, 0.0, 10.0, 10.0),
                    &crate::engine::Paint::new(MARK),
                );
            },
        );
        drawn()
    }

    #[test]
    fn the_background_goes_down_before_the_application_paints() {
        // `draw_color` recorded nothing until this tick, so this -- the one
        // rule the call has -- was not a claim any test could make. Painted
        // after instead, the background covers the whole application; not
        // painted at all, whatever the last frame left behind shows through.
        let calls = frame(2.0);
        assert_eq!(
            calls.first(),
            Some(&Drawn::Color { argb: BACKGROUND.0 }),
            "{calls:?}"
        );
        assert!(
            calls
                .iter()
                .any(|call| matches!(call, Drawn::Rect { argb, .. } if *argb == MARK.0)),
            "and the application painted after it: {calls:?}"
        );
    }

    #[test]
    fn the_background_is_filled_once_whatever_the_device_pixel_ratio() {
        // A scale of exactly one skips the transform layer -- "pushing an
        // identity would cost a layer to say nothing" -- and the background
        // must not be skipped with it.
        for dpr in [1.0, 2.0, 3.0] {
            let calls = frame(dpr);
            let fills = calls
                .iter()
                .filter(|call| matches!(call, Drawn::Color { .. }))
                .count();
            assert_eq!(fills, 1, "at {dpr}: {calls:?}");
        }
    }

    #[test]
    fn reading_directions_cross_in_the_embedders_encoding() {
        // The embedder's `FlutterTextDirection` (embedder.h) and the engine's
        // `SemanticsNode::textDirection` (semantics_node.h) agree on
        // 0 = unknown, 1 = rtl, 2 = ltr, and the ABI has to speak both.
        assert_eq!(pack_text_direction(Some(TextDirection::Rtl)), 1);
        assert_eq!(pack_text_direction(Some(TextDirection::Ltr)), 2);
        assert_eq!(
            pack_text_direction(None),
            0,
            "nothing to read is unknown, not guessed"
        );
    }
}
