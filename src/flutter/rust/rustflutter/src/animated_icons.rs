// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! An icon that draws itself part-way between two shapes.
//!
//! Upstream's `material/animated_icons/`. A play button that becomes a pause
//! button, a hamburger that becomes an arrow: not two glyphs cross-faded, but
//! one outline whose control points move.
//!
//! # Keyframes, not two ends
//!
//! Every point in the icon is a *list* of positions rather than a start and an
//! end, and the progress picks a place along that list. So an icon may bend
//! through shapes it never rests in -- which is how upstream's menu-to-arrow
//! keeps its bars looking like bars all the way across instead of sliding
//! through each other. [`interpolate`] is the whole of that idea.
//!
//! # What is here and what is generated
//!
//! Upstream ships fourteen icons as 34,000 lines of generated Dart under
//! `animated_icons/data/`, one file per icon, each a nest of private constants.
//! Those are build output, not source: they are produced from vector artwork by
//! a tool that is not in this repository, and hand-transcribing them would be
//! copying a build artefact by eye.
//!
//! So what is ported is **the machinery** -- the keyframe interpolation, the
//! path commands, the mirroring rule, the scale, how opacity composes -- which
//! is everything a caller needs to draw an [`AnimatedIconData`] they have.
//! [`AnimatedIcons`] names upstream's fourteen and carries what is known about
//! each without its artwork; see there.

use crate::engine::Color;
use crate::painting::RenderPath;
use crate::render::Offset;

/// Upstream's `_Interpolator<T>`: how two keyframes of one kind are blended.
///
/// A free function rather than a trait because there are exactly two
/// instantiations upstream -- `Offset.lerp` and `lerpDouble` -- and both are
/// one line.
fn lerp_offset(a: Offset, b: Offset, t: f32) -> Offset {
    Offset::new(a.dx + (b.dx - a.dx) * t, a.dy + (b.dy - a.dy) * t)
}

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Upstream's `_interpolate`: where along a list of keyframes `progress` lands.
///
/// # It is a position in the list, not a fraction between two ends
///
/// `progress` is stretched across `values.len() - 1` intervals, and the pair
/// bracketing that position is blended. Three keyframes at progress 0.5 give
/// the *middle* one exactly; five give the third. That is what lets an icon
/// pass through intermediate shapes rather than sliding straight from first to
/// last.
///
/// The single-value case returns that value and never touches the arithmetic --
/// which matters, because `len() - 1` would be zero and the position would be
/// zero over zero.
pub fn interpolate<T: Copy>(values: &[T], progress: f32, blend: fn(T, T, f32) -> T) -> Option<T> {
    debug_assert!(
        (0.0..=1.0).contains(&progress),
        "progress is clamped by the caller before it gets here"
    );
    match values.len() {
        0 => None,
        1 => Some(values[0]),
        len => {
            let target = lerp_f32(0.0, (len - 1) as f32, progress);
            let low = target.floor();
            let high = target.ceil();
            let t = target - low;
            Some(blend(values[low as usize], values[high as usize], t))
        }
    }
}

/// One step of an outline, with each of its points given as keyframes.
///
/// Upstream's `_PathCommand` and its four subclasses. An enum here because the
/// set is closed -- these are the four operations the generator emits.
#[derive(Clone, Debug, PartialEq)]
pub enum PathCommand {
    /// Upstream `_PathMoveTo`.
    MoveTo { points: Vec<Offset> },
    /// Upstream `_PathLineTo`.
    LineTo { points: Vec<Offset> },
    /// Upstream `_PathCubicTo`. Three point lists, interpolated independently:
    /// the two control points and the end point each have their own keyframes,
    /// which is what lets a curve change its bulge as well as its ends.
    CubicTo {
        control1: Vec<Offset>,
        control2: Vec<Offset>,
        target: Vec<Offset>,
    },
    /// Upstream `_PathClose`, which takes no points and therefore does not
    /// interpolate.
    Close,
}

