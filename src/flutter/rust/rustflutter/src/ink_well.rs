// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Upstream `material/ink_well.dart`'s two widgets: [`InkResponse`], the area
//! of a material that answers a touch, and [`InkWell`], its rectangular
//! variant.
//!
//! The features themselves -- the splash, the ripple, the highlight -- are in
//! [`crate::ink`]. This is what decides *when* one is made and *which*, which
//! is the whole of the class: a splash on press, three highlights that come
//! and go with pressed, hover and focus, and the callbacks that fire as each
//! turns on and off.
//!
//! # Two highlights and a splash, and why they are not the same thing
//!
//! A splash is an event: it marks the moment of contact, travels, and is gone
//! whatever happens next. A highlight is a state: it is on while the pointer
//! is over, or held, or the control has focus, and it stays on until that
//! stops being true. So there is at most one *current* splash but a whole set
//! of them in flight at once -- a second press starts a second splash and
//! [cancels](crate::ink::InteractiveInkFeature::cancel) the first rather than
//! removing it -- while there is exactly one highlight per kind, reactivated
//! rather than replaced.
//!
//! # What is not ported, each with upstream's reason
//!
//! * **The nested-response registry** (`_ParentInkResponseProvider`,
//!   `markChildInkResponsePressed`, `_anyChildInkResponsePressed`). It exists
//!   so that an `InkWell` inside another does not splash both: the inner one
//!   tells the outer it is pressed, and the outer declines. It needs an
//!   inherited value that a descendant *writes* to, which this crate's
//!   `provide`/`inherited` pair does not do -- see [`crate::framework`].
//! * **`wantKeepAlive`**. Upstream keeps a scrolled-away response alive while
//!   it still has ink in flight, so a splash does not vanish mid-animation
//!   when the row leaves the window. This crate has no keep-alive at all (the
//!   same gap [`crate::grid`] records), and a row that leaves the window
//!   takes its splash with it.
//! * **Focus, actions and the focus-highlight mode.** Upstream's
//!   `handleFocusHighlightModeChange` hides the focus highlight entirely when
//!   the `FocusManager` is in touch mode, because a focus ring on a
//!   touchscreen means nothing. The focus highlight here is driven by
//!   [`InkResponseState::update_highlight`] like the other two; whoever owns
//!   focus calls it.
//! * **`statesController` and `overlayColor`.** A resolved overlay colour
//!   would replace all three of `highlightColor`/`hoverColor`/`focusColor` at
//!   once; the three are here and the property that overrides them is not,
//!   pending [`crate::widget_state`] reaching this control.

use std::cell::Cell;
use std::rc::Rc;

use crate::components::theme_of;
use crate::engine::Color;
use crate::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent, single, stateful};
use crate::gestures::PointerHandlers;
use crate::ink::{
    InkFeatureKind, InkHighlight, InkHighlightShape, InteractiveInkFeature,
    InteractiveInkFeatureFactory,
};
use crate::render::{Offset, Size, StackPosition};

/// Which of the three states a highlight is showing. Upstream's private
/// `_HighlightType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightType {
    Pressed,
    Hover,
    Focus,
}

impl HighlightType {
    /// Upstream's `getFadeDurationForType`.
    ///
    /// The pressed highlight fades over 200ms and the other two over 50.
    /// Press is an action the reader took and can watch; hover and focus
    /// follow a pointer or a tab key that is already somewhere else by the
    /// time a 200ms fade would finish, so they have to keep up.
    pub fn fade_micros(self, hover_micros: Option<i64>) -> i64 {
        match self {
            HighlightType::Pressed => 200_000,
            HighlightType::Hover | HighlightType::Focus => hover_micros.unwrap_or(50_000),
        }
    }

    fn index(self) -> usize {
        match self {
            HighlightType::Pressed => 0,
            HighlightType::Hover => 1,
            HighlightType::Focus => 2,
        }
    }

    pub const ALL: [HighlightType; 3] = [
        HighlightType::Pressed,
        HighlightType::Hover,
        HighlightType::Focus,
    ];
}

/// What an [`InkResponse`] remembers between frames: the ink in flight.
#[derive(Default)]
pub struct InkResponseState {
    /// Upstream's `_splashes`, a set. Every splash that has not finished
    /// fading, not only the current one -- a quick double press leaves two on
    /// the screen at once, and dropping the first would be a flicker.
    splashes: Vec<(u64, InteractiveInkFeature)>,
    /// Upstream's `_currentSplash`: the one a tap would confirm. `None` once
    /// it has been confirmed or cancelled, which is why confirming twice
    /// cannot happen.
    current: Option<u64>,
    next_serial: u64,
    /// Upstream's `_highlights` map, one slot per [`HighlightType`].
    highlights: [Option<InteractiveInkFeature>; 3],
    /// Upstream's `_hovering`.
    pub hovering: bool,
    /// Upstream's `_hasFocus`.
    pub has_focus: bool,
    /// The region's size, filled in at layout so the next splash knows how
    /// far it has to reach -- the same arrangement, and the same one-frame
    /// caveat, as [`crate::ink::Ink`].
    size: Rc<Cell<Size>>,
    now_micros: i64,
}

impl InkResponseState {
    /// Upstream's `_startNewSplash`.
    ///
    /// The previous current splash is **cancelled, not removed**: it carries
    /// on fading where it is while the new one grows. A press that arrives
    /// during another press is two marks on the surface, because two things
    /// happened.
    pub fn start_splash(&mut self, splash: InteractiveInkFeature) -> u64 {
        let at = splash.phase.started_micros;
        if let Some(current) = self.take_current() {
            current.cancel(at);
        }
        let serial = self.next_serial;
        self.next_serial += 1;
        self.splashes.push((serial, splash));
        self.current = Some(serial);
        serial
    }

    fn take_current(&mut self) -> Option<&mut InteractiveInkFeature> {
        let current = self.current.take()?;
        self.splashes
            .iter_mut()
            .find(|(serial, _)| *serial == current)
            .map(|(_, splash)| splash)
    }

    /// Upstream's `handleTap`, first line: the current splash is confirmed
    /// and forgotten. Answers whether there was one.
    pub fn confirm_splash(&mut self, at_micros: i64) -> bool {
        match self.take_current() {
            Some(splash) => {
                splash.confirm(at_micros);
                true
            }
            None => false,
        }
    }

    /// Upstream's `handleTapCancel`, same shape.
    pub fn cancel_splash(&mut self, at_micros: i64) -> bool {
        match self.take_current() {
            Some(splash) => {
                splash.cancel(at_micros);
                true
            }
            None => false,
        }
    }

    /// Upstream's `updateHighlight`, minus the callbacks -- which the caller
    /// fires, because upstream fires them *after* the highlight has changed
    /// and only when it did.
    ///
    /// The early return is the part that matters: `value == (highlight !=
    /// null && highlight.active)` means asking for a state the highlight is
    /// already in changes nothing and reports nothing. Without it, a mouse
    /// sitting still over a control would call `onHover(true)` on every frame
    /// that re-read the pointer.
    ///
    /// Answers whether anything changed, so the caller knows whether to fire.
    pub fn update_highlight(
        &mut self,
        kind: HighlightType,
        value: bool,
        at_micros: i64,
        make: impl FnOnce() -> InteractiveInkFeature,
    ) -> bool {
        let slot = &mut self.highlights[kind.index()];
        let showing = slot.as_ref().is_some_and(|highlight| highlight.active());
        if value == showing {
            return false;
        }
        match (value, slot.as_mut()) {
            (true, Some(highlight)) => highlight.activate(at_micros),
            (true, None) => *slot = Some(make()),
            (false, Some(highlight)) => highlight.cancel(at_micros),
            // Asked to turn off a highlight that was never made. The early
            // return above has already handled it -- `showing` is false and
            // so is `value` -- so this is unreachable rather than a no-op.
            (false, None) => unreachable!("the early return covers this"),
        }
        true
    }

    pub fn highlight(&self, kind: HighlightType) -> Option<&InteractiveInkFeature> {
        self.highlights[kind.index()].as_ref()
    }

    /// Whether any highlight is present at all. Upstream's `highlightsExist`,
    /// which is half of what it keeps a scrolled-away response alive for.
    pub fn highlights_exist(&self) -> bool {
        self.highlights.iter().any(|slot| slot.is_some())
    }

    pub fn splash_count(&self) -> usize {
        self.splashes.len()
    }

