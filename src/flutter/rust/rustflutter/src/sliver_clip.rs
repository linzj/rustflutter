//! Ports of `rendering/sliver_clip.dart` and `widgets/sliver_clip.dart`:
//! [`ClipOverlapBehavior`], [`RenderSliverClipRect`], [`RenderSliverClipRRect`],
//! [`SliverClipRect`] and [`SliverClipRRect`].
//!
//! A box that clips its child has one question to answer -- what shape. A
//! *sliver* that clips its child has a second one, and it is the whole reason
//! this file exists apart from `ClipRect`: **what to do about the region a
//! pinned sliver is painting over.**
//!
//! The situation is a `CustomScrollView` whose first sliver is a pinned
//! `SliverAppBar` with a translucent background. The bar reports a
//! `layoutExtent` smaller than its `paintExtent`, so the sliver after it is
//! laid out at a scroll position the bar is drawing on top of. That difference
//! arrives as [`SliverConstraints::overlap`]. Nothing clips it by default: the
//! list's items scroll *under* the bar and, because the bar is translucent,
//! show through it.
//!
//! Clipping the child to the sliver's own bounds does not fix that -- the
//! overlapped strip is inside those bounds. The clip has to be shortened by
//! the overlap, and [`ClipOverlapBehavior`] is the three answers to how.

use crate::borders::{BorderRadiusGeometry, RRect};
use crate::direction::TextDirection;
use crate::engine::Rect;
use crate::painting::ClipBehavior;
use crate::render::{
    Axis, AxisDirection, Offset, Size, SliverConstraints, SliverGeometry,
    apply_growth_direction_to_axis_direction,
};

/// Upstream `ClipOverlapBehavior`: how a sliver's clip reacts to the area
/// other slivers overlap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClipOverlapBehavior {
    /// The clip ignores the overlap entirely.
    ///
    /// Not the same as "no clip": the child is still clipped to the sliver's
    /// bounds, it is just that the overlapped strip is inside them. Content
    /// there renders underneath a translucent pinned sliver, which is what
    /// every sliver did before this class existed.
    None,
    /// The clip's leading edge is truncated at the overlap boundary.
    ///
    /// The default, and the only one a plain rectangle can distinguish.
    #[default]
    FollowEdge,
    /// The clip's leading edge starts moving **earlier**, so that a rounded
    /// corner is never sheared off by the overlap boundary.
    ///
    /// This is only meaningful for a shape with corners. See
    /// [`RenderSliverClipRRect::build_clip`] for what "earlier" means as a
    /// number; on [`RenderSliverClipRect`] it is documented to behave exactly
    /// like [`ClipOverlapBehavior::FollowEdge`], and
    /// [`RenderSliverClipRect::preserve_shape_is_follow_edge`] is that claim
    /// made checkable.
    PreserveShape,
}

/// Upstream `RenderSliver.getMaxPaintRect`: the rectangle covering everything
/// the sliver could paint, in the sliver's own painting coordinates.
///
/// Not the visible rectangle. It spans `maxPaintExtent` -- what the sliver
/// *would* report with an infinite viewport -- shifted back by how far it has
/// been scrolled, so a list scrolled halfway gets a rect whose top is
/// negative. That is the point: the clip is meant to travel with the content,
/// and a rect that only covered the visible part would clip the child against
/// a box that stands still while the child slides through it.
///
/// Two details that are easy to get wrong, both taken from upstream:
///
/// * An infinite `max_paint_extent` (an unbounded lazy list) is replaced with
///   `scroll_offset + cache_extent + cache_origin` -- a finite rect covering
///   what has actually been built, since there is nothing else to measure.
/// * The leading offset is capped at
///   `scroll_extent - max_scroll_obstruction_extent`, so a **pinned** sliver's
///   rect stops receding once the sliver is holding still at the edge. Without
///   the cap its clip would keep sliding away and eventually leave the pinned
///   content entirely.
pub fn max_paint_rect(constraints: &SliverConstraints, geometry: &SliverGeometry) -> Rect {
    if *geometry == SliverGeometry::ZERO {
        return Rect::ltrb(0.0, 0.0, 0.0, 0.0);
    }

    let max_paint_extent = if geometry.max_paint_extent.is_infinite() {
        constraints.scroll_offset + geometry.cache_extent + constraints.cache_origin
    } else {
        geometry.max_paint_extent
    };
    let paint_extent = geometry.paint_extent;
    let leading_offset = constraints.scroll_offset.clamp(
        0.0,
        (geometry.scroll_extent - geometry.max_scroll_obstruction_extent).max(0.0),
    );
    let cross_axis_extent = geometry
        .cross_axis_extent
        .unwrap_or(constraints.cross_axis_extent);

    let rect = match constraints.axis() {
        Axis::Horizontal => Rect::xywh(-leading_offset, 0.0, max_paint_extent, cross_axis_extent),
        Axis::Vertical => Rect::xywh(0.0, -leading_offset, cross_axis_extent, max_paint_extent),
    };

    // Reversing the axis mirrors the rect within the painted extent, because
    // painting coordinates always run down and right while the sliver's scroll
    // offset may run the other way.
    match apply_growth_direction_to_axis_direction(
        constraints.axis_direction,
        constraints.growth_direction,
    ) {
        AxisDirection::Right | AxisDirection::Down => rect,
        AxisDirection::Left => Rect::ltrb(
            paint_extent - rect.right,
            rect.top,
            paint_extent - rect.left,
            rect.bottom,
        ),
        AxisDirection::Up => Rect::ltrb(
            rect.left,
            paint_extent - rect.bottom,
            rect.right,
            paint_extent - rect.top,
        ),
    }
}

