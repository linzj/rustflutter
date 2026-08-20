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
/// The part worth porting *here* is
/// [`MagnifierController::shift_within_bounds`], which is where the loupe's
/// position is actually decided. The rest of upstream's controller is an
/// `OverlayEntry`'s lifetime, and that is
/// [`crate::magnifier_host::MagnifierHost`]: it holds the entry, moves it as
/// the gesture moves, hides it when the platform says to, and takes it down.
///
/// What the host still cannot do is *magnify* -- that needs a backdrop read
/// with a scale and the paint bridge has only a blur. The loupe's body is
/// drawn; what shows through it is not.
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

/// Where a loupe ended up, and how it should get there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MagnifierPlacement {
    /// The loupe's top-left corner, in global coordinates.
    pub position: Offset,
    /// How far the focal point is pushed from the loupe's own centre, on top
    /// of the platform's standing offset.
    pub extra_focal_point_offset: Offset,
    /// Whether the move to this position should be animated rather than
    /// tracked directly. See [`TextMagnifier::place`].
    pub animate: bool,
    /// Whether the loupe should be on screen at all.
    pub shown: bool,
}

/// Upstream `TextMagnifier` (`material/magnifier.dart`): the Android loupe's
/// positioning.
///
/// Upstream's widget is a `StatefulWidget` whose whole job is one method,
/// `_determineMagnifierPositionAndFocalPoint`. That method is here as a pure
/// function of the gesture and the screen -- what is left in upstream's state
/// is a listener, a timer and an `AnimatedPositioned`, none of which decide
/// anything.
pub struct TextMagnifier;

impl TextMagnifier {
    /// Upstream's `jumpBetweenLinesAnimationDuration`, in microseconds.
    pub const JUMP_BETWEEN_LINES_MICROS: i64 = 70_000;

    /// Upstream's `_determineMagnifierPositionAndFocalPoint`.
    ///
    /// `previous` is where the loupe was last frame, or `None` on the first --
    /// upstream keeps its `_magnifierPosition` null for exactly one frame so
    /// that the *first* appearance is not animated. A loupe that animated into
    /// place from wherever the last one happened to be would slide across the
    /// screen when it appeared.
    pub fn place(
        info: MagnifierInfo,
        screen: Rect,
        previous: Option<Offset>,
    ) -> MagnifierPlacement {
        let size = Magnifier::DEFAULT_SIZE;
        // Upstream's `basicMagnifierOffset`: the loupe is drawn from its top
        // left, so this moves it to be centred on the point *and* lifted above
        // the finger.
        let basic = Offset::new(
            size.width / 2.0,
            size.height + Magnifier::STANDARD_VERTICAL_FOCAL_POINT_SHIFT,
        );

        // **The loupe tracks the finger horizontally but never leaves the
        // line.** Dragging past the end of a line would otherwise show the
        // reader empty space beside the text they are trying to place a caret
        // in.
        let magnifier_x = info.global_gesture_position.dx.clamp(
            info.current_line_boundaries.left,
            info.current_line_boundaries.right,
        );
        // Vertically it follows the *caret*, not the finger -- the finger may
        // be anywhere below, but the loupe belongs over the line.
        let caret_centre_y = (info.caret_rect.top + info.caret_rect.bottom) / 2.0;
        let unadjusted = Rect::ltrb(
            magnifier_x - basic.dx,
            caret_centre_y - basic.dy,
            magnifier_x - basic.dx + size.width,
            caret_centre_y - basic.dy + size.height,
        );
        let adjusted = MagnifierController::shift_within_bounds(unadjusted, screen);

        // **The focal point is pulled in so the loupe never shows anything
        // outside the field.** Half the loupe's width, divided by the
        // magnification, is how much *source* it shows either side of its
        // centre.
        let max_edge_inset = (size.width / 2.0) / Magnifier::MAGNIFICATION_SCALE;
        let adjusted_centre_x = (adjusted.left + adjusted.right) / 2.0;
        let field_centre_x = (info.field_bounds.left + info.field_bounds.right) / 2.0;
        let new_global_focal_x = if info.field_bounds.width() < max_edge_inset * 2.0 {
            // A field narrower than the loupe's own view cannot avoid showing
            // something outside itself, so upstream stops trying and points at
            // the middle -- a fixed wrong is easier to read than one that
            // slides about.
            field_centre_x
        } else {
            adjusted_centre_x.clamp(
                info.field_bounds.left + max_edge_inset,
                info.field_bounds.right - max_edge_inset,
            )
        };

        MagnifierPlacement {
            position: Offset::new(adjusted.left, adjusted.top),
            extra_focal_point_offset: Offset::new(
                new_global_focal_x - adjusted_centre_x,
                // **When the loupe is pushed down off the top of the screen,
                // the focal point moves by exactly as much.** Otherwise a
                // loupe held against the top edge would start showing the
                // wrong line -- the shift that keeps it on screen has to be
                // undone in what it looks at.
                unadjusted.top - adjusted.top,
            ),
            // **Only a vertical move is animated.** Sliding along a line
            // should feel attached to the finger, so the x tracks directly;
            // jumping to another line should read as a jump, so the y eases.
            animate: previous.is_some_and(|previous| previous.dy != adjusted.top),
            shown: true,
        }
    }
}