    /// Everything still worth painting, splashes first and highlights over
    /// them -- upstream's order, since the material walks its feature list in
    /// the order they were added and a highlight added on press comes after
    /// the splash that press started.
    pub fn features(&self) -> impl Iterator<Item = &InteractiveInkFeature> {
        self.splashes
            .iter()
            .map(|(_, splash)| splash)
            .chain(self.highlights.iter().flatten())
    }

    /// Moves the clock, and drops whatever has finished. Answers whether the
    /// frame changed anything, which is what a per-frame `advance` owes.
    pub fn advance(&mut self, now_micros: i64) -> bool {
        if self.splashes.is_empty() && !self.highlights_exist() {
            return false;
        }
        self.now_micros = now_micros;
        for (_, splash) in self.splashes.iter_mut() {
            splash.advance(now_micros);
        }
        for slot in self.highlights.iter_mut().flatten() {
            slot.advance(now_micros);
        }
        let current = self.current;
        self.splashes.retain(|(serial, splash)| {
            // The current splash is never dropped for being finished: it has
            // not settled, so it is not finished, and a press held for longer
            // than the grow still has to be there when the finger lifts.
            splash.alive() || Some(*serial) == current
        });
        for slot in self.highlights.iter_mut() {
            if slot.as_ref().is_some_and(|highlight| !highlight.alive()) {
                *slot = None;
            }
        }
        true
    }
}

/// Upstream `InkResponse`: an area of a material that answers a touch with
/// ink.
///
/// The circular one. [`InkWell`] is the same thing with a rectangular
/// highlight clipped to its box, which is the difference upstream draws
/// between the two and the only one.
pub struct InkResponse {
    id: u64,
    /// A *builder* rather than a widget, for the reason [`crate::ink::Ink`]
    /// gives: a stateful component is rebuilt from the same widget instance
    /// every time its own state changes, so a child stored as a widget would
    /// be handed over on the first build and gone on the second.
    build_child: Box<dyn Fn() -> AnyWidget>,
    on_tap: Option<Rc<dyn Fn()>>,
    on_tap_cancel: Option<Rc<dyn Fn()>>,
    /// Upstream's `onHighlightChanged`, which fires for the *pressed*
    /// highlight only -- not for hover or focus, which have `onHover` and
    /// `onFocusChange`.
    on_highlight_changed: Option<Rc<dyn Fn(bool)>>,
    on_hover: Option<Rc<dyn Fn(bool)>>,
    pub contained_ink_well: bool,
    pub highlight_shape: InkHighlightShape,
    radius: Option<f32>,
    splash_color: Option<Color>,
    highlight_color: Option<Color>,
    hover_color: Option<Color>,
    focus_color: Option<Color>,
    /// Upstream's `onFocusChange`, told when the keyboard arrives or leaves.
    on_focus_change: Option<Rc<dyn Fn(bool)>>,
    /// The focus node this response is, when it is one -- upstream's
    /// `focusNode`/`canRequestFocus`.
    ///
    /// # Why it has to be named rather than made up
    ///
    /// Upstream's default is `canRequestFocus: true` with an internal node the
    /// widget creates for itself. This crate's focus registry is keyed by
    /// caller-chosen ids, so an internal node would need an id out of thin air
    /// -- and a well already shares its `id` with whatever it wraps, which for
    /// a search bar is a text field that has a focus node of its own. Two
    /// nodes under one id is worse than none.
    ///
    /// So focus is opted into with an id the caller owns. `None` is a well
    /// that cannot be reached by the keyboard, which is right for a well
    /// wrapped around something else focusable and wrong for one that *is* the
    /// control.
    focus_id: Option<u64>,
    splash_factory: Option<InteractiveInkFeatureFactory>,
    hover_micros: Option<i64>,
    enabled: bool,
    /// Upstream's `getRectCallback`: the rectangle the ink is measured
    /// against, instead of the region's own bounds.
    ///
    /// It exists for exactly one thing upstream, and that thing explains it:
    /// [`crate::components::TableRowInkWell`] hands back the *row*, so a
    /// press in one cell splashes across the whole row. Without the hook a
    /// splash would fill the cell and stop at its edge, which would say the
    /// cell was pressed when the row was.
    ///
    /// Given the size the region was actually laid out at, and answering a
    /// rectangle in the region's own coordinates.
    #[allow(clippy::type_complexity)]
    rect_callback: Option<Rc<dyn Fn(Size) -> crate::engine::Rect>>,
    /// Upstream's `borderRadius`, and it shapes the **clip** rather than
    /// anything drawn.
    ///
    /// See [`InkResponse::with_border_radius`].
    border_radius: Option<crate::borders::BorderRadius>,
    /// Upstream's `customBorder`, which wins over `borderRadius` when both are
    /// given -- the order in `paintInkCircle` says so, and it is the right way
    /// round: a caller who named a whole shape has said more than one who
    /// named four corners.
    custom_border: Option<crate::borders::ShapeBorder>,
}

impl InkResponse {
    /// Upstream's `InkResponse.borderRadius`: the rounding of the clip a
    /// contained splash is held inside.
    ///
    /// Upstream clips the splash itself, in `paintInkCircle`:
    ///
    /// ```dart
    /// if (customBorder != null) {
    ///   canvas.clipPath(customBorder.getOuterPath(rect, textDirection: textDirection));
    /// } else if (borderRadius != BorderRadius.zero) {
    ///   canvas.clipRRect(RRect.fromRectAndCorners(rect, ...));
    /// } else {
    ///   canvas.clipRect(rect);
    /// }
    /// ```
    ///
    /// Three branches, and this had only the third -- the same gap
    /// [`crate::ink::Ink`] had, in the class `InkWell` is actually built
    /// from. A rounded card or a pill-shaped response showed the ripple
    /// filling the corners of its **bounding rectangle**, square wedges of
    /// colour outside the shape, growing with the ripple.
    pub fn with_border_radius(mut self, radius: crate::borders::BorderRadius) -> Self {
        self.border_radius = Some(radius);
        self
    }

    /// Upstream's `InkResponse.customBorder`, which takes precedence over
    /// [`InkResponse::with_border_radius`].
    pub fn with_custom_border(mut self, border: crate::borders::ShapeBorder) -> Self {
        self.custom_border = Some(border);
        self
    }

    pub fn new(id: u64, build_child: impl Fn() -> AnyWidget + 'static) -> InkResponse {
        InkResponse {
            id,
            build_child: Box::new(build_child),
            on_tap: None,
            on_tap_cancel: None,
            on_highlight_changed: None,
            on_hover: None,
            contained_ink_well: false,
            border_radius: None,
            custom_border: None,
            // Upstream's default, and the reason `InkWell` exists to change
            // it: a response with no box to fill is a circle.
            highlight_shape: InkHighlightShape::Circle,
            radius: None,
            splash_color: None,
            highlight_color: None,
            hover_color: None,
            focus_color: None,
            on_focus_change: None,
            focus_id: None,
            splash_factory: None,
            hover_micros: None,
            enabled: true,
            rect_callback: None,
        }
    }

    /// See [`InkResponse::rect_callback`].
    pub fn with_rect(mut self, rect: impl Fn(Size) -> crate::engine::Rect + 'static) -> Self {
        self.rect_callback = Some(Rc::new(rect));
        self
    }

    pub fn with_on_tap(mut self, on_tap: impl Fn() + 'static) -> Self {
        self.on_tap = Some(Rc::new(on_tap));
        self
    }

    pub fn with_on_tap_cancel(mut self, on_tap_cancel: impl Fn() + 'static) -> Self {
        self.on_tap_cancel = Some(Rc::new(on_tap_cancel));
        self
    }

    pub fn with_on_highlight_changed(mut self, on_change: impl Fn(bool) + 'static) -> Self {
        self.on_highlight_changed = Some(Rc::new(on_change));
        self
    }

    pub fn with_on_hover(mut self, on_hover: impl Fn(bool) + 'static) -> Self {
        self.on_hover = Some(Rc::new(on_hover));
        self
    }

    pub fn with_contained(mut self, contained: bool) -> Self {
        self.contained_ink_well = contained;
        self
    }

    pub fn with_highlight_shape(mut self, shape: InkHighlightShape) -> Self {
        self.highlight_shape = shape;
        self
    }

    pub fn with_radius(mut self, radius: f32) -> Self {
        self.radius = Some(radius);
        self
    }

    pub fn with_splash_color(mut self, color: Color) -> Self {
        self.splash_color = Some(color);
        self
    }

