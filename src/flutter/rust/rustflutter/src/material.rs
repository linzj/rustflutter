// Copyright 2013 The Flutter Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Upstream `material/material.dart`: the surface everything Material is
//! drawn on, and the list of ink it holds.
//!
//! A [`Material`] is three things at once, and they are worth separating
//! because only the third is unusual:
//!
//! 1. **A shape with a colour**, rounded or circular by its
//!    [`MaterialType`].
//! 2. **An elevation**, which is a shadow in a light theme and a lightening
//!    of the surface itself in a dark one -- see [`crate::elevation_overlay`].
//! 3. **A place ink is painted.** Every splash, ripple and highlight in the
//!    application is painted by the nearest `Material` above the control that
//!    made it, not by the control. That is why upstream's `InkWell` asserts
//!    it has one, and why a button on no material draws no splash however
//!    correct the button is.
//!
//! The third is the one this file exists for. A control that painted its own
//! splash would paint it *inside* itself: clipped by its own bounds, under
//! anything it draws, and gone the moment it rebuilt. Handing the ink to the
//! surface underneath puts it above the surface's colour, below the content,
//! and outlives whatever started it -- which is exactly how ink behaves.
//!
//! # What is not ported
//!
//! * **`Material.of(context)`.** Upstream's is an ancestor lookup for a
//!   render object (`LookupBoundary.findAncestorRenderObjectOfType`), and
//!   this crate's [`BuildContext`] has no ancestor walk -- the same gap
//!   `LookupBoundary` itself is waiting on. So the controller is passed to
//!   whoever needs it rather than found, and [`crate::ink_well::InkResponse`]
//!   keeps its own features instead of handing them up.
//! * **The animated interior.** Upstream's `_MaterialInterior` walks the
//!   shape, the elevation and the colour to their new values over
//!   `kThemeChangeDuration`. This crate's implicit-animation helper is over
//!   `Lerp`, which requires `Copy`, and a `ShapeBorder` is not one -- the
//!   same boundary [`crate::drawer::DrawerHeader`] records. [`ShapeBorderTween`]
//!   is here, so the arithmetic exists; what is missing is the driver.
//! * **`AnimatedDefaultTextStyle`**, which upstream wraps the child in so a
//!   material carries a text style down. This crate has no ambient text
//!   style; components build their text from [`crate::components::Theme`].

use crate::components::theme_of;
use crate::engine::Color;
use crate::framework::{AnyWidget, BuildContext, Component, leaf, single};
use crate::ink::{InkDecoration, InteractiveInkFeature};
use crate::painting::ClipBehavior;

/// Upstream `MaterialType`: the five shapes a material comes in.
///
/// Not a style: the type decides the *shape* and, for two of them, where the
/// colour comes from. `Transparency` is the odd one -- a material with no
/// colour at all, which exists so that ink has somewhere to land on a surface
/// that is already painted by something else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MaterialType {
    /// The default: a rectangle in the theme's canvas colour.
    #[default]
    Canvas,
    /// A rectangle with 2-pixel corners, in the theme's card colour.
    Card,
    Circle,
    /// A rectangle with 2-pixel corners and no colour of its own.
    Button,
    /// No shape and no colour -- ink and nothing else.
    Transparency,
}

impl MaterialType {
    /// Upstream's `kMaterialEdges`, as a method rather than a map.
    ///
    /// `None` means "no rounding", which is not the same as a radius of zero
    /// for [`MaterialType::Circle`]: a circle's shape is its own and a border
    /// radius would contradict it. Upstream asserts exactly that in
    /// `Material`'s constructor.
    pub fn border_radius(self) -> Option<f32> {
        match self {
            MaterialType::Card | MaterialType::Button => Some(2.0),
            MaterialType::Canvas | MaterialType::Circle | MaterialType::Transparency => None,
        }
    }