/// Upstream `CupertinoTextMagnifier` (`cupertino/magnifier.dart`): the iOS
/// loupe's positioning, which is a different set of decisions.
pub struct CupertinoTextMagnifier;

impl CupertinoTextMagnifier {
    /// Upstream's `dragResistance` default.
    pub const DRAG_RESISTANCE: f32 = 10.0;
    /// Upstream's `hideBelowThreshold` default.
    pub const HIDE_BELOW_THRESHOLD: f32 = 48.0;
    /// Upstream's `horizontalScreenEdgePadding` default.
    pub const HORIZONTAL_SCREEN_EDGE_PADDING: f32 = 10.0;
    /// Upstream's `CupertinoMagnifier.kMagnifierAboveFocalPoint`, which is
    /// **negative**: the loupe sits above the point it magnifies.
    pub const MAGNIFIER_ABOVE_FOCAL_POINT: f32 = -26.0;

    /// Upstream's `_determineMagnifierPositionAndFocalPoint`.
    ///
    /// Three decisions differ from the Material one, and each is iOS being
    /// iOS:
    ///
    /// * **It hides rather than follows.** Dragged further than
    ///   `hideBelowThreshold` below the line, the loupe goes away entirely --
    ///   a reader who has pulled that far down has stopped aiming at text.
    /// * **It resists downward drag.** The lens never rises above the line's
    ///   centre, and going down it moves at a tenth of the finger's rate, so
    ///   it lags behind and stays legible instead of chasing.
    /// * **It does not reposition vertically for the screen.** Upstream
    ///   expands the bounds vertically by the loupe's own height so that
    ///   `shiftWithinBounds` constrains x only -- with a comment saying so.
    ///   An iOS loupe is allowed to go off the top; a Material one is not.
    pub fn place(info: MagnifierInfo, screen: Rect) -> MagnifierPlacement {
        let size = CupertinoMagnifier::DEFAULT_SIZE;
        let line_centre_y = (info.caret_rect.top + info.caret_rect.bottom) / 2.0;
        let below = line_centre_y - info.global_gesture_position.dy;

        if below < -CupertinoTextMagnifier::HIDE_BELOW_THRESHOLD {
            return MagnifierPlacement {
                position: Offset::ZERO,
                extra_focal_point_offset: Offset::ZERO,
                animate: false,
                shown: false,
            };
        }

        // `max(centre, centre - below/resistance)`: at or above the line the
        // lens sits on the line, and below it the drag is divided down.
        let lens_y =
            line_centre_y.max(line_centre_y - below / CupertinoTextMagnifier::DRAG_RESISTANCE);
        let raw = Rect::ltrb(
            info.global_gesture_position.dx - size.width / 2.0,
            lens_y - (size.height - CupertinoTextMagnifier::MAGNIFIER_ABOVE_FOCAL_POINT),
            info.global_gesture_position.dx + size.width / 2.0,
            lens_y - (size.height - CupertinoTextMagnifier::MAGNIFIER_ABOVE_FOCAL_POINT)
                + size.height,
        );

        // The vertical slack upstream adds so that only x is constrained.
        let slack = size.height + CupertinoTextMagnifier::MAGNIFIER_ABOVE_FOCAL_POINT;
        let padding = CupertinoTextMagnifier::HORIZONTAL_SCREEN_EDGE_PADDING;
        let bounds = Rect::ltrb(
            screen.left + padding,
            screen.top - slack,
            screen.right - padding,
            screen.bottom + slack,
        );
        let adjusted = MagnifierController::shift_within_bounds(raw, bounds);

        MagnifierPlacement {
            position: Offset::new(adjusted.left, adjusted.top),
            // **However far the lens lagged, the focal point makes it up.**
            // That is what lets the loupe drift behind the finger and still
            // show the line the caret is on.
            extra_focal_point_offset: Offset::new(0.0, line_centre_y - lens_y),
            animate: false,
            shown: true,
        }
    }
}

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

    fn screen() -> Rect {
        Rect::ltrb(0.0, 0.0, 400.0, 800.0)
    }

    /// A caret on a line running the field's width, with the finger on it.
    fn info_at(gesture_x: f32, gesture_y: f32, caret_y: f32) -> MagnifierInfo {
        MagnifierInfo::new(
            Offset::new(gesture_x, gesture_y),
            Rect::ltrb(
                gesture_x - 1.0,
                caret_y - 8.0,
                gesture_x + 1.0,
                caret_y + 8.0,
            ),
            Rect::ltrb(50.0, caret_y - 10.0, 350.0, caret_y + 10.0),
            Rect::ltrb(20.0, 100.0, 380.0, 600.0),
        )
    }

    #[test]
    fn the_material_loupe_tracks_the_finger_but_never_leaves_the_line() {
        // Dragging past the end of a line would otherwise show the reader
        // empty space beside the text they are placing a caret in.
        let inside = TextMagnifier::place(info_at(200.0, 300.0, 300.0), screen(), None);
        let centre = inside.position.dx + Magnifier::DEFAULT_SIZE.width / 2.0;
        assert!((centre - 200.0).abs() < 0.01, "over the finger");

        let past_the_end = TextMagnifier::place(info_at(390.0, 300.0, 300.0), screen(), None);
        let clamped = past_the_end.position.dx + Magnifier::DEFAULT_SIZE.width / 2.0;
        assert!((clamped - 350.0).abs() < 0.01, "stopped at the line's end");
    }

    #[test]
    fn the_material_loupe_follows_the_caret_vertically_not_the_finger() {
        // The finger may be anywhere below; the loupe belongs over the line.
        let high_finger = TextMagnifier::place(info_at(200.0, 250.0, 300.0), screen(), None);
        let low_finger = TextMagnifier::place(info_at(200.0, 500.0, 300.0), screen(), None);
        assert_eq!(high_finger.position.dy, low_finger.position.dy);
    }

    #[test]
    fn only_a_vertical_move_is_animated() {
        // Sliding along a line should feel attached to the finger; jumping to
        // another line should read as a jump.
        let first = TextMagnifier::place(info_at(200.0, 300.0, 300.0), screen(), None);
        assert!(!first.animate, "the first appearance never animates");

        let along =
            TextMagnifier::place(info_at(250.0, 300.0, 300.0), screen(), Some(first.position));
        assert!(!along.animate, "same line, tracked directly");

        let to_another_line =
            TextMagnifier::place(info_at(250.0, 340.0, 340.0), screen(), Some(first.position));
        assert!(to_another_line.animate);
    }

    #[test]
    fn a_loupe_pushed_off_the_top_moves_its_focal_point_by_as_much() {
        // Otherwise a loupe held against the top edge would start showing the
        // wrong line: the shift that keeps it on screen has to be undone in
        // what it looks at.
        let near_top = TextMagnifier::place(info_at(200.0, 10.0, 10.0), screen(), None);
        assert_eq!(near_top.position.dy, 0.0, "held against the edge");
        assert!(
            near_top.extra_focal_point_offset.dy < 0.0,
            "and looking back up by the same amount: {:?}",
            near_top.extra_focal_point_offset
        );

        // Well away from the edge there is nothing to undo.
        let middle = TextMagnifier::place(info_at(200.0, 400.0, 400.0), screen(), None);
        assert_eq!(middle.extra_focal_point_offset.dy, 0.0);
    }

    #[test]
    fn a_field_narrower_than_the_loupes_own_view_gets_a_fixed_focal_point() {
        // It cannot avoid showing something outside itself, so upstream stops
        // trying and points at the middle -- a fixed wrong is easier to read
        // than one that slides about.
        let narrow = MagnifierInfo::new(
            Offset::new(200.0, 300.0),
            Rect::ltrb(199.0, 292.0, 201.0, 308.0),
            Rect::ltrb(190.0, 290.0, 210.0, 310.0),
            Rect::ltrb(190.0, 100.0, 210.0, 600.0),
        );
        let placed = TextMagnifier::place(narrow, screen(), None);
        let centre = placed.position.dx + Magnifier::DEFAULT_SIZE.width / 2.0;
        let focal = centre + placed.extra_focal_point_offset.dx;
        assert!((focal - 200.0).abs() < 0.01, "the field's own centre");
    }

    #[test]
    fn the_ios_loupe_hides_when_dragged_far_below_the_line() {
        // A reader who has pulled that far down has stopped aiming at text, so
        // iOS takes the loupe away rather than following them off it.
        let just_below = CupertinoTextMagnifier::place(info_at(200.0, 340.0, 300.0), screen());
        assert!(just_below.shown);

        let far_below = CupertinoTextMagnifier::place(info_at(200.0, 400.0, 300.0), screen());
        assert!(!far_below.shown, "past the 48-pixel threshold");
    }

    #[test]
    fn the_ios_loupe_resists_a_downward_drag_and_never_rises_above_the_line() {
        // It moves at a tenth of the finger's rate going down, so it lags and
        // stays legible instead of chasing.
        let on_the_line = CupertinoTextMagnifier::place(info_at(200.0, 300.0, 300.0), screen());
        let dragged_down = CupertinoTextMagnifier::place(info_at(200.0, 340.0, 300.0), screen());
        let moved = dragged_down.position.dy - on_the_line.position.dy;
        assert!(moved > 0.0 && moved < 40.0 / 2.0, "lagging: {moved}");

        // And a finger *above* the line does not lift the lens at all.
        let dragged_up = CupertinoTextMagnifier::place(info_at(200.0, 260.0, 300.0), screen());
        assert_eq!(dragged_up.position.dy, on_the_line.position.dy);
    }

    #[test]
    fn however_far_the_ios_lens_lags_its_focal_point_makes_it_up() {
        // Which is what lets the loupe drift behind the finger and still show
        // the line the caret is on.
        let on_the_line = CupertinoTextMagnifier::place(info_at(200.0, 300.0, 300.0), screen());
        assert_eq!(on_the_line.extra_focal_point_offset.dy, 0.0);

        let dragged = CupertinoTextMagnifier::place(info_at(200.0, 340.0, 300.0), screen());
        assert!(dragged.extra_focal_point_offset.dy < 0.0);
    }

    #[test]
    fn the_ios_loupe_is_held_off_the_screen_sides_but_not_the_top() {
        // Upstream expands the bounds vertically by the loupe's own height so
        // that the shift constrains x only, with a comment saying so: an iOS
        // loupe may go off the top where a Material one may not.
        let at_the_left = CupertinoTextMagnifier::place(info_at(5.0, 300.0, 300.0), screen());
        assert_eq!(
            at_the_left.position.dx,
            CupertinoTextMagnifier::HORIZONTAL_SCREEN_EDGE_PADDING,
            "held off the side"
        );

        let at_the_top = CupertinoTextMagnifier::place(info_at(200.0, 5.0, 5.0), screen());
        assert!(
            at_the_top.position.dy < 0.0,
            "and allowed off the top: {:?}",
            at_the_top.position
        );
        // Where the Material one is not.
        let material_top = TextMagnifier::place(info_at(200.0, 5.0, 5.0), screen(), None);
        assert_eq!(material_top.position.dy, 0.0);
    }
}