    pub fn with_highlight_color(mut self, color: Color) -> Self {
        self.highlight_color = Some(color);
        self
    }

    pub fn with_hover_color(mut self, color: Color) -> Self {
        self.hover_color = Some(color);
        self
    }

    pub fn with_focus_color(mut self, color: Color) -> Self {
        self.focus_color = Some(color);
        self
    }

    /// Upstream's `onFocusChange`. Only ever called for a response that is a
    /// focus node -- see [`InkResponse::with_focus`].
    pub fn with_on_focus_change(mut self, handler: impl Fn(bool) + 'static) -> Self {
        self.on_focus_change = Some(Rc::new(handler));
        self
    }

    /// Makes this response a focus node, so that the keyboard can reach it and
    /// it says so. See [`InkResponse::focus_id`].
    pub fn with_focus(mut self, focus_id: u64) -> Self {
        self.focus_id = Some(focus_id);
        self
    }

    pub fn with_splash_factory(mut self, factory: InteractiveInkFeatureFactory) -> Self {
        self.splash_factory = Some(factory);
        self
    }

    /// Upstream's `hoverDuration`, which applies to the focus highlight too.
    pub fn with_hover_micros(mut self, micros: i64) -> Self {
        self.hover_micros = Some(micros);
        self
    }

    /// Upstream's `enabled`, which it derives from whether any callback was
    /// given. Kept explicit here because a control with no `onTap` but a
    /// `onHighlightChanged` is a real thing.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The colour a highlight of `kind` paints with.
    ///
    /// **A disabled response still makes its highlights, at alpha zero.**
    /// That is upstream's `enabled ? resolved : resolved.withAlpha(0)`, and
    /// it looks wasteful until the lifecycle is the reason: the highlight has
    /// to exist so that becoming enabled again is a colour change rather than
    /// a highlight appearing from nowhere half way through a hover.
    /// The colour one of the three highlights is drawn in.
    ///
    /// `material` is the fallback, and it is the *theme's* answer rather than
    /// one invented here: see `ThemeData::overlay_color`. This used to fall
    /// back to the primary at 8% for all three states, which is upstream's
    /// value for none of them and disagreed with what `focus::StateHighlight`
    /// paints for the same three states.
    fn highlight_color_for(
        &self,
        kind: HighlightType,
        material: &crate::theme::ThemeData,
    ) -> Color {
        let resolved = match kind {
            HighlightType::Pressed => self.highlight_color,
            HighlightType::Hover => self.hover_color,
            HighlightType::Focus => self.focus_color,
        }
        .unwrap_or_else(|| material.overlay_color(kind));
        if self.enabled {
            resolved
        } else {
            resolved.with_alpha(0)
        }
    }
}

/// One highlight of `kind`, in `colour`. The shape and radius come from the
/// response; the fade duration from the kind, because press is watched and
/// hover is chased. Free rather than a method so the pointer handlers, which
/// outlive the build that made them, can hold it.
fn make_highlight(
    shape: InkHighlightShape,
    radius: Option<f32>,
    hover_micros: Option<i64>,
    kind: HighlightType,
    colour: Color,
    at_micros: i64,
) -> InteractiveInkFeature {
    let highlight = match shape {
        InkHighlightShape::Circle => InkHighlight::circular(radius),
        InkHighlightShape::Rectangle => InkHighlight::new(),
    }
    .with_fade_micros(kind.fade_micros(hover_micros));
    InteractiveInkFeature::new(InkFeatureKind::Highlight(highlight), colour, at_micros)
}

impl StatefulComponent for InkResponse {
    type State = InkResponseState;

    fn key(&self) -> crate::framework::Key {
        Some(self.id)
    }

    fn advance(&self, state: &mut InkResponseState, frame_time_micros: i64) -> bool {
        state.advance(frame_time_micros)
    }

