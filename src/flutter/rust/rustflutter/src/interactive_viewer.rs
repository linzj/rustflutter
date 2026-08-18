// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! A pannable, zoomable viewport: the user drags to pan and pinches to scale
//! the child, with the transform clamped to a boundary and a scale range.
//!
//! Upstream this is `InteractiveViewer` over `_InteractiveViewerState` over
//! `TransformationController` (`widgets/interactive_viewer.dart`). The widget
//! half here is [`InteractiveViewer`], a stateful component whose state
//! ([`InteractiveViewerState`]) holds what upstream's `State` holds: the
//! controller, the gesture bookkeeping (`_referenceFocalPoint`, `_scaleStart`,
//! `_currentAxis`, `_gestureType`) and the inertia animation. The boundary and
//! clamping math keeps upstream's names and shapes: [`matrix_translate`] is
//! `_matrixTranslate`, [`matrix_scale`] is `_matrixScale`, [`exceeds_by`] is
//! `_exceedsBy`, and so on.
//!
//! What upstream has and this port does not, deliberately:
//!
//! * **Matrix4.** The engine's transform layer takes a 2D affine, so the
//!   controller's value is an [`Affine2D`] (`[f32; 6]`) rather than a
//!   `Matrix4`. Every upstream method the viewer uses keeps its name on
//!   [`Affine2D`]: `translate_by_double`, `scale_by_double`,
//!   `max_scale_on_axis`, `inverted`, `transform_point`.
//! * **Rotation.** Upstream hardcodes `_rotateEnabled = false` (its TODO at
//!   flutter/flutter#57698); the constant is ported as [`ROTATE_ENABLED`] and
//!   rotation gestures are exactly as unreachable here as they are there.
//!   `_getAxisAlignedBoundingBoxWithRotation` keeps its rotation parameter and
//!   is always called with zero.
//! * **Scale inertia.** Upstream's `_onScaleEnd` runs a friction animation off
//!   `ScaleEndDetails.scaleVelocity`; this crate's `ScaleEvent` carries no
//!   scale velocity (gestures.rs's module docs), so a pinch ends without
//!   inertia. Pan inertia off `DragEndEvent.velocity` is ported in full.
//! * **Listeners on the controller.** Upstream's `TransformationController` is
//!   a `ValueNotifier`; here it is a shared cell. Rebuilds are driven by the
//!   `StateHandle` every mutation goes through, which is the notification the
//!   frame actually needs -- the same call `editable.rs` documents for
//!   `TextEditingController` (its `state_sink`).
//! * **Trackpad scroll.** Upstream's `_receivedPointerSignal` distinguishes a
//!   trackpad pan from a mouse-wheel zoom by pointer kind and a widget flag;
//!   the `ScrollEvent` this crate delivers does not carry the kind, so every
//!   scroll signal is the wheel-zoom branch. `trackpadScrollCausesScale` and
//!   the `InteractiveViewer.builder` constructor are not carried.
//! * **`onInteraction*` details.** The callbacks take no arguments: the only
//!   thing the gallery's demo reads from them is that an interaction started,
//!   and the gesture events themselves are already delivered through the
//!   gesture handlers.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::animation::Curve;
use crate::engine::Rect;
use crate::framework::{
    AnyWidget, BuildContext, Key, StateHandle, StatefulComponent, single, stateful,
};
use crate::gestures::{
    DragEndEvent, DragEvent, MIN_FLING_VELOCITY, PointerHandlers, ScaleEvent, ScrollEvent,
};
use crate::physics::FrictionSimulation;
use crate::render::{
    Alignment, Axis, EdgeInsets, Offset, RenderClipRect, RenderOverflowBox, RenderPointerRegion,
    RenderRef, RenderSizeReporter, RenderTransform, Size,
};

/// Upstream's `_kDrag`: the coefficient of friction in the inertial
/// translation animation, "eyeballed to give a feel similar to Google
/// Photos".
pub const INTERACTION_END_FRICTION_COEFFICIENT: f32 = 0.0000135;

/// Upstream's default `scaleFactor`, `kDefaultMouseScrollToScaleFactor`
/// (`widgets/scrollable_helpers.dart`).
pub const DEFAULT_SCROLL_TO_SCALE_FACTOR: f32 = 200.0;

/// Upstream's hardcoded `_rotateEnabled`. False there and false here; the
/// constant exists so the rotation branches port as written rather than being
/// silently dropped.
const ROTATE_ENABLED: bool = false;

// -- Affine2D -----------------------------------------------------------------

/// A 2D affine transformation, `[a, b, c, d, e, f]` applied as
/// `x' = a*x + c*y + e`, `y' = b*x + d*y + f` -- the same convention
/// [`crate::render::RenderTransform`] paints with.
///
/// Upstream the controller's value is a `Matrix4` (vector_math); the viewer
/// only ever builds translations and uniform scales, so the affine loses
/// nothing. The methods keep the names of the `Matrix4` members the viewer
/// calls.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine2D(pub [f32; 6]);

