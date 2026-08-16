// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! How far down a list you are.
//!
//! A long list with no scrollbar tells the reader nothing about how much of it
//! they have seen or how much is left, and there is no way to find out except
//! by scrolling to the end. Upstream this is `Scrollbar` over `RawScrollbar`,
//! painted by `ScrollbarPainter`; the arithmetic here is that painter's.
//!
//! It fades: visible while the list is moving, gone 600ms after it stops, over
//! a 300ms fade. Upstream's `_kScrollbarTimeToFade` and
//! `_kScrollbarFadeDuration`, and the reason is that a scrollbar is an answer
//! to a question the reader only asks while scrolling.

use crate::components::theme_of;
use crate::engine::Color;
use crate::framework::{AnyWidget, BuildContext, StateHandle, StatefulComponent, stateful};
use crate::render::{Axis, EdgeInsets, StackPosition};

/// How thick the thumb is. Upstream's Material `_kScrollbarThickness`.
pub const THICKNESS: f32 = 8.0;

/// The shortest a thumb may be drawn, however long the list is. Upstream's
/// `_kScrollbarMinLength`: a thumb that shrinks with the content becomes a
/// dot on a list of ten thousand rows, and a dot cannot be aimed at.
pub const MIN_THUMB_LENGTH: f32 = 48.0;

/// How long after the last scroll the thumb starts to go.
pub const TIME_TO_FADE_MICROS: i64 = 600_000;

/// How long it takes to go.
pub const FADE_MICROS: i64 = 300_000;

/// Where the thumb goes and how big it is.
///
/// `viewport` is what is visible, `content` is the whole thing, `offset` is how
/// far down. Returns `None` when there is nothing to scroll -- upstream hides
/// the scrollbar in exactly that case rather than drawing a full-length thumb.
pub fn thumb(viewport: f32, content: f32, offset: f32) -> Option<(f32, f32)> {
    if viewport <= 0.0 || content <= viewport {
        return None;
    }
    // The proportion of the content that is visible, floored so it can still
    // be grabbed. Upstream's ScrollbarPainter does the same in
    // `_thumbExtent`.
    let proportional = viewport / content * viewport;
    let length = proportional.max(MIN_THUMB_LENGTH).min(viewport);
    let max_offset = content - viewport;
    let fraction = (offset / max_offset).clamp(0.0, 1.0);
    // The thumb travels the track *minus its own length*, which is why this is
    // not simply the scroll fraction times the viewport: at the bottom the
    // thumb's far edge has to land on the far edge of the track.
    Some((fraction * (viewport - length), length))
}

/// What a [`Scrollbar`] remembers between frames.
#[derive(Default)]
pub struct ScrollbarState {
    /// When the offset last changed, and what it was.
    last_moved_micros: i64,
    last_offset: f32,
    now_micros: i64,
    started: bool,
}

impl ScrollbarState {
    /// How visible the thumb is right now.
    fn opacity(&self) -> f32 {
        if !self.started {
            return 0.0;
        }
        let since = (self.now_micros - self.last_moved_micros).max(0);
        if since <= TIME_TO_FADE_MICROS {
            return 1.0;
        }
        let fading = (since - TIME_TO_FADE_MICROS) as f32 / FADE_MICROS as f32;
        (1.0 - fading).clamp(0.0, 1.0)
    }
}

/// A thumb along the edge of a scrollable, showing how far down it is.
///
/// It has no child: it is an overlay, put into the same `Stack` as the
/// scrollable and sized to the same box. That is not a shortcut -- it is what
/// keeps it out of the layout, so adding one cannot change where anything
/// else ends up, which is upstream's arrangement too (the painter draws into
/// the scrollable's own layer).
///
/// It is told the offset and the extents rather than owning them: upstream
/// reads them off the `ScrollPosition` through a notification, and the caller
/// here already holds the [`crate::scrolling::Scroll`].
///
/// ```ignore
/// Stack::new()
///     .push(list)
///     .push(component(Scrollbar::new(scroll.offset, height, content)))
/// ```
pub struct Scrollbar {
    axis: Axis,
    offset: f32,
    viewport: f32,
    content: f32,
    color: Option<Color>,
}

impl Scrollbar {
    pub fn new(offset: f32, viewport: f32, content: f32) -> Scrollbar {
        Scrollbar { axis: Axis::Vertical, offset, viewport, content, color: None }
    }

