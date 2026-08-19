//! The transition family, from upstream `widgets/transitions.dart`.
//!
//! An *implicit* animation ([`crate::implicit`]) is given a target and finds
//! its own way there. An *explicit* one is handed an animation somebody else
//! drives, and does one thing with its value: slide, scale, rotate, fade,
//! clip, position, decorate, align. That is this file.
//!
//! # What an `Animation<T>` is here
//!
//! Upstream every transition takes an `Animation<T>` -- `Animation<Offset>`
//! for a slide, `Animation<RelativeRect>` for a position -- built by handing
//! a `Tween<T>` a driving `Animation<double>` (`tween.animate(controller)`).
//! This crate's [`Animation`](crate::animation::Animation) is the driving
//! half only: it answers an `f32`, because a trait with an associated output
//! could not be the `Rc<dyn Animation>` that [`ProxyAnimation`] and
//! [`CurvedAnimation`] pass around. So the typed animation is spelled as the
//! pair it is made of -- the driver and the tween, handed over separately and
//! evaluated together:
//!
//! ```ignore
//! // Upstream: SlideTransition(position: tween.animate(controller), child: ...)
//! SlideTransition::new(controller, OffsetTween::new(a, b), || child()).into_widget()
//! ```
//!
//! # Listening, in a frame-driven tree
//!
//! Upstream's `AnimatedWidget` adds a listener in `initState` and calls
//! `setState` from it. Here the equivalent runs in
//! [`advance`](crate::framework::StatefulComponent::advance), which every
//! mounted component gets once per frame before anything is built: the
//! transition compares the animation's value against the one it last drew and
//! asks for a rebuild when they differ. Same rule -- draw when the value
//! moved -- read from the other end.
//!
//! # Recorded divergences
//!
//! * A child is a closure here, not a widget instance. Upstream's
//!   `AnimatedWidget` keeps the `child` it was given and hands the same
//!   instance back on every rebuild, which is the documented reason to pass
//!   `child` to a builder rather than build it inside. `AnyWidget` is not
//!   `Clone` (its closures are not), so the child is rebuilt and the element
//!   tree reconciles it; the effect is the same and the saving is not.
//! * `filterQuality` is dropped: the engine ABI has no image-filter quality
//!   on a transform layer.
//! * `alwaysIncludeSemantics` is dropped: `RenderOpacity` drops the subtree
//!   from semantics exactly when it stops painting it (upstream's default),
//!   and there is no flag to keep it.
//! * `PositionedTransition` and `RelativePositionedTransition` are positions,
//!   not widgets -- see their own notes.

use std::cell::RefCell;
use std::rc::Rc;

use crate::animation::{Animatable, Animation, AnimationListener, AnimationStatus, Tween};
use crate::decoration::Decoration;
use crate::direction::TextDirection;
use crate::foundation::Listenable;
use crate::framework::{
    AnyWidget, BuildContext, Key, StateHandle, StatefulComponent, single, stateful,
};
use crate::painting::Matrix4;
use crate::render::{
    Alignment, AlignmentDirectional, AlignmentGeometry, Axis, DecorationPosition, Offset,
    ProxySliverBehavior, RelativeRect, RenderAlign, RenderClipRect, RenderDecoratedBox,
    RenderFractionalTranslation, RenderOpacity, RenderProxySliver, RenderTransform, Size,
    StackPosition,
};

/// What a transition last drew: the value and the status it was built from.
///
/// Upstream holds no such thing -- it is told when to rebuild. Here the tell
/// is a per-frame comparison, so the last answer has to be kept.
#[derive(Default)]
pub struct AnimatedWidgetState {
    drawn: RefCell<Option<(f32, AnimationStatus)>>,
}

/// Upstream `AnimatedWidget`: the base every transition is built on -- an
/// animation, and a way to build from its value.
///
/// The concrete transitions below are all this widget with their own build
/// closure; use it directly for a shape the family does not cover.
pub struct AnimatedWidget<F> {
    animation: Rc<dyn Animation>,
    build: F,
    key: Key,
}

impl<F> AnimatedWidget<F>
where
    F: Fn(&dyn Animation) -> AnyWidget + 'static,
{
    pub fn new(animation: Rc<dyn Animation>, build: F) -> AnimatedWidget<F> {
        AnimatedWidget {
            animation,
            build,
            key: None,
        }
    }

    pub fn with_key(mut self, key: u64) -> Self {
        self.key = Some(key);
        self
    }

    pub fn into_widget(self) -> AnyWidget {
        stateful(self)
    }
}

impl<F> StatefulComponent for AnimatedWidget<F>
where
    F: Fn(&dyn Animation) -> AnyWidget + 'static,
{
    type State = AnimatedWidgetState;

    fn key(&self) -> Key {
        self.key
    }

    /// Upstream's `_handleChange`, arrived at from the other side: rebuild
    /// when what is on screen is no longer what the animation says.
    ///
    /// The `is_animating` half is what keeps the frames coming: a running
    /// animation asks for the next frame even on the tick where its value
    /// happened not to move, exactly as an upstream controller notifies its
    /// listeners on every tick it takes.
    fn advance(&self, state: &mut Self::State, _frame_time_micros: i64) -> bool {
        let now = (self.animation.value(), self.animation.status());
        *state.drawn.borrow() != Some(now) || self.animation.is_animating()
    }

    fn build(
        &self,
        state: &Self::State,
        _handle: StateHandle<Self::State>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        *state.drawn.borrow_mut() = Some((self.animation.value(), self.animation.status()));
        (self.build)(&*self.animation)
    }
}