impl Affine2D {
    /// `Matrix4.identity()`.
    pub const IDENTITY: Affine2D = Affine2D([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `matrix.clone()..translate(dx, dy)`: a translation applied after this
    /// one (post-concatenation, the way `Matrix4.translate` works).
    pub fn translate_by_double(&self, dx: f32, dy: f32) -> Affine2D {
        let [a, b, c, d, e, f] = self.0;
        Affine2D([a, b, c, d, a * dx + c * dy + e, b * dx + d * dy + f])
    }

    /// `matrix.clone()..scale(s)`: a uniform scale applied after this one.
    pub fn scale_by_double(&self, s: f32) -> Affine2D {
        let [a, b, c, d, e, f] = self.0;
        Affine2D([a * s, b * s, c * s, d * s, e, f])
    }

    /// `matrix.getTranslation()`.
    pub fn translation(&self) -> Offset {
        Offset::new(self.0[4], self.0[5])
    }

    /// `matrix.clone()..setTranslation(v)`.
    pub fn with_translation(&self, translation: Offset) -> Affine2D {
        let [a, b, c, d, _, _] = self.0;
        Affine2D([a, b, c, d, translation.dx, translation.dy])
    }

    /// `matrix.getMaxScaleOnAxis()`: the larger of the two basis vectors'
    /// lengths.
    pub fn max_scale_on_axis(&self) -> f32 {
        let [a, b, c, d, _, _] = self.0;
        (a * a + b * b).sqrt().max((c * c + d * d).sqrt())
    }

    /// `Matrix4.inverted(matrix)`.
    pub fn inverted(&self) -> Affine2D {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        assert!(
            det != 0.0,
            "an interactive viewer's matrix is never singular"
        );
        let inv = 1.0 / det;
        Affine2D([
            d * inv,
            -b * inv,
            -c * inv,
            a * inv,
            (c * f - d * e) * inv,
            (b * e - a * f) * inv,
        ])
    }

    /// `matrix.transform3(Vector3(point.dx, point.dy, 0))`, read back as an
    /// `Offset`.
    pub fn transform_point(&self, point: Offset) -> Offset {
        let [a, b, c, d, e, f] = self.0;
        Offset::new(
            a * point.dx + c * point.dy + e,
            b * point.dx + d * point.dy + f,
        )
    }
}

// -- TransformationController ---------------------------------------------------

/// A thin wrapper on a shared cell whose value is the [`Affine2D`] the viewer
/// transforms its child by.
///
/// Upstream's `TransformationController` (`widgets/interactive_viewer.dart`),
/// a `ValueNotifier<Matrix4>`. The listener list is the one thing not ported:
/// every write here happens inside a `set_state` or an `advance`, and those
/// are what ask for the frame, so there is no one left to notify.
#[derive(Clone)]
pub struct TransformationController {
    value: Rc<RefCell<Affine2D>>,
}

impl TransformationController {
    /// `TransformationController()`: the identity matrix, no transformation.
    pub fn new() -> TransformationController {
        Self::default()
    }

    /// `TransformationController(value)`.
    pub fn with_value(value: Affine2D) -> TransformationController {
        TransformationController {
            value: Rc::new(RefCell::new(value)),
        }
    }

    /// The current transformation. `controller.value`.
    pub fn value(&self) -> Affine2D {
        *self.value.borrow()
    }

    /// Sets the current transformation. `controller.value = m`.
    pub fn set_value(&self, value: Affine2D) {
        *self.value.borrow_mut() = value;
    }

    /// The scene point at the given viewport point. Upstream's `toScene`: the
    /// inverse transformation of the scene, so the point the child drew under
    /// the viewport point is what comes back.
    pub fn to_scene(&self, viewport_point: Offset) -> Offset {
        self.value().inverted().transform_point(viewport_point)
    }

    /// Whether two controllers are the same cell. Upstream asks the same
    /// question by object identity in `_InteractiveViewerState.didUpdateWidget`.
    pub fn same_cell(&self, other: &TransformationController) -> bool {
        Rc::ptr_eq(&self.value, &other.value)
    }
}

impl Default for TransformationController {
    fn default() -> TransformationController {
        Self::with_value(Affine2D::IDENTITY)
    }
}

// -- The boundary math ----------------------------------------------------------
//
// Everything from here to the widget ports the file-private helpers of
// `widgets/interactive_viewer.dart` one for one. They are pure functions of
// the matrix, the boundary and the viewport, so the tests pin them directly.

/// Upstream's `Quad` (vector_math), four corner points in order: top-left,
/// top-right, bottom-right, bottom-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad(pub [Offset; 4]);

impl Quad {
    /// `Quad.points(point0, point1, point2, point3)`.
    fn points(p0: Offset, p1: Offset, p2: Offset, p3: Offset) -> Quad {
        Quad([p0, p1, p2, p3])
    }
}

/// The closest point to `point` on the segment `l1`..`l2`. Upstream's
/// `InteractiveViewer.getNearestPointOnLine`.
pub fn get_nearest_point_on_line(point: Offset, l1: Offset, l2: Offset) -> Offset {
    let length_squared = (l2.dx - l1.dx).powi(2) + (l2.dy - l1.dy).powi(2);

    // In this case, l1 == l2.
    if length_squared == 0.0 {
        return l1;
    }

    // Calculate how far down the line segment the closest point is and return
    // the point.
    let l1_p = point.minus(l1);
    let l1_l2 = l2.minus(l1);
    let fraction = ((l1_p.dx * l1_l2.dx + l1_p.dy * l1_l2.dy) / length_squared).clamp(0.0, 1.0);
    l1.plus(Offset::new(l1_l2.dx * fraction, l1_l2.dy * fraction))
}

/// Given a quad, return its axis aligned bounding box. Upstream's
/// `InteractiveViewer.getAxisAlignedBoundingBox`.
pub fn get_axis_aligned_bounding_box(quad: Quad) -> Quad {
    let [p0, p1, p2, p3] = quad.0;
    let min_x = p0.dx.min(p1.dx.min(p2.dx.min(p3.dx)));
    let min_y = p0.dy.min(p1.dy.min(p2.dy.min(p3.dy)));
    let max_x = p0.dx.max(p1.dx.max(p2.dx.max(p3.dx)));
    let max_y = p0.dy.max(p1.dy.max(p2.dy.max(p3.dy)));
    Quad::points(
        Offset::new(min_x, min_y),
        Offset::new(max_x, min_y),
        Offset::new(max_x, max_y),
        Offset::new(min_x, max_y),
    )
}

/// Returns true iff the point is inside the rectangle given by the quad,
/// inclusively. Upstream's `InteractiveViewer.pointIsInside`, the algorithm
/// from <https://math.stackexchange.com/a/190373>.
pub fn point_is_inside(point: Offset, quad: Quad) -> bool {
    let a_m = point.minus(quad.0[0]);
    let a_b = quad.0[1].minus(quad.0[0]);
    let a_d = quad.0[3].minus(quad.0[0]);

    let a_m_a_b = a_m.dx * a_b.dx + a_m.dy * a_b.dy;
    let a_b_a_b = a_b.dx * a_b.dx + a_b.dy * a_b.dy;
    let a_m_a_d = a_m.dx * a_d.dx + a_m.dy * a_d.dy;
    let a_d_a_d = a_d.dx * a_d.dx + a_d.dy * a_d.dy;

    0.0 <= a_m_a_b && a_m_a_b <= a_b_a_b && 0.0 <= a_m_a_d && a_m_a_d <= a_d_a_d
}

/// Get the point inside (inclusively) the given quad that is nearest to the
/// given point. Upstream's `InteractiveViewer.getNearestPointInside`.
pub fn get_nearest_point_inside(point: Offset, quad: Quad) -> Offset {
    // If the point is inside the axis aligned bounding box, then it's ok where
    // it is.
    if point_is_inside(point, quad) {
        return point;
    }

    // Otherwise, return the nearest point on the quad.
    let closest_points = [
        get_nearest_point_on_line(point, quad.0[0], quad.0[1]),
        get_nearest_point_on_line(point, quad.0[1], quad.0[2]),
        get_nearest_point_on_line(point, quad.0[2], quad.0[3]),
        get_nearest_point_on_line(point, quad.0[3], quad.0[0]),
    ];
    let mut min_distance = f32::INFINITY;
    let mut closest_overall = point;
    for close_point in closest_points {
        let distance = (point.dx - close_point.dx).hypot(point.dy - close_point.dy);
        if distance < min_distance {
            min_distance = distance;
            closest_overall = close_point;
        }
    }
    closest_overall
}

/// Transform the four corners of the viewport by the inverse of the given
/// matrix. Upstream's `_transformViewport`: the viewport transforms as the
/// inverse of the child (moving the child left is equivalent to moving the
/// viewport right).
pub fn transform_viewport(matrix: Affine2D, viewport: Rect) -> Quad {
    let inverse = matrix.inverted();
    Quad::points(
        inverse.transform_point(Offset::new(viewport.left, viewport.top)),
        inverse.transform_point(Offset::new(viewport.right, viewport.top)),
        inverse.transform_point(Offset::new(viewport.right, viewport.bottom)),
        inverse.transform_point(Offset::new(viewport.left, viewport.bottom)),
    )
}

/// Find the axis aligned bounding box for the rect rotated about its center by
/// the given amount. Upstream's `_getAxisAlignedBoundingBoxWithRotation`.
pub fn get_axis_aligned_bounding_box_with_rotation(rect: Rect, rotation: f32) -> Quad {
    let (sin, cos) = rotation.sin_cos();
    let center = Offset::new(rect.width() / 2.0, rect.height() / 2.0);
    // Matrix4.identity()
    //   ..translate(center.dx, center.dy)
    //   ..rotateZ(rotation)
    //   ..translate(-center.dx, -center.dy)
    // as an affine.
    let rotation_matrix = Affine2D([
        cos,
        sin,
        -sin,
        cos,
        center.dx - cos * center.dx + sin * center.dy,
        center.dy - sin * center.dx - cos * center.dy,
    ]);
    let boundaries_rotated = Quad::points(
        rotation_matrix.transform_point(Offset::new(rect.left, rect.top)),
        rotation_matrix.transform_point(Offset::new(rect.right, rect.top)),
        rotation_matrix.transform_point(Offset::new(rect.right, rect.bottom)),
        rotation_matrix.transform_point(Offset::new(rect.left, rect.bottom)),
    );
    get_axis_aligned_bounding_box(boundaries_rotated)
}

/// Round the output values. Upstream's `_round`: a workaround for a precision
/// problem where values that should have been zero were given as within
/// 10^-10 of zero. Upstream goes through `toStringAsFixed(9)`; the fixed-point
/// round here is the same nine decimal places.
pub fn round_offset(offset: Offset) -> Offset {
    const PLACES: f32 = 1e9;
    Offset::new(
        (offset.dx * PLACES).round() / PLACES,
        (offset.dy * PLACES).round() / PLACES,
    )
}

/// Return the amount that viewport lies outside of boundary. If the viewport
/// is completely contained within the boundary (inclusively), then returns
/// `Offset::ZERO`. Upstream's `_exceedsBy`.
pub fn exceeds_by(boundary: Quad, viewport: Quad) -> Offset {
    let viewport_points = viewport.0;
    let mut largest_excess = Offset::ZERO;
    for point in viewport_points {
        let point_inside = get_nearest_point_inside(point, boundary);
        let excess = Offset::new(point_inside.dx - point.dx, point_inside.dy - point.dy);
        if excess.dx.abs() > largest_excess.dx.abs() {
            largest_excess = Offset::new(excess.dx, largest_excess.dy);
        }
        if excess.dy.abs() > largest_excess.dy.abs() {
            largest_excess = Offset::new(largest_excess.dx, excess.dy);
        }
    }

    round_offset(largest_excess)
}

/// Align the given offset to the given axis by allowing movement only in the
/// axis direction. Upstream's `_alignAxis`.
pub fn align_axis(offset: Offset, axis: Axis) -> Offset {
    match axis {
        Axis::Horizontal => Offset::new(offset.dx, 0.0),
        Axis::Vertical => Offset::new(0.0, offset.dy),
    }
}

/// Given two points, return the axis where the distance between the points is
/// greatest. If they are equal, return `None`. Upstream's `_getPanAxis`.
pub fn pan_axis_of(point1: Offset, point2: Offset) -> Option<Axis> {
    if point1 == point2 {
        return None;
    }
    let x = point2.dx - point1.dx;
    let y = point2.dy - point1.dy;
    if x.abs() > y.abs() {
        Some(Axis::Horizontal)
    } else {
        Some(Axis::Vertical)
    }
}

/// Given a velocity and drag, calculate the time at which motion will come to
/// a stop, within the margin of `effectively_motionless`. Upstream's
/// `_getFinalTime`.
pub fn final_time(velocity: f32, drag: f32, effectively_motionless: f32) -> f32 {
    (effectively_motionless / velocity).ln() / (drag / 100.0).ln()
}

/// Whether the boundary is infinite in every direction. Upstream asserts the
/// margin is infinite on all sides or on none (`_boundaryRect`), so the two
/// cases this returns for are the only two there are.
fn boundary_is_infinite(boundary: Rect) -> bool {
    boundary.left == f32::NEG_INFINITY
        && boundary.top == f32::NEG_INFINITY
        && boundary.right == f32::INFINITY
        && boundary.bottom == f32::INFINITY
}

/// Return a new matrix representing the given matrix after applying the given
/// translation. Upstream's `_matrixTranslate`, with what its closure captured
/// passed in: the pan-axis configuration, the boundary rect, the viewport and
/// the current rotation.
#[allow(clippy::too_many_arguments)]
pub fn matrix_translate(
    matrix: Affine2D,
    translation: Offset,
    pan_axis: PanAxis,
    current_axis: Option<Axis>,
    boundary_rect: Rect,
    viewport: Rect,
    current_rotation: f32,
) -> Affine2D {
    if translation == Offset::ZERO {
        return matrix;
    }

    let aligned_translation = match current_axis {
        Some(axis) => match pan_axis {
            PanAxis::Horizontal => align_axis(translation, Axis::Horizontal),
            PanAxis::Vertical => align_axis(translation, Axis::Vertical),
            PanAxis::Aligned => align_axis(translation, axis),
            PanAxis::Free => translation,
        },
        None => translation,
    };

    let next_matrix = matrix.translate_by_double(aligned_translation.dx, aligned_translation.dy);

    // Transform the viewport to determine where its four corners will be after
    // the child has been transformed.
    let next_viewport = transform_viewport(next_matrix, viewport);

    // If the boundaries are infinite, then no need to check if the translation
    // fits within them.
    if boundary_is_infinite(boundary_rect) {
        return next_matrix;
    }

    // Expand the boundaries with rotation. This prevents the problem where a
    // mismatch in orientation between the viewport and boundaries effectively
    // limits translation. With this approach, all points that are visible with
    // no rotation are visible after rotation.
    let boundaries_aabb_quad =
        get_axis_aligned_bounding_box_with_rotation(boundary_rect, current_rotation);

    // If the given translation fits completely within the boundaries, allow it.
    let offending_distance = exceeds_by(boundaries_aabb_quad, next_viewport);
    if offending_distance == Offset::ZERO {
        return next_matrix;
    }

    // Desired translation goes out of bounds, so translate to the nearest
    // in-bounds point instead.
    let next_total_translation = next_matrix.translation();
    let current_scale = matrix.max_scale_on_axis();
    let corrected_total_translation = Offset::new(
        next_total_translation.dx - offending_distance.dx * current_scale,
        next_total_translation.dy - offending_distance.dy * current_scale,
    );
    // Upstream's TODO (flutter/flutter#57698): this needs some work to handle
    // rotation properly. Here rotation is never enabled, as upstream's is not.
    let corrected_matrix = matrix.with_translation(corrected_total_translation);

    // Double check that the corrected translation fits.
    let corrected_viewport = transform_viewport(corrected_matrix, viewport);
    let offending_corrected_distance = exceeds_by(boundaries_aabb_quad, corrected_viewport);
    if offending_corrected_distance == Offset::ZERO {
        return corrected_matrix;
    }

    // If the corrected translation doesn't fit in either direction, don't allow
    // any translation at all. This happens when the viewport is larger than the
    // entire boundary.
    if offending_corrected_distance.dx != 0.0 && offending_corrected_distance.dy != 0.0 {
        return matrix;
    }

    // Otherwise, allow translation in only the direction that fits. This
    // happens when the viewport is larger than the boundary in one direction.
    let unidirectional_corrected_total_translation = Offset::new(
        if offending_corrected_distance.dx == 0.0 {
            corrected_total_translation.dx
        } else {
            0.0
        },
        if offending_corrected_distance.dy == 0.0 {
            corrected_total_translation.dy
        } else {
            0.0
        },
    );
    matrix.with_translation(unidirectional_corrected_total_translation)
}

/// Return a new matrix representing the given matrix after applying the given
/// scale. Upstream's `_matrixScale`.
pub fn matrix_scale(
    matrix: Affine2D,
    scale: f32,
    min_scale: f32,
    max_scale: f32,
    viewport: Rect,
    boundary_rect: Rect,
) -> Affine2D {
    if scale == 1.0 {
        return matrix;
    }
    debug_assert!(scale != 0.0);

    // Don't allow a scale that results in an overall scale beyond min/max
    // scale.
    let current_scale = matrix.max_scale_on_axis();
    let total_scale = (current_scale * scale).max(
        // Ensure that the scale cannot make the child so big that it can't fit
        // inside the boundaries (in either direction).
        (viewport.width() / boundary_rect.width()).max(viewport.height() / boundary_rect.height()),
    );
    let clamped_total_scale = total_scale.clamp(min_scale, max_scale);
    let clamped_scale = clamped_total_scale / current_scale;
    matrix.scale_by_double(clamped_scale)
}

// -- The widget -----------------------------------------------------------------

/// This enum is used to specify the behavior of the [`InteractiveViewer`] when
/// the user drags the viewport. Upstream's `PanAxis`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanAxis {
    /// The user can only pan the viewport along the horizontal axis.
    Horizontal,
    /// The user can only pan the viewport along the vertical axis.
    Vertical,
    /// The user can pan the viewport along the horizontal and vertical axes
    /// but not diagonally.
    Aligned,
    /// The user can pan the viewport freely in any direction.
    #[default]
    Free,
}

/// A classification of relevant user gestures. Each contiguous user gesture is
/// represented by exactly one type. Upstream's `_GestureType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GestureType {
    Pan,
    Scale,
    // Upstream's `_GestureType.rotate`: unreachable, because `ROTATE_ENABLED`
    // is false, exactly as `_rotateEnabled` is upstream.
    #[allow(dead_code)]
    Rotate,
}

