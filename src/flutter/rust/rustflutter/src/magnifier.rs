// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Upstream `widgets/magnifier.dart`: the loupe a finger drags over text.
//!
//! A magnifier exists because a finger covers what it is pointing at. On a
//! touch screen the one place the reader most needs to see -- where the caret
//! will land -- is exactly the place their fingertip is on top of, so the
//! system lifts a copy of it out and shows it above the finger.
//!
//! That is why nearly everything here is about *position* rather than about
//! magnification. The scale is one number; the rest of the file is the
//! arithmetic that keeps the loupe on screen, over the right line, and out of
//! its own way.

use crate::engine::{Color, Rect};
use crate::painting::{BoxShadow, ClipBehavior};
use crate::render::{Offset, Size};

/// Upstream `MagnifierInfo`: everything the loupe needs to know about the
/// gesture it is following.
///
/// Four rectangles rather than one point, and each answers a different
/// question. The **gesture position** is where the finger is; the **caret
/// rect** is what the loupe should be centred over, which is not the same
/// thing once the finger has dragged past the end of a word; the **current
/// line** bounds the loupe horizontally so it does not wander onto the line
/// above; and the **field bounds** stop it leaving the text field altogether.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MagnifierInfo {
    pub global_gesture_position: Offset,
    pub caret_rect: Rect,
    pub current_line_boundaries: Rect,
    pub field_bounds: Rect,
}

impl MagnifierInfo {
    /// Upstream's `MagnifierInfo.empty`: all zeros.
    ///
    /// It exists so a `ValueNotifier<MagnifierInfo>` can be built before the
    /// first gesture arrives -- there is no such thing as "no info" once a
    /// notifier has to hold one, so the zero is the stand-in.
    pub const EMPTY: MagnifierInfo = MagnifierInfo {
        global_gesture_position: Offset::ZERO,
        caret_rect: Rect::ltrb(0.0, 0.0, 0.0, 0.0),
        current_line_boundaries: Rect::ltrb(0.0, 0.0, 0.0, 0.0),
        field_bounds: Rect::ltrb(0.0, 0.0, 0.0, 0.0),
    };

    pub fn new(
        global_gesture_position: Offset,
        caret_rect: Rect,
        current_line_boundaries: Rect,
        field_bounds: Rect,
    ) -> MagnifierInfo {
        MagnifierInfo {
            global_gesture_position,
            caret_rect,
            current_line_boundaries,
            field_bounds,
        }
    }
}

/// Upstream `MagnifierDecoration`: what the loupe looks like around its
/// window.
#[derive(Clone, Debug, PartialEq)]
pub struct MagnifierDecoration {
    pub opacity: f32,
    pub shadows: Option<Vec<BoxShadow>>,
    /// Upstream's `shape`, a plain rounded rectangle by default -- which is to
    /// say square corners, since the default `RoundedRectangleBorder` has no
    /// radius. The platforms round it themselves.
    pub corner_radius: f32,
}

impl MagnifierDecoration {
    pub fn new() -> MagnifierDecoration {
        MagnifierDecoration {
            opacity: 1.0,
            shadows: None,
            corner_radius: 0.0,
        }
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    pub fn with_shadows(mut self, shadows: Vec<BoxShadow>) -> Self {
        self.shadows = Some(shadows);
        self
    }

    pub fn with_corner_radius(mut self, radius: f32) -> Self {
        self.corner_radius = radius;
        self
    }
}

impl Default for MagnifierDecoration {
    fn default() -> MagnifierDecoration {
        MagnifierDecoration::new()
    }
}

/// Upstream `RawMagnifier`: the loupe itself, with no policy about where it
/// goes.
#[derive(Clone, Debug, PartialEq)]
pub struct RawMagnifier {
    pub size: Size,
    pub decoration: MagnifierDecoration,
    pub clip_behavior: ClipBehavior,
    /// Upstream's `focalPointOffset`: how far the magnified point sits from
    /// the loupe's own centre.
    ///
    /// Not zero in practice, and that is the whole trick: the loupe is drawn
    /// *above* the finger while showing what is *under* it, so the focal point
    /// has to be pushed down by roughly the distance the loupe was lifted.
    pub focal_point_offset: Offset,
    pub magnification_scale: f32,
}

impl RawMagnifier {
    /// Upstream asserts `magnificationScale != 0`, with its own words:
    /// "results in undefined behavior".
    ///
    /// Worth keeping as an assert rather than a clamp. A zero scale collapses
    /// every point of the source to one point, so there is no sensible image
    /// to draw and no nearby value that is what the caller meant -- a caller
    /// who computed zero has a bug, and a clamp would hide it behind a loupe
    /// that merely looked odd.
    pub fn new(size: Size) -> RawMagnifier {
        RawMagnifier {
            size,
            decoration: MagnifierDecoration::new(),
            clip_behavior: ClipBehavior::None,
            focal_point_offset: Offset::ZERO,
            magnification_scale: 1.0,
        }
    }