/// [`AnimatedWidget`] as a widget.
pub fn animated_widget<F>(animation: Rc<dyn Animation>, build: F) -> AnyWidget
where
    F: Fn(&dyn Animation) -> AnyWidget + 'static,
{
    AnimatedWidget::new(animation, build).into_widget()
}

// -- The listenable builders --------------------------------------------------

/// Upstream `ListenableBuilder`: rebuilt whenever a [`Listenable`] says so.
///
/// A [`ChangeNotifier`](crate::foundation::ChangeNotifier) is not a clock, so
/// this one cannot be polled per frame the way [`AnimatedWidget`] is. It
/// subscribes instead, on its first build, and the callback marks the element
/// dirty through its [`StateHandle`] -- which is upstream's
/// `addListener(_handleChange)` / `setState` pair with the same two halves.
pub struct ListenableBuilder<F> {
    listenable: Rc<dyn Listenable>,
    builder: F,
    key: Key,
}

/// The subscription a [`ListenableBuilder`] holds, kept so it can be dropped
/// with the element -- upstream's `dispose` removing its listener.
#[derive(Default)]
pub struct ListenableBuilderState {
    subscription: RefCell<Option<Rc<dyn Fn()>>>,
}

impl<F> ListenableBuilder<F>
where
    F: Fn() -> AnyWidget + 'static,
{
    pub fn new(listenable: Rc<dyn Listenable>, builder: F) -> ListenableBuilder<F> {
        ListenableBuilder {
            listenable,
            builder,
            key: None,
        }
    }

    pub fn with_key(mut self, key: u64) -> Self {
        self.key = Some(key);
        self
    }

    pub fn into_widget(self) -> AnyWidget {
        stateful(self)
    }
}

impl<F> StatefulComponent for ListenableBuilder<F>
where
    F: Fn() -> AnyWidget + 'static,
{
    type State = ListenableBuilderState;

    fn key(&self) -> Key {
        self.key
    }

    fn build(
        &self,
        state: &Self::State,
        handle: StateHandle<Self::State>,
        _context: &mut BuildContext,
    ) -> AnyWidget {
        if state.subscription.borrow().is_none() {
            // Upstream subscribes in `initState`, which is the first build's
            // moment. `set_state` with an empty mutation is a rebuild request
            // and nothing else, which is what `setState(() {})` is.
            let callback: Rc<dyn Fn()> = Rc::new(move || {
                handle.set_state(|_| {});
            });
            self.listenable.add_listener(Rc::clone(&callback));
            *state.subscription.borrow_mut() = Some(callback);
        }
        (self.builder)()
    }
}

/// Upstream `AnimatedBuilder`: [`ListenableBuilder`] under its animation
/// name. Upstream's takes a `Listenable` too and only its documentation
/// differs; here it takes an [`Animation`], because that is the type its
/// callers hold and it saves them the frame-driven/subscribed distinction.
pub struct AnimatedBuilder;

impl AnimatedBuilder {
    #[allow(clippy::new_ret_no_self)]
    pub fn new<F>(animation: Rc<dyn Animation>, builder: F) -> AnyWidget
    where
        F: Fn() -> AnyWidget + 'static,
    {
        animated_widget(animation, move |_| builder())
    }
}

// -- Tweens the transitions want ----------------------------------------------

/// Upstream `AlignmentTween` (`rendering/tweens.dart`): two absolute
/// alignments interpolated.
#[derive(Clone, Copy, Debug)]
pub struct AlignmentTween {
    pub begin: Alignment,
    pub end: Alignment,
}

impl Tween for AlignmentTween {
    type Output = Alignment;

    fn lerp(&self, t: f32) -> Alignment {
        Alignment::lerp(self.begin, self.end, t)
    }
}

impl Animatable for AlignmentTween {
    type Output = Alignment;

    fn transform(&self, t: f32) -> Alignment {
        self.lerp(t)
    }
}

/// Upstream `AlignmentGeometryTween` (`rendering/tweens.dart`): the alignment
/// families interpolated, mixed pairs included.
#[derive(Clone, Copy, Debug)]
pub struct AlignmentGeometryTween {
    pub begin: AlignmentGeometry,
    pub end: AlignmentGeometry,
}

impl Tween for AlignmentGeometryTween {
    type Output = AlignmentGeometry;

    fn lerp(&self, t: f32) -> AlignmentGeometry {
        // Upstream's `AlignmentGeometryTween.lerp` is
        // `AlignmentGeometry.lerp(begin, end, t)!`: both ends are here, so
        // the null branches cannot be reached.
        AlignmentGeometry::lerp(Some(self.begin), Some(self.end), t).expect("both ends given")
    }
}

impl Animatable for AlignmentGeometryTween {
    type Output = AlignmentGeometry;

    fn transform(&self, t: f32) -> AlignmentGeometry {
        self.lerp(t)
    }
}

/// Upstream `DecorationTween` (`widgets/implicit_animations.dart`): whatever
/// [`Decoration::lerp`] can get between.
#[derive(Clone, Debug)]
pub struct DecorationTween {
    pub begin: Decoration,
    pub end: Decoration,
}

impl Tween for DecorationTween {
    type Output = Decoration;

    fn lerp(&self, t: f32) -> Decoration {
        Decoration::lerp(Some(self.begin.clone()), Some(self.end.clone()), t)
            .expect("both ends given")
    }
}

impl Animatable for DecorationTween {
    type Output = Decoration;

    fn transform(&self, t: f32) -> Decoration {
        self.lerp(t)
    }
}

/// Upstream `RelativeRectTween`: a [`RelativeRect`] interpolated, which is
/// what a position animates through when the container's size is not known
/// until layout.
#[derive(Clone, Copy, Debug)]
pub struct RelativeRectTween {
    pub begin: RelativeRect,
    pub end: RelativeRect,
}