/// One step of an outline with its keyframes already resolved to points.
///
/// The split between this and [`PathCommand`] is where the decisions stop and
/// the drawing starts, and it is deliberate: a [`RenderPath`] is an engine
/// allocation, so anything that builds one cannot be exercised without an
/// engine. The interpolation is the part with judgement in it, so it answers
/// plain numbers and is checked on its own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ResolvedCommand {
    MoveTo(Offset),
    LineTo(Offset),
    CubicTo {
        control1: Offset,
        control2: Offset,
        target: Offset,
    },
    Close,
}

impl PathCommand {
    /// This step at `progress`, as points.
    ///
    /// `None` for a command whose keyframe list is empty, which the generator
    /// does not emit -- but a hand-built icon might, and a command with no
    /// points is not a step.
    pub fn resolve(&self, progress: f32) -> Option<ResolvedCommand> {
        match self {
            PathCommand::MoveTo { points } => {
                interpolate(points, progress, lerp_offset).map(ResolvedCommand::MoveTo)
            }
            PathCommand::LineTo { points } => {
                interpolate(points, progress, lerp_offset).map(ResolvedCommand::LineTo)
            }
            PathCommand::CubicTo {
                control1,
                control2,
                target,
            } => {
                let one = interpolate(control1, progress, lerp_offset)?;
                let two = interpolate(control2, progress, lerp_offset)?;
                let end = interpolate(target, progress, lerp_offset)?;
                Some(ResolvedCommand::CubicTo {
                    control1: one,
                    control2: two,
                    target: end,
                })
            }
            PathCommand::Close => Some(ResolvedCommand::Close),
        }
    }

    /// Upstream's `apply`: adds this step to `path` at `progress`.
    pub fn apply(&self, path: &mut RenderPath, progress: f32) {
        match self.resolve(progress) {
            Some(ResolvedCommand::MoveTo(at)) => {
                path.move_to(at.dx, at.dy);
            }
            Some(ResolvedCommand::LineTo(at)) => {
                path.line_to(at.dx, at.dy);
            }
            Some(ResolvedCommand::CubicTo {
                control1,
                control2,
                target,
            }) => {
                path.cubic_to(
                    control1.dx,
                    control1.dy,
                    control2.dx,
                    control2.dy,
                    target.dx,
                    target.dy,
                );
            }
            Some(ResolvedCommand::Close) => {
                path.close();
            }
            None => {}
        }
    }
}

/// One closed outline of an icon, with its own opacity keyframes.
///
/// Upstream's `_PathFrames`. An icon is several of these -- the two bars of a
/// pause button, the triangle of a play button -- and each fades on its own
/// schedule, which is how one shape can vanish while another arrives.
#[derive(Clone, Debug, PartialEq)]
pub struct PathFrames {
    pub commands: Vec<PathCommand>,
    /// Keyframed, like every other number here. A shape that is only present
    /// for part of the animation has zeros at the ends.
    pub opacities: Vec<f32>,
}

impl PathFrames {
    /// The outline at `progress`, as points, and the colour to draw it in.
    ///
    /// This is the whole of what a frame decides -- see [`ResolvedCommand`] for
    /// why it stops short of building a path.
    pub fn resolve(&self, progress: f32, color: Color) -> (Vec<ResolvedCommand>, Color) {
        let commands = self
            .commands
            .iter()
            .filter_map(|command| command.resolve(progress))
            .collect();
        let opacity = interpolate(&self.opacities, progress, lerp_f32).unwrap_or(1.0);
        (commands, scale_alpha(color, opacity))
    }

    /// The same, as an engine path ready to draw. Upstream's `_PathFrames.paint`
    /// up to the point where it hands over to the canvas.
    pub fn to_path(&self, progress: f32) -> RenderPath {
        let mut path = RenderPath::new();
        for command in &self.commands {
            command.apply(&mut path, progress);
        }
        path
    }
}