/// Upstream `_RenderSliverCustomClip.getClipOriginForOverlap`: the main-axis
/// offset the clip's leading edge should start at.
///
/// This is the arithmetic the whole file turns on, and it is four lines that
/// each say something:
///
/// ```dart
/// final double effectiveOverlap = math.max(0.0, constraints.overlap);
/// final double flexibleClipExtent =
///     math.max(0.0, insideClipExtent - geometry!.maxScrollObstructionExtent);
/// final double minClipOrigin = -math.min(flexibleClipExtent, constraints.scrollOffset);
/// return clampDouble(flexibleClipExtent - constraints.scrollOffset, minClipOrigin, effectiveOverlap);
/// ```
///
/// * **The overlap is floored at zero.** `constraints.overlap` goes negative
///   in a reversed growth direction, and a negative overlap here would push
///   the clip's edge past the sliver's own leading edge -- clipping away
///   content nothing is covering.
/// * **`flexibleClipExtent` is how much of the clip may slide under the
///   overlap before its edge has to move.** The obstruction extent is
///   subtracted because a pinned sliver's own content never scrolls away, so
///   that part of the clip has no slack.
/// * **The clamp's low end is negative**, and only as negative as the content
///   already scrolled. Once the sliver has travelled far enough, the clip's
///   edge leads its bounds instead of trailing them.
/// * **The clamp's high end is the overlap itself**, which is the resting
///   state: while there is slack, the clip's edge sits exactly on the
///   overlap boundary. That is [`ClipOverlapBehavior::FollowEdge`].
///
/// `inside_clip_extent` is the caller's choice, and it is the *only* thing
/// [`ClipOverlapBehavior::PreserveShape`] changes.
pub fn clip_origin_for_overlap(
    constraints: &SliverConstraints,
    geometry: &SliverGeometry,
    inside_clip_extent: f32,
) -> f32 {
    let effective_overlap = constraints.overlap.max(0.0);
    let flexible_clip_extent =
        (inside_clip_extent - geometry.max_scroll_obstruction_extent).max(0.0);
    // Both terms are non-negative, so this is never above zero -- and
    // `effective_overlap` is never below it. The clamp's ends cannot cross.
    let min_clip_origin = -flexible_clip_extent.min(constraints.scroll_offset.max(0.0));
    (flexible_clip_extent - constraints.scroll_offset).clamp(min_clip_origin, effective_overlap)
}

/// Moves whichever edge of `clip` leads the scroll to `origin`, leaving the
/// other three alone.
///
/// The four arms are one idea seen from four directions: the *leading* edge in
/// painting coordinates is the top when the axis runs down, the bottom when it
/// runs up, and so on -- and for the two reversed directions the origin is
/// measured back from `paint_extent` rather than forward from zero.
///
/// `max`/`min` rather than assignment: the origin only ever pulls the edge
/// inwards. A clip already shorter than the overlap is not lengthened to reach
/// it.
fn clip_leading_edge(
    clip: Rect,
    axis_direction: AxisDirection,
    paint_extent: f32,
    origin: f32,
) -> Rect {
    match axis_direction {
        AxisDirection::Down => Rect::ltrb(clip.left, clip.top.max(origin), clip.right, clip.bottom),
        AxisDirection::Up => Rect::ltrb(
            clip.left,
            clip.top,
            clip.right,
            clip.bottom.min(paint_extent - origin),
        ),
        AxisDirection::Right => {
            Rect::ltrb(clip.left.max(origin), clip.top, clip.right, clip.bottom)
        }
        AxisDirection::Left => Rect::ltrb(
            clip.left,
            clip.top,
            clip.right.min(paint_extent - origin),
            clip.bottom,
        ),
    }
}

/// Upstream `RenderSliverClipRect`: a sliver that clips its child to a
/// rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderSliverClipRect {
    /// What a `CustomClipper<Rect>` answered, in the **child's** coordinates
    /// (`getClip(maxPaintRect.size)`), or `None` for no clipper.
    ///
    /// Upstream shifts it by `maxPaintRect.topLeft` before use, and that shift
    /// is not bookkeeping: `topLeft` is negative for a scrolled sliver, so a
    /// clipper that returns `Offset.zero & size` -- the obvious way to write
    /// "all of it" -- lands somewhere different from `maxPaintRect` unless it
    /// is shifted. See [`RenderSliverClipRect::build_clip`].
    pub clipper: Option<Rect>,
    /// Upstream's default is [`ClipBehavior::HardEdge`]: a rectangle has no
    /// diagonal edges to alias, so anti-aliasing it would cost a save layer
    /// for nothing.
    pub clip_behavior: ClipBehavior,
    pub clip_overlap: ClipOverlapBehavior,
}

impl Default for RenderSliverClipRect {
    fn default() -> Self {
        RenderSliverClipRect::new()
    }
}

impl RenderSliverClipRect {
    pub fn new() -> RenderSliverClipRect {
        RenderSliverClipRect {
            clipper: None,
            clip_behavior: ClipBehavior::HardEdge,
            clip_overlap: ClipOverlapBehavior::FollowEdge,
        }
    }

    /// Upstream `buildClip`.
    ///
    /// The rectangle is the clipper's (shifted onto the paint rect) or the
    /// paint rect itself, and then -- unless the overlap is being ignored --
    /// its leading edge is pulled in to
    /// [`clip_origin_for_overlap`].
    ///
    /// The extent handed to that function is the clip's **full** width or
    /// height, in every mode. That is the sentence that makes
    /// [`ClipOverlapBehavior::PreserveShape`] a synonym for
    /// [`ClipOverlapBehavior::FollowEdge`] here: with nothing but square
    /// corners there is no inner rectangle to measure instead.
    pub fn build_clip(&self, constraints: &SliverConstraints, geometry: &SliverGeometry) -> Rect {
        let max_paint = max_paint_rect(constraints, geometry);
        let mut new_clip = match self.clipper {
            Some(clip) => shift(clip, Offset::new(max_paint.left, max_paint.top)),
            None => max_paint,
        };

        if self.clip_overlap != ClipOverlapBehavior::None {
            let clip_extent = match constraints.axis() {
                Axis::Horizontal => new_clip.width(),
                Axis::Vertical => new_clip.height(),
            };
            let origin = clip_origin_for_overlap(constraints, geometry, clip_extent);
            new_clip = clip_leading_edge(
                new_clip,
                apply_growth_direction_to_axis_direction(
                    constraints.axis_direction,
                    constraints.growth_direction,
                ),
                geometry.paint_extent,
                origin,
            );
        }
        new_clip
    }