impl RelativeRectTween {
    pub const fn new(begin: RelativeRect, end: RelativeRect) -> RelativeRectTween {
        RelativeRectTween { begin, end }
    }
}

impl Tween for RelativeRectTween {
    type Output = RelativeRect;

    fn lerp(&self, t: f32) -> RelativeRect {
        // Upstream's `RelativeRect.lerp(begin, end, t)!`: both ends are here,
        // so the null branches cannot be reached.
        RelativeRect::lerp(Some(self.begin), Some(self.end), t).expect("both ends given")
    }
}

impl Animatable for RelativeRectTween {
    type Output = RelativeRect;

    fn transform(&self, t: f32) -> RelativeRect {
        self.lerp(t)
    }
}

// -- The transitions ----------------------------------------------------------

/// Upstream `SlideTransition`: the child moved by a fraction of its own size.
pub struct SlideTransition<T, F> {
    position: Rc<dyn Animation>,
    tween: T,
    transform_hit_tests: bool,
    text_direction: Option<TextDirection>,
    child: F,
}

impl<T, F> SlideTransition<T, F>
where
    T: Animatable<Output = Offset> + 'static,
    F: Fn() -> AnyWidget + 'static,
{
    pub fn new(position: Rc<dyn Animation>, tween: T, child: F) -> SlideTransition<T, F> {
        SlideTransition {
            position,
            tween,
            transform_hit_tests: true,
            text_direction: None,
            child,
        }
    }

    /// Upstream `transformHitTests`, defaulting to true as upstream does.
    pub fn with_transform_hit_tests(mut self, transform: bool) -> Self {
        self.transform_hit_tests = transform;
        self
    }

    /// Upstream `textDirection`: with one, a positive `dx` moves the child in
    /// reading order rather than rightwards.
    pub fn with_text_direction(mut self, direction: TextDirection) -> Self {
        self.text_direction = Some(direction);
        self
    }

    pub fn into_widget(self) -> AnyWidget {
        let SlideTransition {
            position,
            tween,
            transform_hit_tests,
            text_direction,
            child,
        } = self;
        animated_widget(position, move |animation| {
            let mut offset = tween.transform(animation.value());
            if text_direction == Some(TextDirection::Rtl) {
                offset = Offset::new(-offset.dx, offset.dy);
            }
            single(child(), move |inner| {
                RenderFractionalTranslation::new((offset.dx, offset.dy), inner)
                    .with_transform_hit_tests(transform_hit_tests)
            })
        })
    }
}

/// Upstream `MatrixTransition`: a matrix computed from the animation on every
/// frame and applied to the child.
///
/// The engine's transform layer takes a 2D affine, so the matrix is flattened
/// to one: the z row and the perspective column are dropped. Upstream's own
/// dartpad example (a Y-axis rotation with perspective) is therefore not
/// reproducible here -- the same limit `RenderTransform` already carries.
pub struct MatrixTransition<M, F> {
    animation: Rc<dyn Animation>,
    on_transform: M,
    alignment: Alignment,
    child: F,
}

impl<M, F> MatrixTransition<M, F>
where
    M: Fn(f32) -> Matrix4 + 'static,
    F: Fn() -> AnyWidget + 'static,
{
    pub fn new(animation: Rc<dyn Animation>, on_transform: M, child: F) -> MatrixTransition<M, F> {
        MatrixTransition {
            animation,
            on_transform,
            alignment: Alignment::CENTER,
            child,
        }
    }

    /// Upstream `alignment`, the origin the transform happens about.
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub fn into_widget(self) -> AnyWidget {
        let MatrixTransition {
            animation,
            on_transform,
            alignment,
            child,
        } = self;
        animated_widget(animation, move |animation| {
            let affine = flatten(on_transform(animation.value()));
            single(child(), move |inner| {
                RenderTransform::new(affine, inner).with_origin(alignment)
            })
        })
    }
}

/// A `Matrix4` as the engine's `[a, b, c, d, e, f]` affine: the two upper
/// rows of the two leading columns, and the translation.
fn flatten(matrix: Matrix4) -> [f32; 6] {
    let m = &matrix.storage;
    // Column-major: `storage[column * 4 + row]`.
    [m[0], m[1], m[4], m[5], m[12], m[13]]
}

/// Upstream `ScaleTransition`: [`MatrixTransition`] with
/// `Matrix4.diagonal3Values(v, v, 1)`.
pub struct ScaleTransition;

impl ScaleTransition {
    #[allow(clippy::new_ret_no_self)]
    pub fn new<F>(scale: Rc<dyn Animation>, child: F) -> MatrixTransition<fn(f32) -> Matrix4, F>
    where
        F: Fn() -> AnyWidget + 'static,
    {
        MatrixTransition::new(scale, Self::handle_scale_matrix, child)
    }

    /// Upstream's `_handleScaleMatrix`.
    fn handle_scale_matrix(value: f32) -> Matrix4 {
        Matrix4::diagonal3_values(value, value, 1.0)
    }
}

/// Upstream `RotationTransition`: [`MatrixTransition`] with
/// `Matrix4.rotationZ(v * 2 * pi)` -- the animation counts whole turns.
pub struct RotationTransition;

impl RotationTransition {
    #[allow(clippy::new_ret_no_self)]
    pub fn new<F>(turns: Rc<dyn Animation>, child: F) -> MatrixTransition<fn(f32) -> Matrix4, F>
    where
        F: Fn() -> AnyWidget + 'static,
    {
        MatrixTransition::new(turns, Self::handle_turns_matrix, child)
    }

    /// Upstream's `_handleTurnsMatrix`.
    fn handle_turns_matrix(value: f32) -> Matrix4 {
        Matrix4::rotation_z(value * std::f32::consts::PI * 2.0)
    }
}