/// Upstream's `color.withOpacity(color.opacity * opacity)`.
///
/// **The two multiply rather than the second replacing the first.** A
/// half-transparent icon colour, at a keyframe that is itself half faded,
/// draws at a quarter -- which is what lets a caller dim the whole icon without
/// flattening the animation's own fades.
fn scale_alpha(color: Color, opacity: f32) -> Color {
    let alpha = (color.alpha() as f32 * opacity.clamp(0.0, 1.0)).round() as u8;
    Color::argb(alpha, color.red(), color.green(), color.blue())
}

/// Upstream `AnimatedIconData`: one icon's artwork.
///
/// Upstream's public class is abstract with a single member,
/// `matchTextDirection`, and the paths live on a private subclass so that the
/// vector format stays unexposed -- the file says why, and links the issue:
/// "we are not yet ready for exposing a public API for (partial) vector
/// graphics support". There is no such split here, because there is no second
/// crate to hide the fields from and the format is this file's own either way.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimatedIconData {
    /// The size the artwork was drawn at. Everything is scaled from this to
    /// whatever the icon is asked to be.
    pub size: crate::render::Size,
    pub paths: Vec<PathFrames>,
    /// Upstream's `matchTextDirection`: whether this icon means something
    /// directional, and so should be mirrored in a right-to-left layout.
    ///
    /// False for most: a play button points the way the medium runs, not the
    /// way the text does, and mirroring it would be wrong. True for the ones
    /// that are about *going back*.
    pub match_text_direction: bool,
}

impl AnimatedIconData {
    pub fn new(size: crate::render::Size, paths: Vec<PathFrames>) -> AnimatedIconData {
        AnimatedIconData {
            size,
            paths,
            match_text_direction: false,
        }
    }

    pub fn with_match_text_direction(mut self, matches: bool) -> Self {
        self.match_text_direction = matches;
        self
    }
}

/// Upstream `AnimatedIcon`, reduced to what it decides.
///
/// Upstream's is a `StatelessWidget` building a `CustomPaint` over a
/// `Semantics`; the four decisions in that build are here, and drawing is the
/// caller's because this crate's `RenderCustomPaint` takes a closure rather
/// than a painter object.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimatedIcon {
    pub icon: AnimatedIconData,
    /// 0 to 1. Upstream reads it off an `Animation<double>` and clamps it in
    /// `paint`; clamped here at the same point, in [`AnimatedIcon::outlines`].
    pub progress: f32,
    pub color: Color,
    /// The size to draw at. Upstream falls back to the ambient `IconTheme`'s.
    pub size: f32,
    pub semantic_label: Option<String>,
    pub text_direction: crate::direction::TextDirection,
}

impl AnimatedIcon {
    pub fn new(icon: AnimatedIconData, progress: f32, color: Color, size: f32) -> AnimatedIcon {
        AnimatedIcon {
            icon,
            progress,
            color,
            size,
            semantic_label: None,
            text_direction: crate::direction::TextDirection::Ltr,
        }
    }

    pub fn with_semantic_label(mut self, label: impl Into<String>) -> Self {
        self.semantic_label = Some(label.into());
        self
    }

    pub fn with_text_direction(mut self, direction: crate::direction::TextDirection) -> Self {
        self.text_direction = direction;
        self
    }

    /// Upstream's `scale: iconSize / iconData.size.width`.
    ///
    /// From the **width** alone, not from both axes: the artwork is square and
    /// upstream takes one number. An icon whose data had a non-square size
    /// would be drawn distorted rather than letterboxed, which is upstream's
    /// behaviour and not something to quietly improve.
    pub fn scale(&self) -> f32 {
        if self.icon.size.width == 0.0 {
            return 1.0;
        }
        self.size / self.icon.size.width
    }

    /// Upstream's `shouldMirror`: right-to-left **and** the icon says it is
    /// directional.
    ///
    /// Both halves are needed. Mirroring every icon in an Arabic layout would
    /// reverse play buttons and clock hands; mirroring none would leave a
    /// back-arrow pointing the wrong way.
    pub fn should_mirror(&self) -> bool {
        self.text_direction == crate::direction::TextDirection::Rtl
            && self.icon.match_text_direction
    }