    pub fn with_magnification_scale(mut self, scale: f32) -> Self {
        debug_assert!(scale != 0.0, "a magnification scale of zero is undefined");
        self.magnification_scale = scale;
        self
    }

    pub fn with_focal_point_offset(mut self, offset: Offset) -> Self {
        self.focal_point_offset = offset;
        self
    }

    pub fn with_decoration(mut self, decoration: MagnifierDecoration) -> Self {
        self.decoration = decoration;
        self
    }

    pub fn with_clip_behavior(mut self, clip: ClipBehavior) -> Self {
        self.clip_behavior = clip;
        self
    }

    /// **A scale below 1 shrinks rather than magnifies**, and upstream allows
    /// it: the assert is only against zero. A caller who wants a wide-angle
    /// view of a field is not doing anything wrong, so this only reports which
    /// it is.
    pub fn magnifies(&self) -> bool {
        self.magnification_scale.abs() > 1.0
    }
}

/// Upstream `MagnifierController`: who owns the loupe while it is up.
///
/// The part worth porting is [`MagnifierController::shift_within_bounds`],
/// which is where the loupe's position is actually decided; the rest of
/// upstream's controller is an `OverlayEntry`'s lifetime, and this crate has
/// no overlay (the gap [`crate::material`] records for `Material.of`).
pub struct MagnifierController;

impl MagnifierController {
    /// Upstream's `shiftWithinBounds`: the smallest move that puts `rect`
    /// inside `bounds`.
    ///
    /// **Shifted, never resized.** A loupe pushed against the screen's edge
    /// slides along it and keeps its size, because a magnifier that shrank as
    /// it neared the edge would change how much text it showed exactly when
    /// the reader was working at the margin.
    ///
    /// The two axes are decided independently -- a loupe past the left edge
    /// and below the bottom moves diagonally in one step rather than being
    /// clamped on whichever axis is tested first.
    pub fn shift_within_bounds(rect: Rect, bounds: Rect) -> Rect {
        debug_assert!(
            rect.width() <= bounds.width(),
            "cannot shift a rect wider than its bounds"
        );
        debug_assert!(
            rect.height() <= bounds.height(),
            "cannot shift a rect taller than its bounds"
        );

        let mut dx = 0.0;
        if rect.left < bounds.left {
            dx = bounds.left - rect.left;
        } else if rect.right > bounds.right {
            dx = bounds.right - rect.right;
        }

        let mut dy = 0.0;
        if rect.top < bounds.top {
            dy = bounds.top - rect.top;
        } else if rect.bottom > bounds.bottom {
            dy = bounds.bottom - rect.bottom;
        }

        Rect::ltrb(
            rect.left + dx,
            rect.top + dy,
            rect.right + dx,
            rect.bottom + dy,
        )
    }
}

/// Upstream `TextMagnifierConfiguration`: whether a text field has a magnifier
/// at all, and which.
///
/// # Off is the absence of a builder, not a flag
///
/// Upstream's `disabled` is the **default-constructed** configuration, and its
/// `magnifierBuilder` falls back to a function that returns null. So a field
/// with no configured builder simply builds no loupe -- there is no separate
/// "enabled" bit that could disagree with whether a builder exists.
///
/// That matters for a platform that has no magnifier: it supplies nothing, and
/// every field on it is correct without a single conditional.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextMagnifierConfiguration {
    /// Whether a builder was given at all.
    pub has_builder: bool,
    /// Upstream's `shouldDisplayHandlesInMagnifier`, on by default.
    ///
    /// The handles are drawn *inside* the magnified image, which sounds like
    /// clutter and is not: the reader is dragging a handle, and a loupe that
    /// hid the very thing being dragged would show them the text without
    /// showing them where they had got to in it.
    pub should_display_handles_in_magnifier: bool,
}