/// Decide which type of gesture this is by comparing the amount of scale and
/// rotation in the gesture, if any. Upstream's `_getGestureType`, with
/// `scale_enabled` and `ROTATE_ENABLED` standing in for the widget flags it
/// read.
fn gesture_type_of(scale: f32, rotation: f32, scale_enabled: bool) -> GestureType {
    let scale = if !scale_enabled { 1.0 } else { scale };
    let rotation = if !ROTATE_ENABLED { 0.0 } else { rotation };
    if (scale - 1.0).abs() > rotation.abs() {
        GestureType::Scale
    } else if rotation != 0.0 {
        GestureType::Rotate
    } else {
        GestureType::Pan
    }
}

/// The widget configuration the gesture handlers close over. Handlers cannot
/// borrow the widget -- they outlive the build -- so the flags the gesture
/// math reads travel as values.
#[derive(Clone, Copy)]
struct ViewerConfig {
    boundary_margin: EdgeInsets,
    pan_axis: PanAxis,
    pan_enabled: bool,
    scale_enabled: bool,
    min_scale: f32,
    max_scale: f32,
    scale_factor: f32,
    interaction_end_friction_coefficient: f32,
}

/// The pan inertia a fling left behind: the friction animation upstream runs
/// on `_controller` from `_onScaleEnd`'s pan branch.
#[derive(Clone, Copy, Debug)]
struct Inertia {
    /// The translation when the finger lifted. Upstream animates a
    /// `Tween(begin: translation, end: finalX)`; these are its begin and end.
    from: Offset,
    to: Offset,
    /// How long the animation runs. Upstream's `_controller.duration`,
    /// `_getFinalTime` in milliseconds.
    duration_micros: i64,
    /// The frame clock when the first `advance` after the fling ran; the
    /// release itself carries no timestamp the state can read.
    started_micros: Option<i64>,
}