    fn build(
        &self,
        state: &InkResponseState,
        handle: StateHandle<InkResponseState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let material = crate::theme::ThemeData::of(context);
        let splash_color = self.splash_color.unwrap_or(theme.primary.with_alpha(0x1f));
        let factory = self.splash_factory.unwrap_or_default();
        let contained = self.contained_ink_well;
        let child = (self.build_child)();

        // Everything the handlers need, captured rather than reached for: a
        // handler runs long after the build that made it.
        let pressed_colour = self.highlight_color_for(HighlightType::Pressed, &material);
        let hover_colour = self.highlight_color_for(HighlightType::Hover, &material);
        let shape = self.highlight_shape;
        let radius = self.radius;
        let hover_micros = self.hover_micros;
        let make = move |kind: HighlightType, colour: Color, at: i64| {
            make_highlight(shape, radius, hover_micros, kind, colour, at)
        };

        let size_sink = Rc::clone(&state.size);
        let on_highlight_changed = self.on_highlight_changed.clone();
        let on_hover = self.on_hover.clone();
        let on_tap = self.on_tap.clone();
        let on_tap_cancel = self.on_tap_cancel.clone();
        let enabled = self.enabled;

        let handlers = {
            let down_handle = handle.clone();
            let down_sink = Rc::clone(&size_sink);
            let down_rect = self.rect_callback.clone();
            let down_changed = on_highlight_changed.clone();
            let up_handle = handle.clone();
            let cancel_handle = handle.clone();
            let hover_handle = handle.clone();

            PointerHandlers::new()
                .with_pointer_down(move |event| {
                    if !enabled {
                        return;
                    }
                    let at = event.local_position;
                    let now = event.time_stamp_micros;
                    // Upstream measures the splash against
                    // `getRectCallback`'s answer when there is one, so a table
                    // row's splash reaches the row rather than the cell -- and
                    // the touch point moves into that rectangle's frame with
                    // it, or the circle would grow from the wrong place.
                    let laid_out = down_sink.get();
                    let (size, at) = match &down_rect {
                        Some(rect) => {
                            let rect = rect(laid_out);
                            (
                                Size::new(rect.width(), rect.height()),
                                Offset::new(at.dx - rect.left, at.dy - rect.top),
                            )
                        }
                        None => (laid_out, at),
                    };
                    let changed = down_changed.clone();
                    down_handle.set_state(move |state| {
                        state.start_splash(factory.create(size, at, splash_color, contained, now));
                        if state.update_highlight(HighlightType::Pressed, true, now, || {
                            make(HighlightType::Pressed, pressed_colour, now)
                        }) {
                            if let Some(changed) = &changed {
                                changed(true);
                            }
                        }
                    });
                })
                .with_pointer_up({
                    let changed = on_highlight_changed.clone();
                    let tap = on_tap.clone();
                    move |event| {
                        let now = event.time_stamp_micros;
                        let changed = changed.clone();
                        let tap = tap.clone();
                        up_handle.set_state(move |state| {
                            // Upstream's `handleTap`: confirm, forget, drop
                            // the pressed highlight, *then* call back.
                            let had = state.confirm_splash(now);
                            if state.update_highlight(HighlightType::Pressed, false, now, || {
                                unreachable!("turning a highlight off never makes one")
                            }) {
                                if let Some(changed) = &changed {
                                    changed(false);
                                }
                            }
                            if had {
                                if let Some(tap) = &tap {
                                    tap();
                                }
                            }
                        });
                    }
                })
                .with_pointer_cancel(move |event| {
                    let now = event.time_stamp_micros;
                    let changed = on_highlight_changed.clone();
                    let cancelled = on_tap_cancel.clone();
                    cancel_handle.set_state(move |state| {
                        let had = state.cancel_splash(now);
                        if state.update_highlight(HighlightType::Pressed, false, now, || {
                            unreachable!("turning a highlight off never makes one")
                        }) {
                            if let Some(changed) = &changed {
                                changed(false);
                            }
                        }
                        if had {
                            if let Some(cancelled) = &cancelled {
                                cancelled();
                            }
                        }
                    });
                })
                .with_hover_change(move |hovering| {
                    let on_hover = on_hover.clone();
                    hover_handle.set_state(move |state| {
                        state.hovering = hovering;
                        let now = state.now_micros;
                        if state.update_highlight(HighlightType::Hover, hovering, now, || {
                            make(HighlightType::Hover, hover_colour, now)
                        }) {
                            if let Some(on_hover) = &on_hover {
                                on_hover(hovering);
                            }
                        }
                    });
                })
        };

        // What to paint, decided here so the closure below holds values
        // rather than the state.
        let painted: Vec<(Offset, f32, Color)> = state
            .features()
            .filter_map(|feature| {
                let size = size_sink.get();
                let colour = feature.paint_color();
                if colour.alpha() == 0 || size.width <= 0.0 {
                    return None;
                }
                feature
                    .ink_circle(size)
                    .filter(|(_, radius)| *radius > 0.0)
                    .map(|(centre, radius)| (centre, radius, colour))
            })
            .collect();
        let id = self.id;
        let border_radius = self.border_radius;
        let custom_border = self.custom_border.clone();

        // Upstream's `Semantics(onTap: simulateTap)` in `_InkResponseState.build`,
        // and it is the whole reason a screen reader can press anything built
        // out of an ink well. Assembled here rather than at the call sites for
        // the reason [`crate::semantics::tappable`] gives: the two ways in must
        // not be able to disagree about what pressing does.
        //
        // **`simulateTap`, not `onTap`.** Upstream's handler is
        // `_startSplash(context: context); handleTap();` -- it puts a real
        // splash on the surface first. That is not decoration: a reader
        // activating a control is often sitting beside someone watching the
        // screen, and a press that fires the callback with nothing visible
        // happening looks like the press was lost.
        //
        // No `button` flag, because upstream's does not set one. An ink well is
        // how a thing is pressed, not what it is; a `Chip` or a `ListTile` says
        // what it is for itself, and a well that called everything a button
        // would put the word on rows and cards that are not.
        let semantics_tap = self.on_tap.clone().filter(|_| self.enabled).map(|on_tap| {
            let tap_handle = handle.clone();
            let tap_sink = Rc::clone(&size_sink);
            std::rc::Rc::new(move |_: crate::gestures::TapEvent| {
                let on_tap = on_tap.clone();
                let size = tap_sink.get();
                tap_handle.set_state(move |state| {
                    let now = state.now_micros;
                    // The centre, because a reader activated the control and
                    // not a point on it -- upstream's `_startSplash` with no
                    // details reaches for the same middle.
                    let centre = Offset::new(size.width / 2.0, size.height / 2.0);
                    state.start_splash(factory.create(size, centre, splash_color, contained, now));
                    state.confirm_splash(now);
                    on_tap();
                });
            }) as std::rc::Rc<dyn Fn(crate::gestures::TapEvent)>
        });
        let node = crate::semantics::node_id_for(self.id);

        // Upstream's `_handleFocusUpdate`: `updateHighlight(_HighlightType
        // .focus, value: hasFocus)`. The focus highlight had a colour, a fade
        // length and a slot in the state, and **nothing raised it** -- so a
        // menu line reached by Tab looked exactly like one nobody had reached.
        //
        // It is the same call the pointer makes for hover, with the same
        // guard, because being under the pointer and being under the keyboard
        // are the same kind of statement: neither is an action the reader
        // took, both have to keep up with something already moving.
        let child = match self.focus_id {
            None => child,
            Some(focus_id) => {
                let focus_handle = handle.clone();
                let focus_colour = self.highlight_color_for(HighlightType::Focus, &material);
                let on_focus_change = self.on_focus_change.clone();
                crate::framework::component(
                    crate::focus::Focus::new(focus_id, child).with_on_focus_change(
                        move |focused| {
                            let on_focus_change = on_focus_change.clone();
                            focus_handle.set_state(move |state| {
                                let now = state.now_micros;
                                if state.update_highlight(
                                    HighlightType::Focus,
                                    focused,
                                    now,
                                    || make(HighlightType::Focus, focus_colour, now),
                                ) {
                                    if let Some(on_focus_change) = &on_focus_change {
                                        on_focus_change(focused);
                                    }
                                }
                            });
                        },
                    ),
                )
            }
        };

        let responsive = single(child, move |child| {
            // **Passthrough**, not the stack's default of loose. Upstream
            // paints ink features from `_RenderInkFeatures`, a
            // `RenderProxyBox`, so the constraints a caller was given reach
            // its child untouched. Stacking features as real boxes is this
            // port's arrangement, and a loose stack drops a minimum size on
            // the way down -- which leaves the child painting narrower than
            // the box it was given and the difference showing through the
            // ink's own clip.
            // ClipBehavior::None: upstream has no stack here at all -- the
            // ink is painted on the `Material` beneath, and a circular
            // highlight or splash is *meant* to overflow the child's box
            // (an `IconButton`'s 35-radius circle on its 48-pixel target is
            // the everyday case). The default HardEdge clip cut that circle
            // down to a square the size of the button. A contained response
            // still gets its clip from the `RenderClipRect` below, which is
            // where upstream's three clip branches live.
            let mut stack = crate::render::RenderStack::new()
                .with_fit(crate::render::StackFit::Passthrough)
                .with_clip_behavior(crate::painting::ClipBehavior::None)
                .push_boxed(child);
            for (centre, radius, colour) in &painted {
                let circle = crate::widgets::Container::new()
                    .with_size(radius * 2.0, radius * 2.0)
                    .with_color(*colour)
                    .with_corner_radius(*radius);
                // Invisible to the pointer, for the reason `Ink` gives:
                // upstream a feature is not in the tree at all, and here it
                // is a real box stacked over the content.
                stack = stack.push_positioned(
                    crate::render::RenderIgnorePointer::new(circle),
                    StackPosition {
                        left: Some(centre.dx - radius),
                        top: Some(centre.dy - radius),
                        ..Default::default()
                    },
                );
            }
            let watched = crate::render::RenderSizeReporter::new(Rc::clone(&size_sink), stack);
            let region = crate::render::RenderPointerRegion::new(id, watched)
                .with_handlers(handlers.clone());
            if !contained {
                return crate::render::RenderRef::new(region);
            }
            // Upstream's three branches, in upstream's order.
            match (&custom_border, border_radius) {
                (Some(border), _) => {
                    let size = size_sink.get();
                    let path = border.outer_path(
                        crate::engine::Rect::xywh(0.0, 0.0, size.width, size.height),
                        crate::direction::current_direction(),
                    );
                    crate::render::RenderRef::new(crate::render::RenderClipPath::new(path, region))
                }
                (None, Some(radius)) => {
                    // Upstream's middle branch is `clipRRect`, not `clipPath`,
                    // so a radius that is one number on all four corners goes
                    // out as a rounded-rect clip. `with_border_radius` here
                    // builds a path, which is the right answer for four
                    // different corners and a heavier one for the common case.
                    let uniform = [
                        radius.top_left,
                        radius.top_right,
                        radius.bottom_left,
                        radius.bottom_right,
                    ];
                    let first = uniform[0];
                    let same = uniform
                        .iter()
                        .all(|corner| corner.x == first.x && corner.y == first.y)
                        && first.x == first.y;
                    let clip = crate::render::RenderClipRect::new(region);
                    crate::render::RenderRef::new(if same {
                        clip.with_corner_radius(first.x)
                    } else {
                        clip.with_border_radius(radius)
                    })
                }
                (None, None) => {
                    crate::render::RenderRef::new(crate::render::RenderClipRect::new(region))
                }
            }
        });

        // A well with nothing to run is left alone: an empty node saying a
        // reader may press something that does nothing is worse than saying
        // nothing at all, and upstream's `onTap` is null in exactly that case.
        let Some(tap) = semantics_tap else {
            return responsive;
        };
        // **A fold, not an annotation**, and the first version of this got it
        // wrong. An annotation says its own words, and a well has none: it is
        // handed a builder, not a string. That version produced two stops --
        // a blank one a reader could press and a "Delete" one they could not,
        // which is a worse place to stand than the silence it replaced.
        // Upstream needs no such choice because a non-container `Semantics`
        // merges into its child by configuration; folding is how that is said
        // here.
        //
        // The action bit is set as well as the handler, because the two are
        // separate: the handler answers a press, the bit is what a reader is
        // told they may do. With only the handler the control accepts a press
        // nobody knows to make.
        crate::framework::single(responsive, move |inner| {
            let tap = tap.clone();
            crate::render::RenderMergeSemanticsBox::new(inner)
                .with_properties(
                    crate::semantics::SemanticsProperties::label("")
                        .with_action(crate::semantics::SemanticsAction::Tap),
                )
                .with_action(node, move |action| {
                    if action == crate::semantics::SemanticsAction::Tap {
                        tap(crate::gestures::TapEvent {
                            local_position: Offset::ZERO,
                            position: Offset::ZERO,
                            pointer_id: 0,
                        });
                    }
                })
        })
    }
}