impl TextMagnifierConfiguration {
    /// Upstream's `TextMagnifierConfiguration.disabled`, which is the default
    /// constructor -- see the type docs.
    pub const DISABLED: TextMagnifierConfiguration = TextMagnifierConfiguration {
        has_builder: false,
        should_display_handles_in_magnifier: true,
    };

    /// A configuration that does build a loupe.
    pub fn enabled() -> TextMagnifierConfiguration {
        TextMagnifierConfiguration {
            has_builder: true,
            should_display_handles_in_magnifier: true,
        }
    }

    pub fn with_handles_in_magnifier(mut self, show: bool) -> Self {
        self.should_display_handles_in_magnifier = show;
        self
    }

    /// Whether this configuration puts a magnifier up. Upstream answers the
    /// same question by returning null from the builder.
    pub fn builds_a_magnifier(&self) -> bool {
        self.has_builder
    }
}

impl Default for TextMagnifierConfiguration {
    fn default() -> TextMagnifierConfiguration {
        TextMagnifierConfiguration::DISABLED
    }
}

/// Upstream `CupertinoMagnifier` (`cupertino/magnifier.dart`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CupertinoMagnifier {
    pub size: Size,
}

impl CupertinoMagnifier {
    /// Upstream's `kDefaultSize`.
    pub const DEFAULT_SIZE: Size = Size {
        width: 80.0,
        height: 47.5,
    };
    /// Upstream's `kMagnification`.
    pub const MAGNIFICATION: f32 = 1.25;
    /// Upstream's `kBorderRadius`, which is very nearly half the height -- an
    /// iOS loupe is a rounded slot rather than a rectangle.
    pub const BORDER_RADIUS: f32 = 0.5 * (0.5 * 80.0 + 4.0);
    /// Upstream's vertical offset from the focal point: how far above the
    /// finger the loupe is lifted.
    pub const VERTICAL_OFFSET: f32 = -50.0;

    pub fn new() -> CupertinoMagnifier {
        CupertinoMagnifier {
            size: CupertinoMagnifier::DEFAULT_SIZE,
        }
    }
}

impl Default for CupertinoMagnifier {
    fn default() -> CupertinoMagnifier {
        CupertinoMagnifier::new()
    }
}

/// Upstream `Magnifier` (`material/magnifier.dart`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Magnifier {
    pub size: Size,
}

impl Magnifier {
    /// Upstream's `kDefaultMagnifierSize`.
    pub const DEFAULT_SIZE: Size = Size {
        width: 77.37,
        height: 37.9,
    };
    /// Upstream's `kMagnificationScale`.
    pub const MAGNIFICATION_SCALE: f32 = 1.25;
    /// Upstream's `kStandardVerticalFocalPointShift`.
    pub const STANDARD_VERTICAL_FOCAL_POINT_SHIFT: f32 = 22.0;

    pub fn new() -> Magnifier {
        Magnifier {
            size: Magnifier::DEFAULT_SIZE,
        }
    }
}

impl Default for Magnifier {
    fn default() -> Magnifier {
        Magnifier::new()
    }
}