    /// Whether a material of this type takes a press that lands on it but on
    /// none of its children. Upstream's `absorbHitTest`, which is
    /// `type != transparency`.
    ///
    /// A painted surface is a thing; a transparent one is a hole. Tapping the
    /// gap between two buttons on a card should not fall through to whatever
    /// is behind the card, and tapping through a transparency should.
    pub fn absorbs_hit_test(self) -> bool {
        self != MaterialType::Transparency
    }
}

/// Upstream `InkFeature`: anything a [`Material`] paints into itself.
///
/// The two concrete kinds are the interactive ones -- splashes, ripples,
/// highlights -- and the decoration an `Ink` widget lays down. An enum rather
/// than a trait for the reason [`crate::ink::InkFeatureKind`] is one: the set
/// is closed, and a `match` that must answer for every variant is what keeps a
/// third from being half-added.
#[derive(Clone, Debug, PartialEq)]
pub enum InkFeature {
    Interactive(InteractiveInkFeature),
    Decoration(InkDecoration),
}

impl InkFeature {
    /// Whether the feature still has anything to draw.
    ///
    /// A decoration is always alive: it is not an animation, and it goes away
    /// when the widget that put it there does. Upstream says the same by
    /// having `InkDecoration` never call `dispose` on its own.
    pub fn alive(&self) -> bool {
        match self {
            InkFeature::Interactive(feature) => feature.alive(),
            InkFeature::Decoration(_) => true,
        }
    }

    pub fn advance(&mut self, now_micros: i64) {
        if let InkFeature::Interactive(feature) = self {
            feature.advance(now_micros);
        }
    }
}

/// Upstream `MaterialInkController`: what a control adds its ink to.
///
/// Upstream this is an interface the material's render object implements, so
/// that a control can find it by walking up and hand it a feature without
/// knowing what it is. Here it is the thing itself -- a list and a colour --
/// because there is no ancestor walk to find an interface through (see the
/// module docs).
///
/// The order in the list is the order they are painted, and it is the order
/// they were added: a highlight raised by a press is painted over the splash
/// that same press started, because it was added after it.
#[derive(Debug, Default)]
pub struct MaterialInkController {
    /// Upstream's `color`, which the controller carries but does *not* paint
    /// -- upstream's comment says so outright ("the actual painting of this
    /// color is done by a Container"). It is here so a feature can ask what
    /// it is landing on.
    pub color: Option<Color>,
    features: Vec<InkFeature>,
    needs_paint: bool,
}

impl MaterialInkController {
    pub fn new(color: Option<Color>) -> MaterialInkController {
        MaterialInkController {
            color,
            features: Vec::new(),
            needs_paint: false,
        }
    }

    /// Upstream's `addInkFeature`, which marks the material for repaint --
    /// the new feature has nothing on screen yet, so the frame that added it
    /// is a frame that changed.
    pub fn add_ink_feature(&mut self, feature: InkFeature) {
        self.features.push(feature);
        self.mark_needs_paint();
    }

    pub fn mark_needs_paint(&mut self) {
        self.needs_paint = true;
    }

    /// Whether a repaint is owed, clearing the flag. The frame asks once.
    pub fn take_needs_paint(&mut self) -> bool {
        std::mem::replace(&mut self.needs_paint, false)
    }

    pub fn features(&self) -> &[InkFeature] {
        &self.features
    }

    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// Moves the clock and drops what has finished -- upstream's per-feature
    /// `dispose`, which calls back into `_removeFeature`.
    ///
    /// A layout change marks a repaint without dropping anything, which is
    /// upstream's `_didChangeLayout`: a splash measured against a box that
    /// resized is in the wrong place, not finished.
    pub fn advance(&mut self, now_micros: i64) -> bool {
        if self.features.is_empty() {
            return false;
        }
        for feature in self.features.iter_mut() {
            feature.advance(now_micros);
        }
        let before = self.features.len();
        self.features.retain(|feature| feature.alive());
        if self.features.len() != before {
            self.mark_needs_paint();
        }
        true
    }

    /// Upstream's `_didChangeLayout`: a repaint is owed only if there is ink
    /// to reposition. A material with nothing in it does not care that it
    /// resized.
    pub fn did_change_layout(&mut self) {
        if !self.features.is_empty() {
            self.mark_needs_paint();
        }
    }
}