/// Upstream `InkWell`: the rectangular [`InkResponse`].
///
/// Upstream it is a subclass whose constructor passes `containedInkWell:
/// true` and `highlightShape: BoxShape.rectangle` and changes nothing else --
/// so here it is a facade that builds that `InkResponse`, the same shape
/// [`crate::widgets::Wrap`] and [`crate::overflow_bar::OverflowBar`] take.
///
/// The two settings go together and that is the point: a rectangular
/// highlight fills its box, so a response that did not clip to its box would
/// paint the highlight past its own edge.
pub struct InkWell;

impl InkWell {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(id: u64, build_child: impl Fn() -> AnyWidget + 'static) -> InkResponse {
        InkResponse::new(id, build_child)
            .with_contained(true)
            .with_highlight_shape(InkHighlightShape::Rectangle)
    }
}

/// [`InkResponse`] as a widget.
pub fn ink_response(id: u64, build_child: impl Fn() -> AnyWidget + 'static) -> AnyWidget {
    stateful(InkResponse::new(id, build_child))
}

/// [`InkWell`] as a widget.
pub fn ink_well(id: u64, build_child: impl Fn() -> AnyWidget + 'static) -> AnyWidget {
    stateful(InkWell::new(id, build_child))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ink::{InkRipple, InkSettlement};

    const MS: i64 = 1_000;
    const INK: Color = Color::argb(0x40, 0x33, 0x66, 0x99);

    fn splash_at(micros: i64) -> InteractiveInkFeature {
        InteractiveInkFeatureFactory::Ripple.create(
            Size::new(100.0, 100.0),
            Offset::new(50.0, 50.0),
            INK,
            true,
            micros,
        )
    }

    fn highlight_at(micros: i64) -> InteractiveInkFeature {
        InteractiveInkFeature::new(InkFeatureKind::Highlight(InkHighlight::new()), INK, micros)
    }

    #[test]
    fn a_second_press_cancels_the_first_splash_rather_than_removing_it() {
        // Two things happened, so there are two marks on the surface. The
        // first carries on fading where it is while the second grows.
        let mut state = InkResponseState::default();
        state.start_splash(splash_at(0));
        state.start_splash(splash_at(10 * MS));
        assert_eq!(state.splash_count(), 2);
        let first = state.features().next().expect("the first splash");
        assert_eq!(
            first.phase.settled,
            Some((10 * MS, InkSettlement::Cancelled))
        );
    }

    #[test]
    fn only_the_current_splash_is_confirmed_by_a_tap() {
        // The older one was already cancelled and must stay cancelled: a tap
        // does not retroactively confirm a press the reader abandoned.
        let mut state = InkResponseState::default();
        state.start_splash(splash_at(0));
        state.start_splash(splash_at(10 * MS));
        assert!(state.confirm_splash(20 * MS));
        let settlements: Vec<_> = state
            .features()
            .map(|feature| feature.phase.settled.map(|(_, how)| how))
            .collect();
        assert_eq!(
            settlements,
            vec![
                Some(InkSettlement::Cancelled),
                Some(InkSettlement::Confirmed)
            ]
        );
    }

    #[test]
    fn confirming_twice_cannot_happen_because_the_current_is_forgotten() {
        // Upstream sets `_currentSplash = null` in the same breath as
        // confirming it. Without that, a second up -- which the platform can
        // deliver -- would move the fade's start and let the splash outlive
        // its tap.
        let mut state = InkResponseState::default();
        state.start_splash(splash_at(0));
        assert!(state.confirm_splash(10 * MS));
        assert!(!state.confirm_splash(50 * MS), "there is no current splash");
        let only = state.features().next().expect("the splash");
        assert_eq!(
            only.phase.settled,
            Some((10 * MS, InkSettlement::Confirmed))
        );
    }

    #[test]
    fn asking_for_a_state_a_highlight_is_already_in_reports_nothing() {
        // The early return, and the reason it matters: a mouse sitting still
        // over a control would otherwise call onHover(true) every frame.
        let mut state = InkResponseState::default();
        assert!(state.update_highlight(HighlightType::Hover, true, 0, || highlight_at(0)));
        assert!(!state.update_highlight(HighlightType::Hover, true, MS, || highlight_at(MS)));
        assert!(state.update_highlight(HighlightType::Hover, false, 2 * MS, || unreachable!()));
        assert!(!state.update_highlight(HighlightType::Hover, false, 3 * MS, || unreachable!()));
    }

    #[test]
    fn a_highlight_that_comes_back_mid_fade_is_the_same_highlight() {
        // Upstream calls `activate()` on the existing one rather than making
        // a new one, so the alpha continues from where it had got to instead
        // of restarting from nothing.
        let mut state = InkResponseState::default();
        state.update_highlight(HighlightType::Hover, true, 0, || highlight_at(0));
        state.advance(InkHighlight::FADE_MICROS);
        assert_eq!(
            state.highlight(HighlightType::Hover).unwrap().opacity(),
            1.0
        );

        state.update_highlight(
            HighlightType::Hover,
            false,
            InkHighlight::FADE_MICROS,
            || unreachable!(),
        );
        state.advance(InkHighlight::FADE_MICROS + InkHighlight::FADE_MICROS / 2);
        let half_way_out = state.highlight(HighlightType::Hover).unwrap().opacity();
        assert!((half_way_out - 0.5).abs() < 0.01, "{half_way_out}");

        // Back again: it brightens from a half rather than from nothing.
        let now = InkHighlight::FADE_MICROS + InkHighlight::FADE_MICROS / 2;
        assert!(state.update_highlight(HighlightType::Hover, true, now, || unreachable!()));
        assert!((state.highlight(HighlightType::Hover).unwrap().opacity() - 0.5).abs() < 0.01);
    }

    #[test]
    fn the_pressed_highlight_fades_more_slowly_than_hover_and_focus() {
        // Press is an action the reader took and can watch; hover and focus
        // follow a pointer or a tab key that is already somewhere else.
        assert_eq!(HighlightType::Pressed.fade_micros(None), 200 * MS);
        assert_eq!(HighlightType::Hover.fade_micros(None), 50 * MS);
        assert_eq!(HighlightType::Focus.fade_micros(None), 50 * MS);
        // And the override reaches hover and focus but not press.
        assert_eq!(HighlightType::Pressed.fade_micros(Some(5 * MS)), 200 * MS);
        assert_eq!(HighlightType::Hover.fade_micros(Some(5 * MS)), 5 * MS);
    }

    #[test]
    fn the_current_splash_survives_a_press_held_past_its_own_animation() {
        // It has not settled, so it is not finished -- and it still has to be
        // there when the finger finally lifts.
        let mut state = InkResponseState::default();
        state.start_splash(splash_at(0));
        state.advance(10_000 * MS);
        assert_eq!(state.splash_count(), 1);
        assert!(state.confirm_splash(10_000 * MS), "still confirmable");
    }

    #[test]
    fn a_finished_splash_is_dropped_and_a_finished_highlight_with_it() {
        let mut state = InkResponseState::default();
        state.start_splash(splash_at(0));
        state.update_highlight(HighlightType::Pressed, true, 0, || highlight_at(0));
        state.confirm_splash(MS);
        state.update_highlight(HighlightType::Pressed, false, MS, || unreachable!());

        state.advance(MS + InkRipple::FADE_OUT_MICROS);
        assert_eq!(state.splash_count(), 0);
        assert!(!state.highlights_exist());
        // And with nothing left, a frame has nothing to say.
        assert!(!state.advance(MS + InkRipple::FADE_OUT_MICROS + MS));
    }

    #[test]
    fn an_ink_well_takes_its_overlay_colours_from_the_theme() {
        // Upstream falls back per state -- `highlightColor`, `focusColor`,
        // `hoverColor` -- and this used to answer all three with the primary
        // at 8%, a value upstream has for none of them. The three were also
        // what `focus::StateHighlight` already painted, so one crate gave two
        // answers to "what colour is a hover".
        let material = crate::theme::ThemeData::light();
        let plain = InkResponse::new(1, || crate::framework::leaf(|| crate::widgets::Empty));

        assert_eq!(
            plain.highlight_color_for(HighlightType::Hover, &material),
            material.hover_color
        );
        assert_eq!(
            plain.highlight_color_for(HighlightType::Focus, &material),
            material.focus_color
        );
        assert_eq!(
            plain.highlight_color_for(HighlightType::Pressed, &material),
            material.highlight_color
        );

        // The three are genuinely different, or the assertions above would
        // hold for any implementation that returned one colour for all of
        // them -- which is exactly what this used to do.
        assert_ne!(material.hover_color, material.focus_color);
        assert_ne!(material.focus_color, material.highlight_color);

        // A colour given explicitly still wins over the theme's.
        let told = InkResponse::new(1, || crate::framework::leaf(|| crate::widgets::Empty))
            .with_hover_color(INK);
        assert_eq!(
            told.highlight_color_for(HighlightType::Hover, &material),
            INK
        );
    }

    #[test]
    fn a_disabled_response_still_makes_its_highlights_at_alpha_zero() {
        // Upstream's `enabled ? resolved : resolved.withAlpha(0)`. It looks
        // wasteful until the lifecycle is the reason: the highlight has to
        // exist so that becoming enabled again is a colour change rather than
        // a highlight appearing from nowhere half way through a hover.
        let material = crate::theme::ThemeData::dark();
        let enabled = InkResponse::new(1, || crate::framework::leaf(|| crate::widgets::Empty))
            .with_hover_color(INK);
        let disabled = InkResponse::new(1, || crate::framework::leaf(|| crate::widgets::Empty))
            .with_hover_color(INK)
            .with_enabled(false);
        assert_eq!(
            enabled.highlight_color_for(HighlightType::Hover, &material),
            INK
        );
        assert_eq!(
            disabled
                .highlight_color_for(HighlightType::Hover, &material)
                .alpha(),
            0
        );
    }

    #[test]
    fn an_ink_well_is_a_contained_response_with_a_rectangular_highlight() {
        // The two go together: a rectangular highlight fills its box, so a
        // response that did not clip would paint it past its own edge.
        let well = InkWell::new(7, || crate::framework::leaf(|| crate::widgets::Empty));
        assert!(well.contained_ink_well);
        assert_eq!(well.highlight_shape, InkHighlightShape::Rectangle);

        // Where a bare response is neither.
        let response = InkResponse::new(7, || crate::framework::leaf(|| crate::widgets::Empty));
        assert!(!response.contained_ink_well);
        assert_eq!(response.highlight_shape, InkHighlightShape::Circle);
    }

    // -- The shape a contained response holds its ink inside ----------------

    /// Paints an `InkResponse` and hands back what the compositor was asked
    /// for.
    fn response_calls(
        build: impl FnOnce(InkResponse) -> InkResponse,
    ) -> Vec<crate::engine_test_stubs::Drawn> {
        use crate::framework::ElementTree;
        use crate::render::{BoxConstraints, RenderConstrainedBox};

        let response = build(
            InkResponse::new(9301, || {
                crate::framework::leaf(|| RenderConstrainedBox::tight(120.0, 48.0))
            })
            .with_contained(true),
        );
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::stateful(response));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(400.0, 200.0));
        crate::render::flush_layout();

        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    #[test]
    fn a_response_with_a_border_radius_is_clipped_round_and_one_without_is_not() {
        // Upstream's `paintInkCircle` clips the splash with `borderRadius`
        // when it has one and with the plain rect otherwise. This had only the
        // rect, in the class `InkWell` is built from -- so a rounded card
        // showed the ripple filling the corners of its bounding rectangle,
        // square wedges of colour outside the shape.
        let square = response_calls(|response| response);
        assert!(
            square
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::ClipRectLayer { .. })),
            "no radius, a square clip"
        );

        let rounded = response_calls(|response| {
            response.with_border_radius(crate::borders::BorderRadius::circular(12.0))
        });
        let radii: Vec<f32> = rounded
            .iter()
            .filter_map(|call| match call {
                crate::engine_test_stubs::Drawn::ClipRRectLayer { radius_x, .. } => Some(*radius_x),
                _ => None,
            })
            .collect();
        assert_eq!(radii, vec![12.0], "the radius it was given: {rounded:?}");
        assert!(
            !rounded
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::ClipRectLayer { .. })),
            "and nothing square"
        );
    }

    #[test]
    fn a_custom_border_wins_over_a_border_radius() {
        // Upstream's order: `customBorder` is tested first. A caller who named
        // a whole shape has said more than one who named four corners, and the
        // two are allowed to be given together.
        let calls = response_calls(|response| {
            response
                .with_border_radius(crate::borders::BorderRadius::circular(12.0))
                .with_custom_border(crate::borders::ShapeBorder::Circle(
                    crate::borders::CircleBorder::new(crate::borders::BorderSide::NONE, 0.0),
                ))
        });
        assert!(
            calls
                .iter()
                .any(|call| matches!(call, crate::engine_test_stubs::Drawn::ClipPathLayer { .. })),
            "the shape's own path: {calls:?}"
        );
        assert!(
            !calls.iter().any(|call| matches!(
                call,
                crate::engine_test_stubs::Drawn::ClipRRectLayer { .. }
                    | crate::engine_test_stubs::Drawn::ClipRectLayer { .. }
            )),
            "and neither of the two simpler clips: {calls:?}"
        );
    }

    #[test]
    fn an_uncontained_response_is_not_clipped_whatever_shape_it_was_given() {
        // `contained` is the switch; the shape is only what it clips to.
        let clipped = |calls: &[crate::engine_test_stubs::Drawn]| {
            calls.iter().any(|call| {
                matches!(
                    call,
                    crate::engine_test_stubs::Drawn::ClipRectLayer { .. }
                        | crate::engine_test_stubs::Drawn::ClipRRectLayer { .. }
                )
            })
        };
        assert!(clipped(&response_calls(|response| response
            .with_border_radius(crate::borders::BorderRadius::circular(
                12.0
            )))));
        assert!(!clipped(&response_calls(|response| response
            .with_contained(false)
            .with_border_radius(crate::borders::BorderRadius::circular(
                12.0
            )))));
    }

    #[test]
    fn a_response_hands_its_child_the_constraints_it_was_given() {
        // The same fix `Ink` needed: upstream paints ink features from a
        // `RenderProxyBox`, so constraints reach the child untouched, while a
        // stack loosens by default and drops a minimum on the way down.
        use crate::framework::ElementTree;
        use crate::render::{BoxConstraints, RenderConstrainedBox};

        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::stateful(InkResponse::new(9302, || {
            crate::framework::leaf(|| {
                crate::widgets::Container::new()
                    .with_color(INK)
                    .with_child(RenderConstrainedBox::tight(30.0, 10.0))
            })
        })));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::new(200.0, 400.0, 0.0, 200.0));
        crate::render::flush_layout();

        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context = crate::render::PaintContext::new(
                &mut layers,
                crate::render::Size::new(600.0, 400.0),
            );
            crate::render::RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        let filled = crate::engine_test_stubs::drawn()
            .into_iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::Rect { left, right, .. } => Some((left, right)),
                _ => None,
            })
            .expect("the child painted");
        assert_eq!(filled, (0.0, 200.0));
    }

    // -- What a reader who cannot see the splash is offered -----------------

    /// Mounts `widget` and hands back the tree, the root and what a screen
    /// reader would be given.
    fn mounted(
        widget: AnyWidget,
    ) -> (
        crate::framework::ElementTree,
        crate::render::BoxedRender,
        Vec<crate::semantics::SemanticsNode>,
    ) {
        use crate::framework::ElementTree;
        use crate::render::BoxConstraints;

        crate::semantics::set_enabled(true);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            widget,
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(&mut root, BoxConstraints::loose(400.0, 400.0));
        crate::semantics::mark_needs_update();
        let nodes = crate::semantics::flush(Size::new(400.0, 400.0), &root).unwrap_or_default();
        crate::semantics::set_enabled(false);
        (tree, root, nodes)
    }

    /// A well around the word "Delete".
    fn delete_well(on_tap: Option<Rc<dyn Fn()>>) -> InkResponse {
        let mut well = InkWell::new(9304, || {
            crate::framework::component(crate::components::Label::new("Delete"))
        });
        if let Some(on_tap) = on_tap {
            well = well.with_on_tap(move || on_tap());
        }
        well
    }

    fn well_saying_delete(
        on_tap: Option<Rc<dyn Fn()>>,
    ) -> (
        crate::framework::ElementTree,
        crate::render::BoxedRender,
        Vec<crate::semantics::SemanticsNode>,
    ) {
        mounted(crate::framework::stateful(delete_well(on_tap)))
    }

    #[test]
    fn a_well_with_something_to_do_is_one_stop_that_says_what_it_does() {
        // The gap this closed: an ink well published no action at all, so
        // every control built on one -- a simple dialog's options, a table's
        // rows -- was a thing a screen reader could read and not press.
        //
        // **One** stop, not two. The first attempt annotated the well, and an
        // annotation says its own words while a well has none: that produced a
        // blank node a reader could press sitting above a "Delete" they could
        // not, which is worse than the silence it replaced.
        let (_tree, _root, nodes) = well_saying_delete(Some(Rc::new(|| {})));
        let pressable: Vec<_> = nodes
            .iter()
            .filter(|node| node.properties.actions != 0)
            .collect();
        assert_eq!(pressable.len(), 1, "one stop, not two: {nodes:?}");
        assert_eq!(pressable[0].properties.label, "Delete");
        assert_eq!(
            pressable[0].properties.actions,
            crate::semantics::SemanticsAction::Tap as i32
        );
        assert!(
            !pressable[0].properties.flags.is_button,
            "a well is how a thing is pressed, not what it is"
        );
    }

    #[test]
    fn a_well_with_nothing_to_do_offers_nothing() {
        // Upstream's `onTap` is null and its `Semantics.onTap` with it. A node
        // advertising an action that goes nowhere sends a reader to press
        // something that will not answer.
        let (_tree, _root, nodes) = well_saying_delete(None);
        assert!(
            nodes.iter().all(|node| node.properties.actions == 0),
            "{nodes:?}"
        );
    }

    #[test]
    fn pressing_that_node_runs_the_callback_a_finger_would_have_run() {
        let ran = Rc::new(std::cell::Cell::new(false));
        let sink = Rc::clone(&ran);
        let (_tree, root, nodes) = well_saying_delete(Some(Rc::new(move || sink.set(true))));
        let node = nodes
            .iter()
            .find(|node| node.properties.actions != 0)
            .expect("a pressable node");
        assert!(crate::semantics::perform_action(
            &root,
            node.id,
            crate::semantics::SemanticsAction::Tap
        ));
        assert!(ran.get(), "the reader's press reached the callback");
    }

    #[test]
    fn pressing_it_leaves_the_same_mark_on_the_surface_a_finger_leaves() {
        // Upstream's handler is `simulateTap`, which is
        // `_startSplash(context: context); handleTap();` -- the splash first
        // and the callback second. Skipping the splash would be invisible to
        // the reader who pressed and plainly wrong to whoever is watching the
        // screen beside them: the press would look like it was swallowed.
        let (mut tree, root, nodes) = well_saying_delete(Some(Rc::new(|| {})));
        let node = nodes
            .iter()
            .find(|node| node.properties.actions != 0)
            .expect("a pressable node");
        assert!(crate::semantics::perform_action(
            &root,
            node.id,
            crate::semantics::SemanticsAction::Tap
        ));

        // A frame later, because a splash starts at nothing and grows: asked
        // at the instant of the press it has no radius yet and paints nothing,
        // which is true of a finger's splash too.
        tree.rebuild_dirty();
        assert!(
            tree.advance_frame(100 * MS),
            "a splash is an animation and wants frames"
        );
        // Rebuilt *after* the frame, not before: what is painted is decided
        // during the build, so a build that ran before the clock moved would
        // paint the splash at the radius it had when it started -- nothing.
        tree.rebuild_dirty();
        let mut after = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut after,
            crate::render::BoxConstraints::loose(400.0, 400.0),
        );
        let mut layers = crate::engine::LayerTree::new(600, 400);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(600.0, 400.0));
            crate::render::RenderBox::paint(&after, &mut context, Offset::ZERO);
        }
        // The splash is drawn as a rounded box, so a round-cornered rectangle
        // in the calls is the mark itself. **Where** it is matters as much as
        // that it exists: a mutation moving the splash's origin to the
        // top-left corner survived a test that only counted the circles, and a
        // splash spreading from a corner is the wrong picture of what a reader
        // did -- they activated the control, not its edge.
        let (left, top, right, bottom) = crate::engine_test_stubs::drawn()
            .iter()
            .find_map(|call| match call {
                crate::engine_test_stubs::Drawn::RRect {
                    left,
                    top,
                    right,
                    bottom,
                    ..
                } => Some((*left, *top, *right, *bottom)),
                _ => None,
            })
            .expect("the press put no ink on the surface");
        let well = 42.0_f32;
        assert!(
            ((left + right) / 2.0 - well / 2.0).abs() < 1.0,
            "the splash grew from {left}..{right}, not the middle of the well"
        );
        assert!(
            ((top + bottom) / 2.0 - 16.8 / 2.0).abs() < 1.0,
            "the splash grew from {top}..{bottom}, not the middle of the well"
        );
    }

    #[test]
    fn a_well_that_cannot_be_used_does_not_say_it_can() {
        // `enabled: false` is upstream's `_primaryEnabled` false, and its
        // `Semantics.onTap` goes with it. Told otherwise, a reader is sent to
        // press a thing that has already been switched off -- and hears
        // nothing back, because there is nothing to hear.
        let (_tree, _root, nodes) = mounted(crate::framework::stateful(
            delete_well(Some(Rc::new(|| {}))).with_enabled(false),
        ));
        assert!(
            nodes.iter().all(|node| node.properties.actions == 0),
            "{nodes:?}"
        );
    }

    #[test]
    fn a_well_the_clips_cut_away_cannot_be_pressed() {
        // The rule `find_handler` was written for, now that a fold answers
        // there too: a node a reader was never told about is not one they can
        // activate. Without the clip check on this branch, an ink well
        // scrolled out of its viewport would still run its callback on a stale
        // identifier.
        let ran = Rc::new(std::cell::Cell::new(false));
        let sink = Rc::clone(&ran);
        let well = crate::framework::stateful(delete_well(Some(Rc::new(move || sink.set(true)))));
        // Nothing gets through a zero-sized clip.
        let (_tree, root, _nodes) = mounted(crate::framework::single(well, |inner| {
            crate::render::RenderClipRect::new(
                crate::render::RenderConstrainedBox::tight(0.0, 0.0).with_child(inner),
            )
        }));
        assert!(
            !crate::semantics::perform_action(
                &root,
                crate::semantics::node_id_for(9304),
                crate::semantics::SemanticsAction::Tap
            ),
            "a node nobody could reach answered a press"
        );
        assert!(!ran.get());
    }

    #[test]
    fn a_rebuilt_well_presses_the_callback_the_last_build_meant() {
        // A handler closes over the build that made it. Keeping the first
        // one -- which is what happens if the render object does not take the
        // fresh object's on update -- means a dialog whose option changed
        // still runs the old option's callback for anyone using a reader,
        // while a finger runs the new one.
        let which = Rc::new(std::cell::Cell::new(0));

        let first = Rc::clone(&which);
        let mut tree = {
            let (tree, _root, _nodes) = well_saying_delete(Some(Rc::new(move || first.set(1))));
            tree
        };
        let second = Rc::clone(&which);
        tree.rebuild(crate::theme::MaterialTheme::new(
            crate::theme::ThemeData::light(),
            crate::framework::stateful(delete_well(Some(Rc::new(move || second.set(2))))),
        ));
        let mut root = tree.build_render_tree().expect("a root");
        crate::render::RenderBox::layout(
            &mut root,
            crate::render::BoxConstraints::loose(400.0, 400.0),
        );
        assert!(crate::semantics::perform_action(
            &root,
            crate::semantics::node_id_for(9304),
            crate::semantics::SemanticsAction::Tap
        ));
        assert_eq!(which.get(), 2, "the press reached the build before last");
    }
}