    pub fn horizontal(mut self) -> Self {
        self.axis = Axis::Horizontal;
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

impl StatefulComponent for Scrollbar {
    type State = ScrollbarState;

    fn advance(&self, state: &mut ScrollbarState, frame_time_micros: i64) -> bool {
        let was = state.opacity();
        if !state.started || state.last_offset != self.offset {
            state.started = true;
            state.last_offset = self.offset;
            state.last_moved_micros = frame_time_micros;
        }
        state.now_micros = frame_time_micros;
        // Keep asking for frames while it is on its way out; the frame that
        // reaches zero still has to be drawn, or the thumb stays on screen.
        let now = state.opacity();
        now > 0.0 || was > 0.0
    }

    fn build(
        &self,
        state: &ScrollbarState,
        _handle: StateHandle<ScrollbarState>,
        context: &mut BuildContext,
    ) -> AnyWidget {
        let theme = theme_of(context);
        let color = self.color.unwrap_or(theme.text_muted);
        let opacity = state.opacity();
        let thumb = thumb(self.viewport, self.content, self.offset);
        let axis = self.axis;

        crate::framework::leaf(move || {
            let mut stack = crate::render::RenderStack::new();
            if let (Some((start, length)), true) = (thumb, opacity > 0.0) {
                let bar = crate::widgets::Container::new()
                    .with_color(color.with_alpha((0xB0 as f32 * opacity) as u8))
                    .with_corner_radius(THICKNESS / 2.0);
                let (bar, position) = match axis {
                    Axis::Vertical => (
                        bar.with_size(THICKNESS, length),
                        StackPosition { top: Some(start), right: Some(2.0), ..Default::default() },
                    ),
                    Axis::Horizontal => (
                        bar.with_size(length, THICKNESS),
                        StackPosition {
                            left: Some(start),
                            bottom: Some(2.0),
                            ..Default::default()
                        },
                    ),
                };
                stack = stack.push_positioned(bar, position);
            }
            // Drawn over the list and invisible to it: a bar that took the
            // taps meant for the rows underneath would be worse than no bar.
            crate::render::RenderIgnorePointer::new(stack)
        })
    }
}

/// [`Scrollbar`] as a widget.
pub fn scrollbar(offset: f32, viewport: f32, content: f32) -> AnyWidget {
    stateful(Scrollbar::new(offset, viewport, content))
}

/// Padding a list should leave for a scrollbar that is drawn over it.
pub const GUTTER: EdgeInsets = EdgeInsets::only(0.0, 0.0, THICKNESS + 4.0, 0.0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_to_scroll_is_nothing_to_draw() {
        assert!(thumb(500.0, 400.0, 0.0).is_none(), "the content fits");
        assert!(thumb(500.0, 500.0, 0.0).is_none(), "exactly fits");
        assert!(thumb(0.0, 500.0, 0.0).is_none(), "no viewport at all");
    }

    #[test]
    fn the_thumb_is_the_visible_proportion() {
        // A quarter of the content is visible, so the thumb is a quarter of
        // the track.
        let (start, length) = thumb(500.0, 2000.0, 0.0).expect("scrollable");
        assert_eq!(length, 125.0);
        assert_eq!(start, 0.0);
    }

    #[test]
    fn the_thumb_reaches_the_bottom_at_the_bottom() {
        let viewport = 500.0;
        let content = 2000.0;
        let (start, length) = thumb(viewport, content, content - viewport).expect("scrollable");
        // The thumb's far edge lands on the track's far edge, which is what
        // makes "am I at the end" answerable at a glance.
        assert!((start + length - viewport).abs() < 0.01);
    }

    #[test]
    fn a_very_long_list_still_has_a_thumb_you_can_see() {
        let (_, length) = thumb(500.0, 500_000.0, 0.0).expect("scrollable");
        assert_eq!(length, MIN_THUMB_LENGTH, "a dot cannot be aimed at");
    }

    #[test]
    fn the_thumb_shows_while_it_moves_and_goes_afterwards() {
        let mut state = ScrollbarState::default();
        let bar = Scrollbar::new(0.0, 500.0, 2000.0);

        assert!(bar.advance(&mut state, 1_000_000), "a first frame shows it");
        assert_eq!(state.opacity(), 1.0);

        // Still, for half a second: still visible.
        bar.advance(&mut state, 1_400_000);
        assert_eq!(state.opacity(), 1.0);

        // Past the delay, fading.
        bar.advance(&mut state, 1_000_000 + TIME_TO_FADE_MICROS + FADE_MICROS / 2);
        let fading = state.opacity();
        assert!(fading > 0.0 && fading < 1.0, "{fading}");

        // Gone, and it stops asking for frames one frame later.
        let last = bar.advance(&mut state, 1_000_000 + TIME_TO_FADE_MICROS + FADE_MICROS);
        assert_eq!(state.opacity(), 0.0);
        assert!(last, "the frame that reaches zero still has to be drawn");
        assert!(!bar.advance(&mut state, 3_000_000), "and then it is idle");
    }

    #[test]
    fn scrolling_again_brings_it_back() {
        let mut state = ScrollbarState::default();
        let still = Scrollbar::new(0.0, 500.0, 2000.0);
        still.advance(&mut state, 0);
        still.advance(&mut state, TIME_TO_FADE_MICROS + FADE_MICROS);
        assert_eq!(state.opacity(), 0.0);

        let moved = Scrollbar::new(120.0, 500.0, 2000.0);
        moved.advance(&mut state, TIME_TO_FADE_MICROS + FADE_MICROS + 16_000);
        assert_eq!(state.opacity(), 1.0);
    }
}