/// Upstream `Material`: the surface Material Design is drawn on.
///
/// See the module docs for what it is for. The fields are upstream's, less
/// the ones whose driver this crate does not have (see there).
pub struct Material {
    child: std::cell::RefCell<Option<AnyWidget>>,
    pub material_type: MaterialType,
    pub elevation: f32,
    pub color: Option<Color>,
    pub shadow_color: Option<Color>,
    pub surface_tint_color: Option<Color>,
    pub border_radius: Option<f32>,
    pub shape: Option<crate::borders::ShapeBorder>,
    /// Upstream's `borderOnForeground`: whether the shape's outline is drawn
    /// over the child or under it. Over, by default, so an outline is not
    /// hidden by a child that fills its surface.
    pub border_on_foreground: bool,
    pub clip_behavior: ClipBehavior,
}

impl Material {
    /// Upstream's `Material.defaultSplashRadius`, re-exported at the name it
    /// has upstream. The value lives in [`crate::ink`], beside the splashes
    /// that read it.
    pub const DEFAULT_SPLASH_RADIUS: f32 = crate::ink::DEFAULT_SPLASH_RADIUS;

    pub fn new(child: AnyWidget) -> Material {
        Material {
            child: std::cell::RefCell::new(Some(child)),
            material_type: MaterialType::Canvas,
            elevation: 0.0,
            color: None,
            shadow_color: None,
            surface_tint_color: None,
            border_radius: None,
            shape: None,
            border_on_foreground: true,
            clip_behavior: ClipBehavior::None,
        }
    }

    pub fn with_type(mut self, material_type: MaterialType) -> Self {
        self.material_type = material_type;
        self
    }