/// Upstream `SizeTransition`: a clip that grows, with the child aligned
/// inside it. The child is laid out at its full size throughout and clipped
/// to a fraction of it; nothing about the child's own layout animates.
pub struct SizeTransition<F> {
    size_factor: Rc<dyn Animation>,
    axis: Axis,
    alignment: Option<AlignmentGeometry>,
    axis_alignment: Option<f32>,
    fixed_cross_axis_size_factor: Option<f32>,
    child: F,
}

impl<F> SizeTransition<F>
where
    F: Fn() -> AnyWidget + 'static,
{
    pub fn new(size_factor: Rc<dyn Animation>, child: F) -> SizeTransition<F> {
        SizeTransition {
            size_factor,
            axis: Axis::Vertical,
            alignment: None,
            axis_alignment: None,
            fixed_cross_axis_size_factor: None,
            child,
        }
    }

    /// Which axis the factor scales. Upstream defaults to `Axis.vertical`.
    pub fn with_axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    pub fn with_alignment(mut self, alignment: AlignmentGeometry) -> Self {
        self.alignment = Some(alignment);
        self
    }

    /// Upstream's deprecated `axisAlignment`: one number on the animating
    /// axis, the other end pinned. Kept because upstream still builds from it
    /// when `alignment` is absent, and the two are mutually exclusive there.
    pub fn with_axis_alignment(mut self, axis_alignment: f32) -> Self {
        self.axis_alignment = Some(axis_alignment);
        self
    }

    /// Upstream `fixedCrossAxisSizeFactor`: the cross axis clipped to a fixed
    /// fraction instead of filling the parent.
    pub fn with_fixed_cross_axis_size_factor(mut self, factor: f32) -> Self {
        debug_assert!(factor >= 0.0, "a cross-axis factor is not negative");
        self.fixed_cross_axis_size_factor = Some(factor);
        self
    }

    pub fn into_widget(self) -> AnyWidget {
        let SizeTransition {
            size_factor,
            axis,
            alignment,
            axis_alignment,
            fixed_cross_axis_size_factor,
            child,
        } = self;
        debug_assert!(
            axis_alignment.is_none() || alignment.is_none(),
            "cannot provide both axisAlignment and alignment"
        );
        animated_widget(size_factor, move |animation| {
            // Upstream's build, line for line: the factor floors at zero on
            // the animating axis (an overshooting curve must not invert the
            // clip), and the cross axis takes the fixed factor or nothing.
            let factor = animation.value().max(0.0);
            let (width_factor, height_factor) = match axis {
                Axis::Horizontal => (Some(factor), fixed_cross_axis_size_factor),
                Axis::Vertical => (fixed_cross_axis_size_factor, Some(factor)),
            };
            let resolved = alignment
                .unwrap_or_else(|| default_size_alignment(axis, axis_alignment))
                .resolve(crate::direction::current_direction());
            single(child(), move |inner| {
                RenderClipRect::new(
                    RenderAlign::new(resolved, inner).with_factors(width_factor, height_factor),
                )
            })
        })
    }
}

/// Upstream `SizeTransition.build`'s alignment switch: start-relative on the
/// animating axis, pinned to the leading edge on the other.
fn default_size_alignment(axis: Axis, axis_alignment: Option<f32>) -> AlignmentGeometry {
    let along = axis_alignment.unwrap_or(0.0);
    let (start, y) = match axis {
        Axis::Horizontal => (along, -1.0),
        Axis::Vertical => (-1.0, along),
    };
    AlignmentGeometry::Directional(AlignmentDirectional { start, y })
}

/// Upstream `FadeTransition`: the child painted at the animation's opacity.
///
/// Setting the opacity to zero does not stop hit testing -- upstream says the
/// same, and gives the same advice: wrap in an `IgnorePointer` at the ends if
/// an invisible child should also be untouchable.
pub struct FadeTransition<F> {
    opacity: Rc<dyn Animation>,
    child: F,
}

impl<F> FadeTransition<F>
where
    F: Fn() -> AnyWidget + 'static,
{
    pub fn new(opacity: Rc<dyn Animation>, child: F) -> FadeTransition<F> {
        FadeTransition { opacity, child }
    }

    pub fn into_widget(self) -> AnyWidget {
        let FadeTransition { opacity, child } = self;
        animated_widget(opacity, move |animation| {
            let value = animation.value();
            single(child(), move |inner| RenderOpacity::new(value, inner))
        })
    }
}

/// Upstream `SliverFadeTransition`: [`FadeTransition`] for a sliver child.
pub struct SliverFadeTransition<F> {
    opacity: Rc<dyn Animation>,
    sliver: F,
}

impl<F> SliverFadeTransition<F>
where
    F: Fn() -> AnyWidget + 'static,
{
    pub fn new(opacity: Rc<dyn Animation>, sliver: F) -> SliverFadeTransition<F> {
        SliverFadeTransition { opacity, sliver }
    }

    pub fn into_widget(self) -> AnyWidget {
        let SliverFadeTransition { opacity, sliver } = self;
        animated_widget(opacity, move |animation| {
            let value = animation.value();
            single(sliver(), move |inner| {
                RenderProxySliver::new(ProxySliverBehavior::Opacity(value), inner)
            })
        })
    }
}