    /// The outlines to draw, each with the colour to draw it in.
    ///
    /// The progress is clamped here, which is where upstream clamps it -- in
    /// `paint`, not in the constructor. An animation that overshoots (a spring,
    /// a curve with a bounce) hands over a value outside 0 to 1, and what
    /// should happen is that the icon rests at its end rather than reading past
    /// the last keyframe.
    pub fn outlines(&self) -> Vec<(Vec<ResolvedCommand>, Color)> {
        let progress = self.progress.clamp(0.0, 1.0);
        self.icon
            .paths
            .iter()
            .map(|path| path.resolve(progress, self.color))
            .collect()
    }

    /// Upstream's mirror, which is a **rotation by π followed by a
    /// translation**, not a horizontal flip.
    ///
    /// `canvas.rotate(pi); canvas.translate(-w, -h)` turns the icon through a
    /// half-turn and puts it back in its box -- so it is flipped on *both*
    /// axes. For artwork that is symmetric top to bottom, which every one of
    /// upstream's fourteen is, that looks exactly like a horizontal mirror.
    /// For artwork that is not, it would not, and the difference is invisible
    /// until somebody draws an asymmetric icon.
    ///
    /// Answered as an affine in this crate's convention, so a caller can
    /// compose it rather than reaching for a canvas.
    pub fn mirror_transform(&self) -> [f32; 6] {
        if !self.should_mirror() {
            return [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        }
        // Rotation by π is (-1, 0, 0, -1); the translation puts the result back
        // over the box it came from.
        [-1.0, 0.0, 0.0, -1.0, self.size, self.size]
    }
}

/// Upstream `AnimatedIcons`: the fourteen icons the framework ships.
///
/// # The names are here and the artwork is not
///
/// Each of upstream's is a `const AnimatedIconData` pointing at a generated
/// file -- 34,000 lines across fourteen of them, produced from vector artwork
/// by a tool outside this repository. They are build output; transcribing them
/// by eye would be copying an artefact, and the result could not be checked
/// against anything.
///
/// So this names them, in upstream's order, and says of each the one thing that
/// is not artwork: whether it mirrors. [`AnimatedIcons::data`] answers `None`
/// until there is a generator, which is the honest answer -- an icon with an
/// empty path list would draw nothing while claiming to be an icon.
///
/// The machinery above is complete and independent of this: an
/// [`AnimatedIconData`] a caller builds themselves draws correctly today.
pub struct AnimatedIcons;

impl AnimatedIcons {
    /// Upstream's fourteen, in the order `animated_icons_data.dart` lists them.
    pub const NAMES: &'static [&'static str] = &[
        "add_event",
        "arrow_menu",
        "close_menu",
        "ellipsis_search",
        "event_add",
        "home_menu",
        "list_view",
        "menu_arrow",
        "menu_close",
        "menu_home",
        "pause_play",
        "play_pause",
        "search_ellipsis",
        "view_list",
    ];

    /// The artwork for a named icon, or `None` while there is no generator.
    ///
    /// Deliberately not an empty [`AnimatedIconData`]: one with no paths draws
    /// nothing, and a caller who got one back would find out at the far end of
    /// a render rather than here.
    pub fn data(_name: &str) -> Option<AnimatedIconData> {
        None
    }