    /// Upstream's documented claim that the two behaviours coincide on a plain
    /// rectangle, as something that can be checked rather than believed.
    pub fn preserve_shape_is_follow_edge(
        constraints: &SliverConstraints,
        geometry: &SliverGeometry,
        clipper: Option<Rect>,
    ) -> bool {
        let follow = RenderSliverClipRect {
            clipper,
            clip_overlap: ClipOverlapBehavior::FollowEdge,
            ..RenderSliverClipRect::new()
        };
        let preserve = RenderSliverClipRect {
            clip_overlap: ClipOverlapBehavior::PreserveShape,
            ..follow
        };
        follow.build_clip(constraints, geometry) == preserve.build_clip(constraints, geometry)
    }

    /// Upstream `clipContains`, the hit-test shape.
    pub fn clip_contains(offset: Offset, clip: Rect) -> bool {
        contains(clip, offset)
    }
}

/// Upstream `RenderSliverClipRRect`: the same, with rounded corners.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderSliverClipRRect {
    /// Ignored entirely when [`clipper`](Self::clipper) is set -- upstream
    /// says so in the constructor's doc and then reads the clipper first in
    /// `buildClip`, so a caller who sets both silently loses the radius.
    pub border_radius: BorderRadiusGeometry,
    pub clipper: Option<RRect>,
    /// Upstream's default here is [`ClipBehavior::AntiAlias`], not
    /// `HardEdge` as on [`RenderSliverClipRect`]. The defaults differ because
    /// the shapes do: a curve rasterized hard-edged is a visibly jagged
    /// corner, and a rectangle anti-aliased is a save layer bought for nothing.
    pub clip_behavior: ClipBehavior,
    pub clip_overlap: ClipOverlapBehavior,
    pub text_direction: Option<TextDirection>,
}

impl Default for RenderSliverClipRRect {
    fn default() -> Self {
        RenderSliverClipRRect::new()
    }
}

impl RenderSliverClipRRect {
    pub fn new() -> RenderSliverClipRRect {
        RenderSliverClipRRect {
            border_radius: BorderRadiusGeometry::Zero,
            clipper: None,
            clip_behavior: ClipBehavior::AntiAlias,
            clip_overlap: ClipOverlapBehavior::FollowEdge,
            text_direction: None,
        }
    }

    /// Upstream `buildClip`.
    ///
    /// Identical to the rectangle's except for one line, and that line is the
    /// entire difference between the two overlap behaviours:
    ///
    /// ```dart
    /// final double insideClipExtent = switch ((constraints.axis, clipOverlap)) {
    ///   (Axis.horizontal, ClipOverlapBehavior.preserveShape) => newClip.middleRect.width,
    ///   (Axis.vertical, ClipOverlapBehavior.preserveShape) => newClip.middleRect.height,
    ///   (Axis.horizontal, _) => newClip.width,
    ///   (Axis.vertical, _) => newClip.height,
    /// };
    /// ```
    ///
    /// `middleRect` is the rectangle left over between the corner radii, so it
    /// is **shorter** than the clip by the top and bottom radii. Feeding a
    /// shorter extent to [`clip_origin_for_overlap`] shrinks
    /// `flexibleClipExtent`, which means `flexible - scrollOffset` falls below
    /// the overlap **at a smaller scroll offset**. In plain terms: the clip's
    /// leading edge leaves the overlap boundary and starts travelling with the
    /// content sooner, by exactly the height of the corners -- which is the
    /// distance over which a corner would otherwise be sitting on the boundary
    /// being sheared flat.
    ///
    /// The corners themselves are carried through unchanged; upstream rebuilds
    /// the `RRect` with `RRect.fromLTRBAndCorners` and the four original
    /// radii. Nothing about the shape changes -- only when it starts moving.
    pub fn build_clip(&self, constraints: &SliverConstraints, geometry: &SliverGeometry) -> RRect {
        let max_paint = max_paint_rect(constraints, geometry);
        let mut new_clip = match self.clipper {
            Some(clip) => RRect {
                rect: shift(clip.rect, Offset::new(max_paint.left, max_paint.top)),
                ..clip
            },
            None => self
                .border_radius
                .resolve(self.text_direction.unwrap_or(TextDirection::Ltr))
                .to_rrect(max_paint),
        };

        if self.clip_overlap != ClipOverlapBehavior::None {
            let middle = middle_rect(&new_clip);
            let inside_clip_extent = match (constraints.axis(), self.clip_overlap) {
                (Axis::Horizontal, ClipOverlapBehavior::PreserveShape) => middle.width(),
                (Axis::Vertical, ClipOverlapBehavior::PreserveShape) => middle.height(),
                (Axis::Horizontal, _) => new_clip.rect.width(),
                (Axis::Vertical, _) => new_clip.rect.height(),
            };
            let origin = clip_origin_for_overlap(constraints, geometry, inside_clip_extent);
            new_clip = RRect {
                rect: clip_leading_edge(
                    new_clip.rect,
                    apply_growth_direction_to_axis_direction(
                        constraints.axis_direction,
                        constraints.growth_direction,
                    ),
                    geometry.paint_extent,
                    origin,
                ),
                ..new_clip
            };
        }
        new_clip
    }

    /// Upstream `clipContains`, which is `RRect.contains` -- corner-aware, so
    /// a tap in the cut-away corner of a rounded clip misses the child even
    /// though it is inside the bounding box.
    pub fn clip_contains(offset: Offset, clip: &RRect) -> bool {
        clip.contains(offset)
    }
}

/// Upstream `RRect.middleRect`: the rectangle between the corner radii.
///
/// Each side is pulled in by the larger of the two radii that meet it, so the
/// result is the largest rectangle guaranteed to be inside the rounded one.
pub fn middle_rect(rrect: &RRect) -> Rect {
    let left_radius = rrect.bottom_left.x.max(rrect.top_left.x);
    let top_radius = rrect.top_left.y.max(rrect.top_right.y);
    let right_radius = rrect.top_right.x.max(rrect.bottom_right.x);
    let bottom_radius = rrect.bottom_right.y.max(rrect.bottom_left.y);
    Rect::ltrb(
        rrect.rect.left + left_radius,
        rrect.rect.top + top_radius,
        rrect.rect.right - right_radius,
        rrect.rect.bottom - bottom_radius,
    )
}