/// What an [`InteractiveViewer`] remembers between frames. Upstream's
/// `_InteractiveViewerState` fields, minus the animation controllers (this
/// crate's `advance` is the ticker) and the GlobalKeys (the size reporters
/// are how a build here reads what layout concluded).
pub struct InteractiveViewerState {
    /// The transformation. `_transformer`.
    transformer: TransformationController,
    /// The viewport's size, filled in at layout. Upstream's `_viewport`
    /// getter, which reads the parent render box's size; the same answer one
    /// frame later, which only matters for a gesture before the first layout.
    viewport_size: Rc<Cell<Size>>,
    /// The child's size, filled in at layout. Upstream's `_boundaryRect`
    /// reads it off the child's render box the same way.
    child_size: Rc<Cell<Size>>,
    /// `_gestureType`.
    gesture_type: Option<GestureType>,
    /// `_currentAxis`, used with `PanAxis::Aligned`.
    current_axis: Option<Axis>,
    /// `_referenceFocalPoint`: where the current gesture began, in scene
    /// coordinates.
    reference_focal_point: Option<Offset>,
    /// `_scaleStart`: the scale value at the start of a scaling gesture.
    scale_start: Option<f32>,
    /// `_currentRotation`. Never other than zero: `ROTATE_ENABLED` is false.
    current_rotation: f32,
    /// The running pan inertia, if any.
    inertia: Option<Inertia>,
}

impl Default for InteractiveViewerState {
    fn default() -> InteractiveViewerState {
        InteractiveViewerState {
            transformer: TransformationController::default(),
            viewport_size: Rc::new(Cell::new(Size::ZERO)),
            child_size: Rc::new(Cell::new(Size::ZERO)),
            gesture_type: None,
            current_axis: None,
            reference_focal_point: None,
            scale_start: None,
            current_rotation: 0.0,
            inertia: None,
        }
    }
}

impl InteractiveViewerState {
    /// The controller this viewer transforms by. What an owner uses to set the
    /// matrix from outside -- the gallery's demo resets to its home matrix
    /// through it, as upstream's does through its own controller.
    pub fn transformer(&self) -> TransformationController {
        self.transformer.clone()
    }

    /// `_boundaryRect`: the boundary margin inflated around the child.
    fn boundary_rect(&self, config: &ViewerConfig) -> Rect {
        let child = self.child_size.get();
        debug_assert!(
            child.width > 0.0 && child.height > 0.0,
            "InteractiveViewer's child must have nonzero dimensions."
        );
        let margin = config.boundary_margin;
        Rect::ltrb(
            -margin.left,
            -margin.top,
            child.width + margin.right,
            child.height + margin.bottom,
        )
    }

    /// `_viewport`.
    fn viewport_rect(&self) -> Rect {
        let size = self.viewport_size.get();
        Rect::ltrb(0.0, 0.0, size.width, size.height)
    }

    /// `_matrixTranslate` with the state's captures supplied.
    fn translate(&self, matrix: Affine2D, translation: Offset, config: &ViewerConfig) -> Affine2D {
        matrix_translate(
            matrix,
            translation,
            config.pan_axis,
            self.current_axis,
            self.boundary_rect(config),
            self.viewport_rect(),
            self.current_rotation,
        )
    }