/// Upstream `PositionedTransition`: a stacked child whose four insets are
/// animated.
///
/// Upstream's is a `Positioned`, and `Positioned` is a `ParentDataWidget` the
/// enclosing `Stack` reads off its child. This crate's stack takes its
/// children's positions directly ([`RenderStack::push_positioned`]), so there
/// is nothing for a child widget to annotate: the transition is the position,
/// and the stack asks it for one on every build. The enclosing component is
/// already rebuilding every frame -- it is the one ticking the controller --
/// so no listener is needed on this side.
///
/// [`RenderStack::push_positioned`]: crate::render::RenderStack::push_positioned
pub struct PositionedTransition<T> {
    rect: Rc<dyn Animation>,
    tween: T,
}

impl<T> PositionedTransition<T>
where
    T: Animatable<Output = RelativeRect>,
{
    pub fn new(rect: Rc<dyn Animation>, tween: T) -> PositionedTransition<T> {
        PositionedTransition { rect, tween }
    }

    /// The rect this animation is at now.
    pub fn rect(&self) -> RelativeRect {
        self.tween.transform(self.rect.value())
    }

    /// That rect as a stack position -- upstream's
    /// `Positioned.fromRelativeRect`.
    pub fn position(&self) -> StackPosition {
        self.rect().to_stack_position()
    }
}

/// Upstream `RelativePositionedTransition`: the animated value is a `Rect`
/// inside a container of a known `size`, and the position is what is left
/// over on each side.
///
/// A position rather than a widget, for the same reason as
/// [`PositionedTransition`].
pub struct RelativePositionedTransition<T> {
    rect: Rc<dyn Animation>,
    tween: T,
    size: Size,
}

impl<T> RelativePositionedTransition<T>
where
    T: Animatable<Output = crate::engine::Rect>,
{
    pub fn new(rect: Rc<dyn Animation>, tween: T, size: Size) -> RelativePositionedTransition<T> {
        RelativePositionedTransition { rect, tween, size }
    }

    /// Upstream's build: `RelativeRect.fromSize(rect.value ?? Rect.zero, size)`.
    pub fn relative_rect(&self) -> RelativeRect {
        RelativeRect::from_size(self.tween.transform(self.rect.value()), self.size)
    }

    pub fn position(&self) -> StackPosition {
        self.relative_rect().to_stack_position()
    }
}

/// Upstream `DecoratedBoxTransition`: a decoration interpolated behind (or
/// over) the child.
pub struct DecoratedBoxTransition<T, F> {
    decoration: Rc<dyn Animation>,
    tween: T,
    position: DecorationPosition,
    child: F,
}

impl<T, F> DecoratedBoxTransition<T, F>
where
    T: Animatable<Output = Decoration> + 'static,
    F: Fn() -> AnyWidget + 'static,
{
    pub fn new(decoration: Rc<dyn Animation>, tween: T, child: F) -> DecoratedBoxTransition<T, F> {
        DecoratedBoxTransition {
            decoration,
            tween,
            position: DecorationPosition::Background,
            child,
        }
    }

    /// Upstream `position`, defaulting to `DecorationPosition.background`.
    pub fn with_position(mut self, position: DecorationPosition) -> Self {
        self.position = position;
        self
    }

    pub fn into_widget(self) -> AnyWidget {
        let DecoratedBoxTransition {
            decoration,
            tween,
            position,
            child,
        } = self;
        animated_widget(decoration, move |animation| {
            let painted = tween.transform(animation.value());
            single(child(), move |inner| {
                RenderDecoratedBox::new()
                    .with_decoration(painted.clone())
                    .with_position(position)
                    .with_child(inner)
            })
        })
    }
}

/// Upstream `AlignTransition`: the child's alignment animated.
pub struct AlignTransition<T, F> {
    alignment: Rc<dyn Animation>,
    tween: T,
    width_factor: Option<f32>,
    height_factor: Option<f32>,
    child: F,
}

impl<T, F> AlignTransition<T, F>
where
    T: Animatable<Output = AlignmentGeometry> + 'static,
    F: Fn() -> AnyWidget + 'static,
{
    pub fn new(alignment: Rc<dyn Animation>, tween: T, child: F) -> AlignTransition<T, F> {
        AlignTransition {
            alignment,
            tween,
            width_factor: None,
            height_factor: None,
            child,
        }
    }

    pub fn with_factors(mut self, width: Option<f32>, height: Option<f32>) -> Self {
        self.width_factor = width;
        self.height_factor = height;
        self
    }

    pub fn into_widget(self) -> AnyWidget {
        let AlignTransition {
            alignment,
            tween,
            width_factor,
            height_factor,
            child,
        } = self;
        animated_widget(alignment, move |animation| {
            let resolved = tween
                .transform(animation.value())
                .resolve(crate::direction::current_direction());
            single(child(), move |inner| {
                RenderAlign::new(resolved, inner).with_factors(width_factor, height_factor)
            })
        })
    }
}