/// The clip cache shared by both render objects, upstream
/// `_RenderSliverCustomClip`'s `_clip` / `_getClip` / `markNeedsClip`.
///
/// The clip is **not** recomputed when something writes to the render object.
/// It is thrown away, the object is marked for paint, and `buildClip` runs
/// lazily in the next paint or hit test. Upstream's own comment says why:
/// several writes in one frame would each recompute a clip that only the last
/// one gets to keep, and a clip is only ever needed at paint time anyway.
///
/// The state is generic over the clip shape so both spellings share it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SliverClipCache<T> {
    clip: Option<T>,
    /// How many times the cached clip was invalidated -- upstream's
    /// `markNeedsPaint` + `markNeedsSemanticsUpdate` seen from outside.
    invalidations: usize,
}

impl<T: Copy> SliverClipCache<T> {
    pub fn new() -> SliverClipCache<T> {
        SliverClipCache {
            clip: None,
            invalidations: 0,
        }
    }

    /// Upstream `_getClip`, whose first branch is the load-bearing one:
    /// [`ClipBehavior::None`] does not mean "clip to everything", it means
    /// **no clip layer at all**, and the cache is cleared rather than filled.
    /// The caller paints the child directly, so a `Clip.none` sliver costs no
    /// layer.
    pub fn get_clip(
        &mut self,
        clip_behavior: ClipBehavior,
        build: impl FnOnce() -> T,
    ) -> Option<T> {
        if clip_behavior == ClipBehavior::None {
            self.clip = None;
        } else if self.clip.is_none() {
            self.clip = Some(build());
        }
        self.clip
    }

    /// Upstream `markNeedsClip`, and also `performLayout`'s `_clip = null`:
    /// a new layout invalidates the clip unconditionally, because
    /// `getMaxPaintRect` reads the geometry that just changed.
    pub fn mark_needs_clip(&mut self) {
        self.clip = None;
        self.invalidations += 1;
    }

    pub fn cached(&self) -> Option<T> {
        self.clip
    }

    pub fn invalidations(&self) -> usize {
        self.invalidations
    }
}

/// Upstream `_RenderSliverCustomClip.hitTest`'s coordinate change.
///
/// A sliver hit test arrives as a (main axis, cross axis) pair; the clip is a
/// rectangle in painting coordinates. The four arms convert one to the other,
/// and the two reversed directions measure the main axis back from
/// `paint_extent` -- the same mirroring [`max_paint_rect`] does, for the same
/// reason.
///
/// Returns `None` when `clip_behavior` is [`ClipBehavior::None`], where
/// upstream skips the test entirely and lets every hit through.
pub fn hit_test_offset(
    constraints: &SliverConstraints,
    geometry: &SliverGeometry,
    clip_behavior: ClipBehavior,
    main_axis_position: f32,
    cross_axis_position: f32,
) -> Option<Offset> {
    if clip_behavior == ClipBehavior::None {
        return None;
    }
    Some(
        match apply_growth_direction_to_axis_direction(
            constraints.axis_direction,
            constraints.growth_direction,
        ) {
            AxisDirection::Down => Offset::new(cross_axis_position, main_axis_position),
            AxisDirection::Right => Offset::new(main_axis_position, cross_axis_position),
            AxisDirection::Up => Offset::new(
                cross_axis_position,
                geometry.paint_extent - main_axis_position,
            ),
            AxisDirection::Left => Offset::new(
                geometry.paint_extent - main_axis_position,
                cross_axis_position,
            ),
        },
    )
}

/// Upstream `describeApproximatePaintClip`, which the compositor uses to
/// decide what it may cull.
///
/// Note what it does **not** consult: `clipOverlap`. The approximate clip is
/// the clipper's own rect or the whole paint rect, never the overlap-shortened
/// one. Reporting the shortened rect would let the compositor cull content
/// that the overlap clip may stop hiding a fraction of a frame later.
pub fn describe_approximate_paint_clip(
    constraints: &SliverConstraints,
    geometry: &SliverGeometry,
    clip_behavior: ClipBehavior,
    approximate_clipper: Option<Rect>,
) -> Option<Rect> {
    if clip_behavior == ClipBehavior::None {
        return None;
    }
    let max_paint = max_paint_rect(constraints, geometry);
    Some(match approximate_clipper {
        Some(clip) => shift(clip, Offset::new(max_paint.left, max_paint.top)),
        None => max_paint,
    })
}

// -- The widgets (upstream widgets/sliver_clip.dart) --------------------------

/// Upstream `SliverClipRect`.
///
/// Everything it does is hand its four values to
/// [`RenderSliverClipRect`], on creation and again on every update. What it
/// contributes is the *defaults*, which is why it is worth a type: a caller
/// writing `SliverClipRect(sliver: ...)` and nothing else gets a hard-edged
/// clip that follows the overlap edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliverClipRect {
    pub clipper: Option<Rect>,
    pub clip_behavior: ClipBehavior,
    pub clip_overlap: ClipOverlapBehavior,
}

impl Default for SliverClipRect {
    fn default() -> Self {
        SliverClipRect::new()
    }
}

impl SliverClipRect {
    pub fn new() -> SliverClipRect {
        SliverClipRect {
            clipper: None,
            clip_behavior: ClipBehavior::HardEdge,
            clip_overlap: ClipOverlapBehavior::FollowEdge,
        }
    }

    /// Upstream `createRenderObject`, which is also `updateRenderObject`: the
    /// same three fields written the same way, so a rebuild cannot produce a
    /// render object that differs from a fresh one.
    pub fn create_render_object(&self) -> RenderSliverClipRect {
        RenderSliverClipRect {
            clipper: self.clipper,
            clip_behavior: self.clip_behavior,
            clip_overlap: self.clip_overlap,
        }
    }
}

/// Upstream `SliverClipRRect`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliverClipRRect {
    pub border_radius: BorderRadiusGeometry,
    pub clipper: Option<RRect>,
    pub clip_behavior: ClipBehavior,
    pub clip_overlap: ClipOverlapBehavior,
}