    /// `_matrixScale` with the state's captures supplied.
    fn scale(&self, matrix: Affine2D, scale: f32, config: &ViewerConfig) -> Affine2D {
        matrix_scale(
            matrix,
            scale,
            config.min_scale,
            config.max_scale,
            self.viewport_rect(),
            self.boundary_rect(config),
        )
    }

    /// `_gestureIsSupported`.
    fn gesture_is_supported(
        &self,
        gesture_type: Option<GestureType>,
        config: &ViewerConfig,
    ) -> bool {
        match gesture_type {
            Some(GestureType::Rotate) => ROTATE_ENABLED,
            Some(GestureType::Scale) => config.scale_enabled,
            Some(GestureType::Pan) | None => config.pan_enabled,
        }
    }

    /// `_onScaleStart`, minus the details: the gesture bookkeeping resets and
    /// any running inertia stops, as upstream's controllers are stopped.
    fn interaction_started(&mut self, focal_point_scene: Offset) {
        self.inertia = None;
        self.gesture_type = None;
        self.current_axis = None;
        self.scale_start = Some(self.transformer.value().max_scale_on_axis());
        self.reference_focal_point = Some(focal_point_scene);
    }

    /// The pan branch shared by the one-finger drag and the two-finger slide:
    /// upstream's `_onScaleUpdate` `pan` case, with the caller re-reading the
    /// reference point afterwards the way upstream reassigns
    /// `_referenceFocalPoint` at the end of the branch.
    fn pan_to(&mut self, focal_point_scene: Offset, config: &ViewerConfig) {
        let Some(reference) = self.reference_focal_point else {
            return;
        };
        if self.current_axis.is_none() {
            self.current_axis = pan_axis_of(reference, focal_point_scene);
        }
        // Translate so that the same point in the scene is underneath the
        // focal point before and after the movement.
        let translation_change = focal_point_scene.minus(reference);
        let matrix = self.translate(self.transformer.value(), translation_change, config);
        self.transformer.set_value(matrix);
    }

    /// The scale branch of `_onScaleUpdate`.
    fn scale_to(&mut self, event_scale: f32, local_focal_point: Offset, config: &ViewerConfig) {
        let (Some(scale_start), Some(reference)) = (self.scale_start, self.reference_focal_point)
        else {
            return;
        };
        // event_scale gives the amount to change the scale as of the start of
        // this gesture, so calculate the amount to scale as of the previous
        // update.
        let current = self.transformer.value().max_scale_on_axis();
        let desired_scale = scale_start * event_scale;
        let scale_change = desired_scale / current;
        let matrix = self.scale(self.transformer.value(), scale_change, config);
        self.transformer.set_value(matrix);

        // While scaling, translate such that the user's two fingers stay on
        // the same places in the scene.
        let focal_point_scene_scaled = self.transformer.to_scene(local_focal_point);
        let matrix = self.translate(
            self.transformer.value(),
            focal_point_scene_scaled.minus(reference),
            config,
        );
        self.transformer.set_value(matrix);

        // local_focal_point should now be at the same location as the original
        // reference point. If it's not, that's because the translate came in
        // contact with a boundary. In that case, update the reference so
        // subsequent updates happen in relation to the new effective focal
        // point.
        let focal_point_scene_check = self.transformer.to_scene(local_focal_point);
        if round_offset(reference) != round_offset(focal_point_scene_check) {
            self.reference_focal_point = Some(focal_point_scene_check);
        }
    }

    /// The pan case of `_onScaleEnd`: a fling becomes a friction animation.
    /// Returns without an animation when the release was not a fling, which is
    /// upstream's `kMinFlingVelocity` guard.
    fn fling(&mut self, velocity: Offset, config: &ViewerConfig) {
        if velocity.distance() < MIN_FLING_VELOCITY {
            self.current_axis = None;
            return;
        }
        let translation = self.transformer.value().translation();
        let friction_simulation_x = FrictionSimulation::new(
            config.interaction_end_friction_coefficient,
            translation.dx,
            velocity.dx,
        );
        let friction_simulation_y = FrictionSimulation::new(
            config.interaction_end_friction_coefficient,
            translation.dy,
            velocity.dy,
        );
        let t_final = final_time(
            velocity.distance(),
            config.interaction_end_friction_coefficient,
            10.0,
        );
        self.inertia = Some(Inertia {
            from: translation,
            to: Offset::new(
                friction_simulation_x.final_x(),
                friction_simulation_y.final_x(),
            ),
            duration_micros: (t_final * 1_000_000.0).round() as i64,
            started_micros: None,
        });
    }
}

/// A widget that enables pan and zoom interactions with its child.
///
/// The user can transform the child by dragging to pan or pinching to zoom;
/// a mouse wheel zooms about the cursor. Upstream's `InteractiveViewer`
/// (`widgets/interactive_viewer.dart`), used through [`interactive_viewer`]:
///
/// ```ignore
/// interactive_viewer(
///     InteractiveViewer::new(id, || board_widget())
///         .with_boundary_margin(EdgeInsets::all(400.0))
///         .with_min_scale(0.01)
///         .with_transformation_controller(controller),
/// )
/// ```
///
/// The child is a builder rather than a widget for the same reason
/// [`crate::ink::Ink`]'s is: a stateful component is rebuilt from the same
/// widget instance every time its state changes, and a child stored as a
/// widget would be handed over on the first build and gone on the second.
pub struct InteractiveViewer {
    /// Distinguishes this viewer's pointer region from the others in the
    /// tree, for hit testing and for element reuse.
    id: u64,
    child: Rc<dyn Fn() -> AnyWidget>,
    /// `boundaryMargin`. `EdgeInsets.zero` upstream: boundaries the exact size
    /// and position of the child.
    boundary_margin: EdgeInsets,
    /// `constrained`: whether the viewport's size constraints are applied to
    /// the child. True upstream by default; false lays the child out against
    /// infinite constraints inside an overflow box, so a child bigger than the
    /// viewport can be panned to reveal itself.
    constrained: bool,
    /// `panAxis`.
    pan_axis: PanAxis,
    /// `panEnabled`.
    pan_enabled: bool,
    /// `scaleEnabled`.
    scale_enabled: bool,
    /// `minScale`, 0.8 upstream.
    min_scale: f32,
    /// `maxScale`, 2.5 upstream.
    max_scale: f32,
    /// `scaleFactor`, `kDefaultMouseScrollToScaleFactor` upstream.
    scale_factor: f32,
    /// `interactionEndFrictionCoefficient`, `_kDrag` upstream.
    interaction_end_friction_coefficient: f32,
    /// `transformationController`.
    transformation_controller: Option<TransformationController>,
    /// `onInteractionStart`, minus the details (see the module docs).
    on_interaction_start: Option<Rc<dyn Fn()>>,
    /// `onInteractionUpdate`.
    on_interaction_update: Option<Rc<dyn Fn()>>,
    /// `onInteractionEnd`.
    on_interaction_end: Option<Rc<dyn Fn()>>,
}