#[cfg(test)]
mod ink_focus_tests {
    use super::*;
    use crate::engine_test_stubs::Drawn;
    use crate::framework::{ElementTree, leaf, stateful};
    use crate::render::{BoxConstraints, Offset, RenderBox, Size};

    const INK: u64 = 7401;
    const FOCUS: u64 = 7402;
    const OTHER: u64 = 7403;
    const FOCUS_COLOUR: Color = Color(0xFF00_FF00);

    fn body() -> AnyWidget {
        leaf(|| crate::widgets::SizedBox::new(80.0, 40.0))
    }

    /// A response that can be reached by the keyboard, with a focus highlight
    /// nobody could mistake for anything else.
    fn focusable() -> InkResponse {
        InkResponse::new(INK, body)
            .with_focus(FOCUS)
            .with_focus_color(FOCUS_COLOUR)
            .with_on_tap(|| {})
    }

    /// What the response painted, after `frames` frames of its own clock.
    fn painted(widget: AnyWidget, frames: &[i64]) -> Vec<Drawn> {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        // Laid out **first**. A highlight is sized from the response's own
        // rectangle, which a build only knows once a layout has happened, so a
        // test that never laid out would watch nothing appear and could not
        // tell that from nothing being asked for.
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(200.0, 100.0));
        crate::render::flush_layout();
        for at in frames {
            tree.advance_frame(*at);
            tree.rebuild_dirty();
        }
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(200.0, 100.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(200, 100);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(200.0, 100.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        crate::engine_test_stubs::drawn()
    }

    fn painted_in(drawn: &[Drawn], wanted: Color) -> bool {
        drawn.iter().any(|call| match call {
            Drawn::Rect { argb, .. } | Drawn::RRect { argb, .. } | Drawn::Oval { argb, .. } => {
                Color(*argb).red() == wanted.red()
                    && Color(*argb).green() == wanted.green()
                    && Color(*argb).blue() == wanted.blue()
                    && Color(*argb).alpha() > 0
            }
            Drawn::Circle { argb, .. } => {
                Color(*argb).red() == wanted.red()
                    && Color(*argb).green() == wanted.green()
                    && Color(*argb).blue() == wanted.blue()
                    && Color(*argb).alpha() > 0
            }
            _ => false,
        })
    }

    #[test]
    fn a_response_the_keyboard_reached_says_so() {
        // `HighlightType::Focus` had a colour, a fade length and a slot in the
        // state, and **nothing raised it**. So a menu line reached by Tab
        // looked exactly like one nobody had reached -- which for a keyboard
        // reader is the whole of the feedback.
        crate::focus::unfocus();
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(focusable()));
        // Laid out **before** the focus arrives: the splash and the highlight
        // are sized from the response's own rectangle, which a build only
        // knows after a layout has happened. A first draft focused first and
        // watched a highlight of size zero not appear.
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(200.0, 100.0));
        crate::render::flush_layout();
        tree.advance_frame(0);
        tree.rebuild_dirty();