impl Default for SliverClipRRect {
    fn default() -> Self {
        SliverClipRRect::new()
    }
}

impl SliverClipRRect {
    pub fn new() -> SliverClipRRect {
        SliverClipRRect {
            border_radius: BorderRadiusGeometry::Zero,
            clipper: None,
            clip_behavior: ClipBehavior::AntiAlias,
            clip_overlap: ClipOverlapBehavior::FollowEdge,
        }
    }

    /// Upstream `createRenderObject`. The text direction is read from the
    /// ambient `Directionality` with `maybeOf` rather than `of` -- a sliver
    /// clip with a symmetric radius is perfectly usable with no
    /// `Directionality` above it, and demanding one would make the widget
    /// throw in a place a plain `ClipRRect` would not.
    pub fn create_render_object(
        &self,
        text_direction: Option<TextDirection>,
    ) -> RenderSliverClipRRect {
        RenderSliverClipRRect {
            border_radius: self.border_radius,
            clipper: self.clipper,
            clip_behavior: self.clip_behavior,
            clip_overlap: self.clip_overlap,
            text_direction,
        }
    }
}

// -- Small geometry helpers ---------------------------------------------------

fn shift(rect: Rect, offset: Offset) -> Rect {
    Rect::ltrb(
        rect.left + offset.dx,
        rect.top + offset.dy,
        rect.right + offset.dx,
        rect.bottom + offset.dy,
    )
}

/// Upstream `Rect.contains`, whose bounds are **half open**: the leading edges
/// are inside and the trailing ones are not, so two rectangles that share an
/// edge do not both contain a point on it.
fn contains(rect: Rect, offset: Offset) -> bool {
    offset.dx >= rect.left
        && offset.dx < rect.right
        && offset.dy >= rect.top
        && offset.dy < rect.bottom
}