impl InteractiveViewer {
    pub fn new(id: u64, child: impl Fn() -> AnyWidget + 'static) -> InteractiveViewer {
        InteractiveViewer {
            id,
            child: Rc::new(child),
            boundary_margin: EdgeInsets::ZERO,
            constrained: true,
            pan_axis: PanAxis::default(),
            pan_enabled: true,
            scale_enabled: true,
            min_scale: 0.8,
            max_scale: 2.5,
            scale_factor: DEFAULT_SCROLL_TO_SCALE_FACTOR,
            interaction_end_friction_coefficient: INTERACTION_END_FRICTION_COEFFICIENT,
            transformation_controller: None,
            on_interaction_start: None,
            on_interaction_update: None,
            on_interaction_end: None,
        }
    }

    pub fn with_boundary_margin(mut self, margin: EdgeInsets) -> Self {
        // Upstream's constructor assert: the margin is either fully infinite
        // or fully finite, never a mix.
        let sides = [margin.left, margin.top, margin.right, margin.bottom];
        debug_assert!(
            sides.iter().all(|side| side.is_infinite())
                || sides.iter().all(|side| side.is_finite()),
            "boundaryMargin must be either fully infinite or fully finite"
        );
        self.boundary_margin = margin;
        self
    }

    pub fn with_constrained(mut self, constrained: bool) -> Self {
        self.constrained = constrained;
        self
    }

    pub fn with_pan_axis(mut self, pan_axis: PanAxis) -> Self {
        self.pan_axis = pan_axis;
        self
    }

    pub fn with_pan_enabled(mut self, enabled: bool) -> Self {
        self.pan_enabled = enabled;
        self
    }

    pub fn with_scale_enabled(mut self, enabled: bool) -> Self {
        self.scale_enabled = enabled;
        self
    }

    pub fn with_min_scale(mut self, scale: f32) -> Self {
        debug_assert!(scale > 0.0 && scale.is_finite());
        self.min_scale = scale;
        self
    }

    pub fn with_max_scale(mut self, scale: f32) -> Self {
        debug_assert!(scale > 0.0 && !scale.is_nan());
        self.max_scale = scale;
        debug_assert!(self.max_scale >= self.min_scale);
        self
    }

    pub fn with_scale_factor(mut self, factor: f32) -> Self {
        self.scale_factor = factor;
        self
    }

    pub fn with_interaction_end_friction_coefficient(mut self, coefficient: f32) -> Self {
        debug_assert!(coefficient > 0.0);
        self.interaction_end_friction_coefficient = coefficient;
        self
    }

    pub fn with_transformation_controller(mut self, controller: TransformationController) -> Self {
        self.transformation_controller = Some(controller);
        self
    }

    pub fn with_on_interaction_start(mut self, handler: impl Fn() + 'static) -> Self {
        self.on_interaction_start = Some(Rc::new(handler));
        self
    }

    pub fn with_on_interaction_update(mut self, handler: impl Fn() + 'static) -> Self {
        self.on_interaction_update = Some(Rc::new(handler));
        self
    }

    pub fn with_on_interaction_end(mut self, handler: impl Fn() + 'static) -> Self {
        self.on_interaction_end = Some(Rc::new(handler));
        self
    }

    fn config(&self) -> ViewerConfig {
        ViewerConfig {
            boundary_margin: self.boundary_margin,
            pan_axis: self.pan_axis,
            pan_enabled: self.pan_enabled,
            scale_enabled: self.scale_enabled,
            min_scale: self.min_scale,
            max_scale: self.max_scale,
            scale_factor: self.scale_factor,
            interaction_end_friction_coefficient: self.interaction_end_friction_coefficient,
        }
    }
}

impl StatefulComponent for InteractiveViewer {
    type State = InteractiveViewerState;

    fn key(&self) -> Key {
        Some(self.id)
    }

    fn initial_state(&self) -> InteractiveViewerState {
        let mut state = InteractiveViewerState::default();
        if let Some(controller) = &self.transformation_controller {
            state.transformer = controller.clone();
        }
        state
    }

    fn did_update_widget(&self, old: &Self, state: &mut InteractiveViewerState) {
        // Upstream's `didUpdateWidget`: an unchanged controller keeps the
        // state it has; a new one replaces it, and a dropped one falls back to
        // an internal controller.
        let unchanged = match (
            &self.transformation_controller,
            &old.transformation_controller,
        ) {
            (Some(new), Some(old)) => new.same_cell(old),
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return;
        }
        state.transformer = match &self.transformation_controller {
            Some(controller) => controller.clone(),
            None => TransformationController::default(),
        };
    }

    fn advance(&self, state: &mut InteractiveViewerState, frame_time_micros: i64) -> bool {
        // `_handleInertiaAnimation`: the friction animation upstream's
        // `_controller` ticks. There is no scale-animation branch: a pinch
        // here ends without a velocity to fling (see the module docs).
        let Some(inertia) = &mut state.inertia else {
            return false;
        };
        let started = *inertia.started_micros.get_or_insert(frame_time_micros);
        let elapsed = (frame_time_micros - started).max(0);
        let duration = inertia.duration_micros.max(1);
        let t = (elapsed as f32 / duration as f32).clamp(0.0, 1.0);
        // The tween is chained through `Curves.decelerate` upstream.
        let eased = Curve::Decelerate.transform(t);
        let value = inertia
            .from
            .plus(inertia.to.minus(inertia.from).scaled(eased));
        let finished = t >= 1.0;
        if finished {
            state.inertia = None;
            state.current_axis = None;
        }
        // Translate such that the resulting translation is the animation's
        // value.
        let translation = state.transformer.value().translation();
        let change = state
            .transformer
            .to_scene(value)
            .minus(state.transformer.to_scene(translation));
        let matrix = state.translate(state.transformer.value(), change, &self.config());
        state.transformer.set_value(matrix);
        // The frame that clears the animation still has to be drawn, or the
        // last step of the fling never shows. The same rule
        // `animation::Controller::tick` follows.
        true
    }