/// A colour a loupe's border is drawn in, kept beside the two platform
/// magnifiers so the file has one place for their differences.
pub const MAGNIFIER_DEFAULT_BORDER: Color = Color::argb(0x33, 0, 0, 0);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loupe_slides_along_an_edge_rather_than_shrinking_at_it() {
        // A magnifier that shrank as it neared the edge would change how much
        // text it showed exactly when the reader was working at the margin.
        let screen = Rect::ltrb(0.0, 0.0, 400.0, 800.0);
        let off_the_left = Rect::ltrb(-30.0, 100.0, 50.0, 150.0);
        let shifted = MagnifierController::shift_within_bounds(off_the_left, screen);
        assert_eq!(shifted.left, 0.0);
        assert_eq!(shifted.width(), off_the_left.width(), "same size");
        assert_eq!(
            shifted.top, off_the_left.top,
            "and the other axis is left alone"
        );
    }

    #[test]
    fn the_two_axes_are_decided_independently() {
        // A loupe past the left edge *and* below the bottom moves diagonally
        // in one step, rather than being clamped on whichever axis happens to
        // be tested first.
        let screen = Rect::ltrb(0.0, 0.0, 400.0, 800.0);
        let corner = Rect::ltrb(-20.0, 790.0, 60.0, 840.0);
        let shifted = MagnifierController::shift_within_bounds(corner, screen);
        assert_eq!(shifted.left, 0.0);
        assert_eq!(shifted.bottom, 800.0);
        assert_eq!(shifted.width(), corner.width());
        assert_eq!(shifted.height(), corner.height());
    }

    #[test]
    fn a_loupe_already_inside_is_not_moved_at_all() {
        let screen = Rect::ltrb(0.0, 0.0, 400.0, 800.0);
        let inside = Rect::ltrb(100.0, 100.0, 180.0, 150.0);
        assert_eq!(
            MagnifierController::shift_within_bounds(inside, screen),
            inside
        );
    }

    #[test]
    fn off_is_the_absence_of_a_builder_rather_than_a_flag() {
        // Upstream's `disabled` *is* the default constructor, and its builder
        // falls back to one that returns null. So a platform with no magnifier
        // supplies nothing and every field on it is correct without a single
        // conditional.
        assert_eq!(
            TextMagnifierConfiguration::default(),
            TextMagnifierConfiguration::DISABLED
        );
        assert!(!TextMagnifierConfiguration::DISABLED.builds_a_magnifier());
        assert!(TextMagnifierConfiguration::enabled().builds_a_magnifier());
    }

    #[test]
    fn the_handles_are_drawn_inside_the_magnified_image_by_default() {
        // It sounds like clutter and is not: the reader is dragging a handle,
        // and a loupe that hid the thing being dragged would show them the
        // text without showing where they had got to in it.
        assert!(TextMagnifierConfiguration::DISABLED.should_display_handles_in_magnifier);
        assert!(
            !TextMagnifierConfiguration::enabled()
                .with_handles_in_magnifier(false)
                .should_display_handles_in_magnifier
        );
    }

    #[test]
    fn a_scale_below_one_shrinks_and_upstream_allows_it() {
        // The assert is only against zero. A caller wanting a wide-angle view
        // of a field is not doing anything wrong.
        let wide = RawMagnifier::new(Size::new(80.0, 40.0)).with_magnification_scale(0.5);
        assert!(!wide.magnifies());
        let loupe = RawMagnifier::new(Size::new(80.0, 40.0)).with_magnification_scale(1.25);
        assert!(loupe.magnifies());
        // And exactly one is neither: it shows the text at its own size.
        let flat = RawMagnifier::new(Size::new(80.0, 40.0));
        assert_eq!(flat.magnification_scale, 1.0);
        assert!(!flat.magnifies());
    }

    #[test]
    fn both_platforms_magnify_by_the_same_quarter() {
        // The sizes and shapes differ but the scale does not, which is worth
        // knowing: the loupe is not there to make text large, it is there to
        // lift it out from under a finger.
        assert_eq!(
            CupertinoMagnifier::MAGNIFICATION,
            Magnifier::MAGNIFICATION_SCALE
        );
        assert_eq!(CupertinoMagnifier::MAGNIFICATION, 1.25);
    }

    #[test]
    fn an_ios_loupe_is_a_rounded_slot_and_a_material_one_is_not() {
        // iOS rounds by nearly half its height; Material's is a shallower
        // rectangle. The two platforms' shapes are their own.
        let ios = CupertinoMagnifier::DEFAULT_SIZE;
        assert!(CupertinoMagnifier::BORDER_RADIUS > ios.height / 2.5);
        assert!(
            Magnifier::DEFAULT_SIZE.height < ios.height,
            "and the Material one is shallower"
        );
    }

    #[test]
    fn the_loupe_is_lifted_above_the_finger_it_follows() {
        // Which is the whole point of the widget: a finger covers what it is
        // pointing at, so the copy is shown above it. iOS lifts by a negative
        // offset and Material shifts its focal point by a positive one -- two
        // spellings of the same move.
        assert!(CupertinoMagnifier::VERTICAL_OFFSET < 0.0);
        assert!(Magnifier::STANDARD_VERTICAL_FOCAL_POINT_SHIFT > 0.0);
    }

    #[test]
    fn the_empty_info_exists_so_a_notifier_can_hold_one_before_the_first_touch() {
        // There is no such thing as "no info" once a notifier has to hold one,
        // so the zero is the stand-in.
        assert_eq!(MagnifierInfo::EMPTY.global_gesture_position, Offset::ZERO);
        assert_eq!(MagnifierInfo::EMPTY.caret_rect.width(), 0.0);
        assert_eq!(MagnifierInfo::EMPTY.field_bounds.height(), 0.0);
    }
}
