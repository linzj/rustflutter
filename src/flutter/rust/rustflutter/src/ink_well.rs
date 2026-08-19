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
    contained_ink_well: bool,
    highlight_shape: InkHighlightShape,
    radius: Option<f32>,
    splash_color: Option<Color>,
    highlight_color: Option<Color>,
    hover_color: Option<Color>,
    focus_color: Option<Color>,
    splash_factory: Option<InteractiveInkFeatureFactory>,
    hover_micros: Option<i64>,
    enabled: bool,
}

impl InkResponse {
    pub fn new(id: u64, build_child: impl Fn() -> AnyWidget + 'static) -> InkResponse {
        InkResponse {
            id,
            build_child: Box::new(build_child),
            on_tap: None,
            on_tap_cancel: None,
            on_highlight_changed: None,
            on_hover: None,
            contained_ink_well: false,
            // Upstream's default, and the reason `InkWell` exists to change
            // it: a response with no box to fill is a circle.
            highlight_shape: InkHighlightShape::Circle,
            radius: None,
            splash_color: None,
            highlight_color: None,
            hover_color: None,
            focus_color: None,
            splash_factory: None,
            hover_micros: None,
            enabled: true,
        }
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
    fn highlight_color_for(&self, kind: HighlightType, theme: &crate::components::Theme) -> Color {
        let resolved = match kind {
            HighlightType::Pressed => self.highlight_color,
            HighlightType::Hover => self.hover_color,
            HighlightType::Focus => self.focus_color,
        }
        .unwrap_or(theme.primary.with_alpha(0x14));
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
        let splash_color = self.splash_color.unwrap_or(theme.primary.with_alpha(0x1f));
        let factory = self.splash_factory.unwrap_or_default();
        let contained = self.contained_ink_well;
        let child = (self.build_child)();

        // Everything the handlers need, captured rather than reached for: a
        // handler runs long after the build that made it.
        let pressed_colour = self.highlight_color_for(HighlightType::Pressed, &theme);
        let hover_colour = self.highlight_color_for(HighlightType::Hover, &theme);
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
                    let size = down_sink.get();
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

        single(child, move |child| {
            let mut stack = crate::render::RenderStack::new().push_boxed(child);
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
            if contained {
                crate::render::RenderRef::new(crate::render::RenderClipRect::new(region))
            } else {
                crate::render::RenderRef::new(region)
            }
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
    fn a_disabled_response_still_makes_its_highlights_at_alpha_zero() {
        // Upstream's `enabled ? resolved : resolved.withAlpha(0)`. It looks
        // wasteful until the lifecycle is the reason: the highlight has to
        // exist so that becoming enabled again is a colour change rather than
        // a highlight appearing from nowhere half way through a hover.
        let theme = crate::components::Theme::dark();
        let enabled = InkResponse::new(1, || crate::framework::leaf(|| crate::widgets::Empty))
            .with_hover_color(INK);
        let disabled = InkResponse::new(1, || crate::framework::leaf(|| crate::widgets::Empty))
            .with_hover_color(INK)
            .with_enabled(false);
        assert_eq!(
            enabled.highlight_color_for(HighlightType::Hover, &theme),
            INK
        );
        assert_eq!(
            disabled
                .highlight_color_for(HighlightType::Hover, &theme)
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
}