    fn build(
        &self,
        state: &InteractiveViewerState,
        handle: StateHandle<InteractiveViewerState>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        let matrix = state.transformer.value();
        let child = (self.child)();

        // The gesture handlers. Upstream hangs them on one GestureDetector's
        // scale recognizer; here the recognizers are separate (gestures.rs's
        // module docs), so a one-finger pan arrives as drag events and a pinch
        // as scale events, and the shared branches on the state are what both
        // funnel into.
        let handlers = PointerHandlers::new()
            .with_drag_start({
                let handle = handle.clone();
                let on_start = self.on_interaction_start.clone();
                move |event: DragEvent| {
                    if let Some(on_start) = &on_start {
                        on_start();
                    }
                    handle.set_state(move |state| {
                        let focal = state.transformer.to_scene(event.local_position);
                        state.interaction_started(focal);
                    });
                }
            })
            .with_drag_update({
                let handle = handle.clone();
                let config = self.config();
                let on_update = self.on_interaction_update.clone();
                move |event: DragEvent| {
                    handle.set_state(move |state| {
                        state.gesture_type = Some(GestureType::Pan);
                        if !state.gesture_is_supported(state.gesture_type, &config) {
                            return;
                        }
                        let focal = state.transformer.to_scene(event.local_position);
                        state.pan_to(focal, &config);
                        // Panning tracks the finger: the reference point moves
                        // to where the gesture is now, as upstream reassigns
                        // `_referenceFocalPoint` at the end of the pan branch.
                        state.reference_focal_point =
                            Some(state.transformer.to_scene(event.local_position));
                    });
                    if let Some(on_update) = &on_update {
                        on_update();
                    }
                }
            })
            .with_drag_end({
                let handle = handle.clone();
                let config = self.config();
                let on_end = self.on_interaction_end.clone();
                move |event: DragEndEvent| {
                    if let Some(on_end) = &on_end {
                        on_end();
                    }
                    handle.set_state(move |state| {
                        state.scale_start = None;
                        state.reference_focal_point = None;
                        let gesture_type = state.gesture_type.take();
                        if !state.gesture_is_supported(gesture_type, &config) {
                            state.current_axis = None;
                            return;
                        }
                        if gesture_type == Some(GestureType::Pan) {
                            state.fling(event.velocity, &config);
                        }
                    });
                }
            })
            .with_scale_start({
                let handle = handle.clone();
                let on_start = self.on_interaction_start.clone();
                move |event: ScaleEvent| {
                    if let Some(on_start) = &on_start {
                        on_start();
                    }
                    handle.set_state(move |state| {
                        let focal = state.transformer.to_scene(event.local_focal_point);
                        state.interaction_started(focal);
                    });
                }
            })
            .with_scale_update({
                let handle = handle.clone();
                let config = self.config();
                let on_update = self.on_interaction_update.clone();
                move |event: ScaleEvent| {
                    handle.set_state(move |state| {
                        // A gesture that was marked as a pan may be
                        // reinterpreted once its scale departs from 1, as
                        // upstream's `_onScaleUpdate` re-decides.
                        state.gesture_type = Some(gesture_type_of(
                            event.scale,
                            event.rotation,
                            config.scale_enabled,
                        ));
                        if !state.gesture_is_supported(state.gesture_type, &config) {
                            return;
                        }
                        match state.gesture_type {
                            Some(GestureType::Scale) => {
                                state.scale_to(event.scale, event.local_focal_point, &config)
                            }
                            Some(GestureType::Pan) => {
                                let focal = state.transformer.to_scene(event.local_focal_point);
                                state.pan_to(focal, &config);
                                state.reference_focal_point =
                                    Some(state.transformer.to_scene(event.local_focal_point));
                            }
                            _ => {}
                        }
                    });
                    if let Some(on_update) = &on_update {
                        on_update();
                    }
                }
            })
            .with_scale_end({
                let handle = handle.clone();
                let on_end = self.on_interaction_end.clone();
                move |_event: ScaleEvent| {
                    if let Some(on_end) = &on_end {
                        on_end();
                    }
                    handle.set_state(move |state| {
                        state.scale_start = None;
                        state.reference_focal_point = None;
                        state.gesture_type = None;
                        state.current_axis = None;
                    });
                }
            })
            .with_scroll({
                // `_receivedPointerSignal`'s mouse-wheel branch: a scroll
                // scales about the cursor, then translates so the cursor
                // stays over the same scene point.
                let handle = handle.clone();
                let config = self.config();
                let on_start = self.on_interaction_start.clone();
                let on_update = self.on_interaction_update.clone();
                let on_end = self.on_interaction_end.clone();
                move |event: ScrollEvent| {
                    // Ignore left and right mouse wheel scroll.
                    if event.delta.dy == 0.0 {
                        return;
                    }
                    let scale_change = (-event.delta.dy / config.scale_factor).exp();
                    if let Some(on_start) = &on_start {
                        on_start();
                    }
                    handle.set_state(move |state| {
                        state.inertia = None;
                        if !state.gesture_is_supported(Some(GestureType::Scale), &config) {
                            return;
                        }
                        let focal_point_scene = state.transformer.to_scene(event.local_position);
                        let matrix = state.scale(state.transformer.value(), scale_change, &config);
                        state.transformer.set_value(matrix);
                        // After scaling, translate such that the event's
                        // position is at the same scene point before and after
                        // the scale.
                        let focal_point_scene_scaled =
                            state.transformer.to_scene(event.local_position);
                        let matrix = state.translate(
                            state.transformer.value(),
                            focal_point_scene_scaled.minus(focal_point_scene),
                            &config,
                        );
                        state.transformer.set_value(matrix);
                    });
                    if let Some(on_update) = &on_update {
                        on_update();
                    }
                    if let Some(on_end) = &on_end {
                        on_end();
                    }
                }
            });

        let constrained = self.constrained;
        let id = self.id;
        let child_size = Rc::clone(&state.child_size);
        let viewport_size = Rc::clone(&state.viewport_size);

        single(child, move |child| {
            // `_InteractiveViewerBuilt`: a `ClipRect` around a `Transform`,
            // with the transform applied about the child's top-left corner
            // (upstream's `Transform(alignment: null)`), the child reporting
            // its size for the boundary math. The gesture region sits outside
            // the clip, upstream's `Listener` + `GestureDetector` around the
            // built child, and is `HitTestBehavior.opaque` by default here,
            // "necessary when panning off screen" upstream.
            let transformed = RenderTransform::new(
                matrix.0,
                RenderSizeReporter::new(Rc::clone(&child_size), child),
            )
            .with_origin(Alignment::TOP_LEFT);
            let inner: RenderRef = if constrained {
                RenderRef::new(transformed)
            } else {
                // The `constrained: false` branch: upstream's
                // `OverflowBox(alignment: topLeft, min: 0, max: infinity)`.
                RenderRef::new(
                    RenderOverflowBox::new(transformed)
                        .with_min_width(0.0)
                        .with_min_height(0.0)
                        .with_max_width(f32::INFINITY)
                        .with_max_height(f32::INFINITY)
                        .with_alignment(Alignment::TOP_LEFT),
                )
            };
            RenderRef::new(
                RenderPointerRegion::new(
                    id,
                    RenderSizeReporter::new(Rc::clone(&viewport_size), RenderClipRect::new(inner)),
                )
                .with_handlers(handlers.clone()),
            )
        })
    }
}