/// A listener that only tells, for a caller that wants to know an animation
/// moved without holding a widget -- the crate's spelling of
/// `animation.addListener(callback)`.
pub fn on_animation_change(animation: &dyn Animation, callback: Rc<dyn Fn()>) -> AnimationListener {
    let listener = AnimationListener {
        on_value: callback,
        on_status: None,
    };
    animation.add_listener(listener.clone());
    listener
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{AnimationController, Curve, OffsetTween, RectTween};
    use crate::decoration::BoxDecoration;
    use crate::engine::{Color, Rect};
    use crate::framework::{ElementTree, leaf};
    use crate::render::{BoxConstraints, BoxedRender, Fill, HitTestResult, RenderBox};
    use crate::widgets::SizedBox;
    use std::time::Duration;

    fn controller_at(value: f32) -> Rc<AnimationController> {
        let controller = AnimationController::new(Duration::from_millis(100));
        controller.set_value(value);
        controller
    }

    /// Mounts a widget, builds its render tree and lays it out.
    fn laid_out(widget: AnyWidget, width: f32, height: f32) -> BoxedRender {
        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        let mut root = tree.build_render_tree().expect("a root");
        root.layout(BoxConstraints::loose(width, height));
        root
    }

    fn child_offsets(root: &BoxedRender) -> Vec<Offset> {
        let mut seen = Vec::new();
        root.visit_children(&mut |_, offset| seen.push(offset));
        seen
    }

    #[test]
    fn a_slide_moves_the_child_by_a_fraction_of_its_own_size() {
        let controller = controller_at(1.0);
        let widget = SlideTransition::new(
            controller,
            OffsetTween::new(Offset::ZERO, Offset::new(0.5, 0.25)),
            || leaf(|| SizedBox::new(40.0, 20.0)),
        )
        .into_widget();

        let root = laid_out(widget, 100.0, 100.0);
        // Half the child's width across, a quarter of its height down.
        assert_eq!(child_offsets(&root), vec![Offset::new(20.0, 5.0)]);
    }

    #[test]
    fn a_slide_that_does_not_transform_hit_tests_is_hit_where_it_was_laid_out() {
        // A bare `SizedBox` is not a hit target -- upstream's `hitTestSelf`
        // is false for anything that only holds a size -- so the child here
        // is a decorated box, which is one.
        let hittable = || leaf(|| RenderDecoratedBox::new().with_child(SizedBox::new(40.0, 20.0)));
        let widget = SlideTransition::new(
            controller_at(1.0),
            OffsetTween::new(Offset::ZERO, Offset::new(1.0, 0.0)),
            hittable,
        )
        .with_transform_hit_tests(false)
        .into_widget();

        let root = laid_out(widget, 100.0, 100.0);
        let mut result = HitTestResult::new();
        // Painted a whole width to the right, and hit at the origin all the
        // same -- upstream's `transformHitTests: false`.
        assert!(root.hit_test(Offset::new(10.0, 10.0), &mut result));

        let moved = SlideTransition::new(
            controller_at(1.0),
            OffsetTween::new(Offset::ZERO, Offset::new(1.0, 0.0)),
            hittable,
        )
        .into_widget();
        let moved = laid_out(moved, 100.0, 100.0);
        let mut miss = HitTestResult::new();
        assert!(
            !moved.hit_test(Offset::new(10.0, 10.0), &mut miss),
            "with the default the hit test moves with the paint"
        );
    }

    #[test]
    fn an_rtl_slide_reads_a_positive_dx_as_leftwards() {
        let widget = SlideTransition::new(
            controller_at(1.0),
            OffsetTween::new(Offset::ZERO, Offset::new(0.5, 0.0)),
            || leaf(|| SizedBox::new(40.0, 20.0)),
        )
        .with_text_direction(TextDirection::Rtl)
        .into_widget();

        let root = laid_out(widget, 100.0, 100.0);
        assert_eq!(child_offsets(&root), vec![Offset::new(-20.0, 0.0)]);
    }

    #[test]
    fn a_scale_is_a_diagonal_matrix_and_a_rotation_is_a_z_rotation() {
        assert_eq!(
            flatten(ScaleTransition::handle_scale_matrix(2.0)),
            [2.0, 0.0, 0.0, 2.0, 0.0, 0.0]
        );
        // A quarter turn: the x axis maps onto the y axis.
        let quarter = flatten(RotationTransition::handle_turns_matrix(0.25));
        assert!(quarter[0].abs() < 1e-6);
        assert!((quarter[1] - 1.0).abs() < 1e-6);
        assert!((quarter[2] + 1.0).abs() < 1e-6);
        assert!(quarter[3].abs() < 1e-6);
    }

    #[test]
    fn a_size_transition_clips_to_a_fraction_of_the_child() {
        let widget = SizeTransition::new(controller_at(0.5), || leaf(|| SizedBox::new(40.0, 20.0)))
            .into_widget();
        // Vertical by default: half the child's height. The cross axis has
        // no factor, so the align inside fills the bounded width it was
        // given -- upstream's `Align` shrink-wraps only where a factor says
        // to, and `SizeTransition` passes none on the cross axis unless
        // `fixedCrossAxisSizeFactor` is set.
        assert_eq!(
            laid_out(widget, 100.0, 100.0).size(),
            Size::new(100.0, 10.0)
        );

        let across =
            SizeTransition::new(controller_at(0.25), || leaf(|| SizedBox::new(40.0, 20.0)))
                .with_axis(Axis::Horizontal)
                .into_widget();
        assert_eq!(
            laid_out(across, 100.0, 100.0).size(),
            Size::new(10.0, 100.0)
        );
    }

    #[test]
    fn a_fixed_cross_axis_factor_clips_the_other_axis_too() {
        let widget = SizeTransition::new(controller_at(0.5), || leaf(|| SizedBox::new(40.0, 20.0)))
            .with_fixed_cross_axis_size_factor(0.5)
            .into_widget();
        assert_eq!(laid_out(widget, 100.0, 100.0).size(), Size::new(20.0, 10.0));
    }

    #[test]
    fn a_size_factor_below_zero_floors_rather_than_inverting() {
        // Upstream's `math.max(sizeFactor.value, 0.0)`: an overshooting curve
        // must not invert the clip.
        let controller = AnimationController::new(Duration::from_millis(100));
        controller.set_value(-1.0);
        let widget = SizeTransition::new(controller as Rc<dyn Animation>, || {
            leaf(|| SizedBox::new(40.0, 20.0))
        })
        .into_widget();
        assert_eq!(laid_out(widget, 100.0, 100.0).size(), Size::new(100.0, 0.0));
    }

    #[test]
    fn a_relative_rect_tween_walks_the_four_insets() {
        let tween = RelativeRectTween::new(
            RelativeRect::FILL,
            RelativeRect::from_ltrb(10.0, 20.0, 30.0, 40.0),
        );
        assert_eq!(
            tween.lerp(0.5),
            RelativeRect::from_ltrb(5.0, 10.0, 15.0, 20.0)
        );
    }

    #[test]
    fn a_relative_rect_resolves_against_its_container_both_ways() {
        let rect =
            RelativeRect::from_size(Rect::ltrb(10.0, 20.0, 60.0, 70.0), Size::new(100.0, 100.0));
        assert_eq!(rect, RelativeRect::from_ltrb(10.0, 20.0, 40.0, 30.0));
        assert_eq!(
            rect.to_rect(Rect::ltrb(0.0, 0.0, 100.0, 100.0)),
            Rect::ltrb(10.0, 20.0, 60.0, 70.0)
        );
        assert_eq!(rect.to_size(Size::new(100.0, 100.0)), Size::new(50.0, 50.0));
        assert!(rect.has_insets());
        assert!(!RelativeRect::FILL.has_insets());
        // A shift grows two insets and shrinks the opposite two.
        assert_eq!(
            RelativeRect::FILL.shift(Offset::new(5.0, 5.0)),
            RelativeRect::from_ltrb(5.0, 5.0, -5.0, -5.0)
        );
    }

    #[test]
    fn a_positioned_transition_reads_out_as_a_stack_position() {
        let transition = PositionedTransition::new(
            controller_at(1.0),
            RelativeRectTween::new(
                RelativeRect::FILL,
                RelativeRect::from_ltrb(4.0, 8.0, 12.0, 16.0),
            ),
        );
        let position = transition.position();
        assert_eq!(position.left, Some(4.0));
        assert_eq!(position.top, Some(8.0));
        assert_eq!(position.right, Some(12.0));
        assert_eq!(position.bottom, Some(16.0));
        assert_eq!(position.width, None, "four edges settle the extent");
    }

    #[test]
    fn a_relative_positioned_transition_measures_what_is_left_over() {
        let transition = RelativePositionedTransition::new(
            controller_at(1.0),
            RectTween {
                begin: Rect::ltrb(0.0, 0.0, 0.0, 0.0),
                end: Rect::ltrb(10.0, 20.0, 60.0, 70.0),
            },
            Size::new(100.0, 100.0),
        );
        // Right and bottom are what is left of the container beyond the rect.
        assert_eq!(
            transition.relative_rect(),
            RelativeRect::from_ltrb(10.0, 20.0, 40.0, 30.0)
        );
    }

    #[test]
    fn a_fade_at_zero_paints_nothing_and_still_lays_the_child_out() {
        let widget = FadeTransition::new(controller_at(0.0), || leaf(|| SizedBox::new(40.0, 20.0)))
            .into_widget();
        assert_eq!(laid_out(widget, 100.0, 100.0).size(), Size::new(40.0, 20.0));
    }

    #[test]
    fn a_transition_rebuilds_exactly_when_the_value_it_drew_went_stale() {
        let controller = AnimationController::new(Duration::from_millis(100));
        let built = Rc::new(std::cell::Cell::new(0));
        let counter = Rc::clone(&built);
        let widget = FadeTransition::new(Rc::clone(&controller) as Rc<dyn Animation>, move || {
            counter.set(counter.get() + 1);
            leaf(|| SizedBox::new(10.0, 10.0))
        })
        .into_widget();

        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        assert_eq!(built.get(), 1);

        // Nothing running, nothing moved: no rebuild.
        assert!(!tree.advance_frame(0));
        tree.rebuild_dirty();
        assert_eq!(built.get(), 1);

        // Ticked: the value moved, so the frame that follows redraws.
        controller.forward();
        controller.tick(Duration::from_millis(50));
        assert!(tree.advance_frame(16_000));
        tree.rebuild_dirty();
        assert_eq!(built.get(), 2);

        // Landed: the frame that finishes is drawn, and the one after is idle.
        controller.tick(Duration::from_millis(50));
        assert!(tree.advance_frame(32_000));
        tree.rebuild_dirty();
        assert_eq!(built.get(), 3);
        assert!(!tree.advance_frame(48_000));
        tree.rebuild_dirty();
        assert_eq!(built.get(), 3);
    }

    #[test]
    fn a_decorated_box_transition_interpolates_the_decoration() {
        let tween = DecorationTween {
            begin: Decoration::Box(
                BoxDecoration::new().with_fill(Fill::Solid(Color::argb(0, 255, 0, 0))),
            ),
            end: Decoration::Box(
                BoxDecoration::new().with_fill(Fill::Solid(Color::argb(255, 255, 0, 0))),
            ),
        };
        match tween.lerp(0.5) {
            Decoration::Box(decoration) => match decoration.fill.expect("a fill on both ends") {
                Fill::Solid(color) => assert_eq!(color.alpha(), 128),
                other => panic!("expected a solid fill, got {other:?}"),
            },
            other => panic!("expected a box decoration, got {other:?}"),
        }

        // And it reaches the render tree: the transition wraps its child in a
        // decorated box.
        let widget = DecoratedBoxTransition::new(controller_at(1.0), tween, || {
            leaf(|| SizedBox::new(40.0, 20.0))
        })
        .with_position(DecorationPosition::Foreground)
        .into_widget();
        assert_eq!(laid_out(widget, 100.0, 100.0).size(), Size::new(40.0, 20.0));
    }

    #[test]
    fn an_align_transition_walks_the_alignment() {
        let widget = AlignTransition::new(
            controller_at(1.0),
            AlignmentGeometryTween {
                begin: AlignmentGeometry::Absolute(Alignment::new(-1.0, -1.0)),
                end: AlignmentGeometry::Absolute(Alignment::new(1.0, 1.0)),
            },
            || leaf(|| SizedBox::new(40.0, 20.0)),
        )
        .into_widget();

        let root = laid_out(widget, 100.0, 100.0);
        // Aligned to the bottom right of the space it filled.
        assert_eq!(child_offsets(&root), vec![Offset::new(60.0, 80.0)]);
    }

    #[test]
    fn a_listenable_builder_rebuilds_when_it_is_told() {
        let notifier = Rc::new(crate::foundation::ValueNotifier::new(0_i32));
        let built = Rc::new(std::cell::Cell::new(0));
        let counter = Rc::clone(&built);
        let widget =
            ListenableBuilder::new(Rc::clone(&notifier) as Rc<dyn Listenable>, move || {
                counter.set(counter.get() + 1);
                leaf(|| SizedBox::new(10.0, 10.0))
            })
            .into_widget();

        let mut tree = ElementTree::new();
        tree.rebuild(widget);
        assert_eq!(built.get(), 1);

        notifier.set_value(1);
        tree.rebuild_dirty();
        assert_eq!(built.get(), 2);

        // The same value again tells nobody, so nothing is rebuilt.
        notifier.set_value(1);
        tree.rebuild_dirty();
        assert_eq!(built.get(), 2);
    }

    #[test]
    fn a_controller_is_an_animation_the_object_graph_can_take() {
        let controller = AnimationController::new(Duration::from_millis(100));
        assert_eq!(controller.status(), AnimationStatus::Dismissed);
        controller.forward();
        assert_eq!(controller.status(), AnimationStatus::Forward);
        assert!(controller.is_animating());
        controller.tick(Duration::from_millis(100));
        assert_eq!(controller.status(), AnimationStatus::Completed);
        assert!(
            !controller.is_animating(),
            "a stopped ticker is not animating, whatever the status says"
        );

        // And it composes: the object graph takes it as a parent.
        let curved = crate::animation::CurvedAnimation::new(
            Rc::clone(&controller) as Rc<dyn Animation>,
            Curve::Linear,
        );
        assert_eq!(curved.value(), 1.0);
        assert_eq!(curved.status(), AnimationStatus::Completed);
    }

    #[test]
    fn a_controller_given_a_vsync_runs_itself_off_the_frame_clock() {
        use crate::ticker::{TickerProvider, Tickers};

        let tickers = Tickers::new();
        let controller = AnimationController::new(Duration::from_millis(100));
        controller.with_vsync(&tickers);
        assert!(!tickers.tick(0), "nothing started, nothing to tick");

        controller.forward();
        // The first tick sets the ticker's own zero, so it moves nothing.
        assert!(tickers.tick(1_000_000));
        assert_eq!(controller.raw_value(), 0.0);

        // Fifty of a hundred milliseconds later, halfway.
        assert!(tickers.tick(1_050_000));
        assert!((controller.raw_value() - 0.5).abs() < 1e-6);

        // The frame it lands on is the last one it asks for, and it stops
        // its own ticker on the way out.
        assert!(!tickers.tick(1_100_000));
        assert_eq!(controller.raw_value(), 1.0);
        assert_eq!(controller.status(), AnimationStatus::Completed);
        assert!(!controller.is_animating());
    }

    #[test]
    fn a_transition_driven_by_a_vsync_redraws_until_the_animation_lands() {
        use crate::framework::StateHandle;
        use crate::ticker::{TickerProvider, Tickers};

        // The shape a real caller has: a state holding the tickers and the
        // controller, an `advance` that ticks them, and a transition reading
        // the controller.
        struct FadeIn {
            controller: Rc<AnimationController>,
            tickers: Rc<Tickers>,
        }

        struct FadeState;

        impl Default for FadeState {
            fn default() -> FadeState {
                FadeState
            }
        }

        impl crate::framework::StatefulComponent for FadeIn {
            type State = FadeState;

            fn advance(&self, _state: &mut FadeState, frame_time_micros: i64) -> bool {
                self.tickers.tick(frame_time_micros)
            }

            fn build(
                &self,
                _state: &FadeState,
                _handle: StateHandle<FadeState>,
                _context: &mut crate::framework::BuildContext,
            ) -> AnyWidget {
                FadeTransition::new(Rc::clone(&self.controller) as Rc<dyn Animation>, || {
                    leaf(|| SizedBox::new(10.0, 10.0))
                })
                .into_widget()
            }
        }

        let tickers = Rc::new(Tickers::new());
        let controller = AnimationController::new(Duration::from_millis(100));
        controller.with_vsync(&*tickers);
        controller.forward();

        let mut tree = ElementTree::new();
        tree.rebuild(crate::framework::stateful(FadeIn {
            controller: Rc::clone(&controller),
            tickers: Rc::clone(&tickers),
        }));

        // Frames keep being asked for while it runs, and stop when it lands.
        assert!(tree.advance_frame(1_000_000));
        tree.rebuild_dirty();
        assert!(tree.advance_frame(1_050_000));
        tree.rebuild_dirty();
        assert!(tree.advance_frame(1_100_000));
        tree.rebuild_dirty();
        assert_eq!(controller.value(), 1.0);
        assert!(
            !tree.advance_frame(1_116_000),
            "landed, so nothing is asked"
        );
    }

    #[test]
    fn a_controller_stopped_in_the_middle_keeps_the_way_it_was_going() {
        let controller = AnimationController::new(Duration::from_millis(100));
        controller.forward();
        controller.tick(Duration::from_millis(50));
        controller.stop();
        assert_eq!(controller.status(), AnimationStatus::Forward);
        assert!(!controller.is_animating());
    }
}