    pub fn with_elevation(mut self, elevation: f32) -> Self {
        self.elevation = elevation;
        self
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_shadow_color(mut self, color: Color) -> Self {
        self.shadow_color = Some(color);
        self
    }

    pub fn with_surface_tint_color(mut self, color: Color) -> Self {
        self.surface_tint_color = Some(color);
        self
    }

    /// Upstream asserts that a shape and a border radius are never both
    /// given, and that neither is given for a circle. Both are debug
    /// assertions there; here the shape simply wins, and
    /// [`Material::debug_assert_valid`] is where the assertions live.
    pub fn with_border_radius(mut self, radius: f32) -> Self {
        self.border_radius = Some(radius);
        self
    }

    pub fn with_shape(mut self, shape: crate::borders::ShapeBorder) -> Self {
        self.shape = Some(shape);
        self
    }

    pub fn with_border_on_foreground(mut self, on_foreground: bool) -> Self {
        self.border_on_foreground = on_foreground;
        self
    }

    pub fn with_clip_behavior(mut self, clip_behavior: ClipBehavior) -> Self {
        self.clip_behavior = clip_behavior;
        self
    }

    /// Upstream's three constructor assertions, which say the same thing
    /// twice over: a material's shape has exactly one source.
    pub fn debug_assert_valid(&self) {
        debug_assert!(
            !(self.shape.is_some() && self.border_radius.is_some()),
            "a shape and a border radius are two answers to one question"
        );
        debug_assert!(
            !(self.material_type == MaterialType::Circle
                && (self.shape.is_some() || self.border_radius.is_some())),
            "a circle's shape is its own"
        );
        debug_assert!(self.elevation >= 0.0, "elevation is a height, not a depth");
    }

    /// The radius this material actually rounds by: its own if it was given
    /// one, otherwise its type's.
    pub fn effective_border_radius(&self) -> Option<f32> {
        self.border_radius
            .or_else(|| self.material_type.border_radius())
    }

    /// The colour the surface is painted, with elevation already applied.
    ///
    /// The two steps are upstream's: the *background* comes from the widget,
    /// then the type, then the theme -- and only `canvas` and `card` have a
    /// theme colour at all, which is why the other three answer `None` and a
    /// non-transparent material without a colour is an error upstream asserts
    /// on.
    ///
    /// The two theme colours are `ThemeData`'s own `canvasColor` and
    /// `cardColor`, not the component theme's surfaces. This read the latter,
    /// so `ThemeData::canvas_color` reached nothing -- and `canvas` is the
    /// default type, so it is the colour most materials would have taken. Then M3's surface tint is laid over it, which is what an elevated
    /// surface looks like in a dark theme where a shadow would be invisible.
    pub fn effective_color(&self, theme: &crate::theme::ThemeData) -> Option<Color> {
        let background = self.color.or(match self.material_type {
            MaterialType::Canvas => Some(theme.canvas_color),
            MaterialType::Card => Some(theme.card_color),
            MaterialType::Button | MaterialType::Circle | MaterialType::Transparency => None,
        })?;
        Some(
            crate::elevation_overlay::ElevationOverlay::apply_surface_tint(
                background,
                self.surface_tint_color,
                self.elevation,
            ),
        )
    }
}

impl Component for Material {
    fn build(&self, context: &mut BuildContext) -> AnyWidget {
        self.debug_assert_valid();
        let theme = theme_of(context);
        let material = crate::theme::ThemeData::of(context);
        let child = self
            .child
            .borrow_mut()
            .take()
            .unwrap_or_else(|| leaf(|| crate::widgets::Empty));
        let colour = self.effective_color(&material);
        // The renderer's shadow table is indexed by whole elevation steps, as
        // `Card` documents; a material asked for half a step gets the nearer
        // one rather than none.
        let elevation = self.elevation.round().max(0.0) as u32;
        let radius = self.effective_border_radius();
        let circle = self.material_type == MaterialType::Circle;

        single(child, move |inner| {
            let mut container = crate::widgets::Container::new().with_child(inner);
            if let Some(colour) = colour {
                container = container.with_color(colour);
            }
            if elevation > 0 {
                container = container.with_elevation(elevation);
            }
            // A circle rounds by half its shorter side, which the renderer
            // reaches with a radius larger than the box: it clamps. Anything
            // else rounds by what it was given.
            match (circle, radius) {
                (true, _) => container = container.with_corner_radius(f32::MAX / 4.0),
                (false, Some(radius)) => container = container.with_corner_radius(radius),
                (false, None) => {}
            }
            Box::new(container)
        })
    }
}

/// Upstream `ShapeBorderTween`: one shape walked into another.
///
/// The whole of it is [`crate::borders::ShapeBorder::lerp`], which is where
/// the interesting part already is -- a rounded rectangle becoming a circle,
/// a border becoming nothing. What the tween adds is the two ends, and two
/// things about them are worth naming because both look like bugs:
///
/// * **A null end is a shape scaled to nothing, not the absence of one.**
///   Upstream's `lerp` reaches `a.lerpTo(null, t)`, which is `a.scale(1 - t)`
///   -- so lerping a circle to null ends at a circle border of zero width,
///   and the result is `Some` at every `t` including 1.
/// * **The endpoints are not the classes they were given.** Lerping a
///   `CircleBorder` to a `StadiumBorder` answers a transition shape at *every*
///   `t`, including 0 and 1, parameterised to look like the end it is at. The
///   guarantee is the geometry, not the type -- which is why a caller that
///   `match`es on the variant must not assume it gets its own back.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ShapeBorderTween {
    pub begin: Option<crate::borders::ShapeBorder>,
    pub end: Option<crate::borders::ShapeBorder>,
}

impl ShapeBorderTween {
    pub fn new(
        begin: Option<crate::borders::ShapeBorder>,
        end: Option<crate::borders::ShapeBorder>,
    ) -> ShapeBorderTween {
        ShapeBorderTween { begin, end }
    }