/// [`InteractiveViewer`] as a widget.
pub fn interactive_viewer(viewer: InteractiveViewer) -> AnyWidget {
    stateful(viewer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: f32, top: f32, right: f32, bottom: f32) -> Rect {
        Rect::ltrb(left, top, right, bottom)
    }

    #[test]
    fn an_affine_translates_scales_and_inverts() {
        let m = Affine2D::IDENTITY
            .translate_by_double(10.0, 20.0)
            .scale_by_double(2.0);
        assert_eq!(m.translation(), Offset::new(10.0, 20.0));
        assert_eq!(m.max_scale_on_axis(), 2.0);
        assert_eq!(
            m.transform_point(Offset::new(1.0, 1.0)),
            Offset::new(12.0, 22.0)
        );
        // The inverse undoes it, which is what `to_scene` is.
        let scene = m.inverted().transform_point(Offset::new(12.0, 22.0));
        assert!((scene.dx - 1.0).abs() < 1e-4 && (scene.dy - 1.0).abs() < 1e-4);
    }

    #[test]
    fn to_scene_reports_the_point_under_the_viewport_point() {
        let controller =
            TransformationController::with_value(Affine2D::IDENTITY.translate_by_double(5.0, 7.0));
        assert_eq!(controller.to_scene(Offset::new(5.0, 7.0)), Offset::ZERO);
    }

    #[test]
    fn the_nearest_point_on_a_degenerate_line_is_the_line() {
        // Upstream's `lengthSquared == 0` branch.
        let at = Offset::new(3.0, 4.0);
        assert_eq!(get_nearest_point_on_line(Offset::ZERO, at, at), at);
    }

    #[test]
    fn a_point_inside_a_quad_is_its_own_nearest_point() {
        let quad = Quad::points(
            Offset::ZERO,
            Offset::new(10.0, 0.0),
            Offset::new(10.0, 10.0),
            Offset::new(0.0, 10.0),
        );
        let inside = Offset::new(5.0, 5.0);
        assert!(point_is_inside(inside, quad));
        assert_eq!(get_nearest_point_inside(inside, quad), inside);
        // A point beyond the left edge clamps to the edge.
        let outside = Offset::new(-4.0, 5.0);
        assert!(!point_is_inside(outside, quad));
        assert_eq!(
            get_nearest_point_inside(outside, quad),
            Offset::new(0.0, 5.0)
        );
    }

    #[test]
    fn exceeds_by_reports_how_far_the_viewport_is_outside() {
        let boundary = Quad::points(
            Offset::ZERO,
            Offset::new(100.0, 0.0),
            Offset::new(100.0, 100.0),
            Offset::new(0.0, 100.0),
        );
        // Contained: no excess.
        let contained = Quad::points(
            Offset::new(10.0, 10.0),
            Offset::new(90.0, 10.0),
            Offset::new(90.0, 90.0),
            Offset::new(10.0, 90.0),
        );
        assert_eq!(exceeds_by(boundary, contained), Offset::ZERO);
        // Ten over the left edge: the correction points back right.
        let over = Quad::points(
            Offset::new(-10.0, 0.0),
            Offset::new(90.0, 0.0),
            Offset::new(90.0, 100.0),
            Offset::new(-10.0, 100.0),
        );
        assert_eq!(exceeds_by(boundary, over), Offset::new(10.0, 0.0));
    }

    #[test]
    fn a_translation_past_the_boundary_is_clamped_back() {
        // The child is 100x100, the boundary is the child (no margin), the
        // viewport is 50x50 at scale 1: panning right by 10 would expose 10
        // pixels past the left edge, so the matrix stays at the boundary.
        let boundary = rect(0.0, 0.0, 100.0, 100.0);
        let viewport = rect(0.0, 0.0, 50.0, 50.0);
        let home = Affine2D::IDENTITY;
        let moved = matrix_translate(
            home,
            Offset::new(10.0, 0.0),
            PanAxis::Free,
            None,
            boundary,
            viewport,
            0.0,
        );
        assert_eq!(moved, home);
        // Panning left by 10 is fully inside: the viewport then covers
        // scene x 10..60 of a 100-wide boundary.
        let moved = matrix_translate(
            home,
            Offset::new(-10.0, 0.0),
            PanAxis::Free,
            None,
            boundary,
            viewport,
            0.0,
        );
        assert_eq!(moved, home.translate_by_double(-10.0, 0.0));
        // Panning past the far edge clamps to it.
        let moved = matrix_translate(
            home,
            Offset::new(-80.0, 0.0),
            PanAxis::Free,
            None,
            boundary,
            viewport,
            0.0,
        );
        assert_eq!(moved, home.translate_by_double(-50.0, 0.0));
    }

    #[test]
    fn an_infinite_boundary_allows_any_translation() {
        let boundary = rect(
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        );
        let viewport = rect(0.0, 0.0, 50.0, 50.0);
        let moved = matrix_translate(
            Affine2D::IDENTITY,
            Offset::new(1e6, -1e6),
            PanAxis::Free,
            None,
            boundary,
            viewport,
            0.0,
        );
        assert_eq!(moved.translation(), Offset::new(1e6, -1e6));
    }

    #[test]
    fn aligned_pan_axis_keeps_only_the_first_direction() {
        let boundary = rect(
            f32::NEG_INFINITY,
            f32::NEG_INFINITY,
            f32::INFINITY,
            f32::INFINITY,
        );
        let viewport = rect(0.0, 0.0, 50.0, 50.0);
        let moved = matrix_translate(
            Affine2D::IDENTITY,
            Offset::new(10.0, 6.0),
            PanAxis::Aligned,
            Some(Axis::Horizontal),
            boundary,
            viewport,
            0.0,
        );
        assert_eq!(moved.translation(), Offset::new(10.0, 0.0));
    }

    #[test]
    fn a_scale_beyond_the_limits_is_clamped() {
        // Child and boundary 100x100, viewport 50x50. Scaling the identity by
        // 4 exceeds maxScale 2.5, so 2.5 is what the matrix gets.
        let viewport = rect(0.0, 0.0, 50.0, 50.0);
        let boundary = rect(0.0, 0.0, 100.0, 100.0);
        let scaled = matrix_scale(Affine2D::IDENTITY, 4.0, 0.8, 2.5, viewport, boundary);
        assert_eq!(scaled.max_scale_on_axis(), 2.5);
        // Scaling down to 0.1 falls below minScale 0.8.
        let scaled = matrix_scale(Affine2D::IDENTITY, 0.1, 0.8, 2.5, viewport, boundary);
        assert_eq!(scaled.max_scale_on_axis(), 0.8);
    }

    #[test]
    fn a_scale_cannot_make_the_child_smaller_than_the_viewport() {
        // `_matrixScale`'s boundary-fit floor: the viewport is as big as the
        // boundary, so any scale below 1 would expose the outside even with
        // minScale lower.
        let viewport = rect(0.0, 0.0, 100.0, 100.0);
        let boundary = rect(0.0, 0.0, 100.0, 100.0);
        let scaled = matrix_scale(Affine2D::IDENTITY, 0.5, 0.01, 2.5, viewport, boundary);
        assert_eq!(scaled.max_scale_on_axis(), 1.0);
    }

    #[test]
    fn pan_axis_of_picks_the_longer_leg() {
        assert_eq!(
            pan_axis_of(Offset::ZERO, Offset::new(10.0, 4.0)),
            Some(Axis::Horizontal)
        );
        assert_eq!(
            pan_axis_of(Offset::ZERO, Offset::new(4.0, 10.0)),
            Some(Axis::Vertical)
        );
        assert_eq!(pan_axis_of(Offset::ZERO, Offset::ZERO), None);
    }

    #[test]
    fn final_time_is_upstreams_formula() {
        // `_getFinalTime`: log(effectivelyMotionless / velocity) /
        // log(drag / 100). Positive while the fling outruns the motionless
        // threshold, and the duration the inertia tween runs for -- at whose
        // end the value is the simulations' final_x by the tween's
        // construction, whatever the raw simulations would say at that time.
        let drag = INTERACTION_END_FRICTION_COEFFICIENT;
        let t = final_time(1000.0, drag, 10.0);
        assert!((t - (10.0f32 / 1000.0).ln() / (drag / 100.0).ln()).abs() < 1e-6);
        assert!(t > 0.0);
        // A fling at exactly the motionless threshold takes no time.
        assert_eq!(final_time(10.0, drag, 10.0), 0.0);
    }
}