/// The size of a rect, for a clipper that is handed `maxPaintRect.size`.
pub fn rect_size(rect: Rect) -> Size {
    Size::new(rect.width(), rect.height())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::borders::Radius;
    use crate::render::GrowthDirection;

    /// A card 200 logical pixels tall, in a 400-wide vertical viewport,
    /// scrolled `scroll_offset` with a pinned bar overlapping `overlap`.
    fn card(scroll_offset: f32, overlap: f32) -> (SliverConstraints, SliverGeometry) {
        let constraints = SliverConstraints {
            scroll_offset,
            overlap,
            cross_axis_extent: 400.0,
            remaining_paint_extent: 600.0,
            ..SliverConstraints::default()
        };
        let geometry = SliverGeometry {
            paint_extent: (200.0 - scroll_offset).clamp(0.0, 600.0),
            ..SliverGeometry::new(200.0, 200.0, 200.0, 200.0, false)
        };
        (constraints, geometry)
    }

    // -- The paint rect --------------------------------------------------------

    #[test]
    fn a_sliver_with_no_geometry_has_no_paint_rect() {
        // Upstream's early return, and it is not the redundant guard it looks
        // like. Falling through, everything below computes zero *except* the
        // cross axis extent, which is `null` in a zero geometry and therefore
        // falls back to the constraints' -- giving a 400-wide rect of zero
        // height rather than nothing at all. The first version of this test
        // used the default constraints, whose cross extent is zero, and so
        // could not tell the two apart.
        let constraints = SliverConstraints {
            cross_axis_extent: 400.0,
            ..SliverConstraints::default()
        };
        assert_eq!(
            max_paint_rect(&constraints, &SliverGeometry::ZERO),
            Rect::ltrb(0.0, 0.0, 0.0, 0.0)
        );
    }

    #[test]
    fn the_paint_rect_travels_with_the_content_rather_than_standing_still() {
        // Its top goes negative as the sliver scrolls, so the clip moves with
        // the child. A rect covering only the visible part would be a window
        // the child slides through, which is the opposite of a clip.
        let (constraints, geometry) = card(60.0, 0.0);
        assert_eq!(
            max_paint_rect(&constraints, &geometry),
            Rect::ltrb(0.0, -60.0, 400.0, 140.0)
        );
    }

    #[test]
    fn a_pinned_slivers_rect_stops_receding_once_it_holds_still() {
        // The leading offset is capped at `scrollExtent -
        // maxScrollObstructionExtent`. Without the cap a pinned header's clip
        // would keep sliding away from the content it is pinned over.
        let constraints = SliverConstraints {
            scroll_offset: 500.0,
            cross_axis_extent: 400.0,
            ..SliverConstraints::default()
        };
        let geometry = SliverGeometry {
            max_scroll_obstruction_extent: 56.0,
            ..SliverGeometry::new(200.0, 56.0, 200.0, 56.0, false)
        };
        // Capped at 200 - 56, not the 500 actually scrolled.
        assert_eq!(max_paint_rect(&constraints, &geometry).top, -144.0);
    }

    #[test]
    fn an_unbounded_sliver_is_measured_by_what_it_has_actually_built() {
        // There is no `maxPaintExtent` to use, so upstream substitutes the
        // scrolled distance plus the cache band.
        let constraints = SliverConstraints {
            scroll_offset: 300.0,
            cache_origin: -250.0,
            cross_axis_extent: 400.0,
            ..SliverConstraints::default()
        };
        let geometry = SliverGeometry {
            scroll_extent: f32::INFINITY,
            max_paint_extent: f32::INFINITY,
            ..SliverGeometry::new(f32::INFINITY, 600.0, f32::INFINITY, 850.0, false)
        };
        let rect = max_paint_rect(&constraints, &geometry);
        // 300 + 850 - 250 = 900.
        assert_eq!(rect.height(), 900.0);
        assert!(rect.height().is_finite());
    }

    #[test]
    fn a_reversed_axis_mirrors_the_rect_inside_the_painted_extent() {
        // Painting coordinates always run down and right; the scroll offset
        // may not. The mirror is what reconciles them.
        let (constraints, geometry) = card(60.0, 0.0);
        let up = SliverConstraints {
            axis_direction: AxisDirection::Up,
            ..constraints
        };
        let down = max_paint_rect(&constraints, &geometry);
        let mirrored = max_paint_rect(&up, &geometry);
        assert_eq!(
            mirrored,
            Rect::ltrb(
                down.left,
                geometry.paint_extent - down.bottom,
                down.right,
                geometry.paint_extent - down.top,
            )
        );
    }

    #[test]
    fn a_reversed_growth_direction_flips_the_axis_the_same_way() {
        // `applyGrowthDirectionToAxisDirection` is consulted, not the raw
        // `axisDirection` -- a reversed list scrolling down paints upwards.
        let (constraints, geometry) = card(60.0, 0.0);
        let reversed = SliverConstraints {
            growth_direction: GrowthDirection::Reverse,
            ..constraints
        };
        let up = SliverConstraints {
            axis_direction: AxisDirection::Up,
            ..constraints
        };
        assert_eq!(
            max_paint_rect(&reversed, &geometry),
            max_paint_rect(&up, &geometry)
        );
    }

    // -- The overlap origin ----------------------------------------------------

    #[test]
    fn while_there_is_slack_the_clip_sits_exactly_on_the_overlap_boundary() {
        // That resting state is what `followEdge` means, and it is the clamp's
        // upper end doing it.
        let (constraints, geometry) = card(0.0, 56.0);
        assert_eq!(
            clip_origin_for_overlap(&constraints, &geometry, 200.0),
            56.0
        );
        let (constraints, geometry) = card(100.0, 56.0);
        assert_eq!(
            clip_origin_for_overlap(&constraints, &geometry, 200.0),
            56.0
        );
    }

    #[test]
    fn a_negative_overlap_is_floored_rather_than_pushing_the_clip_inwards() {
        // `constraints.overlap` goes negative in a reversed growth direction.
        // Used as-is it would clip away content nothing is covering.
        let (constraints, geometry) = card(0.0, -40.0);
        assert_eq!(clip_origin_for_overlap(&constraints, &geometry, 200.0), 0.0);
    }

    #[test]
    fn a_pinned_slivers_own_extent_is_not_slack() {
        // The obstruction extent comes off the flexible part: content that
        // never scrolls away cannot slide under the overlap.
        let constraints = SliverConstraints {
            scroll_offset: 150.0,
            overlap: 56.0,
            ..SliverConstraints::default()
        };
        let loose = SliverGeometry::new(200.0, 200.0, 200.0, 200.0, false);
        let pinned = SliverGeometry {
            max_scroll_obstruction_extent: 80.0,
            ..loose
        };
        // 200 - 150 = 50, under the 56 cap.
        assert_eq!(clip_origin_for_overlap(&constraints, &loose, 200.0), 50.0);
        // (200 - 80) - 150 = -30: the pinned part has no slack left to give.
        assert_eq!(clip_origin_for_overlap(&constraints, &pinned, 200.0), -30.0);
    }

    #[test]
    fn the_origin_never_recedes_further_than_the_flexible_extent() {
        // The clamp's low end, `-min(flexibleClipExtent, scrollOffset)`. Only
        // ever `-flexibleClipExtent` in practice: the `min` picks the scroll
        // offset exactly when the scroll offset is the smaller of the two, and
        // in that case `flexible - scrollOffset` is already positive and the
        // low end cannot bind at all. Written out, it is defensive.
        let constraints = SliverConstraints {
            scroll_offset: 100.0,
            overlap: 0.0,
            ..SliverConstraints::default()
        };
        let geometry = SliverGeometry::new(200.0, 200.0, 200.0, 200.0, false);
        // 20 - 100 = -80, floored at -20.
        assert_eq!(
            clip_origin_for_overlap(&constraints, &geometry, 20.0),
            -20.0
        );
        // With no flexible extent at all it cannot recede one pixel, however
        // far the content has scrolled -- which is the case I expected to read
        // -100 and does not.
        assert_eq!(clip_origin_for_overlap(&constraints, &geometry, 0.0), 0.0);
    }

    #[test]
    fn even_with_no_overlap_the_clip_never_reaches_above_the_leading_edge() {
        // `effectiveOverlap` is zero, so the clamp's upper end is zero, so the
        // origin is zero, so the leading edge is pulled from the paint rect's
        // negative top up to the sliver's own leading painted edge. A sliver
        // under nothing at all still has its clip shortened -- which is
        // exactly why the isolating test below has to turn the behaviour off
        // rather than just setting the overlap to zero.
        let (constraints, geometry) = card(60.0, 0.0);
        let clipped = RenderSliverClipRect::new().build_clip(&constraints, &geometry);
        assert_eq!(max_paint_rect(&constraints, &geometry).top, -60.0);
        assert_eq!(clipped.top, 0.0);
    }

    // -- The rectangle clip ----------------------------------------------------

    #[test]
    fn follow_edge_truncates_the_clip_at_the_overlap_boundary() {
        let (constraints, geometry) = card(0.0, 56.0);
        let clipped = RenderSliverClipRect::new().build_clip(&constraints, &geometry);
        assert_eq!(clipped, Rect::ltrb(0.0, 56.0, 400.0, 200.0));
    }

    #[test]
    fn ignoring_the_overlap_leaves_the_paint_rect_exactly_as_it_was() {
        // `none` is not "no clip" -- the child is still clipped to the bounds.
        // It is the overlap shortening that is skipped.
        let (constraints, geometry) = card(0.0, 56.0);
        let ignoring = RenderSliverClipRect {
            clip_overlap: ClipOverlapBehavior::None,
            ..RenderSliverClipRect::new()
        };
        assert_eq!(
            ignoring.build_clip(&constraints, &geometry),
            max_paint_rect(&constraints, &geometry)
        );
    }

    #[test]
    fn preserve_shape_is_follow_edge_on_a_plain_rectangle() {
        // Upstream documents the equality; here it is checked across the whole
        // travel rather than taken on faith.
        for scrolled in [0.0, 40.0, 100.0, 160.0, 199.0, 200.0] {
            let (constraints, geometry) = card(scrolled, 56.0);
            assert!(
                RenderSliverClipRect::preserve_shape_is_follow_edge(&constraints, &geometry, None),
                "at scroll offset {scrolled}"
            );
        }
    }

    #[test]
    fn a_clipper_is_shifted_onto_the_paint_rect_rather_than_used_where_it_sits() {
        // The obvious clipper -- `Offset.zero & size` -- is not the paint rect
        // once the sliver has scrolled, because the paint rect's top is
        // negative. The shift is what makes "all of it" mean all of it.
        let (constraints, geometry) = card(60.0, 0.0);
        let paint_rect = max_paint_rect(&constraints, &geometry);
        let size = rect_size(paint_rect);
        // The overlap step is turned off so this measures the shift alone --
        // see the test above for why zero overlap would not have been enough.
        let whole = RenderSliverClipRect {
            clipper: Some(Rect::xywh(0.0, 0.0, size.width, size.height)),
            clip_overlap: ClipOverlapBehavior::None,
            ..RenderSliverClipRect::new()
        };
        assert_eq!(whole.build_clip(&constraints, &geometry), paint_rect);
    }

    #[test]
    fn the_origin_only_pulls_the_edge_inwards() {
        // `max`, not assignment. A clipper already shorter than the overlap
        // must not be lengthened out to reach it.
        let (constraints, geometry) = card(0.0, 56.0);
        let paint_rect = max_paint_rect(&constraints, &geometry);
        let short = RenderSliverClipRect {
            clipper: Some(Rect::ltrb(0.0, 90.0, 400.0, 150.0)),
            ..RenderSliverClipRect::new()
        };
        let clipped = short.build_clip(&constraints, &geometry);
        assert_eq!(clipped.top, 90.0 + paint_rect.top);
        assert!(clipped.top > 56.0);
    }

    // -- The rounded clip ------------------------------------------------------

    fn rounded(radius: f32) -> RenderSliverClipRRect {
        RenderSliverClipRRect {
            border_radius: BorderRadiusGeometry::circular(radius),
            ..RenderSliverClipRRect::new()
        }
    }

    #[test]
    fn preserve_shape_keeps_a_corner_sized_window_where_follow_edge_has_collapsed() {
        // The card has scrolled until everything still painted lies inside the
        // overlap. `followEdge` closes the clip completely -- the corners are
        // sheared flat on the boundary. `preserveShape` measures the middle
        // rect instead, so the clip's edge left the boundary 48 pixels of
        // scrolling earlier, and a band exactly the height of the two corners
        // is still open for them to slide through.
        let (constraints, geometry) = card(160.0, 56.0);
        let follow = rounded(24.0);
        let preserve = RenderSliverClipRRect {
            clip_overlap: ClipOverlapBehavior::PreserveShape,
            ..follow
        };
        let follow_clip = follow.build_clip(&constraints, &geometry).rect;
        let preserve_clip = preserve.build_clip(&constraints, &geometry).rect;

        assert_eq!(follow_clip.height(), 0.0);
        assert_eq!(preserve_clip.height(), 48.0);
        assert_eq!(preserve_clip.top, -8.0);
    }

    #[test]
    fn the_corners_themselves_are_carried_through_unchanged() {
        // Upstream rebuilds the RRect with the four original radii; only the
        // moved edge differs. "Shifts inwards" is about when the edge starts
        // moving, not about the shape being redrawn.
        let (constraints, geometry) = card(160.0, 56.0);
        let preserve = RenderSliverClipRRect {
            clip_overlap: ClipOverlapBehavior::PreserveShape,
            ..rounded(24.0)
        };
        let clip = preserve.build_clip(&constraints, &geometry);
        for corner in [
            clip.top_left,
            clip.top_right,
            clip.bottom_left,
            clip.bottom_right,
        ] {
            assert_eq!(corner, Radius::circular(24.0));
        }
    }

    #[test]
    fn with_square_corners_the_two_behaviours_meet_again() {
        // The whole difference is the middle rect, and a zero radius makes it
        // the whole rect.
        let (constraints, geometry) = card(160.0, 56.0);
        let follow = rounded(0.0);
        let preserve = RenderSliverClipRRect {
            clip_overlap: ClipOverlapBehavior::PreserveShape,
            ..follow
        };
        assert_eq!(
            follow.build_clip(&constraints, &geometry),
            preserve.build_clip(&constraints, &geometry)
        );
    }

    #[test]
    fn a_clipper_silently_wins_over_the_border_radius() {
        // Upstream reads the clipper first and never falls back, so a caller
        // who sets both loses the radius without being told.
        let (constraints, geometry) = card(0.0, 0.0);
        let both = RenderSliverClipRRect {
            clipper: Some(RRect::from_rect_and_radius(
                Rect::ltrb(0.0, 0.0, 100.0, 100.0),
                Radius::circular(4.0),
            )),
            ..rounded(24.0)
        };
        assert_eq!(
            both.build_clip(&constraints, &geometry).top_left,
            Radius::circular(4.0)
        );
    }

    #[test]
    fn the_middle_rect_is_pulled_in_by_the_larger_radius_on_each_side() {
        let rrect = RRect::from_rect_and_corners(
            Rect::ltrb(0.0, 0.0, 100.0, 100.0),
            Radius::circular(10.0),
            Radius::circular(30.0),
            Radius::circular(5.0),
            Radius::circular(20.0),
        );
        // left: max(bottomLeft.x 20, topLeft.x 10); top: max(tl.y 10, tr.y 30)
        // right: max(tr.x 30, br.x 5); bottom: max(br.y 5, bl.y 20)
        assert_eq!(middle_rect(&rrect), Rect::ltrb(20.0, 30.0, 70.0, 80.0));
    }

    // -- The cache -------------------------------------------------------------

    #[test]
    fn a_clip_none_sliver_asks_for_no_layer_at_all() {
        // Not "a clip covering everything" -- no clip. The child is painted
        // directly and the cache is left empty.
        let mut cache: SliverClipCache<Rect> = SliverClipCache::new();
        let mut built = 0;
        let clip = cache.get_clip(ClipBehavior::None, || {
            built += 1;
            Rect::ltrb(0.0, 0.0, 1.0, 1.0)
        });
        assert_eq!(clip, None);
        assert_eq!(built, 0, "the clip is not even computed");
    }

    #[test]
    fn the_clip_is_built_once_and_reused_until_something_invalidates_it() {
        // Upstream recomputes lazily at paint rather than eagerly on write:
        // several writes in one frame would each build a clip only the last
        // one keeps.
        let mut cache: SliverClipCache<Rect> = SliverClipCache::new();
        let mut built = 0;
        {
            let mut build_once = |cache: &mut SliverClipCache<Rect>| {
                cache.get_clip(ClipBehavior::HardEdge, || {
                    built += 1;
                    Rect::ltrb(0.0, 0.0, 1.0, 1.0)
                })
            };
            assert!(build_once(&mut cache).is_some());
            assert!(build_once(&mut cache).is_some());
        }
        assert_eq!(built, 1);

        cache.mark_needs_clip();
        assert_eq!(cache.cached(), None);
        assert!(
            cache
                .get_clip(ClipBehavior::HardEdge, || {
                    built += 1;
                    Rect::ltrb(0.0, 0.0, 1.0, 1.0)
                })
                .is_some()
        );
        assert_eq!(built, 2);
        assert_eq!(cache.invalidations(), 1);
    }

    // -- Hit testing and culling ----------------------------------------------

    #[test]
    fn the_hit_offset_puts_the_main_axis_on_whichever_axis_it_is() {
        let (constraints, geometry) = card(0.0, 0.0);
        let at = |axis_direction| {
            hit_test_offset(
                &SliverConstraints {
                    axis_direction,
                    ..constraints
                },
                &geometry,
                ClipBehavior::HardEdge,
                30.0,
                7.0,
            )
        };
        assert_eq!(at(AxisDirection::Down), Some(Offset::new(7.0, 30.0)));
        assert_eq!(at(AxisDirection::Right), Some(Offset::new(30.0, 7.0)));
        // Reversed: measured back from the painted extent, 200 here.
        assert_eq!(at(AxisDirection::Up), Some(Offset::new(7.0, 170.0)));
        assert_eq!(at(AxisDirection::Left), Some(Offset::new(170.0, 7.0)));
    }

    #[test]
    fn a_clip_none_sliver_tests_no_hits_against_a_shape() {
        // Upstream skips the whole block, so every hit reaches the child.
        let (constraints, geometry) = card(0.0, 0.0);
        assert_eq!(
            hit_test_offset(&constraints, &geometry, ClipBehavior::None, 30.0, 7.0),
            None
        );
    }

    #[test]
    fn a_rounded_clip_rejects_a_tap_in_the_cut_away_corner() {
        let clip =
            RRect::from_rect_and_radius(Rect::ltrb(0.0, 0.0, 100.0, 100.0), Radius::circular(40.0));
        assert!(RenderSliverClipRRect::clip_contains(
            Offset::new(50.0, 50.0),
            &clip
        ));
        assert!(!RenderSliverClipRRect::clip_contains(
            Offset::new(2.0, 2.0),
            &clip
        ));
        // The same point is inside the bounding rectangle, which is why the
        // rectangle spelling has to be a different method.
        assert!(RenderSliverClipRect::clip_contains(
            Offset::new(2.0, 2.0),
            clip.rect
        ));
    }

    #[test]
    fn the_approximate_clip_ignores_the_overlap() {
        // The compositor culls against it. Handing it the shortened rect would
        // let it drop content the overlap clip may stop hiding next frame.
        let (constraints, geometry) = card(0.0, 56.0);
        let approximate =
            describe_approximate_paint_clip(&constraints, &geometry, ClipBehavior::HardEdge, None);
        assert_eq!(approximate, Some(max_paint_rect(&constraints, &geometry)));
        assert_ne!(
            approximate,
            Some(RenderSliverClipRect::new().build_clip(&constraints, &geometry))
        );
    }

    #[test]
    fn nothing_is_culled_when_nothing_is_clipped() {
        let (constraints, geometry) = card(0.0, 56.0);
        assert_eq!(
            describe_approximate_paint_clip(&constraints, &geometry, ClipBehavior::None, None),
            None
        );
    }

    // -- The widgets -----------------------------------------------------------

    #[test]
    fn the_two_widgets_default_to_different_clip_behaviours() {
        // A curve rasterized hard-edged is a jagged corner; a rectangle
        // anti-aliased is a save layer bought for nothing.
        assert_eq!(SliverClipRect::new().clip_behavior, ClipBehavior::HardEdge);
        assert_eq!(
            SliverClipRRect::new().clip_behavior,
            ClipBehavior::AntiAlias
        );
        assert_eq!(
            SliverClipRect::new().clip_overlap,
            ClipOverlapBehavior::FollowEdge
        );
        assert_eq!(
            SliverClipRRect::new().clip_overlap,
            ClipOverlapBehavior::FollowEdge
        );
    }

    #[test]
    fn a_rebuild_produces_the_same_render_object_a_fresh_one_would() {
        // `createRenderObject` and `updateRenderObject` write the same fields
        // the same way, which is what stops a rebuild from drifting.
        let widget = SliverClipRect {
            clipper: Some(Rect::ltrb(1.0, 2.0, 3.0, 4.0)),
            clip_behavior: ClipBehavior::AntiAlias,
            clip_overlap: ClipOverlapBehavior::None,
        };
        assert_eq!(
            widget.create_render_object().clip_overlap,
            ClipOverlapBehavior::None
        );
        assert_eq!(widget.create_render_object(), widget.create_render_object());
    }

    #[test]
    fn a_rounded_sliver_clip_works_with_no_directionality_above_it() {
        // `Directionality.maybeOf`, not `of`: a symmetric radius needs no text
        // direction, and demanding one would throw where `ClipRRect` does not.
        let widget = SliverClipRRect {
            border_radius: BorderRadiusGeometry::circular(12.0),
            ..SliverClipRRect::new()
        };
        let render = widget.create_render_object(None);
        assert_eq!(render.text_direction, None);
        let (constraints, geometry) = card(0.0, 0.0);
        assert_eq!(
            render.build_clip(&constraints, &geometry).top_left,
            Radius::circular(12.0)
        );
    }
}