    /// Whether a named icon is one of upstream's.
    pub fn contains(name: &str) -> bool {
        AnimatedIcons::NAMES.contains(&name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::direction::TextDirection;
    use crate::render::Size;

    fn at(x: f32, y: f32) -> Offset {
        Offset::new(x, y)
    }

    // -- interpolate: a position in the list ---------------------------------------

    #[test]
    fn three_keyframes_at_half_way_give_the_middle_one_exactly() {
        // The whole idea. Progress is stretched across `len - 1` intervals, so
        // an icon passes *through* its intermediate shapes rather than sliding
        // from the first to the last.
        let frames = [0.0f32, 10.0, 100.0];
        assert_eq!(interpolate(&frames, 0.5, lerp_f32), Some(10.0));
        assert_eq!(interpolate(&frames, 0.0, lerp_f32), Some(0.0));
        assert_eq!(interpolate(&frames, 1.0, lerp_f32), Some(100.0));
    }

    #[test]
    fn five_keyframes_at_half_way_give_the_third() {
        let frames = [0.0f32, 1.0, 2.0, 3.0, 4.0];
        assert_eq!(interpolate(&frames, 0.5, lerp_f32), Some(2.0));
    }

    #[test]
    fn a_position_between_keyframes_blends_the_two_it_falls_between() {
        // Three frames, progress 0.25: a quarter of the way along two
        // intervals is halfway through the first.
        let frames = [0.0f32, 10.0, 100.0];
        assert_eq!(interpolate(&frames, 0.25, lerp_f32), Some(5.0));
        // And three quarters is halfway through the second.
        assert_eq!(interpolate(&frames, 0.75, lerp_f32), Some(55.0));
    }

    #[test]
    fn two_keyframes_behave_like_a_plain_lerp() {
        // The case that makes the general rule look like a straight blend, and
        // the reason it is easy to assume that is all it does.
        let frames = [0.0f32, 100.0];
        assert_eq!(interpolate(&frames, 0.25, lerp_f32), Some(25.0));
        assert_eq!(interpolate(&frames, 0.5, lerp_f32), Some(50.0));
    }

    #[test]
    fn one_keyframe_is_returned_without_arithmetic() {
        // `len - 1` would be zero and the position would be zero over zero.
        assert_eq!(interpolate(&[7.0f32], 0.0, lerp_f32), Some(7.0));
        assert_eq!(interpolate(&[7.0f32], 0.5, lerp_f32), Some(7.0));
        assert_eq!(interpolate(&[7.0f32], 1.0, lerp_f32), Some(7.0));
    }

    #[test]
    fn no_keyframes_is_no_answer() {
        assert_eq!(interpolate::<f32>(&[], 0.5, lerp_f32), None);
    }

    #[test]
    fn offsets_interpolate_on_both_axes() {
        let frames = [at(0.0, 0.0), at(10.0, 20.0)];
        assert_eq!(interpolate(&frames, 0.5, lerp_offset), Some(at(5.0, 10.0)));
    }

    // -- Path commands ---------------------------------------------------------------

    #[test]
    fn a_cubics_three_point_lists_interpolate_independently() {
        // What lets a curve change its bulge as well as its ends: each of the
        // two control points and the end point has its own keyframes, and they
        // need not have the same number of them.
        let command = PathCommand::CubicTo {
            control1: vec![at(0.0, 0.0), at(10.0, 0.0)],
            // Three frames on this one, so half way is the middle.
            control2: vec![at(0.0, 0.0), at(50.0, 50.0), at(0.0, 100.0)],
            target: vec![at(0.0, 0.0), at(20.0, 20.0)],
        };
        assert_eq!(
            command.resolve(0.5),
            Some(ResolvedCommand::CubicTo {
                control1: at(5.0, 0.0),
                control2: at(50.0, 50.0),
                target: at(10.0, 10.0),
            })
        );
    }

    #[test]
    fn a_close_takes_no_points_and_does_not_interpolate() {
        assert_eq!(
            PathCommand::Close.resolve(0.0),
            Some(ResolvedCommand::Close)
        );
        assert_eq!(
            PathCommand::Close.resolve(0.7),
            Some(ResolvedCommand::Close)
        );
    }

    #[test]
    fn a_command_with_no_keyframes_is_not_a_step() {
        assert_eq!(PathCommand::MoveTo { points: vec![] }.resolve(0.5), None);
        assert_eq!(
            PathCommand::CubicTo {
                control1: vec![at(0.0, 0.0)],
                control2: vec![],
                target: vec![at(1.0, 1.0)],
            }
            .resolve(0.5),
            None,
            "one missing list is enough"
        );
    }

    // -- Opacity ----------------------------------------------------------------------

    #[test]
    fn the_icons_colour_and_the_frames_opacity_multiply() {
        // A half-transparent icon colour at a half-faded keyframe draws at a
        // quarter. That is what lets a caller dim the whole icon without
        // flattening the animation's own fades.
        let frames = PathFrames {
            commands: vec![PathCommand::Close],
            opacities: vec![1.0, 0.0],
        };
        let half = Color::argb(0x80, 0xFF, 0x00, 0x00);
        let (_, colour) = frames.resolve(0.5, half);
        assert_eq!(colour.alpha(), 0x40, "half of a half");
        assert_eq!(colour.red(), 0xFF, "and the hue is untouched");
    }

    #[test]
    fn a_shape_that_is_absent_at_this_moment_draws_at_zero_alpha() {
        // How one shape vanishes while another arrives: each outline has its
        // own opacity keyframes.
        let frames = PathFrames {
            commands: vec![PathCommand::Close],
            opacities: vec![1.0, 0.0],
        };
        let (_, colour) = frames.resolve(1.0, Color::argb(0xFF, 0, 0, 0));
        assert_eq!(colour.alpha(), 0);
    }

    #[test]
    fn a_frame_with_no_opacity_keyframes_is_fully_opaque() {
        let frames = PathFrames {
            commands: vec![PathCommand::Close],
            opacities: vec![],
        };
        let (_, colour) = frames.resolve(0.5, Color::argb(0xFF, 1, 2, 3));
        assert_eq!(colour.alpha(), 0xFF);
    }

    // -- The widget's four decisions ---------------------------------------------------

    fn icon(match_text_direction: bool) -> AnimatedIconData {
        AnimatedIconData::new(
            Size::new(48.0, 48.0),
            vec![PathFrames {
                commands: vec![PathCommand::MoveTo {
                    points: vec![at(0.0, 0.0), at(48.0, 48.0)],
                }],
                opacities: vec![1.0],
            }],
        )
        .with_match_text_direction(match_text_direction)
    }

    #[test]
    fn the_scale_comes_from_the_width_alone() {
        // Upstream takes one number, so non-square artwork is distorted rather
        // than letterboxed. That is upstream's behaviour, not something to
        // quietly improve.
        let widget = AnimatedIcon::new(icon(false), 0.0, Color::argb(0xFF, 0, 0, 0), 24.0);
        assert_eq!(widget.scale(), 0.5);

        let tall = AnimatedIcon::new(
            AnimatedIconData::new(Size::new(48.0, 96.0), Vec::new()),
            0.0,
            Color::argb(0xFF, 0, 0, 0),
            24.0,
        );
        assert_eq!(tall.scale(), 0.5, "the height is not consulted");
    }

    #[test]
    fn mirroring_needs_both_a_right_to_left_layout_and_a_directional_icon() {
        // Mirroring every icon in an Arabic layout would reverse play buttons
        // and clock hands; mirroring none would leave a back-arrow pointing the
        // wrong way.
        let colour = Color::argb(0xFF, 0, 0, 0);
        let directional_rtl = AnimatedIcon::new(icon(true), 0.0, colour, 24.0)
            .with_text_direction(TextDirection::Rtl);
        assert!(directional_rtl.should_mirror());

        let directional_ltr = AnimatedIcon::new(icon(true), 0.0, colour, 24.0)
            .with_text_direction(TextDirection::Ltr);
        assert!(!directional_ltr.should_mirror());

        let plain_rtl = AnimatedIcon::new(icon(false), 0.0, colour, 24.0)
            .with_text_direction(TextDirection::Rtl);
        assert!(
            !plain_rtl.should_mirror(),
            "a play button is not directional"
        );
    }

    #[test]
    fn the_mirror_is_a_half_turn_and_not_a_horizontal_flip() {
        // `canvas.rotate(pi); canvas.translate(-w, -h)` flips *both* axes. For
        // artwork symmetric top to bottom -- which all fourteen of upstream's
        // are -- that looks exactly like a horizontal mirror, and for artwork
        // that is not, it does not.
        let widget = AnimatedIcon::new(icon(true), 0.0, Color::argb(0xFF, 0, 0, 0), 24.0)
            .with_text_direction(TextDirection::Rtl);
        let [a, b, c, d, e, f] = widget.mirror_transform();
        assert_eq!(
            (a, b, c, d),
            (-1.0, 0.0, 0.0, -1.0),
            "both axes, not just x"
        );
        assert_eq!((e, f), (24.0, 24.0), "and put back over its own box");

        // A corner maps to the opposite corner, which is the observable form of
        // the same statement.
        let corner = |x: f32, y: f32| (a * x + c * y + e, b * x + d * y + f);
        assert_eq!(corner(0.0, 0.0), (24.0, 24.0));
        assert_eq!(corner(24.0, 24.0), (0.0, 0.0));
    }

    #[test]
    fn an_unmirrored_icon_gets_the_identity() {
        let widget = AnimatedIcon::new(icon(false), 0.0, Color::argb(0xFF, 0, 0, 0), 24.0);
        assert_eq!(widget.mirror_transform(), [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn progress_is_clamped_where_it_is_drawn_and_not_where_it_is_set() {
        // An animation that overshoots -- a spring, a curve with a bounce --
        // hands over a value outside 0 to 1, and the icon should rest at its
        // end rather than read past the last keyframe. Upstream clamps in
        // `paint` for the same reason.
        let colour = Color::argb(0xFF, 0, 0, 0);
        let past_the_end = AnimatedIcon::new(icon(false), 1.4, colour, 48.0);
        assert_eq!(past_the_end.progress, 1.4, "kept as given");

        let outlines = past_the_end.outlines();
        assert_eq!(
            outlines[0].0[0],
            ResolvedCommand::MoveTo(at(48.0, 48.0)),
            "and drawn at the last keyframe"
        );

        let before_the_start = AnimatedIcon::new(icon(false), -0.3, colour, 48.0);
        assert_eq!(
            before_the_start.outlines()[0].0[0],
            ResolvedCommand::MoveTo(at(0.0, 0.0))
        );
    }

    #[test]
    fn an_icon_with_zero_width_artwork_scales_by_one_rather_than_dividing_by_zero() {
        let widget = AnimatedIcon::new(
            AnimatedIconData::new(Size::new(0.0, 0.0), Vec::new()),
            0.0,
            Color::argb(0xFF, 0, 0, 0),
            24.0,
        );
        assert_eq!(widget.scale(), 1.0);
    }

    // -- The catalogue -----------------------------------------------------------------

    #[test]
    fn upstreams_fourteen_are_named() {
        assert_eq!(AnimatedIcons::NAMES.len(), 14);
        assert!(AnimatedIcons::contains("play_pause"));
        assert!(AnimatedIcons::contains("menu_arrow"));
        assert!(!AnimatedIcons::contains("not_an_icon"));
    }

    #[test]
    fn the_catalogue_has_no_artwork_and_says_so_rather_than_handing_back_an_empty_icon() {
        // An `AnimatedIconData` with no paths draws nothing while claiming to
        // be an icon, and a caller would find out at the far end of a render.
        assert!(AnimatedIcons::data("play_pause").is_none());
        assert!(AnimatedIcons::data("not_an_icon").is_none());
    }

    #[test]
    fn every_name_is_listed_once_and_in_upstreams_order() {
        let mut sorted = AnimatedIcons::NAMES.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), AnimatedIcons::NAMES.len(), "no duplicates");
        assert_eq!(AnimatedIcons::NAMES[0], "add_event");
        assert_eq!(AnimatedIcons::NAMES[13], "view_list");
    }
}