        crate::focus::focus(FOCUS);
        tree.rebuild_dirty();
        // Past the 50ms the focus highlight fades in over.
        tree.advance_frame(100_000);
        tree.rebuild_dirty();

        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(200.0, 100.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(200, 100);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(200.0, 100.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        assert!(
            painted_in(&crate::engine_test_stubs::drawn(), FOCUS_COLOUR),
            "the focus highlight reached the canvas"
        );
        crate::focus::unfocus();
    }

    #[test]
    fn a_response_nobody_reached_paints_no_highlight() {
        crate::focus::unfocus();
        assert!(
            !painted_in(&painted(stateful(focusable()), &[0, 100_000]), FOCUS_COLOUR),
            "nothing to show"
        );
    }

    #[test]
    fn the_highlight_goes_when_the_keyboard_moves_on() {
        // The other half of the same call: `updateHighlight(focus, value:
        // hasFocus)` with `hasFocus` false. A highlight that only ever came on
        // would leave a trail of lit-up menu lines behind the cursor.
        crate::focus::unfocus();
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![
                stateful(focusable()),
                crate::framework::component(crate::focus::Focus::new(OTHER, body())),
            ],
            |rendered| {
                let mut column = crate::render::RenderFlex::column();
                for child in rendered {
                    column = column.push(child);
                }
                column
            },
        ));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(200.0, 200.0));
        crate::render::flush_layout();
        tree.advance_frame(0);
        tree.rebuild_dirty();

        crate::focus::focus(FOCUS);
        tree.rebuild_dirty();
        tree.advance_frame(100_000);
        tree.rebuild_dirty();

        // It was there first, or the assertion below is about nothing.
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(200.0, 200.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(200, 200);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(200.0, 200.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        assert!(
            painted_in(&crate::engine_test_stubs::drawn(), FOCUS_COLOUR),
            "lit while the keyboard was on it"
        );

        crate::focus::focus(OTHER);
        tree.rebuild_dirty();
        tree.advance_frame(200_000);
        tree.rebuild_dirty();

        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(200.0, 200.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(200, 200);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(200.0, 200.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        assert!(
            !painted_in(&crate::engine_test_stubs::drawn(), FOCUS_COLOUR),
            "the highlight went with the keyboard"
        );
        crate::focus::unfocus();
    }

    #[test]
    fn a_response_with_no_focus_of_its_own_does_not_answer_to_its_own_id() {
        // The default has to be *no node*, not "a node under the response's
        // own id". A well wrapped around a text field shares that id with it,
        // and two nodes under one id is worse than none -- so a well that
        // helpfully registered itself would break the thing it wraps.
        crate::focus::unfocus();
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(
            InkResponse::new(INK, body)
                .with_focus_color(FOCUS_COLOUR)
                .with_on_tap(|| {}),
        ));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(200.0, 100.0));
        crate::render::flush_layout();
        tree.advance_frame(0);
        tree.rebuild_dirty();

        crate::focus::focus(INK);
        tree.rebuild_dirty();
        tree.advance_frame(100_000);
        tree.rebuild_dirty();

        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(200.0, 100.0));
        crate::render::flush_layout();
        let mut layers = crate::engine::LayerTree::new(200, 100);
        crate::engine_test_stubs::reset_drawn();
        {
            let mut context =
                crate::render::PaintContext::new(&mut layers, Size::new(200.0, 100.0));
            RenderBox::paint(&root, &mut context, Offset::ZERO);
        }
        assert!(
            !painted_in(&crate::engine_test_stubs::drawn(), FOCUS_COLOUR),
            "nothing claimed the id"
        );
        crate::focus::unfocus();
    }

    #[test]
    fn the_keyboards_highlight_is_its_own_and_does_not_take_the_pointers() {
        // Three highlights, three slots. Raising the focus one in the hover
        // one's place would look identical on screen -- same colour, same
        // shape -- and then a pointer resting on a focused line would find the
        // slot already taken, or take it away.
        crate::focus::unfocus();
        let mut tree = ElementTree::new();
        tree.rebuild(stateful(focusable()));
        let root = tree.build_render_tree().expect("a root");
        crate::render::schedule_root_layout(&root, BoxConstraints::loose(200.0, 100.0));
        crate::render::flush_layout();
        tree.advance_frame(0);
        tree.rebuild_dirty();

        crate::focus::focus(FOCUS);
        tree.rebuild_dirty();

        let mut stack: Vec<crate::framework::ElementId> = tree.root().into_iter().collect();
        let mut slots = None;
        while let Some(id) = stack.pop() {
            if let Some(found) = tree.state::<InkResponseState, _>(id, |state| {
                (
                    state.highlight(HighlightType::Focus).is_some(),
                    state.highlight(HighlightType::Hover).is_some(),
                    state.highlight(HighlightType::Pressed).is_some(),
                )
            }) {
                slots = Some(found);
                break;
            }
            let mut children = tree.children_of(id);
            children.reverse();
            stack.extend(children);
        }
        assert_eq!(
            slots,
            Some((true, false, false)),
            "the keyboard filled its own slot and left the others alone"
        );
        crate::focus::unfocus();
    }

    #[test]
    fn a_response_that_is_not_a_focus_node_is_not_one() {
        // The default, and it is the right default here: a well wrapped around
        // something that has a focus node of its own -- a search bar's field --
        // would otherwise register a second node under the same id.
        assert!(InkResponse::new(INK, body).focus_id.is_none());
        assert_eq!(focusable().focus_id, Some(FOCUS));
    }

    #[test]
    fn the_caller_is_told_when_the_keyboard_arrives_and_leaves() {
        // Upstream's `onFocusChange`, which a menu item uses to close its
        // children when the focus walks off it.
        crate::focus::unfocus();
        let heard: Rc<std::cell::RefCell<Vec<bool>>> = Rc::new(std::cell::RefCell::new(Vec::new()));
        let sink = Rc::clone(&heard);
        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::many(
            vec![
                stateful(
                    focusable()
                        .with_on_focus_change(move |focused| sink.borrow_mut().push(focused)),
                ),
                crate::framework::component(crate::focus::Focus::new(OTHER, body())),
            ],
            |rendered| {
                let mut column = crate::render::RenderFlex::column();
                for child in rendered {
                    column = column.push(child);
                }
                column
            },
        ));
        tree.build_render_tree();
        tree.advance_frame(0);
        tree.rebuild_dirty();

        crate::focus::focus(FOCUS);
        tree.rebuild_dirty();
        crate::focus::focus(OTHER);
        tree.rebuild_dirty();

        assert_eq!(*heard.borrow(), vec![true, false]);
        crate::focus::unfocus();
    }
}