    /// Upstream's `lerp`.
    pub fn lerp(&self, t: f32) -> Option<crate::borders::ShapeBorder> {
        crate::borders::ShapeBorder::lerp(self.begin.clone(), self.end.clone(), t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::Theme;
    use crate::ink::{InkHighlight, InkRipple, InkSettlement, InteractiveInkFeatureFactory};
    use crate::render::{Offset, Size};

    const INK: Color = Color::argb(0x40, 0x33, 0x66, 0x99);

    fn splash(at_micros: i64) -> InkFeature {
        InkFeature::Interactive(InteractiveInkFeatureFactory::Ripple.create(
            Size::new(100.0, 100.0),
            Offset::new(50.0, 50.0),
            INK,
            true,
            at_micros,
        ))
    }

    #[test]
    fn only_two_of_the_five_types_round_their_corners() {
        // Upstream's `kMaterialEdges`. The `None` for a circle is not a
        // radius of zero: a circle's shape is its own, and upstream asserts
        // that a caller who gives it one is confused.
        assert_eq!(MaterialType::Card.border_radius(), Some(2.0));
        assert_eq!(MaterialType::Button.border_radius(), Some(2.0));
        assert_eq!(MaterialType::Canvas.border_radius(), None);
        assert_eq!(MaterialType::Circle.border_radius(), None);
        assert_eq!(MaterialType::Transparency.border_radius(), None);
    }

    #[test]
    fn a_transparent_material_lets_a_press_through_and_the_others_do_not() {
        // A painted surface is a thing; a transparent one is a hole. Tapping
        // the gap between two buttons on a card must not reach what is behind
        // the card.
        assert!(!MaterialType::Transparency.absorbs_hit_test());
        for solid in [
            MaterialType::Canvas,
            MaterialType::Card,
            MaterialType::Circle,
            MaterialType::Button,
        ] {
            assert!(solid.absorbs_hit_test(), "{solid:?}");
        }
    }

    #[test]
    fn only_canvas_and_card_have_a_colour_of_their_own() {
        // Upstream's switch answers null for the other three, and then
        // asserts that a non-transparent material without a colour is an
        // error -- which is to say a button-type material must be told.
        let theme = crate::theme::ThemeData::dark();
        assert_eq!(
            Material::new(leaf(|| crate::widgets::Empty))
                .with_type(MaterialType::Canvas)
                .effective_color(&theme),
            Some(theme.canvas_color)
        );
        assert_eq!(
            Material::new(leaf(|| crate::widgets::Empty))
                .with_type(MaterialType::Card)
                .effective_color(&theme),
            Some(theme.card_color)
        );
        for uncoloured in [
            MaterialType::Button,
            MaterialType::Circle,
            MaterialType::Transparency,
        ] {
            assert_eq!(
                Material::new(leaf(|| crate::widgets::Empty))
                    .with_type(uncoloured)
                    .effective_color(&theme),
                None,
                "{uncoloured:?}"
            );
        }
        // And a colour given outright wins over all of it.
        assert_eq!(
            Material::new(leaf(|| crate::widgets::Empty))
                .with_type(MaterialType::Button)
                .with_color(Color::WHITE)
                .effective_color(&theme),
            Some(Color::WHITE)
        );
    }

    #[test]
    fn an_elevated_material_is_tinted_rather_than_left_flat() {
        // What an elevated surface looks like in a dark theme, where a shadow
        // would be invisible. See `elevation_overlay`.
        let theme = crate::theme::ThemeData::dark();
        let flat = Material::new(leaf(|| crate::widgets::Empty))
            .with_color(Color::rgb(0x20, 0x20, 0x20))
            .with_surface_tint_color(Color::rgb(0x80, 0x80, 0xFF));
        let raised = Material::new(leaf(|| crate::widgets::Empty))
            .with_color(Color::rgb(0x20, 0x20, 0x20))
            .with_surface_tint_color(Color::rgb(0x80, 0x80, 0xFF))
            .with_elevation(6.0);
        assert_ne!(flat.effective_color(&theme), raised.effective_color(&theme));
    }

    #[test]
    fn a_materials_own_radius_wins_over_its_types() {
        let card = Material::new(leaf(|| crate::widgets::Empty)).with_type(MaterialType::Card);
        assert_eq!(card.effective_border_radius(), Some(2.0));
        assert_eq!(
            Material::new(leaf(|| crate::widgets::Empty))
                .with_type(MaterialType::Card)
                .with_border_radius(16.0)
                .effective_border_radius(),
            Some(16.0)
        );
    }

    #[test]
    fn adding_ink_marks_the_material_for_repaint() {
        // The new feature has nothing on screen yet, so the frame that added
        // it is a frame that changed.
        let mut controller = MaterialInkController::new(Some(Color::WHITE));
        assert!(!controller.take_needs_paint(), "nothing has happened yet");
        controller.add_ink_feature(splash(0));
        assert!(controller.take_needs_paint());
        assert!(!controller.take_needs_paint(), "the frame asks once");
    }

    #[test]
    fn features_are_painted_in_the_order_they_were_added() {
        // Which is why a highlight raised by a press sits over the splash
        // that same press started: it was added after it.
        let mut controller = MaterialInkController::default();
        controller.add_ink_feature(splash(0));
        controller.add_ink_feature(InkFeature::Interactive(InteractiveInkFeature::new(
            crate::ink::InkFeatureKind::Highlight(InkHighlight::new()),
            INK,
            0,
        )));
        assert!(matches!(
            controller.features(),
            [
                InkFeature::Interactive(first),
                InkFeature::Interactive(second)
            ] if matches!(first.kind, crate::ink::InkFeatureKind::Ripple(_))
                && matches!(second.kind, crate::ink::InkFeatureKind::Highlight(_))
        ));
    }

    #[test]
    fn a_finished_splash_is_dropped_and_the_drop_is_a_repaint() {
        let mut controller = MaterialInkController::default();
        controller.add_ink_feature(splash(0));
        controller.take_needs_paint();

        assert!(controller.advance(1_000), "there is ink to advance");
        assert!(!controller.take_needs_paint(), "nothing finished yet");

        if let InkFeature::Interactive(feature) = &mut controller.features[0] {
            feature.confirm(1_000);
        }
        controller.advance(1_000 + InkRipple::FADE_OUT_MICROS);
        assert!(controller.is_empty());
        assert!(
            controller.take_needs_paint(),
            "the last ring has to be erased"
        );
        assert!(!controller.advance(9_999_999), "and then there is nothing");
    }

    #[test]
    fn a_decoration_is_not_an_animation_and_never_finishes() {
        // It goes away when the widget that put it there does, not when a
        // clock runs out -- upstream's `InkDecoration` never disposes itself.
        let mut controller = MaterialInkController::default();
        controller.add_ink_feature(InkFeature::Decoration(InkDecoration::default()));
        controller.advance(10_000_000);
        assert_eq!(controller.features().len(), 1);
    }

    #[test]
    fn a_resize_repaints_the_ink_but_does_not_end_it() {
        // Upstream's `_didChangeLayout`: a splash measured against a box that
        // resized is in the wrong place, not finished.
        let mut controller = MaterialInkController::default();
        controller.did_change_layout();
        assert!(
            !controller.take_needs_paint(),
            "an empty material does not care that it resized"
        );

        controller.add_ink_feature(splash(0));
        controller.take_needs_paint();
        controller.did_change_layout();
        assert!(controller.take_needs_paint());
        assert_eq!(controller.features().len(), 1, "still there");
    }

    #[test]
    fn the_controller_carries_a_colour_it_does_not_paint() {
        // Upstream says so outright: "the actual painting of this color is
        // done by a Container in the widget's build method". It is here so a
        // feature can ask what it is landing on.
        let controller = MaterialInkController::new(Some(Color::WHITE));
        assert_eq!(controller.color, Some(Color::WHITE));
        assert!(MaterialInkController::default().color.is_none());
    }

    #[test]
    fn a_null_end_is_a_shape_scaled_to_nothing_rather_than_the_absence_of_one() {
        // Upstream's `lerp` reaches `a.lerpTo(null, t)`, which is
        // `a.scale(1 - t)`. So the answer is `Some` at every t including 1 --
        // a border of zero width, which draws nothing but is still a shape
        // and still clips. A caller expecting `None` at the end gets a
        // surprise, so it is pinned here.
        let side = crate::borders::BorderSide {
            width: 4.0,
            ..crate::borders::BorderSide::default()
        };
        let circle =
            crate::borders::ShapeBorder::Circle(crate::borders::CircleBorder::new(side, 0.0));
        let tween = ShapeBorderTween::new(Some(circle.clone()), None);
        assert_eq!(tween.lerp(0.0), Some(circle));
        let end = tween.lerp(1.0).expect("still a shape");
        assert_eq!(
            end.outlined_side().map(|side| side.width),
            Some(0.0),
            "scaled to nothing"
        );
    }

    #[test]
    fn a_tween_between_two_shape_classes_is_the_transition_shape_at_both_ends() {
        // Not the classes it was given: lerping a circle to a stadium answers
        // a `StadiumToCircle` at *every* t, parameterised to look like the end
        // it is at. The guarantee is the geometry, not the type -- which is
        // what a caller that matches on the variant has to know.
        let a = crate::borders::ShapeBorder::Circle(crate::borders::CircleBorder::new(
            crate::borders::BorderSide::NONE,
            0.0,
        ));
        let b = crate::borders::ShapeBorder::Stadium(crate::borders::StadiumBorder::new(
            crate::borders::BorderSide::NONE,
        ));
        let tween = ShapeBorderTween::new(Some(a), Some(b));
        let circularity = |t: f32| match tween.lerp(t) {
            Some(crate::borders::ShapeBorder::StadiumToCircle(shape)) => shape.circularity,
            other => panic!("expected the transition shape, got {other:?}"),
        };
        assert_eq!(circularity(0.0), 1.0, "fully the circle it started as");
        assert_eq!(circularity(1.0), 0.0, "fully the stadium it ends as");
        assert_eq!(circularity(0.5), 0.5);
    }

    #[test]
    fn a_settled_splash_still_reports_how_it_settled_through_the_feature_list() {
        let mut controller = MaterialInkController::default();
        controller.add_ink_feature(splash(0));
        if let InkFeature::Interactive(feature) = &mut controller.features[0] {
            feature.cancel(500);
        }
        match &controller.features()[0] {
            InkFeature::Interactive(feature) => {
                assert_eq!(feature.phase.settled, Some((500, InkSettlement::Cancelled)))
            }
            InkFeature::Decoration(_) => panic!("a splash was added"),
        }
    }

    #[test]
    fn the_two_typed_materials_take_two_different_theme_fields() {
        // `tools/unread_theme_fields.py` found `ThemeData::canvas_color`
        // reaching nothing: this mapped `Canvas` to the component theme's
        // `background` and `Card` to its `surface`, where upstream's switch
        // reads `theme.canvasColor` and `theme.cardColor`.
        //
        // Two distinct numbers, so a switch whose two arms named one field
        // answers with a colour that is not its own. `Canvas` is the default
        // type, which is why getting it wrong would have been the common
        // case rather than a corner.
        let theme = crate::theme::ThemeData {
            canvas_color: Color::argb(255, 0, 0, 11),
            card_color: Color::argb(255, 0, 0, 22),
            ..crate::theme::ThemeData::light()
        };
        let of_type = |material_type| {
            Material::new(leaf(|| crate::widgets::Empty))
                .with_type(material_type)
                .effective_color(&theme)
        };
        assert_eq!(
            of_type(MaterialType::Canvas),
            Some(Color::argb(255, 0, 0, 11))
        );
        assert_eq!(
            of_type(MaterialType::Card),
            Some(Color::argb(255, 0, 0, 22))
        );

        // And the default type is `Canvas`, so a material told nothing takes
        // the canvas colour.
        assert_eq!(
            Material::new(leaf(|| crate::widgets::Empty)).effective_color(&theme),
            Some(Color::argb(255, 0, 0, 11))
        );
    }
}
